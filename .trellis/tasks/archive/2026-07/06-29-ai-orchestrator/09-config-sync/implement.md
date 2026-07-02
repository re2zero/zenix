1. `src/api_provider/mod.rs` 扩展：
   - `sync_to_all_enabled_agents()`
   - `sync_all_providers()`
   - `upsert_and_sync()`
   - `validate_all_syncs()` + `SyncStatus` / `AgentSyncStatus`
2. 设置面板中增加"同步到 Agents"按钮
3. `cargo check` 验证
