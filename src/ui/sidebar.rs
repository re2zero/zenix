//! Right sidebar — icon bar with expandable panels for settings and system info.
//! Built with gpui-component native widgets: GroupBox, Separator, Kbd, Label, Progress.
use gpui::{
    div, px, relative, AnyElement, ClickEvent, FontWeight, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement as _, Styled, Window,
};
use gpui_component::{
    Theme,
    group_box::GroupBox,
    h_flex, scroll::ScrollableElement as _, v_flex,
    progress::Progress,
    separator::Separator,
};
use crate::i18n::t;

use crate::sys::{NetInterface, SystemInfo};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Panel { None, SystemInfo }

pub const SIDEBAR_WIDTH: f32 = 44.0;
pub const PANEL_WIDTH: f32 = 280.0;

pub type ThemeCallback = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui::App)>;

// ── Icon bar ──────────────────────────────────────────────────────────

pub fn sidebar(
    theme: &Theme, active: Panel, settings_active: bool,
    on_sysinfo: ThemeCallback, on_settings: ThemeCallback,
) -> gpui::Div {
    let bg = theme.background;
    h_flex().flex_none().w(px(SIDEBAR_WIDTH)).h_full().bg(bg)
        .border_l_1().border_color(theme.border)
        .child(v_flex().size_full().justify_between().py_2()
            .child(v_flex().gap_1().px(px(4.))
                .child(icon_btn("sysinfo","\u{1F4CA}",active==Panel::SystemInfo,on_sysinfo)))
            .child(v_flex().gap_1().px(px(4.))
                .child(icon_btn("settings","\u{2699}\u{FE0F}",settings_active,on_settings))))
}

fn icon_btn(id: &str, icon: &str, active: bool, cb: ThemeCallback) -> impl IntoElement {
    let s = id.to_string();
    let ic = icon.to_string();
    let bg = if active { gpui::rgba(0xFFFFFF18) } else { gpui::rgba(0x00000000) };
    div().id(s).w(px(36.)).h(px(36.)).rounded_md().bg(bg)
        .flex().items_center().justify_center().cursor_pointer()
        .hover(|st| st.bg(gpui::rgba(0xFFFFFF0E)))
        .on_click(cb)
        .child(txt(px(16.)).line_height(px(20.)).child(ic))
}


// ═══════════════════════════════════════════════════════════════════════
// System info panel
// ═══════════════════════════════════════════════════════════════════════

pub fn system_info_panel(theme: &Theme, info: &SystemInfo) -> impl IntoElement {
    let th = theme.clone();
    let ac = th.primary;
    let mu = th.muted_foreground;
    let cd = gpui::hsla(0.01, 0.90, 0.56, 1.0);
    let cw = gpui::hsla(0.10, 1.0, 0.50, 1.0);
    let cg = gpui::hsla(0.33, 0.39, 0.49, 1.0);

    let cores: Vec<(f32, gpui::Hsla)> = info.per_core_percents.iter()
        .map(|&p| if p > 80.0 { (p, cd) } else if p > 50.0 { (p, cw) } else { (p, cg) })
        .collect();

    let mut panel = v_flex()
        .w(px(PANEL_WIDTH)).h_full().bg(th.background)
        .border_l_1().border_color(th.border).p(px(16.)).gap(px(14.))
        .overflow_y_scrollbar()
        .child(Separator::horizontal())

        // Overview
        .child(overview(&info.hostname, &info.kernel_version, &info.uptime_str, info.process_count, mu))
        .child(Separator::horizontal())

        // CPU
        .child(subsec(&t("system.cpu"), vec![
            progress_bar(info.cpu_percent, ac, &t("system.cpu_overall").replace("%{pct}", &format!("{:.1}", info.cpu_percent))),
            core_grid(&cores, &th),
            dim(&format!("{}  \u{00b7}  {} cores  \u{00b7}  {:.0} MHz  \u{00b7}  {:.0}\u{00b0}C",
                trunc_model(&info.cpu_model, 44), info.cpu_count, info.cpu_freq_mhz, info.cpu_temp_c), mu),
            dim(&t("system.cpu_load_avg").replace("%{l1}", &format!("{:.2}", info.load_avg_1))
                .replace("%{l5}", &format!("{:.2}", info.load_avg_5))
                .replace("%{l15}", &format!("{:.2}", info.load_avg_15)), mu),
        ]))
        .child(Separator::horizontal())

        // Memory
        .child(subsec(&t("system.memory"), vec![
            progress_bar(info.mem_percent, ac, &t("system.memory_used")
                .replace("%{used}", &format!("{:.1}", info.mem_used_gb))
                .replace("%{total}", &format!("{:.1}", info.mem_total_gb))
                .replace("%{pct}", &format!("{:.1}", info.mem_percent))),
            dim(&t("system.memory_cache")
                .replace("%{cache}", &format!("{:.1}", info.mem_cached_gb))
                .replace("%{buffers}", &format!("{:.1}", info.mem_buffers_gb)), mu),
        ]));

    // Swap
    if info.swap_total_gb > 0.0 {
        panel = panel.child(Separator::horizontal())
            .child(subsec(&t("system.swap"), vec![
                progress_bar(info.swap_percent, ac, &t("system.swap_used")
                    .replace("%{used}", &format!("{:.1}", info.swap_used_gb))
                    .replace("%{total}", &format!("{:.1}", info.swap_total_gb))
                    .replace("%{pct}", &format!("{:.1}", info.swap_percent))),
            ]));
    }

    // Network
    if !info.net_interfaces.is_empty() {
        let rows: Vec<AnyElement> = info.net_interfaces.iter()
            .map(|ni| net_iface(ni, mu)).collect();
        panel = panel.child(Separator::horizontal())
            .child(subsec(&t("system.network"), rows));
    }

    // Disk
    if !info.disk_mounts.is_empty() {
        let rows: Vec<AnyElement> = info.disk_mounts.iter()
            .map(|dm| disk_mount(dm, mu)).collect();
        panel = panel.child(Separator::horizontal())
            .child(subsec(&t("system.disk"), rows));
    }

    panel
}
fn core_grid(data: &[(f32, gpui::Hsla)], theme: &Theme) -> AnyElement {
    if data.is_empty() { return div().into_any_element(); }
    let t = theme.clone();
    let cols = if data.len() <= 8 { 2 } else if data.len() <= 16 { 4 } else { 6 };
    v_flex().gap(px(2.))
        .children(data.chunks(cols).map(move |row| {
            h_flex().gap(px(2.)).children(row.iter().map(|&(p, c)| {
                let cl = p.clamp(0.0, 100.0);
                div().flex_grow(1.0).h(px(4.)).rounded_sm()
                    .bg(t.muted_foreground.opacity(0.10))
                    .child(div().h_full().w(relative(cl / 100.0)).rounded_sm().bg(c))
            }))
        }))
        .into_any_element()
}

fn net_iface(ni: &NetInterface, mu: gpui::Hsla) -> AnyElement {
    let ip = if ni.ip.is_empty() { String::new() } else { format!(" ({})", ni.ip) };
    h_flex().gap(px(6.)).items_center().justify_between()
        .child(txt(px(10.)).font_weight(FontWeight::MEDIUM).child(format!("{}{ip}", ni.name)))
        .child(txt(px(10.)).text_color(mu)
            .child(format!("\u{2193}{:.1} \u{2191}{:.1} Mbps", ni.rx_rate_mbps, ni.tx_rate_mbps)))
        .into_any_element()
}

fn disk_mount(dm: &crate::sys::DiskMount, mu: gpui::Hsla) -> AnyElement {
    txt(px(10.)).text_color(mu)
        .child(format!("{}  {:.1}%", dm.mount_point, dm.percent))
        .into_any_element()
}

fn subsec(title: &str, body: Vec<AnyElement>) -> AnyElement {
    GroupBox::new()
        .title(sec_title(title))
        .child(v_flex().gap(px(6.)).children(body))
        .into_any_element()
}

fn overview(host: &str, kern: &str, up: &str, procs: u32, mu: gpui::Hsla) -> AnyElement {
    GroupBox::new()
        .title(sec_title(t("system.overview")))
        .child(v_flex().gap(px(4.))
            .child(bold_txt(host))
            .child(txt(px(10.)).text_color(mu).child(kern.to_string()))
            .child(h_flex().gap(px(12.))
                .child(txt(px(10.)).text_color(mu).child(format!("Up {up}")))
                .child(txt(px(10.)).text_color(mu).child(format!("{procs} procs")))))
        .into_any_element()
}

fn progress_bar(pct: f32, color: gpui::Hsla, label: &str) -> AnyElement {
    v_flex().gap(px(4.))
        .child(txt(px(10.)).child(label.to_string()))
        .child(Progress::new("bar").value(pct).color(color))
        .into_any_element()
}

fn dim(t: &str, mu: gpui::Hsla) -> AnyElement {
    txt(px(10.)).text_color(mu).child(t.to_string()).into_any_element()
}

fn txt(size: gpui::Pixels) -> gpui::Div { div().text_size(size) }

fn bold_txt(text: impl Into<String>) -> impl IntoElement {
    let s = text.into();
    div()
        .text_size(px(14.))
        .font_weight(FontWeight::BOLD)
        .child(s)
}

/// Section title for GroupBox headers (smaller than panel title).
fn sec_title(text: impl Into<String>) -> impl IntoElement {
    let s = text.into();
    div()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(s)
}

fn trunc_model(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}...", &s[..max.saturating_sub(3)]) }
}
