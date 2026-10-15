//! The Mono synth's one envelope: linear attack, exponential (RC-style)
//! decay and release. A retrigger restarts the attack from the current
//! level, so it never clicks down to zero.

/// Decay and release cover 99% of their way in their set time.
const LN_100: f32 = 4.605_17;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub struct Adsr {
    stage: Stage,
    level: f32,
    attack_step: f32,
    decay_coef: f32,
    release_coef: f32,
    sustain: f32,
    sample_rate: f32,
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        let mut e = Self {
            stage: Stage::Idle,
            level: 0.0,
            attack_step: 0.0,
            decay_coef: 0.0,
            release_coef: 0.0,
            sustain: 0.0,
            sample_rate,
        };
        e.set(0.005, 0.3, 0.6, 0.2);
        e
    }

    pub fn set(&mut self, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32) {
        let sr = self.sample_rate;
        self.attack_step = 1.0 / (attack_s.max(1e-4) * sr);
        self.decay_coef = (-LN_100 / (decay_s.max(1e-4) * sr)).exp();
        self.release_coef = (-LN_100 / (release_s.max(1e-4) * sr)).exp();
        self.sustain = sustain.clamp(0.0, 1.0);
    }

    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_coef;
                if (self.level - self.sustain).abs() < 1e-4 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            // Follows Sustain knob moves while held.
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level *= self.release_coef;
                if self.level < 1e-5 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn run(e: &mut Adsr, n: usize) -> f32 {
        let mut v = 0.0;
        for _ in 0..n {
            v = e.next();
        }
        v
    }

    #[test]
    fn stages_take_their_times() {
        let mut e = Adsr::new(SR);
        e.set(0.01, 0.1, 0.5, 0.1);
        e.gate_on();
        assert!((run(&mut e, 240) - 0.5).abs() < 0.01, "half way up the attack");
        run(&mut e, 240);
        assert_eq!(e.stage(), Stage::Decay);
        let after_decay = run(&mut e, 4800);
        assert!(after_decay - 0.5 <= 0.5 * 0.011, "{after_decay}");
        run(&mut e, 4800);
        assert_eq!(e.stage(), Stage::Sustain);
        e.gate_off();
        assert!(run(&mut e, 4800) <= 0.5 * 0.011);
    }

    #[test]
    fn retriggering_starts_the_attack_from_the_current_level() {
        let mut e = Adsr::new(SR);
        e.set(0.05, 0.1, 0.5, 1.0);
        e.gate_on();
        run(&mut e, 48_000);
        e.gate_off();
        let before = run(&mut e, 4800);
        e.gate_on();
        assert!(e.next() >= before, "no drop to zero (no click)");
        assert_eq!(e.stage(), Stage::Attack);
    }

    #[test]
    fn it_ends_idle_after_release() {
        let mut e = Adsr::new(SR);
        e.set(0.001, 0.01, 0.5, 0.01);
        e.gate_on();
        run(&mut e, 4800);
        e.gate_off();
        assert_eq!(run(&mut e, 48_000), 0.0);
        assert_eq!(e.stage(), Stage::Idle);
    }
}
