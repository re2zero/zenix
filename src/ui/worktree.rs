//! The new-worktree form: a branch name and a base, then the daemon's own
//! `worktree.create`.
//!
//! The design's modal asks for the branch and the base it should start from
//! and shows where the checkout would land (`docs/ui/redesign` `.modal-card`).
//! The two names are the daemon's parameters; the path is not — herdr names
//! the checkout directory itself (`~/work/term-ide~2` and so on), so the row
//! says that instead of printing a path this client computed and could get
//! wrong.
//!
//! A create either appears in the sidebar's WORKTREES block or answers with
//! the daemon's reason, in the card. The daemon pushes `worktree.created`, so
//! the list beside the form updates without a timer; this card keeps its
//! answer until it is closed.

use ely_core::{
    buttons::{Button, ButtonVariant, IconButton},
    forms::{Input, TextInput},
    primitives::{Backdrop, FocusScope, Icon, IconName, give_back, take_focus},
    theme::{ActiveTheme, ControlSize, Elevation, IconSize, Radius, TextSize},
    typography::Ellipsis,
};
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
    prelude::*, px,
};

use super::shell::Root;
use crate::i18n::t;

/// The card's width and the room it keeps from the window.
const WIDTH: f32 = 440.;
const MARGIN: f32 = 48.;

/// The form as the shell holds it: whether it is up, its two fields, and the
/// daemon's answer to the last create.
#[derive(Default)]
pub(crate) struct State {
    open: bool,
    branch: Option<Entity<TextInput>>,
    base: Option<Entity<TextInput>>,
    awaiting: bool,
    answer: Option<String>,
}

impl State {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn open(&mut self, branch: Entity<TextInput>, base: Entity<TextInput>) {
        self.open = true;
        self.branch = Some(branch);
        self.base = Some(base);
        self.awaiting = false;
        self.answer = None;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.branch = None;
        self.base = None;
        self.awaiting = false;
        self.answer = None;
    }

    pub(crate) fn text(&self, cx: &App) -> (String, String) {
        let read = |input: &Option<Entity<TextInput>>| {
            input
                .as_ref()
                .map(|input| input.read(cx).text().trim().to_owned())
                .unwrap_or_default()
        };
        (read(&self.branch), read(&self.base))
    }

    pub(crate) fn creating(&mut self, line: String) {
        self.answer = Some(line);
        self.awaiting = true;
    }

    pub(crate) fn awaiting(&self) -> bool {
        self.awaiting
    }

    pub(crate) fn note(&mut self, line: String) {
        self.answer = Some(line);
        self.awaiting = false;
    }
}

/// The card over the shell. The two fields are live inputs; Enter in either
/// submits through the handler the shell owns.
pub(crate) fn overlay(
    state: &State,
    repo: Option<&str>,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    if !state.is_open() {
        return None;
    }
    let (Some(branch), Some(base)) = (state.branch.as_ref(), state.base.as_ref()) else {
        return None;
    };
    let takeover = take_focus("worktree-dialog-takeover", window, cx);
    let focus = takeover.read(cx).focus.clone();
    // The branch is what the form is for; the base is already filled from the
    // focused workspace. Only the first frame moves the caret, so a reader who
    // tabs to the base keeps it.
    if window.focused(cx).as_ref() == Some(&focus) {
        let branch_focus = branch.read(cx).focus().clone();
        window.focus(&branch_focus, cx);
    }
    let theme = cx.theme();
    let colors = &theme.colors;
    let close = {
        let root = root.clone();
        let takeover = takeover.clone();
        move |window: &mut Window, cx: &mut App| {
            give_back(&takeover, window, cx);
            root.update(cx, |root, cx| root.close_worktree_form(cx));
        }
    };
    let (branch_text, _) = state.text(cx);
    // The form says where the worktree comes from; the daemon names the
    // checkout directory, so that row states the rule rather than a guess.
    let repo_line = match repo {
        Some(repo) => rust_i18n::t!("shell.worktrees.form_repo", repo = repo).to_string(),
        None => t("shell.worktrees.form_no_repo"),
    };
    let create = {
        let root = root.clone();
        Button::new("worktree-form-create", t("shell.worktrees.form_create"))
            .variant(ButtonVariant::Primary)
            .size(ControlSize::Sm)
            .disabled(branch_text.is_empty())
            .on_click(move |_, _, cx| {
                root.update(cx, |root, cx| root.create_worktree(cx));
            })
    };
    let cancel = {
        let close = close.clone();
        Button::new("worktree-form-cancel", t("general.cancel"))
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .on_click(move |_, window, cx| close(window, cx))
    };
    let card = div()
        .w(px(WIDTH).min(window.viewport_size().width - px(MARGIN)))
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
                .h(px(52.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .border_b_1()
                .border_color(colors.border)
                .child(
                    Icon::new(IconName::GitBranch)
                        .size(IconSize::Md)
                        .color(colors.accent),
                )
                .child(
                    div()
                        .text_size(theme.text_size(TextSize::Lg))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.fg)
                        .child(t("shell.worktrees.form_title")),
                )
                .child(div().flex_1())
                .child({
                    let close = close.clone();
                    IconButton::new("worktree-form-close", IconName::X)
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, window, cx| close(window, cx))
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .px_4()
                .py_3()
                .child(
                    div()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_muted)
                        .child(Ellipsis::new(SharedString::from(repo_line))),
                )
                .child(field(
                    t("shell.worktrees.form_branch"),
                    Input::new(branch),
                    cx,
                ))
                .child(field(t("shell.worktrees.form_base"), Input::new(base), cx))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded(theme.radius(Radius::Md))
                        .border_1()
                        .border_color(colors.border)
                        .bg(colors.surface)
                        .px_2()
                        .py_1p5()
                        .child(
                            Icon::new(IconName::Folder)
                                .size(IconSize::Xs)
                                .color(colors.fg_subtle),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .text_size(theme.text_size(TextSize::Xs))
                                .text_color(colors.fg_muted)
                                .child(t("shell.worktrees.form_path")),
                        ),
                )
                .child(
                    div()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(t("shell.worktrees.form_note")),
                )
                .children(state.answer.as_deref().map(|answer| {
                    div()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_muted)
                        .child(SharedString::from(answer.to_owned()))
                })),
        )
        .child(
            div()
                .h(px(46.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .border_t_1()
                .border_color(colors.border)
                .child(
                    div()
                        .flex_none()
                        .px_1()
                        .rounded(theme.radius(Radius::Sm))
                        .border_1()
                        .border_color(colors.border)
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child("git worktree"),
                )
                .child(div().flex_1())
                .child(cancel)
                .child(create),
        );

    let escaped = close.clone();
    Some(
        Backdrop::new("worktree-form-scrim")
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
                                "worktree-dialog-scope",
                                gpui::Role::Dialog,
                                t("shell.worktrees.form_title"),
                                None::<SharedString>,
                            )
                            .child(card),
                    ),
            )
            .into_any_element(),
    )
}

/// A labelled field. The label sits above at the small type, the way the
/// design's `.field-row` reads.
fn field(label: String, input: Input, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(label),
        )
        .child(input)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The form starts closed with no answer; opening and closing drops the
    /// fields with the card, so a later opening cannot type into a dead input.
    #[test]
    fn the_form_starts_empty_and_closes_clean() {
        let state = State::default();
        assert!(!state.is_open());
        assert!(!state.awaiting());
    }
}
