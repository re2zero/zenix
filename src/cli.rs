//! The binary's non-window verbs, and the arguments a window launch carries.
//!
//! `main` only calls [`crate::run`], and `run` asks here first. A verb
//! (`mcp-agent`, `--help`, `--version`) means this process is a tool rather
//! than a window: its stdin and stdout belong to whoever launched it, so
//! neither the GUI path nor the shell's tracing (which writes to stdout) may
//! be entered. Otherwise the arguments name the daemon connection the window
//! opens on — the default local daemon, an explicit session, an endpoint
//! socket, or a remote host over SSH — and [`Launch::Window`] carries it.
//!
//! The same connection, as a one-line word, is what a second `zenix` process
//! sends the running instance over the single-instance socket
//! ([`crate::single_instance`]); [`encode_launch`] / [`decode_launch`] are
//! that grammar's two halves.

use herdr_client::ConnectTarget;

const USAGE: &str = "\
zenix — terminal workspace manager

Usage:
  zenix                        Open a window on the local daemon; when one
                               is already running, that instance opens one
                               more window instead
  zenix --session <name>       Open a window on a named local daemon session
  zenix --socket <path>        Open a window on an explicit endpoint socket;
                               the JSON API socket is expected beside it
  zenix --ssh <target>         Open a window attached to a remote host's
  [--session <name>]           herdr over SSH; the surfaces that read the
                               JSON API socket stay local-window only
  zenix --focus                Raise a running window instead of opening
  [--session|--socket|--ssh …] one; naming a daemon raises that daemon's
                               window, bare raises the frontmost one. When
                               nothing is running, this opens a window.
  zenix mcp-agent              Serve the one-tool MCP bridge on stdin/stdout
  zenix --version              Print the version
  zenix --help                 This text

`zenix mcp-agent` is an MCP server for agent CLIs that speak MCP but have no
shell. It offers one tool, `agent_send`, which submits a prompt to another
agent in the running herdr daemon. A client launches it with:
  {\"command\": \"zenix\", \"args\": [\"mcp-agent\"]}
";

/// What this invocation asks the process to be.
pub enum Launch {
    /// The MCP bridge tool: run it and exit with its code.
    McpAgent,
    Help,
    Version,
    /// Open a window — the first of a new process, or one more in the
    /// running instance — connected to this daemon.
    Window(ConnectTarget),
    /// Raise a window that is already open: any of them, or the one on this
    /// daemon. A process with no windows yet opens the named target instead.
    Focus(Option<ConnectTarget>),
}

/// Read one invocation. A flag without its value, or a flag nothing answers,
/// is the error string `run` prints before exiting.
pub fn parse<I>(args: I) -> Result<Launch, String>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    match args.first() {
        None => return Ok(Launch::Window(ConnectTarget::Local)),
        // Nothing else is read for the bridge: its input is stdin, not the
        // command line.
        Some(v) => match v.as_str() {
            "mcp-agent" => return Ok(Launch::McpAgent),
            "-h" | "--help" | "help" => return Ok(Launch::Help),
            "-V" | "--version" => return Ok(Launch::Version),
            _ => {}
        },
    }
    let mut target: Option<ConnectTarget> = None;
    let mut focus = false;
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        // `--focus` names no target of its own; the target flags beside it do.
        if flag == "--focus" {
            focus = true;
            i += 1;
            continue;
        }
        let Some(value) = args.get(i + 1) else {
            return Err(format!("{flag} needs a value (see `zenix --help`)"));
        };
        let value = value.clone();
        match flag {
            "--ssh" => {
                target = Some(ConnectTarget::Ssh {
                    target: value,
                    session: String::from("default"),
                });
            }
            // A --session beside --ssh names the remote session; on its own
            // it names a local daemon session.
            "--session" => match &mut target {
                Some(ConnectTarget::Ssh { session, .. }) => *session = value,
                Some(_) => {
                    return Err("--session cannot follow --socket (see `zenix --help`)".into());
                }
                None => {
                    target = Some(ConnectTarget::Session {
                        name: value,
                        development: false,
                    });
                }
            },
            "--socket" => {
                if target.is_some() {
                    return Err("--socket cannot follow another target flag".into());
                }
                target = Some(ConnectTarget::Socket(value.into()));
            }
            _ => return Err(format!("unknown flag `{flag}` (see `zenix --help`)")),
        }
        i += 2;
    }
    Ok(match (focus, target) {
        (true, target) => Launch::Focus(target),
        (false, Some(target)) => Launch::Window(target),
        (false, None) => Launch::Window(ConnectTarget::Local),
    })
}

/// The single-instance wire word for a launch: fields separated by tabs,
/// because a session name cannot hold one and an SSH target never would.
pub(crate) fn encode_launch(target: &ConnectTarget) -> String {
    match target {
        ConnectTarget::Local => String::from("open"),
        ConnectTarget::Session { name, .. } => format!("session\t{name}"),
        ConnectTarget::Socket(path) => format!("socket\t{}", path.display()),
        ConnectTarget::Ssh { target, session } => format!("ssh\t{target}\t{session}"),
        ConnectTarget::Wsl { distro, session } => format!("wsl\t{distro}\t{session}"),
    }
}

/// The single-instance wire word for a whole launch — the ask a second
/// process forwards, focus included. A verb (`mcp-agent`, `--help`) is not
/// a window ask and answers `None`.
pub(crate) fn encode_forward(launch: &Launch) -> Option<String> {
    match launch {
        Launch::Window(target) => Some(encode_launch(target)),
        Launch::Focus(None) => Some(String::from("focus")),
        Launch::Focus(Some(target)) => Some(format!("focus\t{}", encode_launch(target))),
        _ => None,
    }
}

/// What a forwarded word asks the running instance for.
pub(crate) enum Forward {
    /// Open one more window on the target the word carries.
    Open(ConnectTarget),
    /// Raise a window instead: any of them, or one on the named daemon.
    Focus(Option<ConnectTarget>),
}

/// The other half of [`encode_forward`]. A word this build cannot read
/// answers `None`, and the ask is dropped rather than guessed at.
pub(crate) fn decode_forward(word: &str) -> Option<Forward> {
    let (verb, rest) = match word.split_once('\t') {
        Some((verb, rest)) => (verb, Some(rest)),
        None => (word, None),
    };
    match verb {
        // `focus` alone raises the frontmost window; a target word after it
        // names the daemon whose window should come up.
        "focus" => Some(Forward::Focus(match rest {
            None => None,
            Some(rest) => Some(decode_launch(rest)?),
        })),
        _ => Some(Forward::Open(decode_launch(word)?)),
    }
}

/// The other half of [`encode_launch`]. A word this build cannot read
/// answers `None`, and the ask is dropped rather than guessed at.
fn decode_launch(word: &str) -> Option<ConnectTarget> {
    let mut fields = word.split('\t');
    match fields.next()? {
        "open" => Some(ConnectTarget::Local),
        "session" => Some(ConnectTarget::Session {
            name: fields.next()?.to_owned(),
            development: false,
        }),
        "socket" => Some(ConnectTarget::Socket(fields.next()?.into())),
        "ssh" => Some(ConnectTarget::Ssh {
            target: fields.next()?.to_owned(),
            session: fields.next().unwrap_or("default").to_owned(),
        }),
        "wsl" => Some(ConnectTarget::Wsl {
            distro: fields.next()?.to_owned(),
            session: fields.next().unwrap_or("default").to_owned(),
        }),
        _ => None,
    }
}

/// Run a verb if the arguments name one, and return the exit code. `None` means
/// this invocation is a window, and [`Launch::Window`] from [`parse`] is the
/// target it opens on.
pub fn dispatch() -> Option<i32> {
    match parse(std::env::args().skip(1)) {
        Ok(Launch::McpAgent) => Some(crate::mcp::agent_bridge::run_stdio()),
        Ok(Launch::Help) => {
            print!("{USAGE}");
            Some(0)
        }
        Ok(Launch::Version) => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            Some(0)
        }
        Ok(Launch::Window(_) | Launch::Focus(_)) => None,
        Err(message) => {
            eprintln!("zenix: {message}");
            Some(2)
        }
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

    /// `--version` reads the manifest's own version, so there is no second
    /// string that could drift from what the package is built as.
    #[test]
    fn the_usage_documents_the_version_verb() {
        assert!(USAGE.contains("zenix --version"));
    }

    fn parse_from(args: &[&str]) -> Result<Launch, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn a_bare_launch_is_a_local_window() {
        assert!(matches!(
            parse_from(&[]),
            Ok(Launch::Window(ConnectTarget::Local))
        ));
    }

    #[test]
    fn the_flags_name_their_targets() {
        assert!(matches!(
            parse_from(&["--session", "dev"]),
            Ok(Launch::Window(ConnectTarget::Session { .. }))
        ));
        assert!(matches!(
            parse_from(&["--socket", "/tmp/s.sock"]),
            Ok(Launch::Window(ConnectTarget::Socket(_)))
        ));
        let Ok(Launch::Window(ConnectTarget::Ssh { target, session })) =
            parse_from(&["--ssh", "ops@box"])
        else {
            panic!("--ssh must parse");
        };
        assert_eq!(target, "ops@box");
        assert_eq!(session, "default");
    }

    #[test]
    fn a_session_after_ssh_names_the_remote_session() {
        let Ok(Launch::Window(ConnectTarget::Ssh { target, session })) =
            parse_from(&["--ssh", "ops@box", "--session", "prod"])
        else {
            panic!("--ssh --session must parse");
        };
        assert_eq!(target, "ops@box");
        assert_eq!(session, "prod");
    }

    #[test]
    fn a_flag_without_a_value_is_a_usage_error() {
        assert!(parse_from(&["--ssh"]).is_err());
        assert!(parse_from(&["--session"]).is_err());
        assert!(parse_from(&["--what"]).is_err());
    }

    #[test]
    fn a_launch_round_trips_through_the_wire_word() {
        let targets = [
            ConnectTarget::Local,
            ConnectTarget::Session {
                name: "dev".into(),
                development: false,
            },
            ConnectTarget::Socket("/tmp/herdr/herdr-client.sock".into()),
            ConnectTarget::Ssh {
                target: "ops@box.example".into(),
                session: "default".into(),
            },
            ConnectTarget::Wsl {
                distro: "Ubuntu".into(),
                session: "default".into(),
            },
        ];
        for target in &targets {
            let back = decode_launch(&encode_launch(target)).expect("every encoded launch decodes");
            assert_eq!(back, *target, "word `{}`", encode_launch(target));
        }
    }

    #[test]
    fn the_retired_focus_word_now_raises_a_window() {
        // Earlier builds sent bare `focus` for this ask; it means the same
        // thing again — raise, not open.
        assert!(matches!(
            decode_forward("focus"),
            Some(Forward::Focus(None))
        ));
    }

    #[test]
    fn a_focus_ask_round_trips_with_its_target() {
        let asks = [
            (Launch::Focus(None), "focus".to_string()),
            (
                Launch::Focus(Some(ConnectTarget::Session {
                    name: "dev".into(),
                    development: false,
                })),
                "focus\tsession\tdev".to_string(),
            ),
            (
                Launch::Focus(Some(ConnectTarget::Ssh {
                    target: "ops@box".into(),
                    session: "prod".into(),
                })),
                "focus\tssh\tops@box\tprod".to_string(),
            ),
        ];
        for (launch, word) in &asks {
            assert_eq!(&encode_forward(launch).expect("a window ask encodes"), word);
            let Launch::Focus(expected) = launch else {
                panic!("the ask was a focus");
            };
            match decode_forward(word) {
                Some(Forward::Focus(which)) => assert_eq!(which, *expected, "word `{word}`"),
                _ => panic!("`{word}` did not decode as a focus"),
            }
        }
    }

    #[test]
    fn an_open_ask_still_round_trips_through_the_forward() {
        let word = "session\tdev";
        assert!(matches!(
            decode_forward(word),
            Some(Forward::Open(ConnectTarget::Session { .. }))
        ));
        assert_eq!(
            encode_forward(&Launch::Window(ConnectTarget::Local)).as_deref(),
            Some("open")
        );
    }

    #[test]
    fn a_word_nobody_reads_is_dropped() {
        assert!(decode_forward("gamma\t1").is_none());
        assert!(decode_forward("focus\tgamma").is_none());
    }

    #[test]
    fn a_focus_flag_alone_names_no_target() {
        assert!(matches!(parse_from(&["--focus"]), Ok(Launch::Focus(None))));
    }

    #[test]
    fn a_focus_ask_names_its_daemon_in_any_flag_order() {
        for args in [
            vec!["--focus", "--session", "dev"],
            vec!["--session", "dev", "--focus"],
        ] {
            let Ok(parsed) = parse_from(&args) else {
                panic!("`{args:?}` must parse");
            };
            assert!(matches!(
                parsed,
                Launch::Focus(Some(ConnectTarget::Session { .. }))
            ));
        }
    }

    #[test]
    fn a_focus_ask_keeps_the_value_flags_working() {
        let Ok(Launch::Focus(Some(ConnectTarget::Ssh { target, .. }))) =
            parse_from(&["--focus", "--ssh", "ops@box"])
        else {
            panic!("--focus --ssh must parse");
        };
        assert_eq!(target, "ops@box");
        assert!(parse_from(&["--focus", "--what"]).is_err());
    }
}
