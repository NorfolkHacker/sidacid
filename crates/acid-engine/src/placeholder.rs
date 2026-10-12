//! Stand-in voice for the FM and sampler slots until their real engines
//! arrive: 8 sine voices with a 5 ms attack and 80 ms release. Real-time
//! safe: fixed voice array, no allocation.

use std::f32::consts::TAU;

const VOICES: usize = 8;
const ATTACK_S: f32 = 0.005;
const RELEASE_S: f32 = 0.08;
/// Per-voice peak before volume, so a full chord stays below clipping.
const VOICE_GAIN: f32 = 0.3;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    held: bool,
    pitch: u8,
    phase: f32,
    inc: f32,
    amp: f32,
    env: f32,
    /// Start order, for stealing the oldest voice.
    age: u32,
}

pub struct SineVoices {
    voices: [Voice; VOICES],
    sample_rate: f32,
    attack_step: f32,
    release_step: f32,
    volume: f32,
    clock: u32,
}

impl SineVoices {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: [Voice::default(); VOICES],
            sample_rate,
            attack_step: 1.0 / (ATTACK_S * sample_rate),
            release_step: 1.0 / (RELEASE_S * sample_rate),
            volume: 0.49,
            clock: 0,
        }
    }

    /// Knob 0..1, squared like the other volume knobs.
    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0).powi(2);
    }

    pub fn note_on(&mut self, pitch: u8, vel: u8) {
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        let i = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| {
                (0..VOICES)
                    .max_by_key(|&i| clock.wrapping_sub(self.voices[i].age))
                    .unwrap_or(0)
            });
        let freq = 440.0 * 2f32.powf((pitch.min(127) as f32 - 69.0) / 12.0);
        self.voices[i] = Voice {
            active: true,
            held: true,
            pitch,
            phase: 0.0,
            inc: freq / self.sample_rate,
            amp: vel.min(127) as f32 / 127.0 * VOICE_GAIN,
            env: 0.0,
            age: clock,
        };
    }

    pub fn note_off(&mut self, pitch: u8) {
        for v in &mut self.voices {
            if v.active && v.held && v.pitch == pitch {
                v.held = false;
            }
        }
    }

    /// Releases every voice; the tails ring out.
    pub fn all_off(&mut self) {
        for v in &mut self.voices {
            v.held = false;
        }
    }

    /// Silences everything immediately.
    pub fn reset(&mut self) {
        self.voices = [Voice::default(); VOICES];
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Adds into `left`/`right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (attack, release, volume) = (self.attack_step, self.release_step, self.volume);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                if v.held {
                    v.env = (v.env + attack).min(1.0);
                } else {
                    v.env -= release;
                    if v.env <= 0.0 {
                        v.active = false;
                        break;
                    }
                }
                let s = (v.phase * TAU).sin() * v.amp * v.env * volume;
                *l += s;
                *r += s;
                v.phase += v.inc;
                v.phase -= v.phase.floor();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 48_000.0;

    fn run(v: &mut SineVoices, n: usize) -> Vec<f32> {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        v.render(&mut l, &mut r);
        assert_eq!(l, r, "mono placeholder");
        l
    }

    #[test]
    fn a_note_sounds_and_its_release_dies() {
        let mut v = SineVoices::new(SR);
        v.note_on(69, 127);
        let l = run(&mut v, 4800);
        assert!(peak(&l) > 0.1 && peak(&l) <= 1.0 && all_finite(&l));
        v.note_off(69);
        run(&mut v, 4800);
        assert_eq!(v.active_voices(), 0);
        assert_eq!(peak(&run(&mut v, 100)), 0.0);
    }

    #[test]
    fn chords_use_voices_and_the_ninth_steals_the_oldest() {
        let mut v = SineVoices::new(SR);
        for p in 60..68 {
            v.note_on(p, 100);
        }
        assert_eq!(v.active_voices(), 8);
        v.note_on(80, 100);
        assert_eq!(v.active_voices(), 8);
        v.note_off(60);
        run(&mut v, 4800);
        assert_eq!(
            v.active_voices(),
            8,
            "pitch 60 was stolen, so nothing released"
        );
        v.all_off();
        run(&mut v, 4800);
        assert_eq!(v.active_voices(), 0);
    }

    #[test]
    fn volume_zero_is_silent_and_reset_is_immediate() {
        let mut v = SineVoices::new(SR);
        v.set_volume(0.0);
        v.note_on(60, 127);
        assert_eq!(peak(&run(&mut v, 480)), 0.0);
        v.set_volume(1.0);
        v.reset();
        assert_eq!(v.active_voices(), 0);
    }
}
