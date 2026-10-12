# SidAcid: FM Synth + Sampler — Design

Date: 2026-10-12
Status: approved in brainstorming, pending spec review

## Goal

Add two machines to SidAcid, in the same look as the existing rack:

1. **FM synth** modelled on the M-VAVE FM-1 (a DX7-style 6-operator,
   32-algorithm polysynth with filter/chorus/phaser effects).
2. **Sampler** for chopped breaks and chromatically played samples
   (voices, stabs), 16 slots.

Both are sequenced with a new **polyphonic piano roll**. The 303 synths and
drum kits keep their current editors and pattern formats unchanged.

## Decisions

| Question | Decision |
|---|---|
| Polyphony | Poly note-event piano roll for the new machines only; 303s unchanged |
| Instance count | Fixed: 1 FM + 1 sampler (6 machines). Count is a constant so 2+2 is cheap later |
| Sampler scope | 16 slots, each Slice or Chromatic mode |
| Sample storage | Embedded in the `.sidacid` file (16-bit PCM, compressed, base64) |
| Import formats | WAV, FLAC, MP3, OGG via `symphonia` |
| FM scope | 6-op / 32 algorithms, DX7 SysEx import, FM-1 effects (filter, chorus, phaser). No arpeggiator |
| FM character | Clean by default; **Vintage** switch for DX7-ish 12-bit grit |
| Architecture | `MachineId` enum + `MACHINES = 6`; typed engine struct per machine kind |

Out of scope: arpeggiator, time-stretching, sample recording, smart
randomise for the new machines, dynamic rack, shipping Yamaha ROM banks.

## 1. Groundwork

### Machines

```rust
pub enum MachineId { Synth1, Synth2, Kit808, Kit909, Fm, Sampler }
pub const MACHINES: usize = 6;
```

Every per-machine array becomes `[_; MACHINES]` indexed by `MachineId`:
`Project.selected`, `Engine::playhead`, `queued_patterns`,
`current_patterns`, `Bar.patterns`, `Bar.mutes`, mixer strips/`chans`.
`take_meters` returns `[f32; MACHINES + 1]` (6 channels + master).

Pitched machines (Synth1, Synth2, Fm) have a transpose: `Bar.transpose`
becomes `[i8; 3]` (index via a `MachineId::transpose_index()` helper).
The sampler is never transposed (it would scramble slice mapping).

The FM and sampler get full mixer strips: distortion, compressor, level,
pan, mute, solo, delay and reverb sends.

### Params

`ParamId` gains `Fm(FmParam)` and `Sampler(SamplerParam)`:

- `FmParam` covers global params plus `Op(op: u8, OpParam)` for 6 operators.
- `SamplerParam` covers global params plus `Slot(slot: u8, SlotParam)` for
  16 slots.

All new params go through the existing knob/automation/record/undo paths.
`PARAM_COUNT` grows (~150 FM + ~200 sampler); `EVENT_RESERVE` already
scales with it. `ParamId::all()` order is stable and appended to, so v1
param maps still load.

### NotePattern

```rust
pub const TICKS_PER_STEP: u16 = 4;
pub const MAX_NOTES: usize = 256;

pub struct Note { pub start: u16, pub len: u16, pub pitch: u8, pub vel: u8 }

pub struct NotePattern {
    pub notes: Vec<Note>,   // sorted by start; len <= MAX_NOTES
    pub length: u8,         // 1..=64 steps
}
```

- `is_well_formed`: length in range, `notes.len() <= MAX_NOTES`, every note
  has `len >= 1`, `start < length * TICKS_PER_STEP`, pitch <= 127,
  vel 1..=127, sorted by start.
- Notes may extend past the pattern end; they are cut at the loop point.
- The engine holds patterns with `MAX_NOTES` capacity reserved so swaps
  never allocate on the audio thread.
- 128 patterns per machine, same A–H × 16 banks and queue-on-loop switching.

### Sequencer

A new `NoteTrack` kind emits `SeqEvent::NoteOn { machine, pitch, vel }` and
`SeqEvent::NoteOff { machine, pitch }` with sample-accurate offsets at tick
resolution. Swing shifts notes that start on odd 1/16 steps, the same as
the 303 tracks. When a pattern stops, switches, or the machine is muted,
all its held notes are sent NoteOff, so no notes hang.

### Live audition

New commands `Command::NoteOn { machine, pitch, vel }` and
`Command::NoteOff { machine, pitch }` play notes immediately, whether the
transport is running or stopped. They are used for piano-roll key clicks
and sampler pad audition.

### Sample handoff (real-time safety)

Decoding and resampling happen on a worker thread. The engine receives
`Arc<SampleData>` through the existing command queue. A replaced buffer
goes back to the UI thread to be dropped, following the existing pattern
swap pattern. `process()` stays allocation-, lock- and I/O-free.

### File format v2

`PROJECT_VERSION = 2`. `Project` gains:

```rust
pub fm: FmMachine,            // patterns: Vec<NotePattern>, bank: Option<SysexBank>
pub sampler: SamplerMachine,  // patterns: Vec<NotePattern>, slots: Vec<SlotData>
```

- `SlotData` holds the name, mode, zone, slice points, original BPM, loop
  points, source sample rate and channel count, and the audio as 16-bit PCM
  that is deflate-compressed (`flate2`) and then base64-encoded.
- `validate` checks the sizes (16 slots, 128 patterns, well-formed
  patterns, zones not overlapping, slice points sorted and in range).
- **v1 migration:** the arrays are padded to 6 machines, the new machines
  start empty, and the params take their defaults. A v1 file loads without
  any loss.

### Copy/paste

Note patterns paste between the FM and sampler. Pasting a note pattern
onto a 303 or drum pattern is refused, and so is the reverse, which
matches the current rules.

## 2. FM synth

### Engine (`acid-engine/src/fm/`)

- **Operators:** 6 sine operators using phase modulation with a sine lookup
  table. All 32 DX7 algorithms are built in as a static routing table,
  with feedback on each algorithm's feedback operator.
- **Per-operator settings:**
  - frequency mode: ratio or fixed
  - coarse, fine, detune and output level
  - velocity sensitivity
  - keyboard level scaling (break point, curves, depths) and keyboard rate scaling
  - a DX7 4-rate / 4-level envelope (R1–R4, L1–L4)
- **Global settings:**
  - algorithm and feedback
  - pitch envelope (4 rates, 4 levels)
  - LFO: sine, triangle, saw up, saw down, square, S&H; with speed, delay, pitch mod depth, amp mod depth and pitch mod sensitivity
  - per-operator amp mod sensitivity
  - transpose, volume
  - Poly/Mono, Glide time
  - Vintage
- **Voices:** 12-voice poly. Voice stealing takes the oldest note and fades the stolen voice over about 2 ms so it doesn't click. Mono mode is legato, with Glide between overlapping notes.
- **Vintage switch:** a coarser sine table plus 12-bit quantised output.
- **Effects inside the machine,** in this order, before the mixer strip:
  1. **Filter:** LP or HP, cutoff and resonance, using sid-core's `Svf`. Has an on/off switch.
  2. **Chorus:** stereo, rate, depth and mix.
  3. **Phaser:** rate, depth, feedback and mix.

### Presets

- **SysEx import:**
  - accepts 32-voice bank dumps (4104 bytes, `F0 43 00 09 20 00 … F7`) and single-voice dumps (163 bytes)
  - checks the checksum and refuses a malformed file with a message
  - maps DX7 parameter ranges onto `FmParam` values
- **Voice browser:**
  - offers the Starter bank or the Imported bank, 32 names each
  - choosing a voice writes its params as one undoable edit
  - the imported bank is saved in the project
- **Starter bank:** about 16 original voices (E-piano, FM bass, acid FM bass, bells, marimba, brass, strings, pad, organ, pluck, lead, and percussion tones), built into the binary as data. Yamaha ROM banks are not shipped.

### Tests

- The algorithm table matches the known DX7 operator graphs, including the carrier set and the feedback operator.
- Envelope stage timing and levels.
- SysEx parse round-trip, and refusal on a bad checksum or wrong size.
- Never more than 12 voices; stealing picks the oldest.
- Mono glide reaches the target pitch.
- `process()` is allocation-free (the counting-allocator test from Plan 4).

## 3. Sampler

### Slots and zones

- There are 16 slots, and each slot owns a non-overlapping zone of MIDI keys:
  - **Slice slot:** one key per slice, starting from the base key.
  - **Chromatic slot:** a low–high key range plus a root note.
- Zones are auto-assigned upward from C1 when a sample loads, and they can be moved. An edit that would make zones overlap is refused.
- A note that falls in no zone is ignored.

### Slice mode

- **Chop modes:**
  - Equal: 4, 8, 16 or 32 slices.
  - Detect: spectral-flux or energy transient detection, with a Sensitivity control and a cap of 64 slices.
- Markers can be dragged in the waveform view.
- A slice plays from its marker to the next marker. A note-off before that triggers the slot's release.
- **Choke** is on by default for slice slots: a new note in the slot cuts the voice it is already playing.
- **Tempo lock:**
  - The original BPM is guessed by assuming the sample is a whole number of 4/4 bars and picking the bar count that lands closest to 90–180 BPM. It can be edited.
  - When the lock is on, the playback rate is `song_bpm / original_bpm`. This repitches the sample; it does not time-stretch.

### Chromatic mode

- The root note sets pitch: the playback rate is `2^((pitch - root + tune) / 12)`.
- Full ADSR, start and end points.
- Optional loop between loop points, with a 10 ms crossfade.

### Per-slot params (`SlotParam`, all automatable)

Level, Pan, Tune (semitones), Fine (cents), Cutoff and Resonance (LP using `Svf`), Attack, Decay, Sustain, Release, Reverse, Choke, and Tempo Lock (slice mode).

Global: Volume and **Grit**, an SP-1200-style mode: about 26 kHz sample-and-hold plus 12-bit quantisation.

### Engine

- 16-voice poly, cubic Hermite interpolation, stereo samples supported.
- **Loading:**
  - `symphonia` decodes the file.
  - The audio is resampled to the engine rate on the worker thread.
  - The source-rate PCM is kept for saving.
- **Limits:** 30 s per slot and about 64 MB of decoded audio across the project. Anything over is refused with a message.

### Tests

- Equal slicing positions.
- Transient detection on synthetic click trains.
- Zone mapping and the overlap refusal.
- Tempo-lock rate math.
- Choke cuts the previous voice.
- Reverse playback.
- The PCM embed → save → load round-trip is bit-exact at 16-bit.
- Allocation-free `process()`.

## 4. UI

### Rack

Synth 1, Synth 2, 808, 909, **FM**, **Sampler**, Mixer (6 strips + master), Song (6 lanes; transpose cells for Synth 1, Synth 2, FM). Same theme, knob widget, pattern pads, length control and panel frame as the existing machines.

### FM panel

- **Top row:**
  - algorithm selector with an **algorithm diagram** of operator boxes and modulation lines; carriers on the bottom row, and the feedback loop drawn
  - clicking an operator box selects it
  - feedback, Poly/Mono, Glide
  - LFO, pitch envelope, Filter, Chorus and Phaser sections
  - Vintage, Volume
- **Operator row:**
  - tabs OP1–OP6, each with a mute toggle (mute sets the operator's output level to 0 non-destructively, with a UI-only stored level)
  - knobs for the selected operator
  - a small editable envelope graph next to the R1–R4 / L1–L4 knobs (labelled Attack, Decay 1, Decay 2, Release)
- **Voice browser:** a Starter / Imported toggle, the voice list, and an **Import .syx** button.
- Target height: about the same as a 303 panel, so the rack stays usable at 1280×800.

### Sampler panel

- **16 slot pads:** each shows the slot's name and mode. Click selects the slot; shift-click auditions it.
- **Waveform view:**
  - draggable slice markers, start and end markers, and loop markers
  - playback cursors
  - zone readout
- A **Load…** button, plus drag-and-drop of audio files onto the panel (the dropped file loads into the selected slot).
- **Slice controls:** Equal 4/8/16/32 or Detect with Sensitivity, and original BPM.
- The per-slot knobs and the Grit switch.

### Poly piano roll

The code is split into `ui/polyroll.rs` (drawing and input) and `ui/poly_edit.rs` (pure, unit-tested edit logic), mirroring `pianoroll.rs` and `roll_edit.rs`.

- Click an empty cell to add a note with the last-used length. Drag a note's body to move it and its right edge to resize it. Right-click deletes.
- Chords: any number of notes per column, up to `MAX_NOTES` per pattern. Adding beyond the cap is refused.
- **Snap:** 1/16 (default), 1/32, 1/64 (one tick), or Off. Off still lands on whole ticks.
- **Velocity lane** below the grid: drag to set each note's velocity.
- Vertical scroll over the full key range; it opens centred on C3 for the FM synth and on the first used zone for the sampler.
- Clicking the keyboard strip sends an audition NoteOn/NoteOff.
- **Sampler mode:** row labels show slot and slice ("Amen ·05", "Vox C3"), and each zone is tinted.
- Each drag gesture is one undo step. Whole-pattern copy and paste use the existing clipboard.

### Tests

- `poly_edit`: add, move, resize, delete, snap modes, chord stacking, the cap refusal, and sort order kept.
- Headless draw tests for the FM panel, sampler panel and poly roll.
- Undo coverage for a voice load, a sample load and a slice edit.

## 5. Build order

Each item is a separate implementation plan, merged to master before the next one starts.

1. **Plan 4 — Groundwork:**
   - `MachineId`/`MACHINES`
   - 6-strip mixer and meters
   - song and `Bar` widening
   - v2 format with v1 migration
   - `NotePattern` and `NoteTrack`
   - audition commands
   - the poly roll
   - placeholder sine-voice machines in the FM and Sampler slots, so the whole path is testable and audible
   - a counting global-allocator integration test that asserts `Engine::process()` makes zero allocations while every machine is playing (no such test exists today)
2. **Plan 5 — FM synth:**
   - engine, algorithms, envelopes, LFO
   - effects
   - SysEx import and the starter bank
   - the FM panel
3. **Plan 6 — Sampler:**
   - decode, resample and embed
   - slots, zones and slicing (equal and detect)
   - tempo lock, choke, chromatic mode, Grit
   - the sampler panel and drag-and-drop
