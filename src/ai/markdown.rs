//! Lightweight Markdown renderer for the AI chat panel.
//!
//! No external dependencies — custom parser supporting:
//!   - `**bold**` → bold text
//!   - `*italic*` → italic text
//!   - `` `inline code` `` → monospace code
//!   - ``` ```code block``` ``` → highlighted code block
//!   - `- item`, `* item`, `1. item` → list items
//!
//! Output is a `Vec<AnyElement>` (GPUI segments) suitable for rendering
//! inside a chat bubble.

use gpui::{
    AnyElement, FontWeight, IntoElement, ParentElement as _, Styled as _,
    div, px,
};
use gpui_component::{Theme, h_flex, v_flex};

// ── Segment types ───────────────────────────────────────────────────────

/// A parsed markdown segment.
#[derive(Debug, Clone)]
pub enum MdSegment {
    Plain(String),
    Bold(String),
    Italic(String),
    InlineCode(String),
    CodeBlock(String),
    ListItem(String),
}

/// Render a markdown string into a vector of GPUI `AnyElement` segments.
pub fn render_markdown(text: &str, theme: &Theme) -> Vec<AnyElement> {
    let segments = parse_markdown(text);
    render_segments(&segments, theme)
}

// ── Parser ──────────────────────────────────────────────────────────────

/// Parse a markdown string into a flat list of segments.
fn parse_markdown(text: &str) -> Vec<MdSegment> {
    let mut segments: Vec<MdSegment> = Vec::new();
    let mut current_plain = String::new();
    let mut in_code_block = false;
    let mut code_block_content = String::new();
    let mut code_block_lang = String::new();

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if !in_code_block && line.starts_with("```") {
            // Start of code block
            if !current_plain.is_empty() {
                segments.push(MdSegment::Plain(current_plain.clone()));
                current_plain.clear();
            }
            in_code_block = true;
            code_block_lang = line[3..].trim().to_string();
            code_block_content.clear();
            i += 1;
            continue;
        }

        if in_code_block {
            if line.starts_with("```") {
                // End of code block
                in_code_block = false;
                segments.push(MdSegment::CodeBlock(code_block_content.clone()));
                code_block_content.clear();
                code_block_lang.clear();
                i += 1;
                continue;
            }
            if !code_block_content.is_empty() {
                code_block_content.push('\n');
            }
            code_block_content.push_str(line);
            i += 1;
            continue;
        }

        // Check for list item
        if is_list_item(line) {
            if !current_plain.is_empty() {
                segments.push(MdSegment::Plain(current_plain.clone()));
                current_plain.clear();
            }
            let content = strip_list_marker(line);
            segments.push(MdSegment::ListItem(content));
            i += 1;
            continue;
        }

        // Parse inline formatting on this line
        let inline_segments = parse_inline(line);
        for seg in inline_segments {
            match seg {
                MdSegment::Plain(s) if !s.is_empty() => {
                    current_plain.push_str(&s);
                }
                _ => {
                    // Flush accumulated plain text before non-plain segment
                    if !current_plain.is_empty() {
                        if !segments.is_empty()
                            && !matches!(segments.last(), Some(MdSegment::ListItem(_)))
                        {
                            segments.push(MdSegment::Plain(String::new())); // newline spacer
                        }
                        segments.push(MdSegment::Plain(current_plain.clone()));
                        current_plain.clear();
                    }
                    segments.push(seg);
                }
            }
        }

        // Flush remaining plain text at end of line
        if !current_plain.is_empty() {
            if !segments.is_empty()
                && !matches!(segments.last(), Some(MdSegment::ListItem(_)))
            {
                segments.push(MdSegment::Plain(String::new())); // newline spacer
            }
            segments.push(MdSegment::Plain(current_plain.clone()));
            current_plain.clear();
        }

        i += 1;
    }

    // Flush remaining
    if in_code_block {
        segments.push(MdSegment::CodeBlock(code_block_content));
    }
    if !current_plain.is_empty() {
        segments.push(MdSegment::Plain(current_plain));
    }

    segments
}

/// Check if a line is a list item (starts with `-`, `*`, or `N. `).
fn is_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return true;
    }
    if let Some(idx) = trimmed.find(". ") {
        if idx > 0 && trimmed[..idx].chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Strip the list marker from a line (e.g. "- item" → "item").
fn strip_list_marker(line: &str) -> String {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return rest.to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return rest.to_string();
    }
    if let Some(idx) = trimmed.find(". ") {
        return trimmed[idx + 2..].to_string();
    }
    trimmed.to_string()
}

/// Parse inline formatting within a single line.
fn parse_inline(line: &str) -> Vec<MdSegment> {
    let mut segments: Vec<MdSegment> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut current_plain = String::new();

    while i < len {
        if chars[i] == '`' {
            // Inline code: ``...`` or `...`
            if i + 1 < len && chars[i + 1] == '`' {
                // Double backtick
                if let Some(end) = find_closing_double_backtick(&chars, i + 2) {
                    if !current_plain.is_empty() {
                        segments.push(MdSegment::Plain(current_plain.clone()));
                        current_plain.clear();
                    }
                    let code = extract_between(&chars, i + 2, end);
                    segments.push(MdSegment::InlineCode(code));
                    i = end + 1;
                    continue;
                }
            } else if let Some(end) = find_closing_single_backtick(&chars, i + 1) {
                if !current_plain.is_empty() {
                    segments.push(MdSegment::Plain(current_plain.clone()));
                    current_plain.clear();
                }
                let code = extract_between(&chars, i + 1, end);
                segments.push(MdSegment::InlineCode(code));
                i = end + 1;
                continue;
            } else {
                // No closing backtick, keep the backtick as plain
                current_plain.push('`');
                i += 1;
                continue;
            }
        }

        if chars[i] == '*' && i + 1 < len {
            // Bold: **text**
            if chars[i + 1] == '*' {
                if let Some(end) = find_closing_double_star(&chars, i + 2) {
                    if !current_plain.is_empty() {
                        segments.push(MdSegment::Plain(current_plain.clone()));
                        current_plain.clear();
                    }
                    let bold_text = extract_between(&chars, i + 2, end);
                    segments.push(MdSegment::Bold(bold_text));
                    i = end + 2;
                    continue;
                }
            }
            // Italic: *text*
            else {
                if let Some(end) = find_closing_single_star(&chars, i + 1) {
                    if !current_plain.is_empty() {
                        segments.push(MdSegment::Plain(current_plain.clone()));
                        current_plain.clear();
                    }
                    let italic_text = extract_between(&chars, i + 1, end);
                    segments.push(MdSegment::Italic(italic_text));
                    i = end + 1;
                    continue;
                }
            }
        }

        current_plain.push(chars[i]);
        i += 1;
    }

    if !current_plain.is_empty() {
        segments.push(MdSegment::Plain(current_plain));
    }

    segments
}

fn find_closing_single_backtick(chars: &[char], start: usize) -> Option<usize> {
    chars[start..]
        .iter()
        .position(|&c| c == '`')
        .map(|p| p + start)
}

fn find_closing_single_star(chars: &[char], start: usize) -> Option<usize> {
    for (i, &c) in chars[start..].iter().enumerate() {
        if c == '*' {
            let abs_i = start + i;
            if abs_i + 1 >= chars.len() || chars[abs_i + 1] != '*' {
                return Some(abs_i);
            }
        }
    }
    None
}

fn find_closing_double_backtick(chars: &[char], start: usize) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == '`' && i + 1 < chars.len() && chars[i + 1] == '`' {
            return Some(i);
        }
    }
    None
}

fn find_closing_double_star(chars: &[char], start: usize) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            return Some(i);
        }
    }
    None
}

fn extract_between(chars: &[char], start: usize, end: usize) -> String {
    chars[start..end].iter().collect()
}

// ── Renderer ────────────────────────────────────────────────────────────

/// Render a list of segments into GPUI `AnyElement` objects.
fn render_segments(segments: &[MdSegment], theme: &Theme) -> Vec<AnyElement> {
    let fg = theme.foreground;
    let mu = theme.muted_foreground;
    let list_prefix = gpui::rgba(0xCCCCCC88);

    segments
        .iter()
        .map(|seg| match seg {
            MdSegment::Plain(s) => {
                // Skip empty spacers
                if s.is_empty() {
                    return div().h(px(4.)).into_any_element();
                }
                txt(px(13.))
                    .text_color(fg)
                    .line_height(px(18.))
                    .child(s.clone())
                    .into_any_element()
            }
            MdSegment::Bold(s) => {
                txt(px(13.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(fg)
                    .line_height(px(18.))
                    .child(s.clone())
                    .into_any_element()
            }
            MdSegment::Italic(s) => {
                let italic = s.clone();
                txt(px(13.))
                    .italic()
                    .text_color(fg)
                    .line_height(px(18.))
                    .child(italic)
                    .into_any_element()
            }
            MdSegment::InlineCode(s) => {
                let code_text = s.clone();
                v_flex()
                    .gap(px(2.))
                    .child(
                        div()
                            .rounded_sm()
                            .px(px(4.))
                            .py(px(1.))
                            .bg(gpui::rgba(0xFFFFFF06))
                            .child(
                                txt(px(12.))
                                    .font_family("Lilex")
                                    .text_color(mu)
                                    .child(code_text),
                            ),
                    )
                    .into_any_element()
            }
            MdSegment::CodeBlock(s) => {
                let code = s.clone();
                v_flex()
                    .gap(px(2.))
                    .child(
                        div()
                            .rounded_md()
                            .bg(gpui::rgba(0xFFFFFF04))
                            .border_1()
                            .border_color(gpui::rgba(0xFFFFFF10))
                            .p(px(10.))
                            .child(
                                txt(px(12.))
                                    .font_family("Lilex")
                                    .line_height(px(16.))
                                    .text_color(mu)
                                    .child(code),
                            ),
                    )
                    .into_any_element()
            }
            MdSegment::ListItem(s) => {
                let item_text = s.clone();
                h_flex()
                    .gap(px(8.))
                    .items_start()
                    .child(
                        txt(px(13.))
                            .text_color(list_prefix)
                            .child("•".to_string()),
                    )
                    .child(
                        txt(px(13.))
                            .text_color(fg)
                            .line_height(px(18.))
                            .child(item_text),
                    )
                    .into_any_element()
            }
        })
        .collect()
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn txt(size: gpui::Pixels) -> gpui::Div {
    div().text_size(size)
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text() {
        let segments = parse_markdown("Hello world");
        assert!(!segments.is_empty());
        let plain_segments: Vec<&MdSegment> = segments.iter().filter(|s| matches!(s, MdSegment::Plain(_))).collect();
        assert!(plain_segments.iter().any(|s| matches!(s, MdSegment::Plain(s) if s == "Hello world")));
    }

    #[test]
    fn test_bold() {
        let segments = parse_markdown("**bold text**");
        assert_eq!(segments.len(), 1);
        match &segments[0] {
            MdSegment::Bold(s) => assert_eq!(s, "bold text"),
            _ => panic!("expected Bold, got {:?}", segments),
        }
    }

    #[test]
    fn test_italic() {
        let segments = parse_markdown("*italic text*");
        let italic = segments.iter().find(|s| matches!(s, MdSegment::Italic(s) if s == "italic text"));
        assert!(italic.is_some(), "should find italic, got {:?}", segments);
    }

    #[test]
    fn test_inline_code() {
        let segments = parse_markdown("Use `fn main` in Rust");
        assert!(!segments.is_empty());
        let has_code = segments.iter().any(|s| matches!(s, MdSegment::InlineCode(c) if c == "fn main"));
        assert!(has_code, "should find inline code, got {:?}", segments);
    }

    #[test]
    fn test_code_block() {
        let text = "```rust\nfn main() {}\n```";
        let segments = parse_markdown(text);
        let has_code = segments.iter().any(|s| matches!(s, MdSegment::CodeBlock(c) if c.contains("fn main")));
        assert!(has_code, "should find code block, got {:?}", segments);
    }

    #[test]
    fn test_list_item_dash() {
        let segments = parse_markdown("- item one\n- item two");
        let list_items: Vec<&MdSegment> = segments.iter().filter(|s| matches!(s, MdSegment::ListItem(_))).collect();
        assert_eq!(list_items.len(), 2);
        match &list_items[0] {
            MdSegment::ListItem(s) => assert_eq!(s, "item one"),
            _ => panic!("expected ListItem"),
        }
    }

    #[test]
    fn test_list_item_numbered() {
        let segments = parse_markdown("1. first\n2. second");
        let list_items: Vec<&MdSegment> = segments.iter().filter(|s| matches!(s, MdSegment::ListItem(_))).collect();
        assert_eq!(list_items.len(), 2);
        match &list_items[0] {
            MdSegment::ListItem(s) => assert_eq!(s, "first"),
            _ => panic!("expected ListItem"),
        }
    }

    #[test]
    fn test_list_item_star() {
        let segments = parse_markdown("* star item");
        let list_items: Vec<&MdSegment> = segments.iter().filter(|s| matches!(s, MdSegment::ListItem(_))).collect();
        assert_eq!(list_items.len(), 1);
        match &list_items[0] {
            MdSegment::ListItem(s) => assert_eq!(s, "star item"),
            _ => panic!("expected ListItem"),
        }
    }

    #[test]
    fn test_mixed_inline() {
        let text = "Use **bold** and *italic* and `code`";
        let segments = parse_markdown(text);
        assert!(segments.iter().any(|s| matches!(s, MdSegment::Bold(s) if s == "bold")));
        assert!(segments.iter().any(|s| matches!(s, MdSegment::Italic(s) if s == "italic")));
        assert!(segments.iter().any(|s| matches!(s, MdSegment::InlineCode(s) if s == "code")));
    }

    #[test]
    fn test_empty_string() {
        let segments = parse_markdown("");
        assert!(segments.is_empty() || segments.iter().all(|s| matches!(s, MdSegment::Plain(s) if s.is_empty())));
    }

    #[test]
    fn test_multiline_plain() {
        let segments = parse_markdown("line one\nline two");
        let plain_segments: Vec<&MdSegment> = segments.iter().filter(|s| matches!(s, MdSegment::Plain(s) if !s.is_empty())).collect();
        assert!(plain_segments.len() >= 2, "should have at least 2 non-empty plain segments, got {:?}", segments);
    }

    #[test]
    fn test_unmatched_backtick_kept_as_plain() {
        let segments = parse_markdown("no closing ` backtick");
        let has_plain = segments.iter().any(|s| matches!(s, MdSegment::Plain(s) if s.contains('`')));
        assert!(has_plain, "should keep unmatched backtick as plain text, got {:?}", segments);
    }

    #[test]
    fn test_unmatched_star_kept_as_plain() {
        let segments = parse_markdown("no closing * star");
        let has_plain = segments.iter().any(|s| matches!(s, MdSegment::Plain(s) if s.contains('*')));
        assert!(has_plain, "should keep unmatched star as plain text, got {:?}", segments);
    }

    #[test]
    fn test_double_backtick_inline_code() {
        let segments = parse_markdown("``fn main() {}``");
        let has_code = segments.iter().any(|s| matches!(s, MdSegment::InlineCode(s) if s == "fn main() {}"));
        assert!(has_code, "should parse double-backtick code, got {:?}", segments);
    }
}
