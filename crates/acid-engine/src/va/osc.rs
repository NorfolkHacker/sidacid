//! The Mono synth's VCO: band-limited (PolyBLEP) saw and pulse, plus a
//! sub oscillator that counts the VCO's cycles so it stays phase-locked.

/// Sub oscillator shapes, in `MonoParam::SubWave` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubWave {
    Oct1Square,
    Oct2Square,
    /// Two octaves down, high one cycle in four (25%).
    Oct2Pulse,
}

/// PolyBLEP residual at phase `t` for a step of −2 at phase 0. For a step
/// of height `h`, add `h / 2 · poly_blep`.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VcoOut {
    pub saw: f32,
    pub pulse: f32,
    pub sub: f32,
}

pub struct Vco {
    phase: f32,
    /// VCO cycles completed; the sub osc's shape is read from its low bits.
    cycles: u32,
}

impl Default for Vco {
    fn default() -> Self {
        Self::new()
    }
}

impl Vco {
    pub fn new() -> Self {
        Self { phase: 0.0, cycles: 0 }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    fn sub_value(wave: SubWave, cycles: u32) -> f32 {
        let high = match wave {
            SubWave::Oct1Square => cycles & 1 == 0,
            SubWave::Oct2Square => cycles & 2 == 0,
            SubWave::Oct2Pulse => cycles & 3 == 0,
        };
        if high { 1.0 } else { -1.0 }
    }

    /// One sample. `dt` is frequency / sample rate (clamped to 0..=0.45);
    /// `duty` is the pulse's high fraction (clamped to 0.05..=0.5).
    pub fn next(&mut self, dt: f32, duty: f32, sub: SubWave) -> VcoOut {
        let dt = dt.clamp(0.0, 0.45);
        let duty = duty.clamp(0.05, 0.5);
        let t = self.phase;
        let saw = 2.0 * t - 1.0 - poly_blep(t, dt);
        let naive = if t < duty { 1.0 } else { -1.0 };
        let pulse = naive + poly_blep(t, dt) - poly_blep((t - duty).rem_euclid(1.0), dt);
        // The sub only steps at the VCO's wrap, so it shares the saw's blep
        // window: just after the wrap it smooths the step it took, just
        // before it the step it is about to take.
        let cur = Self::sub_value(sub, self.cycles);
        let mut s = cur;
        if t < dt {
            let prev = Self::sub_value(sub, self.cycles.wrapping_sub(1));
            s += 0.5 * (cur - prev) * poly_blep(t, dt);
        } else if t > 1.0 - dt {
            let next = Self::sub_value(sub, self.cycles.wrapping_add(1));
            s += 0.5 * (next - cur) * poly_blep(t, dt);
        }
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.cycles = self.cycles.wrapping_add(1);
        }
        VcoOut { saw, pulse, sub: s }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Magnitude of `hz` in `x` (one DFT bin).
    fn magnitude(x: &[f32], hz: f64) -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, v) in x.iter().enumerate() {
            let a = std::f64::consts::TAU * hz * n as f64 / SR as f64;
            re += *v as f64 * a.cos();
            im -= *v as f64 * a.sin();
        }
        (re * re + im * im).sqrt() / x.len() as f64
    }

    #[test]
    fn the_saw_aliases_far_less_than_a_naive_saw() {
        // 2950 Hz: harmonic 9 (26550 Hz) folds to 21450 Hz.
        let f = 2950.0;
        let mut vco = Vco::new();
        let blep: Vec<f32> = (0..SR as usize)
            .map(|_| vco.next(f / SR, 0.5, SubWave::Oct1Square).saw)
            .collect();
        let naive: Vec<f32> = (0..SR as usize)
            .map(|n| 2.0 * ((n as f32 * f / SR) % 1.0) - 1.0)
            .collect();
        let ratio = |x: &[f32]| magnitude(x, 21_450.0) / magnitude(x, f as f64);
        assert!(ratio(&blep) < 0.5 * ratio(&naive), "{} vs {}", ratio(&blep), ratio(&naive));
    }

    fn sign_changes(x: &[f32]) -> usize {
        x.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count()
    }

    #[test]
    fn the_sub_is_an_octave_below_and_locked() {
        let mut vco = Vco::new();
        let sub: Vec<f32> = (0..SR as usize)
            .map(|_| vco.next(1000.0 / SR, 0.5, SubWave::Oct1Square).sub)
            .collect();
        // A 500 Hz square changes sign 1000 times a second.
        assert!((sign_changes(&sub) as i32 - 1000).abs() <= 2, "{}", sign_changes(&sub));
    }

    #[test]
    fn the_two_octave_pulse_is_high_a_quarter_of_the_time() {
        let mut vco = Vco::new();
        let sub: Vec<f32> = (0..SR as usize)
            .map(|_| vco.next(1000.0 / SR, 0.5, SubWave::Oct2Pulse).sub)
            .collect();
        let high = sub.iter().filter(|v| **v > 0.0).count() as f32 / sub.len() as f32;
        assert!((high - 0.25).abs() < 0.01, "{high}");
    }

    #[test]
    fn pulse_width_sets_the_duty_cycle() {
        for (duty, mean) in [(0.5, 0.0), (0.1, -0.8)] {
            let mut vco = Vco::new();
            let x: Vec<f32> = (0..SR as usize)
                .map(|_| vco.next(220.0 / SR, duty, SubWave::Oct1Square).pulse)
                .collect();
            let m = x.iter().sum::<f32>() / x.len() as f32;
            assert!((m - mean).abs() < 0.02, "duty {duty}: mean {m}");
        }
    }
}
