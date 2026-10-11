# SidAcid Plan 3 — Song Mode, Automation, Randomise and Song Export

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish SidAcid's feature set:
- a song mode that plays an arrangement of bars, each bar choosing a pattern per machine, with mutes, transpose and a loop range;
- recording of pattern changes and touched-knob automation into the song, one undo step per record pass;
- a song editor;
- smart, seeded randomise for synths and drums;
- export of the whole song to WAV with a progress readout.

**Architecture:**
- **Pure song helpers** (`acid-engine/src/song.rs`) are shared by the sequencer, the offline renderer and the UI. They cover bar lengths, global step positions, automation lookup and editing, and bar insert/delete/duplicate.
- **Song playback** lives in the audio-thread `Sequencer`. The `Engine` holds the `Song`, swapped in whole like patterns. It keeps a *base* value per parameter (knob or automation) that lane restores return to, and a *touched* flag that lets a held knob override recorded automation.
- **Recording** runs on the UI thread. It uses the `song_step` the audio thread already reports in `Status`, writes automation through a merging `Edit::Song`, and tells the engine `Touch` / `SetParamLive`.
- **Randomisers** are pure, seeded functions (`rand_pcg`).

**Tech Stack:** Rust 1.99 (edition 2024), eframe/egui 0.31, cpal 0.15, rtrb 0.3, hound 3.5. This plan adds rand 0.8 and rand_pcg 0.3.

**Spec:** `docs/superpowers/specs/2026-10-11-sid-acid-design.md`, §8 (song mode), §10, §11 and §12. Plans 1 and 2 are merged. Deferred review notes are in `docs/superpowers/plans/2026-10-11-sidacid-plan2-deferred.txt`.

## Global Constraints

- **Code and dependencies:**
  - Rust edition 2024, workspace resolver 3.
  - `sid-core` stays dependency-free.
  - `acid-engine` must not depend on GUI or audio-device crates. It may add `rand` and `rand_pcg`.
- **Audio-thread rules** cover `EngineHost::process` and everything it calls:
  - No allocation, freeing, locks or I/O.
  - Replaced boxes, including songs, go back to the UI thread in `Event`s.
  - The sequencer's event buffer must be reserved big enough that it never grows: 64 + 2 × `PARAM_COUNT`. A validated song has at most one automation lane per parameter.
  - Reading a `BTreeMap` (`get`, `range`) is allowed. Inserting is not.
- **Project model:**
  - The UI thread owns the `Project`. Every change goes through `Document` (undoable) and is synced to the engine through `Command`s.
  - Machine indices: 0 = Synth 1, 1 = Synth 2, 2 = 808, 3 = 909.
  - Song: at most 999 bars. A bar has an `Option<PatternRef>` per machine, a mute per machine, and a transpose per synth.
  - A bar's length is the longest assigned pattern, or 16 steps when no pattern is assigned. Automation uses global step indices counted from the song's start.
- **Undo:** 200 levels. A record pass is one undo step, and so are a knob drag and one randomise click.
- **egui:** use the egui 0.31 API. If a call signature differs slightly, make the minimal equivalent change and note it. Never change behaviour.
- **Shell safety (for implementers):**
  - Run commands in the foreground.
  - Never run a command that reads standard input.
  - Write files with editing tools, not shell heredocs.
  - Leave no background processes.
  - The GUI may only be launched as `timeout 10 ./target/release/sidacid`.
- **Checks:** before every commit, `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check` must all be clean.

### Deliberate deviations from the spec (rulings; don't flag these)

1. **Where recorded points are made** (spec §10). Points are written by the UI from `Status.song_step`, not sent audio → UI. The engine gets `Touch(id, on)`, so a held knob overrides playback of recorded automation, and `SetParamLive(id, v)`, so the sound follows the knob without changing its saved value. The effect is the same with less audio-thread machinery.
2. **No extra 5 ms smoothing for automation.** Values change at step boundaries. Synth parameters already update every 16 samples. Mixer levels step, which is acceptable for v1.
3. **Pattern/Song mode** is transport state shown in the UI, not saved in the project. Record is only available in Song mode. Recording past the song's end isn't supported: the song stops.
4. **Bar insert, delete and duplicate shift the automation with the bars.** Changing a pattern's length later does not re-time automation.
5. **Switch-type controls are not recorded as automation.** These are discrete on/off and selector parameters. During a record pass they still edit their saved value.
6. **Song export ignores the loop range** so it always ends.
7. **"Octave" dice** moves a step up an octave with 8 % probability and down with 4 %.

---

## File Structure

```
Cargo.toml                       (modify) + rand, rand_pcg workspace deps
crates/acid-engine/
  Cargo.toml                     (modify) + rand, rand_pcg
  src/lib.rs                     (modify) + pub mod random; pub mod song;
  src/song.rs                    (new)    bar lengths, positions, automation edit/lookup, bar ops
  src/project.rs                 (modify) song validation
  src/sequencer.rs               (modify) Mode, song playback, Automation events
  src/engine.rs                  (modify) song storage, base/touched params, mode, song start
  src/host.rs                    (modify) SetMode, Song, SetSongStart, Touch, SetParamLive; DisposeSong; Status song fields
  src/random.rs                  (new)    randomise_synth, drum_template, evolve_drums
  src/render.rs                  (modify) render_song(+progress), render_song_wav
crates/app/src/
  document.rs                    (modify) Edit::Song
  ui/mod.rs                      (modify) ViewState fields, record hooks, rack + song panel
  ui/record.rs                   (new)    mode switch, record arm, play/stop, automation + pattern recording
  ui/knob.rs                     (modify) automated ring
  ui/params_ui.rs                (modify) live values while recording, touch commands, param_label
  ui/transport.rs                (modify) Pattern/Song switch, Rec, bar readout
  ui/keys.rs                     (modify) Space uses play()/stop()
  ui/pads.rs                     (modify) record pattern changes
  ui/song_panel.rs               (new)    song editor
  ui/synth_panel.rs              (modify) dice buttons
  ui/drum_panel.rs               (modify) Template / Evolve
  ui/files.rs                    (modify) export Loop/Song + progress
```

---

### Task 1: Song helpers and song validation

**Files:**
- Create: `crates/acid-engine/src/song.rs`
- Modify: `crates/acid-engine/src/lib.rs` (add `pub mod song;`, keeping the list alphabetical), `crates/acid-engine/src/project.rs` (`validate`)

**Interfaces:**
- Produces, in `acid_engine::song`:
  - Constants `MAX_BARS: usize = 999` and `DEFAULT_BAR_STEPS: usize = 16`.
  - Type aliases `SynthBanks<'a> = [&'a [SynthPattern]; 2]` and `DrumBanks<'a> = [&'a [DrumPattern]; 2]`.
  - `banks(p: &Project) -> (SynthBanks<'_>, DrumBanks<'_>)`
  - `bar_steps(bar: &Bar, synths: SynthBanks<'_>, drums: DrumBanks<'_>) -> usize`
  - `bar_start_step(song: &Song, bar: usize, synths, drums) -> u32`
  - `total_steps(song: &Song, synths, drums) -> u32`
  - `value_at(lane: &AutoLane, step: u32) -> Option<f32>`
  - `lane(song: &Song, param: ParamId) -> Option<&AutoLane>`
  - `is_automated(song: &Song, param: ParamId) -> bool`
  - `write_automation(song: &mut Song, param: ParamId, from: u32, to: u32, value: f32)`
  - `clear_automation(song: &mut Song, param: ParamId)`
  - `write_pattern_ref(song: &mut Song, bar: usize, machine: usize, slot: u8)`
- `Project::validate` additionally rejects:
  - more than `MAX_BARS` bars;
  - a loop range with `a > b` or `b >= bars.len()`;
  - an automation lane with an invalid or duplicate `param`;
  - a point that is non-finite or outside 0..=1.

- [ ] **Step 1: Write the failing tests**

`crates/acid-engine/src/song.rs`, tests first:
```rust
//! Pure song helpers shared by the sequencer, offline render and the UI:
//! bar lengths, global step positions, automation lookup and editing.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{MasterParam, SynthParam};
    use crate::project::{DrumPattern, Project, SynthPattern};

    fn project_with_lengths(synth0: u8, drum0: u8) -> Project {
        let mut p = Project::default();
        p.synths[0].patterns[1] = SynthPattern { length: synth0, ..SynthPattern::default() };
        p.drums[0].patterns[2] = DrumPattern { length: drum0, ..DrumPattern::default() };
        p
    }

    fn bar(refs: [Option<u8>; 4]) -> Bar {
        Bar { patterns: refs.map(|r| r.map(PatternRef)), ..Bar::default() }
    }

    #[test]
    fn bar_length_is_the_longest_assigned_pattern() {
        let p = project_with_lengths(32, 12);
        let (s, d) = banks(&p);
        assert_eq!(bar_steps(&bar([None; 4]), s, d), DEFAULT_BAR_STEPS);
        assert_eq!(bar_steps(&bar([Some(1), None, Some(2), None]), s, d), 32);
        assert_eq!(bar_steps(&bar([None, None, Some(2), None]), s, d), 12);
    }

    #[test]
    fn start_steps_accumulate_bar_lengths() {
        let mut p = project_with_lengths(32, 16);
        p.song.bars = vec![bar([None; 4]), bar([Some(1), None, None, None]), bar([None; 4])];
        let (s, d) = banks(&p);
        assert_eq!(bar_start_step(&p.song, 0, s, d), 0);
        assert_eq!(bar_start_step(&p.song, 1, s, d), 16);
        assert_eq!(bar_start_step(&p.song, 2, s, d), 48);
        assert_eq!(total_steps(&p.song, s, d), 64);
    }

    #[test]
    fn value_at_holds_the_last_point() {
        let mut song = Song::default();
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        write_automation(&mut song, id, 4, 4, 0.2);
        write_automation(&mut song, id, 10, 10, 0.8);
        let l = lane(&song, id).unwrap();
        assert_eq!(value_at(l, 3), None);
        assert_eq!(value_at(l, 4), Some(0.2));
        assert_eq!(value_at(l, 9), Some(0.2));
        assert_eq!(value_at(l, 10), Some(0.8));
        assert_eq!(value_at(l, 99), Some(0.8));
    }

    #[test]
    fn write_automation_fills_an_inclusive_range_and_clamps() {
        let mut song = Song::default();
        let id = ParamId::Master(MasterParam::Level);
        assert!(!is_automated(&song, id));
        write_automation(&mut song, id, 7, 5, 1.5);
        let l = lane(&song, id).unwrap();
        assert_eq!(l.points.len(), 3);
        assert!(l.points.values().all(|v| *v == 1.0));
        write_automation(&mut song, id, 6, 6, 0.25);
        assert_eq!(lane(&song, id).unwrap().points[&6], 0.25);
        assert_eq!(song.automation.len(), 1, "one lane per param");
        assert!(is_automated(&song, id));
        clear_automation(&mut song, id);
        assert!(!is_automated(&song, id));
        assert!(song.automation.is_empty());
    }

    #[test]
    fn recorded_pattern_change_runs_until_the_next_change() {
        let mut song = Song::default();
        song.bars = [0, 0, 0, 5, 0].iter().map(|&r| bar([Some(r), None, None, None])).collect();
        write_pattern_ref(&mut song, 1, 0, 9);
        let refs: Vec<_> = song.bars.iter().map(|b| b.patterns[0].unwrap().0).collect();
        assert_eq!(refs, vec![0, 9, 9, 5, 0]);
        write_pattern_ref(&mut song, 99, 0, 1);
        assert_eq!(song.bars.len(), 5, "out-of-range bar is ignored");
    }
}
```

Add to the `tests` module in `project.rs`:
```rust
    #[test]
    fn song_shapes_are_validated() {
        use crate::params::SynthParam;
        let mut p = Project::default();
        p.song.bars = vec![Bar::default(); crate::song::MAX_BARS + 1];
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));

        let mut p = Project::default();
        p.song.bars = vec![Bar::default(); 3];
        p.song.loop_range = Some((2, 1));
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        p.song.loop_range = Some((1, 3));
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        p.song.loop_range = Some((1, 2));
        p.validate().unwrap();

        let id = ParamId::Synth(0, SynthParam::Cutoff);
        let mut p = Project::default();
        let lane = AutoLane { param: id, points: [(0, 0.5)].into_iter().collect() };
        p.song.automation = vec![lane.clone(), lane.clone()];
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))), "duplicate lane");
        p.song.automation = vec![AutoLane { param: ParamId::Synth(7, SynthParam::Cutoff), ..lane.clone() }];
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))), "invalid param");
        p.song.automation = vec![AutoLane { param: id, points: [(0, f32::NAN)].into_iter().collect() }];
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))), "NaN point");
        p.song.automation = vec![AutoLane { param: id, points: [(0, 1.5)].into_iter().collect() }];
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))), "out-of-range point");
    }
```

Add `pub mod song;` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine song`
Expected: compile errors.

- [ ] **Step 3: Implement song.rs**

Insert above the tests:
```rust
use crate::params::ParamId;
use crate::project::{AutoLane, Bar, DrumPattern, PatternRef, Project, STEPS, Song, SynthPattern};
use std::collections::BTreeMap;

pub const MAX_BARS: usize = 999;
pub const DEFAULT_BAR_STEPS: usize = 16;

pub type SynthBanks<'a> = [&'a [SynthPattern]; 2];
pub type DrumBanks<'a> = [&'a [DrumPattern]; 2];

pub fn banks(p: &Project) -> (SynthBanks<'_>, DrumBanks<'_>) {
    (
        [&p.synths[0].patterns[..], &p.synths[1].patterns[..]],
        [&p.drums[0].patterns[..], &p.drums[1].patterns[..]],
    )
}

/// Steps in a bar: its longest assigned pattern, or 16 when none is assigned.
pub fn bar_steps(bar: &Bar, synths: SynthBanks<'_>, drums: DrumBanks<'_>) -> usize {
    let mut longest = 0;
    for (m, r) in bar.patterns.iter().enumerate() {
        let Some(PatternRef(slot)) = r else { continue };
        let length = if m < 2 {
            synths[m].get(*slot as usize).map(|p| p.length)
        } else {
            drums[m - 2].get(*slot as usize).map(|p| p.length)
        };
        if let Some(l) = length {
            longest = longest.max((l as usize).clamp(1, STEPS));
        }
    }
    if longest == 0 { DEFAULT_BAR_STEPS } else { longest }
}

/// Global step at which `bar` starts (`bar == bars.len()` gives the song length).
pub fn bar_start_step(song: &Song, bar: usize, synths: SynthBanks<'_>, drums: DrumBanks<'_>) -> u32 {
    song.bars.iter().take(bar).map(|b| bar_steps(b, synths, drums) as u32).sum()
}

pub fn total_steps(song: &Song, synths: SynthBanks<'_>, drums: DrumBanks<'_>) -> u32 {
    bar_start_step(song, song.bars.len(), synths, drums)
}

/// The lane's value at `step`: its last point at or before it.
pub fn value_at(lane: &AutoLane, step: u32) -> Option<f32> {
    lane.points.range(..=step).next_back().map(|(_, v)| *v)
}

pub fn lane(song: &Song, param: ParamId) -> Option<&AutoLane> {
    song.automation.iter().find(|l| l.param == param)
}

pub fn is_automated(song: &Song, param: ParamId) -> bool {
    lane(song, param).is_some_and(|l| !l.points.is_empty())
}

/// Writes `value` at every step from `from` to `to` (inclusive, either order)
/// in `param`'s lane, creating the lane if needed.
pub fn write_automation(song: &mut Song, param: ParamId, from: u32, to: u32, value: f32) {
    let value = value.clamp(0.0, 1.0);
    let (a, b) = (from.min(to), from.max(to));
    let index = match song.automation.iter().position(|l| l.param == param) {
        Some(i) => i,
        None => {
            song.automation.push(AutoLane { param, points: BTreeMap::new() });
            song.automation.len() - 1
        }
    };
    for step in a..=b {
        song.automation[index].points.insert(step, value);
    }
}

pub fn clear_automation(song: &mut Song, param: ParamId) {
    song.automation.retain(|l| l.param != param);
}

/// A pattern change made while recording: sets `machine` to `slot` in `bar`
/// and in the following bars that still held the bar's old reference.
pub fn write_pattern_ref(song: &mut Song, bar: usize, machine: usize, slot: u8) {
    let Some(old) = song.bars.get(bar).map(|b| b.patterns[machine]) else { return };
    for b in bar..song.bars.len() {
        if b > bar && song.bars[b].patterns[machine] != old {
            break;
        }
        song.bars[b].patterns[machine] = Some(PatternRef(slot));
    }
}
```

- [ ] **Step 4: Extend validation**

In `project.rs` `validate`, before the final `Ok(())`, add:
```rust
        if self.song.bars.len() > crate::song::MAX_BARS {
            return bad(format!("song has {} bars", self.song.bars.len()));
        }
        if let Some((a, b)) = self.song.loop_range
            && (a > b || b as usize >= self.song.bars.len())
        {
            return bad("song loop range is out of bounds".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for lane in &self.song.automation {
            if !lane.param.is_valid() || !seen.insert(lane.param) {
                return bad("song automation lane is invalid or repeated".into());
            }
            if lane.points.values().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
                return bad("song automation value is out of range".into());
            }
        }
```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): song helpers and song validation"
```

---

### Task 2: Song playback in the sequencer

**Files:**
- Modify: `crates/acid-engine/src/sequencer.rs`, `crates/acid-engine/src/engine.rs` (the one `tick` call site)

**Interfaces:**
- Consumes: the `song` helpers from Task 1.
- Produces, in `acid_engine::sequencer`:
  - `pub enum Mode { #[default] Pattern, Song }`, which derives `Clone, Copy, Debug, Default, PartialEq, Eq`.
  - A new variant `SeqEvent::Automation { id: ParamId, value: f32 }`.
  - `Sequencer` gains:
    - `set_mode(Mode)` (ignored while playing) and `mode() -> Mode`
    - `set_song_start(bar: usize)`
    - `song_bar() -> usize` and `song_step() -> u32`
    - `take_ended() -> bool`, true once after the song played past its last bar
  - `tick(&mut self, synths, drums, song: &Song, out: &mut Vec<SeqEvent>)`. The new `song` parameter is the only signature change.
- Song-mode behaviour:
  - At each bar start, every machine restarts at step 0 on the bar's pattern. A machine is silent if its reference is `None`, its mute is set, or its slot is out of range. Synth transpose is the pattern's plus the bar's.
  - Bar length comes from `song::bar_steps`, and shorter patterns loop inside the bar.
  - After the last bar the sequencer jumps to the loop start, if the loop end was passed. Otherwise it stops: it emits `NoteOff` for open gates and sets `ended`.
  - Automation: on each step start, a lane with a point at `song_step` emits `Automation`. On the first step after `play()` or a loop jump, every lane emits its `value_at`.
  - Pattern-mode behaviour is unchanged, and `select()` is ignored while a song plays.

- [ ] **Step 1: Write the failing tests**

In `sequencer.rs` tests:
1. Add `song: crate::project::Song` to `Fixture`, initialised with `Song::default()` in `Fixture::new`.
2. Change the `seq.tick(...)` call in `Fixture::run` to pass `&self.song` before `&mut buf`.

Then add these tests:
```rust
    fn song_bar(refs: [Option<u8>; 4]) -> crate::project::Bar {
        crate::project::Bar { patterns: refs.map(|r| r.map(crate::project::PatternRef)), ..Default::default() }
    }

    fn notes(ev: &[(usize, SeqEvent)]) -> Vec<(usize, u8)> {
        ev.iter()
            .filter_map(|(p, e)| match e {
                SeqEvent::Note { midi, .. } => Some((*p, *midi)),
                _ => None,
            })
            .collect()
    }

    fn song_seq() -> Sequencer {
        let mut s = new_seq();
        s.set_mode(Mode::Song);
        s
    }

    #[test]
    fn song_plays_bars_in_order() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(0);
        f.synths[0][1].steps[0] = on(7);
        f.song.bars = vec![song_bar([Some(0), None, None, None]), song_bar([Some(1), None, None, None])];
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 33 * 5513);
        assert_eq!(notes(&ev), vec![(0, 36), (88_200, 43)]);
        assert!(!s.is_playing(), "stops after the last bar");
        assert!(s.take_ended());
        assert!(!s.take_ended(), "reported once");
    }

    #[test]
    fn bar_length_follows_the_longest_pattern_and_shorter_ones_loop() {
        let mut f = Fixture::new();
        f.synths[0][0].length = 8;
        f.synths[0][0].steps[0] = on(0);
        f.drums[0][0].length = 16;
        f.song.bars = vec![song_bar([Some(0), None, Some(0), None])];
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 17 * 5513);
        assert_eq!(notes(&ev).len(), 2, "8-step synth pattern plays twice in the 16-step bar");
    }

    #[test]
    fn none_and_mute_silence_a_machine_and_transpose_adds() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(0);
        f.drums[0][0].steps[0][0] = DrumHit::On;
        let mut muted = song_bar([None, None, Some(0), None]);
        muted.mutes[2] = true;
        let mut lifted = song_bar([Some(0), None, Some(0), None]);
        lifted.transpose[0] = 5;
        f.song.bars = vec![muted, lifted];
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 32 * 5513);
        let drums = ev.iter().filter(|(_, e)| matches!(e, SeqEvent::Drum { .. })).count();
        assert_eq!(drums, 1, "muted in bar 1, playing in bar 2");
        assert_eq!(notes(&ev), vec![(88_200, 41)], "no synth in bar 1; bar 2 transposed +5");
    }

    #[test]
    fn loop_range_repeats_and_resets_the_song_step() {
        let mut f = Fixture::new();
        f.song.bars = vec![song_bar([None; 4]), song_bar([None; 4]), song_bar([None; 4])];
        f.song.loop_range = Some((1, 2));
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 48 * 5513 + 10);
        assert!(s.is_playing());
        assert_eq!(s.song_bar(), 1, "jumped back to the loop start");
        assert_eq!(s.song_step(), 16);
    }

    #[test]
    fn automation_emits_points_and_syncs_on_start() {
        use crate::params::{MasterParam, ParamId};
        let id = ParamId::Master(MasterParam::Level);
        let mut f = Fixture::new();
        f.song.bars = vec![song_bar([None; 4]), song_bar([None; 4])];
        crate::song::write_automation(&mut f.song, id, 3, 3, 0.25);
        crate::song::write_automation(&mut f.song, id, 16, 16, 0.75);
        let autos = |ev: &[(usize, SeqEvent)]| -> Vec<(usize, f32)> {
            ev.iter()
                .filter_map(|(p, e)| match e {
                    SeqEvent::Automation { value, .. } => Some((*p, *value)),
                    _ => None,
                })
                .collect()
        };
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 17 * 5513);
        assert_eq!(autos(&ev), vec![(16_538, 0.25), (88_200, 0.75)]);

        let mut s = song_seq();
        s.set_song_start(1);
        s.play();
        assert_eq!(s.song_bar(), 1);
        let ev = f.run(&mut s, 10);
        assert_eq!(autos(&ev), vec![(0, 0.75)], "starting mid-song applies the held value");
        assert_eq!(s.song_step(), 16);
    }

    #[test]
    fn pattern_mode_ignores_the_song_and_mode_is_fixed_while_playing() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(0);
        f.song.bars = vec![song_bar([None; 4])];
        let mut s = new_seq();
        s.play();
        s.set_mode(Mode::Song);
        assert_eq!(s.mode(), Mode::Pattern);
        let ev = f.run(&mut s, 100);
        assert_eq!(notes(&ev), vec![(0, 36)]);
    }

    #[test]
    fn an_empty_song_ends_immediately() {
        let f = Fixture::new();
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 10);
        assert!(!s.is_playing());
        assert!(s.take_ended());
    }
```

In `engine.rs` `process_block`, change the `self.seq.tick(...)` call to pass `&self.song` before `&mut self.events`. Temporarily add a field `song: crate::project::Song` to `Engine`, initialised with `Song::default()` in `new`. Task 3 builds on this field.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine sequencer`
Expected: compile errors (`Mode` and `song_bar` don't exist yet).

- [ ] **Step 3: Implement song mode**

In `sequencer.rs`:

1. Imports and the new types:
   ```rust
   use crate::params::ParamId;
   use crate::project::{DrumHit, DrumPattern, Gate, PatternRef, Song, SynthPattern};
   use crate::song::{DEFAULT_BAR_STEPS, bar_start_step, bar_steps, value_at};

   #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
   pub enum Mode {
       #[default]
       Pattern,
       Song,
   }
   ```
2. Add this `SeqEvent` variant:
   ```rust
       /// A recorded song-automation value for this step.
       Automation {
           id: ParamId,
           value: f32,
       },
   ```
3. Add these `Track` fields: `silent: bool` (the machine plays nothing this bar) and `transpose_add: i8` (the bar's synth transpose).
4. Add these `Sequencer` fields, initialised in `new` as `mode: Mode::Pattern, song_start: 0, song_bar: 0, bar_step: 0, bar_len: DEFAULT_BAR_STEPS, song_step: 0, seek: false, auto_sync: false, ended: false`:
   ```rust
       mode: Mode,
       song_start: usize,
       song_bar: usize,
       bar_step: usize,
       bar_len: usize,
       song_step: u32,
       /// The song position must be recomputed at the next bar start.
       seek: bool,
       /// The next step emits every lane's held value, not just its points.
       auto_sync: bool,
       ended: bool,
   ```
5. Add these methods:
   ```rust
       /// Ignored while playing (the UI stops before switching).
       pub fn set_mode(&mut self, mode: Mode) {
           if !self.playing {
               self.mode = mode;
           }
       }

       pub fn mode(&self) -> Mode {
           self.mode
       }

       pub fn set_song_start(&mut self, bar: usize) {
           self.song_start = bar;
       }

       pub fn song_bar(&self) -> usize {
           self.song_bar
       }

       pub fn song_step(&self) -> u32 {
           self.song_step
       }

       /// True once after a song played past its last bar.
       pub fn take_ended(&mut self) -> bool {
           std::mem::take(&mut self.ended)
       }

       /// Loads the current bar's patterns, mutes and transposes. Returns
       /// false (and stops) when the song has no bar here.
       fn load_bar(&mut self, song: &Song, synths: [&[SynthPattern]; 2], drums: [&[DrumPattern]; 2], out: &mut Vec<SeqEvent>) -> bool {
           let Some(bar) = song.bars.get(self.song_bar) else {
               for s in 0..2 {
                   if self.tracks[s].gate_open {
                       out.push(SeqEvent::NoteOff { synth: s });
                   }
               }
               self.stop();
               self.ended = true;
               return false;
           };
           if self.seek {
               self.song_step = bar_start_step(song, self.song_bar, synths, drums);
               self.seek = false;
               self.auto_sync = true;
           }
           self.bar_len = bar_steps(bar, synths, drums);
           for m in 0..4 {
               let bank_len = if m < 2 { synths[m].len() } else { drums[m - 2].len() };
               let t = &mut self.tracks[m];
               t.step = 0;
               match bar.patterns[m] {
                   Some(PatternRef(slot)) if (slot as usize) < bank_len => {
                       t.pattern = slot;
                       t.silent = bar.mutes[m];
                   }
                   _ => t.silent = true,
               }
               if m < 2 {
                   t.transpose_add = bar.transpose[m];
               }
           }
           true
       }
   ```
6. In `play()`, add this before the tracks loop, and inside the tracks loop add `t.silent = false; t.transpose_add = 0;`:
   ```rust
           self.ended = false;
           if self.mode == Mode::Song {
               self.song_bar = self.song_start;
               self.bar_step = 0;
               self.bar_len = DEFAULT_BAR_STEPS;
               self.seek = true;
           }
   ```
   In `stop()`'s tracks loop add `t.silent = false; t.transpose_add = 0;`.
7. In `select()`, return early when `self.mode == Mode::Song && self.playing`.
8. Change `tick`'s signature to take `song: &Song` before `out`.
9. At the top of `Phase::StepStart`, add:
   ```rust
                   if self.mode == Mode::Song && self.bar_step == 0 && !self.load_bar(song, synths, drums, out) {
                       return;
                   }
   ```
10. In the StepStart synth loop, immediately after `let t = &mut self.tracks[s];`, add:
    ```rust
                       if t.silent {
                           if t.gate_open {
                               out.push(SeqEvent::NoteOff { synth: s });
                               t.gate_open = false;
                           }
                           t.prev_slide = false;
                           if let Some(id) = t.lane_override.take() {
                               out.push(SeqEvent::ParamRestore { id });
                           }
                           continue;
                       }
    ```
    In the same loop, replace both `step.midi(pat.transpose)` calls with `step.midi(pat.transpose.saturating_add(t.transpose_add))`.
11. In the StepStart drum loop, after `let t = &mut self.tracks[2 + k];`, add `if t.silent { continue; }`.
12. At the end of `Phase::StepStart`, just before `self.phase = Phase::Mid;`, add:
    ```rust
                   if self.mode == Mode::Song {
                       for lane in &song.automation {
                           let value = if self.auto_sync {
                               value_at(lane, self.song_step)
                           } else {
                               lane.points.get(&self.song_step).copied()
                           };
                           if let Some(value) = value {
                               out.push(SeqEvent::Automation { id: lane.param, value });
                           }
                       }
                       self.auto_sync = false;
                   }
    ```
13. In `Phase::Mid`'s synth gate loop, skip silent tracks: after `let t = &mut self.tracks[s];` add `if t.silent { continue; }`.
14. In `Phase::Mid`, after `self.global_step += 1;`, add:
    ```rust
                   if self.mode == Mode::Song {
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
    ```

In `engine.rs`'s event match, add the arm `SeqEvent::Automation { id, value } => self.apply_param(id, value),`. Task 3 refines it.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

Hand-checked positions at 120 BPM (5512.5 samples per step):
- Bar 2 starts at step 16, sample 88 200.
- Step 3 starts at ceil(16 537.5) = 16 538.
- After 48 steps plus a few samples, a 3-bar song with loop (1, 2) has wrapped to bar 1, so `song_step` = 16.

- [ ] **Step 5: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): song mode in the sequencer — bars, mutes, transpose, loop, automation"
```

---

### Task 3: Engine and host support for songs, touch and live values

**Files:**
- Modify: `crates/acid-engine/src/engine.rs`, `crates/acid-engine/src/host.rs`

**Interfaces:**
- Consumes: `Mode` and `SeqEvent::Automation` from Task 2, and the `song` helpers.
- Produces:
  - **`Engine` methods:**
    - `set_mode(Mode)` and `mode() -> Mode`
    - `set_song_start(bar: usize)`
    - `song_bar() -> usize` and `song_step() -> u32`
    - `swap_song(&mut self, song: &mut Song) -> bool`
    - `touch(id: ParamId, on: bool)`
    - `set_param_live(id: ParamId, v: f32)`
  - **Value rules:**
    - The engine keeps `base` values. `set_param` sets both the knob and the base.
    - `set_param_live` and untouched `Automation` events set only the base.
    - Lane restores apply the base.
    - `stop()` resets the base to the knobs, clears touches and re-applies everything.
    - A song that ends by itself stops the engine, as `stop()` does.
  - **`load_project`** also loads `p.song`.
  - **`host::Command`** gains `SetMode(Mode)`, `Song(Box<Song>)`, `SetSongStart(u16)`, `Touch(ParamId, bool)` and `SetParamLive(ParamId, f32)`.
  - **`host::Event`** gains `DisposeSong(Box<Song>)`.
  - **`host::Status`** gains `mode: Mode`, `song_bar: u16` and `song_step: u32`.

- [ ] **Step 1: Write the failing tests**

Add to `engine.rs` tests:
```rust
    fn two_bar_demo() -> Project {
        let mut p = crate::demo::demo_project();
        let bar = crate::project::Bar {
            patterns: [Some(crate::project::PatternRef(0)); 4],
            ..Default::default()
        };
        p.song.bars = vec![bar, bar];
        p
    }

    #[test]
    fn a_song_plays_and_stops_by_itself() {
        let mut e = Engine::from_project(SR, &two_bar_demo());
        e.set_mode(crate::sequencer::Mode::Song);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01);
        render(&mut e, 4.0); // two bars at 128 BPM ≈ 3.75 s
        assert!(!e.is_playing(), "song ended");
    }

    #[test]
    fn automation_plays_in_song_mode_and_stop_restores_the_knob() {
        let mut p = two_bar_demo();
        crate::song::write_automation(&mut p.song, ParamId::Master(MasterParam::Level), 0, 0, 0.0);
        let mut e = Engine::from_project(SR, &p);
        e.set_mode(crate::sequencer::Mode::Song);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert_eq!(peak(&l[64..]), 0.0, "automation holds master at 0");
        e.stop();
        e.set_mode(crate::sequencer::Mode::Pattern);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01, "knob value restored by stop");
    }

    #[test]
    fn a_touched_param_ignores_automation() {
        let mut p = two_bar_demo();
        crate::song::write_automation(&mut p.song, ParamId::Master(MasterParam::Level), 0, 200, 0.0);
        let mut e = Engine::from_project(SR, &p);
        e.set_mode(crate::sequencer::Mode::Song);
        e.touch(ParamId::Master(MasterParam::Level), true);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01);
    }

    #[test]
    fn live_values_do_not_move_the_knob() {
        let mut e = demo_engine();
        e.play();
        e.set_param_live(ParamId::Master(MasterParam::Level), 0.0);
        let (l, _) = render(&mut e, 0.5);
        assert_eq!(peak(&l[64..]), 0.0);
        e.stop();
        e.play();
        let (l, _) = render(&mut e, 0.5);
        assert!(peak(&l) > 0.01, "stop restored the saved knob");
    }

    #[test]
    fn swap_song_validates() {
        let mut e = demo_engine();
        let mut good = two_bar_demo().song;
        assert!(e.swap_song(&mut good));
        let mut bad = crate::project::Song { bars: vec![Default::default(); crate::song::MAX_BARS + 1], ..Default::default() };
        assert!(!e.swap_song(&mut bad));
        assert_eq!(bad.bars.len(), crate::song::MAX_BARS + 1, "rejected song is untouched");
    }

    #[test]
    fn song_start_and_position() {
        let mut e = Engine::from_project(SR, &two_bar_demo());
        e.set_mode(crate::sequencer::Mode::Song);
        e.set_song_start(1);
        e.play();
        render(&mut e, 0.01);
        assert_eq!(e.song_bar(), 1);
        assert_eq!(e.song_step(), 16);
        assert_eq!(e.mode(), crate::sequencer::Mode::Song);
    }
```

Add to `host.rs` tests:
```rust
    #[test]
    fn song_commands_and_status() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let mut song = crate::project::Song::default();
        song.bars = vec![crate::project::Bar::default(); 4];
        tx.push(Command::Song(Box::new(song))).unwrap();
        tx.push(Command::SetMode(crate::sequencer::Mode::Song)).unwrap();
        tx.push(Command::SetSongStart(2)).unwrap();
        tx.push(Command::Play).unwrap();
        run(&mut host, 2048);
        let events: Vec<Event> = std::iter::from_fn(|| rx.pop().ok()).collect();
        assert!(events.iter().any(|e| matches!(e, Event::DisposeSong(_))));
        let status = events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Status(s) => Some(*s),
                _ => None,
            })
            .unwrap();
        assert_eq!(status.mode, crate::sequencer::Mode::Song);
        assert_eq!(status.song_bar, 2);
        assert_eq!(status.song_step, 32);
    }
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine`
Expected: compile errors.

- [ ] **Step 3: Implement the engine changes**

In `engine.rs`:
1. Add these imports: `use crate::project::{Song, PATTERNS};` (keep the existing ones), `use crate::sequencer::Mode;` and `use crate::song::MAX_BARS;`.
2. Add `const EVENT_RESERVE: usize = 64 + 2 * PARAM_COUNT;` and use `Vec::with_capacity(EVENT_RESERVE)` for `events`.
3. Add these fields next to `knobs` and initialise them in `new` (`base`: same as `knobs`; `touched`: `vec![false; PARAM_COUNT]`). The `song` field already exists from Task 2.
   ```rust
       /// Effective value per param: the knob, or song automation / live
       /// record value. Lane restores return here.
       base: Vec<f32>,
       /// Params held by the user while recording: automation skips them.
       touched: Vec<bool>,
   ```
4. In `load_project`, add `self.song = p.song.clone();`.
5. In `set_param`, after `self.knobs[i] = v;`, add `self.base[i] = v;`.
6. In `restore_param`, use `self.base[i]` instead of `self.knobs[i]`.
7. Replace `stop()` with:
   ```rust
       pub fn stop(&mut self) {
           self.seq.stop();
           for s in &mut self.synths {
               s.note_off();
           }
           self.base.copy_from_slice(&self.knobs);
           self.touched.fill(false);
           self.restore_all_knobs();
       }
   ```
8. Add these methods:
   ```rust
       /// Plays a value without changing the saved knob (record mode).
       pub fn set_param_live(&mut self, id: ParamId, v: f32) {
           let Some(i) = id.index() else { return };
           if !v.is_finite() {
               return;
           }
           let v = v.clamp(0.0, 1.0);
           self.base[i] = v;
           self.apply_param(id, v);
       }

       /// While touched, song automation for `id` is not applied.
       pub fn touch(&mut self, id: ParamId, on: bool) {
           if let Some(i) = id.index() {
               self.touched[i] = on;
           }
       }

       fn apply_automation(&mut self, id: ParamId, v: f32) {
           let Some(i) = id.index() else { return };
           if self.touched[i] || !v.is_finite() {
               return;
           }
           self.base[i] = v.clamp(0.0, 1.0);
           self.apply_param(id, self.base[i]);
       }

       /// Swaps in a new song (real-time safe). An invalid song leaves both
       /// sides untouched and returns false.
       pub fn swap_song(&mut self, song: &mut Song) -> bool {
           let refs_ok = song.bars.iter().flat_map(|b| b.patterns).flatten().all(|r| (r.0 as usize) < PATTERNS);
           let lanes_ok = song.automation.len() <= PARAM_COUNT && song.automation.iter().all(|l| l.param.is_valid());
           if song.bars.len() > MAX_BARS || !refs_ok || !lanes_ok {
               return false;
           }
           std::mem::swap(&mut self.song, song);
           true
       }

       pub fn set_mode(&mut self, mode: Mode) {
           self.seq.set_mode(mode);
       }

       pub fn mode(&self) -> Mode {
           self.seq.mode()
       }

       pub fn set_song_start(&mut self, bar: usize) {
           self.seq.set_song_start(bar);
       }

       pub fn song_bar(&self) -> usize {
           self.seq.song_bar()
       }

       pub fn song_step(&self) -> u32 {
           self.seq.song_step()
       }
   ```
9. In `process_block`, change the `Automation` arm to `SeqEvent::Automation { id, value } => self.apply_automation(id, value),`. After the event loop and before `continue;`, add:
   ```rust
                   if self.seq.take_ended() {
                       self.stop();
                   }
   ```

- [ ] **Step 4: Implement the host changes**

In `host.rs`:
1. Add the imports `use crate::project::Song;` and `use crate::sequencer::Mode;`.
2. Add these `Command` variants:
   ```rust
       SetMode(Mode),
       Song(Box<Song>),
       SetSongStart(u16),
       /// The user is holding (true) or released (false) a knob while recording.
       Touch(ParamId, bool),
       /// Sound follows the knob without changing its saved value (record mode).
       SetParamLive(ParamId, f32),
   ```
3. Add `DisposeSong(Box<Song>)` to `Event`.
4. Add these fields to `Status`:
   ```rust
       pub mode: Mode,
       pub song_bar: u16,
       /// Global song step (song mode).
       pub song_step: u32,
   ```
5. In `drain_commands`:
   ```rust
                   Command::SetMode(mode) => self.engine.set_mode(mode),
                   Command::Song(mut song) => {
                       self.engine.swap_song(&mut song);
                       self.dispose(Event::DisposeSong(song));
                   }
                   Command::SetSongStart(bar) => self.engine.set_song_start(bar as usize),
                   Command::Touch(id, on) => self.engine.touch(id, on),
                   Command::SetParamLive(id, v) => self.engine.set_param_live(id, v),
   ```
6. In `send_status`, fill the new fields:
   ```rust
               mode: self.engine.mode(),
               song_bar: self.engine.song_bar().min(u16::MAX as usize) as u16,
               song_step: self.engine.song_step(),
   ```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

The app crate must still compile. It never constructs `Status` literally, and its `match` over `Event` uses `if let`, so it should. If a `match` over `Command` or `Event` in the app needs the new arms, add them so they drop the box.

- [ ] **Step 6: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): songs, touch and live values through the engine and host"
```

---

### Task 4: Synth randomiser

**Files:**
- Modify: `Cargo.toml`, `crates/acid-engine/Cargo.toml`, `crates/acid-engine/src/lib.rs`
- Create: `crates/acid-engine/src/random.rs`

**Interfaces:**
- Produces, in `acid_engine::random`:
  - `Dice { notes, accent, slide, rest, octave: bool }`, which derives `Clone, Copy, Debug, Default, PartialEq, Eq` and has `Dice::ALL`.
  - `randomise_synth(p: &SynthPattern, dice: Dice, groove: &DrumPattern, seed: u64) -> SynthPattern`
- Rules:
  - The scale is chosen by weight.
  - Note movement is a Markov walk preferring the root, repeats, −1 and +2 degrees.
  - Leaps of more than 7 semitones are folded back by octaves.
  - Gate probability is higher on the groove's BD/SD steps.
  - Slides chain.
  - An accent never falls directly after a slide.
  - Only steps inside `length` change.

- [ ] **Step 1: Add the dependencies**

Root `Cargo.toml`, under `[workspace.dependencies]`, add:
```toml
rand = "0.8"
rand_pcg = "0.3"
```
`crates/acid-engine/Cargo.toml`, under `[dependencies]`, add:
```toml
rand.workspace = true
rand_pcg.workspace = true
```
`lib.rs`: add `pub mod random;`, keeping the list alphabetical.

- [ ] **Step 2: Write the failing tests**

`crates/acid-engine/src/random.rs`, tests first:
```rust
//! Seeded pattern randomisers (spec §11). Pure functions: the same seed
//! always gives the same pattern, so tests are deterministic.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drums::BD;

    fn groove_four_on_floor() -> DrumPattern {
        let mut g = DrumPattern::default();
        for s in [0, 4, 8, 12] {
            g.steps[s][BD] = DrumHit::On;
        }
        g
    }

    fn semis(s: &SynthStep) -> i32 {
        s.octave as i32 * 12 + s.note as i32
    }

    #[test]
    fn same_seed_same_pattern_different_seed_different() {
        let base = SynthPattern::default();
        let g = groove_four_on_floor();
        let a = randomise_synth(&base, Dice::ALL, &g, 7);
        assert_eq!(a, randomise_synth(&base, Dice::ALL, &g, 7));
        assert_ne!(a, randomise_synth(&base, Dice::ALL, &g, 8));
    }

    #[test]
    fn no_dice_changes_nothing() {
        let base = crate::demo::demo_project().synths[0].patterns[0].clone();
        assert_eq!(randomise_synth(&base, Dice::default(), &groove_four_on_floor(), 1), base);
    }

    #[test]
    fn each_die_touches_only_its_field() {
        let base = crate::demo::demo_project().synths[0].patterns[0].clone();
        let g = groove_four_on_floor();
        let r = randomise_synth(&base, Dice { rest: true, ..Dice::default() }, &g, 3);
        for (a, b) in base.steps.iter().zip(&r.steps) {
            assert_eq!((a.note, a.octave, a.accent, a.slide), (b.note, b.octave, b.accent, b.slide));
        }
        let r = randomise_synth(&base, Dice { notes: true, ..Dice::default() }, &g, 3);
        for (a, b) in base.steps.iter().zip(&r.steps) {
            assert_eq!((a.gate, a.accent, a.slide), (b.gate, b.accent, b.slide));
        }
    }

    #[test]
    fn notes_stay_in_range_and_leaps_are_folded() {
        let g = groove_four_on_floor();
        for seed in 0..200 {
            let r = randomise_synth(&SynthPattern::default(), Dice::ALL, &g, seed);
            for s in r.steps.iter().take(16) {
                assert!(s.note < 12 && (-1..=1).contains(&s.octave), "seed {seed}: {s:?}");
            }
            let n = randomise_synth(&SynthPattern::default(), Dice { notes: true, ..Dice::default() }, &g, seed);
            for w in n.steps[..16].windows(2) {
                assert!((semis(&w[1]) - semis(&w[0])).abs() <= 12, "seed {seed}");
            }
        }
    }

    #[test]
    fn never_an_accent_directly_after_a_slide() {
        let g = groove_four_on_floor();
        for seed in 0..300 {
            let r = randomise_synth(&SynthPattern::default(), Dice::ALL, &g, seed);
            for i in 1..16 {
                assert!(!(r.steps[i - 1].slide && r.steps[i].accent), "seed {seed} step {i}");
            }
        }
    }

    #[test]
    fn gates_lean_toward_the_groove() {
        let g = groove_four_on_floor();
        let (mut on_hit, mut on_off) = (0, 0);
        for seed in 0..300 {
            let r = randomise_synth(&SynthPattern::default(), Dice { rest: true, ..Dice::default() }, &g, seed);
            for i in 0..16 {
                if r.steps[i].gate != Gate::Off {
                    if i % 4 == 0 { on_hit += 1 } else { on_off += 1 }
                }
            }
        }
        let hit_rate = on_hit as f32 / (300.0 * 4.0);
        let off_rate = on_off as f32 / (300.0 * 12.0);
        assert!(hit_rate > off_rate + 0.2, "hit {hit_rate} off {off_rate}");
    }

    #[test]
    fn steps_beyond_the_length_are_untouched() {
        let mut base = SynthPattern::default();
        base.length = 8;
        let r = randomise_synth(&base, Dice::ALL, &groove_four_on_floor(), 5);
        assert_eq!(&r.steps[8..], &base.steps[8..]);
    }
}
```

- [ ] **Step 3: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine random`
Expected: compile errors.

- [ ] **Step 4: Implement the synth randomiser**

Insert above the tests:
```rust
use crate::drums::{BD, SD};
use crate::project::{DrumHit, DrumPattern, Gate, STEPS, SynthPattern, SynthStep};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dice {
    pub notes: bool,
    pub accent: bool,
    pub slide: bool,
    pub rest: bool,
    pub octave: bool,
}

impl Dice {
    pub const ALL: Dice = Dice { notes: true, accent: true, slide: true, rest: true, octave: true };
}

/// (scale degrees in semitones, selection weight)
const SCALES: [(&[i32], f32); 5] = [
    (&[0, 2, 3, 5, 7, 8, 10], 2.0), // natural minor
    (&[0, 2, 3, 5, 7, 9, 11], 2.0), // melodic minor
    (&[0, 1, 3, 5, 7, 8, 10], 1.0), // phrygian
    (&[0, 2, 4, 5, 7, 9, 11], 1.0), // major
    (&[0, 2, 3, 6, 7, 8, 11], 0.5), // hungarian minor
];
const MAX_LEAP: i32 = 7;
const LOWEST: i32 = -12;
const HIGHEST: i32 = 23;

fn pick_scale(rng: &mut Pcg32) -> &'static [i32] {
    let total: f32 = SCALES.iter().map(|(_, w)| w).sum();
    let mut r = rng.gen_range(0.0..total);
    for (scale, weight) in SCALES {
        if r < weight {
            return scale;
        }
        r -= weight;
    }
    SCALES[0].0
}

fn degree_to_semitone(scale: &[i32], degree: i32) -> i32 {
    let n = scale.len() as i32;
    scale[degree.rem_euclid(n) as usize] + 12 * degree.div_euclid(n)
}

fn length_of(len: u8) -> usize {
    (len as usize).clamp(1, STEPS)
}

/// Steps where the paired drum pattern has a kick or snare.
fn groove_hits(groove: &DrumPattern, len: usize) -> Vec<bool> {
    let glen = length_of(groove.length);
    (0..len)
        .map(|i| {
            let step = groove.steps[i % glen];
            step[BD] != DrumHit::Off || step[SD] != DrumHit::Off
        })
        .collect()
}

pub fn randomise_synth(p: &SynthPattern, dice: Dice, groove: &DrumPattern, seed: u64) -> SynthPattern {
    let mut rng = Pcg32::seed_from_u64(seed);
    let mut out = p.clone();
    let len = length_of(p.length);
    let hits = groove_hits(groove, len);
    if dice.rest {
        for i in 0..len {
            let prob = if hits[i] { 0.85 } else { 0.5 };
            out.steps[i].gate = if rng.gen_bool(prob) { Gate::On } else { Gate::Off };
        }
    }
    if dice.notes {
        let scale = pick_scale(&mut rng);
        let mut degree = 0i32;
        let mut prev: Option<i32> = None;
        for step in out.steps.iter_mut().take(len) {
            let r: f32 = rng.r#gen();
            degree = if r < 0.35 {
                0
            } else if r < 0.55 {
                degree
            } else if r < 0.70 {
                degree - 1
            } else if r < 0.85 {
                degree + 2
            } else {
                rng.gen_range(-3..=7)
            };
            let mut semis = degree_to_semitone(scale, degree);
            if let Some(p) = prev {
                while semis - p > MAX_LEAP {
                    semis -= 12;
                }
                while p - semis > MAX_LEAP {
                    semis += 12;
                }
            }
            while semis < LOWEST {
                semis += 12;
            }
            while semis > HIGHEST {
                semis -= 12;
            }
            prev = Some(semis);
            step.octave = semis.div_euclid(12) as i8;
            step.note = semis.rem_euclid(12) as u8;
        }
    }
    if dice.octave {
        for step in out.steps.iter_mut().take(len) {
            let r: f32 = rng.r#gen();
            if r < 0.08 {
                step.octave = (step.octave + 1).min(1);
            } else if r < 0.12 {
                step.octave = (step.octave - 1).max(-1);
            }
        }
    }
    if dice.slide {
        let mut prev = false;
        for step in out.steps.iter_mut().take(len) {
            let on = rng.gen_bool(if prev { 0.5 } else { 0.15 });
            step.slide = on;
            prev = on;
        }
    }
    if dice.accent {
        for i in 0..len {
            out.steps[i].accent = rng.gen_bool(if hits[i] { 0.35 } else { 0.2 });
        }
    }
    if dice.accent || dice.slide {
        for i in 1..len {
            if out.steps[i - 1].slide {
                out.steps[i].accent = false;
            }
        }
    }
    out
}
```
Edition 2024 reserves `gen`, so the code calls `rng.r#gen()`. If clippy prefers another form, `rng.gen_range(0.0..1.0)` is equivalent.

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/acid-engine
git commit -m "feat(engine): seeded groove-aware synth randomiser"
```

---

### Task 5: Drum templates and evolver

**Files:**
- Modify: `crates/acid-engine/src/random.rs`

**Interfaces:**
- Consumes: `Pcg32` usage and the instrument constants.
- Produces:
  - `drum_template(seed: u64, length: u8) -> DrumPattern`. It builds the kick style (four-on-the-floor or broken), a backbeat on SD or CP, and one of four hat styles with matching open hats, repeated every 16 steps.
  - `evolve_drums(p: &DrumPattern, seed: u64) -> DrumPattern`. It applies 1–3 weighted add/move/accent mutations.
- Both functions guarantee an open hat mutes the closed hat on the same step.

- [ ] **Step 1: Write the failing tests**

Add to `random.rs` tests:
```rust
    use crate::drums::{CH, CP, OH};

    fn hits(p: &DrumPattern) -> usize {
        p.steps.iter().flatten().filter(|h| **h != DrumHit::Off).count()
    }

    #[test]
    fn template_has_a_kick_on_one_and_a_backbeat() {
        for seed in 0..50 {
            let p = drum_template(seed, 16);
            assert_eq!(p.length, 16);
            assert_eq!(p.steps[0][BD], DrumHit::Accent, "seed {seed}");
            for s in [4, 12] {
                assert!(p.steps[s][SD] != DrumHit::Off || p.steps[s][CP] != DrumHit::Off, "seed {seed}");
            }
            assert!(p.steps.iter().all(|s| !(s[OH] != DrumHit::Off && s[CH] != DrumHit::Off)));
        }
    }

    #[test]
    fn template_repeats_over_longer_patterns() {
        let p = drum_template(3, 32);
        assert_eq!(p.length, 32);
        assert_eq!(p.steps[16][BD], DrumHit::Accent);
        assert_eq!(&p.steps[..16], &p.steps[16..32]);
        assert!(p.steps[32..].iter().flatten().all(|h| *h == DrumHit::Off));
    }

    #[test]
    fn template_is_deterministic() {
        assert_eq!(drum_template(9, 16), drum_template(9, 16));
    }

    #[test]
    fn evolve_makes_small_deterministic_changes() {
        let base = drum_template(1, 16);
        let mut changed = 0;
        for seed in 0..50 {
            let e = evolve_drums(&base, seed);
            assert_eq!(e, evolve_drums(&base, seed));
            let diff = base.steps.iter().flatten().zip(e.steps.iter().flatten()).filter(|(a, b)| a != b).count();
            assert!(diff <= 9, "seed {seed}: {diff} cells changed"); // 3 ops × (2 cells + a muted closed hat)
            if diff > 0 {
                changed += 1;
            }
            assert!(e.steps.iter().all(|s| !(s[OH] != DrumHit::Off && s[CH] != DrumHit::Off)));
            assert!(e.steps[16..].iter().flatten().all(|h| *h == DrumHit::Off), "stays inside the length");
        }
        assert!(changed >= 40, "evolve usually changes something ({changed}/50)");
    }

    #[test]
    fn evolving_an_empty_pattern_adds_hits() {
        let e = evolve_drums(&DrumPattern::default(), 11);
        assert!(hits(&e) >= 1);
    }
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine random`
Expected: compile errors.

- [ ] **Step 3: Implement drums**

Change the drums import to `use crate::drums::{BD, CH, CP, OH, SD};`, then add:
```rust
const KICKS: [&[usize]; 4] = [&[0, 4, 8, 12], &[0, 3, 6, 10], &[0, 6, 8, 11], &[0, 7, 10]];
/// (closed hat steps, open hat steps)
const HATS: [(&[usize], &[usize]); 4] = [
    (&[0, 4, 8, 12], &[2, 6, 10, 14]),
    (&[0, 2, 4, 6, 8, 10, 12, 14], &[]),
    (&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15], &[]),
    (&[0, 3, 4, 7, 8, 11, 12, 15], &[14]),
];
/// How likely each instrument is to gain a hit when evolving.
const INST_WEIGHTS: [f32; 8] = [1.0, 0.8, 0.4, 0.4, 0.5, 0.4, 2.0, 1.0];
/// Downbeats over offbeats over the rest.
const STEP_WEIGHTS: [f32; 4] = [3.0, 1.0, 2.0, 1.0];

fn open_mutes_closed(p: &mut DrumPattern) {
    for step in &mut p.steps {
        if step[OH] != DrumHit::Off {
            step[CH] = DrumHit::Off;
        }
    }
}

fn weighted(rng: &mut Pcg32, weights: &[f32]) -> usize {
    let total: f32 = weights.iter().sum();
    let mut r = rng.gen_range(0.0..total);
    for (i, w) in weights.iter().enumerate() {
        if r < *w {
            return i;
        }
        r -= w;
    }
    weights.len() - 1
}

fn random_hit(rng: &mut Pcg32, p: &DrumPattern, len: usize) -> Option<(usize, usize)> {
    let count = p.steps[..len].iter().flatten().filter(|h| **h != DrumHit::Off).count();
    if count == 0 {
        return None;
    }
    let mut k = rng.gen_range(0..count);
    for (step, row) in p.steps[..len].iter().enumerate() {
        for (inst, hit) in row.iter().enumerate() {
            if *hit != DrumHit::Off {
                if k == 0 {
                    return Some((step, inst));
                }
                k -= 1;
            }
        }
    }
    None
}

pub fn drum_template(seed: u64, length: u8) -> DrumPattern {
    let mut rng = Pcg32::seed_from_u64(seed);
    let len = length_of(length);
    let mut p = DrumPattern { length: len as u8, ..DrumPattern::default() };
    let kick = if rng.gen_bool(0.5) { KICKS[0] } else { KICKS[rng.gen_range(1..KICKS.len())] };
    let backbeat = if rng.gen_bool(0.5) { SD } else { CP };
    let (closed, open) = HATS[rng.gen_range(0..HATS.len())];
    for block in (0..len).step_by(16) {
        let mut put = |step: usize, inst: usize, hit: DrumHit| {
            if block + step < len {
                p.steps[block + step][inst] = hit;
            }
        };
        for &s in kick {
            put(s, BD, if s == 0 { DrumHit::Accent } else { DrumHit::On });
        }
        for s in [4, 12] {
            put(s, backbeat, DrumHit::On);
        }
        for &s in closed {
            put(s, CH, DrumHit::On);
        }
        for &s in open {
            put(s, OH, DrumHit::On);
        }
    }
    open_mutes_closed(&mut p);
    p
}

pub fn evolve_drums(p: &DrumPattern, seed: u64) -> DrumPattern {
    let mut rng = Pcg32::seed_from_u64(seed);
    let mut out = p.clone();
    let len = length_of(p.length);
    let step_weights: Vec<f32> = (0..len).map(|i| STEP_WEIGHTS[i % STEP_WEIGHTS.len()]).collect();
    for _ in 0..rng.gen_range(1..=3) {
        let op: f32 = rng.r#gen();
        if op < 0.5 {
            let inst = weighted(&mut rng, &INST_WEIGHTS);
            let step = weighted(&mut rng, &step_weights);
            if out.steps[step][inst] == DrumHit::Off {
                out.steps[step][inst] = DrumHit::On;
            }
        } else if op < 0.8 {
            if let Some((step, inst)) = random_hit(&mut rng, &out, len) {
                let to = if rng.gen_bool(0.5) { (step + 1) % len } else { (step + len - 1) % len };
                if out.steps[to][inst] == DrumHit::Off {
                    out.steps[to][inst] = out.steps[step][inst];
                    out.steps[step][inst] = DrumHit::Off;
                }
            }
        } else if let Some((step, inst)) = random_hit(&mut rng, &out, len) {
            out.steps[step][inst] = match out.steps[step][inst] {
                DrumHit::On => DrumHit::Accent,
                _ => DrumHit::On,
            };
        }
    }
    open_mutes_closed(&mut out);
    out
}
```
`evolve_drums` allocates a small `Vec`. That's fine: it runs on the UI thread.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

- [ ] **Step 5: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): drum templates and evolver"
```

---

### Task 6: Song render and `Edit::Song`

**Files:**
- Modify: `crates/acid-engine/src/render.rs`, `crates/app/src/document.rs`

**Interfaces:**
- Produces:
  - `render::MAX_SONG_SECONDS: usize = 3600`.
  - `render::render_song(project: &Project, progress: impl FnMut(f32)) -> (Vec<f32>, Vec<f32>)`. It ignores the loop range. `progress` runs from 0 to 1 and ends with 1.0.
  - `render::render_song_wav(project: &Project, path: &Path, progress: impl FnMut(f32)) -> Result<(), hound::Error>`.
  - `document::Edit::Song { song: Box<Song> }`, with merge key `MergeKey::Song`. Its command is `Command::Song(Box<Song>)`, carrying the project's song after the edit.

- [ ] **Step 1: Write the failing tests**

Add to `render.rs` tests:
```rust
    fn two_bar_song() -> Project {
        let mut p = crate::demo::demo_project();
        p.bpm = 120.0;
        let bar = crate::project::Bar { patterns: [Some(crate::project::PatternRef(0)); 4], ..Default::default() };
        p.song.bars = vec![bar, bar];
        p
    }

    #[test]
    fn song_render_covers_the_song_plus_tail() {
        let mut seen = Vec::new();
        let (l, r) = render_song(&two_bar_song(), |f| seen.push(f));
        assert_eq!(l.len(), r.len());
        let sr = EXPORT_SAMPLE_RATE as usize;
        assert!(l.len() >= 4 * sr, "two bars at 120 BPM = 4 s");
        assert!(l.len() <= 15 * sr);
        assert!(rms(&l[..4 * sr]) > 0.02);
        assert!(seen.windows(2).all(|w| w[1] >= w[0]), "progress is monotonic");
        assert_eq!(seen.last().copied(), Some(1.0));
    }

    #[test]
    fn looped_song_still_ends_when_exported() {
        let mut p = two_bar_song();
        p.song.loop_range = Some((0, 1));
        let (l, _) = render_song(&p, |_| {});
        assert!(l.len() <= 15 * EXPORT_SAMPLE_RATE as usize);
    }

    #[test]
    fn empty_song_renders_almost_nothing() {
        let (l, _) = render_song(&crate::demo::demo_project(), |_| {});
        assert!(l.len() <= 2 * EXPORT_SAMPLE_RATE as usize);
    }
```

Add to `document.rs` tests:
```rust
    #[test]
    fn song_edits_merge_into_one_record_pass() {
        use acid_engine::project::Bar;
        let mut d = doc();
        for n in 1..=5 {
            let mut song = d.project.song.clone();
            song.bars = vec![Bar::default(); n];
            let cmds = d.apply(Edit::Song { song: Box::new(song) }, true);
            assert!(matches!(cmds.as_slice(), [Command::Song(s)] if s.bars.len() == n));
        }
        d.end_gesture();
        d.undo();
        assert!(d.project.song.bars.is_empty(), "the whole pass undoes at once");
        d.redo();
        assert_eq!(d.project.song.bars.len(), 5);
    }
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test --workspace`
Expected: compile errors.

- [ ] **Step 3: Implement song rendering**

In `render.rs`:
1. Add the imports `use crate::sequencer::Mode;` and `use crate::song;`.
2. Factor the tail loop out of `render_patterns` into `fn render_tail(engine: &mut Engine, l: &mut Vec<f32>, r: &mut Vec<f32>)`, and call it from `render_patterns` so behaviour is unchanged.
3. Add:
   ```rust
   pub const MAX_SONG_SECONDS: usize = 3600;

   /// Renders the whole song from bar 1 (the loop range is ignored so the
   /// export always ends), then lets FX tails ring out. `progress` gets
   /// 0..=1 and finishes with 1.0.
   pub fn render_song(project: &Project, mut progress: impl FnMut(f32)) -> (Vec<f32>, Vec<f32>) {
       let sr = EXPORT_SAMPLE_RATE as f32;
       let mut p = project.clone();
       p.song.loop_range = None;
       let mut engine = Engine::from_project(sr, &p);
       engine.set_mode(Mode::Song);
       engine.set_song_start(0);
       engine.play();
       let (synths, drums) = song::banks(&p);
       let steps = song::total_steps(&p.song, synths, drums).max(1) as f64;
       let step_samples = 60.0 / p.bpm.clamp(40.0, 300.0) as f64 / 4.0 * sr as f64;
       let expected = steps * step_samples;
       let cap = MAX_SONG_SECONDS * EXPORT_SAMPLE_RATE as usize;
       let (mut l, mut r) = (Vec::new(), Vec::new());
       let mut cl = vec![0.0; CHUNK];
       let mut cr = vec![0.0; CHUNK];
       progress(0.0);
       while engine.is_playing() && l.len() < cap {
           engine.process(&mut cl, &mut cr);
           l.extend_from_slice(&cl);
           r.extend_from_slice(&cr);
           progress((l.len() as f64 / expected).min(0.99) as f32);
       }
       engine.stop();
       render_tail(&mut engine, &mut l, &mut r);
       progress(1.0);
       (l, r)
   }

   pub fn render_song_wav(project: &Project, path: &Path, progress: impl FnMut(f32)) -> Result<(), hound::Error> {
       let (l, r) = render_song(project, progress);
       write_wav(path, &l, &r)
   }
   ```
   An empty song ends on its first tick, so the output is one chunk plus a short silent tail.

- [ ] **Step 4: Implement `Edit::Song`**

In `document.rs`:
1. Import `Song` from `acid_engine::project`.
2. Add the `Edit` variant `Song { song: Box<Song> }` and the `MergeKey` variant `Song`.
3. In `merge_key`: `Edit::Song { .. } => MergeKey::Song,`.
4. In `apply_raw`:
   ```rust
               Edit::Song { song } => Edit::Song { song: Box::new(std::mem::replace(&mut p.song, *song)) },
   ```
5. In `commands_for`:
   ```rust
               Edit::Song { .. } => vec![Command::Song(Box::new(p.song.clone()))],
   ```
If `Edit::Song` is unused outside tests until Task 7, put `#[allow(dead_code)] // used by Task 7` on that variant only.

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add crates
git commit -m "feat: offline song render with progress, and undoable song edits"
```

---

### Task 7: Transport modes, recording and automated knobs

**Files:**
- Create: `crates/app/src/ui/record.rs`
- Modify: `crates/app/src/ui/mod.rs`, `ui/transport.rs`, `ui/keys.rs`, `ui/params_ui.rs`, `ui/knob.rs`, `ui/pads.rs`

**Interfaces:**
- Consumes: `Mode`, the new `Command`s, the new `Status` fields, `Edit::Song`, and `song::{write_automation, write_pattern_ref, is_automated, lane}`.
- Produces:
  - New `ViewState` fields:
    - `mode: Mode`, `record: bool`, `song_cursor: usize`
    - `live: std::collections::HashMap<ParamId, f32>`
    - `rec_last: Option<(ParamId, u32)>`
    - `was_playing: bool`
  - New `SidAcidApp` methods: `recording`, `set_mode`, `set_record`, `stop_recording`, `play`, `stop`, `record_automation`, `record_pattern_change` and `release_knob`.
  - `knob::knob` gains a final parameter, `automated: bool`, which draws an outer ring.
- Behaviour while recording (`record` is armed, the mode is Song and the song is playing):
  - Turning a knob sends `SetParamLive` and shows a live value. The saved knob value doesn't change.
  - The first frame a knob is touched sends `Touch(id, true)`. Releasing it sends `Touch(id, false)`.
  - Each frame, the touched knob's live value is written into the song from the last recorded step up to `status.song_step`, filling any skipped steps. All writes form one `Edit::Song` gesture.
  - A pad click writes the pattern reference from the current bar onwards.
  - The gesture closes when Rec is disarmed, Stop is pressed, the mode changes, or the song stops.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `ui/mod.rs`:
```rust
    fn recording_app() -> SidAcidApp {
        let mut doc = Document::new(acid_engine::demo::demo_project());
        doc.project.song.bars = vec![acid_engine::project::Bar::default(); 4];
        let mut app = SidAcidApp::headless(doc);
        app.view.mode = acid_engine::sequencer::Mode::Song;
        app.view.record = true;
        app.status.playing = true;
        app
    }

    #[test]
    fn recording_writes_held_knob_values_as_one_undo_step() {
        use acid_engine::params::{ParamId, SynthParam};
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        let mut app = recording_app();
        app.view.touched_knob = Some(id);
        app.view.live.insert(id, 0.7);
        app.status.song_step = 4;
        app.record_automation();
        app.view.live.insert(id, 0.9);
        app.status.song_step = 7;
        app.record_automation();
        let lane = acid_engine::song::lane(&app.doc.project.song, id).unwrap();
        let points: Vec<(u32, f32)> = lane.points.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(points, vec![(4, 0.7), (5, 0.9), (6, 0.9), (7, 0.9)]);
        assert_eq!(app.doc.project.param(id), acid_engine::demo::demo_project().param(id), "saved knob untouched");
        app.stop_recording();
        app.doc.undo();
        assert!(app.doc.project.song.automation.is_empty());
    }

    #[test]
    fn nothing_is_recorded_unless_armed_in_song_mode_and_playing() {
        use acid_engine::params::{ParamId, SynthParam};
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        for case in 0..3 {
            let mut app = recording_app();
            match case {
                0 => app.view.record = false,
                1 => app.view.mode = acid_engine::sequencer::Mode::Pattern,
                _ => app.status.playing = false,
            }
            app.view.touched_knob = Some(id);
            app.view.live.insert(id, 0.7);
            app.record_automation();
            assert!(app.doc.project.song.automation.is_empty(), "case {case}");
        }
    }

    #[test]
    fn recorded_pad_change_fills_following_bars() {
        let mut app = recording_app();
        app.status.song_bar = 1;
        app.record_pattern_change(0, 9);
        let refs: Vec<_> = app.doc.project.song.bars.iter().map(|b| b.patterns[0].map(|r| r.0)).collect();
        assert_eq!(refs, vec![None, Some(9), Some(9), Some(9)]);
    }

    #[test]
    fn switching_mode_stops_and_disarms() {
        let mut app = recording_app();
        app.pending.clear();
        app.set_mode(acid_engine::sequencer::Mode::Pattern);
        assert!(!app.view.record);
        assert!(matches!(app.pending.as_slice(), [Command::Stop, Command::SetMode(acid_engine::sequencer::Mode::Pattern)]));
    }

    #[test]
    fn stop_twice_returns_to_the_loop_start() {
        let mut app = recording_app();
        app.doc.project.song.loop_range = Some((1, 2));
        app.status.playing = false;
        app.view.song_cursor = 3;
        app.stop();
        assert_eq!(app.view.song_cursor, 1);
        app.doc.project.song.loop_range = None;
        app.stop();
        assert_eq!(app.view.song_cursor, 0);
    }

    #[test]
    fn song_play_sends_the_start_bar_first() {
        let mut app = recording_app();
        app.status.playing = false;
        app.view.song_cursor = 2;
        app.pending.clear();
        app.play();
        assert!(matches!(app.pending.as_slice(), [Command::SetSongStart(2), Command::Play]));
    }
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p sidacid`
Expected: compile errors.

- [ ] **Step 3: Implement record.rs**

`crates/app/src/ui/record.rs`:
```rust
//! Transport modes and recording: Pattern/Song switch, Rec arm, play/stop
//! (stop twice returns to the start), and writing touched-knob automation
//! and pad changes into the song as one undoable record pass.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::params::ParamId;
use acid_engine::sequencer::Mode;
use acid_engine::song;

impl SidAcidApp {
    pub(crate) fn recording(&self) -> bool {
        self.view.record && self.view.mode == Mode::Song && self.status.playing
    }

    pub(crate) fn set_mode(&mut self, mode: Mode) {
        if self.view.mode == mode {
            return;
        }
        self.stop_recording();
        self.view.record = false;
        self.view.mode = mode;
        self.send(vec![Command::Stop, Command::SetMode(mode)]);
    }

    pub(crate) fn set_record(&mut self, on: bool) {
        if !on {
            self.stop_recording();
        }
        self.view.record = on && self.view.mode == Mode::Song;
    }

    /// Ends the current record pass: one undo step, live values dropped.
    pub(crate) fn stop_recording(&mut self) {
        self.doc.end_gesture();
        if let Some(id) = self.view.touched_knob {
            self.send(vec![Command::Touch(id, false)]);
        }
        self.view.live.clear();
        self.view.rec_last = None;
    }

    pub(crate) fn play(&mut self) {
        if self.view.mode == Mode::Song {
            self.send(vec![Command::SetSongStart(self.view.song_cursor as u16)]);
        }
        self.send(vec![Command::Play]);
    }

    /// Stop; pressed while already stopped, returns to the song start.
    pub(crate) fn stop(&mut self) {
        if !self.status.playing {
            self.view.song_cursor = self.doc.project.song.loop_range.map_or(0, |(a, _)| a as usize);
            self.send(vec![Command::SetSongStart(self.view.song_cursor as u16)]);
        }
        if self.view.record {
            self.stop_recording();
        }
        self.send(vec![Command::Stop]);
    }

    /// Writes the held knob's live value into the song up to the current step.
    pub(crate) fn record_automation(&mut self) {
        if !self.recording() {
            self.view.rec_last = None;
            return;
        }
        let Some(id) = self.view.touched_knob else {
            self.view.rec_last = None;
            return;
        };
        let Some(&value) = self.view.live.get(&id) else { return };
        let step = self.status.song_step;
        if song::lane(&self.doc.project.song, id).and_then(|l| l.points.get(&step)) == Some(&value) {
            return;
        }
        let from = match self.view.rec_last {
            Some((p, s)) if p == id && s < step => s + 1,
            _ => step,
        };
        let mut new_song = self.doc.project.song.clone();
        song::write_automation(&mut new_song, id, from, step, value);
        let cmds = self.doc.apply(Edit::Song { song: Box::new(new_song) }, true);
        self.send(cmds);
        self.view.rec_last = Some((id, step));
    }

    /// While recording, a pad click also writes the pattern into the song.
    pub(crate) fn record_pattern_change(&mut self, machine: usize, slot: u8) {
        if !self.recording() {
            return;
        }
        let bar = self.status.song_bar as usize;
        if bar >= self.doc.project.song.bars.len() {
            return;
        }
        let mut new_song = self.doc.project.song.clone();
        song::write_pattern_ref(&mut new_song, bar, machine, slot);
        let cmds = self.doc.apply(Edit::Song { song: Box::new(new_song) }, true);
        self.send(cmds);
    }

    /// The knob's gesture is over: release its touch, and outside a record
    /// pass close its undo gesture.
    pub(crate) fn release_knob(&mut self, id: ParamId) {
        if self.view.touched_knob != Some(id) {
            return;
        }
        self.view.touched_knob = None;
        self.send(vec![Command::Touch(id, false)]);
        if !self.recording() {
            self.doc.end_gesture();
        }
    }
}
```

- [ ] **Step 4: Wire it into the UI**

1. **`ui/mod.rs`**
   - Add `mod record;`.
   - Add these `ViewState` fields:
     ```rust
         /// Pattern or Song transport mode (not saved).
         pub mode: acid_engine::sequencer::Mode,
         /// Record armed (song mode only).
         pub record: bool,
         /// Song bar playback starts from, and the bar the song editor shows.
         pub song_cursor: usize,
         /// Knob values heard while recording (saved knobs stay unchanged).
         pub live: std::collections::HashMap<acid_engine::params::ParamId, f32>,
         /// Last step recorded for the held knob.
         pub rec_last: Option<(acid_engine::params::ParamId, u32)>,
         pub was_playing: bool,
     ```
   - In `ui()`, right after `self.handle_shortcuts(ctx);`, add:
     ```rust
             self.record_automation();
             if self.view.was_playing && !self.status.playing && self.view.record {
                 self.stop_recording();
             }
             self.view.was_playing = self.status.playing;
     ```
   - In `files.rs` `load_document`, also do `self.view.live.clear(); self.view.rec_last = None; self.view.record = false; self.view.song_cursor = 0;`.

2. **`ui/knob.rs`**
   - Add the final parameter `automated: bool` to `knob`.
   - After the body circle is painted, add:
     ```rust
         if automated {
             painter.circle_stroke(center, radius + 6.0, Stroke::new(1.5_f32, theme::SLIDE));
         }
     ```

3. **`ui/params_ui.rs`**
   - Rewrite the continuous branch of `param_knob`. Keep the discrete-switch branch as is.
     ```rust
             let recording = self.recording();
             let value = if recording {
                 self.view.live.get(&id).copied().unwrap_or_else(|| self.doc.project.param(id))
             } else {
                 self.doc.project.param(id)
             };
             let automated = acid_engine::song::is_automated(&self.doc.project.song, id);
             let tip = format!(
                 "{} {:.0}%{}  (drag, Shift = fine, double-click = reset)",
                 info.name,
                 value * 100.0,
                 if automated { "  · automated in song" } else { "" }
             );
             let out = knob::knob(ui, label, value, info.default, &tip, automated);
             if let Some(v) = out.value {
                 if recording {
                     self.view.live.insert(id, v);
                     self.send(vec![Command::SetParamLive(id, v)]);
                 } else {
                     let cmds = self.doc.apply(Edit::Param { id, value: v }, true);
                     self.send(cmds);
                 }
             }
             if out.touched {
                 if self.view.touched_knob != Some(id) && recording {
                     self.send(vec![Command::Touch(id, true)]);
                 }
                 self.view.touched_knob = Some(id);
             } else if self.view.touched_knob == Some(id) {
                 self.release_knob(id);
             }
             if out.released {
                 self.release_knob(id);
             }
     ```
   - Add `use acid_engine::host::Command;`.
   - Add a pure label helper and test it. Make `drum_panel::INST_NAMES` and `mixer_panel::CHANNEL_NAMES` `pub(crate)` if they aren't already.
     ```rust
     /// "Synth 1 Cutoff", "808 BD Tune", "909 Volume", "Synth 2 Level", "Feedback", "Master Ratio".
     pub fn param_label(id: ParamId) -> String {
         const KITS: [&str; 2] = ["808", "909"];
         let pick = |names: &[&'static str], i: u8| names.get(i as usize).copied().unwrap_or("?");
         match id {
             ParamId::Synth(s, p) => format!("Synth {} {}", s + 1, p.name()),
             ParamId::Drum(k, i, p) => format!("{} {} {}", pick(&KITS, k), pick(&super::drum_panel::INST_NAMES, i), p.name()),
             ParamId::Kit(k, p) => format!("{} {}", pick(&KITS, k), p.name()),
             ParamId::Channel(c, p) => format!("{} {}", pick(&super::mixer_panel::CHANNEL_NAMES, c), p.name()),
             ParamId::Fx(p) => p.name().to_string(),
             ParamId::Master(p) => format!("Master {}", p.name()),
         }
     }
     ```
     Test:
     ```rust
         #[test]
         fn param_labels_name_the_machine() {
             use acid_engine::params::{DrumParam, MasterParam, SynthParam};
             assert_eq!(param_label(ParamId::Synth(0, SynthParam::Cutoff)), "Synth 1 Cutoff");
             assert_eq!(param_label(ParamId::Drum(1, 0, DrumParam::Tune)), "909 BD Tune");
             assert_eq!(param_label(ParamId::Master(MasterParam::CompRatio)), "Master Ratio");
         }
     ```

4. **`ui/transport.rs`**
   - Replace the Play and Stop button handlers with `self.play()` and `self.stop()`.
   - After the Stop button add:
     ```rust
                 ui.separator();
                 for (m, label) in [(Mode::Pattern, "Pattern"), (Mode::Song, "Song")] {
                     if ui.selectable_label(self.view.mode == m, label).clicked() {
                         self.set_mode(m);
                     }
                 }
                 let rec_text = egui::RichText::new("Rec").color(if self.view.record { theme::ACCENT } else { ui.visuals().text_color() });
                 let rec = ui.add_enabled(self.view.mode == Mode::Song, egui::Button::new(rec_text).selected(self.view.record));
                 if rec.on_hover_text("Record pattern changes and knob moves into the song").clicked() {
                     self.set_record(!self.view.record);
                 }
                 if self.view.mode == Mode::Song {
                     let bar = if self.status.playing { self.status.song_bar as usize } else { self.view.song_cursor };
                     ui.label(format!("Bar {}", bar + 1));
                 }
     ```
   - Add `use acid_engine::sequencer::Mode;`.

5. **`ui/keys.rs`:** the `PlayStop` arm becomes `if self.status.playing { self.stop() } else { self.play() }`.

6. **`ui/pads.rs`:** after `self.send(cmds); self.view.focus = machine;` in the pad click, add `self.record_pattern_change(machine, slot);`.

- [ ] **Step 5: Run the tests and smoke-test**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

Then run `cargo build --release -p sidacid && timeout 10 ./target/release/sidacid; echo exit=$?`.
Expected: exit 124.

- [ ] **Step 6: Commit**

```bash
git add crates/app
git commit -m "feat(app): pattern/song transport, record arm, knob automation recording"
```

---

### Task 8: Song editor

**Files:**
- Modify: `crates/acid-engine/src/song.rs` (bar operations)
- Create: `crates/app/src/ui/song_panel.rs`
- Modify: `crates/app/src/ui/mod.rs`

**Interfaces:**
- **New in `song.rs`:**
  - `insert_bars(song: &mut Song, at: usize, bars: &[Bar], synths, drums)`. It respects `MAX_BARS`, shifts later automation by the inserted steps, and shifts loop indices at or after `at`.
  - `delete_bars(song: &mut Song, first: usize, last: usize, synths, drums)`. It removes the bars' automation, shifts later points back, and clears the loop range if a loop endpoint is deleted.
  - `duplicate_bars(song: &mut Song, first: usize, last: usize, synths, drums)`. It inserts a copy right after `last`, including the range's automation.
- **New in the app:**
  - `SidAcidApp::song_panel(ui)`.
  - The `ViewState` fields `song_sel_end: Option<usize>` and `song_clip: Option<Vec<Bar>>`.

- [ ] **Step 1: Write the failing tests**

Add to `song.rs` tests:
```rust
    fn sixteen_step_song(n: usize) -> (Project, ParamId) {
        let mut p = Project::default();
        p.song.bars = vec![Bar::default(); n];
        (p, ParamId::Master(MasterParam::Level))
    }

    #[test]
    fn insert_shifts_automation_and_loop() {
        let (mut p, id) = sixteen_step_song(3);
        write_automation(&mut p.song, id, 5, 5, 0.1);
        write_automation(&mut p.song, id, 20, 20, 0.2);
        p.song.loop_range = Some((1, 2));
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        insert_bars(&mut p.song, 1, &[Bar::default(), Bar::default()], s, d);
        assert_eq!(p.song.bars.len(), 5);
        let keys: Vec<u32> = lane(&p.song, id).unwrap().points.keys().copied().collect();
        assert_eq!(keys, vec![5, 52], "points from bar 1 on move by 32 steps");
        assert_eq!(p.song.loop_range, Some((3, 4)));
    }

    #[test]
    fn delete_removes_and_shifts_automation_and_clears_a_broken_loop() {
        let (mut p, id) = sixteen_step_song(4);
        write_automation(&mut p.song, id, 5, 5, 0.1);
        write_automation(&mut p.song, id, 20, 20, 0.2);
        write_automation(&mut p.song, id, 40, 40, 0.3);
        p.song.loop_range = Some((2, 3));
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        delete_bars(&mut p.song, 1, 1, s, d);
        assert_eq!(p.song.bars.len(), 3);
        let keys: Vec<u32> = lane(&p.song, id).unwrap().points.keys().copied().collect();
        assert_eq!(keys, vec![5, 24]);
        assert_eq!(p.song.loop_range, Some((1, 2)), "loop after the cut moves back");
        delete_bars(&mut p.song, 2, 2, s, d);
        assert_eq!(p.song.loop_range, None, "loop endpoint deleted");
    }

    #[test]
    fn duplicate_copies_bars_and_their_automation() {
        let (mut p, id) = sixteen_step_song(2);
        p.song.bars[1].mutes[2] = true;
        write_automation(&mut p.song, id, 18, 18, 0.6);
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        duplicate_bars(&mut p.song, 1, 1, s, d);
        assert_eq!(p.song.bars.len(), 3);
        assert!(p.song.bars[2].mutes[2]);
        let keys: Vec<u32> = lane(&p.song, id).unwrap().points.keys().copied().collect();
        assert_eq!(keys, vec![18, 34]);
    }

    #[test]
    fn bar_count_is_capped() {
        let (mut p, _) = sixteen_step_song(MAX_BARS - 1);
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        insert_bars(&mut p.song, 0, &[Bar::default(); 5], s, d);
        assert_eq!(p.song.bars.len(), MAX_BARS);
    }
```
The tests read the banks from a `snapshot` clone so the song can be mutated while the banks are borrowed. The UI does the same with an owned clone of the project.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine song`
Expected: compile errors.

- [ ] **Step 3: Implement the bar operations**

Add to `song.rs`:
```rust
/// Moves every automation point at or after `from` by `delta` steps.
fn shift_automation(song: &mut Song, from: u32, delta: i64) {
    for lane in &mut song.automation {
        let moved: Vec<(u32, f32)> = lane.points.range(from..).map(|(k, v)| (*k, *v)).collect();
        for (k, _) in &moved {
            lane.points.remove(k);
        }
        for (k, v) in moved {
            let to = k as i64 + delta;
            if to >= 0 {
                lane.points.insert(to as u32, v);
            }
        }
    }
}

pub fn insert_bars(song: &mut Song, at: usize, bars: &[Bar], synths: SynthBanks<'_>, drums: DrumBanks<'_>) {
    let at = at.min(song.bars.len());
    let room = MAX_BARS.saturating_sub(song.bars.len());
    let bars = &bars[..bars.len().min(room)];
    if bars.is_empty() {
        return;
    }
    let start = bar_start_step(song, at, synths, drums);
    let added: u32 = bars.iter().map(|b| bar_steps(b, synths, drums) as u32).sum();
    shift_automation(song, start, added as i64);
    song.bars.splice(at..at, bars.iter().copied());
    if let Some((a, b)) = song.loop_range {
        let n = bars.len() as u16;
        let bump = |i: u16| if i as usize >= at { i + n } else { i };
        song.loop_range = Some((bump(a), bump(b)));
    }
}

pub fn delete_bars(song: &mut Song, first: usize, last: usize, synths: SynthBanks<'_>, drums: DrumBanks<'_>) {
    if song.bars.is_empty() {
        return;
    }
    let last = last.min(song.bars.len() - 1);
    if first > last {
        return;
    }
    let start = bar_start_step(song, first, synths, drums);
    let end = bar_start_step(song, last + 1, synths, drums);
    for lane in &mut song.automation {
        lane.points.retain(|k, _| *k < start || *k >= end);
    }
    shift_automation(song, end, -((end - start) as i64));
    song.bars.drain(first..=last);
    let n = (last - first + 1) as u16;
    song.loop_range = song.loop_range.and_then(|(a, b)| {
        let map = |i: u16| -> Option<u16> {
            if (i as usize) < first {
                Some(i)
            } else if i as usize > last {
                Some(i - n)
            } else {
                None
            }
        };
        Some((map(a)?, map(b)?))
    });
}

pub fn duplicate_bars(song: &mut Song, first: usize, last: usize, synths: SynthBanks<'_>, drums: DrumBanks<'_>) {
    if first > last || last >= song.bars.len() {
        return;
    }
    let copy: Vec<Bar> = song.bars[first..=last].to_vec();
    let src_start = bar_start_step(song, first, synths, drums);
    let src_end = bar_start_step(song, last + 1, synths, drums);
    let points: Vec<(ParamId, Vec<(u32, f32)>)> = song
        .automation
        .iter()
        .map(|l| (l.param, l.points.range(src_start..src_end).map(|(k, v)| (k - src_start, *v)).collect()))
        .collect();
    let before = song.bars.len();
    insert_bars(song, last + 1, &copy, synths, drums);
    if song.bars.len() == before {
        return;
    }
    let dst = bar_start_step(song, last + 1, synths, drums);
    for (param, pts) in points {
        for (offset, v) in pts {
            write_automation(song, param, dst + offset, dst + offset, v);
        }
    }
}
```

- [ ] **Step 4: Implement the song panel**

`crates/app/src/ui/song_panel.rs`:
```rust
//! Song editor: bar timeline (click selects, Shift-click extends), the
//! selected bar's patterns/mutes/transpose, bar operations, loop markers,
//! and the list of automated parameters.

use super::params_ui::param_label;
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::project::{Bar, PatternRef, Song};
use acid_engine::sequencer::Mode;
use acid_engine::song;
use eframe::egui;

const MACHINES: [&str; 4] = ["Synth 1", "Synth 2", "808", "909"];

impl SidAcidApp {
    /// Selected bar range (first, last), clamped to the song.
    fn song_selection(&self) -> Option<(usize, usize)> {
        let n = self.doc.project.song.bars.len();
        if n == 0 {
            return None;
        }
        let a = self.view.song_cursor.min(n - 1);
        let b = self.view.song_sel_end.unwrap_or(a).min(n - 1);
        Some((a.min(b), a.max(b)))
    }

    fn apply_song(&mut self, song: Song, merge: bool) {
        let cmds = self.doc.apply(Edit::Song { song: Box::new(song) }, merge);
        self.send(cmds);
    }

    fn bar_from_selected_patterns(&self) -> Bar {
        Bar { patterns: self.doc.project.selected.map(|s| Some(PatternRef(s))), ..Bar::default() }
    }

    pub(crate) fn song_panel(&mut self, ui: &mut egui::Ui) {
        let title = format!("SONG   {} bars", self.doc.project.song.bars.len());
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt("song")
            .default_open(true)
            .show(ui, |ui| {
                self.song_toolbar(ui);
                self.song_timeline(ui);
                self.bar_editor(ui);
                self.automation_list(ui);
            });
    }

    fn song_toolbar(&mut self, ui: &mut egui::Ui) {
        let project = self.doc.project.clone();
        let (synths, drums) = song::banks(&project);
        let sel = self.song_selection();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add bar").on_hover_text("Append a bar using each machine's selected pattern").clicked() {
                let mut s = project.song.clone();
                let at = s.bars.len();
                song::insert_bars(&mut s, at, &[self.bar_from_selected_patterns()], synths, drums);
                self.view.song_cursor = at;
                self.view.song_sel_end = None;
                self.apply_song(s, false);
            }
            if ui.button("Insert").clicked() {
                let mut s = project.song.clone();
                let at = sel.map_or(s.bars.len(), |(a, _)| a);
                let bar = sel.map_or_else(|| self.bar_from_selected_patterns(), |(a, _)| s.bars[a]);
                song::insert_bars(&mut s, at, &[bar], synths, drums);
                self.apply_song(s, false);
            }
            if let Some((a, b)) = sel {
                if ui.button("Duplicate").clicked() {
                    let mut s = project.song.clone();
                    song::duplicate_bars(&mut s, a, b, synths, drums);
                    self.apply_song(s, false);
                }
                if ui.button("Delete").clicked() {
                    let mut s = project.song.clone();
                    song::delete_bars(&mut s, a, b, synths, drums);
                    self.view.song_cursor = a.min(s.bars.len().saturating_sub(1));
                    self.view.song_sel_end = None;
                    self.apply_song(s, false);
                }
                if ui.button("Copy").clicked() {
                    self.view.song_clip = Some(project.song.bars[a..=b].to_vec());
                }
                if ui.button("Loop selection").clicked() {
                    let mut s = project.song.clone();
                    s.loop_range = Some((a as u16, b as u16));
                    self.apply_song(s, false);
                }
            }
            if let Some(clip) = self.view.song_clip.clone()
                && ui.button(format!("Paste {} bars", clip.len())).clicked()
            {
                let mut s = project.song.clone();
                let at = sel.map_or(s.bars.len(), |(_, b)| b + 1);
                song::insert_bars(&mut s, at, &clip, synths, drums);
                self.apply_song(s, false);
            }
            if project.song.loop_range.is_some() && ui.button("No loop").clicked() {
                let mut s = project.song.clone();
                s.loop_range = None;
                self.apply_song(s, false);
            }
        });
    }

    fn song_timeline(&mut self, ui: &mut egui::Ui) {
        let bars = self.doc.project.song.bars.clone();
        let loop_range = self.doc.project.song.loop_range;
        let sel = self.song_selection();
        let playing_bar = (self.status.playing && self.view.mode == Mode::Song).then_some(self.status.song_bar as usize);
        let mut clicked = None;
        egui::ScrollArea::horizontal().id_salt("song_timeline").show(ui, |ui| {
            ui.horizontal(|ui| {
                if bars.is_empty() {
                    ui.weak("No bars yet — press \"Add bar\".");
                }
                for (i, bar) in bars.iter().enumerate() {
                    let cell = |m: usize| {
                        let label = bar.patterns[m].map_or("—".to_string(), |r| r.label());
                        if bar.mutes[m] { format!("{label}×") } else { label }
                    };
                    let text = format!("{}\n{}\n{}\n{}\n{}", i + 1, cell(0), cell(1), cell(2), cell(3));
                    let mut rich = egui::RichText::new(text).monospace().size(10.0);
                    if Some(i) == playing_bar {
                        rich = rich.color(theme::ACID).strong();
                    } else if loop_range.is_some_and(|(a, b)| (a as usize..=b as usize).contains(&i)) {
                        rich = rich.color(theme::QUEUED);
                    }
                    let selected = sel.is_some_and(|(a, b)| (a..=b).contains(&i));
                    let r = ui.add_sized([44.0, 84.0], egui::Button::new(rich).selected(selected));
                    if r.clicked() {
                        clicked = Some((i, ui.input(|inp| inp.modifiers.shift)));
                    }
                }
            });
        });
        if let Some((i, extend)) = clicked {
            if extend {
                self.view.song_sel_end = Some(i);
            } else {
                self.view.song_cursor = i;
                self.view.song_sel_end = None;
                if !self.status.playing {
                    self.send(vec![Command::SetSongStart(i as u16)]);
                }
            }
        }
    }

    fn bar_editor(&mut self, ui: &mut egui::Ui) {
        let Some((a, _)) = self.song_selection() else { return };
        let mut bar = self.doc.project.song.bars[a];
        let selected = self.doc.project.selected;
        let mut changed = false;
        let mut merge = false;
        let mut finished = false;
        ui.label(egui::RichText::new(format!("Bar {}", a + 1)).strong());
        egui::Grid::new("bar_editor").striped(true).show(ui, |ui| {
            for m in 0..4 {
                ui.label(MACHINES[m]);
                ui.monospace(bar.patterns[m].map_or("—".to_string(), |r| r.label()));
                if ui.button(format!("Use {}", PatternRef(selected[m]).label())).clicked() {
                    bar.patterns[m] = Some(PatternRef(selected[m]));
                    changed = true;
                }
                if ui.button("Clear").clicked() {
                    bar.patterns[m] = None;
                    changed = true;
                }
                if ui.checkbox(&mut bar.mutes[m], "Mute").changed() {
                    changed = true;
                }
                if m < 2 {
                    let r = ui.add(egui::DragValue::new(&mut bar.transpose[m]).range(-12..=12).prefix("Trans "));
                    if r.changed() {
                        changed = true;
                        merge = true;
                    }
                    finished |= r.drag_stopped() || r.lost_focus();
                }
                ui.end_row();
            }
        });
        if changed {
            let mut s = self.doc.project.song.clone();
            s.bars[a] = bar;
            self.apply_song(s, merge);
        }
        if finished {
            self.doc.end_gesture();
        }
    }

    fn automation_list(&mut self, ui: &mut egui::Ui) {
        let lanes: Vec<_> = self.doc.project.song.automation.iter().map(|l| (l.param, l.points.len())).collect();
        if lanes.is_empty() {
            return;
        }
        ui.separator();
        ui.label(egui::RichText::new("Automation").strong());
        for (param, points) in lanes {
            ui.horizontal(|ui| {
                ui.label(format!("{}  ({points} steps)", param_label(param)));
                if ui.button("Clear").clicked() {
                    let mut s = self.doc.project.song.clone();
                    song::clear_automation(&mut s, param);
                    self.apply_song(s, false);
                }
            });
        }
        if ui.button("Clear all automation").clicked() {
            let mut s = self.doc.project.song.clone();
            s.automation.clear();
            self.apply_song(s, false);
        }
    }
}
```

In `ui/mod.rs`:
1. Add `mod song_panel;`.
2. Add these `ViewState` fields:
   ```rust
       /// Shift-click end of the song bar selection.
       pub song_sel_end: Option<usize>,
       /// Bars copied in the song editor.
       pub song_clip: Option<Vec<acid_engine::project::Bar>>,
   ```
3. Add `self.song_panel(ui);` at the end of `rack`.
4. In `files.rs` `load_document`, also reset `song_sel_end = None`.

Add to `ui/mod.rs` tests:
```rust
    #[test]
    fn song_panel_draws_and_bar_ops_are_undoable() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        run_frames(&mut app, 2);
        let mut song = app.doc.project.song.clone();
        song.bars = vec![acid_engine::project::Bar::default(); 3];
        app.apply_song(song, false);
        app.view.song_cursor = 0;
        app.view.song_sel_end = Some(1);
        assert_eq!(app.song_selection(), Some((0, 1)));
        run_frames(&mut app, 2);
        app.doc.undo();
        assert!(app.doc.project.song.bars.is_empty());
    }
```
`apply_song` and `song_selection` are private to `song_panel.rs`. Make them `pub(crate)` so the test in `mod.rs` can call them.

- [ ] **Step 5: Run the tests and smoke-test**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

Then run `cargo build --release -p sidacid && timeout 10 ./target/release/sidacid; echo exit=$?`.
Expected: exit 124.

- [ ] **Step 6: Commit**

```bash
git add crates
git commit -m "feat: song editor — timeline, bar editing, bar ops with automation, loop, automation list"
```

---

### Task 9: Randomise buttons and song export

**Files:**
- Modify: `crates/app/src/ui/synth_panel.rs`, `ui/drum_panel.rs`, `ui/files.rs`, `ui/mod.rs`

**Interfaces:**
- **Consumes:** `random::{Dice, randomise_synth, drum_template, evolve_drums}` and `render::render_song_wav`.
- **Produces:**
  - `ViewState` fields:
    - `seed_counter: u64`
    - `export_song: bool`
    - `export_progress: Option<std::sync::Arc<std::sync::atomic::AtomicU32>>`
  - `SidAcidApp` methods:
    - `next_seed`
    - `randomise_synth(synth, dice)`
    - `drum_template(kit)`
    - `evolve_drums(kit)`
  - `files::export_status(song: bool, bars: u32, progress: Option<f32>) -> String`.
- **Behaviour:**
  - Each dice click is one undoable whole-pattern edit on the selected pattern.
  - Synth 1's groove comes from the 808's selected pattern, and Synth 2's from the 909's.
  - Export offers "Loop (N bars)" or "Song". Song export is disabled while the song has no bars, and it shows its percentage in the status line.

- [ ] **Step 1: Write the failing tests**

Add to `ui/mod.rs` tests:
```rust
    #[test]
    fn dice_and_drum_buttons_are_single_undo_steps() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let before = app.doc.project.clone();
        app.randomise_synth(0, acid_engine::random::Dice::ALL);
        assert_ne!(app.doc.project.synths[0].patterns[0], before.synths[0].patterns[0]);
        app.doc.undo();
        assert_eq!(app.doc.project, before);
        app.drum_template(1);
        assert_ne!(app.doc.project.drums[1].patterns[0], before.drums[1].patterns[0]);
        app.doc.undo();
        app.evolve_drums(0);
        app.doc.undo();
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn seeds_differ_between_clicks() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        assert_ne!(app.next_seed(), app.next_seed());
    }
```
Add to `ui/files.rs` (new `#[cfg(test)] mod tests`):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_status_text() {
        assert_eq!(export_status(false, 8, None), "Exporting 8 bars…");
        assert_eq!(export_status(true, 0, Some(0.425)), "Exporting song 42%…");
        assert_eq!(export_status(true, 0, None), "Exporting song…");
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p sidacid`
Expected: compile errors.

- [ ] **Step 3: Implement randomise actions and buttons**

In `ui/mod.rs`:
1. Add the `ViewState` fields `seed_counter: u64`, `export_song: bool` and `export_progress: Option<std::sync::Arc<std::sync::atomic::AtomicU32>>`.
2. Add an `impl SidAcidApp` block. Put it at the bottom of `mod.rs`, or in `synth_panel.rs` if you prefer:
   ```rust
   impl SidAcidApp {
       /// A fresh seed per click (time mixed with a counter).
       pub(crate) fn next_seed(&mut self) -> u64 {
           self.view.seed_counter = self.view.seed_counter.wrapping_add(1);
           let nanos = std::time::SystemTime::now()
               .duration_since(std::time::UNIX_EPOCH)
               .map(|d| d.as_nanos() as u64)
               .unwrap_or(0);
           nanos ^ self.view.seed_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15)
       }

       pub(crate) fn randomise_synth(&mut self, synth: usize, dice: acid_engine::random::Dice) {
           let seed = self.next_seed();
           let p = &self.doc.project;
           let slot = p.selected[synth];
           let groove = &p.drums[synth].patterns[p.selected[2 + synth] as usize];
           let pattern = acid_engine::random::randomise_synth(&p.synths[synth].patterns[slot as usize], dice, groove, seed);
           let cmds = self.doc.apply(crate::document::Edit::SynthPattern { synth, slot, pattern: Box::new(pattern) }, false);
           self.send(cmds);
           self.view.focus = synth;
       }

       pub(crate) fn drum_template(&mut self, kit: usize) {
           let seed = self.next_seed();
           let slot = self.doc.project.selected[2 + kit];
           let length = self.doc.project.drums[kit].patterns[slot as usize].length;
           let pattern = acid_engine::random::drum_template(seed, length);
           let cmds = self.doc.apply(crate::document::Edit::DrumPattern { kit, slot, pattern: Box::new(pattern) }, false);
           self.send(cmds);
           self.view.focus = 2 + kit;
       }

       pub(crate) fn evolve_drums(&mut self, kit: usize) {
           let seed = self.next_seed();
           let slot = self.doc.project.selected[2 + kit];
           let pattern = acid_engine::random::evolve_drums(&self.doc.project.drums[kit].patterns[slot as usize], seed);
           let cmds = self.doc.apply(crate::document::Edit::DrumPattern { kit, slot, pattern: Box::new(pattern) }, false);
           self.send(cmds);
           self.view.focus = 2 + kit;
       }
   }
   ```

In `ui/synth_panel.rs`, below the pads/Len/Trans row and before the editor, add:
```rust
                ui.horizontal(|ui| {
                    use acid_engine::random::Dice;
                    ui.label("Dice");
                    let dice = [
                        ("Notes", Dice { notes: true, ..Dice::default() }),
                        ("Accent", Dice { accent: true, ..Dice::default() }),
                        ("Slide", Dice { slide: true, ..Dice::default() }),
                        ("Rest", Dice { rest: true, ..Dice::default() }),
                        ("Octave", Dice { octave: true, ..Dice::default() }),
                        ("All", Dice::ALL),
                    ];
                    for (label, d) in dice {
                        if ui.button(label).on_hover_text("Randomise the selected pattern (undoable)").clicked() {
                            self.randomise_synth(synth, d);
                        }
                    }
                });
```

In `ui/drum_panel.rs`, in the row holding the pads and the Len `DragValue`, after Len, add:
```rust
                    ui.separator();
                    if ui.button("Template").on_hover_text("Replace with a fresh groove (undoable)").clicked() {
                        self.drum_template(kit);
                    }
                    if ui.button("Evolve").on_hover_text("Small random changes (undoable)").clicked() {
                        self.evolve_drums(kit);
                    }
```

- [ ] **Step 4: Implement song export with progress**

In `ui/files.rs`:
1. Add the helper:
   ```rust
   pub fn export_status(song: bool, bars: u32, progress: Option<f32>) -> String {
       match (song, progress) {
           (true, Some(p)) => format!("Exporting song {:.0}%…", (p * 100.0).floor()),
           (true, None) => "Exporting song…".to_string(),
           (false, _) => format!("Exporting {bars} bars…"),
       }
   }
   ```
2. In the export dialog, rename the window to "Export to WAV". Above the bars `DragValue`, add:
   ```rust
                       let has_song = !self.doc.project.song.bars.is_empty();
                       if !has_song {
                           self.view.export_song = false;
                       }
                       ui.radio_value(&mut self.view.export_song, false, "Loop of the selected patterns");
                       ui.add_enabled_ui(has_song, |ui| {
                           ui.radio_value(&mut self.view.export_song, true, "Whole song");
                       });
   ```
   Show the bars `DragValue` only when `!self.view.export_song`.
3. In `start_export`:
   - Use the default file name `"{name}-song.wav"` when exporting a song.
   - For a song:
     ```rust
             let progress = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
             let shared = progress.clone();
             std::thread::spawn(move || {
                 let result = acid_engine::render::render_song_wav(&project, &path, |f| {
                     shared.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed);
                 })
                 .map(|()| path)
                 .map_err(|e| e.to_string());
                 let _ = tx.send(result);
             });
             self.view.export_progress = Some(progress);
             self.view.status_line = Some(export_status(true, 0, Some(0.0)));
     ```
   - For a loop, keep the existing code, and set the status line with `export_status(false, bars, None)`.
4. In `poll_export`:
   - Before `try_recv`, if `export_progress` is set, refresh the status line with `export_status(true, 0, Some(f32::from_bits(p.load(Relaxed))))`.
   - On completion, clear `export_progress`.

- [ ] **Step 5: Run the tests and smoke-test**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

Then run `cargo build --release -p sidacid && timeout 10 ./target/release/sidacid; echo exit=$?`.
Expected: exit 124.

- [ ] **Step 6: Commit**

```bash
git add crates/app
git commit -m "feat(app): dice, drum template/evolve buttons, and song export with progress"
```

---

## Spec coverage (Plan 3)

| Spec section | Covered by |
|---|---|
| §8 Song mode: bars, bar length = longest pattern, per-bar restart, mutes, transpose, None = silent, loop, stop at end; global automation steps | Tasks 1, 2 |
| §8 Transport: Record (song mode only), Pattern/Song switch, stop twice → start | Task 7 |
| §10 Song editor: timeline, bar refs/mutes/transpose, insert/delete/duplicate/copy/paste, loop markers | Task 8 |
| §10 Record: pattern changes into bars; touched-only knob overwrite while held; one undo step per pass | Tasks 3, 6, 7 |
| §10 Automation playback at step boundaries; coloured ring; clear per param / all; ignored in pattern mode | Tasks 2, 3, 7, 8 |
| §11 Synth dice (Notes/Accent/Slide/Rest/Octave/All; weighted scales; Markov; leap folding; groove-aware; no accent after slide) | Tasks 4, 9 |
| §11 Drum Template and Evolve; seeded RNG | Tasks 5, 9 |
| §12 Song export with progress; loop export kept | Tasks 6, 9 |
| Plan 2 final-review notes: song position in Status, engine base value for restores, record pass as one undo step, event buffer sized for automation | Tasks 3, 6, 7 |
