//! role="tool" rendering — inline collapsible agent output card.
//!
//! Parses the `agent_activities` JSON (see design.md §2.1) stored on the
//! ChatMessage and renders a foldable card showing agent_name / status /
//! summary. Reuses the same visual language as `agent_block.rs`.

use std::rc::Rc;

use crate::ai::markdown;
use crate::ai::session::ChatMessage;
use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Theme, h_flex, v_flex};

struct ToolMeta {
    agent_name: String,
    status: String,
    summary: Option<String>,
    is_running: bool,
    #[allow(dead_code)]
    group_id: Option<i64>,
}

fn parse_meta(msg: &ChatMessage) -> ToolMeta {
    let raw = msg.agent_activities.as_deref().unwrap_or("{}");
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(v) => ToolMeta {
            agent_name: v
                .get("agent_name")
                .and_then(|n| n.as_str())
                .unwrap_or("agent")
                .to_string(),
            status: v
                .get("status")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string(),
            summary: v.get("summary").and_then(|n| n.as_str()).map(String::from),
            is_running: v
                .get("is_running")
                .and_then(|n| n.as_bool())
                .unwrap_or(false),
            group_id: v.get("group_id").and_then(|n| n.as_i64()),
        },
        Err(_) => ToolMeta {
            agent_name: "agent".into(),
            status: "unknown".into(),
            summary: None,
            is_running: false,
            group_id: None,
        },
    }
}

pub(super) fn render(
    msg: &ChatMessage,
    theme: &Theme,
    group_count: Option<usize>,
    collapsed: bool,
    on_toggle: Rc<dyn Fn(&mut Window, &mut App) + 'static>,
) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let fg = th.foreground;
    let border = th.border;
    let bg = th.background;
    let meta = parse_meta(msg);

    let (status_color, status_icon) = match meta.status.as_str() {
        "completed" | "success" | "done" => (th.accent, "✓"),
        "failed" | "error" => (gpui::hsla(0.0, 0.8, 0.55, 1.0), "✕"),
        "running" | "working" => (gpui::hsla(0.55, 0.8, 0.6, 1.0), "⟳"),
        _ => (mu, "○"),
    };

    let label = if meta.is_running {
        "running".to_string()
    } else {
        meta.status.clone()
    };

    let header = h_flex()
        .justify_between()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .py(px(8.))
        .rounded_md()
        .bg(th.muted)
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    super::txt(px(12.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(meta.agent_name.clone()),
                )
                .child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .child(
                            super::txt(px(11.))
                                .text_color(status_color)
                                .child(status_icon),
                        )
                        .child(super::txt(px(11.)).text_color(mu).child(label)),
                )
                .when_some(group_count, |this, count| {
                    this.child(
                        super::txt(px(10.))
                            .text_color(mu)
                            .px(px(6.))
                            .py(px(1.))
                            .rounded_sm()
                            .bg(bg)
                            .child(format!("{} calls", count)),
                    )
                }),
        )
        .child(
            super::txt(px(10.))
                .text_color(mu)
                .child(if collapsed { "▶" } else { "▼" }),
        );

    let body: AnyElement = if collapsed {
        div().into_any_element()
    } else if msg.content.trim().is_empty() {
        let summary = meta.summary.unwrap_or_else(|| meta.status.clone());
        div()
            .p(px(10.))
            .text_size(px(11.))
            .text_color(mu)
            .child(summary)
            .into_any_element()
    } else {
        let segments = markdown::render_markdown(&msg.content, theme);
        if segments.is_empty() {
            div()
                .p(px(10.))
                .text_size(px(12.))
                .text_color(mu)
                .child(msg.content.clone())
                .into_any_element()
        } else {
            v_flex()
                .p(px(10.))
                .gap(px(4.))
                .children(segments)
                .into_any_element()
        }
    };

    v_flex()
        .w_full()
        .py(px(6.))
        .child(
            v_flex()
                .w_full()
                .rounded_md()
                .border_1()
                .border_color(border)
                .overflow_hidden()
                .child(
            div()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    (on_toggle)(window, cx);
                })
                .child(header),
        )
        .child(body),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{App, Window};
    use gpui_component::Theme;

    fn test_theme() -> Theme {
        Theme::default()
    }

    fn noop_toggle() -> Rc<dyn Fn(&mut Window, &mut App) + 'static> {
        Rc::new(|_, _| {})
    }

    fn tool_msg(activities: Option<&str>) -> ChatMessage {
        ChatMessage {
            id: 1,
            session_id: "s-test".into(),
            role: "tool".into(),
            content: "tool output".into(),
            agent_activities: activities.map(|a| a.to_string()),
            reasoning_text: None,
            reasoning_duration_ms: None,
            timestamp: "2024-01-01T00:00:00Z".into(),
            sequence: 0,
        }
    }

    #[test]
    fn test_render_tool_card_with_meta() {
        let activities = r#"{"agent_name":"claude","status":"completed","summary":"done","is_running":false,"group_id":0}"#;
        let _element = render(&tool_msg(Some(activities)), &test_theme(), None, false, noop_toggle());
    }

    #[test]
    fn test_render_tool_card_no_activities() {
        let _element = render(&tool_msg(None), &test_theme(), None, false, noop_toggle());
    }

    #[test]
    fn test_render_tool_card_running() {
        let activities = r#"{"agent_name":"claude","status":"running","is_running":true}"#;
        let _element = render(&tool_msg(Some(activities)), &test_theme(), None, false, noop_toggle());
    }

    #[test]
    fn test_render_tool_card_empty_content() {
        let activities = r#"{"agent_name":"claude","status":"completed","summary":"ok"}"#;
        let mut msg = tool_msg(Some(activities));
        msg.content = String::new();
        let _element = render(&msg, &test_theme(), None, false, noop_toggle());
    }

    #[test]
    fn test_render_tool_card_collapsed() {
        let activities = r#"{"agent_name":"claude","status":"completed","summary":"done","is_running":false}"#;
        let _element = render(&tool_msg(Some(activities)), &test_theme(), None, true, noop_toggle());
    }
}
