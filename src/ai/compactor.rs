//! Context compaction for long conversations.
//!
//! When a conversation grows too long, the orchestrator engine can compact
//! (compress) the message history so it fits within the LLM's context window.
//!
//! This module provides a simple token estimator and a compactor that trims
//! older messages while preserving the most recent ones.
//!
//! The compaction is a *simplification* — it does not invoke the LLM to
//! generate natural-language summaries. Instead it preserves the most recent
//! messages (by `keep_recent_ratio`) and drops the rest, producing a summary
//! placeholder. Future iterations may replace the placeholder with a real
//! LLM-generated summary.

use crate::ai::ChatMessageRef;

/// Average characters per token (conservative estimate).
pub const CHARS_PER_TOKEN_ESTIMATE: usize = 3;

/// Approximate token overhead per message (system prompt tokens, role prefix).
pub const TOKEN_OVERHEAD: usize = 10;

/// Default context window size for GPT-4o and similar models.
pub const DEFAULT_CONTEXT_WINDOW: usize = 128_000;

/// Default reserve buffer (roughly 8% of context window).
pub const DEFAULT_RESERVE: usize = 10_240;

/// Default ratio of recent messages to keep after compression.
pub const DEFAULT_KEEP_RECENT_RATIO: f32 = 0.2;

/// Compaction configuration parameters.
///
/// Tune these to match the context window of the model you are using.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Maximum token count for the context window.
    pub context_window_tokens: usize,
    /// Reserve token budget (tokens kept free for the response).
    pub reserve_tokens: usize,
    /// Ratio of messages to keep (e.g. 0.2 keeps the most recent 20%).
    pub keep_recent_ratio: f32,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            context_window_tokens: DEFAULT_CONTEXT_WINDOW,
            reserve_tokens: DEFAULT_RESERVE,
            keep_recent_ratio: DEFAULT_KEEP_RECENT_RATIO,
        }
    }
}

/// Estimate the number of tokens in a text string.
///
/// Uses a simple character-count / 3 heuristic with a per-message overhead.
/// This is intentionally conservative — it over-estimates rather than
/// under-estimates to avoid accidental overflow.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / CHARS_PER_TOKEN_ESTIMATE + TOKEN_OVERHEAD
}

/// A message reference for compaction calculations.
///
/// This is a lightweight view used by the compactor — it only needs the
/// role and content, not session metadata. Fields own their data so
/// `MessageRef` can be stored independently of the original message list.
#[derive(Debug, Clone)]
pub struct MessageRef {
    pub role: String,
    pub content: String,
}

impl<'a> From<&'a ChatMessageRef> for MessageRef {
    fn from(msg: &'a ChatMessageRef) -> Self {
        Self {
            role: msg.role.clone(),
            content: msg.content.clone(),
        }
    }
}

/// Compacted message collection returned by `Compactor::compress_messages`.
#[derive(Debug, Clone)]
pub struct CompactedMessages {
    /// Number of messages before compression.
    pub original_count: usize,
    /// Number of messages that were compacted away.
    pub compressed_count: usize,
    /// Number of messages retained after compression.
    pub kept_count: usize,
    /// Token estimate of the original message list.
    pub before_tokens: usize,
    /// Token estimate of the retained messages (excluding the summary placeholder).
    pub after_tokens: usize,
    /// Messages that were dropped (for generating a summary placeholder).
    pub dropped_messages: Vec<MessageRef>,
    /// Messages retained after compression (most recent).
    pub kept_messages: Vec<MessageRef>,
}

/// Context compactor — trims long message histories to fit within
/// an LLM's context window.
///
/// The compactor does not invoke the LLM for summarization. Instead it
/// preserves the most recent messages and produces a summary placeholder
/// that callers can optionally send to the LLM for natural-language
/// summarization.
pub struct Compactor {
    config: CompactionConfig,
}

impl Compactor {
    /// Create a new compactor with the given configuration.
    pub fn new(config: CompactionConfig) -> Self {
        Self { config }
    }

    /// Create a new compactor with default configuration.
    pub fn default_compactor() -> Self {
        Self::new(CompactionConfig::default())
    }

    /// Estimate the total token count for a list of message references.
    ///
    /// Each message's tokens are estimated from its text content plus a
    /// fixed overhead per message. Role prefixes (e.g. "user:", "assistant:")
    /// are also counted.
    pub fn estimate_messages_tokens(&self, messages: &[MessageRef]) -> usize {
        messages
            .iter()
            .map(|m| {
                // Content tokens
                let content_tokens = estimate_tokens(&m.content);
                // Role prefix overhead (~5 tokens for "assistant: " or similar)
                let role_tokens = estimate_tokens(&m.role);
                content_tokens + role_tokens
            })
            .sum()
    }

    /// Check whether the message list exceeds the safe token threshold.
    ///
    /// Returns `true` if total tokens > (`context_window_tokens` - `reserve_tokens`).
    pub fn should_compress(&self, messages: &[MessageRef]) -> bool {
        let threshold = self.config.context_window_tokens.saturating_sub(self.config.reserve_tokens);
        let total = self.estimate_messages_tokens(messages);
        total > threshold
    }

    /// Compress a message list by keeping only the most recent messages.
    ///
    /// 1. Estimates token count of the full list.
    /// 2. Computes how many messages to keep: `max(1, messages.len() * keep_recent_ratio)`.
    /// 3. Returns the kept messages (most recent) and dropped messages (oldest).
    ///
    /// At least one message is always kept to avoid an empty context.
    pub fn compress_messages(&self, messages: &[MessageRef]) -> CompactedMessages {
        let original_count = messages.len();
        let before_tokens = self.estimate_messages_tokens(messages);

        if original_count == 0 {
            return CompactedMessages {
                original_count: 0,
                compressed_count: 0,
                kept_count: 0,
                before_tokens: 0,
                after_tokens: 0,
                dropped_messages: Vec::new(),
                kept_messages: Vec::new(),
            };
        }

        // Calculate how many messages to keep (at least 1)
        let keep_recent_count = ((original_count as f32 * self.config.keep_recent_ratio)
            .ceil() as usize)
            .max(1);

        let kept_count = keep_recent_count.min(original_count);
        let compressed_count = original_count - kept_count;

        // Split: dropped = oldest messages, kept = most recent.
        // Since MessageRef already owns its strings, we just clone the entries.
        let (dropped, kept) = if compressed_count > 0 {
            let split_at = original_count - kept_count;
            (messages[..split_at].to_vec(), messages[split_at..].to_vec())
        } else {
            (Vec::new(), messages.to_vec())
        };

        let after_tokens = self.estimate_messages_tokens(&kept);

        CompactedMessages {
            original_count,
            compressed_count,
            kept_count,
            before_tokens,
            after_tokens,
            dropped_messages: dropped,
            kept_messages: kept,
        }
    }

    /// Generate a system-message summary placeholder for dropped messages.
    ///
    /// This is a simple text template — callers may pass it to the LLM
    /// to produce a real natural-language summary, or include it directly
    /// as a compact context indicator.
    pub fn generate_summary_placeholder(dropped_count: usize, before_tokens: usize) -> String {
        format!(
            "[COMPACTED: 之前的 {} 条消息（约 {} tokens）已被压缩。请基于最近的对话继续。]",
            dropped_count, before_tokens
        )
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> ChatMessageRef {
        ChatMessageRef {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    fn msg_ref(role: &str, content: &str) -> MessageRef {
        MessageRef {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    // ── estimate_tokens tests ─────────────────────────────────────────

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), TOKEN_OVERHEAD);
    }

    #[test]
    fn test_estimate_tokens_short_text() {
        // "hello" = 5 chars / 3 ≈ 1 + 10 overhead = 11
        let tokens = estimate_tokens("hello");
        assert!(tokens >= TOKEN_OVERHEAD);
        assert!(tokens < TOKEN_OVERHEAD + 5);
    }

    #[test]
    fn test_estimate_tokens_long_text() {
        // 300 chars ≈ 100 tokens + 10 overhead = ~110
        let text = "a".repeat(300);
        let tokens = estimate_tokens(&text);
        assert_eq!(tokens, 300 / 3 + TOKEN_OVERHEAD);
    }

    #[test]
    fn test_estimate_tokens_proportional() {
        let short = "hello world";
        let long = "hello world ".repeat(10); // 120 chars
        assert!(estimate_tokens(&long) > estimate_tokens(short));
    }

    // ── Compactor config tests ────────────────────────────────────────

    #[test]
    fn test_default_config() {
        let config = CompactionConfig::default();
        assert_eq!(config.context_window_tokens, 128_000);
        assert_eq!(config.reserve_tokens, 10_240);
        assert!((config.keep_recent_ratio - 0.2).abs() < f32::EPSILON);
    }

    // ── estimate_messages_tokens tests ────────────────────────────────

    #[test]
    fn test_estimate_messages_tokens_single() {
        let compactor = Compactor::default_compactor();
        let messages = vec![msg_ref("user", "hello")];
        let tokens = compactor.estimate_messages_tokens(&messages);
        // "user" = 4/3+10=11, "hello" = 5/3+10=11, total = 22
        assert!(tokens > 0);
    }

    #[test]
    fn test_estimate_messages_tokens_multiple() {
        let compactor = Compactor::default_compactor();
        let messages = vec![
            msg_ref("user", "hello"),
            msg_ref("assistant", "world"),
        ];
        let single = compactor.estimate_messages_tokens(&vec![msg_ref("user", "hello")]);
        let multi = compactor.estimate_messages_tokens(&messages);
        // Multi should be more than single
        assert!(multi > single);
    }

    #[test]
    fn test_estimate_messages_tokens_empty() {
        let compactor = Compactor::default_compactor();
        assert_eq!(compactor.estimate_messages_tokens(&[]), 0);
    }

    // ── should_compress tests ─────────────────────────────────────────

    #[test]
    fn test_should_compress_short_messages() {
        let config = CompactionConfig {
            context_window_tokens: 1000,
            reserve_tokens: 100,
            keep_recent_ratio: 0.2,
        };
        let compactor = Compactor::new(config);
        let messages = vec![msg_ref("user", "hi")];
        // 1 short message should NOT trigger compression
        assert!(!compactor.should_compress(&messages));
    }

    #[test]
    fn test_should_compress_over_threshold() {
        let config = CompactionConfig {
            context_window_tokens: 100,
            reserve_tokens: 10,
            keep_recent_ratio: 0.2,
        };
        let compactor = Compactor::new(config);
        // Build messages that exceed threshold (90 tokens)
        let messages: Vec<MessageRef> = (0..20)
            .map(|i| {
                msg_ref(
                    "user",
                    &format!("message number {} with some content", i),
                )
            })
            .collect();
        assert!(compactor.should_compress(&messages));
    }

    #[test]
    fn test_should_compress_empty_messages() {
        let config = CompactionConfig {
            context_window_tokens: 100,
            reserve_tokens: 10,
            keep_recent_ratio: 0.2,
        };
        let compactor = Compactor::new(config);
        assert!(!compactor.should_compress(&[]));
    }

    // ── compress_messages tests ───────────────────────────────────────

    #[test]
    fn test_compress_messages_basic() {
        let config = CompactionConfig {
            context_window_tokens: 100,
            reserve_tokens: 10,
            keep_recent_ratio: 0.2,
        };
        let compactor = Compactor::new(config);

        let messages: Vec<MessageRef> = (0..10)
            .map(|i| msg_ref("user", &format!("msg {}", i)))
            .collect();

        let compacted = compactor.compress_messages(&messages);
        assert_eq!(compacted.original_count, 10);
        assert!(compacted.compressed_count > 0);
        assert!(compacted.kept_count > 0);
        assert_eq!(
            compacted.original_count,
            compacted.compressed_count + compacted.kept_count
        );
        // Kept messages should be the most recent (last in the list)
        let kept_texts: Vec<String> = compacted.kept_messages.iter().map(|m| m.content.clone()).collect();
        assert!(kept_texts.contains(&"msg 9".to_string()));
        // Dropped messages should be the oldest
        let dropped_texts: Vec<String> = compacted.dropped_messages.iter().map(|m| m.content.clone()).collect();
        assert!(dropped_texts.contains(&"msg 0".to_string()));
    }

    #[test]
    fn test_compress_messages_keeps_at_least_one() {
        let config = CompactionConfig {
            context_window_tokens: 50,
            reserve_tokens: 10,
            keep_recent_ratio: 0.1,
        };
        let compactor = Compactor::new(config);
        let messages = vec![msg_ref("user", "only message")];

        let compacted = compactor.compress_messages(&messages);
        assert_eq!(compacted.kept_count, 1);
        assert_eq!(compacted.compressed_count, 0);
    }

    #[test]
    fn test_compress_messages_empty() {
        let compactor = Compactor::default_compactor();
        let compacted = compactor.compress_messages(&[]);
        assert_eq!(compacted.original_count, 0);
        assert_eq!(compacted.kept_count, 0);
        assert_eq!(compacted.compressed_count, 0);
    }

    #[test]
    fn test_compress_messages_token_tracking() {
        let config = CompactionConfig {
            context_window_tokens: 100,
            reserve_tokens: 10,
            keep_recent_ratio: 0.3,
        };
        let compactor = Compactor::new(config);

        let messages: Vec<MessageRef> = (0..8)
            .map(|i| msg_ref("user", &format!("content line {}", i)))
            .collect();

        let compacted = compactor.compress_messages(&messages);
        assert!(compacted.before_tokens > compacted.after_tokens);
    }

    #[test]
    fn test_compress_with_chat_message_ref() {
        let config = CompactionConfig {
            context_window_tokens: 100,
            reserve_tokens: 10,
            keep_recent_ratio: 0.25,
        };
        let compactor = Compactor::new(config);

        let chat_msgs: Vec<ChatMessageRef> = (0..12)
            .map(|i| msg("user", &format!("message {}", i)))
            .collect();

        let refs: Vec<MessageRef> = chat_msgs.iter().map(MessageRef::from).collect();
        let compacted = compactor.compress_messages(&refs);

        assert_eq!(compacted.original_count, 12);
        assert!(compacted.kept_count >= 1);
    }

    // ── generate_summary_placeholder tests ────────────────────────────

    #[test]
    fn test_generate_summary_placeholder() {
        let placeholder = Compactor::generate_summary_placeholder(15, 50000);
        assert!(placeholder.contains("15"));
        assert!(placeholder.contains("50000"));
        assert!(placeholder.contains("COMPACTED"));
    }

    #[test]
    fn test_generate_summary_placeholder_zero() {
        let placeholder = Compactor::generate_summary_placeholder(0, 0);
        assert!(placeholder.contains("0"));
    }

    // ── Integration-style: should_compress → compress_messages ────────

    #[test]
    fn test_compress_pipeline() {
        let config = CompactionConfig {
            context_window_tokens: 200,
            reserve_tokens: 20,
            keep_recent_ratio: 0.2,
        };
        let compactor = Compactor::new(config);

        let messages: Vec<MessageRef> = (0..15)
            .map(|i| msg_ref("user", &format!("message number {} with extra content for token estimation", i)))
            .collect();

        // First verify compression is needed
        assert!(compactor.should_compress(&messages));

        // Then compress
        let compacted = compactor.compress_messages(&messages);
        assert!(compacted.compressed_count > 0);
        assert!(compacted.kept_count > 0);
        assert!(compacted.before_tokens > compacted.after_tokens);

        // After compression, it should no longer need compressing
        // (unless the kept messages are still too long, which is possible)
        let _re_check = compactor.should_compress(&compacted.kept_messages);
        // We don't assert the result here because the kept messages
        // might still be over the threshold depending on content length.
    }
}
