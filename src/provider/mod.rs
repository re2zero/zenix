//! Provider registry — single source of truth for AI agent CLI tools.
//!
//! A `Provider` describes an AI agent tool (Claude Code, Codex, Cursor, etc.)
//! and its capabilities: where its config files live, whether it supports
//! MCP servers and skills directories.
//!
//! `ProviderRegistry` is the central registry. Use `ProviderRegistry::builtin()`
//! to get the built-in list, then query by capability.

pub mod builtin;

use std::path::PathBuf;

/// A known AI agent CLI provider.
#[derive(Debug, Clone)]
pub struct Provider {
    /// Short machine name (e.g. "claude").
    pub name: &'static str,
    /// Human-readable display name (e.g. "Claude Code").
    pub display_name: &'static str,
    /// Binary names to check on PATH (first match wins).
    pub binary_names: &'static [&'static str],
    /// Install command for this provider.
    pub install_command: &'static str,
    /// Function that returns the config directory (if it exists).
    pub config_dir: fn() -> Option<PathBuf>,
    /// MCP config filename relative to config_dir (e.g. "config.json").
    /// `None` means this provider doesn't use Claude-format MCP configs.
    pub mcp_config_file: Option<&'static str>,
    /// Skills directory name relative to config_dir (e.g. "skills").
    /// `None` means this provider doesn't support skill directories.
    pub skills_dir: Option<&'static str>,
}

impl Provider {
    /// Resolve the full MCP config file path for this provider.
    pub fn mcp_config_path(&self) -> Option<PathBuf> {
        let dir = (self.config_dir)()?;
        Some(dir.join(self.mcp_config_file?))
    }

    /// Resolve the full skills directory path for this provider.
    pub fn skills_path(&self) -> Option<PathBuf> {
        let dir = (self.config_dir)()?;
        Some(dir.join(self.skills_dir?))
    }

    /// Whether this provider uses the Claude-format JSON MCP config.
    pub fn supports_mcp(&self) -> bool {
        self.mcp_config_file.is_some()
    }

    /// Whether this provider supports skill directories.
    pub fn supports_skills(&self) -> bool {
        self.skills_dir.is_some()
    }
}

/// Central registry of known AI agent providers.
#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: Vec<Provider>,
}

impl ProviderRegistry {
    /// Create a registry with the built-in 12 providers.
    pub fn builtin() -> Self {
        Self {
            providers: crate::provider::builtin::all_providers(),
        }
    }

    /// Find a provider by name.
    pub fn find(&self, name: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.name == name)
    }

    /// Iterate over all providers.
    pub fn all(&self) -> &[Provider] {
        &self.providers
    }

    /// Providers that support MCP (Claude-format JSON config).
    pub fn mcp_supporting(&self) -> Vec<&Provider> {
        self.providers.iter().filter(|p| p.supports_mcp()).collect()
    }

    /// MCP-supporting provider names (sorted).
    pub fn mcp_agent_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .mcp_supporting()
            .iter()
            .map(|p| p.name.to_string())
            .collect();
        names.sort();
        names
    }

    /// Providers that support skill directories.
    pub fn skills_supporting(&self) -> Vec<&Provider> {
        self.providers
            .iter()
            .filter(|p| p.supports_skills())
            .collect()
    }

    /// Skills-supporting provider names (sorted).
    pub fn skill_agent_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .skills_supporting()
            .iter()
            .map(|p| p.name.to_string())
            .collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_has_providers() {
        let reg = ProviderRegistry::builtin();
        assert!(!reg.all().is_empty());
    }

    #[test]
    fn test_find_claude() {
        let reg = ProviderRegistry::builtin();
        let claude = reg.find("claude").unwrap();
        assert_eq!(claude.display_name, "Claude Code");
        assert!(claude.supports_mcp());
        assert!(claude.supports_skills());
    }

    #[test]
    fn test_mcp_supporting() {
        let reg = ProviderRegistry::builtin();
        let mcp = reg.mcp_supporting();
        // claude, codex, opencode, pi, omp, kilo, hermes
        assert!(mcp.len() >= 7);
    }

    #[test]
    fn test_skills_supporting() {
        let reg = ProviderRegistry::builtin();
        let skills = reg.skills_supporting();
        assert!(skills.len() >= 7);
    }

    #[test]
    fn test_provider_name_unique() {
        let reg = ProviderRegistry::builtin();
        let mut seen = std::collections::HashSet::new();
        for p in reg.all() {
            assert!(seen.insert(p.name), "duplicate provider name: {}", p.name);
        }
    }
}
