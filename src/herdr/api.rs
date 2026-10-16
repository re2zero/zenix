//! The daemon's JSON API lane: the socket, one request in and one reply line
//! back.
//!
//! Two lanes lead to the same daemon, and they are not interchangeable. The
//! generation-1 endpoint socket zenix paints through (`herdr-client.sock`)
//! advertises a fixed 40-method set and refuses anything else with
//! `unsupported_method`; it carries no `agent.*`, no `pane.read` and no
//! `plugin.*`. The API socket beside it is the lane the herdr CLI itself uses:
//! one newline-delimited JSON request per connection, one line of JSON back,
//! and the whole method table — the agent lifecycle, pane reads and the plugin
//! surface the shell hosts.
//!
//! Evidence and line references: `.relay/notes/herdr-agent-api-lane.md`.
//!
//! What lives here is the transport and nothing else: where the socket is, how
//! a call is framed and how a reply is read. The agent vocabulary built on it
//! sits in `crate::ai::endpoint`, beside its only consumer.
//!
//! Everything here is synchronous and one-shot: a call connects, writes a line,
//! reads a line and closes. There is no shared connection state to guard, which
//! is why a caller can hold this by value and call it from a synchronous path.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::client;

/// How long to wait for a reply that the daemon is expected to produce at once.
/// Waits the daemon performs on the caller's behalf (`pane.wait_for_output`, an
/// `agent.prompt` with a submit deadline) pass their own budget instead.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
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

impl ApiError {
    /// The daemon's error code, when the failure was an API error.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api { code, .. } => Some(code),
            _ => None,
        }
    }
}

/// A client for the daemon's JSON API socket.
#[derive(Clone, Debug)]
pub struct ApiClient {
    socket: PathBuf,
}

impl ApiClient {
    /// The client for this machine's daemon, resolved the same way the daemon
    /// resolves it (env override, session directory, config root).
    pub fn local() -> Self {
        Self {
            socket: client::herdr_api_socket_path(),
        }
    }

    /// The client for an explicit socket path.
    pub fn at(socket: PathBuf) -> Self {
        Self { socket }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    /// One request, waiting [`DEFAULT_TIMEOUT`] for its reply.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, ApiError> {
        self.call_within(method, params, DEFAULT_TIMEOUT)
    }

    /// One request, waiting `timeout` for its reply. Each call is its own
    /// connection: the API lane is request/response, not a stream.
    pub fn call_within(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, ApiError> {
        let mut stream = UnixStream::connect(&self.socket).map_err(|source| ApiError::Connect {
            path: self.socket.clone(),
            source,
        })?;
        let io = |source: std::io::Error| ApiError::Io {
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
            Ok(0) => Err(ApiError::NoReply),
            Ok(_) => parse_reply(&line),
            Err(source) => Err(io(source)),
        }
    }
}

/// The one place a request id is minted: request/response pairs are matched by
/// the daemon echoing it back, and a process only needs uniqueness within
/// itself.
fn next_request_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "zenix-{}-{}",
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
fn parse_reply(line: &str) -> Result<Value, ApiError> {
    let value: Value =
        serde_json::from_str(line.trim_end()).map_err(|err| ApiError::Protocol(err.to_string()))?;
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        return Err(ApiError::Api {
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
pub(crate) fn decode_field<T: serde::de::DeserializeOwned>(
    result: &Value,
    field: &str,
    fallback: T,
) -> Result<T, ApiError> {
    match result.get(field) {
        None | Some(Value::Null) => Ok(fallback),
        Some(value) => decode(value),
    }
}

/// Pull a field every successful reply of this method must carry. A daemon that
/// answered without it is a protocol mismatch, not an empty result.
pub(crate) fn required_field<T: serde::de::DeserializeOwned>(
    result: &Value,
    field: &str,
) -> Result<T, ApiError> {
    match result.get(field) {
        None | Some(Value::Null) => Err(ApiError::Protocol(format!(
            "the reply for this method carries no {field}"
        ))),
        Some(value) => decode(value),
    }
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, ApiError> {
    serde_json::from_value(value.clone()).map_err(|err| ApiError::Protocol(err.to_string()))
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
                .is_some_and(|id| id.starts_with("zenix-")),
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
        assert!(matches!(error, ApiError::Protocol(_)));
    }

    #[test]
    fn an_absent_field_falls_back_instead_of_failing() {
        let empty = json!({});
        let plugins: Vec<Value> = decode_field(&empty, "plugins", Vec::new()).expect("empty");
        assert!(plugins.is_empty());
    }

    #[test]
    fn a_missing_required_field_is_a_protocol_error() {
        let error = required_field::<Value>(&json!({}), "plugins").expect_err("a protocol error");
        assert!(matches!(error, ApiError::Protocol(_)));
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
