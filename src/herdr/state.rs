//! The daemon's pushed truth: one snapshot, and the surface that belongs to it.
//!
//! Ported from herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/state.rs`. Only
//! the branches a terminal pane needs are carried over; notifications, sounds,
//! integrations and agent recency stay upstream.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use herdr_client::ClientEvent;
use herdr_client::protocol::{ClientShellSnapshot, PaneSurfaceFrame, ServerMessage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionStatus {
    Connecting,
    AwaitingSnapshot,
    Connected,
    Disconnected,
}

impl ConnectionStatus {
    pub(crate) fn is_connected(self) -> bool {
        matches!(self, Self::AwaitingSnapshot | Self::Connected)
    }
}

/// The live view of one daemon connection. Cloned out of the connection's
/// mutex for each paint, so a long paint never holds the reader's lock.
#[derive(Clone)]
pub(crate) struct LiveState {
    pub(crate) snapshot: Option<Arc<ClientShellSnapshot>>,
    /// The surface the snapshot describes. `None` until a coherent one lands.
    pub(crate) surface: Option<Arc<PaneSurfaceFrame>>,
    pub(crate) status: ConnectionStatus,
    pub(crate) error: Option<String>,
    pub(crate) version_mismatch: Option<herdr_client::VersionMismatch>,
    /// Raw base64 OSC 52 payloads waiting to be written to the system
    /// clipboard, oldest first, bounded at `osc52::MAX_PENDING`: every write
    /// replaces the whole clipboard, so only the tail can still matter.
    /// Decoding stays with the view; only the queue bound is shared.
    pub(crate) clipboard_writes: VecDeque<String>,
    /// Bumped whenever a paint-relevant field changed, so a view can tell
    /// whether it already drew this state.
    pub(crate) revision: u64,
    /// Request ids the daemon has not answered yet. A shell action that must
    /// not be queued twice while one is already travelling (a split drag)
    /// reads this instead of keeping its own flag.
    pub(crate) in_flight: BTreeSet<String>,
    /// The last command the daemon refused, with its reason. Cleared by the
    /// next request the daemon accepts.
    pub(crate) rejected: Option<String>,
    /// Answers to commands a caller is waiting on, by request id. A caller that
    /// needs the daemon's payload (an orchestration tool reporting what a pane
    /// printed, say) parks on the revision and then takes its answer; a caller
    /// that only queues a command never touches this.
    answers: HashMap<String, serde_json::Value>,
    /// Insertion order of `answers`, so the map stays bounded on a long-lived
    /// connection where some answers are never taken.
    answer_order: VecDeque<String>,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            snapshot: None,
            surface: None,
            status: ConnectionStatus::Connecting,
            error: None,
            version_mismatch: None,
            clipboard_writes: VecDeque::new(),
            revision: 0,
            in_flight: BTreeSet::new(),
            rejected: None,
            answers: HashMap::new(),
            answer_order: VecDeque::new(),
        }
    }
}

impl LiveState {
    pub(crate) fn boot_id(&self) -> Option<&str> {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.boot_id.as_str())
    }

    pub(crate) fn apply(&mut self, event: ClientEvent) {
        match event {
            ClientEvent::Connected(_) => {
                self.status = ConnectionStatus::AwaitingSnapshot;
                self.error = None;
                self.version_mismatch = None;
            }
            ClientEvent::Snapshot(snapshot) => {
                // A surface from another boot or projection cannot be painted
                // against this snapshot; wait for the matching one.
                if self
                    .surface
                    .as_ref()
                    .is_some_and(|surface| !coherent(&snapshot, surface))
                {
                    self.surface = None;
                }
                self.snapshot = Some(snapshot);
                self.status = ConnectionStatus::Connected;
                self.error = None;
                self.revision += 1;
            }
            ClientEvent::Surface(surface) => {
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| coherent(snapshot, &surface))
                {
                    self.surface = Some(surface);
                    self.revision += 1;
                }
            }
            // Sent just before the `Disconnected` that carries the text.
            ClientEvent::VersionMismatch(mismatch) => self.version_mismatch = Some(mismatch),
            // The daemon answered a command: the request is no longer in
            // flight, which a split drag waits on. A refused command comes back
            // as this same answer with an error body rather than a separate
            // event, so its reason is read from the payload and kept for the
            // status bar; without that the command would just vanish.
            ClientEvent::Response {
                request_id,
                response,
            } => {
                self.in_flight.remove(&request_id);
                if let Some(message) = error_message(&response) {
                    self.rejected = Some(message.to_owned());
                }
                // The payload is kept for a caller waiting on this answer, and
                // the revision moves so that waiter wakes instead of spinning.
                self.remember_answer(request_id, response);
                self.revision += 1;
            }
            ClientEvent::CommandRejected { request_id, reason } => {
                if let Some(request_id) = &request_id {
                    self.in_flight.remove(request_id);
                }
                self.rejected = Some(reason.to_string());
                self.revision += 1;
            }
            ClientEvent::Disconnected { reason, .. } => {
                self.status = ConnectionStatus::Disconnected;
                self.error = Some(reason);
                self.snapshot = None;
                self.surface = None;
                // Requests queued on the connection that just ended will never
                // be answered, so nothing may keep waiting on them.
                self.in_flight.clear();
                self.revision += 1;
            }
            ClientEvent::Message(ServerMessage::Clipboard { data }) => {
                // OSC 52 bytes from a pane, base64-encoded by the daemon.
                // Every write replaces the whole clipboard, so only the tail
                // can still matter: drop the oldest past the bound.
                while self.clipboard_writes.len() >= super::osc52::MAX_PENDING {
                    self.clipboard_writes.pop_front();
                }
                self.clipboard_writes.push_back(data);
                // `revision` is deliberately not bumped: the view drains this
                // queue unconditionally on every pull, so a repaint is not
                // needed for the payload to be noticed.
            }
            _ => {}
        }
    }

    /// Remove and return the queued clipboard payloads, oldest first. They
    /// stay base64 here; the view decodes them, keeping this module free of
    /// the base64 rules.
    pub(crate) fn take_clipboard(&mut self) -> Vec<String> {
        self.clipboard_writes.drain(..).collect()
    }

    /// Keep one answer for a waiting caller, bounded to the newest few: most
    /// answers belong to callers that only needed the queueing to succeed.
    fn remember_answer(&mut self, request_id: String, response: serde_json::Value) {
        const MAX_ANSWERS: usize = 16;
        if !self.answers.contains_key(&request_id) {
            self.answer_order.push_back(request_id.clone());
        }
        self.answers.insert(request_id, response);
        while self.answer_order.len() > MAX_ANSWERS {
            if let Some(expired) = self.answer_order.pop_front() {
                self.answers.remove(&expired);
            }
        }
    }

    /// Remove and return the answer to `request_id`, if it has arrived.
    pub(crate) fn take_answer(&mut self, request_id: &str) -> Option<serde_json::Value> {
        let answer = self.answers.remove(request_id)?;
        self.answer_order.retain(|id| id != request_id);
        Some(answer)
    }

    /// Record a command the daemon has been handed. The rejection of an
    /// earlier command no longer describes what is happening.
    pub(crate) fn note_request(&mut self, request_id: String) {
        self.in_flight.insert(request_id);
        self.rejected = None;
    }

    /// Whether a command is still waiting for its answer.
    pub(crate) fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
    }
}

/// The message an error answer carries, as `{"error": {"code", "message"}}`.
/// A successful answer's payload names no error.
fn error_message(response: &serde_json::Value) -> Option<&str> {
    response
        .get("error")
        .filter(|error| !error.is_null())
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
}

/// Whether a surface describes the same boot and projection as `snapshot`.
fn coherent(snapshot: &ClientShellSnapshot, surface: &PaneSurfaceFrame) -> bool {
    revisions_match(
        &snapshot.boot_id,
        snapshot.revision,
        &surface.boot_id,
        surface.projection_revision,
    )
}

/// The one invariant every paint depends on: a frame is only drawn against the
/// snapshot whose boot and revision it was projected from. Split out from
/// [`coherent`] so it can be tested without building protocol fixtures.
fn revisions_match(boot_id: &str, revision: u64, frame_boot_id: &str, projection: u64) -> bool {
    boot_id == frame_boot_id && revision == projection
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_surface_from_another_projection_is_not_accepted() {
        assert!(!revisions_match("boot", 3, "boot", 2));
        assert!(revisions_match("boot", 3, "boot", 3));
    }

    #[test]
    fn a_surface_from_another_boot_is_not_accepted() {
        assert!(!revisions_match("boot-a", 1, "boot-b", 1));
    }

    #[test]
    fn a_disconnect_clears_the_state_it_owns() {
        let mut state = LiveState::default();
        state.status = ConnectionStatus::Connected;
        state.apply(ClientEvent::Disconnected {
            reason: "closed".into(),
            ssh: None,
        });
        assert_eq!(state.status, ConnectionStatus::Disconnected);
        assert_eq!(state.error.as_deref(), Some("closed"));
        assert!(state.snapshot.is_none());
        assert!(state.surface.is_none());
    }

    #[test]
    fn a_clipboard_message_queues_one_payload() {
        let mut state = LiveState::default();
        state.apply(ClientEvent::Message(ServerMessage::Clipboard {
            data: "dGVzdA==".into(),
        }));
        assert_eq!(state.take_clipboard(), ["dGVzdA=="]);
    }

    #[test]
    fn more_clipboard_messages_than_the_bound_keep_the_newest_in_order() {
        let mut state = LiveState::default();
        let bound = crate::herdr::osc52::MAX_PENDING;
        let total = bound * 2;
        for index in 0..total {
            state.apply(ClientEvent::Message(ServerMessage::Clipboard {
                data: index.to_string(),
            }));
        }
        let expected: Vec<String> = (total - bound..total)
            .map(|index| index.to_string())
            .collect();
        assert_eq!(state.take_clipboard(), expected);
    }

    #[test]
    fn taking_the_clipboard_drains_the_queue() {
        let mut state = LiveState::default();
        state.apply(ClientEvent::Message(ServerMessage::Clipboard {
            data: "dGVzdA==".into(),
        }));
        assert_eq!(state.take_clipboard(), ["dGVzdA=="]);
        assert!(state.take_clipboard().is_empty());
    }

    #[test]
    fn an_answer_retires_the_request_it_names() {
        let mut state = LiveState::default();
        state.note_request("request-1".into());
        assert!(state.has_in_flight());
        state.apply(ClientEvent::Response {
            request_id: "request-1".into(),
            response: serde_json::json!({}),
        });
        assert!(!state.has_in_flight());
    }

    #[test]
    fn an_error_answer_keeps_the_reason_the_daemon_gave() {
        let mut state = LiveState::default();
        state.note_request("request-1".into());
        state.apply(ClientEvent::Response {
            request_id: "request-1".into(),
            response: serde_json::json!({
                "id": "request-1",
                "error": {
                    "code": "workspace_group_close_required",
                    "message": "workspace has linked worktree workspaces",
                },
            }),
        });
        assert!(!state.has_in_flight());
        assert_eq!(
            state.rejected.as_deref(),
            Some("workspace has linked worktree workspaces")
        );
    }

    #[test]
    fn a_successful_answer_leaves_no_rejection_behind() {
        let mut state = LiveState::default();
        state.note_request("request-1".into());
        state.apply(ClientEvent::Response {
            request_id: "request-1".into(),
            response: serde_json::json!({"id": "request-1", "result": {"type": "ok"}}),
        });
        assert!(state.rejected.is_none());
    }

    #[test]
    fn an_answer_is_kept_for_its_caller_and_taken_once() {
        let mut state = LiveState::default();
        let before = state.revision;
        state.note_request("request-1".into());
        state.apply(ClientEvent::Response {
            request_id: "request-1".into(),
            response: serde_json::json!({
                "id": "request-1",
                "result": {"type": "pane_read", "read": {"text": "hello"}},
            }),
        });
        assert!(state.revision > before, "a waiter parks on the revision");
        let answer = state.take_answer("request-1").expect("the answer landed");
        assert_eq!(answer["result"]["read"]["text"], "hello");
        assert!(state.take_answer("request-1").is_none());
    }

    #[test]
    fn an_untaken_answer_does_not_grow_without_bound() {
        let mut state = LiveState::default();
        for index in 0..20 {
            state.apply(ClientEvent::Response {
                request_id: format!("request-{index}"),
                response: serde_json::json!({"id": format!("request-{index}")}),
            });
        }
        assert!(state.take_answer("request-0").is_none());
        assert!(state.take_answer("request-19").is_some());
    }

    #[test]
    fn a_null_error_is_not_a_rejection() {
        let mut state = LiveState::default();
        state.apply(ClientEvent::Response {
            request_id: "request-1".into(),
            response: serde_json::json!({"id": "request-1", "error": null}),
        });
        assert!(state.rejected.is_none());
    }

    #[test]
    fn an_answer_to_another_request_leaves_this_one_waiting() {
        let mut state = LiveState::default();
        state.note_request("request-1".into());
        state.apply(ClientEvent::Response {
            request_id: "request-2".into(),
            response: serde_json::json!({}),
        });
        assert!(state.has_in_flight());
    }

    #[test]
    fn a_rejection_is_kept_as_the_reason_it_carries() {
        let mut state = LiveState::default();
        state.note_request("request-1".into());
        state.apply(ClientEvent::CommandRejected {
            request_id: Some("request-1".into()),
            reason: herdr_client::Error::UnsupportedMethod,
        });
        assert!(!state.has_in_flight());
        assert!(state.rejected.is_some());
    }

    #[test]
    fn a_new_request_clears_the_previous_rejection() {
        let mut state = LiveState::default();
        state.apply(ClientEvent::CommandRejected {
            request_id: None,
            reason: herdr_client::Error::UnsupportedMethod,
        });
        assert!(state.rejected.is_some());
        state.note_request("request-2".into());
        assert!(state.rejected.is_none());
    }

    #[test]
    fn a_disconnect_retires_every_request_in_flight() {
        let mut state = LiveState::default();
        state.note_request("request-1".into());
        state.apply(ClientEvent::Disconnected {
            reason: "closed".into(),
            ssh: None,
        });
        assert!(!state.has_in_flight());
    }
}
