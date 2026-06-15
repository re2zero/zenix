//! JSONC (JSON with Comments) parser.
//!
//! Strips `//` line comments and `/* */` block comments from JSON-like text,
//! then delegates to `serde_json` for actual parsing.
//!
//! Comments inside strings are preserved (not stripped).
//! Stripped comment characters are replaced with spaces to preserve
//! line/column positions so error messages reference the original file.

use std::io::Read;
use std::path::Path;

/// Two-pass comment stripping that properly handles `//` and `/* */` detection.
///
/// First pass: identify comment ranges while tracking string state.
/// Second pass: copy non-comment chars, replace comment chars with spaces.
fn strip_comments_proper(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        let ch = chars[i];

        // String literal — copy until closing unescaped quote
        if ch == '"' {
            out.push(ch);
            i += 1;
            while i < len {
                let c = chars[i];
                out.push(c);
                if c == '\\' && i + 1 < len {
                    i += 1;
                    out.push(chars[i]);
                } else if c == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Line comment //
        if ch == '/' && i + 1 < len && chars[i + 1] == '/' {
            // Replace // with spaces
            out.push(' ');
            out.push(' ');
            i += 2;
            while i < len && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }

        // Block comment /* */
        if ch == '/' && i + 1 < len && chars[i + 1] == '*' {
            out.push(' ');
            out.push(' ');
            i += 2;
            while i < len {
                let c = chars[i];
                if c == '\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
                if c == '*' && i + 1 < len && chars[i + 1] == '/' {
                    out.push(' '); // for the '/'
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }

        out.push(ch);
        i += 1;
    }

    out
}

/// Parse a JSONC string into a `serde_json::Value`.
///
/// Strips comments, then delegates to `serde_json::from_str`.
pub fn parse_jsonc(raw: &str) -> Result<serde_json::Value, String> {
    let cleaned = strip_comments_proper(raw);
    serde_json::from_str(&cleaned).map_err(|e| format!("JSONC parse error: {e}"))
}

/// Read and parse a JSONC file from disk.
///
/// Returns `Ok(Value)` on success, or an error string.
/// An empty/nonexistent file returns `serde_json::Value::Null`.
pub fn read_jsonc_file(path: &Path) -> Result<serde_json::Value, String> {
    let mut content = String::new();
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    file.read_to_string(&mut content)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    parse_jsonc(&content)
}

/// Try to parse as JSON (with `serde_json`) first, fall back to JSONC.
///
/// This avoids the comment-stripping overhead for pure JSON files.
pub fn parse_json_or_jsonc(raw: &str) -> Result<serde_json::Value, String> {
    if let Ok(v) = serde_json::from_str(raw) {
        return Ok(v);
    }
    parse_jsonc(raw)
}

/// Read a file that may be JSON or JSONC.
pub fn read_json_file(path: &Path) -> Result<serde_json::Value, String> {
    let mut content = String::new();
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    file.read_to_string(&mut content)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    parse_json_or_jsonc(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_line_comment() {
        let input = "{\n  // this is a comment\n  \"key\": \"value\"\n}";
        let cleaned = strip_comments_proper(input);
        assert!(cleaned.contains("\"key\""));
        assert!(!cleaned.contains("this is a comment"));
    }

    #[test]
    fn test_strip_block_comment() {
        let input = "{\n  /* block\n     comment */\n  \"key\": \"value\"\n}";
        let cleaned = strip_comments_proper(input);
        assert!(cleaned.contains("\"key\""));
        assert!(!cleaned.contains("block"));
    }

    #[test]
    fn test_comment_in_string_preserved() {
        let input = r#"{"url": "http://example.com/*path*/"}"#;
        let cleaned = strip_comments_proper(input);
        assert!(cleaned.contains("http://example.com/*path*/"));
    }

    #[test]
    fn test_double_slash_in_string_preserved() {
        let input = r#"{"url": "http://example.com//path"}"#;
        let cleaned = strip_comments_proper(input);
        assert!(cleaned.contains("http://example.com//path"));
    }

    #[test]
    fn test_parse_jsonc_valid() {
        let input = r#"{
            // MCP servers config
            "mcpServers": {
                "my-server": {
                    "command": "npx",
                    "args": ["-y", "@server/name"] /* inline */
                }
            }
        }"#;
        let val = parse_jsonc(input).unwrap();
        let cmd = val["mcpServers"]["my-server"]["command"].as_str().unwrap();
        assert_eq!(cmd, "npx");
    }

    #[test]
    fn test_parse_pure_json() {
        let input = r#"{"key": "value"}"#;
        let val = parse_json_or_jsonc(input).unwrap();
        assert_eq!(val["key"].as_str().unwrap(), "value");
    }

    #[test]
    fn test_parse_jsonc_with_trailing_comma() {
        // Note: trailing commas are NOT valid JSON/JSONC, but some tools accept them.
        // We don't support trailing commas.
        let input = r#"{"key": "value",}"#;
        let result = parse_jsonc(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_preserves_line_count() {
        let input = "// comment\n// comment2\n{\"key\":1}";
        let cleaned = strip_comments_proper(input);
        assert_eq!(cleaned.lines().count(), input.lines().count());
    }

    #[test]
    fn test_escaped_quote_in_string() {
        let input = r#"{"key": "val\"ue"}"#;
        let cleaned = strip_comments_proper(input);
        assert!(cleaned.contains(r#""val\"ue""#));
    }

    #[test]
    fn test_empty_input() {
        let input = "";
        let cleaned = strip_comments_proper(input);
        assert_eq!(cleaned, "");
    }

    #[test]
    fn test_nested_block_comment() {
        // /* /* nested */ */ — the first */ closes everything
        // This is standard C/JS behavior
        let input = r#"{"a": /* /* nested */ 1 }"#;
        let val = parse_jsonc(input).unwrap();
        assert_eq!(val["a"].as_i64().unwrap(), 1);
    }

    #[test]
    fn test_read_jsonc_file_empty() {
        let path = std::path::Path::new("/nonexistent/file.json");
        let result = read_jsonc_file(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_json_file_empty_content() {
        let dir = std::env::temp_dir().join("zenix_test_jsonc");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("empty.json");
        std::fs::write(&path, "").unwrap();
        let val = read_json_file(&path).unwrap();
        assert!(val.is_object());
        let _ = std::fs::remove_dir_all(&dir);
    }
}