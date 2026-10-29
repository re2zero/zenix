//! The conversation surface, drawn twice: once with the component library's
//! own parts, once in the shape zenix is aiming at.
//!
//! ```bash
//! cargo run --example chat_gallery
//! ```
//!
//! Page 1 (`Page::Parts`) lays every `ely-chat` part zenix would use on one
//! scroll, each section captioned with what the part is and whether it covers
//! the need. Page 2 (`Page::Draft`) is the design draft: no role labels, a
//! user turn that does not fill the column, one-line step rows that fold,
//! whole markdown, a planning approval, and the model popup laid out beside
//! the composer.
//!
//! This is a gallery, not a shell surface: it opens no connection, holds no
//! session, and keeps no state beyond the page switch and the folds it draws.

use std::time::{Duration, Instant};

use ely_chat::{
    Attachment, AttachmentChip, CapabilityCards, ChatContainer, CitationBadge, CodeBlock,
    ContextChips, ErrorMessage, FollowUpSuggestions, MessageActions, MessageAvatar, MessageBubble,
    MessageFooter, MessageHeader, MessageList, PromptInput, RateLimitNotice, Role,
    ScrollToBottomButton, Source, SourceCard, StepState, StreamingCursor, StreamingMarkdown,
    StreamingText, SuggestionChips, ThinkingBlock, ThinkingDuration, ThinkingIndicator,
    WelcomeScreen, step_mark,
};
use ely_core::buttons::{Button, ButtonVariant, CopyButton, IconButton};
use ely_core::forms::{Choice, TextInput};
use ely_core::primitives::{Disclosure, Icon, IconName};
use ely_core::theme::{
    ActiveTheme, ControlSize, IconSize, Mode, Palette, Radius, TextSize, Theme as ChromeTheme,
};
use gpui::{
    AnyElement, App, Bounds, Context, ElementId, Entity, FontWeight, IntoElement, ParentElement,
    Rems, Render, SharedString, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    linear_color_stop, linear_gradient, point, prelude::*, px, size,
};

const MARKDOWN_SAMPLE: &str = r#"# 会话渲染规格

这一段验证**行内粗体**、*斜体*、~~删除线~~、`inline code` 与 [链接](https://example.invalid)。

## 列表

- 无序项一
- 无序项二
  - 嵌套项

1. 有序项一
2. 有序项二

- [x] 已完成的检查
- [ ] 未完成的检查

## 表格

| 块 | 组件 | 折叠 |
| --- | --- | :---: |
| 思考 | ThinkingBlock | 是 |
| 步骤 | 自建 StepRow | 是 |
| 正文 | StreamingMarkdown | 否 |

## 代码

```rust
fn grade(status: Status) -> StepState {
    match status {
        Status::Running => StepState::Working,
        Status::Done => StepState::Done,
        Status::Failed => StepState::Failed,
    }
}
```

> 引用：编排的每一步都应有一行可折叠的交代，而不是一整张卡。

## 公式与图

行内公式 $E = mc^2$，块公式：

$$
\int_0^1 x^2 dx = \frac{1}{3}
$$

```mermaid
graph LR
  A[用户] --> B[编排器]
```

---

分隔线之上是全部块类型。
"#;

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Parts,
    Draft,
}

/// Everything a section draws with, read off the theme once per frame. Owned
/// copies keep the draw code free to take `&mut App` for folds and marks.
struct Look {
    colors: Palette,
    xs: Rems,
    sm: Rems,
    base: Rems,
    r_sm: Rems,
    r_md: Rems,
    r_xl: Rems,
}

impl Look {
    fn of(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            colors: theme.colors.clone(),
            xs: theme.text_size(TextSize::Xs),
            sm: theme.text_size(TextSize::Sm),
            base: theme.text_size(TextSize::Base),
            r_sm: theme.radius(Radius::Sm),
            r_md: theme.radius(Radius::Md),
            r_xl: theme.radius(Radius::Xl),
        }
    }
}

/// One step of an assistant turn.
struct Step {
    key: &'static str,
    verb: &'static str,
    target: &'static str,
    meta: &'static str,
    state: StepState,
}

struct Gallery {
    page: Page,
    input: Entity<TextInput>,
}

impl Gallery {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line(2, 8)
                .placeholder("提出后续修改要求")
        });
        Self {
            // A headless run picks the page it needs to photograph:
            // `CHAT_GALLERY_PAGE=parts cargo run --example chat_gallery`.
            page: match std::env::var("CHAT_GALLERY_PAGE").as_deref() {
                Ok("parts") => Page::Parts,
                _ => Page::Draft,
            },
            input,
        }
    }

    fn switch(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    fn top_bar(&self, look: &Look, cx: &mut Context<Self>) -> AnyElement {
        let pick = |label: &str, active: bool| {
            let button = Button::new(
                ElementId::from(SharedString::from(label)),
                label.to_string(),
            )
            .size(ControlSize::Sm);
            if active {
                button.variant(ButtonVariant::Secondary)
            } else {
                button.variant(ButtonVariant::Ghost)
            }
        };
        let parts_button = pick("① 组件原件", self.page == Page::Parts).on_click(cx.listener(
            |this: &mut Self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                this.switch(Page::Parts, cx)
            },
        ));
        let draft_button = pick("② 会话样张", self.page == Page::Draft).on_click(cx.listener(
            |this: &mut Self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>| {
                this.switch(Page::Draft, cx)
            },
        ));
        div()
            .flex_none()
            .h(px(44.))
            .px_4()
            .flex()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(look.colors.border)
            .bg(look.colors.surface)
            .child(
                div()
                    .text_size(look.sm)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("zenix · 会话组件画廊"),
            )
            .child(
                div()
                    .text_size(look.xs)
                    .text_color(look.colors.fg_subtle)
                    .child("ely-chat @ 30c66fe · gpui-pre 0.3.10"),
            )
            .child(div().flex_1())
            .child(parts_button)
            .child(draft_button)
            .into_any_element()
    }

    // ---------------------------------------------------------------- parts

    fn parts(&mut self, window: &mut Window, cx: &mut Context<Self>, look: &Look) -> AnyElement {
        let colors = look.colors.clone();

        // The step marks borrow the window and the app; collect them before
        // anything else reads the theme.
        let marks: Vec<AnyElement> = [
            (StepState::Waiting, "等待"),
            (StepState::Working, "进行中"),
            (StepState::Done, "完成"),
            (StepState::Failed, "失败"),
        ]
        .into_iter()
        .enumerate()
        .map(|(ix, (state, _))| {
            step_mark(
                ElementId::from(SharedString::from(format!("mark-{ix}"))),
                state,
                window,
                cx,
            )
        })
        .collect();

        let mut column = div()
            .w_full()
            .max_w(px(980.))
            .mx_auto()
            .flex()
            .flex_col()
            .gap_4()
            .py_6()
            .px_6();

        let bubbles = {
            let mut list = div().flex().flex_col().gap_3();
            let roles = [
                (
                    Role::User,
                    "公子",
                    "22:14",
                    "先看看 ely 的会话组件，再按 zcode 那种紧凑折叠的样子设计。",
                ),
                (
                    Role::Assistant,
                    "编排器",
                    "22:14",
                    "四个角色的气泡都在这里；设计稿里头像、名字、时间都会撤掉，见第 2 页。",
                ),
                (
                    Role::System,
                    "系统",
                    "22:15",
                    "上下文压缩：已丢弃 4 条工具结果。",
                ),
                (Role::Error, "错误", "22:16", "连接中断：daemon 未响应。"),
            ];
            for (ix, (role, name, at, text)) in roles.into_iter().enumerate() {
                let body = div()
                    .text_size(look.sm)
                    .text_color(colors.fg)
                    .child(text.to_string())
                    .into_any_element();
                let footer = MessageFooter::new().fact(format!("{} tok", 120 * (ix + 1)));
                list = list.child(
                    MessageBubble::new(role)
                        .avatar(MessageAvatar::new(
                            ElementId::from(SharedString::from(format!("bubble-avatar-{ix}"))),
                            role,
                            name,
                        ))
                        .header(MessageHeader::new(
                            ElementId::from(SharedString::from(format!("bubble-head-{ix}"))),
                            format!("{name} · {at}"),
                        ))
                        .footer(footer)
                        .child(body),
                );
            }
            list.into_any_element()
        };
        column = column.child(section(
            look,
            "消息气泡 · MessageBubble / MessageAvatar / MessageHeader / MessageFooter",
            "四角色齐备（用户、助手、系统、错误）。头像、名字与时间是库画的；zenix 不需要这层壳，只留 body。",
            bubbles,
        ));

        column = column.child(section(
            look,
            "Markdown 全量 · StreamingMarkdown",
            "标题、粗斜体、删除线、行内码、链接、有序/无序/嵌套列表、任务列表、表格、代码围栏、引用、分隔线。这张卡本身就是覆盖度实测。",
            div()
                .w_full()
                .text_size(look.sm)
                .text_color(colors.fg)
                .child(StreamingMarkdown::new(
                    ElementId::from(SharedString::from("md-sample")),
                    MARKDOWN_SAMPLE,
                    false,
                )),
        ));

        column = column.child(section(
            look,
            "流式 · StreamingText / StreamingMarkdown / StreamingCursor",
            "上面是纯文本流（带光标），下面是 markdown 流；live=true 时光标与揭示才动。",
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(look.sm)
                        .text_color(colors.fg_muted)
                        .child(StreamingText::new(
                            ElementId::from(SharedString::from("stream-text")),
                            "正在把第二段写进来",
                            true,
                        ))
                        .child(StreamingCursor::new(ElementId::from(SharedString::from(
                            "stream-cursor",
                        )))),
                )
                .child(div().text_size(look.sm).text_color(colors.fg).child(
                    StreamingMarkdown::new(
                        ElementId::from(SharedString::from("stream-md")),
                        "### 边写边读\n\n- 第一行\n- 第二行还在路上",
                        true,
                    ),
                )),
        ));

        column = column.child(section(
            look,
            "推理 · ThinkingIndicator / ThinkingDuration / ThinkingBlock",
            "形态与设计稿一致（一行标题 + chevron + 左侧发丝线正文），但标题文案写死在库里：\"Thought for 12 seconds\"。中文界面要自绘标题，只借排版。点开看正文。",
            div()
                .flex()
                .flex_col()
                .gap_3()
                .text_size(look.sm)
                .child(ThinkingIndicator::new(ElementId::from(SharedString::from(
                    "thinking-live",
                ))))
                .child(ThinkingDuration::new(Duration::from_secs(12)))
                .child(ThinkingBlock::new(
                    ElementId::from(SharedString::from("thinking-done")),
                    "先确认 ely 的会话组件覆盖到哪一层：气泡、流式、推理、动作都有，工具与编排没有。",
                    false,
                    Duration::from_secs(12),
                ))
                .child(ThinkingBlock::new(
                    ElementId::from(SharedString::from("thinking-now")),
                    "正在比较 BitFun 的 transcript 模型与 zenix 的消息结构。",
                    true,
                    Duration::ZERO,
                )),
        ));

        column = column.child(section(
            look,
            "步骤标记 · step_mark / StepState",
            "库只给了四种状态的小标记。步骤行、组行、工具卡都没有 —— 编排面要自建。",
            {
                let labels = ["等待", "进行中", "完成", "失败"];
                let mut row = div().flex().items_center().gap_6().text_size(look.sm);
                for (mark, label) in marks.into_iter().zip(labels) {
                    row = row.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(mark)
                            .child(div().text_color(colors.fg_muted).child(label)),
                    );
                }
                row.into_any_element()
            },
        ));

        column = column.child(section(
            look,
            "消息动作 · MessageActions / CopyButton",
            "复制、编辑、重试、赞、踩都有。tooltip 写死英文（\"Copy\" / \"Good answer\" / \"Regenerate\"），zenix 的自定义按钮已经绕开它。",
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(MessageActions::new(
                    ElementId::from(SharedString::from("actions")),
                    "答案正文",
                ))
                .child(CopyButton::new(
                    ElementId::from(SharedString::from("copy")),
                    "复制内容",
                ))
                .child(
                    IconButton::new(
                        ElementId::from(SharedString::from("thumb")),
                        IconName::Sparkles,
                    )
                    .variant(ButtonVariant::Ghost)
                    .size(ControlSize::Sm)
                    .tooltip("赞成（自绘即可）"),
                ),
        ));

        column = column.child(section(
            look,
            "错误与限流 · ErrorMessage / RateLimitNotice",
            "错误卡带 detail 与重试；限流卡自带倒计时。zenix 现有错误卡是自绘的（本地化文案），结构与文案都不可直接照搬。",
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    ErrorMessage::new(ElementId::from(SharedString::from("error")), "连接中断")
                        .detail("daemon socket 拒绝连接")
                        .on_retry(|_, _| {}),
                )
                .child(RateLimitNotice::new(
                    ElementId::from(SharedString::from("rate")),
                    Instant::now() + Duration::from_secs(45),
                )),
        ));

        column = column.child(section(
            look,
            "欢迎态 · WelcomeScreen / CapabilityCards / SuggestionChips / FollowUpSuggestions",
            "空面板的四件套：问候、能力卡、起始提示、追问建议。zenix 现在只用了 SuggestionChips。",
            div().w_full().flex().flex_col().gap_4().child(
                WelcomeScreen::new(ElementId::from(SharedString::from("welcome")), 9)
                    .name("公子")
                    .child(
                        div()
                            .text_size(look.base)
                            .text_color(colors.fg_muted)
                            .child("今天要做什么？"),
                    )
                    .child(CapabilityCards::new(
                        ElementId::from(SharedString::from("caps")),
                        [
                            (IconName::Terminal, "编排", "把一句话拆成多 agent 的任务"),
                            (IconName::Search, "检索", "在工作区里找文件与符号"),
                            (
                                IconName::LayoutGrid,
                                "布局",
                                "读取并应用 .zenix/layout.toml",
                            ),
                        ],
                        |_, _, _| {},
                    ))
                    .child(SuggestionChips::new(
                        ElementId::from(SharedString::from("chips")),
                        [
                            ("开一个新工作区", IconName::Plus),
                            ("看看当前布局", IconName::LayoutDashboard),
                        ],
                        |_, _, _| {},
                    ))
                    .child(FollowUpSuggestions::new(
                        ElementId::from(SharedString::from("follow")),
                        ["把 layout 导出成文件", "列一下当前的 agent"],
                        |_, _, _| {},
                    )),
            ),
        ));

        column = column.child(section(
            look,
            "引用与来源 · SourceCard / SourceList / CitationBadge",
            "来源卡与行内角标；编排回答的附录可以复用。",
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    SourceCard::new(
                        ElementId::from(SharedString::from("source-card")),
                        Source {
                            site: "Ely".into(),
                            title: "Chat components".into(),
                            url: "https://example.invalid/ely/chat".into(),
                            snippet: Some("MessageList, PromptInput, ThinkingBlock…".into()),
                        },
                    )
                    .number(1),
                )
                .child(CitationBadge::new(
                    ElementId::from(SharedString::from("source-badge")),
                    2,
                    Source {
                        site: "herdr".into(),
                        title: "Endpoint protocol".into(),
                        url: "https://example.invalid/herdr/wire".into(),
                        snippet: None,
                    },
                )),
        ));

        column = column.child(section(
            look,
            "附件与上下文 · AttachmentChip / ContextChips",
            "附件胶囊带进度与移除；上下文胶囊是 `@` 提及的外形。zenix 还没有附件后端，这两个先当形态参考。",
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(AttachmentChip::new(
                            ElementId::from(SharedString::from("att-1")),
                            Attachment {
                                key: "a".into(),
                                name: "zenix.yml".into(),
                                bytes: 12_400,
                                preview: None,
                                progress: None,
                            },
                        ))
                        .child(AttachmentChip::new(
                            ElementId::from(SharedString::from("att-2")),
                            Attachment {
                                key: "b".into(),
                                name: "herdr-server.log".into(),
                                bytes: 240_000,
                                preview: None,
                                progress: Some(0.42),
                            },
                        )),
                )
                .child(ContextChips::new(
                    ElementId::from(SharedString::from("ctx")),
                    [
                        ("locales/zenix.yml", IconName::FileText),
                        ("src/ui/chat.rs", IconName::FileCode),
                    ],
                )),
        ));

        column = column.child(section(
            look,
            "代码块 · CodeBlock",
            "语言标签、复制、可选运行/应用。zenix 的折叠体没有用它：它把标题印成 \"code\"、tooltip 印英文，所以现有实现自绘了等宽块。",
            CodeBlock::new(
                ElementId::from(SharedString::from("code-block")),
                "fn main() {\n    println!(\"hello from ely\");\n}",
            )
            .language("rust"),
        ));

        column = column.child(section(
            look,
            "输入卡 · PromptInput（above / tool / commands）",
            "圆角卡 + 上下文行 + 工具行 + `/` 命令面板 + 随文就绪变色的发送键。zenix 已全量复用；设计稿要改的只是工具行里放什么。",
            PromptInput::new(
                ElementId::from(SharedString::from("composer")),
                &self.input,
                |_, _| {},
            )
            .above(
                div()
                    .text_size(look.xs)
                    .text_color(colors.fg_muted)
                    .child("编排器 · deepseek-v4.1-flash · 3 轮 · 1.2k tok"),
            )
            .tool(
                Button::new(
                    ElementId::from(SharedString::from("mode")),
                    "完全访问".to_string(),
                )
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm),
            )
            .tool(
                Button::new(
                    ElementId::from(SharedString::from("model")),
                    "deepseek-v4.1-flash · 最高".to_string(),
                )
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm),
            )
            .commands(
                vec![
                    Choice::new("new", "新对话".to_string()),
                    Choice::new("clear", "清空".to_string()),
                ],
                |_, _, _| {},
            ),
        ));

        column = column.child(section(
            look,
            "列表与滚动 · MessageList / ChatContainer / ScrollToBottomButton",
            "MessageList 只渲染可见行，ChatContainer 管列宽与底栏，ScrollToBottomButton 报未读条数。zenix 已用前两个，未读数在业务层。",
            div()
                .w_full()
                .h(px(240.))
                .rounded(look.r_md)
                .border_1()
                .border_color(colors.border)
                .overflow_hidden()
                .relative()
                .child(
                    ChatContainer::new(MessageList::new(
                        ElementId::from(SharedString::from("mlist")),
                        12,
                        move |ix, _window, cx| {
                            let theme = cx.theme();
                            let visible = ix % 2 == 0;
                            let ink = if visible {
                                theme.colors.fg
                            } else {
                                theme.colors.fg_muted
                            };
                            div()
                                .w_full()
                                .px_3()
                                .py_2()
                                .text_size(theme.text_size(TextSize::Sm))
                                .text_color(ink)
                                .child(format!("第 {} 行：MessageList 只画可见的行", ix + 1))
                                .into_any_element()
                        },
                    ))
                    .composer(
                        div()
                            .px_3()
                            .py_2()
                            .border_t_1()
                            .border_color(colors.border)
                            .text_size(look.xs)
                            .text_color(colors.fg_subtle)
                            .child("ChatContainer 的 composer 槽"),
                    ),
                )
                .child(div().absolute().right(px(12.)).bottom(px(12.)).child(
                    ScrollToBottomButton::new(
                        ElementId::from(SharedString::from("scroll-bottom")),
                        2,
                        |_, _| {},
                    ),
                )),
        ));

        column = column.child(
            div()
                .text_size(look.xs)
                .text_color(colors.fg_subtle)
                .child("第 1 页完 · 按顶部「② 会话样张」看设计稿"),
        );

        div()
            .id("parts-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(column)
            .into_any_element()
    }

    // ---------------------------------------------------------------- draft

    fn draft(&mut self, window: &mut Window, cx: &mut Context<Self>, look: &Look) -> AnyElement {
        let colors = look.colors.clone();

        let mut transcript = div().w_full().flex().flex_col().gap_6().py_6();

        transcript = transcript.child(self.user_turn(
            look,
            "draft-u1",
            "先写一个 demo 看看 ely 的会话组件，再按 zcode 那种简洁、能折叠的样子设计。会话不分「你」和「助手」。",
        ));

        // The first assistant turn: reasoning, two reads, a shell group, the
        // answer, and the turn's own line.
        let mut turn = div().w_full().flex().flex_col().gap_3();
        turn = turn.child(fold_row(
            look,
            "draft-think-1",
            "思考",
            "12 秒",
            "先看 ely 的会话组件覆盖到哪一层，再决定哪些自建。工具行与分组库里没有。",
            window,
            cx,
        ));
        turn = turn.child(step_row(
            look,
            Step {
                key: "draft-step-1",
                verb: "读取",
                target: "ely-chat/src/lib.rs",
                meta: "导出清单",
                state: StepState::Done,
            },
            window,
            cx,
        ));
        turn = turn.child(step_row(
            look,
            Step {
                key: "draft-step-2",
                verb: "读取",
                target: "src/ui/chat.rs",
                meta: "2 541 行",
                state: StepState::Done,
            },
            window,
            cx,
        ));
        turn = turn.child(group_row(
            look,
            "draft-group-1",
            "终端",
            "2 个命令",
            vec![
                ("cargo run --example chat_gallery", "编译通过 · 41s"),
                ("cargo test --lib", "536 passed"),
            ],
            window,
            cx,
        ));
        turn = turn.child(
            div()
                .w_full()
                .text_size(look.sm)
                .text_color(colors.fg)
                .child(StreamingMarkdown::new(
                    ElementId::from(SharedString::from("draft-md-1")),
                    "### 结论\n\n三处都改好了：字体卡片已删，卡片标题统一继承 `fg`，滚动内容右侧留出 12px 给滚动条。\n\n| 块 | 来源 | 折叠 |\n| --- | --- | :---: |\n| 思考 | `ThinkingBlock` 排版 | 是 |\n| 步骤 | 自建单行 | 是 |\n| 组 | 自建 chip | 是 |\n| 正文 | `StreamingMarkdown` | 否 |\n\n> 每一步一行交代，比一张卡安静得多。",
                    false,
                )),
        );
        turn = turn.child(tail_row(look, "draft-tail-1", "已工作 20 秒", "22:14"));
        transcript = transcript.child(turn);

        transcript =
            transcript.child(self.user_turn(look, "draft-u2", "批准后继续，但别动 vendor。"));

        // The second turn: an edit step, the planning approval, an error row.
        let mut turn2 = div().w_full().flex().flex_col().gap_3();
        turn2 = turn2.child(step_row(
            look,
            Step {
                key: "draft-step-3",
                verb: "编辑",
                target: "locales/zenix.yml",
                meta: "−12",
                state: StepState::Done,
            },
            window,
            cx,
        ));
        turn2 = turn2.child(approval_card(look));
        turn2 = turn2.child(
            div()
                .w_full()
                .text_size(look.sm)
                .text_color(colors.fg)
                .child(StreamingMarkdown::new(
                    ElementId::from(SharedString::from("draft-md-2")),
                    "已按清单执行，`vendor/` 未触碰。下面这条是失败态的读法：\n\n```text\ngit status --short\n M locales/zenix.yml\n```",
                    false,
                )),
        );
        turn2 = turn2.child(error_row(look));
        transcript = transcript.child(turn2);

        let composer = self.draft_composer(look);

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(session_head(look))
                    .child(
                        div()
                            .id("draft-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(div().max_w(px(860.)).mx_auto().px_8().child(transcript)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_8()
                            .pb_5()
                            .pt_2()
                            .child(div().max_w(px(860.)).mx_auto().child(composer)),
                    ),
            )
            .child(side_column(look))
            .into_any_element()
    }

    /// A user turn: no label, narrower than the column, and set to the right so
    /// the model's own words keep the full measure.
    fn user_turn(&self, look: &Look, key: &'static str, text: &'static str) -> AnyElement {
        div()
            .w_full()
            .flex()
            .justify_end()
            .child(
                div()
                    .id(ElementId::from(SharedString::from(key)))
                    .max_w(px(520.))
                    .px_4()
                    .py_3()
                    .rounded(look.r_md)
                    .border_1()
                    .border_color(look.colors.border)
                    .bg(look.colors.surface)
                    .text_size(look.sm)
                    .text_color(look.colors.fg)
                    .child(text),
            )
            .into_any_element()
    }

    /// The composer in the shape zcode has: one card, the field, and a row of
    /// `+ / permission / model+level / send`. `PromptInput` draws its own send
    /// button and puts every tool left of it, so the draft draws the card here
    /// and keeps the real field inside it.
    fn draft_composer(&self, look: &Look) -> AnyElement {
        let colors = look.colors.clone();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(look.r_xl)
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.surface)
            .child(self.input.clone())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        IconButton::new(
                            ElementId::from(SharedString::from("draft-attach")),
                            IconName::Plus,
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .tooltip("附件（尚未接后端）"),
                    )
                    .child(
                        Button::new(
                            ElementId::from(SharedString::from("draft-mode")),
                            "完全访问".to_string(),
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .icon(IconName::Shield),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new(
                            ElementId::from(SharedString::from("draft-model")),
                            "deepseek-v4.1-flash · 最高".to_string(),
                        )
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .icon(IconName::CircleDot),
                    )
                    .child(
                        div()
                            .flex_none()
                            .size(px(30.))
                            .rounded_full()
                            .bg(colors.accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(IconName::ArrowUp)
                                    .size(IconSize::Sm)
                                    .color(colors.on_accent),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = Look::of(cx);
        let page = match self.page {
            Page::Parts => self.parts(window, cx, &look),
            Page::Draft => self.draft(window, cx, &look),
        };
        let font = cx.theme().font_family.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family(font)
            .bg(look.colors.bg)
            .text_color(look.colors.fg)
            .child(self.top_bar(&look, cx))
            .child(page)
    }
}

// ------------------------------------------------------------------ shapes

fn section(look: &Look, title: &str, note: &str, body: impl IntoElement) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded(look.r_md)
        .border_1()
        .border_color(look.colors.border)
        .bg(look.colors.surface)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(look.sm)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_size(look.xs)
                        .text_color(look.colors.fg_muted)
                        .child(note.to_string()),
                ),
        )
        .child(body)
        .into_any_element()
}

/// One step of a turn: a status mark, a verb, its target, and a quiet piece of
/// meta. Flat by design — no card, no border, a hover fill only.
fn step_row(look: &Look, step: Step, window: &mut Window, cx: &mut App) -> AnyElement {
    let colors = look.colors.clone();
    let id = ElementId::from(SharedString::from(step.key));
    let mark = step_mark((id.clone(), "mark").into(), step.state, window, cx);
    div()
        .id(id)
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .py_1()
        .px_2()
        .rounded(look.r_sm)
        .hover(|style| style.bg(colors.hover))
        .child(mark)
        .child(
            div()
                .flex_none()
                .text_size(look.sm)
                .text_color(colors.fg_muted)
                .child(step.verb),
        )
        .child(
            div()
                .min_w_0()
                .text_size(look.sm)
                .text_color(colors.fg)
                .child(step.target),
        )
        .child(div().flex_1())
        .child(
            div()
                .flex_none()
                .text_size(look.xs)
                .text_color(colors.fg_subtle)
                .child(step.meta.to_string()),
        )
        .into_any_element()
}

/// A reasoning step: one line with its length, and the body it opens. The
/// shape is `ThinkingBlock`'s without the library's English header.
fn fold_row(
    look: &Look,
    key: &'static str,
    label: &'static str,
    meta: &'static str,
    body: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let colors = look.colors.clone();
    let id = ElementId::from(SharedString::from(key));
    let state = window.use_keyed_state((id.clone(), "open"), cx, |_, _| false);
    let open = *state.read(cx);
    let toggle = {
        let state = state.clone();
        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
            state.update(cx, |open, cx| {
                *open = !*open;
                cx.notify();
            });
        }
    };
    let mut block = div().w_full().flex().flex_col().gap_1().child(
        div()
            .id(id.clone())
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(look.r_sm)
            .cursor_pointer()
            .hover(|style| style.bg(colors.hover))
            .on_click(toggle)
            .child(
                Icon::new(IconName::Sparkles)
                    .size(IconSize::Sm)
                    .color(colors.fg_subtle),
            )
            .child(
                div()
                    .text_size(look.sm)
                    .text_color(colors.fg_muted)
                    .child(label),
            )
            .child(
                div()
                    .text_size(look.xs)
                    .text_color(colors.fg_subtle)
                    .child(format!("· {meta}")),
            )
            .child(div().flex_1())
            .child(Disclosure::new((id.clone(), "chevron"), open).size(IconSize::Sm)),
    );
    if open {
        block = block.child(
            div()
                .pl_4()
                .ml_3()
                .border_l_1()
                .border_color(colors.border)
                .py_1()
                .text_size(look.xs)
                .text_color(colors.fg_muted)
                .child(body.to_string()),
        );
    }
    block.into_any_element()
}

/// A run of same-family steps behind one line: `终端 · 2 个命令`, and the
/// commands it covers when opened.
fn group_row(
    look: &Look,
    key: &'static str,
    label: &'static str,
    count: &'static str,
    lines: Vec<(&'static str, &'static str)>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let colors = look.colors.clone();
    let id = ElementId::from(SharedString::from(key));
    let state = window.use_keyed_state((id.clone(), "open"), cx, |_, _| false);
    let open = *state.read(cx);
    let toggle = {
        let state = state.clone();
        move |_: &gpui::ClickEvent, _window: &mut Window, cx: &mut App| {
            state.update(cx, |open, cx| {
                *open = !*open;
                cx.notify();
            });
        }
    };
    let mut block = div().w_full().flex().flex_col().gap_1().child(
        div()
            .id(id.clone())
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(look.r_sm)
            .cursor_pointer()
            .hover(|style| style.bg(colors.hover))
            .on_click(toggle)
            .child(
                Icon::new(IconName::Terminal)
                    .size(IconSize::Sm)
                    .color(colors.fg_subtle),
            )
            .child(
                div()
                    .text_size(look.sm)
                    .text_color(colors.fg_muted)
                    .child(label),
            )
            .child(
                div()
                    .text_size(look.xs)
                    .text_color(colors.fg_subtle)
                    .child(format!("· {count}")),
            )
            .child(div().flex_1())
            .child(Disclosure::new((id.clone(), "chevron"), open).size(IconSize::Sm)),
    );
    if open {
        let mut body = div()
            .flex()
            .flex_col()
            .gap_1()
            .pl_4()
            .ml_3()
            .border_l_1()
            .border_color(colors.border)
            .py_1();
        for (command, result) in lines {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(look.xs)
                            .text_color(colors.fg)
                            .child(format!("$ {command}")),
                    )
                    .child(
                        div()
                            .text_size(look.xs)
                            .text_color(colors.fg_subtle)
                            .child(result),
                    ),
            );
        }
        block = block.child(body);
    }
    block.into_any_element()
}

/// What the user answers before a step that writes: the command, what it
/// touches, and the three answers. `Root`'s own card has this exact shape.
fn approval_card(look: &Look) -> AnyElement {
    let colors = look.colors.clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .rounded(look.r_md)
        .border_1()
        .border_color(colors.border_strong)
        .bg(colors.surface)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(IconSize::Sm)
                        .color(colors.warning),
                )
                .child(
                    div()
                        .text_size(look.sm)
                        .font_weight(FontWeight::MEDIUM)
                        .child("需要批准"),
                )
                .child(
                    div()
                        .text_size(look.xs)
                        .text_color(colors.fg_subtle)
                        .child("提交会改动工作区以外的文件"),
                ),
        )
        .child(
            div()
                .p_2()
                .rounded(look.r_sm)
                .bg(colors.sunken)
                .text_size(look.xs)
                .text_color(colors.fg)
                .child("git commit -am \"feat(ui): rebuild the shell\""),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("approve-deny", "拒绝".to_string())
                        .variant(ButtonVariant::Ghost)
                        .size(ControlSize::Sm),
                )
                .child(
                    Button::new("approve-always", "总是允许".to_string())
                        .variant(ButtonVariant::Secondary)
                        .size(ControlSize::Sm),
                )
                .child(
                    Button::new("approve-allow", "允许".to_string())
                        .variant(ButtonVariant::Primary)
                        .size(ControlSize::Sm),
                ),
        )
        .into_any_element()
}

fn error_row(look: &Look) -> AnyElement {
    let colors = look.colors.clone();
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .p_3()
        .rounded(look.r_md)
        .bg(colors.danger_subtle)
        .child(
            Icon::new(IconName::TriangleAlert)
                .size(IconSize::Sm)
                .color(colors.danger),
        )
        .child(
            div()
                .text_size(look.sm)
                .text_color(colors.danger)
                .child("这一轮已中断：daemon 拒绝连接"),
        )
        .child(div().flex_1())
        .child(
            Button::new("error-retry", "重试".to_string())
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm),
        )
        .into_any_element()
}

/// The turn's own line: how long it worked, and what can be done with it.
fn tail_row(look: &Look, key: &'static str, took: &'static str, at: &'static str) -> AnyElement {
    let colors = look.colors.clone();
    let id = ElementId::from(SharedString::from(key));
    div()
        .id(id.clone())
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .py_1()
        .child(
            div()
                .text_size(look.xs)
                .text_color(colors.fg_subtle)
                .child(took.to_string()),
        )
        .child(Disclosure::new((id.clone(), "chevron"), false).size(IconSize::Sm))
        .child(div().flex_1())
        .child(
            IconButton::new((id.clone(), "copy"), IconName::Copy)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip("复制"),
        )
        .child(
            IconButton::new((id.clone(), "good"), IconName::ThumbsUp)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip("赞"),
        )
        .child(
            IconButton::new((id.clone(), "bad"), IconName::ThumbsDown)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip("踩"),
        )
        .child(
            IconButton::new((id.clone(), "retry"), IconName::RefreshCw)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip("重试"),
        )
        .child(
            IconButton::new((id.clone(), "more"), IconName::Ellipsis)
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .tooltip("更多"),
        )
        .child(
            div()
                .text_size(look.xs)
                .text_color(colors.fg_subtle)
                .child(at),
        )
        .into_any_element()
}

fn session_head(look: &Look) -> AnyElement {
    let colors = look.colors.clone();
    div()
        .flex_none()
        .h(px(44.))
        .px_8()
        .flex()
        .items_center()
        .gap_2()
        .border_b_1()
        .border_color(colors.border)
        .child(
            div()
                .text_size(look.sm)
                .font_weight(FontWeight::MEDIUM)
                .child("新会话"),
        )
        .child(div().size(px(6.)).rounded_full().bg(colors.success))
        .child(
            div()
                .text_size(look.xs)
                .text_color(colors.fg_subtle)
                .child("编排中 · 3 个 agent"),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_size(look.xs)
                .text_color(colors.fg_subtle)
                .child("22:14"),
        )
        .into_any_element()
}

/// The right-hand column of the draft: the block legend, then the model popup
/// exactly as it should open above the composer.
fn side_column(look: &Look) -> AnyElement {
    let colors = look.colors.clone();
    let legend = [
        ("用户", "窄条、右对齐、无标签"),
        ("思考行", "一行 + chevron，正文缩进"),
        ("步骤行", "标记 + 动词 + 目标 + meta"),
        ("组行", "同类步骤合成一行"),
        ("正文", "全宽 markdown，无气泡"),
        ("审批", "边框加重，三选一"),
        ("错误", "整行染色 + 重试"),
        ("尾行", "耗时 + 动作 + 时间"),
    ];
    let mut rows = div().flex().flex_col().gap_2();
    for (name, note) in legend {
        rows = rows.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .w(px(56.))
                        .flex_none()
                        .text_size(look.xs)
                        .text_color(colors.fg)
                        .child(name),
                )
                .child(
                    div()
                        .text_size(look.xs)
                        .text_color(colors.fg_subtle)
                        .child(note),
                ),
        );
    }

    div()
        .w(px(360.))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .gap_5()
        .p_5()
        .border_l_1()
        .border_color(colors.border)
        .bg(colors.bg)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_size(look.sm)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("块类型"),
                )
                .child(rows),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(look.sm)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("模型浮层 · 展开态"),
                        )
                        .child(
                            div()
                                .text_size(look.xs)
                                .text_color(colors.fg_subtle)
                                .child("点模型 chip 向上弹出；Ctrl+T 直接改思考强度"),
                        ),
                )
                .child(model_popup(look)),
        )
        .into_any_element()
}

fn model_popup(look: &Look) -> AnyElement {
    let colors = look.colors.clone();
    let models = [
        ("deepseek-v4.1-flash", Some("视觉"), true),
        ("qwen3-coder-plus", None, false),
        ("glm-4.6", Some("视觉"), false),
    ];
    let mut list = div().flex().flex_col().gap_1();
    for (name, badge, chosen) in models {
        let mut row = div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .rounded(look.r_sm)
            .when(chosen, |row| row.bg(colors.hover))
            .child(div().text_size(look.sm).text_color(colors.fg).child(name));
        if let Some(badge) = badge {
            row = row.child(
                div()
                    .px_1p5()
                    .py_0p5()
                    .rounded(look.r_sm)
                    .bg(colors.sunken)
                    .text_size(look.xs)
                    .text_color(colors.fg_muted)
                    .child(badge),
            );
        }
        row = row.child(div().flex_1());
        if chosen {
            row = row.child(
                Icon::new(IconName::Check)
                    .size(IconSize::Sm)
                    .color(colors.accent),
            );
        }
        list = list.child(row);
    }

    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .rounded(look.r_xl)
        .border_1()
        .border_color(colors.border_strong)
        .bg(colors.overlay)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .size(px(20.))
                        .rounded(look.r_sm)
                        .bg(colors.accent)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Icon::new(IconName::Bot)
                                .size(IconSize::Xs)
                                .color(colors.on_accent),
                        ),
                )
                .child(div().text_size(look.sm).text_color(colors.fg).child("oc"))
                .child(
                    div()
                        .px_1p5()
                        .py_0p5()
                        .rounded(look.r_sm)
                        .bg(colors.sunken)
                        .text_size(look.xs)
                        .text_color(colors.fg_muted)
                        .child("API"),
                )
                .child(div().flex_1())
                .child(
                    Icon::new(IconName::ChevronRight)
                        .size(IconSize::Sm)
                        .color(colors.fg_subtle),
                ),
        )
        .child(
            div()
                .text_size(look.xs)
                .text_color(colors.fg_subtle)
                .child("模型"),
        )
        .child(list)
        .child(div().h(px(1.)).w_full().bg(colors.border))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(look.sm)
                        .text_color(colors.fg_muted)
                        .child("思考强度"),
                )
                .child(div().text_size(look.sm).text_color(colors.fg).child("最高"))
                .child(div().flex_1())
                .child(
                    div()
                        .px_1p5()
                        .py_0p5()
                        .rounded(look.r_sm)
                        .border_1()
                        .border_color(colors.border)
                        .text_size(look.xs)
                        .text_color(colors.fg_subtle)
                        .child("Ctrl+T"),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().h(px(6.)).rounded_full().bg(linear_gradient(
                    90.,
                    linear_color_stop(colors.accent, 0.),
                    linear_color_stop(colors.info, 1.),
                )))
                .child(
                    div()
                        .size(px(14.))
                        .rounded_full()
                        .bg(colors.fg)
                        .border_2()
                        .border_color(colors.overlay),
                ),
        )
        .child(
            Button::new(
                ElementId::from(SharedString::from("popup-pick")),
                "选择模型".to_string(),
            )
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Sm),
        )
        .into_any_element()
}

fn main() {
    gpui_platform::application()
        .with_assets(ely_core::Assets)
        .with_quit_mode(gpui::QuitMode::LastWindowClosed)
        .run(|cx| {
            ely_core::init(cx).expect("the component library could not start");
            // The library's own strings (tooltips, the palette's copy) follow
            // the reader's locale; `zh-CN` is the tag zenix maps its config to.
            cx.set_global(ely_core::i18n::I18n::new("zh-CN"));
            ChromeTheme::set_palette(Mode::Dark, Some(Palette::dark(false)), cx);
            ChromeTheme::set_palette(Mode::Light, Some(Palette::light(false)), cx);
            ChromeTheme::set_mode_now(Mode::Dark, cx);
            let bounds = WindowBounds::Windowed(Bounds {
                origin: point(px(80.), px(60.)),
                size: size(px(1280.), px(900.)),
            });
            let options = WindowOptions {
                window_bounds: Some(bounds),
                titlebar: Some(TitlebarOptions {
                    title: Some("zenix · 会话组件画廊".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            cx.open_window(options, |window, cx| cx.new(|cx| Gallery::new(window, cx)))
                .expect("could not open the gallery window");
        });
}
