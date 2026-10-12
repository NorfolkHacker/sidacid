//! One sampler voice: plays a segment of a sample forwards or backwards,
//! optionally looping with a crossfade, through an attack/decay/release
//! envelope and an optional low-pass filter.

use super::sample::SampleData;
use super::settings::SlotSettings;
use super::slot::{SlotMode, SlotPlay};
use sid_core::Svf;

/// Fade at the end of a segment (avoids a click when a slice stops).
const END_FADE_S: f32 = 0.003;
/// Release used when a newer note in the same slot chokes this one.
const CHOKE_S: f32 = 0.005;
/// Slowest playback rate (keeps a voice from stalling).
const MIN_RATE: f64 = 1e-6;
/// Loop crossfade length.
const LOOP_XFADE_S: f64 = 0.010;
/// Envelope level at which a releasing voice stops.
const SILENT: f32 = 1e-4;

/// What a note plays: a range of source frames, its direction, an
/// optional loop, and the playback rate in source frames per output sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub reverse: bool,
    pub looping: Option<(f64, f64)>,
    pub rate: f64,
}

/// The segment `key` plays in a slot, or `None` outside its zone.
pub fn segment(
    play: &SlotPlay,
    key: u8,
    s: &SlotSettings,
    sample_rate: u32,
    frames: usize,
    out_rate: f32,
    bpm: f32,
) -> Option<Segment> {
    let (lo, hi) = play.key_range();
    if key < lo || key > hi || frames == 0 {
        return None;
    }
    let frames = frames as u32;
    let base = sample_rate as f64 / out_rate as f64;
    let tuned = (s.tune as f64 + s.fine_cents as f64 / 100.0) / 12.0;
    let (start, end, rate, looping) = match play.mode {
        SlotMode::Slice => {
            let i = (key - lo) as usize;
            if i >= play.slice_count {
                return None;
            }
            let start = play.slices[i];
            let end = if i + 1 < play.slice_count {
                play.slices[i + 1]
            } else {
                frames
            };
            let tempo = if s.tempo_lock && play.original_bpm > 0.0 {
                bpm as f64 / play.original_bpm as f64
            } else {
                1.0
            };
            (start, end, base * 2f64.powf(tuned) * tempo, None)
        }
        SlotMode::Chromatic => {
            let end = play.end.min(frames);
            let semis = tuned + (key as f64 - play.root as f64) / 12.0;
            let looping = (play.loop_on && !s.reverse && play.loop_start < play.loop_end.min(end))
                .then(|| (play.loop_start as f64, play.loop_end.min(end) as f64));
            (play.start, end, base * 2f64.powf(semis), looping)
        }
    };
    (start < end).then_some(Segment {
        start: start as f64,
        end: end as f64,
        reverse: s.reverse,
        looping,
        rate: rate.max(MIN_RATE),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Release,
    Off,
}

pub struct Voice {
    pub active: bool,
    pub held: bool,
    pub live: bool,
    pub slot: usize,
    pub key: u8,
    pub age: u32,
    pos: f64,
    step: f64,
    end: f64,
    reverse: bool,
    looping: bool,
    loop_start: f64,
    loop_end: f64,
    env: f32,
    stage: Stage,
    release_coef: f32,
    gain: f32,
    filter: [Svf; 2],
    sample_rate: f32,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            active: false,
            held: false,
            live: false,
            slot: 0,
            key: 0,
            age: 0,
            pos: 0.0,
            step: 0.0,
            end: 0.0,
            reverse: false,
            looping: false,
            loop_start: 0.0,
            loop_end: 0.0,
            env: 0.0,
            stage: Stage::Off,
            release_coef: 0.0,
            gain: 0.0,
            filter: [Svf::new(sample_rate), Svf::new(sample_rate)],
            sample_rate,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        seg: &Segment,
        s: &SlotSettings,
        slot: usize,
        key: u8,
        vel: u8,
        live: bool,
        age: u32,
    ) {
        self.active = true;
        self.held = true;
        self.live = live;
        self.slot = slot;
        self.key = key;
        self.age = age;
        self.reverse = seg.reverse;
        if seg.reverse {
            self.pos = seg.end - 1.0;
            self.step = -seg.rate;
            self.end = seg.start;
        } else {
            self.pos = seg.start;
            self.step = seg.rate;
            self.end = seg.end;
        }
        match seg.looping {
            Some((a, b)) if !seg.reverse && a < b => {
                self.looping = true;
                self.loop_start = a;
                self.loop_end = b;
            }
            _ => self.looping = false,
        }
        self.env = 0.0;
        self.stage = Stage::Attack;
        self.gain = vel.min(127) as f32 / 127.0 * s.level_gain;
        self.filter = [Svf::new(self.sample_rate), Svf::new(self.sample_rate)];
    }

    pub fn release(&mut self, seconds: f32) {
        if self.active && self.stage != Stage::Off {
            self.held = false;
            self.stage = Stage::Release;
            self.release_coef = (-1.0 / (seconds.max(1e-4) * self.sample_rate)).exp();
        }
    }

    pub fn choke(&mut self) {
        self.release(CHOKE_S);
    }

    pub fn kill(&mut self) {
        self.active = false;
        self.held = false;
        self.stage = Stage::Off;
    }

    /// Adds this voice into `out_l`/`out_r`.
    pub fn render(
        &mut self,
        sample: &SampleData,
        s: &SlotSettings,
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        if !self.active {
            return;
        }
        let sr = self.sample_rate;
        let attack_inc = 1.0 / (s.attack_s * sr).max(1.0);
        let decay_coef = (-1.0 / (s.decay_s * sr).max(1.0)).exp();
        let end_fade = (END_FADE_S * sr).max(1.0);
        let loop_len = self.loop_end - self.loop_start;
        let xfade = (LOOP_XFADE_S * sample.sample_rate as f64)
            .min(loop_len * 0.5)
            .max(1.0);
        let filter_on = s.cutoff_knob < 0.999 || s.resonance > 0.0;
        if filter_on {
            for f in &mut self.filter {
                f.set(s.cutoff_hz, s.resonance);
            }
        }
        for i in 0..out_l.len().min(out_r.len()) {
            match self.stage {
                Stage::Attack => {
                    self.env += attack_inc;
                    if self.env >= 1.0 {
                        self.env = 1.0;
                        self.stage = Stage::Decay;
                    }
                }
                Stage::Decay => {
                    self.env = s.sustain + (self.env - s.sustain) * decay_coef;
                    if s.sustain <= 0.0 && self.env < SILENT {
                        self.kill();
                        return;
                    }
                }
                Stage::Release => {
                    self.env *= self.release_coef;
                    if self.env < SILENT {
                        self.kill();
                        return;
                    }
                }
                Stage::Off => return,
            }
            let past_end = if self.reverse {
                self.pos < self.end
            } else {
                self.pos >= self.end
            };
            if past_end && !self.looping {
                self.kill();
                return;
            }
            let (mut l, mut r) = sample.read(self.pos);
            if self.looping && self.pos > self.loop_end - xfade {
                let a = ((self.loop_end - self.pos) / xfade) as f32;
                let (l2, r2) = sample.read(self.pos - loop_len);
                l = l * a + l2 * (1.0 - a);
                r = r * a + r2 * (1.0 - a);
            }
            let edge = if self.looping {
                1.0
            } else {
                ((((self.pos - self.end).abs() / self.step.abs().max(MIN_RATE)) as f32) / end_fade)
                    .min(1.0)
            };
            if filter_on {
                l = self.filter[0].process(l);
                r = self.filter[1].process(r);
            }
            let g = self.env * edge * self.gain;
            out_l[i] += l * g * s.pan_l;
            out_r[i] += r * g * s.pan_r;
            self.pos += self.step;
            if self.looping && self.pos >= self.loop_end {
                self.pos = self.loop_start + (self.pos - self.loop_start).rem_euclid(loop_len);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::SlotParam;
    use crate::sampler::slot::{SlotMeta, SlotPlay};

    const SR: f32 = 48_000.0;

    fn ramp_sample(frames: usize) -> SampleData {
        SampleData {
            sample_rate: 48_000,
            left: (0..frames).map(|i| i as f32 / frames as f32).collect(),
            right: None,
        }
    }

    fn sine_sample(frames: usize) -> SampleData {
        SampleData {
            sample_rate: 48_000,
            left: (0..frames).map(|i| (i as f32 * 0.05).sin() * 0.8).collect(),
            right: None,
        }
    }

    fn run(v: &mut Voice, sample: &SampleData, s: &SlotSettings, n: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        v.render(sample, s, &mut l, &mut r);
        assert_eq!(l, r, "centred mono");
        l
    }

    #[test]
    fn slices_map_keys_to_segments() {
        let play = SlotPlay::from_meta(&SlotMeta::sliced(36, vec![0, 1000, 3000], 120.0, 4000));
        let s = SlotSettings::default();
        let seg = segment(&play, 37, &s, 48_000, 4000, SR, 120.0).unwrap();
        assert_eq!((seg.start, seg.end, seg.rate), (1000.0, 3000.0, 1.0));
        let last = segment(&play, 38, &s, 48_000, 4000, SR, 120.0).unwrap();
        assert_eq!((last.start, last.end), (3000.0, 4000.0));
        assert!(
            segment(&play, 39, &s, 48_000, 4000, SR, 120.0).is_none(),
            "past the slices"
        );
        assert!(
            segment(&play, 35, &s, 48_000, 4000, SR, 120.0).is_none(),
            "below the zone"
        );
    }

    #[test]
    fn tempo_lock_and_rates() {
        let play = SlotPlay::from_meta(&SlotMeta::sliced(36, vec![0], 100.0, 4000));
        let mut s = SlotSettings::default();
        assert_eq!(
            segment(&play, 36, &s, 48_000, 4000, SR, 120.0)
                .unwrap()
                .rate,
            1.0,
            "lock off"
        );
        s.tempo_lock = true;
        let r = segment(&play, 36, &s, 48_000, 4000, SR, 120.0)
            .unwrap()
            .rate;
        assert!((r - 1.2).abs() < 1e-9);
        let r = segment(&play, 36, &s, 24_000, 4000, SR, 100.0)
            .unwrap()
            .rate;
        assert!(
            (r - 0.5).abs() < 1e-9,
            "source rate is converted on the fly"
        );
        s.tempo_lock = false;
        s.tune = 12;
        let r = segment(&play, 36, &s, 48_000, 4000, SR, 120.0)
            .unwrap()
            .rate;
        assert!((r - 2.0).abs() < 1e-9);
    }

    #[test]
    fn chromatic_keys_and_loops() {
        let mut meta = SlotMeta::chromatic(48, 71, 60, 8000);
        meta.loop_on = true;
        meta.loop_start = 2000;
        meta.loop_end = 6000;
        let play = SlotPlay::from_meta(&meta);
        let s = SlotSettings::default();
        let seg = segment(&play, 72 - 12, &s, 48_000, 8000, SR, 120.0).unwrap();
        assert_eq!(seg.rate, 1.0);
        assert_eq!(seg.looping, Some((2000.0, 6000.0)));
        assert!(
            (segment(&play, 71, &s, 48_000, 8000, SR, 120.0)
                .unwrap()
                .rate
                - 2f64.powf(11.0 / 12.0))
            .abs()
                < 1e-9
        );
        let mut rev = s;
        rev.reverse = true;
        assert_eq!(
            segment(&play, 60, &rev, 48_000, 8000, SR, 120.0)
                .unwrap()
                .looping,
            None,
            "no loop backwards"
        );
    }

    #[test]
    fn a_segment_plays_to_its_end_then_stops_with_a_fade() {
        let sample = sine_sample(4000);
        let s = SlotSettings::default();
        let seg = Segment {
            start: 1000.0,
            end: 2000.0,
            reverse: false,
            looping: None,
            rate: 1.0,
        };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 36, 127, false, 1);
        let out = run(&mut v, &sample, &s, 1200);
        assert!(!v.active, "ended after 1000 frames");
        let peak = out[..900].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.3);
        assert!(
            out[998].abs() < 0.05 * peak,
            "faded at the end: {}",
            out[998]
        );
        assert!(out[1100..].iter().all(|x| *x == 0.0));
    }

    #[test]
    fn reverse_plays_backwards() {
        let sample = ramp_sample(1000);
        let s = SlotSettings {
            reverse: true,
            ..Default::default()
        };
        let seg = Segment {
            start: 0.0,
            end: 1000.0,
            reverse: true,
            looping: None,
            rate: 1.0,
        };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 36, 127, false, 1);
        let out = run(&mut v, &sample, &s, 600);
        assert!(out[200] > out[400] && out[400] > out[550], "falling ramp");
    }

    #[test]
    fn double_rate_halves_the_length() {
        let sample = sine_sample(4000);
        let s = SlotSettings::default();
        let seg = Segment {
            start: 0.0,
            end: 2000.0,
            reverse: false,
            looping: None,
            rate: 2.0,
        };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 36, 127, false, 1);
        run(&mut v, &sample, &s, 990);
        assert!(v.active);
        run(&mut v, &sample, &s, 20);
        assert!(!v.active);
    }

    #[test]
    fn loops_hold_while_the_key_is_down_and_crossfade_smoothly() {
        let sample = sine_sample(8000);
        let s = SlotSettings::default();
        let seg = Segment {
            start: 0.0,
            end: 8000.0,
            reverse: false,
            looping: Some((2000.0, 3000.0)),
            rate: 1.0,
        };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 60, 127, false, 1);
        let out = run(&mut v, &sample, &s, 20_000);
        assert!(v.active, "still looping");
        let max_step = out[5000..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_step < 0.1, "no jumps at the loop point: {max_step}");
    }

    #[test]
    fn a_loop_shorter_than_the_step_keeps_looping() {
        let sample = sine_sample(8000);
        let s = SlotSettings::default();
        for rate in [4.0, 7.3] {
            let seg = Segment {
                start: 0.0,
                end: 8000.0,
                reverse: false,
                looping: Some((2000.0, 2002.0)),
                rate,
            };
            let mut v = Voice::new(SR);
            v.start(&seg, &s, 0, 60, 127, false, 1);
            let out = run(&mut v, &sample, &s, 2000);
            assert!(v.active, "rate {rate}");
            assert!(
                (2000.0..2002.0).contains(&v.pos),
                "rate {rate}: pos {} left the loop",
                v.pos
            );
            // at an integer rate the loop revisits a single frame
            if rate != 4.0 {
                let tail = &out[1900..];
                assert!(
                    tail.iter().any(|x| (x - tail[0]).abs() > 1e-4),
                    "rate {rate}: output is constant"
                );
            }
        }
    }

    #[test]
    fn zero_sustain_decay_frees_the_voice() {
        let sample = sine_sample(48_000);
        let mut s = SlotSettings::default();
        s.set(SlotParam::Sustain, 0.0);
        s.set(SlotParam::Decay, 0.0);
        let seg = Segment {
            start: 0.0,
            end: 48_000.0,
            reverse: false,
            looping: None,
            rate: 1.0,
        };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 36, 127, false, 1);
        run(&mut v, &sample, &s, 10);
        assert!(v.active && v.held);
        run(&mut v, &sample, &s, 40_000);
        assert!(!v.active, "silent forever, so freed");
    }

    #[test]
    fn a_zero_bpm_tempo_lock_cannot_stall_a_voice() {
        let play = SlotPlay::from_meta(&SlotMeta::sliced(36, vec![0], 100.0, 4000));
        let s = SlotSettings {
            tempo_lock: true,
            ..Default::default()
        };
        assert!(segment(&play, 36, &s, 48_000, 4000, SR, 0.0).unwrap().rate > 0.0);
    }

    #[test]
    fn release_and_choke_end_the_voice() {
        let sample = sine_sample(48_000);
        let s = SlotSettings::default();
        let seg = Segment {
            start: 0.0,
            end: 48_000.0,
            reverse: false,
            looping: None,
            rate: 1.0,
        };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 36, 127, false, 1);
        run(&mut v, &sample, &s, 100);
        v.release(0.01);
        assert!(!v.held);
        run(&mut v, &sample, &s, 6000);
        assert!(!v.active);
        let mut c = Voice::new(SR);
        c.start(&seg, &s, 0, 36, 127, false, 1);
        run(&mut c, &sample, &s, 100);
        c.choke();
        run(&mut c, &sample, &s, 3000);
        assert!(!c.active, "choked within 65 ms");
    }

    #[test]
    fn settings_default_and_decode() {
        let mut s = SlotSettings::default();
        assert_eq!((s.tune, s.fine_cents, s.sustain), (0, 0.0, 1.0));
        assert!(!s.reverse && !s.choke && !s.tempo_lock);
        assert!(
            (s.pan_l - 1.0).abs() < 1e-6 && (s.pan_r - 1.0).abs() < 1e-6,
            "centre"
        );
        s.set(SlotParam::Pan, 0.0);
        assert!(s.pan_r.abs() < 1e-6 && s.pan_l > 1.3);
        s.set(SlotParam::Choke, 1.0);
        assert!(s.choke);
    }
}
