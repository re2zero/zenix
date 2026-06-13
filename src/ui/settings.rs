//! Resizable settings dialog with vertical tab navigation.

use std::collections::HashMap;

use gpui::{
    div, px, relative, AnyElement, ClickEvent, InteractiveElement as _, IntoElement,
    ParentElement, StatefulInteractiveElement as _, Styled, Window, prelude::FluentBuilder as _,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex, scroll::ScrollableElement as _, v_flex,
    input::Input,
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

/// Actions that the MCP tab can request from the app.
#[derive(Clone)]
pub enum McpAction {
    ShowAddForm,
    CancelAddForm,
    SubmitAddForm,
    SetFormType { mcp_type: String },
    EditZenix { name: String },
    RemoveZenix { name: String },
    ToggleZenixAgent { server_name: String, agent: String },
    RemoveFromAgent { name: String, agent: String },
    ToggleForAgent { name: String, agent: String },
}

#[derive(Clone)]
pub enum CliAction {
    Install { name: String },
    Update { name: String },
    Uninstall { name: String },
    InstallHook { name: String },
}

/// Mode for the inline dialog input in settings tabs.
#[derive(Clone, PartialEq)]
pub enum InputDialogMode {
    Hidden,
    SkillAddGit,
    SkillAddLocal,
}

/// State for the MCP "Add Server" form.
pub struct McpFormState {
    pub editing_name: Option<String>,
    pub name: Option<gpui::Entity<gpui_component::input::InputState>>,
    pub command: Option<gpui::Entity<gpui_component::input::InputState>>,
    pub args: Option<gpui::Entity<gpui_component::input::InputState>>,
    pub env: Option<gpui::Entity<gpui_component::input::InputState>>,
    pub mcp_type: String,
}

const TAB_PANEL_W: f32 = 140.0;
const TAB_MIN_W: f32 = 480.0;
const TAB_MIN_H: f32 = 360.0;

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
    mcp_zenix_servers: &[crate::mcp::ZenixMcpServer],
    mcp_per_agent: &[(String, Vec<crate::mcp::McpServerEntry>)],
    mcp_form: Option<&McpFormState>,
    mcp_available_agents: &[String],
    skills: &HashMap<String, crate::skills::SkillInfo>,
    skill_dialog_input: Option<&gpui::Entity<gpui_component::input::InputState>>,
    skill_dialog_mode: &InputDialogMode,
    on_skill_action: Box<dyn Fn(SkillAction, &mut Window, &mut gpui::App)>,
    on_mcp_action: Box<dyn Fn(McpAction, &mut Window, &mut gpui::App)>,
    on_cli_action: Box<dyn Fn(CliAction, &mut Window, &mut gpui::App)>,
    win_w: f32,
    win_h: f32,
) -> impl IntoElement {
    let dialog_w = (win_w * 0.75).max(TAB_MIN_W);
    let dialog_h = (win_h * 0.75).max(TAB_MIN_H);
    let active_tab_c = active_tab;
    let otc = std::rc::Rc::new(on_tab_change);
    let osa = std::rc::Rc::new(on_skill_action);
    let oma = std::rc::Rc::new(on_mcp_action);
    let oca = std::rc::Rc::new(on_cli_action);
    let oc = std::rc::Rc::new(on_close);

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
        .on_click({
            let oc = oc.clone();
            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { oc(w, a); }
        })
        .child(
            div()
                .id("settings-dialog")
                .w(px(dialog_w)).h(px(dialog_h))
                .bg(gpui::rgba(0x1e1e2eFF))
                .border_1().border_color(gpui::rgba(0x45475aFF))
                .rounded_lg().shadow_lg()
                .flex().flex_col()
                .font_family("Lilex")
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                // Header
                .child(
                    h_flex().flex_none().justify_between().items_center()
                        .px(px(16.)).py(px(8.))
                        .child(txt(px(14.)).font_weight(gpui::FontWeight::BOLD).child(t("settings.title")))
                        .child(Button::new("close-settings").ghost().label("\u{2715}").on_click({
                            let oc = oc.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                oc(w, a);
                            }
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
                                    agents, mcp_zenix_servers, mcp_per_agent, mcp_form,
                                    mcp_available_agents, skills, skill_dialog_input, skill_dialog_mode,
                                    &osa, &oma, &oca))
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
    agents: &[AgentCliInfo],
    mcp_zenix_servers: &[crate::mcp::ZenixMcpServer],
    mcp_per_agent: &[(String, Vec<crate::mcp::McpServerEntry>)],
    mcp_form: Option<&McpFormState>,
    mcp_available_agents: &[String],
    skills: &HashMap<String, crate::skills::SkillInfo>,
    skill_dialog_input: Option<&gpui::Entity<gpui_component::input::InputState>>,
    skill_dialog_mode: &InputDialogMode,
    on_skill_action: &std::rc::Rc<Box<dyn Fn(SkillAction, &mut Window, &mut gpui::App)>>,
    on_mcp_action: &std::rc::Rc<Box<dyn Fn(McpAction, &mut Window, &mut gpui::App)>>,
    on_cli_action: &std::rc::Rc<Box<dyn Fn(CliAction, &mut Window, &mut gpui::App)>>,
) -> AnyElement {
    match active_tab {
        0 => tab_general(locale, on_locale_change).into_any_element(),
        1 => tab_appearance(font_size, current_theme_name, theme_buttons, on_font_down, on_font_up).into_any_element(),
        2 => tab_terminal().into_any_element(),
        3 => tab_agent_cli(agents, on_cli_action).into_any_element(),
        4 => {
            tab_mcp(mcp_zenix_servers, mcp_per_agent, mcp_form, mcp_available_agents,
                on_mcp_action).into_any_element()
        }
        5 => tab_skills(skills, skill_dialog_input, skill_dialog_mode, on_skill_action).into_any_element(),
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

fn tab_appearance(font_size: f32, _current_theme_name: &str, theme_buttons: Vec<AnyElement>, on_font_down: ThemeCallback, on_font_up: ThemeCallback) -> impl IntoElement {
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

fn tab_agent_cli(
    agents: &[AgentCliInfo],
    on_cli_action: &std::rc::Rc<Box<dyn Fn(CliAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    v_flex().gap(px(10.))
        .child(setting_group_header(t("settings.tab.agent_cli")))
        .child(h_flex().gap(px(8.)).flex_wrap()
            .children(agents.iter().map(|a| agent_card(a, on_cli_action))))
}

fn agent_card(
    info: &AgentCliInfo,
    on_cli_action: &std::rc::Rc<Box<dyn Fn(CliAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    let (status_text, status_color) = match &info.status {
        AgentCliStatus::Installed { version } => (format!("v{version}"), gpui::rgba(0x00DD44FF)),
        AgentCliStatus::Outdated { current, expected } => (format!("v{current}\u{2192}v{expected}"), gpui::rgba(0xFFAA00FF)),
        AgentCliStatus::BinaryOnly => ("binary only".into(), gpui::rgba(0x8888FFFF)),
        AgentCliStatus::NotInstalled => ("not installed".into(), gpui::rgba(0xFF4444FF)),
    };
    v_flex().gap(px(4.)).py(px(8.)).px(px(10.)).w(px(170.))
        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
        .child(txt(px(12.)).font_weight(gpui::FontWeight::MEDIUM).child(info.display_name.to_string()))
        .child(h_flex().justify_between().items_center()
            .child(txt(px(10.)).text_color(status_color).child(status_text))
            .child(h_flex().gap(px(4.)).children(agent_action_buttons(info, on_cli_action))))
}
// ── MCP ──────────────────────────────────────────────────────

fn tab_mcp(
    zenix_servers: &[crate::mcp::ZenixMcpServer],
    per_agent: &[(String, Vec<crate::mcp::McpServerEntry>)],
    mcp_form: Option<&McpFormState>,
    available_agents: &[String],
    on_mcp_action: &std::rc::Rc<Box<dyn Fn(McpAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    let add_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { oma(McpAction::ShowAddForm, w, a); }
    };

    let form_el = mcp_form.map(|form| mcp_add_form(form, on_mcp_action));

    v_flex().gap(px(12.))
        // Header row
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(setting_group_header("MCP Servers"))
            .child(Button::new("add-mcp").ghost().label(t("mcp.add_server")).on_click(add_cb)))
        // Add form (when visible)
        .when_some(form_el, |el, form| el.child(form))
        // Zenix-managed servers
        .child(if zenix_servers.is_empty() && mcp_form.is_none() {
            txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("No zenix-managed servers").into_any_element()
        } else {
            h_flex().flex_wrap().gap(px(10.)).children(zenix_servers.iter().map(|s| {
                div().w(relative(0.48)).child(mcp_card(s, available_agents, on_mcp_action))
            })).into_any_element()
        })
        // Separator
        .child(div().h(px(1.)).w_full().bg(gpui::rgba(0x45475a44)))
        .child(setting_group_header("\u{5df2}\u{914d}\u{7f6e}"))
        .child(v_flex().gap(px(16.)).children(available_agents.iter().map(|agent| {
            let entries = per_agent.iter()
                .find(|(a, _)| a == agent)
                .map(|(_, e)| e.as_slice())
                .unwrap_or(&[]);
            mcp_agent_section(agent, entries, on_mcp_action)
        })))
}

fn mcp_card(
    s: &crate::mcp::ZenixMcpServer,
    available_agents: &[String],
    on_mcp_action: &std::rc::Rc<Box<dyn Fn(McpAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    let name = s.name.clone();
    let enabled_count = s.enabled_agents.len();
    let total = available_agents.len();
    let status = format!("{enabled_count}/{total} agents");
    let sc = if enabled_count > 0 { gpui::rgba(0x00DD44FF) } else { gpui::rgba(0xFF4444FF) };
    let type_label = match s.mcp_type {
        crate::mcp::McpType::Sse => "sse",
        crate::mcp::McpType::Stdio => "stdio",
    };
    let type_color = gpui::rgba(0x8888FFFF);

    let env_text = if s.env.is_empty() {
        String::new()
    } else {
        s.env.keys().map(|k| format!("{k}=***")).collect::<Vec<_>>().join(", ")
    };

    let remove_cb = {
        let oma = on_mcp_action.clone();
        let remove_name = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::RemoveZenix { name: remove_name.clone() }, w, a);
        }
    };
    let edit_cb = {
        let oma = on_mcp_action.clone();
        let edit_name = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::EditZenix { name: edit_name.clone() }, w, a);
        }
    };

    v_flex().gap(px(6.)).py(px(8.)).px(px(10.))
        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(h_flex().gap(px(8.)).items_center()
                .child(txt(px(13.)).font_weight(gpui::FontWeight::MEDIUM).child(name.clone()))
                .child(txt(px(10.)).text_color(type_color).child(type_label.to_string()))
                .child(txt(px(11.)).text_color(sc).child(status)))
            .child(h_flex().gap(px(4.))
                .child(Button::new(format!("edit-zenix-mcp-{name}")).ghost().compact().label("Edit").on_click(edit_cb))
                .child(Button::new(format!("rm-zenix-mcp-{name}")).ghost().compact().label(t("mcp.remove_server")).on_click(remove_cb))))
        .child(txt(px(11.)).text_color(gpui::rgba(0x888888FF))
            .child(format!("{} {}", s.command, s.args.join(" "))))
        .when(!env_text.is_empty(), |el| {
            el.child(txt(px(10.)).text_color(gpui::rgba(0x888888FF)).child(env_text))
        })
        .child(h_flex().gap(px(8.)).flex_wrap().children(available_agents.iter().map(|agent| {
            let checked = s.enabled_agents.contains(agent);
            let srv = name.clone();
            let agent_name = agent.clone();
            let oma = on_mcp_action.clone();
            div().id(format!("mcp-cb-{srv}-{agent_name}")).cursor_pointer().py(px(4.)).px(px(8.))
                .rounded_md().hover(|d| d.bg(gpui::rgba(0xFFFFFF08)))
                .child(txt(px(12.)).child(format!(
                    "{} {agent}",
                    if checked { "\u{2611}" } else { "\u{2610}" }
                )))
                .on_click(move |_: &ClickEvent, w: &mut Window, app: &mut gpui::App| {
                    oma(McpAction::ToggleZenixAgent {
                        server_name: srv.clone(),
                        agent: agent_name.clone(),
                    }, w, app);
                })
        })))
}

fn mcp_add_form(
    form: &McpFormState,
    on_mcp_action: &std::rc::Rc<Box<dyn Fn(McpAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    let cancel_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { oma(McpAction::CancelAddForm, w, a); }
    };
    let submit_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { oma(McpAction::SubmitAddForm, w, a); }
    };

    let is_stdio = form.mcp_type == "stdio" || form.mcp_type.is_empty();
    let submit_label = if form.editing_name.is_some() { "Update" } else { "Add" };
    let form_title = if form.editing_name.is_some() { "Edit MCP Server" } else { "Add MCP Server" };

    v_flex().gap(px(8.)).py(px(10.)).px(px(12.))
        .border_1().border_color(gpui::rgba(0x45475a66)).rounded_md()
        .child(txt(px(13.)).font_weight(gpui::FontWeight::BOLD).child(form_title))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child("Name"))
            .child(form.name.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child("Type"))
            .child(Button::new("mcp-type-stdio")
                .when(is_stdio, |b| b.primary()).when(!is_stdio, |b| b.ghost())
                .compact().label("stdio").on_click({
                    let oma = on_mcp_action.clone();
                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                        oma(McpAction::SetFormType { mcp_type: "stdio".into() }, w, a);
                    }
                }))
            .child(Button::new("mcp-type-sse")
                .when(!is_stdio, |b| b.primary()).when(is_stdio, |b| b.ghost())
                .compact().label("sse").on_click({
                    let oma = on_mcp_action.clone();
                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                        oma(McpAction::SetFormType { mcp_type: "sse".into() }, w, a);
                    }
                })))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child("Command"))
            .child(form.command.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child("Args"))
            .child(form.args.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).items_start()
            .child(txt(px(12.)).w(px(60.)).child("Env"))
            .child(form.env.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).justify_end()
            .child(Button::new("mcp-form-cancel").ghost().label("Cancel").on_click(cancel_cb))
            .child(Button::new("mcp-form-submit").primary().label(submit_label).on_click(submit_cb)))
}

fn mcp_agent_section(
    agent: &str,
    entries: &[crate::mcp::McpServerEntry],
    on_mcp_action: &std::rc::Rc<Box<dyn Fn(McpAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    v_flex().gap(px(8.))
        .child(txt(px(13.)).font_weight(gpui::FontWeight::BOLD).child(agent.to_string()))
        .child(if entries.is_empty() {
            txt(px(11.)).text_color(gpui::rgba(0x666666FF)).child("\u{65e0}").into_any_element()
        } else {
            h_flex().flex_wrap().gap(px(8.)).children(entries.iter().map(|e| {
                div().w(relative(0.48)).child(mcp_agent_entry(agent, e, on_mcp_action))
            })).into_any_element()
        })
}

fn mcp_agent_entry(
    agent: &str,
    entry: &crate::mcp::McpServerEntry,
    on_mcp_action: &std::rc::Rc<Box<dyn Fn(McpAction, &mut Window, &mut gpui::App)>>,
) -> impl IntoElement {
    let name = entry.name.clone();
    let cmd = format!("{} {}", entry.command, entry.args.join(" "));
    let disabled = entry.disabled;

    let toggle_cb = {
        let oma = on_mcp_action.clone();
        let toggle_name = name.clone();
        let toggle_agent = agent.to_string();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::ToggleForAgent { name: toggle_name.clone(), agent: toggle_agent.clone() }, w, a);
        }
    };
    let remove_cb = {
        let oma = on_mcp_action.clone();
        let remove_name = name.clone();
        let remove_agent = agent.to_string();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::RemoveFromAgent { name: remove_name.clone(), agent: remove_agent.clone() }, w, a);
        }
    };

    let status_text = if disabled { t("mcp.disabled") } else { t("mcp.enabled") };
    let status_color = if disabled { gpui::rgba(0xFF4444FF) } else { gpui::rgba(0x00DD44FF) };

    v_flex().gap(px(4.)).py(px(6.)).px(px(10.))
        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(h_flex().gap(px(8.)).items_center()
                .child(txt(px(12.)).font_weight(gpui::FontWeight::MEDIUM).child(name.clone()))
                .child(txt(px(10.)).text_color(status_color).child(status_text)))
            .child(h_flex().gap(px(4.))
                .child(Button::new(format!("toggle-agent-mcp-{agent}-{name}")).ghost().compact()
                    .label(if disabled { t("mcp.enable") } else { t("mcp.disable") })
                    .on_click(toggle_cb))
                .child(Button::new(format!("remove-agent-mcp-{agent}-{name}")).ghost().compact()
                    .label(t("mcp.remove_server"))
                    .on_click(remove_cb))))
        .child(txt(px(10.)).text_color(gpui::rgba(0x888888FF)).child(cmd))
}

fn agent_action_buttons(
    info: &AgentCliInfo,
    on_cli_action: &std::rc::Rc<Box<dyn Fn(CliAction, &mut Window, &mut gpui::App)>>,
) -> Vec<AnyElement> {
    let name = info.name.to_string();
    let mut btns: Vec<AnyElement> = Vec::new();
    match &info.status {
        AgentCliStatus::NotInstalled => {
            let n = name.clone();
            let oca = on_cli_action.clone();
            btns.push(Button::new(format!("install-{n}")).ghost().compact().label(t("agent.install"))
                .on_click(move |_, w, a| { oca(CliAction::Install { name: n.clone() }, w, a); })
                .into_any_element());
        }
        AgentCliStatus::Outdated { .. } => {
            let n1 = name.clone();
            let n2 = name.clone();
            let n3 = name.clone();
            let oca1 = on_cli_action.clone();
            let oca2 = on_cli_action.clone();
            let oca3 = on_cli_action.clone();
            btns.push(Button::new(format!("update-{n1}")).ghost().compact().label(t("agent.update"))
                .on_click(move |_, w, a| { oca1(CliAction::Update { name: n1.clone() }, w, a); })
                .into_any_element());
            btns.push(Button::new(format!("uninstall-{n2}")).ghost().compact().label(t("agent.uninstall"))
                .on_click(move |_, w, a| { oca2(CliAction::Uninstall { name: n2.clone() }, w, a); })
                .into_any_element());
            btns.push(Button::new(format!("hook-{n3}")).ghost().compact().label(t("agent.install_hook"))
                .on_click(move |_, w, a| { oca3(CliAction::InstallHook { name: n3.clone() }, w, a); })
                .into_any_element());
        }
        AgentCliStatus::Installed { .. } => {
            let n1 = name.clone();
            let n2 = name.clone();
            let oca1 = on_cli_action.clone();
            let oca2 = on_cli_action.clone();
            btns.push(Button::new(format!("uninstall-{n1}")).ghost().compact().label(t("agent.uninstall"))
                .on_click(move |_, w, a| { oca1(CliAction::Uninstall { name: n1.clone() }, w, a); })
                .into_any_element());
            btns.push(Button::new(format!("hook-{n2}")).ghost().compact().label(t("agent.install_hook"))
                .on_click(move |_, w, a| { oca2(CliAction::InstallHook { name: n2.clone() }, w, a); })
                .into_any_element());
        }
        AgentCliStatus::BinaryOnly => {
            let n = name.clone();
            let oca = on_cli_action.clone();
            btns.push(Button::new(format!("hook-{n}")).ghost().compact().label(t("agent.install_hook"))
                .on_click(move |_, w, a| { oca(CliAction::InstallHook { name: n.clone() }, w, a); })
                .into_any_element());
        }
    }
    btns
}

fn tab_skills(
    skills: &HashMap<String, crate::skills::SkillInfo>,
    skill_dialog_input: Option<&gpui::Entity<gpui_component::input::InputState>>,
    skill_dialog_mode: &InputDialogMode,
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
        .when_some(skill_dialog_input, |el, inp| {
            el.when(*skill_dialog_mode == InputDialogMode::SkillAddGit, |el| {
                el.child(v_flex().gap(px(8.)).py(px(8.)).px(px(10.))
                    .border_1().border_color(gpui::rgba(0x45475a66)).rounded_md()
                    .child(txt(px(12.)).child("Git URL"))
                    .child(Input::new(inp)))
            })
            .when(*skill_dialog_mode == InputDialogMode::SkillAddLocal, |el| {
                el.child(v_flex().gap(px(8.)).py(px(8.)).px(px(10.))
                    .border_1().border_color(gpui::rgba(0x45475a66)).rounded_md()
                    .child(txt(px(12.)).child("Local Path"))
                    .child(Input::new(inp)))
            })
        })
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
