//! The pre-link review surface: the shell's half of `crate::plugins::trust`.
//!
//! A plugin is a directory of commands herdr will run as the user, and linking
//! one is the moment that becomes true. The daemon validates a manifest and
//! refuses a broken one, but it never shows the commands themselves — so the
//! client does, before the link, on the one screen a person reads in a hurry.
//!
//! The surface is laid over the shell and holds the keyboard while it is open,
//! like the settings page and the command palette: the pane beside it must not
//! be typed into while a path is being entered.
//!
//! Reading the manifest is a synchronous file read of one small TOML file, per
//! click, on the UI thread. It starts no process: the commands a manifest names
//! are shown, never run, until the daemon runs them itself.

use std::path::{Path, PathBuf};

use gpui_kit::component::{ActiveTheme, Theme, h_flex, v_flex};
use gpui_kit::*;

use super::trust::{self, Preview};
use crate::shell::registry::{Dispatch, ShellRequest};

/// The review as the shell holds it: the path that was typed, what reading it
/// found, and the daemon's answer to a link.
#[derive(Default)]
pub(crate) struct TrustReview {
    open: bool,
    /// What is in the input. A plugin directory, or nothing.
    path: String,
    /// The directory the preview below is about, so Link links what is on
    /// screen rather than whatever has since been typed.
    reviewed: Option<PathBuf>,
    /// `None` until a path has been read. A read that failed keeps its sentence.
    preview: Option<Result<Preview, String>>,
    /// What the daemon said to the last link attempt, until the registry
    /// refresh says otherwise.
    answer: Option<String>,
}

impl TrustReview {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Open on an empty input: a review that remembered the last plugin would
    /// hide the one about to be looked at.
    pub(crate) fn open(&mut self) {
        self.open = true;
        self.path.clear();
        self.reviewed = None;
        self.preview = None;
        self.answer = None;
    }

    /// Open already reading one directory: what a click on an installed plugin
    /// asks for, so its commands can be read back after the fact.
    pub(crate) fn open_at(&mut self, dir: &Path) {
        self.open();
        self.path = dir.to_string_lossy().into_owned();
        self.review();
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    /// The daemon's answer to a link, or that one is in flight.
    pub(crate) fn note(&mut self, line: String) {
        self.answer = Some(line);
    }

    /// Read the path in the input, and say what it would run.
    pub(crate) fn review(&mut self) {
        let typed = self.path.trim();
        self.answer = None;
        if typed.is_empty() {
            self.reviewed = None;
            self.preview = Some(Err("Type the directory a plugin lives in.".to_owned()));
            return;
        }
        let dir = PathBuf::from(typed);
        self.preview = Some(trust::preview(&dir));
        self.reviewed = Some(dir);
    }

    /// What Enter does: read the path, or link it when it has already been read
    /// and what was read is readable. Reading twice would make Enter the only
    /// key, and the link the only mouse action, which is the wrong way round.
    pub(crate) fn enter(&mut self) -> ShellRequest {
        let typed = self.path.trim().to_owned();
        let reviewed = self
            .reviewed
            .as_ref()
            .is_some_and(|dir| dir.to_string_lossy() == typed);
        let readable = self.preview.as_ref().is_some_and(Result::is_ok);
        if reviewed && readable {
            ShellRequest::LinkPlugin(typed)
        } else {
            ShellRequest::ReviewPlugin
        }
    }

    /// Type into the input. Returns whether the keystroke was the review's,
    /// which is what keeps it from reaching the pane underneath.
    pub(crate) fn keystroke(&mut self, keystroke: &Keystroke) -> bool {
        // A chord is never path text; shift is not a chord, it is how an
        // upper-case letter or a shifted symbol is typed.
        let chord = keystroke.modifiers.control
            || keystroke.modifiers.alt
            || keystroke.modifiers.platform
            || keystroke.modifiers.function;
        if chord {
            return false;
        }
        match keystroke.key.as_str() {
            "backspace" => {
                self.path.pop();
            }
            _ => {
                let typed = keystroke
                    .key_char
                    .as_deref()
                    .filter(|text| !text.is_empty())
                    .unwrap_or(keystroke.key.as_str());
                if typed.chars().count() == 1 {
                    self.path.push_str(typed);
                }
            }
        }
        true
    }

    fn path(&self) -> &str {
        &self.path
    }
}

/// The review, laid over the shell. `key` is the shell's own handler, so a
/// keystroke means the same here as it does in the palette.
pub(crate) fn overlay(
    review: &TrustReview,
    focus: &FocusHandle,
    dispatch: &Dispatch,
    key: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let close = dispatch.clone();
    let read = dispatch.clone();
    let typed_path = review.path().to_owned();
    let linkable = review
        .preview
        .as_ref()
        .is_some_and(Result::is_ok)
        .then(|| typed_path.clone());
    let input = if typed_path.is_empty() {
        div()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child("/path/to/plugin")
    } else {
        div()
            .text_sm()
            .text_color(theme.foreground)
            .child(typed_path)
    };
    let mut boxed = v_flex()
        .id("plugin-trust")
        .w(px(680.))
        .max_h(px(620.))
        .overflow_hidden()
        .rounded_md()
        .bg(theme.background)
        .text_color(theme.foreground)
        .border_1()
        .border_color(theme.border)
        .track_focus(focus)
        .on_key_down(key)
        .child(
            h_flex()
                .w_full()
                .flex_none()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex_none()
                        .text_sm()
                        .child("Link a plugin — what it would run"),
                )
                .child(div().flex_1())
                .child(button("plugin-trust-close", "Close", &theme).on_click({
                    let close = close.clone();
                    move |_, window, cx| {
                        close(ShellRequest::ClosePluginTrust, window, cx);
                    }
                })),
        )
        .child(
            h_flex()
                .w_full()
                .flex_none()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(div().flex_1().min_w_0().child(input))
                .child(button("plugin-trust-review", "Review", &theme).on_click({
                    let read = read.clone();
                    move |_, window, cx| {
                        read(ShellRequest::ReviewPlugin, window, cx);
                    }
                }))
                .children(linkable.map(|path| {
                    button("plugin-trust-link", "Link", &theme).on_click({
                        let link = dispatch.clone();
                        move |_, window, cx| {
                            link(ShellRequest::LinkPlugin(path.clone()), window, cx);
                        }
                    })
                })),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .p_2()
                .child(body(review.preview.as_ref(), &theme)),
        );
    if let Some(answer) = review.answer.as_deref() {
        boxed = boxed.child(
            div()
                .flex_none()
                .w_full()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(answer.to_owned()),
        );
    }
    v_flex()
        .id("plugin-trust-overlay")
        .absolute()
        .inset_0()
        .occlude()
        .size_full()
        .items_center()
        .justify_center()
        // A scrim rather than a page: linking a plugin is about what is behind
        // this — the sidebar the plugin appears in — so the shell stays visible
        // and dimmed instead of hidden.
        .bg(theme.background.opacity(0.72))
        .child(boxed)
        .into_any_element()
}

/// What the review says: an invitation, why a path could not be read, or the
/// commands a manifest declares.
fn body(preview: Option<&Result<Preview, String>>, theme: &Theme) -> AnyElement {
    match preview {
        None => note(
            "Type a plugin directory and press Enter to read the commands it would run.",
            theme.muted_foreground,
        ),
        Some(Err(error)) => note(error, theme.danger),
        Some(Ok(preview)) => declared(preview, theme),
    }
}

/// One plugin's declarations, as one screen: what it is, how many commands it
/// would run, each of them, and the ones worth stopping on.
fn declared(preview: &Preview, theme: &Theme) -> AnyElement {
    let mut column = v_flex().w_full().gap_1().child(
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .child(format!("{} — {}", preview.name, preview.plugin_id)),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(preview.version.clone()),
            ),
    );
    column = column.child(note(
        &format!(
            "{} command(s) would run as you, with the plugin's directory as the working directory.",
            preview.commands.len()
        ),
        theme.muted_foreground,
    ));
    for warning in &preview.warnings {
        column = column.child(note(warning, theme.warning));
    }
    if preview.commands.is_empty() {
        column = column.child(note(
            "This manifest declares no command.",
            theme.muted_foreground,
        ));
    }
    for (index, command) in preview.commands.iter().enumerate() {
        let hazard = preview.hazards.iter().find(|hazard| hazard.index == index);
        let line = trust::render_command(command);
        let mut row = v_flex().w_full().gap_0p5().child(
            div()
                .w_full()
                .min_w_0()
                .text_xs()
                .text_color(if hazard.is_some() {
                    theme.danger
                } else {
                    theme.foreground
                })
                .child(line),
        );
        if let Some(hazard) = hazard {
            row = row.child(note(hazard.reason, theme.danger));
        }
        column = column.child(row);
    }
    column.into_any_element()
}

fn note(text: &str, color: Hsla) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .text_xs()
        .text_color(color)
        .child(text.to_owned())
        .into_any_element()
}

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

    fn typed(text: &str) -> TrustReview {
        let mut review = TrustReview::default();
        review.open();
        review.path = text.to_owned();
        review
    }

    #[test]
    fn enter_reads_a_path_before_it_links_one() {
        let mut review = typed("/tmp/nothing-here");
        assert!(matches!(review.enter(), ShellRequest::ReviewPlugin));
        // Reading it left nothing readable, so Enter reads again rather than
        // linking a directory that produced no manifest.
        assert!(matches!(review.enter(), ShellRequest::ReviewPlugin));
    }

    #[test]
    fn enter_links_once_the_path_on_screen_is_the_one_that_was_read() {
        let dir = std::env::temp_dir().join(format!("zenix-trust-view-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory to read");
        std::fs::write(
            dir.join(trust::FILE_NAME),
            "id = \"test.plugin\"\nname = \"Test\"\nversion = \"0.1.0\"\nmin_herdr_version = \"0.9.0\"\n",
        )
        .expect("a manifest to read");
        let mut review = typed(&dir.to_string_lossy());
        assert!(matches!(review.enter(), ShellRequest::ReviewPlugin));
        // What the shell does with that request.
        review.review();
        match review.enter() {
            ShellRequest::LinkPlugin(path) => assert_eq!(path, dir.to_string_lossy()),
            _ => panic!("expected a link of the path that was read"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typing_after_a_review_is_not_treated_as_reviewed() {
        let dir = std::env::temp_dir().join(format!("zenix-trust-view-2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory to read");
        std::fs::write(
            dir.join(trust::FILE_NAME),
            "id = \"test.plugin\"\nname = \"Test\"\nversion = \"0.1.0\"\nmin_herdr_version = \"0.9.0\"\n",
        )
        .expect("a manifest to read");
        let mut review = typed(&dir.to_string_lossy());
        review.review();
        review.path.push('x');
        assert!(
            matches!(review.enter(), ShellRequest::ReviewPlugin),
            "a path that has changed since the read must be read again"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_input_is_a_read_of_nothing_and_says_so() {
        let mut review = typed("   ");
        review.review();
        assert!(matches!(
            review.preview,
            Some(Err(ref message)) if message.contains("directory")
        ));
        assert!(review.reviewed.is_none());
    }

    #[test]
    fn a_chord_is_not_path_text_and_shift_is() {
        let mut review = typed("/tmp");
        let chord = Keystroke {
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
            key: "v".into(),
            key_char: Some("v".into()),
        };
        assert!(!review.keystroke(&chord));
        assert_eq!(review.path(), "/tmp");
        let shifted = Keystroke {
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            key: "S".into(),
            key_char: Some("S".into()),
        };
        assert!(review.keystroke(&shifted));
        assert_eq!(review.path(), "/tmpS");
        assert!(review.keystroke(&Keystroke {
            modifiers: Modifiers::default(),
            key: "backspace".into(),
            key_char: None,
        }));
        assert_eq!(review.path(), "/tmp");
    }
}
