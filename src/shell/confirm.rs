//! The confirmation a destructive request asks for before it runs.
//!
//! Two requests in the shell cannot be taken back once the daemon has done
//! them — removing a worktree's workspace deletes the checkout from disk, and
//! unlinking a plugin forgets it — and neither has an undo on the daemon's
//! side. Both now ask first, through the one dialog defined here: a small card
//! over a scrim, Escape and a click in the dark both answering no, and the
//! confirm button the only path to the request. The settings surface's
//! armed button is the other pattern this shell knows, and it stays where it
//! is: a reset writes this client's own file, while these two reach the
//! daemon's world.

use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable as _, Theme, h_flex, v_flex};
use gpui_kit::*;

use super::registry::{Dispatch, ShellRequest};
use crate::i18n::t;

/// What is being asked about. The kinds are the two requests the shell will
/// not run unasked; each carries the name the dialog shows, which is also the
/// name the outcome note uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmKind {
    RemoveWorktree { workspace_id: String, name: String },
    UnlinkPlugin { plugin_id: String, name: String },
}

impl ConfirmKind {
    /// The request the confirm button dispatches. The dialog is the only
    /// caller, so this mapping is the whole meaning of "confirmed".
    fn confirmed_request(&self, force: bool) -> ShellRequest {
        match self {
            Self::RemoveWorktree { workspace_id, name } => ShellRequest::RemoveWorktree {
                workspace_id: workspace_id.clone(),
                name: name.clone(),
                force,
            },
            Self::UnlinkPlugin { plugin_id, .. } => ShellRequest::UnlinkPlugin(plugin_id.clone()),
        }
    }
}

/// The dialog's whole state: what it asks about, and the force row's answer.
/// `force` is read only by a worktree removal; the unlink kind has no such
/// row and the value stays wherever it was left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Confirm {
    pub(crate) kind: ConfirmKind,
    pub(crate) force: bool,
}

impl From<ConfirmKind> for Confirm {
    fn from(kind: ConfirmKind) -> Self {
        Self { kind, force: false }
    }
}

/// The dialog, mounted over every other surface: it blocks what it asks about.
pub(crate) fn overlay(
    confirm: &Confirm,
    dispatch: &Dispatch,
    focus: &FocusHandle,
    _: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme();
    let title = match &confirm.kind {
        ConfirmKind::RemoveWorktree { .. } => t("confirm.remove_worktree.title"),
        ConfirmKind::UnlinkPlugin { .. } => t("confirm.unlink_plugin.title"),
    };
    let body = match &confirm.kind {
        ConfirmKind::RemoveWorktree { name, .. } => {
            rust_i18n::t!("confirm.remove_worktree.body", name = name).to_string()
        }
        ConfirmKind::UnlinkPlugin { name, .. } => {
            rust_i18n::t!("confirm.unlink_plugin.body", name = name).to_string()
        }
    };
    let confirm_label = match &confirm.kind {
        ConfirmKind::RemoveWorktree { .. } => t("confirm.remove"),
        ConfirmKind::UnlinkPlugin { .. } => t("confirm.unlink"),
    };
    let card = v_flex()
        .id("confirm-dialog")
        .w(px(400.))
        .max_w_full()
        .flex_none()
        .gap(px(14.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow(vec![
            BoxShadow::new(px(0.), px(18.), black().opacity(0.45)).blur_radius(px(48.)),
            BoxShadow::new(px(0.), px(2.), black().opacity(0.3)).blur_radius(px(8.)),
        ])
        .p(px(18.))
        // A press anywhere outside the dialog dismisses, exactly as Escape
        // does: the listener is the dialog's own, and fires only when the
        // press landed outside its bounds, so a click on a control never
        // reads as a click on the dark around the dialog.
        .on_mouse_down_out({
            let dismiss = dispatch.clone();
            move |event, window, cx| {
                if event.button == MouseButton::Left {
                    dismiss(ShellRequest::DismissConfirm, window, cx);
                }
            }
        })
        .child(
            div()
                .w_full()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .w_full()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(body),
        )
        .child(force_row(confirm, dispatch, theme))
        .child(
            h_flex()
                .w_full()
                .justify_end()
                .gap_2()
                .child(
                    button("confirm-cancel", &t("general.cancel"), theme).on_click({
                        let dismiss = dispatch.clone();
                        move |_, window, cx| dismiss(ShellRequest::DismissConfirm, window, cx)
                    }),
                )
                .child(button("confirm-accept", &confirm_label, theme).on_click({
                    let run = dispatch.clone();
                    let request = confirm.kind.confirmed_request(confirm.force);
                    move |_, window, cx| run(request.clone(), window, cx)
                })),
        );
    // The scrim is the settings dialog's: dark enough to name what is blocked,
    // and Escape answers no the same way a click in it does.
    let dismiss = dispatch.clone();
    v_flex()
        .id("confirm-overlay")
        .absolute()
        .inset_0()
        .occlude()
        .size_full()
        .bg(black().opacity(if theme.is_dark() { 0.45 } else { 0.22 }))
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.))
        .track_focus(focus)
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                dismiss(ShellRequest::DismissConfirm, window, cx);
            }
        })
        .child(card)
        .into_any_element()
}

/// The worktree removal's second question: whether uncommitted changes are
/// deleted too. The daemon refuses a dirty worktree without force, and
/// dirtiness is a fact about the checkout this client cannot know beforehand —
/// so the row is asked here rather than a failed first attempt being shown.
fn force_row(confirm: &Confirm, dispatch: &Dispatch, theme: &Theme) -> AnyElement {
    if !matches!(confirm.kind, ConfirmKind::RemoveWorktree { .. }) {
        return div().into_any_element();
    }
    h_flex()
        .w_full()
        .gap(px(6.))
        .items_center()
        .child(
            Switch::new("confirm-force")
                .checked(confirm.force)
                .small()
                .on_click({
                    let toggle = dispatch.clone();
                    move |value, window, cx| {
                        toggle(ShellRequest::SetConfirmForce(*value), window, cx);
                    }
                }),
        )
        .child(
            div()
                .min_w_0()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(t("confirm.remove_worktree.force")),
        )
        .into_any_element()
}

/// The button the dialog's footer uses: the same shape the plugin views'
/// buttons have, so the surfaces read as one shell.
fn button(id: &'static str, label: &str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px_2()
        .py_0p5()
        .rounded_sm()
        .bg(theme.secondary)
        .text_color(theme.secondary_foreground)
        .text_xs()
        .cursor_pointer()
        .child(label.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    /// `ShellRequest` carries no `PartialEq`, so what the dialog confirms is
    /// read by the fields that matter rather than compared whole.
    #[test]
    fn a_confirmed_removal_names_the_workspace_and_the_force_it_was_given() {
        let kind = ConfirmKind::RemoveWorktree {
            workspace_id: "w-3".to_owned(),
            name: "worktree/green-stone".to_owned(),
        };
        let ShellRequest::RemoveWorktree {
            workspace_id,
            name,
            force,
        } = kind.confirmed_request(false)
        else {
            panic!("a worktree kind confirms as a removal");
        };
        assert_eq!(workspace_id, "w-3");
        assert_eq!(name, "worktree/green-stone");
        assert!(!force);
        let ShellRequest::RemoveWorktree { force, .. } = kind.confirmed_request(true) else {
            panic!("a worktree kind confirms as a removal");
        };
        assert!(force, "the force row's answer travels with the request");
    }

    #[test]
    fn a_confirmed_unlink_is_the_unlink_that_already_was() {
        let kind = ConfirmKind::UnlinkPlugin {
            plugin_id: "board".to_owned(),
            name: "board".to_owned(),
        };
        let ShellRequest::UnlinkPlugin(plugin_id) = kind.confirmed_request(false) else {
            panic!("an unlink kind confirms as an unlink");
        };
        assert_eq!(plugin_id, "board");
    }

    #[test]
    fn a_dialog_opens_unforced() {
        let confirm = Confirm::from(ConfirmKind::UnlinkPlugin {
            plugin_id: "board".to_owned(),
            name: "board".to_owned(),
        });
        assert!(!confirm.force);
    }
}
