//! Every automatable control, its name, default (0..1 normalised) and
//! number of discrete positions, plus helpers mapping 0..1 to real units.

use crate::machine::{MachineType, MachineUid};
use serde::{Deserialize, Serialize};

pub const DRUM_INSTRUMENTS: usize = 8;
/// Operators in the FM synth.
pub const FM_OPS: usize = 6;
/// Sample slots in the sampler.
pub const SAMPLER_SLOTS: usize = 16;

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

/// Positions of the Mono synth's switches.
pub mod mono_steps {
    pub const RANGE: usize = 4;
    pub const PWM_SOURCE: usize = 3;
    pub const SUB_WAVE: usize = 3;
    pub const TRIGGER: usize = 3;
    pub const VCA_MODE: usize = 2;
    pub const LFO_WAVES: usize = 4;
    pub const PORTAMENTO: usize = 3;
    pub const ARP_MODE: usize = 3;
    pub const ARP_RANGE: usize = 3;
    pub const ARP_RATE: usize = 6;
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
param_enum!(MonoParam {
    Range => "Range", Tune => "Tune", VcoMod => "VCO Mod", PulseWidth => "Pulse Width",
    PwmSource => "PWM Source", PulseLevel => "Pulse", SawLevel => "Saw", SubLevel => "Sub",
    SubWave => "Sub Wave", NoiseLevel => "Noise", Cutoff => "Cutoff", Resonance => "Resonance",
    EnvAmount => "Env", VcfMod => "VCF Mod", KeyFollow => "Key Follow", VelToVcf => "Vel > VCF",
    Attack => "Attack", Decay => "Decay", Sustain => "Sustain", Release => "Release",
    Trigger => "Trigger", VcaMode => "VCA Mode", VelToVca => "Vel > VCA",
    LfoWave => "LFO Wave", LfoRate => "LFO Rate", Portamento => "Portamento",
    PortaTime => "Porta Time", ArpOn => "Arp", ArpMode => "Arp Mode", ArpRange => "Arp Range",
    ArpRate => "Arp Rate", Drift => "Drift", ChorusRate => "Chorus Rate",
    ChorusDepth => "Chorus Depth", ChorusMix => "Chorus Mix", Volume => "Volume",
});

impl MonoParam {
    pub fn info(self) -> ParamInfo {
        use MonoParam as M;
        use mono_steps as s;
        let n = self.name();
        // A switch resting on position `at` of `steps`.
        let switch = |steps: usize, at: usize| disc(n, at as f32 / (steps - 1) as f32, steps as u8);
        match self {
            M::Range => switch(s::RANGE, 1),
            M::PwmSource => switch(s::PWM_SOURCE, 0),
            M::SubWave => switch(s::SUB_WAVE, 0),
            M::Trigger => switch(s::TRIGGER, 0),
            M::VcaMode => switch(s::VCA_MODE, 0),
            M::LfoWave => switch(s::LFO_WAVES, 0),
            M::Portamento => switch(s::PORTAMENTO, 0),
            M::ArpOn => switch(2, 0),
            M::ArpMode => switch(s::ARP_MODE, 0),
            M::ArpRange => switch(s::ARP_RANGE, 0),
            M::ArpRate => switch(s::ARP_RATE, 3),
            M::Tune | M::KeyFollow | M::Cutoff | M::ChorusDepth => cont(n, 0.5),
            M::SawLevel => cont(n, 0.8),
            M::Resonance => cont(n, 0.2),
            M::EnvAmount | M::Decay | M::LfoRate => cont(n, 0.4),
            M::Sustain => cont(n, 0.6),
            M::Release => cont(n, 0.2),
            M::PortaTime | M::ChorusRate => cont(n, 0.3),
            M::Volume => cont(n, 0.7),
            M::VcoMod
            | M::PulseWidth
            | M::PulseLevel
            | M::SubLevel
            | M::NoiseLevel
            | M::VcfMod
            | M::VelToVcf
            | M::Attack
            | M::VelToVca
            | M::Drift
            | M::ChorusMix => cont(n, 0.0),
        }
    }
}

/// A control on one machine. Which variants a machine accepts depends on
/// its type (`fits`); every machine has the channel-strip params.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MachineParam {
    Synth(SynthParam),
    Drum(u8, DrumParam),
    Kit(KitParam),
    Channel(ChannelParam),
    Fm(FmParam),
    FmOp(u8, OpParam),
    Sampler(SamplerParam),
    SamplerSlot(u8, SlotParam),
    Mono(MonoParam),
}

/// A control shared by the whole rack: send effects and master bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GlobalParam {
    Fx(FxParam),
    Master(MasterParam),
}

/// Every automatable control: a machine's (by uid) or a global one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ParamId {
    Machine(MachineUid, MachineParam),
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

const fn is_kit(ty: MachineType) -> bool {
    matches!(ty, MachineType::Kit808 | MachineType::Kit909)
}

impl MachineParam {
    pub fn info(self) -> ParamInfo {
        use ChannelParam as C;
        use SynthParam as S;
        match self {
            MachineParam::Synth(p) => {
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
            MachineParam::Drum(_, p) => {
                cont(p.name(), if p == DrumParam::Level { 0.8 } else { 0.5 })
            }
            MachineParam::Kit(p) => cont(p.name(), if p == KitParam::Volume { 0.7 } else { 0.5 }),
            MachineParam::Channel(p) => {
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
            MachineParam::Fm(p) => {
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
                    F::PitchModSens => cont(n, 3.0 / (fm_steps::PITCH_MOD_SENS - 1) as f32),
                    F::Transpose => cont(n, 0.5),
                    F::Mono | F::Vintage | F::FilterOn | F::FilterMode => disc(n, 0.0, 2),
                    F::Glide | F::Resonance | F::ChorusMix | F::PhaserMix => cont(n, 0.0),
                    F::Cutoff => cont(n, 1.0),
                    F::ChorusRate | F::PhaserRate | F::PhaserFeedback => cont(n, 0.3),
                    F::ChorusDepth | F::PhaserDepth => cont(n, 0.5),
                }
            }
            MachineParam::FmOp(op, p) => {
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
                    O::Coarse => cont(n, 1.0 / (fm_steps::COARSE - 1) as f32),
                    O::Detune => cont(n, 0.5),
                    O::On => disc(n, 1.0, 2),
                }
            }
            MachineParam::Sampler(p) => match p {
                SamplerParam::Volume => cont(p.name(), 0.7),
                SamplerParam::Grit => disc(p.name(), 0.0, 2),
            },
            MachineParam::SamplerSlot(_, p) => {
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
            MachineParam::Mono(p) => p.info(),
        }
    }

    /// Valid on a machine of `ty`: channel params on every machine, the
    /// type's own params on that type, with in-range indices.
    pub fn fits(self, ty: MachineType) -> bool {
        match self {
            MachineParam::Channel(_) => true,
            MachineParam::Synth(_) => ty == MachineType::Synth,
            MachineParam::Drum(i, _) => is_kit(ty) && (i as usize) < DRUM_INSTRUMENTS,
            MachineParam::Kit(_) => is_kit(ty),
            MachineParam::Fm(_) => ty == MachineType::Fm,
            MachineParam::FmOp(op, _) => ty == MachineType::Fm && (op as usize) < FM_OPS,
            MachineParam::Sampler(_) => ty == MachineType::Sampler,
            MachineParam::SamplerSlot(s, _) => {
                ty == MachineType::Sampler && (s as usize) < SAMPLER_SLOTS
            }
            MachineParam::Mono(_) => ty == MachineType::Mono,
        }
    }

    /// Position in `MachineParam::all_for(ty)`; `None` if `!fits(ty)`.
    pub fn index_for(self, ty: MachineType) -> Option<usize> {
        if !self.fits(ty) {
            return None;
        }
        let base = ChannelParam::ALL.len();
        let drum_n = DrumParam::ALL.len();
        Some(match self {
            MachineParam::Channel(p) => p as usize,
            MachineParam::Synth(p) => base + p as usize,
            MachineParam::Drum(i, p) => base + i as usize * drum_n + p as usize,
            MachineParam::Kit(p) => base + DRUM_INSTRUMENTS * drum_n + p as usize,
            MachineParam::Fm(p) => base + p as usize,
            MachineParam::FmOp(op, p) => {
                base + FmParam::ALL.len() + op as usize * OpParam::ALL.len() + p as usize
            }
            MachineParam::Sampler(p) => base + p as usize,
            MachineParam::SamplerSlot(s, p) => {
                base + SamplerParam::ALL.len() + s as usize * SlotParam::ALL.len() + p as usize
            }
            MachineParam::Mono(p) => base + p as usize,
        })
    }

    /// Every param of a machine of `ty`: channel params first, then the
    /// type's own in declaration order.
    pub fn all_for(ty: MachineType) -> Vec<MachineParam> {
        let mut v: Vec<MachineParam> = ChannelParam::ALL
            .iter()
            .map(|&p| MachineParam::Channel(p))
            .collect();
        match ty {
            MachineType::Synth => v.extend(SynthParam::ALL.iter().map(|&p| MachineParam::Synth(p))),
            MachineType::Kit808 | MachineType::Kit909 => {
                for i in 0..DRUM_INSTRUMENTS as u8 {
                    v.extend(DrumParam::ALL.iter().map(|&p| MachineParam::Drum(i, p)));
                }
                v.extend(KitParam::ALL.iter().map(|&p| MachineParam::Kit(p)));
            }
            MachineType::Fm => {
                v.extend(FmParam::ALL.iter().map(|&p| MachineParam::Fm(p)));
                for op in 0..FM_OPS as u8 {
                    v.extend(OpParam::ALL.iter().map(|&p| MachineParam::FmOp(op, p)));
                }
            }
            MachineType::Sampler => {
                v.extend(SamplerParam::ALL.iter().map(|&p| MachineParam::Sampler(p)));
                for s in 0..SAMPLER_SLOTS as u8 {
                    v.extend(
                        SlotParam::ALL
                            .iter()
                            .map(|&p| MachineParam::SamplerSlot(s, p)),
                    );
                }
            }
            MachineType::Mono => v.extend(MonoParam::ALL.iter().map(|&p| MachineParam::Mono(p))),
        }
        v
    }

    pub fn count_for(ty: MachineType) -> usize {
        ChannelParam::ALL.len()
            + match ty {
                MachineType::Synth => SynthParam::ALL.len(),
                MachineType::Kit808 | MachineType::Kit909 => {
                    DRUM_INSTRUMENTS * DrumParam::ALL.len() + KitParam::ALL.len()
                }
                MachineType::Fm => FmParam::ALL.len() + FM_OPS * OpParam::ALL.len(),
                MachineType::Sampler => {
                    SamplerParam::ALL.len() + SAMPLER_SLOTS * SlotParam::ALL.len()
                }
                MachineType::Mono => MonoParam::ALL.len(),
            }
    }
}

impl GlobalParam {
    pub const COUNT: usize = FxParam::ALL.len() + MasterParam::ALL.len();

    pub fn info(self) -> ParamInfo {
        use FxParam as F;
        use MasterParam as M;
        match self {
            GlobalParam::Fx(p) => {
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
            GlobalParam::Master(p) => {
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

    /// Position in `GlobalParam::all()`.
    pub fn index(self) -> usize {
        match self {
            GlobalParam::Fx(p) => p as usize,
            GlobalParam::Master(p) => FxParam::ALL.len() + p as usize,
        }
    }

    /// Fx params then Master params.
    pub fn all() -> Vec<GlobalParam> {
        FxParam::ALL
            .iter()
            .map(|&p| GlobalParam::Fx(p))
            .chain(MasterParam::ALL.iter().map(|&p| GlobalParam::Master(p)))
            .collect()
    }
}

impl ParamId {
    pub fn info(self) -> ParamInfo {
        match self {
            ParamId::Machine(_, p) => p.info(),
            ParamId::Fx(p) => GlobalParam::Fx(p).info(),
            ParamId::Master(p) => GlobalParam::Master(p).info(),
        }
    }

    pub fn global(self) -> Option<GlobalParam> {
        match self {
            ParamId::Machine(..) => None,
            ParamId::Fx(p) => Some(GlobalParam::Fx(p)),
            ParamId::Master(p) => Some(GlobalParam::Master(p)),
        }
    }
}

impl From<GlobalParam> for ParamId {
    fn from(g: GlobalParam) -> Self {
        match g {
            GlobalParam::Fx(p) => ParamId::Fx(p),
            GlobalParam::Master(p) => ParamId::Master(p),
        }
    }
}

impl MachineUid {
    /// Control `p` on this machine.
    pub fn param(self, p: impl Into<MachineParam>) -> ParamId {
        ParamId::Machine(self, p.into())
    }
}

macro_rules! machine_param_from {
    ($($t:ident => $v:ident),+ $(,)?) => {
        $(impl From<$t> for MachineParam {
            fn from(p: $t) -> Self {
                MachineParam::$v(p)
            }
        })+
    };
}

machine_param_from!(
    SynthParam => Synth,
    KitParam => Kit,
    ChannelParam => Channel,
    FmParam => Fm,
    SamplerParam => Sampler,
    MonoParam => Mono,
);

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
    use crate::machine::MachineType;
    use std::collections::HashSet;

    const SYNTH: MachineUid = MachineUid(1);
    const FM: MachineUid = MachineUid(5);
    const SAMPLER: MachineUid = MachineUid(6);
    fn fm_op(op: u8, p: OpParam) -> ParamId {
        FM.param(MachineParam::FmOp(op, p))
    }

    #[test]
    fn per_type_index_spaces_match_all_for() {
        for ty in MachineType::ALL {
            let all = MachineParam::all_for(ty);
            assert_eq!(all.len(), MachineParam::count_for(ty));
            for (i, p) in all.iter().enumerate() {
                assert!(p.fits(ty));
                assert_eq!(p.index_for(ty), Some(i), "{ty:?} {p:?}");
            }
        }
        assert_eq!(
            MachineParam::Fm(FmParam::Volume).index_for(MachineType::Synth),
            None
        );
        assert_eq!(
            MachineParam::Drum(8, DrumParam::Tune).index_for(MachineType::Kit808),
            None
        );
        assert!(MachineParam::Channel(ChannelParam::Level).fits(MachineType::Sampler));
    }

    #[test]
    fn global_params_index_densely() {
        for (i, g) in GlobalParam::all().into_iter().enumerate() {
            assert_eq!(g.index(), i);
        }
        assert_eq!(GlobalParam::all().len(), GlobalParam::COUNT);
    }

    #[test]
    fn per_type_counts_cover_the_old_flat_table() {
        // Old PARAM_COUNT = 589: 2 synths + 2 kits + 6 channels + fx + master + fm + ops + slots.
        let ch = ChannelParam::ALL.len();
        let c = |t| MachineParam::count_for(t) - ch;
        let total = 2 * c(MachineType::Synth)
            + c(MachineType::Kit808)
            + c(MachineType::Kit909)
            + c(MachineType::Fm)
            + c(MachineType::Sampler)
            + 6 * ch
            + GlobalParam::COUNT;
        assert_eq!(total, 589);
    }

    #[test]
    fn every_machine_type_lists_each_param_once() {
        for ty in MachineType::ALL {
            let all = MachineParam::all_for(ty);
            let unique: HashSet<_> = all.iter().collect();
            assert_eq!(unique.len(), all.len(), "{ty:?}");
        }
        let fm = MachineParam::all_for(MachineType::Fm);
        assert_eq!(
            fm[ChannelParam::ALL.len()],
            MachineParam::Fm(FmParam::Volume)
        );
        assert!(fm.contains(&MachineParam::FmOp(5, OpParam::On)));
        let sampler = MachineParam::all_for(MachineType::Sampler);
        assert_eq!(
            sampler[sampler.len() - 1],
            MachineParam::SamplerSlot(15, SlotParam::TempoLock)
        );
        assert!(sampler.contains(&MachineParam::Sampler(SamplerParam::Volume)));
        assert_eq!(FM.param(FmParam::Volume).info().default, 0.7);
    }

    #[test]
    fn sampler_params_and_maps() {
        let sampler = MachineType::Sampler;
        assert!(MachineParam::SamplerSlot(15, SlotParam::Level).fits(sampler));
        assert!(!MachineParam::SamplerSlot(16, SlotParam::Level).fits(sampler));
        assert_eq!(
            MachineParam::SamplerSlot(16, SlotParam::Level).index_for(sampler),
            None
        );
        assert_eq!(SAMPLER.param(SamplerParam::Grit).info().steps, 2);
        assert_eq!(
            SAMPLER
                .param(MachineParam::SamplerSlot(3, SlotParam::Choke))
                .info()
                .steps,
            2
        );
        assert_eq!(
            SAMPLER
                .param(MachineParam::SamplerSlot(3, SlotParam::Release))
                .info()
                .default,
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
        assert_eq!(sampler_map::level_gain(1.0), 1.25);
        assert!((sampler_map::decay_seconds(0.0) - 0.005).abs() < 1e-6);
        assert!((sampler_map::decay_seconds(1.0) - 4.0).abs() < 1e-3);
        // out-of-range knob values clamp to the ends
        assert_eq!(sampler_map::level_gain(2.0), 1.25);
        assert_eq!(sampler_map::level_gain(-1.0), 0.0);
        assert_eq!(sampler_map::tune_semitones(5.0), 24);
        assert_eq!(sampler_map::tune_semitones(-5.0), -24);
        assert_eq!(sampler_map::fine_cents(-1.0), -50.0);
        assert!((sampler_map::cutoff_hz(-1.0) - 40.0).abs() < 1e-3);
        assert!((sampler_map::cutoff_hz(2.0) - 18_000.0).abs() < 1.0);
    }

    #[test]
    fn fm_defaults_are_the_dx7_init_voice() {
        let d = |id: ParamId| id.info().default;
        assert_eq!(d(fm_op(0, OpParam::Level)), 1.0);
        assert_eq!(d(fm_op(1, OpParam::Level)), 0.0);
        assert_eq!(d(fm_op(3, OpParam::R1)), 1.0);
        assert_eq!(d(fm_op(3, OpParam::L4)), 0.0);
        assert!((d(fm_op(2, OpParam::Coarse)) - 1.0 / 31.0).abs() < 1e-6);
        // Decoded through the step counts, the defaults are the DX7 INIT
        // values: changing an fm_steps constant must change its default too.
        let step = |id: ParamId, steps: usize| step_index(d(id), steps as u8);
        assert_eq!(step(fm_op(2, OpParam::Coarse), fm_steps::COARSE), 1);
        assert_eq!(step(fm_op(2, OpParam::Detune), fm_steps::DETUNE), 7);
        assert_eq!(
            step(FM.param(FmParam::PitchModSens), fm_steps::PITCH_MOD_SENS),
            3
        );
        assert_eq!(step(FM.param(FmParam::Transpose), fm_steps::TRANSPOSE), 24);
        assert_eq!(d(fm_op(2, OpParam::Detune)), 0.5);
        assert_eq!(d(fm_op(5, OpParam::On)), 1.0);
        assert!((d(FM.param(FmParam::PitchL1)) - 50.0 / 99.0).abs() < 1e-6);
        assert_eq!(d(FM.param(FmParam::Transpose)), 0.5);
        assert_eq!(d(FM.param(FmParam::Algorithm)), 0.0);
        assert_eq!(FM.param(FmParam::LfoWave).info().steps, 6);
        assert_eq!(fm_op(0, OpParam::LeftCurve).info().steps, 4);
        assert_eq!(
            FM.param(FmParam::Algorithm).info().steps,
            0,
            "a rounded knob"
        );
    }

    #[test]
    fn fm_operator_ids_are_bounded() {
        assert!(MachineParam::FmOp(5, OpParam::Fine).fits(MachineType::Fm));
        assert!(!MachineParam::FmOp(6, OpParam::Fine).fits(MachineType::Fm));
        assert_eq!(
            MachineParam::FmOp(6, OpParam::Fine).index_for(MachineType::Fm),
            None
        );
    }

    #[test]
    fn defaults_are_normalised_and_names_nonempty() {
        let infos = MachineType::ALL
            .into_iter()
            .flat_map(MachineParam::all_for)
            .map(MachineParam::info)
            .chain(GlobalParam::all().into_iter().map(GlobalParam::info));
        for info in infos {
            assert!((0.0..=1.0).contains(&info.default), "{info:?}");
            assert!(!info.name.is_empty());
        }
    }

    #[test]
    fn params_fit_only_their_machine_types() {
        use MachineType as T;
        assert!(!MachineParam::Synth(SynthParam::Tune).fits(T::Kit808));
        assert!(!MachineParam::Drum(8, DrumParam::Tune).fits(T::Kit909));
        assert!(MachineParam::Drum(7, DrumParam::Tune).fits(T::Kit909));
        assert!(!MachineParam::Kit(KitParam::Volume).fits(T::Synth));
        assert!(!MachineParam::Sampler(SamplerParam::Grit).fits(T::Fm));
        assert!(!MachineParam::FmOp(0, OpParam::On).fits(T::Sampler));
        for ty in T::ALL {
            assert!(MachineParam::Channel(ChannelParam::Level).fits(ty));
        }
    }

    #[test]
    fn discrete_params_report_steps() {
        assert_eq!(SYNTH.param(SynthParam::Wave).info().steps, 2);
        assert_eq!(SYNTH.param(SynthParam::FilterMode).info().steps, 3);
        assert_eq!(ParamId::Fx(FxParam::DelayDivision).info().steps, 6);
        assert_eq!(SYNTH.param(SynthParam::Cutoff).info().steps, 0);
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
    fn mono_params_fit_only_mono_machines() {
        use crate::machine::MachineType as T;
        let p = MachineParam::Mono(MonoParam::Cutoff);
        assert!(p.fits(T::Mono));
        assert!(!p.fits(T::Fm));
        assert!(!MachineParam::Fm(FmParam::Volume).fits(T::Mono));
        let all = MachineParam::all_for(T::Mono);
        assert_eq!(all.len(), MachineParam::count_for(T::Mono));
        for (i, q) in all.iter().enumerate() {
            assert_eq!(q.index_for(T::Mono), Some(i), "{q:?}");
        }
        assert_eq!(p.info(), MonoParam::Cutoff.info());
    }
}
