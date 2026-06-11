#![windows_subsystem = "windows"]

mod app;
mod config;
mod herdr;
mod sidebar;
mod system_info;
mod terminal;
mod terminal_element;

use gpui::{App, AppContext, Bounds, point, size};
use gpui_component::{Root, Theme, ThemeRegistry};
use gpui_component_assets::Assets;

use crate::app::DeepinHerdr;

const EMBEDDED_THEME_JSONS: &[&str] = &[
    include_str!("../assets/themes/matrix.json"),
    include_str!("../assets/themes/tokyonight.json"),
    include_str!("../assets/themes/gruvbox.json"),
    include_str!("../assets/themes/solarized.json"),
];

fn load_embedded_themes(cx: &mut App) {
    let registry = ThemeRegistry::global_mut(cx);
    for theme_json in EMBEDDED_THEME_JSONS {
        if let Err(err) = registry.load_themes_from_str(theme_json) {
            tracing::warn!("failed to load embedded theme: {err:#}");
        }
    }
}

const EMBEDDED_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Lilex-Regular.ttf"),
    include_bytes!("../assets/fonts/Lilex-Bold.ttf"),
    include_bytes!("../assets/fonts/Lilex-Italic.ttf"),
    include_bytes!("../assets/fonts/Lilex-BoldItalic.ttf"),
];

fn load_embedded_fonts(cx: &mut App) {
    let text_system = cx.text_system();
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = EMBEDDED_FONTS
        .iter()
        .map(|data| std::borrow::Cow::Borrowed(*data))
        .collect();
    if let Err(err) = text_system.add_fonts(fonts) {
        tracing::warn!("failed to load embedded fonts: {err:#}");
    }
}

fn open_main_window(cx: &mut App) {
    let config = config::ConfigStore::load().unwrap_or_default();

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

    let saved_theme = config.theme_name().to_string();

    cx.open_window(options, |window, cx| {
        window.activate_window();
        window.set_window_title("deepin-herdr");
        Theme::sync_system_appearance(Some(window), cx);

        // Apply persisted theme AFTER system appearance sync
        if !saved_theme.is_empty() {
            let registry = ThemeRegistry::global(cx);
            if let Some(tc) = registry.themes().get(saved_theme.as_str()).cloned() {
                Theme::global_mut(cx).apply_config(&tc);
            }
        }
        // Force embedded Lilex font after all theme applications.
        let theme = Theme::global_mut(cx);
        theme.font_family = "Lilex".into();

        let view = cx.new(|cx| DeepinHerdr::new(window, cx, config));
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
        load_embedded_themes(cx);
        load_embedded_fonts(cx);

        // Override gpui_component's default font family with embedded Lilex.
        let theme = Theme::global_mut(cx);
        theme.font_family = "Lilex".into();

        open_main_window(cx);
    });
}
