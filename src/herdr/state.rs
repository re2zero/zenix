//! The daemon's pushed truth: one snapshot, and the surface that belongs to it.
//!
//! Ported from herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/state.rs`. The
//! branches a terminal pane needs, plus the daemon's notifications, titles and
//! bells, are carried over; sound playback, integrations and agent recency
//! stay upstream.

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use herdr_client::ClientEvent;
use herdr_client::protocol::{
    ClientShellSnapshot, PaneSurfaceFrame, SemanticNotification, ServerMessage,
};

/// A system notification the daemon asked for, once each. The view drains
/// these and hands them to the platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Notice {
    pub(crate) tag: String,
    pub(crate) title: String,
    pub(crate) body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionStatus {
    Connecting,
    AwaitingSnapshot,
    Connected,
    Disconnected,
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
    /// The last answers the daemon sent, kept bounded so a caller that owns a
    /// request (a link activation) can read its payload after the view's
    /// pull. The reader never waits for anyone to take one.
    pub(crate) responses: VecDeque<(String, serde_json::Value)>,
    /// Notifications the daemon asked the platform to show, oldest first.
    pub(crate) notices: VecDeque<Notice>,
    /// The title the focused terminal reported, if any.
    pub(crate) title: Option<String>,
    /// Bells the daemon forwarded, and the notifications that asked for
    /// attention: the status bar reads both as an unread count.
    pub(crate) bells: u64,
    pub(crate) attention: u64,
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
            responses: VecDeque::new(),
            notices: VecDeque::new(),
            title: None,
            bells: 0,
            attention: 0,
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
                if self.responses.len() >= MAX_RESPONSES {
                    self.responses.pop_front();
                }
                self.responses.push_back((request_id, response));
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
                // The picture the last snapshot painted stays: a reconnect
                // negotiates a new snapshot before its first frame, and until
                // then the shell dims what it has and says it is reconnecting.
                // Keeping it is what turns a drop into a state, not a blank
                // window.
                self.status = ConnectionStatus::Disconnected;
                self.error = Some(reason);
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
            ClientEvent::Message(ServerMessage::Notify {
                kind,
                message,
                body,
            }) => {
                use herdr_protocol::NotifyKind;
                match kind {
                    NotifyKind::Toast | NotifyKind::SystemToast => {
                        self.push_notice(Notice {
                            tag: "zenix-herdr-notify".into(),
                            title: message,
                            body: body.unwrap_or_default(),
                        });
                    }
                    // There is no sound player in this client; the request is
                    // recorded rather than silently dropped by the catch-all.
                    NotifyKind::Sound => {
                        tracing::debug!(message, "daemon asked for a sound");
                        self.bells = self.bells.saturating_add(1);
                        self.revision += 1;
                    }
                }
            }
            ClientEvent::Message(ServerMessage::SemanticNotification(notification)) => {
                self.apply_semantic(notification);
            }
            ClientEvent::Message(ServerMessage::TerminalBell { count }) => {
                self.bells = self.bells.saturating_add(u64::from(count.max(1)));
                self.revision += 1;
            }
            ClientEvent::Message(ServerMessage::WindowTitle { title }) if self.title != title => {
                self.title = title;
                self.revision += 1;
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

    /// Remove and return the notifications waiting to reach the platform.
    pub(crate) fn take_notices(&mut self) -> Vec<Notice> {
        self.notices.drain(..).collect()
    }

    /// Remove and return the answer to `request_id`, if it is still held.
    pub(crate) fn take_response(&mut self, request_id: &str) -> Option<serde_json::Value> {
        let index = self.responses.iter().position(|(id, _)| id == request_id)?;
        self.responses.remove(index).map(|(_, response)| response)
    }

    /// Whether the picture on screen is the last one a connection painted,
    /// rather than one the live daemon currently stands behind.
    pub(crate) fn stale(&self) -> bool {
        self.status == ConnectionStatus::Disconnected && self.snapshot.is_some()
    }

    fn push_notice(&mut self, notice: Notice) {
        while self.notices.len() >= MAX_NOTICES {
            self.notices.pop_front();
        }
        self.notices.push_back(notice);
    }

    /// A notification the daemon already resolved against its own session:
    /// its title and body are what the user sees, and an agent asking for
    /// attention also lights the rail's unread count.
    fn apply_semantic(&mut self, notification: SemanticNotification) {
        use herdr_protocol::SemanticNotificationKind;
        if matches!(notification.kind, SemanticNotificationKind::NeedsAttention) {
            self.attention = self.attention.saturating_add(1);
            self.revision += 1;
        }
        let tag = notification
            .pane_id
            .or(notification.agent)
            .or(notification.tab_id)
            .or(notification.workspace_id)
            .unwrap_or_else(|| "zenix-herdr".into());
        self.push_notice(Notice {
            tag: format!("zenix-{tag}"),
            title: notification.title,
            body: notification.body.unwrap_or_default(),
        });
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

/// How many answers are kept for a caller that owns one request (a link
/// activation). Enough that a user-driven request never loses its answer to a
/// burst of shell commands.
const MAX_RESPONSES: usize = 64;
/// How many platform notifications may wait for the next pull.
const MAX_NOTICES: usize = 16;

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

    fn snapshot(boot: &str) -> ClientShellSnapshot {
        serde_json::from_value(serde_json::json!({
            "boot_id": boot,
            "revision": 1,
            "update_install_command": "",
            "latest_release_notes_available": false,
            "integration_updates_available": false,
            "worktree_directory": "",
            "tab_bar_right_separator": "",
            "tab_bar_right": [],
            "agent_order": [],
            "workspaces": [],
            "tabs": [],
            "panes": [],
            "agents": [],
            "commands": [],
        }))
        .expect("a snapshot with no workspaces still deserializes")
    }

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
    fn a_disconnect_keeps_the_last_picture_and_marks_it_stale() {
        let mut state = LiveState::default();
        state.apply(ClientEvent::Snapshot(Arc::new(snapshot("boot"))));
        state.apply(ClientEvent::Disconnected {
            reason: "closed".into(),
            ssh: None,
        });
        assert_eq!(state.status, ConnectionStatus::Disconnected);
        assert_eq!(state.error.as_deref(), Some("closed"));
        // The surface stays painted; the shell dims it and says it is
        // reconnecting instead of blanking the window.
        assert!(state.snapshot.is_some());
        assert!(state.stale());
    }

    #[test]
    fn a_terminal_title_and_a_bell_are_kept() {
        let mut state = LiveState::default();
        state.apply(ClientEvent::Message(ServerMessage::WindowTitle {
            title: Some("vim src/main.rs".into()),
        }));
        assert_eq!(state.title.as_deref(), Some("vim src/main.rs"));
        state.apply(ClientEvent::Message(ServerMessage::TerminalBell {
            count: 2,
        }));
        assert_eq!(state.bells, 2);
    }

    #[test]
    fn a_notification_becomes_a_bounded_platform_notice() {
        let mut state = LiveState::default();
        for index in 0..MAX_NOTICES + 3 {
            state.apply(ClientEvent::Message(ServerMessage::Notify {
                kind: herdr_protocol::NotifyKind::Toast,
                message: format!("toast {index}"),
                body: None,
            }));
        }
        let notices = state.take_notices();
        assert_eq!(notices.len(), MAX_NOTICES);
        assert_eq!(notices.first().map(|n| n.title.as_str()), Some("toast 3"));
        assert!(state.take_notices().is_empty());
    }

    #[test]
    fn a_semantic_notification_needing_attention_lights_the_count() {
        let mut state = LiveState::default();
        state.apply(ClientEvent::Message(ServerMessage::SemanticNotification(
            SemanticNotification {
                kind: herdr_protocol::SemanticNotificationKind::NeedsAttention,
                title: "agent blocked".into(),
                body: Some("waiting for input".into()),
                sound: None,
                agent: Some("claude".into()),
                workspace_id: None,
                tab_id: None,
                pane_id: Some("p1".into()),
                position: None,
            },
        )));
        assert_eq!(state.attention, 1);
        let notices = state.take_notices();
        assert_eq!(notices[0].tag, "zenix-p1");
        assert_eq!(notices[0].body, "waiting for input");
    }

    #[test]
    fn an_answer_is_held_for_its_owner_and_taken_once() {
        let mut state = LiveState::default();
        state.apply(ClientEvent::Response {
            request_id: "request-1".into(),
            response: serde_json::json!({"result": {"type": "pane_link_activated"}}),
        });
        let answer = state.take_response("request-1").unwrap();
        assert_eq!(answer["result"]["type"], "pane_link_activated");
        assert!(state.take_response("request-1").is_none());
        assert!(state.take_response("request-2").is_none());
    }

    #[test]
    fn more_answers_than_the_bound_keep_the_newest() {
        let mut state = LiveState::default();
        for index in 0..MAX_RESPONSES + 5 {
            state.apply(ClientEvent::Response {
                request_id: format!("request-{index}"),
                response: serde_json::json!({"index": index}),
            });
        }
        assert!(state.take_response("request-0").is_none());
        assert_eq!(
            state
                .take_response(&format!("request-{}", MAX_RESPONSES + 4))
                .unwrap()["index"],
            MAX_RESPONSES + 4
        );
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
