# SidAcid: Open Rack and Memory Meter — Design

Date: 2026-10-17
Status: approved in brainstorming, pending spec review

## Goal

Remove the 16-machine rack cap (`MAX_MACHINES`). In its place, the
transport bar's memory readout measures the app against the system's RAM
and warns, but never blocks, when an action would leave the system short.
The separate 256 MB sample pool goes away: samples count toward the same
meter.

## Decisions

| Question | Decision |
|---|---|
| Rack cap | None. Per-position storage grows; the audio thread still never allocates |
| What the meter measures | The app's resident memory against total system RAM, coloured by the system's available RAM |
| Blocking | Never. Actions that would push the meter into red show a warning, and the user can carry on |
| Sample pool (256 MB) | Removed; samples count toward the meter |
| System RAM source | The `sysinfo` crate (cross-platform), refreshed once a second with the existing `MEM` tick |

Out of scope: a per-machine memory column in the mixer, swapping samples
to disk, and any CPU-based cap (the CPU readout and dropout counter already
cover that).

## 1. Engine: growable rack

`MAX_MACHINES` goes. Each `[T; MAX_MACHINES]` becomes a `Vec<T>` (or a
boxed slice). Its length is the engine's **rack capacity**, a multiple of
16 at least as large as the rack. The capacity is chosen on the UI thread
and arrives with the buffers that use it.

- `RackSwap` gains a `capacity`, plus every per-position buffer the engine
  holds at that capacity: `into` (slots), `slot_ns`, the event and param
  buffers sized today from `MAX_MACHINES` (`engine.rs` `SLOT_EVENTS`, the
  param count), the mixer's per-channel state, and `selected`. When the
  new rack fits the engine's current capacity, the swap carries the
  smaller set it carries today. When it does not, `RackSwap::build`
  allocates the next multiple of 16, and `Engine::set_rack` moves the
  old buffers back out in the swap for disposal on the UI thread, as it
  does today with the old slot `Vec`.
- `RackMap` becomes a `Vec<Option<usize>>` built with the swap.
  `remap_positions` and `remap_meters` work on slices.
- `Engine::set_rack` still refuses malformed swaps (`rack_rejected`).
  "Malformed" now means buffers shorter than the stated capacity, or a
  rack longer than the capacity, instead of a rack longer than 16.
- `ReplaceEngine` (project load) builds the engine at the capacity the
  project needs.

### Status

`Status` is `Copy` and travels by value through the `rtrb` event ring.
Its per-position arrays (`playhead`, `patterns`, `queued`, `meters`,
`slot_cpu`) move into a `PositionStatus` whose `Vec`s are allocated by the
UI thread:

- `Status` keeps its scalar fields and gains `positions:
  Option<Box<PositionStatus>>` (it stops being `Copy` and becomes `Clone`).
- The UI keeps a small pool of empty boxes, sized to the current capacity.
  It sends them to the audio thread with `Command::StatusBuffers` (a few at
  a time), and recycles each one after reading it.
- The audio thread fills a box when it has one of the right capacity, and
  sends `positions: None` when it has none. The UI then keeps its last
  per-position values for that frame. A box of the wrong capacity (from
  before a resize) goes back to the UI unfilled with the next status, so
  it is dropped on the UI thread.
- The `meters` entry for the master channel moves to a scalar
  `master_meter` field.

### Project and validation

- `Project::validate` drops the more-than-16 check. Duplicate uids and
  `next_uid` checks stay. The uid space (`u16`) is the only bound;
  `add_machine` refuses with "the rack has no free machine ids" once
  `next_uid` would wrap.
- `migrate.rs` loops over the machines it finds instead of `0..16`.
- `SAMPLE_POOL` and its checks go. That means
  `project.rs` (`sampler_bytes > SAMPLE_POOL`, the load budget at
  `SAMPLE_POOL / 2`), `sampler_load.rs`, `rack_sync.rs` and the
  `with_budget` decode guard in `slot.rs`. The per-sample guards stay:
  `MAX_SECONDS` and `MAX_BYTES` (30 s at 192 kHz stereo). A project file
  can still not inflate past these limits per sample.
- Project loading gets a new guard in their place: the decoded audio of a
  project may not exceed the system's available RAM at load time. It fails
  with "Not enough memory to open this project: 2.1 GB of audio, 1.4 GB
  free".

### The `no_alloc` test

It still renders racks without allocating. It gains cases for:

- a rack of 40 machines;
- a rack edit that grows the capacity from 16 to 32;
- a status with no buffer from the pool.

## 2. App: no cap in the UI

- `rack_header.rs`: **+ Add machine** is always enabled. The `n / 16`
  count becomes `n machines`.
- `document.rs`: `add_machine`, duplicate and paste lose their cap checks.
- `rack_sync.rs` and `mixer_panel.rs` work on whatever length the status
  reports.
- The perf hover's `Top:` line already lists by uid and needs no change.

## 3. The memory meter

The transport bar's `MEM 412 MB` becomes **`MEM 412 MB / 16 GB`**:

- The first figure is the app's resident memory (`memory-stats`, as
  today). The second is total system RAM (`sysinfo`).
- Its colour comes from the system's **available** RAM:
  - **amber** below 25 % of total or 2 GB, whichever is larger;
  - **red** below 10 % of total or 512 MB, whichever is larger.
  
  These thresholds are constants in `perf.rs`.
- On hover it shows:

  ```
  App: 412 MB · System free: 9.8 GB of 16 GB
  Samples 180 MB · Machines ~40 MB · Other 192 MB
  ```

  "Samples" is the decoded audio of every Sampler and Pad Kit.
  "Machines" is the sum of `Slot::memory_estimate()`, a new per-type
  constant (struct size plus fixed buffers such as delay lines).
  "Other" is the rest.
- The `Smp n/256 MB` readout is removed.

### Warnings

Before an action that adds memory, the app estimates its cost and checks
it against the available RAM:

- adding or duplicating a machine (`memory_estimate`);
- loading a sample or kit (decoded size × 2, which allows for the undo
  copy);
- opening a project (its decoded audio).

If the estimate would leave less than the red threshold, the action asks
first, in a small modal: "Adding this leaves about 300 MB of free memory.
The system may slow down or the app may be stopped." with **Go ahead** and
**Cancel**. The default button is **Cancel**. Nothing is ever refused
outright, except the per-sample guards and the project-load guard above.

## 4. Tests

- Engine:
  - a 40-machine rack plays, and a rack edit grows its capacity from 16 to
    32 without losing playheads, patterns or meters;
  - a malformed swap (buffers shorter than the capacity) is refused;
  - `remap_positions` handles different lengths;
  - `no_alloc` covers the cases in section 1.
- Project:
  - `validate` accepts 40 machines;
  - a project with more than 256 MB of audio loads (with available RAM
    faked high), and the load guard refuses when available RAM is faked
    low.
- App:
  - **+ Add machine** stays enabled past 16;
  - the meter's colour follows faked available-RAM values at each
    threshold;
  - the warning appears for an add that crosses red, and **Cancel** leaves
    the project unchanged.

  System memory is read through a small `SystemMemory` trait so tests can
  fake it.
- Remove the tests that assert the cap or the 256 MB pool
  (`project.rs:1299`, `rack_header.rs:377`, `ui/mod.rs:2401`/`2764`,
  `document.rs` cap tests) or rewrite them for the new behaviour.

## 5. Docs

- `rack-and-patterns.md`: no rack limit; mention the meter.
- `sampler.md` Limits: the pool row goes. The text points to the meter
  and the warning.
- `mixer-and-effects.md` performance readout: the new MEM text, its
  colours and hover; `Smp` removed.
- The rack and sampler specs (`2026-10-14-modular-rack-design.md`,
  `2026-10-12-fm-sampler-design.md`) get a note that this spec lifts the
  cap and the pool.

## 6. Build order

1. `PositionStatus` and the status buffer pool, while the cap is still 16.
2. Per-position engine buffers become `Vec`s sized from `RackSwap`; growth
   in steps of 16.
3. Lift the cap in project, migrate and app; remove `SAMPLE_POOL`.
4. `sysinfo`, `SystemMemory`, the meter, the hover breakdown and
   `memory_estimate`.
5. Warnings and the project-load guard.
6. Docs, then screenshots in the running app with 20+ machines.
