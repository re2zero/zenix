# AI 编排器模块

> 多 Agent 编排调度：LLM 编排 + herdr IPC + Agent CLI 调度

---

## 1. 模块结构

```
src/ai/
├── mod.rs          # Module root, re-exports
├── chat.rs         # Chat panel UI (消息列表、输入框、底部栏)
├── session.rs      # SQLite 持久化 (CRUD、FTS5 搜索)
├── engine.rs       # 编排器引擎 (LLM API、SSE 流式、调度循环)
├── herdr.rs        # Herdr IPC 客户端 (Unix socket、JSON-line)
└── config.rs       # ZenixAI 配置 (provider/model/thinking)
```

### Re-export 约定

```rust
// mod.rs
pub mod chat;
pub mod config;
pub mod engine;
pub mod error;
pub mod herdr;
pub mod session;

pub use session::*;   // Session, ChatMessage, AgentActivity, SearchHit, SessionManager
pub use engine::*;    // OrchestratorEngine, AgentTask, AgentResult, DispatchDirective, DispatchStreamEvent
```

> **Why**: 避免 `crate::ai::chat::AiChatPanelData` 等长路径，但 `#[allow(dead_code)]` 禁止——死代码必须移除或标记为 Phase N prep。

---

## 2. Herdr IPC 协议

### 2.1 连接方式

- **Unix socket**: `~/.config/herdr/herdr-client.sock`
- **协议**: JSON-line (每行一个完整 JSON 对象)
- **类型自包含**: 不导入 herdr 作为 Rust crate（herdr 是外部进程）

```rust
// 连接示例
let stream = UnixStream::connect(&socket_path)?;
stream.set_read_timeout(Some(Duration::from_secs(10)))?;
stream.set_write_timeout(Some(Duration::from_secs(10)))?;

// 发送请求
let request = HerdrRequest { id: "zenix-req-0", method: HerdrMethod::ServerPing };
writeln!(writer, "{}", serde_json::to_string(&request)?)?;

// 读取响应
let mut line = String::new();
reader.read_line(&mut line)?;
let response: HerdrResponse = serde_json::from_str(line.trim())?;
```

### 2.2 请求/响应格式

```json
// Request
{"id": "zenix-req-0", "method": "workspace.list", "params": {}}

// Success
{"id": "zenix-req-0", "result": {"type": "workspace_list", "workspaces": [...]}}

// Error
{"id": "zenix-req-0", "error": {"code": "pane_not_found", "message": "..."}}
```

### 2.3 支持的 RPC 方法 (17 种)

| 方法 | 参数类型 | 响应类型 | 用途 |
|------|----------|----------|------|
| `server.ping` | `PingParams` | `Pong` | 连接验证 |
| `workspace.list` | `EmptyParams` | `WorkspaceList` | 列出所有 workspace |
| `workspace.get` | `WorkspaceTarget` | `WorkspaceInfo` | 获取单个 workspace |
| `workspace.create` | `WorkspaceCreateParams` | `WorkspaceCreated` | 创建 workspace |
| `workspace.close` | `WorkspaceTarget` | `Ok` | 关闭 workspace |
| `tab.list` | `TabListParams` | `TabList` | 列出 tabs |
| `tab.get` | `TabTarget` | `TabInfo` | 获取单个 tab |
| `tab.create` | `TabCreateParams` | `TabCreated` | 创建 tab |
| `agent.list` | `EmptyParams` | `AgentList` | 列出 agents |
| `agent.start` | `AgentStartParams` | `AgentStarted` | 启动 agent CLI |
| `agent.send` | `AgentSendParams` | `Ok` | 发送文本到 agent |
| `agent.read` | `AgentReadParams` | `PaneRead` | 读取 agent 输出 |
| `pane.list` | `PaneListParams` | `PaneList` | 列出 panes |
| `pane.get` | `PaneTarget` | `PaneInfo` | 获取单个 pane |
| `pane.split` | `PaneSplitParams` | `PaneInfo` | 分割 pane |
| `pane.send_text` | `PaneSendTextParams` | `Ok` | 发送文本到 pane |
| `pane.read` | `PaneReadParams` | `PaneRead` | 读取 pane 输出 |
| `pane.wait_for_output` | `PaneWaitForOutputParams` | `OutputMatched` | 等待输出匹配 |
| `events.subscribe` | `EventsSubscribeParams` | `SubscriptionStarted` | 订阅事件 |

### 2.4 关键数据类型

```rust
pub struct WorkspaceInfo {
    pub workspace_id: String,
    pub label: String,
    pub focused: bool,
    pub pane_count: usize,
    pub active_tab_id: String,
    pub agent_status: AgentStatus,
}

pub struct AgentInfo {
    pub terminal_id: String,
    pub agent: Option<String>,
    pub agent_status: AgentStatus,
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    pub cwd: Option<String>,
}

pub struct PaneInfo {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub focused: bool,
    pub cwd: Option<String>,
    pub agent: Option<String>,
    pub agent_status: AgentStatus,
    pub revision: u64,
}

pub enum AgentStatus { Idle, Working, Blocked, Done, Unknown }
pub enum ReadSource { Visible, Recent, RecentUnwrapped, Detection }
pub enum OutputMatch { Substring { value: String }, Regex { value: String } }
```

### 2.5 Socket 重连策略

每次 IPC 调用打开新 `UnixStream`（不保持持久连接）：
- **优点**: 隐式重连，无需手动管理连接状态
- **缺点**: 每次调用有 connect 开销
- **适用**: 桌面应用短操作，可接受

---

## 3. 编排器引擎

### 3.1 LLM 通信

#### `chat_stream` (SSE 流式)

```rust
pub async fn chat_stream(
    &self,
    messages: &[ChatMessageRef],
    on_event: impl Fn(StreamEvent) + Send + 'static,
) -> Result<(), AiError>
```

**实现要点**:
- 使用 `reqwest` + `tokio::spawn` 发送 HTTP 请求
- **禁止** `spawn_blocking` + `try_current()` 模式（`spawn_blocking` 线程无 tokio context）
- SSE 解析：逐字节缓冲，`data: ` 前缀提取 JSON，`[DONE]` 终止

```rust
// ✅ 正确：直接在 async context 中 await
let resp = http_client.post(&endpoint).json(&payload).send().await?;
let mut stream = resp.bytes_stream();
while let Some(chunk) = stream.next().await {
    // parse SSE...
}

// ❌ 错误：spawn_blocking + try_current
tokio::task::spawn_blocking(move || {
    let rt = tokio::runtime::Handle::try_current()?; // 永远失败
    rt.block_on(async { /* ... */ })
})
```

#### `chat_simple` (非流式)

```rust
pub fn chat_simple(&self, messages: &[ChatMessageRef]) -> Result<String, AiError>
```

- 使用 `ureq` (同步 HTTP)
- 适用于简单 Q&A，无需实时渲染

### 3.2 调度循环 (`chat_and_dispatch`)

```rust
pub async fn chat_and_dispatch(
    &self,
    messages: &[ChatMessageRef],
    herdr_client: &mut HerdrIpcClient,
    workspace_id: &str,
    on_event: impl Fn(DispatchStreamEvent) + Send + Sync + 'static,
) -> Result<String, AiError>
```

**流程**:
1. **Phase 1**: LLM 调用 (SSE 流式) → 收集完整响应文本
2. **Phase 2**: 解析调度指令 (`parse_dispatch_directive`)
3. **Phase 3**: 执行 Agent (`execute_agent_task`)
4. **Phase 4**: 汇总结果 → 返回 LLM 生成最终回复
5. **Loop**: 如果 LLM 再次调度，重复 Phase 1-4（最多 5 轮）

```rust
loop {
    let response_text = self.call_llm_collect(&loop_messages, ...).await?;
    
    if let Some(directive) = parse_dispatch_directive(&response_text) {
        // Execute agents
        for task in directive.to_tasks() {
            let result = execute_agent_task(herdr_client, &task, workspace_id, TIMEOUT)?;
            on_event(DispatchStreamEvent::AgentComplete(result));
        }
        
        // Build summary for next LLM round
        let summary = agent_results.join("\n\n---\n\n");
        loop_messages = vec![
            ChatMessageRef { role: "system", content: SYSTEM_PROMPT },
            ChatMessageRef { role: "user", content: format!("Agent results:\n{summary}") },
        ];
    } else {
        // No dispatch — final response
        final_response = response_text;
        break;
    }
}
```

### 3.3 调度指令解析

#### JSON 格式 (优先)

```json
{
  "agents": [
    {
      "agent_name": "claude",
      "instruction": "Implement JWT token refresh in src/auth/jwt.rs",
      "cwd": "/home/user/project"
    }
  ]
}
```

#### 文本格式 (Fallback)

```
Agent: claude
Task: Implement JWT token refresh in src/auth/jwt.rs
CWD: /home/user/project
```

**解析策略**:
1. 提取 ` ```json ... ``` ` 或 ` ``` ... ``` ` 代码块
2. 解析 JSON `{"agents": [...]}`
3. 无代码块 → 尝试内联 JSON
4. 无 JSON → 尝试文本格式 (`Agent:\s*(\w+)`, `Task:\s*(.+)`)
5. 无匹配 → 返回 `None`（LLM 直接回复）

```rust
pub fn parse_dispatch_directive(response: &str) -> Option<DispatchDirective> {
    // 1. Try JSON code block
    if let Some(json_text) = extract_json_block(response) {
        if let Some(d) = parse_json_dispatch(&json_text) {
            return Some(d);
        }
    }
    // 2. Try inline JSON
    if let Some(d) = parse_json_dispatch(response) {
        return Some(d);
    }
    // 3. Fallback to text format
    parse_text_dispatch(response)
}
```

### 3.4 Agent 执行流程

```rust
pub fn execute_agent_task(
    herdr_client: &mut HerdrIpcClient,
    task: &AgentTask,
    workspace_id: &str,
    timeout_ms: u64,
) -> AgentResult
```

**步骤**:
1. `start_agent(name, Some(workspace_id), Some(&cwd), vec![])` → 获取 `AgentInfo`
2. `thread::sleep(500ms)` → 等待 agent 初始化
3. `send_agent_text(terminal_id, &instruction)` → 发送任务指令
4. `wait_for_output(pane_id, "[AGENT_DONE]", timeout_ms)` → 等待完成标记
5. `read_agent(terminal_id, ReadSource::Recent, 500)` → 读取输出（fallback）

**错误处理**:
- Agent 启动失败 → `AgentResult { exit_status: "failed", ... }`
- 发送指令失败 → 记录 warn，继续等待
- 超时 → fallback 读取 recent output，`exit_status: "timeout"`

---

## 4. 会话持久化

### 4.1 SQLite Schema

```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT 'New Chat',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    workspace TEXT,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    thinking_level TEXT NOT NULL DEFAULT 'medium'
        CHECK (thinking_level IN ('low', 'medium', 'high', 'xhigh'))
);

CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'orchestrator')),
    content TEXT NOT NULL,
    agent_activities TEXT,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    sequence INTEGER NOT NULL
);

CREATE INDEX idx_messages_session ON messages(session_id, sequence);

CREATE VIRTUAL TABLE messages_fts USING fts5(
    content,
    content=messages,
    content_rowid=id,
    tokenize='unicode61'
);

-- 仅索引 user 角色消息 (PRD: 搜索范围 = user content only)
CREATE TRIGGER messages_ai AFTER INSERT ON messages WHEN new.role = 'user' BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;
```

### 4.2 FTS5 搜索

- **搜索范围**: 仅 `role = 'user'` 的消息
- **查询清洗**: 每个 token 包裹引号，过滤特殊字符
- **结果**: `SearchHit { message, session_title, snippet }`

```rust
pub fn search_messages(&self, query: &str, session_id: Option<&str>) -> Result<Vec<SearchHit>> {
    let safe_query = sanitize_fts_query(query);
    // ... FTS5 MATCH query ...
}
```

---

## 5. 错误处理

### 5.1 AiError 枚举

```rust
pub enum AiError {
    ConfigMissing(String),      // provider/model 缺失
    ApiUnavailable(String),     // API 不可达
    ApiError(String),           // LLM API 错误 (HTTP 401/403/429)
    AgentCliNotFound(String),   // Agent CLI 未安装
    IpcError(String),           // herdr IPC 通信失败
    SerializationError(String), // JSON 序列化/反序列化错误
    Timeout(String),            // 超时
}
```

### 5.2 用户友好消息

```rust
pub fn user_friendly(err: &AiError) -> String {
    match err {
        AiError::ConfigMissing(msg) => "请在「设置 → 扩展 → API 提供商」中配置...",
        AiError::ApiError(msg) if msg.contains("401") => "API Key 无效或已过期...",
        AiError::ApiError(msg) if msg.contains("429") => "请求过于频繁...",
        AiError::AgentCliNotFound(agent) => format!("Agent CLI「{agent}」未安装..."),
        _ => format!("AI 服务返回错误：{}", truncate_err(msg, 80)),
    }
}
```

### 5.3 可重试判断

```rust
pub fn retryable(err: &AiError) -> bool {
    matches!(err, AiError::ApiUnavailable(_) | AiError::ApiError(_) | AiError::Timeout(_) | AiError::IpcError(_))
}
```

---

## 6. 设计决策

### 6.1 tokio Runtime 初始化

**决策**: 在 `main.rs` 中初始化 tokio runtime

```rust
fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let _guard = rt.enter();
    // ... GPUI app initialization ...
}
```

**原因**:
- `reqwest` 需要 tokio runtime 才能运行
- `chat_stream` 使用 `tokio::spawn` 发送 HTTP 请求
- `spawn_blocking` 线程无 tokio context，`try_current()` 永远失败

**Cargo.toml 依赖**:
```toml
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros"] }
reqwest = { version = "0.12", features = ["json", "stream"] }
```

### 6.2 Herdr 类型自包含

**决策**: 不导入 herdr 作为 Rust crate，在 `ai/herdr.rs` 中定义自己的类型

**原因**:
- herdr 是外部进程，通过 Unix socket 通信
- 避免编译时依赖 herdr 源码
- 类型与 herdr wire protocol 对齐即可

### 6.3 Socket 每请求重连

**决策**: 每次 IPC 调用打开新 `UnixStream`

**原因**:
- 隐式重连，无需手动管理连接状态
- 桌面应用短操作，connect 开销可接受
- 简化实现，减少 bug

### 6.4 调度循环最多 5 轮

**决策**: `max_dispatch_rounds = 5`

**原因**:
- 防止 LLM 无限调度（bug 或恶意输入）
- 5 轮足以覆盖复杂任务分解
- 达到上限时返回汇总 + 通知

---

## 7. 反模式

### 7.1 禁止 `spawn_blocking` + `try_current`

```rust
// ❌ 错误：永远失败
tokio::task::spawn_blocking(move || {
    let rt = tokio::runtime::Handle::try_current()?; // 在 spawn_blocking 线程上永远失败
    rt.block_on(async { /* ... */ })
})

// ✅ 正确：直接在 async context 中 await
pub async fn chat_stream(&self, ...) -> Result<()> {
    let resp = http_client.post(&endpoint).send().await?;
    // ...
}
```

### 7.2 禁止 `#[allow(dead_code)]`

```rust
// ❌ 禁止
#[allow(dead_code)]
pub fn get_orchestrator_provider(...) -> Option<ApiProviderConfig> { ... }

// ✅ 正确：移除死代码，或标记为 Phase N prep 注释
// Phase 2: 用于 AI 设置面板的 provider 选择
```

### 7.3 禁止 `unwrap()` in 生产代码

```rust
// ❌ 禁止
let config = ConfigStore::load().unwrap();

// ✅ 正确：使用 anyhow::Context
let config = ConfigStore::load().context("load config")?;
```

---

## 8. 测试要求

### 8.1 单元测试覆盖

| 模块 | 必需测试 |
|------|----------|
| `herdr.rs` | JSON 序列化/反序列化 round-trip（所有关键类型）、请求 ID 生成、响应解析（success/error/empty/invalid） |
| `engine.rs` | `parse_dispatch_directive`（JSON code block、inline JSON、text format、malformed input）、`AgentResult` 构造、`execute_agent_task` 错误路径 |
| `session.rs` | CRUD、FTS5 搜索（含特殊字符）、ON DELETE CASCADE、搜索范围过滤（仅 user 消息） |

### 8.2 测试断言点

```rust
#[test]
fn test_parse_dispatch_directive_json() {
    let json = r#"{"agents":[{"agent_name":"claude","instruction":"test","cwd":"/tmp"}]}"#;
    let directive = parse_dispatch_directive(json).unwrap();
    assert_eq!(directive.to_tasks().len(), 1);
    assert_eq!(directive.to_tasks()[0].agent_name, "claude");
}

#[test]
fn test_parse_dispatch_directive_text() {
    let text = "Agent: claude\nTask: implement login\nCWD: /project";
    let directive = parse_dispatch_directive(text).unwrap();
    assert!(matches!(directive, DispatchDirective::Text { agent_name, .. } if agent_name == "claude"));
}

#[test]
fn test_parse_dispatch_directive_none() {
    let text = "I'll help you with that. Here's the solution...";
    assert!(parse_dispatch_directive(text).is_none());
}
```

---

## 9. 跨模块一致性检查清单

在实现 AI 模块新功能时，检查：

- [ ] `HerdrIpcClient` 方法签名与 `engine.rs` 调用匹配
- [ ] `AgentTask`/`AgentResult` 与 `session.rs` 的 `AgentActivity` 职责分离（运行时 vs 持久化）
- [ ] `DispatchStreamEvent` 变体与 `chat.rs` 的 `AiChatPanelData` 字段兼容
- [ ] `AiError` 变体覆盖所有错误路径（herdr、LLM、配置、序列化）
- [ ] 所有 `serde` 注解正确（`#[serde(tag = "method", content = "params")]` 等）
- [ ] JSON 字段名与 herdr wire protocol 对齐
- [ ] 无 `unwrap()` 在生产代码中
- [ ] 无 `#[allow(dead_code)]`

---

## 10. 已知限制与未来改进

### 10.1 当前限制

- **单 Agent 串行执行**: `chat_and_dispatch` 按顺序执行 Agent，不支持并行
- **无 Agent 实时监控**: 通过 `wait_for_output` 等待完成，无实时状态推送
- **无配置同步**: API Provider 配置未自动同步到 Agent CLI（Phase 3 实现）

### 10.2 未来改进方向

- **并行 Agent 执行**: 多 Agent 同时运行，提高复杂度任务效率
- **Event 订阅**: 使用 `events.subscribe` 实现 Agent 状态实时推送
- **配置同步**: 通过 `config_adapters/` 基础设施自动同步 API 配置到 Agent CLI
- **Agent 输出 UI**: Agent 内联输出显示为可折叠区块（Phase 6 实现）
