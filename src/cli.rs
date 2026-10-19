//! The binary's non-window verbs.
//!
//! `main` only calls [`crate::run`], and `run` asks here first. An argument that
//! names a verb means this process is a tool rather than a window: its stdin and
//! stdout belong to whoever launched it, so neither the GUI path nor the shell's
//! tracing (which writes to stdout) may be entered.

const USAGE: &str = "\
zenix — terminal workspace manager

Usage:
  zenix            Open (or attach to) the window
  zenix mcp-agent  Serve the one-tool MCP bridge on stdin/stdout
  zenix --help     This text

`zenix mcp-agent` is an MCP server for agent CLIs that speak MCP but have no
shell. It offers one tool, `agent_send`, which submits a prompt to another
agent in the running herdr daemon. A client launches it with:
  {\"command\": \"zenix\", \"args\": [\"mcp-agent\"]}
";

/// Run a verb if the arguments name one, and return the exit code. `None` means
/// this invocation is a window.
pub fn dispatch() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    let verb = args.next()?;
    match verb.as_str() {
        // Nothing else is read: the bridge takes its input from stdin, not from
        // the command line.
        "mcp-agent" => Some(crate::mcp::agent_bridge::run_stdio()),
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Some(0)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usage_names_the_one_verb_a_client_launches() {
        assert!(USAGE.contains("zenix mcp-agent"));
        assert!(USAGE.contains("\"args\": [\"mcp-agent\"]"));
    }
}
