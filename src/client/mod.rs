//! Herdr binary management: bundled binary, first-run install, PATH fallback.

use std::path::PathBuf;
use std::process::Stdio;

use crate::platform;

/// Path to the herdr binary built/copied at compile time (dev/test convenience).
const BUNDLED_HERDR: &str = env!("HERDR_BINARY");

/// User-local install path — herdr self-updates from here.
fn user_herdr_path() -> PathBuf {
    platform::local_bin_dir().join(platform::herdr_binary_name())
}

/// Ensure herdr is available, installing to ~/.local/bin/herdr on first run.
/// Returns the path to a usable herdr binary, or None.
pub fn ensure_herdr() -> Option<PathBuf> {
    // 1. Already installed at the preferred user-local path
    let user = user_herdr_path();
    if user.is_file() {
        return Some(user);
    }

    // 2. Bundled binary (dev: copied by build.rs; release: alongside zenix)
    let bundled = PathBuf::from(BUNDLED_HERDR);
    if bundled.is_file() {
        return Some(bundled);
    }

    // 3. PATH lookup
    if let Ok(path) = std::env::var("PATH") {
        let name = platform::herdr_binary_name();
        for dir in std::env::split_paths(&path) {
            let c = dir.join(name);
            if c.is_file() {
                return Some(c);
            }
        }
    }

    // 4. Seed binary from package install — copy to user-local path
    let seed = platform::seed_herdr();
    if seed.is_file() {
        if let Some(parent) = user.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::copy(&seed, &user).is_ok() {
            // Make it executable (copy preserves mode from seed)
            tracing::info!("installed herdr to {}", user.display());
            return Some(user);
        }
    }

    None
}
/// Find the herdr binary (legacy — prefer `ensure_herdr()`).
pub fn find_herdr_binary() -> Option<PathBuf> {
    ensure_herdr()
}

/// Path of the daemon's endpoint socket — the `herdr-client.sock` that speaks
/// the generation-1 endpoint protocol, never the JSON API `herdr.sock`.
///
/// Resolution is delegated to the vendored endpoint client, so it follows the
/// daemon's own rules: `HERDR_SOCKET_PATH` / `HERDR_CLIENT_SOCKET_PATH`
/// overrides, then `HERDR_SESSION` (else the default session) under the config
/// root. An unusable override degrades to the default config root.
pub fn herdr_socket_path() -> PathBuf {
    match herdr_client::ConnectTarget::Local.socket_path() {
        Ok(path) => path,
        Err(err) => {
            tracing::warn!(%err, "ignoring herdr socket override");
            platform::config_dir().join("herdr/herdr-client.sock")
        }
    }
}

/// Path of the daemon's JSON API socket — the `herdr.sock` the herdr CLI itself
/// talks to, and the only lane that advertises the agent and pane-read methods
/// the orchestration tools need. The endpoint socket beside it
/// (`herdr-client.sock`) speaks generation 1 with a fixed 40-method set and
/// refuses everything else; see `.relay/notes/herdr-agent-api-lane.md`.
///
/// Resolution mirrors the endpoint socket so both lanes reach the same daemon:
/// `HERDR_SOCKET_PATH` names this socket directly, otherwise it is the endpoint
/// socket's sibling under the same config root and session directory.
pub fn herdr_api_socket_path() -> PathBuf {
    if let Ok(path) = std::env::var("HERDR_SOCKET_PATH") {
        return PathBuf::from(path);
    }
    herdr_socket_path().with_file_name("herdr.sock")
}

/// Start the herdr server in the background.
/// Returns true if the process was spawned successfully.
///
/// The daemon's own output is appended to [`daemon_log_path`] instead of being
/// thrown away: a start that fails has no other witness, and the first-run
/// card offers the tail of that file beside the reason. `stdin` is null so the
/// server is fully detached from the window.
pub fn start_herdr_server() -> bool {
    let binary = match ensure_herdr() {
        Some(path) => path,
        None => return false,
    };

    let log = daemon_log_path();
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let sink = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .ok();
    let (stdout, stderr) = match (&sink, &sink) {
        (Some(file), Some(_)) => match (file.try_clone(), file.try_clone()) {
            (Ok(out), Ok(err)) => (Stdio::from(out), Stdio::from(err)),
            _ => (Stdio::null(), Stdio::null()),
        },
        _ => (Stdio::null(), Stdio::null()),
    };

    match std::process::Command::new(&binary)
        .arg("server")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
    {
        Ok(_) => {
            tracing::info!("herdr server started: {binary:?}");
            true
        }
        Err(err) => {
            tracing::error!("failed to start herdr server: {err}");
            false
        }
    }
}

/// The file a spawned daemon's output is appended to, in zenix's own config
/// directory: `herdr-server.log`. One file across spawns, so a failure after a
/// successful run still has its own last lines.
pub fn daemon_log_path() -> PathBuf {
    platform::zenix_config_dir().join("herdr-server.log")
}

/// The last `lines` lines of the daemon log, or `None` when it has never been
/// written. Only the file's tail is read, so a log left growing across spawns
/// cannot make this cost more than a few kilobytes.
pub fn daemon_log_tail(lines: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    const TAIL_BYTES: u64 = 32 * 1024;
    let path = daemon_log_path();
    let mut file = std::fs::File::open(&path).ok()?;
    let length = file.metadata().ok()?.len();
    if length > TAIL_BYTES {
        file.seek(SeekFrom::Start(length - TAIL_BYTES)).ok()?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let tail: Vec<&str> = text.lines().rev().take(lines.max(1)).collect();
    if tail.is_empty() {
        return None;
    }
    Some(tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
}

/// The command a person can run themselves when the in-place start fails: the
/// binary this client would use, then `server`.
pub fn daemon_start_command() -> String {
    match ensure_herdr() {
        Some(path) => format!("{} server", path.display()),
        None => format!("{} server", platform::herdr_binary_name()),
    }
}

/// Check whether the herdr client socket is ready.
///
/// On Unix, connects via `UnixStream`.
/// On Windows, connects via `interprocess` named pipe — matching herdr's own
/// `connect_local_stream()` logic so client and server use the same pipe name.
pub fn is_socket_ready(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        UnixStream::connect(path).is_ok()
    }

    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, Stream as LocalStream, prelude::*};
        let name = match path
            .to_string_lossy()
            .to_string()
            .to_ns_name::<GenericNamespaced>()
        {
            Ok(name) => name,
            Err(_) => return false,
        };
        LocalStream::connect(name).is_ok()
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        false
    }
}
