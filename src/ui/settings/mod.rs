//! Resizable settings dialog with vertical tab navigation.

use std::collections::HashMap;
use std::rc::Rc;

use crate::agent::AgentCliInfo;
use crate::api_provider::{ApiFormat, ApiProviderConfig, ThinkingLevel};
use crate::i18n::t;
use crate::provider::ProviderRegistry;
use gpui::{
    AnyElement, ClickEvent, Entity, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    scroll::ScrollableElement as _,
    searchable_list::SearchableVec,
    select::SelectState,
    v_flex,
};

use super::ThemeCallback;

mod components;
mod helpers;
mod tabs;
use helpers::*;

// ── Settings backdrop element with hitbox ─────────────────────────────

/// Creates a settings dialog backdrop that covers the entire window,
/// intercepts mouse events, and shows an arrow cursor.
pub fn settings_backdrop(
    win_w: f32,
    win_h: f32,
    on_close: Rc<CloseCb>,
    dialog: AnyElement,
) -> impl IntoElement {
    div()
        .absolute()
        .top(px(0.))
        .left(px(0.))
        .w(px(win_w))
        .h(px(win_h))
        .bg(gpui::rgba(0x000000AA))
        .cursor_default()
        .on_mouse_down(gpui::MouseButton::Left, {
            let oc = on_close.clone();
            move |_: &gpui::MouseDownEvent, w: &mut Window, a: &mut gpui::App| {
                oc(w, a);
            }
        })
        .child(
            div()
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .w(px(win_w))
                .h(px(win_h))
                .flex()
                .items_center()
                .justify_center()
                .child(dialog),
        )
}
pub type ActionCb<T> = Box<dyn Fn(T, &mut Window, &mut gpui::App)>;
pub type SharedActionCb<T> = Rc<Box<dyn Fn(T, &mut Window, &mut gpui::App)>>;
pub type CloseCb = Box<dyn Fn(&mut Window, &mut gpui::App)>;

#[derive(Clone)]
pub enum SkillAction {
    Refresh,
    SetSubTab(SkillsSubTab),
    Link { skill_name: String },
    Unlink { skill_name: String },
    InstallGit { url: String },
    InstallLocal { path: String },
    Remove { skill_name: String },
    // Market (skills.sh API)
    FetchMarket,
    InstallApi { skill_id: String },
}

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
    TestServer { name: String },
    TestAgentServer { name: String, agent: String },
}

#[derive(Clone)]
pub enum ApiProviderAction {
    ShowAddForm,
    CancelForm,
    SubmitForm,
    SetFormat {
        format: ApiFormat,
    },
    SetThinkingLevel {
        level: ThinkingLevel,
    },
    SetSuffix {
        suffix: String,
    },
    ApplyPreset {
        preset: crate::api_provider::ProviderPreset,
    },
    Edit {
        name: String,
    },
    Remove {
        name: String,
    },
    FetchModels {
        name: String,
    },
    SelectModel {
        model: String,
    },
    ToggleAgent {
        name: String,
        agent: String,
    },
    TestConnection {
        name: String,
    },
}

#[derive(Clone)]
pub enum TerminalAction {
    SetCursorStyle(String),
    ToggleCursorBlink,
    SetOpacity(f32),
    SetScrollback(u32),
}

#[derive(Clone, PartialEq)]
pub enum InputDialogMode {
    Hidden,
    SkillAddGit,
    SkillAddLocal,
}

#[derive(Clone, Debug)]
pub enum PendingConfirm {
    RemoveMcp(String),
    RemoveSkill(String),
    RemoveApi(String),
}
impl PendingConfirm {
    pub fn message(&self) -> String {
        match self {
            PendingConfirm::RemoveMcp(name) => {
                t("settings.confirm.delete_mcp").replace("{name}", name)
            }
            PendingConfirm::RemoveSkill(name) => {
                t("settings.confirm.delete_skill").replace("{name}", name)
            }
            PendingConfirm::RemoveApi(name) => {
                t("settings.confirm.delete_api").replace("{name}", name)
            }
        }
    }
}

pub struct McpFormState {
    pub editing_name: Option<String>,
    pub name: Option<Entity<gpui_component::input::InputState>>,
    pub command: Option<Entity<gpui_component::input::InputState>>,
    pub args: Option<Entity<gpui_component::input::InputState>>,
    pub env: Option<Entity<gpui_component::input::InputState>>,
    pub url: Option<Entity<gpui_component::input::InputState>>,
    pub mcp_type: String,
    /// Original command before editing (preserved when switching to SSE mode)
    pub original_command: Option<String>,
}

pub struct ApiProviderFormState {
    pub editing_name: Option<String>,
    pub name: Option<Entity<gpui_component::input::InputState>>,
    pub base_url: Option<Entity<gpui_component::input::InputState>>,
    pub api_key: Option<Entity<gpui_component::input::InputState>>,
    pub url_suffix: Option<Entity<gpui_component::input::InputState>>,
    pub format: ApiFormat,
    pub thinking_level: ThinkingLevel,
    pub available_models: Vec<String>,
    pub selected_model: String,
    pub model_search: String,
}

/// Sub-tab for the Extensions tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExtensionSubTab {
    Api = 0,
    Mcp,
    Skills,
}

/// Sub-tab for the Skills tab within Extensions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkillsSubTab {
    Installed = 0,
    Market,
}

/// A brief toast notification shown temporarily.
#[derive(Clone, Debug)]
pub struct ToastMessage {
    pub text: String,
    pub msg_type: String, // "success" | "error"
}

/// State for the right-click context menu popup.
#[derive(Clone, Debug)]
pub struct ContextMenuState {
    pub x: f32,
    pub y: f32,
    pub agent_index: usize,
    pub agent_name: String,
}

const TAB_PANEL_W: f32 = 140.0;
const TAB_MIN_W: f32 = 480.0;
const TAB_MIN_H: f32 = 360.0;

pub fn skill_agent_names() -> Vec<String> {
    ProviderRegistry::builtin().skill_agent_names()
}

/// Bundled parameters for the settings dialog.
pub struct SettingsDialogProps<'a> {
    pub active_tab: usize,
    pub on_tab_change: ActionCb<usize>,
    pub on_extension_tab_change: ActionCb<ExtensionSubTab>,
    pub active_extension_subtab: ExtensionSubTab,
    pub on_close: CloseCb,
    pub font_size: f32,
    pub current_theme_name: &'a str,
    pub theme_buttons: Vec<AnyElement>,
    pub font_select: Option<&'a Entity<SelectState<SearchableVec<String>>>>,
    pub on_font_down: ThemeCallback,
    pub on_font_up: ThemeCallback,
    pub locale: &'a str,
    pub on_locale_change: ActionCb<String>,
    pub agents: &'a [AgentCliInfo],
    pub selected_agent: Option<usize>,
    pub on_agent_select: ActionCb<Option<usize>>,
    pub on_agent_refresh: ActionCb<()>,
    pub mcp_servers: &'a [crate::mcp::ZenixMcpServer],
    pub mcp_per_agent: &'a [(String, Vec<crate::mcp::McpServerEntry>)],
    pub mcp_form: Option<&'a McpFormState>,
    pub mcp_available_agents: &'a [String],
    pub skills: &'a HashMap<String, crate::skills::SkillInfo>,
    pub skill_dialog_input: Option<&'a Entity<gpui_component::input::InputState>>,
    pub skill_dialog_mode: &'a InputDialogMode,
    pub on_skill_action: ActionCb<SkillAction>,
    pub on_mcp_action: ActionCb<McpAction>,
    pub api_providers: &'a [ApiProviderConfig],
    pub api_form: Option<&'a ApiProviderFormState>,
    pub on_api_action: ActionCb<ApiProviderAction>,
    pub mcp_test_results: &'a HashMap<String, String>,
    pub api_test_results: &'a HashMap<String, String>,
    pub agent_search: &'a str,
    pub agent_search_input: Option<&'a Entity<gpui_component::input::InputState>>,
    pub on_agent_toggle_installed: ActionCb<bool>,
    pub agent_installed_only: bool,
    pub active_skills_subtab: SkillsSubTab,
    pub skill_search: &'a str,
    pub mcp_search: &'a str,
    pub mcp_search_input: Option<&'a Entity<gpui_component::input::InputState>>,
    pub api_search: &'a str,
    pub api_search_input: Option<&'a Entity<gpui_component::input::InputState>>,
    pub skill_search_input: Option<&'a Entity<gpui_component::input::InputState>>,
    pub market_skills: &'a Vec<crate::skills::ApiSkill>,
    pub market_loading: bool,
    pub market_error: Option<&'a str>,
    pub terminal_cursor_blink: bool,
    pub terminal_cursor_style: &'a str,
    pub terminal_opacity: f32,
    pub terminal_scrollback: u32,
    pub scrollback_input: Option<&'a Entity<gpui_component::input::InputState>>,
    pub on_terminal_action: ActionCb<TerminalAction>,
    pub win_w: f32,
    pub toast: Option<&'a ToastMessage>,
    pub context_menu: Option<&'a ContextMenuState>,
    pub on_context_menu_dismiss: ActionCb<()>,
    pub pending_confirm: Option<&'a PendingConfirm>,
    pub on_confirm_action: ActionCb<bool>,
    pub font_family: &'a str,
    pub win_h: f32,
    pub zenix_ai_config: &'a crate::ai::config::ZenixAIConfig,
    pub zenix_ai_providers: &'a [crate::api_provider::ApiProviderConfig],
    pub zenix_ai_thinking_levels: Vec<String>,
    pub on_zenixai_provider: SharedActionCb<String>,
    pub on_zenixai_model: SharedActionCb<String>,
    pub on_zenixai_thinking: SharedActionCb<String>,
}

pub fn settings_dialog(props: SettingsDialogProps) -> impl IntoElement {
    let SettingsDialogProps {
        active_tab,
        on_tab_change,
        on_extension_tab_change,
        active_extension_subtab,
        on_close,
        font_size,
        current_theme_name,
        theme_buttons,
        font_select,
        on_font_down,
        on_font_up,
        locale,
        on_locale_change,
        agents,
        selected_agent,
        on_agent_select,
        on_agent_refresh,
        mcp_servers: mcp_zenix_servers,
        mcp_per_agent,
        mcp_form,
        mcp_available_agents,
        skills,
        skill_dialog_input,
        skill_dialog_mode,
        on_skill_action,
        on_mcp_action,
        api_providers,
        api_form: api_provider_form,
        on_api_action,
        mcp_test_results,
        api_test_results,
        agent_search,
        agent_search_input,
        on_agent_toggle_installed,
        agent_installed_only,
        active_skills_subtab,
        skill_search,
        terminal_cursor_blink,
        terminal_cursor_style,
        terminal_opacity,
        mcp_search,
        mcp_search_input,
        api_search,
        api_search_input,
        skill_search_input,
        market_skills,
        market_loading,
        market_error,
        on_terminal_action,
        terminal_scrollback,
        scrollback_input,
        toast,
        context_menu,
        on_context_menu_dismiss,
        pending_confirm,
        on_confirm_action,
        font_family,
        win_w,
        win_h,
        zenix_ai_config,
        zenix_ai_providers,
        zenix_ai_thinking_levels,
        on_zenixai_provider,
        on_zenixai_model,
        on_zenixai_thinking,
    } = props;

    let dialog_w = (win_w * 0.75).max(TAB_MIN_W);
    let dialog_h = (win_h * 0.75).max(TAB_MIN_H);
    let ocmd = Rc::new(on_context_menu_dismiss);
    let active_tab_c = active_tab;
    let otc = Rc::new(on_tab_change);
    let osa = Rc::new(on_skill_action);
    let oma = Rc::new(on_mcp_action);
    let oaa = Rc::new(on_api_action);
    let oc = Rc::new(on_close);
    let oat = Rc::new(on_agent_refresh);
    let oca = Rc::new(on_confirm_action);
    let oet = Rc::new(on_extension_tab_change);
    let oati = Rc::new(on_agent_toggle_installed);
    let oas = Rc::new(on_agent_select);

    let ota = Rc::new(on_terminal_action);
    let tab_labels = [
        t("settings.tab.general"),
        t("settings.tab.appearance"),
        t("settings.tab.terminal"),
        t("settings.tab.agent_cli"),
        t("settings.tab.extensions"),
        t("settings.tab.zenix_ai"),
    ];

    let dialog = div()
        .id("settings-dialog")
        .w(px(dialog_w)).h(px(dialog_h))
        .bg(gpui::rgba(0x1e1e2eff))
        .border_1().border_color(gpui::rgba(0x45475aff))
        .rounded_lg().shadow_lg()
        .font_family(font_family)
        .flex().flex_col()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        // Header with close button
        .child(
            h_flex().flex_none().justify_between().items_center()
                .px(px(16.)).py(px(8.))
                .border_b_1().border_color(gpui::rgba(0x45475a33))
                .child(txt(px(14.)).font_weight(gpui::FontWeight::BOLD).child(t("settings.title")))
                .child(
                    div().id("close-btn").w(px(28.)).h(px(28.)).rounded_md()
                        .flex().items_center().justify_center()
                        .cursor_pointer()
                        .hover(|d| d.bg(gpui::rgba(0xFFFFFF14)))
                        .child(txt(px(14.)).text_color(gpui::rgba(0x888888FF)).child("\u{2715}"))
                        .on_click({
                            let oc = oc.clone();
                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                oc(w, a);
                            }
                        })
                )
        )
        // Left panel: tabs
        .child(
            h_flex().flex_none().h_full()
            .child(
                v_flex().flex_none().w(px(TAB_PANEL_W)).h_full().py_2()
                .children((0..tab_labels.len()).map(|i| {
                    let label = tab_labels[i].clone();
                    let otc = otc.clone();
                    let is_active = active_tab_c == i;
                    Button::new(format!("tab-{i}"))
                        .cursor_pointer()
                        .compact()
                        .label(label)
                        .when(is_active, |b| b.primary())
                        .when(!is_active, |b| b.ghost())
                        .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| otc(i, w, a))
                        .into_any_element()
                }))
            )
            // Body: content area
            .child(
                div().flex_grow(1.0).min_h_0().overflow_y_scrollbar().p(px(20.))
                .child(tabs::tab_content(active_tab, font_size, current_theme_name, theme_buttons,
                    on_font_down, on_font_up, locale, on_locale_change,
                    agents, selected_agent, &oas,
                    mcp_zenix_servers, mcp_per_agent, mcp_form,
                    mcp_available_agents, skills, skill_dialog_input, skill_dialog_mode,
                    &osa, &oma,
                    api_providers, api_provider_form, &oaa,
                    font_select,
                    active_extension_subtab, &oet,
                    mcp_test_results, api_test_results,
                    agent_search, &oat, &otc, agent_search_input, &oati, agent_installed_only,
                    active_skills_subtab, skill_search, skill_search_input,
                    mcp_search, mcp_search_input,
                    api_search, api_search_input,
                    terminal_cursor_blink, terminal_cursor_style,
                    terminal_opacity, terminal_scrollback, scrollback_input,
                    &ota, market_skills, market_loading, market_error,
                    zenix_ai_config, zenix_ai_providers,
                    zenix_ai_thinking_levels, &on_zenixai_provider,
                    &on_zenixai_model, &on_zenixai_thinking))
            )
        )
        // Toast overlay
        .when_some(toast, |el, t| {
            let bg = if t.msg_type == "error" { gpui::rgba(0xFF4444CC) } else { gpui::rgba(0x00DD44CC) };
            el.child(
                div().absolute().bottom(px(24.)).right(px(24.))
                    .px(px(16.)).py(px(10.))
                    .bg(bg).rounded_md().shadow_lg()
                    .child(txt(px(13.)).text_color(gpui::rgba(0xFFFFFFFF)).child(t.text.clone()))
            )
        })
        // Context menu
        .when_some(context_menu, |el, cm| {
            let ocmd = ocmd.clone();
            el.child(
                div().absolute().left(px(cm.x)).top(px(cm.y))
                    .px(px(12.)).py(px(8.))
                    .bg(gpui::rgba(0x2e2e3eff)).border_1().border_color(gpui::rgba(0x45475aff))
                    .rounded_md().shadow_lg()
                    .child(
                        v_flex().gap(px(4.)).min_w(px(140.))
                            .child(
                                div().px(px(8.)).py(px(4.)).cursor_pointer()
                                    .hover(|d| d.bg(gpui::rgba(0xFFFFFF14)))
                                    .child(txt(px(12.)).child(format!("Copy: {}", cm.agent_name)))
                                    .on_mouse_down(gpui::MouseButton::Left, {
                                        let ocmd = ocmd.clone();
                                        let _n = cm.agent_name.clone();
                                        move |_: &gpui::MouseDownEvent, _w: &mut Window, _a: &mut gpui::App| {
                                            ocmd((), _w, _a);
                                        }
                                    })
                            )
                    )
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| cx.stop_propagation())
            )
        })
        // Confirmation dialog
        .when_some(pending_confirm, |el, pc| {
            let oca = oca.clone();
            el.child(
                div().absolute().top_0().left_0().w_full().h_full()
                    .flex().items_center().justify_center()
                    .child(
                        div().px(px(24.)).py(px(20.)).bg(gpui::rgba(0x2e2e3eff))
                            .border_1().border_color(gpui::rgba(0x45475aff)).rounded_lg().shadow_xl()
                            .child(v_flex().gap(px(12.))
                                .child(txt(px(14.)).child(pc.message()))
                                .child(h_flex().gap(px(8.)).justify_end()
                                    .child(Button::new("confirm-yes")
                                        .cursor_pointer()
                                        .primary()
                                        .label(t("general.yes"))
                                        .on_click({
                                            let oca = oca.clone();
                                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| oca(true, w, a)
                                        }))
                                    .child(Button::new("confirm-no")
                                        .cursor_pointer()
                                        .ghost()
                                        .label(t("general.no"))
                                        .on_click({
                                            let oca = oca.clone();
                                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| oca(false, w, a)
                                        }))
                                )
                            )
                    )
            )
        })
        .into_any_element();

    settings_backdrop(win_w, win_h, oc, dialog)
}
