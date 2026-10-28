//! The aside's fleet tab: the running agents as cards, the transitions this
//! client watched, and the two-column TAIL comparison.
//!
//! The data is already in place and is not read twice:
//!
//! | What a card shows | Where it comes from |
//! |---|---|
//! | name, role, pane | the snapshot's `agents`, in `agent_order` |
//! | status, held age | [`crate::fleet`]'s watched status, else the snapshot's |
//! | the transition strip | one segment per `pane.agent_status_changed` this client read |
//! | the recent transitions | the same stream's ring, newest first |
//! | a tail column | `pane.read` on demand, when the reader asks for it |
//!
//! Three decisions follow U2 (the sidebar) and U3-2 (the conversation):
//!
//! - **Nothing is polled.** The cards repaint on the snapshot's pulses and on
//!   the fleet stream's transition pulses; the tails are read when a card's
//!   terminal button, a mode switch or the refresh button asks, never on a
//!   timer.
//! - **Ages are computed at paint.** `events::age` runs against the moment the
//!   frame is drawn, the way the history rows and the event ring already do.
//!   A self-ticking element leaves the previous string's pixels behind in a
//!   row nobody laid out again (U3-2's finding).
//! - **A failed read is a line in its column.** A pane that left the daemon,
//!   or a socket that refused the read, states why where the output would
//!   be; nothing here panics, and no column is invented to fill the gap.
//!
//! The strip's empty slots are history this client did not watch — the daemon
//! does not replay — so they are drawn as the background hairline rather than
//! guessed at. The colours are the sidebar's triad (`sidebar::status_color_of`)
//! so a status reads the same wherever it is drawn.

use ely_core::buttons::{ButtonVariant, IconButton};
use ely_core::data_display::{Badge, Tone};
use ely_core::layout::{ScrollArea, SimpleGrid};
use ely_core::primitives::{Icon, IconName};
use ely_core::theme::{ActiveTheme, ControlSize, IconSize, Radius, TextSize};
use ely_core::typography::Ellipsis;
use gpui::{
    AnyElement, App, ClickEvent, Entity, FontWeight, Hsla, IntoElement, ParentElement,
    SharedString, Styled, Window, div, prelude::*, px,
};
use herdr_client::protocol::{ClientShellAgent, ClientShellSnapshot};

use super::shell::Root;
use super::sidebar;
use crate::events;
use crate::fleet::{FleetState, Tail, Tails};
use crate::herdr::{Connection, LiveState};
use crate::i18n::t;

/// How many rows the panel asks the daemon for when it reads a tail.
pub(crate) const READ_ROWS: u32 = 200;

/// How many of those rows a column keeps. The design's comparison is the
/// last two hundred lines, side by side; the column scrolls.
pub(crate) const READ_LINES: usize = READ_ROWS as usize;

/// Segments in a card's transition strip (the design's strip is eight).
const STRIP: usize = 8;

/// Transitions listed under the cards, newest first.
const RECENT: usize = 5;

/// The card grid's minimum column width: two cards at the default 372px
/// aside, one at the schema's 240px floor.
const CARD_MIN: f32 = 150.;

/// What the tab shows: the roster of cards, or the TAIL comparison.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Roster,
    Compare,
}

impl Mode {
    fn id(self) -> &'static str {
        match self {
            Self::Roster => "roster",
            Self::Compare => "compare",
        }
    }

    fn label(self) -> String {
        match self {
            Self::Roster => t("shell.fleet.mode_roster"),
            Self::Compare => t("shell.fleet.mode_compare"),
        }
    }
}

/// The panel's own state: what is in front, what is picked for the comparison,
/// and the tails read for the picked panes. None of it belongs to the daemon,
/// so none of it is in the snapshot.
#[derive(Debug, Default)]
pub(crate) struct State {
    mode: Mode,
    /// The panes picked for TAIL, in pick order, at most two.
    picked: Vec<String>,
    /// The last read answers, by pane.
    tails: Tails,
    /// A read is in flight; a second ask waits for it.
    reading: bool,
    /// Bumped per read; an answer from an older request is dropped.
    generation: u64,
}

impl State {
    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }

    pub(crate) fn picked(&self) -> &[String] {
        &self.picked
    }

    pub(crate) fn is_picked(&self, pane: &str) -> bool {
        self.picked.iter().any(|picked| picked == pane)
    }

    pub(crate) fn reading(&self) -> bool {
        self.reading
    }

    pub(crate) fn tails(&self) -> &Tails {
        &self.tails
    }

    /// Put a mode in front. The caller decides whether the switch should read.
    pub(crate) fn set_mode(&mut self, mode: Mode) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        true
    }

    /// Pick a pane for the comparison, or unpick it. A third pick drops the
    /// oldest: the design's comparison is two columns, and replacing the
    /// leftmost is what a reader pressing a third card expects.
    pub(crate) fn toggle_pick(&mut self, pane: &str) {
        if let Some(ix) = self.picked.iter().position(|picked| picked == pane) {
            self.picked.remove(ix);
            self.tails.remove(pane);
            return;
        }
        self.picked.push(pane.to_owned());
        while self.picked.len() > 2 {
            let dropped = self.picked.remove(0);
            self.tails.remove(&dropped);
        }
    }

    /// Open the comparison on `pane`: the card's terminal button means "this
    /// one, and one more". The mode switch is part of the same press.
    pub(crate) fn open_tail(&mut self, pane: &str) {
        self.mode = Mode::Compare;
        if !self.is_picked(pane) {
            self.toggle_pick(pane);
        }
    }

    /// Drop every pick and the tails read for them; the next selection starts
    /// from a clean slate rather than showing output for a pane nobody asked
    /// about any more.
    pub(crate) fn clear_picks(&mut self) {
        self.picked.clear();
        self.tails.clear();
    }

    /// Take a read request, if there is one to take. Two picks are what a
    /// comparison is; one is a selection still being made. A read already in
    /// flight is not stacked (a second press would only race the first).
    pub(crate) fn begin_read(&mut self) -> Option<(u64, Vec<String>)> {
        if self.picked.len() != 2 || self.reading {
            return None;
        }
        self.generation += 1;
        self.reading = true;
        Some((self.generation, self.picked.clone()))
    }

    /// Fold a read's answer in. An answer to a superseded request is dropped
    /// rather than shown over a newer one.
    pub(crate) fn finish_read(&mut self, generation: u64, tails: Tails) -> bool {
        if generation != self.generation {
            return false;
        }
        self.reading = false;
        self.tails = tails;
        true
    }
}

/// What a body needs from [`Root`], one borrow per source.
pub(crate) struct Sources<'a> {
    pub(crate) state: &'a State,
    pub(crate) live: &'a LiveState,
    pub(crate) stream: &'a FleetState,
    pub(crate) connection: &'a Connection,
}

/// The tab's body: a head with the live count and the mode chips, then the
/// roster or the comparison.
pub(crate) fn body(sources: Sources<'_>, root: &Entity<Root>, cx: &App) -> AnyElement {
    let Some(snapshot) = sources.live.snapshot.as_deref() else {
        return hint_block(t("shell.views.no_data"), cx);
    };
    let agents = sidebar::ordered_agents(snapshot);
    let now = events::now_unix_ms();
    let running = sidebar::active_agents(snapshot);
    let mut column =
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .child(head(sources.state, running, root, cx));
    let content = match sources.state.mode() {
        Mode::Roster => roster(sources.state, &sources, snapshot, &agents, now, root, cx),
        Mode::Compare => compare(sources.state, &sources, snapshot, &agents, now, root, cx),
    };
    column = column.child(content);
    column.into_any_element()
}

/// The 34px head under the tab strip: what the view names, how many are
/// running, and the two mode chips. The count is the snapshot's, the word is
/// the fleet's.
fn head(state: &State, running: usize, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut chips = div().flex().flex_none().items_center().gap(px(4.));
    for mode in [Mode::Roster, Mode::Compare] {
        let chosen = state.mode() == mode;
        let root = root.clone();
        chips = chips.child(chip(
            SharedString::from(format!("fleet-mode-{}", mode.id())),
            mode.label(),
            chosen,
            move |_, _, cx| {
                root.update(cx, |root, cx| root.fleet_set_mode(mode, cx));
            },
            cx,
        ));
    }
    // A count of none is a quiet badge: green would promise activity that is
    // not there.
    let running_tone = if running > 0 {
        Tone::Success
    } else {
        Tone::Neutral
    };
    div()
        .h(px(34.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .border_b_1()
        .border_color(colors.border)
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Sm))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.fg)
                .child(t("shell.view.fleet")),
        )
        .child(
            Badge::new(rust_i18n::t!("shell.fleet.running", count = running).to_string())
                .tone(running_tone)
                .dot(),
        )
        .child(div().flex_1())
        .child(chips)
        .into_any_element()
}

/// The roster: the cards, the transitions this client read, and the line that
/// names where both come from.
fn roster(
    state: &State,
    sources: &Sources<'_>,
    snapshot: &ClientShellSnapshot,
    agents: &[&ClientShellAgent],
    now: u64,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut cards = SimpleGrid::new("fleet-cards", px(CARD_MIN), px(9.));
    for agent in agents {
        cards = cards.child(card(agent, sources, state, now, root, cx));
    }
    let mut column = div().flex().flex_col().gap(px(10.)).p(px(10.)).child(cards);
    if let Some(error) = sources.stream.error.as_deref() {
        column = column.child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.danger)
                .child(Ellipsis::new(SharedString::from(
                    rust_i18n::t!("shell.fleet.stale", error = error).to_string(),
                ))),
        );
    }
    if agents.is_empty() {
        column = column.child(hint_block(t("fleet.empty"), cx));
    } else {
        column = column.child(recent(sources.stream, snapshot, now, cx));
    }
    column = column.child(source_note(cx));
    div()
        .flex_1()
        .min_h_0()
        .child(
            ScrollArea::new("fleet-roster")
                .vertical()
                .shadows()
                .size_full()
                .child(column),
        )
        .into_any_element()
}

/// One card: dot, name and held age; the pane line; the transition strip; the
/// status and the two presses. The accent border is the pick for TAIL.
fn card(
    agent: &ClientShellAgent,
    sources: &Sources<'_>,
    state: &State,
    now: u64,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let hover = colors.hover;
    let pane = agent.pane_id.clone();
    let (status, since) = pane_status(agent, sources.stream);
    let picked = state.is_picked(&pane);
    let name = sidebar::agent_name(agent);
    let role = agent.agent.clone().filter(|role| *role != name);
    let age = since
        .map(|since| events::age(now, since))
        .unwrap_or_default();
    let strip = strip(&pane, &status, sources.stream, cx);

    let pick = root.clone();
    let pick_pane = pane.clone();
    let focus = sources.connection.clone();
    let focus_pane = pane.clone();
    let tail = root.clone();
    let tail_pane = pane.clone();
    div()
        .id(SharedString::from(format!("fleet-card-{pane}")))
        .w_full()
        .flex()
        .flex_col()
        .gap(px(7.))
        .p(px(10.))
        .rounded(theme.radius(Radius::Xl))
        .border_1()
        .border_color(if picked { colors.accent } else { colors.border })
        .bg(colors.surface)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(move |_, _, cx| {
            let pane = pick_pane.clone();
            pick.update(cx, |root, cx| root.fleet_toggle_pick(&pane, cx));
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(7.))
                .child(dot(sidebar::status_color_of(&status, cx)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Sm))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.fg)
                        .child(Ellipsis::new(SharedString::from(name))),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(age),
                ),
        )
        .child(
            div()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(Ellipsis::new(SharedString::from(match role {
                    Some(role) => format!("{pane} · {role}"),
                    None => pane.clone(),
                }))),
        )
        .child(strip)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(badge(&status))
                .child(div().flex_1())
                .child(
                    IconButton::new(
                        SharedString::from(format!("fleet-focus-{pane}")),
                        IconName::Eye,
                    )
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .tooltip(t("shell.fleet.focus"))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        focus.focus_pane(&focus_pane);
                    }),
                )
                .child(
                    IconButton::new(
                        SharedString::from(format!("fleet-tail-{pane}")),
                        IconName::Terminal,
                    )
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .tooltip(t("shell.fleet.tail"))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        let pane = tail_pane.clone();
                        tail.update(cx, |root, cx| root.fleet_open_tail(&pane, cx));
                    }),
                ),
        )
        .into_any_element()
}

/// The transition strip: the oldest status this client read first, the held
/// status last, and the unwatched slots as hairlines. A pane the stream has
/// not seen move has one filled slot — the status it is in — not a history
/// invented for it.
fn strip(pane: &str, current: &str, stream: &FleetState, cx: &App) -> AnyElement {
    let colors = &cx.theme().colors;
    let mut row = div().flex().gap(px(2.)).h(px(8.));
    for status in strip_slots(stream, pane, current) {
        let color = match status {
            // The status's own colour at full strength: an idle slot still has
            // to read as filled against the hairline the unwatched slots use.
            Some(status) => sidebar::status_color_of(&status, cx),
            None => colors.border,
        };
        row = row.child(
            div()
                .flex_1()
                .h_full()
                .rounded(px(2.))
                .bg(color)
                .into_any_element(),
        );
    }
    row.into_any_element()
}

/// The strip's slots, left to right: the watched transitions oldest first,
/// then the status held now, padded on the left with the slots this client
/// never saw. A transition whose `to` is the status held now is the current
/// segment, not a repeated one, so it is folded into the last slot.
fn strip_slots(stream: &FleetState, pane: &str, current: &str) -> Vec<Option<String>> {
    let mut events: Vec<String> = stream
        .history(pane, STRIP - 1)
        .into_iter()
        .map(|transition| transition.to.clone())
        .collect();
    if events.first().is_some_and(|newest| newest == current) {
        events.remove(0);
    }
    events.reverse();
    events.push(current.to_owned());
    let hidden = events.len().saturating_sub(STRIP);
    events.drain(..hidden);
    let mut slots = vec![None; STRIP];
    let start = STRIP - events.len();
    for (ix, status) in events.into_iter().enumerate() {
        slots[start + ix] = Some(status);
    }
    slots
}

/// A status as the panel writes it: the color's word and the same tone.
fn badge(status: &str) -> AnyElement {
    Badge::new(sidebar::status_word(status))
        .tone(tone(status))
        .dot()
        .into_any_element()
}

fn tone(status: &str) -> Tone {
    match status {
        "working" => Tone::Info,
        "blocked" => Tone::Warning,
        "done" => Tone::Success,
        _ => Tone::Neutral,
    }
}

/// The transitions under the cards, newest first, each naming the agent its
/// pane belongs to.
fn recent(stream: &FleetState, snapshot: &ClientShellSnapshot, now: u64, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut column = div().flex().flex_col().gap(px(5.));
    let head = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            Icon::new(IconName::RefreshCw)
                .size(IconSize::Xs)
                .color(colors.fg_subtle),
        )
        .child(
            div()
                .flex_1()
                .text_size(theme.text_size(TextSize::Xs))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.fg_subtle)
                .child(t("shell.fleet.recent")),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(stream.transitions.len().to_string()),
        );
    column = column.child(head);
    for transition in stream.transitions.iter().take(RECENT) {
        let name = snapshot
            .agents
            .iter()
            .find(|agent| agent.pane_id == transition.pane_id)
            .map(sidebar::agent_name)
            // The pane may have left the fleet; the event still named the CLI
            // that moved, so the row says that rather than a bare pane id.
            .or_else(|| transition.agent.clone())
            .unwrap_or_else(|| transition.pane_id.clone());
        column = column.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(dot(sidebar::status_color_of(&transition.to, cx)))
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(events::age(now, transition.at_unix_ms)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_muted)
                        .child(Ellipsis::new(SharedString::from(name))),
                )
                .child(badge(&transition.to)),
        );
    }
    column.into_any_element()
}

/// Where the rows come from, in the design's own sentence. The event name is
/// the wire's and stays mono; the sentence around it is the reader's.
fn source_note(cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(2.))
        .p(px(8.))
        .rounded(theme.radius(Radius::Lg))
        .border_1()
        .border_color(colors.border)
        .bg(colors.surface)
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(colors.fg_subtle)
        .child(t("shell.fleet.source_prefix"))
        .child(
            div()
                .text_color(colors.fg_muted)
                .child("pane.agent_status_changed"),
        )
        .child(t("shell.fleet.source_suffix"))
        .into_any_element()
}

/// The TAIL tab: the selection until two panes are picked, then the two
/// columns.
fn compare(
    state: &State,
    sources: &Sources<'_>,
    snapshot: &ClientShellSnapshot,
    agents: &[&ClientShellAgent],
    now: u64,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    if state.picked().len() < 2 {
        return selection(state, sources, agents, root, cx);
    }
    let left = state.picked()[0].clone();
    let right = state.picked()[1].clone();
    let left_tail = state.tails().get(&left);
    let right_tail = state.tails().get(&right);
    let diff = diff_marks(
        left_tail.map(|tail| tail.lines.as_slice()).unwrap_or(&[]),
        right_tail.map(|tail| tail.lines.as_slice()).unwrap_or(&[]),
    );
    let left_name = name_of(snapshot, &left);
    let right_name = name_of(snapshot, &right);
    let head = compare_head(&left_name, &right_name, state, root, cx);
    let columns = div()
        .flex()
        .items_start()
        .gap(px(6.))
        .p(px(10.))
        .child(tail_column(
            &left_name,
            agent_status(snapshot, sources.stream, &left),
            left_tail,
            &diff.left,
            state.reading(),
            now,
            cx,
        ))
        .child(tail_column(
            &right_name,
            agent_status(snapshot, sources.stream, &right),
            right_tail,
            &diff.right,
            state.reading(),
            now,
            cx,
        ));
    let scroll = ScrollArea::new("fleet-compare")
        .vertical()
        .shadows()
        .size_full()
        .child(columns);
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(head)
        .child(div().flex_1().min_h_0().child(scroll))
        .child(compare_foot(state, &diff, sources, cx))
        .into_any_element()
}

/// The comparison's head: what is being compared, and the two presses that
/// change it.
fn compare_head(
    left: &str,
    right: &str,
    state: &State,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let reselect = root.clone();
    let refresh = root.clone();
    div()
        .h(px(34.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .border_b_1()
        .border_color(colors.border)
        .child(
            Icon::new(IconName::Terminal)
                .size(IconSize::Xs)
                .color(colors.fg_subtle),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.fg_muted)
                .child(t("fleet.tail_compare")),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(Ellipsis::new(SharedString::from(format!(
                    "{left} · {right}"
                )))),
        )
        .child(chip(
            "fleet-reselect",
            t("shell.fleet.reselect"),
            false,
            move |_, _, cx| {
                reselect.update(cx, |root, cx| root.fleet_clear_picks(cx));
            },
            cx,
        ))
        .child(
            IconButton::new("fleet-refresh", IconName::RefreshCw)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip(t("shell.fleet.refresh"))
                .on_click(move |_, _, cx| {
                    refresh.update(cx, |root, cx| root.fleet_read(cx));
                }),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(
                    rust_i18n::t!("shell.fleet.picked", count = state.picked().len()).to_string(),
                ),
        )
        .into_any_element()
}

/// One agent's tail: a head naming it, the read's age, then the lines with the
/// ones the other column does not share highlighted. Reading, failure and
/// emptiness are each stated in place.
fn tail_column(
    name: &str,
    status: Option<String>,
    tail: Option<&Tail>,
    marks: &[bool],
    reading: bool,
    now: u64,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut head = div()
        .flex()
        .items_center()
        .gap(px(5.))
        .pb(px(4.))
        .child(match status.as_deref() {
            Some(status) => dot(sidebar::status_color_of(status, cx)),
            // The pane left the fleet: its column still names the pane, and
            // the read below states what the daemon says now.
            None => dot(colors.fg_disabled),
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Xs))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.fg)
                .child(Ellipsis::new(SharedString::from(name.to_owned()))),
        );
    if let Some(tail) = tail {
        head = head.child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(
                    rust_i18n::t!(
                        "shell.fleet.read_at",
                        age = events::age(now, tail.at_unix_ms)
                    )
                    .to_string(),
                ),
        );
    }
    let mut body = div().flex().flex_col().gap(px(1.));
    match tail {
        Some(tail) if tail.error.is_some() => {
            let error = tail.error.clone().unwrap_or_default();
            body = body.child(
                div()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.danger)
                    .child(rust_i18n::t!("shell.fleet.failed", error = error).to_string()),
            );
        }
        Some(tail) if tail.lines.is_empty() => {
            body = body.child(
                div()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_subtle)
                    .child(t("fleet.tail_empty")),
            );
        }
        Some(tail) => {
            for (ix, line) in tail.lines.iter().enumerate() {
                let differs = marks.get(ix).copied().unwrap_or(false);
                body = body.child(tail_line(line, differs, cx));
            }
        }
        None if reading => {
            body = body.child(
                div()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_subtle)
                    .child(t("shell.fleet.reading")),
            );
        }
        None => {
            body = body.child(
                div()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_subtle)
                    .child(t("fleet.tail_empty")),
            );
        }
    }
    if status.is_none() {
        body = body.child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(t("shell.fleet.not_running")),
        );
    }
    div()
        .flex_1()
        .min_w_0()
        .p(px(7.))
        .rounded(theme.radius(Radius::Lg))
        .border_1()
        .border_color(colors.border)
        .bg(colors.sunken)
        .child(head)
        .child(body)
        .into_any_element()
}

/// One tail row: mono, cut at the column's edge, and washed in the selection
/// colour when the other column does not carry it.
fn tail_line(line: &str, differs: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .min_w_0()
        .px(px(2.))
        .text_size(theme.text_size(TextSize::Xs))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_ellipsis()
        .text_color(if differs { colors.fg } else { colors.fg_muted })
        .when(differs, |line| line.bg(colors.selection))
        .child(line.to_owned())
        .into_any_element()
}

/// The comparison's foot: how many rows differ, how many were read, and the
/// stream's own state when it is not connected.
fn compare_foot(state: &State, diff: &Diff, sources: &Sources<'_>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let rows = READ_ROWS;
    let summary = if state.reading() {
        t("shell.fleet.reading")
    } else if diff.count == 0 {
        rust_i18n::t!("shell.fleet.no_diff", rows = rows).to_string()
    } else {
        rust_i18n::t!("shell.fleet.diff", count = diff.count, rows = rows).to_string()
    };
    let mut foot = div()
        .h(px(26.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .border_t_1()
        .border_color(colors.border)
        .child(
            Icon::new(if diff.count == 0 {
                IconName::Check
            } else {
                IconName::CircleDot
            })
            .size(IconSize::Xs)
            .color(colors.fg_subtle),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(Ellipsis::new(SharedString::from(summary))),
        );
    if let Some(error) = sources.stream.error.as_deref() {
        foot = foot.child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.danger)
                .child(Ellipsis::new(SharedString::from(
                    rust_i18n::t!("shell.fleet.stale", error = error).to_string(),
                ))),
        );
    }
    foot.into_any_element()
}

/// The comparison before two panes are picked: what to do, and the agents to
/// do it with. A row is a toggle, so a mistaken pick is one press away from
/// undoing itself.
fn selection(
    state: &State,
    sources: &Sources<'_>,
    agents: &[&ClientShellAgent],
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut column = div().flex().flex_col().gap(px(6.)).p(px(10.)).child(
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                Icon::new(IconName::Terminal)
                    .size(IconSize::Xs)
                    .color(colors.fg_subtle),
            )
            .child(
                div()
                    .text_size(theme.text_size(TextSize::Xs))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.fg_muted)
                    .child(t("fleet.tail_compare")),
            ),
    );
    let hint = if state.picked().is_empty() {
        t("shell.fleet.pick_hint")
    } else {
        rust_i18n::t!("shell.fleet.pick_more", count = state.picked().len()).to_string()
    };
    column = column.child(
        div()
            .text_size(theme.text_size(TextSize::Xs))
            .text_color(colors.fg_subtle)
            .child(hint),
    );
    if agents.is_empty() {
        column = column.child(hint_block(t("fleet.empty"), cx));
    } else {
        for agent in agents {
            column = column.child(selection_row(agent, sources.stream, state, root, cx));
        }
    }
    div()
        .flex_1()
        .min_h_0()
        .child(
            ScrollArea::new("fleet-selection")
                .vertical()
                .shadows()
                .size_full()
                .child(column),
        )
        .into_any_element()
}

fn selection_row(
    agent: &ClientShellAgent,
    stream: &FleetState,
    state: &State,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let hover = colors.hover;
    let pane = agent.pane_id.clone();
    let (status, _) = pane_status(agent, stream);
    let picked = state.is_picked(&pane);
    let name = sidebar::agent_name(agent);
    let row = root.clone();
    let row_pane = pane.clone();
    div()
        .id(SharedString::from(format!("fleet-pick-{pane}")))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .h(px(28.))
        .px(px(7.))
        .rounded(theme.radius(Radius::Md))
        .border_1()
        .border_color(if picked { colors.accent } else { colors.border })
        .bg(if picked { colors.hover } else { colors.surface })
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(move |_, _, cx| {
            let pane = row_pane.clone();
            row.update(cx, |root, cx| root.fleet_toggle_pick(&pane, cx));
        })
        .child(dot(sidebar::status_color_of(&status, cx)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Sm))
                .text_color(colors.fg)
                .child(Ellipsis::new(SharedString::from(name))),
        )
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(pane),
        )
        .child(badge(&status))
        .into_any_element()
}

/// A status chip: the mode switch's and the comparison's "pick again" share
/// one drawing, the way the event filter's chips do.
fn chip(
    id: impl Into<SharedString>,
    label: String,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let hover = colors.hover;
    div()
        .id(id.into())
        .flex_none()
        .h(px(20.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(theme.radius(Radius::Sm))
        .border_1()
        .border_color(if active {
            colors.border_strong
        } else {
            colors.border
        })
        .bg(if active { colors.active } else { colors.bg })
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(if active { colors.fg } else { colors.fg_muted })
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(on_click)
        .child(label)
        .into_any_element()
}

fn hint_block(text: String, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .p(px(8.))
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(theme.colors.fg_subtle)
        .child(text)
        .into_any_element()
}

fn dot(color: Hsla) -> AnyElement {
    div()
        .flex_none()
        .size(px(6.))
        .rounded_full()
        .bg(color)
        .into_any_element()
}

/// A pane's status as the fleet stream saw it, with the moment it was entered
/// when this client watched it move. A pane the stream has not heard from
/// keeps the snapshot's status and no held time — nothing is invented.
fn pane_status(agent: &ClientShellAgent, stream: &FleetState) -> (String, Option<u64>) {
    match stream.watched(&agent.pane_id) {
        Some(watched) => (
            watched.status.clone(),
            (watched.since_unix_ms > 0).then_some(watched.since_unix_ms),
        ),
        None => (sidebar::status_str(agent.agent_status).to_owned(), None),
    }
}

/// A pane's status by id, for a comparison column: the stream's word when it
/// has one, else the snapshot's, else `None` when the pane left the fleet.
fn agent_status(snapshot: &ClientShellSnapshot, stream: &FleetState, pane: &str) -> Option<String> {
    if let Some(watched) = stream.watched(pane) {
        return Some(watched.status.clone());
    }
    snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane)
        .map(|agent| sidebar::status_str(agent.agent_status).to_owned())
}

fn name_of(snapshot: &ClientShellSnapshot, pane: &str) -> String {
    snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane)
        .map(sidebar::agent_name)
        .unwrap_or_else(|| pane.to_owned())
}

/// Which lines each column does not share with the other. The tails are
/// aligned from their ends, because that is what a tail is: the last line of
/// each is the newest, and two runs of different lengths differ where one has
/// output the other does not. `count` is the number of aligned positions that
/// differ, not the number of highlighted rows (a mismatch marks both sides).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Diff {
    pub(crate) left: Vec<bool>,
    pub(crate) right: Vec<bool>,
    pub(crate) count: usize,
}

pub(crate) fn diff_marks(left: &[String], right: &[String]) -> Diff {
    let mut diff = Diff {
        left: vec![false; left.len()],
        right: vec![false; right.len()],
        count: 0,
    };
    for offset in 0..left.len().max(right.len()) {
        let l = left.len().checked_sub(offset + 1);
        let r = right.len().checked_sub(offset + 1);
        match (l, r) {
            (Some(l), Some(r)) => {
                if left[l] != right[r] {
                    diff.left[l] = true;
                    diff.right[r] = true;
                    diff.count += 1;
                }
            }
            (Some(l), None) => {
                diff.left[l] = true;
                diff.count += 1;
            }
            (None, Some(r)) => {
                diff.right[r] = true;
                diff.count += 1;
            }
            (None, None) => break,
        }
    }
    diff
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::fleet::Transition;

    fn transition(pane: &str, at: u64, from: Option<&str>, to: &str) -> Transition {
        Transition {
            at_unix_ms: at,
            pane_id: pane.to_owned(),
            agent: None,
            from: from.map(str::to_owned),
            to: to.to_owned(),
        }
    }

    fn stream(transitions: Vec<Transition>) -> FleetState {
        FleetState {
            transitions: VecDeque::from(transitions),
            current: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn picking_a_third_pane_drops_the_oldest() {
        let mut state = State::default();
        state.toggle_pick("w1:p1");
        state.toggle_pick("w1:p2");
        assert!(state.is_picked("w1:p1"));
        state.toggle_pick("w1:p3");
        assert_eq!(state.picked, ["w1:p2", "w1:p3"]);
        assert!(!state.is_picked("w1:p1"));
    }

    #[test]
    fn picking_the_same_pane_again_unpicks_it() {
        let mut state = State::default();
        state.toggle_pick("w1:p1");
        state.toggle_pick("w1:p2");
        state.toggle_pick("w1:p1");
        assert_eq!(state.picked, ["w1:p2"]);
    }

    #[test]
    fn opening_a_tail_switches_mode_and_includes_the_pane() {
        let mut state = State::default();
        state.open_tail("w1:p1");
        assert_eq!(state.mode(), Mode::Compare);
        assert!(state.is_picked("w1:p1"));
        // Opening the same card's tail again does not double-pick it.
        state.open_tail("w1:p1");
        assert_eq!(state.picked, ["w1:p1"]);
    }

    #[test]
    fn a_read_needs_two_picks_and_no_read_in_flight() {
        let mut state = State::default();
        state.toggle_pick("w1:p1");
        assert!(state.begin_read().is_none());
        state.toggle_pick("w1:p2");
        let (generation, panes) = state.begin_read().expect("two picks");
        assert_eq!(panes, ["w1:p1", "w1:p2"]);
        assert!(state.reading());
        assert!(state.begin_read().is_none());
        assert!(state.finish_read(generation, Tails::new()));
        assert!(!state.reading());
    }

    #[test]
    fn an_answer_to_a_superseded_read_is_dropped() {
        let mut state = State::default();
        state.toggle_pick("w1:p1");
        state.toggle_pick("w1:p2");
        let (generation, _) = state.begin_read().expect("two picks");
        assert!(!state.finish_read(generation + 1, Tails::new()));
        assert!(state.tails().is_empty());
    }

    #[test]
    fn clearing_the_picks_drops_their_tails() {
        let mut state = State::default();
        state.toggle_pick("w1:p1");
        state.toggle_pick("w1:p2");
        let (generation, _) = state.begin_read().expect("two picks");
        state.finish_read(generation, Tails::new());
        state.clear_picks();
        assert!(state.picked().is_empty());
        assert!(state.tails().is_empty());
    }

    #[test]
    fn the_strip_ends_in_the_held_status_without_repeating_it() {
        // Newest first, as the stream's ring stores them.
        let state = stream(vec![
            transition("w1:p1", 30, Some("idle"), "working"),
            transition("w1:p1", 20, None, "idle"),
        ]);
        let slots = strip_slots(&state, "w1:p1", "working");
        assert_eq!(slots.len(), STRIP);
        assert_eq!(slots[STRIP - 2].as_deref(), Some("idle"));
        assert_eq!(slots[STRIP - 1].as_deref(), Some("working"));
        assert!(slots[..STRIP - 2].iter().all(Option::is_none));
    }

    #[test]
    fn a_strip_whose_newest_transition_is_not_the_held_status_keeps_both() {
        let state = stream(vec![transition("w1:p1", 30, Some("idle"), "working")]);
        let slots = strip_slots(&state, "w1:p1", "blocked");
        assert_eq!(slots[STRIP - 2].as_deref(), Some("working"));
        assert_eq!(slots[STRIP - 1].as_deref(), Some("blocked"));
    }

    #[test]
    fn a_strip_with_no_history_is_one_status_and_empty_slots() {
        let slots = strip_slots(&stream(Vec::new()), "w1:p1", "idle");
        assert_eq!(slots[STRIP - 1].as_deref(), Some("idle"));
        assert!(slots[..STRIP - 1].iter().all(Option::is_none));
    }

    #[test]
    fn a_strip_longer_than_its_slots_keeps_the_newest() {
        let mut transitions = Vec::new();
        for ix in 0..12 {
            transitions.push(transition(
                "w1:p1",
                100 + ix,
                None,
                if ix % 2 == 0 { "idle" } else { "working" },
            ));
        }
        transitions.reverse();
        let state = stream(transitions);
        let slots = strip_slots(&state, "w1:p1", "working");
        assert_eq!(slots.len(), STRIP);
        // The newest transition is the held status and folds into the last
        // slot; six older ones plus that leave one unwatched slot at the left.
        assert!(slots[0].is_none());
        assert!(slots[1..].iter().all(Option::is_some));
    }

    #[test]
    fn identical_tails_have_no_differences() {
        let lines = vec!["one".to_owned(), "two".to_owned()];
        let diff = diff_marks(&lines, &lines);
        assert_eq!(diff.count, 0);
        assert!(diff.left.iter().all(|mark| !mark));
        assert!(diff.right.iter().all(|mark| !mark));
    }

    #[test]
    fn a_mismatched_line_is_marked_on_both_sides() {
        let left = vec!["one".to_owned(), "two".to_owned()];
        let right = vec!["one".to_owned(), "TWO".to_owned()];
        let diff = diff_marks(&left, &right);
        assert_eq!(diff.count, 1);
        assert_eq!(diff.left, [false, true]);
        assert_eq!(diff.right, [false, true]);
    }

    #[test]
    fn a_longer_tail_marks_only_its_extra_lines() {
        let left = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let right = vec!["b".to_owned(), "c".to_owned()];
        let diff = diff_marks(&left, &right);
        assert_eq!(diff.count, 1);
        assert_eq!(diff.left, [true, false, false]);
        assert_eq!(diff.right, [false, false]);
        // The extra line is at the top: tails line up from the end.
        let left = vec!["b".to_owned(), "c".to_owned()];
        let right = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let diff = diff_marks(&left, &right);
        assert_eq!(diff.count, 1);
        assert_eq!(diff.left, [false, false]);
        assert_eq!(diff.right, [true, false, false]);
    }
}
