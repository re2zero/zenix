//! The shell's command directory and the daemon methods its commands become.
//!
//! Ported in shape from herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/controls.rs`.
//! The directory is data: the keymap, the sidebar and the contributions all
//! read this one table, so a shortcut cannot drift from what it runs. A
//! command names no id of its own; the target is resolved from the snapshot
//! when it runs, so a closed tab or workspace cannot be addressed twice.

use herdr_client::Method;
use herdr_client::protocol::{
    ClientShellPane, ClientShellSnapshot, ClientShellTab, ClientShellWorkspace,
};
use serde_json::{Value, json};

/// A command the shell can run, from the keymap, the sidebar or a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Deserialize)]
pub(crate) enum Command {
    NewWorkspace,
    CloseWorkspace,
    NextWorkspace,
    PreviousWorkspace,
    WorkspaceNumber(u8),
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    MoveTabPrevious,
    MoveTabNext,
    TabNumber(u8),
    SplitRight,
    SplitDown,
    ClosePane,
    Zoom,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    ResizeLeft,
    ResizeRight,
    ResizeUp,
    ResizeDown,
    /// Local: the sidebar's own visibility.
    ToggleSidebar,
    /// Local: the right-hand panel's visibility.
    ToggleAside,
    /// Local: light or dark chrome.
    ToggleTheme,
    /// Local: the settings surface.
    ToggleSettings,
    /// Local: the command palette, over the shell.
    TogglePalette,
    /// Local: re-read the plugin registry, which the daemon does not push.
    RefreshPlugins,
    /// Local: read a plugin directory before linking it.
    LinkPlugin,
    /// Local: dismiss the daemon's popup pane, if one is open.
    ClosePopup,
    /// Local: write the focused tab's layout to the project's template file.
    SaveLayout,
    /// Local: restore the project's template as a new tab.
    RestoreLayout,
}

impl Command {
    /// Whether the shell runs this itself rather than sending it to the
    /// daemon. The directory owns the answer so a chord and a click on a
    /// contribution cannot disagree about where a command goes.
    pub(crate) fn is_local(self) -> bool {
        matches!(
            self,
            Command::ToggleSidebar
                | Command::ToggleAside
                | Command::ToggleTheme
                | Command::ToggleSettings
                | Command::TogglePalette
                | Command::RefreshPlugins
                | Command::LinkPlugin
                | Command::ClosePopup
                | Command::SaveLayout
                | Command::RestoreLayout
        )
    }
}

/// One command's entry in the directory: what it is called, and the keystrokes
/// that reach it.
pub(crate) struct CommandInfo {
    pub(crate) command: Command,
    /// Stable id for menus, the palette and plugin contributions.
    pub(crate) id: &'static str,
    /// Keystrokes bound directly through GPUI's keymap.
    pub(crate) direct: &'static [&'static str],
    /// Keys typed after the prefix.
    pub(crate) prefixed: &'static [&'static str],
}

impl CommandInfo {
    /// What the command is called, read from the `command` section of the
    /// locale file, which is keyed by the command's own id. Resolved when a row
    /// is drawn rather than stored in the table, so the same table can be read
    /// in either language and the directory keeps one name for a command.
    pub(crate) fn label(&self) -> String {
        crate::i18n::t(&format!("command.{}", self.id))
    }
}

/// Every command the shell mounts, in the order a list would show them.
pub(crate) const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        command: Command::NewWorkspace,
        id: "workspace.new",
        direct: &["cmd-shift-n"],
        prefixed: &["shift-n"],
    },
    CommandInfo {
        command: Command::CloseWorkspace,
        id: "workspace.close",
        direct: &[],
        prefixed: &["shift-w"],
    },
    CommandInfo {
        command: Command::NextWorkspace,
        id: "workspace.next",
        direct: &[],
        prefixed: &["n"],
    },
    CommandInfo {
        command: Command::PreviousWorkspace,
        id: "workspace.previous",
        direct: &[],
        prefixed: &["p"],
    },
    CommandInfo {
        command: Command::NewTab,
        id: "tab.new",
        direct: &["cmd-t"],
        prefixed: &["t"],
    },
    CommandInfo {
        command: Command::CloseTab,
        id: "tab.close",
        direct: &["cmd-shift-w"],
        prefixed: &["shift-t"],
    },
    CommandInfo {
        command: Command::NextTab,
        id: "tab.next",
        direct: &["cmd-shift-]"],
        prefixed: &["]"],
    },
    CommandInfo {
        command: Command::PreviousTab,
        id: "tab.previous",
        direct: &["cmd-shift-["],
        prefixed: &["["],
    },
    // Upstream binds these only to a tab dragged along its strip and leaves the
    // keys empty; zenix has no tab drag yet, so a pair of chords takes over the
    // job it does there. They are unshifted keys on purpose: a shifted symbol
    // arrives as the symbol itself (see `terminal::key_code`), so only a letter
    // or an unshifted key can be matched by `keymap::matches`.
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
        direct: &["cmd-d"],
        prefixed: &["s"],
    },
    CommandInfo {
        command: Command::SplitDown,
        id: "pane.split-down",
        direct: &["cmd-shift-d"],
        prefixed: &["v"],
    },
    CommandInfo {
        command: Command::ClosePane,
        id: "pane.close",
        direct: &["cmd-w"],
        prefixed: &["x"],
    },
    CommandInfo {
        command: Command::Zoom,
        id: "pane.zoom",
        direct: &["cmd-shift-enter"],
        prefixed: &["z"],
    },
    CommandInfo {
        command: Command::FocusLeft,
        id: "pane.focus-left",
        direct: &["cmd-alt-left"],
        prefixed: &["h"],
    },
    CommandInfo {
        command: Command::FocusRight,
        id: "pane.focus-right",
        direct: &["cmd-alt-right"],
        prefixed: &["l"],
    },
    CommandInfo {
        command: Command::FocusUp,
        id: "pane.focus-up",
        direct: &["cmd-alt-up"],
        prefixed: &["k"],
    },
    CommandInfo {
        command: Command::FocusDown,
        id: "pane.focus-down",
        direct: &["cmd-alt-down"],
        prefixed: &["j"],
    },
    CommandInfo {
        command: Command::ResizeLeft,
        id: "pane.resize-left",
        direct: &[],
        prefixed: &["shift-h"],
    },
    CommandInfo {
        command: Command::ResizeRight,
        id: "pane.resize-right",
        direct: &[],
        prefixed: &["shift-l"],
    },
    CommandInfo {
        command: Command::ResizeUp,
        id: "pane.resize-up",
        direct: &[],
        prefixed: &["shift-k"],
    },
    CommandInfo {
        command: Command::ResizeDown,
        id: "pane.resize-down",
        direct: &[],
        prefixed: &["shift-j"],
    },
    CommandInfo {
        command: Command::ToggleSidebar,
        id: "view.toggle-sidebar",
        direct: &["cmd-b"],
        prefixed: &["b"],
    },
    CommandInfo {
        command: Command::ToggleAside,
        id: "view.toggle-chat",
        direct: &[],
        prefixed: &["a"],
    },
    CommandInfo {
        command: Command::ToggleTheme,
        id: "view.toggle-theme",
        direct: &["cmd-shift-l"],
        prefixed: &[],
    },
    CommandInfo {
        command: Command::ToggleSettings,
        id: "view.settings",
        direct: &["cmd-,"],
        prefixed: &[],
    },
    // The palette is a view of this same directory plus the daemon's plugin
    // actions, so it is a command like any other and reaches itself (a second
    // press closes it).
    CommandInfo {
        command: Command::TogglePalette,
        id: "view.palette",
        direct: &["cmd-shift-p"],
        prefixed: &[],
    },
    // The plugin surface, which nothing pushes: a plugin linked in a shell
    // appears here only after this reads the registry again.
    CommandInfo {
        command: Command::RefreshPlugins,
        id: "plugins.refresh",
        direct: &[],
        prefixed: &[],
    },
    // The review is a view of the shell's own, like the palette: linking a
    // directory runs commands as the user, so what it would run is read first.
    CommandInfo {
        command: Command::LinkPlugin,
        id: "plugins.link",
        direct: &[],
        prefixed: &[],
    },
    CommandInfo {
        command: Command::ClosePopup,
        id: "view.close-popup",
        direct: &[],
        prefixed: &[],
    },
    // The project's layout template. Saving reads the focused tab; restoring is
    // deliberately not automatic, because `layout.apply` always makes a new tab.
    CommandInfo {
        command: Command::SaveLayout,
        id: "layout.save",
        direct: &[],
        prefixed: &[],
    },
    CommandInfo {
        command: Command::RestoreLayout,
        id: "layout.restore",
        direct: &[],
        prefixed: &[],
    },
];

/// The daemon method a command becomes, or `None` when the shell runs it
/// itself (a local view toggle) or the snapshot cannot name its target.
pub(crate) fn request(command: Command, snapshot: &ClientShellSnapshot) -> Option<(Method, Value)> {
    let workspace = focused_workspace(snapshot);
    let tab = focused_tab(snapshot);
    let pane = focused_pane(snapshot);
    Some(match command {
        Command::NewWorkspace => {
            let mut params = json!({"focus": true});
            if let Some(workspace) = workspace {
                params["source_workspace_id"] = json!(workspace.workspace_id);
            }
            (Method::WorkspaceCreate, params)
        }
        // No confirmation dialog yet, so a workspace the daemon wants
        // confirmed (a worktree group) is refused and the reason is shown in
        // the status bar instead of being dropped.
        Command::CloseWorkspace => (
            Method::WorkspaceClose,
            json!({"workspace_id": workspace?.workspace_id, "close_group": false}),
        ),
        Command::NextWorkspace | Command::PreviousWorkspace => {
            let workspace = workspace?;
            let workspaces = &snapshot.workspaces;
            let index = workspaces
                .iter()
                .position(|w| w.workspace_id == workspace.workspace_id)?;
            let next = if command == Command::NextWorkspace {
                (index + 1) % workspaces.len()
            } else {
                (index + workspaces.len() - 1) % workspaces.len()
            };
            (
                Method::WorkspaceFocus,
                json!({"workspace_id": workspaces[next].workspace_id}),
            )
        }
        // By position, as the daemon's own switch does: a workspace's number
        // is an id that is never reused, so it drifts from the sidebar once
        // one closes.
        Command::WorkspaceNumber(number) => {
            let target = snapshot
                .workspaces
                .get(usize::from(number).checked_sub(1)?)?;
            (
                Method::WorkspaceFocus,
                json!({"workspace_id": target.workspace_id}),
            )
        }
        Command::NewTab => (
            Method::TabCreate,
            json!({"workspace_id": workspace?.workspace_id, "focus": true}),
        ),
        Command::CloseTab => (Method::TabClose, json!({"tab_id": tab?.tab_id})),
        Command::NextTab | Command::PreviousTab => {
            let workspace = &workspace?.workspace_id;
            let tabs: Vec<_> = snapshot
                .tabs
                .iter()
                .filter(|t| &t.workspace_id == workspace)
                .collect();
            let index = tabs
                .iter()
                .position(|t| Some(&t.tab_id) == snapshot.focused_tab_id.as_ref())?;
            let next = if command == Command::NextTab {
                (index + 1) % tabs.len()
            } else {
                (index + tabs.len() - 1) % tabs.len()
            };
            (Method::TabFocus, json!({"tab_id": tabs[next].tab_id}))
        }
        // As Herdr's TUI moves it: the index counts the moving tab still in
        // place, and a tab at either end wraps around to the other.
        Command::MoveTabPrevious | Command::MoveTabNext => {
            let tab = tab?;
            let tabs: Vec<_> = snapshot
                .tabs
                .iter()
                .filter(|t| t.workspace_id == tab.workspace_id)
                .collect();
            if tabs.len() <= 1 {
                return None;
            }
            let source = tabs.iter().position(|t| t.tab_id == tab.tab_id)?;
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
        Command::TabNumber(number) => {
            let workspace = workspace?;
            let target = snapshot
                .tabs
                .iter()
                .filter(|t| t.workspace_id == workspace.workspace_id)
                .nth(usize::from(number).checked_sub(1)?)?;
            (Method::TabFocus, json!({"tab_id": target.tab_id}))
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
        Command::Zoom => (
            Method::PaneZoom,
            json!({"pane_id": pane?.pane_id, "mode": "toggle"}),
        ),
        Command::FocusLeft | Command::FocusRight | Command::FocusUp | Command::FocusDown => (
            Method::PaneFocusDirection,
            json!({"pane_id": pane?.pane_id, "direction": direction(command)?}),
        ),
        // The daemon nudges the split by its own default step, as its TUI does.
        Command::ResizeLeft | Command::ResizeRight | Command::ResizeUp | Command::ResizeDown => (
            Method::PaneResize,
            json!({"pane_id": pane?.pane_id, "direction": direction(command)?}),
        ),
        Command::ToggleSidebar
        | Command::ToggleAside
        | Command::ToggleTheme
        | Command::ToggleSettings
        | Command::TogglePalette
        | Command::RefreshPlugins
        | Command::LinkPlugin
        | Command::ClosePopup
        | Command::SaveLayout
        | Command::RestoreLayout => return None,
    })
}

/// The pane direction a directional command names.
fn direction(command: Command) -> Option<&'static str> {
    Some(match command {
        Command::FocusLeft | Command::ResizeLeft => "left",
        Command::FocusRight | Command::ResizeRight => "right",
        Command::FocusUp | Command::ResizeUp => "up",
        Command::FocusDown | Command::ResizeDown => "down",
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
        .find(|t| &t.tab_id == id && t.workspace_id == workspace.workspace_id)
}

fn focused_pane(snapshot: &ClientShellSnapshot) -> Option<&ClientShellPane> {
    let tab = focused_tab(snapshot)?;
    let id = snapshot.focused_pane_id.as_ref()?;
    snapshot
        .panes
        .iter()
        .find(|p| &p.pane_id == id && p.tab_id == tab.tab_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_names_both_focus_and_resize() {
        assert_eq!(direction(Command::FocusLeft), Some("left"));
        assert_eq!(direction(Command::ResizeDown), Some("down"));
        assert_eq!(direction(Command::NewTab), None);
    }

    #[test]
    fn local_commands_become_no_request() {
        let snapshot = empty_snapshot();
        assert!(request(Command::ToggleSidebar, &snapshot).is_none());
        assert!(request(Command::ToggleAside, &snapshot).is_none());
        assert!(request(Command::ToggleTheme, &snapshot).is_none());
        assert!(request(Command::ToggleSettings, &snapshot).is_none());
        assert!(request(Command::TogglePalette, &snapshot).is_none());
        assert!(request(Command::RefreshPlugins, &snapshot).is_none());
        assert!(request(Command::ClosePopup, &snapshot).is_none());
    }

    /// A command the shell runs itself must never also name a daemon method,
    /// however full the snapshot is.
    #[test]
    fn a_local_command_never_becomes_a_daemon_method() {
        let snapshot = empty_snapshot();
        let local: Vec<&str> = COMMANDS
            .iter()
            .filter(|info| info.command.is_local())
            .map(|info| info.id)
            .collect();
        assert_eq!(
            local,
            vec![
                "view.toggle-sidebar",
                "view.toggle-chat",
                "view.toggle-theme",
                "view.settings",
                "view.palette",
                "plugins.refresh",
                "plugins.link",
                "view.close-popup",
                "layout.save",
                "layout.restore"
            ]
        );
        for info in COMMANDS {
            if info.command.is_local() {
                assert!(request(info.command, &snapshot).is_none(), "{}", info.id);
            }
        }
    }

    #[test]
    fn a_command_without_a_target_becomes_no_request() {
        let snapshot = empty_snapshot();
        assert!(request(Command::NewTab, &snapshot).is_none());
        assert!(request(Command::ClosePane, &snapshot).is_none());
    }

    #[test]
    fn every_command_is_documented_once() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|info| info.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total);
    }

    #[test]
    fn direct_keystrokes_parse_and_are_unique() {
        let mut seen: Vec<String> = Vec::new();
        for info in COMMANDS {
            for keystroke in info.direct {
                assert!(
                    gpui_kit::Keystroke::parse(keystroke).is_ok(),
                    "{}: {keystroke} does not parse",
                    info.id
                );
                assert!(!seen.contains(&(*keystroke).to_owned()), "{keystroke}");
                seen.push((*keystroke).to_owned());
            }
        }
    }

    /// A label is a key the locale file answers, not a string kept in the table.
    /// `i18n::t` hands back the key verbatim when nothing is found, so a label
    /// that comes back as its own key is a command whose key was never added.
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

    fn empty_snapshot() -> ClientShellSnapshot {
        serde_json::from_value(serde_json::json!({
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

    /// Herdr's TUI counts the insert index with the moving tab still in place,
    /// and wraps a tab at either end around to the other.
    #[test]
    fn a_tab_moves_within_its_workspace_and_wraps_at_either_end() {
        let mut snapshot = empty_snapshot();
        snapshot.workspaces = vec![workspace("w1"), workspace("w2")];
        snapshot.focused_workspace_id = Some("w1".into());
        // Another workspace's tabs are in the snapshot and never count.
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
        // A lone tab has nowhere to go.
        snapshot.tabs = vec![tab("a", "w1")];
        snapshot.focused_tab_id = Some("a".into());
        assert!(request(Command::MoveTabNext, &snapshot).is_none());
        assert!(request(Command::MoveTabPrevious, &snapshot).is_none());
    }

    fn workspace(workspace_id: &str) -> ClientShellWorkspace {
        ClientShellWorkspace {
            workspace_id: workspace_id.into(),
            active_tab_id: "a".into(),
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
}
