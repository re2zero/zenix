# 系统提示词外部化重构 — 技术设计

## 总体方案

```
┌──────────────────────────────────────────────────┐
│  ~/.config/zenix/prompts/                         │
│    orchestrator.md  ← 用户可编辑                  │
│    (future: assistant.md, code-reviewer.md...)    │
└──────┬───────────────────────────────────────────┘
       │ init() → seed + read once at startup
       ▼
┌──────────────────────────────────────────┐
│  src/ai/prompts.rs                        │
│                                           │
│  ┌─────────────────────┐                  │
│  │ PROMPT_CACHE:        │ ← in-memory      │
│  │  Mutex<HashMap>     │    cache          │
│  │   "orchestrator" →  │                  │
│  │     String          │                  │
│  └────────┬────────────┘                  │
│           │ get_template("orchestrator")  │
│           ▼                               │
│  FALLBACK_ORCHESTRATOR → compile-time     │
└──────┬───────────────────────────────────┘
       │ build_system_prompt()
       ▼
┌──────────────────────────────────────────┐
│  engine.rs                                │
│                                           │
│  template = prompts::get_template(...)    │
│    → replace {lang_instruction}           │
│    → append 角色列表 (if any)             │
│    → append 工作区信息 (if any)           │
│                                           │
│  → 最终 system_prompt (String)            │
└──────────────────────────────────────────┘
```

**变化**: 工具列表现在在模板文件中，不再是代码拼接。

## 加载时机

**启动时加载，缓存到内存，不每次读盘。**

- `init()` 在 `ZenixApp::new()` 中同步执行：seed prompts 目录 → 读取所有 `.md` 文件 → 缓存
- `get_template()` 从内存缓存取内容，缓存缺失时返回编译期 fallback 常量
- 修改 prompt 文件后需重启 zenix 生效（同 config.json 行为）

## 模块变更

### 新增: `src/ai/prompts.rs` — 提示词管理

```rust
use std::collections::HashMap;
use std::sync::Mutex;

/// 启动时初始化：seed + 加载所有 .md 文件到缓存。
/// 失败时只 log warn，不阻塞启动。
pub fn init() {
    if let Err(e) = seed_prompts() {
        tracing::warn!(target: "prompts", error = %e, "failed to seed prompts");
    }
    load_all();
    tracing::info!(target: "prompts", count = ?cache().lock().unwrap().len(), "prompts loaded");
}

/// 获取已加载的模板内容。
/// 缓存中有 → 返回 clone；无 → 返回编译期 fallback 常量。
/// 调用方无需感知 fallback 逻辑。
pub fn get_template(name: &str) -> String {
    cache().lock().unwrap()
        .get(name)
        .cloned()
        .unwrap_or_else(|| match name {
            "orchestrator" => FALLBACK_ORCHESTRATOR.to_string(),
            _ => FALLBACK_ORCHESTRATOR.to_string(),
        })
}

/// 按文件粒度 seed。已有文件不覆盖，缺失文件写入内置版本。
fn seed_prompts() -> Result<()> {
    let dir = platform::zenix_config_dir().join("prompts");
    std::fs::create_dir_all(&dir)?;
    seed_one(&dir, "orchestrator", BUILTIN_ORCHESTRATOR)?;
    // future: seed_one(&dir, "assistant", BUILTIN_ASSISTANT)?;
    Ok(())
}

fn seed_one(dir: &Path, name: &str, content: &str) -> Result<()> {
    let path = dir.join(format!("{name}.md"));
    if !path.exists() {
        std::fs::write(&path, content)?;
    }
    Ok(())
}

/// 读取 prompts 目录下所有 .md 文件到缓存。
fn load_all() {
    let dir = platform::zenix_config_dir().join("prompts");
    let dir = match std::fs::read_dir(&dir) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(target: "prompts", error = %e, "prompts dir not readable");
            return;
        }
    };
    let mut map = HashMap::new();
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                match std::fs::read_to_string(&path) {
                    Ok(content) => { map.insert(name.to_string(), content); }
                    Err(e) => tracing::warn!(target: "prompts", ?path, error = %e, "failed to read prompt file"),
                }
            }
        }
    }
    *cache().lock().unwrap() = map;
}

fn cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
    &CACHE
}
```

### 修改: `src/ai/engine.rs`

**删除**:
- `DEFAULT_SYSTEM_PROMPT` 常量

**保留不变**:
- `build_system_prompt()` 函数签名 — `(roles_summary, workspace_info, lang) -> String`
- `lang_instruction()` — 替换 `{lang_instruction}` 用
- `detect_language()` — 不变

**修改**:
- `build_system_prompt()` 调 `prompts::get_template("orchestrator")` 获取模板
- 模板中已含工具列表、任务分发规则、工作流、约束
- 代码只追加角色列表、工作区信息（均为运行时动态内容）

### 修改: `src/ai/mod.rs`

- 添加 `pub mod prompts;`

### 修改: `src/app.rs`

- 在 `ZenixApp::new()` 主初始化块中调 `crate::ai::prompts::init()`
- 位置：config 加载之后，其他 seed/init 附近

## 模板文件格式

### `orchestrator.md`（内置版本，seed 写入）

```markdown
# Zenix 编排器系统提示词
# 编辑此文件可自定义 AI 编排器的行为和回复风格。
# 修改后重启 zenix 生效。
# {lang_instruction} 是语言指令占位符，会自动替换。

你是 ZenixAI 编排器。你负责将用户的任务原样分发给专业 Agent CLI。
你不分析或改写任务内容，只负责分发和汇总结果。
{lang_instruction}

## 可用工具
- spawn_worker: 创建一个 worker 并启动 Agent CLI，支持网格位置
- send_text: 向 worker 发送文本指令
- read_output: 读取 worker 的最新输出
- wait_for_output: 等待 worker 输出包含指定模式（如 [DONE]）
- list_workers: 查看所有运行中的 worker
- close_worker: 关闭 worker pane

## 任务分发
- 用户说"帮我问一下 X"、"帮我查一下 Y"等委托请求 → 立即使用 spawn_worker
- 直接将具体任务原样分发，不要分析或改写任务内容
- 每个委托创建一个独立的 worker
- 简单的问答或闲聊直接回复，不要 spawn worker

## worker_name 命名规范
给每个 worker 分配一个简短有意义的名称，格式: <cli缩写>_<任务标识>
例如：oc_checkgit、cc_refactor、claude_debug

## 工作流
1. 用 spawn_worker 创建 worker，分发具体任务
2. 用 wait_for_output 等待 worker 完成
3. 汇总结果，给出最终回复

## 约束
- 不分析或改写要分发的任务内容，直接原样传递
- 你不直接写代码或运行命令
- {lang_instruction}
```

### 语言占位符

| 占位符 | 替换为 |
|--------|--------|
| `{lang_instruction}` | `"用中文回复。"` (zh) 或 `"Reply in the same language as the user's input."` (en) |

## 初始化流程

```
ZenixApp::new()
  → prompts::init()
    → seed_prompts()
      → ~/.config/zenix/prompts/ 不存在 → 创建
      → orchestrator.md 不存在 → 写入 BUILTIN_ORCHESTRATOR
      → 已存在 → 跳过（不覆盖用户修改）
    → load_all()
      → 遍历 prompts/*.md
      → 逐个读取内容入缓存
      → 文件读失败 → log warn，跳过该文件
      → 无任何文件 → 缓存为空

每次聊天构建 prompt:
  → build_system_prompt(roles, workspace, lang)
    → template = prompts::get_template("orchestrator")
      ├── 缓存命中 → 返回 clone
      └── 缓存未命中 → 返回 FALLBACK_ORCHESTRATOR
    → template.replace("{lang_instruction}", lang_instruction(lang))
    → 追加 ## 可用角色（if has_roles）
    → 追加 ## 当前工作空间（if has_workspace）
    → 返回最终 String
```

## seed 机制

```rust
fn seed_prompts() -> Result<()> {
    let dir = platform::zenix_config_dir().join("prompts");
    std::fs::create_dir_all(&dir)?;
    seed_one(&dir, "orchestrator", BUILTIN_ORCHESTRATOR)?;
    // future: seed_one(&dir, "assistant", BUILTIN_ASSISTANT)?;
    Ok(())
}

fn seed_one(dir: &Path, name: &str, content: &str) -> Result<()> {
    let path = dir.join(format!("{name}.md"));
    if !path.exists() {
        std::fs::write(&path, content)?;
    }
    Ok(())
}
```

## 编译期常量

两个常量 `FALLBACK_ORCHESTRATOR` 和 `BUILTIN_ORCHESTRATOR`：

| 常量 | 用途 | 区别 |
|------|------|------|
| `FALLBACK_ORCHESTRATOR` | 运行时缓存缺失时使用 | 无注释头，仅纯内容 |
| `BUILTIN_ORCHESTRATOR` | 首次 seed 写入文件 | 带 `#` 注释头说明可编辑 |

两者都包含 `{lang_instruction}` 占位符。

## 兼容性

- 现有 `OrchestratorEngine::new()` 和 `.with_system_prompt()` 不变
- `chat_with_tools()` 签名的 system_prompt 参数不变
- 动态内容（角色、工作区）追加顺序不变
- 用户无 prompts 目录 / 文件损坏 / 无权限 → 日志警告 → 透明降级到 fallback 常量

## 维护注意

工具列表从代码移到了模板文件。新增工具时需要：
1. 在代码中实现工具
2. 更新 `BUILTIN_ORCHESTRATOR` 常量中的工具列表
3. 已有用户需要手动编辑 `orchestrator.md` 添加工具描述，或删除文件后重启重新 seed

## 文件结构总结

### 新增
| 文件 | 作用 |
|------|------|
| `src/ai/prompts.rs` | `init()` + `get_template()` + `seed_prompts()` + `load_all()` + 编译期常量 |

### 修改
| 文件 | 变更 |
|------|------|
| `src/ai/engine.rs` | 删除 `DEFAULT_SYSTEM_PROMPT`；`build_system_prompt()` 改从 `get_template()` 获取模板 |
| `src/ai/mod.rs` | 加 `pub mod prompts;` |
| `src/app.rs` | `ZenixApp::new()` 中调 `prompts::init()` |

### 运行时文件
| 路径 | 说明 |
|------|------|
| `~/.config/zenix/prompts/orchestrator.md` | 编排器提示词模板，用户可编辑；重启生效 |
