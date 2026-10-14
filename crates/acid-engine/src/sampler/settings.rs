//! A slot's knobs in real units.

use crate::params::{MachineParam, SlotParam, on, sampler_map};
use std::f32::consts::{FRAC_PI_2, SQRT_2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotSettings {
    pub level_gain: f32,
    pub pan_l: f32,
    pub pan_r: f32,
    pub tune: i32,
    pub fine_cents: f32,
    pub cutoff_knob: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,
    pub reverse: bool,
    pub choke: bool,
    pub tempo_lock: bool,
}

impl SlotSettings {
    pub fn set(&mut self, p: SlotParam, v: f32) {
        let v = v.clamp(0.0, 1.0);
        match p {
            SlotParam::Level => self.level_gain = sampler_map::level_gain(v),
            SlotParam::Pan => {
                let angle = v * FRAC_PI_2;
                self.pan_l = angle.cos() * SQRT_2;
                self.pan_r = angle.sin() * SQRT_2;
            }
            SlotParam::Tune => self.tune = sampler_map::tune_semitones(v),
            SlotParam::Fine => self.fine_cents = sampler_map::fine_cents(v),
            SlotParam::Cutoff => {
                self.cutoff_knob = v;
                self.cutoff_hz = sampler_map::cutoff_hz(v);
            }
            SlotParam::Resonance => self.resonance = v,
            SlotParam::Attack => self.attack_s = sampler_map::attack_seconds(v),
            SlotParam::Decay => self.decay_s = sampler_map::decay_seconds(v),
            SlotParam::Sustain => self.sustain = v,
            SlotParam::Release => self.release_s = sampler_map::release_seconds(v),
            SlotParam::Reverse => self.reverse = on(v),
            SlotParam::Choke => self.choke = on(v),
            SlotParam::TempoLock => self.tempo_lock = on(v),
        }
    }
}

impl Default for SlotSettings {
    /// Every slot knob at its default.
    fn default() -> Self {
        let mut s = SlotSettings {
            level_gain: 0.0,
            pan_l: 1.0,
            pan_r: 1.0,
            tune: 0,
            fine_cents: 0.0,
            cutoff_knob: 1.0,
            cutoff_hz: 18_000.0,
            resonance: 0.0,
            attack_s: 0.001,
            decay_s: 0.1,
            sustain: 1.0,
            release_s: 0.05,
            reverse: false,
            choke: false,
            tempo_lock: false,
        };
        for p in SlotParam::ALL {
            s.set(p, MachineParam::SamplerSlot(0, p).info().default);
        }
        s
    }
}
