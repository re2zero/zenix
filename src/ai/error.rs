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
#[allow(dead_code)]
pub fn user_friendly(err: &AiError) -> String {
    match err {
        AiError::ConfigMissing(msg) => {
            if msg.contains("provider") {
                "请先在「设置 → 扩展 → API 提供商」中配置一个 API Provider。".into()
            } else if msg.contains("model") {
                "请先在 API 提供商中选择或输入一个模型名称。".into()
            } else {
                format!("AI 配置不完整：{msg}")
            }
        }
        AiError::ApiUnavailable(msg) => {
            format!(
                "无法连接 AI 服务。请检查 API 配置和网络连接：{}",
                truncate_err(msg, 60)
            )
        }
        AiError::ApiError(msg) => {
            if msg.contains("401") {
                "API Key 无效或已过期，请检查 API 提供商配置。".into()
            } else if msg.contains("403") {
                "API 权限不足，请检查 API 提供商配置。".into()
            } else if msg.contains("429") {
                "请求过于频繁，请稍后重试。".into()
            } else if msg.contains("400") {
                "模型请求格式有误，请检查模型名称和配置。".into()
            } else {
                format!("AI 服务返回错误：{}", truncate_err(msg, 80))
            }
        }
        AiError::AgentCliNotFound(agent) => {
            format!("Agent CLI「{agent}」未安装。请在「设置 → 扩展 → Agent CLI」中安装该工具。",)
        }
        AiError::IpcError(msg) => {
            format!("herdr 通信失败：{}", truncate_err(msg, 60))
        }
        AiError::SerializationError(msg) => {
            format!("数据序列化失败：{}", truncate_err(msg, 60))
        }
        AiError::Timeout(msg) => {
            format!("操作超时：{}", truncate_err(msg, 40))
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
    }

    #[test]
    fn test_user_friendly_401() {
        let e = AiError::ApiError("HTTP 401 unauthorized".into());
        let msg = user_friendly(&e);
        assert!(msg.contains("API Key"));
    }

    #[test]
    fn test_user_friendly_429() {
        let e = AiError::ApiError("HTTP 429 rate limit".into());
        let msg = user_friendly(&e);
        assert!(msg.contains("请求过于频繁"));
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
