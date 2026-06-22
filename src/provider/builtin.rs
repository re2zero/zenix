//! Built-in provider definitions — all 12 known AI agent CLI tools.
//!
//! Each provider declares its name, binary names for PATH detection,
//! config directory resolver, and which capabilities it supports
//! (MCP config, skills directory).

use crate::platform;
use super::Provider;

/// Returns the complete list of built-in providers.
pub fn all_providers() -> Vec<Provider> {
    vec![
        Provider {
            name: "claude",
            display_name: "Claude Code",
            binary_names: &["claude"],
            install_command: "npm install -g @anthropic-ai/claude-code",
            config_dir: || platform::env_or_home("CLAUDE_CONFIG_DIR", "~/.claude"),
            mcp_config_file: Some("claude_desktop_config.json"),
            skills_dir: Some("skills"),
        },
        Provider {
            name: "codex",
            display_name: "OpenAI Codex",
            binary_names: &["codex"],
            install_command: "npm install -g @openai/codex",
            config_dir: || platform::env_or_home("CODEX_HOME", "~/.codex"),
            mcp_config_file: Some("config.json"),
            skills_dir: Some("skills"),
        },
        Provider {
            name: "opencode",
            display_name: "OpenCode",
            binary_names: &["opencode"],
            install_command: "npm install -g @elizaos/opencode",
            config_dir: || platform::env_or_home("OPENCODE_CONFIG_DIR", "~/.opencode"),
            mcp_config_file: Some("config.json"),
            skills_dir: Some("skills"),
        },
        Provider {
            name: "copilot",
            display_name: "GitHub Copilot",
            binary_names: &["copilot"],
            install_command: "npm install -g @github/copilot-cli",
            config_dir: || platform::env_or_home("COPYLOT_CONFIG_DIR", "~/.github/copilot"),
            mcp_config_file: None,
            skills_dir: None,
        },
        Provider {
            name: "droid",
            display_name: "Droid",
            binary_names: &["droid"],
            install_command: "npm install -g @anthropic-ai/droid",
            config_dir: || platform::env_or_home("DROID_CONFIG_DIR", "~/.droid"),
            mcp_config_file: Some("config.json"),
            skills_dir: Some("skills"),
        },
        Provider {
            name: "kimi",
            display_name: "Kimi",
            binary_names: &["kimi"],
            install_command: "npm install -g @kimi/kimi-cli",
            config_dir: || platform::env_or_home("KIMI_CONFIG_DIR", "~/.kimi"),
            mcp_config_file: None,
            skills_dir: None,
        },
        Provider {
            name: "pi",
            display_name: "Pi",
            binary_names: &["pi"],
            install_command: "npm install -g @anthropic-ai/pi",
            config_dir: || platform::env_or_home("PI_CONFIG_DIR", "~/.pi"),
            mcp_config_file: Some("config.json"),
            skills_dir: Some("skills"),
        },
        Provider {
            name: "omp",
            display_name: "OMP",
            binary_names: &["omp"],
            install_command: "npm install -g @anthropic-ai/omp",
            config_dir: || platform::env_or_home("OMP_CONFIG_DIR", "~/.omp"),
            mcp_config_file: Some("config.json"),
            skills_dir: Some("skills"),
        },
        Provider {
            name: "kilo",
            display_name: "Kilo",
            binary_names: &["kilo"],
            install_command: "npm install -g @anthropic-ai/kilo",
            config_dir: || platform::env_or_home("KILO_CONFIG_DIR", "~/.kilo"),
            mcp_config_file: Some("config.json"),
            skills_dir: Some("skills"),
        },
        Provider {
            name: "hermes",
            display_name: "Hermes",
            binary_names: &["hermes"],
            install_command: "npm install -g @anthropic-ai/hermes",
            config_dir: || platform::env_or_home("HERMES_CONFIG_DIR", "~/.hermes"),
            mcp_config_file: Some("config.json"),
            skills_dir: Some("skills"),
        },
        Provider {
            name: "cursor",
            display_name: "Cursor",
            binary_names: &["cursor"],
            install_command: "Download from cursor.com",
            config_dir: || platform::env_or_home("CURSOR_CONFIG_DIR", "~/.cursor"),
            mcp_config_file: None,
            skills_dir: None,
        },
        Provider {
            name: "windsurf",
            display_name: "Windsurf",
            binary_names: &["windsurf"],
            install_command: "Download from codeium.com/windsurf",
            config_dir: || platform::env_or_home("WINDSURF_CONFIG_DIR", "~/.windsurf"),
            mcp_config_file: None,
            skills_dir: None,
        },
    ]
}
