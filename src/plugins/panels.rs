//! The plugin host's own contributions: what is installed, and what the daemon
//! last ran for it.
//!
//! They mount through the shell's registry exactly as the shell's own sidebar
//! blocks do, so nothing here is reached by name from the shell's render
//! (PLAN.md §5.4). Both blocks stay out of the sidebar entirely while there is
//! nothing to say — no plugin linked, nothing run — which is the state of a
//! client that has never touched a plugin.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::host::PluginState;
use super::model::{
    Availability, Context, InstalledPlugin, LogStatus, ManifestPane, OwnedPane, PluginAction,
    PluginCommandLog,
};
use crate::shell::registry::{Contribution, Invocation, ShellRequest};

/// The sidebar's plugin blocks, mounted below the shell's own.
pub(crate) const PANELS: &[Contribution] = &[
    Contribution {
        id: "plugins",
        render: render_plugins,
    },
    Contribution {
        id: "plugin-logs",
        render: render_logs,
    },
];

/// How many log entries the sidebar shows. The daemon keeps a 200-entry ring
/// for the whole session; the tail is what a glance needs, and the full ring
/// belongs to a log surface of its own.
const LOGS_SHOWN: usize = 5;

/// What is linked: each plugin, the actions it offers here, and the panes it
/// declared. A click runs an action, or opens a pane, with the context the
/// daemon completes.
fn render_plugins(invocation: &Invocation, _: &mut Window, _: &mut App) -> AnyElement {
    let theme = invocation.theme;
    let state = invocation.plugins;
    if state.plugins.is_empty() && state.error.is_none() {
        return div().into_any_element();
    }
    let availability = Availability::of(invocation.snapshot);
    let mut column = v_flex().w_full().px_1().py_1().gap_0p5().child(header(
        "plugins-header",
        "Plugins",
        "refresh",
        ShellRequest::RefreshPlugins,
        invocation,
    ));
    if let Some(error) = state.error.as_deref() {
        column = column.child(line(error, theme.danger));
    }
    // One counter across every row keeps the element ids unique without the
    // index having to mean anything.
    let mut index = 0usize;
    for plugin in &state.plugins {
        column = column.child(plugin_row(index, plugin, invocation));
        index += 1;
        for action in available_actions(state, plugin, availability) {
            column = column.child(action_row(index, action, invocation));
            index += 1;
        }
        for pane in &plugin.panes {
            column = column.child(pane_row(index, plugin, pane, invocation));
            index += 1;
        }
    }
    for pane in live_plugin_panes(state, invocation.snapshot) {
        column = column.child(owned_pane_row(index, pane, invocation));
        index += 1;
    }
    column.into_any_element()
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
        "Plugin logs",
        "refresh",
        ShellRequest::RefreshPlugins,
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

/// One plugin: its name, where it came from, and what the daemon said about it.
fn plugin_row(index: usize, plugin: &InstalledPlugin, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let mut detail = plugin.plugin_id.clone();
    if !plugin.enabled {
        detail.push_str(" · disabled");
    }
    let mut row = v_flex()
        .id(("plugin", index))
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
                        .min_w_0()
                        .flex_1()
                        .text_sm()
                        .text_color(theme.foreground)
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
        .child(line(&detail, theme.muted_foreground));
    for warning in &plugin.warnings {
        row = row.child(line(warning, theme.warning));
    }
    row.into_any_element()
}

/// One action the plugin offers in the current state. Clicking it runs it.
fn action_row(index: usize, action: &PluginAction, invocation: &Invocation) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let title = action.title.clone();
    let context = context_label(action);
    let cloned = action.clone();
    let row = h_flex()
        .id(("plugin-action", index))
        .w_full()
        .px_1()
        .py_0p5()
        .gap_1()
        .items_center()
        .rounded_sm()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(
                ShellRequest::PluginAction {
                    action: cloned.clone(),
                    source: "sidebar",
                },
                window,
                cx,
            );
        })
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_sm()
                .text_color(theme.sidebar_foreground)
                .child(title),
        );
    if context.is_empty() {
        row.into_any_element()
    } else {
        row.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(context),
        )
        .into_any_element()
    }
}

/// One pane a plugin declared. Clicking it opens that pane at its placement.
fn pane_row(
    index: usize,
    plugin: &InstalledPlugin,
    pane: &ManifestPane,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    let (plugin_id, entrypoint, title) = (
        plugin.plugin_id.clone(),
        pane.id.clone(),
        pane.title.clone(),
    );
    let placement = pane.placement;
    h_flex()
        .id(("plugin-pane", index))
        .w_full()
        .px_1()
        .py_0p5()
        .gap_1()
        .items_center()
        .rounded_sm()
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            dispatch(
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
                .flex_none()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(placement.as_str()),
        )
        .into_any_element()
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

/// The actions one plugin offers in the current state, in the daemon's order.
fn available_actions<'a>(
    state: &'a PluginState,
    plugin: &InstalledPlugin,
    availability: Availability,
) -> Vec<&'a PluginAction> {
    state
        .actions
        .iter()
        .filter(|action| action.plugin_id == plugin.plugin_id && action.is_available(availability))
        .collect()
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

/// What an action asks for, as the dim suffix on its row: the contexts the
/// daemon would offer it in. A global action has none to show.
fn context_label(action: &PluginAction) -> String {
    let Some(contexts) = &action.contexts else {
        return String::new();
    };
    contexts
        .iter()
        .filter(|context| **context != Context::Global)
        .map(|context| context.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// A block's title line, with an affordance on the right that asks for the
/// data to be read again. Nothing pushes a plugin change, so every block that
/// shows plugin state needs one.
fn header(
    id: &'static str,
    title: &'static str,
    action: &'static str,
    request: ShellRequest,
    invocation: &Invocation,
) -> AnyElement {
    let theme = invocation.theme;
    let dispatch = invocation.dispatch.clone();
    h_flex()
        .w_full()
        .px_1()
        .pb_1()
        .justify_between()
        .items_center()
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(title),
        )
        .child(
            div()
                .id(id)
                .text_xs()
                .text_color(theme.muted_foreground)
                .cursor_pointer()
                .on_click(move |_, window, cx| dispatch(request.clone(), window, cx))
                .child(action),
        )
        .into_any_element()
}

/// One dim line of text inside a block.
fn line(text: &str, color: Hsla) -> AnyElement {
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

    use crate::plugins::model::Placement;

    fn plugin(id: &str) -> InstalledPlugin {
        InstalledPlugin {
            plugin_id: id.into(),
            name: id.into(),
            version: "0.1.0".into(),
            description: None,
            enabled: true,
            panes: vec![ManifestPane {
                id: "board".into(),
                title: "Project board".into(),
                placement: Placement::Overlay,
            }],
            warnings: Vec::new(),
        }
    }

    fn action(plugin_id: &str, action_id: &str, contexts: Option<Vec<Context>>) -> PluginAction {
        PluginAction {
            plugin_id: plugin_id.into(),
            action_id: action_id.into(),
            title: action_id.into(),
            description: None,
            contexts,
        }
    }

    #[test]
    fn only_the_named_plugins_actions_are_offered() {
        let state = PluginState {
            plugins: vec![plugin("a"), plugin("b")],
            actions: vec![
                action("a", "one", None),
                action("b", "two", None),
                action("c", "three", None),
            ],
            ..PluginState::default()
        };
        let offered = available_actions(&state, &state.plugins[0], Availability::default());
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].action_id, "one");
    }

    /// The sidebar shows what the current focus can run: an action for a
    /// workspace is hidden while the daemon reports no focused workspace.
    #[test]
    fn actions_follow_the_availability_of_the_focused_state() {
        let state = PluginState {
            plugins: vec![plugin("a")],
            actions: vec![
                action("a", "global", None),
                action("a", "per-workspace", Some(vec![Context::Workspace])),
            ],
            ..PluginState::default()
        };
        let offered = available_actions(&state, &state.plugins[0], Availability::default());
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].action_id, "global");
        let offered = available_actions(
            &state,
            &state.plugins[0],
            Availability {
                workspace: true,
                ..Availability::default()
            },
        );
        assert_eq!(offered.len(), 2);
    }

    #[test]
    fn a_global_action_shows_no_context_suffix() {
        assert_eq!(context_label(&action("a", "x", None)), "");
        assert_eq!(
            context_label(&action("a", "x", Some(vec![Context::Global]))),
            ""
        );
        assert_eq!(
            context_label(&action(
                "a",
                "x",
                Some(vec![Context::Global, Context::Pane, Context::Selection])
            )),
            "pane,selection"
        );
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
