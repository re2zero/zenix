# 系统提示词外部化重构 — 执行计划

## 执行顺序

```
Step 1: 创建 src/ai/prompts.rs — init/get_template/seed/load_all/builtin 常量
   ↓
Step 2: 修改 engine.rs — build_system_prompt() 使用 get_template()
   ↓
Step 3: 修改 ai/mod.rs — 注册 prompts 模块
   ↓
Step 4: 修改 app.rs — 启动时调用 prompts::init()
   ↓
Step 5: cargo check + cargo clippy
   ↓
Step 6: 手动验证
```

## Step 1: 创建 prompts.rs

**文件**: `src/ai/prompts.rs`（新增 ~160 行）

**核心接口**:

```rust
/// 启动时初始化：seed + 加载到缓存
pub fn init();

/// 获取模板：缓存 → 找不到 → 返回编译期 fallback
pub fn get_template(name: &str) -> String;
```

**内部函数**:

```rust
fn seed_prompts() -> Result<()>;      // 按文件粒度 seed
fn seed_one(dir: &Path, name: &str, content: &str) -> Result<()>;  // 单文件 seed
fn load_all();                        // 读取 prompts/*.md 入缓存
fn cache() -> &'static Mutex<HashMap<String, String>>;  // 静态缓存引用
```

**常量**:

```rust
/// 编译期 fallback（运行时缓存缺失时使用）
const FALLBACK_ORCHESTRATOR: &str = r#"..."#;

/// 内置模板（首次 seed 写入文件用，带注释头）
const BUILTIN_ORCHESTRATOR: &str = r#"..."#;
```

**`FALLBACK_ORCHESTRATOR` 内容**:
```
你是 ZenixAI 编排器。你负责将用户的任务原样分发给专业 Agent CLI。
你不分析或改写任务内容，只负责分发和汇总结果。
{lang_instruction}

## 可用工具
(工具列表...)

## 任务分发
(规则...)

## worker_name 命名规范
(规范...)

## 工作流
(步骤...)

## 约束
(约束...)
{lang_instruction}
```

**`BUILTIN_ORCHESTRATOR`** = `FALLBACK_ORCHESTRATOR` + 顶部 `#` 注释头。

**验证**: `cargo check`

## Step 2: 修改 engine.rs

**删除**:
- `DEFAULT_SYSTEM_PROMPT` 常量（第 39-40 行）

**修改** `build_system_prompt()`:

```rust
pub fn build_system_prompt(roles_summary: &str, workspace_info: &str, lang: &str) -> String {
    let mut template = crate::ai::prompts::get_template("orchestrator");
    template = template.replace("{lang_instruction}", lang_instruction(lang));

    let mut prompt = String::with_capacity(template.len() + 512);
    prompt.push_str(&template);
    prompt.push_str("\n\n");

    if !roles_summary.is_empty() {
        prompt.push_str("## 可用角色\n");
        prompt.push_str(roles_summary);
        prompt.push_str("\n\n");
    }
    if !workspace_info.is_empty() {
        prompt.push_str("## 当前工作空间\n");
        prompt.push_str(workspace_info);
        prompt.push_str("\n\n");
    }
    prompt
}
```

**注意**: 不再有 `!has_roles && !has_workspace` 提前返回短路——因为模板内容始终存在（fallback 兜底），无需区分有无动态内容。`get_template()` 内部处理 fallback，engine.rs 不需要知道 fallback 常量的存在。

**验证**: `cargo check`

## Step 3: 修改 ai/mod.rs

```rust
// 在 pub mod workers; 之后添加
pub mod prompts;
```

**验证**: `cargo check`

## Step 4: 修改 app.rs

在 `ZenixApp::new()` 中，config 初始化完成后、MCP Server 启动前，添加：

```rust
// 加载系统提示词模板
crate::ai::prompts::init();
```

**位置参考**: 约在 app.rs 第 320-330 行区域（各 field 初始化之后，MCP server 启动之前）。

**验证**: `cargo check`

## Step 5: 质量检查

```bash
cargo check
cargo clippy  # 无新增警告
```

## Step 6: 手动验证

```bash
# 1. 首次运行 — seed 和加载
rm -rf ~/.config/zenix/prompts/
cargo run
# 确认 ~/.config/zenix/prompts/orchestrator.md 被创建
# 确认 AI 聊天正常

# 2. 修改文件后重启生效
vim ~/.config/zenix/prompts/orchestrator.md  # 修改内容
# 重启 zenix
# 发送消息，确认新 prompt 生效

# 3. 删除文件后降级
rm ~/.config/zenix/prompts/orchestrator.md
# 重启 zenix → 重新 seed
# AI 聊天正常使用重新 seed 的模板

# 4. 模拟文件不可读 — fallback 到编译期常量
chmod 000 ~/.config/zenix/prompts/orchestrator.md
# 重启 zenix → log warn → 使用 FALLBACK_ORCHESTRATOR
# AI 聊天正常
chmod 644 ~/.config/zenix/prompts/orchestrator.md
```

## 回滚点

| 步骤 | 回滚操作 |
|------|---------|
| Step 1 | `git checkout -- src/ai/prompts.rs` |
| Step 2 | `git checkout -- src/ai/engine.rs` |
| Step 3 | `git checkout -- src/ai/mod.rs` |
| Step 4 | `git checkout -- src/app.rs` |

## 质量门禁

- [ ] `cargo check` 无错误
- [ ] `cargo clippy` 无新增警告
- [ ] 启动时 seed：prompts 目录不存在 → 创建；orchestrator.md 不存在 → 写入
- [ ] 已有文件不被覆盖
- [ ] 启动后 `get_template("orchestrator")` 返回文件内容
- [ ] 文件不可读/不存在 → `get_template()` 返回 fallback 常量
- [ ] `{lang_instruction}` 替换正确（zh→用中文回复; en→Reply in the same language）
- [ ] 动态内容（角色、工作区）正常追加到模板内容之后
