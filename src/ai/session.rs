//! SQLite persistence layer for AI orchestrator sessions.
//!
//! Database location: `~/.config/zenix/ai/zenix-ai.db`

use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::platform;

pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub workspace: Option<String>,
    pub provider_id: String,
    pub model: String,
    pub thinking_level: String,
}

#[derive(Clone)]
pub struct ChatMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub agent_activities: Option<String>,
    /// Reasoning/thinking content shown as a collapsible block above the
    /// main text. Set during streaming via `DispatchStreamEvent::Thinking`.
    /// Not persisted to SQLite — ephemeral display-only field.
    pub reasoning_text: Option<String>,
    /// Elapsed time for reasoning in milliseconds, set on stream completion.
    /// Displayed in the collapsed reasoning block header as "Thought: Xms".
    pub reasoning_duration_ms: Option<u64>,
    pub timestamp: String,
    pub sequence: i64,
}

#[allow(dead_code)]
pub struct AgentActivity {
    pub agent_name: String,
    /// "running" | "completed" | "failed"
    pub status: String,
    pub summary: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[allow(dead_code)]
pub struct SearchHit {
    pub message: ChatMessage,
    pub session_title: String,
    pub snippet: String,
}

#[allow(dead_code)]
impl AgentActivity {
    pub fn to_json(activities: &[AgentActivity]) -> Result<String> {
        let items: Vec<serde_json::Value> = activities
            .iter()
            .map(|a| {
                serde_json::json!({
                    "agent_name": a.agent_name,
                    "status": a.status,
                    "summary": a.summary,
                    "started_at": a.started_at,
                    "completed_at": a.completed_at,
                })
            })
            .collect();
        serde_json::to_string(&items).context("serialize agent_activities")
    }

    pub fn from_json(json: &str) -> Result<Vec<AgentActivity>> {
        let items: Vec<serde_json::Value> =
            serde_json::from_str(json).context("parse agent_activities JSON")?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            out.push(AgentActivity {
                agent_name: item
                    .get("agent_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                status: item
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                summary: item
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                started_at: item
                    .get("started_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                completed_at: item
                    .get("completed_at")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });
        }
        Ok(out)
    }
}

const SCHEMA_SQL: &str = r#"
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
    agent_activities TEXT,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    sequence INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_session
    ON messages(session_id, sequence);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    content=messages,
    content_rowid=id,
    tokenize='unicode61'
);

-- Only index user-role messages (PRD: 搜索范围 = user content only)
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages
    WHEN new.role = 'user'
BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages
    WHEN old.role = 'user'
BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
END;

CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages
    WHEN old.role = 'user'
BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES('delete', old.id, old.content);
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;

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

CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories
BEGIN
    INSERT INTO memories_fts(rowid, key, value) VALUES (new.id, new.key, new.value);
END;

CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories
BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, key, value) VALUES('delete', old.id, old.key, old.value);
END;

CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories
BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, key, value) VALUES('delete', old.id, old.key, old.value);
    INSERT INTO memories_fts(rowid, key, value) VALUES (new.id, new.key, new.value);
END;
"#;

pub struct SessionManager {
    conn: Mutex<Connection>,
}

#[allow(dead_code)]
impl SessionManager {
    pub fn open() -> Result<Self> {
        let db_path = platform::zenix_config_dir().join("ai").join("zenix-ai.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create db dir {:?}", parent))?;
        }
        let conn =
            Connection::open(&db_path).with_context(|| format!("open sqlite db {:?}", db_path))?;
        Self::init(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("open in-memory db")?;
        Self::init(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init(conn: &Connection) -> Result<()> {
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .context("set journal_mode=WAL")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .context("enable foreign_keys")?;
        conn.execute_batch(SCHEMA_SQL).context("apply schema")?;
        Ok(())
    }

    /// Returns sessions ordered by `updated_at` DESC.
    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, title, created_at, updated_at, workspace,
                        provider_id, model, thinking_level
                 FROM sessions ORDER BY updated_at DESC",
            )
            .context("prepare list_sessions")?;
        let rows = stmt
            .query_map([], map_session_row)
            .context("query list_sessions")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("read session row")?);
        }
        Ok(out)
    }

    /// Returns `None` if the session id does not exist.
    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, title, created_at, updated_at, workspace,
                        provider_id, model, thinking_level
                 FROM sessions WHERE id = ?1",
            )
            .context("prepare get_session")?;
        let mut rows = stmt
            .query_map(params![id], map_session_row)
            .context("query get_session")?;
        match rows.next() {
            Some(row) => Ok(Some(row.context("read session row")?)),
            None => Ok(None),
        }
    }

    pub fn create_session(&self, session: &Session) -> Result<()> {
        self.conn.lock().unwrap()
            .execute(
                "INSERT INTO sessions
                    (id, title, created_at, updated_at, workspace,
                     provider_id, model, thinking_level)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    session.id,
                    session.title,
                    session.created_at,
                    session.updated_at,
                    session.workspace,
                    session.provider_id,
                    session.model,
                    session.thinking_level,
                ],
            )
            .context("insert session")?;
        Ok(())
    }

    pub fn update_session(&self, session: &Session) -> Result<()> {
        self.conn.lock().unwrap()
            .execute(
                "UPDATE sessions SET
                    title = ?2,
                    updated_at = ?3,
                    workspace = ?4,
                    provider_id = ?5,
                    model = ?6,
                    thinking_level = ?7
                 WHERE id = ?1",
                params![
                    session.id,
                    session.title,
                    session.updated_at,
                    session.workspace,
                    session.provider_id,
                    session.model,
                    session.thinking_level,
                ],
            )
            .context("update session")?;
        Ok(())
    }

    /// Associated messages are removed via `ON DELETE CASCADE`.
    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.conn.lock().unwrap()
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])
            .context("delete session")?;
        Ok(())
    }

    pub fn add_message(&self, msg: &ChatMessage) -> Result<()> {
        self.conn.lock().unwrap()
            .execute(
                "INSERT INTO messages
                    (session_id, role, content, agent_activities, timestamp, sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    msg.session_id,
                    msg.role,
                    msg.content,
                    msg.agent_activities,
                    msg.timestamp,
                    msg.sequence,
                ],
            )
            .context("insert message")?;
        Ok(())
    }

    /// Returns messages ordered by `sequence` ASC.
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, role, content, agent_activities,
                        timestamp, sequence
                 FROM messages WHERE session_id = ?1 ORDER BY sequence ASC",
            )
            .context("prepare get_messages")?;
        let rows = stmt
            .query_map(params![session_id], map_message_row)
            .context("query get_messages")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("read message row")?);
        }
        Ok(out)
    }

    /// Full-text search over message content via FTS5.
    ///
    /// Each whitespace-separated token in `query` is wrapped in double quotes
    /// after stripping non-alphanumeric/underscore characters, making the
    /// search safe against FTS5 special characters. An optional `session_id`
    /// restricts the search to a single session.
    pub fn search_messages(&self, query: &str, session_id: Option<&str>) -> Result<Vec<SearchHit>> {
        let safe_query = sanitize_fts_query(query);
        if safe_query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let sql = match session_id {
            Some(_) => {
                "
                SELECT m.id, m.session_id, m.role, m.content, m.agent_activities,
                       m.timestamp, m.sequence, s.title,
                       snippet(messages_fts, 0, '<b>', '</b>', '...', 32) AS snippet
                FROM messages_fts fts
                JOIN messages m ON fts.rowid = m.id
                JOIN sessions s ON m.session_id = s.id
                WHERE messages_fts MATCH ?1 AND m.session_id = ?2
                ORDER BY rank
                LIMIT 20
            "
            }
            None => {
                "
                SELECT m.id, m.session_id, m.role, m.content, m.agent_activities,
                       m.timestamp, m.sequence, s.title,
                       snippet(messages_fts, 0, '<b>', '</b>', '...', 32) AS snippet
                FROM messages_fts fts
                JOIN messages m ON fts.rowid = m.id
                JOIN sessions s ON m.session_id = s.id
                WHERE messages_fts MATCH ?1
                ORDER BY rank
                LIMIT 20
            "
            }
        };

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(sql).context("prepare search_messages")?;

        let rows = if let Some(sid) = session_id {
            stmt.query_map(params![safe_query, sid], map_search_hit_row)
                .context("query search_messages (filtered)")?
        } else {
            stmt.query_map(params![safe_query], map_search_hit_row)
                .context("query search_messages (all)")?
        };

        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("read search hit row")?);
        }
        Ok(out)
    }

    // ── Memory Methods ─────────────────────────────────────────────────

    /// Write a key/value pair to the memories table (UPSERT).
    pub fn write_memory(&self, key: &str, value: &str, session_id: &str) -> Result<()> {
        self.conn.lock().unwrap()
            .execute(
                "INSERT INTO memories (key, value, session_id, updated_at)
                 VALUES (?1, ?2, ?3, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET
                     value = excluded.value,
                     session_id = excluded.session_id,
                     updated_at = datetime('now')",
                params![key, value, session_id],
            )
            .context("write memory")?;
        Ok(())
    }

    /// Read a memory entry by key.
    pub fn read_memory(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT value FROM memories WHERE key = ?1")
            .context("prepare read_memory")?;
        let mut rows = stmt
            .query_map(params![key], |row| row.get::<_, String>(0))
            .context("query read_memory")?;
        match rows.next() {
            Some(row) => Ok(Some(row.context("read memory row")?)),
            None => Ok(None),
        }
    }

    /// Search memory entries via FTS5.
    pub fn search_memory(&self, query: &str, limit: usize) -> Result<Vec<(String, String)>> {
        let safe_query = sanitize_fts_query(query);
        if safe_query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT m.key, m.value FROM memories_fts fts
             JOIN memories m ON fts.rowid = m.id
             WHERE memories_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2"
        );
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&sql).context("prepare search_memory")?;
        let rows = stmt
            .query_map(params![safe_query, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("query search_memory")?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("read search_memory row")?);
        }
        Ok(out)
    }

    /// Delete a memory entry.
    pub fn delete_memory(&self, key: &str) -> Result<()> {
        self.conn.lock().unwrap()
            .execute("DELETE FROM memories WHERE key = ?1", params![key])
            .context("delete memory")?;
        Ok(())
    }
}

#[allow(dead_code)]
fn map_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        workspace: row.get(4)?,
        provider_id: row.get(5)?,
        model: row.get(6)?,
        thinking_level: row.get(7)?,
    })
}

fn map_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatMessage> {
    Ok(ChatMessage {
        id: row.get(0)?,
        session_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        agent_activities: row.get(4)?,
        reasoning_text: None,
        reasoning_duration_ms: None,
        timestamp: row.get(5)?,
        sequence: row.get(6)?,
    })
}

#[allow(dead_code)]
fn map_search_hit_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchHit> {
    Ok(SearchHit {
        message: ChatMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            role: row.get(2)?,
            content: row.get(3)?,
            agent_activities: row.get(4)?,
            reasoning_text: None,
            reasoning_duration_ms: None,
            timestamp: row.get(5)?,
            sequence: row.get(6)?,
        },
        session_title: row.get(7)?,
        snippet: row.get(8)?,
    })
}

/// Sanitize a raw user query into a safe FTS5 MATCH expression.
///
/// Each whitespace-separated token is cleaned (only alphanumerics and
/// underscores retained) then wrapped in double quotes. This prevents FTS5
/// syntax errors from special characters like `!@#$%^&*()`.
#[allow(dead_code)]
fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|w| {
            let cleaned: String = w
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if cleaned.is_empty() {
                "\"\"".to_string()
            } else {
                format!("\"{}\"", cleaned)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(id: &str, title: &str) -> Session {
        Session {
            id: id.to_string(),
            title: title.to_string(),
            created_at: "2024-01-01T00:00:00".to_string(),
            updated_at: "2024-01-01T00:00:00".to_string(),
            workspace: None,
            provider_id: "test-provider".to_string(),
            model: "test-model".to_string(),
            thinking_level: "medium".to_string(),
        }
    }

    fn make_message(session_id: &str, role: &str, content: &str, seq: i64) -> ChatMessage {
        ChatMessage {
            id: 0,
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            agent_activities: None,
            reasoning_text: None,
            reasoning_duration_ms: None,
            timestamp: "2024-01-01T00:00:00".to_string(),
            sequence: seq,
        }
    }

    #[test]
    fn test_create_and_list_sessions() {
        let mgr = SessionManager::open_in_memory().unwrap();

        let s1 = make_session("s1", "First Chat");
        let s2 = make_session("s2", "Second Chat");
        mgr.create_session(&s1).unwrap();
        mgr.create_session(&s2).unwrap();

        let sessions = mgr.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        let titles: Vec<String> = sessions.iter().map(|s| s.title.clone()).collect();
        assert!(titles.contains(&"First Chat".to_string()));
        assert!(titles.contains(&"Second Chat".to_string()));
    }

    #[test]
    fn test_add_and_get_messages() {
        let mgr = SessionManager::open_in_memory().unwrap();

        let session = make_session("msg-session", "Message Test");
        mgr.create_session(&session).unwrap();

        mgr.add_message(&make_message("msg-session", "user", "Hello world", 1))
            .unwrap();
        mgr.add_message(&make_message("msg-session", "orchestrator", "Hi there", 2))
            .unwrap();
        mgr.add_message(&make_message("msg-session", "user", "How are you?", 3))
            .unwrap();

        let msgs = mgr.get_messages("msg-session").unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].sequence, 1);
        assert_eq!(msgs[0].content, "Hello world");
        assert_eq!(msgs[1].sequence, 2);
        assert_eq!(msgs[1].content, "Hi there");
        assert_eq!(msgs[2].sequence, 3);
        assert_eq!(msgs[2].content, "How are you?");
    }

    #[test]
    fn test_fts_search() {
        let mgr = SessionManager::open_in_memory().unwrap();

        let session = make_session("fts-session", "FTS Test");
        mgr.create_session(&session).unwrap();

        mgr.add_message(&make_message(
            "fts-session",
            "user",
            "The quick brown fox",
            1,
        ))
        .unwrap();
        mgr.add_message(&make_message(
            "fts-session",
            "orchestrator",
            "The lazy dog sleeps",
            2,
        ))
        .unwrap();

        let hits = mgr.search_messages("quick", None).unwrap();
        assert!(!hits.is_empty(), "should find 'quick'");
        let hit = &hits[0];
        assert_eq!(hit.message.content, "The quick brown fox");
        assert!(hit.snippet.contains("quick"));
    }

    #[test]
    fn test_fts_special_chars() {
        let mgr = SessionManager::open_in_memory().unwrap();

        let session = make_session("special-session", "Special Chars");
        mgr.create_session(&session).unwrap();

        mgr.add_message(&make_message(
            "special-session",
            "user",
            "testing special characters here",
            1,
        ))
        .unwrap();

        let hits = mgr.search_messages("test!@#$", None).unwrap();
        let _ = hits;

        let hits = mgr.search_messages("special", None).unwrap();
        assert!(!hits.is_empty(), "should find 'special'");
    }

    #[test]
    fn test_delete_session_cascades() {
        let mgr = SessionManager::open_in_memory().unwrap();

        let session = make_session("cascade-session", "Cascade Test");
        mgr.create_session(&session).unwrap();

        mgr.add_message(&make_message("cascade-session", "user", "msg 1", 1))
            .unwrap();
        mgr.add_message(&make_message("cascade-session", "user", "msg 2", 2))
            .unwrap();

        assert_eq!(mgr.get_messages("cascade-session").unwrap().len(), 2);

        mgr.delete_session("cascade-session").unwrap();

        assert!(mgr.get_session("cascade-session").unwrap().is_none());
        assert_eq!(mgr.get_messages("cascade-session").unwrap().len(), 0);
    }

    #[test]
    fn test_fts_search_only_user_messages() {
        let mgr = SessionManager::open_in_memory().unwrap();

        let session = make_session("scope-session", "Scope Test");
        mgr.create_session(&session).unwrap();

        mgr.add_message(&make_message(
            "scope-session",
            "user",
            "uniquekeyword here",
            1,
        ))
        .unwrap();
        mgr.add_message(&make_message(
            "scope-session",
            "orchestrator",
            "uniquekeyword in orchestrator",
            2,
        ))
        .unwrap();

        // Searching for "uniquekeyword" should only return the user message,
        // not the orchestrator message (PRD: 搜索范围 = user content only).
        let hits = mgr.search_messages("uniquekeyword", None).unwrap();
        assert_eq!(hits.len(), 1, "should only find user message");
        assert_eq!(hits[0].message.role, "user");
        assert_eq!(hits[0].message.content, "uniquekeyword here");
    }

    #[test]
    fn test_search_with_session_filter() {
        let mgr = SessionManager::open_in_memory().unwrap();

        let s1 = make_session("filter-s1", "Session One");
        let s2 = make_session("filter-s2", "Session Two");
        mgr.create_session(&s1).unwrap();
        mgr.create_session(&s2).unwrap();

        mgr.add_message(&make_message("filter-s1", "user", "shared keyword here", 1))
            .unwrap();
        mgr.add_message(&make_message(
            "filter-s2",
            "user",
            "shared keyword there",
            1,
        ))
        .unwrap();

        let all_hits = mgr.search_messages("shared", None).unwrap();
        assert_eq!(all_hits.len(), 2, "should find in both sessions");

        let s1_hits = mgr.search_messages("shared", Some("filter-s1")).unwrap();
        assert_eq!(s1_hits.len(), 1, "should find only in s1");
        assert_eq!(s1_hits[0].message.session_id, "filter-s1");

        let s2_hits = mgr.search_messages("shared", Some("filter-s2")).unwrap();
        assert_eq!(s2_hits.len(), 1, "should find only in s2");
        assert_eq!(s2_hits[0].message.session_id, "filter-s2");
    }
}
