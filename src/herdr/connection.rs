//! One daemon endpoint connection: connect, read its events, own the truth.
//!
//! The reader runs on its own thread and only ever locks the state for the
//! length of one `apply`; the UI clones the state out for a paint. `on_change`
//! is called after each applied event so a view can repaint without polling.
//! Nothing here polls the daemon.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use herdr_client::protocol::ClientPaneInputEvent;
use herdr_client::{
    ClientEvent, ClientHandle, ConnectOptions, ConnectTarget, Method, VersionMismatch, connect,
};
use serde_json::Value;

use super::state::{ConnectionStatus, LiveState, Notice};
use crate::client;

/// How long to wait for a freshly spawned daemon to publish its endpoint.
const DAEMON_STARTUP: Duration = Duration::from_secs(10);
const DAEMON_POLL: Duration = Duration::from_millis(50);
/// The first wait after a drop, doubled on each failed attempt up to the cap.
/// A daemon that is gone for good is retried every half minute rather than
/// busy-looping on its socket.
const RECONNECT_INITIAL: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);
/// How often a waiting reader checks whether the shell asked it to try now.
const RETRY_POLL: Duration = Duration::from_millis(100);

/// A running endpoint connection. Cloning shares the one reader and the one
/// state.
#[derive(Clone)]
pub(crate) struct Connection {
    /// The pushed state, cloned out for each paint.
    changed: Arc<Mutex<LiveState>>,
    handle: Arc<Mutex<Option<ClientHandle>>>,
    /// Set by the shell's retry action: the reader stops waiting out its
    /// backoff and dials at once.
    retry: Arc<AtomicBool>,
}

impl Connection {
    /// Connect in the background. Returns immediately; `on_change` fires after
    /// every applied event. A dropped endpoint is dialled again on a backoff,
    /// so a restarted daemon heals without reopening the window.
    pub(crate) fn start(
        target: ConnectTarget,
        on_change: impl Fn() -> bool + Send + 'static,
    ) -> Connection {
        let changed = Arc::new(Mutex::new(LiveState::default()));
        let handle = Arc::new(Mutex::new(None));
        let retry = Arc::new(AtomicBool::new(false));

        let connection = Connection {
            changed: changed.clone(),
            handle: handle.clone(),
            retry: retry.clone(),
        };

        thread::Builder::new()
            .name("zenix-herdr".into())
            .spawn(move || read_events(target, changed, handle, retry, on_change))
            .expect("failed to spawn the herdr reader thread");

        connection
    }

    /// Ask the reader to dial now instead of waiting out its backoff.
    pub(crate) fn retry_now(&self) {
        self.retry.store(true, Ordering::Release);
    }

    /// A copy of the current state, for one paint.
    pub(crate) fn state(&self) -> LiveState {
        lock(&self.changed).clone()
    }

    /// Remove and return the answer to `request_id`, if it has arrived.
    pub(crate) fn take_response(&self, request_id: &str) -> Option<Value> {
        lock(&self.changed).take_response(request_id)
    }

    /// Remove and return the notifications waiting for the platform.
    pub(crate) fn take_notices(&self) -> Vec<Notice> {
        lock(&self.changed).take_notices()
    }

    /// Whether `request_id` is still waiting for any answer. A rejected
    /// request is retired without a `Response`, so a caller that must not
    /// wait forever asks this after the answer slot came up empty.
    pub(crate) fn in_flight(&self, request_id: &str) -> bool {
        lock(&self.changed).in_flight.contains(request_id)
    }

    /// Clear the unread count for bells and attention notifications. The
    /// counter lives on the shared state, so the next pulled copy is zeroed
    /// too.
    pub(crate) fn acknowledge_alerts(&self) {
        let mut state = lock(&self.changed);
        if state.bells == 0 && state.attention == 0 {
            return;
        }
        state.bells = 0;
        state.attention = 0;
        state.revision += 1;
    }

    /// Remove and return the pending OSC 52 clipboard payloads, oldest first.
    ///
    /// This drains the shared state under the mutex instead of going through
    /// `state()`: `state()` clones the `LiveState`, so draining the clone
    /// would leave the reader's queue intact and the same payloads would be
    /// delivered again on the next pull.
    pub(crate) fn take_clipboard(&self) -> Vec<String> {
        lock(&self.changed).take_clipboard()
    }

    pub(crate) fn footer(&self) -> Option<String> {
        let state = lock(&self.changed);
        if let Some(error) = &state.error {
            return Some(error.clone());
        }
        state.version_mismatch.as_ref().map(|mismatch| {
            let note = mismatch
                .server_version()
                .map(|version| format!(" (version {version})"))
                .unwrap_or_default();
            match mismatch {
                VersionMismatch::DaemonOutdated { .. } => {
                    format!("the herdr daemon must be updated{note}")
                }
                VersionMismatch::ClientOutdated { .. } => {
                    format!("zenix must be updated{note}")
                }
            }
        })
    }

    /// Queue one pane's semantic input. `boot_id` is read here so callers name
    /// only the pane. `false` when the connection was not ready or the send
    /// failed, so a caller can tell a delivered press from a lost one.
    pub(crate) fn send_input(&self, pane_id: &str, events: Vec<ClientPaneInputEvent>) -> bool {
        let Some(handle) = self.handle() else {
            return false;
        };
        let Some(boot_id) = self.boot_id() else {
            return false;
        };
        match handle.send_input(&boot_id, pane_id, events) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(%err, pane_id, "pane input was not sent");
                false
            }
        }
    }

    /// Queue one popup's semantic input. A popup is not a pane — it never
    /// appears in the snapshot's pane list and the daemon names it by terminal
    /// id — so it has its own lane; everything else mirrors `send_input`,
    /// `boot_id` included, because a caller that cannot tell a delivered
    /// keystroke from a lost one would double-send it.
    pub(crate) fn send_popup_input(
        &self,
        terminal_id: &str,
        events: Vec<ClientPaneInputEvent>,
    ) -> bool {
        let Some(handle) = self.handle() else {
            return false;
        };
        let Some(boot_id) = self.boot_id() else {
            return false;
        };
        match handle.send_popup_input(&boot_id, terminal_id, events) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(%err, terminal_id, "popup input was not sent");
                false
            }
        }
    }

    /// Report the surface geometry the daemon should lay out for. `false` when
    /// the connection is not ready yet, so the caller can retry on the next
    /// paint rather than believing the daemon was told.
    pub(crate) fn resize(&self, options: ConnectOptions) -> bool {
        let Some(handle) = self.handle() else {
            return false;
        };
        let Some(boot_id) = self.boot_id() else {
            return false;
        };
        match handle.resize(&boot_id, options) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(%err, "surface resize was not sent");
                false
            }
        }
    }

    /// Ask the daemon to focus a pane. The new focus comes back as a snapshot;
    /// this only requests it, and focuses nothing locally.
    pub(crate) fn focus_pane(&self, pane_id: &str) {
        let Some(handle) = self.handle() else { return };
        let Some(boot_id) = self.boot_id() else {
            return;
        };
        match handle.focus_pane(&boot_id, pane_id) {
            Ok(request_id) => self.note(request_id),
            Err(err) => tracing::warn!(%err, pane_id, "pane focus was not sent"),
        }
    }

    /// Ask the daemon to focus a tab. The new focus comes back as a snapshot.
    pub(crate) fn focus_tab(&self, tab_id: &str) {
        let Some(handle) = self.handle() else { return };
        let Some(boot_id) = self.boot_id() else {
            return;
        };
        match handle.focus_tab(&boot_id, tab_id) {
            Ok(request_id) => self.note(request_id),
            Err(err) => tracing::warn!(%err, tab_id, "tab focus was not sent"),
        }
    }

    /// Ask the daemon to focus a workspace, which also activates its tab.
    pub(crate) fn focus_workspace(&self, workspace_id: &str) {
        let Some(handle) = self.handle() else { return };
        let Some(boot_id) = self.boot_id() else {
            return;
        };
        match handle.focus_workspace(&boot_id, workspace_id) {
            Ok(request_id) => self.note(request_id),
            Err(err) => tracing::warn!(%err, workspace_id, "workspace focus was not sent"),
        }
    }

    /// Hand one command to the daemon and remember it until its answer lands.
    /// `None` when the connection was not ready or the daemon refused the
    /// queue, so a caller can tell a queued command from a lost one.
    pub(crate) fn request(&self, method: Method, params: Value) -> Option<String> {
        let handle = self.handle()?;
        let boot_id = self.boot_id()?;
        match handle.request(&boot_id, method, params) {
            Ok(request_id) => {
                self.note(request_id.clone());
                Some(request_id)
            }
            Err(err) => {
                tracing::warn!(%err, %method, "command was not sent");
                None
            }
        }
    }

    /// Record a request the daemon has been handed, so a caller can wait for
    /// its answer through `LiveState::has_in_flight`.
    fn note(&self, request_id: String) {
        lock(&self.changed).note_request(request_id);
    }

    /// Whether a command is still waiting for its answer, read from the shared
    /// state rather than a view's pulled copy: an answer does not change the
    /// revision, so that copy stays stale exactly while a drag waits on it.
    pub(crate) fn has_in_flight(&self) -> bool {
        lock(&self.changed).has_in_flight()
    }

    fn handle(&self) -> Option<ClientHandle> {
        lock(&self.handle).clone()
    }

    fn boot_id(&self) -> Option<String> {
        lock(&self.changed).boot_id().map(str::to_owned)
    }
}

/// Ensure a daemon answers at `target`, then relay its events. A dropped or
/// refused endpoint is retried on a backoff until the reader thread is gone.
fn read_events(
    target: ConnectTarget,
    state: Arc<Mutex<LiveState>>,
    handle_slot: Arc<Mutex<Option<ClientHandle>>>,
    retry: Arc<AtomicBool>,
    on_change: impl Fn() -> bool,
) {
    // `false` means the view that owns this connection is gone: nothing is
    // owed a retry, so the reader stops.
    let fail = |reason: String| {
        lock(&state).apply(ClientEvent::Disconnected { reason, ssh: None });
        on_change()
    };

    let mut delay = RECONNECT_INITIAL;
    loop {
        if retry.swap(false, Ordering::AcqRel) {
            delay = RECONNECT_INITIAL;
        }
        let connected = match ensure_daemon(&target) {
            Err(reason) => {
                if !fail(reason) {
                    return;
                }
                false
            }
            Ok(()) => match connect(target.clone(), ConnectOptions::default()) {
                Err(err) => {
                    if !fail(format!("endpoint connect failed: {err}")) {
                        return;
                    }
                    false
                }
                Ok(client) => {
                    *lock(&handle_slot) = Some(client.handle.clone());
                    let mut connected = false;
                    while let Ok(event) = client.events.recv() {
                        // A welcome proves the endpoint is up, so a later drop
                        // starts the backoff from the first second again.
                        if matches!(event, ClientEvent::Connected(_)) {
                            connected = true;
                            delay = RECONNECT_INITIAL;
                        }
                        lock(&state).apply(event);
                        // A `false` means the view that owns this connection is
                        // gone; a window that closed must not keep dialling.
                        if !on_change() {
                            lock(&handle_slot).take();
                            return;
                        }
                    }
                    lock(&handle_slot).take();
                    {
                        let mut live = lock(&state);
                        if live.status != ConnectionStatus::Disconnected {
                            live.apply(ClientEvent::Disconnected {
                                reason: "the daemon closed the endpoint".into(),
                                ssh: None,
                            });
                        }
                    }
                    if !on_change() {
                        return;
                    }
                    connected
                }
            },
        };
        // A connection that never welcomed us earns a longer wait; one that
        // worked resets the ladder.
        if connected {
            delay = RECONNECT_INITIAL;
        }
        if wait_out(&retry, delay) {
            delay = RECONNECT_INITIAL;
        } else {
            delay = (delay * 2).min(RECONNECT_MAX);
        }
    }
}

/// Sleep for `delay`, waking early when the shell asked for a retry now.
/// `true` when it did.
fn wait_out(retry: &AtomicBool, delay: Duration) -> bool {
    let deadline = Instant::now() + delay;
    loop {
        if retry.swap(false, Ordering::AcqRel) {
            return true;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        thread::sleep(left.min(RETRY_POLL));
    }
}

/// Start the daemon when nothing answers at the socket, and wait for its
/// endpoint to come up. Local targets only: a remote host is the daemon's to
/// reach, not ours to spawn.
fn ensure_daemon(target: &ConnectTarget) -> Result<(), String> {
    let ConnectTarget::Local = target else {
        return Ok(());
    };
    let socket = client::herdr_socket_path();
    if client::is_socket_ready(&socket) {
        return Ok(());
    }
    if client::ensure_herdr().is_none() {
        return Err("no herdr binary found to start the daemon".into());
    }
    if !client::start_herdr_server() {
        return Err("failed to start the herdr daemon".into());
    }
    let deadline = Instant::now() + DAEMON_STARTUP;
    while Instant::now() < deadline {
        if client::is_socket_ready(&socket) {
            return Ok(());
        }
        thread::sleep(DAEMON_POLL);
    }
    Err(format!(
        "the herdr daemon did not publish {} in time",
        socket.display()
    ))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
