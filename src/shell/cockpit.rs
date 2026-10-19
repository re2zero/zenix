//! The orchestration cockpit: the right-hand panel's block that treats the
//! running agents as a fleet rather than a list.
//!
//! Three things it does that a monitor of the daemon's snapshot cannot:
//!
//! - it names what each agent came *from* — the transitions, read off the API
//!   lane (`crate::fleet`), not the snapshot's current value;
//! - it says how long each agent has been in the status it is in, which needs a
//!   start time the snapshot does not carry;
//! - it reads the tail of every agent's pane on demand and puts them beside each
//!   other, so a fan-out of several agents can be compared instead of visited
//!   one at a time.
//!
//! What it deliberately is not: a dispatcher. Starting an agent is the
//! orchestrator's job (`crate::ai::tools`, and the MCP tools built on them), and
//! this block only ever watches, focuses and reads.

use gpui_kit::component::{Theme, h_flex, v_flex};
use gpui_kit::*;
use herdr_client::protocol::ClientShellAgent;

use super::registry::{Builtin, Invocation, ShellRequest};
use crate::events::age;
use crate::fleet::{FleetState, Tail};
use crate::i18n::t;

/// The right-hand panel's blocks this module contributes.
pub(crate) const ASIDE: &[Builtin] = &[Builtin {
    id: "cockpit",
    render,
}];

/// How tall the block is. The fleet is bounded by the panel, not by the window:
/// a long roster scrolls inside the block rather than pushing the chat panel
/// out of the way.
const HEIGHT: f32 = 320.;

/// Transitions one agent's row names before the timeline is cut off.
const HISTORY: usize = 2;

/// How much of a tail line a comparison card shows, in characters.
const TAIL_COLUMNS: usize = 96;

fn render(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let Some(snapshot) = invocation.snapshot else {
        return div().into_any_element();
    };
    let mut column = v_flex()
        .id("cockpit")
        .flex_none()
        .max_h(px(HEIGHT))
        .w_full()
        .overflow_hidden()
        .px_1()
        .py_1()
        .gap_0p5()
        .border_t_1()
        .border_color(theme.border)
        .child(head(invocation));
    if snapshot.agents.is_empty() {
        column = column.child(line(&t("fleet.empty"), theme.muted_foreground));
    } else {
        let fleet = invocation.fleet;
        // A comparison is the panel in its other mode: the rows keep their
        // status, and the room the timelines took goes to the tails. Reading
        // both at once is what the height cannot afford.
        let timeline = invocation.tails.is_empty();
        // Blocked first, then everything else in the daemon's own order: an
        // agent waiting on a person is the one thing in the fleet worth reading
        // before the rest.
        let (blocked, rest): (Vec<&ClientShellAgent>, Vec<&ClientShellAgent>) = snapshot
            .agents
            .iter()
            .partition(|agent| status_word(agent) == "blocked");
        column = column.children(
            blocked
                .into_iter()
                .chain(rest)
                .enumerate()
                .map(|(index, agent)| agent_row(index, agent, fleet, timeline, invocation)),
        );
    }
    if let Some(error) = invocation.fleet.error.as_deref() {
        column = column.child(line(error, theme.danger));
    }
    if !invocation.tails.is_empty() {
        column = column.child(tails(invocation, snapshot.agents.iter()));
    }
    column.into_any_element()
}

/// Title, the fleet's shape as one line, and the two things a reader asks for:
/// read every tail, or start the timeline over.
fn head(invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let compare = dispatch.clone();
    let clear = dispatch;
    let summary = invocation.snapshot.map_or_else(String::new, |snapshot| {
        rust_i18n::t!(
            "fleet.summary",
            agents = snapshot.agents.len(),
            blocked = invocation.fleet.count("blocked")
        )
        .to_string()
    });
    h_flex()
        .w_full()
        .px_1()
        .pb_1()
        .gap_2()
        .items_center()
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(t("fleet.title")),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(summary),
        )
        .child(action(
            "fleet-compare",
            t("fleet.compare"),
            theme,
            move |window, cx| compare(ShellRequest::CompareFleet, window, cx),
        ))
        .child(action(
            "fleet-clear",
            t("fleet.clear"),
            theme,
            move |window, cx| clear(ShellRequest::ClearFleet, window, cx),
        ))
        .into_any_element()
}

/// One agent: its status and how long it has held it, and the transitions that
/// led there.
fn agent_row(
    index: usize,
    agent: &ClientShellAgent,
    fleet: &FleetState,
    timeline: bool,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let pane_id = agent.pane_id.clone();
    let status = status_word(agent);
    let now = crate::events::now_unix_ms();
    // A pane the stream has not seen change has no start time of its own; the
    // row says nothing rather than claiming it has been there for "now".
    let held = fleet
        .watched(&agent.pane_id)
        .map(|watched| format!(" · {}", age(now, watched.since_unix_ms)))
        .unwrap_or_default();
    let mut row = v_flex()
        .id(("fleet-agent", index))
        .w_full()
        .px_1()
        .py_0p5()
        .gap_0p5()
        .rounded_sm()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::FocusPane(pane_id.clone()), window, cx);
        })
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(status_dot(status, theme))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_sm()
                        .text_color(if agent.focused {
                            theme.foreground
                        } else {
                            theme.sidebar_foreground
                        })
                        .child(agent_label(agent)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(status_color(status, theme))
                        .child(format!("{status}{held}")),
                ),
        );
    if timeline {
        for transition in fleet.history(&agent.pane_id, HISTORY) {
            // The first status this client saw has no predecessor; a row that
            // said so would be naming the client's own arrival rather than the
            // agent's.
            let arrow = match transition.from.as_deref() {
                Some(from) => format!("{from} -> {}", transition.to),
                None => format!("-> {}", transition.to),
            };
            row = row.child(
                div()
                    .pl_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{arrow} · {}", age(now, transition.at_unix_ms))),
            );
        }
    }
    row.into_any_element()
}

/// The tails of every agent, one card each, so a fan-out is read side by side.
fn tails<'a>(
    invocation: &Invocation,
    agents: impl Iterator<Item = &'a ClientShellAgent>,
) -> AnyElement {
    let theme = invocation.theme;
    let mut column = v_flex()
        .w_full()
        .pt_1()
        .gap_1()
        .border_t_1()
        .border_color(theme.border);
    for agent in agents {
        let Some(tail) = invocation.tails.get(&agent.pane_id) else {
            continue;
        };
        column = column.child(tail_card(agent, tail, theme));
    }
    column.into_any_element()
}

fn tail_card(agent: &ClientShellAgent, tail: &Tail, theme: &Theme) -> AnyElement {
    let mut card = v_flex().w_full().gap_0p5().child(
        h_flex()
            .w_full()
            .gap_1()
            .items_center()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(agent_label(agent)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(age(crate::events::now_unix_ms(), tail.at_unix_ms)),
            ),
    );
    if let Some(error) = tail.error.as_deref() {
        return card.child(line(error, theme.danger)).into_any_element();
    }
    if tail.lines.is_empty() {
        return card
            .child(line(&t("fleet.tail_empty"), theme.muted_foreground))
            .into_any_element();
    }
    for row in &tail.lines {
        card = card.child(
            div()
                .w_full()
                .min_w_0()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(cut(row, TAIL_COLUMNS)),
        );
    }
    card.into_any_element()
}

/// One dim line of text inside the block.
fn line(text: &str, color: Hsla) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .px_1()
        .text_xs()
        .text_color(color)
        .child(text.to_owned())
        .into_any_element()
}

fn action(
    id: &'static str,
    label: String,
    theme: &Theme,
    click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px_1()
        .rounded_sm()
        .text_xs()
        .bg(theme.secondary)
        .text_color(theme.secondary_foreground)
        .cursor_pointer()
        .on_click(move |_, window, cx| click(window, cx))
        .child(label)
        .into_any_element()
}

/// The daemon's own word for what an agent is doing.
fn status_word(agent: &ClientShellAgent) -> &'static str {
    match agent.agent_status {
        herdr_client::protocol::AgentStatus::Working => "working",
        herdr_client::protocol::AgentStatus::Blocked => "blocked",
        herdr_client::protocol::AgentStatus::Done => "done",
        herdr_client::protocol::AgentStatus::Idle => "idle",
        herdr_client::protocol::AgentStatus::Unknown => "unknown",
    }
}

fn status_color(status: &str, theme: &Theme) -> Hsla {
    match status {
        "working" => theme.success,
        "blocked" => theme.danger,
        "done" => theme.accent,
        _ => theme.muted_foreground,
    }
}

fn status_dot(status: &str, theme: &Theme) -> AnyElement {
    div()
        .w(px(6.))
        .h(px(6.))
        .flex_none()
        .rounded_full()
        .bg(status_color(status, theme))
        .into_any_element()
}

/// What an agent is called: the name the daemon was given, its CLI, or the pane.
fn agent_label(agent: &ClientShellAgent) -> String {
    agent
        .name
        .clone()
        .or_else(|| agent.display_agent.clone())
        .or_else(|| agent.agent.clone())
        .unwrap_or_else(|| agent.pane_id.clone())
}

/// Cut a tail row to what the panel can show, on a character boundary.
fn cut(text: &str, columns: usize) -> String {
    if text.chars().count() <= columns {
        return text.to_owned();
    }
    let cut: String = text.chars().take(columns).collect();
    format!("{cut}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    #[test]
    fn a_tail_row_is_cut_to_what_the_panel_can_show() {
        assert_eq!(cut("short", 10), "short");
        assert_eq!(cut(&"x".repeat(20), 5), "xxxxx...");
        // Cut on a character boundary, not a byte one.
        let han = "汉".repeat(10);
        assert_eq!(cut(&han, 3), "汉汉汉...");
    }

    #[test]
    fn an_agent_is_named_by_its_name_then_its_cli_then_its_pane() {
        let mut agent = agent_of("fleet-a", "pi", "working");
        assert_eq!(agent_label(&agent), "fleet-a");
        agent.name = None;
        assert_eq!(agent_label(&agent), "pi");
        agent.agent = None;
        assert_eq!(agent_label(&agent), "w1:p2");
    }

    /// The status word is the daemon's own; the colour beside it is the only
    /// thing this module decides about it.
    #[test]
    fn a_status_word_is_the_one_the_daemon_used() {
        assert_eq!(status_word(&agent_of("a", "pi", "blocked")), "blocked");
        assert_eq!(status_word(&agent_of("b", "pi", "working")), "working");
        assert_eq!(status_word(&agent_of("c", "pi", "idle")), "idle");
        assert_eq!(status_word(&agent_of("d", "pi", "done")), "done");
    }

    fn agent_of(name: &str, cli: &str, status: &str) -> ClientShellAgent {
        serde_json::from_value(serde_json::json!({
            "pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1",
            "name": name, "agent": cli, "agent_status": status,
            "state_change_seq": 1, "focused": false, "state_labels": [], "tokens": [],
        }))
        .expect("an agent")
    }
}
