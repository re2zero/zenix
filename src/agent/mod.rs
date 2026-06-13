//! Agent CLI detection — checks local filesystem for installed agent CLIs
//! and their herdr integration hooks. No herdr socket dependency.

use std::path::PathBuf;

/// Status of an agent CLI on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCliStatus {
    /// Hook exists and version matches.
    Installed { version: u32 },
    /// Hook exists but version is behind.
    Outdated { current: u32, expected: u32 },
    /// Binary found on PATH but no herdr hook installed.
    BinaryOnly,
    /// Neither binary nor hook found.
    NotInstalled,
}

/// Information about a known AI agent CLI.
pub struct AgentCliInfo {
    /// Short machine name (e.g. "claude").
    pub name: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Binary names to check on PATH (first match wins).
    pub binary_names: &'static [&'static str],
    /// Function that returns the herdr integration hook directory (if any).
    pub hook_dir: fn() -> Option<PathBuf>,
    /// Name of the hook file within the hook directory.
    pub hook_filename: &'static str,
    /// Expected version in the hook file (HERDR_INTEGRATION_VERSION line).
    pub expected_version: u32,
    /// Detected status.
    pub status: AgentCliStatus,
}

/// Check if a binary exists anywhere on `$PATH`.
fn binary_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .as_ref()
        .and_then(|p| p.to_str())
        .map(|path| {
            path.split(':').any(|dir| {
                let p = PathBuf::from(dir).join(name);
                p.is_file()
            })
        })
        .unwrap_or(false)
}

/// Check if ANY of the given binary names exist on PATH.
fn any_binary_on_path(names: &[&str]) -> bool {
    names.iter().any(|n| binary_on_path(n))
}

/// Parse `HERDR_INTEGRATION_VERSION=N` from a hook file.
fn parse_hook_version(path: &PathBuf) -> Option<u32> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("HERDR_INTEGRATION_VERSION=") {
            return v.trim().parse().ok();
        }
    }
    None
}

/// Resolve home directory, falling back to `/root`.
fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()))
}

/// Resolve an env-var-based directory, with a `~`-based fallback.
fn env_or_home(var: &str, fallback_suffix: &str) -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(var) {
        let p = PathBuf::from(dir);
        if p.exists() { return Some(p); }
    }
    let p = home().join(fallback_suffix.trim_start_matches("~/"));
    if p.exists() { Some(p) } else { None }
}

// ── Hook directory functions ──────────────────────────────────────

fn claude_hook_dir() -> Option<PathBuf> {
    env_or_home("CLAUDE_CONFIG_DIR", "~/.claude")
        .map(|d| d.join("hooks"))
}

fn codex_hook_dir() -> Option<PathBuf> {
    env_or_home("CODEX_HOME", "~/.codex")
        .map(|d| d.join("hooks"))
}

fn opencode_hook_dir() -> Option<PathBuf> {
    Some(home().join(".config/opencode/plugins"))
}

fn pi_hook_dir() -> Option<PathBuf> {
    env_or_home("PI_CODING_AGENT_DIR", "~/.pi-coding-agent")
        .map(|d| d.join("extensions"))
}

fn omp_hook_dir() -> Option<PathBuf> {
    Some(home().join(".config/omp/extensions"))
}

fn copilot_hook_dir() -> Option<PathBuf> {
    env_or_home("COPILOT_HOME", "~/.copilot")
        .map(|d| d.join("hooks"))
}

fn droid_hook_dir() -> Option<PathBuf> {
    Some(home().join(".factory/hooks"))
}

fn kimi_hook_dir() -> Option<PathBuf> {
    env_or_home("KIMI_CODE_HOME", "~/.kimi-code")
        .map(|d| d.join("hooks"))
}

fn kilo_hook_dir() -> Option<PathBuf> {
    Some(home().join(".config/kilo/plugin"))
}

fn hermes_hook_dir() -> Option<PathBuf> {
    Some(home().join(".hermes/plugins/herdr-agent-state"))
}

fn qoder_hook_dir() -> Option<PathBuf> {
    env_or_home("QODER_CONFIG_DIR", "~/.qoder")
        .map(|d| d.join("hooks"))
}

fn cursor_hook_dir() -> Option<PathBuf> {
    env_or_home("CURSOR_CONFIG_DIR", "~/.cursor")
        .map(|d| d.join("hooks"))
}

// ── Registry ──────────────────────────────────────────────────────

/// Detect all known agent CLIs and return their status.
pub fn detect_all_agents() -> Vec<AgentCliInfo> {
    AGENTS.iter().map(|spec| {
        let has_binary = any_binary_on_path(spec.binary_names);
        let hook_path = (spec.hook_dir)().map(|d| d.join(spec.hook_filename));
        let hook_version = hook_path.as_ref().and_then(parse_hook_version);

        let status = match (has_binary, hook_version) {
            (_, Some(v)) if v >= spec.expected_version => AgentCliStatus::Installed { version: v },
            (_, Some(v)) => AgentCliStatus::Outdated { current: v, expected: spec.expected_version },
            (true, None) => AgentCliStatus::BinaryOnly,
            (false, None) => AgentCliStatus::NotInstalled,
        };

        AgentCliInfo {
            name: spec.name,
            display_name: spec.display_name,
            binary_names: spec.binary_names,
            hook_dir: spec.hook_dir,
            hook_filename: spec.hook_filename,
            expected_version: spec.expected_version,
            status,
        }
    }).collect()
}

/// Resolve the hook file path for a named agent.
///
/// Looks up the agent in the built-in registry by `agent_name`, then joins
/// its hook directory (if resolved) with its hook filename.
/// Returns `None` if the agent is unknown or its hook directory doesn't exist.
pub fn hook_path_for(agent_name: &str) -> Option<PathBuf> {
    let spec = AGENTS.iter().find(|a| a.name == agent_name)?;
    let dir = (spec.hook_dir)()?;
    Some(dir.join(spec.hook_filename))
}

/// Remove the hook file for a named agent.
///
/// Returns `Err` if no hook file exists for the agent.
pub fn uninstall_hook(agent_name: &str) -> Result<(), String> {
    let path = hook_path_for(agent_name).ok_or_else(|| "Hook not found".to_string())?;
    std::fs::remove_file(&path).map_err(|e| e.to_string())
}

struct AgentSpec {
    name: &'static str,
    display_name: &'static str,
    binary_names: &'static [&'static str],
    hook_dir: fn() -> Option<PathBuf>,
    hook_filename: &'static str,
    expected_version: u32,
}

static AGENTS: &[AgentSpec] = &[
    AgentSpec {
        name: "claude",
        display_name: "Claude Code",
        binary_names: &["claude"],
        hook_dir: claude_hook_dir,
        hook_filename: "herdr-agent-state.sh",
        expected_version: 5,
    },
    AgentSpec {
        name: "codex",
        display_name: "OpenAI Codex",
        binary_names: &["codex"],
        hook_dir: codex_hook_dir,
        hook_filename: "herdr-agent-state.sh",
        expected_version: 5,
    },
    AgentSpec {
        name: "opencode",
        display_name: "OpenCode",
        binary_names: &["opencode"],
        hook_dir: opencode_hook_dir,
        hook_filename: "herdr-agent-state.js",
        expected_version: 5,
    },
    AgentSpec {
        name: "pi",
        display_name: "Pi Coding Agent",
        binary_names: &["pi"],
        hook_dir: pi_hook_dir,
        hook_filename: "herdr-agent-state.ts",
        expected_version: 2,
    },
    AgentSpec {
        name: "omp",
        display_name: "OMP",
        binary_names: &["omp"],
        hook_dir: omp_hook_dir,
        hook_filename: "herdr-omp-agent-state.ts",
        expected_version: 2,
    },
    AgentSpec {
        name: "copilot",
        display_name: "GitHub Copilot",
        binary_names: &["copilot"],
        hook_dir: copilot_hook_dir,
        hook_filename: "herdr-agent-state.sh",
        expected_version: 2,
    },
    AgentSpec {
        name: "droid",
        display_name: "Factory Droid",
        binary_names: &["droid", "factory"],
        hook_dir: droid_hook_dir,
        hook_filename: "herdr-agent-state.sh",
        expected_version: 2,
    },
    AgentSpec {
        name: "kimi",
        display_name: "Kimi Code",
        binary_names: &["kimi-code", "kimi"],
        hook_dir: kimi_hook_dir,
        hook_filename: "herdr-agent-state.sh",
        expected_version: 3,
    },
    AgentSpec {
        name: "kilo",
        display_name: "Kilo Code",
        binary_names: &["kilo"],
        hook_dir: kilo_hook_dir,
        hook_filename: "herdr-agent-state.js",
        expected_version: 1,
    },
    AgentSpec {
        name: "hermes",
        display_name: "Hermes",
        binary_names: &["hermes"],
        hook_dir: hermes_hook_dir,
        hook_filename: "__init__.py",
        expected_version: 2,
    },
    AgentSpec {
        name: "qoder",
        display_name: "Qoder CLI",
        binary_names: &["qoder"],
        hook_dir: qoder_hook_dir,
        hook_filename: "herdr-agent-state.sh",
        expected_version: 2,
    },
    AgentSpec {
        name: "cursor",
        display_name: "Cursor",
        binary_names: &["cursor"],
        hook_dir: cursor_hook_dir,
        hook_filename: "herdr-agent-state.sh",
        expected_version: 1,
    },
];
