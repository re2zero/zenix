//! AI Chat panel UI — message list, input bar, bottom bar.
//!
//! Renders as an inline panel (same position as SystemInfo) in the right
//! sidebar area. Users type messages, the orchestrator engine streams replies.

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    Window, div, px,
};
use gpui_component::{
    Theme, button::{Button, ButtonVariants as _}, h_flex, scroll::ScrollableElement as _, v_flex,
};

use crate::ai::session::ChatMessage;
use crate::i18n::t;

/// Width of the AI chat panel (wider than SystemInfo for chat readability).
pub const AI_PANEL_WIDTH: f32 = 400.0;

/// Maximum number of messages kept in the local display buffer.
const MAX_DISPLAY_MESSAGES: usize = 200;

/// AI chat panel data (state only, no GPUI rendering logic).
///
/// The panel is rendered by `render_ai_chat_panel()` which takes this data
/// plus the theme and produces an `IntoElement`.
pub struct AiChatPanelData {
    /// Display messages (user + orchestrator).
    pub messages: Vec<ChatMessage>,
    /// Whether the orchestrator is currently streaming a reply.
    pub is_streaming: bool,
    /// Input text value.
    pub input_value: String,
}

impl AiChatPanelData {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            is_streaming: false,
            input_value: String::new(),
        }
    }

    /// Append a message to the display buffer.
    pub fn append_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        if self.messages.len() > MAX_DISPLAY_MESSAGES {
            self.messages = self.messages.split_off(self.messages.len() - MAX_DISPLAY_MESSAGES);
        }
    }

    /// Clear all messages.
    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }

    /// Set streaming state.
    pub fn set_streaming(&mut self, streaming: bool) {
        self.is_streaming = streaming;
    }

    /// Set input text.
    pub fn set_input(&mut self, value: String) {
        self.input_value = value;
    }
}

// ── Render ──────────────────────────────────────────────────────────────

/// Render the AI chat panel as a GPUI element.
pub fn render_ai_chat_panel(
    data: &AiChatPanelData,
    theme: &Theme,
) -> impl IntoElement {
    let message_elements: Vec<AnyElement> = data
        .messages
        .iter()
        .map(|msg| render_message(msg, theme))
        .collect();

    let mut panel = h_flex()
        .w(px(AI_PANEL_WIDTH))
        .h_full()
        .bg(theme.background)
        .border_l_1()
        .border_color(theme.border)
        .flex_col()
        .overflow_y_scrollbar()
        // Message area — scrollable
        .child(
            div()
                .flex_grow(1.0)
                .min_h_0()
                .overflow_y_scrollbar()
                .p(px(12.))
                .gap(px(8.))
                .children(message_elements),
        );

    // Streaming indicator (conditional — not on Scrollable)
    if data.is_streaming {
        panel = panel.child(
            h_flex()
                .items_center()
                .gap(px(6.))
                .p(px(8.))
                .child(
                    div()
                        .w(px(8.))
                        .h(px(8.))
                        .rounded_full()
                        .bg(gpui::hsla(0.55, 0.8, 0.6, 1.0)),
                )
                .child(
                    gpui::div()
                        .text_size(px(11.))
                        .text_color(gpui::rgba(0x888888FF))
                        .child(t("ai.chat.streaming")),
                ),
        );
    }

    // Input area
    panel = panel.child(
        h_flex()
            .gap(px(8.))
            .p(px(8.))
            .border_t_1()
            .border_color(theme.border)
            .child(
                gpui::div()
                    .flex_grow(1.0)
                    .child(
                        gpui::div()
                            .text_size(px(13.))
                            .child(data.input_value.clone()),
                    ),
            )
            .child(
                Button::new("ai-chat-send")
                    .ghost()
                    .label(if data.is_streaming {
                        t("ai.chat.stop")
                    } else {
                        t("ai.chat.send")
                    })
                    .w(px(60.))
                    .primary(),
            ),
    );

    // Bottom bar
    panel = panel.child(
        h_flex()
            .justify_between()
            .items_center()
            .px(px(8.))
            .py(px(4.))
            .border_t_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap(px(4.))
                    .child(
                        Button::new("ai-chat-new")
                            .ghost()
                            .label(t("ai.chat.new_session"))
                            .w(px(72.)),
                    )
                    .child(
                        Button::new("ai-chat-history")
                            .ghost()
                            .label(t("ai.chat.history"))
                            .w(px(72.)),
                    ),
            )
            .child(
                txt(px(10.))
                    .text_color(gpui::rgba(0x888888FF))
                    .child(format!(
                        "{} {}",
                        data.messages.len(),
                        t("ai.chat.messages")
                    )),
            ),
    );

    panel
}

// ── Message rendering ───────────────────────────────────────────────────

fn render_message(msg: &ChatMessage, theme: &Theme) -> AnyElement {
    let is_user = msg.role == "user";
    let content = msg.content.clone();
    let time = msg.timestamp.clone();

    let bubble = if is_user {
        // User message — right-aligned, primary color
        h_flex()
            .justify_end()
            .max_w(px(320.))
            .child(
                div()
                    .rounded_lg()
                    .rounded_bl_md()
                    .bg(theme.primary.opacity(0.2))
                    .p(px(10.))
                    .pr(px(14.))
                    .child(
                        gpui::div()
                            .text_size(px(13.))
                            .text_color(theme.foreground)
                            .child(content),
                    ),
            )
    } else {
        // Orchestrator message — left-aligned
        h_flex()
            .justify_start()
            .max_w(px(320.))
            .child(
                div()
                    .rounded_lg()
                    .rounded_br_md()
                    .bg(theme.muted_foreground.opacity(0.08))
                    .p(px(10.))
                    .pr(px(14.))
                    .child(
                        gpui::div()
                            .text_size(px(13.))
                            .text_color(theme.foreground)
                            .child(content),
                    ),
            )
    };

    v_flex()
        .w_full()
        .child(bubble.into_any_element())
        .child(
            txt(px(9.))
                .text_color(gpui::rgba(0x888888FF))
                .px(px(4.))
                .child(time),
        )
        .into_any_element()
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn txt(size: gpui::Pixels) -> gpui::Div {
    div().text_size(size)
}
