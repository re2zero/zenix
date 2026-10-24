//! The settings surface: pages the registry hands in, items that write the
//! config.
//!
//! Nothing here is reached by name from the shell's render — the pages are
//! contributions ([`super::registry::SettingsSection`]), exactly as the
//! sidebar's panels and the status bar's segments are, so a page a plugin
//! declared would mount the same way (PLAN.md §5.4).
//!
//! Every item is a view of one config key (`crate::config`): its value comes
//! from the config global, and its setter goes through `config::update`, so the
//! file, the running client and the panel cannot disagree about what a setting
//! is. A value the client cannot act on is not offered here at all.
//!
//! The surface is a dialog over the shell, sized like the prototype's
//! (`docs/ui/prototype.html`, `.dialog.settings`): a card capped at 1000×660
//! and centred on a scrim, a 190px page list down the left, the fields on the
//! right. It is not a full-window view — a settings page with its controls
//! spread across a 4K window is the thing this surface exists to avoid.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Icon, Sizable as _, Theme, ThemeMode, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::commands::{self, Command};
use super::keymap::Keymap;
use super::registry::{Dispatch, SettingsSection, ShellRequest};
use crate::config::{
    self, ASIDE_WIDTH_MAX, ASIDE_WIDTH_MIN, Appearance, Layout, SIDEBAR_WIDTH_MAX,
    SIDEBAR_WIDTH_MIN, TerminalConfig,
};
use crate::i18n::t;
use crate::platform;
use crate::terminal::Theme as TerminalTheme;

/// The built-in pages, in the order the settings nav lists them: one per config
/// section, the reference pages the prototype puts between them, then the file
/// itself.
pub(crate) const SETTINGS_SECTIONS: &[SettingsSection] = &[
    SettingsSection {
        id: "appearance",
        page: appearance_page,
    },
    SettingsSection {
        id: "terminal",
        page: terminal_page,
    },
    SettingsSection {
        id: "layout",
        page: layout_page,
    },
    SettingsSection {
        id: "keys",
        page: keys_page,
    },
    SettingsSection {
        id: "plugins",
        page: plugins_page,
    },
    SettingsSection {
        id: "mcp",
        page: mcp_page,
    },
    SettingsSection {
        id: "config",
        page: config_page,
    },
];

/// Where a named page sits in [`SETTINGS_SECTIONS`]. The status bar asks for
/// the config page by name rather than by position, so a page added in front
/// of it cannot turn the diagnostic count into a link to the wrong page.
pub(crate) fn page_index(id: &str) -> Option<usize> {
    SETTINGS_SECTIONS
        .iter()
        .position(|section| section.id == id)
}

// ── the page model ─────────────────────────────────────────────────────────

/// A page's revert action: what the arrow in its header writes back.
type Reset = Rc<dyn Fn(&mut App)>;
/// A field's write path; the control owns it for as long as it lives.
type SetText = Rc<dyn Fn(SharedString, &mut App)>;
type SetNumber = Rc<dyn Fn(f32, &mut App)>;

/// One page of the dialog: a title and a line of context, then its sections.
pub(crate) struct Page {
    title: SharedString,
    description: SharedString,
    sections: Vec<Section>,
    /// What the page's revert button writes. `None` for the reference pages:
    /// they show facts, and there is nothing to put back.
    reset: Option<Reset>,
    /// Marks the nav row, e.g. the config page while the file has diagnostics.
    warn: bool,
}

struct Section {
    title: Option<SharedString>,
    rows: Vec<Row>,
}

/// One row: a label column (title and a line on what it does) with the control
/// at its right, the prototype's `.field`. A row without a label is a block
/// and takes the full width.
struct Row {
    label: Option<SharedString>,
    description: Option<SharedString>,
    control: AnyElement,
}

fn page(title: impl Into<SharedString>, description: impl Into<SharedString>) -> Page {
    Page {
        title: title.into(),
        description: description.into(),
        sections: Vec::new(),
        reset: None,
        warn: false,
    }
}

impl Page {
    fn section(mut self, section: Section) -> Self {
        self.sections.push(section);
        self
    }

    fn reset(mut self, reset: impl Fn(&mut App) + 'static) -> Self {
        self.reset = Some(Rc::new(reset));
        self
    }

    fn warn(mut self) -> Self {
        self.warn = true;
        self
    }
}

impl Section {
    fn new() -> Self {
        Self {
            title: None,
            rows: Vec::new(),
        }
    }

    fn field(
        mut self,
        label: impl Into<SharedString>,
        description: Option<SharedString>,
        control: AnyElement,
    ) -> Self {
        self.rows.push(Row {
            label: Some(label.into()),
            description,
            control,
        });
        self
    }

    fn block(mut self, element: impl IntoElement) -> Self {
        self.rows.push(Row {
            label: None,
            description: None,
            control: element.into_any_element(),
        });
        self
    }
}

// ── pages ──────────────────────────────────────────────────────────────────

/// The chrome's own look: which theme family draws it, which mode of that
/// family, and in which language.
fn appearance_page(cx: &App) -> Page {
    let families: Vec<(SharedString, SharedString)> = config::theme::NAMES
        .iter()
        .map(|name| {
            (
                SharedString::from(*name),
                t(&format!("settings.appearance.family_{name}")).into(),
            )
        })
        .collect();
    page(
        t("settings.appearance.title"),
        t("settings.appearance.description"),
    )
    .reset(|cx| {
        config::update(cx, |config| config.appearance = Appearance::default());
    })
    .section(
        Section::new()
            .field(
                t("settings.appearance.family"),
                Some(t("settings.appearance.family_description").into()),
                dropdown(
                    "appearance-family",
                    families,
                    config::get(cx).appearance.theme.clone(),
                    |value, cx| {
                        config::update(cx, |config| config.appearance.theme = value.to_string());
                    },
                ),
            )
            .field(
                t("settings.appearance.theme"),
                Some(t("settings.appearance.theme_description").into()),
                dropdown(
                    "appearance-mode",
                    vec![
                        ("light".into(), t("settings.appearance.theme_light").into()),
                        ("dark".into(), t("settings.appearance.theme_dark").into()),
                    ],
                    config::get(cx).appearance.theme_mode.name().to_owned(),
                    |value, cx| {
                        let mode = match value.as_ref() {
                            "dark" => ThemeMode::Dark,
                            _ => ThemeMode::Light,
                        };
                        config::update(cx, |config| config.appearance.theme_mode = mode);
                    },
                ),
            )
            .field(
                t("settings.appearance.locale"),
                Some(t("settings.appearance.locale_description").into()),
                dropdown(
                    "appearance-locale",
                    vec![
                        ("en".into(), "English".into()),
                        ("zh-CN".into(), "简体中文".into()),
                    ],
                    config::get(cx).appearance.locale.clone(),
                    |value, cx| {
                        config::update(cx, |config| config.appearance.locale = value.to_string());
                    },
                ),
            ),
    )
}

/// The palettes named cell colors resolve against, one per mode, so switching
/// the chrome's mode keeps the terminal readable.
fn terminal_page(cx: &App) -> Page {
    let choices: Vec<(SharedString, SharedString)> = TerminalTheme::BUILTIN_NAMES
        .iter()
        .map(|name| (SharedString::from(*name), SharedString::from(*name)))
        .collect();
    page(
        t("settings.terminal.title"),
        t("settings.terminal.description"),
    )
    .reset(|cx| {
        config::update(cx, |config| config.terminal = TerminalConfig::default());
    })
    .section(
        Section::new()
            .field(
                t("settings.terminal.palette_dark"),
                Some(t("settings.terminal.palette_dark_description").into()),
                dropdown(
                    "terminal-palette-dark",
                    choices.clone(),
                    config::get(cx).terminal.palette_dark.clone(),
                    |value, cx| {
                        config::update(cx, |config| {
                            config.terminal.palette_dark = value.to_string()
                        });
                    },
                ),
            )
            .field(
                t("settings.terminal.palette_light"),
                Some(t("settings.terminal.palette_light_description").into()),
                dropdown(
                    "terminal-palette-light",
                    choices,
                    config::get(cx).terminal.palette_light.clone(),
                    |value, cx| {
                        config::update(cx, |config| {
                            config.terminal.palette_light = value.to_string()
                        });
                    },
                ),
            ),
    )
}

/// The client's own chrome: the sidebar and the right-hand chat panel, neither
/// of which the daemon knows anything about.
fn layout_page(cx: &App) -> Page {
    let layout = &config::get(cx).layout;
    page(t("settings.layout.title"), t("settings.layout.description"))
        .reset(|cx| {
            config::update(cx, |config| config.layout = Layout::default());
        })
        .section(
            Section::new()
                .field(
                    t("settings.layout.sidebar_visible"),
                    Some(t("settings.layout.sidebar_description").into()),
                    switch(
                        "layout-sidebar-visible",
                        layout.sidebar_visible,
                        |value, cx| {
                            config::update(cx, |config| config.layout.sidebar_visible = value);
                        },
                    ),
                )
                .field(
                    t("settings.layout.sidebar_width"),
                    None,
                    stepper(
                        "layout-sidebar-width",
                        layout.sidebar_width,
                        SIDEBAR_WIDTH_MIN,
                        SIDEBAR_WIDTH_MAX,
                        8.0,
                        cx,
                        |value, cx| {
                            config::update(cx, |config| config.layout.set_sidebar_width(value));
                        },
                    ),
                )
                .field(
                    t("settings.layout.aside_visible"),
                    Some(t("settings.layout.aside_description").into()),
                    switch("layout-aside-visible", layout.aside_visible, |value, cx| {
                        config::update(cx, |config| config.layout.aside_visible = value);
                    }),
                )
                .field(
                    t("settings.layout.aside_width"),
                    None,
                    stepper(
                        "layout-aside-width",
                        layout.aside_width,
                        ASIDE_WIDTH_MIN,
                        ASIDE_WIDTH_MAX,
                        8.0,
                        cx,
                        |value, cx| {
                            config::update(cx, |config| config.layout.set_aside_width(value));
                        },
                    ),
                ),
        )
}

/// The keystrokes that reach the shell, read off the command directory — the
/// one table the keymap, the palette and this page all read, so a row here
/// cannot name a chord that does not run. The page writes nothing: chords are
/// the directory's, and a plugin's declared ones live in the plugin view.
fn keys_page(cx: &App) -> Page {
    let keymap = Keymap::default();
    let theme = cx.theme();
    let mut section = Section::new().field(
        t("settings.keys.prefix"),
        None,
        key_cap(keymap.prefix_label(), theme),
    );
    for info in commands::COMMANDS {
        let mut chords: Vec<String> = info
            .direct
            .iter()
            .map(|key| keymap.chord_label(key))
            .collect();
        chords.extend(info.prefixed.iter().map(|key| keymap.chord_label(key)));
        if chords.is_empty() {
            continue;
        }
        section = section.field(
            info.label(),
            None,
            h_flex()
                .gap_1()
                .children(chords.into_iter().map(|chord| key_cap(chord, theme)))
                .into_any_element(),
        );
    }
    page(t("settings.keys.title"), t("settings.keys.description")).section(section)
}

/// What a plugin is on this client, and the one gate between its directory and
/// a run: the review. There are no toggles here because the client offers no
/// choice to toggle — declarations are read, and a link is only a link after
/// its argv has been shown.
fn plugins_page(cx: &App) -> Page {
    let theme = cx.theme();
    page(
        t("settings.plugins.title"),
        t("settings.plugins.description"),
    )
    .section(
        Section::new()
            .field(
                t("settings.plugins.host"),
                None,
                mono_text(t("settings.plugins.host_value"), theme),
            )
            .field(
                t("settings.plugins.daemon_config"),
                None,
                mono_text(platform::herdr_config_path().display().to_string(), theme),
            )
            .field(
                t("settings.plugins.declarations"),
                None,
                mono_text(t("settings.plugins.declarations_value"), theme),
            )
            .field(
                t("settings.plugins.review"),
                None,
                mono_text(t("settings.plugins.review_value"), theme),
            )
            .field(
                t("settings.plugins.sandbox"),
                None,
                mono_text(t("settings.plugins.sandbox_value"), theme),
            )
            .block(note(t("settings.plugins.sandbox_value"), theme.warning)),
    )
}

/// What this window offers an agent, and what a shell-less agent can reach
/// back through: the orchestrator's socket, the tools, and the reverse bridge.
fn mcp_page(cx: &App) -> Page {
    let theme = cx.theme();
    page(t("settings.mcp.title"), t("settings.mcp.description")).section(
        Section::new()
            .field(
                t("settings.mcp.orchestrator"),
                None,
                mono_text(
                    crate::mcp::orchestrator_socket_path().display().to_string(),
                    theme,
                ),
            )
            .field(
                t("settings.mcp.tools"),
                None,
                mono_text(t("settings.mcp.tools_value"), theme),
            )
            .field(
                t("settings.mcp.bridge"),
                None,
                mono_text(t("settings.mcp.bridge_value"), theme),
            ),
    )
}

/// The file itself, and what was wrong with it: the one place a hand-edited
/// value can be seen to have had no effect.
fn config_page(cx: &App) -> Page {
    let theme = cx.theme();
    let diagnostics = config::diagnostics(cx);
    let page = page(t("settings.config.title"), t("settings.config.description")).section(
        Section::new()
            .field(
                t("settings.config.path"),
                None,
                mono_text(config::path_of(cx).display().to_string(), theme),
            )
            .field(t("settings.config.problems"), None, diagnostics_field(cx))
            .field(t("settings.config.daemon"), None, daemon_field(cx)),
    );
    // The nav's warning mark belongs to a config that actually has something
    // to say; a clean file needs no second "no problems" sentence below it.
    if diagnostics.is_empty() {
        page
    } else {
        page.warn()
    }
}

// ── controls ───────────────────────────────────────────────────────────────

const UNDO: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 14 4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/></svg>"#;
const CLOSE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="m7 7 10 10M17 7 7 17"/></svg>"#;
const PLUS: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>"#;
const MINUS: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/></svg>"#;
const WARN: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 8v5M12 17h.01"/><path d="M10.3 4.2 2.7 18a1.8 1.8 0 0 0 1.6 2.7h15.4A1.8 1.8 0 0 0 21.3 18L13.7 4.2a1.9 1.9 0 0 0-3.4 0Z"/></svg>"#;

/// A dropdown that writes one config value. The button wears the current
/// option's label; the menu ticks it, the way the prototype's selects do.
fn dropdown(
    id: &'static str,
    options: Vec<(SharedString, SharedString)>,
    current: String,
    set: impl Fn(SharedString, &mut App) + 'static,
) -> AnyElement {
    let current = SharedString::from(current);
    let label = options
        .iter()
        .find(|(value, _)| *value == current)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| current.clone());
    let set: SetText = Rc::new(set);
    Button::new(id)
        .label(label)
        .dropdown_caret(true)
        .outline()
        .small()
        // The prototype's selects share one column width, so the page reads
        // as a form rather than a row of differently sized buttons.
        .min_w(px(150.))
        .dropdown_menu(move |menu, _, _| {
            options.iter().fold(menu, |menu, (value, label)| {
                let selected = *value == current;
                let value = value.clone();
                let set = set.clone();
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(selected)
                        .on_click(move |_, _, cx| set(value.clone(), cx)),
                )
            })
        })
        .into_any_element()
}

fn switch(id: &'static str, checked: bool, set: impl Fn(bool, &mut App) + 'static) -> AnyElement {
    Switch::new(id)
        .checked(checked)
        .small()
        .on_click(move |value, _, cx| set(*value, cx))
        .into_any_element()
}

/// A whole-pixel number with − and +, for the two widths. The config accepts
/// fractions; a stepper does not produce one.
fn stepper(
    id: &'static str,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    cx: &App,
    set: impl Fn(f32, &mut App) + 'static,
) -> AnyElement {
    let theme = cx.theme();
    let set: SetNumber = Rc::new(set);
    let nudge = |delta: f32| {
        let set = set.clone();
        let next = (value + delta).clamp(min, max).round();
        move |window: &mut Window, cx: &mut App| {
            set(next, cx);
            window.refresh();
        }
    };
    h_flex()
        .gap_1p5()
        .items_center()
        .child(step_button(
            format!("{id}-minus"),
            MINUS,
            value > min,
            theme,
            nudge(-step),
        ))
        .child(
            div()
                .min_w(px(52.))
                .text_size(px(12.))
                .text_color(theme.foreground)
                .child(format!("{value} px")),
        )
        .child(step_button(
            format!("{id}-plus"),
            PLUS,
            value < max,
            theme,
            nudge(step),
        ))
        .into_any_element()
}

fn step_button(
    id: impl Into<ElementId>,
    icon: &'static [u8],
    enabled: bool,
    theme: &Theme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .w(px(24.))
        .h(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_color(if enabled {
            theme.border
        } else {
            theme.border.opacity(0.5)
        })
        .text_color(if enabled {
            theme.foreground
        } else {
            theme.muted_foreground.opacity(0.5)
        })
        .cursor_pointer()
        .when(enabled, |button| {
            button
                .hover(|style| style.bg(theme.list_hover))
                .on_click(move |_, window, cx| on_click(window, cx))
        })
        .child(Icon::default().data(icon).w(px(12.)).h(px(12.)))
        .into_any_element()
}

fn key_cap(label: String, theme: &Theme) -> AnyElement {
    div()
        .px(px(5.))
        .py(px(2.))
        .rounded(px(3.))
        .border_1()
        .border_b_2()
        .border_color(theme.border)
        .font_family(theme.mono_font_family.clone())
        .text_size(px(10.))
        .text_color(theme.muted_foreground)
        .child(label)
        .into_any_element()
}

fn mono_text(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    div()
        .font_family(theme.mono_font_family.clone())
        .text_size(px(11.))
        .text_color(theme.muted_foreground)
        .child(text.into())
        .into_any_element()
}

fn note(text: impl Into<SharedString>, color: Hsla) -> AnyElement {
    h_flex()
        .w_full()
        .gap_2()
        .px(px(10.))
        .py(px(8.))
        .rounded(px(5.))
        .border_1()
        .border_color(color.opacity(0.32))
        .bg(color.opacity(0.08))
        .text_size(px(11.5))
        .text_color(color)
        .child(Icon::default().data(WARN).w(px(13.)).h(px(13.)).flex_none())
        .child(div().flex_1().child(text.into()))
        .into_any_element()
}

/// Every problem the last load or save found, or a line saying there is none.
fn diagnostics_field(cx: &App) -> AnyElement {
    let theme = cx.theme();
    let diagnostics = config::diagnostics(cx);
    if diagnostics.is_empty() {
        return mono_text(t("settings.config.no_problems"), theme);
    }
    let mut list = v_flex().gap_1();
    for diagnostic in diagnostics {
        let color = if diagnostic.is_blocking() {
            theme.danger
        } else {
            theme.warning
        };
        let text = if diagnostic.path.is_empty() {
            diagnostic.message.clone()
        } else {
            format!("{}: {}", diagnostic.path, diagnostic.message)
        };
        list = list.child(
            div()
                .w_full()
                .text_size(px(11.))
                .text_color(color)
                .child(text),
        );
    }
    list.into_any_element()
}

/// The daemon's own file, named so a setting that is not here can be found
/// where it does live (see `crate::config`'s module docs).
fn daemon_field(cx: &App) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .gap_1()
        .child(mono_text(
            platform::herdr_config_path().display().to_string(),
            theme,
        ))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.muted_foreground)
                .child(t("settings.config.daemon_hint")),
        )
        .into_any_element()
}

/// A borderless icon button: the dialog's close and the page's revert.
fn icon_button(
    id: impl Into<ElementId>,
    icon: &'static [u8],
    color: Hsla,
    hover: Hsla,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .w(px(26.))
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .cursor_pointer()
        .text_color(color)
        .hover(move |style| style.bg(hover))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(Icon::default().data(icon).w(px(14.)).h(px(14.)))
        .into_any_element()
}

// ── the surface ────────────────────────────────────────────────────────────

/// The settings dialog, mounted over the shell while it is open. `focus` is
/// the shell's, so the pane beside it stops taking keystrokes; Escape (or the
/// close button) hands it back.
pub(crate) fn overlay(
    sections: &'static [SettingsSection],
    selected: usize,
    focus: &FocusHandle,
    dispatch: &Dispatch,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let pages: Vec<Page> = sections.iter().map(|section| (section.page)(cx)).collect();
    let selected = selected.min(pages.len().saturating_sub(1));
    // The nav reads names and marks off the pages before the open one is
    // consumed: its controls are elements and move into the frame.
    let entries: Vec<(SharedString, bool)> = pages
        .iter()
        .map(|page| (page.title.clone(), page.warn))
        .collect();
    let open = pages.into_iter().nth(selected);
    let escape = dispatch.clone();
    let close = dispatch.clone();
    let footer_close = dispatch.clone();
    let scrim = if theme.is_dark() {
        black().opacity(0.45)
    } else {
        black().opacity(0.22)
    };
    let dialog = v_flex()
        .w_full()
        .max_w(px(1000.))
        .h_full()
        .max_h(px(660.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .overflow_hidden()
        .shadow(vec![
            BoxShadow::new(px(0.), px(18.), black().opacity(0.45)).blur_radius(px(48.)),
            BoxShadow::new(px(0.), px(2.), black().opacity(0.3)).blur_radius(px(8.)),
        ])
        .child(dialog_head(&theme, &close, cx))
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(nav(&entries, selected, &theme, dispatch))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_y_scrollbar()
                        .children(open.map(|page| render_page(page, &theme))),
                ),
        )
        .child(
            h_flex()
                .flex_none()
                .w_full()
                .h(px(40.))
                .px_3()
                .gap_2()
                .items_center()
                .border_t_1()
                .border_color(theme.border)
                .text_size(px(11.))
                .text_color(theme.muted_foreground)
                .child(t("settings.effective"))
                .child(div().flex_1())
                .child(
                    Button::new("settings-footer-close")
                        .label(t("settings.close"))
                        .ghost()
                        .small()
                        .on_click(move |_, window, cx| {
                            footer_close(
                                ShellRequest::Command(Command::ToggleSettings),
                                window,
                                cx,
                            );
                        }),
                ),
        );
    v_flex()
        .id("settings-overlay")
        .absolute()
        .inset_0()
        .occlude()
        .size_full()
        .bg(scrim)
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.))
        .track_focus(focus)
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                escape(ShellRequest::Command(Command::ToggleSettings), window, cx);
            }
        })
        .child(dialog)
        .into_any_element()
}

fn dialog_head(theme: &Theme, close: &Dispatch, cx: &App) -> AnyElement {
    let close = close.clone();
    h_flex()
        .flex_none()
        .w_full()
        .h(px(40.))
        .px(px(12.))
        .gap_2()
        .items_center()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(t("settings.title")),
        )
        .child(
            div()
                .px(px(6.))
                .py(px(1.))
                .rounded_full()
                .border_1()
                .border_color(theme.border)
                .bg(theme.muted)
                .font_family(theme.mono_font_family.clone())
                .text_size(px(10.5))
                .text_color(theme.muted_foreground)
                .child(config::path_of(cx).display().to_string()),
        )
        .child(div().flex_1())
        .child(icon_button(
            "settings-close",
            CLOSE,
            theme.muted_foreground,
            theme.list_hover,
            move |window, cx| {
                close(ShellRequest::Command(Command::ToggleSettings), window, cx);
            },
        ))
        .into_any_element()
}

fn nav(
    entries: &[(SharedString, bool)],
    selected: usize,
    theme: &Theme,
    dispatch: &Dispatch,
) -> AnyElement {
    v_flex()
        .flex_none()
        .w(px(190.))
        .h_full()
        .p(px(6.))
        .gap_0p5()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.sidebar)
        .children(entries.iter().enumerate().map(|(index, (title, warn))| {
            let dispatch = dispatch.clone();
            let active = index == selected;
            h_flex()
                .id(("settings-nav", index))
                .w_full()
                .h(px(30.))
                .px_2()
                .gap_2()
                .items_center()
                .rounded(px(5.))
                .cursor_pointer()
                .text_size(px(12.))
                .bg(if active {
                    theme.sidebar_accent
                } else {
                    theme.sidebar
                })
                .text_color(if active {
                    theme.sidebar_accent_foreground
                } else {
                    theme.sidebar_foreground
                })
                .when(!active, |row| {
                    row.hover(|style| style.bg(theme.sidebar_accent))
                })
                .on_click(move |_, window, cx| {
                    dispatch(ShellRequest::ShowSettingsPage(index), window, cx);
                })
                .child(div().flex_1().child(title.clone()))
                .children(warn.then(|| {
                    div()
                        .text_size(px(10.5))
                        .text_color(theme.warning)
                        .child("!")
                }))
        }))
        .into_any_element()
}

fn render_page(page: Page, theme: &Theme) -> AnyElement {
    let mut column = v_flex().w_full().px(px(18.)).pt(px(14.)).pb(px(24.)).child(
        h_flex()
            .w_full()
            .items_start()
            .gap_2()
            .child(
                v_flex()
                    .flex_1()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(page.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted_foreground)
                            .child(page.description.clone()),
                    ),
            )
            .children(page.reset.as_ref().map(|reset| {
                let reset = reset.clone();
                icon_button(
                    "settings-page-reset",
                    UNDO,
                    theme.muted_foreground,
                    theme.list_hover,
                    move |window, cx| {
                        reset(cx);
                        window.refresh();
                    },
                )
            }))
            // The prototype puts one field's worth of air under the page's
            // description before the first row's hairline.
            .mb(px(14.)),
    );
    for section in page.sections {
        if let Some(title) = section.title {
            column = column.child(
                div()
                    .mt(px(16.))
                    .mb(px(2.))
                    .text_size(px(10.))
                    .text_color(theme.muted_foreground)
                    .child(title),
            );
        }
        let last = section.rows.len().saturating_sub(1);
        for (index, row) in section.rows.into_iter().enumerate() {
            let control = match row.label {
                Some(label) => h_flex()
                    .w_full()
                    .items_start()
                    .gap(px(14.))
                    .py(px(10.))
                    .when(index != last, |row| {
                        row.border_b_1().border_color(theme.border)
                    })
                    .child(
                        v_flex()
                            .w(px(190.))
                            .flex_none()
                            .gap(px(2.))
                            .child(div().text_size(px(12.5)).child(label))
                            .children(row.description.map(|description| {
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme.muted_foreground)
                                    .child(description)
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_wrap()
                            .gap_1p5()
                            .items_center()
                            .child(row.control),
                    )
                    .into_any_element(),
                None => div()
                    .w_full()
                    .py(px(6.))
                    .child(row.control)
                    .into_any_element(),
            };
            column = column.child(control);
        }
    }
    column.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    // `use super::*` re-exports GPUI's `test` attribute macro, which would
    // otherwise shadow the builtin one and recurse: name the builtin.
    use std::prelude::v1::test;

    #[test]
    fn every_page_has_a_unique_id() {
        let mut ids: Vec<&str> = SETTINGS_SECTIONS.iter().map(|section| section.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "settings page ids are not unique");
    }

    #[test]
    fn the_config_page_can_be_named() {
        let index = page_index("config").expect("the config page is part of the surface");
        assert_eq!(SETTINGS_SECTIONS[index].id, "config");
        assert!(page_index("no-such-page").is_none());
    }
}
