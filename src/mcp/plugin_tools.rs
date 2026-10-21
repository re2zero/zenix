//! The forward half of the MCP bridge: a plugin action, offered as an MCP tool.
//!
//! A plugin's actions already reach the command palette and, when the plugin
//! declares a chord, the keymap. This module is the same list seen from an
//! agent's side: every action the daemon reports becomes one tool, and calling
//! it runs the action and waits for the daemon to finish it. All three come from
//! one catalog ([`crate::plugins::actions`]), so the three cannot disagree about
//! which actions exist.
//!
//! Two things are not what they look like:
//!
//! * **The names are this client's.** The daemon's own name for an action is
//!   `plugin.id.action`, and a dot is a fine character on the API lane — but a
//!   tool name is read by an MCP client, and the format MCP settled on
//!   (SEP-986: 1–64 characters of `A-Za-z0-9_-.`) is narrower than the API
//!   lane's, with clients in the field narrower still. So every action is
//!   exposed under a name built from `[A-Za-z0-9_]` only, assigned once by the
//!   catalog, which is why the list and the call agree without either side
//!   remembering anything.
//! * **The call blocks.** `plugin.action.invoke` answers as soon as the daemon
//!   has *spawned* the command; the outcome is only ever written into the log
//!   ring. A tool call has to answer with something, so this one polls the ring
//!   for the entry it started and returns what the command printed.
//!
//! The read side is the daemon's, always: a tool list is one `plugin.action.list`
//! per `tools/list`, never a cache a second client could disagree with.

use std::thread;
use std::time::{Duration, Instant};

use rmcp::model::{CallToolResult, Tool};
use serde_json::{Map, Value, json};

use super::handler::McpToolHandler;
use crate::herdr::api::ApiClient;
use crate::plugins::actions::{Catalog, Exposed};
use crate::plugins::api as plugin_api;
use crate::plugins::model::{LogStatus, PluginAction, PluginCommandLog};
use crate::plugins::ui_manifest;

/// How many ring entries one poll asks for. The ring holds 200 for the whole
/// session, and a busy plugin can push a short-lived command out of a smaller
/// window before this side sees it settle.
const LOG_WINDOW: usize = 100;

/// How long a tool call waits for the command the daemon started. Actions are
/// short by nature — a pane's long-running command is a pane, not an action —
/// and a client's own request timeout is in the same range.
const OUTCOME_DEADLINE: Duration = Duration::from_secs(60);

/// How often the ring is read while waiting. The daemon writes the outcome in
/// place as soon as the process exits, so this is the whole latency of a call.
const OUTCOME_POLL: Duration = Duration::from_millis(120);

/// The invocation context fields a caller may set. The daemon fills every other
/// field from its own live state (`merge_plugin_context`), so these are
/// overrides, not a complete context — see [`crate::plugins::api::invoke_action_in`].
const CONTEXT_FIELDS: &[(&str, &str)] = &[
    (
        "workspace_id",
        "Run as if this workspace were focused, instead of the daemon's focused one.",
    ),
    (
        "workspace_cwd",
        "The workspace directory the action sees, overriding the daemon's.",
    ),
    (
        "tab_id",
        "Run as if this tab were focused, instead of the daemon's focused one.",
    ),
    (
        "focused_pane_id",
        "Run as if this pane were focused, instead of the daemon's focused one.",
    ),
    (
        "focused_pane_cwd",
        "The focused pane's directory as the action should see it.",
    ),
    (
        "selected_text",
        "The selection the action should see. Actions scoped to a selection are otherwise refusals.",
    ),
    (
        "correlation_id",
        "An id of your own, echoed back so a later correlation is possible.",
    ),
    (
        "clicked_url",
        "The URL a link handler should treat as clicked.",
    ),
    (
        "link_handler_id",
        "The link handler the action should treat as invoked.",
    ),
];

/// Every action the daemon reports, as a tool. A daemon this process cannot
/// reach yields no tools rather than a failure: a client asking for a tool list
/// while the daemon is down gets the built-in tools, which is the honest answer.
pub(crate) fn tools(api: &ApiClient) -> Vec<Tool> {
    catalog(api)
        .entries()
        .iter()
        .map(|entry| tool(entry))
        .collect()
}

/// Run the bridged tool called `name`, if one is. `None` means no plugin action
/// is exposed under that name, which leaves the caller to report it as unknown.
pub(crate) fn call_named(
    api: &ApiClient,
    name: &str,
    args: &Map<String, Value>,
) -> Option<CallToolResult> {
    let catalog = catalog(api);
    let entry = catalog.named(name)?;
    Some(call(api, entry, args))
}

/// The one derivation this bridge shares with the palette and the keymap: the
/// daemon's actions, paired with the declarations the plugins made for them.
///
/// The declarations are read here rather than taken from the shell's cache, for
/// the same reason the action list is: a tool list is one read of the daemon's
/// own answer, and a cache would be a second client's opinion.
fn catalog(api: &ApiClient) -> Catalog {
    let actions = match plugin_api::list_actions(api) {
        Ok(actions) => actions,
        Err(error) => {
            tracing::warn!(%error, "the plugin action list could not be read for the MCP bridge");
            Vec::new()
        }
    };
    let ui = match plugin_api::list_plugins(api) {
        Ok(plugins) => ui_manifest::mounted_ui(&plugins),
        Err(error) => {
            tracing::warn!(%error, "the plugin registry could not be read for the MCP bridge");
            ui_manifest::UiContributions::default()
        }
    };
    Catalog::build(&actions, &ui)
}

/// The tool's input schema: every context field a caller may override, and
/// nothing else — an unknown field is a client mistake, not something to pass
/// on to a plugin.
fn input_schema() -> Value {
    let properties: Map<String, Value> = CONTEXT_FIELDS
        .iter()
        .map(|(name, description)| {
            (
                (*name).to_owned(),
                json!({"type": "string", "description": description}),
            )
        })
        .collect();
    json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    })
}

/// One action as an MCP tool: what it does, and the context fields it accepts.
fn tool(entry: &Exposed) -> Tool {
    let mut description = format!(
        "Run the herdr plugin action `{}` in the daemon and report its exit code and output.",
        entry.action.qualified_id()
    );
    if let Some(detail) = entry
        .action
        .description
        .as_deref()
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
    {
        description.push(' ');
        description.push_str(detail);
    }
    McpToolHandler::tool_def(entry.tool_name(), &description, input_schema())
}

/// The context a call supplies, from the arguments it was given. Only fields the
/// caller named are sent: the daemon merges this over its own live state, and a
/// field invented here would be a second copy of the truth.
fn context_from(args: &Map<String, Value>) -> Value {
    let mut context = Map::new();
    for (name, _) in CONTEXT_FIELDS {
        if let Some(value) = args
            .get(*name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            context.insert((*name).to_owned(), Value::String(value.to_owned()));
        }
    }
    // The daemon completes the context and reads this field to know where an
    // invocation came from; a bridged call is not a click, and says so.
    context.insert(
        "invocation_source".to_owned(),
        Value::String("mcp".to_owned()),
    );
    Value::Object(context)
}

/// Run one action and answer with its outcome.
fn call(api: &ApiClient, entry: &Exposed, args: &Map<String, Value>) -> CallToolResult {
    let action = &entry.action;
    let name = action.qualified_id();
    let log = match plugin_api::invoke_action_in(api, action, context_from(args)) {
        Ok(log) => log,
        Err(error) => return McpToolHandler::err(format!("{name} was not run: {error}")),
    };
    match await_outcome(api, &action.plugin_id, &log.log_id) {
        Some(finished) => report(action, &finished),
        // The command is still going, or its entry has already been pushed out
        // of the ring. Both are said as they are: the outcome is not lost, it is
        // in the plugin log.
        None => McpToolHandler::err(format!(
            "{name} has not finished within {}s; the daemon reports its outcome in the plugin log.",
            OUTCOME_DEADLINE.as_secs()
        )),
    }
}

/// Wait for the log entry this invocation started to settle, by reading the ring
/// until it has. The daemon writes the outcome into that same entry
/// (`AppEvent::PluginCommandFinished`), so the entry's presence is the signal.
fn await_outcome(api: &ApiClient, plugin_id: &str, log_id: &str) -> Option<PluginCommandLog> {
    let deadline = Instant::now() + OUTCOME_DEADLINE;
    loop {
        if let Ok(logs) = plugin_api::list_logs(api, LOG_WINDOW)
            && let Some(finished) = settled(&logs, plugin_id, log_id)
        {
            return Some(finished.clone());
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(OUTCOME_POLL);
    }
}

/// The settled entry for one invocation, if the ring has it: the same plugin and
/// log id, no longer running. A running entry is not an outcome, and an entry
/// that is gone was pushed out of the ring rather than finished.
fn settled<'a>(
    logs: &'a [PluginCommandLog],
    plugin_id: &str,
    log_id: &str,
) -> Option<&'a PluginCommandLog> {
    logs.iter().find(|log| {
        log.plugin_id == plugin_id && log.log_id == log_id && log.status != LogStatus::Running
    })
}

/// One outcome as a tool result: a heading that always carries the status and
/// exit code, then whatever the command printed. What it printed is the reason
/// an agent would call one of these at all, so it is not truncated here.
fn report(action: &PluginAction, log: &PluginCommandLog) -> CallToolResult {
    let mut heading = format!("{}: {}", action.qualified_id(), log.status.label());
    if let Some(code) = log.exit_code {
        heading.push_str(&format!(" (exit {code})"));
    }
    let mut body = String::new();
    if let Some(error) = log
        .error
        .as_deref()
        .filter(|error| !error.trim().is_empty())
    {
        body.push_str(error);
    } else {
        if let Some(out) = log.stdout.as_deref().filter(|out| !out.trim().is_empty()) {
            body.push_str(out);
        }
        if let Some(err) = log.stderr.as_deref().filter(|err| !err.trim().is_empty()) {
            if !body.is_empty() {
                body.push_str("\n--- stderr ---\n");
            }
            body.push_str(err);
        }
    }
    let text = if body.is_empty() {
        heading
    } else {
        format!("{heading}\n{body}")
    };
    if log.status == LogStatus::Succeeded {
        McpToolHandler::ok(text)
    } else {
        McpToolHandler::err(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::model::PluginAction;

    fn action(plugin_id: &str, action_id: &str) -> PluginAction {
        PluginAction {
            plugin_id: plugin_id.into(),
            action_id: action_id.into(),
            title: format!("{action_id} title"),
            description: None,
            contexts: None,
        }
    }

    fn log(log_id: &str, status: LogStatus) -> PluginCommandLog {
        PluginCommandLog {
            log_id: log_id.into(),
            plugin_id: "a.b".into(),
            action_id: Some("apply".into()),
            event: None,
            command: vec!["true".into()],
            status,
            started_unix_ms: 1,
            finished_unix_ms: None,
            exit_code: None,
            stdout: None,
            stderr: None,
            error: None,
        }
    }

    fn text(result: &CallToolResult) -> String {
        let value = serde_json::to_value(result).expect("a serializable result");
        value["content"][0]["text"]
            .as_str()
            .expect("a text content block")
            .to_owned()
    }

    fn is_error(result: &CallToolResult) -> bool {
        let value = serde_json::to_value(result).expect("a serializable result");
        value["isError"].as_bool().unwrap_or(false)
    }

    #[test]
    fn the_schema_names_every_field_a_call_may_override_and_nothing_else() {
        let schema = input_schema();
        let properties = schema["properties"].as_object().expect("properties");
        assert_eq!(properties.len(), CONTEXT_FIELDS.len());
        for (name, _) in CONTEXT_FIELDS {
            assert!(properties.contains_key(*name), "{name}");
        }
        assert_eq!(schema["additionalProperties"], json!(false));
    }

    #[test]
    fn only_the_context_fields_a_caller_named_are_sent() {
        let mut args = Map::new();
        args.insert("workspace_id".into(), json!("w2"));
        args.insert("selected_text".into(), json!(""));
        args.insert("something_else".into(), json!("dropped"));
        let context = context_from(&args);
        assert_eq!(context["workspace_id"], json!("w2"));
        assert_eq!(context["invocation_source"], json!("mcp"));
        assert!(
            context.get("selected_text").is_none(),
            "an empty string is not a selection"
        );
        assert!(context.get("something_else").is_none());
        assert_eq!(
            context.as_object().map(Map::len),
            Some(2),
            "the daemon fills every other field"
        );
    }

    #[test]
    fn only_an_entry_that_stopped_running_is_an_outcome() {
        let logs = vec![log("plugin-log-1", LogStatus::Running)];
        assert!(settled(&logs, "a.b", "plugin-log-1").is_none());
        let logs = vec![log("plugin-log-1", LogStatus::Succeeded)];
        assert_eq!(
            settled(&logs, "a.b", "plugin-log-1").map(|log| log.status),
            Some(LogStatus::Succeeded)
        );
        assert!(
            settled(&logs, "a.b", "plugin-log-2").is_none(),
            "an entry that is gone was pushed out of the ring, not finished"
        );
        assert!(
            settled(&logs, "other.plugin", "plugin-log-1").is_none(),
            "a log id is the daemon's own and a plugin's entries are what is read"
        );
    }

    #[test]
    fn an_outcome_carries_the_exit_code_and_what_the_command_printed() {
        let action = action("a.b", "apply");
        let mut finished = log("plugin-log-1", LogStatus::Failed);
        finished.exit_code = Some(3);
        finished.stderr = Some("deliberate failure\n".into());
        let result = report(&action, &finished);
        assert!(is_error(&result));
        let shown = text(&result);
        assert!(shown.contains("a.b.apply: failed (exit 3)"), "{shown}");
        assert!(shown.contains("deliberate failure"), "{shown}");

        let mut succeeded = log("plugin-log-2", LogStatus::Succeeded);
        succeeded.exit_code = Some(0);
        succeeded.stdout = Some("done\n".into());
        let result = report(&action, &succeeded);
        assert!(!is_error(&result));
        let shown = text(&result);
        assert!(shown.contains("done"), "{shown}");
    }

    #[test]
    fn a_command_the_daemon_could_not_spawn_reports_its_reason() {
        let mut failed = log("plugin-log-1", LogStatus::Failed);
        failed.error = Some("spawn failed: No such file or directory".into());
        let result = report(&action("a.b", "apply"), &failed);
        assert!(is_error(&result));
        assert!(text(&result).contains("No such file or directory"));
    }

    #[test]
    fn a_tool_is_named_and_described_for_the_action_it_runs() {
        let mut apply = action("a.b", "apply");
        apply.description = Some("Applies the project layout.".into());
        let catalog = Catalog::build(&[apply], &Default::default());
        let tool = tool(&catalog.entries()[0]);
        let value = serde_json::to_value(&tool).expect("a serializable tool");
        assert_eq!(value["name"], json!("plugin_a_b_apply"));
        let description = value["description"].as_str().expect("a description");
        assert!(description.contains("a.b.apply"), "{description}");
        assert!(
            description.contains("Applies the project layout."),
            "{description}"
        );
        assert_eq!(value["inputSchema"]["type"], json!("object"));
    }
}
