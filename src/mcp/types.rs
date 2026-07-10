use serde::{Deserialize, Serialize};

/// A message sent between workers via the MCP communication bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMessage {
    /// Sender worker name
    pub from: String,
    /// Recipient worker name
    pub to: String,
    /// Message content
    pub text: String,
    /// ISO 8601 timestamp
    pub timestamp: String,
}

impl WorkerMessage {
    /// Memory key prefix for worker messages stored in SQLite.
    pub const MEMORY_KEY_PREFIX: &'static str = "worker_msg:";

    /// Build the SQLite memory key for a recipient's inbox entry.
    pub fn inbox_key(recipient: &str, message_id: &str) -> String {
        format!("{}{}:{}", Self::MEMORY_KEY_PREFIX, recipient, message_id)
    }

    /// Parse a memory key back into recipient and message_id.
    pub fn parse_inbox_key(key: &str) -> Option<(String, String)> {
        let rest = key.strip_prefix(Self::MEMORY_KEY_PREFIX)?;
        let colon = rest.find(':')?;
        let recipient = rest[..colon].to_string();
        let message_id = rest[colon + 1..].to_string();
        Some((recipient, message_id))
    }
}
