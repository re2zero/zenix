//! The shell's own contributions: the sidebar's workspace panel and the
//! status bar's segments.
//!
//! They are registered in [`super::registry`] rather than reached by name
//! from the shell's render, so replacing this file with a plugin's would not
//! change how the shell mounts them. The layout is skeleton level by decision
//! (D3): a workspace's name, its cwd and git branch, and its agent's status.

use std::path::{Path, PathBuf};

use gpui_kit::component::{ActiveTheme, Theme, h_flex, v_flex};
use gpui_kit::*;
use herdr_client::protocol::{
    AgentStatus, ClientShellAgent, ClientShellSnapshot, ClientShellWorkspace,
};

use super::registry::{
    Invocation, ShellRequest, SidebarBlock, SidebarView, StatusAlign, StatusSegment,
};
use super::reorder::{self, WorkspaceDrag};
use crate::herdr::ConnectionStatus;
use crate::i18n::t;
use crate::layout::{DIR, FILE_NAME, LayoutState, Template};
use crate::plugins::panels::{header, line};

/// The sidebar's blocks, each naming the rail view it belongs to.
pub(crate) const PANELS: &[SidebarBlock] = &[
    SidebarBlock {
        view: SidebarView::Workspaces,
        render: render_workspaces,
    },
    SidebarBlock {
        view: SidebarView::Agents,
        render: render_agents,
    },
    SidebarBlock {
        view: SidebarView::Layout,
        render: render_layout,
    },
];

/// The status bar's segments, per side and left to right (PLAN.md §M11a): what
/// the session is opens the bar, what the client thinks closes it.
pub(crate) const STATUS_SEGMENTS: &[StatusSegment] = &[
    StatusSegment {
        align: StatusAlign::Start,
        render: render_connection,
    },
    StatusSegment {
        align: StatusAlign::Start,
        render: render_counts,
    },
    StatusSegment {
        align: StatusAlign::Start,
        render: render_grid,
    },
    StatusSegment {
        align: StatusAlign::Start,
        render: render_agent_counts,
    },
    StatusSegment {
        align: StatusAlign::End,
        render: render_config,
    },
    StatusSegment {
        align: StatusAlign::End,
        render: render_rejection,
    },
    StatusSegment {
        align: StatusAlign::End,
        render: render_branch,
    },
    StatusSegment {
        align: StatusAlign::End,
        render: render_prefix,
    },
];

/// A view with nothing to show still names itself: the rail can switch to an
/// empty view, and a blank column reads as a broken one rather than an empty
/// one. Shared with the plugin host's blocks for the same reason.
pub(crate) fn empty_view(title: String, body: String, theme: &Theme) -> AnyElement {
    v_flex()
        .w_full()
        .px_2()
        .py_1()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(title),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(body),
        )
        .into_any_element()
}

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
            .child(t("shell.workspaces")),
    );
    let mut count = 0;
    if let Some(snapshot) = invocation.snapshot {
        count = snapshot.workspaces.len();
        column = column.children(
            snapshot
                .workspaces
                .iter()
                .enumerate()
                .map(|(index, workspace)| workspace_row(index, workspace, snapshot, invocation)),
        );
    }
    // The column is the gap after the last row: a drop no row claimed lands
    // here and appends, which is how a workspace reaches the end.
    let dispatch = invocation.dispatch.clone();
    column
        .on_drop(move |drag: &WorkspaceDrag, window, cx| {
            if let Some(insert_index) = reorder::gap_at_end(drag.from, count) {
                dispatch(
                    ShellRequest::MoveWorkspace {
                        workspace_id: drag.workspace_id.clone(),
                        insert_index,
                    },
                    window,
                    cx,
                );
            }
        })
        .into_any_element()
}

fn workspace_row(
    index: usize,
    workspace: &ClientShellWorkspace,
    snapshot: &ClientShellSnapshot,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let drop = invocation.dispatch.clone();
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
        // A row carried onto another lands in the gap before it; the gaps
        // around its own place change nothing.
        .on_drag(
            WorkspaceDrag {
                workspace_id: workspace.workspace_id.clone(),
                from: index,
            },
            |_, _, _, cx| cx.new(|_| EmptyView),
        )
        .on_drop(move |drag: &WorkspaceDrag, window, cx| {
            if let Some(insert_index) = reorder::gap_before(drag.from, index) {
                drop(
                    ShellRequest::MoveWorkspace {
                        workspace_id: drag.workspace_id.clone(),
                        insert_index,
                    },
                    window,
                    cx,
                );
            }
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
        return empty_view(
            t("ai.agent.monitor_title"),
            t("shell.views.empty.agents"),
            theme,
        );
    };
    let agents = ordered_agents(snapshot);
    if agents.is_empty() {
        return empty_view(
            t("ai.agent.monitor_title"),
            t("shell.views.empty.agents"),
            theme,
        );
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
pub(crate) fn ordered_agents(snapshot: &ClientShellSnapshot) -> Vec<&ClientShellAgent> {
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

/// The project the focused pane belongs to and the template found in it. The block
/// speaks only once a project is known, which is after the first snapshot has named
/// a focused pane. Saving and restoring are both clicks: an apply always makes a new
/// tab, so the block must not run one on its own.
fn render_layout(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let layout = invocation.layout;
    let Some(root) = layout.root.as_ref() else {
        return div().into_any_element();
    };
    let dispatch = invocation.dispatch.clone();
    let path = layout_path(layout, root);
    let mut column = v_flex().w_full().px_1().py_1().gap_0p5().child(header(
        "layout-header",
        &t("layout.title"),
        &t("layout.save"),
        ShellRequest::SaveLayout,
        None,
        invocation,
    ));
    let row = h_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .justify_between()
        .child(line(&path.display().to_string(), theme.muted_foreground));
    column = column.child(if layout.template.is_some() {
        row.child(
            div()
                .id("layout-restore")
                .text_xs()
                .text_color(theme.muted_foreground)
                .cursor_pointer()
                .on_click(move |_, window, cx| dispatch(ShellRequest::RestoreLayout, window, cx))
                .child(t("layout.restore")),
        )
    } else {
        row
    });
    if let Some(template) = layout.template.as_ref() {
        column = column.child(line(&layout_detail(template), theme.muted_foreground));
    } else {
        column = column.child(line(&t("layout.no_template"), theme.muted_foreground));
    }
    if let Some(last) = layout.last.as_deref() {
        column = column.child(line(last, theme.success));
    }
    if let Some(error) = layout.error.as_deref() {
        column = column.child(line(error, theme.danger));
    }
    column.into_any_element()
}

/// The file the block names: the one found by walking up from the pane, or the one
/// saving would create when the project has none.
fn layout_path(layout: &LayoutState, root: &Path) -> PathBuf {
    layout
        .path
        .clone()
        .unwrap_or_else(|| root.join(DIR).join(FILE_NAME))
}

/// How many panes the template would create, and the tab name it carries when it
/// carries one. An export cannot carry a name, so this is the file's own field.
fn layout_detail(template: &Template) -> String {
    let panes = rust_i18n::t!("layout.panes", panes = template.root.panes()).to_string();
    match template.tab_label.as_deref() {
        Some(label) => format!("{panes} · {label}"),
        None => panes,
    }
}

/// What the connection is, as the (text, color) the status bar and the title
/// bar both show. Each surface paints the pair its own way; the meaning is
/// decided once, here.
pub(crate) fn connection_indication(
    status: ConnectionStatus,
    error: Option<&str>,
    theme: &Theme,
) -> (String, Hsla) {
    match (status, error) {
        (ConnectionStatus::Disconnected, Some(error)) => (
            rust_i18n::t!("status.disconnected", reason = error).to_string(),
            theme.danger,
        ),
        // The key reads "Disconnected: %{reason}", so it needs a reason. The
        // daemon always carries one when it drops the connection
        // (`state::apply`'s Disconnected arm), so this is the fallback for the
        // case where there is nothing to name.
        (ConnectionStatus::Disconnected, None) => (t("status.no_reason"), theme.danger),
        (ConnectionStatus::Connecting, _) => (t("status.connecting"), theme.muted_foreground),
        (ConnectionStatus::AwaitingSnapshot, _) => {
            (t("status.awaiting_snapshot"), theme.muted_foreground)
        }
        (ConnectionStatus::Connected, _) => (t("status.connected"), theme.success),
    }
}

fn render_connection(invocation: &Invocation, _: &mut Window, _: &mut App) -> Option<AnyElement> {
    let theme = invocation.theme;
    let (label, color) = connection_indication(invocation.status, invocation.status_error, theme);
    Some(
        h_flex()
            .gap_1p5()
            .items_center()
            .child(div().size(px(6.)).rounded_full().bg(color))
            .child(div().text_xs().text_color(color).child(label))
            .into_any_element(),
    )
}

/// What the session holds, the way the prototype reads it: a count per thing
/// the daemon publishes, so the bar says how big the session is without opening
/// a panel.
fn render_counts(invocation: &Invocation, _: &mut Window, _: &mut App) -> Option<AnyElement> {
    let theme = invocation.theme;
    let snapshot = invocation.snapshot?;
    let label = rust_i18n::t!(
        "status.counts",
        workspaces = counted(
            snapshot.workspaces.len(),
            "status.count.ws",
            "status.count.ws"
        ),
        tabs = counted(
            snapshot.tabs.len(),
            "status.count.tab_one",
            "status.count.tab_many"
        ),
        panes = counted(
            snapshot.panes.len(),
            "status.count.pane_one",
            "status.count.pane_many"
        ),
    )
    .to_string();
    Some(
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(label)
            .into_any_element(),
    )
}

/// A count and its unit. rust-i18n 4 has no plural forms, so the singular is a
/// key of its own and the choice is made here.
fn counted(count: usize, one: &str, many: &str) -> String {
    rust_i18n::t!(if count == 1 { one } else { many }, count = count).to_string()
}

/// The config file's problems, so a hand-edited value that had no effect is
/// visible without opening the settings surface. The Config page of the
/// settings panel lists them one by one.
fn render_config(_: &Invocation, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let theme = cx.theme().clone();
    let diagnostics = crate::config::diagnostics(cx);
    if diagnostics.is_empty() {
        return None;
    }
    let color = if diagnostics.iter().any(|item| item.is_blocking()) {
        theme.danger
    } else {
        theme.warning
    };
    Some(
        div()
            .text_xs()
            .text_color(color)
            .child(
                rust_i18n::t!("settings.config.problems_short", count = diagnostics.len())
                    .to_string(),
            )
            .into_any_element(),
    )
}

fn render_grid(invocation: &Invocation, _: &mut Window, _: &mut App) -> Option<AnyElement> {
    let theme = invocation.theme;
    let surface = invocation.surface?;
    Some(
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(format!(
                "{}x{} cells",
                surface.frame.width, surface.frame.height
            ))
            .into_any_element(),
    )
}

/// What the agents the fleet stream is watching are doing. The counts come
/// from the same transitions the cockpit draws, so the bar and the fleet panel
/// cannot disagree; a client that has watched no status yet says nothing.
fn render_agent_counts(invocation: &Invocation, _: &mut Window, _: &mut App) -> Option<AnyElement> {
    let theme = invocation.theme;
    if invocation.fleet.current.is_empty() {
        return None;
    }
    let running = invocation.fleet.count("running");
    let blocked = invocation.fleet.count("blocked");
    Some(
        h_flex()
            .gap_1p5()
            .items_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(rust_i18n::t!("status.agents", running = running, blocked = blocked).to_string())
            .into_any_element(),
    )
}

/// Why the daemon refused the last command, so a command that needs a
/// confirmation it cannot ask for is not simply dropped.
fn render_rejection(invocation: &Invocation, _: &mut Window, _: &mut App) -> Option<AnyElement> {
    let theme = invocation.theme;
    let reason = invocation.rejection?;
    Some(
        div()
            .text_xs()
            .text_color(theme.danger)
            .child(reason.to_owned())
            .into_any_element(),
    )
}

/// The focused workspace's branch, so the bar names the checkout the grid is
/// working in without repeating the sidebar's whole detail line.
fn render_branch(invocation: &Invocation, _: &mut Window, _: &mut App) -> Option<AnyElement> {
    let theme = invocation.theme;
    let branch = focused_branch(invocation.snapshot?)?;
    Some(
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(branch.to_owned())
            .into_any_element(),
    )
}

/// The focused workspace's branch. A workspace that is not a checkout has none,
/// and neither has a snapshot with no focused workspace.
fn focused_branch(snapshot: &ClientShellSnapshot) -> Option<&str> {
    let id = snapshot.focused_workspace_id.as_deref()?;
    snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == id)
        .and_then(|workspace| workspace.branch.as_deref())
}

/// The armed prefix, so a chord's first key is visible while it waits.
fn render_prefix(invocation: &Invocation, _: &mut Window, _: &mut App) -> Option<AnyElement> {
    let theme = invocation.theme;
    let label = invocation.prefix?;
    Some(
        div()
            .px_1()
            .rounded_sm()
            .text_xs()
            .bg(theme.accent)
            .text_color(theme.accent_foreground)
            .child(label.to_owned())
            .into_any_element(),
    )
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
    fn the_status_bar_names_the_focused_workspaces_branch() {
        assert_eq!(focused_branch(&snapshot_with_panes()), Some("main"));
        // A workspace that is not a checkout has no branch to name, so the
        // segment stays off the bar rather than showing an empty cell.
        let mut snapshot = snapshot_with_panes();
        snapshot.workspaces[0].branch = None;
        assert_eq!(focused_branch(&snapshot), None);
        snapshot.focused_workspace_id = None;
        assert_eq!(focused_branch(&snapshot), None);
    }

    #[test]
    fn a_status_count_inflects_its_unit() {
        assert_eq!(
            counted(1, "status.count.tab_one", "status.count.tab_many"),
            "1 tab"
        );
        assert_eq!(
            counted(3, "status.count.tab_one", "status.count.tab_many"),
            "3 tabs"
        );
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

    #[test]
    fn the_block_names_the_file_saving_would_create_when_the_project_has_none() {
        let project = Path::new("/tmp/zenix-layout-project");
        let state = LayoutState {
            root: Some(project.to_path_buf()),
            ..LayoutState::default()
        };
        assert_eq!(
            layout_path(&state, project),
            project.join(DIR).join(FILE_NAME),
            "the file a save would create"
        );
        // A template found further up than the project is the one the block names.
        let state = LayoutState {
            root: Some(project.to_path_buf()),
            path: Some(Path::new("/tmp/zenix-layout-project/.zenix/layout.toml").to_path_buf()),
            ..LayoutState::default()
        };
        assert_eq!(
            layout_path(&state, project),
            Path::new("/tmp/zenix-layout-project/.zenix/layout.toml").to_path_buf()
        );
    }

    #[test]
    fn the_layout_detail_names_the_tab_only_when_the_template_carries_one() {
        let template: Template =
            toml::from_str("[root]\ntype = \"pane\"\n").expect("a one-pane template");
        let detail = layout_detail(&template);
        assert!(!detail.contains("zenix"), "{detail}");
        let named = Template {
            tab_label: Some("zenix".to_owned()),
            ..template.clone()
        };
        assert!(layout_detail(&named).contains("zenix"), "{named:?}");
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
