# Modular Rack (a): Engine & Project Model Implementation Plan (Plan 10)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fixed six-machine model in the engine and project format with a rack of up to 16 slots addressed by stable machine IDs, migrating old projects — while the app still shows today's six machines and the demo sounds bit-identical.

**Architecture:** Machines get a permanent `MachineUid`; `ParamId` becomes `Machine(uid, MachineParam) | Fx | Master`; `Project` v3 holds `machines: Vec<MachineEntry>`; song bars hold cells keyed by uid. The engine holds `Vec<Slot>` (capacity 16) in rack order, each slot owning its DSP, knobs, pattern bank, mixer strip and render buffer. Song bars are compiled to per-position arrays on the UI thread. Rack changes are swapped in with one `SetRack` command, built on the UI thread, with old slots returned for disposal. A golden-render test pins the audio across every task.

**Tech Stack:** Rust 2024 workspace (`crates/acid-engine`, `crates/app` = package `sidacid`), serde + ron 0.8, rtrb, egui 0.31.

**Spec:** `docs/superpowers/specs/2026-10-14-modular-rack-design.md` (sections 1, 2 and 5 "Plan (a)"; sections 3–4 are plans 10b/10c).

## Global Constraints

- Rack cap: `MAX_MACHINES = 16`. Machine types: 303 Synth, 808 Kit, 909 Kit, FM, Sampler.
- `MachineUid(u16)`; allocated from `Project::next_uid`; never reused; independent of rack position.
- Migrated / new-project rack: uids 1–6, types and names in order: Synth "Synth 1", Synth "Synth 2", 808 Kit "808", 909 Kit "909", FM "FM", Sampler "Sampler"; `next_uid = 7`.
- `PROJECT_VERSION = 3`; v1 → v2 → v3 and v2 → v3 load paths; newer → `TooNew`.
- No allocation, deallocation, locking or blocking inside `Engine::process` / `EngineHost::process` (counting-allocator test in `crates/acid-engine/tests/no_alloc.rs` must pass and gain coverage for `SetRack`, `Pattern`, `Song`).
- **Golden render must stay bit-identical in every task** (Task 1's test). Rendering order is rack order (= today's Synth1, Synth2, 808, 909, FM, Sampler) and mixer summation order must not change. If a task cannot keep the hashes, STOP and report — never update the golden constants.
- The app must look and behave as today at the end of every task (six machines, same panels); plan 10b adds rack UI.
- Tests: `cargo test --workspace`; lint: `cargo clippy --workspace --all-targets` with zero warnings; `cargo fmt --all`.
- Commit trailer: blank line then `Co-Authored-By: <model you are> <noreply@anthropic.com>`.
- Work on branch `rack-engine` (create from master at the start of Task 1).

## Spec amendment (decided while planning)

The spec's `MachineEntry` has a separate `channel` map and per-type param maps. This plan uses **one map per machine**, `MachineEntry::params: BTreeMap<MachineParam, f32>`, holding the machine's channel-strip params (`MachineParam::Channel(_)`) and its type params alike. Same data, one lookup path (`Project::param(ParamId)`); the type structs keep only patterns/sound data. Task 4 updates the spec text accordingly.

## File map

| File | Change |
|---|---|
| `crates/acid-engine/tests/golden.rs` (new) + `tests/fixtures/golden_v2.sidacid` (new) | Bit-exact render pins |
| `crates/acid-engine/src/machine.rs` | `MachineUid`, `MachineType`, `MAX_MACHINES`, default names; temporary `MachineId` bridge (removed in Task 9) |
| `crates/acid-engine/src/params.rs` | `ParamId`/`MachineParam`/`GlobalParam`; per-type local index spaces; retire `PARAM_COUNT` |
| `crates/acid-engine/src/migrate.rs` | Frozen v2 schema (`ProjectV2`, `LegacyParamId`, `BarV2`) + `upgrade()` to v3; v1 upgrades into `ProjectV2` |
| `crates/acid-engine/src/project.rs` | v3 `Project`, `MachineEntry`, `MachineBody`, `DrumMachine::kind`, `Bar`/`Cell`, accessors, validate |
| `crates/acid-engine/src/rack.rs` (new) | `Slot`, `MachineDsp`, `SlotPatterns`, `RackSwap`, `CompiledSong`/`CompiledBar` |
| `crates/acid-engine/src/engine.rs` | `slots: Vec<Slot>`; render loop; uid-addressed params/patterns/notes; `set_rack` |
| `crates/acid-engine/src/mixer.rs` | Strips live in slots; mixer mixes `&mut [Slot]` |
| `crates/acid-engine/src/sequencer.rs`, `song.rs` | Tracks `[Track; 16]` by position; `Banks` per slot; `SeqEvent` by slot index |
| `crates/acid-engine/src/host.rs` | Uid-addressed commands, `Pattern` unification, `SetRack`, `DisposeRack`, `Status` `[_; 16]` + `rack_gen` |
| `crates/acid-engine/src/render.rs`, `demo.rs`, `random.rs` | Follow the new model |
| `crates/acid-engine/tests/no_alloc.rs` | New coverage |
| `crates/app/src/**` | Follow every API change; still six machines; panels keyed by uid by Task 9 |

---

### Task 1: Golden render pins

**Files:**
- Create: `crates/acid-engine/tests/golden.rs`, `crates/acid-engine/tests/fixtures/golden_v2.sidacid`

**Interfaces:**
- Consumes (stable through the whole plan): `acid_engine::project::Project::from_ron(&str) -> Result<Project, ProjectError>`, `acid_engine::render::render_patterns(&Project, bars: u32) -> (Vec<f32>, Vec<f32>)`, `acid_engine::render::render_song(&Project, impl FnMut(f32)) -> (Vec<f32>, Vec<f32>)`.
- Produces: `cargo test -p acid-engine --test golden` — must pass unchanged after every later task.

- [ ] **Step 1: Branch** — `git checkout -b rack-engine`.

- [ ] **Step 2: Write a one-off fixture generator** as an `#[ignore]` test at the bottom of `crates/acid-engine/tests/golden.rs`:

```rust
/// One-off: writes the golden fixture from today's model. Deleted in Step 4.
#[test]
#[ignore]
fn write_golden_fixture() {
    use acid_engine::project::{AutoLane, Bar, Note, PatternRef};
    let mut p = acid_engine::demo::demo_project();
    // Every machine plays in a 4-bar looped song with transposes and mutes.
    p.fm.patterns[0].notes.push(Note { start: 0, len: 30, pitch: 60, vel: 100 });
    p.fm.patterns[0].notes.push(Note { start: 32, len: 30, pitch: 67, vel: 90 });
    p.sampler.patterns[0].notes.clear();
    let mut bars = Vec::new();
    for b in 0..4u8 {
        let mut bar = Bar::default();
        for m in 0..6 {
            bar.patterns[m] = Some(PatternRef(0));
        }
        bar.mutes[3] = b == 2;
        bar.transpose = [b as i8, -(b as i8), 2 * b as i8];
        bars.push(bar);
    }
    p.song.bars = bars;
    p.song.loop_range = None;
    p.song.automation.push(AutoLane {
        param: acid_engine::params::ParamId::Synth(0, acid_engine::params::SynthParam::Cutoff),
        points: [(0u32, 0.2f32), (32, 0.9), (63, 0.4)].into_iter().collect(),
    });
    p.song.automation.push(AutoLane {
        param: acid_engine::params::ParamId::Channel(2, acid_engine::params::ChannelParam::Level),
        points: [(0u32, 0.7f32), (40, 0.3)].into_iter().collect(),
    });
    std::fs::create_dir_all("tests/fixtures").unwrap();
    std::fs::write("tests/fixtures/golden_v2.sidacid", p.to_ron()).unwrap();
}
```

(Adjust field names only if the compiler disagrees — e.g. if `Note` fields differ, read `project.rs` `struct Note`. The sampler has no slots loaded in the demo; that is fine — sampler audio paths are covered by `no_alloc.rs` and unit tests.)

Run: `cd crates/acid-engine && cargo test --test golden write_golden_fixture -- --ignored` → creates the fixture. Confirm `head -c 200 tests/fixtures/golden_v2.sidacid` shows `version: 2`.

- [ ] **Step 3: Write the pin test** (top of `golden.rs`), first with placeholder constants `0`, run it, read the printed hashes, then paste them in:

```rust
//! Bit-exact audio pins: the rack refactor must not change one sample.
//! The fixture is a v2 project; every later model loads it via migration.

use acid_engine::project::Project;
use acid_engine::render::{render_patterns, render_song};

const PATTERN_HASH: u64 = 0; // filled in from the first run
const SONG_HASH: u64 = 0; // filled in from the first run

fn fnv(l: &[f32], r: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (a, b) in l.iter().zip(r) {
        for bits in [a.to_bits(), b.to_bits()] {
            for byte in bits.to_le_bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}

fn fixture() -> Project {
    let text = include_str!("fixtures/golden_v2.sidacid");
    Project::from_ron(text).expect("golden fixture loads")
}

#[test]
fn pattern_mode_render_is_bit_identical() {
    let (l, r) = render_patterns(&fixture(), 4);
    let h = fnv(&l, &r);
    assert_eq!(h, PATTERN_HASH, "pattern render changed: {h:#x} ({} frames)", l.len());
}

#[test]
fn song_mode_render_is_bit_identical() {
    let (l, r) = render_song(&fixture(), |_| {});
    let h = fnv(&l, &r);
    assert_eq!(h, SONG_HASH, "song render changed: {h:#x} ({} frames)", l.len());
}

#[test]
fn the_golden_render_is_not_silence() {
    let (l, _) = render_song(&fixture(), |_| {});
    assert!(l.iter().any(|s| s.abs() > 0.01));
}
```

Run: `cargo test -p acid-engine --test golden` → the two hash tests FAIL printing the real hashes; paste them into the constants; re-run → PASS. Run it twice more to confirm determinism.

- [ ] **Step 4: Delete the generator test** (it uses the old API) and commit:

```bash
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
git add crates/acid-engine/tests/golden.rs crates/acid-engine/tests/fixtures/golden_v2.sidacid
git commit -m "test(engine): bit-exact golden render pins for the rack refactor"
```

---

### Task 2: Machine identity types

**Files:**
- Modify: `crates/acid-engine/src/machine.rs`

**Interfaces:**
- Produces (permanent):
```rust
pub const MAX_MACHINES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MachineUid(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MachineType { Synth, Kit808, Kit909, Fm, Sampler }

impl MachineType {
    pub const ALL: [MachineType; 5];
    /// "303 Synth", "808 Kit", "909 Kit", "FM", "Sampler" (menus).
    pub fn label(self) -> &'static str;
    /// Name stem for default names: "Synth", "808", "909", "FM", "Sampler".
    pub fn stem(self) -> &'static str;
    pub fn transposes(self) -> bool; // Synth and Fm
}

/// Default name for a new machine of `ty`, given the names already in the
/// rack: the stem alone if unused ("808"), else stem + lowest free number
/// ≥ 2 ("808 2"); Synth always numbers ("Synth 1", "Synth 2", …).
pub fn default_name(ty: MachineType, existing: &[&str]) -> String;

/// Today's rack: (uid, type, name) for uids 1..=6.
pub fn standard_rack() -> [(MachineUid, MachineType, &'static str); 6];
```
- Produces (temporary bridge, removed in Task 9): `impl MachineId { pub fn uid(self) -> MachineUid; pub fn from_uid(MachineUid) -> Option<MachineId>; pub fn machine_type(self) -> MachineType }` — `Synth1 → 1 … Sampler → 6`.

- [ ] **Step 1: Failing tests** in `machine.rs` tests:

```rust
    #[test]
    fn default_names_number_from_two_except_synths() {
        assert_eq!(default_name(MachineType::Kit808, &[]), "808");
        assert_eq!(default_name(MachineType::Kit808, &["808"]), "808 2");
        assert_eq!(default_name(MachineType::Kit808, &["808", "808 2"]), "808 3");
        assert_eq!(default_name(MachineType::Kit808, &["808 2"]), "808");
        assert_eq!(default_name(MachineType::Synth, &[]), "Synth 1");
        assert_eq!(default_name(MachineType::Synth, &["Synth 1", "Synth 2"]), "Synth 3");
        assert_eq!(default_name(MachineType::Sampler, &["Sampler", "Bass"]), "Sampler 2");
    }

    #[test]
    fn the_standard_rack_is_todays_six() {
        let r = standard_rack();
        let names: Vec<_> = r.iter().map(|m| m.2).collect();
        assert_eq!(names, ["Synth 1", "Synth 2", "808", "909", "FM", "Sampler"]);
        let uids: Vec<_> = r.iter().map(|m| m.0 .0).collect();
        assert_eq!(uids, [1, 2, 3, 4, 5, 6]);
        for (m, (uid, ty, _)) in MachineId::ALL.iter().zip(r) {
            assert_eq!(m.uid(), uid);
            assert_eq!(m.machine_type(), ty);
            assert_eq!(MachineId::from_uid(uid), Some(*m));
        }
        assert_eq!(MachineId::from_uid(MachineUid(7)), None);
    }

    #[test]
    fn only_synths_and_fm_transpose() {
        let t: Vec<_> = MachineType::ALL.iter().map(|t| t.transposes()).collect();
        assert_eq!(t, [true, false, false, true, false]);
    }
```

- [ ] **Step 2:** `cargo test -p acid-engine machine::` → compile errors.
- [ ] **Step 3: Implement** (serde derive needs `use serde::{Deserialize, Serialize};`). `default_name`: for Synth, smallest n ≥ 1 with "Synth n" unused; others: stem if unused, else smallest n ≥ 2 with "stem n" unused.
- [ ] **Step 4:** tests pass; full workspace check; commit `feat(engine): machine uids, types and default names`.

---

### Task 3: Uid-scoped parameters

**Files:**
- Modify: `crates/acid-engine/src/params.rs`, `engine.rs`, `project.rs`, `migrate.rs`, `song.rs`, `sequencer.rs`, `random.rs`, `demo.rs`, `render.rs`, `tests/no_alloc.rs`; every `crates/app/src` use of `ParamId`.

**Interfaces:**
- Consumes: `MachineUid`, `MachineType`, `MachineId::{uid, from_uid}` (Task 2).
- Produces (permanent):
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MachineParam {
    Synth(SynthParam), Drum(u8, DrumParam), Kit(KitParam), Channel(ChannelParam),
    Fm(FmParam), FmOp(u8, OpParam), Sampler(SamplerParam), SamplerSlot(u8, SlotParam),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GlobalParam { Fx(FxParam), Master(MasterParam) }
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ParamId { Machine(MachineUid, MachineParam), Fx(FxParam), Master(MasterParam) }

impl MachineParam {
    pub fn info(self) -> ParamInfo;                 // today's info() arms, moved
    /// Valid on a machine of `ty` (Channel on all; Synth on Synth; Drum/Kit on kits; …;
    /// Drum inst < 8, FmOp op < 6, SamplerSlot slot < 16).
    pub fn fits(self, ty: MachineType) -> bool;
    /// Position in `MachineParam::all_for(ty)`; None if `!fits(ty)`.
    pub fn index_for(self, ty: MachineType) -> Option<usize>;
    pub fn all_for(ty: MachineType) -> Vec<MachineParam>; // Channel params first, then the type's
    pub fn count_for(ty: MachineType) -> usize;
}
impl GlobalParam {
    pub fn info(self) -> ParamInfo;
    pub fn index(self) -> usize;           // position in all()
    pub fn all() -> Vec<GlobalParam>;      // Fx params then Master params
    pub const COUNT: usize;
}
impl ParamId {
    pub fn info(self) -> ParamInfo;        // delegates
    pub fn global(self) -> Option<GlobalParam>;
}
```
- Removed: `ParamId::{Synth, Drum, Kit, Channel, Fm, Sampler, FmOp, SamplerSlot}` variants, `PARAM_COUNT`, `ParamId::index/all/is_valid`, `SYNTHS`, `KITS`, `CHANNELS` stay only if still used by engine internals (they may stay until Task 5/6; mark nothing new depending on them).
- Migration (permanent): `migrate::LegacyParamId` = a frozen copy of today's `ParamId` enum (same variant names and order, so v1/v2 RON parses unchanged) with `fn upgrade(self) -> ParamId`: `Synth(s,p) → Machine(uid(s+1), Synth(p))`; `Drum(k,i,p) → Machine(uid(k+3), Drum(i,p))`; `Kit(k,p) → Machine(uid(k+3), Kit(p))`; `Channel(c,p) → Machine(uid(c+1), Channel(p))`; `Fm(p) → Machine(uid(5), Fm(p))`; `FmOp(o,p) → Machine(uid(5), FmOp(o,p))`; `Sampler(p) → Machine(uid(6), Sampler(p))`; `SamplerSlot(s,p) → Machine(uid(6), SamplerSlot(s,p))`; `Fx/Master` unchanged.

**Approach (interim, until Task 5):** the engine keeps its flat `knobs/base/touched` arrays. Add a private bridge in `engine.rs`:

```rust
/// Interim (Tasks 3–4): flat position of a param in the fixed six-machine
/// layout — the same order the old `ParamId::all()` used, so knob storage
/// and automation behave exactly as before. Replaced by per-slot storage in Task 5.
fn flat_index(id: ParamId) -> Option<usize>
```

Implement it by mapping a uid-scoped id back through `MachineId::from_uid` to the old block layout (synths, kits, channels, fx, master, fm, sampler, fm ops, slots) using the old block sizes. Also provide `fn flat_ids() -> Vec<ParamId>` in the same order (replacement for the old `ParamId::all()`), and `const FLAT_COUNT: usize = 589;` with a test `flat_ids().len() == FLAT_COUNT` and `flat_index(flat_ids()[i]) == Some(i)` for all i. **Keeping this order is what keeps the golden hashes** (param application order in `load_project` must be the old order).

Project on-disk: `params: BTreeMap<ParamId, f32>` now serialises new-style keys. Bump `PROJECT_VERSION` to 3 now. `from_ron` dispatch: `0|1` → `ProjectV1` (its `params` typed `BTreeMap<LegacyParamId, f32>`, automation lanes typed with a legacy lane struct) → `.upgrade()`; `2` → a frozen `migrate::ProjectV2` (today's `Project` shape with `LegacyParamId` keys, `LegacyAutoLane { param: LegacyParamId, points }`) → `.upgrade()`; `3` → `Project`. (Task 4 replaces v3's shape again; the branch never ships the interim v3.)

App: mechanical rewrite of every `ParamId::Synth(s, p)` → `ParamId::Machine(MachineId::from_index(s).uid()…)`. Add these helpers in `params.rs` to keep call sites short (temporary, deleted in Task 9 together with `MachineId`):

```rust
pub fn synth_param(s: usize, p: SynthParam) -> ParamId;           // uid s+1
pub fn drum_param(k: usize, i: u8, p: DrumParam) -> ParamId;     // uid k+3
pub fn kit_param(k: usize, p: KitParam) -> ParamId;
pub fn channel_param(m: usize, p: ChannelParam) -> ParamId;      // uid m+1
pub fn fm_param(p: FmParam) -> ParamId;                          // uid 5
pub fn fm_op_param(op: u8, p: OpParam) -> ParamId;
pub fn sampler_param(p: SamplerParam) -> ParamId;                // uid 6
pub fn slot_param(slot: u8, p: SlotParam) -> ParamId;
```

Code that pattern-matches on old variants (`match id { ParamId::Synth(s, p) => … }` in engine `apply_param`, mixer routing, app `params_ui.rs`) matches `ParamId::Machine(uid, MachineParam::Synth(p))` and resolves the machine with `MachineId::from_uid(uid)`.

- [ ] **Step 1: Failing tests** in `params.rs`:

```rust
    #[test]
    fn per_type_index_spaces_match_all_for() {
        for ty in MachineType::ALL {
            let all = MachineParam::all_for(ty);
            assert_eq!(all.len(), MachineParam::count_for(ty));
            for (i, p) in all.iter().enumerate() {
                assert!(p.fits(ty));
                assert_eq!(p.index_for(ty), Some(i), "{ty:?} {p:?}");
            }
        }
        assert_eq!(MachineParam::Fm(FmParam::Volume).index_for(MachineType::Synth), None);
        assert_eq!(MachineParam::Drum(8, DrumParam::Tune).index_for(MachineType::Kit808), None);
        assert!(MachineParam::Channel(ChannelParam::Level).fits(MachineType::Sampler));
    }

    #[test]
    fn global_params_index_densely() {
        for (i, g) in GlobalParam::all().into_iter().enumerate() {
            assert_eq!(g.index(), i);
        }
        assert_eq!(GlobalParam::all().len(), GlobalParam::COUNT);
    }

    #[test]
    fn per_type_counts_cover_the_old_flat_table() {
        // Old PARAM_COUNT = 589: 2 synths + 2 kits + 6 channels + fx + master + fm + ops + slots.
        let ch = ChannelParam::ALL.len();
        let c = |t| MachineParam::count_for(t) - ch;
        let total = 2 * c(MachineType::Synth) + c(MachineType::Kit808) + c(MachineType::Kit909)
            + c(MachineType::Fm) + c(MachineType::Sampler) + 6 * ch + GlobalParam::COUNT;
        assert_eq!(total, 589);
    }
```

In `migrate.rs` tests:

```rust
    #[test]
    fn legacy_params_map_to_the_standard_uids() {
        use crate::machine::MachineUid as U;
        use crate::params::*;
        let m = |u, p| ParamId::Machine(U(u), p);
        assert_eq!(LegacyParamId::Synth(1, SynthParam::Cutoff).upgrade(), m(2, MachineParam::Synth(SynthParam::Cutoff)));
        assert_eq!(LegacyParamId::Drum(1, 3, DrumParam::Tone).upgrade(), m(4, MachineParam::Drum(3, DrumParam::Tone)));
        assert_eq!(LegacyParamId::Kit(0, KitParam::Accent).upgrade(), m(3, MachineParam::Kit(KitParam::Accent)));
        assert_eq!(LegacyParamId::Channel(5, ChannelParam::Pan).upgrade(), m(6, MachineParam::Channel(ChannelParam::Pan)));
        assert_eq!(LegacyParamId::FmOp(2, OpParam::Level).upgrade(), m(5, MachineParam::FmOp(2, OpParam::Level)));
        assert_eq!(LegacyParamId::SamplerSlot(9, SlotParam::Tune).upgrade(), m(6, MachineParam::SamplerSlot(9, SlotParam::Tune)));
        assert_eq!(LegacyParamId::Fx(FxParam::DelayMs).upgrade(), ParamId::Fx(FxParam::DelayMs));
    }
```

In `engine.rs` tests: the `flat_ids`/`flat_index` round trip described above.

- [ ] **Step 2:** run → compile errors.
- [ ] **Step 3: Implement** params types; `LegacyParamId`, `LegacyAutoLane`, `ProjectV2` in `migrate.rs` (copy today's `Project`/`Song`/`Bar` shapes as `ProjectV2`/`SongV2` with legacy params); make `ProjectV1::upgrade` return `ProjectV2` and add `ProjectV2::upgrade() -> Project`; engine bridge; update every caller (engine, mixer routing, sequencer automation, song helpers, random, demo, render, no_alloc test, app). Existing tests that construct old variants are rewritten with the helper fns.
- [ ] **Step 4:** `cargo test --workspace` — **the golden test must pass unchanged**; existing v1/v2 migration tests pass; fmt; clippy zero warnings. Launch check: `cargo build -p sidacid && timeout 4 target/debug/sidacid; echo exit=$?` (124 = ok).
- [ ] **Step 5:** commit `refactor(engine,app): uid-scoped ParamId; v2 projects migrate their params`.

---

### Task 4: Project v3 model

**Files:**
- Modify: `crates/acid-engine/src/project.rs`, `migrate.rs`, `song.rs`, `engine.rs` (`load_project`), `sequencer.rs` (bar reading), `render.rs`, `demo.rs`, `random.rs`, `tests/no_alloc.rs`, `crates/app/src/**` (all `project.synths/drums/fm/sampler/params/selected` and `Bar` field uses); `docs/superpowers/specs/2026-10-14-modular-rack-design.md` (amendment text).

**Interfaces:**
- Consumes: Tasks 2–3.
- Produces (permanent):
```rust
pub struct Project {
    pub version: u32, pub name: String, pub bpm: f32, pub swing: f32,
    pub machines: Vec<MachineEntry>,
    pub next_uid: u16,
    pub globals: BTreeMap<GlobalParam, f32>,
    pub song: Song,
}
pub struct MachineEntry {
    pub uid: MachineUid,
    pub name: String,
    pub selected: u8,
    pub params: BTreeMap<MachineParam, f32>,
    pub body: MachineBody,
}
pub enum MachineBody { Synth(SynthMachine), Kit(DrumMachine), Fm(FmMachine), Sampler(SamplerMachine) }
pub struct DrumMachine { pub kind: KitKind, pub patterns: Vec<DrumPattern> } // kind: #[serde] required
pub struct Cell { pub pattern: Option<PatternRef>, pub mute: bool, pub transpose: i8 } // Default, Copy
pub struct Bar { pub cells: Vec<(MachineUid, Cell)> }          // Clone (no longer Copy)

impl MachineEntry {
    pub fn new(uid: MachineUid, ty: MachineType, name: String) -> Self; // default params for ty
    pub fn machine_type(&self) -> MachineType;
}
impl MachineBody { pub fn machine_type(&self) -> MachineType; pub fn new(ty: MachineType) -> Self; }
impl Bar {
    pub fn cell(&self, uid: MachineUid) -> Cell;                 // default if absent
    pub fn cell_mut(&mut self, uid: MachineUid) -> &mut Cell;    // inserts default
}
impl Project {
    pub fn machine(&self, uid: MachineUid) -> Option<&MachineEntry>;
    pub fn machine_mut(&mut self, uid: MachineUid) -> Option<&mut MachineEntry>;
    pub fn position(&self, uid: MachineUid) -> Option<usize>;
    pub fn synth(&self, uid) -> Option<&SynthMachine>;   pub fn synth_mut(...)
    pub fn kit(&self, uid) -> Option<&DrumMachine>;      pub fn kit_mut(...)
    pub fn fm(&self, uid) -> Option<&FmMachine>;         pub fn fm_mut(...)
    pub fn sampler(&self, uid) -> Option<&SamplerMachine>; pub fn sampler_mut(...)
    pub fn note_patterns(&self, uid) -> Option<&Vec<NotePattern>>; // FM or Sampler
    pub fn note_patterns_mut(&mut self, uid) -> Option<&mut Vec<NotePattern>>;
    pub fn param(&self, id: ParamId) -> f32;               // entry params / globals / info default
    pub fn set_param(&mut self, id: ParamId, v: f32);      // no-op for unknown uid or non-fitting param
}
```
- `Project::default()` = `standard_rack()` machines with default bodies (`Kit` entries get `KitKind::K808`/`K909`), default params, `next_uid: 7`, default globals.
- `validate` (v3) rejects, with `ProjectError::Invalid(msg)`: `machines.len() > MAX_MACHINES`; duplicate uids; `next_uid <= max uid`; a bar cell or automation lane naming a uid not in the rack; an automation `Machine(uid, p)` whose `p` doesn't fit the machine's type; a param key that doesn't fit its machine; plus every existing per-machine check (pattern counts, well-formedness, sampler slot count/memory against `MEMORY_LIMIT` — plan 10c changes the limit). **Interim rule (removed in Task 5):** the machine types in order must equal the standard rack's types — message `"rack layout not supported yet"`.
- Migration: `ProjectV2::upgrade()` builds the six entries (uids 1–6, standard names), moves each machine's patterns/sound into its body (`drums[0]` → Kit K808, `drums[1]` → Kit K909), sorts legacy params into entry params / globals via `LegacyParamId::upgrade()`, `selected[m]` → entry m, and each `BarV2 { patterns, mutes, transpose }` → `Bar { cells }` with one cell per machine: `pattern = patterns[m]`, `mute = mutes[m]`, `transpose = transpose[ti]` for machines with `MachineId::ALL[m].transpose_index() == Some(ti)`, else 0. Automation lanes upgrade their `ParamId`.
- Engine and app read the project via the uid accessors with `MachineId::X.uid()` (the six still exist). `song.rs` helpers that took `bar.patterns[m]` use `bar.cell(MachineId::from_index(m).uid())`. `Banks::of(&Project)` builds from the accessors.

- [ ] **Step 1: Failing tests** (project.rs / migrate.rs):

```rust
    #[test]
    fn a_v2_project_upgrades_to_the_standard_six() {
        let v2 = include_str!("../tests/fixtures/golden_v2.sidacid");
        let p = Project::from_ron(v2).unwrap();
        assert_eq!(p.version, PROJECT_VERSION);
        let names: Vec<_> = p.machines.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Synth 1", "Synth 2", "808", "909", "FM", "Sampler"]);
        let uids: Vec<_> = p.machines.iter().map(|m| m.uid.0).collect();
        assert_eq!(uids, [1, 2, 3, 4, 5, 6]);
        assert_eq!(p.next_uid, 7);
        assert!(matches!(&p.machines[2].body, MachineBody::Kit(k) if k.kind == KitKind::K808));
        assert!(matches!(&p.machines[3].body, MachineBody::Kit(k) if k.kind == KitKind::K909));
        // The fixture's bar 2 mutes the 909 and transposes Synth 2 by -2, FM by 4.
        let bar = &p.song.bars[2];
        assert!(bar.cell(MachineUid(4)).mute);
        assert_eq!(bar.cell(MachineUid(2)).transpose, -2);
        assert_eq!(bar.cell(MachineUid(5)).transpose, 4);
        assert_eq!(bar.cell(MachineUid(3)).transpose, 0);
        p.validate().unwrap();
    }

    #[test]
    fn v3_round_trips() {
        let p = crate::demo::demo_project();
        assert_eq!(Project::from_ron(&p.to_ron()).unwrap(), p);
    }

    #[test]
    fn validate_rejects_broken_racks() {
        let ok = Project::default();
        let mut p = ok.clone();
        p.machines[1].uid = p.machines[0].uid;
        assert!(p.validate().is_err(), "duplicate uid");
        let mut p = ok.clone();
        p.next_uid = 6;
        assert!(p.validate().is_err(), "next_uid not above every uid");
        let mut p = ok.clone();
        p.song.bars.push(Bar { cells: vec![(MachineUid(42), Cell::default())] });
        assert!(p.validate().is_err(), "cell for unknown uid");
        let mut p = ok.clone();
        p.song.automation.push(AutoLane {
            param: ParamId::Machine(MachineUid(1), MachineParam::Fm(FmParam::Volume)),
            points: Default::default(),
        });
        assert!(p.validate().is_err(), "FM param on a synth");
        let mut p = ok.clone();
        p.machines[0].params.insert(MachineParam::Kit(KitParam::Volume), 0.5);
        assert!(p.validate().is_err(), "kit param on a synth");
    }

    #[test]
    fn project_params_resolve_through_entries_and_globals() {
        let mut p = Project::default();
        let id = ParamId::Machine(MachineUid(3), MachineParam::Kit(KitParam::Accent));
        p.set_param(id, 0.25);
        assert_eq!(p.param(id), 0.25);
        p.set_param(ParamId::Master(MasterParam::Level), 0.1);
        assert_eq!(p.param(ParamId::Master(MasterParam::Level)), 0.1);
        let unknown = ParamId::Machine(MachineUid(99), MachineParam::Synth(SynthParam::Tune));
        p.set_param(unknown, 0.9);
        assert_eq!(p.param(unknown), MachineParam::Synth(SynthParam::Tune).info().default);
    }
```

Keep all existing v1 migration tests passing (v1 → v2 → v3).

- [ ] **Step 2:** run → compile errors.
- [ ] **Step 3: Implement** the model, migration, accessors, validate; update every engine/app/test caller (the app keeps addressing the six through `MachineId::X.uid()` + accessors). Spec amendment: replace the `channel` field line in the spec's `MachineEntry` block with `params: BTreeMap<MachineParam, f32>, // channel strip + type params` and adjust the sentence after it.
- [ ] **Step 4:** full checks; **golden passes unchanged**; launch check (exit 124).
- [ ] **Step 5:** commit `feat(engine): project v3 — machine entries, uid-keyed bars, migration from v2`.

---

### Task 5: Engine slots

**Files:**
- Create: `crates/acid-engine/src/rack.rs` (+ `pub mod rack;` in `lib.rs`)
- Modify: `engine.rs`, `mixer.rs`, `project.rs` (drop the interim layout rule), `host.rs` (only as needed), tests.

**Interfaces:**
- Consumes: Tasks 2–4.
- Produces:
```rust
// rack.rs
pub enum MachineDsp { Synth(BassSynth), Kit(DrumKit), Fm(Box<FmSynth>), Sampler(Box<Sampler>) }
pub enum SlotPatterns { Synth(Vec<SynthPattern>), Drum(Vec<DrumPattern>), Notes(Vec<NotePattern>) }
pub struct Slot {
    pub uid: MachineUid,
    pub ty: MachineType,
    pub dsp: MachineDsp,
    pub knobs: Box<[f32]>, pub base: Box<[f32]>, pub touched: Box<[bool]>, // len count_for(ty)
    pub patterns: SlotPatterns,
    pub strip: ChannelStrip,
    pub buf: StereoBuf,
}
impl Slot {
    /// Builds a slot for `entry` (allocates: UI thread / construction only).
    pub fn from_entry(entry: &MachineEntry, sample_rate: f32) -> Slot;
    pub fn render(&mut self, span: Range<usize>);   // dsp → buf
}
```
- `Engine { slots: Vec<Slot> /* capacity MAX_MACHINES */, mixer: Mixer /* shared sends + master only */, global_knobs/base/touched: [f32; GlobalParam::COUNT]…, … }`.
- `Engine::from_project(sample_rate, &Project)` builds one slot per machine in rack order (any layout ≤ 16); `Engine::new(sr)` = `from_project(sr, &Project::default())`.
- Param path: `set_param(ParamId::Machine(uid, p))` → find slot by uid (linear) → `p.index_for(slot.ty)` → that slot's knobs/base → apply to that slot's DSP or strip. Globals → global arrays → mixer. Automation/touch/restore follow the same path. The Task 3 `flat_index` bridge is deleted.
- `Mixer::process(&mut self, slots: &mut [Slot], n, out_l, out_r)`: walks slots in order, applying each slot's `strip` to its `buf` (same per-strip DSP as today), solo across slots, summing to master and sends **in the same order and arithmetic as today** (golden!). Meters: `[f32; MAX_MACHINES + 1]` by position (unused positions 0), master last.
- `load_project` order: apply params in the old order to keep golden (for each slot in rack order: channel params are applied when? — read today's `ParamId::all()` order: synths, kits, channels, fx, master, fm, sampler, fm ops, slots. Applying a value is idempotent per param, so order only matters if applying one param reads another; check `apply_param` for cross-param reads (e.g. FM op levels, delay sync) and keep any dependent pair in the old relative order).
- `project.rs`: remove the interim layout rule; `validate` allows any 0..=16 machines.
- Interim for the sequencer (until Task 6): the sequencer still has six tracks keyed by the standard layout; until Task 6 the engine only sequences when the rack is the standard six (`self.standard_layout: bool`), otherwise it renders slots but plays no patterns. Unit-test both.

- [ ] **Step 1: Failing tests** (engine.rs tests):

```rust
    #[test]
    fn any_rack_layout_builds_and_renders() {
        let mut p = Project::default();
        p.machines.swap(0, 3); // 909 first
        let e = MachineEntry::new(MachineUid(7), MachineType::Sampler, "Sampler 2".into());
        p.machines.push(e);
        p.next_uid = 8;
        p.validate().unwrap();
        let mut eng = Engine::from_project(48000.0, &p);
        assert_eq!(eng.slot_uids(), vec![4, 2, 3, 1, 5, 6, 7].into_iter().map(MachineUid).collect::<Vec<_>>());
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        eng.process(&mut l, &mut r);
    }

    #[test]
    fn params_reach_the_slot_with_that_uid() {
        let mut eng = Engine::new(48000.0);
        let id = ParamId::Machine(MachineUid(2), MachineParam::Synth(SynthParam::Cutoff));
        eng.set_param(id, 0.123);
        assert_eq!(eng.knob(id), Some(0.123));
        let other = ParamId::Machine(MachineUid(1), MachineParam::Synth(SynthParam::Cutoff));
        assert_ne!(eng.knob(other), Some(0.123));
        assert_eq!(eng.knob(ParamId::Machine(MachineUid(99), MachineParam::Synth(SynthParam::Cutoff))), None);
        // A param that doesn't fit the machine type is ignored.
        eng.set_param(ParamId::Machine(MachineUid(2), MachineParam::Fm(FmParam::Volume)), 0.5);
    }

    #[test]
    fn sixteen_machines_fit_without_reallocating() {
        let mut p = Project::default();
        for u in 7..=16u16 {
            p.machines.push(MachineEntry::new(MachineUid(u), MachineType::Synth, format!("Synth {u}")));
        }
        p.next_uid = 17;
        p.validate().unwrap();
        let eng = Engine::from_project(48000.0, &p);
        assert_eq!(eng.slot_capacity(), MAX_MACHINES);
        let mut too_many = p.clone();
        too_many.machines.push(MachineEntry::new(MachineUid(17), MachineType::Fm, "FM 2".into()));
        too_many.next_uid = 18;
        assert!(too_many.validate().is_err());
    }
```

(`Engine::slot_uids() -> Vec<MachineUid>`, `Engine::knob(ParamId) -> Option<f32>`, `Engine::slot_capacity() -> usize` are small test-facing accessors — `#[cfg(test)]`-free is fine; keep them `pub`.)

- [ ] **Step 2:** run → fail.
- [ ] **Step 3: Implement** `rack.rs`, engine and mixer changes; port existing engine/mixer tests.
- [ ] **Step 4:** full checks; **golden unchanged**; no_alloc passes; launch check.
- [ ] **Step 5:** commit `feat(engine): rack of up to 16 slots, each owning its DSP, knobs, patterns and strip`.

---

### Task 6: Sequencer and song by slot position

**Files:**
- Modify: `sequencer.rs`, `song.rs`, `rack.rs`, `engine.rs`, `host.rs` (`Status`), `render.rs`; app status consumers (`ui/mod.rs`, panels reading `status.playhead/patterns/queued/meters`).

**Interfaces:**
- Produces:
```rust
// rack.rs
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompiledBar { pub patterns: [Option<PatternRef>; MAX_MACHINES], pub mutes: [bool; MAX_MACHINES], pub transpose: [i8; MAX_MACHINES] }
pub struct CompiledSong { pub bars: Vec<CompiledBar>, pub loop_range: Option<(u16, u16)>, pub automation: Vec<AutoLane> }
impl CompiledSong {
    /// Resolves uid cells to rack positions (UI thread; allocates).
    pub fn compile(song: &Song, rack: &[MachineUid]) -> CompiledSong;
}
// song.rs
#[derive(Clone, Copy)]
pub enum Bank<'a> { Synth(&'a [SynthPattern]), Drum(&'a [DrumPattern]), Notes(&'a [NotePattern]) }
#[derive(Clone, Copy)]
pub struct Banks<'a> { pub slots: [Option<Bank<'a>>; MAX_MACHINES] }
impl<'a> Banks<'a> {
    pub fn of_slots(slots: &'a [Slot]) -> Self;
    pub fn of(p: &'a Project) -> Self;          // rack order
    pub fn bank_len(&self, pos: usize) -> usize;
    pub fn length(&self, pos: usize, slot: u8) -> Option<usize>;
}
// sequencer.rs
pub enum SeqEvent { Note { slot: usize, midi, accent, legato }, NoteOff { slot: usize }, Drum { slot: usize, inst, accent },
                    PolyOn { slot: usize, pitch, vel }, PolyOff { slot: usize, pitch }, Param{..}, ParamRestore{..}, Automation{..}, AutomationReset{..} }
// Sequencer: tracks: [Track; MAX_MACHINES], chosen: [u8; MAX_MACHINES]; a track's kind comes from banks.slots[pos].
// host.rs
pub struct Status { pub playhead: [usize; MAX_MACHINES], pub patterns: [u8; MAX_MACHINES], pub queued: [Option<u8>; MAX_MACHINES],
                    pub meters: [f32; MAX_MACHINES + 1], pub rack_gen: u32, /* other fields unchanged */ }
```
- `Command::Song(Box<CompiledSong>)` (was `Box<Song>`); the app compiles with `CompiledSong::compile(&project.song, &uids_in_rack_order)` wherever it sends a song today. `Event::DisposeSong` carries `Box<CompiledSong>`.
- Transpose applies only on slots whose type `transposes()`; the `TRANSPOSED`/`NOTE_MACHINES`/`FIRST_NOTE_MACHINE` constants and the `[Track; 6]` assumptions are deleted. `rack_gen` stays 0 until Task 8.
- Remove Task 5's `standard_layout` interim: any rack sequences.
- Offline `render.rs` uses the same compiled path (so it renders any rack).

- [ ] **Step 1: Failing tests:**

```rust
    // rack.rs
    #[test]
    fn songs_compile_to_rack_positions() {
        let mut song = Song::default();
        let mut bar = Bar::default();
        *bar.cell_mut(MachineUid(5)) = Cell { pattern: Some(PatternRef(3)), mute: true, transpose: 2 };
        song.bars.push(bar);
        let c = CompiledSong::compile(&song, &[MachineUid(5), MachineUid(1)]);
        assert_eq!(c.bars[0].patterns[0], Some(PatternRef(3)));
        assert!(c.bars[0].mutes[0]);
        assert_eq!(c.bars[0].transpose[0], 2);
        assert_eq!(c.bars[0].patterns[1], None);
    }

    // engine.rs
    #[test]
    fn a_reordered_rack_plays_the_same_patterns() {
        // Same machines, reversed order: every machine still plays its own pattern.
        let p = crate::demo::demo_project();
        let mut q = p.clone();
        q.machines.reverse();
        let a = crate::render::render_patterns(&p, 2);
        let b = crate::render::render_patterns(&q, 2);
        // Summation order differs, so compare energy, not bits.
        let e = |x: &(Vec<f32>, Vec<f32>)| x.0.iter().map(|s| (s * s) as f64).sum::<f64>();
        assert!((e(&a) - e(&b)).abs() / e(&a) < 1e-3);
    }

    #[test]
    fn a_second_303_sequences_its_own_patterns() {
        let mut p = Project::default();
        let mut e = MachineEntry::new(MachineUid(7), MachineType::Synth, "Synth 3".into());
        if let MachineBody::Synth(s) = &mut e.body {
            s.patterns[0] = p.synth(MachineUid(1)).unwrap().patterns[0].clone();
        }
        p.machines.push(e);
        p.next_uid = 8;
        let mut eng = Engine::from_project(48000.0, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 4096], vec![0.0; 4096]);
        eng.process(&mut l, &mut r);
        assert!(eng.playhead()[6] > 0, "slot 7 advanced");
    }
```

(Use whatever demo content makes `a_second_303…` audible; the assertion is on the playhead.)

- [ ] **Step 2:** fail. **Step 3:** implement; port sequencer tests (they used machine indices 0..6 — they become positions in the standard rack, so most keep their numbers). **Step 4:** full checks, **golden unchanged**, no_alloc, launch. **Step 5:** commit `feat(engine): sequencer, song and status by rack position`.

---

### Task 7: Uid-addressed commands

**Files:**
- Modify: `host.rs`, `engine.rs`, `tests/no_alloc.rs`, `crates/app/src/document.rs` (`commands_for`), `crates/app/src/ui/**` (audition, sampler load, pattern selects).

**Interfaces:**
- Produces (`host.rs`):
```rust
pub enum PatternData { Synth(SynthPattern), Drum(DrumPattern), Notes(NotePattern) }
pub enum Command {
    SetParam(ParamId, f32), SetBpm(f32), SetSwing(f32), Play, Stop,
    SelectPattern { uid: MachineUid, slot: u8 },
    Pattern { uid: MachineUid, index: u8, pattern: Box<PatternData> },   // replaces Synth/Drum/NotePattern
    SamplerSample { uid: MachineUid, slot: u8, sample: Option<Arc<SampleData>> },
    SamplerPlay { uid: MachineUid, slot: u8, play: Box<SlotPlay> },
    ReplaceEngine(Box<Engine>), SetMode(Mode), Song(Box<CompiledSong>), SetSongStart(u16),
    Touch(ParamId, bool), SetParamLive(ParamId, f32),
    NoteOn { uid: MachineUid, pitch: u8, vel: u8 }, NoteOff { uid: MachineUid, pitch: u8 },
}
pub enum Event { Status(Status), DisposePattern(Box<PatternData>), DisposeEngine(Box<Engine>), DisposeSong(Box<CompiledSong>),
                 DisposeSample(Option<Arc<SampleData>>), DisposeSlotPlay(Box<SlotPlay>) }
```
- Engine: `swap_pattern(uid, index, &mut Box<PatternData>)` swaps into the slot's bank when the variant matches its `SlotPatterns` and `index < PATTERNS`; otherwise leaves the box unchanged — either way the host returns the box as `DisposePattern` (old pattern on success, the rejected one otherwise). Unknown uid → same. `NoteOn/Off` and `Sampler*` resolve the uid; non-matching type or unknown uid is ignored (payload disposed).
- App: `commands_for` and every send site use `MachineId::X.uid()` (still six) — Task 9 swaps those for real uids.

- [ ] **Step 1: Failing tests** (host.rs):

```rust
    #[test]
    fn commands_for_an_unknown_uid_are_ignored_and_disposed() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::Pattern { uid: MachineUid(99), index: 0, pattern: Box::new(PatternData::Synth(Default::default())) }).unwrap();
        tx.push(Command::NoteOn { uid: MachineUid(99), pitch: 60, vel: 100 }).unwrap();
        run(&mut host, 512);
        assert!(std::iter::from_fn(|| rx.pop().ok()).any(|e| matches!(e, Event::DisposePattern(_))));
    }

    #[test]
    fn a_pattern_of_the_wrong_type_is_refused() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let before = host.engine().pattern_len(MachineUid(3), 0); // 808 drum pattern length
        tx.push(Command::Pattern { uid: MachineUid(3), index: 0, pattern: Box::new(PatternData::Synth(Default::default())) }).unwrap();
        run(&mut host, 512);
        assert_eq!(host.engine().pattern_len(MachineUid(3), 0), before);
        assert!(std::iter::from_fn(|| rx.pop().ok()).any(|e| matches!(e, Event::DisposePattern(_))));
    }
```

(`Engine::pattern_len(uid, index) -> Option<u8>` returns the stored pattern's `length` field — small accessor for tests and the UI.) Extend `no_alloc.rs`: send `Pattern` (each variant) and `Song(Box<CompiledSong>)` under the armed counter.

- [ ] **Step 2–5:** fail → implement → full checks (golden unchanged, no_alloc, launch) → commit `feat(engine): commands address machines by uid; one Pattern command`.

---

### Task 8: SetRack — rack swaps on the audio thread

**Files:**
- Modify: `rack.rs`, `engine.rs`, `host.rs`, `sequencer.rs` (track moves), `tests/no_alloc.rs`.

**Interfaces:**
- Produces:
```rust
// rack.rs
pub struct RackSwap {
    pub order: Vec<MachineUid>,              // new rack order, ≤ 16
    pub fresh: Vec<Option<Slot>>,            // same length as order: Some(new slot) or None (= move the existing one)
    pub into: Vec<Slot>,                     // empty, capacity MAX_MACHINES
    pub song: Box<CompiledSong>,             // compiled for `order`
}
impl RackSwap {
    /// UI-thread helper: builds a swap from the project (after a rack edit) and the uids the engine has now.
    pub fn build(project: &Project, current: &[MachineUid], sample_rate: f32) -> Box<RackSwap>;
}
// host.rs
Command::SetRack(Box<RackSwap>)
Event::DisposeRack(Box<RackSwap>)   // carries the old slot Vec (in `into`), unused fresh slots, old song
```
- `Engine::set_rack(&mut self, swap: &mut RackSwap)` (no alloc): for each position i, take `fresh[i]` if Some, else move the slot with `order[i]` out of `self.slots` (swap_remove-free: use `Option` take via index search + `std::mem::replace` with a placeholder slot? — **do not construct slots on the audio thread**: instead `self.slots` is drained by moving elements into `swap.into` in the new order using `Vec::swap`/`swap_remove` on `self.slots` — both are allocation-free; then `std::mem::swap(&mut self.slots, &mut swap.into)` so the old Vec (now holding only removed slots) goes back in the swap). Sequencer track state moves with its slot (permute `tracks`/`chosen` by the same mapping; new slots get fresh tracks). Swap the song (`std::mem::swap` with `swap.song`). `rack_gen += 1`. Slots that survive keep voices sounding.
- `Status::rack_gen` reports the engine's counter.
- Host: `Command::SetRack(mut b)` → `engine.set_rack(&mut b)` → push `Event::DisposeRack(b)`.

- [ ] **Step 1: Failing tests:**

```rust
    // engine.rs
    #[test]
    fn a_rack_swap_keeps_surviving_machines_playing() {
        let p = crate::demo::demo_project();
        let mut eng = Engine::from_project(48000.0, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 2048], vec![0.0; 2048]);
        eng.process(&mut l, &mut r);
        let head = eng.playhead()[0];
        // Remove the 909 (uid 4), move Synth 1 last, add an FM.
        let mut q = p.clone();
        q.machines.retain(|m| m.uid != MachineUid(4));
        let s1 = q.machines.remove(0);
        q.machines.push(s1);
        q.machines.push(MachineEntry::new(MachineUid(7), MachineType::Fm, "FM 2".into()));
        q.next_uid = 8;
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0);
        eng.set_rack(&mut swap);
        let order: Vec<u16> = eng.slot_uids().iter().map(|u| u.0).collect();
        assert_eq!(order, [2, 3, 5, 6, 1, 7]);
        assert_eq!(eng.playhead()[4], head, "Synth 1 kept its sequencer position");
        assert_eq!(swap.into.len(), 1, "the 909 came back for disposal");
        assert_eq!(swap.into[0].uid, MachineUid(4));
        assert_eq!(eng.rack_gen(), 1);
    }

    #[test]
    fn a_surviving_voice_keeps_sounding_through_a_swap() {
        let p = Project::default();
        let mut eng = Engine::from_project(48000.0, &p);
        eng.note_on(MachineUid(5), 60, 100); // FM audition (Task 7 API)
        let (mut l, mut r) = (vec![0.0; 1024], vec![0.0; 1024]);
        eng.process(&mut l, &mut r);
        let mut q = p.clone();
        q.machines.retain(|m| m.uid != MachineUid(1));
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0);
        eng.set_rack(&mut swap);
        eng.process(&mut l, &mut r);
        assert!(l.iter().any(|s| s.abs() > 1e-4), "FM still sounding");
    }
```

(`Engine::note_on(uid, pitch, vel)` is the audition entry the host's `NoteOn` calls; use whatever name Task 7 produced.) In `no_alloc.rs`, add: build a `RackSwap` (remove one, reorder, add one) on the test thread, then arm the counter and push `Command::SetRack` through `EngineHost::process`, asserting zero allocations; the `DisposeRack` event is popped and dropped after disarming.

- [ ] **Step 2–5:** fail → implement → full checks (golden unchanged, no_alloc incl. new case, launch) → commit `feat(engine): SetRack swaps the rack without allocating; rack_gen in status`.

---

### Task 9: Retire the fixed-six bridge

**Files:**
- Modify: `machine.rs` (delete `MachineId`, `MachineKind`, `kind_of`, `MACHINES`, `TRANSPOSED`, `NOTE_MACHINES`, `FIRST_NOTE_MACHINE`), `params.rs` (delete the Task 3 helper fns and any remaining `SYNTHS/KITS/CHANNELS`), every remaining user in `acid-engine` and `crates/app`.

**Interfaces:**
- Produces: the app addresses machines by the uids in `project.machines`. Specifically:
  - `ui/mod.rs::rack` draws one panel per `project.machines` entry in order, dispatching on `entry.machine_type()` to the existing synth/drum/FM/sampler panel fns, which now take `uid: MachineUid` instead of an index.
  - Per-machine UI state arrays (`ViewState.bank: [u8; MACHINES]`, `writers`, `scroll_accum`, `scroll_at`, `poly_*: [_; 2]`, …) become `HashMap<MachineUid, …>` (or one `HashMap<MachineUid, PanelState>` — implementer's choice, documented in the report), defaulted on first use.
  - Mixer panel: one strip per machine in rack order, labelled with `entry.name` (replaces `CHANNEL_NAMES`).
  - Song panel: one lane per machine, labelled with `entry.name`; transpose shown when `ty.transposes()`.
  - Document edits keyed by `uid` instead of `synth`/`kit`/machine index (`Edit::Step { uid, … }`, `Edit::NotePattern { uid, … }`, `Edit::SamplerSlot { uid, slot, … }`, etc.); `MergeKey` likewise. Clipboard: `Clip` kind from `entry.machine_type()`; paste rules unchanged (synth↔synth, drum↔drum incl. 808↔909, notes↔notes).
  - The app sends `RackSwap`-free behaviour as today (no rack editing yet); `rack_gen` is stored from the last status but not yet compared (plan 10b).
- Golden test still passes; the app shows exactly today's six panels in the same order with the same labels ("Synth 1", "Synth 2", "808", "909", "FM", "Sampler" — check the panel headers' existing strings and keep them identical).

- [ ] **Step 1: Failing test** — a compile-level guard plus a behaviour test in the app:

```rust
    // crates/app/src/ui/mod.rs tests
    #[test]
    fn the_rack_draws_one_panel_per_machine_in_order() {
        let mut p = acid_engine::project::Project::default();
        p.machines.swap(0, 5);
        let app = SidAcidApp::headless(Document::new(p));
        let order: Vec<String> = app.rack_panel_titles();
        assert_eq!(order, ["Sampler", "Synth 2", "808", "909", "FM", "Synth 1"]);
    }
```

(`rack_panel_titles(&self) -> Vec<String>` is a small helper returning the header text the rack would draw, built from the same code path the panels use for their titles.) Then delete `MachineId` & friends and let the compiler list every remaining site.

- [ ] **Step 2–4:** fail → implement → `cargo test --workspace`, golden unchanged, no_alloc, clippy zero, fmt; launch check (exit 124); **screenshot check is done by the controller**, not the implementer.
- [ ] **Step 5:** commit `refactor(app,engine): retire MachineId — panels, mixer, song and edits follow the rack`.

---

### Task 10: Manual verification (controller)

- [ ] Launch the app (`cargo run --release -p sidacid`), screenshot: six panels, mixer strips and song lanes labelled as before.
- [ ] Open `~/Documents/Showcase.sidacid` (a v2 project) through a throwaway build that sets the open path at startup; it loads, plays (CPU meter moves), and saves as v3; reopen the v3 file.
- [ ] `cargo run --release -p sidacid -- render-demo /tmp/x.wav` succeeds.
- [ ] Record results and anything left open in `docs/superpowers/plans/2026-10-14-sidacid-plan10-deferred.txt`; commit.
