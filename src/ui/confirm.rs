//! The one confirmation the shell asks before a request it cannot undo.
//!
//! Two daemon calls reach beyond the client's own files: removing a worktree
//! closes the workspace and deletes the checkout from disk, and unlinking a
//! plugin forgets it. Neither has an undo on the daemon's side, so both pass
//! through the card drawn here — Escape, a press in the scrim and Cancel all
//! answer no, and only the confirming button dispatches.
//!
//! The library's `ConfirmationCard` draws the same idea but its cancel button
//! is an English literal, and this client's strings resolve through
//! `locales/zenix.yml`; the card is composed here from the same button and
//! surface primitives for that reason. The settings dialog's focus pattern is
//! reused whole: the overlay takes the keyboard, traps Tab, and hands focus
//! back on the way out — a raised card must not let a keystroke reach the pane
//! behind it.

use std::rc::Rc;

use ely_core::buttons::{Button, ButtonVariant};
use ely_core::forms::Switch;
use ely_core::primitives::{Backdrop, FocusScope, give_back, take_focus};
use ely_core::theme::{ActiveTheme, ControlSize, Elevation, Radius, TextSize};
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, Styled, Window, div, prelude::*, px,
};

use super::shell::Root;
use crate::i18n::t;

/// What is being asked about: the two requests the shell will not run unasked,
/// each carrying the name the card shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    RemoveWorktree { workspace_id: String, name: String },
    UnlinkPlugin { plugin_id: String, name: String },
}

impl Kind {
    fn title(&self) -> String {
        match self {
            Self::RemoveWorktree { .. } => t("confirm.remove_worktree.title"),
            Self::UnlinkPlugin { .. } => t("confirm.unlink_plugin.title"),
        }
    }

    fn body(&self) -> String {
        match self {
            Self::RemoveWorktree { name, .. } => {
                rust_i18n::t!("confirm.remove_worktree.body", name = name).to_string()
            }
            Self::UnlinkPlugin { name, .. } => {
                rust_i18n::t!("confirm.unlink_plugin.body", name = name).to_string()
            }
        }
    }

    fn accept_label(&self) -> String {
        match self {
            Self::RemoveWorktree { .. } => t("confirm.remove"),
            Self::UnlinkPlugin { .. } => t("confirm.unlink"),
        }
    }

    /// Whether the card carries the force row. Only a worktree removal has a
    /// second question the daemon can ask — whether uncommitted changes may go
    /// too — and the answer is a flag on that one request.
    fn needs_force(&self) -> bool {
        matches!(self, Self::RemoveWorktree { .. })
    }
}

/// The card's whole state: what it asks about, and the force row's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Confirm {
    pub(crate) kind: Kind,
    pub(crate) force: bool,
}

impl From<Kind> for Confirm {
    fn from(kind: Kind) -> Self {
        Self { kind, force: false }
    }
}

/// Type aliases keep the two callbacks one line each at the call sites.
type Close = Rc<dyn Fn(&mut Window, &mut App)>;

/// The card over the window. Drawn only while a confirmation is pending, so the
/// focus takeover lives with the element: closing drops it, and a later
/// confirmation takes it again.
pub(crate) fn overlay(
    confirm: &Confirm,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let takeover = take_focus("confirm-dialog-takeover", window, cx);
    let focus = takeover.read(cx).focus.clone();
    let theme = cx.theme();
    let colors = &theme.colors;
    let close: Close = {
        let root = root.clone();
        let takeover = takeover.clone();
        Rc::new(move |window, cx| {
            give_back(&takeover, window, cx);
            root.update(cx, |root, cx| root.dismiss_confirm(cx));
        })
    };
    let accept = {
        let root = root.clone();
        let takeover = takeover.clone();
        Rc::new(move |window: &mut Window, cx: &mut App| {
            give_back(&takeover, window, cx);
            root.update(cx, |root, cx| root.confirm_accept(window, cx));
        })
    };

    let mut card = div()
        .w(px(400.))
        .max_w_full()
        .flex()
        .flex_col()
        .gap(px(14.))
        .p(px(18.))
        .rounded(theme.radius(Radius::Lg))
        .border_1()
        .border_color(colors.border)
        .bg(colors.surface)
        .shadow(theme.elevation(Elevation::Modal))
        .child(
            div()
                .text_size(theme.text_size(TextSize::Base))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(colors.fg)
                .child(confirm.kind.title()),
        )
        .child(
            div()
                .text_size(theme.text_size(TextSize::Sm))
                .text_color(colors.fg_muted)
                .child(confirm.kind.body()),
        );
    if confirm.kind.needs_force() {
        card = card.child(force_row(confirm, root, cx));
    }
    let cancel = {
        let close = close.clone();
        Button::new("confirm-cancel", t("general.cancel"))
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .on_click(move |_, window, cx| close(window, cx))
    };
    let confirm_button = {
        let accept = accept.clone();
        Button::new("confirm-accept", confirm.kind.accept_label())
            .variant(ButtonVariant::Danger)
            .size(ControlSize::Sm)
            .on_click(move |_, window, cx| accept(window, cx))
    };
    card = card.child(
        div()
            .flex()
            .justify_end()
            .gap_2()
            .child(cancel)
            .child(confirm_button),
    );

    let escaped = close.clone();
    Backdrop::new("confirm-scrim")
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
                            "confirm-dialog-scope",
                            gpui::Role::Dialog,
                            confirm.kind.title(),
                            None::<gpui::SharedString>,
                        )
                        .child(card),
                ),
        )
        .into_any_element()
}

/// The worktree removal's second question: whether uncommitted changes are
/// deleted too. The daemon refuses a dirty worktree without force, and
/// dirtiness is a fact this client cannot know beforehand — so the row is asked
/// here instead of a failed first attempt being shown.
fn force_row(confirm: &Confirm, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let checked = confirm.force;
    let toggle = root.clone();
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            Switch::new("confirm-force", checked).on_change(move |value, _, cx| {
                toggle.update(cx, |root, cx| root.set_confirm_force(value, cx));
            }),
        )
        .child(
            div()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(t("confirm.remove_worktree.force")),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A card opens unforced; only the worktree kind carries the force row, and
    /// the two kinds name their own title and button.
    #[test]
    fn a_confirmation_opens_unforced_and_only_a_removal_has_a_force_row() {
        let remove = Confirm::from(Kind::RemoveWorktree {
            workspace_id: "w-3".into(),
            name: "feat/green".into(),
        });
        assert!(!remove.force);
        assert!(remove.kind.needs_force());
        let unlink = Confirm::from(Kind::UnlinkPlugin {
            plugin_id: "board".into(),
            name: "board".into(),
        });
        assert!(!unlink.kind.needs_force());
        assert_ne!(remove.kind.accept_label(), unlink.kind.accept_label());
        assert_ne!(remove.kind.title(), unlink.kind.title());
    }

    /// The body names what is being removed: a card that did not would ask
    /// about the wrong row.
    #[test]
    fn the_body_names_the_subject() {
        let remove = Confirm::from(Kind::RemoveWorktree {
            workspace_id: "w-3".into(),
            name: "worktree/green-stone".into(),
        });
        assert!(remove.kind.body().contains("worktree/green-stone"));
        let unlink = Confirm::from(Kind::UnlinkPlugin {
            plugin_id: "board".into(),
            name: "The Board".into(),
        });
        assert!(unlink.kind.body().contains("The Board"));
    }
}
