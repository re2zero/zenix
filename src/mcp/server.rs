use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use rmcp::service::serve_server;
use rmcp::transport::IntoTransport;
use tokio::sync::oneshot;

use super::handler::McpToolHandler;
use crate::ai::session::SessionManager;
use crate::ai::tools::ToolExecutor;
use crate::ai::workers::WorkerRegistry;
use crate::platform;

struct ShutdownTx {
    stdio: oneshot::Sender<()>,
    sse: Option<oneshot::Sender<()>>,
}

pub struct McpOrchestrationServer {
    shutdown_tx: Option<ShutdownTx>,
    join_handle: Option<tokio::task::JoinHandle<()>>,
    pub socket_path: Option<PathBuf>,
}

impl McpOrchestrationServer {
    pub async fn start(
        executor: Arc<ToolExecutor>,
        worker_registry: Arc<WorkerRegistry>,
        session_manager: Arc<SessionManager>,
        workspace_id: String,
        enable_sse: bool,
    ) -> Result<Self> {
        let handler = McpToolHandler::new(executor, worker_registry, session_manager, workspace_id);
        let (stdio_shutdown_tx, stdio_shutdown_rx) = oneshot::channel::<()>();
        let (sse_shutdown_tx, sse_shutdown_rx) = oneshot::channel::<()>();

        // Stdio server
        let (stdin, stdout) = rmcp::transport::io::stdio();
        let transport = (stdin, stdout).into_transport();
        let running = serve_server(handler.clone(), transport).await
            .map_err(|e| anyhow::anyhow!("MCP stdio server failed: {e}"))?;

        // SSE server (optional)
        let socket_path = if enable_sse {
            let path = platform::zenix_config_dir().join("mcp-orchestrator.sock");
            let sse_path = path.clone();
            let sse_handler = handler.clone();
            tokio::task::spawn(async move {
                super::sse::start_sse_server(sse_handler, sse_path, sse_shutdown_rx).await;
            });
            Some(path)
        } else {
            None
        };

        let join_handle = tokio::task::spawn(async move {
            tokio::select! {
                _ = stdio_shutdown_rx => {
                    tracing::info!("MCP server shutting down");
                }
            }
            drop(running);
        });

        let shutdown_tx = ShutdownTx {
            stdio: stdio_shutdown_tx,
            sse: if enable_sse { Some(sse_shutdown_tx) } else { None },
        };

        tracing::info!("MCP orchestration server started (stdio{})",
            if enable_sse { " + sse" } else { "" });
        Ok(Self { shutdown_tx: Some(shutdown_tx), join_handle: Some(join_handle), socket_path })
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.stdio.send(());
            if let Some(sse_tx) = tx.sse { let _ = sse_tx.send(()); }
        }
        if let Some(handle) = self.join_handle.take() { let _ = handle.await; }
        tracing::info!("MCP orchestration server stopped");
    }
}

impl Drop for McpOrchestrationServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.stdio.send(());
            if let Some(sse_tx) = tx.sse { let _ = sse_tx.send(()); }
        }
    }
}
