# 异常处理完善 — 设计

## 错误类型

```rust
// src/ai/error.rs

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AiError {
    #[error("LLM API error: {0}")]
    ApiError(String),

    #[error("API rate limited: retry after {0}s")]
    RateLimited(u64),

    #[error("Herdr IPC error: {0}")]
    HerdrError(String),

    #[error("Agent '{agent}' failed: {detail}")]
    AgentFailed { agent: String, detail: String },

    #[error("Agent '{agent}' timed out after {timeout}s")]
    AgentTimeout { agent: String, timeout: u64 },

    #[error("Database error: {0}")]
    Database(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("{0}")]
    Other(String),
}
```

## 重试函数

```rust
// src/ai/error.rs

/// 带指数退避的异步重试
pub async fn retry_with_backoff<F, Fut, T>(
    operation: F,
    max_retries: u32,
    initial_delay: Duration,
) -> Result<T, AiError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, AiError>>,
{
    let mut delay = initial_delay;
    for attempt in 0..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if attempt == max_retries {
                    return Err(e);
                }
                tracing::warn!("Attempt {}/{} failed: {e}", attempt + 1, max_retries);
                smol::Timer::after(delay).await;
                delay *= 2;
            }
        }
    }
    unreachable!()
}
```

## 用户友好错误消息

```rust
/// 将技术错误转换为用户可理解的错误消息
pub fn user_facing_error(error: &AiError) -> String {
    match error {
        AiError::ApiError(msg) => {
            if msg.contains("401") || msg.contains("403") {
                "API 认证失败，请检查你的 API Key 是否正确配置。".into()
            } else if msg.contains("429") || msg.contains("rate") {
                "API 请求过于频繁，请稍后再试。".into()
            } else if msg.contains("5") {
                "LLM 服务暂时不可用，正在自动重试...".into()
            } else {
                format!("LLM 调用出错：{msg}")
            }
        }
        AiError::HerdrError(msg) => {
            if msg.contains("Connection refused") || msg.contains("No such file") {
                "无法连接到 herdr 进程。请确保 herdr 正在运行（在终端中执行 'herdr' 启动）。".into()
            } else {
                format!("herdr 通信错误：{msg}")
            }
        }
        AiError::AgentFailed { agent, detail } => {
            format!("Agent '{agent}' 执行失败：{detail}")
        }
        AiError::AgentTimeout { agent, timeout } => {
            format!("Agent '{agent}' 执行超时（{timeout} 秒无响应）")
        }
        AiError::Database(msg) => {
            format!("数据库错误：{msg}")
        }
        AiError::ConfigError(msg) => {
            format!("配置错误：{msg}")
        }
        AiError::Other(msg) => msg.clone(),
    }
}
```

## Agent 输出截断

```rust
const MAX_AGENT_OUTPUT_BYTES: usize = 100 * 1024; // 100KB

/// 截断 agent 输出到最大限制
pub fn truncate_agent_output(output: &str) -> String {
    if output.len() > MAX_AGENT_OUTPUT_BYTES {
        let truncated: String = output
            .char_indices()
            .take_while(|(i, _)| *i < MAX_AGENT_OUTPUT_BYTES)
            .map(|(_, c)| c)
            .collect();
        format!("{truncated}\n\n[输出过长，已截断至 {max_kb}KB]",
            max_kb = MAX_AGENT_OUTPUT_BYTES / 1024)
    } else {
        output.to_string()
    }
}
```

## 集成点

engine.rs 中的集成：
- `chat_stream()` 中 API 错误 → `user_facing_error()` → `StreamEvent::Error`
- HTTP 5xx → 自动 `retry_with_backoff()`
- SSE 流中断 → 断点处重试

herdr.rs 中的集成：
- socket 连接失败 → `AiError::HerdrError` → 用户提示
- IPC 请求超时 → `AiError::HerdrError` → 重试

chat.rs 中的集成：
- `StreamEvent::Error` → 在消息列表中显示红色错误框
- 错误框包含重试按钮（点击重新发送）
