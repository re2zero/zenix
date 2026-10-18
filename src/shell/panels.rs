//! The shell's own contributions: the sidebar's workspace panel and the
//! status bar's segments.
//!
//! They are registered in [`super::registry`] rather than reached by name
//! from the shell's render, so replacing this file with a plugin's would not
//! change how the shell mounts them. The layout is skeleton level by decision
//! (D3): a workspace's name, its cwd and git branch, and its agent's status.

use gpui_kit::component::{ActiveTheme, Theme, h_flex, v_flex};
use gpui_kit::*;
use herdr_client::protocol::{
    AgentStatus, ClientShellAgent, ClientShellSnapshot, ClientShellWorkspace,
};

use super::registry::{Builtin, Invocation, ShellRequest};
use crate::herdr::ConnectionStatus;
use crate::i18n::t;

/// The sidebar's blocks, top to bottom.
pub(crate) const PANELS: &[Builtin] = &[
    Builtin {
        id: "workspaces",
        render: render_workspaces,
    },
    Builtin {
        id: "agents",
        render: render_agents,
    },
];

/// The status bar's segments, left to right.
pub(crate) const STATUS_SEGMENTS: &[Builtin] = &[
    Builtin {
        id: "connection",
        render: render_connection,
    },
    Builtin {
        id: "config",
        render: render_config,
    },
    Builtin {
        id: "grid",
        render: render_grid,
    },
    Builtin {
        id: "prefix",
        render: render_prefix,
    },
    Builtin {
        id: "rejection",
        render: render_rejection,
    },
];

/// Every workspace the daemon knows, the focused one picked out. Selection is
/// not a local field: it is the `focused` flag the snapshot carries.
fn render_workspaces(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let mut column = v_flex().w_full().px_1().py_1().gap_0p5().child(
        div()
            .px_1()
            .pb_1()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child("Workspaces"),
    );
    if let Some(snapshot) = invocation.snapshot {
        column = column.children(
            snapshot
                .workspaces
                .iter()
                .enumerate()
                .map(|(index, workspace)| workspace_row(index, workspace, snapshot, invocation)),
        );
    }
    column.into_any_element()
}

fn workspace_row(
    index: usize,
    workspace: &ClientShellWorkspace,
    snapshot: &ClientShellSnapshot,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let workspace_id = workspace.workspace_id.clone();
    let selected = workspace.focused;
    let name = workspace.label.clone();
    let detail = workspace_detail(snapshot, workspace);
    let row = h_flex()
        .id(("workspace", index))
        .w_full()
        .px_1()
        .py_0p5()
        .gap_1()
        .items_center()
        .rounded_sm()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(
                ShellRequest::FocusWorkspace(workspace_id.clone()),
                window,
                cx,
            );
        })
        .child(status_dot(workspace.agent_status, theme))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(if selected {
                            theme.foreground
                        } else {
                            theme.sidebar_foreground
                        })
                        .child(name),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(detail),
                ),
        );
    if selected {
        row.bg(theme.selection).into_any_element()
    } else {
        row.into_any_element()
    }
}

/// A workspace's cwd and git branch, as one dim line. The cwd comes from the
/// workspace's focused pane; the daemon carries no workspace-level cwd (only
/// the default one a new workspace would start in).
fn workspace_detail(snapshot: &ClientShellSnapshot, workspace: &ClientShellWorkspace) -> String {
    let cwd = snapshot
        .panes
        .iter()
        .filter(|pane| pane.workspace_id == workspace.workspace_id)
        .min_by_key(|pane| u8::from(!pane.focused))
        .and_then(|pane| pane.foreground_cwd.as_deref().or(pane.cwd.as_deref()));
    match (cwd, workspace.branch.as_deref()) {
        (Some(cwd), Some(branch)) => format!("{cwd} · {branch}"),
        (Some(cwd), None) => cwd.to_owned(),
        (None, Some(branch)) => branch.to_owned(),
        (None, None) => String::new(),
    }
}

/// A dot for an agent's status: filled while it works, red while it waits for
/// an answer, faint while idle or unknown.
fn status_dot(status: AgentStatus, theme: &Theme) -> impl IntoElement {
    let color = match status {
        AgentStatus::Working => theme.success,
        AgentStatus::Blocked => theme.danger,
        AgentStatus::Done => theme.accent,
        AgentStatus::Idle | AgentStatus::Unknown => theme.muted_foreground,
    };
    div()
        .w(px(6.))
        .h(px(6.))
        .flex_none()
        .rounded_full()
        .bg(color)
}

/// Every agent the daemon reports, so a worker started anywhere — from the
/// shell, from the MCP orchestrator, or by herdr itself — appears here without
/// polling: the list is built from the pushed snapshot. Clicking a row focuses
/// its pane.
fn render_agents(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let Some(snapshot) = invocation.snapshot else {
        return div().into_any_element();
    };
    let agents = ordered_agents(snapshot);
    if agents.is_empty() {
        return div().into_any_element();
    }
    let column = v_flex()
        .w_full()
        .px_1()
        .py_1()
        .gap_0p5()
        .child(
            div()
                .px_1()
                .pb_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(t("ai.agent.monitor_title")),
        )
        .children(
            agents
                .into_iter()
                .enumerate()
                .map(|(index, agent)| agent_row(index, agent, invocation)),
        );
    column.into_any_element()
}

fn agent_row(index: usize, agent: &ClientShellAgent, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let pane_id = agent.pane_id.clone();
    let focused = agent.focused;
    let name = agent
        .name
        .clone()
        .or_else(|| agent.display_agent.clone())
        .or_else(|| agent.agent.clone())
        .unwrap_or_else(|| agent.pane_id.clone());
    let detail = agent_detail(agent);
    let row = h_flex()
        .id(("agent", index))
        .w_full()
        .px_1()
        .py_0p5()
        .gap_1()
        .items_center()
        .rounded_sm()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::FocusPane(pane_id.clone()), window, cx);
        })
        .child(status_dot(agent.agent_status, theme))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(if focused {
                            theme.foreground
                        } else {
                            theme.sidebar_foreground
                        })
                        .child(name),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(detail),
                ),
        );
    if focused {
        row.bg(theme.selection).into_any_element()
    } else {
        row.into_any_element()
    }
}

/// Agents in the daemon's order. `agent_order` is a pane-id list that only
/// carries meaning while an agent view is active; without one the snapshot's
/// own order stands.
fn ordered_agents(snapshot: &ClientShellSnapshot) -> Vec<&ClientShellAgent> {
    if snapshot.agent_view_label.is_some() {
        let ordered: Vec<&ClientShellAgent> = snapshot
            .agent_order
            .iter()
            .filter_map(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .find(|agent| &agent.pane_id == pane_id)
            })
            .collect();
        if !ordered.is_empty() {
            return ordered;
        }
    }
    snapshot.agents.iter().collect()
}

/// What an agent is doing, as one dim line: its pane title when the daemon has
/// one, otherwise its status beside the workspace it lives in.
fn agent_detail(agent: &ClientShellAgent) -> String {
    let title = agent
        .terminal_title_stripped
        .as_deref()
        .or(agent.title.as_deref())
        .filter(|title| !title.is_empty());
    match title {
        Some(title) => title.to_owned(),
        None => format!(
            "{} · {}",
            status_label(agent.agent_status),
            agent.workspace_id
        ),
    }
}

fn status_label(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Idle => "idle",
        AgentStatus::Unknown => "unknown",
    }
}

fn render_connection(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let (label, color) = match (invocation.status, invocation.status_error) {
        (ConnectionStatus::Disconnected, Some(error)) => (
            rust_i18n::t!("status.disconnected", reason = error).to_string(),
            theme.danger,
        ),
        // The key reads "Disconnected: %{reason}", so it needs a reason. The
        // daemon always carries one when it drops the connection
        // (`state::apply`'s Disconnected arm), so this is the fallback for the
        // case where there is nothing to name.
        (ConnectionStatus::Disconnected, None) => ("disconnected".to_owned(), theme.danger),
        (ConnectionStatus::Connecting, _) => (
            "connecting to the herdr daemon...".to_owned(),
            theme.muted_foreground,
        ),
        (ConnectionStatus::AwaitingSnapshot, _) => (
            "connected; waiting for snapshot".to_owned(),
            theme.muted_foreground,
        ),
        (ConnectionStatus::Connected, _) => (t("status.connected"), theme.success),
    };
    div()
        .text_xs()
        .text_color(color)
        .child(label)
        .into_any_element()
}

/// The config file's problems, so a hand-edited value that had no effect is
/// visible without opening the settings surface. The Config page of the
/// settings panel lists them one by one.
fn render_config(_: &Invocation, _: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme().clone();
    let diagnostics = crate::config::diagnostics(cx);
    if diagnostics.is_empty() {
        return div().into_any_element();
    }
    let color = if diagnostics.iter().any(|item| item.is_blocking()) {
        theme.danger
    } else {
        theme.warning
    };
    div()
        .text_xs()
        .text_color(color)
        .child(
            rust_i18n::t!("settings.config.problems_short", count = diagnostics.len()).to_string(),
        )
        .into_any_element()
}

fn render_grid(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let label = invocation
        .surface
        .as_ref()
        .map(|surface| format!("{}x{} cells", surface.frame.width, surface.frame.height))
        .unwrap_or_default();
    div()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(label)
        .into_any_element()
}

/// Why the daemon refused the last command, so a command that needs a
/// confirmation it cannot ask for is not simply dropped.
fn render_rejection(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    match invocation.rejection {
        Some(reason) => div()
            .text_xs()
            .text_color(theme.danger)
            .child(reason.to_owned())
            .into_any_element(),
        None => div().into_any_element(),
    }
}

/// The armed prefix, so a chord's first key is visible while it waits.
fn render_prefix(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    match invocation.prefix {
        Some(label) => div()
            .px_1()
            .rounded_sm()
            .text_xs()
            .bg(theme.accent)
            .text_color(theme.accent_foreground)
            .child(label.to_owned())
            .into_any_element(),
        None => div().into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // `use super::*` re-exports GPUI's `test` attribute macro, which would
    // otherwise shadow the builtin one and recurse: name the builtin.
    use std::prelude::v1::test;

    use herdr_client::protocol::ClientShellSnapshot;

    #[test]
    fn the_detail_line_pairs_a_pane_cwd_with_the_branch() {
        let snapshot = snapshot_with_panes();
        let workspace = &snapshot.workspaces[0];
        assert_eq!(workspace_detail(&snapshot, workspace), "/tmp/work · main");
    }

    #[test]
    fn the_detail_line_prefers_the_focused_pane_cwd() {
        let snapshot = snapshot_with_panes();
        let focused = snapshot
            .panes
            .iter()
            .find(|pane| pane.focused)
            .expect("a focused pane");
        assert!(focused.cwd.as_deref() == Some("/tmp/work"));
    }

    #[test]
    fn a_workspace_without_a_branch_shows_only_its_cwd() {
        let mut snapshot = snapshot_with_panes();
        snapshot.workspaces[0].branch = None;
        let workspace = &snapshot.workspaces[0];
        assert_eq!(workspace_detail(&snapshot, workspace), "/tmp/work");
    }

    #[test]
    fn agents_follow_the_daemons_order_while_an_agent_view_is_active() {
        let mut snapshot = snapshot_with_agents();
        snapshot.agent_view_label = Some("working".into());
        snapshot.agent_order = vec!["w1:p9".into(), "w1:p2".into()];
        let ordered: Vec<&str> = ordered_agents(&snapshot)
            .iter()
            .map(|agent| agent.pane_id.as_str())
            .collect();
        assert_eq!(ordered, ["w1:p9", "w1:p2"]);
    }

    #[test]
    fn agents_keep_the_snapshot_order_without_an_agent_view() {
        let snapshot = snapshot_with_agents();
        let ordered: Vec<&str> = ordered_agents(&snapshot)
            .iter()
            .map(|agent| agent.pane_id.as_str())
            .collect();
        assert_eq!(ordered, ["w1:p2", "w1:p9"]);
    }

    #[test]
    fn an_agent_view_order_that_names_no_agent_falls_back_to_the_snapshot() {
        let mut snapshot = snapshot_with_agents();
        snapshot.agent_view_label = Some("gone".into());
        snapshot.agent_order = vec!["w9:p1".into()];
        assert_eq!(ordered_agents(&snapshot).len(), 2);
    }

    #[test]
    fn an_agent_detail_prefers_the_pane_title_over_its_status() {
        let snapshot = snapshot_with_agents();
        assert_eq!(agent_detail(&snapshot.agents[0]), "build · passing");
    }

    #[test]
    fn an_agent_without_a_title_shows_its_status_and_workspace() {
        let mut snapshot = snapshot_with_agents();
        snapshot.agents[0].terminal_title_stripped = None;
        snapshot.agents[0].title = None;
        assert_eq!(agent_detail(&snapshot.agents[0]), "working · w1");
    }

    fn snapshot_with_agents() -> ClientShellSnapshot {
        let mut snapshot = snapshot_with_panes();
        snapshot.agents = serde_json::from_value(serde_json::json!([
            {
                "pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1",
                "name": "oc-check", "terminal_title_stripped": "build · passing",
                "agent_status": "working", "state_change_seq": 2, "focused": true,
                "state_labels": [], "tokens": []
            },
            {
                "pane_id": "w1:p9", "workspace_id": "w1", "tab_id": "w1:t1",
                "name": "codex-review", "agent_status": "idle",
                "state_change_seq": 1, "focused": false,
                "state_labels": [], "tokens": []
            }
        ]))
        .expect("the fixture agents deserialize");
        snapshot
    }

    fn snapshot_with_panes() -> ClientShellSnapshot {
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
            "focused_workspace_id": "w1",
            "focused_tab_id": "w1:t1",
            "focused_pane_id": "w1:p2",
            "workspaces": [{
                "workspace_id": "w1",
                "active_tab_id": "w1:t1",
                "new_workspace_cwd": "/tmp/work",
                "number": 1,
                "label": "work",
                "custom_label": false,
                "branch": "main",
                "tokens": [],
                "focused": true,
                "agent_status": "working",
            }],
            "tabs": [],
            "panes": [
                {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1",
                 "cwd": "/tmp/other", "focused": false, "right_click_passthrough": false},
                {"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1",
                 "cwd": "/tmp/work", "focused": true, "right_click_passthrough": false},
            ],
            "agents": [],
            "commands": [],
        }))
        .expect("the fixture snapshot deserializes")
    }
}
