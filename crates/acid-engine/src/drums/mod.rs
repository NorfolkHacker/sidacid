//! SID drum machines. Each drum is a `DrumPatch`: a small program that
//! sweeps an oscillator's pitch (optionally switching waveform for an
//! attack click, ring-modding or adding a second oscillator) and mixes in
//! filtered LFSR noise with an optional multi-burst (clap) envelope. A kit
//! turns its 4 knobs per instrument into patches and plays them on a pool
//! of 6 voices, each with its own filter.

pub mod kit808;
pub mod kit909;

use crate::params::{DrumParam, KitParam, DRUM_INSTRUMENTS};
use sid_core::{Oscillator, Svf, Waveform, FILTER_HP};

pub const BD: usize = 0;
pub const SD: usize = 1;
pub const LT: usize = 2;
pub const HT: usize = 3;
pub const CP: usize = 4;
pub const RS: usize = 5;
pub const CH: usize = 6;
pub const OH: usize = 7;
pub const POOL_SIZE: usize = 6;

const LN_MINUS_60_DB: f32 = -6.907_755;
const END_LEVEL: f32 = 1.0e-4;
const CHOKE_TAU_S: f32 = 0.005;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Osc2Mode {
    #[default]
    Off,
    /// Oscillator 1 (triangle) ring-modulated by oscillator 2.
    Ring,
    /// Oscillator 2 (same waveform) mixed with oscillator 1.
    Add,
}

#[derive(Clone, Copy, Debug)]
pub struct DrumPatch {
    pub tone_wave: Waveform,
    pub tone_start_hz: f32,
    pub tone_end_hz: f32,
    /// Time constant of the exponential pitch drop.
    pub pitch_decay_s: f32,
    /// Tone amplitude time to -60 dB.
    pub tone_decay_s: f32,
    pub tone_level: f32,
    pub pulse_width: f32,
    /// Waveform used for the first `click_s` seconds.
    pub click_wave: Option<Waveform>,
    pub click_s: f32,
    pub osc2_mode: Osc2Mode,
    pub osc2_ratio: f32,
    pub noise_level: f32,
    /// Noise tail time to -60 dB (after any bursts).
    pub noise_decay_s: f32,
    /// LFSR clocks per sample, 0..1.
    pub noise_rate: f32,
    pub noise_filter_mode: u8,
    pub noise_cutoff_hz: f32,
    pub noise_res: f32,
    pub bursts: u8,
    pub burst_spacing_s: f32,
}

impl Default for DrumPatch {
    fn default() -> Self {
        Self {
            tone_wave: Waveform::Triangle,
            tone_start_hz: 100.0,
            tone_end_hz: 100.0,
            pitch_decay_s: 0.01,
            tone_decay_s: 0.1,
            tone_level: 0.0,
            pulse_width: 0.5,
            click_wave: None,
            click_s: 0.0,
            osc2_mode: Osc2Mode::Off,
            osc2_ratio: 1.0,
            noise_level: 0.0,
            noise_decay_s: 0.1,
            noise_rate: 1.0,
            noise_filter_mode: FILTER_HP,
            noise_cutoff_hz: 1000.0,
            noise_res: 0.0,
            bursts: 0,
            burst_spacing_s: 0.01,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KitKind {
    K808,
    K909,
}

impl KitKind {
    pub fn patch(self, inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch {
        match self {
            KitKind::K808 => kit808::patch(inst, tune, decay, tone),
            KitKind::K909 => kit909::patch(inst, tune, decay, tone),
        }
    }
}

struct DrumVoice {
    active: bool,
    inst: usize,
    age: u64,
    patch: DrumPatch,
    osc1: Oscillator,
    osc2: Oscillator,
    noise: Oscillator,
    filter: Svf,
    sample_rate: f32,
    t: f32,
    gain: f32,
    pitch_env: f32,
    pitch_coef: f32,
    tone_amp: f32,
    tone_coef: f32,
    noise_amp: f32,
    noise_coef: f32,
    choking: bool,
    choke: f32,
    choke_coef: f32,
}

impl DrumVoice {
    fn new(sample_rate: f32, seed: u32) -> Self {
        let mut noise = Oscillator::new(seed * 7 + 3);
        noise.waveform = Waveform::Noise;
        Self {
            active: false,
            inst: 0,
            age: 0,
            patch: DrumPatch::default(),
            osc1: Oscillator::new(seed),
            osc2: Oscillator::new(seed + 1),
            noise,
            filter: Svf::new(sample_rate),
            sample_rate,
            t: 0.0,
            gain: 0.0,
            pitch_env: 1.0,
            pitch_coef: 0.0,
            tone_amp: 1.0,
            tone_coef: 0.0,
            noise_amp: 1.0,
            noise_coef: 0.0,
            choking: false,
            choke: 1.0,
            choke_coef: (-1.0 / (CHOKE_TAU_S * sample_rate)).exp(),
        }
    }

    fn start(&mut self, inst: usize, patch: DrumPatch, gain: f32, age: u64) {
        let sr = self.sample_rate;
        self.active = true;
        self.inst = inst;
        self.age = age;
        self.patch = patch;
        self.gain = gain;
        self.t = 0.0;
        self.pitch_env = 1.0;
        self.pitch_coef = (-1.0 / (patch.pitch_decay_s.max(1.0e-4) * sr)).exp();
        self.tone_amp = 1.0;
        self.tone_coef = (LN_MINUS_60_DB / (patch.tone_decay_s.max(1.0e-3) * sr)).exp();
        self.noise_amp = 1.0;
        self.noise_coef = (LN_MINUS_60_DB / (patch.noise_decay_s.max(1.0e-3) * sr)).exp();
        self.choking = false;
        self.choke = 1.0;
        self.osc1.reset_phase();
        self.osc2.reset_phase();
        self.osc1.set_pulse_width(patch.pulse_width);
        self.osc2.set_pulse_width(patch.pulse_width);
        self.osc2.waveform = patch.tone_wave;
        self.noise.set_freq(patch.noise_rate.clamp(0.0, 1.0) * sr / 16.0, sr);
        self.filter.reset();
        self.filter.mode = patch.noise_filter_mode;
        self.filter.set(patch.noise_cutoff_hz, patch.noise_res);
    }

    fn tick(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }
        let p = self.patch;
        let sr = self.sample_rate;

        let hz = p.tone_end_hz + (p.tone_start_hz - p.tone_end_hz) * self.pitch_env;
        self.pitch_env *= self.pitch_coef;
        self.osc1.waveform = match p.click_wave {
            Some(w) if self.t < p.click_s => w,
            _ => p.tone_wave,
        };
        self.osc1.set_freq(hz, sr);
        let raw_tone = match p.osc2_mode {
            Osc2Mode::Off => self.osc1.sample(false),
            Osc2Mode::Ring => {
                self.osc2.set_freq(hz * p.osc2_ratio, sr);
                self.osc1.sample(self.osc2.msb())
            }
            Osc2Mode::Add => {
                self.osc2.set_freq(hz * p.osc2_ratio, sr);
                0.5 * (self.osc1.sample(false) + self.osc2.sample(false))
            }
        };
        self.osc1.advance();
        self.osc2.advance();
        let tone = raw_tone * self.tone_amp * p.tone_level;
        self.tone_amp *= self.tone_coef;

        let burst_total = p.bursts as f32 * p.burst_spacing_s;
        let noise_env = if self.t < burst_total {
            let phase = self.t % p.burst_spacing_s;
            (-phase / (p.burst_spacing_s / 3.0)).exp()
        } else {
            let a = self.noise_amp;
            self.noise_amp *= self.noise_coef;
            a
        };
        let n = self.noise.sample(false);
        self.noise.advance();
        let noise = self.filter.process(n) * noise_env * p.noise_level;

        self.t += 1.0 / sr;
        if self.choking {
            self.choke *= self.choke_coef;
        }
        let out = (tone + noise) * self.gain * self.choke;

        let tone_done = self.tone_amp * p.tone_level < END_LEVEL;
        let noise_done = self.t > burst_total && noise_env * p.noise_level < END_LEVEL;
        if (tone_done && noise_done) || self.choke < END_LEVEL {
            self.active = false;
        }
        out
    }
}

pub struct DrumKit {
    kind: KitKind,
    voices: Vec<DrumVoice>,
    /// Normalised Tune, Decay, Tone, Level per instrument.
    knobs: [[f32; 4]; DRUM_INSTRUMENTS],
    volume: f32,
    accent_amt: f32,
    age: u64,
}

impl DrumKit {
    pub fn new(kind: KitKind, sample_rate: f32) -> Self {
        Self {
            kind,
            voices: (0..POOL_SIZE as u32).map(|i| DrumVoice::new(sample_rate, i * 3)).collect(),
            knobs: [[0.5, 0.5, 0.5, 0.8]; DRUM_INSTRUMENTS],
            volume: 0.7,
            accent_amt: 0.5,
            age: 0,
        }
    }

    pub fn set_param(&mut self, inst: usize, p: DrumParam, v: f32) {
        self.knobs[inst][p as usize] = v.clamp(0.0, 1.0);
    }

    pub fn set_kit_param(&mut self, p: KitParam, v: f32) {
        match p {
            KitParam::Volume => self.volume = v.clamp(0.0, 1.0),
            KitParam::Accent => self.accent_amt = v.clamp(0.0, 1.0),
        }
    }

    pub fn trigger(&mut self, inst: usize, accent: bool) {
        if inst == CH {
            for v in self.voices.iter_mut().filter(|v| v.active && v.inst == OH) {
                v.choking = true;
            }
        }
        let idx = self
            .voices
            .iter()
            .position(|v| v.active && v.inst == inst)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .unwrap_or_else(|| {
                (0..self.voices.len()).min_by_key(|&i| self.voices[i].age).expect("pool is not empty")
            });
        let [tune, decay, tone, level] = self.knobs[inst];
        let patch = self.kind.patch(inst, tune, decay, tone);
        let gain = level * level * if accent { 1.0 + self.accent_amt } else { 1.0 };
        self.age += 1;
        self.voices[idx].start(inst, patch, gain, self.age);
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn is_playing(&self, inst: usize) -> bool {
        self.voices.iter().any(|v| v.active && v.inst == inst)
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let master = self.volume * self.volume * 1.5;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut s = 0.0;
            for v in &mut self.voices {
                s += v.tick();
            }
            s *= master;
            *l += s;
            *r += s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 44_100.0;

    fn render(k: &mut DrumKit, seconds: f32) -> Vec<f32> {
        let n = (seconds * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        k.render(&mut l, &mut r);
        l
    }

    #[test]
    fn every_instrument_sounds_and_ends() {
        for kind in [KitKind::K808, KitKind::K909] {
            for inst in 0..DRUM_INSTRUMENTS {
                let mut k = DrumKit::new(kind, SR);
                k.set_param(inst, DrumParam::Decay, 1.0); // longest decay
                k.trigger(inst, false);
                let out = render(&mut k, 3.0);
                assert!(all_finite(&out), "{kind:?} {inst}");
                assert!(peak(&out) > 0.01, "{kind:?} {inst} is silent");
                assert!(peak(&out) < 2.0, "{kind:?} {inst} too loud");
                assert_eq!(k.active_voices(), 0, "{kind:?} {inst} never ends");
            }
        }
    }

    #[test]
    fn retrigger_reuses_the_instruments_voice() {
        let mut k = DrumKit::new(KitKind::K808, SR);
        k.trigger(BD, false);
        render(&mut k, 0.01);
        k.trigger(BD, false);
        assert_eq!(k.active_voices(), 1);
    }

    #[test]
    fn pool_is_limited_to_six_voices() {
        let mut k = DrumKit::new(KitKind::K808, SR);
        for inst in 0..DRUM_INSTRUMENTS {
            k.trigger(inst, false);
        }
        assert_eq!(k.active_voices(), POOL_SIZE);
    }

    #[test]
    fn closed_hat_chokes_open_hat() {
        let mut k = DrumKit::new(KitKind::K808, SR);
        k.set_param(OH, DrumParam::Decay, 1.0);
        k.trigger(OH, false);
        render(&mut k, 0.05);
        assert!(k.is_playing(OH));
        k.trigger(CH, false);
        render(&mut k, 0.1);
        assert!(!k.is_playing(OH));
    }

    #[test]
    fn accent_is_louder() {
        let mut a = DrumKit::new(KitKind::K808, SR);
        let mut p = DrumKit::new(KitKind::K808, SR);
        a.set_kit_param(KitParam::Accent, 1.0);
        p.set_kit_param(KitParam::Accent, 1.0);
        a.trigger(SD, true);
        p.trigger(SD, false);
        assert!(peak(&render(&mut a, 0.2)) > 1.5 * peak(&render(&mut p, 0.2)));
    }

    #[test]
    fn level_knob_zero_is_silent() {
        let mut k = DrumKit::new(KitKind::K808, SR);
        k.set_param(BD, DrumParam::Level, 0.0);
        k.trigger(BD, false);
        assert_eq!(peak(&render(&mut k, 0.2)), 0.0);
    }

    #[test]
    fn tune_raises_kick_pitch() {
        let low = kit808::patch(BD, 0.0, 0.5, 0.5);
        let high = kit808::patch(BD, 1.0, 0.5, 0.5);
        assert!(high.tone_end_hz > 3.0 * low.tone_end_hz);
    }

    #[test]
    fn clap_uses_bursts() {
        let p = kit808::patch(CP, 0.5, 0.5, 0.5);
        assert!(p.bursts >= 3);
        assert!(p.noise_level > 0.0);
    }
}
