//! Built-in provider definitions — 8 known AI agent CLI tools with config management.
//!
//! Each provider declares its name, binary names for PATH detection,
//! config directory resolver, and which capabilities it supports
//! (MCP config, skills directory).

use super::Provider;
use crate::platform;

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
            name: "droid",
            display_name: "Droid",
            binary_names: &["droid"],
            install_command: "npm install -g @anthropic-ai/droid",
            config_dir: || platform::env_or_home("DROID_CONFIG_DIR", "~/.droid"),
            mcp_config_file: Some("config.json"),
            skills_dir: Some("skills"),
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
    ]
}
