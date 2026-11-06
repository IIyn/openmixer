//! Persistence of the mixer state, one TOML file per model.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::device::Profile;
use crate::mixer::MixerState;

fn path(profile: &Profile) -> Option<PathBuf> {
    let file = profile.name.to_lowercase().replace(' ', "-");
    Some(
        dirs::config_dir()?
            .join("openmixer")
            .join(format!("{file}.toml")),
    )
}

/// Saved state for `profile`, or the defaults if none fits.
pub fn load(profile: &Profile) -> MixerState {
    let loaded = path(profile)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| match toml::from_str::<MixerState>(&s) {
            Ok(state) => Some(state),
            Err(e) => {
                log::warn!("saved state unreadable, using defaults: {e}");
                None
            }
        });
    match loaded {
        Some(state) if state.fits(profile) => state,
        _ => MixerState::new(profile),
    }
}

pub fn save(profile: &Profile, state: &MixerState) -> Result<()> {
    let path = path(profile).context("config directory not found")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let text = toml::to_string_pretty(state).context("serialising mixer state")?;
    // Write then rename, so a crash never leaves a truncated file.
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
