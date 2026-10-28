//! The chrome's look: the appearance page's four layers, as the component
//! library draws them.
//!
//! The design separates the layers, and so does this module: the mode (light,
//! dark, or the system's), the accent, the code theme, and whether the
//! terminal follows a light chrome. The chrome itself reads none of these
//! names — everything it draws is the library's [`Palette`], which [`sync`]
//! installs once per mode, so a swap is one global write.
//!
//! The names downstairs are engineering names: the code themes are `Ely`,
//! `Quiet` and `Paper`. What a reader sees comes from `locales/zenix.yml`
//! (`theme.*`) — 标准 / 静谧 / 纸面. Nothing here renders a string; the label
//! keys are what the appearance page resolves.
//!
//! The config spells the same four layers (`appearance.mode`, `.accent`,
//! `.syntax`, `.terminal_follows_mode`), so [`ThemeSettings::from_config`] is a
//! direct read; an old file's `theme_mode` / `theme` keys are migrated by the
//! config's own schema walk, which announces them as deprecated.

use ely_core::i18n::I18n;
use ely_core::theme::{
    Mix, Mode, Palette, Syntax, SyntaxTheme, Theme as ChromeTheme, contrast, syntax_themes,
};
use gpui::{App, Global, Hsla, rgb};

use crate::{config, terminal};

/// How the chrome picks light or dark: one always, or as the system does.
///
/// The config spells the same three choices `light` / `dark` / `system`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModeSetting {
    /// Dark text on light grounds.
    Light,
    /// Light text on dark grounds: the design's default.
    #[default]
    Dark,
    /// As the system is.
    System,
}

impl ModeSetting {
    /// Every choice, in the design selector's order.
    pub const ALL: [ModeSetting; 3] = [ModeSetting::Light, ModeSetting::Dark, ModeSetting::System];

    /// The mode it shows when the system is in `system`.
    pub fn resolve(self, system: Mode) -> Mode {
        match self {
            ModeSetting::Light => Mode::Light,
            ModeSetting::Dark => Mode::Dark,
            ModeSetting::System => system,
        }
    }

    /// Where its name lives in `locales/zenix.yml`.
    pub fn label_key(self) -> &'static str {
        match self {
            ModeSetting::Light => "theme.mode.light",
            ModeSetting::Dark => "theme.mode.dark",
            ModeSetting::System => "theme.mode.system",
        }
    }
}

/// The accent the chrome paints chosen rows and primary buttons with: the
/// palette's own single color, one of the named hues, or any color the owner
/// picked in the well.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Accent {
    /// The palette's own accent: near-black in light mode, near-white in dark.
    #[default]
    Default,
    /// The palette's blue.
    Blue,
    /// The palette's teal — 青 on screen.
    Teal,
    /// The palette's ochre — 琥珀 on screen.
    Amber,
    /// The palette's rose.
    Rose,
    /// The palette's violet.
    Violet,
    /// The palette's green.
    Green,
    /// A color read off the well.
    Custom(Hsla),
}

impl Accent {
    /// The named presets, in the appearance page's order; the well beside
    /// them covers 自定义.
    pub const PRESETS: [Accent; 7] = [
        Accent::Default,
        Accent::Blue,
        Accent::Teal,
        Accent::Amber,
        Accent::Rose,
        Accent::Violet,
        Accent::Green,
    ];

    /// The color it paints, in `palette`'s mode. The named hues are the
    /// palette's own chart, so they stay legible in both modes and readable
    /// for color-blind eyes.
    pub fn color(self, palette: &Palette) -> Hsla {
        match self {
            Accent::Default => palette.accent,
            Accent::Blue => palette.chart[0],
            Accent::Teal => palette.chart[1],
            Accent::Amber => palette.chart[2],
            Accent::Rose => palette.chart[3],
            Accent::Violet => palette.chart[4],
            Accent::Green => palette.chart[5],
            Accent::Custom(color) => color,
        }
    }

    /// Where its name lives in `locales/zenix.yml`.
    pub fn label_key(self) -> &'static str {
        match self {
            Accent::Default => "theme.accent.default",
            Accent::Blue => "theme.accent.blue",
            Accent::Teal => "theme.accent.teal",
            Accent::Amber => "theme.accent.amber",
            Accent::Rose => "theme.accent.rose",
            Accent::Violet => "theme.accent.violet",
            Accent::Green => "theme.accent.green",
            Accent::Custom(_) => "theme.accent.custom",
        }
    }
}

/// Which colors code and markdown code blocks set in: the standard set, the
/// quiet one, or the warm paper one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CodeTheme {
    /// The library's own colors, made from the palette.
    #[default]
    Standard,
    /// Low-chroma inks that keep to the text's own tones.
    Quiet,
    /// A warm paper set.
    Paper,
}

impl CodeTheme {
    /// Every code theme, in the appearance page's order.
    pub const ALL: [CodeTheme; 3] = [CodeTheme::Standard, CodeTheme::Quiet, CodeTheme::Paper];

    /// The library's name for this set.
    pub fn name(self) -> &'static str {
        match self {
            CodeTheme::Standard => "Ely",
            CodeTheme::Quiet => "Quiet",
            CodeTheme::Paper => "Paper",
        }
    }

    /// The library's theme for this set. Looked up by name so a reordering of
    /// [`syntax_themes`] cannot silently swap two code palettes; a set the
    /// library drops falls back to the standard one rather than failing a
    /// frame.
    pub fn theme(self) -> SyntaxTheme {
        let themes = syntax_themes();
        themes
            .iter()
            .find(|theme| theme.name == self.name())
            .cloned()
            .unwrap_or_else(|| themes[0].clone())
    }

    /// Its colors in `mode`.
    pub fn syntax(self, mode: Mode) -> Syntax {
        self.theme().of(mode)
    }

    /// Where its name lives in `locales/zenix.yml`.
    pub fn label_key(self) -> &'static str {
        match self {
            CodeTheme::Standard => "theme.code.standard",
            CodeTheme::Quiet => "theme.code.quiet",
            CodeTheme::Paper => "theme.code.paper",
        }
    }
}

/// The appearance page's four layers, as one value.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeSettings {
    /// Light, dark, or the system's.
    pub mode: ModeSetting,
    /// What chosen rows and primary buttons are painted with.
    pub accent: Accent,
    /// What code and markdown code blocks are painted with.
    pub code_theme: CodeTheme,
    /// Whether the terminal takes the light mode's colors too. Off, it keeps
    /// the dark set in a light chrome, as the design's visual anchor.
    pub terminal_follows_mode: bool,
}

impl Default for ThemeSettings {
    /// The design's defaults (PLAN §3, decision 01): a dark chrome, the
    /// palette's own accent, the standard code theme, a terminal that follows.
    fn default() -> Self {
        Self {
            mode: ModeSetting::Dark,
            accent: Accent::Default,
            code_theme: CodeTheme::Standard,
            terminal_follows_mode: true,
        }
    }
}

impl ThemeSettings {
    /// The settings a config file describes.
    pub fn from_config(appearance: &config::Appearance) -> Self {
        Self {
            mode: match appearance.mode {
                config::ThemeMode::Light => ModeSetting::Light,
                config::ThemeMode::Dark => ModeSetting::Dark,
                config::ThemeMode::System => ModeSetting::System,
            },
            accent: match &appearance.accent {
                config::AccentChoice::Default => Accent::Default,
                config::AccentChoice::Blue => Accent::Blue,
                config::AccentChoice::Teal => Accent::Teal,
                config::AccentChoice::Amber => Accent::Amber,
                config::AccentChoice::Rose => Accent::Rose,
                config::AccentChoice::Violet => Accent::Violet,
                config::AccentChoice::Green => Accent::Green,
                config::AccentChoice::Custom(text) => accent_from_hex(text),
            },
            code_theme: match appearance.syntax {
                config::SyntaxChoice::Standard => CodeTheme::Standard,
                config::SyntaxChoice::Quiet => CodeTheme::Quiet,
                config::SyntaxChoice::Paper => CodeTheme::Paper,
            },
            terminal_follows_mode: appearance.terminal_follows_mode,
        }
    }

    /// The palette `mode` draws with: the library's own, its accent moved to
    /// the chosen one, its code colors set from the code theme.
    pub fn palette(&self, mode: Mode) -> Palette {
        let mut palette = tone_palette(mode);
        let accent = self.accent.color(&palette);
        if accent != palette.accent {
            palette.accent = accent;
            // Hover steps toward the body ink, which darkens it in a light
            // chrome and lightens it in a dark one.
            palette.accent_hover = accent.mix(&palette.fg, 0.12);
            // The ink on the accent is whichever of the two grounds reads
            // better on it; a hue that passes neither still keeps the better
            // pair rather than a fixed guess.
            palette.on_accent = if contrast(accent, palette.bg) >= contrast(accent, palette.fg) {
                palette.bg
            } else {
                palette.fg
            };
        }
        palette.syntax = self.code_theme.syntax(mode);
        palette
    }

    /// The terminal's sixteen colors in a chrome of `mode`.
    ///
    /// The terminal follows the mode unless the owner turned that off: then it
    /// keeps the dark set even in a light chrome. The accent does not reach
    /// the ANSI colors, by design.
    pub fn terminal_colors(&self, mode: Mode) -> [Hsla; 16] {
        tone_palette(self.terminal_tone(mode)).ansi
    }

    /// The mode the grid draws in: the chrome's, unless the owner anchored the
    /// terminal dark.
    pub fn terminal_tone(&self, mode: Mode) -> Mode {
        if self.terminal_follows_mode {
            mode
        } else {
            Mode::Dark
        }
    }

    /// The grid's whole palette in a chrome of `mode`: the tone's sixteen ANSI
    /// entries, its ink, and the design's ground for that tone — the page's
    /// own in a light chrome, one step below it in a dark one (`--term-bg`).
    ///
    /// A terminal anchored dark keeps all of it in a light chrome: same
    /// ground, same entries, same ink.
    pub(crate) fn terminal_theme(&self, mode: Mode) -> terminal::Theme {
        let tone = self.terminal_tone(mode);
        let palette = tone_palette(tone);
        terminal::Theme::from_ansi(
            palette.ansi.map(packed),
            packed(match tone {
                Mode::Light => palette.bg,
                Mode::Dark => palette.sunken,
            }),
            packed(palette.fg),
            packed(palette.fg_subtle),
        )
    }
}

/// A grid before the shell has dressed it: the default dark chrome's own
/// palette, through the same builder every window uses.
///
/// This exists so the painter and the view can be constructed without a
/// theme; it is not a palette of the terminal's own (U1-3's decision), and
/// every real paint calls `set_appearance` with the config's theme first.
impl Default for terminal::Theme {
    fn default() -> Self {
        ThemeSettings::default().terminal_theme(Mode::Dark)
    }
}

/// The library's palette for one tone, before any of the four layers are laid
/// on it. The appearance page reads it for the accent swatches (U3-1): the
/// named hues are the palette's own chart, and a swatch must show the color
/// the page is about to pick even when the current accent is a custom one.
pub(crate) fn tone_palette(mode: Mode) -> Palette {
    match mode {
        Mode::Light => Palette::light(false),
        Mode::Dark => Palette::dark(false),
    }
}

/// A color as the daemon's cells and the painter spell it: packed 24-bit RGB,
/// rounded the way the library rounds its own (`channel × 255`), so a themed
/// hex value comes back out of the pack unchanged.
fn packed(color: Hsla) -> u32 {
    let rgb = color.to_rgb();
    let byte = |channel: f32| (channel.clamp(0., 1.) * 255.).round() as u32;
    (byte(rgb.r) << 16) | (byte(rgb.g) << 8) | byte(rgb.b)
}

/// A `#rrggbb` accent as the theme's own color type. The config's walk keeps
/// the spelling canonical, so a value that does not parse is a hand-built
/// config, not a file: it falls back to the default accent rather than
/// failing.
fn accent_from_hex(text: &str) -> Accent {
    let Some(digits) = text.strip_prefix('#') else {
        return Accent::Default;
    };
    if digits.len() != 6 {
        return Accent::Default;
    }
    match u32::from_str_radix(digits, 16) {
        Ok(hex) => Accent::Custom(rgb(hex).into()),
        Err(_) => Accent::Default,
    }
}

/// What the last [`sync`] installed: the four layers, and the mode they
/// resolved to. The resolved mode is part of the value because a
/// `mode: system` chrome follows the system: the four layers can stay
/// identical while the OS moves from light to dark, and that move is a
/// reinstall rather than a no-op.
#[derive(Default)]
struct Installed(Option<(ThemeSettings, Mode)>);

impl Global for Installed {}

/// Dress the component library in the config's appearance.
///
/// Called from the config's apply path, which runs once at startup before the
/// window opens and again on every write — so the first frame is already in
/// the right mode, and a later change follows a settings edit. The first
/// install lands at once (`set_mode_now`): a fade has no window to play in
/// yet and would leave the first frame half-dressed. A later change fades, as
/// a click should. A call whose four layers and resolved mode already match
/// does nothing, so an unrelated write starts no fade and repaints nothing;
/// a `mode: system` chrome re-installs when the system's appearance moves.
pub fn sync(cx: &mut App) {
    // A test app, or a startup that failed before the library's init, has no
    // theme to dress; the frame then shows the library's own defaults.
    if cx.try_global::<ChromeTheme>().is_none() {
        return;
    }
    let settings = ThemeSettings::from_config(&config::get(cx).appearance);
    let mode = settings.mode.resolve(cx.window_appearance().into());
    // The library's own components read its catalogs — the palette's
    // placeholder, its empty line and its recency heading among them — so its
    // locale follows the config's before any of them is drawn. This is not
    // part of the installed-layers check below: a locale is not a layer, and
    // an English palette in a Chinese chrome is the bug it prevents.
    install_locale(cx);
    let installed = cx
        .try_global::<Installed>()
        .and_then(|installed| installed.0.clone());
    if installed.as_ref() == Some(&(settings.clone(), mode)) {
        return;
    }

    // Both palettes go in ahead of the mode, so a switch either way needs no
    // rebuild of the other half.
    ChromeTheme::set_palette(Mode::Light, Some(settings.palette(Mode::Light)), cx);
    ChromeTheme::set_palette(Mode::Dark, Some(settings.palette(Mode::Dark)), cx);
    if installed.is_none() {
        ChromeTheme::set_mode_now(mode, cx);
    } else {
        ChromeTheme::set_mode(mode, cx);
    }
    // No faces are installed: the chrome and the grid set text in whatever
    // the component library's theme ships (Inter and JetBrains Mono), so this
    // client never imposes a font of its own.
    cx.set_global(Installed(Some((settings, mode))));
}

/// The library's locale for the config's own. The config ships `en` and
/// `zh-CN`; the library names the first `en-US` and panics on a tag it does
/// not carry, so the mapping is total and falls back to English.
fn library_locale(locale: &str) -> &'static str {
    match locale {
        "zh-CN" => "zh-CN",
        _ => "en-US",
    }
}

/// Install the library's message catalog in the config's locale. The global
/// is rewritten only when the tag moved: every config write runs through
/// [`sync`], and repainting every window for an unrelated edit would cancel
/// the palette's fade (and any other open animation).
fn install_locale(cx: &mut App) {
    let locale = library_locale(&config::get(cx).appearance.locale);
    if cx
        .try_global::<I18n>()
        .is_some_and(|i18n| i18n.locale().tag == locale)
    {
        return;
    }
    cx.set_global(I18n::new(locale));
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use ely_core::theme::{ActiveTheme, Mode, Palette, syntax_themes};
    use gpui::{TestAppContext, hsla, rgb};

    use super::*;
    use crate::i18n::t;

    #[test]
    fn the_three_modes_resolve_against_the_system() {
        assert_eq!(ModeSetting::Light.resolve(Mode::Dark), Mode::Light);
        assert_eq!(ModeSetting::Dark.resolve(Mode::Light), Mode::Dark);
        assert_eq!(ModeSetting::System.resolve(Mode::Dark), Mode::Dark);
        assert_eq!(ModeSetting::System.resolve(Mode::Light), Mode::Light);
    }

    #[test]
    fn the_defaults_are_the_designs() {
        let settings = ThemeSettings::default();
        assert_eq!(settings.mode, ModeSetting::Dark);
        assert_eq!(settings.accent, Accent::Default);
        assert_eq!(settings.code_theme, CodeTheme::Standard);
        assert!(settings.terminal_follows_mode);
    }

    /// The config's two locales are both carried by the library; a value the
    /// schema would reject still lands on a tag the library knows rather than
    /// panicking in `I18n::new`.
    #[test]
    fn the_library_locale_covers_the_configs() {
        assert_eq!(library_locale("en"), "en-US");
        assert_eq!(library_locale("zh-CN"), "zh-CN");
        assert_eq!(library_locale("fr"), "en-US");
        for locale in ["en", "zh-CN", ""] {
            assert!(
                ely_core::i18n::LOCALES
                    .iter()
                    .any(|known| known.tag == library_locale(locale)),
                "{locale}"
            );
        }
    }

    #[test]
    fn the_configs_defaults_are_the_designs() {
        let settings = ThemeSettings::from_config(&config::Appearance::default());
        assert_eq!(settings, ThemeSettings::default());
    }

    #[test]
    fn every_layer_is_read_off_the_config() {
        let appearance = config::Appearance {
            mode: config::ThemeMode::System,
            accent: config::AccentChoice::Custom("#aa732b".into()),
            syntax: config::SyntaxChoice::Quiet,
            terminal_follows_mode: false,
            ..config::Appearance::default()
        };
        let settings = ThemeSettings::from_config(&appearance);
        assert_eq!(settings.mode, ModeSetting::System);
        assert_eq!(settings.accent, Accent::Custom(rgb(0xaa732b).into()));
        assert_eq!(settings.code_theme, CodeTheme::Quiet);
        assert!(!settings.terminal_follows_mode);
    }

    #[test]
    fn a_custom_accent_that_does_not_parse_degrades_to_the_default() {
        for text in ["not a color", "#12345", "#gggggg", "#1234567"] {
            let appearance = config::Appearance {
                accent: config::AccentChoice::Custom(text.into()),
                ..config::Appearance::default()
            };
            assert_eq!(
                ThemeSettings::from_config(&appearance).accent,
                Accent::Default,
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_preset_accent_repaints_the_accent_pair() {
        let settings = ThemeSettings {
            accent: Accent::Blue,
            ..ThemeSettings::default()
        };
        for mode in [Mode::Light, Mode::Dark] {
            let base = match mode {
                Mode::Light => Palette::light(false),
                Mode::Dark => Palette::dark(false),
            };
            let palette = settings.palette(mode);
            assert_eq!(palette.accent, base.chart[0]);
            assert_ne!(palette.accent_hover, palette.accent);
            let ink = contrast(palette.accent, palette.on_accent);
            assert_eq!(
                ink,
                contrast(palette.accent, base.bg).max(contrast(palette.accent, base.fg)),
                "the ink is the better of the palette's two grounds"
            );
            // Only the accent moves: focus stays the library's blue, and the
            // grounds keep their own values.
            assert_eq!(palette.focus, base.focus);
            assert_eq!(palette.bg, base.bg);
            assert_eq!(palette.surface, base.surface);
        }
    }

    #[test]
    fn the_default_accent_keeps_the_librarys_pair() {
        for mode in [Mode::Light, Mode::Dark] {
            let base = match mode {
                Mode::Light => Palette::light(false),
                Mode::Dark => Palette::dark(false),
            };
            let palette = ThemeSettings::default().palette(mode);
            assert_eq!(palette.accent, base.accent);
            assert_eq!(palette.accent_hover, base.accent_hover);
            assert_eq!(palette.on_accent, base.on_accent);
        }
    }

    #[test]
    fn a_custom_accent_is_painted_as_given() {
        let color = hsla(0.9, 0.6, 0.5, 1.0);
        let settings = ThemeSettings {
            accent: Accent::Custom(color),
            ..ThemeSettings::default()
        };
        let palette = settings.palette(Mode::Light);
        assert_eq!(palette.accent, color);
        assert_eq!(palette.accent_hover, color.mix(&palette.fg, 0.12));
    }

    #[test]
    fn each_code_theme_sets_the_palettes_code_colors() {
        let themes = syntax_themes();
        for (index, code) in CodeTheme::ALL.into_iter().enumerate() {
            assert_eq!(code.theme().name, themes[index].name);
            for mode in [Mode::Light, Mode::Dark] {
                let palette = ThemeSettings {
                    code_theme: code,
                    ..ThemeSettings::default()
                }
                .palette(mode);
                assert_eq!(palette.syntax, themes[index].of(mode));
            }
        }
    }

    #[test]
    fn the_terminal_follows_the_mode_unless_told_otherwise() {
        let follows = ThemeSettings::default();
        assert_eq!(
            follows.terminal_colors(Mode::Light),
            Palette::light(false).ansi
        );
        assert_eq!(
            follows.terminal_colors(Mode::Dark),
            Palette::dark(false).ansi
        );

        let anchored = ThemeSettings {
            terminal_follows_mode: false,
            ..ThemeSettings::default()
        };
        assert_eq!(
            anchored.terminal_colors(Mode::Light),
            Palette::dark(false).ansi,
            "the dark anchor holds in a light chrome"
        );
        assert_eq!(
            anchored.terminal_colors(Mode::Dark),
            Palette::dark(false).ansi
        );
    }

    /// The grid takes the tone's palette as it is: the sixteen entries, its
    /// ink, the design's ground, and the muted tone the grid's own chrome is
    /// drawn in. Nothing the appearance page's other layers change reaches it.
    #[test]
    fn the_terminal_theme_is_the_palettes_own_entries() {
        let settings = ThemeSettings {
            accent: Accent::Rose,
            code_theme: CodeTheme::Paper,
            ..ThemeSettings::default()
        };
        for mode in [Mode::Light, Mode::Dark] {
            let palette = tone_palette(mode);
            let theme = settings.terminal_theme(mode);
            assert_eq!(theme.palette[..16], palette.ansi.map(packed));
            assert_eq!(theme.foreground, packed(palette.fg));
            assert_eq!(theme.cursor, theme.foreground);
            assert_eq!(theme.muted, packed(palette.fg_subtle));
            assert_eq!(theme.bold, None);
            assert_eq!(
                theme.background,
                packed(match mode {
                    Mode::Light => palette.bg,
                    Mode::Dark => palette.sunken,
                }),
                "{mode:?}: the grid's ground"
            );
        }
    }

    #[test]
    fn the_accent_and_code_theme_do_not_reach_the_terminal() {
        let plain = ThemeSettings::default();
        let dressed = ThemeSettings {
            accent: Accent::Rose,
            code_theme: CodeTheme::Paper,
            ..ThemeSettings::default()
        };
        for mode in [Mode::Light, Mode::Dark] {
            assert_eq!(dressed.terminal_theme(mode), plain.terminal_theme(mode));
        }
    }

    #[test]
    fn the_anchored_terminal_keeps_the_dark_grid_in_a_light_chrome() {
        let anchored = ThemeSettings {
            terminal_follows_mode: false,
            ..ThemeSettings::default()
        };
        assert_eq!(
            anchored.terminal_theme(Mode::Light),
            anchored.terminal_theme(Mode::Dark)
        );
        assert_ne!(
            anchored.terminal_theme(Mode::Light),
            ThemeSettings::default().terminal_theme(Mode::Light)
        );
    }

    /// The packing is lossless for the palette's own hex values: an off-by-one
    /// rounding would paint a different color than the design names.
    #[test]
    fn packed_colors_round_trip_exactly() {
        for hex in [
            0x000000, 0x181613, 0x202020, 0x252525, 0x3f3f3f, 0xa34942, 0xacaaa8, 0xf3f1f0,
            0xfcfaf7, 0xffffff,
        ] {
            assert_eq!(packed(rgb(hex).into()), hex, "{hex:#08x}");
        }
    }

    /// WCAG 2 contrast as the painter would measure it, on the packed values.
    fn packed_contrast(a: u32, b: u32) -> f32 {
        fn luminance(color: u32) -> f32 {
            let channel = |shift: u32| {
                let value = ((color >> shift) & 255) as f32 / 255.;
                if value <= 0.03928 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
        }
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// The grid's own ground carries body text at 4.5:1 in both tones, and no
    /// ANSI entry vanishes into it: the six named hues clear 3:1, and even the
    /// two entries nearest the ground (black on dark, bright white on light)
    /// stay distinguishable.
    #[test]
    fn the_grids_ground_carries_its_ink() {
        let settings = ThemeSettings::default();
        for mode in [Mode::Light, Mode::Dark] {
            let theme = settings.terminal_theme(mode);
            assert!(
                packed_contrast(theme.foreground, theme.background) >= 4.5,
                "{mode:?}: body ink on the ground"
            );
            for (index, color) in theme.palette[..16].iter().enumerate() {
                assert!(
                    packed_contrast(*color, theme.background) >= 1.5,
                    "{mode:?}: ANSI {index} disappears into the ground"
                );
            }
            for index in 1..=6 {
                assert!(
                    packed_contrast(theme.palette[index], theme.background) >= 3.,
                    "{mode:?}: ANSI hue {index} is too faint for text"
                );
            }
        }
    }

    /// The library's sixteen entries come in the order every terminal reads
    /// them: black, red, green, yellow, blue, magenta, cyan, white, then the
    /// eight bright ones. The daemon's named colors address them by position
    /// (`terminal::color`), so a reordered palette would paint every program's
    /// red as its green and nothing else would notice.
    #[test]
    fn the_ansi_entries_keep_the_terminals_order() {
        let channels = |color: u32| {
            (
                ((color >> 16) & 255) as u16,
                ((color >> 8) & 255) as u16,
                (color & 255) as u16,
            )
        };
        let gray = |(r, g, b): (u16, u16, u16)| {
            let (max, min) = (r.max(g).max(b), r.min(g).min(b));
            max - min < 24
        };
        let luma = |(r, g, b): (u16, u16, u16)| (r + g + b) / 3;

        let settings = ThemeSettings::default();
        for mode in [Mode::Light, Mode::Dark] {
            let theme = settings.terminal_theme(mode);
            // Each of the two sets: black, red, green, yellow, blue, magenta,
            // cyan, white.
            for offset in [0, 8] {
                assert!(gray(channels(theme.palette[offset])), "{mode:?}: black");
                let (r, g, b) = channels(theme.palette[offset + 1]);
                assert!(r > g && r > b, "{mode:?}: red");
                let (r, g, b) = channels(theme.palette[offset + 2]);
                assert!(g > r && g > b, "{mode:?}: green");
                let (r, g, b) = channels(theme.palette[offset + 3]);
                assert!(r > b && g > b, "{mode:?}: yellow");
                let (r, g, b) = channels(theme.palette[offset + 4]);
                assert!(b > r && b > g, "{mode:?}: blue");
                let (r, g, b) = channels(theme.palette[offset + 5]);
                assert!(r > g && b > g, "{mode:?}: magenta");
                let (r, g, b) = channels(theme.palette[offset + 6]);
                assert!(g > r && b > r, "{mode:?}: cyan");
                assert!(gray(channels(theme.palette[offset + 7])), "{mode:?}: white");
            }
            // And the four grays step up: black, bright black, white, bright
            // white.
            let grays = [0, 8, 7, 15].map(|index| luma(channels(theme.palette[index])));
            assert!(
                grays.windows(2).all(|pair| pair[0] < pair[1]),
                "{mode:?}: {grays:?}"
            );
        }
    }

    #[test]
    fn every_layer_has_a_name_in_the_catalog() {
        let names = ModeSetting::ALL
            .into_iter()
            .map(ModeSetting::label_key)
            .chain(Accent::PRESETS.into_iter().map(Accent::label_key))
            .chain(CodeTheme::ALL.into_iter().map(CodeTheme::label_key))
            .chain(["theme.accent.custom"]);
        for key in names {
            assert_ne!(t(key), key, "a layer without a name: {key}");
        }
    }

    #[test]
    fn the_code_themes_go_by_the_designs_chinese_names() {
        let previous = crate::i18n::locale();
        crate::i18n::set_locale("zh-CN");
        assert_eq!(t(CodeTheme::Standard.label_key()), "标准");
        assert_eq!(t(CodeTheme::Quiet.label_key()), "静谧");
        assert_eq!(t(CodeTheme::Paper.label_key()), "纸面");
        crate::i18n::set_locale(&previous);
    }

    #[gpui::test]
    fn a_sync_dresses_the_library_in_the_configs_mode(cx: &mut TestAppContext) {
        let absent = std::env::temp_dir().join("zenix-theme-test-absent.json");
        cx.update(|cx| {
            ChromeTheme::init(cx);
            cx.set_global(config::ConfigStore::load_from(absent));
            sync(cx);
        });

        let (mode, palette, ui_face, mono_face) = cx.read(|cx| {
            let theme = cx.theme();
            (
                theme.mode(),
                theme.colors.clone(),
                theme.font_family.clone(),
                theme.mono_family.clone(),
            )
        });
        let settings = ThemeSettings::from_config(&config::Appearance::default());
        assert_eq!(mode, settings.mode.resolve(mode));
        assert_eq!(palette, settings.palette(mode));
        // The component library's own faces, untouched: zenix installs no
        // font, so a sync must leave the theme's families where it found them.
        assert_eq!(ui_face.as_ref(), "Inter");
        assert_eq!(mono_face.as_ref(), "JetBrains Mono");
    }

    /// A `mode: system` chrome follows the system's appearance while the
    /// window is open: with the four layers unchanged, an install whose
    /// resolved mode was dark must run again when the system reports light.
    /// The fake app reports a light system, so `sync` re-resolves to light.
    #[gpui::test]
    fn a_system_mode_reinstalls_when_the_resolved_mode_moves(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join("zenix-theme-system-mode-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test config dir");
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"appearance": {"mode": "system"}}"#)
            .expect("the test config file");
        cx.update(|cx| {
            ChromeTheme::init(cx);
            cx.set_global(config::ConfigStore::load_from(path));
            // What the last install left: the same four layers, resolved dark.
            let last = ThemeSettings {
                mode: ModeSetting::System,
                ..ThemeSettings::default()
            };
            cx.set_global(Installed(Some((last, Mode::Dark))));
            sync(cx);
        });

        let mode = cx.read(|cx| cx.theme().mode());
        assert_eq!(
            mode,
            Mode::Light,
            "the test platform reports a light system"
        );
    }
}
