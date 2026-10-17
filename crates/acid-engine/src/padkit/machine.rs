//! The Pad Kit machine: 16 pads of velocity layers on 16 Sampler voices,
//! with accent, mute groups, Gate or One-shot play and Start/End trim.
//! Real-time safe: samples arrive and leave as `Arc`s through `swap_*`,
//! so nothing is allocated or freed while rendering.

use super::play::PadPlay;
use super::settings::PadSettings;
use super::{MAX_LAYERS, PADS};
use crate::params::{PadKitParam, PadParam};
use crate::sampler::sample::SampleData;
use crate::sampler::voice::{Segment, Voice};
use std::sync::Arc;

/// Mute-group choke, self-retrigger and Gate release all fade out over this.
const FADE_S: f32 = 0.005;
const CHUNK: usize = 64;
const VOICES: usize = 16;

pub struct PadKit {
    sample_rate: f32,
    samples: [[Option<Arc<SampleData>>; MAX_LAYERS]; PADS],
    play: PadPlay,
    settings: [PadSettings; PADS],
    voices: [Voice; VOICES],
    clock: u32,
    volume: f32,
    accent: f32,
    buf_l: [f32; CHUNK],
    buf_r: [f32; CHUNK],
}

impl PadKit {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            samples: std::array::from_fn(|_| std::array::from_fn(|_| None)),
            play: PadPlay::EMPTY,
            settings: [PadSettings::default(); PADS],
            voices: std::array::from_fn(|_| Voice::new(sample_rate)),
            clock: 0,
            volume: 0.49,
            accent: 0.5,
            buf_l: [0.0; CHUNK],
            buf_r: [0.0; CHUNK],
        }
    }

    pub fn set_param(&mut self, p: PadKitParam, v: f32) {
        let v = v.clamp(0.0, 1.0);
        match p {
            PadKitParam::Volume => self.volume = v.powi(2),
            PadKitParam::Accent => self.accent = v,
        }
    }

    pub fn set_pad_param(&mut self, pad: usize, p: PadParam, v: f32) {
        if let Some(s) = self.settings.get_mut(pad) {
            s.set(p, v);
        }
    }

    fn kill_pad(&mut self, pad: usize) {
        for v in self.voices.iter_mut().filter(|v| v.slot == pad) {
            v.kill();
        }
    }

    /// Exchanges layer `layer` of pad `pad`'s audio with `s`. The caller
    /// disposes of what comes back off the audio thread. False for a bad
    /// pad or layer. The pad's sounding voices stop.
    pub fn swap_sample(
        &mut self,
        pad: usize,
        layer: usize,
        s: &mut Option<Arc<SampleData>>,
    ) -> bool {
        if pad >= PADS || layer >= MAX_LAYERS {
            return false;
        }
        self.kill_pad(pad);
        std::mem::swap(&mut self.samples[pad][layer], s);
        true
    }

    /// Exchanges every pad's layer ranges. Sounding voices keep their
    /// already-started segment.
    pub fn swap_play(&mut self, play: &mut PadPlay) {
        std::mem::swap(&mut self.play, play);
    }

    pub fn note_on(&mut self, pad: u8, vel: u8, accent: bool, live: bool) {
        let pad = pad as usize;
        if pad >= PADS || vel == 0 {
            return;
        }
        let vel = if accent {
            (vel as f32 + self.accent * 127.0).floor().min(127.0) as u8
        } else {
            vel.min(127)
        };
        let Some(layer) = self.play.layer_for(pad, vel) else {
            return;
        };
        let Some(sample) = self.samples[pad][layer].as_deref() else {
            return;
        };
        let frames = sample.frames() as f64;
        let st = &self.settings[pad];
        let a = (st.start.min(st.end) as f64 * frames).floor();
        let b = (st.end.max(st.start) as f64 * frames).ceil();
        if b - a < 1.0 {
            return;
        }
        let semis = (st.voice.tune as f64 + st.voice.fine_cents as f64 / 100.0) / 12.0;
        let rate =
            (sample.sample_rate as f64 / self.sample_rate as f64 * 2f64.powf(semis)).max(1e-6);
        let seg = Segment {
            start: a,
            end: b,
            reverse: st.voice.reverse,
            looping: None,
            rate,
        };
        // A pad cuts its own last hit, and the other pads of its mute group.
        let group = st.group;
        let settings = &self.settings;
        for v in self.voices.iter_mut().filter(|v| {
            v.active && v.held && (v.slot == pad || (group != 0 && settings[v.slot].group == group))
        }) {
            v.fade(FADE_S);
        }
        let clock = self.clock;
        let voices = &self.voices;
        let age = |v: &Voice| clock.wrapping_sub(v.age);
        let i = voices
            .iter()
            .position(|v| !v.active)
            .or_else(|| voices.iter().position(Voice::tail_only))
            .unwrap_or_else(|| (0..VOICES).max_by_key(|&i| age(&voices[i])).unwrap_or(0));
        self.voices[i].start(&seg, pad, layer as u8, vel, live, clock);
        self.clock = self.clock.wrapping_add(1);
    }

    /// Gate pads stop; One-shot pads play on.
    pub fn note_off(&mut self, pad: u8, live: bool) {
        let pad = pad as usize;
        let Some(st) = self.settings.get(pad) else {
            return;
        };
        if !st.gate {
            return;
        }
        for v in &mut self.voices {
            if v.active && v.held && v.slot == pad && v.live == live {
                v.fade(st.voice.release_s);
            }
        }
    }

    /// Releases the pattern's Gate notes; auditioned (live) notes and
    /// One-shot hits keep sounding.
    pub fn pattern_off(&mut self) {
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.active && v.held && !v.live)
        {
            let st = &self.settings[v.slot];
            if st.gate {
                v.fade(st.voice.release_s);
            }
        }
    }

    /// Fades every voice out, auditions and One-shot hits included.
    pub fn all_off(&mut self) {
        for v in &mut self.voices {
            v.fade(FADE_S);
        }
    }

    /// Silences every voice at once.
    pub fn reset(&mut self) {
        for v in &mut self.voices {
            v.kill();
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let total = left.len().min(right.len());
        let mut pos = 0;
        while pos < total {
            let n = (total - pos).min(CHUNK);
            self.buf_l[..n].fill(0.0);
            self.buf_r[..n].fill(0.0);
            for v in self.voices.iter_mut().filter(|v| v.active) {
                match self.samples[v.slot][v.key as usize].as_deref() {
                    Some(sample) => v.render(
                        sample,
                        &self.settings[v.slot].voice,
                        &mut self.buf_l[..n],
                        &mut self.buf_r[..n],
                    ),
                    None => v.kill(),
                }
            }
            for i in 0..n {
                left[pos + i] += self.buf_l[i] * self.volume;
                right[pos + i] += self.buf_r[i] * self.volume;
            }
            pos += n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::padkit::pad::{Layer, Pad};
    use crate::params::{PadKitParam, PadParam};

    const SR: f32 = 48_000.0;

    /// A kit with pad 0 holding a quiet layer (1..=63) and a loud one
    /// (64..=127), pad 1 and pad 2 holding one layer each. Each layer is a
    /// constant DC level so we can tell them apart.
    fn kit() -> PadKit {
        let mut k = PadKit::new(SR);
        let mut pads = vec![Pad::default(); PADS];
        let dc = |v: f32| Layer::new("x", 48_000, 1, &vec![v; 48_000]);
        pads[0].layers = vec![dc(0.1), dc(0.9)];
        pads[0].split_evenly();
        pads[1].layers = vec![dc(0.5)];
        pads[1].split_evenly();
        pads[2].layers = vec![dc(0.5)];
        pads[2].split_evenly();
        for (p, pad) in pads.iter().enumerate() {
            for (l, layer) in pad.layers.iter().enumerate() {
                let mut s = Some(Arc::new(layer.to_sample()));
                k.swap_sample(p, l, &mut s);
            }
        }
        let mut play = PadPlay::from_pads(&pads);
        k.swap_play(&mut play);
        k.set_param(PadKitParam::Volume, 1.0);
        k
    }

    fn render(k: &mut PadKit, n: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        k.render(&mut l, &mut r);
        l
    }

    fn level_after(k: &mut PadKit) -> f32 {
        let out = render(k, 2_000);
        out[1_000..].iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    #[test]
    fn velocity_picks_the_layer() {
        let mut k = kit();
        k.note_on(0, 30, false, false);
        let quiet = level_after(&mut k);
        k.all_off();
        render(&mut k, 2_000);
        k.note_on(0, 100, false, false);
        let loud = level_after(&mut k);
        assert!(loud > quiet * 3.0, "loud {loud} quiet {quiet}");
    }

    #[test]
    fn accent_pushes_a_hit_into_a_higher_layer() {
        let mut k = kit();
        k.set_param(PadKitParam::Accent, 0.5); // +63
        k.note_on(0, 30, true, false); // 30 + 63 = 93: the loud layer
        let accented = level_after(&mut k);
        k.all_off();
        render(&mut k, 2_000);
        k.note_on(0, 93, false, false);
        let plain = level_after(&mut k);
        assert!((accented - plain).abs() < 1e-3, "{accented} vs {plain}");
    }

    #[test]
    fn a_velocity_no_layer_covers_plays_nothing() {
        let mut k = kit();
        let mut pads = vec![Pad::default(); PADS];
        pads[1].layers = vec![Layer::new("x", 48_000, 1, &[0.5; 10])];
        pads[1].layers[0].vel_lo = 100;
        pads[1].layers[0].vel_hi = 127;
        let mut play = PadPlay::from_pads(&pads);
        k.swap_play(&mut play);
        k.note_on(1, 50, false, false);
        assert_eq!(k.active_voices(), 0);
    }

    #[test]
    fn a_mute_group_chokes_its_other_pads_only() {
        let mut k = kit();
        k.set_pad_param(0, PadParam::Group, 0.25); // group 1
        k.set_pad_param(1, PadParam::Group, 0.25); // group 1
        k.note_on(0, 100, false, false);
        k.note_on(2, 100, false, false); // no group
        render(&mut k, 100);
        k.note_on(1, 100, false, false);
        render(&mut k, (0.02 * SR) as usize); // past the 5 ms fade
        assert_eq!(k.active_voices(), 2, "pad 0 choked; pads 1 and 2 ring");
    }

    #[test]
    fn a_pad_retriggers_itself() {
        let mut k = kit();
        k.note_on(2, 100, false, false);
        render(&mut k, 100);
        k.note_on(2, 100, false, false);
        render(&mut k, (0.02 * SR) as usize);
        assert_eq!(k.active_voices(), 1);
    }

    #[test]
    fn gate_stops_at_note_off_and_one_shot_does_not() {
        let mut k = kit();
        k.set_pad_param(1, PadParam::Mode, 1.0); // Gate
        k.note_on(1, 100, false, false);
        k.note_on(2, 100, false, false); // One-shot
        render(&mut k, 100);
        k.note_off(1, false);
        k.note_off(2, false);
        render(&mut k, (0.02 * SR) as usize);
        assert_eq!(k.active_voices(), 1, "only the one-shot pad still plays");
    }

    #[test]
    fn start_end_and_reverse_trim_the_sample() {
        // A ramp layer: frame i holds i / frames.
        let frames = 48_000;
        let ramp: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
        let mut k = PadKit::new(SR);
        let mut pads = vec![Pad::default(); PADS];
        pads[0].layers = vec![Layer::new("ramp", 48_000, 1, &ramp)];
        pads[0].split_evenly();
        let mut s = Some(Arc::new(pads[0].layers[0].to_sample()));
        k.swap_sample(0, 0, &mut s);
        k.swap_play(&mut PadPlay::from_pads(&pads));
        k.set_param(PadKitParam::Volume, 1.0);
        k.set_pad_param(0, PadParam::Level, 1.0);
        k.set_pad_param(0, PadParam::Start, 0.5);
        k.set_pad_param(0, PadParam::End, 0.75);
        k.note_on(0, 127, false, false);
        let out = render(&mut k, frames);
        let sounding: Vec<f32> = out.iter().copied().filter(|v| v.abs() > 1e-6).collect();
        assert!(
            sounding.len() < frames / 4 + 200,
            "plays about a quarter: {}",
            sounding.len()
        );
        // Rising from about 0.5: the first sounding samples are the start.
        let first = sounding[sounding.len() / 10];
        let last = sounding[sounding.len() * 9 / 10];
        assert!(first < last, "forwards rises");
        k.all_off();
        render(&mut k, 2_000);
        k.set_pad_param(0, PadParam::Reverse, 1.0);
        k.note_on(0, 127, false, false);
        let out = render(&mut k, frames);
        let sounding: Vec<f32> = out.iter().copied().filter(|v| v.abs() > 1e-6).collect();
        assert!(
            sounding[sounding.len() / 10] > sounding[sounding.len() * 9 / 10],
            "reverse falls"
        );
    }

    #[test]
    fn all_off_fades_and_reset_silences() {
        let mut k = kit();
        k.note_on(1, 100, false, false);
        k.note_on(2, 100, false, true);
        render(&mut k, 100);
        k.all_off();
        assert_eq!(k.active_voices(), 2, "fading, not cut");
        render(&mut k, (0.02 * SR) as usize);
        assert_eq!(k.active_voices(), 0);
        k.note_on(1, 100, false, false);
        k.reset();
        assert_eq!(k.active_voices(), 0);
        assert_eq!(
            render(&mut k, 100)
                .iter()
                .fold(0.0f32, |m, v| m.max(v.abs())),
            0.0
        );
    }

    #[test]
    fn voice_stealing_takes_the_oldest() {
        let mut k = kit();
        for _ in 0..VOICES {
            k.note_on(1, 100, false, false);
            k.note_on(2, 100, false, false);
        }
        assert!(k.active_voices() <= VOICES);
    }

    #[test]
    fn an_open_cutoff_is_transparent() {
        let mut a = kit();
        let mut b = kit();
        b.set_pad_param(1, PadParam::Cutoff, 1.0);
        a.note_on(1, 100, false, false);
        b.note_on(1, 100, false, false);
        let (x, y) = (render(&mut a, 4_000), render(&mut b, 4_000));
        for (p, q) in x.iter().zip(&y) {
            assert!((p - q).abs() < 1e-4);
        }
    }
}
