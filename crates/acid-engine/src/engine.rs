//! The whole groovebox: two bass synths, two drum kits, the mixer and the
//! sequencer, rendered sample-accurately. process() is real-time safe:
//! no allocation, locking or I/O.

use crate::drums::{DrumKit, KitKind};
use crate::mixer::{Mixer, StereoBuf};
use crate::params::{PARAM_COUNT, ParamId};
use crate::project::{DrumPattern, PATTERNS, Project, Song, SynthPattern};
use crate::sequencer::{Mode, SeqEvent, Sequencer};
use crate::song::MAX_BARS;
use crate::synth::BassSynth;

/// Upper bound on sequencer ticks fired without time advancing; guards
/// against a sequencer that never moves forward.
const MAX_TICKS_PER_POSITION: usize = 64;
/// Sequencer event buffer size: never grows on the audio thread.
const EVENT_RESERVE: usize = 64 + 2 * PARAM_COUNT;

pub struct Engine {
    sample_rate: f32,
    synths: [BassSynth; 2],
    kits: [DrumKit; 2],
    mixer: Mixer,
    seq: Sequencer,
    synth_patterns: [Vec<SynthPattern>; 2],
    drum_patterns: [Vec<DrumPattern>; 2],
    chans: [StereoBuf; 4],
    events: Vec<SeqEvent>,
    /// Knob values set through `set_param`, indexed by `ParamId::index`.
    knobs: Vec<f32>,
    /// Effective value per param: the knob, or song automation / live
    /// record value. Lane restores return here.
    base: Vec<f32>,
    /// Params held by the user while recording: automation skips them.
    touched: Vec<bool>,
    /// `ParamId::all()`, so knobs can be re-applied without allocating.
    ids: Vec<ParamId>,
    guard_fired: bool,
    song: crate::project::Song,
}

impl Engine {
    pub const MAX_BLOCK: usize = 512;

    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            synths: [BassSynth::new(sample_rate), BassSynth::new(sample_rate)],
            kits: [
                DrumKit::new(KitKind::K808, sample_rate),
                DrumKit::new(KitKind::K909, sample_rate),
            ],
            mixer: Mixer::new(sample_rate, Self::MAX_BLOCK),
            seq: Sequencer::new(sample_rate),
            synth_patterns: [
                vec![SynthPattern::default(); PATTERNS],
                vec![SynthPattern::default(); PATTERNS],
            ],
            drum_patterns: [
                vec![DrumPattern::default(); PATTERNS],
                vec![DrumPattern::default(); PATTERNS],
            ],
            chans: std::array::from_fn(|_| StereoBuf::new(Self::MAX_BLOCK)),
            events: Vec::with_capacity(EVENT_RESERVE),
            knobs: ParamId::all().iter().map(|id| id.info().default).collect(),
            base: ParamId::all().iter().map(|id| id.info().default).collect(),
            touched: vec![false; PARAM_COUNT],
            ids: ParamId::all(),
            guard_fired: false,
            song: crate::project::Song::default(),
        }
    }

    /// Not real-time safe (clones patterns); call while stopped or from
    /// a non-audio thread before handing the engine to the audio thread.
    pub fn load_project(&mut self, p: &Project) {
        for s in 0..2 {
            self.synth_patterns[s] = p.synths[s].patterns.clone();
            self.drum_patterns[s] = p.drums[s].patterns.clone();
        }
        for id in ParamId::all() {
            self.set_param(id, p.param(id));
        }
        self.song = p.song.clone();
        self.set_bpm(p.bpm);
        self.set_swing(p.swing);
        for (m, &slot) in p.selected.iter().enumerate() {
            self.seq.select(m, slot);
        }
    }

    /// Out-of-range ids and non-finite values are ignored; values are
    /// clamped to 0..=1. The value is remembered as the knob position.
    pub fn set_param(&mut self, id: ParamId, v: f32) {
        let Some(i) = id.index() else { return };
        if !v.is_finite() {
            return;
        }
        let v = v.clamp(0.0, 1.0);
        self.knobs[i] = v;
        self.base[i] = v;
        self.apply_param(id, v);
    }

    /// Applies a value without remembering it (param-lane overrides).
    fn apply_param(&mut self, id: ParamId, v: f32) {
        if !id.is_valid() || !v.is_finite() {
            return;
        }
        let v = v.clamp(0.0, 1.0);
        match id {
            ParamId::Synth(s, p) => self.synths[s as usize].set_param(p, v),
            ParamId::Drum(k, i, p) => self.kits[k as usize].set_param(i as usize, p, v),
            ParamId::Kit(k, p) => self.kits[k as usize].set_kit_param(p, v),
            _ => self.mixer.set_param(id, v),
        }
    }

    fn restore_param(&mut self, id: ParamId) {
        if let Some(i) = id.index() {
            let v = self.base[i];
            self.apply_param(id, v);
        }
    }

    fn restore_all_knobs(&mut self) {
        for i in 0..PARAM_COUNT {
            let (id, v) = (self.ids[i], self.knobs[i]);
            self.apply_param(id, v);
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.seq.set_bpm(bpm);
        self.mixer.set_bpm(self.seq.bpm());
    }

    pub fn set_swing(&mut self, swing: f32) {
        self.seq.set_swing(swing);
    }

    pub fn play(&mut self) {
        self.seq.play();
    }

    pub fn stop(&mut self) {
        self.seq.stop();
        for s in &mut self.synths {
            s.note_off();
        }
        self.base.copy_from_slice(&self.knobs);
        self.touched.fill(false);
        self.restore_all_knobs();
    }

    /// Plays a value without changing the saved knob (record mode).
    pub fn set_param_live(&mut self, id: ParamId, v: f32) {
        let Some(i) = id.index() else { return };
        if !v.is_finite() {
            return;
        }
        let v = v.clamp(0.0, 1.0);
        self.base[i] = v;
        self.apply_param(id, v);
    }

    /// While touched, song automation for `id` is not applied.
    pub fn touch(&mut self, id: ParamId, on: bool) {
        if let Some(i) = id.index() {
            self.touched[i] = on;
        }
    }

    fn apply_automation(&mut self, id: ParamId, v: f32) {
        let Some(i) = id.index() else { return };
        if self.touched[i] || !v.is_finite() {
            return;
        }
        self.base[i] = v.clamp(0.0, 1.0);
        self.apply_param(id, self.base[i]);
    }

    /// Swaps in a new song (real-time safe). An invalid song leaves both
    /// sides untouched and returns false.
    pub fn swap_song(&mut self, song: &mut Song) -> bool {
        let refs_ok = song
            .bars
            .iter()
            .flat_map(|b| b.patterns)
            .flatten()
            .all(|r| (r.0 as usize) < PATTERNS);
        let lanes_ok = song.automation.len() <= PARAM_COUNT
            && song.automation.iter().all(|l| l.param.is_valid());
        if song.bars.len() > MAX_BARS || !refs_ok || !lanes_ok {
            return false;
        }
        std::mem::swap(&mut self.song, song);
        true
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.seq.set_mode(mode);
    }

    pub fn mode(&self) -> Mode {
        self.seq.mode()
    }

    pub fn set_song_start(&mut self, bar: usize) {
        self.seq.set_song_start(bar);
    }

    pub fn song_bar(&self) -> usize {
        self.seq.song_bar()
    }

    pub fn song_step(&self) -> u32 {
        self.seq.song_step()
    }

    pub fn is_playing(&self) -> bool {
        self.seq.is_playing()
    }

    /// Ignores `machine >= 4` and `slot >= 128`.
    pub fn select_pattern(&mut self, machine: usize, slot: u8) {
        if machine < 4 && (slot as usize) < PATTERNS {
            self.seq.select(machine, slot);
        }
    }

    /// Builds an engine with a project loaded. Allocates; call off the
    /// audio thread.
    pub fn from_project(sample_rate: f32, p: &Project) -> Engine {
        let mut e = Engine::new(sample_rate);
        e.load_project(p);
        e
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Swaps `p` with the stored pattern (real-time safe). An invalid
    /// target or malformed pattern leaves both untouched and returns false.
    pub fn swap_synth_pattern(&mut self, synth: usize, slot: u8, p: &mut SynthPattern) -> bool {
        if synth >= 2 || slot as usize >= PATTERNS || !p.is_well_formed() {
            return false;
        }
        std::mem::swap(&mut self.synth_patterns[synth][slot as usize], p);
        true
    }

    pub fn swap_drum_pattern(&mut self, kit: usize, slot: u8, p: &mut DrumPattern) -> bool {
        if kit >= 2 || slot as usize >= PATTERNS || !p.is_well_formed() {
            return false;
        }
        std::mem::swap(&mut self.drum_patterns[kit][slot as usize], p);
        true
    }

    /// Stores `p` and returns the old pattern. An invalid target or a
    /// malformed pattern is not stored: `p` is handed back unchanged.
    pub fn set_synth_pattern(
        &mut self,
        synth: usize,
        slot: u8,
        mut p: SynthPattern,
    ) -> SynthPattern {
        self.swap_synth_pattern(synth, slot, &mut p);
        p
    }

    pub fn set_drum_pattern(&mut self, kit: usize, slot: u8, mut p: DrumPattern) -> DrumPattern {
        self.swap_drum_pattern(kit, slot, &mut p);
        p
    }

    pub fn queued_patterns(&self) -> [Option<u8>; 4] {
        std::array::from_fn(|m| self.seq.queued_pattern(m))
    }

    /// See `Mixer::take_meters`.
    pub fn take_meters(&mut self) -> [f32; 5] {
        self.mixer.take_meters()
    }

    /// True once if the mixer's NaN guard fired since the last call.
    pub fn take_guard_fired(&mut self) -> bool {
        std::mem::take(&mut self.guard_fired)
    }

    pub fn playhead(&self) -> [usize; 4] {
        std::array::from_fn(|m| self.seq.playhead(m))
    }

    pub fn current_patterns(&self) -> [u8; 4] {
        std::array::from_fn(|m| self.seq.current_pattern(m))
    }

    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        debug_assert_eq!(out_l.len(), out_r.len());
        let total = out_l.len().min(out_r.len());
        let mut offset = 0;
        while offset < total {
            let n = (total - offset).min(Self::MAX_BLOCK);
            self.process_block(
                &mut out_l[offset..offset + n],
                &mut out_r[offset..offset + n],
            );
            offset += n;
        }
    }

    fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len();
        for c in &mut self.chans {
            c.l[..n].fill(0.0);
            c.r[..n].fill(0.0);
        }
        let mut pos = 0;
        let mut ticks_here = 0;
        while pos < n {
            let until = self.seq.samples_to_next_tick();
            if until == 0 && ticks_here < MAX_TICKS_PER_POSITION {
                ticks_here += 1;
                self.events.clear();
                self.seq.tick(
                    [&self.synth_patterns[0], &self.synth_patterns[1]],
                    [&self.drum_patterns[0], &self.drum_patterns[1]],
                    &self.song,
                    &mut self.events,
                );
                for i in 0..self.events.len() {
                    match self.events[i] {
                        SeqEvent::Note {
                            synth,
                            midi,
                            accent,
                            legato,
                        } => self.synths[synth].note_on(midi, accent, legato),
                        SeqEvent::NoteOff { synth } => self.synths[synth].note_off(),
                        SeqEvent::Drum { kit, inst, accent } => {
                            self.kits[kit].trigger(inst, accent)
                        }
                        SeqEvent::Param { id, value } => self.apply_param(id, value),
                        SeqEvent::ParamRestore { id } => self.restore_param(id),
                        SeqEvent::Automation { id, value } => self.apply_automation(id, value),
                    }
                }
                if self.seq.take_ended() {
                    self.stop();
                }
                continue;
            }
            // Past the tick bound (a broken tempo) just render on.
            ticks_here = 0;
            let run = if until == 0 {
                n - pos
            } else {
                until.min(n - pos)
            };
            let span = pos..pos + run;
            for s in 0..2 {
                let c = &mut self.chans[s];
                self.synths[s].render(&mut c.l[span.clone()], &mut c.r[span.clone()]);
            }
            for k in 0..2 {
                let c = &mut self.chans[2 + k];
                self.kits[k].render(&mut c.l[span.clone()], &mut c.r[span.clone()]);
            }
            self.seq.advance(run);
            pos += run;
        }
        if self.mixer.process(&mut self.chans, n, out_l, out_r) {
            self.guard_fired = true;
            for s in &mut self.synths {
                s.reset();
            }
            for k in &mut self.kits {
                k.reset();
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one field of a default pattern
mod tests {
    use super::*;
    use crate::params::{ChannelParam, DrumParam, KitParam, MasterParam, SynthParam};
    use crate::testutil::{all_finite, peak, rms};

    const SR: f32 = 44_100.0;

    fn render(e: &mut Engine, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        e.process(&mut l, &mut r);
        (l, r)
    }

    fn demo_engine() -> Engine {
        let mut e = Engine::new(SR);
        e.load_project(&crate::demo::demo_project());
        e
    }

    #[test]
    fn stopped_engine_is_silent() {
        let mut e = demo_engine();
        let (l, _) = render(&mut e, 0.5);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn playing_demo_makes_bounded_sound() {
        let mut e = demo_engine();
        e.play();
        let (l, r) = render(&mut e, 4.0);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(rms(&l) > 0.02, "rms = {}", rms(&l));
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn odd_buffer_sizes_render_identically() {
        let mut a = demo_engine();
        let mut b = demo_engine();
        a.play();
        b.play();
        let (la, _) = render(&mut a, 1.0);
        let mut lb = Vec::new();
        let mut remaining = la.len();
        while remaining > 0 {
            let n = remaining.min(37);
            let mut l = vec![0.0; n];
            let mut r = vec![0.0; n];
            b.process(&mut l, &mut r);
            lb.extend(l);
            remaining -= n;
        }
        for (x, y) in la.iter().zip(&lb) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn master_level_zero_is_silent() {
        let mut e = demo_engine();
        e.set_param(ParamId::Master(MasterParam::Level), 0.0);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn stop_closes_synth_gates_and_tails_die() {
        let mut e = demo_engine();
        for c in 0..4 {
            e.set_param(ParamId::Channel(c, ChannelParam::DelaySend), 0.0);
            e.set_param(ParamId::Channel(c, ChannelParam::ReverbSend), 0.0);
        }
        e.play();
        render(&mut e, 1.0);
        e.stop();
        render(&mut e, 3.0);
        let (l, _) = render(&mut e, 0.2);
        assert!(peak(&l) < 1e-3, "peak = {}", peak(&l));
    }

    #[test]
    fn pattern_replacement_returns_old_pattern() {
        let mut e = demo_engine();
        let old = e.set_synth_pattern(0, 0, SynthPattern::default());
        assert!(
            old.steps
                .iter()
                .any(|s| s.gate != crate::project::Gate::Off)
        );
        let old_drums = e.set_drum_pattern(0, 0, DrumPattern::default());
        assert!(
            old_drums
                .steps
                .iter()
                .flatten()
                .any(|h| *h != crate::project::DrumHit::Off)
        );
    }

    #[test]
    fn nan_tempo_cannot_hang_or_poison() {
        let mut e = demo_engine();
        e.set_bpm(f32::NAN);
        e.set_swing(f32::NAN);
        e.play();
        let (l, r) = render(&mut e, 0.5);
        assert!(all_finite(&l) && all_finite(&r));
    }

    #[test]
    fn broken_clock_cannot_hang_process() {
        let mut e = demo_engine();
        e.play();
        e.seq.poison_clock();
        let (l, r) = render(&mut e, 0.2);
        assert_eq!(l.len(), r.len());
        assert!(all_finite(&l) && all_finite(&r));
    }

    #[test]
    fn out_of_range_indices_are_ignored() {
        let mut e = demo_engine();
        e.play();
        e.select_pattern(0, 200);
        e.select_pattern(9, 0);
        e.set_param(ParamId::Drum(0, 9, DrumParam::Tune), 0.5);
        e.set_param(ParamId::Synth(7, SynthParam::Cutoff), 0.5);
        e.set_param(ParamId::Channel(4, ChannelParam::Level), 0.5);
        e.set_param(ParamId::Kit(2, KitParam::Volume), 0.5);
        let (l, _) = render(&mut e, 0.5);
        assert!(all_finite(&l));
        assert_eq!(e.current_patterns(), [0, 0, 0, 0]);
    }

    #[test]
    fn malformed_or_misplaced_patterns_are_rejected() {
        let mut e = demo_engine();
        e.play();
        let mut short = SynthPattern::default();
        short.steps.truncate(3);
        let back = e.set_synth_pattern(0, 0, short.clone());
        assert_eq!(back, short);
        let mut zero = DrumPattern::default();
        zero.length = 0;
        assert_eq!(e.set_drum_pattern(0, 0, zero.clone()), zero);
        let mut lane = SynthPattern::default();
        lane.param_lane = Some(crate::project::ParamLane {
            param: ParamId::Fx(crate::params::FxParam::DelayMs),
            values: vec![None; 2],
        });
        assert_eq!(e.set_synth_pattern(0, 0, lane.clone()), lane);
        assert_eq!(
            e.set_synth_pattern(2, 0, SynthPattern::default()),
            SynthPattern::default()
        );
        assert_eq!(
            e.set_drum_pattern(0, 200, DrumPattern::default()),
            DrumPattern::default()
        );
        let (l, _) = render(&mut e, 1.0);
        assert!(all_finite(&l) && rms(&l) > 0.02, "still playing the demo");
    }

    #[test]
    fn non_finite_param_is_dropped_and_values_clamped() {
        let mut e = demo_engine();
        e.set_param(ParamId::Synth(0, SynthParam::Cutoff), f32::NAN);
        e.set_param(ParamId::Channel(0, ChannelParam::Level), 50.0);
        e.play();
        let (l, _) = render(&mut e, 0.1);
        assert!(all_finite(&l));
        e.set_param(ParamId::Synth(0, SynthParam::Cutoff), 0.5);
        let (l, _) = render(&mut e, 3.0);
        assert!(all_finite(&l) && peak(&l) > 0.01);
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn playhead_advances() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.3);
        assert!(e.playhead()[2] >= 2);
        assert_eq!(e.current_patterns(), [0, 0, 0, 0]);
    }

    #[test]
    fn lane_value_overrides_only_its_step() {
        use crate::project::ParamLane;
        let mut e = demo_engine();
        let mut p = crate::demo::demo_project().synths[0].patterns[0].clone();
        let mut values = vec![None; 64];
        values[0] = Some(0.0);
        p.param_lane = Some(ParamLane {
            param: ParamId::Master(MasterParam::Level),
            values,
        });
        e.set_synth_pattern(0, 0, p);
        e.play();
        let (l, _) = render(&mut e, 0.5);
        // Step 0 (about 5500 samples at the demo's 128 BPM + swing) is silent...
        assert_eq!(peak(&l[64..5000]), 0.0);
        // ...then the knob value comes back.
        assert!(peak(&l[6000..20_000]) > 0.01);
    }

    #[test]
    fn from_project_matches_load_project() {
        let p = crate::demo::demo_project();
        let mut a = Engine::from_project(SR, &p);
        let mut b = Engine::new(SR);
        b.load_project(&p);
        a.play();
        b.play();
        let (la, _) = render(&mut a, 0.5);
        let (lb, _) = render(&mut b, 0.5);
        assert_eq!(la, lb);
        assert_eq!(a.sample_rate(), SR);
    }

    #[test]
    fn swap_exchanges_patterns_and_rejects_bad_ones() {
        let mut e = demo_engine();
        let mut p = SynthPattern::default();
        assert!(e.swap_synth_pattern(0, 0, &mut p));
        assert!(
            p.steps.iter().any(|s| s.gate != crate::project::Gate::Off),
            "got the old pattern back"
        );
        let mut bad = SynthPattern::default();
        bad.steps.pop();
        assert!(!e.swap_synth_pattern(0, 1, &mut bad));
        assert_eq!(bad.steps.len(), 63, "rejected pattern is untouched");
        let mut d = DrumPattern::default();
        assert!(e.swap_drum_pattern(1, 0, &mut d));
        assert!(!e.swap_drum_pattern(2, 0, &mut d));
    }

    #[test]
    fn meters_report_and_reset() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.5);
        let m = e.take_meters();
        assert!(m[0] > 0.01 && m[2] > 0.01 && m[4] > 0.01, "{m:?}");
        assert!(m[4] <= 1.0 + 1e-6, "master is post-clip");
        e.stop();
        render(&mut e, 5.0);
        e.take_meters();
        render(&mut e, 0.05);
        assert!(e.take_meters().iter().all(|v| *v < 1e-3));
        assert!(!e.take_guard_fired());
    }

    #[test]
    fn queued_patterns_are_reported() {
        let mut e = demo_engine();
        e.play();
        e.select_pattern(3, 5);
        assert_eq!(e.queued_patterns(), [None, None, None, Some(5)]);
    }

    #[test]
    fn stop_restores_knobs_overridden_by_a_lane() {
        use crate::project::ParamLane;
        let mut e = demo_engine();
        let mut p = crate::demo::demo_project().synths[0].patterns[0].clone();
        p.param_lane = Some(ParamLane {
            param: ParamId::Master(MasterParam::Level),
            values: vec![Some(0.0); 64],
        });
        e.set_synth_pattern(0, 0, p);
        e.play();
        render(&mut e, 0.3);
        e.stop();
        e.set_synth_pattern(
            0,
            0,
            crate::demo::demo_project().synths[0].patterns[0].clone(),
        );
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01, "master level was restored by stop()");
    }

    fn two_bar_demo() -> Project {
        let mut p = crate::demo::demo_project();
        let bar = crate::project::Bar {
            patterns: [Some(crate::project::PatternRef(0)); 4],
            ..Default::default()
        };
        p.song.bars = vec![bar, bar];
        p
    }

    #[test]
    fn a_song_plays_and_stops_by_itself() {
        let mut e = Engine::from_project(SR, &two_bar_demo());
        e.set_mode(crate::sequencer::Mode::Song);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01);
        render(&mut e, 4.0); // two bars at 128 BPM ≈ 3.75 s
        assert!(!e.is_playing(), "song ended");
    }

    #[test]
    fn automation_plays_in_song_mode_and_stop_restores_the_knob() {
        let mut p = two_bar_demo();
        crate::song::write_automation(&mut p.song, ParamId::Master(MasterParam::Level), 0, 0, 0.0);
        let mut e = Engine::from_project(SR, &p);
        e.set_mode(crate::sequencer::Mode::Song);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert_eq!(peak(&l[64..]), 0.0, "automation holds master at 0");
        e.stop();
        e.set_mode(crate::sequencer::Mode::Pattern);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01, "knob value restored by stop");
    }

    #[test]
    fn a_touched_param_ignores_automation() {
        let mut p = two_bar_demo();
        crate::song::write_automation(
            &mut p.song,
            ParamId::Master(MasterParam::Level),
            0,
            200,
            0.0,
        );
        let mut e = Engine::from_project(SR, &p);
        e.set_mode(crate::sequencer::Mode::Song);
        e.touch(ParamId::Master(MasterParam::Level), true);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01);
    }

    #[test]
    fn live_values_do_not_move_the_knob() {
        let mut e = demo_engine();
        e.play();
        e.set_param_live(ParamId::Master(MasterParam::Level), 0.0);
        let (l, _) = render(&mut e, 0.5);
        assert_eq!(peak(&l[64..]), 0.0);
        e.stop();
        e.play();
        let (l, _) = render(&mut e, 0.5);
        assert!(peak(&l) > 0.01, "stop restored the saved knob");
    }

    #[test]
    fn swap_song_validates() {
        let mut e = demo_engine();
        let mut good = two_bar_demo().song;
        assert!(e.swap_song(&mut good));
        let mut bad = crate::project::Song {
            bars: vec![Default::default(); crate::song::MAX_BARS + 1],
            ..Default::default()
        };
        assert!(!e.swap_song(&mut bad));
        assert_eq!(
            bad.bars.len(),
            crate::song::MAX_BARS + 1,
            "rejected song is untouched"
        );
    }

    #[test]
    fn song_start_and_position() {
        let mut e = Engine::from_project(SR, &two_bar_demo());
        e.set_mode(crate::sequencer::Mode::Song);
        e.set_song_start(1);
        e.play();
        render(&mut e, 0.01);
        assert_eq!(e.song_bar(), 1);
        assert_eq!(e.song_step(), 16);
        assert_eq!(e.mode(), crate::sequencer::Mode::Song);
    }
}
