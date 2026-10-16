//! The data behind a declared contribution: one command's stdout, parsed as
//! JSON, read off the UI thread and with a deadline.
//!
//! A declaration names a command, not a poll interval. The host therefore runs
//! one when it has a reason to — the first time a declaration appears, or when
//! a user asks for a refresh — and keeps the last answer for every paint after
//! that. Nothing here runs on the UI thread: the command is another process, and
//! a plugin that hangs must not take the window with it.
//!
//! The command is run with `HERDR_PLUGIN_CONTEXT_JSON`, the same variable and
//! the same JSON shape the daemon sets for a plugin action, so an author writes
//! one contract and both paths honour it.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use herdr_client::protocol::ClientShellSnapshot;
use serde_json::{Map, Value};

use super::ui_manifest::{Declared, UiContributions};

/// How long a declared command may run before it is stopped and reported as a
/// timeout. A panel that never answers is worse than one that says it gave up.
const DEADLINE: Duration = Duration::from_secs(5);

/// How often a running command is asked whether it is done.
const POLL: Duration = Duration::from_millis(25);

/// What a declaration's data is filed under: the contribution's own key, the
/// command to read for it, and the directory to read it from — the plugin's own
/// root, which is where the daemon runs a plugin's commands from too.
pub(crate) struct Load {
    pub(crate) id: String,
    pub(crate) command: Vec<String>,
    pub(crate) cwd: PathBuf,
}

/// The last read of one declaration's data, as a paint finds it.
#[derive(Clone, Debug)]
pub(crate) enum Data {
    /// Asked for, not answered yet. Shown as such rather than as an empty panel,
    /// because "still reading" and "read, and empty" are different answers.
    Loading,
    /// The command's stdout, as JSON. The view decides what to make of the shape.
    Ready(Arc<Value>),
    /// What went wrong: a command that could not start, exited non-zero, ran past
    /// the deadline, or printed something that is not JSON.
    Failed(String),
}

/// Every declaration's data, as one value to read per paint.
#[derive(Clone, Debug, Default)]
pub(crate) struct UiData {
    entries: HashMap<String, Data>,
}

impl UiData {
    pub(crate) fn get(&self, id: &str) -> Option<&Data> {
        self.entries.get(id)
    }
}

/// The reader: one worker thread, one cache, one pulse channel.
pub(crate) struct UiDataHost {
    cache: Arc<Mutex<UiData>>,
    commands: Sender<Request>,
}

impl UiDataHost {
    /// Start the reader and its worker. The returned receiver pulses once per
    /// answer, so the shell repaints when data arrives instead of polling.
    pub(crate) fn start() -> (Self, Receiver<()>) {
        let cache = Arc::new(Mutex::new(UiData::default()));
        let (commands, requests) = crossbeam_channel::unbounded();
        // Bounded at one: a burst of answers coalesces into a single repaint.
        let (pulses_out, pulses) = crossbeam_channel::bounded(1);
        let worker_cache = cache.clone();
        thread::Builder::new()
            .name("zenix-plugin-ui-data".into())
            .spawn(move || work(worker_cache, requests, pulses_out))
            .expect("failed to spawn the plugin UI data thread");
        (Self { cache, commands }, pulses)
    }

    /// A copy of every answer for one paint.
    pub(crate) fn state(&self) -> UiData {
        lock(&self.cache).clone()
    }

    /// Ask for the data of every declaration that has none yet, or of all of
    /// them when `force` is set. A declaration whose read is already in flight is
    /// not asked again, so a repaint cannot multiply a plugin's commands.
    pub(crate) fn sync(&self, loads: Vec<Load>, context: Arc<Value>, force: bool) {
        let _ = self.commands.send(Request::Load {
            loads,
            context,
            force,
        });
    }
}

enum Request {
    Load {
        loads: Vec<Load>,
        context: Arc<Value>,
        force: bool,
    },
}

fn work(cache: Arc<Mutex<UiData>>, requests: Receiver<Request>, pulses: Sender<()>) {
    while let Ok(Request::Load {
        loads,
        context,
        force,
    }) = requests.recv()
    {
        let pending: Vec<Load> = {
            let mut state = lock(&cache);
            if force {
                state.entries.clear();
            }
            loads
                .into_iter()
                .filter(|load| !state.entries.contains_key(&load.id))
                .collect()
        };
        if pending.is_empty() {
            continue;
        }
        {
            let mut state = lock(&cache);
            for load in &pending {
                state.entries.insert(load.id.clone(), Data::Loading);
            }
        }
        let _ = pulses.try_send(());
        for load in pending {
            let data = read(&load, &context);
            lock(&cache).entries.insert(load.id, data);
            let _ = pulses.try_send(());
        }
    }
}

/// Run one declaration's command and read its answer.
fn read(load: &Load, context: &Value) -> Data {
    let Some((program, arguments)) = load.command.split_first() else {
        return Data::Failed("the declaration names no command".to_owned());
    };
    let mut command = Command::new(resolve_program(program, &load.cwd));
    command
        .args(arguments)
        .current_dir(&load.cwd)
        .env("HERDR_PLUGIN_CONTEXT_JSON", context.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A command gets its own process group, so stopping it stops whatever it
    // started: a plugin that spawned a helper must not leave it behind on every
    // refresh.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return Data::Failed(format!("{program} could not be started: {error}")),
    };
    // Both pipes are drained on their own threads: a command that writes more
    // than a pipe holds would otherwise block until the deadline, and look like a
    // command that hung.
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < DEADLINE => thread::sleep(POLL),
            // Out of time: stop it and say so, rather than waiting for a plugin
            // that may never answer. The drain threads are left to finish on
            // their own — the writer is gone, so they end as soon as the last
            // holder of the pipe closes it.
            Ok(None) => {
                stop(&mut child);
                return Data::Failed(format!(
                    "{program} did not finish within {}s and was stopped",
                    DEADLINE.as_secs()
                ));
            }
            Err(error) => {
                return Data::Failed(format!("{program} could not be waited for: {error}"));
            }
        }
    };
    let stdout = stdout
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let stderr = stderr
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    match status.code() {
        Some(0) => {}
        Some(code) => {
            let why = first_line(&stderr).or_else(|| first_line(&stdout));
            return match why {
                Some(why) => Data::Failed(format!("{program} exited {code}: {why}")),
                None => Data::Failed(format!("{program} exited {code}")),
            };
        }
        None => return Data::Failed(format!("{program} was killed by a signal")),
    }
    match serde_json::from_slice::<Value>(&stdout) {
        Ok(value) => Data::Ready(Arc::new(value)),
        Err(error) => Data::Failed(format!("{program}'s stdout is not JSON: {error}")),
    }
}

/// Stop a command that ran out of time, and everything it started with it.
fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The command leads its own process group, whose id is its pid.
        let _ = Command::new("kill")
            .args(["-9", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// A command naming a path beside the manifest — `./bin/tool` — is resolved
/// against the plugin's root, the way the daemon resolves a plugin's own
/// commands (herdr's `plugin_command::program_for_cwd`). A bare name is left to
/// `PATH`.
fn resolve_program(program: &str, cwd: &Path) -> PathBuf {
    let path = Path::new(program);
    if path.is_relative() && program.contains('/') {
        return cwd.join(path.strip_prefix(".").unwrap_or(path));
    }
    path.to_path_buf()
}

fn drain(mut stream: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stream.read_to_end(&mut buffer);
        buffer
    })
}

fn first_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.to_owned())
}

/// The context a declared contribution is run with: the fields the snapshot
/// knows, under the daemon's own names, plus the one field only the host can
/// name. A focus the snapshot does not report stays out rather than being
/// invented (design §3).
pub(crate) fn context(snapshot: Option<&ClientShellSnapshot>) -> Value {
    let mut context = Map::new();
    context.insert(
        "invocation_source".to_owned(),
        Value::String("zenix_ui".to_owned()),
    );
    let Some(snapshot) = snapshot else {
        return Value::Object(context);
    };
    if let Some(id) = snapshot.focused_workspace_id.as_deref() {
        context.insert("workspace_id".to_owned(), Value::String(id.to_owned()));
        if let Some(workspace) = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == id)
        {
            context.insert(
                "workspace_label".to_owned(),
                Value::String(workspace.label.clone()),
            );
            if !workspace.new_workspace_cwd.is_empty() {
                context.insert(
                    "workspace_cwd".to_owned(),
                    Value::String(workspace.new_workspace_cwd.clone()),
                );
            }
        }
    }
    if let Some(id) = snapshot.focused_tab_id.as_deref() {
        context.insert("tab_id".to_owned(), Value::String(id.to_owned()));
    }
    if let Some(id) = snapshot.focused_pane_id.as_deref() {
        context.insert("focused_pane_id".to_owned(), Value::String(id.to_owned()));
        if let Some(cwd) = snapshot
            .panes
            .iter()
            .find(|pane| pane.pane_id == id)
            .and_then(|pane| pane.cwd.as_deref())
            .filter(|cwd| !cwd.is_empty())
        {
            context.insert("focused_pane_cwd".to_owned(), Value::String(cwd.to_owned()));
        }
    }
    Value::Object(context)
}

/// Every source the mounted contributions name, in the order they mount.
pub(crate) fn loads(ui: &UiContributions) -> Vec<Load> {
    let mut loads = Vec::new();
    loads.extend(ui.panels.iter().filter_map(|panel| {
        panel.source.as_ref().map(|source| Load {
            id: panel.qualified_id(),
            command: source.command.clone(),
            cwd: panel.plugin_root.clone(),
        })
    }));
    loads.extend(ui.status_items.iter().filter_map(|item| {
        item.source.as_ref().map(|source| Load {
            id: item.qualified_id(),
            command: source.command.clone(),
            cwd: item.plugin_root.clone(),
        })
    }));
    loads
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    use super::super::ui_manifest::UiContributions;

    fn read_json(id: &str, command: &[&str]) -> Data {
        read_in(id, command, Path::new("/tmp"))
    }

    fn read_in(id: &str, command: &[&str], cwd: &Path) -> Data {
        let load = Load {
            id: id.to_owned(),
            command: command.iter().map(|part| (*part).to_owned()).collect(),
            cwd: cwd.to_path_buf(),
        };
        read(&load, &Value::Object(Map::new()))
    }

    #[test]
    fn a_command_that_prints_json_is_read_as_json() {
        match read_json("x", &["sh", "-c", "echo '{\"count\": 3}'"]) {
            Data::Ready(value) => assert_eq!(value["count"], 3),
            other => panic!("expected ready data, got {other:?}"),
        }
    }

    /// Every failure has to be sayable in a panel: the reason is the whole
    /// difference between a plugin that is broken and one that is slow.
    #[test]
    fn a_non_zero_exit_reports_the_code_and_what_it_said() {
        match read_json("x", &["sh", "-c", "echo broken 1>&2; exit 3"]) {
            Data::Failed(why) => {
                assert!(why.contains("exited 3"), "{why}");
                assert!(why.contains("broken"), "{why}");
            }
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn stdout_that_is_not_json_is_a_failure_not_an_empty_panel() {
        match read_json("x", &["sh", "-c", "echo not json"]) {
            Data::Failed(why) => assert!(why.contains("not JSON"), "{why}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_command_that_does_not_exist_is_a_failure() {
        match read_json("x", &["definitely-not-a-program"]) {
            Data::Failed(why) => assert!(why.contains("could not be started"), "{why}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_declaration_without_a_command_is_a_failure() {
        match read_json("x", &[]) {
            Data::Failed(why) => assert!(why.contains("names no command"), "{why}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    /// A command that hangs is stopped at the deadline. This is the one test
    /// that spends the whole deadline, so it is the only one that is slow.
    #[test]
    fn a_command_that_hangs_is_stopped_at_the_deadline() {
        let started = Instant::now();
        match read_json("x", &["sh", "-c", "sleep 30"]) {
            Data::Failed(why) => assert!(why.contains("did not finish"), "{why}"),
            other => panic!("expected a timeout, got {other:?}"),
        }
        assert!(started.elapsed() >= DEADLINE, "the deadline was honoured");
    }

    #[test]
    fn loads_cover_panels_and_status_items_in_mount_order() {
        let ui = UiContributions::parse(
            "a.b",
            std::path::Path::new("/tmp/zenix-ui-root"),
            r#"
[[panel]]
id = "board"
title = "Board"
[panel.source]
command = ["sh", "board.sh"]

[[status_item]]
id = "wip"
format = "{count} WIP"
source = { command = ["sh", "wip.sh"] }

[[panel]]
id = "quiet"
title = "Quiet"
"#,
        );
        let loads = loads(&ui);
        let ids: Vec<&str> = loads.iter().map(|load| load.id.as_str()).collect();
        assert_eq!(ids, vec!["a.b:panel:board", "a.b:status:wip"]);
        // Every load runs where the declaration lives.
        assert!(
            loads
                .iter()
                .all(|load| load.cwd == Path::new("/tmp/zenix-ui-root"))
        );
    }

    /// A declaration's command runs in the plugin's own root: the daemon runs a
    /// plugin's commands there, and a `zenix-ui.toml` script sits beside the
    /// manifest in exactly the same way.
    #[test]
    fn a_source_command_runs_in_the_plugins_own_directory() {
        let root = std::env::temp_dir().join(format!("zenix-ui-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the fixture directory is created");
        std::fs::write(
            root.join("print.sh"),
            "#!/bin/sh\necho '{\"where\": \"plugin root\"}'\n",
        )
        .expect("the fixture script is written");
        let data = read_in("x", &["sh", "print.sh"], &root);
        let _ = std::fs::remove_dir_all(&root);
        match data {
            Data::Ready(value) => assert_eq!(value["where"], "plugin root"),
            other => panic!("expected the script's output, got {other:?}"),
        }
    }

    #[test]
    fn a_relative_program_is_resolved_against_the_plugins_root() {
        assert_eq!(
            resolve_program("./bin/tool", Path::new("/root")),
            PathBuf::from("/root/bin/tool")
        );
        assert_eq!(
            resolve_program("sh", Path::new("/root")),
            PathBuf::from("sh")
        );
    }

    /// The reader end to end, in the shape the shell sees it: a request, then an
    /// answer that can be read out of the cache.
    #[test]
    fn the_reader_answers_a_request() {
        let (host, pulses) = UiDataHost::start();
        let load = Load {
            id: "a.b:panel:x".to_owned(),
            command: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "echo '{\"count\": 1}'".to_owned(),
            ],
            cwd: std::env::temp_dir(),
        };
        host.sync(vec![load], Arc::new(Value::Object(Map::new())), false);
        let value = await_answer(&host, &pulses, "a.b:panel:x");
        assert_eq!(value["count"], 1);
    }

    /// Asking twice is not running the plugin's command twice: a panel that
    /// repaints must not multiply a plugin's side effects.
    #[test]
    fn a_second_request_does_not_run_the_command_again() {
        let counter = std::env::temp_dir().join(format!(
            "zenix-ui-data-count-second-{}.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&counter);
        let (host, pulses) = UiDataHost::start();
        let load = Load {
            id: "a.b:panel:count".to_owned(),
            command: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                format!("echo run >> {}", counter.display()),
            ],
            cwd: std::env::temp_dir(),
        };
        host.sync(vec![load], Arc::new(Value::Object(Map::new())), false);
        // The command prints no JSON, so this only waits for the run to end.
        await_settled(&host, &pulses, "a.b:panel:count");
        host.sync(
            vec![Load {
                id: "a.b:panel:count".to_owned(),
                command: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
                cwd: std::env::temp_dir(),
            }],
            Arc::new(Value::Object(Map::new())),
            false,
        );
        thread::sleep(Duration::from_millis(300));
        let runs = std::fs::read_to_string(&counter).unwrap_or_default();
        let _ = std::fs::remove_file(&counter);
        assert_eq!(runs.lines().count(), 1, "the command ran once: {runs:?}");
    }

    /// A forced refresh does read it again: that is what asking for one means.
    #[test]
    fn a_forced_request_asks_again() {
        let (host, pulses) = UiDataHost::start();
        let script = std::env::temp_dir().join(format!(
            "zenix-ui-data-forced-ask-{}.sh",
            std::process::id()
        ));
        let load = |script: &Path| Load {
            id: "a.b:panel:forced".to_owned(),
            command: vec!["sh".to_owned(), script.display().to_string()],
            cwd: std::env::temp_dir(),
        };
        std::fs::write(&script, "echo '{\"n\": 1}'\n").expect("the fixture script is written");
        host.sync(
            vec![load(&script)],
            Arc::new(Value::Object(Map::new())),
            false,
        );
        let value = await_answer(&host, &pulses, "a.b:panel:forced");
        assert_eq!(value["n"], 1);
        std::fs::write(&script, "echo '{\"n\": 2}'\n").expect("the fixture script is replaced");
        host.sync(
            vec![load(&script)],
            Arc::new(Value::Object(Map::new())),
            true,
        );
        // The forced read replaces the old answer, so wait for the new one rather
        // than for any answer at all.
        let value = await_value(&host, &pulses, "a.b:panel:forced", |value| value["n"] == 2);
        assert_eq!(value["n"], 2);
        let _ = std::fs::remove_file(&script);
    }

    /// Wait for one declaration's answer, waking on the reader's own pulses.
    fn await_answer(host: &UiDataHost, pulses: &Receiver<()>, id: &str) -> Arc<Value> {
        await_value(host, pulses, id, |_| true)
    }

    /// Wait for a read to end, however it ended. A command that prints no JSON
    /// is a failure rather than an answer, and the run is what matters here.
    fn await_settled(host: &UiDataHost, pulses: &Receiver<()>, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if matches!(
                host.state().get(id),
                Some(Data::Ready(_)) | Some(Data::Failed(_))
            ) {
                return;
            }
            assert!(Instant::now() < deadline, "the reader never settled {id}");
            let _ = pulses.recv_timeout(Duration::from_millis(200));
        }
    }

    /// The same, for an answer that is expected to change.
    fn await_value(
        host: &UiDataHost,
        pulses: &Receiver<()>,
        id: &str,
        matched: impl Fn(&Value) -> bool,
    ) -> Arc<Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(Data::Ready(value)) = host.state().get(id)
                && matched(value)
            {
                return value.clone();
            }
            assert!(Instant::now() < deadline, "the reader never answered {id}");
            let _ = pulses.recv_timeout(Duration::from_millis(200));
        }
    }

    #[test]
    fn the_context_carries_the_daemons_names_and_only_what_is_known() {
        let empty = context(None);
        assert_eq!(empty["invocation_source"], "zenix_ui");
        assert!(empty.get("workspace_id").is_none());

        let snapshot: ClientShellSnapshot = serde_json::from_value(serde_json::json!({
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
            "workspaces": [{"workspace_id": "w1", "active_tab_id": "w1:t1",
                            "new_workspace_cwd": "/tmp", "number": 1, "label": "main",
                            "custom_label": false, "tokens": [], "focused": true,
                            "agent_status": "idle"}],
            "tabs": [],
            "panes": [{"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1",
                       "cwd": "/tmp/work", "focused": true, "right_click_passthrough": false}],
            "agents": [],
            "commands": [],
        }))
        .expect("the fixture snapshot deserializes");
        let context = context(Some(&snapshot));
        assert_eq!(context["workspace_id"], "w1");
        assert_eq!(context["workspace_label"], "main");
        assert_eq!(context["workspace_cwd"], "/tmp");
        assert_eq!(context["tab_id"], "w1:t1");
        assert_eq!(context["focused_pane_id"], "w1:p1");
        assert_eq!(context["focused_pane_cwd"], "/tmp/work");
        // Nothing the snapshot cannot answer is invented.
        assert!(context.get("selected_text").is_none());
        assert!(context.get("correlation_id").is_none());
    }
}
