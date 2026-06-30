# Herdr IPC 客户端

## 目标

实现与 herdr 守护进程的 Unix socket JSON-line IPC 通信。提供 pane 操作、agent 管理、workspace 查询、事件订阅能力。

## 背景

herdr 通过 Unix socket (`~/.local/share/herdr/herdr.sock` 或环境变量 `HERDR_SOCKET`) 暴露 JSON-line API。每条请求和响应均为单行 JSON，以换行符分隔。

请求格式:
```json
{"cmd": "<command>", "key": "value", ...}
```

成功响应:
```json
{"type": "ok", "data": <value>}
```

错误响应:
```json
{"type": "error", "message": "..."}
```

## 接口

```rust
/// Herdr IPC 客户端
pub struct HerdrIpcClient {
    socket_path: PathBuf,
    stream: UnixStream,   // std::os::unix::net::UnixStream
}

// ── 数据结构 ──

pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub tabs: Vec<TabInfo>,
    pub cwd: String,
}

pub struct TabInfo {
    pub id: String,
    pub title: String,
    pub panes: Vec<PaneInfo>,
}

pub struct PaneInfo {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub agent_name: Option<String>,
}

pub struct AgentStatus {
    pub pane_id: String,
    pub agent_name: String,
    pub status: String,   // "idle" | "working" | "blocked" | "done"
    pub recent_output: String,
}

// ── 操作 ──

impl HerdrIpcClient {
    /// 连接到 herdr socket
    pub fn connect() -> Result<Self>;
    
    /// 发送 JSON-line 请求并等待响应
    fn send_request(&mut self, request: &serde_json::Value) -> Result<serde_json::Value>;
    
    /// 列出所有 workspace
    pub fn list_workspaces(&mut self) -> Result<Vec<WorkspaceInfo>>;

    /// 在指定 workspace 中创建新的 pane
    /// cwd: 工作目录
    pub fn create_pane(&mut self, workspace_id: &str, cwd: &str) -> Result<PaneInfo>;

    /// 在 pane 中启动 agent CLI
    pub fn start_agent(&mut self, pane_id: &str, agent_name: &str) -> Result<()>;

    /// 向 pane 发送文本指令（模拟键盘输入）
    pub fn send_text(&mut self, pane_id: &str, text: &str) -> Result<()>;

    /// 读取 pane 的输出
    /// source: "visible" | "recent" | "recent-unwrapped" | "full"
    pub fn read_output(&mut self, pane_id: &str, source: &str) -> Result<String>;

    /// 等待 pane 输出匹配 substring 或 regex pattern
    /// timeout_ms: 超时（毫秒）
    pub fn wait_for_output(&mut self, pane_id: &str, pattern: &str, timeout_ms: u64) -> Result<String>;

    /// 获取所有运行中的 agent 状态
    pub fn list_agent_statuses(&mut self) -> Result<Vec<AgentStatus>>;

    /// 订阅事件流
    /// 返回: 事件 channel receiver
    pub fn subscribe_events(&mut self) -> Result<crossbeam_channel::Receiver<Event>>;
}

/// 事件类型
pub enum Event {
    PaneCreated { pane_id: String, tab_id: String },
    PaneClosed { pane_id: String },
    Output { pane_id: String, text: String },
    AgentStatusChanged { pane_id: String, status: String },
    Error(String),
}
```

## 协议细节

### list_workspaces

请求:
```json
{"cmd": "workspace.list"}
```

响应 (type: ok, data):
```json
[
  {"id": "w1", "name": "main", "tabs": [...], "cwd": "/home/user/project"}
]
```

### create_pane

请求:
```json
{"cmd": "pane.split", "tab_id": "<id>", "cwd": "/path"}
```

### start_agent

请求:
```json
{"cmd": "agent.start", "pane_id": "<id>", "agent": "claude"}
```

### send_text

请求:
```json
{"cmd": "pane.send_text", "pane_id": "<id>", "text": "do something"}
```

### read_output

请求:
```json
{"cmd": "pane.read", "pane_id": "<id>", "source": "recent"}
```

### wait_for_output

请求:
```json
{"cmd": "pane.wait_for_output", "pane_id": "<id>", "match": {"substring": "pattern"}, "timeout": 300000}
```

### subscribe_events

请求:
```json
{"cmd": "events.subscribe"}
```

事件推送（多行 JSON, 每行一个事件）:
```json
{"type": "pane_output", "pane_id": "<id>", "text": "..."}
{"type": "agent_status", "pane_id": "<id>", "status": "working"}
```

## Requirements

- [ ] Unix socket 连接（std::os::unix::net::UnixStream）
- [ ] JSON-line 请求/响应协议（send_request）
- [ ] list_workspaces
- [ ] create_pane
- [ ] start_agent
- [ ] send_text
- [ ] read_output
- [ ] wait_for_output
- [ ] list_agent_statuses
- [ ] subscribe_events
- [ ] Error 处理（socket disconnect、timeout、protocol error）

## Acceptance Criteria

- [ ] 连接运行中的 herdr 实例成功
- [ ] `list_workspaces()` 返回正确的 workspace 列表
- [ ] `create_pane()` + `send_text()` + `read_output()` 可执行命令并读取输出
- [ ] `wait_for_output()` 在 pattern 匹配时正确返回
- [ ] `subscribe_events()` 可收到 pane_output 事件
- [ ] socket 断开时返回错误（不 panic）
- [ ] JSON 解析错误处理（malformed response）

## Notes

- 仅在 Unix 系统上可用（`#[cfg(unix)]`）
- socket path: 先检查环境变量 `HERDR_SOCKET`，否则 `~/.local/share/herdr/herdr.sock`
- 所有 IPC 操作在 `cx.background_executor()` 中执行，不阻塞 UI
