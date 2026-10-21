//! Drawing what a plugin declared: a panel of one of the built-in views, the
//! sidebar line that opens it, and a status-bar segment formatted from data.
//!
//! A view reads JSON and nothing else. The declaration names a command, the
//! command prints JSON, and the shape of that JSON picks the layout — an object
//! is a key/value list, an array a list, a string (or an array) a log, and
//! `{columns, rows}` (or an array of objects) a table. A shape a view cannot use
//! is shown as it arrived rather than hidden: a wrong shape is the plugin
//! author's to see. The rules live in [`Rows`], which is why they can be tested
//! without a window.

use gpui_kit::component::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use gpui_kit::component::{Icon, Theme, h_flex, v_flex};
use gpui_kit::*;
use serde_json::Value;

use super::panels::{header, line};
use super::ui_data::Data;
use super::ui_manifest::{Declared, PanelSpec, SidebarItemSpec, StatusItemSpec, ViewKind};
use crate::shell::registry::{Invocation, ShellRequest};

/// The mark a plugin's own contribution carries: the plugin's id, in the accent
/// the prototype gives plugin furniture. Every slot that can hold a plugin's
/// contribution draws this same badge, so a sidebar row, a panel card and a
/// status segment are recognisable as one plugin's — and a contribution drawn
/// without one is the shell's own, by construction.
pub(crate) fn source_badge(theme: &Theme, plugin_id: &str) -> AnyElement {
    // The same puzzle the rail uses for plugins, small enough to sit in a line
    // of text; `currentColor` lets the badge's own accent reach it.
    let glyph = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 4h3a2 2 0 0 1 2 2v1.5a1.5 1.5 0 1 0 3 0V6h1a2 2 0 0 1 2 2v3h-1.5a1.5 1.5 0 1 0 0 3H20v3a2 2 0 0 1-2 2h-3v-1.5a1.5 1.5 0 1 0-3 0V19H9a2 2 0 0 1-2-2v-3H5.5a1.5 1.5 0 1 1 0-3H7V6a2 2 0 0 1 2-2Z"/></svg>"#;
    h_flex()
        .flex_none()
        .gap_1()
        .items_center()
        .px_1()
        .rounded(px(3.))
        .bg(theme.magenta.opacity(0.12))
        .text_color(theme.magenta)
        .border_1()
        .border_color(theme.magenta.opacity(0.3))
        .child(Icon::default().data(glyph).w(px(9.)).h(px(9.)))
        .child(div().text_xs().child(plugin_id.to_owned()))
        .into_any_element()
}

/// One declared panel: whose it is, its title, and the data its command last
/// printed. The data is read by the host, never by this view, so a panel is a
/// pure function of the last answer.
pub(crate) fn panel(
    spec: &PanelSpec,
    invocation: &Invocation,
    _: &mut Window,
    _: &mut App,
) -> AnyElement {
    let theme = invocation.theme;
    let column = v_flex().w_full().px_1().py_1().gap_0p5().child(header(
        spec.qualified_id(),
        &spec.title,
        &crate::i18n::t("general.refresh"),
        ShellRequest::RefreshPlugins,
        Some(source_badge(theme, &spec.plugin_id)),
        invocation,
    ));
    column
        .child(match invocation.ui_data.get(&spec.qualified_id()) {
            Some(Data::Ready(value)) => draw(rows(spec.view, value), theme),
            Some(Data::Failed(why)) => failure(why, theme),
            // Nothing yet and nothing wrong: the command has not answered.
            Some(Data::Loading) | None => line(
                &crate::i18n::t("shell.views.reading"),
                theme.muted_foreground,
            ),
        })
        .into_any_element()
}

/// A sidebar line that opens the panels a plugin declared. A drawer panel
/// mounts in the right-hand panel, so the line's one job is to show it — and to
/// say whose line it is.
pub(crate) fn sidebar_item(
    spec: &SidebarItemSpec,
    invocation: &Invocation,
    _: &mut Window,
    _: &mut App,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let title = spec.title.clone();
    h_flex()
        .id(spec.qualified_id())
        .w_full()
        .px_1()
        .py_0p5()
        .gap_1()
        .items_center()
        .rounded_sm()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::ShowPluginPanels, window, cx);
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_sm()
                .text_color(theme.sidebar_foreground)
                .child(title),
        )
        .child(source_badge(theme, &spec.plugin_id))
        .into_any_element()
}

/// One status segment: whose it is, and the declared format with the answer's
/// fields in it. The segment is narrower than a row, so the plugin is named by
/// its id in the accent rather than by a badge.
pub(crate) fn status_item(
    spec: &StatusItemSpec,
    invocation: &Invocation,
    _: &mut Window,
    _: &mut App,
) -> AnyElement {
    let theme = invocation.theme;
    let answer = match invocation.ui_data.get(&spec.qualified_id()) {
        Some(Data::Ready(value)) => Some(value.as_ref()),
        _ => None,
    };
    h_flex()
        .flex_none()
        .gap_1()
        .items_center()
        .text_xs()
        .text_color(theme.magenta)
        .child(div().child(spec.plugin_id.clone()))
        .child(div().child("·"))
        .child(div().child(fill(&spec.format, answer)))
        .into_any_element()
}

/// What one JSON answer becomes, before any of it is drawn.
#[derive(Debug, PartialEq, Eq)]
enum Rows {
    /// A header row and body rows.
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// A key and its value, per row.
    Pairs(Vec<(String, String)>),
    /// One cell per row.
    Lines(Vec<String>),
}

fn rows(view: ViewKind, value: &Value) -> Rows {
    match view {
        ViewKind::Table => table(value),
        ViewKind::Kv => Rows::Pairs(pairs(value)),
        ViewKind::List => match value {
            Value::Array(items) => Rows::Lines(items.iter().map(cell).collect()),
            other => Rows::Lines(vec![cell(other)]),
        },
        ViewKind::Log => match value {
            Value::String(text) => Rows::Lines(text.lines().map(str::to_owned).collect()),
            Value::Array(items) => Rows::Lines(items.iter().map(cell).collect()),
            other => Rows::Lines(vec![cell(other)]),
        },
    }
}

/// A table, from either of the two shapes a plugin can print: the explicit
/// `{columns, rows}`, or an array of objects whose keys become the columns.
fn table(value: &Value) -> Rows {
    match value {
        Value::Object(map) => {
            let columns = map
                .get("columns")
                .and_then(Value::as_array)
                .map(|columns| columns.iter().map(cell).collect());
            let body = map
                .get("rows")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().map(row_cells).collect());
            match (columns, body) {
                (Some(columns), Some(rows)) => Rows::Table { columns, rows },
                _ => Rows::Pairs(pairs(value)),
            }
        }
        Value::Array(items) => {
            let mut objects = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::Object(map) => objects.push(map),
                    // Not a table at all: show what arrived.
                    _ => return Rows::Lines(items.iter().map(cell).collect()),
                }
            }
            let mut columns: Vec<String> = Vec::new();
            for map in &objects {
                for key in map.keys() {
                    if !columns.iter().any(|column| column == key) {
                        columns.push(key.clone());
                    }
                }
            }
            let rows = objects
                .iter()
                .map(|map| {
                    columns
                        .iter()
                        .map(|column| map.get(column).map(cell).unwrap_or_default())
                        .collect()
                })
                .collect();
            Rows::Table { columns, rows }
        }
        // Neither shape: a table of one key/value pair is still readable.
        _ => Rows::Pairs(pairs(value)),
    }
}

fn pairs(value: &Value) -> Vec<(String, String)> {
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| (key.clone(), cell(value)))
            .collect(),
        other => vec![(String::new(), cell(other))],
    }
}

fn row_cells(row: &Value) -> Vec<String> {
    match row {
        Value::Array(cells) => cells.iter().map(cell).collect(),
        other => vec![cell(other)],
    }
}

/// One JSON value as the text of a cell. A container is shown as compact JSON,
/// because guessing at a nested table would be a view of its own.
fn cell(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The status text, with `{field}` holes filled from the answer. A hole with no
/// field behind it stays visible as `–`: a segment that silently drops a number
/// reads as a zero.
fn fill(format: &str, answer: Option<&Value>) -> String {
    let mut text = String::with_capacity(format.len());
    let mut rest = format;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        text.push_str(&rest[..open]);
        let name = &rest[open + 1..open + close];
        let hole = answer.and_then(|answer| answer.get(name)).map(cell);
        text.push_str(hole.as_deref().unwrap_or("–"));
        rest = &rest[open + close + 1..];
    }
    text.push_str(rest);
    text
}

fn draw(rows: Rows, theme: &Theme) -> AnyElement {
    match rows {
        Rows::Table { columns, rows } => Table::new()
            .w_full()
            .child(
                TableHeader::new().child(
                    TableRow::new().children(
                        columns
                            .iter()
                            .map(|column| TableHead::new().text_xs().child(column.clone())),
                    ),
                ),
            )
            .child(TableBody::new().children(rows.iter().map(|row| {
                TableRow::new().children(
                    row.iter()
                        .map(|cell| TableCell::new().text_xs().child(cell.clone())),
                )
            })))
            .into_any_element(),
        Rows::Pairs(pairs) => {
            if pairs.is_empty() {
                return line(
                    &crate::i18n::t("shell.views.no_data"),
                    theme.muted_foreground,
                );
            }
            let mut column = v_flex().w_full().gap_0p5();
            for (key, value) in pairs {
                column = column.child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(key),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_xs()
                                .text_color(theme.foreground)
                                .child(value),
                        ),
                );
            }
            column.into_any_element()
        }
        Rows::Lines(lines) => {
            if lines.is_empty() {
                return line(
                    &crate::i18n::t("shell.views.no_data"),
                    theme.muted_foreground,
                );
            }
            let mut column = v_flex().w_full().gap_0p5();
            for text in lines {
                column = column.child(line(&text, theme.foreground));
            }
            column.into_any_element()
        }
    }
}

/// A source that failed says why, in the panel the failure belongs to: the rest
/// of the shell keeps working.
fn failure(why: &str, theme: &Theme) -> AnyElement {
    v_flex()
        .w_full()
        .px_1()
        .py_0p5()
        .rounded_sm()
        .border_1()
        .border_color(theme.danger)
        .child(line(why, theme.danger))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    fn json(text: &str) -> Value {
        serde_json::from_str(text).expect("the fixture JSON parses")
    }

    #[test]
    fn an_object_is_a_key_value_view_in_its_own_order() {
        assert_eq!(
            rows(ViewKind::Kv, &json(r#"{"b": 1, "a": "two"}"#)),
            Rows::Pairs(vec![("b".into(), "1".into()), ("a".into(), "two".into())])
        );
    }

    #[test]
    fn an_array_is_a_list_of_one_cell_per_element() {
        assert_eq!(
            rows(ViewKind::List, &json(r#"["one", 2, {"three": true}]"#)),
            Rows::Lines(vec!["one".into(), "2".into(), r#"{"three":true}"#.into()])
        );
    }

    #[test]
    fn a_string_is_a_log_of_its_lines_and_an_array_is_a_log_of_its_elements() {
        assert_eq!(
            rows(ViewKind::Log, &json(r#""first\nsecond""#)),
            Rows::Lines(vec!["first".into(), "second".into()])
        );
        assert_eq!(
            rows(ViewKind::Log, &json(r#"["only"]"#)),
            Rows::Lines(vec!["only".into()])
        );
    }

    #[test]
    fn a_table_reads_the_declared_columns_and_rows() {
        let value = json(r#"{"columns": ["Pane", "Status"], "rows": [["w1:p1", "ready"]]}"#);
        assert_eq!(
            rows(ViewKind::Table, &value),
            Rows::Table {
                columns: vec!["Pane".into(), "Status".into()],
                rows: vec![vec!["w1:p1".into(), "ready".into()]],
            }
        );
    }

    /// An array of objects is the other table shape: the columns are the keys in
    /// the order they first appear, and a row that lacks one leaves a hole rather
    /// than shifting its neighbours.
    #[test]
    fn an_array_of_objects_becomes_a_table_with_the_keys_as_columns() {
        let value = json(r#"[{"pane": "w1:p1", "lines": 42}, {"pane": "w1:p2"}]"#);
        assert_eq!(
            rows(ViewKind::Table, &value),
            Rows::Table {
                columns: vec!["pane".into(), "lines".into()],
                rows: vec![
                    vec!["w1:p1".into(), "42".into()],
                    vec!["w1:p2".into(), String::new()],
                ],
            }
        );
    }

    /// A shape the view cannot use is shown as it arrived: a plugin author
    /// reading their own panel sees the answer, not an empty box.
    #[test]
    fn a_shape_the_view_cannot_use_is_still_shown() {
        assert_eq!(
            rows(ViewKind::Table, &json(r#"[1, 2]"#)),
            Rows::Lines(vec!["1".into(), "2".into()])
        );
        assert_eq!(
            rows(ViewKind::Kv, &json("7")),
            Rows::Pairs(vec![(String::new(), "7".into())])
        );
        // The explicit table shape with a piece missing falls back to its pairs.
        assert_eq!(
            rows(ViewKind::Table, &json(r#"{"columns": ["a"]}"#)),
            Rows::Pairs(vec![("columns".into(), r#"["a"]"#.into())])
        );
    }

    #[test]
    fn a_status_format_fills_its_holes_and_marks_the_ones_it_cannot() {
        assert_eq!(fill("{count} WIP", Some(&json(r#"{"count": 3}"#))), "3 WIP");
        // No answer yet, and no such field: both stay visible.
        assert_eq!(fill("{count} WIP", None), "– WIP");
        assert_eq!(fill("{count} WIP", Some(&json("{}"))), "– WIP");
        // Text around the holes is untouched, and a hole that never closes is
        // left as it was typed.
        assert_eq!(fill("a{one}b{two}c", Some(&json(r#"{"one": 1}"#))), "a1b–c");
        assert_eq!(
            fill("open {count", Some(&json(r#"{"count": 1}"#))),
            "open {count"
        );
    }
}
