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

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::platform;

// ── Thinking Level ──────────────────────────────────────────────────

/// Discrete reasoning-effort level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingLevel {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

impl Default for ThinkingLevel {
    fn default() -> Self {
        ThinkingLevel::Medium
    }
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
}

// ── API Format ──────────────────────────────────────────────────────

/// A preset API provider configuration.
#[derive(Debug, Clone)]
pub struct ProviderPreset {
    pub name: &'static str,
    pub base_url: &'static str,
    pub suffix: &'static str,
    pub format: ApiFormat,
}

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

    /// All preset providers for this format.
    pub fn presets(&self) -> Vec<ProviderPreset> {
        match self {
            ApiFormat::OpenAI => vec![
                ProviderPreset {
                    name: "OpenAI",
                    base_url: "https://api.openai.com/v1",
                    suffix: "/v1/chat/completions",
                    format: ApiFormat::OpenAI,
                },
                ProviderPreset {
                    name: "Groq",
                    base_url: "https://api.groq.com/openai/v1",
                    suffix: "/v1/chat/completions",
                    format: ApiFormat::OpenAI,
                },
                ProviderPreset {
                    name: "DeepSeek",
                    base_url: "https://api.deepseek.com/v1",
                    suffix: "/v1/chat/completions",
                    format: ApiFormat::OpenAI,
                },
                ProviderPreset {
                    name: "Together",
                    base_url: "https://api.together.xyz/v1",
                    suffix: "/v1/chat/completions",
                    format: ApiFormat::OpenAI,
                },
                ProviderPreset {
                    name: "OpenRouter",
                    base_url: "https://openrouter.ai/api/v1",
                    suffix: "/v1/chat/completions",
                    format: ApiFormat::OpenAI,
                },
                ProviderPreset {
                    name: "Ollama",
                    base_url: "http://localhost:11434/v1",
                    suffix: "/v1/chat/completions",
                    format: ApiFormat::OpenAI,
                },
            ],
            ApiFormat::Anthropic => vec![ProviderPreset {
                name: "Anthropic",
                base_url: "https://api.anthropic.com/v1",
                suffix: "/v1/messages",
                format: ApiFormat::Anthropic,
            }],
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

    /// URL for fetching available models.
    pub fn models_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{base}{}", self.format.models_path())
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

// ── Agent Sync ───────────────────────────────────────────────────────

/// Sync a provider to a specific agent CLI's config file.
///
/// Uses the `AgentConfigAdapter` for the target agent to determine
/// the correct config format and file path.
pub fn sync_api_to_agent(provider: &ApiProviderConfig, agent_name: &str) -> Result<(), String> {
    let adapter = crate::config_adapters::adapters::adapter_for(agent_name)
        .ok_or_else(|| format!("unknown or unsupported agent: {agent_name}"))?;

    // Get the API config file path from the adapter
    let config_path = get_api_config_path(agent_name)?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }

    // Check if the adapter generates env vars or provider entries
    if let Some(env_vars) = adapter.api_env_vars(provider) {
        // Write env vars to the config file under "env" key
        let mut map = crate::config::scanner::ConfigMap::load(&config_path)?;
        let env_value: serde_json::Value = env_vars
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect::<serde_json::Map<_, _>>()
            .into();
        map.upsert(&["env"], env_value);
        map.save()
    } else if let Some(entry) = adapter.api_config_entry(provider) {
        // Write provider entry directly to the config file
        let mut map = crate::config::scanner::ConfigMap::load(&config_path)?;
        map.upsert(&[&provider.name], entry);
        map.save()
    } else {
        Err(format!(
            "agent '{agent_name}' does not support API provider configuration"
        ))
    }
}

/// Remove a provider from a specific agent's config.
pub fn unsync_api_from_agent(provider_name: &str, agent_name: &str) -> Result<(), String> {
    // Verify the agent is supported
    let _adapter = crate::config_adapters::adapters::adapter_for(agent_name)
        .ok_or_else(|| format!("unknown or unsupported agent: {agent_name}"))?;

    let config_path = get_api_config_path(agent_name)?;

    if !config_path.exists() {
        return Ok(());
    }

    let mut map = crate::config::scanner::ConfigMap::load(&config_path)?;

    // For OpenCode: remove the provider entry by name
    if agent_name == "opencode" {
        map.remove(&[provider_name]);
        map.save()?;
        return Ok(());
    }

    // For all other agents: check if other providers also use this agent
    // before removing env keys (to avoid deleting another provider's config)
    let other_providers_use_agent = {
        let providers = load_all();
        providers
            .iter()
            .filter(|p| p.name != provider_name)
            .any(|p| p.enabled_agents.iter().any(|a| a == agent_name))
    };

    if other_providers_use_agent {
        // Another provider also uses this agent — don't remove env keys
        // They might be needed by the other provider
        return Ok(());
    }

    // Safe to remove known env keys
    let known_keys: Vec<&str> = vec![
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_MODEL",
    ];

    // Only remove keys that are in the env section
    if let Some(env_obj) = map.get(&["env"]).and_then(|v| v.as_object()) {
        let keys_to_remove: Vec<&&str> = known_keys
            .iter()
            .filter(|key| env_obj.contains_key(**key))
            .collect();

        if !keys_to_remove.is_empty() {
            for key in keys_to_remove {
                map.remove(&["env", *key]);
            }
            map.save()?;
        }
    }

    Ok(())
}

/// Get the API config file path for an agent.
fn get_api_config_path(agent_name: &str) -> Result<std::path::PathBuf, String> {
    let registry = crate::provider::ProviderRegistry::builtin();
    let prov = registry
        .find(agent_name)
        .ok_or_else(|| format!("unknown agent: {agent_name}"))?;

    // Use the provider's mcp_config_file if available, otherwise fall back to config_dir/config.json
    prov.mcp_config_path()
        .or_else(|| {
            (prov.config_dir)().map(|d| d.join("config.json"))
        })
        .ok_or_else(|| format!("no config path for agent: {agent_name}"))
}

/// Toggle a provider's enabled state for an agent, syncing the config.
pub fn toggle_api_agent(provider: &mut ApiProviderConfig, agent_name: &str) -> Result<(), String> {
    let enabled = provider.enabled_agents.iter().any(|a| a == agent_name);
    if enabled {
        provider.enabled_agents.retain(|a| a != agent_name);
        unsync_api_from_agent(&provider.name, agent_name)?;
    } else {
        provider.enabled_agents.push(agent_name.to_string());
        sync_api_to_agent(provider, agent_name)?;
    }
    upsert(provider)
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
        Err(_) => return vec![],
    };
    serde_json::from_str(&content).unwrap_or_default()
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

/// Add or update a provider by name.
pub fn upsert(config: &ApiProviderConfig) -> Result<(), String> {
    let mut all = load_all();
    all.retain(|p| p.name != config.name);
    all.push(config.clone());
    save_all(&all)
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

/// Test connection to an API provider endpoint.
pub fn test_connection(config: &ApiProviderConfig) -> Result<String, String> {
    use std::time::Duration;
    let url = config.models_url();
    match ureq::get(&url)
        .set("Authorization", &format!("Bearer {}", config.api_key))
        .set("Content-Type", "application/json")
        .timeout(Duration::from_secs(10))
        .call()
    {
        Ok(resp) => {
            let status = resp.status();
            if status == 200 || status == 401 || status == 403 || status == 400 {
                Ok(format!("OK: endpoint reachable (HTTP {status})"))
            } else {
                Err(format!("HTTP {status}: unexpected response"))
            }
        }
        Err(e) => Err(format!("Connection failed: {e}")),
    }
}

// ── Model fetching ───────────────────────────────────────────────────

/// Fetch available model names from an API provider's /v1/models endpoint.
pub fn fetch_models(config: &ApiProviderConfig) -> Result<Vec<String>, String> {
    let url = config.models_url();
    let response = ureq::get(&url)
        .set("Authorization", &format!("Bearer {}", config.api_key))
        .set("Content-Type", "application/json")
        .call()
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let body = response
        .into_string()
        .map_err(|e| format!("read response: {e}"))?;

    parse_openai_model_list(&body)
}

fn parse_openai_model_list(json: &str) -> Result<Vec<String>, String> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("parse JSON: {e}"))?;

    let data = root
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "response missing 'data' array".to_string())?;

    let models: Vec<String> = data
        .iter()
        .filter_map(|entry| entry.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    if models.is_empty() {
        return Err("no models found in response".into());
    }

    Ok(models)
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

    #[test]
    fn test_parse_openai_model_list() {
        let json = r#"{"object":"list","data":[{"id":"gpt-4o","object":"model"},{"id":"gpt-4-turbo","object":"model"}]}"#;
        let models = parse_openai_model_list(json).unwrap();
        assert_eq!(models, vec!["gpt-4o", "gpt-4-turbo"]);
    }
}
