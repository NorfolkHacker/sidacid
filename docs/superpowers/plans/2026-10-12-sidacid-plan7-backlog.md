# SidAcid Plan 7 — Backlog Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix every deferred minor finding from Plans 2–6 that still applies (about 118 items: behaviour, robustness, test gaps and docs), applying the 16 accepted design decisions.

**Architecture:** No new architecture. Each task owns one area of the code and a list of triage item IDs.

**How to use the spec:** The spec (below) has one section per item, giving its location (file:line), the proposed fix, a test idea, its size and notes. The implementer reads each listed item's section and writes the fix and its test, test-first.
- Items marked ALREADY-FIXED or OBSOLETE in the spec need no work.
- If an item turns out to be already fixed, or impossible as described, the implementer says so in the report with evidence instead of forcing a change.

**Tech Stack:** Rust 2024, egui/eframe 0.31, as before. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-12-backlog-cleanup.md`. Read its "Accepted decisions" table, then the triage section for each item in your task. Search for `### <ID>`, for example `### P2-T4d`.

## Global Constraints

- `cargo test --workspace` passes at the end of every task. The baseline is 444. `cargo clippy --workspace --all-targets` reports 0 warnings, and `cargo fmt --all` is run before each commit.
- **Real-time rule:** `Engine::process`, everything it calls, and `EngineHost::process` never allocate, lock or do I/O. `crates/acid-engine/tests/no_alloc.rs` must keep passing.
- **Tests:**
  - Every BEHAVIOUR or ROBUSTNESS fix gets a test that fails before the fix and passes after it. Show RED and GREEN in the report.
  - TEST items add the missing test, or strengthen the weak one.
  - DOC items fix the comment.
  - Never weaken an existing assertion. If a decision changes behaviour an existing test pins, update that test to the new behaviour and say so in the report.
- **Project files:** `PROJECT_VERSION` stays 2, and project files from earlier builds still load.
- **Commits:** one commit per coherent fix group is fine. Every commit message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Report:** each task's report must list every ID it owns, with an outcome — FIXED (what changed + which test), ALREADY-FIXED (evidence) or NOT-FIXED (why, only if impossible). Nothing may be silently skipped.

## Why this plan doesn't inline code

This plan departs from the usual writing-plans format on purpose. With 118 small fixes spread over the whole codebase, inlining every code change would mean duplicating the spec's per-item design in a much larger document. Each item's section in the spec already gives location, fix and test. The implementers (standard or most-capable tier) write the code test-first, and every task gets the usual spec-and-quality review.

---

### Task 1: Engine host and transport

**Files:** `crates/acid-engine/src/host.rs`, `engine.rs` (and its tests), `mixer.rs` (P2-T2a doc only)

**Items:** P2-T1a, P2-T1b, P2-T2a, P2-T3a, P2-T3b, P2-T3c, P3-T3a, P3-T3b, P4-Fa, P4-T2a, P4-T3a, P4-T5a, P5-Fa, P5-T6a, P5-T6b, P6-T5b

**Decisions:**
- **#8:** Play and Stop release only pattern voices, so auditions keep sounding. Stop-twice still hard-silences everything.
- **#12:** The first Stop after a song ends by itself only releases notes. That means the "already stopped" check must distinguish "stopped because the song ended" from "stopped by the user". For example, `end_playback` on song end sets a flag that makes the next `stop()` release-only, and a following `stop()` resets.

- [ ] Read the spec sections for each item, and the two decisions.
- [ ] For each item: write the failing test, run it (RED), implement the fix, run it (GREEN).
- [ ] Run `cargo test --workspace` (including `no_alloc`) and `cargo clippy --workspace --all-targets`.
- [ ] Commit (`fix(engine): …`, with the trailer).

### Task 2: Sequencer and song model

**Files:** `crates/acid-engine/src/sequencer.rs`, `song.rs`

**Items:** P3-T1c, P3-T1d, P3-T2a, P3-T2b, P3-T8d, P4-T4a, P4-T4b

**Decisions:**
- **#5:** Keep the 303-like slide or tie across a bar boundary. Document it and pin it with a test.
- **#7:** Inserting a bar inside the loop, or right at its end, extends the loop.

- [ ] Read the spec sections and decisions.
- [ ] Test-first fix per item (RED → GREEN).
- [ ] Workspace tests and clippy.
- [ ] Commit.

### Task 3: Randomiser and render

**Files:** `crates/acid-engine/src/random.rs`, `render.rs`

**Items:** P3-T4a, P3-T4b, P3-T4c, P3-T5a, P3-T5b, P3-T5c, P3-T6a, P3-T6b

**Notes:**
- P3-T4a keeps leaps within 7 semitones at the range edges, as the spec says. Tighten the test to ≤7.
- P3-T5a changes some seeded outputs. Update seed-pinned expectations only where they encode the old, wrong behaviour, and list them in the report.

- [ ] Read the spec sections.
- [ ] Test-first fix per item.
- [ ] Workspace tests and clippy.
- [ ] Commit.

### Task 4: FM synth

**Files:** `crates/acid-engine/src/fm/*`, `params.rs` (P5-T1a), project tests (P5-T8a/c), `THIRD_PARTY_NOTICES.md` if needed

**Items:** P5-T1a, P5-T2a, P5-T2b, P5-T3a, P5-T3b, P5-T4b, P5-T4c, P5-T4d, P5-T5a, P5-T5b, P5-T5c, P5-T7a, P5-T7b, P5-T7c, P5-T8a, P5-T8c

**Decision #10:** FM mono keeps one shared key stack. This is a doc fix only.

**Note:** P5-T8a ("compat test strips RON lines; use a fixture") pairs with the sampler compat test at `project.rs`. Add a small checked-in Plan-4-era RON fixture (no `voice_name`, `bank` or `slots` fields) under `crates/acid-engine/tests/fixtures/` or as a `const &str`, and use it for both the FM and sampler backward-compatibility tests.

- [ ] Read the spec sections and decision.
- [ ] Test-first fix per item. P5-T5b (stale filter or phaser state when toggled) is on the audio path: reset the state on the off→on edge without allocating.
- [ ] Workspace tests, including `no_alloc`, and clippy.
- [ ] Commit.

### Task 5: Sampler engine

**Files:** `crates/acid-engine/src/sampler/*` (voice, machine, decode, slot), `params.rs` tests (P6-T1b)

**Items:** P6-T1b, P6-T2a, P6-T3a, P6-T3b, P6-T3c, P6-T3d, P6-T4a, P6-T4b, P6-T4c, P6-T5a, P6-T6c, P6-T6d, P6-Fa, P6-Pa (engine side), P6-Pd

**Decisions:**
- **#13:** A truncated file loads with a warning. Add `Decoded.truncated: bool` (or a warning field) that the app shows in Task 9.
- **#14:** `swap_play` must not kill a slot's voices unless the zone layout (key range or mode) changed. Voices keep their current segment, and the new markers apply to the next notes. The existing kill-on-swap test must be updated to cover both cases.
- **#16:** A stolen voice fades out over about 2 ms. Reuse the FM-style declick: carry the old voice's last output and decay it.

**Note:** P6-Fa is Level/Pan smoothing on sounding voices. Use a short per-sample ramp toward the target (one-pole or linear over ~5 ms), with no allocation. Keep `level_changes_reach_a_sounding_voice` passing, with a tolerance if needed.

- [ ] Read the spec sections and decisions.
- [ ] Test-first fix per item.
- [ ] Workspace tests, including `no_alloc`, and clippy.
- [ ] Commit.

### Task 6: Document, undo and files

**Files:** `crates/app/src/document.rs`, `ui/files.rs`, `main.rs`/`audio.rs` if an item lives there

**Items:** P2-T4a, P2-T4b, P2-T4c, P2-T4d, P2-T5a, P2-T11a, P2-T11b, P2-T11d, P2-T11e, P5-T9a, P5-T9b, P6-Pb, P6-Pc

**Decisions:**
- **#1:** Undo back to the saved point clears the dirty flag. Track the undo depth or generation at save time; redo back to it also counts.
- **#4:** Always append `.sidacid` unless the name already ends with it, case-insensitively.
- **#15:** Saving stays on the UI thread, with a "Saving…" status shown first. That needs one frame to paint the status before the blocking save. For example, set `status_line`, request a repaint and run the save on the next frame.

**Note:** for P6-Pb (undo memory), cap the undo history by approximate bytes as well as by count. For example, drop the oldest entries while the total size of `SlotData` held in undo and redo exceeds 128 MB. Pick a simple, documented budget.

- [ ] Read the spec sections and decisions.
- [ ] Test-first fix per item.
- [ ] Workspace tests and clippy.
- [ ] Commit.

### Task 7: App shell: pump, knobs, step editor, record, song and synth panels

**Files:** `crates/app/src/ui/mod.rs`, `knob.rs`, `params_ui.rs`, `step303.rs`, `step_edit.rs`, `record.rs`, `song_panel.rs`, `synth_panel.rs`, `drum_panel.rs`, `transport.rs`, `mixer_panel.rs`

**Items:** P2-T2b, P2-T2c, P2-T3d, P2-T6d, P2-T7a, P2-T7b, P2-T7c, P2-T7d, P2-T9a, P2-T9b, P2-T10b, P2-T10c, P3-T7a, P3-T7b, P3-T7c, P3-T7d, P3-T8c, P3-T9b, P3-T9c, P4-T7a, P4-T10a

**Decisions:**
- **#2:** The step-writer stops when another pattern is selected.
- **#6:** Switch and BPM edits during a record pass merge into that pass's undo step.

**Note:** P2-T7d (scroll gestures closing on the first idle frame) needs a short debounce, about 300 ms of no wheel input, before `end_gesture`. Apply it to both the step-editor scroll and the knob scroll paths.

- [ ] Read the spec sections and decisions.
- [ ] Test-first fix per item. Headless UI tests are fine; extract pure helpers where a behaviour can't be driven headless.
- [ ] Workspace tests and clippy.
- [ ] Commit.

### Task 8: Piano rolls, note panel and FM panel

**Files:** `crates/app/src/ui/pianoroll.rs`, `roll_edit.rs`, `polyroll.rs`, `poly_edit.rs`, `note_panel.rs`, `fm_panel.rs`, `fm_voices.rs`, `alg_diagram.rs`

**Items:** P2-T8a, P2-T8b, P2-T8c, P4-Pb, P4-T8a, P4-T8b, P4-T9a, P4-T9b, P4-T9c, P4-T9d, P5-T8b, P5-T10a, P5-T10b, P5-T10c

**Decisions:**
- **#3:** A 303-roll drag doesn't erase notes it passes over. Drag against a snapshot taken at drag start, and only the drop replaces overlapped notes.
- **#9:** Same-pitch overlaps created by a poly-roll move or resize merge on drop. A note hidden past a shortened end is not deleted by an overlapping add.
- **#11:** The FM browser highlights the loaded voice by its stored source. Store `(browser, index)` in the view when a voice is loaded; clear it on any manual FM param edit, or keep it until the next load. Choose and document.

**Note:** also check `polyroll.rs` for the same lane hit-test bug as P2-T8a, as the spec notes.

- [ ] Read the spec sections and decisions.
- [ ] Test-first fix per item, using pure helpers in `roll_edit`/`poly_edit` where possible.
- [ ] Workspace tests and clippy.
- [ ] Commit.

### Task 9: Sampler panel

**Files:** `crates/app/src/ui/sampler_panel.rs`, `sampler_load.rs`, `sampler_ui.rs`

**Items:** P6-Fb, P6-T7c, P6-T8a, P6-T9a, P6-T9b, P6-T9c, plus the app side of P6-Pa (#14) and P6-T6c (#13: show the truncation warning)

**Notes:**
- Task 5 has already changed the engine so that meta swaps don't kill voices. Here, make sure marker drags still send `SamplerPlay` and are one undo step per drag.
- P6-T9d (the manual check) can't be done by an agent. List it as owed to the user in the report.

- [ ] Read the spec sections.
- [ ] Test-first fix per item.
- [ ] Workspace tests and clippy.
- [ ] Commit.

### Task 10: Docs sweep and closing checklist

**Files:** any remaining DOC items. Also `docs/superpowers/plans/*-deferred.txt`.

**Items:** every DOC or TEST item in the spec's "tests/docs" section not already owned by Tasks 1–9: P2-T11e, P2-T2b, P2-T2c, P2-T3d, P2-T8c, P2-T9a, P5-T8a, P6-T1b, P6-T5a, P2-T10c. Skip any that an earlier task already completed, and confirm with git log or grep.

- [ ] Build a checklist of every STILL-APPLIES ID in the spec. Check each against the Task 1–9 reports in the SDD workspace and against the code, and fix any stragglers.
- [ ] Append a line to each `*-deferred.txt` file: "Resolved in Plan 7 (backlog cleanup), see docs/superpowers/plans/2026-10-12-sidacid-plan7-backlog.md", plus any item deliberately left (with reason).
- [ ] Workspace tests and clippy.
- [ ] Commit.

---

## Self-review notes

- The ID lists cover every STILL-APPLIES item from the triage by area. Task 10's checklist catches any item missed by parsing.
- Task ordering: Task 5 (the engine stops killing voices on a meta swap) comes before Task 9 (the panel relies on it). Task 4's RON fixture is reused by sampler compat tests; Task 5 may use it.
- Each user decision maps to exactly one task.
