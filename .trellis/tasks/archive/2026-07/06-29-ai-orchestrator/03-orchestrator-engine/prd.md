# 编排器引擎

## 目标

实现编排器的 LLM API 客户端，接收消息列表、通过 SSE 流式获取回复、通过回调分发事件。不包含 UI，只做 API 通信层。

## 设计

### SSE 流式协议支持

两种 API 格式：

**OpenAI 格式** (默认):
```
data: {"id":"...","object":"chat.completion.chunk","choices":[{"delta":{"content":"Hello"}}]}
```

**Anthropic 格式**:
```
event: content_block_delta
data: {"type":"content_block_delta","delta":{"text":"Hello"}}
```

### StreamEvent

```rust
// src/ai/engine.rs

/// 编排器流式回调事件
pub enum StreamEvent {
    /// 一个 token 文本块（流式输出）
    Token(String),
    /// 编排器决定分发 agent 任务
    AgentDispatch(AgentTask),
    /// Agent 实时输出
    AgentOutput { agent_name: String, text: String },
    /// Agent 执行完成
    AgentComplete(AgentResult),
    /// 错误
    Error(String),
    /// 流结束（所有处理完成）
    Done,
}

pub struct AgentTask {
    pub agent_name: String,
    pub instruction: String,
    pub cwd: String,
    pub pane_id: Option<String>,
}

pub struct AgentResult {
    pub agent_name: String,
    pub pane_id: String,
    pub exit_status: String,  // "completed" | "failed" | "timeout"
    pub output: String,
    pub summary: Option<String>,
}
```

### OrchestratorEngine

```rust
pub struct OrchestratorEngine {
    http_client: reqwest::Client,
}

impl OrchestratorEngine {
    /// 创建引擎实例（从配置读取 API provider 信息）
    pub fn new(provider: &ApiProviderConfig) -> Self;

    /// 单轮 chat 请求，SSE 流式回调
    /// messages: 历史消息（ChatMessage 列表）
    /// system_prompt: 编排器系统指令
    /// on_event: 实时事件回调（在 `cx.background_executor()` 中调用）
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        system_prompt: &str,
        on_event: impl FnMut(StreamEvent),
    ) -> Result<()>;

    /// 非流式单轮请求（用于小型决定，如自动命名会话）
    pub async fn chat_simple(
        &self,
        messages: &[ChatMessage],
        system_prompt: &str,
    ) -> Result<String>;
}
```

### SSE 解析器

```rust
/// 解析 SSE data: 行。处理两种格式：
/// - OpenAI: data: {"choices":[{"delta":{"content":"..."}}]}
/// - Anthropic: event: content_block_delta \n data: {...}
fn parse_sse_line(line: &str) -> Option<String> {
    // 1. 去掉 "data: " 前缀
    // 2. JSON 解析提取 content/text token
    // 3. [DONE] 标记返回 None
}
```

### Tokio Runtime

`reqwest` 需要 tokio runtime。在 `main.rs` 中初始化：

```rust
fn main() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let _guard = rt.enter();
    // ...
    App::new().run(app)
}
```

`OrchestratorEngine` 内部使用 `reqwest::Client`：

```rust
impl OrchestratorEngine {
    fn build_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest::Client")
    }

    pub async fn chat_stream(/* ... */) -> Result<()> {
        let endpoint = provider.endpoint_url();
        let body = serde_json::json!({
            "model": provider.model,
            "messages": messages,    // 转换为 API 格式
            "stream": true,
        });

        let response = self.http_client
            .post(&endpoint)
            .header("Authorization", format!("Bearer {}", provider.api_key))
            .json(&body)
            .send()
            .await
            .context("send request")?;

        let mut stream = response.bytes_stream();
        // 逐行读取，解析 SSE data: 行
        // 通过 on_event 回调发出 Token / Done / Error
    }
}
```

## Requirements

- [ ] reqwest Client 构建（tokio runtime 初始化在 main.rs）
- [ ] OpenAI 格式 SSE 解析
- [ ] Anthropic 格式 SSE 解析
- [ ] `chat_stream()` 实现（流式回调）
- [ ] `chat_simple()` 实现（非流式）
- [ ] 错误处理（网络错误/HTTP 错误/JSON 解析错误）

## Acceptance Criteria

- [ ] 连接真实 OpenAI API endpoint 可收到流式 tokens
- [ ] 连接真实 Anthropic API endpoint 可收到流式 tokens
- [ ] 网络错误返回 Error 事件（不 panic）
- [ ] HTTP 4xx/5xx 返回 Error 事件
- [ ] `chat_simple()` 返回完整字符串

## Out of Scope

- 重试逻辑（交给 10-error-handling）
- Agent dispatch 逻辑（交给 06-agent-dispatch）
- 消息格式转换的通用化（只支持 Chat Completions 格式）
