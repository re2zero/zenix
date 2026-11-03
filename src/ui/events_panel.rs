//! The event log, drawn where a debug surface belongs: under a bug icon in
//! the status bar's right corner, not in navigation.
//!
//! The sidebar's events view retired 2026-11-03: a stream of daemon
//! announcements is not daily navigation, it is the answer to "what did the
//! daemon just do". The stream itself keeps running exactly as before — the
//! worktree list is refreshed by its `worktree.*` lines and the inspector
//! reads the ring whether the panel is drawn or not — so the config's
//! `debug.events` flag now gates this panel's entry (the status bar's bug
//! icon, set from the settings dialog's about page) instead of a rail
//! destination.
//!
//! The panel is the main column's foot: pressing the icon expands it above
//! the bar, pressing again — or the header's × — folds it away. It draws the
//! ring the old view drew, the filter chips and the newest-first rows with
//! their class dots, and a row's press still opens the inspector drawer,
//! where the JSON, the raw line and the 21-name contract live.

use ely_core::{
    buttons::IconButton,
    layout::ScrollArea,
    primitives::{Icon, IconName},
    theme::{ActiveTheme, ControlSize, IconSize, Radius, TextSize},
    typography::Ellipsis,
};
use gpui::{
    AnyElement, App, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use super::shell::Root;
use super::sidebar::{empty_line, error_line, row_id};
use crate::events::{self, EventFilter, EventState};
use crate::i18n::t;
use crate::plugins::host::PluginState;

/// The expanded panel's height. A debug log is read by the newest lines; a
/// taller pane would take room the grid is open for.
const PANEL_HEIGHT: f32 = 240.;
const PANEL_HEAD_HEIGHT: f32 = 32.;

/// The expanded log: one fixed-height pane between the grid and the status
/// bar, reading the shell's copy of the ring. Nothing here writes state —
/// a chip press asks [`Root`] to move the filter, a row press hands the
/// event to the inspector, the × only asks to fold.
pub(crate) fn panel(
    state: &EventState,
    plugins: &PluginState,
    selected: Option<&events::Event>,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let hooked = |kind: &str| plugins.plugins.iter().any(|plugin| plugin.hooks_on(kind));
    let now = events::now_unix_ms();

    let mut column = div()
        .flex_none()
        .flex()
        .flex_col()
        .h(px(PANEL_HEIGHT))
        .border_t_1()
        .border_color(colors.border)
        .bg(colors.bg)
        .child(head(state, root, cx));

    if let Some(error) = state.error.as_deref() {
        column = column.child(error_line(error, cx));
    }
    column = column.child(chips(state.filter, root, cx));

    let rows: Vec<&events::Event> = state
        .events
        .iter()
        .filter(|event| state.filter.matches(event, &hooked))
        .collect();
    let mut list = div().flex().flex_col().gap(px(1.)).px_1p5().pb_2();
    if rows.is_empty() {
        list = list.child(empty_line(t("shell.debug_log.empty"), cx));
    }
    for (index, event) in rows.into_iter().enumerate() {
        let picked = selected.is_some_and(|captured| captured == event);
        list = list.child(event_row(
            index,
            event,
            events::class(event, &hooked),
            picked,
            now,
            root,
            cx,
        ));
    }

    column
        .child(
            ScrollArea::new("debug-log-body")
                .vertical()
                .shadows()
                .size_full()
                .child(list),
        )
        .into_any_element()
}

/// The panel's one line of chrome: what it is, whether the stream is live,
/// how much it holds, and the two ways out — the contract the rows are
/// written against, and the fold.
fn head(state: &EventState, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let contract = root.clone();
    let close = root.clone();
    let live = state.error.is_none();
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .h(px(PANEL_HEAD_HEIGHT))
        .pl_3()
        .pr_2()
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Sm))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.fg)
                .child(t("shell.debug_log.title")),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(if live { colors.success } else { colors.danger })
                .child(if live {
                    t("shell.debug_log.live")
                } else {
                    t("shell.debug_log.down")
                }),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(
                    rust_i18n::t!("shell.debug_log.count", count = state.events.len()).to_string(),
                ),
        )
        .child(div().flex_1())
        .child(
            div()
                .id("debug-log-contract")
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .hover(move |style| style.text_color(colors.fg))
                .on_click(move |_, _, cx| {
                    contract.update(cx, |root, cx| root.open_contract(cx));
                })
                .child(
                    Icon::new(IconName::FileText)
                        .size(IconSize::Xs)
                        .color(colors.fg_subtle),
                )
                .child(t("shell.inspector.contract_foot")),
        )
        .child(
            IconButton::new("debug-log-close", IconName::X)
                .size(ControlSize::Sm)
                .tooltip(t("shell.debug_log.close"))
                .on_click(move |_, _, cx| {
                    close.update(cx, |root, cx| root.close_debug_log(cx));
                }),
        )
        .into_any_element()
}

/// The chips are the filter's own id and label; pressing one writes the
/// stream's cache and repaints, because the ring is the one copy of events.
fn chips(filter: EventFilter, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut row = div().flex().flex_wrap().gap_1().px_1().pt_1();
    for choice in EventFilter::ALL {
        let chosen = choice == filter;
        let root = root.clone();
        row = row.child(
            div()
                .id(row_id("event-chip", choice.id()))
                .flex_none()
                .h(px(20.))
                .px_1p5()
                .flex()
                .items_center()
                .rounded(theme.radius(Radius::Sm))
                .border_1()
                .border_color(if chosen {
                    colors.border_strong
                } else {
                    colors.border
                })
                .bg(if chosen { colors.active } else { colors.bg })
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(if chosen { colors.fg } else { colors.fg_muted })
                .cursor_pointer()
                .hover(move |style| style.bg(colors.hover))
                .on_click(move |_, _, cx| {
                    root.update(cx, |root, cx| root.set_event_filter(choice, cx));
                })
                .child(choice.label()),
        );
    }
    row.into_any_element()
}

fn event_row(
    index: usize,
    event: &events::Event,
    class: EventFilter,
    selected: bool,
    now: u64,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let tone = match class {
        EventFilter::Agents => colors.info,
        EventFilter::Plugins => colors.accent,
        EventFilter::Tabs => colors.success,
        EventFilter::Layout => colors.warning,
        EventFilter::All => colors.fg_subtle,
    };
    let capture = root.clone();
    let captured = event.clone();
    div()
        .id(("event", index))
        .relative()
        .flex()
        .gap_1p5()
        .px_2()
        .py(px(5.))
        .rounded(theme.radius(Radius::Md))
        .cursor_pointer()
        .when(selected, |row| row.bg(colors.surface))
        .hover(move |style| style.bg(colors.hover))
        .on_click(move |_, window, cx| {
            let captured = captured.clone();
            capture.update(cx, |root, cx| root.open_event(captured.clone(), window, cx));
        })
        .when(selected, |row| {
            row.child(
                div()
                    .absolute()
                    .left(px(-6.))
                    .top(px(6.))
                    .bottom(px(6.))
                    .w(px(2.))
                    .rounded_full()
                    .bg(colors.accent),
            )
        })
        .child(
            div()
                .flex_none()
                .pt(px(3.))
                .size(px(5.))
                .rounded_full()
                .bg(tone),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_muted)
                        .child(div().flex_none().child(events::age(now, event.at_unix_ms)))
                        .child(
                            div()
                                .min_w_0()
                                .child(Ellipsis::new(SharedString::from(event.kind.clone()))),
                        ),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(Ellipsis::new(SharedString::from(event.detail.clone()))),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use crate::i18n::t;

    /// A debug surface that prints its own key is a broken surface: every
    /// word the panel and its status-bar icon say resolves through the
    /// locale file.
    #[test]
    fn the_debug_log_speaks_through_the_locale_file() {
        for key in [
            "shell.debug_log.title",
            "shell.debug_log.toggle",
            "shell.debug_log.close",
            "shell.debug_log.live",
            "shell.debug_log.down",
            "shell.debug_log.empty",
            "shell.debug_log.count",
        ] {
            assert_ne!(t(key), key, "{key} has no locale entry");
        }
    }
}
