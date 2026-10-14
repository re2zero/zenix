//! The built-in orchestrator: one MCP server, listening on a unix socket in
//! zenix's own config directory.
//!
//! stdio is deliberately not used. zenix is a GUI process whose stdin/stdout
//! belong to the desktop it was launched from, not to a protocol; the socket is
//! what an agent connects to. It follows the process: the shell holds this
//! value, and dropping the window drops the listener.

use std::sync::Arc;

use tokio::sync::oneshot;

use super::handler::McpToolHandler;
use crate::ai::session::SessionManager;
use crate::ai::tools::ToolExecutor;
use crate::ai::workers::WorkerRegistry;

/// A running orchestrator listener. Holding the value keeps the listener alive;
/// dropping it asks the task to stop, and the task unlinks its socket.
pub struct McpOrchestrationServer {
    shutdown_tx: Option<oneshot::Sender<()>>,
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

        // Detached on purpose: nothing waits for the task, and the shutdown
        // signal is what the value's own Drop sends.
        tokio_handle().spawn(async move {
            super::sse::start_sse_server(handler, listening_on, shutdown_rx).await;
        });
        tracing::info!(socket = %socket_path.display(), "MCP orchestration server starting");
        Self {
            shutdown_tx: Some(shutdown_tx),
        }
    }
}

impl Drop for McpOrchestrationServer {
    fn drop(&mut self) {
        // The task removes its socket when its shutdown lands, so a window
        // closing is enough.
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
