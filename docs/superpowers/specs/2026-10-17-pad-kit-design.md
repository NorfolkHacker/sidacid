# SidAcid: Pad Kit — Design

Date: 2026-10-17
Status: approved in brainstorming, pending spec review
Depends on: `2026-10-17-open-rack-memory-meter-design.md` (built first)

## Goal

Add a **Pad Kit** machine: a sample-based drum machine with 16 pads.
Each pad plays samples the user loads. It is sequenced like the existing
kits, with one dock row per pad and hits, accents, chance, ratchet and
conditions. Each pad has up to four velocity layers, its own tone and
envelope controls, and an optional mute group. A whole kit of 16 pads
saves to and loads from a single file.

User-facing text (UI, manual) never names a hardware product or
manufacturer. The machine's menu label is "Pad Kit" and its name stem is
"Pads" ("Pads", "Pads 2", …).

## Decisions

| Question | Decision |
|---|---|
| Pads | 16, in a 4×4 grid; a note's pitch 0–15 is the pad |
| Layers | 1–4 samples per pad, chosen by velocity range; ranges split evenly by default and can be edited |
| Accent | Raises the note's velocity by the kit's Accent amount before the layer is chosen |
| Per-pad controls | Tune, Decay, Level, Pan, Cutoff, Resonance, Start, End, Reverse, Mode (One-shot / Gate), Mute group (Off, 1–4) |
| Kit controls | Volume, Accent |
| Voices | 16 shared; a pad retriggers itself; when every voice is busy, the oldest is reused |
| Choke | A hit cuts off the other pads in its mute group (fast fade) |
| Samples | Per pad, through **Load…** or dropped files; kits save and load as one `.padkit` file |
| Architecture | `MachineType::PadKit` with its own `acid-engine/src/padkit/` module, reusing the sampler's decode, PCM and `SampleData` |

Out of scope: Template and Evolve, slicing, looping, layer round-robin,
per-pad effects sends, and a pad performance view with live recording
from the pads.

## 1. Engine (`acid-engine/src/padkit/`)

### Data (project side)

```rust
pub struct PadKitMachine {            // in project.rs MachineBody::PadKit
    pub pads: [Pad; PADS],            // PADS = 16
    pub patterns: Vec<DrumPattern>,   // as DrumMachine
}
pub struct Pad {
    pub name: String,                 // defaults to the first layer's file stem
    pub layers: Vec<Layer>,           // 0..=4
}
pub struct Layer {
    pub vel_lo: u8, pub vel_hi: u8,   // inclusive, 1..=127
    pub data: PadSample,              // name, sample rate, channels, PCM
}
```

- `PadSample` reuses the sampler's PCM storage: 16-bit, deflated and
  base64 in the file, with the same `MAX_SECONDS` and `MAX_BYTES` guards.
  It reuses the sampler's `encode`/`decode` and `to_sample()` helpers.
  Those move from `sampler/slot.rs` into `sampler/sample.rs` (or a shared
  `pcm.rs`) so both machines can use them.
- `Pad::is_well_formed`: at most 4 layers, and `vel_lo <= vel_hi`. Ranges
  may overlap; the first layer that matches wins. A velocity no range
  covers plays nothing.
- **Auto-split:** whenever a layer is added or removed, the ranges reset
  to an even split. With 2 layers they are 1–63 and 64–127; with 4 they
  are 1–31, 32–63, 64–95 and 96–127. An edited range stays as edited
  until the next add or remove.

### Params (`params.rs`)

`PadKitParam` (kit) is Volume and Accent. `PadParam` (per pad, `PADS` of
each):

| Param | Range | Default |
|---|---|---|
| Tune | −24 … +24 semitones (continuous) | 0 |
| Decay | 0–100 %. At 100 the amp stays open to the sample's end; lower values add an exponential decay from 10 ms up | 100 % |
| Level | 0–100 %, on the kits' level curve | 80 % |
| Pan | L100 … R100 | centre |
| Cutoff | 20 Hz … 20 kHz, exponential | 20 kHz (open) |
| Resonance | 0–100 % | 0 |
| Start | 0–100 % of the sample | 0 |
| End | 0–100 % of the sample, kept above Start | 100 % |
| Reverse | switch | off |
| Mode | One-shot / Gate | One-shot |
| Mute group | Off, 1–4 | Off |

All of them are automatable like the other machines' params.
`count_for(PadKit)` is `PadKitParam::ALL.len() + PADS *
PadParam::ALL.len()`.

### Playback (`padkit/machine.rs`, `padkit/voice.rs`)

- `note_on(pad, vel, accent)`:
  1. Add `Accent × 127` to `vel` when the note is accented, clamped to
     127.
  2. Pick the layer by velocity.
  3. If the pad has a mute group, fade out every voice of the other pads
     in that group over 5 ms.
  4. Fade out the pad's own ringing voice the same way.
  5. Take a free voice, or else the oldest one.
- Velocity also scales the level linearly, `vel / 127`, as the sampler
  does.
- The voice reads `SampleData` with the sampler's interpolating reader.
  Its rate comes from Tune and the sample's rate over the engine's rate.
  The read position runs from Start to End, or from End down to Start
  when Reverse is on.
- Signal path:

  ```
  sample read → SVF low-pass (sid-core svf) → amp decay → level → pan → kit Volume → out
  ```

- **One-shot** voices ignore note-off. **Gate** voices fade over 5 ms at
  note-off. Either way a voice stops at the end of its play range.
- New samples reach the audio thread the way the sampler's do: an
  `Arc<SampleData>` swapped in per (pad, layer) with
  `Engine::swap_padkit_sample`. Layer ranges arrive the same way, as a
  fixed-size `PadPlay` (`[(u8, u8); 4]` plus a layer count) built on the
  UI thread. The audio thread never allocates.

### Machine integration

- `MachineType::PadKit`. It is added to `ALL`, `label` ("Pad Kit") and
  `stem` ("Pads"), and does not transpose.
- `Slot::PadKit` sits next to the kits in `engine.rs`. The sequencer
  treats it as a drum machine with 16 rows: the `DRUM_INSTRUMENTS` checks
  (`sequencer.rs:1363`, `lane.rs:748`) become `drum_rows(ty)`, which is
  8 for the kits and 16 for the Pad Kit.
- `memory_estimate` (from the open-rack spec) counts the decoded layers.
- The acid synths' Dice pairing with kits ignores Pad Kits, because their
  rows are not BD/SD/hats.

## 2. Kit files (`.padkit`)

A RON file holding `PadKitFile { version: 1, pads: [Pad; 16], params:
PadKitParams }`. The audio is embedded the same way as in projects.

- **Save kit…** writes every pad's layers and the kit's and pads' param
  values.
- **Load kit…** replaces every pad's layers and the param values in one
  undo step. Patterns are untouched.
- The load is checked against the memory warning (open-rack spec, section
  3) before it is applied.
- A file that fails to parse, or holds a malformed pad, is refused whole,
  with the reason ("pad 3: layer 2 has no audio").

## 3. UI (`app/src/ui/padkit_panel.rs`)

From top to bottom:

1. **Kit knobs:** Volume and Accent, then **Save kit…** and
   **Load kit…**.
2. **The pad grid:** 4×4, with pad 1 at the bottom left as on hardware
   pads (pad 13 at the top left).
   - Each pad shows its number, the first eight characters of its name and
     one dot per layer. An empty pad shows "·".
   - Click selects a pad. Shift-click-and-hold auditions it at velocity
     100.
   - Dropping one file loads it as a new layer of that pad. Dropping
     several files onto the grid loads one per pad, starting at that pad.
   - A pad flashes when it plays.
3. **The selected pad's row:** its number and editable name, **Load…**
   (adds a layer, or replaces the selected layer when the pad already has
   4), **Clear** (the selected layer) and **Clear pad**.
   - Then the layer tabs 1–4, each with its `lo–hi` velocity range as two
     drag fields.
4. **The waveform** of the selected layer, with Start and End markers you
   can drag. It is mirrored when Reverse is on.
5. **The selected pad's knobs:** Tune, Decay, Level, Pan, Cutoff, Res,
   Start, End, then the Reverse switch, the Mode toggle and the Mute group
   selector.
6. **The pattern pads and Len**, as on the kits.

Loading reuses `sampler_load.rs`'s decode path and its errors. A file
dialog only opens on a click, never on hover. Every load, clear, rename,
range edit and kit load is one undo step, with the sample data counted in
the undo byte budget (`document.rs`).

**Dock:** the drum lane draws `drum_rows(ty)` rows. The Pad Kit labels
them with the pads' names (or "P1"–"P16" when a pad is empty), and pad 1
is at the bottom. Hit editing, the accent strip, velocity, chance, ratchet
and conditions are unchanged.

## 4. Tests

- **Engine:**
  - a layer is chosen by velocity, including accent pushing a hit into a
    higher layer and a velocity no range covers;
  - a mute group chokes the other pads in it but not pads outside it;
  - a pad retriggers itself;
  - Start, End and Reverse read the expected frames;
  - Gate stops at note-off and One-shot does not;
  - voice stealing takes the oldest voice;
  - Cutoff at 20 kHz is transparent (within tolerance);
  - `no_alloc` covers a Pad Kit with loaded layers playing a pattern.
- **Project:**
  - a Pad Kit with layers round-trips through save and load;
  - `validate` rejects 5 layers and an inverted range;
  - the auto-split ranges match the table in section 1.
- **App:**
  - a load, clear and rename can each be undone;
  - dropping 3 files fills 3 consecutive pads;
  - a `.padkit` file round-trips, and a broken one is refused whole;
  - the dock shows 16 rows labelled with the pad names.
- **Running app:** load a few samples, sequence a pattern and check it is
  audible; take a screenshot for the manual (`docs/images/padkit.png`).

## 5. Docs

- New manual chapter `docs/manual/pad-kit.md`: the panel, pads and
  layers, knobs, mute groups, kit files and dock rows.
- Link it from `index.md`, `rack-and-patterns.md` (the machine list) and
  `drum-kits.md` (see also).
- `keyboard-reference.md`: Shift-click to audition.

## 6. Build order

1. Move the PCM helpers out of `sampler/slot.rs` so they can be shared.
   Add `MachineType::PadKit`, the project data, params and validation.
2. The engine voice and machine: layers, tune, trim and reverse, filter,
   decay, Gate and One-shot, mute groups. Then `Slot` integration,
   `drum_rows` and `no_alloc`.
3. The panel: grid, selected-pad row, knobs, waveform; loading and drag
   and drop; undo.
4. Dock rows and labels.
5. `.padkit` save and load.
6. Docs and screenshots.
