# Modular Rack (b): Rack Management UI Implementation Plan (Plan 11)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user add, remove, rename, reorder and duplicate machines (up to 16, any mix of 303 / 808 / 909 / FM / Sampler), all undoable, while playing, with the engine following through allocation-free `SetRack` swaps.

**Architecture:** Rack operations live on `Project` in the engine crate (pure, tested). The app's `Document` gains four undoable edits built on them. After every document change (edit, undo, redo, load) the app compares the project's rack order with the order it last sent to the engine and, if different, sends one `SetRack` built by `RackSwap::build`. A host-level rack generation that survives `ReplaceEngine` lets the UI ignore statuses from before its latest rack. A shared panel-header helper adds ↑ ↓ ⧉ ✕ and inline rename to every machine panel, and an "Add machine" bar sits under the last panel.

**Tech Stack:** Rust 2024 workspace (`crates/acid-engine`, `crates/app` = package `sidacid`), egui/eframe 0.31, rtrb.

**Spec:** `docs/superpowers/specs/2026-10-14-modular-rack-design.md` (section 3, plus section 1 "Removing a machine" and section 2 "Status"). Hazards carried from plan 10: `docs/superpowers/plans/2026-10-14-sidacid-plan10-deferred.txt` ("CARRIED TO PLAN 10b" and "FROM THE FINAL REVIEW").

## Global Constraints

- Rack cap `MAX_MACHINES = 16`. Types: 303 Synth, 808 Kit, 909 Kit, FM, Sampler. Add-menu labels from `MachineType::label()`: "303 Synth", "808 Kit", "909 Kit", "FM", "Sampler".
- New machines: next uid from `Project::next_uid` (then `next_uid += 1`; uids never reused, also not after undo), type's default sound, empty patterns, name `default_name(ty, &existing_names)`, appended at the end of the rack.
- Duplicate: deep copy of the machine (params, patterns, sound/bank, sampler audio), new uid, name `"<name> copy"`, inserted directly below the source; song cells and automation lanes are **not** copied.
- Remove: immediate, no confirmation; status line `Removed '<name>' — Ctrl+Z to undo`. Removal also deletes the machine's song cells, its song automation lanes, and every 303 pattern `param_lane` (on any synth) whose `ParamId` names the removed uid — all restored by undo.
- Rename: double-click the name, type, Enter commits; empty name → `default_name` for its type; Escape cancels.
- Move: ↑ / ↓ one place; disabled at the ends.
- At 16 machines Add and ⧉ are disabled with tooltip `Rack full (16)`. Add bar shows `n / 16`.
- Rack edits are undoable in the same linear history as all other edits; undo/redo must never apply an edit to a machine that does not exist at that point (test it).
- No allocation/locking on the audio thread (`crates/acid-engine/tests/no_alloc.rs` passes); golden render test (`crates/acid-engine/tests/golden.rs`) passes unchanged.
- Panel header text for unchanged racks stays byte-identical to today (`"{NAME}   {pattern}"`, `"{NAME} DRUMS   {pattern}"`, FM adds `"   {voice}"`), with NAME = machine name upper-cased.
- Tests: `cargo test --workspace`; `cargo clippy --workspace --all-targets` zero warnings; `cargo fmt --all`.
- Commit trailer: blank line then `Co-Authored-By: <model you are> <noreply@anthropic.com>`.
- Branch `rack-ui` from master (created by the controller before Task 1).

---

### Task 1: Engine groundwork for live rack edits

**Files:** `crates/acid-engine/src/host.rs`, `engine.rs`, `sequencer.rs`, `project.rs`, tests in those files and `tests/no_alloc.rs`.

**Interfaces (produces):**
- `Status::rack_gen` now comes from a counter on `EngineHost` (not the engine): `+1` on every accepted `SetRack` **and** every `ReplaceEngine`; wrapping. The engine's own `rack_gen` field/getter is removed (or kept private for tests only).
- `Status::rack_rejected: bool` — a `SetRack` was refused since the previous status (like `song_rejected`; cleared when a status is sent).
- `Engine::load_project` becomes `pub(crate)`.
- New slots join in phase: in `Engine::set_rack`, a fresh slot's track starts at the position the other tracks are at — pattern mode: step = global step counter modulo the new pattern's length (the same rule the sequencer uses when a pattern starts mid-flow; read `Sequencer` for an existing helper such as the one used by `resync`/queued switches and reuse it); song mode: the slot plays the current bar's cell from the current step (as `resync` does for all tracks). Surviving slots are untouched.
- `Project::validate` rejects a bar with two cells for the same uid (`"song bar names a machine twice"`).

- [ ] **Step 1: Failing tests.**

```rust
    // host.rs tests
    #[test]
    fn rack_generation_counts_swaps_and_engine_replacements() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let last_gen = |rx: &mut rtrb::Consumer<Event>| {
            std::iter::from_fn(|| rx.pop().ok())
                .filter_map(|e| match e { Event::Status(s) => Some(s.rack_gen), _ => None })
                .last()
        };
        run(&mut host, 2048);
        assert_eq!(last_gen(&mut rx), Some(0));
        let p = crate::demo::demo_project();
        tx.push(Command::ReplaceEngine(Box::new(Engine::from_project(SR, &p)))).unwrap();
        run(&mut host, 2048);
        assert_eq!(last_gen(&mut rx), Some(1), "a new engine is a new rack");
        let mut q = p.clone();
        q.machines.pop();
        let swap = crate::rack::RackSwap::build(&q, &host.engine().slot_uids(), SR);
        tx.push(Command::SetRack(swap)).unwrap();
        run(&mut host, 2048);
        assert_eq!(last_gen(&mut rx), Some(2));
    }

    #[test]
    fn a_refused_rack_swap_is_reported_once() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let mut p = crate::demo::demo_project();
        let extra = crate::project::MachineEntry::new(
            crate::machine::MachineUid(7), crate::machine::MachineType::Fm, "FM 2".into());
        p.machines.push(extra);
        p.next_uid = 8;
        let mut swap = crate::rack::RackSwap::build(&p, &host.engine().slot_uids(), SR);
        // Malformed: uid 7 is neither a fresh slot nor in the engine.
        swap.fresh[6] = None;
        tx.push(Command::SetRack(swap)).unwrap();
        run(&mut host, 2048);
        let flags: Vec<bool> = std::iter::from_fn(|| rx.pop().ok())
            .filter_map(|e| match e { Event::Status(s) => Some(s.rack_rejected), _ => None })
            .collect();
        assert_eq!(flags.iter().filter(|f| **f).count(), 1);
    }

    // engine.rs tests
    #[test]
    fn a_machine_added_while_playing_joins_in_phase() {
        let p = crate::demo::demo_project();
        let mut eng = Engine::from_project(48000.0, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 48000], vec![0.0; 48000]); // ~1 s: well past step 0
        eng.process(&mut l, &mut r);
        let mut q = p.clone();
        let mut e = MachineEntry::new(MachineUid(7), MachineType::Synth, "Synth 3".into());
        if let MachineBody::Synth(s) = &mut e.body {
            s.patterns[0] = q.synth(MachineUid(1)).unwrap().patterns[0].clone();
        }
        q.machines.push(e);
        q.next_uid = 8;
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0);
        eng.set_rack(&mut swap);
        assert_eq!(eng.playhead()[6], eng.playhead()[0], "same pattern length, same step");
    }

    #[test]
    fn an_empty_rack_renders_silence() {
        let mut p = Project::default();
        p.machines.clear();
        p.validate().unwrap();
        let (l, r) = crate::render::render_patterns(&p, 1);
        assert!(!l.is_empty() && l.iter().chain(&r).all(|s| s.abs() < 1e-6));
    }

    // project.rs tests
    #[test]
    fn a_bar_may_not_name_a_machine_twice() {
        let mut p = Project::default();
        p.song.bars.push(Bar { cells: vec![(MachineUid(1), Cell::default()), (MachineUid(1), Cell::default())] });
        assert!(p.validate().unwrap_err().to_string().contains("twice"));
    }
```

(Adjust the in-phase test's sample count / assertion only if the demo's patterns make `playhead()[0]` and a 16-step copy differ for a reason unrelated to phase — e.g. different lengths; keep the intent: the new track's step equals an existing same-length track's step.)

- [ ] **Step 2:** run → fail. **Step 3:** implement. **Step 4:** full checks (golden unchanged, no_alloc). **Step 5:** commit `feat(engine): rack generation survives engine swaps; refused swaps reported; new machines join in phase`.

---

### Task 2: Rack operations on Project

**Files:** `crates/acid-engine/src/project.rs` (or a new `crates/acid-engine/src/rack_edit.rs` re-exported from `project`), tests.

**Interfaces (produces):**
```rust
/// Everything removing a machine takes out of the project, so undo can put it back exactly.
#[derive(Clone, Debug, PartialEq)]
pub struct RemovedMachine {
    pub index: usize,                                  // its rack position
    pub entry: MachineEntry,
    pub cells: Vec<(usize, Cell)>,                     // (bar index, its cell)
    pub lanes: Vec<(usize, AutoLane)>,                 // (index in song.automation, lane)
    pub param_lanes: Vec<(MachineUid, u8, ParamLane)>, // (synth uid, pattern index, lane) that targeted it
}
impl Project {
    /// A fresh machine of `ty` with the next uid and a default name; does not insert it.
    pub fn new_machine(&mut self, ty: MachineType) -> MachineEntry;          // bumps next_uid
    /// Deep copy of `uid` with a new uid and "<name> copy"; does not insert it. None if absent.
    pub fn duplicate_machine(&mut self, uid: MachineUid) -> Option<MachineEntry>; // bumps next_uid
    /// Inserts at `index` (clamped to len); None if the rack is full or the uid is taken.
    pub fn insert_machine(&mut self, index: usize, entry: MachineEntry) -> Option<()>;
    pub fn remove_machine(&mut self, uid: MachineUid) -> Option<RemovedMachine>;
    /// Puts a removed machine back exactly (entry at its index, cells, lanes at their indices, param lanes).
    pub fn restore_machine(&mut self, removed: RemovedMachine);
    /// Moves to `to` (clamped); returns the old index.
    pub fn move_machine(&mut self, uid: MachineUid, to: usize) -> Option<usize>;
    /// Sets the name; an empty or all-whitespace name becomes the type's default name. Returns the old name.
    pub fn rename_machine(&mut self, uid: MachineUid, name: &str) -> Option<String>;
}
```
- `default_name` takes the other machines' names (excluding the renamed machine itself).

- [ ] **Step 1: Failing tests:**

```rust
    #[test]
    fn new_machines_take_fresh_uids_and_default_names() {
        let mut p = Project::default();
        let a = p.new_machine(MachineType::Kit909);
        assert_eq!((a.uid, a.name.as_str()), (MachineUid(7), "909 2"));
        p.insert_machine(p.machines.len(), a).unwrap();
        let b = p.new_machine(MachineType::Synth);
        assert_eq!((b.uid, b.name.as_str()), (MachineUid(8), "Synth 3"));
        assert_eq!(p.next_uid, 9);
        p.validate().unwrap();
    }

    #[test]
    fn the_rack_refuses_a_seventeenth_machine_and_a_taken_uid() {
        let mut p = Project::default();
        while p.machines.len() < MAX_MACHINES {
            let m = p.new_machine(MachineType::Fm);
            p.insert_machine(p.machines.len(), m).unwrap();
        }
        let extra = p.new_machine(MachineType::Fm);
        assert!(p.insert_machine(0, extra).is_none());
        let mut q = Project::default();
        let dup = q.machines[0].clone();
        assert!(q.insert_machine(0, dup).is_none());
    }

    #[test]
    fn removing_a_machine_takes_its_cells_lanes_and_foreign_param_lanes_and_restore_undoes_it() {
        let mut p = crate::demo::demo_project();
        // Synth 1's pattern 0 has a param lane on the 909's Level; the song names the 909; automation too.
        let target = ParamId::Machine(MachineUid(4), MachineParam::Kit(KitParam::Volume));
        p.synth_mut(MachineUid(1)).unwrap().patterns[0].param_lane =
            Some(ParamLane { param: target, values: vec![Some(0.5); 16] });
        let mut bar = Bar::default();
        *bar.cell_mut(MachineUid(4)) = Cell { pattern: Some(PatternRef(1)), mute: false, transpose: 0 };
        *bar.cell_mut(MachineUid(1)) = Cell { pattern: Some(PatternRef(0)), mute: false, transpose: 0 };
        p.song.bars.push(bar);
        p.song.automation.push(AutoLane { param: target, points: [(0u32, 0.3f32)].into_iter().collect() });
        p.validate().unwrap();
        let before = p.clone();
        let removed = p.remove_machine(MachineUid(4)).unwrap();
        assert_eq!(removed.index, 3);
        assert!(p.machine(MachineUid(4)).is_none());
        assert_eq!(p.song.bars[0].cells.len(), 1);
        assert!(p.song.automation.is_empty());
        assert!(p.synth(MachineUid(1)).unwrap().patterns[0].param_lane.is_none());
        p.validate().unwrap();
        p.restore_machine(removed);
        assert_eq!(p, before);
    }

    #[test]
    fn duplicates_copy_sound_and_patterns_but_not_song_cells() {
        let mut p = crate::demo::demo_project();
        let d = p.duplicate_machine(MachineUid(1)).unwrap();
        assert_eq!(d.name, "Synth 1 copy");
        assert_ne!(d.uid, MachineUid(1));
        assert_eq!(d.body, p.machine(MachineUid(1)).unwrap().body);
        assert_eq!(d.params, p.machine(MachineUid(1)).unwrap().params);
        p.insert_machine(1, d.clone()).unwrap();
        assert!(p.song.bars.iter().all(|b| b.cells.iter().all(|(u, _)| *u != d.uid)));
        p.validate().unwrap();
    }

    #[test]
    fn moving_and_renaming() {
        let mut p = Project::default();
        assert_eq!(p.move_machine(MachineUid(1), 99), Some(0));
        assert_eq!(p.rack_uids().last(), Some(&MachineUid(1)));
        assert_eq!(p.rename_machine(MachineUid(3), "Boom"), Some("808".to_string()));
        assert_eq!(p.machine(MachineUid(3)).unwrap().name, "Boom");
        p.rename_machine(MachineUid(3), "  ");
        assert_eq!(p.machine(MachineUid(3)).unwrap().name, "808");
    }
```

(Use whichever pattern length the `ParamLane` needs — read `ParamLane`/`SynthPattern::is_well_formed`.)

- [ ] **Steps 2–5:** fail → implement → full checks → commit `feat(engine): rack operations on Project — add, duplicate, remove/restore, move, rename`.

---

### Task 3: Undoable rack edits in the Document

**Files:** `crates/app/src/document.rs`, tests.

**Interfaces (produces):**
```rust
// Edit gains:
InsertMachine { removed: Box<RemovedMachine> },   // inverse of Remove; also used for Add/Duplicate (empty cells/lanes)
RemoveMachine { uid: MachineUid },
MoveMachine { uid: MachineUid, to: usize },
RenameMachine { uid: MachineUid, name: String },
// MergeKey: a variant that never merges (rack edits are always their own undo step).
impl Document {
    pub fn add_machine(&mut self, ty: MachineType) -> Option<(MachineUid, Vec<Command>)>;      // None at 16
    pub fn duplicate_machine(&mut self, uid: MachineUid) -> Option<(MachineUid, Vec<Command>)>; // None at 16 or absent
    pub fn remove_machine(&mut self, uid: MachineUid) -> Option<(String /*name*/, Vec<Command>)>;
    pub fn move_machine(&mut self, uid: MachineUid, to: usize) -> Vec<Command>;
    pub fn rename_machine(&mut self, uid: MachineUid, name: &str) -> Vec<Command>;
}
```
- `apply_raw`: `RemoveMachine{uid}` → `project.remove_machine(uid)` → returns `InsertMachine{removed}`; `InsertMachine{removed}` → `project.restore_machine(*removed)` → returns `RemoveMachine{uid}`; Move/Rename return their inverse with the old index/name. Add = apply `InsertMachine` with `RemovedMachine { index: len, entry: project.new_machine(ty), cells: [], lanes: [], param_lanes: [] }`; Duplicate likewise with `duplicate_machine` at `index = source + 1`.
- `commands_for` returns **no commands** for the four rack edits (the app sends one `SetRack` after any rack change — Task 4). A `Group` containing a rack edit is not produced by this plan.
- `is_no_op`: Move to the same index, Rename to the same resulting name → true.
- Robustness: every `.expect(NO_…)` in `apply_raw`, `is_no_op`, `commands_for` stays (the linear history guarantees existence); add the tests below proving it.

- [ ] **Step 1: Failing tests:**

```rust
    #[test]
    fn rack_edits_undo_and_redo() {
        let mut d = Document::new(acid_engine::demo::demo_project());
        let start = d.project.clone();
        let (uid, _) = d.add_machine(MachineType::Sampler).unwrap();
        d.rename_machine(uid, "Breaks");
        d.move_machine(uid, 0);
        let (copy, _) = d.duplicate_machine(MachineUid(1)).unwrap();
        let (name, _) = d.remove_machine(MachineUid(4)).unwrap();
        assert_eq!(name, "909");
        let end = d.project.clone();
        for _ in 0..5 { d.undo(); }
        assert_eq!(d.project.machines, start.machines);
        assert_eq!(d.project.song, start.song);
        for _ in 0..5 { d.redo(); }
        assert_eq!(d.project, end);
        assert!(d.project.machine(copy).is_some());
        assert_eq!(d.project.machines[0].name, "Breaks");
    }

    #[test]
    fn edits_to_a_machine_survive_its_removal_and_return() {
        let mut d = Document::new(acid_engine::demo::demo_project());
        let id = ParamId::Machine(MachineUid(2), MachineParam::Synth(SynthParam::Cutoff));
        d.apply(Edit::Param { id, value: 0.1 }, false);
        d.remove_machine(MachineUid(2));
        d.undo(); // machine back
        d.undo(); // knob back — must not panic
        assert_eq!(d.project.param(id), acid_engine::demo::demo_project().param(id));
        d.redo();
        d.redo();
        assert!(d.project.machine(MachineUid(2)).is_none());
        d.undo();
        assert_eq!(d.project.param(id), 0.1);
    }

    #[test]
    fn the_rack_cap_and_no_op_edits() {
        let mut d = Document::new(acid_engine::project::Project::default());
        while d.project.machines.len() < 16 { d.add_machine(MachineType::Kit808).unwrap(); }
        assert!(d.add_machine(MachineType::Fm).is_none());
        assert!(d.duplicate_machine(MachineUid(1)).is_none());
        let depth = d.undo_depth();
        d.move_machine(MachineUid(1), 0);
        d.rename_machine(MachineUid(1), "Synth 1");
        assert_eq!(d.undo_depth(), depth, "no-ops leave no undo step");
    }
```

(`undo_depth()` — add a tiny `pub fn undo_depth(&self) -> usize` if no equivalent exists; use whatever `Document::apply`'s signature really is for the `Param` edit.)

- [ ] **Steps 2–5:** fail → implement → full checks → commit `feat(app): undoable add, duplicate, remove, move and rename of machines`.

---

### Task 4: Keeping the engine's rack in step

**Files:** `crates/app/src/ui/mod.rs`, `ui/files.rs`, `ui/record.rs`, `ui/audio_settings.rs` (any place that (re)starts or reloads audio), panels that read shared FM/sampler UI state, tests.

**Interfaces (produces):**
- `SidAcidApp` gains `engine_rack: Vec<MachineUid>` (the uid order the engine has, as far as the UI knows) and `rack_expected: u32` (the rack generation the next matching status will carry).
  - On `start_audio`, `load_document`, audio-settings apply and any other `ReplaceEngine`: `engine_rack = project.rack_uids()`, `rack_expected = rack_expected.wrapping_add(1)` for a `ReplaceEngine` sent to a running host (or `0` when a fresh `AudioLink`/host is created — match the host's counter semantics from Task 1 exactly and test it).
  - `pub(crate) fn sync_rack(&mut self)`: if `project.rack_uids() != engine_rack` → `send(vec![Command::SetRack(RackSwap::build(&project, &engine_rack, sample_rate))])`, `engine_rack = project.rack_uids()`, `rack_expected += 1`, prune `view.panels` / `view.wave_cache` / `view.focus` / `view.audition` / `view.pad_note` / any other uid-keyed UI state of machines no longer in the rack. With no audio: still update `engine_rack` (so tests can inspect `flushed`/`pending`). Called after every document operation that can change the rack (the Task 5 handlers, undo, redo).
  - `sample_rate`: from `self.audio` (`a.sample_rate`), else 48000.0 (no engine to match).
- `on_status`: when `status.rack_gen != rack_expected`, keep the previous per-machine arrays (`playhead`, `patterns`, `queued`, `meters`) and take only the transport fields; when `status.rack_rejected` → `debug_assert!` false and reload the engine from the project (`load_project`), resetting `engine_rack`/`rack_expected` as on load.
- Shared per-machine UI state moves into `PanelState`: `sampler_slot`, `fm_op`, `fm_browser`, `fm_voice_src` (and anything else found shared that is per machine). `load_document`'s resets apply per panel.
- `record_pattern_change`: return early unless `project.machine(uid).is_some()`.

- [ ] **Step 1: Failing tests** (ui/mod.rs tests, headless app):

```rust
    #[test]
    fn a_rack_edit_sends_one_set_rack_built_against_the_engine_order() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.pending.clear();
        let before = app.engine_rack.clone();
        app.add_machine(MachineType::Fm); // Task 5 adds the UI; this task adds the method (doc op + sync_rack)
        let swaps: Vec<_> = app.pending.iter().filter(|c| matches!(c, Command::SetRack(_))).collect();
        assert_eq!(swaps.len(), 1);
        if let Command::SetRack(s) = swaps[0] {
            assert_eq!(s.order, app.doc.project.rack_uids());
            assert_eq!(s.fresh.iter().filter(|f| f.is_some()).count(), 1);
        }
        assert_ne!(app.engine_rack, before);
        app.pending.clear();
        app.undo();
        assert!(app.pending.iter().any(|c| matches!(c, Command::SetRack(_))), "undo syncs too");
        assert_eq!(app.engine_rack, before);
    }

    #[test]
    fn statuses_from_an_older_rack_do_not_move_panels() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Sampler);
        let mut old = Status::default();
        old.rack_gen = app.rack_expected.wrapping_sub(1);
        old.playhead[0] = 9;
        app.on_status(old, 1.0);
        assert_ne!(app.status.playhead[0], 9);
        let mut new = Status::default();
        new.rack_gen = app.rack_expected;
        new.playhead[0] = 9;
        app.on_status(new, 2.0);
        assert_eq!(app.status.playhead[0], 9);
    }

    #[test]
    fn removed_machines_leave_no_ui_state_behind() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.panel(MachineUid(6)).sampler_slot = 3;
        app.remove_machine(MachineUid(6));
        assert!(!app.view.panels.contains_key(&MachineUid(6)));
    }
```

(`add_machine`, `remove_machine`, `undo` here are `SidAcidApp` methods: doc op → `send(cmds)` → `sync_rack()`; `remove_machine` also sets the status line per the constraints. If `undo` already exists as an app method, extend it to call `sync_rack`.)

- [ ] **Steps 2–5:** fail → implement → full checks (golden, no_alloc) → launch check → commit `feat(app): the engine's rack follows the document; stale-rack statuses ignored; per-machine UI state`.

---

### Task 5: Rack controls in the UI

**Files:** `crates/app/src/ui/rack_header.rs` (new), `synth_panel.rs`, `drum_panel.rs`, `fm_panel.rs`, `sampler_panel.rs`, `ui/mod.rs` (`rack()`: add bar), tests.

**Interfaces (produces):**
- `pub(crate) fn machine_panel(&mut self, ui, uid, salt: &'static str, title: String, body: impl FnOnce(&mut SidAcidApp, &mut egui::Ui))` in `rack_header.rs`: an `egui::collapsing_header::CollapsingState` keyed by the same id the panels use today (`(salt, uid)` id_salt → keep each panel's open/closed state and the `forced_open` test hook working), whose header row shows the title (unchanged text), then right-aligned buttons `↑` `↓` `⧉` `✕` (tooltips "Move up", "Move down", "Duplicate", "Remove"), disabled per the constraints, and handles double-click-to-rename with an inline `TextEdit` (state `view.renaming: Option<(MachineUid, String)>`; Enter commits via `rename_machine`, Escape or focus loss without Enter cancels). The four panels call it instead of building their own `CollapsingHeader`.
- After the last machine panel and before the mixer: `+ Add machine ▾` menu button (items in `MachineType::ALL` order with `label()`), disabled at 16 with tooltip `Rack full (16)`, followed by `n / 16`.
- Button handlers call the Task 4 app methods (`add_machine`, `duplicate_machine`, `remove_machine`, `move_machine`, `rename_machine`), which do doc op → send → `sync_rack`.
- A new machine's panel is open by default; focus moves to it.

- [ ] **Step 1: Failing tests** (headless, using the app methods and pure helpers — clicking egui buttons in tests is not required):

```rust
    #[test]
    fn rack_buttons_follow_the_rules() {
        let app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let first = app.doc.project.machines[0].uid;
        let last = app.doc.project.machines.last().unwrap().uid;
        assert!(!app.can_move_up(first) && app.can_move_down(first));
        assert!(app.can_move_up(last) && !app.can_move_down(last));
        assert!(app.can_add());
    }

    #[test]
    fn renaming_commits_on_enter_and_restores_the_default_when_empty() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.begin_rename(MachineUid(3));
        app.view.renaming.as_mut().unwrap().1 = "Boom".into();
        app.commit_rename();
        assert_eq!(app.panel_title(MachineUid(3)), "Boom");
        app.begin_rename(MachineUid(3));
        app.view.renaming.as_mut().unwrap().1 = String::new();
        app.commit_rename();
        assert_eq!(app.panel_title(MachineUid(3)), "808");
        app.begin_rename(MachineUid(3));
        app.cancel_rename();
        assert!(app.view.renaming.is_none());
    }

    #[test]
    fn headers_keep_todays_text_and_the_rack_draws_with_controls() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        run_frames(&mut app, 2); // draws every panel through machine_panel without panicking
        app.add_machine(MachineType::Kit909);
        run_frames(&mut app, 2);
        assert_eq!(app.panel_title(*app.doc.project.rack_uids().last().unwrap()), "909 2");
    }
```

- [ ] **Steps 2–5:** fail → implement → full checks → launch check → commit `feat(app): rack controls — add menu, move, duplicate, remove, inline rename`.

---

### Task 6: Manual verification (controller)

- [ ] Throwaway build that, at startup, builds a rack of 3×303 + 2 Samplers + 909 + FM through the app methods, starts song or pattern playback is not clickable — instead verify by screenshots that headers show ↑ ↓ ⧉ ✕, the Add bar shows `7 / 16`, and the mixer/song panels list the new machines (scroll with a second throwaway build that collapses the machine panels via `forced_open`).
- [ ] Offline: a probe example that loads a saved 16-machine project (built through `Project` ops) and renders it (pattern + song) without error; save → reload round trip.
- [ ] Record results in `docs/superpowers/plans/2026-10-14-sidacid-plan11-deferred.txt`; commit.
