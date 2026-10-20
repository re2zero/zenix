//! Daemon smoke gate: connect to the local herdr daemon and print the Welcome,
//! Snapshot and Surface it sends, then prove the orchestration lane answers.
//!
//! Usage: `cargo run --bin zenix-probe` (honours `HERDR_SESSION`,
//! `HERDR_SOCKET_PATH` and `HERDR_CLIENT_SOCKET_PATH` like the daemon does).
//! Exit code is 0 only when all three endpoint messages arrived before the
//! deadline and the daemon's JSON API socket answered `agent.list`.
//!
//! Both lanes are checked because they are not interchangeable: the endpoint
//! lane carries rendering and input but refuses `agent.*` / `pane.read` with
//! `unsupported_method`, and the orchestration tools are defined on the API
//! lane. See `.relay/notes/herdr-agent-api-lane.md`.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use herdr_client::protocol::PaneSurfaceFrame;
use herdr_client::{ClientEvent, ConnectOptions, ConnectTarget, connect};
use zenix::ai::endpoint::ApiEndpoint;

/// Budget for the full handshake plus the first pushed surface.
const DEADLINE: Duration = Duration::from_secs(15);

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zenix_probe=info,herdr_client=info".into()),
        )
        .init();

    let target = ConnectTarget::Local;
    let socket = target.socket_path();

    match &socket {
        Ok(path) => println!("endpoint: {} ({target:?})", path.display()),
        Err(err) => {
            eprintln!("endpoint resolution failed: {err}");
            return ExitCode::FAILURE;
        }
    }

    let client = match connect(target, ConnectOptions::default()) {
        Ok(client) => client,
        Err(err) => {
            eprintln!("connect failed: {err}");
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let mut welcome = false;
    let mut snapshot = false;
    let mut surface = false;
    let mut mismatch = false;

    // The daemon on this machine need not be the release this client pinned.
    // Generation 1 keeps them talking, but the seed and the running daemon
    // disagreeing silently is what the pin exists to stop, so it is printed
    // rather than assumed.
    match daemon_version() {
        Some(version) => {
            let pinned = zenix::herdr::UPSTREAM_VERSION;
            if version == pinned {
                println!("version: daemon={version} pinned={pinned} match=true");
            } else {
                println!("version: daemon={version} pinned={pinned} match=false");
                eprintln!(
                    "warning: the running daemon is herdr {version}, this client was built \
                     against {pinned} (vendor/herdr-pins.tsv)"
                );
            }
        }
        None => println!(
            "version: daemon=unknown pinned={} match=unknown",
            zenix::herdr::UPSTREAM_VERSION
        ),
    }

    while !(welcome && snapshot && surface) {
        let remaining = DEADLINE.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            eprintln!("deadline reached after {:?}", started.elapsed());
            break;
        }
        match client.events.recv_timeout(remaining) {
            Ok(ClientEvent::Connected(welcome_message)) => {
                welcome = true;
                println!(
                    "welcome: generation={} server_version={} tools={} capabilities={} \
                     codecs={{snapshot={}, surface={}, input={}, blob={}}}",
                    welcome_message.generation,
                    welcome_message.server_version,
                    welcome_message.methods.len(),
                    welcome_message.capabilities.len(),
                    welcome_message.snapshot_codec,
                    welcome_message.surface_codec,
                    welcome_message.input_codec,
                    welcome_message.blob_codec,
                );
                if let Some(error) = &welcome_message.error {
                    eprintln!("handshake error: {} {}", error.code, error.message);
                }
            }
            Ok(ClientEvent::Snapshot(frame)) => {
                snapshot = true;
                let focused_pane = frame.focused_pane_id.as_deref().unwrap_or("-");
                println!(
                    "snapshot: boot_id={} revision={} workspaces={} tabs={} panes={} agents={} \
                     commands={} focused={{workspace={}, tab={}, pane={}}}",
                    frame.boot_id,
                    frame.revision,
                    frame.workspaces.len(),
                    frame.tabs.len(),
                    frame.panes.len(),
                    frame.agents.len(),
                    frame.commands.len(),
                    frame.focused_workspace_id.as_deref().unwrap_or("-"),
                    frame.focused_tab_id.as_deref().unwrap_or("-"),
                    focused_pane,
                );
                for workspace in &frame.workspaces {
                    println!(
                        "  workspace {} number={} label={:?} cwd={} branch={} focused={}",
                        workspace.workspace_id,
                        workspace.number,
                        workspace.label,
                        workspace.new_workspace_cwd,
                        workspace.branch.as_deref().unwrap_or("-"),
                        workspace.focused,
                    );
                }
                // The cockpit's roster follows this order. An active agent view
                // (`agent.view.set`) is what makes it differ from the snapshot's
                // own pane order.
                println!(
                    "  agent view: label={} order=[{}]",
                    frame.agent_view_label.as_deref().unwrap_or("-"),
                    frame.agent_order.join(", "),
                );
            }
            Ok(ClientEvent::Surface(frame)) => {
                surface = true;
                print_surface(&frame);
            }
            Ok(ClientEvent::VersionMismatch(found)) => {
                mismatch = true;
                eprintln!("version mismatch: {found:?}");
            }
            Ok(ClientEvent::Disconnected { reason, .. }) => {
                eprintln!("disconnected: {reason}");
                break;
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("event stream ended: {error}");
                break;
            }
        }
    }

    client.handle.disconnect();

    // The orchestration lane: `agent.list` is served only here, and the tools
    // the MCP server exposes are defined on it.
    let api = ApiEndpoint::local();
    let api_lane = match api.list_agents() {
        Ok(agents) => {
            println!(
                "api: socket={} agent.list={} agents",
                api.socket_path().display(),
                agents.len()
            );
            true
        }
        Err(error) => {
            eprintln!(
                "api: {} did not answer: {error}",
                api.socket_path().display()
            );
            false
        }
    };

    println!(
        "result: welcome={welcome} snapshot={snapshot} surface={surface} api={api_lane} \
         elapsed={:?}",
        started.elapsed()
    );
    if welcome && snapshot && surface && api_lane && !mismatch {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The version the daemon binary reports, when one can be run.
///
/// `HERDR_BINARY` is the copy `build.rs` placed next to this binary; a build
/// without a seed has no value there, and the daemon on PATH is the next best
/// answer.
fn daemon_version() -> Option<String> {
    let binary = option_env!("HERDR_BINARY").unwrap_or("herdr");
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace().nth(1).map(str::to_owned)
}

fn print_surface(frame: &PaneSurfaceFrame) {
    let cursor = frame
        .frame
        .cursor
        .as_ref()
        .map(|cursor| format!("{}x{}", cursor.x, cursor.y))
        .unwrap_or_else(|| "-".into());
    println!(
        "surface: boot_id={} projection_revision={} surface_revision={} grid={}x{} cells={} \
         panes={} splits={} popup={} graphics_placements={} cursor={}",
        frame.boot_id,
        frame.projection_revision,
        frame.surface_revision,
        frame.frame.width,
        frame.frame.height,
        frame.frame.cells.len(),
        frame.panes.len(),
        frame.splits.len(),
        frame.popup.is_some(),
        frame.graphics.placements.len(),
        cursor,
    );
    for pane in &frame.panes {
        println!(
            "  pane {} rect={}x{}@({},{}) inner={}x{}@({},{}) scroll={} focused={} \
             alternate_screen={} mouse_reporting={}",
            pane.pane_id,
            pane.rect.width,
            pane.rect.height,
            pane.rect.x,
            pane.rect.y,
            pane.inner_rect.width,
            pane.inner_rect.height,
            pane.inner_rect.x,
            pane.inner_rect.y,
            pane.scroll.is_some(),
            pane.focused,
            pane.alternate_screen_active,
            pane.mouse_reporting,
        );
    }
}
