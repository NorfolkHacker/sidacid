//! The FM LFO and pitch envelope. Ranges approximate the DX7: LFO about
//! 0.06–50 Hz, pitch EG ±4 octaves, PMS and AMS as depth tables.

use std::f32::consts::TAU;

/// Semitones of LFO pitch swing at pitch depth 99, by pitch-mod sensitivity 0..7.
pub const PMS_SEMITONES: [f32; 8] = [0.0, 0.1, 0.2, 0.35, 0.6, 1.2, 2.4, 12.0];
/// Log2 attenuation at LFO amp depth 99, by amp-mod sensitivity 0..3.
pub const AMS_DEPTH: [f32; 4] = [0.0, 0.7, 1.7, 4.0];

pub fn lfo_hz(speed99: f32) -> f32 {
    0.062 * 2f32.powf(speed99.clamp(0.0, 99.0) / 99.0 * 9.66)
}

pub fn lfo_delay_seconds(delay99: f32) -> f32 {
    (delay99.clamp(0.0, 99.0) / 99.0).powi(2) * 5.0
}

/// How much of the LFO a note gets `t` seconds after it started: none
/// for the first half of the delay, fading in over the second half.
pub fn lfo_delay_gain(t: f32, delay_s: f32) -> f32 {
    if delay_s <= 0.0 {
        1.0
    } else {
        ((t - 0.5 * delay_s) / (0.5 * delay_s)).clamp(0.0, 1.0)
    }
}

/// One LFO shared by all voices. Waves: 0 tri, 1 saw down, 2 saw up,
/// 3 square, 4 sine, 5 sample & hold.
#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    phase: f32,
    held: f32,
    rng: u32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Lfo {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            held: 0.0,
            rng: 0x1234_5678,
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Advances by `seconds`; returns the value in -1..=1.
    pub fn tick(&mut self, hz: f32, wave: usize, seconds: f32) -> f32 {
        self.phase += hz * seconds;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.held = self.next_random();
        }
        let p = self.phase;
        match wave {
            0 => {
                if p < 0.5 {
                    4.0 * p - 1.0
                } else {
                    3.0 - 4.0 * p
                }
            }
            1 => 1.0 - 2.0 * p,
            2 => 2.0 * p - 1.0,
            3 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            4 => (TAU * p).sin(),
            _ => self.held,
        }
    }

    fn next_random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Pitch-EG level 0..99 in semitones (50 = no change, ±48 at the ends).
pub fn pitch_level_semitones(l99: f32) -> f32 {
    (l99.clamp(0.0, 99.0) - 50.0) / 50.0 * 48.0
}

/// Pitch-EG rate 0..99 in semitones per second.
pub fn pitch_rate(r99: f32) -> f32 {
    0.25 * 2f32.powf(r99.clamp(0.0, 99.0) / 99.0 * 13.0)
}

/// The pitch envelope: starts at L4, moves through L1..L3, holds L3
/// while the key is down, returns to L4 on release.
#[derive(Clone, Copy, Debug)]
pub struct PitchEnv {
    semis: f32,
    stage: usize,
    down: bool,
}

impl Default for PitchEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl PitchEnv {
    pub fn new() -> Self {
        Self {
            semis: 0.0,
            stage: 4,
            down: false,
        }
    }

    pub fn start(&mut self, levels: &[f32; 4]) {
        self.semis = pitch_level_semitones(levels[3]);
        self.stage = 0;
        self.down = true;
    }

    pub fn release(&mut self) {
        self.down = false;
        self.stage = 3;
    }

    /// Advances by `seconds`; returns the pitch offset in semitones.
    pub fn tick(&mut self, rates: &[f32; 4], levels: &[f32; 4], seconds: f32) -> f32 {
        if self.stage < 3 || (self.stage == 3 && !self.down) {
            let target = pitch_level_semitones(levels[self.stage]);
            let step = pitch_rate(rates[self.stage]) * seconds;
            if (target - self.semis).abs() <= step {
                self.semis = target;
                self.stage += 1;
            } else {
                self.semis += step.copysign(target - self.semis);
            }
        }
        self.semis
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_waves_stay_in_range_and_have_their_shape() {
        for wave in 0..6 {
            let mut l = Lfo::new();
            for _ in 0..1000 {
                let v = l.tick(3.0, wave, 0.001);
                assert!((-1.0..=1.0).contains(&v), "wave {wave}: {v}");
            }
        }
        let mut sq = Lfo::new();
        assert_eq!(sq.tick(1.0, 3, 0.1), 1.0);
        assert_eq!(sq.tick(1.0, 3, 0.5), -1.0);
        let mut sh = Lfo::new();
        sh.tick(1.0, 5, 1.0);
        let held = sh.tick(1.0, 5, 0.1);
        assert_eq!(sh.tick(1.0, 5, 0.1), held, "S&H holds within a cycle");
    }

    #[test]
    fn lfo_speed_and_delay_maps() {
        assert!((lfo_hz(0.0) - 0.062).abs() < 1e-3);
        assert!(lfo_hz(99.0) > 45.0 && lfo_hz(99.0) < 60.0);
        assert!(lfo_hz(50.0) > lfo_hz(40.0));
        assert_eq!(lfo_delay_seconds(0.0), 0.0);
        assert_eq!(lfo_delay_gain(0.0, 0.0), 1.0);
        assert_eq!(lfo_delay_gain(0.1, 1.0), 0.0);
        assert_eq!(lfo_delay_gain(2.0, 1.0), 1.0);
    }

    #[test]
    fn flat_pitch_envelope_does_nothing() {
        let mut p = PitchEnv::new();
        let levels = [50.0; 4];
        p.start(&levels);
        for _ in 0..100 {
            assert_eq!(p.tick(&[99.0; 4], &levels, 0.01), 0.0);
        }
    }

    #[test]
    fn pitch_envelope_moves_from_l4_through_l1_and_releases() {
        let mut p = PitchEnv::new();
        let levels = [50.0, 50.0, 50.0, 75.0];
        p.start(&levels);
        let start = p.tick(&[99.0; 4], &levels, 0.0);
        assert!((start - 24.0).abs() < 0.01, "starts at L4: {start}");
        let later = (0..100)
            .map(|_| p.tick(&[99.0; 4], &levels, 0.01))
            .last()
            .unwrap();
        assert_eq!(later, 0.0, "sustains at L3");
        p.release();
        let released = (0..100)
            .map(|_| p.tick(&[99.0; 4], &levels, 0.01))
            .last()
            .unwrap();
        assert!((released - 24.0).abs() < 0.01, "releases to L4");
    }
}
