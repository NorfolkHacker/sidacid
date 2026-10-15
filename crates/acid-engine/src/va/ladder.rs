//! 24 dB/oct resonant low-pass: four zero-delay-feedback (TPT) one-pole
//! stages in a global feedback loop. The loop's input goes through tanh,
//! so at full resonance the filter self-oscillates at a bounded level.

use std::f32::consts::PI;

/// The Mono synth's filter: one channel of the four-stage ladder.
pub struct Ladder {
    s: [f32; 4],
    g: f32,
    /// g / (1 + g): each stage's instantaneous gain.
    big_g: f32,
    k: f32,
    sample_rate: f32,
}

impl Ladder {
    /// A silent filter at 1 kHz with no resonance.
    pub fn new(sample_rate: f32) -> Self {
        let mut l = Self {
            s: [0.0; 4],
            g: 0.0,
            big_g: 0.0,
            k: 0.0,
            sample_rate,
        };
        l.set(1000.0, 0.0);
        l
    }

    /// Clears the stage states (the settings stay).
    pub fn reset(&mut self) {
        self.s = [0.0; 4];
    }

    /// `cutoff_hz` is clamped to 10 Hz..0.45 × sample rate, `k` to 0..=4.2.
    pub fn set(&mut self, cutoff_hz: f32, k: f32) {
        let fc = cutoff_hz.clamp(10.0, 0.45 * self.sample_rate);
        self.g = (PI * fc / self.sample_rate).tan();
        self.big_g = self.g / (1.0 + self.g);
        self.k = k.clamp(0.0, 4.2);
    }

    /// Filters one sample.
    pub fn process(&mut self, x: f32) -> f32 {
        let gg = self.big_g;
        let b = 1.0 / (1.0 + self.g);
        // Linear solve of the loop: y4 = (G⁴·u + S) / (1 + k·G⁴), where S
        // is what the stage states contribute to the last stage's output.
        let s = &self.s;
        let s_sum = gg * gg * gg * b * s[0] + gg * gg * b * s[1] + gg * b * s[2] + b * s[3];
        let g4 = gg * gg * gg * gg;
        // Makeup so raising the resonance doesn't hollow out the passband.
        let input = x * (1.0 + 0.5 * self.k);
        let y4 = (g4 * input + s_sum) / (1.0 + self.k * g4);
        let mut u = (input - self.k * y4).tanh();
        for s in &mut self.s {
            let v = (u - *s) * gg;
            let y = v + *s;
            *s = y + v;
            u = y;
        }
        u
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{peak, rms};

    const SR: f32 = 48_000.0;

    fn ring(k: f32) -> Vec<f32> {
        let mut f = Ladder::new(SR);
        f.set(1000.0, k);
        (0..SR as usize)
            .map(|n| f.process(if n == 0 { 1.0 } else { 0.0 }))
            .collect()
    }

    #[test]
    fn full_resonance_self_oscillates_near_the_cutoff_and_stays_bounded() {
        let y = ring(4.2);
        let tail = &y[y.len() - 24_000..];
        assert!(rms(tail) > 0.05, "{}", rms(tail));
        assert!(peak(&y) < 2.0);
        let crossings = tail
            .windows(2)
            .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
            .count();
        // 1000 Hz over half a second: 1000 sign changes.
        assert!((950..=1050).contains(&crossings), "{crossings}");
    }

    #[test]
    fn below_the_threshold_it_rings_out() {
        let y = ring(3.0);
        assert!(rms(&y[y.len() - 4800..]) < 1e-4);
    }

    #[test]
    fn it_is_a_steep_low_pass() {
        let tone = |hz: f32| -> (f32, f32) {
            let mut f = Ladder::new(SR);
            f.set(500.0, 0.0);
            let x: Vec<f32> = (0..SR as usize)
                .map(|n| 0.1 * (std::f32::consts::TAU * hz * n as f32 / SR).sin())
                .collect();
            let y: Vec<f32> = x.iter().map(|v| f.process(*v)).collect();
            (rms(&x[4800..]), rms(&y[4800..]))
        };
        let (inp, out) = tone(100.0);
        assert!(out > 0.9 * inp, "passband {out} of {inp}");
        let (inp, out) = tone(5000.0);
        assert!(out < 0.01 * inp, "stopband {out} of {inp}");
    }
}
