//! Every automatable control, its name, default (0..1 normalised) and
//! number of discrete positions, plus helpers mapping 0..1 to real units.

use serde::{Deserialize, Serialize};

pub const SYNTHS: usize = 2;
pub const KITS: usize = 2;
pub const CHANNELS: usize = 6;
pub const DRUM_INSTRUMENTS: usize = 8;
/// Operators in the FM synth.
pub const FM_OPS: usize = 6;
/// Number of entries in `ParamId::all()`.
/// Sample slots in the sampler.
pub const SAMPLER_SLOTS: usize = 16;

pub const PARAM_COUNT: usize = 589;

/// Positions of the FM controls that pick from a list. The numeric ones
/// are continuous knobs the synth rounds with `step_index(v, N)`.
pub mod fm_steps {
    pub const ALGORITHM: usize = 32;
    pub const FEEDBACK: usize = 8;
    pub const COARSE: usize = 32;
    pub const DETUNE: usize = 15;
    pub const RATE_SCALING: usize = 8;
    pub const AMP_MOD_SENS: usize = 4;
    pub const VEL_SENS: usize = 8;
    pub const PITCH_MOD_SENS: usize = 8;
    pub const TRANSPOSE: usize = 49;
    pub const CURVES: usize = 4;
    pub const LFO_WAVES: usize = 6;
}

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
param_enum!(FmParam {
    Volume => "Volume", Algorithm => "Algorithm", Feedback => "Feedback", OscSync => "Osc Sync",
    PitchR1 => "Pitch R1", PitchR2 => "Pitch R2", PitchR3 => "Pitch R3", PitchR4 => "Pitch R4",
    PitchL1 => "Pitch L1", PitchL2 => "Pitch L2", PitchL3 => "Pitch L3", PitchL4 => "Pitch L4",
    LfoSpeed => "LFO Speed", LfoDelay => "LFO Delay", LfoPmd => "LFO Pitch Depth",
    LfoAmd => "LFO Amp Depth", LfoSync => "LFO Sync", LfoWave => "LFO Wave",
    PitchModSens => "Pitch Mod Sens", Transpose => "Transpose",
    Mono => "Mono", Glide => "Glide", Vintage => "Vintage",
    FilterOn => "Filter", FilterMode => "Filter Mode", Cutoff => "Cutoff", Resonance => "Resonance",
    ChorusRate => "Chorus Rate", ChorusDepth => "Chorus Depth", ChorusMix => "Chorus Mix",
    PhaserRate => "Phaser Rate", PhaserDepth => "Phaser Depth", PhaserFeedback => "Phaser Feedback",
    PhaserMix => "Phaser Mix",
});
param_enum!(OpParam {
    R1 => "Rate 1", R2 => "Rate 2", R3 => "Rate 3", R4 => "Rate 4",
    L1 => "Level 1", L2 => "Level 2", L3 => "Level 3", L4 => "Level 4",
    BreakPoint => "Break Point", LeftDepth => "Left Depth", RightDepth => "Right Depth",
    LeftCurve => "Left Curve", RightCurve => "Right Curve", RateScaling => "Rate Scaling",
    AmpModSens => "Amp Mod Sens", VelSens => "Velocity", Level => "Level",
    FixedFreq => "Fixed Freq", Coarse => "Coarse", Fine => "Fine", Detune => "Detune", On => "On",
});
param_enum!(SamplerParam { Volume => "Volume", Grit => "Grit" });
param_enum!(SlotParam {
    Level => "Level", Pan => "Pan", Tune => "Tune", Fine => "Fine", Cutoff => "Cutoff",
    Resonance => "Resonance", Attack => "Attack", Decay => "Decay", Sustain => "Sustain",
    Release => "Release", Reverse => "Reverse", Choke => "Choke", TempoLock => "Tempo Lock",
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
    Fm(FmParam),
    Sampler(SamplerParam),
    FmOp(u8, OpParam),
    SamplerSlot(u8, SlotParam),
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
            ParamId::Fm(p) => {
                use FmParam as F;
                let n = p.name();
                let of99 = |x: f32| x / 99.0;
                match p {
                    F::Volume => cont(n, 0.7),
                    F::Algorithm | F::Feedback | F::LfoDelay | F::LfoPmd | F::LfoAmd => {
                        cont(n, 0.0)
                    }
                    F::OscSync | F::LfoSync => disc(n, 1.0, 2),
                    F::PitchR1 | F::PitchR2 | F::PitchR3 | F::PitchR4 => cont(n, 1.0),
                    F::PitchL1 | F::PitchL2 | F::PitchL3 | F::PitchL4 => cont(n, of99(50.0)),
                    F::LfoSpeed => cont(n, of99(35.0)),
                    F::LfoWave => disc(n, 0.0, fm_steps::LFO_WAVES as u8),
                    F::PitchModSens => cont(n, 3.0 / 7.0),
                    F::Transpose => cont(n, 0.5),
                    F::Mono | F::Vintage | F::FilterOn | F::FilterMode => disc(n, 0.0, 2),
                    F::Glide | F::Resonance | F::ChorusMix | F::PhaserMix => cont(n, 0.0),
                    F::Cutoff => cont(n, 1.0),
                    F::ChorusRate | F::PhaserRate | F::PhaserFeedback => cont(n, 0.3),
                    F::ChorusDepth | F::PhaserDepth => cont(n, 0.5),
                }
            }
            ParamId::FmOp(op, p) => {
                use OpParam as O;
                let n = p.name();
                match p {
                    O::R1 | O::R2 | O::R3 | O::R4 | O::L1 | O::L2 | O::L3 => cont(n, 1.0),
                    O::L4 | O::LeftDepth | O::RightDepth | O::Fine => cont(n, 0.0),
                    O::RateScaling | O::AmpModSens | O::VelSens => cont(n, 0.0),
                    O::BreakPoint => cont(n, 39.0 / 99.0),
                    O::LeftCurve | O::RightCurve => disc(n, 0.0, fm_steps::CURVES as u8),
                    O::Level => cont(n, if op == 0 { 1.0 } else { 0.0 }),
                    O::FixedFreq => disc(n, 0.0, 2),
                    O::Coarse => cont(n, 1.0 / 31.0),
                    O::Detune => cont(n, 0.5),
                    O::On => disc(n, 1.0, 2),
                }
            }
            ParamId::Sampler(p) => match p {
                SamplerParam::Volume => cont(p.name(), 0.7),
                SamplerParam::Grit => disc(p.name(), 0.0, 2),
            },
            ParamId::SamplerSlot(_, p) => {
                use SlotParam as S;
                let n = p.name();
                match p {
                    S::Level => cont(n, 0.8),
                    S::Pan | S::Tune | S::Fine | S::Decay => cont(n, 0.5),
                    S::Cutoff | S::Sustain => cont(n, 1.0),
                    S::Resonance | S::Attack => cont(n, 0.0),
                    S::Release => cont(n, 0.3),
                    S::Reverse | S::Choke | S::TempoLock => disc(n, 0.0, 2),
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
            ParamId::FmOp(op, _) => (op as usize) < FM_OPS,
            ParamId::SamplerSlot(s, _) => (s as usize) < SAMPLER_SLOTS,
            ParamId::Fm(_) | ParamId::Sampler(_) | ParamId::Fx(_) | ParamId::Master(_) => true,
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
        let fm_base = master_base + MasterParam::ALL.len();
        let sampler_base = fm_base + FmParam::ALL.len();
        let fm_op_base = sampler_base + SamplerParam::ALL.len();
        let slot_base = fm_op_base + FM_OPS * OpParam::ALL.len();
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
            ParamId::Fm(p) => fm_base + p as usize,
            ParamId::Sampler(p) => sampler_base + p as usize,
            ParamId::FmOp(op, p) => fm_op_base + op as usize * OpParam::ALL.len() + p as usize,
            ParamId::SamplerSlot(s, p) => {
                slot_base + s as usize * SlotParam::ALL.len() + p as usize
            }
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
        v.extend(FmParam::ALL.iter().map(|&p| ParamId::Fm(p)));
        v.extend(SamplerParam::ALL.iter().map(|&p| ParamId::Sampler(p)));
        for op in 0..FM_OPS as u8 {
            v.extend(OpParam::ALL.iter().map(|&p| ParamId::FmOp(op, p)));
        }
        for s in 0..SAMPLER_SLOTS as u8 {
            v.extend(SlotParam::ALL.iter().map(|&p| ParamId::SamplerSlot(s, p)));
        }
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

/// Sampler slot knobs in real units. The engine and the UI share these.
pub mod sampler_map {
    use super::{exp, step_index};

    pub const TUNE_STEPS: usize = 49;

    pub fn level_gain(v: f32) -> f32 {
        v.clamp(0.0, 1.0).powi(2) * 1.25
    }

    pub fn tune_semitones(v: f32) -> i32 {
        step_index(v, TUNE_STEPS as u8) as i32 - 24
    }

    pub fn fine_cents(v: f32) -> f32 {
        (v.clamp(0.0, 1.0) * 100.0 - 50.0).round()
    }

    pub fn cutoff_hz(v: f32) -> f32 {
        exp(v, 40.0, 18_000.0)
    }

    pub fn attack_seconds(v: f32) -> f32 {
        exp(v, 0.001, 2.0)
    }

    pub fn decay_seconds(v: f32) -> f32 {
        exp(v, 0.005, 4.0)
    }

    pub fn release_seconds(v: f32) -> f32 {
        exp(v, 0.005, 4.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_has_every_param_once() {
        let all = ParamId::all();
        assert_eq!(
            all.len(),
            2 * 15 + 2 * 8 * 4 + 2 * 2 + 6 * 16 + 11 + 8 + 34 + 2 + 6 * 22 + 16 * 13
        );
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
    }

    #[test]
    fn fm_params_follow_the_master_params() {
        let all = ParamId::all();
        let first_fm = all
            .iter()
            .position(|id| matches!(id, ParamId::Fm(_)))
            .unwrap();
        assert_eq!(all[first_fm], ParamId::Fm(FmParam::Volume));
        assert!(matches!(all[first_fm - 1], ParamId::Master(_)));
        assert!(all.contains(&ParamId::FmOp(5, OpParam::On)));
        assert_eq!(
            all[all.len() - 1],
            ParamId::SamplerSlot(15, SlotParam::TempoLock)
        );
        assert!(all.contains(&ParamId::Sampler(SamplerParam::Volume)));
        assert_eq!(ParamId::Fm(FmParam::Volume).info().default, 0.7);
    }

    #[test]
    fn sampler_params_and_maps() {
        assert!(ParamId::SamplerSlot(15, SlotParam::Level).is_valid());
        assert!(!ParamId::SamplerSlot(16, SlotParam::Level).is_valid());
        assert_eq!(ParamId::SamplerSlot(16, SlotParam::Level).index(), None);
        assert_eq!(ParamId::Sampler(SamplerParam::Grit).info().steps, 2);
        assert_eq!(ParamId::SamplerSlot(3, SlotParam::Choke).info().steps, 2);
        assert_eq!(
            ParamId::SamplerSlot(3, SlotParam::Release).info().default,
            0.3
        );
        assert_eq!(sampler_map::tune_semitones(0.5), 0);
        assert_eq!(sampler_map::tune_semitones(1.0), 24);
        assert_eq!(sampler_map::tune_semitones(0.0), -24);
        assert_eq!(sampler_map::fine_cents(0.5), 0.0);
        assert_eq!(sampler_map::fine_cents(1.0), 50.0);
        assert!((sampler_map::level_gain(0.8) - 0.8).abs() < 1e-6);
        assert!((sampler_map::attack_seconds(0.0) - 0.001).abs() < 1e-6);
        assert!((sampler_map::release_seconds(1.0) - 4.0).abs() < 1e-3);
        assert!((sampler_map::cutoff_hz(1.0) - 18_000.0).abs() < 1.0);
    }

    #[test]
    fn fm_defaults_are_the_dx7_init_voice() {
        let d = |id: ParamId| id.info().default;
        assert_eq!(d(ParamId::FmOp(0, OpParam::Level)), 1.0);
        assert_eq!(d(ParamId::FmOp(1, OpParam::Level)), 0.0);
        assert_eq!(d(ParamId::FmOp(3, OpParam::R1)), 1.0);
        assert_eq!(d(ParamId::FmOp(3, OpParam::L4)), 0.0);
        assert!((d(ParamId::FmOp(2, OpParam::Coarse)) - 1.0 / 31.0).abs() < 1e-6);
        assert_eq!(d(ParamId::FmOp(2, OpParam::Detune)), 0.5);
        assert_eq!(d(ParamId::FmOp(5, OpParam::On)), 1.0);
        assert!((d(ParamId::Fm(FmParam::PitchL1)) - 50.0 / 99.0).abs() < 1e-6);
        assert_eq!(d(ParamId::Fm(FmParam::Transpose)), 0.5);
        assert_eq!(d(ParamId::Fm(FmParam::Algorithm)), 0.0);
        assert_eq!(ParamId::Fm(FmParam::LfoWave).info().steps, 6);
        assert_eq!(ParamId::FmOp(0, OpParam::LeftCurve).info().steps, 4);
        assert_eq!(
            ParamId::Fm(FmParam::Algorithm).info().steps,
            0,
            "a rounded knob"
        );
    }

    #[test]
    fn fm_operator_ids_are_bounded() {
        assert!(ParamId::FmOp(5, OpParam::Fine).is_valid());
        assert!(!ParamId::FmOp(6, OpParam::Fine).is_valid());
        assert_eq!(ParamId::FmOp(6, OpParam::Fine).index(), None);
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
        assert!(!ParamId::Channel(6, ChannelParam::Level).is_valid());
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
