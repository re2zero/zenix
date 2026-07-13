//! System prompt template management.
//!
//! Loads prompt templates from `~/.config/zenix/prompts/*.md` at startup into
//! an in-memory cache. Users can edit these files to customize AI behavior
//! without recompiling. Changes take effect on restart.
//!
//! The canonical orchestrator prompt lives in `assets/prompts/orchestrator.md`
//! and is embedded into the binary at compile time via `include_str!`. This
//! embedded copy serves as both the fallback (when the disk file is missing or
//! corrupt) and the content seeded to `~/.config/zenix/prompts/` on first run.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use crate::platform;

/// The orchestrator prompt template, embedded from `assets/prompts/orchestrator.md`
/// at compile time. Serves as both the runtime fallback and the content seeded
/// to disk on first run.
const EMBEDDED_ORCHESTRATOR: &str = include_str!("../../assets/prompts/orchestrator.md");

// ── Cache ─────────────────────────────────────────────────────────────────

fn cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: LazyLock<Mutex<HashMap<String, String>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    &CACHE
}

// ── Public API ────────────────────────────────────────────────────────────

/// Initialize the prompt system at startup.
///
/// Seeds built-in prompt files to disk (skips existing files), then loads
/// all `.md` files into the in-memory cache. Logs warnings on I/O errors
/// but never panics or blocks startup.
pub fn init() {
    if let Err(e) = seed_prompts() {
        tracing::warn!(target: "prompts", error = %e, "failed to seed prompt templates");
    }
    load_all();
    let count = cache().lock().unwrap().len();
    tracing::info!(target: "prompts", count, "prompt templates loaded");
}

/// Get a cached prompt template by name.
///
/// Returns the cached file content if available, otherwise falls back to a
/// compile-time constant. The caller always gets a valid prompt string and
/// does not need to handle a fallback path.
pub fn get_template(name: &str) -> String {
    cache()
        .lock()
        .unwrap()
        .get(name)
        .cloned()
        .unwrap_or_else(|| match name {
            "orchestrator" => EMBEDDED_ORCHESTRATOR.to_string(),
            _ => EMBEDDED_ORCHESTRATOR.to_string(),
        })
}

// ── Seed ──────────────────────────────────────────────────────────────────

/// Seed the orchestrator prompt file to disk if it does not exist.
fn seed_prompts() -> std::io::Result<()> {
    let dir = platform::zenix_config_dir().join("prompts");
    std::fs::create_dir_all(&dir)?;
    seed_one(&dir, "orchestrator", EMBEDDED_ORCHESTRATOR)
}

/// Write a single prompt file if it does not exist.
fn seed_one(dir: &Path, name: &str, content: &str) -> std::io::Result<()> {
    let path = dir.join(format!("{name}.md"));
    if !path.exists() {
        std::fs::write(&path, content)?;
    }
    Ok(())
}

// ── Load ──────────────────────────────────────────────────────────────────

/// Read all `.md` files from the prompts directory into the cache.
///
/// Silently skips files that can't be read (logs a warning) so that one
/// corrupt file doesn't prevent loading the rest.
fn load_all() {
    let dir = platform::zenix_config_dir().join("prompts");
    let read_dir = match std::fs::read_dir(&dir) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(target: "prompts", error = %e, "prompts directory not readable");
            return;
        }
    };

    let mut map = HashMap::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md")
            && let Some(name) = path.file_stem().and_then(|s| s.to_str())
        {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    map.insert(name.to_string(), content);
                }
                Err(e) => {
                    tracing::warn!(target: "prompts", ?path, error = %e, "failed to read prompt file");
                }
            }
        }
    }

    *cache().lock().unwrap() = map;
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_template_orchestrator_has_content() {
        let t = get_template("orchestrator");
        assert!(!t.is_empty(), "orchestrator template should not be empty");
        assert!(t.contains("{lang_instruction}"), "template should contain lang placeholder");
    }

    #[test]
    fn test_get_template_unknown_falls_back() {
        let t = get_template("nonexistent");
        assert!(!t.is_empty(), "unknown template should fall back to orchestrator");
    }

    #[test]
    fn test_embedded_contains_tools() {
        assert!(EMBEDDED_ORCHESTRATOR.contains("spawn_worker"));
        assert!(EMBEDDED_ORCHESTRATOR.contains("send_text"));
        let tools = ["spawn_worker", "send_text", "read_output", "wait_for_output",
                     "list_workers", "close_worker"];
        for tool in &tools {
            assert!(EMBEDDED_ORCHESTRATOR.contains(tool), "embedded missing tool: {tool}");
        }
    }

    #[test]
    fn test_embedded_has_comment_header() {
        assert!(EMBEDDED_ORCHESTRATOR.starts_with("#"),
            "embedded orchestrator.md should start with comment header");
        assert!(EMBEDDED_ORCHESTRATOR.contains("编辑此文件"),
            "embedded orchestrator.md should mention editability");
    }

    #[test]
    fn test_embedded_has_lang_placeholder() {
        assert!(EMBEDDED_ORCHESTRATOR.contains("{lang_instruction}"),
            "embedded should contain lang placeholder");
    }

    #[test]
    fn test_seed_one_creates_file() {
        let tmp = std::env::temp_dir().join("zenix_test_prompts");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let result = seed_one(&tmp, "test-prompt", "test content");
        assert!(result.is_ok());

        let path = tmp.join("test-prompt.md");
        assert!(path.exists(), "seed_one should create the file");

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "test content");

        // Second call should not fail or overwrite
        let result2 = seed_one(&tmp, "test-prompt", "overwritten content");
        assert!(result2.is_ok());
        let content2 = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content2, "test content", "seed_one should not overwrite existing files");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_get_template_after_init_uses_cache() {
        // Simulate init by loading a known dir
        let tmp = std::env::temp_dir().join("zenix_test_prompts_cache");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("custom.md"), "custom prompt {lang_instruction}").unwrap();

        // Point the cache at this dir (override via env trick won't work here,
        // but we can manually insert)
        {
            let mut c = cache().lock().unwrap();
            c.insert("custom".to_string(), "custom prompt {lang_instruction}".to_string());
        }

        let t = get_template("custom");
        assert_eq!(t, "custom prompt {lang_instruction}");

        // Cleanup
        cache().lock().unwrap().clear();
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
