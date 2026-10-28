//! The aside's conversation: what was said, what the turn is doing, and the
//! tools it used.
//!
//! The domain under this file is UI-free and already in place (`crate::ai`):
//! [`OrchestratorEngine`] streams a turn, `crate::ai::tools` are the six herdr
//! tools the model may call, `SessionManager` saves what was said. What this
//! module owns is the conversation as the reader sees it — one message list,
//! the reasoning a turn thought with, a card per tool call, the gate a call
//! waits at — and the turn's own lifecycle (send, stream, stop, persist).
//!
//! Three decisions are worth naming:
//!
//! - A turn runs on tokio ([`crate::TOKIO_HANDLE`]) because the engine does
//!   HTTP. Its events come back over a channel a GPUI task drains, the same
//!   shape [`crate::terminal::view::TerminalView`] uses for the daemon's
//!   pushes: the view is re-rendered because an event arrived, not because a
//!   frame was due.
//! - Tool calls act on the daemon's *focused* workspace: the executor is built
//!   with an empty workspace id, which [`ToolExecutor`] documents as "whatever
//!   the daemon has focused". The view therefore needs no snapshot, and never
//!   mutates its own state while it draws.
//! - A message carries the database's own vocabulary (`user` / `orchestrator`,
//!   the only two roles `src/ai/session.rs` accepts); what is *written* above
//!   it is this view's business. Tool names and statuses are shown as the wire
//!   spells them (`spawn_worker`, `running`), so a card can be read against
//!   what the model asked for; the surrounding copy goes through
//!   `crate::i18n`.
//!
//! Several library components are deliberately composed rather than used
//! whole, because their copy is English and the chrome's copy is the reader's:
//! the empty state (`WelcomeScreen` prints a fixed "Good morning"), the
//! reasoning header (`ThinkingDuration` prints "Thought for …"), the tool card
//! (`ToolCallCard` labels its folds "Arguments" / "Result" / "Error"), the
//! error card (`ErrorMessage` says "Try again"), the approval gate
//! (`PermissionPrompt`'s three answers are English) and the message footer's
//! actions (`MessageActions` tooltips). The library still draws the frames:
//! `ChatContainer`, `MessageList`, `MessageBubble`, `StreamingMarkdown`,
//! `StreamingText`, `SuggestionChips`, `ThinkingIndicator`, `CodeBlock`,
//! `step_mark` and `InlineEdit`.
//!
//! Deliberately not here yet: `@` workspace mentions, attachments and session
//! pinning. The store has no pinned flag, and a pin the file forgets would be
//! a control that lies.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use ely_chat::{
    ChatContainer, MessageAvatar, MessageBubble, MessageFooter, MessageHeader, MessageList,
    PromptInput, Role, StepState, StreamingMarkdown, StreamingText, SuggestionChips,
    ThinkingIndicator, step_mark,
};
use ely_core::buttons::{Button, ButtonVariant, IconButton};
use ely_core::forms::{Choice, InlineEdit, InputEvent, TextInput};
use ely_core::primitives::{Disclosure, Icon, IconName};
use ely_core::theme::{ActiveTheme, ControlSize, IconSize, Radius, TextSize};
use ely_core::typography::{Ellipsis, relative_text};
use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, Task, WeakEntity, Window, div, prelude::*, px,
};

use crate::ai::compactor::Compactor;
use crate::ai::endpoint::ApiEndpoint;
use crate::ai::engine::{
    ChatMessageRef, CompactionInfo, DispatchStreamEvent, OrchestratorEngine, ToolApproval,
    build_system_prompt, detect_language,
};
use crate::ai::roles::RoleRegistry;
use crate::ai::session::{
    ChatMessage, Session, SessionManager, ToolCallState, ToolCallStatus, UsageInfo,
};
use crate::ai::tools::{Tool, ToolCall, ToolExecutor, builtin_tools};
use crate::api_provider::{self, ApiProviderConfig, ThinkingLevel};
use crate::config;
use crate::i18n::t;

/// How many tool rounds one turn may take. The engine has its own ceiling; this
/// is the tighter one the panel asks for.
const MAX_TOOL_ROUNDS: u32 = 8;

/// The conversation, as the aside mounts it.
pub(crate) struct ChatView {
    /// Everything said, oldest first. The message the running turn appends to
    /// is `streaming`.
    messages: Vec<ChatMessage>,
    streaming: Option<usize>,
    /// The library's field: multi-line, IME-aware, and the composer's own
    /// submit rule (Enter sends, Shift-Enter breaks the line).
    input: Entity<TextInput>,
    /// Every provider that carries a key — the catalog this client has, read
    /// from the same file the settings page writes.
    providers: Vec<ApiProviderConfig>,
    /// The provider the next turn runs on. Read once when the view is built
    /// and re-read when the panel opens; a picker's choice is then owned by
    /// the view, so a re-read cannot overwrite what the user just chose.
    provider: Option<ApiProviderConfig>,
    /// Why the last send or the last turn failed, until the next one starts.
    error: Option<String>,
    /// What the running turn compacted away, if it had to.
    compacted: Option<CompactionInfo>,
    /// When the streaming turn's first reasoning token arrived, so the block
    /// can say how long it thought for.
    reasoning_started: Option<u64>,
    /// The session the messages are saved under; created with the first send.
    session_id: Option<String>,
    /// The session history while the user has it open; it replaces the
    /// conversation on screen until a session is picked or it is closed.
    history: Option<History>,
    /// The tool call waiting for the user's yes, if one is. The turn is
    /// blocked on the answer.
    pending_approval: Option<ApprovalRequest>,
    /// The running turn's engine task, so Stop can end it.
    turn: Option<tokio::task::JoinHandle<()>>,
    /// The task draining the turn's events.
    drain: Option<Task<()>>,
    /// Tool names the user let run "always" in this sitting. The gate reads it
    /// before it asks again; deliberately not persisted, because a permission
    /// granted in one conversation should not answer for another.
    always_allow: Arc<Mutex<HashSet<String>>>,
    /// Answers that arrived while the aside was closed. The rail's badge says
    /// how many; opening the panel clears it.
    unread: usize,
    /// Whether the panel is on screen. Not the same as "the turn runs": a
    /// closed panel keeps streaming, and what arrived meanwhile is unread.
    on_screen: bool,
    _input: Subscription,
}

impl ChatView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line(2, 6)
                .placeholder(t("ai.placeholder"))
        });
        let changed = cx.subscribe_in(
            &input,
            window,
            |_this: &mut Self, _input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Changed) {
                    cx.notify();
                }
            },
        );
        let providers = usable_providers();
        let provider = providers.first().cloned();
        Self {
            messages: Vec::new(),
            streaming: None,
            input,
            providers,
            provider,
            error: None,
            compacted: None,
            reasoning_started: None,
            session_id: None,
            history: None,
            pending_approval: None,
            turn: None,
            drain: None,
            always_allow: Arc::new(Mutex::new(HashSet::new())),
            unread: 0,
            on_screen: false,
            _input: changed,
        }
    }

    /// The composer's own focus handle: what lets the shell hand the keyboard
    /// to the conversation the moment the panel opens, without the pane beside
    /// it keeping the keys.
    pub(crate) fn composer_focus(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus().clone()
    }

    /// Answers that arrived while the panel was closed.
    pub(crate) fn unread(&self) -> usize {
        self.unread
    }

    /// Whether the panel was on screen when the last answer finished. Called by
    /// the shell whenever the panel is opened or closed; the panel's own
    /// content is untouched by it. Opening re-reads the provider file (the
    /// settings page writes it) and clears the badge.
    pub(crate) fn set_on_screen(&mut self, on_screen: bool, cx: &mut Context<Self>) {
        if self.on_screen == on_screen {
            return;
        }
        self.on_screen = on_screen;
        if on_screen {
            self.unread = 0;
            self.reload_providers();
        }
        cx.notify();
    }

    /// Re-read the provider file, keeping the composer's own choice when it is
    /// still in the catalog. Only called when the panel opens or a turn is
    /// about to start — never from a paint.
    fn reload_providers(&mut self) {
        self.providers = usable_providers();
        let keep = self.provider.as_ref().map(|chosen| chosen.name.clone());
        self.provider = keep
            .and_then(|name| {
                self.providers
                    .iter()
                    .find(|candidate| candidate.name == name)
                    .cloned()
            })
            .or_else(|| self.providers.first().cloned());
    }

    /// Whether a turn is running.
    fn is_streaming(&self) -> bool {
        self.streaming.is_some()
    }

    /// Start a turn: the user's text goes into the list, an empty assistant
    /// message below it waits for the tokens, and the engine is spawned on
    /// tokio. Nothing here waits for the model.
    fn send(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_streaming() {
            return;
        }
        let text = self.input.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let Some((provider, engine)) = self.prepare_turn(&text, cx) else {
            // The text stays in the box: nothing was sent.
            return;
        };
        self.input.update(cx, |input, cx| input.set_text("", cx));
        self.stand_down();
        // A sent message is the conversation again: the history stands down.
        self.history = None;

        let session_id = self.ensure_session(&text, &provider);
        let sequence = self.messages.last().map_or(1, |last| last.sequence + 1);
        let user = message(&session_id, "user", text.clone(), sequence);
        persist(&user);
        self.messages.push(user);
        self.run_turn(&session_id, &provider, engine, cx);
    }

    /// Run the last user message again: whatever the failed turn left below
    /// it is dropped — an answer with nothing in it was never saved — and the
    /// engine gets the same history.
    fn retry(&mut self, cx: &mut Context<Self>) {
        if self.is_streaming() {
            return;
        }
        let Some(user_index) = retry_target(&self.messages) else {
            return;
        };
        let text = self.messages[user_index].content.clone();
        let Some((provider, engine)) = self.prepare_turn(&text, cx) else {
            return;
        };
        self.messages.truncate(user_index + 1);
        self.stand_down();
        let session_id = self.session_id.clone().unwrap_or_default();
        self.run_turn(&session_id, &provider, engine, cx);
    }

    /// The provider and engine the next turn runs on, or the reason there is
    /// none, on screen. The composer's own choice wins; the file is read only
    /// when there is none, so a picker cannot be overwritten between a click
    /// and the next turn.
    fn prepare_turn(
        &mut self,
        first_message: &str,
        cx: &mut Context<Self>,
    ) -> Option<(ApiProviderConfig, OrchestratorEngine)> {
        if self.provider.is_none() {
            self.reload_providers();
        }
        let Some(provider) = self.provider.clone() else {
            self.error = Some(format!(
                "{} {}",
                t("ai.config_empty"),
                t("ai.config_empty_hint")
            ));
            cx.notify();
            return None;
        };
        let engine = match engine_for(&provider, first_message) {
            Ok(engine) => engine,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return None;
            }
        };
        if crate::TOKIO_HANDLE.get().cloned().is_none() {
            self.error = Some("no tokio runtime: the client started without one".to_owned());
            cx.notify();
            return None;
        }
        Some((provider, engine))
    }

    /// Clear what the previous turn left on the panel: its failure note, its
    /// compaction note, its reasoning clock.
    fn stand_down(&mut self) {
        self.error = None;
        self.compacted = None;
        self.reasoning_started = None;
    }

    /// Spawn the engine for a turn whose user message is already in the list:
    /// an empty assistant message waits below it for the tokens.
    fn run_turn(
        &mut self,
        session_id: &str,
        provider: &ApiProviderConfig,
        engine: OrchestratorEngine,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = crate::TOKIO_HANDLE.get().cloned() else {
            return;
        };
        let sequence = self.messages.last().map_or(1, |last| last.sequence + 1);
        let index = self.messages.len();
        self.messages
            .push(message(session_id, "orchestrator", String::new(), sequence));
        self.streaming = Some(index);

        let history: Vec<ChatMessageRef> = self.messages[..index]
            .iter()
            .filter(|message| !message.content.is_empty())
            .map(|message| ChatMessageRef {
                role: wire_role(&message.role).to_owned(),
                content: message.content.clone(),
                ..Default::default()
            })
            .collect();

        let (events, incoming) = crossbeam_channel::unbounded::<DispatchStreamEvent>();
        let (approval_tx, approval_rx) = crossbeam_channel::unbounded::<ApprovalRequest>();
        tracing::info!(
            target: "zenix-chat",
            provider = %provider.name,
            model = %provider.model,
            session = %session_id,
            "turn started"
        );
        let report = events.clone();
        let always_allow = self.always_allow.clone();
        self.turn = Some(runtime.spawn(async move {
            let tools = builtin_tools();
            let tools: Vec<&dyn Tool> = tools.iter().map(|tool| tool.as_ref()).collect();
            // The empty workspace id is the daemon's own focused workspace: the
            // panel has no snapshot of its own to name one from.
            let executor = ToolExecutor::new(ApiEndpoint::local(), String::new());
            let compactor = Compactor::default_compactor();
            let gate = ApprovalGate {
                requests: approval_tx,
                always_allow,
            };
            let outcome = engine
                .chat_with_tools(
                    &history,
                    &tools,
                    &executor,
                    &compactor,
                    MAX_TOOL_ROUNDS,
                    Some(&gate),
                    move |event| {
                        let _ = report.send(event);
                    },
                )
                .await;
            if let Err(error) = outcome {
                // The panel can only act on what arrives through the channel:
                // a failure that stays a log line leaves a turn that looks
                // alive, so the failure goes out as an event first.
                let _ = events.send(DispatchStreamEvent::Error(error.to_string()));
                tracing::warn!(target: "zenix-chat", %error, "the turn did not finish");
            }
        }));

        // The drain: one message in, one repaint. Most are stream events; an
        // approval request parks itself on the panel and the turn waits. It
        // ends when the engine task drops its senders — which is also how
        // Stop, an abort, ends it.
        self.drain = Some(cx.spawn(async move |panel, cx| {
            loop {
                let incoming = incoming.clone();
                let approvals = approval_rx.clone();
                let next = cx
                    .background_executor()
                    .spawn(async move {
                        crossbeam_channel::select! {
                            recv(incoming) -> event => DrainMessage::Event(event),
                            recv(approvals) -> request => DrainMessage::Approval(request.ok()),
                        }
                    })
                    .await;
                match next {
                    DrainMessage::Event(Ok(event)) => {
                        if panel
                            .update(cx, |panel, cx| panel.apply(event, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                    // The turn's senders are gone: what was streamed is what
                    // the session keeps.
                    DrainMessage::Event(Err(_)) => break,
                    DrainMessage::Approval(request) => {
                        if panel
                            .update(cx, |panel, cx| panel.approval_requested(request, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            let _ = panel.update(cx, |panel, cx| panel.finish(cx));
        }));
        cx.notify();
    }

    /// End the running turn. Aborting the engine task drops the sender the
    /// drain is waiting on, so the drain ends by itself and keeps what was
    /// streamed so far. A request already inside the HTTP client finishes on
    /// its own; its answer is discarded with the task.
    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(turn) = self.turn.take() {
            turn.abort();
        }
        cx.notify();
    }

    /// Fold one event into the streaming message.
    fn apply(&mut self, event: DispatchStreamEvent, cx: &mut Context<Self>) {
        let now = now_ms();
        if matches!(event, DispatchStreamEvent::Thinking { .. }) && self.reasoning_started.is_none()
        {
            self.reasoning_started = Some(now);
        }
        // The two legs of a tool call, logged where the UI sees them: a card on
        // screen and a line in the log are evidence of the same thing.
        match &event {
            DispatchStreamEvent::ToolCall { name, .. } => {
                tracing::info!(target: "zenix-chat", tool = %name, "tool call")
            }
            DispatchStreamEvent::ToolResult {
                tool_name,
                is_error,
                ..
            } => tracing::info!(
                target: "zenix-chat",
                tool = %tool_name,
                failed = %is_error,
                "tool result"
            ),
            _ => {}
        }
        let Some(index) = self.streaming else {
            return;
        };
        let Some(message) = self.messages.get_mut(index) else {
            return;
        };
        match fold_event(message, &event, now) {
            Folded::Message => {}
            Folded::Compacted(info) => self.compacted = Some(info),
            Folded::Error(error) => self.error = Some(error),
        }
        cx.notify();
    }

    /// The turn is over: what was streamed is what the session keeps.
    fn finish(&mut self, cx: &mut Context<Self>) {
        self.turn = None;
        let thought_for = self
            .reasoning_started
            .take()
            .map(|start| now_ms().saturating_sub(start));
        if let Some(index) = self.streaming.take()
            && let Some(message) = self.messages.get_mut(index)
        {
            if thought_for.is_some() && message.reasoning_duration_ms.is_none() {
                message.reasoning_duration_ms = thought_for;
            }
            tracing::info!(
                target: "zenix-chat",
                chars = message.content.chars().count(),
                tools = message.tool_calls.len(),
                "turn finished"
            );
            // An answer with nothing in it — a turn that failed before its
            // first token — is not a message anyone will want to read back:
            // the retry drops it rather than saving it.
            let said_something = !message.content.is_empty()
                || !message.tool_calls.is_empty()
                || message.reasoning_text.is_some()
                || message.usage.is_some();
            if said_something {
                persist(message);
                // The store's rows order by `updated_at`: a finished turn
                // moves this session to the top of the history the next time
                // it opens.
                if let Some(session_id) = self.session_id.clone()
                    && let Err(error) = SessionManager::shared().and_then(|manager| {
                        manager.touch_session(&session_id, &crate::util::timestamp_now())
                    })
                {
                    tracing::warn!(target: "zenix-chat", %error, "the session was not marked as touched");
                }
                // Arrived while the panel was closed: the rail's badge counts
                // it until the panel is opened.
                if !self.on_screen {
                    self.unread += 1;
                }
            }
        }
        cx.notify();
    }

    /// Park an approval request on the panel: the turn blocks until the user
    /// answers, and the card above the composer is where they do.
    fn approval_requested(&mut self, request: Option<ApprovalRequest>, cx: &mut Context<Self>) {
        self.pending_approval = request;
        cx.notify();
    }

    /// Answer the pending approval and let the turn go. Dropping the request
    /// without answering — a new session, another history pick — denies it by
    /// the same door: the sender dies, the turn reads a denial.
    fn answer_approval(&mut self, allow: bool, always: bool, cx: &mut Context<Self>) {
        if let Some(request) = self.pending_approval.take() {
            if allow && always {
                self.always_allow
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(request.name.clone());
            }
            let _ = request.respond.send(allow);
        }
        cx.notify();
    }

    /// Forget the conversation on screen, so the next send starts a new session
    /// on disk as well.
    fn new_session(&mut self, cx: &mut Context<Self>) {
        if self.is_streaming() {
            self.stop(cx);
        }
        self.pending_approval = None;
        self.messages.clear();
        self.error = None;
        self.compacted = None;
        self.session_id = None;
        self.history = None;
        cx.notify();
    }

    /// The session the messages are saved under, created with the first send so
    /// a conversation that never happened leaves nothing behind.
    fn ensure_session(&mut self, first_message: &str, provider: &ApiProviderConfig) -> String {
        if let Some(id) = &self.session_id {
            return id.clone();
        }
        let Ok(manager) = SessionManager::shared() else {
            return String::new();
        };
        let now = crate::util::timestamp_now();
        let id = uuid::Uuid::new_v4().to_string();
        let session = Session {
            id: id.clone(),
            title: first_message.chars().take(40).collect(),
            created_at: now.clone(),
            updated_at: now,
            workspace: None,
            provider_id: provider.name.clone(),
            model: provider.model.clone(),
            thinking_level: provider.thinking_level.as_str().to_owned(),
        };
        if manager.create_session(&session).is_err() {
            return String::new();
        }
        self.session_id = Some(id.clone());
        id
    }

    // ── Session history ────────────────────────────────────────────────

    /// Open the history over the conversation, or close it. Opening reads the
    /// store once; the rows stay until the list is opened again.
    fn toggle_history(&mut self, cx: &mut Context<Self>) {
        if self.history.take().is_some() {
            cx.notify();
            return;
        }
        self.history = Some(History {
            sessions: read_sessions(),
            error: None,
            confirm: None,
        });
        cx.notify();
    }

    /// Re-read the store into the open history, after a rename or a delete.
    fn refresh_history(&mut self, cx: &mut Context<Self>) {
        let Some(history) = self.history.as_mut() else {
            return;
        };
        history.sessions = read_sessions();
        cx.notify();
    }

    /// Load a session's turns onto the screen. A running turn belongs to the
    /// conversation being replaced, so it is stopped first.
    fn open_session(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.is_streaming() {
            self.stop(cx);
        }
        let manager = match SessionManager::shared() {
            Ok(manager) => manager,
            Err(error) => {
                self.set_history_error(error.to_string(), cx);
                return;
            }
        };
        let messages = match manager.get_messages(id) {
            Ok(messages) => messages,
            Err(error) => {
                self.set_history_error(error.to_string(), cx);
                return;
            }
        };
        let session = manager.get_session(id).ok().flatten();
        self.history = None;
        self.pending_approval = None;
        self.messages = messages;
        self.streaming = None;
        self.session_id = Some(id.to_owned());
        self.error = None;
        self.compacted = None;
        self.reasoning_started = None;
        if let Some(session) = session {
            self.restore_composer(&session);
        }
        cx.notify();
    }

    /// Point the composer at the session's provider and reasoning level, when
    /// that provider is still in the catalog. The record names the level and
    /// the provider; the model of the next turn is the picker's — a provider
    /// edited since the session is honored as it is now.
    fn restore_composer(&mut self, session: &Session) {
        let Some(mut provider) = self
            .providers
            .iter()
            .find(|candidate| candidate.name == session.provider_id)
            .cloned()
        else {
            return;
        };
        if let Some(level) = level_of(&session.thinking_level) {
            provider.thinking_level = level;
        }
        self.provider = Some(provider);
    }

    /// The store's title for a row, written through `InlineEdit`.
    fn rename_session(&mut self, id: &str, title: String, cx: &mut Context<Self>) {
        let title = title.trim().to_owned();
        if title.is_empty() {
            return;
        }
        let outcome = SessionManager::shared().and_then(|manager| {
            manager
                .get_session(id)?
                .ok_or_else(|| anyhow::anyhow!("no such session"))
                .and_then(|mut session| {
                    session.title = title;
                    manager.update_session(&session)
                })
        });
        if let Err(error) = outcome {
            tracing::warn!(target: "zenix-chat", %error, "the session was not renamed");
            self.set_history_error(error.to_string(), cx);
        }
        self.refresh_history(cx);
    }

    /// Delete a session. The first press arms the row's question; the confirm
    /// bar's own button means it. The conversation on screen cannot outlive
    /// its session.
    fn delete_session(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(history) = self.history.as_mut() {
            history.confirm = Some(id.to_owned());
            history.error = None;
        }
        cx.notify();
    }

    /// Delete the session the confirm bar names, and put the conversation back
    /// to a new one if it was the session on screen.
    fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self
            .history
            .as_ref()
            .and_then(|history| history.confirm.clone())
        else {
            return;
        };
        if let Some(history) = self.history.as_mut() {
            history.confirm = None;
        }
        if self.session_id.as_deref() == Some(id.as_str()) {
            if self.is_streaming() {
                self.stop(cx);
            }
            self.pending_approval = None;
            self.messages.clear();
            self.session_id = None;
        }
        if let Err(error) = SessionManager::shared().and_then(|manager| manager.delete_session(&id))
        {
            tracing::warn!(target: "zenix-chat", %error, "the session was not deleted");
            self.set_history_error(error.to_string(), cx);
        }
        self.refresh_history(cx);
    }

    /// Leave the delete question open on screen without answering it.
    fn cancel_delete(&mut self, cx: &mut Context<Self>) {
        if let Some(history) = self.history.as_mut() {
            history.confirm = None;
        }
        cx.notify();
    }

    fn set_history_error(&mut self, error: String, cx: &mut Context<Self>) {
        if let Some(history) = self.history.as_mut() {
            history.error = Some(error);
        } else {
            self.error = Some(error);
        }
        cx.notify();
    }

    /// The composer's model picker: the provider the next turn runs on.
    fn choose_provider(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(provider) = self
            .providers
            .iter()
            .find(|candidate| candidate.name == name)
            .cloned()
        {
            self.provider = Some(provider);
            cx.notify();
        }
    }

    /// The composer's reasoning-effort picker: the level the next turn asks
    /// for. The provider file is not touched — this is the turn's choice.
    fn choose_level(&mut self, token: &str, cx: &mut Context<Self>) {
        if let (Some(provider), Some(level)) = (self.provider.as_mut(), level_of(token)) {
            provider.thinking_level = level;
            cx.notify();
        }
    }

    /// Put one of the welcome screen's starters into the box, leaving it
    /// focused so the user can adjust the words before sending.
    fn use_starter(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((text, _)) = starter_prompts().into_iter().nth(index) else {
            return;
        };
        self.input.update(cx, |input, cx| input.set_text(text, cx));
        let focus = self.input.read(cx).focus().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// How many rows the message list draws: the messages, then the turn's own
    /// leftovers — the gate, the failure, the compaction note.
    fn row_count(&self) -> usize {
        self.messages.len() + self.trailing().len()
    }

    /// The trailing rows, in drawing order.
    fn trailing(&self) -> Vec<Trailing<'_>> {
        let mut rows = Vec::new();
        if let Some(request) = self.pending_approval.as_ref() {
            rows.push(Trailing::Approval(request));
        }
        if let Some(error) = self.error.as_deref() {
            rows.push(Trailing::Error(error));
        }
        if let Some(info) = self.compacted.as_ref() {
            rows.push(Trailing::Compacted(info));
        }
        rows
    }

    /// Everything one list row needs, copied out of the view so the list's
    /// render closure does not borrow the entity while it draws.
    fn row_model(&self, index: usize) -> Option<RowModel> {
        if let Some(message) = self.messages.get(index) {
            let streaming = self.streaming == Some(index);
            let user = message.role == "user";
            // The call the gate is asking about is the gate's to show; the
            // card would be the same call twice.
            let awaiting = self
                .pending_approval
                .as_ref()
                .map(|request| request.id.clone());
            let tool_calls: Vec<ToolCallState> = message
                .tool_calls
                .iter()
                .filter(|call| awaiting.as_deref() != Some(call.id.as_str()))
                .cloned()
                .collect();
            return Some(RowModel::Message(MessageModel {
                user,
                streaming,
                content: message.content.clone(),
                reasoning: message.reasoning_text.clone(),
                reasoning_took_ms: if streaming {
                    self.reasoning_started
                        .map(|start| now_ms().saturating_sub(start))
                        .or(message.reasoning_duration_ms)
                } else {
                    message.reasoning_duration_ms
                },
                tool_calls,
                usage: message.usage,
                at: timestamp_of(&message.timestamp),
                can_retry: !user && !streaming && index + 1 == self.messages.len(),
            }));
        }
        let trailing = self.trailing();
        let slot = index.checked_sub(self.messages.len())?;
        match trailing.get(slot)? {
            Trailing::Approval(request) => Some(RowModel::Approval(ApprovalModel {
                name: request.name.clone(),
                args: request.args.clone(),
            })),
            Trailing::Error(error) => Some(RowModel::Error((*error).to_owned())),
            Trailing::Compacted(info) => Some(RowModel::Compacted((*info).clone())),
        }
    }
}

/// The trailing rows, borrowed from the view for one paint.
#[derive(Clone, Copy)]
enum Trailing<'a> {
    Approval(&'a ApprovalRequest),
    Error(&'a str),
    Compacted(&'a CompactionInfo),
}

/// One list row, copied out of the view.
enum RowModel {
    Message(MessageModel),
    Approval(ApprovalModel),
    Error(String),
    Compacted(CompactionInfo),
}

struct MessageModel {
    user: bool,
    streaming: bool,
    content: String,
    reasoning: Option<String>,
    reasoning_took_ms: Option<u64>,
    tool_calls: Vec<ToolCallState>,
    usage: Option<UsageInfo>,
    at: Option<jiff::Timestamp>,
    can_retry: bool,
}

struct ApprovalModel {
    name: String,
    args: serde_json::Value,
}

/// A tool call the engine has stopped on, waiting for the user's word. The
/// answer travels back on `respond`: `true` runs the call, `false` sends the
/// model a denial it can read.
struct ApprovalRequest {
    id: String,
    name: String,
    args: serde_json::Value,
    respond: crossbeam_channel::Sender<bool>,
}

/// The panel's half of the approval gate, living inside the turn's engine
/// task. Every question the engine asks lands on the panel's channel; the
/// answer comes back on the request's own.
struct ApprovalGate {
    requests: crossbeam_channel::Sender<ApprovalRequest>,
    /// Tool names the user answered "always" for in this sitting.
    always_allow: Arc<Mutex<HashSet<String>>>,
}

impl ToolApproval for ApprovalGate {
    fn approve(&self, call: &ToolCall) -> bool {
        if self
            .always_allow
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&call.name)
        {
            return true;
        }
        let (respond, answer) = crossbeam_channel::bounded(1);
        let request = ApprovalRequest {
            id: call.id.clone(),
            name: call.name.clone(),
            args: call.arguments.clone(),
            respond,
        };
        // A panel that is gone grants nothing.
        if self.requests.send(request).is_err() {
            return false;
        }
        matches!(answer.recv(), Ok(true))
    }
}

/// One message the drain woke up for.
enum DrainMessage {
    Event(Result<DispatchStreamEvent, crossbeam_channel::RecvError>),
    Approval(Option<ApprovalRequest>),
}

/// The session history, when the user has it open: the store's sessions as
/// rows, replacing the conversation until one is picked or the list is closed.
struct History {
    /// The store's rows, most recently touched first.
    sessions: Vec<Session>,
    /// Why the last rename or delete failed, until the next one.
    error: Option<String>,
    /// The row whose delete button has been pressed once and is waiting for
    /// the confirm bar's answer.
    confirm: Option<String>,
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The panel's tab strip already says which tab this is, so the view
        // opens with the conversation rather than a second title row. The
        // library's frame keeps the messages above and the composer at the
        // bottom; the panel only decides which body goes between them.
        let quiet = self.messages.is_empty()
            && self.error.is_none()
            && self.pending_approval.is_none()
            && self.compacted.is_none();
        let body = if self.history.is_some() {
            self.history_view(window, cx)
        } else if quiet {
            self.welcome(cx)
        } else {
            self.conversation(cx)
        };
        ChatContainer::new(body).composer(self.composer(cx))
    }
}

impl ChatView {
    /// The conversation: one bubble per message, then whatever the turn left
    /// waiting — a gate, a failure, a compaction note — above the composer.
    fn conversation(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let count = self.row_count();
        // Each conversation gets its own list, so loading another session
        // starts at its newest message and its scroll state is its own.
        let id = SharedString::from(format!(
            "chat-messages-{}",
            self.session_id.as_deref().unwrap_or("new")
        ));
        // The list's own padding: the library's list lays rows at content
        // width, so the breathing room belongs to the frame around it.
        div()
            .size_full()
            .px_3()
            .py_3()
            .child(MessageList::new(id, count, move |index, window, cx| {
                let Some(panel) = weak.upgrade() else {
                    return div().into_any_element();
                };
                let model = panel.read(cx).row_model(index);
                match model {
                    Some(model) => row_element(model, &weak, index, window, cx),
                    None => div().into_any_element(),
                }
            }))
            .into_any_element()
    }

    /// The empty panel: what the assistant is for, and three starters that put
    /// their words in the composer rather than sending them blind.
    fn welcome(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let hint = if self.provider.is_some() {
            t("ai.chat.empty_hint")
        } else {
            format!("{} {}", t("ai.config_empty"), t("ai.config_empty_hint"))
        };
        let prompts: Vec<(SharedString, IconName)> = starter_prompts()
            .into_iter()
            .map(|(text, icon)| (text.into(), icon))
            .collect();
        let weak = cx.entity().downgrade();
        let pick = move |index: usize, window: &mut Window, cx: &mut App| {
            if let Some(panel) = weak.upgrade() {
                panel.update(cx, |panel, cx| panel.use_starter(index, window, cx));
            }
        };
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .p_4()
            .text_color(colors.fg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(theme.radius(Radius::Lg))
                            .bg(colors.accent.opacity(0.14))
                            .child(
                                Icon::new(IconName::Sparkles)
                                    .size(IconSize::Sm)
                                    .color(colors.accent),
                            ),
                    )
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Lg))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(t("ai.chat.welcome_title")),
                    ),
            )
            .child(
                div()
                    .max_w(px(320.))
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_muted)
                    .child(hint),
            )
            .child(div().max_w(px(360.)).child(SuggestionChips::new(
                "chat-starters",
                prompts,
                pick,
            )))
            .into_any_element()
    }

    /// The session history: the store's rows, newest first. A row loads its
    /// session; its title edits in place, its bin asks once before it deletes.
    fn history_view(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // The rows below take the context mutably, so the tokens this frame
        // draws with are copied out first: the theme itself is borrowed, and a
        // row builder cannot hold it and the context at once.
        let (xs, border, surface, fg, muted, danger, radius) = {
            let theme = cx.theme();
            (
                theme.text_size(TextSize::Xs),
                theme.colors.border,
                theme.colors.surface,
                theme.colors.fg,
                theme.colors.fg_muted,
                theme.colors.danger,
                theme.radius(Radius::Md),
            )
        };
        let Some(history) = self.history.as_ref() else {
            return div().into_any_element();
        };
        let weak = cx.entity().downgrade();

        let new = {
            let weak = weak.clone();
            move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |panel, cx| panel.new_session(cx));
                }
            }
        };
        let mut column = div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .py_2()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .text_size(xs)
                            .text_color(muted)
                            .child(t("ai.chat.conversations")),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("history-new", t("ai.chat.new_chat"))
                            .variant(ButtonVariant::Ghost)
                            .size(ControlSize::Sm)
                            .icon(IconName::Plus)
                            .on_click(new),
                    ),
            );
        if let Some(error) = history.error.as_deref() {
            column = column.child(
                div()
                    .mx_2()
                    .py_1()
                    .text_size(xs)
                    .text_color(danger)
                    .child(error.to_owned()),
            );
        }
        if history.sessions.is_empty() {
            column = column.child(
                div()
                    .flex_1()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(xs)
                    .text_color(muted)
                    .child(t("ai.chat.history_empty")),
            );
        } else {
            let row_height = config::get(cx).appearance.row_height();
            let mut rows = div()
                .id("chat-history")
                .flex_1()
                .min_h_0()
                .w_full()
                .gap_0p5()
                .flex()
                .flex_col()
                .overflow_y_scroll();
            for session in &history.sessions {
                rows = rows.child(history_row(self, session, row_height, &weak, cx));
            }
            column = column.child(rows);
        }
        if let Some(id) = history.confirm.as_deref() {
            let title = history
                .sessions
                .iter()
                .find(|session| session.id == id)
                .map(|session| session.title.clone())
                .unwrap_or_default();
            let weak_confirm = weak.clone();
            let weak_cancel = weak.clone();
            // The card is the panel's own: the library's `ConfirmationCard`
            // says "Cancel" in English, and every other word here follows the
            // reader's locale.
            column = column.child(
                div()
                    .mx_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .rounded(radius)
                    .border_1()
                    .border_color(border)
                    .bg(surface)
                    .child(
                        div()
                            .text_size(xs)
                            .text_color(fg)
                            .child(rust_i18n::t!("ai.chat.delete_ask", title = title).to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("chat-delete-cancel", t("ai.chat.delete_cancel"))
                                    .variant(ButtonVariant::Ghost)
                                    .size(ControlSize::Sm)
                                    .on_click(move |_, _, cx| {
                                        if let Some(panel) = weak_cancel.upgrade() {
                                            panel.update(cx, |panel, cx| panel.cancel_delete(cx));
                                        }
                                    }),
                            )
                            .child(
                                Button::new("chat-delete-confirm", t("ai.chat.delete"))
                                    .variant(ButtonVariant::Danger)
                                    .size(ControlSize::Sm)
                                    .on_click(move |_, _, cx| {
                                        if let Some(panel) = weak_confirm.upgrade() {
                                            panel.update(cx, |panel, cx| panel.confirm_delete(cx));
                                        }
                                    }),
                            ),
                    ),
            );
        }
        column.into_any_element()
    }

    /// The composer: the library's prompt card holding what the next turn runs
    /// on, the field, and the two pickers that decide the turn. Enter sends and
    /// Shift-Enter breaks the line; `/` offers the panel's own commands.
    fn composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let send = {
            let weak = weak.clone();
            move |window: &mut Window, cx: &mut App| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |panel, cx| panel.send(window, cx));
                }
            }
        };
        let mut prompt = PromptInput::new("chat-composer", &self.input, send);
        if self.is_streaming() {
            let weak = weak.clone();
            prompt = prompt.busy(move |_window, cx| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |panel, cx| panel.stop(cx));
                }
            });
        }
        let commands = {
            let weak = weak.clone();
            move |value: &SharedString, _window: &mut Window, cx: &mut App| {
                let Some(panel) = weak.upgrade() else {
                    return;
                };
                let value = value.to_string();
                panel.update(cx, |panel, cx| match value.as_str() {
                    "new" => panel.new_session(cx),
                    "history" => panel.toggle_history(cx),
                    "clear" => {
                        panel.input.update(cx, |input, cx| input.set_text("", cx));
                        cx.notify();
                    }
                    _ => {}
                });
            }
        };
        prompt
            .above(self.context_line(cx))
            .tool(self.model_picker(&weak))
            .tool(self.level_picker(&weak))
            .tool(command_hint(cx))
            .commands(starter_commands(), commands)
            .into_any_element()
    }

    /// The line above the message box: what the next turn runs on, how much has
    /// been said, and how much it cost.
    fn context_line(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let detail = match self.provider.as_ref() {
            Some(provider) if self.is_streaming() => {
                format!("{} · {}", provider.name, t("ai.chat.streaming"))
            }
            Some(provider) => [provider.name.clone(), provider.model.clone()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<String>>()
                .join(" · "),
            None => t("ai.config_empty"),
        };
        let turns = rust_i18n::t!("ai.chat.turns", count = self.messages.len()).to_string();
        let tokens: u64 = self
            .messages
            .iter()
            .filter_map(|message| message.usage)
            .map(|usage| usage.total_tokens())
            .sum();
        let tone = if self.error.is_some() {
            colors.danger
        } else if self.is_streaming() {
            colors.accent
        } else {
            colors.success
        };
        let mut line = div()
            .flex()
            .flex_none()
            .w_full()
            .px(px(2.))
            .pb(px(2.))
            .gap_1p5()
            .items_center()
            .text_size(px(10.))
            .text_color(colors.fg_muted)
            .child(div().size(px(6.)).flex_none().rounded_full().bg(tone))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(Ellipsis::new(SharedString::from(detail))),
            )
            .child(div().flex_none().child(turns));
        if tokens > 0 {
            line = line.child(div().flex_none().child(format!("{tokens} tok")));
        }
        line.into_any_element()
    }

    /// The model picker: every configured provider that carries a key.
    fn model_picker(&self, weak: &WeakEntity<Self>) -> AnyElement {
        let choices: Vec<Choice> = self
            .providers
            .iter()
            .map(|provider| {
                let label = if provider.model.trim().is_empty() {
                    provider.name.clone()
                } else {
                    format!("{} · {}", provider.name, provider.model)
                };
                Choice::new(provider.name.clone(), label)
            })
            .collect();
        let selected = self.provider.as_ref().map(|provider| provider.name.clone());
        let mut select = ely_core::forms::Select::new("chat-model", choices)
            .size(ControlSize::Sm)
            .placeholder(t("ai.chat.model"));
        if let Some(selected) = selected {
            select = select.selected(selected);
        }
        if self.provider.is_none() {
            select = select.disabled(true);
        }
        let weak = weak.clone();
        let select = select.on_change(move |picked, _, cx| {
            if let Some(panel) = weak.upgrade() {
                let name = picked.to_string();
                panel.update(cx, |panel, cx| panel.choose_provider(&name, cx));
            }
        });
        div().w(px(150.)).min_w_0().child(select).into_any_element()
    }

    /// The reasoning-effort picker: the four levels `ThinkingLevel` defines.
    fn level_picker(&self, weak: &WeakEntity<Self>) -> AnyElement {
        let choices: Vec<Choice> = ThinkingLevel::all()
            .iter()
            .map(|level| Choice::new(level.as_str(), t(&format!("ai.effort.{}", level.as_str()))))
            .collect();
        let selected = self
            .provider
            .as_ref()
            .map(|provider| provider.thinking_level.as_str().to_owned());
        let mut select = ely_core::forms::Select::new("chat-level", choices)
            .size(ControlSize::Sm)
            .placeholder(t("ai.chat.effort"));
        if let Some(selected) = selected {
            select = select.selected(selected);
        }
        if self.provider.is_none() {
            select = select.disabled(true);
        }
        let weak = weak.clone();
        let select = select.on_change(move |picked, _, cx| {
            if let Some(panel) = weak.upgrade() {
                let token = picked.to_string();
                panel.update(cx, |panel, cx| panel.choose_level(&token, cx));
            }
        });
        div().w(px(84.)).min_w_0().child(select).into_any_element()
    }
}

/// One row of the message list: a message, or one of the turn's leftovers.
fn row_element(
    model: RowModel,
    weak: &WeakEntity<ChatView>,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match model {
        RowModel::Message(message) => message_row(message, weak, index, window, cx),
        RowModel::Approval(request) => approval_card(request, weak, window, cx),
        RowModel::Error(error) => error_card(error, weak, cx),
        RowModel::Compacted(info) => compacted_note(&info, cx),
    }
}

/// A message in the library's bubble: the reasoning it thought with above, one
/// card per tool call in the middle, what was said last, and the actions that
/// can be taken on it. What the model says is markdown; what the user typed is
/// their own text, shown as they wrote it.
fn message_row(
    message: MessageModel,
    weak: &WeakEntity<ChatView>,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let streaming = message.streaming;
    let user = message.user;
    let role = if user { Role::User } else { Role::Assistant };
    let name = SharedString::from(role_label(if user { "user" } else { "orchestrator" }));
    let avatar = MessageAvatar::new(("chat-avatar", index), role, name.clone());
    // The library's `MessageHeader.at(...)` embeds a self-ticking
    // `RelativeTime`, whose repaint leaves the previous string's pixels beside
    // the new one when the row does not re-lay out. The time is folded into
    // the name instead: one string, laid out once per paint.
    let header_name = match message.at {
        Some(at) => SharedString::from(format!("{} · {}", name, relative_label(at, cx))),
        None => name,
    };
    let header = MessageHeader::new(("chat-header", index), header_name);
    let mut bubble = MessageBubble::new(role).avatar(avatar).header(header);

    if let Some(reasoning) = message.reasoning.as_deref() {
        bubble = bubble.child(reasoning_block(
            index,
            reasoning,
            streaming,
            message.reasoning_took_ms,
            window,
            cx,
        ));
    }
    for (call_index, call) in message.tool_calls.iter().enumerate() {
        bubble = bubble.child(tool_call(index, call_index, call, window, cx));
    }
    if !message.content.is_empty() {
        if user {
            let mut lines = div().w_full().flex().flex_col().gap_0p5();
            for line in message.content.split('\n') {
                lines = lines.child(div().child(line.to_owned()));
            }
            bubble = bubble.child(lines);
        } else {
            bubble = bubble.child(StreamingMarkdown::new(
                ("chat-md", index),
                message.content.clone(),
                streaming,
            ));
        }
    } else if streaming && message.reasoning.is_none() {
        bubble = bubble
            .child(ThinkingIndicator::new(("chat-waiting", index)).label(t("ai.chat.streaming")));
    }

    if !streaming && !message.content.is_empty() {
        let mut footer = MessageFooter::new();
        if let Some(usage) = message.usage {
            footer = footer.fact(format!("{} tok", usage.total_tokens()));
        }
        let mut actions = div().flex().items_center().gap_1();
        let copy_text = message.content.clone();
        actions = actions.child(
            IconButton::new(("chat-copy", index), IconName::Copy)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip(t("ai.chat.copy"))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                }),
        );
        if message.can_retry {
            let weak = weak.clone();
            actions = actions.child(
                IconButton::new(("chat-retry", index), IconName::RefreshCw)
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .tooltip(t("ai.chat.retry"))
                    .on_click(move |_, _, cx| {
                        if let Some(panel) = weak.upgrade() {
                            panel.update(cx, |panel, cx| panel.retry(cx));
                        }
                    }),
            );
        }
        bubble = bubble.footer(footer.child(actions));
    }
    bubble.into_any_element()
}

/// One tool call. The card carries the wire's own name and status, the
/// arguments the model filled in, and the answer that came back. The labels
/// are the reader's; the fold is the library's `Disclosure`.
fn tool_call(
    message_index: usize,
    call_index: usize,
    call: &ToolCallState,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let status = match call.status {
        ToolCallStatus::Running => StepState::Working,
        ToolCallStatus::Completed => StepState::Done,
        ToolCallStatus::Failed => StepState::Failed,
    };
    let id = gpui::ElementId::from(SharedString::from(format!(
        "chat-tool-{message_index}-{call_index}-{}",
        call.id
    )));
    let opens = !call.args.is_null() || call.result_summary.is_some();
    let state = window.use_keyed_state((id.clone(), "open"), cx, |_, _| false);
    let open = *state.read(cx);
    let mark = step_mark((id.clone(), "mark").into(), status, window, cx);
    let theme = cx.theme();
    let colors = &theme.colors;
    let summary = arguments_summary(&call.args);
    let took = match (call.completed_at, call.started_at) {
        (Some(end), start) => Some(end.saturating_sub(start)),
        _ => None,
    };
    let toggle = {
        let state = state.clone();
        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
            state.update(cx, |open, cx| {
                *open = !*open;
                cx.notify();
            });
        }
    };
    let mut header = div()
        .id(id.clone())
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .when(opens, |row| row.cursor_pointer().on_click(toggle))
        .child(mark)
        .child(
            div()
                .min_w_0()
                .text_color(colors.fg)
                .child(Ellipsis::new(SharedString::from(call.name.clone()))),
        )
        .when(!summary.is_empty(), |row| {
            row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(colors.fg_muted)
                    .child(Ellipsis::new(SharedString::from(summary.clone()))),
            )
        });
    if let Some(took) = took {
        header = header.child(
            div()
                .flex_none()
                .text_color(colors.fg_subtle)
                .child(took_label(took)),
        );
    }
    if opens {
        header = header.child(Disclosure::new((id.clone(), "chevron"), open).size(IconSize::Sm));
    }
    let mut card = div()
        .rounded(theme.radius(Radius::Md))
        .border_1()
        .border_color(colors.border)
        .bg(colors.surface)
        .text_size(theme.text_size(TextSize::Sm))
        .child(header);
    if open && opens {
        let mut body = div().flex().flex_col().gap_2().px_3().pt_1().pb_3();
        if !call.args.is_null() {
            let json =
                serde_json::to_string_pretty(&call.args).unwrap_or_else(|_| call.args.to_string());
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(fold_label(t("ai.chat.tool_arguments"), cx))
                    .child(mono_block(json, colors.fg_muted, cx)),
            );
        }
        if let Some(result) = call.result_summary.as_ref() {
            let failed = call.status == ToolCallStatus::Failed;
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(fold_label(
                        if failed {
                            t("ai.chat.tool_error")
                        } else {
                            t("ai.chat.tool_result")
                        },
                        cx,
                    ))
                    .child(mono_block(
                        result.clone(),
                        if failed {
                            colors.danger
                        } else {
                            colors.fg_muted
                        },
                        cx,
                    )),
            );
        }
        card = card.child(body);
    }
    card.into_any_element()
}

/// A fold's body: the wire's own text, monospaced, on the sunken ground. The
/// library's `CodeBlock` prints "code" as its header and its copy tooltip in
/// English, so a card that already names its fold draws the text itself.
fn mono_block(text: String, ink: gpui::Hsla, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .w_full()
        .p_2()
        .rounded(theme.radius(Radius::Sm))
        .bg(theme.colors.sunken)
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(ink)
        .child(text)
        .into_any_element()
}

/// A fold's quiet name above its body.
fn fold_label(text: String, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .text_size(theme.text_size(TextSize::Xs))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.colors.fg_subtle)
        .child(text)
        .into_any_element()
}

/// The reasoning a turn thought with, folded away until it is asked for: it is
/// the model talking to itself, and it is long. The header is the panel's own
/// so its words follow the reader's locale; the library draws the text inside.
fn reasoning_block(
    index: usize,
    reasoning: &str,
    streaming: bool,
    took_ms: Option<u64>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = SharedString::from(format!("chat-thinking-{index}"));
    let key = gpui::ElementId::from(id.clone());
    let open = window.use_keyed_state((key.clone(), "open"), cx, |_, _| false);
    let opened = *open.read(cx);
    let theme = cx.theme();
    let colors = &theme.colors;
    let label = if streaming {
        t("ai.chat.streaming")
    } else {
        match took_ms.unwrap_or(0) / 1000 {
            0 => t("ai.chat.thought_moment"),
            seconds => rust_i18n::t!("ai.chat.thought_seconds", seconds = seconds).to_string(),
        }
    };
    let toggle = {
        let open = open.clone();
        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
            open.update(cx, |open, cx| {
                *open = !*open;
                cx.notify();
            });
        }
    };
    let mut block = div().w_full().flex().flex_col().gap_1().child(
        div()
            .id(id.clone())
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_1()
            .py_0p5()
            .rounded(theme.radius(Radius::Sm))
            .cursor_pointer()
            .hover(|style| style.bg(colors.hover))
            .on_click(toggle)
            .child(
                div()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_muted)
                    .child(label),
            )
            .child(div().flex_1())
            .child(Disclosure::new((key.clone(), "chevron"), opened).size(IconSize::Sm)),
    );
    if opened {
        block = block.child(
            div()
                .pl_3()
                .border_l_2()
                .border_color(colors.border)
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(StreamingText::new(
                    (key, "text"),
                    reasoning.to_owned(),
                    streaming,
                )),
        );
    }
    block.into_any_element()
}

/// The gate on screen: what the model wants to run, with the arguments it
/// named, and the three answers. The turn is frozen until one lands. The card
/// is the library's visual language with the answers in the reader's language,
/// which the component's own copy is not.
fn approval_card(
    request: ApprovalModel,
    weak: &WeakEntity<ChatView>,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let title = rust_i18n::t!("ai.chat.approval_wants", name = request.name).to_string();
    let detail = arguments_summary(&request.args);
    let answer = |allow: bool, always: bool, weak: WeakEntity<ChatView>| {
        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
            if let Some(panel) = weak.upgrade() {
                panel.update(cx, |panel, cx| panel.answer_approval(allow, always, cx));
            }
        }
    };
    let mut card = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(theme.radius(Radius::Lg))
        .border_1()
        .border_color(colors.warning.opacity(0.45))
        .bg(colors.warning.opacity(0.06))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(IconSize::Sm)
                        .color(colors.warning),
                )
                .child(
                    div()
                        .text_size(theme.text_size(TextSize::Sm))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(colors.warning)
                        .child(title),
                ),
        );
    if !detail.is_empty() {
        card = card.child(
            div()
                .w_full()
                .px_2()
                .py_1()
                .rounded(theme.radius(Radius::Sm))
                .bg(colors.sunken)
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(detail),
        );
    }
    card.child(
        div()
            .w_full()
            .flex()
            .flex_wrap()
            .justify_end()
            .gap_2()
            .child(
                Button::new("approval-deny", t("ai.chat.approval_deny"))
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .on_click(answer(false, false, weak.clone())),
            )
            .child(
                Button::new("approval-always", t("ai.chat.approval_always"))
                    .variant(ButtonVariant::Secondary)
                    .size(ControlSize::Sm)
                    .on_click(answer(true, true, weak.clone())),
            )
            .child(
                Button::new("approval-allow", t("ai.chat.approval_allow"))
                    .variant(ButtonVariant::Primary)
                    .size(ControlSize::Sm)
                    .on_click(answer(true, false, weak.clone())),
            ),
    )
    .into_any_element()
}

/// A failed turn, with its own way back: Retry sends the same user message
/// again and drops what the failure left below it.
fn error_card(error: String, weak: &WeakEntity<ChatView>, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let retry = {
        let weak = weak.clone();
        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
            if let Some(panel) = weak.upgrade() {
                panel.update(cx, |panel, cx| panel.retry(cx));
            }
        }
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded(theme.radius(Radius::Lg))
        .border_1()
        .border_color(colors.danger.opacity(0.35))
        .bg(colors.danger.opacity(0.06))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(theme.text_size(TextSize::Sm))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(colors.danger)
                        .child(t("ai.chat.error_title")),
                )
                .child(div().flex_1())
                .child(
                    Button::new("chat-error-retry", t("ai.chat.retry"))
                        .variant(ButtonVariant::Secondary)
                        .size(ControlSize::Sm)
                        .on_click(retry),
                ),
        )
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(error),
        )
        .into_any_element()
}

/// The turn compacted its own history to fit: said as a note, not as a message.
fn compacted_note(info: &CompactionInfo, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    div()
        .w_full()
        .px_2()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(theme.colors.warning)
        .child(
            rust_i18n::t!(
                "ai.chat.compacted",
                original = info.original_count,
                kept = info.kept_count,
                before = info.before_tokens,
                after = info.after_tokens
            )
            .to_string(),
        )
        .into_any_element()
}

/// One history row: its title (editable in place), when it last moved, and the
/// bin that arms the confirm bar. The loaded row wears the selection fill and
/// the accent bar the workspace tree uses, so the same state reads the same way
/// twice.
fn history_row(
    chat: &ChatView,
    session: &Session,
    row_height: f32,
    weak: &WeakEntity<ChatView>,
    cx: &mut Context<ChatView>,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let loaded = chat.session_id.as_deref() == Some(session.id.as_str());
    let id = gpui::ElementId::from(SharedString::from(format!("history-row-{}", session.id)));

    let select = {
        let weak = weak.clone();
        let session_id = session.id.clone();
        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
            if let Some(panel) = weak.upgrade() {
                let session_id = session_id.clone();
                panel.update(cx, |panel, cx| panel.open_session(&session_id, cx));
            }
        }
    };
    let rename = {
        let weak = weak.clone();
        let session_id = session.id.clone();
        move |title: &SharedString, _window: &mut Window, cx: &mut App| {
            if let Some(panel) = weak.upgrade() {
                let session_id = session_id.clone();
                let title = title.to_string();
                panel.update(cx, |panel, cx| panel.rename_session(&session_id, title, cx));
            }
        }
    };
    let delete = {
        let weak = weak.clone();
        let session_id = session.id.clone();
        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            if let Some(panel) = weak.upgrade() {
                let session_id = session_id.clone();
                panel.update(cx, |panel, cx| panel.delete_session(&session_id, cx));
            }
        }
    };
    let stamp = timestamp_of(&session.updated_at).map(|at| relative_label(at, cx));
    div()
        .id(id.clone())
        .h(px(row_height))
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .pl_1()
        .pr_1()
        .rounded(theme.radius(Radius::Md))
        .when(loaded, |row| row.bg(colors.active))
        .child(
            div()
                .w(px(2.))
                .h(px((row_height - 12.).max(6.)))
                .flex_none()
                .rounded_full()
                .when(loaded, |bar| bar.bg(colors.accent)),
        )
        .child(
            // A press on the title starts the edit, so it must not also reach
            // the row: the row's select would load the session and unmount the
            // list under the field that just took focus.
            div()
                .id((id.clone(), "title-cell"))
                .flex_1()
                .min_w_0()
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    InlineEdit::new((id.clone(), "title"), session.title.clone()).on_commit(rename),
                ),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .children(stamp),
        )
        .child(
            IconButton::new((id.clone(), "delete"), IconName::Trash2)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip(t("ai.chat.delete"))
                .on_click(delete),
        )
        .on_click(select)
        .into_any_element()
}

/// The `/` commands the composer offers. Each one acts on the panel rather
/// than on the text, so Enter cannot also send it.
fn starter_commands() -> Vec<Choice> {
    vec![
        Choice::new("new", t("ai.chat.new_chat")),
        Choice::new("history", t("ai.chat.history")),
        Choice::new("clear", t("ai.chat.clear")),
    ]
}

/// The three prompts the empty panel offers, and the text a pick puts in the
/// box. Both the label and the text are the same words, so the user can edit
/// what they see before it is sent.
fn starter_prompts() -> Vec<(String, IconName)> {
    vec![
        (t("ai.chat.starter_pane"), IconName::Terminal),
        (t("ai.chat.starter_fleet"), IconName::Bot),
        (t("ai.chat.starter_layout"), IconName::LayoutDashboard),
    ]
}

/// The quiet `/` hint in the composer's tool row: a hint, not a button, so it
/// claims nothing a press could not do.
fn command_hint(cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(theme.colors.fg_subtle)
        .child(
            Icon::new(IconName::Command)
                .size(IconSize::Xs)
                .color(theme.colors.fg_subtle),
        )
        .child("/")
        .into_any_element()
}

/// The store's `YYYY-MM-DDTHH:MM:SS` (UTC) as a timestamp. Anything else
/// reads as no time rather than a wrong one.
fn timestamp_of(stamp: &str) -> Option<jiff::Timestamp> {
    let civil = stamp.parse::<jiff::civil::DateTime>().ok()?;
    let zoned = civil.to_zoned(jiff::tz::TimeZone::UTC).ok()?;
    Some(zoned.timestamp())
}

/// How long ago `at` was, in the chrome's language. Computed when the row is
/// drawn rather than left to a ticking element: a self-repainting clock inside
/// a row that is not laid out again leaves its previous string's pixels beside
/// the new one, and nothing here needs a literal tick.
fn relative_label(at: jiff::Timestamp, cx: &App) -> String {
    relative_text(cx, at, jiff::Timestamp::now()).to_string()
}

/// How long a tool call took, in the units a card can show.
fn took_label(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.1} s", ms as f64 / 1000.)
    }
}

/// The provider a turn runs on: the first one with a key, which is the rule
/// the panel has always had. Choosing between several is the picker's job.
fn usable_providers() -> Vec<ApiProviderConfig> {
    api_provider::load_all()
        .into_iter()
        .filter(|provider| !provider.api_key.trim().is_empty())
        .collect()
}

fn level_of(value: &str) -> Option<ThinkingLevel> {
    ThinkingLevel::all()
        .iter()
        .copied()
        .find(|level| level.as_str() == value)
}

fn engine_for(
    provider: &ApiProviderConfig,
    first_message: &str,
) -> Result<OrchestratorEngine, String> {
    let roles = roles_summary();
    let prompt = build_system_prompt(&roles, "", detect_language(first_message));
    OrchestratorEngine::new(provider)
        .map(|engine| engine.with_system_prompt(prompt))
        .map_err(|error| error.to_string())
}

/// The roles the model may delegate to, as one line each. A client that has
/// never seeded them sends none, and the prompt says so by saying nothing.
fn roles_summary() -> String {
    RoleRegistry::load()
        .unwrap_or_default()
        .iter()
        .map(|role| format!("- {}: {}", role.id, role.description))
        .collect::<Vec<String>>()
        .join("\n")
}

/// The store's rows, most recently touched first; a store that cannot be read
/// is an empty list rather than a failed panel.
fn read_sessions() -> Vec<Session> {
    SessionManager::shared()
        .and_then(|manager| manager.list_sessions())
        .unwrap_or_default()
}

fn message(session_id: &str, role: &str, content: String, sequence: i64) -> ChatMessage {
    ChatMessage {
        id: 0,
        session_id: session_id.to_owned(),
        role: role.to_owned(),
        content,
        agent_activities: None,
        reasoning_text: None,
        reasoning_duration_ms: None,
        tool_calls: Vec::new(),
        usage: None,
        timestamp: crate::util::timestamp_now(),
        sequence,
    }
}

/// Save one message, if the conversation has a session to save it under. A
/// failure here must not stop the turn, so it is logged rather than shown.
fn persist(message: &ChatMessage) {
    if message.session_id.is_empty() {
        return;
    }
    match SessionManager::shared() {
        Ok(manager) => {
            if let Err(error) = manager.add_message(message) {
                tracing::warn!(target: "zenix-chat", %error, "the message was not saved");
            }
        }
        Err(error) => {
            tracing::warn!(target: "zenix-chat", %error, "the session store was not opened")
        }
    }
}

/// What is written above a message. The store's own two roles, in the words a
/// reader expects.
fn role_label(role: &str) -> String {
    match role {
        "user" => t("ai.chat.role_you"),
        _ => t("ai.chat.role_assistant"),
    }
}

/// The role the API is spoken to with. The store says `orchestrator` where the
/// wire says `assistant` (`ChatMessageRef` documents the four it accepts), and
/// the store keeps only these two.
fn wire_role(role: &str) -> &'static str {
    match role {
        "user" => "user",
        _ => "assistant",
    }
}

/// The message a retry sends again: the last user message that actually said
/// something. Nothing after it survives the retry, so the failed turn's
/// leftover placeholder is what gets cut.
fn retry_target(messages: &[ChatMessage]) -> Option<usize> {
    messages
        .iter()
        .rposition(|message| message.role == "user" && !message.content.is_empty())
}

/// A tool's arguments as one line: the keys the model filled in, with long
/// values cut, rather than the raw JSON. A card that is opened shows the answer
/// instead.
fn arguments_summary(args: &serde_json::Value) -> String {
    const VALUE_MAX: usize = 60;
    let Some(object) = args.as_object() else {
        return String::new();
    };
    object
        .iter()
        .map(|(key, value)| match value {
            serde_json::Value::String(text) => {
                let cut: String = text.chars().take(VALUE_MAX).collect();
                if text.chars().count() > VALUE_MAX {
                    format!("{key}: \"{cut}...\"")
                } else {
                    format!("{key}: \"{cut}\"")
                }
            }
            other => format!("{key}: {other}"),
        })
        .collect::<Vec<String>>()
        .join("  ")
}

/// Cut a tool's answer to what a collapsed card shows, one line.
fn summarize(text: &str, max: usize) -> String {
    let one_line: String = text
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    if one_line.chars().count() <= max {
        return one_line;
    }
    let cut: String = one_line.chars().take(max).collect();
    format!("{cut}...")
}

/// Epoch milliseconds, for how long a tool call took.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

/// What folding one event left for the panel besides the message itself.
enum Folded {
    /// Nothing but the message changed.
    Message,
    /// The turn compacted its own history to fit.
    Compacted(CompactionInfo),
    /// The turn failed.
    Error(String),
}

/// Fold one stream event into the message it belongs to.
///
/// Pure, so the shapes the engine actually emits — a token, a tool call, the
/// result that answers it, a failure — can be tested without a window, a
/// daemon or a model.
fn fold_event(message: &mut ChatMessage, event: &DispatchStreamEvent, now_ms: u64) -> Folded {
    match event {
        DispatchStreamEvent::Token(text) => {
            message.content.push_str(text);
            Folded::Message
        }
        DispatchStreamEvent::Thinking { text, .. } => {
            message
                .reasoning_text
                .get_or_insert_with(String::new)
                .push_str(text);
            Folded::Message
        }
        DispatchStreamEvent::ToolCall { id, name, args } => {
            message.tool_calls.push(ToolCallState {
                id: id.clone(),
                name: name.clone(),
                args: args.clone(),
                status: ToolCallStatus::Running,
                result_summary: None,
                started_at: now_ms,
                completed_at: None,
            });
            Folded::Message
        }
        DispatchStreamEvent::ToolResult {
            id,
            content,
            is_error,
            ..
        } => {
            // The last call with this id: a model that reuses an id gets a
            // second card rather than an answer landing on the wrong one.
            if let Some(call) = message
                .tool_calls
                .iter_mut()
                .rev()
                .find(|call| &call.id == id)
            {
                call.status = if *is_error {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                };
                call.result_summary = Some(summarize(content, 160));
                call.completed_at = Some(now_ms);
            }
            Folded::Message
        }
        DispatchStreamEvent::RoundStart { .. } => {
            // A new round: the message keeps accumulating, and the reasoning
            // above it is already drawn. Its duration is measured by the view,
            // which is where the clock is. A round that starts after a tool
            // result is a new block of text — the model's own first round may
            // have ended mid-line — so the join is a paragraph break rather
            // than a run-on ("…now.## Heading").
            if !message.content.is_empty() && !message.content.ends_with('\n') {
                message.content.push_str("\n\n");
            } else if !message.content.is_empty() && !message.content.ends_with("\n\n") {
                message.content.push('\n');
            }
            Folded::Message
        }
        DispatchStreamEvent::Compacted(info) => Folded::Compacted(info.clone()),
        DispatchStreamEvent::Error(error) => Folded::Error(error.clone()),
        DispatchStreamEvent::Usage(usage) => {
            message.usage = Some(*usage);
            Folded::Message
        }
        DispatchStreamEvent::Done => Folded::Message,
        // The dispatch-directive path is the older orchestration (the engine's
        // own agent dispatch); the panel's turns go through tools, so these
        // carry nothing to draw. They stay matched rather than ignored so a
        // future variant cannot be missed silently.
        DispatchStreamEvent::Dispatch(_)
        | DispatchStreamEvent::AgentOutput { .. }
        | DispatchStreamEvent::AgentComplete(_) => Folded::Message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_user_message() -> ChatMessage {
        message("s1", "user", "hello".into(), 1)
    }

    fn a_placeholder() -> ChatMessage {
        message("s1", "orchestrator", String::new(), 2)
    }

    #[test]
    fn tokens_and_reasoning_accumulate_in_order() {
        let mut message = a_placeholder();
        fold_event(
            &mut message,
            &DispatchStreamEvent::Thinking {
                round: 0,
                text: "check ".into(),
            },
            10,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::Thinking {
                round: 0,
                text: "dates".into(),
            },
            11,
        );
        fold_event(&mut message, &DispatchStreamEvent::Token("The ".into()), 12);
        fold_event(&mut message, &DispatchStreamEvent::Token("plan".into()), 13);
        assert_eq!(message.reasoning_text.as_deref(), Some("check dates"));
        assert_eq!(message.content, "The plan");
    }

    #[test]
    fn a_tool_result_lands_on_the_call_it_answers() {
        let mut message = a_placeholder();
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolCall {
                id: "c1".into(),
                name: "read_output".into(),
                args: serde_json::json!({"pane": "api", "lines": 20}),
            },
            100,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolResult {
                id: "c1".into(),
                tool_name: "read_output".into(),
                content: "line one\nline two".into(),
                is_error: false,
            },
            140,
        );
        let call = &message.tool_calls[0];
        assert_eq!(call.status, ToolCallStatus::Completed);
        assert_eq!(call.result_summary.as_deref(), Some("line one line two"));
        assert_eq!(call.started_at, 100);
        assert_eq!(call.completed_at, Some(140));
    }

    #[test]
    fn a_failed_call_is_marked_failed_and_reported() {
        let mut message = a_placeholder();
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolCall {
                id: "c1".into(),
                name: "wait_for_output".into(),
                args: serde_json::json!({}),
            },
            1,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolResult {
                id: "c1".into(),
                tool_name: "wait_for_output".into(),
                content: "timed out after 30s".into(),
                is_error: true,
            },
            2,
        );
        assert_eq!(message.tool_calls[0].status, ToolCallStatus::Failed);
    }

    /// A round that begins after a tool result is a new block: the join is a
    /// paragraph break, so the model's second-round heading is a heading.
    #[test]
    fn a_second_round_starts_a_new_paragraph() {
        let mut message = a_placeholder();
        fold_event(
            &mut message,
            &DispatchStreamEvent::Token("Reading.".into()),
            1,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::RoundStart { round: 1 },
            2,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::Token("## Next".into()),
            3,
        );
        assert_eq!(message.content, "Reading.\n\n## Next");
        // A round with nothing said before it adds no leading blank lines.
        let mut empty = a_placeholder();
        fold_event(&mut empty, &DispatchStreamEvent::RoundStart { round: 1 }, 1);
        assert!(empty.content.is_empty());
        // One newline is completed to the paragraph break, not doubled.
        let mut one = a_placeholder();
        fold_event(&mut one, &DispatchStreamEvent::Token("a\n".into()), 1);
        fold_event(&mut one, &DispatchStreamEvent::RoundStart { round: 1 }, 2);
        fold_event(&mut one, &DispatchStreamEvent::Token("b".into()), 3);
        assert_eq!(one.content, "a\n\nb");
    }

    #[test]
    fn a_compaction_is_reported_as_a_note_not_as_a_message() {
        let mut message = a_placeholder();
        let folded = fold_event(
            &mut message,
            &DispatchStreamEvent::Compacted(CompactionInfo {
                original_count: 20,
                compressed_count: 12,
                kept_count: 8,
                before_tokens: 9000,
                after_tokens: 3000,
            }),
            1,
        );
        assert!(matches!(folded, Folded::Compacted(info) if info.kept_count == 8));
        assert!(message.content.is_empty());
    }

    #[test]
    fn a_turns_bill_lands_on_the_message() {
        let mut message = a_placeholder();
        fold_event(
            &mut message,
            &DispatchStreamEvent::Usage(UsageInfo {
                input_tokens: 1200,
                output_tokens: 300,
            }),
            1,
        );
        assert_eq!(message.usage.map(|usage| usage.total_tokens()), Some(1500));
    }

    #[test]
    fn an_arguments_summary_names_the_keys_and_cuts_long_values() {
        let long = "x".repeat(80);
        let summary = arguments_summary(&serde_json::json!({
            "pane": "api",
            "command": long,
            "focus": true,
        }));
        assert!(summary.contains("pane: \"api\""));
        assert!(summary.contains("focus: true"));
        assert!(summary.contains("..."));
        assert!(!summary.contains(&"x".repeat(61)));
        assert_eq!(arguments_summary(&serde_json::json!("bare")), "");
    }

    #[test]
    fn an_answers_summary_is_one_line() {
        assert_eq!(summarize("a\nb", 10), "a b");
        assert_eq!(summarize("abcdef", 3), "abc...");
    }

    /// The retry is the last user message that said something; the empty
    /// placeholder the failed turn left is cut with it.
    #[test]
    fn the_retry_is_the_last_user_message_that_said_something() {
        let messages = vec![
            a_user_message(),
            message("s1", "orchestrator", "answer".into(), 2),
            message("s1", "user", "again".into(), 3),
            a_placeholder(),
        ];
        assert_eq!(retry_target(&messages), Some(2));
        let only_failed = vec![a_user_message(), a_placeholder()];
        assert_eq!(retry_target(&only_failed), Some(0));
        assert_eq!(retry_target(&[a_placeholder()]), None);
    }

    /// The store's timestamp parses into a timestamp, or names nothing.
    #[test]
    fn the_store_timestamp_parses_or_names_nothing() {
        assert!(timestamp_of("2026-10-27T12:34:56").is_some());
        assert!(timestamp_of("yesterday").is_none());
        assert!(timestamp_of("").is_none());
    }

    #[test]
    fn the_store_s_roles_are_labelled_for_a_reader() {
        assert_eq!(role_label("user"), t("ai.chat.role_you"));
        assert_eq!(role_label("orchestrator"), t("ai.chat.role_assistant"));
        assert_ne!(role_label("user"), role_label("orchestrator"));
    }

    #[test]
    fn the_api_is_spoken_to_with_its_own_role_words() {
        assert_eq!(wire_role("user"), "user");
        assert_eq!(wire_role("orchestrator"), "assistant");
    }

    #[test]
    fn a_tools_time_is_said_in_the_unit_it_needs() {
        assert_eq!(took_label(940), "940 ms");
        assert_eq!(took_label(1500), "1.5 s");
    }

    #[test]
    fn a_level_round_trips_through_its_wire_spelling() {
        for level in ThinkingLevel::all() {
            assert_eq!(level_of(level.as_str()), Some(*level));
        }
        assert_eq!(level_of("impossible"), None);
    }
}
