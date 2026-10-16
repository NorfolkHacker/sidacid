# Phase 6: Chance, Ratchet, Trig Conditions and Fill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The engine acts on the note fields stored since phase 1 — `chance`, `ratchet`, `cond` — and a Fill switch; the dock gets a note inspector for the selection, chance/ratchet views in the expression strip, and badges on conditional notes.

**Architecture:** A pure `acid-engine/src/trig.rs` decides whether a note plays (chance hash + condition) and splits a note into ratchet sub-hits. Each sequencer track keeps a loop count and a "previous conditional result"; the acid, drum and poly paths ask `trig` at a note's start tick and emit its ratchet sub-hits. The project gets a `seed`. The app adds `Command::SetFill`, a Fill transport button, the inspector (one undo step per edit over all selected notes) and the strip modes.

**Tech Stack:** Rust 2024, egui 0.31.

**Spec:** `docs/superpowers/specs/2026-10-16-dock-song-lanes-design.md` §6. Phases 1–5 are merged.

## Global Constraints

- User-facing text never names a commercial product or manufacturer.
- **Golden hashes unchanged** (all fixture notes have chance 100, ratchet 1, cond Always; their playback must be bit-identical). no_alloc passes; nothing on the audio thread allocates.
- `Project.seed: u32` is `#[serde(default)]`; no version bump.
- Determinism: the same project renders identically every time (chance depends only on seed, rack position, loop count, note index).
- Branch `phase6-chance`; usual commit identity and `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; `cargo fmt --all`; each task ends with `cargo test --workspace` and clippy `-D warnings` clean.

## Decisions taken in this plan

| Question | Decision |
|---|---|
| Chance roll | `splitmix64(seed ⊕ pos·K1 ⊕ loops·K2 ⊕ index·K3) % 100 < chance` |
| Loop count | Per track, the number of completed passes of its current pattern since Play (Pattern mode) or since its block started (Song mode); a queued pattern switch resets it |
| Ratio(a, b) | Plays when `loops % b == a − 1` |
| First / NotFirst | `loops == 0` / `loops > 0` |
| Fill / NotFill | The engine's fill flag, set by `Command::SetFill(bool)` |
| Pre / NotPre | The track's last conditional result (false after a reset). A note is "conditional" when its `cond != Always` or `chance < 100`; its result (after chance) updates the track's last result |
| Evaluation order | Condition first; if it passes, then chance |
| Ratchet | `n = ratchet` sub-hits at `start + k·len/n` (integer division, `k = 0..n`, sub length `max(1, len/n)`); acid: each sub-hit is a fresh gate with the normal 2-ticks-early release, no legato between sub-hits (a slide applies only after the last); drums: n hits; poly: PolyOff/PolyOn per sub-hit |
| A note that doesn't play | Emits nothing (acid: behaves like a gap — an open gate releases at its normal point) |
| Inspector | In the dock header (Pattern mode) when the focused lane has selected notes: Vel (1–127), Chance (0–100 %), Ratchet (1–8), Cond (combo: Always, 1:2, 2:2, 1:3 … 8:8, First, Not first, Fill, Not fill, Pre, Not pre), and for acid lanes Accent/Slide toggles, drums Accent. A field showing mixed values shows "–". Each change applies to every selected note: one undo step; the selection follows the edited values |
| Strip modes | Per lane view state (not saved): Vel / Chance / Ratchet selector in the lane header when the strip is open (melodic poly lanes; drums get Acc / Chance / Ratchet; acid Acc+Slide / Chance / Ratchet). Chance and ratchet strips draw bars and edit by click/drag like velocity |
| Badges | Notes with chance < 100 or a condition get a dashed outline; a small text badge ("50%", "1:4", "×3", "F", "!F", "1st", "P") drawn when the note is wide enough |
| Fill button | In the transport: on while held; Shift-click latches/unlatches; lit while on |
| Reroll | A "Reroll" button next to the inspector's Chance changes `project.seed` (one undo step) |

---

### Task 1: The trig decision (engine, pure)

**Files:** Create `crates/acid-engine/src/trig.rs`; `lib.rs`; `project.rs` (`Project.seed`, `#[serde(default)]`).

**Interfaces:** `pub struct TrackTrig { pub loops: u32, pub pre: bool }`; `pub fn plays(n: &Note, index: usize, pos: usize, seed: u32, fill: bool, t: &mut TrackTrig) -> bool` (updates `t.pre` for conditional notes); `pub fn sub_hits(n: &Note) -> impl Iterator<Item = (u16 /*start*/, u16 /*len*/)>` (allocation-free); `pub fn is_conditional(n: &Note) -> bool`.

**Tests:** default notes always play and never touch `pre`; chance 0 never plays, 100 always; chance 50 over 1000 loop counts plays 40–60 % and is identical across runs; Ratio(1,4) plays on loops 0, 4, 8; Ratio(4,4) on 3, 7; First/NotFirst; Fill/NotFill; Pre/NotPre follow the previous conditional result; a failing condition skips the chance roll; sub_hits for ratchet 1/3/8 including len < ratchet.

Commit: `feat(engine): decide when a note plays and split ratchets`.

---

### Task 2: Playback (engine)

**Files:** `sequencer.rs` (per-track `TrackTrig`, loop counting at pattern wrap / block start / play / queued switch, fill flag; acid/drum/poly paths call `trig::plays` at a note's start tick and schedule sub-hits), `engine.rs`/`host.rs` (`Command::SetFill(bool)`, `Engine::set_fill`), tests incl. `no_alloc.rs` (a pattern with chance, ratchet and conditions exercises the new paths).

**Behaviour:** as in the decisions table. Acid ratchet sub-hits reuse the existing gate countdowns (a sub-hit sets `gate_left` for its sub length); the note's slide only holds after its last sub-hit. Poly notes track held pitches as now; each sub-hit releases and retriggers. A skipped note emits nothing; for acid, an open gate from an earlier note keeps its normal release.

**Tests:** sequencer event timing for ratchet ×3 on acid, drums and poly; Ratio(1,2) plays every other loop in Pattern mode; loop count resets at a song block start; Fill toggles a `Fill` note; chance 50 renders identically twice; golden unchanged.

Commit: `feat(engine): play chance, ratchet, conditions and fill`.

---

### Task 3: Inspector, strip modes and badges (app)

**Files:** `ui/dock.rs` (inspector + Reroll in the header), `ui/lane.rs` (strip mode selector, chance/ratchet strips, dashed outline + badge), `ui/note_ops.rs` (`set_field(p, idx, f)` helper used by inspector and strips), `ui/mod.rs` (`PanelState::strip_mode`), `document.rs` (`Edit::Seed`).

**Tests:** inspector edits all selected notes in one undo step and keeps them selected; mixed values show "–"; chance strip click sets chance on the notes under it; badge text for each condition; drum inspector shows Accent; Reroll is undoable.

Commit: `feat(app): a note inspector, chance and ratchet strips, condition badges`.

---

### Task 4: Fill, docs and a look

**Files:** `ui/transport.rs` (Fill button: held/Shift-latched, sends `SetFill`), README (chance, ratchet, conditions, Fill, inspector; no product names), spec §6 note on evaluation order and the conditional-note rule. Frame test for Fill (press → SetFill(true), release → SetFill(false); Shift-click latches). Brief app look (cropped screenshot, delete any full capture). Final gate. Commit `feat(app): a Fill button; docs for chance and conditions`.
