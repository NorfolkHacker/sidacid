# SidAcid backlog cleanup — design

Date: 2026-10-12
Status: approved (user accepted all 16 recommended decisions)

Scope: every deferred minor finding from Plans 2–6
(`docs/superpowers/plans/*-deferred.txt`). The triage below checked each one
against master at 0ca69db. **Fix every item whose status is STILL-APPLIES.**
ALREADY-FIXED and OBSOLETE items need no work.

## Accepted decisions

| # | Item(s) | Decision |
|---|---|---|
| 1 | P2-T4d | Undoing back to the saved point clears the dirty flag (track a saved undo depth / generation). |
| 2 | P2-T7b | The 303 step-writer stops when another pattern is selected. |
| 3 | P2-T8b | Dragging a note over others in the 303 piano roll doesn't erase them mid-gesture: the drag works on a snapshot of the pattern at drag start, and only the final drop replaces overlapped notes. |
| 4 | P2-T11a | Saving always appends `.sidacid` unless the name already ends with it (case-insensitive). |
| 5 | P3-T2b | Keep 303-like behaviour: a slide or tie on a bar's last step runs into whatever plays next. Document it and pin it with a test. |
| 6 | P3-T7a | Switch and BPM edits made during a record pass merge into that pass's single undo step. |
| 7 | P3-T8d | Inserting a bar inside the loop, or right at its end, extends the loop to include it. |
| 8 | P4-T5a | Play and Stop release only pattern voices; a held audition keeps sounding. (Stop-twice still hard-silences everything.) |
| 9 | P4-T8b, P4-Pb | Same-pitch overlaps created by moving or resizing a poly-roll note are merged on drop. A note hidden past a shortened pattern end is kept, not deleted by an overlapping add. |
| 10 | P5-T4b | FM mono mode keeps one shared key stack for audition and pattern; document it. |
| 11 | P5-T10b | The FM browser highlights the loaded voice by its stored source (bank + index), not by name. |
| 12 | P5-Fa | The first Stop after a song ends by itself only releases notes; only a deliberate second Stop cuts sound. |
| 13 | P6-T6c | A truncated audio file loads what decoded, with a "the file ended early" warning message. |
| 14 | P6-Pa | Marker and slice edits no longer kill a slot's sounding voices unless the slot's zone layout (key range or mode) changes; edits apply to the next notes. |
| 15 | P6-Pc | Saving stays on the UI thread, with a "Saving…" status shown first. |
| 16 | P6-Pd | A stolen sampler voice fades out over about 2 ms (declick), like the FM synth. |

## Constraints

- **Real-time rule:** `Engine::process`, everything it calls, and `EngineHost::process` never allocate, lock or do I/O. `crates/acid-engine/tests/no_alloc.rs` keeps passing.
- `cargo test --workspace` passes, and `cargo clippy --workspace --all-targets` reports 0 warnings.
- Each behaviour or robustness fix gets a test that fails before it and passes after.
- Test items add the missing test. Doc items fix the comment.
- `PROJECT_VERSION` stays 2, and files saved by earlier builds still load.

---

# SidAcid deferred-findings backlog triage

Source: docs/superpowers/plans/2026-10-1{1,2}-sidacid-plan{2..6}-deferred.txt, checked against master @ 0ca69db (read-only; no cargo run). IDs: P<plan>-T<task><letter>; P = parked, F = final-wave minor.

## Summary

| Status \ Category | BEHAVIOUR | ROBUSTNESS | TEST | DOC | Total |
|---|---|---|---|---|---|
| STILL-APPLIES | 35 | 23 | 50 | 10 | 118 |
| ALREADY-FIXED | 9 | 7 | 2 | 7 | 25 |
| OBSOLETE | 1 | 0 | 0 | 0 | 1 |
| **Total** | 45 | 30 | 52 | 17 | 144 |

| Area | Items | Still applies |
|---|---|---|
| engine/sequencer/song | 26 | 23 |
| FM | 17 | 15 |
| sampler | 16 | 14 |
| app UI/document | 61 | 48 |
| randomiser/render | 8 | 8 |
| tests/docs | 16 | 10 |

Already fixed / obsolete: P2-T6a, P2-T6b, P2-T6c, P2-T6e, P2-T6f, P2-T7e, P2-T10a, P2-T11c, P3-T1a, P3-T1b, P3-T8a, P3-T8b, P3-T9a, P4-T3b, P4-T3c, P4-T6a, P4-Pa, P5-T2c, P5-T4a, P6-T1a, P6-T5c, P6-T6a, P6-T6b, P6-T7a, P6-T7b, P6-T8b

Grouping: each entry carries "Group with:" in Risk/notes; cross-area groups are listed under "Fix groups" at the end.

## engine/sequencer/song (26)

### P2-T1a — No test for a pattern's lane param changing between patterns
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/sequencer.rs:486-494 (the `old != id` restore branch); tests at 967-1012
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** Add a sequencer test. Pattern A has a lane on Cutoff at step 0. Pattern B has a lane on Resonance at step 0. Select B while A is queued, then assert `ParamRestore{Cutoff}` comes before `Param{Resonance}` at the B boundary.
- **Test:** `cargo test -p acid-engine lane_param_change_restores_the_old_param`
- **Size:** S
- **Risk/notes:** Tests only. Group with: P2-T1b.

### P2-T1b — No test for both synths' lanes on the same param
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/sequencer.rs:431-500 (per-track `lane_override`, loop s=0..2)
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** Add a test where synth 0 and synth 1 lane the same shared param, for example `ParamId::Fx(..)`. The test should pin down the current last-writer-wins behaviour. In one step, synth 1's `ParamRestore` (it has no value there) comes after synth 0's `Param`, so synth 0's value is lost for that step. Either document this as intended, or make the restore skip an id that another track's `lane_override` still holds.
- **Test:** Sequencer test that checks the event order for a shared-param lane on both synths.
- **Size:** S
- **Risk/notes:** Lanes only come from loaded files (the app has no lane editor, `is_valid` is the only check in project.rs:465), so the impact is small. Group with: P2-T1a.

### P2-T2a — Muted/unsoloed channels meter 0 (undocumented)
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/mixer.rs:119-123 (doc), 180-185 (`continue` before the meter update)
- **Category:** DOC
- **Area:** engine/sequencer/song
- **Proposed fix:** Extend the `take_meters` doc: "muted (or soloed-out) channels read 0". Also add the same note to `Status::meters` in host.rs:93.
- **Test:** None (doc). Optionally assert m[c]==0 for a muted channel.
- **Size:** S
- **Risk/notes:** Group with: P2-T2b.

### P2-T3a — Status cadence is quantised to the block size (~43/s at 512)
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/host.rs:157-161 (`since_status = 0` drops the remainder)
- **Category:** ROBUSTNESS
- **Area:** engine/sequencer/song
- **Proposed fix:** Keep the remainder: `self.since_status -= self.status_every;`, clamped with `.min(self.status_every)` so one huge block cannot burst. This averages 60 Hz.
- **Test:** Tighten `status_arrives_about_sixty_times_a_second` to (58..=61) with 512-sample blocks.
- **Size:** S
- **Risk/notes:** Real-time path; no allocation. Group with: P2-T3b.

### P2-T3b — Host state is not reset on ReplaceEngine
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/host.rs:211-214
- **Category:** ROBUSTNESS
- **Area:** engine/sequencer/song
- **Proposed fix:** In the `ReplaceEngine` arm, after the swap, set `self.meters = [0.0; CHANNELS+1]; self.guard_fired = false; self.since_status = 0;`. Without this, a stale guard flag shows "Audio fault recovered" for the newly loaded project.
- **Test:** Host test: set `host.guard_fired = true`, push ReplaceEngine, run, and assert the next Status has `guard_fired == false` and zero meters.
- **Size:** S
- **Risk/notes:** Real-time path; plain stores only. Group with: P2-T3a.

### P2-T3c — Statuses share the event ring with disposals (UI must drain every frame)
- **Status:** STILL-APPLIES (mitigated: `DISPOSAL_RESERVE` in host.rs:20-22 from e1d76b5; the UI drains every frame in ui/mod.rs:206-219)
- **Location:** crates/acid-engine/src/host.rs:1-5, 118-120
- **Category:** DOC
- **Area:** engine/sequencer/song
- **Proposed fix:** Document on `channel()`: "drain the Consumer<Event> regularly; while it is stalled only the last DISPOSAL_RESERVE slots are kept for disposals, and boxes beyond that leak".
- **Test:** None (doc).
- **Size:** S
- **Risk/notes:** None.

### P3-T1c — write_automation's range loop has no upper bound
- **Status:** STILL-APPLIES. `for step in a..=b` inserts every step with no cap (song.rs:124-126). The caller record.rs:100-105 fills from `rec_last + 1` up to the current `song_step`. If `song_step` jumps forward (for example, `apply_resync` after an edit that changes bar lengths), it fills the whole gap. A bad `from`/`to` such as 0..u32::MAX would allocate billions of BTreeMap entries.
- **Location:** crates/acid-engine/src/song.rs:124-126; crates/app/src/ui/record.rs:100-105
- **Category:** ROBUSTNESS
- **Area:** engine/sequencer/song
- **Proposed fix:** In `write_automation`, clamp `b` to `a.saturating_add((MAX_BARS * STEPS) as u32)` (the largest song is 999×64 steps). Optionally, in `record_automation`, only gap-fill when `step - s` is small (for example ≤ 4). Otherwise start at `step`.
- **Test:** Call `write_automation(id, 0, u32::MAX, 0.5)` and assert it returns quickly with `points.len() <= MAX_BARS*STEPS + 1`.
- **Size:** S
- **Risk/notes:** UI-thread only, not the real-time path.

### P3-T1d — test gaps: bar_steps with a bad slot or length; write_pattern_ref from None
- **Status:** STILL-APPLIES (partly). "pattern_ref from None" is now covered by `recorded_pattern_change_reaches_the_sampler` (song.rs:479-490, bars default to None; added in 266e103). No test covers `bar_steps` or `Banks::length` with a slot ≥ bank_len, `length == 0`, or `length > STEPS`. The code handles them: filter_map None and clamp(1, STEPS) at song.rs:55-63 and 67-77.
- **Location:** crates/acid-engine/src/song.rs:54-77 (tests at 284-291)
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** Add `bar_steps_ignores_bad_slots_and_clamps_lengths`. It uses a bar that refs slot 200 → DEFAULT_BAR_STEPS, a pattern with length 0 → 1, and length 255 → STEPS.
- **Test:** The new unit test in song.rs.
- **Size:** S
- **Risk/notes:** None.

### P3-T2a — test gaps: song-end NoteOff, silent-track lane restore, loop-jump auto_sync, select ignored in song
- **Status:** STILL-APPLIES (partly). Already fixed in 6a29556: loop-jump auto_sync (`loop_jump_resets_lanes_without_a_point_yet`, sequencer.rs:1201) and select-ignored-in-song (`selection_during_song_playback_is_kept_for_pattern_mode`, sequencer.rs:1165). The poly song-end release is covered by `a_song_ending_releases_held_notes` (sequencer.rs:1433). Still untested:
  - the 303 `NoteOff{synth}` on song end (load_bar, sequencer.rs:291-296)
  - `ParamRestore` when a track turns silent with a lane override (sequencer.rs:433-442)
- **Location:** crates/acid-engine/src/sequencer.rs:290-302, 432-443
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** Add two song-mode tests:
  - (1) A one-bar song whose synth pattern has a step-15 Tie into step 0, so the gate is open at the end. Assert that `SeqEvent::NoteOff{synth:0}` is the last synth event.
  - (2) Bar 1 plays a synth pattern with a param lane value on its last step, then bar 2 is None/muted. Assert `ParamRestore{id}` at the start of bar 2.
- **Test:** As above.
- **Size:** M
- **Risk/notes:** Group with: P3-T2b (same fixtures).

### P3-T2b — slide/tie can hold across a bar boundary (untested)
- **Status:** STILL-APPLIES. On quarter 2, the hold decision looks at `pat.steps[(t.step+1) % len]` of the *current* pattern (sequencer.rs:536-545). It never looks at the pattern the next bar (or a queued switch) loads. Consequences:
  - A Tie on step 0 of the current pattern holds the gate even if the next bar is silent. The note is then cut a half step late, at the next bar's start.
  - If the next bar's pattern starts with Tie, the old pitch keeps sounding.
- **Location:** crates/acid-engine/src/sequencer.rs:529-547
- **Category:** BEHAVIOUR
- **Area:** engine/sequencer/song
- **Proposed fix:** **USER DECISION.** Recommended: accept the current 303-like behaviour (a slide or tie wraps into whatever plays next) and add tests that pin it down. ALT: on the last step of a bar (`bar_step == bar_len-1` in song mode), or when a queued switch is due, compute `next` from the next bar's or queued pattern's step 0 (silent → no hold). `tick` already has `song` and `banks`.
- **Test:** A song with bar 1 whose synth pattern ends in slide→Tie and bar 2 muted. Assert when NoteOff happens (half step vs. bar start).
- **Size:** M (ALT); S (tests only)
- **Risk/notes:** Real-time path: tick must not allocate (no_alloc.rs). Group with: P3-T2a.

### P3-T3a — no host tests for Touch/SetParamLive routing
- **Status:** STILL-APPLIES. host.rs:221-222 routes `Touch` and `SetParamLive` to the engine, but host tests (host.rs:266-444) never push either. Engine-level tests exist only for automation restore.
- **Location:** crates/acid-engine/src/host.rs:221-222
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** Add a host test:
  - Push `SetParamLive(id, 0.3)` and run, then assert the engine's live/base value for `id` is 0.3 (via an existing accessor or a `#[cfg(test)]` getter).
  - Push `Touch(id,true)` with a song automation point for `id` and play in song mode, then assert the value stays put. After `Touch(id,false)` it follows automation again.
- **Test:** As above.
- **Size:** M
- **Risk/notes:** May need a test-only getter for `Engine::base`.

### P3-T3b — a rejected song swap is silent to the UI
- **Status:** STILL-APPLIES. host.rs:216-219 ignores the bool from `Engine::swap_song` (engine.rs:285-300) and returns the box as `DisposeSong` either way. The UI cannot tell. In practice this is unreachable: UI edits are bounded (insert_bars cap, write_pattern_ref guards), and loaded projects pass `Project::validate` (project.rs:441, song lanes at 551).
- **Location:** crates/acid-engine/src/host.rs:216-219; crates/acid-engine/src/engine.rs:285-300
- **Category:** ROBUSTNESS
- **Area:** engine/sequencer/song
- **Proposed fix:** Add `song_rejected: bool` to `Status`. Set it sticky in the host when `swap_song` returns false, and clear it after a successful status push, as `guard_fired` does. The app shows an error line or `debug_assert!`s. This is a cheaper option than a new Event variant.
- **Test:** Host test: push an over-cap Song and assert the next Status has `song_rejected == true`.
- **Size:** M
- **Risk/notes:** Real-time path: a Copy bool only, no allocation.

### P3-T8d — loop behaviour when inserting inside the loop is unspecified
- **Status:** STILL-APPLIES. `insert_bars` (song.rs:178-182) bumps each loop endpoint with `i >= at`. Results by insert position:
  - Inside (a < at ≤ b): the loop grows.
  - At the start (at == a): the loop shifts, and the new bar falls outside it.
  - After b: unchanged.

  Only the shift case is tested (`insert_shifts_automation_and_loop`, song.rs:360-373).
- **Location:** crates/acid-engine/src/song.rs:167-183
- **Category:** BEHAVIOUR
- **Area:** engine/sequencer/song
- **Proposed fix:** **USER DECISION.** Recommended: keep the current rule (inserting strictly inside the loop extends it; inserting at or before the start shifts it). Document it in the doc comment and add a test. ALT: inserting at `at == a` or `at == b+1` also extends the loop. That means bumping only `b` when `a <= at <= b+1`, so both ends are inclusive.
- **Test:** Loop (1,3): insert at 2 → (1,4); insert at 1 → (2,4) under the current rule.
- **Size:** S
- **Risk/notes:** Paste and Duplicate go through insert_bars, so they follow the same rule. Group with: P3-T8c.

### P4-T2a — NotePattern::ticks clamps length although patterns are validated
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/project.rs:133-135
- **Category:** DOC
- **Area:** engine/sequencer/song
- **Proposed fix:** No code change; the clamp is cheap and protects against app-side `NotePattern` values that never went through `is_well_formed` (fields are pub). At most add a one-line comment ("defensive: fields are pub"). Recommend closing as won't-fix.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** Real-time path (called each tick in `Sequencer::note_ticks`), so do not replace it with a panic. Group with: P4-T3a.

### P4-T3a — sequencer `len_of` duplicates the `Banks::length` clamp
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/sequencer.rs:116-118 (used at 445, 509, 536); crates/acid-engine/src/song.rs:62; also crates/acid-engine/src/random.rs:57-59 (`length_of`, the same clamp a third time)
- **Category:** DOC
- **Area:** engine/sequencer/song
- **Proposed fix:** Add `pub fn steps_of(length: u8) -> usize { (length as usize).clamp(1, STEPS) }` in project.rs. Use it in `Banks::length`, replace `len_of` with it, and replace `random::length_of` with it as well.
- **Test:** Existing sequencer tests (`pattern_length_wraps_and_polymeter`) still pass.
- **Size:** S
- **Risk/notes:** Pure refactor, no allocation. Group with: P4-T2a.

### P4-T4a — no test of note tracks under swing
- **Status:** STILL-APPLIES (the only swing test is `swing_delays_odd_steps`, sequencer.rs:781, and it covers the drum/synth steps)
- **Location:** crates/acid-engine/src/sequencer.rs:183-191 (step_len), 587-640 (note_ticks); tests at 1342+
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** Add `poly_notes_follow_swing`. With swing 0.5, put a note at tick `TICKS_PER_STEP` (step 1). Assert its PolyOn sample offset equals the odd step's swung start (the same offset at which a drum hit on step 1 fires).
- **Test:** `cargo test -p acid-engine poly_notes_follow_swing`
- **Size:** S
- **Risk/notes:** Group with: P4-T4b.

### P4-T4b — no test of note tracks when the pattern shrinks mid-play
- **Status:** STILL-APPLIES (the wrap in `note_ticks` at sequencer.rs:595-600, `pos >= ticks` → 0 plus `release_all`, has no test; `shortening_a_note_behind_the_playhead_releases_it` at 1489 shortens a note, not the pattern)
- **Location:** crates/acid-engine/src/sequencer.rs:595-600
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** Add `shrinking_the_pattern_past_the_playhead_wraps_and_releases`, using the existing `after_live_edit` helper (line 1459) or a variant. Play into step 10 of a 16-step pattern with a held note, set `length = 4`, then tick once. Expect a PolyOff for the held pitch and the PolyOn of the note at tick 0.
- **Test:** `cargo test -p acid-engine shrinking_the_pattern`
- **Size:** S
- **Risk/notes:** Group with: P4-T4a.

### P4-T5a — play/stop also release auditioned (live) notes
- **Status:** STILL-APPLIES (`Engine::play` and `end_playback` call `fm.all_off()` / `sampler.all_off()`, which release every voice, including those with `live == true`)
- **Location:** crates/acid-engine/src/engine.rs:184-189, 225-242; crates/acid-engine/src/fm/synth.rs:151-156; crates/acid-engine/src/sampler/machine.rs:165-170
- **Category:** BEHAVIOUR
- **Area:** engine/sequencer/song
- **Proposed fix:** Recommended: add `pattern_off()` to FmSynth and Sampler, which releases only voices with `!v.live`. Call it from `play()` and `end_playback()`. Keep `all_off()` for song end, and keep `reset()` for the second Stop. ALT: keep the current behaviour (transport press is a "panic" for everything), and only document it. **USER DECISION**
- **Test:** Engine test: `note_on(4,60,100)`, `play()`, render 0.3 s, assert the tail is still loud. Repeat with `stop()` (first stop).
- **Size:** S
- **Risk/notes:** Real-time path, no allocation (simple loop). FM mono mode keeps a shared stack, so `pattern_off` would have to rebuild the stack, or mono could keep releasing everything. Interaction: the app's held-key state (`view.audition`, `view.pad_note`) already sends its own NoteOff later, so no stuck notes either way.

### P4-Fa — (Final note for Plans 5–6) real voices must keep release-all-voices-of-pitch on note_off (overlapping same-pitch double PolyOn)
- **Status:** STILL-APPLIES as a missing regression test; the behaviour itself is verified. FM poly `note_off` releases every active held voice with that pitch and `live` flag (fm/synth.rs:143-147). Sampler `note_off` does the same by key (sampler/machine.rs:157-162). The sequencer keeps one held bit per pitch and emits a single PolyOff (sequencer.rs:606-617, 620-628). No test exercises two same-pitch PolyOns followed by one PolyOff.
- **Location:** crates/acid-engine/src/fm/synth.rs:131-149; crates/acid-engine/src/sampler/machine.rs:156-163; crates/acid-engine/src/sequencer.rs:600-628
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** Add `fm_double_note_on_of_a_pitch_releases_both_on_one_off` to the fm/synth.rs tests: two `note_on(60,…,false)`, one `note_off(60,false)`, render past the release, and assert `active_voices() == 0` (or that both voices are `!held`). Add the same for the sampler.
- **Test:** `cargo test -p acid-engine double_note_on`
- **Size:** S
- **Risk/notes:** The sampler half duplicates P6-T4 ("no test of two same-key voices from one source released by one note_off"). Group with: that P6 item, P4-T8b, P4-Pb.

### P5-T6a — no test that an audition note_off leaves pattern voices sounding
- **Status:** STILL-APPLIES (only the other direction is tested: engine.rs:1236 and synth.rs:292-303 check that a pattern release leaves the audition note)
- **Location:** crates/acid-engine/src/engine.rs:1235-1257; crates/acid-engine/src/fm/synth.rs:292-303
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** In the synth test, add `note_on(60, false); note_on(60, true); note_off(60, true)` and assert `active_voices() == 1` with the pattern note held. Optionally do the same at engine level: play a 32-step pattern note, then `e.note_on(4,60,..)` and `e.note_off(4,60)`, and check it still sounds.
- **Test:** As described.
- **Size:** S
- **Risk/notes:** —

### P5-T6b — exact `peak == 0.0` assertion is brittle
- **Status:** STILL-APPLIES (`assert_eq!(peak(&l), 0.0, "OP1 ... at level 0")`)
- **Location:** crates/acid-engine/src/engine.rs:1274
- **Category:** TEST
- **Area:** engine/sequencer/song
- **Proposed fix:** Change it to `assert!(peak(&l) < 1e-6, ...)`.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** Other exact-zero peaks exist (engine.rs:622, 786, 793, 834, 973, 1118, synth.rs:289, 505). Those check silence after reset/mute, where exact zero is the intended invariant; leave them.

### P5-Fa — Stop after a song ends by itself hard-cuts the FM tail (stop-twice semantics)
- **Status:** STILL-APPLIES. `Engine::stop` resets the FM and sampler whenever the sequencer isn't playing. When the song ends, `end_playback` already stopped it, so the user's first Stop press does a hard reset. record.rs:36 and :79 also send `Command::Stop` while stopped, which cuts tails the same way.
- **Location:** crates/acid-engine/src/engine.rs:222-245, 488-490; crates/app/src/ui/record.rs:36, 79
- **Category:** BEHAVIOUR
- **Area:** engine/sequencer/song
- **Proposed fix:** Recommended: add `stop_armed: bool` to `Engine`. Set it to true at the end of `stop()` and to false in `play()`; don't set it in the song-end `end_playback`. In `stop()`, reset only if `!is_playing() && self.stop_armed`, so the first user Stop after a song ends just releases notes and the second one cuts. **USER DECISION** ALT: keep the current behaviour and document that the song end counts as the first Stop. Separately, decide whether the record.rs mode-switch Stops should use a non-cutting command.
- **Test:** Let a 1-bar song with a long FM release end, call `stop()` once, and check the tail still sounds; call `stop()` again and check silence.
- **Size:** S
- **Risk/notes:** Real-time path; a bool is fine. Check the Plan 6 sampler stop tests (they rely on the same stop-twice reset). This may interact with Plan 3/4 song-end items.

### P6-T5b — Engine::stop() doc comment omits the sampler reset
- **Status:** STILL-APPLIES (doc mentions only the FM machine; code also calls `self.sampler.reset()`)
- **Location:** crates/acid-engine/src/engine.rs:222-231
- **Category:** DOC
- **Area:** engine/sequencer/song
- **Proposed fix:** "…also silences the FM and Sampler machines at once, so drones and loops can be cut."
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P3-T1a — write_automation lets NaN through the clamp
- **Status:** ALREADY-FIXED (5b39993). crates/acid-engine/src/song.rs:109-111 returns early on a non-finite value. Tested by `non_finite_automation_is_ignored` (song.rs:435-441).
- **Location:** crates/acid-engine/src/song.rs:108-127
- **Category:** ROBUSTNESS
- **Area:** engine/sequencer/song
- **Proposed fix:** None needed.
- **Test:** Existing test `non_finite_automation_is_ignored`.
- **Size:** S
- **Risk/notes:** None.

### P3-T1b — write_pattern_ref panics on machine>=4 and does not check the slot
- **Status:** ALREADY-FIXED (5b39993). song.rs:137-139 ignores `machine >= MACHINES || slot >= PATTERNS`. Tested by `out_of_range_pattern_refs_are_ignored` (song.rs:443-451).
- **Location:** crates/acid-engine/src/song.rs:136-149
- **Category:** ROBUSTNESS
- **Area:** engine/sequencer/song
- **Proposed fix:** None needed.
- **Test:** Existing test.
- **Size:** S
- **Risk/notes:** None.

### P3-T8a — empty lanes are left after a delete
- **Status:** ALREADY-FIXED (5b39993). song.rs:199 `song.automation.retain(|l| !l.points.is_empty())`. Tested by `deleting_all_of_a_lanes_points_removes_the_lane` (song.rs:453-465).
- **Location:** crates/acid-engine/src/song.rs:185-214
- **Category:** ROBUSTNESS
- **Area:** engine/sequencer/song
- **Proposed fix:** None needed.
- **Test:** Existing test.
- **Size:** S
- **Risk/notes:** None.

## FM (17)

### P5-T1a — fm_steps constants not test-tied to info() divisors (31/14 literals)
- **Status:** STILL-APPLIES (defaults still use literals `1.0 / 31.0` for Coarse and `3.0 / 7.0` for PitchModSens; test repeats `1.0 / 31.0`)
- **Location:** crates/acid-engine/src/params.rs:214, 234, 469
- **Category:** TEST
- **Area:** FM
- **Proposed fix:** Write the defaults as `1.0 / (fm_steps::COARSE - 1) as f32` and `3.0 / (fm_steps::PITCH_MOD_SENS - 1) as f32`. In `fm_defaults_are_the_dx7_init_voice`, add asserts that decoding the default gives the DX7 init values, e.g. `step_index(d(Coarse), COARSE as u8) == 1`, `step_index(d(PitchModSens), PITCH_MOD_SENS as u8) == 3`, `step_index(d(Detune), DETUNE as u8) == 7`.
- **Test:** The extended `fm_defaults_are_the_dx7_init_voice` must fail if an fm_steps constant changes without its default changing too.
- **Size:** S
- **Risk/notes:** `cont()` is a const fn, so the expression has to stay const (an `as f32` cast on a const usize is fine). Group with: P5-T3b.

### P5-T2a — algorithm tests don't pin each algorithm's feedback operator
- **Status:** STILL-APPLIES (only alg 5 and alg 18 feedback are spot-checked)
- **Location:** crates/acid-engine/src/fm/algorithms.rs:115-128
- **Category:** TEST
- **Area:** FM
- **Proposed fix:** Add `feedback_operators_match_the_dx7`, which asserts that `ALGORITHMS.map(|a| a.feedback + 1)` equals the full 32-entry list (6,2,6,6,6,6,6,4,2,3,6,2,6,6,2,6,2,3,6,3,3,6,6,6,6,6,3,5,6,5,6,6), checked against the DX7 chart.
- **Test:** The new test itself.
- **Size:** S
- **Risk/notes:** Algorithms 4 and 6 use OP6 self-feedback by design (the module doc says so). Note that in the test.

### P5-T2b — envelope tests are mostly directional
- **Status:** STILL-APPLIES (the env tests only compare slower vs faster or check a level threshold; no absolute timing)
- **Location:** crates/acid-engine/src/fm/env.rs:239-340
- **Category:** TEST
- **Area:** FM
- **Proposed fix:** Add one or two absolute-timing anchors: for example, attack time at R1=99 under a few ms, and full decay time for a known rate (say R=50) within ±10% of the msfa reference value worked out from the rate table.
- **Test:** A new `rate_timing_matches_msfa_reference` test.
- **Size:** S
- **Risk/notes:** Take the reference numbers from msfa's formula, not from the current output, or the test is circular.

### P5-T3a — the S&H LFO holds 0 for its first cycle
- **Status:** STILL-APPLIES (`held: 0.0` in `Lfo::new`; a new value is only drawn on a phase wrap; `reset()` does not redraw)
- **Location:** crates/acid-engine/src/fm/modulation.rs:45-56, 59-63
- **Category:** BEHAVIOUR
- **Area:** FM
- **Proposed fix:** Draw a sample when the cycle starts. In `new()`, set `held` from one `next_random()` step (build the struct first, then call it). In `reset()` (key-sync), also set `self.held = self.next_random()` so each synced note gets a fresh value. Both are allocation-free.
- **Test:** `Lfo::new().tick(1.0, 5, 0.01) != 0.0`; after `reset()` the held value changes.
- **Size:** S
- **Risk/notes:** `reset()` runs inside `note_on` on the audio thread; `next_random` is a pure xorshift, so this is safe there.

### P5-T3b — default-patch test doesn't assert feedback, volume or glide
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/fm/patch.rs:256-270
- **Category:** TEST
- **Area:** FM
- **Proposed fix:** Add `assert_eq!(p.feedback, 0); assert!((p.volume - 0.7).abs() < 1e-6); assert_eq!(p.glide, 0.0);`, plus `filter_on == false` and `chorus_mix == phaser_mix == 0`.
- **Test:** The extended `default_patch_is_the_init_voice`.
- **Size:** S
- **Risk/notes:** Group with: P5-T1a.

### P5-T4b — mono note_off ignores `live` (accepted deviation)
- **Status:** STILL-APPLIES (the mono branch of `note_off` never checks `live`, so a pattern note-off can release an auditioned mono note and the other way round). The module doc at lines 2-4 says a note-off only releases voices from its own source, which is wrong in mono mode.
- **Location:** crates/acid-engine/src/fm/synth.rs:2-4, 131-142
- **Category:** BEHAVIOUR
- **Area:** FM
- **Proposed fix:** Recommended: keep the behaviour (one shared key stack fits a mono synth) and correct the doc to "in poly mode a note-off only releases voices from its own source; mono shares one key stack". **USER DECISION** ALT: track the source per stack entry (`stack: [(u8, bool); 16]`) and have `stack_remove` match on both pitch and source, so audition and pattern notes behave as two keyboards on one voice.
- **Test:** ALT only: in mono, `note_on(60, live=true)`, `note_on(64, false)`, `note_off(64, false)`; the voice glides back to 60 and is still held; then `note_off(60, false)` is a no-op.
- **Size:** S (doc) or M (ALT)
- **Risk/notes:** Real-time path; a fixed array is fine. Group with: P5-T4d, P5-T5c (same module doc).

### P5-T4c — released-voice steal test doesn't check oldest-first among released voices
- **Status:** STILL-APPLIES (`sounding()` filters on `held`, so the test can't tell whether 43 (older) or 45 was stolen first; after both steals both are gone)
- **Location:** crates/acid-engine/src/fm/synth.rs:403-430, 207-214
- **Category:** TEST
- **Area:** FM
- **Proposed fix:** Add a `#[cfg(test)] fn active_pitches(&self) -> Vec<u8>` that includes released voices. After `note_on(80)`, assert that 43 is gone and 45 is still active.
- **Test:** The modified `a_released_voice_is_stolen_before_a_held_one`.
- **Size:** S
- **Risk/notes:** Test-only helper.

### P5-T4d — misleading comment at synth.rs:111
- **Status:** STILL-APPLIES (likely). Line 111 has moved and I can't identify the original comment for certain; at 4b9037d/2c58502, line 111 was code with no comment. The candidate misleading comments today are: (1) the synth.rs module doc lines 2-4 (the own-source claim is false in mono, see P5-T4b); (2) voice.rs:96-97, which says the stolen voice "fades out over ~2 ms" when `DECLICK_SECONDS` is a 0.5 ms time constant.
- **Location:** crates/acid-engine/src/fm/synth.rs:2-4; crates/acid-engine/src/fm/voice.rs:15-16, 96-97
- **Category:** DOC
- **Area:** FM
- **Proposed fix:** Reword the synth module doc as in P5-T4b. Change the voice.rs comment to "fades out with a 0.5 ms time constant (inaudible within ~2 ms)".
- **Test:** n/a
- **Size:** S
- **Risk/notes:** Group with: P5-T4b, P5-T5c.

### P5-T5a — chorus delay line of 4096 aliases above ~204 kHz (add debug_assert)
- **Status:** STILL-APPLIES (max delay = 0.020·sr; with `CHORUS_LEN` = 4096 the read wraps above 204.8 kHz, and there is no guard)
- **Location:** crates/acid-engine/src/fm/fx.rs:10-11, 21-29, 52-55
- **Category:** ROBUSTNESS
- **Area:** FM
- **Proposed fix:** Simplest: in `Chorus::new`, add `debug_assert!(0.020 * sample_rate + 2.0 < CHORUS_LEN as f32)`. Better: size the buffer from the rate in `new()` (`len = ((0.021 * sr) as usize + 2).next_power_of_two()`), store `len`, and use it in `read`/`process`. `new` already runs off the audio thread.
- **Test:** `Chorus::new(384_000.0)` processes a ramp with full depth and the wet output stays a delayed copy (no wrap-around).
- **Size:** S
- **Risk/notes:** The allocation stays in `new`, so no_alloc.rs is unaffected.

### P5-T5b — bypassed filter and phaser keep stale state (click on re-enable)
- **Status:** STILL-APPLIES (the filter only runs when `filter_on`; the phaser only when `phaser_mix > 0`; neither is reset on the off-to-on transition)
- **Location:** crates/acid-engine/src/fm/fx.rs:134-156
- **Category:** BEHAVIOUR
- **Area:** FM
- **Proposed fix:** Track `was_filter_on` and `was_phaser_on` in `FmFx`. On a false-to-true transition, reset that stage (`self.filter = Svf::new(sr)` is a Copy-type rebuild with no allocation; call `self.phaser.reset()`).
- **Test:** Run a tone with the filter on, turn it off for 1 s of tone, turn it back on, and check the first output sample's step is below the normal max step.
- **Size:** S
- **Risk/notes:** Real-time path. Check that `Svf::new` doesn't allocate (it is already called in `reset()`).

### P5-T5c — synth.rs:2 long doc line
- **Status:** STILL-APPLIES (line 2 is 119 chars)
- **Location:** crates/acid-engine/src/fm/synth.rs:1-4
- **Category:** DOC
- **Area:** FM
- **Proposed fix:** Re-wrap the module doc to under 100 columns, together with the P5-T4b wording fix.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** Group with: P5-T4b, P5-T4d.

### P5-T7a — no fixed-byte test pinning the packed bit layout or checksum
- **Status:** STILL-APPLIES (the sysex tests are round-trip only, apart from the header and length)
- **Location:** crates/acid-engine/src/fm/sysex.rs:309-312, 340-405, 571-597
- **Category:** TEST
- **Area:** FM
- **Proposed fix:** Add `packed_layout_is_the_dx7s`. Build a voice with distinct field values, then assert specific bytes of `pack_voice`, e.g. b[11] = (rc<<2)|lc, b[12] = (det<<3)|rs, b[15] = (coarse<<1)|fixed, b[110] = alg, b[111] = (sync<<3)|fb, b[116] = (pms<<4)|(wave<<1)|lfo_sync. Also assert `checksum(&[1,2,3]) == 122` and the last data byte of a known INIT bank.
- **Test:** The new test.
- **Size:** M
- **Risk/notes:** Take the expected byte layout from the DX7 sysex spec, not from the code.

### P5-T7b — `from_params` doesn't clamp the name
- **Status:** STILL-APPLIES (`v.name = name.to_string()` with no filter or length limit; `is_valid` would then fail on a long or non-ASCII voice name)
- **Location:** crates/acid-engine/src/fm/sysex.rs:255-263
- **Category:** ROBUSTNESS
- **Area:** FM
- **Proposed fix:** Finish with `v.clamped()` instead of `v` (clamping is a no-op on the other fields, which are already in range). Alternatively, factor the name rule out of `clamped()` into `fn clean_name(&str) -> String` and use it in both places.
- **Test:** `from_params("ÄBCDEFGHIJKLMNOP  ", ..).name == "BCDEFGHIJK"` and `.is_valid()`.
- **Size:** S
- **Risk/notes:** Group with: P5-T7c.

### P5-T7c — names test doesn't cover non-ASCII or over-length names
- **Status:** STILL-APPLIES (only "PIANO   " is tested)
- **Location:** crates/acid-engine/src/fm/sysex.rs:633-639
- **Category:** TEST
- **Area:** FM
- **Proposed fix:** Extend the test to check that "ÉLAN PIANO EXTRA" round-trips through pack/unpack and `clamped()` to a name of 10 or fewer printable-ASCII characters, and that a byte of 0x7F or 0x00 in packed b[118..128] decodes as a space or is trimmed.
- **Test:** The extended `names_are_ascii_and_trimmed`.
- **Size:** S
- **Risk/notes:** Group with: P5-T7b.

### P5-T8c — `starter_bank` lacks a doc comment
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/fm/starter.rs:51
- **Category:** DOC
- **Area:** FM
- **Proposed fix:** Add `/// The 16 starter voices (original, not Yamaha ROM), in browser order. Allocates: build once off the audio thread.`
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P4-Pa — (parked) SineVoices note_off also cuts a held audition of the same pitch
- **Status:** ALREADY-FIXED. SineVoices were removed (FM in 4dbe52e, Sampler in 434cce7). The real engines filter on `v.live == live`: fm/synth.rs:143-147 and sampler/machine.rs:156-162. Tests: engine.rs:1236 `audition_survives_a_pattern_release_of_the_same_pitch` and sampler/machine.rs:292 `audition_and_pattern_release_separately`.
- **Location:** crates/acid-engine/src/fm/synth.rs:131-149; crates/acid-engine/src/sampler/machine.rs:156-163
- **Category:** BEHAVIOUR
- **Area:** FM
- **Proposed fix:** none for poly. Residual: in FM **mono** mode `note_off` ignores `live` (synth.rs:132-141, shared stack), so a pattern note_off of the auditioned pitch still cuts the audition. This is inherent to a mono voice; document it, don't fix it.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** P5 deferred has "no test that audition note_off leaves pattern voices" (the reverse direction). Group with: that P5 item.

### P5-T4a — Vintage clamps the mix to ±1 (clean doesn't)
- **Status:** ALREADY-FIXED (commit f7ca8f8: Vintage now quantises across ±`VINTAGE_RANGE` (4.0), and the test `vintage_does_not_clip_chords` covers it)
- **Location:** crates/acid-engine/src/fm/synth.rs:17-18, 189-194, 447-473
- **Category:** BEHAVIOUR
- **Area:** FM
- **Proposed fix:** None. The remaining clamp at ±4 is deliberate 12-bit DAC headroom.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

## sampler (16)

### P4-T5b — voice steal clicks (placeholder only)
- **Status:** STILL-APPLIES for the sampler only. FM is ALREADY-FIXED: the declick is at crates/acid-engine/src/fm/voice.rs:99, with tests `stealing_does_not_click` / `poly_steal_does_not_click` at fm/synth.rs:367+. The sampler's `Voice::start` (sampler/voice.rs:141-172) hard-restarts a stolen active voice: env is reset to 0 and pos jumps.
- **Location:** crates/acid-engine/src/sampler/voice.rs:141-172; crates/acid-engine/src/sampler/machine.rs:141-153
- **Category:** BEHAVIOUR
- **Area:** sampler
- **Proposed fix:** Mirror FM. Track `last_out: [f32; 2]` in the sampler Voice. In `start`, if `self.active`, set `declick = last_out`, then add a ~2-3 ms decaying declick offset in `render`.
- **Test:** Sampler test like `poly_steal_does_not_click`: fill 16 voices, steal one, and assert the first-sample jump is under 1.25 × the normal max step.
- **Size:** M
- **Risk/notes:** Real-time path; must stay allocation-free (no_alloc.rs). Duplicates the P6 Final parked item ("voice steal hard-restarts"). Group with: that P6 parked item.

### P6-T2a — SlotData::to_sample relies on is_valid (pcm divisible by channels)
- **Status:** STILL-APPLIES (to_sample never checks; with an odd stereo pcm left/right get different lengths, channels>2 silently drops right). No panic: `SampleData::read` clamps each side independently.
- **Location:** crates/acid-engine/src/sampler/slot.rs:223-239
- **Category:** ROBUSTNESS
- **Area:** sampler
- **Proposed fix:** in to_sample iterate only `self.pcm[..frames*ch]` (whole frames) and add `debug_assert!(self.is_valid())`; doc comment "expects a valid slot". Callers (engine.rs:102 load_project, document.rs:469) only see validated data today.
- **Test:** unit test: stereo SlotData with 5 i16 → `to_sample()` gives left.len()==right.len()==2.
- **Size:** S
- **Risk/notes:** off the audio thread (allocates by design).

### P6-T3a — sampler voice filter path is untested
- **Status:** STILL-APPLIES (no voice/machine test sets Cutoff/Resonance)
- **Location:** crates/acid-engine/src/sampler/voice.rs:209-214, 262-265
- **Category:** TEST
- **Area:** sampler
- **Proposed fix:** test: render a bright sine/noise with `s.set(SlotParam::Cutoff, 0.1)` vs default; assert lower RMS/high-band energy and finite output.
- **Test:** `cargo test -p acid-engine sampler::voice`
- **Size:** S
- **Risk/notes:** Group with: P6-T3b.

### P6-T3b — filter keeps stale state when toggled off and back on mid-note
- **Status:** STILL-APPLIES (when `filter_on` is false the Svf isn't run; turning it on later resumes from old integrator state → possible click)
- **Location:** crates/acid-engine/src/sampler/voice.rs:209-214
- **Category:** ROBUSTNESS
- **Area:** sampler
- **Proposed fix:** add `filter_was_on: bool` to Voice (false in `start`); in render, if `filter_on && !self.filter_was_on` reset `self.filter = [Svf::new(sr); 2]` (no alloc), then store `filter_was_on = filter_on`.
- **Test:** render with filter on, off for a block, on again; first post-toggle sample stays near the dry level (no jump).
- **Size:** S
- **Risk/notes:** real-time path (render must not allocate; Svf::new is stack-only — keep it so, no_alloc.rs). Group with: P6-T3a.

### P6-T3c — stereo samples and pan are not tested through render
- **Status:** STILL-APPLIES (voice test helper `run` asserts l==r; pan is only checked in `settings_default_and_decode` as numbers)
- **Location:** crates/acid-engine/src/sampler/voice.rs:301-306, 590-603
- **Category:** TEST
- **Area:** sampler
- **Proposed fix:** test rendering a stereo SampleData (left +1, right -1) and a mono sample with Pan=0 → right silent, left ~1.41×.
- **Test:** `cargo test -p acid-engine sampler::voice`
- **Size:** S
- **Risk/notes:** Group with: P6-Fa.

### P6-T3d — loop crossfade reads before loop_start (can reach before the play start / frame 0)
- **Status:** STILL-APPLIES (crossfade blends `read(pos - loop_len)`, i.e. frames in [loop_start - xfade, loop_start); xfade = min(10 ms, loop_len/2). When loop_start is within xfade of `start` (or of 0) it reads material outside the play range / clamped frame 0)
- **Location:** crates/acid-engine/src/sampler/voice.rs:205-208, 250-255
- **Category:** ROBUSTNESS
- **Area:** sampler
- **Proposed fix:** store `seg.start` in Voice (`start_frame`) and use `xfade = (LOOP_XFADE_S*sr).min(loop_len*0.5).min(loop_start - start_frame).max(1.0)` so the pre-loop window never precedes the play start.
- **Test:** chromatic loop with loop_start == start: output stays continuous and only uses frames ≥ start (e.g. sample zero before start, assert no energy from it).
- **Size:** S
- **Risk/notes:** real-time path; plan text mandates "10 ms crossfade" (plan6 md:1073) — this only shortens it when there is no room.

### P6-T4a — no test that one note_off releases two same-key voices from one source
- **Status:** STILL-APPLIES (note_off releases every matching voice, but untested)
- **Location:** crates/acid-engine/src/sampler/machine.rs:156-163
- **Category:** TEST
- **Area:** sampler
- **Proposed fix:** test: note_on(60,…,false) twice (no choke) → 2 voices; one note_off(60,false) + render → 0 voices.
- **Test:** `cargo test -p acid-engine sampler::machine`
- **Size:** S
- **Risk/notes:** behaviour matches release-all-voices-of-pitch used by other machines (Plan 4 note). Group with: P6-T4b.

### P6-T4b — voice steal order under-tested
- **Status:** STILL-APPLIES (`sixteen_voices_and_the_oldest_is_stolen` only checks the count stays 16)
- **Location:** crates/acid-engine/src/sampler/machine.rs:144-153, 335-344
- **Category:** TEST
- **Area:** sampler
- **Proposed fix:** test: fill 16 voices, release one middle key → next note takes the released voice; with all held, the oldest key (60) is the one gone (check via a `voices` key list in a cfg(test) accessor).
- **Test:** `cargo test -p acid-engine sampler::machine`
- **Size:** S
- **Risk/notes:** Group with: P6-T4a, P6-Pd.

### P6-T4c — stale grit_hold on Grit re-enable
- **Status:** STILL-APPLIES (set_param Grit doesn't reset hold/phase; first 1–2 samples after re-enable output the old held value)
- **Location:** crates/acid-engine/src/sampler/machine.rs:58-63, 204-211
- **Category:** ROBUSTNESS
- **Area:** sampler
- **Proposed fix:** in `set_param(Grit)`: `let g = on(v); if g && !self.grit { self.grit_phase = 1.0; } self.grit = g;` so the first sample captures fresh (phase 1.0 → captures immediately).
- **Test:** grit on, render loud, grit off, silence the voice, grit on, render → first sample is 0.
- **Size:** S
- **Risk/notes:** real-time path (no alloc).

### P6-T6c — truncated file returns partial audio silently
- **Status:** STILL-APPLIES (UnexpectedEof / ResetRequired just `break`, decode.rs:101-106)
- **Location:** crates/acid-engine/src/sampler/decode.rs:100-106, 143-150
- **Category:** BEHAVIOUR
- **Area:** sampler
- **Proposed fix:** recommended: keep loading the partial audio (most samplers do; recovers damaged recordings) but tell the user: add `truncated: bool` to `Decoded` set when `codec_params.n_frames` is known and decoded frames < n_frames; sampler_load shows "Loaded, but the file ends early." ALT: refuse with `DecodeError::Truncated`. **USER DECISION**
- **Test:** write a WAV with hound, chop the last 1000 bytes, decode → Ok with truncated=true (or Err under ALT).
- **Size:** S
- **Risk/notes:** Group with: P6-T6d.

### P6-T6d — no malformed / truncated / non-WAV decode tests
- **Status:** STILL-APPLIES (only plain-garbage bytes test exists; nothing for truncated WAV, FormatChanges, >2 channels, FLAC/MP3/OGG)
- **Location:** crates/acid-engine/src/sampler/decode.rs:153-221
- **Category:** TEST
- **Area:** sampler
- **Proposed fix:** add tests: truncated hound WAV; 4-channel WAV → channels==2 with first two kept; WAV header with corrupted fmt chunk → Unsupported, no panic. Non-WAV needs tiny fixture files (optional).
- **Test:** `cargo test -p acid-engine sampler::decode`
- **Size:** M
- **Risk/notes:** Group with: P6-T6c.

### P6-Pa — every meta edit (marker/zone drag) kills the slot's sounding voices
- **Status:** STILL-APPLIES (`Sampler::swap_play` → `kill_slot`, machine.rs:97-104; every Edit::SlotMeta sends SamplerPlay, document.rs:485)
- **Location:** crates/acid-engine/src/sampler/machine.rs:97-104
- **Category:** BEHAVIOUR
- **Area:** sampler
- **Proposed fix:** recommended: in swap_play, kill only if zone layout changed (mode, base_key, high_key/slice count, slices); for start/end/loop/root/bpm-only changes keep voices playing their already-started segment (voices hold their own copies). ALT: also retarget sounding voices' loop/end live (clamped) so loop-point edits are heard on held notes (M, needs care when pos is past the new end). **USER DECISION**
- **Test:** machine test: hold a chromatic note, swap_play with only loop_end moved → voice still active; with base_key moved → killed.
- **Size:** S (ALT: M)
- **Risk/notes:** real-time path; swap_sample must still clear play (f9127a9 invariant). Ruling said acceptable — fix later.

### P6-Pd — voice steal hard-restarts (click)
- **Status:** STILL-APPLIES (Voice::start resets env to 0 and pos immediately on a stolen active voice, voice.rs:141-170; steal at machine.rs:144-153)
- **Location:** crates/acid-engine/src/sampler/voice.rs:141-170; crates/acid-engine/src/sampler/machine.rs:144-153
- **Category:** BEHAVIOUR
- **Area:** sampler
- **Proposed fix:** recommended: per-voice "steal fade": when starting over an active voice, keep a short (≈2 ms) linear fade of the previous output — store `prev_pos/prev_step/prev_env/prev_slot` in Voice and mix them out for the first N samples (no alloc). ALT: leave as is (ruling: fine for typical use; 16 voices rarely exhaust). **USER DECISION**
- **Test:** steal test: fill 16 held voices, note 17 → max sample-to-sample step in the following block below a threshold.
- **Size:** M
- **Risk/notes:** real-time path; no_alloc.rs. Group with: P6-T4b.

### P6-Fa — slot Level/Pan applied live without smoothing (clicks on big jumps)
- **Status:** STILL-APPLIES (voice gain uses `s.level_gain`, `s.pan_l/pan_r` per sample directly, voice.rs:266-268; live since f8204ca)
- **Location:** crates/acid-engine/src/sampler/voice.rs:266-268
- **Category:** BEHAVIOUR
- **Area:** sampler
- **Proposed fix:** add `gain_l, gain_r: f32` to Voice, set to target in `start`; per sample `gain_x += (target_x - gain_x) * k` with k for ~5 ms (compute once per render: `1 - exp(-1/(0.005*sr))`), target = level_gain*pan_x.
- **Test:** voice test: render, jump Level 0→1 between blocks → max step in next block small; `level_changes_reach_a_sounding_voice` needs its 0.5× assert relaxed to after settling.
- **Size:** S
- **Risk/notes:** real-time path (no alloc). Existing test level_changes_reach_a_sounding_voice (voice.rs:387-413) asserts exact halving — will need updating. Group with: P6-T3c.

### P6-T6a — decode: length cap skipped while sample_rate==0
- **Status:** ALREADY-FIXED (0357f9e; first packet's spec is locked and rate-checked 8–192 kHz, `sample_rate = spec.rate` set before the cap, decode.rs:113-137)
- **Location:** crates/acid-engine/src/sampler/decode.rs:113-137
- **Category:** ROBUSTNESS
- **Area:** sampler
- **Proposed fix:** none.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P6-T6b — decode: mid-stream spec change mixes layouts
- **Status:** ALREADY-FIXED (0357f9e; `DecodeError::FormatChanges`, decode.rs:121)
- **Location:** crates/acid-engine/src/sampler/decode.rs:114-123
- **Category:** ROBUSTNESS
- **Area:** sampler
- **Proposed fix:** none (a test is still missing — see P6-T6d).
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

## app UI/document (61)

### P2-T4a — Edit index preconditions are not documented
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/document.rs:17-103 (Edit enum), 244 (`apply_raw` indexes and `expect`s)
- **Category:** DOC
- **Area:** app UI/document
- **Proposed fix:** Add a doc comment on `Edit`/`Document::apply`. Indices must be in range (synth<2, kit<2, slot<PATTERNS, step<STEPS, inst<DRUM_INSTRUMENTS, machine is a note machine, sampler slot<SLOTS), otherwise it panics.
- **Test:** None (doc).
- **Size:** S
- **Risk/notes:** Group with: P2-T4b.

### P2-T4b — SynthPattern/DrumPattern edits are not clamped or validated
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/document.rs:295-306, 335-342 (whole-pattern replace with no check). The engine silently rejects malformed patterns at crates/acid-engine/src/engine.rs:348,356.
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** In `apply_raw`, add `debug_assert!(pattern.is_well_formed())` for SynthPattern, DrumPattern and NotePattern. Alternatively, clamp `length` to 1..=STEPS and `transpose` to -12..=12 to match the per-field edits. Without one of these, the document and engine could disagree.
- **Test:** Unit test: apply a SynthPattern with length 0 or transpose 40, and assert the stored values are clamped.
- **Size:** S
- **Risk/notes:** All current sources (clipboard, randomiser, roll) produce well-formed patterns. Group with: P2-T4a.

### P2-T4c — No-op edits are recorded as undo steps and mark the document dirty
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/document.rs:180-195 (`apply` always pushes and sets dirty)
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** In `apply`, for the cheap variants (Param, Bpm, Swing, SynthStep, DrumHit, *Length, SynthTranspose, View), clone the edit before `apply_raw`. If `inverse == clone`, return with no undo push and no dirty flag. Still return the commands, or return none. Do not clone SamplerSlot, because it holds the sample data.
- **Test:** Double-click reset on a knob already at its default: `!can_undo() && !dirty`.
- **Size:** S
- **Risk/notes:** Clamped values can still slip through as a "change" (unclamped edit vs old value). Compare against the clamped stored value if that matters. Group with: P2-T4d.

### P2-T4d — The document stays dirty after undoing back to the saved state
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/document.rs:209-231 (undo/redo set `dirty = true`), crates/app/src/ui/files.rs:187 (save clears dirty)
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** **USER DECISION.** Recommended: track a saved marker (`saved_depth: Option<usize>` = undo.len() at save). Set it to None when the undo history is trimmed past it, or when redo is cleared below it. Make `dirty = Some(undo.len()) != saved_depth || other_dirty`, where other_dirty covers non-undoable changes such as `select_pattern`. ALT: keep it simple. Any undo or redo leaves the document dirty (current behaviour, conservative).
- **Test:** Edit, save, undo, redo: dirty goes true→false→true→false.
- **Size:** M
- **Risk/notes:** `select_pattern` sets dirty outside undo (document.rs:233-240), so the marker needs a separate "non-undoable dirty" flag. Group with: P2-T4c.

### P2-T5a — A partial trailing frame is left untouched when data.len() % channels != 0
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/audio.rs:123-153 (`fill_interleaved`)
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** After the loop, add `for s in &mut data[frames * channels..] { *s = T::EQUILIBRIUM; }`.
- **Test:** Unit test with `data` of length 2*10+1 and 2 channels: the last sample is 0.
- **Size:** S
- **Risk/notes:** Audio callback path; no allocation.

### P2-T6d — Pump failure paths are untested
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/mod.rs:189-229 (`flush_pending` keeps unsent commands; `pump` failure branch) and 232-239 (auto-retry)
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Extract the failure branch into `fn on_audio_failed(&mut self)` (status reset, audio_error, audio_lost) and unit-test it together with the auto-retry flag logic. The `Err(cmd)` ordering in `flush_pending` would need a ring-full fake link, or the push loop extracted into a pure fn over `FnMut(Command)->Result<(),Command>`.
- **Test:** Headless test that calls `on_audio_failed()` and checks `audio_error`, `status == default`, and that `auto_retried` toggles once.
- **Size:** M
- **Risk/notes:** AudioLink wraps a cpal stream, so it cannot be built in tests. Use a small refactor instead of mocking.

### P2-T7a — Space may re-activate a focused note button (restarting the step-writer)
- **Status:** STILL-APPLIES (reachable only when the button has keyboard focus, which needs Tab navigation; egui 0.31 fakes a primary click on Space/Enter for a focused widget at context.rs:1262-1268, and Space is the writer's Rest key)
- **Location:** crates/app/src/ui/step303.rs:64-67 (`r.clicked()` → `start_writer`)
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Use `r.clicked_by(egui::PointerButton::Primary)` (excludes FAKE_PRIMARY_CLICKED) for the note row's start-writer. Optionally do the same for the other rows.
- **Test:** Manual: Tab to a note button, click it to start writing, press Space. The cursor should advance, not reset.
- **Size:** S
- **Risk/notes:** Group with: P2-T7c, P2-T7d (same fn).

### P2-T7b — Step-writer stays active across pattern selection
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/pads.rs:36 (select; writers untouched), crates/app/src/ui/step_edit.rs:125-129 (cursor clamped, so no panic)
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** **USER DECISION.** Recommended: when a synth's selected slot changes (pads.rs, and song-mode record pad changes), set `self.view.writers[machine] = None` so keystrokes never land in a pattern the user did not click. ALT: keep the writer, so the user can carry on writing at the same cursor in the new pattern. If so, document this in the status label.
- **Test:** Headless: start a writer, call the pad select path, assert `writers[0].is_none()`.
- **Size:** S
- **Risk/notes:** None.

### P2-T7c — Stale pattern clone when two edits hit the same step in one frame
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/step303.rs:20 (clone), 64-90 (edits computed from the clone's `*s`), 110-122 (applied sequentially)
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** Queue row operations (`enum Op { Toggle(row), Nudge(n) }`) instead of precomputed values. At apply time, compute the value from `self.doc.project.synths[synth].patterns[slot].steps[step]`.
- **Test:** Unit test the op-apply helper: a secondary click (gate) and a scroll nudge on one step in one frame keep both changes.
- **Size:** S
- **Risk/notes:** Needs an unusual same-frame combination (right-click while scrolling). Group with: P2-T7a.

### P2-T7d — Scroll end_gesture fires on the first idle frame (each wheel notch while playing becomes a separate undo entry)
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/step303.rs:100-108; crates/app/src/ui/params_ui.rs:175-185 (knob, same pattern); likely also the poly roll `poly_scrolled`
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Debounce: store the time of the last scroll (`view.scroll_at[synth]: f64` and the knob equivalent). Close the gesture only when `now - scroll_at > 0.4 s` and the pointer is no longer scrolling. The UI must keep repainting until then (`request_repaint_after(400ms)`).
- **Test:** Headless frames with injected scroll events 100 ms apart produce one undo entry; a 1 s gap produces two.
- **Size:** M
- **Risk/notes:** The knob path also drives record-mode `Touch(false)`. Keep the touch release immediate and debounce only `end_gesture`. Group with: P2-T6b, P2-T7a.

### P2-T8a — A click in the accent/slide lane over the key column toggles step 0
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/pianoroll.rs:132-145 (`cell_at_clamped` clamps x<left to column 0)
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Guard the lane branches with `p.x >= geom.left`, for example `if p.x >= geom.left && in_lane(...)`. Or compute the step with `((p.x - geom.left)/CELL_W).floor()` and skip when it is negative.
- **Test:** Unit test on a small helper `lane_step(geom, x) -> Option<usize>`: x < left → None.
- **Size:** S
- **Risk/notes:** Check the poly roll (polyroll.rs) for the same clamped lane hit-test. Group with: P2-T8b.

### P2-T8b — Dragging a note across other notes erases them within the gesture (undo restores them)
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/pianoroll.rs:178-207; crates/app/src/ui/roll_edit.rs:139-147 (`move_note` erases, then `place` overwrites)
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** **USER DECISION.** Recommended: snapshot the pattern at drag start. Store `origin: SynthPattern` in the drag state; `RollDrag` is Copy, so keep it in `view.roll_origin[synth]`. Each frame, recompute the move or resize from the origin, so notes passed over reappear and only the final drop position overwrites. ALT: keep the destructive drag (it is a mono line, and undo restores it) and document it in the module doc.
- **Test:** Unit test on a pure `drag_from_origin(origin, drag, step, row)` helper: moving across a note and back leaves it intact.
- **Size:** M
- **Risk/notes:** Group with: P2-T8a, P2-T8c.

### P2-T9b — Len (and Trans) edits don't set machine focus
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/drum_panel.rs:73-77; crates/app/src/ui/synth_panel.rs:53-82; crates/app/src/ui/note_panel.rs:30-40
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Add `self.view.focus = machine;` (2+kit / synth / m) in each `if r.changed()` branch, to match the grid, roll and dice handlers.
- **Test:** Headless: change Len on kit 1 and assert `view.focus == 3`. Easiest after extracting the handler.
- **Size:** S
- **Risk/notes:** Focus picks the copy/paste target.

### P2-T10b — meter_fraction(NaN) returns NaN
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/mixer_panel.rs:53-58
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** Replace the guard with `if !(peak > 0.0) { return 0.0; }`, which catches NaN. Optionally map +inf to 1.0, which clamp already does.
- **Test:** Add `assert_eq!(meter_fraction(f32::NAN), 0.0);` to the existing test at mixer_panel.rs:165.
- **Size:** S
- **Risk/notes:** In practice the engine meters use `f32::max`, which drops NaN, so this is defensive.

### P2-T11a — Save only adds .sidacid when the name has no extension
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/files.rs:181-183
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** **USER DECISION.** Recommended: append `.sidacid` unless the extension already equals it (case-insensitive), so "beat.v2" becomes "beat.v2.sidacid". Use `path.set_file_name(format!("{}.sidacid", file_name))`, not `set_extension`, which would replace ".v2". ALT: keep respecting any user-typed extension (current behaviour). The Open filter then hides those files.
- **Test:** Unit test a pure `with_project_extension(PathBuf) -> PathBuf` helper for "a", "a.v2" and "a.SIDACID".
- **Size:** S
- **Risk/notes:** Group with: P2-T11e.

### P2-T11b — No guard against concurrent exports
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/files.rs:267-303 (`start_export` overwrites `export_rx`, so the first export's result and progress are dropped silently), 375-381
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** At the top of `start_export`, if `export_rx.is_some()` set the message "An export is already running." and return. Also disable the "Export to WAV…" menu item while `export_rx.is_some()` (`add_enabled`).
- **Test:** Headless: set `view.export_rx = Some(rx)`, call the guard path, and assert the message is set and rx is unchanged. The guard must come before the rfd dialog.
- **Size:** S
- **Risk/notes:** Background thread; no audio-thread impact.

### P2-T11d — The unsaved-changes dialog parses the name from the title string
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/files.rs:327-333 (trims " — SidAcid" and '*' from `window_title()`; a project name ending in '*' loses it); name logic at 256-264
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** Extract `fn document_name(&self) -> String` (path file_stem or project.name). Use it in both `window_title` and the dialog.
- **Test:** Test that `document_name()` for a project named "Beat*" stays "Beat*".
- **Size:** S
- **Risk/notes:** Group with: P2-T11e.

### P3-T7a — non-song edits (switches, BPM) during a record pass split its undo step (plan-mandated)
- **Status:** STILL-APPLIES. `Document::apply` (document.rs:180-195) continues a gesture only while the merge key is unchanged. Any non-Song edit (switch params, which are not recorded, BPM, pattern edits) sets a new key. The next recorded Song edit then starts a new undo step. Plan 3 §54 says switches still edit their saved value during a pass, and §38 says one undo step per pass.
- **Location:** crates/app/src/document.rs:180-195; crates/app/src/ui/record.rs:104-113
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** **USER DECISION.** Recommended: accept it and document that interleaved edits split the pass. ALT: add a `record_pass: Option<usize>` (the undo index of the pass's Song entry) to Document, so Song edits during a pass replace that entry's inverse even after other edits. This is riskier because undo would then reorder history.
- **Test:** In a recording app: record, apply a Param edit on a switch, record again, `stop_recording`, then undo and check the result.
- **Size:** S (doc) / M (ALT)
- **Risk/notes:** Interacts with P3-T7b. Undo ordering correctness for the ALT.

### P3-T7b — stop_recording doesn't clear touched_knob (duplicate Touch(false), no re-touch after re-arm)
- **Status:** STILL-APPLIES. record.rs:47-54 sends `Touch(id,false)` but leaves `view.touched_knob = Some(id)`, which causes two problems:
  - Releasing the knob later sends a second `Touch(false)` (record.rs:138-143).
  - If the knob is still held when playback or recording restarts, `knob_gesture` (params_ui.rs:175-179) sees `touched_knob == Some(id)` and never re-sends `Touch(true)`, so song automation fights the held knob.

  This also happens through the auto-stop at mod.rs:243-245.
- **Location:** crates/app/src/ui/record.rs:47-54, 138-147; crates/app/src/ui/params_ui.rs:174-185; crates/app/src/ui/mod.rs:243-245
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Track the engine-side touch separately with a new `view.engine_touch: Option<ParamId>`:
  - `knob_gesture` sends `Touch(id,true)` when `recording && held && engine_touch != Some(id)` and sets it.
  - `release_knob` and `stop_recording` send `Touch(false)` only if `engine_touch == Some(id)`, then clear it.
  - Clear it in `reset_transport_view` too.
- **Test:** Hold the knob (touched each frame), call `stop_recording`, then `release_knob`. Assert exactly one `Touch(false)`. Re-arm and set playing, and assert `Touch(true)` is sent again on the next held frame.
- **Size:** M
- **Risk/notes:** Group with: P3-T7c, P3-T7d.

### P3-T7c — status.playing is stale on a fast stop-stop
- **Status:** STILL-APPLIES. `stop()` (record.rs:66-80) decides "already stopped" from `self.status.playing`, which only updates when a Status event arrives (mod.rs:211-216, about 60 Hz). A second Stop pressed before then does not return to the start. In the same window, `recording()` stays true, so `record_automation` can write one more step after Stop.
- **Location:** crates/app/src/ui/record.rs:13-15, 66-80; crates/app/src/ui/mod.rs:211-216
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** In `stop()`, set `self.status.playing = false` optimistically after sending `Command::Stop`. Do the same in `set_mode`, which also sends Stop.
- **Test:** In a recording app, call `stop()` twice with no pump in between and assert `song_cursor` returns to the start. After the first stop, `record_automation` writes nothing.
- **Size:** S
- **Risk/notes:** A Status already in the ring may flip it back to true for one frame. That is harmless. Group with: P3-T7b.

### P3-T7d — test gaps: touch sequence and dedupe
- **Status:** STILL-APPLIES (partly). The Touch(true)→Touch(false) sequence for a single edit is now covered by `an_edit_without_a_drag_counts_as_a_touch_while_recording` (params_ui.rs:191-228, e6adde0). Still untested:
  - the same-value dedupe early return in `record_automation` (record.rs:96-99)
  - Touch handling across `stop_recording`
- **Location:** crates/app/src/ui/record.rs:83-114
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Add a test: record value 0.5 at step 4, then call `record_automation` again at step 4 with the same value. Assert no new undo entry or apply (the dirty or undo count is unchanged). Add the Touch-across-stop test from P3-T7b.
- **Test:** As above.
- **Size:** S
- **Risk/notes:** Group with: P3-T7b.

### P3-T8c — song panel tests are thin
- **Status:** STILL-APPLIES. Tests cover Add, Loop, Duplicate and Delete (mod.rs:747-776). Nothing exercises `SongAction::Insert`, `Paste`, `NoLoop`, or Copy (song_panel.rs:129-145, 92-94).
- **Location:** crates/app/src/ui/song_panel.rs:112-147
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Add `song_insert_paste_and_no_loop_actions`. It inserts at a selection (copies bar `a`), sets `song_clip` and pastes after the selection, then NoLoop clears `loop_range`. Each step is one undo step.
- **Test:** As above.
- **Size:** S
- **Risk/notes:** Group with: P3-T8d.

### P3-T9b — the evolve undo test doesn't check that evolve changed anything
- **Status:** STILL-APPLIES. `dice_and_drum_buttons_are_single_undo_steps` (mod.rs:327-346) calls `evolve_drums(0)` and undo, but never asserts that the pattern changed. The seed is time-based (synth_panel.rs:169-176) and evolve is a no-op for about 20% of seeds, so a direct `assert_ne` would be flaky.
- **Location:** crates/app/src/ui/mod.rs:327-346
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Evolve up to 10 times, undoing each time, until the pattern differs. Assert it changed at least once and that undo restores `before`. Alternatively, make the seed injectable for tests (`view.seed_override`).
- **Test:** As above.
- **Size:** S
- **Risk/notes:** P3-T5a would make evolve change something more reliably.

### P3-T9c — groove pairing is untested
- **Status:** STILL-APPLIES. `randomise_synth` (synth_panel.rs:178-198) pairs synth N with drum kit N's *selected* pattern (`p.drums[synth].patterns[p.selected[2+synth]]`). No test checks this pairing.
- **Location:** crates/app/src/ui/synth_panel.rs:178-198
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Add an app test with deterministic seeds. Either refactor `randomise_synth` to take a seed internally (for example `randomise_synth_seeded`), or compare against `acid_engine::random::randomise_synth(&pattern, dice, &expected_groove, seed)`. Select drum kit 1's pattern 3, randomise synth 1 with rest dice, and assert it equals the engine function called with that groove.
- **Test:** As above.
- **Size:** S
- **Risk/notes:** Needs a seed hook (shared with P3-T9b).

### P4-T7a — refusal test asserts only `message.is_some()` (could be stale); should assert the project is unchanged
- **Status:** STILL-APPLIES (`note_patterns_copy_between_fm_and_sampler_but_not_onto_synths` checks only `message.is_some()` after pasting onto a 303 pattern)
- **Location:** crates/app/src/ui/mod.rs:610-630 (assert at 629)
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Before the refused paste, add `assert!(app.view.message.is_none()); let before = app.doc.project.clone();`. After it, add `assert_eq!(app.doc.project, before);`, as `pasting_drums_onto_a_synth_is_refused` (mod.rs:633-642) already does.
- **Test:** `cargo test -p app note_patterns_copy_between`
- **Size:** S
- **Risk/notes:** Other refusal tests (mod.rs:912, 943, 987) already pair the message check with a project-unchanged assert.

### P4-T8a — `note_rect` uses unchecked `n.start + 1`
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/poly_edit.rs:180-186 (line 182)
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** Use `n.start.saturating_add(1)`. It can only overflow for `start == u16::MAX`, which validated patterns never contain, but it would panic in debug.
- **Test:** Unit test: `note_rect` of `Note { start: u16::MAX, .. }` does not panic.
- **Size:** S
- **Risk/notes:** UI only.

### P4-T8b — move/resize can create overlapping same-pitch notes
- **Status:** STILL-APPLIES (`move_to` and `resize` change start/pitch/len with no overlap resolution, unlike `add`)
- **Location:** crates/app/src/ui/poly_edit.rs:102-127; engine tolerance at crates/acid-engine/src/sequencer.rs:600-628
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Recommended: leave as is. The engine plays overlaps sanely (held bit + release-all-of-pitch, see P4-Fa), and removing notes mid-drag would break the "index stays valid for the whole drag" invariant (poly_edit.rs:1-3). ALT: on drag end (polyroll.rs:274-276, `drag_stopped`), run a `resolve_overlaps(p, index)` that drops the other same-pitch notes the dragged note overlaps, inside the same undo gesture. **USER DECISION**
- **Test:** (ALT) poly_edit unit test: move a note onto a same-pitch note, call `resolve_overlaps`, and expect one note.
- **Size:** M
- **Risk/notes:** Group with: P4-Pb (the same overlap policy in `add`) and P4-Fa (the engine guarantee that makes overlaps safe).

### P4-T9a — collapsing a panel while a key or drag is held leaves the audition note sounding / the gesture open
- **Status:** STILL-APPLIES. `poly_roll` is only called inside the FM/Sampler `CollapsingHeader` body, and `self.audition(machine, key_down)` (polyroll.rs:331) is the only path that releases. When the panel is collapsed, `view.audition` and `poly_drag[n]` are never cleared.
- **Location:** crates/app/src/ui/fm_panel.rs:94-124; crates/app/src/ui/sampler_panel.rs:41-63; crates/app/src/ui/polyroll.rs:331, 351-372
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Once per frame in `SidAcidApp::ui` (mod.rs:231+), after the panels are drawn: if `view.audition` is Some and the primary pointer is not down, call `self.audition(m, None)`. If any `poly_drag[n]` is Some and the pointer is not down, set it to None and call `doc.end_gesture()`. This mirrors `release_pad_note` (sampler_panel.rs:106-116).
- **Test:** Headless: set `view.audition = Some((4,60))`, run a frame with no pointer, and assert `audition == None` and a `NoteOff` is pending.
- **Size:** S
- **Risk/notes:** Group with: P4-T9c.

### P4-T9b — Len DragValue gesture may stay open on arrow keys / typing (plan-mandated pattern also used by synth Len/Trans)
- **Status:** STILL-APPLIES (`apply(.., true)` always merges; the gesture ends only on `drag_stopped() || lost_focus()`)
- **Location:** crates/app/src/ui/note_panel.rs:25-43; crates/app/src/ui/synth_panel.rs:49-86
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Pass `merge = r.dragged()` instead of `true` (the pattern already used by the song bar editor, song_panel.rs:245-249), so keyboard/typed edits are separate undo steps. Keep the `end_gesture` on drag_stopped/lost_focus.
- **Test:** Hard to drive DragValue headless; a doc-level test of `apply(..,false)` already exists. Manual check: arrow-key twice, undo once, and the length moves back one step.
- **Size:** S
- **Risk/notes:** The plan mandated this pattern (Plan 4 Task 9). The same change applies to the synth Len and Trans fields. Group with: Plan 2/3 items on synth_panel DragValue gestures, if any.

### P4-T9c — no unit test for audition note-on/note-off pairing
- **Status:** STILL-APPLIES (no test calls `audition`; the only related test is `loading_a_document_resets_poly_roll_state`, mod.rs:547)
- **Location:** crates/app/src/ui/polyroll.rs:351-372
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Headless test: call `app.audition(4, Some(60))`, then `Some(62)`, then `None`. Assert `app.pending` holds NoteOn 60, NoteOff 60, NoteOn 62, NoteOff 62 in order. Also check that `audition(5, None)` while machine 4 is held sends nothing.
- **Test:** `cargo test -p app audition_pairs`
- **Size:** S
- **Risk/notes:** `audition` is a private fn in polyroll.rs, so put the test in that module or make it `pub(crate)`. Group with: P4-T9a, P4-T9d.

### P4-T9d — no unit test that one poly-roll drag is one undo step
- **Status:** STILL-APPLIES. The generic merge is tested at document level (document.rs:591, 685), but the poly roll's `apply(.., dragging)` + `end_gesture` flow (polyroll.rs:332-347) is not.
- **Location:** crates/app/src/ui/polyroll.rs:332-347
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Doc-level test: apply three `Edit::NotePattern` with merge=true to the same machine/slot, then `end_gesture`, then `undo` once, and assert the original pattern is back. If needed, factor the polyroll commit into a small `commit_poly_edit(machine, slot, pattern, dragging, finished)` fn and test that.
- **Test:** `cargo test -p app poly_drag_is_one_undo_step`
- **Size:** S
- **Risk/notes:** Group with: P4-T9c.

### P4-T10a — no test of FM transpose editing in the song bar editor
- **Status:** STILL-APPLIES. The engine side is tested (sequencer.rs:1405-1430 `song_mutes_and_transposes_note_tracks`, `bar.transpose[2]`), but the UI bar editor row for FM (song_panel.rs:240-250, via `MachineId::transpose_index`) is not.
- **Location:** crates/app/src/ui/song_panel.rs:240-250; crates/acid-engine/src/machine.rs:73-80
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Unit test in machine.rs: `transpose_index` is Some(2) for Fm and None for drums/Sampler. Headless app test: apply a song edit with `bars[a].transpose[2] = 5` via `apply_song` and assert it undoes, and that a `Command` reaches the engine.
- **Test:** `cargo test -p acid-engine transpose_index` / `cargo test -p app fm_bar_transpose`
- **Size:** S
- **Risk/notes:** DragValue interaction itself isn't headless-testable.

### P4-Pb — (parked) `poly_edit::add` overlap retain can delete a same-pitch note hidden past a shortened end
- **Status:** STILL-APPLIES. The `overlaps` closure tests all notes, including those starting at or after `p.ticks()` (kept but hidden after a length shrink), against the full `start..start+len` span.
- **Location:** crates/app/src/ui/poly_edit.rs:71-89 (closure at 76, retain at 81)
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Recommended: only replace visible notes, by adding `n.start < p.ticks()` to `overlaps`, so hidden notes survive (lengthening the pattern brings them back, possibly overlapping, which the engine tolerates). ALT: keep the current behaviour, since undo restores the note and the case is rare. **USER DECISION**
- **Test:** poly_edit unit test: pattern length 4 with a hidden note at tick 20 (pitch 60); `add(p, 12, 40, 60, 100)`; assert the hidden note is still there.
- **Size:** S
- **Risk/notes:** The `MAX_NOTES` count at line 77 must use the same filter. Group with: P4-T8b, P4-Fa.

### P5-T8b — compat test strips RON lines (use a fixture)
- **Status:** STILL-APPLIES (`fm_voice_name_and_bank_round_trip_and_default` filters out `voice_name:` and `bank:` lines)
- **Location:** crates/acid-engine/src/project.rs:772-792
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Add a checked-in pre-Plan-5 project fixture (e.g. `crates/acid-engine/tests/fixtures/plan4.ron`, saved from an older commit), load it with `include_str!`, and assert `voice_name == "INIT VOICE"`, `bank == None`, and that sampler slots default correctly.
- **Test:** The fixture-based load test.
- **Size:** M
- **Risk/notes:** `projects_without_sampler_slots_still_load` (line 743) probably strips lines the same way, so one fixture can cover both. Group with: P5-T8a.

### P5-T9a — the FmVoice inverse assumes unique ParamIds
- **Status:** STILL-APPLIES (if `values` held a duplicate id, the inverse would restore the intermediate value; `to_params` happens to be unique). `Edit::SamplerSlot` uses the same pattern.
- **Location:** crates/app/src/document.rs:380-393, 394-409
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** Collect the old values, then `.reverse()` them, so undo applies them in reverse order and duplicates restore the original value correctly. Add `debug_assert!` that the ids are unique as documentation.
- **Test:** Apply `FmVoice` with `[(alg,0.2),(alg,0.9)]`, undo, and check that alg equals its default.
- **Size:** S
- **Risk/notes:** Apply the same change to SamplerSlot in one go.

### P5-T9b — the one-voice-bank path is untested
- **Status:** STILL-APPLIES (partly). Importing a single-voice `.syx` is tested (crates/app/src/ui/mod.rs:816-819). Still untested: a project whose `fm.bank` holds 1 voice, or 0 (validate accepts any length from 0 to 32, project.rs:506-509). Such a project shows a 1-entry or empty Imported grid, and an empty bank leaves the Imported tab enabled with nothing in it.
- **Location:** crates/app/src/ui/fm_panel.rs:303-327; crates/acid-engine/src/project.rs:506-509
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Add a headless test with `fm.bank = Some(vec![one voice])` and Imported selected, run frames, and load index 0. Consider having `validate` refuse `Some(vec![])` (or have the loader normalise it to `None`).
- **Test:** As described.
- **Size:** S
- **Risk/notes:** Group with: P5-T10a, P5-T10c.

### P5-T10a — Imported tab shows selected while the bank is None (the grid shows the starter bank)
- **Status:** STILL-APPLIES. After undoing an import (mod.rs:813), `fm_browser` stays `Imported` while the bank is `None`. The disabled button still renders selected, and the grid falls back to the starter voices.
- **Location:** crates/app/src/ui/fm_panel.rs:275-306
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** At the top of `fm_voice_browser`, add `if !has_bank { self.view.fm_browser = FmBrowser::Starter; }`. This normalises the view, so the button and grid agree.
- **Test:** Import a bank, undo, run one frame, and assert `view.fm_browser == Starter`.
- **Size:** S
- **Risk/notes:** files.rs:149 already resets the browser when a document loads. Group with: P5-T9b, P5-T10c.

### P5-T10b — the current-voice highlight matches by name only
- **Status:** STILL-APPLIES (`current = *name == voice_name`). Duplicate names in a bank all highlight, and a voice whose knobs have since been edited stays highlighted.
- **Location:** crates/app/src/ui/fm_panel.rs:309-314
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** Recommended: highlight only when the name matches and the voice's `to_params()` equal the current project params. Precompute this once per frame for the visible bank (at most 32×151 compares). **USER DECISION** ALT: store `fm_voice_index: Option<(FmBrowser, usize)>` in the view state, set it on load and clear it on document load or undo. This is cheaper but not persisted, and it doesn't notice knob edits.
- **Test:** Load starter voice 3, edit one of its knobs, and check that no row is highlighted (recommended option).
- **Size:** M
- **Risk/notes:** UI thread only. Group with: P5-T10a.

### P5-T10c — headless test doesn't cover Imported+None or the collapsed LFO/FX section
- **Status:** STILL-APPLIES (`fm_panel_draws_headless_with_a_bank_and_each_operator` only covers Imported+Some; "LFO · Pitch EG · Effects" is `default_open(false)`, so its knobs are never drawn in tests)
- **Location:** crates/app/src/ui/mod.rs:827-838; crates/app/src/ui/fm_panel.rs:107-120; crates/app/src/ui/mod.rs:315-320 (`run_frames` creates a new `Context` each call)
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** Add a case with Imported and `bank = None`. To open the section, either add a `run_frames_with(ctx, ..)` helper that keeps one `Context` and stores `CollapsingState::load_with_default_open(ctx, id, true)` for the `fm_more` id before the frame, or move the open state into `view` (e.g. `view.fm_more_open`) and use `.open(Some(..))` only in tests.
- **Test:** Run a frame with the section open and assert that a knob in it registered, e.g. via a param edit when it is dragged, or simply that it doesn't panic.
- **Size:** M
- **Risk/notes:** Group with: P5-T10a, P5-T9b.

### P6-T7c — no tests for memory-limit refusal, decoder catch_unwind, redo of a load, undo of a clear
- **Status:** STILL-APPLIES (ui/mod.rs tests cover load/undo, bad rate, junk bytes, meta clash; none of the four listed)
- **Location:** crates/app/src/ui/sampler_load.rs:58-80, 147-166; tests crates/app/src/ui/mod.rs:868-1035
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** add headless tests: (1) fill slots with test WAVs past MEMORY_LIMIT (or a test-only smaller limit) → message, project unchanged; (2) undo after `clear_slot` restores data; (3) `doc.redo()` after undoing a load restores slot + params. catch_unwind is hard to trigger with real codecs — skip or mark as untestable.
- **Test:** `cargo test -p app sampler`
- **Size:** M
- **Risk/notes:** memory test with real 64 MB is slow; consider injecting the limit.

### P6-T8a — x_to_frame / frame_to_x asymmetric zero-width guard
- **Status:** STILL-APPLIES (frame_to_x guards frames.max(1) only; x_to_frame guards width.max(1.0) only — round trip differs for width<1)
- **Location:** crates/app/src/ui/sampler_ui.rs:35-41
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** use the same `width.max(1.0)` in frame_to_x (and `frames.max(1)` symmetry is already harmless in x_to_frame); or early-return `left`/0 when width<=0 in both.
- **Test:** extend `frames_and_pixels_round_trip` with width 0.5 and frames 0.
- **Size:** S
- **Risk/notes:** widget width is clamped ≥200 (sampler_panel.rs:247) so not reachable today.

### P6-T9a — load_document drops pad_note without a NoteOff
- **Status:** STILL-APPLIES (files.rs:148 sets `pad_note = None`; fine when the engine is replaced, but if `audio.load_project` fails (busy) the old engine keeps the held audition note)
- **Location:** crates/app/src/ui/files.rs:135-167
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** before clearing, `if let Some(k) = self.view.pad_note.take() { self.send(vec![Command::NoteOff{machine: Sampler, pitch: k}]) }` (or send an all-notes Stop), ahead of `pending.clear()` — actually send directly to audio since pending is cleared next.
- **Test:** headless: set pad_note, load_document → pad_note None and (with audio mock) a NoteOff queued; mostly manual.
- **Size:** S
- **Risk/notes:** same applies to `view.audition` (files.rs:143). Group with: P6-T9b.

### P6-T9b — end_gesture called on every waveform drag_stopped
- **Status:** STILL-APPLIES (sampler_panel.rs:300-303 ends the gesture even when no marker was grabbed)
- **Location:** crates/app/src/ui/sampler_panel.rs:300-303
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** `if resp.drag_stopped() && self.view.wave_drag.take().is_some() { self.doc.end_gesture(); }` (still clear wave_drag unconditionally).
- **Test:** n/a (harmless today; end_gesture only clears `gesture`).
- **Size:** S
- **Risk/notes:** Group with: P6-T9c.

### P6-T9c — waveform cache key (pcm ptr, len, columns) can collide
- **Status:** STILL-APPLIES (a freed SlotData's allocation — e.g. dropped from the bounded undo stack — can be reused by a new same-length sample → stale peaks)
- **Location:** crates/app/src/ui/sampler_panel.rs:251-259; crates/app/src/ui/mod.rs:108
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** invalidate in `SidAcidApp::send` (ui/mod.rs:165): for each `Command::SamplerSample { slot, .. }` do `self.view.wave_cache.remove(&(slot as usize))` — every load/clear/undo/redo of a slot goes through it. Keep ptr/len as a secondary check.
- **Test:** headless: load A, draw (cache filled), load B into same slot → cache entry for slot gone/rebuilt.
- **Size:** S
- **Risk/notes:** Group with: P6-T9b.

### P6-T9d — manual GUI check of the sampler panel not done — surface to user
- **Status:** STILL-APPLIES (partly: README screenshot 2390fb9/0ca69db proves the panel renders with a sliced sample; interactive marker drag, drop-to-load, Shift-pad audition, zone drag were not confirmed)
- **Location:** crates/app/src/ui/sampler_panel.rs
- **Category:** TEST
- **Area:** app UI/document
- **Proposed fix:** user runs the app: drop a WAV, drag slice/loop markers, Shift-click pads, switch Slice/Chromatic, undo/redo, save+reopen.
- **Test:** manual checklist above.
- **Size:** S
- **Risk/notes:** ask the user.

### P6-Pb — undo stack holds full SlotData outside the 64 MB accounting
- **Status:** STILL-APPLIES (UNDO_LIMIT = 200 entries, no byte budget; each SamplerSlot inverse holds a whole PCM Vec, document.rs:15, 182-195, 395-408)
- **Location:** crates/app/src/document.rs:15, 182-195
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** add `fn edit_bytes(&Edit) -> usize` (SlotData pcm.len()*2) and after push drop oldest undo entries while undo+redo sample bytes > e.g. 256 MB (keep ≥1).
- **Test:** document test: apply 10 × 40 MB slot loads → undo stack sample bytes ≤ budget, newest undo still works.
- **Size:** M
- **Risk/notes:** ruling: "fine for typical use, revisit later". Group with: P6-Pc.

### P6-Pc — save deflates/base64-encodes PCM on the UI thread
- **Status:** STILL-APPLIES (`Project::save` → `to_ron` synchronously from `SidAcidApp::save`, files.rs:184; project.rs:594-599)
- **Location:** crates/app/src/ui/files.rs:172-195; crates/acid-engine/src/project.rs:594-599
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** recommended: keep synchronous (a few hundred ms freeze at worst for 64 MB) and show a "Saving…" status. ALT: clone the project and save on a std::thread, reporting back via a channel (needs dirty/path handling on completion and close-while-saving guards). **USER DECISION**
- **Test:** time `to_ron` on a project with 64 MB of samples; manual.
- **Size:** S (ALT: L)
- **Risk/notes:** ruling deferred. Group with: P6-Pb.

### P6-Fb — load backstop message is generic ("its format is not supported")
- **Status:** STILL-APPLIES (sampler_load.rs:118-123)
- **Location:** crates/app/src/ui/sampler_load.rs:118-123; crates/acid-engine/src/sampler/slot.rs:241-249
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** add `SlotData::problem(&self) -> Option<&'static str>` ("unsupported channel count", "sample rate must be 8–192 kHz", "longer than 30 s", "slice/zone layout doesn't fit") and make `is_valid` = `problem().is_none()`; message uses it.
- **Test:** unit test per reason in slot.rs; app test message text.
- **Size:** S
- **Risk/notes:** decode already reports rate/length precisely, so this is only reachable for meta/zone edge cases.

### P2-T6a — A non-integer stored swing would be snapped by the slider on redraw (a spurious dirty edit)
- **Status:** ALREADY-FIXED (119e0fa: only applied when changed by more than 1e-3, crates/app/src/ui/transport.rs:84; test `plain_redraw_does_not_rewrite_swing_or_bpm`, crates/app/src/ui/mod.rs:705-715)
- **Location:** crates/app/src/ui/transport.rs:75-90
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** None.
- **Test:** Existing test.
- **Size:** S
- **Risk/notes:** None.

### P2-T6b — Scroll and double-click knob edits never close their gesture
- **Status:** ALREADY-FIXED (119e0fa: `KnobOutput::touched` in crates/app/src/ui/knob.rs:21-22,51-77; release on the next idle frame calls `end_gesture` in crates/app/src/ui/params_ui.rs:175-185 and crates/app/src/ui/record.rs:138-147)
- **Location:** crates/app/src/ui/knob.rs:40-131
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** None. The remaining "closes too early" issue is P2-T7d.
- **Test:** Existing `an_edit_without_a_drag_counts_as_a_touch_while_recording`.
- **Size:** S
- **Risk/notes:** Group with: P2-T7d.

### P2-T6c — SetParam coalescing can reorder around other commands
- **Status:** ALREADY-FIXED (119e0fa: coalescing is limited to the trailing run of SetParams, crates/app/src/ui/mod.rs:162-184; test `coalescing_stops_at_a_non_param_command`, mod.rs:690)
- **Location:** crates/app/src/ui/mod.rs:162-184
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** None.
- **Test:** Existing test.
- **Size:** S
- **Risk/notes:** None.

### P2-T6f — Swing slider has no lost_focus end_gesture
- **Status:** ALREADY-FIXED (crates/app/src/ui/transport.rs:88-90 `if r.drag_stopped() || r.lost_focus()`)
- **Location:** crates/app/src/ui/transport.rs:88
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** None.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** None.

### P2-T7e — Misindented `if r.hovered()` line in step303.rs
- **Status:** ALREADY-FIXED (119e0fa reworked step303.rs; crates/app/src/ui/step303.rs:70 is correctly indented)
- **Location:** crates/app/src/ui/step303.rs:70
- **Category:** DOC
- **Area:** app UI/document
- **Proposed fix:** None.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** None.

### P2-T10a — Fault label has no repaint to clear it when it expires
- **Status:** ALREADY-FIXED (119e0fa repaint cadence: with audio running the UI repaints every 16 or 150 ms, crates/app/src/ui/mod.rs:280-283, and `fault_until` is only set from a Status, so audio is present)
- **Location:** crates/app/src/ui/mixer_panel.rs:150-152
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** None. Edge case: if audio dies within 3 s of a fault, the label stays until the next input. Optionally add `ctx.request_repaint_after` up to `fault_until`.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** Group with: P2-T10c.

### P2-T11c — load_document doesn't reset focus, bank or scroll
- **Status:** ALREADY-FIXED (119e0fa: bank from the selected slot at crates/app/src/ui/files.rs:152-154; scroll_accum, scrolling, writers and drags reset at 136-149). `view.focus` is still kept, which is harmless because every project has the same machines.
- **Location:** crates/app/src/ui/files.rs:134-167
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** None. Optionally `self.view.focus = 0`.
- **Test:** Existing `loading_a_document_resyncs_banks_and_clears_gestures` (mod.rs:718).
- **Size:** S
- **Risk/notes:** None.

### P3-T8b — song_toolbar clones the project every frame
- **Status:** ALREADY-FIXED (e6adde0). `song_toolbar` (song_panel.rs:69-110) only collects an action. The clone happens in `song_action` on a click (song_panel.rs:112-147: "cloned only here, never per frame").
- **Location:** crates/app/src/ui/song_panel.rs:69-147
- **Category:** ROBUSTNESS
- **Area:** app UI/document
- **Proposed fix:** None needed.
- **Test:** Existing `song_toolbar_actions_edit_the_song_as_undo_steps` (mod.rs:747).
- **Size:** S
- **Risk/notes:** None.

### P3-T9a — no repaint during export without an audio device
- **Status:** ALREADY-FIXED (e6adde0). crates/app/src/ui/mod.rs:284-287 requests a repaint every 150 ms while `export_rx.is_some()` and there is no audio.
- **Location:** crates/app/src/ui/mod.rs:281-287
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** None needed.
- **Test:** None needed.
- **Size:** S
- **Risk/notes:** None.

### P6-T5c — note_panel.rs module doc still says placeholder
- **Status:** ALREADY-FIXED (b8479ea; doc now "The note-pattern row … shared by the FM and Sampler panels", app/src/ui/note_panel.rs:1)
- **Location:** crates/app/src/ui/note_panel.rs:1
- **Category:** DOC
- **Area:** app UI/document
- **Proposed fix:** none.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P6-T7a — edit_slot_meta refuses malformed metas silently (doc says "with a message")
- **Status:** ALREADY-FIXED (e209a6e; message unless `merge`, sampler_load.rs:214-219; test `malformed_meta_edits_explain_themselves_except_mid_drag`)
- **Location:** crates/app/src/ui/sampler_load.rs:196-223
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** none.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P6-T7b — dead_code attribute placement above doc comments
- **Status:** ALREADY-FIXED (b8479ea removed every app `#[allow(dead_code)]`; none left in crates/app)
- **Location:** crates/app/src/ui/sampler_ui.rs (former)
- **Category:** DOC
- **Area:** app UI/document
- **Proposed fix:** none.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P6-T8b — "·" glyph availability unchecked
- **Status:** OBSOLETE (verified visually: docs/images/sampler.png (0ca69db) shows "3 ·" pads and "Amen ·01" roll labels rendering with egui's default fonts)
- **Location:** crates/app/src/ui/sampler_panel.rs:69-71; crates/app/src/ui/sampler_ui.rs:146
- **Category:** BEHAVIOUR
- **Area:** app UI/document
- **Proposed fix:** none.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

## randomiser/render (8)

### P3-T4a — after the leap fold, the range clamp allows leaps up to 11 semitones at range edges (spec says 7)
- **Status:** STILL-APPLIES. random.rs:110-124 folds to within MAX_LEAP=7, then moves by ±12 to stay in [-12, 23]. A fold of d ∈ [-7,-1] that falls below LOWEST becomes d+12 ∈ [5,11], so leaps of 8 to 11 are possible.
- **Location:** crates/acid-engine/src/random.rs:110-126
- **Category:** BEHAVIOUR
- **Area:** randomiser/render
- **Proposed fix:** After the range clamp, `if let Some(p) = prev && (semis - p).abs() > MAX_LEAP { semis = p; }`. This repeats the previous note, which is in-scale and in-range.
- **Test:** Tighten the leap test (P3-T4b) to `<= MAX_LEAP` for notes-only dice over 500 seeds.
- **Size:** S
- **Risk/notes:** Changes the output for some seeds. No test pins exact patterns except determinism. Group with: P3-T4b.

### P3-T4b — the leap test asserts ≤12 instead of ≤7
- **Status:** STILL-APPLIES. random.rs:466 `.abs() <= 12`.
- **Location:** crates/acid-engine/src/random.rs:446-469
- **Category:** TEST
- **Area:** randomiser/render
- **Proposed fix:** Assert `<= MAX_LEAP` (needs P3-T4a first).
- **Test:** The tightened assertion.
- **Size:** S
- **Risk/notes:** Group with: P3-T4a.

### P3-T4c — test gaps: slide-only accent rule, octave die, short groove wrap
- **Status:** STILL-APPLIES. `never_an_accent_directly_after_a_slide` (random.rs:472) only uses `Dice::ALL`. Nothing tests:
  - slide-only dice clearing base accents (random.rs:155-161)
  - the octave die's ±1 clamp (random.rs:129-138)
  - `groove_hits` wrapping a groove shorter than the synth pattern (random.rs:63-71)
- **Location:** crates/acid-engine/src/random.rs:63-71, 129-161
- **Category:** TEST
- **Area:** randomiser/render
- **Proposed fix:** Add three tests:
  - (1) The base demo pattern has accents everywhere and `Dice{slide:true}`. Assert no accent after a slide, and other accents are unchanged.
  - (2) Octave-only dice over many seeds: octave stays in -1..=1, and at least one step changes.
  - (3) A groove of length 4 with BD on step 0 and a synth of length 16. Assert gate-on rates are higher on steps 0, 4, 8 and 12.
- **Test:** As above.
- **Size:** M
- **Risk/notes:** None.

### P3-T5a — evolve can waste edits on open-hat steps
- **Status:** STILL-APPLIES. In evolve_drums (random.rs:262-288), adding CH (or shifting a CH) onto a step with OH is undone by `open_mutes_closed` at random.rs:289. The edit is wasted, so the "1..=3 changes" may become 0 (the test only requires ≥40/50 seeds to change).
- **Location:** crates/acid-engine/src/random.rs:262-289
- **Category:** BEHAVIOUR
- **Area:** randomiser/render
- **Proposed fix:** In the add op, skip (`continue` and count as a retry) when `inst == CH && out.steps[step][OH] != Off`. In the shift op, likewise when `inst == CH && out.steps[to][OH] != Off`. Optionally loop until 1..=3 *effective* edits with a max of 16 attempts.
- **Test:** Over 0..200 seeds on a template with open hats, assert evolve changes something for every seed (or ≥ 195).
- **Size:** S
- **Risk/notes:** Changes seed outputs. Group with: P3-T5c, P3-T5b.

### P3-T5b — thin tests: style variety, short lengths, empty test with a single seed
- **Status:** STILL-APPLIES.
  - No test that templates vary across seeds (KICKS, HATS, backbeat).
  - No drum_template or evolve test with length < 16 (for example 1 or 3, where the `% len` shift wraps).
  - `evolving_an_empty_pattern_adds_hits` (random.rs:377-380) uses only seed 11.
- **Location:** crates/acid-engine/src/random.rs:308-380
- **Category:** TEST
- **Area:** randomiser/render
- **Proposed fix:** Add three tests:
  - Over 0..50 seeds, at least 3 distinct templates and both SD and CP backbeats appear.
  - `drum_template(s, 3)` and `evolve_drums` on length 1 stay inside the length and do not panic.
  - Loop the empty-pattern test over 0..50 seeds.
- **Test:** As above.
- **Size:** S
- **Risk/notes:** Group with: P3-T5a.

### P3-T5c — open_mutes_closed scans beyond the pattern length
- **Status:** STILL-APPLIES. random.rs:175-181 iterates all `p.steps` (64). evolve_drums clones a user pattern, so hidden OH+CH pairs beyond `length` (left over from shortening a pattern) are silently changed.
- **Location:** crates/acid-engine/src/random.rs:175-181 (callers 251, 289)
- **Category:** ROBUSTNESS
- **Area:** randomiser/render
- **Proposed fix:** Change the signature to `open_mutes_closed(p, len)` and iterate `p.steps[..len]`. Pass `len` from both callers.
- **Test:** A pattern of length 8 with OH+CH at step 20. Evolve it and assert step 20 is unchanged.
- **Size:** S
- **Risk/notes:** Group with: P3-T5a.

### P3-T6a — the empty-song render test is weak
- **Status:** STILL-APPLIES. `empty_song_renders_almost_nothing` (render.rs:229-233) only asserts ≤ 2 s. It does not check silence or progress. The real output is 2 chunks (8820 samples).
- **Location:** crates/acid-engine/src/render.rs:229-233
- **Category:** TEST
- **Area:** randomiser/render
- **Proposed fix:** Assert `l.len() <= 2 * CHUNK`, `rms(&l) < 1e-3`, and that progress ends at 1.0.
- **Test:** The strengthened test.
- **Size:** S
- **Risk/notes:** Group with: P3-T6b.

### P3-T6b — the MAX_SONG_SECONDS cap is untested
- **Status:** STILL-APPLIES. The cap is used at render.rs:84-89, and no test covers it. Testing it directly would need about 1 hour of audio.
- **Location:** crates/acid-engine/src/render.rs:69-103
- **Category:** TEST
- **Area:** randomiser/render
- **Proposed fix:** Pass the cap into `render_song_to(project, cap_samples, progress, sink)`. Public callers pass `MAX_SONG_SECONDS * EXPORT_SAMPLE_RATE`. Then test with a 999-bar song, `cap = 2*CHUNK`, and assert length ≤ cap + tail.
- **Test:** As above.
- **Size:** S
- **Risk/notes:** Group with: P3-T6a.

## tests/docs (16)

### P2-T2b — Meter test skips channels 1 and 3
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/engine.rs:1010-1024 (`meters_report_and_reset` checks m[0], m[2], master only)
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** Assert m[1] > 0.01 and m[3] > 0.01, if the demo plays synth 2 and kit 2; otherwise give those patterns notes. Channels 4 and 5 are already covered by the sampler and FM tests (engine.rs:581, 659).
- **Test:** `cargo test -p acid-engine meters_report_and_reset`
- **Size:** S
- **Risk/notes:** Group with: P2-T2a, P2-T2c.

### P2-T2c — No positive engine-level guard test
- **Status:** STILL-APPLIES (only the mixer has one: mixer.rs:342 `guard_reports_nan_only`)
- **Location:** crates/acid-engine/src/engine.rs:413-416, 518-530; test at 1023 only asserts `!take_guard_fired()`
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** Add an in-crate engine test that forces a NaN into the channel path, for example by poisoning a private synth/mixer field from the test module. Then `process`, and assert `take_guard_fired()` is true once, then false, and that the next block is finite.
- **Test:** `cargo test -p acid-engine guard_fires_and_recovers`
- **Size:** S
- **Risk/notes:** Tests only.

### P2-T3d — Thin command-path tests
- **Status:** STILL-APPLIES
- **Location:** crates/acid-engine/src/host.rs:262-443 (no tests for SetParam, SetSwing, SelectPattern, NotePattern/DrumPattern disposal, Touch, SetParamLive, NoteOn/NoteOff)
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** Add one table-style host test that pushes each command and asserts the engine side effect: `engine().param`/swing/current_patterns, plus a Dispose event for NotePattern and DrumPattern.
- **Test:** `cargo test -p acid-engine host::tests`
- **Size:** M
- **Risk/notes:** Tests only.

### P2-T8c — No undo-depth test for a roll drag
- **Status:** STILL-APPLIES (only the roll_edit unit tests exist: roll_edit.rs:217-300)
- **Location:** crates/app/src/ui/pianoroll.rs:208-230
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** Headless test that injects pointer press, move and release events over several frames (or calls an extracted apply helper with dragging=true and then false), and asserts that one undo restores the original pattern.
- **Test:** `cargo test -p app roll_drag_is_one_undo_step`
- **Size:** M
- **Risk/notes:** Group with: P2-T8b (the same helper makes both testable).

### P2-T9a — Thin drum-panel tests (only cycle_hit)
- **Status:** STILL-APPLIES (template, evolve and paste are now covered at crates/app/src/ui/mod.rs:328-348, 633; grid click and Len are not)
- **Location:** crates/app/src/ui/drum_panel.rs:102-162, tests at 165-175
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** Extract the grid click into `fn click_drum_cell(&mut self, kit, slot, step, inst)` and test the cycle and undo. Also add a headless draw of the drum panel.
- **Test:** `cargo test -p app drum_cell_click_cycles_and_undoes`
- **Size:** S
- **Risk/notes:** Group with: P2-T9b.

### P2-T10c — fault_until is untested
- **Status:** STILL-APPLIES
- **Location:** crates/app/src/ui/mod.rs:212-215
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** Extract status handling from `pump` into `fn on_status(&mut self, status, now)` and test that `guard_fired` sets `fault_until = now + 3.0` and clears `auto_retried`.
- **Test:** `cargo test -p app guard_status_shows_the_fault_for_three_seconds`
- **Size:** S
- **Risk/notes:** Group with: P2-T6d (same pump refactor).

### P2-T11e — No tests for save, load or dialog paths
- **Status:** STILL-APPLIES (the guard, Don't-save and title are tested at crates/app/src/ui/mod.rs:645-676; `save()` and the load error path are not)
- **Location:** crates/app/src/ui/files.rs:112-195
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** Add a headless test. Set `doc.path` to a temp file, call `save(false)`, and assert the file exists and `!dirty`. Then `Project::load` and `load_document` it and compare. Save to an unwritable path should set `view.message` and return false.
- **Test:** `cargo test -p app save_and_reload_round_trip`
- **Size:** S
- **Risk/notes:** `save(false)` with a path set skips rfd, so it can run headless. Pick/save dialogs (rfd) stay untested. Group with: P2-T11a, P2-T11d.

### P5-T8a — new project tests sit above `use super::*` in mod tests
- **Status:** STILL-APPLIES (the tests module opens at line 607; the sampler and FM tests at 609-803 come before `use super::*;` at 805; Plan 6 added more tests above it too)
- **Location:** crates/acid-engine/src/project.rs:605-806
- **Category:** DOC
- **Area:** tests/docs
- **Proposed fix:** Move `use super::*; use crate::params::SynthParam;` to the top of `mod tests`.
- **Test:** cargo test still compiles.
- **Size:** S
- **Risk/notes:** Pure move. Group with: P5-T8b.

### P6-T1b — sampler_map tests skip level_gain(1), decay_seconds and out-of-range clamping
- **Status:** STILL-APPLIES (test `sampler_params_and_maps` covers level_gain(0.8), attack(0), release(1), cutoff(1), tune/fine only)
- **Location:** crates/acid-engine/src/params.rs:440-461 (maps at 372-404)
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** add asserts: `level_gain(1.0)==1.25`, `level_gain(2.0)==1.25`, `level_gain(-1.0)==0.0`, `decay_seconds(0.0)≈0.005`, `decay_seconds(1.0)≈4.0`, `tune_semitones(5.0)==24`, `fine_cents(-1.0)==-50.0`, `cutoff_hz(-1.0)≈40`.
- **Test:** `cargo test -p acid-engine sampler_params_and_maps`
- **Size:** S
- **Risk/notes:** none.

### P6-T5a — engine tests duplicate tone-slot construction 3×
- **Status:** STILL-APPLIES (same sine `tone` Vec built in sampler_project, stopping_twice_silences_sampler_loops, tone_slot_project)
- **Location:** crates/acid-engine/src/engine.rs:555-574, 602-624, 626-639
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** one helper `fn tone_slot(meta: SlotMeta) -> SlotData` used by all three.
- **Test:** `cargo test -p acid-engine engine`
- **Size:** S
- **Risk/notes:** tidy only.

### P2-T6e — allow attributes placed above doc comments
- **Status:** ALREADY-FIXED (no `#[allow]` left in crates/app; the remaining ones in acid-engine sit on test modules without doc comments: lib.rs:26, song.rs:254, project.rs:607, fm/fx.rs:160, engine.rs:533)
- **Location:** n/a
- **Category:** DOC
- **Area:** tests/docs
- **Proposed fix:** None.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** None.

### P4-T3b — no host-level test of Command::NotePattern → DisposeNotePattern
- **Status:** ALREADY-FIXED (crates/acid-engine/tests/no_alloc.rs:173-220 `host_commands_and_rendering_do_not_allocate` asserts exactly one `Event::DisposeNotePattern`; commit 0dc6c71)
- **Location:** crates/acid-engine/tests/no_alloc.rs:211-219; crates/acid-engine/src/host.rs:202-209
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** none
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P4-T3c — engine.rs module doc still says "two synths two kits"
- **Status:** ALREADY-FIXED (crates/acid-engine/src/engine.rs:1-4 now names the FM and Sampler slots; commit 4dbe52e)
- **Location:** crates/acid-engine/src/engine.rs:1-4
- **Category:** DOC
- **Area:** tests/docs
- **Proposed fix:** none
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P4-T6a — no positive-control test proving the allocation counter fires
- **Status:** ALREADY-FIXED (crates/acid-engine/tests/no_alloc.rs:165-171 `the_counter_sees_allocations`; commit 0dc6c71)
- **Location:** crates/acid-engine/tests/no_alloc.rs:165-171
- **Category:** TEST
- **Area:** tests/docs
- **Proposed fix:** none
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P5-T2c — THIRD_PARTY_NOTICES msfa entry lacks the Google copyright line
- **Status:** ALREADY-FIXED (commit 64ca6fb; THIRD_PARTY_NOTICES line 18 reads "Copyright 2012 Google Inc.")
- **Location:** THIRD_PARTY_NOTICES:12-19
- **Category:** DOC
- **Area:** tests/docs
- **Proposed fix:** None.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

### P6-T1a — SAMPLER_SLOTS inserted between PARAM_COUNT's doc comment and PARAM_COUNT (misattached doc)
- **Status:** ALREADY-FIXED (commit 141167d; each const now has its own doc, acid-engine/src/params.rs:12-15)
- **Location:** crates/acid-engine/src/params.rs:12-15
- **Category:** DOC
- **Area:** tests/docs
- **Proposed fix:** none.
- **Test:** n/a
- **Size:** S
- **Risk/notes:** —

## Fix groups (fix together)

- **Engine host `host.rs` (real-time):** P2-T3a + P2-T3b. These are plain stores in `process`, with no allocation.
- **Engine stop/transport (`Engine::stop`, `play`, `end_playback`):** P5-Fa + P4-T5a + the P6 sampler reset in `stop()`. One transport policy covers all three (stop-twice, and whether auditions get released).
- **Same-pitch overlap policy:** P4-T8b + P4-Pb + P4-Fa + P6-T4a. Decide them together; the engine's release-all-of-pitch is the safety net.
- **Audition/voice separation tests:** P4-Pa (fixed) leftovers + P5-T6a + P5-T4b (FM mono shares one key stack).
- **Sequencer `sequencer.rs` (real-time):** P3-T2a + P3-T2b.
- **Randomiser `random.rs`:** P3-T4a + T4b; P3-T5a + T5b + T5c (T5a changes the seed outputs).
- **Render `render.rs`:** P3-T6a + T6b (`render_song_to` takes a cap).
- **Record/touch (`ui/record.rs`, `params_ui.rs`):** P3-T7b + T7c + T7d, together with P3-T1c (`write_automation` range).
- **Song bar ops:** P3-T8c + T8d.
- **Synth panel seed hook:** P3-T9b + T9c.
- **Document undo/dirty (`document.rs`):** P2-T4c + T4d; P2-T4a + T4b; P5-T9a goes with the P6 `Edit::SamplerSlot` undo order.
- **App pump (`ui/mod.rs`):** P2-T6d + P2-T10c. Extract `on_audio_failed` and `on_status`.
- **Step editor and knob scroll (`step303.rs`, `knob.rs`):** P2-T7a + T7c + T7d. T7d's debounce also covers the knob path.
- **Piano roll (`pianoroll.rs`/`roll_edit.rs`):** P2-T8a + T8b + T8c. Check the polyroll for the same lane hit-test bug as T8a.
- **Len/Trans gesture and focus (drum/synth/note panels):** P2-T9b + P4-T9b.
- **Note panel:** P4-T9a + T9c + T9d.
- **Files (`ui/files.rs`):** P2-T11a + T11d + T11e (P2-T11b is separate).
- **FM:** P5-T1a + T3b; P5-T7b + T7c; P5-T8a + T8b, using a RON fixture that also serves the sampler compat test at project.rs:743. The voice browser items P5-T9b + T10a + T10c go together. synth.rs module doc: P5-T4b + T4d + T5c.
- **Sampler voice (real-time):** P6-T3a + T3b (filter); P6-T3c + P6-Fa (pan/level smoothing, which needs `level_changes_reach_a_sounding_voice` updated); P6-T4a/T4b + P6-Pd + P4-T5b (steal order and steal fade).
- **Sampler decode:** P6-T6c + T6d.
- **Sampler panel (app):** P6-T9a + T9b + T9c (T9c can go in `SidAcidApp::send`).
- **Sampler memory/save:** P6-Pb + P6-Pc.
- **Manual check still owed:** P6-T9d. In the sampler panel, try dragging markers, dropping a file, Shift-clicking a pad to audition, and undo/redo.

## Decisions the user should make

1. **P2-T4d:** should undoing back to the saved state clear the dirty flag (needs a saved-depth marker)? Or keep the current "any undo leaves it dirty"?
2. **P2-T7b:** should the step-writer drop when another pattern is selected? Or keep writing into the new pattern?
3. **P2-T8b (piano-roll drag):** snapshot at drag start so notes the drag passes over reappear, or keep the destructive drag and rely on undo?
4. **P2-T11a:** should save always append `.sidacid` unless it is already there? Or respect an extension the user typed?
5. **P3-T2b:** keep the 303-like rule where a slide or tie runs into whatever plays next (and pin it with tests)? Or look ahead into the next bar's or queued pattern?
6. **P3-T7a:** accept that switch/BPM edits split a record pass's undo step (document it)? Or track the pass's undo index?
7. **P3-T8d:** inserting a bar inside the loop: should inserting at the loop edges also extend it?
8. **P4-T5a:** should Play/Stop release auditioned notes (panic) or only pattern voices (`pattern_off()`)?
9. **P4-T8b / P4-Pb:** for same-pitch overlaps after move/resize, tolerate them or resolve them on drop? Should a hidden note past a shortened end survive?
10. **P5-T4b:** in mono FM, should audition and pattern notes share one key stack (fix the doc only)? Or keep a per-source stack?
11. **P5-T10b:** should the current-voice highlight match on name plus params, or use a stored index?
12. **P5-Fa:** should the first Stop after a song ends release notes only (a `stop_armed` flag)? Or keep the hard cut and document it? This also covers the record.rs mode-switch Stops.
13. **P6-T6c:** for a truncated WAV, load it with a warning or refuse it?
14. **P6-Pa:** should a marker or zone edit kill voices only on a zone-layout change? Or also retarget the loop/end of voices that are already sounding?
15. **P6-Pc:** keep saving on the UI thread with a "Saving…" status, or move it to a background thread?
16. **P6-Pd:** should a voice steal get a ~2 ms fade, or stay as is?
