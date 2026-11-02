//! The machines sheet: what every daemon the catalog names is running, one
//! read-only group per machine.
//!
//! The redesign has no multi-machine surface, so this one borrows the frame
//! the project already accepted for `review` — an 840px modal over a scrim,
//! opened from the command directory — and spends it on one machine group per
//! row of [`crate::machines`]'s roster: connection state, daemon version, and
//! the agents the pushed snapshot carries. Nothing here sends a command and
//! nothing polls: each group is the snapshot its endpoint connection pushed,
//! repainted by the same pulse that repaints the shell.
//!
//! A remote machine has an endpoint and no JSON API lane (W7), which is
//! exactly enough for a roster; the face stays read-only rather than growing
//! a degraded command lane. Transitions and tail comparison stay with the
//! per-window fleet, whose stream this sheet does not own.

use ely_core::{
    buttons::{Button, ButtonVariant, IconButton},
    layout::ScrollArea,
    primitives::{Backdrop, FocusScope, Icon, IconName, give_back, take_focus},
    theme::{ActiveTheme, ControlSize, Elevation, IconSize, Radius, TextSize},
    typography::Ellipsis,
};
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
    prelude::*, px,
};

use super::shell::Root;
use super::sidebar::{agent_name, ordered_agents, status_color_of, status_word};
use crate::herdr::{ConnectionStatus, LiveState};
use crate::i18n::t;
use crate::machines::{Fleet, Machine};
use herdr_client::protocol::AgentStatus;

/// The modal's widest card and the room it keeps from the window.
const WIDTH: f32 = 840.;
const MARGIN: f32 = 48.;

/// The sheet's own state: whether it is up. The roster and the live rows are
/// the shell's, so a pulse that lands while the sheet is closed still fills
/// them and nothing is read twice when it reopens.
#[derive(Default)]
pub(crate) struct State {
    open: bool,
}

impl State {
    pub(crate) fn open(&mut self) {
        self.open = true;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }
}

/// One agent of one machine, already projected for the rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentRow {
    pub(crate) name: String,
    pub(crate) status: AgentStatus,
    pub(crate) title: Option<String>,
    pub(crate) focused: bool,
}

/// One machine's live view, pulled once per pulse for the paint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MachineLive {
    pub(crate) label: String,
    pub(crate) own: bool,
    pub(crate) status: ConnectionStatus,
    pub(crate) version: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) workspaces: usize,
    pub(crate) agents: Vec<AgentRow>,
}

/// The pull: the window's own machine reads the shell's [`LiveState`], every
/// other machine reads its connection's. Called from `read_machines` on the
/// pulse, never in a render path.
pub(crate) fn pull_all(fleet: &Fleet, own: &LiveState) -> Vec<MachineLive> {
    fleet
        .roster()
        .iter()
        .map(|machine| {
            let live = if machine.own {
                Some(own.clone())
            } else {
                fleet.connection(&machine.label).map(|c| c.state())
            };
            pull(machine, live.as_ref())
        })
        .collect()
}

fn pull(machine: &Machine, live: Option<&LiveState>) -> MachineLive {
    let Some(live) = live else {
        return MachineLive {
            label: machine.label.clone(),
            own: machine.own,
            status: ConnectionStatus::Connecting,
            version: None,
            error: None,
            workspaces: 0,
            agents: Vec::new(),
        };
    };
    let snapshot = live.snapshot.clone();
    let (workspaces, agents) = snapshot
        .as_deref()
        .map(|snapshot| {
            let agents = ordered_agents(snapshot)
                .iter()
                .map(|agent| AgentRow {
                    name: agent_name(agent),
                    status: agent.agent_status,
                    title: agent
                        .terminal_title_stripped
                        .clone()
                        .or_else(|| agent.title.clone()),
                    focused: agent.focused,
                })
                .collect();
            (snapshot.workspaces.len(), agents)
        })
        .unwrap_or_default();
    // A dial in flight still carries the last failure: the row must keep
    // saying offline between two attempts rather than flipping back to
    // connecting every few seconds. `error` clears only when a dial answers
    // (`Connected`), so it is the honest signal, not the status flip-flop.
    let error = live.error.clone();
    MachineLive {
        label: machine.label.clone(),
        own: machine.own,
        status: if error.is_some() {
            ConnectionStatus::Disconnected
        } else {
            live.status
        },
        version: live.server_version.clone(),
        error,
        workspaces,
        agents,
    }
}

/// The sheet over the shell.
pub(crate) fn overlay(
    state: &State,
    live_rows: &[MachineLive],
    attached: bool,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    if !state.is_open() {
        return None;
    }
    let takeover = take_focus("machines-dialog-takeover", window, cx);
    let focus = takeover.read(cx).focus.clone();
    let theme = cx.theme();
    let colors = &theme.colors;
    let close = {
        let root = root.clone();
        let takeover = takeover.clone();
        move |window: &mut Window, cx: &mut App| {
            give_back(&takeover, window, cx);
            root.update(cx, |root, cx| root.close_machines(cx));
        }
    };

    let total_agents: usize = live_rows.iter().map(|m| m.agents.len()).sum();
    let mut body = div().w_full().flex().flex_col();
    if live_rows.is_empty() {
        body = body.child(
            div()
                .px_3()
                .py_2()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(if attached {
                    t("shell.machines.no_hosts")
                } else {
                    t("shell.machines.reading")
                }),
        );
    }
    for machine in live_rows {
        body = body.child(machine_group(machine, cx));
    }

    let card = div()
        .w(px(WIDTH).min(window.viewport_size().width - px(MARGIN)))
        .max_h(window.viewport_size().height * 0.88)
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(theme.radius(Radius::Xl))
        .border_1()
        .border_color(colors.border_strong)
        .bg(colors.bg)
        .shadow(theme.elevation(Elevation::Modal))
        .child(
            div()
                .h(px(46.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .border_b_1()
                .border_color(colors.border)
                .child(
                    Icon::new(IconName::Server)
                        .size(IconSize::Sm)
                        .color(colors.accent),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Sm))
                        .text_color(colors.fg)
                        .child(t("shell.machines.title")),
                )
                .child(div().flex_1())
                .child({
                    let close = close.clone();
                    IconButton::new("machines-close", IconName::X)
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, window, cx| close(window, cx))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .border_t_1()
                .border_color(colors.border)
                .child(
                    ScrollArea::new("machines-body")
                        .vertical()
                        .size_full()
                        .child(body),
                ),
        )
        .child(
            div()
                .h(px(42.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .border_t_1()
                .border_color(colors.border)
                .child(
                    div()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(Ellipsis::new(SharedString::from(
                            rust_i18n::t!(
                                "shell.machines.foot",
                                machines = live_rows.len(),
                                agents = total_agents
                            )
                            .to_string(),
                        ))),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(t("shell.machines.source")),
                )
                .child(div().flex_1())
                .child({
                    let close = close.clone();
                    Button::new("machines-done", t("settings.close"))
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, window, cx| close(window, cx))
                }),
        );

    let escaped = close.clone();
    Some(
        Backdrop::new("machines-scrim")
            .on_dismiss(move |window, cx| close(window, cx))
            .child(
                div()
                    .on_key_down(move |event, window, cx| {
                        if event.keystroke.key == "escape" {
                            cx.stop_propagation();
                            escaped(window, cx);
                        }
                    })
                    .child(
                        FocusScope::new(&focus)
                            .trap()
                            .node(
                                "machines-dialog-scope",
                                gpui::Role::Dialog,
                                t("shell.machines.title"),
                                None::<SharedString>,
                            )
                            .child(card),
                    ),
            )
            .into_any_element(),
    )
}

/// One machine: its connection line, then its agents.
fn machine_group(machine: &MachineLive, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    let colors = &theme.colors;
    let (dot_color, state_word) = connection_state(machine, cx);
    div()
        .w_full()
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(colors.border)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .bg(colors.sunken)
                .child(dot(dot_color))
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Sm))
                        .text_color(colors.fg)
                        .child(SharedString::from(machine.label.clone())),
                )
                .when(machine.own, |head| {
                    head.child(
                        div()
                            .flex_none()
                            .px_1()
                            .rounded(theme.radius(Radius::Sm))
                            .border_1()
                            .border_color(colors.border)
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_subtle)
                            .child(t("shell.machines.this_window")),
                    )
                })
                .when_some(machine.version.clone(), |head, version| {
                    head.child(
                        div()
                            .flex_none()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_subtle)
                            .child(SharedString::from(version)),
                    )
                })
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_muted)
                        .child(state_word),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_muted)
                        .child(SharedString::from(
                            rust_i18n::t!(
                                "shell.machines.group_counts",
                                agents = machine.agents.len(),
                                workspaces = machine.workspaces
                            )
                            .to_string(),
                        )),
                ),
        )
        .when_some(
            (machine.status == ConnectionStatus::Disconnected)
                .then_some(machine.error.clone())
                .flatten(),
            |group, error| {
                group.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.danger)
                        .child(Ellipsis::new(SharedString::from(error))),
                )
            },
        )
        .children(machine.agents.iter().map(|agent| agent_row(agent, cx)))
        // "No agents" is a fact a connected machine states; an offline one
        // knows nothing, and its reason line already says so.
        .when(
            machine.agents.is_empty() && machine.status != ConnectionStatus::Disconnected,
            |group| {
                group.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(t("shell.machines.no_agents")),
                )
            },
        )
}

/// The colour and the word of a machine's connection line.
fn connection_state(machine: &MachineLive, cx: &App) -> (gpui::Hsla, String) {
    let word = match machine.status {
        ConnectionStatus::Connected => t("shell.machines.connected"),
        ConnectionStatus::Connecting | ConnectionStatus::AwaitingSnapshot => {
            t("shell.machines.connecting")
        }
        ConnectionStatus::Disconnected => t("shell.machines.offline"),
    };
    let color = match machine.status {
        ConnectionStatus::Connected => cx.theme().colors.success,
        ConnectionStatus::Connecting | ConnectionStatus::AwaitingSnapshot => {
            cx.theme().colors.warning
        }
        ConnectionStatus::Disconnected => cx.theme().colors.danger,
    };
    (color, word.to_string())
}

fn agent_row(agent: &AgentRow, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    let colors = &theme.colors;
    let status = sidebar_str(agent.status);
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .h(px(26.))
        .child(dot(status_color_of(status, cx)))
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg)
                .child(SharedString::from(agent.name.clone())),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(status_word(status)),
        )
        .when(agent.focused, |row| {
            row.child(
                div()
                    .flex_none()
                    .px_1()
                    .rounded(theme.radius(Radius::Sm))
                    .bg(colors.sunken)
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_subtle)
                    .child(t("shell.machines.focused")),
            )
        })
        .child(div().flex_1())
        .when_some(
            agent.title.clone().filter(|t| !t.is_empty()),
            |row, title| {
                row.child(
                    div()
                        .min_w_0()
                        .max_w(px(420.))
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(Ellipsis::new(SharedString::from(title))),
                )
            },
        )
}

/// The wire's spelling the sidebar's colour helper reads.
fn sidebar_str(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Idle => "idle",
        AgentStatus::Unknown => "unknown",
    }
}

fn dot(color: gpui::Hsla) -> gpui::Div {
    div().flex_none().size(px(6.)).rounded_full().bg(color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;
    use std::sync::Arc;

    /// The pull projects one machine's snapshot: the agents in the daemon's
    /// own order, the counts beside them, and the connection's view of the
    /// world when there is no snapshot yet.
    #[test]
    fn a_pull_projects_the_snapshot_into_rows() {
        let machine = Machine {
            label: "ssh drive".to_owned(),
            target: herdr_client::ConnectTarget::Local,
            own: false,
        };
        // No live state yet: the reader has not answered. The row says
        // connecting rather than inventing an empty machine.
        let waiting = pull(&machine, None);
        assert_eq!(waiting.status, ConnectionStatus::Connecting);
        assert!(waiting.agents.is_empty());

        let mut live = LiveState {
            server_version: Some("0.9.5".to_owned()),
            ..Default::default()
        };
        let snapshot: herdr_client::protocol::ClientShellSnapshot =
            serde_json::from_value(serde_json::json!({
                "boot_id": "boot",
                "revision": 1,
                "update_install_command": "",
                "latest_release_notes_available": false,
                "integration_updates_available": false,
                "worktree_directory": "",
                "agent_view_label": "Agents",
                "tab_bar_right_separator": "",
                "tab_bar_right": [],
                "agent_order": ["p2", "p1"],
                "workspaces": [],
                "tabs": [],
                "panes": [],
                "commands": [],
                "agents": [
                    {"pane_id": "p1", "workspace_id": "w", "tab_id": "t",
                     "name": null, "display_agent": null, "agent": null, "title": null,
                     "terminal_title": null, "terminal_title_stripped": null,
                     "agent_status": "idle", "state_change_seq": 1,
                     "state_labels": [], "tokens": [], "focused": false},
                    {"pane_id": "p2", "workspace_id": "w", "tab_id": "t",
                     "name": "builder", "display_agent": null, "agent": "claude",
                     "title": null, "terminal_title": null,
                     "terminal_title_stripped": "fix the flake",
                     "agent_status": "working", "state_change_seq": 2,
                     "state_labels": [], "tokens": [], "focused": true},
                ],
            }))
            .expect("the fixture decodes");
        live.snapshot = Some(Arc::new(snapshot));
        live.status = ConnectionStatus::Connected;

        let row = pull(&machine, Some(&live));
        assert_eq!(row.status, ConnectionStatus::Connected);
        // Mid-redial the status flips back to connecting, but the failure is
        // still being carried: the row stays offline and keeps the reason.
        live.status = ConnectionStatus::Connecting;
        live.error = Some("endpoint connect failed: no route".to_owned());
        let row = pull(&machine, Some(&live));
        assert_eq!(row.status, ConnectionStatus::Disconnected);
        assert_eq!(
            row.error.as_deref(),
            Some("endpoint connect failed: no route")
        );
        assert_eq!(row.version.as_deref(), Some("0.9.5"));
        let names: Vec<&str> = row.agents.iter().map(|a| a.name.as_str()).collect();
        // The view label is what makes the daemon's own order run the rows
        // (the sidebar's same rule); without it the snapshot order stands.
        assert_eq!(names, ["builder", "p1"], "agent_order runs the rows");
        assert_eq!(row.agents[0].status, AgentStatus::Working);
        assert_eq!(row.agents[0].title.as_deref(), Some("fix the flake"));
    }
}
