//! The orchestrator's command lane: herdr's JSON API over `herdr.sock`.
//!
//! Two lanes lead to the same daemon, and they are not interchangeable. The
//! generation-1 endpoint socket zenix paints through (`herdr-client.sock`)
//! advertises a fixed 40-method set and refuses anything else with
//! `unsupported_method`; it carries no `agent.*`, no `pane.read` and no
//! `pane.wait_for_output`. The API socket beside it is the lane the herdr CLI
//! itself uses: one newline-delimited JSON request per connection, one line of
//! JSON back, and the whole method table — including the agent lifecycle and
//! pane-read methods the six orchestration tools are defined on.
//!
//! Evidence and line references: `.relay/notes/herdr-agent-api-lane.md`.
//!
//! Everything here is synchronous and one-shot: a call connects, writes a line,
//! reads a line and closes. There is no shared connection state to guard, which
//! is why the tool executor can hold this by value and call it from the
//! synchronous tool path.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::client;

/// How long to wait for a reply that the daemon is expected to produce at once.
/// Waits the daemon performs on the caller's behalf (`pane.wait_for_output`,
/// `agent.prompt` with a submit deadline) pass their own budget instead.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Slack added to a daemon-side wait so its own timeout answer arrives first.
const WAIT_SLACK: Duration = Duration::from_secs(5);

/// How often a readiness wait asks the daemon again. The daemon marks a started
/// agent active on the order of its own settle delay, which is 100 ms.
const READY_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Where a pane read takes its text from. The daemon spells these on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadSource {
    /// Only what the pane shows right now.
    Visible,
    /// The last `lines` of the pane plus its scrollback.
    Recent,
    /// Like `Recent`, with wrapped lines joined back together.
    RecentUnwrapped,
    /// The detection view the daemon uses to classify the pane's agent.
    Detection,
}

impl ReadSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::Recent => "recent",
            Self::RecentUnwrapped => "recent_unwrapped",
            Self::Detection => "detection",
        }
    }
}

/// Which way `pane.split` divides the target pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Right,
    Down,
}

impl SplitDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Down => "down",
        }
    }
}

/// One running agent, as `agent.list` / `agent.info` report it. Only the fields
/// the tools surface are named; the daemon sends more and they are ignored.
#[derive(Clone, Debug, Deserialize)]
pub struct AgentInfo {
    pub pane_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub terminal_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub focused: bool,
    /// The daemon has finished starting the CLI, so the agent takes prompts.
    #[serde(default)]
    pub interactive_ready: bool,
    /// The daemon is still starting (or blocked on) the CLI.
    #[serde(default)]
    pub launch_pending: bool,
}

impl AgentInfo {
    /// Whether this agent can take a prompt yet. A freshly started agent is
    /// reported before its CLI settles, and `agent.prompt` refuses it until
    /// then with `agent_not_ready`.
    pub fn is_ready(&self) -> bool {
        self.interactive_ready && !self.launch_pending
    }
}

/// The agent a start produced, plus the argv the daemon launched it with.
#[derive(Clone, Debug, Deserialize)]
pub struct AgentStarted {
    pub agent: AgentInfo,
    #[serde(default)]
    pub argv: Vec<String>,
}

/// A pane read result: the text plus whether the daemon truncated it.
#[derive(Clone, Debug, Deserialize)]
pub struct PaneRead {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub pane_id: Option<String>,
}

/// The pane a split produced.
#[derive(Clone, Debug, Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
    #[serde(default)]
    pub terminal_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EndpointError {
    #[error("connecting to the herdr API socket {path}: {source}")]
    Connect {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the herdr API socket {path} is unusable: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the herdr daemon closed the API socket without a reply")]
    NoReply,
    #[error("the herdr daemon answered with something that is not a reply: {0}")]
    Protocol(String),
    #[error("agent {name} did not become ready within {waited_ms} ms")]
    AgentNotReady { name: String, waited_ms: u64 },
    #[error("{code}: {message}")]
    Api { code: String, message: String },
}

impl EndpointError {
    /// The daemon's error code, when the failure was an API error.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api { code, .. } => Some(code),
            _ => None,
        }
    }
}

/// The orchestrator's client for the daemon's JSON API.
#[derive(Clone, Debug)]
pub struct ApiEndpoint {
    socket: PathBuf,
}

impl ApiEndpoint {
    /// The endpoint for this machine's daemon, resolved the same way the daemon
    /// resolves it (env override, session directory, config root).
    pub fn local() -> Self {
        Self {
            socket: client::herdr_api_socket_path(),
        }
    }

    /// The endpoint for an explicit socket path.
    pub fn at(socket: PathBuf) -> Self {
        Self { socket }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    /// One request, waiting [`DEFAULT_TIMEOUT`] for its reply.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, EndpointError> {
        self.call_within(method, params, DEFAULT_TIMEOUT)
    }

    /// One request, waiting `timeout` for its reply. Each call is its own
    /// connection: the API lane is request/response, not a stream.
    pub fn call_within(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, EndpointError> {
        let mut stream =
            UnixStream::connect(&self.socket).map_err(|source| EndpointError::Connect {
                path: self.socket.clone(),
                source,
            })?;
        let io = |source: std::io::Error| EndpointError::Io {
            path: self.socket.clone(),
            source,
        };
        stream.set_read_timeout(Some(timeout)).map_err(io)?;
        stream.set_write_timeout(Some(timeout)).map_err(io)?;
        stream
            .write_all(request_line(method, params).as_bytes())
            .map_err(io)?;
        stream.write_all(b"\n").map_err(io)?;
        stream.flush().map_err(io)?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => Err(EndpointError::NoReply),
            Ok(_) => parse_reply(&line),
            Err(source) => Err(io(source)),
        }
    }

    /// Every agent the daemon knows about.
    pub fn list_agents(&self) -> Result<Vec<AgentInfo>, EndpointError> {
        let result = self.call("agent.list", json!({}))?;
        decode_field(&result, "agents", Vec::new())
    }

    /// Start an interactive agent CLI inside an existing pane, under `name`.
    pub fn start_agent(
        &self,
        name: &str,
        kind: &str,
        pane_id: &str,
        args: Vec<String>,
        timeout_ms: u64,
    ) -> Result<AgentStarted, EndpointError> {
        let result = self.call(
            "agent.start",
            json!({
              "name": name,
              "kind": kind,
              "pane_id": pane_id,
              "args": args,
              "timeout_ms": timeout_ms,
            }),
        )?;
        decode(&result)
    }

    /// Submit a prompt to an agent, identified by its name or its pane.
    pub fn prompt_agent(&self, target: &str, text: &str) -> Result<AgentInfo, EndpointError> {
        let result = self.call("agent.prompt", json!({"target": target, "text": text}))?;
        required_field(&result, "agent")
    }

    /// One agent, by name or pane.
    pub fn agent(&self, target: &str) -> Result<AgentInfo, EndpointError> {
        let result = self.call("agent.get", json!({"target": target}))?;
        required_field(&result, "agent")
    }

    /// Wait until a started agent's CLI has settled enough to take a prompt.
    ///
    /// `agent.start` returns as soon as the launch command is written; the
    /// daemon only marks the agent active after the CLI reports itself. A
    /// prompt sent in between is refused with `agent_not_ready`, so the wait
    /// belongs here rather than in every caller.
    pub fn wait_for_agent_ready(
        &self,
        name: &str,
        timeout: Duration,
    ) -> Result<AgentInfo, EndpointError> {
        let deadline = Instant::now() + timeout;
        loop {
            // An agent the daemon has not registered yet answers with an error;
            // that is a "not yet", not a failure of the wait.
            if let Ok(agent) = self.agent(name) {
                if agent.is_ready() {
                    return Ok(agent);
                }
            }
            if Instant::now() >= deadline {
                return Err(EndpointError::AgentNotReady {
                    name: name.to_owned(),
                    waited_ms: timeout.as_millis() as u64,
                });
            }
            std::thread::sleep(READY_POLL_INTERVAL);
        }
    }

    /// Read a pane's text: the visible grid, or the last `lines` including
    /// scrollback.
    pub fn read_pane(
        &self,
        pane_id: &str,
        source: ReadSource,
        lines: u32,
    ) -> Result<PaneRead, EndpointError> {
        let result = self.call(
            "pane.read",
            json!({
              "pane_id": pane_id,
              "source": source.as_str(),
              "lines": lines,
              "strip_ansi": true,
              "format": "text",
            }),
        )?;
        required_field(&result, "read")
    }

    /// Ask the daemon to wait until a pane's output contains `pattern`, and
    /// return the pane text as of the match. The wait happens daemon-side.
    pub fn wait_for_output(
        &self,
        pane_id: &str,
        pattern: &str,
        lines: u32,
        timeout_ms: u64,
    ) -> Result<PaneRead, EndpointError> {
        let result = self.call_within(
            "pane.wait_for_output",
            json!({
              "pane_id": pane_id,
              "source": ReadSource::Recent.as_str(),
              "match": {"type": "substring", "value": pattern},
              "lines": lines,
              "strip_ansi": true,
              "timeout_ms": timeout_ms,
            }),
            Duration::from_millis(timeout_ms) + WAIT_SLACK,
        )?;
        required_field(&result, "read")
    }

    /// Split a pane. `workspace_id` and `cwd` are omitted when `None`, which
    /// leaves the daemon to use the focused workspace and its own default cwd.
    pub fn split_pane(
        &self,
        workspace_id: Option<&str>,
        cwd: Option<&str>,
        direction: SplitDirection,
    ) -> Result<PaneInfo, EndpointError> {
        let result = self.call(
            "pane.split",
            json!({
              "direction": direction.as_str(),
              "workspace_id": workspace_id,
              "cwd": cwd,
            }),
        )?;
        required_field(&result, "pane")
    }

    /// Close a pane, which ends whatever runs inside it.
    pub fn close_pane(&self, pane_id: &str) -> Result<(), EndpointError> {
        self.call("pane.close", json!({"pane_id": pane_id}))?;
        Ok(())
    }

    /// Label a pane, so a worker stays recognisable in the layout.
    pub fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), EndpointError> {
        self.call("pane.rename", json!({"pane_id": pane_id, "label": label}))?;
        Ok(())
    }

    /// Write text straight into a pane's input, without the agent-level submit
    /// path `agent.prompt` takes.
    pub fn send_pane_text(&self, pane_id: &str, text: &str) -> Result<(), EndpointError> {
        self.call("pane.send_text", json!({"pane_id": pane_id, "text": text}))?;
        Ok(())
    }
}

/// The one place a request id is minted: request/response pairs are matched by
/// the daemon echoing it back, and a process only needs uniqueness within
/// itself.
fn next_request_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "zenix-ai-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn request_line(method: &str, params: Value) -> String {
    json!({"id": next_request_id(), "method": method, "params": params}).to_string()
}

/// Read one reply line: `{"id": …, "result": …}` on success, `{"id": …,
/// "error": {"code", "message"}}` on failure. Both are the daemon's own wire
/// shapes (`.relay/notes/herdr-api-schema-0.9.3.json`).
fn parse_reply(line: &str) -> Result<Value, EndpointError> {
    let value: Value = serde_json::from_str(line.trim_end())
        .map_err(|err| EndpointError::Protocol(err.to_string()))?;
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        return Err(EndpointError::Api {
            code: error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_owned(),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        });
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

/// Pull `field` out of a reply result and decode it, treating an absent field
/// as `fallback`.
fn decode_field<T: serde::de::DeserializeOwned>(
    result: &Value,
    field: &str,
    fallback: T,
) -> Result<T, EndpointError> {
    match result.get(field) {
        None | Some(Value::Null) => Ok(fallback),
        Some(value) => decode(value),
    }
}

/// Pull a field every successful reply of this method must carry. A daemon that
/// answered without it is a protocol mismatch, not an empty result.
fn required_field<T: serde::de::DeserializeOwned>(
    result: &Value,
    field: &str,
) -> Result<T, EndpointError> {
    match result.get(field) {
        None | Some(Value::Null) => Err(EndpointError::Protocol(format!(
            "the reply for this method carries no {field}"
        ))),
        Some(value) => decode(value),
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, EndpointError> {
    serde_json::from_value(value.clone()).map_err(|err| EndpointError::Protocol(err.to_string()))
}

/// A description of a tool call's endpoint, for logs and for the probe.
#[derive(Clone, Debug, Serialize)]
pub struct EndpointDescription {
    pub socket: PathBuf,
}

impl From<&ApiEndpoint> for EndpointDescription {
    fn from(endpoint: &ApiEndpoint) -> Self {
        Self {
            socket: endpoint.socket_path().to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_line_carries_the_method_and_its_params() {
        let line = request_line("agent.list", json!({}));
        let parsed: Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(parsed["method"], "agent.list");
        assert_eq!(parsed["params"], json!({}));
        assert!(
            parsed["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("zenix-ai-")),
            "every request carries an id the daemon echoes back"
        );
    }

    #[test]
    fn request_ids_are_unique() {
        let first = request_line("ping", json!({}));
        let second = request_line("ping", json!({}));
        assert_ne!(first, second);
    }

    #[test]
    fn a_successful_reply_yields_its_result() {
        let result = parse_reply(r#"{"id":"a","result":{"type":"agent_list","agents":[]}}"#)
            .expect("a result");
        assert_eq!(result["type"], "agent_list");
    }

    #[test]
    fn an_error_reply_becomes_an_api_error_with_its_code() {
        let error = parse_reply(
            r#"{"id":"a","error":{"code":"unsupported_method","message":"not available"}}"#,
        )
        .expect_err("an api error");
        assert_eq!(error.code(), Some("unsupported_method"));
        assert!(error.to_string().contains("not available"));
    }

    #[test]
    fn a_null_error_is_not_an_error() {
        let result =
            parse_reply(r#"{"id":"a","error":null,"result":{"type":"ok"}}"#).expect("a result");
        assert_eq!(result["type"], "ok");
    }

    #[test]
    fn a_reply_that_is_not_json_is_a_protocol_error() {
        let error = parse_reply("not json").expect_err("a protocol error");
        assert!(matches!(error, EndpointError::Protocol(_)));
    }

    #[test]
    fn read_sources_and_split_directions_use_the_daemons_spelling() {
        assert_eq!(ReadSource::Recent.as_str(), "recent");
        assert_eq!(ReadSource::RecentUnwrapped.as_str(), "recent_unwrapped");
        assert_eq!(ReadSource::Visible.as_str(), "visible");
        assert_eq!(ReadSource::Detection.as_str(), "detection");
        assert_eq!(SplitDirection::Right.as_str(), "right");
        assert_eq!(SplitDirection::Down.as_str(), "down");
    }

    #[test]
    fn an_absent_field_falls_back_instead_of_failing() {
        let empty = json!({});
        let agents: Vec<AgentInfo> = decode_field(&empty, "agents", Vec::new()).expect("empty");
        assert!(agents.is_empty());
    }

    #[test]
    fn an_agent_is_ready_only_after_its_cli_settles() {
        let mut agent: AgentInfo = decode(&json!({"pane_id": "w1:p1"})).expect("an agent");
        assert!(!agent.is_ready(), "a fresh agent takes no prompt yet");
        agent.launch_pending = true;
        agent.interactive_ready = true;
        assert!(!agent.is_ready(), "a pending launch is not ready");
        agent.launch_pending = false;
        assert!(agent.is_ready());
    }

    #[test]
    fn an_agent_decodes_the_fields_the_tools_surface() {
        let agent: AgentInfo = decode(&json!({
          "pane_id": "w1:p2",
          "name": "oc-check",
          "agent": "opencode",
          "agent_status": "working",
          "workspace_id": "w1",
          "terminal_id": "term_1",
          "focused": true
        }))
        .expect("an agent");
        assert_eq!(agent.pane_id, "w1:p2");
        assert_eq!(agent.name.as_deref(), Some("oc-check"));
        assert_eq!(agent.agent_status.as_deref(), Some("working"));
        assert!(agent.focused);
    }

    #[test]
    fn the_api_socket_is_the_endpoint_sockets_sibling() {
        // The rule both lanes share: same config root, same session directory,
        // different socket file name.
        let endpoint = PathBuf::from("/home/zero/.config/herdr/herdr-client.sock");
        assert_eq!(
            endpoint.with_file_name("herdr.sock"),
            PathBuf::from("/home/zero/.config/herdr/herdr.sock")
        );
        let session = PathBuf::from("/home/zero/.config/herdr/sessions/work/herdr-client.sock");
        assert_eq!(
            session.with_file_name("herdr.sock"),
            PathBuf::from("/home/zero/.config/herdr/sessions/work/herdr.sock")
        );
    }
}
