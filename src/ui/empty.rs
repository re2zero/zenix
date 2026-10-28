//! The first-run screen: what the main column shows before there is a
//! workspace to paint.
//!
//! The design's ninth surface (`docs/ui/redesign` `emptyMain`) is the first
//! screen a new install sees: one sentence saying what is missing, four entries
//! that each reach a real destination, and — while the daemon is down — a card
//! that explains the seed path, starts the daemon in place and shows its last
//! output. This module draws exactly that.
//!
//! It is an overlay over the pane grid rather than a replacement for it: the
//! grid stays mounted behind the block, so the terminal's focus, theme and
//! connection survive the first workspace arriving, and the empty screen costs
//! no second code path in the terminal view. The block itself takes every press
//! (`occlude`), so nothing behind it is clicked by accident.
//!
//! Every entry is a command from the directory ([`super::commands`]), not a
//! private action: the button a person clicks and the chord they could have
//! typed end in the same place, and the test holds the four to that table.
//!
//! ## The daemon card
//!
//! "Start daemon" runs the same bootstrap the client uses on its own
//! (`client::start_herdr_server`), off the UI thread because seeding a binary
//! is file work; the connection is asked to dial at once when it returns, so
//! the first snapshot follows on the existing pulse path. The card never opens
//! a terminal: a failed start shows the daemon's own last lines from
//! `herdr-server.log` and offers the command to run by hand.

use ely_core::buttons::{Button, ButtonVariant};
use ely_core::feedback::EmptyState;
use ely_core::layout::ScrollArea;
use ely_core::primitives::{Icon, IconName};
use ely_core::theme::{ActiveTheme, ControlSize, IconSize, Radius, TextSize};
use gpui::{
    AnyElement, App, ClipboardItem, Entity, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::*, px,
};

use super::commands::Command;
use super::shell::Root;
use crate::herdr::{ConnectionStatus, LiveState};
use crate::i18n::t;

/// The block's own width: the design's four cards read two by two, and the
/// daemon card's sentence needs a line's room.
const WIDTH: f32 = 600.;

/// What the daemon card remembers between pulses: the log tail while it is
/// open, the last start's answer, and whether one is running.
#[derive(Default)]
pub(crate) struct State {
    /// The tail of `herdr-server.log`, while the card shows it.
    pub(crate) log: Option<String>,
    /// What the last start attempt said, one line.
    pub(crate) note: Option<String>,
    /// A start is running off the UI thread; the button says so by waiting.
    pub(crate) starting: bool,
}

/// The block over the grid. `live` decides whether the daemon card is drawn;
/// the four entries are always there, because a connected daemon with no
/// workspace is the same question to a reader as an absent one.
pub(crate) fn main(live: &LiveState, state: &State, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .id("first-run")
        .absolute()
        .inset_0()
        .occlude()
        // An opaque ground: the grid is still mounted underneath with the
        // terminal's own background and, when the daemon just dropped, its
        // reconnect card. The first screen is the chrome, not a second card
        // stacked over that one.
        .bg(theme.colors.bg)
        .flex()
        .items_center()
        .justify_center()
        .px_6()
        .py_8()
        .child(
            div()
                .w(px(WIDTH))
                .max_w_full()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    EmptyState::new(
                        "first-run-head",
                        IconName::Sparkles,
                        t("shell.first_run.title"),
                    )
                    .body(t("shell.first_run.body")),
                )
                .child(entries(root, cx))
                .children(
                    (live.status == ConnectionStatus::Disconnected)
                        .then(|| daemon_card(live, state, root, cx)),
                ),
        )
        .into_any_element()
}

/// One card of the two-by-two grid.
struct Entry {
    icon: IconName,
    title: &'static str,
    detail: &'static str,
    command: Command,
}

/// The four ways out of the first screen, in the order the design lists them.
const ENTRIES: [Entry; 4] = [
    Entry {
        icon: IconName::LayoutGrid,
        title: "shell.first_run.workspace.title",
        detail: "shell.first_run.workspace.detail",
        command: Command::NewWorkspace,
    },
    Entry {
        icon: IconName::FileCode,
        title: "shell.first_run.layout.title",
        detail: "shell.first_run.layout.detail",
        command: Command::InspectLayout,
    },
    Entry {
        icon: IconName::Puzzle,
        title: "shell.first_run.trust.title",
        detail: "shell.first_run.trust.detail",
        command: Command::LinkPlugin,
    },
    Entry {
        icon: IconName::Settings,
        title: "shell.first_run.settings.title",
        detail: "shell.first_run.settings.detail",
        command: Command::OpenSettings,
    },
];

fn entries(root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut grid = div().flex().flex_col().gap_2();
    for (row, pair) in ENTRIES.chunks(2).enumerate() {
        grid = grid.child(div().flex().gap_2().items_stretch().children(
            pair.iter().enumerate().map(|(column, entry)| {
                let open = root.clone();
                let command = entry.command;
                div()
                    .id(("first-run-entry", row * 2 + column))
                    // A press target with a picture: assistive technology gets
                    // the button it behaves like, and the card's own title as
                    // its name.
                    .role(gpui::Role::Button)
                    .aria_label(t(entry.title))
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .rounded(theme.radius(Radius::Lg))
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.surface)
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.hover).border_color(colors.border_strong))
                    .on_click(move |_, window, cx| {
                        open.update(cx, |root, cx| root.run_command(command, window, cx));
                    })
                    .child(
                        div()
                            .flex_none()
                            .size(px(34.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(theme.radius(Radius::Md))
                            .bg(colors.sunken)
                            .child(
                                Icon::new(entry.icon)
                                    .size(IconSize::Md)
                                    .color(colors.fg_muted),
                            ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(theme.text_size(TextSize::Sm))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(colors.fg)
                                    .child(t(entry.title)),
                            )
                            .child(
                                div()
                                    .text_size(theme.text_size(TextSize::Xs))
                                    .text_color(colors.fg_muted)
                                    .child(t(entry.detail)),
                            ),
                    )
            }),
        ));
    }
    grid.into_any_element()
}

/// The daemon, when there is none: why, how to start it, and — if it failed —
/// what it said.
fn daemon_card(live: &LiveState, state: &State, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let toggle = {
        let root = root.clone();
        Button::new(
            "daemon-log-toggle",
            if state.log.is_some() {
                t("shell.first_run.daemon.hide_log")
            } else {
                t("shell.first_run.daemon.logs")
            },
        )
        .variant(ButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .on_click(move |_, _, cx| {
            root.update(cx, |root, cx| root.toggle_daemon_log(cx));
        })
    };
    let start = {
        let root = root.clone();
        Button::new("daemon-start", t("shell.first_run.daemon.start"))
            .variant(ButtonVariant::Primary)
            .size(ControlSize::Sm)
            .disabled(state.starting)
            .on_click(move |_, _, cx| {
                root.update(cx, |root, cx| root.start_daemon(cx));
            })
    };
    let copy = {
        Button::new(
            "daemon-copy-command",
            t("shell.first_run.daemon.copy_command"),
        )
        .variant(ButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .on_click(|_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(
                crate::client::daemon_start_command(),
            ));
        })
    };

    let mut card = div()
        .id("daemon-card")
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(theme.radius(Radius::Lg))
        .border_1()
        .border_color(colors.border)
        .bg(colors.surface)
        .child(
            div()
                .flex()
                .items_start()
                .gap_3()
                .child(
                    div()
                        .flex_none()
                        .size(px(30.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(colors.sunken)
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(IconSize::Sm)
                                .color(colors.warning),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .text_size(theme.text_size(TextSize::Sm))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(colors.fg)
                                .child(if state.starting {
                                    t("shell.first_run.daemon.starting")
                                } else {
                                    t("shell.first_run.daemon.title")
                                }),
                        )
                        .child(
                            div()
                                .text_size(theme.text_size(TextSize::Xs))
                                .text_color(colors.fg_muted)
                                .child(t("shell.first_run.daemon.body")),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(toggle)
                        .child(start),
                ),
        );
    if let Some(reason) = live.error.as_deref().filter(|reason| !reason.is_empty()) {
        card = card.child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.danger)
                .child(SharedString::from(reason.to_owned())),
        );
    }
    if let Some(note) = state.note.as_deref() {
        card = card.child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(SharedString::from(note.to_owned())),
        );
    }
    if let Some(log) = state.log.as_deref() {
        card = card
            .child(
                div()
                    .h(px(140.))
                    .rounded(theme.radius(Radius::Md))
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.sunken)
                    .overflow_hidden()
                    .child(
                        ScrollArea::new("daemon-log").vertical().size_full().child(
                            div()
                                .p_2()
                                .text_size(theme.text_size(TextSize::Xs))
                                .text_color(colors.fg_muted)
                                .child(SharedString::from(log.to_owned())),
                        ),
                    ),
            )
            .child(div().flex().justify_end().child(copy));
    }
    card.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::commands::COMMANDS;
    use std::prelude::v1::test;

    /// The first screen's promise is that every entry is a command the shell
    /// can actually run, not a card drawn for the picture.
    #[test]
    fn every_entry_names_a_command_in_the_directory() {
        for entry in &ENTRIES {
            assert!(
                COMMANDS.iter().any(|info| info.command == entry.command),
                "{:?} is not in the command directory",
                entry.command
            );
        }
    }

    /// A missing locale key comes back as the key itself, which would read as
    /// `shell.first_run.layout.title` on screen: the test would rather fail.
    #[test]
    fn every_entry_has_its_copy() {
        for entry in &ENTRIES {
            for key in [entry.title, entry.detail] {
                let copy = t(key);
                assert_ne!(copy, key, "{key} has no translation");
                assert!(!copy.trim().is_empty(), "{key} is blank");
            }
        }
    }

    /// Both languages, for the same reason.
    #[test]
    fn the_block_speaks_both_languages() {
        let previous = crate::i18n::locale();
        for locale in ["en", "zh-CN"] {
            crate::i18n::set_locale(locale);
            for key in [
                "shell.first_run.title",
                "shell.first_run.body",
                "shell.first_run.daemon.title",
                "shell.first_run.daemon.body",
                "shell.first_run.daemon.start",
                "shell.first_run.daemon.logs",
            ] {
                let copy = t(key);
                assert_ne!(copy, key, "{locale}: {key} has no translation");
            }
        }
        crate::i18n::set_locale(&previous);
    }
}
