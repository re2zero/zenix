//! Agent CLI detection — checks local filesystem for installed agent CLIs
//! and their herdr integration hooks. Uses the ProviderRegistry for
//! agent definitions instead of a hardcoded list.

pub mod hooks;

use std::path::PathBuf;

use crate::provider::ProviderRegistry;

/// Status of an agent CLI on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCliStatus {
    /// Hook exists and version matches.
    Installed { version: u32 },
    /// Hook exists but version is behind.
    Outdated { current: u32, expected: u32 },
    /// Binary found on PATH but no herdr hook installed.
    BinaryOnly,
    /// Neither binary nor hook found.
    NotInstalled,
}

/// Information about a known AI agent CLI.
#[allow(dead_code)]
pub struct AgentCliInfo {
    /// Short machine name (e.g. "claude").
    pub name: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Binary names to check on PATH (first match wins).
    pub binary_names: &'static [&'static str],
    /// Resolved hook directory (if provider supports hooks and config dir exists).
    pub hook_dir: Option<PathBuf>,
    /// Name of the hook file within the hook directory.
    pub hook_filename: Option<&'static str>,
    /// Expected version in the hook file (HERDR_INTEGRATION_VERSION line).
    pub expected_version: u32,
    /// Detected status.
    pub status: AgentCliStatus,
}

/// Check if a binary exists anywhere on `$PATH`.
fn binary_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .as_ref()
        .map(|p| {
            std::env::split_paths(p).any(|dir| {
                dir.join(name).is_file()
            })
        })
        .unwrap_or(false)
}

/// Check if ANY of the given binary names exist on PATH.
fn any_binary_on_path(names: &[&str]) -> bool {
    names.iter().any(|n| binary_on_path(n))
}

/// Parse `HERDR_INTEGRATION_VERSION=N` from a hook file.
fn parse_hook_version(path: &PathBuf) -> Option<u32> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("HERDR_INTEGRATION_VERSION=") {
            return v.trim().parse().ok();
        }
    }
    None
}

/// Detect all known agent CLIs and return their status.
pub fn detect_all_agents() -> Vec<AgentCliInfo> {
    let registry = ProviderRegistry::builtin();
    registry.all().iter().map(|prov| {
        let has_binary = any_binary_on_path(prov.binary_names);
        let hook_path = prov.hook_path();
        let hook_version = hook_path.as_ref().and_then(parse_hook_version);

        let expected = prov.expected_hook_version.unwrap_or(0);

        let status = match (has_binary, hook_version) {
            (_, Some(v)) if v >= expected => AgentCliStatus::Installed { version: v },
            (_, Some(v)) => AgentCliStatus::Outdated { current: v, expected },
            (true, None) => AgentCliStatus::BinaryOnly,
            (false, None) => AgentCliStatus::NotInstalled,
        };

        AgentCliInfo {
            name: prov.name,
            display_name: prov.display_name,
            binary_names: prov.binary_names,
            hook_dir: prov.hook_dir_path(),
            hook_filename: prov.hook_filename,
            expected_version: expected,
            status,
        }
    }).collect()
}