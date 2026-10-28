//! The settings surface: seven pages over one dialog, written as they change.
//!
//! The design's dialog is a 1080×780 card over a scrim, a 196px page column on
//! its left and cards of setting rows on its right (`docs/ui/redesign/app.css`:
//! `.dialog`, `.settings-nav`, `.settings-card`). Every control writes
//! `config.json` through [`config::update`] the moment it changes — in-memory
//! value, file and every window move together, and there is no global save
//! button. Only the two pages that own client settings carry a page-level
//! reset; the other five state facts and would have nothing to put back. The
//! frame's own spacing is tight on purpose: at the 780px card height the
//! appearance page's seven rows and three cards have to fit without the last
//! control (density) falling below the fold, so cards sit at `py_1p5`, pages
//! at `gap_1p5`, and the heading gives back the air a page title does not
//! need.
//!
//! ## What each page owns
//!
//! | Page | Acts on | Writes |
//! |---|---|---|
//! | appearance | the chrome's four layers, language, density | `appearance.*` |
//! | terminal | the grid's metrics and cursor, whether it follows a light chrome | `terminal.*`, `appearance.terminal_follows_mode` |
//! | ai | the orchestrator's provider and reasoning effort | `api-providers.json` (the chat's own file, not this client's config) |
//! | keys | the command directory | nothing: the rows are read off [`commands::COMMANDS`] and the keymap |
//! | plugins | the daemon's plugin registry | nothing: host facts and the registry summary |
//! | mcp | the tools this window offers an agent | nothing: the socket, the tool list, the reverse bridge |
//! | about | this build, its licenses and files | nothing |
//!
//! The plugins and MCP pages deliberately have no switches. A disabled plugin,
//! an unlink, a sandbox: none of those are this client's to change (the daemon
//! owns plugin state and runs the commands), so the pages state the fact and
//! leave the action where it exists. The same rule keeps the page widths out of
//! the appearance page: `layout.sidebar_width` is not read by the rebuilt
//! shell, and a control that changes nothing is a lie.
//!
//! ## Which library pieces this page uses, and which it does not
//!
//! `SettingsSection` / `SettingsRow` are the row vocabulary; `ThemeSelector`
//! and `AccentColorPicker` are the two appearance controls that draw exactly
//! what the design asks for. Three pieces from the same crate are not used:
//! `SyntaxThemePicker` prints the library's own engineering names on its cards
//! (`Ely`), which this client must not show; `DensitySelector` offers three
//! densities whose labels are English literals, while the config has two and
//! this client translates them; `SettingsLayout`'s page column has no group
//! heads and cannot be held to the design's 196px, so the column is composed
//! here from the same list primitives' look. The accented code-theme chips are
//! drawn here for the same naming reason, with the sample colors read off the
//! same `Syntax` values the picker would have used.
//!
//! ## Focus
//!
//! The dialog traps Tab inside itself, takes focus when it opens and hands it
//! back to whatever held it (the pane grid) when it closes — the library's
//! `take_focus` / `give_back`, the same pair the palette and the library's
//! dialogs use. Escape closes it from anywhere inside; a `Select` that is open
//! takes Escape first, because its handler sits deeper in the focus path.

use std::rc::Rc;
use std::sync::OnceLock;

use ely_core::{
    buttons::{Button, ButtonVariant, IconButton, SegmentedControl},
    forms::{Choice, Select, Stepper, Switch},
    layout::ScrollArea,
    primitives::{Backdrop, FocusRing, FocusScope, Icon, IconName, give_back, take_focus},
    theme::{ActiveTheme, ControlSize, Elevation, IconSize, Mode, Radius, TextSize},
    typography::{Ellipsis, KbdCombo},
};
use ely_settings::{
    AccentColorPicker, Appearance as PickedAppearance, SettingsRow, SettingsSection, ThemeSelector,
};
use gpui::{
    AnyElement, App, Entity, Hsla, IntoElement, ParentElement, Role, SharedString, Styled, Window,
    div, prelude::*, px,
};

use super::commands;
use super::keymap::Keymap;
use super::shell::Root;
use super::theme::{Accent, CodeTheme, ThemeSettings, tone_palette};
use crate::api_provider::{self, ApiProviderConfig, ThinkingLevel};
use crate::config;
use crate::herdr::LiveState;
use crate::i18n::t;
use crate::plugins::actions::Catalog;
use crate::plugins::host::PluginState;

/// The dialog's widest card, and the shape the design draws (1080×780).
const DIALOG_WIDTH: f32 = 1080.;
const DIALOG_HEIGHT: f32 = 780.;
/// The page column's width (the design's 196px).
const NAV_WIDTH: f32 = 196.;
/// The margin the card keeps from the window at its smallest: below the
/// design's size the card shrinks instead of being clipped.
const DIALOG_MARGIN: f32 = 48.;

/// A page of the settings dialog, in the nav's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Page {
    /// The chrome's four layers, language and density.
    #[default]
    Appearance,
    /// The grid's metrics, cursor and palette behavior.
    Terminal,
    /// The orchestrator's provider file.
    Ai,
    /// The command directory's chords, read-only.
    Keys,
    /// The daemon's plugin host, read-only.
    Plugins,
    /// The tools this window offers an agent, read-only.
    Mcp,
    /// The build, its licenses and its files.
    About,
}

impl Page {
    /// The page's id: the `settings.<id>.*` key stem and the nav item's id.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Page::Appearance => "appearance",
            Page::Terminal => "terminal",
            Page::Ai => "ai",
            Page::Keys => "keys",
            Page::Plugins => "plugins",
            Page::Mcp => "mcp",
            Page::About => "about",
        }
    }

    /// What the page and its nav row are called.
    fn title(self) -> String {
        t(&format!("settings.{}.title", self.id()))
    }

    /// The line under the page's title.
    fn description(self) -> String {
        t(&format!("settings.{}.description", self.id()))
    }

    /// The nav's glyph.
    fn icon(self) -> IconName {
        match self {
            Page::Appearance => IconName::Palette,
            Page::Terminal => IconName::Terminal,
            Page::Ai => IconName::Sparkles,
            Page::Keys => IconName::Command,
            Page::Plugins => IconName::Puzzle,
            Page::Mcp => IconName::Link,
            Page::About => IconName::Info,
        }
    }

    /// Whether the page owns client settings a reset can put back. The
    /// reference pages state facts and have nothing to restore.
    fn resettable(self) -> bool {
        matches!(self, Page::Appearance | Page::Terminal)
    }
}

/// The nav's three groups: what the client runs on, what extends it, and the
/// build itself.
const NAV_GROUPS: [(Page, &[Page], &str); 3] = [
    (
        Page::Appearance,
        &[Page::Appearance, Page::Terminal, Page::Ai],
        "settings.group.app",
    ),
    (
        Page::Keys,
        &[Page::Keys, Page::Plugins, Page::Mcp],
        "settings.group.extensions",
    ),
    (Page::About, &[Page::About], "settings.group.about"),
];

/// The dialog's own state. The pages' controls are built from the config and
/// the registry each frame; only what has no owner elsewhere lives here.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) open: bool,
    pub(crate) page: Page,
    /// The provider the AI page shows, by name. Page-local on purpose: the
    /// chat panel picks the provider it runs on, and a dialog is not a second
    /// authority over that choice.
    provider: Option<String>,
    /// The provider file's contents, read when the dialog opens. A paint never
    /// touches the disk.
    providers: Vec<ApiProviderConfig>,
    /// The last provider write that failed, shown on the AI page until one
    /// succeeds. `save_all` is a file operation with a real failure mode.
    error: Option<String>,
}

impl State {
    /// Read the provider file, once per opening. The chat panel reads the same
    /// file when it starts; this is the dialog's copy, not a second truth.
    pub(crate) fn reload(&mut self) {
        self.providers = api_provider::load_all();
        self.error = None;
    }

    /// The provider the AI page shows: the one picked, else the first usable
    /// one (the rule the chat panel applies), else the first configured.
    pub(crate) fn selected(&self) -> Option<&ApiProviderConfig> {
        self.provider
            .as_deref()
            .and_then(|name| self.providers.iter().find(|provider| provider.name == name))
            .or_else(|| {
                self.providers
                    .iter()
                    .find(|provider| !provider.api_key.trim().is_empty())
            })
            .or_else(|| self.providers.first())
    }

    /// Show `name` on the AI page. A name the file no longer carries falls
    /// back to what `selected` would pick rather than pinning nothing.
    pub(crate) fn choose_provider(&mut self, name: &str) {
        if self.providers.iter().any(|provider| provider.name == name) {
            self.provider = Some(name.to_owned());
            self.error = None;
        }
    }

    /// Persist one provider's reasoning effort. The chat panel reads the file
    /// when it starts, so this is the level its picker opens on.
    pub(crate) fn set_level(&mut self, token: &str, name: &str) {
        let Some(level) = ThinkingLevel::parse_token(token) else {
            return;
        };
        let Some(provider) = self
            .providers
            .iter_mut()
            .find(|provider| provider.name == name)
        else {
            return;
        };
        provider.thinking_level = level;
        match api_provider::save_all(&self.providers) {
            Ok(()) => self.error = None,
            Err(error) => {
                // The file is the truth: show what it now holds, not what the
                // press tried to write.
                self.error = Some(error);
                self.providers = api_provider::load_all();
            }
        }
    }
}

/// Put back the settings a page owns. The appearance page resets only the
/// layers it draws (the terminal's follow switch belongs to the terminal
/// page), and the terminal page resets the grid's metrics and that switch.
/// The reference pages own nothing and are not offered the button.
pub(crate) fn reset_page(page: Page, config: &mut config::ConfigFile) {
    match page {
        Page::Appearance => {
            let defaults = config::Appearance::default();
            let appearance = &mut config.appearance;
            appearance.mode = defaults.mode;
            appearance.accent = defaults.accent;
            appearance.syntax = defaults.syntax;
            appearance.locale = defaults.locale;
            appearance.density = defaults.density;
        }
        Page::Terminal => {
            config.terminal = config::TerminalConfig::default();
            config.appearance.terminal_follows_mode =
                config::Appearance::default().terminal_follows_mode;
        }
        Page::Ai | Page::Keys | Page::Plugins | Page::Mcp | Page::About => {}
    }
}

/// The dialog's one way out: hand focus back, then run the owner's close.
type Close = Rc<dyn Fn(&mut Window, &mut App)>;

/// What the pages read that the shell owns: the daemon's state, the plugin
/// registry, the keymap and whether the orchestrator socket is serving. They
/// are handed over together so the overlay's signature stays one row per
/// surface rather than one per source.
pub(crate) struct Sources<'a> {
    pub(crate) live: &'a LiveState,
    pub(crate) plugins: &'a PluginState,
    pub(crate) keymap: &'a Keymap,
    pub(crate) orchestrator: bool,
}

/// The dialog over the window. It is drawn only while it is open, which is why
/// the focus takeover can live here: closing it drops the takeover with the
/// element, and reopening takes focus again.
pub(crate) fn overlay(
    window: &mut Window,
    state: &State,
    sources: Sources<'_>,
    root: &Entity<Root>,
    cx: &mut App,
) -> AnyElement {
    let page = state.page;
    let content = match page {
        Page::Appearance => page_frame(page, appearance_cards(window, cx), root, cx),
        Page::Terminal => page_frame(page, terminal_cards(cx), root, cx),
        Page::Ai => page_frame(page, ai_cards(state, root, cx), root, cx),
        Page::Keys => page_frame(
            page,
            keys_cards(sources.plugins, sources.keymap, cx),
            root,
            cx,
        ),
        Page::Plugins => page_frame(page, plugins_cards(sources.plugins, cx), root, cx),
        Page::Mcp => page_frame(
            page,
            mcp_cards(sources.plugins, sources.orchestrator, cx),
            root,
            cx,
        ),
        Page::About => page_frame(page, about_cards(sources.live, cx), root, cx),
    };

    let takeover = take_focus("settings-dialog-takeover", window, cx);
    let focus = takeover.read(cx).focus.clone();
    let close: Close = {
        let root = root.clone();
        let takeover = takeover.clone();
        Rc::new(move |window, cx| {
            give_back(&takeover, window, cx);
            root.update(cx, |root, cx| root.close_settings(cx));
        })
    };

    let viewport = window.viewport_size();
    let width = (viewport.width - px(DIALOG_MARGIN)).min(px(DIALOG_WIDTH));
    let height = (viewport.height - px(DIALOG_MARGIN)).min(px(DIALOG_HEIGHT));
    let card = dialog_card(
        page,
        content,
        gpui::size(width, height),
        root,
        close.clone(),
        window,
        cx,
    );

    let escaped = close.clone();
    Backdrop::new("settings-scrim")
        .on_dismiss(move |window, cx| close(window, cx))
        .child(
            div()
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        escaped(window, cx);
                    }
                })
                .child(
                    FocusScope::new(&focus)
                        .trap()
                        .node(
                            "settings-dialog-scope",
                            Role::Dialog,
                            t("settings.title"),
                            Some(SharedString::from(page.title())),
                        )
                        .child(card),
                ),
        )
        .into_any_element()
}

/// The card itself: the head naming the file, the page column, the open page
/// and the foot stating that the file is already written.
fn dialog_card(
    page: Page,
    content: AnyElement,
    size: gpui::Size<gpui::Pixels>,
    root: &Entity<Root>,
    close: Close,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let nav = nav(page, root, window, cx);
    let theme = cx.theme();
    let colors = &theme.colors;
    let path = config::path_of(cx).display().to_string();
    let done = {
        let close = close.clone();
        Button::new("settings-dialog-done", t("settings.close"))
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Sm)
            .on_click(move |_, window, cx| close(window, cx))
    };
    let x = {
        let close = close.clone();
        IconButton::new("settings-dialog-close", IconName::X)
            .variant(ButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .tooltip(t("settings.close"))
            .on_click(move |_, window, cx| close(window, cx))
    };
    div()
        .w(size.width)
        .h(size.height)
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(theme.radius(Radius::Xl))
        .border_1()
        .border_color(colors.border_strong)
        .bg(colors.bg)
        .shadow(theme.elevation(Elevation::Modal))
        .child(
            div()
                .h(px(52.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .border_b_1()
                .border_color(colors.border)
                .child(
                    div()
                        .flex_none()
                        .text_size(theme.text_size(TextSize::Lg))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(colors.fg)
                        .child(t("settings.title")),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.fg_subtle)
                        .child(Ellipsis::new(SharedString::from(path))),
                )
                .child(div().flex_1())
                .child(x),
        )
        .child(div().flex_1().min_h_0().flex().child(nav).child(content))
        .child(
            div()
                .h(px(46.))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .border_t_1()
                .border_color(colors.border)
                .child(div().size(px(6.)).rounded_full().bg(colors.success))
                .child(
                    div()
                        .min_w_0()
                        .text_size(theme.text_size(TextSize::Xs))
                        .text_color(colors.success)
                        .child(Ellipsis::new(SharedString::from(t("settings.saved")))),
                )
                .child(div().flex_1())
                .child(done),
        )
        .into_any_element()
}

/// The page column: three groups of destinations, the open one carrying the
/// design's 2px accent bar at its left edge.
fn nav(selected: Page, root: &Entity<Root>, window: &mut Window, cx: &mut App) -> AnyElement {
    // The column's own tokens are copied before any row takes the window: a
    // tab stop needs `&mut App`, and the theme borrow cannot survive it.
    let (border, subtle, label_size) = {
        let theme = cx.theme();
        (
            theme.colors.border,
            theme.colors.fg_subtle,
            theme.text_size(TextSize::Xs),
        )
    };
    let mut column = div()
        .w(px(NAV_WIDTH))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .gap_0p5()
        .px_2p5()
        .py_2()
        .border_r_1()
        .border_color(border);
    for (_, pages, group_key) in NAV_GROUPS {
        if pages.is_empty() {
            continue;
        }
        column = column.child(
            div()
                .flex_none()
                .px_2()
                .pt_2()
                .pb_1()
                .text_size(label_size)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(subtle)
                .child(t(group_key)),
        );
        for &page in pages {
            column = column.child(nav_item(page, page == selected, root, window, cx));
        }
    }
    column.into_any_element()
}

fn nav_item(
    page: Page,
    selected: bool,
    root: &Entity<Root>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let focus = ely_core::primitives::tab_stop(
        format!("settings-nav-focus-{}", page.id()).into(),
        true,
        window,
        cx,
    );
    let theme = cx.theme();
    let colors = &theme.colors;
    let hover = colors.hover;
    let (ink, glyph) = if selected {
        (colors.fg, colors.fg)
    } else {
        (colors.fg_muted, colors.fg_subtle)
    };
    let clicked = root.clone();
    let typed = root.clone();
    div()
        .id(format!("settings-nav-{}", page.id()))
        .role(Role::Button)
        .aria_label(page.title())
        .track_focus(&focus)
        .focus_ring(cx)
        .border_1()
        .border_color(gpui::transparent_black())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .h(px(32.))
        .px_2()
        .rounded(theme.radius(Radius::Md))
        .cursor_pointer()
        .when(selected, |item| item.bg(colors.surface))
        .when(!selected, |item| item.hover(move |style| style.bg(hover)))
        .on_click(move |_, _, cx| {
            clicked.update(cx, |root, cx| root.set_settings_page(page, cx));
        })
        .on_key_down(move |event, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                typed.update(cx, |root, cx| root.set_settings_page(page, cx));
            }
        })
        .when(selected, |item| {
            item.child(
                div()
                    .absolute()
                    .left_0()
                    .top_1()
                    .bottom_1()
                    .w(px(2.))
                    .rounded_full()
                    .bg(colors.accent),
            )
        })
        .child(Icon::new(page.icon()).size(IconSize::Sm).color(glyph))
        .child(
            div()
                .min_w_0()
                .text_size(theme.text_size(TextSize::Sm))
                .text_color(ink)
                .child(Ellipsis::new(SharedString::from(page.title()))),
        )
        .into_any_element()
}

/// The open page: its title and the one line under it, the reset where the
/// page owns settings, and a scrolling column of cards.
fn page_frame(page: Page, cards: Vec<AnyElement>, root: &Entity<Root>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let mut head = div().flex_1().min_w_0().flex().flex_col().gap_0p5().child(
        div()
            .text_size(theme.text_size(TextSize::Lg))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(colors.fg)
            .child(page.title()),
    );
    head = head.child(
        div()
            .text_size(theme.text_size(TextSize::Xs))
            .text_color(colors.fg_muted)
            .child(page.description()),
    );
    let mut heading = div()
        .flex()
        .items_start()
        .gap_3()
        .px_5()
        .pt_2()
        .pb_0()
        .child(head);
    if page.resettable() {
        let root = root.clone();
        heading = heading.child(
            div().flex_none().child(
                Button::new(
                    format!("settings-reset-{}", page.id()),
                    t("settings.reset_page"),
                )
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .on_click(move |_, _, cx| {
                    root.update(cx, |root, cx| root.reset_settings_page(cx));
                }),
            ),
        );
    }
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .child(heading)
        .child(
            div().flex_1().min_h_0().px_5().pb_0().child(
                ScrollArea::new(format!("settings-page-{}", page.id()))
                    .vertical()
                    .size_full()
                    // The scrollbar is an overlay at the area's right edge; the
                    // gutter keeps it off the cards' border.
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .pb_1()
                            .pr_3()
                            .children(cards),
                    ),
            ),
        )
        .into_any_element()
}

/// One setting: its name, a line on what it does, and the control at the end
/// of the line.
fn row(
    title: impl Into<SharedString>,
    description: Option<String>,
    control: Option<AnyElement>,
) -> AnyElement {
    let mut row = SettingsRow::new(title);
    if let Some(description) = description {
        row = row.description(description);
    }
    if let Some(control) = control {
        row = row.control(control);
    }
    row.into_any_element()
}

/// A card: the design's rounded surface around one section of rows.
fn card(
    title: impl Into<SharedString>,
    description: Option<String>,
    rows: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let mut section = SettingsSection::new(title);
    if let Some(description) = description {
        section = section.description(description);
    }
    for row in rows {
        section = section.row(row);
    }
    div()
        .flex_none()
        .rounded(theme.radius(Radius::Lg))
        .border_1()
        .border_color(theme.colors.border)
        .bg(theme.colors.surface)
        .text_color(theme.colors.fg)
        .px_4()
        .py_1p5()
        .child(section)
        .into_any_element()
}

/// A read-only value at a row's end; monospace for paths, ids and versions.
fn value(text: String, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .max_w(px(380.))
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(theme.colors.fg_muted)
        .child(Ellipsis::new(SharedString::from(text)))
        .into_any_element()
}

/// A state marker: a success dot's word, or a quiet one.
fn badge(text: String, ok: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let colors = &theme.colors;
    let (ink, ground) = if ok {
        (colors.success, colors.success_subtle)
    } else {
        (colors.fg_muted, colors.sunken)
    };
    div()
        .flex_none()
        .px_1p5()
        .h(px(18.))
        .flex()
        .items_center()
        .rounded(theme.radius(Radius::Sm))
        .bg(ground)
        .text_size(theme.text_size(TextSize::Xs))
        .text_color(ink)
        .child(text)
        .into_any_element()
}

/// The key cap a chord is drawn as, or nothing for a command only the palette
/// reaches.
fn chord(label: Option<String>) -> Option<AnyElement> {
    label.map(|label| KbdCombo::new(&label).into_any_element())
}

// ── appearance ───────────────────────────────────────────────────────

fn appearance_cards(window: &mut Window, cx: &mut App) -> Vec<AnyElement> {
    // The values are copied out before the window is borrowed: a control that
    // is a tab stop needs it, and the config cannot stay borrowed across that.
    let (mode, syntax, locale, density) = {
        let appearance = &config::get(cx).appearance;
        (
            appearance.mode,
            appearance.syntax,
            appearance.locale.clone(),
            appearance.density.clone(),
        )
    };
    let theme_rows = vec![
        row(
            t("settings.appearance.theme"),
            Some(t("settings.appearance.theme_description")),
            Some(mode_control(mode)),
        ),
        row(
            t("settings.appearance.accent"),
            Some(t("settings.appearance.accent_description")),
            Some(accent_control(cx)),
        ),
        row(
            t("settings.appearance.syntax"),
            Some(t("settings.appearance.syntax_description")),
            Some(syntax_chips(syntax_of(syntax), window, cx)),
        ),
    ];

    let interface_rows = vec![
        row(
            t("settings.appearance.locale"),
            Some(t("settings.appearance.locale_description")),
            Some(locale_control(&locale)),
        ),
        row(
            t("settings.appearance.density"),
            Some(t("settings.appearance.density_description")),
            Some(density_control(&density)),
        ),
    ];

    vec![
        card(
            t("settings.appearance.theme_card"),
            Some(t("settings.appearance.theme_card_note")),
            theme_rows,
            cx,
        ),
        card(
            t("settings.appearance.interface_card"),
            Some(t("settings.appearance.interface_card_note")),
            interface_rows,
            cx,
        ),
    ]
}

/// The mode's three segments, in the library's own control and its own words.
fn mode_control(mode: config::ThemeMode) -> AnyElement {
    let picked = match mode {
        config::ThemeMode::Light => PickedAppearance::Light,
        config::ThemeMode::Dark => PickedAppearance::Dark,
        config::ThemeMode::System => PickedAppearance::System,
    };
    ThemeSelector::new("settings-mode", picked, |picked, _, cx| {
        let mode = match picked {
            PickedAppearance::Light => config::ThemeMode::Light,
            PickedAppearance::Dark => config::ThemeMode::Dark,
            PickedAppearance::System => config::ThemeMode::System,
        };
        config::update(cx, |config| config.appearance.mode = mode);
    })
    .into_any_element()
}

/// The accent's named presets and the color well beside them. Each press is
/// named back to the config: a preset stays its name, the well becomes
/// `#rrggbb`, so the file always says which of the two the owner picked.
fn accent_control(cx: &App) -> AnyElement {
    let mode = cx.theme().mode();
    let palette = tone_palette(mode);
    let current = ThemeSettings::from_config(&config::get(cx).appearance)
        .accent
        .color(&palette);
    let presets: Vec<(String, Hsla)> = Accent::PRESETS
        .into_iter()
        .map(|accent| (t(accent.label_key()), accent.color(&palette)))
        .collect();
    AccentColorPicker::new("settings-accent", current, presets, move |color, _, cx| {
        let choice = accent_choice(color, mode);
        config::update(cx, |config| config.appearance.accent = choice);
    })
    .into_any_element()
}

/// Which config accent a color picked off the control names. The presets are
/// compared in the mode the picker was drawn in, which is the mode whose
/// colors it showed; anything else is the well's, kept as `#rrggbb`.
fn accent_choice(color: Hsla, mode: Mode) -> config::AccentChoice {
    let palette = tone_palette(mode);
    for accent in Accent::PRESETS {
        if accent.color(&palette) == color {
            let name = match accent {
                Accent::Default => "default",
                Accent::Blue => "blue",
                Accent::Teal => "teal",
                Accent::Amber => "amber",
                Accent::Rose => "rose",
                Accent::Violet => "violet",
                Accent::Green => "green",
                // `PRESETS` holds no custom accent.
                Accent::Custom(_) => "default",
            };
            return config::AccentChoice::parse(name).unwrap_or_default();
        }
    }
    config::AccentChoice::Custom(hex(color))
}

/// A color as the config spells a custom accent: lowercase `#rrggbb`, the
/// same rounding [`super::theme`] packs with.
fn hex(color: Hsla) -> String {
    let rgb = color.to_rgb();
    let byte = |channel: f32| (channel.clamp(0., 1.) * 255.).round() as u32;
    format!("#{:02x}{:02x}{:02x}", byte(rgb.r), byte(rgb.g), byte(rgb.b))
}

/// The config's code-theme choice, as the appearance page's own enum.
fn syntax_of(choice: config::SyntaxChoice) -> CodeTheme {
    match choice {
        config::SyntaxChoice::Standard => CodeTheme::Standard,
        config::SyntaxChoice::Quiet => CodeTheme::Quiet,
        config::SyntaxChoice::Paper => CodeTheme::Paper,
    }
}

/// The id stem of one code theme. The library's own name for the set is an
/// engineering name and never reaches an element id or a reader.
fn code_id(code: CodeTheme) -> &'static str {
    match code {
        CodeTheme::Standard => "standard",
        CodeTheme::Quiet => "quiet",
        CodeTheme::Paper => "paper",
    }
}

/// Three code-theme chips, each previewing the set in three of its inks. The
/// library's picker draws the same idea as cards labelled with its own theme
/// names; this client names the themes only by their translated names.
fn syntax_chips(selected: CodeTheme, window: &mut Window, cx: &mut App) -> AnyElement {
    // Tokens are copied out first: each chip is a tab stop, which needs the
    // window, and the theme borrow cannot span that.
    let mode = cx.theme().mode();
    let (accent, border, surface, bg, hover, fg, muted, radius, label_size) = {
        let theme = cx.theme();
        (
            theme.colors.accent,
            theme.colors.border,
            theme.colors.surface,
            theme.colors.bg,
            theme.colors.hover,
            theme.colors.fg,
            theme.colors.fg_muted,
            theme.radius(Radius::Md),
            theme.text_size(TextSize::Xs),
        )
    };
    let chips = CodeTheme::ALL.into_iter().map(|code| {
        let ink = code.syntax(mode);
        let on = code == selected;
        let label = t(code.label_key());
        let border = if on { accent } else { border };
        let background = if on { surface } else { bg };
        let focus = ely_core::primitives::tab_stop(
            format!("settings-syntax-focus-{}", code_id(code)).into(),
            true,
            window,
            cx,
        );
        div()
            .id(format!("settings-syntax-{}", code_id(code)))
            .role(Role::Button)
            .aria_label(label.clone())
            .track_focus(&focus)
            .focus_ring(cx)
            .border_1()
            .border_color(gpui::transparent_black())
            .flex()
            .flex_none()
            .items_center()
            .gap_1p5()
            .h(px(24.))
            .px_2()
            .rounded(radius)
            .border_1()
            .border_color(border)
            .bg(background)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .on_click(move |_, _, cx| {
                let choice = match code {
                    CodeTheme::Standard => config::SyntaxChoice::Standard,
                    CodeTheme::Quiet => config::SyntaxChoice::Quiet,
                    CodeTheme::Paper => config::SyntaxChoice::Paper,
                };
                config::update(cx, |config| config.appearance.syntax = choice);
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(div().size(px(5.)).rounded_full().bg(ink.keyword))
                    .child(div().size(px(5.)).rounded_full().bg(ink.string))
                    .child(div().size(px(5.)).rounded_full().bg(ink.function)),
            )
            .child(
                div()
                    .text_size(label_size)
                    .text_color(if on { fg } else { muted })
                    .child(label),
            )
    });
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .children(chips)
        .into_any_element()
}

/// The UI language. Each language names itself, which is the one string a
/// locale must not translate.
fn locale_control(locale: &str) -> AnyElement {
    let choices = vec![
        Choice::new("en", "English"),
        Choice::new("zh-CN", "简体中文"),
    ];
    Select::new("settings-locale", choices)
        .selected(locale.to_owned())
        .label(t("settings.appearance.locale"))
        .on_change(|picked, _, cx| {
            let locale = picked.to_string();
            config::update(cx, |config| config.appearance.locale = locale);
        })
        .into_any_element()
}

/// The row density the sidebar's lists are drawn at. Two values, because two
/// is what the config has: the prototype's comfortable row and its compact one.
fn density_control(density: &str) -> AnyElement {
    let selected = if density == "compact" {
        "compact"
    } else {
        "comfortable"
    };
    SegmentedControl::new("settings-density", selected)
        .segment(
            "comfortable",
            t("settings.appearance.density_comfortable"),
            None,
        )
        .segment("compact", t("settings.appearance.density_compact"), None)
        .size(ControlSize::Sm)
        .on_change(|picked, _, cx| {
            let density = picked.to_string();
            config::update(cx, |config| config.appearance.density = density);
        })
        .into_any_element()
}

// ── terminal ─────────────────────────────────────────────────────────

fn terminal_cards(cx: &App) -> Vec<AnyElement> {
    let terminal = &config::get(cx).terminal;
    let follows = config::get(cx).appearance.terminal_follows_mode;
    let preview = rust_i18n::t!(
        "settings.terminal.preview",
        face = cx.theme().mono_family.clone(),
        size = terminal.font_size,
        height = format!("{:.2}", terminal.line_height)
    )
    .to_string();

    let cursor = SegmentedControl::new("settings-cursor", terminal.cursor.clone())
        .segment("block", t("settings.terminal.cursor_block"), None)
        .segment("bar", t("settings.terminal.cursor_bar"), None)
        .segment("underline", t("settings.terminal.cursor_underline"), None)
        .size(ControlSize::Sm)
        .on_change(|picked, _, cx| {
            let cursor = picked.to_string();
            config::update(cx, |config| config.terminal.cursor = cursor);
        })
        .into_any_element();

    let type_rows = vec![
        row(
            t("settings.terminal.font"),
            Some(t("settings.terminal.font_description")),
            Some(value(cx.theme().mono_family.to_string(), cx)),
        ),
        row(
            t("settings.terminal.font_size"),
            Some(t("settings.terminal.font_size_description")),
            Some(measure(
                "settings-font-size",
                terminal.font_size as f64,
                config::FONT_SIZE_MIN as f64,
                config::FONT_SIZE_MAX as f64,
                1.0,
                0,
                "px",
                |size, cx| {
                    config::update(cx, |config| config.terminal.set_font_size(size as f32));
                },
                cx,
            )),
        ),
        row(
            t("settings.terminal.line_height"),
            Some(t("settings.terminal.line_height_description")),
            Some(measure(
                "settings-line-height",
                terminal.line_height as f64,
                config::LINE_HEIGHT_MIN as f64,
                config::LINE_HEIGHT_MAX as f64,
                0.05,
                2,
                "×",
                |height, cx| {
                    config::update(cx, |config| config.terminal.set_line_height(height as f32));
                },
                cx,
            )),
        ),
        row(
            t("settings.terminal.cursor"),
            Some(t("settings.terminal.cursor_description")),
            Some(cursor),
        ),
        row(
            t("settings.terminal.preview_row"),
            None,
            Some(value(preview, cx)),
        ),
    ];

    let follow = Switch::new("settings-terminal-follow", follows)
        .on_change(|on, _, cx| {
            config::update(cx, |config| config.appearance.terminal_follows_mode = on);
        })
        .into_any_element();
    let color_rows = vec![
        row(
            t("settings.terminal.palette"),
            Some(t("settings.terminal.colors_card_note")),
            Some(value(t("settings.terminal.palette_value"), cx)),
        ),
        row(
            t("theme.terminal.follow"),
            Some(t("theme.terminal.follow_hint")),
            Some(follow),
        ),
    ];

    let daemon_config = crate::platform::herdr_config_path().display().to_string();
    let scrollback_rows = vec![
        row(
            t("settings.terminal.scrollback"),
            Some(t("settings.terminal.scrollback_description")),
            Some(value(t("settings.terminal.scrollback_value"), cx)),
        ),
        row(
            t("settings.terminal.scrollback_file"),
            None,
            Some(value(daemon_config, cx)),
        ),
    ];

    vec![
        card(
            t("settings.terminal.type_card"),
            Some(t("settings.terminal.type_card_note")),
            type_rows,
            cx,
        ),
        card(t("settings.terminal.colors_card"), None, color_rows, cx),
        card(
            t("settings.terminal.scrollback_card"),
            None,
            scrollback_rows,
            cx,
        ),
    ]
}

/// A stepper with its unit beside it: the config's schema clamps the value, so
/// the control cannot write one the loader would reject.
#[allow(clippy::too_many_arguments)]
fn measure(
    id: &'static str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    precision: usize,
    unit: &'static str,
    set: impl Fn(f64, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            Stepper::new(id, value)
                .range(min, max)
                .step(step)
                .precision(precision)
                .on_change(move |value, _, cx| set(value, cx)),
        )
        .child(
            div()
                .text_size(theme.text_size(TextSize::Xs))
                .text_color(theme.colors.fg_subtle)
                .child(unit),
        )
        .into_any_element()
}

// ── ai ───────────────────────────────────────────────────────────────

fn ai_cards(state: &State, root: &Entity<Root>, cx: &App) -> Vec<AnyElement> {
    let file = crate::platform::zenix_config_dir()
        .join("api-providers.json")
        .display()
        .to_string();
    let Some(provider) = state.selected() else {
        return vec![
            card(
                t("settings.ai.model_card"),
                Some(t("settings.ai.model_card_note")),
                vec![row(
                    t("settings.ai.none"),
                    Some(rust_i18n::t!("settings.ai.none_hint", path = file).to_string()),
                    None,
                )],
                cx,
            ),
            card(
                t("settings.ai.file_card"),
                None,
                vec![row(t("settings.ai.file"), None, Some(value(file, cx)))],
                cx,
            ),
        ];
    };

    let choices: Vec<Choice> = state
        .providers
        .iter()
        .map(|provider| {
            let label = if provider.model.trim().is_empty() {
                provider.name.clone()
            } else {
                format!("{} · {}", provider.name, provider.model)
            };
            Choice::new(provider.name.clone(), label)
        })
        .collect();
    let provider_select = {
        let name = provider.name.clone();
        let root = root.clone();
        Select::new("settings-ai-provider", choices)
            .selected(name)
            .label(t("settings.ai.provider"))
            .on_change(move |picked, _, cx| {
                let name = picked.to_string();
                root.update(cx, move |root, cx| root.choose_ai_provider(&name, cx));
            })
    };

    let level = provider.thinking_level;
    let mut level_rows = Vec::new();
    let mut control = SegmentedControl::new("settings-ai-level", level.as_str());
    for each in ThinkingLevel::all() {
        control = control.segment(
            each.as_str(),
            t(&format!("ai.effort.{}", each.as_str())),
            None,
        );
    }
    let root = root.clone();
    let level_control = control
        .size(ControlSize::Sm)
        .on_change(move |picked, _, cx| {
            let token = picked.to_string();
            root.update(cx, move |root, cx| root.set_ai_level(&token, cx));
        })
        .into_any_element();
    level_rows.push(row(
        t("settings.ai.level"),
        Some(t("settings.ai.level_note")),
        Some(level_control),
    ));

    let mut rows = vec![
        row(
            t("settings.ai.provider"),
            None,
            Some(provider_select.into_any_element()),
        ),
        row(
            t("settings.ai.format"),
            None,
            Some(value(provider.format.label().to_owned(), cx)),
        ),
        row(
            t("settings.ai.endpoint"),
            None,
            Some(value(provider.endpoint_url(), cx)),
        ),
        row(
            t("settings.ai.model"),
            None,
            Some(value(
                if provider.model.trim().is_empty() {
                    t("settings.ai.model_none")
                } else {
                    provider.model.clone()
                },
                cx,
            )),
        ),
        row(
            t("settings.ai.key"),
            None,
            Some(badge(
                if provider.api_key.trim().is_empty() {
                    t("settings.ai.key_missing")
                } else {
                    t("settings.ai.key_set")
                },
                !provider.api_key.trim().is_empty(),
                cx,
            )),
        ),
    ];
    rows.extend(level_rows);

    let mut context_rows = vec![
        row(
            t("settings.ai.context_window"),
            None,
            Some(value(
                format!("{} tokens", crate::ai::compactor::DEFAULT_CONTEXT_WINDOW),
                cx,
            )),
        ),
        row(
            t("settings.ai.reserve"),
            None,
            Some(value(
                format!("{} tokens", crate::ai::compactor::DEFAULT_RESERVE),
                cx,
            )),
        ),
        row(
            t("settings.ai.keep_recent"),
            None,
            Some(value(
                format!(
                    "{:.0}%",
                    crate::ai::compactor::DEFAULT_KEEP_RECENT_RATIO * 100.
                ),
                cx,
            )),
        ),
    ];
    if let Some(error) = &state.error {
        context_rows.push(row(
            rust_i18n::t!("settings.ai.save_failed", error = error.as_str()).to_string(),
            None,
            None,
        ));
    }

    vec![
        card(
            t("settings.ai.model_card"),
            Some(t("settings.ai.model_card_note")),
            rows,
            cx,
        ),
        card(
            t("settings.ai.context_card"),
            Some(t("settings.ai.context_card_note")),
            context_rows,
            cx,
        ),
        card(
            t("settings.ai.file_card"),
            None,
            vec![row(t("settings.ai.file"), None, Some(value(file, cx)))],
            cx,
        ),
    ]
}

// ── keys ─────────────────────────────────────────────────────────────

fn keys_cards(plugins: &PluginState, keymap: &Keymap, cx: &App) -> Vec<AnyElement> {
    let mut command_rows = Vec::with_capacity(commands::COMMANDS.len());
    for info in commands::COMMANDS {
        let mut end = chord(keymap.label(info.command));
        if end.is_none() {
            end = Some(value(t("settings.keys.palette_only"), cx));
        }
        command_rows.push(row(info.label(), None, end));
    }

    let catalog = Catalog::build(&plugins.actions, &plugins.ui).runnable(&plugins.plugins);
    let mut plugin_rows: Vec<AnyElement> = Vec::new();
    for entry in catalog.entries() {
        let Some(declared) = entry.chord() else {
            continue;
        };
        plugin_rows.push(row(
            format!("{}: {}", entry.action.plugin_id, entry.label()),
            None,
            chord(Some(keymap.chord_label(declared))),
        ));
    }
    if plugin_rows.is_empty() {
        plugin_rows.push(row(t("settings.keys.none"), None, None));
    }

    let prefix_rows = vec![
        row(
            t("settings.keys.prefix"),
            Some(t("settings.keys.prefix_note")),
            Some(KbdCombo::new(&keymap.prefix_label()).into_any_element()),
        ),
        row(
            t("settings.keys.directory"),
            Some(t("settings.keys.directory_note")),
            Some(value(format!("{} commands", commands::COMMANDS.len()), cx)),
        ),
    ];

    vec![
        card(
            t("settings.keys.commands_card"),
            Some(t("settings.keys.commands_card_note")),
            command_rows,
            cx,
        ),
        card(
            t("settings.keys.plugin_card"),
            Some(t("settings.keys.plugin_card_note")),
            plugin_rows,
            cx,
        ),
        card(t("settings.keys.prefix_card"), None, prefix_rows, cx),
    ]
}

// ── plugins ──────────────────────────────────────────────────────────

fn plugins_cards(plugins: &PluginState, cx: &App) -> Vec<AnyElement> {
    let installed = plugins.plugins.len();
    let enabled = plugins
        .plugins
        .iter()
        .filter(|plugin| plugin.enabled)
        .count();
    let host_rows = vec![
        row(
            t("settings.plugins.installed"),
            None,
            Some(value(format!("{installed} / {enabled}"), cx)),
        ),
        row(
            t("settings.plugins.host"),
            None,
            Some(value(t("settings.plugins.host_value"), cx)),
        ),
        row(
            t("settings.plugins.declarations"),
            None,
            Some(value(t("settings.plugins.declarations_value"), cx)),
        ),
        row(
            t("settings.plugins.contract"),
            None,
            Some(value(
                rust_i18n::t!(
                    "settings.plugins.contract_value",
                    version = crate::plugins::ui_manifest::SCHEMA_VERSION
                )
                .to_string(),
                cx,
            )),
        ),
        row(
            t("settings.plugins.review"),
            None,
            Some(value(t("settings.plugins.review_value"), cx)),
        ),
        row(
            t("settings.plugins.sandbox"),
            None,
            Some(value(t("settings.plugins.sandbox_value"), cx)),
        ),
    ];

    let mut list_rows: Vec<AnyElement> = Vec::new();
    for plugin in &plugins.plugins {
        let actions = plugins
            .actions
            .iter()
            .filter(|action| action.plugin_id == plugin.plugin_id)
            .count();
        let slots = plugins
            .declared
            .iter()
            .find(|declared| declared.plugin_id == plugin.plugin_id)
            .map(|declared| {
                declared.ui.commands.len()
                    + declared.ui.sidebar_items.len()
                    + declared.ui.panels.len()
                    + declared.ui.status_items.len()
                    + declared.ui.settings_sections.len()
            })
            .unwrap_or(0);
        list_rows.push(row(
            format!("{} · {}", plugin.name, plugin.plugin_id),
            Some(
                rust_i18n::t!(
                    "settings.plugins.entry",
                    version = plugin.version,
                    actions = actions,
                    slots = slots
                )
                .to_string(),
            ),
            Some(badge(
                if plugin.enabled {
                    t("settings.plugins.enabled")
                } else {
                    t("settings.plugins.disabled")
                },
                plugin.enabled,
                cx,
            )),
        ));
    }
    if list_rows.is_empty() {
        list_rows.push(row(t("settings.plugins.none"), None, None));
    }

    vec![
        card(t("settings.plugins.host"), None, host_rows, cx),
        card(
            t("settings.plugins.list_card"),
            Some(t("settings.plugins.list_card_note")),
            list_rows,
            cx,
        ),
    ]
}

// ── mcp ──────────────────────────────────────────────────────────────

fn mcp_cards(plugins: &PluginState, orchestrator: bool, cx: &App) -> Vec<AnyElement> {
    let tools = orchestrator_tools();
    let service_rows = vec![
        row(
            t("settings.mcp.status"),
            None,
            Some(badge(
                if orchestrator {
                    t("settings.mcp.listening")
                } else {
                    t("settings.mcp.not_listening")
                },
                orchestrator,
                cx,
            )),
        ),
        row(
            t("settings.mcp.orchestrator"),
            None,
            Some(value(
                crate::mcp::orchestrator_socket_path().display().to_string(),
                cx,
            )),
        ),
        row(
            t("settings.mcp.orchestrator_tools"),
            None,
            Some(value(tools.len().to_string(), cx)),
        ),
        row(
            t("settings.mcp.plugin_tools"),
            Some(t("settings.mcp.plugin_tools_value")),
            Some(value(plugins.actions.len().to_string(), cx)),
        ),
    ];
    let bridge_rows = vec![
        row(
            t("settings.mcp.bridge_command"),
            None,
            Some(value("zenix mcp-agent".to_owned(), cx)),
        ),
        row(
            t("settings.mcp.bridge_tool"),
            None,
            Some(value("agent_send".to_owned(), cx)),
        ),
    ];
    let tool_rows: Vec<AnyElement> = tools
        .iter()
        .map(|tool| row(tool.clone(), None, None))
        .collect();
    vec![
        card(t("settings.mcp.service_card"), None, service_rows, cx),
        card(
            t("settings.mcp.bridge"),
            Some(t("settings.mcp.bridge_value")),
            bridge_rows,
            cx,
        ),
        card(t("settings.mcp.tool_list_card"), None, tool_rows, cx),
    ]
}

/// The orchestrator's own tool names. They are compiled into the handler;
/// this is the same list the socket advertises, read once and kept.
fn orchestrator_tools() -> &'static [SharedString] {
    static TOOLS: OnceLock<Vec<SharedString>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        crate::mcp::handler::McpToolHandler::build_tools()
            .into_iter()
            .map(|tool| SharedString::from(tool.name.to_string()))
            .collect()
    })
}

// ── about ────────────────────────────────────────────────────────────

fn about_cards(live: &LiveState, cx: &App) -> Vec<AnyElement> {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let pinned = option_env!("HERDR_UPSTREAM_VERSION").unwrap_or("—");
    let running = live
        .server_version
        .clone()
        .unwrap_or_else(|| "—".to_owned());
    let ui = t("settings.about.ui_value");

    let mut version_rows = vec![
        row(
            "zenix",
            None,
            Some(value(
                format!("{} · {profile}", env!("CARGO_PKG_VERSION")),
                cx,
            )),
        ),
        row(
            t("settings.about.herdr"),
            Some(t("settings.about.herdr_note")),
            Some(value(pinned.to_owned(), cx)),
        ),
        row(t("settings.about.daemon"), None, Some(value(running, cx))),
        row(t("settings.about.ui"), None, Some(value(ui, cx))),
    ];
    if let Some(mismatch) = &live.version_mismatch {
        let note = match mismatch {
            herdr_client::VersionMismatch::DaemonOutdated { .. } => {
                t("settings.about.mismatch_daemon")
            }
            herdr_client::VersionMismatch::ClientOutdated { .. } => {
                t("settings.about.mismatch_client")
            }
        };
        version_rows.push(row(
            t("settings.about.mismatch"),
            Some(note),
            Some(badge(
                mismatch.server_version().unwrap_or("—").to_owned(),
                false,
                cx,
            )),
        ));
    }

    let license_rows = vec![
        row(
            "zenix",
            None,
            Some(value("GPL-3.0-or-later".to_owned(), cx)),
        ),
        row("herdr", None, Some(value("Apache-2.0".to_owned(), cx))),
        row(
            t("settings.about.component_library"),
            None,
            Some(value("MIT".to_owned(), cx)),
        ),
        row(
            "Inter · JetBrains Mono",
            None,
            Some(value("SIL OFL-1.1".to_owned(), cx)),
        ),
    ];

    let mut file_rows = vec![
        row(
            t("settings.config.path"),
            None,
            Some(value(config::path_of(cx).display().to_string(), cx)),
        ),
        row(
            t("settings.about.license_texts"),
            Some(t("settings.about.license_texts_note")),
            Some(value("/usr/share/doc/zenix/".to_owned(), cx)),
        ),
    ];
    let diagnostics = config::diagnostics(cx);
    if diagnostics.is_empty() {
        file_rows.push(row(
            t("settings.config.problems"),
            None,
            Some(value(t("settings.config.no_problems"), cx)),
        ));
    } else {
        for diagnostic in diagnostics {
            file_rows.push(row(
                t("settings.config.problems"),
                Some(format!("{} · {}", diagnostic.path, diagnostic.message)),
                Some(badge(
                    if diagnostic.is_blocking() {
                        "blocking".to_owned()
                    } else {
                        "warning".to_owned()
                    },
                    false,
                    cx,
                )),
            ));
        }
    }

    vec![
        card(t("settings.about.version_card"), None, version_rows, cx),
        card(
            t("settings.about.licenses_card"),
            Some(t("settings.about.licenses_note")),
            license_rows,
            cx,
        ),
        card(t("settings.about.files_card"), None, file_rows, cx),
    ]
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use ely_core::theme::Mode;
    use gpui::hsla;

    use super::*;

    /// The nav's pages, top to bottom: what the two tests below hold the
    /// groups to.
    const PAGES: [Page; 7] = [
        Page::Appearance,
        Page::Terminal,
        Page::Ai,
        Page::Keys,
        Page::Plugins,
        Page::Mcp,
        Page::About,
    ];

    /// A page is a nav row, a heading and a line of context; a missing key
    /// would draw its own key, which is what `t` hands back.
    #[test]
    fn every_page_has_a_name_and_a_line() {
        for page in PAGES {
            assert_ne!(page.title(), format!("settings.{}.title", page.id()));
            assert_ne!(
                page.description(),
                format!("settings.{}.description", page.id()),
                "{}",
                page.id()
            );
        }
    }

    #[test]
    fn every_page_has_a_distinct_id_and_a_nav_slot() {
        let mut ids: Vec<&str> = PAGES.iter().map(|page| page.id()).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total);
        // The nav shows each page exactly once, in its group's order.
        let shown: Vec<Page> = NAV_GROUPS
            .iter()
            .flat_map(|(_, pages, _)| pages.iter().copied())
            .collect();
        assert_eq!(shown, PAGES);
    }

    /// A preset swatch shows the palette's own color in the current mode, and
    /// picking it back must name that preset rather than a custom hex.
    #[test]
    fn every_accent_preset_survives_the_picker() {
        for mode in [Mode::Light, Mode::Dark] {
            let palette = tone_palette(mode);
            for accent in Accent::PRESETS {
                let chosen = accent_choice(accent.color(&palette), mode);
                assert!(
                    !matches!(chosen, config::AccentChoice::Custom(_)),
                    "{accent:?}"
                );
                assert_eq!(chosen.as_str(), named(accent), "{accent:?}");
            }
        }
    }

    fn named(accent: Accent) -> &'static str {
        match accent {
            Accent::Default => "default",
            Accent::Blue => "blue",
            Accent::Teal => "teal",
            Accent::Amber => "amber",
            Accent::Rose => "rose",
            Accent::Violet => "violet",
            Accent::Green => "green",
            Accent::Custom(_) => "custom",
        }
    }

    /// A color from the well becomes `#rrggbb`, and the file's walk reads that
    /// spelling back as the same color. The comparison is on the packed hex,
    /// not the float: the file carries whole channels, so a hue read back is
    /// the same color to a channel's precision, not bit-for-bit.
    #[test]
    fn a_custom_accent_is_written_as_its_hex() {
        let color = hsla(0.9, 0.6, 0.5, 1.0);
        let written = accent_choice(color, Mode::Dark);
        let config::AccentChoice::Custom(text) = written else {
            panic!("a color no preset matches is the well's own");
        };
        assert!(text.starts_with('#') && text.len() == 7, "{text}");
        assert_eq!(text, text.to_lowercase());
        assert_eq!(text, hex(color));
        let settings = ThemeSettings::from_config(&config::Appearance {
            accent: config::AccentChoice::Custom(text.clone()),
            ..config::Appearance::default()
        });
        let Accent::Custom(read_back) = settings.accent else {
            panic!("a hex accent is read back as the well's own");
        };
        assert_eq!(hex(read_back), text);
    }

    /// The appearance page's three code-theme names are the translated ones,
    /// and none of them is the library's engineering name for the set.
    #[test]
    fn the_code_theme_chips_go_by_their_translated_names() {
        let previous = crate::i18n::locale();
        crate::i18n::set_locale("zh-CN");
        let names: Vec<String> = CodeTheme::ALL
            .into_iter()
            .map(|code| t(code.label_key()))
            .collect();
        crate::i18n::set_locale(&previous);
        assert_eq!(names, vec!["标准", "静谧", "纸面"]);
        assert!(names.iter().all(|name| name != "Ely"));
        let mut ids: Vec<&str> = CodeTheme::ALL.into_iter().map(code_id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total);
    }

    /// The page's steppers must cover exactly the schema's range, or a value
    /// the config accepts could not be reached from the dialog.
    #[test]
    fn the_metric_steppers_cover_the_schemas_range() {
        assert_eq!(config::FONT_SIZE_MIN, 10.);
        assert_eq!(config::FONT_SIZE_MAX, 18.);
        assert_eq!(config::LINE_HEIGHT_MIN, 1.0);
        assert_eq!(config::LINE_HEIGHT_MAX, 1.8);
        // The default sits inside both.
        let terminal = config::TerminalConfig::default();
        assert!((config::FONT_SIZE_MIN..=config::FONT_SIZE_MAX).contains(&terminal.font_size));
        assert!(
            (config::LINE_HEIGHT_MIN..=config::LINE_HEIGHT_MAX).contains(&terminal.line_height)
        );
    }

    /// The tool list the MCP page states is the handler's own, read once.
    #[test]
    fn the_orchestrator_tools_are_the_handlers() {
        let tools = orchestrator_tools();
        assert!(!tools.is_empty());
        let mut names: Vec<String> = tools.iter().map(|tool| tool.to_string()).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total);
        assert!(
            names.iter().any(|name| name == "spawn_worker"),
            "the orchestrator's first tool"
        );
    }
}
