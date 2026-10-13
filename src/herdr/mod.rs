//! Daemon-facing facade: the endpoint connection and the pushed state it owns.
//!
//! `herdr-client` speaks the generation-1 endpoint protocol; the workspace,
//! tab, pane and agent model arrives as `ClientEvent` pushes. Nothing here
//! polls the daemon — the endpoint is the state truth.

pub(crate) mod connection;
pub(crate) mod osc52;
pub(crate) mod state;

pub(crate) use connection::Connection;
pub(crate) use osc52::decode;
pub(crate) use state::{ConnectionStatus, LiveState};
