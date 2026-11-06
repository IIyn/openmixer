//! Application preferences (not the mixer state), in settings.toml.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppSettings {
    /// Closing the window quits OpenMixer instead of keeping it in the tray.
    pub quit_on_close: bool,
    /// Look for a new release when the window opens.
    pub check_updates: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            quit_on_close: false,
            check_updates: true,
        }
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("openmixer").join("settings.toml"))
}

pub fn load() -> AppSettings {
    path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(settings: &AppSettings) -> Result<()> {
    let path = path().context("config directory not found")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let text = toml::to_string_pretty(settings).context("serialising settings")?;
    fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}
