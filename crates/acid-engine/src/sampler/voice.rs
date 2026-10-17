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
/// Level/Pan changes glide: a gain step of 1.0 takes this long.
const GAIN_RAMP_S: f32 = 0.005;
/// A stolen voice's old sound fades out over this long.
const DECLICK_S: f32 = 0.002;

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

/// The segment `key` plays in a slot, or `None` outside its zone. It
/// stays inside the sample's `frames` whatever the play's markers say.
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
            // an invalid slot's slices past the audio clamp to its end
            let start = play.slices[i].min(frames);
            let end = if i + 1 < play.slice_count {
                play.slices[i + 1].min(frames)
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
    /// The note is over; only a steal's declick is still fading.
    Tail,
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
    /// First frame of the segment (the loop crossfade never reads before it).
    start_frame: f64,
    reverse: bool,
    looping: bool,
    loop_start: f64,
    loop_end: f64,
    env: f32,
    stage: Stage,
    release_coef: f32,
    /// Velocity factor; the slot's Level is applied live in `render`.
    velocity: f32,
    /// Level × pan per side, gliding toward the slot's settings.
    gain: [f32; 2],
    /// Set by `start`: the next render jumps `gain` to the settings.
    gain_fresh: bool,
    filter: [Svf; 2],
    /// Whether the filter ran in the previous render.
    filter_was_on: bool,
    /// This voice's last output sample (left, right).
    last_out: [f32; 2],
    /// A stolen voice's old output, fading out under the new note.
    declick: [f32; 2],
    declick_left: u32,
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
            start_frame: 0.0,
            reverse: false,
            looping: false,
            loop_start: 0.0,
            loop_end: 0.0,
            env: 0.0,
            stage: Stage::Off,
            release_coef: 0.0,
            velocity: 0.0,
            gain: [0.0; 2],
            gain_fresh: true,
            filter: [Svf::new(sample_rate), Svf::new(sample_rate)],
            filter_was_on: false,
            last_out: [0.0; 2],
            declick: [0.0; 2],
            declick_left: 0,
            sample_rate,
        }
    }

    /// Starts a note. Starting over a sounding voice (a steal) fades its
    /// old sound out over `DECLICK_S` instead of cutting it.
    pub fn start(&mut self, seg: &Segment, slot: usize, key: u8, vel: u8, live: bool, age: u32) {
        if self.active {
            self.declick = self.last_out;
            self.declick_left = (DECLICK_S * self.sample_rate).max(1.0) as u32;
        } else {
            self.declick = [0.0; 2];
            self.declick_left = 0;
        }
        self.last_out = [0.0; 2];
        self.gain_fresh = true;
        self.start_frame = seg.start;
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
        self.velocity = vel.min(127) as f32 / 127.0;
        // A note starts from silence: a filter on from its start runs cold
        // (not primed), while one switched on later is primed.
        self.filter = [Svf::new(self.sample_rate), Svf::new(self.sample_rate)];
        self.filter_was_on = true;
    }

    pub fn release(&mut self, seconds: f32) {
        if self.active && !matches!(self.stage, Stage::Tail | Stage::Off) {
            self.held = false;
            self.stage = Stage::Release;
            self.release_coef = (-1.0 / (seconds.max(1e-4) * self.sample_rate)).exp();
        }
    }

    /// Releases so that a note at full level is silent after `seconds`
    /// (a release's time constant is `seconds` instead).
    pub fn fade(&mut self, seconds: f32) {
        self.release(seconds);
        if self.stage == Stage::Release {
            self.release_coef = (SILENT.ln() / (seconds.max(1e-4) * self.sample_rate)).exp();
        }
    }

    /// Still sounding, but only the fade of a stolen note: not held, and
    /// the first voice to take for a new note.
    pub fn tail_only(&self) -> bool {
        self.active && self.stage == Stage::Tail
    }

    pub fn choke(&mut self) {
        self.release(CHOKE_S);
    }

    pub fn kill(&mut self) {
        self.active = false;
        self.held = false;
        self.stage = Stage::Off;
    }

    /// The voice's own sound has ended. A steal's declick still fading
    /// is played out first (over this block and, if needed, the next ones,
    /// which come back here through `Stage::Tail`), so a very short new
    /// note can't cut it off; then the voice stops.
    fn end_sound(&mut self, out_l: &mut [f32], out_r: &mut [f32], declick_len: f32) {
        for (l, r) in out_l.iter_mut().zip(out_r.iter_mut()) {
            if self.declick_left == 0 {
                break;
            }
            let fade = self.declick_left as f32 / declick_len;
            self.last_out = [self.declick[0] * fade, self.declick[1] * fade];
            *l += self.last_out[0];
            *r += self.last_out[1];
            self.declick_left -= 1;
        }
        if self.declick_left == 0 {
            self.kill();
        } else {
            self.held = false;
            self.stage = Stage::Tail;
        }
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
        // Switched on mid-note: start from this render's first input, not
        // from the state it had when it was last on, nor from silence.
        let mut prime = filter_on && !self.filter_was_on;
        if filter_on {
            if prime {
                self.filter = [Svf::new(sr), Svf::new(sr)];
            }
            for f in &mut self.filter {
                f.set(s.cutoff_hz, s.resonance);
            }
        }
        self.filter_was_on = filter_on;
        let target = [s.level_gain * s.pan_l, s.level_gain * s.pan_r];
        if self.gain_fresh {
            self.gain = target;
            self.gain_fresh = false;
        }
        let max_gain_step = 1.0 / (GAIN_RAMP_S * sr).max(1.0);
        let declick_len = (DECLICK_S * sr).max(1.0);
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
                        self.end_sound(&mut out_l[i..], &mut out_r[i..], declick_len);
                        return;
                    }
                }
                Stage::Release => {
                    self.env *= self.release_coef;
                    if self.env < SILENT {
                        self.end_sound(&mut out_l[i..], &mut out_r[i..], declick_len);
                        return;
                    }
                }
                Stage::Tail => {
                    self.end_sound(&mut out_l[i..], &mut out_r[i..], declick_len);
                    return;
                }
                Stage::Off => return,
            }
            let past_end = if self.reverse {
                self.pos < self.end
            } else {
                self.pos >= self.end
            };
            if past_end && !self.looping {
                self.end_sound(&mut out_l[i..], &mut out_r[i..], declick_len);
                return;
            }
            let (mut l, mut r) = sample.read(self.pos);
            if self.looping && self.pos > self.loop_end - xfade {
                let a = ((self.loop_end - self.pos) / xfade) as f32;
                // the pre-loop window never reaches before the play start
                let (l2, r2) = sample.read((self.pos - loop_len).max(self.start_frame));
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
                if prime {
                    self.filter[0].prime(l);
                    self.filter[1].prime(r);
                    prime = false;
                }
                l = self.filter[0].process(l);
                r = self.filter[1].process(r);
            }
            for (g, t) in self.gain.iter_mut().zip(target) {
                let d = t - *g;
                *g = if d.abs() <= max_gain_step {
                    t
                } else {
                    *g + max_gain_step.copysign(d)
                };
            }
            let g = self.env * edge * self.velocity;
            let (mut vl, mut vr) = (l * g * self.gain[0], r * g * self.gain[1]);
            if self.declick_left > 0 {
                let fade = self.declick_left as f32 / declick_len;
                vl += self.declick[0] * fade;
                vr += self.declick[1] * fade;
                self.declick_left -= 1;
            }
            self.last_out = [vl, vr];
            out_l[i] += vl;
            out_r[i] += vr;
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
    use crate::testutil::{all_finite, peak};

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

    fn run_stereo(
        v: &mut Voice,
        sample: &SampleData,
        s: &SlotSettings,
        n: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        v.render(sample, s, &mut l, &mut r);
        (l, r)
    }

    fn whole(frames: usize) -> Segment {
        Segment {
            start: 0.0,
            end: frames as f64,
            reverse: false,
            looping: None,
            rate: 1.0,
        }
    }

    fn constant(frames: usize, x: f32) -> SampleData {
        SampleData {
            sample_rate: 48_000,
            left: vec![x; frames].into(),
            right: None,
        }
    }

    fn max_step(x: &[f32]) -> f32 {
        x.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    #[test]
    fn the_filter_darkens_a_bright_sample() {
        let bright = SampleData {
            sample_rate: 48_000,
            left: (0..48_000)
                .map(|i| if i % 2 == 0 { 0.8 } else { -0.8 })
                .collect(),
            right: None,
        };
        let open = SlotSettings::default();
        let mut dark = open;
        dark.set(SlotParam::Cutoff, 0.1);
        let (mut a, mut b) = (Voice::new(SR), Voice::new(SR));
        a.start(&whole(48_000), 0, 36, 127, false, 1);
        b.start(&whole(48_000), 0, 36, 127, false, 1);
        let dry = run(&mut a, &bright, &open, 4800);
        let wet = run(&mut b, &bright, &dark, 4800);
        assert!(all_finite(&wet));
        assert!(
            rms(&wet[480..]) < 0.05 * rms(&dry[480..]),
            "{} vs {}",
            rms(&wet[480..]),
            rms(&dry[480..])
        );
        let mut resonant = open;
        resonant.set(SlotParam::Cutoff, 0.5);
        resonant.set(SlotParam::Resonance, 1.0);
        let mut c = Voice::new(SR);
        c.start(&whole(48_000), 0, 36, 127, false, 1);
        assert!(all_finite(&run(
            &mut c,
            &sine_sample(48_000),
            &resonant,
            4800
        )));
    }

    #[test]
    fn a_re_enabled_filter_starts_clean() {
        let sample = sine_sample(48_000);
        let open = SlotSettings::default();
        let mut dark = open;
        dark.set(SlotParam::Cutoff, 0.1);
        let mut bright = open;
        bright.set(SlotParam::Cutoff, 0.9);
        // `a` runs its filter first; `b` never has until the last block
        let (mut a, mut b) = (Voice::new(SR), Voice::new(SR));
        a.start(&whole(48_000), 0, 36, 127, false, 1);
        b.start(&whole(48_000), 0, 36, 127, false, 1);
        run(&mut a, &sample, &dark, 1000);
        run(&mut b, &sample, &open, 1000);
        run(&mut a, &sample, &open, 500);
        run(&mut b, &sample, &open, 500);
        let after = run(&mut a, &sample, &bright, 200);
        let fresh = run(&mut b, &sample, &bright, 200);
        for (x, y) in after.iter().zip(&fresh) {
            assert!((x - y).abs() < 1e-6, "stale filter state: {x} vs {y}");
        }
    }

    #[test]
    fn a_filter_switched_on_mid_note_starts_at_the_dry_level() {
        let open = SlotSettings::default();
        let mut barely = open;
        barely.set(SlotParam::Cutoff, 0.998);
        for (sample, tolerance) in [(constant(48_000, 0.8), 1e-4), (sine_sample(48_000), 0.02)] {
            let (mut dry, mut wet) = (Voice::new(SR), Voice::new(SR));
            dry.start(&whole(48_000), 0, 36, 127, false, 1);
            wet.start(&whole(48_000), 0, 36, 127, false, 1);
            run(&mut dry, &sample, &open, 1000);
            run(&mut wet, &sample, &open, 1000);
            let want = run(&mut dry, &sample, &open, 64);
            let got = run(&mut wet, &sample, &barely, 64);
            for (i, (x, y)) in got.iter().zip(&want).enumerate() {
                assert!((x - y).abs() < tolerance, "sample {i}: {x} vs dry {y}");
            }
        }
    }

    #[test]
    fn a_dark_note_starting_at_a_peak_opens_without_a_thump() {
        // a cosine: the note's first sample is the waveform's peak
        let sample = SampleData {
            sample_rate: 48_000,
            left: (0..48_000).map(|i| (i as f32 * 0.05).cos() * 0.8).collect(),
            right: None,
        };
        let mut dark = SlotSettings::default();
        dark.set(SlotParam::Cutoff, 0.0);
        let mut v = Voice::new(SR);
        v.start(&whole(48_000), 0, 36, 127, false, 1);
        let out = run(&mut v, &sample, &dark, 28_800);
        let onset = peak(&out[..600]);
        let steady = peak(&out[24_000..]);
        assert!(
            onset < 3.0 * steady,
            "onset {onset} vs steady {steady}: primed at note start"
        );
    }

    #[test]
    fn stereo_samples_and_pan_reach_the_outputs() {
        let stereo = SampleData {
            sample_rate: 48_000,
            left: vec![0.5; 4800].into(),
            right: Some(vec![-0.5; 4800].into()),
        };
        let centre = SlotSettings::default();
        let mut v = Voice::new(SR);
        v.start(&whole(4800), 0, 36, 127, false, 1);
        let (l, r) = run_stereo(&mut v, &stereo, &centre, 2000);
        assert!(l[1000] > 0.1, "left side: {}", l[1000]);
        assert!(
            (l[1000] + r[1000]).abs() < 1e-6,
            "right is the left negated"
        );
        let mono = constant(4800, 0.5);
        let mut c = Voice::new(SR);
        c.start(&whole(4800), 0, 36, 127, false, 1);
        let (cl, _) = run_stereo(&mut c, &mono, &centre, 2000);
        let mut hard_left = centre;
        hard_left.set(SlotParam::Pan, 0.0);
        let mut p = Voice::new(SR);
        p.start(&whole(4800), 0, 36, 127, false, 1);
        let (pl, pr) = run_stereo(&mut p, &mono, &hard_left, 2000);
        assert!(pr[1000..].iter().all(|x| x.abs() < 1e-6), "right is silent");
        assert!(
            (pl[1000] / cl[1000] - std::f32::consts::SQRT_2).abs() < 1e-3,
            "{} vs {}",
            pl[1000],
            cl[1000]
        );
    }

    #[test]
    fn the_loop_crossfade_never_reads_before_the_play_start() {
        // loud junk before the play start, a quiet tone after it
        let sample = SampleData {
            sample_rate: 48_000,
            left: (0..8000)
                .map(|i| {
                    if i < 1000 {
                        5.0
                    } else {
                        (i as f32 * 0.05).sin() * 0.5
                    }
                })
                .collect(),
            right: None,
        };
        let s = SlotSettings::default();
        for loop_start in [1000.0, 1100.0] {
            let seg = Segment {
                start: 1000.0,
                end: 8000.0,
                reverse: false,
                looping: Some((loop_start, 3000.0)),
                rate: 1.0,
            };
            let mut v = Voice::new(SR);
            v.start(&seg, 0, 60, 127, false, 1);
            let out = run(&mut v, &sample, &s, 10_000);
            let p = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(p < 0.6, "loop at {loop_start}: read the junk ({p})");
        }
    }

    #[test]
    fn a_loop_from_the_play_start_wraps_without_a_click() {
        let sample = sine_sample(8000);
        let s = SlotSettings::default();
        let seg = Segment {
            start: 0.0,
            end: 8000.0,
            reverse: false,
            looping: Some((0.0, 3000.0)),
            rate: 1.0,
        };
        let mut v = Voice::new(SR);
        v.start(&seg, 0, 60, 127, false, 1);
        let out = run(&mut v, &sample, &s, 10_000);
        let steady = max_step(&out[100..2000]);
        // wraps at 3000, 6000 and 9000 output samples
        for wrap in [3000, 6000, 9000] {
            let near = max_step(&out[wrap - 600..wrap + 50]);
            assert!(
                near < 1.5 * steady,
                "click at the wrap near {wrap}: {near} vs {steady}"
            );
        }
    }

    #[test]
    fn level_jumps_are_smoothed() {
        let sample = constant(48_000, 0.8);
        let silent = SlotSettings {
            level_gain: 0.0,
            ..Default::default()
        };
        let mut full = silent;
        full.set(SlotParam::Level, 1.0);
        let mut v = Voice::new(SR);
        v.start(&whole(48_000), 0, 36, 127, false, 1);
        let before = run(&mut v, &sample, &silent, 1000);
        assert!(before.iter().all(|x| *x == 0.0));
        let after = run(&mut v, &sample, &full, 1000);
        assert!(after[0].abs() < 0.05, "no jump: {}", after[0]);
        assert!(max_step(&after) < 0.05, "{}", max_step(&after));
        assert!(
            (after[999] - 0.8 * 1.25).abs() < 1e-4,
            "settles: {}",
            after[999]
        );
    }

    #[test]
    fn a_restarted_voice_fades_out_its_old_sound() {
        let sample = constant(48_000, 0.8);
        let s = SlotSettings::default();
        let mut v = Voice::new(SR);
        v.start(&whole(48_000), 0, 36, 127, false, 1);
        let before = run(&mut v, &sample, &s, 2000);
        let level = before[1999];
        assert!(level > 0.3);
        v.start(&whole(48_000), 0, 40, 127, false, 2);
        let after = run(&mut v, &sample, &s, 500);
        let mut joined = vec![level];
        joined.extend_from_slice(&after);
        assert!(
            max_step(&joined) < 0.1 * level,
            "steal click: {}",
            max_step(&joined)
        );
        assert!(
            after[100] > 0.0 && after[499] > 0.9 * level,
            "the new note plays"
        );
    }

    #[test]
    fn a_steal_by_a_very_short_note_still_fades_the_old_sound() {
        let sample = constant(48_000, 0.8);
        let s = SlotSettings::default();
        let mut v = Voice::new(SR);
        v.start(&whole(48_000), 0, 36, 127, false, 1);
        let level = run(&mut v, &sample, &s, 2000)[1999];
        // The new note ends after 20 frames, well inside the 2 ms fade.
        v.start(&whole(20), 0, 40, 127, false, 2);
        let mut joined = vec![level];
        joined.extend(run(&mut v, &sample, &s, 50));
        joined.extend(run(&mut v, &sample, &s, 450));
        assert!(
            max_step(&joined) < 0.1 * level,
            "cut mid-fade: {}",
            max_step(&joined)
        );
        assert!(!v.active, "the voice stops once the fade is done");
        assert_eq!(*joined.last().unwrap(), 0.0);
    }

    #[test]
    fn an_invalid_slot_plays_inside_its_pcm() {
        use crate::sampler::slot::SlotData;
        let audio: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
        let mut sliced = SlotMeta::sliced(36, vec![0, 500, 2000, 3000, 1500], 120.0, 5000);
        sliced.end = 9000;
        let mut chromatic = SlotMeta::chromatic(60, 72, 60, 9000);
        chromatic.loop_on = true;
        chromatic.loop_start = 400;
        chromatic.loop_end = 7000;
        let s = SlotSettings::default();
        for meta in [sliced, chromatic] {
            let data = SlotData::new("bad", 48_000, 1, &audio, meta);
            assert!(!data.is_valid());
            let sample = data.to_sample();
            let frames = sample.frames();
            assert_eq!(frames, 1000);
            let play = SlotPlay::from_meta(&data.meta);
            for key in 0..=127 {
                let Some(seg) = segment(&play, key, &s, 48_000, frames, SR, 120.0) else {
                    continue;
                };
                let inside = |x: f64| (0.0..=frames as f64).contains(&x);
                assert!(
                    inside(seg.start) && inside(seg.end) && seg.start < seg.end,
                    "key {key}: {seg:?}"
                );
                if let Some((a, b)) = seg.looping {
                    assert!(inside(a) && inside(b) && a < b, "key {key}: {seg:?}");
                }
                // The bounds above are the guard: rendering stays finite even
                // without them, since `SampleData::read` clamps.
                for reverse in [false, true] {
                    let mut v = Voice::new(SR);
                    v.start(&Segment { reverse, ..seg }, 0, key, 127, false, 1);
                    assert!(all_finite(&run(&mut v, &sample, &s, 4800)));
                }
            }
        }
    }

    #[test]
    fn a_voice_playing_only_its_steal_fade_is_not_held() {
        let sample = constant(48_000, 0.8);
        let s = SlotSettings::default();
        let mut v = Voice::new(SR);
        v.start(&whole(48_000), 0, 36, 127, false, 1);
        let level = run(&mut v, &sample, &s, 2000)[1999];
        v.start(&whole(20), 0, 40, 127, false, 2);
        let mut joined = vec![level];
        joined.extend(run(&mut v, &sample, &s, 50));
        assert!(
            v.active && !v.held,
            "its own note is over; the fade goes on"
        );
        assert!(v.tail_only());
        v.release(1.0);
        assert!(v.tail_only() && !v.held, "a release leaves the fade alone");
        joined.extend(run(&mut v, &sample, &s, 450));
        assert!(max_step(&joined) < 0.1 * level, "no click");
        assert!(!v.active && !v.tail_only());
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
    fn level_changes_reach_a_sounding_voice() {
        let sample = sine_sample(48_000);
        let s = SlotSettings::default();
        let seg = segment(
            &SlotPlay::from_meta(&SlotMeta::chromatic(36, 60, 36, 48_000)),
            36,
            &s,
            48_000,
            48_000,
            SR,
            120.0,
        )
        .unwrap();
        let (mut a, mut b) = (Voice::new(SR), Voice::new(SR));
        a.start(&seg, 0, 36, 100, false, 1);
        b.start(&seg, 0, 36, 100, false, 1);
        assert_eq!(run(&mut a, &sample, &s, 500), run(&mut b, &sample, &s, 500));
        let mut quieter = s;
        quieter.level_gain = s.level_gain * 0.5;
        // the change glides in over a few ms, then holds
        run(&mut a, &sample, &s, 500);
        run(&mut b, &sample, &quieter, 500);
        let full = run(&mut a, &sample, &s, 500);
        let half = run(&mut b, &sample, &quieter, 500);
        assert!(full.iter().any(|x| x.abs() > 0.1), "the voice sounds");
        for (f, h) in full.iter().zip(&half) {
            assert!((f * 0.5 - h).abs() < 1e-6, "{f} vs {h}");
        }
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
        v.start(&seg, 0, 36, 127, false, 1);
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
        v.start(&seg, 0, 36, 127, false, 1);
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
        v.start(&seg, 0, 36, 127, false, 1);
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
        v.start(&seg, 0, 60, 127, false, 1);
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
            v.start(&seg, 0, 60, 127, false, 1);
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
        v.start(&seg, 0, 36, 127, false, 1);
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
        v.start(&seg, 0, 36, 127, false, 1);
        run(&mut v, &sample, &s, 100);
        v.release(0.01);
        assert!(!v.held);
        run(&mut v, &sample, &s, 6000);
        assert!(!v.active);
        let mut c = Voice::new(SR);
        c.start(&seg, 0, 36, 127, false, 1);
        run(&mut c, &sample, &s, 100);
        c.choke();
        run(&mut c, &sample, &s, 3000);
        assert!(!c.active, "choked within 65 ms");
    }

    #[test]
    fn a_fade_is_silent_within_its_time() {
        let sample = constant(48_000, 0.5);
        let s = SlotSettings::default();
        let mut v = Voice::new(SR);
        v.start(&whole(48_000), 0, 36, 127, false, 1);
        run(&mut v, &sample, &s, 1_000);
        v.fade(0.005);
        assert!(!v.held);
        let out = run(&mut v, &sample, &s, 230);
        assert!(v.active && out[229] > 0.0, "still fading at 4.8 ms");
        run(&mut v, &sample, &s, 20);
        assert!(!v.active, "silent by 5.2 ms");
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
