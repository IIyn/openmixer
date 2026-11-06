//! Main window: channel strips per mix, monitor section, routing.

mod settings_view;
mod theme;
mod widgets;

use std::io::{self, BufRead};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};

use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, ScrollWheelEvent, SharedString, Timer, Window, div, prelude::*,
    px,
};

use crate::autostart;
use crate::backend::{Backend, Event};
use crate::config;
use crate::device::profile::NodeKind;
use crate::device::{Profile, RouteSource, protocol};
use crate::linux_fixes;
use crate::mixer::{self, MixerState, Strip, UNITY};
use crate::settings::{self, AppSettings};
use crate::updates::{InstallKind, ReleaseInfo};
use settings_view::{UpdateMsg, UpdateState};

use theme::{FADER_H, MONITOR_H, PAN_W};
use widgets::{button, fader, meter, pan, pan_label, section_title};

const FRAME: Duration = Duration::from_millis(33);
const CONTROL_POLL: Duration = Duration::from_millis(150);
const SAVE_DELAY: Duration = Duration::from_secs(1);
/// Ignore monitor read-back right after a local change, so the knob value
/// in flight does not fight the user.
const READBACK_HOLD: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, PartialEq)]
enum DragTarget {
    Fader { mix: usize, strip: Strip },
    Pan { mix: usize, strip: Strip },
    Monitor,
    Headphones,
}

#[derive(Clone, Copy)]
struct Drag {
    target: DragTarget,
    origin: Point<Pixels>,
    start: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Mixer,
    Settings,
}

enum Status {
    Searching,
    Connected { interface: u8 },
    Offline(String),
}

#[derive(Default, Clone, Copy)]
struct MeterDisplay {
    target: f32,
    level: f32,
    peak: f32,
}

pub struct MixerApp {
    backend: Backend,
    status: Status,
    profile: Option<&'static Profile>,
    state: Option<MixerState>,
    meters: Vec<MeterDisplay>,
    monitor_level: f32,
    monitor_touched: Instant,
    active_mix: usize,
    drag: Option<Drag>,
    dirty_since: Option<Instant>,
    last_frame: Instant,
    soft_mixer: bool,
    autostart: bool,
    view: View,
    /// Started by the tray process (closing the window keeps OpenMixer alive).
    controlled: bool,
    app_settings: AppSettings,
    install_kind: InstallKind,
    update_state: UpdateState,
    update_rx: Option<Receiver<UpdateMsg>>,
    releases: Vec<ReleaseInfo>,
    confirm_reset: bool,
    message: Option<(String, bool)>,
    _quit_hook: gpui::Subscription,
}

impl MixerApp {
    pub fn new(backend: Backend, controlled: bool, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(FRAME).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        // Quitting from the tray must not lose the last second of edits.
        let quit_hook = cx.on_app_quit(|this, _| {
            if this.dirty_since.is_some() {
                this.save();
            }
            async {}
        });
        let mut app = Self {
            backend,
            status: Status::Searching,
            profile: None,
            state: None,
            meters: Vec::new(),
            monitor_level: 0.0,
            monitor_touched: Instant::now(),
            active_mix: 0,
            drag: None,
            dirty_since: None,
            last_frame: Instant::now(),
            soft_mixer: linux_fixes::soft_mixer_installed(),
            autostart: autostart::is_enabled(),
            view: View::Mixer,
            controlled,
            app_settings: settings::load(),
            install_kind: InstallKind::detect(),
            update_state: UpdateState::Idle,
            update_rx: None,
            releases: Vec::new(),
            confirm_reset: false,
            message: None,
            _quit_hook: quit_hook,
        };
        if app.app_settings.check_updates {
            app.check_updates();
        }
        app
    }

    // ---- device events -------------------------------------------------

    fn tick(&mut self, cx: &mut Context<Self>) {
        while let Ok(event) = self.backend.events.try_recv() {
            self.handle(event);
        }
        self.poll_updates();
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        for m in &mut self.meters {
            m.level = if m.target > m.level {
                m.target
            } else {
                (m.level - dt * 1.8).max(m.target)
            };
            m.peak = if m.level > m.peak {
                m.level
            } else {
                (m.peak - dt * 0.35).max(0.0)
            };
        }
        if self.dirty_since.is_some_and(|t| t.elapsed() >= SAVE_DELAY) {
            self.save();
        }
        cx.notify();
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Connected { profile, interface } => {
                self.status = Status::Connected { interface };
                if self.profile.map(|p| p.product_id) != Some(profile.product_id) {
                    self.state = Some(config::load(profile));
                    self.active_mix = 0;
                }
                self.profile = Some(profile);
                self.meters = vec![MeterDisplay::default(); profile.nodes.len()];
                if profile.protocol_verified
                    && let Some(state) = &self.state
                {
                    self.backend.write(state.full_push(profile));
                }
            }
            Event::Disconnected(reason) => {
                self.status = Status::Offline(reason);
                for m in &mut self.meters {
                    m.target = 0.0;
                }
            }
            Event::Meters(levels) => {
                for (m, l) in self.meters.iter_mut().zip(levels) {
                    m.target = l;
                }
            }
            Event::MonitorLevel(level) => {
                let dragging = matches!(
                    self.drag,
                    Some(Drag {
                        target: DragTarget::Monitor,
                        ..
                    })
                );
                if !dragging && self.monitor_touched.elapsed() > READBACK_HOLD {
                    self.monitor_level = level;
                }
            }
            Event::MonitorDim(dim) => {
                if self.monitor_touched.elapsed() > READBACK_HOLD
                    && let Some(state) = &mut self.state
                {
                    state.monitor.dim = dim;
                }
            }
            Event::WriteError(e) => self.message = Some((e, true)),
        }
    }

    /// Shows the mixer of a model without a device; nothing is written.
    fn preview(&mut self, product_id: u16) {
        if let Some(profile) = crate::device::profile::find_profile(product_id) {
            self.state = Some(config::load(profile));
            self.meters = vec![MeterDisplay::default(); profile.nodes.len()];
            self.profile = Some(profile);
        }
    }

    fn save(&mut self) {
        self.dirty_since = None;
        if let (Some(profile), Some(state)) = (self.profile, &self.state)
            && let Err(e) = config::save(profile, state)
        {
            self.message = Some((format!("{e:#}"), true));
        }
    }

    fn mark_dirty(&mut self) {
        if self.dirty_since.is_none() {
            self.dirty_since = Some(Instant::now());
        }
    }

    // ---- edits ----------------------------------------------------------

    fn can_write(&self) -> bool {
        matches!(self.status, Status::Connected { .. })
            && self.profile.is_some_and(|p| p.writable())
    }

    fn push_mix(&self, mix: usize) {
        if let (true, Some(profile), Some(state)) = (self.can_write(), self.profile, &self.state) {
            self.backend.write(state.mix_requests(profile, mix));
        }
    }

    fn edit_strip(&mut self, mix: usize, strip: Strip, f: impl FnMut(&mut mixer::Channel)) {
        if let Some(state) = &mut self.state {
            state.edit(mix, strip, f);
            self.push_mix(mix);
            self.mark_dirty();
        }
    }

    fn value(&self, target: DragTarget) -> f32 {
        let channel = |mix, strip: Strip| {
            self.state
                .as_ref()
                .and_then(|s| s.channel(mix, strip.first))
                .cloned()
        };
        match target {
            DragTarget::Fader { mix, strip } => channel(mix, strip).map_or(0.0, |c| c.fader),
            DragTarget::Pan { mix, strip } => channel(mix, strip).map_or(0.0, |c| c.pan),
            DragTarget::Monitor => self.monitor_level,
            DragTarget::Headphones => self.state.as_ref().map_or(0.0, |s| s.headphones),
        }
    }

    fn set_value(&mut self, target: DragTarget, value: f32) {
        match target {
            DragTarget::Fader { mix, strip } => {
                self.edit_strip(mix, strip, |c| c.fader = value.clamp(0.0, 1.0));
            }
            DragTarget::Pan { mix, strip } => {
                self.edit_strip(mix, strip, |c| c.pan = value.clamp(-1.0, 1.0));
            }
            DragTarget::Monitor => {
                self.monitor_level = value.clamp(0.0, 1.0);
                self.monitor_touched = Instant::now();
                if self.can_write() {
                    self.backend
                        .write(vec![protocol::monitor_volume(self.monitor_level)]);
                }
            }
            DragTarget::Headphones => {
                if let Some(state) = &mut self.state {
                    state.headphones = value.clamp(0.0, 1.0);
                }
                if let (true, Some(p), Some(s)) = (self.can_write(), self.profile, &self.state) {
                    self.backend.write(s.headphone_requests(p));
                }
                self.mark_dirty();
            }
        }
    }

    fn reset_value(&mut self, target: DragTarget) {
        let value = match target {
            DragTarget::Fader { .. } => UNITY,
            DragTarget::Pan { .. } => 0.0,
            DragTarget::Monitor | DragTarget::Headphones => return,
        };
        self.set_value(target, value);
    }

    fn begin_drag(&mut self, target: DragTarget, ev: &MouseDownEvent) {
        if ev.click_count >= 2 {
            self.reset_value(target);
            self.drag = None;
            return;
        }
        self.drag = Some(Drag {
            target,
            origin: ev.position,
            start: self.value(target),
        });
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent) {
        let Some(drag) = self.drag else {
            return;
        };
        if ev.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let dx = f32::from(ev.position.x - drag.origin.x);
        let dy = f32::from(ev.position.y - drag.origin.y);
        // Shift gives fine control.
        let scale = if ev.modifiers.shift { 0.2 } else { 1.0 };
        let value = match drag.target {
            DragTarget::Fader { .. } => drag.start - dy * scale / (FADER_H - theme::THUMB_H),
            DragTarget::Monitor | DragTarget::Headphones => {
                drag.start - dy * scale / (MONITOR_H - theme::THUMB_H)
            }
            DragTarget::Pan { .. } => drag.start + dx * scale / (PAN_W / 2.0),
        };
        self.set_value(drag.target, value);
    }

    fn on_scroll(&mut self, target: DragTarget, ev: &ScrollWheelEvent) {
        let dy = f32::from(ev.delta.pixel_delta(px(20.0)).y);
        if dy == 0.0 {
            return;
        }
        let step = if ev.modifiers.shift { 0.005 } else { 0.02 };
        let value = self.value(target) + step * dy.signum();
        self.set_value(target, value);
    }

    fn toggle_monitor(&mut self, f: impl FnOnce(&mut mixer::Monitor)) {
        let can_write = self.can_write();
        if let Some(state) = &mut self.state {
            f(&mut state.monitor);
            self.monitor_touched = Instant::now();
            if can_write {
                self.backend.write(state.monitor_requests());
            }
            self.mark_dirty();
        }
    }

    fn toggle_phase(&mut self, input: usize) {
        let can_write = self.can_write();
        if let Some(state) = &mut self.state
            && let Some(p) = state.phase.get_mut(input)
        {
            *p = !*p;
            if can_write {
                self.backend.write(state.phase_requests());
            }
            self.mark_dirty();
        }
    }

    fn toggle_link(&mut self, pair: usize) {
        if let Some(state) = &mut self.state {
            state.toggle_link(pair);
            self.mark_dirty();
        }
        if let Some(profile) = self.profile {
            for mix in 0..profile.mixes.len() {
                self.push_mix(mix);
            }
        }
    }

    fn set_route(&mut self, pair: usize, source: RouteSource) {
        if let (Some(profile), Some(state)) = (self.profile, &mut self.state)
            && let Some(slot) = state.routing.get_mut(pair)
        {
            *slot = Some(source);
            if matches!(self.status, Status::Connected { .. }) && profile.writable() {
                self.backend.write(state.route_requests(profile, pair));
            }
            self.mark_dirty();
        }
    }

    fn toggle_soft_mixer(&mut self) {
        let result = if self.soft_mixer {
            linux_fixes::remove_soft_mixer()
        } else {
            linux_fixes::install_soft_mixer()
        };
        self.soft_mixer = linux_fixes::soft_mixer_installed();
        self.message = Some(match result {
            Ok(()) if self.soft_mixer => (
                "Software volume enabled for Audient outputs (WirePlumber restarted).".into(),
                false,
            ),
            Ok(()) => ("WirePlumber fix removed.".into(), false),
            Err(e) => (format!("{e:#}"), true),
        });
    }

    fn toggle_autostart(&mut self) {
        let result = if self.autostart {
            autostart::disable()
        } else {
            autostart::enable()
        };
        self.autostart = autostart::is_enabled();
        self.message = Some(match result {
            Ok(()) if self.autostart => (
                "OpenMixer will start in the system tray at login.".into(),
                false,
            ),
            Ok(()) => ("OpenMixer will no longer start at login.".into(), false),
            Err(e) => (format!("{e:#}"), true),
        });
    }

    // ---- rendering -----------------------------------------------------

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let (status_text, status_colour) = match &self.status {
            Status::Searching => ("Searching…".to_owned(), theme::text_dim()),
            Status::Connected { interface } => {
                (format!("Connected · interface {interface}"), theme::ok())
            }
            Status::Offline(_) => ("Disconnected".to_owned(), theme::mute()),
        };
        let model = self.profile.map_or("Audient iD", |p| p.name);
        let tabs: Vec<AnyElement> = self
            .profile
            .map(|p| p.mixes)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(i, bus)| {
                button(bus.name, i == self.active_mix, theme::accent())
                    .h(px(28.0))
                    .px_3()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, _| this.active_mix = i),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .bg(theme::panel())
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child("OpenMixer"),
                    )
                    .child(div().text_color(theme::text_dim()).child(model))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(status_colour)
                            .child(div().size(px(8.0)).rounded_full().bg(status_colour))
                            .child(status_text),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(self.view == View::Mixer, |d| {
                        d.child(div().flex().gap_1().children(tabs))
                    })
                    .when_some(
                        self.update_available().map(|r| r.version.clone()),
                        |d, v| {
                            d.child(
                                button(format!("Update {v}"), true, theme::solo())
                                    .h(px(28.0))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, _| {
                                            this.view = View::Settings
                                        }),
                                    ),
                            )
                        },
                    )
                    .child(
                        button(
                            if self.view == View::Settings {
                                "← MIXER"
                            } else {
                                "SETTINGS"
                            },
                            false,
                            theme::accent(),
                        )
                        .h(px(28.0))
                        .px_3()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, _| {
                                this.confirm_reset = false;
                                this.view = match this.view {
                                    View::Mixer => View::Settings,
                                    View::Settings => View::Mixer,
                                };
                            }),
                        ),
                    ),
            )
    }

    fn render_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let banner = |bg| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_2()
                .text_sm()
                .bg(bg)
        };
        if let Status::Offline(reason) = &self.status {
            let permission = reason.contains("udev");
            let b = banner(theme::error_bg()).child(div().flex_1().child(reason.clone()));
            if !permission {
                return Some(b);
            }
            return Some(
                div()
                    .flex()
                    .flex_col()
                    .bg(theme::error_bg())
                    .pb_2()
                    .child(b.child(
                        button("Copy command", false, theme::accent()).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    linux_fixes::UDEV_INSTALL_COMMAND.to_owned(),
                                ));
                                this.message = Some((
                                    "Command copied: paste it in a terminal at the project root."
                                        .into(),
                                    false,
                                ));
                            }),
                        ),
                    ))
                    .child(
                        div()
                            .px_4()
                            .text_xs()
                            .text_color(theme::text_dim())
                            .child(linux_fixes::UDEV_INSTALL_COMMAND),
                    ),
            );
        }
        let profile = self.profile?;
        if !profile.mixer_known {
            return Some(banner(theme::warn_bg()).child(format!(
                "{} detected: its protocol is not confirmed yet, so OpenMixer stays read-only to avoid muting the interface.",
                profile.name
            )));
        }
        if !profile.protocol_verified {
            return Some(banner(theme::warn_bg()).child(format!(
                "{}: protocol not verified on this model. Nothing is sent on connect; only your manual changes are written.",
                profile.name
            )));
        }
        None
    }

    fn render_strip(
        &self,
        profile: &'static Profile,
        state: &MixerState,
        strip: Strip,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mix = self.active_mix;
        let Some(ch) = state.channel(mix, strip.first).cloned() else {
            return div().into_any_element();
        };
        let Some(node) = profile.nodes.get(strip.first) else {
            return div().into_any_element();
        };
        let name: SharedString = if strip.stereo {
            let right = profile
                .nodes
                .get(strip.first + 1)
                .map_or("", |n| n.name.rsplit(' ').next().unwrap_or(""));
            format!("{}-{right}", node.name).into()
        } else {
            node.name.into()
        };
        let enabled = profile.writable();
        let fader_target = DragTarget::Fader { mix, strip };
        let pan_target = DragTarget::Pan { mix, strip };

        let meters: Vec<AnyElement> = strip
            .nodes()
            .map(|n| {
                let m = self.meters.get(n).copied().unwrap_or_default();
                meter(m.level, m.peak, FADER_H).into_any_element()
            })
            .collect();

        let mut top = div().flex().gap_1().h(px(22.0));
        if node.kind == NodeKind::Mic {
            let input = strip.first;
            let on = state.phase.get(input).copied().unwrap_or(false);
            top = top.child(button("Ø", on, theme::accent()).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, _| this.toggle_phase(input)),
            ));
        }
        let pair = strip.first / 2;
        let pairable = profile
            .nodes
            .get(pair * 2)
            .zip(profile.nodes.get(pair * 2 + 1))
            .is_some_and(|(a, b)| a.kind == b.kind);
        if pairable && (strip.stereo || strip.first.is_multiple_of(2)) {
            top = top.child(button("LINK", strip.stereo, theme::link()).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, _| this.toggle_link(pair)),
            ));
        }

        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .w(px(theme::STRIP_W * if strip.stereo { 1.15 } else { 1.0 }))
            .p_2()
            .rounded_md()
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border())
            .child(div().text_sm().font_weight(FontWeight::BOLD).child(name))
            .child(top)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .child(
                        pan(ch.pan)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, _| {
                                    this.begin_drag(pan_target, ev)
                                }),
                            )
                            .on_scroll_wheel(cx.listener(
                                move |this, ev: &ScrollWheelEvent, _, _| {
                                    this.on_scroll(pan_target, ev)
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_dim())
                            .child(pan_label(ch.pan)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        fader(ch.fader, FADER_H, enabled)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, _, _| {
                                    this.begin_drag(fader_target, ev)
                                }),
                            )
                            .on_scroll_wheel(cx.listener(
                                move |this, ev: &ScrollWheelEvent, _, _| {
                                    this.on_scroll(fader_target, ev)
                                },
                            )),
                    )
                    .child(div().flex().gap(px(2.0)).children(meters)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme::text())
                    .child(mixer::format_db(mixer::fader_to_db(ch.fader))),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(button("M", ch.mute, theme::mute()).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, _| {
                            this.edit_strip(mix, strip, |c| c.mute = !ch.mute)
                        }),
                    ))
                    .child(button("S", ch.solo, theme::solo()).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, _| {
                            this.edit_strip(mix, strip, |c| c.solo = !ch.solo)
                        }),
                    )),
            )
            .into_any_element()
    }

    fn render_monitor(
        &self,
        profile: &'static Profile,
        state: &MixerState,
        cx: &mut Context<Self>,
    ) -> Div {
        let mon = state.monitor.clone();
        let switch = |label: &'static str, on: bool, colour, f: fn(&mut mixer::Monitor)| {
            button(label, on, colour).w(px(64.0)).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, _| this.toggle_monitor(f)),
            )
        };
        let level_fader = |target: DragTarget, level: f32, label: &'static str| {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .child(div().text_xs().text_color(theme::text_dim()).child(label))
                .child(
                    fader(level, MONITOR_H, true)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, ev: &MouseDownEvent, _, _| {
                                this.begin_drag(target, ev)
                            }),
                        )
                        .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, _| {
                            this.on_scroll(target, ev)
                        })),
                )
                .child(
                    div()
                        .text_xs()
                        .child(mixer::format_db(protocol::level_to_db(level))),
                )
        };
        let mut faders = div().flex().gap_4().child(level_fader(
            DragTarget::Monitor,
            self.monitor_level,
            "MONITOR",
        ));
        if !profile.headphone_channels.is_empty() {
            faders = faders.child(level_fader(
                DragTarget::Headphones,
                state.headphones,
                "PHONES",
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .rounded_md()
            .bg(theme::panel())
            .border_1()
            .border_color(theme::border())
            .child(section_title("MONITOR"))
            .child(
                div().flex().gap_4().child(faders).child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(switch("DIM", mon.dim, theme::solo(), |m| m.dim = !m.dim))
                        .child(switch("CUT", mon.cut, theme::mute(), |m| m.cut = !m.cut))
                        .child(switch("MONO", mon.mono, theme::accent(), |m| {
                            m.mono = !m.mono
                        }))
                        .child(switch("Ø", mon.phase, theme::accent(), |m| {
                            m.phase = !m.phase
                        })),
                ),
            )
    }

    fn render_routing(
        &self,
        profile: &'static Profile,
        state: &MixerState,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        profile.routing?;
        let rows: Vec<AnyElement> = profile
            .output_pairs
            .iter()
            .enumerate()
            .map(|(pair, out)| {
                let current = state.routing.get(pair).copied().flatten();
                let choices: Vec<AnyElement> = RouteSource::ALL
                    .iter()
                    .map(|&src| {
                        button(src.label(), current == Some(src), theme::accent())
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, _| {
                                    this.set_route(pair, src)
                                }),
                            )
                            .into_any_element()
                    })
                    .collect();
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_xs()
                            .child(out.name)
                            .when(current.is_none(), |d| {
                                d.child(div().text_color(theme::text_dim()).child("unknown"))
                            }),
                    )
                    .child(div().flex().gap_1().children(choices))
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .p_3()
                .rounded_md()
                .bg(theme::panel())
                .border_1()
                .border_color(theme::border())
                .child(section_title("OUTPUT ROUTING"))
                .children(rows),
        )
    }

    fn render_footer(&self) -> Option<Div> {
        let (msg, error) = self.message.as_ref()?;
        Some(
            div()
                .px_4()
                .py_2()
                .bg(theme::panel())
                .border_t_1()
                .border_color(theme::border())
                .text_xs()
                .text_color(if *error { theme::mute() } else { theme::ok() })
                .child(msg.clone()),
        )
    }
}

impl Render for MixerApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().flex().flex_1().gap_4().p_4().overflow_hidden();
        if self.view == View::Settings {
            body = div()
                .flex()
                .flex_1()
                .overflow_hidden()
                .child(self.render_settings(cx));
        } else {
            match (self.profile, &self.state) {
                (Some(profile), Some(state)) => {
                    let strips: Vec<AnyElement> = state
                        .strips(profile)
                        .into_iter()
                        .map(|s| self.render_strip(profile, state, s, cx))
                        .collect();
                    body = body
                        .child(
                            div()
                                .id("strips")
                                .flex()
                                .flex_1()
                                .gap_2()
                                .overflow_x_scroll()
                                .children(strips),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_4()
                                .child(self.render_monitor(profile, state, cx))
                                .children(self.render_routing(profile, state, cx)),
                        );
                }
                _ => {
                    body = body
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(
                            div()
                                .text_color(theme::text_dim())
                                .child("Connect an Audient iD interface…"),
                        )
                        .child(
                            button(
                                "Preview without interface (iD14 MKII)",
                                false,
                                theme::accent(),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, _| this.preview(0x0008)),
                            ),
                        );
                }
            }
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .text_sm()
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, _| this.on_mouse_move(ev)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
            .child(self.render_header(cx))
            .children(self.render_banner(cx))
            .child(body)
            .children(self.render_footer())
    }
}

/// Saves pending changes when the window goes away.
impl Drop for MixerApp {
    fn drop(&mut self) {
        if self.dirty_since.is_some() {
            self.save();
        }
    }
}

fn open_main_window(preview: bool, controlled: bool, cx: &mut App) {
    let backend = match Backend::spawn() {
        Ok(b) => b,
        Err(e) => {
            log::error!("could not start the USB thread: {e}");
            return;
        }
    };
    let bounds = gpui::Bounds::centered(None, gpui::size(px(1180.0), px(680.0)), cx);
    let options = gpui::WindowOptions {
        window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some("OpenMixer".into()),
            ..Default::default()
        }),
        app_id: Some("openmixer".into()),
        window_min_size: Some(gpui::size(px(760.0), px(560.0))),
        ..Default::default()
    };
    let opened = cx.open_window(options, |_, cx| {
        cx.new(|cx| {
            let mut app = MixerApp::new(backend, controlled, cx);
            if preview {
                app.preview(0x0008);
            }
            app
        })
    });
    if let Err(e) = opened {
        log::error!("could not open the window: {e:#}");
    }
}

/// Raises the mixer window, reopening it if it was closed to the tray.
fn show_main_window(cx: &mut App) {
    match cx.windows().into_iter().next() {
        Some(window) => {
            if let Err(e) = window.update(cx, |_, window, _| window.activate_window()) {
                log::warn!("could not raise the window: {e:#}");
            }
        }
        None => open_main_window(false, true, cx),
    }
    cx.activate(true);
}

/// Commands from the tray process, read from stdin.
enum Control {
    Show,
    Quit,
}

/// Reads `show` lines from stdin; end of input means the tray process
/// asked us to quit (or died).
fn spawn_stdin_reader() -> Option<Receiver<Control>> {
    let (tx, rx) = unbounded();
    let spawned = thread::Builder::new()
        .name("openmixer-stdin".into())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                match line {
                    Ok(l) if l.trim() == "show" => {
                        if tx.send(Control::Show).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            // The UI may already be gone; nothing to do then.
            tx.send(Control::Quit).ok();
        });
    match spawned {
        Ok(_) => Some(rx),
        Err(e) => {
            log::warn!("could not read commands from stdin: {e}");
            None
        }
    }
}

/// Runs the mixer window until it is closed. With `controlled`, the window
/// also obeys `show` / end-of-input on stdin (sent by the tray process).
pub fn run(preview: bool, controlled: bool) {
    let control = if controlled {
        spawn_stdin_reader()
    } else {
        None
    };
    gpui::Application::new().run(move |cx: &mut App| {
        cx.on_window_closed(move |cx| {
            if cx.windows().is_empty() {
                // "Keep running in the tray" off: take the tray process down too.
                if controlled
                    && settings::load().quit_on_close
                    && let Err(e) = crate::daemon::send("quit")
                {
                    log::warn!("{e:#}");
                }
                cx.quit();
            }
        })
        .detach();
        if let Some(control) = control {
            cx.spawn(async move |cx| {
                loop {
                    Timer::after(CONTROL_POLL).await;
                    while let Ok(cmd) = control.try_recv() {
                        let result = cx.update(|cx| match cmd {
                            Control::Show => show_main_window(cx),
                            Control::Quit => cx.quit(),
                        });
                        if result.is_err() {
                            return;
                        }
                    }
                }
            })
            .detach();
        }
        open_main_window(preview, controlled, cx);
        cx.activate(true);
    });
}
