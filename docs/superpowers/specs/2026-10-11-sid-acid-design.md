# SidAcid — Standalone SID Acid Groovebox: Design Spec

Date: 2026-10-11
Status: Approved design, pending spec review
Working name: **SidAcid** (rename freely)

## 1. Goal

A standalone native desktop acid groovebox built on a C64 SID-style engine. It contains:

- 2 × SID bass synths with 303-style controls (cutoff, resonance, env mod, decay, accent, slide)
- 2 × SID drum machines (an 808-flavoured kit and a 909-flavoured kit)
- Per-channel distortion and compressor, delay and reverb sends, master compressor
- Two swappable sequencer views (classic 303 step strip / piano roll) over the same pattern data
- 128 patterns per machine in 8 banks × 16
- Song mode with arrangement and touched-knob automation recording
- Unison mode: 3 detuned voices per synth
- Smart randomise with undo, swing, WAV export

It is **not** part of Acid OS. It shares no runtime, files or dependencies with it.

Inspirations: midiphy zetaSID (SID drum
programs, per-step parameter lane, detuned unison), raver1975/acid (groove-aware randomise,
drum evolver, swing, bars-with-automation song model).

## 2. Platform and architecture

- Rust (edition 2024) Cargo workspace, native desktop (Linux/SteamOS first; Windows/macOS should work).
- GUI: `eframe`/`egui`, with custom-painted widgets.
- Audio: `cpal`, f32 stereo at the device rate (44.1 or 48 kHz).
- Threading model (approach A):
  - **UI thread** owns the authoritative `Project`, the undo stack, and all editing.
  - **Audio thread** owns the `Engine` (sequencer, 4 machines, FX, mixer) and its own copy of the
    project data it needs to play.
  - UI → audio: `rtrb` lock-free ring buffer of `Command`s (param set, pattern replace, transport,
    song replace, …).
  - Audio → UI: second ring buffer of `Event`s (playhead step/bar, meters, recorded automation points,
    returned old buffers for deallocation).
  - Audio thread never allocates, frees or locks. Large data (patterns, songs) is sent as
    preallocated `Box`es; replaced boxes are sent back to the UI thread to be dropped.

### Workspace layout

```
sidsynth/
  LICENSE                     (project licence)
  THIRD_PARTY_NOTICES.md      (MIT notice for code derived from Acid OS acid-synth)
  crates/
    sid-core/       SID chip emulation (forked from Acid OS acid-synth, MIT)
    acid-engine/    DSP + data + sequencing; no GUI or audio-device deps; fully offline-testable
      src/synth.rs       bass synth (303 control layer over sid-core)
      src/drums/         mod.rs (voice pool, program runner), kit808.rs, kit909.rs
      src/fx/            distortion.rs, compressor.rs, delay.rs, reverb.rs, lofi.rs
      src/mixer.rs
      src/sequencer.rs   clock, swing, pattern playback, song playback, automation
      src/params.rs      ParamId registry: ranges, defaults, names, smoothing
      src/project.rs     Project / Pattern / Song types + serde
      src/random.rs      smart randomise (seeded)
      src/render.rs      offline render → WAV (hound)
      src/engine.rs      top-level Engine: process(&mut [f32] stereo), handle(Command)
    app/            eframe UI, cpal output, command/event queues, undo stack, file I/O
      src/bin: `sidacid` (GUI), `sidacid render-demo <out.wav>` (CLI smoke test)
```

## 3. sid-core (forked SID engine)

Source: copy of Acid OS `v3/crates/acid-synth` (MIT, © 2026 NorfolkHacker). Notice kept in
`THIRD_PARTY_NOTICES.md` and file headers. After the fork it evolves independently.

Changes from the original:

- `SidChip` with **3 voices** (a chip). Machines own as many chips as they need.
- **Output**: f32 per voice and per chip, rendered at any sample rate (phase increments computed
  from Hz, not from the 22 050 Hz table). Remove the u8 output path.
- **Pitch**: `set_freq_hz(v, f32)` with fine pitch for slides/detune.
- **Hard sync**: classic SID mode (voice n resets its phase when voice n-1 (mod 3) wraps), plus a
  per-voice *hidden sync master* mode: each voice may own an extra phase accumulator at a set
  frequency that resets it. The hidden master is silent. This lets every voice be synced at once
  (needed for unison + sync).
- **Ring mod**: kept (triangle only).
- **Envelopes**: ADSR with exponential decay/release (SID-style piecewise exponential), attack
  linear. Gate on/off as before.
- **Filter**: one multimode state-variable filter per chip (LP/BP/HP bitmask, as original), with:
  - cutoff as an f32 in Hz (20 Hz–18 kHz) with exponential mapping from a 0–1 control,
  - resonance 0–1 mapped to Q up to near self-oscillation, with soft saturation inside the loop
    to keep it stable and squelchy,
  - oversampling ×2 inside the filter to keep high-cutoff/high-resonance stable at 44.1/48 kHz,
  - per-voice filter routing as original.
- Waveforms: pulse (with PW), saw, triangle, noise (LFSR). Combined waveforms: out of scope.
- `no_std` is not required any more; keep it dependency-free.

## 4. Bass synths (×2)

Each synth = one `SidChip` + a 303 control layer.

Controls (all automatable `ParamId`s):

| Control      | Range / values                    | Notes |
|--------------|-----------------------------------|-------|
| Tune         | ±12 semitones (fine, continuous)  | |
| Cutoff       | 0–1 → 20 Hz–18 kHz (exp)          | filter base |
| Resonance    | 0–1                               | |
| Env Mod      | 0–1                               | depth of filter envelope sweep |
| Decay        | 0–1 → 30 ms–2.5 s                 | filter envelope decay (normal notes) |
| Accent       | 0–1                               | accent amount (volume + filter) |
| Wave         | Saw / Pulse                       | |
| Pulse Width  | 0–1                               | pulse only |
| Filter Mode  | LP / BP / HP                      | |
| Sync         | off / on                          | hard sync via hidden master; Sync Pitch knob 0–24 st |
| Unison       | off / on                          | |
| Detune       | 0–50 cents                        | unison only |
| Spread       | 0–1                               | unison stereo width |
| Volume       | 0–1                               | channel pre-mixer |

Behaviour:

- **Filter envelope**: exponential decay from peak each note-on; cutoff = base + envmod × env.
- **Accent**: an accented step raises amplitude (≈ +6 dB scaled by Accent), forces short filter
  decay (≈ 200 ms), and adds the accent sweep: a smoothing capacitor model that charges on each
  accented note, so consecutive accents build higher ("accent wow").
- **Slide**: if step n has slide set and step n+1 has a gate, the gate stays open into n+1 and pitch
  glides exponentially over ≈ 60 ms; envelopes do not retrigger. Slide to the same pitch = tie.
- **Gate length**: non-slide notes release at 50 % of the step (adjusted for swing).
- **Unison off**: voice 1 plays alone, centred.
- **Unison on**: all 3 voices play the note, detuned −d/0/+d cents, panned by Spread (−s/0/+s).
- **Sync**: each sounding voice uses its hidden sync master at the played note's frequency; the
  voice itself runs Sync Pitch semitones higher, so sync works the same with or without unison.
- Amp envelope: fast attack, gate-held sustain, short release; accent scales level.

## 5. Drum machines (×2)

Each drum machine = a pool of 6 SID voices (2 chips) running **drum programs**: small per-sample
state machines that switch waveforms and sweep pitch, pulse width and amplitude over time. Drum
voices bypass the shared chip filter; each voice gets its own state-variable filter in the drum
layer (`drums/mod.rs`), so a hat and a kick can be filtered differently at the same time. Instruments steal the oldest voice when the pool is full; an instrument retriggering
reuses its own voice. Open hat is choked by closed hat.

8 instrument slots per kit: BD, SD, LT, HT, CP, RS, CH, OH.
Per-instrument knobs (automatable): Tune, Decay, Tone, Level.
Kit-level: Volume, Accent amount.

- **808 kit (`kit808.rs`)**: BD = triangle with long exponential pitch drop and long decay
  ("boom"); SD = two detuned triangles + high-passed noise; toms = triangle with pitch drop;
  CP = noise with 3–4 rapid envelope bursts then a tail; RS = short pulse click; hats = noise
  through HP filter, CH short / OH long.
- **909 kit (`kit909.rs`)**: BD = pulse attack click switching to triangle with fast pitch sweep,
  punchier; SD = triangle tone + bright noise with independent "snappy" decay; toms = pulse →
  triangle sweep; CP = tighter bursts; RS = ring-mod blip; hats = ring-modulated pulses + noise
  for metallic tone.

Tone controls each program's main timbre parameter (e.g. BD click/drop depth, SD noise/tone
balance, hat filter cutoff). Exact algorithms are tuned by ear during implementation; the program
interface is fixed.

## 6. Mixer and FX

All processing in f32 at device rate.

```
per channel (synth1, synth2, 808, 909):
  source → Distortion (insert) → Compressor (insert) → Level / Pan / Mute / Solo
         ├─ Delay send  ─→ Delay  ─┐
         └─ Reverb send ─→ Reverb ─┤
  dry sum ─────────────────────────┴→ Master Compressor → Master Level → Lo-fi (optional) → out
```

- **Distortion** (per channel): Drive, Tone, Mix, On/Off. Waveshaper (tanh-style + asymmetric),
  ×2 oversampled, post tone low-pass.
- **Compressor** (per channel and master): Threshold, Ratio, Attack, Release, Makeup, On/Off.
  Feed-forward, RMS/peak detector, soft knee.
- **Delay** (global send): time synced to tempo (1/16, 1/8, dotted 1/8, 1/4, 3/16 …) or free ms,
  Feedback (clamped < 1), Tone (LP in feedback), Ping-pong on/off, Return level.
- **Reverb** (global send): Freeverb-style (comb + allpass), Size, Damping, Width, Return level.
- **Lo-fi** (master, global switch): resample-and-hold to 22 050 Hz and quantise to 8 bits, to
  recall the original Acid OS character.
- Mixer strips: Level, Pan, Mute, Solo, Dist on/off, Comp on/off, Delay send, Reverb send.
- Output meters per channel and master (peak, sent as `Event`s ~30 Hz).

## 7. Data model

```rust
struct SynthStep { note: u8 /*0..=11*/, octave: i8 /*-1..=1*/, gate: Gate /*Off|On|Tie*/, accent: bool, slide: bool }
struct SynthPattern {
    steps: [SynthStep; 64], length: u8 /*1..=64*/, transpose: i8,
    param_lane: Option<ParamLane>,     // one optional per-step automation lane (zetaSID style)
}
struct ParamLane { param: ParamId, values: [Option<f32>; 64] }
enum DrumHit { Off, On, Accent }
struct DrumPattern { steps: [[DrumHit; 64]; 8], length: u8 }
struct SynthMachine { patterns: [SynthPattern; 128], knobs: SynthKnobs }
struct DrumMachine  { patterns: [DrumPattern; 128],  knobs: DrumKnobs }
struct PatternRef(u8)                  // 0..128, displayed as bank letter + number: "C07"
struct Bar { patterns: [Option<PatternRef>; 4], mutes: [bool; 4], transpose: [i8; 2] }
struct AutoLane { param: ParamId, points: BTreeMap<u32 /*global step*/, f32> }
struct Song { bars: Vec<Bar> /*≤ 999*/, loop_range: Option<(u16, u16)>, automation: Vec<AutoLane> }
struct Project {
    version: u32, name: String, bpm: f32, swing: f32,
    synths: [SynthMachine; 2], drums: [DrumMachine; 2],
    mixer: MixerState, fx: FxState, song: Song,
}
```

- Banks A–H × 16 patterns per machine = 128.
- `Gate::Tie` = continue the previous note without retrigger (piano roll long notes). `slide`
  differs: it glides pitch into the next step.
- Knob values are normalised `f32` 0–1 in storage; `params.rs` maps them to real units.

### Project files

- Extension `.sidacid`, RON format, contains everything in `Project`.
- `version` field; loader rejects newer major versions with a clear error. Old versions migrate.
- Save is atomic (write temp file in same dir, then rename).

## 8. Sequencer

- Resolution: 16th-note steps; BPM 40–300; global swing 0–75 %: even steps lengthened, odd steps
  shortened by the same amount (pair duration constant).
- Sample-accurate: the engine computes the sample offset of each step boundary within each audio
  block and splits processing there.
- Each machine plays its own pattern length (polymeter allowed).
- In song mode, a bar's length = the longest pattern length among that bar's assigned patterns
  (16 if none). Every machine restarts its pattern at each bar boundary; shorter patterns loop
  inside the bar. Automation uses global step indices counted from song start.
- **Pattern mode**: each machine loops its selected pattern. Selecting a new pattern while
  playing queues it; it starts when that machine's current pattern wraps.
- **Song mode**: plays bars in order (or loops `loop_range`). Bar sets each machine's pattern
  (None = silent), mutes and synth transpose. Stops at the end unless looping.
- Transport: Play, Stop (stop twice = return to start), Record (song mode only), Pattern/Song switch.

## 9. Sequencer views (swappable per synth)

Both views edit the same `SynthPattern`; switching is instant and lossless. The selected view is
a UI preference stored per synth in the project.

- **303 view**: one column per step, rows: Note (C…B keys), Down, Up, Accent, Slide, Gate
  (Off/On/Tie). Step-write mode: pick a step, type notes on the QWERTY keyboard (Z/X octave),
  it advances automatically; Space toggles rest; A/S toggles accent/slide.
- **Piano roll**: 3-octave grid (C-1 to B+1 relative to the base octave) × pattern length.
  Click to place a note, drag horizontally to lengthen (writes On followed by Ties), drag the
  note vertically to change pitch, right-click to delete. Accent and slide shown as a lane below
  the grid, toggled by clicking. Mono: placing a note on a step replaces any existing note there.
- Optional param lane is drawn under either view as a bar graph; choose the param from a menu.

Drum machines have one view: 8 rows × length grid; click cycles Off → On → Accent → Off.

## 10. Song mode and automation

- Song editor: horizontal bar timeline; each bar shows the 4 pattern refs and mutes. Select a
  bar → edit refs/mutes/transpose; insert, delete, duplicate, copy/paste a bar range; set loop
  start/end markers.
- **Record**: in song mode with Record on:
  - Changing a machine's pattern writes that pattern ref into the current bar and following
    bars until the next change.
  - Any knob the user touches while recording writes automation points at the current step for
    as long as it's held. Knobs not touched keep playing recorded automation (touched-only
    overwrite). A touched knob overwrites from touch-start until release.
  - Recorded points travel audio → UI as `Event`s; the UI merges them into the project (one undo
    step per record pass).
- Playback: automation values apply at step boundaries, smoothed (~5 ms) to avoid zipper noise.
  A knob with automation shows a coloured ring. "Clear automation" per param, and for the whole song.
- In pattern mode, automation is ignored; knob positions are live.

## 11. Randomise and undo

- **Synth dice** (separate buttons): Notes, Accent, Slide, Rest, Octave; plus "All".
  - Notes: scale chosen by weight (minor/melodic minor/Phrygian/major/Hungarian minor); Markov
    intervals preferring root, repeats, −1 and +2 scale degrees; leaps > 7 semitones folded
    back by an octave.
  - Groove-aware: note probability raised on steps where the paired drum machine's current
    pattern has BD or SD (synth1 pairs with 808, synth2 with 909).
  - Accent never directly after a slide; slides tend to chain.
- **Drum**: "Template" (four-on-floor / broken, backbeat, hat styles with matching open hats) and
  "Evolve" (weighted add/move/accent mutations using per-step weight tables).
- All randomise uses a seeded RNG (`rand_pcg`) so tests are deterministic.
- **Undo**: every edit is an `EditCommand` applied to the UI project; undo stores the inverse
  (prior state of the touched pattern/bar/param). 200 levels. A knob drag = one entry. Ctrl+Z /
  Ctrl+Y (also Ctrl+Shift+Z). Pattern copy/paste between slots and banks, and to the other
  machine of the same type.

## 12. WAV export

- File → Export: Song, or current patterns × N bars (1–64).
- Offline render with the same `Engine` at 44 100 Hz, 16-bit stereo (`hound`), faster than real time, on a
  background thread with a progress bar; FX tails rendered (until silence or 10 s max).

## 13. UI

eframe/egui; target 1280×800 (Steam Deck); scrollable vertical rack:

```
Transport: ▶ ■ ● REC | BPM | Swing | Pattern/Song | Lo-fi | Master | CPU % | File menu
SYNTH 1: knobs | bank A–H + 16 pattern pads | [303 | Piano roll] | dice | Length | meters
         step editor (303 strip or piano roll)
SYNTH 2: same
808:     8 × (Tune Dec Tone Lvl) | pattern pads | 8-row grid | Template / Evolve
909:     same
MIXER:   4 strips | Delay | Reverb | Master compressor
SONG:    bar timeline, loop markers, automation lane view
```

- Panels collapse to one-line headers.
- Knobs: vertical drag, Shift = fine, double-click = default, scroll wheel; value tooltip;
  coloured ring when automated.
- Keys: Space play/stop; Ctrl+S save; Ctrl+O open; Ctrl+Z/Y undo/redo; Ctrl+C/V pattern copy/paste;
  QWERTY note entry in step-write mode.
- Touchscreen and trackpad work via egui pointer events; controller mapping out of scope.
- Playhead shown in step editors and the song timeline from audio `Event`s.

## 14. Error handling

- No audio device: banner, UI stays usable, "Retry audio" in menu. Stream error: reconnect to
  default device.
- Command ring full: knob updates coalesce (latest value per param re-sent next frame); structural
  commands retry next frame. Queue sizing (4096) makes this rare.
- Bad/newer project file: error dialog, current project untouched.
- DSP safety: feedback clamped, denormal flushing, NaN guard on master (resets FX state and logs).

## 15. Testing

- **sid-core**: unit tests: pitch accuracy (zero-crossing count), hard-sync phase reset,
  exponential envelope shape, filter stability at max resonance (bounded output, no NaN);
  golden render hashes.
- **acid-engine**: offline render tests:
  - step triggers on exact sample offsets, including swing;
  - slide keeps gate open and does not retrigger the envelope; tie likewise; non-slide releases;
  - accent raises level and the accent sweep builds over consecutive accents;
  - unison produces 3 detuned voices;
  - drum programs produce bounded, decaying output; OH choked by CH;
  - pattern queueing switches at the wrap; song bars switch patterns, mutes and transpose;
  - automation: recorded points override only touched params;
  - each FX: impulse response bounded, no NaN, feedback stable;
  - project RON round-trip; version rejection;
  - randomise deterministic per seed and respects rules (no accent after slide, leaps folded);
  - offline render length/format.
- **app**: undo/redo command tests (apply → undo → state equal).
- **Smoke**: `sidacid render-demo out.wav` renders a built-in demo project for listening.
- UI verified manually.

## 16. Out of scope (v1)

MIDI in/out and clock sync; combined SID waveforms; polyphonic chords; multiple songs per project;
controller/gamepad mapping; plugin (VST/CLAP) build; sample import.
