# SQLite 持久化层 — 设计

## SQLite Schema

```sql
-- ~/.config/zenix/ai/zenix-ai.db

-- 确保 journal_mode=WAL（并发读性能）
PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS sessions (
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

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'orchestrator')),
    content TEXT NOT NULL,
    agent_activities TEXT,  -- JSON array of AgentActivity
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    sequence INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_session
    ON messages(session_id, sequence);

-- FTS5 虚拟表（仅索引 user 角色的 content，方便用户搜索自己的输入）
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    content=messages,
    content_rowid=id,
    tokenize='unicode61'
);

-- 触发器：同步 messages INSERT → FTS
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;

-- 触发器：同步 messages DELETE → FTS
CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
END;

-- 触发器：同步 messages UPDATE → FTS
CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;
```

## SessionManager 实现

```rust
use rusqlite::{Connection, params};
use std::path::PathBuf;
use crate::platform;

pub struct SessionManager {
    conn: Connection,
}

impl SessionManager {
    pub fn open() -> Result<Self> {
        let db_path = platform::zenix_config_dir().join("ai").join("zenix-ai.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA_SQL)?;  // CREATE TABLE IF NOT EXISTS
        Ok(Self { conn })
    }
}
```

### Session CRUD

```rust
pub fn list_sessions(&self) -> Result<Vec<Session>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, title, created_at, updated_at, workspace,
                provider_id, model, thinking_level
         FROM sessions ORDER BY updated_at DESC"
    )?;
    // map rows → Vec<Session>
}

pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
    // SELECT ... WHERE id = ?1
}

pub fn create_session(&self, session: &Session) -> Result<()> {
    // INSERT INTO sessions ...
}

pub fn update_session(&self, session: &Session) -> Result<()> {
    // UPDATE sessions SET ... WHERE id = ?1
}

pub fn delete_session(&self, id: &str) -> Result<()> {
    // 级联: DELETE FROM messages WHERE session_id = ?1
    //       DELETE FROM sessions WHERE id = ?1
    // 或者依靠 ON DELETE CASCADE
}
```

### Message CRUD

```rust
pub fn add_message(&self, msg: &ChatMessage) -> Result<()> {
    // INSERT INTO messages (session_id, role, content, agent_activities, timestamp, sequence)
    // VALUES (?1, ?2, ?3, ?4, ?5, ?6)
}

pub fn get_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
    // SELECT ... FROM messages WHERE session_id = ?1 ORDER BY sequence ASC
}
```

### FTS5 Search

```rust
pub fn search_messages(&self, query: &str, session_id: Option<&str>) -> Result<Vec<SearchHit>> {
    // 安全处理特殊字符：每个词包裹双引号
    let safe_query: String = query.split_whitespace()
        .map(|w| {
            let cleaned: String = w.chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_')
                .collect();
            if cleaned.is_empty() { "\"\"".into() }
            else { format!("\"{}\"", cleaned) }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let sql = match session_id {
        Some(sid) => "
            SELECT m.id, m.session_id, m.role, m.content, m.agent_activities,
                   m.timestamp, m.sequence, s.title,
                   snippet(messages_fts, 0, '<b>', '</b>', '...', 32) as snippet
            FROM messages_fts fts
            JOIN messages m ON fts.rowid = m.id
            JOIN sessions s ON m.session_id = s.id
            WHERE messages_fts MATCH ?1 AND m.session_id = ?2
            ORDER BY rank
            LIMIT 20
        ",
        None => "
            SELECT m.id, m.session_id, m.role, m.content, m.agent_activities,
                   m.timestamp, m.sequence, s.title,
                   snippet(messages_fts, 0, '<b>', '</b>', '...', 32) as snippet
            FROM messages_fts fts
            JOIN messages m ON fts.rowid = m.id
            JOIN sessions s ON m.session_id = s.id
            WHERE messages_fts MATCH ?1
            ORDER BY rank
            LIMIT 20
        ",
    };
    // execute, map rows → Vec<SearchHit>
}
```

## 错误处理

所有方法返回 `anyhow::Result<T>`，调用方（chat panel）负责展示错误。

```rust
pub type Result<T> = std::result::Result<T, anyhow::Error>;
```

## 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_list_sessions() {
        let mgr = SessionManager::open_in_memory().unwrap();
        // ...
    }

    #[test]
    fn test_add_and_get_messages() {
        // ...
    }

    #[test]
    fn test_fts_search() {
        // ...
    }

    #[test]
    fn test_fts_special_chars() {
        // 搜索 "test!@#$" 不崩溃
    }

    #[test]
    fn test_delete_session_cascades() {
        // 删除 session 后 messages 也被删除
    }

    #[test]
    fn test_search_with_session_filter() {
        // 指定 session_id 过滤
    }
}
```
