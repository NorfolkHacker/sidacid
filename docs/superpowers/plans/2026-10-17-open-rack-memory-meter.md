# Open Rack and Memory Meter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the 16-machine rack cap and the 256 MB sample pool. In
their place, a memory meter measures the app against system RAM and
warns, without blocking, before actions that would leave the system
short.

**Architecture:**
- Per-position state that belongs to a machine moves into its `Slot`:
  its peak meter and its CPU nanoseconds. A `Slot` is built on the UI
  thread, so this state then grows and reorders with the rack for free.
- Per-position state that must stay in the sequencer becomes `Vec`s sized
  to an engine **rack capacity** (a multiple of 16). When a rack edit
  outgrows that capacity, the UI thread builds bigger buffers and sends
  them inside `RackSwap` as a `Growth`. The audio thread copies its state
  across and swaps the buffers in.
- Per-position status (`playhead`, `patterns`, `queued`, `meters`,
  `slot_cpu`) travels in pooled `Box<PositionStatus>` buffers that the UI
  thread allocates and recycles.
- The audio thread never allocates, and `tests/no_alloc.rs` keeps proving
  it.

**Tech Stack:** Rust 2024 workspace (`acid-engine`, `app`), egui/eframe
0.31, `rtrb` rings, `memory-stats` (process RSS), new: `sysinfo` 0.39
(system RAM).

**Spec:** `docs/superpowers/specs/2026-10-17-open-rack-memory-meter-design.md`

**Deviation from the spec (decided while planning):**
- The spec put every buffer the engine holds "in `RackSwap`". The plan
  moves the meter and CPU accumulators into `Slot` instead, so only the
  sequencer's buffers and the event buffer need a `Growth`.
- `RackMap` stops being a type alias for a fixed array and becomes a
  `Vec` field filled inside the swap.
- Task 10 updates the spec to match.

## Global Constraints

- The audio thread (`Engine::process`, `EngineHost::process`, and every
  `Command` the host applies) must not allocate or free. Every new
  audio-side path gets a `count_allocs` case in
  `crates/acid-engine/tests/no_alloc.rs`.
- User-facing text never names a hardware product or manufacturer.
- Commits:
  - author `NorfolkHacker <17689595+NorfolkHacker@users.noreply.github.com>`;
    the repo has no git identity set, so pass
    `-c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com`;
  - message style `feat(engine): …` / `fix(app): …` / `docs: …`;
  - end every message with
    `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Work on a branch `open-rack` (create it in Task 1). When finished it is
  fast-forward merged into `master` and pushed, but ask first.
- Run the full suite with `cargo test --workspace` and lint with
  `cargo clippy --workspace --all-targets -- -D warnings`. Both must be
  clean at the end of every task.
- Meter thresholds, copied verbatim from the spec:
  - amber below **25 % of total or 2 GB**, whichever is larger;
  - red below **10 % of total or 512 MB**, whichever is larger.
- Rack capacity is a multiple of 16:
  `rack_capacity(n) = max(16, n.div_ceil(16) * 16)`.
- Per-sample guards stay: `MAX_SECONDS` (30 s) and `MAX_BYTES` (30 s at
  192 kHz stereo, 16-bit).

## File map

| File | Change |
|---|---|
| `crates/acid-engine/src/machine.rs` | Remove `MAX_MACHINES`; add `RACK_CHUNK`, `rack_capacity`, `MachineType::memory_estimate` |
| `crates/acid-engine/src/song.rs` | `Banks` borrows slices instead of fixed arrays |
| `crates/acid-engine/src/rack.rs` | `Slot` gains `meter`, `cpu_ns`; `RackSwap` gains `map`, `tys`, `selected: Vec`, `capacity`, `growth`; `Growth`; drop `RackMap` alias and `remap_positions` |
| `crates/acid-engine/src/mixer.rs` | Meters into slots, `master_meter`; drop `remap_meters`, `strip_ns` |
| `crates/acid-engine/src/sequencer.rs` | `tracks`/`clocks`/`chosen` → `Vec`s at capacity; `grow`; `rearrange` on slices; `PENDING` → per-capacity |
| `crates/acid-engine/src/engine.rs` | Capacity, growth in `set_rack`, `fill_positions`, event reserve per capacity |
| `crates/acid-engine/src/host.rs` | `Status` loses arrays, gains `master_meter`/`positions`; `PositionStatus`; buffer pool; new command and event |
| `crates/acid-engine/src/project.rs`, `rack_edit.rs`, `migrate.rs` | Drop cap; uid exhaustion; drop pool check; load guard hook |
| `crates/acid-engine/src/sampler/{mod,slot}.rs` | Drop `SAMPLE_POOL`, `with_budget` |
| `crates/app/src/ui/positions.rs` (new) | UI-side `Shown` per-position status and the buffer pool bookkeeping |
| `crates/app/src/sysmem.rs` (new) | `SystemMemory` trait, `SysinfoMemory`, `FakeMemory` (tests) |
| `crates/app/src/ui/mem_warn.rs` (new) | Estimate + warning modal |
| `crates/app/src/ui/{mod,rack_sync,mixer_panel,pads,perf,rack_header,sampler_load}.rs`, `document.rs` | Follow the above |
| `docs/manual/*.md`, specs | Docs |

---

### Task 1: `Banks` over slices

`Banks` holds two `[_; MAX_MACHINES]` arrays and is rebuilt on every tick.
Make it a borrowed view instead, so it has no size limit. There is no
behaviour change.

**Files:**
- Modify: `crates/acid-engine/src/song.rs:58-130` (`Banks`)
- Modify: every `banks.slots[` / `banks.machines[` use (24 sites:
  `grep -n "banks.slots\|banks.machines" crates/acid-engine/src/*.rs`)
- Test: `crates/acid-engine/src/song.rs` tests module

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy)]
  pub enum Banks<'a> { Slots(&'a [Slot]), Machines(&'a [MachineEntry]) }
  impl<'a> Banks<'a> {
      pub fn empty() -> Self;                       // Banks::Slots(&[])
      pub fn of_slots(slots: &'a [Slot]) -> Self;
      pub fn of(p: &'a Project) -> Self;
      pub fn of_machines(m: &'a [MachineEntry]) -> Self;
      pub fn len(&self) -> usize;                   // machines in the rack
      pub fn bank(&self, pos: usize) -> Option<Bank<'a>>;
      pub fn machine(&self, pos: usize) -> Option<(MachineUid, MachineType)>;
      pub fn bank_len(&self, pos: usize) -> usize;  // unchanged semantics
      pub fn length(&self, pos: usize, slot: u8) -> Option<usize>;
      pub fn transposes(&self, pos: usize) -> bool;
  }
  ```

- [ ] **Step 1: Branch**

```bash
git checkout -b open-rack
```

- [ ] **Step 2: Write the failing test** (in `song.rs` tests)

```rust
#[test]
fn banks_see_every_machine_of_a_long_rack() {
    use crate::project::MachineEntry;
    let machines: Vec<MachineEntry> = (1..=40u16)
        .map(|u| MachineEntry::new(MachineUid(u), MachineType::Fm, format!("F{u}")))
        .collect();
    let b = Banks::of_machines(&machines);
    assert_eq!(b.len(), 40);
    assert_eq!(b.machine(39), Some((MachineUid(40), MachineType::Fm)));
    assert!(b.bank(39).is_some());
    assert!(b.bank(40).is_none() && b.machine(40).is_none());
    assert_eq!(Banks::empty().len(), 0);
}
```

- [ ] **Step 3: Run it and check that it fails**

Run: `cargo test -p acid-engine banks_see_every_machine`
Expected: FAIL to compile (`no method named len`). With today's arrays,
position 39 would not exist anyway.

- [ ] **Step 4: Implement**

Replace the struct and constructors:

```rust
/// Every rack position's pattern bank, borrowed, with the machine there.
/// Cheap to copy. Positions past the rack read `None`.
#[derive(Clone, Copy)]
pub enum Banks<'a> {
    /// The engine's rack, in order.
    Slots(&'a [Slot]),
    /// A project's rack, in order.
    Machines(&'a [MachineEntry]),
}

impl<'a> Banks<'a> {
    /// No machines.
    pub fn empty() -> Self {
        Banks::Slots(&[])
    }

    pub fn of_slots(slots: &'a [Slot]) -> Self {
        Banks::Slots(slots)
    }

    pub fn of(p: &'a Project) -> Self {
        Self::of_machines(&p.machines)
    }

    pub fn of_machines(machines: &'a [MachineEntry]) -> Self {
        Banks::Machines(machines)
    }

    pub fn len(&self) -> usize {
        match self {
            Banks::Slots(s) => s.len(),
            Banks::Machines(m) => m.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The pattern bank at `pos`; `None` past the rack.
    pub fn bank(&self, pos: usize) -> Option<Bank<'a>> {
        use crate::project::MachineBody;
        match *self {
            Banks::Slots(s) => s.get(pos).map(|s| Bank::of_slot(&s.patterns)),
            Banks::Machines(m) => m.get(pos).map(|m| match &m.body {
                MachineBody::Synth(x) => Bank::Synth(&x.patterns),
                MachineBody::Kit(x) => Bank::Drum(&x.patterns),
                MachineBody::Fm(x) => Bank::Notes(&x.patterns),
                MachineBody::Sampler(x) => Bank::Notes(&x.patterns),
                MachineBody::Mono(x) => Bank::Notes(&x.patterns),
            }),
        }
    }

    /// The machine at `pos`; `None` past the rack.
    pub fn machine(&self, pos: usize) -> Option<(MachineUid, MachineType)> {
        match *self {
            Banks::Slots(s) => s.get(pos).map(|s| (s.uid, s.ty)),
            Banks::Machines(m) => m.get(pos).map(|m| (m.uid, m.machine_type())),
        }
    }
}
```

Rewrite `bank_len`, `length` and `transposes` on top of them:

```rust
pub fn bank_len(&self, pos: usize) -> usize {
    self.bank(pos).map_or(0, |b| b.len())
}
pub fn length(&self, pos: usize, slot: u8) -> Option<usize> {
    self.bank(pos)?.length(slot)
}
pub fn transposes(&self, pos: usize) -> bool {
    self.machine(pos).is_some_and(|(_, ty)| ty.transposes())
}
```

Then fix every caller:

- `banks.slots[pos]` → `banks.bank(pos)`.
- `banks.machines[pos]` → `banks.machine(pos)`.
- `banks.slots[pos].is_some()` → `banks.bank(pos).is_some()`.
- `if let Some(Bank::Synth(bank)) = banks.slots[pos]` →
  `if let Some(Bank::Synth(bank)) = banks.bank(pos)`.
- `migrate.rs:354` and `:447`: `(0..MAX_MACHINES)` →
  `(0..banks.len())`. Remove the `MAX_MACHINES` import there if it is
  unused.

- [ ] **Step 5: Run the tests**

Run: `cargo test --workspace`
Expected: all PASS, including the new test.

- [ ] **Step 6: Commit**

```bash
git add -A crates/acid-engine
git commit -m "refactor(engine): Banks borrows the rack instead of copying it into arrays"
```

---

### Task 2: Meters and CPU time live in the slot

Each slot's peak meter and render/strip nanoseconds move into `Slot`.
The mixer keeps only the master peak. This removes `remap_meters`,
`remap_positions`, `Engine::slot_ns`, the host's `meters`/`slot_ns`
arrays and the `strip_ns` argument. The status arrays stay fixed at 16 in
this task; they are filled from the slots.

**Files:**
- Modify: `crates/acid-engine/src/rack.rs` (`Slot`, `from_entry`; delete
  `remap_positions`)
- Modify: `crates/acid-engine/src/mixer.rs` (`meters` → `master_meter`;
  `process` writes `slot.meter`, adds strip time to `slot.cpu_ns`; delete
  `remap_meters`, `rearrange`)
- Modify: `crates/acid-engine/src/engine.rs` (drop `slot_ns`;
  `process_block` adds render time to `s.cpu_ns`; new `take_slot_stats`;
  `take_meters` → `take_master_meter`; `take_cpu_ns` → `take_shared_ns`)
- Modify: `crates/acid-engine/src/host.rs` (accumulate only the master
  meter and shared ns; build the status arrays from the slots)
- Test: the `mixer.rs`, `engine.rs` and `host.rs` tests that use
  `take_meters`/`take_cpu_ns`/`meters[MAX_MACHINES]`

**Interfaces:**
- Produces:
  ```rust
  // rack.rs, on Slot
  pub meter: f32,   // post-fader peak since the last take; 0 when muted
  pub cpu_ns: u64,  // render + strip time since the last take
  // engine.rs
  pub fn take_master_meter(&mut self) -> f32;
  pub fn take_shared_ns(&mut self) -> u64;
  /// Calls `f(pos, meter, cpu_ns)` for each slot in rack order and resets both.
  pub fn take_slot_stats(&mut self, f: impl FnMut(usize, f32, u64));
  ```
  The host keeps its own max/sum of these between statuses only for the
  master meter and the shared ns. The per-slot ones simply stay in the
  slot until a status takes them.

- [ ] **Step 1: Write the failing test** (`engine.rs` tests)

```rust
#[test]
fn a_slots_meter_follows_it_through_a_rack_edit() {
    // Two identical engines render the same audio; one then moves its
    // first machine to the end. Meters, keyed by uid, must agree.
    let p = Project::default();
    let (mut a, mut b) = (Engine::from_project(SR, &p), Engine::from_project(SR, &p));
    a.play();
    b.play();
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    for _ in 0..200 {
        a.process(&mut l, &mut r);
        b.process(&mut l, &mut r);
    }
    let mut q = p.clone();
    let first = q.machines.remove(0);
    q.machines.push(first);
    let mut swap = RackSwap::build(&q, &b.slot_uids(), SR);
    assert!(b.set_rack(&mut swap).is_some());
    let by_uid = |e: &mut Engine| {
        let uids = e.slot_uids();
        let mut v = Vec::new();
        e.take_slot_stats(|pos, m, ns| v.push((uids[pos], m, ns > 0)));
        v.sort_by_key(|x| x.0);
        v
    };
    let (sa, sb) = (by_uid(&mut a), by_uid(&mut b));
    assert!(sa.iter().any(|x| x.1 > 0.0), "something played: {sa:?}");
    assert_eq!(sa, sb, "each slot kept its own meter and time");
    assert!(by_uid(&mut a).iter().all(|x| x.1 == 0.0 && !x.2), "taking resets");
}
```

- [ ] **Step 2: Run it and check that it fails**

Run: `cargo test -p acid-engine a_slots_meter_follows`
Expected: FAIL to compile (`no method named take_slot_stats`).

- [ ] **Step 3: Implement**

1. `rack.rs`: add the fields to `Slot` and set them to `0.0` and `0` in
   `from_entry`. Delete `remap_positions`.
2. `mixer.rs`:
   - Replace `meters: [f32; MAX_MACHINES + 1]` with `master_meter: f32`.
   - In `process`:
     - remove the `strip_ns` parameter and the `count`/`MAX_MACHINES`
       truncation;
     - in the per-slot loop, write
       `slot.meter = slot.meter.max(sl.abs()).max(sr.abs());` and at the
       end
       `slot.cpu_ns = slot.cpu_ns.saturating_add(start.elapsed().as_nanos() as u64);`;
     - the final clip loop writes `self.master_meter`.
   - `take_meters` becomes:
     ```rust
     /// The master's peak (after the final clip) since the last call; resets it.
     pub fn take_master_meter(&mut self) -> f32 {
         std::mem::take(&mut self.master_meter)
     }
     ```
   - Delete `remap_meters` and `Mixer::rearrange`, and drop the
     `self.mixer.rearrange(&map)` call in `engine.rs`.
   - Fix the mixer tests at `mixer.rs:310`, `:317` (drop the last
     argument) and `:398` (read `slots[3].meter` and
     `m.take_master_meter()`).
3. `engine.rs`:
   - Delete `slot_ns` and its `remap_positions` call.
   - In `process_block`, the render loop becomes:
     ```rust
     for s in self.slots.iter_mut() {
         let start = Instant::now();
         s.render(pos..pos + run);
         let spent = start.elapsed().as_nanos() as u64;
         s.cpu_ns = s.cpu_ns.saturating_add(spent);
         slot_spent = slot_spent.saturating_add(spent);
     }
     ```
   - The mixer is now called as
     `self.mixer.process(&mut self.slots, n, out_l, out_r)`, after
     snapshotting the slots' total `cpu_ns`. The strip time then counts
     toward `slot_spent`:
     ```rust
     let before: u64 = self.slots.iter().map(|s| s.cpu_ns).sum();
     let fired = self.mixer.process(&mut self.slots, n, out_l, out_r);
     let after: u64 = self.slots.iter().map(|s| s.cpu_ns).sum();
     slot_spent = slot_spent.saturating_add(after.saturating_sub(before));
     ```
   - Add the accessors:
     ```rust
     pub fn take_master_meter(&mut self) -> f32 { self.mixer.take_master_meter() }
     pub fn take_shared_ns(&mut self) -> u64 { std::mem::take(&mut self.shared_ns) }
     pub fn take_slot_stats(&mut self, mut f: impl FnMut(usize, f32, u64)) {
         for (pos, s) in self.slots.iter_mut().enumerate() {
             f(pos, std::mem::take(&mut s.meter), std::mem::take(&mut s.cpu_ns));
         }
     }
     ```
   - In `load_project`, replace `self.take_cpu_ns()` with
     `self.take_shared_ns();` (new slots start at zero).
   - Rewrite the engine tests that used `take_cpu_ns`/`take_meters`
     (`engine.rs:1444`, `:1473`, `:2011`) on the new accessors.
4. `host.rs`:
   - Remove the `meters` and `slot_ns` fields and their resets. Keep
     `master_meter: f32` (max over blocks) and `shared_ns`.
   - `SetRack` no longer remaps anything; it only bumps `rack_gen` on
     success.
   - `send_status` builds the still-fixed arrays from the slots:
     ```rust
     let mut meters = [0.0; MAX_MACHINES + 1];
     let mut slot_cpu = [0.0; MAX_MACHINES];
     self.engine.take_slot_stats(|pos, m, ns| {
         if pos < MAX_MACHINES {
             meters[pos] = m;
             slot_cpu[pos] = share(ns);
         }
     });
     meters[MAX_MACHINES] = self.master_meter;
     ```
     Take the stats only after the `events.slots() > DISPOSAL_RESERVE`
     check has passed, so a status that can't be sent does not lose them.
     That means splitting the existing `if` so the status is built inside
     it.
   - Fix the host tests at `host.rs:647` and `:759`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace`
Expected: all PASS. `no_alloc` stays green, because `take_slot_stats`
takes a closure and does not allocate.

- [ ] **Step 5: Commit**

```bash
git commit -am "refactor(engine): each slot carries its own meter and CPU time"
```

---

### Task 3: `PositionStatus` and the status buffer pool

`Status` drops its per-position arrays (it is no longer `Copy`). The
audio thread fills UI-allocated `Box<PositionStatus>` buffers taken from a
small pool. The cap is still 16.

**Files:**
- Modify: `crates/acid-engine/src/host.rs`
- Create: `crates/app/src/ui/positions.rs` (UI side: `Shown` and the
  pool)
- Modify: `crates/app/src/ui/mod.rs` (`status` + `shown`, `pump`,
  `on_status`), `rack_sync.rs` (`gate_status`, `follow_rack`),
  `mixer_panel.rs:102,144`, `pads.rs:19-20`, `perf.rs:61`, `mod.rs:764`,
  and the app tests at `mod.rs:843-845, 2576-2610, 2681-2711`,
  `perf.rs:417-431`
- Test: `host.rs` tests, `positions.rs` tests, `no_alloc.rs`

**Interfaces:**
- Produces (engine, `host.rs`):
  ```rust
  /// By rack position, for one status. Allocated on the UI thread
  /// (`with_capacity`), filled on the audio thread without growing.
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct PositionStatus {
      pub playhead: Vec<usize>,
      pub patterns: Vec<u8>,
      pub queued: Vec<Option<u8>>,
      /// Post-fader peaks since the previous status; muted machines read 0.
      pub meters: Vec<f32>,
      /// Share of the audio time spent on each machine.
      pub slot_cpu: Vec<f32>,
  }
  impl PositionStatus {
      pub fn with_capacity(n: usize) -> Box<Self>;
      /// Every Vec has room for `n` entries.
      pub fn fits(&self, n: usize) -> bool;
      pub fn clear(&mut self);
      pub fn len(&self) -> usize;   // playhead.len()
  }
  /// Spare buffers the host holds at once.
  pub const STATUS_BUFFERS: usize = 4;
  // Status: remove playhead/patterns/queued/meters/slot_cpu; add
  pub master_meter: f32,
  pub positions: Option<Box<PositionStatus>>,
  // Status derives Clone, Debug, Default, PartialEq (not Copy)
  // Command:
  StatusBuffer(Box<PositionStatus>),
  // Event:
  /// A status buffer too small for the rack, handed back unfilled.
  DisposePositions(Box<PositionStatus>),
  // Engine:
  /// Appends one entry per rack position (within capacity: never grows);
  /// takes the slots' meters and CPU time. `share` turns ns into a share.
  pub fn fill_positions(&mut self, out: &mut PositionStatus, share: impl Fn(u64) -> f32);
  ```
- Produces (app, `positions.rs`):
  ```rust
  /// The per-position status the panels show, by rack position.
  #[derive(Clone, Debug, Default, PartialEq)]
  pub struct Shown { pub p: PositionStatus }
  impl Shown {
      pub fn playhead(&self, pos: usize) -> usize;        // 0 past the end
      pub fn pattern(&self, pos: usize) -> u8;            // 0 past the end
      pub fn queued(&self, pos: usize) -> Option<u8>;     // None past the end
      pub fn meter(&self, pos: usize) -> f32;             // 0.0 past the end
      pub fn slot_cpu(&self) -> &[f32];
      /// Takes `from`'s values (copying, keeping our own allocation).
      pub fn take_from(&mut self, from: &PositionStatus);
      /// Moves values from `old_rack` order to `new_rack` order; new machines read defaults.
      pub fn follow(&mut self, old_rack: &[MachineUid], new_rack: &[MachineUid]);
  }
  /// Buffers to send so the host holds `STATUS_BUFFERS`; tracks how many are out.
  pub struct BufferPool { out: usize, capacity: usize }
  impl BufferPool {
      pub fn new(capacity: usize) -> Self;          // out = 0
      /// Fresh buffers to send now: `STATUS_BUFFERS - out` of `capacity`.
      pub fn top_up(&mut self) -> Vec<Box<PositionStatus>>;
      /// A buffer came back (filled or disposed): reuse it if it fits, else drop.
      pub fn returned(&mut self, b: Box<PositionStatus>) -> Option<Box<PositionStatus>>;
      pub fn set_capacity(&mut self, capacity: usize);
      /// A new host holds none of ours.
      pub fn reset(&mut self);
  }
  ```
  The app keeps `status: Status` (scalars, with `positions` always
  `None` after intake) and `shown: Shown`.

- [ ] **Step 1: Write the failing engine tests** (`host.rs` tests)

```rust
#[test]
fn statuses_carry_positions_only_with_a_buffer() {
    let (mut host, mut tx, mut rx) = host_with_demo();
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    let statuses = |rx: &mut rtrb::Consumer<Event>| {
        let mut v = Vec::new();
        while let Ok(e) = rx.pop() {
            if let Event::Status(s) = e { v.push(s); }
        }
        v
    };
    for _ in 0..10 { host.process(&mut l, &mut r); }
    assert!(statuses(&mut rx).iter().all(|s| s.positions.is_none()));
    tx.push(Command::StatusBuffer(PositionStatus::with_capacity(16))).ok().unwrap();
    for _ in 0..10 { host.process(&mut l, &mut r); }
    let got = statuses(&mut rx);
    let filled: Vec<_> = got.iter().filter_map(|s| s.positions.as_ref()).collect();
    assert_eq!(filled.len(), 1, "one buffer, one filled status");
    assert_eq!(filled[0].len(), host.engine().slot_uids().len());
}

#[test]
fn a_buffer_too_small_for_the_rack_comes_back_unfilled() {
    let (mut host, mut tx, mut rx) = host_with_demo();
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    tx.push(Command::StatusBuffer(PositionStatus::with_capacity(2))).ok().unwrap();
    for _ in 0..10 { host.process(&mut l, &mut r); }
    let mut disposed = 0;
    while let Ok(e) = rx.pop() {
        match e {
            Event::DisposePositions(_) => disposed += 1,
            Event::Status(s) => assert!(s.positions.is_none()),
            _ => {}
        }
    }
    assert_eq!(disposed, 1);
}
```

Add to `no_alloc.rs`, inside `host_commands_and_rendering_do_not_allocate`
or as a new test of the same shape:

```rust
#[test]
fn status_buffers_do_not_allocate() {
    let (mut host, mut tx, mut rx) = host::channel(Engine::from_project(SR, &busy_project()));
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    for c in [16, 16, 2, 16] {
        tx.push(Command::StatusBuffer(PositionStatus::with_capacity(c))).ok().unwrap();
    }
    let n = count_allocs(|| {
        for _ in 0..2000 { host.process(&mut l, &mut r); }
    });
    assert_eq!(n, 0);
    drop(rx.pop());
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p acid-engine statuses_carry_positions a_buffer_too_small status_buffers_do_not`
Expected: FAIL to compile (`PositionStatus` not found).

- [ ] **Step 3: Implement the engine side**

1. Add `PositionStatus` and its methods:
   ```rust
   pub fn with_capacity(n: usize) -> Box<Self> {
       Box::new(Self {
           playhead: Vec::with_capacity(n),
           patterns: Vec::with_capacity(n),
           queued: Vec::with_capacity(n),
           meters: Vec::with_capacity(n),
           slot_cpu: Vec::with_capacity(n),
       })
   }
   pub fn fits(&self, n: usize) -> bool {
       [self.playhead.capacity(), self.patterns.capacity(), self.queued.capacity(),
        self.meters.capacity(), self.slot_cpu.capacity()].iter().all(|&c| c >= n)
   }
   pub fn clear(&mut self) {
       self.playhead.clear(); self.patterns.clear(); self.queued.clear();
       self.meters.clear(); self.slot_cpu.clear();
   }
   pub fn len(&self) -> usize { self.playhead.len() }
   ```
2. In the host, hold `spare: [Option<Box<PositionStatus>>; STATUS_BUFFERS]`.
   `Command::StatusBuffer(b)` goes into the first empty spare, or straight
   back as `DisposePositions` if every spare is full.
3. `send_status`, inside the `events.slots() > DISPOSAL_RESERVE` branch:
   - Find a spare that fits the rack. Each spare found that does not fit
     goes back as `DisposePositions`, and the search moves on.
   - With a fitting spare: `b.clear()`, then
     `self.engine.fill_positions(&mut b, share)`, then
     `status.positions = Some(b)`.
   - Without one: `positions = None`. The slot stats keep accumulating in
     the slots until a buffer comes.
4. `Engine::fill_positions`:
   ```rust
   pub fn fill_positions(&mut self, out: &mut PositionStatus, share: impl Fn(u64) -> f32) {
       debug_assert!(out.fits(self.slots.len()));
       for pos in 0..self.slots.len().min(out.playhead.capacity()) {
           out.playhead.push(self.seq.playhead(pos));
           out.patterns.push(self.seq.current_pattern(pos));
           out.queued.push(self.seq.queued_pattern(pos));
       }
       self.take_slot_stats(|pos, m, ns| {
           if pos < out.meters.capacity() {
               out.meters.push(m);
               out.slot_cpu.push(share(ns));
           }
       });
   }
   ```
   Delete `Engine::playhead`, `current_patterns` and `queued_patterns`
   (the array-returning ones). Rewrite their tests (`engine.rs:1864`,
   `:1938`, `:2282`) to read `seq` through a test-only
   `fn positions(&mut self) -> Box<PositionStatus>` helper under
   `#[cfg(test)]`, which allocates a buffer of `slots.len()` and fills it.
5. `Status` derives `Clone, Debug, Default, PartialEq`. The
   `#[allow(clippy::large_enum_variant)]` on `Event` can go if clippy no
   longer warns.

- [ ] **Step 4: Run the engine tests**

Run: `cargo test -p acid-engine`
Expected: PASS.

- [ ] **Step 5: Write the failing app tests** (`positions.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::machine::MachineUid as U;

    #[test]
    fn shown_reads_defaults_past_the_rack_and_follows_moves() {
        let mut s = Shown::default();
        let mut p = PositionStatus::default();
        p.playhead = vec![3, 5];
        p.patterns = vec![1, 2];
        p.queued = vec![None, Some(4)];
        p.meters = vec![0.5, 0.25];
        p.slot_cpu = vec![0.1, 0.2];
        s.take_from(&p);
        assert_eq!((s.playhead(1), s.pattern(1), s.queued(1)), (5, 2, Some(4)));
        assert_eq!((s.playhead(9), s.pattern(9), s.queued(9), s.meter(9)), (0, 0, None, 0.0));
        s.follow(&[U(1), U(2)], &[U(2), U(7), U(1)]);
        assert_eq!((s.playhead(0), s.playhead(1), s.playhead(2)), (5, 0, 3));
        assert_eq!(s.meter(0), 0.25);
    }

    #[test]
    fn the_pool_keeps_four_out_and_drops_small_buffers() {
        let mut pool = BufferPool::new(16);
        assert_eq!(pool.top_up().len(), STATUS_BUFFERS);
        assert!(pool.top_up().is_empty());
        let back = pool.returned(PositionStatus::with_capacity(16));
        assert!(back.is_some(), "a fitting buffer is reused");
        pool.set_capacity(32);
        assert!(pool.returned(PositionStatus::with_capacity(16)).is_none());
        assert_eq!(pool.top_up().len(), 1, "one replaced at the new size");
        pool.reset();
        assert_eq!(pool.top_up().len(), STATUS_BUFFERS);
    }
}
```

`returned` semantics: the buffer is counted as no longer out. When it
fits the current capacity it is returned cleared, for the caller to send
again, and counted out again. Otherwise it is dropped, and the next
`top_up` replaces it.

- [ ] **Step 6: Run them and check that they fail**

Run: `cargo test -p sidacid positions::`
Expected: FAIL to compile.

- [ ] **Step 7: Implement the app side**

1. Write `positions.rs` to the interface above and add `mod positions;`
   in `ui/mod.rs`.
2. Add fields to `SidAcidApp`: `pub(crate) shown: Shown` and
   `pool: BufferPool`, created with `BufferPool::new(16)`. Task 5 makes
   this the engine capacity.
3. In `pump`:
   - for `Event::Status(mut s)`: take `s.positions` out, call
     `on_status(s, positions, now)`, then send
     `pool.returned(buf)` back as `Command::StatusBuffer` when it is
     `Some`;
   - for `Event::DisposePositions(b)`: call `pool.returned(b)` and send
     the result back when it is `Some`;
   - then, before `flush_pending`, queue
     `pool.top_up().into_iter().map(Command::StatusBuffer)`.
4. `on_status(status, positions, now)`:
   - when `positions` is `Some` and `status.rack_gen == rack_expected`,
     call `self.shown.take_from(&p)`;
   - `perf.on_status` takes `positions.as_deref()` and only reads
     `slot_cpu` from it when it is `Some` and fresh.
5. `gate_status`: now only resets `rack_gen`. Keeping the shown arrays
   is what not calling `take_from` does. `status.master_meter` is always
   taken.
6. `follow_rack(&rack)` → `self.shown.follow(&self.engine_rack, rack)`.
7. Call `pool.reset()` wherever a new host starts (`rack_for_new_host`)
   and on `ReplaceEngine` (`rack_for_replaced_engine`), because the old
   host's buffers are gone.
8. Replace the reads:
   - `self.status.meters[pos]` → `self.shown.meter(pos)`;
   - `self.status.meters[MAX_MACHINES]` → `self.status.master_meter`;
   - `status.patterns[i]` → `shown.pattern(i)`;
   - `status.queued[i]` → `shown.queued(i)`;
   - `status.playhead[i]` → `shown.playhead(i)`.
   Fix the tests listed under **Files** to set
   `app.shown.p.playhead = vec![…]` and so on, instead of indexing
   arrays.
9. Remove `on_audio_failed`'s `Status::default()` copy semantics: also
   reset `self.shown = Shown::default()`.

- [ ] **Step 8: Run the tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS and clean.

- [ ] **Step 9: Check in the running app**

Use the `run` skill and screenshot memory notes (`sidacid-screenshots`):
- start the app and press play;
- the playheads move and the mixer meters move;
- a rack move keeps every machine's meter on its own strip.

- [ ] **Step 10: Commit**

```bash
git add -A crates
git commit -m "feat(engine): per-position status travels in pooled buffers"
```

---

### Task 4: `RackSwap` carries its map and scratch as `Vec`s

`RackMap` and `tys` are fixed arrays computed on the audio thread.
`selected` is one too. Make all three `Vec`s, built at the rack's length
on the UI thread and filled in place. `set_rack` returns `bool`.

**Files:**
- Modify: `crates/acid-engine/src/rack.rs` (`RackSwap`, `RackSwap::build`,
  remove the `RackMap` alias)
- Modify: `crates/acid-engine/src/engine.rs` (`check_rack`, `set_rack`)
- Modify: `crates/acid-engine/src/sequencer.rs` (`rearrange`,
  `Pending::remap` take slices; scratch `new_of: Vec<Option<usize>>` in
  `Sequencer`)
- Modify: `crates/acid-engine/src/host.rs` (`SetRack` arm)
- Test: `engine.rs` tests at `:1004`, `:2049`, the sequencer tests at
  `:3492`, `:3525`, `:3557`, `:3939`

**Interfaces:**
- Produces:
  ```rust
  pub struct RackSwap {
      pub order: Vec<MachineUid>,
      pub fresh: Vec<Option<Slot>>,
      pub into: Vec<Slot>,                 // capacity >= order.len()
      pub song: Box<CompiledSong>,
      pub selected: Vec<u8>,               // len == order.len()
      /// Filled by the engine: where each new position's slot came from.
      pub map: Vec<Option<usize>>,         // len == order.len()
      /// Engine scratch: each new position's type.
      pub tys: Vec<Option<MachineType>>,   // len == order.len()
  }
  impl Engine { pub fn set_rack(&mut self, swap: &mut RackSwap) -> bool; }
  impl Sequencer {
      pub fn rearrange(&mut self, map: &[Option<usize>], banks: Banks<'_>,
                       song: &CompiledSong, selected: &[u8]);
  }
  ```
  `map[i]` is the old position of the slot now at `i`, or `None` for a
  fresh slot. Positions at or past `map.len()` are cleared tracks.

- [ ] **Step 1: Write the failing test** (`engine.rs` tests)

```rust
#[test]
fn a_rack_swap_reports_where_each_slot_came_from() {
    let p = Project::default();
    let mut e = Engine::from_project(SR, &p);
    let mut q = p.clone();
    q.machines.swap(0, 2);
    let added = q.new_machine(MachineType::Fm);
    q.insert_machine(q.machines.len(), added).unwrap();
    let mut swap = RackSwap::build(&q, &e.slot_uids(), SR);
    assert!(e.set_rack(&mut swap));
    assert_eq!(swap.map, vec![Some(2), Some(1), Some(0), Some(3), Some(4), Some(5), None]);
}
```

- [ ] **Step 2: Run it and check that it fails**

Run: `cargo test -p acid-engine a_rack_swap_reports_where`
Expected: FAIL to compile (`set_rack` returns `Option<RackMap>`; there is
no field `map`).

- [ ] **Step 3: Implement**

1. `RackSwap::build`: keep its signature for now (Task 5 adds capacity).
   Set `into: Vec::with_capacity(order.len().max(MAX_MACHINES))`,
   `selected: machines.iter().map(|m| m.selected).collect()`,
   `map: vec![None; n]` and `tys: vec![None; n]`.
2. `check_rack(&self, swap: &mut RackSwap) -> bool`:
   - check `swap.map.len() == n`, `swap.tys.len() == n`,
     `swap.selected.len() == n` and `swap.into.capacity() >= n`, plus the
     existing checks;
   - write into `swap.map[i]` and `swap.tys[i]` instead of the local
     arrays, starting with `swap.map.fill(None); swap.tys.fill(None);`;
   - the `fits` closure reads `swap.tys`. Because the closure borrows
     `swap` immutably after the writes, compute `param_count` first and
     then build the closure.
3. `set_rack` returns `bool` and passes `&swap.map` and `&swap.selected`
   to `seq.rearrange`.
4. `Sequencer::rearrange`:
   - Keep the `[Track; MAX_MACHINES]` storage until Task 5.
   - `map: &[Option<usize>]`: read `map.get(pos).copied().flatten()`.
   - `selected: &[u8]`: read `selected.get(pos).copied().unwrap_or(0)`.
   - `new_of` becomes a `Sequencer` field, sized to the track count at
     construction (`vec![None; MAX_MACHINES]` for now): `fill(None)`,
     then write.
5. `Pending::remap(&mut self, new_of: &[Option<usize>])`.
6. Host `SetRack` arm: `if self.engine.set_rack(&mut swap) { rack_gen += 1 } else { rack_rejected = true }`.
7. Update the tests that built `[None; MAX_MACHINES]` maps or `&[0; MAX_MACHINES]`
   selections: pass `&vec![...]` of the rack's length.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace`
Expected: PASS, including `no_alloc` (`a_rack_swap_through_the_host_does_not_allocate`).

- [ ] **Step 5: Commit**

```bash
git commit -am "refactor(engine): rack swaps carry their map and scratch"
```

---

### Task 5: Rack capacity and growth

The sequencer's per-position storage and the engine's event buffer get
sized to a rack capacity. `RackSwap` carries a `Growth` whenever the new
rack needs more. The app tracks the engine's capacity and resizes the
status buffer pool to match.

**Files:**
- Modify: `crates/acid-engine/src/machine.rs` (add `RACK_CHUNK`,
  `rack_capacity`)
- Modify: `crates/acid-engine/src/sequencer.rs` (`tracks`, `prev_tracks`,
  `clocks`, `chosen`, `prev_chosen`, `new_of` → `Vec`s; `Pending` sized
  per capacity; `grow`; loops over `0..self.capacity()`)
- Modify: `crates/acid-engine/src/rack.rs` (`Growth`, `RackSwap::build`
  takes the engine capacity)
- Modify: `crates/acid-engine/src/engine.rs` (`capacity`, `from_project`,
  `event_reserve`, `max_param_count`, `check_rack`, `set_rack`,
  `song_fits`)
- Modify: `crates/app/src/ui/rack_sync.rs` (`engine_capacity`), `mod.rs`
  (field)
- Test: `engine.rs`, `sequencer.rs`, `no_alloc.rs`, `rack_sync` tests in
  `ui/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  // machine.rs
  /// Rack capacity grows in steps of this many machines.
  pub const RACK_CHUNK: usize = 16;
  /// The capacity an engine holds for a rack of `machines`.
  pub fn rack_capacity(machines: usize) -> usize {
      machines.div_ceil(RACK_CHUNK).max(1) * RACK_CHUNK
  }
  // sequencer.rs
  /// Events held back per rack position (see PENDING's old comment).
  pub(crate) const PENDING_PER_TRACK: usize = RATCHETS * 7 * 2;
  pub struct SeqBuffers {               // all len == capacity, Track::default()
      pub tracks: Vec<Track>, pub prev_tracks: Vec<Track>,
      pub clocks: Vec<(u64, f64)>, pub chosen: Vec<u8>, pub prev_chosen: Vec<u8>,
      pub new_of: Vec<Option<usize>>,
      pub pending: Vec<(f64, SeqEvent)>, // empty, capacity == capacity * PENDING_PER_TRACK
  }
  impl SeqBuffers { pub fn new(capacity: usize) -> Self; }  // allocates
  impl Sequencer {
      pub fn new(sample_rate: f32, capacity: usize) -> Self;
      pub fn capacity(&self) -> usize;                        // tracks.len()
      /// Moves state into `b` (bigger) and swaps; `b` then holds the old buffers.
      pub fn grow(&mut self, b: &mut SeqBuffers);
      pub fn tick_clocks(&self) -> &[(u64, f64)];
  }
  // rack.rs
  pub struct Growth { pub capacity: usize, pub seq: SeqBuffers, pub events: Vec<SeqEvent> }
  impl Growth { pub fn new(capacity: usize) -> Box<Growth>; }   // events: with_capacity(event_reserve(capacity))
  // RackSwap gains:
  pub capacity: usize,                  // engine capacity after this swap
  pub growth: Option<Box<Growth>>,      // Some iff capacity > engine's now
  impl RackSwap {
      pub fn build(project: &Project, current: &[MachineUid], sample_rate: f32,
                   engine_capacity: usize) -> Box<RackSwap>;
  }
  // engine.rs
  pub fn event_reserve(capacity: usize) -> usize;  // made pub(crate) for Growth
  impl Engine { pub fn capacity(&self) -> usize; }
  ```
  `Track` and `SeqEvent` become `pub(crate)` if they are not already
  (they are only named inside the crate). `SeqBuffers` and `Growth` are
  `pub`, with `#[doc(hidden)]` constructors usable by the app through
  `RackSwap::build`. The app never names `Track`.

- [ ] **Step 1: Write the failing tests**

`machine.rs` tests:
```rust
#[test]
fn rack_capacity_grows_in_chunks_of_sixteen() {
    assert_eq!([0, 1, 16, 17, 32, 33].map(rack_capacity), [16, 16, 16, 32, 32, 48]);
}
```

`engine.rs` tests:
```rust
#[test]
fn a_rack_edit_past_the_capacity_grows_it_and_keeps_playing_state() {
    let mut p = Project::default();
    while p.machines.len() < 16 {
        let m = p.new_machine(MachineType::Kit909);
        p.insert_machine(p.machines.len(), m).unwrap();
    }
    let mut e = Engine::from_project(SR, &p);
    assert_eq!(e.capacity(), 16);
    e.play();
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    for _ in 0..100 { e.process(&mut l, &mut r); }
    let before = e.positions();
    let mut q = p.clone();
    let m = q.new_machine(MachineType::Fm);
    q.insert_machine(q.machines.len(), m).unwrap();
    let mut swap = RackSwap::build(&q, &e.slot_uids(), SR, e.capacity());
    assert_eq!(swap.capacity, 32);
    assert!(swap.growth.is_some());
    assert!(e.set_rack(&mut swap));
    assert_eq!(e.capacity(), 32);
    let after = e.positions();
    assert_eq!(after.playhead[..16], before.playhead[..16], "tracks kept their place");
    assert_eq!(after.len(), 17);
    let g = swap.growth.as_ref().unwrap();
    assert_eq!(g.seq.tracks.len(), 16, "the old buffers come back for disposal");
}

#[test]
fn a_swap_beyond_the_engines_capacity_without_growth_is_refused() {
    let p = Project::default();
    let mut e = Engine::from_project(SR, &p);
    let mut q = p.clone();
    for _ in 0..12 {
        let m = q.new_machine(MachineType::Fm);
        q.insert_machine(q.machines.len(), m).unwrap();
    }
    let mut swap = RackSwap::build(&q, &e.slot_uids(), SR, e.capacity());
    swap.growth = None;
    assert!(!e.set_rack(&mut swap));
    assert_eq!(e.slot_uids(), p.rack_uids());
}
```

The second test needs 18 machines, which goes past 16. Until Task 6
lifts the cap, `insert_machine` refuses the 17th machine, so mark it
`#[ignore = "needs Task 6"]` and remove the attribute in Task 6. The
first test reaches 17 the same way, so ignore it too, and give Task 5 its
own growth test instead:

```rust
#[test]
fn growth_with_the_same_rack_moves_state_into_bigger_buffers() {
    let p = Project::default();
    let mut e = Engine::from_project(SR, &p);
    e.play();
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    for _ in 0..100 { e.process(&mut l, &mut r); }
    let before = e.positions();
    let mut q = p.clone();
    q.machines.swap(0, 1); // a real edit, so the swap is sent
    let mut swap = RackSwap::build(&q, &e.slot_uids(), SR, 16);
    swap.capacity = 32;
    swap.growth = Some(Growth::new(32));
    assert!(e.set_rack(&mut swap));
    assert_eq!(e.capacity(), 32);
    let after = e.positions();
    assert_eq!(after.playhead[0], before.playhead[1]);
}
```

`no_alloc.rs`:
```rust
#[test]
fn growing_the_rack_through_the_host_does_not_allocate() {
    let p = busy_project();
    let (mut host, mut tx, mut rx) = host::channel(Engine::from_project(SR, &p));
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let mut q = p.clone();
    q.machines.swap(0, 1);
    let mut swap = RackSwap::build(&q, &host.engine().slot_uids(), SR, 16);
    swap.capacity = 32;
    swap.growth = Some(Growth::new(32));
    tx.push(Command::SetRack(swap)).ok().unwrap();
    let n = count_allocs(|| {
        for _ in 0..400 { host.process(&mut l, &mut r); }
    });
    assert_eq!(n, 0);
    assert_eq!(host.engine().capacity(), 32);
    while rx.pop().is_ok() {}
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p acid-engine rack_capacity growth_with_the_same growing_the_rack`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the sequencer**

1. Replace the fixed arrays with `Vec`s owned through a `SeqBuffers`
   field `b`, or as separate fields, whichever keeps the diff smaller.
   Every `for pos in 0..MAX_MACHINES` becomes
   `for pos in 0..self.capacity()`. The bodies already check
   `banks.bank(pos)` and skip empty positions.
2. `set_clocks` in Pattern mode: `self.clocks.fill((tick, quarter))`.
   `tick`'s early return: `self.clocks.fill((0, quarter))`.
3. `tick_clocks` returns `&self.clocks`. In `engine.rs` the zip already
   works on a slice: borrow it before the loop, to keep `self.seq`
   borrowed separately from `self.slots`.
4. `rearrange`:
   ```rust
   self.prev_tracks.copy_from_slice(&self.tracks);
   self.prev_chosen.copy_from_slice(&self.chosen);
   ```
   Then read from `prev_*` where the code read `old_tracks`/`old_chosen`,
   and keep `o < self.capacity()` checks where it checked
   `o < MAX_MACHINES`.
5. `Pending`:
   - `Pending::new(capacity)` reserves `capacity * PENDING_PER_TRACK`;
   - `push` bounds on `self.evs.capacity()`;
   - keep the `debug_assert!`, against `capacity`.
6. `grow`:
   ```rust
   pub fn grow(&mut self, b: &mut SeqBuffers) {
       let n = self.capacity().min(b.tracks.len());
       b.tracks[..n].copy_from_slice(&self.tracks[..n]);
       b.clocks[..n].copy_from_slice(&self.clocks[..n]);
       b.chosen[..n].copy_from_slice(&self.chosen[..n]);
       b.pending.clear();
       let room = b.pending.capacity();
       b.pending.extend(self.pending.evs.drain(..).take(room));
       std::mem::swap(&mut self.tracks, &mut b.tracks);
       std::mem::swap(&mut self.prev_tracks, &mut b.prev_tracks);
       std::mem::swap(&mut self.clocks, &mut b.clocks);
       std::mem::swap(&mut self.chosen, &mut b.chosen);
       std::mem::swap(&mut self.prev_chosen, &mut b.prev_chosen);
       std::mem::swap(&mut self.new_of, &mut b.new_of);
       std::mem::swap(&mut self.pending.evs, &mut b.pending);
   }
   ```
   `extend` from `drain` stays within capacity and so does not allocate.
   Check with the `no_alloc` test.
7. Delete `PENDING` and the `const _` assert in `engine.rs`. Replace the
   assert with
   `const _: () = assert!(PENDING_PER_TRACK <= SLOT_EVENTS);`.

- [ ] **Step 4: Implement the engine and rack**

1. `event_reserve(capacity)`:
   ```rust
   pub(crate) fn event_reserve(capacity: usize) -> usize {
       max_param_count(capacity) + capacity * SLOT_EVENTS
   }
   ```
   `max_param_count(capacity)` replaces `MAX_MACHINES` with `capacity`.
   The per-type maximum is a `const`-free computation at construction,
   as today.
2. `from_project`:
   - `let capacity = rack_capacity(p.machines.len());`
   - `slots: Vec::with_capacity(capacity)`,
     `seq: Sequencer::new(sample_rate, capacity)`;
   - `load_project` reserves `event_reserve(capacity)`, drops
     `.take(MAX_MACHINES)`, and loops `for pos in 0..capacity` to set
     selections.
3. `Engine::capacity()` → `self.seq.capacity()`.
4. `check_rack`:
   - the capacity after the swap is
     `swap.growth.as_ref().map_or(self.capacity(), |g| g.capacity)`;
   - refuse when `n` is larger than it;
   - refuse when a growth is not well formed: `g.capacity` less than or
     equal to `self.capacity()`, `g.seq.tracks.len() != g.capacity` (and
     the same for every buffer), or
     `g.events.capacity() < event_reserve(g.capacity)`;
   - `fits_events` compares against the capacity the events buffer will
     have;
   - `song_fits(.., capacity, ..)` replaces `MAX_MACHINES` with that
     capacity.
5. `set_rack`, after `check_rack` passes and before moving slots:
   ```rust
   if let Some(g) = swap.growth.as_mut() {
       self.seq.grow(&mut g.seq);
       self.events.clear();
       std::mem::swap(&mut self.events, &mut g.events);
   }
   ```
   `into` already has room (checked).
6. `RackSwap::build(project, current, rate, engine_capacity)`:
   ```rust
   let n = project.machines.len();
   let capacity = rack_capacity(n).max(engine_capacity);
   let growth = (capacity > engine_capacity).then(|| Growth::new(capacity));
   // into: Vec::with_capacity(capacity)
   ```
   Drop the `.min(MAX_MACHINES)` truncation.
7. `CompiledSong::compile` drops `.take(MAX_MACHINES)`.

- [ ] **Step 5: Implement the app side**

1. Add a field to `SidAcidApp`:
   `pub(crate) engine_capacity: usize`. Set it to
   `rack_capacity(project.machines.len())` in `headless`/`new`,
   `rack_for_new_host` and `rack_for_replaced_engine`. These are the same
   moments the engine is built from the project.
2. In `sync_rack`, build with
   `RackSwap::build(&self.doc.project, &self.engine_rack, rate, self.engine_capacity)`.
   Then `self.engine_capacity = swap.capacity;` and
   `self.pool.set_capacity(swap.capacity);`.
3. Initialise the pool to `BufferPool::new(self.engine_capacity)`, and
   call `set_capacity` in the two `rack_for_*` functions.
4. App test (`ui/mod.rs` tests):
   ```rust
   #[test]
   fn the_app_tracks_the_engines_rack_capacity() {
       let mut app = SidAcidApp::headless(Document::new(Project::default()));
       assert_eq!(app.engine_capacity, 16);
       // With the cap still in place, check the value follows a swap's capacity.
       app.move_machine(SYNTH1, 3);
       assert_eq!(app.engine_capacity, 16);
   }
   ```
   Task 6 extends it past 16.

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS. The two `#[ignore]` tests are skipped for now.

- [ ] **Step 7: Commit**

```bash
git add -A crates
git commit -m "feat(engine): the rack's capacity grows through rack swaps"
```

---

### Task 6: Lift the cap

`MAX_MACHINES` goes. A rack is bounded only by the `u16` uid space.

**Files:**
- Modify: `crates/acid-engine/src/machine.rs:7-8` (delete the constant)
- Modify: `crates/acid-engine/src/project.rs:809-811` (drop the check),
  `:1299-1304` (rewrite the test)
- Modify: `crates/acid-engine/src/rack_edit.rs:51` (drop the length
  check), `:165-171` (`take_uid` fails at `u16::MAX`), `:201` (test)
- Modify: `crates/acid-engine/src/host.rs`, `engine.rs`, `mixer.rs`,
  `rack.rs`: any `MAX_MACHINES` left over
- Modify: `crates/app/src/document.rs:663, 674, 954`,
  `ui/rack_header.rs:44, 60, 377`, `ui/rack_sync.rs` (imports)
- Modify: `crates/acid-engine/tests/no_alloc.rs:148-235` (the
  sixteen-slot tests become forty-slot tests)
- Test: as listed

**Interfaces:**
- Produces:
  ```rust
  // rack_edit.rs
  /// A fresh uid, or None once every u16 uid has been used.
  fn take_uid(&mut self) -> Option<MachineUid>;
  pub fn new_machine(&mut self, ty: MachineType) -> Option<MachineEntry>;
  pub fn duplicate_machine(&mut self, uid: MachineUid) -> Option<MachineEntry>; // already Option
  // insert_machine: None only if the uid is taken.
  // rack_header.rs
  pub(crate) fn can_add(&self) -> bool;          // next_uid < u16::MAX
  pub(crate) fn rack_count_text(&self) -> String; // "1 machine" / "n machines"
  ```
  `new_machine` returning `Option` touches its callers: `document.rs`
  `add_machine` and the tests that call `p.new_machine(..)`. In tests,
  add `.unwrap()`.

- [ ] **Step 1: Write the failing tests**

`project.rs` (replacing the test at `:1299`):
```rust
#[test]
fn a_project_may_hold_forty_machines() {
    let mut p = Project::default();
    while p.machines.len() < 40 {
        let m = p.new_machine(MachineType::Fm).unwrap();
        p.insert_machine(p.machines.len(), m).unwrap();
    }
    p.validate().unwrap();
    let text = p.to_ron();
    assert_eq!(Project::from_ron(&text).unwrap().machines.len(), 40);
}

#[test]
fn uids_run_out_at_the_top_of_the_range() {
    let mut p = Project::default();
    p.next_uid = u16::MAX;
    assert!(p.new_machine(MachineType::Fm).is_none());
}
```

`rack_header.rs` tests (replacing the loop at `:377`):
```rust
#[test]
fn the_add_menu_stays_open_past_sixteen() {
    let mut app = SidAcidApp::headless(Document::new(Project::default()));
    for _ in 0..20 {
        assert!(app.can_add());
        app.add_machine(MachineType::Fm).unwrap();
    }
    assert_eq!(app.doc.project.machines.len(), 26);
    assert_eq!(app.rack_count_text(), "26 machines");
    assert_eq!(app.engine_capacity, 32);
}
```

Remove the `#[ignore]` from the two Task 5 engine tests and change their
`new_machine` calls to `.unwrap()`.

`no_alloc.rs`:
- rename `a_sixteen_slot_rack_does_not_allocate` →
  `a_forty_slot_rack_does_not_allocate`, looping `for (u, &ty) in (7..=40u16).zip(types)`
  with `p.next_uid = 41`;
- rename `busy_sixteen_slot_project` → `busy_forty_slot_project` with
  `for u in 7..=40u16`, `p.next_uid = 41`, and automation on
  `[8u16, 24, 40]`;
- update `a_sixteen_slot_song_with_automation_does_not_allocate` to the
  same.

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test --workspace forty a_project_may_hold uids_run_out the_add_menu_stays past_the_capacity beyond_the_engines`
Expected: FAIL (validation refuses 17+, `insert_machine` returns None).

- [ ] **Step 3: Implement**

1. Delete `MAX_MACHINES`, then let the compiler list what is left.
   Every remaining use has a replacement from Tasks 1–5:
   - `rack_capacity` for sizes;
   - `.len()` for loops;
   - `shown`/`master_meter` in the app.
2. `take_uid`:
   ```rust
   fn take_uid(&mut self) -> Option<MachineUid> {
       let uid = MachineUid(self.next_uid);
       self.next_uid = self.next_uid.checked_add(1)?;
       Some(uid)
   }
   ```
   `next_uid == u16::MAX` itself is never handed out, so the "next uid
   above every uid" invariant still holds.
3. `document.rs`:
   - `add_machine` returns None only when `new_machine` does;
   - `duplicate_machine` drops its length check;
   - the `InsertMachine` undo assertion keeps only
     `p.machine(uid).is_none()`.
4. `rack_header.rs`:
   ```rust
   pub(crate) fn can_add(&self) -> bool {
       self.doc.project.next_uid < u16::MAX
   }
   pub(crate) fn rack_count_text(&self) -> String {
       match self.doc.project.machines.len() {
           1 => "1 machine".into(),
           n => format!("{n} machines"),
       }
   }
   ```
   Update the doc comment "The Add bar's count" to match. Where
   `add_machine` reports a full rack, the message becomes "the rack has no
   free machine ids" (verbatim from the spec) (`grep -n "full" crates/app/src/ui/*.rs` to find
   it).

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS and clean. `grep -rn MAX_MACHINES crates` prints nothing.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "feat: the rack has no machine limit"
```

---

### Task 7: The `SystemMemory` source

This task adds a testable source of system RAM figures, the `sysinfo`
dependency and the meter's level logic, with no UI yet.

**Files:**
- Modify: `Cargo.toml` (workspace deps), `crates/app/Cargo.toml`
- Create: `crates/app/src/sysmem.rs`
- Modify: `crates/app/src/main.rs` (`mod sysmem;`)
- Test: `sysmem.rs` tests

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct MemSnapshot { pub total: u64, pub available: u64 }  // bytes
  pub trait SystemMemory { fn snapshot(&mut self) -> Option<MemSnapshot>; }
  pub struct SysinfoMemory { sys: sysinfo::System }
  impl SysinfoMemory { pub fn new() -> Self; }
  impl SystemMemory for SysinfoMemory { … }  // refresh_memory, then total_memory/available_memory
  #[cfg(test)] pub struct FakeMemory(pub Option<MemSnapshot>);
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum MemLevel { Normal, Low, Critical }   // shown as normal / amber / red
  /// The level for `available` free bytes out of `total`.
  pub fn level(total: u64, available: u64) -> MemLevel;
  pub const GIB: u64 = 1 << 30;
  pub const MIB: u64 = 1 << 20;
  ```

- [ ] **Step 1: Add the dependency**

In the root `Cargo.toml` `[workspace.dependencies]`:
```toml
sysinfo = { version = "0.39", default-features = false, features = ["system"] }
```
In `crates/app/Cargo.toml` `[dependencies]`: `sysinfo.workspace = true`.

Run: `cargo build -p sidacid`
Expected: builds. Then check the method names against the version that
was fetched:
`cargo doc -p sysinfo --no-deps 2>/dev/null; grep -rn "pub fn available_memory\|pub fn total_memory\|pub fn refresh_memory" ~/.cargo/registry/src/*/sysinfo-0.39*/src/common/system.rs`.
Since 0.30 these return **bytes**. If the grep shows otherwise, follow
the source.

- [ ] **Step 2: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_the_spec_thresholds() {
        let t = 16 * GIB;
        // 25 % of 16 GB = 4 GB > 2 GB; 10 % = 1.6 GB > 512 MB.
        assert_eq!(level(t, 5 * GIB), MemLevel::Normal);
        assert_eq!(level(t, 4 * GIB - 1), MemLevel::Low);
        assert_eq!(level(t, 1_600 * MIB), MemLevel::Critical);
        // Small machine: 4 GB total -> floors 2 GB and 512 MB win.
        let s = 4 * GIB;
        assert_eq!(level(s, 2 * GIB + 1), MemLevel::Normal);
        assert_eq!(level(s, 2 * GIB - 1), MemLevel::Low);
        assert_eq!(level(s, 512 * MIB - 1), MemLevel::Critical);
    }

    #[test]
    fn sysinfo_reports_something_sane() {
        let s = SysinfoMemory::new().snapshot().expect("this OS reports memory");
        assert!(s.total > 256 * MIB && s.available <= s.total);
    }
}
```

- [ ] **Step 3: Run them and check that they fail**

Run: `cargo test -p sidacid sysmem::`
Expected: FAIL to compile.

- [ ] **Step 4: Implement**

```rust
//! System RAM figures for the memory meter, behind a trait so tests can
//! fake a machine that is running short.

pub const MIB: u64 = 1 << 20;
pub const GIB: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemSnapshot {
    pub total: u64,
    pub available: u64,
}

pub trait SystemMemory {
    fn snapshot(&mut self) -> Option<MemSnapshot>;
}

pub struct SysinfoMemory {
    sys: sysinfo::System,
}

impl SysinfoMemory {
    pub fn new() -> Self {
        Self { sys: sysinfo::System::new() }
    }
}

impl SystemMemory for SysinfoMemory {
    fn snapshot(&mut self) -> Option<MemSnapshot> {
        self.sys.refresh_memory();
        let total = self.sys.total_memory();
        (total > 0).then(|| MemSnapshot {
            total,
            available: self.sys.available_memory().min(total),
        })
    }
}

#[cfg(test)]
pub struct FakeMemory(pub Option<MemSnapshot>);

#[cfg(test)]
impl SystemMemory for FakeMemory {
    fn snapshot(&mut self) -> Option<MemSnapshot> {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemLevel {
    Normal,
    Low,
    Critical,
}

/// Amber below 25 % of total or 2 GB, red below 10 % or 512 MB
/// (whichever is larger in each case).
pub fn level(total: u64, available: u64) -> MemLevel {
    let low = (total / 4).max(2 * GIB);
    let critical = (total / 10).max(512 * MIB);
    if available < critical {
        MemLevel::Critical
    } else if available < low {
        MemLevel::Low
    } else {
        MemLevel::Normal
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sidacid sysmem::`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/app
git commit -m "feat(app): read system RAM for the memory meter"
```

---

### Task 8: Remove the sample pool; add the project-load guard

**Files:**
- Modify: `crates/acid-engine/src/sampler/mod.rs:24-26` (delete
  `SAMPLE_POOL`)
- Modify: `crates/acid-engine/src/sampler/slot.rs:110-160` (delete
  `with_budget`/`BUDGET`; keep `MAX_BYTES`)
- Modify: `crates/acid-engine/src/project.rs:670` (the parse-time budget
  that used `SAMPLE_POOL / 2`), `:824-831` (the pool check), `:1533` (the
  test)
- Modify: `crates/app/src/ui/sampler_load.rs` (drop `pool_message` and
  the free-space check), `rack_sync.rs:41-48` (duplicate check)
- Modify: `crates/app/src/ui/files.rs` (project open: the guard)
- Test: `project.rs`, `ui/mod.rs:2395-2420` and `:2760-2770` (rewrite)

**Interfaces:**
- Produces:
  ```rust
  // project.rs
  /// Decoded bytes (f32, as `memory_bytes` counts) of every sample in the project.
  pub fn sample_bytes(&self) -> usize;
  // app, files.rs (or wherever project open lives)
  /// Refuses a project whose decoded audio exceeds the system's available RAM.
  fn check_fits(p: &Project, mem: Option<MemSnapshot>) -> Result<(), String>;
  ```
  Message, verbatim from the spec:
  `"Not enough memory to open this project: {audio} of audio, {free} free"`.
  Both figures go through `format_bytes` (Task 9's helper; define it here
  in `sysmem.rs` as `pub fn format_bytes(b: u64) -> String`, printing
  `"2.1 GB"` from 1 GB up and `"300 MB"` below).

- [ ] **Step 1: Read what the parse-time budget at `project.rs:670` guards**

Run: `sed -n 655,690p crates/acid-engine/src/project.rs`

It bounds how much a single project file may inflate while parsing (a
decompression bomb). Keep a bound, without the pool: use
`MAX_BYTES` × the number of sampler slots in the file, the most a
well-formed file can hold. If the code says otherwise, ask before
changing its meaning.

- [ ] **Step 2: Write the failing tests**

`project.rs`:
```rust
#[test]
fn a_project_may_hold_more_than_256_mb_of_audio() {
    let mut p = Project::default();
    fill_sampler(&mut p, MachineUid(6), 300 * 1024 * 1024);
    p.validate().unwrap();
    assert_eq!(p.sample_bytes(), 300 * 1024 * 1024);
}
```
`fill_sampler` is the existing test helper at `project.rs:1478`. One
Sampler's 16 slots hold up to about 368 MB of it.

App (`files.rs` tests). `sampler_with_frames` in `ui/mod.rs` tests
writes mono frames at 4 decoded bytes each. Make it `pub(crate)` under
`#[cfg(test)]` so `files.rs` can use it.

```rust
#[test]
fn opening_a_project_bigger_than_free_memory_is_refused() {
    use crate::sysmem::{GIB, MemSnapshot};
    let none = Project::default();
    let tiny = Some(MemSnapshot { total: GIB, available: 0 });
    assert!(check_fits(&none, tiny).is_ok(), "no audio always fits");
    let mut app = SidAcidApp::headless(Document::new(Project::default()));
    crate::ui::tests::sampler_with_frames(&mut app, 1 <<20); // 4 MB decoded
    let big = &app.doc.project;
    let err = check_fits(big, Some(MemSnapshot { total: GIB, available: 1 << 20 })).unwrap_err();
    assert_eq!(err, "Not enough memory to open this project: 4 MB of audio, 1 MB free");
    assert!(check_fits(big, None).is_ok(), "unknown memory never refuses");
}
```

Rewrite `a_load_past_the_memory_pool_is_refused` (`ui/mod.rs:2400`) as
`a_load_bigger_than_256_mb_total_is_accepted`, asserting that the load
succeeds and `view.message` is `None`. Delete the test at `:2764`, or
rewrite it the same way.

- [ ] **Step 3: Run them and check that they fail**

Run: `cargo test --workspace more_than_256 bigger_than_free 256_mb_total`
Expected: FAIL.

- [ ] **Step 4: Implement**

1. Delete `SAMPLE_POOL`, `with_budget`, `BUDGET` and the budget branch in
   `decode`. Keep the `len > MAX_BYTES` check.
2. `Project::sample_bytes` sums `memory_bytes()` over the samplers. It
   replaces `sampler_load::sample_memory`; keep that as a thin wrapper
   only if many callers use it.
3. `validate` drops the pool check.
4. `sampler_load.rs` drops the free-space refusal and `pool_message`.
   `rack_sync.rs::duplicate_machine` drops its pool check. Task 9 adds the
   warning in its place.
5. `check_fits` sits in the project-open path. Find it with
   `grep -n "fn open\|from_ron\|fn load_project" crates/app/src/ui/files.rs`.
   It runs after parsing and before replacing the document. On `Err`, set
   `view.message` and keep the current project.

- [ ] **Step 5: Run the tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS. `grep -rn SAMPLE_POOL crates` prints nothing.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat: samples share system memory instead of a 256 MB pool"
```

---

### Task 9: The meter, its breakdown and the warnings

**Files:**
- Modify: `crates/acid-engine/src/machine.rs` (`memory_estimate`)
- Create: `crates/acid-engine/tests/slot_memory.rs` (checks the
  estimates)
- Modify: `crates/app/src/ui/perf.rs:191-280` (MEM text, colour, hover;
  remove `Smp`)
- Create: `crates/app/src/ui/mem_warn.rs` (estimate + modal)
- Modify: `crates/app/src/ui/mod.rs` (fields `sysmem: Box<dyn SystemMemory>`,
  `mem: Option<MemSnapshot>`, `pending_warn: Option<MemWarn>`; draw the
  modal), `rack_sync.rs` (`add_machine`, `duplicate_machine` go through
  the check), `sampler_load.rs` (sample load goes through the check)
- Test: `perf.rs`, `mem_warn.rs` tests

**Interfaces:**
- Produces:
  ```rust
  // machine.rs
  impl MachineType {
      /// Rough heap + inline bytes one machine of this type takes in the
      /// engine (DSP state, buffers, channel strip), excluding samples.
      pub const fn memory_estimate(self) -> usize;
  }
  // mem_warn.rs
  pub(crate) enum Pending { Add(MachineType), Duplicate(MachineUid),
                            LoadSample { uid: MachineUid, slot: usize, name: String,
                                         ext: Option<String>, bytes: Vec<u8> },
                            /// A parsed project that passed `check_fits`, with where it came from.
                            Open { project: Box<Project>, path: std::path::PathBuf } }
  pub(crate) struct MemWarn { pub action: Pending, pub free_after: u64 }
  /// Whether an action needing `extra` bytes would leave the system in the red.
  pub(crate) fn warn_for(extra: u64, mem: Option<MemSnapshot>) -> Option<u64>; // free_after
  impl SidAcidApp {
      /// Runs `action` now, or holds it behind the warning.
      pub(crate) fn with_memory_check(&mut self, extra: u64, action: Pending);
      pub(crate) fn memory_warning_ui(&mut self, ctx: &egui::Context);
  }
  ```
  - `warn_for` returns `Some(available - extra)` when
    `level(total, available.saturating_sub(extra)) == Critical`. It returns
    `None` when there is no snapshot or the result is not critical.
  - The extra bytes per action:
    - Add: `ty.memory_estimate()`;
    - Duplicate: `estimate + sampler memory_bytes()`;
    - LoadSample: `decoded_bytes * 2`, where `decoded_bytes` is the length
      of the decoded `SlotData` and the doubling allows for the undo copy;
    - Open: `project.sample_bytes()` plus the sum of `memory_estimate`
      over its rack.
    For a sample load, decode first, which is already done before the
    check today in `sampler_load.rs`, then check, then apply.

- [ ] **Step 1: Write the failing estimate test** (`tests/slot_memory.rs`)

```rust
//! `MachineType::memory_estimate` should be within 2× of what building a
//! slot actually allocates.
use acid_engine::machine::{MachineType, MachineUid};
use acid_engine::project::MachineEntry;
use acid_engine::rack::Slot;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static BYTES: AtomicUsize = AtomicUsize::new(0);
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        BYTES.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p, l) } }
}
#[global_allocator]
static A: Counting = Counting;

#[test]
fn estimates_are_within_two_times_of_real_allocations() {
    for ty in MachineType::ALL {
        let entry = MachineEntry::new(MachineUid(1), ty, "x".into());
        let before = BYTES.load(Ordering::Relaxed);
        let slot = Slot::from_entry(&entry, 48_000.0);
        let used = BYTES.load(Ordering::Relaxed) - before + std::mem::size_of::<Slot>();
        let est = ty.memory_estimate();
        assert!(est * 2 >= used && est <= used * 2, "{ty:?}: estimate {est}, measured {used}");
        drop(slot);
    }
}
```

This test is single-threaded on purpose. Run it with
`-- --test-threads=1` if it is flaky; there is one test in the file
anyway.

- [ ] **Step 2: Run it and check that it fails, then measure**

Run: `cargo test -p acid-engine --test slot_memory -- --nocapture`
Expected: FAIL to compile (`no method memory_estimate`).

Add a temporary `memory_estimate` that returns 1. The assertion messages
then print each type's measured bytes. Set each constant to its measured
value rounded up to the next 4 KiB, and note the date measured in a
comment. Run again: PASS.

- [ ] **Step 3: Write the failing app tests**

`mem_warn.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysmem::{GIB, MIB, MemSnapshot};

    #[test]
    fn a_warning_only_when_the_action_would_reach_red() {
        let m = Some(MemSnapshot { total: 4 * GIB, available: 600 * MIB });
        assert_eq!(warn_for(50 * MIB, m), None, "550 MB is still above 512 MB");
        assert_eq!(warn_for(100 * MIB, m), Some(500 * MIB));
        assert_eq!(warn_for(100 * MIB, None), None);
    }

    #[test]
    fn cancel_leaves_the_rack_alone_and_go_ahead_adds() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.sysmem = Box::new(crate::sysmem::FakeMemory(Some(MemSnapshot {
            total: 4 * GIB,
            available: 512 * MIB, // already red: any add warns
        })));
        app.refresh_memory(0.0);
        app.with_memory_check(1, Pending::Add(MachineType::Fm));
        assert!(app.pending_warn.is_some());
        assert_eq!(app.doc.project.machines.len(), 6);
        app.resolve_memory_warning(false);
        assert!(app.pending_warn.is_none());
        assert_eq!(app.doc.project.machines.len(), 6);
        app.with_memory_check(1, Pending::Add(MachineType::Fm));
        app.resolve_memory_warning(true);
        assert_eq!(app.doc.project.machines.len(), 7);
    }
}
```

`perf.rs`:
```rust
#[test]
fn mem_text_and_colour_follow_the_system() {
    use crate::sysmem::{GIB, MIB, MemSnapshot};
    let normal = egui::Color32::WHITE;
    let s = MemSnapshot { total: 16 * GIB, available: 10 * GIB };
    assert_eq!(mem_text(412 * MIB, Some(s)), "MEM 412 MB / 16 GB");
    assert_eq!(mem_colour(Some(s), normal), normal);
    let low = MemSnapshot { available: 3 * GIB, ..s };
    assert_eq!(mem_colour(Some(low), normal), theme::QUEUED);
    let red = MemSnapshot { available: GIB, ..s };
    assert_eq!(mem_colour(Some(red), normal), theme::ACCENT);
    assert_eq!(mem_text(412 * MIB, None), "MEM 412 MB");
}
```

- [ ] **Step 4: Run them and check that they fail**

Run: `cargo test -p sidacid mem_warn:: mem_text_and_colour`
Expected: FAIL to compile.

- [ ] **Step 5: Implement**

1. `perf.rs`:
   - Add `pub(crate) fn mem_text(app_bytes: u64, sys: Option<MemSnapshot>) -> String`.
     It uses `format_bytes` for the total, which prints whole GB as
     `"16 GB"` and otherwise one decimal place.
   - Add
     `pub(crate) fn mem_colour(sys: Option<MemSnapshot>, normal: Color32) -> Color32`,
     mapping `Normal` → `normal`, `Low` → `theme::QUEUED` and
     `Critical` → `theme::ACCENT`, the same colours the CPU readout uses
     for busy and overloaded.
   - The hover text, verbatim layout from the spec:
     ```
     App: 412 MB · System free: 9.8 GB of 16 GB
     Samples 180 MB · Machines ~40 MB · Other 192 MB
     ```
     - Samples is `project.sample_bytes()`.
     - Machines is the sum of `memory_estimate` over the rack.
     - Other is `app.saturating_sub(samples + machines)`.
   - Delete the `Smp` label and its imports.
2. `SidAcidApp`:
   - `sysmem: Box<dyn SystemMemory>`, set to `SysinfoMemory::new()` in
     `new` and `headless`. Tests swap it.
   - `mem: Option<MemSnapshot>` and
     `refresh_memory(now)`, which reads `self.sysmem.snapshot()` at most
     once a second (the `MEMORY_EVERY` cadence).
   - Call `refresh_memory` from `perf_readout`, next to `tick_memory`.
3. `mem_warn.rs`:
   - `with_memory_check(extra, action)`: refresh first if `mem` is
     `None`. If `warn_for` returns `Some(free)`, set
     `pending_warn = Some(MemWarn { action, free_after: free })`.
     Otherwise run it now.
   - `resolve_memory_warning(go: bool)` runs or drops the pending action.
   - `run(action)` calls the existing functions: `add_machine(ty)`,
     `duplicate_machine(uid)`, `load_sample_bytes(..)` without the
     check. Rename the existing ones `*_now` and make the public names go
     through the check, so every UI entry point is covered without hunting
     for call sites.
   - `memory_warning_ui`: an `egui::Window`, titled "Low memory",
     collapsible(false), resizable(false), anchored at the centre. Its
     text, verbatim from the spec: `"Adding this leaves about {free} of
     free memory. The system may slow down or the app may be stopped."`
     It has two buttons, **Go ahead** and **Cancel**. **Cancel** has
     keyboard focus when the window opens (`response.request_focus()` on
     first show), and Esc cancels. Draw it from the main `update` after
     the panels, while `pending_warn.is_some()`.
   - While the warning is open, keys go to egui. Follow the existing rule
     for dialogs: commit `32c7bf8` "Tab is egui's while typing or with a
     dialog open". Find the predicate it added
     (`grep -n "dialog_open\|fn modal" crates/app/src/ui/*.rs`) and
     include `pending_warn.is_some()` in it.
4. `files.rs` project open: after `check_fits` passes,
   `self.with_memory_check(extra, Pending::Open { project, path })`.
   `run` replaces the document the way the open path does today. Move
   that code into an `open_project_now(project, path)` that both use.
5. `sampler_load.rs`: after decoding,
   `self.with_memory_check(decoded.memory_bytes() as u64 * 2, Pending::LoadSample { .. })`.

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS and clean.

- [ ] **Step 7: Check in the running app**

Use the `run` skill and the `sidacid-screenshots` memory:
- add 20 machines: there is no limit, and the count reads "26 machines";
- the MEM readout shows `/ <total> GB`, and its hover shows the
  breakdown;
- to see the warning without exhausting RAM, run a debug build with the
  env var `SIDACID_FAKE_FREE_MB=400`. Read it in `SysinfoMemory::snapshot`
  under `#[cfg(debug_assertions)]` to override `available`. Add one
  machine: the warning appears; Cancel leaves the rack as it was; Go
  ahead adds the machine.
- take screenshots of the readout, the hover and the warning to show
  the user.

- [ ] **Step 8: Commit**

```bash
git add -A crates
git commit -m "feat(app): a memory meter against system RAM, with a warning before it runs short"
```

---

### Task 10: Docs

**Files:**
- Modify: `docs/manual/rack-and-patterns.md` (no rack limit; the count
  text; link to the meter)
- Modify: `docs/manual/sampler.md` (the `### Limits` table: drop the
  "Sample memory" row and the paragraph on it, at about `:58-67`. Say that
  samples count toward the MEM meter and that a load which would run the
  system short asks first)
- Modify: `docs/manual/mixer-and-effects.md:140-150` (performance readout:
  MEM text, colours, hover, warning; remove the `Smp` row)
- Modify: `docs/manual/keyboard-reference.md` (only if the warning adds
  keys: Esc cancels it)
- Modify: `docs/superpowers/specs/2026-10-14-modular-rack-design.md`,
  `2026-10-12-fm-sampler-design.md` (a one-line note at the top: "The
  16-machine cap and the 256 MB sample pool were lifted by
  2026-10-17-open-rack-memory-meter-design.md.")
- Modify: `docs/superpowers/specs/2026-10-17-open-rack-memory-meter-design.md`
  (record the planning deviation: slot-held meters and CPU time, `Growth`
  for sequencer buffers only)

- [ ] **Step 1: Read `docs/manual/STYLE.md`** and follow it: plain
  wording, no product names, and tables in the same shape as the existing
  ones.

- [ ] **Step 2: Make the edits listed above.** Check every number
  against the code: the thresholds (25 % / 2 GB, 10 % / 512 MB), the
  capacity step (16) and the warning text.

- [ ] **Step 3: Check the links**

Run: `grep -rn "sampler.md#limits\|#the-performance-readout" docs/manual`.
Every anchor still exists.

- [ ] **Step 4: Commit**

```bash
git add docs
git commit -m "docs: no rack limit; the memory meter replaces the sample pool"
```

- [ ] **Step 5: Finish the branch**

Use superpowers:finishing-a-development-branch. Per the user's standing
preference, offer in one line to fast-forward `open-rack` into `master`
and push, and still ask before doing it.
