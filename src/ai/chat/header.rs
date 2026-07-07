//! Header + dropdown + history panel + agent monitor overlay + empty state.

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Styled as _,
    div, px, svg,
};
use gpui_component::{
    Theme,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

use crate::i18n::t;

use super::state::{AiChatAction, AiChatActionHandler, AiChatPanelData};

// ── Header ──────────────────────────────────────────────────────────────

pub(super) fn render_header(
    data: &AiChatPanelData,
    theme: &Theme,
    on_action: AiChatActionHandler,
) -> AnyElement {
    let th = theme.clone();
    let fg = th.foreground;
    let mu = th.muted_foreground;
    let border = th.border;
    let accent = th.accent;

    let title = data.current_session_title.as_deref().unwrap_or("ZenixAI");

    let on_new_chat = on_action.clone();
    let on_menu = on_action.clone();

    let header_bar = h_flex()
        .justify_between()
        .items_center()
        .px(px(12.))
        .py(px(10.))
        .border_b_1()
        .border_color(border)
        .child(
            super::txt(px(14.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(fg)
                .child(title.to_string()),
        )
        .child(
            h_flex()
                .gap(px(2.))
                .items_center()
                .child(
                    Button::new("ai-chat-new")
                        .ghost()
                        .compact()
                        .w(px(26.))
                        .h(px(26.))
                        .tooltip(t("ai.chat.new_chat"))
                        .on_click(move |_, window, cx| {
                            on_new_chat(AiChatAction::NewChat, window, cx)
                        })
                        .child(svg().path("icons/plus.svg").size(px(14.)).text_color(mu)),
                )
                .child(
                    Button::new("ai-chat-menu")
                        .ghost()
                        .compact()
                        .w(px(26.))
                        .h(px(26.))
                        .tooltip("Menu")
                        .on_click(move |_, window, cx| on_menu(AiChatAction::OpenMenu, window, cx))
                        .child(
                            svg()
                                .path("icons/ellipsis.svg")
                                .size(px(14.))
                                .text_color(if data.header_menu_open { accent } else { mu }),
                        ),
                ),
        );

    header_bar.into_any_element()
}

pub(super) fn render_header_menu(
    _data: &AiChatPanelData,
    theme: &Theme,
    on_action: AiChatActionHandler,
) -> AnyElement {
    let th = theme.clone();
    let fg = th.foreground;
    let bg = th.background;
    let border = th.border;

    let on_history = on_action.clone();
    let on_agents = on_action.clone();
    let on_clear = on_action.clone();
    let on_settings = on_action.clone();
    let on_backdrop = on_action.clone();

    let item = |label: &str, cb: AiChatActionHandler, action: AiChatAction| -> AnyElement {
        let label_str: String = label.into();
        let cb = cb.clone();
        h_flex()
            .items_center()
            .px(px(10.))
            .py(px(6.))
            .rounded_md()
            .cursor_pointer()
            .hover(|st| st.bg(th.list_hover))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cb(action.clone(), window, cx);
                cb(AiChatAction::OpenMenu, window, cx);
                cx.stop_propagation();
            })
            .child(super::txt(px(12.)).text_color(fg).child(label_str.clone()))
            .into_any_element()
    };

    let history_item = item(
        &t("ai.history.title").to_string(),
        on_history.clone(),
        AiChatAction::OpenDrawer(crate::ai::chat::DrawerKind::History),
    );
    let agents_item = item(
        &t("ai.agent.monitor_title").to_string(),
        on_agents.clone(),
        AiChatAction::OpenDrawer(crate::ai::chat::DrawerKind::Agents),
    );
    let clear_item = item("Clear chat", on_clear.clone(), AiChatAction::NewChat);
    let settings_item = item("Settings", on_settings.clone(), AiChatAction::OpenSettings);

    let menu = v_flex()
        .absolute()
        .top(px(44.))
        .right(px(12.))
        .w(px(180.))
        .bg(bg)
        .border_1()
        .border_color(border)
        .rounded_md()
        .shadow_md()
        .p(px(4.))
        .gap(px(2.))
        .child(history_item)
        .child(agents_item)
        .child(clear_item)
        .child(settings_item);

    div()
        .absolute()
        .inset_0()
        .occlude()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_backdrop(AiChatAction::OpenMenu, window, cx);
        })
        .child(menu)
        .into_any_element()
}

// ── Empty state ─────────────────────────────────────────────────────────

pub(super) fn render_empty_state(
    _data: &AiChatPanelData,
    theme: &Theme,
    on_action: AiChatActionHandler,
) -> AnyElement {
    let th = theme.clone();
    let fg = th.foreground;
    let mu = th.muted_foreground;
    let accent = th.accent;

    v_flex()
        .flex_grow(1.0)
        .min_h_0()
        .items_center()
        .justify_center()
        .p(px(24.))
        .gap(px(16.))
        .child(
            div()
                .w(px(48.))
                .h(px(48.))
                .rounded_full()
                .bg(th.muted)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    super::txt(px(22.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(accent)
                        .child("Z"),
                ),
        )
        .child(
            super::txt(px(17.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(fg)
                .child("有什么可以帮你?"),
        )
        .child(
            super::txt(px(12.))
                .text_color(mu)
                .child("基于当前 zenix workspace · 输入 / 用命令, @ 加上下文"),
        )
        .child(
            v_flex()
                .w(px(360.))
                .gap(px(6.))
                .items_stretch()
                .child(suggestion_chip(
                    "⎇",
                    "总结最近 5 次 git commits",
                    "git",
                    theme,
                    on_action.clone(),
                ))
                .child(suggestion_chip(
                    "▣",
                    "分析当前工作区结构",
                    "workspace",
                    theme,
                    on_action.clone(),
                ))
                .child(suggestion_chip(
                    "F",
                    "解释当前打开的文件",
                    "code",
                    theme,
                    on_action,
                )),
        )
        .into_any_element()
}

fn suggestion_chip(
    icon: &str,
    label: &str,
    meta: &str,
    theme: &Theme,
    on_action: AiChatActionHandler,
) -> AnyElement {
    let th = theme.clone();
    let fg = th.foreground;
    let mu = th.muted_foreground;
    let border = th.border;
    let accent = th.accent;
    let on_click = on_action.clone();
    let label_str: String = label.into();

    h_flex()
        .w_full()
        .px(px(12.))
        .py(px(8.))
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(th.muted)
        .cursor_pointer()
        .items_center()
        .hover(|st| st.bg(th.list_hover).border_color(accent))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_click(AiChatAction::SuggestionClick(label_str.clone()), window, cx)
        })
        .child(
            super::txt(px(13.))
                .text_color(accent)
                .child(icon.to_string()),
        )
        .child(
            super::txt(px(12.))
                .text_color(fg)
                .ml(px(8.))
                .child(label.to_string()),
        )
        .child(div().flex_1())
        .child(super::txt(px(10.)).text_color(mu).child(meta.to_string()))
        .into_any_element()
}

// ── Agent monitor ───────────────────────────────────────────────────────

pub(super) fn render_agent_monitor(data: &AiChatPanelData, theme: &Theme) -> AnyElement {
    let th = theme.clone();
    let mu = th.muted_foreground;
    let border = th.border;
    let fg = th.foreground;

    let entries: Vec<AnyElement> = data
        .agent_statuses
        .iter()
        .map(|entry| {
            let (dot, color) = match entry.status.as_str() {
                "working" => ("●", th.accent),
                "blocked" => ("◉", gpui::hsla(0.08, 0.9, 0.55, 1.0)),
                "done" => ("✓", mu),
                _ => ("○", mu),
            };

            h_flex()
                .justify_between()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .py(px(6.))
                .rounded_md()
                .hover(|st| st.bg(th.list_hover))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(super::txt(px(12.)).text_color(color).child(dot.to_string()))
                        .child(
                            super::txt(px(12.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(fg)
                                .child(entry.agent_name.clone()),
                        ),
                )
                .child(
                    super::txt(px(10.))
                        .text_color(mu)
                        .child(entry.status.clone()),
                )
                .into_any_element()
        })
        .collect();

    v_flex()
        .gap(px(4.))
        .px(px(8.))
        .py(px(8.))
        .border_b_1()
        .border_color(border)
        .child(
            h_flex().justify_between().items_center().child(
                super::txt(px(11.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(mu)
                    .child(format!(
                        "{} ({})",
                        t("ai.agent.monitor_title").to_uppercase(),
                        data.active_agent_count
                    )),
            ),
        )
        .children(entries)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::session::Session;
    use gpui_component::Theme;

    fn test_theme() -> Theme {
        Theme::default()
    }

    fn noop_handler() -> AiChatActionHandler {
        std::rc::Rc::new(|_, _, _| {})
    }

    #[test]
    fn test_render_header_returns_element() {
        let data = AiChatPanelData::default();
        let theme = test_theme();
        let _element = render_header(&data, &theme, noop_handler());
    }

    #[test]
    fn test_render_empty_state_returns_element() {
        let data = AiChatPanelData::default();
        let theme = test_theme();
        let _element = render_empty_state(&data, &theme, noop_handler());
    }

    #[test]
    fn test_render_agent_monitor_empty() {
        let data = AiChatPanelData::default();
        let theme = test_theme();
        let _element = render_agent_monitor(&data, &theme);
    }

    #[test]
    fn test_render_agent_monitor_with_entries() {
        let mut data = AiChatPanelData::default();
        data.agent_statuses = vec![
            super::super::agent_block::AgentMonitorEntry {
                agent_name: "agent-1".into(),
                pane_id: "p1".into(),
                status: "working".into(),
                recent_output: "thinking...".into(),
                is_expanded: false,
            },
            super::super::agent_block::AgentMonitorEntry {
                agent_name: "agent-2".into(),
                pane_id: "p2".into(),
                status: "done".into(),
                recent_output: String::new(),
                is_expanded: false,
            },
        ];
        data.active_agent_count = 1;
        let theme = test_theme();
        let _element = render_agent_monitor(&data, &theme);
    }

    #[test]
    fn test_suggestion_chip() {
        let theme = test_theme();
        let _element = suggestion_chip("⚠", "Test suggestion", "git", &theme, noop_handler());
    }
}
