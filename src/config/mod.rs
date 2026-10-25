//! zenix's own configuration: what this client remembers between runs.
//!
//! ## Which file owns which setting
//!
//! Two configurations meet in this client, and one rule separates them: **a
//! setting lives with whatever acts on it.**
//!
//! - `zenix/config.json` (this module, `~/.config/zenix/config.json`) holds what
//!   the *client* acts on and the daemon has no opinion about: the chrome's
//!   light/dark mode, the UI language, the terminal palettes the client resolves
//!   named colors against, and the client's own layout (sidebar visibility and
//!   width).
//! - the daemon's own `config.toml` holds what the *daemon* acts on: the shell a
//!   new pane starts, the cwd a new workspace starts in, the scrollback byte
//!   limit, keybindings, toasts, plugin settings. zenix sends commands and reads
//!   snapshots; it neither reads nor writes that file. Settings that belong
//!   there are pointed at from the settings panel's Config page rather than
//!   duplicated here.
//!
//! ## Schema
//!
//! The appearance, terminal and layout sections each are a group in the
//! settings panel, each key written by exactly one item there:
//!
//! ```jsonc
//! {
//!   "appearance": { "theme": "gruvbox", "theme_mode": "light", "locale": "en" },
//!   "terminal": {
//!     "palette_dark": "Default",
//!     "palette_light": "Catppuccin Latte",
//!     "font_size": 14.0,
//!     "line_height": 1.43,
//!     "cursor": "block"
//!   },
//!   "layout": {
//!     "sidebar_visible": true,
//!     "sidebar_width": 232.0,
//!     "aside_visible": false,
//!     "aside_width": 380.0
//!   },
//!   "window": { "width": 1100.0, "height": 720.0, "maximized": false }
//! }
//! ```
//!
//! The `window` section is written by the closing window itself, not by the
//! settings panel: the size it remembers is what the next launch opens into.
//!
//! The file may carry comments (JSONC: [`jsonc::parse_json_or_jsonc`]).
//!
//! ## Values that do not fit the schema
//!
//! A hand-edited file is a normal file, so a wrong value must not stop the
//! client and must not be silently obeyed either. Every load walks the file
//! against [`SECTIONS`] and reports what it found ([`Diagnostic`]): an unknown
//! key is dropped, a wrong type falls back to that field's default, a number
//! outside its range is clamped, and a file that does not parse at all keeps
//! every default. The client starts either way; the problems are listed on the
//! settings panel's Config page and counted in the status bar.
//!
//! Written by the settings panel and the sidebar's own drag, both through
//! [`update`], so the in-memory value, the file and every window move together.

pub mod jsonc;
pub mod scanner;
pub mod theme;

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use gpui_kit::component::ThemeMode;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::terminal::Theme as TerminalTheme;

/// The sidebar's width before anyone drags it, in pixels.
pub const SIDEBAR_WIDTH_DEFAULT: f32 = 232.;
/// The narrowest a stored sidebar width may be.
pub const SIDEBAR_WIDTH_MIN: f32 = 160.;
/// The widest a stored sidebar width may be: past this it is not a panel.
pub const SIDEBAR_WIDTH_MAX: f32 = 4096.;

/// The right-hand panel's width before anyone drags it, in pixels: wide enough
/// for a message and its tool card, which is what it is for.
pub const ASIDE_WIDTH_DEFAULT: f32 = 380.;
/// The narrowest a stored right-hand panel width may be.
pub const ASIDE_WIDTH_MIN: f32 = 240.;
/// The widest a stored right-hand panel width may be.
pub const ASIDE_WIDTH_MAX: f32 = 4096.;

/// The window's size on first launch, before any close has been remembered.
pub const WINDOW_SIZE_DEFAULT: (f32, f32) = (1100., 720.);
/// The narrowest a stored window may be: the window's own minimum.
pub const WINDOW_SIZE_MIN: (f32, f32) = (640., 400.);
/// The widest a stored window may be.
pub const WINDOW_SIZE_MAX: f32 = 16384.;

/// The terminal's font size before anyone changes it, in pixels: the size the
/// grid has always drawn at.
pub const FONT_SIZE_DEFAULT: f32 = 14.;
/// The smallest a stored terminal font size may be (the prototype's slider).
pub const FONT_SIZE_MIN: f32 = 10.;
/// The largest a stored terminal font size may be.
pub const FONT_SIZE_MAX: f32 = 18.;

/// The terminal's line height before anyone changes it: the multiplier that
/// turns the font size into the cell's height, chosen so the default pair
/// produces the cell the grid has always drawn (`14 × 1.43 → 20`).
pub const LINE_HEIGHT_DEFAULT: f32 = 1.43;
/// The smallest a stored line height may be: cells as tall as the glyphs.
pub const LINE_HEIGHT_MIN: f32 = 1.0;
/// The largest a stored line height may be (the prototype's slider).
pub const LINE_HEIGHT_MAX: f32 = 1.8;

/// The locales this client ships strings for.
const LOCALES: &[&str] = &["en", "zh-CN"];

/// The cursor shapes the client can draw the daemon's cursor as. The daemon
/// reports a shape per frame; this is the client's own override (the one part
/// of the cursor a painter may decide).
pub const CURSOR_SHAPES: &[&str] = &["block", "bar", "underline"];

/// The densities the chrome's lists can be drawn at: comfortable is the
/// prototype's 28-pixel row, compact its 24-pixel one — "紧凑把行高降到
/// 24px，一屏多两行".
pub const DENSITIES: &[&str] = &["comfortable", "compact"];
/// The sidebar row's height at the comfortable density, in pixels.
pub const ROW_HEIGHT_COMFORTABLE: f32 = 28.;
/// The sidebar row's height at the compact density, in pixels.
pub const ROW_HEIGHT_COMPACT: f32 = 24.;

// ── The schema ───────────────────────────────────────────────────────

/// Everything zenix remembers, as one document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigFile {
    pub appearance: Appearance,
    pub terminal: TerminalConfig,
    pub layout: Layout,
    pub window: Window,
}

/// What the chrome looks like: which theme family draws it, the mode that
/// picks the family's light or dark variant (an absent variant falls back to
/// gpui-kit's built-in slot), and the UI language. The terminal palette pair
/// follows the mode (see `ShellView`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: String,
    pub theme_mode: ThemeMode,
    pub locale: String,
    /// How tall the chrome's list rows are drawn: `"comfortable"` or
    /// `"compact"`. The grid's rows are the terminal's own metrics.
    pub density: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "default".into(),
            theme_mode: ThemeMode::Light,
            locale: "en".into(),
            density: "comfortable".into(),
        }
    }
}

impl Appearance {
    /// The height a list row is drawn at, as the density names it.
    pub fn row_height(&self) -> f32 {
        if self.density == "compact" {
            ROW_HEIGHT_COMPACT
        } else {
            ROW_HEIGHT_COMFORTABLE
        }
    }
}

/// The palettes the client resolves the daemon's named cell colors against
/// (`terminal::color`), one per mode; the metrics the grid draws cells at; and
/// the shape the cursor is drawn as. Shaping and painting are the client's,
/// so these are the client's to hold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub palette_dark: String,
    pub palette_light: String,
    /// The font's size in whole pixels. The cell's width is measured from the
    /// shaped glyphs every frame; this sets what is shaped.
    pub font_size: f32,
    /// The multiplier from font size to cell height, to two decimals.
    pub line_height: f32,
    /// The cursor shape the client draws: `"block"`, `"bar"` or `"underline"`.
    pub cursor: String,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            palette_dark: "Default".into(),
            palette_light: "Catppuccin Latte".into(),
            font_size: FONT_SIZE_DEFAULT,
            line_height: LINE_HEIGHT_DEFAULT,
            cursor: "block".into(),
        }
    }
}

impl TerminalConfig {
    /// Set the font size as the schema allows it: whole pixels, however the
    /// stepper or a hand-edited file arrived.
    pub fn set_font_size(&mut self, size: f32) {
        self.font_size = size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX).round();
    }

    /// Set the line height as the schema allows it: two decimals, so a
    /// stepper's `+0.05` cannot leave float drift in the file.
    pub fn set_line_height(&mut self, height: f32) {
        self.line_height = ((height.clamp(LINE_HEIGHT_MIN, LINE_HEIGHT_MAX) * 100.).round()) / 100.;
    }
}

/// The client's own layout. The daemon owns the panes; this is the chrome
/// around them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub sidebar_visible: bool,
    pub sidebar_width: f32,
    /// Whether the right-hand panel is drawn beside the grid.
    pub aside_visible: bool,
    pub aside_width: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            sidebar_width: SIDEBAR_WIDTH_DEFAULT,
            // Closed until it is asked for: the grid is what the window is
            // for, and the panel is one chord or one click away.
            aside_visible: false,
            aside_width: ASIDE_WIDTH_DEFAULT,
        }
    }
}

impl Layout {
    /// Set the sidebar's width as the schema allows it: whole pixels, however
    /// far the pointer or an input took it. Both writers — the sidebar's own
    /// drag and the settings panel — come through here, so the file never
    /// carries a fraction of a pixel.
    pub fn set_sidebar_width(&mut self, width: f32) {
        self.sidebar_width = width.clamp(SIDEBAR_WIDTH_MIN, SIDEBAR_WIDTH_MAX).round();
    }

    /// Set the right-hand panel's width, under the same rule as the sidebar's.
    pub fn set_aside_width(&mut self, width: f32) {
        self.aside_width = width.clamp(ASIDE_WIDTH_MIN, ASIDE_WIDTH_MAX).round();
    }
}

/// The window's geometry, remembered when it closes and honored when it opens.
/// The panes' arrangement is the daemon's own (`session.json`); this is the
/// frame the chrome opens into. The position is not kept: Wayland leaves it
/// to the compositor, and a stored X11 origin can come back off-screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Window {
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: WINDOW_SIZE_DEFAULT.0,
            height: WINDOW_SIZE_DEFAULT.1,
            maximized: false,
        }
    }
}

impl Window {
    /// Remember the closing window's size, as the schema allows it: whole
    /// pixels inside the window's own minimum. A maximized window's bounds
    /// are its restore size, so they are the ones the next launch opens at.
    pub fn set_bounds(&mut self, width: f32, height: f32, maximized: bool) {
        self.width = width.clamp(WINDOW_SIZE_MIN.0, WINDOW_SIZE_MAX).round();
        self.height = height.clamp(WINDOW_SIZE_MIN.1, WINDOW_SIZE_MAX).round();
        self.maximized = maximized;
    }
}

// ── The file, walked against the schema ──────────────────────────────

/// How a field is shaped in the file.
#[derive(Clone, Copy)]
enum Field {
    /// `true` or `false`.
    Flag,
    /// One of these strings.
    Choice(&'static [&'static str]),
    /// A number within these bounds.
    Size { min: f32, max: f32 },
}

/// One section of the file, and the fields it may carry.
struct Section {
    name: &'static str,
    fields: &'static [(&'static str, Field)],
}

/// The whole schema: what a load keeps, in the order the panel shows it.
const SECTIONS: &[Section] = &[
    Section {
        name: "appearance",
        fields: &[
            ("theme", Field::Choice(theme::NAMES)),
            ("theme_mode", Field::Choice(&["light", "dark"])),
            ("locale", Field::Choice(LOCALES)),
            ("density", Field::Choice(DENSITIES)),
        ],
    },
    Section {
        name: "terminal",
        fields: &[
            ("palette_dark", Field::Choice(TerminalTheme::BUILTIN_NAMES)),
            ("palette_light", Field::Choice(TerminalTheme::BUILTIN_NAMES)),
            (
                "font_size",
                Field::Size {
                    min: FONT_SIZE_MIN,
                    max: FONT_SIZE_MAX,
                },
            ),
            (
                "line_height",
                Field::Size {
                    min: LINE_HEIGHT_MIN,
                    max: LINE_HEIGHT_MAX,
                },
            ),
            ("cursor", Field::Choice(CURSOR_SHAPES)),
        ],
    },
    Section {
        name: "layout",
        fields: &[
            ("sidebar_visible", Field::Flag),
            (
                "sidebar_width",
                Field::Size {
                    min: SIDEBAR_WIDTH_MIN,
                    max: SIDEBAR_WIDTH_MAX,
                },
            ),
            ("aside_visible", Field::Flag),
            (
                "aside_width",
                Field::Size {
                    min: ASIDE_WIDTH_MIN,
                    max: ASIDE_WIDTH_MAX,
                },
            ),
        ],
    },
    Section {
        name: "window",
        fields: &[
            (
                "width",
                Field::Size {
                    min: WINDOW_SIZE_MIN.0,
                    max: WINDOW_SIZE_MAX,
                },
            ),
            (
                "height",
                Field::Size {
                    min: WINDOW_SIZE_MIN.1,
                    max: WINDOW_SIZE_MAX,
                },
            ),
            ("maximized", Field::Flag),
        ],
    },
];

/// What was wrong with one place in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// The file is not JSON this client can read at all.
    Syntax,
    /// A key no version of the schema knows.
    UnknownKey,
    /// A value of the wrong shape for its key.
    WrongType,
    /// A number outside the range its key allows.
    OutOfRange,
    /// The file could not be written.
    Write,
}

/// One problem found while loading or saving the file, ready to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// Where in the file, e.g. `layout.sidebar_width`; empty for the file as a
    /// whole.
    pub path: String,
    pub kind: DiagnosticKind,
    pub message: String,
}

impl Diagnostic {
    /// Whether this problem kept a setting from being used as written.
    pub fn is_blocking(&self) -> bool {
        matches!(self.kind, DiagnosticKind::Syntax | DiagnosticKind::Write)
    }

    fn unknown_key(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            kind: DiagnosticKind::UnknownKey,
            message: "unknown key — ignored, and dropped the next time the file is written".into(),
        }
    }

    fn wrong_type(path: &str, expected: &str, found: &str) -> Self {
        Self {
            path: path.to_owned(),
            kind: DiagnosticKind::WrongType,
            message: format!("expected {expected}, found {found} — using the default"),
        }
    }

    fn choice(path: &str, found: &str, allowed: &[&str]) -> Self {
        Self {
            path: path.to_owned(),
            kind: DiagnosticKind::WrongType,
            message: format!(
                "{found:?} is not one of {} — using the default",
                allowed.join(", ")
            ),
        }
    }

    fn out_of_range(path: &str, found: f64, min: f32, max: f32, used: f32) -> Self {
        Self {
            path: path.to_owned(),
            kind: DiagnosticKind::OutOfRange,
            message: format!("{found} is outside {min}..={max} — using {used}"),
        }
    }

    fn syntax(message: String) -> Self {
        Self {
            path: String::new(),
            kind: DiagnosticKind::Syntax,
            message: format!("the file could not be read: {message} — using the defaults"),
        }
    }

    fn write(message: String) -> Self {
        Self {
            path: String::new(),
            kind: DiagnosticKind::Write,
            message: format!("the file could not be written: {message}"),
        }
    }
}

/// Read the file's own contents against the schema: what to keep, and what was
/// wrong with the rest.
pub fn parse(text: &str) -> (ConfigFile, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    if text.trim().is_empty() {
        return (ConfigFile::default(), diagnostics);
    }
    let raw = match jsonc::parse_json_or_jsonc(text) {
        Ok(raw) => raw,
        Err(error) => {
            diagnostics.push(Diagnostic::syntax(error));
            return (ConfigFile::default(), diagnostics);
        }
    };
    let kept = sanitize(&raw, &mut diagnostics);
    match serde_json::from_value::<ConfigFile>(kept) {
        Ok(data) => (data, diagnostics),
        Err(error) => {
            // Every value was walked first, so this is a schema bug, not the
            // file's: keep the client running and say so.
            diagnostics.push(Diagnostic::syntax(error.to_string()));
            (ConfigFile::default(), diagnostics)
        }
    }
}

/// The subset of `raw` the schema knows, with the repairable values repaired.
fn sanitize(raw: &Value, diagnostics: &mut Vec<Diagnostic>) -> Value {
    let mut kept = Map::new();
    let Some(object) = raw.as_object() else {
        diagnostics.push(Diagnostic::wrong_type("", "an object", type_name(raw)));
        return Value::Object(kept);
    };
    for (key, value) in object {
        match SECTIONS.iter().find(|section| section.name == key) {
            Some(section) => {
                kept.insert(key.clone(), sanitize_section(section, value, diagnostics));
            }
            None => diagnostics.push(Diagnostic::unknown_key(key)),
        }
    }
    Value::Object(kept)
}

fn sanitize_section(section: &Section, raw: &Value, diagnostics: &mut Vec<Diagnostic>) -> Value {
    let mut kept = Map::new();
    let Some(object) = raw.as_object() else {
        diagnostics.push(Diagnostic::wrong_type(
            section.name,
            "an object",
            type_name(raw),
        ));
        return Value::Object(kept);
    };
    for (key, value) in object {
        let path = format!("{}.{key}", section.name);
        match section.fields.iter().find(|(name, _)| name == key) {
            Some((_, field)) => {
                if let Some(value) = keep(field, &path, value, diagnostics) {
                    kept.insert(key.clone(), value);
                }
            }
            None => diagnostics.push(Diagnostic::unknown_key(&path)),
        }
    }
    Value::Object(kept)
}

/// The value to keep for one field: the file's own when it fits, a repaired one
/// when it only just misses (a number off its range), and nothing when it does
/// not fit at all — that field then takes its default.
fn keep(
    field: &Field,
    path: &str,
    value: &Value,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Value> {
    match field {
        Field::Flag => {
            if value.is_boolean() {
                return Some(value.clone());
            }
            diagnostics.push(Diagnostic::wrong_type(
                path,
                "true or false",
                type_name(value),
            ));
        }
        Field::Choice(allowed) => match value.as_str() {
            Some(text) if allowed.contains(&text) => return Some(value.clone()),
            Some(text) => diagnostics.push(Diagnostic::choice(path, text, allowed)),
            None => diagnostics.push(Diagnostic::wrong_type(path, "a string", type_name(value))),
        },
        Field::Size { min, max } => match value.as_f64() {
            Some(number) => {
                let (min, max) = (f64::from(*min), f64::from(*max));
                let clamped = number.clamp(min, max);
                if clamped != number {
                    diagnostics.push(Diagnostic::out_of_range(
                        path,
                        number,
                        min as f32,
                        max as f32,
                        clamped as f32,
                    ));
                }
                return Some(Value::from(clamped));
            }
            None => diagnostics.push(Diagnostic::wrong_type(path, "a number", type_name(value))),
        },
    }
    None
}

/// What a JSON value is, for a diagnostic that has to read as a sentence.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

// ── The store ────────────────────────────────────────────────────────

/// The config as loaded, plus whatever the load complained about. Kept in the
/// app as a global ([`Global`]) so the settings panel's fields and the shell
/// read one value rather than one each.
pub struct ConfigStore {
    path: PathBuf,
    data: ConfigFile,
    diagnostics: Vec<Diagnostic>,
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::load_from(config_path())
    }
}

impl ConfigStore {
    /// Read the user's file. Never fails: a file that cannot be read is a
    /// diagnostic, not a reason to stop.
    pub fn load() -> Self {
        Self::load_from(config_path())
    }

    pub fn load_from(path: PathBuf) -> Self {
        let (data, diagnostics) = match fs::read_to_string(&path) {
            Ok(text) => parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (ConfigFile::default(), Vec::new())
            }
            Err(error) => {
                let mut diagnostics = Vec::new();
                diagnostics.push(Diagnostic {
                    path: String::new(),
                    kind: DiagnosticKind::Syntax,
                    message: format!("{error} — using the defaults"),
                });
                (ConfigFile::default(), diagnostics)
            }
        };
        Self {
            path,
            data,
            diagnostics,
        }
    }

    pub fn data(&self) -> &ConfigFile {
        &self.data
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Write the file and keep the diagnostics in step with it: a successful
    /// write makes the file canonical, so every problem found while reading it
    /// is resolved; a failed write replaces the previous failure instead of
    /// appending one per attempt.
    pub fn save(&mut self) -> Result<()> {
        let result = self.write();
        match &result {
            Ok(()) => self.diagnostics.clear(),
            Err(error) => {
                self.diagnostics
                    .retain(|item| item.kind != DiagnosticKind::Write);
                self.diagnostics
                    .push(Diagnostic::write(format!("{error:#}")));
            }
        }
        result
    }

    fn write(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("create the config directory")?;
        }
        let mut text = serde_json::to_string_pretty(&self.data).context("serialize the config")?;
        text.push('\n');
        fs::write(&self.path, text).context("write the config file")?;
        Ok(())
    }
}

impl Global for ConfigStore {}

// ── The config as the app uses it ────────────────────────────────────

/// Load the user's file, make it the app's config, and put the client in step
/// with it. Call once, after `gpui_kit::init`.
pub fn init(cx: &mut App) {
    cx.set_global(ConfigStore::load());
    for diagnostic in diagnostics(cx) {
        tracing::warn!(
            path = %diagnostic.path,
            kind = ?diagnostic.kind,
            "config: {}",
            diagnostic.message
        );
    }
    theme::register(cx);
    apply(cx);
}

/// The live config. Panics if [`init`] did not run, as any global does.
pub fn get(cx: &App) -> &ConfigFile {
    cx.global::<ConfigStore>().data()
}

/// Where the file is, and what was wrong with it.
pub fn diagnostics(cx: &App) -> &[Diagnostic] {
    cx.global::<ConfigStore>().diagnostics()
}

pub fn path_of(cx: &App) -> &Path {
    cx.global::<ConfigStore>().path()
}

/// Change the config: the in-memory value, the client's behavior, and the file,
/// in that order. Every writer goes through here, so the three cannot drift.
pub fn update(cx: &mut App, change: impl FnOnce(&mut ConfigFile)) {
    change(&mut cx.global_mut::<ConfigStore>().data);
    apply(cx);
    save(cx);
}

/// Write the current config out, and keep a failed write visible instead of
/// swallowing it. The store records the outcome on itself, so this only logs.
pub fn save(cx: &mut App) {
    if let Err(error) = cx.global_mut::<ConfigStore>().save() {
        tracing::warn!("config: could not be written: {error:#}");
    }
}

/// Put the client in step with the config: the chrome's theme family, the
/// mode and the locale are globals of their own, and are set from scratch on
/// every change so a value can never be half-applied.
fn apply(cx: &mut App) {
    let (theme, mode, locale) = {
        let appearance = &get(cx).appearance;
        (
            appearance.theme.clone(),
            appearance.theme_mode,
            appearance.locale.clone(),
        )
    };
    crate::i18n::set_locale(&locale);
    theme::apply(&theme, mode, cx);
}

fn config_path() -> PathBuf {
    crate::platform::zenix_config_dir().join("config.json")
}

#[cfg(test)]
mod tests {
    // `use super::*` re-exports GPUI's `test` attribute macro, which would
    // otherwise shadow the builtin one and recurse: name the builtin.
    use std::prelude::v1::test;

    use super::*;

    fn kinds(diagnostics: &[Diagnostic]) -> Vec<(String, DiagnosticKind)> {
        diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.path.clone(), diagnostic.kind))
            .collect()
    }

    #[test]
    fn an_empty_file_is_the_defaults_without_complaint() {
        for text in ["", "   \n"] {
            let (config, diagnostics) = parse(text);
            assert_eq!(config, ConfigFile::default(), "{text:?}");
            assert!(diagnostics.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn every_valid_field_loads_as_written() {
        let (config, diagnostics) = parse(
            r#"{
              // the chrome the client draws for itself
              "appearance": { "theme_mode": "dark", "locale": "zh-CN" },
              "terminal": { "palette_dark": "Nord", "palette_light": "Dracula" },
              "layout": { "sidebar_visible": false, "sidebar_width": 300.5 }
            }"#,
        );
        assert_eq!(diagnostics, Vec::new());
        assert_eq!(config.appearance.theme_mode, ThemeMode::Dark);
        assert_eq!(config.appearance.locale, "zh-CN");
        assert_eq!(config.terminal.palette_dark, "Nord");
        assert_eq!(config.terminal.palette_light, "Dracula");
        assert!(!config.layout.sidebar_visible);
        assert_eq!(config.layout.sidebar_width, 300.5);
    }

    #[test]
    fn a_theme_family_is_kept_only_if_it_is_one_of_the_names() {
        let (config, diagnostics) = parse(r#"{"appearance": { "theme": "solarized" }}"#);
        assert_eq!(config.appearance.theme, "solarized");
        assert!(diagnostics.is_empty());

        let (config, diagnostics) = parse(r#"{"appearance": { "theme": "nord" }}"#);
        assert_eq!(
            config.appearance.theme, "default",
            "an unknown family falls back"
        );
        assert_eq!(
            kinds(&diagnostics),
            vec![("appearance.theme".to_owned(), DiagnosticKind::WrongType)]
        );
    }

    #[test]
    fn an_unknown_key_is_reported_and_dropped() {
        let (config, diagnostics) = parse(
            r#"{
              "appearance": { "theme_mode": "dark", "font_family": "Lilex" },
              "wat": 1
            }"#,
        );
        assert_eq!(config.appearance.theme_mode, ThemeMode::Dark);
        assert_eq!(
            kinds(&diagnostics),
            vec![
                (
                    "appearance.font_family".to_owned(),
                    DiagnosticKind::UnknownKey
                ),
                ("wat".to_owned(), DiagnosticKind::UnknownKey),
            ]
        );
        // The known field beside it survives, and the unknown one is not in
        // what would be written back.
        let written = serde_json::to_value(&config).expect("the config serializes");
        assert_eq!(written["appearance"]["font_family"], Value::Null);
        assert_eq!(written["wat"], Value::Null);
    }

    #[test]
    fn a_wrong_type_falls_back_field_by_field() {
        let (config, diagnostics) = parse(
            r#"{
              "appearance": "dark",
              "terminal": { "palette_dark": ["Nord"] },
              "layout": { "sidebar_visible": "yes", "sidebar_width": 300.0 }
            }"#,
        );
        assert_eq!(config.appearance, Appearance::default());
        assert_eq!(config.terminal.palette_dark, "Default");
        assert!(
            config.layout.sidebar_visible,
            "a wrong type takes its field's default"
        );
        assert_eq!(config.layout.sidebar_width, 300.0, "its neighbour is kept");
        assert_eq!(
            kinds(&diagnostics),
            vec![
                ("appearance".to_owned(), DiagnosticKind::WrongType),
                (
                    "terminal.palette_dark".to_owned(),
                    DiagnosticKind::WrongType
                ),
                (
                    "layout.sidebar_visible".to_owned(),
                    DiagnosticKind::WrongType
                ),
            ]
        );
    }

    #[test]
    fn a_value_outside_its_choices_is_reported() {
        let (config, diagnostics) =
            parse(r#"{"appearance": { "theme_mode": "auto", "locale": "fr" }}"#);
        assert_eq!(config.appearance, Appearance::default());
        assert_eq!(
            kinds(&diagnostics),
            vec![
                (
                    "appearance.theme_mode".to_owned(),
                    DiagnosticKind::WrongType
                ),
                ("appearance.locale".to_owned(), DiagnosticKind::WrongType),
            ]
        );
        assert!(
            diagnostics[0].message.contains("light, dark"),
            "{}",
            diagnostics[0].message
        );
    }

    /// The density names a row height; an unknown one is the comfortable row.
    #[test]
    fn the_density_names_a_row_height() {
        let (config, diagnostics) = parse(r#"{"appearance": { "density": "compact" }}"#);
        assert_eq!(config.appearance.density, "compact");
        assert_eq!(config.appearance.row_height(), ROW_HEIGHT_COMPACT);
        assert!(diagnostics.is_empty());

        let (config, diagnostics) = parse(r#"{"appearance": { "density": "cozy" }}"#);
        assert_eq!(
            config.appearance.density, "comfortable",
            "unknown falls back"
        );
        assert_eq!(config.appearance.row_height(), ROW_HEIGHT_COMFORTABLE);
        assert_eq!(
            kinds(&diagnostics),
            vec![("appearance.density".to_owned(), DiagnosticKind::WrongType)]
        );
    }

    /// The terminal's metrics and cursor load as written, and the cursor must
    /// be one of the shapes the painter knows how to draw.
    #[test]
    fn the_terminal_metrics_and_cursor_load_as_written() {
        let (config, diagnostics) = parse(
            r#"{
              "terminal": { "font_size": 16.0, "line_height": 1.55, "cursor": "bar" }
            }"#,
        );
        assert_eq!(diagnostics, Vec::new());
        assert_eq!(config.terminal.font_size, 16.0);
        assert_eq!(config.terminal.line_height, 1.55);
        assert_eq!(config.terminal.cursor, "bar");

        let (config, diagnostics) = parse(r#"{"terminal": { "cursor": "wiggly" }}"#);
        assert_eq!(
            config.terminal.cursor, "block",
            "an unknown shape falls back"
        );
        assert_eq!(
            kinds(&diagnostics),
            vec![("terminal.cursor".to_owned(), DiagnosticKind::WrongType)]
        );
    }

    #[test]
    fn a_terminal_metric_outside_its_range_is_clamped_and_reported() {
        let (config, diagnostics) =
            parse(r#"{"terminal": { "font_size": 40.0, "line_height": 0.5 }}"#);
        assert_eq!(config.terminal.font_size, FONT_SIZE_MAX);
        assert_eq!(config.terminal.line_height, LINE_HEIGHT_MIN);
        assert_eq!(
            kinds(&diagnostics),
            vec![
                ("terminal.font_size".to_owned(), DiagnosticKind::OutOfRange),
                (
                    "terminal.line_height".to_owned(),
                    DiagnosticKind::OutOfRange
                ),
            ]
        );
    }

    /// The writers leave whole pixels and two decimals in the file, whatever
    /// the arithmetic that produced the value.
    #[test]
    fn a_written_terminal_metric_is_clamped_and_tidy() {
        let mut config = TerminalConfig::default();
        config.set_font_size(13.4);
        assert_eq!(config.font_size, 13.);
        config.set_font_size(40.);
        assert_eq!(config.font_size, FONT_SIZE_MAX);
        config.set_line_height(1.4 + 0.05);
        assert_eq!(config.line_height, 1.45);
        config.set_line_height(0.2);
        assert_eq!(config.line_height, LINE_HEIGHT_MIN);

        // The default pair produces the cell the grid has always drawn.
        let defaults = TerminalConfig::default();
        assert_eq!(
            (defaults.font_size * defaults.line_height).round(),
            20.,
            "the default metrics must not change the cell"
        );
    }

    #[test]
    fn a_number_outside_its_range_is_clamped_and_reported() {
        let (config, diagnostics) = parse(r#"{"layout": { "sidebar_width": 12.0 }}"#);
        assert_eq!(config.layout.sidebar_width, SIDEBAR_WIDTH_MIN);
        assert_eq!(
            kinds(&diagnostics),
            vec![(
                "layout.sidebar_width".to_owned(),
                DiagnosticKind::OutOfRange
            )]
        );
        assert!(
            diagnostics[0].message.contains("using 160"),
            "{}",
            diagnostics[0].message
        );
    }

    /// A width a drag wrote is a fraction of a pixel wide, and that is a value
    /// the schema allows: only a number outside its range is a problem.
    #[test]
    fn a_fractional_width_is_in_range_and_quiet() {
        let (config, diagnostics) = parse(r#"{"layout": { "sidebar_width": 349.00534 }}"#);
        assert_eq!(diagnostics, Vec::new());
        assert!((config.layout.sidebar_width - 349.00534).abs() < 0.001);
    }

    /// Whatever writes a width leaves whole pixels in the file.
    #[test]
    fn a_written_width_is_clamped_and_whole() {
        let mut config = ConfigFile::default();
        config.layout.set_sidebar_width(349.00534);
        assert_eq!(config.layout.sidebar_width, 349.);
        config.layout.set_sidebar_width(12.);
        assert_eq!(config.layout.sidebar_width, SIDEBAR_WIDTH_MIN);
        config.layout.set_sidebar_width(9000.);
        assert_eq!(config.layout.sidebar_width, SIDEBAR_WIDTH_MAX);
    }

    /// The right-hand panel's width follows the sidebar's rules, with its own
    /// range: the file cannot hold a panel too narrow to draw a message in.
    #[test]
    fn the_right_hand_panel_width_has_its_own_range() {
        let mut config = ConfigFile::default();
        config.layout.set_aside_width(511.6);
        assert_eq!(config.layout.aside_width, 512.);
        config.layout.set_aside_width(8.);
        assert_eq!(config.layout.aside_width, ASIDE_WIDTH_MIN);
        config.layout.set_aside_width(9000.);
        assert_eq!(config.layout.aside_width, ASIDE_WIDTH_MAX);
        assert!(!Layout::default().aside_visible, "closed until asked for");

        let (parsed, diagnostics) = parse(r#"{"layout": { "aside_width": 10.0 }}"#);
        assert_eq!(parsed.layout.aside_width, ASIDE_WIDTH_MIN);
        assert_eq!(
            kinds(&diagnostics),
            vec![("layout.aside_width".to_owned(), DiagnosticKind::OutOfRange)]
        );
    }

    #[test]
    fn a_broken_file_keeps_every_default_and_says_why() {
        let (config, diagnostics) = parse(r#"{ "appearance": { "theme_mode": }"#);
        assert_eq!(config, ConfigFile::default());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Syntax);
        assert!(diagnostics[0].is_blocking());
    }

    #[test]
    fn a_file_that_is_not_an_object_is_reported() {
        let (config, diagnostics) = parse("[1, 2]");
        assert_eq!(config, ConfigFile::default());
        assert_eq!(
            kinds(&diagnostics),
            vec![("".to_owned(), DiagnosticKind::WrongType)]
        );
    }

    #[test]
    fn what_is_written_loads_back_unchanged() {
        let dir = std::env::temp_dir().join(format!("zenix-config-test-{}", std::process::id()));
        let path = dir.join("config.json");
        let _ = fs::remove_dir_all(&dir);
        // A file with problems first, so the written one replaces a real load.
        fs::create_dir_all(&dir).expect("the temporary directory is created");
        fs::write(&path, r#"{"layout": {"sidebar_width": 1000.0}, "wat": 1}"#)
            .expect("the seed file is written");

        let mut store = ConfigStore::load_from(path.clone());
        assert_eq!(store.data().layout.sidebar_width, 1000.0);
        assert_eq!(store.diagnostics().len(), 1);

        store.save().expect("the config is written");
        assert_eq!(
            store.diagnostics(),
            &[],
            "a successful write resolves the problems found while reading"
        );
        let reloaded = ConfigStore::load_from(path.clone());
        assert_eq!(reloaded.data(), store.data());
        assert_eq!(
            reloaded.diagnostics(),
            &[],
            "a written file has no problems"
        );
        assert!(
            fs::read_to_string(&path)
                .expect("the file is readable")
                .ends_with('\n'),
            "the file ends with a newline"
        );

        // A missing file is not a problem either.
        let missing = ConfigStore::load_from(dir.join("absent.json"));
        assert_eq!(missing.data(), &ConfigFile::default());
        assert_eq!(missing.diagnostics(), &[]);

        // A path that cannot be written reports it rather than pretending,
        // and a second failure replaces the first instead of growing the list.
        let mut blocked = ConfigStore::load_from(dir.clone());
        assert!(blocked.save().is_err(), "a directory is not a config file");
        assert!(blocked.save().is_err(), "the path is still a directory");
        let failures: Vec<_> = blocked
            .diagnostics()
            .iter()
            .filter(|item| item.kind == DiagnosticKind::Write)
            .collect();
        assert_eq!(failures.len(), 1, "one write failure, not one per attempt");

        let _ = fs::remove_dir_all(&dir);
    }
}
