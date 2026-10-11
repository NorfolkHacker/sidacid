//! ADSR envelope: linear attack, exponential decay and release (times are
//! to -60 dB). gate_on restarts the attack from the current level, so
//! retriggering never clicks.

/// Level treated as silence (-100 dB).
const SILENCE: f32 = 1.0e-5;
/// ln(0.001): an exponential that falls 60 dB.
const LN_MINUS_60_DB: f32 = -6.907_755;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvStage {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Debug)]
pub struct Envelope {
    pub stage: EnvStage,
    pub level: f32,
    attack_step: f32,
    decay_coef: f32,
    sustain: f32,
    release_coef: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-sample multiplier that falls 60 dB in `seconds` (0 = instant).
fn fall_coef(seconds: f32, sample_rate: f32) -> f32 {
    if seconds <= 0.0 { 0.0 } else { (LN_MINUS_60_DB / (seconds * sample_rate)).exp() }
}

impl Envelope {
    pub fn new() -> Self {
        Self { stage: EnvStage::Off, level: 0.0, attack_step: 1.0, decay_coef: 0.0, sustain: 1.0, release_coef: 0.0 }
    }

    pub fn set_adsr(&mut self, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32, sample_rate: f32) {
        self.attack_step = if attack_s <= 0.0 { 1.0 } else { 1.0 / (attack_s * sample_rate) };
        self.decay_coef = fall_coef(decay_s, sample_rate);
        self.sustain = sustain.clamp(0.0, 1.0);
        self.release_coef = fall_coef(release_s, sample_rate);
    }

    pub fn gate_on(&mut self) {
        self.stage = EnvStage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != EnvStage::Off {
            self.stage = EnvStage::Release;
        }
    }

    pub fn tick(&mut self) -> f32 {
        match self.stage {
            EnvStage::Off => self.level = 0.0,
            EnvStage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
            }
            EnvStage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_coef;
                if self.level - self.sustain <= SILENCE {
                    self.level = self.sustain;
                    self.stage = EnvStage::Sustain;
                }
            }
            EnvStage::Sustain => self.level = self.sustain,
            EnvStage::Release => {
                self.level *= self.release_coef;
                if self.level <= SILENCE {
                    self.level = 0.0;
                    self.stage = EnvStage::Off;
                }
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 1000.0;

    #[test]
    fn new_envelope_is_off_and_silent() {
        let mut e = Envelope::new();
        assert_eq!(e.stage, EnvStage::Off);
        assert_eq!(e.tick(), 0.0);
    }

    #[test]
    fn linear_attack_takes_attack_time() {
        let mut e = Envelope::new();
        e.set_adsr(0.008, 0.0, 1.0, 0.0, SR); // 8 samples, step 0.125 (exact in f32)
        e.gate_on();
        for _ in 0..7 {
            e.tick();
        }
        assert!(e.level < 1.0 && e.level > 0.85);
        e.tick();
        assert_eq!(e.level, 1.0);
        assert_eq!(e.stage, EnvStage::Decay);
    }

    #[test]
    fn decay_is_exponential_to_minus_60_db() {
        let mut e = Envelope::new();
        e.set_adsr(0.0, 0.1, 0.0, 0.0, SR);
        e.gate_on();
        e.tick(); // instant attack -> 1.0
        for _ in 0..50 {
            e.tick();
        }
        // Halfway through a -60 dB decay is -30 dB = 0.0316.
        assert!((e.level - 0.0316).abs() < 0.003, "level = {}", e.level);
        for _ in 0..50 {
            e.tick();
        }
        assert!((e.level - 0.001).abs() < 0.0003, "level = {}", e.level);
    }

    #[test]
    fn decay_settles_on_sustain() {
        let mut e = Envelope::new();
        e.set_adsr(0.0, 0.01, 0.5, 0.0, SR);
        e.gate_on();
        for _ in 0..200 {
            e.tick();
        }
        assert_eq!(e.stage, EnvStage::Sustain);
        assert_eq!(e.level, 0.5);
    }

    #[test]
    fn release_reaches_off() {
        let mut e = Envelope::new();
        e.set_adsr(0.0, 0.0, 1.0, 0.05, SR);
        e.gate_on();
        e.tick();
        e.gate_off();
        assert_eq!(e.stage, EnvStage::Release);
        for _ in 0..200 {
            e.tick();
        }
        assert_eq!(e.stage, EnvStage::Off);
        assert_eq!(e.level, 0.0);
    }

    #[test]
    fn gate_off_when_off_stays_off() {
        let mut e = Envelope::new();
        e.gate_off();
        assert_eq!(e.stage, EnvStage::Off);
    }

    #[test]
    fn retrigger_attacks_from_current_level() {
        let mut e = Envelope::new();
        e.set_adsr(0.010, 0.0, 1.0, 0.1, SR);
        e.gate_on();
        for _ in 0..20 {
            e.tick();
        }
        e.gate_off();
        for _ in 0..10 {
            e.tick();
        }
        let before = e.level;
        assert!(before > 0.1 && before < 1.0);
        e.gate_on();
        e.tick();
        assert!(e.level > before, "attack continues upward from {before}");
    }
}
