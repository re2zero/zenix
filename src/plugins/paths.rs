//! Where the daemon keeps a plugin's own files, as its commands see them.
//!
//! Every command herdr runs for a plugin is handed three environment variables
//! (herdr v0.9.5 `src/app/api/plugins/env.rs:16`, `plugin_path_env`):
//! `HERDR_PLUGIN_ROOT` — the directory that was linked, which `plugin.list`
//! already reports as `plugin_root` — plus `HERDR_PLUGIN_CONFIG_DIR` and
//! `HERDR_PLUGIN_STATE_DIR`, the two places a plugin may keep its own files.
//! The daemon answers no API that reports those two, so this module computes
//! them the way the daemon does, and the manager's contract block shows the
//! result: a user who wants to inspect what a plugin wrote looks there.
//!
//! Mirrored from herdr v0.9.5, cited by line so a daemon bump can re-check:
//!
//! * `src/config/io.rs:24` (`app_dir_name`) — the daemon's own build decides
//!   `herdr` vs `herdr-dev`; zenix runs pinned release daemons
//!   (`vendor/herdr-pins.tsv`), so the name here is always `herdr`.
//! * `src/config/io.rs:30` (`config_dir`) and `:37` (`state_dir`) —
//!   `XDG_CONFIG_HOME` / `XDG_STATE_HOME` when set, else the platform default.
//! * `src/plugin_paths.rs:21` (`plugin_config_dir`) and `:28`
//!   (`plugin_state_dir`) — the two subpaths under those bases.
//! * `src/plugin_paths.rs:96` (`plugin_config_path_component`) — the
//!   per-plugin directory name: bytes outside `[a-z0-9._-]` percent-encoded,
//!   a trailing dot escaped, an empty id a literal `%plugin`, and a component
//!   past 120 characters truncated and hashed.

use std::path::PathBuf;

/// The app directory the daemon uses, `herdr`: the release builds this client
/// pairs with (`vendor/herdr-pins.tsv`) are not `debug_assertions`, which is
/// the only case herdr v0.9.5 `src/config/io.rs:24` names `herdr-dev`.
const APP_NAME: &str = "herdr";

/// How many characters a per-plugin directory component may hold before the
/// daemon truncates it and appends a hash
/// (`PLUGIN_CONFIG_PATH_COMPONENT_MAX_CHARS`).
const COMPONENT_MAX_CHARS: usize = 120;

/// The directory a plugin's commands are told is theirs for configuration:
/// `<config>/plugins/config/<component>`.
pub(crate) fn plugin_config_dir(plugin_id: &str) -> PathBuf {
    plugin_config_dir_under(&config_base(), plugin_id)
}

/// The directory a plugin's commands are told is theirs for state:
/// `<state>/plugins/<component>`.
pub(crate) fn plugin_state_dir(plugin_id: &str) -> PathBuf {
    plugin_state_dir_under(&state_base(), plugin_id)
}

/// [`plugin_config_dir`] against an explicit base — the shape the tests read,
/// so the paths are checked without touching this process's environment.
fn plugin_config_dir_under(base: &std::path::Path, plugin_id: &str) -> PathBuf {
    base.join("plugins")
        .join("config")
        .join(path_component(plugin_id))
}

/// [`plugin_state_dir`] against an explicit base, for the tests.
fn plugin_state_dir_under(base: &std::path::Path, plugin_id: &str) -> PathBuf {
    base.join("plugins").join(path_component(plugin_id))
}

/// The daemon's config root: `XDG_CONFIG_HOME/herdr`, else `~/.config/herdr`
/// (herdr v0.9.5 `src/config/io.rs:61`, the not-windows branch), else the
/// temp dir when even `HOME` is absent.
fn config_base() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join(APP_NAME);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(format!(".config/{APP_NAME}"));
    }
    std::env::temp_dir().join(APP_NAME)
}

/// The daemon's state root: `XDG_STATE_HOME/herdr`, else `~/.local/state/herdr`
/// (herdr v0.9.5 `src/config/io.rs:88`).
fn state_base() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(dir).join(APP_NAME);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(format!(".local/state/{APP_NAME}"));
    }
    std::env::temp_dir().join(format!("{APP_NAME}-state"))
}

/// The per-plugin directory name, byte for byte what herdr v0.9.5
/// `src/plugin_paths.rs:96` (`plugin_config_path_component`) computes.
fn path_component(value: &str) -> String {
    let mut component = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        {
            component.push(byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(component, "%{byte:02X}");
        }
    }
    if component.ends_with('.') {
        component.pop();
        component.push_str("%2E");
    }
    if component.is_empty() {
        return "%plugin".to_string();
    }
    if is_windows_reserved_stem(&component) {
        component = format!("%{component}");
    }
    if component.chars().count() > COMPONENT_MAX_CHARS {
        let hash = short_id_hash(value);
        let prefix_len = COMPONENT_MAX_CHARS - hash.len() - 1;
        let prefix = component.chars().take(prefix_len).collect::<String>();
        return format!("{prefix}-{hash}");
    }
    component
}

/// Twelve hex characters of the id's SHA-256, the suffix the daemon appends to
/// an over-long component (herdr v0.9.5 `src/api/schema/plugins.rs:150`).
fn short_id_hash(value: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    let digest = Sha256::digest(value.as_bytes());
    let mut hash = String::with_capacity(12);
    for byte in &digest[..6] {
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

/// Windows keeps a set of device names no directory may be called, whatever
/// the platform (herdr v0.9.5 `src/api/schema/plugins.rs:162`) — the daemon
/// escapes the stem wherever it runs, so the mirror does too.
fn is_windows_reserved_stem(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The daemon's own expectations for the component, read off its test
    /// `plugin_config_path_component_is_readable_and_collision_free`
    /// (herdr v0.9.5 `src/plugin_paths.rs:186`).
    #[test]
    fn the_component_matches_the_daemons_own_cases() {
        assert_eq!(
            path_component("examples.agent-telegram-notify"),
            "examples.agent-telegram-notify"
        );
        assert_eq!(path_component("example:a"), "example%3Aa");
        assert_eq!(path_component("Example"), "%45xample");
        assert_ne!(path_component("example:a"), path_component("example-a"));
        assert_ne!(path_component("Example"), path_component("example"));
        assert_eq!(path_component("con"), "%con");
        assert_eq!(path_component("example."), "example%2E");
    }

    /// An id with no usable bytes at all becomes the daemon's `%plugin`.
    #[test]
    fn an_empty_component_becomes_a_placeholder() {
        assert_eq!(path_component(""), "%plugin");
        assert_eq!(path_component("::"), "%3A%3A");
    }

    /// Past 120 characters the component is cut and suffixed with a hash of the
    /// whole id, so two long ids that agree on their first 100 characters still
    /// land in different directories — the collision the daemon's own test pins.
    #[test]
    fn an_over_long_component_is_truncated_and_hashed() {
        let a = path_component(&"A".repeat(120));
        let b = path_component(&"B".repeat(120));
        assert_ne!(a, b);
        assert!(a.chars().count() <= COMPONENT_MAX_CHARS);
        assert!(b.chars().count() <= COMPONENT_MAX_CHARS);
        // The same id, hashed twice, is the same directory.
        assert_eq!(a, path_component(&"A".repeat(120)));
    }

    /// The two directories hang off the bases the daemon reads: config under
    /// `plugins/config`, state directly under `plugins`.
    #[test]
    fn the_directories_hang_off_their_bases_as_the_daemon_lays_them() {
        let base = std::path::Path::new("/state-home");
        assert_eq!(
            plugin_state_dir_under(base, "zenix.smoke"),
            PathBuf::from("/state-home/plugins/zenix.smoke")
        );
        assert_eq!(
            plugin_config_dir_under(base, "zenix.smoke"),
            PathBuf::from("/state-home/plugins/config/zenix.smoke")
        );
    }
}
