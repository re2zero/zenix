//! What the daemon announces, as a live list the shell can draw.
//!
//! The endpoint lane the client paints through carries state: snapshots and
//! surfaces. Everything the daemon announces *beside* that state — a tab
//! created, a pane that exited, an agent whose status changed, the layout that
//! moved — comes on the JSON API lane, and only to a client that asked:
//! `events.subscribe` holds the connection open and writes one line per event
//! until the daemon closes it.
//!
//! Measured against herdr 0.9.5 in an isolated session:
//!
//! - the reply to the subscribe is `{"type":"subscription_started"}`;
//! - every line after it is `{"event": "<kind>", "data": {…}}` — no request id
//!   and no timestamp, which is why an event's time here is when this client
//!   read the line and the panel shows an age rather than a clock;
//! - the kind on the wire is the underscored spelling (`pane_created`) while the
//!   same event is named with dots in a manifest's `[[events]] on` and in the
//!   daemon's plugin log ring (`pane.created`). [`Kind`] carries both, so
//!   nothing else has to derive one from the other.
//!
//! Only the events a plugin manifest may hook are subscribed to: that is the
//! set this panel exists for. One of them cannot be asked for — see
//! [`SUBSCRIBED`].
//!
//! A stream that cannot be opened says so in the panel. The first version of
//! this module recorded the reason and told no one, which looks exactly like a
//! daemon that has nothing to say.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, Sender};
use serde_json::{Value, json};

use crate::herdr::api::{ApiClient, ApiError};

/// How many events are held. The ring is what makes a burst readable after the
/// fact without keeping a session's whole history.
pub(crate) const RING: usize = 60;

/// How long to wait before asking again after a subscription ended. A daemon
/// that is restarting is back within a moment, and this costs nothing while it
/// is up.
const RECONNECT: Duration = Duration::from_secs(2);

/// One event a plugin manifest may hook, in both spellings the wire uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Kind {
    /// The daemon's dot name: what a manifest's `[[events]] on` writes and what
    /// the plugin log ring records for a hook run.
    pub(crate) dotted: &'static str,
    /// What the daemon stamps on a streamed line, which is the variant's name
    /// in snake_case (`herdr/src/api/schema/events.rs:193`).
    pub(crate) wire: &'static str,
}

/// The events this client subscribes to: the ones a manifest hook may name, in
/// the daemon's own order — `PLUGIN_HOOK_EVENT_KINDS`
/// (`herdr/src/api/schema/events.rs:286-309`) — less `pane.agent_status_changed`,
/// whose subscription demands a `pane_id` that a session-wide stream has none of:
/// asking for all 22 is refused with `invalid_request: missing field pane_id`
/// (measured). A plugin that hooks it still runs and its run still reaches the
/// log ring; what this stream cannot show is the event itself.
///
/// The table is held here because no method publishes it; a kind herdr adds
/// later stays invisible to this panel until the table grows, which is the
/// honest cost of not having a discovery call to make.
pub(crate) const SUBSCRIBED: &[Kind] = &[
    Kind {
        dotted: "workspace.created",
        wire: "workspace_created",
    },
    Kind {
        dotted: "workspace.updated",
        wire: "workspace_updated",
    },
    Kind {
        dotted: "workspace.closed",
        wire: "workspace_closed",
    },
    Kind {
        dotted: "workspace.renamed",
        wire: "workspace_renamed",
    },
    Kind {
        dotted: "workspace.moved",
        wire: "workspace_moved",
    },
    Kind {
        dotted: "workspace.reordered",
        wire: "workspace_reordered",
    },
    Kind {
        dotted: "workspace.focused",
        wire: "workspace_focused",
    },
    Kind {
        dotted: "worktree.created",
        wire: "worktree_created",
    },
    Kind {
        dotted: "worktree.opened",
        wire: "worktree_opened",
    },
    Kind {
        dotted: "worktree.removed",
        wire: "worktree_removed",
    },
    Kind {
        dotted: "tab.created",
        wire: "tab_created",
    },
    Kind {
        dotted: "tab.closed",
        wire: "tab_closed",
    },
    Kind {
        dotted: "tab.renamed",
        wire: "tab_renamed",
    },
    Kind {
        dotted: "tab.moved",
        wire: "tab_moved",
    },
    Kind {
        dotted: "tab.focused",
        wire: "tab_focused",
    },
    Kind {
        dotted: "pane.created",
        wire: "pane_created",
    },
    Kind {
        dotted: "pane.closed",
        wire: "pane_closed",
    },
    Kind {
        dotted: "pane.focused",
        wire: "pane_focused",
    },
    Kind {
        dotted: "pane.moved",
        wire: "pane_moved",
    },
    Kind {
        dotted: "pane.exited",
        wire: "pane_exited",
    },
    Kind {
        dotted: "pane.agent_detected",
        wire: "pane_agent_detected",
    },
];

/// The kind a streamed line names, when it is one of [`SUBSCRIBED`].
pub(crate) fn find(wire: &str) -> Option<&'static Kind> {
    SUBSCRIBED.iter().find(|kind| kind.wire == wire)
}

/// One line off the stream, as the panel reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Event {
    /// Epoch milliseconds when this client read the line. The daemon sends no
    /// time with an event, so this is arrival, not occurrence.
    pub(crate) at_unix_ms: u64,
    /// The dot name. An event that is not one of [`HOOKABLE`] keeps the wire's
    /// own spelling rather than being renamed into something it is not.
    pub(crate) kind: String,
    /// What the event was about, from its payload.
    pub(crate) detail: String,
}

/// Which slice of the stream the panel shows: everything, the agents, what a
/// plugin hooked, the session's structure, the layout. A view of the ring, not
/// a second subscription — the stream keeps reading all of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EventFilter {
    #[default]
    All,
    Agents,
    Plugins,
    Tabs,
    Layout,
}

impl EventFilter {
    /// The chips, in the order they are drawn.
    pub(crate) const ALL: [Self; 5] = [
        Self::All,
        Self::Agents,
        Self::Plugins,
        Self::Tabs,
        Self::Layout,
    ];

    /// The stable id the chip's element id and i18n key are built from.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Agents => "agents",
            Self::Plugins => "plugins",
            Self::Tabs => "tabs",
            Self::Layout => "layout",
        }
    }

    /// What the chip is called, from the `shell.event_filter` section of the
    /// locale file.
    pub(crate) fn label(self) -> String {
        crate::i18n::t(&format!("shell.event_filter.{}", self.id()))
    }

    /// Whether an event shows behind this chip. `hooked` answers whether an
    /// installed plugin hooks a kind; the plugins module owns that question,
    /// so it arrives rather than living here.
    pub(crate) fn matches(self, event: &Event, hooked: &dyn Fn(&str) -> bool) -> bool {
        self == Self::All || class(event, hooked) == self
    }
}

/// The one chip an event sits behind, decided once so a row cannot appear
/// under two of them: an agent event is the agent's, a layout event is the
/// layout's, one an installed plugin hooked is the plugin's, and the session's
/// own structure — workspaces, worktrees, tabs, panes — is what is left.
pub(crate) fn class(event: &Event, hooked: &dyn Fn(&str) -> bool) -> EventFilter {
    let kind = event.kind.as_str();
    if kind.contains("agent") {
        EventFilter::Agents
    } else if kind.starts_with("layout.") {
        EventFilter::Layout
    } else if hooked(kind) {
        EventFilter::Plugins
    } else {
        EventFilter::Tabs
    }
}

/// What the stream has read, as one value the shell clones per paint.
#[derive(Clone, Debug, Default)]
pub(crate) struct EventState {
    /// Newest first, which is the order a panel reads them in.
    pub(crate) events: VecDeque<Event>,
    /// Why the stream is not connected, until it is again.
    pub(crate) error: Option<String>,
    /// Which slice the panel is showing. Read by the paint, written by the
    /// dispatch — the worker below never touches it.
    pub(crate) filter: EventFilter,
}

/// The stream: one worker thread, one cache, one pulse channel.
pub(crate) struct Stream {
    cache: Arc<Mutex<EventState>>,
}

impl Stream {
    /// Start the stream and its worker. The returned receiver pulses once per
    /// event, so the shell repaints as they arrive instead of polling.
    pub(crate) fn start() -> (Self, Receiver<()>) {
        let cache = Arc::new(Mutex::new(EventState::default()));
        // Bounded at one: an event that arrives while the shell has not painted
        // the one before it needs no second wakeup.
        let (pulses_out, pulses) = crossbeam_channel::bounded(1);
        let worker_cache = cache.clone();
        thread::Builder::new()
            .name("zenix-events".into())
            .spawn(move || follow(ApiClient::local(), worker_cache, pulses_out))
            .expect("failed to spawn the event stream thread");
        (Self { cache }, pulses)
    }

    /// A copy of the ring for one paint.
    pub(crate) fn state(&self) -> EventState {
        lock(&self.cache).clone()
    }

    /// Drop what has been read. The subscription is untouched: the next event
    /// still arrives, which is what makes this clearing a view rather than
    /// stopping one.
    pub(crate) fn clear(&self) {
        lock(&self.cache).events.clear();
    }

    /// Move the view's filter. The stream reads everything either way; this
    /// decides what the panel shows of it.
    pub(crate) fn set_filter(&self, filter: EventFilter) {
        lock(&self.cache).filter = filter;
    }
}

/// Read the stream, and read it again whenever it ends. A subscription ends
/// when the daemon stops, and when it drops a subscriber that fell behind its
/// retained history; both are answered the same way.
fn follow(client: ApiClient, cache: Arc<Mutex<EventState>>, pulses: Sender<()>) {
    loop {
        let error = match read(&client, &cache, &pulses) {
            // The daemon closed the connection without saying why: say so in the
            // panel rather than showing a stream that quietly stopped.
            Ok(()) => "the daemon closed the event stream".to_owned(),
            Err(error) => error.to_string(),
        };
        set_error(&cache, Some(error), &pulses);
        thread::sleep(RECONNECT);
    }
}

/// Say why the stream is where it is, and wake the shell. An error nobody is
/// told about is a panel that silently shows nothing.
fn set_error(cache: &Mutex<EventState>, error: Option<String>, pulses: &Sender<()>) {
    lock(cache).error = error;
    let _ = pulses.try_send(());
}

fn read(
    client: &ApiClient,
    cache: &Mutex<EventState>,
    pulses: &Sender<()>,
) -> Result<(), ApiError> {
    let params = json!({
        "subscriptions": SUBSCRIBED
            .iter()
            .map(|kind| json!({"type": kind.dotted}))
            .collect::<Vec<_>>(),
    });
    let mut stream = client.subscribe(params)?;
    set_error(cache, None, pulses);
    while let Some(line) = stream.next_line()? {
        match event_from(&line, now_unix_ms()) {
            // The subscription's own acknowledgement is not an event, and the
            // daemon writes nothing else that is not one.
            Ok(None) => {}
            Ok(Some(event)) => {
                {
                    let mut state = lock(cache);
                    state.events.push_front(event);
                    state.events.truncate(RING);
                    state.error = None;
                }
                let _ = pulses.try_send(());
            }
            Err(error) => {
                lock(cache).error = Some(error);
                return Ok(());
            }
        }
    }
    Ok(())
}

/// One line as an event. `Ok(None)` is a line that is not an event; `Err` is a
/// line that says why the stream is over.
fn event_from(line: &str, at_unix_ms: u64) -> Result<Option<Event>, String> {
    let value: Value = serde_json::from_str(line.trim_end())
        .map_err(|error| format!("the event stream wrote something that is not JSON: {error}"))?;
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("error");
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(format!("{code}: {message}"));
    }
    let Some(wire) = value.get("event").and_then(Value::as_str) else {
        return Ok(None);
    };
    let kind = find(wire).map_or_else(|| wire.to_owned(), |kind| kind.dotted.to_owned());
    let data = value.get("data").unwrap_or(&Value::Null);
    Ok(Some(Event {
        at_unix_ms,
        kind,
        detail: detail(data),
    }))
}

/// What an event was about, as the dim line beside its kind: the id its payload
/// names, plus the label or agent status that came with it. One function reads
/// every kind in [`SUBSCRIBED`], because they share these fields rather than each
/// carrying a shape of its own.
fn detail(data: &Value) -> String {
    let subject = ["pane", "tab", "workspace", "layout"]
        .iter()
        .find_map(|key| data.get(*key))
        .unwrap_or(data);
    let mut parts = Vec::new();
    if let Some(id) = ["pane_id", "tab_id", "workspace_id"]
        .iter()
        .find_map(|key| subject.get(*key).and_then(Value::as_str))
    {
        parts.push(id.to_owned());
    }
    // `unknown` is the daemon's word for "no agent detected here", which is the
    // state of most panes: on every row it is noise rather than information.
    for key in ["label", "agent_status"] {
        if let Some(value) = subject
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && *value != "unknown")
        {
            parts.push(value.to_owned());
        }
    }
    parts.join(" · ")
}

/// Epoch milliseconds, for when a line arrived.
pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// How long ago an event arrived, short enough for a sidebar row.
pub(crate) fn age(now_unix_ms: u64, at_unix_ms: u64) -> String {
    let seconds = now_unix_ms.saturating_sub(at_unix_ms) / 1000;
    match seconds {
        0..=1 => "now".to_owned(),
        2..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86_399 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86_400),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two spellings are not two lists that happen to agree: the wire name
    /// is the dot name's separators turned into underscores, because both come
    /// from the same variant. A typo in the table would break that.
    #[test]
    fn the_table_is_the_dot_name_of_every_hookable_event() {
        // The daemon's hookable set has 22 names; the one it cannot stream
        // without a pane id is the one missing here.
        assert_eq!(SUBSCRIBED.len(), 21);
        for kind in SUBSCRIBED {
            assert_eq!(
                kind.wire,
                kind.dotted.replace('.', "_"),
                "{} is the same event as {}",
                kind.dotted,
                kind.wire
            );
        }
    }

    /// Asking for this one is refused with `missing field pane_id`: a manifest
    /// may hook it, but a session-wide stream cannot subscribe to it.
    #[test]
    fn the_event_that_needs_a_pane_is_not_subscribed_to() {
        assert!(find("pane_agent_status_changed").is_none());
        assert!(
            SUBSCRIBED
                .iter()
                .all(|kind| kind.dotted != "pane.agent_status_changed")
        );
    }

    #[test]
    fn both_spellings_are_unique() {
        for (index, kind) in SUBSCRIBED.iter().enumerate() {
            for other in &SUBSCRIBED[index + 1..] {
                assert_ne!(kind.dotted, other.dotted);
                assert_ne!(kind.wire, other.wire);
            }
        }
    }

    #[test]
    fn a_wire_name_finds_its_kind() {
        let kind = find("pane_agent_detected").expect("a known kind");
        assert_eq!(kind.dotted, "pane.agent_detected");
        assert!(find("pane.output_changed").is_none(), "not hookable");
    }

    #[test]
    fn an_event_line_becomes_an_event() {
        let line = r#"{"data":{"pane":{"pane_id":"w1:p4","label":"build","agent_status":"blocked"},"type":"pane_created"},"event":"pane_created"}"#;
        let event = event_from(line, 1_000).expect("a line").expect("an event");
        assert_eq!(event.kind, "pane.created");
        assert_eq!(event.detail, "w1:p4 · build · blocked");
        assert_eq!(event.at_unix_ms, 1_000);
    }

    #[test]
    fn the_subscriptions_acknowledgement_is_not_an_event() {
        let line = r#"{"id":"zenix-1-1","result":{"type":"subscription_started"}}"#;
        assert_eq!(event_from(line, 1_000), Ok(None));
    }

    /// The daemon ends a subscription that fell behind with an error line and
    /// then closes, so the reason has to survive as more than "it stopped".
    #[test]
    fn an_error_line_says_why_the_stream_is_over() {
        let line = r#"{"id":"zenix-1-1","error":{"code":"events_lost","message":"fell behind"}}"#;
        assert_eq!(
            event_from(line, 1_000),
            Err("events_lost: fell behind".to_owned())
        );
    }

    #[test]
    fn a_line_that_is_not_json_is_reported_as_such() {
        let error = event_from("not json", 1_000).expect_err("not a line");
        assert!(error.contains("not JSON"), "{error}");
    }

    /// An event this client did not ask for — a kind herdr added after this
    /// table was written — keeps its own name instead of being mislabelled.
    #[test]
    fn an_unknown_kind_keeps_the_name_the_daemon_used() {
        let line = r#"{"data":{"pane_id":"w1:p1"},"event":"pane.output_changed"}"#;
        let event = event_from(line, 1_000).expect("a line").expect("an event");
        assert_eq!(event.kind, "pane.output_changed");
        assert_eq!(event.detail, "w1:p1");
    }

    #[test]
    fn a_payload_without_a_nested_subject_still_reads() {
        let line = r#"{"data":{"tab_id":"w1:t2","type":"tab_focused"},"event":"tab_focused"}"#;
        let event = event_from(line, 1_000).expect("a line").expect("an event");
        assert_eq!(event.kind, "tab.focused");
        assert_eq!(event.detail, "w1:t2");
    }

    /// A pane with no agent in it reports `unknown`, which is the state of most
    /// panes: the row keeps the id and drops the word.
    #[test]
    fn an_undetected_agent_status_is_not_part_of_the_detail() {
        let line = r#"{"data":{"pane":{"pane_id":"w1:p2","agent_status":"unknown"},"type":"pane_created"},"event":"pane_created"}"#;
        let event = event_from(line, 1_000).expect("a line").expect("an event");
        assert_eq!(event.detail, "w1:p2");
    }

    #[test]
    fn ages_step_from_seconds_to_minutes_to_hours() {
        assert_eq!(age(1_000, 1_000), "now");
        assert_eq!(age(1_500, 1_000), "now");
        assert_eq!(age(12_000, 1_000), "11s");
        assert_eq!(age(60_000, 1_000), "59s");
        assert_eq!(age(121_000, 1_000), "2m");
        assert_eq!(age(7_200_000, 1_000), "1h");
        assert_eq!(age(172_800_000, 1_000), "1d");
        // A clock that moved backwards must not print a negative age.
        assert_eq!(age(1_000, 9_000), "now");
    }

    /// An event sits behind exactly one chip: the agent's, the layout's, the
    /// hooking plugin's, or the session structure's — decided once, so a row
    /// cannot show twice.
    #[test]
    fn an_event_shows_behind_exactly_one_chip() {
        let none = |_kind: &str| false;
        let hooked = |kind: &str| kind == "tab.created";
        let event = |kind: &str| Event {
            at_unix_ms: 1,
            kind: kind.to_owned(),
            detail: String::new(),
        };
        assert_eq!(
            class(&event("pane.agent_detected"), &none),
            EventFilter::Agents
        );
        assert_eq!(class(&event("layout.applied"), &none), EventFilter::Layout);
        assert_eq!(class(&event("tab.created"), &hooked), EventFilter::Plugins);
        // An agent event stays the agent's even when a plugin hooks it, and
        // the session's own structure is what no other claim leaves.
        assert_eq!(
            class(&event("pane.agent_detected"), &hooked),
            EventFilter::Agents
        );
        assert_eq!(class(&event("workspace.created"), &none), EventFilter::Tabs);
        // "All" shows everything; a specific chip shows only its own class.
        assert!(EventFilter::All.matches(&event("workspace.created"), &none));
        assert!(EventFilter::Tabs.matches(&event("workspace.created"), &none));
        assert!(!EventFilter::Agents.matches(&event("workspace.created"), &none));
        assert!(EventFilter::Plugins.matches(&event("tab.created"), &hooked));
        assert!(!EventFilter::Plugins.matches(&event("tab.closed"), &hooked));
    }

    /// The filter is a view of the ring: the state carries it, the stream's
    /// reads never overwrite it, and it starts at everything.
    #[test]
    fn the_filter_starts_at_everything_and_survives_a_read() {
        let state = EventState::default();
        assert_eq!(state.filter, EventFilter::All);
        let read = EventState {
            events: VecDeque::new(),
            error: None,
            filter: EventFilter::Plugins,
        };
        assert_eq!(read.filter, EventFilter::Plugins, "a read keeps the view");
    }
}
