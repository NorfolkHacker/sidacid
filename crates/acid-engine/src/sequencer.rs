//! Pattern-mode sequencer. Sample-accurate: the engine asks how many
//! samples until the next tick, renders that many, then calls tick().
//! Each 16th step has a start tick (notes, drums, param lane) and a mid
//! tick (gate off unless sliding/tied into the next step).

use crate::params::ParamId;
use crate::project::{DrumHit, DrumPattern, Gate, SynthPattern};

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
}

pub struct Sequencer {
    sample_rate: f32,
    bpm: f32,
    swing: f32,
    playing: bool,
    global_step: u64,
    phase: Phase,
    samples_to_next: f64,
    /// 0, 1 = synths; 2, 3 = drum kits.
    tracks: [Track; 4],
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
            tracks: [Track::default(); 4],
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
        for t in &mut self.tracks {
            t.step = 0;
            t.gate_open = false;
            t.prev_slide = false;
            t.lane_override = None;
            if let Some(q) = t.queued.take() {
                t.pattern = q;
            }
        }
    }

    pub fn stop(&mut self) {
        self.playing = false;
        for t in &mut self.tracks {
            t.step = 0;
            t.gate_open = false;
            t.prev_slide = false;
            t.lane_override = None;
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn select(&mut self, machine: usize, slot: u8) {
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

    pub fn tick(
        &mut self,
        synths: [&[SynthPattern]; 2],
        drums: [&[DrumPattern]; 2],
        out: &mut Vec<SeqEvent>,
    ) {
        if !self.playing {
            return;
        }
        let half = self.step_len(self.global_step) * 0.5;
        match self.phase {
            Phase::StepStart => {
                for s in 0..2 {
                    let t = &mut self.tracks[s];
                    let pat = &synths[s][t.pattern as usize];
                    if t.step >= len_of(pat.length) {
                        t.step = 0;
                    }
                    let step = pat.steps[t.step];
                    match step.gate {
                        Gate::On => {
                            let legato = t.prev_slide && t.gate_open;
                            out.push(SeqEvent::Note {
                                synth: s,
                                midi: step.midi(pat.transpose),
                                accent: step.accent,
                                legato,
                            });
                            t.gate_open = true;
                        }
                        Gate::Tie => {
                            if !t.gate_open {
                                out.push(SeqEvent::Note {
                                    synth: s,
                                    midi: step.midi(pat.transpose),
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
                    let pat = &drums[k][t.pattern as usize];
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
                    let pat = &synths[s][t.pattern as usize];
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
                for m in 0..4 {
                    let len = if m < 2 {
                        len_of(synths[m][self.tracks[m].pattern as usize].length)
                    } else {
                        len_of(drums[m - 2][self.tracks[m].pattern as usize].length)
                    };
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
                self.phase = Phase::StepStart;
            }
        }
        self.samples_to_next += half;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{DrumHit, DrumPattern, Gate, SynthPattern, SynthStep};

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
            }
        }

        fn run(&self, seq: &mut Sequencer, total: usize) -> Vec<(usize, SeqEvent)> {
            let mut pos = 0;
            let mut buf = Vec::with_capacity(64);
            let mut out = Vec::new();
            while pos < total {
                let n = seq.samples_to_next_tick().min(total - pos);
                if n == 0 {
                    buf.clear();
                    seq.tick(
                        [&self.synths[0], &self.synths[1]],
                        [&self.drums[0], &self.drums[1]],
                        &mut buf,
                    );
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
}
