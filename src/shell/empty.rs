//! What the grid shows when there is no pane to paint, and what the panel's
//! plugin tab shows before a plugin declares anything.
//!
//! The grid's empty state is the prototype's "想做点什么？" block (PLAN.md
//! §M11c): one line into the command palette, and four entries that each run a
//! command the directory already knows. Nothing here is a second, decorative
//! copy of an action — every card dispatches [`ShellRequest::Command`], so the
//! button a person clicks and the chord they could have typed end in the same
//! place.
//!
//! It is reached in two situations that are the same question to the client:
//! before the first snapshot lands (no surface yet), and when the focused tab
//! has no pane. The daemon gives a new tab one pane, so the second case is
//! rare by design — but it is the honest reading of the state, not a guess.

use gpui_kit::component::{Icon, Theme, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::commands::Command;
use super::registry::{Invocation, ShellRequest};
use crate::i18n::t;

/// The grid with nothing in it: a palette line and the four things a person
/// most often wants next.
pub(crate) fn grid(invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .bg(theme.background)
        .text_color(theme.foreground)
        .p_6()
        .child(
            v_flex()
                .w(px(560.))
                .max_w_full()
                .gap_3()
                .child(mark(theme))
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t("shell.empty.title")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(t("shell.empty.hint")),
                )
                .child(palette_line(invocation))
                .child(cards(invocation)),
        )
        .into_any_element()
}

/// The panel's third tab with nothing declared: the plugin panels are a
/// declaration, so the empty state says where they come from and offers the
/// review that links one.
pub(crate) fn plugin_panels(invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    v_flex()
        .flex_1()
        .min_h_0()
        .w_full()
        .items_center()
        .justify_center()
        .p_4()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(t("shell.empty.panels")),
        )
        .child(
            div()
                .id("empty-trust")
                .px_2()
                .py_0p5()
                .rounded_sm()
                .text_xs()
                .cursor_pointer()
                .bg(theme.secondary)
                .text_color(theme.secondary_foreground)
                .hover(|style| style.bg(theme.secondary_hover))
                .on_click(move |_, window, cx| {
                    dispatch(ShellRequest::Command(Command::LinkPlugin), window, cx);
                })
                .child(t("shell.empty.panels_action")),
        )
        .into_any_element()
}

/// The small square the block is marked with, above the heading.
fn mark(theme: &Theme) -> AnyElement {
    div()
        .size(px(40.))
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted)
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme.muted_foreground)
        .child(Icon::default().data(TERMINAL_ICON).w(px(20.)).h(px(20.)))
        .into_any_element()
}

/// The one-line way in: it opens the palette, which is where a task, a pasted
/// error and a command name all go.
fn palette_line(invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    h_flex()
        .id("empty-palette")
        .w_full()
        .px_3()
        .py_2()
        .gap_2()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .cursor_pointer()
        .text_color(theme.muted_foreground)
        .hover(|style| style.border_color(theme.accent))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(Command::TogglePalette), window, cx);
        })
        .child(Icon::default().data(SEARCH_ICON).w(px(15.)).h(px(15.)))
        .child(div().flex_1().text_sm().child(t("shell.empty.palette")))
        .child(
            div()
                .px_1()
                .rounded_sm()
                .border_1()
                .border_color(theme.border)
                .text_xs()
                .child("Ctrl K"),
        )
        .into_any_element()
}

/// One card: its glyph, the two locale keys it says, and the command it runs.
/// A table rather than four literal blocks, so a test can hold every entry to
/// the command directory instead of trusting the picture.
struct Entry {
    icon: &'static [u8],
    title: &'static str,
    detail: &'static str,
    command: Command,
}

const ENTRIES: [Entry; 4] = [
    Entry {
        icon: WORKSPACES_ICON,
        title: "shell.empty.workspace.title",
        detail: "shell.empty.workspace.detail",
        command: Command::NewWorkspace,
    },
    Entry {
        icon: SPLIT_ICON,
        title: "shell.empty.split.title",
        detail: "shell.empty.split.detail",
        command: Command::SplitRight,
    },
    Entry {
        icon: LAYOUT_ICON,
        title: "shell.empty.layout.title",
        detail: "shell.empty.layout.detail",
        command: Command::RestoreLayout,
    },
    Entry {
        icon: PLUGINS_ICON,
        title: "shell.empty.trust.title",
        detail: "shell.empty.trust.detail",
        command: Command::LinkPlugin,
    },
];

/// The four entries, two by two. Each one is a command from the directory.
fn cards(invocation: &Invocation) -> AnyElement {
    let mut grid = v_flex().w_full().gap_2();
    for (row, pair) in ENTRIES.chunks(2).enumerate() {
        grid = grid.child(h_flex().w_full().gap_2().items_stretch().children(
            pair.iter().enumerate().map(|(column, entry)| {
                card(
                    row * 2 + column,
                    entry.icon,
                    t(entry.title),
                    t(entry.detail),
                    entry.command,
                    invocation,
                )
            }),
        ));
    }
    grid.into_any_element()
}

fn card(
    ix: usize,
    icon: &'static [u8],
    title: String,
    detail: String,
    command: Command,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    v_flex()
        .id(("empty-card", ix))
        .flex_1()
        .min_w_0()
        .gap_1()
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .cursor_pointer()
        .hover(|style| style.bg(theme.muted))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(command), window, cx);
        })
        .child(
            h_flex()
                .gap_1()
                .items_center()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(Icon::default().data(icon).w(px(13.)).h(px(13.)))
                .child(title),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(detail),
        )
        .into_any_element()
}

const TERMINAL_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M12 15h5"/></svg>"#;
const SEARCH_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>"#;
const WORKSPACES_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M9 9v11"/></svg>"#;
const SPLIT_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/></svg>"#;
const LAYOUT_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M12 9v11"/></svg>"#;
const PLUGINS_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 4h3a2 2 0 0 1 2 2v1.5a1.5 1.5 0 1 0 3 0V6h1a2 2 0 0 1 2 2v3h-1.5a1.5 1.5 0 1 0 0 3H20v3a2 2 0 0 1-2 2h-3v-1.5a1.5 1.5 0 1 0-3 0V19H9a2 2 0 0 1-2-2v-3H5.5a1.5 1.5 0 1 1 0-3H7V6a2 2 0 0 1 2-2Z"/></svg>"#;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::shell::commands::COMMANDS;
    use std::prelude::v1::test;

    /// The empty state's promise is that every entry is a command the shell can
    /// actually run, not a card drawn for the picture.
    #[test]
    fn every_entry_names_a_command_in_the_directory() {
        for entry in &ENTRIES {
            assert!(
                COMMANDS.iter().any(|info| info.command == entry.command),
                "{:?} is not in the command directory",
                entry.command
            );
        }
    }

    /// A missing locale key comes back as the key itself, which would read as
    /// `shell.empty.split.title` on screen: the test would rather fail.
    #[test]
    fn every_entry_has_its_copy() {
        for entry in &ENTRIES {
            for key in [entry.title, entry.detail] {
                let copy = t(key);
                assert_ne!(copy, key, "{key} has no translation");
                assert!(!copy.trim().is_empty(), "{key} is blank");
            }
        }
    }
}
