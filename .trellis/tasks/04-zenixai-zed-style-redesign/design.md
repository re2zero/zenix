# Design: zenixAI Zed 风格界面重构

## Zed UI 核心设计模式

### 1. 顶部导航栏 (Header)
- 左侧：Agent 身份标识（"ZenixAI ▾"）
- 右侧：搜索图标（🔍）和更多选项图标（⋯）

### 2. 输入区与占位符 (Input Area & Placeholder)
- 占位符文本格式：
  - 对于 Zed Agent："Message the Zed Agent, @ to include context, / for commands"
  - 对于其他 Agent："Message {agent_name} — @ to include context, / for commands"
- 占位符颜色：低对比度灰色 `gpui::hsla(0.0, 0.0, 0.44, 1.0)`（即 #6e6e6e）
- 输入框背景：`gpui::rgba(0xFFFFFF04)`
- 输入框边框：`gpui::rgba(0xFFFFFF08)`

### 3. 模型选择面板 (Model Selection Panel)
- 第一行文本："Select a model…"附带下拉箭头（▾）
- 下方按钮："Configure"
- 快捷键提示："Ctrl-Alt-C"

### 4. 底部操作栏 (Bottom Action Bar)
- 左侧："+"号图标按钮
- 中间：设置图标（⚙）+ 文本（"ZenixAI Agent"或"Agents (n)"）+ 下拉箭头（▾）
- 右侧：极简的发送/播放按钮（▶）或停止按钮（◼）

## 颜色系统
- 占位符文本颜色：`gpui::hsla(0.0, 0.0, 0.44, 1.0)` (#6e6e6e)
- 输入框背景色：`gpui::rgba(0xFFFFFF04)`
- 输入框边框色：`gpui::rgba(0xFFFFFF08)`
- 悬停背景色：`gpui::rgba(0xFFFFFF08)`

## GPUI 组件映射
- 输入框：`Input::new(entity).bg(...).border_1().border_color(...).rounded_md().p(px(12.))`
- 按钮：`Button::new(...).ghost().compact().w(px(28.)).h(px(28.))`
- 文本：`txt(px(13.)).text_color(...).child(...)`
