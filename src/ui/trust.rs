//! The pre-link trust review: what a plugin directory would run, before it is
//! linked.
//!
//! Linking a plugin hands herdr a directory whose build, startup, action,
//! event, pane and link-handler commands run as the user. The daemon validates
//! a manifest but never shows the commands, so this surface does — every argv,
//! graded by [`crate::plugins::trust`]'s hazard rules — and the link button
//! stays unavailable until the reader has ticked that they read it.
//!
//! The path is a real path field with the system's folder dialog behind it;
//! a directory picked there is read at once. Reading is one small TOML file
//! per action, on the UI thread: it starts no process and touches no file the
//! plugin did not ask to be read. The commands are shown, never run.
//!
//! The dialog is exclusive: it takes the keyboard, traps Tab and hands focus
//! back on close, the same pattern the settings surface uses.

use std::path::{Path, PathBuf};

use ely_core::{
    buttons::{Button, ButtonVariant, IconButton},
    forms::{Checkbox, PathInput, TextInput},
    layout::ScrollArea,
    primitives::{Backdrop, FocusScope, Icon, IconName, give_back, take_focus},
    theme::{ActiveTheme, ControlSize, Elevation, IconSize, Radius, TextSize},
    typography::Ellipsis,
};
use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
    prelude::*, px,
};

use super::manager;
use super::shell::Root;
use crate::i18n::t;
use crate::plugins::trust::{self, CommandLine, HazardLevel, Preview};

/// The card's widest shape and the room it keeps from the window.
const WIDTH: f32 = 720.;
const MARGIN: f32 = 48.;

/// The review as the shell holds it: the field, what reading the path found,
/// whether the reader has acknowledged it, and the daemon's answer to a link.
#[derive(Default)]
pub(crate) struct State {
    open: bool,
    input: Option<Entity<TextInput>>,
    /// The directory the preview below is about, so Link links what is on
    /// screen rather than whatever has since been typed.
    reviewed: Option<PathBuf>,
    preview: Option<Result<Preview, String>>,
    read: bool,
    /// What the daemon said to the last link attempt.
    answer: Option<String>,
    /// A link is in flight; the next plugin pulse fills the answer.
    awaiting: bool,
}

impl State {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Open on a fresh field. The field is a real entity so the system dialog
    /// and the keyboard both type into one state; it is dropped with the
    /// surface.
    pub(crate) fn open(&mut self, input: Entity<TextInput>) {
        self.open = true;
        self.input = Some(input);
        self.reviewed = None;
        self.preview = None;
        self.read = false;
        self.answer = None;
        self.awaiting = false;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.input = None;
        self.reviewed = None;
        self.preview = None;
        self.read = false;
        self.answer = None;
        self.awaiting = false;
    }

    /// The text the field holds, for the review and for Enter.
    pub(crate) fn typed(&self, cx: &App) -> String {
        self.input
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_owned())
            .unwrap_or_default()
    }

    /// Record a read: the preview, the directory it is about, and a cleared
    /// acknowledgement — a new directory is a new thing to read.
    pub(crate) fn reviewed(&mut self, dir: PathBuf, preview: Result<Preview, String>) {
        self.reviewed = Some(dir);
        self.preview = Some(preview);
        self.read = false;
        self.answer = None;
        self.awaiting = false;
    }

    pub(crate) fn set_read(&mut self, read: bool) {
        self.read = read;
    }

    pub(crate) fn is_read(&self) -> bool {
        self.read
    }

    /// The directory a Link would name, when there is one that was actually
    /// read and the answer below is about it.
    pub(crate) fn linkable(&self) -> Option<PathBuf> {
        if !self.read || !self.preview.as_ref().is_some_and(Result::is_ok) {
            return None;
        }
        self.reviewed.clone()
    }

    pub(crate) fn note(&mut self, line: String) {
        self.answer = Some(line);
        self.awaiting = false;
    }

    /// Start a link: the note and the waiting flag until the registry pulse
    /// answers.
    pub(crate) fn linking(&mut self, line: String) {
        self.answer = Some(line);
        self.awaiting = true;
    }

    /// Whether a link is waiting for the daemon's word. Cleared by
    /// [`note`](Self::note).
    pub(crate) fn awaiting(&self) -> bool {
        self.awaiting
    }

    /// The directory the preview is about, for Enter's "same path?" question.
    pub(crate) fn reviewed_path(&self) -> Option<&PathBuf> {
        self.reviewed.as_ref()
    }

    /// Whether the last read succeeded. Enter reads a path before it can link
    /// one, and a failed read is still a read.
    pub(crate) fn preview_ok(&self) -> bool {
        self.preview.as_ref().is_some_and(Result::is_ok)
    }
}

/// The review itself. `key` is the field's submission handler, owned by the
/// shell because it has the host.
pub(crate) fn overlay(
    state: &State,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let input = state.input.as_ref()?;
    let takeover = take_focus("trust-dialog-takeover", window, cx);
    let focus = takeover.read(cx).focus.clone();
    // The field is what the dialog is for, so the first frame puts the caret
    // in it. Later frames leave focus where the reader put it (a button, the
    // checkbox), which is why this only fires while the scope itself holds it.
    if window.focused(cx).as_ref() == Some(&focus) {
        let input_focus = input.read(cx).focus().clone();
        window.focus(&input_focus, cx);
    }
    let theme = cx.theme();
    let colors = &theme.colors;
    let close = {
        let root = root.clone();
        let takeover = takeover.clone();
        move |window: &mut Window, cx: &mut App| {
            give_back(&takeover, window, cx);
            root.update(cx, |root, cx| root.close_trust(cx));
        }
    };

    let preview = state.preview.as_ref();
    let preview_ok = preview.is_some_and(Result::is_ok);
    let linkable = state.linkable().is_some();
    let name = match preview {
        Some(Ok(preview)) if !preview.name.is_empty() => preview.name.clone(),
        Some(Ok(preview)) => preview.plugin_id.clone(),
        _ => t("shell.trust.title"),
    };

    let body = div().flex_1().min_h_0().child(
        ScrollArea::new("trust-body")
            .vertical()
            .shadows()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_4()
                    .py_2()
                    .child(match preview {
                        None => note(&t("shell.trust.invitation"), colors.fg_muted, cx),
                        Some(Err(error)) => note(error, colors.danger, cx),
                        Some(Ok(preview)) => declared(preview, cx),
                    }),
            ),
    );

    let read_row = {
        let toggle = root.clone();
        Checkbox::new("trust-read", state.read)
            .label(t("shell.trust.read"))
            .disabled(!preview_ok)
            .on_change(move |on, _, cx| {
                toggle.update(cx, |root, cx| root.set_trust_read(on, cx));
            })
    };

    let review = {
        let root = root.clone();
        Button::new("trust-review", t("shell.trust.review"))
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Sm)
            .on_click(move |_, _, cx| {
                root.update(cx, |root, cx| root.review_trust(cx));
            })
    };
    let link = {
        let root = root.clone();
        Button::new("trust-link", t("shell.trust.link"))
            .variant(ButtonVariant::Primary)
            .size(ControlSize::Sm)
            .disabled(!linkable)
            .on_click(move |_, _, cx| {
                root.update(cx, |root, cx| root.trust_link(cx));
            })
    };
    let cancel = {
        let close = close.clone();
        Button::new("trust-cancel", t("general.cancel"))
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .on_click(move |_, window, cx| close(window, cx))
    };

    let mut card = div()
        .w(px(WIDTH).min(window.viewport_size().width - px(MARGIN)))
        .max_h(window.viewport_size().height * 0.86)
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
                    Icon::new(IconName::Shield)
                        .size(IconSize::Md)
                        .color(colors.accent),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Lg))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.fg)
                        .child(Ellipsis::new(SharedString::from(name))),
                )
                .children(preview.and_then(|preview| match preview {
                    Ok(preview) if !preview.version.is_empty() => {
                        Some(chip(preview.version.clone(), colors.fg_subtle, cx))
                    }
                    _ => None,
                }))
                .child(div().flex_1())
                .child({
                    let close = close.clone();
                    IconButton::new("trust-close", IconName::X)
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, window, cx| close(window, cx))
                }),
        )
        .child(
            div()
                .flex_none()
                .px_4()
                .pt_2()
                .pb_1()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(t("shell.trust.intro")),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .py_2()
                .child(
                    div().flex_1().min_w_0().child(
                        PathInput::new("trust-path", input)
                            .directories()
                            .on_choose({
                                let root = root.clone();
                                let input = input.clone();
                                move |path, _window, cx| {
                                    let text = path.to_string_lossy().into_owned();
                                    input.update(cx, |input, cx| input.set_text(text, cx));
                                    root.update(cx, |root, cx| root.review_trust(cx));
                                }
                            }),
                    ),
                )
                .child(review),
        );
    card = card.child(body);
    if preview_ok {
        card = card.child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .py_2()
                .border_t_1()
                .border_color(colors.border)
                .child(read_row)
                .child(div().flex_1())
                .children(
                    preview
                        .and_then(|result| result.as_ref().ok())
                        .map(|preview| hazard_summary(preview, cx)),
                ),
        );
    }
    let mut foot = div()
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
                .min_w_0()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(Ellipsis::new(SharedString::from(t("shell.trust.paths")))),
        )
        .child(div().flex_1())
        .child(cancel)
        .child(link);
    if let Some(answer) = state.answer.as_deref() {
        foot = foot.child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(Ellipsis::new(SharedString::from(answer.to_owned()))),
        );
    }
    card = card.child(foot);

    let escaped = close.clone();
    Some(
        Backdrop::new("trust-scrim")
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
                                "trust-dialog-scope",
                                gpui::Role::Dialog,
                                t("shell.trust.title"),
                                None::<SharedString>,
                            )
                            .child(card),
                    ),
            )
            .into_any_element(),
    )
}

/// What a read directory says: what it is, how many commands it would run,
/// each of them graded, the manifest's warnings, and the UI it declares.
fn declared(preview: &Preview, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut column = div().flex().flex_col().gap_1p5();
    column = column.child(
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                div()
                    .text_size(theme.text_size(TextSize::Sm))
                    .text_color(colors.fg)
                    .child(Ellipsis::new(SharedString::from(format!(
                        "{} — {}",
                        preview.name, preview.plugin_id
                    )))),
            )
            .child(
                div()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_muted)
                    .child(
                        rust_i18n::t!("shell.trust.would_run", count = preview.commands.len())
                            .to_string(),
                    ),
            )
            .child(
                div()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_subtle)
                    .child(t("shell.trust.contract")),
            ),
    );
    if preview.commands.is_empty() {
        column = column.child(note(&t("shell.trust.no_commands"), colors.fg_muted, cx));
    }
    for (index, command) in preview.commands.iter().enumerate() {
        column = column.child(argv_line(index, command, cx));
    }
    for warning in &preview.warnings {
        column = column.child(note(warning, colors.warning, cx));
    }

    let slots = manager::slots_of(&preview.ui, &preview.plugin_id);
    let mut declared = div()
        .flex()
        .flex_col()
        .gap_1()
        .pt_2()
        .border_t_1()
        .border_color(colors.border)
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(colors.fg_muted)
                .child(t("shell.trust.declared")),
        );
    if slots.is_empty() {
        declared = declared.child(note(&t("shell.trust.no_ui"), colors.fg_muted, cx));
    }
    for slot in &slots {
        declared = declared.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.info)
                        .child(slot.kind),
                )
                .child(
                    div()
                        .w(px(110.))
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_muted)
                        .child(Ellipsis::new(SharedString::from(slot.id.clone()))),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg)
                        .child(Ellipsis::new(SharedString::from(slot.title.clone()))),
                ),
        );
    }
    column.child(declared).into_any_element()
}

/// One command as the review draws it: the number, the argv, the grade, and
/// the rule's own sentence under it when one matched.
fn argv_line(index: usize, command: &CommandLine, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let verdict = trust::hazard_for(&command.argv);
    let (color, reason, level) = match verdict {
        Some((reason, level)) => (
            match level {
                HazardLevel::High => colors.danger,
                HazardLevel::Medium => colors.warning,
            },
            Some(trust::reason_label(reason)),
            Some(level),
        ),
        None => (colors.fg_muted, None, None),
    };
    let mut row = div().flex().flex_col().gap_0p5().child(
        div()
            .flex()
            .items_start()
            .gap_2()
            .child(
                div()
                    .w(px(16.))
                    .flex_none()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_subtle)
                    .child((index + 1).to_string()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(color)
                    .child(SharedString::from(trust::render_command(command))),
            )
            .child(match level {
                Some(level) => grade_chip(level, cx),
                None => chip(t("shell.manager.stays"), colors.fg_subtle, cx),
            }),
    );
    if let Some(reason) = reason {
        row = row.child(note(&reason, color, cx));
    }
    row.into_any_element()
}

/// The footer's count: how many commands ask for attention, by grade. A
/// manifest with none says so rather than showing an empty count.
fn hazard_summary(preview: &Preview, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let (high, caution) = preview
        .commands
        .iter()
        .filter_map(|command| trust::hazard_for(&command.argv))
        .fold(
            (0usize, 0usize),
            |(high, caution), (_, level)| match level {
                HazardLevel::High => (high + 1, caution),
                HazardLevel::Medium => (high, caution + 1),
            },
        );
    div()
        .flex_none()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(if high > 0 {
            colors.danger
        } else if caution > 0 {
            colors.warning
        } else {
            colors.fg_subtle
        })
        .child(if high + caution == 0 {
            t("shell.trust.all_clear")
        } else {
            rust_i18n::t!("shell.trust.hazard_count", high = high, caution = caution).to_string()
        })
        .into_any_element()
}

fn grade_chip(level: HazardLevel, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let color = match level {
        HazardLevel::High => colors.danger,
        HazardLevel::Medium => colors.warning,
    };
    div()
        .flex_none()
        .px_1()
        .rounded(theme.radius(Radius::Sm))
        .border_1()
        .border_color(color.opacity(0.45))
        .bg(color.opacity(0.10))
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(color)
        .child(t(&format!("shell.manager.{}", level.key())))
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

fn note(text: &str, color: gpui::Hsla, cx: &App) -> AnyElement {
    let size = cx.theme().text_size(TextSize::Xs);
    div()
        .text_size(size)
        .text_color(color)
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

/// The directory a review reads, from the field's text.
pub(crate) fn directory_of(typed: &str) -> Option<PathBuf> {
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }
    Some(Path::new(typed).to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Link needs three things at once: a preview, a successful read, and the
    /// reader's acknowledgement. Dropping any one withdraws the path.
    #[test]
    fn a_link_is_offered_only_after_a_read_and_an_acknowledgement() {
        let dir = PathBuf::from("/tmp/plugin");
        let mut state = State::default();
        state.reviewed(dir.clone(), Err("no manifest".into()));
        assert!(state.linkable().is_none(), "a failed read cannot be linked");
        state.set_read(true);
        assert!(
            state.linkable().is_none(),
            "reading 'yes' to nothing is not a link"
        );

        let preview = Preview {
            plugin_id: "board".into(),
            name: "Board".into(),
            version: "0.1.0".into(),
            commands: Vec::new(),
            warnings: Vec::new(),
            ui: Default::default(),
        };
        state.reviewed(dir.clone(), Ok(preview));
        assert!(
            state.linkable().is_none(),
            "a new read clears the previous acknowledgement"
        );
        state.set_read(true);
        assert_eq!(state.linkable(), Some(dir));
    }

    #[test]
    fn a_blank_field_names_no_directory() {
        assert!(directory_of("  ").is_none());
        assert_eq!(directory_of(" /tmp/p "), Some(PathBuf::from("/tmp/p")));
    }
}
