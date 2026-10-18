//! zenix — extensible terminal workspace manager with plugin and sub-app support.
//!
//! This crate is the library: it owns the module tree and the startup path.
//! `src/main.rs` is a thin binary that only calls [`run`], so the tree is
//! compiled once instead of once per target. The library exposes the public API
//! for plugins and sub-applications to integrate with zenix.

rust_i18n::i18n!("locales");

use std::borrow::Cow;
use std::sync::OnceLock;

use gpui_kit::*;

/// Handle to the tokio runtime created by [`run`].
///
/// Required because GPUI's async executor is not a tokio runtime, so
/// `tokio::task::spawn` would panic ("no reactor running") when called
/// from a GPUI-spawned task. Code that needs to run on tokio (e.g.
/// `reqwest` async HTTP) fetches this handle and calls `.spawn()` on it.
pub static TOKIO_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

pub mod ai;
pub mod api_provider;
pub mod client;
pub mod config;
pub mod config_adapters;
pub mod herdr;
pub mod i18n;
pub mod layout;
pub mod mcp;
pub mod platform;
pub mod plugins;
pub mod provider;
pub mod shell;
pub mod single_instance;
pub mod sys;
pub mod terminal;
pub mod util;

// Re-export core types for plugin/sub-app convenience
pub use client::{find_herdr_binary, herdr_socket_path, is_socket_ready, start_herdr_server};
pub use config::ConfigStore;
pub use sys::{CpuSamples, SystemInfo};

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

/// Start the client: tracing, the tokio runtime, the config directory, the
/// single-instance lock, then the GPUI application and its first window.
pub fn run() {
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
