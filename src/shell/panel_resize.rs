//! The sidebar's width: drag the edge that faces the grid, double-click it to
//! go back to the default width. It never takes more than `MAX_SHARE` of the
//! window, however wide it was made, so a narrow window keeps room for the grid
//! beside it.
//!
//! Ported in shape from herdr-gpui (Apache-2.0)
//! `crates/herdr-gpui/src/panel_resize.rs`. The sidebar is this client's own
//! chrome, so its width is local; upstream saves it with the rest of its window
//! chrome, which zenix does when settings land in M5.

use gpui_kit::*;

/// How wide the grabbable edge is, laid over the sidebar's border.
const HANDLE: f32 = 6.;
/// The tint a resizable edge shows under the pointer.
const RESIZE_HOVER: u32 = 0x78a9ff44;
/// The share of the window the sidebar may take.
const MAX_SHARE: f32 = 0.6;
/// A stored width beyond this is not a real panel.
const MAX_WIDTH: f32 = 4096.;

/// The sidebar's width: its default until the user drags it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelWidth {
    chosen: Option<f32>,
    default: f32,
    min: f32,
}

/// The sidebar before it is dragged, as in herdr-gpui's `sidebar/metrics.rs`.
pub(crate) const SIDEBAR: PanelWidth = PanelWidth::new(232., 160.);

impl PanelWidth {
    pub(crate) const fn new(default: f32, min: f32) -> Self {
        Self {
            chosen: None,
            default,
            min,
        }
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

    /// Follows the pointer at `x` while the sidebar, laid out at `bounds`, is
    /// dragged by its inner edge. The sidebar sits on the left, so its width
    /// runs from the panel's own left edge to the pointer. Whether it changed.
    pub(crate) fn drag(&mut self, bounds: Bounds<Pixels>, x: Pixels) -> bool {
        let width = f32::from(x - bounds.left()).clamp(self.min, MAX_WIDTH);
        if self.chosen == Some(width) {
            return false;
        }
        self.chosen = Some(width);
        true
    }

    /// Back to the default width.
    pub(crate) fn reset(&mut self) {
        self.chosen = None;
    }
}

/// The edge to drag, laid over the sidebar's inner edge; put it inside the
/// sidebar, which must be `relative()` and listen for `on_drag_move`. `reset`
/// runs on a double-click.
pub(crate) fn grip(
    id: &'static str,
    reset: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .top_0()
        .right_0()
        .h_full()
        .w(px(HANDLE))
        .cursor(CursorStyle::ResizeLeftRight)
        .hover(|style| style.bg(rgba(RESIZE_HOVER)))
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            cx.stop_propagation();
            if event.click_count == 2 {
                reset(event, window, cx);
            }
        })
        // The value only names the gesture: the shell owns the width, and
        // resolves the pointer against the edge it was started on.
        .on_drag(SidebarDrag, |_, _, _, cx| cx.new(|_| EmptyView))
}

/// The drag a press on [`grip`] starts.
pub(crate) struct SidebarDrag;

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
}
