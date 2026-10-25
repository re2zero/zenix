//! The daemon's worktrees, as `worktree.list` reports them.
//!
//! A worktree is how an agent gets its own desk: one checkout per task, opened
//! as an ordinary workspace the moment it is needed. The daemon owns them all —
//! it discovers them with `git worktree list` on a background thread, opens one
//! as a workspace on `worktree.open`, and creates one on `worktree.create` —
//! and announces every change as `worktree.created` / `worktree.opened` /
//! `worktree.removed`, which arrive on the event stream this client already
//! reads. So the cache here is refreshed when the daemon says something moved
//! and when the focused workspace changes the repo the list is scoped to, and
//! never on a timer.
//!
//! Every read is a blocking one-shot call on the API lane — the daemon runs its
//! discovery off its own request path, so a call may take a moment — and it
//! runs on this module's worker thread: never on the UI thread, and never on
//! the endpoint connection that owns the paint state. The shell learns about a
//! change from a pulse and reads the cache, the same shape the plugin host
//! takes.
//!
//! Verified against the daemon 0.9.5 (`vendor/herdr-pins.tsv`):
//!
//! * `src/api/schema/worktrees.rs` is the wire shape: `WorktreeInfo` carries
//!   `path`, `branch`, `label`, the bare / detached / prunable /
//!   linked-worktree flags, and `open_workspace_id` — the workspace the
//!   worktree is already open as, which is what makes a row a focus rather
//!   than an open.
//! * `worktree.open` takes exactly one of `path` or `branch`
//!   (`src/app/api/worktrees/reads.rs:151`); a focus asks the daemon to switch
//!   to the workspace it opens or finds, and an already-open worktree is
//!   focused rather than opened twice (`finish_worktree_open`).
//! * `worktree.create` answers synchronously with the created workspace; the
//!   heavy lifting is the daemon's own `WorktreeAdd` app event.
//! * `trust_repository` is left false everywhere: the client does not decide
//!   which repositories are safe to run git in, and an untrusted repository's
//!   error is shown rather than worked around.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use crossbeam_channel::{Receiver, Sender};
use serde::Deserialize;

use crate::herdr::api::ApiClient;

/// One worktree the daemon discovered, as the sidebar's block draws it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct WorktreeEntry {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) branch: Option<String>,
    /// The name a human reads. The daemon fills it from the branch, so a
    /// detached worktree is the one place `label` says something `branch`
    /// cannot.
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) is_bare: bool,
    #[serde(default)]
    pub(crate) is_detached: bool,
    #[serde(default)]
    pub(crate) is_prunable: bool,
    #[serde(default)]
    pub(crate) is_linked_worktree: bool,
    /// The workspace this worktree is open as, when it is open. A row that
    /// carries one focuses that workspace; a row that does not asks the daemon
    /// to open it.
    #[serde(default)]
    pub(crate) open_workspace_id: Option<String>,
}

impl WorktreeEntry {
    /// What the row shows: the branch when there is one, the daemon's label
    /// when there is not.
    pub(crate) fn name(&self) -> &str {
        self.branch.as_deref().unwrap_or(&self.label)
    }

    /// Whether the row can be acted on at all: a bare repository is not a
    /// checkout, and a prunable one has lost its directory — the daemon
    /// refuses both on `worktree.open`.
    pub(crate) fn openable(&self) -> bool {
        !self.is_bare && !self.is_prunable
    }
}

/// The repository the list belongs to, as `worktree.list` reports it. One
/// block is one repo's worktrees; the name says which.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct WorktreeSource {
    #[serde(default)]
    pub(crate) repo_name: String,
    #[serde(default)]
    pub(crate) repo_root: String,
}

/// Everything the shell knows about worktrees, as one value to read per paint.
#[derive(Clone, Debug, Default)]
pub(crate) struct WorktreeState {
    pub(crate) entries: Vec<WorktreeEntry>,
    pub(crate) source: Option<WorktreeSource>,
    /// The workspace id the list was asked for. A list that outlives its scope
    /// is shown until the fresh one lands — the shell re-asks the moment the
    /// focus moves — but the block can tell the two apart with this.
    pub(crate) scope: Option<String>,
    /// Why the last refresh failed, until one succeeds. `not_git_worktree` and
    /// the untrusted-repository refusals land here, and the block says them.
    pub(crate) error: Option<String>,
    /// The last thing this client asked the daemon to do, and how it answered.
    pub(crate) last: Option<String>,
}

/// The worktree host: one worker thread, one cache, one pulse channel.
pub(crate) struct WorktreeHost {
    cache: Arc<Mutex<WorktreeState>>,
    commands: Sender<Request>,
}

impl WorktreeHost {
    /// Start the host and its worker. The returned receiver pulses once per
    /// state change, so the shell repaints on a change instead of polling.
    pub(crate) fn start() -> (Self, Receiver<()>) {
        let cache = Arc::new(Mutex::new(WorktreeState::default()));
        let (commands, requests) = crossbeam_channel::unbounded();
        // Bounded at one: the shell only needs to know that something moved
        // since its last read, so a burst of changes coalesces into one wakeup.
        let (pulses_out, pulses) = crossbeam_channel::bounded(1);
        let worker_cache = cache.clone();
        thread::Builder::new()
            .name("zenix-worktrees".into())
            .spawn(move || work(ApiClient::local(), worker_cache, requests, pulses_out))
            .expect("failed to spawn the worktree worker thread");
        let host = Self { cache, commands };
        (host, pulses)
    }

    /// A copy of the cache for one paint.
    pub(crate) fn state(&self) -> WorktreeState {
        lock(&self.cache).clone()
    }

    /// Re-read the focused workspace's repo worktrees. `None` asks the daemon
    /// for its own active workspace's repo, which is the right scope before a
    /// first snapshot has said which workspace is focused.
    pub(crate) fn refresh(&self, scope: Option<String>) {
        let _ = self.commands.send(Request::Refresh { scope });
    }

    /// Open one of the listed worktrees as a workspace, focused. A worktree
    /// that is already open is focused rather than opened twice — the daemon
    /// does that on its side, and the answer says which happened.
    pub(crate) fn open(&self, scope: String, target: WorktreeTarget) {
        let _ = self.commands.send(Request::Open {
            scope,
            target: target.into_params(),
        });
    }

    /// Create a worktree from the focused workspace's repo, with the daemon's
    /// own naming and focused on arrival. A branch name is the daemon's to
    /// generate until this client has a place to ask for one.
    pub(crate) fn create(&self, scope: String) {
        let _ = self.commands.send(Request::Create { scope });
    }
}

/// What a row's click asks for, resolved before the request leaves the UI
/// thread so the worker sends exactly one of the two the daemon accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorktreeTarget {
    Branch(String),
    Path(String),
}

impl WorktreeTarget {
    fn into_params(self) -> serde_json::Value {
        match self {
            Self::Branch(branch) => serde_json::json!({ "branch": branch, "focus": true }),
            Self::Path(path) => serde_json::json!({ "path": path, "focus": true }),
        }
    }
}

/// One unit of work for the worker thread.
enum Request {
    Refresh {
        scope: Option<String>,
    },
    Open {
        scope: String,
        target: serde_json::Value,
    },
    Create {
        scope: String,
    },
}

fn work(
    client: ApiClient,
    cache: Arc<Mutex<WorktreeState>>,
    commands: Receiver<Request>,
    pulses: Sender<()>,
) {
    // No fallback tick: the daemon announces every worktree change on the
    // event stream, and the shell re-asks on a focus change — there is no
    // change this side can miss by waiting.
    while let Ok(request) = commands.recv() {
        handle(&client, &cache, request);
        let _ = pulses.try_send(());
    }
}

fn handle(client: &ApiClient, cache: &Mutex<WorktreeState>, request: Request) {
    match request {
        Request::Refresh { scope } => refresh(client, cache, scope),
        Request::Open { scope, target } => {
            let mut params = target;
            params["workspace_id"] = serde_json::json!(scope);
            match client.call("worktree.open", params) {
                Ok(answer) => {
                    let reopened = answer
                        .get("already_open")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    note(
                        cache,
                        if reopened {
                            "worktree: focused".to_owned()
                        } else {
                            "worktree: opened".to_owned()
                        },
                    );
                    refresh(client, cache, Some(scope));
                }
                Err(error) => note(cache, format!("the worktree was not opened: {error}")),
            }
        }
        Request::Create { scope } => {
            let params = serde_json::json!({ "workspace_id": scope, "focus": true });
            match client.call("worktree.create", params) {
                Ok(answer) => {
                    let label = answer
                        .pointer("/worktree/label")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("worktree");
                    note(cache, format!("{label}: created"));
                    refresh(client, cache, Some(scope));
                }
                Err(error) => note(cache, format!("the worktree was not created: {error}")),
            }
        }
    }
}

fn refresh(client: &ApiClient, cache: &Mutex<WorktreeState>, scope: Option<String>) {
    // Exactly one of workspace_id or cwd may be supplied, and neither is also
    // an answer: the daemon falls back to its own active workspace.
    let params = match &scope {
        Some(workspace_id) => serde_json::json!({ "workspace_id": workspace_id }),
        None => serde_json::json!({}),
    };
    match client.call("worktree.list", params) {
        Ok(answer) => {
            let entries = answer
                .get("worktrees")
                .map(|value| {
                    serde_json::from_value::<Vec<WorktreeEntry>>(value.clone()).unwrap_or_default()
                })
                .unwrap_or_default();
            let source = answer
                .get("source")
                .and_then(|value| serde_json::from_value::<WorktreeSource>(value.clone()).ok());
            let mut state = lock(cache);
            state.entries = entries;
            state.source = source;
            state.scope = scope;
            state.error = None;
        }
        // The list could not be read — the workspace's cwd is not a repository,
        // the repository is not trusted, the daemon is gone. Keep the last
        // working answer and say why it is the last one.
        Err(error) => {
            let mut state = lock(cache);
            state.error = Some(error.to_string());
            state.scope = scope;
        }
    }
}

fn note(cache: &Mutex<WorktreeState>, line: String) {
    tracing::info!(target: "zenix-worktrees", "{line}");
    lock(cache).last = Some(line);
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    /// One row of `worktree.list` as the daemon serializes it, absent flags
    /// omitted the way `skip_serializing_if` leaves them.
    #[test]
    fn a_worktree_entry_decodes_the_daemons_report() {
        let entry: WorktreeEntry = serde_json::from_value(serde_json::json!({
            "path": "/tmp/zenix-m17",
            "branch": "zenix-m17",
            "label": "zenix-m17",
            "is_linked_worktree": true,
            "open_workspace_id": "w123",
        }))
        .expect("decodes a linked, open worktree");
        assert_eq!(entry.name(), "zenix-m17");
        assert_eq!(entry.open_workspace_id.as_deref(), Some("w123"));
        assert!(entry.openable());

        let detached: WorktreeEntry = serde_json::from_value(serde_json::json!({
            "path": "/repo",
            "label": "repo",
            "is_detached": true,
        }))
        .expect("decodes a detached worktree with no branch");
        assert_eq!(detached.name(), "repo", "the label fills in for a branch");
        assert!(detached.open_workspace_id.is_none());
    }

    /// A bare repository and a prunable worktree are rows the daemon refuses
    /// to open, so the block draws them as facts rather than as clicks.
    #[test]
    fn a_bare_or_prunable_worktree_is_not_openable() {
        let bare: WorktreeEntry = serde_json::from_value(serde_json::json!({
            "path": "/repo", "label": "repo", "is_bare": true,
        }))
        .expect("decodes a bare entry");
        let prunable: WorktreeEntry = serde_json::from_value(serde_json::json!({
            "path": "/gone", "label": "gone", "is_prunable": true,
        }))
        .expect("decodes a prunable entry");
        assert!(!bare.openable());
        assert!(!prunable.openable());
    }

    /// The list the daemon answers with decodes whole: source and entries, in
    /// the order `git worktree list` reports them.
    #[test]
    fn the_list_answer_decodes_its_source_and_entries() {
        let answer = serde_json::json!({
            "source": {
                "repo_key": "k", "repo_name": "zenix",
                "repo_root": "/src/zenix", "source_checkout_path": "/src/zenix",
            },
            "worktrees": [
                {"path": "/src/zenix", "branch": "main", "label": "main",
                 "open_workspace_id": "w1"},
                {"path": "/tmp/zenix-m17", "branch": "zenix-m17",
                 "label": "zenix-m17", "is_linked_worktree": true},
            ],
        });
        let source: Option<WorktreeSource> = answer
            .get("source")
            .and_then(|value| serde_json::from_value(value.clone()).ok());
        let entries: Vec<WorktreeEntry> =
            serde_json::from_value(answer.get("worktrees").cloned().unwrap_or_default())
                .expect("decodes the entries");
        assert_eq!(source.expect("decodes the source").repo_name, "zenix");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].open_workspace_id.as_deref(), Some("w1"));
        assert!(entries[1].open_workspace_id.is_none());
    }

    /// A row's click resolves to exactly one of the two the daemon accepts
    /// before the request leaves the UI thread.
    #[test]
    fn a_target_is_one_of_branch_or_path_and_always_focuses() {
        let branch = serde_json::json!({ "branch": "zenix-m17", "focus": true });
        assert_eq!(
            WorktreeTarget::Branch("zenix-m17".into()).into_params(),
            branch
        );
        assert_eq!(
            WorktreeTarget::Path("/tmp/wt".into()).into_params(),
            serde_json::json!({ "path": "/tmp/wt", "focus": true })
        );
        assert!(branch.get("branch").is_some() && branch.get("path").is_none());
    }
}
