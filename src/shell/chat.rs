//! The AI chat panel: the right-hand panel's one block (M6's UI half, second
//! part).
//!
//! The domain it drives is UI-free and already in place (`crate::ai`):
//! [`OrchestratorEngine`] streams a turn, [`crate::ai::tools`] are the six
//! herdr tools the model may call, [`SessionManager`] saves what was said.
//! What this file owns is the conversation as the user sees it — one message
//! list, the reasoning a turn thought with, and a card per tool call — and the
//! turn's own lifecycle (send, stream, stop, persist).
//!
//! Three decisions are worth naming:
//!
//! - A turn runs on tokio ([`crate::TOKIO_HANDLE`]) because the engine does
//!   HTTP. Its events come back over a channel that a GPUI task drains, the
//!   same shape `TerminalView` uses for the daemon's pushes: the panel is
//!   re-rendered because an event arrived, not because a frame was due.
//! - Tool calls act on the daemon's *focused* workspace: the executor is built
//!   with an empty workspace id, which [`ToolExecutor`] documents as "whatever
//!   the daemon has focused". The panel therefore needs no snapshot, and never
//!   mutates its own state while it draws.
//! - A message carries the database's own vocabulary (`user` / `orchestrator`,
//!   the only two roles `src/ai/session.rs` accepts); what is *written* above
//!   it is this panel's business. Tool names and status words are shown as the
//!   wire spells them (`spawn_worker`, `running`), so a card can be read
//!   against what the model asked for; the surrounding copy goes through
//!   `crate::i18n`.
//!
//! Deliberately not here yet: markdown rendering (the old `ai/markdown.rs` went
//! with the M1 UI), model or workspace pickers, the `@` mention popup, session
//! history, and reasoning-token collapsing per round.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::searchable_list::{SearchableListItem, SearchableVec};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IndexPath, Sizable as _, Size, Theme, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::registry::{AsideBlock, AsideTab, Invocation};
use crate::ai::compactor::Compactor;
use crate::ai::endpoint::ApiEndpoint;
use crate::ai::engine::{
    ChatMessageRef, DispatchStreamEvent, OrchestratorEngine, build_system_prompt, detect_language,
};
use crate::ai::roles::RoleRegistry;
use crate::ai::session::{ChatMessage, Session, SessionManager, ToolCallState, ToolCallStatus};
use crate::ai::tools::{Tool, ToolExecutor, builtin_tools};
use crate::api_provider::{self, ApiProviderConfig, ThinkingLevel};
use crate::i18n::t;

/// The right-hand panel's blocks this module contributes: the conversation is
/// the panel's first tab.
pub(crate) const ASIDE: &[AsideBlock] = &[AsideBlock {
    tab: AsideTab::Chat,
    render,
}];

/// How many tool rounds one turn may take. The engine has its own ceiling; this
/// is the tighter one the panel asks for.
const MAX_TOOL_ROUNDS: u32 = 8;

/// The panel's block: state that needs a window (its input owns a focus handle)
/// is created by the frame that first draws it and kept per window from then
/// on. `use_keyed_state` is GPUI's own way to do that, and it re-renders the
/// shell whenever the panel changes.
fn render(_: &Invocation, window: &mut Window, cx: &mut App) -> AnyElement {
    window
        .use_keyed_state("chat-panel", cx, ChatPanel::new)
        .into_any_element()
}

struct ChatPanel {
    /// Everything said, oldest first. The message the running turn appends to
    /// is `streaming`.
    messages: Vec<ChatMessage>,
    streaming: Option<usize>,
    input: Entity<InputState>,
    /// The provider the next turn runs on, and the one the composer names. Read
    /// once when the panel is built and then owned by the composer: a picker
    /// that re-read the file would forget what the user just chose.
    provider: Option<ApiProviderConfig>,
    /// The model picker's list: every configured provider that carries a key,
    /// which is the whole catalog this client has (`crate::api_provider`, the
    /// same list `crate::ai::engine` picks from — no second enum).
    model_select: Entity<SelectState<SearchableVec<ModelChoice>>>,
    /// The reasoning-effort picker: the four levels `ThinkingLevel` defines.
    effort_select: Entity<SelectState<SearchableVec<EffortChoice>>>,
    /// Why the last send or the last turn failed, until the next one starts.
    error: Option<String>,
    /// What the running turn compacted away, if it had to.
    compacted: Option<String>,
    /// When the streaming turn's first reasoning token arrived, so the block
    /// can say how long it thought for.
    reasoning_started: Option<u64>,
    /// The session the messages are saved under; created with the first send.
    session_id: Option<String>,
    /// The running turn's engine task, so Stop can end it.
    turn: Option<tokio::task::JoinHandle<()>>,
    /// The task draining the turn's events.
    drain: Option<Task<()>>,
    /// The tool cards and reasoning blocks the user opened, by their own id.
    open: Vec<String>,
    scroll: ScrollHandle,
    _input: Subscription,
    _model: Subscription,
    _effort: Subscription,
}

impl ChatPanel {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(t("ai.placeholder")));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |panel: &mut Self, _input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => panel.send(window, cx),
                InputEvent::Change => cx.notify(),
                InputEvent::Focus | InputEvent::Blur => {}
            },
        );
        let provider = available_provider();
        let models: Vec<ModelChoice> = usable_providers().iter().map(ModelChoice::of).collect();
        let chosen = provider.as_ref().and_then(|provider| {
            models
                .iter()
                .position(|choice| choice.name == provider.name)
        });
        let model_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(models),
                chosen.map(|row| IndexPath::default().row(row)),
                window,
                cx,
            )
        });
        let levels = levels();
        let level = provider
            .as_ref()
            .map(|provider| provider.thinking_level)
            .unwrap_or_default();
        let effort_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(levels),
                Some(IndexPath::default().row(level_index(level))),
                window,
                cx,
            )
        });
        let model = cx.subscribe_in(
            &model_select,
            window,
            |panel: &mut Self,
             _select,
             event: &SelectEvent<SearchableVec<ModelChoice>>,
             _window,
             cx| {
                let SelectEvent::Confirm(Some(name)) = event else {
                    return;
                };
                if let Some(choice) = usable_providers()
                    .into_iter()
                    .find(|provider| provider.name == name.as_ref())
                {
                    panel.provider = Some(choice);
                    cx.notify();
                }
            },
        );
        let effort = cx.subscribe_in(
            &effort_select,
            window,
            |panel: &mut Self,
             _select,
             event: &SelectEvent<SearchableVec<EffortChoice>>,
             _window,
             cx| {
                let SelectEvent::Confirm(Some(level)) = event else {
                    return;
                };
                if let Some(provider) = panel.provider.as_mut()
                    && let Some(level) = level_of(level.as_ref())
                {
                    provider.thinking_level = level;
                    cx.notify();
                }
            },
        );
        Self {
            messages: Vec::new(),
            streaming: None,
            input,
            provider,
            model_select,
            effort_select,
            error: None,
            compacted: None,
            reasoning_started: None,
            session_id: None,
            turn: None,
            drain: None,
            open: Vec::new(),
            scroll: ScrollHandle::new(),
            _input: subscription,
            _model: model,
            _effort: effort,
        }
    }

    /// Whether a turn is running.
    fn is_streaming(&self) -> bool {
        self.streaming.is_some()
    }

    /// Start a turn: the user's text goes into the list, an empty assistant
    /// message below it waits for the tokens, and the engine is spawned on
    /// tokio. Nothing here waits for the model.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_streaming() {
            return;
        }
        let text = self.input.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        // A provider the composer already holds is the user's choice; the file
        // is only read when there is none, so a picker cannot be overwritten
        // between a click and the next turn.
        if self.provider.is_none() {
            self.provider = available_provider();
        }
        let Some(provider) = self.provider.clone() else {
            // The text stays in the box: nothing was sent.
            self.error = Some(format!(
                "{} {}",
                t("ai.config_empty"),
                t("ai.config_empty_hint")
            ));
            cx.notify();
            return;
        };
        let engine = match engine_for(&provider, &text) {
            Ok(engine) => engine,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let Some(runtime) = crate::TOKIO_HANDLE.get().cloned() else {
            self.error = Some("no tokio runtime: the client started without one".to_owned());
            cx.notify();
            return;
        };

        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.error = None;
        self.compacted = None;
        self.reasoning_started = None;

        let session_id = self.ensure_session(&text, &provider);
        let sequence = self.messages.last().map_or(1, |last| last.sequence + 1);
        let user = message(&session_id, "user", text.clone(), sequence);
        persist(&user);
        self.messages.push(user);

        let index = self.messages.len();
        self.messages.push(message(
            &session_id,
            "orchestrator",
            String::new(),
            sequence + 1,
        ));
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
        tracing::info!(
            target: "zenix-chat",
            provider = %provider.name,
            model = %provider.model,
            session = %session_id,
            "turn started"
        );
        self.turn = Some(runtime.spawn(async move {
            let tools = builtin_tools();
            let tools: Vec<&dyn Tool> = tools.iter().map(|tool| tool.as_ref()).collect();
            // The empty workspace id is the daemon's own focused workspace: the
            // panel has no snapshot of its own to name one from.
            let executor = ToolExecutor::new(ApiEndpoint::local(), String::new());
            let compactor = Compactor::default_compactor();
            let outcome = engine
                .chat_with_tools(
                    &history,
                    &tools,
                    &executor,
                    &compactor,
                    MAX_TOOL_ROUNDS,
                    move |event| {
                        let _ = events.send(event);
                    },
                )
                .await;
            if let Err(error) = outcome {
                // A turn that fails inside the loop reports it through an
                // event; this is the path where the failure is only logged.
                tracing::warn!(target: "zenix-chat", %error, "the turn did not finish");
            }
        }));

        // The drain: one event in, one repaint. It ends when the engine task
        // drops its sender — which is also how Stop, an abort, ends it.
        self.drain = Some(cx.spawn(async move |panel, cx| {
            loop {
                let incoming = incoming.clone();
                let next = cx
                    .background_executor()
                    .spawn(async move { incoming.recv() })
                    .await;
                let Ok(event) = next else { break };
                if panel
                    .update(cx, |panel, cx| panel.apply(event, cx))
                    .is_err()
                {
                    break;
                }
            }
            let _ = panel.update(cx, |panel, cx| panel.finish(cx));
        }));
        self.scroll.scroll_to_bottom();
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
            Folded::Compacted(note) => self.compacted = Some(note),
            Folded::Error(error) => self.error = Some(error),
        }
        self.scroll.scroll_to_bottom();
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
            persist(message);
        }
        cx.notify();
    }

    /// Open or close one card.
    fn toggle(&mut self, id: String, cx: &mut Context<Self>) {
        if let Some(at) = self.open.iter().position(|open| open == &id) {
            self.open.remove(at);
        } else {
            self.open.push(id);
        }
        cx.notify();
    }

    /// Forget the conversation on screen, so the next send starts a new session
    /// on disk as well.
    fn new_session(&mut self, cx: &mut Context<Self>) {
        if self.is_streaming() {
            self.stop(cx);
        }
        self.messages.clear();
        self.open.clear();
        self.error = None;
        self.compacted = None;
        self.session_id = None;
        cx.notify();
    }

    /// The session the messages are saved under, created with the first send so
    /// a conversation that never happened leaves nothing behind.
    fn ensure_session(&mut self, first_message: &str, provider: &ApiProviderConfig) -> String {
        if let Some(id) = &self.session_id {
            return id.clone();
        }
        let Ok(manager) = SessionManager::open() else {
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
}

impl Render for ChatPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mut rows: Vec<AnyElement> = Vec::with_capacity(self.messages.len());
        for (index, message) in self.messages.iter().enumerate() {
            rows.push(message_row(index, message, &self.open, cx));
        }

        // The panel's tab strip already says which tab this is, so the block
        // opens with the conversation rather than a second title row.
        let mut column = v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(theme.sidebar)
            .text_color(theme.foreground);
        if rows.is_empty() {
            column = column.child(empty_state(self, &theme));
        } else {
            column = column.child(
                v_flex()
                    .id("chat-messages")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .px_2()
                    .py_2()
                    .gap_3()
                    .track_scroll(&self.scroll)
                    .overflow_y_scrollbar()
                    .children(rows),
            );
        }
        column
            .child(notes(self, &theme))
            .child(composer(self, &theme, cx))
            .into_any_element()
    }
}

/// A message, with the reasoning it thought with above and one card per tool
/// call in the middle: what was thought, what was called, what was answered.
fn message_row(
    index: usize,
    message: &ChatMessage,
    open: &[String],
    cx: &mut Context<ChatPanel>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let mut row = v_flex().w_full().gap_1().child(who_row(message, &theme));
    if let Some(reasoning) = message.reasoning_text.as_deref() {
        row = row.child(reasoning_block(index, reasoning, message, open, cx));
    }
    for call in &message.tool_calls {
        row = row.child(tool_card(index, call, open, cx));
    }
    for line in message.content.split('\n') {
        row = row.child(div().w_full().text_sm().child(line.to_owned()));
    }
    row.into_any_element()
}

/// The reasoning a turn thought with, folded away until it is asked for: it is
/// the model talking to itself, and it is long.
fn reasoning_block(
    index: usize,
    reasoning: &str,
    message: &ChatMessage,
    open: &[String],
    cx: &mut Context<ChatPanel>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let id = format!("reasoning-{index}");
    let element = SharedString::from(id.clone());
    let expanded = open.contains(&id);
    let seconds = message
        .reasoning_duration_ms
        .map(|ms| format!(" · {:.1}s", ms as f64 / 1000.))
        .unwrap_or_default();
    let toggle =
        cx.listener(move |panel, _: &ClickEvent, _window, cx| panel.toggle(id.clone(), cx));
    let mut block = v_flex()
        .w_full()
        .gap_1()
        .rounded_sm()
        .border_1()
        .border_color(theme.border)
        .child(
            h_flex()
                .id(element)
                .w_full()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .cursor_pointer()
                .on_click(toggle)
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("reasoning{seconds}")),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(if expanded { "-" } else { "+" }),
                ),
        );
    if expanded {
        for line in reasoning.split('\n') {
            block = block.child(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(line.to_owned()),
            );
        }
    }
    block.into_any_element()
}

/// One tool call: its name, how it ended and how long it took, with the
/// arguments it was given; its answer is one click away.
fn tool_card(
    index: usize,
    call: &ToolCallState,
    open: &[String],
    cx: &mut Context<ChatPanel>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let id = format!("tool-{index}-{}", call.id);
    let element = SharedString::from(id.clone());
    let expanded = open.contains(&id);
    let (status, color) = match call.status {
        ToolCallStatus::Running => ("running", theme.accent),
        ToolCallStatus::Completed => ("done", theme.success),
        ToolCallStatus::Failed => ("failed", theme.danger),
    };
    let duration = call
        .completed_at
        .map(|end| format!("{:.1}s", end.saturating_sub(call.started_at) as f64 / 1000.));
    let toggle =
        cx.listener(move |panel, _: &ClickEvent, _window, cx| panel.toggle(id.clone(), cx));
    let summary = arguments_summary(&call.args);
    // A call still running carries the accent on its edge, so a turn that is
    // waiting on a tool reads before its status word does.
    let border = if call.status == ToolCallStatus::Running {
        theme.accent
    } else {
        theme.border
    };
    let mut card = v_flex()
        .w_full()
        .rounded_sm()
        .border_1()
        .border_color(border)
        .bg(theme.muted)
        .child(
            h_flex()
                .id(element)
                .w_full()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .cursor_pointer()
                .on_click(toggle)
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.foreground)
                        .child(call.name.clone()),
                )
                .child(div().text_xs().text_color(color).child(status))
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(duration.unwrap_or_default()),
                )
                // The answer is one click away, and the control that says so is
                // the same one the reasoning block carries.
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(if expanded { "-" } else { "+" }),
                ),
        );
    // A call with no arguments (`list_workers`) gets no line for them.
    if !summary.is_empty() {
        card = card.child(
            div()
                .px_2()
                .pb_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(summary),
        );
    }
    card.when(expanded, |card| {
        card.child(
            div()
                .px_2()
                .pb_1()
                .text_xs()
                .text_color(theme.foreground)
                .child(call.result_summary.clone().unwrap_or_default()),
        )
    })
    .into_any_element()
}

/// The composer: what this conversation is running on, the message box, and the
/// two pickers that decide the next turn. The pickers read the catalog this
/// client actually has — the configured providers and the levels
/// [`ThinkingLevel`] defines — so neither is a list invented here.
fn composer(panel: &ChatPanel, theme: &Theme, cx: &mut Context<ChatPanel>) -> AnyElement {
    v_flex()
        .flex_none()
        .w_full()
        .border_t_1()
        .border_color(theme.border)
        .child(context_line(panel, theme, cx))
        .child(
            h_flex()
                .w_full()
                .px_2()
                .pt_1()
                .gap_2()
                .items_center()
                .child(div().flex_1().min_w_0().child(Input::new(&panel.input)))
                .child(send_button(panel, theme, cx)),
        )
        .child(
            h_flex()
                .w_full()
                .px_2()
                .py_1()
                .gap_1()
                .items_center()
                // The model's name is the longer of the two, so it takes the
                // room and the level keeps a fixed column.
                .child(
                    div().flex_1().min_w_0().child(
                        Select::new(&panel.model_select)
                            .with_size(Size::XSmall)
                            .placeholder(t("ai.config_empty"))
                            .menu_width(px(260.))
                            .disabled(panel.provider.is_none()),
                    ),
                )
                .child(
                    div().w(px(120.)).child(
                        Select::new(&panel.effort_select)
                            .with_size(Size::XSmall)
                            .placeholder(t("ai.chat.effort"))
                            .menu_width(px(180.))
                            .disabled(panel.provider.is_none()),
                    ),
                ),
        )
        .into_any_element()
}

/// One line naming what the next turn runs on, and the two controls that act on
/// the conversation rather than on its text.
fn context_line(panel: &ChatPanel, theme: &Theme, cx: &mut Context<ChatPanel>) -> AnyElement {
    let detail = match panel.provider.as_ref() {
        Some(provider) if panel.is_streaming() => {
            format!("{} · {}", provider.name, t("ai.chat.streaming"))
        }
        Some(provider) => [
            provider.name.as_str(),
            provider.model.as_str(),
            provider.thinking_level.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<&str>>()
        .join(" · "),
        None => format!("{} {}", t("ai.config_empty"), t("ai.config_empty_hint")),
    };
    let stop = cx.listener(|panel, _: &ClickEvent, _window, cx| panel.stop(cx));
    let fresh = cx.listener(|panel, _: &ClickEvent, _window, cx| panel.new_session(cx));
    h_flex()
        .flex_none()
        .w_full()
        .px_2()
        .pt_1()
        .gap_2()
        .items_center()
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(detail),
        )
        .child(button(
            "chat-new",
            t("ai.chat.new_session"),
            true,
            fresh,
            theme,
        ))
        .children(
            panel
                .is_streaming()
                .then(|| button("chat-stop", t("ai.chat.stop"), true, stop, theme)),
        )
        .into_any_element()
}

/// The button that sends what is in the box: the prototype's round trigger,
/// lit while there is something to send and dead while a turn runs. Enter
/// sends the same turn, so the button is the mouse-shaped way of asking.
fn send_button(panel: &ChatPanel, theme: &Theme, cx: &mut Context<ChatPanel>) -> AnyElement {
    let send = cx.listener(|panel, _: &ClickEvent, window, cx| panel.send(window, cx));
    let ready = !panel.is_streaming() && !panel.input.read(cx).value().trim().is_empty();
    div()
        .id("chat-send")
        .size(px(26.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .when(ready, |button| {
            button
                .bg(theme.accent)
                .text_color(theme.accent_foreground)
                .cursor_pointer()
                .on_click(send)
        })
        .when(!ready, |button| {
            button.bg(theme.muted).text_color(theme.muted_foreground)
        })
        .child(Icon::default().data(ARROW_UP_ICON).w(px(14.)).h(px(14.)))
        .into_any_element()
}

/// The last failure and the last compaction, above the message box where they
/// cannot be mistaken for something the model said.
fn notes(panel: &ChatPanel, theme: &Theme) -> AnyElement {
    let mut column = v_flex().w_full().gap_1();
    if let Some(error) = &panel.error {
        column = column.child(
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.danger)
                .child(error.clone()),
        );
    }
    if let Some(note) = &panel.compacted {
        column = column.child(
            div()
                .px_2()
                .text_xs()
                .text_color(theme.warning)
                .child(note.clone()),
        );
    }
    column.into_any_element()
}

fn empty_state(panel: &ChatPanel, theme: &Theme) -> AnyElement {
    let hint = if panel.provider.is_some() {
        t("ai.chat.empty_hint")
    } else {
        format!("{} {}", t("ai.config_empty"), t("ai.config_empty_hint"))
    };
    v_flex()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .p_4()
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(hint),
        )
        .into_any_element()
}

/// A card-styled button, as the shell's own title bar draws them. A disabled
/// one keeps its shape but takes no clicks.
fn button(
    id: &'static str,
    label: String,
    enabled: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    theme: &Theme,
) -> AnyElement {
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_sm()
        .text_xs()
        .bg(if enabled {
            theme.secondary
        } else {
            theme.muted
        })
        .text_color(if enabled {
            theme.secondary_foreground
        } else {
            theme.muted_foreground
        })
        .when(enabled, move |button| {
            button.cursor_pointer().on_click(click)
        })
        .child(label)
        .into_any_element()
}

/// The composer's send trigger, the same stroke the prototype draws there.
const ARROW_UP_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M6 11l6-6 6 6"/></svg>"#;

/// What one stream event did, for the panel to act on beyond the message.
enum Folded {
    /// Nothing but the message changed.
    Message,
    /// The turn compacted its own history to fit.
    Compacted(String),
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
            // above it is already drawn. Its duration is measured by the panel,
            // which is where the clock is.
            Folded::Message
        }
        DispatchStreamEvent::Compacted(info) => Folded::Compacted(format!(
            "compacted {} messages to {} ({} -> {} tokens)",
            info.original_count, info.compressed_count, info.before_tokens, info.after_tokens
        )),
        DispatchStreamEvent::Error(error) => Folded::Error(error.clone()),
        DispatchStreamEvent::Done => Folded::Message,
        // The dispatch-directive path is the older orchestration (the engine's
        // own agent dispatch); the chat panel's turns go through tools, so
        // these carry nothing to draw. They stay matched rather than ignored so
        // a future variant cannot be missed silently.
        DispatchStreamEvent::Dispatch(_)
        | DispatchStreamEvent::AgentOutput { .. }
        | DispatchStreamEvent::AgentComplete(_) => Folded::Message,
    }
}

/// The provider a turn runs on: the first one with a key, which is the rule the
/// old UI had. Choosing between several is a settings question (M7/M9).
/// Every configured provider that carries a key: the catalog this client has,
/// as `crate::api_provider` reads it. A provider without a key cannot run a
/// turn, so it is not offered as a choice.
fn usable_providers() -> Vec<ApiProviderConfig> {
    api_provider::load_all()
        .into_iter()
        .filter(|provider| !provider.api_key.trim().is_empty())
        .collect()
}

fn available_provider() -> Option<ApiProviderConfig> {
    usable_providers().into_iter().next()
}

/// One row of the model picker: a provider, named by what it is and what it
/// runs. Its value is the provider's name, which is how it is found again.
#[derive(Clone)]
struct ModelChoice {
    name: SharedString,
    title: SharedString,
}

impl ModelChoice {
    fn of(provider: &ApiProviderConfig) -> Self {
        let title = if provider.model.trim().is_empty() {
            provider.name.clone()
        } else {
            format!("{} · {}", provider.name, provider.model)
        };
        Self {
            name: provider.name.clone().into(),
            title: title.into(),
        }
    }
}

impl SearchableListItem for ModelChoice {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &SharedString {
        &self.name
    }
}

/// One row of the reasoning-effort picker. Its value is the level's wire
/// spelling, which is what `ThinkingLevel` stores.
#[derive(Clone)]
struct EffortChoice {
    value: SharedString,
    title: SharedString,
}

impl SearchableListItem for EffortChoice {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &SharedString {
        &self.value
    }
}

fn levels() -> Vec<EffortChoice> {
    ThinkingLevel::all()
        .iter()
        .map(|level| EffortChoice {
            value: level.as_str().into(),
            title: t(&format!("ai.effort.{}", level.as_str())).into(),
        })
        .collect()
}

fn level_index(level: ThinkingLevel) -> usize {
    ThinkingLevel::all()
        .iter()
        .position(|candidate| *candidate == level)
        .unwrap_or(0)
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
    match SessionManager::open() {
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

/// A message's who row, the prototype's: the sender's mark, its name, and the
/// minute it was written. The mark is the brand initial for the orchestrator
/// and the reader's own for a user turn, so a scroll-back reads at a glance.
fn who_row(message: &ChatMessage, theme: &Theme) -> AnyElement {
    let user = message.role == "user";
    let (mark, mark_bg, mark_fg) = if user {
        (t("ai.chat.self"), theme.list_active, theme.foreground)
    } else {
        ("z".to_owned(), theme.magenta.opacity(0.22), theme.magenta)
    };
    h_flex()
        .w_full()
        .gap_1p5()
        .items_center()
        .child(
            div()
                .size(px(16.))
                .rounded_sm()
                .flex()
                .items_center()
                .justify_center()
                .bg(mark_bg)
                .text_color(mark_fg)
                .text_size(px(9.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(mark),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(role_label(&message.role)),
        )
        .children(clock_of(&message.timestamp).map(|minute| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(minute)
        }))
        .into_any_element()
}

/// The minute a message was written, out of the store's timestamp — the
/// `HH:MM` between the `T` and the second mark. Anything else the field might
/// hold reads as no time at all rather than a wrong one.
fn clock_of(timestamp: &str) -> Option<String> {
    let bytes = timestamp.as_bytes();
    if bytes.len() >= 16 && bytes[10] == b'T' {
        Some(timestamp[11..16].to_owned())
    } else {
        None
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

/// Cut a tool's answer to what a card can show, one line.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    // `use super::*` re-exports GPUI's `test` attribute macro, which would
    // otherwise shadow the builtin one and recurse: name the builtin.
    use std::prelude::v1::test;

    fn a_user_message() -> ChatMessage {
        message("session", "user", "hello".into(), 1)
    }

    /// The empty assistant message a turn streams into.
    fn a_placeholder() -> ChatMessage {
        message("session", "orchestrator", String::new(), 2)
    }

    /// The who row names the minute out of the store's timestamp, and a
    /// timestamp that is not the store's shape names nothing rather than a
    /// wrong time.
    #[test]
    fn the_clock_is_the_minute_of_the_timestamp() {
        assert_eq!(clock_of("2026-10-23T12:04:31"), Some("12:04".to_owned()));
        assert_eq!(clock_of(""), None);
        assert_eq!(clock_of("not-a-timestamp"), None);
    }

    #[test]
    fn tokens_and_reasoning_accumulate_in_order() {
        let mut message = a_placeholder();
        assert!(matches!(
            fold_event(&mut message, &DispatchStreamEvent::Token("one ".into()), 0),
            Folded::Message
        ));
        fold_event(&mut message, &DispatchStreamEvent::Token("two".into()), 0);
        fold_event(
            &mut message,
            &DispatchStreamEvent::Thinking {
                round: 0,
                text: "why".into(),
            },
            0,
        );
        assert_eq!(message.content, "one two");
        assert_eq!(message.reasoning_text.as_deref(), Some("why"));
    }

    /// A call and its answer are one card: the answer lands on the call with
    /// the same id, with how long it took.
    #[test]
    fn a_tool_result_lands_on_the_call_it_answers() {
        let mut message = a_user_message();
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolCall {
                id: "call_1".into(),
                name: "list_workers".into(),
                args: json!({"detail": true}),
            },
            1_000,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolCall {
                id: "call_2".into(),
                name: "read_output".into(),
                args: json!({}),
            },
            1_100,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolResult {
                id: "call_2".into(),
                tool_name: "read_output".into(),
                content: "the pane said\nsomething".into(),
                is_error: false,
            },
            2_600,
        );
        assert_eq!(message.tool_calls.len(), 2);
        assert_eq!(message.tool_calls[0].status, ToolCallStatus::Running);
        let answered = &message.tool_calls[1];
        assert_eq!(answered.status, ToolCallStatus::Completed);
        assert_eq!(answered.completed_at, Some(2_600));
        assert_eq!(
            answered.result_summary.as_deref(),
            Some("the pane said something")
        );
    }

    #[test]
    fn a_failed_call_is_marked_failed_and_reported() {
        let mut message = a_user_message();
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolCall {
                id: "call_1".into(),
                name: "close_worker".into(),
                args: json!({}),
            },
            0,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolResult {
                id: "call_1".into(),
                tool_name: "close_worker".into(),
                content: "no such pane".into(),
                is_error: true,
            },
            5,
        );
        assert_eq!(message.tool_calls[0].status, ToolCallStatus::Failed);

        let folded = fold_event(
            &mut message,
            &DispatchStreamEvent::Error("HTTP 500".into()),
            5,
        );
        assert!(matches!(folded, Folded::Error(error) if error == "HTTP 500"));
    }

    #[test]
    fn a_compaction_is_reported_as_a_note_not_as_a_message() {
        let mut message = a_placeholder();
        let folded = fold_event(
            &mut message,
            &DispatchStreamEvent::Compacted(crate::ai::engine::CompactionInfo {
                original_count: 40,
                compressed_count: 12,
                kept_count: 28,
                before_tokens: 9_000,
                after_tokens: 2_000,
            }),
            0,
        );
        assert!(matches!(folded, Folded::Compacted(note) if note.contains("40")));
        assert!(
            message.content.is_empty(),
            "the note is not the model's text"
        );
    }

    /// The card shows the keys the model filled in, cut where a value is long,
    /// and nothing at all where the arguments are not an object.
    #[test]
    fn a_arguments_summary_names_the_keys_and_cuts_long_values() {
        let long = "x".repeat(80);
        assert_eq!(
            arguments_summary(&json!({"pane_id": "w1:p2", "lines": 20})),
            "pane_id: \"w1:p2\"  lines: 20"
        );
        assert!(arguments_summary(&json!({"instruction": long})).contains("..."));
        assert_eq!(arguments_summary(&json!(["not", "an", "object"])), "");
    }

    #[test]
    fn an_answers_summary_is_one_line() {
        assert_eq!(summarize("a\nb\nc", 160), "a b c");
        assert_eq!(summarize(&"y".repeat(200), 10), "yyyyyyyyyy...");
    }

    #[test]
    fn the_store_s_roles_are_labelled_for_a_reader() {
        assert_eq!(role_label("user"), t("ai.chat.role_you"));
        assert_eq!(role_label("orchestrator"), t("ai.chat.role_assistant"));
        assert_eq!(role_label("anything-else"), t("ai.chat.role_assistant"));
    }

    /// The API is told `assistant` where the store says `orchestrator`: a turn
    /// that replays its own history must not put the store's word on the wire.
    #[test]
    fn the_api_is_spoken_to_with_its_own_role_words() {
        assert_eq!(wire_role("user"), "user");
        assert_eq!(wire_role("orchestrator"), "assistant");
        assert_eq!(wire_role("anything-else"), "assistant");
    }

    /// The composer's model picker is the configured providers, named by what
    /// they are and what they run — the catalog `crate::ai::engine` picks from.
    #[test]
    fn the_model_picker_names_a_provider_by_its_model() {
        let provider = ApiProviderConfig {
            name: "local".into(),
            format: crate::api_provider::ApiFormat::OpenAI,
            base_url: "http://localhost:8000".into(),
            api_key: "key".into(),
            model: "qwen".into(),
            thinking_level: ThinkingLevel::High,
            url_suffix: "/v1/chat/completions".into(),
            enabled_agents: Vec::new(),
        };
        let choice = ModelChoice::of(&provider);
        assert_eq!(choice.title(), "local · qwen");
        assert_eq!(choice.value(), "local");
        // A provider with no model named is still offered, by its name alone.
        let choice = ModelChoice::of(&ApiProviderConfig {
            model: String::new(),
            ..provider
        });
        assert_eq!(choice.title(), "local");
    }

    /// The effort picker is `ThinkingLevel` itself, so a level's label and its
    /// wire spelling have to round-trip through the row's value.
    #[test]
    fn the_effort_picker_round_trips_every_level() {
        assert_eq!(levels().len(), ThinkingLevel::all().len());
        for (index, level) in ThinkingLevel::all().iter().enumerate() {
            assert_eq!(level_index(*level), index);
            assert_eq!(level_of(level.as_str()), Some(*level));
        }
        assert_eq!(level_of("not-a-level"), None);
    }
}
