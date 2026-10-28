//! The project layout inspector: `.zenix/layout.toml` as the file it is.
//!
//! The template travels with the repository; this surface is where it is read
//! whole. The design gives it an 840px modal (`docs/ui/redesign` `.layout-modal`):
//! the file's own text with line numbers on the left, the split tree it
//! describes on the right, and the three facts a reader needs — how many panes,
//! how many splits, and what the file is for.
//!
//! The left column shows the file's bytes, not a re-serialized tree: comments a
//! person wrote are part of what they are reviewing, and a template printed
//! back from its parse would lose them. A file that does not parse keeps its
//! text and shows the parser's sentence instead of hiding either.
//!
//! The two actions are the daemon's: Export reads `layout.export` for the
//! focused tab and writes the project's file, Apply sends the file back through
//! `layout.apply`. Apply always creates a tab — it is not idempotent — which the
//! side column says in words rather than in a tooltip. Both calls run off the
//! UI thread, and the file is re-read when export lands.

use ely_core::{
    buttons::{Button, ButtonVariant, IconButton},
    forms::{PathInput, TextInput},
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
use crate::layout::{LayoutState, Node, SplitDirection, Template};

/// The modal's widest card and the room it keeps from the window.
const WIDTH: f32 = 840.;
const MARGIN: f32 = 48.;
/// The grid's shared-edge gap, as the facts column states it: the painter's own
/// card gap, not a config value.
const GAP: f32 = 8.;

/// The inspector's own state: open, and nothing else. What it shows belongs to
/// The inspector's own state: open, and — while no project has been read — the
/// directory field that points it at one. What it shows belongs to the shell's
/// [`LayoutState`], which the focused pane's cwd keeps current.
#[derive(Default)]
pub(crate) struct State {
    open: bool,
    /// The field the first-run entry needs: a repository to read before any
    /// pane exists to name one. Dropped when the sheet closes, so a later
    /// opening starts from whatever the shell knows then.
    input: Option<Entity<TextInput>>,
}

impl State {
    pub(crate) fn open(&mut self, input: Option<Entity<TextInput>>) {
        self.open = true;
        self.input = input;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.input = None;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// The directory field, while this opening offers one.
    pub(crate) fn input(&self) -> Option<&Entity<TextInput>> {
        self.input.as_ref()
    }

    /// What the field says, trimmed. Empty when there is no field.
    pub(crate) fn typed(&self, cx: &App) -> String {
        self.input
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_owned())
            .unwrap_or_default()
    }
}

/// The modal over the shell, or nothing while it is closed. `workspace` and
/// `tab` are the focused ids Apply and Export need; each action stays
/// unavailable without its own.
pub(crate) fn overlay(
    state: &State,
    layout: &LayoutState,
    workspace: Option<&str>,
    tab: Option<&str>,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    if !state.is_open() {
        return None;
    }
    let takeover = take_focus("layout-dialog-takeover", window, cx);
    let focus = takeover.read(cx).focus.clone();
    let theme = cx.theme();
    let colors = &theme.colors;
    let close = {
        let root = root.clone();
        let takeover = takeover.clone();
        move |window: &mut Window, cx: &mut App| {
            give_back(&takeover, window, cx);
            root.update(cx, |root, cx| root.close_layout(cx));
        }
    };
    let path = layout
        .path
        .clone()
        .or_else(|| {
            layout
                .root
                .as_ref()
                .map(|root| root.join(crate::layout::DIR).join(crate::layout::FILE_NAME))
        })
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| t("shell.layout_sheet.no_project"));
    let sync_badge = if layout.template.is_some() {
        (t("shell.layout_sheet.synced"), colors.success)
    } else if layout.path.is_some() {
        (t("shell.layout_sheet.broken"), colors.danger)
    } else {
        (t("shell.layout_sheet.absent"), colors.fg_subtle)
    };

    let apply = {
        let root = root.clone();
        Button::new("layout-apply", t("shell.layout_sheet.apply"))
            .variant(ButtonVariant::Primary)
            .size(ControlSize::Sm)
            .disabled(layout.template.is_none() || workspace.is_none())
            .on_click(move |_, _, cx| {
                root.update(cx, |root, cx| root.apply_layout(cx));
            })
    };
    let export = {
        let root = root.clone();
        Button::new("layout-export", t("shell.layout_sheet.export"))
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Sm)
            .disabled(tab.is_none() || layout.root.is_none())
            .on_click(move |_, _, cx| {
                root.update(cx, |root, cx| root.export_layout(cx));
            })
    };
    let copy = {
        let path = path.clone();
        Button::new("layout-copy-path", t("shell.layout_sheet.copy_path"))
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            // With no file read yet there is no path to copy: the header's
            // sentence is a state, not a location.
            .disabled(layout.path.is_none())
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(path.clone()));
            })
    };

    let mut side = div()
        .w(px(380.))
        .flex_none()
        .flex()
        .flex_col()
        .gap_2()
        .p_3();
    side = side.child(
        div()
            .text_size(theme.text_size(TextSize::Xs))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(colors.fg_subtle)
            .child(t("shell.layout_sheet.diagram")),
    );
    if let Some(template) = layout.template.as_ref() {
        side = side.child(
            div()
                .h(px(180.))
                .flex()
                .gap_1()
                .rounded(theme.radius(Radius::Md))
                .border_1()
                .border_color(colors.border)
                .bg(colors.surface)
                .p_2()
                .child(tree_node(&template.root, cx)),
        );
        side = side.child(facts(template, cx));
    } else {
        side = side.child(
            div()
                .h(px(120.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius(Radius::Md))
                .border_1()
                .border_color(colors.border)
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(t("shell.layout_sheet.empty")),
        );
    }
    side = side.child(
        div()
            .rounded(theme.radius(Radius::Md))
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .p_2()
            .text_size(theme.text_size(TextSize::Xs))
            .text_color(colors.fg_muted)
            .child(t("shell.layout_sheet.not_idempotent")),
    );
    // Apply needs a workspace to restore into. Saying so here is what makes
    // "open layout" honest on a first run, where the file can be read but not
    // yet put anywhere.
    if layout.template.is_some() && workspace.is_none() {
        side = side.child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.warning)
                .child(t("shell.layout_sheet.needs_workspace")),
        );
    }
    if let Some(last) = layout.last.as_deref() {
        side = side.child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.success)
                .child(Ellipsis::new(SharedString::from(last.to_owned()))),
        );
    }
    if let Some(error) = layout.error.as_deref() {
        side = side.child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.danger)
                .child(SharedString::from(error.to_owned())),
        );
    }
    side = side.child(
        div()
            .text_size(theme.text_size(TextSize::Xs))
            .text_color(colors.fg_subtle)
            .child(t("shell.layout_sheet.walk")),
    );

    let mut code_column = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(colors.border);
    if layout.path.is_none() {
        code_column = code_column.child(
            div()
                .p_4()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(t("shell.layout_sheet.no_template")),
        );
    } else {
        let text = layout.text.clone().unwrap_or_default();
        let mut lines = div().flex().flex_col().py_2();
        for (index, line) in text.lines().enumerate() {
            lines = lines.child(
                div()
                    .flex()
                    .gap_3()
                    .px_3()
                    .child(
                        div()
                            .w(px(22.))
                            .flex_none()
                            .text_right()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_disabled)
                            .child((index + 1).to_string()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_muted)
                            // The file's indentation is its structure: leading
                            // spaces travel as no-break ones so a wrapped run
                            // keeps them.
                            .child(SharedString::from(line.replace(' ', "\u{a0}"))),
                    ),
            );
        }
        code_column = code_column.child(
            div().flex_1().min_h_0().child(
                ScrollArea::new("layout-code")
                    .vertical()
                    .shadows()
                    .size_full()
                    .child(lines),
            ),
        );
    }

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
                    Icon::new(IconName::FileCode)
                        .size(IconSize::Sm)
                        .color(colors.accent),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Sm))
                        .text_color(colors.fg)
                        .child(".zenix/layout.toml"),
                )
                .child(
                    div()
                        .flex_none()
                        .px_1()
                        .rounded(theme.radius(Radius::Sm))
                        .border_1()
                        .border_color(sync_badge.1.opacity(0.45))
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(sync_badge.1)
                        .child(sync_badge.0),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(Ellipsis::new(SharedString::from(path))),
                )
                .child(div().flex_1())
                .child(copy)
                .child(export)
                .child(apply)
                .child({
                    let close = close.clone();
                    IconButton::new("layout-close", IconName::X)
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, window, cx| close(window, cx))
                }),
        )
        // With no project read yet, the sheet asks for one instead of showing
        // an empty file: the first-run entry opens exactly this state.
        .children(state.input().map(|input| picker_row(input, root, cx)))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(code_column)
                .child(side),
        )
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
                        .child(Ellipsis::new(SharedString::from(t(
                            "shell.layout_sheet.foot",
                        )))),
                )
                .child(div().flex_1())
                .child({
                    let close = close.clone();
                    Button::new("layout-done", t("settings.close"))
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .on_click(move |_, window, cx| close(window, cx))
                }),
        );

    let escaped = close.clone();
    Some(
        Backdrop::new("layout-scrim")
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
                                "layout-dialog-scope",
                                gpui::Role::Dialog,
                                t("shell.layout_sheet.title"),
                                None::<SharedString>,
                            )
                            .child(card),
                    ),
            )
            .into_any_element(),
    )
}

/// The directory the sheet reads: how the inspector is pointed at a project
/// before any pane names one. The system dialog and the field write the same
/// state, and a chosen directory or a submitted path re-scans it in place.
fn picker_row(input: &Entity<TextInput>, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(colors.border)
        .child(
            div()
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(t("shell.layout_sheet.pick")),
        )
        .child(
            div().flex_1().min_w_0().child(
                PathInput::new("layout-path", input)
                    .directories()
                    .on_choose({
                        let root = root.clone();
                        let input = input.clone();
                        move |path, _window, cx| {
                            input.update(cx, |input, cx| {
                                input.set_text(path.to_string_lossy().into_owned(), cx);
                            });
                            root.update(cx, |root, cx| root.scan_layout_from_input(cx));
                        }
                    }),
            ),
        )
        .into_any_element()
}

/// The split tree as the restored tab would be shaped: a right split is a row,
/// a down split is a column, and each ratio is the child's grow share.
fn tree_node(node: &Node, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    match node {
        Node::Pane { pane } => div()
            .min_w_0()
            .min_h_0()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .rounded(theme.radius(Radius::Sm))
            .border_1()
            .border_color(colors.accent.opacity(0.55))
            .bg(colors.bg)
            .px_1()
            .child(
                div()
                    .max_w_full()
                    .text_size(theme.text_size(TextSize::Xs))
                    .text_color(colors.fg_muted)
                    .child(Ellipsis::new(SharedString::from(pane_tag(pane)))),
            )
            .into_any_element(),
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => div()
            .min_w_0()
            .min_h_0()
            .flex_1()
            .flex()
            .gap_1()
            .map(|split| match direction {
                SplitDirection::Right => split.flex_row(),
                SplitDirection::Down => split.flex_col(),
            })
            .child(
                div()
                    .flex_grow(*ratio)
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .child(tree_node(first, cx)),
            )
            .child(
                div()
                    .flex_grow(1.0 - ratio)
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .child(tree_node(second, cx)),
            )
            .into_any_element(),
    }
}

/// What a leaf is called: its label, the command it would run, or "pane".
fn pane_tag(pane: &crate::layout::Pane) -> String {
    if let Some(label) = pane.label.as_deref() {
        return label.to_owned();
    }
    if let Some(command) = pane.command.as_ref()
        && !command.is_empty()
    {
        return command.join(" ");
    }
    t("shell.layout_sheet.pane")
}

/// The design's four facts: panes, splits, schema, the grid's gap.
fn facts(template: &Template, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let panes = template.root.panes();
    let splits = panes.saturating_sub(1);
    div()
        .flex()
        .gap_1()
        .children(
            [
                (panes.to_string(), t("shell.layout_sheet.panes")),
                (splits.to_string(), t("shell.layout_sheet.splits")),
                ("v1".to_owned(), t("shell.layout_sheet.schema")),
                (format!("{GAP}px"), t("shell.layout_sheet.gap")),
            ]
            .map(|(number, label)| {
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .rounded(theme.radius(Radius::Sm))
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.surface)
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Sm))
                            .text_color(colors.fg)
                            .child(number),
                    )
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_subtle)
                            .child(label),
                    )
                    .into_any_element()
            }),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A leaf is named by its label, else by its command, else by the locale's
    /// word — never by an empty line.
    #[test]
    fn a_leaf_always_has_a_name() {
        let mut pane = crate::layout::Pane::default();
        assert_eq!(pane_tag(&pane), t("shell.layout_sheet.pane"));
        pane.command = Some(vec!["cargo".into(), "test".into()]);
        assert_eq!(pane_tag(&pane), "cargo test");
        pane.label = Some("builder".into());
        assert_eq!(pane_tag(&pane), "builder");
    }

    /// The inspector is one open flag with no hidden page state; opening twice
    /// and closing is idempotent.
    #[test]
    fn the_inspector_opens_and_closes() {
        let mut state = State::default();
        assert!(!state.is_open());
        state.open(None);
        state.open(None);
        assert!(state.is_open());
        state.close();
        assert!(!state.is_open());
    }
}
