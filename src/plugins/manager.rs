//! The plugin manager: every linked plugin, one at a time, and everything the
//! selected one contributes to this client.
//!
//! The sidebar's plugin view is the glance — what is installed and roughly what
//! it brings. This is the same registry read one plugin at a time: its
//! declaration, the slots it fills (each one carrying the same source badge the
//! shell draws), the actions it offers, the panes it declares, and what the
//! daemon last ran for it. What it changes, it changes through the daemon —
//! enable, disable and unlink are `plugin.*` calls, never a local filter, because
//! the daemon is what decides which contributions exist.
//!
//! The manager is laid over the shell and holds the keyboard while it is open,
//! like the review and the palette. It has no input of its own: Escape closes it
//! and every command is a click.

use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{ActiveTheme, Theme, h_flex, v_flex};
use gpui_kit::*;

use super::host::PluginState;
use super::model::{Availability, Context, InstalledPlugin, LogStatus, Placement, PluginAction};
use super::panels::{button, line, section, status_dot};
use super::ui_manifest::UiContributions;
use super::views::source_badge;
use crate::i18n::t;
use crate::shell::registry::{Dispatch, ShellRequest};

/// How many of the daemon's log entries one plugin's page shows. The ring is
/// the whole session's; a page is about one plugin's last few runs.
const LOGS_SHOWN: usize = 3;

/// The manager as the shell holds it: whether it is up, and which plugin's page
/// is in front.
#[derive(Default)]
pub(crate) struct Manager {
    open: bool,
    selected: Option<String>,
}

impl Manager {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Open on `start_at`, or on whatever was in front last time. A selected
    /// plugin that has since been unlinked falls back to the first one, which
    /// [`Manager::selected`] resolves — the registry, not this state, decides
    /// what exists.
    pub(crate) fn open(&mut self, start_at: Option<&str>) {
        self.open = true;
        if let Some(plugin_id) = start_at {
            self.selected = Some(plugin_id.to_owned());
        }
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    pub(crate) fn select(&mut self, plugin_id: &str) {
        self.selected = Some(plugin_id.to_owned());
    }

    /// The plugin whose page is shown: the selected one while it is still
    /// installed, otherwise the first.
    fn selected<'a>(&self, plugins: &'a [InstalledPlugin]) -> Option<&'a InstalledPlugin> {
        let selected = self
            .selected
            .as_deref()
            .and_then(|id| plugins.iter().find(|plugin| plugin.plugin_id == id));
        selected.or_else(|| plugins.first())
    }
}

/// The manager, laid over the shell. `key` is the shell's own handler, so a
/// keystroke means the same here as it does in the palette.
pub(crate) fn overlay(
    manager: &Manager,
    state: &PluginState,
    availability: Availability,
    focus: &FocusHandle,
    dispatch: &Dispatch,
    key: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let close = dispatch.clone();
    let mut boxed = v_flex()
        .id("plugin-manager")
        .w(px(940.))
        .h(px(600.))
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
                .child(div().flex_none().text_sm().child(t("shell.manager.title")))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("plugin.list"),
                )
                .child(div().flex_1())
                .child(
                    button("plugin-manager-trust", &t("shell.manager.review"), &theme).on_click({
                        let open = dispatch.clone();
                        move |_, window, cx| open(ShellRequest::OpenPluginTrust, window, cx)
                    }),
                )
                .child(
                    button("plugin-manager-close", &t("settings.close"), &theme).on_click({
                        let close = close.clone();
                        move |_, window, cx| close(ShellRequest::ClosePluginManager, window, cx)
                    }),
                ),
        );
    boxed = match manager.selected(&state.plugins) {
        None => boxed.child(
            div()
                .flex_1()
                .p_3()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(t("views.empty.plugins")),
        ),
        Some(plugin) => boxed.child(
            h_flex()
                .flex_1()
                .min_h_0()
                .items_start()
                .child(list(manager, state, &theme, dispatch))
                .child(detail(plugin, state, availability, &theme, dispatch)),
        ),
    };
    v_flex()
        .id("plugin-manager-overlay")
        .absolute()
        .inset_0()
        .occlude()
        .size_full()
        .items_center()
        .justify_center()
        // The shell stays visible behind it: a plugin is what it contributes to
        // what is already on screen.
        .bg(theme.background.opacity(0.72))
        .child(boxed)
        .into_any_element()
}

/// Every linked plugin, one row each: what it is, whether it runs, and how much
/// of this client it fills. The row that is selected is the page beside it.
fn list(manager: &Manager, state: &PluginState, theme: &Theme, dispatch: &Dispatch) -> AnyElement {
    let selected = manager
        .selected(&state.plugins)
        .map(|p| p.plugin_id.clone());
    let rows = state.plugins.iter().enumerate().map(|(index, plugin)| {
        let run = dispatch.clone();
        let plugin_id = plugin.plugin_id.clone();
        let ui = slots_of(&state.ui, &plugin.plugin_id).len();
        let actions = state
            .actions
            .iter()
            .filter(|action| action.plugin_id == plugin.plugin_id)
            .count();
        let on = selected.as_deref() == Some(plugin.plugin_id.as_str());
        let mut row = v_flex()
            .id(("plugin-manager-row", index))
            .w_full()
            .px_2()
            .py_1()
            .gap_0p5()
            .cursor_pointer();
        if on {
            row = row.bg(theme.secondary);
        }
        row.on_click(move |_, window, cx| {
            run(ShellRequest::SelectPlugin(plugin_id.clone()), window, cx);
        })
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(status_dot(theme, plugin.enabled))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_sm()
                        .child(plugin.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(plugin.version.clone()),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(plugin.plugin_id.clone()),
                )
                .children((ui > 0).then(|| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.magenta)
                        .child(format!("UI {ui}"))
                }))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("{actions} actions")),
                ),
        )
    });
    v_flex()
        .id("plugin-manager-list")
        .w(px(260.))
        .h_full()
        .flex_none()
        .py_1()
        .gap_0p5()
        .border_r_1()
        .border_color(theme.border)
        .overflow_y_scrollbar()
        .children(rows)
        .into_any_element()
}

/// One plugin's page: the four things that can be done to it, under everything
/// it is about. The buttons sit outside the scrolling body, so a page whose
/// declarations run past the dialog's height still ends with them.
fn detail(
    plugin: &InstalledPlugin,
    state: &PluginState,
    availability: Availability,
    theme: &Theme,
    dispatch: &Dispatch,
) -> AnyElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .child(body(plugin, state, availability, theme, dispatch))
        .child(footer(plugin, theme, dispatch))
        .into_any_element()
}

/// Everything the page says about one plugin: whose it is, what it declares,
/// what it offers, and what the daemon last ran for it.
fn body(
    plugin: &InstalledPlugin,
    state: &PluginState,
    availability: Availability,
    theme: &Theme,
    dispatch: &Dispatch,
) -> AnyElement {
    let enabled = plugin.enabled;
    let ui = slots_of(declared_for(state, &plugin.plugin_id), &plugin.plugin_id);
    let actions: Vec<&PluginAction> = state
        .actions
        .iter()
        .filter(|action| action.plugin_id == plugin.plugin_id)
        .collect();
    let logs: Vec<_> = state
        .logs
        .iter()
        .filter(|log| log.plugin_id == plugin.plugin_id)
        .take(LOGS_SHOWN)
        .collect();
    let mut column = v_flex()
        .id("plugin-manager-detail")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .w_full()
        .p_2()
        .gap_1()
        .overflow_y_scrollbar()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(div().text_sm().child(plugin.name.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(plugin.version.clone()),
                )
                .child(source_badge(theme, &plugin.plugin_id)),
        )
        .child(line(&plugin.plugin_root, theme.muted_foreground));
    if let Some(description) = plugin.description.as_deref() {
        column = column.child(line(description, theme.sidebar_foreground));
    }
    column = column.child(line(
        &format!(
            "{} · {} actions · {} panes · {} logs",
            if enabled {
                t("shell.manager.enabled")
            } else {
                t("shell.manager.disabled")
            },
            actions.len(),
            plugin.panes.len(),
            state
                .logs
                .iter()
                .filter(|log| log.plugin_id == plugin.plugin_id)
                .count()
        ),
        theme.muted_foreground,
    ));
    column = column.child(line(
        &if ui.is_empty() {
            t("shell.manager.no_ui").to_string()
        } else {
            rust_i18n::t!("shell.manager.declares", count = ui.len()).to_string()
        },
        theme.muted_foreground,
    ));
    for warning in &plugin.warnings {
        column = column.child(line(warning, theme.warning));
    }

    // The slots this plugin fills, each named the way the shell names it, so a
    // badge seen on screen can be found here.
    if !ui.is_empty() {
        column = column.child(section(&t("shell.manager.slots"), theme));
        column = column.children(
            ui.iter()
                .map(|slot| slot_row(slot, theme))
                .collect::<Vec<_>>(),
        );
    }

    column = column.child(section(&t("shell.manager.actions"), theme));
    if actions.is_empty() {
        column = column.child(line(&t("shell.manager.no_actions"), theme.muted_foreground));
    }
    if !plugin.enabled {
        column = column.child(line(&t("shell.manager.actions_disabled"), theme.warning));
    }
    column = column.children(actions.iter().enumerate().map(|(index, action)| {
        let run = dispatch.clone();
        let cloned = (*action).clone();
        // Offered here means runnable *and* in scope for the current focus: the
        // daemon refuses an action of a disabled plugin, so a row that cannot
        // run is not a row that does nothing when clicked.
        let offered = plugin.enabled && action.is_available(availability);
        let mut row = h_flex()
            .id(("plugin-manager-action", index))
            .w_full()
            .px_1()
            .py_0p5()
            .gap_1()
            .items_center()
            .rounded_sm();
        if offered {
            row = row
                .cursor_pointer()
                .hover(|style| style.bg(theme.secondary))
                .on_click(move |_, window, cx| {
                    run(
                        ShellRequest::PluginAction {
                            action: cloned.clone(),
                            source: "manager",
                        },
                        window,
                        cx,
                    );
                });
        }
        row.child(
            div()
                .min_w_0()
                .flex_1()
                .text_sm()
                .text_color(if offered {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .child(action.title.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(contexts(action)),
        )
    }));

    if !plugin.panes.is_empty() {
        column = column.child(section(&t("shell.manager.panes"), theme));
        column = column.children(plugin.panes.iter().enumerate().map(|(index, pane)| {
            let open = dispatch.clone();
            let plugin_id = plugin.plugin_id.clone();
            let entrypoint = pane.id.clone();
            let title = pane.title.clone();
            let placement = pane.placement;
            h_flex()
                .id(("plugin-manager-pane", index))
                .w_full()
                .px_1()
                .py_0p5()
                .gap_1()
                .items_center()
                .rounded_sm()
                .cursor_pointer()
                .hover(|style| style.bg(theme.secondary))
                .on_click(move |_, window, cx| {
                    open(
                        ShellRequest::OpenPluginPane {
                            plugin_id: plugin_id.clone(),
                            entrypoint: entrypoint.clone(),
                            title: title.clone(),
                            placement,
                        },
                        window,
                        cx,
                    );
                })
                .child(div().min_w_0().flex_1().text_sm().child(pane.title.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(placement_label(pane.placement)),
                )
        }));
    }

    if !logs.is_empty() {
        column = column.child(section(&t("shell.plugins.logs"), theme));
        column = column.children(logs.iter().map(|log| {
            let status = log.status.label();
            let color = if log.status == LogStatus::Failed {
                theme.danger
            } else {
                theme.muted_foreground
            };
            line(&format!("{} · {}", status, log.command.join(" ")), color)
        }));
    }

    column.into_any_element()
}

/// The four things a plugin's page can be asked to do. Every one of them is the
/// daemon's: enable, disable and unlink are `plugin.*` calls, and reviewing the
/// commands is the same read as before a link.
fn footer(plugin: &InstalledPlugin, theme: &Theme, dispatch: &Dispatch) -> AnyElement {
    let plugin_id = plugin.plugin_id.clone();
    let root = plugin.plugin_root.clone();
    let enabled = plugin.enabled;
    h_flex()
        .w_full()
        .flex_none()
        .pt_1()
        .gap_2()
        .items_center()
        .child(
            button("plugin-manager-refresh", &t("general.refresh"), theme).on_click({
                let refresh = dispatch.clone();
                move |_, window, cx| refresh(ShellRequest::RefreshPlugins, window, cx)
            }),
        )
        // Reviewing a linked plugin is the same read as before a link: the
        // directory is still a list of commands this client will run.
        .child(
            button("plugin-manager-inspect", &t("shell.manager.argv"), theme).on_click({
                let review = dispatch.clone();
                move |_, window, cx| {
                    review(ShellRequest::OpenPluginTrustAt(root.clone()), window, cx)
                }
            }),
        )
        .child(
            button(
                "plugin-manager-toggle",
                &if enabled {
                    t("shell.manager.disable")
                } else {
                    t("shell.manager.enable")
                },
                theme,
            )
            .on_click({
                let run = dispatch.clone();
                let plugin_id = plugin_id.clone();
                move |_, window, cx| {
                    run(
                        ShellRequest::SetPluginEnabled {
                            plugin_id: plugin_id.clone(),
                            enabled: !enabled,
                        },
                        window,
                        cx,
                    )
                }
            }),
        )
        .child(
            button("plugin-manager-unlink", &t("shell.manager.unlink"), theme).on_click({
                let run = dispatch.clone();
                let plugin_id = plugin_id.clone();
                move |_, window, cx| run(ShellRequest::UnlinkPlugin(plugin_id.clone()), window, cx)
            }),
        )
        .into_any_element()
}

/// One slot a plugin declared, named as the shell's own contribution ids are:
/// the kind, the local id, and what the user sees.
pub(crate) struct Slot<'a> {
    kind: &'static str,
    id: &'a str,
    title: String,
}

/// What one plugin declares for this client, enabled or not. The mounted set is
/// only the enabled half, and a page is about the plugin rather than about what
/// is currently on screen.
pub(crate) fn declared_for<'a>(state: &'a PluginState, plugin_id: &str) -> &'a UiContributions {
    state
        .declared
        .iter()
        .find(|entry| entry.plugin_id == plugin_id)
        .map(|entry| &entry.ui)
        .unwrap_or(&state.ui)
}

/// How many slots one plugin fills, which is what its row's badge counts.
pub(crate) fn slot_count(ui: &UiContributions, plugin_id: &str) -> usize {
    slots_of(ui, plugin_id).len()
}

/// The slots one plugin fills. Read off the declarations the shell mounted, so a
/// slot is listed exactly while a badge for it can be drawn.
pub(crate) fn slots_of<'a>(ui: &'a UiContributions, plugin_id: &str) -> Vec<Slot<'a>> {
    let mut slots = Vec::new();
    for item in ui.sidebar_items.iter().filter(|s| s.plugin_id == plugin_id) {
        slots.push(Slot {
            kind: "sidebar",
            id: &item.local_id,
            title: item.title.clone(),
        });
    }
    for panel in ui.panels.iter().filter(|s| s.plugin_id == plugin_id) {
        slots.push(Slot {
            kind: "panel",
            id: &panel.local_id,
            title: panel.title.clone(),
        });
    }
    for item in ui.status_items.iter().filter(|s| s.plugin_id == plugin_id) {
        slots.push(Slot {
            kind: "status",
            id: &item.local_id,
            title: item.format.clone(),
        });
    }
    for command in ui.commands.iter().filter(|s| s.plugin_id == plugin_id) {
        slots.push(Slot {
            kind: "command",
            id: &command.local_id,
            title: command.title.clone(),
        });
    }
    slots
}

pub(crate) fn slot_row(slot: &Slot<'_>, theme: &Theme) -> AnyElement {
    h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .child(
            div()
                .flex_none()
                .w(px(64.))
                .text_xs()
                .text_color(theme.magenta)
                .child(slot.kind),
        )
        .child(
            div()
                .flex_none()
                .w(px(120.))
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(slot.id.to_owned()),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_xs()
                .text_color(theme.foreground)
                .child(slot.title.clone()),
        )
        .into_any_element()
}

/// The contexts an action says it wants, in the daemon's own words. An action
/// that names none is global, which is the daemon's meaning for an absent key.
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

fn placement_label(placement: Placement) -> &'static str {
    placement.as_str()
}

/// The manager's own glyph, for a caller that wants to label the way in.
pub(crate) fn icon() -> &'static [u8] {
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 4h3a2 2 0 0 1 2 2v1.5a1.5 1.5 0 1 0 3 0V6h1a2 2 0 0 1 2 2v3h-1.5a1.5 1.5 0 1 0 0 3H20v3a2 2 0 0 1-2 2h-3v-1.5a1.5 1.5 0 1 0-3 0V19H9a2 2 0 0 1-2-2v-3H5.5a1.5 1.5 0 1 1 0-3H7V6a2 2 0 0 1 2-2Z"/></svg>"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    fn plugin(id: &str) -> InstalledPlugin {
        InstalledPlugin {
            plugin_id: id.into(),
            name: id.into(),
            version: "0.1.0".into(),
            plugin_root: "/tmp/plugin".into(),
            description: None,
            enabled: true,
            panes: Vec::new(),
            events: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// The selected page follows the registry: a plugin that was unlinked leaves
    /// the page it was on to the first one still installed, because the daemon,
    /// not this state, decides what exists.
    #[test]
    fn a_page_falls_back_when_the_plugin_it_showed_is_gone() {
        let mut manager = Manager::default();
        let installed = vec![plugin("a"), plugin("b")];
        assert_eq!(
            manager.selected(&installed).map(|p| p.plugin_id.as_str()),
            Some("a"),
            "nothing selected yet shows the first"
        );
        manager.select("b");
        assert_eq!(
            manager.selected(&installed).map(|p| p.plugin_id.as_str()),
            Some("b")
        );
        manager.select("gone");
        assert_eq!(
            manager.selected(&installed).map(|p| p.plugin_id.as_str()),
            Some("a"),
            "a page cannot outlive the plugin it describes"
        );
        assert!(manager.selected(&[]).is_none());
    }

    /// Every slot a plugin declared is named once, under the kind the shell
    /// mounts it as — the same four collections its badges are drawn from.
    #[test]
    fn every_declared_slot_is_named_once() {
        let ui = UiContributions::parse(
            "board.app",
            std::path::Path::new("/tmp/board"),
            r#"
[[sidebar_item]]
id = "board"
title = "Project board"
panel = "board"

[[panel]]
id = "board"
title = "Project board"
placement = "drawer"
view = "table"

[panel.source]
command = ["true"]

[[status_item]]
id = "wip"
source = { command = ["true"] }
format = "{count} WIP"

[[command]]
id = "sync"
title = "Sync"
action = "sync"
"#,
        );
        let slots = slots_of(&ui, "board.app");
        let named: Vec<(&str, &str)> = slots.iter().map(|slot| (slot.kind, slot.id)).collect();
        assert_eq!(
            named,
            vec![
                ("sidebar", "board"),
                ("panel", "board"),
                ("status", "wip"),
                ("command", "sync"),
            ]
        );
        assert_eq!(slot_count(&ui, "board.app"), 4);
        assert_eq!(slot_count(&ui, "other.app"), 0);
    }
}
