//! The built-in orchestrator: one MCP server, listening on a unix socket in
//! zenix's own config directory.
//!
//! stdio is deliberately not used. zenix is a GUI process whose stdin/stdout
//! belong to the desktop it was launched from, not to a protocol; the socket is
//! what an agent connects to. It follows the process: the shell holds this
//! value, and dropping the window drops the listener.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::oneshot;

use super::handler::McpToolHandler;
use crate::ai::session::SessionManager;
use crate::ai::tools::ToolExecutor;
use crate::ai::workers::WorkerRegistry;

/// A running orchestrator listener.
pub struct McpOrchestrationServer {
    shutdown_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<tokio::task::JoinHandle<()>>,
    /// The socket the listener is bound to, for logs and for the MCP entry that
    /// points at it (`super::builtin_orchestrator_server`).
    pub socket_path: PathBuf,
}

impl McpOrchestrationServer {
    /// Queue the listener on the process's tokio runtime. Returns immediately:
    /// binding happens on that task, and a bind failure is logged there.
    pub fn start(
        executor: Arc<ToolExecutor>,
        worker_registry: Arc<WorkerRegistry>,
        session_manager: Arc<SessionManager>,
        workspace_id: String,
    ) -> Self {
        let handler = McpToolHandler::new(executor, worker_registry, session_manager, workspace_id);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let socket_path = super::orchestrator_socket_path();
        let listening_on = socket_path.clone();

        let join_handle = tokio_handle().spawn(async move {
            super::sse::start_sse_server(handler, listening_on, shutdown_rx).await;
        });

        tracing::info!(socket = %socket_path.display(), "MCP orchestration server starting");
        Self {
            shutdown_tx: Some(shutdown_tx),
            join_handle: Some(join_handle),
            socket_path,
        }
    }

    /// Stop the listener and wait for its task.
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.await;
        }
        tracing::info!("MCP orchestration server stopped");
    }
}

impl Drop for McpOrchestrationServer {
    fn drop(&mut self) {
        // The task removes its socket when its shutdown lands, so a window
        // closing is enough; `shutdown` is only for callers that need to wait.
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// The process's tokio runtime. GPUI's executor is not tokio, so a listener
/// started from the shell spawns here.
fn tokio_handle() -> tokio::runtime::Handle {
    tokio::runtime::Handle::try_current().unwrap_or_else(|_| {
        crate::TOKIO_HANDLE
            .get()
            .expect("TOKIO_HANDLE is set before any window opens")
            .clone()
    })
}
