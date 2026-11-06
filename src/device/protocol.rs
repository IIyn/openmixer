//! Wire encoding of the Audient iD control requests.
//!
//! Every control is a UAC2-style class request:
//! `SET  bmRequestType 0x21 bRequest 0x01`, `GET 0xA1 0x01`,
//! `wValue = (selector << 8) | channel`, `wIndex = (entity << 8) | interface`,
//! little-endian payload. Levels are int16 in 1/256 dB, `0x8000` = silence.

pub const REQ_TYPE_SET: u8 = 0x21;
pub const REQ_TYPE_GET: u8 = 0xA1;
pub const REQ_CUR: u8 = 0x01;
pub const REQ_MEM: u8 = 0x03;

pub const ENTITY_INPUT: u8 = 0x0b;
pub const ENTITY_OUTPUT_VOLUME: u8 = 0x0c;
pub const ENTITY_ROUTER: u8 = 0x33;
pub const ENTITY_MONITOR: u8 = 0x36;
pub const ENTITY_MIXER: u8 = 0x3c;

pub const MONITOR_VOLUME: u16 = 0x1200;

/// Raw value meaning "silent".
pub const RAW_SILENT: i16 = i16::MIN;

/// One SET request, without the interface number (added by the transport).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub entity: u8,
    pub value: u16,
    pub data: Vec<u8>,
}

impl Request {
    /// Two requests with the same key address the same control; only the
    /// latest one needs to reach the device.
    pub fn key(&self) -> (u8, u16) {
        (self.entity, self.value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorSwitch {
    Mono = 0x00,
    Phase = 0x03,
    Cut = 0x04,
    Dim = 0x05,
}

impl MonitorSwitch {
    pub fn selector(self) -> u16 {
        (self as u16) << 8
    }
}

/// Converts a gain in dB to the device's int16 1/256 dB format.
pub fn db_to_raw(db: f32) -> i16 {
    if !db.is_finite() || db <= -128.0 {
        return RAW_SILENT;
    }
    // `as` saturates, and the clamp keeps 0x8000 reserved for silence.
    ((db * 256.0).round() as i16).max(RAW_SILENT + 1)
}

pub fn raw_to_db(raw: i16) -> f32 {
    if raw == RAW_SILENT {
        f32::NEG_INFINITY
    } else {
        f32::from(raw) / 256.0
    }
}

/// 0..1 travel linear in dB over -128..0 dB, as the front-panel knob.
pub fn level_to_raw(level: f32) -> i16 {
    let level = level.clamp(0.0, 1.0);
    if level <= 0.0005 {
        return RAW_SILENT;
    }
    (-32768.0 + 32767.0 * level).round() as i16
}

pub fn raw_to_level(raw: i16) -> f32 {
    ((f32::from(raw) + 32768.0) / 32767.0).clamp(0.0, 1.0)
}

pub fn level_to_db(level: f32) -> f32 {
    raw_to_db(level_to_raw(level))
}

fn set_i16(entity: u8, value: u16, raw: i16) -> Request {
    Request {
        entity,
        value,
        data: raw.to_le_bytes().to_vec(),
    }
}

fn set_bool(entity: u8, value: u16, on: bool) -> Request {
    Request {
        entity,
        value,
        data: vec![u8::from(on)],
    }
}

/// Gain of one crosspoint of the mixer matrix.
pub fn mixer_cell(node: usize, send: u8, stride: u8, db: f32) -> Option<Request> {
    let cell = node
        .checked_mul(usize::from(stride))?
        .checked_add(usize::from(send))?;
    let cell = u8::try_from(cell).ok()?;
    Some(set_i16(
        ENTITY_MIXER,
        0x0100 | u16::from(cell),
        db_to_raw(db),
    ))
}

pub fn monitor_volume(level: f32) -> Request {
    set_i16(ENTITY_MONITOR, MONITOR_VOLUME, level_to_raw(level))
}

pub fn monitor_switch(switch: MonitorSwitch, on: bool) -> Request {
    set_bool(ENTITY_MONITOR, switch.selector(), on)
}

/// Level of one channel of feature unit 12 (headphones on the iD14).
pub fn output_volume(channel: u8, level: f32) -> Request {
    set_i16(
        ENTITY_OUTPUT_VOLUME,
        0x0200 | u16::from(channel),
        level_to_raw(level),
    )
}

/// Polarity of analogue input `input` (0-based).
pub fn input_phase(input: u8, inverted: bool) -> Request {
    set_bool(ENTITY_INPUT, 0x0d01 + u16::from(input), inverted)
}

/// Selects the source code feeding routing output `output` (0-based).
pub fn route(output: u8, code: u8) -> Request {
    Request {
        entity: ENTITY_ROUTER,
        value: 0x0600 | u16::from(output),
        data: vec![code],
    }
}

/// Size of the meter block: two bytes per node, sixteen nodes.
pub const METER_BLOCK_LEN: usize = 32;
pub const METER_MAX_NODES: usize = METER_BLOCK_LEN / 2;

/// Decodes the GET_MEM meter block to 0..1 display levels.
/// The level of node `n` is the byte at `2n`, shown as `sqrt(byte / 255)`.
pub fn decode_meters(block: &[u8], nodes: usize) -> Vec<f32> {
    (0..nodes)
        .map(|n| {
            block
                .get(n * 2)
                .map(|&b| (f32::from(b) / 255.0).sqrt())
                .unwrap_or(0.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_encoding() {
        assert_eq!(db_to_raw(0.0), 0);
        assert_eq!(db_to_raw(-6.0), -1536);
        assert_eq!(db_to_raw(f32::NEG_INFINITY), RAW_SILENT);
        assert_eq!(db_to_raw(-200.0), RAW_SILENT);
        assert_eq!(db_to_raw(-127.999), RAW_SILENT + 1);
        assert!((raw_to_db(-1536) + 6.0).abs() < 1e-6);
        assert!(raw_to_db(RAW_SILENT).is_infinite());
    }

    #[test]
    fn level_encoding_matches_bid() {
        assert_eq!(level_to_raw(0.0), RAW_SILENT);
        assert_eq!(level_to_raw(1.0), -1);
        assert!((raw_to_level(level_to_raw(0.5)) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn mixer_cell_addressing() {
        let r = mixer_cell(10, 1, 6, 0.0);
        assert_eq!(
            r,
            Some(Request {
                entity: 0x3c,
                value: 0x0100 + 61,
                data: vec![0, 0]
            })
        );
        assert_eq!(mixer_cell(60, 0, 6, 0.0), None);
    }

    #[test]
    fn requests_match_known_captures() {
        assert_eq!(monitor_switch(MonitorSwitch::Dim, true).value, 0x0500);
        assert_eq!(input_phase(1, true).value, 0x0d02);
        assert_eq!(output_volume(3, 1.0).value, 0x0203);
        assert_eq!(route(4, 0x1b).value, 0x0604);
    }

    #[test]
    fn meters_use_even_bytes() {
        let mut block = [0u8; METER_BLOCK_LEN];
        block[2] = 255;
        block[3] = 99;
        let m = decode_meters(&block, 3);
        assert_eq!(m, vec![0.0, 1.0, 0.0]);
        assert_eq!(decode_meters(&block, 20).len(), 20);
    }
}
