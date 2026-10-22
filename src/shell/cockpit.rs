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

use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{Theme, h_flex, v_flex};
use gpui_kit::*;
use herdr_client::protocol::{ClientShellAgent, ClientShellSnapshot};

use super::panels::ordered_agents;
use super::registry::{AsideBlock, AsideTab, Invocation, ShellRequest, SidebarBlock, SidebarView};
use crate::events::age;
use crate::fleet::{FleetState, Tail};
use crate::i18n::t;

/// The right-hand panel's blocks this module contributes: the fleet is the
/// panel's second tab, with the whole panel to itself.
pub(crate) const ASIDE: &[AsideBlock] = &[AsideBlock {
    tab: AsideTab::Fleet,
    render,
}];

/// The sidebar's block: the fleet as the rail's sixth view. The roster and the
/// transitions are the part that reads in a column; the tails stay with the
/// aside's tab, where there is room to put them beside each other.
pub(crate) const SIDEBAR: &[SidebarBlock] = &[SidebarBlock {
    view: SidebarView::Fleet,
    render: render_sidebar,
}];

/// The fleet view's block. One roster, uncut by a comparison mode: a column of
/// agents with how long each status has held and where it came from.
fn render_sidebar(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let Some(snapshot) = invocation.snapshot else {
        return crate::shell::panels::empty_view(
            t("fleet.title"),
            t("shell.views.empty.agents"),
            theme,
        );
    };
    if snapshot.agents.is_empty() {
        return crate::shell::panels::empty_view(t("fleet.title"), t("fleet.empty"), theme);
    }
    let mut column = v_flex()
        .w_full()
        .px_1()
        .py_1()
        .gap_0p5()
        .child(head(invocation));
    // The column is the room the aside tab caps at five rows; the head's
    // summary carries the "+n" for the rest either way.
    column = column.children(
        roster(snapshot)
            .into_iter()
            .take(ROSTER_SHOWN)
            .enumerate()
            .map(|(index, agent)| agent_row(index, agent, invocation.fleet, invocation)),
    );
    if let Some(error) = invocation.fleet.error.as_deref() {
        column = column.child(line(error, theme.danger));
    }
    column.into_any_element()
}

/// Transitions one agent's row names before the timeline is cut off.
const HISTORY: usize = 2;

/// Rows of the roster, and tail cards, the block draws. A fleet larger than this
/// is counted in the header rather than pushed out of the block: the panel is a
/// glance at what is happening, and the block has a fixed height it cannot grow
/// past without taking the chat panel's room.
const ROSTER_SHOWN: usize = 5;
const TAILS_SHOWN: usize = 2;

/// How much of a tail line a comparison card shows, in characters.
const TAIL_COLUMNS: usize = 96;

/// How many transitions a card's strip carries before the status it holds now:
/// three reached statuses plus the current one, which is the history the eye
/// can read in four pixels of height.
const TIMELINE_SEGS: usize = 4;

fn render(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let Some(snapshot) = invocation.snapshot else {
        return div().into_any_element();
    };
    // The tab is the block's whole room, so it fills it and scrolls rather
    // than sitting under the conversation. `overflow_y_scrollbar` wraps the
    // column in gpui-component's `Scrollable`, which owns the handle (per
    // window, keyed by this id) and the wheel — tracking a handle of our own
    // here would leave the wheel driving a different one.
    let mut column = v_flex()
        .id("cockpit")
        .flex_1()
        .min_h_0()
        .w_full()
        .px_2()
        .py_2()
        .gap_1()
        .overflow_y_scrollbar()
        .child(head(invocation));
    if snapshot.agents.is_empty() {
        column = column.child(line(&t("fleet.empty"), theme.muted_foreground));
    } else {
        let fleet = invocation.fleet;
        // The timeline rides on every card: it is four pixels of height, and
        // the tails it once gave way to live in their own section below.
        column = column.children(
            roster(snapshot)
                .into_iter()
                .take(ROSTER_SHOWN)
                .enumerate()
                .map(|(index, agent)| fleet_card(index, agent, fleet, invocation)),
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
        let hidden = snapshot.agents.len().saturating_sub(ROSTER_SHOWN);
        let more = if hidden > 0 {
            format!(" +{hidden}")
        } else {
            String::new()
        };
        format!(
            "{}{more}",
            rust_i18n::t!(
                "fleet.summary",
                agents = snapshot.agents.len(),
                blocked = invocation.fleet.count("blocked")
            )
        )
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

/// The roster's order. With an agent view active the daemon has been told how
/// to filter and sort the fleet, so its order is the one the reader asked for
/// and the block does not second-guess it. Without a view there is no sort to
/// respect, and blocked comes first: an agent waiting on a person is the one
/// thing worth reading before the rest.
fn roster(snapshot: &ClientShellSnapshot) -> Vec<&ClientShellAgent> {
    let ordered = ordered_agents(snapshot);
    if snapshot.agent_view_label.is_some() {
        return ordered;
    }
    let (blocked, rest): (Vec<&ClientShellAgent>, Vec<&ClientShellAgent>) = ordered
        .into_iter()
        .partition(|agent| status_word(agent) == "blocked");
    blocked.into_iter().chain(rest).collect()
}

/// One agent in the sidebar's compact column: its status and how long it has
/// held it, and the transitions that led there as dim lines. The aside's card
/// ([`fleet_card`]) is the same facts in the roomier shape.
fn agent_row(
    index: usize,
    agent: &ClientShellAgent,
    fleet: &FleetState,
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
                        .child(format!("{}{held}", status_label(status))),
                ),
        );
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
    row.into_any_element()
}

/// One agent as the prototype's fleet card: who it is and what it is doing now
/// (name, status label, how long the status has held), the pane it lives in,
/// the transitions it went through as one segmented strip, and the two things a
/// reader does from here — focus the pane, or read its tail beside the others.
/// Sending a prompt is the orchestrator's job, not the card's.
fn fleet_card(
    index: usize,
    agent: &ClientShellAgent,
    fleet: &FleetState,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let focus = invocation.dispatch.clone();
    let tail = invocation.dispatch.clone();
    let pane_id = agent.pane_id.clone();
    let pane_for_tail = agent.pane_id.clone();
    let status = status_word(agent);
    let now = crate::events::now_unix_ms();
    let held = fleet
        .watched(&agent.pane_id)
        .map(|watched| age(now, watched.since_unix_ms))
        .unwrap_or_default();
    v_flex()
        .w_full()
        .gap_1()
        .px_2()
        .py_1p5()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(agent_label(agent)),
                )
                .child(status_tag(status, theme))
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(held),
                ),
        )
        .child(
            div()
                .w_full()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(agent.pane_id.clone()),
        )
        .child(timeline(agent, fleet, theme))
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .id(("fleet-focus", index))
                        .px_1p5()
                        .py(px(1.))
                        .rounded_sm()
                        .border_1()
                        .border_color(theme.border)
                        .text_xs()
                        .cursor_pointer()
                        .hover(|style| style.border_color(theme.muted_foreground))
                        .on_click(move |_, window, cx| {
                            focus(ShellRequest::FocusPane(pane_id.clone()), window, cx);
                        })
                        .child(t("fleet.focus")),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .id(("fleet-tail", index))
                        .px_1p5()
                        .py(px(1.))
                        .rounded_sm()
                        .border_1()
                        .border_color(theme.border)
                        .text_xs()
                        .cursor_pointer()
                        .hover(|style| style.border_color(theme.muted_foreground))
                        .on_click(move |_, window, cx| {
                            tail(ShellRequest::ReadTail(pane_for_tail.clone()), window, cx);
                        })
                        .child(t("fleet.tail")),
                ),
        )
        .into_any_element()
}

/// A status as the prototype's tag reads: the dot and the word beside it, both
/// in the status's own color.
fn status_tag(status: &str, theme: &Theme) -> AnyElement {
    h_flex()
        .gap_1()
        .items_center()
        .child(status_dot(status, theme))
        .child(
            div()
                .text_xs()
                .text_color(status_color(status, theme))
                .child(status_label(status)),
        )
        .into_any_element()
}

/// The transitions one card strip-ifies: one segment per status this client has
/// seen the pane reach, then the status it holds now. A pane the stream has not
/// seen move is one quiet segment rather than a history invented for it.
fn timeline(agent: &ClientShellAgent, fleet: &FleetState, theme: &Theme) -> AnyElement {
    let mut strip = h_flex().w_full().h(px(4.)).gap_0p5();
    for transition in fleet.history(&agent.pane_id, TIMELINE_SEGS - 1) {
        strip = strip.child(seg(&transition.to, theme));
    }
    strip
        .child(seg(status_word(agent), theme))
        .into_any_element()
}

/// One segment of the strip: the status's color, at the strip's full height.
fn seg(status: &str, theme: &Theme) -> AnyElement {
    div()
        .flex_1()
        .h_full()
        .rounded_full()
        .bg(status_color(status, theme))
        .into_any_element()
}

/// The tails read so far, the prototype's TAIL 对比: a head naming the section
/// and how much it holds, then the cards side by side, two across — a third
/// column would be too narrow to read a line of output in. A pane with no tail
/// yet is not a hole; it appears when its card's tail button, or the head's
/// compare, reads it.
fn tails<'a>(
    invocation: &Invocation,
    agents: impl Iterator<Item = &'a ClientShellAgent>,
) -> AnyElement {
    let theme = invocation.theme;
    let shown: Vec<&ClientShellAgent> = agents
        .filter(|agent| invocation.tails.contains_key(&agent.pane_id))
        .take(TAILS_SHOWN)
        .collect();
    let count = invocation.tails.len();
    let mut section = v_flex()
        .w_full()
        .pt_1()
        .gap_1()
        .border_t_1()
        .border_color(theme.border)
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(t("fleet.tail_compare")),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(rust_i18n::t!("fleet.tail_count", count = count).to_string()),
                ),
        );
    for pair in shown.chunks(2) {
        section = section.child(h_flex().w_full().items_stretch().children(pair.iter().map(
            |agent| {
                let tail = invocation
                    .tails
                    .get(&agent.pane_id)
                    .expect("the filter kept only panes with a tail");
                tail_card(agent, tail, theme)
            },
        )));
    }
    section.into_any_element()
}

fn tail_card(agent: &ClientShellAgent, tail: &Tail, theme: &Theme) -> AnyElement {
    let card = v_flex()
        .flex_1()
        .min_w_0()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .px_1p5()
                .py_0p5()
                .border_b_1()
                .border_color(theme.border)
                .child(status_dot(status_word(agent), theme))
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
    let mut body = v_flex().w_full().px_1p5().py_1().gap_0p5();
    if let Some(error) = tail.error.as_deref() {
        body = body.child(line(error, theme.danger));
        return card.child(body).into_any_element();
    }
    if tail.lines.is_empty() {
        body = body.child(line(&t("fleet.tail_empty"), theme.muted_foreground));
        return card.child(body).into_any_element();
    }
    for row in &tail.lines {
        body = body.child(
            div()
                .w_full()
                .min_w_0()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(cut(row, TAIL_COLUMNS)),
        );
    }
    card.child(body).into_any_element()
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

/// The status in the reader's language: the daemon's word decides the color
/// and the sort, the label is what the fleet says.
fn status_label(status: &str) -> String {
    let key = match status {
        "working" => "fleet.status.working",
        "blocked" => "fleet.status.blocked",
        "done" => "fleet.status.done",
        "idle" => "fleet.status.idle",
        _ => "fleet.status.unknown",
    };
    t(key)
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

    /// Every status the daemon can report has a label in the reader's language,
    /// and the daemon's word is still what picks it.
    #[test]
    fn every_status_word_has_its_label() {
        for word in ["working", "blocked", "done", "idle", "unknown"] {
            let label = status_label(word);
            assert_ne!(label, "", "{word} has a blank label");
            assert!(!label.starts_with("fleet."), "{word} has no translation");
        }
    }

    fn agent_of(name: &str, cli: &str, status: &str) -> ClientShellAgent {
        serde_json::from_value(serde_json::json!({
            "pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1",
            "name": name, "agent": cli, "agent_status": status,
            "state_change_seq": 1, "focused": false, "state_labels": [], "tokens": [],
        }))
        .expect("an agent")
    }

    fn snapshot_with(agents: serde_json::Value) -> ClientShellSnapshot {
        serde_json::from_value(serde_json::json!({
            "boot_id": "boot", "revision": 1,
            "update_install_command": "", "latest_release_notes_available": false,
            "integration_updates_available": false, "worktree_directory": "",
            "tab_bar_right_separator": "", "tab_bar_right": [], "agent_order": [],
            "focused_workspace_id": "w1", "focused_tab_id": "w1:t1",
            "focused_pane_id": "w1:p2",
            "workspaces": [], "tabs": [], "panes": [],
            "agents": agents, "commands": [],
        }))
        .expect("the fixture snapshot deserializes")
    }

    fn three_agents() -> serde_json::Value {
        serde_json::json!([
            {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1",
             "name": "a", "agent_status": "idle", "state_change_seq": 1,
             "focused": false, "state_labels": [], "tokens": []},
            {"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1",
             "name": "b", "agent_status": "blocked", "state_change_seq": 2,
             "focused": false, "state_labels": [], "tokens": []},
            {"pane_id": "w1:p3", "workspace_id": "w1", "tab_id": "w1:t1",
             "name": "c", "agent_status": "working", "state_change_seq": 3,
             "focused": false, "state_labels": [], "tokens": []}
        ])
    }

    /// Without an agent view there is no daemon sort to respect, and the block
    /// promotes the agent that is waiting on a person.
    #[test]
    fn without_a_view_the_blocked_agent_comes_first() {
        let snapshot = snapshot_with(three_agents());
        let panes: Vec<&str> = roster(&snapshot)
            .iter()
            .map(|agent| agent.pane_id.as_str())
            .collect();
        assert_eq!(panes, ["w1:p2", "w1:p1", "w1:p3"]);
    }

    /// With a view active the daemon has been told how to filter and sort, so
    /// its order wins over the block's own preference.
    #[test]
    fn an_active_view_keeps_the_daemons_order() {
        let mut snapshot = snapshot_with(three_agents());
        snapshot.agent_view_label = Some("working".into());
        snapshot.agent_order = vec!["w1:p3".into(), "w1:p2".into(), "w1:p1".into()];
        let panes: Vec<&str> = roster(&snapshot)
            .iter()
            .map(|agent| agent.pane_id.as_str())
            .collect();
        assert_eq!(panes, ["w1:p3", "w1:p2", "w1:p1"]);
    }

    /// An order that names no agent falls back to the snapshot, and the view
    /// still suppresses the block's own promotion.
    #[test]
    fn an_empty_view_order_falls_back_to_the_snapshot() {
        let mut snapshot = snapshot_with(three_agents());
        snapshot.agent_view_label = Some("gone".into());
        snapshot.agent_order = vec!["w9:p9".into()];
        let panes: Vec<&str> = roster(&snapshot)
            .iter()
            .map(|agent| agent.pane_id.as_str())
            .collect();
        assert_eq!(panes, ["w1:p1", "w1:p2", "w1:p3"]);
    }
}
