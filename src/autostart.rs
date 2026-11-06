//! Launch at login, through an XDG autostart entry.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

fn entry_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("autostart")
            .join("openmixer.desktop"),
    )
}

pub fn is_enabled() -> bool {
    entry_path().is_some_and(|p| p.exists())
}

/// Starts OpenMixer at login, in the tray only (`--hidden`).
pub fn enable() -> Result<()> {
    let path = entry_path().context("config directory not found")?;
    let exe = std::env::current_exe().context("locating the openmixer binary")?;
    let entry = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=OpenMixer\n\
         Comment=Mixer for Audient iD audio interfaces\n\
         Exec=\"{}\" --hidden\n\
         Icon=openmixer\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(&path, entry).with_context(|| format!("writing {}", path.display()))
}

pub fn disable() -> Result<()> {
    let path = entry_path().context("config directory not found")?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    }
    Ok(())
}
