# 多Agent编排调度 — 技术设计

## 1. 项目结构

### 新增模块

```
src/
  ai/                          # AI 编排模块
    mod.rs                     # Module root, re-exports
    chat.rs                    # Chat panel UI (消息列表、输入框、底部栏)
    session.rs                 # 会话管理 (CRUD、生命周期、FTS5 搜索)
    engine.rs                  # 编排器引擎 (LLM API client、规划、调度)
    herdr.rs                   # Herdr IPC 客户端 (pane 操作、agent 管理、事件订阅)
    config.rs                  # ZenixAI 配置 (从 ConfigFile 读取/写入)
```

### 现有模块的修改

```
src/
  app.rs                       # 新增 AiChat 面板状态、事件处理
  ui/
    sidebar.rs                 # Panel 枚举增加 AiChat 变体
    mod.rs                     # 新增 re-export
  Cargo.toml                   # 新增依赖: reqwest, rusqlite
```

## 2. 架构与数据流

### 整体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                        ZenixApp                                 │
│  ┌──────────┐  ┌──────────────┐  ┌────────────┐  ┌──────────┐  │
│  │ Terminal  │  │  AI Chat     │  │ SystemInfo │  │ Sidebar  │  │
│  │ (main)   │  │  Panel       │  │ Panel      │  │ Icons    │  │
│  └──────────┘  └──────┬───────┘  └────────────┘  └──────────┘  │
│                        │                                         │
│  ┌─────────────────────┼─────────────────────────────────────┐  │
│  │  AI Orchestrator     │                                     │  │
│  │                     ▼                                       │  │
│  │  ┌──────────────────────────────────────────────────────┐  │  │
│  │  │  OrchestratorEngine                                  │  │  │
│  │  │  ┌──────────────┐  ┌────────────────────────────┐    │  │  │
│  │  │  │ LLM Client   │  │ Plan/Dispatch Logic        │    │  │  │
│  │  │  │ (SSE stream) │  │ (decompose, assign, monitor)│    │  │  │
│  │  │  └──────┬───────┘  └───────────┬────────────────┘    │  │  │
│  │  └─────────┼──────────────────────┼──────────────────────┘  │  │
│  │            │                      │                          │  │
│  │            ▼                      ▼                          │  │
│  │  ┌──────────────────────────────────────────────────────┐  │  │
│  │  │  HerdrIpcClient                                      │  │  │
│  │  │  (Unix socket, JSON-line protocol)                   │  │  │
│  │  └──────────────────────┬───────────────────────────────┘  │  │
│  └─────────────────────────┼────────────────────────────────────┘  │
│                            │                                        │
│                            ▼                                        │
│                    ┌───────────────┐                                │
│                    │  herdr daemon │                                │
│                    │  (外部进程)     │                                │
│                    └───────┬───────┘                                │
│                            │                                        │
│              ┌─────────────┼─────────────┐                         │
│              ▼             ▼             ▼                         │
│         ┌─────────┐  ┌─────────┐  ┌─────────┐                     │
│         │ Claude  │  │  Codex  │  │ OpenCode │  ...                │
│         │ Code    │  │         │  │          │                     │
│         │ (pane)  │  │ (pane)  │  │ (pane)   │                     │
│         └─────────┘  └─────────┘  └─────────┘                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 用户请求 → 编排 → 执行 → 响应 数据流

```
1. 用户输入消息 → ChatPanel.on_submit()
2.   → ZenixApp.handle_ai_message()
3.     → OrchestratorEngine.chat(messages)  // SSE 流式
4.       → LLM API: POST /v1/chat/completions
5.       ← SSE stream: tokens 实时写入 ChatPanel
6.   ← 流式 tokens 渲染到聊天气泡
7.     → OrchestratorEngine 解析 LLM 输出
8.       如果 LLM 决定需要执行 agent 任务:
9.         → encode agent 指令
10.        → HerdrIpcClient.send_text(pane_id, instruction)
11.        → HerdrIpcClient.wait_for_output(pane_id, pattern)
12.        → 实时输出推送到 ChatPanel agent block
13.   ← agent 完成 → OrchestratorEngine 汇总结果
14.     → OrchestratorEngine 再次调用 LLM 生成最终回复
15.   ← 最终结果渲染到 ChatPanel
16. → SessionManager.save_message()
```

### 编排器 ↔ LLM 通信

编排器使用标准 Chat Completions API，通过 SSE 流式接收响应：

```json
POST /v1/chat/completions
{
  "model": "gpt-4o-mini",
  "messages": [
    {"role": "system", "content": "你是 ZenixAI 编排器..."},
    {"role": "user", "content": "给项目加登录功能"}
  ],
  "stream": true
}
```

编排器的 system prompt 定义了它的职责：规划、调度、不直接写代码。

### 编排器 ↔ Agent CLI 通信

编排器不直接与 Agent CLI 对话。它通过 **herdr IPC** 发送文本指令、读取输出：

```
// 编排器通过 herdr 的 pane 操作间接控制 agent
HerdrIpcClient:
  // 1. 在对应 workspace 创建 pane
  send({"cmd": "pane.split", "tab_id": "...", "cwd": "/project/path"})
  
  // 2. 在 pane 中启动 agent CLI
  send({"cmd": "agent.start", "pane_id": "...", "agent": "claude"})
  
  // 3. 发送任务指令
  send({"cmd": "pane.send_text", "pane_id": "...", "text": "请实现登录功能..."})
  
  // 4. 等待输出（匹配完成标记）
  response = send({"cmd": "pane.wait_for_output", "pane_id": "...", "match": {"substring": "..."}, "timeout": 300000})
  
  // 5. 读取完整输出
  output = send({"cmd": "pane.read", "pane_id": "...", "source": "recent"})
```

## 3. 组件设计

### 3.1 SessionManager (`ai/session.rs`)

```rust
pub struct Session {
    pub id: String,
    pub title: String,          // 编排器自动命名
    pub created_at: String,
    pub updated_at: String,
    pub workspace: Option<String>,
    pub provider_id: String,
    pub model: String,
    pub thinking_level: ThinkingLevel,
}

pub struct ChatMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String,             // "user" | "orchestrator"
    pub content: String,
    pub agent_activities: Option<Vec<AgentActivity>>,  // JSON
    pub timestamp: String,
    pub sequence: i64,
}

pub struct AgentActivity {
    pub agent_name: String,
    pub status: String,          // "running" | "completed" | "failed"
    pub summary: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

pub struct SessionManager {
    conn: Connection,           // rusqlite
}

impl SessionManager {
    pub fn open() -> Result<Self>;
    pub fn list_sessions(&self) -> Result<Vec<Session>>;
    pub fn get_session(&self, id: &str) -> Result<Option<Session>>;
    pub fn create_session(&self, session: &Session) -> Result<()>;
    pub fn update_session(&self, session: &Session) -> Result<()>;
    pub fn delete_session(&self, id: &str) -> Result<()>;
    pub fn add_message(&self, msg: &ChatMessage) -> Result<()>;
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>>;
    pub fn search_messages(&self, query: &str, session_id: Option<&str>) -> Result<Vec<SearchHit>>;
}
```

### 3.2 OrchestratorEngine (`ai/engine.rs`)

```rust
pub struct OrchestratorEngine {
    http_client: reqwest::Client,
}

pub enum StreamEvent {
    Token(String),              // SSE token chunk
    AgentDispatch(AgentTask),   // 编排器决定分发 agent
    AgentOutput(String),        // Agent 实时输出
    AgentComplete(AgentResult), // Agent 执行完成
    Error(String),              // 错误
    Done,                       // 流结束
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
    pub exit_status: String,
    pub output: String,
    pub summary: Option<String>,
}

impl OrchestratorEngine {
    pub fn new(provider: &ApiProviderConfig) -> Self;
    
    pub async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        on_event: impl Fn(StreamEvent),
    ) -> Result<()>;
}
```

### 3.3 HerdrIpcClient (`ai/herdr.rs`)

```rust
pub struct HerdrIpcClient {
    socket_path: PathBuf,
}

pub struct PaneInfo {
    pub id: String,
    pub title: String,
    pub cwd: String,
}

pub struct AgentStatus {
    pub pane_id: String,
    pub agent_name: String,
    pub status: String,     // "idle" | "working" | "blocked" | "done"
    pub recent_output: String,
}

impl HerdrIpcClient {
    pub fn connect() -> Result<Self>;
    
    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>>;
    pub fn create_pane(&self, workspace_id: &str, cwd: &str) -> Result<PaneInfo>;
    pub fn start_agent(&self, pane_id: &str, agent_name: &str) -> Result<()>;
    pub fn send_text(&self, pane_id: &str, text: &str) -> Result<()>;
    pub fn read_output(&self, pane_id: &str, source: &str) -> Result<String>;
    pub fn wait_for_output(&self, pane_id: &str, pattern: &str, timeout_ms: u64) -> Result<String>;
    pub fn list_agent_statuses(&self) -> Result<Vec<AgentStatus>>;
    pub fn subscribe_events(&self) -> Result<EventStream>;
}
```

### 3.4 ChatPanel UI (`ai/chat.rs`)

核心 UI 组件，渲染在面板区域。使用 GPUI 标准模式：

```rust
pub struct AiChatPanel {
    session_manager: SessionManager,
    current_session_id: String,
    messages: Vec<ChatMessage>,
    input_state: Entity<InputState>,
    is_streaming: bool,
    show_history: bool,
    show_agents: bool,
    agent_statuses: Vec<AgentStatus>,
    // 配置
    selected_provider_id: String,
    selected_model: String,
    selected_workspace: Option<String>,
    thinking_level: ThinkingLevel,
}
```

聊天面板渲染在 `h_flex` 布局的 inline panel 位置（与 SystemInfo 同级）。

## 4. UI 布局实现

### 当前 render 布局 (app.rs)

```rust
h_flex()
    .child(div().flex_grow(1.0).h_full().child(terminal_area))
    .children(panel)                 // 可选的 inline panel
    .child(sidebar(...))             // 图标栏
```

### 实现方式

修改 `Panel` 枚举 → 增加 `AiChat` 变体：

```rust
// ui/sidebar.rs
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    None,
    SystemInfo,
    AiChat,
}
```

sidebar 新增 AI 图标按钮。当 `active_panel == Panel::AiChat` 时，panel 位置渲染 AiChatPanel。

### 面板宽度

- SystemInfo: `PANEL_WIDTH = 280.0`
- AiChat: `AI_PANEL_WIDTH = 380.0` （聊天需要更宽）

## 5. 数据持久化

### SQLite Schema

```sql
-- ~/.config/zenix/ai/zenix-ai.db

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    workspace TEXT,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    thinking_level INTEGER NOT NULL DEFAULT 5
);

CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'orchestrator')),
    content TEXT NOT NULL,
    agent_activities TEXT,  -- JSON array
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    sequence INTEGER NOT NULL
);

CREATE VIRTUAL TABLE messages_fts USING fts5(
    content,
    content=messages,
    content_rowid=id
);

-- Triggers to keep FTS in sync
CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
END;

CREATE TRIGGER messages_au AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;
```

### FTS5 搜索

```rust
pub fn search_messages(&self, query: &str, session_id: Option<&str>) -> Result<Vec<SearchHit>> {
    // 使用 FTS5 的 MATCH 语法
    // 搜索词自动包裹引号防止 FTS5 语法错误
    let safe_query = query.split_whitespace()
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ");
    
    let sql = match session_id {
        Some(sid) => "
            SELECT m.id, m.session_id, m.role, m.content, m.sequence, m.timestamp
            FROM messages_fts fts
            JOIN messages m ON fts.rowid = m.id
            WHERE messages_fts MATCH ?1 AND m.session_id = ?2
            ORDER BY rank
            LIMIT 20
        ",
        None => "
            SELECT m.id, m.session_id, m.role, m.content, m.sequence, m.timestamp
            FROM messages_fts fts
            JOIN messages m ON fts.rowid = m.id
            WHERE messages_fts MATCH ?1
            ORDER BY rank
            LIMIT 20
        ",
    };
    // ...
}
```

## 6. 依赖变更

```toml
# Cargo.toml 新增依赖
reqwest = { version = "0.12", features = ["json", "stream"] }
rusqlite = { version = "0.32", features = ["bundled"] }
futures = "0.3"        # Stream trait for SSE
tokio = { version = "1", features = ["rt", "macros"] }  # reqwest 需要 async runtime
```

GPUI 已有自己的 `cx.background_executor()` 和 `cx.spawn()`，但 `reqwest` 需要 tokio runtime。GPUI 底层使用 `async-task` + `smol`，`reqwest` 可以通过其 `tokio` feature 运行，需要确保 tokio runtime 已初始化。

### 方案：在 main.rs 中初始化 tokio runtime

```rust
fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();  // 让 reqwest 能找到 tokio runtime
    
    App::new().run(...);
}
```

或者使用 `reqwest::Client` 的 `async` 方法配合 GPUI 的 `cx.spawn()`：

```rust
// GPUI 的 cx.spawn() 使用 background_executor() 提供的 runtime
// reqwest 默认需要 tokio。可以在 build_client() 时设置
fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .build()
        .expect("reqwest client")
}
```

但要注意：GPUI 的 `cx.spawn()` 返回的 future 运行在 GPUI 的 background executor（smol-based）上，而 `reqwest` 需要 tokio。需要额外处理。

**替代方案**：使用 `ureq` 的非流式接口 + 轮询（不推荐 — 无法实现真正的 SSE 流式效果）

**推荐方案**：在 `main.rs` 中用 `#[tokio::main]` 或手动创建 tokio runtime，通过 `cx.spawn()` 中使用 `tokio::spawn` 来驱动 reqwest。

## 7. 实现阶段

### Phase 1: 基础 Chat Panel + 编排器引擎（可独立验证）

文件改动：
- `Cargo.toml` → 添加 `reqwest`, `rusqlite`, `tokio`, `futures`
- `main.rs` → 初始化 tokio runtime
- `ui/sidebar.rs` → Panel 增加 `AiChat`，sidebar 增加 AI 图标
- `app.rs` → AiChat panel 渲染、事件处理
- `ai/mod.rs` → 新模块入口
- `ai/session.rs` → SessionManager、SQLite schema
- `ai/chat.rs` → AiChatPanel UI（消息列表、输入框、底部栏、搜索按钮）
- `ai/engine.rs` → OrchestratorEngine（LLM API 调用、SSE 流式）

验证标准：
- ✅ 右面板显示 AI Chat 聊天界面
- ✅ 输入消息 → 编排器 LLM 流式回复
- ✅ 会话创建/切换/删除
- ✅ SQLite 持久化 + FTS5 搜索

### Phase 2: Herdr Agent 集成（依赖 Phase 1）

文件改动：
- `ai/herdr.rs` → HerdrIpcClient
- `ai/engine.rs` → 扩展编排器逻辑（agent dispatch、监控）

验证标准：
- ✅ 编排器可在 herdr workspace 创建 pane
- ✅ 可启动 Agent CLI 并发送指令
- ✅ 可读取 agent 输出
- ✅ Agent 输出在 chat 中显示为内联区块

### Phase 3: Agent 监控 + 配置同步（独立）

文件改动：
- `ai/chat.rs` → 👤 状态浮层、运行中计数
- 已有 `api_provider/` 的 sync 功能增强

验证标准：
- ✅ 👤 按钮显示运行中 agent 数量和状态
- ✅ API Provider 同步到所有 Agent CLI

### Phase 4: 异常处理完善

- 编排器级别：超时重试、agent 切换
- UI 级别：错误提示、重试按钮

## 8. 关键设计决策

| 决策 | 选择 | 原因 |
|------|------|------|
| SSE 实现 | reqwest + tokio | ureq 不支持流式；zenix 需要异步 HTTP |
| 持久化 | SQLite (rusqlite bundled) | 无需系统依赖，FTS5 内建搜索 |
| 编排器通信 | Chat Completions API (标准) | 兼容 OpenAI/Anthropic/ollama 等 |
| Agent CLI 通信 | herdr pane I/O (文本通道) | 通用性最强，无需 agent 适配额外协议 |
| 面板位置 | inline panel（与 SystemInfo 同级） | 侵入最小，复用现有布局机制 |
| 状态管理 | ZenixApp struct 直接持有 | 遵循现有模式（不做独立状态管理器） |
