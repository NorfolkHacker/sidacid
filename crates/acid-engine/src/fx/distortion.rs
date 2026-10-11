//! Waveshaping distortion: asymmetric tanh with real 2x oversampling, a
//! tone low-pass, a DC blocker and dry/wet mix.

use crate::params::{exp, lin};
use std::f32::consts::TAU;

const BIAS: f32 = 0.15;
/// DC blocker pole.
const DC_R: f32 = 0.9995;
/// Butterworth Q for the 2x-rate anti-imaging / anti-alias low-pass.
const BUTTER_Q: f32 = 0.707_1;

pub struct Distortion {
    pub on: bool,
    sample_rate: f32,
    drive: f32,
    mix: f32,
    tone_coef: f32,
    /// Biquad low-pass coefficients at the 2x rate: b0, b1, b2, a1, a2.
    bq: [f32; 5],
    /// Per channel: previous input sample (for the upsampling midpoint).
    prev: [f32; 2],
    /// Per channel: direct form II transposed biquad state.
    bq_z1: [f32; 2],
    bq_z2: [f32; 2],
    /// Per channel: tone low-pass state.
    lp: [f32; 2],
    /// Per channel: DC blocker state.
    dc_x1: [f32; 2],
    dc_y1: [f32; 2],
}

fn shape(x: f32, drive: f32) -> f32 {
    (drive * x + BIAS).tanh() - BIAS.tanh()
}

/// RBJ cookbook low-pass, normalised. Returns [b0, b1, b2, a1, a2].
fn butter_lpf(fc: f32, fs: f32) -> [f32; 5] {
    let w0 = TAU * fc / fs;
    let (sin_w, cos_w) = w0.sin_cos();
    let alpha = sin_w / (2.0 * BUTTER_Q);
    let a0 = 1.0 + alpha;
    let b0 = (1.0 - cos_w) * 0.5 / a0;
    let b1 = (1.0 - cos_w) / a0;
    [b0, b1, b0, -2.0 * cos_w / a0, (1.0 - alpha) / a0]
}

impl Distortion {
    pub fn new(sample_rate: f32) -> Self {
        let mut d = Self {
            on: false,
            sample_rate,
            drive: 1.0,
            mix: 1.0,
            tone_coef: 1.0,
            bq: butter_lpf(0.45 * sample_rate, 2.0 * sample_rate),
            prev: [0.0; 2],
            bq_z1: [0.0; 2],
            bq_z2: [0.0; 2],
            lp: [0.0; 2],
            dc_x1: [0.0; 2],
            dc_y1: [0.0; 2],
        };
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

    /// Clears all filter and oversampling state (used by the mixer's NaN guard).
    pub fn reset(&mut self) {
        self.prev = [0.0; 2];
        self.bq_z1 = [0.0; 2];
        self.bq_z2 = [0.0; 2];
        self.lp = [0.0; 2];
        self.dc_x1 = [0.0; 2];
        self.dc_y1 = [0.0; 2];
    }

    /// One sample through the 2x-rate biquad (direct form II transposed).
    #[inline]
    fn biquad(&mut self, ch: usize, x: f32) -> f32 {
        let [b0, b1, b2, a1, a2] = self.bq;
        let y = b0 * x + self.bq_z1[ch];
        self.bq_z1[ch] = b1 * x - a1 * y + self.bq_z2[ch];
        self.bq_z2[ch] = b2 * x - a2 * y;
        y
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if !self.on {
            return;
        }
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let dry = *x;
                // Upsample to 2x: midpoint, then the sample itself.
                let mid = 0.5 * (self.prev[ch] + dry);
                self.prev[ch] = dry;
                // Shape both 2x samples, then filter them at the 2x rate.
                let _ = self.biquad(ch, shape(mid, self.drive));
                let os = self.biquad(ch, shape(dry, self.drive));
                // Decimate: keep the second 2x sample only.
                self.lp[ch] += (os - self.lp[ch]) * self.tone_coef;
                // DC blocker on the wet path at base rate.
                let wet = self.lp[ch] - self.dc_x1[ch] + DC_R * self.dc_y1[ch];
                self.dc_x1[ch] = self.lp[ch];
                self.dc_y1[ch] = wet;
                *x = dry + (wet - dry) * self.mix;
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

    #[test]
    fn dc_is_removed_at_high_drive() {
        let mut d = Distortion::new(SR);
        d.on = true;
        d.set_drive(1.0);
        d.set_tone(1.0);
        d.set_mix(1.0);
        let mut l: Vec<f32> = (0..44_100).map(|i| (TAU * 110.0 * i as f32 / SR).sin()).collect();
        let mut r = l.clone();
        d.process(&mut l, &mut r);
        let tail = &l[22_050..];
        let mean = tail.iter().sum::<f32>() / tail.len() as f32;
        assert!(mean.abs() < 0.01, "mean = {mean}");
    }
}
