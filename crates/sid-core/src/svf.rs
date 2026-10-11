//! Multimode state-variable filter (TPT / zero-delay-feedback form, after
//! Andrew Simper) with a soft clip on the band-pass state. Unconditionally
//! stable at any cutoff below Nyquist; the clip gives the squelchy,
//! self-limiting resonance a SID/303 filter wants.

use std::f32::consts::PI;

pub const FILTER_LP: u8 = 1;
pub const FILTER_BP: u8 = 2;
pub const FILTER_HP: u8 = 4;

/// Band-pass state limit; lower = earlier, harder resonance squelch.
const STATE_LIMIT: f32 = 3.0;

#[derive(Clone, Debug)]
pub struct Svf {
    pub mode: u8,
    sample_rate: f32,
    cutoff: f32,
    resonance: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic1: f32,
    ic2: f32,
}

impl Svf {
    pub fn new(sample_rate: f32) -> Self {
        let mut f = Self {
            mode: FILTER_LP,
            sample_rate,
            cutoff: -1.0,
            resonance: -1.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        f.set(1000.0, 0.0);
        f
    }

    /// cutoff in Hz (clamped 20 Hz .. min(18 kHz, 0.45 * sample rate)),
    /// resonance 0..1 (Q 0.707 .. ~50).
    pub fn set(&mut self, cutoff_hz: f32, resonance: f32) {
        if cutoff_hz == self.cutoff && resonance == self.resonance {
            return;
        }
        self.cutoff = cutoff_hz;
        self.resonance = resonance;
        let fc = cutoff_hz.clamp(20.0, (0.45 * self.sample_rate).min(18_000.0));
        let g = (PI * fc / self.sample_rate).tan();
        self.k = 1.414 - 1.394 * resonance.clamp(0.0, 1.0);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = soft_clip(2.0 * v1 - self.ic1);
        self.ic2 = 2.0 * v2 - self.ic2;
        if self.ic1.abs() < 1.0e-20 {
            self.ic1 = 0.0;
        }
        if self.ic2.abs() < 1.0e-20 {
            self.ic2 = 0.0;
        }
        let mut out = 0.0;
        if self.mode & FILTER_LP != 0 {
            out += v2;
        }
        if self.mode & FILTER_BP != 0 {
            out += v1;
        }
        if self.mode & FILTER_HP != 0 {
            out += x - self.k * v1 - v2;
        }
        out
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

fn soft_clip(x: f32) -> f32 {
    STATE_LIMIT * (x / STATE_LIMIT).tanh()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: f32 = 44_100.0;

    fn sine(freq: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (TAU * freq * i as f32 / SR).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn run(f: &mut Svf, input: &[f32]) -> Vec<f32> {
        input.iter().map(|&x| f.process(x)).collect()
    }

    #[test]
    fn lowpass_passes_dc() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(1000.0, 0.0);
        let out = run(&mut f, &vec![0.5; 4410]);
        assert!((out[4409] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn lowpass_attenuates_highs() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(200.0, 0.0);
        let out = run(&mut f, &sine(8000.0, 8820));
        assert!(rms(&out[4410..]) < 0.01, "rms = {}", rms(&out[4410..]));
    }

    #[test]
    fn highpass_blocks_dc() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_HP;
        f.set(500.0, 0.0);
        let out = run(&mut f, &vec![0.5; 44_100]);
        assert!(out[44_099].abs() < 1e-3);
    }

    #[test]
    fn bandpass_peaks_at_cutoff() {
        let mut at = Svf::new(SR);
        at.mode = FILTER_BP;
        at.set(1000.0, 0.5);
        let mut away = at.clone();
        let on = run(&mut at, &sine(1000.0, 8820));
        let off = run(&mut away, &sine(8000.0, 8820));
        assert!(rms(&on[4410..]) > 4.0 * rms(&off[4410..]));
    }

    #[test]
    fn max_resonance_is_bounded_and_finite() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(18_000.0, 1.0);
        let mut x = 0x1234_5678u32;
        for _ in 0..SR as usize {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let n = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let y = f.process(n);
            assert!(y.is_finite() && y.abs() < 10.0, "y = {y}");
        }
    }

    #[test]
    fn resonance_rings_after_impulse() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(500.0, 0.95);
        let mut input = vec![0.0; 4410];
        input[0] = 1.0;
        let out = run(&mut f, &input);
        assert!(rms(&out[441..882]) > 1e-3, "still ringing 10 ms later");
    }

    #[test]
    fn cutoff_is_clamped_to_stable_range() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(1.0e6, 0.5);
        let y = run(&mut f, &sine(100.0, 1000));
        assert!(y.iter().all(|v| v.is_finite()));
    }
}
