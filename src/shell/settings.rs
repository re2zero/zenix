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

use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::{ActiveTheme, ThemeMode, h_flex, v_flex};
use gpui_kit::*;

use super::commands::{self, Command};
use super::keymap::Keymap;
use super::registry::{Dispatch, SettingsSection, ShellRequest};
use crate::config::{
    self, ASIDE_WIDTH_DEFAULT, ASIDE_WIDTH_MAX, ASIDE_WIDTH_MIN, SIDEBAR_WIDTH_DEFAULT,
    SIDEBAR_WIDTH_MAX, SIDEBAR_WIDTH_MIN,
};
use crate::i18n::t;
use crate::platform;
use crate::terminal::Theme as TerminalTheme;

/// The built-in pages, in the order the settings sidebar lists them: one per
/// config section, the reference pages the prototype puts between them, then
/// the file itself.
pub(crate) const SETTINGS_SECTIONS: &[SettingsSection] = &[
    SettingsSection {
        page: appearance_page,
    },
    SettingsSection {
        page: terminal_page,
    },
    SettingsSection { page: layout_page },
    SettingsSection { page: keys_page },
    SettingsSection { page: plugins_page },
    SettingsSection { page: mcp_page },
    SettingsSection { page: config_page },
];

/// The chrome's own look: which theme family draws it, which mode of that
/// family, and in which language.
fn appearance_page(_: &App) -> SettingPage {
    let families: Vec<(SharedString, SharedString)> = config::theme::NAMES
        .iter()
        .map(|name| {
            (
                SharedString::from(*name),
                t(&format!("settings.appearance.family_{name}")).into(),
            )
        })
        .collect();
    SettingPage::new(t("settings.appearance.title"))
        .description(t("settings.appearance.description"))
        .group(
            SettingGroup::new()
                .title(t("settings.appearance.title"))
                .item(SettingItem::new(
                    t("settings.appearance.family"),
                    SettingField::dropdown(
                        families,
                        |cx: &App| SharedString::from(config::get(cx).appearance.theme.as_str()),
                        |value: SharedString, cx: &mut App| {
                            config::update(cx, |config| {
                                config.appearance.theme = value.to_string()
                            });
                        },
                    )
                    .default_value("default"),
                ))
                .item(SettingItem::new(
                    t("settings.appearance.theme"),
                    SettingField::dropdown(
                        vec![
                            ("light".into(), t("settings.appearance.theme_light").into()),
                            ("dark".into(), t("settings.appearance.theme_dark").into()),
                        ],
                        |cx: &App| SharedString::from(config::get(cx).appearance.theme_mode.name()),
                        |value: SharedString, cx: &mut App| {
                            let mode = match value.as_ref() {
                                "dark" => ThemeMode::Dark,
                                _ => ThemeMode::Light,
                            };
                            config::update(cx, |config| config.appearance.theme_mode = mode);
                        },
                    )
                    .default_value(ThemeMode::Light.name()),
                ))
                .item(SettingItem::new(
                    t("settings.appearance.locale"),
                    SettingField::dropdown(
                        vec![
                            ("en".into(), "English".into()),
                            ("zh-CN".into(), "简体中文".into()),
                        ],
                        |cx: &App| SharedString::from(config::get(cx).appearance.locale.as_str()),
                        |value: SharedString, cx: &mut App| {
                            config::update(cx, |config| {
                                config.appearance.locale = value.to_string()
                            });
                        },
                    )
                    .default_value("en"),
                )),
        )
}

/// The palettes named cell colors resolve against, one per mode, so switching
/// the chrome's mode keeps the terminal readable.
fn terminal_page(_: &App) -> SettingPage {
    let choices: Vec<(SharedString, SharedString)> = TerminalTheme::BUILTIN_NAMES
        .iter()
        .map(|name| (SharedString::from(*name), SharedString::from(*name)))
        .collect();
    SettingPage::new(t("settings.terminal.title"))
        .description(t("settings.terminal.description"))
        .group(
            SettingGroup::new()
                .title(t("settings.terminal.title"))
                .item(SettingItem::new(
                    t("settings.terminal.palette_dark"),
                    SettingField::dropdown(
                        choices.clone(),
                        |cx: &App| {
                            SharedString::from(config::get(cx).terminal.palette_dark.as_str())
                        },
                        |value: SharedString, cx: &mut App| {
                            config::update(cx, |config| {
                                config.terminal.palette_dark = value.to_string()
                            });
                        },
                    )
                    .default_value("Default"),
                ))
                .item(SettingItem::new(
                    t("settings.terminal.palette_light"),
                    SettingField::dropdown(
                        choices,
                        |cx: &App| {
                            SharedString::from(config::get(cx).terminal.palette_light.as_str())
                        },
                        |value: SharedString, cx: &mut App| {
                            config::update(cx, |config| {
                                config.terminal.palette_light = value.to_string()
                            });
                        },
                    )
                    .default_value("Catppuccin Latte"),
                )),
        )
}

/// The client's own chrome: the sidebar and the right-hand chat panel, neither
/// of which the daemon knows anything about.
fn layout_page(_: &App) -> SettingPage {
    SettingPage::new(t("settings.layout.title"))
        .description(t("settings.layout.description"))
        .group(
            SettingGroup::new()
                .title(t("settings.layout.title"))
                .item(SettingItem::new(
                    t("settings.layout.sidebar_visible"),
                    SettingField::switch(
                        |cx: &App| config::get(cx).layout.sidebar_visible,
                        |value: bool, cx: &mut App| {
                            config::update(cx, |config| config.layout.sidebar_visible = value);
                        },
                    )
                    .default_value(true),
                ))
                .item(SettingItem::new(
                    t("settings.layout.sidebar_width"),
                    SettingField::number_input(
                        NumberFieldOptions {
                            min: f64::from(SIDEBAR_WIDTH_MIN),
                            max: f64::from(SIDEBAR_WIDTH_MAX),
                            step: 1.0,
                        },
                        |cx: &App| f64::from(config::get(cx).layout.sidebar_width),
                        |value: f64, cx: &mut App| {
                            config::update(cx, |config| {
                                config.layout.set_sidebar_width(value as f32)
                            });
                        },
                    )
                    .default_value(f64::from(SIDEBAR_WIDTH_DEFAULT)),
                ))
                .item(SettingItem::new(
                    t("settings.layout.aside_visible"),
                    SettingField::switch(
                        |cx: &App| config::get(cx).layout.aside_visible,
                        |value: bool, cx: &mut App| {
                            config::update(cx, |config| config.layout.aside_visible = value);
                        },
                    )
                    .default_value(false),
                ))
                .item(SettingItem::new(
                    t("settings.layout.aside_width"),
                    SettingField::number_input(
                        NumberFieldOptions {
                            min: f64::from(ASIDE_WIDTH_MIN),
                            max: f64::from(ASIDE_WIDTH_MAX),
                            step: 1.0,
                        },
                        |cx: &App| f64::from(config::get(cx).layout.aside_width),
                        |value: f64, cx: &mut App| {
                            config::update(cx, |config| {
                                config.layout.set_aside_width(value as f32)
                            });
                        },
                    )
                    .default_value(f64::from(ASIDE_WIDTH_DEFAULT)),
                )),
        )
}

/// The keystrokes that reach the shell, read off the command directory — the
/// one table the keymap, the palette and this page all read, so a row here
/// cannot name a chord that does not run. The page writes nothing: chords are
/// the directory's, and a plugin's declared ones live in the plugin view.
fn keys_page(_: &App) -> SettingPage {
    let keymap = Keymap::default();
    let mut group = SettingGroup::new()
        .title(t("settings.keys.title"))
        .item(SettingItem::new(
            t("settings.keys.prefix"),
            text_field(keymap.prefix_label()),
        ));
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
        group = group.item(SettingItem::new(
            info.label(),
            text_field(chords.join("  ")),
        ));
    }
    SettingPage::new(t("settings.keys.title"))
        .description(t("settings.keys.description"))
        .group(group)
}

/// What a plugin is on this client, and the one gate between its directory and
/// a run: the review. There are no toggles here because the client offers no
/// choice to toggle — declarations are read, and a link is only a link after
/// its argv has been shown.
fn plugins_page(_: &App) -> SettingPage {
    SettingPage::new(t("settings.plugins.title"))
        .description(t("settings.plugins.description"))
        .group(
            // One group: the settings nav grows a row per group, so a second
            // one would read as a second page.
            SettingGroup::new()
                .title(t("settings.plugins.title"))
                .item(SettingItem::new(
                    t("settings.plugins.host"),
                    text_field(t("settings.plugins.host_value")),
                ))
                .item(SettingItem::new(
                    t("settings.plugins.daemon_config"),
                    text_field(platform::herdr_config_path().display().to_string()),
                ))
                .item(SettingItem::new(
                    t("settings.plugins.declarations"),
                    text_field(t("settings.plugins.declarations_value")),
                ))
                .item(SettingItem::new(
                    t("settings.plugins.review"),
                    text_field(t("settings.plugins.review_value")),
                ))
                .item(SettingItem::new(
                    t("settings.plugins.sandbox"),
                    text_field(t("settings.plugins.sandbox_value")),
                )),
        )
}

/// What this window offers an agent, and what a shell-less agent can reach
/// back through: the orchestrator's socket, the tools, and the reverse bridge.
fn mcp_page(_: &App) -> SettingPage {
    SettingPage::new(t("settings.mcp.title"))
        .description(t("settings.mcp.description"))
        .group(
            SettingGroup::new()
                .title(t("settings.mcp.title"))
                .item(SettingItem::new(
                    t("settings.mcp.orchestrator"),
                    text_field(crate::mcp::orchestrator_socket_path().display().to_string()),
                ))
                .item(SettingItem::new(
                    t("settings.mcp.tools"),
                    text_field(t("settings.mcp.tools_value")),
                ))
                .item(SettingItem::new(
                    t("settings.mcp.bridge"),
                    text_field(t("settings.mcp.bridge_value")),
                )),
        )
}

/// One read-only line: where a settings row has nothing to set, the fact is
/// the value. Paths and names are copied from where they live, so they cannot
/// drift from the code that uses them.
fn text_field(text: String) -> SettingField<SharedString> {
    SettingField::render(move |_: &_, _, cx: &mut App| {
        let theme = cx.theme().clone();
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(text.clone())
    })
}

/// The file itself, and what was wrong with it: the one place a hand-edited
/// value can be seen to have had no effect.
fn config_page(_: &App) -> SettingPage {
    SettingPage::new(t("settings.config.title"))
        .description(t("settings.config.description"))
        .group(
            SettingGroup::new()
                .title(t("settings.config.title"))
                .item(SettingItem::new(t("settings.config.path"), path_field()))
                .item(SettingItem::new(
                    t("settings.config.problems"),
                    diagnostics_field(),
                ))
                .item(SettingItem::new(
                    t("settings.config.daemon"),
                    daemon_field(),
                )),
        )
}

/// Where the client's config is, as text: it is meant to be opened by hand.
fn path_field() -> SettingField<SharedString> {
    SettingField::render(|_: &_, _, cx: &mut App| {
        let theme = cx.theme().clone();
        div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(config::path_of(cx).display().to_string())
    })
}

/// Every problem the last load or save found, or a line saying there is none.
fn diagnostics_field() -> SettingField<SharedString> {
    SettingField::render(|_: &_, _, cx: &mut App| {
        let theme = cx.theme().clone();
        let diagnostics = config::diagnostics(cx);
        if diagnostics.is_empty() {
            return div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(t("settings.config.no_problems"))
                .into_any_element();
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
            list = list.child(div().text_xs().text_color(color).child(text));
        }
        list.into_any_element()
    })
}

/// The daemon's own file, named so a setting that is not here can be found
/// where it does live (see `crate::config`'s module docs).
fn daemon_field() -> SettingField<SharedString> {
    SettingField::render(|_: &_, _, cx: &mut App| {
        let theme = cx.theme().clone();
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(platform::herdr_config_path().display().to_string()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(t("settings.config.daemon_hint")),
            )
            .into_any_element()
    })
}

/// The settings surface, mounted over the shell while it is open. `focus` is
/// the shell's, so the pane beside it stops taking keystrokes; Escape (or the
/// close button) hands it back.
pub(crate) fn overlay(
    sections: &'static [SettingsSection],
    focus: &FocusHandle,
    dispatch: &Dispatch,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let escape = dispatch.clone();
    let close = dispatch.clone();
    v_flex()
        .id("settings-overlay")
        .absolute()
        .inset_0()
        .occlude()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .track_focus(focus)
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                escape(ShellRequest::Command(Command::ToggleSettings), window, cx);
            }
        })
        .child(
            h_flex()
                .flex_none()
                .w_full()
                .px_3()
                .py_2()
                .justify_between()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(div().text_sm().child(t("settings.title")))
                .child(
                    div()
                        .id("settings-close")
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .text_xs()
                        .bg(theme.secondary)
                        .text_color(theme.secondary_foreground)
                        .cursor_pointer()
                        .on_click(move |_, window, cx| {
                            close(ShellRequest::Command(Command::ToggleSettings), window, cx);
                        })
                        .child(t("settings.close")),
                ),
        )
        .child(
            div().flex_1().min_h_0().child(
                Settings::new("zenix-settings")
                    .pages(sections.iter().map(|section| (section.page)(cx))),
            ),
        )
        .into_any_element()
}
