//! Bottom input bar — multi-line text + provider/model/thinking selectors + send.

use gpui::{
    AnyElement, Entity, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px, svg,
};
use gpui_component::{
    Theme,
    h_flex,
    input::{Input, InputState},
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::api_provider::ThinkingLevel;

use super::state::{AiChatAction, AiChatActionHandler, AiChatPanelData, ConfigDropdownTarget};

const PLACEHOLDER: &str = "Ask anything, dispatch agents, or type / for commands";

pub(super) fn render_input_bar(
    data: &AiChatPanelData,
    theme: &Theme,
    input_entity: Option<&Entity<InputState>>,
    on_action: AiChatActionHandler,
) -> AnyElement {
    let th = theme.clone();
    let bg = th.background;
    let mu = th.muted_foreground;
    let border = th.border;
    let muted_bg = th.muted;
    let accent = th.accent;
    let danger = th.danger;

    let on_send = on_action.clone();
    let on_model = on_action.clone();
    let on_thinking = on_action.clone();

    let input_area: AnyElement = match input_entity {
        Some(entity) => Input::new(entity)
            .border_0()
            .min_h(px(data.input_height))
            .max_h(px(400.))
            .into_any_element(),
        None => div()
            .min_h(px(28.))
            .flex()
            .items_center()
            .px(px(6.))
            .py(px(4.))
            .child(super::txt(px(13.)).text_color(mu).child(PLACEHOLDER))
            .into_any_element(),
    };

    let is_model_open = data.config_dropdown == Some(ConfigDropdownTarget::Model);
    let is_thinking_open = data.config_dropdown == Some(ConfigDropdownTarget::Thinking);

    let model_label = if data.selected_model.is_empty() {
        "model".to_string()
    } else {
        data.selected_model.clone()
    };
    let thinking_label = match data.thinking_level {
        ThinkingLevel::Low => "low",
        ThinkingLevel::Medium => "medium",
        ThinkingLevel::High => "high",
        ThinkingLevel::XHigh => "xhigh",
    };

    let model_btn = tool_button(
        &model_label,
        is_model_open,
        theme,
        on_model,
        AiChatAction::ToggleModelDropdown,
    );
    let model_btn_with_popover: AnyElement = if is_model_open {
        div()
            .relative()
            .child(model_btn)
            .child(
                gpui::deferred(
                    div()
                        .absolute()
                        .bottom(px(26.))
                        .left(px(-4.))
                        .bg(bg)
                        .border_1()
                        .border_color(border)
                        .rounded_md()
                        .shadow_md()
                        .p(px(4.))
                        .child(render_model_dropdown(data, theme, on_action.clone())),
                ),
            )
            .into_any_element()
    } else {
        model_btn
    };

    let thinking_btn = tool_button(
        thinking_label,
        is_thinking_open,
        theme,
        on_thinking,
        AiChatAction::ToggleThinkingDropdown,
    );
    let thinking_btn_with_popover: AnyElement = if is_thinking_open {
        div()
            .relative()
            .child(thinking_btn)
            .child(
                gpui::deferred(
                    div()
                        .absolute()
                        .bottom(px(26.))
                        .right(px(0.))
                        .bg(bg)
                        .border_1()
                        .border_color(border)
                        .rounded_md()
                        .shadow_md()
                        .p(px(4.))
                        .child(render_thinking_dropdown(data, theme, on_action.clone())),
                ),
            )
            .into_any_element()
    } else {
        thinking_btn
    };

    let send_btn = div()
        .id("input-send")
        .w(px(28.))
        .h(px(28.))
        .rounded_md()
        .bg(if data.is_streaming { danger } else { accent })
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_send(AiChatAction::Send, window, cx)
        })
        .child(if data.is_streaming {
            div()
                .w(px(12.))
                .h(px(12.))
                .rounded_sm()
                .bg(gpui::white())
                .into_any_element()
        } else {
            svg()
                .path("icons/arrow-up.svg")
                .size(px(14.))
                .text_color(gpui::white())
                .into_any_element()
        });

    let wrapper = v_flex()
        .gap(px(2.))
        .p(px(4.))
        .pl(px(8.))
        .bg(muted_bg)
        .border_1()
        .border_color(border)
        .rounded_md()
        .child(input_area)
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .child(model_btn_with_popover)
                        .child(thinking_btn_with_popover),
                )
                .child(send_btn),
        );

    let mut bar = v_flex()
        .relative()
        .gap(px(6.))
        .px(px(10.))
        .py(px(8.))
        .border_t_1()
        .border_color(border)
        .child(wrapper)
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .px(px(4.))
                .child(super::txt(px(10.)).text_color(mu).child(
                    if data.is_streaming {
                        "streaming · Esc 中止"
                    } else {
                        "Enter 发送 · Shift+Enter 换行 · ↑↓ 历史"
                    },
                ))
                .child(
                    super::txt(px(10.))
                        .text_color(mu)
                        .child("multi-line · auto-grow"),
                ),
        );

    bar.into_any_element()
}

fn tool_button(
    label: &str,
    active: bool,
    theme: &Theme,
    on_action: AiChatActionHandler,
    action: AiChatAction,
) -> AnyElement {
    let th = theme.clone();
    let fg = th.foreground;
    let mu = th.muted_foreground;
    let border = th.border;
    let accent = th.accent;
    h_flex()
        .gap(px(4.))
        .items_center()
        .px(px(8.))
        .h(px(24.))
        .rounded_md()
        .bg(if active { th.list_hover } else { th.background })
        .border_1()
        .border_color(if active { accent } else { border })
        .cursor_pointer()
        .hover(|st| st.border_color(accent))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_action(action.clone(), window, cx);
        })
        .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(accent))
        .child(super::txt(px(11.)).text_color(fg).child(label.to_string()))
        .child(
            svg()
                .path("icons/chevron-down.svg")
                .size(px(12.))
                .text_color(if active { accent } else { mu }),
        )
        .into_any_element()
}

pub(super) fn render_model_dropdown(
    data: &AiChatPanelData,
    theme: &Theme,
    on_action: AiChatActionHandler,
) -> AnyElement {
    let th = theme.clone();
    let fg = th.foreground;
    let mu = th.muted_foreground;
    let border = th.border;
    let accent = th.accent;

    let mut items: Vec<AnyElement> = Vec::new();

    if data.available_providers.len() > 1
        || (data.available_providers.len() == 1
            && data.available_providers[0] != data.selected_provider_id)
    {
        items.push(
            div()
                .px(px(10.))
                .py(px(3.))
                .child(super::txt(px(9.)).text_color(mu).child("PROVIDERS"))
                .into_any_element(),
        );
        for provider in &data.available_providers {
            let is_selected = *provider == data.selected_provider_id;
            let on_select = on_action.clone();
            let provider_name = provider.clone();
            items.push(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(5.))
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|st| st.bg(th.list_hover))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        on_select(
                            AiChatAction::SelectProvider(provider_name.clone()),
                            window,
                            cx,
                        )
                    })
                    .child(
                        div()
                            .w(px(4.))
                            .h(px(4.))
                            .rounded_full()
                            .bg(if is_selected {
                                accent
                            } else {
                                gpui::transparent_black()
                            }),
                    )
                    .child(
                        super::txt(px(11.))
                            .text_color(if is_selected { fg } else { mu })
                            .child(provider.clone()),
                    )
                    .into_any_element(),
            );
        }
        items.push(
            div()
                .mx(px(10.))
                .my(px(4.))
                .h(px(1.))
                .bg(border)
                .into_any_element(),
        );
    }

    items.push(
        div()
            .px(px(10.))
            .py(px(3.))
            .child(super::txt(px(9.)).text_color(mu).child("MODELS"))
            .into_any_element(),
    );

    if data.available_models.is_empty() {
        items.push(
            div()
                .px(px(10.))
                .py(px(8.))
                .child(super::txt(px(12.)).text_color(mu).child("No matches"))
                .into_any_element(),
        );
    } else {
        for model in &data.available_models {
            let is_selected = *model == data.selected_model;
            let on_select = on_action.clone();
            let model_name = model.clone();
            items.push(
                h_flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|st| st.bg(th.list_hover))
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        on_select(AiChatAction::SelectModel(model_name.clone()), window, cx)
                    })
                    .child(div().w(px(4.)).h(px(4.)).rounded_full().bg(if is_selected {
                        accent
                    } else {
                        gpui::transparent_black()
                    }))
                    .child(
                        super::txt(px(12.))
                            .text_color(if is_selected { fg } else { mu })
                            .font_weight(if is_selected {
                                gpui::FontWeight::MEDIUM
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .child(model.clone()),
                    )
                    .into_any_element(),
            );
        }
    }

    v_flex()
        .w(px(220.))
        .max_h(px(320.))
        .overflow_y_scrollbar()
        .gap(px(2.))
        .children(items)
        .into_any_element()
}

pub(super) fn render_thinking_dropdown(
    data: &AiChatPanelData,
    theme: &Theme,
    on_action: AiChatActionHandler,
) -> AnyElement {
    let th = theme.clone();
    let fg = th.foreground;
    let mu = th.muted_foreground;
    let accent = th.accent;
    let levels = [
        (ThinkingLevel::Low, "low"),
        (ThinkingLevel::Medium, "medium"),
        (ThinkingLevel::High, "high"),
        (ThinkingLevel::XHigh, "xhigh"),
    ];
    let items: Vec<AnyElement> = levels
        .iter()
        .map(|(level, label)| {
            let is_selected = data.thinking_level == *level;
            let on_select = on_action.clone();
            let lvl = *level;
            h_flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(6.))
                .rounded_md()
                .cursor_pointer()
                .hover(|st| st.bg(th.list_hover))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    on_select(AiChatAction::SelectThinkingLevel(lvl), window, cx)
                })
                .child(
                    div()
                        .w(px(4.))
                        .h(px(4.))
                        .rounded_full()
                        .bg(if is_selected {
                            accent
                        } else {
                            gpui::transparent_black()
                        }),
                )
                .child(
                    super::txt(px(12.))
                        .text_color(if is_selected { fg } else { mu })
                        .child(*label),
                )
                .into_any_element()
        })
        .collect();
    v_flex()
        .w(px(140.))
        .gap(px(2.))
        .child(
            super::txt(px(9.))
                .text_color(mu)
                .px(px(10.))
                .py(px(3.))
                .child("THINKING"),
        )
        .children(items)
        .into_any_element()
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
    fn test_render_input_bar_returns_element() {
        let data = AiChatPanelData::default();
        let theme = test_theme();
        let _element = render_input_bar(&data, &theme, None, noop_handler());
    }

    #[test]
    fn test_render_model_dropdown_empty() {
        let data = AiChatPanelData::default();
        let theme = test_theme();
        let _element = render_model_dropdown(&data, &theme, noop_handler());
    }

    #[test]
    fn test_render_model_dropdown_with_models() {
        let mut data = AiChatPanelData::default();
        data.available_models = vec!["gpt-4".into(), "claude-3".into()];
        data.selected_model = "gpt-4".into();
        let theme = test_theme();
        let _element = render_model_dropdown(&data, &theme, noop_handler());
    }
}
