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
    pub fn to_roll(&self) -> NotePattern {
        let len = steps_of(self.length);
        let mut notes = Vec::new();
        for (from, to) in [(0, len), (len, STEPS)] {
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
        assert_eq!(
            notes,
            vec![
                Note::new(0, 4, 36, ACID_VEL),
                Note::new(12, 4, 29, ACID_VEL),
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
}
