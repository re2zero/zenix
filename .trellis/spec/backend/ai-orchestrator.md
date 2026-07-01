# AI 编排器模块

> 多 Agent 编排调度：LLM 编排 + herdr IPC + Agent CLI 调度

---

## 1. 模块结构

```
src/ai/
├── mod.rs          # Module root, re-exports
├── chat.rs         # Chat panel UI (AiChatPanelData, AgentBlock, AgentMonitorEntry, config bar, history, search)
├── session.rs      # SQLite 持久化 (CRUD、FTS5 搜索)
├── engine.rs       # 编排器引擎 (LLM API、SSE 流式、调度循环)
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
pub use engine::*;    // OrchestratorEngine, AgentTask, AgentResult, DispatchDirective, DispatchStreamEvent, parse_dispatch_directive, execute_agent_task
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
| `herdr.rs` | JSON 序列化/反序列化 round-trip（所有关键类型）、请求 ID 生成、响应解析（success/error/empty/invalid）、17 种 RPC 方法 wire protocol 校验 |
| `engine.rs` | `parse_dispatch_directive`（JSON code block、inline JSON、text format、malformed input）、`AgentResult` 构造、`execute_agent_task` 错误路径、`chat_and_dispatch` 调度循环 |
| `session.rs` | CRUD、FTS5 搜索（含特殊字符）、ON DELETE CASCADE、搜索范围过滤（仅 user 消息） |
| `chat.rs` | `AiChatPanelData` 构造、`AgentBlock` 折叠状态管理、`AgentMonitorEntry::from_herdr` 转换、底部栏按钮渲染、config bar pill-style selectors、history panel, agent monitor, search bar |

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

- **并行 Agent 执行**: 多 Agent 同时运行，提高复杂度任务效率
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

## 10.9 工具系统实现总结（2026-07-02）

### 已实现模块

| 模块 | 文件 | 行数 | 说明 |
|------|------|------|------|
| 工具系统 | `src/ai/tools.rs` | 1,956 | Tool trait + 12 个工具定义 |
| 会话压缩 | `src/ai/compactor.rs` | 492 | Compactor + token 估算 + 自动压缩 |
| Function Calling | `src/ai/engine.rs` | +885 | chat_with_tools + ToolExecutor + SSE 工具调用解析 |
| 配置隔离 | `src/app.rs` | +217 | 空配置渲染 |

### 12 个内置工具

| 分类 | 工具 | 实现状态 | herdr IPC |
|------|------|---------|-----------|
| 文件操作 | `read_file` | ✅ 完整 | ❌ |
| 文件操作 | `read_directory` | ✅ 完整 | ❌ |
| 文件操作 | `search_files` | ✅ 完整 | ❌ |
| Agent 管理 | `list_agents` | ✅ 完整 | ✅ agent.list |
| Agent 管理 | `start_agent` | ✅ 完整 | ✅ agent.start |
| Agent 管理 | `send_instruction` | ✅ 完整 | ✅ pane.send_text |
| Agent 管理 | `read_output` | ✅ 完整 | ✅ pane.read |
| 布局控制 | `split_pane` | ✅ 完整 | ✅ pane.split |
| 布局控制 | `arrange_grid` | ✅ 完整 | ✅ pane.arrange |
| 记忆管理 | `read_memory` | ✅ 内存 HashMap | ❌ |
| 记忆管理 | `write_memory` | ✅ 内存 HashMap | ❌ |
| 记忆管理 | `search_memory` | ✅ 内存 HashMap | ❌ |

### Function Calling 循环

```rust
// OrchestratorEngine::chat_with_tools()
loop {
    // 1. 检查是否需要压缩
    if compactor.should_compress(&loop_messages) {
        loop_messages = compactor.compress_messages(&loop_messages);
    }
    
    // 2. 调用 LLM（带工具定义）
    let (content, tool_calls) = chat_stream_with_tools(...).await?;
    
    // 3. 执行工具调用
    if !tool_calls.is_empty() {
        let results = tool_calls.iter().map(|tc| executor.execute(tc)).collect();
        loop_messages.push(tool_results_as_messages(results));
        continue;
    }
    
    // 4. LLM 直接回复，退出循环
    return Ok(content);
}
```

### 设计决策

1. **工具集严格限定在编排视角** — 无 bash/edit/write 等执行类工具
2. **Tool trait 无状态** — 工具通过 ToolExecutor 持有 herdr_client
3. **Token 估算简化** — chars/3 + 10 overhead，不引入 token counter 依赖
4. **记忆管理内存实现** — 后续接 SQLite，不阻塞当前迭代
5. **SSE 双格式支持** — OpenAI + Anthropic 工具调用解析

### 测试

- 194 → 215 测试（新增 21 个）
- 覆盖：工具参数校验、glob 匹配、压缩阈值、Function Calling 循环、配置检查
- clippy 新代码零警告
