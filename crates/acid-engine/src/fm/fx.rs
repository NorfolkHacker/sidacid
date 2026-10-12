//! The FM-1's built-in effects, in order: filter, chorus, phaser. They
//! take the synth's mono mix and add a stereo result into the outputs.
//! With the filter off and both mixes at 0 the signal passes untouched.

use super::patch::FmPatch;
use crate::params::exp;
use sid_core::{FILTER_HP, FILTER_LP, Svf};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Chorus delay line length: covers 20 ms at 192 kHz.
const CHORUS_LEN: usize = 4096;
const PHASER_STAGES: usize = 4;

pub struct Chorus {
    buf: Vec<f32>,
    write: usize,
    phase: f32,
    sample_rate: f32,
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            buf: vec![0.0; CHORUS_LEN],
            write: 0,
            phase: 0.0,
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.phase = 0.0;
    }

    fn read(&self, delay: f32) -> f32 {
        let pos = self.write as f32 - delay + CHORUS_LEN as f32;
        let i = pos.floor();
        let f = pos - i;
        let i = i as usize % CHORUS_LEN;
        let j = (i + 1) % CHORUS_LEN;
        self.buf[i] + (self.buf[j] - self.buf[i]) * f
    }

    /// Writes `x` and returns the (left, right) wet samples.
    pub fn process(&mut self, x: f32, rate: f32, depth: f32) -> (f32, f32) {
        self.buf[self.write] = x;
        self.phase += exp(rate, 0.05, 5.0) / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let base = 0.012 * self.sample_rate;
        let swing = 0.008 * self.sample_rate * depth.clamp(0.0, 1.0);
        let dl = base + swing * 0.5 * (1.0 + (TAU * self.phase).sin());
        let dr = base + swing * 0.5 * (1.0 + (TAU * self.phase + FRAC_PI_2).sin());
        let out = (self.read(dl), self.read(dr));
        self.write = (self.write + 1) % CHORUS_LEN;
        out
    }
}

pub struct Phaser {
    state: [[f32; PHASER_STAGES]; 2],
    last: [f32; 2],
    phase: f32,
    sample_rate: f32,
}

impl Phaser {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            state: [[0.0; PHASER_STAGES]; 2],
            last: [0.0; 2],
            phase: 0.0,
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; PHASER_STAGES]; 2];
        self.last = [0.0; 2];
    }

    /// Four swept all-pass stages per side; returns the all-passed pair.
    pub fn process(&mut self, x: [f32; 2], rate: f32, depth: f32, feedback: f32) -> [f32; 2] {
        self.phase += exp(rate, 0.05, 5.0) / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let mut out = [0.0; 2];
        for ch in 0..2 {
            let sweep = 0.5 * (1.0 + (TAU * (self.phase + ch as f32 * 0.25)).sin());
            let f =
                (200.0 * 20f32.powf(depth.clamp(0.0, 1.0) * sweep)).min(0.45 * self.sample_rate);
            let t = (PI * f / self.sample_rate).tan();
            let a = (t - 1.0) / (t + 1.0);
            let mut y = x[ch] + self.last[ch] * feedback.clamp(0.0, 1.0) * 0.7;
            for s in &mut self.state[ch] {
                let o = a * y + *s;
                *s = y - a * o;
                y = o;
            }
            self.last[ch] = y;
            out[ch] = y;
        }
        out
    }
}

pub struct FmFx {
    filter: Svf,
    chorus: Chorus,
    phaser: Phaser,
    sample_rate: f32,
}

impl FmFx {
    /// Allocates the chorus delay line: call off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            filter: Svf::new(sample_rate),
            chorus: Chorus::new(sample_rate),
            phaser: Phaser::new(sample_rate),
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.filter = Svf::new(self.sample_rate);
        self.chorus.reset();
        self.phaser.reset();
    }

    pub fn process(&mut self, p: &FmPatch, mono: &mut [f32], left: &mut [f32], right: &mut [f32]) {
        if p.filter_on {
            self.filter.mode = if p.filter_hp { FILTER_HP } else { FILTER_LP };
            self.filter.set(p.cutoff_hz(), p.resonance);
            for x in mono.iter_mut() {
                *x = self.filter.process(*x);
            }
        }
        for (i, &x) in mono.iter().enumerate() {
            let (wl, wr) = self.chorus.process(x, p.chorus_rate, p.chorus_depth);
            let mut s = [x + (wl - x) * p.chorus_mix, x + (wr - x) * p.chorus_mix];
            if p.phaser_mix > 0.0 {
                let w = self
                    .phaser
                    .process(s, p.phaser_rate, p.phaser_depth, p.phaser_feedback);
                for ch in 0..2 {
                    s[ch] += (w[ch] - s[ch]) * 0.5 * p.phaser_mix;
                }
            }
            left[i] += s[0];
            right[i] += s[1];
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, rms};

    const SR: f32 = 48_000.0;

    fn tone(hz: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (i as f32 / SR * hz * std::f32::consts::TAU).sin() * 0.5)
            .collect()
    }

    fn run(p: &FmPatch, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut fx = FmFx::new(SR);
        let mut mono = input.to_vec();
        let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        for ((m, l), r) in mono
            .chunks_mut(32)
            .zip(l.chunks_mut(32))
            .zip(r.chunks_mut(32))
        {
            fx.process(p, m, l, r);
        }
        (l, r)
    }

    #[test]
    fn defaults_pass_the_dry_signal_through() {
        let x = tone(300.0, 4800);
        let (l, r) = run(&FmPatch::default(), &x);
        assert_eq!(l, x);
        assert_eq!(r, x);
    }

    #[test]
    fn low_pass_filter_tames_highs_and_high_pass_tames_lows() {
        let mut p = FmPatch::default();
        p.filter_on = true;
        p.cutoff = 0.3; // a few hundred Hz
        let hi = tone(6000.0, 9600);
        let (l, _) = run(&p, &hi);
        assert!(rms(&l[4800..]) < rms(&hi[4800..]) * 0.2);
        p.filter_hp = true;
        p.cutoff = 0.8;
        let lo = tone(100.0, 9600);
        let (l, _) = run(&p, &lo);
        assert!(rms(&l[4800..]) < rms(&lo[4800..]) * 0.2);
    }

    #[test]
    fn chorus_widens_and_phaser_colours() {
        let x = tone(440.0, 24_000);
        let mut p = FmPatch::default();
        p.chorus_mix = 1.0;
        let (l, r) = run(&p, &x);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(
            rms(&l.iter().zip(&r).map(|(a, b)| a - b).collect::<Vec<_>>()) > 0.01,
            "stereo"
        );
        let mut p = FmPatch::default();
        p.phaser_mix = 1.0;
        p.phaser_feedback = 1.0;
        let (l, _) = run(&p, &x);
        assert!(all_finite(&l));
        assert!(rms(&l.iter().zip(&x).map(|(a, b)| a - b).collect::<Vec<_>>()) > 0.01);
        assert!(l.iter().all(|v| v.abs() < 4.0));
    }

    #[test]
    fn extreme_settings_stay_finite() {
        let mut p = FmPatch::default();
        p.filter_on = true;
        p.cutoff = 1.0;
        p.resonance = 1.0;
        p.chorus_mix = 1.0;
        p.chorus_depth = 1.0;
        p.chorus_rate = 1.0;
        p.phaser_mix = 1.0;
        p.phaser_feedback = 1.0;
        p.phaser_rate = 1.0;
        p.phaser_depth = 1.0;
        let (l, r) = run(&p, &tone(1000.0, 48_000));
        assert!(all_finite(&l) && all_finite(&r));
    }
}
