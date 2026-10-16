//! The daemon's `plugin.*` methods, over the JSON API lane.
//!
//! No `plugin.*` method is on the endpoint lane at all: the 40-method whitelist
//! the daemon advertises to a GUI client is fixed and carries none of them, so
//! the whole plugin surface — the registry, the actions, the log ring and the
//! panes — is reached over `herdr.sock` beside it. A plugin *pane* then becomes
//! ordinary daemon state and arrives back on the endpoint lane as a snapshot
//! and a surface push, which is why this module only ever sends.
//!
//! Measured against herdr 0.9.5 in an isolated session; the evidence, including
//! the reply shapes these decoders are written against, is in
//! `.relay/journal/HANDOFF-014.md`.

use serde_json::json;

use super::model::{InstalledPlugin, OpenedPane, Placement, PluginAction, PluginCommandLog};
use crate::herdr::api::{ApiClient, ApiError, decode_field, required_field};

/// How many log entries a refresh asks for. The daemon keeps a 200-entry
/// in-memory ring and answers with the newest `limit` of it, oldest first; the
/// panel shows a handful, and the extra room is what lets an invocation's
/// outcome appear beside the runs before it. Asking for more than 200 would
/// silently get 200 back; asking for none behaves like asking for one.
pub(crate) const LOG_LIMIT: usize = 20;

/// Every installed plugin, enabled or not, with its manifest's declarations and
/// the warnings the daemon collected for it.
pub(crate) fn list_plugins(client: &ApiClient) -> Result<Vec<InstalledPlugin>, ApiError> {
    let result = client.call("plugin.list", json!({}))?;
    decode_field(&result, "plugins", Vec::new())
}

/// Every action every installed plugin declares, whether or not the daemon
/// would run it here.
pub(crate) fn list_actions(client: &ApiClient) -> Result<Vec<PluginAction>, ApiError> {
    let result = client.call("plugin.action.list", json!({}))?;
    decode_field(&result, "actions", Vec::new())
}

/// The daemon's recent plugin commands, oldest first.
pub(crate) fn list_logs(
    client: &ApiClient,
    limit: usize,
) -> Result<Vec<PluginCommandLog>, ApiError> {
    let result = client.call("plugin.log.list", json!({"limit": limit}))?;
    decode_field(&result, "logs", Vec::new())
}

/// Run one action. The call returns as soon as the daemon has spawned the
/// command — its answer's log entry is `running` — so the outcome is read from
/// [`list_logs`] later, never from here.
///
/// Only the invocation source is sent as context. The daemon assembles the
/// whole invocation context from its own live state and takes a supplied field
/// only as an override, so naming the focused ids here would be a second copy
/// of the truth that can disagree with it.
pub(crate) fn invoke_action(
    client: &ApiClient,
    action: &PluginAction,
    source: &str,
) -> Result<PluginCommandLog, ApiError> {
    let result = client.call(
        "plugin.action.invoke",
        json!({
            "action_id": action.action_id,
            "plugin_id": action.plugin_id,
            "context": {"invocation_source": source},
        }),
    )?;
    required_field(&result, "log")
}

/// Open one of a plugin's declared panes. `Ok(None)` is the daemon's answer for
/// a popup, which is not a pane and reports no id.
pub(crate) fn open_pane(
    client: &ApiClient,
    plugin_id: &str,
    entrypoint: &str,
    placement: Placement,
) -> Result<Option<OpenedPane>, ApiError> {
    let result = client.call(
        "plugin.pane.open",
        json!({
            "plugin_id": plugin_id,
            "entrypoint": entrypoint,
            "placement": placement.as_str(),
            "focus": true,
        }),
    )?;
    decode_field(&result, "plugin_pane", None)
}

/// Close a plugin pane. Only a pane the plugin surface owns can be closed this
/// way; the daemon answers `plugin_pane_not_found` for anything else, including
/// a pane that was already closed.
pub(crate) fn close_pane(client: &ApiClient, pane_id: &str) -> Result<(), ApiError> {
    client.call("plugin.pane.close", json!({"pane_id": pane_id}))?;
    Ok(())
}

/// Close the popup pane the daemon has open, if any. A popup is a singleton
/// terminal the daemon owns rather than a pane, so this is the only way to
/// dismiss one from outside the plugin.
pub(crate) fn close_popup(client: &ApiClient) -> Result<(), ApiError> {
    client.call("popup.close", json!({}))?;
    Ok(())
}

/// The pane id an open reported.
pub(crate) fn opened_pane_id(opened: &OpenedPane) -> &str {
    opened.pane.pane_id.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::model::LogStatus;

    #[test]
    fn an_open_reply_for_a_popup_has_no_pane() {
        let result = json!({"type": "ok"});
        let opened: Option<OpenedPane> = decode_field(&result, "plugin_pane", None).expect("ok");
        assert!(opened.is_none());
    }

    #[test]
    fn an_open_reply_carries_the_pane_the_daemon_made() {
        let result = json!({
            "type": "plugin_pane_opened",
            "plugin_pane": {
                "plugin_id": "zenix.m8probe",
                "entrypoint": "p-overlay",
                "pane": {"pane_id": "w1:pA", "workspace_id": "w1", "tab_id": "w1:t1", "focused": true},
            },
        });
        let opened: Option<OpenedPane> =
            decode_field(&result, "plugin_pane", None).expect("a plugin pane");
        let opened = opened.expect("a pane id");
        assert_eq!(opened_pane_id(&opened), "w1:pA");
        assert_eq!(opened.entrypoint, "p-overlay");
    }

    #[test]
    fn an_invocation_answers_with_the_log_entry_the_daemon_started() {
        let result = json!({
            "type": "plugin_action_invoked",
            "action": {"plugin_id": "a.b", "action_id": "x", "title": "X", "command": ["true"]},
            "context": {"invocation_source": "palette"},
            "log": {
                "log_id": "plugin-log-1", "plugin_id": "a.b", "action_id": "x",
                "command": ["true"], "status": "running", "started_unix_ms": 1,
            },
        });
        let log: PluginCommandLog = required_field(&result, "log").expect("a log entry");
        assert_eq!(log.status, LogStatus::Running);
        assert!(log.finished_unix_ms.is_none());
    }
}
