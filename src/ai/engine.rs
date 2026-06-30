//! Orchestrator engine — LLM API client with SSE streaming support.
//!
//! Uses `reqwest` with tokio runtime to communicate with OpenAI/Anthropic/
//! Ollama-compatible endpoints via the standard Chat Completions API.

use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};

use crate::api_provider::ApiProviderConfig;

use super::error::AiError;

/// System prompt defining the orchestrator's role.
pub const SYSTEM_PROMPT: &str =
    "你是 ZenixAI 编排器。你负责理解用户需求、规划任务分解、将具体执行工作分配给专业 Agent CLI（如 Claude Code、Codex、OpenCode 等）。你不直接写代码或运行命令，只负责规划、调度和结果汇总。用中文回复。";

/// Lightweight chat message reference for the engine (no session/sequence).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageRef {
    /// "user" or "orchestrator".
    pub role: String,
    /// Message content.
    pub content: String,
}

/// Events emitted during an LLM streaming call.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A token chunk from the LLM response.
    Token(String),
    /// An error occurred during streaming.
    Error(String),
    /// The stream has finished.
    Done,
}

/// Orchestrator LLM engine.
pub struct OrchestratorEngine {
    http_client: reqwest::Client,
    endpoint_url: String,
    model: String,
    api_key: String,
}

/// SSE response chunk from the LLM API.
#[derive(Debug, Deserialize)]
struct SseChunk {
    id: Option<String>,
    choices: Option<Vec<SseChoice>>,
}

#[derive(Debug, Deserialize)]
struct SseChoice {
    delta: Option<SseDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseDelta {
    content: Option<String>,
    role: Option<String>,
}

impl OrchestratorEngine {
    /// Create a new engine from an API provider configuration.
    ///
    /// Validates that the provider has a valid base_url, api_key, and model
    /// configured. The HTTP client uses a 120-second timeout.
    pub fn new(provider: &ApiProviderConfig) -> Result<Self, AiError> {
        if provider.base_url.is_empty() {
            return Err(AiError::ConfigMissing("base_url is empty".into()));
        }
        if provider.api_key.is_empty() {
            return Err(AiError::ConfigMissing("api_key is empty".into()));
        }
        if provider.model.is_empty() {
            return Err(AiError::ConfigMissing("model is empty".into()));
        }

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .user_agent("ZenixAI/0.1")
            .build()
            .context("build reqwest client")
            .map_err(AiError::from)?;

        Ok(Self {
            http_client,
            endpoint_url: provider.endpoint_url(),
            model: provider.model.clone(),
            api_key: provider.api_key.clone(),
        })
    }

    /// Send a streaming chat request to the LLM API.
    ///
    /// The system prompt is prepended as the first message if not already present.
    /// Calls `on_event` with `StreamEvent::Token(content)` for each token chunk,
    /// `StreamEvent::Error(msg)` on failure, and `StreamEvent::Done` at completion.
    ///
    /// Uses `reqwest::bytes_stream()` to parse SSE "data: " lines.
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessageRef],
        on_event: impl Fn(StreamEvent) + Send + 'static,
    ) -> Result<(), AiError> {
        // Ensure system prompt is first
        let effective_messages = if messages.is_empty()
            || messages.first().map(|m| m.role.as_str()) != Some("system")
        {
            let mut prefixed = vec![ChatMessageRef {
                role: "system".into(),
                content: SYSTEM_PROMPT.into(),
            }];
            prefixed.extend(messages.iter().cloned());
            prefixed
        } else {
            messages.to_vec()
        };

        // Build the request payload
        let payload = serde_json::json!({
            "model": self.current_model(),
            "messages": effective_messages,
            "stream": true,
        });

        let http_client = self.http_client.clone();
        let endpoint = self.endpoint_url().to_string();
        let api_key = self.api_key.clone();
        let on_event_clone = on_event;

        // Spawn the HTTP request in tokio runtime
        let result = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::try_current()
                .map_err(|_| AiError::ApiUnavailable("no tokio runtime available".into()))?;
            rt.block_on(async {
                let resp = http_client
                    .post(&endpoint)
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("Content-Type", "application/json")
                    .json(&payload)
                    .send()
                    .await
                    .context("HTTP request failed")
                    .map_err(AiError::from)?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(AiError::ApiError(format!(
                        "HTTP {status}: {text}"
                    )));
                }

                // Parse SSE stream line-by-line
                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();
                let mut done = false;

                while let Some(chunk) = stream.next().await {
                    let chunk = match chunk {
                        Ok(c) => c,
                        Err(e) => {
                            on_event_clone(StreamEvent::Error(format!("stream error: {e}")));
                            break;
                        }
                    };

                    for byte in chunk {
                        if byte == b'\n' {
                            if buffer.starts_with("data: ") {
                                let data = &buffer[6..];
                                if data.trim() == "[DONE]" {
                                    done = true;
                                    buffer.clear();
                                    continue;
                                }
                                // Try to parse as SSE JSON
                                if let Ok(sse_chunk) =
                                    serde_json::from_str::<SseChunk>(data)
                                {
                                    if let Some(choices) = sse_chunk.choices {
                                        for choice in choices {
                                            if let Some(delta) = choice.delta {
                                                if let Some(text) = delta.content {
                                                    on_event_clone(StreamEvent::Token(text));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            buffer.clear();
                        } else {
                            buffer.push(byte as char);
                        }
                    }
                    if done {
                        break;
                    }
                }

                // Handle any remaining partial line (no trailing newline)
                if !buffer.is_empty() && buffer.starts_with("data: ") {
                    let data = &buffer[6..];
                    if data.trim() == "[DONE]" {
                        done = true;
                    } else if let Ok(sse_chunk) = serde_json::from_str::<SseChunk>(data) {
                        if let Some(choices) = sse_chunk.choices {
                            for choice in choices {
                                if let Some(delta) = choice.delta {
                                    if let Some(text) = delta.content {
                                        on_event_clone(StreamEvent::Token(text));
                                    }
                                }
                            }
                        }
                    }
                }

                if !done {
                    on_event_clone(StreamEvent::Done);
                }
                Ok(())
            })
        })
        .await;

        match result {
            Ok(inner_result) => inner_result,
            Err(e) => Err(AiError::ApiError(format!("spawn failed: {e}"))),
        }
    }

    /// Non-streaming chat: sends a synchronous HTTP request via `ureq` and
    /// collects the complete response into a single string.
    ///
    /// Useful for simple Q&A without real-time rendering.
    pub fn chat_simple(&self, messages: &[ChatMessageRef]) -> Result<String, AiError> {
        // Ensure system prompt is first
        let effective_messages = if messages.is_empty()
            || messages.first().map(|m| m.role.as_str()) != Some("system")
        {
            let mut prefixed = vec![ChatMessageRef {
                role: "system".into(),
                content: SYSTEM_PROMPT.into(),
            }];
            prefixed.extend(messages.iter().cloned());
            prefixed
        } else {
            messages.to_vec()
        };

        let url = self.endpoint_url();
        let api_key = self.api_key.clone();
        let model = self.current_model();

        let payload = serde_json::json!({
            "model": model,
            "messages": effective_messages,
            "stream": false,
        });

        let resp = ureq::post(&url)
            .set("Authorization", &format!("Bearer {api_key}"))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!(&payload))
            .context("ureq HTTP request failed")
            .map_err(AiError::from)?;

        let status = resp.status();
        let text = resp.into_string().context("read response body")?;
        if status < 200 || status >= 300 {
            return Err(AiError::ApiError(format!("HTTP {status}: {text}")));
        }

        let body: serde_json::Value = serde_json::from_str(&text)
            .context("parse response JSON")
            .map_err(AiError::from)?;

        body.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(String::from)
            .ok_or_else(|| AiError::ApiError("response missing content".into()))
    }

    fn endpoint_url(&self) -> &str {
        &self.endpoint_url
    }

    fn current_model(&self) -> &str {
        &self.model
    }

    fn api_key(&self) -> &str {
        &self.api_key
    }
}
