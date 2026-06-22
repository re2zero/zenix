//! Configuration layer — zenix config store + JSONC parsing + config scanning.
//!
//! Sub-modules:
//! - `jsonc`: JSON-with-comments parser
//! - `scanner`: `ConfigMap` for field-preserving JSON/JSONC read/write

pub mod jsonc;
pub mod scanner;

// ── Original ConfigStore ─────────────────────────────────────────────

use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

fn default_terminal_font_size() -> f32 { 14.0 }
fn default_font_family() -> String { "Noto Sans CJK SC, Lilex".into() }
fn default_dark_theme_name() -> String { "system".into() }
fn default_locale() -> String { "en".into() }
fn default_terminal_cursor_style() -> String { "block".into() }
fn default_terminal_cursor_blink() -> bool { true }
fn default_terminal_opacity() -> f32 { 1.0 }
fn default_terminal_scrollback() -> u32 { 10000 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default)]
    pub theme_name: String,
    #[serde(default = "default_dark_theme_name")]
    pub dark_theme_name: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default)]
    pub settings_window_size: Option<(f32, f32)>,
    #[serde(default)]
    pub active_settings_tab: usize,
    #[serde(default = "default_terminal_cursor_style")]
    pub terminal_cursor_style: String,
    #[serde(default = "default_terminal_cursor_blink")]
    pub terminal_cursor_blink: bool,
    #[serde(default = "default_terminal_opacity")]
    pub terminal_opacity: f32,
    #[serde(default = "default_terminal_scrollback")]
    pub terminal_scrollback: u32,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            terminal_font_size: default_terminal_font_size(),
            font_family: default_font_family(),
            theme_name: String::new(),
            dark_theme_name: default_dark_theme_name(),
            locale: default_locale(),
            settings_window_size: None,
            active_settings_tab: 0,
            terminal_cursor_style: default_terminal_cursor_style(),
            terminal_cursor_blink: default_terminal_cursor_blink(),
            terminal_opacity: default_terminal_opacity(),
            terminal_scrollback: default_terminal_scrollback(),
        }
    }
}

pub struct ConfigStore {
    path: PathBuf,
    data: ConfigFile,
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self { path: config_path(), data: ConfigFile::default() }
    }
}
#[allow(dead_code)]
impl ConfigStore {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let data = if path.exists() {
            let content = fs::read_to_string(&path).context("read config file")?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            ConfigFile::default()
        };
        let mut store = Self { path, data };
        // If font_family is empty or default, resolve system font at runtime
        if store.data.font_family.is_empty() || store.data.font_family == default_font_family() {
            store.data.font_family = detect_system_font();
        }
        Ok(store)
    }


    pub fn terminal_font_size(&self) -> f32 {
        self.data.terminal_font_size
    }

    pub fn set_terminal_font_size(&mut self, size: f32) {
        self.data.terminal_font_size = size;
    }

    pub fn font_family(&self) -> &str {
        &self.data.font_family
    }

    pub fn set_font_family(&mut self, family: String) {
        self.data.font_family = family;
    }

    pub fn dark_theme_name(&self) -> &str {
        &self.data.dark_theme_name
    }

    pub fn set_dark_theme_name(&mut self, name: String) {
        self.data.dark_theme_name = name;
    }

    pub fn theme_name(&self) -> &str {
        &self.data.theme_name
    }

    pub fn set_theme_name(&mut self, name: String) {
        self.data.theme_name = name;
    }

    pub fn locale(&self) -> &str {
        &self.data.locale
    }

    pub fn set_locale(&mut self, locale: String) {
        self.data.locale = locale;
    }

    pub fn settings_window_size(&self) -> Option<(f32, f32)> {
        self.data.settings_window_size
    }

    pub fn set_settings_window_size(&mut self, size: Option<(f32, f32)>) {
        self.data.settings_window_size = size;
    }

    pub fn active_settings_tab(&self) -> usize {
        self.data.active_settings_tab
    }

    pub fn set_active_settings_tab(&mut self, tab: usize) {
        self.data.active_settings_tab = tab;
    }

    pub fn terminal_cursor_style(&self) -> &str {
        &self.data.terminal_cursor_style
    }

    pub fn set_terminal_cursor_style(&mut self, style: String) {
        self.data.terminal_cursor_style = style;
    }

    pub fn terminal_cursor_blink(&self) -> bool {
        self.data.terminal_cursor_blink
    }

    pub fn set_terminal_cursor_blink(&mut self, blink: bool) {
        self.data.terminal_cursor_blink = blink;
    }

    pub fn terminal_opacity(&self) -> f32 {
        self.data.terminal_opacity
    }

    pub fn set_terminal_opacity(&mut self, opacity: f32) {
        self.data.terminal_opacity = opacity;
    }

    pub fn terminal_scrollback(&self) -> u32 {
        self.data.terminal_scrollback
    }

    pub fn set_terminal_scrollback(&mut self, scrollback: u32) {
        self.data.terminal_scrollback = scrollback;
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("create config dir")?;
        }
        let content = serde_json::to_string_pretty(&self.data).context("serialize config")?;
        fs::write(&self.path, content).context("write config file")?;
        Ok(())
    }
}

/// Detect the system UI font at runtime via fc-match.
/// Falls back to "Noto Sans CJK SC, Lilex" if detection fails.
fn detect_system_font() -> String {
    if let Ok(output) = std::process::Command::new("fc-match")
        .args(["-s", ":lang=zh"])
        .output()
    {
        if let Some(line) = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
        {
            if let Some(name) = line.split(':').nth(1) {
                let name = name.trim();
                if let Some(start) = name.find('"') {
                    if let Some(end) = name[start + 1..].find('"') {
                        let font = &name[start + 1..start + 1 + end];
                        if !font.is_empty() {
                            return format!("{font}, Lilex");
                        }
                    }
                }
            }
        }
    }
    "Noto Sans CJK SC, Lilex".into()
}

fn config_path() -> PathBuf {
    crate::platform::zenix_config_dir().join("config.json")
}