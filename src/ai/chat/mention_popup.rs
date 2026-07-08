//! @ mention popup — shows active workers for direct messaging.
//!
//! Renders as a deferred overlay above the input bar. Reads active worker
//! instances from WorkerRegistry and filters by the user's query text.

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Styled as _,
    div, px,
};
use gpui_component::{Theme, h_flex, scroll::ScrollableElement as _, v_flex};

use super::state::{AiChatAction, AiChatActionHandler, AiChatPanelData};

const MAX_VISIBLE_ITEMS: usize = 8;

struct MentionItem {
    worker_name: String,
    cli_name: String,
    role_id: String,
    status_label: String,
    abs_idx: usize,
}

pub fn render_mention_popup(
    data: &AiChatPanelData,
    workers: &[crate::ai::workers::WorkerInstance],
    theme: &Theme,
    on_action: AiChatActionHandler,
) -> AnyElement {
    if !data.show_mention_popup {
        return div().into_any_element();
    }

    let th = theme.clone();
    let fg = th.foreground;
    let mu = th.muted_foreground;
    let border = th.border;
    let bg = th.background;
    let accent = th.accent;
    let danger = th.danger;

    let query = data.mention_query.to_lowercase();

    let matching: Vec<MentionItem> = workers
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            query.is_empty()
                || w.worker_name.to_lowercase().contains(&query)
                || w.cli_name.to_lowercase().contains(&query)
                || w.role_id.to_lowercase().contains(&query)
        })
        .map(|(i, w)| MentionItem {
            worker_name: w.worker_name.clone(),
            cli_name: w.cli_name.clone(),
            role_id: w.role_id.clone(),
            status_label: w.status.to_string(),
            abs_idx: i,
        })
        .collect();

    if matching.is_empty() {
        return div().into_any_element();
    }

    let selected_idx = data
        .mention_selected_idx
        .min(matching.len().saturating_sub(1));

    let items: Vec<AnyElement> = matching
        .iter()
        .take(MAX_VISIBLE_ITEMS)
        .map(|item| {
            let is_selected = item.abs_idx == selected_idx;
            let wn = item.worker_name.clone();
            let dt = format!(
                "{} — {} ({})",
                item.worker_name, item.cli_name, item.role_id
            );
            let sc = match item.status_label.as_str() {
                "running" => accent,
                "starting" => mu,
                "done" => gpui::green(),
                s if s.starts_with("error") => danger,
                _ => mu,
            };
            let cb = on_action.clone();
            let list_hover = th.list_hover;

            h_flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .py(px(4.))
                .rounded_md()
                .cursor_pointer()
                .hover(move |st| st.bg(list_hover))
                .bg(if is_selected {
                    th.list_hover
                } else {
                    gpui::transparent_black()
                })
                .on_mouse_down(MouseButton::Left, {
                    let wn2 = wn.clone();
                    let cb2 = cb.clone();
                    move |_, window, cx| {
                        cb2(AiChatAction::ShowMentionPopup(wn2.clone()), window, cx);
                        cb2(AiChatAction::MentionConfirm, window, cx);
                    }
                })
                .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(sc))
                .child(
                    super::txt(px(11.))
                        .text_color(if is_selected { accent } else { fg })
                        .child(dt),
                )
                .into_any_element()
        })
        .collect();

    let count = matching.len();
    let suffix = if count > MAX_VISIBLE_ITEMS {
        format!("\n+ {} more", count - MAX_VISIBLE_ITEMS)
    } else {
        String::new()
    };

    gpui::deferred(
        div()
            .absolute()
            .left(px(8.))
            .bottom(px(0.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .rounded_md()
            .shadow_md()
            .p(px(4.))
            .min_w(px(200.))
            .max_h(px(260.))
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .gap(px(2.))
                    .child(
                        super::txt(px(9.))
                            .text_color(mu)
                            .px(px(8.))
                            .py(px(2.))
                            .child(format!("WORKERS ({})", matching.len())),
                    )
                    .children(items)
                    .child(if !suffix.is_empty() {
                        super::txt(px(9.))
                            .text_color(mu)
                            .px(px(8.))
                            .py(px(2.))
                            .child(suffix)
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            ),
    )
    .into_any_element()
}
