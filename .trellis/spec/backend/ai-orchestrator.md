# AI 编排器模块

> 多 Agent 编排调度：LLM 编排 + herdr IPC + Agent CLI 调度

---

## 1. 模块结构

```
src/ai/
├── mod.rs          # Module root, re-exports
├── chat/           # Chat panel UI (10-module directory)
│   ├── mod.rs      # Panel layout + render_ai_chat_panel
│   ├── state.rs    # AiChatPanelData, WorkspaceOption, AiChatAction
│   ├── header.rs   # Header + workspace dropdown + empty state
│   ├── input.rs    # Input bar + @ mention detection + routing
│   ├── messages.rs # Single-message rendering + streaming indicator
│   ├── agent_block.rs  # AgentBlock, AgentMonitorEntry types
│   ├── tool_card.rs    # Tool call result cards
│   ├── error_card.rs   # Error message card
│   ├── drawer.rs   # History + Agents drawer overlays
│   ├── resize.rs   # Panel + input resize handles
│   └── mention_popup.rs # @ worker mention popup overlay
├── roles.rs        # RoleRegistry, RoleDef, built-in templates
├── workers.rs      # WorkerRegistry, WorkerInstance, WorkerStatus
├── session.rs      # SQLite 持久化 (CRUD、FTS5 搜索)
├── engine.rs       # 编排器引擎 (LLM API、SSE 流式、调度循环)
├── tools.rs        # 6 个编排工具 + Tool trait + ToolExecutor
├── file_ops.rs     # 内部文件操作函数 (read_file_contents, list_directory, search_files)
├── compactor.rs    # 会话压缩 (token 估算 + 自动压缩 + 配置隔离)
├── markdown.rs     # 轻量级 Markdown 渲染 (无外部依赖)
├── herdr.rs        # Herdr IPC 客户端 (Unix socket、JSON-line)
└── error.rs        # AiError 枚举 + user_friendly + retryable
```

### Re-export 约定

```rust
// mod.rs
pub mod chat;
pub mod engine;
pub mod error;
pub mod herdr;
pub mod session;

pub use session::*;   // Session, ChatMessage, AgentActivity, SearchHit, SessionManager
pub use engine::*;    // OrchestratorEngine, AgentTask, AgentResult, DispatchDirective, DispatchStreamEvent, execute_agent_task, chat_with_tools, chat_simple_with_tools
```

> **Note**: `config.rs` was removed in the fourth iteration — ZenixAI config is now integrated into API providers. The AI chat panel reads provider/model/thinking from `api_provider::load_all()` directly.

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

### 2.3 支持的 RPC 方法 (19 种)

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
| `pane.close` | `PaneTarget` | `Ok` | 关闭 pane |
| `tab.close` | `TabTarget` | `Ok` | 关闭 tab |
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

### 3.2 工具调用循环 (`chat_with_tools`)

```rust
pub async fn chat_with_tools(
    &self,
    messages: &[ChatMessageRef],
    tools: &[&dyn Tool],
    executor: &ToolExecutor,
    compactor: &Compactor,
    max_tool_rounds: u32,
    on_event: impl Fn(DispatchStreamEvent) + Send + Sync + 'static,
) -> Result<String, AiError>
```

**流程**:
1. **系统消息注入**: 如果首条消息不是 `role: "system"`，自动在开头插入 `self.system_prompt`
2. **Compaction**: 每次循环开始时检查消息列表 token 用量，触发压缩
3. **Payload 构建**: 用 `build_tool_call_payload()` 构造包含 6 个工具定义的 HTTP 请求体
4. **SSE 流式调用**: 发送 HTTP POST → 逐字节解析 SSE → 同时发射 token / tool_call 事件
5. **工具执行**: `executor.execute()` 调用 llm 选择的工具
6. **结果注入**: tool result 作为 `role: "tool"` 消息追加到消息列表
7. **Loop**: 继续直到 LLM 返回纯文本（无 tool_calls），或达到 max_rounds

```rust
loop {
    // ── Compaction check ──────────
    if compactor.should_compress(&refs) {
        loop_messages = compactor.compress_messages(&refs).kept_messages;
        on_event(DispatchStreamEvent::Compacted(info));
    }

    // ── Build payload & stream ────
    let payload = build_tool_call_payload(&loop_messages, tools, &model);
    let tool_calls = parse_tool_call_sse_stream(resp, |event| { ... }).await;

    // ── Execute tool calls ────────
    if !tool_calls.is_empty() {
        let results = tool_calls.iter().map(|tc| executor.execute(tc)).collect();
        for result in &results {
            loop_messages.push(ChatMessageRef {
                role: "tool".into(),
                content: format!("Tool '{}' returned: ...", result.tool_name),
            });
        }
        continue;  // Next LLM round
    }

    // ── LLM direct reply ──────────
    return Ok(collected_text);
}
```

**废弃**: `chat_and_dispatch()` 和所有 dispatch 解析函数 (`parse_dispatch_directive`, `extract_json_block`, `parse_json_dispatch`, `parse_text_dispatch`, `find_pattern`, `classify_agent_error`) 已于 2026-07-09 删除。系统不再使用 JSON dispatch 格式，完全切换至 LLM function calling。

`execute_agent_task()` 保留为私有辅助函数（仅供参考，不在生产路径中调用）。

### 3.3 动态 System Prompt

`OrchestratorEngine` 使用 `system_prompt: String` 字段（非 `const`），通过 `build_system_prompt()` 构建。

```rust
pub fn build_system_prompt(roles_summary: &str, workspace_info: &str) -> String;
```

**构建时机**: `start_chat_streaming()` 中 LLM 调用前构建一次，通过 `engine.with_system_prompt()` 注入。

**注入内容**:
| 部分 | 来源 | 说明 |
|------|------|------|
| 编排器角色定义 | 内嵌 | `DEFAULT_SYSTEM_PROMPT` 常量 |
| 可用角色列表 | `RoleRegistry::list_roles()` | 每行 `- 角色名 (id) — 描述` |
| 可用工具列表 | 内嵌 | 6 个工具的 name + description 硬编码 |
| 工作空间信息 | `AiChatPanelData.selected_workspace` | `当前会话绑定到 workspace: xxx` |
| 工作流 + 约束 | 内嵌 | 6 步流程 + 约束条件 |

**降级行为**: 若 roles_summary 和 workspace_info 均为空，返回 `DEFAULT_SYSTEM_PROMPT`。

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

### 4.2 Memories 表 (编排器记忆)

```sql
CREATE TABLE IF NOT EXISTS memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL,
    session_id TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    key, value,
    content=memories,
    content_rowid=id
);
```

**FTS5 同步**: 通过 3 个 INSERT/DELETE/UPDATE 触发器 (`memories_ai`, `memories_ad`, `memories_au`) 自动同步。

**SessionManager 方法**:

```rust
pub fn write_memory(&self, key: &str, value: &str, session_id: &str) -> Result<()>;
pub fn read_memory(&self, key: &str) -> Result<Option<String>>;
pub fn search_memory(&self, query: &str, limit: usize) -> Result<Vec<(String, String)>>;
pub fn delete_memory(&self, key: &str) -> Result<()>;
```

### 4.3 FTS5 搜索

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

### 6.4 调度循环最多 5 轮（已迁移至工具调用循环）

**决策**: `max_tool_rounds` (默认 10) 由 `chat_with_tools()` 调用方传入。

**原因**:
- 防止 LLM 无限工具调用（bug 或恶意输入）
- 默认 10 轮（app.rs 传参），`chat_with_tools` 内部 clamp 到 MAX_TOOL_ROUNDS=25
- 达到上限时返回 `AiError::Timeout`

### 6.5 JSON Dispatch → Function Calling 迁移 (2026-07-09)

**决策**: 完全删除 JSON dispatch 格式，切换到 LLM function calling。

**旧架构**:
```
LLM 响应文本 → parse_dispatch_directive() → hardcoded JSON parse → execute_agent_task()
```

**新架构**:
```
LLM function calling → Tool trait → executor.execute() → 6 tools (spawn_worker, send_text, ...)
```

**原因**:
1. **行业标准**: OpenAI/Anthropic 原生支持 function calling，不需要手写解析器
2. **可靠性**: JSON dispatch 格式依赖 LLM 精确输出特定 JSON 结构，容易解析失败
3. **可扩展性**: 新增工具只需实现 Tool trait，无需修改解析逻辑
4. **系统提示简洁**: 无需在 system prompt 中嵌入格式说明

**副作用**:
- 所有 dispatch 解析函数 (`parse_dispatch_directive`, `extract_json_block`, `parse_json_dispatch`, `parse_text_dispatch`, `find_pattern`, `classify_agent_error`) 已删除
- `chat_and_dispatch()` 已删除，由 `chat_with_tools()` 替代
- `ToolExecutor` 从 engine.rs 移至 tools.rs（解决循环依赖）
- SSE 解析提取为独立 `parse_tool_call_sse_stream()` 函数

### 6.6 ToolExecutor 位置 (2026-07-09)

**决策**: `ToolExecutor` 定义在 `tools.rs` 而非 `engine.rs`。

**原因**:
- 避免循环依赖（engine.rs → tools.rs 导入 Tool，tools.rs → engine.rs 导入 ToolExecutor）
- 工具系统的所有权在 tools.rs，executor 作为工具执行上下文放在同一模块
- engine.rs 通过 `use crate::ai::tools::ToolExecutor` 导入

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

### 7.2 禁止 `#![allow(dead_code)]` (模块级)

模块级 `#![allow(dead_code)]` 已从 `tools.rs`, `engine.rs`, `compactor.rs` 移除。单个字段/函数上的 `#[allow(dead_code)]` 仅在保留的未来用途代码中使用。

```rust
// ❌ 禁止 - 模块级
#![allow(dead_code)]  // 已在 ai/ 模块中全部移除

// ✅ 可接受 - 单个函数保留
#[allow(dead_code)]  // 保留给 Phase 2 MCP Server
pub fn execute_agent_task(...) { ... }
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
| `herdr.rs` | JSON 序列化/反序列化 round-trip（所有关键类型）、请求 ID 生成、响应解析（success/error/empty/invalid）、17 种 RPC 方法 wire protocol 校验 |
| `engine.rs` | `build_tool_call_payload` JSON 结构、SSE 工具调用解析、`parse_tool_call_sse_stream` 双格式（OpenAI/Anthropic）、`build_system_prompt` 输出结构、`ToolExecutor` 分派 |
| `roles.rs` | `RoleDef` serde round-trip、内置模板数量/唯一性、frontmatter 格式、`RoleRegistry` 路径、seed + load、create/delete |
| `workers.rs` | 注册/状态更新/取消/cleanup、`send_to_worker` 路径、输出追加 |
| `session.rs` | CRUD、FTS5 搜索（含特殊字符）、ON DELETE CASCADE、搜索范围过滤（仅 user 消息） |
| `chat/` | `AiChatPanelData` 构造、`AgentBlock` 折叠状态管理、`AgentMonitorEntry::from_herdr` 转换、底部栏按钮渲染、config bar pill-style selectors、history panel, agent monitor, search bar、workspace 选择绑定、@ mention popup |

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
- [ ] `ToolExecutor.herdr_client` 方法签名与 `tools.rs` 工具调用匹配
- [ ] `AgentTask`/`AgentResult` 与 `session.rs` 的 `AgentActivity` 职责分离（运行时 vs 持久化）
- [ ] `DispatchStreamEvent` 变体（Token/Thinking/ToolCall/ToolResult/Compacted）与 `chat.rs` 的 `AiChatPanelData` 字段兼容
- [ ] `AiError` 变体覆盖所有错误路径（herdr、LLM、配置、序列化）
- [ ] 所有 `serde` 注解正确（`#[serde(tag = "method", content = "params")]` 等）
- [ ] JSON 字段名与 herdr wire protocol 对齐
- [ ] 无 `unwrap()` 在生产代码中
- [ ] 无 `#[allow(dead_code)]`

---

## 10. 已知限制与未来改进

### 10.1 当前限制

- **herdr 并行依赖**: 并行 dispatch 使用 `std::thread::spawn` + 独立 `HerdrIpcClient` 实例（因 client 非 `Send`），若 herdr 不支持并发连接可能产生竞态
- **AI chat config bar 的 dropdown 交互尚未实现** (目前为静态显示，用户选择通过 app state 管理)

### 10.2 已实现功能

- **Agent 输出 UI**: `AgentBlock` 可折叠内联区块 + `AgentMonitorEntry` 监控浮层 (第三次迭代)
- **Agent 监控**: 底部栏 `👤 运行中 N` + 颜色编码状态图标 (● working, ◉ blocked, ○ idle, ✓ done) (第三次迭代)
- **配置同步**: `api_provider::sync_to_all_agents()` / `unsync_from_unused_agents()` (第三次迭代)
- **配置栏**: 模型选择 ▏思考度 ▏workspace 选择 (第三次迭代)
- **历史面板**: 会话列表，点击切换，删除 (第三次迭代)
- **FTS5 搜索**: 搜索输入 + 结果列表 (第三次迭代)
- **i18n**: 17 unique keys (34 en/zh-CN 翻译条目)
- **侧边栏布局**: AI 图标紧邻设置按钮 (第四次迭代)
- **AI 聊天 UI 重写**: 完整重构渲染函数，改进样式/间距/交互 (第四次迭代)
- **ZenixAI 配置集成**: 移除独立设置 Tab，集成到 API 提供商配置 (第四次迭代)

### 10.3 未来改进方向

- **Event 订阅**: 使用 `events.subscribe` 实现 Agent 状态实时推送
- **多会话并发**: 切换会话时保留后台 agent 运行状态

### 10.4 TODO（待讨论/实现）

1. ~~**AI 会话界面重构**~~ → ✅ 第四次迭代完成
2. ~~**AI 会话界面增强**~~ → ✅ 2026-07-01 完成：Config Bar 交互化、TextArea 输入、Markdown 渲染、Agent Block 实时流式更新
3. ~~**AI 会话 API 配置隔离**~~ → ✅ 2026-07-02 完成：`OrchestratorEngine::has_available_provider()` + 空配置渲染
4. ~~**Rust Agent CLI 调查**~~ → ✅ 2026-07-01 完成：VTCode(shai/pi_agent_rust) 评估完毕，确认不直接复用，但工具系统设计和会话压缩逻辑值得参考
5. ~~**内置工具系统**~~ → ✅ 2026-07-02 完成：12 个工具（3 文件操作 + 4 Agent 管理 + 2 布局控制 + 3 记忆管理），Function Calling 循环
6. ~~**会话压缩**~~ → ✅ 2026-07-02 完成：`Compactor` 模块，token 估算 + 自动压缩 + 配置隔离

### 10.5 AI 界面增强 — 设计方案 (TODO #5, 2026-07-01)

用户确认方案: 1.A + 2.B + 3.A + 4.A

#### 5.1 Config Bar 交互化
- 模型 pill → 下拉选择器（点击展开 provider 列表 → 模型列表）
- workspace pill → 下拉选择器（点击展开 workspace 列表）
- 思考度 pill → 保持不变（点击切换 low/medium/high/xhigh）
- 新增 `ConfigDropdownTarget` enum: `Model` | `Workspace`
- 新增 `AiChatPanelData.config_dropdown: Option<ConfigDropdownTarget>`

#### 5.2 TextArea 输入
- 用 `gpui_component::input::InputState` multiline 替代裸 div
- Enter 发送, Shift+Enter 换行
- 流式输出中显示停止按钮
- `app.rs` 创建 InputState, 订阅事件转发到 `AiChatPanelData`

#### 5.3 Markdown 渲染器
- 新建 `ai/markdown.rs` (轻量级, 无外部依赖)
- 支持: **bold**, `inline code`, ```code blocks```, - list items
- 输出 GPUI `Vec<AnyElement>` segments

#### 5.4 Agent Block 实时流式
- `app.rs` 每 2s 轮询 herdr `list_agents()`
- 比较输出变化, 追加新内容到对应 AgentBlock
- 轮询 interval 可配置

---

## 10.6 内置工具系统（TODO #5, 2026-07-01 设计）

### 设计原则

- **LLM 视角命名**：工具名描述"能做什么"，不暴露"怎么做"（不用 `herdr_x` 前缀）
- **LLM 通过 function calling 调用工具**：编排器不硬编码调度逻辑，LLM 动态决定调用哪些工具
- **简约**：12 个工具覆盖核心场景，不过度设计

### 架构变更

当前 `engine.rs` 的 `chat_and_dispatch` 是**硬编码调度**：
```
LLM 响应 → parse_dispatch_directive() → execute_agent_task()
```

改为**工具调用循环**：
```
loop {
    response = call_llm_with_tools(messages, tools)
    if response.has_tool_calls() {
        results = execute_tool_calls(response.tool_calls)
        messages += tool_results
        continue
    }
    return response.content  // LLM 直接回复
}
```

### 工具清单（12 个）

#### A. 文件操作（3 个）

| 工具 | 说明 | 实现 |
|------|------|------|
| `read_file` | 读取文件内容 | `std::fs::read_to_string` + 大小限制 |
| `read_directory` | 读取目录结构 | `std::fs::read_dir`，递归列出 |
| `search_files` | 按模式搜索文件名 | glob 匹配，限制结果数量 |

#### B. 记忆管理（3 个）

| 工具 | 说明 | 实现 |
|------|------|------|
| `read_memory` | 读取记忆条目 | SQLite `zenix-ai.db` |
| `write_memory` | 写入记忆条目 | SQLite INSERT/REPLACE |
| `search_memory` | 全文搜索记忆 | FTS5 MATCH |

#### C. Agent 管理（4 个）

| 工具 | 说明 | 实现 |
|------|------|------|
| `list_agents` | 查看 Agent 状态 | herdr IPC `agent.list` |
| `start_agent` | 启动 Agent CLI | herdr IPC `agent.start` |
| `send_instruction` | 发送指令给 Agent | herdr IPC `agent.send` |
| `read_output` | 读取 Agent 输出 | herdr IPC `agent.read` |

#### D. 布局控制（2 个）

| 工具 | 说明 | 实现 |
|------|------|------|
| `split_pane` | 分割窗格 | herdr IPC `pane.split` |
| `arrange_grid` | 网状排列窗格 | herdr IPC `pane.arrange` |

### 工具接口

```rust
// ai/tools.rs

pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

pub struct ToolResult {
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;  // LLM 看到的描述
    fn execute(&self, args: serde_json::Value) -> ToolResult;
}

pub fn builtin_tools() -> Vec<Box<dyn Tool>>;
```

### 实现阶段

**Phase 1（MVP）：** 文件操作（3）+ 记忆（3）+ Agent 管理（4）= 10 个
**Phase 2：** 布局控制（2）= 2 个

### 文件位置

- `ai/tools.rs` — 工具定义和实现
- `ai/engine.rs` — 重写 `chat_and_dispatch` 为工具调用循环

---

## 10.7 会话压缩（TODO #6, 2026-07-01 设计）

### 需求

- 长对话时自动压缩，避免超出 LLM 上下文窗口
- 提取要点作为摘要，保留最近对话
- 对用户无感（自动处理，不弹出提示）

### 策略

参考 pi_agent_rust 的压缩算法：

1. **估算 token 使用量**：每 3 个字符 ≈ 1 token（保守估计）
2. **触发条件**：当 token 使用量 > 上下文窗口的 80% 时触发
3. **压缩方式**：
   - 保留最近 N 条消息（默认保留最近 20%）
   - 让 LLM 生成前面对话的摘要
   - 用摘要替换被压缩的对话
4. **压缩记录**：在数据库中记录压缩历史（`compaction_entries` 表）

### 数据库扩展

```sql
CREATE TABLE IF NOT EXISTS compaction_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    summary TEXT NOT NULL,
    kept_from_sequence INTEGER NOT NULL,
    compressed_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### 实现

- `ai/compactor.rs` — 压缩逻辑
- 在 `chat_and_dispatch` 循环中，每次 LLM 调用前检查 token 使用量
- 触发压缩后，将摘要作为 system message 注入下一轮对话

### 参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `context_window_tokens` | 128000 | 上下文窗口大小 |
| `reserve_tokens` | 10240 | 预留 token（~8%）|
| `keep_recent_ratio` | 0.2 | 保留最近消息比例 |

---

## 10.8 Rust Agent CLI 参考（2026-07-01 调查结论）

### 调查项目

| 项目 | Stars | 最近更新 | 语言 | 特点 |
|------|-------|---------|------|------|
| **pi_agent_rust** | 1,238 | 2026-06-30 | Rust | 8 个内置工具，安全优先，零 unsafe code |
| **VTCode** | 722 | 2026-06-30 | Rust | Zed ACP 支持，MCP client/server，21+ LLM provider |
| **shai** | 619 | 2026-06-28 | Rust | 简洁，HTTP 服务模式，SSE 流式 |

### 参考优先级

| 优先级 | 模块 | 参考项目 | 参考文件 | 理由 |
|--------|------|---------|---------|------|
| **P0** | 工具系统 | pi_agent_rust | `src/tools.rs` | 最完整，12 个工具设计直接可用 |
| **P0** | Function Calling | pi_agent_rust | `src/tools.rs` + `src/agent.rs` | 架构最清晰，循环调用逻辑明确 |
| **P1** | 会话压缩 | pi_agent_rust | `src/compaction.rs` | 算法成熟，参数设计合理 |
| **P2** | LLM 调用 | shai / pi_agent_rust | `shai-llm/src/chat.rs` / `src/sse.rs` | 我们的实现已够用，可优化 |
| **P3** | 记忆管理 | pi_agent_rust | `src/session_sqlite.rs` | SQLite 设计可参考 |

### 具体参考点

#### 工具系统（pi_agent_rust `src/tools.rs`）

**借鉴：**
- 工具 trait 设计（`Tool` trait + `ToolEffects` 效果声明）
- 8 个内置工具的参数设计（JSON Schema）
- 工具调用结果格式化（`ContentBlock`）

**不借鉴：**
- bash/edit/write 等执行类工具（ZenixAI 不需要）
- 能力门控（capability gating）（herdr IPC 已安全）

#### Function Calling（pi_agent_rust `src/agent.rs`）

**借鉴：**
- 工具调用循环架构（`call_llm_with_tools`）
- 工具结果注入（`ToolResult` → `ToolContent`）
- 多轮工具调用（LLM 可以连续调用多个工具）

**不借鉴：**
- 扩展系统（extensions）
- 安全沙箱（sandbox）

#### 会话压缩（pi_agent_rust `src/compaction.rs`）

**借鉴：**
- Token 估算算法（CHARS_PER_TOKEN_ESTIMATE = 3）
- 压缩触发条件（上下文窗口的 80%）
- 迭代式摘要更新（不是一次性压缩）
- 压缩记录（`CompactionEntry`）

**不借鉴：**
- 复杂的 token 计数器（我们用简化估算即可）
- 图片 token 估算（ZenixAI 是文本编排器）

#### LLM 调用（shai `shai-llm/src/chat.rs`）

**借鉴：**
- JSON hooks 设计（`JsonHooks` trait，可选优化）
- Provider 抽象（统一 OpenAI/Anthropic/Ollama 接口）

**不借鉴：**
- 12+ provider 实现（ZenixAI 只需要 1 个编排模型）
- 复杂的认证逻辑（我们用 API key 即可）

### 结论

**主要参考 pi_agent_rust**（工具系统 + Function Calling + 会话压缩）
**次要参考 shai**（LLM 调用简化实现）
**不参考 VTCode**（Zed ACP 协议对我们的价值有限，herdr IPC 已覆盖）

---

## 10.9 工具系统实现总结（2026-07-09 重写）

### 概述

2026-07-09 彻底重构了工具系统：

1. **清理旧 dispatch 路径**: 删除 `chat_and_dispatch()` + 6 个 dispatch 解析函数（~500 行），完全切换至 LLM function calling
2. **SQLite 记忆**: 内存 `HashMap` → SQLite `memories` 表 + FTS5
3. **6 个编排工具**: 替换原来的 12 个工具，文件操作提取为内部 `file_ops.rs` 模块
4. **修复 chat_stream_with_tools 空 POST bug**: 旧版发送空 HTTP POST（无 `.json(&payload)`），现在 `chat_with_tools()` 自构建 payload

### 已实现模块

| 模块 | 文件 | 说明 |
|------|------|------|
| 工具系统 | `src/ai/tools.rs` | Tool trait + ToolExecutor + 6 个工具 |
| 文件操作 | `src/ai/file_ops.rs` | 内部文件操作函数（read_file_contents, list_directory, search_files） |
| Herdr IPC | `src/ai/herdr.rs` | +`close_pane()` / `close_tab()` |
| 记忆持久化 | `src/ai/session.rs` | +`memories` 表 + FTS5 + 4 个方法 |
| SSE 解析 | `src/ai/engine.rs` | `parse_tool_call_sse_stream()` 独立函数 |

### 6 个编排工具

| 工具 | herdr IPC | 参数 | 说明 |
|------|-----------|------|------|
| `spawn_worker` | agent.start + pane.split | agent, instruction, cwd?, position? | 创建 worker，支持网格位置 |
| `send_text` | pane.send_text | pane_id, text | 向 worker 发送文本 |
| `read_output` | pane.read | pane_id, lines? | 读取 worker 输出 |
| `wait_for_output` | pane.wait_for_output | pane_id, pattern, timeout_ms? | 等待完成标记 |
| `list_workers` | agent.list | — | 列出运行中的 worker |
| `close_worker` | pane.close | pane_id | 关闭 worker pane |

### 已删除的工具（12 → 6）

| 分类 | 旧工具 (已删除) | 去向 |
|------|----------------|------|
| 文件操作 | `read_file`, `read_directory`, `search_files` | `file_ops.rs` (内部函数，非工具) |
| Agent 管理 | `list_agents`, `start_agent`, `send_instruction` | `list_workers`, `spawn_worker`, `send_text` (改名/合并) |
| Agent 管理 | `read_output` | 保留并改名 (agent → pane 层级) |
| 布局控制 | `split_pane`, `arrange_grid` | 合并到 `spawn_worker` (position 参数) |
| 记忆管理 | `read_memory`, `write_memory`, `search_memory` | SQLite `session.rs` 方法 (非工具) |

### 工具接口

```rust
// ai/tools.rs

pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

pub struct ToolResult {
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    fn execute(&self, executor: &ToolExecutor, args: serde_json::Value) -> ToolResult;
}

// ToolExecutor — 持有 herdr IPC + SessionManager
pub struct ToolExecutor { ... }

impl ToolExecutor {
    pub fn new(
        herdr_client: HerdrIpcClient,
        workspace_id: String,
        session_manager: SessionManager,
    ) -> Self;
    pub fn execute(&self, call: &ToolCall) -> ToolResult;
    pub fn available_tools() -> Vec<Box<dyn Tool>>;
}
```

### Function Calling 循环

```rust
// OrchestratorEngine::chat_with_tools()
// 1. 自动注入 system prompt（如果首条不是 system 角色）
if loop_messages.first().map(|m| m.role) != Some("system") {
    loop_messages.insert(0, ChatMessageRef {
        role: "system", content: self.system_prompt.clone()
    });
}

loop {
    // 2. 压缩检查
    if compactor.should_compress(&refs) {
        loop_messages = compactor.compress_messages(&refs).kept_messages;
    }

    // 3. 构建 payload（含 6 个工具定义）+ SSE 流式调用
    let payload = build_tool_call_payload(&loop_messages, tools, &model);
    let tool_calls = parse_tool_call_sse_stream(resp, |event| { ... }).await;

    // 4. 执行工具
    if !tool_calls.is_empty() {
        let results = tool_calls.iter().map(|tc| executor.execute(tc)).collect();
        loop_messages.extend(results_as_messages(results));
        continue;
    }

    // 5. LLM 直接回复
    return Ok(collected_text);
}
```

### 设计决策

1. **工具集从 LLM 视角命名** — `spawn_worker` 而非 `start_agent`，`send_text` 而非 `send_instruction`
2. **Tool trait 通过 ToolExecutor 访问状态** — `execute(&self, executor: &ToolExecutor, args)`, 而非 `execute(&self, args)`
3. **ToolExecutor 位于 tools.rs** — 解决 engine.rs/tools.rs 循环依赖
4. **SSE 解析独立函数** — `parse_tool_call_sse_stream()` 从 `chat_stream_with_tools` 提取，供 `chat_with_tools` 和 `chat_simple_with_tools` 复用
5. **记忆持久化 SQLite** — `memories` 表 + FTS5 全文搜索，替代内存 HashMap
6. **文件操作降级为内部函数** — 不再通过 LLM 调用，由 orchestrator 内部使用

### 测试

- 172 测试通过（ci test）
- 覆盖：6 个工具参数校验、`ToolExecutor` 分派、SSE 工具调用解析（OpenAI + Anthropic）、工具 payload 构建、记忆 CRUD + FTS5
- cargo check + clippy 零新增警告
