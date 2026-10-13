//! The sampler machine: 16 slots, 16 voices, choke, tempo lock and Grit.
//! Real-time safe: samples arrive and leave as `Arc`s through `swap_*`,
//! so nothing is allocated or freed while rendering.

use super::sample::SampleData;
use super::settings::SlotSettings;
use super::slot::SlotPlay;
use super::voice::{Voice, segment};
use super::{SLOTS, VOICES};
use crate::params::{SamplerParam, SlotParam, on};
use std::sync::Arc;

const CHUNK: usize = 64;
/// Grit's sample-and-hold rate (an SP-1200-style 26.04 kHz).
const GRIT_RATE: f32 = 26_040.0;
/// Grit quantises over ±this range so chords don't clip.
const GRIT_RANGE: f32 = 4.0;

pub struct Sampler {
    sample_rate: f32,
    bpm: f32,
    samples: [Option<Arc<SampleData>>; SLOTS],
    plays: [SlotPlay; SLOTS],
    settings: [SlotSettings; SLOTS],
    voices: [Voice; VOICES],
    clock: u32,
    volume: f32,
    grit: bool,
    grit_phase: f32,
    grit_hold: [f32; 2],
    buf_l: [f32; CHUNK],
    buf_r: [f32; CHUNK],
}

fn grit_quantise(x: f32) -> f32 {
    ((x / GRIT_RANGE).clamp(-1.0, 1.0) * 2048.0).round() / 2048.0 * GRIT_RANGE
}

impl Sampler {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            bpm: 120.0,
            samples: std::array::from_fn(|_| None),
            plays: [SlotPlay::EMPTY; SLOTS],
            settings: [SlotSettings::default(); SLOTS],
            voices: std::array::from_fn(|_| Voice::new(sample_rate)),
            clock: 0,
            volume: 0.49,
            grit: false,
            grit_phase: 0.0,
            grit_hold: [0.0; 2],
            buf_l: [0.0; CHUNK],
            buf_r: [0.0; CHUNK],
        }
    }

    pub fn set_param(&mut self, p: SamplerParam, v: f32) {
        match p {
            SamplerParam::Volume => self.volume = v.clamp(0.0, 1.0).powi(2),
            SamplerParam::Grit => {
                let grit = on(v);
                if grit && !self.grit {
                    // capture a fresh value on the next sample
                    self.grit_phase = 1.0;
                }
                self.grit = grit;
            }
        }
    }

    pub fn set_slot_param(&mut self, slot: usize, p: SlotParam, v: f32) {
        if let Some(s) = self.settings.get_mut(slot) {
            s.set(p, v);
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        if bpm.is_finite() && bpm > 0.0 {
            self.bpm = bpm;
        }
    }

    fn kill_slot(&mut self, slot: usize) {
        for v in self.voices.iter_mut().filter(|v| v.slot == slot) {
            v.kill();
        }
    }

    /// Exchanges a slot's sample with `sample`. The caller disposes of
    /// what comes back off the audio thread. False for a bad slot. The
    /// slot's play is cleared so the old slices never play the new audio;
    /// the `swap_play` that follows a new sample restores it.
    pub fn swap_sample(&mut self, slot: usize, sample: &mut Option<Arc<SampleData>>) -> bool {
        if slot >= SLOTS {
            return false;
        }
        self.kill_slot(slot);
        std::mem::swap(&mut self.samples[slot], sample);
        self.plays[slot] = SlotPlay::EMPTY;
        true
    }

    /// Exchanges a slot's play (zone and markers). Sounding voices keep
    /// their already-started segment unless the zone layout (mode or key
    /// range) changes, which stops them; marker and slice edits apply to
    /// the next notes.
    pub fn swap_play(&mut self, slot: usize, play: &mut SlotPlay) -> bool {
        if slot >= SLOTS {
            return false;
        }
        if !self.plays[slot].same_zones(play) {
            self.kill_slot(slot);
        }
        std::mem::swap(&mut self.plays[slot], play);
        true
    }

    pub fn note_on(&mut self, key: u8, vel: u8, live: bool) {
        if key > 127 || vel == 0 {
            return;
        }
        let Some(slot) = (0..SLOTS).find(|&i| {
            let (lo, hi) = self.plays[i].key_range();
            self.samples[i].is_some() && (lo..=hi).contains(&key)
        }) else {
            return;
        };
        let Some(sample) = self.samples[slot].as_deref() else {
            return;
        };
        let s = self.settings[slot];
        let Some(seg) = segment(
            &self.plays[slot],
            key,
            &s,
            sample.sample_rate,
            sample.frames(),
            self.sample_rate,
            self.bpm,
        ) else {
            return;
        };
        if s.choke {
            for v in self
                .voices
                .iter_mut()
                .filter(|v| v.active && v.slot == slot)
            {
                v.choke();
            }
        }
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        let voices = &self.voices;
        let age = |v: &Voice| clock.wrapping_sub(v.age);
        let i = voices
            .iter()
            .position(|v| !v.active)
            .or_else(|| voices.iter().position(Voice::tail_only))
            .or_else(|| {
                (0..VOICES)
                    .filter(|&i| !voices[i].held)
                    .max_by_key(|&i| age(&voices[i]))
            })
            .unwrap_or_else(|| (0..VOICES).max_by_key(|&i| age(&voices[i])).unwrap_or(0));
        self.voices[i].start(&seg, slot, key, vel.min(127), live, clock);
    }

    pub fn note_off(&mut self, key: u8, live: bool) {
        for v in &mut self.voices {
            if v.active && v.held && v.key == key && v.live == live {
                let release = self.settings[v.slot].release_s;
                v.release(release);
            }
        }
    }

    pub fn all_off(&mut self) {
        for v in &mut self.voices {
            let release = self.settings[v.slot].release_s;
            v.release(release);
        }
    }

    /// Releases the pattern's notes; auditioned (live) notes keep sounding.
    pub fn pattern_off(&mut self) {
        for v in self.voices.iter_mut().filter(|v| !v.live) {
            let release = self.settings[v.slot].release_s;
            v.release(release);
        }
    }

    pub fn reset(&mut self) {
        for v in &mut self.voices {
            v.kill();
        }
        self.grit_hold = [0.0; 2];
        self.grit_phase = 0.0;
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Keys of the voices still held (not released), in voice order.
    #[cfg(test)]
    pub(crate) fn held_keys(&self) -> Vec<u8> {
        self.voices
            .iter()
            .filter(|v| v.active && v.held)
            .map(|v| v.key)
            .collect()
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let total = left.len().min(right.len());
        let mut pos = 0;
        while pos < total {
            let n = (total - pos).min(CHUNK);
            self.buf_l[..n].fill(0.0);
            self.buf_r[..n].fill(0.0);
            for v in self.voices.iter_mut().filter(|v| v.active) {
                match self.samples[v.slot].as_deref() {
                    Some(sample) => v.render(
                        sample,
                        &self.settings[v.slot],
                        &mut self.buf_l[..n],
                        &mut self.buf_r[..n],
                    ),
                    None => v.kill(),
                }
            }
            for i in 0..n {
                let (mut l, mut r) = (self.buf_l[i], self.buf_r[i]);
                if self.grit {
                    self.grit_phase += GRIT_RATE / self.sample_rate;
                    if self.grit_phase >= 1.0 {
                        self.grit_phase -= self.grit_phase.floor();
                        self.grit_hold = [grit_quantise(l), grit_quantise(r)];
                    }
                    [l, r] = self.grit_hold;
                }
                left[pos + i] += l * self.volume;
                right[pos + i] += r * self.volume;
            }
            pos += n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::slot::{SlotMeta, SlotMode, SlotPlay};
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 48_000.0;

    fn sine(frames: usize) -> Arc<SampleData> {
        Arc::new(SampleData {
            sample_rate: 48_000,
            left: (0..frames).map(|i| (i as f32 * 0.05).sin() * 0.8).collect(),
            right: None,
        })
    }

    fn loaded() -> Sampler {
        let mut s = Sampler::new(SR);
        let mut a = Some(sine(48_000));
        assert!(s.swap_sample(0, &mut a));
        assert!(a.is_none(), "got the empty slot back");
        let mut play = SlotPlay::from_meta(&SlotMeta::sliced(
            36,
            vec![0, 12_000, 24_000, 36_000],
            120.0,
            48_000,
        ));
        assert!(s.swap_play(0, &mut play));
        let mut b = Some(sine(48_000));
        s.swap_sample(1, &mut b);
        let mut chrom = SlotPlay::from_meta(&SlotMeta::chromatic(60, 83, 60, 48_000));
        s.swap_play(1, &mut chrom);
        s
    }

    fn render(s: &mut Sampler, n: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        s.render(&mut l, &mut r);
        (l, r)
    }

    #[test]
    fn notes_in_a_zone_play_and_others_are_silent() {
        let mut s = loaded();
        s.note_on(30, 100, false);
        s.note_on(50, 100, false);
        assert_eq!(s.active_voices(), 0, "no zone there");
        s.note_on(37, 100, false);
        s.note_on(64, 100, false);
        assert_eq!(s.active_voices(), 2);
        let (l, r) = render(&mut s, 4800);
        assert!(all_finite(&l) && peak(&l) > 0.1 && peak(&r) > 0.1);
    }

    #[test]
    fn choke_cuts_the_slots_previous_note() {
        let mut s = loaded();
        s.set_slot_param(0, SlotParam::Choke, 1.0);
        s.note_on(36, 100, false);
        render(&mut s, 480);
        s.note_on(37, 100, false);
        render(&mut s, 3000);
        assert_eq!(s.active_voices(), 1, "the first slice was choked");
        let mut free = loaded();
        free.note_on(36, 100, false);
        render(&mut free, 480);
        free.note_on(37, 100, false);
        render(&mut free, 3000);
        assert_eq!(free.active_voices(), 2, "without choke both ring");
    }

    #[test]
    fn audition_and_pattern_release_separately() {
        let mut s = loaded();
        s.note_on(60, 100, true);
        s.note_on(60, 100, false);
        s.note_off(60, false);
        render(&mut s, 24_000);
        assert_eq!(s.active_voices(), 1);
        s.note_off(60, true);
        render(&mut s, 24_000);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn swapping_a_sample_stops_its_voices_and_returns_the_old_one() {
        let mut s = loaded();
        s.note_on(36, 100, false);
        let mut none: Option<Arc<SampleData>> = None;
        assert!(s.swap_sample(0, &mut none));
        assert!(none.is_some(), "old sample handed back for disposal");
        assert_eq!(s.active_voices(), 0);
        s.note_on(36, 100, false);
        assert_eq!(s.active_voices(), 0, "empty slot");
        assert!(!s.swap_sample(16, &mut None));
    }

    #[test]
    fn a_new_sample_is_silent_until_its_play_arrives() {
        let mut s = loaded();
        let mut short = Some(sine(1_000));
        assert!(s.swap_sample(0, &mut short));
        s.note_on(37, 100, false);
        assert_eq!(
            s.active_voices(),
            0,
            "the old slices don't play the new sample"
        );
        let mut play = SlotPlay::from_meta(&SlotMeta::sliced(36, vec![0, 500], 120.0, 1_000));
        assert!(s.swap_play(0, &mut play));
        assert_eq!(play, SlotPlay::EMPTY, "the reset play comes back");
        s.note_on(37, 100, false);
        assert_eq!(s.active_voices(), 1);
    }

    #[test]
    fn sixteen_voices_and_the_oldest_is_stolen() {
        let mut s = loaded();
        for k in 60..76 {
            s.note_on(k, 100, false);
        }
        assert_eq!(s.active_voices(), 16);
        s.note_on(80, 100, false);
        assert_eq!(s.active_voices(), 16);
    }

    #[test]
    fn a_released_voice_is_stolen_before_the_oldest_held_one() {
        let mut s = loaded();
        for k in 60..76 {
            s.note_on(k, 100, false);
        }
        s.note_off(67, false);
        assert_eq!(s.active_voices(), 16, "67 is still releasing");
        s.note_on(80, 100, false);
        let mut want: Vec<u8> = (60..76).collect();
        want[7] = 80;
        assert_eq!(s.held_keys(), want, "80 took the released voice");
        s.note_on(81, 100, false);
        want[0] = 81;
        assert_eq!(s.held_keys(), want, "then the oldest held key (60) goes");
    }

    #[test]
    fn a_voice_left_with_only_its_steal_fade_is_free_first() {
        let dc = || {
            Some(Arc::new(SampleData {
                sample_rate: 48_000,
                left: vec![0.5; 48_000].into(),
                right: None,
            }))
        };
        let mut s = Sampler::new(SR);
        s.swap_sample(0, &mut dc());
        s.swap_play(
            0,
            &mut SlotPlay::from_meta(&SlotMeta::chromatic(60, 83, 60, 48_000)),
        );
        // key 36 plays a 20-frame slice, well inside the 2 ms steal fade
        s.swap_sample(1, &mut dc());
        s.swap_play(
            1,
            &mut SlotPlay::from_meta(&SlotMeta::sliced(36, vec![0, 20], 120.0, 48_000)),
        );
        for k in 60..76 {
            s.note_on(k, 100, false);
        }
        render(&mut s, 2000);
        s.note_off(61, false);
        s.note_on(36, 100, false);
        render(&mut s, 50);
        let mut want: Vec<u8> = (60..76).collect();
        want.remove(1);
        assert_eq!(s.held_keys(), want, "36's note is over");
        assert_eq!(s.active_voices(), 16, "its fade still plays");
        s.note_off(62, false);
        s.note_on(80, 100, false);
        want[1] = 80;
        assert_eq!(
            s.held_keys(),
            want,
            "80 took the fading voice, not the older released 62"
        );
    }

    #[test]
    fn a_stolen_voice_fades_out_without_a_click() {
        let mut s = Sampler::new(SR);
        let mut dc = Some(Arc::new(SampleData {
            sample_rate: 48_000,
            left: vec![0.5; 48_000].into(),
            right: None,
        }));
        s.swap_sample(0, &mut dc);
        let mut play = SlotPlay::from_meta(&SlotMeta::chromatic(36, 96, 60, 48_000));
        s.swap_play(0, &mut play);
        for k in 60..76 {
            s.note_on(k, 100, false);
        }
        let (before, _) = render(&mut s, 2000);
        let per_voice = before[1999] / 16.0;
        assert!(per_voice > 0.01);
        s.note_on(80, 100, false);
        let (after, _) = render(&mut s, 500);
        let mut joined = vec![before[1999]];
        joined.extend_from_slice(&after);
        let step = joined
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(step < 0.1 * per_voice, "steal click {step} vs {per_voice}");
    }

    #[test]
    fn grit_switched_back_on_does_not_replay_a_stale_value() {
        let mut s = loaded();
        s.set_param(SamplerParam::Volume, 1.0);
        s.set_param(SamplerParam::Grit, 1.0);
        s.note_on(60, 127, false);
        for n in [4801, 4802, 4803] {
            render(&mut s, n);
        }
        s.set_param(SamplerParam::Grit, 0.0);
        s.all_off();
        render(&mut s, 48_000);
        assert_eq!(s.active_voices(), 0);
        s.set_param(SamplerParam::Grit, 1.0);
        let (l, r) = render(&mut s, 4);
        assert_eq!((l[0], r[0]), (0.0, 0.0), "the held value is fresh");
    }

    #[test]
    fn a_marker_edit_keeps_voices_but_a_zone_change_stops_them() {
        let mut s = Sampler::new(SR);
        s.swap_sample(0, &mut Some(sine(48_000)));
        let mut meta = SlotMeta::chromatic(48, 71, 60, 48_000);
        meta.loop_on = true;
        meta.loop_start = 10_000;
        meta.loop_end = 30_000;
        s.swap_play(0, &mut SlotPlay::from_meta(&meta));
        s.note_on(60, 100, false);
        render(&mut s, 480);
        let mut markers = meta.clone();
        markers.loop_end = 20_000;
        markers.start = 1_000;
        markers.root = 62;
        assert!(s.swap_play(0, &mut SlotPlay::from_meta(&markers)));
        assert_eq!(s.active_voices(), 1, "marker/loop edits keep the note");
        let (l, _) = render(&mut s, 480);
        assert!(peak(&l) > 0.01, "and it keeps sounding");
        for change in [
            |m: &mut SlotMeta| m.base_key = 50,
            |m: &mut SlotMeta| m.high_key = 70,
            |m: &mut SlotMeta| m.mode = SlotMode::Slice,
        ] {
            let mut zone = markers.clone();
            change(&mut zone);
            s.swap_play(0, &mut SlotPlay::from_meta(&markers));
            s.note_on(60, 100, false);
            assert!(s.active_voices() > 0);
            s.swap_play(0, &mut SlotPlay::from_meta(&zone));
            assert_eq!(s.active_voices(), 0, "a zone change stops the slot");
        }
        let mut sliced = SlotMeta::sliced(36, vec![0, 1000, 2000], 120.0, 48_000);
        s.swap_play(0, &mut SlotPlay::from_meta(&sliced));
        s.note_on(37, 100, false);
        sliced.original_bpm = 100.0;
        s.swap_play(0, &mut SlotPlay::from_meta(&sliced));
        assert_eq!(s.active_voices(), 1, "a tempo edit keeps slices playing");
        sliced.slices = vec![0, 1500, 2000];
        s.swap_play(0, &mut SlotPlay::from_meta(&sliced));
        assert_eq!(s.active_voices(), 1, "moving a slice keeps it playing");
        sliced.slices = vec![0, 1500, 2000, 3000];
        s.swap_play(0, &mut SlotPlay::from_meta(&sliced));
        assert_eq!(s.active_voices(), 0, "adding a slice stops the slot");
        s.note_on(37, 100, false);
        sliced.base_key = 40;
        s.swap_play(0, &mut SlotPlay::from_meta(&sliced));
        assert_eq!(s.active_voices(), 0, "moving the zone stops the slot");
    }

    #[test]
    fn grit_quantises_and_volume_scales() {
        let mut s = loaded();
        s.set_param(SamplerParam::Grit, 1.0);
        s.set_param(SamplerParam::Volume, 1.0);
        s.note_on(60, 127, false);
        let (l, _) = render(&mut s, 4800);
        assert!(peak(&l) > 0.1);
        for v in &l {
            let steps = v / 4.0 * 2048.0;
            assert!((steps - steps.round()).abs() < 1e-3, "{v}");
        }
        let held = l.windows(2).filter(|w| w[0] == w[1]).count();
        assert!(held > 1000, "sample-and-hold repeats values: {held}");
        let mut quiet = loaded();
        quiet.set_param(SamplerParam::Volume, 0.0);
        quiet.note_on(60, 127, false);
        assert_eq!(peak(&render(&mut quiet, 480).0), 0.0);
    }

    #[test]
    fn all_off_releases_and_reset_silences() {
        let mut s = loaded();
        s.note_on(60, 100, false);
        s.note_on(61, 100, true);
        s.all_off();
        render(&mut s, 24_000);
        assert_eq!(s.active_voices(), 0);
        s.note_on(60, 100, false);
        s.reset();
        assert_eq!(s.active_voices(), 0);
        assert_eq!(peak(&render(&mut s, 100).0), 0.0);
    }
}
