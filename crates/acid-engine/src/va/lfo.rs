//! The Mono synth's modulation sources: the LFO, a white-noise source
//! (also the noise oscillator) and Drift, a slow analog-style wander.

/// Tiny deterministic noise source (xorshift32), so renders repeat exactly.
pub struct Noise(u32);

impl Noise {
    /// A source seeded with `seed` (0 is taken as 1).
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// White noise in −1..=1.
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// LFO shapes, in `MonoParam::LfoWave` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoWave {
    Triangle,
    Square,
    /// Sample & hold: a new random value each cycle.
    Random,
    Noise,
}

/// The free-running LFO; one per synth, shared by every destination.
pub struct Lfo {
    phase: f32,
    held: f32,
    noise: Noise,
    sample_rate: f32,
}

impl Lfo {
    /// An LFO at the start of its cycle.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            held: 0.0,
            noise: Noise::new(0x1234_5678),
            sample_rate,
        }
    }

    /// Back to the start of the cycle, the held value at 0.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.held = 0.0;
    }

    /// One sample at `hz`: the value in −1..=1, and true when a cycle just
    /// began (the envelope's LFO trigger).
    pub fn next(&mut self, wave: LfoWave, hz: f32) -> (f32, bool) {
        self.phase += hz / self.sample_rate;
        let wrapped = self.phase >= 1.0;
        if wrapped {
            self.phase -= self.phase.floor();
            self.held = self.noise.next();
        }
        let p = self.phase;
        let v = match wave {
            LfoWave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
            LfoWave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoWave::Random => self.held,
            LfoWave::Noise => self.noise.next(),
        };
        (v, wrapped)
    }
}

/// Samples between Drift's new random targets (~85 ms at 48 kHz).
const DRIFT_HOLD: u32 = 4096;
/// Drift's smoothing time constant, in seconds.
const DRIFT_TAU_S: f32 = 0.4;

/// Two smoothed random walks (pitch, cutoff). The caller scales them by
/// the Drift knob, so Drift 0 adds exactly nothing.
pub struct Drift {
    noise: Noise,
    targets: [f32; 2],
    values: [f32; 2],
    count: u32,
    coef: f32,
}

impl Drift {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            noise: Noise::new(0x9e37_79b9),
            targets: [0.0; 2],
            values: [0.0; 2],
            count: 0,
            coef: 1.0 - (-1.0 / (DRIFT_TAU_S * sample_rate)).exp(),
        }
    }

    pub fn reset(&mut self) {
        self.targets = [0.0; 2];
        self.values = [0.0; 2];
        self.count = 0;
    }

    /// (pitch, cutoff) wander, each in −1..=1.
    pub fn next(&mut self) -> [f32; 2] {
        if self.count == 0 {
            self.targets = [self.noise.next(), self.noise.next()];
            self.count = DRIFT_HOLD;
        }
        self.count -= 1;
        for i in 0..2 {
            self.values[i] += (self.targets[i] - self.values[i]) * self.coef;
        }
        self.values
    }

    /// A fresh per-note offset in −1..=1.
    pub fn note_offset(&mut self) -> f32 {
        self.noise.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn the_triangle_spans_minus_one_to_one_and_wraps_once_a_cycle() {
        let mut l = Lfo::new(SR);
        let (mut lo, mut hi, mut wraps) = (f32::MAX, f32::MIN, 0);
        for _ in 0..2 * SR as usize {
            let (v, w) = l.next(LfoWave::Triangle, 1.0);
            lo = lo.min(v);
            hi = hi.max(v);
            wraps += w as usize;
        }
        assert!(lo < -0.99 && hi > 0.99, "{lo}..{hi}");
        assert_eq!(wraps, 2);
    }

    #[test]
    fn random_holds_its_value_for_a_cycle() {
        let mut l = Lfo::new(SR);
        let mut changes = 0;
        let mut last = l.next(LfoWave::Random, 10.0).0;
        for _ in 0..SR as usize {
            let v = l.next(LfoWave::Random, 10.0).0;
            changes += (v != last) as usize;
            last = v;
        }
        assert!((9..=11).contains(&changes), "{changes}");
    }

    #[test]
    fn drift_wanders_slowly_and_deterministically() {
        let (mut a, mut b) = (Drift::new(SR), Drift::new(SR));
        let mut moved = false;
        let mut prev = a.next();
        b.next();
        for _ in 0..SR as usize {
            let v = a.next();
            assert_eq!(v, b.next());
            assert!(v.iter().all(|x| (-1.0..=1.0).contains(x)));
            assert!((v[0] - prev[0]).abs() < 0.01, "slow");
            moved |= v != prev;
            prev = v;
        }
        assert!(moved);
    }
}
