//! Error handling for the AI orchestrator module.

use std::fmt;

/// AI orchestrator errors.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AiError {
    /// Required configuration (provider/model) is missing.
    ConfigMissing(String),
    /// The configured API provider is unreachable or offline.
    ApiUnavailable(String),
    /// An error from the upstream LLM API (HTTP error, invalid response, etc).
    ApiError(String),
    /// The requested Agent CLI binary is not found on the system.
    AgentCliNotFound(String),
    /// Error communicating with herdr IPC (socket, JSON parse, etc).
    IpcError(String),
    /// Failed to serialize or deserialize JSON.
    SerializationError(String),
    /// A timeout occurred (LLM call, IPC, etc).
    Timeout(String),
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::ConfigMissing(msg) => write!(f, "config missing: {msg}"),
            AiError::ApiUnavailable(msg) => write!(f, "API unavailable: {msg}"),
            AiError::ApiError(msg) => write!(f, "API error: {msg}"),
            AiError::AgentCliNotFound(agent) => {
                write!(f, "agent CLI not found: {agent}")
            }
            AiError::IpcError(msg) => write!(f, "IPC error: {msg}"),
            AiError::SerializationError(msg) => write!(f, "serialization error: {msg}"),
            AiError::Timeout(msg) => write!(f, "timeout: {msg}"),
        }
    }
}

impl std::error::Error for AiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl From<anyhow::Error> for AiError {
    fn from(err: anyhow::Error) -> Self {
        let msg = err.to_string();
        if msg.contains("timeout") || msg.contains("timed out") || msg.contains("deadline") {
            return AiError::Timeout(msg);
        }
        if msg.contains("connection refused")
            || msg.contains("No connection")
            || msg.contains("Could not connect")
        {
            return AiError::ApiUnavailable(msg);
        }
        if msg.contains("401") || msg.contains("403") || msg.contains("auth") {
            return AiError::ApiError(msg);
        }
        AiError::ApiError(msg)
    }
}

/// Returns a user-friendly error message suitable for display in the UI.
///
/// Resolved through the locale table (`ai.error.*`): the guidance names the
/// surfaces that actually exist — `~/.config/zenix/api-providers.json` for
/// providers and `PATH` for agent CLIs — not the settings pages of an older
/// information architecture.
#[allow(dead_code)]
pub fn user_friendly(err: &AiError) -> String {
    match err {
        AiError::ConfigMissing(msg) => {
            if msg.contains("provider") {
                crate::i18n::t("ai.error.config_provider")
            } else if msg.contains("model") {
                crate::i18n::t("ai.error.config_model")
            } else {
                rust_i18n::t!("ai.error.config_generic", msg = msg).to_string()
            }
        }
        AiError::ApiUnavailable(msg) => {
            rust_i18n::t!("ai.error.api_unavailable", msg = truncate_err(msg, 60)).to_string()
        }
        AiError::ApiError(msg) => {
            if msg.contains("401") {
                crate::i18n::t("ai.error.api_401")
            } else if msg.contains("403") {
                crate::i18n::t("ai.error.api_403")
            } else if msg.contains("429") {
                crate::i18n::t("ai.error.api_429")
            } else if msg.contains("400") {
                crate::i18n::t("ai.error.api_400")
            } else {
                rust_i18n::t!("ai.error.api_error", msg = truncate_err(msg, 80)).to_string()
            }
        }
        AiError::AgentCliNotFound(agent) => {
            rust_i18n::t!("ai.error.agent_cli_missing", agent = agent).to_string()
        }
        AiError::IpcError(msg) => {
            rust_i18n::t!("ai.error.ipc", msg = truncate_err(msg, 60)).to_string()
        }
        AiError::SerializationError(msg) => {
            rust_i18n::t!("ai.error.serialization", msg = truncate_err(msg, 60)).to_string()
        }
        AiError::Timeout(msg) => {
            rust_i18n::t!("ai.error.timeout", msg = truncate_err(msg, 40)).to_string()
        }
    }
}

/// Returns true if the error is considered retryable.
#[allow(dead_code)]
pub fn retryable(err: &AiError) -> bool {
    matches!(
        err,
        AiError::ApiUnavailable(_)
            | AiError::ApiError(_)
            | AiError::Timeout(_)
            | AiError::IpcError(_)
    )
}

/// Truncate a string for display in error messages, preserving UTF-8 boundaries.
fn truncate_err(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // `max` counts bytes, so the cut has to fall back to the nearest character
    // boundary: a provider's error text is often Chinese, and slicing it at a
    // byte offset would panic inside the very path that reports failures.
    let keep = max.saturating_sub(3);
    let end = s
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= keep)
        .last()
        .unwrap_or(0);
    format!("{}...", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        let e = AiError::ConfigMissing("no provider".into());
        assert!(format!("{e}").contains("config missing"));
        assert!(format!("{e}").contains("no provider"));

        let e = AiError::AgentCliNotFound("claude".into());
        assert!(format!("{e}").contains("claude"));

        let e = AiError::Timeout("request timed out".into());
        assert!(format!("{e}").contains("timeout"));
    }

    #[test]
    fn test_user_friendly_config_missing() {
        let e = AiError::ConfigMissing("no provider".into());
        let msg = user_friendly(&e);
        assert!(msg.contains("API Provider"));
        // The guidance names the surface that exists, not a settings page of
        // an older information architecture.
        assert!(msg.contains("api-providers.json"));
    }

    #[test]
    fn test_user_friendly_401() {
        let e = AiError::ApiError("HTTP 401 unauthorized".into());
        let msg = user_friendly(&e);
        assert!(msg.contains("api-providers.json"));
    }

    #[test]
    fn test_user_friendly_429() {
        let e = AiError::ApiError("HTTP 429 rate limit".into());
        let msg = user_friendly(&e);
        assert!(msg.contains("Too many requests"));
    }

    #[test]
    fn no_error_message_points_at_a_settings_page() {
        let errors = [
            AiError::ConfigMissing("no provider".into()),
            AiError::ConfigMissing("no model".into()),
            AiError::ConfigMissing("something else".into()),
            AiError::ApiUnavailable("closed".into()),
            AiError::ApiError("HTTP 401".into()),
            AiError::ApiError("HTTP 403".into()),
            AiError::ApiError("HTTP 429".into()),
            AiError::ApiError("HTTP 400".into()),
            AiError::ApiError("HTTP 500 boom".into()),
            AiError::AgentCliNotFound("claude".into()),
            AiError::IpcError("broken pipe".into()),
            AiError::SerializationError("bad json".into()),
            AiError::Timeout("slow".into()),
        ];
        for err in &errors {
            let msg = user_friendly(err);
            assert!(!msg.contains("设置 → 扩展"), "{msg:?}");
            assert!(!msg.is_empty(), "{err:?}");
        }
    }

    #[test]
    fn truncating_a_multibyte_error_stays_on_character_boundaries() {
        let text = "服务返回错误：这是一段很长的中文错误消息，用来确认截断不会落在字符中间。";
        let cut = truncate_err(text, 30);
        assert!(cut.ends_with("..."));
        assert!(cut.len() <= 30);
        assert!(text.starts_with(cut.trim_end_matches("...")));
    }

    #[test]
    fn test_retryable() {
        assert!(retryable(&AiError::ApiUnavailable("closed".into())));
        assert!(retryable(&AiError::ApiError("500".into())));
        assert!(retryable(&AiError::Timeout("slow".into())));
        assert!(retryable(&AiError::IpcError("broken".into())));
        assert!(!retryable(&AiError::ConfigMissing("x".into())));
        assert!(!retryable(&AiError::AgentCliNotFound("x".into())));
        assert!(!retryable(&AiError::SerializationError("x".into())));
    }

    #[test]
    fn test_from_anyhow_timeout() {
        let err: anyhow::Error = anyhow::anyhow!("connection deadline exceeded");
        let ai_err = AiError::from(err);
        assert!(matches!(ai_err, AiError::Timeout(_)));
    }

    #[test]
    fn test_from_anyhow_unavailable() {
        let err: anyhow::Error = anyhow::anyhow!("connection refused: 127.0.0.1:8080");
        let ai_err = AiError::from(err);
        assert!(matches!(ai_err, AiError::ApiUnavailable(_)));
    }

    #[test]
    fn test_from_anyhow_api_error() {
        let err: anyhow::Error = anyhow::anyhow!("HTTP 401 unauthorized");
        let ai_err = AiError::from(err);
        assert!(matches!(ai_err, AiError::ApiError(_)));
    }

    #[test]
    fn test_from_anyhow_default() {
        let err: anyhow::Error = anyhow::anyhow!("some unexpected error");
        let ai_err = AiError::from(err);
        assert!(matches!(ai_err, AiError::ApiError(_)));
    }
}
