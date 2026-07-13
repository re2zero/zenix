# 系统提示词外部化重构

## Goal

将硬编码的 `DEFAULT_SYSTEM_PROMPT` 和 `build_system_prompt()` 中的编排器指令提取为外部 markdown 模板文件，使用户可直接编辑 prompt 文件自定义编排器行为，无需重新编译。

## Background

当前 `DEFAULT_SYSTEM_PROMPT`（engine.rs:40）是硬编码字符串常量，`build_system_prompt()`（engine.rs:72-135）用代码拼接动态 prompt。每次调优 prompt 都需改代码→编译→重启。

已有 `~/.config/zenix/roles/` 模板机制提供了可参考模式：文件存储 + 首运行 seed + 文件不存在 fallback 到编译期常量。

## Requirements

### R1: 提示词文件存储
- 目录：`~/.config/zenix/prompts/`
- 文件格式：markdown（无 YAML frontmatter 要求，纯 markdown 即可）
- 首文件：`orchestrator.md` — 编排器系统提示词
- 首运行 seed：如果 prompts 目录不存在或 orchestrator.md 不存在，写入内置版本

### R2: 加载逻辑
- `load_system_prompt() -> String`：读取 `orchestrator.md`，文件不存在时返回编译期 fallback 常量
- 编译期 fallback 常量保留作为兜底（同 roles template 模式）

### R3: 动态内容注入
- 当前 `build_system_prompt()` 在 prompt 尾部动态追加的内容（工具列表、角色列表、工作区信息）保留为代码拼接
- 模板文件只承载静态指令部分（persona、任务分发规则、工作流、约束）
- 运行时：模板内容 + 代码动态追加 = 最终 prompt

### R4: 文件修改即生效
- 每次发起聊天时重新读取 prompt 文件，不缓存
- 用户编辑 `orchestrator.md` 后下次对话立即生效（不需重启）

### R5: 扩展性
- 目录结构支持后续添加其他 prompt 文件（如 `assistant.md`）
- 方法签名为 `load_system_prompt(name: &str) -> String` 形式，name 默认 `"orchestrator"`

## Constraints

- 不破坏现有 roles template 机制
- 不引入新的第三方依赖
- 不使用 YAML frontmatter 解析（纯 markdown，无需 serde 反序列化）
- 首次 seed 写入的文件应包含注释说明，告知用户可自由编辑

## Acceptance Criteria

- [ ] AC1: `~/.config/zenix/prompts/orchestrator.md` 在首次运行时自动 seed
- [ ] AC2: 文件存在时，聊天使用文件内容（含代码动态追加部分）作为 system prompt
- [ ] AC3: 文件不存在时，使用编译期 fallback 常量，功能不受影响
- [ ] AC4: 编辑 `orchestrator.md` 后，下次发送消息即使用新内容
- [ ] AC5: `cargo check` 通过，无新增 warning
- [ ] AC6: 现有的动态内容（工具列表、角色列表、工作区）正常追加到模板内容后

## Out of Scope

- 管理 UI（设置面板中编辑 prompt）— 用户直接编辑文件即可
- 多 prompt 文件热切换（后续可加，当前只加载 orchestrator.md）
- YAML frontmatter 解析（不需要）

## Notes

- 参考 roles template 的 seed 模式：`seed_builtin_templates()` → 检查目录 + 文件存在 → 写入
- 与 roles 的区别：prompts 是纯 markdown 无 frontmatter，代码尾部追加动态内容
- file 内容有 `{lang_instruction}` 占位符，运行时替换为语言指令（"用中文回复" / "Reply in the same language as the user's input"）
