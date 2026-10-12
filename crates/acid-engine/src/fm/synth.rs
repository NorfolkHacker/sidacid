//! The FM machine: 12 voices (or one in mono mode), the shared LFO, and
//! the mix, which runs through the filter, chorus and phaser. Notes come from the pattern or from audition (`live`); a
//! note-off only releases voices from its own source, and releases every
//! voice of that pitch.

use super::algorithms::ALGORITHMS;
use super::fx::FmFx;
use super::modulation::{Lfo, lfo_hz};
use super::patch::FmPatch;
use super::sine::Sine;
use super::voice::Voice;
use super::{BLOCK, OPS};
use crate::params::{FmParam, OpParam};

pub const VOICES: usize = 12;
const MONO_STACK: usize = 16;
/// Full scale of the Vintage 12-bit quantiser (headroom for chords).
const VINTAGE_RANGE: f32 = 4.0;

pub struct FmSynth {
    sample_rate: f32,
    patch: FmPatch,
    voices: [Voice; VOICES],
    lfo: Lfo,
    sine: Sine,
    clock: u32,
    stack: [u8; MONO_STACK],
    stack_len: usize,
    buf: [f32; BLOCK],
    fx: FmFx,
}

impl FmSynth {
    /// Allocates the sine table: call off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            patch: FmPatch::default(),
            voices: std::array::from_fn(|_| Voice::new(sample_rate)),
            lfo: Lfo::new(),
            sine: Sine::new(),
            clock: 0,
            stack: [0; MONO_STACK],
            stack_len: 0,
            buf: [0.0; BLOCK],
            fx: FmFx::new(sample_rate),
        }
    }

    pub fn patch(&self) -> &FmPatch {
        &self.patch
    }

    pub fn set_param(&mut self, p: FmParam, v: f32) {
        let was_mono = self.patch.mono;
        self.patch.set(p, v);
        if self.patch.mono != was_mono {
            self.all_off();
        }
    }

    pub fn set_op_param(&mut self, op: usize, p: OpParam, v: f32) {
        if op < OPS {
            self.patch.set_op(op, p, v);
        }
    }

    /// Pitches above 127 and velocity 0 are ignored.
    pub fn note_on(&mut self, pitch: u8, vel: u8, live: bool) {
        if pitch > 127 || vel == 0 {
            return;
        }
        self.clock = self.clock.wrapping_add(1);
        if self.patch.lfo_sync {
            self.lfo.reset();
        }
        if self.patch.mono {
            self.mono_on(pitch, vel.min(127), live);
        } else {
            self.poly_on(pitch, vel.min(127), live);
        }
    }

    fn poly_on(&mut self, pitch: u8, vel: u8, live: bool) {
        let clock = self.clock;
        let age = |v: &Voice| clock.wrapping_sub(v.age);
        let voices = &self.voices;
        let i = voices
            .iter()
            .position(|v| !v.active)
            .or_else(|| {
                (0..VOICES)
                    .filter(|&i| !voices[i].held)
                    .max_by_key(|&i| age(&voices[i]))
            })
            .unwrap_or_else(|| (0..VOICES).max_by_key(|&i| age(&voices[i])).unwrap_or(0));
        self.voices[i].start(&self.patch, pitch, vel, live, clock);
    }

    fn stack_top(&self) -> Option<u8> {
        self.stack[..self.stack_len].last().copied()
    }

    fn stack_remove(&mut self, pitch: u8) {
        if let Some(i) = self.stack[..self.stack_len]
            .iter()
            .position(|&p| p == pitch)
        {
            self.stack.copy_within(i + 1..self.stack_len, i);
            self.stack_len -= 1;
        }
    }

    fn mono_on(&mut self, pitch: u8, vel: u8, live: bool) {
        self.stack_remove(pitch);
        if self.stack_len == MONO_STACK {
            self.stack.copy_within(1.., 0);
            self.stack_len -= 1;
        }
        self.stack[self.stack_len] = pitch;
        self.stack_len += 1;
        let v = &mut self.voices[0];
        if v.active && v.held {
            v.glide_to(&self.patch, pitch);
            v.live = live;
        } else {
            v.start(&self.patch, pitch, vel, live, self.clock);
        }
    }

    pub fn note_off(&mut self, pitch: u8, live: bool) {
        if self.patch.mono {
            let top = self.stack_top();
            self.stack_remove(pitch);
            if top == Some(pitch) {
                match self.stack_top() {
                    Some(prev) => self.voices[0].glide_to(&self.patch, prev),
                    None => self.voices[0].release(),
                }
            }
            return;
        }
        for v in &mut self.voices {
            if v.active && v.held && v.pitch == pitch && v.live == live {
                v.release();
            }
        }
    }

    /// Releases every note (tails ring out).
    pub fn all_off(&mut self) {
        self.stack_len = 0;
        for v in &mut self.voices {
            v.release();
        }
    }

    /// Releases the pattern's notes; auditioned (live) notes keep sounding.
    /// Mono mode shares one key stack between both sources: a sounding
    /// audition note stays and the keys under it are forgotten, otherwise
    /// everything is released.
    pub fn pattern_off(&mut self) {
        if self.patch.mono {
            let v = &self.voices[0];
            if v.active && v.held && v.live {
                self.stack[0] = v.pitch;
                self.stack_len = 1;
            } else {
                self.all_off();
            }
            return;
        }
        for v in self.voices.iter_mut().filter(|v| !v.live) {
            v.release();
        }
    }

    /// Silences everything now.
    pub fn reset(&mut self) {
        self.stack_len = 0;
        for v in &mut self.voices {
            v.kill();
        }
        self.fx.reset();
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Adds the machine's output into `left`/`right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let alg = ALGORITHMS[self.patch.algorithm.min(ALGORITHMS.len() - 1)];
        let total = left.len().min(right.len());
        let gain = self.patch.volume * self.patch.volume;
        let mut pos = 0;
        while pos < total {
            let n = (total - pos).min(BLOCK);
            let lfo = self.lfo.tick(
                lfo_hz(self.patch.lfo_speed),
                self.patch.lfo_wave,
                n as f32 / self.sample_rate,
            );
            let buf = &mut self.buf[..n];
            buf.fill(0.0);
            for v in self.voices.iter_mut().filter(|v| v.active) {
                v.render(&self.patch, &alg, lfo, &self.sine, buf);
            }
            for x in buf.iter_mut() {
                if self.patch.vintage {
                    // 12-bit DAC with headroom for chords: ±VINTAGE_RANGE.
                    let q = (*x / VINTAGE_RANGE).clamp(-1.0, 1.0);
                    *x = (q * 2048.0).round() / 2048.0 * VINTAGE_RANGE;
                }
                *x *= gain;
            }
            self.fx.process(
                &self.patch,
                buf,
                &mut left[pos..pos + n],
                &mut right[pos..pos + n],
            );
            pos += n;
        }
    }

    #[cfg(test)]
    pub(crate) fn sounding(&self) -> Vec<u8> {
        self.voices
            .iter()
            .filter(|v| v.active && v.held)
            .map(|v| v.pitch)
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn voice_note(&self, i: usize) -> f32 {
        self.voices[i].note
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{FmParam, OpParam};
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 48_000.0;

    fn render(s: &mut FmSynth, seconds: f32) -> Vec<f32> {
        let n = (seconds * SR) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        s.render(&mut l, &mut r);
        assert_eq!(l, r, "dry FM is mono");
        l
    }

    fn crossings(x: &[f32]) -> usize {
        x.windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count()
    }

    #[test]
    fn init_voice_is_a_sine_at_the_note() {
        let mut s = FmSynth::new(SR);
        s.note_on(69, 100, false);
        render(&mut s, 0.05);
        let x = render(&mut s, 0.1);
        assert!(all_finite(&x) && peak(&x) > 0.05 && peak(&x) < 1.0);
        let c = crossings(&x);
        assert!((86..=90).contains(&c), "440 Hz for 0.1 s: {c} crossings");
    }

    #[test]
    fn a_modulator_adds_harmonics() {
        let mut plain = FmSynth::new(SR);
        plain.note_on(57, 100, false);
        let a = render(&mut plain, 0.2);
        let mut fm = FmSynth::new(SR);
        fm.set_op_param(1, OpParam::Level, 0.9); // OP2 modulates OP1 in algorithm 1
        fm.note_on(57, 100, false);
        let b = render(&mut fm, 0.2);
        assert!(crossings(&b) as f32 > crossings(&a) as f32 * 1.3);
    }

    #[test]
    fn twelve_voices_and_the_oldest_is_stolen() {
        let mut s = FmSynth::new(SR);
        for p in 40..52 {
            s.note_on(p, 100, false);
            render(&mut s, 0.001);
        }
        assert_eq!(s.active_voices(), 12);
        s.note_on(80, 100, false);
        assert_eq!(s.active_voices(), 12);
        assert!(!s.sounding().contains(&40), "the oldest note was stolen");
        assert!(s.sounding().contains(&80));
    }

    #[test]
    fn release_frees_the_voice() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, false);
        render(&mut s, 0.05);
        s.note_off(60, false);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
        assert_eq!(peak(&render(&mut s, 0.01)), 0.0);
    }

    #[test]
    fn audition_and_pattern_notes_release_separately() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, true);
        s.note_on(60, 100, false);
        s.note_off(60, false);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 1, "the audition note still sounds");
        s.note_off(60, true);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn a_note_off_releases_every_voice_of_that_pitch() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, false);
        s.note_on(60, 100, false);
        s.note_off(60, false);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn mono_legato_returns_to_the_held_key() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::Mono, 1.0);
        s.note_on(60, 100, false);
        s.note_on(64, 100, false);
        render(&mut s, 0.01);
        assert_eq!(s.active_voices(), 1);
        assert_eq!(s.sounding(), vec![64]);
        s.note_off(64, false);
        assert_eq!(s.sounding(), vec![60]);
        s.note_off(60, false);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn glide_slides_between_mono_notes() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::Mono, 1.0);
        s.set_param(FmParam::Glide, 0.5); // 0.5 s
        s.note_on(60, 100, false);
        render(&mut s, 0.01);
        s.note_on(72, 100, false);
        render(&mut s, 0.05);
        let mid = s.voice_note(0);
        assert!(mid > 60.5 && mid < 71.5, "gliding: {mid}");
        render(&mut s, 5.0);
        assert!((s.voice_note(0) - 72.0).abs() < 0.01);
    }

    /// Renders whole control blocks until the last sample is louder than
    /// `min`, so a retrigger lands where a hard cut would click.
    fn render_until_loud(s: &mut FmSynth, min: f32) -> Vec<f32> {
        let mut x = render(s, 0.05);
        for _ in 0..1000 {
            if x[x.len() - 1].abs() > min {
                return x;
            }
            x = render(s, BLOCK as f32 / SR);
        }
        panic!("never louder than {min}");
    }

    /// The largest sample-to-sample step in `x`.
    fn max_step(x: &[f32]) -> f32 {
        x.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn stealing_does_not_click() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::Mono, 1.0);
        s.note_on(45, 127, false);
        let before = render_until_loud(&mut s, 0.05);
        let last = before[before.len() - 1];
        assert!(last.abs() > 0.05, "precondition: loud at the cut ({last})");
        s.note_off(45, false);
        s.note_on(45, 127, false); // restarts voice 0 while it still sounds
        let after = render(&mut s, 0.002);
        let jump = (after[0] - last).abs();
        assert!(jump < 0.01, "jump {jump}");
    }

    #[test]
    fn poly_steal_does_not_click() {
        let mut s = FmSynth::new(SR);
        // Twelve in-phase voices: the stolen one is a twelfth of the mix.
        for _ in 0..VOICES {
            s.note_on(45, 80, false);
        }
        let before = render_until_loud(&mut s, 0.3);
        let last = before[before.len() - 1];
        let step = max_step(&before);
        s.note_on(80, 80, false); // steals the oldest held voice
        assert_eq!(s.active_voices(), VOICES);
        let after = render(&mut s, 0.002);
        let jump = (after[0] - last).abs();
        // The other eleven voices move by at most `step`; a hard cut would
        // add the stolen voice's last sample (about last / 12) on top.
        assert!(
            jump < step * 1.25,
            "jump {jump}, normal step {step}, last {last}"
        );
    }

    #[test]
    fn a_released_voice_is_stolen_before_a_held_one() {
        let mut s = FmSynth::new(SR);
        s.set_op_param(0, OpParam::R4, 0.1); // long release: released voices keep ringing
        for p in 40..52 {
            s.note_on(p, 100, false);
            render(&mut s, 0.001);
        }
        s.note_off(43, false);
        s.note_off(45, false);
        render(&mut s, 0.01);
        assert_eq!(s.active_voices(), VOICES, "released voices still ring");
        s.note_on(80, 100, false);
        assert_eq!(s.active_voices(), VOICES);
        let mut held = s.sounding();
        held.sort_unstable();
        let mut want: Vec<u8> = (40..52).filter(|&p| p != 43 && p != 45).collect();
        want.push(80);
        assert_eq!(
            held, want,
            "every held note survives; a released voice was stolen"
        );
        s.note_on(81, 100, false);
        let mut held = s.sounding();
        held.sort_unstable();
        want.push(81);
        assert_eq!(held, want, "the second released voice goes next");
    }

    #[test]
    fn vintage_quantises_to_12_bits() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::Vintage, 1.0);
        s.set_param(FmParam::Volume, 1.0);
        s.note_on(60, 100, false);
        let x = render(&mut s, 0.05);
        assert!(peak(&x) > 0.05);
        for v in x {
            // 12 bits across ±4.0 of headroom: steps of 4/2048.
            let steps = v * 2048.0 / 4.0;
            assert!((steps - steps.round()).abs() < 1e-3, "{v}");
        }
    }

    #[test]
    fn vintage_does_not_clip_chords() {
        let chord = |vintage: bool| {
            let mut s = FmSynth::new(SR);
            s.set_param(FmParam::Vintage, if vintage { 1.0 } else { 0.0 });
            s.set_param(FmParam::Volume, 1.0);
            s.set_param(FmParam::Algorithm, 1.0); // 32: every operator a carrier
            for op in 0..3 {
                s.set_op_param(op, OpParam::Level, 1.0);
            }
            for p in [48, 52, 55, 60] {
                s.note_on(p, 110, false);
            }
            render(&mut s, 0.2)
        };
        let clean = peak(&chord(false));
        let vintage = peak(&chord(true));
        assert!(
            clean > 1.2,
            "precondition: the chord sums above 1.0 ({clean})"
        );
        assert!(vintage < 3.5, "well below the ±4.0 bound: {vintage}");
        assert!(
            (vintage - clean).abs() < clean * 0.05,
            "vintage {vintage} vs clean {clean}: not clipped"
        );
    }

    #[test]
    fn switching_to_mono_releases_held_notes() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, false);
        s.note_on(64, 100, false);
        s.set_param(FmParam::Mono, 1.0);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn chorus_on_makes_the_output_stereo() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::ChorusMix, 1.0);
        s.note_on(60, 100, false);
        let n = 9600;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        s.render(&mut l, &mut r);
        assert_ne!(l, r);
    }

    #[test]
    fn reset_silences_at_once_and_bad_input_is_ignored() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, false);
        s.note_on(200, 100, false);
        s.note_on(61, 0, false);
        assert_eq!(s.active_voices(), 1);
        s.reset();
        assert_eq!(s.active_voices(), 0);
        assert_eq!(peak(&render(&mut s, 0.01)), 0.0);
    }
}
