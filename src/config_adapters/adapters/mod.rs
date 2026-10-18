//! Per-agent configuration adapters.
//!
//! Each supported AI agent CLI has a different way of storing configuration.
//! This module defines the `AgentConfigAdapter` trait and provides concrete
//! implementations for each supported agent.
//!
//! The trait is the extension point for a future agent: `adapter_for` hands one
//! out by name, and `api_env_vars`/`api_config_entry` say what a provider's
//! config looks like in that agent's format. `config_paths` declares the files
//! and directories an adapter owns.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::api_provider::ApiProviderConfig;
use crate::provider::ProviderRegistry;

/// Specifies a config path managed by an agent adapter.
#[derive(Debug, Clone)]
pub enum ConfigPathSpec {
    /// Claude-format JSON MCP config file.
    McpConfig(PathBuf),
    /// Skills directory.
    SkillsDir(PathBuf),
    /// API provider config file (format varies by agent).
    ApiConfig(PathBuf),
    /// Generic config directory (for agents with custom formats).
    ConfigDir(PathBuf),
}

/// Per-agent configuration adapter.
///
/// Each supported agent implements this trait to declare how Zenix
/// should manage its config files for MCP servers, API providers,
/// and skills.
pub trait AgentConfigAdapter: Send + Sync {
    /// Short machine name (e.g. "claude").
    fn name(&self) -> &str;

    /// Human-readable display name (e.g. "Claude Code").
    fn display_name(&self) -> String;

    /// Binary names to check on PATH for detection.
    fn binary_names(&self) -> Vec<String>;

    /// Install command for this agent.
    fn install_command(&self) -> String;

    /// All config paths this adapter manages.
    /// Used for scanning and path resolution.
    fn config_paths(&self) -> Vec<ConfigPathSpec>;

    /// Generate env vars or config entries for an API provider.
    /// Returns the appropriate config data for this agent's format.
    fn api_env_vars(&self, provider: &ApiProviderConfig) -> Option<HashMap<String, String>>;

    /// Generate API provider config entry for this agent's format.
    /// Returns None if this agent doesn't support API provider config via entries.
    fn api_config_entry(&self, provider: &ApiProviderConfig) -> Option<serde_json::Value>;
}

/// Get the adapter for a given agent name.
pub fn adapter_for(agent_name: &str) -> Option<Box<dyn AgentConfigAdapter>> {
    let registry = ProviderRegistry::builtin();
    let _prov = registry.find(agent_name)?;

    // Map provider names to their adapter implementations
    match agent_name {
        "claude" => Some(Box::new(ClaudeAdapter)),
        "opencode" => Some(Box::new(OpenCodeAdapter)),
        "codex" => Some(Box::new(CodexAdapter)),
        "droid" | "pi" | "omp" | "kilo" | "hermes" => {
            Some(Box::new(AnthropicFamilyAdapter::new(agent_name)))
        }
        _ => None,
    }
}

/// Get all available adapters.
pub fn all_adapters() -> Vec<Box<dyn AgentConfigAdapter>> {
    vec![
        Box::new(ClaudeAdapter),
        Box::new(OpenCodeAdapter),
        Box::new(CodexAdapter),
        Box::new(AnthropicFamilyAdapter::new("droid")),
        Box::new(AnthropicFamilyAdapter::new("pi")),
        Box::new(AnthropicFamilyAdapter::new("omp")),
        Box::new(AnthropicFamilyAdapter::new("kilo")),
        Box::new(AnthropicFamilyAdapter::new("hermes")),
    ]
}

/// Get the config directory for an agent.
pub fn agent_config_dir(agent_name: &str) -> Option<PathBuf> {
    let registry = ProviderRegistry::builtin();
    let prov = registry.find(agent_name)?;
    (prov.config_dir)()
}

// ── Claude Adapter ──────────────────────────────────────────────────

/// Adapter for Claude Code (`~/.claude/claude_desktop_config.json`).
///
/// API config: writes `ANTHROPIC_*` env vars into the config JSON under `env` key.
/// MCP config: same file, under `mcpServers` key (handled by mcp/mod.rs).
/// Skills: `~/.claude/skills/` directory.
pub struct ClaudeAdapter;

impl AgentConfigAdapter for ClaudeAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    fn display_name(&self) -> String {
        "Claude Code".into()
    }

    fn binary_names(&self) -> Vec<String> {
        vec!["claude".into()]
    }

    fn install_command(&self) -> String {
        "npm install -g @anthropic-ai/claude-code".into()
    }

    fn config_paths(&self) -> Vec<ConfigPathSpec> {
        let config_dir = agent_config_dir("claude").unwrap_or_default();
        vec![
            ConfigPathSpec::McpConfig(config_dir.join("claude_desktop_config.json")),
            ConfigPathSpec::ApiConfig(config_dir.join("claude_desktop_config.json")),
            ConfigPathSpec::SkillsDir(config_dir.join("skills")),
        ]
    }

    fn api_env_vars(&self, provider: &ApiProviderConfig) -> Option<HashMap<String, String>> {
        Some(provider.to_claude_env())
    }

    fn api_config_entry(&self, _provider: &ApiProviderConfig) -> Option<serde_json::Value> {
        // Claude uses env vars, not a separate config entry
        None
    }
}

// ── OpenCode Adapter ────────────────────────────────────────────────

/// Adapter for OpenCode (`~/.opencode/config.json`).
///
/// API config: writes NPM provider entries into the config JSON.
/// MCP config: same file, under `mcpServers` key.
/// Skills: `~/.opencode/skills/` directory.
pub struct OpenCodeAdapter;

impl AgentConfigAdapter for OpenCodeAdapter {
    fn name(&self) -> &str {
        "opencode"
    }

    fn display_name(&self) -> String {
        "OpenCode".into()
    }

    fn binary_names(&self) -> Vec<String> {
        vec!["opencode".into()]
    }

    fn install_command(&self) -> String {
        "npm install -g @elizaos/opencode".into()
    }

    fn config_paths(&self) -> Vec<ConfigPathSpec> {
        let config_dir = agent_config_dir("opencode").unwrap_or_default();
        vec![
            ConfigPathSpec::McpConfig(config_dir.join("config.json")),
            ConfigPathSpec::ApiConfig(config_dir.join("config.json")),
            ConfigPathSpec::SkillsDir(config_dir.join("skills")),
        ]
    }

    fn api_env_vars(&self, _provider: &ApiProviderConfig) -> Option<HashMap<String, String>> {
        // OpenCode uses provider entries, not env vars
        None
    }

    fn api_config_entry(&self, provider: &ApiProviderConfig) -> Option<serde_json::Value> {
        Some(provider.to_opencode_provider())
    }
}

// ── Codex Adapter ───────────────────────────────────────────────────

/// Adapter for OpenAI Codex (`~/.codex/config.json`).
///
/// API config: writes OpenAI-compatible env vars into the config JSON.
/// MCP config: same file, under `mcpServers` key.
/// Skills: `~/.codex/skills/` directory.
pub struct CodexAdapter;

impl AgentConfigAdapter for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    fn display_name(&self) -> String {
        "OpenAI Codex".into()
    }

    fn binary_names(&self) -> Vec<String> {
        vec!["codex".into()]
    }

    fn install_command(&self) -> String {
        "npm install -g @openai/codex".into()
    }

    fn config_paths(&self) -> Vec<ConfigPathSpec> {
        let config_dir = agent_config_dir("codex").unwrap_or_default();
        vec![
            ConfigPathSpec::McpConfig(config_dir.join("config.json")),
            ConfigPathSpec::ApiConfig(config_dir.join("config.json")),
            ConfigPathSpec::SkillsDir(config_dir.join("skills")),
        ]
    }

    fn api_env_vars(&self, provider: &ApiProviderConfig) -> Option<HashMap<String, String>> {
        Some(provider.to_openai_env())
    }

    fn api_config_entry(&self, _provider: &ApiProviderConfig) -> Option<serde_json::Value> {
        None
    }
}

// ── Anthropic Family Adapter ────────────────────────────────────────

/// Shared adapter for Droid, Pi, OMP, Kilo, and Hermes.
///
/// All these agents use the same Anthropic-style config format:
/// - `~/<agent>/config.json` with `env` key for API config
/// - `mcpServers` key for MCP config
/// - `skills/` directory for skills
///
/// The `agent_name` field distinguishes between them for config path resolution.
pub struct AnthropicFamilyAdapter {
    agent_name: String,
}

impl AnthropicFamilyAdapter {
    pub fn new(agent_name: &str) -> Self {
        Self {
            agent_name: agent_name.to_string(),
        }
    }
}

impl AgentConfigAdapter for AnthropicFamilyAdapter {
    fn name(&self) -> &str {
        &self.agent_name
    }

    fn display_name(&self) -> String {
        match self.agent_name.as_str() {
            "droid" => "Droid".into(),
            "pi" => "Pi".into(),
            "omp" => "OMP".into(),
            "kilo" => "Kilo".into(),
            "hermes" => "Hermes".into(),
            _ => self.agent_name.clone(),
        }
    }

    fn binary_names(&self) -> Vec<String> {
        match self.agent_name.as_str() {
            "droid" => vec!["droid".into()],
            "pi" => vec!["pi".into()],
            "omp" => vec!["omp".into()],
            "kilo" => vec!["kilo".into()],
            "hermes" => vec!["hermes".into()],
            _ => vec![],
        }
    }

    fn install_command(&self) -> String {
        match self.agent_name.as_str() {
            "droid" => "npm install -g @anthropic-ai/droid".into(),
            "pi" => "npm install -g @anthropic-ai/pi".into(),
            "omp" => "npm install -g @anthropic-ai/omp".into(),
            "kilo" => "npm install -g @anthropic-ai/kilo".into(),
            "hermes" => "npm install -g @anthropic-ai/hermes".into(),
            _ => String::new(),
        }
    }

    fn config_paths(&self) -> Vec<ConfigPathSpec> {
        let config_dir = agent_config_dir(&self.agent_name).unwrap_or_default();
        vec![
            ConfigPathSpec::McpConfig(config_dir.join("config.json")),
            ConfigPathSpec::ApiConfig(config_dir.join("config.json")),
            ConfigPathSpec::SkillsDir(config_dir.join("skills")),
        ]
    }

    fn api_env_vars(&self, provider: &ApiProviderConfig) -> Option<HashMap<String, String>> {
        // These agents use Anthropic-style env vars (similar to Claude)
        Some(provider.to_claude_env())
    }

    fn api_config_entry(&self, _provider: &ApiProviderConfig) -> Option<serde_json::Value> {
        None
    }
}
