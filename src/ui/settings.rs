//! Resizable settings dialog with vertical tab navigation.

use std::collections::HashMap;

use gpui::{
    div, px, AnyElement, ClickEvent, InteractiveElement as _, IntoElement,
    ParentElement, StatefulInteractiveElement as _, Styled, Window, prelude::FluentBuilder as _,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex, scroll::ScrollableElement as _, v_flex,
};
use crate::agent::{AgentCliInfo, AgentCliStatus};
use crate::i18n::t;

use super::ThemeCallback;

/// Actions that the Skills tab can request from the app.
#[derive(Clone)]
pub enum SkillAction {
    Refresh,
    Link { skill_name: String },
    Unlink { skill_name: String },
    InstallGit { url: String },
    InstallLocal { path: String },
    Remove { skill_name: String },
}

const TAB_PANEL_W: f32 = 140.0;
const TAB_MIN_W: f32 = 480.0;
const TAB_MIN_H: f32 = 360.0;

// ── Agents that support MCP (Claude-format JSON configs) ───────────
const MCP_AGENTS: &[&str] = &["claude", "opencode", "pi", "omp", "kilo"];
// ── Agents that support skills (directory with SKILL.md) ───────────
pub const SKILL_AGENTS: &[&str] = &["claude", "codex", "opencode", "pi", "omp", "kilo", "hermes"];

pub fn settings_dialog(
    active_tab: usize,
    on_tab_change: Box<dyn Fn(usize, &mut Window, &mut gpui::App)>,
    on_close: Box<dyn Fn(&mut Window, &mut gpui::App)>,
    font_size: f32,
    current_theme_name: &str,
    theme_buttons: Vec<AnyElement>,
    on_font_down: ThemeCallback,
    on_font_up: ThemeCallback,
    locale: &str,
    on_locale_change: Box<dyn Fn(String, &mut Window, &mut gpui::App)>,
    agents: &[AgentCliInfo],
    mcp_servers: &[crate::mcp::UnifiedServer],
    skills: &HashMap<String, crate::skills::SkillInfo>,
    on_skill_action: Box<dyn Fn(SkillAction, &mut Window, &mut gpui::App)>,
    win_w: f32,
    win_h: f32,
) -> impl IntoElement {
    let dialog_w = (win_w * 0.75).max(TAB_MIN_W);
    let dialog_h = (win_h * 0.75).max(TAB_MIN_H);
    let active_tab_c = active_tab;
    let otc = std::rc::Rc::new(on_tab_change);
    let osa = std::rc::Rc::new(on_skill_action);

    let tab_labels = [
        t("settings.tab.general"), t("settings.tab.appearance"),
        t("settings.tab.terminal"), t("settings.tab.agent_cli"),
        t("settings.tab.mcp"), t("settings.tab.skills"),
    ];

    div()
        .id("settings-backdrop")
        .absolute()
        .top_0().left_0()
        .w(px(win_w)).h(px(win_h))
        .bg(gpui::rgba(0x000000AA))
        .flex().items_center().justify_center()
        .on_mouse_down(gpui::MouseButton::Left, |_: &gpui::MouseDownEvent, _: &mut Window, _: &mut gpui::App| {})
        .on_mouse_up(gpui::MouseButton::Left, |_: &gpui::MouseUpEvent, _: &mut Window, _: &mut gpui::App| {})
        .on_click(|_: &ClickEvent, _: &mut Window, _: &mut gpui::App| {})
        .child(
            div()
                .id("settings-dialog")
                .w(px(dialog_w)).h(px(dialog_h))
                .bg(gpui::rgba(0x1e1e2eFF))
                .border_1().border_color(gpui::rgba(0x45475aFF))
                .rounded_lg().shadow_lg()
                .flex().flex_col()
                .font_family("Lilex")
                // Header
                .child(
                    h_flex().flex_none().justify_between().items_center()
                        .px(px(16.)).py(px(8.))
                        .child(txt(px(14.)).font_weight(gpui::FontWeight::BOLD).child(t("settings.title")))
                        .child(Button::new("close-settings").ghost().label("\u{2715}").on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            on_close(w, a);
                        }))
                )
                // Body: left tabs + right content
                .child(
                    h_flex().flex_grow(1.0).min_h_0().overflow_hidden()
                        .child(tab_panel(&otc, active_tab_c, &tab_labels))
                        .child(
                            div().flex_grow(1.0).h_full().overflow_y_scrollbar().p(px(20.))
                                .child(tab_content(active_tab, font_size, current_theme_name, theme_buttons,
                                    on_font_down, on_font_up, locale, on_locale_change,
                                    agents, mcp_servers, skills, &osa))
                        )
                )
        )
}

fn tab_panel(otc: &std::rc::Rc<Box<dyn Fn(usize, &mut Window, &mut gpui::App)>>, active: usize, labels: &[String]) -> impl IntoElement {
    v_flex().flex_none().w(px(TAB_PANEL_W)).h_full()
        .border_r_1().border_color(gpui::rgba(0x45475a44)).py_2()
        .children((0..labels.len()).map(|i| {
            let label = labels[i].clone();
            let otc = otc.clone();
            let sel = active == i;
            div().id(format!("tab-{i}")).w_full().py(px(8.)).px(px(12.)).cursor_pointer()
                .when(sel, |d| d.bg(gpui::rgba(0xFFFFFF14)))
                .hover(|d| d.bg(gpui::rgba(0xFFFFFF08)))
                .child(txt(px(13.)).child(label))
                .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { otc(i, w, a); })
        }).collect::<Vec<_>>())
}

fn tab_content(
    active_tab: usize, font_size: f32, current_theme_name: &str,
    theme_buttons: Vec<AnyElement>, on_font_down: ThemeCallback, on_font_up: ThemeCallback,
    locale: &str, on_locale_change: Box<dyn Fn(String, &mut Window, &mut gpui::App)>,
    agents: &[AgentCliInfo], mcp_servers: &[crate::mcp::UnifiedServer],
    skills: &HashMap<String, crate::skills::SkillInfo>,
    on_skill_action: &std::rc::Rc<Box<dyn Fn(SkillAction, &mut Window, &mut gpui::App)>>,
) -> AnyElement {
    match active_tab {
        0 => tab_general(locale, on_locale_change).into_any_element(),
        1 => tab_appearance(font_size, current_theme_name, theme_buttons, on_font_down, on_font_up).into_any_element(),
        2 => tab_terminal().into_any_element(),
        3 => tab_agent_cli(agents).into_any_element(),
        4 => tab_mcp(mcp_servers).into_any_element(),
        5 => tab_skills(skills, on_skill_action).into_any_element(),
        _ => tab_placeholder().into_any_element(),
    }
}
fn tab_general(locale: &str, on_locale_change: Box<dyn Fn(String, &mut Window, &mut gpui::App)>) -> impl IntoElement {
    let loc = locale.to_string();
    let cb = std::rc::Rc::new(on_locale_change);
    v_flex().gap(px(12.))
        .child(setting_group_header(t("settings.general.language")))
        .child(h_flex().gap(px(4.))
            .child(locale_btn("en", "English", &loc, cb.clone()))
            .child(locale_btn("zh-CN", "中文", &loc, cb)))
}

fn locale_btn(code: &str, label: &str, current: &str, on_change: std::rc::Rc<Box<dyn Fn(String, &mut Window, &mut gpui::App)>>) -> impl IntoElement {
    let is_current = code == current;
    let code_s = code.to_string();
    Button::new(format!("locale-{code}"))
        .when(is_current, |b| b.primary()).when(!is_current, |b| b.ghost())
        .label(label.to_string())
        .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { on_change(code_s.clone(), w, a); })
}

// ── Appearance ───────────────────────────────────────────────────────

fn tab_appearance(font_size: f32, current_theme_name: &str, theme_buttons: Vec<AnyElement>, on_font_down: ThemeCallback, on_font_up: ThemeCallback) -> impl IntoElement {
    let mid = (theme_buttons.len() + 1) / 2;
    let mut iter = theme_buttons.into_iter();
    let col1: Vec<_> = iter.by_ref().take(mid).collect();
    let col2: Vec<_> = iter.collect();
    v_flex().gap(px(16.))
        .child(setting_group_header(t("settings.appearance.title")))
        .child(setting_label(t("settings.appearance.theme")))
        .child(h_flex().gap(px(8.))
            .child(v_flex().flex_grow(1.0).gap(px(4.)).children(col1))
            .child(v_flex().flex_grow(1.0).gap(px(4.)).children(col2)))
        .child(setting_row(t("settings.appearance.font_size"),
            h_flex().gap(px(8.)).items_center()
                .child(Button::new("f-dn").ghost().label("\u{2212}").on_click(on_font_down))
                .child(txt(px(15.)).w(px(44.)).text_center().child(format!("{:.0}px", font_size)))
                .child(Button::new("f-up").ghost().label("+").on_click(on_font_up))))
}

// ── Terminal ─────────────────────────────────────────────────────────

fn tab_terminal() -> impl IntoElement {
    v_flex().gap(px(12.))
        .child(setting_group_header(t("settings.terminal.title")))
        .child(setting_row(t("settings.terminal.font_family"), txt(px(12.)).child("Lilex")))
        .child(setting_row(t("settings.terminal.cursor_style"), txt(px(12.)).child(t("settings.terminal.cursor_block"))))
        .child(setting_row(t("settings.terminal.opacity"), txt(px(12.)).child("100%")))
}

// ── Agent CLI ────────────────────────────────────────────────────────

fn tab_agent_cli(agents: &[AgentCliInfo]) -> impl IntoElement {
    v_flex().gap(px(10.))
        .child(setting_group_header(t("settings.tab.agent_cli")))
        .child(h_flex().gap(px(8.)).flex_wrap().children(agents.iter().map(agent_card)))
}

fn agent_card(info: &AgentCliInfo) -> impl IntoElement {
    let (status_text, status_color) = match &info.status {
        AgentCliStatus::Installed { version } => (format!("v{version}"), gpui::rgba(0x00DD44FF)),
        AgentCliStatus::Outdated { current, expected } => (format!("v{current}→v{expected}"), gpui::rgba(0xFFAA00FF)),
        AgentCliStatus::BinaryOnly => ("binary only".into(), gpui::rgba(0x8888FFFF)),
        AgentCliStatus::NotInstalled => ("not installed".into(), gpui::rgba(0xFF4444FF)),
    };
    v_flex().gap(px(4.)).py(px(8.)).px(px(10.)).w(px(170.))
        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
        .child(txt(px(12.)).font_weight(gpui::FontWeight::MEDIUM).child(info.display_name.to_string()))
        .child(h_flex().justify_between().items_center()
            .child(txt(px(10.)).text_color(status_color).child(status_text))
            .child(h_flex().gap(px(4.)).children(agent_action_buttons(info))))
}
// ── MCP ──────────────────────────────────────────────────────────────

fn tab_mcp(servers: &[crate::mcp::UnifiedServer]) -> impl IntoElement {
    v_flex().gap(px(10.))
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(setting_group_header(t("settings.tab.mcp")))
            .child(Button::new("add-mcp").ghost().label(t("mcp.add_server"))
                .on_click(|_: &ClickEvent, _w: &mut Window, _a: &mut gpui::App| { tracing::info!("add mcp"); })))
        .child(if servers.is_empty() {
            txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child(t("mcp.no_servers")).into_any_element()
        } else {
            v_flex().gap(px(10.)).children(servers.iter().map(mcp_card)).into_any_element()
        })
}

fn mcp_card(s: &crate::mcp::UnifiedServer) -> impl IntoElement {
    let name = s.name.clone();
    let enabled = s.disabled_agents.len() < s.agents.len();
    let status = if enabled { t("mcp.enabled") } else { t("mcp.disabled") };
    let sc = if enabled { gpui::rgba(0x00DD44FF) } else { gpui::rgba(0xFF4444FF) };
    let e = name.clone(); let d = name.clone();

    v_flex().gap(px(6.)).py(px(8.)).px(px(10.))
        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(h_flex().gap(px(8.)).items_center()
                .child(txt(px(13.)).font_weight(gpui::FontWeight::MEDIUM).child(name))
                .child(txt(px(11.)).text_color(sc).child(status)))
            .child(h_flex().gap(px(4.))
                .child(Button::new(format!("edit-mcp-{e}")).ghost().label(t("mcp.edit_server")).on_click(move |_: &ClickEvent, _w: &mut Window, _a: &mut gpui::App| { tracing::info!("edit mcp {e}"); }))
                .child(Button::new(format!("del-mcp-{d}")).ghost().label(t("mcp.remove_server")).on_click(move |_: &ClickEvent, _w: &mut Window, _a: &mut gpui::App| { tracing::info!("remove mcp {d}"); }))))
        .child(txt(px(11.)).text_color(gpui::rgba(0x888888FF))
            .child(format!("{} {}", s.command, s.args.join(" "))))
        .child(h_flex().gap(px(6.)).flex_wrap().children(MCP_AGENTS.iter().map(|agent| {
            let has = s.agents.contains(&agent.to_string());
            let disabled = s.disabled_agents.contains(&agent.to_string());
            let checked = has && !disabled;
            mcp_agent_checkbox(agent.to_string(), checked, s.name.clone())
        })))
}

fn agent_action_buttons(info: &AgentCliInfo) -> Vec<AnyElement> {
    let name = info.name.to_string();
    let mut btns: Vec<AnyElement> = Vec::new();
    match &info.status {
        AgentCliStatus::NotInstalled => {
            let n = name.clone();
            btns.push(Button::new(format!("install-{n}")).ghost().compact().label(t("agent.install")).into_any_element());
        }
        AgentCliStatus::Outdated { .. } => {
            let n = name.clone();
            btns.push(Button::new(format!("update-{n}")).ghost().compact().label(t("agent.update")).into_any_element());
        }
        AgentCliStatus::Installed { .. } => {
            let n = name.clone();
            btns.push(Button::new(format!("uninstall-{n}")).ghost().compact().label(t("agent.uninstall")).into_any_element());
        }
        AgentCliStatus::BinaryOnly => {
            let n = name.clone();
            btns.push(Button::new(format!("hook-{n}")).ghost().compact().label(t("agent.install_hook")).into_any_element());
        }
    }
    btns
}

fn mcp_agent_checkbox(agent: String, checked: bool, _server: String) -> impl IntoElement {
    let prefix = if checked { "\u{2611}" } else { "\u{2610}" };
    div().id(format!("mcp-cb-{agent}")).cursor_pointer().py(px(2.)).px(px(4.))
        .rounded_sm().hover(|d| d.bg(gpui::rgba(0xFFFFFF08)))
        .child(txt(px(11.)).child(format!("{prefix} {agent}")))
        .on_click(move |_: &ClickEvent, _w: &mut Window, _a: &mut gpui::App| {
            tracing::info!("mcp checkbox {agent}");
        })
}

fn tab_skills(
    skills: &HashMap<String, crate::skills::SkillInfo>,
    on_skill_action: &std::rc::Rc<Box<dyn Fn(SkillAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    let zenix_skills = crate::skills::zenix_skills();
    let mut sorted: Vec<_> = zenix_skills.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    // Group per-agent skills: agent -> list of skill refs
    let mut per_agent: HashMap<&str, Vec<&crate::skills::SkillInfo>> = HashMap::new();
    for (_, info) in skills {
        for agent in &info.source_agents { per_agent.entry(agent.as_str()).or_default().push(info); }
        for agent in &info.linked_agents { per_agent.entry(agent.as_str()).or_default().push(info); }
    }

    let refresh_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { osa(SkillAction::Refresh, w, a); }
    };

    v_flex().gap(px(16.))
        // ── Zenix Skills section ──
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(setting_group_header(t("skills.zenix_skills")))
            .child(Button::new("refresh-s").ghost().label(t("skills.refresh")).on_click(refresh_cb)))
        .child(h_flex().gap(px(8.)).flex_wrap()
            .children(sorted.iter().map(|s| zenix_skill_card(s, on_skill_action)))
            .child(add_skill_card(on_skill_action)))
        // ── Per-Agent Skills section ──
        .child(setting_group_header(t("skills.per_agent")))
        .child(v_flex().gap(px(12.)).children(SKILL_AGENTS.iter().filter_map(|agent| {
            let agent_skills = per_agent.get(agent)?;
            let mut sorted_skills: Vec<&crate::skills::SkillInfo> = agent_skills.clone();
            sorted_skills.sort_by(|a, b| a.name.cmp(&b.name));
            Some(per_agent_section(agent, &sorted_skills, on_skill_action))
        })))
}
fn zenix_skill_card(
    s: &crate::skills::SkillInfo,
    on_skill_action: &std::rc::Rc<Box<dyn Fn(SkillAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    let name = s.name.clone();
    let link_name = name.clone();
    let remove_name = name.clone();
    let btn_id_link = format!("link-{name}");
    let btn_id_rm = format!("rm-{name}");
    let link_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(SkillAction::Link { skill_name: link_name.clone() }, w, a);
        }
    };
    let remove_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(SkillAction::Remove { skill_name: remove_name.clone() }, w, a);
        }
    };
    h_flex().gap(px(8.)).py(px(6.)).px(px(8.))
        .w(px(180.))
        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
        .child(
            div().flex_1().min_w_0()
                .child(txt(px(12.)).font_weight(gpui::FontWeight::MEDIUM).child(name))
        )
        .child(
            v_flex().gap(px(2.)).items_center()
                .child(txt(px(10.)).text_color(gpui::rgba(0x44DD88FF)).child("link"))
                .child(h_flex().gap(px(2.))
                    .child(Button::new(btn_id_link).ghost().compact().label(t("skills.link")).on_click(link_cb))
                    .child(Button::new(btn_id_rm).ghost().compact().label(t("skills.remove")).on_click(remove_cb)))
        )
}

fn add_skill_card(
    on_skill_action: &std::rc::Rc<Box<dyn Fn(SkillAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    let git_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(SkillAction::InstallGit { url: String::new() }, w, a);
        }
    };
    let local_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(SkillAction::InstallLocal { path: String::new() }, w, a);
        }
    };
    v_flex().gap(px(4.)).py(px(6.)).px(px(8.))
        .w(px(180.))
        .border_1().border_color(gpui::rgba(0x45475a66)).rounded_md()
        .child(txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child(t("skills.add_skill")))
        .child(h_flex().gap(px(4.)).flex_wrap()
            .child(Button::new("add-git").ghost().compact().label(t("skills.add_from_git")).on_click(git_cb))
            .child(Button::new("add-local").ghost().compact().label(t("skills.add_from_local")).on_click(local_cb)))
}

fn per_agent_section(
    agent: &str,
    skills: &[&crate::skills::SkillInfo],
    on_skill_action: &std::rc::Rc<Box<dyn Fn(SkillAction, &mut Window, &mut gpui::App)>>,
) -> AnyElement {
    v_flex().gap(px(8.))
        .child(txt(px(13.)).font_weight(gpui::FontWeight::BOLD).child(agent.to_string()))
        .child(h_flex().gap(px(8.)).flex_wrap()
            .children(skills.iter().map(|s| per_agent_skill_card(agent, s, on_skill_action))))
        .into_any_element()
}

fn per_agent_skill_card(
    agent: &str,
    s: &crate::skills::SkillInfo,
    on_skill_action: &std::rc::Rc<Box<dyn Fn(SkillAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    let name = s.name.clone();
    let is_source = s.source_agents.contains(&agent.to_string());
    let unlink_name = name.clone();
    let unlink_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(SkillAction::Unlink { skill_name: unlink_name.clone() }, w, a);
        }
    };
    let tag_color = if is_source { gpui::rgba(0xDDCC44FF) } else { gpui::rgba(0x44DD88FF) };
    let tag_text = if is_source { "bind" } else { "link" };
    h_flex().gap(px(8.)).py(px(6.)).px(px(8.))
        .w(px(180.))
        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
        .child(
            div().flex_1().min_w_0()
                .child(txt(px(12.)).font_weight(gpui::FontWeight::MEDIUM).child(name.clone()))
        )
        .child(
            v_flex().gap(px(2.)).items_center()
                .child(txt(px(10.)).text_color(tag_color).child(tag_text))
                .child(if is_source {
                    let move_name = name.clone();
                    let move_cb = {
                        let osa = on_skill_action.clone();
                        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            osa(SkillAction::Link { skill_name: move_name.clone() }, w, a);
                        }
                    };
                    Button::new(format!("move-{name}")).ghost().compact().label(t("skills.move_to_zenix")).on_click(move_cb).into_any_element()
                } else {
                    Button::new(format!("unlink-{name}")).ghost().compact().label(t("skills.unlink")).on_click(unlink_cb).into_any_element()
                })
        )
}

fn skill_agent_checkbox(agent: String, checked: bool) -> impl IntoElement {
    let prefix = if checked { "\u{2611}" } else { "\u{2610}" };
    div().id(format!("skill-cb-{agent}")).cursor_pointer().py(px(2.)).px(px(4.))
        .rounded_sm().hover(|d| d.bg(gpui::rgba(0xFFFFFF08)))
        .child(txt(px(11.)).child(format!("{prefix} {agent}")))
        .on_click(move |_: &ClickEvent, _w: &mut Window, _a: &mut gpui::App| { tracing::info!("skill cb {agent}"); })
}
fn tab_placeholder() -> impl IntoElement {
    v_flex().gap(px(12.)).child(setting_group_header("...")).child(setting_label("..."))
}

// ── Helpers ──────────────────────────────────────────────────────────

fn setting_group_header(title: impl Into<String>) -> impl IntoElement {
    div().text_size(px(15.)).font_weight(gpui::FontWeight::BOLD).mb(px(4.)).child(title.into())
}

fn setting_label(text: impl Into<String>) -> impl IntoElement {
    div().text_size(px(12.)).child(text.into())
}

fn setting_row(label: impl Into<String>, ctrl: impl IntoElement) -> impl IntoElement {
    h_flex().gap(px(12.)).items_center().child(setting_label(label)).child(ctrl)
}

fn txt(size: gpui::Pixels) -> gpui::Div { div().text_size(size) }
