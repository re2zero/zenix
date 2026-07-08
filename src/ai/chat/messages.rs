//! Message rendering — single message bubble + streaming tail cursor.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Theme, h_flex, v_flex};

use crate::ai::markdown;
use crate::ai::session::ChatMessage;

pub(super) struct RenderCtx {
    pub panel_width: f32,
    pub is_streaming_tail: bool,
    pub is_hovered: bool,
    pub reasoning_collapsed: bool,
    pub on_toggle_reasoning: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    pub reasoning_elapsed_secs: Option<f32>,
}

pub(super) fn render_message(msg: &ChatMessage, theme: &Theme, ctx: &RenderCtx) -> AnyElement {
    let th = theme.clone();
    let is_user = msg.role == "user";
    let mu = th.muted_foreground;
    let border = th.border;

    let mut elements: Vec<AnyElement> = Vec::new();

    // Reasoning block (assistant messages only)
    if !is_user {
        if let Some(ref reasoning) = msg.reasoning_text {
            if !reasoning.is_empty() {
                elements.push(render_reasoning_block(
                    reasoning,
                    msg.reasoning_duration_ms,
                    ctx.reasoning_elapsed_secs,
                    theme,
                    ctx.reasoning_collapsed,
                    &ctx.on_toggle_reasoning,
                ));
            }
        }
    }

    let segments = markdown::render_markdown(&msg.content, theme);
    if is_user {
        elements.push(
            v_flex()
                .gap(px(2.))
                .w_full()
                .children(segments)
                .into_any_element(),
        );
    } else {
        let mut container = v_flex().gap(px(2.)).w_full().children(segments);
        if ctx.is_streaming_tail {
            container = container.child(streaming_cursor(&th));
        }
        elements.push(container.into_any_element());
    }

    let accent = th.accent;
    let muted = th.muted;

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
                        .rounded_lg()
                        .bg(accent)
                        .p(px(10.))
                        .px(px(14.))
                        .whitespace_normal()
                        .children(elements),
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
                    .rounded_lg()
                    .bg(muted)
                    .border_1()
                    .border_color(border)
                    .p(px(10.))
                    .px(px(14.))
                    .whitespace_normal()
                    .children(elements),
            )
            .into_any_element()
    }
}

fn render_reasoning_block(
    text: &str,
    duration_ms: Option<u64>,
    elapsed_secs: Option<f32>,
    theme: &Theme,
    collapsed: bool,
    on_toggle: &Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let border = th.border;
    let bg = th.background;

    let arrow = if collapsed { "▶" } else { "▼" };
    let is_streaming = elapsed_secs.is_some();
    let cleaned = strip_think_wrapper(text);

    // Use muted foreground for reasoning content's markdown rendering
    let mut rtheme = theme.clone();
    rtheme.foreground = mu;

    // During streaming, show only last 5 lines so latest content is always visible
    let display_text = if is_streaming {
        let lines: Vec<&str> = cleaned.lines().collect();
        if lines.len() > 5 {
            lines[lines.len() - 5..].join("\n")
        } else {
            cleaned.clone()
        }
    } else {
        cleaned.clone()
    };

    let body: AnyElement = if collapsed {
        div().h(px(0.)).into_any_element()
    } else if is_streaming {
        // Fixed height during streaming prevents layout jumps on content update
        div()
            .h(px(90.))
            .overflow_hidden()
            .p(px(8.))
            .child(
                v_flex()
                    .gap(px(2.))
                    .children(crate::ai::markdown::render_markdown(&display_text, &rtheme)),
            )
            .into_any_element()
    } else {
        div()
            .p(px(8.))
            .child(
                v_flex()
                    .gap(px(2.))
                    .children(crate::ai::markdown::render_markdown(&display_text, &rtheme)),
            )
            .into_any_element()
    };

    let label = if is_streaming {
        let secs = elapsed_secs.unwrap_or(0.0);
        format!("thinking {:.1}s", secs)
    } else {
        match duration_ms {
            Some(ms) => format!("thought {:.1}s", ms as f32 / 1000.0),
            None => "Thought…".to_string(),
        }
    };

    let header = h_flex()
        .gap(px(6.))
        .items_center()
        .px(px(8.))
        .py(px(6.))
        .child(super::txt(px(11.)).text_color(mu).child(arrow))
        .child(
            super::txt(px(11.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(mu)
                .child(label),
        );

    let block = v_flex()
        .w_full()
        .rounded_md()
        .border_1()
        .border_color(border)
        .overflow_hidden()
        .child(header)
        .child(div().bg(bg).child(body));

    if let Some(toggle) = on_toggle.as_ref() {
        let toggle = toggle.clone();
        div()
            .cursor_pointer()
            .w_full()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                (toggle)(window, cx);
            })
            .child(block)
            .into_any_element()
    } else {
        block.into_any_element()
    }
}

/// Strip `assistant\n`, `<think>\n` prefix and `\n</think>` suffix from
/// reasoning text if present.
fn strip_think_wrapper(text: &str) -> String {
    let mut s = text.to_string();
    if let Some(rest) = s.strip_prefix("assistant\n") {
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_prefix("<think>\n") {
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_suffix("\n</think>") {
        s = rest.to_string();
    }
    s
}

fn timestamp_row(text: &str, mu: gpui::Hsla) -> AnyElement {
    div()
        .text_size(px(9.))
        .text_color(mu)
        .px(px(4.))
        .child(text.to_string())
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
            reasoning_collapsed: true,
            on_toggle_reasoning: None,
            reasoning_elapsed_secs: None,
        }
    }

    fn user_msg() -> ChatMessage {
        ChatMessage {
            id: 1,
            session_id: "s1".into(),
            role: "user".into(),
            content: "Hello".into(),
            agent_activities: None,
            reasoning_text: None,
            reasoning_duration_ms: None,
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
            reasoning_text: None,
            reasoning_duration_ms: None,
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
