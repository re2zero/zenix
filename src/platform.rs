//! Cross-platform path resolution and platform helpers.
//!
//! All home/config/data path resolution should go through this module
//! instead of directly reading `HOME` or hardcoding `~/.config/...`.

use std::path::{Path, PathBuf};

/// Cross-platform home directory.
///
/// Uses `dirs::home_dir()` which checks `HOME` (Unix) or `USERPROFILE` (Windows).
/// Falls back to `/` if not determinable (better than `/root` which is Linux-only).
pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Resolve an env-var-based directory with a home-relative fallback.
///
/// Checks `var` first; if not set or path doesn't exist, falls back to
/// `home_dir().join(fallback_suffix)` where fallback uses `~/` prefix convention.
pub fn env_or_home(var: &str, fallback_suffix: &str) -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(var) {
        let p = PathBuf::from(dir);
        if p.exists() {
            return Some(p);
        }
    }
    let p = home_dir().join(fallback_suffix.trim_start_matches("~/"));
    if p.exists() { Some(p) } else { None }
}

/// Cross-platform config directory.
/// Linux: ~/.config, macOS: ~/Library/Application Support, Windows: %APPDATA%
pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| home_dir().join(".config"))
}

/// The zenix settings directory.
pub fn zenix_config_dir() -> PathBuf {
    config_dir().join("zenix")
}

/// The herdr daemon's own config file: the settings *it* acts on (the shell a
/// new pane starts, the cwd a new workspace starts in, the scrollback limit,
/// keybindings, plugin settings). zenix only points at it; see
/// `crate::config`'s module docs for where the boundary is.
///
/// Mirrors `herdr/src/config/io.rs`: `HERDR_CONFIG_PATH` when set, otherwise
/// `<config dir>/<app dir>/config.toml`, where the app dir carries the `-dev`
/// suffix in a debug build.
pub fn herdr_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("HERDR_CONFIG_PATH") {
        return PathBuf::from(path);
    }
    let app_dir = if cfg!(debug_assertions) {
        "herdr-dev"
    } else {
        "herdr"
    };
    config_dir().join(app_dir).join("config.toml")
}
pub fn opencode_config_dir() -> PathBuf {
    config_dir().join("opencode")
}
pub fn omp_config_dir() -> PathBuf {
    config_dir().join("omp")
}
pub fn kilo_config_dir() -> PathBuf {
    config_dir().join("kilo")
}
pub fn hermes_config_dir() -> PathBuf {
    config_dir().join("hermes")
}

/// Local bin directory for user-installed binaries.
pub fn local_bin_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir()
            .unwrap_or_else(home_dir)
            .join("Programs")
    }
    #[cfg(not(target_os = "windows"))]
    {
        home_dir().join(".local").join("bin")
    }
}

pub fn default_shell() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "powershell.exe"
    }
    #[cfg(target_os = "macos")]
    {
        "zsh"
    }
    #[cfg(target_os = "linux")]
    {
        "bash"
    }
    #[cfg(target_os = "freebsd")]
    {
        "tcsh"
    }
}

pub fn herdr_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "herdr.exe"
    } else {
        "herdr"
    }
}

/// Create a directory symlink, cross-platform.
///
/// Uses `symlink` on Unix and `symlink_dir` on Windows.
/// On Windows, this may require administrator privileges or developer mode.
#[allow(dead_code)]
pub fn symlink_dir(src: &Path, dst: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)
            .map_err(|e| format!("symlink({src:?} -> {dst:?}): {e}"))
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(src, dst)
            .map_err(|e| format!("symlink_dir({src:?} -> {dst:?}): {e}"))
    }
}

pub fn seed_herdr() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        PathBuf::from("/usr/share/zenix/herdr")
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/usr/local/share/zenix/herdr")
    }
    #[cfg(target_os = "freebsd")]
    {
        PathBuf::from("/usr/local/share/zenix/herdr")
    }
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(r"C:\Program Files\zenix\herdr.exe")
    }
}
