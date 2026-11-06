//! Stateless building blocks: buttons, faders, meters, pan.

use gpui::{Div, FontWeight, Rgba, div, prelude::*, px};

use super::theme;

pub fn button(label: impl Into<gpui::SharedString>, active: bool, colour: Rgba) -> Div {
    let label = label.into();
    div()
        .flex()
        .items_center()
        .justify_center()
        .px_2()
        .h(px(22.0))
        .min_w(px(26.0))
        .rounded_sm()
        .border_1()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .cursor_pointer()
        .map(|d| {
            if active {
                d.bg(colour).border_color(colour).text_color(theme::bg())
            } else {
                d.bg(theme::panel_raised())
                    .border_color(theme::border())
                    .text_color(theme::text_dim())
                    .hover(|s| s.text_color(theme::text()))
            }
        })
        .child(label)
}

/// Vertical fader; `pos` is 0..1, `height` in pixels. The caller attaches
/// the mouse handlers.
pub fn fader(pos: f32, height: f32, enabled: bool) -> Div {
    let pos = pos.clamp(0.0, 1.0);
    let travel = height - theme::THUMB_H;
    let thumb_top = (1.0 - pos) * travel;
    div()
        .relative()
        .w(px(theme::FADER_W))
        .h(px(height))
        .cursor_ns_resize()
        .child(
            // groove
            div()
                .absolute()
                .left(px(theme::FADER_W / 2.0 - 2.0))
                .top(px(theme::THUMB_H / 2.0))
                .w(px(4.0))
                .h(px(travel))
                .rounded_sm()
                .bg(theme::track()),
        )
        .child(
            // active part of the groove
            div()
                .absolute()
                .left(px(theme::FADER_W / 2.0 - 2.0))
                .top(px(thumb_top + theme::THUMB_H / 2.0))
                .w(px(4.0))
                .h(px(travel - thumb_top))
                .rounded_sm()
                .bg(if enabled {
                    theme::accent()
                } else {
                    theme::border()
                }),
        )
        .child(
            div()
                .absolute()
                .left(px(0.0))
                .top(px(thumb_top))
                .w(px(theme::FADER_W))
                .h(px(theme::THUMB_H))
                .rounded_sm()
                .border_1()
                .border_color(theme::border())
                .bg(if enabled {
                    theme::thumb()
                } else {
                    theme::text_dim()
                })
                .flex()
                .items_center()
                .justify_center()
                .child(div().w(px(18.0)).h(px(2.0)).bg(theme::bg())),
        )
}

/// Vertical level meter with a peak line; levels are 0..1.
pub fn meter(level: f32, peak: f32, height: f32) -> Div {
    let level = level.clamp(0.0, 1.0);
    let peak = peak.clamp(0.0, 1.0);
    div()
        .relative()
        .w(px(theme::METER_W))
        .h(px(height))
        .rounded_sm()
        .bg(theme::track())
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .bottom(px(0.0))
                .left(px(0.0))
                .w_full()
                .h(px(level * height))
                .bg(theme::meter(level)),
        )
        .when(peak > 0.01, |d| {
            d.child(
                div()
                    .absolute()
                    .bottom(px((peak * height - 2.0).max(0.0)))
                    .left(px(0.0))
                    .w_full()
                    .h(px(2.0))
                    .bg(theme::meter(peak)),
            )
        })
}

/// Horizontal pan control; `pan` is -1..1.
pub fn pan(pan: f32) -> Div {
    let half = theme::PAN_W / 2.0;
    let x = half + pan.clamp(-1.0, 1.0) * (half - 4.0);
    let (left, width) = if x < half {
        (x, half - x)
    } else {
        (half, x - half)
    };
    div()
        .relative()
        .w(px(theme::PAN_W))
        .h(px(14.0))
        .rounded_sm()
        .bg(theme::track())
        .cursor_ew_resize()
        .child(
            div()
                .absolute()
                .top(px(5.0))
                .left(px(left))
                .w(px(width.max(1.0)))
                .h(px(4.0))
                .bg(theme::accent()),
        )
        .child(
            div()
                .absolute()
                .top(px(1.0))
                .left(px(x - 2.0))
                .w(px(4.0))
                .h(px(12.0))
                .rounded_sm()
                .bg(theme::thumb()),
        )
}

pub fn pan_label(pan: f32) -> String {
    let pct = (pan * 100.0).round() as i32;
    match pct {
        0 => "C".to_owned(),
        p if p < 0 => format!("L{}", -p),
        p => format!("R{p}"),
    }
}

pub fn section_title(label: &'static str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .text_color(theme::text_dim())
        .child(label)
}
