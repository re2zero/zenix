//! Opening shell windows.
//!
//! Every zenix window is the same shape: geometry from the config's `window`
//! section, the close guard installed before the first paint, and a [`Root`]
//! over its own endpoint connection. [`crate::run`] opens the first window
//! through here; the `view.new-window` chord and a second launch open further
//! ones. Each window is one more connection face over the daemon the config
//! points at — the daemon owns the workspaces, so the windows see the same
//! grid and either can drive it.

use gpui::*;

use ely_core::theme::ActiveTheme;

use crate::config;
use crate::ui::shell::Root;

/// The options a shell window opens with. The stored config decides the
/// window's size and maximized state, written by the closing window itself;
/// the position is the compositor's to choose. The shell draws the whole
/// frame itself, the window controls included, so the window asks for
/// client-side decorations. Where the platform cannot honor that — on X11, a
/// compositor that does not advertise GTK frame extents (KWin with desktop
/// effects off among them) — gpui falls back to the window manager's own
/// decorations and the chrome hides its controls instead of stacking two
/// title bars. macOS keeps its traffic lights drawn over our strip, at the
/// origin the theme reserves for them.
pub(crate) fn window_options(cx: &App) -> WindowOptions {
    let window_config = config::get(cx).window.clone();
    let window_size = size(px(window_config.width), px(window_config.height));
    let window_bounds = if window_config.maximized {
        WindowBounds::Maximized(Bounds {
            origin: Default::default(),
            size: window_size,
        })
    } else {
        WindowBounds::centered(window_size, cx)
    };
    let titlebar = TitlebarOptions {
        title: Some("zenix".into()),
        appears_transparent: true,
        traffic_light_position: Some(cx.theme().traffic_light_origin()),
    };
    WindowOptions {
        window_bounds: Some(window_bounds),
        window_min_size: Some(size(px(640.), px(400.))),
        titlebar: Some(titlebar),
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    }
}

/// Open one more shell window and bring it to the front. Both of the new
/// window's close paths — the strip's own button and the window manager's —
/// save its geometry through the close guard, which goes in before the shell
/// because a close can arrive before the first paint.
pub(crate) fn open_shell_window(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let options = window_options(cx);
    cx.open_window(options, |window, cx| {
        window.activate_window();
        crate::ui::install_close_guard(window, cx);
        cx.new(|cx| Root::new(window, cx))
    })
}
