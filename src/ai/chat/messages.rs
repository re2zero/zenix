//! Message rendering — single message bubble + streaming tail cursor.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, IntoElement, ParentElement as _, Styled as _, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Theme, h_flex, v_flex};

use crate::ai::markdown;
use crate::ai::session::ChatMessage;

pub(super) struct RenderCtx {
    pub panel_width: f32,
    pub is_streaming_tail: bool,
    pub is_hovered: bool,
}

pub(super) fn render_message(msg: &ChatMessage, theme: &Theme, ctx: &RenderCtx) -> AnyElement {
    let th = theme.clone();
    let is_user = msg.role == "user";

    let segments = markdown::render_markdown(&msg.content, theme);
    let body: AnyElement = if is_user {
        v_flex().gap(px(2.)).children(segments).into_any_element()
    } else {
        let mut container = v_flex().gap(px(2.)).children(segments);
        if ctx.is_streaming_tail {
            container = container.child(streaming_cursor(&th));
        }
        container.into_any_element()
    };

    let timestamp = msg.timestamp.clone();
    let accent = th.accent;
    let muted = th.muted;
    let mu = th.muted_foreground;
    let border = th.border;

    if is_user {
        let max_w = (ctx.panel_width * 0.88).clamp(120.0, 480.0);
        v_flex()
            .w_full()
            .py(px(8.))
            .items_end()
            .child(
                h_flex().w_full().justify_end().child(
                    div()
                        .max_w(px(max_w))
                        .min_w_0()
                        .overflow_hidden()
                        .rounded_lg()
                        .bg(accent)
                        .p(px(10.))
                        .px(px(14.))
                        .child(body),
                ),
            )
            .into_any_element()
    } else {
        v_flex()
            .w_full()
            .py(px(8.))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .rounded_lg()
                    .bg(muted)
                    .border_1()
                    .border_color(border)
                    .p(px(10.))
                    .px(px(14.))
                    .child(body),
            )
            .into_any_element()
    }
}

fn timestamp_row(text: &str, mu: gpui::Hsla) -> AnyElement {
    div()
        .text_size(px(9.))
        .text_color(mu)
        .px(px(4.))
        .child(text.to_string())
        .into_any_element()
}

fn render_avatar_z(theme: &Theme) -> AnyElement {
    div()
        .w(px(22.))
        .h(px(22.))
        .mt(px(2.))
        .rounded_full()
        .bg(theme.muted)
        .border_1()
        .border_color(theme.border)
        .flex()
        .items_center()
        .justify_center()
        .child(
            super::txt(px(11.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(theme.accent)
                .child("Z"),
        )
        .into_any_element()
}

fn streaming_cursor(theme: &Theme) -> AnyElement {
    div()
        .mt(px(2.))
        .w(px(8.))
        .h(px(14.))
        .rounded_sm()
        .bg(theme.accent)
        .with_animation(
            "streaming-cursor",
            Animation::new(Duration::from_millis(800))
                .repeat()
                .with_easing(gpui::pulsating_between(0.2, 1.0)),
            |this, delta| this.opacity(delta),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::session::ChatMessage;
    use gpui_component::Theme;

    fn test_theme() -> Theme {
        Theme::default()
    }

    fn ctx(panel_width: f32, tail: bool, hovered: bool) -> RenderCtx {
        RenderCtx {
            panel_width,
            is_streaming_tail: tail,
            is_hovered: hovered,
        }
    }

    fn user_msg() -> ChatMessage {
        ChatMessage {
            id: 1,
            session_id: "s1".into(),
            role: "user".into(),
            content: "Hello".into(),
            agent_activities: None,
            timestamp: "now".into(),
            sequence: 0,
        }
    }

    fn assistant_msg() -> ChatMessage {
        ChatMessage {
            id: 2,
            session_id: "s1".into(),
            role: "assistant".into(),
            content: "Hi there!".into(),
            agent_activities: None,
            timestamp: "now".into(),
            sequence: 1,
        }
    }

    #[test]
    fn test_render_message_user() {
        let _element = render_message(&user_msg(), &test_theme(), &ctx(420.0, false, false));
    }

    #[test]
    fn test_render_message_assistant() {
        let _element = render_message(&assistant_msg(), &test_theme(), &ctx(420.0, false, false));
    }

    #[test]
    fn test_render_message_assistant_streaming_tail() {
        let _element = render_message(&assistant_msg(), &test_theme(), &ctx(420.0, true, false));
    }

    #[test]
    fn test_render_message_user_hover_shows_timestamp() {
        let _element = render_message(&user_msg(), &test_theme(), &ctx(420.0, false, true));
    }

    #[test]
    fn test_user_max_w_clamps() {
        let cases: &[(f32, f32)] = &[
            (360.0, 316.8),
            (420.0, 369.6),
            (480.0, 422.4),
            (600.0, 480.0),
            (720.0, 480.0),
            (100.0, 120.0),
        ];
        for &(panel, expected) in cases {
            let actual = (panel * 0.88).clamp(120.0, 480.0);
            assert_eq!(actual, expected, "panel_width={panel}");
        }
    }
}
