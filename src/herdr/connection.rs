//! One daemon endpoint connection: connect, read its events, own the truth.
//!
//! The reader runs on its own thread and only ever locks the state for the
//! length of one `apply`; the UI clones the state out for a paint. `on_change`
//! is called after each applied event so a view can repaint without polling.
//! A caller that needs a specific push — an orchestration tool waiting for a
//! pane to print something, or for the daemon's answer to a command it queued —
//! parks on the state's condvar via `wait_for_change`, woken by that same
//! apply; nothing here polls the daemon.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use herdr_client::protocol::ClientPaneInputEvent;
use herdr_client::{
    ClientEvent, ClientHandle, ConnectOptions, ConnectTarget, Method, VersionMismatch, connect,
};
use serde_json::Value;

use super::state::{ConnectionStatus, LiveState};
use crate::client;

/// How long to wait for a freshly spawned daemon to publish its endpoint.
const DAEMON_STARTUP: Duration = Duration::from_secs(10);
const DAEMON_POLL: Duration = Duration::from_millis(50);

/// A running endpoint connection. Cloning shares the one reader and the one
/// state.
#[derive(Clone)]
pub(crate) struct Connection {
    /// The pushed state, plus the condvar a caller parks on to be woken by the
    /// next applied event. Waiting here is what lets an orchestration tool
    /// await a pane's output without polling the daemon.
    changed: Arc<(Mutex<LiveState>, Condvar)>,
    handle: Arc<Mutex<Option<ClientHandle>>>,
    stopped: Arc<AtomicBool>,
}

impl Connection {
    /// Connect in the background. Returns immediately; `on_change` fires after
    /// every applied event.
    pub(crate) fn start(
        target: ConnectTarget,
        on_change: impl Fn() + Send + 'static,
    ) -> Connection {
        let changed = Arc::new((Mutex::new(LiveState::default()), Condvar::new()));
        let handle = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));

        let connection = Connection {
            changed: changed.clone(),
            handle: handle.clone(),
            stopped: stopped.clone(),
        };

        let reader_state = changed;
        let reader_handle = handle;
        thread::Builder::new()
            .name("zenix-herdr".into())
            .spawn(move || read_events(target, reader_state, reader_handle, stopped, on_change))
            .expect("failed to spawn the herdr reader thread");

        connection
    }

    /// A copy of the current state, for one paint.
    pub(crate) fn state(&self) -> LiveState {
        lock(&self.changed.0).clone()
    }

    /// Remove and return the pending OSC 52 clipboard payloads, oldest first.
    ///
    /// This drains the shared state under the mutex instead of going through
    /// `state()`: `state()` clones the `LiveState`, so draining the clone
    /// would leave the reader's queue intact and the same payloads would be
    /// delivered again on the next pull.
    pub(crate) fn take_clipboard(&self) -> Vec<String> {
        lock(&self.changed.0).take_clipboard()
    }

    pub(crate) fn footer(&self) -> Option<String> {
        let state = lock(&self.changed.0);
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
        lock(&self.changed.0).note_request(request_id);
    }

    /// Whether a command is still waiting for its answer, read from the shared
    /// state rather than a view's pulled copy: an answer does not change the
    /// revision, so that copy stays stale exactly while a drag waits on it.
    pub(crate) fn has_in_flight(&self) -> bool {
        lock(&self.changed.0).has_in_flight()
    }

    /// The revision a caller passes to `wait_for_change` to be woken by the
    /// next change rather than by one it already saw.
    pub(crate) fn revision(&self) -> u64 {
        lock(&self.changed.0).revision
    }

    /// Park until the state moves past `seen`, or `timeout` elapses. Returns
    /// whether it moved. Every applied event notifies, so this is a push wait:
    /// nothing here asks the daemon anything.
    pub(crate) fn wait_for_change(&self, seen: u64, timeout: Duration) -> bool {
        let (mutex, condvar) = &*self.changed;
        let mut guard = mutex.lock().unwrap_or_else(PoisonError::into_inner);
        let deadline = Instant::now() + timeout;
        loop {
            if guard.revision != seen {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, timed_out) = condvar
                .wait_timeout(guard, remaining)
                .unwrap_or_else(PoisonError::into_inner);
            guard = next;
            if timed_out.timed_out() && guard.revision == seen {
                return false;
            }
        }
    }

    /// Hand one command to the daemon and wait for its answer payload. `None`
    /// when the connection was not ready, the daemon refused the command, the
    /// connection ended, or nothing arrived within `timeout` — a caller that
    /// only needs the queueing to succeed uses `request` instead.
    pub(crate) fn request_await(
        &self,
        method: Method,
        params: Value,
        timeout: Duration,
    ) -> Option<Value> {
        let seen = self.revision();
        let request_id = self.request(method, params)?;
        self.await_answer(&request_id, seen, timeout)
    }

    /// Wait for the answer to a request already queued. `seen` is the revision
    /// read before queueing it.
    pub(crate) fn await_answer(
        &self,
        request_id: &str,
        mut seen: u64,
        timeout: Duration,
    ) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut state = lock(&self.changed.0);
                if let Some(answer) = state.take_answer(request_id) {
                    return Some(answer);
                }
                // A command the daemon refused, and a disconnect that retired
                // it, will never be answered: stop instead of spending the
                // whole timeout on it.
                if !state.in_flight.contains(request_id) {
                    return None;
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !self.wait_for_change(seen, remaining) {
                return lock(&self.changed.0).take_answer(request_id);
            }
            seen = self.revision();
        }
    }

    /// Stops the reader and closes the endpoint. No caller today: this is the
    /// window-close hook wired in M4.
    pub(crate) fn disconnect(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(handle) = self.handle() {
            handle.disconnect();
        }
    }

    fn handle(&self) -> Option<ClientHandle> {
        lock(&self.handle).clone()
    }

    fn boot_id(&self) -> Option<String> {
        lock(&self.changed.0).boot_id().map(str::to_owned)
    }
}

/// Ensure a daemon answers at `target`, then relay its events until it ends.
fn read_events(
    target: ConnectTarget,
    state: Arc<(Mutex<LiveState>, Condvar)>,
    handle_slot: Arc<Mutex<Option<ClientHandle>>>,
    stopped: Arc<AtomicBool>,
    on_change: impl Fn(),
) {
    // Applying an event and waking the parked waiters belong together: a
    // waiter that arrived after `apply` but before the notify would miss it.
    let fail = |reason: String| {
        lock(&state.0).apply(ClientEvent::Disconnected { reason, ssh: None });
        state.1.notify_all();
        on_change();
    };

    if let Err(reason) = ensure_daemon(&target) {
        fail(reason);
        return;
    }

    let client = match connect(target, ConnectOptions::default()) {
        Ok(client) => client,
        Err(err) => {
            fail(format!("endpoint connect failed: {err}"));
            return;
        }
    };
    *lock(&handle_slot) = Some(client.handle.clone());

    while let Ok(event) = client.events.recv() {
        lock(&state.0).apply(event);
        state.1.notify_all();
        on_change();
    }

    lock(&handle_slot).take();
    let reason = if stopped.load(Ordering::Acquire) {
        "disconnected".to_owned()
    } else {
        "the daemon closed the endpoint".to_owned()
    };
    let mut live = lock(&state.0);
    if live.status != ConnectionStatus::Disconnected {
        live.apply(ClientEvent::Disconnected { reason, ssh: None });
    }
    drop(live);
    state.1.notify_all();
    on_change();
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
