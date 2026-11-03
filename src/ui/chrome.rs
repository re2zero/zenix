//! The window's chrome, composed from the component library's shell pieces.
//!
//! U2-1 draws the frame the rest of the rebuild hangs on: the identity strip
//! (brand, focused workspace, the terminal's reported title, the window's own
//! controls), the activity rail, the sidebar's slot, the focused workspace's
//! tab strip and the status bar. Everything is data from the daemon's pushed
//! snapshot — nothing here polls, and nothing here is drawn from a list the
//! client keeps beside it.
//!
//! What this task leaves to its successors, by PLAN.md's U2 split:
//!
//! - the sidebar's rows, head counts and breakpoints were U2-2; this file
//!   composes the column and its head/foot around [`super::sidebar`]'s body.
//! - the panes, the painter, splits and focus were U2-3: the main area mounts
//!   `crate::terminal::view::TerminalView` under the tab strip (see
//!   [`main_area`]).
//! - the command palette, chords and the strip's search trigger are U2-4; the
//!   middle of the identity strip carries the focused terminal's own title
//!   until that trigger exists, rather than an inert button.
//! - settings and the aside's tabs are U3. The rail's footer entries arrive
//!   with the surface they open, so no control here is a dead end. U3-1 gave
//!   the rail its gear; U3-2 gives the strip the conversation panel's switch,
//!   beside the window's own controls.
//!
//! ## Client-side decorations, with a server-side fallback
//!
//! The window asks for client-side decorations (`WindowDecorations::Client` in
//! `crate::run`). On X11 a compositor must advertise GTK frame extents for that
//! to be honored; where it cannot, gpui reports `Decorations::Server` and the
//! window manager draws its own title bar complete with min/max/close. Drawing
//! ours as well would stack two sets of controls, so the strip keeps its
//! identity content and drops the window controls. [`ResizeBorder`] makes the
//! same call: a client-decorated window gets the resize margins, hairline and
//! shadow; a server-decorated one is a plain box.
//!
//! Windows and macOS report `Decorations::Server` from this gpui snapshot
//! regardless of what was asked (the option is an X11/Wayland one), so the
//! controls follow the platform instead: AppKit draws the traffic lights on
//! macOS (the strip leaves their room), and Windows' caption buttons are ours
//! because the native title bar is hidden.

use ely_core::{
    buttons::{ButtonVariant, IconButton},
    primitives::{Icon, IconName},
    shell::{ActivityBar, ActivityItem, StatusBar, StatusBarItem, WindowControls, drag_region},
    theme::{ActiveTheme, ControlSize, IconSize, Palette, Platform, Radius, TextSize},
    typography::{Ellipsis, KbdCombo},
};
use gpui::{
    AnyElement, App, Decorations, Entity, FontWeight, Hsla, IntoElement, ParentElement, Role,
    SharedString, Styled, Window, div, prelude::*, px,
};
use herdr_client::Method;
use herdr_client::protocol::{AgentStatus, ClientShellTab, ClientShellWorkspace};
use serde_json::json;

use super::commands::Command;
use super::keymap::Keymap;
use super::shell::Root;
use super::sidebar;
use crate::config;
use crate::herdr::{Connection, ConnectionStatus, LiveState};
use crate::i18n::t;

/// The sidebar's column, decided with the rest of the layout (PLAN §3, 05).
const SIDEBAR_WIDTH: f32 = 260.;
/// The strip's own height: a 28px tab with 7px of air above and below (the
/// design's 42px), rather than the control token's bare 28.
const TAB_STRIP_HEIGHT: f32 = 42.;
const SIDEBAR_FOOT_HEIGHT: f32 = 24.;
/// The rail's settings destination: a footer item, not one of the five views,
/// so pressing it opens the dialog instead of switching the sidebar.
pub(crate) const SETTINGS_ITEM: &str = "settings";

/// The sidebar views, in the rail's order. The ids are the locale keys'
/// stems (`shell.view.*`, `shell.foot.*`) and the rail's element ids. The
/// event ring is not one of them: it is debug surface, drawn by
/// [`super::events_panel`] over the status bar's bug icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
    Workspaces,
    Agents,
    Fleet,
    Plugins,
}

impl View {
    /// The rail's buttons, top to bottom.
    pub(crate) const ALL: [Self; 4] = [Self::Workspaces, Self::Agents, Self::Fleet, Self::Plugins];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Workspaces => "workspaces",
            Self::Agents => "agents",
            Self::Fleet => "fleet",
            Self::Plugins => "plugins",
        }
    }

    /// The bundled Lucide glyph the rail draws.
    fn icon(self) -> IconName {
        match self {
            Self::Workspaces => IconName::LayoutGrid,
            Self::Agents => IconName::Users,
            Self::Fleet => IconName::Gauge,
            Self::Plugins => IconName::Puzzle,
        }
    }

    /// What the view is called, from `shell.view` in the locale file.
    fn label_key(self) -> &'static str {
        match self {
            Self::Workspaces => "shell.view.workspaces",
            Self::Agents => "shell.view.agents",
            Self::Fleet => "shell.view.fleet",
            Self::Plugins => "shell.view.plugins",
        }
    }

    /// Where the view's rows come from, from `shell.foot`.
    fn source_key(self) -> &'static str {
        match self {
            Self::Workspaces => "shell.foot.workspaces",
            Self::Agents => "shell.foot.agents",
            Self::Fleet => "shell.foot.fleet",
            Self::Plugins => "shell.foot.plugins",
        }
    }

    pub(crate) fn label(self) -> String {
        t(self.label_key())
    }

    fn source(self, data: &sidebar::Data) -> String {
        // The lane views of a remote window have no source to name: their
        // socket is on the other machine, and the foot says that rather than
        // implying a quiet one.
        if !data.lane && matches!(self, Self::Fleet | Self::Plugins) {
            return t("shell.foot.remote").to_string();
        }
        t(self.source_key()).to_string()
    }

    /// The view an item id names, for the rail's callback.
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.id() == id)
    }

    /// Whether this view's source is live right now: the dot must not claim a
    /// stream that is down, nor hide a snapshot-less column behind green. A
    /// kept snapshot beside a dropped connection is the last picture, not a
    /// live one.
    fn source_is_live(self, data: &sidebar::Data) -> bool {
        if !data.lane && self != Self::Agents {
            return false;
        }
        let snapshot = data.live.snapshot.is_some();
        let connected = data.live.status != ConnectionStatus::Disconnected;
        match self {
            Self::Workspaces | Self::Agents => snapshot && connected,
            Self::Fleet => snapshot && data.fleet.error.is_none(),
            Self::Plugins => data.plugins.error.is_none(),
        }
    }
}

/// The strip across the top: who this window is, which workspace is in front,
/// what the focused terminal calls itself, and — when the window draws its own
/// frame — the window controls.
pub(crate) fn title_bar(
    window: &mut Window,
    live: &LiveState,
    keymap: &Keymap,
    target_label: Option<String>,
    root: &Entity<Root>,
    cx: &mut App,
) -> AnyElement {
    // The drag region is claimed before the theme is borrowed: it takes `cx`
    // mutably, the content below only reads it.
    let bar = drag_region("shell-titlebar", window, cx);
    let decorations = window.window_decorations();
    let theme = cx.theme();
    let colors = &theme.colors;
    let mac = theme.platform == Platform::Mac;
    // See the module docs: the platform decides whose buttons these are, and
    // on Linux the window's actual decorations decide whether ours belong.
    let client = matches!(decorations, Decorations::Client { .. });
    let controls = !mac && (cfg!(target_os = "windows") || client);

    let brand = div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius(Radius::Md))
                .bg(colors.accent)
                .text_color(colors.on_accent)
                .text_size(theme.text_size(TextSize::Xs))
                .font_weight(FontWeight::BOLD)
                .child("z"),
        )
        .child(
            div()
                .text_size(theme.text_size(TextSize::Sm))
                .font_weight(FontWeight::SEMIBOLD)
                .child("zenix"),
        );

    // A window whose daemon is not the default local one names it beside the
    // brand: the one on-screen mark of which connection face this is. The
    // label arrives as a value — the shell is mid-render and cannot be read.
    let target_badge = target_label.map(|label| {
        div()
            .flex_none()
            .px_1()
            .py(px(2.))
            .rounded(theme.radius(Radius::Sm))
            .bg(colors.surface)
            .border_1()
            .border_color(colors.border)
            .text_size(theme.text_size(TextSize::Xs))
            .text_color(colors.fg_muted)
            .child(label)
    });
    let pill = focused_workspace(live).map(|workspace| workspace_pill(workspace, live, root, cx));
    let search = search_trigger(keymap, live, root, cx);

    let mut right = div().flex().flex_none().items_center().gap_2().occlude();
    // The panel's switch sits beside the window's own controls: one press
    // opens the conversation or closes it, and the config remembers which.
    let aside_open = config::get(cx).layout.aside_visible;
    let toggle = root.clone();
    let mut aside_button = IconButton::new("titlebar-aside", IconName::MessageSquare)
        .size(ControlSize::Sm)
        .tooltip(t("command.view.toggle-chat"))
        .on_click(move |_, window, cx| {
            toggle.update(cx, |root, cx| root.toggle_aside(window, cx));
        });
    if aside_open {
        aside_button = aside_button.variant(ButtonVariant::Secondary);
    }
    right = right.child(aside_button);
    if controls {
        right = right.child(WindowControls::new("window-controls"));
    }

    let bar = bar
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .h(theme.titlebar_height())
        .bg(colors.bg)
        .border_b_1()
        .border_color(colors.border)
        .pr_1();
    // AppKit's traffic lights sit over the content when the native title bar
    // is hidden; the strip starts after them.
    let bar = if mac {
        bar.pl(theme.traffic_light_inset())
    } else {
        bar.pl_3()
    };
    bar.child(
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .occlude()
            .child(brand)
            .children(target_badge)
            .children(pill),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .justify_center()
            .child(search),
    )
    .child(right)
    .into_any_element()
}

/// The strip's middle: the search trigger that opens the palette. Its key cap
/// is the chord the keymap actually claims, not a hardcoded one. While there is
/// no snapshot the placeholder says the daemon is not connected, so the field
/// never promises a search the session cannot do (U3-5).
fn search_trigger(keymap: &Keymap, live: &LiveState, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let chord = keymap.label(Command::TogglePalette);
    let hint = if live.snapshot.is_none() {
        t("shell.titlebar.search_waiting")
    } else {
        t("shell.titlebar.search")
    };
    let open = root.clone();
    div()
        .id("palette-trigger")
        .role(Role::Button)
        .aria_label(t("command.view.palette"))
        .w_full()
        .max_w(px(420.))
        .h(px(24.))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(theme.radius(Radius::Md))
        .border_1()
        .border_color(colors.border)
        .bg(colors.bg)
        .text_color(colors.fg_muted)
        .cursor_pointer()
        .hover(move |style| style.border_color(colors.border_strong))
        .on_click(move |_, _, cx| {
            open.update(cx, |root, cx| root.toggle_palette(cx));
        })
        .child(
            Icon::new(IconName::Search)
                .size(IconSize::Xs)
                .color(colors.fg_subtle),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(Ellipsis::new(SharedString::from(hint))),
        )
        .children(chord.map(|chord| KbdCombo::new(&chord)))
        .into_any_element()
}

/// The workspace in front: its name, the branch it is on, and the connection
/// the snapshot comes through. A press asks the rail for the workspaces view,
/// which is where a workspace is picked.
fn workspace_pill(
    workspace: &ClientShellWorkspace,
    live: &LiveState,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let (hover, dot) = (colors.hover, connection_color(live.status, colors));
    let root = root.clone();
    div()
        .id("workspace-pill")
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .h(px(26.))
        .px_2()
        .rounded(theme.radius(Radius::Md))
        .border_1()
        .border_color(colors.border)
        .bg(colors.surface)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .on_click(move |_, _, cx| {
            root.update(cx, |root, cx| root.show_view(View::Workspaces, cx));
        })
        .child(div().size(px(6.)).rounded_full().bg(dot))
        .child(
            div()
                .text_size(theme.text_size(TextSize::Sm))
                .text_color(colors.fg)
                .child(Ellipsis::new(SharedString::from(workspace.label.clone()))),
        )
        .children(workspace.branch.as_ref().map(|branch| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .pl_2()
                .border_l_1()
                .border_color(colors.border)
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(Icon::new(IconName::GitBranch).size(IconSize::Xs))
                .child(SharedString::from(branch.clone()))
        }))
        .into_any_element()
}

/// The activity rail: one destination per sidebar view, the agents' away-from-
/// idle count as its badge, and the settings gear at its foot (the design's
/// rail ends in it). The rail's own item look (selected fill, accent edge,
/// hover) is the component's. The event ring is not offered here — it is the
/// status bar's bug icon (`debug.events`), so debug never wears a navigation
/// slot.
pub(crate) fn rail(view: View, live: &LiveState, root: &Entity<Root>) -> AnyElement {
    let mut bar = ActivityBar::new("shell-rail").selected(view.id());
    for item in View::ALL {
        let mut entry = ActivityItem::new(item.id(), item.icon(), item.label());
        if item == View::Agents
            && let Some(count) = active_agents(live)
        {
            entry = entry.badge(count);
        }
        bar = bar.item(entry);
    }
    bar = bar.footer(ActivityItem::new(
        SETTINGS_ITEM,
        IconName::Settings,
        t("settings.title"),
    ));
    let root = root.clone();
    bar.on_select(move |id, _, cx| {
        // The gear is a footer destination, not one of the five views: it
        // opens the settings dialog and leaves the rail's selection alone.
        if id.as_ref() == SETTINGS_ITEM {
            root.update(cx, |root, cx| root.toggle_settings(cx));
            return;
        }
        let Some(view) = View::from_id(id) else {
            return;
        };
        root.update(cx, |root, cx| root.show_view(view, cx));
    })
    .into_any_element()
}

/// The sidebar's column: the view's head with its live count, the rows of the
/// selected view, and the foot naming where those rows come from. The body is
/// [`super::sidebar`]'s; the column, its border and the foot stay here so the
/// five views share one frame.
pub(crate) fn sidebar(
    view: View,
    data: &sidebar::Data,
    connection: &Connection,
    root: &Entity<Root>,
    cx: &mut App,
) -> AnyElement {
    // The rows are built before the theme is borrowed: drawing a view takes
    // `cx` mutably (its presses capture entities), and the frame below only
    // reads it.
    let head = sidebar::head(view, data, cx);
    let body = sidebar::body(view, data, connection, root, cx);
    let theme = cx.theme();
    let colors = &theme.colors;
    // A snapshot-backed view with no snapshot is not green and does not name
    // `snapshot`: its foot says what it is waiting on, in the connection's own
    // tone. A kept snapshot beside a dropped connection says it is the last
    // picture, not that the source is live (U3-5).
    let snapshot_view = matches!(view, View::Workspaces | View::Agents);
    let waiting = data.live.snapshot.is_none() && snapshot_view;
    let stale = snapshot_view
        && data.live.snapshot.is_some()
        && data.live.status == ConnectionStatus::Disconnected;
    let live = !waiting && !stale && view.source_is_live(data);
    let tone = if stale {
        colors.danger
    } else if waiting {
        if data.live.status == ConnectionStatus::Disconnected {
            colors.danger
        } else {
            colors.warning
        }
    } else if live {
        colors.success
    } else {
        colors.fg_subtle
    };
    let source = if waiting {
        t("shell.foot.waiting")
    } else if stale {
        t("shell.foot.stale")
    } else {
        view.source(data)
    };
    div()
        .w(px(SIDEBAR_WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(colors.bg)
        .border_r_1()
        .border_color(colors.border)
        .child(head)
        .child(div().flex_1().min_h_0().child(body))
        .child(
            div()
                .h(px(SIDEBAR_FOOT_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .gap_1p5()
                .px_3()
                .border_t_1()
                .border_color(colors.border)
                .child(div().size(px(5.)).rounded_full().bg(tone))
                .child(
                    div()
                        .min_w(px(0.))
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(Ellipsis::new(SharedString::from(source))),
                ),
        )
        .into_any_element()
}

/// What the main column reads for one paint, bundled because it is one
/// composition: the shell's state, the two surfaces it mounts and the bits of
/// chrome state beside them.
pub(crate) struct Main<'a> {
    pub(crate) live: &'a LiveState,
    pub(crate) connection: &'a Connection,
    pub(crate) panes: &'a Entity<crate::terminal::view::TerminalView>,
    pub(crate) keymap: &'a Keymap,
    pub(crate) prefix_armed: bool,
    pub(crate) empty: &'a super::empty::State,
    /// Whether this window's daemon is the default local one, so the empty
    /// screen may offer to start, copy or tail it. A named session or socket
    /// window and a remote window draw the retry note instead: none of them
    /// owns the default daemon this card would touch.
    pub(crate) local_daemon: bool,
    /// The event log's inputs: the shell's copy of the ring, the plugin
    /// registry the chips' hooked question asks, and the event the inspector
    /// has captured. Drawn only while the reader expanded the panel.
    pub(crate) events: &'a crate::events::EventState,
    pub(crate) plugins: &'a crate::plugins::host::PluginState,
    pub(crate) selected: Option<&'a crate::events::Event>,
    pub(crate) debug_log_open: bool,
    pub(crate) root: &'a Entity<Root>,
}

/// The main region: the focused workspace's tabs above the pane grid, with the
/// status bar at its foot. The bar belongs to this column, not to the window:
/// the design keeps the rail and the sidebar running to the floor, and the bar
/// states the session the main area shows (see `docs/ui/redesign/ui.js`'s
/// `shell`).
///
/// With no workspace to show — a first run, or a daemon with nothing open —
/// the strip is not drawn and the first-run block sits over the grid (U3-5).
/// The grid stays mounted underneath, so its focus, theme and connection
/// survive the first workspace arriving.
pub(crate) fn main_area(main: Main<'_>, cx: &App) -> AnyElement {
    let Main {
        live,
        connection,
        panes,
        keymap,
        prefix_armed,
        empty,
        local_daemon,
        events,
        plugins,
        selected,
        debug_log_open,
        root,
    } = main;
    let first_run = live
        .snapshot
        .as_deref()
        .is_none_or(|snapshot| snapshot.workspaces.is_empty());
    let grid = div()
        .flex_1()
        .min_h_0()
        .relative()
        // The grid takes the room between the strip and the bar. A bare
        // `flex_1` box would give a childless canvas one row; the entity fills
        // it, and the view paints its own ground before the cells.
        .child(panes.clone())
        .children(first_run.then(|| super::empty::main(live, empty, local_daemon, root, cx)));
    let mut column = div().flex_1().min_w_0().min_h_0().flex().flex_col();
    if !first_run {
        column = column.child(tab_strip(live, connection, cx));
    }
    column = column.child(grid);
    // The debug log is the column's own foot when the reader expanded it: the
    // bar stays the window's last line, the log sits directly above it.
    if debug_log_open {
        column = column.child(super::events_panel::panel(
            events, plugins, selected, root, cx,
        ));
    }
    column
        .child(status_bar(
            live,
            keymap,
            prefix_armed,
            debug_log_open,
            root,
            cx,
        ))
        .into_any_element()
}

/// The focused workspace's tabs, then the button that opens another. Selecting
/// a tab asks the daemon to focus it; the new focus comes back as a snapshot,
/// as every focus does. Dropping a tab on another inserts it into the gap
/// before that tab — the daemon's `insert_index` counts the moving row still
/// in place, which is exactly that gap.
fn tab_strip(live: &LiveState, connection: &Connection, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let snapshot = live.snapshot.as_deref();
    let tabs: Vec<&ClientShellTab> = focused_tabs(live);

    let mut bar = ely_core::shell::TabBar::new("shell-tabs");
    for tab in &tabs {
        bar = bar.tab(
            ely_core::shell::WindowTab::new(tab.tab_id.clone(), tab.label.clone())
                .icon(IconName::Terminal),
        );
    }
    if let Some(tab_id) = snapshot.and_then(|snapshot| snapshot.focused_tab_id.clone()) {
        bar = bar.selected(SharedString::from(tab_id));
    }
    let select = connection.clone();
    bar = bar.on_select(move |id, _, _| select.focus_tab(id));
    let close = connection.clone();
    bar = bar.on_close(move |id, _, _| {
        close.request(Method::TabClose, json!({ "tab_id": id.to_string() }));
    });
    let ids: Vec<String> = tabs.iter().map(|tab| tab.tab_id.clone()).collect();
    let reorder = connection.clone();
    bar = bar.on_reorder(move |from, onto, _, _| {
        let (Some(tab_id), Some(index)) = (ids.get(from), insert_gap(from, onto)) else {
            return;
        };
        reorder.request(
            Method::TabMove,
            json!({ "tab_id": tab_id, "insert_index": index }),
        );
    });

    let mut new_tab = IconButton::new("tab-new", IconName::Plus)
        .size(ControlSize::Sm)
        .tooltip(t("command.tab.new"));
    if let Some(workspace) = focused_workspace(live) {
        let workspace_id = workspace.workspace_id.clone();
        let add = connection.clone();
        new_tab = new_tab.on_click(move |_, _, _| {
            add.request(
                Method::TabCreate,
                json!({ "workspace_id": workspace_id, "focus": true }),
            );
        });
    } else {
        // A tab needs a workspace to open in; the button says so by being
        // unavailable rather than by failing a press.
        new_tab = new_tab.disabled(true);
    }

    // The new-tab press sits right after the last tab — that is where a
    // reader reaches for it — and the bar yields width when the tabs run
    // out of room, so the press never falls off the strip's edge.
    div()
        .h(px(TAB_STRIP_HEIGHT))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .px_1p5()
        .border_b_1()
        .border_color(colors.border)
        .child(div().flex_shrink_1().min_w_0().overflow_hidden().child(bar))
        .child(new_tab)
        .into_any_element()
}

/// The bar along the foot: the connection, what the session holds, the focused
/// branch, the config's problems when it has any, the prefix key, and the
/// stream the picture came from. The prefix is always named; while it is armed
/// the same segment lights, so a chord's first key is visible as it waits for
/// the second. The bar's last segment is the debug surface's only entry: with
/// `debug.events` on, a bug icon whose press expands the event log above the
/// bar (lit while it is open).
pub(crate) fn status_bar(
    live: &LiveState,
    keymap: &Keymap,
    prefix_armed: bool,
    debug_open: bool,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut bar = StatusBar::new();

    let mut connection = StatusBarItem::new("status-connection")
        .leading(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(connection_color(live.status, colors)),
        )
        .label(connection_label(live));
    if live.status == ConnectionStatus::Disconnected
        && let Some(reason) = live.error.clone()
    {
        connection =
            connection.tooltip(rust_i18n::t!("status.disconnected", reason = reason).to_string());
    }
    bar = bar.left(connection);

    // With nothing open the bar says so beside the connection: an empty main
    // column is a fact about the session, not a broken view. While the
    // handshake is still under way the bar has nothing to say about workspaces
    // and keeps quiet rather than guessing (U3-5).
    let no_workspace = match live.snapshot.as_deref() {
        Some(snapshot) => snapshot.workspaces.is_empty(),
        None => live.status == ConnectionStatus::Disconnected,
    };
    if no_workspace {
        bar = bar.left(StatusBarItem::new("status-no-workspace").label(t("status.no_workspace")));
    }

    if let Some(counts) = counts(live) {
        bar = bar.left(StatusBarItem::new("status-counts").label(counts));
    }

    if let Some(branch) = focused_workspace(live).and_then(|workspace| workspace.branch.clone()) {
        // The branch is also the way into the working tree it names: a press
        // opens the review sheet on the same repository (U7).
        let open = root.clone();
        bar = bar.right(
            StatusBarItem::new("status-branch")
                .icon(IconName::GitBranch)
                .label(branch)
                .tooltip(t("command.view.review"))
                .on_click(move |_, _, cx| {
                    open.update(cx, |root, cx| root.open_review(cx));
                }),
        );
    }

    let diagnostics = crate::config::diagnostics(cx);
    if !diagnostics.is_empty() {
        let color = if diagnostics.iter().any(|item| item.is_blocking()) {
            colors.danger
        } else {
            colors.warning
        };
        bar = bar.right(
            StatusBarItem::new("status-diagnostics")
                .leading(
                    Icon::new(IconName::TriangleAlert)
                        .size(IconSize::Xs)
                        .color(color),
                )
                .label(diagnostics.len().to_string())
                .tooltip(
                    rust_i18n::t!("settings.config.problems_short", count = diagnostics.len())
                        .to_string(),
                ),
        );
    }

    // A dropped daemon is its own segment, so the bar states it even when the
    // connection label is ellipsized by a narrow window.
    if live.status == ConnectionStatus::Disconnected {
        let mut daemon = StatusBarItem::new("status-daemon")
            .leading(
                Icon::new(IconName::TriangleAlert)
                    .size(IconSize::Xs)
                    .color(colors.warning),
            )
            .label(t("status.daemon_stopped"));
        if let Some(reason) = live.error.clone() {
            daemon =
                daemon.tooltip(rust_i18n::t!("status.disconnected", reason = reason).to_string());
        }
        bar = bar.right(daemon);
    }

    // The source segment names what the picture came from. Before the first
    // snapshot there is no picture to name, and the segment says what it is
    // waiting on instead of `snapshot`.
    let source = if live.snapshot.is_none() {
        t("status.source.waiting")
    } else {
        t("status.source.snapshot")
    };
    bar = bar.right(StatusBarItem::new("status-source").label(source));
    let prefix = StatusBarItem::new("status-prefix")
        .leading(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_1()
                .rounded(theme.radius(Radius::Sm))
                .text_size(theme.text_size(TextSize::Xs))
                .when(prefix_armed, |segment| {
                    segment.bg(colors.accent).text_color(colors.on_accent)
                })
                .when(!prefix_armed, |segment| segment.text_color(colors.fg_muted))
                .child(t("status.prefix"))
                .child(keymap.prefix_label()),
        )
        .tooltip(t("settings.keys.prefix"));
    bar = bar.right(prefix);
    // The debug entry sits at the bar's very end — the bottom-right corner —
    // and only exists while its flag says the reader wants debug surfaces at
    // all. The stream it opens runs either way; this only decides whether the
    // log is offered.
    if config::get(cx).debug.events {
        let toggle = root.clone();
        bar = bar.right(
            StatusBarItem::new("status-debug")
                .leading(
                    Icon::new(IconName::Bug)
                        .size(IconSize::Xs)
                        .color(if debug_open {
                            colors.accent
                        } else {
                            colors.fg_muted
                        }),
                )
                .tooltip(t("shell.debug_log.toggle"))
                .on_click(move |_, _, cx| {
                    toggle.update(cx, |root, cx| root.toggle_debug_log(cx));
                }),
        );
    }
    bar.into_any_element()
}

/// The focused workspace, as the daemon names it.
fn focused_workspace(live: &LiveState) -> Option<&ClientShellWorkspace> {
    let snapshot = live.snapshot.as_deref()?;
    let id = snapshot.focused_workspace_id.as_deref()?;
    snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == id)
}

/// The focused workspace's tabs, in the daemon's order.
fn focused_tabs(live: &LiveState) -> Vec<&ClientShellTab> {
    let Some(snapshot) = live.snapshot.as_deref() else {
        return Vec::new();
    };
    let Some(workspace) = focused_workspace(live) else {
        return Vec::new();
    };
    snapshot
        .tabs
        .iter()
        .filter(|tab| tab.workspace_id == workspace.workspace_id)
        .collect()
}

/// How many agents are away from idle: what the rail's agents badge counts.
fn active_agents(live: &LiveState) -> Option<u32> {
    let snapshot = live.snapshot.as_deref()?;
    let count = snapshot
        .agents
        .iter()
        .filter(|agent| {
            matches!(
                agent.agent_status,
                AgentStatus::Working | AgentStatus::Blocked
            )
        })
        .count();
    u32::try_from(count).ok().filter(|count| *count > 0)
}

/// What the session holds, from the snapshot's own lists, so the bar cannot
/// disagree with the sidebar about how many workspaces there are.
fn counts(live: &LiveState) -> Option<String> {
    let snapshot = live.snapshot.as_deref()?;
    Some(format!(
        "{} · {} · {}",
        rust_i18n::t!("status.count.ws", count = snapshot.workspaces.len()),
        counted(
            "status.count.tab_one",
            "status.count.tab_many",
            snapshot.tabs.len()
        ),
        counted(
            "status.count.pane_one",
            "status.count.pane_many",
            snapshot.panes.len()
        ),
    ))
}

/// A count in its own units. rust-i18n has no plural forms, so the singular is
/// a key of its own.
fn counted(one: &str, many: &str, count: usize) -> String {
    rust_i18n::t!(if count == 1 { one } else { many }, count = count).to_string()
}

/// The connection's word, as the status bar says it. A drop's reason is the
/// item's tooltip, since it is often too long for the bar.
fn connection_label(live: &LiveState) -> String {
    match live.status {
        ConnectionStatus::Connecting => t("status.connecting"),
        ConnectionStatus::AwaitingSnapshot => t("status.awaiting_snapshot"),
        ConnectionStatus::Connected => t("status.connected"),
        ConnectionStatus::Disconnected => t("status.no_reason"),
    }
}

/// The connection's dot: green once a snapshot has landed, amber while the
/// handshake or the first snapshot is still in the air, red once the endpoint
/// is gone.
fn connection_color(status: ConnectionStatus, colors: &Palette) -> Hsla {
    match status {
        ConnectionStatus::Connected => colors.success,
        ConnectionStatus::Connecting | ConnectionStatus::AwaitingSnapshot => colors.warning,
        ConnectionStatus::Disconnected => colors.danger,
    }
}

/// The index a dragged tab inserts at, or `None` when the two gaps around it
/// are all a drop on `onto` can mean. The daemon counts `insert_index` with
/// the moving row still in place, so the gap before the row at `onto` is
/// `onto`, and a row dropped on itself or on the one after it stays put.
fn insert_gap(from: usize, onto: usize) -> Option<usize> {
    (onto != from && onto != from + 1).then_some(onto)
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use ely_core::theme::Palette;

    use super::*;
    use crate::i18n::t;

    #[test]
    fn every_view_has_a_name_a_source_and_an_id_that_reads_back() {
        for view in View::ALL {
            assert_ne!(t(view.label_key()), view.label_key(), "{:?}", view);
            assert_ne!(t(view.source_key()), view.source_key(), "{:?}", view);
            assert_eq!(View::from_id(view.id()), Some(view), "{:?}", view);
        }
        let ids: Vec<&str> = View::ALL.iter().map(|view| view.id()).collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");
    }

    #[test]
    fn a_count_keeps_its_units() {
        for count in [0usize, 1, 2] {
            let text = counted("status.count.tab_one", "status.count.tab_many", count);
            assert!(text.contains(&count.to_string()), "{count}: {text}");
            assert!(!text.contains('%'), "{count}: {text}");
        }
    }

    #[test]
    fn the_connection_dot_reads_each_state() {
        let colors = Palette::dark(false);
        assert_eq!(
            connection_color(ConnectionStatus::Connected, &colors),
            colors.success
        );
        assert_eq!(
            connection_color(ConnectionStatus::Connecting, &colors),
            colors.warning
        );
        assert_eq!(
            connection_color(ConnectionStatus::AwaitingSnapshot, &colors),
            colors.warning
        );
        assert_eq!(
            connection_color(ConnectionStatus::Disconnected, &colors),
            colors.danger
        );
    }

    /// The two gaps around a tab are not moves: a drop there leaves the order
    /// as it is. A drop elsewhere names the gap before the tab it landed on.
    #[test]
    fn a_tab_drop_names_the_gap_it_landed_before() {
        assert_eq!(insert_gap(2, 2), None);
        assert_eq!(insert_gap(2, 3), None);
        assert_eq!(insert_gap(0, 2), Some(2));
        assert_eq!(insert_gap(3, 1), Some(1));
        assert_eq!(insert_gap(0, 0), None);
    }
}
