use std::sync::Arc;

use rmcp::handler::server::ServerHandler;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
    sync::oneshot,
};

use super::handler::McpToolHandler;

pub async fn start_sse_server(
    handler: McpToolHandler,
    socket_path: std::path::PathBuf,
    shutdown_rx: oneshot::Receiver<()>,
) {
    let _ = std::fs::remove_file(&socket_path);
    let listener = match UnixListener::bind(&socket_path) {
        Ok(l) => l,
        Err(e) => { tracing::error!("SSE bind {socket_path:?}: {e}"); return; }
    };
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o700));
    }
    tracing::info!("MCP SSE listening on {socket_path:?}");
    let handler = Arc::new(handler);
    tokio::select! {
        _ = accept_loop(&listener, handler) => {}
        _ = shutdown_rx => { tracing::info!("MCP SSE shutting down"); }
    }
    let _ = std::fs::remove_file(&socket_path);
}

async fn accept_loop(listener: &UnixListener, handler: Arc<McpToolHandler>) {
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => { tracing::warn!("SSE accept: {e}"); continue; }
        };
        tokio::spawn(handle_conn(stream, handler.clone()));
    }
}

async fn handle_conn(stream: tokio::net::UnixStream, handler: Arc<McpToolHandler>) {
    let (read_half, write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    let writer = Arc::new(tokio::sync::Mutex::new(write_half));
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            _ => {}
        }
        let trimmed = line.trim();

        if trimmed.starts_with("GET /") {
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).await.unwrap_or(0) == 0 { break; }
                if h.trim().is_empty() { break; }
            }
            {
                let mut w = writer.lock().await;
                let _ = w.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n").await;
            }
            send_sse(&writer, r#"{"type":"endpoint","data":"/mcp"}"#).await;
        }
        else if trimmed.starts_with("POST /mcp") {
            let mut content_length = 0usize;
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).await.unwrap_or(0) == 0 { break; }
                if h.trim().is_empty() { break; }
                if let Some(len) = h.to_lowercase().strip_prefix("content-length:") {
                    content_length = len.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                use tokio::io::AsyncReadExt;
                let _ = reader.read_exact(&mut body).await;
            }
            let body_str = String::from_utf8_lossy(&body);
            let resp = handle_jsonrpc(&handler, &body_str).await;
            send_sse(&writer, &resp).await;
        }
    }
}

async fn send_sse(writer: &Arc<tokio::sync::Mutex<tokio::io::WriteHalf<tokio::net::UnixStream>>>, data: &str) {
    let mut w = writer.lock().await;
    let _ = w.write_all(format!("event: message\ndata: {data}\n\n").as_bytes()).await;
    let _ = w.flush().await;
}

async fn handle_jsonrpc(handler: &McpToolHandler, body: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"}}"#.into(),
    };
    let method = parsed.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = parsed.get("id");

    let result = match method {
        "tools/list" => {
            let tools = McpToolHandler::build_tools();
            ok_json(id, serde_json::json!({"tools": tools}))
        }
        "tools/call" => {
            let params = parsed.get("params");
            let name = params.and_then(|p| p.get("name")).and_then(|v| v.as_str()).unwrap_or("");
            let args = params.and_then(|p| p.get("arguments"))
                .and_then(|v| v.as_object()).cloned().unwrap_or_default();
            let result = handler.handle_tool_call(name, args);
            ok_json(id, serde_json::to_value(result).unwrap_or_default())
        }
        "initialize" => {
            let info_value = serde_json::to_value(handler.get_info()).unwrap_or_default();
            ok_json(id, info_value)
        }
        "notifications/initialized" | "ping" => ok_json(id, serde_json::json!({})),
        _ => err_json(id, -32601, &format!("Method not found: {method}")),
    };
    result.to_string()
}

fn ok_json(id: Option<&serde_json::Value>, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn err_json(id: Option<&serde_json::Value>, code: i32, msg: &str) -> serde_json::Value {
    serde_json::json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":msg}})
}
