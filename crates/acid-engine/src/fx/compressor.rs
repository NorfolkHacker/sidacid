//! Feed-forward stereo-linked peak compressor with a 6 dB soft knee.

use crate::params::{db_to_gain, exp, lin};

const KNEE_DB: f32 = 6.0;

pub struct Compressor {
    pub on: bool,
    sample_rate: f32,
    threshold_db: f32,
    ratio: f32,
    attack_coef: f32,
    release_coef: f32,
    makeup: f32,
    gr_db: f32,
}

impl Compressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut c = Self {
            on: false,
            sample_rate,
            threshold_db: -12.0,
            ratio: 4.0,
            attack_coef: 0.0,
            release_coef: 0.0,
            makeup: 1.0,
            gr_db: 0.0,
        };
        c.set_attack(0.2);
        c.set_release(0.4);
        c
    }

    /// -40 .. 0 dB.
    pub fn set_threshold(&mut self, v: f32) {
        self.threshold_db = lin(v, -40.0, 0.0);
    }

    /// 1:1 .. 20:1.
    pub fn set_ratio(&mut self, v: f32) {
        self.ratio = exp(v, 1.0, 20.0);
    }

    /// 0.1 .. 100 ms.
    pub fn set_attack(&mut self, v: f32) {
        self.attack_coef = (-1.0 / (exp(v, 0.0001, 0.1) * self.sample_rate)).exp();
    }

    /// 10 .. 1000 ms.
    pub fn set_release(&mut self, v: f32) {
        self.release_coef = (-1.0 / (exp(v, 0.01, 1.0) * self.sample_rate)).exp();
    }

    /// 0 .. +24 dB.
    pub fn set_makeup(&mut self, v: f32) {
        self.makeup = db_to_gain(lin(v, 0.0, 24.0));
    }

    pub fn gain_reduction_db(&self) -> f32 {
        self.gr_db
    }

    /// Clears detector state (used by the mixer's NaN guard).
    pub fn reset(&mut self) {
        self.gr_db = 0.0;
    }

    fn target_gr_db(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold_db;
        let slope = 1.0 / self.ratio - 1.0;
        if over <= -KNEE_DB / 2.0 {
            0.0
        } else if over < KNEE_DB / 2.0 {
            slope * (over + KNEE_DB / 2.0).powi(2) / (2.0 * KNEE_DB)
        } else {
            slope * over
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if !self.on {
            return;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let peak = a.abs().max(b.abs()).max(1.0e-6);
            let target = self.target_gr_db(20.0 * peak.log10());
            let coef = if target < self.gr_db {
                self.attack_coef
            } else {
                self.release_coef
            };
            self.gr_db = target + (self.gr_db - target) * coef;
            let g = db_to_gain(self.gr_db) * self.makeup;
            *a *= g;
            *b *= g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::peak;
    use std::f32::consts::TAU;

    const SR: f32 = 44_100.0;

    fn sine(amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (TAU * 220.0 * i as f32 / SR).sin())
            .collect()
    }

    fn comp(threshold: f32, ratio: f32) -> Compressor {
        let mut c = Compressor::new(SR);
        c.on = true;
        c.set_threshold(threshold);
        c.set_ratio(ratio);
        c.set_attack(0.2);
        c.set_release(0.5);
        c.set_makeup(0.0);
        c
    }

    #[test]
    fn below_threshold_is_unity() {
        let mut c = comp(0.75, 1.0); // -10 dB threshold
        let mut l = sine(0.05, 22_050);
        let mut r = l.clone();
        let orig = l.clone();
        c.process(&mut l, &mut r);
        assert!((peak(&l[11_025..]) - peak(&orig[11_025..])).abs() < 1e-3);
    }

    #[test]
    fn loud_signal_is_reduced_toward_threshold() {
        let mut c = comp(0.5, 1.0); // -20 dB threshold, ratio 20:1
        let mut l = sine(1.0, 44_100);
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        let p = peak(&l[33_075..]);
        assert!(p > 0.08 && p < 0.2, "peak = {p}");
        assert!(c.gain_reduction_db() < -15.0);
    }

    #[test]
    fn off_is_passthrough() {
        let mut c = Compressor::new(SR);
        let mut l = sine(1.0, 1000);
        let mut r = l.clone();
        let orig = l.clone();
        c.process(&mut l, &mut r);
        assert_eq!(l, orig);
    }
}
