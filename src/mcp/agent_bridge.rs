//! The reverse bridge: one tool, for an agent that has no shell.
//!
//! M8c bridged outward — every plugin action became a tool inside zenix's own
//! orchestrator, and the orchestrator speaks to the daemon for the chat panel.
//! This is the other direction, narrowed on purpose: an agent CLI launches
//! `zenix mcp-agent` as its MCP server and gets exactly one tool, `agent_send`,
//! which submits a prompt to another agent in the running daemon. There is no
//! passthrough: the worker mailboxes, the pane lifecycle and the plugin
//! registry the orchestrator's client sees are not on this surface.
//!
//! The transport is stdio, not another socket. zenix's orchestrator socket
//! exists for a GUI process whose stdin belongs to the desktop;
//! `zenix mcp-agent` is a child process whose stdin/stdout *are* the transport,
//! which is the shape every agent CLI already knows how to launch:
//!
//! ```json
//! {"command": "zenix", "args": ["mcp-agent"]}
//! ```
//!
//! Why a tool at all, when `herdr agent prompt <TARGET> <TEXT> [--wait]` exists
//! in the daemon's own CLI? Because that one needs a shell. This one needs an
//! MCP client, and handing out a single narrow capability is the point.

use std::time::Duration;

use rmcp::{handler::server::ServerHandler, model::*, service::RequestContext};
use serde_json::{Map, Value, json};

use crate::ai::endpoint::{AgentInfo, ApiEndpoint, EndpointError};

/// The one tool this server offers.
pub const TOOL: &str = "agent_send";

/// How long a `wait_until` call waits on the daemon when the caller names no
/// budget. The daemon answers `timeout` of its own accord at the same deadline.
const DEFAULT_WAIT: Duration = Duration::from_secs(60);

/// The agent states `wait_until` accepts, spelled as the daemon spells them.
const WAIT_STATES: [&str; 4] = ["idle", "working", "blocked", "done"];

#[derive(Clone)]
pub struct AgentBridge {
    endpoint: ApiEndpoint,
}

impl AgentBridge {
    /// The bridge for this machine's daemon, resolved the way the daemon
    /// resolves it.
    pub fn local() -> Self {
        Self {
            endpoint: ApiEndpoint::local(),
        }
    }

    /// The bridge for an explicit API socket.
    pub fn at(socket: std::path::PathBuf) -> Self {
        Self {
            endpoint: ApiEndpoint::at(socket),
        }
    }

    pub(crate) fn tool() -> Tool {
        Self::tool_def(
            TOOL,
            "Submit a prompt to another agent in the herdr workspace, by agent \
             name or pane id. Set wait_until to block until that agent reaches \
             the state, so a hand-off is finished before you continue.",
            json!({
                "type": "object",
                "properties": {
                    "target": {
                        "type": "string",
                        "description": "Agent name (`reviewer`) or pane id (`w1:p2`)."
                    },
                    "text": {
                        "type": "string",
                        "description": "The prompt, submitted as one turn."
                    },
                    "wait_until": {
                        "type": "string",
                        "enum": WAIT_STATES,
                        "description": "Wait for the agent to reach this state \
                                        after the prompt was accepted."
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Budget for wait_until; 60000 when omitted."
                    }
                },
                "required": ["target", "text"]
            }),
        )
    }

    fn tool_def(name: &str, desc: &str, schema: Value) -> Tool {
        serde_json::from_value(json!({
            "name": name,
            "description": desc,
            "inputSchema": schema
        }))
        .expect("a tool definition is well formed")
    }

    /// Dispatch one call. Unknown names and unusable arguments are answered as
    /// errors rather than refused, because a model reads the text either way.
    pub(crate) fn call(&self, name: &str, args: Map<String, Value>) -> CallToolResult {
        if name != TOOL {
            return Self::err(format!(
                "unknown tool '{name}'; this server offers only {TOOL}"
            ));
        }
        let target = args
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if target.is_empty() {
            return Self::err("target is required (an agent name or a pane id)".into());
        }
        let text = args.get("text").and_then(Value::as_str).unwrap_or("");
        if text.is_empty() {
            return Self::err("text is required".into());
        }
        let wait_until = match args.get("wait_until").and_then(Value::as_str) {
            Some(state) if !WAIT_STATES.contains(&state) => {
                return Self::err(format!(
                    "wait_until must be one of {}",
                    WAIT_STATES.join(", ")
                ));
            }
            state => state,
        };
        let timeout_ms = args.get("timeout_ms").and_then(Value::as_u64);

        match self.send(target, text, wait_until, timeout_ms) {
            Ok(agent) => Self::ok(describe(target, &agent)),
            Err(message) => Self::err(message),
        }
    }

    fn send(
        &self,
        target: &str,
        text: &str,
        wait_until: Option<&str>,
        timeout_ms: Option<u64>,
    ) -> Result<AgentInfo, String> {
        let prompted = match wait_until {
            Some(state) => {
                let budget = timeout_ms
                    .map(Duration::from_millis)
                    .unwrap_or(DEFAULT_WAIT);
                self.endpoint.prompt_agent_and_wait(
                    target,
                    text,
                    &[state],
                    budget.as_millis() as u64,
                )
            }
            None => self.endpoint.prompt_agent(target, text),
        };
        prompted.map_err(|error| self.explain(target, error))
    }

    /// Turn the daemon's refusal into something the calling model can act on:
    /// an unknown target is answered with the agents that do exist.
    fn explain(&self, target: &str, error: EndpointError) -> String {
        let mut text = format!("agent.prompt refused '{target}': {error}");
        if error.code() == Some("agent_not_found") {
            match self.endpoint.list_agents() {
                Ok(agents) if !agents.is_empty() => {
                    let known: Vec<String> = agents
                        .iter()
                        .map(|agent| match agent.name.as_deref() {
                            Some(name) => format!("{name} ({})", agent.pane_id),
                            None => agent.pane_id.clone(),
                        })
                        .collect();
                    text.push_str(&format!("; running agents: {}", known.join(", ")));
                }
                _ => text.push_str("; no agent is running"),
            }
        }
        text
    }

    fn ok(text: String) -> CallToolResult {
        serde_json::from_value(json!({
            "content": [{"type": "text", "text": text}],
            "isError": false
        }))
        .expect("a text result is well formed")
    }

    fn err(text: String) -> CallToolResult {
        serde_json::from_value(json!({
            "content": [{"type": "text", "text": text}],
            "isError": true
        }))
        .expect("a text result is well formed")
    }
}

/// What a successful submission says back. The state is the daemon's, so a
/// caller that asked for `wait_until` sees what it actually matched.
fn describe(target: &str, agent: &AgentInfo) -> String {
    format!(
        "sent to {} ({}) · {}",
        agent.name.as_deref().unwrap_or(target),
        agent.pane_id,
        agent.agent_status.as_deref().unwrap_or("unknown")
    )
}

/// Serve the bridge on this process's stdin/stdout until the client closes it.
///
/// A runtime of its own: this path never touches the GUI's global tokio handle,
/// and it is entered from `main` before a window exists.
pub fn run_stdio() -> i32 {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("zenix mcp-agent: no tokio runtime: {error}");
            return 1;
        }
    };
    runtime.block_on(async {
        match rmcp::serve_server(AgentBridge::local(), rmcp::transport::stdio()).await {
            Ok(service) => {
                // `waiting` is the stdio client closing the pipe, which is how a
                // launcher ends this process's turn.
                match service.waiting().await {
                    Ok(_) => 0,
                    Err(error) => {
                        eprintln!("zenix mcp-agent: the session ended badly: {error}");
                        1
                    }
                }
            }
            Err(error) => {
                eprintln!("zenix mcp-agent: the MCP handshake failed: {error}");
                1
            }
        }
    })
}

impl ServerHandler for AgentBridge {
    fn get_info(&self) -> ServerInfo {
        serde_json::from_value(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "zenix-agent",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Send a prompt to another agent in the herdr workspace."
        }))
        .expect("valid server info")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<rmcp::service::RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        serde_json::from_value(json!({"tools": [Self::tool()]}))
            .map_err(|error| ErrorData::internal_error(format!("one tool: {error}"), None))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<rmcp::service::RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        // A prompt with `wait_until` can hold this call for a minute, and the
        // socket call is blocking: it belongs off the async threads.
        let bridge = self.clone();
        let name = request.name.to_string();
        let args = request.arguments.unwrap_or_default();
        tokio::task::spawn_blocking(move || bridge.call(&name, args))
            .await
            .map_err(|error| {
                ErrorData::internal_error(format!("the tool task stopped: {error}"), None)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::thread::JoinHandle;

    /// A stand-in for the daemon: answers each connection with the next canned
    /// line and hands back the request it was asked. The API lane is one
    /// request per connection, so the replies are positional.
    fn fake_daemon(replies: Vec<Value>) -> (PathBuf, JoinHandle<Vec<Value>>) {
        let socket = std::env::temp_dir().join(format!(
            "zenix-agent-bridge-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock")
                .subsec_nanos()
        ));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).expect("a listener");
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let (stream, _) = listener.accept().expect("a connection");
                let mut line = String::new();
                BufReader::new(stream.try_clone().expect("a clone"))
                    .read_line(&mut line)
                    .expect("a request line");
                requests.push(serde_json::from_str(&line).expect("valid json"));
                let mut stream: UnixStream = stream;
                stream
                    .write_all(format!("{reply}\n").as_bytes())
                    .expect("a reply");
                stream.flush().expect("flushed");
            }
            requests
        });
        (socket, handle)
    }

    fn args(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    fn text_of(result: &CallToolResult) -> String {
        result
            .content
            .first()
            .and_then(|content| content.as_text())
            .map(|text| text.text.clone())
            .unwrap_or_default()
    }

    fn failed(result: &CallToolResult) -> bool {
        result.is_error == Some(true)
    }

    #[test]
    fn the_tool_asks_for_a_target_and_a_text_and_nothing_else_required() {
        let tool = AgentBridge::tool();
        assert_eq!(tool.name.as_ref(), TOOL);
        let schema = serde_json::to_value(tool.input_schema).expect("a schema");
        assert_eq!(schema["required"], json!(["target", "text"]));
        assert_eq!(
            schema["properties"]["wait_until"]["enum"],
            json!(["idle", "working", "blocked", "done"])
        );
    }

    #[test]
    fn a_submission_sends_agent_prompt_and_reports_the_matched_state() {
        let (socket, daemon) = fake_daemon(vec![json!({
            "id": "x",
            "result": {
                "type": "agent_prompted",
                "agent": {"pane_id": "w1:p2", "name": "reviewer", "agent_status": "idle"}
            }
        })]);
        let bridge = AgentBridge::at(socket.clone());
        let result = bridge.call(
            TOOL,
            args(&[
                ("target", json!("reviewer")),
                ("text", json!("take a look")),
            ]),
        );
        assert!(!failed(&result), "{}", text_of(&result));
        assert_eq!(text_of(&result), "sent to reviewer (w1:p2) · idle");

        let request = daemon.join().expect("the daemon thread");
        assert_eq!(request[0]["method"], "agent.prompt");
        assert_eq!(request[0]["params"]["target"], "reviewer");
        assert_eq!(request[0]["params"]["text"], "take a look");
        assert!(
            request[0]["params"].get("wait").is_none(),
            "a plain send does not ask the daemon to wait"
        );
        let _ = std::fs::remove_file(socket);
    }

    #[test]
    fn waiting_hands_the_budget_and_the_state_to_the_daemon() {
        let (socket, daemon) = fake_daemon(vec![json!({
            "id": "x",
            "result": {
                "type": "agent_prompted",
                "agent": {"pane_id": "w1:p2", "name": "reviewer", "agent_status": "idle"}
            }
        })]);
        let bridge = AgentBridge::at(socket.clone());
        let result = bridge.call(
            TOOL,
            args(&[
                ("target", json!("w1:p2")),
                ("text", json!("go")),
                ("wait_until", json!("idle")),
                ("timeout_ms", json!(5000)),
            ]),
        );
        assert!(!failed(&result), "{}", text_of(&result));
        assert_eq!(text_of(&result), "sent to reviewer (w1:p2) · idle");

        let request = daemon.join().expect("the daemon thread");
        assert_eq!(request[0]["params"]["wait"]["until"], json!(["idle"]));
        assert_eq!(request[0]["params"]["wait"]["timeout_ms"], json!(5000));
        let _ = std::fs::remove_file(socket);
    }

    #[test]
    fn an_unknown_target_is_answered_with_the_agents_that_exist() {
        let (socket, daemon) = fake_daemon(vec![
            json!({
                "id": "x",
                "error": {"code": "agent_not_found", "message": "no agent matches 'ghost'"}
            }),
            json!({
                "id": "y",
                "result": {
                    "type": "agent_list",
                    "agents": [
                        {"pane_id": "w1:p1"},
                        {"pane_id": "w1:p2", "name": "reviewer"}
                    ]
                }
            }),
        ]);
        let bridge = AgentBridge::at(socket.clone());
        let result = bridge.call(
            TOOL,
            args(&[("target", json!("ghost")), ("text", json!("hi"))]),
        );
        assert!(failed(&result));
        let text = text_of(&result);
        assert!(text.contains("agent_not_found"), "{text}");
        assert!(
            text.contains("running agents: w1:p1, reviewer (w1:p2)"),
            "{text}"
        );

        let request = daemon.join().expect("the daemon thread");
        assert_eq!(request[1]["method"], "agent.list");
        let _ = std::fs::remove_file(socket);
    }

    #[test]
    fn unusable_arguments_never_reach_the_daemon() {
        let bridge = AgentBridge::at(PathBuf::from("/tmp/zenix-agent-bridge-unused.sock"));

        let missing_target = bridge.call(TOOL, args(&[("text", json!("hi"))]));
        assert!(failed(&missing_target));
        assert!(text_of(&missing_target).contains("target is required"));

        let missing_text = bridge.call(TOOL, args(&[("target", json!("reviewer"))]));
        assert!(failed(&missing_text));
        assert!(text_of(&missing_text).contains("text is required"));

        let bad_state = bridge.call(
            TOOL,
            args(&[
                ("target", json!("reviewer")),
                ("text", json!("hi")),
                ("wait_until", json!("sleeping")),
            ]),
        );
        assert!(failed(&bad_state));
        assert!(text_of(&bad_state).contains("wait_until must be one of"));

        let other_tool = bridge.call("agent_list", args(&[]));
        assert!(failed(&other_tool));
        assert!(text_of(&other_tool).contains("only agent_send"));
    }

    #[test]
    fn a_daemon_that_is_not_there_is_an_error_not_a_panic() {
        let bridge = AgentBridge::at(PathBuf::from("/tmp/zenix-agent-bridge-no-such.sock"));
        let result = bridge.call(
            TOOL,
            args(&[("target", json!("reviewer")), ("text", json!("hi"))]),
        );
        assert!(failed(&result));
        assert!(text_of(&result).contains("refused"));
    }
}
