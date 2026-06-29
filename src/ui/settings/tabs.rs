//! Tab rendering functions extracted from settings.rs.

use std::collections::HashMap;
use std::rc::Rc;

use crate::agent::AgentCliInfo;
use crate::api_provider::ApiProviderConfig;
use crate::i18n::t;
use crate::skills::ApiSkill;
use gpui::{
    AnyElement, ClickEvent, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    scroll::ScrollableElement as _,
    searchable_list::SearchableVec,
    select::{Select, SelectState},
    v_flex,
};

use crate::ui::settings::helpers::*;

use super::components;
use super::{
    ActionCb, ApiProviderAction, ApiProviderFormState, ExtensionSubTab, InputDialogMode, McpAction,
    McpFormState, SharedActionCb, SkillAction, SkillsSubTab, TerminalAction, ThemeCallback,
};

// ── Tab Content Dispatcher ────────────────────────────────────────────

pub fn tab_content(
    active_tab: usize,
    font_size: f32,
    current_theme_name: &str,
    theme_buttons: Vec<AnyElement>,
    on_font_down: ThemeCallback,
    on_font_up: ThemeCallback,
    locale: &str,
    on_locale_change: ActionCb<String>,
    agents: &[AgentCliInfo],
    selected_agent: Option<usize>,
    on_agent_select: &SharedActionCb<Option<usize>>,
    mcp_zenix_servers: &[crate::mcp::ZenixMcpServer],
    mcp_per_agent: &[(String, Vec<crate::mcp::McpServerEntry>)],
    mcp_form: Option<&McpFormState>,
    mcp_available_agents: &[String],
    skills: &HashMap<String, crate::skills::SkillInfo>,
    skill_dialog_input: Option<&Entity<gpui_component::input::InputState>>,
    skill_dialog_mode: &InputDialogMode,
    on_skill_action: &SharedActionCb<SkillAction>,
    on_mcp_action: &SharedActionCb<McpAction>,
    api_providers: &[ApiProviderConfig],
    api_form: Option<&ApiProviderFormState>,
    on_api_action: &SharedActionCb<ApiProviderAction>,
    font_select: Option<&Entity<SelectState<SearchableVec<String>>>>,
    active_extension_subtab: ExtensionSubTab,
    on_extension_tab_change: &SharedActionCb<ExtensionSubTab>,
    mcp_test_results: &HashMap<String, String>,
    api_test_results: &HashMap<String, String>,
    agent_search: &str,
    on_agent_refresh: &SharedActionCb<()>,
    on_tab_change: &SharedActionCb<usize>,
    agent_search_input: Option<&Entity<gpui_component::input::InputState>>,
    on_agent_toggle_installed: &SharedActionCb<bool>,
    agent_installed_only: bool,
    active_skills_subtab: SkillsSubTab,
    skill_search: &str,
    skill_search_input: Option<&Entity<gpui_component::input::InputState>>,
    mcp_search: &str,
    mcp_search_input: Option<&Entity<gpui_component::input::InputState>>,
    api_search: &str,
    api_search_input: Option<&Entity<gpui_component::input::InputState>>,
    terminal_cursor_blink: bool,
    terminal_cursor_style: &str,
    terminal_opacity: f32,
    terminal_scrollback: u32,
    on_terminal_action: &SharedActionCb<TerminalAction>,
    market_skills: &[ApiSkill],
    market_loading: bool,
    market_error: Option<&str>,
) -> AnyElement {
    match active_tab {
        0 => tab_general(locale, on_locale_change).into_any_element(),
        1 => tab_appearance(
            font_size,
            current_theme_name,
            theme_buttons,
            on_font_down,
            on_font_up,
            font_select,
        )
        .into_any_element(),
        2 => tab_terminal(
            terminal_cursor_blink,
            terminal_cursor_style,
            terminal_opacity,
            terminal_scrollback,
            on_terminal_action,
        )
        .into_any_element(),
        3 => tab_agent_cli(
            agents,
            selected_agent,
            on_agent_select,
            mcp_per_agent,
            skills,
            api_providers,
            mcp_available_agents,
            on_mcp_action,
            on_skill_action,
            mcp_test_results,
            agent_search,
            agent_search_input,
            on_agent_toggle_installed,
            agent_installed_only,
            on_agent_refresh,
            on_tab_change,
            on_extension_tab_change,
        )
        .into_any_element(),
        _ => tab_extensions(
            active_extension_subtab,
            on_extension_tab_change,
            mcp_zenix_servers,
            mcp_per_agent,
            mcp_form,
            mcp_available_agents,
            on_mcp_action,
            skills,
            skill_dialog_input,
            skill_search,
            skill_search_input,
            skill_dialog_mode,
            on_skill_action,
            mcp_search,
            mcp_search_input,
            api_search,
            api_search_input,
            api_providers,
            api_form,
            mcp_test_results,
            on_api_action,
            api_test_results,
            active_skills_subtab,
            market_skills,
            market_loading,
            market_error,
        )
        .into_any_element(),
    }
}

// ── General Tab ───────────────────────────────────────────────────────

fn tab_general(locale: &str, on_locale_change: ActionCb<String>) -> impl IntoElement {
    let loc = locale.to_string();
    let cb = Rc::new(on_locale_change);
    let config_dir = crate::platform::zenix_config_dir();
    v_flex()
        .gap(px(16.))
        .child(setting_group_header(t("settings.general.language")))
        .child(
            h_flex()
                .gap(px(4.))
                .child(locale_btn("en", "English", &loc, cb.clone()))
                .child(locale_btn("zh-CN", "\u{4e2d}\u{6587}", &loc, cb)),
        )
        .child(setting_group_header(t("settings.about")))
        .child(txt(px(12.)).child(format!("zenix v{}", env!("CARGO_PKG_VERSION"))))
        .child(
            txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(
                t("settings.config_path").replace("{path}", &config_dir.display().to_string()),
            ),
        )
}

fn locale_btn(
    code: &str,
    label: &str,
    current: &str,
    on_change: SharedActionCb<String>,
) -> impl IntoElement {
    let is_current = code == current;
    let code_s = code.to_string();
    Button::new(format!("locale-{code}"))
        .when(is_current, |b| b.primary())
        .when(!is_current, |b| b.ghost())
        .label(label.to_string())
        .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on_change(code_s.clone(), w, a);
        })
}

// ── Appearance Tab ────────────────────────────────────────────────────

fn tab_appearance(
    font_size: f32,
    _current_theme_name: &str,
    theme_buttons: Vec<AnyElement>,
    on_font_down: ThemeCallback,
    on_font_up: ThemeCallback,
    font_select: Option<&Entity<SelectState<SearchableVec<String>>>>,
) -> impl IntoElement {
    let mid = theme_buttons.len().div_ceil(2);
    let mut iter = theme_buttons.into_iter();
    let col1: Vec<_> = iter.by_ref().take(mid).collect();
    let col2: Vec<_> = iter.collect();
    v_flex()
        .gap(px(16.))
        .child(setting_group_header(t("settings.appearance.title")))
        .child(setting_label(t("settings.appearance.theme")))
        .child(
            h_flex()
                .gap(px(8.))
                .child(v_flex().flex_grow(1.0).gap(px(4.)).children(col1))
                .child(v_flex().flex_grow(1.0).gap(px(4.)).children(col2)),
        )
        .child(setting_row(
            t("settings.appearance.font"),
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    Button::new("f-dn")
                        .ghost()
                        .label("\u{2212}")
                        .text_size(px(18.))
                        .on_click(on_font_down),
                )
                .child(
                    txt(px(15.))
                        .w(px(44.))
                        .text_center()
                        .child(format!("{:.0}px", font_size)),
                )
                .child(
                    Button::new("f-up")
                        .ghost()
                        .label("+")
                        .text_size(px(18.))
                        .on_click(on_font_up),
                )
                .child(
                    font_select
                        .map(|sel| Select::new(sel).w(px(220.)).into_any_element())
                        .unwrap_or_else(|| {
                            div()
                                .w(px(220.))
                                .child(t("settings.appearance.change_font"))
                                .into_any_element()
                        }),
                ),
        ))
}

// ── Terminal Tab ──────────────────────────────────────────────────────

fn tab_terminal(
    cursor_blink: bool,
    cursor_style: &str,
    opacity: f32,
    scrollback: u32,
    ota: &SharedActionCb<TerminalAction>,
) -> impl IntoElement {
    let style_btn = |style: &'static str, current: &str| {
        let sel = style == current;
        let label = match style {
            "block" => t("settings.terminal.cursor_block"),
            "bar" => t("settings.terminal.cursor_bar"),
            "underline" => t("settings.terminal.cursor_underline"),
            _ => style.to_string(),
        };
        let ota = ota.clone();
        let s = style.to_string();
        Button::new(format!("cstyle-{style}"))
            .cursor_pointer()
            .when(sel, |b| b.primary())
            .when(!sel, |b| b.ghost())
            .compact()
            .label(label.to_string())
            .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                ota(TerminalAction::SetCursorStyle(s.clone()), w, a)
            })
    };

    let blink_label = if cursor_blink {
        "\u{2611} on"
    } else {
        "\u{2610} off"
    };
    let ota_blink = ota.clone();

    v_flex()
        .gap(px(16.))
        .child(setting_group_header(t("settings.terminal.title")))
        .child(setting_label(t("settings.terminal.cursor_style")))
        .child(
            h_flex()
                .gap(px(8.))
                .child(style_btn("block", cursor_style))
                .child(style_btn("bar", cursor_style))
                .child(style_btn("underline", cursor_style)),
        )
        .child(setting_row(
            t("settings.terminal.cursor_blink"),
            Button::new("cursor-blink")
                .when(cursor_blink, |b| b.primary())
                .when(!cursor_blink, |b| b.ghost())
                .label(blink_label)
                .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                    ota_blink(TerminalAction::ToggleCursorBlink, w, a)
                }),
        ))
        .child(setting_label(t("settings.terminal.opacity")))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(
                    Button::new("opacity-down")
                        .ghost()
                        .label("-")
                        .text_size(px(18.))
                        .on_click({
                            let ota = ota.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                let new_op = (opacity * 10.0 - 1.0).clamp(0.5, 10.0) / 10.0;
                                ota(TerminalAction::SetOpacity(new_op), w, a);
                            }
                        }),
                )
                .child(
                    txt(px(12.))
                        .w(px(40.))
                        .child(format!("{:.0}%", opacity * 100.0)),
                )
                .child(
                    Button::new("opacity-up")
                        .ghost()
                        .label("+")
                        .text_size(px(18.))
                        .on_click({
                            let ota = ota.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                let new_op = (opacity * 10.0 + 1.0).clamp(0.5, 10.0) / 10.0;
                                ota(TerminalAction::SetOpacity(new_op), w, a);
                            }
                        }),
                ),
        )
        .child(setting_row(
            t("settings.terminal.scrollback"),
            h_flex()
                .gap(px(4.))
                .items_center()
                .child(
                    Button::new("scrollback-down")
                        .ghost()
                        .label("-")
                        .text_size(px(18.))
                        .on_click({
                            let ota = ota.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                let new_sb = (scrollback as i32 - 100).max(100) as u32;
                                ota(TerminalAction::SetScrollback(new_sb), w, a);
                            }
                        }),
                )
                .child(txt(px(12.)).w(px(60.)).child(format!("{scrollback}")))
                .child(
                    Button::new("scrollback-up")
                        .ghost()
                        .label("+")
                        .text_size(px(18.))
                        .on_click({
                            let ota = ota.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                let new_sb = (scrollback as i32 + 100).max(100) as u32;
                                ota(TerminalAction::SetScrollback(new_sb), w, a);
                            }
                        }),
                )
                .child(
                    txt(px(10.))
                        .text_color(gpui::rgba(0x666666FF))
                        .child(t("settings.terminal.lines")),
                ),
        ))
}

// ── Agent CLI Tab ─────────────────────────────────────────────────────

fn tab_agent_cli(
    agents: &[AgentCliInfo],
    selected_idx: Option<usize>,
    on_agent_select: &SharedActionCb<Option<usize>>,
    mcp_per_agent: &[(String, Vec<crate::mcp::McpServerEntry>)],
    skills: &HashMap<String, crate::skills::SkillInfo>,
    api_providers: &[ApiProviderConfig],
    mcp_available_agents: &[String],
    on_mcp_action: &SharedActionCb<McpAction>,
    on_skill_action: &SharedActionCb<SkillAction>,
    mcp_test_results: &HashMap<String, String>,
    agent_search: &str,
    agent_search_input: Option<&Entity<gpui_component::input::InputState>>,
    on_agent_toggle_installed: &SharedActionCb<bool>,
    agent_installed_only: bool,
    on_agent_refresh: &SharedActionCb<()>,
    on_tab_change: &SharedActionCb<usize>,
    on_extension_tab_change: &SharedActionCb<ExtensionSubTab>,
) -> impl IntoElement {
    let filtered_agents: Vec<(usize, &AgentCliInfo)> = agents
        .iter()
        .enumerate()
        .filter(|(_, info)| {
            let matches_search = agent_search.is_empty()
                || info.name.contains(agent_search)
                || info
                    .display_name
                    .to_lowercase()
                    .contains(&agent_search.to_lowercase());
            let matches_installed = !agent_installed_only || info.detected;
            matches_search && matches_installed
        })
        .collect();
    let _has_filter = !agent_search.is_empty() || agent_installed_only;

    // Re-map selected_idx through filtered list
    let _display_selected = selected_idx.and_then(|sid| {
        filtered_agents
            .iter()
            .position(|(orig_i, _)| *orig_i == sid)
    });

    let installed_toggle_cb = {
        let oati = on_agent_toggle_installed.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oati(!agent_installed_only, w, a);
        }
    };

    let installed_label = format!(
        "{} {}",
        if agent_installed_only {
            "\u{2611}"
        } else {
            "\u{2610}"
        },
        t("settings.agent_cli.installed_only")
    );

    v_flex().gap(px(8.)).h_full()
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(setting_group_header(t("settings.tab.agent_cli")))
            .child(Button::new("agent-refresh")
                .cursor_pointer()
                .ghost()
                .label(t("settings.agent_cli.refresh"))
                .on_click({
                    let oar = on_agent_refresh.clone();
                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| oar((), w, a)
                }))
        )
        .child(
            h_flex().flex_grow(1.0).min_h_0().gap(px(8.))
                // Left panel: agent list
                .child(
                    v_flex().flex_none().w(px(210.)).h_full().gap(px(4.))
                        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
                        .child(
                            v_flex().gap_0()
                                .child(
                                    // Search row
                                    h_flex().gap(px(4.)).px(px(6.)).py(px(6.)).items_center()
                                        .border_b_1().border_color(gpui::rgba(0x45475a33))
                                        .child(txt(px(16.)).text_color(gpui::rgba(0x888888FF)).child("\u{1F50D}"))
                                        .child(agent_search_input.map(|e| Input::new(e).flex_grow(1.0).into_any_element())
                                            .unwrap_or_else(|| txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(t("settings.agent_cli.search_hint")).into_any_element()))
                                )
                                .child(
                                    // Installed-only toggle
                                    h_flex().gap(px(4.)).px(px(6.)).py(px(3.))
                                        .border_b_1().border_color(gpui::rgba(0x45475a33))
                                        .child(Button::new("installed-only")
                                            .cursor_pointer()
                                            .when(agent_installed_only, |b| b.primary())
                                            .when(!agent_installed_only, |b| b.ghost())
                                            .compact()
                                            .label(installed_label)
                                            .on_click(installed_toggle_cb)
                                        )
                                        .child(txt(px(10.)).text_color(gpui::rgba(0x666666FF)).child(format!("{} / {}", filtered_agents.len(), agents.len())))
                                )
                                .child(
                                    div().overflow_y_scrollbar().child(
                                        v_flex().gap_0()
                                            .children(filtered_agents.iter().map(|(orig_i, info)| {
                                                let sel = selected_idx == Some(*orig_i);
                                                let oas = on_agent_select.clone();
                                                let indicator = if info.detected { "\u{25CF}" } else { "\u{25CB}" };
                                                let ind_color = if info.detected { gpui::rgba(0x00DD44FF) } else { gpui::rgba(0x666666FF) };
                                                let orig_idx = *orig_i;
                                                div().id(format!("agent-{}", info.name)).w_full()
                                                    .when(sel, |d| d.bg(gpui::rgba(0xFFFFFF14)))
                                                    .hover(|d| d.bg(gpui::rgba(0xFFFFFF08)))
                                                    .cursor_pointer()
                                                    .px(px(10.)).py(px(6.))
                                                    .child(
                                                        h_flex().gap(px(6.)).items_center()
                                                            .child(txt(px(12.)).text_color(ind_color).child(indicator))
                                                            .child(txt(px(12.)).child(info.display_name.to_string()))
                                                    )
                                                    .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                                        oas(Some(orig_idx), w, a);
                                                    })
                                            }))
                                    )
                                )
                        )
                )
                // Right panel: detail
                .child(
                    div().flex_grow(1.0).h_full().overflow_y_scrollbar()
                        .child(
                            match selected_idx.and_then(|i| agents.get(i)) {
                                None => txt(px(12.)).text_color(gpui::rgba(0x888888FF))
                                    .child(t("settings.agent_cli.no_agent_selected"))
                                    .into_any_element(),
                                Some(agent) => agent_detail_panel(
                                    agent, mcp_per_agent, skills, api_providers,
                                    mcp_available_agents, on_mcp_action, on_skill_action,
                                    mcp_test_results, on_tab_change, on_extension_tab_change,
                                ),
                            }
                        )
                )
        )
}

fn agent_detail_panel(
    agent: &AgentCliInfo,
    mcp_per_agent: &[(String, Vec<crate::mcp::McpServerEntry>)],
    skills: &HashMap<String, crate::skills::SkillInfo>,
    api_providers: &[ApiProviderConfig],
    _mcp_available_agents: &[String],
    on_mcp_action: &SharedActionCb<McpAction>,
    on_skill_action: &SharedActionCb<SkillAction>,
    mcp_test_results: &HashMap<String, String>,
    on_tab_change: &SharedActionCb<usize>,
    on_extension_tab_change: &SharedActionCb<ExtensionSubTab>,
) -> AnyElement {
    let agent_name = agent.name;
    let status_text = if agent.detected {
        t("agent.installed")
    } else {
        t("agent.not_installed")
    };
    let status_color = if agent.detected {
        gpui::rgba(0x00DD44FF)
    } else {
        gpui::rgba(0xFF4444FF)
    };

    let mcp_entries: Vec<&crate::mcp::McpServerEntry> = mcp_per_agent
        .iter()
        .find(|(name, _)| name == agent_name)
        .map(|(_, entries)| entries.iter().collect())
        .unwrap_or_default();

    let agent_skills: Vec<&crate::skills::SkillInfo> = skills
        .values()
        .filter(|s| {
            s.source_agents.contains(&agent_name.to_string())
                || s.linked_agents.contains(&agent_name.to_string())
        })
        .collect();

    let agent_providers: Vec<&ApiProviderConfig> = api_providers
        .iter()
        .filter(|p| p.enabled_agents.contains(&agent_name.to_string()))
        .collect();

    v_flex()
        .gap(px(16.))
        .child(
            v_flex()
                .gap(px(4.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .child(
                            txt(px(15.))
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(agent.display_name.to_string()),
                        )
                        .child(txt(px(11.)).text_color(status_color).child(status_text)),
                )
                .when_some(agent.binary_path.as_ref(), |el, path| {
                    el.child(
                        txt(px(11.))
                            .text_color(gpui::rgba(0x888888FF))
                            .child(format!(
                                "{}: {}",
                                t("settings.agent_cli.binary_path"),
                                path.display()
                            )),
                    )
                }),
        )
        .when(!agent.detected, |el| {
            el.child(
                v_flex()
                    .gap(px(4.))
                    .py(px(8.))
                    .px(px(10.))
                    .border_1()
                    .border_color(gpui::rgba(0x44AAFFFF))
                    .rounded_md()
                    .child(txt(px(12.)).child(t("agent.install")))
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(4.))
                            .bg(gpui::rgba(0x00000044))
                            .rounded_sm()
                            .child(
                                txt(px(11.))
                                    .font_family("monospace")
                                    .child(agent.install_command.to_string()),
                            ),
                    ),
            )
        })
        .child(
            v_flex()
                .gap(px(8.))
                .child(
                    txt(px(13.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(t("settings.api.title")),
                )
                .child(if agent_providers.is_empty() {
                    txt(px(11.))
                        .text_color(gpui::rgba(0x888888FF))
                        .child(t("settings.no_api_providers"))
                        .into_any_element()
                } else {
                    v_flex()
                        .gap(px(6.))
                        .children(agent_providers.iter().map(|p| {
                            let fmt_color = match p.format {
                                crate::api_provider::ApiFormat::OpenAI => gpui::rgba(0x44AADDFF),
                                crate::api_provider::ApiFormat::Anthropic => gpui::rgba(0xDD8844FF),
                            };
                            h_flex()
                                .gap(px(8.))
                                .py(px(4.))
                                .px(px(8.))
                                .border_1()
                                .border_color(gpui::rgba(0x45475a44))
                                .rounded_md()
                                .child(
                                    txt(px(12.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(p.name.clone()),
                                )
                                .child(
                                    txt(px(10.))
                                        .text_color(fmt_color)
                                        .child(p.format.label().to_string()),
                                )
                        }))
                        .into_any_element()
                }),
        )
        .child(
            v_flex()
                .gap(px(8.))
                .child(
                    txt(px(13.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(t("settings.mcp.title")),
                )
                .child(if mcp_entries.is_empty() {
                    txt(px(11.))
                        .text_color(gpui::rgba(0x888888FF))
                        .child(t("mcp.no_servers"))
                        .into_any_element()
                } else {
                    v_flex()
                        .gap(px(6.))
                        .children(mcp_entries.iter().map(|entry| {
                            components::mcp_agent_entry_with_test(
                                agent_name,
                                entry,
                                on_mcp_action,
                                mcp_test_results,
                            )
                        }))
                        .into_any_element()
                }),
        )
        .child(
            div()
                .cursor_pointer()
                .child(
                    txt(px(11.))
                        .text_color(gpui::rgba(0x44AADDFF))
                        .child(t("settings.agent_cli.add_mcp_extensions")),
                )
                .on_mouse_down(gpui::MouseButton::Left, {
                    let otc = on_tab_change.clone();
                    let oetc = on_extension_tab_change.clone();
                    move |_: &gpui::MouseDownEvent, w: &mut Window, a: &mut gpui::App| {
                        otc(4, w, a);
                        oetc(ExtensionSubTab::Mcp, w, a);
                    }
                }),
        )
        .child(
            v_flex()
                .gap(px(8.))
                .child(
                    txt(px(13.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(t("settings.tab.skills")),
                )
                .child(if agent_skills.is_empty() {
                    txt(px(11.))
                        .text_color(gpui::rgba(0x888888FF))
                        .child(t("skills.no_skills"))
                        .into_any_element()
                } else {
                    h_flex()
                        .gap(px(8.))
                        .flex_wrap()
                        .children(
                            agent_skills
                                .iter()
                                .map(|s| components::per_agent_skill_card(agent_name, s, on_skill_action)),
                        )
                        .into_any_element()
                }),
        )
        .into_any_element()
}

// ── Extensions Tab ────────────────────────────────────────────────────

fn tab_extensions(
    active_sub: ExtensionSubTab,
    on_sub_tab_change: &SharedActionCb<ExtensionSubTab>,
    mcp_servers: &[crate::mcp::ZenixMcpServer],
    mcp_per_agent: &[(String, Vec<crate::mcp::McpServerEntry>)],
    mcp_form: Option<&McpFormState>,
    mcp_available_agents: &[String],
    on_mcp_action: &SharedActionCb<McpAction>,
    skills: &HashMap<String, crate::skills::SkillInfo>,
    skill_dialog_input: Option<&Entity<gpui_component::input::InputState>>,
    skill_search: &str,
    skill_search_input: Option<&Entity<gpui_component::input::InputState>>,
    skill_dialog_mode: &InputDialogMode,
    on_skill_action: &SharedActionCb<SkillAction>,
    mcp_search: &str,
    mcp_search_input: Option<&Entity<gpui_component::input::InputState>>,
    api_search: &str,
    api_search_input: Option<&Entity<gpui_component::input::InputState>>,
    api_providers: &[ApiProviderConfig],
    api_form: Option<&ApiProviderFormState>,
    mcp_test_results: &HashMap<String, String>,
    on_api_action: &SharedActionCb<ApiProviderAction>,
    api_test_results: &HashMap<String, String>,
    active_skills_subtab: SkillsSubTab,
    market_skills: &[ApiSkill],
    market_loading: bool,
    market_error: Option<&str>,
) -> impl IntoElement {
    let otc = on_sub_tab_change.clone();
    let labels: Vec<(String, ExtensionSubTab)> = vec![
        (t("settings.tab.api"), ExtensionSubTab::Api),
        (t("settings.tab.mcp"), ExtensionSubTab::Mcp),
        (t("settings.tab.skills"), ExtensionSubTab::Skills),
    ];
    v_flex()
        .gap(px(12.))
        .child(zed_tab_bar(
            "ext-sub",
            labels.iter().map(|(l, _)| l.clone()).collect(),
            labels
                .iter()
                .position(|(_, s)| *s == active_sub)
                .unwrap_or(0),
            {
                let labels = labels.clone();
                move |idx, w, a| {
                    if let Some((_, sub)) = labels.get(idx) {
                        otc(*sub, w, a);
                    }
                }
            },
        ))
        .child(match active_sub {
            ExtensionSubTab::Api => tab_api_content(
                api_providers,
                api_form,
                mcp_available_agents,
                on_api_action,
                api_test_results,
                api_search,
                api_search_input,
            )
            .into_any_element(),
            ExtensionSubTab::Mcp => tab_mcp_content(
                mcp_servers,
                mcp_per_agent,
                mcp_form,
                mcp_available_agents,
                on_mcp_action,
                mcp_test_results,
                mcp_search,
                mcp_search_input,
            )
            .into_any_element(),
            ExtensionSubTab::Skills => tab_skills_content(
                skills,
                skill_dialog_input,
                skill_dialog_mode,
                on_skill_action,
                active_skills_subtab,
                skill_search,
                skill_search_input,
                market_skills,
                market_loading,
                market_error,
            )
            .into_any_element(),
        })
}

// ── MCP Content ───────────────────────────────────────────────────────

fn tab_mcp_content(
    zenix_servers: &[crate::mcp::ZenixMcpServer],
    _per_agent: &[(String, Vec<crate::mcp::McpServerEntry>)],
    mcp_form: Option<&McpFormState>,
    available_agents: &[String],
    on_mcp_action: &SharedActionCb<McpAction>,
    mcp_test_results: &HashMap<String, String>,
    mcp_search: &str,
    mcp_search_input: Option<&Entity<gpui_component::input::InputState>>,
) -> impl IntoElement {
    let add_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::ShowAddForm, w, a);
        }
    };

    // Filter servers by search
    let filtered_servers: Vec<&crate::mcp::ZenixMcpServer> = if mcp_search.is_empty() {
        zenix_servers.iter().collect()
    } else {
        let q = mcp_search.to_lowercase();
        zenix_servers
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&q))
            .collect()
    };

    // Only show the form if we're adding a new server (not editing an existing one)
    // When editing, the card itself renders the inline edit form
    let form_el = mcp_form
        .filter(|f| f.editing_name.is_none())
        .map(|form| components::mcp_add_form(form, on_mcp_action));

    v_flex()
        .gap(px(12.))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .justify_between()
                .child(setting_group_header(t("settings.mcp.title")))
                .child(
                    Button::new("add-mcp")
                        .cursor_pointer()
                        .ghost()
                        .label(t("mcp.add_server"))
                        .on_click(add_cb),
                ),
        )
        .child(zed_search_bar(&t("settings.mcp.search"), mcp_search_input))
        .when_some(form_el, |el, form| el.child(form))
        .child(if filtered_servers.is_empty() && mcp_form.is_none() {
            txt(px(12.))
                .text_color(gpui::rgba(0x888888FF))
                .child(t("settings.mcp.no_zenix_servers"))
                .into_any_element()
        } else {
            v_flex()
                .gap(px(8.))
                .children(filtered_servers.iter().map(|s| {
                    components::mcp_card(
                        s,
                        available_agents,
                        on_mcp_action,
                        mcp_form,
                        mcp_test_results,
                    )
                }))
                .into_any_element()
        })
}

// ── Skills Content ────────────────────────────────────────────────────

fn tab_skills_content(
    _skills: &HashMap<String, crate::skills::SkillInfo>,
    skill_dialog_input: Option<&Entity<gpui_component::input::InputState>>,
    skill_dialog_mode: &InputDialogMode,
    on_skill_action: &SharedActionCb<SkillAction>,
    active_subtab: SkillsSubTab,
    skill_search: &str,
    skill_search_input: Option<&Entity<gpui_component::input::InputState>>,
    market_skills: &[ApiSkill],
    market_loading: bool,
    market_error: Option<&str>,
) -> impl IntoElement {
    let zenix_skills = crate::skills::zenix_skills();
    let installed_names: std::collections::HashSet<String> =
        zenix_skills.iter().map(|s| s.name.clone()).collect();

    let refresh_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            osa(SkillAction::Refresh, w, a);
        }
    };

    let labels: Vec<(String, SkillsSubTab)> = vec![
        (t("skills.installed"), SkillsSubTab::Installed),
        (t("skills.market"), SkillsSubTab::Market),
    ];

    let otc = on_skill_action.clone();

    v_flex()
        .gap(px(12.))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .justify_between()
                .child(zed_tab_bar(
                    "skills-sub",
                    labels.iter().map(|(l, _)| l.clone()).collect(),
                    if active_subtab == SkillsSubTab::Installed {
                        0
                    } else {
                        1
                    },
                    {
                        let labels = labels.clone();
                        let otc = otc.clone();
                        move |idx, w, a| {
                            if let Some((_, sub)) = labels.get(idx) {
                                otc(SkillAction::SetSubTab(*sub), w, a);
                            }
                        }
                    },
                ))
                .child(
                    Button::new("refresh-s")
                        .cursor_pointer()
                        .ghost()
                        .label(t("general.refresh"))
                        .on_click(refresh_cb),
                ),
        )
        .child(zed_search_bar(&t("skills.search"), skill_search_input))
        .child(match active_subtab {
            SkillsSubTab::Installed => {
                let filtered: Vec<&crate::skills::SkillInfo> = zenix_skills
                    .iter()
                    .filter(|s| {
                        skill_search.is_empty()
                            || s.name.to_lowercase().contains(&skill_search.to_lowercase())
                    })
                    .collect();
                if filtered.is_empty() {
                    txt(px(12.))
                        .text_color(gpui::rgba(0x888888FF))
                        .child(t("skills.no_installed"))
                        .into_any_element()
                } else {
                    h_flex()
                        .gap(px(8.))
                        .flex_wrap()
                        .min_w_0()
                        .overflow_x_hidden()
                        .children(
                            filtered
                                .iter()
                                .map(|s| components::zenix_skill_card(s, on_skill_action)),
                        )
                        .into_any_element()
                }
            }
            SkillsSubTab::Market => {
                let fetch_cb = {
                    let osa = on_skill_action.clone();
                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                        osa(SkillAction::FetchMarket, w, a);
                    }
                };

                let market_content =
                    if market_loading {
                        txt(px(12.))
                            .text_color(gpui::rgba(0x888888FF))
                            .child(t("skills.market_loading"))
                            .into_any_element()
                    } else if let Some(err) = market_error {
                        txt(px(12.))
                            .text_color(gpui::rgba(0xFF4444FF))
                            .child(err.to_string())
                            .into_any_element()
                    } else if market_skills.is_empty() {
                        v_flex()
                            .gap(px(8.))
                            .child(
                                txt(px(12.))
                                    .text_color(gpui::rgba(0x888888FF))
                                    .child(t("skills.market_empty")),
                            )
                            .child(
                                Button::new("fetch-market")
                                    .cursor_pointer()
                                    .ghost()
                                    .label(t("skills.fetch_market"))
                                    .on_click(fetch_cb.clone()),
                            )
                            .into_any_element()
                    } else {
                        let filtered: Vec<&ApiSkill> = market_skills
                            .iter()
                            .filter(|s| {
                                skill_search.is_empty()
                                    || s.name.to_lowercase().contains(&skill_search.to_lowercase())
                                    || s.slug.to_lowercase().contains(&skill_search.to_lowercase())
                                    || s.source
                                        .to_lowercase()
                                        .contains(&skill_search.to_lowercase())
                            })
                            .collect();
                        let count = filtered.len();
                        v_flex()
                            .gap(px(12.))
                            .child(
                                h_flex()
                                    .gap(px(8.))
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        txt(px(10.))
                                            .text_color(gpui::rgba(0x666666FF))
                                            .child(format!("{} items", count)),
                                    )
                                    .child(
                                        Button::new("refresh-market")
                                            .cursor_pointer()
                                            .ghost()
                                            .compact()
                                            .label(t("general.refresh"))
                                            .on_click(fetch_cb),
                                    ),
                            )
                            .child(if filtered.is_empty() {
                                txt(px(12.))
                                    .text_color(gpui::rgba(0x888888FF))
                                    .child(t("skills.no_skills"))
                                    .into_any_element()
                            } else {
                                h_flex()
                                    .gap(px(8.))
                                    .flex_wrap()
                                    .min_w_0()
                                    .overflow_x_hidden()
                                    .children(filtered.iter().map(|s| {
                                        components::api_skill_card(s, &installed_names, on_skill_action)
                                    }))
                                    .into_any_element()
                            })
                            .into_any_element()
                    };
                v_flex()
                    .gap(px(8.))
                    .child(market_content)
                    .child(components::add_skill_card(on_skill_action))
                    .into_any_element()
            }
        })
        .when_some(skill_dialog_input, |el, inp| {
            el.when(*skill_dialog_mode == InputDialogMode::SkillAddGit, |el| {
                el.child(
                    v_flex()
                        .gap(px(8.))
                        .py(px(8.))
                        .px(px(10.))
                        .border_1()
                        .border_color(gpui::rgba(0x45475a66))
                        .rounded_md()
                        .child(txt(px(12.)).child(t("skills.dialog_git_url")))
                        .child(Input::new(inp)),
                )
            })
            .when(*skill_dialog_mode == InputDialogMode::SkillAddLocal, |el| {
                el.child(
                    v_flex()
                        .gap(px(8.))
                        .py(px(8.))
                        .px(px(10.))
                        .border_1()
                        .border_color(gpui::rgba(0x45475a66))
                        .rounded_md()
                        .child(txt(px(12.)).child(t("skills.dialog_local_path")))
                        .child(Input::new(inp)),
                )
            })
        })
}

// ── API Provider Content ──────────────────────────────────────────────

fn tab_api_content(
    providers: &[ApiProviderConfig],
    form: Option<&ApiProviderFormState>,
    available_agents: &[String],
    on_action: &SharedActionCb<ApiProviderAction>,
    api_test_results: &HashMap<String, String>,
    api_search: &str,
    api_search_input: Option<&Entity<gpui_component::input::InputState>>,
) -> impl IntoElement {
    let on_api = on_action.clone();

    let filtered: Vec<&ApiProviderConfig> = if api_search.is_empty() {
        providers.iter().collect()
    } else {
        let q = api_search.to_lowercase();
        providers
            .iter()
            .filter(|p| p.name.to_lowercase().contains(&q))
            .collect()
    };

    let add_cb = {
        let on_api = on_api.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on_api(ApiProviderAction::ShowAddForm, w, a);
        }
    };

    v_flex()
        .gap(px(12.))
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .justify_between()
                .child(setting_group_header(t("settings.api.title")))
                .child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .child(
                            txt(px(16.))
                                .text_color(gpui::rgba(0x888888FF))
                                .child("\u{1F50D}"),
                        )
                        .child(
                            api_search_input
                                .map(|e| Input::new(e).flex_grow(1.0).into_any_element())
                                .unwrap_or_else(|| {
                                    txt(px(11.))
                                        .text_color(gpui::rgba(0x888888FF))
                                        .child(t("settings.api.search"))
                                        .into_any_element()
                                }),
                        )
                        .child(
                            txt(px(10.))
                                .text_color(gpui::rgba(0x666666FF))
                                .child(format!("{} items", filtered.len())),
                        )
                        .child(
                            Button::new("add-api-provider")
                                .cursor_pointer()
                                .ghost()
                                .label(t("general.add"))
                                .on_click(add_cb),
                        ),
                ),
        )
        // Only show the add form if not editing (editing is inline in the card)
        .when_some(form.filter(|f| f.editing_name.is_none()), |el, f| {
            el.child(components::api_provider_form(f, on_action))
        })
        .child(if filtered.is_empty() && form.is_none() {
            txt(px(12.))
                .text_color(gpui::rgba(0x888888FF))
                .child(t("settings.api.no_providers"))
                .into_any_element()
        } else {
            v_flex()
                .gap(px(8.))
                .children(
                    filtered.iter().map(|p| {
                        components::api_provider_card(p, available_agents, on_action, api_test_results, form)
                    }),
                )
                .into_any_element()
        })
}
