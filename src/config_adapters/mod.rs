//! Agent configuration adapters module.
//!
//! Provides the `AgentConfigAdapter` trait and concrete implementations
//! for each supported AI agent CLI: the extension point for writing one
//! provider's config into an agent's own file format.
//!
//! Consumer: `adapter_for`, the one entry point this module keeps. Its caller
//! (`api_provider`'s sync path) is gone, so it stays as the extension point a
//! future settings page would use. `all_adapters` had only `skills` as a
//! consumer and that module is gone; `config_paths` was read through it.

pub mod adapters;
