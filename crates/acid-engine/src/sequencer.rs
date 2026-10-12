//! Pattern-mode sequencer. Sample-accurate: the engine asks how many
//! samples until the next tick, renders that many, then calls tick().
//! Each 16th step has a start tick (notes, drums, param lane) and a mid
//! tick (gate off unless sliding/tied into the next step).

use crate::machine::{MACHINES, MachineId};
use crate::params::ParamId;
use crate::project::{DrumHit, Gate, PatternRef, Song};
use crate::song::{Banks, DEFAULT_BAR_STEPS, bar_start_step, bar_steps, value_at};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Pattern,
    Song,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SeqEvent {
    Note {
        synth: usize,
        midi: u8,
        accent: bool,
        legato: bool,
    },
    NoteOff {
        synth: usize,
    },
    Drum {
        kit: usize,
        inst: usize,
        accent: bool,
    },
    Param {
        id: ParamId,
        value: f32,
    },
    ParamRestore {
        id: ParamId,
    },
    /// A recorded song-automation value for this step.
    Automation {
        id: ParamId,
        value: f32,
    },
    /// The song position jumped to before this lane's first point: the
    /// param goes back to its knob value.
    AutomationReset {
        id: ParamId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    StepStart,
    Mid,
}

#[derive(Clone, Copy, Debug, Default)]
struct Track {
    step: usize,
    pattern: u8,
    queued: Option<u8>,
    gate_open: bool,
    prev_slide: bool,
    /// Param whose lane value is overriding the knob on the current step.
    lane_override: Option<ParamId>,
    /// The machine plays nothing this bar.
    silent: bool,
    /// The bar's synth transpose.
    transpose_add: i8,
}

pub struct Sequencer {
    sample_rate: f32,
    bpm: f32,
    swing: f32,
    playing: bool,
    global_step: u64,
    phase: Phase,
    samples_to_next: f64,
    /// Indexed by `MachineId::index`.
    tracks: [Track; MACHINES],
    mode: Mode,
    song_start: usize,
    song_bar: usize,
    bar_step: usize,
    bar_len: usize,
    song_step: u32,
    /// The song position must be recomputed at the next bar start.
    seek: bool,
    /// The next step emits every lane's held value, not just its points.
    auto_sync: bool,
    /// The song changed while playing: the next step start recomputes
    /// the song position and the current bar's length.
    resync_pending: bool,
    /// The pattern last selected per machine; restored when song
    /// playback (which overrides the tracks' patterns) is left.
    chosen: [u8; MACHINES],
    ended: bool,
}

fn len_of(length: u8) -> usize {
    (length as usize).clamp(1, crate::project::STEPS)
}

impl Sequencer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            bpm: 125.0,
            swing: 0.0,
            playing: false,
            global_step: 0,
            phase: Phase::StepStart,
            samples_to_next: 0.0,
            tracks: [Track::default(); MACHINES],
            mode: Mode::Pattern,
            song_start: 0,
            song_bar: 0,
            bar_step: 0,
            bar_len: DEFAULT_BAR_STEPS,
            song_step: 0,
            seek: false,
            auto_sync: false,
            resync_pending: false,
            chosen: [0; MACHINES],
            ended: false,
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        if bpm.is_finite() {
            self.bpm = bpm.clamp(40.0, 300.0);
        }
    }

    /// Test hook: makes the tick clock NaN, as a broken tempo would.
    #[cfg(test)]
    pub(crate) fn poison_clock(&mut self) {
        self.samples_to_next = f64::NAN;
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    pub fn set_swing(&mut self, swing: f32) {
        if swing.is_finite() {
            self.swing = swing.clamp(0.0, 0.75);
        }
    }

    fn step_len(&self, step: u64) -> f64 {
        let base = 60.0 / self.bpm as f64 / 4.0 * self.sample_rate as f64;
        let s = self.swing as f64 * 0.5;
        if step.is_multiple_of(2) {
            base * (1.0 + s)
        } else {
            base * (1.0 - s)
        }
    }

    pub fn play(&mut self) {
        self.playing = true;
        self.global_step = 0;
        self.phase = Phase::StepStart;
        self.samples_to_next = 0.0;
        self.ended = false;
        if self.mode == Mode::Song {
            self.song_bar = self.song_start;
            self.bar_step = 0;
            self.bar_len = DEFAULT_BAR_STEPS;
            self.seek = true;
        }
        for t in &mut self.tracks {
            t.step = 0;
            t.gate_open = false;
            t.prev_slide = false;
            t.lane_override = None;
            t.silent = false;
            t.transpose_add = 0;
            if let Some(q) = t.queued.take() {
                t.pattern = q;
            }
        }
        self.resync_pending = false;
    }

    pub fn stop(&mut self) {
        self.playing = false;
        self.resync_pending = false;
        for t in &mut self.tracks {
            t.step = 0;
            t.gate_open = false;
            t.prev_slide = false;
            t.lane_override = None;
            t.silent = false;
            t.transpose_add = 0;
        }
        if self.mode == Mode::Song {
            self.restore_chosen();
        }
    }

    /// Puts each machine back on its last selected pattern.
    fn restore_chosen(&mut self) {
        for (t, &slot) in self.tracks.iter_mut().zip(&self.chosen) {
            t.pattern = slot;
            t.queued = None;
        }
    }

    /// The song was edited while playing: re-derive the song position
    /// (and the current bar's length) at the next step. Real-time safe.
    pub fn resync(&mut self) {
        if self.playing && self.mode == Mode::Song {
            self.resync_pending = true;
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Ignored while playing (the UI stops before switching).
    pub fn set_mode(&mut self, mode: Mode) {
        if !self.playing && self.mode != mode {
            self.mode = mode;
            self.restore_chosen();
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_song_start(&mut self, bar: usize) {
        self.song_start = bar;
    }

    pub fn song_bar(&self) -> usize {
        self.song_bar
    }

    pub fn song_step(&self) -> u32 {
        self.song_step
    }

    /// True once after a song played past its last bar.
    pub fn take_ended(&mut self) -> bool {
        std::mem::take(&mut self.ended)
    }

    /// Loads the current bar's patterns, mutes and transposes. Returns
    /// false (and stops) when the song has no bar here.
    fn load_bar(&mut self, song: &Song, banks: Banks<'_>, out: &mut Vec<SeqEvent>) -> bool {
        let Some(bar) = song.bars.get(self.song_bar) else {
            for s in 0..2 {
                if self.tracks[s].gate_open {
                    out.push(SeqEvent::NoteOff { synth: s });
                }
            }
            self.stop();
            self.ended = true;
            return false;
        };
        if self.seek {
            self.song_step = bar_start_step(song, self.song_bar, banks);
            self.seek = false;
            self.auto_sync = true;
        }
        self.bar_len = bar_steps(bar, banks);
        for m in 0..MACHINES {
            let bank_len = banks.bank_len(m);
            let t = &mut self.tracks[m];
            t.step = 0;
            match bar.patterns[m] {
                Some(PatternRef(slot)) if (slot as usize) < bank_len => {
                    t.pattern = slot;
                    t.silent = bar.mutes[m];
                }
                _ => t.silent = true,
            }
            t.transpose_add = MachineId::ALL[m]
                .transpose_index()
                .map_or(0, |i| bar.transpose[i]);
        }
        true
    }

    /// Song playback ignores the choice (the bars pick the patterns) but
    /// remembers it for when playback stops or the mode changes.
    /// Song automation for the current step. On a sync step every lane
    /// reports its held value, and lanes with no point yet reset.
    fn emit_automation(&mut self, song: &Song, out: &mut Vec<SeqEvent>) {
        for lane in &song.automation {
            if self.auto_sync {
                out.push(match value_at(lane, self.song_step) {
                    Some(value) => SeqEvent::Automation {
                        id: lane.param,
                        value,
                    },
                    None => SeqEvent::AutomationReset { id: lane.param },
                });
            } else if let Some(&value) = lane.points.get(&self.song_step) {
                out.push(SeqEvent::Automation {
                    id: lane.param,
                    value,
                });
            }
        }
        self.auto_sync = false;
    }

    /// Re-derives the song position after a song edit. At a bar start the
    /// bar load does it; mid-bar the current bar's length and position are
    /// recomputed here. A bar that no longer exists ends at its old length.
    fn apply_resync(&mut self, song: &Song, banks: Banks<'_>) {
        self.resync_pending = false;
        if self.bar_step == 0 {
            self.seek = true;
            return;
        }
        let Some(bar) = song.bars.get(self.song_bar) else {
            return;
        };
        self.song_step = bar_start_step(song, self.song_bar, banks) + self.bar_step as u32;
        self.bar_len = bar_steps(bar, banks);
        self.auto_sync = true;
        if self.bar_step >= self.bar_len {
            // The bar got shorter than where we are: the next bar start
            // must find its position afresh.
            self.seek = true;
        }
    }

    pub fn select(&mut self, machine: usize, slot: u8) {
        self.chosen[machine] = slot;
        if self.mode == Mode::Song && self.playing {
            return;
        }
        let t = &mut self.tracks[machine];
        if self.playing {
            t.queued = Some(slot);
        } else {
            t.pattern = slot;
            t.queued = None;
        }
    }

    pub fn current_pattern(&self, machine: usize) -> u8 {
        self.tracks[machine].pattern
    }

    pub fn queued_pattern(&self, machine: usize) -> Option<u8> {
        self.tracks[machine].queued
    }

    pub fn playhead(&self, machine: usize) -> usize {
        self.tracks[machine].step
    }

    pub fn samples_to_next_tick(&self) -> usize {
        if !self.playing {
            return usize::MAX;
        }
        self.samples_to_next.max(0.0).ceil() as usize
    }

    pub fn advance(&mut self, n: usize) {
        if self.playing {
            self.samples_to_next -= n as f64;
        }
    }

    pub fn tick(&mut self, banks: Banks<'_>, song: &Song, out: &mut Vec<SeqEvent>) {
        if !self.playing {
            return;
        }
        let half = self.step_len(self.global_step) * 0.5;
        match self.phase {
            Phase::StepStart => {
                if self.mode == Mode::Song {
                    if self.resync_pending {
                        self.apply_resync(song, banks);
                    }
                    if self.bar_step == 0 && !self.load_bar(song, banks, out) {
                        return;
                    }
                    // Before the pattern lanes, so a lane value wins on its step.
                    self.emit_automation(song, out);
                }
                for s in 0..2 {
                    let t = &mut self.tracks[s];
                    if t.silent {
                        if t.gate_open {
                            out.push(SeqEvent::NoteOff { synth: s });
                            t.gate_open = false;
                        }
                        t.prev_slide = false;
                        if let Some(id) = t.lane_override.take() {
                            out.push(SeqEvent::ParamRestore { id });
                        }
                        continue;
                    }
                    let pat = &banks.synths[s][t.pattern as usize];
                    if t.step >= len_of(pat.length) {
                        t.step = 0;
                    }
                    let step = pat.steps[t.step];
                    match step.gate {
                        Gate::On => {
                            let legato = t.prev_slide && t.gate_open;
                            out.push(SeqEvent::Note {
                                synth: s,
                                midi: step.midi(pat.transpose.saturating_add(t.transpose_add)),
                                accent: step.accent,
                                legato,
                            });
                            t.gate_open = true;
                        }
                        Gate::Tie => {
                            if !t.gate_open {
                                out.push(SeqEvent::Note {
                                    synth: s,
                                    midi: step.midi(pat.transpose.saturating_add(t.transpose_add)),
                                    accent: step.accent,
                                    legato: false,
                                });
                                t.gate_open = true;
                            }
                        }
                        Gate::Off => {
                            if t.gate_open {
                                out.push(SeqEvent::NoteOff { synth: s });
                                t.gate_open = false;
                            }
                        }
                    }
                    t.prev_slide = step.slide && step.gate != Gate::Off;
                    let lane_value = pat.param_lane.as_ref().and_then(|lane| {
                        lane.values
                            .get(t.step)
                            .copied()
                            .flatten()
                            .map(|v| (lane.param, v))
                    });
                    match lane_value {
                        Some((id, value)) => {
                            if let Some(old) = t.lane_override
                                && old != id
                            {
                                out.push(SeqEvent::ParamRestore { id: old });
                            }
                            out.push(SeqEvent::Param { id, value });
                            t.lane_override = Some(id);
                        }
                        None => {
                            if let Some(id) = t.lane_override.take() {
                                out.push(SeqEvent::ParamRestore { id });
                            }
                        }
                    }
                }
                for k in 0..2 {
                    let t = &mut self.tracks[2 + k];
                    if t.silent {
                        continue;
                    }
                    let pat = &banks.drums[k][t.pattern as usize];
                    if t.step >= len_of(pat.length) {
                        t.step = 0;
                    }
                    for (inst, hit) in pat.steps[t.step].iter().enumerate() {
                        match hit {
                            DrumHit::Off => {}
                            DrumHit::On => out.push(SeqEvent::Drum {
                                kit: k,
                                inst,
                                accent: false,
                            }),
                            DrumHit::Accent => out.push(SeqEvent::Drum {
                                kit: k,
                                inst,
                                accent: true,
                            }),
                        }
                    }
                }
                self.phase = Phase::Mid;
            }
            Phase::Mid => {
                for s in 0..2 {
                    let t = &mut self.tracks[s];
                    if t.silent {
                        continue;
                    }
                    let pat = &banks.synths[s][t.pattern as usize];
                    let len = len_of(pat.length);
                    if t.gate_open {
                        let step = pat.steps[t.step];
                        let next = pat.steps[(t.step + 1) % len];
                        let hold = (step.slide && next.gate != Gate::Off) || next.gate == Gate::Tie;
                        if !hold {
                            out.push(SeqEvent::NoteOff { synth: s });
                            t.gate_open = false;
                        }
                    }
                }
                for m in 0..MACHINES {
                    let len = banks.length(m, self.tracks[m].pattern).unwrap_or(1);
                    let t = &mut self.tracks[m];
                    t.step += 1;
                    if t.step >= len {
                        t.step = 0;
                        if let Some(q) = t.queued.take() {
                            t.pattern = q;
                        }
                    }
                }
                self.global_step += 1;
                if self.mode == Mode::Song {
                    self.song_step = self.song_step.saturating_add(1);
                    self.bar_step += 1;
                    if self.bar_step >= self.bar_len {
                        self.bar_step = 0;
                        self.song_bar += 1;
                        if let Some((a, b)) = song.loop_range
                            && self.song_bar > b as usize
                        {
                            self.song_bar = a as usize;
                            self.seek = true;
                        }
                    }
                }
                self.phase = Phase::StepStart;
            }
        }
        self.samples_to_next += half;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{DrumHit, DrumPattern, Gate, NotePattern, SynthPattern, SynthStep};

    const SR: f32 = 44_100.0;

    #[test]
    fn non_finite_tempo_is_ignored() {
        let mut seq = Sequencer::new(SR);
        seq.set_bpm(140.0);
        seq.set_swing(0.3);
        seq.set_bpm(f32::NAN);
        seq.set_bpm(f32::INFINITY);
        seq.set_swing(f32::NAN);
        assert_eq!(seq.bpm(), 140.0);
        assert_eq!(seq.swing, 0.3);
    }

    struct Fixture {
        synths: [Vec<SynthPattern>; 2],
        drums: [Vec<DrumPattern>; 2],
        notes: [Vec<NotePattern>; 2],
        song: crate::project::Song,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                synths: [
                    vec![SynthPattern::default(); 4],
                    vec![SynthPattern::default(); 4],
                ],
                drums: [
                    vec![DrumPattern::default(); 4],
                    vec![DrumPattern::default(); 4],
                ],
                notes: [
                    vec![NotePattern::default(); 4],
                    vec![NotePattern::default(); 4],
                ],
                song: crate::project::Song::default(),
            }
        }

        fn banks(&self) -> Banks<'_> {
            Banks::new(&self.synths, &self.drums, &self.notes)
        }

        fn run(&self, seq: &mut Sequencer, total: usize) -> Vec<(usize, SeqEvent)> {
            let mut pos = 0;
            let mut buf = Vec::with_capacity(64);
            let mut out = Vec::new();
            while pos < total {
                let n = seq.samples_to_next_tick().min(total - pos);
                if n == 0 {
                    buf.clear();
                    seq.tick(self.banks(), &self.song, &mut buf);
                    out.extend(buf.iter().map(|e| (pos, *e)));
                    continue;
                }
                seq.advance(n);
                pos += n;
            }
            out
        }
    }

    fn on(note: u8) -> SynthStep {
        SynthStep {
            note,
            octave: 0,
            gate: Gate::On,
            accent: false,
            slide: false,
        }
    }

    fn new_seq() -> Sequencer {
        let mut s = Sequencer::new(SR);
        s.set_bpm(120.0); // 16th = 5512.5 samples
        s
    }

    #[test]
    fn stopped_sequencer_never_ticks() {
        let s = new_seq();
        assert_eq!(s.samples_to_next_tick(), usize::MAX);
    }

    #[test]
    fn four_on_the_floor_is_sample_accurate() {
        let mut f = Fixture::new();
        for st in [0, 4, 8, 12] {
            f.drums[0][0].steps[st][0] = DrumHit::On;
        }
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 88_200);
        let kicks: Vec<usize> = ev
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e,
                    SeqEvent::Drum {
                        kit: 0,
                        inst: 0,
                        ..
                    }
                )
            })
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(kicks, vec![0, 22_050, 44_100, 66_150]);
    }

    #[test]
    fn odd_steps_land_on_the_next_whole_sample() {
        let mut f = Fixture::new();
        f.drums[1][0].steps[1][6] = DrumHit::Accent;
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 11_000);
        assert_eq!(
            ev,
            vec![(
                5513,
                SeqEvent::Drum {
                    kit: 1,
                    inst: 6,
                    accent: true
                }
            )]
        );
    }

    #[test]
    fn swing_delays_odd_steps() {
        let mut f = Fixture::new();
        f.drums[0][0].steps[1][0] = DrumHit::On;
        f.drums[0][0].steps[2][0] = DrumHit::On;
        let mut s = new_seq();
        s.set_swing(0.5);
        s.play();
        let ev = f.run(&mut s, 12_000);
        let pos: Vec<usize> = ev.iter().map(|(p, _)| *p).collect();
        assert_eq!(pos, vec![6891, 11_025]); // 5512.5 * 1.25 = 6890.6
    }

    #[test]
    fn plain_note_releases_at_half_step() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(0);
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 5000);
        assert_eq!(
            ev,
            vec![
                (
                    0,
                    SeqEvent::Note {
                        synth: 0,
                        midi: 36,
                        accent: false,
                        legato: false
                    }
                ),
                (2757, SeqEvent::NoteOff { synth: 0 }),
            ]
        );
    }

    #[test]
    fn slide_holds_gate_and_marks_next_note_legato() {
        let mut f = Fixture::new();
        f.synths[1][0].steps[0] = SynthStep {
            slide: true,
            ..on(0)
        };
        f.synths[1][0].steps[1] = on(7);
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 11_000);
        assert_eq!(
            ev,
            vec![
                (
                    0,
                    SeqEvent::Note {
                        synth: 1,
                        midi: 36,
                        accent: false,
                        legato: false
                    }
                ),
                (
                    5513,
                    SeqEvent::Note {
                        synth: 1,
                        midi: 43,
                        accent: false,
                        legato: true
                    }
                ),
                (8269, SeqEvent::NoteOff { synth: 1 }),
            ]
        );
    }

    #[test]
    fn tie_extends_without_retrigger() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(5);
        f.synths[0][0].steps[1] = SynthStep {
            gate: Gate::Tie,
            ..on(9)
        };
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 11_000);
        assert_eq!(
            ev,
            vec![
                (
                    0,
                    SeqEvent::Note {
                        synth: 0,
                        midi: 41,
                        accent: false,
                        legato: false
                    }
                ),
                (8269, SeqEvent::NoteOff { synth: 0 }),
            ]
        );
    }

    #[test]
    fn slide_into_rest_releases_at_half_step() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = SynthStep {
            slide: true,
            ..on(0)
        };
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 5000);
        assert_eq!(ev[1], (2757, SeqEvent::NoteOff { synth: 0 }));
    }

    #[test]
    fn accent_and_transpose_pass_through() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = SynthStep {
            accent: true,
            octave: 1,
            ..on(2)
        };
        f.synths[0][0].transpose = -3;
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 10);
        assert_eq!(
            ev[0].1,
            SeqEvent::Note {
                synth: 0,
                midi: 36 + 12 + 2 - 3,
                accent: true,
                legato: false
            }
        );
    }

    #[test]
    fn pattern_length_wraps_and_polymeter() {
        let mut f = Fixture::new();
        f.synths[0][0].length = 3;
        f.synths[0][0].steps[0] = on(0);
        f.drums[0][0].length = 4;
        f.drums[0][0].steps[0][0] = DrumHit::On;
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 12 * 5512); // steps 0..=11 (step 12 starts at 66 150)
        let notes = ev
            .iter()
            .filter(|(_, e)| matches!(e, SeqEvent::Note { .. }))
            .count();
        let kicks = ev
            .iter()
            .filter(|(_, e)| matches!(e, SeqEvent::Drum { .. }))
            .count();
        assert_eq!(notes, 4); // steps 0, 3, 6, 9
        assert_eq!(kicks, 3); // steps 0, 4, 8
    }

    #[test]
    fn selecting_while_playing_queues_until_wrap() {
        let mut f = Fixture::new();
        f.drums[0][1].steps[0][1] = DrumHit::On; // pattern 1 has a snare on step 0
        let mut s = new_seq();
        s.play();
        f.run(&mut s, 3 * 5513);
        s.select(2, 1);
        assert_eq!(s.current_pattern(2), 0);
        assert_eq!(s.queued_pattern(2), Some(1));
        let ev = f.run(&mut s, 14 * 5513);
        let snare = ev
            .iter()
            .find(|(_, e)| matches!(e, SeqEvent::Drum { inst: 1, .. }));
        assert!(snare.is_some());
        assert_eq!(s.current_pattern(2), 1);
    }

    #[test]
    fn selecting_while_stopped_is_immediate() {
        let mut s = new_seq();
        s.select(0, 9);
        assert_eq!(s.current_pattern(0), 9);
        assert_eq!(s.queued_pattern(0), None);
    }

    #[test]
    fn param_lane_emits_param_events() {
        use crate::params::{ParamId, SynthParam};
        use crate::project::ParamLane;
        let mut f = Fixture::new();
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        let mut values = vec![None; 64];
        values[0] = Some(0.8);
        f.synths[0][0].param_lane = Some(ParamLane { param: id, values });
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 10);
        assert_eq!(ev, vec![(0, SeqEvent::Param { id, value: 0.8 })]);
    }

    #[test]
    fn stop_resets_playheads() {
        let f = Fixture::new();
        let mut s = new_seq();
        s.play();
        f.run(&mut s, 5 * 5513);
        assert_eq!(s.playhead(0), 5);
        s.stop();
        assert!(!s.is_playing());
        assert_eq!(s.playhead(0), 0);
    }

    #[test]
    fn lane_override_is_restored_on_the_next_step_without_a_value() {
        use crate::params::{ParamId, SynthParam};
        use crate::project::ParamLane;
        let mut f = Fixture::new();
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        let mut values = vec![None; 64];
        values[0] = Some(0.8);
        f.synths[0][0].param_lane = Some(ParamLane { param: id, values });
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 6000);
        assert_eq!(
            ev,
            vec![
                (0, SeqEvent::Param { id, value: 0.8 }),
                (5513, SeqEvent::ParamRestore { id })
            ]
        );
    }

    fn song_bar(refs: [Option<u8>; 4]) -> crate::project::Bar {
        let mut patterns = [None; crate::machine::MACHINES];
        for (m, r) in refs.into_iter().enumerate() {
            patterns[m] = r.map(crate::project::PatternRef);
        }
        crate::project::Bar {
            patterns,
            ..Default::default()
        }
    }

    fn notes(ev: &[(usize, SeqEvent)]) -> Vec<(usize, u8)> {
        ev.iter()
            .filter_map(|(p, e)| match e {
                SeqEvent::Note { midi, .. } => Some((*p, *midi)),
                _ => None,
            })
            .collect()
    }

    fn song_seq() -> Sequencer {
        let mut s = new_seq();
        s.set_mode(Mode::Song);
        s
    }

    #[test]
    fn song_plays_bars_in_order() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(0);
        f.synths[0][1].steps[0] = on(7);
        f.song.bars = vec![
            song_bar([Some(0), None, None, None]),
            song_bar([Some(1), None, None, None]),
        ];
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 33 * 5513);
        assert_eq!(notes(&ev), vec![(0, 36), (88_200, 43)]);
        assert!(!s.is_playing(), "stops after the last bar");
        assert!(s.take_ended());
        assert!(!s.take_ended(), "reported once");
    }

    #[test]
    fn bar_length_follows_the_longest_pattern_and_shorter_ones_loop() {
        let mut f = Fixture::new();
        f.synths[0][0].length = 8;
        f.synths[0][0].steps[0] = on(0);
        f.drums[0][0].length = 16;
        f.song.bars = vec![song_bar([Some(0), None, Some(0), None])];
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 17 * 5513);
        assert_eq!(
            notes(&ev).len(),
            2,
            "8-step synth pattern plays twice in the 16-step bar"
        );
    }

    #[test]
    fn none_and_mute_silence_a_machine_and_transpose_adds() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(0);
        f.drums[0][0].steps[0][0] = DrumHit::On;
        let mut muted = song_bar([None, None, Some(0), None]);
        muted.mutes[2] = true;
        let mut lifted = song_bar([Some(0), None, Some(0), None]);
        lifted.transpose[0] = 5;
        f.song.bars = vec![muted, lifted];
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 32 * 5513);
        let drums = ev
            .iter()
            .filter(|(_, e)| matches!(e, SeqEvent::Drum { .. }))
            .count();
        assert_eq!(drums, 1, "muted in bar 1, playing in bar 2");
        assert_eq!(
            notes(&ev),
            vec![(88_200, 41)],
            "no synth in bar 1; bar 2 transposed +5"
        );
    }

    #[test]
    fn loop_range_repeats_and_resets_the_song_step() {
        let mut f = Fixture::new();
        f.song.bars = vec![
            song_bar([None; 4]),
            song_bar([None; 4]),
            song_bar([None; 4]),
        ];
        f.song.loop_range = Some((1, 2));
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 48 * 5513 + 10);
        assert!(s.is_playing());
        assert_eq!(s.song_bar(), 1, "jumped back to the loop start");
        assert_eq!(s.song_step(), 16);
    }

    #[test]
    fn automation_emits_points_and_syncs_on_start() {
        use crate::params::{MasterParam, ParamId};
        let id = ParamId::Master(MasterParam::Level);
        let mut f = Fixture::new();
        f.song.bars = vec![song_bar([None; 4]), song_bar([None; 4])];
        crate::song::write_automation(&mut f.song, id, 3, 3, 0.25);
        crate::song::write_automation(&mut f.song, id, 16, 16, 0.75);
        let autos = |ev: &[(usize, SeqEvent)]| -> Vec<(usize, f32)> {
            ev.iter()
                .filter_map(|(p, e)| match e {
                    SeqEvent::Automation { value, .. } => Some((*p, *value)),
                    _ => None,
                })
                .collect()
        };
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 17 * 5513);
        assert_eq!(autos(&ev), vec![(16_538, 0.25), (88_200, 0.75)]);

        let mut s = song_seq();
        s.set_song_start(1);
        s.play();
        assert_eq!(s.song_bar(), 1);
        let ev = f.run(&mut s, 10);
        assert_eq!(
            autos(&ev),
            vec![(0, 0.75)],
            "starting mid-song applies the held value"
        );
        assert_eq!(s.song_step(), 16);
    }

    #[test]
    fn pattern_mode_ignores_the_song_and_mode_is_fixed_while_playing() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(0);
        f.song.bars = vec![song_bar([None; 4])];
        let mut s = new_seq();
        s.play();
        s.set_mode(Mode::Song);
        assert_eq!(s.mode(), Mode::Pattern);
        let ev = f.run(&mut s, 100);
        assert_eq!(notes(&ev), vec![(0, 36)]);
    }

    #[test]
    fn selection_during_song_playback_is_kept_for_pattern_mode() {
        let mut f = Fixture::new();
        for b in f.synths.iter_mut() {
            b.resize(8, SynthPattern::default());
        }
        for b in f.drums.iter_mut() {
            b.resize(8, DrumPattern::default());
        }
        f.synths[0][2].steps[0] = on(4);
        f.synths[0][5].steps[0] = on(9);
        f.song.bars = vec![song_bar([Some(5); 4]), song_bar([Some(5); 4])];
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 3 * 5513);
        for m in 0..4 {
            s.select(m, 2);
            assert_eq!(s.current_pattern(m), 5, "ignored while the song plays");
            assert_eq!(s.queued_pattern(m), None);
        }
        s.stop();
        s.set_mode(Mode::Pattern);
        s.play();
        for m in 0..4 {
            assert_eq!(s.current_pattern(m), 2, "machine {m}");
        }
        let ev = f.run(&mut s, 100);
        assert_eq!(notes(&ev), vec![(0, 40)], "pattern 2 plays, not 5");
    }

    fn resets(ev: &[(usize, SeqEvent)]) -> Vec<usize> {
        ev.iter()
            .filter_map(|(p, e)| matches!(e, SeqEvent::AutomationReset { .. }).then_some(*p))
            .collect()
    }

    #[test]
    fn loop_jump_resets_lanes_without_a_point_yet() {
        use crate::params::{MasterParam, ParamId};
        let id = ParamId::Master(MasterParam::Level);
        let mut f = Fixture::new();
        f.song.bars = vec![song_bar([None; 4]), song_bar([None; 4])];
        f.song.loop_range = Some((0, 1));
        crate::song::write_automation(&mut f.song, id, 20, 20, 0.0);
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 33 * 5513);
        assert!(s.is_playing(), "looping");
        assert_eq!((s.song_bar(), s.song_step()), (0, 1), "back in bar 0");
        // Step 32 (the loop jump) starts at 32 × 5512.5 = 176 400.
        assert_eq!(
            resets(&ev),
            vec![0, 176_400],
            "at the start and the loop jump"
        );
        let reset_then_point: Vec<SeqEvent> = ev
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e,
                    SeqEvent::Automation { .. } | SeqEvent::AutomationReset { .. }
                )
            })
            .map(|(_, e)| *e)
            .collect();
        assert_eq!(
            reset_then_point,
            vec![
                SeqEvent::AutomationReset { id },
                SeqEvent::Automation { id, value: 0.0 },
                SeqEvent::AutomationReset { id },
            ]
        );
    }

    #[test]
    fn song_automation_comes_before_pattern_lanes_in_a_step() {
        use crate::params::{MasterParam, ParamId};
        use crate::project::ParamLane;
        let id = ParamId::Master(MasterParam::Level);
        let mut f = Fixture::new();
        let mut values = vec![None; 64];
        values[2] = Some(0.9);
        f.synths[0][0].param_lane = Some(ParamLane { param: id, values });
        f.song.bars = vec![song_bar([Some(0), None, None, None])];
        crate::song::write_automation(&mut f.song, id, 2, 2, 0.1);
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 3 * 5513);
        let at_step_2: Vec<SeqEvent> = ev
            .iter()
            .filter(|(p, _)| *p == 11_025)
            .map(|(_, e)| *e)
            .collect();
        assert_eq!(
            at_step_2,
            vec![
                SeqEvent::Automation { id, value: 0.1 },
                SeqEvent::Param { id, value: 0.9 },
            ],
            "the pattern lane wins on its step"
        );
    }

    #[test]
    fn resync_mid_bar_recomputes_the_song_position() {
        let mut f = Fixture::new();
        f.synths[0][1].length = 32;
        f.song.bars = vec![song_bar([None; 4]), song_bar([None; 4])];
        let mut s = song_seq();
        s.play();
        // Past the mid tick of step 18: bar 1, bar step 3, song step 19.
        f.run(&mut s, 103_000);
        assert_eq!((s.song_bar(), s.song_step()), (1, 19));
        f.song.bars[0] = song_bar([Some(1), None, None, None]);
        s.resync();
        f.run(&mut s, 3000); // the step-19 start tick
        assert_eq!(s.song_step(), 32 + 3);
        f.run(&mut s, 12 * 5513); // through the step-31 start tick
        assert_eq!((s.song_bar(), s.song_step()), (1, 32 + 15));
    }

    #[test]
    fn resync_is_ignored_unless_a_song_plays() {
        let mut s = new_seq();
        s.play();
        s.resync();
        assert!(!s.resync_pending);
        let mut s = song_seq();
        s.resync();
        assert!(!s.resync_pending);
    }

    #[test]
    fn an_empty_song_ends_immediately() {
        let f = Fixture::new();
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 10);
        assert!(!s.is_playing());
        assert!(s.take_ended());
    }

    #[test]
    fn note_tracks_select_queue_and_report() {
        let mut f = Fixture::new();
        f.notes[0][1].length = 4;
        let mut s = new_seq();
        s.select(4, 1);
        s.select(5, 2);
        assert_eq!((s.current_pattern(4), s.current_pattern(5)), (1, 2));
        s.play();
        s.select(4, 3);
        assert_eq!(s.queued_pattern(4), Some(3));
        f.run(&mut s, 4 * 5513);
        assert_eq!(s.current_pattern(4), 3, "switched after the 4-step pattern");
        assert_eq!(s.queued_pattern(4), None);
    }
}
