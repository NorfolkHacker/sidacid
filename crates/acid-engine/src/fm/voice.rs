//! One FM voice: six operators routed by an algorithm, each with a DX7
//! envelope, plus the pitch envelope, LFO delay and glide.

use super::OPS;
use super::algorithms::Algorithm;
use super::env::{Env, operator_out_level, scale_level, scale_rate, scale_velocity};
use super::modulation::{AMS_DEPTH, PMS_SEMITONES, PitchEnv, lfo_delay_gain, lfo_delay_seconds};
use super::patch::FmPatch;
use super::sine::Sine;

/// Output of one voice per full-level carrier, before the machine volume.
const VOICE_GAIN: f32 = 0.25;
/// Envelope level (log2) below which an operator is silent.
const SILENT: f32 = -13.0;
/// Time constant of the steal fade.
const DECLICK_SECONDS: f32 = 0.0005;
/// Phase shift (in cycles) per unit of modulator output. msfa scales a
/// full-level operator by gain 2.0 (`Exp2::lookup(level - (14 << 24))`), so
/// a full modulator moves its target ±2 cycles and feedback is fed that
/// doubled output too. Carriers are heard at gain 1.0.
const MOD_INDEX: f32 = 2.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Voice {
    pub active: bool,
    /// Key held (not released).
    pub held: bool,
    /// Started by audition rather than the pattern.
    pub live: bool,
    pub pitch: u8,
    /// Start order, for stealing.
    pub age: u32,
    /// Current (gliding) MIDI note.
    pub note: f32,
    pub target_note: f32,
    vel: u8,
    phase: [f32; OPS],
    env: [Env; OPS],
    gain: [f32; OPS],
    out_level: [f32; OPS],
    fb: [f32; 2],
    pitch_env: PitchEnv,
    /// Seconds since the note started (for the LFO delay).
    time: f32,
    declick: f32,
    declick_coef: f32,
    last_out: f32,
    sample_rate: f32,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            active: false,
            held: false,
            live: false,
            pitch: 0,
            age: 0,
            note: 0.0,
            target_note: 0.0,
            vel: 0,
            phase: [0.0; OPS],
            env: [Env::new(sample_rate); OPS],
            gain: [0.0; OPS],
            out_level: [0.0; OPS],
            fb: [0.0; 2],
            pitch_env: PitchEnv::new(),
            time: 0.0,
            declick: 0.0,
            declick_coef: (-1.0 / (DECLICK_SECONDS * sample_rate)).exp(),
            last_out: 0.0,
            sample_rate,
        }
    }

    fn key(&self, patch: &FmPatch) -> i32 {
        (self.pitch as i32 + patch.transpose).clamp(0, 127)
    }

    fn op_out_level(&self, patch: &FmPatch, op: usize, key: i32) -> f32 {
        let o = &patch.ops[op];
        operator_out_level(
            o.level,
            scale_level(
                key,
                o.break_point,
                o.left_depth,
                o.right_depth,
                o.left_curve,
                o.right_curve,
            ),
            scale_velocity(self.vel as i32, o.vel_sens),
        )
    }

    /// Starts a note. A voice that is still sounding fades out over ~2 ms
    /// instead of being cut.
    pub fn start(&mut self, patch: &FmPatch, pitch: u8, vel: u8, live: bool, age: u32) {
        self.declick = if self.active { self.last_out } else { 0.0 };
        self.active = true;
        self.held = true;
        self.live = live;
        self.pitch = pitch;
        self.age = age;
        self.vel = vel;
        self.note = pitch as f32;
        self.target_note = self.note;
        self.time = 0.0;
        self.fb = [0.0; 2];
        let key = self.key(patch);
        for op in 0..OPS {
            let o = &patch.ops[op];
            if patch.osc_sync {
                self.phase[op] = 0.0;
            }
            let ol = self.op_out_level(patch, op, key);
            self.out_level[op] = ol;
            self.env[op].start(o.rates, o.levels, ol, scale_rate(key, o.rate_scaling));
            self.gain[op] = 0.0;
        }
        self.pitch_env.start(&patch.pitch_levels);
    }

    /// Mono legato: move to `pitch` without restarting the envelopes.
    pub fn glide_to(&mut self, patch: &FmPatch, pitch: u8) {
        self.pitch = pitch;
        self.target_note = pitch as f32;
        if patch.glide <= 0.0 {
            self.note = self.target_note;
        }
        self.held = true;
    }

    pub fn release(&mut self) {
        if self.held {
            self.held = false;
            for e in &mut self.env {
                e.release();
            }
            self.pitch_env.release();
        }
    }

    /// Silences the voice immediately.
    pub fn kill(&mut self) {
        *self = Voice::new(self.sample_rate);
    }

    /// Renders up to `BLOCK` samples, adding into `out`. `lfo` is the
    /// shared LFO value (-1..1) for this block.
    pub fn render(
        &mut self,
        patch: &FmPatch,
        alg: &Algorithm,
        lfo: f32,
        sine: &Sine,
        out: &mut [f32],
    ) {
        let n = out.len();
        if n == 0 {
            return;
        }
        let dt = n as f32 / self.sample_rate;
        self.time += dt;
        let lfo = lfo * lfo_delay_gain(self.time, lfo_delay_seconds(patch.lfo_delay));
        if patch.glide > 0.0 && self.note != self.target_note {
            self.note += (self.target_note - self.note) * (1.0 - (-dt / patch.glide).exp());
            if (self.target_note - self.note).abs() < 0.001 {
                self.note = self.target_note;
            }
        } else {
            self.note = self.target_note;
        }
        let semis = self
            .pitch_env
            .tick(&patch.pitch_rates, &patch.pitch_levels, dt)
            + lfo * patch.lfo_pmd / 99.0 * PMS_SEMITONES[patch.pms.min(7)];
        let note_hz = 440.0 * 2f32.powf((self.note + patch.transpose as f32 + semis - 69.0) / 12.0);
        let amp_lfo = (lfo + 1.0) * 0.5 * patch.lfo_amd / 99.0;
        let key = self.key(patch);
        let mut inc = [0.0f32; OPS];
        let mut g0 = [0.0f32; OPS];
        let mut dg = [0.0f32; OPS];
        for op in 0..OPS {
            let o = &patch.ops[op];
            let ol = self.op_out_level(patch, op, key);
            if ol != self.out_level[op] {
                self.out_level[op] = ol;
                self.env[op].set_out_level(ol);
            }
            let level = self.env[op].tick(n) - amp_lfo * AMS_DEPTH[o.amp_mod_sens.min(3)];
            let target = if o.on && level > SILENT {
                2f32.powf(level).min(2.0)
            } else {
                0.0
            };
            inc[op] = o.freq(note_hz) / self.sample_rate;
            g0[op] = self.gain[op];
            dg[op] = (target - self.gain[op]) / n as f32;
            self.gain[op] = target;
        }
        let fb_amount = if patch.feedback <= 0 {
            0.0
        } else {
            2f32.powi(patch.feedback - 8)
        };
        for (i, s) in out.iter_mut().enumerate() {
            let mut outs = [0.0f32; OPS];
            let mut sum = 0.0;
            for op in (0..OPS).rev() {
                let mut m = 0.0;
                let mut bits = alg.mods[op];
                while bits != 0 {
                    m += MOD_INDEX * outs[bits.trailing_zeros() as usize];
                    bits &= bits - 1;
                }
                if op == alg.feedback {
                    m += (self.fb[0] + self.fb[1]) * 0.5 * fb_amount;
                }
                let y = sine.at(self.phase[op] + m, patch.vintage) * (g0[op] + dg[op] * i as f32);
                if op == alg.feedback {
                    self.fb[1] = self.fb[0];
                    self.fb[0] = MOD_INDEX * y;
                }
                outs[op] = y;
                self.phase[op] += inc[op];
                self.phase[op] -= self.phase[op].floor();
                if alg.carriers & (1 << op) != 0 {
                    sum += y;
                }
            }
            let v = sum * VOICE_GAIN + self.declick;
            self.declick *= self.declick_coef;
            *s += v;
            self.last_out = v;
        }
        let carriers_done = (0..OPS)
            .filter(|&op| alg.carriers & (1 << op) != 0)
            .all(|op| self.env[op].is_finished() && self.gain[op] == 0.0);
        if !self.held && carriers_done && self.declick.abs() < 1e-5 {
            self.active = false;
            self.last_out = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fm::BLOCK;
    use crate::fm::algorithms::ALGORITHMS;
    use std::f32::consts::TAU;

    const SR: f32 = 48_000.0;

    /// msfa (the DX7 reference) scales operator output by gain 2.0 at full
    /// level, so a full-level modulator shifts its carrier's phase by up to
    /// ±2 cycles. Algorithm 1, OP2 at 99 modulating OP1 at 99, ratio 1, osc
    /// sync: OP1 must read sin(2π(p + 2·sin(2π p))).
    #[test]
    fn a_full_level_modulator_has_index_two_cycles() {
        let mut patch = FmPatch::default();
        patch.ops[1].level = 99.0;
        for o in &mut patch.ops {
            o.vel_sens = 0;
        }
        let alg = ALGORITHMS[0];
        let sine = Sine::new();
        let mut v = Voice::new(SR);
        v.start(&patch, 69, 127, false, 1);
        let mut buf = [0.0f32; BLOCK];
        for _ in 0..(0.05 * SR) as usize / BLOCK {
            buf.fill(0.0);
            v.render(&patch, &alg, 0.0, &sine, &mut buf);
        }
        let inc = patch.ops[0].freq(440.0) / SR;
        let mut worst = 0.0f32;
        for _ in 0..20 {
            let p0 = v.phase[0];
            assert_eq!(v.phase[1], p0, "ratio 1 + osc sync: operators in phase");
            buf.fill(0.0);
            v.render(&patch, &alg, 0.0, &sine, &mut buf);
            for (i, &y) in buf.iter().enumerate() {
                let p = p0 + i as f32 * inc;
                let want = VOICE_GAIN * (TAU * (p + 2.0 * (TAU * p).sin())).sin();
                worst = worst.max((y - want).abs());
            }
        }
        assert!(worst < 2e-3, "max error {worst}");
    }
}
