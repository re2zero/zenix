use gpui::{
    ClickEvent, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

use crate::ui::settings::styles::*;

pub fn zed_search_bar(
    placeholder: &str,
    input: Option<&Entity<gpui_component::input::InputState>>,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    let ph = placeholder.to_string();
    h_flex()
        .gap(gap_md())
        .items_center()
        .min_h(search_bar_min_h())
        .px(pad_lg())
        .bg(bg_secondary_theme(theme))
        .border_1()
        .border_color(border_color_transparent())
        .rounded(border_radius_md())
        .child(
            txt(font_size_base())
                .text_color(text_color_muted())
                .child("\u{1F50D}"),
        )
        .child(
            input
                .map(|e| e.clone().into_any_element())
                .unwrap_or_else(|| {
                    txt(font_size_base())
                        .text_color(text_color_very_muted())
                        .child(ph)
                        .into_any_element()
                }),
        )
}

pub fn zed_badge(text: impl Into<String>, color: impl Into<gpui::Hsla>) -> impl IntoElement {
    let t = text.into();
    let c: gpui::Hsla = color.into();
    h_flex()
        .items_center()
        .justify_center()
        .px(badge_pad_x())
        .py(badge_pad_y())
        .min_h(badge_min_h())
        .rounded(border_radius_sm())
        .bg(c)
        .child(
            txt(font_size_xs())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(text_color_high())
                .child(t),
        )
}

pub fn zed_card(theme: &gpui_component::Theme) -> gpui::Div {
    v_flex()
        .gap(gap_md())
        .py(pad_lg())
        .px(pad_xl())
        .bg(bg_secondary_theme(theme))
        .border_1()
        .border_color(border_color_transparent())
        .rounded(border_radius_md())
}

pub fn setting_group_header(title: impl Into<String>) -> impl IntoElement {
    txt(font_size_md())
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(text_color_high())
        .child(title.into())
}

pub fn setting_label(text: impl Into<String>) -> impl IntoElement {
    txt(font_size_base())
        .text_color(text_color_medium())
        .child(text.into())
}

pub fn setting_row<L: Into<String>, V: IntoElement>(label: L, ctrl: V) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap(gap_md())
        .child(setting_label(label))
        .child(ctrl)
}

pub fn txt(size: gpui::Pixels) -> gpui::Div {
    gpui::div().text_size(size)
}

#[allow(dead_code)]
pub fn render_status_dot(active: bool) -> gpui::Div {
    div()
        .w(status_dot_size())
        .h(status_dot_size())
        .rounded(border_radius_xs())
        .bg(if active {
            success_color()
        } else {
            text_color_very_muted()
        })
}

#[allow(dead_code)]
pub fn render_status_dot_lg(active: bool) -> gpui::Div {
    div()
        .w(status_dot_lg())
        .h(status_dot_lg())
        .rounded(border_radius_xs())
        .bg(if active {
            success_color()
        } else {
            text_color_muted()
        })
}

#[allow(dead_code)]
pub fn status_indicator(active: bool, label: impl Into<String>) -> impl IntoElement {
    h_flex()
        .gap(gap_sm())
        .items_center()
        .child(render_status_dot_lg(active))
        .child(
            txt(font_size_sm())
                .text_color(text_color_muted())
                .child(label.into()),
        )
}

pub fn zed_tab_bar(
    name: &'static str,
    labels: Vec<String>,
    selected: usize,
    on_click: impl Fn(usize, &mut Window, &mut gpui::App) + Clone + 'static,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    h_flex()
        .gap(gap_xs())
        .py(pad_sm())
        .px(pad_sm())
        .bg(bg_secondary_theme(theme))
        .rounded(border_radius_md())
        .children(labels.iter().enumerate().map(|(idx, label)| {
            let is_selected = idx == selected;
            let oc = on_click.clone();
            let lbl = label.clone();
            gpui::div()
                .id(format!("{name}-tab-{idx}"))
                .flex_1()
                .text_center()
                .px(pad_md())
                .py(pad_sm())
                .rounded(border_radius_sm())
                .cursor_pointer()
                .when(is_selected, |d| d.bg(bg_selected_theme(theme)))
                .when(!is_selected, |d| d.hover(|s| s.bg(bg_hover_theme(theme))))
                .child(
                    txt(font_size_sm())
                        .font_weight(if is_selected {
                            gpui::FontWeight::MEDIUM
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(if is_selected {
                            text_color_high()
                        } else {
                            text_color_muted()
                        })
                        .child(lbl),
                )
                .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                    oc(idx, w, a);
                })
        }))
}

pub fn extension_tab_nav(
    name: &'static str,
    labels: Vec<String>,
    selected: usize,
    on_click: impl Fn(usize, &mut Window, &mut gpui::App) + Clone + 'static,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    h_flex()
        .gap(gap_xs())
        .py(pad_md())
        .px(pad_md())
        .border_b_1()
        .border_color(border_color_transparent())
        .children(labels.iter().enumerate().map(|(idx, label)| {
            let is_selected = idx == selected;
            let oc = on_click.clone();
            let lbl = label.clone();
            gpui::div()
                .id(format!("{name}-ext-tab-{idx}"))
                .px(pad_md())
                .py(pad_sm())
                .rounded_t(border_radius_md())
                .cursor_pointer()
                .when(is_selected, |d| {
                    d.bg(bg_selected_theme(theme))
                        .border_b_2()
                        .border_color(primary_color())
                })
                .when(!is_selected, |d| d.hover(|s| s.bg(bg_hover_theme(theme))))
                .child(
                    txt(font_size_base())
                        .font_weight(if is_selected {
                            gpui::FontWeight::MEDIUM
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(if is_selected {
                            primary_color()
                        } else {
                            text_color_muted()
                        })
                        .child(lbl),
                )
                .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                    oc(idx, w, a);
                })
        }))
}

pub fn section_card(theme: &gpui_component::Theme) -> gpui::Div {
    v_flex()
        .gap(gap_lg())
        .py(pad_xl())
        .px(pad_xl())
        .bg(bg_secondary_theme(theme))
        .border_1()
        .border_color(border_color_transparent_2())
        .rounded(border_radius_md())
}

pub fn section_header() -> gpui::Div {
    txt(font_size_md())
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(text_color_high())
}

#[allow(dead_code)]
pub fn zed_btn_primary(label: impl Into<String>) -> Button {
    Button::new("btn-primary").primary().label(label.into())
}

#[allow(dead_code)]
pub fn zed_btn_ghost(label: impl Into<String>) -> Button {
    Button::new("btn-ghost")
        .ghost()
        .compact()
        .label(label.into())
}
