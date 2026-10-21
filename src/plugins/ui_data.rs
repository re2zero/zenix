//! The data behind a declared contribution: one command's stdout, parsed as
//! JSON, read off the UI thread and with a deadline.
//!
//! A declaration names a command; the host decides when it runs. Two rules do
//! that, and both are about a plugin's commands being real processes:
//!
//! * **On screen only.** A panel's command runs while the aside is showing the
//!   panels tab, a status segment's always (the bar is always painted). A
//!   declaration nobody can see is not run — a closed panel is not worth a
//!   process every ten seconds.
//! * **The declaration sets the cadence.** `refresh_s` in the declaration is how
//!   often the command is run again; absent means [`DEFAULT_EVERY`], and `0`
//!   means once and never again. A board wants a heartbeat; a layout dump does
//!   not.
//!
//! Nothing here runs on the UI thread: the command is another process, and a
//! plugin that hangs must not take the window with it. A re-read keeps the last
//! answer on screen until the new one arrives, so a heartbeat never blanks a
//! panel.
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

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use herdr_client::protocol::ClientShellSnapshot;
use serde_json::{Map, Value};

use super::ui_manifest::{Declared, UiContributions};

/// How long a declared command may run before it is stopped and reported as a
/// timeout. A panel that never answers is worse than one that says it gave up.
const DEADLINE: Duration = Duration::from_secs(5);

/// How often a running command is asked whether it is done.
const POLL: Duration = Duration::from_millis(25);

/// How often a declaration's command is run again when it does not say. Ten
/// seconds is a heartbeat a person reads as "live" without turning a panel into
/// a process spawner.
const DEFAULT_EVERY: Duration = Duration::from_secs(10);

/// The longest cadence a declaration may ask for. An hour is already "when I ask
/// for it"; anything longer is a typo worth clamping rather than honouring.
const MAX_EVERY: Duration = Duration::from_secs(3600);

/// What a declaration's data is filed under: the contribution's own key, the
/// command to read for it, and the directory to read it from — the plugin's own
/// root, which is where the daemon runs a plugin's commands from too.
pub(crate) struct Load {
    pub(crate) id: String,
    pub(crate) command: Vec<String>,
    pub(crate) cwd: PathBuf,
    /// How often to run it again, `None` for once. Resolved from the
    /// declaration by [`loads`], so the worker never reads a spec.
    pub(crate) every: Option<Duration>,
}

/// What can be seen right now, which decides what is read at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OnScreen {
    /// The aside is open on the tab the declared panels mount in.
    pub(crate) panels: bool,
}

impl OnScreen {
    /// Whether a slot with a source is on screen. Status segments are always
    /// painted, so they are always asked for.
    pub(crate) fn shows_panels(self) -> bool {
        self.panels
    }
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

    /// Declare what is on screen and how often each of it wants to be read. The
    /// worker keeps that list as its whole schedule: what is not in it is never
    /// run, and what is in it is re-read on its own cadence. `force` asks for
    /// every answer again now, which is what a refresh click means.
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

/// One declaration the worker is responsible for: what to run, and when it is
/// due next. `due: None` keeps the answer it has — a declaration that asked to
/// be read once.
struct Scheduled {
    load: Load,
    context: Arc<Value>,
    due: Option<Instant>,
}

/// How long the worker may sleep when nothing is scheduled at all. Any request
/// wakes it, so this only bounds a worker with an empty schedule.
const IDLE: Duration = Duration::from_secs(3600);

fn work(cache: Arc<Mutex<UiData>>, requests: Receiver<Request>, pulses: Sender<()>) {
    let mut schedule: HashMap<String, Scheduled> = HashMap::new();
    loop {
        let wait = schedule
            .values()
            .filter_map(|entry| entry.due)
            .map(|due| due.saturating_duration_since(Instant::now()))
            .min()
            .unwrap_or(IDLE);
        match requests.recv_timeout(wait) {
            Ok(Request::Load {
                loads,
                context,
                force,
            }) => {
                if force {
                    // Asking for a refresh means: every declaration on screen is
                    // read again now, whether or not its turn had come.
                    lock(&cache).entries.clear();
                    for entry in schedule.values_mut() {
                        entry.due = Some(Instant::now());
                    }
                }
                // The list is the schedule: a declaration that left the screen
                // stops being read, and its last answer goes with it.
                schedule.retain(|id, _| loads.iter().any(|load| &load.id == id));
                for load in loads {
                    match schedule.get_mut(&load.id) {
                        Some(entry) => {
                            entry.context = context.clone();
                            entry.load = load;
                        }
                        None => {
                            schedule.insert(
                                load.id.clone(),
                                Scheduled {
                                    load,
                                    context: context.clone(),
                                    due: Some(Instant::now()),
                                },
                            );
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        run_due(&cache, &mut schedule, &pulses);
    }
}

/// Read every declaration whose turn it is. An answer already on screen is kept
/// while its replacement runs, so a heartbeat does not blank a panel.
fn run_due(cache: &Mutex<UiData>, schedule: &mut HashMap<String, Scheduled>, pulses: &Sender<()>) {
    let now = Instant::now();
    let due: Vec<String> = schedule
        .iter()
        .filter(|(_, entry)| entry.due.is_some_and(|due| due <= now))
        .map(|(id, _)| id.clone())
        .collect();
    if due.is_empty() {
        return;
    }
    for id in due {
        let Some(entry) = schedule.get_mut(&id) else {
            continue;
        };
        {
            let mut state = lock(cache);
            // Only a declaration with no answer at all says "reading": a
            // re-read is not a state change a viewer needs to see.
            if !state.entries.contains_key(&id) {
                state.entries.insert(id.clone(), Data::Loading);
                let _ = pulses.try_send(());
            }
        }
        let data = read(&entry.load, &entry.context);
        lock(cache).entries.insert(id.clone(), data);
        let _ = pulses.try_send(());
        entry.due = entry
            .load
            .every
            .and_then(|every| entry.due.map(|due| due + every));
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
pub(crate) fn loads(ui: &UiContributions, screen: OnScreen) -> Vec<Load> {
    let mut loads = Vec::new();
    if screen.shows_panels() {
        loads.extend(ui.panels.iter().filter_map(|panel| {
            panel.source.as_ref().map(|source| Load {
                id: panel.qualified_id(),
                command: source.command.clone(),
                cwd: panel.plugin_root.clone(),
                every: every(panel.refresh_s),
            })
        }));
    }
    loads.extend(ui.status_items.iter().filter_map(|item| {
        item.source.as_ref().map(|source| Load {
            id: item.qualified_id(),
            command: source.command.clone(),
            cwd: item.plugin_root.clone(),
            every: every(item.refresh_s),
        })
    }));
    loads
}

/// How often a declaration's command runs again. `refresh_s` is the author's
/// answer; absent is the host's default, and `0` is "once".
fn every(refresh_s: Option<u64>) -> Option<Duration> {
    match refresh_s {
        Some(0) => None,
        Some(seconds) => Some(Duration::from_secs(seconds).min(MAX_EVERY)),
        None => Some(DEFAULT_EVERY),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    fn read_json(id: &str, command: &[&str]) -> Data {
        read_in(id, command, Path::new("/tmp"))
    }

    fn read_in(id: &str, command: &[&str], cwd: &Path) -> Data {
        let load = Load {
            id: id.to_owned(),
            command: command.iter().map(|part| (*part).to_owned()).collect(),
            cwd: cwd.to_path_buf(),
            every: None,
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
        let ui = crate::plugins::ui_manifest::parse(
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
        let loads = loads(&ui, OnScreen { panels: true });
        let ids: Vec<&str> = loads.iter().map(|load| load.id.as_str()).collect();
        assert_eq!(ids, vec!["a.b:panel:board", "a.b:status:wip"]);
        // Every load runs where the declaration lives.
        assert!(
            loads
                .iter()
                .all(|load| load.cwd == Path::new("/tmp/zenix-ui-root"))
        );
    }

    /// The cadence is the declaration's to set and the host's to default: an
    /// absent key means a heartbeat, `0` means once, and a huge number is
    /// clamped rather than honoured.
    #[test]
    fn a_declaration_sets_its_own_cadence_and_the_host_sets_the_default() {
        assert_eq!(every(None), Some(DEFAULT_EVERY));
        assert_eq!(every(Some(0)), None);
        assert_eq!(every(Some(2)), Some(Duration::from_secs(2)));
        assert_eq!(every(Some(u64::MAX)), Some(MAX_EVERY));
    }

    /// A panel is read only while its slot is on screen; a status segment always
    /// is, because the bar is never hidden.
    #[test]
    fn a_panel_is_read_only_while_its_slot_is_on_screen() {
        let ui = crate::plugins::ui_manifest::parse(
            "a.b",
            std::path::Path::new("/tmp/zenix-ui-root"),
            r#"
[[panel]]
id = "board"
title = "Board"
refresh_s = 0
[panel.source]
command = ["sh", "board.sh"]

[[status_item]]
id = "wip"
format = "{count} WIP"
source = { command = ["sh", "wip.sh"] }
"#,
        );
        let hidden = loads(&ui, OnScreen { panels: false });
        let ids: Vec<&str> = hidden.iter().map(|load| load.id.as_str()).collect();
        assert_eq!(ids, vec!["a.b:status:wip"]);

        let shown = loads(&ui, OnScreen { panels: true });
        let ids: Vec<&str> = shown.iter().map(|load| load.id.as_str()).collect();
        assert_eq!(ids, vec!["a.b:panel:board", "a.b:status:wip"]);
        // The panel asked to be read once; the segment asked for nothing, which
        // is the host's default.
        assert_eq!(shown[0].every, None);
        assert_eq!(shown[1].every, Some(DEFAULT_EVERY));
    }

    /// A declaration that asks for a cadence is read again on its own, without
    /// anyone asking: that is what makes a board a board. The answer on screen is
    /// kept while the re-read runs, so the panel does not blink.
    #[test]
    fn a_declaration_with_a_cadence_is_read_again_by_itself() {
        let counter = std::env::temp_dir().join(format!(
            "zenix-ui-data-heartbeat-{}.txt",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&counter);
        let (host, pulses) = UiDataHost::start();
        let script =
            std::env::temp_dir().join(format!("zenix-ui-data-heartbeat-{}.sh", std::process::id()));
        std::fs::write(
            &script,
            format!("echo run >> {}\necho '{{\"n\": 1}}'\n", counter.display()),
        )
        .expect("the fixture script is written");
        host.sync(
            vec![Load {
                id: "a.b:panel:beat".to_owned(),
                command: vec!["sh".to_owned(), script.display().to_string()],
                cwd: std::env::temp_dir(),
                every: Some(Duration::from_millis(200)),
            }],
            Arc::new(Value::Object(Map::new())),
            false,
        );
        await_answer(&host, &pulses, "a.b:panel:beat");
        // Two more turns of the same cadence.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut runs = 0;
        while Instant::now() < deadline {
            runs = std::fs::read_to_string(&counter)
                .unwrap_or_default()
                .lines()
                .count();
            if runs >= 3 {
                break;
            }
            let _ = pulses.recv_timeout(Duration::from_millis(100));
        }
        let _ = std::fs::remove_file(&counter);
        let _ = std::fs::remove_file(&script);
        assert!(runs >= 3, "the heartbeat ran {runs} time(s)");
        // It is still an answer, not a spinner: the panel never went back to
        // "reading".
        assert!(matches!(
            host.state().get("a.b:panel:beat"),
            Some(Data::Ready(_))
        ));
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
            every: None,
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
            every: None,
        };
        host.sync(vec![load], Arc::new(Value::Object(Map::new())), false);
        // The command prints no JSON, so this only waits for the run to end.
        await_settled(&host, &pulses, "a.b:panel:count");
        host.sync(
            vec![Load {
                id: "a.b:panel:count".to_owned(),
                command: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
                cwd: std::env::temp_dir(),
                every: None,
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
            every: None,
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
