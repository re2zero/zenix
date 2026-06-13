//! Herdr binary management: bundled binary, first-run install, PATH fallback.

use std::path::PathBuf;

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
    platform::config_dir()
        .join("herdr/herdr-client.sock")
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
///
/// On Unix, connects via `UnixStream`.
/// On Windows, connects via `interprocess` named pipe — matching herdr's own
/// `connect_local_stream()` logic so client and server use the same pipe name.
pub fn is_socket_ready(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        return UnixStream::connect(path).is_ok();
    }

    #[cfg(windows)]
    {
        use interprocess::local_socket::{
            prelude::*,
            GenericNamespaced,
            Stream as LocalStream,
        };
        let name = match path
            .to_string_lossy()
            .to_string()
            .to_ns_name::<GenericNamespaced>()
        {
            Ok(name) => name,
            Err(_) => return false,
        };
        return LocalStream::connect(name).is_ok();
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        false
    }
}
