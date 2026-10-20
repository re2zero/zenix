//! Daemon-facing facade: the endpoint connection and the pushed state it owns.
//!
//! `herdr-client` speaks the generation-1 endpoint protocol; the workspace,
//! tab, pane and agent model arrives as `ClientEvent` pushes. Nothing here
//! polls the daemon — the endpoint is the state truth.

/// The herdr release this client is built against.
///
/// It comes from `vendor/herdr-pins.tsv` (the same row `scripts/fetch-herdr.sh`
/// downloads and `build.rs` checks `res/herdr` against), so the claim cannot
/// drift from the seed the package ships. A daemon that answers a different
/// version is still spoken to — the endpoint protocol is generation 1 — but
/// `zenix-probe` reports the mismatch rather than pretending they agree.
pub const UPSTREAM_VERSION: &str = env!("HERDR_UPSTREAM_VERSION");

/// The JSON API lane is public: `zenix-probe` and the shell's plugin host are
/// both clients of it, and neither goes through the AI module to reach it.
pub mod api;
pub(crate) mod connection;
pub(crate) mod osc52;
pub(crate) mod state;

pub(crate) use connection::Connection;
pub(crate) use osc52::decode;
pub(crate) use state::{ConnectionStatus, LiveState};
