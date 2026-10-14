# Modular rack — design

Date: 2026-10-14. Step 3 of 3 in the "modular SidAcid" series (1: performance
meter, merged; 2: audio driver/device settings, merged 9cc7f9a).

## Goal

Replace the fixed six-machine rack (2×303, 808, 909, FM, Sampler) with a
rack of up to **16 machines of any mix** of five types — 303 Synth, 808 Kit,
909 Kit, FM, Sampler — that the user can add, remove, rename, reorder and
duplicate. Show CPU per machine. Share one 256 MB sample pool across all
samplers.

## Decisions (from brainstorming)

| Question | Decision |
|---|---|
| What may the rack hold | Any mix of the five types, cap 16 machines |
| New project rack | Today's six (303, 303, 808, 909, FM, Sampler); old projects load as those six |
| Machine operations | Add, remove, rename, reorder, duplicate — all undoable |
| Sample memory | One shared pool across all samplers, 256 MB (was 64 MB for the one sampler) |
| Per-machine CPU | Shown on each machine's panel header; transport CPU hover lists the top machines |
| Architecture | Slot rack with stable machine IDs and a `Machine` enum (not trait objects, not 16-of-each preallocation) |
| Delivery | Three plans: (a) engine + project model + migration, (b) rack management UI, (c) per-machine CPU + shared sample pool |

## Out of scope

New machine types; per-machine effects inserts; dragging machines with the
mouse (↑/↓ buttons only); a user-saved default rack; MIDI.

## 1. Machine identity and project model

### IDs

- `MachineUid(u16)`, allocated from `Project::next_uid` when a machine is
  created; never reused within a project. Rack position is the index in
  `Project::machines` and is independent of the uid.

### Project format v3

```rust
struct Project {
    version: u32,                         // 3
    // bpm, swing, other existing global fields unchanged
    machines: Vec<MachineEntry>,          // rack order, 0..=16 entries
    next_uid: u16,
    globals: BTreeMap<GlobalParam, f32>,  // FX + master params
    song: Song,
}
struct MachineEntry {
    uid: MachineUid,
    name: String,                         // user-editable
    selected: u8,                         // selected pattern
    channel: BTreeMap<ChannelParam, f32>, // the machine's mixer strip
    body: MachineBody,
}
enum MachineBody {
    Synth(SynthMachine),     // its params + 128 patterns
    Kit(DrumMachine),        // carries KitKind::{K808, K909}; params + patterns
    Fm(FmMachine),
    Sampler(SamplerMachine), // slots + embedded audio
}
```

- Each per-type machine struct owns its parameter map (today's global
  `params` map is split per machine) beside its patterns.
- An empty rack is valid (the song plays silence).
- Default names: the type label plus the next free number for that type —
  "Synth 3", "808 2", "909 2", "FM 2", "Sampler 2". Migrated and new-project
  machines are named "Synth 1", "Synth 2", "808", "909", "FM", "Sampler".
- `validate` rejects: more than 16 machines; duplicate uids; `next_uid` ≤
  any existing uid; song cells or automation lanes naming a uid not in the
  rack; plus today's per-machine checks.

### Parameters

```rust
enum ParamId {
    Machine(MachineUid, MachineParam),
    Fx(FxParam),
    Master(MasterParam),
}
enum MachineParam {
    Synth(SynthParam), Drum(u8, DrumParam), Kit(KitParam),
    Channel(ChannelParam), Fm(FmParam), FmOp(u8, OpParam),
    Sampler(SamplerParam), SamplerSlot(u8, SlotParam),
}
```

- Each machine type has its own local flat index space (a 303's params, a
  kit's 8 instruments × drum params + kit params + channel params, etc.).
  The global `PARAM_COUNT` table is retired.
- `info()` (name/default/steps) stays a `match`, keyed by `MachineParam`.
- Which `MachineParam` variants are valid depends on the machine's type
  (e.g. `Fm(_)` only on FM machines); every machine accepts `Channel(_)`.

### Song

- `Bar { cells: Vec<(MachineUid, Cell)> , … }` with
  `Cell { pattern: Option<PatternRef>, mute: bool, transpose: i8 }`.
  Transpose is used only by Synth and FM machines and ignored elsewhere.
- Automation lanes keep `ParamId`, now uid-scoped.
- Removing a machine deletes its cells and lanes (undoable, section 3).

### Migration

- v2 → v3: the six machines get uids 1–6 in today's order and the names
  above; `next_uid = 7`. `ParamId::Channel(n, p)` moves to machine n's
  `channel`; synth/kit/FM/sampler params move into their machine; bar
  arrays become cells (patterns, mutes, transposes via today's
  `transpose_index`); automation `ParamId`s are remapped.
- v1 → v2 → v3 (existing v1 upgrade, then the new step).
- Rendering the demo project before and after migration must produce
  identical audio (sample-exact).

## 2. Engine and audio thread

```rust
struct Engine {
    slots: Vec<Slot>,      // capacity 16 reserved at construction; rack order
    mixer: Mixer,          // shared sends, delay, reverb, master chain
    seq: Sequencer,        // tracks: [Track; 16], by slot position
    // …
}
struct Slot {
    uid: MachineUid,
    dsp: MachineDsp,       // Synth(BassSynth) | Kit(DrumKit) | Fm(FmSynth) | Sampler(Box<Sampler>)
    knobs: Box<[f32]>, base: Box<[f32]>, touched: Box<[bool]>, // sized per type
    patterns: SlotPatterns,// Synth / Drum / Note pattern bank for its type
    strip: ChannelStrip,   // the machine's mixer channel
    buf: StereoBuf,        // MAX_BLOCK render buffer
}
```

- Render loop: each slot renders into its own `buf`; the mixer then walks
  slots (strip + buf), computing solo across all slots, feeding the shared
  sends and master.
- Commands address machines by uid: `SetParam(ParamId)`;
  `Pattern { uid, index, Box<PatternData> }` (replaces `SynthPattern`,
  `DrumPattern`, `NotePattern`); `SelectPattern { uid, … }`;
  `NoteOn/NoteOff { uid, … }`; `SamplerSample/SamplerPlay { uid, slot, … }`.
  The engine resolves uid → slot by linear search (≤16). A command for a
  uid not in the rack is ignored; any boxed payload is returned for
  disposal as today.
- Rack edits: `Command::SetRack(Box<RackSwap>)` where `RackSwap` holds the
  new uid order, the new `Slot`s fully built on the UI thread, an empty
  capacity-16 `Vec<Slot>` to fill, and the recompiled song (bars resolved
  to per-position arrays). The audio thread moves surviving slots across by
  uid — keeping voices, patterns and sequencer track state — swaps the
  song, and returns the old `Vec` (holding removed slots) as
  `Event::DisposeRack`. No allocation or deallocation on the audio thread.
  A machine keeps sounding through edits to other machines.
- `Status` gains `rack_gen: u32` (incremented per `SetRack`); per-machine
  arrays (`playhead`, `patterns`, `queued`, `meters`) become `[_; 16]` by
  position (meters: 16 + master). The UI ignores statuses whose `rack_gen`
  differs from the last rack it sent.
- The NaN guard resets every slot, as today for every machine.

## 3. App and UI (plan b)

- Document edits: `AddMachine { index, entry }`,
  `RemoveMachine { index, entry, cells, lanes }` (keeps what undo needs),
  `MoveMachine { from, to }`, `RenameMachine { uid, old, new }`. Duplicate is
  an `AddMachine` with a deep copy (sound, patterns, sampler audio), a new
  uid, name "<name> copy", inserted directly below the source; song cells
  are not copied. Each rack edit's commands end with one `SetRack`.
- Existing edits (patterns, note patterns, sampler slots, params) are keyed
  by uid instead of per-kind indices.
- Per-machine UI state (`bank`, step writers, scroll accumulators, poly-roll
  drags/snap/len/scrolled, etc.) moves into
  `HashMap<MachineUid, PanelState>`, created on first draw, pruned on rack
  change.
- Panel header: `▾ NAME (Type)   CPU n%   [↑][↓][⧉][✕]`.
  - Double-click name → inline edit; Enter commits; empty → default name.
  - ↑/↓ move one place; disabled at the ends.
  - ⧉ duplicate; ✕ remove at once (no confirmation); status line
    "Removed '<name>' — Ctrl+Z to undo".
- After the last panel: `[+ Add machine ▾]` (303 Synth · 808 Kit · 909 Kit ·
  FM · Sampler) and "n / 16". At 16, Add and ⧉ are disabled with tooltip
  "Rack full (16)". Added machines append with default sound, empty
  patterns, default name.
- Mixer: one strip per machine in rack order, labelled by name; scrolls
  horizontally when wide.
- Song editor: one lane per machine in rack order, labelled by name;
  transpose shown only on Synth and FM lanes.
- Clipboard: paste allowed between machines of the same pattern type —
  303↔303, 808↔909 (same drum pattern format), FM↔Sampler (both note
  patterns, as today); refused otherwise.
- 303 / drum / FM / sampler panels take `(uid, &MachineEntry)`; their
  contents are unchanged.

## 4. Per-machine CPU and shared sample pool (plan c)

- Each slot's work (DSP + its strip) is timed with `Instant` in the render
  loop and summed into `slot_ns: [u64; 16]`; shared work (sends, master,
  sequencer) into `shared_ns`. Each status reports
  `slot_cpu: [f32; 16]` and `shared_cpu: f32` as shares of the audio time
  covered (1.0 = whole budget), then resets. No allocation.
- UI smooths each figure like the main readout and shows `CPU n%` in the
  panel header: amber > 25 %, red > 50 %. Transport CPU hover adds
  "Top: <name> n% · <name> n% · <name> n% · FX/master n%".
- Slot figures are used only from a status whose `rack_gen` matches.
- `MEMORY_LIMIT` (64 MB) becomes `SAMPLE_POOL = 256 MB`, counted across all
  samplers. Each slot's decoded audio counts once per use (a duplicated
  sampler counts again even though the `Arc` is shared, because a saved
  project embeds and reloads copies separately).
  - `validate`: total ≤ pool.
  - Parse: one embedded-PCM budget of pool / 2 shared across all samplers.
  - Sample load: refused if total − slot's current + new > pool:
    "Not enough sample memory: <n> MB needed, <m> MB free of 256 MB".
  - Duplicate of a sampler: refused the same way.
  - Meter: `Smp <total>/256 MB`.

## 5. Testing

Plan (a):
- Migration: v2 demo and a v1 file load as six machines (uids 1–6), with
  params, channel strips, bars, mutes, transposes and automation on the
  right machines; the demo renders sample-identically before/after.
- v3 save/load round trip; `validate` rejections listed in section 1.
- Engine: `SetRack` add/remove/reorder mid-playback keeps surviving voices
  and sequencer positions; removed slots return as `DisposeRack`; commands
  to a removed uid are ignored and disposed; the counting-allocator test
  covers `SetRack`, `Pattern` and `Song`; statuses carry the new `rack_gen`.
- Per-type param index spaces match their `all()`; uid-scoped automation
  drives the right slot.

Plan (b):
- Each document edit's do/undo/redo; remove + undo restores cells, lanes
  and sampler audio; each emits the right `SetRack`.
- 16-cap disables Add/Duplicate; empty rename → default name.
- Clipboard paste rules; stale-`rack_gen` status ignored; panel state
  pruned on removal.
- Manual (Deck): rack of 3×303 + 2 Samplers + 909, song mode, reorder and
  remove while playing, undo, save, reload.

Plan (c):
- Slot shares sum ≤ total CPU; figures follow machines through a reorder;
  a busy sampler reads above an idle 303.
- Pool counts duplicates; over-pool load and duplicate refused with the
  exact messages; shared parse budget; pre-existing projects ≤ 64 MB load
  unchanged.

All limits (rack cap, pool, validation) are checked on the UI thread before
any command is sent; no new failure paths reach the audio thread.
