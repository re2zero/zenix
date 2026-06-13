//! Herdr binary management: bundled binary, first-run install, PATH fallback.

use std::path::PathBuf;

/// Path to the herdr binary built/copied at compile time (dev/test convenience).
const BUNDLED_HERDR: &str = env!("HERDR_BINARY");

/// Seed path for herdr installed by the deb/rpm package.
const SEED_HERDR: &str = "/usr/share/zenix/herdr";

/// User-local install path — herdr self-updates from here.
fn user_herdr_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home).join(".local/bin/herdr")
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
        for dir in std::env::split_paths(&path) {
            let c = dir.join("herdr");
            if c.is_file() {
                return Some(c);
            }
        }
    }

    // 4. Seed binary from package install — copy to user-local path
    let seed = PathBuf::from(SEED_HERDR);
    if seed.is_file() {
        let _ = std::fs::create_dir_all(user.parent().unwrap());
        if std::fs::copy(&seed, &user).is_ok() {
            // Make it executable (copy preserves mode from seed)
            tracing::info!("installed herdr to {}", user.display());
            return Some(user);
        }
    }

    None
}
/// Find the herdr binary (legacy — prefer `ensure_herdr()`).
pub fn find_herdr_binary() -> Option<PathBuf> { ensure_herdr() }

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
    let binary = match ensure_herdr() {
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
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        return UnixStream::connect(path).is_ok();
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}
