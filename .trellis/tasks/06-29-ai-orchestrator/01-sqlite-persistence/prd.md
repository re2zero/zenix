# SQLite 持久化层

## 目标

实现 AI 编排侧边栏的会话历史存储层，基于 SQLite + FTS5，支持会话 CRUD、消息读写、全文搜索。

## 接口契约（下游使用者须知）

下游 `04-chat-panel-ui` 将使用此模块导出的 `SessionManager` 和数据结构。

```rust
// 在 src/ai/mod.rs 中 pub mod session;
// 在 src/ai/session.rs 中实现

pub struct Session {
    pub id: String,                    // UUID v4
    pub title: String,                 // 自动智能命名
    pub created_at: String,            // ISO 8601
    pub updated_at: String,
    pub workspace: Option<String>,      // herdr workspace name
    pub provider_id: String,            // 使用的 API provider ID
    pub model: String,                  // 模型名
    pub thinking_level: String,         // "low" | "medium" | "high" | "xhigh"
}

pub struct ChatMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String,                   // "user" | "orchestrator"
    pub content: String,
    pub agent_activities: Option<String>,  // JSON 序列化 AgentActivity[]
    pub timestamp: String,              // ISO 8601
    pub sequence: i64,                  // 会话内序号
}

pub struct AgentActivity {
    pub agent_name: String,
    pub status: String,                 // "running" | "completed" | "failed"
    pub summary: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

pub struct SearchHit {
    pub message: ChatMessage,
    pub session_title: String,
    pub snippet: String,                // 匹配上下文片段
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

## Requirements

- [ ] SessionManager::open() 创建/打开 SQLite 数据库
- [ ] 自动执行 CREATE TABLE / migration
- [ ] Session CRUD（list/get/create/update/delete）
- [ ] Message CRUD（add/get）
- [ ] FTS5 全文搜索（兼容特殊字符，session_id 可选过滤）
- [ ] 数据库路径：`~/.config/zenix/ai/zenix-ai.db`
- [ ] 搜索范围：用户输入过的内容（user 角色的 content）

## Acceptance Criteria

- [ ] `SessionManager::open()` 如果 db 不存在则自动创建并运行 migration
- [ ] `create_session` + `list_sessions` 能正确写入和读取
- [ ] `add_message` + `get_messages` 按 sequence 顺序返回
- [ ] `search_messages("some text")` 返回匹配的 message + context snippet
- [ ] `search_messages("特殊字符!@#$")` 不崩溃
- [ ] 删除 session 时级联删除其 messages
- [ ] 所有操作返回 `Result<T, String>`（使用 `anyhow::Context`）

## Out of Scope

- 数据库 migration 版本管理（第一版直接 CREATE TABLE IF NOT EXISTS）
- 消息编辑/删除（后续支持）
