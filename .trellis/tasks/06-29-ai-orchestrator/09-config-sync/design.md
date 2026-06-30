# API Provider Agent 配置同步增强 — 设计

## 当前状态

现有 `api_provider/mod.rs` 已有：
- `sync_api_to_agent()` — 写入 agent config
- `unsync_api_from_agent()` — 移除 agent config
- `toggle_api_agent()` — 切换启用状态
- `to_opencode_provider()` — OpenCode 格式
- `to_claude_env()` — Claude 格式
- `to_openai_env()` — OpenAI 格式

## 增强内容

### 批量同步函数

```rust
// api_provider/mod.rs 新增

/// 将 provider 同步到所有已启用的 agent
pub fn sync_to_all_enabled_agents(provider: &ApiProviderConfig) -> Result<(), String> {
    for agent_name in &provider.enabled_agents {
        sync_api_to_agent(provider, agent_name)?;
    }
    Ok(())
}

/// 将所有 provider 同步到各自的 agent
pub fn sync_all_providers() -> Result<(), String> {
    let providers = load_all();
    let mut errors = Vec::new();
    for provider in &providers {
        if !provider.enabled_agents.is_empty() {
            if let Err(e) = sync_to_all_enabled_agents(provider) {
                errors.push(format!("{}: {e}", provider.name));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

/// 保存 + 自动同步
pub fn upsert_and_sync(config: &ApiProviderConfig) -> Result<(), String> {
    upsert(config)?;
    sync_to_all_enabled_agents(config)?;
    Ok(())
}
```

### 验证函数

```rust
/// 验证所有 provider 的同步状态
pub fn validate_all_syncs() -> Result<Vec<SyncStatus>, String> {
    let providers = load_all();
    let mut results = Vec::new();

    for provider in &providers {
        let mut status = SyncStatus {
            provider_name: provider.name.clone(),
            agents: Vec::new(),
        };

        for agent_name in &provider.enabled_agents {
            let config_path = get_api_config_path(agent_name)?;
            let ok = config_path.exists();
            status.agents.push(AgentSyncStatus {
                agent_name: agent_name.clone(),
                config_path: config_path.display().to_string(),
                synced: ok,
            });
        }

        results.push(status);
    }

    Ok(results)
}

pub struct SyncStatus {
    pub provider_name: String,
    pub agents: Vec<AgentSyncStatus>,
}

pub struct AgentSyncStatus {
    pub agent_name: String,
    pub config_path: String,
    pub synced: bool,
}
```
