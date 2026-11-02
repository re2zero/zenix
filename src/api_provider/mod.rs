//! API Provider management — configure upstream AI API connections.
//!
//! Supports two API formats:
//!   - OpenAI-compatible (suffix options: /v1/chat/completions, /v1/responses)
//!   - Anthropic-compatible (suffix: /v1/messages)
//!
//! Each provider stores: name, format, base URL, API key, the models it
//! offers, the selected model (`model`), thinking level (discrete), and URL
//! suffix. An older file carries only `model`; [`ApiProviderConfig::sync_models`]
//! turns it into the list on load, and the model page edits the list.
//!
//! Config generation for different Agent CLI formats is provided
//! via `to_opencode_provider()` and `to_claude_env()` methods.
//!
//! Config is persisted to `~/.config/zenix/api-providers.json`.
//!
//! Consumers: `src/ai/engine.rs` picks a provider for a session (and reads its
//! `endpoint_url`) and `src/ui/settings.rs` lists them — both through `load_all`
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

    /// The token the picker carries as a row's value.
    pub fn token(&self) -> &'static str {
        match self {
            ApiFormat::OpenAI => "openai",
            ApiFormat::Anthropic => "anthropic",
        }
    }

    /// A format from its picker token.
    pub fn from_token(token: &str) -> Option<ApiFormat> {
        match token {
            "openai" => Some(ApiFormat::OpenAI),
            "anthropic" => Some(ApiFormat::Anthropic),
            _ => None,
        }
    }
}

// ── Models ──────────────────────────────────────────────────────────

/// One model a provider offers. The provider's `model` names the selected
/// one; the list is what the model page and the chat's picker show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConfig {
    /// The id the API takes (e.g. "gpt-4o").
    pub id: String,
    /// What the pickers call it; empty means the id.
    #[serde(default)]
    pub label: String,
    /// Whether it accepts image input.
    #[serde(default)]
    pub vision: bool,
    /// The model's context window in tokens, when it is known. The chat
    /// panel's context ring divides its last bill by this; a model without
    /// one shows only the bill. Absent from the file until someone sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
}

impl ModelConfig {
    /// A model known only by its id.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: String::new(),
            vision: false,
            context_window: None,
        }
    }

    /// The name to draw: the label when one is set, else the id.
    pub fn display_label(&self) -> &str {
        let label = self.label.trim();
        if label.is_empty() {
            self.id.trim()
        } else {
            label
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
    /// The selected model's id; one of `models`.
    pub model: String,
    /// Reasoning effort level.
    pub thinking_level: ThinkingLevel,
    /// URL suffix after base_url (e.g. "/v1/chat/completions").
    pub url_suffix: String,
    /// Agent CLIs this provider is applied to (e.g. ["claude", "opencode"]).
    #[serde(default)]
    pub enabled_agents: Vec<String>,
    /// The models this provider offers. An older file has only `model`; the
    /// list is filled on load, so a hand-written file keeps working.
    #[serde(default)]
    pub models: Vec<ModelConfig>,
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

    /// Make the selected id and the list agree. An older file's `model`
    /// becomes the list's only entry; a hand-edited id the list lacks is
    /// inserted so the model a turn runs on is never invisible; an empty
    /// selection falls back to the first listed model.
    pub fn sync_models(&mut self) {
        self.model = self.model.trim().to_owned();
        self.models.retain(|model| !model.id.trim().is_empty());
        if self.models.is_empty() {
            if !self.model.is_empty() {
                self.models.push(ModelConfig::new(self.model.clone()));
            }
            return;
        }
        if self.model.is_empty() {
            self.model = self.models[0].id.clone();
        } else if !self.models.iter().any(|model| model.id == self.model) {
            self.models.insert(0, ModelConfig::new(self.model.clone()));
        }
    }

    /// Point the provider at `id` when the list carries it.
    pub fn select_model(&mut self, id: &str) -> bool {
        if self.models.iter().any(|model| model.id == id) {
            self.model = id.to_owned();
            true
        } else {
            false
        }
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
    ///
    /// Every model the provider offers is listed; an older file with only a
    /// selected model lists that one.
    pub fn to_opencode_provider(&self) -> serde_json::Value {
        let base_url = self.endpoint_url();
        let mut models = serde_json::Map::new();
        for model in &self.models {
            if model.id.trim().is_empty() {
                continue;
            }
            models.insert(
                model.id.clone(),
                serde_json::json!({ "name": model.display_label() }),
            );
        }
        if models.is_empty() && !self.model.trim().is_empty() {
            models.insert(
                self.model.clone(),
                serde_json::json!({ "name": self.model }),
            );
        }

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
        Ok(mut providers) => {
            // The list is derived from `model` for an older file; doing it on
            // read means every consumer sees the same catalog without a
            // write, and the first save carries it to disk.
            for provider in &mut providers {
                ApiProviderConfig::sync_models(provider);
            }
            providers
        }
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
            models: vec![],
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
            models: vec![],
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
            models: vec![],
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
            models: vec![],
        };
        let env = cfg.to_openai_env();
        assert_eq!(env["OPENAI_API_KEY"], "sk-xxx");
        assert_eq!(env["OPENAI_BASE_URL"], "https://api.openai.com");
        assert_eq!(env["OPENAI_MODEL"], "gpt-4o");
    }

    /// A file written before the list existed still names its selected model;
    /// `sync_models` is what gives it the list every consumer reads.
    #[test]
    fn test_old_file_gains_its_model_in_the_list() {
        let mut cfg: ApiProviderConfig = serde_json::from_str(
            r#"{
                "name": "old",
                "format": "openai",
                "base_url": "https://api.openai.com",
                "api_key": "sk-x",
                "model": "gpt-4o",
                "thinking_level": "medium",
                "url_suffix": "/v1/chat/completions"
            }"#,
        )
        .expect("an older file has no models key");
        assert!(cfg.models.is_empty(), "serde(default) leaves it empty");
        cfg.sync_models();
        assert_eq!(cfg.models.len(), 1);
        assert_eq!(cfg.models[0].id, "gpt-4o");
        assert_eq!(cfg.model, "gpt-4o");
        assert!(!cfg.models[0].vision);
    }

    /// The list and the selected id always agree: an empty selection takes the
    /// first model, and an id the list lacks is kept by inserting it rather
    /// than silently running a model the page cannot show.
    #[test]
    fn test_sync_models_reconciles_the_selection() {
        let mut cfg = ApiProviderConfig {
            name: "p".into(),
            format: ApiFormat::OpenAI,
            base_url: "https://api.openai.com".into(),
            api_key: "sk-x".into(),
            model: "b".into(),
            thinking_level: ThinkingLevel::Medium,
            url_suffix: "/v1/chat/completions".into(),
            enabled_agents: vec![],
            models: vec![ModelConfig::new("a"), ModelConfig::new("b")],
        };
        cfg.sync_models();
        assert_eq!(cfg.model, "b");
        assert_eq!(cfg.models.len(), 2, "a selection already listed stays put");

        cfg.model.clear();
        cfg.sync_models();
        assert_eq!(cfg.model, "a", "nothing selected runs on the first model");

        cfg.model = "hand-written".into();
        cfg.sync_models();
        assert_eq!(cfg.models[0].id, "hand-written");
        assert_eq!(cfg.models.len(), 3);
    }

    /// An empty list and an empty selection stay empty: a provider with no
    /// models is a provider the chat cannot run, and the page says so.
    #[test]
    fn test_sync_models_leaves_a_model_less_provider_alone() {
        let mut cfg = ApiProviderConfig {
            name: "blank".into(),
            format: ApiFormat::OpenAI,
            base_url: "https://api.openai.com".into(),
            api_key: String::new(),
            model: String::new(),
            thinking_level: ThinkingLevel::Low,
            url_suffix: "/v1/chat/completions".into(),
            enabled_agents: vec![],
            models: vec![ModelConfig::new("  ")],
        };
        cfg.sync_models();
        assert!(cfg.models.is_empty());
        assert!(cfg.model.is_empty());
    }

    /// Opencode's provider entry lists the whole catalog; a model with no
    /// label goes by its id, and a file that predates the list still lists its
    /// one selected model.
    #[test]
    fn test_to_opencode_provider_lists_every_model() {
        let mut cfg = ApiProviderConfig {
            name: "deepin".into(),
            format: ApiFormat::OpenAI,
            base_url: "https://ai.xxx.com".into(),
            api_key: "ah-xxx".into(),
            model: "glm-5".into(),
            thinking_level: ThinkingLevel::High,
            url_suffix: "/v1/chat/completions".into(),
            enabled_agents: vec![],
            models: vec![
                ModelConfig {
                    id: "glm-5".into(),
                    label: "GLM 5".into(),
                    vision: true,
                    context_window: Some(200_000),
                },
                ModelConfig::new("glm-5-air"),
            ],
        };
        cfg.sync_models();
        let prov = cfg.to_opencode_provider();
        assert_eq!(prov["models"]["glm-5"]["name"], "GLM 5");
        assert_eq!(prov["models"]["glm-5-air"]["name"], "glm-5-air");
    }

    /// A model entry is at least its id: the two capabilities default to off,
    /// so an older list written by hand still reads.
    #[test]
    fn test_model_config_defaults() {
        let model: ModelConfig = serde_json::from_str(r#"{"id":"x"}"#).expect("id alone is enough");
        assert_eq!(model.label, "");
        assert!(!model.vision);
        assert_eq!(model.display_label(), "x");
    }

    #[test]
    fn test_format_tokens_round_trip() {
        for format in [ApiFormat::OpenAI, ApiFormat::Anthropic] {
            assert_eq!(ApiFormat::from_token(format.token()), Some(format));
        }
        assert_eq!(ApiFormat::from_token("other"), None);
    }
}
