//! The command palette: the shell's own directory and the daemon's plugin
//! actions, in one filterable list.
//!
//! The palette is a row of data, not a second command system: an entry is
//! either a command from the directory or a plugin action, and both leave
//! through the same [`ShellRequest`] the sidebar's clicks use. Nothing here
//! knows what a plugin action does; it hands the action back to the shell,
//! which hands it to the daemon.
//!
//! A plugin row is one catalog entry ([`crate::plugins::actions`]) — the same
//! entry the keymap binds a chord to and the MCP bridge exposes as a tool, so a
//! row cannot outlive the action it would run.
//!
//! The query is a real input: it holds the keyboard while the palette is
//! open, so IME, the caret and paste are the component's, not a hand-rolled
//! keystroke scanner. The four keys around it — up, down, enter, escape —
//! are the palette's own actions, bound in the [`CONTEXT`] key context the
//! dialog carries: one ring out from the input. A single-line input has no
//! caret movement of its own to answer up and down, and enter and escape
//! propagate, so the palette's bindings are what the four keys reach.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Icon, Theme, h_flex, v_flex};
use gpui_kit::*;

use std::rc::Rc;
use std::sync::Arc;

use super::commands::{COMMANDS, Command};
use super::keymap::Keymap;
use super::registry::{Dispatch, ShellRequest};
use crate::i18n::t;
use crate::plugins::actions::Exposed;

/// The key context the palette's dialog carries. The four bindings the shell
/// registers against it (`shell::keymap::bind_keys`) match only while this
/// context is on the focused path, so no other input in the app loses a key.
pub(crate) const CONTEXT: &str = "Palette";

gpui_kit::actions!(
    zenix,
    [
        PaletteSelectPrev,
        PaletteSelectNext,
        PaletteRun,
        PaletteDismiss
    ]
);

/// How many runs the recency ring remembers. A ring, not a list: the one
/// after the last forgets the first.
const RECENT_LEN: usize = 16;

/// One row the palette can run.
#[derive(Clone)]
pub(crate) enum Entry {
    Command(Command),
    /// A workspace of the session: picking one is the same focus request the
    /// sidebar's rows make, so the palette reaches a workspace the sidebar
    /// cannot show (it may belong to another rail view or be scrolled away).
    /// The tab count rides along because the row is the prototype's "how big is
    /// it" line: it is what tells three workspaces apart before picking one.
    Workspace {
        workspace_id: String,
        label: String,
        branch: Option<String>,
        tabs: usize,
    },
    Plugin(Arc<Exposed>),
}

impl Entry {
    /// Which section the row belongs to, in the order the sections are
    /// offered: the shell's directory, the session's workspaces, the plugins'
    /// actions.
    fn group(&self) -> u8 {
        match self {
            Self::Command(_) => 0,
            Self::Workspace { .. } => 1,
            Self::Plugin(_) => 2,
        }
    }

    /// What the row is called.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Command(command) => COMMANDS
                .iter()
                .find(|info| info.command == *command)
                .map(|info| info.label())
                .unwrap_or_else(|| format!("{command:?}")),
            // The prototype's workspace row reads "name — branch", so the
            // branch is on the label line rather than a second dim one.
            Self::Workspace { label, branch, .. } => match branch {
                Some(branch) => format!("{label} — {branch}"),
                None => label.clone(),
            },
            Self::Plugin(entry) => entry.label().to_owned(),
        }
    }

    /// The dim line under the label: which plugin an action belongs to, the
    /// workspace's branch, or the command's own id.
    fn detail(&self) -> String {
        match self {
            Self::Command(command) => COMMANDS
                .iter()
                .find(|info| info.command == *command)
                .map(|info| info.id.to_owned())
                .unwrap_or_default(),
            Self::Workspace { branch, .. } => branch.clone().unwrap_or_default(),
            Self::Plugin(entry) => entry.action.qualified_id(),
        }
    }

    /// The keystroke that reaches this row without the palette. A plugin's
    /// chord is shown the way it is pressed: claimed outright when it carries a
    /// modifier of its own, after the prefix when it is a bare key — see
    /// [`crate::shell::keymap`]. A workspace carries no chord; its right-hand
    /// slot says how many tabs it holds instead.
    fn hint(&self, keymap: &Keymap) -> Option<String> {
        match self {
            Self::Plugin(entry) => entry.chord().map(|key| keymap.chord_label(key)),
            Self::Workspace { tabs, .. } => {
                Some(rust_i18n::t!("shell.palette.tab_count", count = tabs).to_string())
            }
            Self::Command(command) => {
                let info = COMMANDS.iter().find(|info| info.command == *command)?;
                if let Some(direct) = info.direct.first() {
                    return Some((*direct).to_owned());
                }
                info.prefixed
                    .first()
                    .map(|chord| format!("{} {chord}", keymap.prefix_label()))
            }
        }
    }

    /// What running the row asks the shell to do.
    pub(crate) fn request(&self) -> ShellRequest {
        match self {
            Self::Command(command) => ShellRequest::Command(*command),
            // The palette is a click like any other: the daemon still assembles
            // the invocation context, and only the source is ours to name.
            Self::Plugin(entry) => ShellRequest::PluginAction {
                action: entry.action.clone(),
                source: "palette",
            },
            Self::Workspace { workspace_id, .. } => {
                ShellRequest::FocusWorkspace(workspace_id.clone())
            }
        }
    }

    /// The identity the recency ring remembers a run by. Ids, not labels: a
    /// label follows the locale, an id is what the next snapshot names the
    /// same way.
    fn key(&self) -> String {
        match self {
            Self::Command(command) => COMMANDS
                .iter()
                .find(|info| info.command == *command)
                .map(|info| info.id.to_owned())
                .unwrap_or_else(|| format!("{command:?}")),
            Self::Workspace { workspace_id, .. } => workspace_id.clone(),
            Self::Plugin(entry) => entry.action.qualified_id(),
        }
    }
}

/// The palette's own state: whether it is open, the query input it lends the
/// keyboard to, the scroll state of the drawn rows, which row is selected,
/// and the ring of recently run rows.
#[derive(Default)]
pub(crate) struct Palette {
    open: bool,
    query: Option<Entity<InputState>>,
    list: Option<ListState>,
    selected: usize,
    recent: Vec<String>,
    /// The subscription to the query input's events, held from the first
    /// open on: a dropped subscription would stop delivering.
    subscription: Option<Subscription>,
}

impl Palette {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Open with an empty query: a palette that remembered the last search
    /// would hide the command the user is about to look for. The input is
    /// created on the first open — it wants a window, which construction
    /// does not have — and returned, because the shell has to focus it and
    /// subscribe to it once.
    pub(crate) fn open(&mut self, window: &mut Window, cx: &mut App) -> Entity<InputState> {
        self.open = true;
        self.selected = 0;
        let input = match &self.query {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx));
                input.update(cx, |state, cx| {
                    state.set_placeholder(t("shell.palette.prompt").to_string(), window, cx);
                });
                self.query = Some(input.clone());
                input
            }
        };
        input.update(cx, |state, cx| state.set_value("", window, cx));
        input
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    /// The input the keyboard sits in while the palette is open.
    pub(crate) fn input(&self) -> Option<Entity<InputState>> {
        self.query.clone()
    }

    /// Whether the input's events are already subscribed to.
    pub(crate) fn input_subscribed(&self) -> Option<&Subscription> {
        self.subscription.as_ref()
    }

    /// Hold the subscription the shell took on the first open.
    pub(crate) fn hold_subscription(&mut self, subscription: Subscription) {
        self.subscription = Some(subscription);
    }

    /// A clone of the rows' scroll state, for the selection to take the
    /// scroll with it.
    pub(crate) fn list(&self) -> Option<ListState> {
        self.list.clone()
    }

    /// The query as typed, read off the input.
    fn query_text(&self, cx: &App) -> String {
        self.query
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// The rows this query leaves. [`Self::matches`] reads the live query off
    /// the input; this is the pure core the tests drive without a window.
    pub(crate) fn filtered<'a>(
        query: &str,
        recent: &[String],
        entries: &'a [Entry],
    ) -> Vec<&'a Entry> {
        let query = query.trim().to_lowercase();
        let rank = |entry: &Entry| {
            recent
                .iter()
                .position(|key| *key == entry.key())
                .unwrap_or(usize::MAX)
        };
        let mut matches: Vec<&Entry> = entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.label().to_lowercase().contains(&query)
                    || entry.detail().to_lowercase().contains(&query)
            })
            .collect();
        // Sections stay contiguous — the recency only reorders inside one, so
        // a header keeps meaning what it says.
        matches.sort_by_key(|entry| (entry.group(), rank(entry)));
        matches
    }

    /// The rows this query leaves, most recently run first inside each
    /// section.
    pub(crate) fn matches<'a>(&self, entries: &'a [Entry], cx: &App) -> Vec<&'a Entry> {
        Self::filtered(&self.query_text(cx), &self.recent, entries)
    }

    /// A row was run: the ring remembers it, most recent first.
    pub(crate) fn note_run(&mut self, entry: &Entry) {
        let key = entry.key();
        self.recent.retain(|seen| seen != &key);
        self.recent.insert(0, key);
        self.recent.truncate(RECENT_LEN);
    }

    /// Move the selection `step` rows, stopping at the ends instead of
    /// wrapping: an end is information, and a wrap past it is a jump to a row
    /// the eye did not travel to.
    pub(crate) fn step(&mut self, step: isize, rows: usize) {
        if rows == 0 {
            return;
        }
        self.selected = if step < 0 {
            self.selected.saturating_sub(step.unsigned_abs())
        } else {
            (self.selected + step as usize).min(rows - 1)
        };
    }

    /// Follow the rows the palette draws changing underneath the selection.
    pub(crate) fn clamp_selection(&mut self, rows: usize) {
        if rows == 0 {
            self.selected = 0;
        } else if self.selected >= rows {
            self.selected = rows - 1;
        }
    }

    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    /// Return the selection to the first row: the rows the query leaves
    /// changed underneath it.
    pub(crate) fn reset_selection(&mut self) {
        self.selected = 0;
    }

    /// The row running answers to.
    pub(crate) fn selected_entry<'a>(&self, matches: &[&'a Entry]) -> Option<&'a Entry> {
        matches.get(self.selected).copied()
    }

    /// Point the rows' scroll state at `count`: called from the events that
    /// change what the rows are — a keystroke, a snapshot push — never from
    /// render. A changed count resets the scroll, which is right: a new set
    /// of rows is a new list.
    pub(crate) fn sync_rows(&mut self, count: usize) {
        match &self.list {
            Some(list) if list.item_count() == count => {}
            Some(list) => list.reset(count),
            None => {
                self.list = Some(ListState::new(count, ListAlignment::Top, px(200.)));
            }
        }
        self.clamp_selection(count);
    }
}

/// One drawn row: a section's name where the rows change kind, or an entry.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PlanRow {
    Header(u8),
    /// An index into the matches.
    Item(usize),
}

/// The flat row plan the list draws: the matches in their matched order, with
/// a header wherever the section changes. Both the render and the selection's
/// scroll-to-view walk this, so they cannot disagree about which row is
/// where.
pub(crate) fn plan(matches: &[&Entry]) -> Vec<PlanRow> {
    let mut rows = Vec::new();
    let mut last_group: Option<u8> = None;
    for (index, entry) in matches.iter().enumerate() {
        let group = entry.group();
        if last_group != Some(group) {
            rows.push(PlanRow::Header(group));
            last_group = Some(group);
        }
        rows.push(PlanRow::Item(index));
    }
    rows
}

/// What one list slot draws, as owned data: the list's render callback is
/// `'static`, so the matched entries and their precomputed hints move into it
/// rather than borrow the frame.
enum ListItem {
    Header(u8),
    Item {
        entry: Entry,
        hint: Option<String>,
        selected: bool,
        index: usize,
    },
}

/// One of the palette's listeners, as the shell hears it.
pub(crate) type PaletteListener<A> = Rc<dyn Fn(&A, &mut Window, &mut App)>;

/// The palette's four keys, as the shell listens for them. The overlay is a
/// free function over the shell's data, so its listeners ride in together.
pub(crate) struct PaletteActions {
    pub(crate) select_prev: PaletteListener<PaletteSelectPrev>,
    pub(crate) select_next: PaletteListener<PaletteSelectNext>,
    pub(crate) run: PaletteListener<PaletteRun>,
    pub(crate) dismiss: PaletteListener<PaletteDismiss>,
}

/// The palette, laid over the shell while it is open. The query input holds
/// the keyboard, so the pane beside it is not typed into while a command is
/// being looked up; the four listeners are the palette's own actions (see
/// [`CONTEXT`]).
pub(crate) fn overlay(
    palette: &Palette,
    entries: &[Entry],
    keymap: &Keymap,
    dispatch: &Dispatch,
    actions: PaletteActions,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let matches = palette.matches(entries, cx);
    let rows = plan(&matches);
    let close = dispatch.clone();

    // The prototype's own numbers: the palette hangs at 11vh and is
    // min(680px, 92vw) wide with a 68vh cap, so a short window shrinks it
    // instead of clipping the rows.
    let viewport = window.viewport_size();
    let top = viewport.height * 0.11;
    let width = px(680.).min(viewport.width * 0.92);
    let max_height = viewport.height * 0.68;

    let input = palette.input().expect("an open palette has its input");
    let focus = input.read(cx).focus_handle(cx).clone();
    let list_state = palette.list().expect("an open palette has its rows");

    let mut items: Vec<ListItem> = Vec::with_capacity(rows.len());
    let mut drawn = 0usize;
    for row in &rows {
        match row {
            PlanRow::Header(group) => items.push(ListItem::Header(*group)),
            PlanRow::Item(index) => {
                let entry = matches[*index];
                items.push(ListItem::Item {
                    entry: entry.clone(),
                    hint: entry.hint(keymap),
                    selected: *index == palette.selected(),
                    index: drawn,
                });
                drawn += 1;
            }
        }
    }

    // The section headers and the rows the keyboard can reach. The list
    // scrolls what the dialog's height leaves room for, and the selection's
    // own moves take the scroll with them.
    let empty = items.is_empty();
    let list_theme = theme.clone();
    let list_dispatch = dispatch.clone();
    let list = list(list_state, move |index, _, _| match items.get(index) {
        Some(ListItem::Header(group)) => group_header(*group, &list_theme),
        Some(ListItem::Item {
            entry,
            hint,
            selected,
            index,
        }) => row(
            entry,
            hint.as_deref(),
            *selected,
            *index,
            &list_theme,
            &list_dispatch,
        ),
        None => div().into_any_element(),
    })
    .w_full()
    .flex_1()
    .min_h_0();

    let body: AnyElement = if empty {
        div()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(t("shell.palette.no_matches"))
            .into_any_element()
    } else {
        list.into_any_element()
    };

    let boxed = v_flex()
        .id("palette")
        .w(width)
        .max_w_full()
        .max_h(max_height)
        .overflow_hidden()
        .rounded(px(8.))
        .bg(theme.background)
        .text_color(theme.foreground)
        .border_1()
        .border_color(theme.border)
        .shadow(vec![
            BoxShadow::new(px(0.), px(18.), black().opacity(0.45)).blur_radius(px(48.)),
            BoxShadow::new(px(0.), px(2.), black().opacity(0.3)).blur_radius(px(8.)),
        ])
        .key_context(CONTEXT)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .track_focus(&focus)
        .on_action({
            let actions = actions.select_prev.clone();
            move |event: &PaletteSelectPrev, window, cx| actions(event, window, cx)
        })
        .on_action({
            let actions = actions.select_next.clone();
            move |event: &PaletteSelectNext, window, cx| actions(event, window, cx)
        })
        .on_action({
            let actions = actions.run.clone();
            move |event: &PaletteRun, window, cx| actions(event, window, cx)
        })
        .on_action({
            let actions = actions.dismiss.clone();
            move |event: &PaletteDismiss, window, cx| actions(event, window, cx)
        })
        .child(
            h_flex()
                .w_full()
                .px(px(12.))
                .py(px(10.))
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    Icon::default()
                        .data(super::icons::SEARCH)
                        .w(px(14.))
                        .h(px(14.))
                        .flex_none()
                        .text_color(theme.muted_foreground),
                )
                .child(
                    Input::new(&input)
                        .appearance(false)
                        .bordered(false)
                        .w_full(),
                )
                .child(
                    div()
                        .px_1()
                        .rounded_xs()
                        .border_1()
                        .border_color(theme.border)
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("Esc"),
                ),
        )
        .child(body)
        // The keyboard path, where a first-time look for it ends: the same
        // three keys the rows above answer to, and the one fact that keeps the
        // plugin section from reading like a second command system. It carries
        // the background so a row the height cap clips does not bleed through.
        .child(
            h_flex()
                .w_full()
                .px_2()
                .py_1()
                .gap_2()
                .items_center()
                .bg(theme.background)
                .border_t_1()
                .border_color(theme.border)
                .text_size(px(10.5))
                .text_color(theme.muted_foreground)
                .child(t("shell.palette.hints"))
                .child(div().flex_1())
                .child(t("shell.palette.source")),
        );
    h_flex()
        .id("palette-scrim")
        .absolute()
        .inset_0()
        .occlude()
        .justify_center()
        .items_start()
        .pt(top)
        // The dark around the dialog dismisses, on the press itself rather
        // than the click: a press inside the dialog never reaches this
        // listener, because the dialog stops it first.
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            close(ShellRequest::Command(Command::TogglePalette), window, cx);
        })
        .child(boxed)
        .into_any_element()
}

/// One section's name, where the rows change from one kind to the next.
fn group_header(group: u8, theme: &Theme) -> AnyElement {
    let key = match group {
        0 => "shell.palette.group_commands",
        1 => "shell.palette.group_workspaces",
        _ => "shell.palette.group_actions",
    };
    div()
        .px(px(8.))
        .pt(px(6.))
        .pb(px(3.))
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.muted_foreground)
        .child(t(key))
        .into_any_element()
}

/// One row: the label and the key that reaches it without the palette. The
/// row the selection sits on is an opaque step above the dialog.
fn row(
    entry: &Entry,
    hint: Option<&str>,
    selected: bool,
    index: usize,
    theme: &Theme,
    dispatch: &Dispatch,
) -> AnyElement {
    let request = entry.request();
    let run = dispatch.clone();
    let mut row = h_flex()
        .id(("palette-row", index))
        .w_full()
        .px(px(8.))
        .py(px(6.))
        .gap(px(9.))
        .rounded(px(5.))
        .items_center()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            run(request.clone(), window, cx);
        })
        // A plugin row wears its source badge, the same mark every other
        // contribution of that plugin carries: the section header already says
        // where the rows come from, the badge says which one.
        .children(match entry {
            Entry::Plugin(entry) => Some(crate::plugins::views::source_badge(
                theme,
                &entry.action.plugin_id,
            )),
            _ => None,
        })
        // One line per row, as the prototype's palette reads: the label owns
        // the row, the key or count at its right. The id/branch detail stays
        // searchable without being drawn twice under the thing it names.
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_size(px(12.5))
                .child(entry.label()),
        );
    if let Some(hint) = hint {
        row = row.child(
            div()
                .flex_none()
                .font_family(theme.mono_font_family.clone())
                .text_size(px(10.5))
                .text_color(theme.muted_foreground)
                .child(hint.to_owned()),
        );
    }
    if selected {
        // `list_active` is clamped to a 20% tint by the theme layer; the
        // prototype's selected row is an opaque step above the dialog, so the
        // secondary surface is the honest match here.
        row.bg(theme.secondary).into_any_element()
    } else {
        row.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    use crate::plugins::actions::Catalog;
    use crate::plugins::model::{Availability, Context, PluginAction};
    use crate::plugins::ui_manifest::UiContributions;

    fn action(plugin_id: &str, action_id: &str, title: &str) -> PluginAction {
        PluginAction {
            plugin_id: plugin_id.into(),
            action_id: action_id.into(),
            title: title.into(),
            description: None,
            contexts: Some(vec![Context::Workspace]),
        }
    }

    /// One catalog entry, as the shell builds it from what the daemon reports
    /// and what the plugin declared.
    fn announced(key: &str) -> Arc<Exposed> {
        let ui = crate::plugins::ui_manifest::parse(
            "example.board",
            std::path::Path::new("/tmp/board"),
            &format!(
                r#"
[[command]]
id = "apply"
title = "Apply project layout"
action = "apply"
key = "{key}"
"#
            ),
        );
        Catalog::build(&[action("example.board", "apply", "apply title")], &ui).entries()[0].clone()
    }

    fn entries() -> Vec<Entry> {
        vec![
            Entry::Command(Command::ToggleSettings),
            Entry::Plugin(announced("ctrl-shift-l")),
        ]
    }

    #[test]
    fn an_empty_query_shows_every_row() {
        assert_eq!(Palette::filtered("", &[], &entries()).len(), 2);
    }

    #[test]
    fn a_query_narrows_on_the_label_and_on_the_plugin_id() {
        assert_eq!(Palette::filtered("apply", &[], &entries()).len(), 1);
        assert_eq!(
            Palette::filtered("apply", &[], &entries())[0].label(),
            "Apply project layout"
        );

        // The plugin id matches too, so a user can search by the plugin.
        assert_eq!(Palette::filtered("board", &[], &entries()).len(), 1);
    }

    /// The query is matched against the whole entry: a label's case is the
    /// label's own business, and the trim is the shell's.
    #[test]
    fn a_query_ignores_case_and_edges() {
        assert_eq!(Palette::filtered("  APPLY ", &[], &entries()).len(), 1);
    }

    #[test]
    fn selection_stops_at_the_ends_instead_of_wrapping() {
        let mut palette = Palette::default();
        palette.step(1, 3);
        palette.step(1, 3);
        palette.step(1, 3);
        assert_eq!(palette.selected(), 2);
        for _ in 0..5 {
            palette.step(-1, 3);
        }
        assert_eq!(palette.selected(), 0);
        // An empty list has nothing to move across, and nothing to run.
        palette.step(1, 0);
        assert_eq!(palette.selected(), 0);
        assert!(palette.selected_entry(&[]).is_none());
    }

    /// The rows change under the selection: a query narrowed, a snapshot
    /// pushed. The selection follows the rows, not the other way round.
    #[test]
    fn the_selection_follows_a_shrinking_row_set() {
        let mut palette = Palette::default();
        palette.step(1, 3);
        palette.sync_rows(2);
        assert_eq!(palette.selected(), 1);
        palette.sync_rows(0);
        assert_eq!(palette.selected(), 0);
    }

    /// A run puts its row in front of its section — and never out of it: a
    /// recency rank reorders inside a section, the sections keep their order.
    #[test]
    fn a_run_ranks_its_row_first_inside_the_section() {
        let mut palette = Palette::default();
        let second = Entry::Command(Command::MoveTabNext);
        palette.note_run(&second);
        let mut entries = entries();
        entries.insert(0, second);
        let matches = Palette::filtered("", &palette.recent, &entries);
        assert_eq!(matches[0].detail(), "tab.move-right");
        // Sections stay contiguous: a header goes where the section changes,
        // and the plugin section stays behind the commands'.
        let rows = plan(&matches);
        assert!(matches!(rows[0], PlanRow::Header(0)));
        assert!(matches!(rows[1], PlanRow::Item(0)));
    }

    /// The ring is a ring: the run after the last forgets the first.
    #[test]
    fn the_recency_ring_forgets_its_oldest() {
        let mut palette = Palette::default();
        let entries: Vec<Entry> = COMMANDS
            .iter()
            .take(RECENT_LEN + 2)
            .map(|info| Entry::Command(info.command))
            .collect();
        let keys: Vec<String> = entries.iter().map(|entry| entry.key()).collect();
        for entry in &entries {
            palette.note_run(entry);
        }
        assert_eq!(palette.recent.len(), RECENT_LEN);
        // Most recent first: the last run leads, and the first two runs fell
        // off the end.
        assert_eq!(palette.recent.first(), keys.last());
        assert!(!palette.recent.contains(&keys[0]));
        assert!(!palette.recent.contains(&keys[1]));
    }

    /// The plan is the rows the list draws: a header wherever the section
    /// changes, an item for every match.
    #[test]
    fn the_plan_puts_a_header_where_the_section_changes() {
        let entries = entries();
        let matches = Palette::filtered("", &[], &entries);
        let rows = plan(&matches);
        assert_eq!(
            rows,
            vec![
                PlanRow::Header(0),
                PlanRow::Item(0),
                PlanRow::Header(2),
                PlanRow::Item(1)
            ]
        );
    }

    /// A row runs what it says it runs: the same request a sidebar click on
    /// the same subject would send.
    #[test]
    fn a_command_row_runs_its_command_and_a_plugin_row_runs_its_action() {
        match Entry::Command(Command::ToggleSettings).request() {
            ShellRequest::Command(command) => assert_eq!(command, Command::ToggleSettings),
            _ => panic!("a command row runs a command"),
        }
        match Entry::Plugin(announced("ctrl-shift-l")).request() {
            ShellRequest::PluginAction { action, source } => {
                assert_eq!(action.action_id, "apply");
                assert_eq!(source, "palette");
            }
            _ => panic!("a plugin row runs a plugin action"),
        }
    }

    /// A workspace row focuses its workspace, is found by branch as well as by
    /// name, and sits in its own section between the commands and the plugins.
    #[test]
    fn a_workspace_row_focuses_its_workspace_and_names_its_branch() {
        let row = Entry::Workspace {
            workspace_id: "w2".into(),
            label: "BitFun".into(),
            branch: Some("feat/x".into()),
            tabs: 2,
        };
        assert_eq!(row.label(), "BitFun — feat/x");
        assert_eq!(row.detail(), "feat/x");
        assert_eq!(row.group(), 1);
        assert_eq!(Entry::Command(Command::ToggleSettings).group(), 0);
        match row.request() {
            ShellRequest::FocusWorkspace(id) => assert_eq!(id, "w2"),
            _ => panic!("a workspace row focuses a workspace"),
        }
        // No chord reaches a workspace: its right-hand slot is the tab count.
        assert_eq!(
            row.hint(&Keymap::default()),
            Some(rust_i18n::t!("shell.palette.tab_count", count = 2).to_string())
        );

        let entries = vec![Entry::Command(Command::ToggleSettings), row];
        let matches = Palette::filtered("feat", &[], &entries);
        assert_eq!(matches.len(), 1, "the query reaches the branch");
        assert_eq!(matches[0].label(), "BitFun — feat/x");
    }

    #[test]
    fn a_command_row_shows_the_key_that_reaches_it_and_a_declared_plugin_row_its_own() {
        let keymap = Keymap::default();
        assert_eq!(
            Entry::Command(Command::ToggleSettings)
                .hint(&keymap)
                .as_deref(),
            Some("cmd-,")
        );
        // A prefixed chord is shown with the prefix it follows.
        assert_eq!(
            Entry::Command(Command::NewTab).hint(&keymap).as_deref(),
            Some("cmd-t")
        );
        assert_eq!(
            Entry::Command(Command::MoveTabNext)
                .hint(&keymap)
                .as_deref(),
            Some("ctrl-b .")
        );
        // A plugin's chord with a modifier of its own is shown alone …
        assert_eq!(
            Entry::Plugin(announced("ctrl-shift-l"))
                .hint(&keymap)
                .as_deref(),
            Some("ctrl-shift-l")
        );
        // … and a bare one with the prefix, because that is how it is pressed.
        assert_eq!(
            Entry::Plugin(announced("shift-y")).hint(&keymap).as_deref(),
            Some("ctrl-b shift-y")
        );
        // An action nothing declared a chord for shows none.
        let undeclared = Catalog::build(&[action("a.b", "x", "X")], &UiContributions::default());
        assert!(
            Entry::Plugin(undeclared.entries()[0].clone())
                .hint(&keymap)
                .is_none()
        );
    }

    /// §M11d's exit condition, seen from the palette: one declaration is the
    /// row, the chord and the tool, and unlinking the plugin leaves none of the
    /// three.
    #[test]
    fn one_declaration_reaches_the_row_the_chord_and_the_tool() {
        let ui = crate::plugins::ui_manifest::parse(
            "example.board",
            std::path::Path::new("/tmp/board"),
            r#"
[[command]]
id = "apply"
title = "Apply project layout"
action = "apply"
key = "ctrl-shift-l"
"#,
        );
        let actions = [action("example.board", "apply", "apply title")];
        let catalog = Catalog::build(&actions, &ui);
        let entry = catalog.entries()[0].clone();

        let row = Entry::Plugin(entry.clone());
        assert_eq!(row.label(), "Apply project layout");

        let mut keymap = Keymap::default();
        keymap.set_plugins(&catalog);
        assert_eq!(row.hint(&keymap).as_deref(), Some("ctrl-shift-l"));
        assert_eq!(
            keymap.claimed(&Keystroke::parse("ctrl-shift-l").expect("the chord parses")),
            Some(crate::shell::keymap::Chord::Plugin(entry.action.clone()))
        );

        assert!(
            catalog.named(entry.tool_name()).is_some(),
            "the tool an agent calls is the row's own action"
        );

        // Unlinked: the daemon stops reporting the action, which is all it takes.
        let gone = Catalog::build(&[], &ui);
        assert!(gone.offered(Availability::of(None)).next().is_none());
        assert!(gone.named(entry.tool_name()).is_none());
    }

    /// A declaration names the row: the daemon's action title is a description
    /// of the action, while the declaration is what the plugin wants its own
    /// command to be called here.
    #[test]
    fn a_declared_title_names_the_row_and_the_daemons_names_it_alone() {
        assert_eq!(
            Entry::Plugin(announced("ctrl-shift-l")).label(),
            "Apply project layout"
        );
        let undeclared = Catalog::build(
            &[action("a.b", "x", "Action title")],
            &UiContributions::default(),
        );
        assert_eq!(
            Entry::Plugin(undeclared.entries()[0].clone()).label(),
            "Action title"
        );
    }
}
