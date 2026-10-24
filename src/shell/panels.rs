//! The shell's own contributions: the sidebar's workspace panel and the
//! status bar's segments.
//!
//! They are registered in [`super::registry`] rather than reached by name
//! from the shell's render, so replacing this file with a plugin's would not
//! change how the shell mounts them. The layout is skeleton level by decision
//! (D3): a workspace's name, its cwd and git branch, and its agent's status.

use std::path::{Path, PathBuf};

use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme, Icon, Theme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use herdr_client::protocol::{
    AgentStatus, ClientShellAgent, ClientShellSnapshot, ClientShellTab, ClientShellWorkspace,
};

use super::commands::Command;
use super::registry::{
    Invocation, RenameSubject, ShellRequest, SidebarBlock, SidebarView, StatusAlign, StatusSegment,
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

/// Every workspace the daemon knows, the focused one picked out and opened
/// into its tabs. Selection is not a local field: it is the `focused` flag the
/// snapshot carries, and the tree reads it the same way the daemon does — the
/// focused workspace is the one whose tabs are on show.
fn render_workspaces(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let new_dispatch = invocation.dispatch.clone();
    let hover = theme.sidebar_accent;
    let mut column = v_flex().w_full().px(px(6.)).py(px(6.)).gap_0p5().child(
        h_flex()
            .w_full()
            .h(px(24.))
            .px(px(6.))
            .gap(px(6.))
            .items_center()
            .child(
                div()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(t("shell.workspaces").to_uppercase()),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("workspaces-new")
                    .w(px(20.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(theme.muted_foreground)
                    .hover(move |style| style.bg(hover))
                    .on_click(move |_, window, cx| {
                        new_dispatch(ShellRequest::Command(Command::NewWorkspace), window, cx);
                    })
                    .child(Icon::default().data(PLUS).w(px(12.)).h(px(12.))),
            ),
    );
    let mut count = 0;
    if let Some(snapshot) = invocation.snapshot {
        count = snapshot.workspaces.len();
        column =
            column.children(snapshot.workspaces.iter().enumerate().flat_map(
                |(index, workspace)| workspace_rows(index, workspace, snapshot, invocation),
            ));
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

/// One workspace, then — when it is the focused one — its tabs as nested rows:
/// the tree opens the place the user is working and leaves the rest folded.
fn workspace_rows(
    index: usize,
    workspace: &ClientShellWorkspace,
    snapshot: &ClientShellSnapshot,
    invocation: &Invocation,
) -> Vec<AnyElement> {
    let mut rows = vec![workspace_row(index, workspace, snapshot, invocation)];
    if workspace.focused {
        // The prototype's `.pgroup`: the open workspace's tabs hang off a
        // hairline, so the tree shows what belongs to what.
        rows.push(
            v_flex()
                .w_full()
                .ml(px(12.))
                .pl(px(2.))
                .border_l_1()
                .border_color(invocation.theme.sidebar_border)
                .children(
                    workspace_tabs(snapshot, &workspace.workspace_id)
                        .into_iter()
                        .enumerate()
                        .map(|(tab_index, tab)| tab_sub_row(tab_index, tab, snapshot, invocation)),
                )
                .into_any_element(),
        );
    }
    rows
}

/// A workspace's tabs, in the daemon's own numbering.
fn workspace_tabs<'a>(
    snapshot: &'a ClientShellSnapshot,
    workspace_id: &str,
) -> Vec<&'a ClientShellTab> {
    let mut tabs: Vec<&ClientShellTab> = snapshot
        .tabs
        .iter()
        .filter(|tab| tab.workspace_id == workspace_id)
        .collect();
    tabs.sort_by_key(|tab| tab.number);
    tabs
}

/// How many panes a tab holds, as the sub-row's trailing count.
fn tab_pane_count(snapshot: &ClientShellSnapshot, tab_id: &str) -> usize {
    snapshot
        .panes
        .iter()
        .filter(|pane| pane.tab_id == tab_id)
        .count()
}

/// The glyph a tree row leads with: a folder for the place, a pane for a tab.
const FOLDER_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/></svg>"#;
const TAB_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M12 15h5"/></svg>"#;
const PLUS: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>"#;

/// The open/closed mark of a tree row, the prototype's twisty.
fn twisty(open: bool, theme: &Theme) -> AnyElement {
    div()
        .w(px(10.))
        .flex_none()
        .text_size(px(9.))
        .text_color(theme.muted_foreground)
        .child(if open { "▾" } else { "▸" })
        .into_any_element()
}

/// The 2px bar a selected row carries at its leading edge.
fn selected_bar(theme: &Theme) -> AnyElement {
    div()
        .absolute()
        .left_0()
        .top(px(6.))
        .bottom(px(6.))
        .w(px(2.))
        .rounded_full()
        .bg(theme.primary)
        .into_any_element()
}

/// One nested tab row: a click focuses the tab, the same request the tab strip's
/// pills answer, and its trail is what the tab is holding — its agents' dot and
/// its pane count.
fn tab_sub_row(
    index: usize,
    tab: &ClientShellTab,
    snapshot: &ClientShellSnapshot,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let tab_id = tab.tab_id.clone();
    let active = Some(tab.tab_id.as_str()) == snapshot.focused_tab_id.as_deref();
    let row = h_flex()
        .id(("workspace-tab", index))
        .w_full()
        .h(px(26.))
        .relative()
        .pl(px(22.))
        .pr(px(7.))
        .gap(px(7.))
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|style| style.bg(theme.sidebar_accent))
        .on_click(move |_, window, cx| dispatch(ShellRequest::FocusTab(tab_id.clone()), window, cx))
        .when(active, |row| row.child(selected_bar(theme)))
        .child(twisty(active, theme))
        .child(
            div()
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(Icon::default().data(TAB_ICON).w(px(12.)).h(px(12.))),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_size(px(12.))
                .text_color(if active {
                    theme.foreground
                } else {
                    theme.sidebar_foreground
                })
                .child(tab.label.clone()),
        )
        .children(
            super::tab_agent_status(&snapshot.agents, &tab.tab_id)
                .filter(|status| !matches!(status, AgentStatus::Idle | AgentStatus::Unknown))
                .map(|status| {
                    div()
                        .id(("workspace-tab-dot", index))
                        .child(status_dot(status, theme))
                }),
        )
        .child(
            div()
                .flex_none()
                .font_family(theme.mono_font_family.clone())
                .text_size(px(10.5))
                .text_color(theme.muted_foreground)
                .child(tab_pane_count(snapshot, &tab.tab_id).to_string()),
        );
    if active {
        row.bg(theme.sidebar_accent).into_any_element()
    } else {
        row.into_any_element()
    }
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
    let tab_count = workspace_tabs(snapshot, &workspace.workspace_id).len();
    // A workspace being renamed is its editor, not its row: no focus click,
    // no drag while the name is on the table.
    if let Some(input) = invocation
        .rename
        .and_then(|(subject, input)| match subject {
            RenameSubject::Workspace(renaming) if *renaming == workspace.workspace_id => {
                Some(input.clone())
            }
            _ => None,
        })
    {
        let keys = invocation.dispatch.clone();
        return h_flex()
            .id(("workspace-rename", index))
            .w_full()
            .h(px(28.))
            .px(px(7.))
            .gap(px(7.))
            .items_center()
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.sidebar_accent)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => {
                        cx.stop_propagation();
                        keys(ShellRequest::CommitRename, window, cx);
                    }
                    "escape" => {
                        cx.stop_propagation();
                        keys(ShellRequest::CancelRename, window, cx);
                    }
                    _ => {}
                }
            })
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&input)
                        .h(px(22.))
                        .appearance(false)
                        .bordered(false),
                ),
            )
            .into_any_element();
    }
    let row = h_flex()
        .id(("workspace", index))
        .w_full()
        .h(px(28.))
        .relative()
        .px(px(7.))
        .gap(px(7.))
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|style| style.bg(theme.sidebar_accent))
        // The second click of a double-click is a rename, not another focus.
        .on_click(move |event, window, cx| {
            if let ClickEvent::Mouse(click) = event
                && click.up.click_count >= 2
            {
                cx.stop_propagation();
                dispatch(
                    ShellRequest::StartRename(RenameSubject::Workspace(workspace_id.clone())),
                    window,
                    cx,
                );
                return;
            }
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
        .when(selected, |row| row.child(selected_bar(theme)))
        .child(twisty(selected, theme))
        .child(
            div()
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(Icon::default().data(FOLDER_ICON).w(px(13.)).h(px(13.))),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_size(px(12.))
                .text_color(if selected {
                    theme.foreground
                } else {
                    theme.sidebar_foreground
                })
                .child(name),
        )
        .children(
            (!matches!(
                workspace.agent_status,
                AgentStatus::Idle | AgentStatus::Unknown
            ))
            .then(|| status_dot(workspace.agent_status, theme)),
        )
        .child(
            div()
                .flex_none()
                .font_family(theme.mono_font_family.clone())
                .text_size(px(10.5))
                .text_color(theme.muted_foreground)
                .child(tab_count.to_string()),
        );
    if selected {
        row.bg(theme.sidebar_accent).into_any_element()
    } else {
        row.into_any_element()
    }
}

/// A dot for an agent's status: filled while it works, red while it waits for
/// an answer, faint while idle or unknown. The tab strip wears the same dot,
/// so a status means one color everywhere the shell draws it.
pub(crate) fn status_dot(status: AgentStatus, theme: &Theme) -> impl IntoElement {
    let color = match status {
        AgentStatus::Working => theme.info,
        AgentStatus::Blocked => theme.warning,
        AgentStatus::Done => theme.success,
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
        .px(px(6.))
        .py(px(6.))
        .gap_0p5()
        .child(
            h_flex()
                .w_full()
                .h(px(24.))
                .px(px(6.))
                .items_center()
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(t("ai.agent.monitor_title").to_uppercase()),
                ),
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
        .h(px(28.))
        .relative()
        .px(px(7.))
        .gap(px(7.))
        .items_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|style| style.bg(theme.sidebar_accent))
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::FocusPane(pane_id.clone()), window, cx);
        })
        .when(focused, |row| row.child(selected_bar(theme)))
        .child(status_dot(agent.agent_status, theme))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_size(px(12.))
                .text_color(if focused {
                    theme.foreground
                } else {
                    theme.sidebar_foreground
                })
                .child(name),
        )
        .child(
            div()
                .min_w_0()
                .max_w(px(120.))
                .truncate()
                .font_family(theme.mono_font_family.clone())
                .text_size(px(10.5))
                .text_color(theme.muted_foreground)
                .child(detail),
        );
    if focused {
        row.bg(theme.sidebar_accent).into_any_element()
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
            .child(div().text_size(px(11.)).text_color(color).child(label))
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
            .text_size(px(11.))
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
/// visible without opening the settings surface. The count is the click into
/// that surface: the Config page is where the problems are listed one by one.
fn render_config(invocation: &Invocation, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
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
    let dispatch = invocation.dispatch.clone();
    Some(
        div()
            .id("status-config")
            .text_size(px(11.))
            .text_color(color)
            .cursor_pointer()
            .hover(|style| style.text_color(theme.foreground))
            .on_click(move |_, window, cx| {
                // Land on the page that lists the problems, not on whichever
                // page the dialog was left on.
                if let Some(page) = super::settings::page_index("config") {
                    dispatch(ShellRequest::ShowSettingsPage(page), window, cx);
                }
                dispatch(ShellRequest::Command(Command::ToggleSettings), window, cx);
            })
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
            .text_size(px(11.))
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
            .text_size(px(11.))
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
            .text_size(px(11.))
            .text_color(theme.danger)
            .child(reason.to_owned())
            .into_any_element(),
    )
}

/// The focused workspace's branch, so the bar names the checkout the grid is
/// working in without repeating the sidebar's whole detail line. The click is
/// the tree the branch lives in: the workspaces view, where every checkout is
/// one row deep.
fn render_branch(invocation: &Invocation, _: &mut Window, _: &mut App) -> Option<AnyElement> {
    let theme = invocation.theme;
    let branch = focused_branch(invocation.snapshot?)?;
    let dispatch = invocation.dispatch.clone();
    Some(
        div()
            .id("status-branch")
            .text_size(px(11.))
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|style| style.text_color(theme.foreground))
            .on_click(move |_, window, cx| {
                dispatch(ShellRequest::ShowView(SidebarView::Workspaces), window, cx);
            })
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

/// The prefix key, named at the bar's right edge the way the prototype reads
/// it; while the prefix is armed the same segment lights, so a chord's first
/// key is visible as it waits for the second.
fn render_prefix(invocation: &Invocation, _: &mut Window, _: &mut App) -> Option<AnyElement> {
    let theme = invocation.theme;
    Some(
        h_flex()
            .gap_1()
            .items_center()
            .px_1()
            .rounded(px(3.))
            .font_family(theme.mono_font_family.clone())
            .text_size(px(10.))
            .when(invocation.prefix_armed, |segment| {
                segment
                    .bg(theme.primary)
                    .text_color(theme.primary_foreground)
            })
            .when(!invocation.prefix_armed, |segment| {
                segment.text_color(theme.muted_foreground)
            })
            .child(t("status.prefix"))
            .child(invocation.prefix.to_owned())
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

    /// The tree opens the focused workspace into its tabs, in the daemon's own
    /// numbering; another workspace stays folded.
    #[test]
    fn the_focused_workspace_opens_into_its_tabs_in_daemon_order() {
        let mut snapshot = snapshot_with_panes();
        snapshot.tabs = serde_json::from_value(serde_json::json!([
            {"tab_id": "w1:t2", "workspace_id": "w1", "number": 2,
             "label": "second", "custom_label": false, "zoomed": false,
             "focused": false, "agent_status": "idle"},
            {"tab_id": "w1:t1", "workspace_id": "w1", "number": 1,
             "label": "first", "custom_label": false, "zoomed": false,
             "focused": true, "agent_status": "idle"},
            {"tab_id": "w2:t1", "workspace_id": "w2", "number": 1,
             "label": "elsewhere", "custom_label": false, "zoomed": false,
             "focused": false, "agent_status": "idle"}
        ]))
        .expect("the fixture tabs deserialize");
        let tabs: Vec<&str> = workspace_tabs(&snapshot, "w1")
            .iter()
            .map(|tab| tab.tab_id.as_str())
            .collect();
        assert_eq!(tabs, ["w1:t1", "w1:t2"], "daemon numbering, not list order");
        assert!(workspace_tabs(&snapshot, "w9").is_empty());
        // The focused pane's tab is the tree's own active row.
        let active = workspace_tabs(&snapshot, "w1")
            .into_iter()
            .find(|tab| Some(tab.tab_id.as_str()) == snapshot.focused_tab_id.as_deref());
        assert_eq!(
            active.expect("the focused tab is among them").label,
            "first"
        );
    }

    /// A sub-row's trail counts the tab's own panes, not the workspace's.
    #[test]
    fn a_tab_rows_count_is_its_own_panes() {
        let snapshot = snapshot_with_panes();
        assert_eq!(tab_pane_count(&snapshot, "w1:t1"), 2);
        assert_eq!(tab_pane_count(&snapshot, "w1:t9"), 0);
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
