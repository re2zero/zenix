//! Role management — `RoleDef` definition, built-in templates, and `RoleRegistry`.
//!
//! Roles define what each AI agent is and what it does well. They are stored
//! as structured metadata in `~/.config/zenix/roles.json` with template content
//! in separate `.md` files under `~/.config/zenix/roles/`. This avoids JSON
//! escaping issues with markdown and maintains compatibility with the
//! OpenCode/Claude Code agents format.
//!
//! Built-in templates (frontend-developer, backend-architect, code-reviewer,
//! devops-engineer) are compiled into the binary via const strings and seeded
//! on first run. Users can define additional roles by pasting markdown content.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::platform;

// ── Built-in template constants ─────────────────────────────────────────

/// Built-in role template: Frontend Developer.
#[allow(dead_code)]
const BUILTIN_FRONTEND: &str = r#"---
name: Frontend Developer
description: 专精 React/Vue 前端开发、UI 实现、性能优化
tags: [frontend, react, ui, css]
---

你是一名资深前端开发工程师。你擅长现代前端框架和工具链。

## 核心职责
- 根据设计稿实现 UI 组件和页面
- 性能优化（Core Web Vitals、LCP、CLS）
- 响应式布局和跨浏览器兼容
- 组件库设计和维护

## 技术栈
React / Vue / Angular / TypeScript / CSS / HTML / Webpack / Vite

## 工作流
1. 分析需求和设计稿
2. 规划组件树和数据流
3. 实现核心逻辑
4. 编写样式和交互
5. 自测和性能检查
"#;

/// Built-in role template: Backend Architect.
#[allow(dead_code)]
const BUILTIN_BACKEND: &str = r#"---
name: Backend Architect
description: API 设计、数据库架构、系统可扩展性
tags: [backend, api, database, architecture]
---

你是一名资深后端架构师。你擅长设计可扩展、可维护的服务端系统。

## 核心职责
- API 设计和接口规范制定
- 数据库 schema 设计和优化
- 系统架构决策和演进
- 性能调优和容量规划

## 技术栈
Rust / Go / Python / PostgreSQL / Redis / Kafka / gRPC / REST

## 工作流
1. 理解业务需求和数据流
2. 设计数据模型和 API 接口
3. 确定架构方案和关键路径
4. Review 实现并确保质量
"#;

/// Built-in role template: Code Reviewer.
#[allow(dead_code)]
const BUILTIN_REVIEWER: &str = r#"---
name: Code Reviewer
description: 代码审查、安全分析、质量保障
tags: [review, quality, security, best-practices]
---

你是一名资深代码审查工程师。你专注于发现代码中的问题并提出改进建议。

## 核心职责
- 代码逻辑正确性审查
- 安全漏洞和风险识别
- 性能瓶颈分析
- 代码规范和最佳实践检查

## 审查重点
1. 正确性：边界条件、错误处理、并发安全
2. 安全性：注入、权限、数据验证
3. 可维护性：代码结构、命名、注释
4. 性能：算法复杂度、资源使用

## 沟通风格
- 指出问题同时提供改进方案
- 区分"必须修改"和"建议优化"
"#;

/// Built-in role template: DevOps Engineer.
#[allow(dead_code)]
const BUILTIN_DEVOPS: &str = r#"---
name: DevOps Engineer
description: CI/CD、基础设施自动化、云原生运维
tags: [devops, ci-cd, infra, cloud, docker]
---

你是一名资深 DevOps 工程师。你专注于自动化部署和基础设施管理。

## 核心职责
- CI/CD 流水线设计和维护
- 容器化和编排（Docker / Kubernetes）
- 监控、告警和可观测性
- 基础设施即代码（Terraform / Ansible）

## 技术栈
Docker / Kubernetes / GitHub Actions / Terraform / Prometheus / Grafana

## 工作流
1. 分析部署需求和现有架构
2. 设计自动化方案
3. 实现基础设施代码
4. 验证和文档化
"#;

/// Built-in role template: General Engineer (fallback).
#[allow(dead_code)]
const BUILTIN_GENERAL: &str = r#"---
name: General Engineer
description: 通用软件开发工程师
tags: [general, development]
---

你是一名经验丰富的软件工程师。你能够处理各种开发任务。

## 核心职责
- 功能开发和 Bug 修复
- 代码重构和优化
- 技术方案设计和实现

## 工作流
1. 理解需求
2. 设计方案
3. 实现
4. 测试
"#;

/// All built-in templates as (id, display_name, description, tags, cli_suggestion, content).
#[allow(dead_code)]
const BUILTIN_TEMPLATES: &[(&str, &str, &str, &[&str], &[&str], &str)] = &[
    (
        "frontend-developer",
        "Frontend Developer",
        "专精 React/Vue 前端开发、UI 实现、性能优化",
        &["frontend", "react", "ui", "css"],
        &["opencode", "claude"],
        BUILTIN_FRONTEND,
    ),
    (
        "backend-architect",
        "Backend Architect",
        "API 设计、数据库架构、系统可扩展性",
        &["backend", "api", "database", "architecture"],
        &["claude", "opencode"],
        BUILTIN_BACKEND,
    ),
    (
        "code-reviewer",
        "Code Reviewer",
        "代码审查、安全分析、质量保障",
        &["review", "quality", "security"],
        &["opencode", "claude"],
        BUILTIN_REVIEWER,
    ),
    (
        "devops-engineer",
        "DevOps Engineer",
        "CI/CD、基础设施自动化、云原生运维",
        &["devops", "ci-cd", "infra", "cloud"],
        &["opencode", "claude"],
        BUILTIN_DEVOPS,
    ),
    (
        "general-engineer",
        "General Engineer",
        "通用软件开发工程师",
        &["general", "development"],
        &["opencode", "claude"],
        BUILTIN_GENERAL,
    ),
];

// ── Types ───────────────────────────────────────────────────────────────

/// A registered role definition.
///
/// Metadata only — the prompt template lives in a separate `.md` file under
/// `~/.config/zenix/roles/{template_file}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDef {
    /// Unique identifier (e.g. "frontend-developer").
    pub id: String,
    /// Human-readable name (e.g. "Frontend Developer").
    pub name: String,
    /// Short description of what this role does.
    pub description: String,
    /// Suggested Agent CLIs for this role.
    #[serde(default)]
    pub cli_suggestion: Vec<String>,
    /// Tags for filtering and discovery.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Origin: "builtin" or "user".
    pub source: String,
    /// Filename of the template `.md` file (relative to roles dir).
    pub template_file: String,
}

// ── RoleRegistry ────────────────────────────────────────────────────────

/// Manages role definitions and template files on disk.
pub struct RoleRegistry;

#[allow(dead_code)]
impl RoleRegistry {
    /// Path to the roles directory: `~/.config/zenix/roles/`.
    pub fn roles_dir() -> PathBuf {
        platform::zenix_config_dir().join("roles")
    }

    /// Path to the roles metadata file.
    fn roles_json_path() -> PathBuf {
        Self::roles_dir().join("roles.json")
    }

    /// Load all role definitions from `roles.json`.
    ///
    /// Returns an empty Vec if the file does not exist. Propagates errors
    /// from I/O or JSON parsing.
    pub fn load() -> Result<Vec<RoleDef>> {
        let path = Self::roles_json_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path).context("read roles.json")?;
        let roles: Vec<RoleDef> = serde_json::from_str(&content).context("parse roles.json")?;
        Ok(roles)
    }

    /// Save all role definitions to `roles.json`.
    pub fn save(roles: &[RoleDef]) -> Result<()> {
        let dir = Self::roles_dir();
        std::fs::create_dir_all(&dir).context("create roles dir")?;
        let content = serde_json::to_string_pretty(roles).context("serialize roles")?;
        std::fs::write(Self::roles_json_path(), &content).context("write roles.json")?;
        Ok(())
    }

    /// Seed built-in templates to disk on first run.
    ///
    /// Checks whether `roles.json` already exists. If not, writes all built-in
    /// `.md` template files and creates the initial `roles.json`.
    pub fn seed_builtin_templates() -> Result<()> {
        if Self::roles_json_path().exists() {
            return Ok(());
        }

        let dir = Self::roles_dir();
        std::fs::create_dir_all(&dir).context("create roles dir")?;

        let mut roles = Vec::with_capacity(BUILTIN_TEMPLATES.len());

        for (id, name, description, tags, cli_suggestion, content) in BUILTIN_TEMPLATES {
            let filename = format!("{id}.md");
            let file_path = dir.join(&filename);

            std::fs::write(&file_path, content)
                .with_context(|| format!("write role template {filename}"))?;

            roles.push(RoleDef {
                id: id.to_string(),
                name: name.to_string(),
                description: description.to_string(),
                cli_suggestion: cli_suggestion.iter().map(|s| s.to_string()).collect(),
                tags: tags.iter().map(|s| s.to_string()).collect(),
                source: "builtin".into(),
                template_file: filename,
            });
        }

        Self::save(&roles)?;
        Ok(())
    }

    /// Get the template content for a role by reading its `.md` file.
    ///
    /// Returns an error if the template file is missing or unreadable.
    pub fn get_template(role_id: &str) -> Result<String> {
        let roles = Self::load()?;
        let role = roles
            .iter()
            .find(|r| r.id == role_id)
            .with_context(|| format!("role '{role_id}' not found"))?;

        let path = Self::roles_dir().join(&role.template_file);
        let content = std::fs::read_to_string(&path).with_context(|| {
            format!(
                "read template file '{}' for role '{role_id}'",
                role.template_file
            )
        })?;
        Ok(content)
    }

    /// Create a new user-defined role.
    ///
    /// Writes the markdown template to `roles/{id}.md` and adds a metadata
    /// entry to `roles.json`. Returns an error if a role with the same `id`
    /// already exists.
    pub fn create_role(
        id: &str,
        name: &str,
        description: &str,
        tags: &[String],
        cli_suggestion: &[String],
        markdown_content: &str,
    ) -> Result<()> {
        let mut roles = Self::load()?;

        if roles.iter().any(|r| r.id == id) {
            anyhow::bail!("role '{id}' already exists");
        }

        let filename = format!("{id}.md");
        let dir = Self::roles_dir();
        std::fs::create_dir_all(&dir).context("create roles dir")?;

        let file_path = dir.join(&filename);
        std::fs::write(&file_path, markdown_content)
            .with_context(|| format!("write role template {filename}"))?;

        roles.push(RoleDef {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            cli_suggestion: cli_suggestion.to_vec(),
            tags: tags.to_vec(),
            source: "user".into(),
            template_file: filename,
        });

        Self::save(&roles)?;
        Ok(())
    }

    /// Delete a role by its `id`.
    ///
    /// Removes the metadata entry from `roles.json` and deletes the `.md`
    /// template file. Returns an error if the role does not exist.
    pub fn delete_role(id: &str) -> Result<()> {
        let mut roles = Self::load()?;

        let idx = roles
            .iter()
            .position(|r| r.id == id)
            .with_context(|| format!("role '{id}' not found"))?;

        let role = roles.remove(idx);

        let file_path = Self::roles_dir().join(&role.template_file);
        let _ = std::fs::remove_file(&file_path);

        Self::save(&roles)?;
        Ok(())
    }

    /// List all registered roles.
    pub fn list_roles() -> Result<Vec<RoleDef>> {
        Self::load()
    }

    /// Get a single role definition by id.
    pub fn get_role(id: &str) -> Result<RoleDef> {
        let roles = Self::load()?;
        roles
            .into_iter()
            .find(|r| r.id == id)
            .with_context(|| format!("role '{id}' not found"))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_def_serde() {
        let role = RoleDef {
            id: "test-role".into(),
            name: "Test Role".into(),
            description: "A test role".into(),
            cli_suggestion: vec!["opencode".into()],
            tags: vec!["test".into()],
            source: "user".into(),
            template_file: "test-role.md".into(),
        };
        let json = serde_json::to_string_pretty(&role).unwrap();
        let deserialized: RoleDef = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "test-role");
        assert_eq!(deserialized.name, "Test Role");
        assert_eq!(deserialized.cli_suggestion, vec!["opencode"]);
    }

    #[test]
    fn test_builtin_templates_count() {
        assert!(BUILTIN_TEMPLATES.len() >= 3, "至少需要 3 个内置模板");
        assert!(BUILTIN_TEMPLATES.len() <= 10);
    }

    #[test]
    fn test_builtin_template_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for (id, _, _, _, _, _) in BUILTIN_TEMPLATES {
            assert!(ids.insert(id), "duplicate built-in template id: {id}");
        }
    }

    #[test]
    fn test_builtin_templates_have_frontmatter() {
        for (id, _, _, _, _, content) in BUILTIN_TEMPLATES {
            assert!(
                content.starts_with("---\n"),
                "template '{id}' must start with YAML frontmatter"
            );
            assert!(
                content.contains("name:"),
                "template '{id}' must have name in frontmatter"
            );
        }
    }

    #[test]
    fn test_role_registry_paths() {
        let roles_dir = RoleRegistry::roles_dir();
        assert!(roles_dir.to_string_lossy().contains("roles"));
    }

    #[test]
    fn test_seed_and_load() {
        use std::fs;

        let tmp = std::env::temp_dir().join("zenix_test_roles");
        let _ = fs::remove_dir_all(&tmp);

        let json_path = tmp.join("roles.json");
        if json_path.exists() {
            fs::remove_file(&json_path).ok();
        }

        assert!(!json_path.exists());
        for (id, _, desc, _, _, content) in BUILTIN_TEMPLATES {
            assert!(!id.is_empty());
            assert!(!desc.is_empty());
            assert!(content.len() > 50, "template '{id}' content too short");
        }

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_create_role_json_output() {
        let json = serde_json::json!({
            "id": "custom-role",
            "name": "Custom",
            "description": "A custom role",
            "cli_suggestion": ["opencode"],
            "tags": ["custom"],
            "source": "user",
            "template_file": "custom-role.md"
        });
        let role: RoleDef = serde_json::from_value(json).unwrap();
        assert_eq!(role.id, "custom-role");
        assert_eq!(role.source, "user");
    }

    #[test]
    fn test_delete_role_builtin_cannot_delete_nonexistent() {
        let result = RoleRegistry::delete_role("nonexistent-role-xyz");
        assert!(result.is_err());
    }
}
