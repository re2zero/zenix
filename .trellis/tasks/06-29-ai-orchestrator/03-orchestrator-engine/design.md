# 编排器引擎 — 设计

## Tokio Runtime 初始化

```rust
// src/main.rs
fn main() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let _guard = rt.enter();  // 让 reqwest 的 async 操作能找到 runtime

    let app = App::new().with_assets(Assets).run(|| ...);
    // ...
}
```

## SSE 解析

```rust
// src/ai/engine.rs

/// 从 SSE data 行中提取 content token。
/// 支持两种格式：
///
/// OpenAI:
///   data: {"id":"...","choices":[{"delta":{"content":"Hello","reasoning_content":null},"index":0}]}
///
/// Anthropic (content_block_delta):
///   event: content_block_delta
///   data: {"type":"content_block_delta","delta":{"text":"Hello"}}
///
/// Anthropic (message_stop):
///   event: message_stop
///   data: {"type":"message_stop"}
fn extract_sse_content(line: &str) -> Option<String> {
    let line = line.strip_prefix("data: ")?.trim();
    if line == "[DONE]" { return None; }

    // Try OpenAI format
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
        // OpenAI: choices[0].delta.content
        if let Some(choices) = val.get("choices").and_then(|c| c.as_array()) {
            if let Some(delta) = choices.first().and_then(|c| c.get("delta")) {
                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                    if !content.is_empty() { return Some(content.to_string()); }
                }
            }
            return None;  // finished or non-content chunk
        }
        // Anthropic: type == "content_block_delta", delta.text
        if val.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
            if let Some(text) = val.get("delta").and_then(|d| d.get("text")).and_then(|t| t.as_str()) {
                return Some(text.to_string());
            }
        }
    }
    None
}
```

## OrchestratorEngine

```rust
use reqwest::Client;
use futures::StreamExt;
use anyhow::{Context, Result};
use crate::api_provider::ApiProviderConfig;
use super::session::ChatMessage;

/// 占位结构体 — 任务 06（agent-dispatch）会替换为带完整方法的实现。
/// 此处先定义为简单数据容器，让 StreamEvent 能编译。
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct AgentTask {
    pub agent_name: String,
    pub instruction: String,
    pub cwd: String,
    pub pane_id: Option<String>,
}

/// 占位结构体 — 任务 06 会替换。
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct AgentResult {
    pub agent_name: String,
    pub pane_id: String,
    pub exit_status: String,
    pub output: String,
    pub summary: Option<String>,
}

/// SSE 流事件。Agent* 变体由任务 06 的 dispatch 逻辑发出，
/// 此阶段仅定义签名（`#[allow(dead_code)]`），chat_stream() 不会 emit 它们。
pub enum StreamEvent {
    Token(String),
    #[allow(dead_code)]
    AgentDispatch(AgentTask),
    #[allow(dead_code)]
    AgentOutput { agent_name: String, text: String },
    #[allow(dead_code)]
    AgentComplete(AgentResult),
    Error(String),
    Done,
}

pub struct OrchestratorEngine {
    http_client: Client,
}

impl OrchestratorEngine {
    pub fn new(provider: &ApiProviderConfig) -> Self {
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest::Client");
        Self { http_client }
    }

    /// 流式调用 Chat Completions API
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        system_prompt: &str,
        mut on_event: impl FnMut(StreamEvent),
    ) -> Result<()> {
        // 1. 构建请求体
        let body = self.build_request_body(messages, system_prompt);

        // 2. 发送请求
        let response = match self.send_request(&body).await {
            Ok(r) => r,
            Err(e) => {
                on_event(StreamEvent::Error(format!("API request failed: {e}")));
                return Err(e);
            }
        };

        // 3. 逐行读取 SSE
        let mut stream = response.bytes_stream();
        let mut buf = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    buf.extend_from_slice(&chunk);
                    // 按行处理
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line = buf[..pos].to_vec();
                        buf = buf[pos + 1..].to_vec();
                        let line_str = String::from_utf8_lossy(&line);

                        if let Some(token) = extract_sse_content(&line_str) {
                            on_event(StreamEvent::Token(token));
                        }
                    }
                }
                Err(e) => {
                    on_event(StreamEvent::Error(format!("Stream error: {e}")));
                    return Err(e.into());
                }
            }
        }

        on_event(StreamEvent::Done);
        Ok(())
    }

    /// 非流式调用
    pub async fn chat_simple(
        &self,
        message: &str,
        system_prompt: &str,
    ) -> Result<String> {
        let msg = ChatMessage {
            role: "user".into(),
            content: message.into(),
            ..Default::default()
        };
        // 构建非流式请求...
        let body = serde_json::json!({
            "model": "...",
            "messages": [...],
            "stream": false,
        });
        // ...
    }

    // ── 内部方法 ──

    fn build_request_body(
        &self,
        messages: &[ChatMessage],
        system_prompt: &str,
    ) -> serde_json::Value {
        let api_messages: Vec<serde_json::Value> = std::iter::once(
            serde_json::json!({"role": "system", "content": system_prompt})
        ).chain(messages.iter().map(|m| serde_json::json!({
            "role": m.role,
            "content": m.content,
        }))).collect();

        serde_json::json!({
            "messages": api_messages,
            "stream": true,
        })
    }

    async fn send_request(
        &self,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response> {
        // 从 provider config 获取 endpoint 和 api_key
        // POST endpoint with Authorization header
        todo!()
    }
}
```

## Requirements

- [ ] `main.rs` tokio runtime 初始化
- [ ] `OrchestratorEngine::new()`
- [ ] SSE 解析器（OpenAI + Anthropic）
- [ ] `chat_stream()` 完整实现
- [ ] `chat_simple()` 完整实现
- [ ] 错误处理（HTTP 错误、JSON 解析错误、网络错误）
