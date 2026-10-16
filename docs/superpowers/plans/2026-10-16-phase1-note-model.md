# Phase 1: Unified Note Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Acid synths and drums store their patterns as tick-based notes (1/64 resolution) like the FM, sampler and Mono; old projects convert and sound identical; the 303 step view is removed and the acid synth is edited in the piano roll.

**Architecture:** `Note` gains accent/slide/chance/ratchet/cond fields. `SynthPattern` and `DrumPattern` keep their names (so pattern banks stay type-safe) but hold a `roll: NotePattern` instead of step arrays. A new `acid-engine/src/steps.rs` keeps the old 1/16 step types as a *grid view* with lossless conversions, used by the v4→v5 migration, the randomiser and the drum grid UI. The sequencer runs acid and drum tracks per tick, reproducing the old 303 gate timing exactly; the bit-exact golden render tests are the acceptance gate.

**Tech Stack:** Rust 2024 (1.88+), serde + RON, egui/eframe, cargo test.

**Spec:** `docs/superpowers/specs/2026-10-16-dock-song-lanes-design.md` (section 1). Phase 2 (the dock) builds on this.

## Global Constraints

- User-facing text never names a commercial product or manufacturer.
- `PROJECT_VERSION` becomes 5. Versions 0–4 must still load.
- The golden hashes in `crates/acid-engine/tests/golden.rs` (`PATTERN_HASH = 0x37e7_dc98_f0d6_23f1`, `SONG_HASH = 0xb18d_e278_6f54_1314`) must NOT change.
- The audio thread never allocates (`crates/acid-engine/tests/no_alloc.rs` must pass).
- `TICKS_PER_STEP` stays 4 (1 tick = 1/64 note).
- Work on a branch `phase1-note-model`. Commit identity: the repo has no global git identity; commit with `git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit ...`, and end every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Before each commit: `cargo fmt --all`.
- Between Task 3 and the end of Task 4 the `app` crate does not compile (the engine types change under it). Run engine tests with `cargo test -p acid-engine` in that window. From Task 4 on, `cargo test --workspace` must pass.

## The acid gate rule (read before Tasks 2 and 3)

The old 303 step sequencer, per 1/16 step (4 ticks):
- tick 0 of a step: an `On` step starts a note (legato if the previous step slid and the gate is still open); a `Tie` continues; an `Off` closes an open gate.
- tick 2 (mid-step): an open gate closes unless `(step.slide && next.gate != Off) || next.gate == Tie`, where `next` is the same pattern's next step (wrapping).

So an `On` followed by n−1 `Tie`s sounds as one note whose gate closes at tick `4n − 2`. The new model stores that as one `Note { start: 4·s, len: 4·n }` and the engine closes an acid gate **2 ticks before the note's written end** (at least 1 tick after it starts). A note with `slide` whose end is exactly where another note starts keeps the gate open and that next note plays legato. This reproduces the old timing to the tick, and a one-step note drawn in the roll gets the 303's half-step gate.

---

### Task 1: Note fields and trig condition type

**Files:**
- Modify: `crates/acid-engine/src/project.rs` (the `Note` struct ~line 124, `NotePattern::is_well_formed` ~line 149, tests module)
- Modify: every `Note { start, len, pitch, vel }` literal: `crates/acid-engine/src/engine.rs`, `project.rs`, `render.rs`, `sequencer.rs`, `tests/no_alloc.rs`, `crates/app/src/document.rs`, `crates/app/src/ui/mod.rs`, `crates/app/src/ui/poly_edit.rs` (find them with `grep -rn "Note {" crates --include=*.rs | grep -v "SeqEvent\|RollNote\|PolyOn\|struct Note"`)

**Interfaces:**
- Produces: `Note { start: u16, len: u16, pitch: u8, vel: u8, accent: bool, slide: bool, chance: u8, ratchet: u8, cond: Cond }`, `Note::new(start, len, pitch, vel) -> Note` (const fn, other fields default), `enum Cond { Always, Ratio(u8, u8), First, NotFirst, Fill, NotFill, Pre, NotPre }` with `Default = Always` and `Cond::is_valid(self) -> bool`.

- [ ] **Step 1: Create the branch**

```bash
git switch -c phase1-note-model
```

- [ ] **Step 2: Write the failing tests** (append to `mod tests` in `project.rs`)

```rust
    #[test]
    fn notes_save_without_default_fields_and_load_them_back() {
        let n = Note::new(4, 4, 60, 100);
        let text = ron::ser::to_string(&n).unwrap();
        assert_eq!(text, "(start:4,len:4,pitch:60,vel:100)");
        let back: Note = ron::from_str(&text).unwrap();
        assert_eq!(back, n);
        assert_eq!((back.chance, back.ratchet, back.cond), (100, 1, Cond::Always));
        let flagged = Note {
            accent: true,
            slide: true,
            chance: 50,
            ratchet: 3,
            cond: Cond::Ratio(1, 4),
            ..n
        };
        let back: Note = ron::from_str(&ron::ser::to_string(&flagged).unwrap()).unwrap();
        assert_eq!(back, flagged);
    }

    #[test]
    fn note_fields_are_validated() {
        let ok = |n: Note| NotePattern {
            notes: vec![n],
            length: 16,
        }
        .is_well_formed();
        let n = Note::new(0, 4, 60, 100);
        assert!(ok(n));
        assert!(!ok(Note { chance: 101, ..n }));
        assert!(!ok(Note { ratchet: 0, ..n }));
        assert!(!ok(Note { ratchet: 9, ..n }));
        assert!(ok(Note { ratchet: 8, ..n }));
        assert!(!ok(Note { cond: Cond::Ratio(0, 4), ..n }));
        assert!(!ok(Note { cond: Cond::Ratio(5, 4), ..n }));
        assert!(!ok(Note { cond: Cond::Ratio(1, 9), ..n }));
        assert!(ok(Note { cond: Cond::Ratio(4, 4), ..n }));
    }
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p acid-engine notes_save_without_default_fields`
Expected: compile error (`Note::new`, `Cond` not found).

- [ ] **Step 4: Implement** — replace the `Note` struct in `project.rs` with:

```rust
/// When a note plays, beyond its chance (spec phase 6; stored from v5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cond {
    #[default]
    Always,
    /// Plays on pass `a` of every `b` loops (1 <= a <= b <= 8).
    Ratio(u8, u8),
    First,
    NotFirst,
    Fill,
    NotFill,
    /// The lane's previous conditional note played.
    Pre,
    NotPre,
}

impl Cond {
    pub fn is_valid(self) -> bool {
        match self {
            Cond::Ratio(a, b) => (1..=8).contains(&b) && (1..=b).contains(&a),
            _ => true,
        }
    }

    fn is_always(&self) -> bool {
        *self == Cond::Always
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn full_chance() -> u8 {
    100
}

fn is_full_chance(c: &u8) -> bool {
    *c == 100
}

fn one(n: &u8) -> bool {
    *n == 1
}

fn single() -> u8 {
    1
}

/// One note of a note pattern, in ticks (`TICKS_PER_STEP` per step).
/// Fields past `vel` are skipped when at their defaults, so a plain
/// FM or sampler note saves as before.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub start: u16,
    pub len: u16,
    pub pitch: u8,
    pub vel: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub accent: bool,
    /// Acid: glide into a note starting where this one ends.
    #[serde(default, skip_serializing_if = "is_false")]
    pub slide: bool,
    /// Play chance in percent, 0..=100.
    #[serde(default = "full_chance", skip_serializing_if = "is_full_chance")]
    pub chance: u8,
    /// Even re-triggers within the note, 1..=8.
    #[serde(default = "single", skip_serializing_if = "one")]
    pub ratchet: u8,
    #[serde(default, skip_serializing_if = "Cond::is_always")]
    pub cond: Cond,
}

impl Note {
    pub const fn new(start: u16, len: u16, pitch: u8, vel: u8) -> Self {
        Self {
            start,
            len,
            pitch,
            vel,
            accent: false,
            slide: false,
            chance: 100,
            ratchet: 1,
            cond: Cond::Always,
        }
    }
}
```

and in `NotePattern::is_well_formed` extend the per-note check:

```rust
            && self.notes.iter().all(|n| {
                n.len >= 1
                    && n.start < max_start
                    && n.pitch <= 127
                    && (1..=127).contains(&n.vel)
                    && n.chance <= 100
                    && (1..=8).contains(&n.ratchet)
                    && n.cond.is_valid()
            })
```

- [ ] **Step 5: Replace every `Note { start, len, pitch, vel }` literal** with `Note::new(start, len, pitch, vel)`. Literals that use `..` update syntax are left as they are. Example, `poly_edit.rs` `add`:

```rust
    p.notes.push(Note::new(start, len, pitch, vel.clamp(1, 127)));
```

- [ ] **Step 6: Run the whole workspace**

Run: `cargo test --workspace`
Expected: all pass, including `golden.rs`.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add -A
git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit -m "feat(engine): notes carry accent, slide, chance, ratchet and a trig condition

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: The step-grid view and its conversions

**Files:**
- Create: `crates/acid-engine/src/steps.rs`
- Modify: `crates/acid-engine/src/lib.rs` (add `pub mod steps;`)
- Modify: `crates/acid-engine/src/project.rs` (move `Gate`, `SynthStep` and `DrumHit` out; re-export them)

**Interfaces:**
- Consumes: `Note::new`, `NotePattern`, `BASE_MIDI`, `STEPS`, `TICKS_PER_STEP`, `steps_of` (project.rs); `DRUM_INSTRUMENTS` (params.rs).
- Produces (all `pub` in `acid_engine::steps`):
  - `Gate`, `SynthStep` (with `midi(transpose)`), `DrumHit`: moved unchanged from project.rs, and re-exported from `project` via `pub use crate::steps::{DrumHit, Gate, SynthStep};` so existing paths keep working until Task 3.
  - `const ACID_VEL: u8 = 100` (velocity of acid and drum notes; the engine ignores it).
  - `struct SynthGrid { pub steps: Vec<SynthStep>, pub length: u8 }` with `from_roll(&NotePattern) -> SynthGrid` and `to_roll(&self) -> NotePattern`.
  - `struct DrumGrid { pub steps: Vec<[DrumHit; DRUM_INSTRUMENTS]>, pub length: u8 }` with `from_roll` and `to_roll`.
  - `fn drum_hit_at(roll: &NotePattern, step: usize, inst: usize) -> DrumHit`
  - `fn set_drum_hit(roll: &mut NotePattern, step: usize, inst: usize, hit: DrumHit) -> bool` (true when changed).

- [ ] **Step 1: Move the types.** Cut `Gate`, `SynthStep` (with its `impl`) and `DrumHit` from `project.rs` into the new `steps.rs`, and add `pub use crate::steps::{DrumHit, Gate, SynthStep};` near the top of `project.rs`. Add `pub mod steps;` to `lib.rs`. Run `cargo test --workspace`. Expected: PASS (pure move).

- [ ] **Step 2: Write the failing tests** (in `steps.rs`)

```rust
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
        let g = grid(32, &[(0, a), (1, b), (2, step(11, 1, Gate::Tie)), (31, step(4, 0, Gate::On))]);
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
        assert_eq!(roll.notes[0], Note { accent: true, ..Note::new(0, 4, 0, ACID_VEL) });
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
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test -p acid-engine steps::`
Expected: compile errors (`SynthGrid`, `DrumGrid`, `ACID_VEL`, ... not found).

- [ ] **Step 4: Implement** (add to `steps.rs` above the tests; keep the moved types)

```rust
//! The old 1/16 step patterns, kept as a grid *view* of note patterns:
//! the v4 → v5 migration, the randomiser and the drum grid read and write
//! notes through it. An acid note of n steps is an `On` and n−1 `Tie`s;
//! a drum hit is a one-step note whose pitch is the instrument.

use crate::params::DRUM_INSTRUMENTS;
use crate::project::{BASE_MIDI, Note, NotePattern, STEPS, TICKS_PER_STEP, steps_of};
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
                    ..Note::new(
                        i as u16 * STEP,
                        (j - i) as u16 * STEP,
                        s.midi(0),
                        ACID_VEL,
                    )
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
/// `hit` is Off, adds one on the step. False when the cell already was `hit`.
pub fn set_drum_hit(roll: &mut NotePattern, step: usize, inst: usize, hit: DrumHit) -> bool {
    if step >= STEPS || inst >= DRUM_INSTRUMENTS || drum_hit_at(roll, step, inst) == hit {
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
```

The fold keeps `octave` in −1..=1 (MIDI 24..=59, the old editor's three octaves): pitch 90 → 78 → 66 → 54, `rel` 18 → note 6, octave 1; pitch 10 → 22 → 34, `rel` −2 → note 10, octave −1.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p acid-engine steps::`
Expected: PASS (6 tests). Then `cargo test --workspace`: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add -A
git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit -m "feat(engine): a 1/16 step-grid view over note patterns

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Acid and drum patterns become notes (engine crate)

**Files:**
- Modify: `crates/acid-engine/src/project.rs` (pattern types, `SynthMachine`, `SeqView` removal, `roll`/`roll_mut`, version, validation, `from_ron`, tests)
- Modify: `crates/acid-engine/src/migrate.rs` (frozen v4 types and the v4 → v5 upgrade; v1/v2 upgrade through v4)
- Modify: `crates/acid-engine/src/sequencer.rs` (per-tick acid and drum tracks, tests)
- Modify: `crates/acid-engine/src/song.rs` (`Bank::length`, tests)
- Modify: `crates/acid-engine/src/random.rs` (grid adapters)
- Modify: `crates/acid-engine/src/demo.rs`, `host.rs` tests, `engine.rs` tests, `tests/no_alloc.rs`
- Test: `crates/acid-engine/tests/golden.rs` (unchanged; must pass)

**Interfaces:**
- Consumes: Task 1 `Note`; Task 2 `SynthGrid`, `DrumGrid`, `ACID_VEL`, `Gate`, `SynthStep`, `DrumHit`.
- Produces:
  - `pub struct SynthPattern { pub roll: NotePattern, pub transpose: i8, pub param_lane: Option<ParamLane> }`
  - `pub struct DrumPattern { pub roll: NotePattern }`
  - `pub struct SynthMachine { pub patterns: Vec<SynthPattern> }` (no `view`)
  - `Project::roll(&self, uid, slot: u8) -> Option<&NotePattern>` and `Project::roll_mut(&mut self, uid, slot: u8) -> Option<&mut NotePattern>` for every machine type.
  - `PROJECT_VERSION = 5`. `SeqView` lives on only in `migrate.rs`.
  - `project` no longer re-exports `Gate`/`SynthStep`/`DrumHit`; use `acid_engine::steps::…`.

- [ ] **Step 1: Change the pattern types** in `project.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthPattern {
    /// Monophonic notes; the roll editor keeps them from overlapping.
    pub roll: NotePattern,
    pub transpose: i8,
    /// One optional value per 1/16 step.
    pub param_lane: Option<ParamLane>,
}

impl SynthPattern {
    /// Shape the engine relies on: a well-formed roll and a full-size lane.
    pub fn is_well_formed(&self) -> bool {
        self.roll.is_well_formed()
            && self
                .param_lane
                .as_ref()
                .is_none_or(|l| l.values.len() == STEPS)
    }
}

impl Default for SynthPattern {
    fn default() -> Self {
        Self {
            roll: NotePattern::default(),
            transpose: 0,
            param_lane: None,
        }
    }
}

/// A drum pattern: one-step notes whose pitch is the instrument (0..8).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DrumPattern {
    pub roll: NotePattern,
}

impl DrumPattern {
    pub fn is_well_formed(&self) -> bool {
        self.roll.is_well_formed()
            && self
                .roll
                .notes
                .iter()
                .all(|n| (n.pitch as usize) < DRUM_INSTRUMENTS)
    }
}
```

Delete `SeqView` and `SynthMachine::view` (and the `pub use crate::steps::…` re-export from Task 2). `SynthMachine` becomes `#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)] pub struct SynthMachine { pub patterns: Vec<SynthPattern> }` with a `Default` that fills `PATTERNS` patterns (keep the manual `impl Default`, minus `view`).

Add next to `note_patterns`:

```rust
    /// Pattern `slot`'s notes, for any machine.
    pub fn roll(&self, uid: MachineUid, slot: u8) -> Option<&NotePattern> {
        let s = slot as usize;
        match &self.machine(uid)?.body {
            MachineBody::Synth(m) => Some(&m.patterns.get(s)?.roll),
            MachineBody::Kit(m) => Some(&m.patterns.get(s)?.roll),
            MachineBody::Fm(m) => m.patterns.get(s),
            MachineBody::Mono(m) => m.patterns.get(s),
            MachineBody::Sampler(m) => m.patterns.get(s),
        }
    }

    pub fn roll_mut(&mut self, uid: MachineUid, slot: u8) -> Option<&mut NotePattern> {
        let s = slot as usize;
        match &mut self.machine_mut(uid)?.body {
            MachineBody::Synth(m) => Some(&mut m.patterns.get_mut(s)?.roll),
            MachineBody::Kit(m) => Some(&mut m.patterns.get_mut(s)?.roll),
            MachineBody::Fm(m) => m.patterns.get_mut(s),
            MachineBody::Mono(m) => m.patterns.get_mut(s),
            MachineBody::Sampler(m) => m.patterns.get_mut(s),
        }
    }
```

Set `pub const PROJECT_VERSION: u32 = 5;`.

- [ ] **Step 2: Freeze the v4 format in `migrate.rs`.** Add frozen copies of today's (pre-change) types, named with a `V4` suffix, each deriving `Clone, Debug, PartialEq, Serialize, Deserialize`, with the same field names and order as before this task so v3/v4 RON parses unchanged:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SeqView {
    #[default]
    Step303,
    PianoRoll,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SynthPatternV4 {
    pub steps: Vec<SynthStep>,
    pub length: u8,
    pub transpose: i8,
    pub param_lane: Option<ParamLane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SynthMachineV4 {
    pub patterns: Vec<SynthPatternV4>,
    pub view: SeqView,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DrumPatternV4 {
    pub steps: Vec<[DrumHit; DRUM_INSTRUMENTS]>,
    pub length: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DrumMachineV4 {
    pub kind: KitKind,
    pub patterns: Vec<DrumPatternV4>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum MachineBodyV4 {
    Synth(SynthMachineV4),
    Kit(DrumMachineV4),
    Fm(FmMachine),
    Sampler(SamplerMachine),
    Mono(MonoMachine),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct MachineEntryV4 {
    pub uid: MachineUid,
    pub name: String,
    pub selected: u8,
    pub params: BTreeMap<MachineParam, f32>,
    pub body: MachineBodyV4,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProjectV4 {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub machines: Vec<MachineEntryV4>,
    pub next_uid: u16,
    pub globals: BTreeMap<GlobalParam, f32>,
    pub song: Song,
}
```

Conversions (each pattern goes through the Task 2 grid, so there is exactly one conversion rule):

```rust
impl SynthPatternV4 {
    fn upgrade(self) -> SynthPattern {
        SynthPattern {
            roll: SynthGrid {
                steps: self.steps,
                length: self.length,
            }
            .to_roll(),
            transpose: self.transpose,
            param_lane: self.param_lane,
        }
    }
}

impl DrumPatternV4 {
    fn upgrade(self) -> DrumPattern {
        DrumPattern {
            roll: DrumGrid {
                steps: self.steps,
                length: self.length,
            }
            .to_roll(),
        }
    }
}

impl MachineBodyV4 {
    fn upgrade(self) -> MachineBody {
        match self {
            MachineBodyV4::Synth(s) => MachineBody::Synth(SynthMachine {
                patterns: s.patterns.into_iter().map(SynthPatternV4::upgrade).collect(),
            }),
            MachineBodyV4::Kit(k) => MachineBody::Kit(DrumMachine {
                kind: k.kind,
                patterns: k.patterns.into_iter().map(DrumPatternV4::upgrade).collect(),
            }),
            MachineBodyV4::Fm(f) => MachineBody::Fm(f),
            MachineBodyV4::Sampler(s) => MachineBody::Sampler(s),
            MachineBodyV4::Mono(m) => MachineBody::Mono(m),
        }
    }
}

impl ProjectV4 {
    /// A v5 project: acid and drum steps become notes.
    pub(crate) fn upgrade(self) -> Project {
        Project {
            version: PROJECT_VERSION,
            name: self.name,
            bpm: self.bpm,
            swing: self.swing,
            machines: self
                .machines
                .into_iter()
                .map(|m| MachineEntry {
                    uid: m.uid,
                    name: m.name,
                    selected: m.selected,
                    params: m.params,
                    body: m.body.upgrade(),
                })
                .collect(),
            next_uid: self.next_uid,
            globals: self.globals,
            song: self.song,
        }
    }
}
```

Then make the old chain end in v4: `SynthMachineV2::upgrade` returns `SynthMachineV4` (patterns → `SynthPatternV4 { steps, length, transpose, param_lane }`), `DrumMachineV2` holds `Vec<DrumPatternV4>` (its `#[cfg(test)] Default` uses `DrumPatternV4 { steps: vec![[DrumHit::Off; DRUM_INSTRUMENTS]; STEPS], length: 16 }`), and `ProjectV2::upgrade` returns `ProjectV4`. Build it the way it builds a `Project` today, but from a default v4 project: add `#[cfg(test)]`-free helper `ProjectV4::standard()` that mirrors `Project::default()` with `MachineBodyV4` bodies (synths `SynthMachineV4 { patterns: vec![empty v4 pattern; PATTERNS], view: SeqView::Step303 }`, kits `DrumMachineV4 { kind, patterns: vec![empty; PATTERNS] }`, FM/Sampler/Mono defaults, params from `MachineParam::all_for`, `next_uid: 7`, default globals). `ProjectV2::upgrade`'s param loop needs a `machine_mut`-style lookup: use `p.machines.iter_mut().find(|m| m.uid == uid)`; for globals call `p.globals.insert(g, v)` where `global.global()` gives the `GlobalParam` (mirror `Project::set_param`'s global branch).

- [ ] **Step 3: Route versions in `Project::from_ron`:**

```rust
        let p = match head.version {
            0 | 1 => ron::from_str::<crate::migrate::ProjectV1>(text)
                .map_err(parse)?
                .upgrade()
                .upgrade()
                .upgrade(),
            2 => with_pcm_budget(|| ron::from_str::<crate::migrate::ProjectV2>(text))
                .map_err(parse)?
                .upgrade()
                .upgrade(),
            // v4 only added the Mono machine: a v3 file reads as v4.
            3 | 4 => with_pcm_budget(|| ron::from_str::<crate::migrate::ProjectV4>(text))
                .map_err(parse)?
                .upgrade(),
            PROJECT_VERSION => with_pcm_budget(|| ron::from_str::<Project>(text)).map_err(parse)?,
            v => return Err(ProjectError::TooNew(v)),
        };
```

- [ ] **Step 4: Write the migration test** (in `migrate.rs` tests)

```rust
    #[test]
    fn v4_steps_upgrade_to_notes() {
        use crate::project::{Note, PROJECT_VERSION, Project};
        use crate::steps::{ACID_VEL, DrumHit, Gate, SynthStep};
        let mut v4 = ProjectV4::standard();
        let MachineBodyV4::Synth(s) = &mut v4.machines[0].body else {
            panic!("position 0 is Synth 1")
        };
        let first = SynthStep {
            note: 0,
            octave: 0,
            gate: Gate::On,
            accent: true,
            slide: false,
        };
        s.patterns[0].steps[0] = first;
        s.patterns[0].steps[1] = SynthStep {
            gate: Gate::Tie,
            slide: true,
            ..first
        };
        let MachineBodyV4::Kit(k) = &mut v4.machines[2].body else {
            panic!("position 2 is the 808")
        };
        k.patterns[0].steps[4][1] = DrumHit::Accent;
        let up = Project::from_ron(&ron::ser::to_string(&v4).unwrap()).unwrap();
        assert_eq!(up.version, PROJECT_VERSION);
        let synth = up.synth(MachineUid(1)).unwrap();
        assert_eq!(
            synth.patterns[0].roll.notes,
            vec![Note { accent: true, slide: true, ..Note::new(0, 8, 36, ACID_VEL) }]
        );
        let kit = up.kit(MachineUid(3)).unwrap();
        assert_eq!(
            kit.patterns[0].roll.notes,
            vec![Note { accent: true, ..Note::new(16, 4, 1, ACID_VEL) }]
        );
    }
```

`ProjectV4::standard()` sets `version: 4`. Also update the existing `project.rs` tests `version_3_files_still_load` / `v3_round_trips` / `a_v2_project_upgrades_to_the_standard_six` to the new shapes: a v3/v4 file is now produced by serialising `ProjectV4::standard()` with `version: 3` or `4`; a v5 round trip uses `Project::default()`.

- [ ] **Step 5: Port the sequencer to per-tick acid and drum tracks.** In `sequencer.rs`:

Replace the `Track` fields `gate_open`/`prev_slide` block with:

```rust
    /// Acid: the gate is open (a note sounds).
    gate_open: bool,
    /// Acid: the sounding note slides into a note starting at its end.
    prev_slide: bool,
    /// Acid: ticks until the sounding note's gate-off check (0 = none).
    gate_left: u16,
    /// Acid: ticks from that check to the note's end, where a held gate
    /// must find the next note.
    hold_dist: u16,
    /// Acid: ticks until a held gate must find a note (0 = none).
    hold_left: u16,
    /// Acid: the sounding note's end tick (clipped to the pattern).
    cur_end: u16,
```

and update the module doc comment: every track now runs per tick; acid gate rule as in this plan's "acid gate rule" section. Reset `gate_left`, `hold_left` to 0 in `play()` and `stop()` beside `gate_open`.

Add helpers:

```rust
/// The note of `roll` that starts at tick `pos` (the first stored: acid
/// patterns are kept monophonic by the editor).
fn note_starting(roll: &NotePattern, pos: u16) -> Option<&Note> {
    roll.notes.iter().find(|n| n.start == pos && pos < roll.ticks())
}

/// Advances a step-free track one tick; at the pattern's end it wraps and
/// takes a queued pattern.
fn advance(t: &mut Track, ticks: u16) {
    t.step = (t.pos / TICKS_PER_STEP) as usize;
    t.pos += 1;
    if t.pos >= ticks {
        t.pos = 0;
        if let Some(q) = t.queued.take() {
            t.pattern = q;
        }
    }
}
```

Replace `synth_step` with `synth_tick`:

```rust
    /// One tick of the acid track at rack position `pos`: the gate-off
    /// check two ticks before a note's end (held when the note slides into
    /// one starting at its end), a held gate's end, a note start, and on a
    /// step's first tick the param lane.
    fn synth_tick(&mut self, pos: usize, bank: &[SynthPattern], out: &mut Vec<SeqEvent>) {
        let t = &mut self.tracks[pos];
        let Some(pat) = bank.get(t.pattern as usize) else {
            return;
        };
        let roll = &pat.roll;
        let ticks = roll.ticks();
        if t.pos >= ticks {
            t.pos = 0;
        }
        let p = t.pos;
        if t.silent {
            if t.gate_open {
                out.push(SeqEvent::NoteOff { slot: pos });
                t.gate_open = false;
            }
            t.prev_slide = false;
            t.gate_left = 0;
            t.hold_left = 0;
            if let Some(id) = t.lane_override.take() {
                out.push(SeqEvent::ParamRestore { id });
            }
            advance(t, ticks);
            return;
        }
        let mut check_hold = false;
        if t.gate_open {
            if t.gate_left > 0 {
                t.gate_left -= 1;
                if t.gate_left == 0 {
                    // Looks at this pattern's own note at the end (the 303
                    // way): a held gate then runs into whatever plays next.
                    if t.prev_slide && note_starting(roll, t.cur_end % ticks).is_some() {
                        t.hold_left = t.hold_dist;
                        check_hold = t.hold_left == 0;
                    } else {
                        out.push(SeqEvent::NoteOff { slot: pos });
                        t.gate_open = false;
                    }
                }
            } else if t.hold_left > 0 {
                t.hold_left -= 1;
                check_hold = t.hold_left == 0;
            }
        }
        if check_hold && note_starting(roll, p).is_none() {
            out.push(SeqEvent::NoteOff { slot: pos });
            t.gate_open = false;
        }
        if let Some(n) = note_starting(roll, p) {
            let legato = t.prev_slide && t.gate_open;
            out.push(SeqEvent::Note {
                slot: pos,
                midi: transposed(n.pitch, pat.transpose.saturating_add(t.transpose_add)),
                accent: n.accent,
                legato,
            });
            let end = n.start.saturating_add(n.len.max(1)).min(ticks);
            let span = end - n.start;
            let gate = span.saturating_sub(2).max(1);
            t.gate_open = true;
            t.prev_slide = n.slide;
            t.gate_left = gate;
            t.hold_dist = span - gate;
            t.hold_left = 0;
            t.cur_end = end;
        }
        if p % TICKS_PER_STEP == 0 {
            let step = (p / TICKS_PER_STEP) as usize;
            let lane_value = pat.param_lane.as_ref().and_then(|lane| {
                lane.values
                    .get(step)
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
        advance(t, ticks);
    }
```

Why this is bit-identical for converted patterns: a converted note starts on tick 0 of a step (old: `On` at quarter 0), `gate = 4n − 2` puts the check on quarter 2 of its last step (old: mid-step check), and `hold_dist = 2` ends a held gate on the next step's quarter 0 (old: the next step's `Off` closed it there). The silent branch fires on the tick the bar load sets `silent` (quarter 0), as before. The old lane emitted on quarter 0 of every step; so does this.

Add `drum_tick`:

```rust
    /// One tick of the drum track at rack position `pos`: every hit
    /// starting now, in instrument order (closed hat before open hat, so
    /// the choke works as before), each instrument at most once.
    fn drum_tick(&mut self, pos: usize, bank: &[DrumPattern], out: &mut Vec<SeqEvent>) {
        let t = &mut self.tracks[pos];
        let Some(pat) = bank.get(t.pattern as usize) else {
            return;
        };
        let roll = &pat.roll;
        let ticks = roll.ticks();
        if t.pos >= ticks {
            t.pos = 0;
        }
        if !t.silent {
            let (mut on, mut accent) = (0u8, 0u8);
            for n in roll.notes.iter().filter(|n| n.start == t.pos) {
                if (n.pitch as usize) < DRUM_INSTRUMENTS {
                    on |= 1 << n.pitch;
                    if n.accent {
                        accent |= 1 << n.pitch;
                    }
                }
            }
            for inst in 0..DRUM_INSTRUMENTS {
                if on & (1 << inst) != 0 {
                    out.push(SeqEvent::Drum {
                        slot: pos,
                        inst,
                        accent: accent & (1 << inst) != 0,
                    });
                }
            }
        }
        advance(t, ticks);
    }
```

Rewrite `tick`'s body (keep the song-mode quarter-0 prologue and the quarter-2 song counters exactly as they are; only the track work moves):

```rust
        if self.sub == 0 && self.mode == Mode::Song {
            if self.resync_pending {
                self.apply_resync(song, banks);
            }
            if self.bar_step == 0 && !self.load_bar(song, banks, out) {
                return;
            }
            // Before the pattern lanes, so a lane value wins on its step.
            self.emit_automation(song, out);
        }
        for pos in 0..MAX_MACHINES {
            if let Some(Bank::Synth(bank)) = banks.slots[pos] {
                self.synth_tick(pos, bank, out);
            }
        }
        for pos in 0..MAX_MACHINES {
            if let Some(Bank::Drum(bank)) = banks.slots[pos] {
                self.drum_tick(pos, bank, out);
            }
        }
        if self.sub == 2 && self.mode == Mode::Song {
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
        self.note_ticks(banks, out);
```

(then the existing `sub`/`global_step`/`samples_to_next` bookkeeping). Delete the old quarter-2 "step advance" loop: every track now advances itself. In `join`, drop the step-track `else` branch: every track sets `t.pos` and `t.step` the way the `Bank::Notes` branch does (remove the `if matches!(… Bank::Notes …)` condition).

Imports: `use crate::params::DRUM_INSTRUMENTS;`, `use crate::project::{DrumPattern, Note, NotePattern, PatternRef, SynthPattern, TICKS_PER_STEP};` (drop `DrumHit`, `Gate`, `steps_of` if unused).

- [ ] **Step 6: Port the sequencer tests.** In the `tests` module, replace step-pattern setup with notes. Add these helpers and rewrite each test's setup with them, keeping every assertion on event timing unchanged (the timings must still hold — that is the point):

```rust
    use crate::steps::{ACID_VEL, DrumGrid, DrumHit, Gate, SynthGrid, SynthStep};

    /// Sets synth pattern steps the old way, through the grid view.
    fn set_steps(p: &mut SynthPattern, length: u8, steps: &[(usize, SynthStep)]) {
        let mut g = SynthGrid::from_roll(&p.roll);
        g.length = length;
        for &(i, s) in steps {
            g.steps[i] = s;
        }
        p.roll = g.to_roll();
    }

    fn set_hits(p: &mut DrumPattern, length: u8, hits: &[(usize, usize, DrumHit)]) {
        let mut g = DrumGrid::from_roll(&p.roll);
        g.length = length;
        for &(step, inst, h) in hits {
            g.steps[step][inst] = h;
        }
        p.roll = g.to_roll();
    }
```

e.g. `f.synths[0][0].steps[0] = on(0);` becomes `set_steps(&mut f.synths[0][0], 16, &[(0, on(0))]);` and `f.drums[0][0].steps[st][0] = DrumHit::On;` becomes `set_hits(&mut f.drums[0][0], 16, &[(st, 0, DrumHit::On)]);`; `f.synths[0][0].length = 3;` becomes `f.synths[0][0].roll.length = 3;`.

Add two new tests for behaviour the step model could not express:

```rust
    #[test]
    fn an_off_grid_acid_note_plays_at_its_tick_with_a_short_gate() {
        let mut f = Fixture::new();
        f.synths[0][0].roll.notes = vec![Note::new(5, 1, 48, ACID_VEL)];
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 4 * 5512);
        let quarter = 5512.5 / 4.0;
        let on = ev.iter().find(|(_, e)| matches!(e, SeqEvent::Note { .. })).unwrap();
        let off = ev.iter().find(|(_, e)| matches!(e, SeqEvent::NoteOff { .. })).unwrap();
        assert_eq!(on.0, (5.0 * quarter).ceil() as usize);
        assert_eq!(off.0, (6.0 * quarter).ceil() as usize, "one tick later");
    }

    #[test]
    fn a_drum_hit_between_sixteenths_plays_on_its_tick() {
        let mut f = Fixture::new();
        f.drums[0][0].roll.notes = vec![Note::new(2, 4, 0, ACID_VEL)];
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 5512);
        let hit = ev.iter().find(|(_, e)| matches!(e, SeqEvent::Drum { .. })).unwrap();
        assert_eq!(hit.0, (2.0 * 5512.5 / 4.0_f64).ceil() as usize);
    }
```

(If the fixture's sample-position rounding differs, compute the expected positions with the same rounding the existing timing tests use; copy their idiom.)

- [ ] **Step 7: Port the rest of the engine crate.**
  - `song.rs`: `Bank::length` reads `b.get(s)?.roll.length` for `Synth` and `Drum`; tests build patterns with `SynthPattern { roll: NotePattern { length: 32, ..NotePattern::default() }, ..SynthPattern::default() }`.
  - `random.rs`: keep every algorithm; run it on the grid view. `randomise_synth` starts `let mut grid = SynthGrid::from_roll(&p.roll); let groove = DrumGrid::from_roll(&groove.roll); let len = steps_of(p.roll.length);`, uses `grid.steps` wherever it used `out.steps`, and ends `let mut out = p.clone(); out.roll = grid.to_roll(); out`. `groove_hits`, `open_mutes_closed` and `random_hit` take `&DrumGrid` / `&mut DrumGrid`. `drum_template` builds a `DrumGrid { steps: vec![[DrumHit::Off; DRUM_INSTRUMENTS]; STEPS], length: len as u8 }` and returns `DrumPattern { roll: grid.to_roll() }`. `evolve_drums` converts in and out the same way. Update the module doc: "Synth and drum dice work on the 1/16 grid: a roll snaps off-grid notes to it."
  - `demo.rs`: build the two acid lines and both kits as grids, then `pattern.roll = grid.to_roll()`. Its test checks `roll.notes.iter().any(|n| n.slide)` and `…any(|n| n.accent)`.
  - `host.rs` test `replaced_pattern_comes_back_for_disposal`: assert `!old.roll.notes.is_empty()`.
  - `engine.rs` tests: lines ~968–990 set up audible patterns: `pat.roll.notes.push(Note::new(0, 4, 36, ACID_VEL))` instead of `pat.steps[0].gate = Gate::On`; the kit loop becomes `k.patterns[0].roll = DrumGrid { steps: vec![[DrumHit::Accent; DRUM_INSTRUMENTS]; STEPS], length: 16 }.to_roll()`. Malformed-pattern tests (~1865–1985): a "short" pattern is now `p.roll.length = 0`; a malformed drum pattern is a note with `pitch: 8`; a malformed synth lane stays `lane.values.pop()`.
  - `tests/no_alloc.rs`: build its patterns with notes (`Note::new`), making sure the drum patterns still hit every instrument and the synth pattern still slides (`slide: true` on a note followed by an adjacent note) so every code path runs.

- [ ] **Step 8: Run the engine crate**

Run: `cargo test -p acid-engine`
Expected: all pass, **including `golden.rs` with the hashes unchanged**. If a golden hash changes, do not update it: diff the event streams of the old and new sequencer (check out `master` into a worktree, log `(sample, SeqEvent)` for `render_patterns` of the fixture on both) and fix the sequencer until they match.

- [ ] **Step 9: Commit** (the app crate does not build yet; say so)

```bash
cargo fmt --all
git add -A
git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit -m "feat(engine): acid and drum patterns are notes; v4 projects convert

The sequencer runs acid and drum tracks per tick, so notes can sit on
any 1/64. Converted projects render bit-identically (golden tests).
The app crate is ported in the next commit.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The app on the new model; the 303 view removed

**Files:**
- Delete: `crates/app/src/ui/step303.rs`, `crates/app/src/ui/step_edit.rs`, `crates/app/src/ui/pianoroll.rs`, `crates/app/src/ui/roll_edit.rs`
- Modify: `crates/app/src/ui/mod.rs` (module list, `PanelView` fields, tests)
- Modify: `crates/app/src/document.rs` (edits, merge keys, no-op checks, apply, commands, tests)
- Modify: `crates/app/src/ui/synth_panel.rs`, `drum_panel.rs`, `polyroll.rs`, `poly_edit.rs`, `files.rs`, `keys.rs`, `scroll.rs`, `knob.rs`, `sampler_ui.rs`, `pads.rs`

**Interfaces:**
- Consumes: `Project::roll`, `Project::roll_mut`, `SynthPattern.roll`, `DrumPattern.roll`, `steps::{drum_hit_at, set_drum_hit, DrumHit}`.
- Produces:
  - `Edit::NotePattern` and `Edit::NoteLength` apply to **any** machine's pattern via `roll_mut`.
  - Removed edits: `SynthStep`, `SynthLength`, `DrumHit`, `DrumLength`, `View` (and their `MergeKey`s).
  - `fn pattern_command(p: &Project, uid, slot) -> Command` in `document.rs`: the `Command::Pattern` whose `PatternData` matches the machine's type.
  - `pub(crate) const NOTE_NAMES` moves to `polyroll.rs`; `pub fn scroll_notches` moves to `scroll.rs`.
  - `keys::shortcut(key, modifiers)` loses its `step_writing` argument.
  - `poly_roll` draws any machine's roll, including the acid synth's.

- [ ] **Step 1: Delete the 303 editor.** Remove the four files and their `mod` lines. Move `NOTE_NAMES` to `polyroll.rs` (`pub(crate) const NOTE_NAMES: [&str; 12] = […];`) and `scroll_notches` plus its `SCROLL_NOTCH` constant to `scroll.rs`; fix the imports in `knob.rs` and `sampler_ui.rs`. From `PanelView` remove `writer`, `scroll_accum`, `scroll_at`, `roll_drag`, `roll_origin`; in `scroll.rs::tick_scroll_gestures` drop the per-panel loop (only `knob_scroll` remains) and update the module comment to "Mouse-wheel gestures on knobs". In `keys.rs` remove `step_writing` (Space is always Play/Stop unless typing). Delete the writer resets in `pads.rs`, `files.rs`, `synth_panel.rs` and the tests that exercised the writer or the view toggle (`step_writer_is_dropped_when_its_synth_leaves_step_view`, `selecting_another_pattern_stops_that_synths_step_writer`, the `scroll.rs` 303 tests).

- [ ] **Step 2: Write the failing document test** (in `document.rs` tests)

```rust
    #[test]
    fn note_edits_reach_acid_and_drum_patterns() {
        use acid_engine::project::{Note, NotePattern};
        let mut d = Document::new(Project::default());
        // A drum pattern's pitch is its instrument (0..8).
        for (uid, pitch) in [(SYNTH1, 40), (KIT808, 3)] {
            let roll = NotePattern {
                notes: vec![Note::new(3, 5, pitch, 100)],
                length: 12,
            };
            let cmds = d.apply(
                Edit::NotePattern { uid, slot: 2, pattern: Box::new(roll.clone()) },
                false,
            );
            assert_eq!(d.project.roll(uid, 2), Some(&roll));
            assert!(matches!(&cmds[..], [Command::Pattern { .. }]));
            d.apply(Edit::NoteLength { uid, slot: 2, length: 20 }, false);
            assert_eq!(d.project.roll(uid, 2).unwrap().length, 20);
        }
        d.undo();
        assert_eq!(d.project.roll(KIT808, 2).unwrap().length, 12);
    }
```

(use the test module's existing uid constants and `Document` constructor; if the 808's constant has another name there, use that.)

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test -p app note_edits_reach_acid_and_drum_patterns`
Expected: compile errors until the steps below are done (the crate does not build yet).

- [ ] **Step 4: Port `document.rs`.**
  - Delete the `SynthStep`, `SynthLength`, `DrumHit`, `DrumLength` and `View` edit variants, their `MergeKey`s, no-op checks and `apply_raw` arms.
  - `NotePattern` apply: `let pat = p.roll_mut(uid, slot).expect(NO_NOTES); let old = std::mem::replace(pat, pattern);` (clamp length and `debug_assert!` as now). `NoteLength` likewise through `roll_mut`; its no-op check uses `p.roll(*uid, *slot).is_some_and(|r| r.length == *length)`.
  - Add:

```rust
/// The command that hands the engine pattern `slot` of `uid` as it is now.
fn pattern_command(p: &Project, uid: MachineUid, slot: u8) -> Command {
    let s = slot as usize;
    let data = match &p.machine(uid).expect(NO_MACHINE).body {
        MachineBody::Synth(m) => PatternData::Synth(m.patterns[s].clone()),
        MachineBody::Kit(m) => PatternData::Drum(m.patterns[s].clone()),
        MachineBody::Fm(m) => PatternData::Notes(m.patterns[s].clone()),
        MachineBody::Mono(m) => PatternData::Notes(m.patterns[s].clone()),
        MachineBody::Sampler(m) => PatternData::Notes(m.patterns[s].clone()),
    };
    Command::Pattern {
        uid,
        index: slot,
        pattern: Box::new(data),
    }
}
```

  and in `commands_for` map `SynthTranspose`, `SynthPattern`, `DrumPattern`, `NotePattern`, `NoteLength` all to `vec![pattern_command(p, *uid, *slot)]`.
  - Port the document tests that used `SynthStep`/`DrumHit` edits to `NotePattern` edits on the acid/drum rolls, keeping what each test checks (undo, merge, record-pass dirty tracking).

- [ ] **Step 5: Port the panels.**
  - `synth_panel.rs`: Len uses `pattern.roll.length` and `set_synth_length` applies `Edit::NoteLength`; remove the 303/Piano roll selector and the step-write label; the panel body ends with `app.poly_roll(ui, uid);`. The groove lookup is unchanged (`DrumPattern` still).
  - `drum_panel.rs`: the grid reads `let hit = drum_hit_at(&pattern.roll, step, inst);` and `len = pattern.roll.length as usize`. `click_drum_cell` becomes:

```rust
    pub(crate) fn click_drum_cell(
        &mut self,
        uid: MachineUid,
        slot: u8,
        step: usize,
        inst: usize,
        hit: Option<DrumHit>,
    ) {
        let Some(mut roll) = self.drum_pattern(uid, slot).map(|p| p.roll.clone()) else {
            return;
        };
        let hit = hit.unwrap_or_else(|| cycle_hit(drum_hit_at(&roll, step, inst)));
        if !set_drum_hit(&mut roll, step, inst, hit) {
            return;
        }
        let cmds = self.doc.apply(
            Edit::NotePattern {
                uid,
                slot,
                pattern: Box::new(roll),
            },
            false,
        );
        self.send(cmds);
        self.view.focus = Some(uid);
    }
```

    (keep whatever the current function does after `send`). The Len field applies `Edit::NoteLength`. Its tests read cells with `drum_hit_at(&…patterns[slot].roll, 3, 2)` and lengths with `.roll.length`.
  - `polyroll.rs` / `poly_edit.rs`: read the pattern with `self.doc.project.roll(uid, slot)` instead of `note_patterns(uid)`; everything else is unchanged in this task (the acid behaviour comes in Task 5). The FM/sampler/Mono start pitches are unchanged; `roll_start_pitch` returns `BASE_MIDI as u8` (C2) for a synth.
  - `files.rs`: the clip types are unchanged; fix field paths only.
  - `ui/mod.rs` tests: replace step checks such as `.steps.iter().all(|s| s.gate == Gate::Off)` with `.roll.notes.is_empty()`, and `doc.project.synth_mut(SYNTH1).unwrap().view = …` lines are deleted.

- [ ] **Step 6: Build and test the workspace**

Run: `cargo test --workspace`
Expected: PASS (engine and app). Then `cargo clippy --workspace --all-targets -- -D warnings`: no warnings.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add -A
git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit -m "feat(app): acid and drum editing on notes; the 303 step view is gone

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: The acid synth in the piano roll: one note at a time, accent and slide lanes

**Files:**
- Modify: `crates/app/src/ui/poly_edit.rs` (new pure functions and tests)
- Modify: `crates/app/src/ui/polyroll.rs` (acid mode)

**Interfaces:**
- Consumes: `poly_edit::{add, move_to, resize, resolve_overlaps, note_at}`, `Note.accent`, `Note.slide`.
- Produces:
  - `pub fn make_mono(p: &mut NotePattern, keep: usize) -> usize` — the kept note stays; other notes starting inside it are deleted, a note sounding into its start is cut there. Returns the kept note's new index.
  - `pub fn note_covering(p: &NotePattern, tick: u16) -> Option<usize>`
  - `pub enum Flag { Accent, Slide }` and `pub fn toggle_flag(p: &mut NotePattern, tick: u16, flag: Flag) -> bool`

- [ ] **Step 1: Write the failing tests** (in `poly_edit.rs` tests)

```rust
    fn pat(notes: &[(u16, u16, u8)]) -> NotePattern {
        NotePattern {
            notes: notes.iter().map(|&(s, l, p)| Note::new(s, l, p, 100)).collect(),
            length: 16,
        }
    }

    #[test]
    fn make_mono_keeps_the_edited_note_and_cuts_around_it() {
        // 0..8 sounds into the kept note at 4; 6 and 8 start inside it;
        // 20 is clear of it.
        let mut p = pat(&[(0, 8, 40), (4, 8, 50), (6, 2, 41), (8, 4, 42), (20, 4, 43)]);
        let kept = make_mono(&mut p, 1);
        assert_eq!(p.notes[kept], Note::new(4, 8, 50, 100));
        let mut starts: Vec<_> = p.notes.iter().map(|n| (n.start, n.len)).collect();
        starts.sort();
        assert_eq!(starts, vec![(0, 4), (4, 8), (20, 4)]);
    }

    #[test]
    fn make_mono_drops_a_note_with_the_same_start() {
        let mut p = pat(&[(4, 4, 40), (4, 2, 52)]);
        let kept = make_mono(&mut p, 1);
        assert_eq!(p.notes.len(), 1);
        assert_eq!(p.notes[kept].pitch, 52);
    }

    #[test]
    fn flags_toggle_on_the_note_under_the_tick() {
        let mut p = pat(&[(4, 8, 40)]);
        assert!(toggle_flag(&mut p, 9, Flag::Accent));
        assert!(p.notes[0].accent);
        assert!(toggle_flag(&mut p, 4, Flag::Slide));
        assert!(p.notes[0].slide);
        assert!(toggle_flag(&mut p, 11, Flag::Accent));
        assert!(!p.notes[0].accent);
        assert!(!toggle_flag(&mut p, 12, Flag::Slide), "no note at 12");
        assert_eq!(note_covering(&p, 3), None);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p app make_mono`
Expected: compile error (`make_mono` not found).

- [ ] **Step 3: Implement** (in `poly_edit.rs`)

```rust
/// Keeps an acid pattern monophonic around note `keep` after an edit:
/// other notes starting inside it are deleted, a note sounding into its
/// start is cut there. Returns `keep`'s new index.
pub fn make_mono(p: &mut NotePattern, keep: usize) -> usize {
    let Some(&k) = p.notes.get(keep) else {
        return keep;
    };
    let k_end = end(&k);
    let mut kept = Vec::with_capacity(p.notes.len());
    let mut at = 0;
    for (i, n) in p.notes.iter().enumerate() {
        if i == keep {
            at = kept.len();
            kept.push(*n);
        } else if n.start >= k.start && n.start < k_end {
            continue;
        } else if n.start < k.start && end(n) > k.start {
            kept.push(Note {
                len: k.start - n.start,
                ..*n
            });
        } else {
            kept.push(*n);
        }
    }
    p.notes = kept;
    at
}

/// The note sounding at `tick` (the first stored).
pub fn note_covering(p: &NotePattern, tick: u16) -> Option<usize> {
    p.notes.iter().position(|n| n.start <= tick && tick < end(n))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    Accent,
    Slide,
}

/// Flips accent or slide on the note sounding at `tick`.
pub fn toggle_flag(p: &mut NotePattern, tick: u16, flag: Flag) -> bool {
    let Some(i) = note_covering(p, tick) else {
        return false;
    };
    let n = &mut p.notes[i];
    match flag {
        Flag::Accent => n.accent = !n.accent,
        Flag::Slide => n.slide = !n.slide,
    }
    true
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p app poly_edit`
Expected: PASS.

- [ ] **Step 5: Wire acid mode into `polyroll.rs`.** At the top of `poly_roll` compute `let acid = self.doc.project.synth(uid).is_some();`. Then:
  - After a click-add, `let i = make_mono(&mut pattern, i)` when `acid` (use the index `add` returned).
  - On drag stop, call `make_mono(&mut pattern, index)` instead of `resolve_overlaps` when `acid`.
  - When `acid`, draw two 12 pt lanes labelled "Acc" and "Slide" in place of the velocity lane. Each note gets a filled cell under its first tick (accent: `theme::ACCENT`; slide: `theme::ACID`, drawn under its last tick). A click in a lane at tick `t` calls `toggle_flag(&mut pattern, t, Flag::Accent | Flag::Slide)`, `changed |= …; finished = true;`.
  - When `acid`, draw a short line from the right end of each sliding note's rectangle to the left end of a note that starts at its end (the slide's tie line).
  - Update the module doc comment: the roll also serves the acid synths (one note at a time, accent and slide lanes instead of velocity).
  - The key strip's audition is skipped for the acid synth (the engine has no audition for it).

- [ ] **Step 6: Add a headless draw test** (in `ui/mod.rs`, next to `note_panels_and_poly_rolls_draw_headless`): draw the default app with Synth 1 holding a sliding note followed by an adjacent note, for one frame, and assert it does not panic and `app.view.panel(SYNTH1).poly_scrolled` is true. Copy the setup idiom of the existing headless test.

- [ ] **Step 7: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all
git add -A
git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit -m "feat(app): the acid synths edit in the piano roll at 1/64 with accent and slide lanes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Docs, spec fix-up and a check in the running app

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-10-16-dock-song-lanes-design.md` (keyboard-entry note-key reference)

- [ ] **Step 1: README.** In "What's in the box":
  - Replace the "**Two sequencer views per synth,** …" bullet group with: "**Piano roll for the bass synths:** 1/16 to 1/64 snap, one note at a time, accent and slide lanes, and slides drawn as ties."
  - The drum machines' bullets gain: "Hits are stored at 1/64 resolution; the grid edits them on 1/16 steps."
  - Remove "tracker-style step-write". Keep product names out.

- [ ] **Step 2: Spec.** In section 2 "Keyboard entry", replace "The existing note keys (`keys.rs`)" with "The tracker key layout of the removed step-writer (`crates/app/src/ui/step_edit.rs` at commit `32308c6`: Z S X D C V G B H N J M for the lower octave, Q 2 W 3 E R 5 T 6 Y 7 U I above)".

- [ ] **Step 3: Check the real app.** Build and run `cargo run --release -p app`, open a v4 project saved before this branch (e.g. check out `master`'s demo by saving one from a `master` build first, or use `crates/acid-engine/tests/fixtures/golden_v2.sidacid`), play it in Pattern and Song mode and listen for differences. Take a screenshot (see the `sidacid-screenshots` memory: `python3 -c "from PIL import ImageGrab; ImageGrab.grab().save(path)"`) showing a synth panel with the piano roll and accent/slide lanes. Draw a note at 1/64 snap and confirm it plays off the grid.

- [ ] **Step 4: Final verification**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all clean and passing.

- [ ] **Step 5: Commit**

```bash
git add -A
git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit -m "docs: the bass synths' piano roll replaces the step view

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Then offer the merge into `master` and push, as for every finished SidAcid branch (ask first).
