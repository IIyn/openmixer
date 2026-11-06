//! System tray icon (StatusNotifierItem), so closing the window keeps
//! OpenMixer running in the background.

use crossbeam_channel::Sender;
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{Category, Icon, MenuItem, ToolTip, Tray};

/// Icon pixels sent over D-Bus, so the tray shows the icon even when the
/// icon theme lookup fails (e.g. the launcher was never installed).
const ICON_PNGS: [&[u8]; 4] = [
    include_bytes!("../packaging/icons/openmixer-22.png"),
    include_bytes!("../packaging/icons/openmixer-32.png"),
    include_bytes!("../packaging/icons/openmixer-48.png"),
    include_bytes!("../packaging/icons/openmixer-64.png"),
];

/// Decodes an RGBA PNG to the ARGB32 (network order) layout of the SNI spec.
fn decode_icon(bytes: &[u8]) -> Option<Icon> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let data = buf
        .get(..info.buffer_size())?
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [a, r, g, b])
        .collect();
    Some(Icon {
        width: i32::try_from(info.width).ok()?,
        height: i32::try_from(info.height).ok()?,
        data,
    })
}

pub enum TrayAction {
    Show,
    Quit,
    /// Restart on the freshly installed version after an update.
    Restart,
}

struct OpenMixerTray {
    actions: Sender<TrayAction>,
    icons: Vec<Icon>,
}

impl OpenMixerTray {
    fn send(&self, action: TrayAction) {
        if self.actions.send(action).is_err() {
            log::warn!("tray action dropped: the UI is gone");
        }
    }
}

impl Tray for OpenMixerTray {
    fn id(&self) -> String {
        "openmixer".into()
    }

    fn title(&self) -> String {
        "OpenMixer".into()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn icon_name(&self) -> String {
        // Installed by packaging/install-desktop.sh.
        "openmixer".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "OpenMixer".into(),
            description: "Audient iD mixer".into(),
            ..Default::default()
        }
    }

    /// Left click on the icon.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayAction::Show);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Show OpenMixer".into(),
                icon_name: "openmixer".into(),
                activate: Box::new(|this: &mut Self| this.send(TrayAction::Show)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|this: &mut Self| this.send(TrayAction::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// A running tray icon; dropping it removes the icon.
pub struct TrayIcon {
    _handle: ksni::blocking::Handle<OpenMixerTray>,
}

/// Starts the tray icon, sending clicks to `actions`. `None` when the
/// desktop has no tray.
pub fn spawn(actions: Sender<TrayAction>) -> Option<TrayIcon> {
    let icons = ICON_PNGS.iter().filter_map(|b| decode_icon(b)).collect();
    match (OpenMixerTray { actions, icons }).spawn() {
        Ok(handle) => Some(TrayIcon { _handle: handle }),
        Err(e) => {
            log::warn!("no system tray available: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icons_decode() {
        let icons: Vec<Icon> = ICON_PNGS.iter().filter_map(|b| decode_icon(b)).collect();
        assert_eq!(icons.len(), ICON_PNGS.len());
        for icon in icons {
            assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
        }
    }
}
