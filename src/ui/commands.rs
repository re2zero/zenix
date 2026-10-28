//! The shell's command directory: one table the chords, the palette and the
//! chrome's own triggers all read.
//!
//! A command names no id of its own: it is an entry in [`COMMANDS`], and that
//! entry's `id` is both the locale key stump (`command.<id>`) and the stable
//! name a palette row carries. The keymap binds the entry's chords; the
//! palette draws its label and the same chords; the status bar and the title
//! strip ask the table what the palette costs. None of the three can drift
//! from what a command does, because all three run through
//! [`Root::run_command`].
//!
//! The table is deliberately small: workspaces, tabs, panes, the terminal's own
//! find and copy mode, a worktree, the palette, the settings dialog, and — as
//! of U3-4 — the two management destinations the palette reaches without a
//! chord: linking a plugin and inspecting the project's layout. The fleet and
//! the events surface are reached from the rail and their own rows; a command
//! row would duplicate those without adding a key.
//!
//! ## Where a command runs
//!
//! A local command is one the window runs itself: `Find`, `CopyMode`,
//! `TogglePalette`, `ToggleAside`, `OpenSettings`, `LinkPlugin`,
//! `InspectLayout`, and `NewWorktree`, which opens a form the daemon's naming
//! then completes. `Root::run_command` is the one match that draws that line,
//! so a chord and a palette row cannot disagree about whether the daemon is
//! asked. Everything else is a daemon method, resolved against the snapshot in
//! [`request`] so a closed tab, workspace or pane cannot be addressed twice.

use herdr_client::Method;
use herdr_client::protocol::{
    ClientShellPane, ClientShellSnapshot, ClientShellTab, ClientShellWorkspace,
};
use serde_json::{Value, json};

use crate::i18n::t;

/// A command the shell can run, from a chord or a palette row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Command {
    NewWorkspace,
    NewWorktree,
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    MoveTabPrevious,
    MoveTabNext,
    SplitRight,
    SplitDown,
    ClosePane,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    Find,
    CopyMode,
    TogglePalette,
    ToggleAside,
    OpenSettings,
    LinkPlugin,
    InspectLayout,
}

/// One command's entry: what it is called, and the keystrokes that reach it.
pub(crate) struct CommandInfo {
    pub(crate) command: Command,
    /// Stable id: the `command.<id>` locale key, and the palette row's value
    /// once the palette prefixes it (see `super::palette`).
    pub(crate) id: &'static str,
    /// A chord with a modifier of its own, reached without the prefix. The
    /// palette, the settings dialog and the aside have one each; everything
    /// else is behind the prefix so typing in a pane keeps its Ctrl keys.
    pub(crate) direct: &'static [&'static str],
    /// Keys typed after the prefix.
    pub(crate) prefixed: &'static [&'static str],
}

impl CommandInfo {
    /// What the command is called, read from the `command` section of the
    /// locale file. Resolved when a row is drawn rather than stored in the
    /// table, so the same table speaks either language.
    pub(crate) fn label(&self) -> String {
        t(&format!("command.{}", self.id))
    }
}

/// Every command this shell mounts, in the order a list shows them.
pub(crate) const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        command: Command::NewWorkspace,
        id: "workspace.new",
        direct: &[],
        prefixed: &["shift-n"],
    },
    // Palette-only: a branch name is the daemon's to generate until the client
    // has a place to ask for one.
    CommandInfo {
        command: Command::NewWorktree,
        id: "worktree.new",
        direct: &[],
        prefixed: &[],
    },
    CommandInfo {
        command: Command::NewTab,
        id: "tab.new",
        direct: &[],
        prefixed: &["t"],
    },
    CommandInfo {
        command: Command::CloseTab,
        id: "tab.close",
        direct: &[],
        prefixed: &["shift-t"],
    },
    CommandInfo {
        command: Command::NextTab,
        id: "tab.next",
        direct: &[],
        prefixed: &["]"],
    },
    CommandInfo {
        command: Command::PreviousTab,
        id: "tab.previous",
        direct: &[],
        prefixed: &["["],
    },
    // A shifted symbol arrives as the symbol itself, so these stay unshifted
    // keys: a letter or a plain punctuation key is what the keymap can match.
    CommandInfo {
        command: Command::MoveTabPrevious,
        id: "tab.move-left",
        direct: &[],
        prefixed: &[","],
    },
    CommandInfo {
        command: Command::MoveTabNext,
        id: "tab.move-right",
        direct: &[],
        prefixed: &["."],
    },
    CommandInfo {
        command: Command::SplitRight,
        id: "pane.split-right",
        direct: &[],
        prefixed: &["s"],
    },
    CommandInfo {
        command: Command::SplitDown,
        id: "pane.split-down",
        direct: &[],
        prefixed: &["v"],
    },
    CommandInfo {
        command: Command::ClosePane,
        id: "pane.close",
        direct: &[],
        prefixed: &["x"],
    },
    CommandInfo {
        command: Command::FocusLeft,
        id: "pane.focus-left",
        direct: &[],
        prefixed: &["h"],
    },
    CommandInfo {
        command: Command::FocusRight,
        id: "pane.focus-right",
        direct: &[],
        prefixed: &["l"],
    },
    CommandInfo {
        command: Command::FocusUp,
        id: "pane.focus-up",
        direct: &[],
        prefixed: &["k"],
    },
    CommandInfo {
        command: Command::FocusDown,
        id: "pane.focus-down",
        direct: &[],
        prefixed: &["j"],
    },
    // Ctrl-F inside the grid is the terminal's own chord, decided by the view
    // so a text field elsewhere keeps its Ctrl-F; the row and the prefixed
    // chord are the directory's way to the same field.
    CommandInfo {
        command: Command::Find,
        id: "pane.find",
        direct: &[],
        prefixed: &["f"],
    },
    CommandInfo {
        command: Command::CopyMode,
        id: "pane.copy-mode",
        direct: &[],
        prefixed: &["c"],
    },
    CommandInfo {
        command: Command::TogglePalette,
        id: "view.palette",
        direct: &["secondary-shift-p"],
        prefixed: &["p"],
    },
    // The conversation panel's own chord, the design's `⌘⇧A`: it carries its
    // own modifier, so it is claimed outright like the palette's. `ctrl-b a`
    // is left to whatever reads after the prefix, as herdr leaves it.
    CommandInfo {
        command: Command::ToggleAside,
        id: "view.toggle-chat",
        direct: &["secondary-shift-a"],
        prefixed: &[],
    },
    // The native settings chord, claimed outright beside the palette's: it
    // carries its own modifier, so no prefix press has to arm first. The
    // prefixed `,` is already the tab-move chord, which is why this one must
    // not also sit behind the prefix.
    CommandInfo {
        command: Command::OpenSettings,
        id: "view.settings",
        direct: &["secondary-,"],
        prefixed: &[],
    },
    // A plugin is linked from a directory the reader names; the review reads
    // it before the daemon does. Palette-only: typing a path is not a chord.
    CommandInfo {
        command: Command::LinkPlugin,
        id: "plugin.link",
        direct: &[],
        prefixed: &[],
    },
    // The project's layout file, its split tree and the daemon's apply.
    // Palette-only for the same reason: it is a destination, not a keystroke.
    CommandInfo {
        command: Command::InspectLayout,
        id: "layout.inspect",
        direct: &[],
        prefixed: &[],
    },
];

impl Command {
    /// The table's entry for this command, when it has one. The table and the
    /// enum are held to the same size by the tests; a caller in a render path
    /// still answers `None` rather than panicking if that ever drifts.
    pub(crate) fn info(self) -> Option<&'static CommandInfo> {
        COMMANDS.iter().find(|info| info.command == self)
    }

    /// The command an id names, for a palette row remembering its run.
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        COMMANDS
            .iter()
            .find(|info| info.id == id)
            .map(|info| info.command)
    }
}

/// The daemon method a command becomes, and its parameters, resolved from the
/// snapshot the command runs against. A local command — or a daemon command
/// whose target is gone — is `None`, which the caller treats as "nothing to
/// do" rather than as an error.
pub(crate) fn request(command: Command, snapshot: &ClientShellSnapshot) -> Option<(Method, Value)> {
    let workspace = focused_workspace(snapshot);
    let tab = focused_tab(snapshot);
    let pane = focused_pane(snapshot);
    Some(match command {
        Command::NewWorkspace => new_workspace(Some(snapshot)),
        Command::NewTab => (
            Method::TabCreate,
            json!({"workspace_id": workspace?.workspace_id, "focus": true}),
        ),
        Command::CloseTab => (Method::TabClose, json!({"tab_id": tab?.tab_id})),
        Command::NextTab | Command::PreviousTab => {
            let workspace = &workspace?.workspace_id;
            let tabs: Vec<&ClientShellTab> = snapshot
                .tabs
                .iter()
                .filter(|tab| &tab.workspace_id == workspace)
                .collect();
            let index = tabs
                .iter()
                .position(|tab| Some(&tab.tab_id) == snapshot.focused_tab_id.as_ref())?;
            let next = if command == Command::NextTab {
                (index + 1) % tabs.len()
            } else {
                (index + tabs.len() - 1) % tabs.len()
            };
            (Method::TabFocus, json!({"tab_id": tabs[next].tab_id}))
        }
        // As herdr's TUI moves it: `insert_index` counts the moving tab still
        // in place, and a tab at either end wraps around to the other.
        Command::MoveTabPrevious | Command::MoveTabNext => {
            let tab = tab?;
            let tabs: Vec<&ClientShellTab> = snapshot
                .tabs
                .iter()
                .filter(|candidate| candidate.workspace_id == tab.workspace_id)
                .collect();
            if tabs.len() <= 1 {
                return None;
            }
            let source = tabs
                .iter()
                .position(|candidate| candidate.tab_id == tab.tab_id)?;
            let insert_index = if command == Command::MoveTabNext {
                if source + 1 == tabs.len() {
                    0
                } else {
                    source + 2
                }
            } else if source == 0 {
                tabs.len()
            } else {
                source - 1
            };
            (
                Method::TabMove,
                json!({"tab_id": tab.tab_id, "insert_index": insert_index}),
            )
        }
        Command::SplitRight | Command::SplitDown => (
            Method::PaneSplit,
            json!({
                "target_pane_id": pane?.pane_id,
                "direction": if command == Command::SplitRight { "right" } else { "down" },
                "focus": true,
            }),
        ),
        Command::ClosePane => (Method::PaneClose, json!({"pane_id": pane?.pane_id})),
        Command::FocusLeft | Command::FocusRight | Command::FocusUp | Command::FocusDown => (
            Method::PaneFocusDirection,
            json!({"pane_id": pane?.pane_id, "direction": direction(command)?}),
        ),
        Command::NewWorktree
        | Command::Find
        | Command::CopyMode
        | Command::TogglePalette
        | Command::ToggleAside
        | Command::OpenSettings
        | Command::LinkPlugin
        | Command::InspectLayout => {
            return None;
        }
    })
}

/// The `workspace.create` request, as the chord and the first-run screens both
/// see it. Creating the first workspace needs no snapshot: the source is only
/// the workspace whose directory the daemon copies, so it is optional here
/// where [`request`] always has one.
pub(crate) fn new_workspace(snapshot: Option<&ClientShellSnapshot>) -> (Method, Value) {
    let mut params = json!({"focus": true});
    if let Some(workspace) = snapshot.and_then(focused_workspace) {
        params["source_workspace_id"] = json!(workspace.workspace_id);
    }
    (Method::WorkspaceCreate, params)
}

/// A pane focus direction, in the daemon's vocabulary.
fn direction(command: Command) -> Option<&'static str> {
    Some(match command {
        Command::FocusLeft => "left",
        Command::FocusRight => "right",
        Command::FocusUp => "up",
        Command::FocusDown => "down",
        _ => return None,
    })
}

fn focused_workspace(snapshot: &ClientShellSnapshot) -> Option<&ClientShellWorkspace> {
    let id = snapshot.focused_workspace_id.as_ref()?;
    snapshot.workspaces.iter().find(|w| &w.workspace_id == id)
}

fn focused_tab(snapshot: &ClientShellSnapshot) -> Option<&ClientShellTab> {
    let workspace = focused_workspace(snapshot)?;
    let id = snapshot.focused_tab_id.as_ref()?;
    snapshot
        .tabs
        .iter()
        .find(|tab| &tab.tab_id == id && tab.workspace_id == workspace.workspace_id)
}

fn focused_pane(snapshot: &ClientShellSnapshot) -> Option<&ClientShellPane> {
    let tab = focused_tab(snapshot)?;
    let id = snapshot.focused_pane_id.as_ref()?;
    snapshot
        .panes
        .iter()
        .find(|pane| &pane.pane_id == id && pane.tab_id == tab.tab_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::t;

    /// The commands the window runs itself, mirroring `Root::run_command`'s own
    /// arms. Kept here rather than in the directory: dispatch is that match's
    /// decision, and this list is what the test below holds it to.
    fn is_local(command: Command) -> bool {
        matches!(
            command,
            Command::Find
                | Command::CopyMode
                | Command::TogglePalette
                | Command::ToggleAside
                | Command::NewWorktree
                | Command::OpenSettings
                | Command::LinkPlugin
                | Command::InspectLayout
        )
    }

    fn empty_snapshot() -> ClientShellSnapshot {
        serde_json::from_value(json!({
            "boot_id": "boot",
            "revision": 1,
            "update_install_command": "",
            "latest_release_notes_available": false,
            "integration_updates_available": false,
            "worktree_directory": "",
            "tab_bar_right_separator": "",
            "tab_bar_right": [],
            "agent_order": [],
            "workspaces": [],
            "tabs": [],
            "panes": [],
            "agents": [],
            "commands": [],
        }))
        .expect("a snapshot with no workspaces still deserializes")
    }

    fn workspace(workspace_id: &str) -> ClientShellWorkspace {
        ClientShellWorkspace {
            workspace_id: workspace_id.into(),
            active_tab_id: String::new(),
            new_workspace_cwd: String::new(),
            number: 1,
            label: workspace_id.into(),
            custom_label: false,
            branch: None,
            git_ahead_behind: None,
            tokens: Vec::new(),
            worktree: None,
            focused: false,
            agent_status: herdr_client::protocol::AgentStatus::Unknown,
        }
    }

    fn tab(tab_id: &str, workspace_id: &str) -> ClientShellTab {
        ClientShellTab {
            tab_id: tab_id.into(),
            workspace_id: workspace_id.into(),
            number: 1,
            label: tab_id.into(),
            custom_label: false,
            zoomed: false,
            focused: false,
            agent_status: herdr_client::protocol::AgentStatus::Unknown,
        }
    }

    fn pane(pane_id: &str, workspace_id: &str, tab_id: &str) -> ClientShellPane {
        ClientShellPane {
            pane_id: pane_id.into(),
            workspace_id: workspace_id.into(),
            tab_id: tab_id.into(),
            label: None,
            cwd: None,
            foreground_cwd: None,
            focused: false,
            right_click_passthrough: false,
        }
    }

    #[test]
    fn every_command_has_one_entry() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|info| info.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total);
        for info in COMMANDS {
            assert_eq!(Command::from_id(info.id), Some(info.command), "{}", info.id);
        }
        // A variant with no entry would panic in `info`; this walks the table
        // once so the enum and the directory stay the same size.
        assert_eq!(COMMANDS.len(), 22);
    }

    /// A label is a key the locale file answers, not a string kept in the
    /// table. `t` hands back the key verbatim when nothing is found, so a
    /// label that comes back as its own key is a command whose key was never
    /// added.
    #[test]
    fn every_command_label_resolves_through_the_locale_file() {
        for info in COMMANDS {
            assert_ne!(
                info.label(),
                format!("command.{}", info.id),
                "{} has no key in the `command` section",
                info.id
            );
        }
    }

    #[test]
    fn a_local_command_never_becomes_a_daemon_method() {
        let snapshot = empty_snapshot();
        for info in COMMANDS {
            if is_local(info.command) {
                assert!(request(info.command, &snapshot).is_none(), "{}", info.id);
            }
        }
    }

    #[test]
    fn a_command_without_a_target_becomes_no_request() {
        let snapshot = empty_snapshot();
        assert!(request(Command::NewTab, &snapshot).is_none());
        assert!(request(Command::ClosePane, &snapshot).is_none());
        assert!(request(Command::FocusLeft, &snapshot).is_none());
    }

    #[test]
    fn the_commands_name_the_focused_things() {
        let mut snapshot = empty_snapshot();
        snapshot.workspaces = vec![workspace("w1"), workspace("w2")];
        snapshot.focused_workspace_id = Some("w1".into());
        snapshot.tabs = vec![tab("a", "w1"), tab("foreign", "w2"), tab("b", "w1")];
        snapshot.focused_tab_id = Some("a".into());
        snapshot.panes = vec![pane("w1:p1", "w1", "a")];
        snapshot.focused_pane_id = Some("w1:p1".into());

        assert_eq!(
            request(Command::NewTab, &snapshot),
            Some((
                Method::TabCreate,
                json!({"workspace_id": "w1", "focus": true})
            ))
        );
        assert_eq!(
            request(Command::CloseTab, &snapshot),
            Some((Method::TabClose, json!({"tab_id": "a"})))
        );
        assert_eq!(
            request(Command::NextTab, &snapshot),
            Some((Method::TabFocus, json!({"tab_id": "b"})))
        );
        assert_eq!(
            request(Command::SplitRight, &snapshot),
            Some((
                Method::PaneSplit,
                json!({"target_pane_id": "w1:p1", "direction": "right", "focus": true})
            ))
        );
        assert_eq!(
            request(Command::FocusDown, &snapshot),
            Some((
                Method::PaneFocusDirection,
                json!({"pane_id": "w1:p1", "direction": "down"})
            ))
        );
    }

    /// Herdr's TUI counts the insert index with the moving tab still in place,
    /// and wraps a tab at either end around to the other.
    #[test]
    fn a_tab_moves_within_its_workspace_and_wraps_at_either_end() {
        let mut snapshot = empty_snapshot();
        snapshot.workspaces = vec![workspace("w1"), workspace("w2")];
        snapshot.focused_workspace_id = Some("w1".into());
        snapshot.tabs = vec![
            tab("a", "w1"),
            tab("foreign", "w2"),
            tab("b", "w1"),
            tab("c", "w1"),
        ];
        for (focused, previous, next) in [("a", 3, 2), ("b", 0, 3), ("c", 1, 0)] {
            snapshot.focused_tab_id = Some(focused.into());
            for (command, insert_index) in [
                (Command::MoveTabPrevious, previous),
                (Command::MoveTabNext, next),
            ] {
                assert_eq!(
                    request(command, &snapshot),
                    Some((
                        Method::TabMove,
                        json!({"tab_id": focused, "insert_index": insert_index}),
                    )),
                    "{focused} {command:?}"
                );
            }
        }
        snapshot.tabs = vec![tab("a", "w1")];
        snapshot.focused_tab_id = Some("a".into());
        assert!(request(Command::MoveTabNext, &snapshot).is_none());
        assert!(request(Command::MoveTabPrevious, &snapshot).is_none());
    }

    /// The palette's own label is the one the settings surface repeats;
    /// keeping the check here means a rename has one place to go wrong.
    #[test]
    fn the_palette_is_a_command_like_any_other() {
        let info = Command::TogglePalette
            .info()
            .expect("the palette has an entry");
        assert_eq!(info.id, "view.palette");
        assert_eq!(info.prefixed, &["p"]);
        assert_eq!(info.direct, &["secondary-shift-p"]);
        assert_eq!(t(&format!("command.{}", info.id)), info.label());
    }

    /// The settings dialog is reached by its own modifier chord, never from
    /// behind the prefix: the prefixed `,` is already the tab move, and a
    /// second meaning for it would make one of the two unreachable.
    #[test]
    fn the_settings_dialog_has_its_own_chord() {
        let info = Command::OpenSettings
            .info()
            .expect("the settings dialog has an entry");
        assert_eq!(info.id, "view.settings");
        assert_eq!(info.direct, &["secondary-,"]);
        assert!(
            info.prefixed.is_empty(),
            "the prefixed , belongs to the tab move"
        );
        let key = format!("command.{}", info.id);
        assert_ne!(t(&key), key, "the label resolves through the locale file");
        // And no other command claims the same direct chord.
        let claims = COMMANDS
            .iter()
            .filter(|other| other.direct.contains(&"secondary-,"))
            .count();
        assert_eq!(claims, 1);
    }

    /// The conversation panel's chord is the design's `⌘⇧A`, claimed outright
    /// beside the palette's and the settings dialog's.
    #[test]
    fn the_conversation_panel_has_its_own_chord() {
        let info = Command::ToggleAside.info().expect("the aside has an entry");
        assert_eq!(info.id, "view.toggle-chat");
        assert_eq!(info.direct, &["secondary-shift-a"]);
        assert!(
            info.prefixed.is_empty(),
            "a panel toggle claims its own modifier chord"
        );
        let key = format!("command.{}", info.id);
        assert_ne!(t(&key), key, "the label resolves through the locale file");
        let claims = COMMANDS
            .iter()
            .filter(|other| other.direct.contains(&"secondary-shift-a"))
            .count();
        assert_eq!(claims, 1);
    }

    /// The first-run screens have no snapshot to resolve against, and the
    /// daemon's own focused workspace is the source a new one copies. So the
    /// request must exist without one, and name a source only when there is a
    /// workspace to name.
    #[test]
    fn a_new_workspace_needs_no_snapshot_and_names_its_source_when_it_has_one() {
        let (method, params) = new_workspace(None);
        assert_eq!(method, Method::WorkspaceCreate);
        assert_eq!(params, json!({"focus": true}));

        let mut snapshot = empty_snapshot();
        snapshot.focused_workspace_id = Some("w1".into());
        snapshot.workspaces.push(workspace("w1"));
        let (_, params) = new_workspace(Some(&snapshot));
        assert_eq!(params["focus"], json!(true));
        assert_eq!(params["source_workspace_id"], json!("w1"));
    }
}
