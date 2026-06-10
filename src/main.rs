#![windows_subsystem = "windows"]

mod app;
mod herdr;
mod terminal;
mod terminal_element;

use gpui::{App, AppContext, Bounds, point, size};
use gpui_component::{Root, Theme};
use gpui_component_assets::Assets;

use crate::app::DeepinHerdr;

fn open_main_window(cx: &mut App) {
    let bounds = cx
        .displays()
        .first()
        .map(|display| {
            let db = display.bounds();
            let width = db.size.width * 0.8;
            let height = db.size.height * 0.9;
            let x = db.origin.x + (db.size.width - width) / 2.0;
            let y = db.origin.y + (db.size.height - height) / 2.0;
            Bounds::new(point(x, y), size(width, height))
        });

    let mut options = gpui::WindowOptions::default();
    options.window_bounds = bounds.map(gpui::WindowBounds::Windowed);

    cx.open_window(options, |window, cx| {
        window.activate_window();
        window.set_window_title("deepin-herdr");
        Theme::sync_system_appearance(Some(window), cx);

        let view = cx.new(|cx| DeepinHerdr::new(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    })
    .expect("failed to open window");
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let app = gpui_platform::application()
        .with_assets(Assets)
        .with_quit_mode(gpui::QuitMode::LastWindowClosed);

    app.run(move |cx| {
        gpui_component::init(cx);
        open_main_window(cx);
    });
}
