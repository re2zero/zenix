//! Shared UI helper functions extracted from settings.rs.

use gpui::{
    ClickEvent, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, prelude::FluentBuilder as _, px,
};
use gpui_component::{h_flex, v_flex};

/// Zed-style search bar with magnifying glass icon.
pub fn zed_search_bar(
    placeholder: &str,
    input: Option<&Entity<gpui_component::input::InputState>>,
) -> impl IntoElement {
    let ph = placeholder.to_string();
    h_flex()
        .gap(px(0.))
        .items_center()
        .min_h(px(32.))
        .px(px(10.))
        .bg(gpui::rgba(0x00000033))
        .border_1()
        .border_color(gpui::rgba(0x45475a44))
        .rounded(px(8.))
        .child(txt(px(14.)).text_color(gpui::rgba(0x888888FF)).child("🔍"))
        .child(
            input
                .map(|e| e.clone().into_any_element())
                .unwrap_or_else(|| {
                    txt(px(14.))
                        .text_color(gpui::rgba(0x888888FF))
                        .child(ph)
                        .into_any_element()
                }),
        )
}

/// Zed-style badge.
pub fn zed_badge(text: impl Into<String>, color: gpui::Rgba) -> impl IntoElement {
    let t = text.into();
    h_flex()
        .items_center()
        .justify_center()
        .px(px(6.))
        .py(px(1.))
        .min_h(px(18.))
        .rounded_sm()
        .bg(color)
        .child(
            txt(px(10.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(gpui::rgba(0x101010FF))
                .child(t),
        )
}

/// Zed-style card container.
pub fn zed_card() -> gpui::Div {
    v_flex()
        .gap(px(4.))
        .py(px(6.))
        .px(px(10.))
        .border_1()
        .border_color(gpui::rgba(0x45475a44))
        .rounded_md()
}

/// Setting group header.
pub fn setting_group_header(title: impl Into<String>) -> impl IntoElement {
    txt(px(13.))
        .font_weight(gpui::FontWeight::BOLD)
        .child(title.into())
}

/// Setting label.
pub fn setting_label(text: impl Into<String>) -> impl IntoElement {
    txt(px(11.))
        .text_color(gpui::rgba(0xAAAAAAFF))
        .child(text.into())
}

/// Setting row with label and value.
pub fn setting_row<L: Into<String>, V: IntoElement>(label: L, ctrl: V) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap(px(12.))
        .child(setting_label(label))
        .child(ctrl)
}

/// Plain text element.
pub fn txt(size: gpui::Pixels) -> gpui::Div {
    gpui::div().text_size(size)
}

/// Zed-style tab bar.
pub fn zed_tab_bar(
    name: &'static str,
    labels: Vec<String>,
    selected: usize,
    on_click: impl Fn(usize, &mut Window, &mut gpui::App) + Clone + 'static,
) -> impl IntoElement {
    h_flex()
        .gap(px(0.))
        .items_center()
        .children(labels.iter().enumerate().map(|(idx, label)| {
            let is_selected = idx == selected;
            let oc = on_click.clone();
            let lbl = label.clone();
            gpui::div()
                .id(format!("{name}-tab-{idx}"))
                .px(px(12.))
                .py(px(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(is_selected, |d| d.bg(gpui::rgba(0x3B82F622)))
                .child(
                    txt(px(13.))
                        .font_weight(if is_selected {
                            gpui::FontWeight::MEDIUM
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(if is_selected {
                            gpui::rgba(0x3B82F6FF)
                        } else {
                            gpui::rgba(0x888888FF)
                        })
                        .child(lbl),
                )
                .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                    oc(idx, w, a);
                })
        }))
}
