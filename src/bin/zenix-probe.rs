//! Daemon endpoint smoke gate: connect to the local herdr daemon and print the
//! Welcome, Snapshot and Surface it sends.
//!
//! Usage: `cargo run --bin zenix-probe` (honours `HERDR_SESSION`,
//! `HERDR_SOCKET_PATH` and `HERDR_CLIENT_SOCKET_PATH` like the daemon does).
//! Exit code is 0 only when all three messages arrived before the deadline.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use herdr_client::protocol::PaneSurfaceFrame;
use herdr_client::{ClientEvent, ConnectOptions, ConnectTarget, connect};

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

    println!(
        "result: welcome={welcome} snapshot={snapshot} surface={surface} elapsed={:?}",
        started.elapsed()
    );
    if welcome && snapshot && surface && !mismatch {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
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
            "  pane {} rect={}x{}@({},{}) scroll={} focused={} alternate_screen={} \
             mouse_reporting={}",
            pane.pane_id,
            pane.rect.width,
            pane.rect.height,
            pane.rect.x,
            pane.rect.y,
            pane.scroll.is_some(),
            pane.focused,
            pane.alternate_screen_active,
            pane.mouse_reporting,
        );
    }
}
