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
pub mod client;
pub mod config;
pub mod config_adapters;
pub mod herdr;
pub mod i18n;
pub mod mcp;
pub mod platform;
pub mod plugins;
pub mod provider;
pub mod shell;
pub mod single_instance;
pub mod skills;
pub mod sys;
pub mod terminal;
pub mod util;

// Re-export core types for plugin/sub-app convenience
pub use client::{find_herdr_binary, herdr_socket_path, is_socket_ready, start_herdr_server};
pub use config::ConfigStore;
pub use sys::{CpuSamples, SystemInfo};
