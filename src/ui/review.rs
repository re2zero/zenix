//! The review sheet: the focused repository's changes, as git spells them.
//!
//! The redesign has no review surface, so this one borrows the frame the
//! project already accepted for `layout_sheet` — an 840px modal over a scrim,
//! opened from the command directory — and spends the width on the one thing a
//! reader needs: the diff. The left column is the index in two sections
//! (staged, then unstaged and untracked); the right is the selected file's
//! unified diff, one row per line, coloured by what the line is.
//!
//! Nothing here reads git: `crate::review` owns the worker thread and the two
//! caches, and this module only turns what it found into rows. A select asks
//! for one file's diff and carries the state's generation, so a slow read of
//! the file the reader left cannot land on the one they are looking at.

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
use crate::i18n::t;
use crate::review::{ChangedFile, DiffState, ReviewState};

/// The modal's widest card and the room it keeps from the window.
const WIDTH: f32 = 840.;
const MARGIN: f32 = 48.;
/// The file column: wide enough for a path and its counts.
const FILES: f32 = 280.;

/// The sheet's own state: open, the picked file, and a note for the case where
/// there is no repository to read at all.
#[derive(Default)]
pub(crate) struct State {
    open: bool,
    note: Option<String>,
    selected: Option<Selected>,
    generation: u64,
}

/// The file the right column shows, and which side of the index asked for it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Selected {
    pub(crate) path: String,
    pub(crate) staged: bool,
    pub(crate) untracked: bool,
}

impl State {
    pub(crate) fn open(&mut self, note: Option<String>) {
        self.open = true;
        self.note = note;
        self.selected = None;
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.note = None;
        self.selected = None;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub(crate) fn selected(&self) -> Option<&Selected> {
        self.selected.as_ref()
    }

    /// Pick a file and hand back the generation the read belongs to.
    pub(crate) fn select(&mut self, path: String, staged: bool, untracked: bool) -> u64 {
        self.selected = Some(Selected {
            path,
            staged,
            untracked,
        });
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

/// What one diff line is, from its first character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffKind {
    /// `diff --git`, `index`, `---`/`+++`, mode and rename lines: git's own
    /// header, drawn quiet.
    Meta,
    Hunk,
    Added,
    Removed,
    Context,
}

/// Classify a unified diff line. The header checks come first: `--- ` and
/// `+++ ` are file headers, not a removed and an added line.
pub(crate) fn diff_kind(line: &str) -> DiffKind {
    if line.starts_with("diff ")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("new file")
        || line.starts_with("deleted file")
        || line.starts_with("old mode")
        || line.starts_with("new mode")
        || line.starts_with("similarity index")
        || line.starts_with("rename ")
        || line.starts_with("Binary files")
        || line.starts_with("\\ No newline")
    {
        return DiffKind::Meta;
    }
    if line.starts_with("@@") {
        return DiffKind::Hunk;
    }
    match line.chars().next() {
        Some('+') => DiffKind::Added,
        Some('-') => DiffKind::Removed,
        _ => DiffKind::Context,
    }
}

/// The diff as rows ready to draw.
pub(crate) fn diff_rows(text: &str) -> Vec<(DiffKind, String)> {
    text.lines()
        .map(|line| (diff_kind(line), line.to_owned()))
        .collect()
}

/// The file rows one side of the index holds, in the order git listed them.
fn side<'a>(files: impl Iterator<Item = &'a ChangedFile>) -> Vec<&'a ChangedFile> {
    files.collect()
}

/// The modal over the shell, or nothing while it is closed.
pub(crate) fn overlay(
    state: &State,
    review: &ReviewState,
    diff: &DiffState,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    if !state.is_open() {
        return None;
    }
    let takeover = take_focus("review-dialog-takeover", window, cx);
    let focus = takeover.read(cx).focus.clone();
    let theme = cx.theme();
    let colors = &theme.colors;
    let close = {
        let root = root.clone();
        let takeover = takeover.clone();
        move |window: &mut Window, cx: &mut App| {
            give_back(&takeover, window, cx);
            root.update(cx, |root, cx| root.close_review(cx));
        }
    };

    let repo = review
        .root
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or_default()
        .to_owned();

    let refresh = {
        let root = root.clone();
        Button::new("review-refresh", t("shell.review.refresh"))
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .on_click(move |_, _, cx| {
                root.update(cx, |root, cx| root.refresh_review(cx));
            })
    };

    let staged = side(review.staged());
    let unstaged = side(review.unstaged());
    let total = staged.len() + unstaged.len();

    let mut files = div().w(px(FILES)).flex_none().flex().flex_col().min_h_0();
    files = files.child(section_head(t("shell.review.staged"), staged.len(), cx));
    files = files.child(file_list(&staged, true, state, root, cx));
    files = files.child(section_head(t("shell.review.unstaged"), unstaged.len(), cx));
    files = files.child(file_list(&unstaged, false, state, root, cx));
    if total == 0 && review.error.is_none() && state.note().is_none() {
        files = files.child(
            div()
                .px_3()
                .py_2()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(t("shell.review.empty")),
        );
    }

    let mut body = div()
        .flex_1()
        .min_h_0()
        .flex()
        .border_t_1()
        .border_color(colors.border);
    body = body.child(files);
    body = body.child(
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(colors.border)
            .child(diff_body(state, diff, cx)),
    );

    let reason = state
        .note()
        .map(str::to_owned)
        .or_else(|| review.error.clone());
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
                    Icon::new(IconName::GitBranch)
                        .size(IconSize::Sm)
                        .color(colors.accent),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Sm))
                        .text_color(colors.fg)
                        .child(t("shell.review.title")),
                )
                .when(!repo.is_empty(), |head| {
                    head.child(
                        div()
                            .flex_none()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_subtle)
                            .child(SharedString::from(repo)),
                    )
                })
                .when_some(review.branch.clone(), |head, branch| {
                    head.child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_1()
                            .rounded(theme.radius(Radius::Sm))
                            .bg(colors.sunken)
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_muted)
                            .child(branch),
                    )
                    .when(review.ahead > 0, |head| {
                        head.child(gap_mark(format!("+{}", review.ahead), colors.success))
                    })
                    .when(review.behind > 0, |head| {
                        head.child(gap_mark(format!("−{}", review.behind), colors.danger))
                    })
                })
                .child(div().flex_1())
                .child(refresh)
                .child({
                    let close = close.clone();
                    IconButton::new("review-close", IconName::X)
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, window, cx| close(window, cx))
                }),
        )
        .child(body)
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
                            rust_i18n::t!("shell.review.foot", count = total).to_string(),
                        ))),
                )
                .when_some(reason, |foot, reason| {
                    foot.child(
                        div()
                            .min_w_0()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.danger)
                            .child(Ellipsis::new(SharedString::from(reason))),
                    )
                })
                .child(div().flex_1())
                .child({
                    let close = close.clone();
                    Button::new("review-done", t("settings.close"))
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, window, cx| close(window, cx))
                }),
        );

    let escaped = close.clone();
    Some(
        Backdrop::new("review-scrim")
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
                                "review-dialog-scope",
                                gpui::Role::Dialog,
                                t("shell.review.title"),
                                None::<SharedString>,
                            )
                            .child(card),
                    ),
            )
            .into_any_element(),
    )
}

fn gap_mark(label: String, color: gpui::Hsla) -> AnyElement {
    let theme_color = color;
    div()
        .flex_none()
        .text_size(px(11.))
        .text_color(theme_color)
        .child(label)
        .into_any_element()
}

fn section_head(name: String, count: usize, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .px_3()
        .pt_2()
        .pb_1()
        .flex()
        .items_center()
        .gap_1()
        .text_size(theme.text_size(TextSize::Xs))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(colors.fg_subtle)
        .child(name)
        .child(
            div()
                .text_color(colors.fg_disabled)
                .child(format!("· {count}")),
        )
        .into_any_element()
}

fn file_list(
    files: &[&ChangedFile],
    staged: bool,
    state: &State,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut list = div().flex().flex_col();
    for file in files {
        let picked = state
            .selected()
            .is_some_and(|selected| selected.path == file.path && selected.staged == staged);
        let letter = file.letter(staged);
        let (additions, deletions) = if staged {
            (file.staged_additions, file.staged_deletions)
        } else {
            (file.worktree_additions, file.worktree_deletions)
        };
        let path = file.path.clone();
        let untracked = file.untracked;
        let open = root.clone();
        list = list.child(
            div()
                .id(gpui::ElementId::from(SharedString::from(format!(
                    "review-file-{staged}-{}",
                    file.path
                ))))
                .w_full()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .rounded(theme.radius(Radius::Sm))
                .when(picked, |row| row.bg(colors.active))
                .hover(|style| style.bg(colors.hover))
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let path = path.clone();
                    open.update(cx, |root, cx| {
                        root.select_review_file(path, staged, untracked, cx)
                    });
                })
                .child(
                    div()
                        .w(px(16.))
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Xs))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(letter_color(letter, colors))
                        .child(letter.to_string()),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_muted)
                        .child(Ellipsis::new(SharedString::from(file.path.clone()))),
                )
                .child(div().flex_1())
                .when(additions > 0 || deletions > 0, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .flex()
                            .gap_1()
                            .font_family(theme.mono_family.clone())
                            .text_size(theme.text_size(TextSize::Xs))
                            .child(
                                div()
                                    .text_color(colors.success)
                                    .child(format!("+{additions}")),
                            )
                            .child(
                                div()
                                    .text_color(colors.danger)
                                    .child(format!("−{deletions}")),
                            ),
                    )
                }),
        );
    }
    list.into_any_element()
}

fn letter_color(letter: char, colors: &ely_core::theme::Palette) -> gpui::Hsla {
    match letter {
        'A' => colors.success,
        'D' => colors.danger,
        'R' | 'C' => colors.warning,
        'M' => colors.accent,
        _ => colors.fg_subtle,
    }
}

fn diff_body(state: &State, diff: &DiffState, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let selected = match state.selected() {
        Some(selected) => selected,
        None => {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(t("shell.review.pick"))
                .into_any_element();
        }
    };
    let side_name = if selected.staged {
        t("shell.review.staged")
    } else if selected.untracked {
        t("shell.review.new_file")
    } else {
        t("shell.review.unstaged")
    };
    let header = div()
        .px_3()
        .py_2()
        .flex()
        .items_center()
        .gap_2()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(colors.fg_muted)
        .child(
            div()
                .min_w_0()
                .font_family(theme.mono_family.clone())
                .child(Ellipsis::new(SharedString::from(selected.path.clone()))),
        )
        .child(
            div()
                .flex_none()
                .px_1()
                .rounded(theme.radius(Radius::Sm))
                .bg(colors.sunken)
                .text_color(colors.fg_subtle)
                .child(side_name),
        );
    // A read for another file (or another side) is not this file's answer yet.
    let current = diff.generation == state.generation()
        && diff.path.as_deref() == Some(selected.path.as_str())
        && diff.staged == selected.staged;
    if !current {
        return div()
            .flex_1()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .px_3()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_subtle)
                    .child(t("shell.review.reading")),
            )
            .into_any_element();
    }
    if let Some(error) = diff.error.as_deref() {
        return div()
            .flex_1()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .px_3()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.danger)
                    .child(SharedString::from(error.to_owned())),
            )
            .into_any_element();
    }
    let rows = diff_rows(&diff.text);
    let mut lines = div().flex().flex_col().py_1();
    for (kind, line) in &rows {
        let (ink, wash) = match kind {
            DiffKind::Meta => (colors.fg_subtle, None),
            DiffKind::Hunk => (colors.accent, None),
            DiffKind::Added => (colors.success, Some(colors.success_subtle)),
            DiffKind::Removed => (colors.danger, Some(colors.danger.opacity(0.10))),
            DiffKind::Context => (colors.fg_muted, None),
        };
        let mut row = div()
            .w_full()
            .px_3()
            .flex()
            .gap_2()
            .font_family(theme.mono_family.clone())
            .text_size(theme.text_size(TextSize::Xs))
            .text_color(ink);
        if let Some(wash) = wash {
            row = row.bg(wash);
        }
        lines = lines.child(
            row.child(
                // The diff's own indentation is its content: leading spaces travel
                // as no-break ones so a wrapped row keeps them.
                div()
                    .min_w_0()
                    .child(SharedString::from(line.replace(' ', "\u{a0}"))),
            ),
        );
    }
    let mut column = div().flex_1().min_h_0().flex().flex_col().child(header);
    column = column.child(
        div().flex_1().min_h_0().child(
            ScrollArea::new("review-diff")
                .vertical()
                .size_full()
                .child(lines),
        ),
    );
    if diff.truncated {
        column = column.child(
            div()
                .px_3()
                .py_1()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.warning)
                .child(t("shell.review.truncated")),
        );
    }
    column.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_read_by_its_first_marker_not_its_words() {
        assert_eq!(diff_kind("diff --git a/x b/x"), DiffKind::Meta);
        assert_eq!(diff_kind("--- a/x"), DiffKind::Meta);
        assert_eq!(diff_kind("+++ b/x"), DiffKind::Meta);
        assert_eq!(diff_kind("@@ -1,2 +1,3 @@"), DiffKind::Hunk);
        assert_eq!(diff_kind("+added"), DiffKind::Added);
        assert_eq!(diff_kind("-removed"), DiffKind::Removed);
        assert_eq!(diff_kind(" unchanged"), DiffKind::Context);
        assert_eq!(diff_kind("\\ No newline at end of file"), DiffKind::Meta);
        assert_eq!(diff_kind(""), DiffKind::Context);
    }

    #[test]
    fn the_rows_keep_the_text_verbatim() {
        let rows = diff_rows("@@ -1 +1 @@\n-old\n+new");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], (DiffKind::Hunk, "@@ -1 +1 @@".into()));
        assert_eq!(rows[1], (DiffKind::Removed, "-old".into()));
        assert_eq!(rows[2], (DiffKind::Added, "+new".into()));
    }

    #[test]
    fn a_select_advances_the_generation() {
        let mut state = State::default();
        assert_eq!(state.generation(), 0);
        let first = state.select("a.txt".into(), false, false);
        assert_eq!(first, 1);
        let second = state.select("b.txt".into(), true, false);
        assert_eq!(second, 2);
        assert_eq!(state.selected().map(|s| s.path.as_str()), Some("b.txt"));
        assert!(state.selected().unwrap().staged);
    }
}
