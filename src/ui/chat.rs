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
//! actions (`MessageActions` tooltips). Since U5 the conversation is drawn as
//! blocks rather than bubbles: a user turn is a narrow strip on the right, the
//! model's words are the full column, and a tool call is one flat step row —
//! no avatar, no name, no role label. The library still draws the frame and the
//! text: `ChatContainer`, `MessageList`, `StreamingMarkdown`, `StreamingText`,
//! `SuggestionChips`, `ThinkingIndicator` and `step_mark`. The history rows'
//! rename field is the library's `Editing` driven from a pencil press, not its
//! click-to-edit `InlineEdit`: a row press must stay the way into a
//! conversation.
//!
//! The composer is the one place where the library's own composite was set
//! aside: `PromptInput` draws its frame and its send button, and the design
//! asks for a row of `context / + / permission / model · level / send` around
//! the field. So the card is composed here while the field, its `/`
//! suggestions and its IME stay the library's (`TextInput` + `Suggestions`);
//! the card's popups float over the conversation through the library's
//! `float`, anchored to the card's own measured bounds. `Ctrl-T` walks the
//! reasoning levels, and both the model and the level land in
//! `api-providers.json`, so the models page and the panel read one file.
//!
//! The row's own meanings (2026-11-02): the `+` is attachments and the few
//! other composer acts — a new chat and the history live in the panel's
//! header ([`super::aside`]), and the model is named by the chip at the row's
//! end, so nothing says the model twice. The permission chip is a menu with a
//! row per mode (the chosen one checked; full access wears the warning tone
//! on the chip itself), and the model chip's menu is two levels: the provider
//! is chosen in a flyout beside the panel, its models and the effort in the
//! panel itself. At the row's left end the context ring shows how much of the
//! model's window the next turn will re-send, from the last turn's bill, and
//! opens the numbers beside it.
//!
//! Attachments ride the composer: text files are inlined into the turn's own
//! words (so the store keeps what was sent), images ride as data-URL parts on
//! the last user message when the chosen model takes vision input, and
//! anything else is refused in place. They are cleared by the send that
//! carries them.
//!
//! Deliberately not here yet: `@` workspace mentions and session pinning. The
//! store has no pinned flag, and a pin the file forgets would be a control
//! that lies.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use ely_chat::{
    ChatContainer, MessageList, StepState, StreamingMarkdown, StreamingText, SuggestionChips,
    ThinkingIndicator, step_mark,
};
use ely_core::buttons::{Button, ButtonVariant, IconButton, SegmentedControl};
use ely_core::forms::{
    Choice, Editing, Enter, Input, InputEvent, OnValue, Pick, Suggestions, TextInput,
    active_trigger, float, replace_trigger,
};
use ely_core::motion::ProgressRing;
use ely_core::primitives::{Disclosure, Icon, IconName, Tooltip, raise};
use ely_core::theme::{ActiveTheme, ControlSize, IconSize, Radius, TextSize};
use ely_core::typography::{Ellipsis, KbdCombo, relative_text};
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, ElementId, Entity, FocusHandle, FontWeight,
    IntoElement, ParentElement, PathPromptOptions, Pixels, Render, SharedString, Styled,
    Subscription, Task, WeakEntity, Window, anchored, canvas, div, point, prelude::*, px, relative,
    rems,
};

use crate::ai::compactor::Compactor;
use crate::ai::endpoint::ApiEndpoint;
use crate::ai::engine::{
    ChatMessageRef, CompactionInfo, DispatchStreamEvent, OrchestratorEngine, ToolApproval,
    build_system_prompt, detect_language,
};
use crate::ai::roles::RoleRegistry;
use crate::ai::session::{
    ChatMessage, Entry, Session, SessionManager, ToolCallState, ToolCallStatus, UsageInfo,
};
use crate::ai::tools::{Tool, ToolCall, ToolExecutor, builtin_tools};
use crate::api_provider::{self, ApiProviderConfig, ThinkingLevel};
use crate::config;
use crate::i18n::t;
use crate::ui::ime;

/// How many tool rounds one turn may take. The engine has its own ceiling; this
/// is the tighter one the panel asks for.
const MAX_TOOL_ROUNDS: u32 = 8;

/// How many characters of a text attachment ride the turn. A code file past
/// this is cut, and the marker says so — a silent whole-file paste would be
/// the model's whole window for one file.
const ATTACH_TEXT_MAX: usize = 32_000;

/// How many bytes of an image are accepted. The vision providers reject
/// anything like a screenshot at full resolution twice over; past this the
/// attach is refused rather than sent to fail.
const ATTACH_IMAGE_MAX: usize = 8 * 1024 * 1024;

/// One file waiting to ride the next turn. The composer's `+` puts them here;
/// the send that carries them takes them off.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Attachment {
    path: std::path::PathBuf,
    kind: AttachmentKind,
}

/// What a picked file became once it was read.
#[derive(Clone, Debug, PartialEq)]
enum AttachmentKind {
    /// An image the API takes as a data-URL part: the bytes, base64, and the
    /// media type the extension named.
    Image { media_type: String, data: String },
    /// A text file inlined into the turn's own words; `complete` is whether
    /// the whole file made it under the cut.
    Text { excerpt: String, complete: bool },
    /// The read said no: not readable text, not an image this client knows,
    /// or too large. The chip says the reason instead of a send that fails
    /// two steps later.
    Failed { reason: String },
}

impl Attachment {
    fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    fn is_image(&self) -> bool {
        matches!(self.kind, AttachmentKind::Image { .. })
    }

    /// The image as the engine carries it, ready for the request's parts.
    fn as_part(&self) -> Option<crate::ai::engine::ImagePart> {
        match &self.kind {
            AttachmentKind::Image { media_type, data } => Some(crate::ai::engine::ImagePart {
                media_type: media_type.clone(),
                data: data.clone(),
            }),
            _ => None,
        }
    }

    /// The words the turn carries for this attachment when the image itself
    /// goes as a part: a marker the transcript and the store can show.
    fn marker(&self) -> Option<String> {
        match &self.kind {
            AttachmentKind::Image { .. } => {
                Some(rust_i18n::t!("ai.chat.attach_image", name = self.name()).to_string())
            }
            _ => None,
        }
    }

    /// The words a text attachment adds to the turn: a fenced block that
    /// names its file. This *is* what the model and the store keep.
    fn inlined(&self) -> Option<String> {
        match &self.kind {
            AttachmentKind::Text { excerpt, complete } => Some(format!(
                "[{}{}]\n```\n{}\n```",
                rust_i18n::t!("ai.chat.attach_text", name = self.name()),
                if *complete {
                    String::new()
                } else {
                    t("ai.chat.attach_cut")
                },
                excerpt
            )),
            _ => None,
        }
    }
}

/// The media types the composer accepts as images, by extension. Anything
/// else readable is inlined as text; anything unreadable is refused in place.
fn image_media_type(path: &std::path::Path) -> Option<&'static str> {
    match path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        _ => None,
    }
}

/// Read the picked files off the UI thread. One answer per file, always: a
/// failure is a [`AttachmentKind::Failed`] the chip can name, not a missing
/// chip.
fn read_attachments(paths: Vec<std::path::PathBuf>) -> Vec<Attachment> {
    use base64::Engine as _;
    paths
        .into_iter()
        .map(|path| {
            let kind = (|| {
                if let Some(media_type) = image_media_type(&path) {
                    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
                    if bytes.len() > ATTACH_IMAGE_MAX {
                        return Err(rust_i18n::t!("ai.chat.attach_too_large").to_string());
                    }
                    return Ok(AttachmentKind::Image {
                        media_type: media_type.to_owned(),
                        data: base64::engine::general_purpose::STANDARD.encode(bytes),
                    });
                }
                let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
                let text = String::from_utf8(bytes)
                    .map_err(|_| rust_i18n::t!("ai.chat.attach_binary").to_string())?;
                let complete = text.len() <= ATTACH_TEXT_MAX;
                let excerpt = if complete {
                    text
                } else {
                    text.chars().take(ATTACH_TEXT_MAX).collect()
                };
                Ok(AttachmentKind::Text { excerpt, complete })
            })();
            let kind = kind.unwrap_or_else(|reason| AttachmentKind::Failed { reason });
            Attachment { path, kind }
        })
        .collect()
}

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
    /// The user message being edited: its words sit in the composer, and the
    /// next send replaces it and everything below it.
    editing: Option<usize>,
    /// The running turn's engine task, so Stop can end it.
    turn: Option<tokio::task::JoinHandle<()>>,
    /// The task draining the turn's events.
    drain: Option<Task<()>>,
    /// Tool names the user let run "always" in this sitting. The gate reads it
    /// before it asks again; deliberately not persisted, because a permission
    /// granted in one conversation should not answer for another.
    always_allow: Arc<Mutex<HashSet<String>>>,
    /// Whether this window's daemon has a JSON lane for the chat's tools. A
    /// remote window's model conversation runs, but no daemon tool is offered
    /// or executed — its agents live on the other machine.
    daemon_tools: bool,
    /// The composer menu standing open, if one is.
    menu: Option<ComposerMenu>,
    /// The model menu's second level: the provider flyout, while it is open.
    provider_menu: bool,
    /// Why the last model or level choice could not be written to the provider
    /// file, until the menu is opened again.
    menu_error: Option<String>,
    /// This conversation's permission mode: while set, the gate runs a tool
    /// call without parking. Deliberately not persisted and off by default,
    /// because a permission granted in one conversation should not answer for
    /// the next: starting or loading a session puts it back to asking.
    full_access: Arc<AtomicBool>,
    /// Files waiting to ride the next send, in pick order. The send that
    /// carries them takes them; a new chat drops them unread.
    attachments: Vec<Attachment>,
    /// The window's shell, so a menu row can open the models settings page on
    /// it. Set once by the shell right after this entity is built; a menu row
    /// pressed before that (impossible in practice) does nothing.
    pub(crate) host: Option<WeakEntity<super::shell::Root>>,
    /// Answers that arrived while the aside was closed. The rail's badge says
    /// how many; opening the panel clears it.
    unread: usize,
    /// Whether the panel is on screen. Not the same as "the turn runs": a
    /// closed panel keeps streaming, and what arrived meanwhile is unread.
    on_screen: bool,
    _input: Subscription,
}

impl ChatView {
    pub(crate) fn new(window: &mut Window, daemon_tools: bool, cx: &mut Context<Self>) -> Self {
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
            editing: None,
            turn: None,
            drain: None,
            always_allow: Arc::new(Mutex::new(HashSet::new())),
            menu: None,
            menu_error: None,
            provider_menu: false,
            attachments: Vec::new(),
            host: None,
            daemon_tools,
            full_access: Arc::new(AtomicBool::new(false)),
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
    /// tokio. Nothing here waits for the model. The composer's attachments
    /// ride along: text files are inlined into the turn's words, images go as
    /// request parts — and refuse the send on a model that takes no images.
    fn send(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_streaming() {
            return;
        }
        let text = self.input.read(cx).text().trim().to_owned();
        if text.is_empty() && self.attachments.is_empty() {
            return;
        }
        let Some((provider, engine)) = self.prepare_turn(&text, cx) else {
            // The text stays in the box: nothing was sent.
            return;
        };
        let has_images = self.attachments.iter().any(Attachment::is_image);
        let vision = selected_model(&provider).is_some_and(|model| model.vision);
        if has_images && !vision {
            self.error = Some(t("ai.chat.attach_needs_vision"));
            cx.notify();
            return;
        }
        // The turn's words are the field's text with the text attachments
        // folded in below it, so the store keeps what the model was actually
        // given; images leave a marker and ride as parts instead.
        let mut words = text;
        for attachment in &self.attachments {
            match attachment.inlined() {
                Some(block) => {
                    if !words.is_empty() {
                        words.push_str("\n\n");
                    }
                    words.push_str(&block);
                }
                None => {
                    if let Some(marker) = attachment.marker() {
                        if !words.is_empty() {
                            words.push_str("\n\n");
                        }
                        words.push_str(&marker);
                    }
                }
            }
        }
        self.input.update(cx, |input, cx| input.set_text("", cx));
        let attachments = std::mem::take(&mut self.attachments);
        self.stand_down();
        // A sent message is the conversation again: the history stands down,
        // and so does any menu the composer had open.
        self.history = None;
        self.menu = None;
        self.provider_menu = false;
        self.menu_error = None;

        let session_id = self.ensure_session(&words, &provider);
        // An edit replaces the message it started from and every answer after
        // it, in the store as well as on screen: a reload that brought the old
        // turn back would be a lie.
        if let Some(index) = self.editing.take() {
            if let Some(replaced) = self.messages.get(index) {
                truncate_store(&replaced.session_id, replaced.sequence);
            }
            cut_before(&mut self.messages, index);
        }
        let sequence = self.messages.last().map_or(1, |last| last.sequence + 1);
        let user = message(&session_id, "user", words.clone(), sequence);
        persist(&user);
        self.messages.push(user);
        // The parts ride the request, not the store: the pixels belong to this
        // one call, the marker above to the transcript.
        let images = attachments
            .iter()
            .filter_map(Attachment::as_part)
            .collect::<Vec<_>>();
        self.run_turn(&session_id, &provider, engine, images, cx);
    }

    /// Load one of the user's own messages back into the composer and enter
    /// the edit state. Sending then replaces that message and its answer. A
    /// turn in progress is not editable: the message it is answering would be
    /// cut while it streams.
    fn start_edit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_streaming() {
            return;
        }
        let Some(text) = edit_target(&self.messages, index) else {
            return;
        };
        self.editing = Some(index);
        self.input.update(cx, |input, cx| input.set_text(text, cx));
        let focus = self.input.read(cx).focus().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Leave the edit state and empty the box the edited words came into.
    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            self.input.update(cx, |input, cx| input.set_text("", cx));
            cx.notify();
        }
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
        self.editing = None;
        self.messages.truncate(user_index + 1);
        self.stand_down();
        let session_id = self.session_id.clone().unwrap_or_default();
        self.run_turn(&session_id, &provider, engine, Vec::new(), cx);
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
    /// an empty assistant message waits below it for the tokens. `images` are
    /// the attachments' parts, carried on the request's last user message.
    fn run_turn(
        &mut self,
        session_id: &str,
        provider: &ApiProviderConfig,
        engine: OrchestratorEngine,
        images: Vec<crate::ai::engine::ImagePart>,
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

        let mut history: Vec<ChatMessageRef> = self.messages[..index]
            .iter()
            .filter(|message| !message.content.is_empty())
            .map(|message| ChatMessageRef {
                role: wire_role(&message.role).to_owned(),
                content: message.content.clone(),
                ..Default::default()
            })
            .collect();
        // The images belong to the turn just sent: they ride its own user
        // message, the last one in the history just built.
        if !images.is_empty()
            && let Some(last_user) = history.iter_mut().rev().find(|entry| entry.role == "user")
        {
            last_user.images = Some(images);
        }

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
        let full_access = self.full_access.clone();
        let daemon_tools = self.daemon_tools;
        self.turn = Some(runtime.spawn(async move {
            let tools = if daemon_tools {
                builtin_tools()
            } else {
                Vec::new()
            };
            let tools: Vec<&dyn Tool> = tools.iter().map(|tool| tool.as_ref()).collect();
            // The empty workspace id is the daemon's own focused workspace: the
            // panel has no snapshot of its own to name one from.
            let executor = if daemon_tools {
                ToolExecutor::new(ApiEndpoint::local(), String::new())
            } else {
                ToolExecutor::without_daemon()
            };
            let compactor = Compactor::default_compactor();
            let gate = ApprovalGate {
                requests: approval_tx,
                always_allow,
                full_access,
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
        // A reasoning run is the stretch between its first thinking token and
        // the next thing the model does. When that next thing arrives, the run
        // closes with its measured duration.
        if matches!(event, DispatchStreamEvent::Thinking { .. }) {
            if self.reasoning_started.is_none() {
                self.reasoning_started = Some(now);
            }
        } else if let Some(start) = self.reasoning_started.take()
            && let Some(index) = self.streaming
            && let Some(message) = self.messages.get_mut(index)
        {
            close_reasoning(message, now.saturating_sub(start));
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
            if let Some(took) = thought_for {
                close_reasoning(message, took);
            }
            tracing::info!(
                target: "zenix-chat",
                chars = message.content.chars().count(),
                entries = message.entries.len(),
                "turn finished"
            );
            // An answer with nothing in it — a turn that failed before its
            // first token — is not a message anyone will want to read back:
            // the retry drops it rather than saving it.
            let said_something = !message.content.is_empty()
                || !message.entries.is_empty()
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
    pub(crate) fn new_session(&mut self, cx: &mut Context<Self>) {
        if self.is_streaming() {
            self.stop(cx);
        }
        self.pending_approval = None;
        self.editing = None;
        self.menu = None;
        self.provider_menu = false;
        self.attachments.clear();
        self.full_access.store(false, Ordering::Relaxed);
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
    pub(crate) fn toggle_history(&mut self, cx: &mut Context<Self>) {
        if self.history.take().is_some() {
            cx.notify();
            return;
        }
        self.history = Some(History {
            sessions: read_sessions(),
            error: None,
            confirm: None,
            renaming: None,
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
        self.full_access.store(false, Ordering::Relaxed);
        self.messages = messages;
        self.streaming = None;
        self.session_id = Some(id.to_owned());
        self.editing = None;
        self.error = None;
        self.compacted = None;
        self.reasoning_started = None;
        self.attachments.clear();
        self.provider_menu = false;
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

    /// Start editing a row's title: the library's `Editing` opens a field over
    /// the current one, all of it selected and focused. A row press stays the
    /// way into the conversation — renaming is the pencil's job, not the
    /// title's, so a single click never lands the reader in an editor.
    fn begin_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(history) = self.history.as_mut() else {
            return;
        };
        let Some(title) = history
            .sessions
            .iter()
            .find(|session| session.id == id)
            .map(|session| session.title.clone())
        else {
            return;
        };
        let editing = cx.new(|_| Editing::default());
        let weak = cx.entity().downgrade();
        let session_id = id.to_owned();
        editing.update(cx, |owner, _| {
            owner.on_commit = Some(Rc::new(move |text: &SharedString, _, cx| {
                if let Some(panel) = weak.upgrade() {
                    let session_id = session_id.clone();
                    let title = text.to_string();
                    panel.update(cx, |panel, cx| panel.rename_session(&session_id, title, cx));
                }
            }));
        });
        Editing::begin(&editing, title, window, cx);
        history.renaming = Some((id.to_owned(), editing));
        cx.notify();
    }

    /// The edit's Escape: the field closes and the title stays as it was.
    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, editing)) = self
            .history
            .as_mut()
            .and_then(|history| history.renaming.take())
        else {
            return;
        };
        editing.update(cx, |editing, cx| {
            editing.finish(true, window, cx);
            editing.give_back(window, cx);
        });
        cx.notify();
    }

    /// The store's title for a row, kept through the library's `Editing`.
    fn rename_session(&mut self, id: &str, title: String, cx: &mut Context<Self>) {
        if let Some(history) = self.history.as_mut() {
            history.renaming = None;
        }
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
    /// The model the next turn runs on. Written back to the provider file, so
    /// the models page and the panel read the same selection.
    fn pick_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(provider) = self.provider.as_mut() else {
            return;
        };
        if !provider.select_model(id) {
            return;
        }
        self.persist_choice(cx);
        cx.notify();
    }

    /// The reasoning effort the next turn asks for, written back to the file
    /// the settings page reads (it is the same `thinking_level` there).
    fn choose_level(&mut self, token: &str, cx: &mut Context<Self>) {
        let Some(level) = level_of(token) else {
            return;
        };
        let Some(provider) = self.provider.as_mut() else {
            return;
        };
        if provider.thinking_level == level {
            return;
        }
        provider.thinking_level = level;
        self.persist_choice(cx);
        cx.notify();
    }

    /// `Ctrl-T`: the next level, wrapping, so the one key walks all four.
    fn cycle_level(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self
            .provider
            .as_ref()
            .map(|provider| provider.thinking_level)
        else {
            return;
        };
        let next = next_level(current);
        self.choose_level(next.as_str(), cx);
    }

    /// Write the composer's model and level into the provider file. A failure
    /// is kept for the menu to show: the panel and the models page read one
    /// file, and a write that did not land would make them disagree.
    fn persist_choice(&mut self, cx: &mut Context<Self>) {
        let Some(chosen) = self.provider.as_ref() else {
            return;
        };
        let (name, model, level) = (
            chosen.name.clone(),
            chosen.model.clone(),
            chosen.thinking_level,
        );
        let mut all = api_provider::load_all();
        let Some(entry) = all.iter_mut().find(|provider| provider.name == name) else {
            self.menu_error = Some(t("ai.chat.provider_gone"));
            cx.notify();
            return;
        };
        entry.model = model;
        entry.thinking_level = level;
        // The list follows the selection: a hand-edited id the file's list
        // lacks is added rather than lost on the next read.
        entry.sync_models();
        match api_provider::save_all(&all) {
            Ok(()) => self.menu_error = None,
            Err(error) => {
                self.menu_error = Some(
                    rust_i18n::t!("ai.chat.save_failed", error = error.to_string()).to_string(),
                );
            }
        }
    }

    /// The gate's mode right now: the flag is shared with the running turn, so
    /// switching it while a turn waits takes effect on the next call.
    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::of(self.full_access.load(Ordering::Relaxed))
    }

    /// A row of the permission menu was pressed: this sitting's gate takes the
    /// mode from the next tool call on.
    fn set_permission_mode(&mut self, mode: PermissionMode, cx: &mut Context<Self>) {
        self.full_access
            .store(mode == PermissionMode::Full, Ordering::Relaxed);
        self.menu = None;
        cx.notify();
    }

    /// The composer's menus. The provider flyout is the model menu's second
    /// level: opening another menu puts it away with the panel it hangs from.
    fn toggle_menu(&mut self, menu: ComposerMenu, cx: &mut Context<Self>) {
        self.menu = if self.menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
        if self.menu != Some(ComposerMenu::Model) {
            self.provider_menu = false;
        }
        self.menu_error = None;
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() || self.provider_menu {
            self.provider_menu = false;
            cx.notify();
        }
    }

    /// The model panel's provider row was pressed: open or close the flyout
    /// that lists the providers.
    fn toggle_provider_menu(&mut self, cx: &mut Context<Self>) {
        self.provider_menu = !self.provider_menu;
        cx.notify();
    }

    /// The flyout's provider row: the next turn runs on that provider, its
    /// models fill the panel, and the flyout goes away.
    fn choose_provider(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(provider) = self
            .providers
            .iter()
            .find(|candidate| candidate.name == name)
            .cloned()
        {
            self.provider = Some(provider);
            self.provider_menu = false;
            cx.notify();
        }
    }

    /// The flyout's manage row: the models page, where a provider's catalog is
    /// kept. The shell owns that dialog; the panel only asks for it.
    fn open_models_settings(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        self.provider_menu = false;
        let host = self.host.clone();
        cx.notify();
        if let Some(host) = host.and_then(|host| host.upgrade()) {
            host.update(cx, |root, cx| {
                root.open_settings(cx);
                root.set_settings_page(super::settings::Page::Ai, cx);
            });
        }
    }

    /// Ask for files and read them off the UI thread; each answer becomes a
    /// chip above the field, whatever it turned out to be.
    pub(crate) fn attach_files(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |panel, cx| {
            // The dialog answers through a oneshot: a cancelled dialog is a
            // `None`, not an error, and both mean "nothing picked".
            let picked = picked.await.ok().and_then(|answer| answer.ok()).flatten();
            let Some(paths) = picked else {
                return;
            };
            if paths.is_empty() {
                return;
            }
            let read = cx
                .background_executor()
                .spawn(async move { read_attachments(paths) })
                .await;
            panel
                .update(cx, |panel, cx| {
                    panel.attachments.extend(read);
                    panel.menu = None;
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// A chip's ×: the file will not ride the next send.
    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            cx.notify();
        }
    }

    /// One of the panel's own `/` commands. Both the field's menu and the `+`
    /// button's rows land here, so the two cannot drift.
    fn run_chat_command(&mut self, value: &str, cx: &mut Context<Self>) {
        match value {
            "new" => self.new_session(cx),
            "history" => self.toggle_history(cx),
            "clear" => {
                self.input.update(cx, |input, cx| input.set_text("", cx));
                cx.notify();
            }
            _ => {}
        }
    }

    /// Put one of the welcome screen's starters into the box, leaving it
    /// focused so the user can adjust the words before sending.
    fn use_starter(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((text, _)) = starter_prompts().into_iter().nth(index) else {
            return;
        };
        self.editing = None;
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
            let rows = transcript_rows(
                message,
                awaiting.as_deref(),
                streaming,
                self.reasoning_started,
                now_ms(),
            );
            return Some(RowModel::Message(MessageModel {
                user,
                streaming,
                content: message.content.clone(),
                rows,
                usage: message.usage,
                at: timestamp_of(&message.timestamp),
                can_retry: !user && !streaming && index + 1 == self.messages.len(),
                can_edit: user && !message.content.is_empty() && self.streaming.is_none(),
                editing: user && self.editing == Some(index),
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
    /// The turn's timeline in arrival order, ready to draw.
    rows: Vec<TranscriptRow>,
    usage: Option<UsageInfo>,
    at: Option<jiff::Timestamp>,
    can_retry: bool,
    can_edit: bool,
    editing: bool,
}

/// One row of a turn's timeline: a thought, a run of consecutive steps, or a
/// run of text. The rows are built in arrival order, so the reader sees the
/// turn as it happened rather than three sorted piles.
enum TranscriptRow {
    Reasoning {
        text: String,
        took_ms: Option<u64>,
        /// This is the run still being written.
        live: bool,
    },
    /// Consecutive steps, before `step_runs` decides which of them fold.
    Steps(Vec<ToolCallState>),
    Text {
        text: String,
        live: bool,
    },
}

/// Build the timeline to draw. The call the approval gate is asking about is
/// taken out of its run — the gate's card is that call — and the rest keep
/// their order. A text run breaks a step run: a group never spans a paragraph.
fn transcript_rows(
    message: &ChatMessage,
    awaiting: Option<&str>,
    streaming: bool,
    reasoning_started: Option<u64>,
    now_ms: u64,
) -> Vec<TranscriptRow> {
    let entries = &message.entries;
    let mut rows = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let live = streaming && index + 1 == entries.len();
        match &entries[index] {
            Entry::Reasoning { text, duration_ms } => {
                let took_ms = duration_ms.or_else(|| {
                    live.then(|| reasoning_started.map(|start| now_ms.saturating_sub(start)))
                        .flatten()
                });
                rows.push(TranscriptRow::Reasoning {
                    text: text.clone(),
                    took_ms,
                    live,
                });
                index += 1;
            }
            Entry::Text { text } => {
                rows.push(TranscriptRow::Text {
                    text: text.clone(),
                    live,
                });
                index += 1;
            }
            Entry::Step { .. } => {
                let mut calls = Vec::new();
                while index < entries.len() {
                    let Entry::Step { call } = &entries[index] else {
                        break;
                    };
                    if awaiting != Some(call.id.as_str()) {
                        calls.push(call.clone());
                    }
                    index += 1;
                }
                if !calls.is_empty() {
                    rows.push(TranscriptRow::Steps(calls));
                }
            }
        }
    }
    rows
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
    /// Whether this sitting runs tools without asking at all. The panel's
    /// permission chip owns the flag; off unless the reader turned it on.
    full_access: Arc<AtomicBool>,
}

impl ToolApproval for ApprovalGate {
    fn approve(&self, call: &ToolCall) -> bool {
        // Full access answers before the call is ever parked: the turn does
        // not wait, and nothing is silently granted later.
        if self.full_access.load(Ordering::Relaxed) {
            return true;
        }
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
    /// The row whose title is being edited, with the library's `Editing`
    /// running its field. A press on the pencil opens it; Enter or a blur
    /// keeps, Escape reverts.
    renaming: Option<(String, Entity<Editing>)>,
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
        ChatContainer::new(body).composer(self.composer(window, cx))
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
    /// session; its pencil renames in place, its bin asks once before it
    /// deletes.
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

    /// The composer: one card in the shape the design draws — the field, then
    /// a row of `+ / permission / model · level / send`. The library's
    /// `PromptInput` draws its own frame and send button, so the card is
    /// composed here; the library still draws the field, its `/` menu and its
    /// IME. Enter sends, Shift-Enter breaks the line, and `Ctrl-T` walks the
    /// reasoning levels.
    fn composer(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (radius, base, focus_ring, border_strong, surface, fg, fg_subtle, fg_muted, hover) = {
            let theme = cx.theme();
            (
                theme.radius(Radius::Xl),
                theme.text_size(TextSize::Base),
                theme.colors.focus,
                theme.colors.border_strong,
                theme.colors.surface,
                theme.colors.fg,
                theme.colors.fg_subtle,
                theme.colors.fg_muted,
                theme.colors.hover,
            )
        };
        let (chip_radius, chip_text, chip_height) = {
            let theme = cx.theme();
            (
                theme.radius(Radius::Md),
                theme.text_size(TextSize::Sm),
                theme.control_height(ControlSize::Sm),
            )
        };
        let warning = cx.theme().colors.warning;
        let weak = cx.entity().downgrade();
        let ime_input = self.input.clone();
        let ready = !self.input.read(cx).text().trim().is_empty() || !self.attachments.is_empty();
        let streaming = self.is_streaming();
        let focused = self.input.read(cx).focus().contains_focused(window, cx);

        let send = {
            let weak = weak.clone();
            move |window: &mut Window, cx: &mut App| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |panel, cx| panel.send(window, cx));
                }
            }
        };
        let field = div()
            .capture_action({
                let send = send.clone();
                move |_: &Enter, window: &mut Window, cx: &mut App| {
                    cx.stop_propagation();
                    if ready && !streaming {
                        send(window, cx);
                    }
                }
            })
            .text_size(base)
            .child(self.input.clone());
        let on_command: OnValue = {
            let weak = weak.clone();
            Rc::new(
                move |value: &SharedString, _window: &mut Window, cx: &mut App| {
                    if let Some(panel) = weak.upgrade() {
                        let value = value.to_string();
                        panel.update(cx, |panel, cx| panel.run_chat_command(&value, cx));
                    }
                },
            )
        };
        let field = command_offers(
            "chat-composer-offers".into(),
            &self.input,
            &starter_commands(),
            on_command,
            cx,
        )
        .wrap(field, window, cx);

        // The card's own bounds live in keyed state, written by the paint that
        // lays it out (the same shape the library's popovers use), so the menu
        // can float over the conversation instead of pushing it up. The width
        // also decides the compact row: below 260px the permission chip and the
        // model chip drop to their icon, so nothing overlaps when the aside is
        // dragged narrow.
        let anchor =
            window.use_keyed_state("chat-composer-anchor", cx, |_, _| None::<Bounds<Pixels>>);
        let compact = anchor
            .read(cx)
            .as_ref()
            .is_some_and(|bounds| bounds.size.width < px(260.));
        // Each menu's trigger measures itself, so a press on it closes the
        // menu through the trigger's own click rather than as an outside press.
        let plus_anchor =
            window.use_keyed_state("chat-plus-anchor", cx, |_, _| None::<Bounds<Pixels>>);
        let model_anchor =
            window.use_keyed_state("chat-model-anchor", cx, |_, _| None::<Bounds<Pixels>>);
        let permission_anchor =
            window.use_keyed_state("chat-permission-anchor", cx, |_, _| None::<Bounds<Pixels>>);
        let context_anchor =
            window.use_keyed_state("chat-context-anchor", cx, |_, _| None::<Bounds<Pixels>>);
        // The model panel's provider row, and the flyout it opens: the row's
        // bounds place the flyout beside the panel, both bounds together tell
        // an outside press on either from one that puts the menu away.
        let provider_anchor =
            window.use_keyed_state("chat-provider-anchor", cx, |_, _| None::<Bounds<Pixels>>);
        let provider_panel_anchor =
            window.use_keyed_state("chat-provider-panel-anchor", cx, |_, _| {
                None::<Bounds<Pixels>>
            });

        // The `+`: attachments and the composer's other small acts. A new chat
        // and the history live in the panel's header.
        let plus = IconButton::new("chat-plus", IconName::Paperclip)
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .tooltip(t("ai.chat.attach"))
            .on_click({
                let weak = weak.clone();
                move |_, _, cx| {
                    if let Some(panel) = weak.upgrade() {
                        panel.update(cx, |panel, cx| {
                            panel.toggle_menu(ComposerMenu::Commands, cx)
                        });
                    }
                }
            });

        // The permission chip opens the mode menu; a mode row chooses. While
        // full access is on the chip itself wears the warning tone — that is
        // the state a reader must not miss.
        let mode = self.permission_mode();
        let permission = {
            let open = self.menu == Some(ComposerMenu::Permission);
            let (ink, ground) = if mode == PermissionMode::Full {
                (warning, warning.opacity(0.14))
            } else {
                (fg_muted, hover)
            };
            let weak = weak.clone();
            let mut chip = div()
                .id("chat-permission")
                .role(gpui::Role::Button)
                .aria_label(mode.label())
                .tooltip(Tooltip::text(mode.hint()))
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .h(chip_height)
                .rounded(chip_radius)
                .text_size(chip_text)
                .text_color(ink)
                .when(mode == PermissionMode::Full || open, |chip| chip.bg(ground))
                .child(Icon::new(mode.icon()).size(IconSize::Xs).color(ink));
            if !compact {
                chip = chip.child(div().flex_none().child(mode.label()));
            }
            chip = chip.child(
                Icon::new(IconName::ChevronDown)
                    .size(IconSize::Xs)
                    .color(ink),
            );
            chip.cursor_pointer()
                .hover(move |style| style.bg(ground))
                .on_click(move |_, _, cx| {
                    if let Some(panel) = weak.upgrade() {
                        panel.update(cx, |panel, cx| {
                            panel.toggle_menu(ComposerMenu::Permission, cx)
                        });
                    }
                })
        };

        // The context ring: how much of the model's window the next turn will
        // re-send, from the last turn's bill. It stands only when a bill is
        // known; without a window to be full of it is a quiet outline.
        let context = self.context_usage().map(|usage| {
            let open = self.menu == Some(ComposerMenu::Context);
            let weak = weak.clone();
            let title = context_title(&usage, cx);
            let mut button = div()
                .id("chat-context")
                .role(gpui::Role::Button)
                .aria_label(t("ai.chat.context"))
                .tooltip(Tooltip::text(title))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(24.))
                .rounded_full()
                .when(open, |button| button.bg(hover))
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .on_click(move |_, _, cx| {
                    if let Some(panel) = weak.upgrade() {
                        panel.update(cx, |panel, cx| panel.toggle_menu(ComposerMenu::Context, cx));
                    }
                });
            button = match usage.fraction() {
                Some(fraction) => {
                    button.child(ProgressRing::new("chat-context-ring", fraction).size(rems(0.9)))
                }
                None => button.child(
                    div()
                        .size(px(11.))
                        .rounded_full()
                        .border_1()
                        .border_color(fg_subtle),
                ),
            };
            button
        });

        // The model chip is drawn here rather than as a `Button`: its label is
        // two pieces, and only the model's name may shorten — the effort at its
        // end is what `Ctrl-T` moves, so it stays whole however narrow the
        // aside is.
        let model = {
            let enabled = self.provider.is_some();
            let open = self.menu == Some(ComposerMenu::Model);
            let name = self
                .provider
                .as_ref()
                .map(model_name)
                .unwrap_or_else(|| t("ai.config_empty"));
            let level = self.provider.as_ref().map(level_label);
            let weak = weak.clone();
            let mut chip = div()
                .id("chat-model")
                .role(gpui::Role::Button)
                .aria_label(t("ai.chat.menu_models"))
                .flex()
                .items_center()
                .gap_1()
                .min_w_0()
                .px_2()
                .h(chip_height)
                .rounded(chip_radius)
                .text_size(chip_text)
                .text_color(fg)
                .when(open, |chip| chip.bg(hover))
                // Narrow, the name is only in the tooltip; wide, it is on the
                // chip itself, so a tooltip would just repeat it.
                .when(compact, |chip| chip.tooltip(Tooltip::text(name.clone())))
                .child(
                    Icon::new(IconName::Sparkles)
                        .size(IconSize::Xs)
                        .color(fg_subtle),
                );
            if !compact {
                chip = chip.child(
                    div()
                        .min_w_0()
                        .child(Ellipsis::new(SharedString::from(name))),
                );
            }
            if let Some(level) = level {
                // Narrow, the level stands alone (the name is in the tooltip),
                // so it takes the ink the name would have had.
                let (text, ink) = if compact {
                    (level, fg)
                } else {
                    (format!("· {level}"), fg_muted)
                };
                chip = chip.child(div().flex_none().text_color(ink).child(text));
            }
            chip = chip.child(
                Icon::new(IconName::ChevronDown)
                    .size(IconSize::Xs)
                    .color(fg_subtle),
            );
            if enabled {
                chip.cursor_pointer()
                    .hover(|style| style.bg(hover))
                    .on_click(move |_, _, cx| {
                        if let Some(panel) = weak.upgrade() {
                            panel
                                .update(cx, |panel, cx| panel.toggle_menu(ComposerMenu::Model, cx));
                        }
                    })
            } else {
                chip.opacity(0.45)
            }
        };

        let measure = anchor.clone();
        let card = div()
            .id("chat-composer")
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(radius)
            .border_1()
            .border_color(if focused { focus_ring } else { border_strong })
            .bg(surface)
            .on_key_down({
                let weak = weak.clone();
                move |event: &gpui::KeyDownEvent, _window: &mut Window, cx: &mut App| {
                    let Some(panel) = weak.upgrade() else {
                        return;
                    };
                    if event.keystroke.key == "escape" && panel.read(cx).menu.is_some() {
                        cx.stop_propagation();
                        panel.update(cx, |panel, cx| panel.close_menu(cx));
                        return;
                    }
                    if event.keystroke.modifiers.control && event.keystroke.key == "t" {
                        cx.stop_propagation();
                        panel.update(cx, |panel, cx| panel.cycle_level(cx));
                    }
                }
            })
            .children(self.edit_line(cx))
            .children(attachment_chips(&self.attachments, &weak, cx))
            .child(field)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(measured(&plus_anchor, plus).flex_none())
                    .child(measured(&permission_anchor, permission).flex_none())
                    .child(div().flex_1())
                    .children(context)
                    .child(measured(&model_anchor, model))
                    .child(self.send_circle(&weak, ready, streaming, cx)),
            )
            .child(
                canvas(
                    move |bounds, window, cx| {
                        // The caret is the candidate window's anchor while the
                        // composer holds the focus (see ui::ime).
                        let state = ime_input.read(cx);
                        if state.focus().is_focused(window) {
                            ime::push_spot(state.caret_bounds(), window, cx);
                        }
                        if measure.read(cx).as_ref() != Some(&bounds) {
                            measure.update(cx, |slot, cx| {
                                *slot = Some(bounds);
                                cx.notify();
                                window.request_animation_frame();
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );

        let mut root = div().relative().w_full().child(card);
        if let Some(open) = self.menu {
            let bounds = anchor.read(cx).as_ref().copied();
            if let Some(bounds) = bounds {
                // The panel never grows wider than the card it hangs from,
                // so a narrow aside cannot push it past the window's edge.
                let width = bounds.size.width.min(px(320.));
                // Nor taller than the side it opens on: a window shorter than
                // the menu would otherwise pin its top to the window's edge and
                // cut the effort control off the bottom, out of reach. The
                // lists give up their rows; the labels and the effort row keep
                // theirs, so what is left on screen still says what it is.
                let room = panel_room(bounds, window.viewport_size().height);
                let (panel, rows) = match open {
                    ComposerMenu::Model => {
                        self.model_panel(&weak, width, room, &provider_anchor, cx)
                    }
                    ComposerMenu::Permission => self.permission_panel(&weak, width, cx),
                    ComposerMenu::Context => self.context_panel(width, room, cx),
                    ComposerMenu::Commands => self.plus_panel(&weak, width, cx),
                };
                let trigger = match open {
                    ComposerMenu::Model => model_anchor.read(cx).as_ref().copied(),
                    ComposerMenu::Permission => permission_anchor.read(cx).as_ref().copied(),
                    ComposerMenu::Context => context_anchor.read(cx).as_ref().copied(),
                    ComposerMenu::Commands => plus_anchor.read(cx).as_ref().copied(),
                };
                // The provider flyout's bounds, while it is open: a press
                // inside it, or on the row that opened it, is not an outside
                // press as far as this menu is concerned.
                let flyout = (open == ComposerMenu::Model && self.provider_menu)
                    .then(|| provider_panel_anchor.read(cx).as_ref().copied())
                    .flatten();
                let provider_row = provider_anchor.read(cx).as_ref().copied();
                let panel = panel.occlude().on_mouse_down_out({
                    let weak = weak.clone();
                    move |event: &gpui::MouseDownEvent, _window: &mut Window, cx: &mut App| {
                        // The trigger that opened the menu closes it through
                        // its own press; anything else outside the panel,
                        // the composer included, puts it away.
                        if trigger.is_some_and(|bounds| bounds.contains(&event.position)) {
                            return;
                        }
                        if flyout.is_some_and(|bounds| bounds.contains(&event.position)) {
                            return;
                        }
                        if provider_row.is_some_and(|bounds| bounds.contains(&event.position)) {
                            return;
                        }
                        if let Some(panel) = weak.upgrade() {
                            panel.update(cx, |panel, cx| panel.close_menu(cx));
                        }
                    }
                });
                root = root.child(float("chat-composer-menu", bounds, rows, panel, window, cx));
                // The provider flyout: the model menu's second level. It
                // prefers the panel's right side, but it never leaves the
                // composer's own column — a flyout hanging over the grid
                // would be a popup about another surface. Where the column
                // cannot hold panel and flyout side by side, the flyout
                // clamps inside the card's right edge and overlaps the
                // panel, the way a submenu does when its parent sits near
                // the screen edge.
                if open == ComposerMenu::Model
                    && self.provider_menu
                    && let Some(row) = provider_row
                {
                    const FLYOUT_WIDTH: f32 = 240.;
                    let panel_right = bounds.origin.x + width;
                    let card_right = bounds.origin.x + bounds.size.width - px(8.);
                    let x = (panel_right + px(6.))
                        .max(bounds.origin.x + px(2.))
                        .min(card_right - px(FLYOUT_WIDTH));
                    // Raised above the panel's own layer: a plain anchored
                    // element paints under the deferred draw `float` makes,
                    // and the panel would cover the flyout it belongs to.
                    root = root.child(
                        raise(
                            "chat-provider-flyout",
                            anchored()
                                .position(point(x, row.origin.y))
                                .anchor(gpui::Anchor::TopLeft)
                                .child(
                                    measured(
                                        &provider_panel_anchor,
                                        self.provider_panel(&weak, room, cx),
                                    )
                                    .occlude(),
                                ),
                        )
                        .with_priority(2),
                    );
                }
            }
        }
        root.into_any_element()
    }

    /// The round send at the row's end: `↑` sends, `■` stops, and an empty box
    /// keeps it quiet without taking the press away from Stop.
    fn send_circle(
        &self,
        weak: &WeakEntity<Self>,
        ready: bool,
        streaming: bool,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let (ground, ink, glyph) = if streaming {
            (colors.surface, colors.fg_muted, IconName::Square)
        } else {
            (colors.accent, colors.on_accent, IconName::ArrowUp)
        };
        let pressable = streaming || ready;
        let click = {
            let weak = weak.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |panel, cx| {
                        if streaming {
                            panel.stop(cx);
                        } else {
                            panel.send(window, cx);
                        }
                    });
                }
            }
        };
        let mut circle = div()
            .id("chat-send")
            .role(gpui::Role::Button)
            .aria_label(t("ai.chat.send"))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(30.))
            .rounded_full()
            .bg(ground)
            .when(streaming, |circle| {
                circle.border_1().border_color(colors.border)
            })
            .child(Icon::new(glyph).size(IconSize::Sm).color(ink));
        if pressable {
            circle = circle
                .cursor_pointer()
                .hover(|style| style.opacity(0.88))
                .on_click(click);
        } else {
            circle = circle.opacity(0.45);
        }
        circle.into_any_element()
    }

    /// The line above the field while a message is being revised: what the
    /// next send will do to the turn it came from, and the way out. The
    /// model's name lives on the chip below — it is not said twice.
    fn edit_line(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.editing?;
        let theme = cx.theme();
        let colors = &theme.colors;
        let weak = cx.entity().downgrade();
        Some(
            div()
                .flex()
                .flex_none()
                .w_full()
                .px(px(2.))
                .pb(px(2.))
                .gap_1p5()
                .items_center()
                .text_size(px(10.))
                .text_color(colors.fg_muted)
                .child(
                    div()
                        .size(px(6.))
                        .flex_none()
                        .rounded_full()
                        .bg(colors.accent),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(Ellipsis::new(SharedString::from(t("ai.chat.editing")))),
                )
                .child(
                    IconButton::new("chat-edit-cancel", IconName::X)
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .tooltip(t("ai.chat.edit_cancel"))
                        .on_click(move |_, _, cx| {
                            if let Some(panel) = weak.upgrade() {
                                panel.update(cx, |panel, cx| panel.cancel_edit(cx));
                            }
                        }),
                )
                .into_any_element(),
        )
    }

    /// How full the model's window is, as the next turn will find it: the
    /// last bill's input plus its output is what the next request re-sends.
    /// `cached` is the part of the input the provider answered from its
    /// cache, when it says. No bill, no ring — nothing is invented.
    fn context_usage(&self) -> Option<ContextUsage> {
        let usage = self
            .messages
            .iter()
            .rev()
            .find_map(|message| message.usage)?;
        let capacity = self
            .provider
            .as_ref()
            .and_then(selected_model)
            .and_then(|model| model.context_window)
            .filter(|window| *window > 0);
        Some(ContextUsage {
            used: usage.input_tokens + usage.output_tokens,
            capacity,
            cached: usage.cached_tokens,
            input: usage.input_tokens,
            output: usage.output_tokens,
        })
    }

    /// The context ring's panel: the numbers behind the ring, in the units
    /// they come in. A window nobody set is said, not guessed.
    fn context_panel(
        &self,
        width: Pixels,
        room: Pixels,
        cx: &Context<Self>,
    ) -> (gpui::Stateful<gpui::Div>, usize) {
        let theme = cx.theme();
        let colors = &theme.colors;
        let zh = crate::config::get(cx).appearance.locale.starts_with("zh");
        let Some(usage) = self.context_usage() else {
            let panel = ely_core::forms::surface("chat-menu-context-surface", cx)
                .w(width)
                .max_h(room)
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .child(panel_note(t("ai.chat.context_none"), cx));
            return (panel, 1);
        };
        let turns = self.messages.iter().filter(|m| m.role == "user").count();
        let mut rows = 2usize;
        let panel = ely_core::forms::surface("chat-menu-context-surface", cx)
            .w(width)
            .max_h(room)
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Sm))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(colors.fg)
                            .child(t("ai.chat.context")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_subtle)
                            .child(context_size(&usage, zh)),
                    ),
            )
            .children(usage.fraction().map(|fraction| {
                div()
                    .flex_shrink_0()
                    .px_2()
                    .child(ely_core::motion::ProgressBar::new(
                        "chat-context-bar",
                        fraction,
                    ))
            }))
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_muted)
                            .child(t("ai.chat.context_last_input")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg)
                            .child(token_text(usage.input, zh)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_muted)
                            .child(t("ai.chat.context_last_output")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg)
                            .child(token_text(usage.output, zh)),
                    ),
            );
        let panel = if usage.cached > 0 && usage.input > 0 {
            rows += 1;
            let rate = (usage.cached as f64 / usage.input as f64 * 100.0).round();
            panel.child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_muted)
                            .child(t("ai.chat.context_cache")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg)
                            .child(
                                rust_i18n::t!("ai.chat.context_cache_rate", rate = rate)
                                    .to_string(),
                            ),
                    ),
            )
        } else {
            panel
        };
        let panel = panel
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_muted)
                            .child(t("ai.chat.turns_row")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg)
                            .child(turns.to_string()),
                    ),
            )
            .children(
                (usage.capacity.is_none()).then(|| panel_note(t("ai.chat.context_no_window"), cx)),
            );
        (panel, rows)
    }

    /// The permission chip's menu: one row per mode, its own word for what it
    /// does, the chosen one checked. Full access keeps the warning tone here
    /// too, so the two agree.
    fn permission_panel(
        &self,
        weak: &WeakEntity<Self>,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> (gpui::Stateful<gpui::Div>, usize) {
        let theme = cx.theme();
        let colors = &theme.colors;
        let chosen = self.permission_mode();
        let mut panel = ely_core::forms::surface("chat-menu-permission-surface", cx)
            .w(width)
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        for mode in [PermissionMode::Ask, PermissionMode::Full] {
            let ink = if mode == PermissionMode::Full {
                colors.warning
            } else {
                colors.fg_muted
            };
            let chosen_here = chosen == mode;
            let weak = weak.clone();
            panel = panel.child(
                div()
                    .id(format!("chat-permission-{}", mode.id()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(theme.radius(Radius::Sm))
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.hover))
                    .on_click(move |_, _, cx| {
                        if let Some(panel) = weak.upgrade() {
                            panel.update(cx, |panel, cx| panel.set_permission_mode(mode, cx));
                        }
                    })
                    .child(Icon::new(mode.icon()).size(IconSize::Sm).color(ink))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(theme.text_size(TextSize::Sm))
                                    .text_color(colors.fg)
                                    .child(mode.label()),
                            )
                            .child(
                                div()
                                    .text_size(theme.text_size(TextSize::Xs))
                                    .text_color(colors.fg_subtle)
                                    .child(mode.hint()),
                            ),
                    )
                    .children(chosen_here.then(|| {
                        Icon::new(IconName::Check)
                            .size(IconSize::Sm)
                            .color(colors.fg)
                    })),
            );
        }
        (panel, 2)
    }

    /// The model chip's menu: the provider it runs on (a row that opens the
    /// provider flyout), that provider's models, and the reasoning effort. A
    /// choice that could not be written is said here rather than swallowed.
    /// `width` is the composer's own, so the panel cannot hang off a narrow
    /// aside's edge; `room` is the height the side it opens on offers, which
    /// the model list shares and the fixed rows do not give up.
    fn model_panel(
        &self,
        weak: &WeakEntity<Self>,
        width: Pixels,
        room: Pixels,
        provider_anchor: &Entity<Option<Bounds<Pixels>>>,
        cx: &mut Context<Self>,
    ) -> (gpui::Stateful<gpui::Div>, usize) {
        let theme = cx.theme();
        let colors = &theme.colors;
        let mut rows = 3;
        let open = self.provider_menu;
        let name = self.provider.as_ref().map(|p| p.name.clone());
        let format = self
            .provider
            .as_ref()
            .map(|provider| provider.format.label());
        let header = div()
            .id("chat-menu-provider")
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded(theme.radius(Radius::Sm))
            .when(open, |row| row.bg(colors.hover))
            .cursor_pointer()
            .hover(|style| style.bg(colors.hover))
            .on_click({
                let weak = weak.clone();
                move |_, _, cx| {
                    if let Some(panel) = weak.upgrade() {
                        panel.update(cx, |panel, cx| panel.toggle_provider_menu(cx));
                    }
                }
            })
            .child(
                Icon::new(IconName::Sparkles)
                    .size(IconSize::Sm)
                    .color(colors.fg_muted),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_size(theme.text_size(TextSize::Sm))
                    .text_color(colors.fg)
                    .child(Ellipsis::new(SharedString::from(
                        name.clone().unwrap_or_else(|| t("ai.config_empty")),
                    ))),
            )
            .children(format.map(|format| badge(format.to_owned(), false, cx)))
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(IconSize::Xs)
                .color(colors.fg_subtle),
            );

        let mut panel = ely_core::forms::surface("chat-menu-model-surface", cx)
            .w(width)
            .max_h(room)
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(measured(provider_anchor, header))
            .child(div().h(px(1.)).w_full().flex_shrink_0().bg(colors.border))
            .child(panel_label(t("ai.chat.menu_models"), cx));

        let mut model_rows_div = div()
            .id("chat-menu-models")
            .flex()
            .flex_col()
            .gap_1()
            .max_h(px(200.))
            .min_h(px(48.))
            .flex_shrink_1()
            .overflow_y_scroll();
        match self.provider.as_ref() {
            Some(provider) if provider.models.is_empty() => {
                panel = panel.child(panel_note(t("ai.chat.no_models"), cx));
            }
            Some(provider) => {
                for row in model_rows(provider) {
                    rows += 1;
                    let weak = weak.clone();
                    let id = row.id.clone();
                    model_rows_div = model_rows_div.child(
                        div()
                            .id(format!("chat-model-row-{}", row.id))
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .py_1()
                            .rounded(theme.radius(Radius::Sm))
                            .when(row.selected, |line| line.bg(colors.hover))
                            .cursor_pointer()
                            .hover(|style| style.bg(colors.hover))
                            .on_click(move |_, _, cx| {
                                if let Some(panel) = weak.upgrade() {
                                    let id = id.clone();
                                    panel.update(cx, |panel, cx| panel.pick_model(&id, cx));
                                }
                            })
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(theme.text_size(TextSize::Sm))
                                    .text_color(colors.fg)
                                    .child(Ellipsis::new(SharedString::from(row.label))),
                            )
                            .when(row.vision, |line| {
                                line.child(badge(t("ai.chat.vision"), false, cx))
                            })
                            .child(div().flex_1())
                            .when(row.selected, |line| {
                                line.child(
                                    Icon::new(IconName::Check)
                                        .size(IconSize::Xs)
                                        .color(colors.accent),
                                )
                            }),
                    );
                }
                panel = panel.child(model_rows_div);
            }
            None => panel = panel.child(panel_note(t("ai.config_empty"), cx)),
        }

        panel = panel
            .child(div().h(px(1.)).w_full().flex_shrink_0().bg(colors.border))
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Sm))
                            .text_color(colors.fg_muted)
                            .child(t("ai.chat.effort")),
                    )
                    .child(div().flex_1())
                    .child(KbdCombo::new("ctrl-t")),
            );
        if let Some(provider) = self.provider.as_ref() {
            let level = provider.thinking_level;
            let mut control =
                SegmentedControl::new("chat-menu-level", level.as_str()).size(ControlSize::Sm);
            for each in ThinkingLevel::all() {
                control = control.segment(
                    each.as_str(),
                    t(&format!("ai.effort.{}", each.as_str())),
                    None,
                );
            }
            let weak = weak.clone();
            control = control.on_change(move |picked, _, cx| {
                if let Some(panel) = weak.upgrade() {
                    let token = picked.to_string();
                    panel.update(cx, |panel, cx| panel.choose_level(&token, cx));
                }
            });
            panel = panel.child(div().flex_shrink_0().child(control));
        }
        if let Some(error) = self.menu_error.as_deref() {
            panel = panel.child(
                div()
                    .flex_shrink_0()
                    .px_2()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.danger)
                    .child(error.to_owned()),
            );
        }
        (panel, rows)
    }

    /// The provider flyout, the model menu's second level: every provider
    /// this client has, the chosen one checked, and the way into the models
    /// page. A provider picked here refills the panel behind it; many
    /// providers never grow one giant list.
    fn provider_panel(
        &self,
        weak: &WeakEntity<Self>,
        room: Pixels,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = cx.theme();
        let colors = &theme.colors;
        let mut panel = ely_core::forms::surface("chat-menu-provider-surface", cx)
            .w(px(240.))
            .max_h(room)
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(panel_label(t("ai.chat.menu_providers"), cx));
        if self.providers.is_empty() {
            panel = panel.child(panel_note(t("ai.config_empty"), cx));
        }
        for provider in &self.providers {
            let chosen = self
                .provider
                .as_ref()
                .is_some_and(|current| current.name == provider.name);
            let weak = weak.clone();
            let name = provider.name.clone();
            let format = provider.format.label();
            panel = panel.child(
                div()
                    .id(format!("chat-provider-{}", provider.name))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(theme.radius(Radius::Sm))
                    .when(chosen, |row| row.bg(colors.hover))
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.hover))
                    .on_click(move |_, _, cx| {
                        if let Some(panel) = weak.upgrade() {
                            let name = name.clone();
                            panel.update(cx, |panel, cx| panel.choose_provider(&name, cx));
                        }
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_size(theme.text_size(TextSize::Sm))
                            .text_color(colors.fg)
                            .child(Ellipsis::new(SharedString::from(provider.name.clone()))),
                    )
                    .child(badge(format.to_owned(), false, cx))
                    .when(chosen, |row| {
                        row.child(
                            Icon::new(IconName::Check)
                                .size(IconSize::Sm)
                                .color(colors.accent),
                        )
                    }),
            );
        }
        let manage = weak.clone();
        panel
            .child(div().h(px(1.)).w_full().flex_shrink_0().bg(colors.border))
            .child(
                div()
                    .id("chat-provider-manage")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(theme.radius(Radius::Sm))
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.hover))
                    .on_click(move |_, _, cx| {
                        if let Some(panel) = manage.upgrade() {
                            panel.update(cx, |panel, cx| panel.open_models_settings(cx));
                        }
                    })
                    .child(
                        Icon::new(IconName::Settings)
                            .size(IconSize::Sm)
                            .color(colors.fg_muted),
                    )
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Sm))
                            .text_color(colors.fg)
                            .child(t("ai.chat.manage_models")),
                    ),
            )
    }

    /// The `+` button's menu: what the composer itself can take in. A new chat
    /// and the history are the header's presses; this is the field's.
    fn plus_panel(
        &self,
        weak: &WeakEntity<Self>,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> (gpui::Stateful<gpui::Div>, usize) {
        let theme = cx.theme();
        let colors = &theme.colors;
        let mut rows = 0;
        let mut panel = ely_core::forms::surface("chat-menu-command-surface", cx)
            .w(width)
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        let mut row_of = |panel: gpui::Stateful<gpui::Div>,
                          id: &'static str,
                          icon: IconName,
                          label: String,
                          press: WeakEntity<Self>,
                          act: fn(&mut ChatView, &mut Context<ChatView>)|
         -> gpui::Stateful<gpui::Div> {
            rows += 1;
            panel.child(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(theme.radius(Radius::Sm))
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.hover))
                    .on_click(move |_, _, cx| {
                        if let Some(panel) = press.upgrade() {
                            panel.update(cx, |panel, cx| {
                                panel.menu = None;
                                act(panel, cx);
                            });
                        }
                    })
                    .child(Icon::new(icon).size(IconSize::Sm).color(colors.fg_subtle))
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Sm))
                            .text_color(colors.fg)
                            .child(label),
                    ),
            )
        };
        panel = row_of(
            panel,
            "chat-command-attach",
            IconName::Paperclip,
            t("ai.chat.attach"),
            weak.clone(),
            |panel, cx| panel.attach_files(cx),
        );
        panel = row_of(
            panel,
            "chat-command-clear",
            IconName::Eraser,
            t("ai.chat.clear_input"),
            weak.clone(),
            |panel, cx| {
                panel.input.update(cx, |input, cx| input.set_text("", cx));
                cx.notify();
            },
        );
        (panel, rows)
    }
}

/// Which of the composer's menus is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComposerMenu {
    /// The model chip's: the provider row, its models, the reasoning effort —
    /// and the provider flyout as its second level.
    Model,
    /// The permission chip's: one row per gate mode.
    Permission,
    /// The context ring's: the numbers behind it.
    Context,
    /// The `+`'s: attachments and the field's own small acts.
    Commands,
}

/// What the gate does with a tool call: work autonomously and ask when a call
/// is risky, or run everything because the reader allowed this sitting full
/// access.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PermissionMode {
    #[default]
    Ask,
    Full,
}

impl PermissionMode {
    fn of(full_access: bool) -> Self {
        if full_access { Self::Full } else { Self::Ask }
    }

    /// The row's and the chip's element id.
    fn id(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Full => "full",
        }
    }

    /// Both modes carry the shield; the tone — the palette's ink, or the
    /// warning tone while full access is on — is what tells them apart.
    fn icon(self) -> IconName {
        IconName::Shield
    }

    fn label(self) -> String {
        t(match self {
            Self::Ask => "ai.chat.permission_ask",
            Self::Full => "ai.chat.permission_full",
        })
    }

    fn hint(self) -> String {
        t(match self {
            Self::Ask => "ai.chat.permission_ask_hint",
            Self::Full => "ai.chat.permission_full_hint",
        })
    }
}

/// How full the model's window is, as the context ring and its panel read it.
#[derive(Clone, Copy, Debug)]
struct ContextUsage {
    /// What the next request re-sends: the last bill's input plus its output.
    used: u64,
    /// The chosen model's window, when the models page set one.
    capacity: Option<u64>,
    /// The input the provider answered from cache, when it says.
    cached: u64,
    input: u64,
    output: u64,
}

impl ContextUsage {
    /// The ring's fill, 0..=1, when a window is known to be full.
    fn fraction(&self) -> Option<f32> {
        let capacity = self.capacity?;
        if self.used == 0 {
            return Some(0.);
        }
        Some(((self.used as f64 / capacity as f64).min(1.0) as f32).clamp(0., 1.))
    }
}

/// The ring's tooltip: what is used, against what, in the reader's units.
fn context_title(usage: &ContextUsage, cx: &App) -> String {
    let zh = crate::config::get(cx).appearance.locale.starts_with("zh");
    match usage.capacity {
        Some(capacity) => rust_i18n::t!(
            "ai.chat.context_full",
            used = token_text(usage.used, zh),
            capacity = token_text(capacity, zh)
        ),
        None => rust_i18n::t!("ai.chat.context_used", used = token_text(usage.used, zh)),
    }
    .to_string()
}

/// The head of the context panel: the same numbers as the tooltip, spelled
/// out.
fn context_size(usage: &ContextUsage, zh: bool) -> String {
    match usage.capacity {
        Some(capacity) => {
            let rate = if usage.used > 0 {
                format!(
                    " ({:.0}%)",
                    usage.used.min(capacity) as f64 / capacity as f64 * 100.
                )
            } else {
                String::new()
            };
            format!(
                "{} / {}{}",
                token_text(usage.used, zh),
                token_text(capacity, zh),
                rate
            )
        }
        None => format!("{} tok", usage.used),
    }
}

/// A token count as the reader says it: 万 above ten thousand in Chinese,
/// k/M steps otherwise.
fn token_text(tokens: u64, zh: bool) -> String {
    fn step(tokens: f64, unit: f64, suffix: &str) -> String {
        let value = tokens / unit;
        let text = if value >= 10. || (value * 10.).round() == value * 10. {
            format!("{value:.0}")
        } else {
            format!("{value:.1}")
        };
        format!("{text}{suffix}")
    }
    if zh && tokens >= 10_000 {
        step(tokens as f64, 10_000., "万")
    } else if tokens >= 1_000_000 {
        step(tokens as f64, 1_000_000., "M")
    } else if tokens >= 10_000 {
        step(tokens as f64, 1_000., "k")
    } else {
        tokens.to_string()
    }
}

/// The `/` menu of the composer, built the way the library's `PromptInput`
/// builds its own: the rows open under the trigger while the reader types.
/// Written here because the card is ours while the field and its suggestions
/// stay the library's.
fn command_offers(
    id: ElementId,
    field: &Entity<TextInput>,
    commands: &[Choice],
    on_command: OnValue,
    cx: &App,
) -> Suggestions {
    let input = field.read(cx);
    let (text, caret) = (input.text().to_string(), input.cursor());
    // The commands belong to the whole box, so the trigger must open the
    // message rather than a word in the middle of one.
    let active =
        active_trigger(&text, caret, &['/'], |ch| !ch.is_whitespace()).filter(|(at, _)| *at == 0);
    let query = active.map_or(String::new(), |(at, _)| text[at + 1..caret].to_lowercase());
    let found: Vec<Choice> = commands
        .iter()
        .filter(|choice| choice.label.to_lowercase().contains(&query))
        .cloned()
        .collect();
    let rows = found.clone();
    let state = field.clone();
    let target = field.clone();
    let pick: Pick = Rc::new(move |ix: usize, window: &mut Window, cx: &mut App| {
        // Rows are only offered under a trigger, but a pick that arrives
        // without one does nothing rather than panicking.
        let Some((at, _)) = active else {
            return;
        };
        let caret = target.read(cx).cursor();
        replace_trigger(&target, at, caret, "", cx);
        tracing::info!(target: "zenix-chat", "composer: picked {}", found[ix].value);
        on_command(&found[ix].value, window, cx);
    });
    Suggestions {
        id,
        state,
        trigger: active.map(|(at, _)| at),
        rows,
        pick,
    }
}

/// One row of the model menu, copied out of the provider so the popup can be
/// built without the provider borrowed.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelRow {
    id: String,
    label: String,
    vision: bool,
    selected: bool,
}

/// The rows of the model menu: every model the provider lists, with the one
/// the next turn runs on marked.
fn model_rows(provider: &ApiProviderConfig) -> Vec<ModelRow> {
    provider
        .models
        .iter()
        .map(|model| ModelRow {
            id: model.id.clone(),
            label: model.display_label().to_owned(),
            vision: model.vision,
            selected: model.id == provider.model,
        })
        .collect()
}

/// The provider's chosen model, when its list still names the id.
fn selected_model(provider: &ApiProviderConfig) -> Option<&crate::api_provider::ModelConfig> {
    provider
        .models
        .iter()
        .find(|model| model.id == provider.model)
}

/// The model a turn runs on, as the chip's first half: the display label the
/// list gives it, else the id itself (a file older than its list still names
/// one).
fn model_name(provider: &ApiProviderConfig) -> String {
    provider
        .models
        .iter()
        .find(|model| model.id == provider.model)
        .map(|model| model.display_label().to_owned())
        .unwrap_or_else(|| provider.model.trim().to_owned())
}

/// The next reasoning level, wrapping: `Ctrl-T` walks all four with one key.
fn next_level(level: ThinkingLevel) -> ThinkingLevel {
    let all = ThinkingLevel::all();
    let at = all.iter().position(|each| *each == level).unwrap_or(0);
    all[(at + 1) % all.len()]
}

/// The chip's second half: the effort the next turn asks for.
fn level_label(provider: &ApiProviderConfig) -> String {
    t(&format!("ai.effort.{}", provider.thinking_level.as_str()))
}

/// How tall a menu hanging from `anchor` may grow: the room on the side with
/// more space, less a hair of breathing room. A window shorter than the menu
/// makes the library pin the popup to the window's edge, which would put the
/// effort row past the bottom; the panel is capped at this instead and its
/// lists give up their rows. The floor keeps the panel readable in a window
/// narrower than the menu's own chrome.
fn panel_room(anchor: Bounds<Pixels>, viewport: Pixels) -> Pixels {
    let above = anchor.origin.y;
    let below = viewport - anchor.bottom();
    let side = if above > below { above } else { below };
    (side - px(8.)).max(px(160.))
}

/// Wraps `child` and records its laid-out bounds in `slot`, so a menu it opens
/// can tell a press on its own trigger from an outside one (the library's
/// popovers measure their trigger the same way).
fn measured(slot: &Entity<Option<Bounds<Pixels>>>, child: impl IntoElement) -> gpui::Div {
    let slot = slot.clone();
    div().relative().min_w_0().child(child).child(
        canvas(
            move |bounds, window, cx| {
                if slot.read(cx).as_ref() != Some(&bounds) {
                    slot.update(cx, |slot, cx| {
                        *slot = Some(bounds);
                        cx.notify();
                        window.request_animation_frame();
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

/// A quiet name above a section of the composer's menu.
fn panel_label(text: String, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex_shrink_0()
        .px_2()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(theme.colors.fg_subtle)
        .child(text)
        .into_any_element()
}

/// A line a menu shows in place of rows, such as a provider with no models.
fn panel_note(text: String, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex_shrink_0()
        .px_2()
        .py_1()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(theme.colors.fg_muted)
        .child(text)
        .into_any_element()
}

/// A small token on a menu row (a format, a capability): the settings page's
/// own shape, one step quieter.
fn badge(text: String, ok: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let (ink, ground) = if ok {
        (colors.success, colors.success_subtle)
    } else {
        (colors.fg_muted, colors.sunken)
    };
    div()
        .flex_none()
        .px_1p5()
        .h(px(18.))
        .flex()
        .items_center()
        .rounded(theme.radius(Radius::Sm))
        .bg(ground)
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(ink)
        .child(text)
        .into_any_element()
}

/// The composer's attachment chips, above the field: one per picked file,
/// its icon by kind, its × taking it back off. `None` while nothing waits.
fn attachment_chips(
    attachments: &[Attachment],
    weak: &WeakEntity<ChatView>,
    cx: &App,
) -> Option<AnyElement> {
    if attachments.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut row = div().flex().flex_wrap().flex_none().w_full().gap_1();
    for (index, attachment) in attachments.iter().enumerate() {
        let (icon, ink) = match &attachment.kind {
            AttachmentKind::Image { .. } => (IconName::FileImage, colors.fg_muted),
            AttachmentKind::Text { .. } => (IconName::FileText, colors.fg_muted),
            AttachmentKind::Failed { .. } => (IconName::CircleAlert, colors.danger),
        };
        let remove = weak.clone();
        let failed = attachment.kind.failed();
        let label = match &attachment.kind {
            AttachmentKind::Failed { reason } => {
                format!("{} · {reason}", attachment.name())
            }
            _ => attachment.name(),
        };
        row = row.child(
            div()
                .id(SharedString::from(format!("chat-attach-{index}")))
                .flex()
                .flex_none()
                .max_w(px(220.))
                .items_center()
                .gap_1p5()
                .pl_2()
                .pr_1()
                .h(px(24.))
                .rounded(theme.radius(Radius::Md))
                .border_1()
                .border_color(colors.border)
                .bg(colors.sunken)
                .child(Icon::new(icon).size(IconSize::Xs).color(ink))
                .child(
                    div()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(if failed {
                            colors.danger
                        } else {
                            colors.fg_muted
                        })
                        .child(Ellipsis::new(SharedString::from(label))),
                )
                .child(
                    IconButton::new(
                        SharedString::from(format!("chat-attach-remove-{index}")),
                        IconName::X,
                    )
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .tooltip(t("ai.chat.attach_remove"))
                    .on_click(move |_, _, cx| {
                        if let Some(panel) = remove.upgrade() {
                            panel.update(cx, |panel, cx| panel.remove_attachment(index, cx));
                        }
                    }),
                ),
        );
    }
    Some(row.into_any_element())
}

impl AttachmentKind {
    /// A read that said no. The chip's ink and its name-for-the-reader follow.
    fn failed(&self) -> bool {
        matches!(self, AttachmentKind::Failed { .. })
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

/// A message as a block. The user's words are a narrow strip set to the right;
/// the model's are the column itself — no bubble, no avatar, no name — with
/// what it thought and the tools it used above them, and the turn's own line
/// last. What the model says is markdown; what the user typed is their own
/// text, shown as they wrote it.
fn message_row(
    message: MessageModel,
    weak: &WeakEntity<ChatView>,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if message.user {
        user_block(message, weak, index, cx)
    } else {
        assistant_block(message, weak, index, window, cx)
    }
}

/// A user turn: a strip that never fills the column, wearing no name, avatar
/// or time. Only the words, and the two actions that repeat or revise them.
fn user_block(
    message: MessageModel,
    weak: &WeakEntity<ChatView>,
    index: usize,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut block = div()
        .w_full()
        .flex()
        .flex_col()
        .items_end()
        .gap_1()
        .pb_6()
        .child(
            div()
                .id(("chat-user", index))
                .max_w(px(520.))
                .px_4()
                .py_3()
                .rounded(theme.radius(Radius::Md))
                .border_1()
                .border_color(if message.editing {
                    colors.accent
                } else {
                    colors.border
                })
                .bg(colors.surface)
                .text_size(theme.text_size(TextSize::Base))
                .line_height(relative(1.6))
                .text_color(colors.fg)
                .child(plain_lines(message.content.clone())),
        );
    if !message.content.is_empty() {
        let mut actions = div()
            .flex()
            .items_center()
            .gap_1()
            .child(copy_button(("chat-copy", index), &message.content));
        if message.can_edit {
            let weak = weak.clone();
            actions = actions.child(if message.editing {
                // A different id from the pencil: the library caches a
                // tooltip by element id, and the words change with the state.
                IconButton::new(("chat-edit-cancel", index), IconName::X)
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .tooltip(t("ai.chat.edit_cancel"))
                    .on_click(move |_, _, cx| {
                        if let Some(panel) = weak.upgrade() {
                            panel.update(cx, |panel, cx| panel.cancel_edit(cx));
                        }
                    })
            } else {
                IconButton::new(("chat-edit", index), IconName::Pencil)
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .tooltip(t("ai.chat.edit"))
                    .on_click(move |_, window, cx| {
                        if let Some(panel) = weak.upgrade() {
                            panel.update(cx, |panel, cx| panel.start_edit(index, window, cx));
                        }
                    })
            });
        }
        block = block.child(actions);
    }
    block.into_any_element()
}

/// The model's turn: the reasoning it thought with, one quiet row per tool it
/// used, what it said in full, and the turn's own line. Flat by design — none
/// of it wears a card.
fn assistant_block(
    message: MessageModel,
    weak: &WeakEntity<ChatView>,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let streaming = message.streaming;
    let mut block = div().w_full().flex().flex_col().gap_3().pb_6();
    for (row_index, row) in message.rows.iter().enumerate() {
        block = block.child(match row {
            TranscriptRow::Reasoning {
                text,
                took_ms,
                live,
            } => reasoning_block(index, row_index, text, *live, *took_ms, window, cx),
            TranscriptRow::Steps(calls) => {
                let mut steps = div().w_full().flex().flex_col().gap_1();
                for run in step_runs(calls) {
                    steps = steps.child(match run {
                        StepRun::One(call) => tool_call(index, call, window, cx),
                        StepRun::Group { family, calls } => {
                            step_group(index, family, calls, window, cx)
                        }
                    });
                }
                steps.into_any_element()
            }
            TranscriptRow::Text { text, live } => {
                let theme = cx.theme();
                div()
                    .w_full()
                    .text_size(theme.text_size(TextSize::Base))
                    .line_height(relative(1.6))
                    .text_color(theme.colors.fg)
                    .child(StreamingMarkdown::new(
                        SharedString::from(format!("chat-md-{index}-{row_index}")),
                        text.clone(),
                        *live,
                    ))
                    .into_any_element()
            }
        });
    }
    if streaming && message.rows.is_empty() {
        block = block
            .child(ThinkingIndicator::new(("chat-waiting", index)).label(t("ai.chat.streaming")));
    }
    if !streaming && !message.content.is_empty() {
        block = block.child(tail_row(&message, weak, index, cx));
    }
    block.into_any_element()
}

/// The user's words, line for line: what they typed, not what a markdown
/// renderer would make of it.
fn plain_lines(text: String) -> AnyElement {
    let mut lines = div().w_full().flex().flex_col().gap_0p5();
    for line in text.split('\n') {
        lines = lines.child(div().child(line.to_owned()));
    }
    lines.into_any_element()
}

/// The copy affordance both roles carry: the words as they stand on screen.
fn copy_button(id: (&'static str, usize), text: &str) -> IconButton {
    let text = text.to_owned();
    IconButton::new(id, IconName::Copy)
        .variant(ButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .tooltip(t("ai.chat.copy"))
        .on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        })
}

/// The turn's own line: what it cost, the actions that can be taken on it, and
/// when it was said. Quiet by design; the words above it are the point.
fn tail_row(
    message: &MessageModel,
    weak: &WeakEntity<ChatView>,
    index: usize,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut row = div()
        .id(("chat-tail", index))
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .py_1();
    if let Some(usage) = message.usage {
        row = row.child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(format!("{} tok", usage.total_tokens())),
        );
    }
    row = row.child(div().flex_1());
    row = row.child(copy_button(("chat-copy", index), &message.content));
    if message.can_retry {
        let weak = weak.clone();
        row = row.child(
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
    if let Some(at) = message.at {
        row = row.child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(relative_label(at, cx)),
        );
    }
    row.into_any_element()
}

/// The family a tool call belongs to. The family decides two things: the words
/// a row reads with, and whether a run of them folds into one line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepFamily {
    /// The model driving a pane's terminal (`send_text`). Always foldable.
    Terminal,
    /// Looking at what a pane or the fleet is doing. Foldable once every call
    /// in the run has finished — a running or failed read stays visible.
    Read,
    /// Calls with consequences of their own (`spawn_worker`, `close_worker`)
    /// and anything the client does not know. Never folded away.
    Manage,
}

impl StepFamily {
    fn groupable(self) -> bool {
        self != StepFamily::Manage
    }

    fn label(self) -> String {
        match self {
            StepFamily::Terminal => t("ai.chat.group.terminal"),
            StepFamily::Read => t("ai.chat.group.read"),
            StepFamily::Manage => String::new(),
        }
    }
}

/// What a call did, in the reader's words: the verb, what it acted on, and the
/// quiet extra. A tool the client does not know keeps its wire name and falls
/// back to its arguments, so a new tool reads as itself rather than vanishing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepVerb {
    Send,
    Read,
    Wait,
    List,
    Start,
    Close,
}

impl StepVerb {
    fn label(self) -> String {
        t(match self {
            StepVerb::Send => "ai.chat.step.send",
            StepVerb::Read => "ai.chat.step.read",
            StepVerb::Wait => "ai.chat.step.wait",
            StepVerb::List => "ai.chat.step.list",
            StepVerb::Start => "ai.chat.step.start",
            StepVerb::Close => "ai.chat.step.close",
        })
    }
}

/// One tool call as a step row.
#[derive(Clone, Debug, PartialEq)]
struct StepFacts {
    family: StepFamily,
    /// `None` for a tool this client has no verb for; the row then shows the
    /// name the wire used.
    verb: Option<StepVerb>,
    target: String,
    meta: Option<String>,
}

/// One entry of the conversation's step list: a call on its own, or a run of
/// same-family calls behind one line.
#[derive(Debug)]
enum StepRun<'a> {
    One(&'a ToolCallState),
    Group {
        family: StepFamily,
        calls: &'a [ToolCallState],
    },
}

/// Derive what a call acted on. The wire's argument names are the only thing
/// that can be trusted here: a call that has not run yet has no result, and its
/// arguments are already in hand.
fn step_facts(call: &ToolCallState) -> StepFacts {
    let arg = |key: &str| {
        call.args
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let non_empty = |text: String| Some(text).filter(|text| !text.is_empty());
    match call.name.as_str() {
        "send_text" => StepFacts {
            family: StepFamily::Terminal,
            verb: Some(StepVerb::Send),
            target: summarize(&arg("text"), 60),
            meta: non_empty(arg("pane_id")),
        },
        "read_output" => StepFacts {
            family: StepFamily::Read,
            verb: Some(StepVerb::Read),
            target: arg("pane_id"),
            meta: call
                .args
                .get("lines")
                .and_then(|value| value.as_u64())
                .map(|lines| rust_i18n::t!("ai.chat.step.lines", count = lines).to_string()),
        },
        "wait_for_output" => StepFacts {
            family: StepFamily::Read,
            verb: Some(StepVerb::Wait),
            target: arg("pane_id"),
            meta: non_empty(summarize(&arg("pattern"), 40)),
        },
        "list_workers" => StepFacts {
            family: StepFamily::Read,
            verb: Some(StepVerb::List),
            target: String::new(),
            meta: None,
        },
        "spawn_worker" => StepFacts {
            family: StepFamily::Manage,
            verb: Some(StepVerb::Start),
            target: arg("agent"),
            meta: non_empty(arg("worker_name")),
        },
        "close_worker" => StepFacts {
            family: StepFamily::Manage,
            verb: Some(StepVerb::Close),
            target: arg("pane_id"),
            meta: None,
        },
        _ => StepFacts {
            family: StepFamily::Manage,
            verb: None,
            target: arguments_summary(&call.args),
            meta: None,
        },
    }
}

/// How one turn's calls read: consecutive calls of a groupable family become a
/// run; a run folds only when the whole run may be folded away. The rules are
/// the design's (§3.2, ported from BitFun's policy, not its code):
///
/// - the terminal family always folds, failures included;
/// - reads fold only when every call in the run finished and none failed;
/// - management calls and unknown tools stand on their own;
/// - a run of one is not a group — it keeps its own step row.
fn step_runs(calls: &[ToolCallState]) -> Vec<StepRun<'_>> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < calls.len() {
        let family = step_facts(&calls[start]).family;
        if !family.groupable() {
            runs.push(StepRun::One(&calls[start]));
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < calls.len() && step_facts(&calls[end]).family == family {
            end += 1;
        }
        let run = &calls[start..end];
        let folds = match family {
            StepFamily::Terminal => true,
            StepFamily::Read => run
                .iter()
                .all(|call| call.status == ToolCallStatus::Completed),
            StepFamily::Manage => false,
        };
        if run.len() > 1 && folds {
            runs.push(StepRun::Group { family, calls: run });
        } else {
            for call in run {
                runs.push(StepRun::One(call));
            }
        }
        start = end;
    }
    runs
}

/// One tool call as a step: a status mark, the verb, what it acted on, and the
/// quiet extra — a flat row with no frame. Opening it shows the arguments and
/// the answer that came back.
fn tool_call(
    message_index: usize,
    call: &ToolCallState,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let status = match call.status {
        ToolCallStatus::Running => StepState::Working,
        ToolCallStatus::Completed => StepState::Done,
        // An interrupted call never got an answer; it wears the failed mark
        // rather than pretending to still be working.
        ToolCallStatus::Failed | ToolCallStatus::Interrupted => StepState::Failed,
    };
    let id = gpui::ElementId::from(SharedString::from(format!(
        "chat-tool-{message_index}-{}",
        call.id
    )));
    let opens = !call.args.is_null() || call.result_summary.is_some();
    let state = window.use_keyed_state((id.clone(), "open"), cx, |_, _| false);
    let open = *state.read(cx);
    let mark = step_mark((id.clone(), "mark").into(), status, window, cx);
    let theme = cx.theme();
    let colors = &theme.colors;
    let facts = step_facts(call);
    let verb = match facts.verb {
        Some(verb) => verb.label(),
        None => call.name.clone(),
    };
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
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded(theme.radius(Radius::Sm))
        .when(opens, |row| {
            row.cursor_pointer()
                .hover(|style| style.bg(colors.hover))
                .on_click(toggle)
        })
        .child(mark)
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Sm))
                .text_color(colors.fg_muted)
                .child(Ellipsis::new(SharedString::from(verb))),
        )
        .when(!facts.target.is_empty(), |row| {
            row.child(
                div()
                    .min_w_0()
                    .text_size(theme.text_size(TextSize::Sm))
                    .text_color(colors.fg)
                    .child(Ellipsis::new(SharedString::from(facts.target.clone()))),
            )
        })
        .child(div().flex_1());
    if let Some(meta) = facts.meta.as_ref() {
        header = header.child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(meta.clone()),
        );
    }
    if let Some(took) = took {
        header = header.child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(took_label(took)),
        );
    }
    if opens {
        header = header.child(Disclosure::new((id.clone(), "chevron"), open).size(IconSize::Sm));
    }
    let mut block = div().w_full().flex().flex_col().gap_1().child(header);
    if open && opens {
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .pl_4()
            .ml_3()
            .border_l_1()
            .border_color(colors.border)
            .py_1();
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
        block = block.child(body);
    }
    block.into_any_element()
}

/// A run of same-family steps behind one line: `终端 · 2 个命令`, and the calls
/// it covers when opened. Each member keeps its own verb, target and answer, so
/// opening the run is reading the transcript, not a summary of it.
fn step_group(
    message_index: usize,
    family: StepFamily,
    calls: &[ToolCallState],
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let first = calls
        .first()
        .map(|call| call.id.clone())
        .unwrap_or_default();
    let id = gpui::ElementId::from(SharedString::from(format!(
        "chat-group-{message_index}-{first}"
    )));
    let state = window.use_keyed_state((id.clone(), "open"), cx, |_, _| false);
    let open = *state.read(cx);
    let theme = cx.theme();
    let colors = &theme.colors;
    let failed = calls
        .iter()
        .filter(|call| call.status == ToolCallStatus::Failed)
        .count();
    let count = match family {
        StepFamily::Terminal => {
            rust_i18n::t!("ai.chat.group.commands", count = calls.len()).to_string()
        }
        _ => rust_i18n::t!("ai.chat.group.reads", count = calls.len()).to_string(),
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
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded(theme.radius(Radius::Sm))
        .cursor_pointer()
        .hover(|style| style.bg(colors.hover))
        .on_click(toggle)
        .child(
            Icon::new(match family {
                StepFamily::Terminal => IconName::Terminal,
                _ => IconName::BookOpen,
            })
            .size(IconSize::Sm)
            .color(colors.fg_subtle),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Sm))
                .text_color(colors.fg_muted)
                .child(family.label()),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(format!("· {count}")),
        );
    if failed > 0 {
        header = header.child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.warning)
                .child(rust_i18n::t!("ai.chat.group.failed", count = failed).to_string()),
        );
    }
    let mut block = div().w_full().flex().flex_col().gap_1().child(
        header
            .child(div().flex_1())
            .child(Disclosure::new((id.clone(), "chevron"), open).size(IconSize::Sm)),
    );
    if open {
        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .pl_4()
            .ml_3()
            .border_l_1()
            .border_color(colors.border)
            .py_1();
        for call in calls {
            let facts = step_facts(call);
            let failed = call.status == ToolCallStatus::Failed;
            let lead = match family {
                StepFamily::Terminal => format!("$ {}", facts.target),
                _ => match facts.verb {
                    Some(verb) => format!("{} {}", verb.label(), facts.target),
                    None => format!("{} {}", call.name, facts.target),
                },
            };
            let mut line = div().flex().flex_col().gap_0p5().child(
                div()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(if failed { colors.danger } else { colors.fg })
                    .child(lead.trim_end().to_owned()),
            );
            if let Some(result) = call.result_summary.as_ref() {
                line = line.child(
                    div()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(if failed {
                            colors.danger
                        } else {
                            colors.fg_subtle
                        })
                        .child(result.clone()),
                );
            }
            body = body.child(line);
        }
        block = block.child(body);
    }
    block.into_any_element()
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

/// The thinking row's own words: the label, and the quiet length that follows
/// it. Drawn here because the library's `ThinkingDuration` prints "Thought
/// for …" and the row must speak the reader's language.
fn thought_words(streaming: bool, took_ms: Option<u64>) -> (String, Option<String>) {
    if streaming {
        return (t("ai.chat.streaming"), None);
    }
    let meta = match took_ms.unwrap_or(0) / 1000 {
        0 => t("ai.chat.thought_moment"),
        seconds => rust_i18n::t!("ai.chat.thought_took", seconds = seconds).to_string(),
    };
    (t("ai.chat.thought"), Some(meta))
}

/// The reasoning a turn thought with, folded away until it is asked for: it is
/// the model talking to itself, and it is long. The header is the panel's own
/// so its words follow the reader's locale; the library draws the text inside.
fn reasoning_block(
    index: usize,
    run: usize,
    reasoning: &str,
    streaming: bool,
    took_ms: Option<u64>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = SharedString::from(format!("chat-thinking-{index}-{run}"));
    let key = gpui::ElementId::from(id.clone());
    let open = window.use_keyed_state((key.clone(), "open"), cx, |_, _| false);
    let opened = *open.read(cx);
    let theme = cx.theme();
    let colors = &theme.colors;
    let (label, meta) = thought_words(streaming, took_ms);
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
            .px_2()
            .py_1()
            .rounded(theme.radius(Radius::Sm))
            .cursor_pointer()
            .hover(|style| style.bg(colors.hover))
            .on_click(toggle)
            .child(
                Icon::new(IconName::Sparkles)
                    .size(IconSize::Sm)
                    .color(colors.fg_subtle),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(theme.text_size(TextSize::Sm))
                    .text_color(colors.fg_muted)
                    .child(label),
            )
            .when_some(meta, |row, meta| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(format!("· {meta}")),
                )
            })
            .child(div().flex_1())
            .child(Disclosure::new((key.clone(), "chevron"), opened).size(IconSize::Sm)),
    );
    if opened {
        block = block.child(
            div()
                .pl_4()
                .ml_3()
                .border_l_1()
                .border_color(colors.border)
                .py_1()
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
        .pb_2()
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
    let begin = {
        let weak = weak.clone();
        let session_id = session.id.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            if let Some(panel) = weak.upgrade() {
                panel.update(cx, |panel, cx| panel.begin_rename(&session_id, window, cx));
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
    // While this row's title is being edited, the cell carries the library's
    // field instead of the text.
    let editing = chat
        .history
        .as_ref()
        .and_then(|history| history.renaming.as_ref())
        .filter(|(renaming, _)| renaming == &session.id)
        .map(|(_, editing)| editing.clone());
    let title_cell: AnyElement = if let Some(editing) = editing {
        let escape = {
            let weak = weak.clone();
            move |event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut App| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    if let Some(panel) = weak.upgrade() {
                        panel.update(cx, |panel, cx| panel.cancel_rename(window, cx));
                    }
                }
            }
        };
        div()
            .id((id.clone(), "edit-cell"))
            .flex_1()
            .min_w_0()
            // The open field must not let a press reach the row: the row's
            // select would load the session and unmount the field under the
            // caret.
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_key_down(escape)
            .children(editing.read(cx).field().map(|field| Input::new(&field)))
            .into_any_element()
    } else {
        div()
            .flex_1()
            .min_w_0()
            .child(Ellipsis::new(SharedString::from(session.title.clone())))
            .into_any_element()
    };
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
        .child(title_cell)
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .children(stamp),
        )
        .child(
            IconButton::new((id.clone(), "rename"), IconName::Pencil)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip(t("ai.chat.rename"))
                .on_click(begin),
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
        entries: Vec::new(),
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

/// What editing `index` puts back in the composer: the user's own words, when
/// the row is one of theirs. The model's messages have nothing to revise.
fn edit_target(messages: &[ChatMessage], index: usize) -> Option<String> {
    let message = messages.get(index)?;
    (message.role == "user" && !message.content.is_empty()).then(|| message.content.clone())
}

/// Cut the conversation back to just before `index`: an edit replaces the
/// message it started from and every answer below it.
fn cut_before(messages: &mut Vec<ChatMessage>, index: usize) {
    messages.truncate(index);
}

/// Forget the replaced turn in the store, at and after `sequence`. A failure
/// here must not stop the new turn, so it is logged rather than shown.
fn truncate_store(session_id: &str, sequence: i64) {
    if session_id.is_empty() {
        return;
    }
    match SessionManager::shared() {
        Ok(manager) => {
            if let Err(error) = manager.truncate_messages(session_id, sequence) {
                tracing::warn!(
                    target: "zenix-chat",
                    %error,
                    "the replaced messages were not removed"
                );
            }
        }
        Err(error) => {
            tracing::warn!(target: "zenix-chat", %error, "the session store was not opened")
        }
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
            append_text(&mut message.entries, text);
            Folded::Message
        }
        DispatchStreamEvent::Thinking { text, .. } => {
            append_reasoning(&mut message.entries, text);
            Folded::Message
        }
        DispatchStreamEvent::ToolCall { id, name, args } => {
            message.entries.push(Entry::Step {
                call: ToolCallState {
                    id: id.clone(),
                    name: name.clone(),
                    args: args.clone(),
                    status: ToolCallStatus::Running,
                    result_summary: None,
                    started_at: now_ms,
                    completed_at: None,
                },
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
            for entry in message.entries.iter_mut().rev() {
                if let Entry::Step { call } = entry
                    && &call.id == id
                {
                    call.status = if *is_error {
                        ToolCallStatus::Failed
                    } else {
                        ToolCallStatus::Completed
                    };
                    call.result_summary = Some(summarize(content, 160));
                    call.completed_at = Some(now_ms);
                    break;
                }
            }
            Folded::Message
        }
        DispatchStreamEvent::RoundStart { .. } => {
            // A new round: the message keeps accumulating, and the reasoning
            // above it is already drawn. Its duration is measured by the view,
            // which is where the clock is. A round that starts after a tool
            // result is a new block of text — the model's own first round may
            // have ended mid-line — so the join is a paragraph break rather
            // than a run-on ("…now.## Heading"). The timeline keeps the same
            // rule: the text run after a step is its own entry.
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

/// Text keeps extending the run it belongs to; anything else between two text
/// tokens — a step, a thought — ends the run and starts a new one.
fn append_text(entries: &mut Vec<Entry>, text: &str) {
    if let Some(Entry::Text { text: current }) = entries.last_mut() {
        current.push_str(text);
        return;
    }
    entries.push(Entry::Text {
        text: text.to_owned(),
    });
}

fn append_reasoning(entries: &mut Vec<Entry>, text: &str) {
    if let Some(Entry::Reasoning { text: current, .. }) = entries.last_mut() {
        current.push_str(text);
        return;
    }
    entries.push(Entry::Reasoning {
        text: text.to_owned(),
        duration_ms: None,
    });
}

/// Stamp the reasoning run that just ended. The run is the last entry by
/// construction: this is called on the first non-thinking event after it.
fn close_reasoning(message: &mut ChatMessage, took_ms: u64) {
    if let Some(Entry::Reasoning { duration_ms, .. }) = message.entries.last_mut()
        && duration_ms.is_none()
    {
        *duration_ms = Some(took_ms);
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
    fn a_menu_is_capped_by_the_room_on_its_wider_side() {
        // A composer at the foot of a 900px window: the room is above it.
        let low = Bounds::new(
            gpui::point(px(0.0), px(780.0)),
            gpui::size(px(184.0), px(120.0)),
        );
        assert_eq!(panel_room(low, px(900.0)), px(772.0));
        // The same anchor near the top opens down, where the room is.
        let high = Bounds::new(
            gpui::point(px(0.0), px(40.0)),
            gpui::size(px(184.0), px(120.0)),
        );
        assert_eq!(panel_room(high, px(900.0)), px(732.0));
        // A window shorter than the chrome still yields the floor, never a
        // negative cap.
        let tight = Bounds::new(
            gpui::point(px(0.0), px(4.0)),
            gpui::size(px(184.0), px(300.0)),
        );
        assert_eq!(panel_room(tight, px(400.0)), px(160.0));
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
        assert_eq!(message.content, "The plan");
        assert_eq!(
            message.entries,
            vec![
                Entry::Reasoning {
                    text: "check dates".into(),
                    duration_ms: None
                },
                Entry::Text {
                    text: "The plan".into()
                },
            ]
        );
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
        let Entry::Step { call } = &message.entries[0] else {
            panic!("the call is the turn's only entry");
        };
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
        let Entry::Step { call } = &message.entries[0] else {
            panic!("the call is the turn's only entry");
        };
        assert_eq!(call.status, ToolCallStatus::Failed);
    }

    /// The timeline is the order things happened: a thought, the text that
    /// followed it, the step it led to, then the next round's thought and
    /// text. The old shape kept three piles (thoughts, steps, text) and could
    /// not say where a step sat between two paragraphs.
    #[test]
    fn a_turn_keeps_its_entries_in_arrival_order() {
        let mut message = a_placeholder();
        fold_event(
            &mut message,
            &DispatchStreamEvent::Thinking {
                round: 0,
                text: "look".into(),
            },
            1,
        );
        close_reasoning(&mut message, 500);
        fold_event(
            &mut message,
            &DispatchStreamEvent::Token("Reading.".into()),
            600,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolCall {
                id: "c1".into(),
                name: "read_output".into(),
                args: serde_json::json!({}),
            },
            700,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::ToolResult {
                id: "c1".into(),
                tool_name: "read_output".into(),
                content: "ok".into(),
                is_error: false,
            },
            800,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::Thinking {
                round: 1,
                text: "done".into(),
            },
            900,
        );
        fold_event(
            &mut message,
            &DispatchStreamEvent::Token("Done.".into()),
            950,
        );
        let kinds: Vec<&str> = message
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Reasoning { .. } => "reasoning",
                Entry::Step { .. } => "step",
                Entry::Text { .. } => "text",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["reasoning", "text", "step", "reasoning", "text"]
        );
        assert_eq!(
            message.entries[0],
            Entry::Reasoning {
                text: "look".into(),
                duration_ms: Some(500),
            }
        );
        // A run that already has its duration keeps it.
        close_reasoning(&mut message, 99);
        let Entry::Reasoning { duration_ms, .. } = &message.entries[0] else {
            panic!("the first entry is the thought");
        };
        assert_eq!(*duration_ms, Some(500));
    }

    /// Rows follow the entries: text ends a step run, so the steps on either
    /// side of a paragraph are drawn as separate runs.
    #[test]
    fn the_rows_follow_the_entries_and_text_breaks_a_run() {
        let mut message = a_placeholder();
        message.entries = vec![
            Entry::Reasoning {
                text: "think".into(),
                duration_ms: Some(900),
            },
            Entry::Text {
                text: "Reading.".into(),
            },
            Entry::Step {
                call: a_call(
                    "c1",
                    "send_text",
                    serde_json::json!({ "text": "ls" }),
                    ToolCallStatus::Completed,
                ),
            },
            Entry::Step {
                call: a_call(
                    "c2",
                    "send_text",
                    serde_json::json!({ "text": "pwd" }),
                    ToolCallStatus::Completed,
                ),
            },
            Entry::Text {
                text: "Done.".into(),
            },
            Entry::Step {
                call: a_call(
                    "c3",
                    "send_text",
                    serde_json::json!({ "text": "git status" }),
                    ToolCallStatus::Completed,
                ),
            },
        ];
        let rows = transcript_rows(&message, None, false, None, 0);
        assert_eq!(rows.len(), 5, "six entries, two of them one step run");
        match &rows[0] {
            TranscriptRow::Reasoning { took_ms, .. } => assert_eq!(*took_ms, Some(900)),
            _ => panic!("the thought comes first"),
        }
        match &rows[1] {
            TranscriptRow::Text { text, live } => {
                assert_eq!(text, "Reading.");
                assert!(!live);
            }
            _ => panic!("then the text"),
        }
        match &rows[2] {
            TranscriptRow::Steps(calls) => {
                assert_eq!(calls.len(), 2, "consecutive steps are one run");
            }
            _ => panic!("then the run"),
        }
        match &rows[3] {
            TranscriptRow::Text { text, .. } => assert_eq!(text, "Done."),
            _ => panic!("text ends the run"),
        }
        match &rows[4] {
            TranscriptRow::Steps(calls) => assert_eq!(calls.len(), 1),
            _ => panic!("a step after a paragraph is its own run"),
        }
    }

    /// The call the gate is asking about is drawn by the gate; the timeline
    /// leaves it out and keeps the rest of the run together.
    #[test]
    fn the_gate_s_call_leaves_its_run() {
        let mut message = a_placeholder();
        message.entries = vec![
            Entry::Step {
                call: a_call(
                    "c1",
                    "send_text",
                    serde_json::json!({}),
                    ToolCallStatus::Completed,
                ),
            },
            Entry::Step {
                call: a_call(
                    "c2",
                    "send_text",
                    serde_json::json!({}),
                    ToolCallStatus::Running,
                ),
            },
            Entry::Step {
                call: a_call(
                    "c3",
                    "send_text",
                    serde_json::json!({}),
                    ToolCallStatus::Completed,
                ),
            },
        ];
        let rows = transcript_rows(&message, Some("c2"), false, None, 0);
        match &rows[0] {
            TranscriptRow::Steps(calls) => {
                let ids: Vec<&str> = calls.iter().map(|call| call.id.as_str()).collect();
                assert_eq!(ids, vec!["c1", "c3"]);
            }
            _ => panic!("one run remains"),
        }
        // A run that was only the asked call leaves nothing to draw.
        let mut alone = a_placeholder();
        alone.entries = vec![Entry::Step {
            call: a_call(
                "c1",
                "send_text",
                serde_json::json!({}),
                ToolCallStatus::Running,
            ),
        }];
        assert!(transcript_rows(&alone, Some("c1"), false, None, 0).is_empty());
    }

    /// Only the last entry is still being written; an open thought shows the
    /// view's clock, and an older run keeps the duration it was stamped with.
    #[test]
    fn only_the_last_entry_is_live() {
        let mut message = a_placeholder();
        message.entries = vec![
            Entry::Text { text: "hi".into() },
            Entry::Reasoning {
                text: "late".into(),
                duration_ms: None,
            },
        ];
        let rows = transcript_rows(&message, None, true, Some(1_000), 4_500);
        match &rows[0] {
            TranscriptRow::Text { live, .. } => assert!(!live),
            _ => panic!("text first"),
        }
        match &rows[1] {
            TranscriptRow::Reasoning { live, took_ms, .. } => {
                assert!(*live);
                assert_eq!(*took_ms, Some(3_500));
            }
            _ => panic!("the open thought is last"),
        }
        // Once the turn is over the same rows are frozen, clock and all.
        let after = transcript_rows(&message, None, false, Some(1_000), 9_000);
        match &after[1] {
            TranscriptRow::Reasoning { live, took_ms, .. } => {
                assert!(!live);
                assert_eq!(*took_ms, None);
            }
            _ => panic!("the thought is still last"),
        }
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
                cached_tokens: 900,
            }),
            1,
        );
        assert_eq!(message.usage.map(|usage| usage.total_tokens()), Some(1500));
    }

    fn a_call(
        id: &str,
        name: &str,
        args: serde_json::Value,
        status: ToolCallStatus,
    ) -> ToolCallState {
        let finished = status != ToolCallStatus::Running;
        ToolCallState {
            id: id.to_owned(),
            name: name.to_owned(),
            args,
            status,
            result_summary: finished.then(|| "ok".to_owned()),
            started_at: 0,
            completed_at: finished.then_some(5),
        }
    }

    #[test]
    fn an_edit_puts_a_user_message_back_and_cuts_the_turn() {
        let mut messages = vec![
            message("s1", "user", "first".into(), 1),
            message("s1", "orchestrator", "answer".into(), 2),
            message("s1", "user", "second".into(), 3),
            message("s1", "orchestrator", "answer 2".into(), 4),
        ];
        assert_eq!(edit_target(&messages, 2).as_deref(), Some("second"));
        assert_eq!(
            edit_target(&messages, 1),
            None,
            "the model's words are not editable"
        );
        assert_eq!(edit_target(&messages, 9), None);

        cut_before(&mut messages, 2);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages.last().map(|m| m.content.as_str()), Some("answer"));
        assert_eq!(
            messages.last().map(|m| m.sequence + 1),
            Some(3),
            "the replacement continues the sequence of what is left"
        );

        cut_before(&mut messages, 0);
        assert!(
            messages.is_empty(),
            "editing the first message clears the turn"
        );
    }

    #[test]
    fn a_thought_row_names_its_length() {
        let (label, meta) = thought_words(false, Some(12_000));
        assert_eq!(label, t("ai.chat.thought"));
        let took = rust_i18n::t!("ai.chat.thought_took", seconds = 12).to_string();
        assert_eq!(meta.as_deref(), Some(took.as_str()));

        let (label, meta) = thought_words(false, Some(400));
        assert_eq!(label, t("ai.chat.thought"));
        assert_eq!(meta.as_deref(), Some(t("ai.chat.thought_moment").as_str()));

        let (label, meta) = thought_words(true, Some(5_000));
        assert_eq!(label, t("ai.chat.streaming"));
        assert!(
            meta.is_none(),
            "a live row says it is thinking, not how long"
        );
    }

    #[test]
    fn a_step_row_names_what_the_tool_did() {
        let read = a_call(
            "c1",
            "read_output",
            serde_json::json!({ "pane_id": "w1:p1", "lines": 20 }),
            ToolCallStatus::Completed,
        );
        let facts = step_facts(&read);
        assert_eq!(facts.family, StepFamily::Read);
        assert_eq!(facts.verb, Some(StepVerb::Read));
        assert_eq!(facts.target, "w1:p1");
        let lines = rust_i18n::t!("ai.chat.step.lines", count = 20).to_string();
        assert_eq!(facts.meta.as_deref(), Some(lines.as_str()));

        let send = a_call(
            "c2",
            "send_text",
            serde_json::json!({ "pane_id": "w1:p2", "text": "cargo test\n--all" }),
            ToolCallStatus::Completed,
        );
        let facts = step_facts(&send);
        assert_eq!(facts.family, StepFamily::Terminal);
        assert_eq!(facts.verb, Some(StepVerb::Send));
        assert_eq!(
            facts.target, "cargo test --all",
            "newlines fold to one line"
        );
        assert_eq!(facts.meta.as_deref(), Some("w1:p2"));
    }

    #[test]
    fn an_unknown_tool_keeps_its_wire_name_and_arguments() {
        let call = a_call(
            "c1",
            "plugin_thing",
            serde_json::json!({ "path": "x" }),
            ToolCallStatus::Completed,
        );
        let facts = step_facts(&call);
        assert_eq!(facts.verb, None);
        assert_eq!(facts.family, StepFamily::Manage);
        assert!(facts.target.contains("path"));
    }

    #[test]
    fn a_run_of_commands_folds_even_when_one_fails() {
        let calls = vec![
            a_call(
                "a",
                "send_text",
                serde_json::json!({ "text": "ls" }),
                ToolCallStatus::Completed,
            ),
            a_call(
                "b",
                "send_text",
                serde_json::json!({ "text": "cat nope" }),
                ToolCallStatus::Failed,
            ),
        ];
        let runs = step_runs(&calls);
        assert_eq!(runs.len(), 1);
        match &runs[0] {
            StepRun::Group { family, calls } => {
                assert_eq!(*family, StepFamily::Terminal);
                assert_eq!(calls.len(), 2);
            }
            other => panic!("expected one terminal group, got {other:?}"),
        }
    }

    #[test]
    fn a_read_run_with_a_failure_stays_visible() {
        let calls = vec![
            a_call(
                "a",
                "read_output",
                serde_json::json!({ "pane_id": "w1:p1" }),
                ToolCallStatus::Completed,
            ),
            a_call(
                "b",
                "read_output",
                serde_json::json!({ "pane_id": "w1:p1" }),
                ToolCallStatus::Failed,
            ),
            a_call(
                "c",
                "read_output",
                serde_json::json!({ "pane_id": "w1:p1" }),
                ToolCallStatus::Completed,
            ),
        ];
        let runs = step_runs(&calls);
        assert_eq!(runs.len(), 3, "a failed read is not folded away");
        assert!(runs.iter().all(|run| matches!(run, StepRun::One(_))));
    }

    #[test]
    fn finished_reads_fold_but_a_running_one_does_not() {
        let done = vec![
            a_call(
                "a",
                "read_output",
                serde_json::json!({ "pane_id": "w1:p1" }),
                ToolCallStatus::Completed,
            ),
            a_call(
                "b",
                "list_workers",
                serde_json::json!({}),
                ToolCallStatus::Completed,
            ),
        ];
        assert!(matches!(
            step_runs(&done).as_slice(),
            [StepRun::Group {
                family: StepFamily::Read,
                ..
            }]
        ));

        let live = vec![
            a_call(
                "a",
                "read_output",
                serde_json::json!({ "pane_id": "w1:p1" }),
                ToolCallStatus::Completed,
            ),
            a_call(
                "b",
                "read_output",
                serde_json::json!({ "pane_id": "w1:p1" }),
                ToolCallStatus::Running,
            ),
        ];
        assert_eq!(
            step_runs(&live).len(),
            2,
            "a running read stays its own row"
        );
    }

    #[test]
    fn a_single_call_keeps_its_own_row() {
        let calls = vec![a_call(
            "a",
            "send_text",
            serde_json::json!({ "text": "ls" }),
            ToolCallStatus::Completed,
        )];
        assert!(matches!(step_runs(&calls).as_slice(), [StepRun::One(_)]));
    }

    #[test]
    fn management_calls_break_a_run() {
        let calls = vec![
            a_call(
                "a",
                "send_text",
                serde_json::json!({ "text": "ls" }),
                ToolCallStatus::Completed,
            ),
            a_call(
                "b",
                "spawn_worker",
                serde_json::json!({ "agent": "claude", "instruction": "go" }),
                ToolCallStatus::Completed,
            ),
            a_call(
                "c",
                "send_text",
                serde_json::json!({ "text": "ls" }),
                ToolCallStatus::Completed,
            ),
        ];
        let runs = step_runs(&calls);
        assert_eq!(runs.len(), 3, "a spawn is never folded into the commands");
        assert!(runs.iter().all(|run| matches!(run, StepRun::One(_))));
        assert_eq!(step_facts(&calls[1]).verb, Some(StepVerb::Start));
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

    fn a_provider() -> ApiProviderConfig {
        ApiProviderConfig {
            name: "Work".into(),
            format: crate::api_provider::ApiFormat::OpenAI,
            base_url: "https://api.example.com".into(),
            api_key: "sk-test".into(),
            model: String::new(),
            thinking_level: ThinkingLevel::default(),
            url_suffix: "/v1/chat/completions".into(),
            enabled_agents: Vec::new(),
            models: Vec::new(),
        }
    }

    /// The gate starts by asking; the permission menu's modes carry distinct
    /// words, and the flag's reading follows it.
    #[test]
    fn the_permission_chip_starts_asking_and_the_menu_names_both_modes() {
        let mode = PermissionMode::default();
        assert_eq!(mode, PermissionMode::Ask);
        assert_eq!(mode.id(), "ask");
        assert_eq!(PermissionMode::Full.id(), "full");
        assert_eq!(PermissionMode::of(true), PermissionMode::Full);
        assert_eq!(PermissionMode::of(false), PermissionMode::Ask);
        assert_ne!(mode.label(), PermissionMode::Full.label());
        assert_ne!(mode.hint(), PermissionMode::Full.hint());
        assert_eq!(mode.hint(), t("ai.chat.permission_ask_hint"));
    }

    /// `Ctrl-T` walks all four levels and wraps, so one key reaches every one.
    #[test]
    fn ctrl_t_walks_every_level_and_wraps() {
        let mut level = ThinkingLevel::Low;
        let mut seen = vec![level];
        for _ in 1..ThinkingLevel::all().len() {
            level = next_level(level);
            seen.push(level);
        }
        assert_eq!(seen, ThinkingLevel::all().to_vec());
        assert_eq!(
            next_level(level),
            ThinkingLevel::Low,
            "the last level wraps to the first"
        );
    }

    /// The menu lists every model the provider offers; an empty label reads as
    /// the id and the selected model is the one marked.
    #[test]
    fn the_model_menu_lists_every_model_and_marks_the_chosen_one() {
        use crate::api_provider::ModelConfig;
        let mut provider = a_provider();
        provider.models = vec![
            ModelConfig {
                id: "gpt-4o".into(),
                label: String::new(),
                vision: true,
                context_window: None,
            },
            ModelConfig {
                id: "o3".into(),
                label: "O3 mini".into(),
                vision: false,
                context_window: None,
            },
        ];
        provider.model = "o3".into();
        let rows = model_rows(&provider);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "gpt-4o", "an empty label reads as the id");
        assert!(rows[0].vision);
        assert!(!rows[0].selected);
        assert_eq!(rows[1].label, "O3 mini");
        assert!(rows[1].selected);
        assert!(!rows[1].vision);
    }

    /// The chip names the model and the effort the next turn asks for, even
    /// when the provider's list is older than its selected id.
    #[test]
    fn the_model_chip_names_the_model_and_its_effort() {
        use crate::api_provider::ModelConfig;
        let mut provider = a_provider();
        provider.model = "gpt-4o".into();
        provider.models = vec![ModelConfig {
            id: "gpt-4o".into(),
            label: "GPT-4o".into(),
            vision: false,
            context_window: None,
        }];
        provider.thinking_level = ThinkingLevel::High;
        assert_eq!(model_name(&provider), "GPT-4o");
        assert_eq!(level_label(&provider), t("ai.effort.high"));

        provider.models.clear();
        assert_eq!(model_name(&provider), "gpt-4o");
    }
}
