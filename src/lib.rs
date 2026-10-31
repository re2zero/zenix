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
//! outlive the code that started them, but each has a way to end: the daemon
//! reader stops when its window's pulse inbox closes, the plugin host and the
//! two request-serving workers end when their channels disconnect, and the
//! event and fleet streams read a stop flag the window's drop sets (a stream
//! parked in one blocking line read ends at that read's next answer). The
//! orchestrator socket (`mcp`) is the one deliberately process-lifetime
//! surface: it serves until the process ends, whichever windows come and go.

rust_i18n::i18n!("locales");

use std::sync::OnceLock;

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

    // The verb check above already answered every non-window launch, so what
    // is left names what this process's first window is: the daemon to open
    // on, or — for a focus ask with nothing running yet — the daemon whose
    // window would have been raised, opened instead.
    let launch = match cli::parse(std::env::args().skip(1)) {
        Ok(launch @ (cli::Launch::Window(_) | cli::Launch::Focus(_))) => launch,
        _ => unreachable!("dispatch answered every non-window launch"),
    };
    let first_target = match &launch {
        cli::Launch::Window(target) => target.clone(),
        cli::Launch::Focus(Some(target)) => target.clone(),
        cli::Launch::Focus(None) => herdr_client::ConnectTarget::Local,
        _ => unreachable!("dispatch answered every non-window launch"),
    };
    // A second instance forwards the launch it parsed — ask included — and
    // exits; this one keeps the receiver.
    let second_launch_rx = match single_instance::try_acquire(&launch) {
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
        .run(move |cx| {
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
            // The first window, on the target the command line named. Its
            // shape — geometry, client decorations, the close guard and a
            // shell over its own endpoint connection — is `ui::windows`, the
            // same face the `view.new-window` chord and a second launch use.
            ui::windows::open_shell_window(cx, first_target).expect("failed to open window");

            // A second launch asks this process for one more window, or for
            // a running one to be raised. The receiver is crossbeam, so the
            // blocking read runs on the background executor and a fresh
            // clone carries each ask.
            cx.spawn(async move |cx| {
                loop {
                    let rx = second_launch_rx.clone();
                    let signal = cx
                        .background_executor()
                        .spawn(async move { rx.recv() })
                        .await;
                    let Ok(signal) = signal else {
                        break;
                    };
                    let handled = match signal {
                        single_instance::Signal::Open(target) => {
                            cx.update(|cx| ui::windows::open_shell_window(cx, target).map(|_| ()))
                        }
                        single_instance::Signal::Focus(which) => cx.update(|cx| {
                            if ui::windows::focus_window(cx, which.as_ref()) {
                                Ok(())
                            } else {
                                // Nobody to raise yet: the ask opens the
                                // window it would have raised.
                                ui::windows::open_shell_window(
                                    cx,
                                    which.unwrap_or(herdr_client::ConnectTarget::Local),
                                )
                                .map(|_| ())
                            }
                        }),
                    };
                    if let Err(error) = handled {
                        tracing::error!(%error, "a second launch's ask could not be answered");
                        break;
                    }
                }
            })
            .detach();
        });

    // Release lock on exit
    single_instance::release();
}
