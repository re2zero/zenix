//! A panel's width: drag the edge that faces the grid, double-click it to go
//! back to the default width. It never takes more than `MAX_SHARE` of the
//! window, however wide it was made, so a narrow window keeps room for the grid
//! beside it.
//!
//! Ported in shape from herdr-gpui (Apache-2.0)
//! `crates/herdr-gpui/src/panel_resize.rs`, including its `restore`/`unsaved`
//! pair: the width a drag leaves behind is saved once, when the gesture ends
//! (`take_unsaved`), not on every frame of it. What is saved, and what a stored
//! width may be, lives in [`crate::config`].
//!
//! Two panels are drawn this way, one on each side of the grid, so the same
//! width runs from a different edge in each: [`Edge::Right`] is the sidebar's
//! inner edge (its width is measured from its own left edge), [`Edge::Left`] is
//! the right-hand panel's (measured from its own right edge).

use gpui_kit::*;

use crate::config::{
    ASIDE_WIDTH_DEFAULT, ASIDE_WIDTH_MAX, ASIDE_WIDTH_MIN, SIDEBAR_WIDTH_DEFAULT,
    SIDEBAR_WIDTH_MAX, SIDEBAR_WIDTH_MIN,
};

/// How wide the grabbable edge is, laid over the panel's border.
const HANDLE: f32 = 6.;
/// The tint a resizable edge shows under the pointer.
const RESIZE_HOVER: u32 = 0x78a9ff44;
/// The share of the window a panel may take.
const MAX_SHARE: f32 = 0.6;

/// Which edge of a panel the pointer drags, and therefore which of the panel's
/// own edges the width is measured from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edge {
    /// The panel sits on the left; the handle is its right edge, and the width
    /// runs from the panel's left edge to the pointer.
    Right,
    /// The panel sits on the right; the handle is its left edge, and the width
    /// runs from the pointer to the panel's right edge.
    Left,
}

/// The sidebar's width: its default until the user drags it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelWidth {
    chosen: Option<f32>,
    default: f32,
    min: f32,
    max: f32,
    edge: Edge,
    /// Changed since it was last saved.
    unsaved: bool,
}

/// The sidebar before it is dragged, as in herdr-gpui's `sidebar/metrics.rs`.
pub(crate) const SIDEBAR: PanelWidth = PanelWidth::new(
    SIDEBAR_WIDTH_DEFAULT,
    SIDEBAR_WIDTH_MIN,
    SIDEBAR_WIDTH_MAX,
    Edge::Right,
);

/// The right-hand panel before it is dragged.
pub(crate) const ASIDE: PanelWidth = PanelWidth::new(
    ASIDE_WIDTH_DEFAULT,
    ASIDE_WIDTH_MIN,
    ASIDE_WIDTH_MAX,
    Edge::Left,
);

impl PanelWidth {
    pub(crate) const fn new(default: f32, min: f32, max: f32, edge: Edge) -> Self {
        Self {
            chosen: None,
            default,
            min,
            max,
            edge,
            unsaved: false,
        }
    }

    /// The width the user chose, to save; `None` follows the default.
    pub(crate) fn chosen(&self) -> Option<f32> {
        self.chosen
    }

    /// The width a stored config names; anything unusable keeps the default.
    pub(crate) fn restore(&mut self, stored: f32) {
        self.chosen = Some(stored)
            .filter(|width| width.is_finite() && *width > 0.)
            .map(|width| width.clamp(self.min, self.max));
        self.unsaved = false;
    }

    /// The width to draw beside `available` pixels of space: the chosen or
    /// default width, never more than `MAX_SHARE` of the space, and never less
    /// than the minimum while the space allows it. Space not known yet (`0`)
    /// caps nothing.
    pub(crate) fn width(&self, available: f32) -> f32 {
        let width = self.chosen.unwrap_or(self.default);
        if available <= 0. {
            return width;
        }
        width
            .min(available * MAX_SHARE)
            .max(self.min.min(available))
    }

    /// Follows the pointer at `x` while the panel, laid out at `bounds`, is
    /// dragged by its inner edge. Whether it changed.
    pub(crate) fn drag(&mut self, bounds: Bounds<Pixels>, x: Pixels) -> bool {
        let width = match self.edge {
            Edge::Right => f32::from(x - bounds.left()),
            Edge::Left => f32::from(bounds.right() - x),
        }
        .clamp(self.min, self.max);
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

    /// Whether the width changed since this last answered, so a drag is saved
    /// once, when it ends.
    pub(crate) fn take_unsaved(&mut self) -> bool {
        std::mem::take(&mut self.unsaved)
    }
}

/// The edge to drag, laid over the panel's inner edge; put it inside the panel,
/// which must be `relative()` and listen for `on_drag_move`. `reset` runs on a
/// double-click.
pub(crate) fn grip<D: 'static>(
    id: &'static str,
    edge: Edge,
    drag: D,
    reset: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let handle = div()
        .id(id)
        .absolute()
        .top_0()
        .h_full()
        .w(px(HANDLE))
        .cursor(CursorStyle::ResizeLeftRight)
        .hover(|style| style.bg(rgba(RESIZE_HOVER)));
    let handle = match edge {
        Edge::Right => handle.right_0(),
        Edge::Left => handle.left_0(),
    };
    handle
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            cx.stop_propagation();
            if event.click_count == 2 {
                reset(event, window, cx);
            }
        })
        // The value only names the gesture: the shell owns the width, and
        // resolves the pointer against the edge it was started on.
        .on_drag(drag, |_, _, _, cx| cx.new(|_| EmptyView))
}

/// The drag a press on the sidebar's [`grip`] starts.
pub(crate) struct SidebarDrag;

/// The drag a press on the right-hand panel's [`grip`] starts.
pub(crate) struct AsideDrag;

#[cfg(test)]
mod tests {
    // `use super::*` re-exports GPUI's `test` attribute macro, which would
    // otherwise shadow the builtin one and recurse: name the builtin.
    use super::*;
    use std::prelude::v1::test;

    fn bounds(left: f32, width: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(left), px(0.)), size(px(width), px(600.)))
    }

    #[test]
    fn dragging_the_edge_follows_the_pointer_and_stops_at_the_minimum() {
        let mut sidebar = SIDEBAR;
        assert_eq!(sidebar.width(0.), 232.);
        assert!(sidebar.drag(bounds(0., 232.), px(300.)));
        assert_eq!(sidebar.width(0.), 300.);
        assert!(
            !sidebar.drag(bounds(0., 300.), px(300.)),
            "a pointer that did not move changes nothing"
        );
        // Below the minimum, the sidebar stops rather than disappear.
        sidebar.drag(bounds(0., 300.), px(100.));
        assert_eq!(sidebar.width(0.), 160.);
        sidebar.reset();
        assert_eq!(sidebar.width(0.), 232.);
    }

    #[test]
    fn a_dragged_sidebar_keeps_to_its_share_of_a_narrow_window() {
        let mut sidebar = SIDEBAR;
        sidebar.drag(bounds(0., 232.), px(900.));
        assert_eq!(sidebar.width(2000.), 900.);
        assert_eq!(sidebar.width(1000.), 600., "60% of the window is the cap");
        assert_eq!(sidebar.width(300.), 180.);
        // The minimum holds while the window has room for it, and gives way
        // only once the window is narrower than the minimum itself.
        assert_eq!(sidebar.width(150.), 150.);
    }

    /// A drag is worth saving once, when it ends, however many frames it took.
    #[test]
    fn only_a_finished_drag_is_worth_saving() {
        let mut sidebar = SIDEBAR;
        assert!(!sidebar.take_unsaved(), "nothing was dragged");
        sidebar.drag(bounds(0., 232.), px(300.));
        assert_eq!(sidebar.chosen(), Some(300.));
        assert!(sidebar.take_unsaved());
        assert!(!sidebar.take_unsaved(), "the same drag is saved once");
        sidebar.drag(bounds(0., 300.), px(300.));
        assert!(!sidebar.take_unsaved(), "a pointer that did not move");
    }

    /// The right-hand panel is dragged by its left edge, so its width runs the
    /// other way: from the pointer to the panel's own right edge.
    #[test]
    fn the_right_hand_panel_measures_its_width_from_its_left_edge() {
        let panel = bounds(1000., 380.);
        let mut aside = ASIDE;
        assert_eq!(aside.width(0.), 380.);
        assert!(aside.drag(panel, px(1100.)));
        assert_eq!(aside.width(0.), 280., "right edge 1380 minus pointer 1100");
        // Dragging past the maximum stops at it, rather than taking the grid.
        aside.drag(panel, px(-9000.));
        assert_eq!(aside.width(0.), 4096.);
        // And past its own left edge it stops at the minimum.
        aside.drag(panel, px(9000.));
        assert_eq!(aside.width(0.), 240.);
        aside.reset();
        assert_eq!(aside.width(0.), 380.);
    }

    /// What the config names is what is drawn, and it is not something to save
    /// again: the panel only restores it.
    #[test]
    fn a_stored_width_is_restored_and_the_default_remains_the_fallback() {
        let mut sidebar = SIDEBAR;
        sidebar.restore(349.);
        assert_eq!(sidebar.width(0.), 349.);
        assert!(!sidebar.take_unsaved());
        // A stored width below the minimum is drawn at the minimum rather than
        // as a sliver; the config's own range check reports the file.
        sidebar.restore(20.);
        assert_eq!(sidebar.width(0.), 160.);
        assert_eq!(sidebar.chosen(), Some(160.));
    }
}
