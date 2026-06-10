//! Herdr binary management: find, install, and launch.

use std::path::PathBuf;

/// Find the herdr binary in standard locations.
pub fn find_herdr_binary() -> Option<PathBuf> {
    // 1. Check PATH
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("herdr");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // 2. Check ~/.local/bin/herdr
    let home = PathBuf::from(
        std::env::var("HOME")
            .unwrap_or_else(|_| "/root".to_string()),
    );
    let fallback = home.join(".local/bin/herdr");
    if fallback.is_file() {
        return Some(fallback);
    }
    None
}

/// Compute the herdr client socket path.
pub fn herdr_socket_path() -> PathBuf {
    let home = std::env::var("HOME")
        .unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".config/herdr/herdr-client.sock")
}

/// Start the herdr server in the background.
/// Returns true if the process was spawned successfully.
pub fn start_herdr_server() -> bool {
    let binary = match find_herdr_binary() {
        Some(path) => path,
        None => return false,
    };

    match std::process::Command::new(&binary)
        .arg("server")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
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

/// Check whether the herdr client socket is ready.
pub fn is_socket_ready(path: &std::path::Path) -> bool {
    use std::os::unix::net::UnixStream;
    UnixStream::connect(path).is_ok()
}
