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

use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, FontWeight, IntoElement, ParentElement as _, Styled as _, div, px};
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
    Heading(u8, String),
    Blockquote(String),
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Link(String, String),
    Task(bool, String),
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
            // Task item: "- [ ]" or "- [x]" / "* [ ]" etc.
            if let Some((checked, text)) = parse_task_item(line) {
                if !current_plain.is_empty() {
                    segments.push(MdSegment::Plain(current_plain.clone()));
                    current_plain.clear();
                }
                segments.push(MdSegment::Task(checked, text));
                i += 1;
                continue;
            }
            if !current_plain.is_empty() {
                segments.push(MdSegment::Plain(current_plain.clone()));
                current_plain.clear();
            }
            let content = strip_list_marker(line);
            segments.push(MdSegment::ListItem(content));
            i += 1;
            continue;
        }

        // Heading: "# " .. "###### "
        if let Some((level, text)) = parse_heading(line) {
            if !current_plain.is_empty() {
                segments.push(MdSegment::Plain(current_plain.clone()));
                current_plain.clear();
            }
            segments.push(MdSegment::Heading(level, text));
            i += 1;
            continue;
        }

        // Blockquote: "> ..."
        if let Some(text) = parse_blockquote(line) {
            if !current_plain.is_empty() {
                segments.push(MdSegment::Plain(current_plain.clone()));
                current_plain.clear();
            }
            segments.push(MdSegment::Blockquote(text));
            i += 1;
            continue;
        }

        // Table: current line has "|" and next line is a separator row.
        if is_table_start(&lines, i) {
            if !current_plain.is_empty() {
                segments.push(MdSegment::Plain(current_plain.clone()));
                current_plain.clear();
            }
            let (headers, rows, consumed) = parse_table(&lines, i);
            segments.push(MdSegment::Table { headers, rows });
            i += consumed;
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
            if !segments.is_empty() && !matches!(segments.last(), Some(MdSegment::ListItem(_))) {
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

/// Parse a task item line: `- [ ] text` or `- [x] text` (also `*` marker).
fn parse_task_item(line: &str) -> Option<(bool, String)> {
    let trimmed = line.trim_start();
    let after_marker = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))?;
    let bytes = after_marker.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'[' || bytes[2] != b']' {
        return None;
    }
    let checked = match bytes[1] {
        b' ' => false,
        b'x' | b'X' => true,
        _ => return None,
    };
    Some((checked, after_marker[3..].trim_start().to_string()))
}

/// Parse a heading line: 1-6 `#` followed by a space.
fn parse_heading(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.bytes().take_while(|&b| b == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    if rest.is_empty() {
        return Some((level as u8, String::new()));
    }
    if rest.starts_with(' ') {
        return Some((level as u8, rest.trim_start().to_string()));
    }
    None
}

/// Parse a blockquote line: leading `>` optionally followed by a space.
fn parse_blockquote(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix('>')?;
    if rest.is_empty() {
        Some(String::new())
    } else if let Some(stripped) = rest.strip_prefix(' ') {
        Some(stripped.to_string())
    } else {
        None
    }
}

/// Split a table row by `|`, trimming edges and cells.
fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed.trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

/// A table separator cell: only `-` and `:` allowed, at least one `-`.
fn is_separator_cell(cell: &str) -> bool {
    !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':') && cell.contains('-')
}

/// Detect a table: current line contains `|` and next line is a separator.
fn is_table_start(lines: &[&str], i: usize) -> bool {
    if i + 1 >= lines.len() {
        return false;
    }
    let cur = lines[i];
    let next = lines[i + 1];
    if !cur.contains('|') || !next.contains('|') {
        return false;
    }
    let cells = split_table_row(next);
    !cells.is_empty() && cells.iter().all(|c| is_separator_cell(c))
}

/// Parse a table starting at line `i`. Returns headers, rows, lines consumed.
fn parse_table(lines: &[&str], i: usize) -> (Vec<String>, Vec<Vec<String>>, usize) {
    let headers = split_table_row(lines[i]);
    let mut rows = Vec::new();
    let mut idx = i + 2;
    while idx < lines.len() {
        let line = lines[idx];
        if !line.contains('|') || line.trim().is_empty() {
            break;
        }
        rows.push(split_table_row(line));
        idx += 1;
    }
    (headers, rows, idx - i)
}

/// Parse inline formatting within a single line.
fn parse_inline(line: &str) -> Vec<MdSegment> {
    let mut segments: Vec<MdSegment> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut current_plain = String::new();

    while i < len {
        // Link: [text](url)
        if chars[i] == '[' {
            if let Some((text, url, end)) = parse_link(&chars, i) {
                if !current_plain.is_empty() {
                    segments.push(MdSegment::Plain(current_plain.clone()));
                    current_plain.clear();
                }
                let _ = &text;
                segments.push(MdSegment::Link(text, url));
                i = end;
                continue;
            }
        }

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

/// Parse a markdown link `[text](url)` starting at `start` (the `[`).
/// Returns (text, url, index_after_closing_paren) on success.
fn parse_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    let len = chars.len();
    let close_bracket = (start + 1..len).find(|&j| chars[j] == ']')?;
    if close_bracket + 1 >= len || chars[close_bracket + 1] != '(' {
        return None;
    }
    let open_paren = close_bracket + 1;
    let close_paren = (open_paren + 1..len).find(|&j| chars[j] == ')')?;
    if close_paren == open_paren + 1 {
        return None;
    }
    let text: String = chars[start + 1..close_bracket].iter().collect();
    let url: String = chars[open_paren + 1..close_paren].iter().collect();
    Some((text, url, close_paren + 1))
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
            MdSegment::Bold(s) => txt(px(13.))
                .font_weight(FontWeight::BOLD)
                .text_color(fg)
                .line_height(px(18.))
                .child(s.clone())
                .into_any_element(),
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
                    .w_full()
                    .gap(px(8.))
                    .items_start()
                    .child(txt(px(13.)).text_color(list_prefix).child("•".to_string()))
                    .child(
                        txt(px(13.))
                            .text_color(fg)
                            .line_height(px(18.))
                            .child(item_text),
                    )
                    .into_any_element()
            }
            MdSegment::Heading(level, s) => {
                let (size, lh, weight) = match *level {
                    1 => (px(20.), px(26.), FontWeight::BOLD),
                    2 => (px(17.), px(23.), FontWeight::BOLD),
                    3 => (px(15.), px(21.), FontWeight::SEMIBOLD),
                    _ => (px(14.), px(20.), FontWeight::SEMIBOLD),
                };
                txt(size)
                    .font_weight(weight)
                    .text_color(fg)
                    .line_height(lh)
                    .child(s.clone())
                    .into_any_element()
            }
            MdSegment::Blockquote(s) => h_flex()
                .w_full()
                .gap(px(8.))
                .items_start()
                .child(div().w(px(3.)).h_full().bg(gpui::rgba(0xFFFFFF20)))
                .child(
                    txt(px(13.))
                        .italic()
                        .text_color(mu)
                        .line_height(px(18.))
                        .child(s.clone()),
                )
                .into_any_element(),
            MdSegment::Link(text, _url) => txt(px(13.))
                .text_color(theme.accent)
                .underline()
                .line_height(px(18.))
                .child(text.clone())
                .into_any_element(),
            MdSegment::Task(checked, s) => {
                let box_char = if *checked { "☑" } else { "☐" };
                let box_color = if *checked { theme.accent } else { mu };
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .items_start()
                    .child(
                        txt(px(13.))
                            .text_color(box_color)
                            .child(box_char.to_string()),
                    )
                    .child(
                        txt(px(13.))
                            .text_color(fg)
                            .line_height(px(18.))
                            .child(s.clone()),
                    )
                    .into_any_element()
            }
            MdSegment::Table { headers, rows } => {
                let col_count = headers.len();
                let mut grid = v_flex()
                    .gap(px(0.))
                    .child(render_table_row(headers, true, theme));
                for row in rows.iter() {
                    let padded = pad_row(row, col_count);
                    grid = grid.child(render_table_row(&padded, false, theme));
                }
                grid.into_any_element()
            }
        })
        .collect()
}

fn pad_row(row: &[String], col_count: usize) -> Vec<String> {
    let mut out = row.to_vec();
    while out.len() < col_count {
        out.push(String::new());
    }
    out
}

fn render_table_row(cells: &[String], is_header: bool, theme: &Theme) -> AnyElement {
    let fg = theme.foreground;
    let mu = theme.muted_foreground;
    let border = gpui::rgba(0xFFFFFF18);
    let header_bg = gpui::rgba(0xFFFFFF0C);
    let row = h_flex();
    let row = if is_header { row.bg(header_bg) } else { row };
    let count = cells.len();
    let mut row = row;
    for (i, cell) in cells.iter().enumerate() {
        let color = if is_header { fg } else { mu };
        let weight = if is_header {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        };
        row = row.child(
            div()
                .flex_1()
                .min_w(px(40.))
                .px(px(8.))
                .py(px(4.))
                .border_1()
                .border_color(border)
                .when(i + 1 < count, |p| p.border_r_0())
                .child(
                    txt(px(12.))
                        .text_color(color)
                        .font_weight(weight)
                        .child(cell.clone()),
                ),
        );
    }
    row.into_any_element()
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn txt(size: gpui::Pixels) -> gpui::Div {
    div().text_size(size).whitespace_normal().min_w_0()
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text() {
        let segments = parse_markdown("Hello world");
        assert!(!segments.is_empty());
        let plain_segments: Vec<&MdSegment> = segments
            .iter()
            .filter(|s| matches!(s, MdSegment::Plain(_)))
            .collect();
        assert!(
            plain_segments
                .iter()
                .any(|s| matches!(s, MdSegment::Plain(s) if s == "Hello world"))
        );
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
        let italic = segments
            .iter()
            .find(|s| matches!(s, MdSegment::Italic(s) if s == "italic text"));
        assert!(italic.is_some(), "should find italic, got {:?}", segments);
    }

    #[test]
    fn test_inline_code() {
        let segments = parse_markdown("Use `fn main` in Rust");
        assert!(!segments.is_empty());
        let has_code = segments
            .iter()
            .any(|s| matches!(s, MdSegment::InlineCode(c) if c == "fn main"));
        assert!(has_code, "should find inline code, got {:?}", segments);
    }

    #[test]
    fn test_code_block() {
        let text = "```rust\nfn main() {}\n```";
        let segments = parse_markdown(text);
        let has_code = segments
            .iter()
            .any(|s| matches!(s, MdSegment::CodeBlock(c) if c.contains("fn main")));
        assert!(has_code, "should find code block, got {:?}", segments);
    }

    #[test]
    fn test_list_item_dash() {
        let segments = parse_markdown("- item one\n- item two");
        let list_items: Vec<&MdSegment> = segments
            .iter()
            .filter(|s| matches!(s, MdSegment::ListItem(_)))
            .collect();
        assert_eq!(list_items.len(), 2);
        match &list_items[0] {
            MdSegment::ListItem(s) => assert_eq!(s, "item one"),
            _ => panic!("expected ListItem"),
        }
    }

    #[test]
    fn test_list_item_numbered() {
        let segments = parse_markdown("1. first\n2. second");
        let list_items: Vec<&MdSegment> = segments
            .iter()
            .filter(|s| matches!(s, MdSegment::ListItem(_)))
            .collect();
        assert_eq!(list_items.len(), 2);
        match &list_items[0] {
            MdSegment::ListItem(s) => assert_eq!(s, "first"),
            _ => panic!("expected ListItem"),
        }
    }

    #[test]
    fn test_list_item_star() {
        let segments = parse_markdown("* star item");
        let list_items: Vec<&MdSegment> = segments
            .iter()
            .filter(|s| matches!(s, MdSegment::ListItem(_)))
            .collect();
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
        assert!(
            segments
                .iter()
                .any(|s| matches!(s, MdSegment::Bold(s) if s == "bold"))
        );
        assert!(
            segments
                .iter()
                .any(|s| matches!(s, MdSegment::Italic(s) if s == "italic"))
        );
        assert!(
            segments
                .iter()
                .any(|s| matches!(s, MdSegment::InlineCode(s) if s == "code"))
        );
    }

    #[test]
    fn test_empty_string() {
        let segments = parse_markdown("");
        assert!(
            segments.is_empty()
                || segments
                    .iter()
                    .all(|s| matches!(s, MdSegment::Plain(s) if s.is_empty()))
        );
    }

    #[test]
    fn test_multiline_plain() {
        let segments = parse_markdown("line one\nline two");
        let plain_segments: Vec<&MdSegment> = segments
            .iter()
            .filter(|s| matches!(s, MdSegment::Plain(s) if !s.is_empty()))
            .collect();
        assert!(
            plain_segments.len() >= 2,
            "should have at least 2 non-empty plain segments, got {:?}",
            segments
        );
    }

    #[test]
    fn test_unmatched_backtick_kept_as_plain() {
        let segments = parse_markdown("no closing ` backtick");
        let has_plain = segments
            .iter()
            .any(|s| matches!(s, MdSegment::Plain(s) if s.contains('`')));
        assert!(
            has_plain,
            "should keep unmatched backtick as plain text, got {:?}",
            segments
        );
    }

    #[test]
    fn test_unmatched_star_kept_as_plain() {
        let segments = parse_markdown("no closing * star");
        let has_plain = segments
            .iter()
            .any(|s| matches!(s, MdSegment::Plain(s) if s.contains('*')));
        assert!(
            has_plain,
            "should keep unmatched star as plain text, got {:?}",
            segments
        );
    }

    #[test]
    fn test_double_backtick_inline_code() {
        let segments = parse_markdown("``fn main() {}``");
        let has_code = segments
            .iter()
            .any(|s| matches!(s, MdSegment::InlineCode(s) if s == "fn main() {}"));
        assert!(
            has_code,
            "should parse double-backtick code, got {:?}",
            segments
        );
    }

    #[test]
    fn test_heading() {
        let segments = parse_markdown("## Title");
        let has_h = segments
            .iter()
            .any(|s| matches!(s, MdSegment::Heading(2, t) if t == "Title"));
        assert!(has_h, "should parse h2, got {:?}", segments);
    }

    #[test]
    fn test_blockquote() {
        let segments = parse_markdown("> quoted text");
        let has_bq = segments
            .iter()
            .any(|s| matches!(s, MdSegment::Blockquote(t) if t == "quoted text"));
        assert!(has_bq, "should parse blockquote, got {:?}", segments);
    }

    #[test]
    fn test_table() {
        let text = "| Name | Age |\n| --- | --- |\n| Ada | 30 |\n| Bob | 25 |";
        let segments = parse_markdown(text);
        let has_table = segments
            .iter()
            .any(|s| matches!(s, MdSegment::Table { headers, .. } if headers.len() == 2));
        assert!(has_table, "should parse table, got {:?}", segments);
    }

    #[test]
    fn test_link() {
        let segments = parse_markdown("see [docs](https://example.com) now");
        let has_link = segments.iter().any(
            |s| matches!(s, MdSegment::Link(t, u) if t == "docs" && u == "https://example.com"),
        );
        assert!(has_link, "should parse link, got {:?}", segments);
    }

    #[test]
    fn test_task_item_unchecked() {
        let segments = parse_markdown("- [ ] todo");
        let has_task = segments
            .iter()
            .any(|s| matches!(s, MdSegment::Task(false, t) if t == "todo"));
        assert!(has_task, "should parse unchecked task, got {:?}", segments);
    }

    #[test]
    fn test_task_item_checked() {
        let segments = parse_markdown("- [x] done");
        let has_task = segments
            .iter()
            .any(|s| matches!(s, MdSegment::Task(true, t) if t == "done"));
        assert!(has_task, "should parse checked task, got {:?}", segments);
    }
}
