//! Resizable settings dialog with vertical tab navigation.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    div, px, relative, AnyElement, ClickEvent, Entity, InteractiveElement as _, IntoElement,
    ParentElement, StatefulInteractiveElement as _, Styled, Window, prelude::FluentBuilder as _,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex, scroll::ScrollableElement as _, v_flex,
    input::Input,
    select::{Select, SelectState},
    searchable_list::SearchableVec,
};
use crate::agent::AgentCliInfo;
use crate::api_provider::{ApiFormat, ApiProviderConfig, ThinkingLevel};
use crate::i18n::t;
use crate::provider::ProviderRegistry;

use super::ThemeCallback;

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
}

#[derive(Clone)]
pub enum ApiProviderAction {
    ShowAddForm,
    CancelForm,
    SubmitForm,
    SetFormat { format: ApiFormat },
    SetThinkingLevel { level: ThinkingLevel },
    SetSuffix { suffix: String },
    ApplyPreset { preset: crate::api_provider::ProviderPreset },
    Edit { name: String },
    Remove { name: String },
    FetchModels { name: String },
    SelectModel { model: String },
    ToggleAgent { name: String, agent: String },
    TestConnection { name: String },
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
            PendingConfirm::RemoveMcp(name) => t("settings.confirm.delete_mcp").replace("{name}", name),
            PendingConfirm::RemoveSkill(name) => t("settings.confirm.delete_skill").replace("{name}", name),
            PendingConfirm::RemoveApi(name) => t("settings.confirm.delete_api").replace("{name}", name),
        }
    }
}

pub struct McpFormState {
    pub editing_name: Option<String>,
    pub name: Option<Entity<gpui_component::input::InputState>>,
    pub command: Option<Entity<gpui_component::input::InputState>>,
    pub args: Option<Entity<gpui_component::input::InputState>>,
    pub env: Option<Entity<gpui_component::input::InputState>>,
    pub mcp_type: String,
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
    pub terminal_cursor_blink: bool,
    pub terminal_cursor_style: &'a str,
    pub terminal_opacity: f32,
    pub terminal_scrollback: u32,
    pub on_terminal_action: ActionCb<TerminalAction>,
    pub win_w: f32,
    pub toast: Option<&'a ToastMessage>,
    pub context_menu: Option<&'a ContextMenuState>,
    pub on_context_menu_dismiss: ActionCb<()>,
    pub pending_confirm: Option<&'a PendingConfirm>,
    pub on_confirm_action: ActionCb<bool>,
    pub win_h: f32,
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
        on_terminal_action,
        terminal_scrollback,
        toast,
        context_menu,
        on_context_menu_dismiss,
        pending_confirm,
        on_confirm_action,
        win_w,
        win_h,
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
    ];

    div()
        .id("settings-backdrop")
        .absolute()
        .top_0().left_0()
        .w(px(win_w)).h(px(win_h))
        .bg(gpui::rgba(0x000000AA))
        .flex().items_center().justify_center()
        .on_mouse_down(gpui::MouseButton::Left, |_: &gpui::MouseDownEvent, _: &mut Window, _: &mut gpui::App| {})
        .on_mouse_down(gpui::MouseButton::Right, {
            let ocmd = ocmd.clone();
            move |_: &gpui::MouseDownEvent, w: &mut Window, a: &mut gpui::App| {
                // Only dismiss if context menu is open — check not needed here, callback handles it
                ocmd((), w, a);
            }
        })
        .on_key_down({
            let oc = oc.clone();
            move |event: &gpui::KeyDownEvent, w: &mut Window, a: &mut gpui::App| {
                if event.keystroke.key == "escape" {
                    oc(w, a);
                }
            }
        })
        .on_click({
            let oc = oc.clone();
            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { oc(w, a); }
        })
        .child(
            div()
                .id("settings-dialog")
                .w(px(dialog_w)).h(px(dialog_h))
                .bg(gpui::rgba(0x1e1e2eff))
                .border_1().border_color(gpui::rgba(0x45475aff))
                .rounded_lg().shadow_lg()
                .flex().flex_col()
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
                    h_flex().flex_grow(1.0).min_h_0().overflow_hidden().gap(px(12.))
                        .child(tab_panel(&otc, active_tab_c, &tab_labels))
                        .child(
                            div().flex_grow(1.0).h_full().overflow_y_scrollbar().p(px(20.))
                                .child(tab_content(active_tab, font_size, current_theme_name, theme_buttons,
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
                                    terminal_opacity, terminal_scrollback,
                                    &ota))
                        )
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
                                    .child(Button::new("confirm-yes").primary().label(t("general.yes"))
                                        .on_click({
                                            let oca = oca.clone();
                                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| oca(true, w, a)
                                        }))
                                    .child(Button::new("confirm-no").ghost().label(t("general.no"))
                                        .on_click({
                                            let oca = oca.clone();
                                            move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| oca(false, w, a)
                                        }))
                                )
                            )
                    )
            )
        })
}
fn tab_panel(otc: &SharedActionCb<usize>, active: usize, labels: &[String]) -> impl IntoElement {
    v_flex().flex_none().w(px(TAB_PANEL_W)).h_full().py_2()
        .children((0..labels.len()).map(|i| {
            let label = labels[i].clone();
            let otc = otc.clone();
            let sel = active == i;
            let variant = if sel { "primary" } else { "ghost" };
            let el = Button::new(format!("tab-{i}")).compact().label(label);
            if variant == "primary" { el.primary() } else { el.ghost() }
                .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| otc(i, w, a))
                .into_any_element()
        }))
}
fn tab_content(
    active_tab: usize, font_size: f32, current_theme_name: &str,
    theme_buttons: Vec<AnyElement>, on_font_down: ThemeCallback, on_font_up: ThemeCallback,
    locale: &str, on_locale_change: ActionCb<String>,
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
) -> AnyElement {
    match active_tab {
        0 => tab_general(locale, on_locale_change).into_any_element(),
        1 => tab_appearance(font_size, current_theme_name, theme_buttons, on_font_down, on_font_up, font_select).into_any_element(),
        2 => tab_terminal(terminal_cursor_blink, terminal_cursor_style, terminal_opacity, terminal_scrollback, on_terminal_action).into_any_element(),
        3 => tab_agent_cli(agents, selected_agent, on_agent_select,
            mcp_per_agent, skills, api_providers, mcp_available_agents,
            on_mcp_action, on_skill_action,
            mcp_test_results, agent_search, agent_search_input, on_agent_toggle_installed, agent_installed_only,
            on_agent_refresh, on_tab_change, on_extension_tab_change).into_any_element(),
        _ => tab_extensions(
            active_extension_subtab, on_extension_tab_change,
            mcp_zenix_servers, mcp_per_agent, mcp_form, mcp_available_agents, on_mcp_action,
            skills, skill_dialog_input,
            skill_search, skill_search_input,
            skill_dialog_mode, on_skill_action,
            mcp_search, mcp_search_input,
            api_search, api_search_input,
            api_providers, api_form, mcp_test_results, on_api_action, api_test_results, active_skills_subtab).into_any_element(),
    }
}

// ── General ─────────────────────────────────────────────────────────

fn tab_general(locale: &str, on_locale_change: ActionCb<String>) -> impl IntoElement {
    let loc = locale.to_string();
    let cb = Rc::new(on_locale_change);
    v_flex().gap(px(12.))
        .child(setting_group_header(t("settings.general.language")))
        .child(h_flex().gap(px(4.))
            .child(locale_btn("en", "English", &loc, cb.clone()))
            .child(locale_btn("zh-CN", "\u{4e2d}\u{6587}", &loc, cb)))
}

fn locale_btn(code: &str, label: &str, current: &str, on_change: SharedActionCb<String>) -> impl IntoElement {
    let is_current = code == current;
    let code_s = code.to_string();
    Button::new(format!("locale-{code}"))
        .when(is_current, |b| b.primary()).when(!is_current, |b| b.ghost())
        .label(label.to_string())
        .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { on_change(code_s.clone(), w, a); })
}

// ── Appearance ───────────────────────────────────────────────────────

fn tab_appearance(
    font_size: f32, _current_theme_name: &str, theme_buttons: Vec<AnyElement>,
    on_font_down: ThemeCallback, on_font_up: ThemeCallback,
    font_select: Option<&Entity<SelectState<SearchableVec<String>>>>,
) -> impl IntoElement {
    let mid = theme_buttons.len().div_ceil(2);
    let mut iter = theme_buttons.into_iter();
    let col1: Vec<_> = iter.by_ref().take(mid).collect();
    let col2: Vec<_> = iter.collect();
    v_flex().gap(px(16.))
        .child(setting_group_header(t("settings.appearance.title")))
        .child(setting_label(t("settings.appearance.theme")))
        .child(h_flex().gap(px(8.))
            .child(v_flex().flex_grow(1.0).gap(px(4.)).children(col1))
            .child(v_flex().flex_grow(1.0).gap(px(4.)).children(col2)))
        .child(setting_row(t("settings.appearance.font"),
            h_flex().gap(px(8.)).items_center()
                .child(Button::new("f-dn").ghost().label("\u{2212}").text_size(px(18.)).on_click(on_font_down))
                .child(txt(px(15.)).w(px(44.)).text_center().child(format!("{:.0}px", font_size)))
                .child(Button::new("f-up").ghost().label("+").text_size(px(18.)).on_click(on_font_up))
                .child(
                    font_select.map(|sel| Select::new(sel).w(px(220.)).into_any_element())
                        .unwrap_or_else(|| div().w(px(220.)).child(t("settings.appearance.change_font")).into_any_element())
                )))
}
fn tab_terminal(cursor_blink: bool, cursor_style: &str, opacity: f32, scrollback: u32, ota: &SharedActionCb<TerminalAction>) -> impl IntoElement {
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
            .when(sel, |b| b.primary()).when(!sel, |b| b.ghost())
            .compact()
            .label(label.to_string())
            .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| ota(TerminalAction::SetCursorStyle(s.clone()), w, a))
    };

    let blink_label = if cursor_blink { "\u{2611} on" } else { "\u{2610} off" };
    let ota_blink = ota.clone();

    v_flex().gap(px(16.))
        .child(setting_group_header(t("settings.terminal.title")))
        .child(setting_label(t("settings.terminal.cursor_style")))
        .child(h_flex().gap(px(8.))
            .child(style_btn("block", cursor_style))
            .child(style_btn("bar", cursor_style))
            .child(style_btn("underline", cursor_style)))
        .child(setting_row(t("settings.terminal.cursor_blink"),
            Button::new("cursor-blink")
                .when(cursor_blink, |b| b.primary()).when(!cursor_blink, |b| b.ghost())
                .label(blink_label)
                .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| ota_blink(TerminalAction::ToggleCursorBlink, w, a))
        ))
        .child(setting_label(t("settings.terminal.opacity")))
        .child(
            h_flex().gap(px(8.)).items_center()
                .child(Button::new("opacity-down").ghost().label("-").text_size(px(18.))
                    .on_click({
                        let ota = ota.clone();
                        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            let new_op = (opacity * 10.0 - 1.0).clamp(0.5, 10.0) / 10.0;
                            ota(TerminalAction::SetOpacity(new_op), w, a);
                        }
                    }))
                .child(txt(px(12.)).w(px(40.)).child(format!("{:.0}%", opacity * 100.0)))
                .child(Button::new("opacity-up").ghost().label("+").text_size(px(18.))
                    .on_click({
                        let ota = ota.clone();
                        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            let new_op = (opacity * 10.0 + 1.0).clamp(0.5, 10.0) / 10.0;
                            ota(TerminalAction::SetOpacity(new_op), w, a);
                        }
                    }))
        )
        .child(setting_row(t("settings.terminal.scrollback"),
            h_flex().gap(px(4.)).items_center()
                .child(Button::new("scrollback-down").ghost().label("-").text_size(px(18.))
                    .on_click({
                        let ota = ota.clone();
                        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            let new_sb = (scrollback as i32 - 100).max(100) as u32;
                            ota(TerminalAction::SetScrollback(new_sb), w, a);
                        }
                    }))
                .child(txt(px(12.)).w(px(60.)).child(format!("{scrollback}")))
                .child(Button::new("scrollback-up").ghost().label("+").text_size(px(18.))
                    .on_click({
                        let ota = ota.clone();
                        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            let new_sb = (scrollback as i32 + 100).max(100) as u32;
                            ota(TerminalAction::SetScrollback(new_sb), w, a);
                        }
                    }))
                .child(txt(px(10.)).text_color(gpui::rgba(0x666666FF)).child(t("settings.terminal.lines")))
        ))
}

// ── Agent CLI Master/Detail ───────────────────────────────────────────

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
    let filtered_agents: Vec<(usize, &AgentCliInfo)> = agents.iter().enumerate()
        .filter(|(_, info)| {
            let matches_search = agent_search.is_empty()
                || info.name.contains(agent_search)
                || info.display_name.to_lowercase().contains(&agent_search.to_lowercase());
            let matches_installed = !agent_installed_only || info.detected;
            matches_search && matches_installed
        })
        .collect();
    let _has_filter = !agent_search.is_empty() || agent_installed_only;

    // Re-map selected_idx through filtered list
    let _display_selected = selected_idx.and_then(|sid| {
        filtered_agents.iter().position(|(orig_i, _)| *orig_i == sid)
    });

    let installed_toggle_cb = {
        let oati = on_agent_toggle_installed.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { oati(!agent_installed_only, w, a); }
    };

    let installed_label = format!("{} {}", if agent_installed_only { "\u{2611}" } else { "\u{2610}" }, t("settings.agent_cli.installed_only"));

    v_flex().gap(px(8.)).h_full()
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(setting_group_header(t("settings.tab.agent_cli")))
            .child(Button::new("agent-refresh").ghost().label(t("settings.agent_cli.refresh")).on_click({
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
                                            .when(agent_installed_only, |b| b.primary()).when(!agent_installed_only, |b| b.ghost())
                                            .compact().label(installed_label)
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
    let status_text = if agent.detected { t("agent.installed") } else { t("agent.not_installed") };
    let status_color = if agent.detected { gpui::rgba(0x00DD44FF) } else { gpui::rgba(0xFF4444FF) };

    let mcp_entries: Vec<&crate::mcp::McpServerEntry> = mcp_per_agent.iter()
        .find(|(name, _)| name == agent_name)
        .map(|(_, entries)| entries.iter().collect())
        .unwrap_or_default();

    let agent_skills: Vec<&crate::skills::SkillInfo> = skills.values()
        .filter(|s| s.source_agents.contains(&agent_name.to_string()) || s.linked_agents.contains(&agent_name.to_string()))
        .collect();

    let agent_providers: Vec<&ApiProviderConfig> = api_providers.iter()
        .filter(|p| p.enabled_agents.contains(&agent_name.to_string()))
        .collect();

    v_flex().gap(px(16.))
        .child(v_flex().gap(px(4.))
            .child(h_flex().gap(px(8.)).items_center()
                .child(txt(px(15.)).font_weight(gpui::FontWeight::BOLD).child(agent.display_name.to_string()))
                .child(txt(px(11.)).text_color(status_color).child(status_text))
            )
            .when_some(agent.binary_path.as_ref(), |el, path| {
                el.child(txt(px(11.)).text_color(gpui::rgba(0x888888FF))
                    .child(format!("{}: {}", t("settings.agent_cli.binary_path"), path.display())))
            })
        )
        .child(v_flex().gap(px(8.))
            .child(txt(px(13.)).font_weight(gpui::FontWeight::BOLD).child(t("settings.api.title")))
            .child(if agent_providers.is_empty() {
                txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(t("settings.no_api_providers")).into_any_element()
            } else {
                v_flex().gap(px(6.)).children(agent_providers.iter().map(|p| {
                    let fmt_color = match p.format {
                        ApiFormat::OpenAI => gpui::rgba(0x44AADDFF),
                        ApiFormat::Anthropic => gpui::rgba(0xDD8844FF),
                    };
                    h_flex().gap(px(8.)).py(px(4.)).px(px(8.))
                        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
                        .child(txt(px(12.)).font_weight(gpui::FontWeight::MEDIUM).child(p.name.clone()))
                        .child(txt(px(10.)).text_color(fmt_color).child(p.format.label().to_string()))
                })).into_any_element()
            })
        )
        .child(v_flex().gap(px(8.))
            .child(txt(px(13.)).font_weight(gpui::FontWeight::BOLD).child(t("settings.mcp.title")))
            .child(if mcp_entries.is_empty() {
                txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(t("mcp.no_servers")).into_any_element()
            } else {
                v_flex().gap(px(6.)).children(mcp_entries.iter().map(|entry| {
                    mcp_agent_entry_with_test(agent_name, entry, on_mcp_action, mcp_test_results)
                })).into_any_element()
            })
        )
        .child(
            div().cursor_pointer()
                .child(txt(px(11.)).text_color(gpui::rgba(0x44AADDFF))
                    .child(t("settings.agent_cli.add_mcp_extensions")))
                .on_mouse_down(gpui::MouseButton::Left, {
                    let otc = on_tab_change.clone();
                    let oetc = on_extension_tab_change.clone();
                    move |_: &gpui::MouseDownEvent, w: &mut Window, a: &mut gpui::App| {
                        otc(4, w, a);
                        oetc(ExtensionSubTab::Mcp, w, a);
                    }
                })
        )
        .child(v_flex().gap(px(8.))
            .child(txt(px(13.)).font_weight(gpui::FontWeight::BOLD).child(t("settings.tab.skills")))
            .child(if agent_skills.is_empty() {
                txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(t("skills.no_skills")).into_any_element()
            } else {
                h_flex().gap(px(8.)).flex_wrap().children(agent_skills.iter().map(|s| {
                    per_agent_skill_card(agent_name, s, on_skill_action)
                })).into_any_element()
            })
        )
        .into_any_element()
}

// ── Extensions sub-tab container ─────────────────────────────────────

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
) -> impl IntoElement {
    let otc = on_sub_tab_change.clone();
    let labels: Vec<(String, ExtensionSubTab)> = vec![
        (t("settings.tab.api"), ExtensionSubTab::Api),
        (t("settings.tab.mcp"), ExtensionSubTab::Mcp),
        (t("settings.tab.skills"), ExtensionSubTab::Skills),
    ];
    v_flex().gap(px(12.))
        .child(
            gpui_component::tab::TabBar::new("ext-sub-tabs")
                .segmented()
                .selected_index(labels.iter().position(|(_, s)| *s == active_sub).unwrap_or(0))
                .on_click({
                    let labels = labels.clone();
                    move |idx, w, a| {
                        if let Some((_, sub)) = labels.get(*idx) {
                            otc(*sub, w, a);
                        }
                    }
                })
                .children(labels.iter().map(|(l, _)| gpui_component::tab::Tab::new().label(l.to_string())))
        )
        .child(match active_sub {
            ExtensionSubTab::Api => tab_api_content(api_providers, api_form, mcp_available_agents, on_api_action, api_test_results, api_search, api_search_input).into_any_element(),
            ExtensionSubTab::Mcp => tab_mcp_content(mcp_servers, mcp_per_agent, mcp_form, mcp_available_agents, on_mcp_action, mcp_test_results, mcp_search, mcp_search_input).into_any_element(),
            ExtensionSubTab::Skills => tab_skills_content(skills, skill_dialog_input, skill_dialog_mode, on_skill_action, active_skills_subtab, skill_search, skill_search_input).into_any_element(),
        })
}
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
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { oma(McpAction::ShowAddForm, w, a); }
    };

    // Filter servers by search
    let filtered_servers: Vec<&crate::mcp::ZenixMcpServer> = if mcp_search.is_empty() {
        zenix_servers.iter().collect()
    } else {
        let q = mcp_search.to_lowercase();
        zenix_servers.iter().filter(|s| s.name.to_lowercase().contains(&q)).collect()
    };

    let form_el = mcp_form.map(|form| mcp_add_form(form, on_mcp_action));

    v_flex().gap(px(12.))
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(setting_group_header(t("settings.mcp.title")))
            .child(h_flex().gap(px(4.)).items_center()
                .child(txt(px(16.)).text_color(gpui::rgba(0x888888FF)).child("\u{1F50D}"))
                .child(mcp_search_input.map(|e| Input::new(e).flex_grow(1.0).into_any_element())
                    .unwrap_or_else(|| txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(t("settings.mcp.search")).into_any_element()))
                .child(txt(px(10.)).text_color(gpui::rgba(0x666666FF)).child(format!("{} servers", filtered_servers.len())))
                .child(Button::new("add-mcp").ghost().label(t("mcp.add_server")).on_click(add_cb))))
        .when_some(form_el, |el, form| el.child(form))
        .child(if filtered_servers.is_empty() && mcp_form.is_none() {
            txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child(t("settings.mcp.no_zenix_servers")).into_any_element()
        } else {
            h_flex().flex_wrap().gap(px(10.)).children(filtered_servers.iter().map(|s| {
                div().w(relative(0.48)).child(mcp_card(s, available_agents, on_mcp_action, mcp_form, mcp_test_results))
            })).into_any_element()
        })
}
fn mcp_card(
    s: &crate::mcp::ZenixMcpServer,
    available_agents: &[String],
    on_mcp_action: &SharedActionCb<McpAction>,
    mcp_form: Option<&McpFormState>,
    mcp_test_results: &HashMap<String, String>,
) -> AnyElement {
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

    let test_result = mcp_test_results.get(&name);
    let is_editing = mcp_form.and_then(|f| f.editing_name.as_ref()).map_or(false, |en| en == &s.name);

    // If this server is being edited inline, render form fields
    if is_editing {
        let form = mcp_form.unwrap();
        return mcp_inline_edit_card(s, form, on_mcp_action).into_any_element();
    }

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
    let test_cb = {
        let oma = on_mcp_action.clone();
        let test_name = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::TestServer { name: test_name.clone() }, w, a);
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
                .child(Button::new(format!("edit-zenix-mcp-{name}")).ghost().compact().label(t("general.edit")).on_click(edit_cb))
                .child(Button::new(format!("test-zenix-mcp-{name}")).ghost().compact().label(t("general.test")).on_click(test_cb))
                .child(Button::new(format!("rm-zenix-mcp-{name}")).ghost().compact().label(t("mcp.remove_server")).on_click(remove_cb))))
        .when_some(test_result, |el, tr| {
            let res_color = if tr.starts_with("OK:") { gpui::rgba(0x00DD44FF) } else { gpui::rgba(0xFF4444FF) };
            el.child(txt(px(10.)).text_color(res_color).child(tr.to_string()))
        })
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
        .into_any_element()
}

fn mcp_inline_edit_card(
    _s: &crate::mcp::ZenixMcpServer,
    form: &McpFormState,
    on_mcp_action: &SharedActionCb<McpAction>,
) -> impl IntoElement {
    let cancel_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { oma(McpAction::CancelAddForm, w, a); }
    };
    let submit_cb = {
        let oma = on_mcp_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { oma(McpAction::SubmitAddForm, w, a); }
    };
    let is_sse = form.mcp_type == "sse";

    v_flex().gap(px(6.)).py(px(8.)).px(px(10.))
        .border_1().border_color(gpui::rgba(0x44AAFFFF)).rounded_md()
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.name")))
            .child(form.name.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.type")))
            .child(Button::new("inline-mcp-type-stdio")
                .when(!is_sse, |b| b.primary()).when(is_sse, |b| b.ghost())
                .compact().label("stdio").on_click({
                    let oma = on_mcp_action.clone();
                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                        oma(McpAction::SetFormType { mcp_type: "stdio".into() }, w, a);
                    }
                }))
            .child(Button::new("inline-mcp-type-sse")
                .when(is_sse, |b| b.primary()).when(!is_sse, |b| b.ghost())
                .compact().label("sse").on_click({
                    let oma = on_mcp_action.clone();
                    move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                        oma(McpAction::SetFormType { mcp_type: "sse".into() }, w, a);
                    }
                })))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.command")))
            .child(form.command.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.args")))
            .child(form.args.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).items_start()
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.env")))
            .child(form.env.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).justify_end()
            .child(Button::new("inline-mcp-form-cancel").ghost().label(t("general.cancel")).on_click(cancel_cb))
            .child(Button::new("inline-mcp-form-submit").primary().label(t("general.update")).on_click(submit_cb)))
}

fn mcp_add_form(
    form: &McpFormState,
    on_mcp_action: &SharedActionCb<McpAction>,
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
    let submit_label = if form.editing_name.is_some() { t("general.update") } else { t("general.add") };
    let form_title = if form.editing_name.is_some() { t("settings.mcp.form.title_edit") } else { t("settings.mcp.form.title_add") };

    v_flex().gap(px(8.)).py(px(10.)).px(px(12.))
        .border_1().border_color(gpui::rgba(0x45475a66)).rounded_md()
        .child(txt(px(13.)).font_weight(gpui::FontWeight::BOLD).child(form_title))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.name")))
            .child(form.name.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.type")))
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
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.command")))
            .child(form.command.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.args")))
            .child(form.args.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).items_start()
            .child(txt(px(12.)).w(px(60.)).child(t("settings.mcp.form.env")))
            .child(form.env.as_ref().map(|e| Input::new(e).into_any_element())
                .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element())))
        .child(h_flex().gap(px(8.)).justify_end()
            .child(Button::new("mcp-form-cancel").ghost().label(t("general.cancel")).on_click(cancel_cb))
            .child(Button::new("mcp-form-submit").primary().label(submit_label).on_click(submit_cb)))
}


fn mcp_agent_entry_with_test(
    agent: &str,
    entry: &crate::mcp::McpServerEntry,
    on_mcp_action: &SharedActionCb<McpAction>,
    mcp_test_results: &HashMap<String, String>,
) -> impl IntoElement {
    let name = entry.name.clone();
    let cmd = format!("{} {}", entry.command, entry.args.join(" "));
    let disabled = entry.disabled;
    let test_result = mcp_test_results.get(&name);

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
    let test_cb = {
        let oma = on_mcp_action.clone();
        let test_name = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            oma(McpAction::TestServer { name: test_name.clone() }, w, a);
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
                .child(Button::new(format!("test-agent-mcp-{agent}-{name}")).ghost().compact()
                    .label(t("general.test")).on_click(test_cb))
                .child(Button::new(format!("remove-agent-mcp-{agent}-{name}")).ghost().compact()
                    .label(t("mcp.remove_server"))
                    .on_click(remove_cb))))
        .child(txt(px(10.)).text_color(gpui::rgba(0x888888FF)).child(cmd))
        .when_some(test_result, |el, tr| {
            let res_color = if tr.starts_with("OK:") { gpui::rgba(0x00DD44FF) } else { gpui::rgba(0xFF4444FF) };
            el.child(txt(px(10.)).text_color(res_color).child(tr.to_string()))
        })
}

// ── Skills (Extensions sub-tab) ──────────────────────────────────────

fn tab_skills_content(
    _skills: &HashMap<String, crate::skills::SkillInfo>,
    skill_dialog_input: Option<&Entity<gpui_component::input::InputState>>,
    skill_dialog_mode: &InputDialogMode,
    on_skill_action: &SharedActionCb<SkillAction>,
    active_subtab: SkillsSubTab,
    skill_search: &str,
    skill_search_input: Option<&Entity<gpui_component::input::InputState>>,
) -> impl IntoElement {
    let zenix_skills = crate::skills::zenix_skills();
    let mut sorted: Vec<_> = zenix_skills.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let refresh_cb = {
        let osa = on_skill_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { osa(SkillAction::Refresh, w, a); }
    };

    let filtered: Vec<&crate::skills::SkillInfo> = sorted.iter()
        .filter(|s| {
            skill_search.is_empty()
                || s.name.to_lowercase().contains(&skill_search.to_lowercase())
        })
        .copied()
        .collect();

    let labels: Vec<(String, SkillsSubTab)> = vec![
        (t("skills.installed"), SkillsSubTab::Installed),
        (t("skills.market"), SkillsSubTab::Market),
    ];

    let otc = on_skill_action.clone();

    v_flex().gap(px(16.))
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(h_flex().gap(px(4.))
                .child(setting_group_header(t("skills.zenix_skills")))
                .child(
                    gpui_component::tab::TabBar::new("skills-sub-tabs")
                        .segmented()
                        .selected_index(labels.iter().position(|(_, s)| *s == active_subtab).unwrap_or(0))
                        .on_click({
                            let labels = labels.clone();
                            let otc = otc.clone();
                            move |idx, w, a| {
                                if let Some((_, sub)) = labels.get(*idx) {
                                    otc(SkillAction::SetSubTab(*sub), w, a);
                                }
                            }
                        })
                        .children(labels.iter().map(|(l, _)| gpui_component::tab::Tab::new().label(l.to_string())))
                )
            )
            .child(Button::new("refresh-s").ghost().label(t("general.refresh")).on_click(refresh_cb)))
        .child(h_flex().gap(px(4.)).items_center()
            .child(txt(px(16.)).text_color(gpui::rgba(0x888888FF)).child("\u{1F50D}"))
            .child(skill_search_input.map(|e| Input::new(e).flex_grow(1.0).into_any_element())
                .unwrap_or_else(|| txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(t("skills.search")).into_any_element()))
            .child(txt(px(10.)).text_color(gpui::rgba(0x666666FF)).child(format!("{} items", filtered.len())))
        )
        .child(match active_subtab {
            SkillsSubTab::Installed => {
                if filtered.is_empty() {
                    txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child(t("skills.no_installed")).into_any_element()
                } else {
                    h_flex().gap(px(8.)).flex_wrap().children(filtered.iter().map(|s| {
                        zenix_skill_card(s, on_skill_action)
                    })).into_any_element()
                }
            }
            SkillsSubTab::Market => {
                v_flex().gap(px(12.))
                    .child(txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child(t("skills.market_description")))
                    .child(
                        h_flex().gap(px(8.)).flex_wrap()
                            .children(filtered.iter().map(|s| zenix_skill_card(s, on_skill_action)))
                    )
                    .child(add_skill_card(on_skill_action))
                    .into_any_element()
            }
        })
        .when_some(skill_dialog_input, |el, inp| {
            el.when(*skill_dialog_mode == InputDialogMode::SkillAddGit, |el| {
                el.child(v_flex().gap(px(8.)).py(px(8.)).px(px(10.))
                    .border_1().border_color(gpui::rgba(0x45475a66)).rounded_md()
                    .child(txt(px(12.)).child(t("skills.dialog_git_url")))
                    .child(Input::new(inp)))
            })
            .when(*skill_dialog_mode == InputDialogMode::SkillAddLocal, |el| {
                el.child(v_flex().gap(px(8.)).py(px(8.)).px(px(10.))
                    .border_1().border_color(gpui::rgba(0x45475a66)).rounded_md()
                    .child(txt(px(12.)).child(t("skills.dialog_local_path")))
                    .child(Input::new(inp)))
            })
        })
}
fn zenix_skill_card(
    s: &crate::skills::SkillInfo,
    on_skill_action: &SharedActionCb<SkillAction>,
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
        .w(px(200.))
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
    on_skill_action: &SharedActionCb<SkillAction>,
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
        .w(px(200.))
        .border_1().border_color(gpui::rgba(0x45475a66)).rounded_md()
        .child(txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child(t("skills.add_skill")))
        .child(h_flex().gap(px(4.)).flex_wrap()
            .child(Button::new("add-git").ghost().compact().label(t("skills.add_from_git")).on_click(git_cb))
            .child(Button::new("add-local").ghost().compact().label(t("skills.add_from_local")).on_click(local_cb)))
}

fn per_agent_skill_card(
    agent: &str,
    s: &crate::skills::SkillInfo,
    on_skill_action: &SharedActionCb<SkillAction>,
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
        .w(px(200.))
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

// ── API Providers (Extensions sub-tab) ────────────────────────────────

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
        providers.iter().filter(|p| p.name.to_lowercase().contains(&q)).collect()
    };

    let add_cb = {
        let on_api = on_api.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on_api(ApiProviderAction::ShowAddForm, w, a);
        }
    };

    v_flex().gap(px(12.))
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(setting_group_header(t("settings.api.title")))
            .child(h_flex().gap(px(4.)).items_center()
                .child(txt(px(16.)).text_color(gpui::rgba(0x888888FF)).child("\u{1F50D}"))
                .child(api_search_input.map(|e| Input::new(e).flex_grow(1.0).into_any_element())
                    .unwrap_or_else(|| txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(t("settings.api.search")).into_any_element()))
                .child(txt(px(10.)).text_color(gpui::rgba(0x666666FF)).child(format!("{} items", filtered.len())))
                .child(Button::new("add-api-provider").ghost().label(t("general.add")).on_click(add_cb))))
        .when_some(form, |el, f| el.child(api_provider_form(f, on_action)))
        .child(if filtered.is_empty() && form.is_none() {
            txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child(t("settings.api.no_providers")).into_any_element()
        } else {
            v_flex().gap(px(8.)).children(filtered.iter().map(|p| {
                api_provider_card(p, available_agents, on_action, api_test_results)
            })).into_any_element()
        })
}

fn api_provider_card(
    p: &ApiProviderConfig,
    available_agents: &[String],
    on_action: &SharedActionCb<ApiProviderAction>,
    api_test_results: &HashMap<String, String>,
) -> impl IntoElement {
    let name = p.name.clone();
    let format_label = p.format.label();
    let format_color = match p.format {
        ApiFormat::OpenAI => gpui::rgba(0x44AADDFF),
        ApiFormat::Anthropic => gpui::rgba(0xDD8844FF),
    };
    let endpoint = format!("{}{}", p.base_url.trim_end_matches('/'), p.url_suffix);
    let think = format!("think: {}", p.thinking_level.label());
    let masked_key = if p.api_key.len() > 8 {
        format!("{}...{}", &p.api_key[..4], &p.api_key[p.api_key.len()-4..])
    } else {
        "****".to_string()
    };
    let test_result = api_test_results.get(&name);

    let edit_cb = {
        let on = on_action.clone();
        let n = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::Edit { name: n.clone() }, w, a);
        }
    };
    let test_cb = {
        let on = on_action.clone();
        let n = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::TestConnection { name: n.clone() }, w, a);
        }
    };
    let remove_cb = {
        let on = on_action.clone();
        let n = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::Remove { name: n.clone() }, w, a);
        }
    };
    let fetch_cb = {
        let on = on_action.clone();
        let n = name.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::FetchModels { name: n.clone() }, w, a);
        }
    };

    v_flex().gap(px(6.)).py(px(8.)).px(px(10.))
        .border_1().border_color(gpui::rgba(0x45475a44)).rounded_md()
        .child(h_flex().gap(px(8.)).items_center().justify_between()
            .child(h_flex().gap(px(8.)).items_center()
                .child(txt(px(13.)).font_weight(gpui::FontWeight::MEDIUM).child(name.clone()))
                .child(txt(px(10.)).text_color(format_color).child(format_label)))
            .child(h_flex().gap(px(4.))
                .child(Button::new(format!("edit-api-{name}")).ghost().compact().label(t("general.edit")).on_click(edit_cb))
                .child(Button::new(format!("test-api-{name}")).ghost().compact().label(t("general.test")).on_click(test_cb))
                .child(Button::new(format!("fetch-api-{name}")).ghost().compact().label(t("general.models")).on_click(fetch_cb))
                .child(Button::new(format!("rm-api-{name}")).ghost().compact().label(t("general.remove")).on_click(remove_cb))))
        .when_some(test_result, |el, tr| {
            let res_color = if tr.starts_with("OK:") { gpui::rgba(0x00DD44FF) } else { gpui::rgba(0xFF4444FF) };
            el.child(txt(px(10.)).text_color(res_color).child(tr.to_string()))
        })
        .child(txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(endpoint))
        .child(h_flex().gap(px(12.))
            .child(txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(format!("model: {}", p.model)))
            .child(txt(px(11.)).text_color(gpui::rgba(0x888888FF)).child(think))
            .child(txt(px(11.)).text_color(gpui::rgba(0x666666FF)).child(format!("key: {masked_key}"))))
        .child(h_flex().gap(px(4.)).flex_wrap().children(available_agents.iter().map(|agent| {
            let checked = p.enabled_agents.contains(agent);
            let srv = name.clone();
            let agent_name = agent.clone();
            let on = on_action.clone();
            Button::new(format!("api-agent-{srv}-{agent_name}"))
                .when(checked, |b| b.primary())
                .when(!checked, |b| b.ghost())
                .compact()
                .label(format!(
                    "{} {}",
                    if checked { "\u{2611}" } else { "\u{2610}" },
                    agent_name
                ))
                .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                    on(ApiProviderAction::ToggleAgent {
                        name: srv.clone(),
                        agent: agent_name.clone(),
                    }, w, a);
                })
        })))
}

fn api_provider_form(
    form: &ApiProviderFormState,
    on_action: &SharedActionCb<ApiProviderAction>,
) -> impl IntoElement {
    let cancel_cb = {
        let on = on_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { on(ApiProviderAction::CancelForm, w, a); }
    };
    let submit_cb = {
        let on = on_action.clone();
        move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| { on(ApiProviderAction::SubmitForm, w, a); }
    };

    let form_title = if form.editing_name.is_some() { t("settings.api.form.title_edit") } else { t("settings.api.form.title_add") };
    let submit_label = if form.editing_name.is_some() { t("general.update") } else { t("general.add") };
    let presets = form.format.presets();

    v_flex().gap(px(8.)).py(px(10.)).px(px(12.))
        .border_1().border_color(gpui::rgba(0x45475a66)).rounded_md()
        .child(txt(px(13.)).font_weight(gpui::FontWeight::BOLD).child(form_title))
        .child(form_field(t("settings.api.form.name"), form.name.as_ref()))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(80.)).child(t("settings.api.form.format")))
            .child(api_format_btn(ApiFormat::OpenAI, form.format, on_action))
            .child(api_format_btn(ApiFormat::Anthropic, form.format, on_action)))
        // Preset selector
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(80.)).child(t("settings.api.form.preset")))
            .child(h_flex().gap(px(4.)).flex_wrap().children(presets.iter().map(|preset| {
                let p = preset.clone();
                let on = on_action.clone();
                Button::new(format!("preset-{}", p.name.to_lowercase().replace(' ', "-")))
                    .ghost().compact()
                    .label(p.name.to_string())
                    .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                        on(ApiProviderAction::ApplyPreset { preset: p.clone() }, w, a);
                    })
            }))))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(80.)).child(t("settings.api.form.suffix")))
            .child(h_flex().gap(px(4.)).children(
                form.format.suffix_options().iter().map(|suffix| {
                    let s = suffix.to_string();
                    let on = on_action.clone();
                    Button::new(format!("suffix-{}", s.replace('/', "-")))
                        .ghost().compact().label(s.clone())
                        .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            on(ApiProviderAction::SetSuffix { suffix: s.clone() }, w, a);
                        })
                })
            )))
        .child(form_field(t("settings.api.form.custom"), form.url_suffix.as_ref()))
        .child(form_field(t("settings.api.form.base_url"), form.base_url.as_ref()))
        .child(form_field(t("settings.api.form.api_key"), form.api_key.as_ref()))
        .child(h_flex().gap(px(8.)).items_center()
            .child(txt(px(12.)).w(px(80.)).child(t("settings.api.form.think")))
            .child(h_flex().gap(px(4.)).children(
                ThinkingLevel::all().iter().map(|level| {
                    let lev = *level;
                    let selected = lev == form.thinking_level;
                    let on = on_action.clone();
                    Button::new(format!("think-{}", lev.label().to_lowercase()))
                        .when(selected, |b| b.primary())
                        .when(!selected, |b| b.ghost())
                        .compact()
                        .label(lev.label().to_string())
                        .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                            on(ApiProviderAction::SetThinkingLevel { level: lev }, w, a);
                        })
                })
            )))
        .child(if !form.available_models.is_empty() {
            let selected = form.selected_model.clone();
            v_flex().gap(px(4.))
                .child(txt(px(12.)).child(t("settings.api.form.model")))
                .child(txt(px(11.)).text_color(gpui::rgba(0x44DD88FF))
                    .child(t("settings.api.form.models_available").replace("{count}", &form.available_models.len().to_string())))
                .child(h_flex().gap(px(4.)).flex_wrap().children(
                    form.available_models.iter().take(10).map(|m| {
                        let m_clone = m.clone();
                        let sel = *m == selected;
                        let on = on_action.clone();
                        Button::new(format!("model-{}", m_clone))
                            .when(sel, |b| b.primary())
                            .when(!sel, |b| b.ghost())
                            .compact()
                            .label(m_clone.clone())
                            .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
                                on(ApiProviderAction::SelectModel { model: m_clone.clone() }, w, a);
                            })
                    })
                ))
                .into_any_element()
        } else {
            txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child(t("settings.api.form.model_fetch_hint")).into_any_element()
        })
        .child(h_flex().gap(px(8.)).justify_end()
            .child(Button::new("api-form-cancel").ghost().label(t("general.cancel")).on_click(cancel_cb))
            .child(Button::new("api-form-submit").primary().label(submit_label).on_click(submit_cb)))
}

fn api_format_btn(
    fmt: ApiFormat,
    current: ApiFormat,
    on_action: &SharedActionCb<ApiProviderAction>,
) -> impl IntoElement {
    let selected = fmt == current;
    let on = on_action.clone();
    Button::new(format!("api-fmt-{}", fmt.label().to_lowercase()))
        .when(selected, |b| b.primary())
        .when(!selected, |b| b.ghost())
        .compact()
        .label(fmt.label().to_string())
        .on_click(move |_: &ClickEvent, w: &mut Window, a: &mut gpui::App| {
            on(ApiProviderAction::SetFormat { format: fmt }, w, a);
        })
}

fn form_field(
    label: impl Into<String>,
    input: Option<&Entity<gpui_component::input::InputState>>,
) -> impl IntoElement {
    h_flex().gap(px(8.)).items_center()
        .child(txt(px(12.)).w(px(80.)).child(label.into()))
        .child(input.map(|e| Input::new(e).into_any_element())
            .unwrap_or_else(|| txt(px(12.)).text_color(gpui::rgba(0x888888FF)).child("_").into_any_element()))
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
