//! API Provider management — configure upstream AI API connections.
//!
//! Supports two API formats:
//!   - OpenAI-compatible (suffix options: /v1/chat/completions, /v1/responses)
//!   - Anthropic-compatible (suffix: /v1/messages)
//!
//! Each provider stores: name, format, base URL, API key, model,
//! thinking level (discrete), and URL suffix.
//!
//! Config generation for different Agent CLI formats is provided
//! via `to_opencode_provider()` and `to_claude_env()` methods.
//!
//! Config is persisted to `~/.config/zenix/api-providers.json`.
//!
//! Consumers: `src/ai/engine.rs` picks a provider for a session (and reads its
//! `endpoint_url`) and `src/shell/chat.rs` lists them — both through `load_all`
//! and `ApiProviderConfig`. The per-agent sync path, the presets, model fetching
//! and the connection test had no caller and are gone. The config-generation
//! methods (`to_opencode_provider`, `to_claude_env`, `to_openai_env`) stay
//! because the adapter implementations in `src/config_adapters/adapters` call
//! them. The store's write half (`save_all`, `remove`) is still unwired.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::platform;

// ── Thinking Level ──────────────────────────────────────────────────

/// Discrete reasoning-effort level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThinkingLevel {
    #[serde(rename = "low")]
    #[default]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

impl ThinkingLevel {
    pub fn label(&self) -> &'static str {
        match self {
            ThinkingLevel::Low => "Low",
            ThinkingLevel::Medium => "Medium",
            ThinkingLevel::High => "High",
            ThinkingLevel::XHigh => "X-High",
        }
    }

    pub fn all() -> &'static [ThinkingLevel] {
        &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::XHigh,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::XHigh => "xhigh",
        }
    }

    /// Display label for UI rendering (e.g. "Medium", not "medium").
    pub fn display_label(&self) -> &'static str {
        match self {
            ThinkingLevel::Low => "Low",
            ThinkingLevel::Medium => "Medium",
            ThinkingLevel::High => "High",
            ThinkingLevel::XHigh => "X-High",
        }
    }

    pub fn parse_token(s: &str) -> Option<ThinkingLevel> {
        match s {
            "low" => Some(ThinkingLevel::Low),
            "medium" => Some(ThinkingLevel::Medium),
            "high" => Some(ThinkingLevel::High),
            "xhigh" => Some(ThinkingLevel::XHigh),
            _ => None,
        }
    }
}

// ── API Format ──────────────────────────────────────────────────────

/// Supported API format families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiFormat {
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "anthropic")]
    Anthropic,
}

impl ApiFormat {
    /// Default base URL for the official API.
    pub fn default_base_url(&self) -> &'static str {
        match self {
            ApiFormat::OpenAI => "https://api.openai.com",
            ApiFormat::Anthropic => "https://api.anthropic.com",
        }
    }

    /// Available URL suffix options for this format (user selects one).
    pub fn suffix_options(&self) -> &'static [&'static str] {
        match self {
            ApiFormat::OpenAI => &["/v1/chat/completions", "/v1/responses"],
            ApiFormat::Anthropic => &["/v1/messages"],
        }
    }

    /// Default suffix (first option).
    pub fn default_suffix(&self) -> &'static str {
        self.suffix_options()[0]
    }

    /// API path to list available models.
    pub fn models_path(&self) -> &'static str {
        "/v1/models"
    }

    /// Display label.
    pub fn label(&self) -> &'static str {
        match self {
            ApiFormat::OpenAI => "OpenAI",
            ApiFormat::Anthropic => "Anthropic",
        }
    }
}

// ── API Provider Config ─────────────────────────────────────────────

/// A configured API provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiProviderConfig {
    /// User-given name for this provider (e.g. "My OpenAI").
    pub name: String,
    /// API format family.
    pub format: ApiFormat,
    /// Base URL (without trailing slash, e.g. "https://api.openai.com").
    pub base_url: String,
    /// API key (masked in UI).
    pub api_key: String,
    /// Currently selected model name.
    pub model: String,
    /// Reasoning effort level.
    pub thinking_level: ThinkingLevel,
    /// URL suffix after base_url (e.g. "/v1/chat/completions").
    pub url_suffix: String,
    /// Agent CLIs this provider is applied to (e.g. ["claude", "opencode"]).
    #[serde(default)]
    pub enabled_agents: Vec<String>,
}

impl ApiProviderConfig {
    /// Full endpoint URL: base_url + url_suffix.
    pub fn endpoint_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        let suffix = if self.url_suffix.starts_with('/') {
            &self.url_suffix
        } else {
            return format!("{base}/{}", self.url_suffix);
        };
        format!("{base}{suffix}")
    }
}

// ── Agent CLI Config Generation ─────────────────────────────────────

/// OpenCode provider config: npm package + options + models map.
///
/// Output format:
/// ```json
/// {
///   "npm": "@ai-sdk/openai-compatible",
///   "options": { "baseURL": "...", "apiKey": "..." },
///   "models": { "gpt-4o": { "name": "gpt-4o" } }
/// }
/// ```
impl ApiProviderConfig {
    /// Generate an OpenCode-compatible provider entry.
    pub fn to_opencode_provider(&self) -> serde_json::Value {
        let base_url = self.endpoint_url();
        let mut models = serde_json::Map::new();
        models.insert(
            self.model.clone(),
            serde_json::json!({ "name": self.model }),
        );

        let mut options = serde_json::Map::new();
        options.insert("baseURL".into(), serde_json::Value::String(base_url));
        options.insert(
            "apiKey".into(),
            serde_json::Value::String(self.api_key.clone()),
        );

        serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": options,
            "models": models,
        })
    }
}

/// Claude Code config: writes env vars to Claude's config.
///
/// Output is a map of env vars to write into Claude's `claude_desktop_config.json`:
/// ```json
/// {
///   "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
///   "ANTHROPIC_AUTH_TOKEN": "ah-xxx",
///   "ANTHROPIC_MODEL": "claude-sonnet-4-20250514",
///   "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-sonnet-4-20250514",
///   "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-20250514",
///   "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-sonnet-4-20250514"
/// }
/// ```
impl ApiProviderConfig {
    /// Generate Claude Code environment variable overrides.
    pub fn to_claude_env(&self) -> HashMap<String, String> {
        let base = self.base_url.trim_end_matches('/').to_string();
        let mut env = HashMap::new();
        env.insert("ANTHROPIC_BASE_URL".into(), base);
        env.insert("ANTHROPIC_AUTH_TOKEN".into(), self.api_key.clone());
        env.insert("ANTHROPIC_MODEL".into(), self.model.clone());
        env.insert("ANTHROPIC_DEFAULT_HAIKU_MODEL".into(), self.model.clone());
        env.insert("ANTHROPIC_DEFAULT_SONNET_MODEL".into(), self.model.clone());
        env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".into(), self.model.clone());
        env
    }

    /// Generate generic OpenAI-compatible environment variables
    /// (suitable for Codex, Cursor, and other OpenAI-format agents).
    pub fn to_openai_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("OPENAI_API_KEY".into(), self.api_key.clone());
        env.insert(
            "OPENAI_BASE_URL".into(),
            self.base_url.trim_end_matches('/').to_string(),
        );
        env.insert("OPENAI_MODEL".into(), self.model.clone());
        env
    }
}

// ── Zenix Provider ────────────────────────────────────────────────────

/// Return the first configured "Zenix" provider, if any.
///
/// A Zenix provider is identified by name == "Zenix" (case-sensitive).
/// If multiple Zenix providers exist, the first one is returned.
pub fn zenix_provider() -> Option<ApiProviderConfig> {
    load_all().into_iter().find(|p| p.name == "Zenix")
}

/// Return all configured "Zenix" providers.
pub fn zenix_providers() -> Vec<ApiProviderConfig> {
    load_all()
        .into_iter()
        .filter(|p| p.name == "Zenix")
        .collect()
}

// ── Persistence ──────────────────────────────────────────────────────

fn config_path() -> PathBuf {
    platform::zenix_config_dir().join("api-providers.json")
}

/// Load all configured API providers from disk.
pub fn load_all() -> Vec<ApiProviderConfig> {
    let path = config_path();
    if !path.exists() {
        return vec![];
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("failed to read api-providers config: {e}");
            return vec![];
        }
    };
    match serde_json::from_str(&content) {
        Ok(providers) => providers,
        Err(e) => {
            tracing::warn!("failed to parse api-providers config: {e}");
            vec![]
        }
    }
}

/// Save all API providers to disk.
pub fn save_all(providers: &[ApiProviderConfig]) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(providers).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

/// Remove a provider by name.
pub fn remove(name: &str) -> Result<(), String> {
    let mut all = load_all();
    let before = all.len();
    all.retain(|p| p.name != name);
    if all.len() == before {
        return Err(format!("provider '{name}' not found"));
    }
    save_all(&all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinking_level_all() {
        assert_eq!(ThinkingLevel::all().len(), 4);
    }

    #[test]
    fn test_suffix_options_openai() {
        let opts = ApiFormat::OpenAI.suffix_options();
        assert!(opts.contains(&"/v1/chat/completions"));
        assert!(opts.contains(&"/v1/responses"));
    }

    #[test]
    fn test_suffix_options_anthropic() {
        let opts = ApiFormat::Anthropic.suffix_options();
        assert!(opts.contains(&"/v1/messages"));
    }

    #[test]
    fn test_endpoint_url() {
        let cfg = ApiProviderConfig {
            name: "test".into(),
            format: ApiFormat::OpenAI,
            base_url: "https://api.openai.com".into(),
            api_key: "sk-test".into(),
            model: "gpt-4o".into(),
            thinking_level: ThinkingLevel::Medium,
            url_suffix: "/v1/chat/completions".into(),
            enabled_agents: vec![],
        };
        assert_eq!(
            cfg.endpoint_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_to_opencode_provider() {
        let cfg = ApiProviderConfig {
            name: "deepin".into(),
            format: ApiFormat::OpenAI,
            base_url: "https://ai.xxx.com".into(),
            api_key: "ah-xxx".into(),
            model: "glm-5".into(),
            thinking_level: ThinkingLevel::High,
            url_suffix: "/v1/chat/completions".into(),
            enabled_agents: vec![],
        };
        let prov = cfg.to_opencode_provider();
        assert_eq!(prov["npm"].as_str().unwrap(), "@ai-sdk/openai-compatible");
        assert_eq!(
            prov["options"]["baseURL"].as_str().unwrap(),
            "https://ai.xxx.com/v1/chat/completions"
        );
        assert_eq!(prov["models"]["glm-5"]["name"].as_str().unwrap(), "glm-5");
    }

    #[test]
    fn test_to_claude_env() {
        let cfg = ApiProviderConfig {
            name: "test".into(),
            format: ApiFormat::Anthropic,
            base_url: "https://ai.xxx.com".into(),
            api_key: "ah-xxx".into(),
            model: "glm-5".into(),
            thinking_level: ThinkingLevel::Low,
            url_suffix: "/v1/messages".into(),
            enabled_agents: vec![],
        };
        let env = cfg.to_claude_env();
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://ai.xxx.com");
        assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "ah-xxx");
        assert_eq!(env["ANTHROPIC_MODEL"], "glm-5");
        assert_eq!(env["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "glm-5");
    }

    #[test]
    fn test_to_openai_env() {
        let cfg = ApiProviderConfig {
            name: "test".into(),
            format: ApiFormat::OpenAI,
            base_url: "https://api.openai.com".into(),
            api_key: "sk-xxx".into(),
            model: "gpt-4o".into(),
            thinking_level: ThinkingLevel::Medium,
            url_suffix: "/v1/chat/completions".into(),
            enabled_agents: vec![],
        };
        let env = cfg.to_openai_env();
        assert_eq!(env["OPENAI_API_KEY"], "sk-xxx");
        assert_eq!(env["OPENAI_BASE_URL"], "https://api.openai.com");
        assert_eq!(env["OPENAI_MODEL"], "gpt-4o");
    }
}
