//! role="error" rendering — danger-styled alert card with a Retry button.

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Styled as _,
    div, hsla, px,
};
use gpui_component::{Theme, h_flex, v_flex};

use crate::ai::session::ChatMessage;

use super::state::{AiChatAction, AiChatActionHandler};

pub(super) fn render(
    msg: &ChatMessage,
    theme: &Theme,
    on_action: AiChatActionHandler,
    ix: usize,
) -> AnyElement {
    let th = theme.clone();
    let danger = th.danger;
    let danger_dim = hsla(danger.h, danger.s, danger.l, 0.08);
    let danger_border = hsla(danger.h, danger.s, 0.5, 0.5);
    let fg = th.foreground;
    let mu = th.muted_foreground;
    let border = th.border;
    let accent = th.accent;
    let on_retry = on_action.clone();
    let on_settings = on_action.clone();

    v_flex()
        .w_full()
        .py(px(8.))
        .child(
            v_flex()
                .w_full()
                .gap(px(6.))
                .p(px(10.))
                .rounded_md()
                .bg(danger_dim)
                .border_l_2()
                .border_y_1()
                .border_r_1()
                .border_color(danger_border)
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(
                            div()
                                .w(px(16.))
                                .h(px(16.))
                                .rounded_full()
                                .bg(danger)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    super::txt(px(10.))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(gpui::white())
                                        .child("!"),
                                ),
                        )
                        .child(
                            super::txt(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(danger)
                                .child("Error"),
                        ),
                )
                .child(
                    super::txt(px(12.))
                        .text_color(fg)
                        .pl(px(24.))
                        .child(msg.content.clone()),
                )
                .child(
                    h_flex()
                        .gap(px(8.))
                        .pl(px(24.))
                        .child(primary_button("重试", on_retry, ix, accent))
                        .child(ghost_button("打开 Settings", on_settings, border, mu)),
                ),
        )
        .into_any_element()
}

fn primary_button(
    label: &str,
    on_action: AiChatActionHandler,
    ix: usize,
    accent: gpui::Hsla,
) -> impl IntoElement {
    div()
        .id(("error-retry", ix))
        .px(px(10.))
        .py(px(4.))
        .rounded_md()
        .bg(accent)
        .text_size(px(11.))
        .text_color(gpui::white())
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_action(AiChatAction::RetryLast, window, cx)
        })
        .child(label.to_string())
}

fn ghost_button(
    label: &str,
    on_action: AiChatActionHandler,
    border: gpui::Hsla,
    fg: gpui::Hsla,
) -> impl IntoElement {
    div()
        .px(px(10.))
        .py(px(4.))
        .rounded_md()
        .border_1()
        .border_color(border)
        .text_size(px(11.))
        .text_color(fg)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_action(AiChatAction::OpenSettings, window, cx)
        })
        .child(label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::Theme;

    fn test_theme() -> Theme {
        Theme::default()
    }

    fn noop_handler() -> AiChatActionHandler {
        std::rc::Rc::new(|_, _, _| {})
    }

    #[test]
    fn test_render_error_card() {
        let msg = ChatMessage {
            id: 1,
            session_id: "s1".into(),
            role: "error".into(),
            content: "API key invalid".into(),
            agent_activities: None,
            timestamp: "now".into(),
            sequence: 0,
        };
        let _element = render(&msg, &test_theme(), noop_handler(), 0);
    }
}
