//! Mixer state as the user sees it, and its translation to matrix cells.
//!
//! The hardware matrix cannot be read back reliably, so this state is the
//! source of truth: it is saved to disk and pushed whole on connect.

use serde::{Deserialize, Serialize};

use crate::device::profile::{NodeKind, Profile, RouteSource};
use crate::device::protocol::{self, MonitorSwitch, Request};

/// Fader travel at unity gain (0 dB is the top of the fader).
pub const UNITY: f32 = 1.0;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Channel {
    /// Fader position, 0..1 (cubic taper, 1 = 0 dB).
    pub fader: f32,
    /// -1 = left, 0 = centre, 1 = right (balance on linked pairs).
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Channel {
    fn new(fader: f32) -> Self {
        Self {
            fader,
            pan: 0.0,
            mute: false,
            solo: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Mix {
    pub channels: Vec<Channel>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Monitor {
    pub dim: bool,
    pub cut: bool,
    pub mono: bool,
    pub phase: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MixerState {
    pub mixes: Vec<Mix>,
    /// Stereo link per node pair `(2k, 2k + 1)`.
    pub links: Vec<bool>,
    /// Polarity inversion per analogue input.
    pub phase: Vec<bool>,
    pub monitor: Monitor,
    /// Last source chosen per output pair (the device cannot be read back).
    #[serde(with = "routing_serde")]
    pub routing: Vec<Option<RouteSource>>,
    pub headphones: f32,
    pub show_digital: bool,
}

/// TOML has no null: unknown routes are stored as the string "unknown".
mod routing_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::device::profile::RouteSource;

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum Slot {
        Set(RouteSource),
        Unknown(String),
    }

    pub fn serialize<S: Serializer>(v: &[Option<RouteSource>], s: S) -> Result<S::Ok, S::Error> {
        v.iter()
            .map(|r| match r {
                Some(src) => Slot::Set(*src),
                None => Slot::Unknown("unknown".to_owned()),
            })
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<Option<RouteSource>>, D::Error> {
        Ok(Vec::<Slot>::deserialize(d)?
            .into_iter()
            .map(|slot| match slot {
                Slot::Set(src) => Some(src),
                Slot::Unknown(_) => None,
            })
            .collect())
    }
}

/// One visible channel strip: a single node or a linked pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strip {
    pub first: usize,
    pub stereo: bool,
}

impl Strip {
    pub fn nodes(self) -> impl Iterator<Item = usize> {
        let n = if self.stereo { 2 } else { 1 };
        self.first..self.first + n
    }
}

/// Cubic taper: half travel ≈ -18 dB, 10 % ≈ -60 dB.
pub fn fader_to_amp(pos: f32) -> f32 {
    pos.clamp(0.0, 1.0).powi(3)
}

pub fn amp_to_db(amp: f32) -> f32 {
    if amp <= 1e-6 {
        f32::NEG_INFINITY
    } else {
        20.0 * amp.log10()
    }
}

pub fn fader_to_db(pos: f32) -> f32 {
    amp_to_db(fader_to_amp(pos))
}

pub fn format_db(db: f32) -> String {
    if !db.is_finite() || db <= -127.0 {
        "-∞".to_owned()
    } else if db <= -10.0 {
        format!("{db:.0}")
    } else {
        format!("{db:+.1}")
    }
}

/// Balance law: centre is unity on both sides, the far side fades out.
fn balance(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0))
}

impl MixerState {
    pub fn new(profile: &Profile) -> Self {
        let nodes = profile.nodes.len();
        let mut links = vec![false; nodes.div_ceil(2)];
        let channels: Vec<Channel> = profile
            .nodes
            .iter()
            .map(|n| match n.kind {
                // Computer playback at unity so both channels are heard.
                NodeKind::Daw => Channel::new(UNITY),
                // Inputs closed to avoid feedback through the speakers.
                NodeKind::Mic | NodeKind::Digital => Channel::new(0.0),
            })
            .collect();
        for (pair, link) in links.iter_mut().enumerate() {
            let a = profile.nodes.get(pair * 2);
            let b = profile.nodes.get(pair * 2 + 1);
            if let (Some(a), Some(b)) = (a, b) {
                *link = a.kind == b.kind && a.kind != NodeKind::Mic;
            }
        }
        Self {
            mixes: vec![Mix { channels }; profile.mixes.len()],
            links,
            phase: vec![false; profile.mic_count()],
            monitor: Monitor::default(),
            routing: vec![None; profile.output_pairs.len()],
            headphones: 0.6,
            show_digital: false,
        }
    }

    /// Repairs a state loaded from disk that does not fit the profile.
    pub fn fits(&self, profile: &Profile) -> bool {
        let nodes = profile.nodes.len();
        self.mixes.len() == profile.mixes.len()
            && self.mixes.iter().all(|m| m.channels.len() == nodes)
            && self.links.len() == nodes.div_ceil(2)
            && self.phase.len() == profile.mic_count()
            && self.routing.len() == profile.output_pairs.len()
    }

    pub fn is_linked(&self, node: usize) -> bool {
        self.links.get(node / 2).copied().unwrap_or(false)
    }

    pub fn strips(&self, profile: &Profile) -> Vec<Strip> {
        let mut strips = Vec::new();
        let mut node = 0;
        while let Some(n) = profile.nodes.get(node) {
            let stereo =
                node.is_multiple_of(2) && self.is_linked(node) && node + 1 < profile.nodes.len();
            if n.kind != NodeKind::Digital || self.show_digital {
                strips.push(Strip {
                    first: node,
                    stereo,
                });
            }
            node += if stereo { 2 } else { 1 };
        }
        strips
    }

    pub fn channel(&self, mix: usize, node: usize) -> Option<&Channel> {
        self.mixes.get(mix)?.channels.get(node)
    }

    /// Applies `f` to every node of a strip, keeping linked pairs identical.
    pub fn edit(&mut self, mix: usize, strip: Strip, mut f: impl FnMut(&mut Channel)) {
        if let Some(m) = self.mixes.get_mut(mix) {
            for node in strip.nodes() {
                if let Some(c) = m.channels.get_mut(node) {
                    f(c);
                }
            }
        }
    }

    pub fn toggle_link(&mut self, pair: usize) {
        let Some(link) = self.links.get_mut(pair) else {
            return;
        };
        *link = !*link;
        if *link {
            // Linking copies the left strip onto the right one in every mix.
            for m in &mut self.mixes {
                if let Some(left) = m.channels.get(pair * 2).cloned()
                    && let Some(right) = m.channels.get_mut(pair * 2 + 1)
                {
                    *right = left;
                }
            }
        }
    }

    /// Left/right send amplitudes of `node` in `mix`.
    fn sends(&self, mix: usize, node: usize) -> (f32, f32) {
        let Some(m) = self.mixes.get(mix) else {
            return (0.0, 0.0);
        };
        let any_solo = m.channels.iter().any(|c| c.solo);
        let linked = self.is_linked(node);
        // A linked pair is driven by its left channel.
        let owner = if linked { node & !1 } else { node };
        let Some(c) = m.channels.get(owner) else {
            return (0.0, 0.0);
        };
        if c.mute || (any_solo && !c.solo) {
            return (0.0, 0.0);
        }
        let amp = fader_to_amp(c.fader);
        let (bl, br) = balance(c.pan);
        if linked {
            if node.is_multiple_of(2) {
                (amp * bl, 0.0)
            } else {
                (0.0, amp * br)
            }
        } else {
            (amp * bl, amp * br)
        }
    }

    /// Requests for the crosspoints of one mix.
    pub fn mix_requests(&self, profile: &Profile, mix: usize) -> Vec<Request> {
        let Some(bus) = profile.mixes.get(mix) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(profile.nodes.len() * 2);
        for node in 0..profile.nodes.len() {
            let (l, r) = self.sends(mix, node);
            for (send, amp) in [(bus.left_send, l), (bus.left_send + 1, r)] {
                if let Some(req) =
                    protocol::mixer_cell(node, send, profile.mixer_stride, amp_to_db(amp))
                {
                    out.push(req);
                }
            }
        }
        out
    }

    pub fn monitor_requests(&self) -> Vec<Request> {
        vec![
            protocol::monitor_switch(MonitorSwitch::Dim, self.monitor.dim),
            protocol::monitor_switch(MonitorSwitch::Cut, self.monitor.cut),
            protocol::monitor_switch(MonitorSwitch::Mono, self.monitor.mono),
            protocol::monitor_switch(MonitorSwitch::Phase, self.monitor.phase),
        ]
    }

    pub fn phase_requests(&self) -> Vec<Request> {
        self.phase
            .iter()
            .enumerate()
            .filter_map(|(i, &on)| Some(protocol::input_phase(u8::try_from(i).ok()?, on)))
            .collect()
    }

    pub fn headphone_requests(&self, profile: &Profile) -> Vec<Request> {
        profile
            .headphone_channels
            .iter()
            .map(|&ch| protocol::output_volume(ch, self.headphones))
            .collect()
    }

    pub fn route_requests(&self, profile: &Profile, pair: usize) -> Vec<Request> {
        let (Some(scheme), Some(out), Some(Some(source))) = (
            profile.routing,
            profile.output_pairs.get(pair),
            self.routing.get(pair),
        ) else {
            return Vec::new();
        };
        [out.left, out.left + 1]
            .into_iter()
            .map(|o| protocol::route(o, scheme.code(*source, o)))
            .collect()
    }

    /// Everything pushed when a verified device connects. Monitor and
    /// headphone levels are left alone: they follow the front-panel knob.
    /// The monitor output trims are reset to 0 dB on both sides.
    pub fn full_push(&self, profile: &Profile) -> Vec<Request> {
        let mut reqs = Vec::new();
        for mix in 0..profile.mixes.len() {
            reqs.extend(self.mix_requests(profile, mix));
        }
        reqs.extend(self.monitor_requests());
        reqs.extend(self.phase_requests());
        reqs.extend(
            profile
                .speaker_channels
                .iter()
                .map(|&ch| protocol::output_volume(ch, 1.0)),
        );
        reqs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::profile::find_profile;

    fn id14() -> &'static Profile {
        match find_profile(0x0008) {
            Some(p) => p,
            None => unreachable!("iD14 MKII profile missing"),
        }
    }

    fn cell_db(reqs: &[Request], cell: u16) -> Option<f32> {
        reqs.iter().find(|r| r.value == 0x0100 + cell).map(|r| {
            let raw = i16::from_le_bytes([r.data[0], r.data[1]]);
            protocol::raw_to_db(raw)
        })
    }

    #[test]
    fn default_daw_pair_is_stereo_at_unity() {
        let p = id14();
        let s = MixerState::new(p);
        let reqs = s.mix_requests(p, 0);
        // DAW 1 is node 10: L send = cell 60, R send = cell 61.
        assert_eq!(cell_db(&reqs, 60), Some(0.0));
        assert_eq!(cell_db(&reqs, 61).map(f32::is_infinite), Some(true));
        // DAW 2 is node 11: L silent, R at unity.
        assert_eq!(cell_db(&reqs, 66).map(f32::is_infinite), Some(true));
        assert_eq!(cell_db(&reqs, 67), Some(0.0));
        // Every cell of the mix is written, so stale gains are cleared.
        assert_eq!(reqs.len(), p.nodes.len() * 2);
    }

    #[test]
    fn solo_silences_others() {
        let p = id14();
        let mut s = MixerState::new(p);
        let mic = Strip {
            first: 0,
            stereo: false,
        };
        s.edit(0, mic, |c| {
            c.fader = UNITY;
            c.solo = true;
        });
        let reqs = s.mix_requests(p, 0);
        assert_eq!(cell_db(&reqs, 0), Some(0.0));
        assert_eq!(cell_db(&reqs, 1), Some(0.0));
        assert_eq!(cell_db(&reqs, 60).map(f32::is_infinite), Some(true));
    }

    #[test]
    fn pan_hard_left() {
        let p = id14();
        let mut s = MixerState::new(p);
        let mic = Strip {
            first: 1,
            stereo: false,
        };
        s.edit(1, mic, |c| {
            c.fader = UNITY;
            c.pan = -1.0;
        });
        let reqs = s.mix_requests(p, 1);
        // Mic 2 is node 1; Cue A uses sends 2/3 -> cells 8/9.
        assert_eq!(cell_db(&reqs, 8), Some(0.0));
        assert_eq!(cell_db(&reqs, 9).map(f32::is_infinite), Some(true));
    }

    #[test]
    fn strips_hide_digital_by_default() {
        let p = id14();
        let s = MixerState::new(p);
        let strips = s.strips(p);
        assert_eq!(
            strips,
            vec![
                Strip {
                    first: 0,
                    stereo: false
                },
                Strip {
                    first: 1,
                    stereo: false
                },
                Strip {
                    first: 10,
                    stereo: true
                },
                Strip {
                    first: 12,
                    stereo: true
                },
            ]
        );
    }

    #[test]
    fn taper() {
        assert_eq!(fader_to_db(1.0), 0.0);
        assert!(fader_to_db(0.0).is_infinite());
        assert!((fader_to_db(0.5) + 18.06).abs() < 0.1);
        assert_eq!(format_db(f32::NEG_INFINITY), "-∞");
    }

    #[test]
    fn routing_requests() {
        let p = id14();
        let mut s = MixerState::new(p);
        assert!(s.route_requests(p, 0).is_empty());
        s.routing[2] = Some(RouteSource::Main);
        let r = s.route_requests(p, 2);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].value, 0x0604);
        assert_eq!(r[0].data, vec![0x1b]);
        assert_eq!(r[1].data, vec![0x1c]);
    }
}

#[cfg(test)]
mod persist_tests {
    use super::*;
    use crate::device::profile::PROFILES;

    #[test]
    fn state_round_trips_through_toml() {
        for p in PROFILES {
            let s = MixerState::new(p);
            let text = toml::to_string_pretty(&s);
            assert!(text.is_ok(), "{}: {:?}", p.name, text.err());
            let back: Result<MixerState, _> = toml::from_str(&text.unwrap_or_default());
            assert_eq!(back.ok(), Some(s));
        }
    }
}
