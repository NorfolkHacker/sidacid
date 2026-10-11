//! Waveshaping distortion: asymmetric tanh with 2x oversampling, a tone
//! low-pass and dry/wet mix.

use crate::params::{exp, lin};
use std::f32::consts::TAU;

const BIAS: f32 = 0.15;

pub struct Distortion {
    pub on: bool,
    sample_rate: f32,
    drive: f32,
    mix: f32,
    tone_coef: f32,
    lp: [f32; 2],
    prev: [f32; 2],
}

fn shape(x: f32, drive: f32) -> f32 {
    (drive * x + BIAS).tanh() - BIAS.tanh()
}

impl Distortion {
    pub fn new(sample_rate: f32) -> Self {
        let mut d = Self { on: false, sample_rate, drive: 1.0, mix: 1.0, tone_coef: 1.0, lp: [0.0; 2], prev: [0.0; 2] };
        d.set_drive(0.3);
        d.set_tone(0.6);
        d
    }

    pub fn set_drive(&mut self, v: f32) {
        self.drive = exp(v, 1.0, 40.0);
    }

    pub fn set_tone(&mut self, v: f32) {
        let fc = exp(v, 800.0, 12_000.0);
        self.tone_coef = 1.0 - (-TAU * fc / self.sample_rate).exp();
    }

    pub fn set_mix(&mut self, v: f32) {
        self.mix = lin(v, 0.0, 1.0);
    }

    /// Clears filter state (used by the mixer's NaN guard).
    pub fn reset(&mut self) {
        self.lp = [0.0; 2];
        self.prev = [0.0; 2];
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if !self.on {
            return;
        }
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let dry = *x;
                let mid = 0.5 * (self.prev[ch] + dry);
                self.prev[ch] = dry;
                let wet = 0.5 * (shape(mid, self.drive) + shape(dry, self.drive));
                self.lp[ch] += (wet - self.lp[ch]) * self.tone_coef;
                *x = dry + (self.lp[ch] - dry) * self.mix;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};
    use std::f32::consts::TAU;

    const SR: f32 = 44_100.0;

    fn sine(amp: f32) -> Vec<f32> {
        (0..4410).map(|i| amp * (TAU * 110.0 * i as f32 / SR).sin()).collect()
    }

    #[test]
    fn off_is_passthrough() {
        let mut d = Distortion::new(SR);
        let mut l = sine(0.5);
        let mut r = l.clone();
        let orig = l.clone();
        d.process(&mut l, &mut r);
        assert_eq!(l, orig);
    }

    #[test]
    fn high_drive_saturates_and_stays_bounded() {
        let mut d = Distortion::new(SR);
        d.on = true;
        d.set_drive(1.0);
        d.set_tone(1.0);
        d.set_mix(1.0);
        let mut l = sine(1.0);
        let mut r = l.clone();
        d.process(&mut l, &mut r);
        assert!(all_finite(&l));
        assert!(peak(&l) < 1.3);
        // Saturated: most of the waveform sits near the rails.
        let near_rail = l.iter().filter(|v| v.abs() > 0.7).count();
        assert!(near_rail > l.len() / 2, "near_rail = {near_rail}");
    }

    #[test]
    fn zero_mix_is_dry() {
        let mut d = Distortion::new(SR);
        d.on = true;
        d.set_drive(1.0);
        d.set_mix(0.0);
        let mut l = sine(0.5);
        let mut r = l.clone();
        let orig = l.clone();
        d.process(&mut l, &mut r);
        for (a, b) in l.iter().zip(&orig) {
            assert!((a - b).abs() < 1e-6);
        }
    }
}
