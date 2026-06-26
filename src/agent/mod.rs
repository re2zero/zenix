//! Agent CLI detection — checks local filesystem for installed agent CLIs.
//! Uses the ProviderRegistry for agent definitions.

use std::process::Command;

use std::path::PathBuf;

use crate::provider::ProviderRegistry;

/// Information about a known AI agent CLI.
pub struct AgentCliInfo {
    /// Short machine name (e.g. "claude").
    pub name: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Binary names to check on PATH (first match wins).
    pub binary_names: &'static [&'static str],
    /// Install command for this agent.
    pub install_command: &'static str,
    /// Resolved binary path if found on PATH.
    pub binary_path: Option<PathBuf>,
    /// Whether the agent CLI binary was found on PATH.
    pub detected: bool,
}

/// Find a binary on `$PATH` and return its full path.
fn find_binary(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").as_ref().and_then(|p| {
        std::env::split_paths(p).find_map(|dir| {
            let full = dir.join(name);
            if full.is_file() { Some(full) } else { None }
        })
    })
}

/// Check if ANY of the given binary names exist on PATH and return the first path.
fn find_any_binary(names: &[&str]) -> Option<PathBuf> {
    names.iter().find_map(|n| find_binary(n))
}

/// Detect all known agent CLIs and return their status.
pub fn detect_all_agents() -> Vec<AgentCliInfo> {
    let registry = ProviderRegistry::builtin();
    registry
        .all()
        .iter()
        .map(|prov| {
            let binary_path = find_any_binary(prov.binary_names);
            AgentCliInfo {
                name: prov.name,
                display_name: prov.display_name,
                binary_names: prov.binary_names,
                install_command: prov.install_command,
                binary_path: binary_path.clone(),
                detected: binary_path.is_some(),
            }
        })
        .collect()
}
/// Install a known agent CLI using its registry-defined command.
pub fn install_agent(agent_name: &str) -> Result<(), String> {
    let registry = ProviderRegistry::builtin();
    let provider = registry
        .find(agent_name)
        .ok_or_else(|| format!("Unknown agent: {agent_name}"))?;
    let command = provider.install_command.trim();
    if let Some(url) = download_url(command) {
        return open_download_url(&url);
    }

    let status = if cfg!(target_os = "windows") {
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                command,
            ])
            .status()
    } else {
        Command::new(crate::platform::default_shell())
            .arg("-c")
            .arg(command)
            .status()
    }
    .map_err(|e| format!("Failed to run install command: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("Install command exited with {status}"))
    }
}

fn download_url(command: &str) -> Option<String> {
    let rest = command.strip_prefix("Download from ")?;
    let host = rest.split('/').next()?.trim();
    let host = host
        .strip_prefix("http://")
        .or_else(|| host.strip_prefix("https://"))
        .unwrap_or(host)
        .split(':')
        .next()
        .unwrap_or(host)
        .strip_suffix(".com")
        .unwrap_or(host);
    match host {
        "cursor" => Some("https://www.cursor.com/download".into()),
        "windsurf" => Some("https://www.windsurf.com/download".into()),
        _ => Some(format!("https://{host}")),
    }
}

fn open_download_url(url: &str) -> Result<(), String> {
    let status = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", url]).status()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else {
        Command::new("xdg-open").arg(url).status()
    }
    .map_err(|e| format!("Failed to open browser: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("Failed to open browser for {url}"))
    }
}
