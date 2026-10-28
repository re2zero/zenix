//! The plugin manager: one plugin's page, docked to the window's right edge.
//!
//! The sidebar's plugins view lists the daemon's registry; a row opens this
//! drawer on that plugin. It is the design's 480px column (`docs/ui/redesign`
//! `.drawer.w480`): the plugin's identity, four tabs (overview / actions /
//! panes / logs), and the footer that governs it — enable or disable, review
//! the commands it runs, or unlink it through the shared confirmation.
//!
//! Everything shown here is read from [`PluginState`], which the host fills
//! off the UI thread; the drawer itself asks the daemon for nothing and starts
//! no timer. The one thing it decides is what a press dispatches: a toggle is
//! `plugin.enable` / `plugin.disable`, an unlink is `plugin.unlink` behind the
//! confirmation, an action is `plugin.action.invoke`, and a pane is
//! `plugin.pane.open` — the same calls every other surface makes.
//!
//! A drawer is not a mode: it wears no scrim and leaves the shell beside it
//! readable. Escape, its own close button and a press outside it all close it,
//! and the keyboard comes back to the grid.

use ely_core::{
    buttons::{Button, ButtonVariant, IconButton},
    layout::ScrollArea,
    primitives::{Icon, IconName},
    theme::{ActiveTheme, ControlSize, Elevation, IconSize, Radius, TextSize},
    typography::Ellipsis,
};
use gpui::{
    AnyElement, App, Entity, FocusHandle, IntoElement, ParentElement, SharedString, Styled, Window,
    div, prelude::*, px,
};

use super::keymap::Keymap;
use super::shell::Root;
use crate::i18n::t;
use crate::plugins::host::PluginState;
use crate::plugins::model::{
    Availability, Context, InstalledPlugin, ManifestPane, PluginAction, PluginCommandLog,
    PluginSource, SourceKind,
};
use crate::plugins::trust::{self, CommandLine};
use crate::plugins::{paths, ui_manifest::UiContributions};

/// The drawer's width and the shell edges it keeps visible: the title strip
/// above and the status bar below, so the frame still reads as the window's.
const WIDTH: f32 = 480.;
const TOP: f32 = 38.;
const BOTTOM: f32 = 26.;
/// How many of a plugin's log entries its page shows.
const LOGS_SHOWN: usize = 20;

/// The drawer's pages, in strip order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Overview,
    Actions,
    Panes,
    Logs,
}

impl Tab {
    pub(crate) const ALL: &'static [Tab] = &[Tab::Overview, Tab::Actions, Tab::Panes, Tab::Logs];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Actions => "actions",
            Self::Panes => "panes",
            Self::Logs => "logs",
        }
    }

    fn label(self) -> String {
        t(&format!("shell.manager.tab.{}", self.id()))
    }
}

/// The drawer's own state: whether it is up, which plugin it shows, and which
/// page is in front.
#[derive(Default)]
pub(crate) struct State {
    open: bool,
    plugin_id: Option<String>,
    tab: Tab,
}

impl State {
    pub(crate) fn open(&mut self, plugin_id: &str) {
        self.open = true;
        self.plugin_id = Some(plugin_id.to_owned());
        self.tab = Tab::Overview;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.plugin_id = None;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn tab(&self) -> Tab {
        self.tab
    }

    pub(crate) fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    /// The plugin the drawer is about, or `None` when it is closed or the
    /// plugin was unlinked while its page was open. The registry, not this
    /// state, decides what exists.
    fn selected<'a>(&self, plugins: &'a [InstalledPlugin]) -> Option<&'a InstalledPlugin> {
        if !self.open {
            return None;
        }
        let id = self.plugin_id.as_deref()?;
        plugins.iter().find(|plugin| plugin.plugin_id == id)
    }

    /// Whether the row for `plugin_id` is the one in front, which the sidebar
    /// draws as its selected row.
    pub(crate) fn shows(&self, plugin_id: &str) -> bool {
        self.open && self.plugin_id.as_deref() == Some(plugin_id)
    }

    /// The plugin in front, for the sidebar's selected row and the shell's
    /// check that the page's plugin still exists.
    pub(crate) fn plugin_id(&self) -> Option<&str> {
        self.open.then_some(self.plugin_id.as_deref()).flatten()
    }
}

/// What the drawer reads, one borrow per source. Handed over together so the
/// overlay's signature stays one row per surface rather than one per source.
pub(crate) struct Sources<'a> {
    pub(crate) state: &'a State,
    pub(crate) plugins: &'a PluginState,
    pub(crate) availability: Availability,
    pub(crate) keymap: &'a Keymap,
    /// The shell's handle for the drawer: opening focuses it and closing hands
    /// the keyboard back to the grid.
    pub(crate) focus: &'a FocusHandle,
    /// A modal surface is above the drawer (the trust review it opened, the
    /// contract, the confirmation). A press inside that surface is not a press
    /// outside this one: the drawer must not close under it.
    pub(crate) blocked: bool,
}

/// The drawer over the shell, or nothing when it is closed or its plugin is
/// gone.
pub(crate) fn overlay(
    sources: Sources<'_>,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &App,
) -> Option<AnyElement> {
    let Sources {
        state,
        plugins,
        availability,
        keymap,
        focus,
        blocked,
    } = sources;
    let plugin = state.selected(&plugins.plugins)?;
    let theme = cx.theme();
    let colors = &theme.colors;
    let width = px(WIDTH).min(window.viewport_size().width * 0.94);
    let close = {
        let root = root.clone();
        move |window: &mut Window, cx: &mut App| {
            root.update(cx, |root, cx| root.close_plugin_manager(window, cx));
        }
    };
    let panel = div()
        .id("plugin-manager-drawer")
        .track_focus(focus)
        .absolute()
        .top(px(TOP))
        .bottom(px(BOTTOM))
        .right_0()
        .w(width)
        .flex()
        .flex_col()
        .overflow_hidden()
        .border_l_1()
        .border_color(colors.border_strong)
        .bg(colors.bg)
        .shadow(theme.elevation(Elevation::Modal))
        .on_key_down({
            let close = close.clone();
            move |event, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    close(window, cx);
                }
            }
        })
        .when(!blocked, |panel| {
            panel.on_mouse_down_out({
                let close = close.clone();
                move |event, window, cx| {
                    if event.button == gpui::MouseButton::Left {
                        close(window, cx);
                    }
                }
            })
        })
        .child(head(plugin, plugins, root, cx))
        .child(strip(state.tab, root, cx))
        .child(
            div().flex_1().min_h_0().child(
                ScrollArea::new("plugin-manager-body")
                    .vertical()
                    .shadows()
                    .size_full()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .child(match state.tab {
                                Tab::Overview => overview(plugin, plugins, cx),
                                Tab::Actions => {
                                    actions(plugin, plugins, availability, keymap, root, cx)
                                }
                                Tab::Panes => panes(plugin, root, cx),
                                Tab::Logs => logs(plugin, plugins, cx),
                            }),
                    ),
            ),
        )
        .child(footer(plugin, root, cx));
    Some(
        div()
            .id("plugin-manager-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .child(panel)
            .into_any_element(),
    )
}

/// The head: what plugin this is, where it came from, and the close button.
fn head(
    plugin: &InstalledPlugin,
    plugins: &PluginState,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let local = plugins
        .declared
        .iter()
        .find(|entry| entry.plugin_id == plugin.plugin_id)
        .is_some_and(|entry| !slots_of(&entry.ui, &plugin.plugin_id).is_empty());
    let close = root.clone();
    div()
        .h(px(40.))
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .border_b_1()
        .border_color(colors.border)
        .child(
            Icon::new(IconName::Puzzle)
                .size(IconSize::Sm)
                .color(colors.accent),
        )
        .child(
            div()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Sm))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(colors.fg)
                .child(Ellipsis::new(SharedString::from(plugin.name.clone()))),
        )
        .child(chip(plugin.version.clone(), colors.fg_subtle, cx))
        .child(chip(
            if local {
                t("shell.plugins.source_local")
            } else {
                t("shell.plugins.source_daemon")
            },
            if local {
                colors.accent
            } else {
                colors.fg_subtle
            },
            cx,
        ))
        .child(div().flex_1())
        .child(
            IconButton::new("plugin-manager-close", IconName::X)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip(t("shell.aside.close"))
                .on_click(move |_, window, cx| {
                    close.update(cx, |root, cx| root.close_plugin_manager(window, cx));
                }),
        )
        .into_any_element()
}

/// The four page chips. One press moves the body; the head's counts stay put.
fn strip(tab: Tab, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut row = div()
        .h(px(34.))
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .px_2()
        .border_b_1()
        .border_color(colors.border);
    for candidate in Tab::ALL {
        let active = *candidate == tab;
        let choose = root.clone();
        let chosen = *candidate;
        row = row.child(
            div()
                .id(SharedString::from(format!(
                    "plugin-manager-tab-{}",
                    candidate.id()
                )))
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .rounded(theme.radius(Radius::Sm))
                .text_size(theme.text_size(TextSize::Xs))
                .cursor_pointer()
                .when(active, |chip| chip.bg(colors.surface).text_color(colors.fg))
                .when(!active, |chip| {
                    chip.text_color(colors.fg_muted)
                        .hover(move |style| style.bg(colors.hover))
                })
                .on_click(move |_, _, cx| {
                    choose.update(cx, |root, cx| root.set_manager_tab(chosen, cx));
                })
                .child(candidate.label()),
        );
    }
    row.into_any_element()
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

/// The plugin's whole contract on one page: where it lives, what binds it to
/// the daemon and this client, the UI it declares, every argv it would run,
/// and the last thing that happened to it.
fn overview(plugin: &InstalledPlugin, plugins: &PluginState, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let ui = declared_for(plugins, &plugin.plugin_id);
    let slots = slots_of(ui, &plugin.plugin_id);
    let plugin_logs = logs_of(plugins, plugin);
    let mut column = div().flex().flex_col().gap_2();

    let mut contract = card(t("shell.manager.contract"), cx);
    contract = contract
        .child(kv_row(
            t("shell.manager.manifest"),
            mono(&plugin.manifest_path, colors.fg_muted, cx),
            cx,
        ))
        .child(kv_row(
            t("shell.manager.state_dir"),
            mono(
                &paths::plugin_state_dir(&plugin.plugin_id).to_string_lossy(),
                colors.fg_muted,
                cx,
            ),
            cx,
        ))
        .child(kv_row(
            t("shell.manager.config_dir"),
            mono(
                &paths::plugin_config_dir(&plugin.plugin_id).to_string_lossy(),
                colors.fg_muted,
                cx,
            ),
            cx,
        ))
        .child(kv_row(
            t("shell.manager.source"),
            text(&source_text(&plugin.source), colors.fg, cx),
            cx,
        ))
        .child(kv_row(
            t("shell.manager.platforms"),
            text(
                &plugin
                    .platforms
                    .as_ref()
                    .map(|platforms| {
                        platforms
                            .iter()
                            .map(|platform| platform.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| t("shell.manager.undeclared")),
                colors.fg,
                cx,
            ),
            cx,
        ))
        .child(kv_row(
            t("shell.manager.min_herdr"),
            text(
                &if plugin.min_herdr_version.is_empty() {
                    t("shell.manager.undeclared")
                } else {
                    plugin.min_herdr_version.clone()
                },
                colors.fg,
                cx,
            ),
            cx,
        ))
        .child(kv_row(
            t("shell.manager.contract_kind"),
            text(
                &format!(
                    "herdr plugin v1 · {} · {}",
                    t("shell.manager.no_sandbox"),
                    if slots.is_empty() {
                        t("shell.manager.no_ui_declaration")
                    } else {
                        "zenix-ui.toml".to_owned()
                    }
                ),
                colors.fg_muted,
                cx,
            ),
            cx,
        ));
    for warning in &plugin.warnings {
        contract = contract.child(text(warning, colors.warning, cx));
    }
    column = column.child(contract);

    // The argv the daemon runs for this plugin, graded — the same read the
    // pre-link review makes, now taken off the installed contract.
    let commands = trust::contract_commands(plugin);
    let mut argv = card(t("shell.manager.argv_section"), cx);
    if commands.is_empty() {
        argv = argv.child(text(&t("shell.manager.argv_none"), colors.fg_muted, cx));
    }
    for (index, command) in commands.iter().enumerate() {
        argv = argv.child(argv_line(index, command, cx));
    }
    column = column.child(argv);

    let mut declared = card(t("shell.manager.slots"), cx);
    if slots.is_empty() {
        declared = declared.child(text(&t("shell.manager.no_ui"), colors.fg_muted, cx));
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
    column = column.child(declared);

    let mut running = card(t("shell.plugins.logs"), cx);
    if plugin_logs.is_empty() {
        running = running.child(text(
            &t("shell.plugins.events-not-run"),
            colors.fg_muted,
            cx,
        ));
    }
    for log in plugin_logs.iter().take(3) {
        running = running.child(log_row(log, cx));
    }
    column = column.child(running);
    column.into_any_element()
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

fn actions(
    plugin: &InstalledPlugin,
    plugins: &PluginState,
    availability: Availability,
    keymap: &Keymap,
    root: &Entity<Root>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let actions = actions_of(plugins, plugin);
    let logs = logs_of(plugins, plugin);
    let mut column = div().flex().flex_col().gap_1p5();
    if actions.is_empty() {
        return text(&t("shell.manager.no_actions"), colors.fg_muted, cx);
    }
    if !plugin.enabled {
        column = column.child(text(
            &t("shell.manager.actions_disabled"),
            colors.warning,
            cx,
        ));
    }
    column = column.child(text(&t("shell.manager.argv_section"), colors.fg_subtle, cx));
    for (index, action) in actions.iter().enumerate() {
        let offered = plugin.enabled && action.is_available(availability);
        let chord = keymap.plugin_chord_label(&action.plugin_id, &action.action_id);
        let contexts = contexts(action);
        let description = action.description.clone();
        let run = root.clone();
        let cloned = (*action).clone();
        let mut row = div()
            .id(("plugin-manager-action", index))
            .flex()
            .items_start()
            .gap_2()
            .rounded(theme.radius(Radius::Md))
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .px_2()
            .py_1p5()
            .when(offered, |row| {
                row.cursor_pointer()
                    .hover(move |style| style.border_color(colors.border_strong))
                    .on_click(move |_, _, cx| {
                        let action = cloned.clone();
                        run.update(cx, |root, cx| root.run_manifest_action(action, cx));
                    })
            })
            .child(
                Icon::new(IconName::Play)
                    .size(IconSize::Xs)
                    .color(if offered {
                        colors.accent
                    } else {
                        colors.fg_disabled
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Sm))
                            .text_color(if offered { colors.fg } else { colors.fg_muted })
                            .child(Ellipsis::new(SharedString::from(action.title.clone()))),
                    )
                    .child(
                        div()
                            .text_size(theme.text_size(TextSize::Xs))
                            .text_color(colors.fg_subtle)
                            .child(if let Some(description) = description {
                                Ellipsis::new(SharedString::from(description))
                            } else {
                                Ellipsis::new(SharedString::from(contexts.clone()))
                            }),
                    ),
            )
            .children(chord.map(|chord| chip(chord, colors.fg_muted, cx)))
            .children(last_status(&logs, action).map(|(word, failed)| {
                chip(
                    word,
                    if failed {
                        colors.danger
                    } else {
                        colors.fg_subtle
                    },
                    cx,
                )
            }));
        row = row.child(chip(contexts, colors.fg_subtle, cx));
        column = column.child(row);
    }
    column
        .child(text(
            &t("shell.manager.contexts_hint"),
            colors.fg_subtle,
            cx,
        ))
        .into_any_element()
}

/// The newest run of one action in the log ring, as its status word. An action
/// with no run yet says nothing rather than "idle".
fn last_status(logs: &[&PluginCommandLog], action: &PluginAction) -> Option<(String, bool)> {
    let log = logs
        .iter()
        .find(|log| log.action_id.as_deref() == Some(action.action_id.as_str()))?;
    Some((
        log.status.label().to_owned(),
        log.status == crate::plugins::model::LogStatus::Failed,
    ))
}

// ---------------------------------------------------------------------------
// Panes
// ---------------------------------------------------------------------------

fn panes(plugin: &InstalledPlugin, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    if plugin.panes.is_empty() {
        return text(&t("shell.manager.argv_none"), colors.fg_muted, cx);
    }
    let mut column = div().flex().flex_col().gap_1p5();
    for (index, pane) in plugin.panes.iter().enumerate() {
        let open = root.clone();
        let plugin_id = plugin.plugin_id.clone();
        let entrypoint = pane.id.clone();
        let title = pane.title.clone();
        let placement = pane.placement;
        column = column.child(
            div()
                .id(("plugin-manager-pane", index))
                .flex()
                .items_center()
                .gap_2()
                .rounded(theme.radius(Radius::Md))
                .border_1()
                .border_color(colors.border)
                .bg(colors.surface)
                .px_2()
                .py_1p5()
                .cursor_pointer()
                .hover(move |style| style.border_color(colors.border_strong))
                .on_click(move |_, window, cx| {
                    let (plugin_id, entrypoint, title) =
                        (plugin_id.clone(), entrypoint.clone(), title.clone());
                    open.update(cx, |root, cx| {
                        root.open_plugin_pane(plugin_id, entrypoint, title, placement, window, cx)
                    });
                })
                .child(
                    Icon::new(IconName::Terminal)
                        .size(IconSize::Xs)
                        .color(colors.accent),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Sm))
                        .text_color(colors.fg)
                        .child(Ellipsis::new(SharedString::from(pane.title.clone()))),
                )
                .child(chip(placement_text(pane), colors.fg_subtle, cx)),
        );
    }
    column.into_any_element()
}

fn placement_text(pane: &ManifestPane) -> String {
    pane.placement.as_str().to_owned()
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

fn logs(plugin: &InstalledPlugin, plugins: &PluginState, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let logs = logs_of(plugins, plugin);
    if logs.is_empty() {
        return text(&t("shell.plugins.events-not-run"), colors.fg_muted, cx);
    }
    let mut column = div().flex().flex_col().gap_1();
    for log in logs.iter().take(LOGS_SHOWN) {
        column = column.child(log_row(log, cx));
    }
    column.into_any_element()
}

/// One log entry: what ran, the outcome, and the first line of output.
fn log_row(log: &PluginCommandLog, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let failed = log.status == crate::plugins::model::LogStatus::Failed;
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .rounded(theme.radius(Radius::Sm))
        .border_1()
        .border_color(colors.border)
        .px_2()
        .py_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_muted)
                        .child(Ellipsis::new(SharedString::from(log.command.join(" ")))),
                )
                .child({
                    let mut chip = div()
                        .flex_none()
                        .px_1()
                        .rounded(theme.radius(Radius::Sm))
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(if failed {
                            colors.danger
                        } else {
                            colors.fg_subtle
                        });
                    if failed {
                        chip = chip.bg(colors.danger.opacity(0.08));
                    }
                    chip.child(log.status.label().to_owned())
                }),
        )
        .child(
            div()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(if failed {
                    colors.danger
                } else {
                    colors.fg_subtle
                })
                .child(Ellipsis::new(SharedString::from(log.outcome()))),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Footer
// ---------------------------------------------------------------------------

/// The governance strip: what state the plugin is in, the switch, and the two
/// requests that leave the client — reviewing the commands and unlinking.
fn footer(plugin: &InstalledPlugin, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let plugin_id = plugin.plugin_id.clone();
    let name = plugin.name.clone();
    let plugin_root = plugin.plugin_root.clone();
    let enabled = plugin.enabled;
    let toggle = root.clone();
    let toggle_id = plugin_id.clone();
    let unlink = root.clone();
    let unlink_id = plugin_id.clone();
    let unlink_name = name.clone();
    let review = root.clone();
    div()
        .h(px(46.))
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .border_t_1()
        .border_color(colors.border)
        .child(div().size(px(6.)).rounded_full().bg(if enabled {
            colors.success
        } else {
            colors.fg_disabled
        }))
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_muted)
                .child(if enabled {
                    t("shell.manager.enabled")
                } else {
                    t("shell.manager.disabled")
                }),
        )
        .child(
            ely_core::forms::Switch::new("plugin-manager-enabled", enabled).on_change(
                move |on, _, cx| {
                    let plugin_id = toggle_id.clone();
                    toggle.update(cx, |root, cx| {
                        root.set_plugin_enabled(plugin_id.clone(), on, cx)
                    });
                },
            ),
        )
        .child(div().flex_1())
        .child(
            Button::new("plugin-manager-review", t("shell.manager.argv"))
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .on_click(move |_, window, cx| {
                    let path = plugin_root.clone();
                    review.update(cx, |root, cx| root.open_trust_at(Some(path), window, cx));
                }),
        )
        .child(
            Button::new("plugin-manager-unlink", t("shell.manager.unlink"))
                .variant(ButtonVariant::Danger)
                .size(ControlSize::Sm)
                .on_click(move |_, _, cx| {
                    let (plugin_id, name) = (unlink_id.clone(), unlink_name.clone());
                    unlink.update(cx, |root, cx| root.ask_unlink_plugin(plugin_id, name, cx));
                }),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

/// A card: a section head and the rows under it, the design's `.kv-card`.
fn card(title: String, cx: &App) -> gpui::Div {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .rounded(theme.radius(Radius::Lg))
        .border_1()
        .border_color(colors.border)
        .bg(colors.bg)
        .p_2p5()
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(colors.fg_muted)
                .child(title),
        )
}

fn kv_row(label: String, value: AnyElement, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(colors.fg_subtle)
                .child(label),
        )
        .child(div().min_w_0().flex_1().child(value))
        .into_any_element()
}

fn text(text: &str, color: gpui::Hsla, cx: &App) -> AnyElement {
    let size = cx.theme().text_size(TextSize::Xs);
    div()
        .text_size(size)
        .text_color(color)
        .child(Ellipsis::new(SharedString::from(text.to_owned())))
        .into_any_element()
}

fn mono(text: &str, color: gpui::Hsla, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(color)
        .child(Ellipsis::new(SharedString::from(text.to_owned())))
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

/// One command the daemon will run, graded the way the pre-link review grades
/// it: the rule's color and the rule's sentence, or the muted in-plugin word.
fn argv_line(index: usize, command: &CommandLine, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let verdict = trust::hazard_for(&command.argv);
    let (color, reason, level) = match verdict {
        Some((reason, level)) => (
            match level {
                trust::HazardLevel::High => colors.danger,
                trust::HazardLevel::Medium => colors.warning,
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
                    .w(px(14.))
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
        row = row.child(text(&reason, color, cx));
    }
    row.into_any_element()
}

fn grade_chip(level: trust::HazardLevel, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let color = match level {
        trust::HazardLevel::High => colors.danger,
        trust::HazardLevel::Medium => colors.warning,
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

/// What one plugin declares for this client, enabled or not. The mounted set
/// is the enabled half; a page is about the plugin, not about what is drawn.
fn declared_for<'a>(state: &'a PluginState, plugin_id: &str) -> &'a UiContributions {
    state
        .declared
        .iter()
        .find(|entry| entry.plugin_id == plugin_id)
        .map(|entry| &entry.ui)
        .unwrap_or(&state.ui)
}

/// One slot a plugin filled: the kind, the local id, and what a reader sees.
pub(crate) struct Slot {
    pub(crate) kind: &'static str,
    pub(crate) id: String,
    pub(crate) title: String,
}

pub(crate) fn slots_of(ui: &UiContributions, plugin_id: &str) -> Vec<Slot> {
    let mut slots = Vec::new();
    for item in ui.sidebar_items.iter().filter(|s| s.plugin_id == plugin_id) {
        slots.push(Slot {
            kind: "sidebar",
            id: item.local_id.clone(),
            title: item.title.clone(),
        });
    }
    for panel in ui.panels.iter().filter(|s| s.plugin_id == plugin_id) {
        slots.push(Slot {
            kind: "panel",
            id: panel.local_id.clone(),
            title: panel.title.clone(),
        });
    }
    for item in ui.status_items.iter().filter(|s| s.plugin_id == plugin_id) {
        slots.push(Slot {
            kind: "status",
            id: item.local_id.clone(),
            title: item.format.clone(),
        });
    }
    for command in ui.commands.iter().filter(|s| s.plugin_id == plugin_id) {
        slots.push(Slot {
            kind: "command",
            id: command.local_id.clone(),
            title: command.title.clone(),
        });
    }
    slots
}

fn actions_of<'a>(state: &'a PluginState, plugin: &InstalledPlugin) -> Vec<&'a PluginAction> {
    state
        .actions
        .iter()
        .filter(|action| action.plugin_id == plugin.plugin_id)
        .collect()
}

fn logs_of<'a>(state: &'a PluginState, plugin: &InstalledPlugin) -> Vec<&'a PluginCommandLog> {
    state
        .logs
        .iter()
        .filter(|log| log.plugin_id == plugin.plugin_id)
        .collect()
}

fn contexts(action: &PluginAction) -> String {
    match action.contexts.as_deref() {
        None => Context::Global.as_str().to_owned(),
        Some(contexts) => contexts
            .iter()
            .map(|context| context.as_str())
            .collect::<Vec<_>>()
            .join(","),
    }
}

/// Where a plugin was linked from, as one line.
fn source_text(source: &PluginSource) -> String {
    match source.kind {
        SourceKind::Local => t("shell.manager.source_local"),
        SourceKind::Github => {
            let mut parts = vec!["GitHub".to_owned()];
            if let (Some(owner), Some(repo)) = (source.owner.as_deref(), source.repo.as_deref()) {
                parts.push(format!("{owner}/{repo}"));
            }
            match (
                source.requested_ref.as_deref(),
                source.resolved_commit.as_deref(),
            ) {
                (Some(reference), Some(commit)) if commit.starts_with(reference) => {
                    parts.push(format!("@{}", short(commit)));
                }
                (Some(reference), Some(commit)) => {
                    parts.push(format!("@{reference} ({})", short(commit)));
                }
                (Some(reference), None) => parts.push(format!("@{reference}")),
                (None, Some(commit)) => parts.push(format!("@{}", short(commit))),
                (None, None) => {}
            }
            if let Some(subdir) = source.subdir.as_deref() {
                parts.push(subdir.to_owned());
            }
            parts.join(" ")
        }
    }
}

fn short(commit: &str) -> String {
    commit.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str) -> InstalledPlugin {
        InstalledPlugin::fixture(id)
    }

    /// The drawer shows the plugin it was opened on, and a plugin unlinked
    /// while its page is open leaves nothing to show rather than a stale page.
    #[test]
    fn the_drawer_shows_the_plugin_it_was_opened_on_or_nothing() {
        let mut state = State::default();
        assert!(state.selected(&[]).is_none());
        state.open("a");
        let installed = vec![plugin("a"), plugin("b")];
        assert_eq!(
            state.selected(&installed).map(|p| p.plugin_id.as_str()),
            Some("a")
        );
        assert!(state.shows("a"));
        assert!(!state.shows("b"));
        state.set_tab(Tab::Logs);
        assert_eq!(state.tab(), Tab::Logs);
        state.open("b");
        assert_eq!(
            state.tab(),
            Tab::Overview,
            "a new page opens on the overview"
        );
        assert_eq!(
            state.selected(&installed).map(|p| p.plugin_id.as_str()),
            Some("b")
        );
        state.close();
        assert!(state.selected(&installed).is_none());
        state.open("gone");
        assert!(state.selected(&installed).is_none());
    }

    /// The four pages are one set with distinct ids and resident labels.
    #[test]
    fn every_page_has_an_id_and_a_label() {
        let mut seen = Vec::new();
        for tab in Tab::ALL {
            assert!(!seen.contains(&tab.id()));
            seen.push(tab.id());
            let key = format!("shell.manager.tab.{}", tab.id());
            assert_ne!(t(&key), key, "{key} resolves through the locale file");
        }
        assert_eq!(Tab::ALL.len(), 4);
    }

    /// A plugin's actions, panes and logs are the ones whose plugin id matches;
    /// the drawer must not borrow another plugin's rows.
    #[test]
    fn only_the_plugins_own_rows_are_collected() {
        let state = PluginState {
            actions: vec![
                PluginAction {
                    plugin_id: "a".into(),
                    action_id: "one".into(),
                    title: "One".into(),
                    description: None,
                    contexts: None,
                },
                PluginAction {
                    plugin_id: "b".into(),
                    action_id: "two".into(),
                    title: "Two".into(),
                    description: None,
                    contexts: None,
                },
            ],
            logs: vec![
                serde_json::from_value(serde_json::json!({
                    "log_id": "l1", "plugin_id": "a", "action_id": "one",
                    "command": ["true"], "status": "succeeded", "exit_code": 0,
                }))
                .expect("a log"),
                serde_json::from_value(serde_json::json!({
                    "log_id": "l2", "plugin_id": "b", "command": ["true"],
                    "status": "failed", "exit_code": 1,
                }))
                .expect("a log"),
            ],
            ..PluginState::default()
        };
        let a = plugin("a");
        assert_eq!(actions_of(&state, &a).len(), 1);
        assert_eq!(logs_of(&state, &a).len(), 1);
        let latest = last_status(&logs_of(&state, &a), actions_of(&state, &a)[0])
            .expect("the action ran once");
        assert_eq!(latest, ("succeeded".to_owned(), false));
        // An action that never ran has no status word at all.
        let never = PluginAction {
            plugin_id: "a".into(),
            action_id: "never".into(),
            title: "Never".into(),
            description: None,
            contexts: None,
        };
        assert!(last_status(&logs_of(&state, &a), &never).is_none());
    }

    /// The source line keeps the commit short and names the ref when the two
    /// differ; a local plugin says so in the locale's words.
    #[test]
    fn a_source_line_names_the_coordinates() {
        let github = PluginSource {
            kind: SourceKind::Github,
            owner: Some("acme".into()),
            repo: Some("board".into()),
            requested_ref: Some("main".into()),
            resolved_commit: Some("0123456789abcdef".into()),
            subdir: None,
        };
        assert_eq!(source_text(&github), "GitHub acme/board @main (0123456)");
        let local = PluginSource::default();
        assert_eq!(source_text(&local), t("shell.manager.source_local"));
    }
}
