//! Colours and sizes of the mixer UI.

use gpui::{Rgba, rgb};

pub fn bg() -> Rgba {
    rgb(0x16171b)
}
pub fn panel() -> Rgba {
    rgb(0x202227)
}
pub fn panel_raised() -> Rgba {
    rgb(0x2a2d33)
}
pub fn border() -> Rgba {
    rgb(0x34373e)
}
pub fn text() -> Rgba {
    rgb(0xe6e7ea)
}
pub fn text_dim() -> Rgba {
    rgb(0x8b8f99)
}
pub fn accent() -> Rgba {
    rgb(0x3fa7d6)
}
pub fn track() -> Rgba {
    rgb(0x0f1013)
}
pub fn thumb() -> Rgba {
    rgb(0xd5d7dc)
}
pub fn mute() -> Rgba {
    rgb(0xd64545)
}
pub fn solo() -> Rgba {
    rgb(0xe0b030)
}
pub fn link() -> Rgba {
    rgb(0x4fbf7a)
}
pub fn warn_bg() -> Rgba {
    rgb(0x4a3a12)
}
pub fn error_bg() -> Rgba {
    rgb(0x4a1c1c)
}
pub fn ok() -> Rgba {
    rgb(0x4fbf7a)
}

/// Meter colour for a 0..1 display level.
pub fn meter(level: f32) -> Rgba {
    if level > 0.92 {
        rgb(0xe04848)
    } else if level > 0.75 {
        rgb(0xe0b030)
    } else {
        rgb(0x4fbf7a)
    }
}

pub const FADER_H: f32 = 220.0;
pub const FADER_W: f32 = 26.0;
pub const THUMB_H: f32 = 34.0;
pub const METER_W: f32 = 6.0;
pub const PAN_W: f32 = 64.0;
pub const STRIP_W: f32 = 88.0;
pub const MONITOR_H: f32 = 180.0;
