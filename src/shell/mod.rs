//! The workspace shell: a title bar, a sidebar of workspaces, a tab strip,
//! the terminal grid, and a status bar.
//!
//! The daemon owns the layout: the shell draws one composite surface and asks
//! the daemon to create, close and focus workspaces, tabs and panes. Local
//! state is only what the daemon has no opinion about — the sidebar's
//! visibility and the chrome's light/dark mode.
//!
//! Structure follows herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/window.rs`
//! in shape, at skeleton scale by decision (D3): the rich sidebar rows, the
//! worktree grouping, the browser tabs, the menus and the command palette
//! stay upstream.

pub(crate) mod commands;
pub(crate) mod keymap;
pub(crate) mod panel_resize;
pub(crate) mod panels;
pub(crate) mod registry;

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, Theme as UiTheme, ThemeMode, h_flex, v_flex};
use gpui_kit::*;
use herdr_client::protocol::ClientShellTab;

use crate::herdr::Connection;
use crate::terminal::Theme as TerminalTheme;
use crate::terminal::view::TerminalView;
use commands::Command;
use keymap::{Keymap, RunCommand};
use panel_resize::{PanelWidth, SIDEBAR, SidebarDrag};
use registry::{Dispatch, Invocation, Registry, ShellRequest};

const TITLE_BAR_HEIGHT: f32 = 34.;
const TAB_STRIP_HEIGHT: f32 = 28.;

pub(crate) struct ShellView {
    terminal: Entity<TerminalView>,
    connection: Connection,
    registry: Registry,
    keymap: Keymap,
    /// Whether the sidebar is shown. Local: the daemon does not know about it.
    sidebar_visible: bool,
    /// How wide the sidebar is drawn. Local, like its visibility.
    sidebar: PanelWidth,
    /// The prefix has been typed and the next keystroke is its chord.
    prefix_armed: bool,
    /// The terminal's last drawn revision, so a paint that changed nothing
    /// else does not rebuild the sidebar.
    drawn: u64,
    _observe: Subscription,
    _prefix: Subscription,
}

impl ShellView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let terminal = cx.new(TerminalView::new);
        let connection = terminal.read(cx).connection();
        let observe = cx.observe(&terminal, |shell, terminal, cx| {
            let revision = terminal.read(cx).live().revision;
            if revision != shell.drawn {
                shell.drawn = revision;
                cx.notify();
            }
        });
        let prefix = Self::intercept_prefix(cx);
        let mut shell = Self {
            terminal,
            connection,
            registry: Registry::builtin(),
            keymap: Keymap::default(),
            sidebar_visible: true,
            sidebar: SIDEBAR,
            prefix_armed: false,
            drawn: 0,
            _observe: observe,
            _prefix: prefix,
        };
        shell.sync_terminal_theme(cx);
        shell
    }

    /// The terminal keeps the keyboard: the shell has no focus of its own.
    pub(crate) fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.terminal.read(cx).focus_handle().clone()
    }

    /// GPUI calls keystroke interceptors before any binding or key handler, so
    /// an armed prefix claims the next key wherever focus is. Interceptors are
    /// app-wide; with one window (M4) there is nothing to filter by, and a
    /// second window must claim only its own keystrokes when it arrives.
    fn intercept_prefix(cx: &mut Context<Self>) -> Subscription {
        let view = cx.weak_entity();
        cx.intercept_keystrokes(move |event, window, cx| {
            let _ = view.update(cx, |shell, cx| {
                shell.prefix_keystroke(&event.keystroke, window, cx);
            });
        })
    }

    fn prefix_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_prefix = self.keymap.is_prefix(keystroke);
        if !self.prefix_armed {
            if is_prefix {
                self.prefix_armed = true;
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        let chord = self.keymap.chord(keystroke);
        self.prefix_armed = false;
        cx.notify();
        // Typing the prefix twice sends it on as ordinary input rather than
        // swallowing it, so a pane that binds the prefix still receives it.
        if is_prefix {
            return;
        }
        cx.stop_propagation();
        if let Some(command) = chord {
            self.run(command, window, cx);
        }
        // A chord with nothing bound to it is swallowed, as in herdr.
    }

    /// Run one directory command: locally when the shell owns it, otherwise as
    /// the daemon method the snapshot resolves it to.
    fn run(&mut self, command: Command, _window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                cx.notify();
            }
            Command::ToggleTheme => {
                toggle_theme(cx);
                self.sync_terminal_theme(cx);
            }
            command => self.dispatch(ShellRequest::Command(command), cx),
        }
    }

    /// A keystroke GPUI bound directly, dispatched as its action. The prefix
    /// chords arrive through the interceptor instead, and both end in [`Self::run`].
    fn run_command(&mut self, action: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        self.run(action.command, window, cx);
    }

    /// Hand a request to the daemon, or run it locally. Both the keymap and a
    /// contribution's click arrive here, so the two cannot behave differently.
    fn dispatch(&mut self, request: ShellRequest, cx: &mut Context<Self>) {
        match request {
            ShellRequest::FocusWorkspace(workspace_id) => {
                self.connection.focus_workspace(&workspace_id)
            }
            ShellRequest::FocusTab(tab_id) => self.connection.focus_tab(&tab_id),
            ShellRequest::Command(command) => {
                let snapshot = self.terminal.read(cx).live().snapshot.clone();
                if let Some((method, params)) = snapshot
                    .as_ref()
                    .and_then(|snapshot| commands::request(command, snapshot))
                {
                    self.connection.request(method, params);
                }
            }
        }
    }

    /// The terminal palette that matches the chrome's mode. The zenix theme
    /// system and this one meet in M5; until then the terminal keeps its own
    /// built-in palettes and follows the mode's light/dark pair. The gpui-kit
    /// theme is the only source of truth for the mode: a second local flag
    /// could disagree with it.
    fn sync_terminal_theme(&mut self, cx: &mut Context<Self>) {
        let name = if cx.theme().is_dark() {
            "Default"
        } else {
            "Catppuccin Latte"
        };
        let theme = TerminalTheme::builtin(name).unwrap_or_default();
        self.terminal
            .update(cx, |view, cx| view.set_theme(theme, cx));
    }

    /// Follow the pointer with the sidebar's edge, resolved against the
    /// sidebar's own bounds: the drag reports where the pointer is, not how far
    /// the panel should grow.
    fn drag_sidebar(
        &mut self,
        event: &DragMoveEvent<SidebarDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar.drag(event.bounds, event.event.position.x) {
            cx.notify();
        }
    }

    /// A double-click on the grip puts the sidebar back to its default width.
    fn reset_sidebar(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.reset();
        cx.notify();
    }
}

/// Flip the chrome between light and dark, reading the mode that is actually
/// in effect rather than tracking one.
fn toggle_theme(cx: &mut App) {
    let mode = if cx.theme().is_dark() {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    UiTheme::change(mode, None, cx);
}

impl Render for ShellView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let live = self.terminal.read(cx).live().clone();
        let error = self.connection.footer();
        let theme = cx.theme().clone();
        let dispatch: Dispatch = Rc::new({
            let connection = self.connection.clone();
            let snapshot = live.snapshot.clone();
            let entity = cx.entity();
            move |request, _window, cx| match request {
                ShellRequest::FocusWorkspace(workspace_id) => {
                    connection.focus_workspace(&workspace_id)
                }
                ShellRequest::FocusTab(tab_id) => connection.focus_tab(&tab_id),
                ShellRequest::Command(Command::ToggleSidebar) => {
                    let _ = entity.update(cx, |shell, cx| {
                        shell.sidebar_visible = !shell.sidebar_visible;
                        cx.notify();
                    });
                }
                ShellRequest::Command(Command::ToggleTheme) => {
                    let _ = entity.update(cx, |shell, cx| {
                        toggle_theme(cx);
                        shell.sync_terminal_theme(cx);
                    });
                }
                ShellRequest::Command(command) => {
                    if let Some((method, params)) = snapshot
                        .as_ref()
                        .and_then(|snapshot| commands::request(command, snapshot))
                    {
                        connection.request(method, params);
                    }
                }
            }
        });
        let prefix = self.prefix_armed.then(|| self.keymap.prefix_label());
        let invocation = Invocation {
            snapshot: live.snapshot.as_deref(),
            surface: live.surface.as_deref(),
            status: live.status,
            status_error: error.as_deref(),
            rejection: live.rejected.as_deref(),
            prefix: prefix.as_deref(),
            theme: &theme,
            dispatch: &dispatch,
        };

        // Not `h_flex()`: that centres its children on the cross axis, which
        // would leave the grid column at its content height instead of filling
        // the window.
        let sidebar_width = self
            .sidebar
            .width(window.viewport_size().width.to_f64() as f32);
        let mut body = div().flex().flex_row().flex_1().min_h_0().w_full();
        if self.sidebar_visible {
            body = body.child(
                v_flex()
                    .relative()
                    .w(px(sidebar_width))
                    .h_full()
                    .flex_none()
                    .overflow_hidden()
                    .bg(theme.sidebar)
                    .border_r_1()
                    .border_color(theme.sidebar_border)
                    // The edge facing the grid is the sidebar's own handle, so
                    // the drag is resolved inside the panel it resizes.
                    .on_drag_move(cx.listener(Self::drag_sidebar))
                    .child(panel_resize::grip(
                        "sidebar-grip",
                        cx.listener(Self::reset_sidebar),
                    ))
                    .children(
                        self.registry
                            .panels()
                            .iter()
                            .map(|panel| (panel.render)(&invocation, window, cx)),
                    ),
            );
        }
        v_flex()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            // Every directly bound keystroke dispatches its action here: a
            // binding with no handler matches and does nothing.
            .on_action(cx.listener(Self::run_command))
            .child(title_bar(&invocation, self.sidebar_visible))
            .child(
                body.child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .child(tab_strip(&invocation))
                        // The grid needs the remaining height: inside a flex
                        // column a childless `size_full` collapses to the height
                        // its own content asks for, which for a canvas is one row.
                        .child(div().flex_1().min_h_0().child(self.terminal.clone())),
                ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .w_full()
                    .px_2()
                    .py_0p5()
                    .gap_3()
                    .items_center()
                    .bg(theme.status_bar)
                    .border_t_1()
                    .border_color(theme.status_bar_border)
                    .children(
                        self.registry
                            .status_segments()
                            .iter()
                            .map(|segment| (segment.render)(&invocation, window, cx)),
                    ),
            )
    }
}

/// The chrome above the grid: which workspace is in front, and the two local
/// toggles. Window controls stay the platform's until M7 has a frameless
/// window to draw them on.
fn title_bar(invocation: &Invocation, sidebar_visible: bool) -> AnyElement {
    let theme = invocation.theme;
    let title = invocation
        .snapshot
        .and_then(|snapshot| {
            let id = snapshot.focused_workspace_id.as_ref()?;
            snapshot
                .workspaces
                .iter()
                .find(|workspace| &workspace.workspace_id == id)
                .map(|workspace| workspace.label.clone())
        })
        .unwrap_or_default();
    h_flex()
        .flex_none()
        .w_full()
        .h(px(TITLE_BAR_HEIGHT))
        .px_2()
        .gap_2()
        .items_center()
        .bg(theme.title_bar)
        .border_b_1()
        .border_color(theme.title_bar_border)
        .child(toggle_button(
            "toggle-sidebar",
            if sidebar_visible { "<" } else { ">" },
            Command::ToggleSidebar,
            invocation,
        ))
        .child(div().flex_1().text_sm().child(title))
        .child(toggle_button(
            "toggle-theme",
            if theme.is_dark() { "light" } else { "dark" },
            Command::ToggleTheme,
            invocation,
        ))
        .into_any_element()
}

fn toggle_button(
    id: &'static str,
    label: &'static str,
    command: Command,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_sm()
        .text_xs()
        .bg(theme.secondary)
        .text_color(theme.secondary_foreground)
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::Command(command), window, cx);
        })
        .child(label)
        .into_any_element()
}

/// The focused workspace's tabs, then the button that opens another one.
fn tab_strip(invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let tabs: Vec<&ClientShellTab> = invocation
        .snapshot
        .map(|snapshot| {
            let workspace = snapshot.focused_workspace_id.as_ref().and_then(|id| {
                snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| &workspace.workspace_id == id)
            });
            match workspace {
                Some(workspace) => snapshot
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == workspace.workspace_id)
                    .collect(),
                None => Vec::new(),
            }
        })
        .unwrap_or_default();
    let mut strip = h_flex()
        .flex_none()
        .w_full()
        .h(px(TAB_STRIP_HEIGHT))
        .px_1()
        .gap_1()
        .items_center()
        .bg(theme.tab_bar)
        .border_b_1()
        .border_color(theme.border);
    strip = strip.children(tabs.into_iter().map(|tab| tab_button(tab, invocation)));
    strip
        .child(toggle_button("new-tab", "+", Command::NewTab, invocation))
        .into_any_element()
}

fn tab_button(tab: &ClientShellTab, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let open = invocation.dispatch.clone();
    let close = invocation.dispatch.clone();
    let tab_id = tab.tab_id.clone();
    let label = if tab.zoomed {
        format!("{} ^", tab.label)
    } else {
        tab.label.clone()
    };
    h_flex()
        .id(("tab", tab.number))
        .px_2()
        .py_0p5()
        .gap_1()
        .items_center()
        .rounded_sm()
        .bg(if tab.focused {
            theme.tab_active
        } else {
            theme.tab
        })
        .text_color(if tab.focused {
            theme.tab_active_foreground
        } else {
            theme.tab_foreground
        })
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            open(ShellRequest::FocusTab(tab_id.clone()), window, cx);
        })
        .child(div().text_sm().child(label))
        .child(
            div()
                .id(("tab-close", tab.number))
                .px_0p5()
                .text_xs()
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close(ShellRequest::Command(Command::CloseTab), window, cx);
                })
                .child("x"),
        )
        .into_any_element()
}
