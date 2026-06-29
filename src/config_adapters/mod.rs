//! Agent configuration adapters module.
//!
//! Provides the `AgentConfigAdapter` trait and concrete implementations
//! for each supported AI agent CLI. This replaces the hardcoded match
//! statements in `sync_api_to_agent()` and provides a clean extension
//! point for future agents.

pub mod adapters;
