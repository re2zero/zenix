//! Agent CLI detection — checks local filesystem for installed agent CLIs.
//! Uses the ProviderRegistry for agent definitions.

use std::path::PathBuf;

use crate::provider::ProviderRegistry;

/// Information about a known AI agent CLI.
pub struct AgentCliInfo {
    /// Short machine name (e.g. "claude").
    pub name: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Binary names to check on PATH (first match wins).
    pub binary_names: &'static [&'static str],
    /// Install command for this agent.
    pub install_command: &'static str,
    /// Resolved binary path if found on PATH.
    pub binary_path: Option<PathBuf>,
    /// Whether the agent CLI binary was found on PATH.
    pub detected: bool,
}

/// Find a binary on `$PATH` and return its full path.
fn find_binary(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").as_ref().and_then(|p| {
        std::env::split_paths(p).find_map(|dir| {
            let full = dir.join(name);
            if full.is_file() { Some(full) } else { None }
        })
    })
}

/// Check if ANY of the given binary names exist on PATH and return the first path.
fn find_any_binary(names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(|n| find_binary(n))
}

/// Detect all known agent CLIs and return their status.
pub fn detect_all_agents() -> Vec<AgentCliInfo> {
    let registry = ProviderRegistry::builtin();
    registry.all().iter().map(|prov| {
        let binary_path = find_any_binary(prov.binary_names);
        AgentCliInfo {
            name: prov.name,
            display_name: prov.display_name,
            binary_names: prov.binary_names,
            install_command: prov.install_command,
            binary_path: binary_path.clone(),
            detected: binary_path.is_some(),
        }
    }).collect()
}