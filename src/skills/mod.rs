//! Unified skills management — scan and symlink skills across compatible agents.
//!
//! Compatibility matrix:
//!   Group A (Claude-format: directory with SKILL.md): Claude, Codex, Pi, OMP, Kilo, Hermes
//!   Group B (OpenCode-format, also reads Claude-format): OpenCode
//!
//! OpenCode can read both formats, so skills for Group A are also usable by OpenCode.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::platform;

/// Information about a skill installed across agents.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    /// Agents that have this skill as a real directory (not symlink).
    pub source_agents: Vec<String>,
    /// Agents that have this skill as a symlink.
    pub linked_agents: Vec<String>,
}

// ── Per-agent skills directories ─────────────────────────────────────

/// Returns Vec<(agent_name, skills_dir)> for agents that support skill directories.
fn agent_skills_dirs() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();

    // Claude
    if let Some(dir) = platform::env_or_home("CLAUDE_CONFIG_DIR", "~/.claude") {
        out.push(("claude".into(), dir.join("skills")));
    }
    // Codex
    if let Some(dir) = platform::env_or_home("CODEX_HOME", "~/.codex") {
        out.push(("codex".into(), dir.join("skills")));
    }
    // OpenCode
    out.push(("opencode".into(), platform::opencode_config_dir().join("skills")));
    // Pi
    if let Some(dir) = platform::env_or_home("PI_CODING_AGENT_DIR", "~/.pi-coding-agent") {
        out.push(("pi".into(), dir.join("skills")));
    }
    // OMP
    out.push(("omp".into(), platform::omp_config_dir().join("skills")));
    // Kilo
    out.push(("kilo".into(), platform::kilo_config_dir().join("skills")));
    // Hermes
    out.push(("hermes".into(), platform::home_dir().join(".hermes/skills")));

    out
}

// ── Scanning ─────────────────────────────────────────────────────────

/// Scan all agents for installed skills.
pub fn scan_all_skills() -> HashMap<String, SkillInfo> {
    let mut map: HashMap<String, SkillInfo> = HashMap::new();

    for (agent, dir) in agent_skills_dirs() {
        if !dir.exists() { continue; }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            // Check if contains SKILL.md
            if !path.join("SKILL.md").exists() { continue; }
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

            let is_symlink = std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);

            map.entry(name.clone())
                .and_modify(|info| {
                    if is_symlink {
                        info.linked_agents.push(agent.clone());
                    } else {
                        info.source_agents.push(agent.clone());
                    }
                })
                .or_insert_with(|| {
                    let (sources, linked) = if is_symlink {
                        (vec![], vec![agent.clone()])
                    } else {
                        (vec![agent.clone()], vec![])
                    };
                    SkillInfo { name, source_agents: sources, linked_agents: linked }
                });
        }
    }

    map
}

// ── Link / Unlink ────────────────────────────────────────────────────

/// Choose the best source directory for a skill. Prefers Claude, then any Group A agent.
pub fn best_source(skill: &SkillInfo) -> Option<(String, PathBuf)> {
    let preferred = ["claude", "codex", "pi", "omp", "kilo", "hermes"];
    for agent in &preferred {
        if skill.source_agents.contains(&agent.to_string()) {
            if let Some(dir) = source_dir_for(agent, &skill.name) {
                return Some((agent.to_string(), dir));
            }
        }
    }
    for agent in &skill.source_agents {
        if let Some(dir) = source_dir_for(agent, &skill.name) {
            return Some((agent.clone(), dir));
        }
    }
    None
}

fn source_dir_for(agent: &str, skill_name: &str) -> Option<PathBuf> {
    for (name, dir) in agent_skills_dirs() {
        if name == agent {
            let p = dir.join(skill_name);
            if p.exists() && p.is_dir() && p.join("SKILL.md").exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Create a symlink from source skill directory to target agent's skills dir.
pub fn link_skill(skill_name: &str, target_agent: &str) -> Result<(), String> {
    let skills = scan_all_skills();
    let info = skills.get(skill_name).ok_or_else(|| format!("skill '{skill_name}' not found"))?;

    let (source_agent, source_dir) = best_source(info)
        .ok_or_else(|| format!("no source agent found for '{skill_name}'"))?;

    let target_dir = agent_skills_dirs().into_iter()
        .find(|(a, _)| a == target_agent)
        .map(|(_, d)| d)
        .ok_or_else(|| format!("target agent '{target_agent}' not supported"))?;

    if !target_dir.exists() {
        std::fs::create_dir_all(&target_dir)
            .map_err(|e| format!("create target dir: {e}"))?;
    }

    let link_path = target_dir.join(skill_name);
    if link_path.exists() {
        return Err(format!("'{skill_name}' already exists in {target_agent}"));
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(&source_dir, &link_path)
        .map_err(|e| format!("symlink from {source_agent} to {target_agent}: {e}"))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&source_dir, &link_path)
        .map_err(|e| format!("symlink from {source_agent} to {target_agent}: {e}"))?;
    Ok(())
}

/// Remove a symlink from target agent's skills dir. NEVER deletes real directories.
pub fn unlink_skill(skill_name: &str, target_agent: &str) -> Result<(), String> {
    let target_dir = agent_skills_dirs().into_iter()
        .find(|(a, _)| a == target_agent)
        .map(|(_, d)| d)
        .ok_or_else(|| format!("target agent '{target_agent}' not supported"))?;

    let link_path = target_dir.join(skill_name);
    if !link_path.exists() {
        return Err(format!("'{skill_name}' not found in {target_agent}"));
    }

    let metadata = std::fs::symlink_metadata(&link_path)
        .map_err(|e| format!("stat: {e}"))?;

    if metadata.file_type().is_symlink() {
        std::fs::remove_file(&link_path)
            .map_err(|e| format!("unlink: {e}"))
    } else {
        Err(format!("'{skill_name}' in {target_agent} is a real directory, not a symlink — refusing to delete"))
    }
}

/// Centralized skills directory managed by zenix.
pub fn zenix_skills_dir() -> PathBuf {
    platform::zenix_config_dir().join("skills")
}

/// List skills in the zenix centralized directory.
pub fn zenix_skills() -> Vec<SkillInfo> {
    let dir = zenix_skills_dir();
    if !dir.exists() { return vec![]; }
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };
    entries.flatten()
        .filter(|e| e.path().is_dir() && e.path().join("SKILL.md").exists())
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            SkillInfo { name, source_agents: vec!["zenix".into()], linked_agents: vec![] }
        })
        .collect()
}

/// Install a skill from a git URL into the zenix skills directory.
/// Returns the skill name on success.
pub fn install_from_git(url: &str) -> Result<String, String> {
    let dir = zenix_skills_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create skills dir: {e}"))?;

    // Derive skill name from URL: last path segment without .git suffix
    let name = url.trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit('/').next()
        .unwrap_or("unknown")
        .to_string();

    if name.is_empty() || name == "unknown" {
        return Err("cannot derive skill name from URL".into());
    }

    let dest = dir.join(&name);
    if dest.exists() {
        return Err(format!("skill '{name}' already exists"));
    }

    let status = std::process::Command::new("git")
        .args(["clone", "--depth", "1", url, &dest.to_string_lossy()])
        .status()
        .map_err(|e| format!("git clone failed: {e}"))?;

    if !status.success() {
        // Clean up partial clone
        let _ = std::fs::remove_dir_all(&dest);
        return Err(format!("git clone exited with {:?}", status.code()));
    }

    // Validate: must contain SKILL.md
    if !dest.join("SKILL.md").exists() {
        let _ = std::fs::remove_dir_all(&dest);
        return Err("cloned repo does not contain SKILL.md — not a valid skill".into());
    }

    Ok(name)
}

/// Install a skill from a local directory (copies it into zenix skills dir).
/// Returns the skill name on success.
pub fn install_from_local(local_path: &str) -> Result<String, String> {
    let src = PathBuf::from(local_path);
    if !src.is_dir() {
        return Err(format!("'{local_path}' is not a directory"));
    }
    if !src.join("SKILL.md").exists() {
        return Err("directory does not contain SKILL.md — not a valid skill".into());
    }

    let dir = zenix_skills_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create skills dir: {e}"))?;

    let name = src.file_name()
        .ok_or_else(|| String::from("cannot derive skill name from path"))?
        .to_string_lossy()
        .to_string();

    let dest = dir.join(&name);
    if dest.exists() {
        return Err(format!("skill '{name}' already exists"));
    }

    copy_dir_recursive(&src, &dest)
        .map_err(|e| format!("copy: {e}"))?;

    Ok(name)
}

/// Remove a skill from the zenix centralized directory.
pub fn remove_zenix_skill(name: &str) -> Result<(), String> {
    let dir = zenix_skills_dir().join(name);
    if !dir.exists() {
        return Err(format!("skill '{name}' not found in zenix skills"));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("remove: {e}"))
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("create dir: {e}"))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("read dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read entry: {e}"))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| format!("copy file: {e}"))?;
        }
    }
    Ok(())
}
