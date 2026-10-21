//! The plugin host's own contributions: what is installed, and what the daemon
//! last ran for it.
//!
//! They mount through the shell's registry exactly as the shell's own sidebar
//! blocks do, so nothing here is reached by name from the shell's render
//! (PLAN.md §5.4). Both blocks stay out of the sidebar entirely while there is
//! nothing to say — no plugin linked, nothing run — which is the state of a
//! client that has never touched a plugin.

use gpui_kit::component::{Theme, h_flex, v_flex};
use gpui_kit::*;

use super::actions::Catalog;
use super::host::PluginState;
use super::model::{InstalledPlugin, LogStatus, OwnedPane, PluginCommandLog};
use crate::events::{self, Event};
use crate::mcp::handler::McpToolHandler;
use crate::shell::commands::COMMANDS;
use crate::shell::registry::{Invocation, ShellRequest, SidebarBlock, SidebarView};

/// The sidebar's plugin blocks, each naming the rail view it belongs to. The
/// plugin host lives in the plugin view; the event stream beside it is the
/// events view's block.
pub(crate) const PANELS: &[SidebarBlock] = &[
    SidebarBlock {
        view: SidebarView::Plugins,
        render: render_plugins,
    },
    SidebarBlock {
        view: SidebarView::Plugins,
        render: render_logs,
    },
    SidebarBlock {
        view: SidebarView::Events,
        render: render_events,
    },
];

/// How many log entries the sidebar shows. The daemon keeps a 200-entry ring
/// for the whole session; the tail is what a glance needs, and the full ring
/// belongs to a log surface of its own.
const LOGS_SHOWN: usize = 5;

/// How many events the sidebar shows. The stream keeps more ([`crate::events::RING`]);
/// this is the part that fits beside the panels.
const EVENTS_SHOWN: usize = 8;

/// What is linked, and how much of this client the plugins fill. A row opens the
/// plugin's own page in the manager: the actions it offers, the panes it declares
/// and the commands it would run are one plugin's detail, not four lines under
/// every name here.
fn render_plugins(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let state = invocation.plugins;
    if state.plugins.is_empty() && state.error.is_none() {
        return crate::shell::panels::empty_view(
            crate::i18n::t("shell.plugins.title"),
            crate::i18n::t("shell.views.empty.plugins"),
            theme,
        );
    }
    let mut column = v_flex().w_full().px_1().py_1().gap_0p5().child(header(
        "plugins-header",
        &crate::i18n::t("shell.plugins.title"),
        &crate::i18n::t("general.refresh"),
        ShellRequest::RefreshPlugins,
        None,
        invocation,
    ));
    if let Some(error) = state.error.as_deref() {
        column = column.child(line(error, theme.danger));
    }
    column = column.children(
        state
            .plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| plugin_row(index, plugin, invocation)),
    );
    // The panes this client opened, and the daemon still reports: the one list
    // that is about a running thing rather than a declaration.
    column = column.children(
        live_plugin_panes(state, invocation.snapshot)
            .into_iter()
            .enumerate()
            .map(|(index, pane)| owned_pane_row(index, pane, invocation)),
    );
    // What the plugins fill, slot by slot, beside what the shell fills itself.
    // The numbers are the registry's own: a plugin that was unlinked is in
    // neither half of any row.
    column = column.child(section(&crate::i18n::t("shell.manager.overview"), theme));
    column = column.children(inventory(state).into_iter().enumerate().map(
        |(index, (slot, builtin, plugins))| inventory_row(index, slot, builtin, plugins, theme),
    ));
    let open = {
        let dispatch = invocation.dispatch.clone();
        button(
            "plugins-manager",
            &crate::i18n::t("shell.manager.open"),
            theme,
        )
        .on_click(move |_, window, cx| dispatch(ShellRequest::OpenPluginManager(None), window, cx))
    };
    column.child(open).into_any_element()
}

/// One slot's tally: the shell's own contributions beside the plugins'.
type Inventory = Vec<(&'static str, usize, usize)>;

/// How many contributions each slot holds, the shell's own and the plugins'.
/// Every number is read off the same constants the registry mounts, so a row
/// cannot claim a slot that is not actually filled.
fn inventory(state: &PluginState) -> Inventory {
    let ui = &state.ui;
    // The palette's and the MCP bridge's plugin half is what can actually run,
    // so a disabled plugin counts nowhere until it is enabled again.
    let catalogued = Catalog::build(&state.actions, ui)
        .runnable(&state.plugins)
        .entries()
        .len();
    vec![
        (
            "sidebar",
            crate::shell::panels::PANELS.len() + PANELS.len(),
            ui.sidebar_items.len(),
        ),
        (
            "aside panel",
            crate::shell::chat::ASIDE.len() + crate::shell::cockpit::ASIDE.len(),
            ui.panels.len(),
        ),
        (
            "status segment",
            crate::shell::panels::STATUS_SEGMENTS.len(),
            ui.status_items.len(),
        ),
        (
            "settings page",
            crate::shell::settings::SETTINGS_SECTIONS.len(),
            0,
        ),
        ("command / palette", COMMANDS.len(), catalogued),
        ("MCP tool", McpToolHandler::build_tools().len(), catalogued),
    ]
}

/// Whether a plugin runs, as a dot: the same shape the agent rows use.
pub(crate) fn status_dot(theme: &Theme, enabled: bool) -> AnyElement {
    div()
        .flex_none()
        .w(px(8.))
        .h(px(8.))
        .rounded_full()
        .bg(if enabled {
            theme.success
        } else {
            theme.muted_foreground
        })
        .into_any_element()
}

/// A section label: dim, spaced off what comes before it.
pub(crate) fn section(label: &str, theme: &Theme) -> AnyElement {
    div()
        .w_full()
        .pt_1()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(label.to_owned())
        .into_any_element()
}

/// One inventory row: the slot, then `builtin + plugin`.
fn inventory_row(
    index: usize,
    slot: &str,
    builtin: usize,
    plugins: usize,
    theme: &Theme,
) -> AnyElement {
    h_flex()
        .id(("plugin-inventory", index))
        .w_full()
        .px_1()
        .py_0p5()
        .gap_1()
        .items_center()
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_xs()
                .text_color(theme.sidebar_foreground)
                .child((*slot).to_owned()),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(if plugins > 0 {
                    theme.magenta
                } else {
                    theme.muted_foreground
                })
                .child(format!("{builtin} + {plugins}")),
        )
        .into_any_element()
}

/// The daemon's plugin log ring, newest first. An invocation is fire-and-forget
/// — its answer says only `running` — so this block is where its outcome
/// appears, including the stdout or stderr of the command that failed.
fn render_logs(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let state = invocation.plugins;
    if state.logs.is_empty() && state.last.is_none() {
        return div().into_any_element();
    }
    let mut column = v_flex().w_full().px_1().py_1().gap_0p5().child(header(
        "plugin-logs-header",
        &crate::i18n::t("shell.plugins.logs"),
        &crate::i18n::t("general.refresh"),
        ShellRequest::RefreshPlugins,
        None,
        invocation,
    ));
    if let Some(last) = state.last.as_deref() {
        column = column.child(line(last, theme.muted_foreground));
    }
    column = column.children(
        state
            .logs
            .iter()
            .take(LOGS_SHOWN)
            .enumerate()
            .map(|(index, log)| log_row(index, log, invocation)),
    );
    column.into_any_element()
}

/// The daemon's event stream, and the hook chain each event starts.
///
/// This is the plugin author's debug surface. What a hook fires on, and what the
/// daemon ran when it did, are two separate answers — the manifest's
/// declarations are in the registry, the runs are in the log ring — and neither
/// of them says what actually happened, in what order. Beside a streamed event,
/// the plugins hooked on that very kind and the newest run the ring holds for
/// them are what make the chain readable.
fn render_events(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let state = invocation.events;
    if state.events.is_empty() && state.error.is_none() {
        // Nothing has happened yet and the stream is healthy: the view still
        // says so, because the rail can put it in front of the user.
        return crate::shell::panels::empty_view(
            crate::i18n::t("shell.plugins.events"),
            crate::i18n::t("shell.views.empty.events"),
            theme,
        );
    }
    let mut column = v_flex().w_full().px_1().py_1().gap_0p5().child(header(
        "plugin-events-header",
        &crate::i18n::t("shell.plugins.events"),
        &crate::i18n::t("general.clear"),
        ShellRequest::ClearEvents,
        None,
        invocation,
    ));
    if let Some(error) = state.error.as_deref() {
        column = column.child(line(error, theme.danger));
    }
    let now = events::now_unix_ms();
    column = column.children(
        state
            .events
            .iter()
            .take(EVENTS_SHOWN)
            .enumerate()
            .map(|(index, event)| event_row(index, event, now, invocation)),
    );
    column.into_any_element()
}

/// One event: when it arrived, what it was, and what it was about — then, under
/// it, the plugins that hook this kind and how their last run for it ended.
fn event_row(index: usize, event: &Event, now: u64, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let mut row = v_flex()
        .id(("plugin-event", index))
        .w_full()
        .px_1()
        .py_0p5()
        .gap_0p5()
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(events::age(now, event.at_unix_ms)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.sidebar_foreground)
                        .child(event.kind.clone()),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(event.detail.clone()),
                ),
        );
    row = row.children(
        invocation
            .plugins
            .plugins
            .iter()
            .filter(|plugin| plugin.hooks_on(&event.kind))
            .map(|plugin| {
                line(
                    &chain(plugin, &event.kind, invocation.plugins),
                    theme.muted_foreground,
                )
            }),
    );
    row.into_any_element()
}

/// A hooked plugin and the outcome of its newest run for this event, as one
/// line. `startup` is the daemon's name for a hook that no event triggers, so it
/// never matches one of these.
fn chain(plugin: &InstalledPlugin, event: &str, state: &PluginState) -> String {
    let run = state
        .logs
        .iter()
        .find(|log| log.plugin_id == plugin.plugin_id && log.event.as_deref() == Some(event));
    match run {
        Some(log) => format!("→ {} · {}", plugin.plugin_id, log.outcome()),
        None => format!(
            "→ {} · {}",
            plugin.plugin_id,
            crate::i18n::t("shell.plugins.events-not-run")
        ),
    }
}

/// One plugin: its name, where it came from, and what the daemon said about it.
fn plugin_row(index: usize, plugin: &InstalledPlugin, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let plugin_id = plugin.plugin_id.clone();
    let slots = super::manager::slot_count(
        super::manager::declared_for(invocation.plugins, &plugin.plugin_id),
        &plugin.plugin_id,
    );
    let actions = invocation
        .plugins
        .actions
        .iter()
        .filter(|action| action.plugin_id == plugin.plugin_id)
        .count();
    let mut row =
        v_flex()
            .id(("plugin", index))
            .w_full()
            .px_1()
            .py_0p5()
            .gap_0p5()
            .rounded_sm()
            .cursor_pointer()
            .hover(|style| style.bg(theme.sidebar_accent))
            .on_click(move |_, window, cx| {
                dispatch(
                    ShellRequest::OpenPluginManager(Some(plugin_id.clone())),
                    window,
                    cx,
                );
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
                            .text_color(theme.sidebar_foreground)
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
                            .child(format!("{actions} actions")),
                    )
                    .children((slots > 0).then(|| {
                        div().flex_none().text_xs().text_color(theme.magenta).child(
                            rust_i18n::t!("shell.manager.ui_slots", count = slots).to_string(),
                        )
                    })),
            );
    for warning in &plugin.warnings {
        row = row.child(line(warning, theme.warning));
    }
    row.into_any_element()
}

/// A plugin pane this client opened and the daemon still reports. The daemon
/// exposes no ownership marker on a pane, so a pane is recognised here only
/// because this client opened it — and a pane that has since closed is dropped
/// rather than offered as a row that cannot work.
fn owned_pane_row(index: usize, pane: &OwnedPane, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let close = invocation.dispatch.clone();
    let pane_id = pane.pane_id.clone();
    let closing = pane.pane_id.clone();
    h_flex()
        .id(("plugin-owned-pane", index))
        .w_full()
        .px_1()
        .py_0p5()
        .gap_1()
        .items_center()
        .rounded_sm()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(ShellRequest::FocusPane(pane_id.clone()), window, cx);
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_sm()
                .text_color(theme.sidebar_foreground)
                .child(pane.title.clone()),
        )
        .child(
            div()
                .id(("plugin-owned-pane-close", index))
                .px_0p5()
                .text_xs()
                .text_color(theme.muted_foreground)
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    close(ShellRequest::ClosePluginPane(closing.clone()), window, cx);
                })
                .child("x"),
        )
        .into_any_element()
}

/// One line of a command's log: what ran, how it ended, and the first line of
/// what it said.
fn log_row(index: usize, log: &PluginCommandLog, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let color = match log.status {
        LogStatus::Running => theme.muted_foreground,
        LogStatus::Succeeded => theme.success,
        LogStatus::Failed => theme.danger,
    };
    v_flex()
        .id(("plugin-log", index))
        .w_full()
        .px_1()
        .py_0p5()
        .gap_0p5()
        .child(div().w_full().text_xs().text_color(color).child(log.what()))
        .child(line(&log.outcome(), theme.muted_foreground))
        .into_any_element()
}

/// The plugin panes this client opened that the snapshot still reports.
fn live_plugin_panes<'a>(
    state: &'a PluginState,
    snapshot: Option<&herdr_client::protocol::ClientShellSnapshot>,
) -> Vec<&'a OwnedPane> {
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    state
        .panes
        .iter()
        .filter(|pane| {
            snapshot
                .panes
                .iter()
                .any(|live| live.pane_id == pane.pane_id)
        })
        .collect()
}

/// A small button inside a block: the shape the review and the manager use for
/// everything they can be asked to do.
pub(crate) fn button(id: &'static str, label: &str, theme: &Theme) -> Stateful<Div> {
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

/// A block's title line, with an affordance on the right that asks for the
/// data to be read again. Nothing pushes a plugin change, so every block that
/// shows plugin state needs one. The identity is the caller's: a built-in block
/// and a declared panel both come through here.
pub(crate) fn header(
    id: impl Into<ElementId>,
    title: &str,
    action: &str,
    request: ShellRequest,
    extra: Option<AnyElement>,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let id = id.into();
    h_flex()
        .w_full()
        .px_1()
        .pb_1()
        .justify_between()
        .items_center()
        .child(
            h_flex()
                .min_w_0()
                .flex_1()
                .gap_1()
                .items_center()
                .children(extra)
                .child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(title.to_owned()),
                ),
        )
        .child(
            div()
                .id(id)
                .text_xs()
                .text_color(theme.muted_foreground)
                .cursor_pointer()
                .on_click(move |_, window, cx| dispatch(request.clone(), window, cx))
                .child(action.to_owned()),
        )
        .into_any_element()
}

/// One dim line of text inside a block.
pub(crate) fn line(text: &str, color: Hsla) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .text_xs()
        .text_color(color)
        .child(text.to_owned())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    use crate::plugins::model::{ManifestEventHook, ManifestPane, Placement};

    fn plugin(id: &str) -> InstalledPlugin {
        InstalledPlugin {
            plugin_id: id.into(),
            name: id.into(),
            version: "0.1.0".into(),
            plugin_root: "/tmp/plugin".into(),
            description: None,
            enabled: true,
            panes: vec![ManifestPane {
                id: "board".into(),
                title: "Project board".into(),
                placement: Placement::Overlay,
            }],
            events: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// The inventory is the registry's own tally: the shell's own count beside
    /// the plugins', and a plugin's half is exactly what its declarations fill.
    /// Unlinking a plugin must move its number to zero without touching the
    /// shell's.
    #[test]
    fn the_inventory_counts_the_shell_beside_the_plugins() {
        let empty = inventory(&PluginState::default());
        let builtin: Vec<usize> = empty.iter().map(|(_, builtin, _)| *builtin).collect();
        assert!(builtin.iter().all(|count| *count > 0), "{builtin:?}");
        assert!(empty.iter().all(|(_, _, plugins)| *plugins == 0));

        let declared = crate::plugins::ui_manifest::parse(
            "a",
            std::path::Path::new("/tmp/plugin"),
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
action = "one"
"#,
        );
        let state = PluginState {
            plugins: vec![plugin("a")],
            actions: vec![],
            ui: declared,
            ..PluginState::default()
        };
        let filled = inventory(&state);
        let plugins: Vec<usize> = filled.iter().map(|(_, _, plugins)| *plugins).collect();
        let before: Vec<usize> = empty.iter().map(|(_, _, plugins)| *plugins).collect();
        assert_eq!(plugins, vec![1, 1, 1, 0, 0, 0], "one declaration each");
        assert_eq!(before, vec![0; 6]);
        // The shell's own half does not move because a plugin declared something.
        let builtin_after: Vec<usize> = filled.iter().map(|(_, builtin, _)| *builtin).collect();
        assert_eq!(builtin_after, builtin);
    }

    /// A pane the daemon no longer reports is not offered: the only record of a
    /// plugin pane is this client's own, so it has to be pruned against the
    /// snapshot that is the truth.
    #[test]
    fn only_panes_the_snapshot_still_reports_are_listed() {
        let state = PluginState {
            panes: vec![
                OwnedPane {
                    pane_id: "w1:pA".into(),
                    plugin_id: "a".into(),
                    entrypoint: "board".into(),
                    title: "Project board".into(),
                },
                OwnedPane {
                    pane_id: "w1:pZ".into(),
                    plugin_id: "a".into(),
                    entrypoint: "board".into(),
                    title: "Project board".into(),
                },
            ],
            ..PluginState::default()
        };
        let snapshot = snapshot_with_pane("w1:pA");
        let live = live_plugin_panes(&state, Some(&snapshot));
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].pane_id, "w1:pA");
        assert!(live_plugin_panes(&state, None).is_empty());
    }

    /// The chain is one line: the plugin that hooks an event, and how its newest
    /// run *for that event* ended. A run for another event must not be borrowed
    /// to fill the line in — that would show an outcome that this event did not
    /// cause.
    #[test]
    fn a_hooked_event_shows_the_outcome_of_its_own_newest_run() {
        let mut hooked = plugin("a");
        hooked.events = vec![ManifestEventHook {
            on: "pane.exited".into(),
        }];
        let state = PluginState {
            logs: vec![
                log("a", Some("pane.exited"), LogStatus::Succeeded, Some(0)),
                log("a", Some("pane.created"), LogStatus::Failed, Some(3)),
            ],
            ..PluginState::default()
        };
        assert!(chain(&hooked, "pane.exited", &state).contains("exit 0"));
        let never_ran = chain(&hooked, "tab.created", &state);
        assert!(never_ran.starts_with("→ a · "), "{never_ran}");
        assert!(!never_ran.contains("exit"), "{never_ran}");
    }

    fn log(
        plugin_id: &str,
        event: Option<&str>,
        status: LogStatus,
        exit_code: Option<i32>,
    ) -> PluginCommandLog {
        PluginCommandLog {
            log_id: "plugin-log-1".into(),
            plugin_id: plugin_id.into(),
            action_id: None,
            event: event.map(str::to_owned),
            command: vec!["true".into()],
            status,
            exit_code,
            stdout: None,
            stderr: None,
            error: None,
        }
    }

    fn snapshot_with_pane(pane_id: &str) -> herdr_client::protocol::ClientShellSnapshot {
        serde_json::from_value(serde_json::json!({
            "boot_id": "boot",
            "revision": 1,
            "update_install_command": "",
            "latest_release_notes_available": false,
            "integration_updates_available": false,
            "worktree_directory": "",
            "tab_bar_right_separator": "",
            "tab_bar_right": [],
            "agent_order": [],
            "workspaces": [],
            "tabs": [],
            "panes": [{"pane_id": pane_id, "workspace_id": "w1", "tab_id": "w1:t1",
                       "focused": true, "right_click_passthrough": false}],
            "agents": [],
            "commands": [],
        }))
        .expect("the fixture snapshot deserializes")
    }
}
