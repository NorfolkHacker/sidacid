//! Every automatable control, its name, default (0..1 normalised) and
//! number of discrete positions, plus helpers mapping 0..1 to real units.

use serde::{Deserialize, Serialize};

pub const SYNTHS: usize = 2;
pub const KITS: usize = 2;
pub const CHANNELS: usize = 4;
pub const DRUM_INSTRUMENTS: usize = 8;
/// Number of entries in `ParamId::all()`.
pub const PARAM_COUNT: usize = 181;

macro_rules! param_enum {
    ($name:ident { $($var:ident => $label:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum $name { $($var),+ }
        impl $name {
            pub const ALL: [$name; [$($name::$var),+].len()] = [$($name::$var),+];
            pub fn name(self) -> &'static str {
                match self { $($name::$var => $label),+ }
            }
        }
    };
}

param_enum!(SynthParam {
    Tune => "Tune", Cutoff => "Cutoff", Resonance => "Resonance", EnvMod => "Env Mod",
    Decay => "Decay", Accent => "Accent", Wave => "Wave", PulseWidth => "Pulse Width",
    FilterMode => "Filter Mode", Sync => "Sync", SyncPitch => "Sync Pitch", Unison => "Unison",
    Detune => "Detune", Spread => "Spread", Volume => "Volume",
});
param_enum!(DrumParam { Tune => "Tune", Decay => "Decay", Tone => "Tone", Level => "Level" });
param_enum!(KitParam { Volume => "Volume", Accent => "Accent" });
param_enum!(ChannelParam {
    Level => "Level", Pan => "Pan", Mute => "Mute", Solo => "Solo",
    DistOn => "Dist On", DistDrive => "Drive", DistTone => "Dist Tone", DistMix => "Dist Mix",
    CompOn => "Comp On", CompThreshold => "Threshold", CompRatio => "Ratio",
    CompAttack => "Attack", CompRelease => "Release", CompMakeup => "Makeup",
    DelaySend => "Delay Send", ReverbSend => "Reverb Send",
});
param_enum!(FxParam {
    DelaySync => "Delay Sync", DelayDivision => "Delay Time", DelayMs => "Delay ms",
    DelayFeedback => "Feedback", DelayTone => "Delay Tone", DelayPingPong => "Ping-Pong",
    DelayReturn => "Delay Return", ReverbSize => "Size", ReverbDamping => "Damping",
    ReverbWidth => "Width", ReverbReturn => "Reverb Return",
});
param_enum!(MasterParam {
    CompOn => "Comp On", CompThreshold => "Threshold", CompRatio => "Ratio",
    CompAttack => "Attack", CompRelease => "Release", CompMakeup => "Makeup",
    Level => "Master", LoFi => "Lo-Fi",
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ParamId {
    Synth(u8, SynthParam),
    Drum(u8, u8, DrumParam),
    Kit(u8, KitParam),
    Channel(u8, ChannelParam),
    Fx(FxParam),
    Master(MasterParam),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamInfo {
    pub name: &'static str,
    pub default: f32,
    /// 0 = continuous; n = n discrete positions.
    pub steps: u8,
}

const fn cont(name: &'static str, default: f32) -> ParamInfo {
    ParamInfo {
        name,
        default,
        steps: 0,
    }
}

const fn disc(name: &'static str, default: f32, steps: u8) -> ParamInfo {
    ParamInfo {
        name,
        default,
        steps,
    }
}

impl ParamId {
    pub fn info(self) -> ParamInfo {
        use ChannelParam as C;
        use FxParam as F;
        use MasterParam as M;
        use SynthParam as S;
        match self {
            ParamId::Synth(_, p) => {
                let n = p.name();
                match p {
                    S::Tune => cont(n, 0.5),
                    S::Cutoff => cont(n, 0.35),
                    S::Resonance => cont(n, 0.6),
                    S::EnvMod => cont(n, 0.5),
                    S::Decay => cont(n, 0.4),
                    S::Accent => cont(n, 0.6),
                    S::Wave => disc(n, 0.0, 2),
                    S::PulseWidth => cont(n, 0.5),
                    S::FilterMode => disc(n, 0.0, 3),
                    S::Sync => disc(n, 0.0, 2),
                    S::SyncPitch => cont(n, 0.5),
                    S::Unison => disc(n, 0.0, 2),
                    S::Detune => cont(n, 0.3),
                    S::Spread => cont(n, 0.6),
                    S::Volume => cont(n, 0.7),
                }
            }
            ParamId::Drum(_, _, p) => cont(p.name(), if p == DrumParam::Level { 0.8 } else { 0.5 }),
            ParamId::Kit(_, p) => cont(p.name(), if p == KitParam::Volume { 0.7 } else { 0.5 }),
            ParamId::Channel(_, p) => {
                let n = p.name();
                match p {
                    C::Level => cont(n, 0.7),
                    C::Pan => cont(n, 0.5),
                    C::Mute | C::Solo | C::DistOn | C::CompOn => disc(n, 0.0, 2),
                    C::DistDrive => cont(n, 0.3),
                    C::DistTone => cont(n, 0.6),
                    C::DistMix => cont(n, 1.0),
                    C::CompThreshold => cont(n, 0.7),
                    C::CompRatio => cont(n, 0.25),
                    C::CompAttack => cont(n, 0.2),
                    C::CompRelease => cont(n, 0.4),
                    C::CompMakeup => cont(n, 0.0),
                    C::DelaySend | C::ReverbSend => cont(n, 0.0),
                }
            }
            ParamId::Fx(p) => {
                let n = p.name();
                match p {
                    F::DelaySync => disc(n, 1.0, 2),
                    F::DelayDivision => disc(n, 0.4, 6),
                    F::DelayMs => cont(n, 0.3),
                    F::DelayFeedback => cont(n, 0.4),
                    F::DelayTone => cont(n, 0.6),
                    F::DelayPingPong => disc(n, 1.0, 2),
                    F::DelayReturn => cont(n, 0.7),
                    F::ReverbSize => cont(n, 0.6),
                    F::ReverbDamping => cont(n, 0.5),
                    F::ReverbWidth => cont(n, 1.0),
                    F::ReverbReturn => cont(n, 0.6),
                }
            }
            ParamId::Master(p) => {
                let n = p.name();
                match p {
                    M::CompOn => disc(n, 1.0, 2),
                    M::CompThreshold => cont(n, 0.8),
                    M::CompRatio => cont(n, 0.15),
                    M::CompAttack => cont(n, 0.3),
                    M::CompRelease => cont(n, 0.4),
                    M::CompMakeup => cont(n, 0.0),
                    M::Level => cont(n, 0.8),
                    M::LoFi => disc(n, 0.0, 2),
                }
            }
        }
    }

    /// True when every index is within the machine counts.
    pub fn is_valid(self) -> bool {
        match self {
            ParamId::Synth(s, _) => (s as usize) < SYNTHS,
            ParamId::Drum(k, i, _) => (k as usize) < KITS && (i as usize) < DRUM_INSTRUMENTS,
            ParamId::Kit(k, _) => (k as usize) < KITS,
            ParamId::Channel(c, _) => (c as usize) < CHANNELS,
            ParamId::Fx(_) | ParamId::Master(_) => true,
        }
    }

    /// Position of this id in `ParamId::all()`; `None` for an invalid id.
    pub fn index(self) -> Option<usize> {
        if !self.is_valid() {
            return None;
        }
        let synth_n = SynthParam::ALL.len();
        let drum_n = DrumParam::ALL.len();
        let per_kit = DRUM_INSTRUMENTS * drum_n + KitParam::ALL.len();
        let kit_base = SYNTHS * synth_n;
        let channel_base = kit_base + KITS * per_kit;
        let fx_base = channel_base + CHANNELS * ChannelParam::ALL.len();
        let master_base = fx_base + FxParam::ALL.len();
        Some(match self {
            ParamId::Synth(s, p) => s as usize * synth_n + p as usize,
            ParamId::Drum(k, i, p) => {
                kit_base + k as usize * per_kit + i as usize * drum_n + p as usize
            }
            ParamId::Kit(k, p) => {
                kit_base + k as usize * per_kit + DRUM_INSTRUMENTS * drum_n + p as usize
            }
            ParamId::Channel(c, p) => {
                channel_base + c as usize * ChannelParam::ALL.len() + p as usize
            }
            ParamId::Fx(p) => fx_base + p as usize,
            ParamId::Master(p) => master_base + p as usize,
        })
    }

    pub fn all() -> Vec<ParamId> {
        let mut v = Vec::new();
        for s in 0..SYNTHS as u8 {
            v.extend(SynthParam::ALL.iter().map(|&p| ParamId::Synth(s, p)));
        }
        for k in 0..KITS as u8 {
            for i in 0..DRUM_INSTRUMENTS as u8 {
                v.extend(DrumParam::ALL.iter().map(|&p| ParamId::Drum(k, i, p)));
            }
            v.extend(KitParam::ALL.iter().map(|&p| ParamId::Kit(k, p)));
        }
        for c in 0..CHANNELS as u8 {
            v.extend(ChannelParam::ALL.iter().map(|&p| ParamId::Channel(c, p)));
        }
        v.extend(FxParam::ALL.iter().map(|&p| ParamId::Fx(p)));
        v.extend(MasterParam::ALL.iter().map(|&p| ParamId::Master(p)));
        v
    }
}

pub fn lin(v: f32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * v.clamp(0.0, 1.0)
}

/// Exponential map; `lo` must be > 0.
pub fn exp(v: f32, lo: f32, hi: f32) -> f32 {
    lo * (hi / lo).powf(v.clamp(0.0, 1.0))
}

pub fn step_index(v: f32, steps: u8) -> usize {
    (v.clamp(0.0, 1.0) * (steps.max(1) - 1) as f32).round() as usize
}

pub fn on(v: f32) -> bool {
    v >= 0.5
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_has_every_param_once() {
        let all = ParamId::all();
        assert_eq!(all.len(), 2 * 15 + 2 * 8 * 4 + 2 * 2 + 4 * 16 + 11 + 8);
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
    }

    #[test]
    fn defaults_are_normalised_and_names_nonempty() {
        for id in ParamId::all() {
            let info = id.info();
            assert!((0.0..=1.0).contains(&info.default), "{id:?}");
            assert!(!info.name.is_empty());
        }
    }

    #[test]
    fn index_validity() {
        assert!(ParamId::all().iter().all(|id| id.is_valid()));
        assert!(!ParamId::Synth(2, SynthParam::Tune).is_valid());
        assert!(!ParamId::Drum(0, 8, DrumParam::Tune).is_valid());
        assert!(!ParamId::Drum(2, 0, DrumParam::Tune).is_valid());
        assert!(!ParamId::Kit(2, KitParam::Volume).is_valid());
        assert!(!ParamId::Channel(4, ChannelParam::Level).is_valid());
    }

    #[test]
    fn discrete_params_report_steps() {
        assert_eq!(ParamId::Synth(0, SynthParam::Wave).info().steps, 2);
        assert_eq!(ParamId::Synth(0, SynthParam::FilterMode).info().steps, 3);
        assert_eq!(ParamId::Fx(FxParam::DelayDivision).info().steps, 6);
        assert_eq!(ParamId::Synth(0, SynthParam::Cutoff).info().steps, 0);
    }

    #[test]
    fn mapping_helpers() {
        assert_eq!(lin(0.5, -12.0, 12.0), 0.0);
        assert!((exp(0.0, 20.0, 18_000.0) - 20.0).abs() < 1e-3);
        assert!((exp(1.0, 20.0, 18_000.0) - 18_000.0).abs() < 0.5);
        assert!((exp(0.5, 100.0, 10_000.0) - 1000.0).abs() < 0.1);
        assert_eq!(step_index(0.0, 3), 0);
        assert_eq!(step_index(0.5, 3), 1);
        assert_eq!(step_index(1.0, 3), 2);
        assert_eq!(step_index(7.0, 3), 2);
        assert!(on(0.5) && !on(0.49));
        assert!((db_to_gain(-6.0) - 0.501).abs() < 1e-3);
    }

    #[test]
    fn index_matches_registry_order() {
        let all = ParamId::all();
        assert_eq!(all.len(), PARAM_COUNT);
        for (i, id) in all.iter().enumerate() {
            assert_eq!(id.index(), Some(i), "{id:?}");
        }
        assert_eq!(ParamId::Synth(2, SynthParam::Tune).index(), None);
        assert_eq!(ParamId::Drum(0, 8, DrumParam::Tune).index(), None);
    }
}
