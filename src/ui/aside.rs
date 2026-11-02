//! The right-hand panel's frame: its header, the conversation below it, and
//! the 34px rail the collapsed panel folds into.
//!
//! The panel carries one body, the conversation — the fleet tab and its TAIL
//! comparison retired 2026-11-02 (the sidebar's fleet view is where the fleet
//! lives). The header names the panel and holds its three presses: a new
//! chat, the session history, and the close that folds the column into the
//! rail. The conversation's unread count rides both the header and the rail.
//!
//! Whether the panel is drawn at all is the shell's decision
//! (`shell::aside_state`): the config's `layout.aside_visible` is the intent,
//! and the viewport's 1180px breakpoint decides whether the window can hold
//! the column. At a width too narrow for it the panel is hidden rather than
//! squeezed — the grid is what the window is for.
//!
//! The column's width is the reader's to drag: [`grip`] lays a 6px edge over
//! the border facing the grid, [`Width`] owns the number and [`Root`] writes
//! it to the config once, when the gesture ends. A double-click puts the
//! default back. The width never takes more than [`MAX_SHARE`] of the window,
//! so a narrow window keeps room for the grid beside it.

use ely_core::buttons::{ButtonVariant, IconButton};
use ely_core::primitives::{Icon, IconName};
use ely_core::theme::{ActiveTheme, ControlSize, IconSize, TextSize};
use ely_core::typography::Ellipsis;
use gpui::{
    AnyElement, App, CursorStyle, Entity, IntoElement, MouseButton, ParentElement, Render,
    SharedString, Styled, Window, div, prelude::*, px,
};

use super::chat::ChatView;
use super::shell::Root;
use crate::config::{ASIDE_WIDTH_DEFAULT, ASIDE_WIDTH_MAX, ASIDE_WIDTH_MIN};
use crate::i18n::t;

/// The grabbable edge's width, laid over the column's border.
const HANDLE: f32 = 6.;
/// The share of the window the panel may take, however wide it was dragged.
const MAX_SHARE: f32 = 0.6;

/// The panel's width: its default until the reader drags the edge, then what
/// the pointer left, never wider than [`MAX_SHARE`] of the window. The value is
/// saved once per gesture — [`take_unsaved`](Width::take_unsaved) answers after
/// the release — so a drag writes the config once, not once per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Width {
    chosen: Option<f32>,
    unsaved: bool,
}

impl Width {
    /// The default width, before any drag.
    pub(crate) const fn new() -> Self {
        Self {
            chosen: None,
            unsaved: false,
        }
    }

    /// The width a stored config names; anything unusable keeps the default.
    pub(crate) fn restore(&mut self, stored: f32) {
        self.chosen = Some(stored)
            .filter(|width| width.is_finite() && *width > 0.)
            .map(|width| width.clamp(ASIDE_WIDTH_MIN, ASIDE_WIDTH_MAX));
        self.unsaved = false;
    }

    /// The width to draw beside `available` pixels of space: the chosen or the
    /// default width, never more than [`MAX_SHARE`] of the space, and never
    /// less than the minimum while the space allows it. Space not known yet
    /// (`0`) caps nothing.
    pub(crate) fn width(&self, available: f32) -> f32 {
        let width = self.chosen.unwrap_or(ASIDE_WIDTH_DEFAULT);
        if available <= 0. {
            return width;
        }
        width
            .min(available * MAX_SHARE)
            .max(ASIDE_WIDTH_MIN.min(available))
    }

    /// Follows the pointer at `x` while the panel's right edge is at `right`:
    /// the width runs from the pointer to that edge. Whether it changed.
    pub(crate) fn drag(&mut self, right: f32, x: f32) -> bool {
        let width = (right - x).clamp(ASIDE_WIDTH_MIN, ASIDE_WIDTH_MAX);
        if self.chosen == Some(width) {
            return false;
        }
        self.chosen = Some(width);
        self.unsaved = true;
        true
    }

    /// Back to the default width.
    pub(crate) fn reset(&mut self) {
        self.chosen = None;
        self.unsaved = true;
    }

    /// The width the reader chose, to save; `None` follows the default.
    pub(crate) fn chosen(&self) -> Option<f32> {
        self.chosen
    }

    /// Whether the width changed since this last answered, so a drag is saved
    /// once, when it ends.
    pub(crate) fn take_unsaved(&mut self) -> bool {
        std::mem::take(&mut self.unsaved)
    }
}

/// The drag a press on the panel's [`grip`] starts. The value only names the
/// gesture: the shell owns the width and resolves the pointer against the
/// panel's edge.
pub(crate) struct AsideDrag;

/// What follows the pointer during a drag. The gesture needs a view to render,
/// and a resize has nothing to show under the cursor.
struct DragGhost;

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The edge to drag: a 6px strip over the column's left border, indicated by
/// the cursor and a tint under the pointer. Put it inside the panel's wrapper,
/// which must be `relative()` and listen for `on_drag_move`; a double-click
/// resets the width to the default.
pub(crate) fn grip(root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let hover = theme.colors.accent;
    let reset = root.clone();
    div()
        .id("aside-grip")
        .absolute()
        .left_0()
        .top_0()
        .h_full()
        .w(px(HANDLE))
        .cursor(CursorStyle::ResizeLeftRight)
        .hover(move |style| style.bg(hover.opacity(0.35)))
        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
            cx.stop_propagation();
            if event.click_count == 2 {
                reset.update(cx, |root, cx| root.reset_aside_width(cx));
            }
        })
        .on_drag(AsideDrag, |_, _, _, cx| cx.new(|_| DragGhost))
        .into_any_element()
}

/// The open panel: a column at the shell's width, the header on top and the
/// conversation filling the rest.
pub(crate) fn column(
    chat: &Entity<ChatView>,
    unread: usize,
    width: f32,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .w(px(width))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .border_l_1()
        .border_color(colors.border)
        .bg(colors.bg)
        .child(header(chat, unread, root, cx))
        .child(chat.clone())
        .into_any_element()
}

/// The strip above the conversation: the panel's name, the unread badge, and
/// the three presses — a new chat, the session history, and the close that
/// folds the panel into its rail.
fn header(chat: &Entity<ChatView>, unread: usize, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let panel = chat.clone();
    let history = chat.clone();
    let close = root.clone();
    div()
        .h(px(36.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(3.))
        .px(px(6.))
        .border_b_1()
        .border_color(colors.border)
        .child(
            Icon::new(IconName::MessageSquare)
                .size(IconSize::Xs)
                .color(colors.fg_muted),
        )
        .child(
            div()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Sm))
                .text_color(colors.fg)
                .child(Ellipsis::new(SharedString::from(t("shell.aside.chat")))),
        )
        .children((unread > 0).then(|| badge(unread, cx)))
        .child(div().flex_1())
        .child(
            IconButton::new("aside-new-chat", IconName::SquarePen)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip(t("ai.chat.new_chat"))
                .on_click(move |_, _, cx| {
                    panel.update(cx, |panel, cx| panel.new_session(cx));
                }),
        )
        .child(
            IconButton::new("aside-history", IconName::History)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip(t("ai.chat.history"))
                .on_click(move |_, _, cx| {
                    history.update(cx, |panel, cx| panel.toggle_history(cx));
                }),
        )
        .child(
            IconButton::new("aside-close", IconName::X)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip(t("shell.aside.close"))
                .on_click(move |_, window, cx| {
                    close.update(cx, |root, cx| root.toggle_aside(window, cx));
                }),
        )
        .into_any_element()
}

/// The collapsed panel: a 34px rail with the conversation's button and its
/// unread count. A press opens the panel and gives the composer the keys.
pub(crate) fn rail(unread: usize, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let open = root.clone();
    let mut slot = div().relative().child(
        IconButton::new("aside-rail-chat", IconName::MessageSquare)
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .tooltip(t("shell.aside.chat"))
            .on_click(move |_, window, cx| {
                open.update(cx, |root, cx| root.open_aside(window, cx));
            }),
    );
    if unread > 0 {
        slot = slot.child(div().absolute().top_0().right_0().child(badge(unread, cx)));
    }
    div()
        .w(px(34.))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(3.))
        .py(px(7.))
        .border_l_1()
        .border_color(colors.border)
        .bg(colors.bg)
        .child(slot)
        .into_any_element()
}

/// The unread count, in the accent: the same chip the header wears.
fn badge(unread: usize, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .h(px(15.))
        .min_w(px(15.))
        .px(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(colors.accent)
        .text_color(colors.on_accent)
        .text_size(px(9.))
        .child(if unread > 99 {
            "99+".to_owned()
        } else {
            unread.to_string()
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    // `use super::*` re-exports GPUI's `test` attribute macro, which would
    // otherwise shadow the builtin one and recurse: name the builtin.
    use super::*;
    use std::prelude::v1::test;

    #[test]
    fn the_width_defaults_and_survives_a_restore() {
        let mut width = Width::new();
        assert_eq!(width.width(1600.), ASIDE_WIDTH_DEFAULT);
        width.restore(480.);
        assert_eq!(width.width(1600.), 480.);
        // A stored value outside the range is brought back into it; one that
        // is not a width at all leaves the default alone.
        width.restore(9000.);
        assert_eq!(width.width(0.), ASIDE_WIDTH_MAX);
        width.restore(f32::NAN);
        assert_eq!(width.width(0.), ASIDE_WIDTH_DEFAULT);
        assert!(!width.take_unsaved(), "a restore is not a drag");
    }

    #[test]
    fn dragging_the_edge_follows_the_pointer_and_stops_at_the_minimum() {
        let mut width = Width::new();
        assert!(width.drag(1600., 1240.), "372 wide before the drag");
        assert_eq!(width.width(0.), 360.);
        assert!(
            !width.drag(1600., 1240.),
            "a pointer that did not move changes nothing"
        );
        // Below the minimum, the panel stops rather than disappear.
        width.drag(1600., 1500.);
        assert_eq!(width.width(0.), ASIDE_WIDTH_MIN);
        width.reset();
        assert_eq!(width.width(0.), ASIDE_WIDTH_DEFAULT);
        assert!(width.take_unsaved(), "a reset is saved like a drag");
        assert!(!width.take_unsaved(), "and only once");
    }

    #[test]
    fn a_wide_panel_keeps_room_for_the_grid() {
        let mut width = Width::new();
        width.drag(1600., 500.);
        assert_eq!(width.width(0.), 1100., "the drag itself is kept");
        assert_eq!(
            width.width(1600.),
            1600. * MAX_SHARE,
            "but never more than 60% of the window"
        );
        assert_eq!(
            width.width(800.),
            800. * MAX_SHARE,
            "and a narrow window caps it further"
        );
    }
}
