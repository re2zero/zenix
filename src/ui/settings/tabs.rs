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
use crate::ui::settings::styles::*;

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
    theme: &gpui_component::Theme,
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
    active_skills_subtab: SkillsSubTab,
    skill_search: &str,
    skill_search_input: Option<&Entity<gpui_component::input::InputState>>,
    terminal_cursor_blink: bool,
    terminal_cursor_style: &str,
    terminal_opacity: f32,
    terminal_scrollback: u32,
    scrollback_input: Option<&Entity<gpui_component::input::InputState>>,
    on_terminal_action: &SharedActionCb<TerminalAction>,
    market_skills: &[ApiSkill],
    market_loading: bool,
    market_error: Option<&str>,
) -> AnyElement {
    match active_tab {
        0 => tab_general(locale, on_locale_change, theme).into_any_element(),
        1 => tab_appearance(
            font_size,
            current_theme_name,
            theme_buttons,
            on_font_down,
            on_font_up,
            font_select,
            theme,
        )
        .into_any_element(),
        2 => tab_terminal(
            terminal_cursor_blink,
            terminal_cursor_style,
            terminal_opacity,
            terminal_scrollback,
            scrollback_input,
            on_terminal_action,
            theme,
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
            on_extension_tab_change,
            theme,
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
            api_providers,
            api_form,
            mcp_test_results,
            on_api_action,
            api_test_results,
            active_skills_subtab,
            market_skills,
            market_loading,
            market_error,
            theme,
        )
        .into_any_element(),
    }
}

// ── General Tab ───────────────────────────────────────────────────────

fn tab_general(
    locale: &str,
    on_locale_change: ActionCb<String>,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    let loc = locale.to_string();
    let cb = Rc::new(on_locale_change);
    let config_dir = crate::platform::zenix_config_dir();
    v_flex()
        .gap(gap_xl())
        .child(
            section_card(theme)
                .child(section_header().child(t("settings.general.language")))
                .child(
                    h_flex()
                        .gap(gap_sm())
                        .child(locale_btn("en", "English", &loc, cb.clone()))
                        .child(locale_btn("zh-CN", "\u{4e2d}\u{6587}", &loc, cb)),
                ),
        )
        .child(
            section_card(theme)
                .child(section_header().child(t("settings.about")))
                .child(txt(font_size_sm()).child(format!("zenix v{}", env!("CARGO_PKG_VERSION"))))
                .child(txt(font_size_sm()).text_color(text_color_muted()).child(
                    t("settings.config_path").replace("{path}", &config_dir.display().to_string()),
                )),
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
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    let mid = theme_buttons.len().div_ceil(2);
    let mut iter = theme_buttons.into_iter();
    let col1: Vec<_> = iter.by_ref().take(mid).collect();
    let col2: Vec<_> = iter.collect();
    v_flex()
        .gap(gap_xl())
        .child(
            section_card(theme)
                .child(section_header().child(t("settings.appearance.title")))
                .child(setting_label(t("settings.appearance.theme")))
                .child(
                    h_flex()
                        .gap(gap_md())
                        .child(v_flex().flex_grow(1.0).gap(gap_sm()).children(col1))
                        .child(v_flex().flex_grow(1.0).gap(gap_sm()).children(col2)),
                ),
        )
        .child(
            section_card(theme)
                .child(section_header().child(t("settings.appearance.font")))
                .child(setting_row(
                    t("settings.appearance.font"),
                    h_flex()
                        .gap(gap_md())
                        .items_center()
                        .child(
                            Button::new("f-dn")
                                .ghost()
                                .label("\u{2212}")
                                .text_size(font_size_md())
                                .on_click(on_font_down),
                        )
                        .child(
                            txt(font_size_sm())
                                .w(tab_icon_w())
                                .text_center()
                                .child(format!("{:.0}px", font_size)),
                        )
                        .child(
                            Button::new("f-up")
                                .ghost()
                                .label("+")
                                .text_size(font_size_md())
                                .on_click(on_font_up),
                        )
                        .child(
                            font_select
                                .map(|sel| Select::new(sel).w(form_field_w()).into_any_element())
                                .unwrap_or_else(|| {
                                    div()
                                        .w(form_field_w())
                                        .child(t("settings.appearance.change_font"))
                                        .into_any_element()
                                }),
                        ),
                )),
        )
}

// ── Terminal Tab ──────────────────────────────────────────────────────

fn tab_terminal(
    cursor_blink: bool,
    cursor_style: &str,
    opacity: f32,
    scrollback: u32,
    _scrollback_input: Option<&Entity<gpui_component::input::InputState>>,
    ota: &SharedActionCb<TerminalAction>,
    theme: &gpui_component::Theme,
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

    // Scrollback: readonly display with +/- buttons (long-press supported).
    let scrollback_display: AnyElement = txt(font_size_sm())
        .w(scrollback_display_w())
        .text_center()
        .child(format!("{scrollback}"))
        .into_any_element();

    v_flex().gap(gap_xl()).child(
        section_card(theme)
            .child(section_header().child(t("settings.terminal.title")))
            .child(setting_row(
                t("settings.terminal.cursor_style"),
                h_flex()
                    .gap(gap_md())
                    .child(style_btn("block", cursor_style))
                    .child(style_btn("bar", cursor_style))
                    .child(style_btn("underline", cursor_style)),
            ))
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
            .child(setting_row(
                t("settings.terminal.opacity"),
                h_flex()
                    .gap(gap_md())
                    .items_center()
                    .child(
                        Button::new("opacity-down")
                            .ghost()
                            .label("-")
                            .text_size(font_size_md())
                            .on_click({
                                let ota = ota.clone();
                                move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                    let pct =
                                        (opacity * 100.0 - 1.0).round().clamp(30.0, 100.0) as i32;
                                    let new_op = pct as f32 / 100.0;
                                    ota(TerminalAction::SetOpacity(new_op), w, a);
                                }
                            }),
                    )
                    .child(
                        txt(font_size_sm())
                            .w(tab_icon_w())
                            .text_center()
                            .child(format!("{:.0}%", (opacity * 100.0).round())),
                    )
                    .child(
                        Button::new("opacity-up")
                            .ghost()
                            .label("+")
                            .text_size(font_size_md())
                            .on_click({
                                let ota = ota.clone();
                                move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                    let pct =
                                        (opacity * 100.0 + 1.0).round().clamp(30.0, 100.0) as i32;
                                    let new_op = pct as f32 / 100.0;
                                    ota(TerminalAction::SetOpacity(new_op), w, a);
                                }
                            }),
                    ),
            ))
            .child(setting_row(
                t("settings.terminal.scrollback"),
                h_flex()
                    .gap(gap_sm())
                    .items_center()
                    .child(
                        Button::new("scrollback-down")
                            .ghost()
                            .label("-")
                            .text_size(font_size_md())
                            .on_click({
                                let ota = ota.clone();
                                move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                    let new_sb = (scrollback as i32 - 100).max(100) as u32;
                                    ota(TerminalAction::SetScrollback(new_sb), w, a);
                                }
                            }),
                    )
                    .child(scrollback_display)
                    .child(
                        Button::new("scrollback-up")
                            .ghost()
                            .label("+")
                            .text_size(font_size_md())
                            .on_click({
                                let ota = ota.clone();
                                move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                    let new_sb = (scrollback as i32 + 100).min(100000) as u32;
                                    ota(TerminalAction::SetScrollback(new_sb), w, a);
                                }
                            }),
                    ),
            )),
    )
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
    on_extension_tab_change: &SharedActionCb<ExtensionSubTab>,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    let filtered_agents: Vec<(usize, &AgentCliInfo)> = agents.iter().enumerate().collect();

    // Re-map selected_idx through filtered list
    let _display_selected = selected_idx.and_then(|sid| {
        filtered_agents
            .iter()
            .position(|(orig_i, _)| *orig_i == sid)
    });

    v_flex().gap(gap_md()).h_full()
        .child(h_flex().gap(gap_md()).items_center().justify_between()
            .child(setting_group_header(t("settings.tab.agent_cli")))
        )
        .child(
            h_flex().flex_grow(1.0).min_h_0().gap(gap_md())
                .child(
                    v_flex().flex_none().w(sidebar_w()).h_full().gap_0()
                        .bg(bg_secondary_theme(theme))
                        .border_1().border_color(border_color_transparent())
                        .rounded(border_radius_md())
                        .child(
                            v_flex().gap_0()
                                .child(
                                    h_flex().gap(gap_sm()).px(pad_md()).py(pad_md()).items_center()
                                 )
                                 .child(
                                     h_flex().gap(gap_sm()).px(pad_md()).py(pad_sm())
                                         .border_b_1().border_color(border_color_transparent())
                                         .child(txt(font_size_xs()).text_color(text_color_muted()).child(format!("{} / {}", filtered_agents.len(), agents.len())))
                                 )
                                .child(
                                    div().overflow_y_scrollbar().child(
                                        v_flex().gap_0()
                                            .children(filtered_agents.iter().map(|(orig_i, info)| {
                                                let sel = selected_idx == Some(*orig_i);
                                                let oas = on_agent_select.clone();
                                                let indicator = if info.detected { "\u{25CF}" } else { "\u{25CB}" };
                                                let ind_color = if info.detected { success_color() } else { text_color_muted() };
                                                let orig_idx = *orig_i;
                                                 div().id(format!("agent-{}", info.name)).w_full()
                                                     .when(sel, |d| d.bg(bg_selected_theme(theme)))
                                                     .hover(|d| d.bg(bg_hover_theme(theme)))
                                                     .cursor_pointer()
                                                    .px(pad_md()).py(pad_sm())
                                                    .child(
                                                        h_flex().gap(gap_sm()).items_center()
                                                            .child(txt(font_size_xs()).text_color(ind_color).child(indicator))
                                                            .child(txt(font_size_sm()).child(info.display_name.to_string()))
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
                                None => txt(font_size_sm()).text_color(text_color_very_muted())
                                    .child(t("settings.agent_cli.no_agent_selected"))
                                    .into_any_element(),
                                Some(agent) => agent_detail_panel(
                                    agent, mcp_per_agent, skills, api_providers,
                                    mcp_available_agents, on_mcp_action, on_skill_action,
                                    mcp_test_results, on_extension_tab_change,
                                    theme,
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
    on_extension_tab_change: &SharedActionCb<ExtensionSubTab>,
    theme: &gpui_component::Theme,
) -> AnyElement {
    let agent_name = agent.name;
    let status_text = if agent.detected {
        t("agent.installed")
    } else {
        t("agent.not_installed")
    };
    let status_color = if agent.detected {
        success_color()
    } else {
        error_color()
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
        .gap(gap_lg())
        .child(
            v_flex()
                .gap(gap_sm())
                .child(
                    h_flex()
                        .gap(gap_md())
                        .items_center()
                        .child(
                            txt(font_size_lg())
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(agent.display_name.to_string()),
                        )
                        .child(
                            txt(font_size_base())
                                .text_color(status_color)
                                .child(status_text),
                        ),
                )
                .when_some(agent.binary_path.as_ref(), |el, path| {
                    el.child(
                        txt(font_size_base())
                            .text_color(text_color_muted())
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
                    .gap(gap_sm())
                    .py(pad_md())
                    .px(pad_md())
                    .border_1()
                    .border_color(border_color_active())
                    .rounded(border_radius_md())
                    .child(txt(font_size_sm()).child(t("agent.install")))
                    .child(
                        div()
                            .px(pad_sm())
                            .py(pad_sm())
                            .bg(card_bg_transparent())
                            .rounded(border_radius_sm())
                            .child(
                                txt(font_size_sm())
                                    .font_family("monospace")
                                    .child(agent.install_command.to_string()),
                            ),
                    ),
            )
        })
        .child(
            v_flex()
                .gap(gap_md())
                .child(section_header().child(t("settings.api.title")))
                .child(if agent_providers.is_empty() {
                    txt(font_size_sm())
                        .text_color(text_color_very_muted())
                        .child(t("settings.no_api_providers"))
                        .into_any_element()
                } else {
                    v_flex()
                        .gap(gap_sm())
                        .children(agent_providers.iter().map(|p| {
                            let fmt_color = match p.format {
                                crate::api_provider::ApiFormat::OpenAI => badge_blue(),
                                crate::api_provider::ApiFormat::Anthropic => badge_orange(),
                            };
                            h_flex()
                                .gap(gap_md())
                                .py(pad_sm())
                                .px(pad_md())
                                .border_1()
                                .border_color(border_color_transparent_2())
                                .rounded(border_radius_sm())
                                .child(
                                    txt(font_size_sm())
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(p.name.clone()),
                                )
                                .child(
                                    txt(font_size_sm())
                                        .text_color(fmt_color)
                                        .child(p.format.label().to_string()),
                                )
                        }))
                        .into_any_element()
                }),
        )
        .child(
            v_flex()
                .gap(gap_md())
                .child(section_header().child(t("settings.mcp.title")))
                .child(if mcp_entries.is_empty() {
                    txt(font_size_sm())
                        .text_color(text_color_very_muted())
                        .child(t("mcp.no_servers"))
                        .into_any_element()
                } else {
                    v_flex()
                        .gap(gap_sm())
                        .children(mcp_entries.iter().map(|entry| {
                            components::mcp_agent_entry_with_test(
                                agent_name,
                                entry,
                                on_mcp_action,
                                mcp_test_results,
                                theme,
                            )
                        }))
                        .into_any_element()
                }),
        )
        .child(
            div()
                .cursor_pointer()
                .child(
                    txt(font_size_base())
                        .text_color(badge_blue())
                        .child(t("settings.agent_cli.add_mcp_extensions")),
                )
                .on_mouse_down(gpui::MouseButton::Left, {
                    let oetc = on_extension_tab_change.clone();
                    move |_: &gpui::MouseDownEvent, w: &mut Window, a: &mut gpui::App| {
                        oetc(ExtensionSubTab::Mcp, w, a);
                    }
                }),
        )
        .child(
            v_flex()
                .gap(gap_md())
                .child(section_header().child(t("settings.tab.skills")))
                .child(if agent_skills.is_empty() {
                    txt(font_size_sm())
                        .text_color(text_color_very_muted())
                        .child(t("skills.no_skills"))
                        .into_any_element()
                } else {
                    h_flex()
                        .gap(gap_sm())
                        .flex_wrap()
                        .children(agent_skills.iter().map(|s| {
                            components::per_agent_skill_card(agent_name, s, on_skill_action)
                        }))
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
    api_providers: &[ApiProviderConfig],
    api_form: Option<&ApiProviderFormState>,
    mcp_test_results: &HashMap<String, String>,
    on_api_action: &SharedActionCb<ApiProviderAction>,
    api_test_results: &HashMap<String, String>,
    active_skills_subtab: SkillsSubTab,
    market_skills: &[ApiSkill],
    market_loading: bool,
    market_error: Option<&str>,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    let otc = on_sub_tab_change.clone();
    let labels: Vec<(String, ExtensionSubTab)> = vec![
        (t("settings.tab.api"), ExtensionSubTab::Api),
        (t("settings.tab.mcp"), ExtensionSubTab::Mcp),
        (t("settings.tab.skills"), ExtensionSubTab::Skills),
        ("Roles".to_string(), ExtensionSubTab::Roles),
    ];
    v_flex()
        .gap(gap_lg())
        .child(extension_tab_nav(
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
            theme,
        ))
        .child(match active_sub {
            ExtensionSubTab::Api => tab_api_content(
                api_providers,
                api_form,
                mcp_available_agents,
                on_api_action,
                api_test_results,
            )
            .into_any_element(),
            ExtensionSubTab::Mcp => tab_mcp_content(
                mcp_servers,
                mcp_per_agent,
                mcp_form,
                mcp_available_agents,
                on_mcp_action,
                mcp_test_results,
                theme,
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
                theme,
            )
            .into_any_element(),
            ExtensionSubTab::Roles => tab_roles_content(theme).into_any_element(),
        })
}

// ── MCP Content ───────────────────────────────────────────────────────

fn tab_mcp_content(
    zenix_servers: &[crate::mcp::ZenixMcpServer],
    per_agent: &[(String, Vec<crate::mcp::McpServerEntry>)],
    mcp_form: Option<&McpFormState>,
    available_agents: &[String],
    on_mcp_action: &SharedActionCb<McpAction>,
    mcp_test_results: &HashMap<String, String>,
    theme: &gpui_component::Theme,
) -> impl IntoElement {
    let add_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::ShowAddForm, w, a);
        }
    };

    // No filtering needed for MCP servers
    let filtered_servers: Vec<&crate::mcp::ZenixMcpServer> = zenix_servers.iter().collect();

    // Only show the form if we're adding a new server (not editing an existing one)
    // When editing, the card itself renders the inline edit form
    let form_el = mcp_form
        .filter(|f| f.editing_name.is_none())
        .map(|form| components::mcp_add_form(form, on_mcp_action));

    v_flex()
        .gap(gap_lg())
        .child(
            h_flex()
                .gap(gap_md())
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
        .when_some(form_el, |el, form| el.child(form))
        .child(if filtered_servers.is_empty() && mcp_form.is_none() {
            txt(font_size_sm())
                .text_color(text_color_very_muted())
                .child(t("settings.mcp.no_zenix_servers"))
                .into_any_element()
        } else {
            v_flex()
                .gap(gap_md())
                .children(filtered_servers.iter().map(|s| {
                    components::mcp_card(
                        s,
                        available_agents,
                        on_mcp_action,
                        mcp_form,
                        mcp_test_results,
                        theme,
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
    theme: &gpui_component::Theme,
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
        .gap(gap_lg())
        .child(
            h_flex()
                .gap(gap_md())
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
                    theme,
                ))
                .child(
                    Button::new("refresh-s")
                        .cursor_pointer()
                        .ghost()
                        .label(t("general.refresh"))
                        .on_click(refresh_cb),
                ),
        )
        .child(zed_search_bar(
            &t("skills.search"),
            skill_search_input,
            theme,
        ))
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
                    txt(font_size_sm())
                        .text_color(text_color_very_muted())
                        .child(t("skills.no_installed"))
                        .into_any_element()
                } else {
                    h_flex()
                        .gap(gap_sm())
                        .flex_wrap()
                        .min_w_0()
                        .overflow_x_hidden()
                        .children(
                            filtered
                                .iter()
                                .map(|s| components::zenix_skill_card(s, on_skill_action, theme)),
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

                let market_content = if market_loading {
                    txt(font_size_sm())
                        .text_color(text_color_muted())
                        .child(t("skills.market_loading"))
                        .into_any_element()
                } else if let Some(err) = market_error {
                    txt(font_size_sm())
                        .text_color(error_color())
                        .child(err.to_string())
                        .into_any_element()
                } else if market_skills.is_empty() {
                    v_flex()
                        .gap(gap_sm())
                        .child(
                            txt(font_size_sm())
                                .text_color(text_color_muted())
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
                        .gap(gap_lg())
                        .child(
                            h_flex()
                                .gap(gap_md())
                                .items_center()
                                .justify_between()
                                .child(
                                    txt(font_size_sm())
                                        .text_color(text_color_very_muted())
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
                            txt(font_size_sm())
                                .text_color(text_color_very_muted())
                                .child(t("skills.no_skills"))
                                .into_any_element()
                        } else {
                            h_flex()
                                .gap(gap_sm())
                                .flex_wrap()
                                .min_w_0()
                                .overflow_x_hidden()
                                .children(filtered.iter().map(|s| {
                                    components::api_skill_card(
                                        s,
                                        &installed_names,
                                        on_skill_action,
                                        theme,
                                    )
                                }))
                                .into_any_element()
                        })
                        .into_any_element()
                };
                v_flex()
                    .gap(gap_md())
                    .child(market_content)
                    .child(components::add_skill_card(on_skill_action))
                    .into_any_element()
            }
        })
        .when_some(skill_dialog_input, |el, inp| {
            el.when(*skill_dialog_mode == InputDialogMode::SkillAddGit, |el| {
                el.child(
                    v_flex()
                        .gap(gap_md())
                        .py(pad_md())
                        .px(pad_xl())
                        .border_1()
                        .border_color(border_color_transparent_2())
                        .rounded(border_radius_md())
                        .child(txt(font_size_sm()).child(t("skills.dialog_git_url")))
                        .child(Input::new(inp)),
                )
            })
            .when(*skill_dialog_mode == InputDialogMode::SkillAddLocal, |el| {
                el.child(
                    v_flex()
                        .gap(gap_md())
                        .py(pad_md())
                        .px(pad_xl())
                        .border_1()
                        .border_color(border_color_transparent_2())
                        .rounded(border_radius_md())
                        .child(txt(font_size_sm()).child(t("skills.dialog_local_path")))
                        .child(Input::new(inp)),
                )
            })
        })
}

// ── API Provider Content ──────────────────────────────────────────────

// ── Roles Content ─────────────────────────────────────────────────────

fn tab_roles_content(theme: &gpui_component::Theme) -> impl IntoElement {
    let roles = crate::ai::roles::RoleRegistry::list_roles().unwrap_or_default();

    v_flex()
        .gap(gap_lg())
        .child(setting_group_header("角色管理"))
        .child(if roles.is_empty() {
            txt(font_size_sm())
                .text_color(text_color_very_muted())
                .child("暂无已注册的角色")
                .into_any_element()
        } else {
            v_flex()
                .gap(gap_md())
                .children(roles.iter().map(|role| {
                    let src_color = if role.source == "builtin" {
                        badge_blue()
                    } else {
                        badge_orange()
                    };
                    let src_label = if role.source == "builtin" {
                        "内置"
                    } else {
                        "用户"
                    };

                    div()
                        .px(pad_md())
                        .py(pad_md())
                        .border_1()
                        .border_color(border_color_transparent_2())
                        .rounded(border_radius_md())
                        .child(
                            v_flex()
                                .gap(gap_sm())
                                .child(
                                    h_flex()
                                        .gap(gap_md())
                                        .items_center()
                                        .child(
                                            txt(font_size_base())
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .child(role.name.clone()),
                                        )
                                        .child(
                                            txt(font_size_xs())
                                                .text_color(src_color)
                                                .child(src_label),
                                        )
                                        .child(
                                            txt(font_size_xs())
                                                .text_color(text_color_muted())
                                                .child(format!("({})", role.id)),
                                        ),
                                )
                                .child(
                                    txt(font_size_sm())
                                        .text_color(text_color_muted())
                                        .child(role.description.clone()),
                                )
                                .child(h_flex().gap(gap_sm()).children(role.tags.iter().map(
                                    |tag| {
                                        div()
                                            .px(pad_sm())
                                            .py(px(2.))
                                            .bg(card_bg_transparent())
                                            .rounded(border_radius_sm())
                                            .child(
                                                txt(font_size_xs())
                                                    .text_color(text_color_muted())
                                                    .child(tag.clone()),
                                            )
                                            .into_any_element()
                                    },
                                )))
                                .child(
                                    h_flex().gap(gap_sm()).child(
                                        txt(font_size_xs())
                                            .text_color(text_color_very_muted())
                                            .child(format!(
                                                "推荐 CLI: {}",
                                                role.cli_suggestion.join(", ")
                                            )),
                                    ),
                                ),
                        )
                        .into_any_element()
                }))
                .into_any_element()
        })
}

// ── API Provider Content ──────────────────────────────────────────────

fn tab_api_content(
    providers: &[ApiProviderConfig],
    form: Option<&ApiProviderFormState>,
    available_agents: &[String],
    on_action: &SharedActionCb<ApiProviderAction>,
    api_test_results: &HashMap<String, String>,
) -> impl IntoElement {
    let on_api = on_action.clone();

    // No filtering needed for API providers
    let filtered: Vec<&ApiProviderConfig> = providers.iter().collect();

    let add_cb = {
        let on_api = on_api.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on_api(ApiProviderAction::ShowAddForm, w, a);
        }
    };

    v_flex()
        .gap(gap_lg())
        .child(
            h_flex()
                .gap(gap_md())
                .items_center()
                .justify_between()
                .child(setting_group_header(t("settings.api.title")))
                .child(
                    Button::new("add-api-provider")
                        .cursor_pointer()
                        .ghost()
                        .label(t("general.add"))
                        .on_click(add_cb),
                ),
        )
        // Only show the add form if not editing (editing is inline in the card)
        .when_some(form.filter(|f| f.editing_name.is_none()), |el, f| {
            el.child(components::api_provider_form(f, on_action))
        })
        .child(if filtered.is_empty() && form.is_none() {
            txt(font_size_sm())
                .text_color(text_color_very_muted())
                .child(t("settings.api.no_providers"))
                .into_any_element()
        } else {
            v_flex()
                .gap(gap_md())
                .children(filtered.iter().map(|p| {
                    components::api_provider_card(
                        p,
                        available_agents,
                        on_action,
                        api_test_results,
                        form,
                    )
                }))
                .into_any_element()
        })
}
