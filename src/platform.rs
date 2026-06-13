//! Cross-platform path resolution and platform helpers.
//!
//! All home/config/data path resolution should go through this module
//! instead of directly reading `HOME` or hardcoding `~/.config/...`.

use std::path::PathBuf;

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
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

/// Cross-platform config directory.
/// Linux: ~/.config, macOS: ~/Library/Application Support, Windows: %APPDATA%
pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| home_dir().join(".config"))
}

pub fn zenix_config_dir() -> PathBuf { config_dir().join("zenix") }
pub fn opencode_config_dir() -> PathBuf { config_dir().join("opencode") }
pub fn omp_config_dir() -> PathBuf { config_dir().join("omp") }
pub fn kilo_config_dir() -> PathBuf { config_dir().join("kilo") }
pub fn hermes_config_dir() -> PathBuf { config_dir().join("hermes") }

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
}

pub fn herdr_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "herdr.exe"
    } else {
        "herdr"
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
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(r"C:\Program Files\zenix\herdr.exe")
    }
}
