//! The command palette: the directory, the session's workspaces and the
//! plugins' actions in one list, over the library's `CommandPalette`.
//!
//! The palette is a projection, not a second command system. Every row's value
//! is a [`Target`] encoded with its kind, and the three kinds are the three
//! sections the design asks for:
//!
//! * **commands** — the directory ([`super::commands::COMMANDS`]), the same
//!   table the keymap binds and the chrome's trigger labels;
//! * **workspaces** — the session's own, named with their branch, so the
//!   palette reaches one the sidebar cannot show;
//! * **plugin actions** — the catalog's offered entries ([`Catalog`]), the
//!   same list the plugin's chord and the MCP tool come from. A disabled
//!   plugin's actions are filtered by [`Catalog::runnable`] before they are
//!   offered, because the daemon lists them but refuses to run them.
//!
//! The query field, the keyboard (arrows, Enter, Escape), the focus trap and
//! the recency group are the library's. What stays here is the rows and what a
//! picked value means. The recency ring is the shell's because a run outlives
//! one opening of the palette; it stores the same encoded values the rows
//! carry, so a row that disappeared (a closed workspace, a disabled plugin) is
//! simply not offered as a recent one.
//!
//! The library's palette draws no footer and no per-section counts; the
//! prototype's stretch card is wider than the component's 600×400. Those are
//! the component's own shape and the design's mapping table names it, so U2-4
//! does not fork it. If a footer matters later, it belongs upstream.

use ely_core::navigation::{Command as Row, CommandPalette};
use ely_core::primitives::IconName;
use gpui::{AnyElement, Entity, IntoElement, SharedString};

use super::commands::{self, Command};
use super::keymap::Keymap;
use super::shell::Root;
use crate::herdr::LiveState;
use crate::i18n::t;
use crate::plugins::actions::Catalog;
use crate::plugins::host::PluginState;
use crate::plugins::model::Availability;

/// How many runs the recency ring remembers. A ring, not a list: the one after
/// the last forgets the first.
const RECENT_LEN: usize = 16;

/// The palette's own state: whether it is open, and what was run recently.
/// The library owns the query, the cursor and the scroll inside one opening.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) open: bool,
    recent: Vec<SharedString>,
}

impl State {
    /// Remember a run, newest first, without duplicates. A value that no
    /// longer names a row is harmless: the palette filters its recents
    /// against the rows it was given.
    pub(crate) fn note_run(&mut self, value: &SharedString) {
        self.recent.retain(|seen| seen != value);
        self.recent.insert(0, value.clone());
        self.recent.truncate(RECENT_LEN);
    }

    fn recents(&self) -> &[SharedString] {
        &self.recent
    }
}

/// What picking a row asks the shell to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Command(Command),
    /// A workspace to focus, by id.
    Workspace(String),
    /// A plugin action by its MCP tool name: unique per action, and the one
    /// name the catalog can always resolve again.
    Plugin(String),
}

/// The value a row carries. The kind is a prefix so a value decoded after a
/// snapshot move still names the same kind of thing.
pub(crate) fn value_command(info: &commands::CommandInfo) -> SharedString {
    format!("command:{}", info.id).into()
}

fn value_workspace(workspace_id: &str) -> SharedString {
    format!("workspace:{workspace_id}").into()
}

fn value_plugin(tool_name: &str) -> SharedString {
    format!("plugin:{tool_name}").into()
}

/// What an encoded value names, or nothing when it is not one of ours (a
/// plugin declaration, a stale row from another build).
pub(crate) fn decode(value: &str) -> Option<Target> {
    let (kind, rest) = value.split_once(':')?;
    if rest.is_empty() {
        return None;
    }
    match kind {
        "command" => Command::from_id(rest).map(Target::Command),
        "workspace" => Some(Target::Workspace(rest.to_owned())),
        "plugin" => Some(Target::Plugin(rest.to_owned())),
        _ => None,
    }
}

/// The palette's element id. The library keys its query field to this id, and
/// the field lives exactly as long as the element is drawn: closing the
/// palette drops it, so every opening starts on an empty query.
pub(crate) fn id() -> &'static str {
    "shell-palette"
}

/// The palette over the shell, with the three sections built from the state a
/// paint was handed.
pub(crate) fn overlay(
    state: &State,
    live: &LiveState,
    plugins: &PluginState,
    keymap: &Keymap,
    root: &Entity<Root>,
) -> AnyElement {
    let mut commands = Vec::with_capacity(commands::COMMANDS.len());
    for info in commands::COMMANDS {
        let mut row = Row::new(value_command(info), info.label());
        if let Some(icon) = icon(info.command) {
            row = row.icon(icon);
        }
        if let Some(keys) = keymap.label(info.command) {
            row = row.keys(keys);
        }
        commands.push(row);
    }

    let workspaces: Vec<Row> = live
        .snapshot
        .as_deref()
        .map(|snapshot| {
            snapshot
                .workspaces
                .iter()
                .map(|workspace| {
                    let label = match &workspace.branch {
                        Some(branch) => format!("{} — {branch}", workspace.label),
                        None => workspace.label.clone(),
                    };
                    Row::new(value_workspace(&workspace.workspace_id), label)
                        .icon(IconName::LayoutGrid)
                })
                .collect()
        })
        .unwrap_or_default();

    // The catalog is derived from the registry read on the last pulse; a
    // disabled plugin's actions are dropped before they can be offered, and
    // the focus's contexts decide the rest.
    let availability = Availability::of(live.snapshot.as_deref());
    let actions: Vec<Row> = Catalog::build(&plugins.actions, &plugins.ui)
        .runnable(&plugins.plugins)
        .offered(availability)
        .map(|entry| {
            let mut row = Row::new(
                value_plugin(entry.tool_name()),
                format!("{}: {}", entry.action.plugin_id, entry.label()),
            )
            .icon(IconName::Puzzle);
            if let Some(chord) = entry.chord() {
                row = row.keys(keymap.chord_label(chord));
            }
            row
        })
        .collect();

    let close = root.clone();
    let run = root.clone();
    CommandPalette::new(id(), move |_, cx| {
        close.update(cx, |root, cx| root.close_palette(cx));
    })
    .group(t("shell.palette.group_commands"), commands)
    .group(t("shell.palette.group_workspaces"), workspaces)
    .group(t("shell.palette.group_actions"), actions)
    .recent(state.recents().iter().cloned())
    .on_run(move |value, window, cx| {
        run.update(cx, |root, cx| root.run_palette_value(value, window, cx));
    })
    .into_any_element()
}

/// The glyph a command's row wears. A command with no honest icon draws none
/// rather than borrowing one that would lie about what it does.
fn icon(command: Command) -> Option<IconName> {
    Some(match command {
        Command::NewWorkspace => IconName::FolderPlus,
        Command::NewWorktree => IconName::GitBranch,
        Command::NewTab => IconName::Plus,
        Command::CloseTab | Command::ClosePane => IconName::X,
        Command::NextTab | Command::MoveTabNext => IconName::ArrowRight,
        Command::PreviousTab | Command::MoveTabPrevious => IconName::ArrowLeft,
        Command::SplitRight => IconName::Columns2,
        Command::SplitDown => IconName::Rows2,
        Command::FocusLeft => IconName::ArrowLeft,
        Command::FocusRight => IconName::ArrowRight,
        Command::FocusUp => IconName::ArrowUp,
        Command::FocusDown => IconName::ArrowDown,
        Command::Find => IconName::Search,
        Command::CopyMode => IconName::Copy,
        Command::TogglePalette => IconName::Command,
        Command::ToggleAside => IconName::MessageSquare,
        Command::NewWindow => IconName::AppWindow,
        Command::OpenSettings => IconName::Settings,
        Command::LinkPlugin => IconName::Shield,
        Command::InspectLayout => IconName::FileCode,
        Command::OpenReview => IconName::GitBranch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str) -> &'static commands::CommandInfo {
        commands::COMMANDS
            .iter()
            .find(|info| info.id == id)
            .expect("the test names a directory id")
    }

    #[test]
    fn every_kind_of_value_round_trips() {
        assert_eq!(
            decode(&value_command(info("tab.new"))),
            Some(Target::Command(Command::NewTab))
        );
        assert_eq!(
            decode(&value_workspace("w-1")),
            Some(Target::Workspace("w-1".into()))
        );
        assert_eq!(
            decode(&value_plugin("plugin_board_app_sync")),
            Some(Target::Plugin("plugin_board_app_sync".into()))
        );
    }

    /// A value that does not name one of our rows is dropped, not guessed at:
    /// the palette's own values are the only ones it can run.
    #[test]
    fn a_foreign_or_empty_value_names_nothing() {
        assert_eq!(decode(""), None);
        assert_eq!(decode("other:thing"), None);
        assert_eq!(decode("command:"), None);
        assert_eq!(decode("command:tab.retired"), None);
        assert_eq!(decode("workspace:"), None);
        assert_eq!(decode("plugin:"), None);
    }

    /// A workspace id may itself contain a colon; the kind is only the first
    /// segment, so the rest belongs to the id.
    #[test]
    fn only_the_first_segment_is_the_kind() {
        assert_eq!(
            decode("workspace:group:one"),
            Some(Target::Workspace("group:one".into()))
        );
    }

    #[test]
    fn the_recency_ring_leads_with_the_last_run_and_forgets_the_oldest() {
        let mut state = State::default();
        for run in 0..RECENT_LEN {
            state.note_run(&format!("command:run-{run}").into());
        }
        assert_eq!(state.recents().len(), RECENT_LEN);
        assert_eq!(
            state.recents().first().map(|value| value.as_ref()),
            Some("command:run-15")
        );
        state.note_run(&"command:run-0".into());
        assert_eq!(state.recents().len(), RECENT_LEN);
        assert_eq!(
            state.recents().first().map(|value| value.as_ref()),
            Some("command:run-0")
        );
        // The duplicate is one row, not two.
        assert_eq!(
            state
                .recents()
                .iter()
                .filter(|value| value.as_ref() == "command:run-0")
                .count(),
            1
        );
    }
}
