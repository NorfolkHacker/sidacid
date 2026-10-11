//! A 3-voice SID chip: oscillator + envelope per voice, ring modulation and
//! hard sync from the previous voice, a per-voice hidden sync master (so
//! every voice can be synced at once), constant-power panning, and one
//! multimode filter run as a stereo pair with shared settings.

use crate::{EnvStage, Envelope, Oscillator, Svf, Waveform, FILTER_LP};
use std::f32::consts::FRAC_PI_4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncMode {
    #[default]
    Off,
    /// Reset when the previous voice ((v + 2) % 3) wraps, as on the SID.
    Classic,
    /// Reset when this voice's own silent `master` oscillator wraps.
    Hidden,
}

#[derive(Clone, Debug)]
pub struct Voice {
    pub osc: Oscillator,
    pub env: Envelope,
    pub master: Oscillator,
    pub ring: bool,
    pub sync: SyncMode,
    pub filter_route: bool,
    pub level: f32,
    gain_l: f32,
    gain_r: f32,
}

impl Voice {
    fn new(index: usize) -> Self {
        Self {
            osc: Oscillator::new(index as u32),
            env: Envelope::new(),
            master: Oscillator::new(index as u32 + 16),
            ring: false,
            sync: SyncMode::Off,
            filter_route: false,
            level: 1.0,
            gain_l: FRAC_PI_4.cos(),
            gain_r: FRAC_PI_4.sin(),
        }
    }
}

pub struct SidChip {
    sample_rate: f32,
    voices: [Voice; 3],
    filters: [Svf; 2],
}

impl SidChip {
    pub const VOICES: usize = 3;

    pub fn new(sample_rate: f32) -> Self {
        let mut filters = [Svf::new(sample_rate), Svf::new(sample_rate)];
        for f in &mut filters {
            f.mode = FILTER_LP;
        }
        Self { sample_rate, voices: std::array::from_fn(Voice::new), filters }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn voice(&self, v: usize) -> &Voice {
        &self.voices[v]
    }

    pub fn voice_mut(&mut self, v: usize) -> &mut Voice {
        &mut self.voices[v]
    }

    pub fn set_waveform(&mut self, v: usize, wave: Waveform) {
        self.voices[v].osc.waveform = wave;
    }

    pub fn set_freq_hz(&mut self, v: usize, hz: f32) {
        let sr = self.sample_rate;
        self.voices[v].osc.set_freq(hz, sr);
    }

    pub fn freq_hz(&self, v: usize) -> f32 {
        self.voices[v].osc.freq(self.sample_rate)
    }

    pub fn set_pulse_width(&mut self, v: usize, width: f32) {
        self.voices[v].osc.set_pulse_width(width);
    }

    pub fn set_adsr(&mut self, v: usize, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32) {
        let sr = self.sample_rate;
        self.voices[v].env.set_adsr(attack_s, decay_s, sustain, release_s, sr);
    }

    pub fn gate_on(&mut self, v: usize) {
        self.voices[v].env.gate_on();
    }

    pub fn gate_off(&mut self, v: usize) {
        self.voices[v].env.gate_off();
    }

    pub fn set_ring(&mut self, v: usize, on: bool) {
        self.voices[v].ring = on;
    }

    pub fn set_sync(&mut self, v: usize, mode: SyncMode) {
        self.voices[v].sync = mode;
    }

    pub fn set_sync_master_hz(&mut self, v: usize, hz: f32) {
        let sr = self.sample_rate;
        self.voices[v].master.set_freq(hz, sr);
    }

    pub fn set_filter_route(&mut self, v: usize, routed: bool) {
        self.voices[v].filter_route = routed;
    }

    /// Constant-power pan, -1 (left) .. 1 (right).
    pub fn set_pan(&mut self, v: usize, pan: f32) {
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        self.voices[v].gain_l = angle.cos();
        self.voices[v].gain_r = angle.sin();
    }

    pub fn set_level(&mut self, v: usize, level: f32) {
        self.voices[v].level = level;
    }

    pub fn set_filter(&mut self, cutoff_hz: f32, resonance: f32) {
        for f in &mut self.filters {
            f.set(cutoff_hz, resonance);
        }
    }

    pub fn set_filter_mode(&mut self, mode: u8) {
        for f in &mut self.filters {
            f.mode = mode;
        }
    }

    /// Silences the chip and clears all filter and oscillator state
    /// (settings are kept). Used to recover from a poisoned (NaN) state.
    pub fn reset(&mut self) {
        for v in &mut self.voices {
            v.env.stage = EnvStage::Off;
            v.env.level = 0.0;
            v.osc.reset_phase();
            v.master.reset_phase();
        }
        for f in &mut self.filters {
            f.reset();
        }
    }

    pub fn is_silent(&self) -> bool {
        self.voices.iter().all(|v| v.env.stage == EnvStage::Off)
    }

    pub fn render_frame(&mut self) -> (f32, f32) {
        let msbs = [self.voices[0].osc.msb(), self.voices[1].osc.msb(), self.voices[2].osc.msb()];
        let (mut fl, mut fr, mut dl, mut dr) = (0.0, 0.0, 0.0, 0.0);
        for v in 0..3 {
            let voice = &mut self.voices[v];
            let ring_msb = voice.ring && msbs[(v + 2) % 3];
            let s = voice.osc.sample(ring_msb) * voice.env.tick() * voice.level;
            if voice.filter_route {
                fl += s * voice.gain_l;
                fr += s * voice.gain_r;
            } else {
                dl += s * voice.gain_l;
                dr += s * voice.gain_r;
            }
        }
        let mut wrapped = [false; 3];
        for v in 0..3 {
            wrapped[v] = self.voices[v].osc.advance();
        }
        for v in 0..3 {
            match self.voices[v].sync {
                SyncMode::Off => {}
                SyncMode::Classic => {
                    if wrapped[(v + 2) % 3] {
                        self.voices[v].osc.reset_phase();
                    }
                }
                SyncMode::Hidden => {
                    if self.voices[v].master.advance() {
                        self.voices[v].osc.reset_phase();
                    }
                }
            }
        }
        (self.filters[0].process(fl) + dl, self.filters[1].process(fr) + dr)
    }

    pub fn render_add(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (a, b) = self.render_frame();
            *l += a;
            *r += b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;

    fn render(chip: &mut SidChip, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        chip.render_add(&mut l, &mut r);
        (l, r)
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    /// Voice v: instant attack, full sustain, short release, unfiltered.
    fn open_voice(chip: &mut SidChip, v: usize, wave: Waveform, hz: f32) {
        chip.set_waveform(v, wave);
        chip.set_freq_hz(v, hz);
        chip.set_adsr(v, 0.0, 0.0, 1.0, 0.005);
        chip.set_filter_route(v, false);
        chip.gate_on(v);
    }

    #[test]
    fn new_chip_is_silent() {
        let mut chip = SidChip::new(SR);
        assert!(chip.is_silent());
        let (l, r) = render(&mut chip, 512);
        assert_eq!(peak(&l) + peak(&r), 0.0);
    }

    #[test]
    fn gated_voice_sounds_and_release_silences() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Saw, 220.0);
        let (l, _) = render(&mut chip, 2205);
        assert!(peak(&l) > 0.5);
        chip.gate_off(0);
        render(&mut chip, 4410);
        assert!(chip.is_silent());
        let (l, _) = render(&mut chip, 512);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn freq_round_trips() {
        let mut chip = SidChip::new(SR);
        chip.set_freq_hz(1, 110.0);
        assert!((chip.freq_hz(1) - 110.0).abs() < 0.01);
    }

    #[test]
    fn centre_pan_is_equal_power() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Pulse, 100.0);
        let (l, r) = chip.render_frame();
        assert!((l - r).abs() < 1e-6);
        assert!((l.abs() - 0.7071).abs() < 1e-3);
    }

    #[test]
    fn hard_left_pan_silences_right() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Pulse, 100.0);
        chip.set_pan(0, -1.0);
        let (l, r) = render(&mut chip, 512);
        assert!(peak(&l) > 0.9);
        assert!(peak(&r) < 1e-6);
    }

    #[test]
    fn ring_mod_flips_triangle_using_previous_voice() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Triangle, 0.0);
        chip.voice_mut(0).osc.phase = 0x2000_0000; // triangle = -0.5
        chip.voice_mut(2).osc.phase = 0x8000_0000; // partner msb set (voice 2 is silent)
        let (plain, _) = chip.render_frame();
        chip.voice_mut(0).osc.phase = 0x2000_0000;
        chip.set_ring(0, true);
        let (ringed, _) = chip.render_frame();
        assert!((plain + 0.5 * 0.7071).abs() < 1e-3, "plain = {plain}");
        assert!((ringed - 0.5 * 0.7071).abs() < 1e-3, "ringed = {ringed}");
    }

    #[test]
    fn classic_sync_resets_when_previous_voice_wraps() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Saw, 100.0);
        open_voice(&mut chip, 1, Waveform::Saw, 370.0);
        chip.set_sync(1, SyncMode::Classic);
        let mut resets = 0;
        for _ in 0..SR as usize {
            let before = chip.voice(0).osc.phase;
            chip.render_frame();
            if chip.voice(0).osc.phase < before {
                assert_eq!(chip.voice(1).osc.phase, 0);
                resets += 1;
            }
        }
        assert!((99..=100).contains(&resets));
    }

    #[test]
    fn hidden_sync_resets_on_master_wrap() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Saw, 350.0);
        chip.set_sync(0, SyncMode::Hidden);
        chip.set_sync_master_hz(0, 100.0);
        let mut resets = 0;
        for _ in 0..SR as usize {
            let before = chip.voice(0).master.phase;
            chip.render_frame();
            if chip.voice(0).master.phase < before {
                assert_eq!(chip.voice(0).osc.phase, 0);
                resets += 1;
            }
        }
        assert!((99..=100).contains(&resets));
    }

    #[test]
    fn filter_route_applies_filter() {
        let mut dry = SidChip::new(SR);
        open_voice(&mut dry, 0, Waveform::Saw, 3000.0);
        let mut wet = SidChip::new(SR);
        open_voice(&mut wet, 0, Waveform::Saw, 3000.0);
        wet.set_filter_route(0, true);
        wet.set_filter_mode(crate::FILTER_LP);
        wet.set_filter(100.0, 0.0);
        let (d, _) = render(&mut dry, 4410);
        let (w, _) = render(&mut wet, 4410);
        assert!(peak(&w[2205..]) < 0.25 * peak(&d[2205..]));
    }

    #[test]
    fn level_scales_output() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Pulse, 100.0);
        chip.set_level(0, 0.5);
        let (l, _) = chip.render_frame();
        assert!((l.abs() - 0.5 * 0.7071).abs() < 1e-3);
    }
}
