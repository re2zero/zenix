# Herdr IPC 客户端 — 设计

## Socket 连接

```rust
use std::os::unix::net::UnixStream;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

pub struct HerdrIpcClient {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl HerdrIpcClient {
    /// 确定 socket 路径
    fn socket_path() -> PathBuf {
        std::env::var("HERDR_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let base = dirs::data_dir()
                    .unwrap_or_else(|| PathBuf::from("~/.local/share"));
                base.join("herdr").join("herdr.sock")
            })
    }

    /// 连接 herdr socket
    pub fn connect() -> Result<Self> {
        let path = Self::socket_path();
        let stream = UnixStream::connect(&path)
            .with_context(|| format!("connect to herdr socket at {}", path.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self { stream, reader })
    }
}
```

## JSON-line 通信协议

```rust
impl HerdrIpcClient {
    /// 发送请求并等待响应，响应不超过 5s
    fn send_request(&mut self, cmd: &str, args: serde_json::Value) -> Result<serde_json::Value> {
        let mut request = serde_json::json!({ "cmd": cmd });
        if let Some(obj) = args.as_object() {
            for (k, v) in obj {
                request[k] = v.clone();
            }
        }

        // 发送
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        self.stream.write_all(line.as_bytes())?;
        self.stream.flush()?;

        // 读取响应（一行 JSON）
        let mut resp_line = String::new();
        self.reader.read_line(&mut resp_line)?;
        let resp: serde_json::Value = serde_json::from_str(resp_line.trim())?;

        // 检查错误
        match resp.get("type").and_then(|t| t.as_str()) {
            Some("ok") => Ok(resp.get("data").cloned().unwrap_or(serde_json::Value::Null)),
            Some("error") => Err(anyhow::anyhow!(
                "herdr error: {}",
                resp.get("message").and_then(|m| m.as_str()).unwrap_or("unknown")
            )),
            _ => Err(anyhow::anyhow!("unexpected response: {}", resp_line.trim())),
        }
    }
}
```

## 操作实现

```rust
impl HerdrIpcClient {
    pub fn list_workspaces(&mut self) -> Result<Vec<WorkspaceInfo>> {
        let data = self.send_request("workspace.list", serde_json::json!({}))?;
        serde_json::from_value(data).context("parse workspace list")
    }

    pub fn create_pane(&mut self, workspace_id: &str, cwd: &str) -> Result<PaneInfo> {
        let data = self.send_request("pane.split", serde_json::json!({
            "tab_id": workspace_id,
            "cwd": cwd,
        }))?;
        serde_json::from_value(data).context("parse pane info")
    }

    pub fn start_agent(&mut self, pane_id: &str, agent_name: &str) -> Result<()> {
        self.send_request("agent.start", serde_json::json!({
            "pane_id": pane_id,
            "agent": agent_name,
        }))?;
        Ok(())
    }

    pub fn send_text(&mut self, pane_id: &str, text: &str) -> Result<()> {
        self.send_request("pane.send_text", serde_json::json!({
            "pane_id": pane_id,
            "text": text,
        }))?;
        Ok(())
    }

    pub fn read_output(&mut self, pane_id: &str, source: &str) -> Result<String> {
        let data = self.send_request("pane.read", serde_json::json!({
            "pane_id": pane_id,
            "source": source,
        }))?;
        Ok(data.as_str().unwrap_or("").to_string())
    }

    pub fn wait_for_output(&mut self, pane_id: &str, pattern: &str, timeout_ms: u64) -> Result<String> {
        let data = self.send_request("pane.wait_for_output", serde_json::json!({
            "pane_id": pane_id,
            "match": { "substring": pattern },
            "timeout": timeout_ms,
        }))?;
        Ok(data.as_str().unwrap_or("").to_string())
    }

    pub fn list_agent_statuses(&mut self) -> Result<Vec<AgentStatus>> {
        let data = self.send_request("pane.report-agent", serde_json::json!({}))?;
        let agents: Vec<AgentStatus> = serde_json::from_value(data)?;
        Ok(agents)
    }
}
```

## 事件订阅

事件订阅需要保持长连接持续读取。使用 `crossbeam_channel` 将事件推送到 receiver：

```rust
use std::thread;
use crossbeam_channel::{unbounded, Receiver, Sender};

pub fn subscribe_events(&mut self) -> Result<Receiver<Event>> {
    self.send_request("events.subscribe", serde_json::json!({}))?;

    let (tx, rx) = unbounded();
    let mut reader = BufReader::new(
        self.stream.try_clone()?  // clone 用于事件读取
    );

    thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => { let _ = tx.send(Event::Error("socket closed".into())); break; }
                Ok(_) => {
                    let event = parse_event(line.trim());
                    if let Some(e) = event {
                        if tx.send(e).is_err() { break; }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Event::Error(format!("read error: {e}")));
                    break;
                }
            }
        }
    });

    Ok(rx)
}

fn parse_event(line: &str) -> Option<Event> {
    let val: serde_json::Value = serde_json::from_str(line).ok()?;
    match val.get("type").and_then(|t| t.as_str()) {
        Some("pane_output") => Some(Event::Output {
            pane_id: val.get("pane_id")?.as_str()?.to_string(),
            text: val.get("text")?.as_str()?.to_string(),
        }),
        Some("agent_status") => Some(Event::AgentStatusChanged {
            pane_id: val.get("pane_id")?.as_str()?.to_string(),
            status: val.get("status")?.as_str()?.to_string(),
        }),
        _ => None,
    }
}
```

## Requirements

- [ ] `HerdrIpcClient::connect()` — Unix socket 连接
- [ ] `send_request()` — JSON-line 发送/接收
- [ ] `list_workspaces()`
- [ ] `create_pane()` / `start_agent()`
- [ ] `send_text()` / `read_output()` / `wait_for_output()`
- [ ] `list_agent_statuses()`
- [ ] `subscribe_events()` — 事件 channel
- [ ] 错误处理（socket 断开、超时、协议错误）
- [ ] `#[cfg(unix)]` 门控

## 注意事项

- `UnixStream` 是 `std::os::unix::net::UnixStream`，非 tokio 版本
- 所有 IPC 操作在 `cx.background_executor()` 中调用
- 事件订阅使用独立线程 + channel，通过 `Receiver` 非阻塞接收
