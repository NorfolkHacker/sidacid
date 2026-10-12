//! The FM patch: every FM knob decoded from its 0..1 value into DX7
//! units, plus each operator's frequency.

use super::OPS;
use crate::params::{FmParam, OpParam, ParamId, exp, fm_steps, on, step_index};

/// A 0..1 knob as a DX7 0..99 value.
pub fn d99(v: f32) -> f32 {
    v.clamp(0.0, 1.0) * 99.0
}

/// A 0..1 knob as one of `n` positions.
pub fn pick(v: f32, n: usize) -> usize {
    step_index(v, n as u8)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpPatch {
    pub rates: [f32; 4],
    pub levels: [f32; 4],
    pub break_point: i32,
    pub left_depth: i32,
    pub right_depth: i32,
    pub left_curve: i32,
    pub right_curve: i32,
    pub rate_scaling: i32,
    pub amp_mod_sens: usize,
    pub vel_sens: i32,
    pub level: f32,
    pub fixed: bool,
    pub coarse: i32,
    pub fine: f32,
    pub detune: i32,
    pub on: bool,
}

const BLANK_OP: OpPatch = OpPatch {
    rates: [0.0; 4],
    levels: [0.0; 4],
    break_point: 0,
    left_depth: 0,
    right_depth: 0,
    left_curve: 0,
    right_curve: 0,
    rate_scaling: 0,
    amp_mod_sens: 0,
    vel_sens: 0,
    level: 0.0,
    fixed: false,
    coarse: 0,
    fine: 0.0,
    detune: 0,
    on: false,
};

impl OpPatch {
    pub fn set(&mut self, p: OpParam, v: f32) {
        use OpParam as O;
        match p {
            O::R1 => self.rates[0] = d99(v),
            O::R2 => self.rates[1] = d99(v),
            O::R3 => self.rates[2] = d99(v),
            O::R4 => self.rates[3] = d99(v),
            O::L1 => self.levels[0] = d99(v),
            O::L2 => self.levels[1] = d99(v),
            O::L3 => self.levels[2] = d99(v),
            O::L4 => self.levels[3] = d99(v),
            O::BreakPoint => self.break_point = d99(v).round() as i32,
            O::LeftDepth => self.left_depth = d99(v).round() as i32,
            O::RightDepth => self.right_depth = d99(v).round() as i32,
            O::LeftCurve => self.left_curve = pick(v, fm_steps::CURVES) as i32,
            O::RightCurve => self.right_curve = pick(v, fm_steps::CURVES) as i32,
            O::RateScaling => self.rate_scaling = pick(v, fm_steps::RATE_SCALING) as i32,
            O::AmpModSens => self.amp_mod_sens = pick(v, fm_steps::AMP_MOD_SENS),
            O::VelSens => self.vel_sens = pick(v, fm_steps::VEL_SENS) as i32,
            O::Level => self.level = d99(v),
            O::FixedFreq => self.fixed = on(v),
            O::Coarse => self.coarse = pick(v, fm_steps::COARSE) as i32,
            O::Fine => self.fine = d99(v),
            O::Detune => self.detune = pick(v, fm_steps::DETUNE) as i32 - 7,
            O::On => self.on = on(v),
        }
    }

    /// Frequency in Hz for a note sounding at `note_hz`. Ratio mode:
    /// coarse 0 = ×0.5, fine adds up to +99 %, detune ±7 cents. Fixed
    /// mode: 10^(coarse mod 4 + fine/100) Hz.
    pub fn freq(&self, note_hz: f32) -> f32 {
        if self.fixed {
            10f32.powf(((self.coarse & 3) as f32 * 100.0 + self.fine) / 100.0)
        } else {
            let coarse = if self.coarse == 0 {
                0.5
            } else {
                self.coarse as f32
            };
            note_hz * coarse * (1.0 + self.fine / 100.0) * 2f32.powf(self.detune as f32 / 1200.0)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FmPatch {
    pub ops: [OpPatch; OPS],
    pub algorithm: usize,
    pub feedback: i32,
    pub osc_sync: bool,
    pub pitch_rates: [f32; 4],
    pub pitch_levels: [f32; 4],
    pub lfo_speed: f32,
    pub lfo_delay: f32,
    pub lfo_pmd: f32,
    pub lfo_amd: f32,
    pub lfo_sync: bool,
    pub lfo_wave: usize,
    pub pms: usize,
    /// Semitones, -24..=24.
    pub transpose: i32,
    /// The volume knob, 0..1 (squared when applied).
    pub volume: f32,
    pub mono: bool,
    /// Glide time in seconds.
    pub glide: f32,
    pub vintage: bool,
    pub filter_on: bool,
    pub filter_hp: bool,
    /// Knob 0..1 (mapped to Hz by the filter).
    pub cutoff: f32,
    pub resonance: f32,
    pub chorus_rate: f32,
    pub chorus_depth: f32,
    pub chorus_mix: f32,
    pub phaser_rate: f32,
    pub phaser_depth: f32,
    pub phaser_feedback: f32,
    pub phaser_mix: f32,
}

const BLANK: FmPatch = FmPatch {
    ops: [BLANK_OP; OPS],
    algorithm: 0,
    feedback: 0,
    osc_sync: false,
    pitch_rates: [0.0; 4],
    pitch_levels: [0.0; 4],
    lfo_speed: 0.0,
    lfo_delay: 0.0,
    lfo_pmd: 0.0,
    lfo_amd: 0.0,
    lfo_sync: false,
    lfo_wave: 0,
    pms: 0,
    transpose: 0,
    volume: 0.0,
    mono: false,
    glide: 0.0,
    vintage: false,
    filter_on: false,
    filter_hp: false,
    cutoff: 0.0,
    resonance: 0.0,
    chorus_rate: 0.0,
    chorus_depth: 0.0,
    chorus_mix: 0.0,
    phaser_rate: 0.0,
    phaser_depth: 0.0,
    phaser_feedback: 0.0,
    phaser_mix: 0.0,
};

impl FmPatch {
    pub fn set(&mut self, p: FmParam, v: f32) {
        use FmParam as F;
        match p {
            F::Volume => self.volume = v.clamp(0.0, 1.0),
            F::Algorithm => self.algorithm = pick(v, fm_steps::ALGORITHM),
            F::Feedback => self.feedback = pick(v, fm_steps::FEEDBACK) as i32,
            F::OscSync => self.osc_sync = on(v),
            F::PitchR1 => self.pitch_rates[0] = d99(v),
            F::PitchR2 => self.pitch_rates[1] = d99(v),
            F::PitchR3 => self.pitch_rates[2] = d99(v),
            F::PitchR4 => self.pitch_rates[3] = d99(v),
            F::PitchL1 => self.pitch_levels[0] = d99(v),
            F::PitchL2 => self.pitch_levels[1] = d99(v),
            F::PitchL3 => self.pitch_levels[2] = d99(v),
            F::PitchL4 => self.pitch_levels[3] = d99(v),
            F::LfoSpeed => self.lfo_speed = d99(v),
            F::LfoDelay => self.lfo_delay = d99(v),
            F::LfoPmd => self.lfo_pmd = d99(v),
            F::LfoAmd => self.lfo_amd = d99(v),
            F::LfoSync => self.lfo_sync = on(v),
            F::LfoWave => self.lfo_wave = pick(v, fm_steps::LFO_WAVES),
            F::PitchModSens => self.pms = pick(v, fm_steps::PITCH_MOD_SENS),
            F::Transpose => self.transpose = pick(v, fm_steps::TRANSPOSE) as i32 - 24,
            F::Mono => self.mono = on(v),
            F::Glide => self.glide = v.clamp(0.0, 1.0).powi(2) * 2.0,
            F::Vintage => self.vintage = on(v),
            F::FilterOn => self.filter_on = on(v),
            F::FilterMode => self.filter_hp = on(v),
            F::Cutoff => self.cutoff = v.clamp(0.0, 1.0),
            F::Resonance => self.resonance = v.clamp(0.0, 1.0),
            F::ChorusRate => self.chorus_rate = v.clamp(0.0, 1.0),
            F::ChorusDepth => self.chorus_depth = v.clamp(0.0, 1.0),
            F::ChorusMix => self.chorus_mix = v.clamp(0.0, 1.0),
            F::PhaserRate => self.phaser_rate = v.clamp(0.0, 1.0),
            F::PhaserDepth => self.phaser_depth = v.clamp(0.0, 1.0),
            F::PhaserFeedback => self.phaser_feedback = v.clamp(0.0, 1.0),
            F::PhaserMix => self.phaser_mix = v.clamp(0.0, 1.0),
        }
    }

    /// Ignores `op >= OPS`.
    pub fn set_op(&mut self, op: usize, p: OpParam, v: f32) {
        if let Some(o) = self.ops.get_mut(op) {
            o.set(p, v);
        }
    }

    /// Filter cutoff in Hz.
    pub fn cutoff_hz(&self) -> f32 {
        exp(self.cutoff, 40.0, 18_000.0)
    }
}

impl Default for FmPatch {
    /// Every FM param at its default: the DX7 INIT VOICE.
    fn default() -> Self {
        let mut p = BLANK;
        for id in ParamId::all() {
            match id {
                ParamId::Fm(f) => p.set(f, id.info().default),
                ParamId::FmOp(op, o) => p.set_op(op as usize, o, id.info().default),
                _ => {}
            }
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_patch_is_the_init_voice() {
        let p = FmPatch::default();
        assert_eq!(p.algorithm, 0);
        assert_eq!(p.ops[0].level, 99.0);
        assert_eq!(p.ops[1].level, 0.0);
        assert_eq!(p.ops[3].coarse, 1);
        assert_eq!(p.ops[3].detune, 0);
        assert!(p.ops.iter().all(|o| o.on && !o.fixed));
        assert_eq!(p.ops[2].rates, [99.0; 4]);
        assert_eq!(p.ops[2].levels, [99.0, 99.0, 99.0, 0.0]);
        assert_eq!(p.ops[0].break_point, 39);
        assert!(p.osc_sync && p.lfo_sync && !p.mono && !p.vintage);
        assert_eq!((p.transpose, p.pms, p.lfo_wave), (0, 3, 0));
        assert!((p.pitch_levels[0] - 50.0).abs() < 1e-3);
    }

    #[test]
    fn knobs_decode_to_dx7_units() {
        let mut p = FmPatch::default();
        p.set(FmParam::Algorithm, 1.0);
        p.set(FmParam::Transpose, 0.0);
        p.set(FmParam::Feedback, 1.0);
        assert_eq!((p.algorithm, p.transpose, p.feedback), (31, -24, 7));
        p.set_op(2, OpParam::Coarse, 1.0);
        p.set_op(2, OpParam::Detune, 1.0);
        p.set_op(2, OpParam::LeftCurve, 1.0);
        p.set_op(2, OpParam::On, 0.0);
        let o = p.ops[2];
        assert_eq!((o.coarse, o.detune, o.left_curve, o.on), (31, 7, 3, false));
        p.set(FmParam::Glide, 1.0);
        assert!((p.glide - 2.0).abs() < 1e-6);
        p.set_op(9, OpParam::Level, 1.0); // out of range: ignored
    }

    #[test]
    fn operator_frequencies() {
        let mut o = FmPatch::default().ops[0];
        assert!((o.freq(440.0) - 440.0).abs() < 1e-3);
        o.coarse = 0;
        assert!((o.freq(440.0) - 220.0).abs() < 1e-3);
        o.coarse = 1;
        o.fine = 50.0;
        assert!((o.freq(440.0) - 660.0).abs() < 1e-3);
        o.fine = 0.0;
        o.detune = 7;
        assert!((o.freq(440.0) - 440.0 * 2f32.powf(7.0 / 1200.0)).abs() < 1e-3);
        o.fixed = true;
        o.coarse = 2;
        o.fine = 0.0;
        assert!(
            (o.freq(440.0) - 100.0).abs() < 1e-2,
            "fixed: ignores the note"
        );
        o.coarse = 1;
        o.fine = 50.0;
        assert!((o.freq(55.0) - 10f32.powf(1.5)).abs() < 1e-2);
    }
}
