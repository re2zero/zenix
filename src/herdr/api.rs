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
//!
//! [`ApiClient::subscribe`] is the one method that does not close: a
//! subscription is a connection the daemon keeps writing on, so it hands the
//! reader back instead of a decoded reply. Nothing else here knows about it.

use std::io::{BufRead, BufReader, Read, Write};
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

    /// Ask for a subscription and read the daemon's acknowledgement. The
    /// connection stays open after that and carries event lines rather than
    /// replies, which is why this hands back a reader: the daemon writes one
    /// more reply only to refuse the subscription, and that refusal comes back
    /// as an error here.
    pub fn subscribe(&self, params: Value) -> Result<ApiStream, ApiError> {
        let mut stream = UnixStream::connect(&self.socket).map_err(|source| ApiError::Connect {
            path: self.socket.clone(),
            source,
        })?;
        let io = |source: std::io::Error| ApiError::Io {
            path: self.socket.clone(),
            source,
        };
        stream
            .set_write_timeout(Some(DEFAULT_TIMEOUT))
            .map_err(io)?;
        stream
            .write_all(request_line("events.subscribe", params).as_bytes())
            .map_err(io)?;
        stream.write_all(b"\n").map_err(io)?;
        stream.flush().map_err(io)?;

        // No read timeout: a subscription is idle until the daemon has something
        // to say, and a stream that gave up after thirty seconds of quiet would
        // report a working daemon as gone.
        let mut subscription = ApiStream {
            path: self.socket.clone(),
            stream,
            pending: Vec::new(),
        };
        let first = subscription.next_line()?.ok_or(ApiError::NoReply)?;
        let acknowledged = parse_reply(&first)?;
        accept_subscription(&acknowledged)?;
        Ok(subscription)
    }
}

/// A subscription is only started once the daemon says so. Anything else on
/// that first line means this is not the stream it was asked to be.
fn accept_subscription(result: &Value) -> Result<(), ApiError> {
    match result.get("type").and_then(Value::as_str) {
        Some("subscription_started") => Ok(()),
        other => Err(ApiError::Protocol(format!(
            "the daemon answered {other:?} to a subscription"
        ))),
    }
}

/// What a poll of a subscription produced.
#[derive(Debug)]
pub enum Line {
    /// One line the daemon wrote.
    Line(String),
    /// Nothing arrived within the poll's deadline. The stream is still open.
    Idle,
    /// The daemon closed the connection.
    Closed,
}

/// A subscription the daemon is still writing on.
#[derive(Debug)]
pub struct ApiStream {
    path: PathBuf,
    stream: UnixStream,
    /// Bytes read but not yet a whole line: a line can arrive in pieces, and a
    /// poll that gives up half way through one must not lose it.
    pending: Vec<u8>,
}

impl ApiStream {
    /// The next line the daemon wrote. `None` is the daemon closing the
    /// connection, which is how a subscription ends: it stopped, or it dropped
    /// this subscriber for falling behind its retained history.
    pub fn next_line(&mut self) -> Result<Option<String>, ApiError> {
        loop {
            match self.poll_within(None)? {
                Line::Line(line) => return Ok(Some(line)),
                Line::Closed => return Ok(None),
                // Without a deadline the read only returns for one of the two
                // answers above.
                Line::Idle => {}
            }
        }
    }

    /// The next line, or [`Line::Idle`] when `timeout` passes first. A
    /// subscription whose set of interests changes — one connection cannot be
    /// renegotiated — uses this to notice, close and subscribe again.
    pub fn next_line_within(&mut self, timeout: Duration) -> Result<Line, ApiError> {
        self.poll_within(Some(timeout))
    }

    fn poll_within(&mut self, timeout: Option<Duration>) -> Result<Line, ApiError> {
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=end).collect();
                return Ok(Line::Line(String::from_utf8_lossy(&line).into_owned()));
            }
            self.stream
                .set_read_timeout(timeout)
                .map_err(|source| ApiError::Io {
                    path: self.path.clone(),
                    source,
                })?;
            let mut chunk = [0u8; 8192];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Ok(Line::Closed),
                Ok(read) => self.pending.extend_from_slice(&chunk[..read]),
                Err(source)
                    if matches!(
                        source.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(Line::Idle);
                }
                Err(source) => {
                    return Err(ApiError::Io {
                        path: self.path.clone(),
                        source,
                    });
                }
            }
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
    fn only_a_started_subscription_opens_a_stream() {
        let started = json!({"type": "subscription_started"});
        assert!(accept_subscription(&started).is_ok());
        let other = json!({"type": "pong"});
        assert!(accept_subscription(&other).is_err());
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
