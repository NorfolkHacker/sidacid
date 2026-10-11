//! The 303-style bass synth: one SID chip, a filter envelope with Env Mod
//! and Decay, accent (level + short decay + the "accent wow" sweep that
//! builds over consecutive accents), slide, and 3-voice detuned unison.

use crate::params::{ParamId, SynthParam, exp, lin, on, step_index};
use sid_core::{FILTER_BP, FILTER_HP, FILTER_LP, SidChip, SyncMode, Waveform};

/// Pitch, cutoff and envelope update every this many samples.
const CONTROL_BLOCK: usize = 16;
const BASE_LEVEL: f32 = 0.35;
/// Accented notes use this fixed short filter decay, as on the 303.
const ACCENT_DECAY_S: f32 = 0.2;
/// Glide time constant; settles in about 60 ms.
const SLIDE_TAU_S: f32 = 0.02;
const ACCENT_CHARGE_TAU_S: f32 = 0.03;
const ACCENT_DISCHARGE_TAU_S: f32 = 0.15;
const ENV_MOD_OCTAVES: f32 = 4.0;
const ACCENT_OCTAVES: f32 = 3.0;
const LN_MINUS_60_DB: f32 = -6.907_755;

fn midi_to_hz(semis: f32) -> f32 {
    440.0 * 2f32.powf((semis - 69.0) / 12.0)
}

pub struct BassSynth {
    chip: SidChip,
    sample_rate: f32,
    tune: f32,
    cutoff_hz: f32,
    resonance: f32,
    env_mod: f32,
    decay_s: f32,
    accent_amt: f32,
    wave: Waveform,
    pulse_width: f32,
    sync: bool,
    sync_semis: f32,
    unison: bool,
    detune_cents: f32,
    spread: f32,
    volume: f32,
    gate: bool,
    accent: bool,
    note_semis: f32,
    target_semis: f32,
    filter_env: f32,
    accent_cap: f32,
    control_counter: usize,
}

impl BassSynth {
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            chip: SidChip::new(sample_rate),
            sample_rate,
            tune: 0.0,
            cutoff_hz: 1000.0,
            resonance: 0.0,
            env_mod: 0.0,
            decay_s: 0.3,
            accent_amt: 0.0,
            wave: Waveform::Saw,
            pulse_width: 0.5,
            sync: false,
            sync_semis: 0.0,
            unison: false,
            detune_cents: 0.0,
            spread: 0.0,
            volume: 0.7,
            gate: false,
            accent: false,
            note_semis: 45.0,
            target_semis: 45.0,
            filter_env: 0.0,
            accent_cap: 0.0,
            control_counter: 0,
        };
        for p in SynthParam::ALL {
            s.set_param(p, ParamId::Synth(0, p).info().default);
        }
        s.apply_voice_setup();
        s
    }

    pub fn chip(&self) -> &SidChip {
        &self.chip
    }

    pub fn filter_env(&self) -> f32 {
        self.filter_env
    }

    pub fn accent_cap(&self) -> f32 {
        self.accent_cap
    }

    pub fn gate_open(&self) -> bool {
        self.gate
    }

    pub fn set_param(&mut self, p: SynthParam, v: f32) {
        match p {
            SynthParam::Tune => self.tune = lin(v, -12.0, 12.0),
            SynthParam::Cutoff => self.cutoff_hz = exp(v, 20.0, 18_000.0),
            SynthParam::Resonance => self.resonance = v.clamp(0.0, 1.0),
            SynthParam::EnvMod => self.env_mod = v.clamp(0.0, 1.0),
            SynthParam::Decay => self.decay_s = exp(v, 0.03, 2.5),
            SynthParam::Accent => self.accent_amt = v.clamp(0.0, 1.0),
            SynthParam::Wave => {
                self.wave = if step_index(v, 2) == 0 {
                    Waveform::Saw
                } else {
                    Waveform::Pulse
                };
                self.apply_voice_setup();
            }
            SynthParam::PulseWidth => {
                self.pulse_width = lin(v, 0.05, 0.95);
                self.apply_voice_setup();
            }
            SynthParam::FilterMode => {
                let mode = [FILTER_LP, FILTER_BP, FILTER_HP][step_index(v, 3)];
                self.chip.set_filter_mode(mode);
            }
            SynthParam::Sync => {
                self.sync = on(v);
                self.apply_voice_setup();
            }
            SynthParam::SyncPitch => self.sync_semis = lin(v, 0.0, 24.0),
            SynthParam::Unison => {
                self.unison = on(v);
                self.apply_voice_setup();
            }
            SynthParam::Detune => self.detune_cents = lin(v, 0.0, 50.0),
            SynthParam::Spread => {
                self.spread = v.clamp(0.0, 1.0);
                self.apply_voice_setup();
            }
            SynthParam::Volume => {
                self.volume = v.clamp(0.0, 1.0);
                self.update_levels();
            }
        }
        self.control(false);
    }

    fn voice_count(&self) -> usize {
        if self.unison { 3 } else { 1 }
    }

    fn apply_voice_setup(&mut self) {
        let count = self.voice_count();
        for v in 0..SidChip::VOICES {
            self.chip.set_waveform(v, self.wave);
            self.chip.set_pulse_width(v, self.pulse_width);
            self.chip.set_filter_route(v, true);
            self.chip.set_sync(
                v,
                if self.sync {
                    SyncMode::Hidden
                } else {
                    SyncMode::Off
                },
            );
            self.chip.set_adsr(v, 0.003, 0.0, 1.0, 0.012);
            let pan = if self.unison {
                [-self.spread, 0.0, self.spread][v]
            } else {
                0.0
            };
            self.chip.set_pan(v, pan);
            if self.gate {
                if v < count {
                    self.chip.gate_on(v);
                } else {
                    self.chip.gate_off(v);
                }
            }
        }
        self.update_levels();
    }

    fn update_levels(&mut self) {
        let accent_gain = if self.accent {
            1.0 + self.accent_amt
        } else {
            1.0
        };
        let per_voice =
            BASE_LEVEL * self.volume * accent_gain / if self.unison { 1.6 } else { 1.0 };
        let count = self.voice_count();
        for v in 0..SidChip::VOICES {
            self.chip
                .set_level(v, if v < count { per_voice } else { 0.0 });
        }
    }

    pub fn note_on(&mut self, midi: u8, accent: bool, legato: bool) {
        self.target_semis = midi as f32;
        self.accent = accent;
        if !(legato && self.gate) {
            self.note_semis = self.target_semis;
            self.filter_env = 1.0;
            let count = self.voice_count();
            for v in 0..SidChip::VOICES {
                if v < count {
                    self.chip.gate_on(v);
                } else {
                    self.chip.gate_off(v);
                }
            }
        }
        self.gate = true;
        self.update_levels();
        self.control(false);
    }

    /// Drops all sound and internal state (chip, filter envelope, accent
    /// sweep, gate) but keeps the panel settings.
    pub fn reset(&mut self) {
        self.chip.reset();
        self.gate = false;
        self.accent = false;
        self.filter_env = 0.0;
        self.accent_cap = 0.0;
        self.note_semis = self.target_semis;
        self.control_counter = 0;
        self.control(false);
    }

    pub fn note_off(&mut self) {
        self.gate = false;
        for v in 0..SidChip::VOICES {
            self.chip.gate_off(v);
        }
    }

    /// Recomputes pitch and cutoff. With `advance`, first moves glide,
    /// filter envelope and accent sweep forward by one control block.
    fn control(&mut self, advance: bool) {
        if advance {
            let n = CONTROL_BLOCK as f32;
            let sr = self.sample_rate;
            let glide_k = 1.0 - (-n / (SLIDE_TAU_S * sr)).exp();
            self.note_semis += (self.target_semis - self.note_semis) * glide_k;
            let decay = if self.accent {
                ACCENT_DECAY_S
            } else {
                self.decay_s
            };
            self.filter_env *= (LN_MINUS_60_DB * n / (decay * sr)).exp();
            let input = if self.accent && self.gate {
                self.filter_env
            } else {
                0.0
            };
            let tau = if input > self.accent_cap {
                ACCENT_CHARGE_TAU_S
            } else {
                ACCENT_DISCHARGE_TAU_S
            };
            self.accent_cap += (input - self.accent_cap) * (1.0 - (-n / (tau * sr)).exp());
        }
        let octaves = self.env_mod * self.filter_env * ENV_MOD_OCTAVES
            + self.accent_amt * self.accent_cap * ACCENT_OCTAVES;
        let cutoff = (self.cutoff_hz * 2f32.powf(octaves)).clamp(20.0, 18_000.0);
        self.chip.set_filter(cutoff, self.resonance);
        let base = self.note_semis + self.tune;
        for v in 0..self.voice_count() {
            let detune = if self.unison {
                [-1.0, 0.0, 1.0][v] * self.detune_cents / 100.0
            } else {
                0.0
            };
            let hz = midi_to_hz(base + detune);
            if self.sync {
                self.chip.set_sync_master_hz(v, hz);
                self.chip
                    .set_freq_hz(v, hz * 2f32.powf(self.sync_semis / 12.0));
            } else {
                self.chip.set_freq_hz(v, hz);
            }
        }
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.control_counter == 0 {
                self.control(true);
                self.control_counter = CONTROL_BLOCK;
            }
            self.control_counter -= 1;
            let (a, b) = self.chip.render_frame();
            *l += a;
            *r += b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak, rms};

    const SR: f32 = 44_100.0;

    fn render(s: &mut BassSynth, seconds: f32) -> Vec<f32> {
        let n = (seconds * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        s.render(&mut l, &mut r);
        l
    }

    #[test]
    fn silent_without_notes() {
        let mut s = BassSynth::new(SR);
        assert_eq!(peak(&render(&mut s, 0.1)), 0.0);
    }

    #[test]
    fn note_sets_pitch_and_sounds() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false); // A2 = 110 Hz
        assert!((s.chip().freq_hz(0) - 110.0).abs() < 0.1);
        assert!(rms(&render(&mut s, 0.1)) > 0.01);
    }

    #[test]
    fn tune_shifts_pitch() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Tune, 1.0); // +12 semitones
        s.note_on(45, false, false);
        assert!((s.chip().freq_hz(0) - 220.0).abs() < 0.2);
    }

    #[test]
    fn note_off_releases_to_silence() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false);
        render(&mut s, 0.1);
        s.note_off();
        assert!(!s.gate_open());
        render(&mut s, 0.1);
        assert!(peak(&render(&mut s, 0.05)) < 1e-4);
    }

    #[test]
    fn reset_recovers_from_poisoned_state() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, true, false);
        render(&mut s, 0.05);
        s.filter_env = f32::NAN;
        s.accent_cap = f32::NAN;
        assert!(!all_finite(&render(&mut s, 0.01)), "state is poisoned");
        s.reset();
        assert!(!s.gate_open());
        assert_eq!(peak(&render(&mut s, 0.05)), 0.0, "silent after reset");
        s.note_on(45, false, false);
        let out = render(&mut s, 0.1);
        assert!(all_finite(&out) && rms(&out) > 0.01);
    }

    #[test]
    fn new_note_retriggers_filter_envelope() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false);
        render(&mut s, 0.3);
        assert!(s.filter_env() < 0.5);
        s.note_on(48, false, false);
        assert_eq!(s.filter_env(), 1.0);
    }

    #[test]
    fn legato_glides_without_retrigger() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false);
        render(&mut s, 0.3);
        let env = s.filter_env();
        s.note_on(57, false, true); // up an octave, sliding
        assert!(s.filter_env() <= env, "envelope not retriggered");
        render(&mut s, 0.005);
        let mid = s.chip().freq_hz(0);
        assert!(mid > 111.0 && mid < 215.0, "gliding, mid = {mid}");
        render(&mut s, 0.15);
        assert!((s.chip().freq_hz(0) - 220.0).abs() < 1.0);
    }

    #[test]
    fn legato_with_closed_gate_is_a_normal_note() {
        let mut s = BassSynth::new(SR);
        s.note_on(57, false, true);
        assert!((s.chip().freq_hz(0) - 220.0).abs() < 0.2);
        assert_eq!(s.filter_env(), 1.0);
    }

    #[test]
    fn accent_is_louder() {
        let mut plain = BassSynth::new(SR);
        let mut acc = BassSynth::new(SR);
        for s in [&mut plain, &mut acc] {
            s.set_param(SynthParam::Accent, 1.0);
        }
        plain.note_on(45, false, false);
        acc.note_on(45, true, false);
        let a = rms(&render(&mut acc, 0.1));
        let p = rms(&render(&mut plain, 0.1));
        assert!(a > 1.3 * p, "accent {a} vs plain {p}");
    }

    #[test]
    fn accent_sweep_builds_over_consecutive_accents() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Accent, 1.0);
        let peak_cap = |s: &mut BassSynth| {
            let mut m: f32 = 0.0;
            for _ in 0..(0.125 * SR) as usize / 64 {
                render_n(s, 64);
                m = m.max(s.accent_cap());
            }
            m
        };
        s.note_on(45, true, false);
        let first = peak_cap(&mut s);
        s.note_on(45, true, false);
        let second = peak_cap(&mut s);
        assert!(second > first * 1.05, "first {first}, second {second}");
    }

    fn render_n(s: &mut BassSynth, n: usize) {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        s.render(&mut l, &mut r);
    }

    #[test]
    fn unison_plays_three_detuned_voices() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Unison, 1.0);
        s.set_param(SynthParam::Detune, 1.0); // 50 cents
        s.note_on(45, false, false);
        let f: Vec<f32> = (0..3).map(|v| s.chip().freq_hz(v)).collect();
        assert!(f[0] < f[1] && f[1] < f[2], "{f:?}");
        assert!((f[1] - 110.0).abs() < 0.1);
        assert!((f[2] / f[1] - 2f32.powf(0.5 / 12.0)).abs() < 1e-3);
        assert!((0..3).all(|v| s.chip().voice(v).level > 0.0));
    }

    #[test]
    fn mono_mode_uses_one_voice() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false);
        assert!(s.chip().voice(0).level > 0.0);
        assert_eq!(s.chip().voice(1).level, 0.0);
        assert_eq!(s.chip().voice(2).level, 0.0);
    }

    #[test]
    fn unison_spread_is_stereo() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Unison, 1.0);
        s.set_param(SynthParam::Spread, 1.0);
        s.set_param(SynthParam::Detune, 1.0);
        s.note_on(45, false, false);
        let n = (0.2 * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        s.render(&mut l, &mut r);
        let diff: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
        assert!(rms(&diff) > 0.01);
    }

    #[test]
    fn sync_uses_hidden_master_at_note_pitch() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Sync, 1.0);
        s.set_param(SynthParam::SyncPitch, 0.5); // +12 st
        s.note_on(45, false, false);
        assert_eq!(s.chip().voice(0).sync, sid_core::SyncMode::Hidden);
        assert!((s.chip().voice(0).master.freq(SR) - 110.0).abs() < 0.1);
        assert!((s.chip().freq_hz(0) - 220.0).abs() < 0.2);
    }

    #[test]
    fn extreme_settings_stay_finite_and_bounded() {
        let mut s = BassSynth::new(SR);
        for p in SynthParam::ALL {
            s.set_param(p, 1.0);
        }
        s.note_on(30, true, false);
        let out = render(&mut s, 1.0);
        assert!(all_finite(&out));
        assert!(peak(&out) < 4.0);
    }
}
