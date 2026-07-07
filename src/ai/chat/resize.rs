use gpui::{
    AppContext as _, InteractiveElement as _, IntoElement, Stateful,
    StatefulInteractiveElement as _, Styled, div, px,
};
use gpui_component::Theme;

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
/// Visual only — drag detection is handled by app-level mouse tracking.
pub fn render_input_resize_handle(theme: &Theme) -> Stateful<gpui::Div> {
    div()
        .id("ai-input-resize-handle")
        .h(px(1.))
        .w_full()
        .cursor_row_resize()
        .bg(theme.border)
        .hover(|style| style.bg(theme.accent))
}
