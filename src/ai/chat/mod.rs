//! AI Chat panel UI — message list, input bar, header, history panel,
//! agent blocks, and agent monitoring.
//!
//! Renders as an inline panel in the right sidebar area. Uses gpui-component
//! widgets (Button, Input, scroll, etc.) for consistent theming.
//!
//! Design principles:
//! - Single column layout: header → messages (flex_grow) → input
//! - No redundant toolbars — every button has a clear purpose
//! - Empty state shows centered welcome + input at bottom
//! - Model selector is a compact dropdown in the input row
//!
//! Module layout (see design.md §1.1):
//! - `state`       — `AiChatPanelData` and supporting types
//! - `messages`    — single-message rendering + streaming indicator
//! - `input`       — input bar (multi-line in Phase 4)
//! - `header`      — header + dropdown + history + agent monitor + empty state
//! - `agent_block` — inline agent output cards + monitor entry types

mod agent_block;
mod drawer;
mod error_card;
mod header;
mod input;
mod mention_popup;
mod messages;
pub mod resize;
mod state;
mod tool_card;

// Re-export public API consumed by `app.rs` and `ui/sidebar.rs`.
pub use agent_block::{AgentBlock, AgentMonitorEntry};
pub use state::{
    AiChatAction, AiChatActionHandler, AiChatPanelData, ConfigDropdownTarget, DrawerKind,
    WorkspaceOption,
};

use gpui::{
    Entity, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::{Theme, input::InputState, v_flex};

use crate::ai::workers::WorkerRegistry;


// ═══════════════════════════════════════════════════════════════════════
// Main render entry
// ═══════════════════════════════════════════════════════════════════════

/// Render the AI chat panel as a GPUI element.
///
/// Layout: header → (overlays) → messages/input → no bottom toolbar.
/// The panel is a single column with messages taking all available space
/// and the input pinned to the bottom.
pub fn render_ai_chat_panel(
    data: &AiChatPanelData,
    theme: &Theme,
    input_entity: Option<&Entity<InputState>>,
    on_action: state::AiChatActionHandler,
    worker_registry: WorkerRegistry,
) -> impl IntoElement {
    let th = theme.clone();
    let bg = th.background;
    let border = th.border;

    let has_messages = !data.messages.is_empty();

    let panel_width = data
        .ai_panel_width
        .clamp(resize::MIN_PANEL_WIDTH, resize::MAX_PANEL_WIDTH);
    let panel_key_action = on_action.clone();
    let mut panel = div()
        .id("ai-chat-panel")
        .relative()
        .w(px(panel_width))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .bg(bg)
        .border_l_1()
        .border_color(border)
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            let key = &event.keystroke.key;
            let modifiers = &event.keystroke.modifiers;
            if key.eq_ignore_ascii_case("escape") {
                panel_key_action(AiChatAction::Escape, window, cx);
                cx.stop_propagation();
            } else if key.eq_ignore_ascii_case("l") && modifiers.control {
                panel_key_action(AiChatAction::ClearChat, window, cx);
                cx.stop_propagation();
            }
        });

    panel = panel.child(header::render_header(data, theme, on_action.clone()));

    let content: gpui::AnyElement = if has_messages {
        let mut msg_area = v_flex().flex_grow(1.0).min_h(px(200.)).px(px(4.));

        if data.scrolled_off_count > 0 {
            msg_area = msg_area.child(render_scrolled_off_banner(data.scrolled_off_count, theme));
        }

        let messages = data.messages.clone();
        let list_state = data.list_state.clone();
        let msg_count = messages.len();
        let list_count = list_state.item_count();
        if list_count != msg_count {
            tracing::warn!(
                target: "zenixai",
                list_count,
                msg_count,
                "list count mismatch — resetting"
            );
            list_state.reset(msg_count);
        }
        let theme_for_list = theme.clone();
        let streaming_msg_idx = data.streaming_msg_idx;
        let is_streaming = data.is_streaming;
        let hovered_msg_id = data.hovered_msg_id;
        let collapsed_tool_ids = data.collapsed_tool_ids.clone();
        let collapsed_reasoning_ids = data.collapsed_reasoning_ids.clone();
        let on_action_for_hover = on_action.clone();
        let on_action_for_tool = on_action.clone();
        let on_action_for_reasoning = on_action.clone();
        let on_action_for_retry = on_action.clone();
        msg_area = msg_area.child(
            gpui::list(list_state, move |ix, _window, _cx| {
                if ix >= msg_count {
                    return div().into_any_element();
                }
                let msg = &messages[ix];
                let is_streaming_tail = is_streaming && streaming_msg_idx == Some(ix);
                let reasoning_collapsed = collapsed_reasoning_ids.contains(&msg.id);
                let reasoning_toggle = on_action_for_reasoning.clone();
                let msg_id_for_reasoning = msg.id;
                let rctx = messages::RenderCtx {
                    panel_width,
                    is_streaming_tail,
                    is_hovered: hovered_msg_id == Some(msg.id),
                    reasoning_collapsed,
                    on_toggle_reasoning: Some(std::rc::Rc::new(move |window, cx| {
                        (reasoning_toggle)(
                            state::AiChatAction::ToggleReasoningCollapse(msg_id_for_reasoning),
                            window,
                            cx,
                        );
                    })),
                };
                let msg_id = msg.id;
                let inner = match msg.role.as_str() {
                    "tool" => {
                        let prev_is_tool = ix > 0 && messages[ix - 1].role == "tool";
                        let group_count = if !prev_is_tool {
                            let mut count = 0usize;
                            let mut j = ix;
                            while j < msg_count && messages[j].role == "tool" {
                                count += 1;
                                j += 1;
                            }
                            if count > 1 { Some(count) } else { None }
                        } else {
                            None
                        };
                        let tool_collapsed = collapsed_tool_ids.contains(&msg_id);
                        let tool_toggle = on_action_for_tool.clone();
                        tool_card::render(
                            msg,
                            &theme_for_list,
                            group_count,
                            tool_collapsed,
                            std::rc::Rc::new(move |window, cx| {
                                (tool_toggle)(
                                    state::AiChatAction::ToggleToolCollapse(msg_id),
                                    window,
                                    cx,
                                );
                            }),
                        )
                    }
                    "error" => {
                        error_card::render(msg, &theme_for_list, on_action_for_retry.clone(), ix)
                    }
                    _ => messages::render_message(msg, &theme_for_list, &rctx),
                };
                let on_action_hover = on_action_for_hover.clone();
                div()
                    .id(("msg-row", ix))
                    .w_full()
                    .on_hover(move |hovered, w, cx| {
                        if *hovered {
                            on_action_hover(
                                state::AiChatAction::SetHoveredMessage(Some(msg_id)),
                                w,
                                cx,
                            );
                        } else {
                            on_action_hover(state::AiChatAction::SetHoveredMessage(None), w, cx);
                        }
                    })
                    .child(inner)
                    .into_any_element()
            })
            .px(px(0.))
            .flex_grow(1.0)
            .min_h(px(100.)),
        );

        msg_area.into_any_element()
    } else {
        header::render_empty_state(data, theme, on_action.clone())
    };
    panel = panel.child(v_flex().flex_grow(1.0).min_h_0().child(content));

    panel = panel.child(resize::render_input_resize_handle(
        theme,
        data.input_height,
        on_action.clone(),
    ));
    let workers_for_input = worker_registry.active_workers();
    panel = panel.child(input::render_input_bar(
        data,
        theme,
        input_entity,
        on_action.clone(),
        &workers_for_input,
    ));

    if data.header_menu_open {
        panel = panel.child(gpui::deferred(header::render_header_menu(
            data,
            theme,
            on_action.clone(),
        )));
    }

    if let Some(kind) = data.drawer {
        panel = panel.child(gpui::deferred(drawer::render_drawer_overlay(
            kind,
            data,
            theme,
            on_action.clone(),
        )));
    }

    panel = panel.child(resize::render_resize_handle(theme));

    panel
}

fn render_scrolled_off_banner(count: usize, theme: &Theme) -> impl IntoElement {
    let mu = theme.muted_foreground;
    div()
        .w_full()
        .h(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.muted)
        .text_size(px(10.))
        .text_color(mu)
        .child(format!("{} messages hidden above", count))
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Helper that returns a `Div` with the given text size pre-applied.
///
/// This is `pub(super)`-visible to descendant modules via `super::txt(...)`.
fn txt(size: gpui::Pixels) -> gpui::Div {
    div().text_size(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::chat::state::AiChatActionHandler;
    use gpui_component::Theme;

    fn test_theme() -> Theme {
        Theme::default()
    }

    fn noop_handler() -> AiChatActionHandler {
        std::rc::Rc::new(|_, _, _| {})
    }

    #[test]
    fn test_render_ai_chat_panel_returns_element() {
        let data = AiChatPanelData::default();
        let theme = test_theme();
        let registry = crate::ai::workers::WorkerRegistry::new();
        let _element = render_ai_chat_panel(&data, &theme, None, noop_handler(), registry);
    }
}
