# Phase 5: Scale Highlight, Snap and Fold Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The project gets one key (root + scale). Melodic lanes shade in-scale rows and mark the root; a "Snap to scale" toggle makes new notes, dragged notes and ↑/↓ transposes land on scale degrees; each melodic lane can fold away out-of-scale rows.

**Architecture:** A pure `acid-engine/src/scale.rs` (scale tables, `Key::contains`, `Key::snap`, `Key::step`) shared with the randomiser. `Project.key: Key` (serde-defaulted, undoable via `Edit::Key`). In the app, `LaneGeom` gains a folded row mode backed by a fixed-size row table, so folded lanes map rows↔pitches exactly like the others. Snap is applied at the few places pitches are chosen (click add, key entry, drag pitch, ↑/↓).

**Tech Stack:** Rust 2024, serde + RON, egui 0.31.

**Spec:** `docs/superpowers/specs/2026-10-16-dock-song-lanes-design.md` §5. Phases 1–4 are merged.

## Global Constraints

- User-facing text never names a commercial product or manufacturer.
- No version bump: `Project.key` and `LaneView.fold` are `#[serde(default)]` and skipped when default; old files load as Chromatic / unfolded.
- No engine audio changes; golden hashes unchanged; no_alloc passes.
- Drum lanes ignore the key entirely.
- A key change is one undo step; the snap and fold toggles are view settings (snap saved in the project's key, fold in `LaneView`) changed directly without undo or dirty, like other lane view settings.
- Branch `phase5-scale`; commit identity flags and `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; `cargo fmt --all`; app package `sidacid`; each task ends with `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Decisions taken in this plan

| Question | Decision |
|---|---|
| Scales offered | Chromatic (off) plus the randomiser's five: natural minor, melodic minor, phrygian, major, hungarian minor. Names in the UI: "Chromatic", "Minor", "Melodic minor", "Phrygian", "Major", "Hungarian minor" |
| Where the key lives | `Project.key: Key { root: u8 /* 0..12, C = 0 */, scale: Scale, snap: bool }` |
| Snap rule | Nearest in-scale pitch; ties go up |
| ↑/↓ with snap on | Each selected note moves to the next/previous in-scale pitch (from an off-scale note: to the nearest in-scale pitch in that direction); Shift+↑/↓ (octave) unchanged |
| Drag pitch with snap on | The pointer's pitch is snapped before computing the group's pitch delta (delta of the grabbed note); other notes move by that delta and are then each snapped |
| Fold | `LaneView.fold: bool`, a "Fold" toggle in melodic lane headers; folded rows = in-scale pitches plus any pitch that holds a note in the shown pattern; with Chromatic, fold shows all rows |
| Randomiser | Unchanged behaviour; it reads its scale tables from `scale.rs` (same five, same weights) |

---

### Task 1: Scale module and the project key (engine)

**Files:** Create `crates/acid-engine/src/scale.rs`; modify `lib.rs`, `project.rs` (`Project.key`, validation: root < 12), `random.rs` (use the tables).

**Interfaces:**
```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scale { #[default] Chromatic, Minor, MelodicMinor, Phrygian, Major, HungarianMinor }
impl Scale {
    pub const ALL: [Scale; 6];
    pub fn degrees(self) -> &'static [u8]; // semitones above the root; Chromatic = 0..12
    pub fn label(self) -> &'static str;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key { pub root: u8, pub scale: Scale, #[serde(default)] pub snap: bool }
impl Key {
    pub fn contains(self, pitch: u8) -> bool;
    pub fn snap(self, pitch: u8) -> u8;          // nearest in-scale, ties up, within 0..=127
    pub fn step(self, pitch: u8, up: bool) -> u8; // next/previous in-scale pitch, clamped
    pub fn is_root(self, pitch: u8) -> bool;
}
pub const ROOT_NAMES: [&str; 12]; // C, C#, D, ... B
```
`random.rs` keeps its weights (2, 2, 1, 1, 0.5 in the same order) and gets degree lists from `Scale::degrees` (as i32).

**Tests:** degrees of each scale; `contains` for C minor; `snap` ties up (C# in C major → D), clamping at 0 and 127; `step` up/down within and across octaves and from an off-scale pitch; Chromatic contains everything and `step` is ±1; key round-trips in RON and is absent when default; the randomiser's seeded outputs are unchanged (existing tests pass).

Commit: `feat(engine): scales and a project key`.

---

### Task 2: Key picker, highlight and snap

**Files:** `crates/app/src/document.rs` (`Edit::Key { key: Key }` with merge key, inverse, no engine commands), `ui/dock.rs` (header: root combo, scale combo, "Snap to scale" toggle — Pattern mode only), `ui/lane.rs` (row shading/root mark; snap on click-add and drag), `ui/key_entry.rs` (`apply_entry` takes an `Option<Key>`: with snap, a typed note snaps), `ui/dock.rs` (↑/↓ selection transposes use `Key::step` when snap is on), `ui/note_ops.rs` (a `transpose_in_key(p, idx, up, key)` helper).

**Behaviour:** In melodic lanes (not drums), out-of-scale rows are drawn darker (on top of the black/white key shading) when the scale isn't Chromatic, and the root row gets a thin `theme::ACID` line at its bottom and its key-strip label shows the root name. Snap applies as in the decisions table. Changing root or scale is one undo step (`Edit::Key`); the snap toggle is set directly on `project.key.snap` (no undo, no dirty).

**Tests:** key edit undo; the header combos change the key (frame test); click-add with snap on lands on the scale; key entry snaps; ↑ moves selected notes by scale degree; drag snaps the pointer pitch; drum lanes unaffected.

Commit: `feat(app): pick a key; lanes show and snap to it`.

---

### Task 3: Fold

**Files:** `crates/acid-engine/src/project.rs` (`LaneView.fold`), `crates/app/src/ui/lane_geom.rs` (`Rows::Folded(FoldRows)` where `FoldRows { pitches: [u8; 128], count: u8 }` is `Copy`, ordered high→low; `row_count`, `row_of`, `pitch_of_row` handle it), `ui/lane.rs` (Fold toggle in melodic lane headers; build `FoldRows` from the key and the pattern's note pitches; pitch scroll clamps to the folded height).

**Tests:** `FoldRows` from C major plus an off-scale note keeps that note's row; row↔pitch round-trips; folded lane draws headless; clicking a folded row adds the right pitch (frame test); fold is ignored for drums and when collapsed; fold persists in the project (RON round-trip) and isn't undoable/dirtying.

Commit: `feat(app): fold a lane to its scale`.

---

### Task 4: Docs and a look

README dock bullets gain the key picker, scale highlight, Snap to scale and Fold (no product names). Brief app look (cropped screenshot into the git-ignored workspace, delete any full-desktop capture). Final gate. Commit `docs: scale tools`.
