//! Agent configuration adapters module.
//!
//! Provides the `AgentConfigAdapter` trait and concrete implementations
//! for each supported AI agent CLI. This replaces the hardcoded match
//! statements in `sync_api_to_agent()` and provides a clean extension
//! point for future agents.
//!
//! Consumers are only other unwired paths: `api_provider` asks for an adapter
//! to write one provider's config (`adapter_for`), and `skills` asks for the
//! adapter list to find each agent's skills directory (`all_adapters`). Neither
//! entry point is reached from the shell, so nothing here runs at runtime.
//! `SyncResult` is the one type in this module with no caller at all.

pub mod adapters;
