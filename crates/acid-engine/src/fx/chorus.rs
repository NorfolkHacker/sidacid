//! Stereo chorus: one modulated delay line read at two LFO phases 90°
//! apart. Shared by the FM and Mono machines.

use crate::params::exp;
use std::f32::consts::{FRAC_PI_2, TAU};

pub struct Chorus {
    buf: Vec<f32>,
    write: usize,
    phase: f32,
    sample_rate: f32,
}

impl Chorus {
    /// Sizes the delay line to cover the 20 ms maximum delay at
    /// `sample_rate`. Allocates: call off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        let len = ((0.021 * sample_rate.max(1.0)) as usize + 2).next_power_of_two();
        Self {
            buf: vec![0.0; len],
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
        let len = self.buf.len();
        let pos = self.write as f32 - delay + len as f32;
        let i = pos.floor();
        let f = pos - i;
        let i = i as usize % len;
        let j = (i + 1) % len;
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
        self.write = (self.write + 1) % self.buf.len();
        out
    }
}
