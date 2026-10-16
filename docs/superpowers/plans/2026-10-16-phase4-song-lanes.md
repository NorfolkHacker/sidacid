# Phase 4: Song Lanes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The song stops being a row of bars and becomes one lane of pattern blocks per machine. Blocks never overlap, a block lengthens with its pattern and pushes later blocks on its lane right (ripple), gaps are silence, and the dock edits the song in Song mode. Old songs convert and sound exactly the same.

**Architecture:** `Song { lanes, end, loop_range, automation }` with `Block { start, pattern, len: Option<u16>, mute, transpose }` (steps). Pure lane helpers in `acid-engine/src/song.rs` do placement without overlap, ripple, insert/delete time and recorded pattern changes. The engine compiles lanes to rack positions; the sequencer in Song mode follows each track's own block (start, length, mute, transpose) instead of shared bars, with a per-track arpeggiator clock from the block start. The document ripples the song in the same undo step as a pattern-length change. The dock gets a Song mode that draws and edits blocks, reusing phase 3's interaction rules.

**Tech Stack:** Rust 2024, serde + RON, egui 0.31.

**Spec:** `docs/superpowers/specs/2026-10-16-dock-song-lanes-design.md` §4. Phases 1–3 are merged.

## Global Constraints

- User-facing text never names a commercial product or manufacturer.
- `PROJECT_VERSION` becomes 6; versions 0–5 still load.
- **The golden hashes (`crates/acid-engine/tests/golden.rs`: `PATTERN_HASH = 0x37e7_dc98_f0d6_23f1`, `SONG_HASH = 0xb18d_e278_6f54_1314`) must not change**, and `tests/fixtures/v4_demo.sidacid` must still load equal to the demo. A converted song must play, end and loop exactly as before.
- The audio thread never allocates (`tests/no_alloc.rs`).
- Blocks on one lane never overlap after any operation.
- Every song edit is one undo step (`Edit::Song`, or an `Edit::Group` that includes it).
- Work on branch `phase4-song-lanes`. Commit identity flags and `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; `cargo fmt --all`; app package `sidacid`. Between Task 2 and Task 3 the app crate may not compile; engine tests run with `cargo test -p acid-engine` in that window.

## Decisions taken in this plan

| Question | Decision |
|---|---|
| Song length | `Song.end: Option<u32>`; the song ends at `max(end, last block end)`. Conversion sets `end` to the old song's total steps, so trailing empty bars and bar lengths are kept exactly; new songs have `end = None`. The spec's "rounded up to a 16-step bar" is replaced by this (exactness for converted songs) — amend the spec in Task 6 |
| Loop range | Steps `(start, end)`, end exclusive. Conversion: bar a's start step to bar b's end step |
| Song start / cursor | A step (`u32`). `Command::SetSongStart(u32)`; status keeps `song_step`, and `song_bar` becomes `song_step / 16` (display only) |
| Arp clock in Song mode | Per track: ticks since that track's block start (was: since the bar start) |
| Block end | `start + len.unwrap_or(pattern length)`, read live (a pattern edited during playback changes it, as a bar's length did) |
| Ripple | When a pattern's length grows by `d`, each `len: None` block of it on its machine's lane grows; later blocks on that lane move right only as far as needed to clear the block in front (cascading). Shrinking leaves gaps. `Some` blocks never ripple. Done in the document as part of the pattern edit's undo step |
| Placement | `place` puts a block at the requested start if free, else at the nearest free start (searching both ways on the 1-step grid, preferring later on ties); if it can't fit anywhere before `MAX_SONG_STEPS` it is refused |
| Recording a pattern change | At the play position floored to a 16-step bar: the covering block is split there (left part keeps its pattern with a fixed length); the right part and every following block that touched it with the same old pattern switch to the new pattern; with no covering block, a new `len: None` block is placed there, cut (`Some`) to end before the next block |
| Insert time / Delete time | Insert at `t` for `n` steps: blocks starting at or after `t` and automation points at or after `t` move right by `n`; the loop follows like bars did. Delete `[a, b)`: blocks starting inside are removed, a block running into `a` is cut to end at `a`, later blocks and automation move left by `b − a`, points inside are removed, a loop inside is cleared |
| Rack edits | Removing a machine removes its lane (restored on undo), as its cells were |
| Song-mode selection | By value: `PanelState::song_sel: Vec<Block>` |

---

### Task 1: The block song data, helpers and conversion (engine)

**Files:** `crates/acid-engine/src/project.rs` (`Block`, `Song`, validation, version), `crates/acid-engine/src/song.rs` (helpers; bar helpers move to migrate), `crates/acid-engine/src/migrate.rs` (`SongV5`, `BarV5`, `CellV5`, `ProjectV5`, upgrade), `crates/acid-engine/src/rack_edit.rs` (lanes instead of cells).

**Interfaces:**
- `Block { start: u32, pattern: PatternRef, #[serde(default, skip_serializing_if = "Option::is_none")] len: Option<u16>, #[serde(default, skip_serializing_if = "is_false")] mute: bool, #[serde(default, skip_serializing_if = "is_zero")] transpose: i8 }` with `Block::new(start, pattern)`.
- `Song { lanes: Vec<(MachineUid, Vec<Block>)>, #[serde(default)] end: Option<u32>, loop_range: Option<(u32, u32)>, automation: Vec<AutoLane> }`; `Song::lane(&self, uid) -> &[Block]` (empty if none), `Song::lane_mut(&mut self, uid) -> &mut Vec<Block>` (creating it).
- `song.rs`: a pattern-length lookup `pub type Lens<'a> = &'a dyn Fn(MachineUid, u8) -> u32;` and `pub fn lens_of(p: &Project) -> impl Fn(MachineUid, u8) -> u32` (steps of that machine's pattern, 16 if missing); `block_end(b, uid, lens) -> u32`; `song_end(song, lens) -> u32`; `block_at(blocks, uid, step, lens) -> Option<usize>`; `place(blocks, uid, block, lens) -> Option<usize>`; `ripple(blocks, uid, slot, old_len, new_len, lens)`; `insert_time(song, at, n, lens)`; `delete_time(song, a, b, lens)`; `record_pattern(song, uid, step, slot, lens)`; existing automation helpers (`value_at`, `lane`, `is_automated`, `write_automation`, `clear_automation`, `MAX_SONG_STEPS`) stay.
- Validation: blocks per lane sorted by start and non-overlapping (using pattern lengths), pattern refs in range, uids in the rack and unique, `loop_range` with `start < end`, `end`/starts within `MAX_SONG_STEPS`.
- Conversion v5 → v6: `BarV5`'s cells per bar start (bar lengths from `bar_steps` over the v5 patterns) → one block per cell with a pattern (`len: None` when the pattern's length equals the bar's, else `Some(bar len)`), mute and transpose carried; `end = Some(total)`; loop `(start of bar a, end of bar b)`. Keep the v5 bar helpers (`bar_steps`, `bar_start_step`) private to migrate.

**Tests** (write first): block helpers (end with `None`/`Some`, `song_end` with and without `end`, `block_at`); `place` exact/nearest-free/refused; `ripple` growth cascade, gap absorption, shrink leaves gap, `Some` untouched; `insert_time`/`delete_time` with automation and loop; `record_pattern` split and run-through; conversion of a hand-built v5 song (incl. a short pattern looping in a longer bar → `Some(bar)`, an empty bar in the middle and at the end, mutes, transposes, loop) asserts the exact blocks, `end` and loop; a v5 RON round-trip loads. The engine crate's other tests (song.rs bar tests) are replaced by these.

Commit: `feat(engine): songs are lanes of pattern blocks; v5 songs convert`.

---

### Task 2: Song playback on blocks (engine)

**Files:** `crates/acid-engine/src/rack.rs` (`CompiledSong { lanes: Vec<Vec<Block>> /* per rack position */, end: Option<u32>, loop_range: Option<(u32, u32)>, automation }`), `sequencer.rs`, `engine.rs`, `host.rs`, `render.rs`, tests incl. `tests/no_alloc.rs`.

**Behaviour:**
- `Track` gains `block: Option<usize>` (index into its lane), `block_start: u32`, `block_end: u32`.
- Song mode, on quarter 0 of each step (where `load_bar` ran): apply a pending resync (re-seek every track at the current position, `auto_sync`); handle the loop (`song_step >= loop end` → `song_step = loop start`, re-seek, `auto_sync`); handle the end (`song_step >= song end` → the existing end-of-song NoteOffs/releases, `stop`, `ended`); then per track: a block starting now loads it (pattern, `pos = 0`, `step = 0`, `silent = mute`, transpose for transposing machines, and the phase-1 bar-line gate cut); else reaching `block_end` makes it silent (same gate cut); then automation. `song_step` still advances on quarter 2.
- Seek (play, resync, loop, rack join): for each track, the block covering `song_step` (pattern, silent = mute, transpose, `pos = ((song_step − start) × 4 + sub) mod pattern ticks`, `block_end`), else silent.
- `pub fn tick_clocks(&self) -> [(u64, f64); MAX_MACHINES]`: Pattern mode as now for every position; Song mode per position from its block start (same quarter-3 rule as the old bar clock). `engine.rs` takes the array before `tick` and gives each Mono its own position's clock.
- `set_song_start(step: u32)`, `song_step()`, `song_bar() = song_step / 16`; `Command::SetSongStart(u32)`; `Status.song_bar` keeps its meaning (`step / 16`).
- `render.rs` uses the new end for progress.
- Port every sequencer/engine/host song test to blocks keeping their timing assertions (a bar becomes one block per machine at the bar's start); the arp-clock test becomes `the_tick_clock_restarts_at_every_block`.

**Gate:** `cargo test -p acid-engine` incl. golden (unchanged hashes) and no_alloc. If a golden hash changes, compare the event streams of master and the branch for `render_song` of the fixture and fix the sequencer — never edit the hashes.

Commit: `feat(engine): Song mode plays each machine's own blocks`.

---

### Task 3: The app on the block song

**Files:** `crates/app/src/document.rs` (ripple on pattern-length edits; rack-edit inverses), `crates/app/src/ui/record.rs` (`record_pattern`, song cursor in steps), `transport.rs` (bar display from `song_step`), `song_panel.rs` (remove the bar timeline, bar editor and bar toolbar; keep the automation list), `files.rs` (export "has song" check, loaded-document resets), `pads.rs`, `mod.rs` (`ViewState::song_cursor: u32`; remove `song_sel_end`, `song_clip`), tests.

**Behaviour:**
- Pattern-length changes (`NoteLength`, and `NotePattern`/`SynthPattern`/`DrumPattern` whose `length` differs) ripple that machine's lane via `song::ripple` inside the same `apply`; the inverse restores both (return `Edit::Group([pattern inverse, Edit::Song { old }])` when the song changed) and the commands include the recompiled song.
- Recording a pad change in Song mode writes `song::record_pattern` at `status.song_step`.
- Play in Song mode sends `SetSongStart(song_cursor)`; Stop-while-stopped returns the cursor to the loop start (or 0).
- The transport shows `bar = song_step / 16 + 1` and `step`.
- The song panel keeps its title (bars = `song_end / 16` rounded up) and the automation list only.
- Port or replace the app's song tests (recording, rack-edit undo with song lanes, export) keeping what they check.

Commit: `feat(app): the app on block songs; pattern lengths ripple the song`.

---

### Task 4: The dock in Song mode — drawing, placing and moving blocks

**Files:** `crates/app/src/ui/song_lane.rs` (new: one machine's song lane), `dock.rs` (switch on `view.mode`), `ui/song_ops.rs` (new pure ops on block selections, mirroring `note_ops`: `resolve`, `in_box`, `in_range`, `shift` (bar-snapped steps, no overlap: refuses a move that would overlap and stops at the nearest free spot), `delete`, `copy`, `paste`, `duplicate`, `resize` (sets `len: Some`)), `mod.rs` (`PanelState::song_sel: Vec<Block>`, `song_drag`).

**Behaviour** (Song mode only; Pattern mode is unchanged):
- The ruler shows bars (1, 2, 3 … every 16 steps) across `song_end + 64` steps; the playhead is `status.song_step`; a click on the ruler sets the song cursor (and the engine's start when stopped); a drag selects a step range snapped to bars (shared `DockState::range`, now in steps in Song mode).
- Each lane draws its blocks as rounded rectangles labelled with the pattern (`A01`), muted blocks dimmed, a fixed-length block's looped repeats marked with thin dividers, and a mini preview of the pattern's notes inside (pitch scaled to the block height, drums as dots).
- Mouse, by analogy with phase 3: click selects a block (Shift toggles); drag on empty space box-selects; drag a block moves the selection by whole bars (Alt: single steps) without overlap; Ctrl-drag moves copies; drag a block's right edge sets `len: Some` (bar-snapped, Alt steps) for the selection; right-click deletes the block or the selection; double-click empty space places the machine's selected pattern at the bar under the pointer (`place`); double-click a block switches to Pattern mode and selects that block's pattern on its machine.
- Every edit is one `Edit::Song` undo step; selection changes make none.

Tests: frame tests for place, select, move (incl. a refused overlap landing on the nearest free spot), Ctrl copy, resize, delete, open-pattern.

Commit: `feat(app): edit song blocks in the dock`.

---

### Task 5: Song-mode header, keys and clipboard

**Files:** `dock.rs`, `keys.rs`.

**Behaviour:**
- Dock header in Song mode: **Capture** (places every machine's selected pattern at the song cursor, one undo step); with a ruler range: **Loop** (sets `loop_range`), **Insert time** (`insert_time` at range start for range length), **Delete time** (`delete_time` over the range); with exactly one block selected: its pattern (bank letter + number pickers), **Mute** toggle and **Trans** (−12..12, only for machine types that transpose); a "Clear loop" button when a loop is set.
- Keys with dock focus in Song mode: ←/→ move the block selection one bar (Alt: one step), Delete/Backspace delete it, Esc clears it, Ctrl+C/X/V/D/A on blocks (paste at the song cursor; a multi-lane clip — from a range — pastes back into the same machines; overlaps land on the nearest free spot), all one undo step; keyboard note entry is off in Song mode.
- The block clipboard is separate from the note clipboard (`ViewState::block_clip`).

Tests: header buttons (capture, loop, insert time, delete time, mute, transpose, pattern change), keys and clipboard as frame tests.

Commit: `feat(app): song header tools, keys and block clipboard`.

---

### Task 6: Docs, spec and a look

- README "Song mode" bullets rewritten for lanes of blocks (no product names).
- Spec §4: replace "rounded up to 16 steps" with the `end` rule; note per-track arp clock.
- Run the app briefly (no synthetic input), cropped screenshot to the git-ignored workspace, delete any full-desktop capture, close the app.
- Final gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
- Commit: `docs: song lanes`.
