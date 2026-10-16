//! The orchestrator's agent vocabulary on the daemon's JSON API lane.
//!
//! Two lanes lead to the same daemon, and they are not interchangeable. The
//! generation-1 endpoint socket zenix paints through (`herdr-client.sock`)
//! advertises a fixed 40-method set and refuses anything else with
//! `unsupported_method`; it carries no `agent.*`, no `pane.read` and no
//! `pane.wait_for_output`. The API socket beside it is the lane the herdr CLI
//! itself uses, and the whole method table is on it: the agent lifecycle and
//! pane reads the six orchestration tools are defined on, and the plugin
//! surface the shell hosts.
//!
//! Evidence and line references: `.relay/notes/herdr-agent-api-lane.md`.
//!
//! The socket, the framing and the reply shape are [`crate::herdr::api`]'s; what
//! this module adds is the agent lifecycle built on top of them.

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;

use crate::herdr::api::{ApiClient, decode_field, required_field};

/// The API-lane client, under the name the orchestration side has always used.
pub type ApiEndpoint = ApiClient;

/// The API lane's error, under the name the orchestration side has always used.
pub use crate::herdr::api::ApiError as EndpointError;

/// Slack added to a daemon-side wait so its own timeout answer arrives first.
const WAIT_SLACK: Duration = Duration::from_secs(5);

/// How often a readiness wait asks the daemon again. The daemon marks a started
/// agent active on the order of its own settle delay, which is 100 ms.
const READY_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Where a pane read takes its text from. The daemon also spells `visible`,
/// `recent_unwrapped` and `detection`; only the scrollback read has a caller so
/// far, and a second variant arrives with its first use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadSource {
    /// The last `lines` of the pane plus its scrollback.
    Recent,
}

impl ReadSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recent => "recent",
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

/// One running agent, as `agent.list` / `agent.get` report it. Only the fields
/// the tools surface are named; the daemon sends more and they are ignored.
#[derive(Clone, Debug, Deserialize)]
pub struct AgentInfo {
    pub pane_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub terminal_id: Option<String>,
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

/// A pane read result. The daemon reports more (its revision, the tab it
/// belongs to, whether it truncated); only the text has a caller so far.
#[derive(Clone, Debug, Deserialize)]
pub struct PaneRead {
    #[serde(default)]
    pub text: String,
}

/// The pane a split produced.
#[derive(Clone, Debug, Deserialize)]
pub struct PaneInfo {
    pub pane_id: String,
}

impl ApiEndpoint {
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
    ) -> Result<AgentInfo, EndpointError> {
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
        required_field(&result, "agent")
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
            if let Ok(agent) = self.agent(name)
                && agent.is_ready()
            {
                return Ok(agent);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn read_sources_and_split_directions_use_the_daemons_spelling() {
        assert_eq!(ReadSource::Recent.as_str(), "recent");
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
        let mut agent: AgentInfo =
            crate::herdr::api::decode(&json!({"pane_id": "w1:p1"})).expect("an agent");
        assert!(!agent.is_ready(), "a fresh agent takes no prompt yet");
        agent.launch_pending = true;
        agent.interactive_ready = true;
        assert!(!agent.is_ready(), "a pending launch is not ready");
        agent.launch_pending = false;
        assert!(agent.is_ready());
    }

    #[test]
    fn an_agent_decodes_the_fields_the_tools_surface() {
        let agent: AgentInfo = crate::herdr::api::decode(&json!({
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
        assert_eq!(agent.workspace_id.as_deref(), Some("w1"));
    }

    #[test]
    fn the_endpoint_builds_for_an_explicit_socket() {
        let endpoint = ApiEndpoint::at(PathBuf::from("/tmp/zenix-endpoint-test.sock"));
        assert_eq!(
            endpoint.socket_path(),
            std::path::Path::new("/tmp/zenix-endpoint-test.sock")
        );
    }
}
