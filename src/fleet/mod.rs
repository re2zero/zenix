//! The orchestration cockpit's data: which agents are in flight, what each one
//! is doing, and the transitions each went through while this client watched.
//!
//! The daemon's snapshot already carries the fleet — one `ClientShellAgent` per
//! running agent, with its status and a `state_change_seq` that moves when the
//! status does. What a snapshot cannot carry is the *history*: it is replaced
//! wholesale, so a status that changed twice between two snapshots leaves one
//! value behind. The transitions come from the API lane instead, per pane:
//!
//! Measured against herdr 0.9.5 in an isolated session
//! (`.relay/notes/zenix-fleet-check.sh`):
//!
//! - `events.subscribe` accepts `{"type":"pane.agent_status_changed",
//!   "pane_id": …}` and answers `{"type":"subscription_started"}`; a session-wide
//!   request for the same event is refused for lack of that field, which is why
//!   [`crate::events`] does not subscribe to it at all;
//! - the line is `{"event":"pane.agent_status_changed","data":{pane_id,
//!   workspace_id, agent_status, agent,…}}` — the kind keeps its *dotted*
//!   spelling here, where the global stream writes the underscored one;
//! - there is no replay: a subscription opened while the fleet is already idle
//!   hears nothing until something changes, so the first status of a pane is
//!   whatever the painted snapshot says, not what this stream said;
//! - the daemon stamps no time on a line, so a transition's time here is when
//!   this client read it, and the panel shows an age rather than a clock — the
//!   same decision [`crate::events`] made for the same reason.
//!
//! One connection carries one fixed set of panes, so a fleet that gained or lost
//! an agent is followed by dropping the connection and subscribing again — which
//! is what [`Stream::watch`] asks for by moving the version the worker polls.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use serde_json::{Value, json};

use crate::herdr::api::{ApiClient, ApiError, Line};

/// How many transitions are kept. The cockpit is a window on what is happening,
/// not a log of the session.
pub(crate) const RING: usize = 48;

/// How long a poll waits before the worker looks at its own state again. The
/// daemon answers a status change within about a second of the screen moving.
const POLL: Duration = Duration::from_millis(500);

/// How long to wait before subscribing again after a stream ended.
const RECONNECT: Duration = Duration::from_secs(2);

/// Rows a tail read asks the daemon for.
pub(crate) const TAIL_ROWS: u32 = 40;

/// Rows of a tail the panel draws per agent. The comparison is a glance, not a
/// read: every row here costs the other agents a row of their own.
pub(crate) const TAIL_SHOWN: usize = 3;

/// One agent's status change, as this client read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Transition {
    /// Epoch milliseconds when this client read the line.
    pub(crate) at_unix_ms: u64,
    pub(crate) pane_id: String,
    /// The status this client last saw the pane in, when it has seen one.
    pub(crate) from: Option<String>,
    pub(crate) to: String,
}

/// One pane this client is subscribed to, and the status it is in now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Watched {
    pub(crate) pane_id: String,
    pub(crate) status: String,
    /// When the status was entered, as this client saw it. A pane whose status
    /// has not moved since the client started watching has no start time, and
    /// the panel says so rather than inventing one.
    pub(crate) since_unix_ms: u64,
}

/// What the fleet stream has read, as one value the shell clones per paint.
#[derive(Clone, Debug, Default)]
pub(crate) struct FleetState {
    /// Newest first, which is the order a panel reads them in.
    pub(crate) transitions: VecDeque<Transition>,
    /// The current status of every pane being watched.
    pub(crate) current: Vec<Watched>,
    /// Why the stream is not connected, until it is again.
    pub(crate) error: Option<String>,
}

impl FleetState {
    /// The status this stream saw for a pane, and when it was entered.
    pub(crate) fn watched(&self, pane_id: &str) -> Option<&Watched> {
        self.current.iter().find(|w| w.pane_id == pane_id)
    }

    /// The transitions of one pane, newest first, up to `limit` of them.
    pub(crate) fn history<'a>(&'a self, pane_id: &str, limit: usize) -> Vec<&'a Transition> {
        self.transitions
            .iter()
            .filter(|transition| transition.pane_id == pane_id)
            .take(limit)
            .collect()
    }

    /// How many panes are in a status, as the header counts them.
    pub(crate) fn count(&self, status: &str) -> usize {
        self.current
            .iter()
            .filter(|watched| watched.status == status)
            .count()
    }
}

/// One agent's recent output, read on demand for the side-by-side view.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tail {
    /// Epoch milliseconds when the read came back.
    pub(crate) at_unix_ms: u64,
    /// The last rows of the pane, oldest first.
    pub(crate) lines: Vec<String>,
    /// Why the read failed, when it did.
    pub(crate) error: Option<String>,
}

/// The tails of every agent, by pane id.
pub(crate) type Tails = HashMap<String, Tail>;

/// The last `shown` non-blank rows of a read, oldest first. A pane's text ends
/// in the blank rows under its prompt; a comparison wants the lines above them.
pub(crate) fn tail_lines(text: &str, shown: usize) -> Vec<String> {
    let mut rows: Vec<String> = text
        .lines()
        .rev()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .take(shown)
        .map(str::to_owned)
        .collect();
    rows.reverse();
    rows
}

/// The stream: one worker thread, one cache, one pulse channel.
pub(crate) struct Stream {
    cache: Arc<Mutex<FleetState>>,
    watch: Arc<Mutex<Watch>>,
}

/// Which panes are worth a subscription, and which generation of that list the
/// worker is on. One connection cannot be renegotiated, so a new generation is
/// the worker's signal to drop the connection and subscribe again.
#[derive(Debug, Default)]
struct Watch {
    version: u64,
    panes: Vec<String>,
}

impl Stream {
    /// Start the stream and its worker. The returned receiver pulses once per
    /// transition, so the shell repaints as they arrive instead of polling.
    pub(crate) fn start() -> (Self, Receiver<()>) {
        let cache = Arc::new(Mutex::new(FleetState::default()));
        let watch = Arc::new(Mutex::new(Watch::default()));
        // Bounded at one: a transition that arrives while the shell has not
        // painted the one before it needs no second wakeup.
        let (pulses_out, pulses) = crossbeam_channel::bounded(1);
        let worker_cache = cache.clone();
        let worker_watch = watch.clone();
        thread::Builder::new()
            .name("zenix-fleet".into())
            .spawn(move || follow(ApiClient::local(), worker_cache, worker_watch, pulses_out))
            .expect("failed to spawn the fleet stream thread");
        (Self { cache, watch }, pulses)
    }

    /// Follow these panes. Called with the snapshot's agent list on every
    /// revision; only a set that actually changed moves the generation.
    pub(crate) fn watch(&self, panes: Vec<String>) {
        let panes = {
            let mut watch = lock(&self.watch);
            if watch.panes == panes {
                return;
            }
            watch.panes = panes;
            watch.version += 1;
            watch.panes.clone()
        };
        // A pane that left the fleet takes its current status with it; its
        // transitions stay, because what it did is still what happened.
        lock(&self.cache)
            .current
            .retain(|watched| panes.contains(&watched.pane_id));
    }

    /// A copy of the state for one paint.
    pub(crate) fn state(&self) -> FleetState {
        lock(&self.cache).clone()
    }

    /// Drop the transitions read so far. The subscription is untouched.
    pub(crate) fn clear(&self) {
        lock(&self.cache).transitions.clear();
    }
}

/// Read the fleet, and read it again whenever the set of panes changes or the
/// stream ends.
fn follow(
    client: ApiClient,
    cache: Arc<Mutex<FleetState>>,
    watch: Arc<Mutex<Watch>>,
    pulses: Sender<()>,
) {
    // The status each pane was last seen in, so a transition can name what it
    // came from. It lives here rather than in the cache because it is the
    // reader's own memory of the stream.
    let mut last: HashMap<String, String> = HashMap::new();
    loop {
        let (version, panes) = {
            let watch = lock(&watch);
            (watch.version, watch.panes.clone())
        };
        if panes.is_empty() {
            // No agents: nothing to listen to, and nothing wrong either.
            set_error(&cache, None, &pulses);
            thread::sleep(POLL);
            continue;
        }
        let outcome = read(&client, &cache, &watch, version, &panes, &mut last, &pulses);
        match outcome {
            // The set of panes changed: subscribe again at once, not after a
            // backoff — a fleet that just grew should be followed immediately.
            Ok(Ended::Rewatch) => continue,
            Ok(Ended::Closed) => set_error(
                &cache,
                Some("the daemon closed the fleet stream".into()),
                &pulses,
            ),
            Err(error) => set_error(&cache, Some(error.to_string()), &pulses),
        }
        thread::sleep(RECONNECT);
    }
}

/// Why a subscription ended.
enum Ended {
    /// The set of panes changed; subscribe again with the new one.
    Rewatch,
    /// The daemon closed the connection.
    Closed,
}

/// Say why the stream is where it is, and wake the shell. An error nobody is
/// told about looks exactly like a fleet that has nothing to say.
fn set_error(cache: &Mutex<FleetState>, error: Option<String>, pulses: &Sender<()>) {
    let mut state = lock(cache);
    if state.error == error {
        return;
    }
    state.error = error;
    drop(state);
    let _ = pulses.try_send(());
}

fn read(
    client: &ApiClient,
    cache: &Mutex<FleetState>,
    watch: &Mutex<Watch>,
    version: u64,
    panes: &[String],
    last: &mut HashMap<String, String>,
    pulses: &Sender<()>,
) -> Result<Ended, ApiError> {
    let params = json!({
        "subscriptions": panes
            .iter()
            .map(|pane_id| json!({"type": "pane.agent_status_changed", "pane_id": pane_id}))
            .collect::<Vec<Value>>(),
    });
    let mut stream = client.subscribe(params)?;
    set_error(cache, None, pulses);
    loop {
        if lock(watch).version != version {
            return Ok(Ended::Rewatch);
        }
        match stream.next_line_within(POLL)? {
            // Nothing yet, and the daemon has not closed: look at the watch
            // again, which is how a fleet that grew is noticed.
            Line::Idle => continue,
            Line::Closed => return Ok(Ended::Closed),
            Line::Line(line) => {
                let at_unix_ms = crate::events::now_unix_ms();
                match transition_from(&line) {
                    Ok(None) => {}
                    Ok(Some((pane_id, status))) => {
                        apply(cache, last, &pane_id, status, at_unix_ms, pulses);
                    }
                    Err(reason) => {
                        set_error(cache, Some(reason), pulses);
                        return Ok(Ended::Closed);
                    }
                }
            }
        }
    }
}

/// One line as a status change. `Ok(None)` is a line that is not one — the
/// subscription's acknowledgement, or an event this stream did not ask for.
fn transition_from(line: &str) -> Result<Option<(String, String)>, String> {
    let value: Value = serde_json::from_str(line.trim_end())
        .map_err(|error| format!("the fleet stream wrote something that is not JSON: {error}"))?;
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("error");
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(format!("{code}: {message}"));
    }
    if value.get("event").and_then(Value::as_str) != Some("pane.agent_status_changed") {
        return Ok(None);
    }
    let data = value.get("data").unwrap_or(&Value::Null);
    let Some(pane_id) = data.get("pane_id").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(status) = data.get("agent_status").and_then(Value::as_str) else {
        return Ok(None);
    };
    Ok(Some((pane_id.to_owned(), status.to_owned())))
}

/// Fold one status change into the cache: a transition to read and the pane's
/// current status to count.
fn apply(
    cache: &Mutex<FleetState>,
    last: &mut HashMap<String, String>,
    pane_id: &str,
    status: String,
    at_unix_ms: u64,
    pulses: &Sender<()>,
) {
    {
        let mut state = lock(cache);
        match state.current.iter_mut().find(|w| w.pane_id == pane_id) {
            Some(watched) => {
                watched.status = status.clone();
                watched.since_unix_ms = at_unix_ms;
            }
            None => state.current.push(Watched {
                pane_id: pane_id.to_owned(),
                status: status.clone(),
                since_unix_ms: at_unix_ms,
            }),
        }
    }
    let from = last.insert(pane_id.to_owned(), status.clone());
    // A repeated status is not a transition: the daemon only writes when one
    // moved, so a repeat here is this client reading the same change twice.
    if from.as_deref() == Some(status.as_str()) {
        return;
    }
    {
        let mut state = lock(cache);
        state.transitions.push_front(Transition {
            at_unix_ms,
            pane_id: pane_id.to_owned(),
            from,
            to: status,
        });
        state.transitions.truncate(RING);
    }
    let _ = pulses.try_send(());
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_change_becomes_a_transition() {
        let line = r#"{"data":{"agent":"pi","agent_status":"working","pane_id":"w1:p2","workspace_id":"w1"},"event":"pane.agent_status_changed"}"#;
        let change = transition_from(line).expect("a line").expect("a change");
        assert_eq!(change.0, "w1:p2");
        assert_eq!(change.1, "working");
    }

    /// The acknowledgement is the first line of a subscription, and the daemon
    /// may also write events this stream did not ask for.
    #[test]
    fn a_line_that_is_not_a_status_change_is_skipped() {
        let acknowledgement = r#"{"id":"zenix-1-1","result":{"type":"subscription_started"}}"#;
        assert_eq!(transition_from(acknowledgement), Ok(None));
        let other = r#"{"data":{"pane_id":"w1:p2"},"event":"pane_scroll_changed"}"#;
        assert_eq!(transition_from(other), Ok(None));
    }

    #[test]
    fn an_error_line_says_why_the_stream_is_over() {
        let line = r#"{"id":"zenix-1-1","error":{"code":"events_lost","message":"fell behind"}}"#;
        assert_eq!(
            transition_from(line),
            Err("events_lost: fell behind".to_owned())
        );
    }

    #[test]
    fn a_line_that_is_not_json_is_reported_as_such() {
        let error = transition_from("not json").expect_err("not a line");
        assert!(error.contains("not JSON"), "{error}");
    }

    /// The first status a pane is seen in has no predecessor, and a repeated
    /// status is not a transition.
    #[test]
    fn the_first_status_has_no_predecessor_and_a_repeat_is_not_a_transition() {
        let cache = Mutex::new(FleetState::default());
        let mut last = HashMap::new();
        let (pulses, pulses_in) = crossbeam_channel::unbounded();
        apply(&cache, &mut last, "w1:p2", "idle".into(), 10, &pulses);
        apply(&cache, &mut last, "w1:p2", "idle".into(), 20, &pulses);
        apply(&cache, &mut last, "w1:p2", "working".into(), 30, &pulses);
        let state = lock(&cache).clone();
        assert_eq!(state.transitions.len(), 2);
        assert_eq!(state.transitions[1].from, None);
        assert_eq!(state.transitions[1].to, "idle");
        assert_eq!(state.transitions[0].from.as_deref(), Some("idle"));
        assert_eq!(state.transitions[0].to, "working");
        assert_eq!(
            state.watched("w1:p2").map(|w| w.status.as_str()),
            Some("working")
        );
        assert_eq!(state.watched("w1:p2").map(|w| w.since_unix_ms), Some(30));
        assert_eq!(state.count("working"), 1);
        // One pulse per transition: the repeated status woke nobody.
        assert_eq!(pulses_in.try_iter().count(), 2);
    }

    #[test]
    fn a_pane_s_history_is_its_own_newest_first() {
        let cache = Mutex::new(FleetState::default());
        let mut last = HashMap::new();
        let (pulses, _) = crossbeam_channel::bounded(1);
        apply(&cache, &mut last, "w1:p1", "idle".into(), 1, &pulses);
        apply(&cache, &mut last, "w1:p2", "blocked".into(), 2, &pulses);
        apply(&cache, &mut last, "w1:p1", "working".into(), 3, &pulses);
        let state = lock(&cache).clone();
        let history: Vec<&str> = state
            .history("w1:p1", 5)
            .iter()
            .map(|transition| transition.to.as_str())
            .collect();
        assert_eq!(history, ["working", "idle"]);
        assert_eq!(state.history("w1:p2", 5).len(), 1);
        assert_eq!(state.history("w9:p9", 5).len(), 0);
    }

    /// The ring is a window, not a log: the oldest transitions fall off.
    #[test]
    fn the_ring_keeps_the_newest_transitions() {
        let cache = Mutex::new(FleetState::default());
        let mut last = HashMap::new();
        let (pulses, _) = crossbeam_channel::bounded(1);
        for index in 0..RING + 8 {
            let status = if index % 2 == 0 { "idle" } else { "working" };
            apply(
                &cache,
                &mut last,
                "w1:p2",
                status.into(),
                index as u64,
                &pulses,
            );
        }
        let state = lock(&cache).clone();
        assert_eq!(state.transitions.len(), RING);
        assert_eq!(
            state.transitions.front().map(|t| t.at_unix_ms),
            Some((RING + 7) as u64)
        );
    }

    #[test]
    fn a_tail_keeps_the_last_non_blank_rows_oldest_first() {
        let text = "one\n\ntwo\nthree\n\n";
        assert_eq!(tail_lines(text, 2), ["two", "three"]);
        assert_eq!(tail_lines("only\n", 6), ["only"]);
        assert!(tail_lines("\n\n", 3).is_empty());
    }

    #[test]
    fn watching_the_same_panes_twice_moves_no_generation() {
        let cache = Arc::new(Mutex::new(FleetState::default()));
        let watch = Arc::new(Mutex::new(Watch::default()));
        let stream = Stream {
            cache: cache.clone(),
            watch: watch.clone(),
        };
        stream.watch(vec!["w1:p2".into()]);
        let first = lock(&watch).version;
        stream.watch(vec!["w1:p2".into()]);
        assert_eq!(lock(&watch).version, first);
        stream.watch(vec!["w1:p2".into(), "w1:p3".into()]);
        assert_eq!(lock(&watch).version, first + 1);
    }

    /// A pane that left the fleet stops being counted, and its history stays:
    /// what it did still happened.
    #[test]
    fn a_pane_that_left_stops_being_watched_but_keeps_its_history() {
        let cache = Arc::new(Mutex::new(FleetState::default()));
        let watch = Arc::new(Mutex::new(Watch::default()));
        let stream = Stream {
            cache: cache.clone(),
            watch: watch.clone(),
        };
        let mut last = HashMap::new();
        let (pulses, _) = crossbeam_channel::bounded(1);
        apply(&cache, &mut last, "w1:p2", "working".into(), 1, &pulses);
        stream.watch(vec!["w1:p2".into()]);
        assert_eq!(lock(&cache).count("working"), 1);
        stream.watch(Vec::new());
        let state = lock(&cache).clone();
        assert_eq!(state.current.len(), 0);
        assert_eq!(state.transitions.len(), 1);
    }

    #[test]
    fn clearing_drops_the_transitions_and_leaves_the_current_status() {
        let cache = Arc::new(Mutex::new(FleetState::default()));
        let watch = Arc::new(Mutex::new(Watch::default()));
        let stream = Stream {
            cache: cache.clone(),
            watch,
        };
        let mut last = HashMap::new();
        let (pulses, _) = crossbeam_channel::bounded(1);
        apply(&cache, &mut last, "w1:p2", "blocked".into(), 1, &pulses);
        stream.clear();
        let state = lock(&cache).clone();
        assert!(state.transitions.is_empty());
        assert_eq!(state.count("blocked"), 1);
    }
}
