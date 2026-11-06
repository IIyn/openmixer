//! The Settings screen: general options, Linux fixes, mixer, updates.

use std::thread;

use crossbeam_channel::unbounded;
use gpui::{
    AnyElement, ClipboardItem, Context, Div, FontWeight, MouseButton, MouseDownEvent, div,
    prelude::*, px,
};

use super::widgets::{button, section_title};
use super::{MixerApp, theme};
use crate::mixer::MixerState;
use crate::updates::{self, NoteLine, ReleaseInfo};
use crate::{daemon, linux_fixes, settings};

/// How many past releases the notes list shows.
const NOTES_SHOWN: usize = 5;

pub enum UpdateMsg {
    Checked(Result<Vec<ReleaseInfo>, String>),
    Installed(Result<String, String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    Checked,
    Installing(String),
    Installed(String),
    Failed(String),
}

impl MixerApp {
    pub(super) fn latest_release(&self) -> Option<&ReleaseInfo> {
        self.releases.first()
    }

    pub(super) fn update_available(&self) -> Option<&ReleaseInfo> {
        self.latest_release().filter(|r| r.is_newer())
    }

    pub(super) fn check_updates(&mut self) {
        if matches!(
            self.update_state,
            UpdateState::Checking | UpdateState::Installing(_)
        ) {
            return;
        }
        let (tx, rx) = unbounded();
        let spawned = thread::Builder::new()
            .name("openmixer-update-check".into())
            .spawn(move || {
                let result = updates::fetch_releases().map_err(|e| format!("{e:#}"));
                tx.send(UpdateMsg::Checked(result)).ok();
            });
        match spawned {
            Ok(_) => {
                self.update_state = UpdateState::Checking;
                self.update_rx = Some(rx);
            }
            Err(e) => self.update_state = UpdateState::Failed(e.to_string()),
        }
    }

    fn install_update(&mut self) {
        let Some(release) = self.update_available().cloned() else {
            return;
        };
        let kind = self.install_kind.clone();
        let (tx, rx) = unbounded();
        let version = release.version.clone();
        let spawned = thread::Builder::new()
            .name("openmixer-update-install".into())
            .spawn(move || {
                let result = updates::install(&release, &kind)
                    .map(|()| release.version.clone())
                    .map_err(|e| format!("{e:#}"));
                tx.send(UpdateMsg::Installed(result)).ok();
            });
        match spawned {
            Ok(_) => {
                self.update_state = UpdateState::Installing(version);
                self.update_rx = Some(rx);
            }
            Err(e) => self.update_state = UpdateState::Failed(e.to_string()),
        }
    }

    /// Polls the background update thread; called every frame.
    pub(super) fn poll_updates(&mut self) {
        let Some(rx) = &self.update_rx else {
            return;
        };
        let Ok(msg) = rx.try_recv() else {
            return;
        };
        self.update_rx = None;
        self.update_state = match msg {
            UpdateMsg::Checked(Ok(releases)) => {
                self.releases = releases;
                UpdateState::Checked
            }
            UpdateMsg::Checked(Err(e)) | UpdateMsg::Installed(Err(e)) => UpdateState::Failed(e),
            UpdateMsg::Installed(Ok(version)) => UpdateState::Installed(version),
        };
    }

    fn restart(&mut self) {
        if let Err(e) = daemon::send("restart") {
            self.message = Some((
                format!("{e:#}. Close and reopen OpenMixer to use the new version."),
                true,
            ));
        }
    }

    fn set_app_settings(&mut self, f: impl FnOnce(&mut settings::AppSettings)) {
        f(&mut self.app_settings);
        if let Err(e) = settings::save(&self.app_settings) {
            self.message = Some((format!("{e:#}"), true));
        }
    }

    fn reset_mix(&mut self) {
        if !self.confirm_reset {
            self.confirm_reset = true;
            return;
        }
        self.confirm_reset = false;
        let Some(profile) = self.profile else {
            return;
        };
        let state = MixerState::new(profile);
        if self.can_write() {
            self.backend.write(state.full_push(profile));
        }
        self.state = Some(state);
        self.mark_dirty();
        self.message = Some(("Mix reset to the defaults.".into(), false));
    }

    // ---- rendering -----------------------------------------------------

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("settings")
            .flex_1()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_4()
                    .max_w(px(820.0))
                    .child(self.render_general(cx))
                    .child(self.render_linux(cx))
                    .child(self.render_mixer_settings(cx))
                    .child(self.render_updates(cx)),
            )
            .into_any_element()
    }

    fn render_general(&self, cx: &mut Context<Self>) -> Div {
        let tray_note = if self.controlled {
            "When off, closing the window quits OpenMixer."
        } else {
            "No system tray detected: closing the window always quits."
        };
        card("GENERAL")
            .child(row(
                "Start at login",
                "Starts OpenMixer in the system tray when you log in.",
                toggle(self.autostart).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, _| this.toggle_autostart()),
                ),
            ))
            .child(row(
                "Keep running in the tray",
                tray_note,
                toggle(!self.app_settings.quit_on_close).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, _| {
                        this.set_app_settings(|s| s.quit_on_close = !s.quit_on_close)
                    }),
                ),
            ))
    }

    fn render_linux(&self, cx: &mut Context<Self>) -> Div {
        let udev = if linux_fixes::udev_rule_installed() {
            div()
                .text_xs()
                .text_color(theme::ok())
                .child("Installed")
                .into_any_element()
        } else {
            button("Copy install command", false, theme::accent())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            linux_fixes::UDEV_INSTALL_COMMAND.to_owned(),
                        ));
                        this.message = Some((
                            "Command copied: paste it in a terminal at the project root.".into(),
                            false,
                        ));
                    }),
                )
                .into_any_element()
        };
        card("AUDIO / LINUX")
            .child(row(
                "L/R balance fix",
                "The firmware ignores the left half of the hardware volume, which can \
                 silence one side. Makes PipeWire use software volume for Audient \
                 outputs (restarts WirePlumber).",
                toggle(self.soft_mixer).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, _| this.toggle_soft_mixer()),
                ),
            ))
            .child(row(
                "USB access (udev rule)",
                "Lets your user talk to the interface without root. Packages \
                 (.deb/.rpm) install it automatically.",
                udev,
            ))
    }

    fn render_mixer_settings(&self, cx: &mut Context<Self>) -> Div {
        let show_digital = self.state.as_ref().is_some_and(|s| s.show_digital);
        let reset_label = if self.confirm_reset {
            "Click again to confirm"
        } else {
            "Reset mix"
        };
        card("MIXER")
            .child(row(
                "Show digital inputs",
                "Shows the optical (ADAT/S/PDIF) input channels in the mixer.",
                toggle(show_digital).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, _| {
                        if let Some(s) = &mut this.state {
                            s.show_digital = !s.show_digital;
                            this.mark_dirty();
                        }
                    }),
                ),
            ))
            .child(row(
                "Reset mix",
                "Puts every fader, pan and switch back to the defaults \
                 (computer playback at 0 dB, inputs closed).",
                button(reset_label, self.confirm_reset, theme::mute()).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, _| this.reset_mix()),
                ),
            ))
    }

    fn render_updates(&self, cx: &mut Context<Self>) -> Div {
        let status: (String, gpui::Rgba) = match &self.update_state {
            UpdateState::Idle => ("Not checked yet.".into(), theme::text_dim()),
            UpdateState::Checking => ("Checking GitHub…".into(), theme::text_dim()),
            UpdateState::Checked => match (self.update_available(), self.latest_release()) {
                (Some(r), _) => (
                    format!("Version {} is available.", r.version),
                    theme::solo(),
                ),
                (None, Some(_)) => ("OpenMixer is up to date.".into(), theme::ok()),
                (None, None) => ("No release published yet.".into(), theme::text_dim()),
            },
            UpdateState::Installing(v) => (format!("Installing {v}…"), theme::text_dim()),
            UpdateState::Installed(v) => (
                format!("Version {v} installed. Restart to use it."),
                theme::ok(),
            ),
            UpdateState::Failed(e) => (format!("Update failed: {e}"), theme::mute()),
        };

        let mut actions =
            div()
                .flex()
                .gap_2()
                .child(button("Check now", false, theme::accent()).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, _| this.check_updates()),
                ));
        if let Some(release) = self.update_available()
            && !matches!(
                self.update_state,
                UpdateState::Installing(_) | UpdateState::Installed(_)
            )
        {
            actions = if self.install_kind.can_self_update() {
                actions.child(button("Install update", true, theme::ok()).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, _| this.install_update()),
                ))
            } else {
                let url = release.url.clone();
                actions.child(
                    button("Open release page", true, theme::accent()).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, _| {
                            if let Err(e) = updates::open_in_browser(&url) {
                                this.message = Some((format!("{e:#}"), true));
                            }
                        }),
                    ),
                )
            };
        }
        if matches!(self.update_state, UpdateState::Installed(_)) {
            actions = actions.child(
                button("Restart OpenMixer", true, theme::ok()).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, _| this.restart()),
                ),
            );
        }

        let package_note = (!self.install_kind.can_self_update()).then(|| {
            div()
                .text_xs()
                .text_color(theme::text_dim())
                .child("Installed by your package manager: updates come from it.")
        });

        let notes: Vec<AnyElement> = self
            .releases
            .iter()
            .take(NOTES_SHOWN)
            .map(render_release)
            .collect();

        card("UPDATES")
            .child(row(
                "Check for updates at startup",
                "Looks for a new release on GitHub when the window opens.",
                toggle(self.app_settings.check_updates).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, _| {
                        this.set_app_settings(|s| s.check_updates = !s.check_updates)
                    }),
                ),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_xs().text_color(theme::text_dim()).child(format!(
                        "Installed: {} ({})",
                        updates::CURRENT_VERSION,
                        self.install_kind.label()
                    )))
                    .child(div().text_sm().text_color(status.1).child(status.0))
                    .children(package_note)
                    .child(actions),
            )
            .when(!notes.is_empty(), |d| {
                d.child(section_title("RELEASE NOTES")).children(notes)
            })
    }
}

fn card(title: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_md()
        .bg(theme::panel())
        .border_1()
        .border_color(theme::border())
        .child(section_title(title))
}

fn row(title: &'static str, description: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap_1()
                .child(div().text_sm().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_dim())
                        .child(description),
                ),
        )
        .child(control)
}

/// Pill-shaped on/off switch.
fn toggle(on: bool) -> Div {
    div()
        .flex()
        .items_center()
        .w(px(40.0))
        .h(px(22.0))
        .px(px(3.0))
        .rounded_full()
        .cursor_pointer()
        .bg(if on {
            theme::accent()
        } else {
            theme::panel_raised()
        })
        .border_1()
        .border_color(if on { theme::accent() } else { theme::border() })
        .when(on, |d| d.justify_end())
        .child(div().size(px(14.0)).rounded_full().bg(theme::thumb()))
}

fn render_release(release: &ReleaseInfo) -> AnyElement {
    let title =
        if release.name.is_empty() || release.name.trim_start_matches('v') == release.version {
            format!("v{}", release.version)
        } else {
            format!("v{} — {}", release.version, release.name)
        };
    let lines: Vec<AnyElement> = updates::parse_notes(&release.notes)
        .into_iter()
        .map(|line| match line {
            NoteLine::Heading(t) => div()
                .pt_1()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .child(t)
                .into_any_element(),
            NoteLine::Bullet(t) => div()
                .flex()
                .gap_2()
                .text_sm()
                .child(div().text_color(theme::accent()).child("•"))
                .child(div().flex_1().child(t))
                .into_any_element(),
            NoteLine::Text(t) => div().text_sm().child(t).into_any_element(),
            NoteLine::Blank => div().h(px(6.0)).into_any_element(),
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_md()
        .bg(theme::panel_raised())
        .child(
            div()
                .flex()
                .justify_between()
                .child(div().font_weight(FontWeight::BOLD).child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text_dim())
                        .child(release.day().to_owned()),
                ),
        )
        .when(lines.is_empty(), |d| {
            d.child(
                div()
                    .text_xs()
                    .text_color(theme::text_dim())
                    .child("No release notes."),
            )
        })
        .children(lines)
        .into_any_element()
}
