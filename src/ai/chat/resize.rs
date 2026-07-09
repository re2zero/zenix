use std::rc::Rc;

use gpui::{InteractiveElement as _, MouseButton, Stateful, Styled, Window, div, px};
use gpui_component::Theme;

use super::state::AiChatAction;

/// Panel width constraints (matches config clamping).
pub const MIN_PANEL_WIDTH: f32 = 360.0;
pub const MAX_PANEL_WIDTH: f32 = 720.0;
pub const MIN_INPUT_HEIGHT: f32 = 60.0;
pub const MAX_INPUT_HEIGHT: f32 = 400.0;

/// Render the resize handle at the panel's left edge.
///
/// Visual only — drag detection is handled by app-level mouse tracking.
pub fn render_resize_handle(theme: &Theme) -> Stateful<gpui::Div> {
    div()
        .id("ai-panel-resize-handle")
        .absolute()
        .left(px(0.))
        .top(px(0.))
        .h_full()
        .w(px(1.))
        .cursor_col_resize()
        .bg(theme.border)
        .hover(|style| style.bg(theme.accent))
}

/// Render the resize handle above the input area.
///
/// Fires `BeginInputResize` on mouse-down so the app-level mouse-move
/// handler can track the drag across the whole window.
pub fn render_input_resize_handle(
    theme: &Theme,
    input_height: f32,
    on_action: Rc<dyn Fn(AiChatAction, &mut Window, &mut gpui::App) + 'static>,
) -> Stateful<gpui::Div> {
    div()
        .id("ai-input-resize-handle")
        .h(px(1.))
        .w_full()
        .cursor_row_resize()
        .bg(theme.border)
        .hover(|style| style.bg(theme.accent))
        .on_mouse_down(MouseButton::Left, {
            move |ev, _window, cx| {
                let start_y = f32::from(ev.position.y);
                on_action(
                    AiChatAction::BeginInputResize(start_y, input_height),
                    _window,
                    cx,
                );
                cx.stop_propagation();
            }
        })
}
