//! Simple config store for persisting user preferences.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

fn default_terminal_font_size() -> f32 { 14.0 }
fn default_dark_theme_name() -> String { "system".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,
    #[serde(default)]
    pub theme_name: String,
    #[serde(default = "default_dark_theme_name")]
    pub dark_theme_name: String,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            terminal_font_size: default_terminal_font_size(),
            theme_name: String::new(),
            dark_theme_name: default_dark_theme_name(),
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
impl ConfigStore {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let data = if path.exists() {
            let content = fs::read_to_string(&path).context("read config file")?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            ConfigFile::default()
        };
        Ok(Self { path, data })
    }

    pub fn terminal_font_size(&self) -> f32 {
        self.data.terminal_font_size
    }

    pub fn set_terminal_font_size(&mut self, size: f32) {
        self.data.terminal_font_size = size;
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

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("create config dir")?;
        }
        let content = serde_json::to_string_pretty(&self.data).context("serialize config")?;
        fs::write(&self.path, content).context("write config file")?;
        Ok(())
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home).join(".config/deepin-herdr/config.json")
}
