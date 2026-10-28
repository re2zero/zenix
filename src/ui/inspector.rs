//! The event inspector: one streamed event's own line, and the contract the
//! stream is written against.
//!
//! The sidebar's events view is the stream; pressing a row opens this 420px
//! drawer (`docs/ui/redesign` `.drawer.w420`) on that event. The daemon sends
//! one JSON line per event and no timestamp, so the three tabs are honest
//! about what exists: the parsed envelope, the line as it arrived byte for
//! byte, and what this client knows about the kind — its two spellings and
//! whether the session-wide subscription can carry it.
//!
//! The contract is the same table the stream subscribes from
//! ([`crate::events::SUBSCRIBED`]), drawn as the daemon names the events on
//! the wire and as a manifest's `[[events]] on` names them. The list is copied
//! for a plugin author, who needs exactly those two spellings.
//!
//! Neither surface polls: a captured event is a copy, and the contract is a
//! constant. Escape, the close button and a press outside the drawer put it
//! away.

use ely_core::{
    buttons::{Button, ButtonVariant, IconButton},
    layout::{ScrollArea, SimpleGrid},
    primitives::{Backdrop, FocusScope, Icon, IconName, give_back, take_focus},
    theme::{ActiveTheme, ControlSize, Elevation, IconSize, Radius, TextSize},
    typography::Ellipsis,
};
use gpui::{
    AnyElement, App, ClipboardItem, Entity, FocusHandle, IntoElement, MouseButton, ParentElement,
    SharedString, Styled, Window, div, prelude::*, px,
};

use super::shell::Root;
use crate::events::{self, Event, EventState};
use crate::i18n::t;
use crate::plugins::host::PluginState;

/// The inspector's width and the shell edges it keeps visible.
const WIDTH: f32 = 420.;
const TOP: f32 = 38.;
const BOTTOM: f32 = 26.;
/// The contract's card: the design's 660px over a scrim.
const CONTRACT_WIDTH: f32 = 660.;

/// The drawer's three tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Json,
    Raw,
    Contract,
}

impl Tab {
    pub(crate) const ALL: &'static [Tab] = &[Tab::Json, Tab::Raw, Tab::Contract];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Raw => "raw",
            Self::Contract => "contract",
        }
    }

    fn label(self) -> String {
        t(&format!("shell.inspector.tab.{}", self.id()))
    }
}

/// What the inspector holds: the event it was opened on, the tab in front, and
/// whether the contract is showing over it.
#[derive(Default)]
pub(crate) struct State {
    open: bool,
    selected: Option<Event>,
    tab: Tab,
    contract: bool,
}

impl State {
    /// Open on one event, keeping the tab the reader last read. A copy is
    /// taken, not an index: the ring keeps moving under a selected row.
    pub(crate) fn open(&mut self, event: Event) {
        self.open = true;
        self.selected = Some(event);
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.selected = None;
        self.contract = false;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn selected(&self) -> Option<&Event> {
        self.selected.as_ref()
    }

    pub(crate) fn tab(&self) -> Tab {
        self.tab
    }

    pub(crate) fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    pub(crate) fn contract_open(&self) -> bool {
        self.contract
    }

    pub(crate) fn open_contract(&mut self) {
        self.contract = true;
    }

    pub(crate) fn close_contract(&mut self) {
        self.contract = false;
    }
}

/// What the drawer reads, one borrow per source.
pub(crate) struct Sources<'a> {
    pub(crate) state: &'a State,
    pub(crate) plugins: &'a PluginState,
    pub(crate) stream: &'a EventState,
    /// The shell's handle for the drawer: opening focuses it and closing hands
    /// the keyboard back to the grid.
    pub(crate) focus: &'a FocusHandle,
    /// A modal surface is above the drawer (the contract it opened). A press
    /// inside that surface is not a press outside this one.
    pub(crate) blocked: bool,
}

/// The drawer over the shell, or nothing while it is closed.
pub(crate) fn overlay(
    sources: Sources<'_>,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &App,
) -> Option<AnyElement> {
    let Sources {
        state,
        plugins,
        stream,
        focus,
        blocked,
    } = sources;
    let event = state.selected()?;
    let theme = cx.theme();
    let colors = &theme.colors;
    let width = px(WIDTH).min(window.viewport_size().width * 0.94);
    let close = {
        let root = root.clone();
        move |window: &mut Window, cx: &mut App| {
            root.update(cx, |root, cx| root.close_inspector(window, cx));
        }
    };
    let panel = div()
        .id("event-inspector")
        .track_focus(focus)
        .absolute()
        .top(px(TOP))
        .bottom(px(BOTTOM))
        .right_0()
        .w(width)
        .flex()
        .flex_col()
        .overflow_hidden()
        .border_l_1()
        .border_color(colors.border_strong)
        .bg(colors.bg)
        .shadow(theme.elevation(Elevation::Modal))
        .on_key_down({
            let close = close.clone();
            move |event, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    close(window, cx);
                }
            }
        })
        .when(!blocked, |panel| {
            panel.on_mouse_down_out({
                let close = close.clone();
                move |event, window, cx| {
                    if event.button == MouseButton::Left {
                        close(window, cx);
                    }
                }
            })
        })
        .child(head(event, root, cx))
        .child(strip(state.tab, root, cx))
        .child(
            div().flex_1().min_h_0().child(
                ScrollArea::new("event-inspector-body")
                    .vertical()
                    .shadows()
                    .size_full()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .child(match state.tab {
                                Tab::Json => code(&pretty(event), cx),
                                Tab::Raw => code(&event.raw, cx),
                                Tab::Contract => contract_facts(event, plugins, root, cx),
                            })
                            .child(metadata(event, plugins, stream, cx)),
                    ),
            ),
        )
        .child(foot(event, cx));
    Some(
        div()
            .id("event-inspector-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .child(panel)
            .into_any_element(),
    )
}

/// The head: which event this is, and the close button.
fn head(event: &Event, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let close = root.clone();
    div()
        .h(px(40.))
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .border_b_1()
        .border_color(colors.border)
        .child(
            Icon::new(IconName::Activity)
                .size(IconSize::Sm)
                .color(colors.warning),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg)
                .child(Ellipsis::new(SharedString::from(event.kind.clone()))),
        )
        .child(
            IconButton::new("event-inspector-close", IconName::X)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .on_click(move |_, window, cx| {
                    close.update(cx, |root, cx| root.close_inspector(window, cx));
                }),
        )
        .into_any_element()
}

/// The three reading modes.
fn strip(tab: Tab, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut row = div()
        .h(px(34.))
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .border_b_1()
        .border_color(colors.border);
    for candidate in Tab::ALL {
        let active = *candidate == tab;
        let choose = root.clone();
        let chosen = *candidate;
        row = row.child(
            div()
                .id(SharedString::from(format!(
                    "event-inspector-tab-{}",
                    candidate.id()
                )))
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .rounded(theme.radius(Radius::Sm))
                .text_size(theme.text_size(TextSize::Xs))
                .cursor_pointer()
                .when(active, |chip| chip.bg(colors.surface).text_color(colors.fg))
                .when(!active, |chip| {
                    chip.text_color(colors.fg_muted)
                        .hover(move |style| style.bg(colors.hover))
                })
                .on_click(move |_, _, cx| {
                    choose.update(cx, |root, cx| root.set_inspector_tab(chosen, cx));
                })
                .child(candidate.label()),
        );
    }
    row.into_any_element()
}

/// A block of text in the mono face, on the sunken ground the design gives
/// code. Long lines wrap rather than being clipped: a payload's tail matters.
fn code(text: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .rounded(theme.radius(Radius::Md))
        .border_1()
        .border_color(colors.border)
        .bg(colors.surface)
        .p_2()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(colors.fg_muted)
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

/// The contract tab: this kind's two spellings, whether the stream can carry
/// it, and the way to the full table.
fn contract_facts(
    event: &Event,
    plugins: &PluginState,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let known = events::find(&event.wire);
    let hooked = |kind: &str| plugins.plugins.iter().any(|plugin| plugin.hooks_on(kind));
    let mut column = div()
        .flex()
        .flex_col()
        .gap_1p5()
        .rounded(theme.radius(Radius::Md))
        .border_1()
        .border_color(colors.border)
        .p_2()
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(t("shell.inspector.contract")),
        )
        .child(kv(
            t("shell.inspector.kind_dotted"),
            mono(&event.kind, colors.fg, cx),
            cx,
        ))
        .child(kv(
            t("shell.inspector.kind_wire"),
            mono(&event.wire, colors.fg_muted, cx),
            cx,
        ))
        .child(kv(
            t("shell.inspector.class"),
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg)
                .child(events::class(event, &hooked).label())
                .into_any_element(),
            cx,
        ))
        .child(kv(
            t("shell.inspector.hooked"),
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(if hooked(&event.kind) {
                    colors.accent
                } else {
                    colors.fg_subtle
                })
                .child(if hooked(&event.kind) {
                    t("shell.inspector.hooked_yes")
                } else {
                    t("shell.inspector.hooked_no")
                })
                .into_any_element(),
            cx,
        ));
    column = column.child(
        div()
            .text_size(theme.text_size(TextSize::Xs))
            .text_color(if known.is_some() {
                colors.fg_subtle
            } else {
                colors.warning
            })
            .child(if known.is_some() {
                rust_i18n::t!(
                    "shell.inspector.subscribed",
                    count = events::SUBSCRIBED.len()
                )
                .to_string()
            } else {
                t("shell.inspector.unlisted")
            }),
    );
    let open = root.clone();
    column
        .child(
            Button::new(
                "event-inspector-contract",
                t("shell.inspector.open_contract"),
            )
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Sm)
            .on_click(move |_, _, cx| {
                open.update(cx, |root, cx| root.open_contract(cx));
            }),
        )
        .into_any_element()
}

/// The facts beside the payload: where it came from, what class it is, when it
/// arrived, and how many bytes the daemon wrote. The lane's boundary is stated
/// too — a reader should not expect a plugin's data to arrive here.
fn metadata(event: &Event, plugins: &PluginState, stream: &EventState, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let hooked = |kind: &str| plugins.plugins.iter().any(|plugin| plugin.hooks_on(kind));
    let now = events::now_unix_ms();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .rounded(theme.radius(Radius::Md))
        .border_1()
        .border_color(colors.border)
        .p_2()
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(colors.fg_muted)
                .child(t("shell.inspector.metadata")),
        )
        .child(kv(
            t("shell.inspector.source"),
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(if stream.error.is_none() {
                    colors.fg
                } else {
                    colors.danger
                })
                .child(if stream.error.is_none() {
                    t("shell.inspector.source_live")
                } else {
                    t("shell.inspector.source_down")
                })
                .into_any_element(),
            cx,
        ))
        .child(kv(
            t("shell.inspector.class"),
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg)
                .child(events::class(event, &hooked).label())
                .into_any_element(),
            cx,
        ))
        .child(kv(
            t("shell.inspector.arrived"),
            mono(&events::age(now, event.at_unix_ms), colors.fg_muted, cx),
            cx,
        ))
        .child(kv(
            t("shell.inspector.size"),
            mono(&format!("{} B", event.raw.len()), colors.fg_muted, cx),
            cx,
        ))
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(t("shell.inspector.lane")),
        )
        .into_any_element()
}

/// The foot: when the line arrived, and the way to take the JSON away.
fn foot(event: &Event, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let now = events::now_unix_ms();
    let json = pretty(event);
    div()
        .h(px(40.))
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .border_t_1()
        .border_color(colors.border)
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(events::age(now, event.at_unix_ms)),
        )
        .child(div().flex_1())
        .child(
            Button::new("event-inspector-copy", t("shell.inspector.copy"))
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(json.clone()));
                }),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// The contract
// ---------------------------------------------------------------------------

/// The 21-name contract over the shell. Drawn only while it is open, and it
/// takes the keyboard like the settings dialog: the list is a reference, not a
/// surface to type into.
pub(crate) fn contract_overlay(
    state: &State,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    if !state.contract_open() {
        return None;
    }
    let takeover = take_focus("contract-dialog-takeover", window, cx);
    let focus = takeover.read(cx).focus.clone();
    let theme = cx.theme();
    let colors = &theme.colors;
    let close = {
        let root = root.clone();
        let takeover = takeover.clone();
        move |window: &mut Window, cx: &mut App| {
            give_back(&takeover, window, cx);
            root.update(cx, |root, cx| root.close_contract(cx));
        }
    };
    let mut grid = SimpleGrid::new("contract-grid", px(150.), px(14.));
    for kind in events::SUBSCRIBED {
        grid = grid.child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg)
                        .child(kind.dotted),
                )
                .child(
                    div()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(kind.wire),
                ),
        );
    }
    let copy = events::SUBSCRIBED
        .iter()
        .map(|kind| format!("{}\t{}", kind.dotted, kind.wire))
        .collect::<Vec<_>>()
        .join("\n");
    let card = div()
        .w(px(CONTRACT_WIDTH).min(window.viewport_size().width - px(48.)))
        .max_h(window.viewport_size().height * 0.82)
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
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .pt_3()
                .child(
                    Icon::new(IconName::Link)
                        .size(IconSize::Sm)
                        .color(colors.accent),
                )
                .child(
                    div()
                        .text_size(theme.text_size(TextSize::Lg))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.fg)
                        .child(t("shell.inspector.contract_title")),
                )
                .child(chip(
                    rust_i18n::t!(
                        "shell.inspector.contract_badge",
                        count = events::SUBSCRIBED.len()
                    )
                    .to_string(),
                    colors.accent,
                    cx,
                ))
                .child(div().flex_1())
                .child({
                    let close = close.clone();
                    IconButton::new("contract-close", IconName::X)
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, window, cx| close(window, cx))
                }),
        )
        .child(
            div()
                .flex_none()
                .px_4()
                .pt_1()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(t("shell.inspector.contract_intro")),
        )
        .child(
            div().flex_1().min_h_0().child(
                ScrollArea::new("contract-body")
                    .vertical()
                    .shadows()
                    .size_full()
                    .child(div().p_4().child(grid)),
            ),
        )
        .child(
            div()
                .h(px(42.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .border_t_1()
                .border_color(colors.border)
                .child(
                    div()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(Ellipsis::new(SharedString::from(t("shell.inspector.lane")))),
                )
                .child(div().flex_1())
                .child(
                    Button::new("contract-copy", t("shell.inspector.copy_list"))
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                        }),
                ),
        );
    let escaped = close.clone();
    Some(
        Backdrop::new("contract-scrim")
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
                                "contract-dialog-scope",
                                gpui::Role::Dialog,
                                t("shell.inspector.contract_title"),
                                None::<SharedString>,
                            )
                            .child(card),
                    ),
            )
            .into_any_element(),
    )
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

fn kv(label: String, value: AnyElement, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .w(px(72.))
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(label),
        )
        .child(div().min_w_0().flex_1().child(value))
        .into_any_element()
}

fn mono(text: &str, color: gpui::Hsla, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(color)
        .child(Ellipsis::new(SharedString::from(text.to_owned())))
        .into_any_element()
}

fn chip(label: String, color: gpui::Hsla, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .flex_none()
        .px_1()
        .rounded(theme.radius(Radius::Sm))
        .border_1()
        .border_color(colors.border)
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(color)
        .child(Ellipsis::new(SharedString::from(label)))
        .into_any_element()
}

/// The line as pretty JSON. A line that does not parse is shown as it arrived:
/// the stream is the only place it exists, and hiding it would lose it.
fn pretty(event: &Event) -> String {
    serde_json::from_str::<serde_json::Value>(&event.raw)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| event.raw.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(at: u64) -> Event {
        Event {
            at_unix_ms: at,
            kind: "pane.created".into(),
            detail: "w1:p1".into(),
            wire: "pane_created".into(),
            raw: r#"{"event":"pane_created","data":{"pane":{"pane_id":"w1:p1"}}}"#.into(),
        }
    }

    /// The selection is a copy of one arrival: the ring moving under it must
    /// not change what the inspector shows, and two events with the same kind
    /// and subject are told apart by their arrival and their line.
    #[test]
    fn the_selection_is_one_captured_arrival() {
        let mut state = State::default();
        assert!(state.selected().is_none());
        state.open(event(1_000));
        assert_eq!(state.selected(), Some(&event(1_000)));
        assert_ne!(
            state.selected(),
            Some(&event(2_000)),
            "a later arrival is another event"
        );
        state.set_tab(Tab::Raw);
        state.open(event(2_000));
        assert_eq!(state.tab(), Tab::Raw, "the reading mode is kept");
        assert_eq!(state.selected().map(|e| e.at_unix_ms), Some(2_000));
        state.open_contract();
        assert!(state.contract_open());
        state.close();
        assert!(!state.is_open());
        assert!(
            !state.contract_open(),
            "closing the drawer closes its contract"
        );
        assert!(state.selected().is_none());
    }

    /// Pretty-printing is a view of the same JSON, and a malformed line
    /// survives byte for byte, because the raw line is the only copy of it.
    #[test]
    fn pretty_printing_keeps_the_json_and_a_broken_line() {
        let original: serde_json::Value =
            serde_json::from_str(&event(1).raw).expect("the fixture is JSON");
        let printed: serde_json::Value =
            serde_json::from_str(&pretty(&event(1))).expect("the printed form is JSON");
        assert_eq!(printed, original);
        let mut broken = event(1);
        broken.raw = "not json".into();
        assert_eq!(pretty(&broken), "not json");
    }

    /// The three tabs have distinct ids and resident labels.
    #[test]
    fn every_tab_has_an_id_and_a_label() {
        let mut seen = Vec::new();
        for tab in Tab::ALL {
            assert!(!seen.contains(&tab.id()));
            seen.push(tab.id());
            let key = format!("shell.inspector.tab.{}", tab.id());
            assert_ne!(t(&key), key, "{key} resolves through the locale file");
        }
    }
}
