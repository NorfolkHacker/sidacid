//! A pad's knobs in real units, as the Sampler voice takes them.

use crate::params::{MachineParam, PadParam, on, pad_map};
use crate::sampler::settings::SlotSettings;
use std::f32::consts::{FRAC_PI_2, SQRT_2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadSettings {
    /// Level, pan, tune/fine, cutoff, resonance, envelope and reverse.
    pub voice: SlotSettings,
    /// 0..=1 of the sample.
    pub start: f32,
    pub end: f32,
    pub gate: bool,
    /// 0 = no group.
    pub group: u8,
}

impl PadSettings {
    pub fn set(&mut self, p: PadParam, v: f32) {
        let v = v.clamp(0.0, 1.0);
        let s = &mut self.voice;
        match p {
            PadParam::Tune => {
                let semis = pad_map::tune_semitones(v);
                s.tune = semis.trunc() as i32;
                s.fine_cents = semis.fract() * 100.0;
            }
            PadParam::Decay => match pad_map::decay_seconds(v) {
                // Open: the amp holds until the sample (or the gate) ends.
                None => s.sustain = 1.0,
                Some(secs) => {
                    s.sustain = 0.0;
                    s.decay_s = secs;
                }
            },
            PadParam::Level => s.level_gain = pad_map::level_gain(v),
            PadParam::Pan => {
                let angle = v * FRAC_PI_2;
                s.pan_l = angle.cos() * SQRT_2;
                s.pan_r = angle.sin() * SQRT_2;
            }
            PadParam::Cutoff => {
                s.cutoff_knob = v;
                s.cutoff_hz = pad_map::cutoff_hz(v);
            }
            PadParam::Resonance => s.resonance = v,
            PadParam::Start => self.start = v,
            PadParam::End => self.end = v,
            PadParam::Reverse => s.reverse = on(v),
            PadParam::Mode => self.gate = pad_map::gate(v),
            PadParam::Group => self.group = pad_map::group(v),
        }
    }
}

impl Default for PadSettings {
    /// Every pad knob at its default.
    fn default() -> Self {
        // A drum hit: instant attack, and the Gate release is the 5 ms fade.
        let voice = SlotSettings {
            attack_s: 0.0005,
            release_s: 0.005,
            ..SlotSettings::default()
        };
        let mut s = PadSettings {
            voice,
            start: 0.0,
            end: 1.0,
            gate: false,
            group: 0,
        };
        for p in PadParam::ALL {
            s.set(p, MachineParam::Pad(0, p).info().default);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{PadParam, pad_map};

    #[test]
    fn defaults_are_a_one_shot_hit_at_the_default_knobs() {
        let s = PadSettings::default();
        assert_eq!((s.start, s.end, s.gate, s.group), (0.0, 1.0, false, 0));
        assert_eq!(s.voice.tune, 0);
        assert_eq!(s.voice.fine_cents, 0.0);
        assert_eq!(s.voice.sustain, 1.0, "Decay open: the sample plays out");
        assert_eq!(s.voice.level_gain, pad_map::level_gain(0.8));
        assert!((s.voice.pan_l - s.voice.pan_r).abs() < 1e-6);
        assert!(!s.voice.reverse);
        assert_eq!(s.voice.release_s, 0.005);
    }

    #[test]
    fn knobs_map_to_real_units() {
        let mut s = PadSettings::default();
        s.set(PadParam::Tune, 0.75); // +12 semitones
        assert_eq!((s.voice.tune, s.voice.fine_cents), (12, 0.0));
        s.set(PadParam::Tune, 0.5 + 1.5 / 48.0); // +1.5 semitones
        assert_eq!(s.voice.tune, 1);
        assert!((s.voice.fine_cents - 50.0).abs() < 1e-2);
        s.set(PadParam::Decay, 0.0);
        assert_eq!(s.voice.sustain, 0.0);
        assert!((s.voice.decay_s - 0.01).abs() < 1e-6);
        s.set(PadParam::Decay, 1.0);
        assert_eq!(s.voice.sustain, 1.0);
        s.set(PadParam::Level, 0.5);
        assert!((s.voice.level_gain - 0.25).abs() < 1e-6, "the kits' curve");
        s.set(PadParam::Pan, 0.0);
        assert!(s.voice.pan_r.abs() < 1e-6);
        s.set(PadParam::Cutoff, 0.0);
        assert_eq!((s.voice.cutoff_knob, s.voice.cutoff_hz), (0.0, 20.0));
        s.set(PadParam::Start, 0.25);
        s.set(PadParam::End, 0.5);
        s.set(PadParam::Reverse, 1.0);
        s.set(PadParam::Mode, 1.0);
        s.set(PadParam::Group, 1.0);
        assert_eq!(
            (s.start, s.end, s.voice.reverse, s.gate, s.group),
            (0.25, 0.5, true, true, 4)
        );
    }
}
