# SidAcid Plan 2 — App, Live Audio and Editors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Plan 1 engine into a playable desktop app:
- a window showing both synths, both drum kits and the mixer;
- live audio;
- the classic 303 step editor and a piano roll, swappable per synth;
- 8 banks × 16 pattern pads per machine;
- undo and redo;
- New, Open, Save and Save As, plus pattern copy and paste;
- an "export loop to WAV" command.

**Architecture:** The UI thread owns the authoritative `Project` inside a `Document`, which also holds undo. Every edit produces `Command`s. These go through a lock-free `rtrb` ring to an `EngineHost`, which owns the `Engine` on the cpal audio thread. The host sends back `Event`s: a ~60 Hz `Status` with playheads, meters and CPU load, and the old pattern or engine boxes, so memory is always freed on the UI thread. To load a project, the UI builds a new `Engine` and swaps it in whole. The GUI uses eframe/egui 0.31 with custom-painted knobs and grids.

**Tech Stack:** Rust 1.99 (edition 2024), eframe/egui 0.31 (glow, wayland, x11), cpal 0.15, rtrb 0.3, rfd 0.15. All four were confirmed to build and open a window and audio stream on this machine (SteamOS, Wayland, 44.1 kHz default device).

**Spec:** `docs/superpowers/specs/2026-10-11-sid-acid-design.md`. Plan 1, merged, is `docs/superpowers/plans/2026-10-11-sidacid-plan1-engine.md`.

## Global Constraints

- **Code base:** Rust edition 2024, workspace resolver 3. `sid-core` stays dependency-free.
- **Dependencies:** `acid-engine` may add `rtrb`, but no GUI or audio-device crates. Only `crates/app` uses eframe, cpal and rfd.
- **Audio-thread rules:** this covers `EngineHost::process` and everything it calls.
  - No allocation, no freeing, no locks and no I/O.
  - Replaced patterns and engines go back to the UI thread inside `Event`s.
  - If the event ring is full, the box is leaked with `std::mem::forget`. That is a rare, bounded leak, never a free on the audio thread.
- **UI thread owns the project:** the UI never touches the `Engine` directly once audio is running. All changes go through `Command`.
- **Machine indices:** 0 = Synth 1, 1 = Synth 2, 2 = 808 kit, 3 = 909 kit. Mixer channels use the same order.
- **Pattern limits:** 128 patterns per machine (banks A–H × 16), 64 steps per pattern, length 1–64.
- **Window size:** 1280×800 default, 960×600 minimum (Steam Deck screen).
- **Undo:** 200 levels. A knob drag, BPM drag or piano-roll drag is one undo step.
- **egui version:** use the eframe 0.31 / egui 0.31 API. If a call signature differs slightly from this plan, for example a builder method name, make the minimal equivalent change and note it in your report. Never change behaviour.
- **Test command:** `cargo test --workspace`, run from the repo root. GUI behaviour is checked with headless `egui::Context::run` frames plus unit tests of the pure helpers.

### Deliberate deviations from the spec (rulings; don't flag these)

1. **Step-write keys (spec §9):** the spec says Z/X change octave and A/S toggle accent/slide. Those keys collide with the standard tracker piano layout, where Z, S and X are notes. This plan uses:
   - **Note rows:** Z-row = one octave and Q-row = the octave above (Z S X D C V G B H N J M, Q 2 W 3 E R 5 T 6 Y 7 U I).
   - **Octave:** `[` and `]`.
   - **Step marks:** A = accent, F = slide, `-` = tie, Space = rest.
   - **Navigation:** arrows move, Backspace clears the previous step, Esc leaves step-write.
2. **Param lanes (spec §9):** a lane value overrides the knob for that step only. Steps with no lane value play the knob's value. Stop restores every knob. Plan 1 left a lane's value stuck in place.
3. **Pattern/Song switch and REC (spec §13):** these belong to Plan 3, so the Plan 2 transport has only Play and Stop.
4. **Export:** this plan exports the selected patterns as a loop of N bars (1–64). Song export comes in Plan 3.
5. **Collapsing:** panels collapse with egui `CollapsingHeader`, which keeps its open/closed state in egui memory. It is not saved in the project.
6. **Starting project:** the app opens on the built-in demo project, unnamed and not dirty.

---

## File Structure

```
crates/acid-engine/src/
  params.rs        (modify) ParamId::index, PARAM_COUNT
  sequencer.rs     (modify) SeqEvent::ParamRestore, lane override tracking
  engine.rs        (modify) knob memory + lane restore, from_project, swap_*_pattern,
                            take_meters, take_guard_fired, queued_patterns, sample_rate
  mixer.rs         (modify) per-channel + master peak meters
  host.rs          (new)    Command, Event, Status, EngineHost, channel()
crates/app/src/
  main.rs          (modify) GUI by default; `render-demo` kept
  audio.rs         (new)    AudioLink (cpal stream), fill_interleaved
  document.rs      (new)    Document, Edit, undo/redo, edit -> Command
  ui/mod.rs        (new)    SidAcidApp (eframe::App), ViewState, command pump, rack
  ui/theme.rs      (new)    colours and visuals
  ui/knob.rs       (new)    knob + switch widgets, drag maths
  ui/params_ui.rs  (new)    param_knob helper, discrete value names
  ui/transport.rs  (new)    transport bar
  ui/pads.rs       (new)    bank + pattern pads
  ui/step_edit.rs  (new)    pure 303 helpers + StepWriter + key mapping
  ui/step303.rs    (new)    303 step editor
  ui/synth_panel.rs(new)    synth panel
  ui/roll_edit.rs  (new)    pure piano-roll note editing + geometry
  ui/pianoroll.rs  (new)    piano roll editor
  ui/drum_panel.rs (new)    drum machine panel
  ui/mixer_panel.rs(new)    mixer, FX returns, master
  ui/files.rs      (new)    new/open/save/export, unsaved dialog, clipboard, title
  ui/keys.rs       (new)    global shortcuts
```

---

### Task 1: Param-lane overrides that restore to the knob value

**Files:**
- Modify: `crates/acid-engine/src/params.rs`, `crates/acid-engine/src/sequencer.rs`, `crates/acid-engine/src/engine.rs`

**Interfaces:**
- Produces:
  - `params::PARAM_COUNT: usize` (181).
  - `ParamId::index(self) -> Option<usize>`, which returns the id's position in `ParamId::all()`, or `None` if the id is invalid.
  - `SeqEvent::ParamRestore { id: ParamId }`, a new variant.
- Behaviour:
  - `Engine::set_param` remembers knob values.
  - Lane `Param` events apply without being remembered.
  - `ParamRestore` re-applies the remembered value.
  - `Engine::stop()` re-applies every remembered knob.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `params.rs`:
```rust
    #[test]
    fn index_matches_registry_order() {
        let all = ParamId::all();
        assert_eq!(all.len(), PARAM_COUNT);
        for (i, id) in all.iter().enumerate() {
            assert_eq!(id.index(), Some(i), "{id:?}");
        }
        assert_eq!(ParamId::Synth(2, SynthParam::Tune).index(), None);
        assert_eq!(ParamId::Drum(0, 8, DrumParam::Tune).index(), None);
    }
```

Add to the `tests` module in `sequencer.rs`:
```rust
    #[test]
    fn lane_override_is_restored_on_the_next_step_without_a_value() {
        use crate::params::{ParamId, SynthParam};
        use crate::project::ParamLane;
        let mut f = Fixture::new();
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        let mut values = vec![None; 64];
        values[0] = Some(0.8);
        f.synths[0][0].param_lane = Some(ParamLane { param: id, values });
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 6000);
        assert_eq!(ev, vec![(0, SeqEvent::Param { id, value: 0.8 }), (5513, SeqEvent::ParamRestore { id })]);
    }
```

Add to the `tests` module in `engine.rs`:
```rust
    #[test]
    fn lane_value_overrides_only_its_step() {
        use crate::project::ParamLane;
        let mut e = demo_engine();
        let mut p = crate::demo::demo_project().synths[0].patterns[0].clone();
        let mut values = vec![None; 64];
        values[0] = Some(0.0);
        p.param_lane = Some(ParamLane { param: ParamId::Master(MasterParam::Level), values });
        e.set_synth_pattern(0, 0, p);
        e.play();
        let (l, _) = render(&mut e, 0.5);
        // Step 0 (about 5500 samples at the demo's 128 BPM + swing) is silent...
        assert_eq!(peak(&l[64..5000]), 0.0);
        // ...then the knob value comes back.
        assert!(peak(&l[6000..20_000]) > 0.01);
    }

    #[test]
    fn stop_restores_knobs_overridden_by_a_lane() {
        use crate::project::ParamLane;
        let mut e = demo_engine();
        let mut p = crate::demo::demo_project().synths[0].patterns[0].clone();
        p.param_lane = Some(ParamLane { param: ParamId::Master(MasterParam::Level), values: vec![Some(0.0); 64] });
        e.set_synth_pattern(0, 0, p);
        e.play();
        render(&mut e, 0.3);
        e.stop();
        e.set_synth_pattern(0, 0, crate::demo::demo_project().synths[0].patterns[0].clone());
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01, "master level was restored by stop()");
    }
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine`
Expected: compile errors (`PARAM_COUNT`, `index` and `ParamRestore` don't exist yet).

- [ ] **Step 3: Implement `ParamId::index` and `PARAM_COUNT`**

In `params.rs`, below the `SYNTHS`/`KITS`/`CHANNELS`/`DRUM_INSTRUMENTS` constants:
```rust
/// Number of entries in `ParamId::all()`.
pub const PARAM_COUNT: usize = 181;
```
Add this inside `impl ParamId`, after `is_valid`:
```rust
    /// Position of this id in `ParamId::all()`; `None` for an invalid id.
    pub fn index(self) -> Option<usize> {
        if !self.is_valid() {
            return None;
        }
        let synth_n = SynthParam::ALL.len();
        let drum_n = DrumParam::ALL.len();
        let per_kit = DRUM_INSTRUMENTS * drum_n + KitParam::ALL.len();
        let kit_base = SYNTHS * synth_n;
        let channel_base = kit_base + KITS * per_kit;
        let fx_base = channel_base + CHANNELS * ChannelParam::ALL.len();
        let master_base = fx_base + FxParam::ALL.len();
        Some(match self {
            ParamId::Synth(s, p) => s as usize * synth_n + p as usize,
            ParamId::Drum(k, i, p) => kit_base + k as usize * per_kit + i as usize * drum_n + p as usize,
            ParamId::Kit(k, p) => kit_base + k as usize * per_kit + DRUM_INSTRUMENTS * drum_n + p as usize,
            ParamId::Channel(c, p) => channel_base + c as usize * ChannelParam::ALL.len() + p as usize,
            ParamId::Fx(p) => fx_base + p as usize,
            ParamId::Master(p) => master_base + p as usize,
        })
    }
```

- [ ] **Step 4: Emit `ParamRestore` from the sequencer**

In `sequencer.rs`:
1. Add the variant to `SeqEvent`:
   ```rust
       ParamRestore { id: ParamId },
   ```
2. Add the field `lane_override: Option<ParamId>` to `Track`. `Track` derives `Default`, so it starts as `None`.
3. In both `play()` and `stop()`, inside the existing `for t in &mut self.tracks` loop, add `t.lane_override = None;`.
4. In `tick`, `Phase::StepStart`, synth loop, replace the existing param-lane statement (the `if let Some(lane) = &pat.param_lane && let Some(Some(value)) = lane.values.get(t.step) { out.push(SeqEvent::Param { … }); }` block) with:
   ```rust
                    let lane_value = pat
                        .param_lane
                        .as_ref()
                        .and_then(|lane| lane.values.get(t.step).copied().flatten().map(|v| (lane.param, v)));
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
   ```
   If the borrow checker objects because `t` is `&mut self.tracks[s]` while `pat` borrows `synths`, the two are disjoint, so it should be fine. If it still objects, read `t.step` into a local first.

- [ ] **Step 5: Remember knobs in the engine**

In `engine.rs`:
1. Add `use crate::params::PARAM_COUNT;` to the imports.
2. Add these fields to `Engine`:
   ```rust
       /// Knob values set through `set_param`, indexed by `ParamId::index`.
       knobs: Vec<f32>,
       /// `ParamId::all()`, so knobs can be re-applied without allocating.
       ids: Vec<ParamId>,
   ```
3. Initialise them in `Engine::new`:
   ```rust
               knobs: ParamId::all().iter().map(|id| id.info().default).collect(),
               ids: ParamId::all(),
   ```
4. Replace `set_param` with the following, and add the new helpers:
   ```rust
       /// Out-of-range ids and non-finite values are ignored; values are
       /// clamped to 0..=1. The value is remembered as the knob position.
       pub fn set_param(&mut self, id: ParamId, v: f32) {
           let Some(i) = id.index() else { return };
           if !v.is_finite() {
               return;
           }
           let v = v.clamp(0.0, 1.0);
           self.knobs[i] = v;
           self.apply_param(id, v);
       }

       /// Applies a value without remembering it (param-lane overrides).
       fn apply_param(&mut self, id: ParamId, v: f32) {
           if !id.is_valid() || !v.is_finite() {
               return;
           }
           let v = v.clamp(0.0, 1.0);
           match id {
               ParamId::Synth(s, p) => self.synths[s as usize].set_param(p, v),
               ParamId::Drum(k, i, p) => self.kits[k as usize].set_param(i as usize, p, v),
               ParamId::Kit(k, p) => self.kits[k as usize].set_kit_param(p, v),
               _ => self.mixer.set_param(id, v),
           }
       }

       fn restore_param(&mut self, id: ParamId) {
           if let Some(i) = id.index() {
               let v = self.knobs[i];
               self.apply_param(id, v);
           }
       }

       fn restore_all_knobs(&mut self) {
           for i in 0..PARAM_COUNT {
               let (id, v) = (self.ids[i], self.knobs[i]);
               self.apply_param(id, v);
           }
       }
   ```
5. In `stop()`, after the `note_off` loop, add `self.restore_all_knobs();`.
6. In `process_block`'s event match, change the `Param` arm and add `ParamRestore`:
   ```rust
                           SeqEvent::Param { id, value } => self.apply_param(id, value),
                           SeqEvent::ParamRestore { id } => self.restore_param(id),
   ```

- [ ] **Step 6: Run the tests to confirm they pass**

Run: `cargo test --workspace`
Expected: all pass. `cargo clippy --workspace --all-targets` must report no warnings, and `cargo fmt --check` must be clean (run `cargo fmt` if needed).

- [ ] **Step 7: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): param-lane values override one step and restore to the knob"
```

---

### Task 2: Engine API for the live host

**Files:**
- Modify: `crates/acid-engine/src/engine.rs`, `crates/acid-engine/src/mixer.rs`

**Interfaces:**
- Produces:
  - `Engine::from_project(sample_rate: f32, p: &Project) -> Engine`
  - `Engine::sample_rate(&self) -> f32`
  - `Engine::swap_synth_pattern(&mut self, synth: usize, slot: u8, p: &mut SynthPattern) -> bool`
  - `Engine::swap_drum_pattern(&mut self, kit: usize, slot: u8, p: &mut DrumPattern) -> bool`
  - `Engine::queued_patterns(&self) -> [Option<u8>; 4]`
  - `Engine::take_meters(&mut self) -> [f32; 5]`: peaks since the last call, for channels 0–3 post-fader and the master post-clip. The call resets them.
  - `Engine::take_guard_fired(&mut self) -> bool`
  - `Mixer::take_meters(&mut self) -> [f32; 5]`
- `set_synth_pattern` and `set_drum_pattern` keep their behaviour, now implemented through the swap methods.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `engine.rs`:
```rust
    #[test]
    fn from_project_matches_load_project() {
        let p = crate::demo::demo_project();
        let mut a = Engine::from_project(SR, &p);
        let mut b = Engine::new(SR);
        b.load_project(&p);
        a.play();
        b.play();
        let (la, _) = render(&mut a, 0.5);
        let (lb, _) = render(&mut b, 0.5);
        assert_eq!(la, lb);
        assert_eq!(a.sample_rate(), SR);
    }

    #[test]
    fn swap_exchanges_patterns_and_rejects_bad_ones() {
        let mut e = demo_engine();
        let mut p = SynthPattern::default();
        assert!(e.swap_synth_pattern(0, 0, &mut p));
        assert!(p.steps.iter().any(|s| s.gate != crate::project::Gate::Off), "got the old pattern back");
        let mut bad = SynthPattern::default();
        bad.steps.pop();
        assert!(!e.swap_synth_pattern(0, 1, &mut bad));
        assert_eq!(bad.steps.len(), 63, "rejected pattern is untouched");
        let mut d = DrumPattern::default();
        assert!(e.swap_drum_pattern(1, 0, &mut d));
        assert!(!e.swap_drum_pattern(2, 0, &mut d));
    }

    #[test]
    fn meters_report_and_reset() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.5);
        let m = e.take_meters();
        assert!(m[0] > 0.01 && m[2] > 0.01 && m[4] > 0.01, "{m:?}");
        assert!(m.iter().all(|v| *v <= 1.0 + 1e-6));
        e.stop();
        render(&mut e, 5.0);
        e.take_meters();
        render(&mut e, 0.05);
        assert!(e.take_meters().iter().all(|v| *v < 1e-3));
        assert!(!e.take_guard_fired());
    }

    #[test]
    fn queued_patterns_are_reported() {
        let mut e = demo_engine();
        e.play();
        e.select_pattern(3, 5);
        assert_eq!(e.queued_patterns(), [None, None, None, Some(5)]);
    }
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine engine`
Expected: compile errors.

- [ ] **Step 3: Add mixer meters**

In `mixer.rs`:
1. Add the field `meters: [f32; 5],` to `Mixer` and initialise it to `[0.0; 5]` in `new`.
2. Add this method:
   ```rust
       /// Peak levels since the last call: channels 0..4 post-fader, then
       /// the master after the final clip. Resets them.
       pub fn take_meters(&mut self) -> [f32; 5] {
           std::mem::take(&mut self.meters)
       }
   ```
3. In `process`, change the strip loop header to `for (c, (strip, buf)) in self.strips.iter_mut().zip(chans.iter_mut()).enumerate() {`.
4. Inside the strip loop's `for i in 0..n` body, after computing `sl` and `sr`, add:
   ```rust
                   self.meters[c] = self.meters[c].max(sl.abs()).max(sr.abs());
   ```
5. At the end of `process`, in the final clamp loop, track the master peak. Replace the loop with:
   ```rust
           for v in out_l.iter_mut().chain(out_r.iter_mut()) {
               *v = v.clamp(-1.0, 1.0);
               self.meters[4] = self.meters[4].max(v.abs());
           }
   ```

- [ ] **Step 4: Add the engine methods**

In `engine.rs`:
1. Add the fields `sample_rate: f32,` and `guard_fired: bool,` to `Engine`, initialised in `new` (`sample_rate`, `false`).
2. In `process_block`, where the mixer guard result is checked, set `self.guard_fired = true;` inside the `if`.
3. Add these methods and re-implement `set_*_pattern`:
   ```rust
       /// Builds an engine with a project loaded. Allocates; call off the
       /// audio thread.
       pub fn from_project(sample_rate: f32, p: &Project) -> Engine {
           let mut e = Engine::new(sample_rate);
           e.load_project(p);
           e
       }

       pub fn sample_rate(&self) -> f32 {
           self.sample_rate
       }

       /// Swaps `p` with the stored pattern (real-time safe). An invalid
       /// target or malformed pattern leaves both untouched and returns false.
       pub fn swap_synth_pattern(&mut self, synth: usize, slot: u8, p: &mut SynthPattern) -> bool {
           if synth >= 2 || slot as usize >= PATTERNS || !p.is_well_formed() {
               return false;
           }
           std::mem::swap(&mut self.synth_patterns[synth][slot as usize], p);
           true
       }

       pub fn swap_drum_pattern(&mut self, kit: usize, slot: u8, p: &mut DrumPattern) -> bool {
           if kit >= 2 || slot as usize >= PATTERNS || !p.is_well_formed() {
               return false;
           }
           std::mem::swap(&mut self.drum_patterns[kit][slot as usize], p);
           true
       }

       /// Stores `p` and returns the old pattern. An invalid target or a
       /// malformed pattern is not stored: `p` is handed back unchanged.
       pub fn set_synth_pattern(&mut self, synth: usize, slot: u8, mut p: SynthPattern) -> SynthPattern {
           self.swap_synth_pattern(synth, slot, &mut p);
           p
       }

       pub fn set_drum_pattern(&mut self, kit: usize, slot: u8, mut p: DrumPattern) -> DrumPattern {
           self.swap_drum_pattern(kit, slot, &mut p);
           p
       }

       pub fn queued_patterns(&self) -> [Option<u8>; 4] {
           std::array::from_fn(|m| self.seq.queued_pattern(m))
       }

       /// See `Mixer::take_meters`.
       pub fn take_meters(&mut self) -> [f32; 5] {
           self.mixer.take_meters()
       }

       /// True once if the mixer's NaN guard fired since the last call.
       pub fn take_guard_fired(&mut self) -> bool {
           std::mem::take(&mut self.guard_fired)
       }
   ```
   Delete the old `set_synth_pattern` and `set_drum_pattern` bodies.

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --workspace`, then `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): live-host API — from_project, pattern swap, meters, guard flag"
```

---

### Task 3: EngineHost, the real-time command/event bridge

**Files:**
- Modify: `Cargo.toml` (workspace deps), `crates/acid-engine/Cargo.toml`, `crates/acid-engine/src/lib.rs`
- Create: `crates/acid-engine/src/host.rs`

**Interfaces:**
- Consumes: the Task 2 engine API.
- Produces, in `acid_engine::host`:
  - **Constants:** `COMMAND_CAPACITY: usize = 4096` and `EVENT_CAPACITY: usize = 1024`.
  - **`Command` enum:**
    - `SetParam(ParamId, f32)`, `SetBpm(f32)`, `SetSwing(f32)`, `Play`, `Stop`
    - `SelectPattern { machine: u8, slot: u8 }`
    - `SynthPattern { synth: u8, slot: u8, pattern: Box<SynthPattern> }`
    - `DrumPattern { kit: u8, slot: u8, pattern: Box<DrumPattern> }`
    - `ReplaceEngine(Box<Engine>)`
  - **`Status` struct:** fields `playing: bool`, `playhead: [usize; 4]`, `patterns: [u8; 4]`, `queued: [Option<u8>; 4]`, `meters: [f32; 5]`, `cpu: f32` and `guard_fired: bool`. Derives `Clone, Copy, Debug, Default, PartialEq`.
  - **`Event` enum:** `Status(Status)`, `DisposeSynthPattern(Box<SynthPattern>)`, `DisposeDrumPattern(Box<DrumPattern>)` and `DisposeEngine(Box<Engine>)`.
  - **`EngineHost`:**
    - `process(&mut self, out_l: &mut [f32], out_r: &mut [f32])`
    - `set_cpu_load(&mut self, load: f32)`
    - `engine(&self) -> &Engine`
  - **`channel(engine: Engine) -> (EngineHost, rtrb::Producer<Command>, rtrb::Consumer<Event>)`**

- [ ] **Step 1: Add the dependency**

Root `Cargo.toml`, under `[workspace.dependencies]`, add:
```toml
rtrb = "0.3"
```
In `crates/acid-engine/Cargo.toml`, under `[dependencies]`, add:
```toml
rtrb.workspace = true
```
Add `pub mod host;` to `lib.rs`, keeping the module list alphabetical (it goes between `fx` and `mixer`).

- [ ] **Step 2: Write the failing tests**

`crates/acid-engine/src/host.rs`, tests first:
```rust
//! Real-time bridge between the UI thread and the audio thread. The UI
//! pushes `Command`s; the audio callback calls `EngineHost::process`,
//! which applies them, renders, and pushes back `Event`s: a ~60 Hz
//! `Status`, plus every replaced pattern or engine so that memory is
//! freed on the UI thread, never on the audio thread.

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;

    fn host_with_demo() -> (EngineHost, rtrb::Producer<Command>, rtrb::Consumer<Event>) {
        channel(Engine::from_project(SR, &crate::demo::demo_project()))
    }

    fn run(host: &mut EngineHost, samples: usize) {
        let mut l = vec![0.0; 512];
        let mut r = vec![0.0; 512];
        let mut done = 0;
        while done < samples {
            host.process(&mut l, &mut r);
            done += 512;
        }
    }

    #[test]
    fn commands_reach_the_engine() {
        let (mut host, mut tx, _rx) = host_with_demo();
        tx.push(Command::SetBpm(140.0)).unwrap();
        tx.push(Command::Play).unwrap();
        run(&mut host, 512);
        assert!(host.engine().is_playing());
        tx.push(Command::Stop).unwrap();
        run(&mut host, 512);
        assert!(!host.engine().is_playing());
    }

    #[test]
    fn replaced_pattern_comes_back_for_disposal() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::SynthPattern { synth: 0, slot: 0, pattern: Box::new(SynthPattern::default()) }).unwrap();
        run(&mut host, 512);
        let mut found = false;
        while let Ok(ev) = rx.pop() {
            if let Event::DisposeSynthPattern(old) = ev {
                assert!(old.steps.iter().any(|s| s.gate != crate::project::Gate::Off));
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn status_arrives_about_sixty_times_a_second() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::Play).unwrap();
        run(&mut host, SR as usize);
        let mut statuses = Vec::new();
        while let Ok(ev) = rx.pop() {
            if let Event::Status(s) = ev {
                statuses.push(s);
            }
        }
        assert!((40..=61).contains(&statuses.len()), "{}", statuses.len());
        let last = statuses.last().unwrap();
        assert!(last.playing);
        assert!(last.meters[4] > 0.0);
    }

    #[test]
    fn replace_engine_swaps_and_returns_the_old_one() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::Play).unwrap();
        run(&mut host, 512);
        tx.push(Command::ReplaceEngine(Box::new(Engine::new(SR)))).unwrap();
        run(&mut host, 512);
        assert!(!host.engine().is_playing());
        let disposed = std::iter::from_fn(|| rx.pop().ok()).any(|e| matches!(e, Event::DisposeEngine(_)));
        assert!(disposed);
    }

    #[test]
    fn full_event_ring_does_not_panic() {
        let (mut host, mut tx, _rx) = host_with_demo();
        for i in 0..2000 {
            tx.push(Command::DrumPattern { kit: 0, slot: (i % 128) as u8, pattern: Box::new(DrumPattern::default()) }).unwrap();
        }
        run(&mut host, 512);
    }
}
```

- [ ] **Step 3: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine host`
Expected: compile errors.

- [ ] **Step 4: Implement the host**

Insert above the tests in `host.rs`:
```rust
use crate::engine::Engine;
use crate::params::ParamId;
use crate::project::{DrumPattern, SynthPattern};
use rtrb::{Consumer, Producer, PushError, RingBuffer};

pub const COMMAND_CAPACITY: usize = 4096;
pub const EVENT_CAPACITY: usize = 1024;
const STATUS_RATE_HZ: f32 = 60.0;
/// Weight of the newest CPU-load sample in the smoothed value.
const CPU_SMOOTHING: f32 = 0.1;

pub enum Command {
    SetParam(ParamId, f32),
    SetBpm(f32),
    SetSwing(f32),
    Play,
    Stop,
    SelectPattern { machine: u8, slot: u8 },
    SynthPattern { synth: u8, slot: u8, pattern: Box<SynthPattern> },
    DrumPattern { kit: u8, slot: u8, pattern: Box<DrumPattern> },
    ReplaceEngine(Box<Engine>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    pub playing: bool,
    pub playhead: [usize; 4],
    pub patterns: [u8; 4],
    pub queued: [Option<u8>; 4],
    /// Peaks since the previous status: channels 0..4, then master.
    pub meters: [f32; 5],
    /// Smoothed fraction of the callback's time budget spent rendering.
    pub cpu: f32,
    pub guard_fired: bool,
}

pub enum Event {
    Status(Status),
    DisposeSynthPattern(Box<SynthPattern>),
    DisposeDrumPattern(Box<DrumPattern>),
    DisposeEngine(Box<Engine>),
}

pub struct EngineHost {
    engine: Box<Engine>,
    commands: Consumer<Command>,
    events: Producer<Event>,
    status_every: usize,
    since_status: usize,
    meters: [f32; 5],
    guard_fired: bool,
    cpu: f32,
}

/// Wires an engine to a fresh pair of rings. Call on the UI thread; move
/// the `EngineHost` into the audio callback.
pub fn channel(engine: Engine) -> (EngineHost, Producer<Command>, Consumer<Event>) {
    let (command_tx, command_rx) = RingBuffer::new(COMMAND_CAPACITY);
    let (event_tx, event_rx) = RingBuffer::new(EVENT_CAPACITY);
    let status_every = (engine.sample_rate() / STATUS_RATE_HZ).max(1.0) as usize;
    let host = EngineHost {
        engine: Box::new(engine),
        commands: command_rx,
        events: event_tx,
        status_every,
        since_status: 0,
        meters: [0.0; 5],
        guard_fired: false,
        cpu: 0.0,
    };
    (host, command_tx, event_rx)
}

impl EngineHost {
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Records how much of the callback's time budget the last render
    /// used (1.0 = all of it).
    pub fn set_cpu_load(&mut self, load: f32) {
        if load.is_finite() {
            self.cpu += (load - self.cpu) * CPU_SMOOTHING;
        }
    }

    /// Applies pending commands, renders, and reports. Real-time safe.
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        self.drain_commands();
        self.engine.process(out_l, out_r);
        for (acc, m) in self.meters.iter_mut().zip(self.engine.take_meters()) {
            *acc = acc.max(m);
        }
        self.guard_fired |= self.engine.take_guard_fired();
        self.since_status += out_l.len();
        if self.since_status >= self.status_every {
            self.since_status = 0;
            self.send_status();
        }
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                Command::SetParam(id, v) => self.engine.set_param(id, v),
                Command::SetBpm(bpm) => self.engine.set_bpm(bpm),
                Command::SetSwing(swing) => self.engine.set_swing(swing),
                Command::Play => self.engine.play(),
                Command::Stop => self.engine.stop(),
                Command::SelectPattern { machine, slot } => self.engine.select_pattern(machine as usize, slot),
                Command::SynthPattern { synth, slot, mut pattern } => {
                    self.engine.swap_synth_pattern(synth as usize, slot, &mut pattern);
                    self.dispose(Event::DisposeSynthPattern(pattern));
                }
                Command::DrumPattern { kit, slot, mut pattern } => {
                    self.engine.swap_drum_pattern(kit as usize, slot, &mut pattern);
                    self.dispose(Event::DisposeDrumPattern(pattern));
                }
                Command::ReplaceEngine(mut engine) => {
                    std::mem::swap(&mut self.engine, &mut engine);
                    self.dispose(Event::DisposeEngine(engine));
                }
            }
        }
    }

    /// Hands a box back to the UI thread. If the ring is full the box is
    /// leaked rather than freed here: freeing would break real-time safety.
    fn dispose(&mut self, event: Event) {
        if let Err(PushError::Full(event)) = self.events.push(event) {
            std::mem::forget(event);
        }
    }

    fn send_status(&mut self) {
        let status = Status {
            playing: self.engine.is_playing(),
            playhead: self.engine.playhead(),
            patterns: self.engine.current_patterns(),
            queued: self.engine.queued_patterns(),
            meters: self.meters,
            cpu: self.cpu,
            guard_fired: self.guard_fired,
        };
        if self.events.push(Event::Status(status)).is_ok() {
            self.meters = [0.0; 5];
            self.guard_fired = false;
        }
    }
}
```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --workspace`, plus `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/acid-engine
git commit -m "feat(engine): lock-free EngineHost command/event bridge"
```

---

### Task 4: Document, edits and undo

**Files:**
- Modify: `Cargo.toml`, `crates/app/Cargo.toml`, `crates/app/src/main.rs`
- Create: `crates/app/src/document.rs`

**Interfaces:**
- Consumes: `acid_engine::host::Command` and the `acid_engine::project` types.
- Produces, in `crate::document`:
  - `UNDO_LIMIT: usize = 200`.
  - `Edit` variants:
    - `Param { id, value }`
    - `SynthStep { synth, slot, step, value }`, `SynthLength { synth, slot, length }`, `SynthTranspose { synth, slot, transpose }`, `SynthPattern { synth, slot, pattern: Box<SynthPattern> }`
    - `DrumHit { kit, slot, step, inst, hit }`, `DrumLength { kit, slot, length }`, `DrumPattern { kit, slot, pattern: Box<DrumPattern> }`
    - `Bpm(f32)`, `Swing(f32)`, `View { synth, view }`
    - Machine and step indices are `usize`; slots are `u8`.
  - `Document` with pub fields `project: Project`, `path: Option<PathBuf>` and `dirty: bool`, plus these methods:
    - `new(project) -> Self`
    - `apply(&mut self, edit: Edit, merge: bool) -> Vec<Command>`
    - `end_gesture(&mut self)`
    - `undo(&mut self) -> Vec<Command>` and `redo(&mut self) -> Vec<Command>`
    - `can_undo(&self) -> bool` and `can_redo(&self) -> bool`
    - `select_pattern(&mut self, machine: usize, slot: u8) -> Vec<Command>`
- Merge rule: an edit applied with `merge = true` folds into the previous undo entry when the previous entry had the same merge key and was also applied with `merge = true`, with no `end_gesture` between them.

- [ ] **Step 1: Set up the app crate's dependencies**

Root `Cargo.toml`, under `[workspace.dependencies]`, add:
```toml
eframe = { version = "0.31", default-features = false, features = ["glow", "wayland", "x11", "default_fonts"] }
cpal = "0.15"
rfd = "0.15"
```
Replace the `[dependencies]` of `crates/app/Cargo.toml` with:
```toml
[dependencies]
acid-engine = { path = "../acid-engine" }
eframe.workspace = true
cpal.workspace = true
rfd.workspace = true
rtrb.workspace = true
```
At the top of `crates/app/src/main.rs`, below the doc comment, add `mod document;`.

- [ ] **Step 2: Write the failing tests**

`crates/app/src/document.rs`, tests first:
```rust
//! The editable project plus undo/redo. Every change is an `Edit`;
//! applying one returns the engine `Command`s that bring the audio
//! thread in line. Undo stores the inverse edit.

#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::params::{ParamId, SynthParam};
    use acid_engine::project::{DrumHit, Gate, SeqView, SynthStep};

    fn doc() -> Document {
        Document::new(Project::default())
    }

    const CUTOFF: ParamId = ParamId::Synth(0, SynthParam::Cutoff);

    #[test]
    fn param_edit_updates_project_and_emits_command() {
        let mut d = doc();
        let cmds = d.apply(Edit::Param { id: CUTOFF, value: 0.9 }, false);
        assert_eq!(d.project.param(CUTOFF), 0.9);
        assert!(matches!(cmds.as_slice(), [Command::SetParam(id, v)] if *id == CUTOFF && *v == 0.9));
        assert!(d.dirty);
    }

    #[test]
    fn undo_and_redo_round_trip() {
        let mut d = doc();
        let before = d.project.clone();
        let step = SynthStep { note: 3, octave: 1, gate: Gate::On, accent: true, slide: false };
        d.apply(Edit::SynthStep { synth: 1, slot: 5, step: 2, value: step }, false);
        d.apply(Edit::DrumHit { kit: 0, slot: 0, step: 4, inst: 1, hit: DrumHit::Accent }, false);
        let after = d.project.clone();
        d.undo();
        d.undo();
        assert_eq!(d.project, before);
        assert!(!d.can_undo());
        d.redo();
        d.redo();
        assert_eq!(d.project, after);
        assert!(!d.can_redo());
    }

    #[test]
    fn a_gesture_is_one_undo_step() {
        let mut d = doc();
        let start = d.project.param(CUTOFF);
        for v in [0.4, 0.5, 0.6, 0.7] {
            d.apply(Edit::Param { id: CUTOFF, value: v }, true);
        }
        d.end_gesture();
        d.apply(Edit::Param { id: CUTOFF, value: 0.1 }, true);
        d.end_gesture();
        d.undo();
        assert_eq!(d.project.param(CUTOFF), 0.7);
        d.undo();
        assert_eq!(d.project.param(CUTOFF), start);
    }

    #[test]
    fn different_params_never_merge() {
        let mut d = doc();
        d.apply(Edit::Param { id: CUTOFF, value: 0.9 }, true);
        d.apply(Edit::Param { id: ParamId::Synth(0, SynthParam::Resonance), value: 0.9 }, true);
        d.undo();
        assert_eq!(d.project.param(CUTOFF), 0.9);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut d = doc();
        d.apply(Edit::Bpm(140.0), false);
        d.undo();
        assert!(d.can_redo());
        d.apply(Edit::Bpm(150.0), false);
        assert!(!d.can_redo());
    }

    #[test]
    fn step_edits_send_the_whole_pattern() {
        let mut d = doc();
        let cmds = d.apply(Edit::SynthLength { synth: 0, slot: 3, length: 7 }, false);
        match cmds.as_slice() {
            [Command::SynthPattern { synth: 0, slot: 3, pattern }] => assert_eq!(pattern.length, 7),
            _ => panic!("expected one SynthPattern command"),
        }
        let cmds = d.apply(Edit::DrumLength { kit: 1, slot: 2, length: 12 }, false);
        assert!(matches!(cmds.as_slice(), [Command::DrumPattern { kit: 1, slot: 2, .. }]));
    }

    #[test]
    fn values_are_clamped() {
        let mut d = doc();
        d.apply(Edit::Bpm(1000.0), false);
        assert_eq!(d.project.bpm, 300.0);
        d.apply(Edit::Swing(-1.0), false);
        assert_eq!(d.project.swing, 0.0);
        d.apply(Edit::SynthLength { synth: 0, slot: 0, length: 0 }, false);
        assert_eq!(d.project.synths[0].patterns[0].length, 1);
        d.apply(Edit::Param { id: CUTOFF, value: 2.0 }, false);
        assert_eq!(d.project.param(CUTOFF), 1.0);
    }

    #[test]
    fn view_edit_is_undoable_and_silent() {
        let mut d = doc();
        let cmds = d.apply(Edit::View { synth: 1, view: SeqView::PianoRoll }, false);
        assert!(cmds.is_empty());
        assert_eq!(d.project.synths[1].view, SeqView::PianoRoll);
        d.undo();
        assert_eq!(d.project.synths[1].view, SeqView::Step303);
    }

    #[test]
    fn undo_history_is_bounded() {
        let mut d = doc();
        for i in 0..(UNDO_LIMIT + 50) {
            d.apply(Edit::Bpm(60.0 + i as f32 * 0.1), false);
        }
        let mut n = 0;
        while d.can_undo() {
            d.undo();
            n += 1;
        }
        assert_eq!(n, UNDO_LIMIT);
    }

    #[test]
    fn select_pattern_marks_dirty_and_commands_engine() {
        let mut d = doc();
        let cmds = d.select_pattern(2, 17);
        assert_eq!(d.project.selected[2], 17);
        assert!(matches!(cmds.as_slice(), [Command::SelectPattern { machine: 2, slot: 17 }]));
        assert!(d.dirty);
    }
}
```

- [ ] **Step 3: Run the tests to confirm they fail**

Run: `cargo test -p sidacid`
Expected: compile errors. The first build of the app crate downloads and compiles eframe, which takes a few minutes.

- [ ] **Step 4: Implement the document**

Insert above the tests in `document.rs`:
```rust
use acid_engine::host::Command;
use acid_engine::params::ParamId;
use acid_engine::project::{DrumHit, DrumPattern, Project, STEPS, SeqView, SynthPattern, SynthStep};
use std::path::PathBuf;

pub const UNDO_LIMIT: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Param { id: ParamId, value: f32 },
    SynthStep { synth: usize, slot: u8, step: usize, value: SynthStep },
    SynthLength { synth: usize, slot: u8, length: u8 },
    SynthTranspose { synth: usize, slot: u8, transpose: i8 },
    SynthPattern { synth: usize, slot: u8, pattern: Box<SynthPattern> },
    DrumHit { kit: usize, slot: u8, step: usize, inst: usize, hit: DrumHit },
    DrumLength { kit: usize, slot: u8, length: u8 },
    DrumPattern { kit: usize, slot: u8, pattern: Box<DrumPattern> },
    Bpm(f32),
    Swing(f32),
    View { synth: usize, view: SeqView },
}

/// Which edits may fold into one undo step during a gesture.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MergeKey {
    Param(ParamId),
    Synth(usize, u8),
    Drum(usize, u8),
    Bpm,
    Swing,
    View(usize),
}

fn merge_key(edit: &Edit) -> MergeKey {
    match edit {
        Edit::Param { id, .. } => MergeKey::Param(*id),
        Edit::SynthStep { synth, slot, .. }
        | Edit::SynthLength { synth, slot, .. }
        | Edit::SynthTranspose { synth, slot, .. }
        | Edit::SynthPattern { synth, slot, .. } => MergeKey::Synth(*synth, *slot),
        Edit::DrumHit { kit, slot, .. } | Edit::DrumLength { kit, slot, .. } | Edit::DrumPattern { kit, slot, .. } => {
            MergeKey::Drum(*kit, *slot)
        }
        Edit::Bpm(_) => MergeKey::Bpm,
        Edit::Swing(_) => MergeKey::Swing,
        Edit::View { synth, .. } => MergeKey::View(*synth),
    }
}

pub struct Document {
    pub project: Project,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Merge key of the open gesture, if the last edit was a merging one.
    gesture: Option<MergeKey>,
}

impl Document {
    pub fn new(project: Project) -> Self {
        Self { project, path: None, dirty: false, undo: Vec::new(), redo: Vec::new(), gesture: None }
    }

    /// Applies `edit` and returns the engine commands to send. With
    /// `merge`, consecutive edits to the same target form one undo step
    /// until `end_gesture`.
    pub fn apply(&mut self, edit: Edit, merge: bool) -> Vec<Command> {
        let key = merge_key(&edit);
        let continuing = merge && self.gesture == Some(key) && !self.undo.is_empty();
        let inverse = self.apply_raw(edit);
        let cmds = self.commands_for(&inverse);
        if !continuing {
            self.undo.push(inverse);
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.gesture = merge.then_some(key);
        self.redo.clear();
        self.dirty = true;
        cmds
    }

    pub fn end_gesture(&mut self) {
        self.gesture = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> Vec<Command> {
        self.gesture = None;
        let Some(edit) = self.undo.pop() else { return Vec::new() };
        let inverse = self.apply_raw(edit);
        let cmds = self.commands_for(&inverse);
        self.redo.push(inverse);
        self.dirty = true;
        cmds
    }

    pub fn redo(&mut self) -> Vec<Command> {
        self.gesture = None;
        let Some(edit) = self.redo.pop() else { return Vec::new() };
        let inverse = self.apply_raw(edit);
        let cmds = self.commands_for(&inverse);
        self.undo.push(inverse);
        self.dirty = true;
        cmds
    }

    /// Selecting a pattern is a transport action: saved, not undoable.
    pub fn select_pattern(&mut self, machine: usize, slot: u8) -> Vec<Command> {
        self.project.selected[machine] = slot;
        self.dirty = true;
        vec![Command::SelectPattern { machine: machine as u8, slot }]
    }

    /// Applies an edit and returns its inverse.
    fn apply_raw(&mut self, edit: Edit) -> Edit {
        let p = &mut self.project;
        match edit {
            Edit::Param { id, value } => {
                let old = p.param(id);
                p.params.insert(id, value.clamp(0.0, 1.0));
                Edit::Param { id, value: old }
            }
            Edit::SynthStep { synth, slot, step, value } => {
                let old = std::mem::replace(&mut p.synths[synth].patterns[slot as usize].steps[step], value);
                Edit::SynthStep { synth, slot, step, value: old }
            }
            Edit::SynthLength { synth, slot, length } => {
                let pat = &mut p.synths[synth].patterns[slot as usize];
                let old = std::mem::replace(&mut pat.length, length.clamp(1, STEPS as u8));
                Edit::SynthLength { synth, slot, length: old }
            }
            Edit::SynthTranspose { synth, slot, transpose } => {
                let pat = &mut p.synths[synth].patterns[slot as usize];
                let old = std::mem::replace(&mut pat.transpose, transpose.clamp(-12, 12));
                Edit::SynthTranspose { synth, slot, transpose: old }
            }
            Edit::SynthPattern { synth, slot, pattern } => {
                let old = std::mem::replace(&mut p.synths[synth].patterns[slot as usize], *pattern);
                Edit::SynthPattern { synth, slot, pattern: Box::new(old) }
            }
            Edit::DrumHit { kit, slot, step, inst, hit } => {
                let old = std::mem::replace(&mut p.drums[kit].patterns[slot as usize].steps[step][inst], hit);
                Edit::DrumHit { kit, slot, step, inst, hit: old }
            }
            Edit::DrumLength { kit, slot, length } => {
                let pat = &mut p.drums[kit].patterns[slot as usize];
                let old = std::mem::replace(&mut pat.length, length.clamp(1, STEPS as u8));
                Edit::DrumLength { kit, slot, length: old }
            }
            Edit::DrumPattern { kit, slot, pattern } => {
                let old = std::mem::replace(&mut p.drums[kit].patterns[slot as usize], *pattern);
                Edit::DrumPattern { kit, slot, pattern: Box::new(old) }
            }
            Edit::Bpm(bpm) => Edit::Bpm(std::mem::replace(&mut p.bpm, bpm.clamp(40.0, 300.0))),
            Edit::Swing(swing) => Edit::Swing(std::mem::replace(&mut p.swing, swing.clamp(0.0, 0.75))),
            Edit::View { synth, view } => {
                Edit::View { synth, view: std::mem::replace(&mut p.synths[synth].view, view) }
            }
        }
    }

    /// Commands that sync the engine with the project at `edit`'s target.
    fn commands_for(&self, edit: &Edit) -> Vec<Command> {
        let p = &self.project;
        match edit {
            Edit::Param { id, .. } => vec![Command::SetParam(*id, p.param(*id))],
            Edit::SynthStep { synth, slot, .. }
            | Edit::SynthLength { synth, slot, .. }
            | Edit::SynthTranspose { synth, slot, .. }
            | Edit::SynthPattern { synth, slot, .. } => vec![Command::SynthPattern {
                synth: *synth as u8,
                slot: *slot,
                pattern: Box::new(p.synths[*synth].patterns[*slot as usize].clone()),
            }],
            Edit::DrumHit { kit, slot, .. } | Edit::DrumLength { kit, slot, .. } | Edit::DrumPattern { kit, slot, .. } => {
                vec![Command::DrumPattern {
                    kit: *kit as u8,
                    slot: *slot,
                    pattern: Box::new(p.drums[*kit].patterns[*slot as usize].clone()),
                }]
            }
            Edit::Bpm(_) => vec![Command::SetBpm(p.bpm)],
            Edit::Swing(_) => vec![Command::SetSwing(p.swing)],
            Edit::View { .. } => Vec::new(),
        }
    }
}
```

Until Task 6 uses `Document`, the compiler warns about unused items. Put `#![allow(dead_code)] // wired up by the UI in Task 6` at the top of `document.rs`, below the doc comment. Task 6 removes it.

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/app
git commit -m "feat(app): document model with edits, gestures and bounded undo/redo"
```

---

### Task 5: Live audio output

**Files:**
- Create: `crates/app/src/audio.rs`
- Modify: `crates/app/src/main.rs` (add `mod audio;`)

**Interfaces:**
- Consumes: `acid_engine::host::{channel, Command, Event, EngineHost}`, `Engine::from_project`.
- Produces, in `crate::audio`:
  - `AudioLink`, with pub fields `sample_rate: f32` and `device_name: String`, and these methods:
    - `start(project: &Project) -> Result<AudioLink, String>`
    - `send(&mut self, cmd: Command) -> Result<(), Command>`
    - `poll(&mut self) -> Option<Event>`
    - `has_failed(&self) -> bool`
    - `load_project(&mut self, project: &Project) -> Result<(), Command>`
  - `RENDER_CHUNK: usize = 4096`
  - `fill_interleaved<T: cpal::Sample + cpal::FromSample<f32>>(render: &mut impl FnMut(&mut [f32], &mut [f32]), l: &mut [f32], r: &mut [f32], data: &mut [T], channels: usize)`

- [ ] **Step 1: Write the failing tests**

`crates/app/src/audio.rs`, tests first:
```rust
//! Live output through cpal. The engine runs inside the device callback
//! via `EngineHost`; the UI talks to it only through the command and
//! event rings held by `AudioLink`.

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(l: &mut [f32], r: &mut [f32]) {
        l.fill(0.5);
        r.fill(-0.25);
    }

    #[test]
    fn stereo_interleaves_across_chunks() {
        let mut l = vec![0.0; RENDER_CHUNK];
        let mut r = vec![0.0; RENDER_CHUNK];
        let mut data = vec![0.0f32; 2 * (RENDER_CHUNK * 2 + 17)];
        fill_interleaved(&mut constant, &mut l, &mut r, &mut data, 2);
        for frame in data.chunks(2) {
            assert_eq!(frame, &[0.5, -0.25]);
        }
    }

    #[test]
    fn mono_device_gets_the_average() {
        let mut l = vec![0.0; 64];
        let mut r = vec![0.0; 64];
        let mut data = vec![0.0f32; 100];
        fill_interleaved(&mut constant, &mut l, &mut r, &mut data, 1);
        assert!(data.iter().all(|v| (*v - 0.125).abs() < 1e-6));
    }

    #[test]
    fn extra_channels_are_silent() {
        let mut l = vec![0.0; 64];
        let mut r = vec![0.0; 64];
        let mut data = vec![1.0f32; 4 * 10];
        fill_interleaved(&mut constant, &mut l, &mut r, &mut data, 4);
        for frame in data.chunks(4) {
            assert_eq!(frame, &[0.5, -0.25, 0.0, 0.0]);
        }
    }

    #[test]
    fn integer_formats_convert() {
        let mut l = vec![0.0; 64];
        let mut r = vec![0.0; 64];
        let mut data = vec![0i16; 2 * 8];
        fill_interleaved(&mut constant, &mut l, &mut r, &mut data, 2);
        assert!(data[0] > 16_000 && data[1] < -8_000);
    }
}
```
Add `mod audio;` to `main.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p sidacid audio`
Expected: compile errors.

- [ ] **Step 3: Implement the audio link**

Insert above the tests in `audio.rs`:
```rust
use acid_engine::engine::Engine;
use acid_engine::host::{Command, Event, EngineHost, channel};
use acid_engine::project::Project;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use rtrb::{Consumer, Producer, PushError};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Frames rendered per engine call inside the device callback.
pub const RENDER_CHUNK: usize = 4096;

pub struct AudioLink {
    _stream: cpal::Stream,
    commands: Producer<Command>,
    events: Consumer<Event>,
    failed: Arc<AtomicBool>,
    pub sample_rate: f32,
    pub device_name: String,
}

impl AudioLink {
    /// Opens the default output device and starts the engine on it with
    /// `project` loaded.
    pub fn start(project: &Project) -> Result<AudioLink, String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no audio output device found")?;
        let device_name = device.name().unwrap_or_else(|_| "audio device".into());
        let supported = device.default_output_config().map_err(|e| format!("audio device: {e}"))?;
        let sample_rate = supported.sample_rate().0 as f32;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let (engine_host, commands, events) = channel(Engine::from_project(sample_rate, project));
        let failed = Arc::new(AtomicBool::new(false));
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, engine_host, failed.clone()),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, engine_host, failed.clone()),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, engine_host, failed.clone()),
            other => Err(format!("unsupported sample format {other:?}")),
        }?;
        stream.play().map_err(|e| format!("could not start audio: {e}"))?;
        Ok(AudioLink { _stream: stream, commands, events, failed, sample_rate, device_name })
    }

    /// Queues a command; hands it back if the ring is full.
    pub fn send(&mut self, cmd: Command) -> Result<(), Command> {
        self.commands.push(cmd).map_err(|PushError::Full(c)| c)
    }

    pub fn poll(&mut self) -> Option<Event> {
        self.events.pop().ok()
    }

    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Builds a fresh engine for `project` here on the UI thread and
    /// swaps it in whole on the audio thread.
    pub fn load_project(&mut self, project: &Project) -> Result<(), Command> {
        self.send(Command::ReplaceEngine(Box::new(Engine::from_project(self.sample_rate, project))))
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut host: EngineHost,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate.0 as f32;
    let mut l = vec![0.0f32; RENDER_CHUNK];
    let mut r = vec![0.0f32; RENDER_CHUNK];
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let started = Instant::now();
                let frames = data.len() / channels.max(1);
                let mut render = |a: &mut [f32], b: &mut [f32]| host.process(a, b);
                fill_interleaved(&mut render, &mut l, &mut r, data, channels);
                if frames > 0 {
                    let budget = frames as f32 / sample_rate;
                    host.set_cpu_load(started.elapsed().as_secs_f32() / budget);
                }
            },
            move |err| {
                eprintln!("audio stream error: {err}");
                failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| format!("could not open audio stream: {e}"))
}

/// Renders into `l`/`r` in chunks and writes interleaved device frames:
/// left/right on channels 0/1, their average on a mono device, silence on
/// any further channels.
pub fn fill_interleaved<T>(
    render: &mut impl FnMut(&mut [f32], &mut [f32]),
    l: &mut [f32],
    r: &mut [f32],
    data: &mut [T],
    channels: usize,
) where
    T: Sample + FromSample<f32>,
{
    let channels = channels.max(1);
    let frames = data.len() / channels;
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(l.len()).min(r.len());
        let (bl, br) = (&mut l[..n], &mut r[..n]);
        render(bl, br);
        for i in 0..n {
            let frame = &mut data[(done + i) * channels..(done + i + 1) * channels];
            if channels == 1 {
                frame[0] = T::from_sample(0.5 * (bl[i] + br[i]));
            } else {
                frame[0] = T::from_sample(bl[i]);
                frame[1] = T::from_sample(br[i]);
                for s in &mut frame[2..] {
                    *s = T::EQUILIBRIUM;
                }
            }
        }
        done += n;
    }
}
```
The `move |data, _|` closure borrows `host` mutably inside `render`, and `render` is created and dropped inside each callback, so the borrow ends before `host.set_cpu_load`. If the borrow checker still objects, inline `fill_interleaved(&mut |a, b| host.process(a, b), …)` in a block.

Until Task 6 wires it up, add `#![allow(dead_code)] // wired up by the UI in Task 6` below the doc comment.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean. Tests never open a device.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "feat(app): cpal audio link running the engine host"
```

---

### Task 6: App window, transport bar and knob widget

**Files:**
- Modify: `crates/app/src/main.rs`, `crates/app/src/document.rs` and `crates/app/src/audio.rs` (remove the temporary `dead_code` allows)
- Create: `crates/app/src/ui/mod.rs`, `ui/theme.rs`, `ui/knob.rs`, `ui/params_ui.rs`, `ui/transport.rs`

**Interfaces:**
- Consumes: `Document`, `Edit`, `AudioLink`, and the `host` types.
- Produces:
  - `ui::SidAcidApp` with these members:
    - pub(crate) fields `doc`, `audio`, `audio_error`, `status`, `pending: Vec<Command>` and `view: ViewState`
    - `new(cc)` and `headless(doc)`
    - `ui(&mut self, ctx)`
    - pub(crate) `send(&mut self, cmds: Vec<Command>)` and `start_audio(&mut self)`
    - private `rack(&mut self, ui)`, which later tasks fill in
  - `ViewState`, which derives `Default`, with fields `focus: usize` and `message: Option<String>`. Later tasks add fields.
  - `knob::{knob, switch, drag_to_value, scroll_to_value, KnobOutput, KNOB_SIZE}`.
  - `params_ui::discrete_names` and `SidAcidApp::param_knob(&mut self, ui, id, label)`.
  - `#[cfg(test)] ui::run_frames(app, n)`.

- [ ] **Step 1: Write the failing tests**

`crates/app/src/ui/knob.rs`, tests first:
```rust
//! Custom-painted rotary knob and click-to-cycle switch.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_up_raises_the_value() {
        assert!(drag_to_value(0.5, -10.0, false) > 0.5);
        assert!(drag_to_value(0.5, 10.0, false) < 0.5);
    }

    #[test]
    fn fine_drag_is_slower() {
        let coarse = drag_to_value(0.5, -10.0, false) - 0.5;
        let fine = drag_to_value(0.5, -10.0, true) - 0.5;
        assert!(fine > 0.0 && fine < coarse / 3.0);
    }

    #[test]
    fn values_stay_in_range() {
        assert_eq!(drag_to_value(0.99, -1000.0, false), 1.0);
        assert_eq!(drag_to_value(0.01, 1000.0, false), 0.0);
        assert_eq!(scroll_to_value(1.0, 5.0), 1.0);
        assert_eq!(scroll_to_value(0.0, -5.0), 0.0);
        assert!(scroll_to_value(0.5, 1.0) > 0.5);
        assert_eq!(scroll_to_value(0.5, 0.0), 0.5);
    }
}
```

`crates/app/src/ui/params_ui.rs`, tests first:
```rust
//! Binding knobs and switches to project parameters.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_discrete_param_has_a_name_per_step() {
        for id in ParamId::all() {
            let steps = id.info().steps;
            if steps >= 2 {
                assert_eq!(discrete_names(id).len(), steps as usize, "{id:?}");
            }
        }
    }
}
```

`crates/app/src/ui/mod.rs`, tests at the bottom:
```rust
#[cfg(test)]
pub(crate) fn run_frames(app: &mut SidAcidApp, frames: usize) {
    let ctx = egui::Context::default();
    for _ in 0..frames {
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_draws_headless() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        run_frames(&mut app, 3);
        assert!(app.audio.is_none());
    }

    #[test]
    fn commands_without_audio_are_dropped_not_queued_forever() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.send(vec![Command::Play, Command::Stop]);
        run_frames(&mut app, 1);
        assert!(app.pending.is_empty());
    }

    #[test]
    fn repeated_param_commands_coalesce() {
        use acid_engine::params::{ParamId, SynthParam};
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        app.send(vec![Command::SetParam(id, 0.1), Command::SetParam(id, 0.2), Command::SetParam(id, 0.3)]);
        assert_eq!(app.pending.len(), 1);
        assert!(matches!(app.pending[0], Command::SetParam(_, v) if v == 0.3));
    }
}
```

- [ ] **Step 2: Implement the theme and knob**

`ui/theme.rs`:
```rust
//! Colours and global visuals.

use eframe::egui::{self, Color32};

pub const ACID: Color32 = Color32::from_rgb(0xB8, 0xF5, 0x3A);
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x50, 0x3C);
pub const SLIDE: Color32 = Color32::from_rgb(0x4C, 0xB8, 0xFF);
pub const QUEUED: Color32 = Color32::from_rgb(0xFF, 0xC8, 0x3C);
pub const KNOB_BODY: Color32 = Color32::from_rgb(0x3A, 0x3E, 0x44);
pub const KNOB_TRACK: Color32 = Color32::from_rgb(0x55, 0x5A, 0x60);
pub const GRID_BG: Color32 = Color32::from_rgb(0x1A, 0x1C, 0x20);
pub const GRID_DARK: Color32 = Color32::from_rgb(0x14, 0x15, 0x18);
pub const GRID_LINE: Color32 = Color32::from_rgb(0x30, 0x33, 0x38);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.selection.bg_fill = ACID.gamma_multiply(0.45);
    visuals.hyperlink_color = ACID;
    ctx.set_visuals(visuals);
}
```

Insert above the tests in `ui/knob.rs`:
```rust
use super::theme;
use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, Vec2, pos2, vec2};
use std::f32::consts::PI;

pub const KNOB_SIZE: f32 = 36.0;
const CELL: Vec2 = vec2(KNOB_SIZE + 10.0, KNOB_SIZE + 16.0);
const COARSE_PER_PIXEL: f32 = 0.006;
const FINE_PER_PIXEL: f32 = 0.001;
const SCROLL_STEP: f32 = 0.02;
const ANGLE_MIN: f32 = 0.75 * PI;
const ANGLE_MAX: f32 = 2.25 * PI;

pub struct KnobOutput {
    /// The new value, if the user changed it this frame.
    pub value: Option<f32>,
    /// The drag ended this frame (closes an undo gesture).
    pub released: bool,
}

pub fn drag_to_value(value: f32, dy: f32, fine: bool) -> f32 {
    let per_pixel = if fine { FINE_PER_PIXEL } else { COARSE_PER_PIXEL };
    (value - dy * per_pixel).clamp(0.0, 1.0)
}

pub fn scroll_to_value(value: f32, scroll: f32) -> f32 {
    let step = if scroll > 0.0 {
        SCROLL_STEP
    } else if scroll < 0.0 {
        -SCROLL_STEP
    } else {
        0.0
    };
    (value + step).clamp(0.0, 1.0)
}

/// A rotary knob: drag vertically (Shift = fine), scroll, double-click to
/// reset to `default`.
pub fn knob(ui: &mut egui::Ui, label: &str, value: f32, default: f32, tooltip: &str) -> KnobOutput {
    let (rect, response) = ui.allocate_exact_size(CELL, Sense::click_and_drag());
    let mut new_value = None;
    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let v = drag_to_value(value, response.drag_delta().y, fine);
        if v != value {
            new_value = Some(v);
        }
    }
    if response.double_clicked() {
        new_value = Some(default);
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            new_value = Some(scroll_to_value(value, scroll));
        }
    }

    let shown = new_value.unwrap_or(value);
    let painter = ui.painter();
    let center = pos2(rect.center().x, rect.top() + KNOB_SIZE / 2.0 + 2.0);
    let radius = KNOB_SIZE / 2.0 - 3.0;
    let arc = |from: f32, to: f32| -> Vec<egui::Pos2> {
        (0..=24)
            .map(|i| {
                let a = from + (to - from) * i as f32 / 24.0;
                center + vec2(a.cos(), a.sin()) * (radius + 3.0)
            })
            .collect()
    };
    painter.add(egui::Shape::line(arc(ANGLE_MIN, ANGLE_MAX), Stroke::new(2.0, theme::KNOB_TRACK)));
    let angle = ANGLE_MIN + (ANGLE_MAX - ANGLE_MIN) * shown;
    painter.add(egui::Shape::line(arc(ANGLE_MIN, angle), Stroke::new(2.5, theme::ACID)));
    painter.circle_filled(center, radius, theme::KNOB_BODY);
    painter.line_segment(
        [center, center + vec2(angle.cos(), angle.sin()) * (radius - 2.0)],
        Stroke::new(2.0, Color32::WHITE),
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 6.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.0),
        ui.visuals().text_color(),
    );
    let response = response.on_hover_text(tooltip);
    KnobOutput { value: new_value, released: response.drag_stopped() }
}

/// A button that cycles through `names` (left click forward, right click
/// back). Returns the new index when it changes.
pub fn switch(ui: &mut egui::Ui, label: &str, index: usize, names: &[&str]) -> Option<usize> {
    let n = names.len().max(1);
    let index = index.min(n - 1);
    let text = egui::RichText::new(format!("{label}\n{}", names[index])).size(10.0);
    let on = index > 0;
    let response = ui.add_sized(CELL, egui::Button::new(text).selected(on));
    if response.clicked() {
        Some((index + 1) % n)
    } else if response.secondary_clicked() {
        Some((index + n - 1) % n)
    } else {
        None
    }
}
```

- [ ] **Step 3: Implement the param helper, transport and app shell**

Insert above the tests in `ui/params_ui.rs`:
```rust
use super::{SidAcidApp, knob};
use crate::document::Edit;
use acid_engine::params::{FxParam, ParamId, SynthParam, step_index};
use eframe::egui;

/// Display names for each position of a discrete parameter.
pub fn discrete_names(id: ParamId) -> &'static [&'static str] {
    match id {
        ParamId::Synth(_, SynthParam::Wave) => &["Saw", "Pulse"],
        ParamId::Synth(_, SynthParam::FilterMode) => &["LP", "BP", "HP"],
        ParamId::Fx(FxParam::DelayDivision) => &["1/16", "1/8", "3/16", "1/4", "3/8", "1/2"],
        _ => &["Off", "On"],
    }
}

impl SidAcidApp {
    /// A knob (continuous) or switch (discrete) bound to `id`.
    pub(crate) fn param_knob(&mut self, ui: &mut egui::Ui, id: ParamId, label: &str) {
        let info = id.info();
        let value = self.doc.project.param(id);
        if info.steps >= 2 {
            let names = discrete_names(id);
            if let Some(i) = knob::switch(ui, label, step_index(value, info.steps), names) {
                let v = i as f32 / (info.steps - 1) as f32;
                let cmds = self.doc.apply(Edit::Param { id, value: v }, false);
                self.send(cmds);
            }
            return;
        }
        let tip = format!("{} {:.0}%  (drag, Shift = fine, double-click = reset)", info.name, value * 100.0);
        let out = knob::knob(ui, label, value, info.default, &tip);
        if let Some(v) = out.value {
            let cmds = self.doc.apply(Edit::Param { id, value: v }, true);
            self.send(cmds);
        }
        if out.released {
            self.doc.end_gesture();
        }
    }
}
```

`ui/transport.rs`:
```rust
//! The transport bar: play/stop, tempo, swing, lo-fi, master, CPU.

use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::params::{MasterParam, ParamId};
use eframe::egui;

impl SidAcidApp {
    pub(crate) fn transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(egui::RichText::new("SidAcid").color(theme::ACID).strong());
            ui.separator();
            let playing = self.status.playing;
            if ui.add(egui::Button::new("Play").selected(playing)).clicked() {
                self.send(vec![Command::Play]);
            }
            if ui.button("Stop").clicked() {
                self.send(vec![Command::Stop]);
            }
            ui.separator();
            let mut bpm = self.doc.project.bpm;
            let r = ui.add(egui::DragValue::new(&mut bpm).range(40.0..=300.0).speed(0.2).max_decimals(1).suffix(" BPM"));
            if r.changed() {
                let cmds = self.doc.apply(Edit::Bpm(bpm), true);
                self.send(cmds);
            }
            if r.drag_stopped() || r.lost_focus() {
                self.doc.end_gesture();
            }
            let mut swing = self.doc.project.swing * 100.0;
            let r = ui.add(egui::Slider::new(&mut swing, 0.0..=75.0).text("Swing %").max_decimals(0));
            if r.changed() {
                let cmds = self.doc.apply(Edit::Swing(swing / 100.0), true);
                self.send(cmds);
            }
            if r.drag_stopped() {
                self.doc.end_gesture();
            }
            ui.separator();
            self.param_knob(ui, ParamId::Master(MasterParam::LoFi), "Lo-Fi");
            self.param_knob(ui, ParamId::Master(MasterParam::Level), "Master");
            ui.separator();
            let cpu = self.status.cpu * 100.0;
            let colour = if cpu > 80.0 { theme::ACCENT } else { ui.visuals().text_color() };
            ui.label(egui::RichText::new(format!("CPU {cpu:.0}%")).color(colour));
        });
    }
}
```

`ui/mod.rs`, above the test helpers:
```rust
//! The SidAcid window: transport bar on top, a scrolling rack of machine
//! panels below. The UI owns the project (`Document`); every change goes
//! to the audio thread as `Command`s through `AudioLink`.

pub mod knob;
mod params_ui;
pub mod theme;
mod transport;

use crate::audio::AudioLink;
use crate::document::Document;
use acid_engine::host::{Command, Event, Status};
use eframe::egui;
use std::time::Duration;

#[derive(Default)]
pub struct ViewState {
    /// Machine that keyboard shortcuts act on (0..4).
    pub focus: usize,
    pub message: Option<String>,
}

pub struct SidAcidApp {
    pub(crate) doc: Document,
    pub(crate) audio: Option<AudioLink>,
    pub(crate) audio_error: Option<String>,
    pub(crate) status: Status,
    pub(crate) pending: Vec<Command>,
    pub(crate) view: ViewState,
}

impl SidAcidApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self::headless(Document::new(acid_engine::demo::demo_project()));
        app.start_audio();
        app
    }

    /// An app with no audio device (tests, or before audio starts).
    pub fn headless(doc: Document) -> Self {
        Self { doc, audio: None, audio_error: None, status: Status::default(), pending: Vec::new(), view: ViewState::default() }
    }

    pub(crate) fn start_audio(&mut self) {
        self.pending.clear();
        match AudioLink::start(&self.doc.project) {
            Ok(link) => {
                self.audio = Some(link);
                self.audio_error = None;
            }
            Err(e) => {
                self.audio = None;
                self.audio_error = Some(e);
            }
        }
    }

    /// Queues commands for the audio thread. Repeated values for the same
    /// parameter collapse into the latest one.
    pub(crate) fn send(&mut self, cmds: Vec<Command>) {
        for cmd in cmds {
            if let &Command::SetParam(id, v) = &cmd
                && let Some(Command::SetParam(_, pending)) =
                    self.pending.iter_mut().find(|c| matches!(c, Command::SetParam(p, _) if *p == id))
            {
                *pending = v;
                continue;
            }
            self.pending.push(cmd);
        }
    }

    /// Sends queued commands and reads events. Without audio, commands are
    /// dropped: the engine is rebuilt from the project when audio starts.
    fn pump(&mut self) {
        let Some(audio) = self.audio.as_mut() else {
            self.pending.clear();
            return;
        };
        let mut unsent = Vec::new();
        for cmd in self.pending.drain(..) {
            if !unsent.is_empty() {
                unsent.push(cmd);
            } else if let Err(cmd) = audio.send(cmd) {
                unsent.push(cmd);
            }
        }
        self.pending = unsent;
        while let Some(event) = audio.poll() {
            if let Event::Status(status) = event {
                self.status = status;
            }
            // Dispose events drop here, on the UI thread.
        }
        if audio.has_failed() {
            self.audio = None;
            self.status = Status::default();
            self.audio_error = Some("The audio device stopped.".into());
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        self.pump();
        egui::TopBottomPanel::top("transport").show(ctx, |ui| self.transport(ui));
        if let Some(err) = self.audio_error.clone() {
            egui::TopBottomPanel::top("audio_banner").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(theme::ACCENT, format!("No audio: {err}"));
                    if ui.button("Retry audio").clicked() {
                        self.start_audio();
                    }
                });
            });
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.rack(ui));
        });
        if self.status.playing {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn rack(&mut self, ui: &mut egui::Ui) {
        ui.label("Machines appear here.");
    }
}

impl eframe::App for SidAcidApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}
```

Replace `crates/app/src/main.rs` with:
```rust
//! SidAcid: a SID-powered acid groovebox. Run without arguments for the
//! app; `sidacid render-demo [out.wav]` renders the demo groove offline.

mod audio;
mod document;
mod ui;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        None => run_app(),
        Some("render-demo") => render_demo(args.get(2).map(String::as_str).unwrap_or("demo.wav")),
        Some(_) => {
            eprintln!("usage: sidacid            (open the app)\n       sidacid render-demo [out.wav]");
            ExitCode::from(2)
        }
    }
}

fn run_app() -> ExitCode {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("SidAcid")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    match eframe::run_native("SidAcid", options, Box::new(|cc| Ok(Box::new(ui::SidAcidApp::new(cc))))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("could not open the SidAcid window: {e}");
            ExitCode::FAILURE
        }
    }
}

fn render_demo(path: &str) -> ExitCode {
    let project = acid_engine::demo::demo_project();
    match acid_engine::render::render_patterns_wav(&project, 8, Path::new(path)) {
        Ok(()) => {
            println!("wrote {path}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("render failed: {e}");
            ExitCode::FAILURE
        }
    }
}
```
Remove the temporary `#![allow(dead_code)]` lines from `document.rs` and `audio.rs`. If an item is still unused until a later task (for example `Document::can_undo`, `AudioLink::load_project` or `device_name`), put `#[allow(dead_code)] // used by <Task N>` on that item alone, never on the module.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

- [ ] **Step 5: Smoke-test the window**

Run: `cargo run --release -p sidacid`
Expected:
- A 1280×800 window opens with the transport bar.
- Pressing Play starts the demo groove, and Stop stops it.
- Dragging BPM changes the tempo live.
- The CPU readout updates.

Close the window. If no audio device is available, the red "No audio" banner and its Retry button appear instead. Report what you saw.

- [ ] **Step 6: Commit**

```bash
git add crates/app
git commit -m "feat(app): window, transport bar, knob widget and live audio pump"
```

---

### Task 7: Synth panels — knobs, pattern pads and the 303 step editor

**Files:**
- Create: `crates/app/src/ui/pads.rs`, `ui/step_edit.rs`, `ui/step303.rs`, `ui/synth_panel.rs`
- Modify: `crates/app/src/ui/mod.rs`

**Interfaces:**
- Produces:
  - `step_edit::{NOTE_NAMES, cycle_gate, gate_label, step_label, nudge_note, toggle_row, WriteKey, StepWriter, WriteAction, key_to_write}`
  - `SidAcidApp::{pattern_pads, synth_panel, step303, handle_step_write}`
  - New `ViewState` fields: `bank: [u8; 4]` and `writers: [Option<StepWriter>; 2]`
- Step-write: clicking a step's note cell starts step-write at that step. While it is active, keys are handled by `handle_step_write` (deviation 1).

- [ ] **Step 1: Write the failing tests**

`ui/step_edit.rs`, tests first:
```rust
//! Pure 303-editor logic: labels, row toggles, note nudging and the
//! step-write keyboard state machine (tracker layout, see plan deviation 1).

#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::project::SynthPattern;

    fn on(note: u8, octave: i8) -> SynthStep {
        SynthStep { note, octave, gate: Gate::On, accent: false, slide: false }
    }

    #[test]
    fn gate_cycles_off_on_tie() {
        assert_eq!(cycle_gate(Gate::Off), Gate::On);
        assert_eq!(cycle_gate(Gate::On), Gate::Tie);
        assert_eq!(cycle_gate(Gate::Tie), Gate::Off);
    }

    #[test]
    fn labels() {
        assert_eq!(step_label(&on(1, 0)), "C#");
        assert_eq!(step_label(&SynthStep::default()), "·");
        assert_eq!(gate_label(Gate::Tie), "—");
    }

    #[test]
    fn nudge_crosses_octaves_and_clamps() {
        assert_eq!(nudge_note(on(11, 0), 1), on(0, 1));
        assert_eq!(nudge_note(on(0, 0), -1), on(11, -1));
        assert_eq!(nudge_note(on(0, -1), -5), on(0, -1));
        assert_eq!(nudge_note(on(11, 1), 3), on(11, 1));
    }

    #[test]
    fn row_toggles() {
        let s = on(0, 0);
        assert_eq!(toggle_row(1, s).octave, -1);
        assert_eq!(toggle_row(1, toggle_row(1, s)).octave, 0);
        assert_eq!(toggle_row(2, s).octave, 1);
        assert!(toggle_row(3, s).accent);
        assert!(toggle_row(4, s).slide);
        assert_eq!(toggle_row(5, s).gate, Gate::Tie);
    }

    #[test]
    fn writer_enters_notes_and_advances() {
        let mut p = SynthPattern::default();
        p.length = 4;
        let mut w = StepWriter::new(3);
        match w.handle(WriteKey::Note(4), &p) {
            WriteAction::Set { step: 3, value } => assert_eq!(value, on(4, 0)),
            other => panic!("{other:?}"),
        }
        assert_eq!(w.cursor, 0, "wraps at the pattern length");
        match w.handle(WriteKey::Note(14), &p) {
            WriteAction::Set { step: 0, value } => assert_eq!(value, on(2, 1)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn writer_rest_tie_and_marks() {
        let mut p = SynthPattern::default();
        p.steps[0] = on(5, 0);
        let mut w = StepWriter::new(0);
        match w.handle(WriteKey::Accent, &p) {
            WriteAction::Set { step: 0, value } => assert!(value.accent && value.note == 5),
            other => panic!("{other:?}"),
        }
        assert_eq!(w.cursor, 0, "marks do not advance");
        assert!(matches!(w.handle(WriteKey::Rest, &p), WriteAction::Set { step: 0, value } if value.gate == Gate::Off));
        assert!(matches!(w.handle(WriteKey::Tie, &p), WriteAction::Set { step: 1, value } if value.gate == Gate::Tie));
        assert_eq!(w.cursor, 2);
        assert!(matches!(w.handle(WriteKey::Back, &p), WriteAction::Set { step: 1, value } if value.gate == Gate::Off));
        assert_eq!(w.cursor, 1);
        assert!(matches!(w.handle(WriteKey::Exit, &p), WriteAction::Exit));
    }

    #[test]
    fn writer_octave_is_bounded() {
        let p = SynthPattern::default();
        let mut w = StepWriter::new(0);
        w.handle(WriteKey::OctaveDown, &p);
        w.handle(WriteKey::OctaveDown, &p);
        assert_eq!(w.octave, -1);
        assert!(matches!(w.handle(WriteKey::Note(0), &p), WriteAction::Set { value, .. } if value.octave == -1));
        w.handle(WriteKey::OctaveUp, &p);
        w.handle(WriteKey::OctaveUp, &p);
        w.handle(WriteKey::OctaveUp, &p);
        assert_eq!(w.octave, 1);
        assert!(matches!(w.handle(WriteKey::Note(12), &p), WriteAction::Set { value, .. } if value.octave == 1));
    }

    #[test]
    fn cursor_is_clamped_after_the_pattern_shrinks() {
        let mut p = SynthPattern::default();
        p.length = 2;
        let mut w = StepWriter::new(10);
        assert!(matches!(w.handle(WriteKey::Note(0), &p), WriteAction::Set { step: 0, .. }));
    }

    #[test]
    fn tracker_key_layout() {
        use eframe::egui::Key;
        assert_eq!(key_to_write(Key::Z), Some(WriteKey::Note(0)));
        assert_eq!(key_to_write(Key::S), Some(WriteKey::Note(1)));
        assert_eq!(key_to_write(Key::M), Some(WriteKey::Note(11)));
        assert_eq!(key_to_write(Key::Q), Some(WriteKey::Note(12)));
        assert_eq!(key_to_write(Key::I), Some(WriteKey::Note(24)));
        assert_eq!(key_to_write(Key::Space), Some(WriteKey::Rest));
        assert_eq!(key_to_write(Key::A), Some(WriteKey::Accent));
        assert_eq!(key_to_write(Key::F), Some(WriteKey::Slide));
        assert_eq!(key_to_write(Key::OpenBracket), Some(WriteKey::OctaveDown));
        assert_eq!(key_to_write(Key::K), None);
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Add `mod step_edit;` to `ui/mod.rs`, then run `cargo test -p sidacid step_edit`.
Expected: compile errors.

- [ ] **Step 3: Implement step_edit**

Insert above the tests in `ui/step_edit.rs`:
```rust
use acid_engine::project::{Gate, SynthPattern, SynthStep};
use eframe::egui::Key;

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn cycle_gate(gate: Gate) -> Gate {
    match gate {
        Gate::Off => Gate::On,
        Gate::On => Gate::Tie,
        Gate::Tie => Gate::Off,
    }
}

pub fn gate_label(gate: Gate) -> &'static str {
    match gate {
        Gate::Off => "·",
        Gate::On => "■",
        Gate::Tie => "—",
    }
}

pub fn step_label(step: &SynthStep) -> &'static str {
    if step.gate == Gate::Off { "·" } else { NOTE_NAMES[step.note as usize % 12] }
}

/// Moves a step's pitch by `delta` semitones within the three-octave range.
pub fn nudge_note(step: SynthStep, delta: i32) -> SynthStep {
    let total = ((step.octave.clamp(-1, 1) as i32 + 1) * 12 + (step.note % 12) as i32 + delta).clamp(0, 35);
    SynthStep { note: (total % 12) as u8, octave: (total / 12) as i8 - 1, ..step }
}

/// Click behaviour of the 303 rows: 1 Down, 2 Up, 3 Accent, 4 Slide, 5 Gate.
pub fn toggle_row(row: usize, step: SynthStep) -> SynthStep {
    match row {
        1 => SynthStep { octave: if step.octave < 0 { 0 } else { -1 }, ..step },
        2 => SynthStep { octave: if step.octave > 0 { 0 } else { 1 }, ..step },
        3 => SynthStep { accent: !step.accent, ..step },
        4 => SynthStep { slide: !step.slide, ..step },
        5 => SynthStep { gate: cycle_gate(step.gate), ..step },
        _ => step,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKey {
    /// Semitones above C of the writer's octave (0..=24).
    Note(u8),
    Rest,
    Tie,
    Accent,
    Slide,
    Left,
    Right,
    Back,
    OctaveDown,
    OctaveUp,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WriteAction {
    Set { step: usize, value: SynthStep },
    Moved,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepWriter {
    pub cursor: usize,
    pub octave: i8,
}

impl StepWriter {
    pub fn new(cursor: usize) -> Self {
        Self { cursor, octave: 0 }
    }

    pub fn handle(&mut self, key: WriteKey, pattern: &SynthPattern) -> WriteAction {
        let len = (pattern.length as usize).clamp(1, pattern.steps.len());
        if self.cursor >= len {
            self.cursor = 0;
        }
        let here = self.cursor;
        let current = pattern.steps[here];
        match key {
            WriteKey::Note(semis) => {
                let octave = (self.octave as i32 + semis as i32 / 12).clamp(-1, 1) as i8;
                let value = SynthStep { note: semis % 12, octave, gate: Gate::On, ..current };
                self.cursor = (here + 1) % len;
                WriteAction::Set { step: here, value }
            }
            WriteKey::Rest => {
                self.cursor = (here + 1) % len;
                WriteAction::Set { step: here, value: SynthStep { gate: Gate::Off, ..current } }
            }
            WriteKey::Tie => {
                self.cursor = (here + 1) % len;
                WriteAction::Set { step: here, value: SynthStep { gate: Gate::Tie, ..current } }
            }
            WriteKey::Accent => WriteAction::Set { step: here, value: SynthStep { accent: !current.accent, ..current } },
            WriteKey::Slide => WriteAction::Set { step: here, value: SynthStep { slide: !current.slide, ..current } },
            WriteKey::Left => {
                self.cursor = (here + len - 1) % len;
                WriteAction::Moved
            }
            WriteKey::Right => {
                self.cursor = (here + 1) % len;
                WriteAction::Moved
            }
            WriteKey::Back => {
                let prev = (here + len - 1) % len;
                self.cursor = prev;
                WriteAction::Set { step: prev, value: SynthStep { gate: Gate::Off, ..pattern.steps[prev] } }
            }
            WriteKey::OctaveDown => {
                self.octave = (self.octave - 1).max(-1);
                WriteAction::Moved
            }
            WriteKey::OctaveUp => {
                self.octave = (self.octave + 1).min(1);
                WriteAction::Moved
            }
            WriteKey::Exit => WriteAction::Exit,
        }
    }
}

/// Tracker piano layout: Z-row one octave, Q-row the octave above.
pub fn key_to_write(key: Key) -> Option<WriteKey> {
    use WriteKey::*;
    Some(match key {
        Key::Z => Note(0),
        Key::S => Note(1),
        Key::X => Note(2),
        Key::D => Note(3),
        Key::C => Note(4),
        Key::V => Note(5),
        Key::G => Note(6),
        Key::B => Note(7),
        Key::H => Note(8),
        Key::N => Note(9),
        Key::J => Note(10),
        Key::M => Note(11),
        Key::Comma | Key::Q => Note(12),
        Key::Num2 => Note(13),
        Key::W => Note(14),
        Key::Num3 => Note(15),
        Key::E => Note(16),
        Key::R => Note(17),
        Key::Num5 => Note(18),
        Key::T => Note(19),
        Key::Num6 => Note(20),
        Key::Y => Note(21),
        Key::Num7 => Note(22),
        Key::U => Note(23),
        Key::I => Note(24),
        Key::Space => Rest,
        Key::Minus => Tie,
        Key::A => Accent,
        Key::F => Slide,
        Key::ArrowLeft => Left,
        Key::ArrowRight => Right,
        Key::Backspace => Back,
        Key::OpenBracket => OctaveDown,
        Key::CloseBracket => OctaveUp,
        Key::Escape => Exit,
        _ => return None,
    })
}
```

- [ ] **Step 4: Implement pads, the 303 editor and the synth panel**

`ui/pads.rs`:
```rust
//! Bank (A–H) and pattern (1–16) pads for one machine.

use super::{SidAcidApp, theme};
use acid_engine::project::BANK_SIZE;
use eframe::egui;

pub fn bank_letter(bank: u8) -> String {
    ((b'A' + bank) as char).to_string()
}

impl SidAcidApp {
    pub(crate) fn pattern_pads(&mut self, ui: &mut egui::Ui, machine: usize) {
        let selected = self.doc.project.selected[machine];
        let playing = self.status.playing.then_some(self.status.patterns[machine]);
        let queued = self.status.queued[machine];
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                for bank in 0..8u8 {
                    let shown = self.view.bank[machine] == bank;
                    if ui.selectable_label(shown, bank_letter(bank)).clicked() {
                        self.view.bank[machine] = bank;
                    }
                }
            });
            ui.horizontal(|ui| {
                let bank = self.view.bank[machine];
                for i in 0..BANK_SIZE as u8 {
                    let slot = bank * BANK_SIZE as u8 + i;
                    let mut text = egui::RichText::new(format!("{:>2}", i + 1)).monospace();
                    if Some(slot) == queued {
                        text = text.color(theme::QUEUED);
                    } else if Some(slot) == playing {
                        text = text.color(theme::ACID).strong();
                    }
                    if ui.selectable_label(slot == selected, text).clicked() {
                        let cmds = self.doc.select_pattern(machine, slot);
                        self.send(cmds);
                        self.view.focus = machine;
                    }
                }
            });
        });
    }
}
```

`ui/step303.rs`:
```rust
//! The classic 303 step editor: one column per step; rows for note,
//! octave down/up, accent, slide and gate.

use super::step_edit::{StepWriter, WriteAction, gate_label, key_to_write, nudge_note, step_label, toggle_row};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use eframe::egui;

const CELL: [f32; 2] = [28.0, 18.0];
const ROWS: [&str; 6] = ["Note", "Down", "Up", "Acc", "Slide", "Gate"];

impl SidAcidApp {
    pub(crate) fn step303(&mut self, ui: &mut egui::Ui, synth: usize) {
        let slot = self.doc.project.selected[synth];
        let pattern = self.doc.project.synths[synth].patterns[slot as usize].clone();
        let len = pattern.length as usize;
        let playhead =
            (self.status.playing && self.status.patterns[synth] == slot).then_some(self.status.playhead[synth]);
        let cursor = self.view.writers[synth].map(|w| w.cursor);
        let mut edits = Vec::new();
        let mut start_writer = None;
        egui::ScrollArea::horizontal().id_salt(("s303", synth)).show(ui, |ui| {
            egui::Grid::new(("grid303", synth)).spacing([2.0, 2.0]).show(ui, |ui| {
                ui.label("");
                for i in 0..len {
                    let mut t = egui::RichText::new(format!("{}", i + 1)).size(10.0);
                    if Some(i) == playhead {
                        t = t.color(theme::ACID).strong();
                    }
                    ui.label(t);
                }
                ui.end_row();
                for (row, name) in ROWS.iter().enumerate() {
                    ui.label(*name);
                    for (i, s) in pattern.steps.iter().take(len).enumerate() {
                        let (text, on) = match row {
                            0 => (step_label(s), Some(i) == cursor || (s.gate != acid_engine::project::Gate::Off)),
                            1 => ("v", s.octave < 0),
                            2 => ("^", s.octave > 0),
                            3 => ("A", s.accent),
                            4 => ("S", s.slide),
                            _ => (gate_label(s.gate), s.gate != acid_engine::project::Gate::Off),
                        };
                        let mut rich = egui::RichText::new(text).monospace();
                        if row == 0 && Some(i) == cursor {
                            rich = rich.color(theme::QUEUED).strong();
                        } else if row == 3 && s.accent {
                            rich = rich.color(theme::ACCENT);
                        } else if row == 4 && s.slide {
                            rich = rich.color(theme::SLIDE);
                        }
                        let r = ui.add_sized(CELL, egui::Button::new(rich).selected(on));
                        if row == 0 {
                            if r.clicked() {
                                start_writer = Some(i);
                            }
                            if r.secondary_clicked() {
                                edits.push((i, toggle_row(5, *s)));
                            }
                            let scroll = if r.hovered() { ui.input(|inp| inp.raw_scroll_delta.y) } else { 0.0 };
                            if scroll != 0.0 {
                                edits.push((i, nudge_note(*s, if scroll > 0.0 { 1 } else { -1 })));
                            }
                            r.on_hover_text("Click: step-write from here  ·  Scroll: pitch  ·  Right-click: gate");
                        } else if r.clicked() {
                            edits.push((i, toggle_row(row, *s)));
                        }
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(i) = start_writer {
            self.view.writers[synth] = Some(StepWriter::new(i));
            self.view.writers[1 - synth] = None;
            self.view.focus = synth;
        }
        for (step, value) in edits {
            let cmds = self.doc.apply(Edit::SynthStep { synth, slot, step, value }, false);
            self.send(cmds);
            self.view.focus = synth;
        }
    }

    /// Feeds key presses to the active step-writer, if any.
    pub(crate) fn handle_step_write(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let Some(synth) = (0..2).find(|&s| self.view.writers[s].is_some()) else { return };
        let keys: Vec<egui::Key> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } if !modifiers.command => Some(*key),
                    _ => None,
                })
                .collect()
        });
        for key in keys {
            let Some(write_key) = key_to_write(key) else { continue };
            let slot = self.doc.project.selected[synth];
            let Some(mut writer) = self.view.writers[synth] else { return };
            let action = writer.handle(write_key, &self.doc.project.synths[synth].patterns[slot as usize]);
            match action {
                WriteAction::Set { step, value } => {
                    self.view.writers[synth] = Some(writer);
                    let cmds = self.doc.apply(Edit::SynthStep { synth, slot, step, value }, false);
                    self.send(cmds);
                }
                WriteAction::Moved => self.view.writers[synth] = Some(writer),
                WriteAction::Exit => {
                    self.view.writers[synth] = None;
                    return;
                }
            }
        }
    }
}
```

`ui/synth_panel.rs`:
```rust
//! A synth panel: 303 knobs, pattern pads, length/transpose, editor.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::params::{ParamId, SynthParam};
use acid_engine::project::PatternRef;
use eframe::egui;

const KNOBS: [(SynthParam, &str); 15] = [
    (SynthParam::Tune, "Tune"),
    (SynthParam::Cutoff, "Cutoff"),
    (SynthParam::Resonance, "Reso"),
    (SynthParam::EnvMod, "Env Mod"),
    (SynthParam::Decay, "Decay"),
    (SynthParam::Accent, "Accent"),
    (SynthParam::Wave, "Wave"),
    (SynthParam::PulseWidth, "PW"),
    (SynthParam::FilterMode, "Mode"),
    (SynthParam::Sync, "Sync"),
    (SynthParam::SyncPitch, "Sync Pt"),
    (SynthParam::Unison, "Unison"),
    (SynthParam::Detune, "Detune"),
    (SynthParam::Spread, "Spread"),
    (SynthParam::Volume, "Volume"),
];

impl SidAcidApp {
    pub(crate) fn synth_panel(&mut self, ui: &mut egui::Ui, synth: usize) {
        let slot = self.doc.project.selected[synth];
        let title = format!("SYNTH {}   {}", synth + 1, PatternRef(slot).label());
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt(("synth", synth))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (p, label) in KNOBS {
                        self.param_knob(ui, ParamId::Synth(synth as u8, p), label);
                    }
                });
                ui.horizontal(|ui| {
                    self.pattern_pads(ui, synth);
                    ui.separator();
                    let pattern = &self.doc.project.synths[synth].patterns[slot as usize];
                    let mut length = pattern.length;
                    let mut transpose = pattern.transpose;
                    if ui.add(egui::DragValue::new(&mut length).range(1..=64).prefix("Len ")).changed() {
                        let cmds = self.doc.apply(Edit::SynthLength { synth, slot, length }, false);
                        self.send(cmds);
                    }
                    if ui.add(egui::DragValue::new(&mut transpose).range(-12..=12).prefix("Trans ")).changed() {
                        let cmds = self.doc.apply(Edit::SynthTranspose { synth, slot, transpose }, false);
                        self.send(cmds);
                    }
                    if let Some(w) = self.view.writers[synth] {
                        ui.label(format!("Step-write: step {}  octave {:+}  (Esc to stop)", w.cursor + 1, w.octave));
                    }
                });
                self.step303(ui, synth);
            });
    }
}
```

- [ ] **Step 5: Wire the synth panels into the app**

In `ui/mod.rs`:
1. Add the modules `mod pads;`, `mod step303;`, `mod step_edit;` and `mod synth_panel;`. Keep the module list alphabetical.
2. Add these fields to `ViewState`:
   ```rust
       /// Bank shown on each machine's pads (0..8).
       pub bank: [u8; 4],
       /// Active 303 step-writer per synth.
       pub writers: [Option<step_edit::StepWriter>; 2],
   ```
3. In `ui()`, call `self.handle_step_write(ctx);` right after `self.pump();`.
4. Replace the body of `rack` with:
   ```rust
           for synth in 0..2 {
               self.synth_panel(ui, synth);
               ui.add_space(6.0);
           }
   ```

- [ ] **Step 6: Run the tests and smoke-test**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean. `ui_draws_headless` now draws both synth panels.

Run: `cargo run --release -p sidacid`. Check each of these:
1. The knobs change the sound live.
2. Clicking pattern pads switches patterns at the next pattern wrap. While queued, the pad is shown in amber.
3. Clicking the Accent, Slide and Gate cells changes the line.
4. Clicking a note cell starts step-write. Typing Z X C enters notes, and Esc leaves step-write.

Report what you saw.

- [ ] **Step 7: Commit**

```bash
git add crates/app
git commit -m "feat(app): synth panels with knobs, pattern pads and 303 step editor"
```

---

### Task 8: Piano roll and the view switch

**Files:**
- Create: `crates/app/src/ui/roll_edit.rs`, `ui/pianoroll.rs`
- Modify: `crates/app/src/ui/mod.rs`, `ui/synth_panel.rs`

**Interfaces:**
- Produces:
  - `roll_edit::{ROLL_ROWS, RollNote, RollDrag, RollGeom, row_of, notes, note_at, place, delete_at, resize, move_note, toggle_accent, toggle_slide}`
  - `SidAcidApp::piano_roll`
  - The `ViewState` field `roll_drag: [Option<RollDrag>; 2]`
- Note model:
  - A note is an `On` step followed by `Tie` steps.
  - Accent lives on the note's first step, and slide on its last step.
  - The roll is monophonic: placing a note removes any notes it overlaps.

- [ ] **Step 1: Write the failing tests**

`ui/roll_edit.rs`, tests first:
```rust
//! Pure piano-roll editing on the shared mono pattern, plus geometry.
//! A note is an On step followed by Tie steps; accent is on its first
//! step and slide on its last (the slide into the following note).

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(len: u8) -> SynthPattern {
        SynthPattern { length: len, ..SynthPattern::default() }
    }

    #[test]
    fn notes_group_on_and_ties() {
        let mut p = pattern(16);
        place(&mut p, 2, 3, 14);
        place(&mut p, 8, 1, 0);
        let n = notes(&p);
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].start, n[0].len, n[0].row), (2, 3, 14));
        assert_eq!(p.steps[2].gate, Gate::On);
        assert_eq!(p.steps[3].gate, Gate::Tie);
        assert_eq!((p.steps[2].note, p.steps[2].octave), (2, 0));
        assert_eq!((n[1].start, n[1].row), (8, 0));
        assert_eq!(p.steps[8].octave, -1);
    }

    #[test]
    fn placing_over_a_note_replaces_it() {
        let mut p = pattern(16);
        place(&mut p, 2, 4, 10);
        place(&mut p, 4, 1, 20);
        let n = notes(&p);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].start, 4);
        assert_eq!(p.steps[2].gate, Gate::Off);
    }

    #[test]
    fn placing_before_a_tie_keeps_the_notes_separate() {
        let mut p = pattern(16);
        place(&mut p, 4, 3, 10);
        place(&mut p, 2, 2, 12);
        assert_eq!(notes(&p).len(), 1, "overlapping note removed");
        let mut p = pattern(16);
        p.steps[5].gate = Gate::Tie;
        place(&mut p, 3, 2, 12);
        assert_eq!(p.steps[5].gate, Gate::On);
    }

    #[test]
    fn notes_are_clipped_to_the_pattern_length() {
        let mut p = pattern(8);
        place(&mut p, 6, 10, 5);
        assert_eq!(notes(&p)[0].len, 2);
        place(&mut p, 9, 1, 5);
        assert_eq!(notes(&p).len(), 1, "start beyond length is ignored");
    }

    #[test]
    fn delete_resize_move() {
        let mut p = pattern(16);
        place(&mut p, 0, 2, 12);
        toggle_accent(&mut p, 1);
        toggle_slide(&mut p, 0);
        assert!(p.steps[0].accent && p.steps[1].slide);
        resize(&mut p, 0, 4);
        assert_eq!(note_at(&p, 3).unwrap().len, 4);
        assert!(p.steps[0].accent && p.steps[3].slide, "marks kept on resize");
        move_note(&mut p, 0, 6, 20);
        let n = note_at(&p, 6).unwrap();
        assert_eq!((n.start, n.len, n.row), (6, 4, 20));
        assert!(p.steps[6].accent && p.steps[9].slide, "marks kept on move");
        assert!(delete_at(&mut p, 8));
        assert!(notes(&p).is_empty());
        assert!(!delete_at(&mut p, 8));
    }

    #[test]
    fn geometry_maps_cells() {
        let g = RollGeom { left: 100.0, top: 50.0, cell_w: 20.0, cell_h: 10.0, steps: 16 };
        assert_eq!(g.cell_at(100.0, 50.0), Some((0, ROLL_ROWS - 1)));
        assert_eq!(g.cell_at(145.0, 50.0 + 10.0 * 35.5), Some((2, 0)));
        assert_eq!(g.cell_at(99.0, 60.0), None);
        assert_eq!(g.cell_at(100.0 + 20.0 * 16.0, 60.0), None);
        assert_eq!(g.cell_at_clamped(1000.0, -5.0), (15, ROLL_ROWS - 1));
        let (x0, y0, x1, y1) = g.note_rect(2, 0, 3);
        assert_eq!((x0, x1), (140.0, 200.0));
        assert_eq!((y0, y1), (50.0 + 350.0, 50.0 + 360.0));
        assert!(g.near_right_edge(&RollNote { start: 2, len: 3, row: 0, accent: false, slide: false }, 198.0));
        assert!(!g.near_right_edge(&RollNote { start: 2, len: 3, row: 0, accent: false, slide: false }, 170.0));
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Add `mod roll_edit;` to `ui/mod.rs`, then run `cargo test -p sidacid roll_edit`.
Expected: compile errors.

- [ ] **Step 3: Implement roll_edit**

Insert above the tests in `ui/roll_edit.rs`:
```rust
use acid_engine::project::{Gate, SynthPattern, SynthStep};

/// Three octaves: row 0 = C at octave -1, row 35 = B at octave +1.
pub const ROLL_ROWS: usize = 36;
const EDGE_PX: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollNote {
    pub start: usize,
    pub len: usize,
    pub row: usize,
    pub accent: bool,
    pub slide: bool,
}

/// An in-progress mouse drag in the roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollDrag {
    Move { start: usize, grab: usize },
    Resize { start: usize },
}

pub fn row_of(step: &SynthStep) -> usize {
    (step.octave.clamp(-1, 1) + 1) as usize * 12 + (step.note % 12) as usize
}

fn step_for(row: usize, gate: Gate) -> SynthStep {
    SynthStep { note: (row % 12) as u8, octave: (row / 12) as i8 - 1, gate, accent: false, slide: false }
}

fn length_of(p: &SynthPattern) -> usize {
    (p.length as usize).clamp(1, p.steps.len())
}

pub fn notes(p: &SynthPattern) -> Vec<RollNote> {
    let len = length_of(p);
    let mut out = Vec::new();
    let mut i = 0;
    while i < len {
        if p.steps[i].gate == Gate::Off {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        while end < len && p.steps[end].gate == Gate::Tie {
            end += 1;
        }
        out.push(RollNote {
            start,
            len: end - start,
            row: row_of(&p.steps[start]),
            accent: p.steps[start].accent,
            slide: p.steps[end - 1].slide,
        });
        i = end;
    }
    out
}

pub fn note_at(p: &SynthPattern, step: usize) -> Option<RollNote> {
    notes(p).into_iter().find(|n| step >= n.start && step < n.start + n.len)
}

fn erase(p: &mut SynthPattern, n: RollNote) {
    for s in &mut p.steps[n.start..n.start + n.len] {
        *s = SynthStep { gate: Gate::Off, accent: false, slide: false, ..*s };
    }
}

/// Writes a note (On + Ties), removing any note it overlaps. Starts at or
/// beyond the pattern length are ignored; the note is clipped to it.
pub fn place(p: &mut SynthPattern, start: usize, len: usize, row: usize) {
    let plen = length_of(p);
    if start >= plen {
        return;
    }
    let len = len.clamp(1, plen - start);
    let row = row.min(ROLL_ROWS - 1);
    for n in notes(p) {
        if n.start < start + len && start < n.start + n.len {
            erase(p, n);
        }
    }
    for k in 0..len {
        p.steps[start + k] = step_for(row, if k == 0 { Gate::On } else { Gate::Tie });
    }
    if start + len < plen && p.steps[start + len].gate == Gate::Tie {
        p.steps[start + len].gate = Gate::On;
    }
}

pub fn delete_at(p: &mut SynthPattern, step: usize) -> bool {
    match note_at(p, step) {
        Some(n) => {
            erase(p, n);
            true
        }
        None => false,
    }
}

fn restore_marks(p: &mut SynthPattern, start: usize, accent: bool, slide: bool) {
    if let Some(n) = note_at(p, start) {
        p.steps[n.start].accent = accent;
        p.steps[n.start + n.len - 1].slide = slide;
    }
}

/// Changes the length of the note starting at `start`, keeping its marks.
pub fn resize(p: &mut SynthPattern, start: usize, new_len: usize) {
    if let Some(n) = note_at(p, start)
        && n.start == start
    {
        erase(p, n);
        place(p, start, new_len, n.row);
        restore_marks(p, start, n.accent, n.slide);
    }
}

/// Moves the note starting at `start` to `new_start` / `new_row`.
pub fn move_note(p: &mut SynthPattern, start: usize, new_start: usize, new_row: usize) {
    if let Some(n) = note_at(p, start)
        && n.start == start
    {
        erase(p, n);
        place(p, new_start, n.len, new_row);
        restore_marks(p, new_start, n.accent, n.slide);
    }
}

pub fn toggle_accent(p: &mut SynthPattern, step: usize) {
    if let Some(n) = note_at(p, step) {
        p.steps[n.start].accent = !p.steps[n.start].accent;
    }
}

pub fn toggle_slide(p: &mut SynthPattern, step: usize) {
    if let Some(n) = note_at(p, step) {
        let last = n.start + n.len - 1;
        p.steps[last].slide = !p.steps[last].slide;
    }
}

/// Grid placement in screen pixels. Row `ROLL_ROWS - 1` is at the top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RollGeom {
    pub left: f32,
    pub top: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub steps: usize,
}

impl RollGeom {
    pub fn cell_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let col = ((x - self.left) / self.cell_w).floor();
        let r = ((y - self.top) / self.cell_h).floor();
        if col < 0.0 || r < 0.0 || col as usize >= self.steps || r as usize >= ROLL_ROWS {
            return None;
        }
        Some((col as usize, ROLL_ROWS - 1 - r as usize))
    }

    pub fn cell_at_clamped(&self, x: f32, y: f32) -> (usize, usize) {
        let col = ((x - self.left) / self.cell_w).floor().clamp(0.0, (self.steps.max(1) - 1) as f32) as usize;
        let r = ((y - self.top) / self.cell_h).floor().clamp(0.0, (ROLL_ROWS - 1) as f32) as usize;
        (col, ROLL_ROWS - 1 - r)
    }

    /// (x0, y0, x1, y1) of a note.
    pub fn note_rect(&self, start: usize, row: usize, len: usize) -> (f32, f32, f32, f32) {
        let x0 = self.left + start as f32 * self.cell_w;
        let y0 = self.top + (ROLL_ROWS - 1 - row) as f32 * self.cell_h;
        (x0, y0, x0 + len as f32 * self.cell_w, y0 + self.cell_h)
    }

    pub fn near_right_edge(&self, note: &RollNote, x: f32) -> bool {
        let (_, _, x1, _) = self.note_rect(note.start, note.row, note.len);
        x >= x1 - EDGE_PX && x <= x1 + 1.0
    }
}
```

- [ ] **Step 4: Implement the piano roll UI and the view switch**

`ui/pianoroll.rs`:
```rust
//! Piano roll over the same mono pattern as the 303 view. Click to add a
//! note, drag its body to move, drag its right edge to lengthen,
//! right-click to delete. Accent/slide lanes sit underneath.

use super::roll_edit::{ROLL_ROWS, RollDrag, RollGeom, move_note, note_at, notes, place, resize, toggle_accent, toggle_slide};
use super::step_edit::NOTE_NAMES;
use super::{SidAcidApp, theme};
use crate::document::Edit;
use eframe::egui::{self, Align2, FontId, Sense, Stroke, pos2, vec2};

const CELL_W: f32 = 22.0;
const CELL_H: f32 = 9.0;
const KEY_W: f32 = 30.0;
const LANE_H: f32 = 12.0;
const BLACK_KEYS: [bool; 12] = [false, true, false, true, false, false, true, false, true, false, true, false];

impl SidAcidApp {
    pub(crate) fn piano_roll(&mut self, ui: &mut egui::Ui, synth: usize) {
        let slot = self.doc.project.selected[synth];
        let mut pattern = self.doc.project.synths[synth].patterns[slot as usize].clone();
        let len = pattern.length as usize;
        let playhead =
            (self.status.playing && self.status.patterns[synth] == slot).then_some(self.status.playhead[synth]);
        let mut changed = false;
        let mut finished = false;
        egui::ScrollArea::horizontal().id_salt(("roll", synth)).show(ui, |ui| {
            let size = vec2(KEY_W + CELL_W * len as f32, CELL_H * ROLL_ROWS as f32 + 2.0 * LANE_H + 4.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
            let geom = RollGeom { left: rect.left() + KEY_W, top: rect.top(), cell_w: CELL_W, cell_h: CELL_H, steps: len };
            let grid_bottom = rect.top() + CELL_H * ROLL_ROWS as f32;
            let accent_lane = grid_bottom + 2.0;
            let slide_lane = accent_lane + LANE_H + 2.0;
            let painter = ui.painter_at(rect);

            for row in 0..ROLL_ROWS {
                let (_, y0, _, y1) = geom.note_rect(0, row, 1);
                let bg = if BLACK_KEYS[row % 12] { theme::GRID_DARK } else { theme::GRID_BG };
                painter.rect_filled(egui::Rect::from_min_max(pos2(geom.left, y0), pos2(rect.right(), y1)), 0.0, bg);
                if row % 12 == 0 {
                    painter.text(
                        pos2(rect.left() + 2.0, (y0 + y1) / 2.0),
                        Align2::LEFT_CENTER,
                        format!("{}{}", NOTE_NAMES[0], row as i32 / 12 - 1),
                        FontId::monospace(9.0),
                        ui.visuals().text_color(),
                    );
                }
            }
            for step in 0..=len {
                let x = geom.left + step as f32 * CELL_W;
                let colour = if step % 4 == 0 { theme::KNOB_TRACK } else { theme::GRID_LINE };
                painter.line_segment([pos2(x, rect.top()), pos2(x, grid_bottom)], Stroke::new(1.0, colour));
            }
            if let Some(step) = playhead {
                let x = geom.left + step as f32 * CELL_W;
                painter.rect_filled(
                    egui::Rect::from_min_max(pos2(x, rect.top()), pos2(x + CELL_W, grid_bottom)),
                    0.0,
                    theme::ACID.gamma_multiply(0.15),
                );
            }
            for n in notes(&pattern) {
                let (x0, y0, x1, y1) = geom.note_rect(n.start, n.row, n.len);
                let r = egui::Rect::from_min_max(pos2(x0 + 1.0, y0 + 1.0), pos2(x1 - 1.0, y1 - 1.0));
                painter.rect_filled(r, 2.0, if n.accent { theme::ACCENT } else { theme::ACID });
                if n.slide {
                    painter.rect_stroke(r, 2.0, Stroke::new(2.0, theme::SLIDE), egui::StrokeKind::Inside);
                }
                let lane = |y: f32, on: bool, label: &str, colour| {
                    let lx = geom.left + n.start as f32 * CELL_W;
                    let lr = egui::Rect::from_min_size(pos2(lx + 1.0, y), vec2(CELL_W - 2.0, LANE_H));
                    painter.rect_filled(lr, 2.0, if on { colour } else { theme::GRID_LINE });
                    painter.text(lr.center(), Align2::CENTER_CENTER, label, FontId::monospace(9.0), egui::Color32::BLACK);
                };
                lane(accent_lane, n.accent, "A", theme::ACCENT);
                lane(slide_lane, n.slide, "S", theme::SLIDE);
            }

            let pointer = resp.interact_pointer_pos();
            let in_lane = |y: f32, top: f32| y >= top && y < top + LANE_H;
            if resp.clicked()
                && let Some(p) = pointer
            {
                let (step, _) = geom.cell_at_clamped(p.x, p.y);
                if in_lane(p.y, accent_lane) {
                    toggle_accent(&mut pattern, step);
                    changed = true;
                    finished = true;
                } else if in_lane(p.y, slide_lane) {
                    toggle_slide(&mut pattern, step);
                    changed = true;
                    finished = true;
                } else if let Some((step, row)) = geom.cell_at(p.x, p.y)
                    && note_at(&pattern, step).is_none()
                {
                    place(&mut pattern, step, 1, row);
                    changed = true;
                    finished = true;
                }
            }
            if resp.secondary_clicked()
                && let Some(p) = pointer
                && let Some((step, _)) = geom.cell_at(p.x, p.y)
            {
                changed |= super::roll_edit::delete_at(&mut pattern, step);
                finished = true;
            }
            if resp.drag_started()
                && let Some(p) = pointer
                && let Some((step, row)) = geom.cell_at(p.x, p.y)
            {
                self.view.roll_drag[synth] = Some(match note_at(&pattern, step) {
                    Some(n) if geom.near_right_edge(&n, p.x) => RollDrag::Resize { start: n.start },
                    Some(n) => RollDrag::Move { start: n.start, grab: step - n.start },
                    None => {
                        place(&mut pattern, step, 1, row);
                        changed = true;
                        RollDrag::Resize { start: step }
                    }
                });
            }
            if resp.dragged()
                && let Some(p) = pointer
                && let Some(drag) = self.view.roll_drag[synth]
            {
                let (step, row) = geom.cell_at_clamped(p.x, p.y);
                match drag {
                    RollDrag::Resize { start } => {
                        let new_len = step.saturating_sub(start) + 1;
                        if note_at(&pattern, start).is_some_and(|n| n.start == start && n.len != new_len) {
                            resize(&mut pattern, start, new_len);
                            changed = true;
                        }
                    }
                    RollDrag::Move { start, grab } => {
                        let new_start = step.saturating_sub(grab);
                        if let Some(n) = note_at(&pattern, start)
                            && n.start == start
                            && (new_start != start || row != n.row)
                        {
                            move_note(&mut pattern, start, new_start, row);
                            self.view.roll_drag[synth] = Some(RollDrag::Move { start: new_start, grab });
                            changed = true;
                        }
                    }
                }
            }
            if resp.drag_stopped() {
                self.view.roll_drag[synth] = None;
                finished = true;
            }
        });
        if changed {
            let dragging = self.view.roll_drag[synth].is_some();
            let cmds = self.doc.apply(Edit::SynthPattern { synth, slot, pattern: Box::new(pattern) }, dragging);
            self.send(cmds);
            self.view.focus = synth;
        }
        if finished {
            self.doc.end_gesture();
        }
    }
}
```
`painter.rect_stroke` takes a `StrokeKind` argument in egui 0.31. If your egui version's signature differs, adapt per the global constraints.

In `ui/synth_panel.rs`:
1. Add `use acid_engine::project::SeqView;`.
2. Inside the `ui.horizontal` row, after the transpose `DragValue`, add the view switch:
   ```rust
                       ui.separator();
                       let view = self.doc.project.synths[synth].view;
                       for (v, label) in [(SeqView::Step303, "303"), (SeqView::PianoRoll, "Piano roll")] {
                           if ui.selectable_label(view == v, label).clicked() && view != v {
                               let cmds = self.doc.apply(Edit::View { synth, view: v }, false);
                               self.send(cmds);
                               self.view.writers[synth] = None;
                           }
                       }
   ```
3. Replace the final `self.step303(ui, synth);` with:
   ```rust
                   match self.doc.project.synths[synth].view {
                       SeqView::Step303 => self.step303(ui, synth),
                       SeqView::PianoRoll => self.piano_roll(ui, synth),
                   }
   ```

In `ui/mod.rs`, add `mod pianoroll;` and `mod roll_edit;`. Add this `ViewState` field:
```rust
    /// Mouse drag in progress in each synth's piano roll.
    pub roll_drag: [Option<roll_edit::RollDrag>; 2],
```

Add this test to the `tests` module in `ui/mod.rs`:
```rust
    #[test]
    fn piano_roll_view_draws_headless() {
        let mut doc = Document::new(acid_engine::demo::demo_project());
        doc.project.synths[0].view = acid_engine::project::SeqView::PianoRoll;
        let mut app = SidAcidApp::headless(doc);
        run_frames(&mut app, 2);
    }
```

- [ ] **Step 5: Run the tests and smoke-test**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

Run: `cargo run --release -p sidacid` and check:
1. Switch Synth 1 to "Piano roll". The demo line appears as notes.
2. Click to add a note, drag a note's body to move it, drag its right edge to lengthen it, and right-click to delete it.
3. Clicking in the A and S lanes toggles accent and slide.
4. Switching back to "303" shows the same line.
5. Ctrl+Z isn't wired up until Task 11.

Report what you saw.

- [ ] **Step 6: Commit**

```bash
git add crates/app
git commit -m "feat(app): piano roll editor with move/resize/delete and 303/roll view switch"
```

---

### Task 9: Drum machine panels

**Files:**
- Create: `crates/app/src/ui/drum_panel.rs`
- Modify: `crates/app/src/ui/mod.rs`

**Interfaces:**
- Produces:
  - `drum_panel::{INST_NAMES, cycle_hit}`
  - `SidAcidApp::drum_panel(&mut self, ui, kit: usize)`, which handles machine `2 + kit`.

- [ ] **Step 1: Write the failing test**

`ui/drum_panel.rs`, test first:
```rust
//! A drum machine panel: per-instrument knobs, kit knobs, pattern pads,
//! length, and the 8-row step grid (click cycles Off → On → Accent).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_cycle() {
        assert_eq!(cycle_hit(DrumHit::Off), DrumHit::On);
        assert_eq!(cycle_hit(DrumHit::On), DrumHit::Accent);
        assert_eq!(cycle_hit(DrumHit::Accent), DrumHit::Off);
    }
}
```
Add `mod drum_panel;` to `ui/mod.rs`.

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test -p sidacid drum_panel`
Expected: compile error.

- [ ] **Step 3: Implement the panel**

Insert above the test in `ui/drum_panel.rs`:
```rust
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::params::{DrumParam, KitParam, ParamId};
use acid_engine::project::{DrumHit, PatternRef};
use eframe::egui;

pub const INST_NAMES: [&str; 8] = ["BD", "SD", "LT", "HT", "CP", "RS", "CH", "OH"];
const KIT_NAMES: [&str; 2] = ["808", "909"];
const KNOB_ROWS: [(DrumParam, &str); 4] =
    [(DrumParam::Tune, "Tune"), (DrumParam::Decay, "Decay"), (DrumParam::Tone, "Tone"), (DrumParam::Level, "Level")];
const CELL: [f32; 2] = [24.0, 18.0];

pub fn cycle_hit(hit: DrumHit) -> DrumHit {
    match hit {
        DrumHit::Off => DrumHit::On,
        DrumHit::On => DrumHit::Accent,
        DrumHit::Accent => DrumHit::Off,
    }
}

impl SidAcidApp {
    pub(crate) fn drum_panel(&mut self, ui: &mut egui::Ui, kit: usize) {
        let machine = 2 + kit;
        let slot = self.doc.project.selected[machine];
        let title = format!("{} DRUMS   {}", KIT_NAMES[kit], PatternRef(slot).label());
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt(("drums", kit))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    egui::Grid::new(("drum_knobs", kit)).spacing([0.0, 0.0]).show(ui, |ui| {
                        ui.label("");
                        for name in INST_NAMES {
                            ui.label(egui::RichText::new(name).strong());
                        }
                        ui.end_row();
                        for (p, label) in KNOB_ROWS {
                            ui.label(label);
                            for inst in 0..8u8 {
                                self.param_knob(ui, ParamId::Drum(kit as u8, inst, p), "");
                            }
                            ui.end_row();
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        self.param_knob(ui, ParamId::Kit(kit as u8, KitParam::Volume), "Volume");
                        self.param_knob(ui, ParamId::Kit(kit as u8, KitParam::Accent), "Accent");
                    });
                });
                ui.horizontal(|ui| {
                    self.pattern_pads(ui, machine);
                    ui.separator();
                    let mut length = self.doc.project.drums[kit].patterns[slot as usize].length;
                    if ui.add(egui::DragValue::new(&mut length).range(1..=64).prefix("Len ")).changed() {
                        let cmds = self.doc.apply(Edit::DrumLength { kit, slot, length }, false);
                        self.send(cmds);
                    }
                });
                self.drum_grid(ui, kit, slot);
            });
    }

    fn drum_grid(&mut self, ui: &mut egui::Ui, kit: usize, slot: u8) {
        let machine = 2 + kit;
        let pattern = self.doc.project.drums[kit].patterns[slot as usize].clone();
        let len = pattern.length as usize;
        let playhead =
            (self.status.playing && self.status.patterns[machine] == slot).then_some(self.status.playhead[machine]);
        let mut edits = Vec::new();
        egui::ScrollArea::horizontal().id_salt(("drum_grid", kit)).show(ui, |ui| {
            egui::Grid::new(("drum_grid_cells", kit)).spacing([2.0, 2.0]).show(ui, |ui| {
                ui.label("");
                for i in 0..len {
                    let mut t = egui::RichText::new(format!("{}", i + 1)).size(10.0);
                    if Some(i) == playhead {
                        t = t.color(theme::ACID).strong();
                    }
                    ui.label(t);
                }
                ui.end_row();
                for (inst, name) in INST_NAMES.iter().enumerate() {
                    ui.label(*name);
                    for step in 0..len {
                        let hit = pattern.steps[step][inst];
                        let (text, colour) = match hit {
                            DrumHit::Off => ("·", ui.visuals().weak_text_color()),
                            DrumHit::On => ("■", theme::ACID),
                            DrumHit::Accent => ("■", theme::ACCENT),
                        };
                        let r = ui.add_sized(
                            CELL,
                            egui::Button::new(egui::RichText::new(text).color(colour)).selected(hit != DrumHit::Off),
                        );
                        if r.clicked() {
                            edits.push((step, inst, cycle_hit(hit)));
                        }
                        if r.secondary_clicked() {
                            edits.push((step, inst, DrumHit::Off));
                        }
                    }
                    ui.end_row();
                }
            });
        });
        for (step, inst, hit) in edits {
            let cmds = self.doc.apply(Edit::DrumHit { kit, slot, step, inst, hit }, false);
            self.send(cmds);
            self.view.focus = machine;
        }
    }
}
```

In `ui/mod.rs`, extend `rack`:
```rust
        for synth in 0..2 {
            self.synth_panel(ui, synth);
            ui.add_space(6.0);
        }
        for kit in 0..2 {
            self.drum_panel(ui, kit);
            ui.add_space(6.0);
        }
```

- [ ] **Step 4: Run the tests and smoke-test**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

Run: `cargo run --release -p sidacid` and check:
1. Clicking grid cells changes the beat live. Clicks cycle On, Accent, Off, and right-click clears a cell.
2. The drum knobs change each instrument.
3. Pattern pads and Length work.

Report what you saw.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "feat(app): 808/909 drum panels with knobs, pads and step grid"
```

---

### Task 10: Mixer, FX and master panel

**Files:**
- Create: `crates/app/src/ui/mixer_panel.rs`
- Modify: `crates/app/src/ui/mod.rs`

**Interfaces:**
- Produces:
  - `mixer_panel::{CHANNEL_NAMES, meter_fraction}`
  - `SidAcidApp::mixer_panel(&mut self, ui)`
- Meters come from `Status.meters`: channels 0–3, plus master at index 4.

- [ ] **Step 1: Write the failing test**

`ui/mixer_panel.rs`, test first:
```rust
//! Mixer strips, delay and reverb returns, master compressor, meters.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_uses_a_48_db_scale() {
        assert_eq!(meter_fraction(0.0), 0.0);
        assert_eq!(meter_fraction(1.0), 1.0);
        assert!((meter_fraction(0.5) - (1.0 - 6.02 / 48.0)).abs() < 0.01);
        assert_eq!(meter_fraction(0.001), 0.0);
        assert_eq!(meter_fraction(4.0), 1.0);
    }
}
```
Add `mod mixer_panel;` to `ui/mod.rs`.

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test -p sidacid mixer_panel`
Expected: compile error.

- [ ] **Step 3: Implement the panel**

Insert above the test in `ui/mixer_panel.rs`:
```rust
use super::{SidAcidApp, theme};
use acid_engine::params::{ChannelParam, FxParam, MasterParam, ParamId};
use eframe::egui::{self, Sense, vec2};

pub const CHANNEL_NAMES: [&str; 4] = ["Synth 1", "Synth 2", "808", "909"];
const METER_FLOOR_DB: f32 = -48.0;

const STRIP: [(ChannelParam, &str); 16] = [
    (ChannelParam::Level, "Level"),
    (ChannelParam::Pan, "Pan"),
    (ChannelParam::Mute, "Mute"),
    (ChannelParam::Solo, "Solo"),
    (ChannelParam::DistOn, "Dist"),
    (ChannelParam::DistDrive, "Drive"),
    (ChannelParam::DistTone, "Tone"),
    (ChannelParam::DistMix, "Mix"),
    (ChannelParam::CompOn, "Comp"),
    (ChannelParam::CompThreshold, "Thresh"),
    (ChannelParam::CompRatio, "Ratio"),
    (ChannelParam::CompAttack, "Attack"),
    (ChannelParam::CompRelease, "Release"),
    (ChannelParam::CompMakeup, "Makeup"),
    (ChannelParam::DelaySend, "Delay"),
    (ChannelParam::ReverbSend, "Reverb"),
];
const DELAY: [(FxParam, &str); 7] = [
    (FxParam::DelaySync, "Sync"),
    (FxParam::DelayDivision, "Time"),
    (FxParam::DelayMs, "ms"),
    (FxParam::DelayFeedback, "Fdbk"),
    (FxParam::DelayTone, "Tone"),
    (FxParam::DelayPingPong, "Ping"),
    (FxParam::DelayReturn, "Return"),
];
const REVERB: [(FxParam, &str); 4] = [
    (FxParam::ReverbSize, "Size"),
    (FxParam::ReverbDamping, "Damp"),
    (FxParam::ReverbWidth, "Width"),
    (FxParam::ReverbReturn, "Return"),
];
const MASTER: [(MasterParam, &str); 6] = [
    (MasterParam::CompOn, "Comp"),
    (MasterParam::CompThreshold, "Thresh"),
    (MasterParam::CompRatio, "Ratio"),
    (MasterParam::CompAttack, "Attack"),
    (MasterParam::CompRelease, "Release"),
    (MasterParam::CompMakeup, "Makeup"),
];

/// Peak (linear) to meter height 0..1 on a -48..0 dB scale.
pub fn meter_fraction(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0)
}

fn meter(ui: &mut egui::Ui, peak: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 80.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, theme::GRID_DARK);
    let f = meter_fraction(peak);
    let mut lit = rect;
    lit.set_top(rect.bottom() - rect.height() * f);
    painter.rect_filled(lit, 1.0, if peak >= 0.99 { theme::ACCENT } else { theme::ACID });
}

impl SidAcidApp {
    pub(crate) fn mixer_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(egui::RichText::new("MIXER & FX").strong())
            .id_salt("mixer")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for ch in 0..4u8 {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong(CHANNEL_NAMES[ch as usize]);
                                meter(ui, self.status.meters[ch as usize]);
                            });
                            egui::Grid::new(("strip", ch)).spacing([0.0, 0.0]).show(ui, |ui| {
                                for (i, (p, label)) in STRIP.iter().enumerate() {
                                    self.param_knob(ui, ParamId::Channel(ch, *p), label);
                                    if i % 4 == 3 {
                                        ui.end_row();
                                    }
                                }
                            });
                        });
                        ui.separator();
                    }
                    ui.vertical(|ui| {
                        ui.strong("Delay");
                        egui::Grid::new("delay").spacing([0.0, 0.0]).show(ui, |ui| {
                            for (i, (p, label)) in DELAY.iter().enumerate() {
                                self.param_knob(ui, ParamId::Fx(*p), label);
                                if i % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                        ui.strong("Reverb");
                        egui::Grid::new("reverb").spacing([0.0, 0.0]).show(ui, |ui| {
                            for (i, (p, label)) in REVERB.iter().enumerate() {
                                self.param_knob(ui, ParamId::Fx(*p), label);
                                if i % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong("Master");
                            meter(ui, self.status.meters[4]);
                        });
                        egui::Grid::new("master").spacing([0.0, 0.0]).show(ui, |ui| {
                            for (i, (p, label)) in MASTER.iter().enumerate() {
                                self.param_knob(ui, ParamId::Master(*p), label);
                                if i % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                        if self.status.guard_fired {
                            ui.colored_label(theme::ACCENT, "Audio fault recovered");
                        }
                    });
                });
            });
    }
}
```

In `ui/mod.rs`, add `self.mixer_panel(ui);` at the end of `rack`.

`Status.guard_fired` only reports one status window. Keep the "recovered" label up for 3 seconds:
1. Add the field `pub fault_until: f64,` to `ViewState`.
2. In `pump()`, when a status with `guard_fired` arrives, set `self.view.fault_until = ctx_time + 3.0`. To have the time available, change `pump(&mut self)` to `pump(&mut self, now: f64)`, and in `ui()` call `self.pump(ctx.input(|i| i.time));`.
3. In `mixer_panel`, show the label when `ui.input(|i| i.time) < self.view.fault_until` instead of reading `self.status.guard_fired`.

- [ ] **Step 4: Run the tests and smoke-test**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

Run: `cargo run --release -p sidacid` and check that each of these is audible live:
- the meters move
- Mute and Solo
- Dist on Synth 1
- delay and reverb sends
- the master compressor

Report what you saw.

- [ ] **Step 5: Commit**

```bash
git add crates/app
git commit -m "feat(app): mixer strips, delay/reverb, master section and meters"
```

---

### Task 11: Files, clipboard, shortcuts and the unsaved-changes guard

**Files:**
- Create: `crates/app/src/ui/files.rs`, `crates/app/src/ui/keys.rs`
- Modify: `crates/app/src/ui/mod.rs`, `crates/app/src/ui/transport.rs`

**Interfaces:**
- Produces:
  - `keys::{Shortcut, shortcut}`
  - `files::{Pending, Clip}`
  - `SidAcidApp::{file_menu, request, perform, load_document, save, copy_pattern, paste_pattern, dialogs, window_title, handle_shortcuts, start_export, poll_export}`
  - New `ViewState` fields: `confirm: Option<Pending>`, `quit: bool`, `clipboard: Option<Clip>`, `export_open: bool`, `export_bars: u32`, `export_rx: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>` and `last_title: String`.

- [ ] **Step 1: Write the failing tests**

`ui/keys.rs`, tests first:
```rust
//! Global keyboard shortcuts.

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Key, Modifiers};

    #[test]
    fn command_shortcuts() {
        let cmd = Modifiers::COMMAND;
        assert_eq!(shortcut(Key::S, cmd, false), Some(Shortcut::Save));
        assert_eq!(shortcut(Key::O, cmd, false), Some(Shortcut::Open));
        assert_eq!(shortcut(Key::N, cmd, false), Some(Shortcut::New));
        assert_eq!(shortcut(Key::Z, cmd, false), Some(Shortcut::Undo));
        assert_eq!(shortcut(Key::Z, cmd | Modifiers::SHIFT, false), Some(Shortcut::Redo));
        assert_eq!(shortcut(Key::Y, cmd, false), Some(Shortcut::Redo));
        assert_eq!(shortcut(Key::C, cmd, true), Some(Shortcut::Copy));
        assert_eq!(shortcut(Key::V, cmd, true), Some(Shortcut::Paste));
    }

    #[test]
    fn space_plays_unless_step_writing() {
        assert_eq!(shortcut(Key::Space, Modifiers::NONE, false), Some(Shortcut::PlayStop));
        assert_eq!(shortcut(Key::Space, Modifiers::NONE, true), None);
        assert_eq!(shortcut(Key::Z, Modifiers::NONE, false), None);
    }
}
```

Add these to the `tests` module in `ui/mod.rs`:
```rust
    #[test]
    fn copy_paste_between_synths_is_undoable() {
        use acid_engine::project::Gate;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.focus = 0;
        app.copy_pattern();
        app.view.focus = 1;
        app.doc.project.selected[1] = 9;
        app.paste_pattern();
        assert_eq!(app.doc.project.synths[1].patterns[9], app.doc.project.synths[0].patterns[0]);
        let cmds = app.doc.undo();
        app.send(cmds);
        assert!(app.doc.project.synths[1].patterns[9].steps.iter().all(|s| s.gate == Gate::Off));
    }

    #[test]
    fn pasting_drums_onto_a_synth_is_refused() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.focus = 2;
        app.copy_pattern();
        app.view.focus = 0;
        let before = app.doc.project.clone();
        app.paste_pattern();
        assert_eq!(app.doc.project, before);
        assert!(app.view.message.is_some());
    }

    #[test]
    fn title_shows_name_and_dirty_mark() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        assert_eq!(app.window_title(), "Demo — SidAcid");
        app.doc.dirty = true;
        app.doc.path = Some(std::path::PathBuf::from("/tmp/acid line.sidacid"));
        assert_eq!(app.window_title(), "acid line* — SidAcid");
    }

    #[test]
    fn new_with_unsaved_changes_asks_first() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.doc.dirty = true;
        app.request(files::Pending::New);
        assert!(app.view.confirm.is_some());
        assert_eq!(app.doc.project.name, "Demo");
        app.view.confirm = None;
        app.doc.dirty = false;
        app.request(files::Pending::New);
        assert_eq!(app.doc.project.name, "Untitled");
    }
```

- [ ] **Step 2: Run the tests to confirm they fail**

Add `mod files;` and `mod keys;` to `ui/mod.rs`, then run `cargo test -p sidacid`.
Expected: compile errors.

- [ ] **Step 3: Implement shortcuts**

Insert above the tests in `ui/keys.rs`:
```rust
use super::SidAcidApp;
use super::files::Pending;
use acid_engine::host::Command;
use eframe::egui::{self, Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    PlayStop,
    Save,
    Open,
    New,
    Undo,
    Redo,
    Copy,
    Paste,
}

pub fn shortcut(key: Key, modifiers: Modifiers, step_writing: bool) -> Option<Shortcut> {
    if modifiers.command {
        return match key {
            Key::S => Some(Shortcut::Save),
            Key::O => Some(Shortcut::Open),
            Key::N => Some(Shortcut::New),
            Key::Z if modifiers.shift => Some(Shortcut::Redo),
            Key::Z => Some(Shortcut::Undo),
            Key::Y => Some(Shortcut::Redo),
            Key::C => Some(Shortcut::Copy),
            Key::V => Some(Shortcut::Paste),
            _ => None,
        };
    }
    (key == Key::Space && !step_writing).then_some(Shortcut::PlayStop)
}

impl SidAcidApp {
    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let step_writing = self.view.writers.iter().any(Option::is_some);
        let pressed: Vec<Shortcut> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } => shortcut(*key, *modifiers, step_writing),
                    // Some platforms deliver Ctrl+C / Ctrl+V as clipboard events.
                    egui::Event::Copy => Some(Shortcut::Copy),
                    egui::Event::Paste(_) => Some(Shortcut::Paste),
                    _ => None,
                })
                .collect()
        });
        for s in pressed {
            match s {
                Shortcut::PlayStop => {
                    let cmd = if self.status.playing { Command::Stop } else { Command::Play };
                    self.send(vec![cmd]);
                }
                Shortcut::Save => {
                    self.save(false);
                }
                Shortcut::Open => self.request(Pending::Open),
                Shortcut::New => self.request(Pending::New),
                Shortcut::Undo => {
                    let cmds = self.doc.undo();
                    self.send(cmds);
                }
                Shortcut::Redo => {
                    let cmds = self.doc.redo();
                    self.send(cmds);
                }
                Shortcut::Copy => self.copy_pattern(),
                Shortcut::Paste => self.paste_pattern(),
            }
        }
    }
}
```

- [ ] **Step 4: Implement files, clipboard, export and dialogs**

`ui/files.rs`:
```rust
//! New / Open / Save / Save As, loop export, the unsaved-changes guard,
//! pattern copy & paste and the window title.

use super::SidAcidApp;
use crate::document::{Document, Edit};
use acid_engine::project::{DrumPattern, Project, SynthPattern};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc;

const EXTENSION: &str = "sidacid";
const DEFAULT_EXPORT_BARS: u32 = 8;

/// An action waiting on the unsaved-changes question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    New,
    Open,
    Quit,
}

pub enum Clip {
    Synth(SynthPattern),
    Drum(DrumPattern),
}

fn project_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("SidAcid project", &[EXTENSION])
}

impl SidAcidApp {
    pub(crate) fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            if ui.button("New").clicked() {
                ui.close_menu();
                self.request(Pending::New);
            }
            if ui.button("Open…").clicked() {
                ui.close_menu();
                self.request(Pending::Open);
            }
            if ui.button("Save").clicked() {
                ui.close_menu();
                self.save(false);
            }
            if ui.button("Save As…").clicked() {
                ui.close_menu();
                self.save(true);
            }
            ui.separator();
            if ui.button("Export loop to WAV…").clicked() {
                ui.close_menu();
                if self.view.export_bars == 0 {
                    self.view.export_bars = DEFAULT_EXPORT_BARS;
                }
                self.view.export_open = true;
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                ui.close_menu();
                self.request(Pending::Quit);
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui.add_enabled(self.doc.can_undo(), egui::Button::new("Undo  (Ctrl+Z)")).clicked() {
                ui.close_menu();
                let cmds = self.doc.undo();
                self.send(cmds);
            }
            if ui.add_enabled(self.doc.can_redo(), egui::Button::new("Redo  (Ctrl+Y)")).clicked() {
                ui.close_menu();
                let cmds = self.doc.redo();
                self.send(cmds);
            }
            ui.separator();
            if ui.button("Copy pattern  (Ctrl+C)").clicked() {
                ui.close_menu();
                self.copy_pattern();
            }
            if ui.button("Paste pattern  (Ctrl+V)").clicked() {
                ui.close_menu();
                self.paste_pattern();
            }
        });
    }

    /// Runs `action` now, or asks first if there are unsaved changes.
    pub(crate) fn request(&mut self, action: Pending) {
        if self.doc.dirty {
            self.view.confirm = Some(action);
        } else {
            self.perform(action);
        }
    }

    pub(crate) fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => self.load_document(Document::new(Project::default())),
            Pending::Open => {
                if let Some(path) = project_dialog().pick_file() {
                    match Project::load(&path) {
                        Ok(project) => {
                            let mut doc = Document::new(project);
                            doc.path = Some(path);
                            self.load_document(doc);
                        }
                        Err(e) => self.view.message = Some(format!("Could not open {}:\n{e}", path.display())),
                    }
                }
            }
            Pending::Quit => self.view.quit = true,
        }
    }

    pub(crate) fn load_document(&mut self, doc: Document) {
        self.doc = doc;
        self.view.writers = [None; 2];
        self.view.roll_drag = [None; 2];
        self.pending.clear();
        if let Some(audio) = self.audio.as_mut()
            && audio.load_project(&self.doc.project).is_err()
        {
            self.view.message = Some("The audio engine is busy; the project will play after Retry audio.".into());
            self.audio = None;
            self.audio_error = Some("engine busy while loading".into());
        }
    }

    /// Saves to the current path (or asks for one). Returns true on success.
    pub(crate) fn save(&mut self, save_as: bool) -> bool {
        let path = if save_as || self.doc.path.is_none() {
            project_dialog().set_file_name(format!("{}.{EXTENSION}", self.doc.project.name)).save_file()
        } else {
            self.doc.path.clone()
        };
        let Some(mut path) = path else { return false };
        if path.extension().is_none() {
            path.set_extension(EXTENSION);
        }
        match self.doc.project.save(&path) {
            Ok(()) => {
                self.doc.path = Some(path);
                self.doc.dirty = false;
                true
            }
            Err(e) => {
                self.view.message = Some(format!("Could not save {}:\n{e}", path.display()));
                false
            }
        }
    }

    pub(crate) fn copy_pattern(&mut self) {
        let m = self.view.focus;
        let slot = self.doc.project.selected[m] as usize;
        self.view.clipboard = Some(if m < 2 {
            Clip::Synth(self.doc.project.synths[m].patterns[slot].clone())
        } else {
            Clip::Drum(self.doc.project.drums[m - 2].patterns[slot].clone())
        });
    }

    pub(crate) fn paste_pattern(&mut self) {
        let m = self.view.focus;
        let slot = self.doc.project.selected[m];
        let edit = match (&self.view.clipboard, m < 2) {
            (Some(Clip::Synth(p)), true) => Edit::SynthPattern { synth: m, slot, pattern: Box::new(p.clone()) },
            (Some(Clip::Drum(p)), false) => Edit::DrumPattern { kit: m - 2, slot, pattern: Box::new(p.clone()) },
            (None, _) => return,
            _ => {
                self.view.message = Some("The clipboard holds a different kind of pattern.".into());
                return;
            }
        };
        let cmds = self.doc.apply(edit, false);
        self.send(cmds);
    }

    pub(crate) fn window_title(&self) -> String {
        let name = self
            .doc
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.doc.project.name.clone());
        format!("{name}{} — SidAcid", if self.doc.dirty { "*" } else { "" })
    }

    pub(crate) fn start_export(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("WAV audio", &["wav"])
            .set_file_name(format!("{}-loop.wav", self.doc.project.name))
            .save_file()
        else {
            return;
        };
        let project = self.doc.project.clone();
        let bars = self.view.export_bars.clamp(1, 64);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = acid_engine::render::render_patterns_wav(&project, bars, &path)
                .map(|()| path)
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
        self.view.export_rx = Some(rx);
        self.view.message = Some(format!("Exporting {bars} bars…"));
    }

    pub(crate) fn poll_export(&mut self) {
        let Some(rx) = &self.view.export_rx else { return };
        if let Ok(result) = rx.try_recv() {
            self.view.export_rx = None;
            self.view.message = Some(match result {
                Ok(path) => format!("Exported {}", path.display()),
                Err(e) => format!("Export failed: {e}"),
            });
        }
    }

    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.view.confirm {
            let name = self.window_title().trim_end_matches(" — SidAcid").trim_end_matches('*').to_string();
            egui::Window::new("Unsaved changes").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(format!("Save changes to \"{name}\" first?"));
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() && self.save(false) {
                        self.view.confirm = None;
                        self.perform(action);
                    }
                    if ui.button("Don't save").clicked() {
                        self.view.confirm = None;
                        self.doc.dirty = false;
                        self.perform(action);
                    }
                    if ui.button("Cancel").clicked() {
                        self.view.confirm = None;
                    }
                });
            });
        }
        if self.view.export_open {
            let mut open = true;
            egui::Window::new("Export loop").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
                ui.label("Renders the selected pattern of every machine.");
                ui.add(egui::DragValue::new(&mut self.view.export_bars).range(1..=64).suffix(" bars"));
                if ui.button("Export…").clicked() {
                    self.view.export_open = false;
                    self.start_export();
                }
            });
            if !open {
                self.view.export_open = false;
            }
        }
        if let Some(msg) = self.view.message.clone() {
            egui::Window::new("SidAcid").collapsible(false).resizable(false).show(ctx, |ui| {
                ui.label(msg);
                if ui.button("OK").clicked() {
                    self.view.message = None;
                }
            });
        }
    }
}

```

In `ui/transport.rs`, make the first item of the horizontal row `self.file_menu(ui); ui.separator();`, placed before the "SidAcid" heading.

In `ui/mod.rs`:
1. Add these fields to `ViewState`:
   ```rust
       pub confirm: Option<files::Pending>,
       pub quit: bool,
       pub clipboard: Option<files::Clip>,
       pub export_open: bool,
       pub export_bars: u32,
       pub export_rx: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
       pub last_title: String,
   ```
2. In `ui()`, after `self.handle_step_write(ctx);`, add:
   ```rust
           self.handle_shortcuts(ctx);
           self.poll_export();
           if ctx.input(|i| i.viewport().close_requested()) && self.doc.dirty && !self.view.quit {
               ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
               self.view.confirm = Some(files::Pending::Quit);
           }
           if self.view.quit {
               ctx.send_viewport_cmd(egui::ViewportCommand::Close);
           }
           let title = self.window_title();
           if title != self.view.last_title {
               ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
               self.view.last_title = title;
           }
   ```
3. At the end of `ui()`, before the repaint request, add `self.dialogs(ctx);`.
4. Remove any `#[allow(dead_code)]` item attributes left from Task 6 whose items are now used: `can_undo`, `can_redo`, `load_project` and so on. If `AudioLink::device_name` is still unused, show it as the hover text on the CPU label in `transport.rs`, using `.on_hover_text(...)` with the device name when audio is running.

- [ ] **Step 5: Run the tests and smoke-test**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --check`.
Expected: all clean.

Run: `cargo run --release -p sidacid` and check:
1. Space plays and stops.
2. Ctrl+Z / Ctrl+Y undo and redo knob drags, step edits and piano-roll drags.
3. Ctrl+C on Synth 1, click a Synth 2 pad, then Ctrl+V pastes the pattern there.
4. File → Save As writes a `.sidacid` file, the title loses its `*`, and File → Open loads it back.
5. Editing and then choosing File → New asks first.
6. Closing the window with unsaved changes asks first.
7. File → Export loop writes a WAV that plays.

Report what you saw.

- [ ] **Step 6: Commit**

```bash
git add crates/app
git commit -m "feat(app): file menu, save/open/export, clipboard, shortcuts and unsaved guard"
```

---

## Spec coverage (Plan 2)

| Spec section | Covered by |
|---|---|
| §2 UI owns project; rtrb command/event rings; real-time audio thread; boxes returned for disposal | Tasks 2, 3, 5, 6 |
| §8 Pattern mode playback, queued switch shown on pads | Tasks 3, 7 (engine from Plan 1) |
| §9 303 view, step-write, piano roll, lossless switching, param lane playback semantics | Tasks 1, 7, 8 |
| §11 Undo/redo (200 levels, gestures), pattern copy/paste | Tasks 4, 11 |
| §12 Export (loop of N bars) | Task 11 (song export: Plan 3) |
| §13 UI layout: transport, synth/drum panels, mixer, collapsing panels, knobs, keys, playheads, CPU | Tasks 6–11 |
| §14 No audio device banner and retry; stream error; command ring full (coalesce + retry); bad project file message; NaN guard indicator | Tasks 5, 6, 10, 11 |
| §7 Project files (atomic save, version check) | Plan 1 + Task 11 |
| §10 Song mode, §11 randomise, song export | **Plan 3** |
