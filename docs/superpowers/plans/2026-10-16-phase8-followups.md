# Phase 8: Follow-ups and Known Bugs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ratchets time their sub-hits between 1/64 ticks, the known small bugs left after phases 3–7 are fixed, and the README describes the Mono machine and shows current images.

**Architecture:** The sequencer keeps emitting events per tick; a ratchet sub-hit that falls between ticks carries a sample offset within its tick (`SeqEvent` gains a sub-tick delay), and the engine delays the event by that many samples inside the block it renders. App bugs are fixed where they live (`ui/dock.rs`, `ui/tool_menu.rs`, `document.rs`, the engine's per-track played/chance state).

**Tech Stack:** Rust 2024, egui 0.31.

**Spec:** `docs/superpowers/specs/2026-10-16-dock-song-lanes-design.md` (§6 ratchets, §7 tools). Phases 1–7 are merged.

## Global Constraints

- User-facing text never names a commercial product or manufacturer.
- Golden hashes in `crates/acid-engine/tests/golden.rs` never change (PATTERN_HASH 0x37e7_dc98_f0d6_23f1, SONG_HASH 0xb18d_e278_6f54_1314). `no_alloc` passes; nothing on the audio thread allocates.
- No project version bump; old projects play the same except ratchets now land on exact fractions.
- Branch `phase8-followups`; commit with `git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit`, message ending `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; `cargo fmt --all`; each task ends with `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` clean.

---

### Task 1: Sub-tick ratchet timing (engine)

A note with ratchet `n` plays `n` sub-hits at exactly `start + k·len/n` ticks (k = 0..n), as fractions, not rounded to ticks; each sub-hit lasts `len/n` (acid gates still release at the normal point before the next sub-hit; poly releases and retriggers; drums hit). So a one-step (4-tick) note with ratchet 8 gives 8 evenly spaced hits, not 4.

- `trig::sub_hits` yields exact positions (e.g. in a fixed-point unit such as 1/256 tick, or tick + fraction); the sequencer emits a sub-hit on the tick it falls in, with its offset within that tick.
- `SeqEvent` (or the event queue the engine reads) carries the in-tick offset; the engine converts it to samples using the current samples-per-tick and delays the event inside the render block (events past the block end carry over without allocating).
- Hits on whole ticks (ratchet 1, and sub-hits that land on ticks) keep today's exact sample timing, so golden stays bit-identical.
- Tests: ratchet 8 on a 4-tick note gives 8 events with evenly spaced sample times; ratchet 3 on 12 ticks unchanged; ratchet 3 on 4 ticks spaced 4/3 ticks; acid, drum and poly paths; no_alloc covers a sub-tick ratchet; golden unchanged.
- Update README and spec text that says "a one-step note re-triggers at most 4 times".

Commit: `feat(engine): ratchets land between ticks`.

### Task 2: App bug fixes

Fix each, with a frame/unit test that fails before the fix:

1. Tab can move egui focus off the dock so dock keys (entry, nudge, clipboard) stop working until a click. Tab must not strand dock keys (keep the dock's key handling while no text field has focus, or stop Tab from moving focus away from the dock).
2. Esc while a combo box inside a tool popup is open cancels the whole tool; it must only close the combo (a second Esc then cancels the tool).
3. Apply after a foreign edit (an edit not made by the tool, e.g. a global undo-stack change while the session was open) can leave the preview as extra undo steps around it. Apply must always yield exactly one undo step holding origin → result, whatever happened in between.
4. Cancelling a tool when the undo history is at its 200-step limit leaves the history one step shorter (the oldest step was evicted by the preview). Cancel must leave the history exactly as before the session (e.g. don't push preview steps onto the bounded stack; keep the preview outside history until Apply).

Commit: `fix(app): Tab, Esc and undo history around note tools`.

### Task 3: Played and chance state survive edits during playback

The engine's poly played-record and the chance roll are keyed by note index, so an edit that reorders or inserts notes mid-pass (settle, delete, paste) can briefly cut a ringing note, re-trigger one, or re-roll chance. Make them stable across edits: e.g. when the app sends a pattern edit, it also sends (or the engine derives) an index remap so the played bitset and pending ratchets follow their notes, and chance uses a key that doesn't change when unrelated notes move (keep golden hashes: fixture notes have chance 100, so the chance key can change freely). No allocation on the audio thread. Tests: deleting an earlier note while a later poly note rings doesn't cut or re-trigger it; inserting a note before a chance-50 note doesn't change that note's outcome in the same pass.

Commit: `fix(engine): played and chance state follow notes across edits`.

### Task 4: README Mono section and images

- README describes the Mono machine (what it is, its controls, its arpeggiator) in the same style as the other machines, read from the code; no product names.
- Refresh `docs/images/screenshot.png` (whole window showing the dock with lanes), `fm.png` (FM synth editor) and `sampler.png` (sampler), captured with PIL ImageGrab cropped to the SidAcid window; the controller does this step itself (it runs on the user's live desktop).

Commit: `docs: the Mono machine and fresh images`.
