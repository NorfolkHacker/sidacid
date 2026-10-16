//! The old 1/16 step patterns, kept as a grid *view* of note patterns:
//! the v4 → v5 migration, the randomiser and the drum grid read and write
//! notes through it. An acid note of n steps is an `On` and n−1 `Tie`s;
//! a drum hit is a one-step note whose pitch is the instrument.

use crate::params::DRUM_INSTRUMENTS;
use crate::project::{BASE_MIDI, MAX_NOTES, Note, NotePattern, STEPS, TICKS_PER_STEP, steps_of};
use serde::{Deserialize, Serialize};

/// Velocity of acid and drum notes. Their engines ignore it.
pub const ACID_VEL: u8 = 100;
const STEP: u16 = TICKS_PER_STEP;

#[derive(Clone, Debug, PartialEq)]
pub struct SynthGrid {
    /// `STEPS` steps.
    pub steps: Vec<SynthStep>,
    pub length: u8,
}

impl SynthGrid {
    /// Every note on the 1/16 grid: its start floors to a step, its length
    /// rounds up to whole steps, and its pitch folds by octaves into the
    /// grid's three octaves (MIDI 24..=59). Later notes overwrite earlier.
    pub fn from_roll(roll: &NotePattern) -> Self {
        let mut steps = vec![SynthStep::default(); STEPS];
        let mut notes: Vec<&Note> = roll.notes.iter().collect();
        notes.sort_by_key(|n| n.start);
        for n in notes {
            let first = (n.start / STEP) as usize;
            if first >= STEPS {
                continue;
            }
            let count = (n.len.max(1) as usize).div_ceil(STEP as usize);
            let mut pitch = n.pitch as i32;
            while pitch < BASE_MIDI - 12 {
                pitch += 12;
            }
            while pitch > BASE_MIDI + 23 {
                pitch -= 12;
            }
            let rel = pitch - BASE_MIDI;
            let base = SynthStep {
                note: rel.rem_euclid(12) as u8,
                octave: rel.div_euclid(12) as i8,
                gate: Gate::On,
                accent: n.accent,
                slide: false,
            };
            let last = (first + count).min(STEPS) - 1;
            for (k, s) in steps.iter_mut().enumerate().take(last + 1).skip(first) {
                *s = SynthStep {
                    gate: if k == first { Gate::On } else { Gate::Tie },
                    accent: k == first && n.accent,
                    slide: k == last && n.slide,
                    ..base
                };
            }
        }
        Self {
            steps,
            length: roll.length,
        }
    }

    /// One note per run (an `On` or a `Tie` with no open note, plus the
    /// `Tie`s after it). Runs are cut at the pattern length, as the old
    /// engine wrapped there; steps past the length convert too (kept
    /// hidden, as the roll keeps notes past the end). Pitch and accent come
    /// from the run's first step, slide from its last.
    ///
    /// A `Tie` on step 0 after a playing last step continued the note
    /// across the loop: the last run then slides into the first, which
    /// takes its pitch (unaccented), so the gate holds over the wrap at
    /// the same pitch. (After a bar line or a pattern switch the next
    /// pattern isn't known here: such a Tie starts its own note.)
    pub fn to_roll(&self) -> NotePattern {
        let len = steps_of(self.length);
        let mut notes = Vec::new();
        let mut in_length = 0;
        for (k, (from, to)) in [(0, len), (len, STEPS)].into_iter().enumerate() {
            if k == 1 {
                in_length = notes.len();
            }
            let mut i = from;
            while i < to {
                let s = self.steps[i];
                if s.gate == Gate::Off {
                    i += 1;
                    continue;
                }
                let mut j = i + 1;
                while j < to && self.steps[j].gate == Gate::Tie {
                    j += 1;
                }
                notes.push(Note {
                    accent: s.accent,
                    slide: self.steps[j - 1].slide,
                    ..Note::new(i as u16 * STEP, (j - i) as u16 * STEP, s.midi(0), ACID_VEL)
                });
                i = j;
            }
        }
        if self.steps[0].gate == Gate::Tie && self.steps[len - 1].gate != Gate::Off {
            // Runs inside the length: the first starts at 0, the last ends at len.
            let last: &mut Note = &mut notes[in_length - 1];
            last.slide = true;
            let pitch = last.pitch;
            notes[0].pitch = pitch;
            notes[0].accent = false;
        }
        NotePattern {
            notes,
            length: self.length,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrumGrid {
    /// `STEPS` steps.
    pub steps: Vec<[DrumHit; DRUM_INSTRUMENTS]>,
    pub length: u8,
}

impl DrumGrid {
    pub fn from_roll(roll: &NotePattern) -> Self {
        let mut steps = vec![[DrumHit::Off; DRUM_INSTRUMENTS]; STEPS];
        for (s, row) in steps.iter_mut().enumerate() {
            for (inst, hit) in row.iter_mut().enumerate() {
                *hit = drum_hit_at(roll, s, inst);
            }
        }
        Self {
            steps,
            length: roll.length,
        }
    }

    /// One one-step note per hit, by step then instrument.
    pub fn to_roll(&self) -> NotePattern {
        let mut notes = Vec::new();
        for (s, row) in self.steps.iter().enumerate() {
            for (inst, hit) in row.iter().enumerate() {
                if *hit != DrumHit::Off {
                    notes.push(Note {
                        accent: *hit == DrumHit::Accent,
                        ..Note::new(s as u16 * STEP, STEP, inst as u8, ACID_VEL)
                    });
                }
            }
        }
        NotePattern {
            notes,
            length: self.length,
        }
    }
}

fn in_step(n: &Note, step: usize, inst: usize) -> bool {
    n.pitch as usize == inst && (n.start / STEP) as usize == step
}

/// The grid cell: Accent if any hit of `inst` starting in `step` is
/// accented, On if there is a hit, else Off.
pub fn drum_hit_at(roll: &NotePattern, step: usize, inst: usize) -> DrumHit {
    let mut hit = DrumHit::Off;
    for n in roll.notes.iter().filter(|n| in_step(n, step, inst)) {
        if n.accent {
            return DrumHit::Accent;
        }
        hit = DrumHit::On;
    }
    hit
}

/// Sets a grid cell: removes `inst`'s hits starting in `step` and, unless
/// `hit` is Off, adds one on the step. False (nothing changed) when the cell
/// already was `hit`, or when adding would exceed `MAX_NOTES`.
pub fn set_drum_hit(roll: &mut NotePattern, step: usize, inst: usize, hit: DrumHit) -> bool {
    if step >= STEPS || inst >= DRUM_INSTRUMENTS || drum_hit_at(roll, step, inst) == hit {
        return false;
    }
    let removed = roll.notes.iter().filter(|n| in_step(n, step, inst)).count();
    if hit != DrumHit::Off && roll.notes.len() - removed >= MAX_NOTES {
        return false;
    }
    roll.notes.retain(|n| !in_step(n, step, inst));
    if hit != DrumHit::Off {
        roll.notes.push(Note {
            accent: hit == DrumHit::Accent,
            ..Note::new(step as u16 * STEP, STEP, inst as u8, ACID_VEL)
        });
    }
    true
}

/// Steps a note covers on the grid, as `SynthGrid::from_roll` shows it:
/// from the step its start falls in, as many as its length rounds up to.
fn grid_span(n: &Note) -> (usize, usize) {
    let first = (n.start / STEP) as usize;
    let count = (n.len.max(1) as usize).div_ceil(STEP as usize);
    (first, (first + count).min(STEPS) - 1)
}

fn end_of(n: &Note) -> u16 {
    n.start.saturating_add(n.len.max(1))
}

/// The note the grid shows on `step`: the latest-starting one covering it.
fn shown_on(roll: &NotePattern, step: usize) -> Option<usize> {
    roll.notes
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            let (first, last) = grid_span(n);
            (first..=last).contains(&step)
        })
        .max_by_key(|(_, n)| n.start)
        .map(|(i, _)| i)
}

/// Notes sounding past tick `at` that started before it now end there.
fn cut_at(roll: &mut NotePattern, at: u16) {
    for n in roll
        .notes
        .iter_mut()
        .filter(|n| n.start < at && end_of(n) > at)
    {
        n.len = at - n.start;
    }
}

/// Sets one grid step by changing only the notes under it, so whatever
/// the grid can't show (chance, ratchet, condition, an off-grid start or
/// length, a pitch outside its octaves) stays. A gate change adds, splits,
/// lengthens, joins or removes notes; otherwise a pitch change moves the
/// whole note by the same interval, accent is its first step's and slide
/// its last step's (marks elsewhere change nothing). A Tie with no note
/// before it starts one, as the old grid played it. False when nothing
/// changed, or when the edit would exceed `MAX_NOTES`.
pub fn set_synth_step(roll: &mut NotePattern, step: usize, value: SynthStep) -> bool {
    if step >= STEPS {
        return false;
    }
    let cur = SynthGrid::from_roll(roll).steps[step];
    if value == cur {
        return false;
    }
    let before = roll.clone();
    let at = step as u16 * STEP;
    let shown = shown_on(roll, step);
    let shift =
        |pitch: u8| (pitch as i32 + value.midi(0) as i32 - cur.midi(0) as i32).clamp(0, 127) as u8;
    match (cur.gate, value.gate, shown) {
        (Gate::Off, Gate::Off, _) => {}
        (Gate::Off, _, _) => {
            let prev = step.checked_sub(1).and_then(|s| shown_on(roll, s));
            match prev {
                Some(p) if value.gate == Gate::Tie => {
                    let n = &mut roll.notes[p];
                    n.len = at + STEP - n.start;
                }
                _ => {
                    if roll.notes.len() >= MAX_NOTES {
                        return false;
                    }
                    cut_at(roll, at);
                    roll.notes.push(Note {
                        accent: value.accent,
                        slide: value.slide,
                        ..Note::new(at, STEP, value.midi(0), ACID_VEL)
                    });
                }
            }
        }
        (Gate::On, Gate::Off, Some(i)) => {
            // Its tail past the step stays as a note; any shorter notes
            // before it in the step go too.
            let n = roll.notes[i];
            let tail = grid_span(&n).1 > step;
            let mut k = 0;
            roll.notes.retain(|m| {
                k += 1;
                (k - 1 == i && tail) || (m.start / STEP) as usize != step
            });
            if let Some(m) = roll.notes.iter_mut().find(|m| **m == n) {
                m.start = at + STEP;
                m.len = end_of(&n) - m.start;
            }
            cut_at(roll, at);
        }
        (Gate::Tie, Gate::Off | Gate::On, Some(i)) => {
            let n = roll.notes[i];
            let (_, last) = grid_span(&n);
            let m = &mut roll.notes[i];
            m.len = at - n.start;
            m.slide = false;
            let tail = if value.gate == Gate::On {
                at
            } else {
                at + STEP
            };
            if end_of(&n) > tail {
                if roll.notes.len() >= MAX_NOTES {
                    *roll = before;
                    return false;
                }
                let on = value.gate == Gate::On;
                roll.notes.push(Note {
                    start: tail,
                    len: end_of(&n) - tail,
                    pitch: if on { shift(n.pitch) } else { n.pitch },
                    accent: on && value.accent,
                    slide: if on && step == last {
                        value.slide
                    } else {
                        n.slide
                    },
                    ..n
                });
            }
        }
        (Gate::On, Gate::Tie, Some(i)) => {
            let Some(p) = step.checked_sub(1).and_then(|s| shown_on(roll, s)) else {
                return false;
            };
            let n = roll.notes[i];
            let q = &mut roll.notes[p];
            q.len = end_of(&n) - q.start;
            q.slide = n.slide;
            // The joined note and any shorter ones before it in the step.
            roll.notes.retain(|m| (m.start / STEP) as usize != step);
        }
        (_, _, Some(i)) => {
            let n = &mut roll.notes[i];
            let (first, last) = grid_span(n);
            if (value.note, value.octave) != (cur.note, cur.octave) {
                n.pitch = shift(n.pitch);
            }
            if value.accent != cur.accent && step == first {
                n.accent = value.accent;
            }
            if value.slide != cur.slide && step == last {
                n.slide = value.slide;
            }
        }
        (_, _, None) => {}
    }
    *roll != before
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gate {
    #[default]
    Off,
    On,
    /// Continue the previous note without retriggering.
    Tie,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthStep {
    pub note: u8,
    pub octave: i8,
    pub gate: Gate,
    pub accent: bool,
    pub slide: bool,
}

impl SynthStep {
    pub fn with_gate(self, gate: Gate) -> Self {
        Self { gate, ..self }
    }

    pub fn midi(&self, transpose: i8) -> u8 {
        (BASE_MIDI + 12 * self.octave as i32 + self.note as i32 + transpose as i32).clamp(0, 127)
            as u8
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrumHit {
    #[default]
    Off,
    On,
    Accent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Note;

    fn step(note: u8, octave: i8, gate: Gate) -> SynthStep {
        SynthStep {
            note,
            octave,
            gate,
            accent: false,
            slide: false,
        }
    }

    fn grid(len: u8, set: &[(usize, SynthStep)]) -> SynthGrid {
        let mut g = SynthGrid {
            steps: vec![SynthStep::default(); STEPS],
            length: len,
        };
        for &(i, s) in set {
            g.steps[i] = s;
        }
        g
    }

    #[test]
    fn a_tie_on_step_0_after_a_playing_last_step_continues_over_the_loop() {
        let mut lead = step(9, 0, Gate::Tie);
        lead.accent = true;
        let mut last = step(5, 0, Gate::On);
        last.accent = true;
        let g = grid(
            16,
            &[
                (0, lead),
                (1, step(0, 0, Gate::Tie)),
                (4, step(2, 0, Gate::On)),
                (14, last),
                (15, step(0, 0, Gate::Tie)),
                (20, step(7, 0, Gate::Tie)), // past the length: untouched
            ],
        );
        assert_eq!(
            g.to_roll().notes,
            vec![
                Note::new(0, 8, 41, ACID_VEL), // the last run's pitch, unaccented
                Note::new(16, 4, 38, ACID_VEL),
                Note {
                    accent: true,
                    slide: true,
                    ..Note::new(56, 8, 41, ACID_VEL)
                },
                Note::new(80, 4, 43, ACID_VEL),
            ]
        );
        // A rest on the last step: the Tie is a note of its own.
        let g = grid(16, &[(0, lead), (14, last)]);
        assert_eq!(
            g.to_roll().notes,
            vec![
                Note {
                    accent: true,
                    ..Note::new(0, 4, 45, ACID_VEL)
                },
                Note {
                    accent: true,
                    ..Note::new(56, 4, 41, ACID_VEL)
                },
            ]
        );
        // One run over the whole loop slides into itself.
        let g = grid(
            4,
            &[
                (0, lead),
                (1, step(0, 0, Gate::Tie)),
                (2, step(0, 0, Gate::Tie)),
                (3, step(0, 0, Gate::Tie)),
            ],
        );
        assert_eq!(
            g.to_roll().notes,
            vec![Note {
                slide: true,
                ..Note::new(0, 16, 45, ACID_VEL)
            }]
        );
    }

    #[test]
    fn an_on_and_its_ties_become_one_note() {
        let mut first = step(3, 0, Gate::On);
        first.accent = true;
        let mut last = step(9, 1, Gate::Tie); // a tie keeps the On's pitch
        last.slide = true;
        let g = grid(16, &[(2, first), (3, step(0, 0, Gate::Tie)), (4, last)]);
        let roll = g.to_roll();
        assert_eq!(roll.length, 16);
        assert_eq!(
            roll.notes,
            vec![Note {
                accent: true,
                slide: true,
                ..Note::new(8, 12, 39, ACID_VEL)
            }]
        );
    }

    #[test]
    fn a_leading_tie_starts_its_own_note_and_runs_stop_at_the_length() {
        let g = grid(
            4,
            &[
                (0, step(0, 0, Gate::Tie)),
                (3, step(5, -1, Gate::On)),
                (4, step(5, -1, Gate::Tie)), // past the length: its own run
                (10, step(7, 0, Gate::On)),  // hidden past the length: kept
            ],
        );
        let notes = g.to_roll().notes;
        // Step 3 plays into step 0's Tie over the loop: it slides, and the
        // Tie's note takes its pitch.
        assert_eq!(
            notes,
            vec![
                Note::new(0, 4, 29, ACID_VEL),
                Note {
                    slide: true,
                    ..Note::new(12, 4, 29, ACID_VEL)
                },
                Note::new(16, 4, 29, ACID_VEL),
                Note::new(40, 4, 43, ACID_VEL),
            ]
        );
    }

    #[test]
    fn on_grid_synth_notes_round_trip() {
        let mut a = step(0, -1, Gate::On);
        a.slide = true;
        let mut b = step(11, 1, Gate::On);
        b.accent = true;
        let g = grid(
            32,
            &[
                (0, a),
                (1, b),
                (2, step(11, 1, Gate::Tie)),
                (31, step(4, 0, Gate::On)),
            ],
        );
        let back = SynthGrid::from_roll(&g.to_roll());
        assert_eq!(back.length, 32);
        assert_eq!(back.steps[..3], g.steps[..3]);
        assert_eq!(back.steps[31], g.steps[31]);
    }

    #[test]
    fn off_grid_and_out_of_range_notes_snap_into_the_grid() {
        let roll = NotePattern {
            notes: vec![Note::new(5, 9, 90, 100), Note::new(20, 1, 10, 100)],
            length: 16,
        };
        let g = SynthGrid::from_roll(&roll);
        // tick 5 is in step 1; 9 ticks round up to 3 steps; 90 folds to 54.
        assert_eq!(g.steps[1], step(6, 1, Gate::On));
        assert_eq!(g.steps[2].gate, Gate::Tie);
        assert_eq!(g.steps[3].gate, Gate::Tie);
        assert_eq!(g.steps[4].gate, Gate::Off);
        // tick 20 is step 5; pitch 10 folds up to 34 (A#, octave -1).
        assert_eq!(g.steps[5], step(10, -1, Gate::On));
    }

    #[test]
    fn drum_grid_round_trips_and_hits_toggle() {
        let mut g = DrumGrid {
            steps: vec![[DrumHit::Off; DRUM_INSTRUMENTS]; STEPS],
            length: 16,
        };
        g.steps[0][0] = DrumHit::Accent;
        g.steps[0][6] = DrumHit::On;
        g.steps[63][7] = DrumHit::On;
        let roll = g.to_roll();
        assert_eq!(
            roll.notes[0],
            Note {
                accent: true,
                ..Note::new(0, 4, 0, ACID_VEL)
            }
        );
        assert_eq!(roll.notes[1], Note::new(0, 4, 6, ACID_VEL));
        assert_eq!(roll.notes[2], Note::new(252, 4, 7, ACID_VEL));
        assert_eq!(DrumGrid::from_roll(&roll), g);

        let mut roll = roll;
        assert_eq!(drum_hit_at(&roll, 0, 0), DrumHit::Accent);
        assert!(set_drum_hit(&mut roll, 0, 0, DrumHit::On));
        assert_eq!(drum_hit_at(&roll, 0, 0), DrumHit::On);
        assert!(!set_drum_hit(&mut roll, 0, 0, DrumHit::On), "no change");
        assert!(set_drum_hit(&mut roll, 0, 0, DrumHit::Off));
        assert_eq!(drum_hit_at(&roll, 0, 0), DrumHit::Off);
        // An off-grid hit inside step 2 counts as that step's hit.
        roll.notes.push(Note::new(9, 4, 1, 100));
        assert_eq!(drum_hit_at(&roll, 2, 1), DrumHit::On);
        assert!(set_drum_hit(&mut roll, 2, 1, DrumHit::Off));
        assert!(roll.notes.iter().all(|n| n.pitch != 1));
    }

    #[test]
    fn a_full_drum_grid_fits_a_note_pattern() {
        let g = DrumGrid {
            steps: vec![[DrumHit::On; DRUM_INSTRUMENTS]; STEPS],
            length: 64,
        };
        let roll = g.to_roll();
        assert_eq!(roll.notes.len(), STEPS * DRUM_INSTRUMENTS);
        assert!(roll.is_well_formed());
    }

    #[test]
    fn set_drum_hit_refuses_to_exceed_the_note_cap() {
        let mut roll = DrumGrid {
            steps: vec![[DrumHit::On; DRUM_INSTRUMENTS]; STEPS],
            length: 64,
        }
        .to_roll();
        roll.notes.truncate(MAX_NOTES);
        // Free cell would need a new note: refused.
        roll.notes.pop();
        roll.notes.push(Note::new(0, 4, 100, 100)); // filler off the drum pitches
        assert_eq!(roll.notes.len(), MAX_NOTES);
        assert!(!set_drum_hit(&mut roll, 63, 7, DrumHit::On));
        assert_eq!(roll.notes.len(), MAX_NOTES);
        // An occupied cell can still be replaced or cleared.
        assert!(set_drum_hit(&mut roll, 0, 0, DrumHit::Accent));
        assert_eq!(roll.notes.len(), MAX_NOTES);
        assert!(set_drum_hit(&mut roll, 0, 0, DrumHit::Off));
        assert!(set_drum_hit(&mut roll, 63, 7, DrumHit::On));
    }

    fn roll(notes: Vec<Note>) -> NotePattern {
        NotePattern { notes, length: 16 }
    }

    /// A note with every field the grid can't show set away from its default.
    fn rich(start: u16, len: u16, pitch: u8) -> Note {
        Note {
            chance: 50,
            ratchet: 3,
            cond: crate::project::Cond::Fill,
            ..Note::new(start, len, pitch, 77)
        }
    }

    fn at(r: &NotePattern, s: usize) -> SynthStep {
        SynthGrid::from_roll(r).steps[s]
    }

    #[test]
    fn a_step_set_to_what_it_shows_changes_nothing() {
        let mut r = roll(vec![rich(0, 8, 40)]);
        let before = r.clone();
        assert!(!set_synth_step(&mut r, 1, at(&before, 1)));
        assert!(!set_synth_step(&mut r, 5, at(&before, 5)));
        assert_eq!(r, before);
    }

    #[test]
    fn a_gate_on_a_rest_adds_a_one_step_note() {
        let mut r = roll(vec![]);
        let value = SynthStep {
            accent: true,
            ..step(4, 0, Gate::On)
        };
        assert!(set_synth_step(&mut r, 3, value));
        assert_eq!(
            r.notes,
            vec![Note {
                accent: true,
                ..Note::new(12, 4, 40, ACID_VEL)
            }]
        );
    }

    #[test]
    fn a_new_note_cuts_a_tail_that_spills_into_its_step() {
        // Starts in step 0 and sounds into step 1, which the grid shows as a rest.
        let mut r = roll(vec![rich(2, 4, 40)]);
        assert_eq!(at(&r, 1).gate, Gate::Off);
        assert!(set_synth_step(&mut r, 1, step(0, 0, Gate::On)));
        assert_eq!(r.notes, vec![rich(2, 2, 40), Note::new(4, 4, 36, ACID_VEL)]);
    }

    #[test]
    fn a_rest_on_a_note_start_keeps_its_tail_as_a_note() {
        let mut r = roll(vec![rich(0, 4, 40), rich(8, 12, 43)]);
        assert!(set_synth_step(&mut r, 0, SynthStep::default()));
        assert_eq!(r.notes, vec![rich(8, 12, 43)]);
        assert!(set_synth_step(&mut r, 2, SynthStep::default()));
        assert_eq!(
            r.notes,
            vec![rich(12, 8, 43)],
            "starts a step later, same end"
        );
    }

    #[test]
    fn a_tie_on_a_rest_lengthens_the_note_before() {
        let mut r = roll(vec![Note {
            slide: true,
            ..rich(0, 4, 40)
        }]);
        assert!(set_synth_step(&mut r, 1, step(4, 0, Gate::Tie)));
        assert_eq!(
            r.notes,
            vec![Note {
                slide: true,
                ..rich(0, 8, 40)
            }]
        );
        // No note before: the tie starts one, as the old grid played it.
        let mut r = roll(vec![]);
        assert!(set_synth_step(&mut r, 2, step(4, 0, Gate::Tie)));
        assert_eq!(r.notes, vec![Note::new(8, 4, 40, ACID_VEL)]);
    }

    #[test]
    fn a_tie_on_a_note_start_joins_it_to_the_note_before() {
        let later = Note {
            slide: true,
            accent: true,
            ..Note::new(4, 8, 50, ACID_VEL)
        };
        let mut r = roll(vec![rich(0, 4, 40), later]);
        let tie = at(&r, 1).with_gate(Gate::Tie);
        assert!(set_synth_step(&mut r, 1, tie));
        assert_eq!(
            r.notes,
            vec![Note {
                slide: true,
                ..rich(0, 12, 40)
            }],
            "the earlier note's settings, the later one's slide"
        );
        // Nothing before to join: unchanged.
        let mut r = roll(vec![rich(4, 4, 40)]);
        let tie = at(&r, 1).with_gate(Gate::Tie);
        assert!(!set_synth_step(&mut r, 1, tie));
    }

    #[test]
    fn a_rest_inside_a_note_splits_it() {
        let mut r = roll(vec![Note {
            accent: true,
            slide: true,
            ..rich(0, 16, 40)
        }]);
        assert!(set_synth_step(&mut r, 1, SynthStep::default()));
        assert_eq!(
            r.notes,
            vec![
                Note {
                    accent: true,
                    ..rich(0, 4, 40)
                },
                Note {
                    slide: true,
                    ..rich(8, 8, 40)
                },
            ]
        );
        // On its last step: just shorter.
        let mut r = roll(vec![rich(0, 8, 40)]);
        assert!(set_synth_step(&mut r, 1, SynthStep::default()));
        assert_eq!(r.notes, vec![rich(0, 4, 40)]);
    }

    #[test]
    fn a_gate_inside_a_note_splits_it_there_with_the_new_pitch() {
        let mut r = roll(vec![Note {
            slide: true,
            ..rich(0, 12, 40)
        }]);
        let value = SynthStep {
            accent: true,
            ..step(7, 0, Gate::On)
        };
        assert!(set_synth_step(&mut r, 1, value));
        assert_eq!(
            r.notes,
            vec![
                rich(0, 4, 40),
                Note {
                    accent: true,
                    slide: true,
                    ..rich(4, 8, 43)
                },
            ]
        );
    }

    #[test]
    fn pitch_moves_the_whole_note_and_keeps_everything_else() {
        // Off the grid in time and above the grid's octaves.
        let mut r = roll(vec![rich(5, 9, 72)]);
        let shown = at(&r, 1);
        assert_eq!((shown.note, shown.octave), (0, 1), "72 folds to 48");
        let up = SynthStep {
            note: 2,
            ..at(&r, 2)
        };
        assert!(set_synth_step(&mut r, 2, up));
        assert_eq!(r.notes, vec![rich(5, 9, 74)], "a tie step moves it too");
        let down = SynthStep {
            octave: -1,
            ..at(&r, 1)
        };
        assert!(set_synth_step(&mut r, 1, down));
        assert_eq!(r.notes, vec![rich(5, 9, 50)]);
    }

    #[test]
    fn accent_is_the_first_step_and_slide_the_last() {
        let mut r = roll(vec![rich(0, 12, 40)]);
        let toggle = |r: &mut NotePattern, s: usize, f: fn(&mut SynthStep)| {
            let mut v = at(r, s);
            f(&mut v);
            set_synth_step(r, s, v)
        };
        assert!(
            !toggle(&mut r, 1, |v| v.accent = true),
            "a tie step has no accent"
        );
        assert!(
            !toggle(&mut r, 1, |v| v.slide = true),
            "nor a slide before the last"
        );
        assert!(toggle(&mut r, 0, |v| v.accent = true));
        assert!(toggle(&mut r, 2, |v| v.slide = true));
        assert_eq!(
            r.notes,
            vec![Note {
                accent: true,
                slide: true,
                ..rich(0, 12, 40)
            }]
        );
    }

    #[test]
    fn every_gate_edit_shows_on_the_grid() {
        let base = roll(vec![
            rich(0, 4, 40),
            rich(4, 12, 43),
            rich(20, 4, 36),
            rich(26, 3, 38),
        ]);
        for s in 0..12 {
            for gate in [Gate::Off, Gate::On, Gate::Tie] {
                let mut r = base.clone();
                let value = at(&r, s).with_gate(gate);
                set_synth_step(&mut r, s, value);
                assert!(r.is_well_formed());
                let shown = at(&r, s).gate;
                let want = if gate == Gate::Tie && (s == 0 || at(&base, s - 1).gate == Gate::Off) {
                    Gate::On
                } else {
                    gate
                };
                assert_eq!(shown, want, "step {s} set to {gate:?}");
                let mut sorted = r.notes.clone();
                sorted.sort_by_key(|n| n.start);
                for w in sorted.windows(2) {
                    assert!(
                        w[0].start + w[0].len <= w[1].start,
                        "no overlap: {sorted:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn set_synth_step_refuses_to_exceed_the_note_cap() {
        let mut r = roll(vec![Note::new(255, 1, 40, 1); MAX_NOTES]);
        assert!(!set_synth_step(&mut r, 0, step(0, 0, Gate::On)));
        assert_eq!(r.notes.len(), MAX_NOTES);
    }
}
