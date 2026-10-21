//! The daemon's plugin vocabulary, as `plugin.*` reports it.
//!
//! Field names and optionality mirror herdr's own schema (protocol 22,
//! `schema_version` 1, dumped with `herdr api schema --json`). The daemon omits
//! an absent collection rather than sending it empty — a global action has no
//! `contexts` key at all, and `warnings` / `platforms` / `description` simply
//! vanish — so every collection here is `#[serde(default)]` and "absent" is
//! modelled rather than smoothed into "empty" where the two differ.

use serde::Deserialize;

/// One installed plugin, as `plugin.list` reports it. Actions and panes are the
/// manifest's declarations; the authoritative action list, with each action's
/// effective platforms, is `plugin.action.list` ([`PluginAction`]).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct InstalledPlugin {
    pub(crate) plugin_id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    /// Where the plugin lives on this machine. The daemon reports it because the
    /// host needs it: `<plugin_root>/zenix-ui.toml` is the UI the plugin
    /// declares for this client, which the daemon neither reads nor validates.
    pub(crate) plugin_root: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) panes: Vec<ManifestPane>,
    /// The manifest's `[[events]]` hooks: what the daemon runs when one of these
    /// events happens, and nothing else in the reply says which events an
    /// installed plugin is listening for.
    #[serde(default)]
    pub(crate) events: Vec<ManifestEventHook>,
    /// What the daemon found wrong at link or registry load: an undeclared
    /// platform list, an unknown event name. Non-fatal, and the only signal a
    /// user gets about either.
    #[serde(default)]
    pub(crate) warnings: Vec<String>,
}

impl InstalledPlugin {
    /// Whether this plugin runs something for `event`. The name is the daemon's
    /// dot name, which is what a manifest writes in `on` and what the log ring
    /// records against the run; a disabled plugin hooks nothing, because the
    /// daemon would not run it.
    pub(crate) fn hooks_on(&self, event: &str) -> bool {
        self.enabled && self.events.iter().any(|hook| hook.on == event)
    }
}

/// One `[[events]]` hook: the event it fires on. The daemon also reports the
/// command it would run, and this client does not read it: what a hook *would*
/// run is the trust review's subject before the plugin is linked, and what it
/// *did* run is in the log ring beside this.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ManifestEventHook {
    pub(crate) on: String,
}

/// One pane a plugin declares. Opening it is `plugin.pane.open`, and the
/// placement decides where the daemon puts it: an overlay or popup is chrome
/// over the grid, while split, tab and zoomed are ordinary layout.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ManifestPane {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) placement: Placement,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Placement {
    #[default]
    Overlay,
    Popup,
    Split,
    Tab,
    Zoomed,
}

impl Placement {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Overlay => "overlay",
            Self::Popup => "popup",
            Self::Split => "split",
            Self::Tab => "tab",
            Self::Zoomed => "zoomed",
        }
    }
}

/// One action, as `plugin.action.list` reports it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct PluginAction {
    pub(crate) plugin_id: String,
    pub(crate) action_id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    /// Which states the action wants to be offered in. The daemon omits the key
    /// for an action that declares no contexts, which means global — an empty
    /// vector is not the same answer, so this stays an `Option`.
    #[serde(default)]
    pub(crate) contexts: Option<Vec<Context>>,
}

impl PluginAction {
    /// The daemon's own globally unique name for the action, `plugin.id.action`.
    pub(crate) fn qualified_id(&self) -> String {
        format!("{}.{}", self.plugin_id, self.action_id)
    }

    /// Whether the current focus offers everything the action asks for. An
    /// action with no contexts, or with the global one, is always offered; a
    /// selection-scoped action is not, because zenix has no selection yet.
    pub(crate) fn is_available(&self, availability: Availability) -> bool {
        self.contexts
            .as_ref()
            .is_none_or(|contexts| availability.satisfies(contexts))
    }
}

/// One context an action may name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Context {
    Global,
    Workspace,
    Tab,
    Pane,
    Selection,
}

impl Context {
    /// The name the daemon uses, for the row that shows what an action wants.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Workspace => "workspace",
            Self::Tab => "tab",
            Self::Pane => "pane",
            Self::Selection => "selection",
        }
    }
}

/// What the focused state can satisfy right now, read from the daemon's
/// snapshot. This is the host's own decision: the daemon lists every action
/// regardless of where it would be offered, including ones for a platform it is
/// not running on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Availability {
    pub(crate) workspace: bool,
    pub(crate) tab: bool,
    pub(crate) pane: bool,
    /// zenix has no terminal selection yet, so this is never true; it is a
    /// field rather than a constant so the day selection lands, only the
    /// collector changes.
    pub(crate) selection: bool,
}

impl Availability {
    /// Whether the focus satisfies any of these contexts. An empty list is
    /// satisfied by everything, which is what a declaration that names no
    /// context means.
    pub(crate) fn satisfies(self, contexts: &[Context]) -> bool {
        contexts.is_empty() || contexts.iter().any(|context| self.has(*context))
    }

    fn has(self, context: Context) -> bool {
        match context {
            Context::Global => true,
            Context::Workspace => self.workspace,
            Context::Tab => self.tab,
            Context::Pane => self.pane,
            Context::Selection => self.selection,
        }
    }

    /// What the daemon's snapshot offers: a focused workspace, its tab and its
    /// pane. A snapshot that has not arrived yet offers nothing, which is the
    /// truth — no command can be addressed without one.
    pub(crate) fn of(snapshot: Option<&herdr_client::protocol::ClientShellSnapshot>) -> Self {
        let Some(snapshot) = snapshot else {
            return Self::default();
        };
        Self {
            workspace: snapshot.focused_workspace_id.is_some(),
            tab: snapshot.focused_tab_id.is_some(),
            pane: snapshot.focused_pane_id.is_some(),
            selection: false,
        }
    }
}

/// One command the daemon ran for a plugin: an action, an event hook or a
/// startup hook. There is no completion push, so a log entry is the only place
/// an invocation's outcome appears.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PluginCommandLog {
    pub(crate) log_id: String,
    pub(crate) plugin_id: String,
    #[serde(default)]
    pub(crate) action_id: Option<String>,
    #[serde(default)]
    pub(crate) event: Option<String>,
    #[serde(default)]
    pub(crate) command: Vec<String>,
    pub(crate) status: LogStatus,
    #[serde(default)]
    pub(crate) exit_code: Option<i32>,
    #[serde(default)]
    pub(crate) stdout: Option<String>,
    #[serde(default)]
    pub(crate) stderr: Option<String>,
    /// Why the daemon could not spawn the command at all, as opposed to what the
    /// command itself printed.
    #[serde(default)]
    pub(crate) error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LogStatus {
    Running,
    Succeeded,
    Failed,
}

impl LogStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

impl PluginCommandLog {
    /// What the daemon ran, as one line: the action, the event hook, or the
    /// startup hook that has neither.
    pub(crate) fn what(&self) -> String {
        match (self.action_id.as_deref(), self.event.as_deref()) {
            (Some(action), _) => format!("{}/{action}", self.plugin_id),
            (None, Some(event)) => format!("{}: {event}", self.plugin_id),
            (None, None) => self.plugin_id.clone(),
        }
    }

    /// The outcome line: the status, the exit code while there is one, and
    /// whatever the command last said. Only the first line is shown — a plugin's
    /// whole output belongs in a log view of its own, not in a sidebar row.
    pub(crate) fn outcome(&self) -> String {
        let mut parts = vec![self.status.label().to_owned()];
        if let Some(code) = self.exit_code {
            parts.push(format!("exit {code}"));
        }
        if let Some(error) = self.error.as_deref().filter(|error| !error.is_empty()) {
            return format!("{}: {error}", parts.join(" "));
        }
        let stream = if self.status == LogStatus::Failed {
            self.stderr.as_deref().or(self.stdout.as_deref())
        } else {
            self.stdout.as_deref().or(self.stderr.as_deref())
        };
        if let Some(text) = stream.and_then(first_line).filter(|line| !line.is_empty()) {
            parts.push(text.to_owned());
        }
        parts.join(" · ")
    }
}

fn first_line(text: &str) -> Option<&str> {
    text.lines().next().map(str::trim_end)
}

/// A plugin pane this client opened. A pane carries no ownership marker on
/// either lane — `PluginPaneInfo.pane` is an ordinary `PaneInfo`, and the
/// snapshot shows only a label and a cwd — so the only way to know which panes
/// the plugin surface owns is to remember the opens this client made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnedPane {
    pub(crate) pane_id: String,
    pub(crate) plugin_id: String,
    pub(crate) entrypoint: String,
    pub(crate) title: String,
}

/// What `plugin.pane.open` answers with. Only the pane id is used: the rest of
/// the `PaneInfo` arrives in the snapshot anyway.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct OpenedPane {
    pub(crate) pane: PaneId,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PaneId {
    pub(crate) pane_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(contexts: Option<Vec<Context>>) -> PluginAction {
        PluginAction {
            plugin_id: "example.board".into(),
            action_id: "apply".into(),
            title: "Apply layout".into(),
            description: None,
            contexts,
        }
    }

    #[test]
    fn an_action_without_contexts_is_global_and_always_offered() {
        let global = action(None);
        assert!(global.is_available(Availability::default()));
        assert!(action(Some(vec![])).is_available(Availability::default()));
        assert!(action(Some(vec![Context::Global])).is_available(Availability::default()));
    }

    #[test]
    fn a_scoped_action_follows_the_focused_state() {
        let workspace = action(Some(vec![Context::Workspace]));
        assert!(!workspace.is_available(Availability::default()));
        assert!(workspace.is_available(Availability {
            workspace: true,
            ..Availability::default()
        }));
    }

    /// An action asking for either pane or selection is offered while there is a
    /// focused pane, which is what the daemon's own context merge would do with
    /// it too.
    #[test]
    fn a_scoped_action_is_offered_when_any_of_its_contexts_is_satisfied() {
        let either = action(Some(vec![Context::Pane, Context::Selection]));
        assert!(either.is_available(Availability {
            pane: true,
            ..Availability::default()
        }));
        // No selection support in zenix yet, so pane alone is what carries it.
        let selection_only = action(Some(vec![Context::Selection]));
        assert!(!selection_only.is_available(Availability {
            pane: true,
            ..Availability::default()
        }));
    }

    #[test]
    fn availability_comes_from_the_focused_ids() {
        let snapshot: herdr_client::protocol::ClientShellSnapshot =
            serde_json::from_value(serde_json::json!({
                "boot_id": "boot",
                "revision": 1,
                "update_install_command": "",
                "latest_release_notes_available": false,
                "integration_updates_available": false,
                "worktree_directory": "",
                "tab_bar_right_separator": "",
                "tab_bar_right": [],
                "agent_order": [],
                "focused_workspace_id": "w1",
                "focused_tab_id": "w1:t1",
                "focused_pane_id": "w1:p1",
                "workspaces": [],
                "tabs": [],
                "panes": [],
                "agents": [],
                "commands": [],
            }))
            .expect("the fixture snapshot deserializes");
        let availability = Availability::of(Some(&snapshot));
        assert!(availability.workspace && availability.tab && availability.pane);
        assert!(!availability.selection, "zenix has no selection yet");
        assert_eq!(Availability::of(None), Availability::default());
    }

    #[test]
    fn an_absent_contexts_key_decodes_as_global_and_an_empty_one_as_empty() {
        let one: PluginAction = serde_json::from_value(serde_json::json!({
            "plugin_id": "a.b", "action_id": "x", "title": "X", "command": ["true"]
        }))
        .expect("decodes without contexts");
        assert_eq!(one.contexts, None);
        let two: PluginAction = serde_json::from_value(serde_json::json!({
            "plugin_id": "a.b", "action_id": "x", "title": "X", "contexts": [], "command": ["true"]
        }))
        .expect("decodes an empty contexts list");
        assert_eq!(two.contexts, Some(vec![]));
    }

    #[test]
    fn the_qualified_id_is_the_daemons_own_name() {
        assert_eq!(action(None).qualified_id(), "example.board.apply");
    }

    #[test]
    fn a_plugin_decodes_with_every_optional_collection_absent() {
        let plugin: InstalledPlugin = serde_json::from_value(serde_json::json!({
            "plugin_id": "zenix.m8probe",
            "name": "Zenix M8 Probe",
            "version": "0.1.0",
            "manifest_path": "/tmp/plugin/herdr-plugin.toml",
            "plugin_root": "/tmp/plugin",
            "enabled": true,
            "source": {"kind": "local"},
        }))
        .expect("decodes a plugin that declares no panes or warnings");
        assert!(plugin.panes.is_empty());
        assert!(plugin.warnings.is_empty());
        assert!(plugin.enabled);
        assert_eq!(plugin.plugin_root, "/tmp/plugin");
    }

    #[test]
    fn a_plugin_decodes_the_event_hooks_its_manifest_declares() {
        let plugin: InstalledPlugin = serde_json::from_value(serde_json::json!({
            "plugin_id": "zenix.m8probe",
            "name": "Zenix M8 Probe",
            "version": "0.1.0",
            "manifest_path": "/tmp/plugin/herdr-plugin.toml",
            "plugin_root": "/tmp/plugin",
            "enabled": true,
            "events": [{"on": "pane.exited", "command": ["sh", "hook.sh"]}],
        }))
        .expect("decodes a plugin that hooks an event");
        assert_eq!(plugin.events.len(), 1);
        assert_eq!(plugin.events[0].on, "pane.exited");
        assert!(plugin.hooks_on("pane.exited"));
        assert!(!plugin.hooks_on("pane.created"));
    }

    /// The daemon runs a disabled plugin's hooks for nobody, so a row that
    /// claimed one would be naming a hook that never fires.
    #[test]
    fn a_disabled_plugin_hooks_nothing() {
        let plugin: InstalledPlugin = serde_json::from_value(serde_json::json!({
            "plugin_id": "zenix.m8probe",
            "name": "Zenix M8 Probe",
            "version": "0.1.0",
            "manifest_path": "/tmp/plugin/herdr-plugin.toml",
            "plugin_root": "/tmp/plugin",
            "enabled": false,
            "events": [{"on": "pane.exited", "command": ["sh", "hook.sh"]}],
        }))
        .expect("decodes a disabled plugin");
        assert!(!plugin.hooks_on("pane.exited"));
    }

    #[test]
    fn placements_round_trip_through_the_daemons_spelling() {
        assert_eq!(Placement::default(), Placement::Overlay);
        assert_eq!(Placement::Split.as_str(), "split");
    }

    #[test]
    fn a_log_outcome_leads_with_its_status_and_the_lines_that_matter() {
        let failed: PluginCommandLog = serde_json::from_value(serde_json::json!({
            "log_id": "plugin-log-5", "plugin_id": "zenix.m8probe", "action_id": "fail",
            "command": ["sh", "fail.sh"], "status": "failed",
            "exit_code": 7, "stdout": "before\n", "stderr": "deliberate failure\n",
        }))
        .expect("decodes a failed log");
        assert_eq!(failed.what(), "zenix.m8probe/fail");
        assert_eq!(failed.outcome(), "failed · exit 7 · deliberate failure");

        // A spawn failure has no exit code and no output: the daemon's reason is
        // the only thing worth showing.
        let mut spawn_failure = failed.clone();
        spawn_failure.status = LogStatus::Running;
        spawn_failure.exit_code = None;
        spawn_failure.stdout = None;
        spawn_failure.stderr = None;
        spawn_failure.error = Some("no such file".into());
        assert_eq!(spawn_failure.outcome(), "running: no such file");
    }

    #[test]
    fn a_startup_hook_log_names_the_plugin_because_it_has_neither_action_nor_event() {
        let log: PluginCommandLog = serde_json::from_value(serde_json::json!({
            "log_id": "plugin-log-1", "plugin_id": "zenix.m8probe", "event": "startup",
            "command": ["sh", "-c", "true"], "status": "succeeded",
            "exit_code": 0, "stdout": "started\n",
        }))
        .expect("decodes a startup log");
        assert_eq!(log.what(), "zenix.m8probe: startup");
        assert_eq!(log.outcome(), "succeeded · exit 0 · started");
    }
}
