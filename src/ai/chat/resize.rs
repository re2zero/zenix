use std::cell::Cell;

use gpui::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, Render, Stateful,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};
use gpui_component::Theme;

/// Minimum and maximum AI panel width in pixels.
pub const MIN_PANEL_WIDTH: f32 = 360.0;
pub const MAX_PANEL_WIDTH: f32 = 720.0;

/// Drag value carried while resizing the AI panel width.
///
/// `start_x` is captured lazily on the first drag-move event so the delta is
/// measured from the cursor position at drag start, not from render time.
#[derive(Default)]
pub struct PanelResizeDrag {
    pub start_width: f32,
    pub start_x: Cell<Option<f32>>,
}

/// Invisible drag-preview view. gpui requires `on_drag` to produce a renderable
/// preview entity; an empty div keeps the drag invisible while still tracking it.
pub struct DragPreview;

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Render the resize handle at the panel's left edge.
///
/// The handle only starts the drag; the actual width updates are handled by
/// `on_drag_move`/`on_drop` listeners on a wider container (the main UI flex),
/// so that dragging leftward (out of the panel) still tracks.
pub fn render_resize_handle(current_width: f32, theme: &Theme) -> Stateful<gpui::Div> {
    div()
        .id("ai-panel-resize-handle")
        .occlude()
        .absolute()
        .left(px(0.))
        .top(px(0.))
        .h_full()
        .w(px(6.))
        .cursor_col_resize()
        .hover(|style| style.bg(theme.accent))
        .on_drag(
            PanelResizeDrag {
                start_width: current_width,
                start_x: Cell::new(None),
            },
            |_value, _offset, _window, cx| { cx.new(|_cx| DragPreview) },
        )
}

/// Compute the new panel width from a drag event. Returns the clamped width.
pub fn compute_resized_width(drag: &PanelResizeDrag, current_cursor_x: f32) -> f32 {
    let start_x = drag.start_x.get().unwrap_or(current_cursor_x);
    // Panel is anchored on the right; dragging its left edge leftward widens it,
    // so delta = (start_x - current_x) is added to the start width.
    let delta = start_x - current_cursor_x;
    (drag.start_width + delta).clamp(MIN_PANEL_WIDTH, MAX_PANEL_WIDTH)
}

pub const MIN_INPUT_HEIGHT: f32 = 60.0;
pub const MAX_INPUT_HEIGHT: f32 = 400.0;

#[derive(Default)]
pub struct InputResizeDrag {
    pub start_height: f32,
    pub start_y: Cell<Option<f32>>,
}

pub fn render_input_resize_handle(current_height: f32, theme: &Theme) -> Stateful<gpui::Div> {
    div()
        .id("ai-input-resize-handle")
        .occlude()
        .h(px(6.))
        .w_full()
        .cursor_row_resize()
        .bg(theme.border)
        .hover(|style| style.bg(theme.accent))
        .on_drag(
            InputResizeDrag {
                start_height: current_height,
                start_y: Cell::new(None),
            },
            |_value, _offset, _window, cx| cx.new(|_cx| DragPreview),
        )
}

pub fn compute_input_height(drag: &InputResizeDrag, cursor_y: f32) -> f32 {
    let start_y = drag.start_y.get().unwrap_or(cursor_y);
    let delta = start_y - cursor_y;
    (drag.start_height + delta).clamp(MIN_INPUT_HEIGHT, MAX_INPUT_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_resized_width_narrower() {
        let drag = PanelResizeDrag {
            start_width: 420.0,
            start_x: Cell::new(Some(100.0)),
        };
        // cursor moves right (x=150): delta = 100-150 = -50 → narrower
        assert_eq!(compute_resized_width(&drag, 150.0), 370.0);
    }

    #[test]
    fn test_compute_resized_width_wider() {
        let drag = PanelResizeDrag {
            start_width: 420.0,
            start_x: Cell::new(Some(100.0)),
        };
        // cursor moves left (x=40): delta = 100-40 = 60 → wider
        assert_eq!(compute_resized_width(&drag, 40.0), 480.0);
    }

    #[test]
    fn test_clamp_to_min() {
        let drag = PanelResizeDrag {
            start_width: 380.0,
            start_x: Cell::new(Some(100.0)),
        };
        // huge rightward drag → clamped to MIN (360)
        assert_eq!(compute_resized_width(&drag, 1000.0), MIN_PANEL_WIDTH);
    }

    #[test]
    fn test_clamp_to_max() {
        let drag = PanelResizeDrag {
            start_width: 700.0,
            start_x: Cell::new(Some(100.0)),
        };
        // huge leftward drag → clamped to MAX (720)
        assert_eq!(compute_resized_width(&drag, -1000.0), MAX_PANEL_WIDTH);
    }

    #[test]
    fn test_lazy_start_x() {
        let drag = PanelResizeDrag {
            start_width: 420.0,
            start_x: Cell::new(None),
        };
        // first call seeds start_x from current → delta 0 → unchanged
        assert_eq!(compute_resized_width(&drag, 100.0), 420.0);
    }
}
