# SidAcid Plan 4 — Groundwork for FM + Sampler Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Grow SidAcid from 4 to 6 machines (adding FM and Sampler slots that play note patterns through a polyphonic piano roll), with placeholder sine voices so the whole path is audible and testable before the real FM and sampler engines arrive in Plans 5 and 6.

**Architecture:** A `MachineId` enum and `MACHINES = 6` constant index every per-machine array. Machines 0–3 keep their existing typed engines and pattern formats. Machines 4 (FM) and 5 (Sampler) use a new `NotePattern`, which is a list of `Note { start, len, pitch, vel }` events at 4 ticks per 1/16 step. They are played by a new note-track path in the sequencer, which runs on a quarter-step clock. Project files move to v2, and v1 files load through a migration struct.

**Tech Stack:** Rust 2024 (1.88+), egui/eframe, cpal, rtrb, serde + RON. No new dependencies in this plan.

**Spec:** `docs/superpowers/specs/2026-10-12-fm-sampler-design.md` (sections 1, 4 and 5 — "Plan 4"). Read it before starting.

## Global Constraints

- `cargo test --workspace` passes at the end of every task. The baseline is 270 tests. Never delete or weaken an existing assertion unless a step says so explicitly; when a step does, it gives the replacement.
- `cargo clippy --workspace --all-targets` introduces no new warnings; run `cargo fmt --all` before each commit.
- **Real-time rule:** `Engine::process`, everything it calls, and `EngineHost::process` must not allocate, lock or do I/O. Patterns are swapped in with `std::mem::swap`. Replaced boxes go back to the UI thread as `Event::Dispose*`.
- Machine order is fixed: 0 Synth 1, 1 Synth 2, 2 808, 3 909, 4 FM, 5 Sampler. Mixer channel `c` is machine `c`.
- `Bar.transpose` has 3 entries: Synth 1, Synth 2, FM (`MachineId::transpose_index`). The sampler is never transposed.
- `PROJECT_VERSION = 2`. v1 files must load with no data loss; files are always saved as v2.
- Every commit message ends with the trailer line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Deliberate deviations from the spec (simplifications; keep them)

1. **`NotePattern.notes` is not kept sorted.** The sequencer scans every note each tick, so order does not matter. Unsorted indices stay valid during a drag.
2. **A note's `start` is bounded by the 64-step maximum** (`STEPS * TICKS_PER_STEP`), not by the pattern's current length. Shortening a pattern therefore never makes it malformed; notes past the end are simply not played.
3. **The engine does not pre-reserve `MAX_NOTES` capacity.** It never pushes into a note pattern; it only swaps whole patterns.
4. **The roll's snap options are 1/16, 1/32 and 1/64.** The spec's "Off" was identical to 1/64, so it is dropped.
5. **Placeholder params:** `FmParam` and `SamplerParam` each have only `Volume` in this plan. Plans 5 and 6 append the real params.
6. **Sampler row labels:** the sampler roll shows plain note names here. Slot and slice labels arrive with slots in Plan 6.

## File structure

| File | Status | Responsibility |
|---|---|---|
| `crates/acid-engine/src/machine.rs` | new | `MachineId`, `MachineKind`, `MACHINES`, `TRANSPOSED`, `NOTE_MACHINES` |
| `crates/acid-engine/src/migrate.rs` | new | v1 project shapes + `upgrade()` to v2 |
| `crates/acid-engine/src/placeholder.rs` | new | `SineVoices`: 8-voice sine stand-in for FM / Sampler |
| `crates/acid-engine/tests/no_alloc.rs` | new | counting-allocator test of `Engine::process` |
| `crates/acid-engine/src/params.rs` | modify | 6 channels, `FmParam`, `SamplerParam` |
| `crates/acid-engine/src/mixer.rs` | modify | 6 strips, 7 meters |
| `crates/acid-engine/src/project.rs` | modify | `Note`, `NotePattern`, `FmMachine`, `SamplerMachine`, v2 `Bar`/`Project` |
| `crates/acid-engine/src/song.rs` | modify | `Banks` (replaces `SynthBanks`/`DrumBanks`/`banks()`) |
| `crates/acid-engine/src/sequencer.rs` | modify | 6 tracks, quarter-step clock, note tracks |
| `crates/acid-engine/src/engine.rs` | modify | note pattern storage, placeholder voices, audition |
| `crates/acid-engine/src/host.rs` | modify | 6-wide `Status`, `NotePattern`/`NoteOn`/`NoteOff` commands |
| `crates/acid-engine/src/render.rs`, `lib.rs` | modify | `Banks`, module list |
| `crates/app/src/document.rs` | modify | `Edit::NotePattern`, `Edit::NoteLength` |
| `crates/app/src/ui/poly_edit.rs` | new | pure poly-roll editing + geometry |
| `crates/app/src/ui/polyroll.rs` | new | poly roll drawing/input/audition |
| `crates/app/src/ui/note_panel.rs` | new | FM and Sampler panels |
| `crates/app/src/ui/{mod,files,mixer_panel,params_ui,song_panel}.rs` | modify | 6 machines in the UI |
| `README.md` | modify | feature list |

---

### Task 1: Six mixer channels and placeholder machine params

**Files:**
- Modify: `crates/acid-engine/src/params.rs`
- Modify: `crates/acid-engine/src/mixer.rs`
- Modify: `crates/acid-engine/src/engine.rs` (`chans`, `take_meters`, tests)
- Modify: `crates/acid-engine/src/host.rs` (`Status.meters`, `EngineHost.meters`, test)
- Modify: `crates/app/src/ui/mixer_panel.rs`, `crates/app/src/ui/params_ui.rs`

**Interfaces:**
- Produces:
  - `params::CHANNELS == 6`, `params::PARAM_COUNT == 215`
  - `FmParam::Volume` and `SamplerParam::Volume`, both defaulting to 0.7
  - `ParamId::Fm(FmParam)` and `ParamId::Sampler(SamplerParam)`, placed last in `ParamId::all()`
  - `Mixer::take_meters() -> [f32; CHANNELS + 1]`, `Engine::take_meters() -> [f32; CHANNELS + 1]`
  - `Status.meters: [f32; CHANNELS + 1]` (master is index `CHANNELS`)

- [ ] **Step 1: Write the failing tests** in `params.rs` tests

Replace the body of `registry_has_every_param_once` and update `index_validity`:

```rust
    #[test]
    fn registry_has_every_param_once() {
        let all = ParamId::all();
        assert_eq!(
            all.len(),
            2 * 15 + 2 * 8 * 4 + 2 * 2 + 6 * 16 + 11 + 8 + 1 + 1
        );
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
    }

    #[test]
    fn fm_and_sampler_params_come_last() {
        let all = ParamId::all();
        assert_eq!(all[all.len() - 2], ParamId::Fm(FmParam::Volume));
        assert_eq!(all[all.len() - 1], ParamId::Sampler(SamplerParam::Volume));
        assert_eq!(ParamId::Fm(FmParam::Volume).info().default, 0.7);
        assert!(ParamId::Channel(5, ChannelParam::Level).is_valid());
    }
```

In `index_validity` change `ParamId::Channel(4, ChannelParam::Level)` to `ParamId::Channel(6, ChannelParam::Level)`.

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine params::`
Expected: compile error (`FmParam` not found).

- [ ] **Step 3: Implement params**

In `params.rs`:

```rust
pub const CHANNELS: usize = 6;
/// Number of entries in `ParamId::all()`.
pub const PARAM_COUNT: usize = 215;
```

Next to the other `param_enum!` calls:

```rust
param_enum!(FmParam { Volume => "Volume" });
param_enum!(SamplerParam { Volume => "Volume" });
```

Add the variants at the end of `ParamId`:

```rust
    Master(MasterParam),
    Fm(FmParam),
    Sampler(SamplerParam),
```

In `info()` add arms:

```rust
            ParamId::Fm(p) => cont(p.name(), 0.7),
            ParamId::Sampler(p) => cont(p.name(), 0.7),
```

In `is_valid()`, change the last arm to `ParamId::Fm(_) | ParamId::Sampler(_) | ParamId::Fx(_) | ParamId::Master(_) => true,`.

In `index()`, after `master_base`:

```rust
        let fm_base = master_base + MasterParam::ALL.len();
        let sampler_base = fm_base + FmParam::ALL.len();
```

Add these match arms:

```rust
            ParamId::Fm(p) => fm_base + p as usize,
            ParamId::Sampler(p) => sampler_base + p as usize,
```

In `all()`, after the master line:

```rust
        v.extend(FmParam::ALL.iter().map(|&p| ParamId::Fm(p)));
        v.extend(SamplerParam::ALL.iter().map(|&p| ParamId::Sampler(p)));
```

- [ ] **Step 4: Widen the mixer, engine and host to `CHANNELS`**

`mixer.rs`:
- the doc comment says "Six channel strips"
- `meters: [f32; CHANNELS + 1]` and `meters: [0.0; CHANNELS + 1]`
- `pub fn take_meters(&mut self) -> [f32; CHANNELS + 1]`, with the doc comment changed to "channels 0..CHANNELS post-fader, then the master"
- `process(&mut self, chans: &mut [StereoBuf; CHANNELS], ...)`
- the master meter line `self.meters[4]` becomes `self.meters[CHANNELS]`
- in the mixer tests, every `[StereoBuf; 4]` becomes `[StereoBuf; CHANNELS]`

`engine.rs`:
- `chans: [StereoBuf; CHANNELS]`
- `pub fn take_meters(&mut self) -> [f32; CHANNELS + 1]`
- import `CHANNELS` from `crate::params`
- **In the tests:**
  - in `out_of_range_indices_are_ignored`, `ParamId::Channel(4, ...)` becomes `ParamId::Channel(6, ...)`
  - in `meters_report_and_reset`, the master index `4` becomes `CHANNELS`:

```rust
        let m = e.take_meters();
        assert!(m[0] > 0.01 && m[2] > 0.01 && m[CHANNELS] > 0.01, "{m:?}");
        assert!(m[CHANNELS] <= 1.0 + 1e-6, "master is post-clip");
```

`host.rs`:
- `pub meters: [f32; CHANNELS + 1]`, with the doc comment "channels 0..CHANNELS, then master"
- `EngineHost.meters: [f32; CHANNELS + 1]`, initialised to `[0.0; CHANNELS + 1]`
- in the host test, `last.meters[4]` becomes `last.meters[CHANNELS]`
- import `crate::params::CHANNELS`

- [ ] **Step 5: Update the app**

`mixer_panel.rs`:
- `pub const CHANNEL_NAMES: [&str; CHANNELS] = ["Synth 1", "Synth 2", "808", "909", "FM", "Sampler"];`
- `for ch in 0..CHANNELS as u8`
- `self.status.meters[CHANNELS]` for the master meter
- import `acid_engine::params::CHANNELS`

`params_ui.rs` `param_label`, add arms:

```rust
        ParamId::Fm(p) => format!("FM {}", p.name()),
        ParamId::Sampler(p) => format!("Sampler {}", p.name()),
```

In `params_ui.rs` tests, add to `param_labels_name_the_machine`:

```rust
        assert_eq!(param_label(ParamId::Fm(acid_engine::params::FmParam::Volume)), "FM Volume");
        assert_eq!(
            param_label(ParamId::Channel(5, acid_engine::params::ChannelParam::Level)),
            "Sampler Level"
        );
```

- [ ] **Step 6: Run the whole suite**

Run: `cargo test --workspace`
Expected: all pass (270 + 1 new engine test, plus the extended assertions).

- [ ] **Step 7: Commit**

```bash
git add -A crates
git commit -m "feat(engine): six mixer channels and FM/Sampler volume params

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: MachineId, NotePattern and project format v2

**Files:**
- Create: `crates/acid-engine/src/machine.rs`, `crates/acid-engine/src/migrate.rs`
- Modify: `crates/acid-engine/src/lib.rs`, `crates/acid-engine/src/project.rs`
- Modify (compile fixes): `crates/acid-engine/src/engine.rs`, `render.rs`, `song.rs` tests, `sequencer.rs` tests, `crates/app/src/ui/mod.rs`, `crates/app/src/ui/files.rs`

**Interfaces:**
- Produces (in `acid_engine::machine`):

```rust
pub const MACHINES: usize = 6;
pub const TRANSPOSED: usize = 3;
pub const NOTE_MACHINES: usize = 2;
pub enum MachineId { Synth1, Synth2, Kit808, Kit909, Fm, Sampler }
pub enum MachineKind { Synth(usize), Drum(usize), Notes(usize) }
impl MachineId {
    pub const ALL: [MachineId; MACHINES];
    pub fn index(self) -> usize;
    pub fn from_index(i: usize) -> Option<MachineId>;
    pub fn name(self) -> &'static str;          // "Synth 1" "Synth 2" "808" "909" "FM" "Sampler"
    pub fn kind(self) -> MachineKind;
    pub fn transpose_index(self) -> Option<usize>; // Synth1 0, Synth2 1, Fm 2
}
pub fn kind_of(machine: usize) -> Option<MachineKind>;
```

- Produces (in `acid_engine::project`):

```rust
pub const PROJECT_VERSION: u32 = 2;
pub const TICKS_PER_STEP: u16 = 4;
pub const MAX_NOTES: usize = 256;
pub struct Note { pub start: u16, pub len: u16, pub pitch: u8, pub vel: u8 }
pub struct NotePattern { pub notes: Vec<Note>, pub length: u8 }   // Default: no notes, length 16
impl NotePattern { pub fn ticks(&self) -> u16; pub fn is_well_formed(&self) -> bool; }
pub struct FmMachine { pub patterns: Vec<NotePattern> }        // 128 patterns
pub struct SamplerMachine { pub patterns: Vec<NotePattern> }   // 128 patterns
// Bar: patterns [Option<PatternRef>; MACHINES], mutes [bool; MACHINES], transpose [i8; TRANSPOSED]
// Project: + fm: FmMachine, + sampler: SamplerMachine, selected: [u8; MACHINES]
impl Project {
    pub fn note_patterns(&self, machine: usize) -> Option<&Vec<NotePattern>>;
    pub fn note_patterns_mut(&mut self, machine: usize) -> Option<&mut Vec<NotePattern>>;
}
```

- [ ] **Step 1: Create `machine.rs`**

```rust
//! The six machines in the rack. Every per-machine array (selected
//! pattern, playhead, song bar slots, mixer channel) is indexed by
//! `MachineId::index`.

pub const MACHINES: usize = 6;
/// Machines with a song transpose: Synth 1, Synth 2, FM.
pub const TRANSPOSED: usize = 3;
/// Machines sequenced from note patterns: FM, Sampler.
pub const NOTE_MACHINES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MachineId {
    Synth1,
    Synth2,
    Kit808,
    Kit909,
    Fm,
    Sampler,
}

/// What a machine plays, with its index among machines of that kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineKind {
    Synth(usize),
    Drum(usize),
    Notes(usize),
}

impl MachineId {
    pub const ALL: [MachineId; MACHINES] = [
        MachineId::Synth1,
        MachineId::Synth2,
        MachineId::Kit808,
        MachineId::Kit909,
        MachineId::Fm,
        MachineId::Sampler,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<MachineId> {
        Self::ALL.get(i).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            MachineId::Synth1 => "Synth 1",
            MachineId::Synth2 => "Synth 2",
            MachineId::Kit808 => "808",
            MachineId::Kit909 => "909",
            MachineId::Fm => "FM",
            MachineId::Sampler => "Sampler",
        }
    }

    pub fn kind(self) -> MachineKind {
        match self {
            MachineId::Synth1 => MachineKind::Synth(0),
            MachineId::Synth2 => MachineKind::Synth(1),
            MachineId::Kit808 => MachineKind::Drum(0),
            MachineId::Kit909 => MachineKind::Drum(1),
            MachineId::Fm => MachineKind::Notes(0),
            MachineId::Sampler => MachineKind::Notes(1),
        }
    }

    /// Index into `Bar::transpose`; `None` for machines that never transpose.
    pub fn transpose_index(self) -> Option<usize> {
        match self {
            MachineId::Synth1 => Some(0),
            MachineId::Synth2 => Some(1),
            MachineId::Fm => Some(2),
            _ => None,
        }
    }
}

pub fn kind_of(machine: usize) -> Option<MachineKind> {
    MachineId::from_index(machine).map(MachineId::kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_round_trip_and_kinds_are_dense() {
        for (i, m) in MachineId::ALL.iter().enumerate() {
            assert_eq!(m.index(), i);
            assert_eq!(MachineId::from_index(i), Some(*m));
        }
        assert_eq!(MachineId::from_index(MACHINES), None);
        assert_eq!(kind_of(4), Some(MachineKind::Notes(0)));
        assert_eq!(kind_of(5), Some(MachineKind::Notes(1)));
        assert_eq!(kind_of(3), Some(MachineKind::Drum(1)));
        let transposed: Vec<_> = MachineId::ALL
            .iter()
            .filter_map(|m| m.transpose_index())
            .collect();
        assert_eq!(transposed, (0..TRANSPOSED).collect::<Vec<_>>());
    }
}
```

In `lib.rs`, add `pub mod machine;` and `mod migrate;` in alphabetical order with the others.

- [ ] **Step 2: Write the failing project tests** (append to `project.rs` tests)

```rust
    #[test]
    fn note_patterns_validate() {
        let mut p = NotePattern::default();
        assert!(p.is_well_formed());
        assert_eq!(p.ticks(), 16 * TICKS_PER_STEP);
        p.notes.push(Note { start: 0, len: 4, pitch: 60, vel: 100 });
        assert!(p.is_well_formed());
        p.length = 4;
        p.notes.push(Note { start: 200, len: 1, pitch: 60, vel: 1 });
        assert!(p.is_well_formed(), "notes past a shortened end are kept");
        let mut bad = p.clone();
        bad.notes.push(Note { start: 0, len: 0, pitch: 60, vel: 100 });
        assert!(!bad.is_well_formed(), "zero length");
        let mut bad = p.clone();
        bad.notes.push(Note { start: 0, len: 1, pitch: 60, vel: 0 });
        assert!(!bad.is_well_formed(), "zero velocity");
        let mut bad = p.clone();
        bad.notes.push(Note { start: 0, len: 1, pitch: 128, vel: 1 });
        assert!(!bad.is_well_formed(), "pitch above 127");
        let mut bad = p.clone();
        bad.notes.push(Note { start: 256, len: 1, pitch: 1, vel: 1 });
        assert!(!bad.is_well_formed(), "start past 64 steps");
        let mut bad = NotePattern::default();
        bad.notes = vec![Note { start: 0, len: 1, pitch: 1, vel: 1 }; MAX_NOTES + 1];
        assert!(!bad.is_well_formed(), "over the note cap");
    }

    #[test]
    fn note_machines_are_reachable_by_machine_index() {
        let mut p = Project::default();
        assert_eq!(p.fm.patterns.len(), PATTERNS);
        assert_eq!(p.sampler.patterns.len(), PATTERNS);
        assert!(p.note_patterns(0).is_none());
        p.note_patterns_mut(5).unwrap()[3].length = 8;
        assert_eq!(p.sampler.patterns[3].length, 8);
        assert_eq!(p.note_patterns(4).unwrap().len(), PATTERNS);
        p.fm.patterns[1].notes.push(Note { start: 0, len: 1, pitch: 60, vel: 0 });
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn v1_files_load_with_empty_new_machines() {
        use crate::migrate::{BarV1, ProjectV1, SongV1};
        let base = Project::default();
        let mut params = BTreeMap::new();
        params.insert(ParamId::Synth(0, SynthParam::Cutoff), 0.9);
        let mut synths = base.synths.clone();
        synths[1].patterns[2].length = 7;
        let v1 = ProjectV1 {
            version: 1,
            name: "Old".into(),
            bpm: 131.0,
            swing: 0.2,
            synths,
            drums: base.drums.clone(),
            params,
            selected: [1, 2, 3, 4],
            song: SongV1 {
                bars: vec![BarV1 {
                    patterns: [Some(PatternRef(1)), None, Some(PatternRef(3)), None],
                    mutes: [true, false, false, true],
                    transpose: [2, -3],
                }],
                loop_range: Some((0, 0)),
                automation: Vec::new(),
            },
        };
        let text = ron::ser::to_string_pretty(&v1, ron::ser::PrettyConfig::default()).unwrap();
        let p = Project::from_ron(&text).unwrap();
        assert_eq!(p.version, PROJECT_VERSION);
        assert_eq!((p.name.as_str(), p.bpm, p.swing), ("Old", 131.0, 0.2));
        assert_eq!(p.synths[1].patterns[2].length, 7);
        assert_eq!(p.selected, [1, 2, 3, 4, 0, 0]);
        assert_eq!(p.param(ParamId::Synth(0, SynthParam::Cutoff)), 0.9);
        let ch5 = ParamId::Channel(5, crate::params::ChannelParam::Level);
        assert_eq!(p.params.get(&ch5), Some(&ch5.info().default));
        let bar = p.song.bars[0];
        assert_eq!(
            bar.patterns,
            [Some(PatternRef(1)), None, Some(PatternRef(3)), None, None, None]
        );
        assert_eq!(bar.mutes, [true, false, false, true, false, false]);
        assert_eq!(bar.transpose, [2, -3, 0]);
        assert_eq!(p.song.loop_range, Some((0, 0)));
        assert_eq!(p.fm.patterns.len(), PATTERNS);
        assert!(p.sampler.patterns.iter().all(|n| n.notes.is_empty()));
    }
```

Update the existing `ron_round_trip` test's `Bar` literal and add a note:

```rust
        p.song.bars.push(Bar {
            patterns: [Some(PatternRef(5)), None, Some(PatternRef(2)), None, Some(PatternRef(9)), None],
            mutes: [false, true, false, false, false, true],
            transpose: [0, 3, -5],
        });
        p.fm.patterns[9].notes.push(Note { start: 3, len: 6, pitch: 64, vel: 90 });
```

- [ ] **Step 3: Run them to make sure they fail**

Run: `cargo test -p acid-engine project::`
Expected: compile errors (`Note`, `NotePattern`, `migrate` missing).

- [ ] **Step 4: Implement the v2 project types**

In `project.rs`, change `PROJECT_VERSION` to `2`. Add `use crate::machine::{MACHINES, MachineKind, TRANSPOSED, kind_of};` and these constants:

```rust
/// Note-pattern resolution: ticks per 1/16 step.
pub const TICKS_PER_STEP: u16 = 4;
/// Most notes one note pattern may hold.
pub const MAX_NOTES: usize = 256;
```

After `DrumPattern`'s impls, add:

```rust
/// One note of a poly note pattern, in ticks (`TICKS_PER_STEP` per step).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub start: u16,
    pub len: u16,
    pub pitch: u8,
    pub vel: u8,
}

/// A polyphonic pattern for the FM synth and sampler. Notes are in no
/// particular order; notes starting past `length` are kept but not played.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NotePattern {
    pub notes: Vec<Note>,
    pub length: u8,
}

impl NotePattern {
    /// Pattern length in ticks.
    pub fn ticks(&self) -> u16 {
        (self.length as u16).clamp(1, STEPS as u16) * TICKS_PER_STEP
    }

    pub fn is_well_formed(&self) -> bool {
        let max_start = STEPS as u16 * TICKS_PER_STEP;
        (1..=STEPS as u8).contains(&self.length)
            && self.notes.len() <= MAX_NOTES
            && self.notes.iter().all(|n| {
                n.len >= 1 && n.start < max_start && n.pitch <= 127 && (1..=127).contains(&n.vel)
            })
    }
}

impl Default for NotePattern {
    fn default() -> Self {
        Self {
            notes: Vec::new(),
            length: 16,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FmMachine {
    pub patterns: Vec<NotePattern>,
}

impl Default for FmMachine {
    fn default() -> Self {
        Self {
            patterns: vec![NotePattern::default(); PATTERNS],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SamplerMachine {
    pub patterns: Vec<NotePattern>,
}

impl Default for SamplerMachine {
    fn default() -> Self {
        Self {
            patterns: vec![NotePattern::default(); PATTERNS],
        }
    }
}
```

Change `Bar` to:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bar {
    pub patterns: [Option<PatternRef>; MACHINES],
    pub mutes: [bool; MACHINES],
    /// Song transpose for Synth 1, Synth 2, FM (`MachineId::transpose_index`).
    pub transpose: [i8; TRANSPOSED],
}
```

In `Project`, add `pub fm: FmMachine,` and `pub sampler: SamplerMachine,` after `drums`. Change the comment on `selected` to "Selected pattern per machine, by `MachineId::index`." and its type to `pub selected: [u8; MACHINES],`. In `Default`, add `fm: FmMachine::default(), sampler: SamplerMachine::default(),` and `selected: [0; MACHINES],`.

Add to `impl Project`:

```rust
    /// Note patterns of the FM synth (machine 4) or sampler (machine 5).
    pub fn note_patterns(&self, machine: usize) -> Option<&Vec<NotePattern>> {
        match kind_of(machine)? {
            MachineKind::Notes(0) => Some(&self.fm.patterns),
            MachineKind::Notes(_) => Some(&self.sampler.patterns),
            _ => None,
        }
    }

    pub fn note_patterns_mut(&mut self, machine: usize) -> Option<&mut Vec<NotePattern>> {
        match kind_of(machine)? {
            MachineKind::Notes(0) => Some(&mut self.fm.patterns),
            MachineKind::Notes(_) => Some(&mut self.sampler.patterns),
            _ => None,
        }
    }
```

In `validate`, add this after the drum loop:

```rust
        for (name, pats) in [("FM", &self.fm.patterns), ("Sampler", &self.sampler.patterns)] {
            if pats.len() != PATTERNS {
                return bad(format!("{name} has {} patterns", pats.len()));
            }
            if let Some(j) = pats.iter().position(|p| !p.is_well_formed()) {
                return bad(format!("{name} pattern {}", PatternRef(j as u8).label()));
            }
        }
```

Replace `from_ron` with:

```rust
    pub fn from_ron(text: &str) -> Result<Project, ProjectError> {
        let parse = |e: ron::error::SpannedError| ProjectError::Parse(e.to_string());
        let head: VersionOnly = ron::from_str(text).map_err(parse)?;
        if head.version > PROJECT_VERSION {
            return Err(ProjectError::TooNew(head.version));
        }
        let p = if head.version < PROJECT_VERSION {
            ron::from_str::<crate::migrate::ProjectV1>(text)
                .map_err(parse)?
                .upgrade()
        } else {
            ron::from_str::<Project>(text).map_err(parse)?
        };
        p.validate()?;
        Ok(p)
    }
```

(If the `ron` version in `Cargo.lock` names the error type differently, use the type `ron::from_str` returns there; the existing code used `e.to_string()`.)

- [ ] **Step 5: Create `migrate.rs`**

```rust
//! Older project files. v1 had four machines: no FM or sampler, 4-wide
//! bars and two synth transposes.

use crate::params::ParamId;
use crate::project::{AutoLane, Bar, DrumMachine, PatternRef, Project, Song, SynthMachine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct BarV1 {
    pub patterns: [Option<PatternRef>; 4],
    pub mutes: [bool; 4],
    pub transpose: [i8; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SongV1 {
    pub bars: Vec<BarV1>,
    pub loop_range: Option<(u16, u16)>,
    pub automation: Vec<AutoLane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProjectV1 {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub synths: [SynthMachine; 2],
    pub drums: [DrumMachine; 2],
    pub params: BTreeMap<ParamId, f32>,
    pub selected: [u8; 4],
    pub song: SongV1,
}

impl ProjectV1 {
    /// A v2 project: the four old machines as they were, FM and sampler
    /// empty, new params at their defaults.
    pub(crate) fn upgrade(self) -> Project {
        let mut p = Project {
            name: self.name,
            bpm: self.bpm,
            swing: self.swing,
            synths: self.synths,
            drums: self.drums,
            ..Project::default()
        };
        p.params.extend(self.params);
        p.selected[..4].copy_from_slice(&self.selected);
        p.song = Song {
            bars: self
                .song
                .bars
                .into_iter()
                .map(|b| {
                    let mut bar = Bar::default();
                    bar.patterns[..4].copy_from_slice(&b.patterns);
                    bar.mutes[..4].copy_from_slice(&b.mutes);
                    bar.transpose[..2].copy_from_slice(&b.transpose);
                    bar
                })
                .collect(),
            loop_range: self.song.loop_range,
            automation: self.song.automation,
        };
        p
    }
}
```

- [ ] **Step 6: Fix the compile errors this causes across the workspace**

Run `cargo build --workspace --all-targets` and fix each error as follows. This list covers the expected sites:
- `engine.rs` `load_project`: keep calling `self.seq.select(m, slot)` only for the four sequencer tracks that exist until Task 3:
  ```rust
          // Machines 4 and 5 join the sequencer in the next change.
          for (m, &slot) in p.selected.iter().enumerate().take(4) {
  ```
- `engine.rs` test `two_bar_demo` and `render.rs` test: `patterns: [Some(crate::project::PatternRef(0)); 4]` → `; crate::machine::MACHINES]`.
- `song.rs` tests `fn bar(refs: [Option<u8>; 4]) -> Bar`, and `sequencer.rs` tests `fn song_bar(refs: [Option<u8>; 4])`: keep the 4-element parameter and build a 6-wide bar from it:
  ```rust
        let mut patterns = [None; crate::machine::MACHINES];
        for (m, r) in refs.into_iter().enumerate() {
            patterns[m] = r.map(PatternRef);
        }
  ```
  (Keep the rest of each helper's body as it was.)
- app `ui/mod.rs`: `pub bank: [u8; 4]` → `pub bank: [u8; acid_engine::machine::MACHINES]`. Change the `focus` doc comment to "(0..MACHINES)".
- app `ui/files.rs` `load_document`: `for m in 0..4` → `for m in 0..acid_engine::machine::MACHINES`.
- Any other `[x; 4]` literal assigned to `Bar.patterns`, `Bar.mutes` or `Project.selected` → `MACHINES`, and `Bar.transpose` → `TRANSPOSED`.

- [ ] **Step 7: Run the whole suite**

Run: `cargo test --workspace`
Expected: all pass, including the 3 new project tests and the machine test.

- [ ] **Step 8: Commit**

```bash
git add -A crates
git commit -m "feat(engine): MachineId, NotePattern and project format v2 with v1 migration

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Banks and a six-track sequencer and engine

**Files:**
- Modify: `crates/acid-engine/src/song.rs`, `sequencer.rs`, `engine.rs`, `host.rs`, `render.rs`
- Modify: `crates/app/src/ui/song_panel.rs`

**Interfaces:**
- Consumes: `MachineId`, `MachineKind`, `kind_of`, `MACHINES`, `NOTE_MACHINES` (Task 2); `NotePattern` (Task 2).
- Produces:

```rust
// song.rs — replaces SynthBanks, DrumBanks and banks()
#[derive(Clone, Copy)]
pub struct Banks<'a> {
    pub synths: [&'a [SynthPattern]; 2],
    pub drums: [&'a [DrumPattern]; 2],
    pub notes: [&'a [NotePattern]; NOTE_MACHINES],
}
impl<'a> Banks<'a> {
    pub fn new(synths: &'a [Vec<SynthPattern>; 2], drums: &'a [Vec<DrumPattern>; 2],
               notes: &'a [Vec<NotePattern>; NOTE_MACHINES]) -> Self;
    pub fn of(p: &'a Project) -> Self;
    pub fn bank_len(&self, machine: usize) -> usize;              // 0 for an invalid machine
    pub fn length(&self, machine: usize, slot: u8) -> Option<usize>; // steps, clamped 1..=STEPS
}
pub fn bar_steps(bar: &Bar, banks: Banks<'_>) -> usize;
pub fn bar_start_step(song: &Song, bar: usize, banks: Banks<'_>) -> u32;
pub fn total_steps(song: &Song, banks: Banks<'_>) -> u32;
pub fn insert_bars(song: &mut Song, at: usize, bars: &[Bar], banks: Banks<'_>);
pub fn delete_bars(song: &mut Song, first: usize, last: usize, banks: Banks<'_>);
pub fn duplicate_bars(song: &mut Song, first: usize, last: usize, banks: Banks<'_>);
// sequencer.rs
pub fn tick(&mut self, banks: Banks<'_>, song: &Song, out: &mut Vec<SeqEvent>);
// engine.rs
pub fn swap_note_pattern(&mut self, machine: usize, slot: u8, p: &mut NotePattern) -> bool;
pub fn set_note_pattern(&mut self, machine: usize, slot: u8, p: NotePattern) -> NotePattern;
pub fn queued_patterns(&self) -> [Option<u8>; MACHINES];
pub fn playhead(&self) -> [usize; MACHINES];
pub fn current_patterns(&self) -> [u8; MACHINES];
// host.rs
Command::NotePattern { machine: u8, slot: u8, pattern: Box<NotePattern> }
Event::DisposeNotePattern(Box<NotePattern>)
Status { playhead: [usize; MACHINES], patterns: [u8; MACHINES], queued: [Option<u8>; MACHINES], .. }
```

In this task, note tracks select, queue and advance through their patterns like drum tracks, but emit no events yet. Task 4 adds playback.

- [ ] **Step 1: Write the failing tests**

In `song.rs` tests, add (the existing `project_with_lengths` / `bar` helpers stay):

```rust
    #[test]
    fn note_patterns_count_toward_bar_length() {
        let mut p = Project::default();
        p.sampler.patterns[3].length = 48;
        let mut b = Bar::default();
        b.patterns[5] = Some(PatternRef(3));
        assert_eq!(bar_steps(&b, Banks::of(&p)), 48);
        assert_eq!(Banks::of(&p).length(5, 3), Some(48));
        assert_eq!(Banks::of(&p).length(6, 0), None);
        assert_eq!(Banks::of(&p).bank_len(4), PATTERNS);
    }

    #[test]
    fn recorded_pattern_change_reaches_the_sampler() {
        let mut song = Song {
            bars: vec![Bar::default(); 3],
            ..Default::default()
        };
        write_pattern_ref(&mut song, 1, 5, 7);
        assert_eq!(song.bars[1].patterns[5], Some(PatternRef(7)));
        assert_eq!(song.bars[2].patterns[5], Some(PatternRef(7)));
        write_pattern_ref(&mut song, 0, 6, 7);
        assert!(song.bars[0].patterns.iter().all(Option::is_none));
    }
```

In `sequencer.rs` tests, add:

```rust
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
```

In `engine.rs` tests, add:

```rust
    #[test]
    fn note_patterns_swap_and_report() {
        use crate::project::{Note, NotePattern};
        let mut e = demo_engine();
        let mut p = NotePattern::default();
        p.notes.push(Note { start: 0, len: 4, pitch: 60, vel: 100 });
        assert!(e.swap_note_pattern(4, 2, &mut p));
        assert!(p.notes.is_empty(), "got the old, empty pattern back");
        assert!(!e.swap_note_pattern(0, 2, &mut p), "not a note machine");
        let mut bad = NotePattern { length: 0, ..NotePattern::default() };
        assert!(!e.swap_note_pattern(5, 0, &mut bad));
        e.play();
        e.select_pattern(5, 9);
        assert_eq!(e.queued_patterns()[5], Some(9));
        assert_eq!(e.current_patterns().len(), crate::machine::MACHINES);
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine`
Expected: compile errors (`Banks`, `swap_note_pattern`, `Fixture.notes`).

- [ ] **Step 3: Implement `Banks` in `song.rs`**

Delete `SynthBanks`, `DrumBanks` and `banks()`. Add (imports: `NotePattern` from `crate::project`; `MachineKind`, `NOTE_MACHINES`, `MACHINES`, `kind_of` from `crate::machine`):

```rust
/// Every machine's pattern bank, borrowed. Cheap to copy.
#[derive(Clone, Copy)]
pub struct Banks<'a> {
    pub synths: [&'a [SynthPattern]; 2],
    pub drums: [&'a [DrumPattern]; 2],
    pub notes: [&'a [NotePattern]; NOTE_MACHINES],
}

impl<'a> Banks<'a> {
    pub fn new(
        synths: &'a [Vec<SynthPattern>; 2],
        drums: &'a [Vec<DrumPattern>; 2],
        notes: &'a [Vec<NotePattern>; NOTE_MACHINES],
    ) -> Self {
        Self {
            synths: [&synths[0], &synths[1]],
            drums: [&drums[0], &drums[1]],
            notes: [&notes[0], &notes[1]],
        }
    }

    pub fn of(p: &'a Project) -> Self {
        Self {
            synths: [&p.synths[0].patterns, &p.synths[1].patterns],
            drums: [&p.drums[0].patterns, &p.drums[1].patterns],
            notes: [&p.fm.patterns, &p.sampler.patterns],
        }
    }

    /// Patterns in `machine`'s bank; 0 for an invalid machine.
    pub fn bank_len(&self, machine: usize) -> usize {
        match kind_of(machine) {
            Some(MachineKind::Synth(s)) => self.synths[s].len(),
            Some(MachineKind::Drum(k)) => self.drums[k].len(),
            Some(MachineKind::Notes(n)) => self.notes[n].len(),
            None => 0,
        }
    }

    /// Length in steps (1..=STEPS) of `machine`'s pattern `slot`.
    pub fn length(&self, machine: usize, slot: u8) -> Option<usize> {
        let s = slot as usize;
        let length = match kind_of(machine)? {
            MachineKind::Synth(i) => self.synths[i].get(s)?.length,
            MachineKind::Drum(i) => self.drums[i].get(s)?.length,
            MachineKind::Notes(i) => self.notes[i].get(s)?.length,
        };
        Some((length as usize).clamp(1, STEPS))
    }
}
```

Rewrite `bar_steps`:

```rust
/// Steps in a bar: its longest assigned pattern, or 16 when none is assigned.
pub fn bar_steps(bar: &Bar, banks: Banks<'_>) -> usize {
    let longest = (0..MACHINES)
        .filter_map(|m| banks.length(m, bar.patterns[m]?.0))
        .max()
        .unwrap_or(0);
    if longest == 0 {
        DEFAULT_BAR_STEPS
    } else {
        longest
    }
}
```

In `bar_start_step`, `total_steps`, `insert_bars`, `delete_bars` and `duplicate_bars`, replace the `synths: SynthBanks<'_>, drums: DrumBanks<'_>` parameter pair with `banks: Banks<'_>`, and every inner `synths, drums` argument pair with `banks`. In `write_pattern_ref`, change `machine >= 4` to `machine >= MACHINES`. In the `song.rs` tests, replace every `let (s, d) = banks(&p);` with `let b = Banks::of(&p);` and every `s, d` argument pair with `b`.

- [ ] **Step 4: Six tracks in the sequencer**

In `sequencer.rs`:
- Imports: `use crate::machine::{MACHINES, MachineId};` and `use crate::song::{Banks, DEFAULT_BAR_STEPS, bar_start_step, bar_steps, value_at};`. Drop `DrumPattern` and `SynthPattern` from the `crate::project` import if they become unused outside tests.
- Fields: `tracks: [Track; MACHINES]` (doc comment "indexed by `MachineId::index`"), `chosen: [u8; MACHINES]`; in `new`, `[Track::default(); MACHINES]` and `[0; MACHINES]`.
- `load_bar(&mut self, song: &Song, banks: Banks<'_>, out: &mut Vec<SeqEvent>) -> bool` — the per-machine loop becomes:

```rust
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
```

  and its seek line becomes `bar_start_step(song, self.song_bar, banks)`.
- `apply_resync(&mut self, song: &Song, banks: Banks<'_>)`, using `bar_start_step(song, self.song_bar, banks)` and `bar_steps(bar, banks)`.
- `tick(&mut self, banks: Banks<'_>, song: &Song, out: &mut Vec<SeqEvent>)`:
  - `synths[s]` → `banks.synths[s]`, `drums[k]` → `banks.drums[k]`, and pass `banks` to `load_bar` and `apply_resync`
  - the `Phase::Mid` advance loop becomes:

```rust
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
```

- Tests: give `Fixture` a `notes: [Vec<NotePattern>; 2]` field, initialised to `vec![NotePattern::default(); 4]` each (import `crate::project::NotePattern` in the test module), and add:

```rust
        fn banks(&self) -> Banks<'_> {
            Banks::new(&self.synths, &self.drums, &self.notes)
        }
```

  Replace the `seq.tick([...], [...], &self.song, &mut buf)` call with `seq.tick(self.banks(), &self.song, &mut buf)`, and do the same for any other `tick(` call in the tests.

- [ ] **Step 5: Six machines in the engine and host**

`engine.rs`:
- Imports: `crate::machine::{MACHINES, MachineKind, NOTE_MACHINES, kind_of}`; `NotePattern` from `crate::project`; `crate::song::Banks`.
- Field: `note_patterns: [Vec<NotePattern>; NOTE_MACHINES],`, initialised in `new` to `std::array::from_fn(|_| vec![NotePattern::default(); PATTERNS])`.
- `load_project`:
  - after the synth and drum loop, add `self.note_patterns = [p.fm.patterns.clone(), p.sampler.patterns.clone()];`
  - remove the `.take(4)` added in Task 2
- `select_pattern`: `if machine < MACHINES && (slot as usize) < PATTERNS`. Change its doc comment to `machine >= MACHINES`.
- `queued_patterns`/`playhead`/`current_patterns` return `[_; MACHINES]`.
- In `process_block`, replace the `self.seq.tick([...], [...], &self.song, &mut self.events)` call with:

```rust
                self.seq.tick(
                    Banks::new(&self.synth_patterns, &self.drum_patterns, &self.note_patterns),
                    &self.song,
                    &mut self.events,
                );
```

- Add after `set_drum_pattern`:

```rust
    /// Swaps `p` into note machine `machine` (4 = FM, 5 = Sampler).
    /// Real-time safe. An invalid target or malformed pattern leaves both
    /// untouched and returns false.
    pub fn swap_note_pattern(&mut self, machine: usize, slot: u8, p: &mut NotePattern) -> bool {
        let Some(MachineKind::Notes(n)) = kind_of(machine) else {
            return false;
        };
        if slot as usize >= PATTERNS || !p.is_well_formed() {
            return false;
        }
        std::mem::swap(&mut self.note_patterns[n][slot as usize], p);
        true
    }

    pub fn set_note_pattern(&mut self, machine: usize, slot: u8, mut p: NotePattern) -> NotePattern {
        self.swap_note_pattern(machine, slot, &mut p);
        p
    }
```

- Tests: update the asserts `assert_eq!(e.current_patterns(), [0, 0, 0, 0])` to `[0; MACHINES]` (two places), and `assert_eq!(e.queued_patterns(), [None, None, None, Some(5)])` to `[None, None, None, Some(5), None, None]`.

`host.rs`:
- `Status` fields: `playhead: [usize; MACHINES]`, `patterns: [u8; MACHINES]`, `queued: [Option<u8>; MACHINES]`.
- Add a `Command` variant:

```rust
    NotePattern {
        machine: u8,
        slot: u8,
        pattern: Box<NotePattern>,
    },
```

- Add the event `DisposeNotePattern(Box<NotePattern>),`.
- Add a `drain_commands` arm:

```rust
                Command::NotePattern {
                    machine,
                    slot,
                    mut pattern,
                } => {
                    self.engine
                        .swap_note_pattern(machine as usize, slot, &mut pattern);
                    self.dispose(Event::DisposeNotePattern(pattern));
                }
```

`render.rs`: replace `let (synths, drums) = song::banks(&p); let steps = song::total_steps(&p.song, synths, drums)...` with `let steps = song::total_steps(&p.song, song::Banks::of(&p)).max(1) as f64;`.

App `song_panel.rs` `song_action`: replace `let (synths, drums) = song::banks(&self.doc.project);` with `let banks = song::Banks::of(&self.doc.project);`, and each `synths, drums` argument pair with `banks`.

- [ ] **Step 6: Run the whole suite**

Run: `cargo test --workspace`
Expected: all pass (existing sequencer timing tests unchanged).

- [ ] **Step 7: Commit**

```bash
git add -A crates
git commit -m "feat(engine): Banks and six sequencer tracks with note pattern storage

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Quarter-step clock and poly note events

**Files:**
- Modify: `crates/acid-engine/src/sequencer.rs`
- Modify: `crates/acid-engine/src/engine.rs` (`EVENT_RESERVE`, exhaustive event match)

**Interfaces:**
- Consumes: `Banks`, six tracks (Task 3); `TICKS_PER_STEP`, `MAX_NOTES` (Task 2).
- Produces:

```rust
SeqEvent::PolyOn { machine: usize, pitch: u8, vel: u8 }   // machine is 4 or 5
SeqEvent::PolyOff { machine: usize, pitch: u8 }
```

The sequencer ticks 4 times per step:
- **sub 0:** the old StepStart (song bar load, automation, synth notes, drums).
- **sub 2:** the old Mid (synth gate-offs, advancing machines 0–3, song position).
- **Every sub-tick:** the note tracks.

Note-track rules at each tick, where `pos` is the track's tick in its pattern:
1. A silent track releases all held notes and does nothing else.
2. At `pos == 0`, all held notes are released. This covers the loop end, pattern switches and bar changes.
3. Otherwise, a held note whose `min(start + len, ticks) == pos` gets `PolyOff`.
4. Every note with `start == pos` gets `PolyOn`, with pitch `clamp(pitch + transpose_add, 0, 127)`.
5. `pos` advances; at `ticks` it wraps to 0 and the queued pattern takes over.

Held pitches live in a `u128` bitmask per track, so a release always matches what actually sounded. Offs are emitted before ons. `global_step`, used only for swing timing, now increments when `sub` wraps from 3 to 0, so each quarter uses the length of the step it belongs to.

- [ ] **Step 1: Write the failing tests** (in `sequencer.rs` tests)

At 120 BPM a step is 5512.5 samples and a tick is 1378.125 samples. Ticks land on the next whole sample: tick 1 → 1379, tick 4 → 5513, tick 5 → 6891, tick 64 → 88200.

```rust
    use crate::project::Note;

    fn note(start: u16, len: u16, pitch: u8) -> Note {
        Note { start, len, pitch, vel: 100 }
    }

    fn poly(ev: &[(usize, SeqEvent)]) -> Vec<(usize, SeqEvent)> {
        ev.iter()
            .filter(|(_, e)| matches!(e, SeqEvent::PolyOn { .. } | SeqEvent::PolyOff { .. }))
            .copied()
            .collect()
    }

    #[test]
    fn poly_notes_land_on_their_ticks() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(1, 4, 60));
        let mut s = new_seq();
        s.play();
        let ev = poly(&f.run(&mut s, 8000));
        assert_eq!(
            ev,
            vec![
                (1379, SeqEvent::PolyOn { machine: 4, pitch: 60, vel: 100 }),
                (6891, SeqEvent::PolyOff { machine: 4, pitch: 60 }),
            ]
        );
    }

    #[test]
    fn chords_start_together_on_the_sampler() {
        let mut f = Fixture::new();
        for p in [60, 64, 67] {
            f.notes[1][0].notes.push(note(0, 2, p));
        }
        let mut s = new_seq();
        s.play();
        let ons: Vec<_> = poly(&f.run(&mut s, 100))
            .into_iter()
            .filter(|(pos, e)| *pos == 0 && matches!(e, SeqEvent::PolyOn { machine: 5, .. }))
            .collect();
        assert_eq!(ons.len(), 3);
    }

    #[test]
    fn notes_are_cut_at_the_loop_end() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(60, 40, 50));
        let mut s = new_seq();
        s.play();
        let ev = poly(&f.run(&mut s, 88_300));
        assert_eq!(
            ev.last(),
            Some(&(88_200, SeqEvent::PolyOff { machine: 4, pitch: 50 }))
        );
    }

    #[test]
    fn song_mutes_and_transposes_note_tracks() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(0, 4, 60));
        f.notes[1][0].notes.push(note(0, 4, 60));
        let mut bar = song_bar([None; 4]);
        bar.patterns[4] = Some(PatternRef(0));
        bar.patterns[5] = Some(PatternRef(0));
        bar.mutes[5] = true;
        bar.transpose[2] = 3;
        f.song.bars = vec![bar];
        let mut s = new_seq();
        s.set_mode(Mode::Song);
        s.play();
        let ev = poly(&f.run(&mut s, 2000));
        assert_eq!(ev, vec![(0, SeqEvent::PolyOn { machine: 4, pitch: 63, vel: 100 })]);
    }

    #[test]
    fn a_song_ending_releases_held_notes() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(0, 64, 60));
        let mut bar = song_bar([None; 4]);
        bar.patterns[4] = Some(PatternRef(0));
        f.song.bars = vec![bar];
        let mut s = new_seq();
        s.set_mode(Mode::Song);
        s.play();
        let ev = poly(&f.run(&mut s, 90_000));
        assert_eq!(
            ev.last(),
            Some(&(88_200, SeqEvent::PolyOff { machine: 4, pitch: 60 }))
        );
        assert!(!s.is_playing());
    }

    #[test]
    fn a_queued_switch_releases_the_old_patterns_notes() {
        let mut f = Fixture::new();
        f.notes[0][0].length = 1;
        f.notes[0][0].notes.push(note(0, 4, 60));
        f.notes[0][1].length = 1;
        f.notes[0][1].notes.push(note(0, 4, 62));
        let mut s = new_seq();
        s.play();
        s.select(4, 1);
        let ev = poly(&f.run(&mut s, 5600));
        assert_eq!(
            ev,
            vec![
                (0, SeqEvent::PolyOn { machine: 4, pitch: 60, vel: 100 }),
                (5513, SeqEvent::PolyOff { machine: 4, pitch: 60 }),
                (5513, SeqEvent::PolyOn { machine: 4, pitch: 62, vel: 100 }),
            ]
        );
    }
```

(`PatternRef` and `Mode` are already in scope through `use super::*`. If `song_bar` builds its bar differently, set the fields on the returned value as shown.)

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine sequencer::`
Expected: compile error (`SeqEvent::PolyOn` missing).

- [ ] **Step 3: Implement**

In `sequencer.rs`:

1. Add the events to `SeqEvent`:

```rust
    /// A note machine (4 = FM, 5 = Sampler) starts a note.
    PolyOn {
        machine: usize,
        pitch: u8,
        vel: u8,
    },
    PolyOff {
        machine: usize,
        pitch: u8,
    },
```

2. Delete `enum Phase`. Add `const SUBS: u8 = 4;` and a `sub: u8` field, where `sub` is the quarter of the current step (0 = step start, 2 = mid-step), in place of `phase`. Initialise it to `0` in `new` and in `play` (where `self.phase = Phase::StepStart` was).

3. Add two fields to `Track`:

```rust
    /// Note tracks: tick position in the pattern.
    pos: u16,
    /// Note tracks: pitches sounding now, one bit per MIDI note.
    held: u128,
```

   In `play()` and `stop()`, in the loop over tracks, add `t.pos = 0; t.held = 0;`. In `load_bar`'s per-machine loop, add `t.pos = 0;` next to `t.step = 0;`.

4. In `load_bar`'s "no bar here" branch, before `self.stop();`:

```rust
            for m in NOTE_TRACKS {
                release_all(&mut self.tracks[m], m, out);
            }
```

5. Add helpers at module level (import `crate::machine::NOTE_MACHINES`; `crate::project::TICKS_PER_STEP`; add `NotePattern` to the `crate::project` import if needed):

```rust
/// Track indices of the note machines (FM, Sampler).
const NOTE_TRACKS: std::ops::Range<usize> = 4..4 + NOTE_MACHINES;

fn transposed(pitch: u8, add: i8) -> u8 {
    (pitch as i16 + add as i16).clamp(0, 127) as u8
}

/// Note-offs for every pitch the track holds.
fn release_all(t: &mut Track, machine: usize, out: &mut Vec<SeqEvent>) {
    while t.held != 0 {
        let pitch = t.held.trailing_zeros() as u8;
        t.held &= !(1u128 << pitch);
        out.push(SeqEvent::PolyOff { machine, pitch });
    }
}
```

6. Add a method:

```rust
    /// One tick of each note track: releases, then starts, then advance.
    fn note_ticks(&mut self, banks: Banks<'_>, out: &mut Vec<SeqEvent>) {
        for m in NOTE_TRACKS {
            let t = &mut self.tracks[m];
            if t.silent {
                release_all(t, m, out);
                continue;
            }
            let pat = &banks.notes[m - 4][t.pattern as usize];
            let ticks = pat.ticks();
            if t.pos >= ticks {
                t.pos = 0;
            }
            if t.pos == 0 {
                release_all(t, m, out);
            } else {
                for n in &pat.notes {
                    if n.start.saturating_add(n.len).min(ticks) == t.pos {
                        let pitch = transposed(n.pitch, t.transpose_add);
                        if t.held & (1u128 << pitch) != 0 {
                            t.held &= !(1u128 << pitch);
                            out.push(SeqEvent::PolyOff { machine: m, pitch });
                        }
                    }
                }
            }
            for n in &pat.notes {
                if n.start == t.pos {
                    let pitch = transposed(n.pitch, t.transpose_add);
                    t.held |= 1u128 << pitch;
                    out.push(SeqEvent::PolyOn {
                        machine: m,
                        pitch,
                        vel: n.vel,
                    });
                }
            }
            t.step = (t.pos / TICKS_PER_STEP) as usize;
            t.pos += 1;
            if t.pos >= ticks {
                t.pos = 0;
                if let Some(q) = t.queued.take() {
                    t.pattern = q;
                }
            }
        }
    }
```

7. Restructure `tick`:
   - Replace `let half = self.step_len(self.global_step) * 0.5;` with `let quarter = self.step_len(self.global_step) * 0.25;`.
   - Change `match self.phase {` to `match self.sub {`. `Phase::StepStart =>` becomes `0 =>`, and `Phase::Mid =>` becomes `2 =>`. Add `_ => {}`.
   - Delete `self.phase = Phase::Mid;` and `self.phase = Phase::StepStart;`.
   - In the Mid (sub 2) block:
     - delete the line `self.global_step += 1;`
     - in the advance loop, skip the note tracks, since they advance in `note_ticks`: `for m in 0..MACHINES { if NOTE_TRACKS.contains(&m) { continue; } ... }`
   - After the match, replace `self.samples_to_next += half;` with:

```rust
        self.note_ticks(banks, out);
        self.sub = (self.sub + 1) % SUBS;
        if self.sub == 0 {
            self.global_step += 1;
        }
        self.samples_to_next += quarter;
```

   The early `return` inside sub 0, when `load_bar` ends the song, stays as is: the song has stopped and `load_bar` already released the held notes.

In `engine.rs`:
- Raise the event reserve so a worst-case tick never grows the buffer: up to 128 releases and `MAX_NOTES` starts per note machine.

```rust
/// Sequencer event buffer size: never grows on the audio thread. Room for
/// every param, plus per note machine 128 releases and MAX_NOTES starts.
const EVENT_RESERVE: usize = 64 + 2 * PARAM_COUNT + NOTE_MACHINES * (128 + MAX_NOTES);
```

  Import `MAX_NOTES` from `crate::project`.
- In `process_block`'s event match, add a temporary arm, which Task 5 replaces: `SeqEvent::PolyOn { .. } | SeqEvent::PolyOff { .. } => {}`.

- [ ] **Step 4: Run the whole suite**

Run: `cargo test --workspace`
Expected: all pass. The existing timing tests (`four_on_the_floor_is_sample_accurate`, `odd_steps_land_on_the_next_whole_sample`, swing tests, song tests) must pass **unchanged**. If one is off by a sample, the quarter accumulation is wrong; fix the code, not the test.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "feat(engine): quarter-step sequencer clock and poly note events

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Placeholder sine voices and live audition

**Files:**
- Create: `crates/acid-engine/src/placeholder.rs`
- Modify: `crates/acid-engine/src/lib.rs`, `engine.rs`, `host.rs`

**Interfaces:**
- Consumes: `SeqEvent::PolyOn` and `PolyOff` (Task 4); `FmParam` and `SamplerParam` (Task 1).
- Produces:

```rust
// placeholder.rs
pub struct SineVoices;
impl SineVoices {
    pub fn new(sample_rate: f32) -> Self;
    pub fn set_volume(&mut self, v: f32);            // 0..1 knob
    pub fn note_on(&mut self, pitch: u8, vel: u8);
    pub fn note_off(&mut self, pitch: u8);
    pub fn all_off(&mut self);                       // release, tails ring out
    pub fn reset(&mut self);                         // silence now
    pub fn active_voices(&self) -> usize;
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]); // adds
}
// engine.rs
pub fn note_on(&mut self, machine: usize, pitch: u8, vel: u8);   // ignored unless machine 4/5
pub fn note_off(&mut self, machine: usize, pitch: u8);
// host.rs
Command::NoteOn { machine: u8, pitch: u8, vel: u8 }
Command::NoteOff { machine: u8, pitch: u8 }
```

- [ ] **Step 1: Write the failing tests**

`placeholder.rs` (create it with only the test module first, plus `pub struct SineVoices;`, so the test fails on the missing methods), then add to `lib.rs`: `pub mod placeholder;`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 48_000.0;

    fn run(v: &mut SineVoices, n: usize) -> Vec<f32> {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        v.render(&mut l, &mut r);
        assert_eq!(l, r, "mono placeholder");
        l
    }

    #[test]
    fn a_note_sounds_and_its_release_dies() {
        let mut v = SineVoices::new(SR);
        v.note_on(69, 127);
        let l = run(&mut v, 4800);
        assert!(peak(&l) > 0.1 && peak(&l) <= 1.0 && all_finite(&l));
        v.note_off(69);
        run(&mut v, 4800);
        assert_eq!(v.active_voices(), 0);
        assert_eq!(peak(&run(&mut v, 100)), 0.0);
    }

    #[test]
    fn chords_use_voices_and_the_ninth_steals_the_oldest() {
        let mut v = SineVoices::new(SR);
        for p in 60..68 {
            v.note_on(p, 100);
        }
        assert_eq!(v.active_voices(), 8);
        v.note_on(80, 100);
        assert_eq!(v.active_voices(), 8);
        v.note_off(60);
        run(&mut v, 4800);
        assert_eq!(v.active_voices(), 8, "pitch 60 was stolen, so nothing released");
        v.all_off();
        run(&mut v, 4800);
        assert_eq!(v.active_voices(), 0);
    }

    #[test]
    fn volume_zero_is_silent_and_reset_is_immediate() {
        let mut v = SineVoices::new(SR);
        v.set_volume(0.0);
        v.note_on(60, 127);
        assert_eq!(peak(&run(&mut v, 480)), 0.0);
        v.set_volume(1.0);
        v.reset();
        assert_eq!(v.active_voices(), 0);
    }
}
```

Engine tests (in `engine.rs`):

```rust
    #[test]
    fn note_patterns_play_on_their_own_channels() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm.patterns[0].notes.push(Note { start: 0, len: 32, pitch: 60, vel: 120 });
        p.sampler.patterns[0].notes.push(Note { start: 0, len: 32, pitch: 72, vel: 120 });
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.5);
        let m = e.take_meters();
        assert!(m[4] > 0.01 && m[5] > 0.01, "{m:?}");
        assert!(m[..4].iter().all(|v| *v == 0.0), "{m:?}");
    }

    #[test]
    fn live_notes_audition_while_stopped() {
        let mut e = Engine::new(SR);
        e.note_on(0, 60, 100);
        e.note_on(4, 200, 100);
        e.note_on(5, 60, 0);
        let (l, _) = render(&mut e, 0.05);
        assert_eq!(peak(&l), 0.0, "bad machine, pitch and velocity are ignored");
        e.note_on(5, 60, 100);
        let (l, _) = render(&mut e, 0.1);
        assert!(peak(&l) > 0.01);
        e.note_off(5, 60);
        render(&mut e, 0.3);
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-4);
    }

    #[test]
    fn stop_releases_note_machines() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm.patterns[0].notes.push(Note { start: 0, len: 64, pitch: 60, vel: 120 });
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.2);
        e.stop();
        render(&mut e, 0.3);
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-4);
    }

    #[test]
    fn fm_volume_knob_reaches_the_voice() {
        use crate::params::FmParam;
        use crate::project::Note;
        let mut p = Project::default();
        p.fm.patterns[0].notes.push(Note { start: 0, len: 32, pitch: 60, vel: 120 });
        p.params.insert(ParamId::Fm(FmParam::Volume), 0.0);
        let mut e = Engine::from_project(SR, &p);
        e.play();
        let (l, _) = render(&mut e, 0.3);
        assert_eq!(peak(&l), 0.0);
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine`
Expected: compile errors (missing methods).

- [ ] **Step 3: Implement `SineVoices`** (above the test module in `placeholder.rs`)

```rust
//! Stand-in voice for the FM and sampler slots until their real engines
//! arrive: 8 sine voices with a 5 ms attack and 80 ms release. Real-time
//! safe: fixed voice array, no allocation.

use std::f32::consts::TAU;

const VOICES: usize = 8;
const ATTACK_S: f32 = 0.005;
const RELEASE_S: f32 = 0.08;
/// Per-voice peak before volume, so a full chord stays below clipping.
const VOICE_GAIN: f32 = 0.3;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    held: bool,
    pitch: u8,
    phase: f32,
    inc: f32,
    amp: f32,
    env: f32,
    /// Start order, for stealing the oldest voice.
    age: u32,
}

pub struct SineVoices {
    voices: [Voice; VOICES],
    sample_rate: f32,
    attack_step: f32,
    release_step: f32,
    volume: f32,
    clock: u32,
}

impl SineVoices {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: [Voice::default(); VOICES],
            sample_rate,
            attack_step: 1.0 / (ATTACK_S * sample_rate),
            release_step: 1.0 / (RELEASE_S * sample_rate),
            volume: 0.49,
            clock: 0,
        }
    }

    /// Knob 0..1, squared like the other volume knobs.
    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0).powi(2);
    }

    pub fn note_on(&mut self, pitch: u8, vel: u8) {
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        let i = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| {
                (0..VOICES)
                    .max_by_key(|&i| clock.wrapping_sub(self.voices[i].age))
                    .unwrap_or(0)
            });
        let freq = 440.0 * 2f32.powf((pitch.min(127) as f32 - 69.0) / 12.0);
        self.voices[i] = Voice {
            active: true,
            held: true,
            pitch,
            phase: 0.0,
            inc: freq / self.sample_rate,
            amp: vel.min(127) as f32 / 127.0 * VOICE_GAIN,
            env: 0.0,
            age: clock,
        };
    }

    pub fn note_off(&mut self, pitch: u8) {
        for v in &mut self.voices {
            if v.active && v.held && v.pitch == pitch {
                v.held = false;
            }
        }
    }

    pub fn all_off(&mut self) {
        for v in &mut self.voices {
            v.held = false;
        }
    }

    pub fn reset(&mut self) {
        self.voices = [Voice::default(); VOICES];
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Adds into `left`/`right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (attack, release, volume) = (self.attack_step, self.release_step, self.volume);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                if v.held {
                    v.env = (v.env + attack).min(1.0);
                } else {
                    v.env -= release;
                    if v.env <= 0.0 {
                        v.active = false;
                        break;
                    }
                }
                let s = (v.phase * TAU).sin() * v.amp * v.env * volume;
                *l += s;
                *r += s;
                v.phase += v.inc;
                v.phase -= v.phase.floor();
            }
        }
    }
}
```

- [ ] **Step 4: Wire the engine and host**

`engine.rs`:
- Field `note_machines: [SineVoices; NOTE_MACHINES],`, initialised to `std::array::from_fn(|_| SineVoices::new(sample_rate))`. Import `crate::placeholder::SineVoices` and `crate::params::{FmParam, SamplerParam}` as needed.
- `apply_param`: add arms before the `_ =>` mixer arm:

```rust
            ParamId::Fm(FmParam::Volume) => self.note_machines[0].set_volume(v),
            ParamId::Sampler(SamplerParam::Volume) => self.note_machines[1].set_volume(v),
```

- Replace the temporary `PolyOn`/`PolyOff` arm from Task 4 with:

```rust
                        SeqEvent::PolyOn { machine, pitch, vel } => self.note_on(machine, pitch, vel),
                        SeqEvent::PolyOff { machine, pitch } => self.note_off(machine, pitch),
```

- In `process_block`, after the kits render loop:

```rust
            for n in 0..NOTE_MACHINES {
                let c = &mut self.chans[4 + n];
                self.note_machines[n].render(&mut c.l[span.clone()], &mut c.r[span.clone()]);
            }
```

- In the guard-fired branch, add `for v in &mut self.note_machines { v.reset(); }`.
- In `stop()`, after the synth `note_off` loop, add `for v in &mut self.note_machines { v.all_off(); }`.
- Public audition API:

```rust
    /// Starts a note on note machine `machine` (4 = FM, 5 = Sampler) right
    /// away, playing or not. Other machines, pitches above 127 and
    /// velocity 0 are ignored.
    pub fn note_on(&mut self, machine: usize, pitch: u8, vel: u8) {
        if let Some(MachineKind::Notes(n)) = kind_of(machine)
            && pitch <= 127
            && vel > 0
        {
            self.note_machines[n].note_on(pitch, vel.min(127));
        }
    }

    pub fn note_off(&mut self, machine: usize, pitch: u8) {
        if let Some(MachineKind::Notes(n)) = kind_of(machine) {
            self.note_machines[n].note_off(pitch);
        }
    }
```

`host.rs`: add these `Command` variants

```rust
    /// Audition: play a note on a note machine now.
    NoteOn {
        machine: u8,
        pitch: u8,
        vel: u8,
    },
    NoteOff {
        machine: u8,
        pitch: u8,
    },
```

and these `drain_commands` arms:

```rust
                Command::NoteOn { machine, pitch, vel } => {
                    self.engine.note_on(machine as usize, pitch, vel)
                }
                Command::NoteOff { machine, pitch } => self.engine.note_off(machine as usize, pitch),
```

- [ ] **Step 5: Run the whole suite**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(engine): placeholder sine voices for FM and Sampler, live audition

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: No-allocation test for `Engine::process`

**Files:**
- Create: `crates/acid-engine/tests/no_alloc.rs`

**Interfaces:**
- Consumes: `Engine::{from_project, play, set_mode, process, note_on, note_off, swap_note_pattern}`, `acid_engine::demo::demo_project`, `Project` fields, `song::write_automation` (all public).

This test proves the real-time rule. Only the test thread's allocations are counted, and only while armed.

- [ ] **Step 1: Write the test**

```rust
//! `Engine::process` must never allocate (or free): it runs on the audio
//! thread. A counting global allocator records allocations made by this
//! thread while armed.

use acid_engine::engine::Engine;
use acid_engine::params::{MasterParam, ParamId};
use acid_engine::project::{Bar, Note, NotePattern, PatternRef, Project};
use acid_engine::sequencer::Mode;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn note_alloc() {
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_alloc();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note_alloc();
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_alloc();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Runs `f` and returns how many allocations this thread made in it.
fn count_allocs(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    COUNT.with(|c| c.get())
}

const SR: f32 = 48_000.0;

/// The demo groove plus chords on the FM and a riff on the sampler, a
/// 4-bar looped song using every machine, and master-level automation.
fn busy_project() -> Project {
    let mut p = acid_engine::demo::demo_project();
    for (i, root) in [48u8, 53, 55, 50].into_iter().enumerate() {
        for third in [0, 4, 7] {
            p.fm.patterns[0].notes.push(Note {
                start: i as u16 * 16,
                len: 14,
                pitch: root + third,
                vel: 100,
            });
        }
    }
    for k in 0..32u16 {
        p.sampler.patterns[0].notes.push(Note {
            start: k * 2,
            len: 1,
            pitch: 36 + (k % 12) as u8,
            vel: 90,
        });
    }
    let bar = Bar {
        patterns: [Some(PatternRef(0)); acid_engine::machine::MACHINES],
        ..Bar::default()
    };
    p.song.bars = vec![bar; 4];
    p.song.loop_range = Some((0, 3));
    acid_engine::song::write_automation(&mut p.song, ParamId::Master(MasterParam::Level), 0, 63, 0.6);
    p
}

fn render_seconds(e: &mut Engine, l: &mut [f32], r: &mut [f32], seconds: f32) {
    let blocks = (seconds * SR / l.len() as f32) as usize;
    for _ in 0..blocks {
        e.process(l, r);
    }
}

#[test]
fn pattern_mode_does_not_allocate() {
    let mut e = Engine::from_project(SR, &busy_project());
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    e.play();
    let n = count_allocs(|| render_seconds(&mut e, &mut l, &mut r, 6.0));
    assert_eq!(n, 0);
}

#[test]
fn song_mode_with_automation_does_not_allocate() {
    let mut e = Engine::from_project(SR, &busy_project());
    let (mut l, mut r) = (vec![0.0; 333], vec![0.0; 333]);
    e.set_mode(Mode::Song);
    e.play();
    let n = count_allocs(|| render_seconds(&mut e, &mut l, &mut r, 10.0));
    assert_eq!(n, 0);
}

#[test]
fn audition_and_pattern_swaps_do_not_allocate() {
    let mut e = Engine::from_project(SR, &busy_project());
    let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
    let mut replacement = NotePattern::default();
    replacement.notes.push(Note { start: 0, len: 8, pitch: 60, vel: 100 });
    e.play();
    let n = count_allocs(|| {
        e.note_on(4, 72, 110);
        e.note_on(5, 40, 110);
        render_seconds(&mut e, &mut l, &mut r, 0.5);
        assert!(e.swap_note_pattern(4, 0, &mut replacement));
        e.note_off(4, 72);
        e.note_off(5, 40);
        render_seconds(&mut e, &mut l, &mut r, 1.0);
        e.stop();
        render_seconds(&mut e, &mut l, &mut r, 0.5);
    });
    assert_eq!(n, 0);
    drop(replacement);
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p acid-engine --test no_alloc`
Expected: PASS.

If any test fails, there is a real-time bug; fix the engine, never the test. To find the allocation, temporarily change `note_alloc` to disarm, print a backtrace, then re-arm:

```rust
fn note_alloc() {
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            armed.set(false);
            eprintln!("{}", std::backtrace::Backtrace::force_capture());
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
            armed.set(true);
        }
    });
}
```

and run with `-- --nocapture`. Revert this debug change before committing. If the allocation is in pre-existing code (not this plan's), fix it in a separate commit with a `fix(engine):` message.

- [ ] **Step 3: Run the whole suite and commit**

Run: `cargo test --workspace` (expected: all pass)

```bash
git add crates/acid-engine/tests/no_alloc.rs
git commit -m "test(engine): prove Engine::process never allocates

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Note-pattern edits, undo and clipboard in the app

**Files:**
- Modify: `crates/app/src/document.rs`, `crates/app/src/ui/files.rs`

**Interfaces:**
- Consumes: `Project::note_patterns(_mut)`, `MachineKind`, `kind_of` (Task 2); `Command::NotePattern` (Task 3).
- Produces:

```rust
Edit::NotePattern { machine: usize, slot: u8, pattern: Box<NotePattern> }
Edit::NoteLength { machine: usize, slot: u8, length: u8 }
files::Clip::Notes(NotePattern)
```

`machine` is always 4 or 5. Callers only build these edits for note machines.

- [ ] **Step 1: Write the failing tests**

In `document.rs` tests:

```rust
    #[test]
    fn note_pattern_edits_undo_and_sync_the_engine() {
        use acid_engine::project::{Note, NotePattern};
        let mut d = doc();
        let mut p = NotePattern::default();
        p.notes.push(Note { start: 2, len: 3, pitch: 64, vel: 99 });
        let cmds = d.apply(
            Edit::NotePattern {
                machine: 5,
                slot: 7,
                pattern: Box::new(p.clone()),
            },
            false,
        );
        assert_eq!(d.project.sampler.patterns[7], p);
        assert!(matches!(
            cmds.as_slice(),
            [Command::NotePattern { machine: 5, slot: 7, pattern }] if **pattern == p
        ));
        d.apply(Edit::NoteLength { machine: 5, slot: 7, length: 99 }, false);
        assert_eq!(d.project.sampler.patterns[7].length, 64, "clamped");
        d.undo();
        assert_eq!(d.project.sampler.patterns[7].length, 16);
        d.undo();
        assert!(d.project.sampler.patterns[7].notes.is_empty());
        assert!(d.project.fm.patterns[7].notes.is_empty());
    }
```

In `ui/mod.rs` tests (next to `copy_paste_between_synths_is_undoable`; follow that test's setup for building the headless app):

```rust
    #[test]
    fn note_patterns_copy_between_fm_and_sampler_but_not_onto_synths() {
        use acid_engine::project::Note;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.doc.project.fm.patterns[0]
            .notes
            .push(Note { start: 0, len: 4, pitch: 60, vel: 100 });
        app.view.focus = 4;
        app.copy_pattern();
        app.view.focus = 5;
        app.paste_pattern();
        assert_eq!(app.doc.project.sampler.patterns[0].notes.len(), 1);
        app.doc.undo();
        assert!(app.doc.project.sampler.patterns[0].notes.is_empty());
        app.view.focus = 0;
        app.paste_pattern();
        assert!(app.view.message.is_some(), "refused onto a 303 pattern");
    }
```

(Use whatever imports the neighbouring tests use for `Document`/`Project`.)

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p sidacid`
Expected: compile errors.

- [ ] **Step 3: Implement**

`document.rs`:
- Add `NotePattern` to the `acid_engine::project` import.
- Add the `Edit` variants:

```rust
    NotePattern {
        machine: usize,
        slot: u8,
        pattern: Box<NotePattern>,
    },
    NoteLength {
        machine: usize,
        slot: u8,
        length: u8,
    },
```

- Add `MergeKey::NotePattern(usize, u8)` and `MergeKey::NoteLength(usize, u8)`, with these `merge_key` arms:

```rust
        Edit::NotePattern { machine, slot, .. } => MergeKey::NotePattern(*machine, *slot),
        Edit::NoteLength { machine, slot, .. } => MergeKey::NoteLength(*machine, *slot),
```

- `apply_raw` arms:

```rust
            Edit::NotePattern {
                machine,
                slot,
                pattern,
            } => {
                let pats = p.note_patterns_mut(machine).expect("note machine");
                let old = std::mem::replace(&mut pats[slot as usize], *pattern);
                Edit::NotePattern {
                    machine,
                    slot,
                    pattern: Box::new(old),
                }
            }
            Edit::NoteLength {
                machine,
                slot,
                length,
            } => {
                let pat = &mut p.note_patterns_mut(machine).expect("note machine")[slot as usize];
                let old = std::mem::replace(&mut pat.length, length.clamp(1, STEPS as u8));
                Edit::NoteLength {
                    machine,
                    slot,
                    length: old,
                }
            }
```

- `commands_for` arm:

```rust
            Edit::NotePattern { machine, slot, .. } | Edit::NoteLength { machine, slot, .. } => {
                vec![Command::NotePattern {
                    machine: *machine as u8,
                    slot: *slot,
                    pattern: Box::new(
                        p.note_patterns(*machine).expect("note machine")[*slot as usize].clone(),
                    ),
                }]
            }
```

`ui/files.rs`:
- Add `Notes(NotePattern)` to `Clip`. Import `NotePattern` and `acid_engine::machine::{MachineKind, kind_of}`.
- Replace `copy_pattern`'s `self.view.clipboard = Some(if m < 2 {...})` with:

```rust
        self.view.clipboard = match kind_of(m) {
            Some(MachineKind::Synth(s)) => {
                Some(Clip::Synth(self.doc.project.synths[s].patterns[slot].clone()))
            }
            Some(MachineKind::Drum(k)) => {
                Some(Clip::Drum(self.doc.project.drums[k].patterns[slot].clone()))
            }
            Some(MachineKind::Notes(_)) => self
                .doc
                .project
                .note_patterns(m)
                .map(|p| Clip::Notes(p[slot].clone())),
            None => None,
        };
```

- Replace `paste_pattern`'s `let edit = match ...` with:

```rust
        let edit = match (&self.view.clipboard, kind_of(m)) {
            (None, _) => return,
            (Some(Clip::Synth(p)), Some(MachineKind::Synth(s))) => Edit::SynthPattern {
                synth: s,
                slot,
                pattern: Box::new(p.clone()),
            },
            (Some(Clip::Drum(p)), Some(MachineKind::Drum(k))) => Edit::DrumPattern {
                kit: k,
                slot,
                pattern: Box::new(p.clone()),
            },
            (Some(Clip::Notes(p)), Some(MachineKind::Notes(_))) => Edit::NotePattern {
                machine: m,
                slot,
                pattern: Box::new(p.clone()),
            },
            _ => {
                self.view.message = Some("The clipboard holds a different kind of pattern.".into());
                return;
            }
        };
```

- [ ] **Step 4: Run the whole suite**

Run: `cargo test --workspace`
Expected: all pass (the existing `pasting_drums_onto_a_synth_is_refused` still passes).

- [ ] **Step 5: Commit**

```bash
git add -A crates/app
git commit -m "feat(app): undoable note-pattern edits and FM/Sampler copy-paste

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Pure poly-roll editing (`poly_edit.rs`)

**Files:**
- Create: `crates/app/src/ui/poly_edit.rs`
- Modify: `crates/app/src/ui/mod.rs` (add `mod poly_edit;`)

**Interfaces:**
- Consumes: `Note`, `NotePattern`, `MAX_NOTES`, `TICKS_PER_STEP` (Task 2).
- Produces:

```rust
pub const ROWS: usize = 128;
pub enum Snap { Step, Half, Tick }      // 1/16, 1/32, 1/64; Default = Step
impl Snap { pub const ALL: [Snap; 3]; pub fn ticks(self) -> u16; pub fn label(self) -> &'static str; pub fn floor(self, tick: u16) -> u16; }
pub enum PolyDrag { Move { index: usize, grab: u16 }, Resize { index: usize }, Velocity }
pub fn note_at(p: &NotePattern, tick: u16, pitch: u8) -> Option<usize>;
pub fn add(p: &mut NotePattern, start: u16, len: u16, pitch: u8, vel: u8) -> Option<usize>;
pub fn delete(p: &mut NotePattern, index: usize) -> bool;
pub fn move_to(p: &mut NotePattern, index: usize, start: u16, pitch: u8) -> bool;
pub fn resize(p: &mut NotePattern, index: usize, len: u16) -> bool;
pub fn set_velocity_in(p: &mut NotePattern, from: u16, to: u16, vel: u8) -> bool;
pub struct PolyGeom { pub left: f32, pub top: f32, pub tick_w: f32, pub row_h: f32, pub ticks: u16 }
impl PolyGeom {
    pub fn cell_at(&self, x: f32, y: f32) -> Option<(u16, u8)>;
    pub fn cell_at_clamped(&self, x: f32, y: f32) -> (u16, u8);
    pub fn pitch_at(&self, y: f32) -> Option<u8>;
    pub fn row_top(&self, pitch: u8) -> f32;
    pub fn note_rect(&self, n: &Note) -> (f32, f32, f32, f32);
    pub fn near_right_edge(&self, n: &Note, x: f32) -> bool;
}
```

- [ ] **Step 1: Write the failing tests** (bottom of the new file; the file starts with only `use` lines and this module, so it fails to compile)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn pat() -> NotePattern {
        NotePattern::default()
    }

    #[test]
    fn snap_floors_to_its_grid() {
        assert_eq!(Snap::Step.floor(7), 4);
        assert_eq!(Snap::Half.floor(7), 6);
        assert_eq!(Snap::Tick.floor(7), 7);
        assert_eq!(Snap::ALL.map(Snap::ticks), [4, 2, 1]);
        assert_eq!(Snap::default(), Snap::Step);
    }

    #[test]
    fn chords_stack_and_same_pitch_overlaps_are_replaced() {
        let mut p = pat();
        for pitch in [60, 64, 67] {
            assert!(add(&mut p, 0, 4, pitch, 100).is_some());
        }
        assert_eq!(p.notes.len(), 3);
        let i = add(&mut p, 2, 4, 64, 80).unwrap();
        assert_eq!(p.notes.len(), 3, "the old 64 was replaced");
        assert_eq!(p.notes[i], Note { start: 2, len: 4, pitch: 64, vel: 80 });
        assert!(add(&mut p, 4, 4, 60, 100).is_some(), "touching, not overlapping");
        assert_eq!(p.notes.len(), 4);
    }

    #[test]
    fn add_refuses_past_the_end_and_over_the_cap() {
        let mut p = pat();
        assert_eq!(add(&mut p, p.ticks(), 1, 60, 100), None);
        for k in 0..MAX_NOTES {
            add(&mut p, (k % 64) as u16, 1, (k / 64) as u8, 100).unwrap();
        }
        assert_eq!(p.notes.len(), MAX_NOTES);
        assert_eq!(add(&mut p, 0, 1, 100, 100), None);
        assert!(add(&mut p, 0, 1, 0, 50).is_some(), "replacing at the cap is fine");
        assert_eq!(p.notes.len(), MAX_NOTES);
    }

    #[test]
    fn velocity_and_length_are_clamped() {
        let mut p = pat();
        let i = add(&mut p, 0, 0, 60, 0).unwrap();
        assert_eq!((p.notes[i].len, p.notes[i].vel), (1, 1));
        assert!(!resize(&mut p, i, 0), "already length 1");
        assert!(resize(&mut p, i, 9));
        assert_eq!(p.notes[i].len, 9);
        assert!(!resize(&mut p, 5, 3), "bad index");
    }

    #[test]
    fn note_at_finds_the_topmost_note_under_the_pointer() {
        let mut p = pat();
        add(&mut p, 0, 8, 60, 100);
        add(&mut p, 4, 2, 62, 100);
        assert_eq!(note_at(&p, 7, 60), Some(0));
        assert_eq!(note_at(&p, 8, 60), None);
        assert_eq!(note_at(&p, 5, 62), Some(1));
        assert_eq!(note_at(&p, 5, 61), None);
    }

    #[test]
    fn move_and_delete_keep_other_indices() {
        let mut p = pat();
        add(&mut p, 0, 4, 60, 100);
        add(&mut p, 8, 4, 62, 100);
        assert!(move_to(&mut p, 0, 200, 130));
        assert_eq!((p.notes[0].start, p.notes[0].pitch), (p.ticks() - 1, 127));
        assert!(!move_to(&mut p, 0, p.ticks() - 1, 127), "no change");
        assert!(delete(&mut p, 0));
        assert_eq!(p.notes[0].pitch, 62);
        assert!(!delete(&mut p, 3));
    }

    #[test]
    fn velocity_lane_sets_notes_starting_in_the_column() {
        let mut p = pat();
        add(&mut p, 4, 1, 60, 100);
        add(&mut p, 5, 1, 64, 100);
        add(&mut p, 8, 1, 67, 100);
        assert!(set_velocity_in(&mut p, 4, 8, 30));
        assert_eq!(p.notes.iter().map(|n| n.vel).collect::<Vec<_>>(), [30, 30, 100]);
        assert!(!set_velocity_in(&mut p, 4, 8, 30), "no change");
        set_velocity_in(&mut p, 8, 9, 0);
        assert_eq!(p.notes[2].vel, 1);
    }

    #[test]
    fn geometry_round_trips() {
        let g = PolyGeom { left: 30.0, top: 10.0, tick_w: 6.0, row_h: 8.0, ticks: 64 };
        assert_eq!(g.cell_at(30.0, 10.0), Some((0, 127)));
        assert_eq!(g.cell_at(30.0 + 6.0 * 5.5, 10.0 + 8.0 * 67.5), Some((5, 60)));
        assert_eq!(g.cell_at(29.0, 10.0), None, "key strip");
        assert_eq!(g.cell_at(30.0 + 6.0 * 64.0, 10.0), None, "past the end");
        assert_eq!(g.cell_at_clamped(0.0, 9999.0), (0, 0));
        assert_eq!(g.pitch_at(10.0 + 8.0 * 67.5), Some(60));
        let n = Note { start: 4, len: 70, pitch: 60, vel: 1 };
        let (x0, y0, x1, y1) = g.note_rect(&n);
        assert_eq!((x0, y0, y1), (54.0, g.row_top(60), g.row_top(60) + 8.0));
        assert_eq!(x1, 30.0 + 64.0 * 6.0, "drawn up to the pattern end");
        assert!(g.near_right_edge(&n, x1 - 2.0));
        assert!(!g.near_right_edge(&n, x0 + 2.0));
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p sidacid poly_edit`
Expected: compile errors.

- [ ] **Step 3: Implement** (above the tests)

```rust
//! Pure editing of note patterns for the poly piano roll, plus its
//! geometry. Notes stay in insertion order (the engine scans them all),
//! so an index stays valid for the whole of a drag.

use acid_engine::project::{MAX_NOTES, Note, NotePattern, TICKS_PER_STEP};

/// Every MIDI pitch, top row = 127.
pub const ROWS: usize = 128;
const EDGE_PX: f32 = 5.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Snap {
    #[default]
    Step,
    Half,
    Tick,
}

impl Snap {
    pub const ALL: [Snap; 3] = [Snap::Step, Snap::Half, Snap::Tick];

    pub fn ticks(self) -> u16 {
        match self {
            Snap::Step => TICKS_PER_STEP,
            Snap::Half => TICKS_PER_STEP / 2,
            Snap::Tick => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Snap::Step => "1/16",
            Snap::Half => "1/32",
            Snap::Tick => "1/64",
        }
    }

    pub fn floor(self, tick: u16) -> u16 {
        tick - tick % self.ticks()
    }
}

/// A mouse drag in progress in the roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolyDrag {
    /// `grab` = ticks between the note start and where it was grabbed.
    Move { index: usize, grab: u16 },
    Resize { index: usize },
    Velocity,
}

fn end(n: &Note) -> u16 {
    n.start.saturating_add(n.len)
}

/// The last-drawn (topmost) note at `pitch` covering `tick`.
pub fn note_at(p: &NotePattern, tick: u16, pitch: u8) -> Option<usize> {
    p.notes
        .iter()
        .rposition(|n| n.pitch == pitch && tick >= n.start && tick < end(n))
}

/// Adds a note, replacing notes of the same pitch it overlaps. Refused
/// (None) when it starts at or past the pattern end, or when the pattern
/// would exceed `MAX_NOTES`. Returns the new note's index.
pub fn add(p: &mut NotePattern, start: u16, len: u16, pitch: u8, vel: u8) -> Option<usize> {
    if start >= p.ticks() || pitch > 127 {
        return None;
    }
    let len = len.max(1);
    let stop = start.saturating_add(len);
    let overlaps = |n: &Note| n.pitch == pitch && n.start < stop && start < end(n);
    let replaced = p.notes.iter().filter(|n| overlaps(n)).count();
    if p.notes.len() - replaced >= MAX_NOTES {
        return None;
    }
    p.notes.retain(|n| !overlaps(n));
    p.notes.push(Note {
        start,
        len,
        pitch,
        vel: vel.clamp(1, 127),
    });
    Some(p.notes.len() - 1)
}

pub fn delete(p: &mut NotePattern, index: usize) -> bool {
    if index < p.notes.len() {
        p.notes.remove(index);
        true
    } else {
        false
    }
}

/// Moves a note, keeping its start inside the pattern and its pitch in MIDI range.
pub fn move_to(p: &mut NotePattern, index: usize, start: u16, pitch: u8) -> bool {
    let last = p.ticks() - 1;
    let Some(n) = p.notes.get_mut(index) else {
        return false;
    };
    let (start, pitch) = (start.min(last), pitch.min(127));
    if n.start == start && n.pitch == pitch {
        return false;
    }
    n.start = start;
    n.pitch = pitch;
    true
}

pub fn resize(p: &mut NotePattern, index: usize, len: u16) -> bool {
    let Some(n) = p.notes.get_mut(index) else {
        return false;
    };
    let len = len.max(1);
    if n.len == len {
        return false;
    }
    n.len = len;
    true
}

/// Sets the velocity of every note starting in `from..to`.
pub fn set_velocity_in(p: &mut NotePattern, from: u16, to: u16, vel: u8) -> bool {
    let vel = vel.clamp(1, 127);
    let mut changed = false;
    for n in p.notes.iter_mut().filter(|n| (from..to).contains(&n.start)) {
        changed |= n.vel != vel;
        n.vel = vel;
    }
    changed
}

/// Screen layout of the roll grid: one column per tick, one row per
/// pitch, pitch 127 on top. `left` is the grid's left edge (right of the
/// key strip).
#[derive(Clone, Copy, Debug)]
pub struct PolyGeom {
    pub left: f32,
    pub top: f32,
    pub tick_w: f32,
    pub row_h: f32,
    pub ticks: u16,
}

impl PolyGeom {
    pub fn pitch_at(&self, y: f32) -> Option<u8> {
        let r = ((y - self.top) / self.row_h).floor();
        (r >= 0.0 && (r as usize) < ROWS).then(|| (ROWS - 1 - r as usize) as u8)
    }

    pub fn cell_at(&self, x: f32, y: f32) -> Option<(u16, u8)> {
        let col = ((x - self.left) / self.tick_w).floor();
        if col < 0.0 || col as u16 >= self.ticks {
            return None;
        }
        Some((col as u16, self.pitch_at(y)?))
    }

    pub fn cell_at_clamped(&self, x: f32, y: f32) -> (u16, u8) {
        let col = ((x - self.left) / self.tick_w)
            .floor()
            .clamp(0.0, (self.ticks.max(1) - 1) as f32) as u16;
        let r = ((y - self.top) / self.row_h)
            .floor()
            .clamp(0.0, (ROWS - 1) as f32) as usize;
        (col, (ROWS - 1 - r) as u8)
    }

    pub fn row_top(&self, pitch: u8) -> f32 {
        self.top + (ROWS - 1 - pitch.min(127) as usize) as f32 * self.row_h
    }

    /// (x0, y0, x1, y1); notes running past the pattern end are drawn to it.
    pub fn note_rect(&self, n: &Note) -> (f32, f32, f32, f32) {
        let x0 = self.left + n.start as f32 * self.tick_w;
        let stop = end(n).min(self.ticks.max(n.start + 1));
        let x1 = self.left + stop as f32 * self.tick_w;
        let y0 = self.row_top(n.pitch);
        (x0, y0, x1, y0 + self.row_h)
    }

    pub fn near_right_edge(&self, n: &Note, x: f32) -> bool {
        let (_, _, x1, _) = self.note_rect(n);
        x >= x1 - EDGE_PX && x <= x1 + 1.0
    }
}
```

Add `mod poly_edit;` to `ui/mod.rs`, in alphabetical order. If clippy flags unused items before Task 9 uses them, add `#![allow(dead_code)]` at the top of `poly_edit.rs` and remove it in Task 9.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sidacid poly_edit`, then `cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A crates/app
git commit -m "feat(app): pure poly piano-roll editing and geometry

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Poly piano roll widget and the FM / Sampler panels

**Files:**
- Create: `crates/app/src/ui/polyroll.rs`, `crates/app/src/ui/note_panel.rs`
- Modify: `crates/app/src/ui/mod.rs` (modules, `ViewState`, `rack`, tests), `crates/app/src/ui/files.rs` (`load_document` resets)

**Interfaces:**
- Consumes: everything from `poly_edit` (Task 8); `Edit::NotePattern` and `Edit::NoteLength` (Task 7); `Command::NoteOn` and `NoteOff` (Task 5); `MachineId` (Task 2); `self.pattern_pads(ui, machine)`, `self.param_knob(ui, id, label)`, `self.send(cmds)` and `self.doc.apply(edit, merge)` (existing).
- Produces: `SidAcidApp::poly_roll(&mut self, ui, machine: usize)` and `SidAcidApp::note_panel(&mut self, ui, machine: MachineId)`.

New `ViewState` fields (all `Default`-constructible):

```rust
    /// Poly-roll drag per note machine (index 0 = FM, 1 = Sampler).
    pub poly_drag: [Option<poly_edit::PolyDrag>; 2],
    pub poly_snap: [poly_edit::Snap; 2],
    /// Length in ticks of the last note drawn or resized (0 = one step).
    pub poly_len: [u16; 2],
    /// The roll has been scrolled to its starting pitch once.
    pub poly_scrolled: [bool; 2],
    /// Note held by a click on the roll's key strip: (machine, pitch).
    pub audition: Option<(usize, u8)>,
```

- [ ] **Step 1: Write the failing tests** (in `ui/mod.rs` tests, next to `piano_roll_view_draws_headless`; mirror that test's setup)

```rust
    #[test]
    fn note_panels_and_poly_rolls_draw_headless() {
        use acid_engine::project::Note;
        let mut doc = Document::new(Project::default());
        doc.project.fm.patterns[0].notes.push(Note { start: 0, len: 8, pitch: 60, vel: 100 });
        doc.project.sampler.patterns[0].notes.push(Note { start: 4, len: 300, pitch: 36, vel: 20 });
        doc.project.sampler.patterns[0].length = 4;
        let mut app = SidAcidApp::headless(doc);
        run_frames(&mut app, 3);
        assert!(app.view.poly_scrolled.iter().all(|s| *s));
    }

    #[test]
    fn loading_a_document_resets_poly_roll_state() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.view.poly_drag = [Some(poly_edit::PolyDrag::Velocity); 2];
        app.view.poly_scrolled = [true; 2];
        app.view.audition = Some((4, 60));
        app.load_document(Document::new(Project::default()));
        assert_eq!(app.view.poly_drag, [None; 2]);
        assert_eq!(app.view.poly_scrolled, [false; 2]);
        assert_eq!(app.view.audition, None);
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p sidacid`
Expected: compile errors (fields missing).

- [ ] **Step 3: Add the `ViewState` fields and resets**

Add the fields listed above to `ViewState` in `ui/mod.rs`. In `files.rs` `load_document`, next to the existing resets:

```rust
        self.view.poly_drag = [None; 2];
        self.view.poly_scrolled = [false; 2];
        self.view.audition = None;
```

- [ ] **Step 4: Implement `polyroll.rs`**

```rust
//! Poly piano roll for the FM synth and sampler: one column per tick,
//! every MIDI pitch, a velocity lane underneath. Click adds a note (with
//! the last length used), drag the body to move, drag the right edge to
//! stretch, right-click deletes, chords stack freely. Clicking the key
//! strip auditions the pitch.

use super::poly_edit::{
    PolyDrag, PolyGeom, ROWS, add, delete, move_to, note_at, resize, set_velocity_in,
};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::machine::MachineId;
use acid_engine::project::TICKS_PER_STEP;
use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, pos2, vec2};

const TICK_W: f32 = 6.0;
const ROW_H: f32 = 8.0;
const KEY_W: f32 = 30.0;
const VEL_H: f32 = 32.0;
const VIEW_H: f32 = 260.0;
const NEW_VEL: u8 = 100;
/// Pitch the roll first scrolls to: C3 (MIDI 48) for FM, C2 for the sampler.
const START_PITCH: [u8; 2] = [48, 36];
const BLACK_KEYS: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

fn vel_from_y(y: f32, top: f32) -> u8 {
    ((1.0 - (y - top) / VEL_H) * 127.0).round().clamp(1.0, 127.0) as u8
}

impl SidAcidApp {
    pub(crate) fn poly_roll(&mut self, ui: &mut egui::Ui, machine: usize) {
        let n = machine - MachineId::Fm.index();
        let slot = self.doc.project.selected[machine];
        let Some(bank) = self.doc.project.note_patterns(machine) else {
            return;
        };
        let mut pattern = bank[slot as usize].clone();
        let ticks = pattern.ticks();
        let snap = self.view.poly_snap[n];
        let new_len = match self.view.poly_len[n] {
            0 => TICKS_PER_STEP,
            l => l,
        };
        let playhead = (self.status.playing && self.status.patterns[machine] == slot)
            .then_some(self.status.playhead[machine]);
        let width = KEY_W + TICK_W * ticks as f32;
        let mut changed = false;
        let mut finished = false;
        let mut key_down: Option<u8> = None;

        egui::ScrollArea::horizontal()
            .id_salt(("poly_h", machine))
            .show(ui, |ui| {
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(("poly_v", machine))
                    .max_height(VIEW_H);
                if !self.view.poly_scrolled[n] {
                    let y = (ROWS - 1 - START_PITCH[n] as usize) as f32 * ROW_H - VIEW_H / 2.0;
                    scroll = scroll.vertical_scroll_offset(y.max(0.0));
                    self.view.poly_scrolled[n] = true;
                }
                scroll.show(ui, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(
                        vec2(width, ROW_H * ROWS as f32),
                        Sense::click_and_drag(),
                    );
                    let geom = PolyGeom {
                        left: rect.left() + KEY_W,
                        top: rect.top(),
                        tick_w: TICK_W,
                        row_h: ROW_H,
                        ticks,
                    };
                    let painter = ui.painter_at(rect);
                    let held = self.view.audition.filter(|a| a.0 == machine).map(|a| a.1);
                    for pitch in 0..ROWS as u8 {
                        let y0 = geom.row_top(pitch);
                        let black = BLACK_KEYS[pitch as usize % 12];
                        let row = egui::Rect::from_min_max(
                            pos2(geom.left, y0),
                            pos2(rect.right(), y0 + ROW_H),
                        );
                        painter.rect_filled(row, 0.0, if black { theme::GRID_DARK } else { theme::GRID_BG });
                        let key = egui::Rect::from_min_max(
                            pos2(rect.left(), y0),
                            pos2(geom.left - 1.0, y0 + ROW_H - 1.0),
                        );
                        let key_colour = if held == Some(pitch) {
                            theme::ACID
                        } else if black {
                            theme::GRID_DARK
                        } else {
                            Color32::from_gray(190)
                        };
                        painter.rect_filled(key, 1.0, key_colour);
                        if pitch % 12 == 0 {
                            painter.text(
                                pos2(rect.left() + 2.0, y0 + ROW_H / 2.0),
                                Align2::LEFT_CENTER,
                                format!("C{}", pitch as i32 / 12 - 1),
                                FontId::monospace(8.0),
                                Color32::BLACK,
                            );
                        }
                    }
                    for step in 0..=ticks / TICKS_PER_STEP {
                        let x = geom.left + (step * TICKS_PER_STEP) as f32 * TICK_W;
                        let colour = if step % 4 == 0 { theme::KNOB_TRACK } else { theme::GRID_LINE };
                        painter.line_segment(
                            [pos2(x, rect.top()), pos2(x, rect.bottom())],
                            Stroke::new(1.0_f32, colour),
                        );
                    }
                    if let Some(step) = playhead {
                        let x = geom.left + (step as u16 * TICKS_PER_STEP) as f32 * TICK_W;
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                pos2(x, rect.top()),
                                pos2(x + TICKS_PER_STEP as f32 * TICK_W, rect.bottom()),
                            ),
                            0.0,
                            theme::ACID.gamma_multiply(0.15),
                        );
                    }
                    for note in pattern.notes.iter().filter(|n| n.start < ticks) {
                        let (x0, y0, x1, y1) = geom.note_rect(note);
                        let r = egui::Rect::from_min_max(pos2(x0 + 0.5, y0 + 0.5), pos2(x1 - 0.5, y1 - 0.5));
                        let shade = 0.35 + 0.65 * note.vel as f32 / 127.0;
                        painter.rect_filled(r, 2.0, theme::ACID.gamma_multiply(shade));
                    }

                    let pointer = resp.interact_pointer_pos();
                    if resp.is_pointer_button_down_on()
                        && let Some(p) = pointer
                        && p.x < geom.left
                    {
                        key_down = geom.pitch_at(p.y);
                    }
                    if resp.clicked()
                        && let Some(p) = pointer
                        && let Some((tick, pitch)) = geom.cell_at(p.x, p.y)
                        && note_at(&pattern, tick, pitch).is_none()
                    {
                        changed |= add(&mut pattern, snap.floor(tick), new_len, pitch, NEW_VEL).is_some();
                        finished = true;
                    }
                    if resp.secondary_clicked()
                        && let Some(p) = pointer
                        && let Some((tick, pitch)) = geom.cell_at(p.x, p.y)
                        && let Some(i) = note_at(&pattern, tick, pitch)
                    {
                        changed |= delete(&mut pattern, i);
                        finished = true;
                    }
                    if resp.drag_started()
                        && let Some(p) = pointer
                        && let Some((tick, pitch)) = geom.cell_at(p.x, p.y)
                    {
                        self.view.poly_drag[n] = match note_at(&pattern, tick, pitch) {
                            Some(i) if geom.near_right_edge(&pattern.notes[i], p.x) => {
                                Some(PolyDrag::Resize { index: i })
                            }
                            Some(i) => Some(PolyDrag::Move {
                                index: i,
                                grab: tick - pattern.notes[i].start,
                            }),
                            None => {
                                let added = add(&mut pattern, snap.floor(tick), snap.ticks(), pitch, NEW_VEL);
                                changed |= added.is_some();
                                added.map(|index| PolyDrag::Resize { index })
                            }
                        };
                    }
                    if resp.dragged()
                        && let Some(p) = pointer
                        && let Some(drag) = self.view.poly_drag[n]
                    {
                        let (tick, pitch) = geom.cell_at_clamped(p.x, p.y);
                        match drag {
                            PolyDrag::Resize { index } => {
                                if let Some(start) = pattern.notes.get(index).map(|n| n.start) {
                                    let len = (snap.floor(tick) + snap.ticks()).saturating_sub(start).max(1);
                                    if resize(&mut pattern, index, len) {
                                        self.view.poly_len[n] = len;
                                        changed = true;
                                    }
                                }
                            }
                            PolyDrag::Move { index, grab } => {
                                let start = snap.floor(tick.saturating_sub(grab));
                                changed |= move_to(&mut pattern, index, start, pitch);
                            }
                            PolyDrag::Velocity => {}
                        }
                    }
                    if resp.drag_stopped() && self.view.poly_drag[n] != Some(PolyDrag::Velocity) {
                        self.view.poly_drag[n] = None;
                        finished = true;
                    }
                });

                let (lane, lresp) = ui.allocate_exact_size(vec2(width, VEL_H), Sense::click_and_drag());
                let left = lane.left() + KEY_W;
                let painter = ui.painter_at(lane);
                painter.rect_filled(
                    egui::Rect::from_min_max(pos2(left, lane.top()), lane.right_bottom()),
                    0.0,
                    theme::GRID_DARK,
                );
                painter.text(
                    pos2(lane.left() + 2.0, lane.center().y),
                    Align2::LEFT_CENTER,
                    "Vel",
                    FontId::monospace(9.0),
                    ui.visuals().text_color(),
                );
                for note in pattern.notes.iter().filter(|n| n.start < ticks) {
                    let x = left + note.start as f32 * TICK_W;
                    let h = VEL_H * note.vel as f32 / 127.0;
                    painter.rect_filled(
                        egui::Rect::from_min_max(pos2(x, lane.bottom() - h), pos2(x + TICK_W - 1.0, lane.bottom())),
                        0.0,
                        theme::ACID,
                    );
                }
                if lresp.drag_started() {
                    self.view.poly_drag[n] = Some(PolyDrag::Velocity);
                }
                if (lresp.clicked() || lresp.dragged())
                    && let Some(p) = lresp.interact_pointer_pos()
                    && p.x >= left
                {
                    let tick = (((p.x - left) / TICK_W).floor() as u16).min(ticks - 1);
                    let from = snap.floor(tick);
                    changed |= set_velocity_in(&mut pattern, from, from + snap.ticks(), vel_from_y(p.y, lane.top()));
                    finished |= lresp.clicked();
                }
                if lresp.drag_stopped() {
                    self.view.poly_drag[n] = None;
                    finished = true;
                }
            });

        self.audition(machine, key_down);
        if changed {
            let dragging = self.view.poly_drag[n].is_some();
            let cmds = self.doc.apply(
                Edit::NotePattern {
                    machine,
                    slot,
                    pattern: Box::new(pattern),
                },
                dragging,
            );
            self.send(cmds);
            self.view.focus = machine;
        }
        if finished {
            self.doc.end_gesture();
        }
    }

    /// Follows the key-strip press: starts the pressed pitch, stops the old one.
    fn audition(&mut self, machine: usize, pitch: Option<u8>) {
        let now = pitch.map(|p| (machine, p));
        let before = self.view.audition;
        if before == now || (now.is_none() && before.is_some_and(|(m, _)| m != machine)) {
            return;
        }
        let mut cmds = Vec::new();
        if let Some((m, p)) = before {
            cmds.push(Command::NoteOff { machine: m as u8, pitch: p });
        }
        if let Some((m, p)) = now {
            cmds.push(Command::NoteOn { machine: m as u8, pitch: p, vel: NEW_VEL });
        }
        self.view.audition = now;
        self.send(cmds);
    }
}
```

- [ ] **Step 5: Implement `note_panel.rs`**

```rust
//! The FM and Sampler panels. Until their engines arrive each plays a
//! placeholder sine voice: volume knob, pattern pads, length, snap and
//! the poly piano roll.

use super::SidAcidApp;
use super::poly_edit::Snap;
use crate::document::Edit;
use acid_engine::machine::MachineId;
use acid_engine::params::{FmParam, ParamId, SamplerParam};
use acid_engine::project::PatternRef;
use eframe::egui;

impl SidAcidApp {
    pub(crate) fn note_panel(&mut self, ui: &mut egui::Ui, machine: MachineId) {
        let m = machine.index();
        let n = m - MachineId::Fm.index();
        let slot = self.doc.project.selected[m];
        let title = format!("{}   {}", machine.name().to_uppercase(), PatternRef(slot).label());
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt(("notes", m))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let volume = match machine {
                        MachineId::Fm => ParamId::Fm(FmParam::Volume),
                        _ => ParamId::Sampler(SamplerParam::Volume),
                    };
                    self.param_knob(ui, volume, "Volume");
                    ui.weak("Placeholder sine voice until the real engine arrives.");
                });
                egui::ScrollArea::horizontal()
                    .id_salt(("note_pads", m))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            self.pattern_pads(ui, m);
                            ui.separator();
                            let mut length = self
                                .doc
                                .project
                                .note_patterns(m)
                                .map_or(16, |p| p[slot as usize].length);
                            let r = ui.add(egui::DragValue::new(&mut length).range(1..=64).prefix("Len "));
                            if r.changed() {
                                let cmds = self.doc.apply(
                                    Edit::NoteLength {
                                        machine: m,
                                        slot,
                                        length,
                                    },
                                    true,
                                );
                                self.send(cmds);
                            }
                            if r.drag_stopped() || r.lost_focus() {
                                self.doc.end_gesture();
                            }
                            ui.separator();
                            ui.label("Snap");
                            for s in Snap::ALL {
                                if ui.selectable_label(self.view.poly_snap[n] == s, s.label()).clicked() {
                                    self.view.poly_snap[n] = s;
                                }
                            }
                        });
                    });
                self.poly_roll(ui, m);
            });
    }
}
```

- [ ] **Step 6: Add the modules and put the panels in the rack**

In `ui/mod.rs`, add `mod note_panel;` and `mod polyroll;` in alphabetical order, and remove any `#![allow(dead_code)]` added to `poly_edit.rs` in Task 8. In `rack`, after the kits loop:

```rust
        for machine in [acid_engine::machine::MachineId::Fm, acid_engine::machine::MachineId::Sampler] {
            self.note_panel(ui, machine);
            ui.add_space(6.0);
        }
```

- [ ] **Step 7: Run the tests**

Run: `cargo test --workspace`
Expected: all pass.

- [ ] **Step 8: Check it by hand**

Run: `cargo run --release -p sidacid`. Check each of these:
- the FM and SAMPLER panels appear below the 909
- clicking the grid adds notes
- dragging stretches and moves them
- right-click deletes
- a three-note chord plays as a chord on Play
- clicking the key strip sounds the pitch until you release it
- the velocity lane changes the note shade and loudness
- Ctrl+Z undoes a drag as one step

If no audio device is available, say so in the task report instead of skipping silently.

- [ ] **Step 9: Commit**

```bash
git add -A crates/app
git commit -m "feat(app): poly piano roll and FM/Sampler panels with audition

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Song editor for six machines, and the README

**Files:**
- Modify: `crates/app/src/ui/song_panel.rs`, `README.md`

**Interfaces:**
- Consumes: `MachineId::{ALL, name, transpose_index}` (Task 2); a 6-wide `Bar` (Task 2); `write_pattern_ref` accepting machines 4 and 5 (Task 3).

- [ ] **Step 1: Write the failing test** (in `ui/mod.rs` tests, next to `song_panel_draws_and_bar_ops_are_undoable`; mirror its setup)

```rust
    #[test]
    fn song_bars_carry_fm_and_sampler_patterns() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.doc.select_pattern(4, 3);
        app.doc.select_pattern(5, 17);
        app.song_action(song_panel::SongAction::Add, None);
        let bar = app.doc.project.song.bars[0];
        assert_eq!(bar.patterns[4], Some(acid_engine::project::PatternRef(3)));
        assert_eq!(bar.patterns[5], Some(acid_engine::project::PatternRef(17)));
        run_frames(&mut app, 2);
    }
```

(If `song_panel` or `SongAction` is not reachable from the test module, use the path the existing `song_toolbar_actions_edit_the_song_as_undo_steps` test uses.)

- [ ] **Step 2: Run it**

Run: `cargo test -p sidacid song_bars_carry`
Expected: it may already pass, because `bar_from_selected_patterns` maps every `selected` entry. Keep it as a regression test either way, and continue.

- [ ] **Step 3: Show six machines in the song editor**

In `song_panel.rs`:
- Delete `const MACHINES: [&str; 4]` and import `acid_engine::machine::{MACHINES, MachineId}`.
- In `song_timeline`, build the cell text for all machines and make the button taller:

```rust
                        let mut text = format!("{}", i + 1);
                        for m in 0..MACHINES {
                            text.push('\n');
                            text.push_str(&cell(m));
                        }
                        let mut rich = egui::RichText::new(text).monospace().size(10.0);
```

  and `ui.add_sized([44.0, 118.0], ...)`.
- In `bar_editor`, loop `for machine in MachineId::ALL { let m = machine.index(); ui.label(machine.name()); ... }`, and replace the `if m < 2 { ... bar.transpose[m] ... }` block with:

```rust
                if let Some(ti) = machine.transpose_index() {
                    let r = ui.add(
                        egui::DragValue::new(&mut bar.transpose[ti])
                            .range(-12..=12)
                            .prefix("Trans "),
                    );
                    if r.changed() {
                        changed = true;
                        merge = !recording && r.dragged();
                    }
                    finished |= !recording && (r.drag_stopped() || r.lost_focus());
                }
```

- [ ] **Step 4: Update the README**

In `README.md` "What's in the box", after the drum machines bullet, add:

```markdown
- **FM synth and Sampler slots** with a **polyphonic piano roll**:
  - Chords, 1/16–1/64 snap, a velocity lane, and click-to-audition keys.
  - For now each plays a simple sine voice. The 6-operator FM engine and the 16-slot sampler are coming next.
```

In the mixer bullet, change "Per-channel" to "Six channels with per-channel". In the song mode bullet, add that the FM synth has a song transpose alongside the two synths.

- [ ] **Step 5: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all tests pass, no new warnings.

- [ ] **Step 6: Commit**

```bash
git add -A crates/app README.md
git commit -m "feat(app): song editor shows FM and Sampler; README

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Spec coverage (Plan 4 scope)

| Spec item | Task |
|---|---|
| `MachineId` / `MACHINES = 6`, per-machine arrays | 2, 3 |
| 6 mixer strips, meters `[f32; 7]` | 1 |
| `Bar.transpose` for Synth1/Synth2/FM, sampler never transposed | 2, 4, 10 |
| `ParamId::Fm` / `ParamId::Sampler` through knob/automation/undo | 1, 5 (engine), 9 (knob) |
| `NotePattern`, `MAX_NOTES`, `TICKS_PER_STEP`, well-formedness | 2 |
| Sequencer note track, tick-accurate, swing, release on stop/switch/mute | 4, 5 |
| Live audition commands | 5, 9 |
| File format v2 + v1 migration | 2 |
| Copy/paste rules for note patterns | 7 |
| Counting-allocator test | 6 |
| Placeholder sine voices | 5 |
| Poly piano roll: add/move/resize/delete, chords, snap, velocity lane, audition, undo per drag | 8, 9 |
| Rack order, FM + Sampler panels, song lanes | 9, 10 |
| Sample handoff (`Arc<SampleData>`), FM engine, sampler engine | Plans 5–6 (out of scope here) |
