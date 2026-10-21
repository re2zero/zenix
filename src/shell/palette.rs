//! The command palette: the shell's own directory and the daemon's plugin
//! actions, in one filterable list.
//!
//! The palette is a row of data, not a second command system: an entry is
//! either a command from the directory or a plugin action, and both leave
//! through the same [`ShellRequest`] the sidebar's clicks use. Nothing here
//! knows what a plugin action does; it hands the action back to the shell,
//! which hands it to the daemon.
//!
//! A plugin row is one catalog entry ([`crate::plugins::actions`]) — the same
//! entry the keymap binds a chord to and the MCP bridge exposes as a tool, so a
//! row cannot outlive the action it would run.
//!
//! The query is typed straight into the palette: it holds the keyboard while it
//! is open, so the shell needs no text input and no IME client here.

use gpui_kit::component::{ActiveTheme, Theme, h_flex, v_flex};
use gpui_kit::*;

use std::sync::Arc;

use super::commands::{COMMANDS, Command};
use super::keymap::Keymap;
use super::registry::{Dispatch, ShellRequest};
use crate::i18n::t;
use crate::plugins::actions::Exposed;

/// How many rows the palette shows at once. The rest are reached by typing.
const ROWS_SHOWN: usize = 8;

/// One row the palette can run.
#[derive(Clone)]
pub(crate) enum Entry {
    Command(Command),
    Plugin(Arc<Exposed>),
}

impl Entry {
    /// What the row is called.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Command(command) => COMMANDS
                .iter()
                .find(|info| info.command == *command)
                .map(|info| info.label())
                .unwrap_or_else(|| format!("{command:?}")),
            Self::Plugin(entry) => entry.label().to_owned(),
        }
    }

    /// The dim line under the label: which plugin an action belongs to, or the
    /// command's own id.
    fn detail(&self) -> String {
        match self {
            Self::Command(command) => COMMANDS
                .iter()
                .find(|info| info.command == *command)
                .map(|info| info.id.to_owned())
                .unwrap_or_default(),
            Self::Plugin(entry) => entry.action.qualified_id(),
        }
    }

    /// The keystroke that reaches this row without the palette. A plugin's
    /// chord is shown the way it is pressed: claimed outright when it carries a
    /// modifier of its own, after the prefix when it is a bare key — see
    /// [`crate::shell::keymap`].
    fn hint(&self, keymap: &Keymap) -> Option<String> {
        match self {
            Self::Plugin(entry) => entry.chord().map(|key| keymap.chord_label(key)),
            Self::Command(command) => {
                let info = COMMANDS.iter().find(|info| info.command == *command)?;
                if let Some(direct) = info.direct.first() {
                    return Some((*direct).to_owned());
                }
                info.prefixed
                    .first()
                    .map(|chord| format!("{} {chord}", keymap.prefix_label()))
            }
        }
    }

    /// What running the row asks the shell to do.
    pub(crate) fn request(&self) -> ShellRequest {
        match self {
            Self::Command(command) => ShellRequest::Command(*command),
            // The palette is a click like any other: the daemon still assembles
            // the invocation context, and only the source is ours to name.
            Self::Plugin(entry) => ShellRequest::PluginAction {
                action: entry.action.clone(),
                source: "palette",
            },
        }
    }
}

/// The palette's own state: whether it is open, what has been typed into it and
/// which row that leaves selected.
#[derive(Default)]
pub(crate) struct Palette {
    open: bool,
    query: String,
    selected: usize,
}

/// What a keystroke asked the palette to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// Held: the palette swallowed it.
    Handled,
    /// Escape: close and hand the keyboard back.
    Close,
    /// Enter: run the row at this index of [`Palette::matches`].
    Run(usize),
}

impl Palette {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Open with an empty query: a palette that remembered the last search
    /// would hide the command the user is about to look for.
    pub(crate) fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    /// The rows this query leaves, in the order they are shown.
    pub(crate) fn matches<'a>(&self, entries: &'a [Entry]) -> Vec<&'a Entry> {
        let query = self.query.trim().to_lowercase();
        entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.label().to_lowercase().contains(&query)
                    || entry.detail().to_lowercase().contains(&query)
            })
            .collect()
    }

    /// The row Enter runs, as an index into `entries`.
    pub(crate) fn keystroke(&mut self, keystroke: &Keystroke, rows: usize) -> Key {
        // A chord is never query text: the palette has no shortcuts of its own,
        // and typing a modified key into the search would be a surprise. Shift is
        // not a chord: it is how an upper-case letter or a shifted symbol is typed.
        let chord = keystroke.modifiers.control
            || keystroke.modifiers.alt
            || keystroke.modifiers.platform
            || keystroke.modifiers.function;
        match keystroke.key.as_str() {
            "escape" => Key::Close,
            "enter" => {
                if rows == 0 {
                    Key::Handled
                } else {
                    Key::Run(self.selected.min(rows - 1))
                }
            }
            "up" => {
                self.selected = self.selected.saturating_sub(1);
                Key::Handled
            }
            "down" => {
                if rows > 0 {
                    self.selected = (self.selected + 1).min(rows - 1);
                }
                Key::Handled
            }
            "backspace" => {
                self.query.pop();
                self.selected = 0;
                Key::Handled
            }
            _ if !chord => {
                let typed = keystroke
                    .key_char
                    .as_deref()
                    .filter(|text| !text.is_empty())
                    .unwrap_or(keystroke.key.as_str());
                if typed.chars().count() == 1 {
                    self.query.push_str(typed);
                    self.selected = 0;
                }
                Key::Handled
            }
            _ => Key::Handled,
        }
    }

    fn query(&self) -> &str {
        &self.query
    }

    fn selected(&self) -> usize {
        self.selected
    }
}

/// The palette, laid over the shell while it is open. It holds the keyboard,
/// so the pane beside it is not typed into while a command is being looked up.
pub(crate) fn overlay(
    palette: &Palette,
    entries: &[Entry],
    keymap: &Keymap,
    focus: &FocusHandle,
    dispatch: &Dispatch,
    key: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let matches = palette.matches(entries);
    let selected = palette.selected();
    let close = dispatch.clone();
    let query = if palette.query().is_empty() {
        div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(t("shell.palette.prompt"))
    } else {
        div()
            .text_sm()
            .text_color(theme.foreground)
            .child(palette.query().to_owned())
    };
    let mut list = v_flex().w_full().gap_0p5();
    for (index, entry) in matches.iter().take(ROWS_SHOWN).enumerate() {
        list = list.child(row(
            index,
            entry,
            index == selected,
            keymap,
            &theme,
            dispatch,
        ));
    }
    if matches.is_empty() {
        list = list.child(
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(t("shell.palette.no_matches")),
        );
    }
    let boxed = v_flex()
        .id("palette")
        .w(px(560.))
        .max_h(px(420.))
        .overflow_hidden()
        .rounded_md()
        .bg(theme.background)
        .text_color(theme.foreground)
        .border_1()
        .border_color(theme.border)
        .track_focus(focus)
        // Every key belongs to the palette while it is open, including the ones
        // nothing is bound to: the shell's own handler decides what a keystroke
        // means here, and stops it before the pane below sees it.
        .on_key_down(key)
        .child(
            div()
                .w_full()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(theme.border)
                .child(query),
        )
        .child(div().w_full().flex_1().min_h_0().child(list));
    h_flex()
        .id("palette-scrim")
        .absolute()
        .inset_0()
        .occlude()
        .justify_center()
        .items_start()
        .pt(px(72.))
        .on_click(move |_, window, cx| {
            close(ShellRequest::Command(Command::TogglePalette), window, cx);
        })
        .child(boxed)
        .into_any_element()
}

/// One row: the label, the dim detail under it, and the key that reaches it
/// without the palette.
fn row(
    index: usize,
    entry: &Entry,
    selected: bool,
    keymap: &Keymap,
    theme: &Theme,
    dispatch: &Dispatch,
) -> AnyElement {
    let request = entry.request();
    let run = dispatch.clone();
    let hint = entry.hint(keymap);
    let mut row = h_flex()
        .id(("palette-row", index))
        .w_full()
        .px_2()
        .py_1()
        .gap_2()
        .items_center()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            run(request.clone(), window, cx);
        })
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .child(div().text_sm().child(entry.label()))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(entry.detail()),
                ),
        );
    if let Some(hint) = hint {
        row = row.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(hint),
        );
    }
    if selected {
        row.bg(theme.selection).into_any_element()
    } else {
        row.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    use crate::plugins::actions::Catalog;
    use crate::plugins::model::{Availability, Context, PluginAction};
    use crate::plugins::ui_manifest::UiContributions;

    fn action(plugin_id: &str, action_id: &str, title: &str) -> PluginAction {
        PluginAction {
            plugin_id: plugin_id.into(),
            action_id: action_id.into(),
            title: title.into(),
            description: None,
            contexts: Some(vec![Context::Workspace]),
        }
    }

    /// One catalog entry, as the shell builds it from what the daemon reports
    /// and what the plugin declared.
    fn announced(key: &str) -> Arc<Exposed> {
        let ui = crate::plugins::ui_manifest::parse(
            "example.board",
            std::path::Path::new("/tmp/board"),
            &format!(
                r#"
[[command]]
id = "apply"
title = "Apply project layout"
action = "apply"
key = "{key}"
"#
            ),
        );
        Catalog::build(&[action("example.board", "apply", "apply title")], &ui).entries()[0].clone()
    }

    fn entries() -> Vec<Entry> {
        vec![
            Entry::Command(Command::ToggleSettings),
            Entry::Plugin(announced("ctrl-shift-l")),
        ]
    }

    fn typed(key: &str) -> Keystroke {
        Keystroke::parse(key).expect("the test keystrokes parse")
    }

    #[test]
    fn an_empty_query_shows_every_row() {
        let palette = Palette::default();
        assert_eq!(palette.matches(&entries()).len(), 2);
    }

    #[test]
    fn a_query_narrows_on_the_label_and_on_the_plugin_id() {
        let mut palette = Palette::default();
        palette.open();
        for character in "apply".chars() {
            palette.keystroke(&typed(&character.to_string()), 2);
        }
        let entries = entries();
        let matches = palette.matches(&entries);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].label(), "Apply project layout");

        // The plugin id matches too, so a user can search by the plugin.
        let mut palette = Palette::default();
        palette.open();
        for character in "board".chars() {
            palette.keystroke(&typed(&character.to_string()), 2);
        }
        assert_eq!(palette.matches(&entries).len(), 1);
    }

    #[test]
    fn enter_runs_the_selected_row_and_escape_closes() {
        let mut palette = Palette::default();
        palette.open();
        assert_eq!(palette.keystroke(&typed("down"), 2), Key::Handled);
        assert_eq!(palette.keystroke(&typed("enter"), 2), Key::Run(1));
        assert_eq!(palette.keystroke(&typed("escape"), 2), Key::Close);
        // An empty list has nothing to run.
        assert_eq!(palette.keystroke(&typed("enter"), 0), Key::Handled);
    }

    #[test]
    fn selection_stops_at_the_ends_instead_of_wrapping() {
        let mut palette = Palette::default();
        palette.open();
        for _ in 0..5 {
            palette.keystroke(&typed("down"), 3);
        }
        assert_eq!(palette.selected(), 2);
        for _ in 0..5 {
            palette.keystroke(&typed("up"), 3);
        }
        assert_eq!(palette.selected(), 0);
    }

    /// The query changes which rows exist, so a selection from the previous
    /// query would point at a different command.
    #[test]
    fn typing_returns_the_selection_to_the_first_row() {
        let mut palette = Palette::default();
        palette.open();
        palette.keystroke(&typed("down"), 2);
        assert_eq!(palette.selected(), 1);
        palette.keystroke(&typed("a"), 2);
        assert_eq!(palette.selected(), 0);
        palette.keystroke(&typed("backspace"), 2);
        assert_eq!(palette.selected(), 0);
        assert_eq!(palette.query(), "");
    }

    /// A chord is not query text: the palette has no shortcuts of its own, and
    /// a modified key pasted into the search would be a surprise.
    #[test]
    fn a_modified_keystroke_is_not_typed_into_the_query() {
        let mut palette = Palette::default();
        palette.open();
        palette.keystroke(&typed("cmd-a"), 2);
        assert_eq!(palette.query(), "");
    }

    /// Shift is how an upper-case letter or a shifted symbol gets into the query,
    /// and the keystroke's key_char is what the user actually typed.
    #[test]
    fn a_shifted_character_is_typed_into_the_query() {
        let mut palette = Palette::default();
        palette.open();
        palette.keystroke(&typed("shift-c->C"), 2);
        palette.keystroke(&typed("shift-p->P"), 2);
        assert_eq!(palette.query(), "CP");
    }

    #[test]
    fn a_command_row_runs_its_command_and_a_plugin_row_runs_its_action() {
        match Entry::Command(Command::ToggleSettings).request() {
            ShellRequest::Command(command) => assert_eq!(command, Command::ToggleSettings),
            _ => panic!("a command row runs a command"),
        }
        match Entry::Plugin(announced("ctrl-shift-l")).request() {
            ShellRequest::PluginAction { action, source } => {
                assert_eq!(action.action_id, "apply");
                assert_eq!(source, "palette");
            }
            _ => panic!("a plugin row runs a plugin action"),
        }
    }

    #[test]
    fn a_command_row_shows_the_key_that_reaches_it_and_a_declared_plugin_row_its_own() {
        let keymap = Keymap::default();
        assert_eq!(
            Entry::Command(Command::ToggleSettings)
                .hint(&keymap)
                .as_deref(),
            Some("cmd-,")
        );
        // A prefixed chord is shown with the prefix it follows.
        assert_eq!(
            Entry::Command(Command::NewTab).hint(&keymap).as_deref(),
            Some("cmd-t")
        );
        assert_eq!(
            Entry::Command(Command::MoveTabNext)
                .hint(&keymap)
                .as_deref(),
            Some("ctrl-b .")
        );
        // A plugin's chord with a modifier of its own is shown alone …
        assert_eq!(
            Entry::Plugin(announced("ctrl-shift-l"))
                .hint(&keymap)
                .as_deref(),
            Some("ctrl-shift-l")
        );
        // … and a bare one with the prefix, because that is how it is pressed.
        assert_eq!(
            Entry::Plugin(announced("shift-y")).hint(&keymap).as_deref(),
            Some("ctrl-b shift-y")
        );
        // An action nothing declared a chord for shows none.
        let undeclared = Catalog::build(&[action("a.b", "x", "X")], &UiContributions::default());
        assert!(
            Entry::Plugin(undeclared.entries()[0].clone())
                .hint(&keymap)
                .is_none()
        );
    }

    /// §M11d's exit condition, seen from the palette: one declaration is the
    /// row, the chord and the tool, and unlinking the plugin leaves none of the
    /// three.
    #[test]
    fn one_declaration_reaches_the_row_the_chord_and_the_tool() {
        let ui = crate::plugins::ui_manifest::parse(
            "example.board",
            std::path::Path::new("/tmp/board"),
            r#"
[[command]]
id = "apply"
title = "Apply project layout"
action = "apply"
key = "ctrl-shift-l"
"#,
        );
        let actions = [action("example.board", "apply", "apply title")];
        let catalog = Catalog::build(&actions, &ui);
        let entry = catalog.entries()[0].clone();

        let row = Entry::Plugin(entry.clone());
        assert_eq!(row.label(), "Apply project layout");

        let mut keymap = Keymap::default();
        keymap.set_plugins(&catalog);
        assert_eq!(row.hint(&keymap).as_deref(), Some("ctrl-shift-l"));
        assert_eq!(
            keymap.claimed(&Keystroke::parse("ctrl-shift-l").expect("the chord parses")),
            Some(crate::shell::keymap::Chord::Plugin(entry.action.clone()))
        );

        assert!(
            catalog.named(entry.tool_name()).is_some(),
            "the tool an agent calls is the row's own action"
        );

        // Unlinked: the daemon stops reporting the action, which is all it takes.
        let gone = Catalog::build(&[], &ui);
        assert!(gone.offered(Availability::of(None)).next().is_none());
        assert!(gone.named(entry.tool_name()).is_none());
    }

    /// A declaration names the row: the daemon's action title is a description
    /// of the action, while the declaration is what the plugin wants its own
    /// command to be called here.
    #[test]
    fn a_declared_title_names_the_row_and_the_daemons_names_it_alone() {
        assert_eq!(
            Entry::Plugin(announced("ctrl-shift-l")).label(),
            "Apply project layout"
        );
        let undeclared = Catalog::build(
            &[action("a.b", "x", "Action title")],
            &UiContributions::default(),
        );
        assert_eq!(
            Entry::Plugin(undeclared.entries()[0].clone()).label(),
            "Action title"
        );
    }
}
