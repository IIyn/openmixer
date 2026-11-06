//! System tray icon (StatusNotifierItem), so closing the window keeps
//! OpenMixer running in the background.

use crossbeam_channel::{Receiver, Sender, unbounded};
use ksni::blocking::TrayMethods;
use ksni::menu::StandardItem;
use ksni::{Category, MenuItem, ToolTip, Tray};

pub enum TrayAction {
    Show,
    Quit,
}

struct OpenMixerTray {
    actions: Sender<TrayAction>,
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
        Category::Hardware
    }

    fn icon_name(&self) -> String {
        // Installed by packaging/install-desktop.sh.
        "openmixer".into()
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
    pub actions: Receiver<TrayAction>,
}

/// Starts the tray icon, or `None` when the desktop has no tray (then
/// closing the window quits the app, as usual).
pub fn spawn() -> Option<TrayIcon> {
    let (tx, rx) = unbounded();
    match (OpenMixerTray { actions: tx }).spawn() {
        Ok(handle) => Some(TrayIcon {
            _handle: handle,
            actions: rx,
        }),
        Err(e) => {
            log::warn!("no system tray available, closing the window will quit: {e}");
            None
        }
    }
}
