# Modular Rack (c): Per-Machine CPU & Shared Sample Pool Implementation Plan (Plan 12)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show each machine's CPU share on its panel header (and the top consumers in the transport hover), and replace the one-sampler 64 MB limit with one 256 MB sample pool shared by every sampler.

**Architecture:** The engine times each slot's work (its DSP render spans plus its mixer strip) and the shared work, accumulating nanoseconds per rack position; the host turns them into shares of the audio time each status covers. The app smooths those per machine and shows them only from statuses whose rack generation matches. The pool is a single constant used by project validation, the parse budget, sample loading, sampler duplication and the meter.

**Tech Stack:** Rust 2024 workspace (`crates/acid-engine`, `crates/app` = package `sidacid`), egui 0.31, rtrb.

**Spec:** `docs/superpowers/specs/2026-10-14-modular-rack-design.md` §4 and §5 "Plan (c)". Notes: `docs/superpowers/plans/2026-10-14-sidacid-plan11-deferred.txt` ("Plan 10c: put the 256 MB pool refusal for duplicates in SidAcidApp::duplicate_machine").

## Global Constraints

- Timing uses `std::time::Instant` around each slot's work; no allocation, locking or blocking in `Engine::process`/`EngineHost::process` (`tests/no_alloc.rs` must pass).
- `Status` gains `slot_cpu: [f32; MAX_MACHINES]` (by rack position) and `shared_cpu: f32`; each is time spent ÷ audio time covered since the previous status (1.0 = the whole budget); counters reset when a status is sent.
- Header label `CPU n%` (n = smoothed share × 100, rounded); colour: normal ≤ 25 %, amber (`theme::QUEUED`) > 25 %, red (`theme::ACCENT`) > 50 %. Smoothing like the main readout (exponential, the same `CPU_SMOOTHING`-style weight the host uses for `cpu`, applied per status).
- Slot figures are used only from statuses with `rack_gen == rack_expected`; a machine's smoothed value follows its uid.
- Transport CPU hover adds a line: `Top: <name> n% · <name> n% · <name> n% · FX/master n%` (up to three machines with the highest smoothed share, descending; fewer if the rack is smaller).
- `SAMPLE_POOL = 256 * 1024 * 1024` replaces `MEMORY_LIMIT` everywhere. Each slot's decoded audio counts once per use (duplicates count again).
  - `validate`: total ≤ pool, message `sampler audio exceeds 256 MB`.
  - Parse: one embedded-PCM budget of pool / 2 shared across all samplers (`with_pcm_budget`).
  - Sample load refused if total − slot's current + new > pool, message exactly: `Not enough sample memory: <n> MB needed, <m> MB free of 256 MB` where n = new audio in MB rounded up, m = free MB rounded down.
  - Duplicate of a sampler refused the same way (same message format, n = the copy's audio).
  - Meter: `Smp <total>/256 MB`.
- Golden render test unchanged; clippy zero warnings; `cargo fmt --all`; `cargo test --workspace`.
- Commit trailer: blank line then `Co-Authored-By: <model you are> <noreply@anthropic.com>`.
- Branch `rack-cpu` from master (created by the controller).

---

### Task 1: Engine per-slot CPU timing

**Files:** `crates/acid-engine/src/engine.rs`, `rack.rs` (if the strip pass moves), `mixer.rs`, `host.rs`, `tests/no_alloc.rs`.

**Interfaces (produces):**
```rust
impl Engine {
    /// Nanoseconds of work per rack position and for shared work since the last call; resets them.
    pub fn take_cpu_ns(&mut self) -> ([u64; MAX_MACHINES], u64);
}
// Status: pub slot_cpu: [f32; MAX_MACHINES], pub shared_cpu: f32
```
- In `process_block`, wrap each `slot.render(span)` in `Instant::now()`/`elapsed()` adding into `slot_ns[pos]`; in `Mixer::process`, time each slot's strip pass and add to the same position (pass `&mut [u64; MAX_MACHINES]` into the mixer, or have the mixer return per-strip ns). Shared ns = whole `process_block` elapsed − sum of slot ns for that block (saturating).
- `set_rack` permutes `slot_ns` with the `RackMap` (new slots 0) so a machine's partial accumulation follows it.
- Host: on each status, `secs = frames_since_status / sample_rate`; `slot_cpu[i] = ns[i] as f32 / 1e9 / secs`; `shared_cpu` likewise; non-finite or zero `secs` → 0.

- [ ] **Step 1: Failing tests** (engine.rs / host.rs):

```rust
    #[test]
    fn a_busy_sampler_costs_more_than_an_idle_synth() {
        // Rack: Synth 1 (no notes playing) and a Sampler with a long sample and 16 held notes.
        let mut p = Project::default();
        p.machines.retain(|m| m.uid == MachineUid(1) || m.machine_type() == MachineType::Sampler);
        let sampler_uid = p.machines.iter().find(|m| m.machine_type() == MachineType::Sampler).unwrap().uid;
        load_long_sample(&mut p, sampler_uid); // helper: one slot with ~2 s of noise, key range covering 36..=96
        let mut eng = Engine::from_project(48000.0, &p);
        for k in 0..16 { eng.note_on(sampler_uid, 40 + k, 100); }
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..200 { eng.process(&mut l, &mut r); }
        let (ns, _) = eng.take_cpu_ns();
        let pos_s = p.position(sampler_uid).unwrap();
        let pos_y = 1 - pos_s;
        assert!(ns[pos_s] > ns[pos_y], "sampler {} ns vs idle synth {} ns", ns[pos_s], ns[pos_y]);
        assert_eq!(eng.take_cpu_ns().0, [0; MAX_MACHINES], "taking resets");
    }

    #[test]
    fn statuses_carry_shares_that_fit_in_the_budget() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::Play).unwrap();
        run(&mut host, 48000);
        let s = std::iter::from_fn(|| rx.pop().ok())
            .filter_map(|e| match e { Event::Status(s) => Some(s), _ => None })
            .last().unwrap();
        let sum: f32 = s.slot_cpu.iter().sum::<f32>() + s.shared_cpu;
        assert!(s.slot_cpu.iter().all(|c| c.is_finite() && *c >= 0.0));
        assert!(sum > 0.0, "something was timed");
        assert!(sum < 1.0, "a test render on this machine is far below real time: {sum}");
        assert!(s.slot_cpu[6..].iter().all(|c| *c == 0.0), "empty positions read zero");
    }

    #[test]
    fn cpu_accumulators_follow_their_machine_through_a_reorder() {
        let p = crate::demo::demo_project();
        let mut eng = Engine::from_project(48000.0, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..50 { eng.process(&mut l, &mut r); }
        let mut q = p.clone();
        q.machines.reverse();
        let before = eng.peek_cpu_ns(); // test accessor: copy without reset
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0);
        eng.set_rack(&mut swap);
        let after = eng.peek_cpu_ns();
        for i in 0..6 { assert_eq!(after.0[5 - i], before.0[i]); }
    }
```

(`load_long_sample` and `peek_cpu_ns` are small test helpers you add; adapt the sampler set-up to the real sampler API — read `sampler/` and existing sampler tests for how a slot gets audio and a key range.) Add a `no_alloc.rs` case: a sequencing rack processed with the counter armed, `take_cpu_ns` called each block — zero allocations.

- [ ] **Steps 2–5:** fail → implement → full checks (golden unchanged, no_alloc) → commit `feat(engine): per-machine CPU timing in status`.

---

### Task 2: CPU on panel headers and in the transport hover

**Files:** `crates/app/src/ui/perf.rs`, `rack_header.rs`, `rack_sync.rs` (gating), tests.

**Interfaces (produces):**
- `PerfMeter` gains `machine_cpu: HashMap<MachineUid, f32>` (smoothed) and `shared_cpu: f32`; `on_status(&status, now)` gains the rack-order uids and a `fresh: bool` (rack_gen matches) — only fresh statuses update machine values, mapping `slot_cpu[pos]` to `uids[pos]`; removed uids are dropped when pruning UI state.
- `fn cpu_label(share: f32) -> String` → `"CPU n%"`; `fn cpu_colour(share, normal) -> Color32` (thresholds per Global Constraints); `fn top_line(entries: &[(String, f32)], shared: f32) -> String` per the Global Constraints format.
- `panel_header` draws `CPU n%` (with the colour) just left of the ⬆ ⬇ 🗐 🗙 buttons, for every machine with a value (none shown before the first fresh status).
- The transport CPU hover appends the `Top:` line.

- [ ] **Step 1: Failing tests:**

```rust
    #[test]
    fn cpu_labels_and_colours() {
        let n = egui::Color32::WHITE;
        assert_eq!(cpu_label(0.044), "CPU 4%");
        assert_eq!(cpu_colour(0.25, n), n);
        assert_eq!(cpu_colour(0.26, n), theme::QUEUED);
        assert_eq!(cpu_colour(0.51, n), theme::ACCENT);
    }

    #[test]
    fn the_top_line_lists_the_three_busiest_machines() {
        let e = [("Bass".to_string(), 0.04), ("Breaks".into(), 0.09), ("Lead".into(), 0.03), ("Hats".into(), 0.01)];
        assert_eq!(top_line(&e, 0.06), "Top: Breaks 9% · Bass 4% · Lead 3% · FX/master 6%");
        assert_eq!(top_line(&e[..1], 0.0), "Top: Bass 4% · FX/master 0%");
    }

    #[test]
    fn machine_cpu_ignores_stale_statuses_and_follows_uids() {
        let mut p = PerfMeter::default();
        let uids = [MachineUid(1), MachineUid(2)];
        let mut s = Status::default();
        s.slot_cpu[0] = 0.2;
        s.slot_cpu[1] = 0.1;
        p.on_status(&s, 1.0, &uids, false);
        assert!(p.machine_cpu(MachineUid(1)).is_none(), "stale status ignored");
        for t in 0..200 { p.on_status(&s, 2.0 + t as f64, &uids, true); }
        assert!((p.machine_cpu(MachineUid(1)).unwrap() - 0.2).abs() < 0.01);
        let swapped = [MachineUid(2), MachineUid(1)];
        let mut s2 = Status::default();
        s2.slot_cpu[0] = 0.1;
        s2.slot_cpu[1] = 0.2;
        p.on_status(&s2, 500.0, &swapped, true);
        assert!((p.machine_cpu(MachineUid(1)).unwrap() - 0.2).abs() < 0.01, "follows the uid, not the position");
    }
```

(Adapt `on_status`'s existing signature: keep the main CPU/peak behaviour unchanged and existing perf tests passing.)

- [ ] **Steps 2–5:** fail → implement → full checks → launch check (`timeout 4 target/debug/sidacid`, exit 124) → commit `feat(app): per-machine CPU on panel headers and top consumers in the CPU hover`.

---

### Task 3: One 256 MB sample pool

**Files:** `crates/acid-engine/src/sampler/mod.rs`, `project.rs`, `crates/app/src/ui/sampler_load.rs`, `rack_sync.rs`, `perf.rs`, tests in all of them and `crates/app/src/ui/mod.rs` tests that use `MEMORY_LIMIT`.

**Interfaces (produces):**
- `pub const SAMPLE_POOL: usize = 256 * 1024 * 1024;` (remove `MEMORY_LIMIT`; update every user).
- `pub fn pool_message(needed_bytes: usize, free_bytes: usize) -> String` in the app (one place builds the refusal text) → `"Not enough sample memory: {n} MB needed, {m} MB free of 256 MB"` with n = ceil(needed / MiB), m = floor(free / MiB).
- Sample load and sampler duplicate both use it; the duplicate's old "(64 MB in all)" message is replaced.

- [ ] **Step 1: Failing tests:**

```rust
    // app
    #[test]
    fn pool_messages_round_needed_up_and_free_down() {
        let mib = 1024 * 1024;
        assert_eq!(pool_message(31 * mib - 5, 12 * mib + mib / 2),
                   "Not enough sample memory: 31 MB needed, 12 MB free of 256 MB");
        assert_eq!(pool_message(31 * mib + 1, 0), "Not enough sample memory: 32 MB needed, 0 MB free of 256 MB");
    }

    // engine (project.rs)
    #[test]
    fn the_pool_is_shared_across_samplers() {
        let mut p = Project::default();
        let s2 = p.new_machine(MachineType::Sampler);
        p.insert_machine(p.machines.len(), s2.clone()).unwrap();
        fill_sampler(&mut p, MachineUid(6), 100 * 1024 * 1024);  // test helper: decoded bytes in slots
        fill_sampler(&mut p, s2.uid, 100 * 1024 * 1024);
        p.validate().unwrap(); // 200 MB < 256 MB
        let s3 = p.new_machine(MachineType::Sampler);
        p.insert_machine(p.machines.len(), s3.clone()).unwrap();
        fill_sampler(&mut p, s3.uid, 100 * 1024 * 1024);
        assert!(p.validate().unwrap_err().to_string().contains("exceeds 256 MB"));
    }

    #[test]
    fn a_project_over_the_old_limit_round_trips() {
        let mut p = Project::default();
        fill_sampler(&mut p, MachineUid(6), 90 * 1024 * 1024); // > 64 MB, < 256 MB
        let q = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(q, p);
    }
```

Update the existing app tests that build "just over the limit" samplers (ui/mod.rs ~2089, ~2424) to the pool and the new messages; keep their intent (load refused over the pool; duplicate refused over the pool; small ones fine). `fill_sampler` builds `SlotData` whose `memory_bytes()` totals the given amount — read `sampler/slot.rs` for how `SlotData`'s PCM is stored (16-bit embedded vs f32 decoded) and size accordingly; keep fixture sizes as small as the assertions allow (tests must stay fast — prefer a few large slots of silence).

- [ ] **Steps 2–5:** fail → implement → full checks (golden, no_alloc) → commit `feat(engine,app): one 256 MB sample pool shared by every sampler`.

---

### Task 4: Manual verification (controller)

- [ ] Throwaway build with a busy rack (3×303 playing + 2 samplers with sliced breaks if available, else the demo + duplicates): screenshot shows `CPU n%` on headers; hover text cannot be captured reliably — covered by tests.
- [ ] Offline probe: a project with ~150 MB of sampler audio across two samplers saves and reopens.
- [ ] Record results in `docs/superpowers/plans/2026-10-14-sidacid-plan12-deferred.txt`; commit.
