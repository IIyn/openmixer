//! Fixes for the Linux audio stack around the iD interfaces.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The iD14 firmware ignores writes to the left half of the hardware
/// playback volume, so PipeWire's hardware volume tilts the image to one
/// side. Making PipeWire use software volume for Audient sinks avoids it.
pub const WIREPLUMBER_SOFT_MIXER: &str = r#"# Installed by OpenMixer.
# Audient iD firmware ignores the left half of the hardware volume,
# so let PipeWire handle volume in software.
monitor.alsa.rules = [
  {
    matches = [ { node.name = "~alsa_output.usb-Audient.*" } ]
    actions = { update-props = { api.alsa.soft-mixer = true } }
  }
]
"#;

/// Gives the logged-in user access to the USB device (needed for control).
pub const UDEV_RULE: &str = include_str!("../packaging/70-openmixer-audient.rules");

pub const UDEV_INSTALL_COMMAND: &str = "sudo install -m644 packaging/70-openmixer-audient.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger --attr-match=idVendor=2708";

fn wireplumber_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("wireplumber")
            .join("wireplumber.conf.d")
            .join("50-openmixer-audient.conf"),
    )
}

pub fn soft_mixer_installed() -> bool {
    wireplumber_path().is_some_and(|p| p.exists())
}

pub fn install_soft_mixer() -> Result<()> {
    let path = wireplumber_path().context("config directory not found")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(&path, WIREPLUMBER_SOFT_MIXER)
        .with_context(|| format!("writing {}", path.display()))?;
    restart_wireplumber()
}

pub fn remove_soft_mixer() -> Result<()> {
    let path = wireplumber_path().context("config directory not found")?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    }
    restart_wireplumber()
}

fn restart_wireplumber() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "restart", "wireplumber"])
        .status()
        .context("running systemctl")?;
    if !status.success() {
        bail!("systemctl --user restart wireplumber failed ({status})");
    }
    Ok(())
}
