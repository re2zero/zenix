#![windows_subsystem = "windows"]
rust_i18n::i18n!("locales");

use std::borrow::Cow;
use std::sync::OnceLock;

use gpui_kit::*;

pub static TOKIO_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

mod agent;
mod ai;
mod api_provider;
mod client;
mod config;
mod config_adapters;
mod herdr;
mod i18n;
mod mcp;
mod platform;
mod provider;
mod shell;
mod single_instance;
mod skills;
mod sys;
mod terminal;
mod util;

/// The terminal font family, registered from the bundled Lilex faces.
const TERMINAL_FONT: &str = "Lilex";

fn register_fonts(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/Lilex-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Lilex-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Lilex-Italic.ttf")),
        Cow::Borrowed(include_bytes!("../assets/fonts/Lilex-BoldItalic.ttf")),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        tracing::error!(%err, "failed to register the terminal font");
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Initialize tokio runtime for async HTTP (reqwest SSE streaming).
    // GPUI's async executor is not a tokio runtime, so code that needs tokio
    // fetches this handle from `TOKIO_HANDLE` and spawns on it.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let _ = TOKIO_HANDLE.set(rt.handle().clone());
    let _guard = rt.enter();

    // Ensure config dir exists for socket
    std::fs::create_dir_all(crate::platform::zenix_config_dir()).ok();

    // Acquire single-instance lock
    let _second_launch_rx = match single_instance::try_acquire() {
        Ok(rx) => rx,
        Err(e) => {
            eprintln!("zenix: {e}");
            std::process::exit(1);
        }
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(|cx| {
            gpui_kit::init(cx);
            // The stored config decides the chrome's mode and the locale
            // before anything is drawn, so the first frame is already right.
            config::init(cx);
            // The prompt templates the orchestrator sends, seeded to the config
            // directory and cached; without this the embedded copy is used and
            // an edited `prompts/orchestrator.md` would never be read.
            ai::prompts::init();
            register_fonts(cx);
            shell::keymap::bind_keys(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::centered(size(px(1100.), px(720.)), cx)),
                    window_min_size: Some(size(px(640.), px(400.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("zenix".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let view = cx.new(shell::ShellView::new);
                    // The pane takes the keyboard as soon as the window opens, as
                    // upstream focuses in its constructor; without this, keys reach
                    // the pane only after the first click. The handle is cloned
                    // first: `read`'s shared borrow conflicts with `&mut App`.
                    let focus = view.read(cx).focus_handle(cx);
                    window.focus(&focus, cx);
                    view
                },
            )
            .expect("failed to open window");
        });

    // Release lock on exit
    single_instance::release();
}
