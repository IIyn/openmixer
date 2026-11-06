//! Per-model descriptions of the Audient iD family.
//!
//! The numbers come from the reverse-engineering work of MixiD, BiD and Monix
//! (all MIT). Only models whose protocol has been confirmed on real hardware
//! get their state pushed on connect: writing the wrong cells into an
//! unverified model can silence its outputs until it is power-cycled.

pub const AUDIENT_VENDOR_ID: u16 = 0x2708;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Mic,
    Digital,
    Daw,
}

#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub name: &'static str,
    pub kind: NodeKind,
}

const fn mic(name: &'static str) -> Node {
    Node {
        name,
        kind: NodeKind::Mic,
    }
}
const fn digi(name: &'static str) -> Node {
    Node {
        name,
        kind: NodeKind::Digital,
    }
}
const fn daw(name: &'static str) -> Node {
    Node {
        name,
        kind: NodeKind::Daw,
    }
}

/// A stereo mix bus of the hardware mixer: two consecutive sends.
#[derive(Clone, Copy, Debug)]
pub struct MixBus {
    pub name: &'static str,
    /// Index of the left send; the right one is `left_send + 1`.
    pub left_send: u8,
}

/// What can feed an output pair through the routing unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RouteSource {
    Main,
    CueA,
    CueB,
    Daw,
}

impl RouteSource {
    pub const ALL: [RouteSource; 4] = [Self::Main, Self::CueA, Self::CueB, Self::Daw];

    pub fn label(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::CueA => "Cue A",
            Self::CueB => "Cue B",
            Self::Daw => "DAW",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RoutingScheme {
    /// Source-code table of the iD14 family (MixiD, confirmed on an iD14 MKII).
    Id14Table,
}

impl RoutingScheme {
    /// Source code written to routing output `output` (0-based).
    pub fn code(self, source: RouteSource, output: u8) -> u8 {
        let side = output & 1;
        match self {
            Self::Id14Table => match source {
                RouteSource::Main => 0x1b + side,
                RouteSource::CueA => 0x19,
                RouteSource::CueB => 0x1a,
                RouteSource::Daw => output,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OutputPair {
    pub name: &'static str,
    /// Routing-unit index of the left output; the right one is `left + 1`.
    pub left: u8,
}

#[derive(Debug)]
pub struct Profile {
    pub name: &'static str,
    pub product_id: u16,
    /// Mixer matrix rows, in hardware order.
    pub nodes: &'static [Node],
    /// Cells per matrix row (output channels of mixer unit 60).
    pub mixer_stride: u8,
    pub mixes: &'static [MixBus],
    /// Entity/cell layout of the mixer is known for this model.
    pub mixer_known: bool,
    /// Protocol confirmed on real hardware: full state is pushed on connect.
    pub protocol_verified: bool,
    pub routing: Option<RoutingScheme>,
    pub output_pairs: &'static [OutputPair],
    /// Channels of feature unit 12 that drive the headphone level.
    pub headphone_channels: &'static [u8],
    /// Channels of feature unit 12 on the monitor outputs. PipeWire's hardware
    /// volume can leave one side at -127 dB (the firmware ignores the left
    /// half), so they are reset to 0 dB on connect; the knob sets the level.
    pub speaker_channels: &'static [u8],
}

impl Profile {
    pub fn writable(&self) -> bool {
        self.mixer_known
    }

    pub fn mic_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Mic)
            .count()
    }
}

const MIXES_3: &[MixBus] = &[
    MixBus {
        name: "MAIN MIX",
        left_send: 0,
    },
    MixBus {
        name: "CUE A",
        left_send: 2,
    },
    MixBus {
        name: "CUE B",
        left_send: 4,
    },
];

const ID14_NODES: &[Node] = &[
    mic("Mic 1"),
    mic("Mic 2"),
    digi("Digi 1"),
    digi("Digi 2"),
    digi("Digi 3"),
    digi("Digi 4"),
    digi("Digi 5"),
    digi("Digi 6"),
    digi("Digi 7"),
    digi("Digi 8"),
    daw("DAW 1"),
    daw("DAW 2"),
    daw("DAW 3"),
    daw("DAW 4"),
];

const ID14_OUTPUTS: &[OutputPair] = &[
    OutputPair {
        name: "Monitors 1-2",
        left: 0,
    },
    OutputPair {
        name: "Outputs 3-4",
        left: 2,
    },
    OutputPair {
        name: "Headphones",
        left: 4,
    },
];

const ID24_NODES: &[Node] = &[
    mic("Mic 1"),
    mic("Mic 2"),
    digi("Digi 1"),
    digi("Digi 2"),
    digi("Digi 3"),
    digi("Digi 4"),
    digi("Digi 5"),
    digi("Digi 6"),
    digi("Digi 7"),
    digi("Digi 8"),
    daw("DAW 1"),
    daw("DAW 2"),
    daw("DAW 3"),
    daw("DAW 4"),
    daw("DAW 5"),
    daw("DAW 6"),
];

const ID44_NODES: &[Node] = &[
    mic("Mic 1"),
    mic("Mic 2"),
    mic("Mic 3"),
    mic("Mic 4"),
    digi("ADAT 1"),
    digi("ADAT 2"),
    digi("ADAT 3"),
    digi("ADAT 4"),
    digi("ADAT 5"),
    digi("ADAT 6"),
    digi("ADAT 7"),
    digi("ADAT 8"),
    digi("ADAT 9"),
    digi("ADAT 10"),
    digi("ADAT 11"),
    digi("ADAT 12"),
    digi("ADAT 13"),
    digi("ADAT 14"),
    digi("ADAT 15"),
    digi("ADAT 16"),
    daw("DAW 1"),
    daw("DAW 2"),
    daw("DAW 3"),
    daw("DAW 4"),
    daw("DAW 5"),
    daw("DAW 6"),
    daw("DAW 7"),
    daw("DAW 8"),
    daw("DAW 9"),
    daw("DAW 10"),
];

pub static PROFILES: &[Profile] = &[
    Profile {
        name: "iD14 MKII",
        product_id: 0x0008,
        nodes: ID14_NODES,
        mixer_stride: 6,
        mixes: MIXES_3,
        mixer_known: true,
        protocol_verified: true,
        routing: Some(RoutingScheme::Id14Table),
        output_pairs: ID14_OUTPUTS,
        headphone_channels: &[3, 4],
        speaker_channels: &[1, 2],
    },
    Profile {
        name: "iD14",
        product_id: 0x0002,
        nodes: ID14_NODES,
        mixer_stride: 6,
        mixes: MIXES_3,
        mixer_known: true,
        protocol_verified: false,
        routing: Some(RoutingScheme::Id14Table),
        output_pairs: ID14_OUTPUTS,
        headphone_channels: &[3, 4],
        speaker_channels: &[1, 2],
    },
    Profile {
        name: "iD24",
        product_id: 0x000d,
        nodes: ID24_NODES,
        mixer_stride: 6,
        mixes: MIXES_3,
        mixer_known: true,
        protocol_verified: true,
        routing: None,
        output_pairs: &[],
        headphone_channels: &[],
        speaker_channels: &[],
    },
    Profile {
        name: "iD44",
        product_id: 0x0005,
        nodes: ID44_NODES,
        mixer_stride: 6,
        mixes: MIXES_3,
        mixer_known: false,
        protocol_verified: false,
        routing: None,
        output_pairs: &[],
        headphone_channels: &[],
        speaker_channels: &[],
    },
    Profile {
        name: "iD44 MKII",
        product_id: 0x000b,
        nodes: ID44_NODES,
        mixer_stride: 6,
        mixes: MIXES_3,
        mixer_known: false,
        protocol_verified: false,
        routing: None,
        output_pairs: &[],
        headphone_channels: &[],
        speaker_channels: &[],
    },
];

pub fn find_profile(product_id: u16) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.product_id == product_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id14_routing_matches_mixid_table() {
        let s = RoutingScheme::Id14Table;
        assert_eq!(s.code(RouteSource::Main, 0), 0x1b);
        assert_eq!(s.code(RouteSource::Main, 5), 0x1c);
        assert_eq!(s.code(RouteSource::CueA, 3), 0x19);
        assert_eq!(s.code(RouteSource::CueB, 4), 0x1a);
        assert_eq!(s.code(RouteSource::Daw, 2), 0x02);
    }

    #[test]
    fn profiles_fit_mixer_addressing() {
        for p in PROFILES {
            // wValue low byte holds the cell index.
            assert!(
                p.nodes.len() * usize::from(p.mixer_stride) <= 256,
                "{}",
                p.name
            );
        }
    }
}
