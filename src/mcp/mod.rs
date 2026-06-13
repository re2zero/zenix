//! Unified MCP server management — read/write MCP configs across agents.
//!
//! Supported agent formats:
//!   Claude-format JSON (Claude, OpenCode, Pi, OMP, Kilo):
//!     `{ "mcpServers": { <name>: { command, args?, env?, disabled? } } }`
//!
//! Agents without MCP config files (Copilot, Droid, Kimi, Qodercli, Cursor, Codex, Hermes)
//! are skipped — their MCP is managed externally or via different mechanisms.

use std::collections::HashMap;
use std::path::PathBuf;

/// A single MCP server entry.
#[derive(Debug, Clone)]
pub struct McpServerEntry {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub disabled: bool,
}

// ── Per-agent config paths ───────────────────────────────────────────

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()))
}

fn env_or_home(var: &str, fallback: &str) -> Option<PathBuf> {
    if let Ok(dir) = std::env::var(var) {
        let p = PathBuf::from(dir);
        if p.exists() { return Some(p); }
    }
    let p = home().join(fallback.trim_start_matches("~/"));
    if p.exists() { Some(p) } else { None }
}

/// Returns (agent_name, path) for agents that use Claude-format JSON MCP configs.
fn claude_format_configs() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();

    // Claude
    if let Some(dir) = env_or_home("CLAUDE_CONFIG_DIR", "~/.claude") {
        out.push(("claude".into(), dir.join("claude_desktop_config.json")));
    }
    // OpenCode
    out.push(("opencode".into(), home().join(".config/opencode/config.json")));
    // Pi
    if let Some(dir) = env_or_home("PI_CODING_AGENT_DIR", "~/.pi-coding-agent") {
        out.push(("pi".into(), dir.join("config.json")));
    }
    // OMP
    out.push(("omp".into(), home().join(".config/omp/config.json")));
    // Kilo
    out.push(("kilo".into(), home().join(".config/kilo/config.json")));

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
