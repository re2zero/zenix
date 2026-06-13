//! Unified MCP server management — read/write MCP configs across agents.
//!
//! Two data sources:
//!   1. Zenix-managed servers: <config_dir>/zenix/mcp.json (NDJSON)
//!   2. Per-agent Claude-format configs (Claude, OpenCode, Pi, OMP, Kilo)
//!
//! Claude-format JSON:
//!     `{ "mcpServers": { <name>: { command, args?, env?, disabled? } } }`

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::platform;

/// A single MCP server entry.
#[derive(Debug, Clone)]
pub struct McpServerEntry {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum McpType {
    Stdio,
    Sse,
}

impl Default for McpType {
    fn default() -> Self { McpType::Stdio }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZenixMcpServer {
    pub name: String,
    #[serde(rename = "type", default)]
    pub mcp_type: McpType,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub enabled_agents: Vec<String>,
}

// ── Per-agent config paths ───────────────────────────────────────────

/// Returns (agent_name, path) for agents that use Claude-format JSON MCP configs.
fn claude_format_configs() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();

    // Claude
    if let Some(dir) = platform::env_or_home("CLAUDE_CONFIG_DIR", "~/.claude") {
        out.push(("claude".into(), dir.join("claude_desktop_config.json")));
    }
    // OpenCode
    out.push(("opencode".into(), platform::opencode_config_dir().join("config.json")));
    // Pi
    if let Some(dir) = platform::env_or_home("PI_CODING_AGENT_DIR", "~/.pi-coding-agent") {
        out.push(("pi".into(), dir.join("config.json")));
    }
    // Codex
    if let Some(dir) = platform::env_or_home("CODEX_HOME", "~/.codex") {
        out.push(("codex".into(), dir.join("config.json")));
    }
    // OMP
    out.push(("omp".into(), platform::omp_config_dir().join("config.json")));
    // Kilo
    out.push(("kilo".into(), platform::kilo_config_dir().join("config.json")));
    // Hermes
    out.push(("hermes".into(), platform::hermes_config_dir().join("config.json")));

    out
}

// ── JSON read/write ──────────────────────────────────────────────────

/// Parse MCP servers from a Claude-format JSON config.
fn read_claude_mcp(path: &PathBuf) -> Vec<McpServerEntry> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let root: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let servers = match root.get("mcpServers") {
        Some(serde_json::Value::Object(map)) => map,
        _ => return vec![],
    };

    servers.iter().map(|(name, cfg)| {
        McpServerEntry {
            name: name.clone(),
            command: cfg.get("command").and_then(|v| v.as_str()).unwrap_or("").into(),
            args: cfg.get("args").and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.into())).collect())
                .unwrap_or_default(),
            env: cfg.get("env").and_then(|v| v.as_object())
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").into())).collect())
                .unwrap_or_default(),
            disabled: cfg.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false),
        }
    }).collect()
}

/// Write MCP servers to a Claude-format JSON config, preserving all other fields.
fn write_claude_mcp(path: &PathBuf, servers: &[McpServerEntry]) -> Result<(), String> {
    // Read existing config
    let content = std::fs::read_to_string(path).unwrap_or_else(|_| "{}".into());
    let mut root: serde_json::Value = serde_json::from_str(&content)
        .unwrap_or(serde_json::Value::Object(Default::default()));

    // Build servers object
    let mut map = serde_json::Map::new();
    for s in servers {
        let mut entry = serde_json::Map::new();
        entry.insert("command".into(), serde_json::Value::String(s.command.clone()));
        if !s.args.is_empty() {
            entry.insert("args".into(), s.args.iter().map(|a| serde_json::Value::String(a.clone())).collect());
        }
        if !s.env.is_empty() {
            let env_map: serde_json::Map<String, serde_json::Value> = s.env.iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            entry.insert("env".into(), serde_json::Value::Object(env_map));
        }
        if s.disabled {
            entry.insert("disabled".into(), serde_json::Value::Bool(true));
        }
        map.insert(s.name.clone(), serde_json::Value::Object(entry));
    }
    root["mcpServers"] = serde_json::Value::Object(map);

    // Backup
    let bak_path = path.with_extension("json.zenix.bak");
    let _ = std::fs::copy(path, &bak_path);

    // Write
    let pretty = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("serialize error: {e}"))?;
    std::fs::write(path, pretty).map_err(|e| format!("write error: {e}"))?;

    Ok(())
}

// ── Public API ───────────────────────────────────────────────────────

/// Scan all agents and return discovered MCP servers.
/// Returns Vec<(agent_name, Vec<McpServerEntry>)>.
pub fn scan_all_mcp_servers() -> Vec<(String, Vec<McpServerEntry>)> {
    claude_format_configs().into_iter()
        .filter_map(|(name, path)| {
            if path.exists() {
                let servers = read_claude_mcp(&path);
                if !servers.is_empty() {
                    Some((name, servers))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

/// Deduplicate servers by name+command across agents.
/// Returns a unified list with agent badges.
pub fn deduplicated_servers(raw: &[(String, Vec<McpServerEntry>)]) -> Vec<UnifiedServer> {
    let mut map: HashMap<String, UnifiedServer> = HashMap::new();

    for (agent, servers) in raw {
        for s in servers {
            let key = format!("{}|{}", s.name, s.command);
            map.entry(key)
                .and_modify(|e| {
                    if !e.agents.contains(agent) { e.agents.push(agent.clone()); }
                    if s.disabled { e.disabled_agents.push(agent.clone()); }
                })
                .or_insert_with(|| {
                    let agents = vec![agent.clone()];
                    let disabled_agents = if s.disabled { vec![agent.clone()] } else { vec![] };
                    UnifiedServer {
                        name: s.name.clone(),
                        command: s.command.clone(),
                        args: s.args.clone(),
                        env: s.env.clone(),
                        agents,
                        disabled_agents,
                    }
                });
        }
    }

    let mut result: Vec<_> = map.into_values().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// A unified MCP server view across agents.
#[derive(Debug, Clone)]
pub struct UnifiedServer {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub agents: Vec<String>,
    pub disabled_agents: Vec<String>,
}

/// Write an MCP server to selected agents' configs.
pub fn save_mcp_server(
    server: &UnifiedServer,
    target_agents: &[String],
) -> Result<(), String> {
    let entry = McpServerEntry {
        name: server.name.clone(),
        command: server.command.clone(),
        args: server.args.clone(),
        env: server.env.clone(),
        disabled: false,
    };

    for (agent, path) in claude_format_configs() {
        if !target_agents.contains(&agent) && !server.agents.contains(&agent) {
            continue;
        }
        // Read current, upsert the entry, write back
        let mut current = read_claude_mcp(&path);
        current.retain(|s| s.name != server.name);
        if target_agents.contains(&agent) || server.agents.contains(&agent) {
            current.push(entry.clone());
        }
        write_claude_mcp(&path, &current)?;
    }
    Ok(())
}

/// Remove an MCP server from all agents.
pub fn remove_mcp_server(name: &str) -> Result<(), String> {
    for (_agent, path) in claude_format_configs() {
        if !path.exists() { continue; }
        let mut current = read_claude_mcp(&path);
        let before = current.len();
        current.retain(|s| s.name != name);
        if current.len() != before {
            write_claude_mcp(&path, &current)?;
        }
    }
    Ok(())
}

/// Return only the agent names that use Claude-format configs.
pub fn claude_format_config_names() -> Vec<String> {
    claude_format_configs().into_iter().map(|(name, _)| name).collect()
}

/// Toggle a server's `disabled` field for a specific agent.
pub fn toggle_mcp_agent(
    server_name: &str,
    agent_name: &str,
    enabled: bool,
) -> Result<(), String> {
    let (path, config) = claude_format_configs().into_iter()
        .find_map(|(name, path)| {
            if name == agent_name && path.exists() {
                let servers = read_claude_mcp(&path);
                Some((path, servers))
            } else {
                None
            }
        })
        .ok_or_else(|| format!("agent '{agent_name}' not found or config missing"))?;

    let mut servers = config;
    let server = servers.iter_mut()
        .find(|s| s.name == server_name)
        .ok_or_else(|| format!("server '{server_name}' not found in agent '{agent_name}'"))?;
    server.disabled = !enabled;

    write_claude_mcp(&path, &servers)
}

/// Remove an MCP server from a specific agent's config.
pub fn remove_mcp_server_from_agent(
    server_name: &str,
    agent_name: &str,
) -> Result<(), String> {
    let (path, config) = claude_format_configs().into_iter()
        .find_map(|(name, path)| {
            if name == agent_name && path.exists() {
                let servers = read_claude_mcp(&path);
                Some((path, servers))
            } else {
                None
            }
        })
        .ok_or_else(|| format!("agent '{agent_name}' not found or config missing"))?;

    let before = config.len();
    let servers: Vec<_> = config.into_iter().filter(|s| s.name != server_name).collect();
    if servers.len() == before {
        return Err(format!("server '{server_name}' not found in agent '{agent_name}'"));
    }

    write_claude_mcp(&path, &servers)
}

/// Toggle a server's `disabled` state (flip) for a specific agent.
pub fn toggle_mcp_server_for_agent(
    server_name: &str,
    agent_name: &str,
) -> Result<(), String> {
    let (path, config) = claude_format_configs().into_iter()
        .find_map(|(name, path)| {
            if name == agent_name && path.exists() {
                let servers = read_claude_mcp(&path);
                Some((path, servers))
            } else {
                None
            }
        })
        .ok_or_else(|| format!("agent '{agent_name}' not found or config missing"))?;

    let mut servers = config;
    let server = servers.iter_mut()
        .find(|s| s.name == server_name)
        .ok_or_else(|| format!("server '{server_name}' not found in agent '{agent_name}'"))?;
    server.disabled = !server.disabled;

    write_claude_mcp(&path, &servers)
}

/// Scan MCP servers per agent, returning only agents with non-empty server lists.
pub fn scan_per_agent_mcp() -> Vec<(String, Vec<McpServerEntry>)> {
    claude_format_configs().into_iter()
        .filter_map(|(name, path)| {
            if !path.exists() { return None; }
            let servers = read_claude_mcp(&path);
            if servers.is_empty() { return None; }
            Some((name, servers))
        })
        .collect()
}

/// Read the raw JSON content of a specific agent's MCP config file.
pub fn read_agent_mcp_json(agent_name: &str) -> Option<String> {
    let path = claude_format_configs().into_iter()
        .find_map(|(name, p)| {
            if name == agent_name && p.exists() { Some(p) } else { None }
        })?;
    std::fs::read_to_string(&path).ok()
}

// ── Zenix-managed MCP servers (NDJSON) ─────────────────────────

fn zenix_mcp_path() -> PathBuf {
    platform::zenix_config_dir().join("mcp.json")
}

pub fn load_zenix_mcp_servers() -> Vec<ZenixMcpServer> {
    let path = zenix_mcp_path();
    if !path.exists() { return vec![]; }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    content.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn save_zenix_mcp_servers(servers: &[ZenixMcpServer]) -> Result<(), String> {
    let path = zenix_mcp_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    let ndjson: String = servers.iter()
        .map(|s| serde_json::to_string(s).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, if ndjson.is_empty() { "".into() } else { format!("{ndjson}\n") })
        .map_err(|e| format!("write: {e}"))
}

pub fn add_zenix_mcp_server(server: &ZenixMcpServer) -> Result<(), String> {
    let mut servers = load_zenix_mcp_servers();
    servers.retain(|s| s.name != server.name);
    servers.push(server.clone());
    save_zenix_mcp_servers(&servers)
}

pub fn remove_zenix_mcp_server(name: &str) -> Result<(), String> {
    let mut servers = load_zenix_mcp_servers();
    let before = servers.len();
    servers.retain(|s| s.name != name);
    if servers.len() == before {
        return Err(format!("server '{name}' not found"));
    }
    save_zenix_mcp_servers(&servers)
}

pub fn toggle_zenix_mcp_agent(server_name: &str, agent_name: &str) -> Result<(), String> {
    let mut servers = load_zenix_mcp_servers();
    let idx = servers.iter().position(|s| s.name == server_name)
        .ok_or_else(|| format!("server '{server_name}' not found"))?;
    let is_enabled = servers[idx].enabled_agents.iter().any(|a| a == agent_name);
    if is_enabled {
        servers[idx].enabled_agents.retain(|a| a != agent_name);
    } else {
        servers[idx].enabled_agents.push(agent_name.to_string());
    }
    let server = &servers[idx];
    save_zenix_mcp_servers(&servers)?;
    let _ = sync_zenix_server_to_agent(server, agent_name, !is_enabled);
    Ok(())
}

pub fn sync_zenix_server_to_agent(server: &ZenixMcpServer, agent_name: &str, enable: bool) -> Result<(), String> {
    let config_path = claude_format_configs().into_iter()
        .find(|(name, _)| name == agent_name)
        .map(|(_, p)| p)
        .ok_or_else(|| format!("unknown agent: {agent_name}"))?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }

    let mut entries = if config_path.exists() {
        read_claude_mcp(&config_path)
    } else {
        vec![]
    };

    if enable {
        if let Some(existing) = entries.iter_mut().find(|e| e.name == server.name) {
            existing.command = server.command.clone();
            existing.args = server.args.clone();
            existing.env = server.env.clone();
            existing.disabled = false;
        } else {
            entries.push(McpServerEntry {
                name: server.name.clone(),
                command: server.command.clone(),
                args: server.args.clone(),
                env: server.env.clone(),
                disabled: false,
            });
        }
    } else {
        entries.retain(|e| e.name != server.name);
    }

    write_claude_mcp(&config_path, &entries)
}

pub fn claude_format_agent_names() -> Vec<String> {
    claude_format_configs().into_iter().map(|(name, _)| name).collect()
}
