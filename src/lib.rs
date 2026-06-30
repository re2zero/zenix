//! zenix — extensible terminal workspace manager with plugin and sub-app support.
//!
//! This crate is both a binary and a library. The library exposes
//! the public API for plugins and sub-applications to integrate with zenix.

rust_i18n::i18n!("locales");

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
