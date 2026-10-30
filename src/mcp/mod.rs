//! Unified MCP server management — the built-in orchestrator and the socket it
//! listens on.
//!
//! The config-management half (scan/read/add/edit/remove/toggle/test across the
//! per-agent Claude-format configs and zenix's own NDJSON store) is gone: no
//! settings page mounts an MCP panel through the contribution registry, so that
//! half had no consumer. What is left is the path the shell actually runs, plus
//! the reverse bridge a separate process serves.
//!
//! Consumers: `src/ui/shell.rs` calls [`ensure_orchestrator`] at each window's
//! construction and that path runs through `server` → `handler` → `sse`/
//! `plugin_tools`/`types`. `src/cli.rs` runs `agent_bridge` as `zenix
//! mcp-agent`, which is the one place an external agent gets a tool from zenix.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use crate::platform;

/// The process's one orchestrator, started once and held for the process's
/// life: its `Drop` is its shutdown, so the handle must outlive every window
/// — a closed first window must not end the socket a second window's agent
/// is still using. `None` when the session store could not be opened.
static ORCHESTRATOR: OnceLock<Option<server::McpOrchestrationServer>> = OnceLock::new();

/// Start the orchestrator if this process has not already. Every window calls
/// it; only the first starts the socket, so a second window neither fights
/// over the path nor ends the serving one by closing.
pub fn ensure_orchestrator() {
    let _ = ORCHESTRATOR.get_or_init(|| start_orchestrator(String::new()));
}

/// Whether the orchestrator socket is serving. A process that could not open
/// its session store says no; the attempt is not retried, matching the way
/// the config diagnostics keep one line per problem instead of polling.
pub fn orchestrator_running() -> bool {
    ensure_orchestrator();
    ORCHESTRATOR.get().is_some_and(Option::is_some)
}

/// The socket the built-in orchestrator listens on inside zenix's config dir.
pub fn orchestrator_socket_path() -> PathBuf {
    platform::zenix_config_dir().join("mcp-orchestrator.sock")
}

/// Start the built-in orchestrator for this process: one socket in the config
/// directory, serving the tool set in `handler`. `None` when its session store
/// cannot be opened, so a broken AI database degrades the orchestrator rather
/// than the window.
///
/// `workspace_id` is the workspace the orchestrator works in. An empty value
/// means "whichever workspace the daemon has focused", which is what the pane
/// commands then ask for.
pub fn start_orchestrator(workspace_id: String) -> Option<server::McpOrchestrationServer> {
    let session_manager = match crate::ai::session::SessionManager::open() {
        Ok(manager) => manager,
        Err(err) => {
            tracing::error!(%err, "the orchestration server needs the AI session store");
            return None;
        }
    };
    let executor = Arc::new(crate::ai::tools::ToolExecutor::new(
        crate::ai::endpoint::ApiEndpoint::local(),
        workspace_id.clone(),
    ));
    Some(server::McpOrchestrationServer::start(
        executor,
        Arc::new(crate::ai::workers::WorkerRegistry::new()),
        Arc::new(session_manager),
        workspace_id,
    ))
}

pub mod agent_bridge;
pub mod handler;
pub mod plugin_tools;
pub mod server;
pub mod sse;
pub mod types;
