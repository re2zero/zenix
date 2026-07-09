//! Markdown rendering for the AI chat panel.
//!
//! Uses gpui-component's built-in `TextView::markdown()` which provides
//! full GFM support (headings, bold, italic, inline code, code blocks with
//! syntax highlighting, tables, task lists, blockquotes, links, images,
//! strikethrough, and more) via the `markdown-rs` parser.
//!
//! No custom parser needed — gpui-component is already in our dependency tree.

use gpui::{AnyElement, ElementId, IntoElement, ParentElement as _, Styled as _, div, px};
use gpui_component::{Theme, text::TextView, text::TextViewStyle};

/// Render markdown text into a vec containing a single `TextView` element.
///
/// Returns an empty vec when `text` is empty, so callers can use
/// `.children(result)` without special-casing empty content.
pub fn render_markdown(
    text: &str,
    theme: &Theme,
    id: impl Into<ElementId>,
    font_family: &str,
    font_size: f32,
) -> Vec<AnyElement> {
    if text.is_empty() {
        return vec![];
    }
    let style = text_view_style(theme);
    let mut text_view = TextView::markdown(id, text)
        .style(style)
        .text_size(px(font_size));
    if !font_family.is_empty() && font_family != ".default" {
        text_view = text_view.font_family(font_family);
    }
    vec![text_view.into_any_element()]
}

/// Render markdown inside a container with the muted foreground color.
///
/// Used for thinking/reasoning blocks where the text should appear less
/// prominent than the main message content.
pub fn render_markdown_muted(
    text: &str,
    theme: &Theme,
    id: impl Into<ElementId>,
    font_family: &str,
    font_size: f32,
) -> Vec<AnyElement> {
    if text.is_empty() {
        return vec![];
    }
    let style = text_view_style(theme);
    let mut text_view = TextView::markdown(id, text)
        .style(style)
        .text_size(px(font_size));
    if !font_family.is_empty() && font_family != ".default" {
        text_view = text_view.font_family(font_family);
    }
    vec![
        div()
            .text_color(theme.muted_foreground)
            .child(text_view)
            .into_any_element(),
    ]
}

/// Build a `TextViewStyle` from our theme.
fn text_view_style(theme: &Theme) -> TextViewStyle {
    let is_dark = theme.background.l < 0.5;
    let mut style = TextViewStyle::default();
    style.is_dark = is_dark;
    style
}
