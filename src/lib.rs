//! zenix — extensible terminal workspace manager with plugin and sub-app support.
//!
//! This crate is both a binary and a library. The library exposes
//! the public API for plugins and sub-applications to integrate with zenix.

rust_i18n::i18n!("locales");

use std::sync::OnceLock;

/// Handle to the tokio runtime created in `main.rs`.
///
/// Required because GPUI's async executor is not a tokio runtime, so
/// `tokio::task::spawn` would panic ("no reactor running") when called
/// from a GPUI-spawned task. Code that needs to run on tokio (e.g.
/// `reqwest` async HTTP) fetches this handle and calls `.spawn()` on it.
pub static TOKIO_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

pub mod agent;
pub mod ai;
pub mod api_provider;
pub mod config_adapters;
pub mod i18n;
pub mod mcp;
pub mod platform;
pub mod provider;
pub mod skills;

pub mod app;
pub mod client;
pub mod config;
pub mod single_instance;
pub mod sys;
pub mod terminal;
pub mod ui;

// Re-export core types for plugin/sub-app convenience
pub use app::ZenixApp;
pub use client::{find_herdr_binary, herdr_socket_path, is_socket_ready, start_herdr_server};
pub use config::ConfigStore;
pub use sys::{CpuSamples, SystemInfo};
pub use terminal::{
    BackendCommand, BackendEvent, BackendTx, ImeState, RenderSnapshot, TerminalElement,
    TerminalTab, encode_key, encode_mouse_drag, encode_mouse_event, encode_mouse_motion,
    encode_mouse_scroll,
};
pub use ui::{PANEL_WIDTH, Panel, SIDEBAR_WIDTH, ThemeCallback};
