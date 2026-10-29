//! zenix — extensible terminal workspace manager with plugin and sub-app support.
//!
//! This crate is the library: it owns the module tree and the startup path.
//! `src/main.rs` is a thin binary that only calls [`run`], so the tree is
//! compiled once instead of once per target. The library exposes the public API
//! for plugins and sub-applications to integrate with zenix.
//!
//! ## Threads
//!
//! The client's resident threads — the daemon reader (`herdr::connection`), the
//! event stream (`events`), the fleet stream (`fleet`), the plugin host
//! (`plugins::host`) and the worktree worker (`worktrees`) — are detached and
//! live for the process. The quit mode is
//! `LastWindowClosed`: closing the last window is process exit, which takes
//! every thread with it, so there is nothing for a stop flag to stop. A
//! per-window shutdown story arrives with multi-window (PLAN §3 B tier); until
//! then a thread's lifetime is the process's.

rust_i18n::i18n!("locales");

use std::sync::OnceLock;

use ely_core::theme::ActiveTheme;
use gpui::*;

/// Handle to the tokio runtime created by [`run`].
///
/// Required because GPUI's async executor is not a tokio runtime, so
/// `tokio::task::spawn` would panic ("no reactor running") when called
/// from a GPUI-spawned task. Code that needs to run on tokio (e.g.
/// `reqwest` async HTTP) fetches this handle and calls `.spawn()` on it.
pub static TOKIO_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

pub mod ai;
pub mod api_provider;
pub mod cli;
pub mod client;
pub mod config;
pub mod config_adapters;
pub mod events;
pub mod fleet;
pub mod herdr;
pub mod i18n;
pub mod layout;
pub mod mcp;
pub mod platform;
pub mod plugins;
pub mod provider;
pub mod review;
pub mod single_instance;
pub mod sys;
pub mod terminal;
pub mod ui;
pub mod util;
pub mod worktrees;

// Re-export core types for plugin/sub-app convenience
pub use client::{find_herdr_binary, herdr_socket_path, is_socket_ready, start_herdr_server};
pub use config::ConfigStore;
pub use sys::{CpuSamples, SystemInfo};

/// Start the client: tracing, the tokio runtime, the config directory, the
/// single-instance lock, then the GPUI application and its first window.
pub fn run() {
    // A verb on the command line means this process is a tool, not a window:
    // it runs before anything about a desktop exists (`zenix mcp-agent`).
    if let Some(code) = cli::dispatch() {
        std::process::exit(code);
    }

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

    gpui_platform::application()
        // The component library's fonts and icons, served from the binary. It
        // is the whole source: nothing of the retired kit ships any more.
        .with_assets(ely_core::Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(|cx| {
            // The theme global and the library's key bindings. The assets are
            // compiled in, so a failure here is a broken build and a window
            // without a theme would only draw a panic.
            ely_core::init(cx).expect("the component library could not start");
            // The stored config decides the chrome's mode and the locale
            // before anything is drawn, so the first frame is already right.
            config::init(cx);
            // The prompt templates the orchestrator sends, seeded to the config
            // directory and cached; without this the embedded copy is used and
            // an edited `prompts/orchestrator.md` would never be read.
            ai::prompts::init();
            // The window opens the shape the last one closed with: its size
            // and maximized state are the config's `window` section, written
            // by the closing window itself. The position is the compositor's
            // to choose.
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
            // The shell draws the whole frame itself, the window controls
            // included, so the window asks for client-side decorations. Where
            // the platform cannot honor that — on X11, a compositor that does
            // not advertise GTK frame extents (KWin with desktop effects off
            // among them) — gpui falls back to the window manager's own
            // decorations and the chrome hides its controls instead of
            // stacking two title bars. macOS keeps its traffic lights drawn
            // over our strip, at the origin the theme reserves for them.
            let titlebar = TitlebarOptions {
                title: Some("zenix".into()),
                appears_transparent: true,
                traffic_light_position: Some(cx.theme().traffic_light_origin()),
            };
            let options = WindowOptions {
                window_bounds: Some(window_bounds),
                window_min_size: Some(size(px(640.), px(400.))),
                titlebar: Some(titlebar),
                window_decorations: Some(WindowDecorations::Client),
                ..Default::default()
            };
            cx.open_window(options, |window, cx| {
                // Both of the window's close paths — the strip's own button
                // and the window manager's — save its geometry through this
                // one check. It goes in before the shell because a close can
                // arrive before the first paint.
                ui::install_close_guard(window, cx);
                cx.new(|cx| ui::Root::new(window, cx))
            })
            .expect("failed to open window");
        });

    // Release lock on exit
    single_instance::release();
}
