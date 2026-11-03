//! The sidebar's views: what the rail puts in front, drawn as rows.
//!
//! U2-1 left the column's body empty; this module fills it. Each view reads the
//! source it names in its foot and nothing else:
//!
//! | View | Rows come from | A press does |
//! |---|---|---|
//! | workspaces | the snapshot's `workspaces`, `tabs`, `panes` | focus a workspace, or one of its tabs |
//! | workspaces · worktrees | [`crate::worktrees`] (event-refreshed, no timer) | focus an open one, else `worktree.open`; the section's `+` creates one |
//! | agents | the snapshot's `agents` in `agent_order` | focus the agent's pane |
//! | fleet | the same agents, plus [`crate::fleet`]'s watched status | focus the agent's pane |
//! | plugins | [`crate::plugins::host`]'s registry | open its manager drawer |
//!
//! The event ring is not one of them: it is debug surface, drawn by
//! [`super::events_panel`] over the status bar's bug icon (`debug.events`).
//! The project's layout file lost its row here the same day — a per-repo
//! artifact is reached from the palette's `layout.inspect` and the first-run
//! card, not parked in navigation.
//!
//! The workspaces view is two halves of the column rather than one list (the
//! same day): the project tree above, the repo's worktrees below, each with
//! its own scroll behind a hairline, so neither section pushes the other off
//! the foot. An empty daemon keeps the whole column for its first-run block.
//!
//! The rows are drawn here rather than taken from the library's `ListItem`
//! because the design's column is denser than a control row: 28px tree rows,
//! 26px sub-rows, a 30px worktree row, all at the `Xs`/`Sm` end of the type
//! scale. The library's tokens, `Disclosure`, icons and `ScrollArea` are still
//! what the column is made of; no colour is written down here.
//!
//! A workspace with more than one tab folds: its tabs hang under it as
//! sub-rows, which is the design's tree (`docs/ui/redesign/ui.js`'s
//! `sidebarWorkspaces`), and a foldable row starts open — the set of rows the
//! reader folded shut lives in [`Root`]. A workspace with one tab has nothing
//! to fold: its row is the project, named directly. Every workspace row
//! carries the branch the snapshot reports; a worktree row
//! that is open as a workspace carries that workspace's `git_ahead_behind` as
//! `+n` / `-n`, and a closed one has no counts to show — the row says nothing
//! rather than pretending the count is zero.
//!
//! No state is written during a paint: every press either asks the daemon for a
//! focus, hands [`Root`] a request, or folds a row the reader shut.

use ely_core::{
    buttons::{Button, ButtonVariant, IconButton},
    layout::ScrollArea,
    primitives::{Disclosure, Icon, IconName},
    theme::{ActiveTheme, ControlSize, IconSize, Radius, TextSize},
    typography::Ellipsis,
};
use std::collections::HashSet;

use gpui::{
    AnyElement, App, ElementId, Entity, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, div, prelude::*, px,
};
use herdr_client::protocol::{
    AgentStatus, ClientShellAgent, ClientShellSnapshot, ClientShellWorkspace,
};

use super::{chrome::View, shell::Root};
use crate::events;
use crate::fleet::FleetState;
use crate::herdr::{Connection, LiveState};
use crate::i18n::t;
use crate::plugins::host::PluginState;
use crate::plugins::model::InstalledPlugin;
use crate::worktrees::{WorktreeEntry, WorktreeState};

/// The section label's air above and below (`app.css`: `.section-label`).
const SECTION_TOP: f32 = 10.;
const SECTION_BOTTOM: f32 = 4.;

/// The row heights a paint draws at: the config's density (`row_height` is
/// 28px comfortable, 24px compact), with the sub-row and the worktree row
/// staying two pixels under and over the tree row.
fn row_heights(cx: &App) -> (f32, f32, f32) {
    let tree = crate::config::get(cx).appearance.row_height();
    (tree, tree - 2., tree + 2.)
}

/// Everything the sidebar reads, one borrow per source. [`Root`] owns each
/// value; the paint only looks.
pub(crate) struct Data<'a> {
    pub(crate) live: &'a LiveState,
    pub(crate) fleet: &'a FleetState,
    pub(crate) plugins: &'a PluginState,
    pub(crate) worktrees: &'a WorktreeState,
    /// The plugin whose manager page is in front, so its row can do the same.
    pub(crate) manager: Option<&'a str>,
    /// The workspace rows the reader folded shut. A row with more than one tab
    /// folds and starts open; this holds the folded ones, and [`Root`] owns it.
    pub(crate) folded: &'a HashSet<String>,
    /// Whether the window's daemon lane is this machine's socket. A remote
    /// window's lane views are empty by design, and their heads and feet say
    /// so instead of implying a quiet source.
    pub(crate) lane: bool,
}

/// The head's dim count: what the view holds right now, in the view's own
/// units. A view whose source is down says that instead of a stale number.
pub(crate) fn head(view: View, data: &Data, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let count = head_count(view, data);
    div()
        .h(px(36.))
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .pl_3()
        .pr_2()
        .child(
            div()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Sm))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.fg)
                .child(Ellipsis::new(SharedString::from(view.label()))),
        )
        .children(count.map(|count| {
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(count)
        }))
        .into_any_element()
}

fn head_count(view: View, data: &Data) -> Option<SharedString> {
    let snapshot = data.live.snapshot.as_deref();
    match view {
        View::Workspaces => {
            let snapshot = snapshot?;
            Some(
                rust_i18n::t!(
                    "shell.head.workspaces",
                    workspaces = snapshot.workspaces.len(),
                    panes = snapshot.panes.len()
                )
                .into(),
            )
        }
        View::Agents | View::Fleet => {
            let snapshot = snapshot?;
            let active = active_agents(snapshot);
            let key = if view == View::Fleet {
                "shell.head.fleet"
            } else {
                "shell.head.agents"
            };
            Some(rust_i18n::t!(key, agents = snapshot.agents.len(), active = active).into())
        }
        View::Plugins => Some(
            rust_i18n::t!(
                "shell.head.plugins",
                plugins = data.plugins.plugins.len(),
                enabled = data.plugins.plugins.iter().filter(|p| p.enabled).count()
            )
            .into(),
        ),
    }
}

/// The body under the head: the selected view's rows, scrolled when they do
/// not fit. The scroll area is the library's, so the overlay bar behaves like
/// every other surface's. The workspaces view is the exception at the frame
/// level, not the rows — its two sections split the column in half.
pub(crate) fn body(
    view: View,
    data: &Data,
    connection: &Connection,
    root: &Entity<Root>,
    cx: &mut App,
) -> AnyElement {
    let rows = match view {
        View::Workspaces => return workspaces_body(data, connection, root, cx),
        View::Agents => agents(data, connection, cx),
        View::Fleet => fleet(data, connection, cx),
        View::Plugins => plugins(data, root, cx),
    };
    scrolled(row_id("sidebar-body", view.id()), rows)
}

/// One full-height scroll list with the column's padding — what four of the
/// view bodies are.
fn scrolled(id: SharedString, rows: AnyElement) -> AnyElement {
    ScrollArea::new(id)
        .vertical()
        .shadows()
        .size_full()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(1.))
                .px_1p5()
                .pt_0p5()
                .pb_2()
                .child(rows),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Workspaces
// ---------------------------------------------------------------------------

fn workspaces_body(
    data: &Data,
    connection: &Connection,
    root: &Entity<Root>,
    cx: &mut App,
) -> AnyElement {
    // The empty states keep the whole column: a first run has no worktrees
    // half to show yet.
    let ready = matches!(
        data.live.status,
        crate::herdr::ConnectionStatus::Connected
            | crate::herdr::ConnectionStatus::AwaitingSnapshot
    );
    let empty = data
        .live
        .snapshot
        .as_deref()
        .is_none_or(|snapshot| snapshot.workspaces.is_empty());
    if empty {
        return scrolled(
            row_id("sidebar-body", View::Workspaces.id()),
            workspaces_empty(ready, root, cx),
        );
    }
    let Some(snapshot) = data.live.snapshot.as_deref() else {
        return scrolled(
            row_id("sidebar-body", View::Workspaces.id()),
            workspaces_empty(ready, root, cx),
        );
    };

    // The project tree above, the repo's worktrees below: two halves of the
    // column, each with its own scroll, because the sections are peers — one
    // list would make the worktrees wait for the tree to end and push them
    // off the column's foot when the session grows.
    let mut tree = div().flex().flex_col().gap(px(1.));
    for workspace in &snapshot.workspaces {
        let tabs: Vec<_> = snapshot
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id == workspace.workspace_id)
            .collect();
        let foldable = tabs.len() > 1;
        let open = subrows_open(tabs.len(), data.folded.contains(&workspace.workspace_id));
        tree = tree.child(workspace_row(
            workspace,
            tabs.len(),
            foldable,
            open,
            root,
            connection,
            cx,
        ));
        if open {
            for tab in tabs {
                let panes = snapshot
                    .panes
                    .iter()
                    .filter(|pane| pane.tab_id == tab.tab_id)
                    .count();
                tree = tree.child(tab_subrow(
                    &tab.tab_id,
                    &tab.label,
                    tab.agent_status,
                    panes,
                    tab.focused,
                    connection,
                    cx,
                ));
            }
        }
    }
    let upper = scrolled(
        row_id("sidebar-body", "workspaces-tree"),
        tree.into_any_element(),
    );
    let lower = scrolled(
        row_id("sidebar-body", "worktrees"),
        worktrees(data, connection, root, cx),
    );
    let border = cx.theme().colors.border;
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(div().flex_1().min_h_0().overflow_hidden().child(upper))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .border_t_1()
                .border_color(border)
                .child(lower),
        )
        .into_any_element()
}

/// The workspaces view with nothing in it — a first run, or an empty daemon.
/// The design gives the column its own way to start (`docs/ui/redesign` screen
/// 09): the sentence, then the same New Workspace command the palette and the
/// chord run. The button waits while the connection cannot take the request,
/// and the line says what it is waiting for instead of pretending.
fn workspaces_empty(ready: bool, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let create = {
        let root = root.clone();
        Button::new("sidebar-new-workspace", t("command.workspace.new"))
            .variant(ButtonVariant::Primary)
            .size(ControlSize::Sm)
            .disabled(!ready)
            .on_click(move |_, _, cx| {
                root.update(cx, |root, cx| root.new_workspace(cx));
            })
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .px_3()
        .py_6()
        .text_center()
        .child(
            Icon::new(IconName::FolderOpen)
                .size(IconSize::Lg)
                .color(colors.fg_subtle),
        )
        .child(
            div()
                .text_size(theme.text_size(TextSize::Sm))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.fg)
                .child(t("shell.views.empty.workspaces")),
        )
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(if ready {
                    t("shell.views.empty.workspaces_hint")
                } else {
                    t("shell.views.empty.workspaces_waiting")
                }),
        )
        .child(create)
        .into_any_element()
}

fn workspace_row(
    workspace: &ClientShellWorkspace,
    tabs: usize,
    foldable: bool,
    open: bool,
    root: &Entity<Root>,
    connection: &Connection,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let focused = workspace.focused;
    let select = connection.clone();
    let id = workspace.workspace_id.clone();
    let fold = root.clone();
    let toggle_id = workspace.workspace_id.clone();
    tree_row(row_id("workspace", &workspace.workspace_id), focused, cx)
        .cursor_pointer()
        .children(foldable.then(|| {
            // The chevron folds; it does not focus — a press on it stops the
            // row's own click the way the worktree row's × does.
            let fold = fold.clone();
            let toggle_id = toggle_id.clone();
            div()
                .id(row_id("workspace-fold", &workspace.workspace_id))
                .cursor_pointer()
                .child(
                    Disclosure::new(
                        row_id("workspace-disclosure", &workspace.workspace_id),
                        open,
                    )
                    .color(colors.fg_subtle),
                )
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let id = toggle_id.clone();
                    fold.update(cx, |root, cx| root.toggle_workspace_fold(&id, cx));
                })
                .into_any_element()
        }))
        .child(
            Icon::new(if focused {
                IconName::FolderOpen
            } else {
                IconName::Folder
            })
            .size(IconSize::Sm)
            .color(colors.fg_subtle),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Ellipsis::new(SharedString::from(workspace.label.clone()))),
        )
        .children(
            workspace
                .branch
                .as_deref()
                .map(|branch| branch_chip(branch, cx)),
        )
        // The count belongs to a fold: a single-tab workspace is the project's
        // one line, and "1" next to its name says nothing the row doesn't.
        .children(foldable.then(|| meta_text(tabs.to_string(), cx)))
        .on_click(move |_, _, _| select.focus_workspace(&id))
        .into_any_element()
}

/// Whether a workspace's tabs hang under it: more than one tab makes the row
/// foldable, and a foldable row is open unless the reader folded it shut. A
/// single-tab workspace has no fold and no sub-rows — its row is the project.
fn subrows_open(tab_count: usize, folded: bool) -> bool {
    tab_count > 1 && !folded
}

/// The branch a workspace is on, at the row's end. The design puts it on the
/// workspace row the way the identity strip's pill carries it; the cap keeps a
/// long branch name from squeezing the tab count out of the column.
fn branch_chip(branch: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .max_w(px(84.))
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(colors.fg_subtle)
        .child(Icon::new(IconName::GitBranch).size(IconSize::Xs))
        .child(
            div()
                .min_w_0()
                .child(Ellipsis::new(SharedString::from(branch.to_owned()))),
        )
        .into_any_element()
}

fn tab_subrow(
    tab_id: &str,
    label: &str,
    status: AgentStatus,
    panes: usize,
    focused: bool,
    connection: &Connection,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let hover = colors.hover;
    let select = connection.clone();
    let id = tab_id.to_owned();
    div()
        .id(row_id("tab-subrow", tab_id))
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(row_heights(cx).1))
        .ml(px(14.))
        .pl(px(18.))
        .pr_2()
        .border_l_1()
        .border_color(colors.border)
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(if focused { colors.fg } else { colors.fg_muted })
        .when(focused, |row| row.bg(colors.surface))
        .cursor_pointer()
        .hover(move |style| style.bg(hover).text_color(colors.fg))
        .on_click(move |_, _, _| select.focus_tab(&id))
        .child(status_dot(status, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Ellipsis::new(SharedString::from(label.to_owned()))),
        )
        .child(meta_text(
            rust_i18n::t!(
                if panes == 1 {
                    "shell.sidebar.tab_pane_one"
                } else {
                    "shell.sidebar.tab_panes"
                },
                count = panes
            )
            .to_string(),
            cx,
        ))
        .into_any_element()
}

/// The focused repo's worktrees. `open_workspace_id` decides what a press
/// means: a worktree already open as a workspace is focused, one that is not
/// asks the daemon to open it — the same split the old block made (M17).
fn worktrees(data: &Data, connection: &Connection, root: &Entity<Root>, cx: &App) -> AnyElement {
    let state = data.worktrees;
    let plus = {
        let root = root.clone();
        IconButton::new("worktree-new", IconName::Plus)
            .size(ControlSize::Sm)
            .tooltip(t("shell.worktrees.new"))
            .on_click(move |_, window, cx| {
                root.update(cx, |root, cx| root.open_worktree_form(window, cx));
            })
            .into_any_element()
    };
    let mut column =
        div()
            .flex()
            .flex_col()
            .child(section_label(t("shell.worktrees"), Some(plus), cx));
    if let Some(error) = state.error.as_deref() {
        column = column.child(error_line(error, cx));
    } else if state.entries.is_empty() {
        column = column.child(empty_line(t("shell.worktrees.empty"), cx));
    } else {
        for entry in &state.entries {
            column = column.child(worktree_row(entry, data.live, connection, root, cx));
        }
    }
    column.into_any_element()
}

fn worktree_row(
    entry: &WorktreeEntry,
    live: &LiveState,
    connection: &Connection,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let (hover, fg, muted) = (colors.hover, colors.fg, colors.fg_muted);
    let open = entry.openable();
    let branch = entry.branch.clone();
    let path = entry.path.clone();
    let open_workspace = entry.open_workspace_id.clone();
    let marks = git_marks(
        entry
            .open_workspace_id
            .as_deref()
            .and_then(|id| {
                live.snapshot
                    .as_deref()?
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == id)
            })
            .and_then(|workspace| workspace.git_ahead_behind),
    );
    let has_marks = marks.ahead.is_some() || marks.behind.is_some();
    let focus = connection.clone();
    let remove_root = root.clone();
    let root = root.clone();
    let row = div()
        .id(row_id("worktree", &entry.path))
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(row_heights(cx).2))
        .px_2()
        .rounded(theme.radius(Radius::Md))
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(if open { muted } else { colors.fg_subtle })
        .when(open, |row| {
            row.cursor_pointer()
                .hover(move |style| style.bg(hover).text_color(fg))
                .on_click(move |_, _, cx| match &open_workspace {
                    Some(workspace_id) => focus.focus_workspace(workspace_id),
                    None => {
                        let (branch, path) = (branch.clone(), path.clone());
                        root.update(cx, |root, _| root.open_worktree(branch, path));
                    }
                })
        })
        .child(Icon::new(IconName::GitBranch).size(IconSize::Xs))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Ellipsis::new(SharedString::from(entry.name().to_owned()))),
        )
        .children(has_marks.then(|| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .text_size(theme.text_size(TextSize::Xs))
                .children(
                    marks
                        .ahead
                        .map(|text| div().text_color(colors.success).child(text)),
                )
                .children(
                    marks
                        .behind
                        .map(|text| div().text_color(colors.danger).child(text)),
                )
                .into_any_element()
        }))
        .children(
            entry
                .open_workspace_id
                .as_ref()
                .map(|_| div().size(px(5.)).rounded_full().bg(colors.success)),
        )
        // Only a linked worktree that a workspace has open is a removal
        // target; the repo's main checkout is the same row's fact, not an
        // action. The × asks through the shared confirmation, which owns the
        // force switch a dirty checkout needs.
        .children(entry.removable().then(|| {
            let remove = remove_root.clone();
            let workspace_id = entry.open_workspace_id.clone().unwrap_or_default();
            let name = entry.name().to_owned();
            IconButton::new(
                SharedString::from(format!("worktree-remove-{}", entry.path)),
                IconName::X,
            )
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .tooltip(rust_i18n::t!("shell.worktrees.remove", name = name).to_string())
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                let (workspace_id, name) = (workspace_id.clone(), name.clone());
                remove.update(cx, |root, cx| {
                    root.ask_remove_worktree(workspace_id, name, cx)
                });
            })
            .into_any_element()
        }));
    row.into_any_element()
}

// ---------------------------------------------------------------------------
// Agents and the fleet
// ---------------------------------------------------------------------------

fn agents(data: &Data, connection: &Connection, cx: &App) -> AnyElement {
    let Some(snapshot) = data.live.snapshot.as_deref() else {
        return empty_line(t("shell.views.no_data"), cx);
    };
    let agents = ordered_agents(snapshot);
    if agents.is_empty() {
        return empty_line(t("shell.views.empty.agents"), cx);
    }
    let mut column = div().flex().flex_col().gap_0p5();
    for (title, group) in [
        (t("shell.section.active"), active_group(&agents)),
        (t("shell.section.rest"), resting_group(&agents)),
    ] {
        if group.is_empty() {
            continue;
        }
        column = column.child(section_label(title, None, cx));
        for agent in group {
            column = column.child(agent_row(agent, snapshot, connection, cx));
        }
    }
    column.into_any_element()
}

fn fleet(data: &Data, connection: &Connection, cx: &App) -> AnyElement {
    let Some(snapshot) = data.live.snapshot.as_deref() else {
        return empty_line(t("shell.views.no_data"), cx);
    };
    let agents = ordered_agents(snapshot);
    if agents.is_empty() {
        return empty_line(t("fleet.empty"), cx);
    }
    let now = events::now_unix_ms();
    let mut column = div().flex().flex_col().gap_0p5();
    for agent in agents {
        column = column.child(fleet_row(agent, snapshot, data.fleet, now, connection, cx));
    }
    if let Some(error) = data.fleet.error.as_deref() {
        column = column.child(error_line(error, cx));
    }
    column.into_any_element()
}

/// An agent's live status, as the fleet stream saw it: the held time when this
/// client watched the status change, otherwise the snapshot's own status. A
/// pane the stream has not heard from yet is not invented here.
fn fleet_detail(agent: &ClientShellAgent, fleet: &FleetState, now: u64) -> String {
    match fleet.watched(&agent.pane_id) {
        Some(watched) if watched.since_unix_ms > 0 => {
            let status = status_word(&watched.status);
            format!("{status} · {}", events::age(now, watched.since_unix_ms))
        }
        _ => status_word(status_str(agent.agent_status)),
    }
}

pub(crate) fn status_word(status: &str) -> String {
    match status {
        "working" | "blocked" | "done" | "idle" | "unknown" => t(&format!("fleet.status.{status}")),
        other => other.to_owned(),
    }
}

pub(crate) fn status_str(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Idle => "idle",
        AgentStatus::Unknown => "unknown",
    }
}

fn agent_row(
    agent: &ClientShellAgent,
    snapshot: &ClientShellSnapshot,
    connection: &Connection,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let hover = colors.hover;
    let focused = agent.focused;
    let name = agent_name(agent);
    let role = agent.agent.clone().filter(|agent| *agent != name);
    let workspace = workspace_label(snapshot, &agent.workspace_id);
    let select = connection.clone();
    let pane_id = agent.pane_id.clone();
    div()
        .id(row_id("agent", &agent.pane_id))
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(row_heights(cx).0))
        .px_2()
        .rounded(theme.radius(Radius::Md))
        .text_size(theme.text_size(TextSize::Sm))
        .text_color(if focused { colors.fg } else { colors.fg_muted })
        .when(focused, |row| row.bg(colors.surface))
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(move |_, _, _| select.focus_pane(&pane_id))
        .child(status_dot(agent.agent_status, cx))
        .child(avatar(&name, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Ellipsis::new(SharedString::from(name))),
        )
        .children(role.map(|role| {
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(role)
        }))
        .children(workspace.map(|workspace| meta_text(workspace, cx)))
        .into_any_element()
}

fn fleet_row(
    agent: &ClientShellAgent,
    snapshot: &ClientShellSnapshot,
    fleet: &FleetState,
    now: u64,
    connection: &Connection,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let hover = colors.hover;
    let focused = agent.focused;
    let name = agent_name(agent);
    let detail = fleet_detail(agent, fleet, now);
    let workspace = workspace_label(snapshot, &agent.workspace_id);
    let select = connection.clone();
    let pane_id = agent.pane_id.clone();
    div()
        .id(row_id("fleet", &agent.pane_id))
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(row_heights(cx).0))
        .px_2()
        .rounded(theme.radius(Radius::Md))
        .text_size(theme.text_size(TextSize::Sm))
        .text_color(if focused { colors.fg } else { colors.fg_muted })
        .when(focused, |row| row.bg(colors.surface))
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(move |_, _, _| select.focus_pane(&pane_id))
        .child(status_dot(agent.agent_status, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Ellipsis::new(SharedString::from(name))),
        )
        .children(workspace.map(|workspace| meta_text(workspace, cx)))
        .child(meta_text(detail, cx))
        .into_any_element()
}

/// A 18px rounded square with the agent's initial, as the design's avatar.
fn avatar(name: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let initial = name
        .chars()
        .next()
        .map(|ch| ch.to_uppercase().to_string())
        .unwrap_or_default();
    div()
        .flex_none()
        .size(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme.radius(Radius::Sm))
        .bg(colors.accent.opacity(0.14))
        .text_color(colors.accent)
        .text_size(theme.text_size(TextSize::Xs))
        .font_weight(FontWeight::SEMIBOLD)
        .child(initial)
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

fn plugins(data: &Data, root: &Entity<Root>, cx: &App) -> AnyElement {
    let state = data.plugins;
    let mut column = div().flex().flex_col().gap_0p5();
    // The way in for a directory that is not linked yet: the review reads it
    // before the daemon does. The list below can only show what is already
    // there.
    let review = {
        let root = root.clone();
        IconButton::new("plugins-link", IconName::Plus)
            .size(ControlSize::Sm)
            .tooltip(t("shell.trust.open"))
            .on_click(move |_, window, cx| {
                root.update(cx, |root, cx| root.open_trust_at(None, window, cx));
            })
    };
    column = column.child(section_label(
        t("shell.manager.title"),
        Some(review.into_any_element()),
        cx,
    ));
    if state.plugins.is_empty() && state.error.is_none() {
        return column
            .child(empty_line(t("shell.views.empty.plugins"), cx))
            .into_any_element();
    }
    if let Some(error) = state.error.as_deref() {
        column = column.child(error_line(error, cx));
    }
    for plugin in &state.plugins {
        let actions = state
            .actions
            .iter()
            .filter(|action| action.plugin_id == plugin.plugin_id)
            .count();
        let selected = data.manager == Some(plugin.plugin_id.as_str());
        column = column.child(plugin_row(
            plugin,
            actions,
            &state.declared,
            selected,
            root,
            cx,
        ));
    }
    column.into_any_element()
}

fn plugin_row(
    plugin: &InstalledPlugin,
    actions: usize,
    declared: &[crate::plugins::ui_manifest::DeclaredUi],
    selected: bool,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let local = has_local_ui(declared, &plugin.plugin_id);
    let badge = if local {
        t("shell.plugins.source_local")
    } else {
        t("shell.plugins.source_daemon")
    };
    let open = root.clone();
    let plugin_id = plugin.plugin_id.clone();
    div()
        .id(row_id("plugin", &plugin.plugin_id))
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(row_heights(cx).0))
        .px_2()
        .rounded(theme.radius(Radius::Md))
        .text_size(theme.text_size(TextSize::Sm))
        .text_color(if plugin.enabled {
            colors.fg_muted
        } else {
            colors.fg_subtle
        })
        .cursor_pointer()
        .when(selected, |row| row.bg(colors.surface))
        .hover(move |style| style.bg(colors.hover))
        .on_click(move |_, window, cx| {
            open.update(cx, |root, cx| {
                root.open_plugin_manager(&plugin_id, window, cx)
            });
        })
        .when(!plugin.enabled, |row| row.opacity(0.6))
        .when(selected, |row| {
            row.child(
                div()
                    .absolute()
                    .left(px(-6.))
                    .top(px(6.))
                    .bottom(px(6.))
                    .w(px(2.))
                    .rounded_full()
                    .bg(colors.accent),
            )
        })
        .child(
            div()
                .flex_none()
                .size(px(6.))
                .rounded_full()
                .bg(if plugin.enabled {
                    colors.success
                } else {
                    colors.fg_disabled
                }),
        )
        .child(
            Icon::new(IconName::Puzzle)
                .size(IconSize::Sm)
                .color(colors.fg_subtle),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Ellipsis::new(SharedString::from(plugin.name.clone()))),
        )
        .child(meta_text(plugin.version.clone(), cx))
        .child(
            div()
                .flex_none()
                .px_1()
                .rounded(theme.radius(Radius::Sm))
                .border_1()
                .border_color(colors.border)
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(badge),
        )
        .child(meta_text(actions.to_string(), cx))
        .into_any_element()
}

/// Whether a plugin declared a `zenix-ui.toml` this client can draw. The
/// declaration was read by the host off the UI thread; the paint only asks
/// whether any of its slots is filled.
fn has_local_ui(declared: &[crate::plugins::ui_manifest::DeclaredUi], plugin_id: &str) -> bool {
    declared
        .iter()
        .find(|entry| entry.plugin_id == plugin_id)
        .is_some_and(|entry| {
            let ui = &entry.ui;
            !(ui.commands.is_empty()
                && ui.sidebar_items.is_empty()
                && ui.panels.is_empty()
                && ui.status_items.is_empty()
                && ui.settings_sections.is_empty())
        })
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// A row's element id: gpui's `ElementId` names a row by one owned string, so
/// the kind and the key are joined once here rather than at every call site.
/// The debug log's rows share it.
pub(crate) fn row_id(kind: &str, key: &str) -> SharedString {
    SharedString::from(format!("{kind}-{key}"))
}

/// A row's shell: 28px, the design's radius, the pointer's fill. The selected
/// row carries the 2px accent bar at the column's edge.
fn tree_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<gpui::Div> {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(row_heights(cx).0))
        .px_2()
        .rounded(theme.radius(Radius::Md))
        .text_size(theme.text_size(TextSize::Sm))
        .text_color(if selected { colors.fg } else { colors.fg_muted })
        .when(selected, |row| {
            row.bg(colors.surface).child(
                div()
                    .absolute()
                    .left(px(-6.))
                    .top(px(6.))
                    .bottom(px(6.))
                    .w(px(2.))
                    .rounded_full()
                    .bg(colors.accent),
            )
        })
}

/// A section label: uppercase, spaced, with an optional action at the end.
fn section_label(title: String, action: Option<AnyElement>, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .mt(px(SECTION_TOP))
        .mb(px(SECTION_BOTTOM))
        .mx_2()
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.fg_subtle)
                .child(title.to_uppercase()),
        )
        .child(div().flex_1())
        .children(action)
}

/// The dim number at a row's end: mono, one step below the row's text.
fn meta_text(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex_none()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(theme.colors.fg_subtle)
        .child(text.into())
        .into_any_element()
}

fn status_dot(status: AgentStatus, cx: &App) -> AnyElement {
    dot(status_color(status, cx)).into_any_element()
}

/// The two git marks a worktree row can wear: `+n` commits the checkout has
/// that its upstream does not, `-n` the reverse. The daemon reports the counts
/// on the workspace the worktree is open as, so a closed worktree has none —
/// the row says nothing rather than pretending the count is zero.
#[derive(Debug, Default, PartialEq, Eq)]
struct GitMarks {
    ahead: Option<String>,
    behind: Option<String>,
}

fn git_marks(counts: Option<(usize, usize)>) -> GitMarks {
    let Some((ahead, behind)) = counts else {
        return GitMarks::default();
    };
    GitMarks {
        ahead: (ahead > 0).then(|| format!("+{ahead}")),
        behind: (behind > 0).then(|| format!("-{behind}")),
    }
}

/// The color a status wears: the triad the sidebar, the tab strip and the
/// fleet's cards share. A status the enum does not know is the quiet one —
/// never a color borrowed from another state.
fn status_color(status: AgentStatus, cx: &App) -> Hsla {
    let colors = &cx.theme().colors;
    match status {
        AgentStatus::Working => colors.info,
        AgentStatus::Blocked => colors.warning,
        AgentStatus::Done => colors.success,
        AgentStatus::Idle | AgentStatus::Unknown => colors.fg_subtle,
    }
}

/// The same color for a status read as the wire spells it. The fleet stream's
/// transitions carry strings, and a card must paint them the way the sidebar
/// paints the snapshot's enum.
pub(crate) fn status_color_of(status: &str, cx: &App) -> Hsla {
    match status {
        "working" => cx.theme().colors.info,
        "blocked" => cx.theme().colors.warning,
        "done" => cx.theme().colors.success,
        _ => cx.theme().colors.fg_subtle,
    }
}

fn dot(color: Hsla) -> gpui::Div {
    div().flex_none().size(px(6.)).rounded_full().bg(color)
}

pub(crate) fn empty_line(text: String, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .px_2()
        .py_3()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(theme.colors.fg_subtle)
        .child(text)
        .into_any_element()
}

pub(crate) fn error_line(text: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .px_2()
        .py_1()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(theme.colors.danger)
        .child(Ellipsis::new(SharedString::from(text.to_owned())))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Data helpers
// ---------------------------------------------------------------------------

/// Agents in the daemon's order. `agent_order` is a pane-id list that only
/// covers what the daemon's own agents view lists; an id it names but the
/// snapshot lacks is skipped, and an empty order falls back to the snapshot.
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

fn active_group<'a>(agents: &[&'a ClientShellAgent]) -> Vec<&'a ClientShellAgent> {
    agents
        .iter()
        .copied()
        .filter(|agent| {
            matches!(
                agent.agent_status,
                AgentStatus::Working | AgentStatus::Blocked
            )
        })
        .collect()
}

fn resting_group<'a>(agents: &[&'a ClientShellAgent]) -> Vec<&'a ClientShellAgent> {
    agents
        .iter()
        .copied()
        .filter(|agent| {
            !matches!(
                agent.agent_status,
                AgentStatus::Working | AgentStatus::Blocked
            )
        })
        .collect()
}

pub(crate) fn active_agents(snapshot: &ClientShellSnapshot) -> usize {
    snapshot
        .agents
        .iter()
        .filter(|agent| {
            matches!(
                agent.agent_status,
                AgentStatus::Working | AgentStatus::Blocked
            )
        })
        .count()
}

/// The name a row shows: the agent's own, else the CLI's display name, else
/// the CLI, else the pane it lives in.
pub(crate) fn agent_name(agent: &ClientShellAgent) -> String {
    agent
        .name
        .clone()
        .or_else(|| agent.display_agent.clone())
        .or_else(|| agent.agent.clone())
        .unwrap_or_else(|| agent.pane_id.clone())
}

fn workspace_label(snapshot: &ClientShellSnapshot, workspace_id: &str) -> Option<SharedString> {
    snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == workspace_id)
        .map(|workspace| SharedString::from(workspace.label.clone()))
}

/// Where a plugin row says its declaration came from, for the tests: the badge
/// key the row uses.
#[cfg(test)]
fn source_badge(
    plugin: &InstalledPlugin,
    declared: &[crate::plugins::ui_manifest::DeclaredUi],
) -> &'static str {
    if has_local_ui(declared, &plugin.plugin_id) {
        "shell.plugins.source_local"
    } else {
        "shell.plugins.source_daemon"
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ely_core::theme::Palette;

    use super::*;
    use crate::plugins::model::{InstalledPlugin, PluginSource, SourceKind};
    use crate::plugins::ui_manifest::{self, DeclaredUi};

    fn agent(pane_id: &str, status: AgentStatus) -> ClientShellAgent {
        ClientShellAgent {
            pane_id: pane_id.to_owned(),
            workspace_id: "w1".to_owned(),
            tab_id: "t1".to_owned(),
            name: None,
            display_agent: None,
            agent: Some("claude".to_owned()),
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: status,
            state_change_seq: 0,
            state_labels: Vec::new(),
            tokens: Vec::new(),
            focused: false,
        }
    }

    fn plugin(id: &str, enabled: bool) -> InstalledPlugin {
        InstalledPlugin {
            plugin_id: id.to_owned(),
            name: id.to_owned(),
            version: "1.0.0".to_owned(),
            min_herdr_version: String::new(),
            manifest_path: String::new(),
            plugin_root: String::new(),
            enabled,
            platforms: None,
            source: PluginSource {
                kind: SourceKind::Local,
                ..PluginSource::default()
            },
            build: Vec::new(),
            startup: Vec::new(),
            actions: Vec::new(),
            panes: Vec::new(),
            events: Vec::new(),
            link_handlers: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn an_agents_name_falls_back_in_order() {
        let mut agent = agent("pane-1", AgentStatus::Working);
        assert_eq!(agent_name(&agent), "claude");
        agent.display_agent = Some("builder".to_owned());
        assert_eq!(agent_name(&agent), "builder");
        agent.name = Some("planner".to_owned());
        assert_eq!(agent_name(&agent), "planner");
    }

    #[test]
    fn the_groups_partition_the_fleet() {
        let working = agent("a", AgentStatus::Working);
        let blocked = agent("b", AgentStatus::Blocked);
        let done = agent("c", AgentStatus::Done);
        let idle = agent("d", AgentStatus::Idle);
        let agents = vec![&working, &blocked, &done, &idle];
        let active = active_group(&agents);
        let rest = resting_group(&agents);
        assert_eq!(active.len(), 2);
        assert_eq!(rest.len(), 2);
        assert_eq!(active.len() + rest.len(), agents.len());
    }

    #[test]
    fn a_status_with_a_key_is_named_and_anything_else_is_shown_as_it_came() {
        assert_ne!(status_word("working"), "working");
        assert_eq!(status_word("waiting_for_a_human"), "waiting_for_a_human");
        for status in ["working", "blocked", "done", "idle", "unknown"] {
            assert_ne!(status_word(status), status);
        }
    }

    #[test]
    fn a_plugin_is_local_when_its_declaration_fills_a_slot() {
        let with_ui = plugin("with-ui", true);
        let without = plugin("plain", true);
        let empty = DeclaredUi {
            plugin_id: "plain".to_owned(),
            ui: ui_manifest::UiContributions::default(),
        };
        let local = ui_manifest::parse(
            "with-ui",
            Path::new("/tmp/with-ui"),
            "[[sidebar_item]]\nid = \"row\"\ntitle = \"Row\"\n",
        );
        assert!(!local.sidebar_items.is_empty(), "{:?}", local.warnings);
        let declared = vec![
            DeclaredUi {
                plugin_id: "with-ui".to_owned(),
                ui: local,
            },
            empty,
        ];
        assert_eq!(
            source_badge(&without, &declared),
            "shell.plugins.source_daemon"
        );
        assert_eq!(
            source_badge(&with_ui, &declared),
            "shell.plugins.source_local"
        );
        // A declaration with nothing in it is the daemon's registry entry: the
        // badge follows what was drawn, not the file's presence.
        let blank = DeclaredUi {
            plugin_id: "blank".to_owned(),
            ui: ui_manifest::UiContributions::default(),
        };
        assert_eq!(
            source_badge(&plugin("blank", true), &[blank]),
            "shell.plugins.source_daemon"
        );
    }

    /// A row's colours come from the palette, never from a literal: the same
    /// status reads as different ink in the two modes but keeps its token.
    #[test]
    fn a_status_dot_reads_the_palette() {
        let dark = Palette::dark(false);
        let light = Palette::light(false);
        assert_ne!(dark.info, light.info);
        assert_ne!(dark.warning, light.warning);
    }

    #[test]
    fn git_marks_drop_zeroes_and_keep_the_sign() {
        assert_eq!(git_marks(None), GitMarks::default());
        assert_eq!(git_marks(Some((0, 0))), GitMarks::default());
        assert_eq!(git_marks(Some((2, 0))).ahead.as_deref(), Some("+2"));
        assert_eq!(git_marks(Some((0, 3))).behind.as_deref(), Some("-3"));
        let both = git_marks(Some((2, 3)));
        assert_eq!(
            (both.ahead.as_deref(), both.behind.as_deref()),
            (Some("+2"), Some("-3"))
        );
    }

    /// A single-tab workspace is one row with no fold; a multi-tab one folds
    /// and starts open, and only the reader's own fold closes it.
    #[test]
    fn a_fold_needs_more_than_one_tab_and_starts_open() {
        assert!(!subrows_open(0, false));
        assert!(!subrows_open(1, false));
        assert!(subrows_open(2, false));
        assert!(subrows_open(5, false));
        assert!(!subrows_open(2, true));
        assert!(!subrows_open(1, true));
    }
}
