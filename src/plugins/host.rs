//! The plugin host: the daemon's plugin registry, read on this side.
//!
//! The daemon never pushes a plugin change. Measured against 0.9.5, linking,
//! unlinking, enabling and disabling a plugin emit nothing at all on the
//! endpoint lane, and an invocation's outcome is never pushed either — it only
//! ever appears in the log ring. Plugin *panes* are the exception, and only
//! because they are ordinary daemon panes: opening one pushes a snapshot and a
//! surface like any other pane does.
//!
//! So the cache here is refreshed when it can have changed: once at start, on
//! demand, after every mutation this client makes, and on a slow fallback tick
//! for a change made outside the client (a plugin linked in a shell).
//!
//! Every read is a blocking one-shot call on the API lane, so it runs on this
//! module's own worker thread: never on the UI thread, and never on the
//! endpoint connection that owns the paint state. The shell learns about a
//! change from a pulse and then reads the cache, which is the same shape the
//! daemon's own pushes take.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use super::api;
use super::model::{InstalledPlugin, OwnedPane, Placement, PluginAction, PluginCommandLog};
use super::ui_manifest::{self, UiContributions};
use crate::herdr::api::ApiClient;

/// How long the worker waits for work before re-reading the registry anyway.
/// Without it, a plugin linked from a shell would never appear in a running
/// client.
const FALLBACK_TICK: Duration = Duration::from_secs(30);

/// How long to wait for a freshly spawned command before reading the log ring
/// again. Most actions finish in milliseconds; a longer run is caught by the
/// next tick or by an explicit refresh.
const OUTCOME_SETTLE: Duration = Duration::from_millis(700);

/// Everything the shell knows about plugins, as one value to read per paint.
#[derive(Clone, Debug, Default)]
pub(crate) struct PluginState {
    pub(crate) plugins: Vec<InstalledPlugin>,
    pub(crate) actions: Vec<PluginAction>,
    /// Newest first, which is the order a panel reads them in. The daemon
    /// answers oldest first within the window it was asked for.
    pub(crate) logs: Vec<PluginCommandLog>,
    /// Plugin panes this client opened, newest first.
    pub(crate) panes: Vec<OwnedPane>,
    /// What the installed plugins declare for this client to draw, read from
    /// each plugin's own `zenix-ui.toml` and mounted in the shell. The daemon
    /// knows nothing about it: it is the host's own contract with the plugin.
    pub(crate) ui: UiContributions,
    /// Why the last refresh failed, until one succeeds. The registry below may
    /// still be the previous, working answer.
    pub(crate) error: Option<String>,
    /// The last thing this client asked the daemon to do, and how it answered.
    /// An invocation the daemon refused leaves no log entry, so this is the only
    /// place its reason appears.
    pub(crate) last: Option<String>,
}

/// The plugin host: one worker thread, one cache, one pulse channel.
pub(crate) struct PluginHost {
    cache: Arc<Mutex<PluginState>>,
    commands: Sender<Request>,
}

impl PluginHost {
    /// Start the host and its worker. The returned receiver pulses once per
    /// state change, so the shell repaints on a change instead of polling.
    pub(crate) fn start() -> (Self, Receiver<()>) {
        let cache = Arc::new(Mutex::new(PluginState::default()));
        let (commands, requests) = crossbeam_channel::unbounded();
        // Bounded at one: the shell only needs to know that something moved
        // since its last read, so a burst of changes coalesces into one wakeup.
        let (pulses_out, pulses) = crossbeam_channel::bounded(1);
        let worker_cache = cache.clone();
        thread::Builder::new()
            .name("zenix-plugins".into())
            .spawn(move || work(ApiClient::local(), worker_cache, requests, pulses_out))
            .expect("failed to spawn the plugin worker thread");
        let host = Self { cache, commands };
        host.refresh();
        (host, pulses)
    }

    /// A copy of the registry for one paint.
    pub(crate) fn state(&self) -> PluginState {
        lock(&self.cache).clone()
    }

    /// Re-read the installed plugins and their actions.
    pub(crate) fn refresh(&self) {
        let _ = self.commands.send(Request::Refresh);
    }

    /// Re-read the daemon's recent plugin commands.
    pub(crate) fn refresh_logs(&self) {
        let _ = self.commands.send(Request::RefreshLogs);
    }

    /// Link a plugin directory, enabled. The daemon reads the manifest; this
    /// side only names the directory, and the registry is re-read afterwards
    /// because the daemon pushes no change for a link.
    pub(crate) fn link(&self, path: String) {
        let _ = self.commands.send(Request::Link { path });
    }

    /// Run one action. `source` names where the click came from, which is the
    /// one part of the invocation context the daemon cannot know by itself.
    pub(crate) fn invoke(&self, action: PluginAction, source: &'static str) {
        let _ = self.commands.send(Request::Invoke { action, source });
    }

    /// Open one of a plugin's declared panes.
    pub(crate) fn open_pane(
        &self,
        plugin_id: String,
        entrypoint: String,
        title: String,
        placement: Placement,
    ) {
        let _ = self.commands.send(Request::OpenPane {
            plugin_id,
            entrypoint,
            title,
            placement,
        });
    }

    pub(crate) fn close_pane(&self, pane_id: String) {
        let _ = self.commands.send(Request::ClosePane { pane_id });
    }

    /// Dismiss the popup pane the daemon has open, if any.
    pub(crate) fn close_popup(&self) {
        let _ = self.commands.send(Request::ClosePopup);
    }
}

/// One unit of work for the worker thread.
enum Request {
    Refresh,
    RefreshLogs,
    Link {
        path: String,
    },
    Invoke {
        action: PluginAction,
        source: &'static str,
    },
    OpenPane {
        plugin_id: String,
        entrypoint: String,
        title: String,
        placement: Placement,
    },
    ClosePane {
        pane_id: String,
    },
    ClosePopup,
}

fn work(
    client: ApiClient,
    cache: Arc<Mutex<PluginState>>,
    commands: Receiver<Request>,
    pulses: Sender<()>,
) {
    loop {
        match commands.recv_timeout(FALLBACK_TICK) {
            Ok(request) => handle(&client, &cache, request),
            // Nothing was asked for: this is the fallback read, so a plugin
            // linked outside the client shows up, and a command that finished
            // after its invocation settled has its outcome collected.
            Err(RecvTimeoutError::Timeout) => {
                refresh(&client, &cache);
                refresh_logs(&client, &cache);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let _ = pulses.try_send(());
    }
}

fn handle(client: &ApiClient, cache: &Mutex<PluginState>, request: Request) {
    match request {
        Request::Refresh => refresh(client, cache),
        Request::RefreshLogs => refresh_logs(client, cache),
        Request::Invoke { action, source } => {
            let name = action.qualified_id();
            match api::invoke_action(client, &action, source) {
                Ok(log) => note(cache, format!("{name}: {}", log.status.label())),
                Err(error) => note(cache, format!("{name} was not run: {error}")),
            }
            // The daemon answers as soon as the command is spawned, so the
            // outcome is read from the ring, once right away and once after the
            // command has had a moment to finish.
            refresh_logs(client, cache);
            thread::sleep(OUTCOME_SETTLE);
            refresh_logs(client, cache);
        }
        Request::Link { path } => {
            match api::link_plugin(client, &path, true) {
                Ok(plugin) => {
                    note(cache, format!("{}: linked", plugin.plugin_id));
                    // Nothing pushes a link, so the registry is read again here:
                    // this is the one moment the client knows for certain that it
                    // changed.
                    refresh(client, cache);
                }
                Err(error) => note(cache, format!("{path} was not linked: {error}")),
            }
        }
        Request::OpenPane {
            plugin_id,
            entrypoint,
            title,
            placement,
        } => {
            match api::open_pane(client, &plugin_id, &entrypoint, placement) {
                // A popup is the daemon's own singleton terminal and reports no
                // pane, so there is nothing to remember for one.
                Ok(Some(opened)) => {
                    let pane_id = api::opened_pane_id(&opened).to_owned();
                    lock(cache).panes.insert(
                        0,
                        OwnedPane {
                            pane_id: pane_id.clone(),
                            plugin_id,
                            entrypoint,
                            title: title.clone(),
                        },
                    );
                    note(cache, format!("{title}: open as {pane_id}"));
                }
                Ok(None) => note(cache, format!("{title}: popup open")),
                Err(error) => note(cache, format!("{title} was not opened: {error}")),
            }
            // The pane itself arrives on the endpoint lane as a snapshot and a
            // surface push, like any other pane.
        }
        Request::ClosePane { pane_id } => {
            match api::close_pane(client, &pane_id) {
                Ok(()) => note(cache, format!("{pane_id}: closed")),
                Err(error) => note(cache, format!("{pane_id} was not closed: {error}")),
            }
            lock(cache).panes.retain(|pane| pane.pane_id != pane_id);
        }
        Request::ClosePopup => match api::close_popup(client) {
            Ok(()) => note(cache, "popup: closed".to_owned()),
            Err(error) => note(cache, format!("the popup was not closed: {error}")),
        },
    }
}

fn refresh(client: &ApiClient, cache: &Mutex<PluginState>) {
    match (api::list_plugins(client), api::list_actions(client)) {
        (Ok(plugins), Ok(actions)) => {
            // The declared UI is read here rather than at paint time: it is a
            // file per plugin, and this is the thread that may block on one.
            let ui = ui_manifest::mounted_ui(&plugins);
            let mut state = lock(cache);
            state.plugins = plugins;
            state.actions = actions;
            state.ui = ui;
            state.error = None;
        }
        // The registry could not be read at all: keep the last working answer
        // and say why it is the last one.
        (Err(error), _) | (_, Err(error)) => lock(cache).error = Some(error.to_string()),
    }
}

fn refresh_logs(client: &ApiClient, cache: &Mutex<PluginState>) {
    match api::list_logs(client, api::LOG_LIMIT) {
        Ok(mut logs) => {
            logs.reverse();
            lock(cache).logs = logs;
        }
        Err(error) => lock(cache).error = Some(error.to_string()),
    }
}

fn note(cache: &Mutex<PluginState>, line: String) {
    tracing::info!(target: "zenix-plugins", "{line}");
    lock(cache).last = Some(line);
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
