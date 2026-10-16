# SidAcid

An acid groovebox built on a C64 SID-style sound engine. It runs as a native desktop app and is laid out for the Steam Deck screen.

![SidAcid](docs/images/screenshot.png)

<p>
  <img src="docs/images/fm.png" alt="The FM synth: algorithm diagram, operator editor, voice browser" width="49%">
  <img src="docs/images/sampler.png" alt="The sampler: slot pads and a sliced break with markers" width="49%">
</p>

## What's in the box

- **Two SID bass synths with 303-style controls:**
  - Tune, Cutoff, Resonance, Env Mod, Decay and Accent.
  - Saw or pulse wave, LP/BP/HP filter, hard sync.
  - Accent sweep that builds over consecutive accents, and slide/tie.
  - 3-voice detuned **unison** with stereo spread.
- **Two SID drum machines:**
  - An 808-flavoured and a 909-flavoured kit: BD, SD, LT, HT, CP, RS, CH and OH.
  - Tune, Decay, Tone and Level per instrument.
  - Closed hat chokes open hat.
- **FM synth** in the style of the DX7 / M-VAVE FM-1:
  - 6 operators, 32 algorithms with a live algorithm diagram, DX7 envelopes, keyboard and velocity scaling.
  - LFO and pitch EG.
  - 12-voice poly, or mono with glide.
  - Filter, chorus and phaser, plus a Vintage switch for 12-bit grit.
  - A 16-voice starter bank, and **DX7 .syx import** (32-voice banks or single voices).
- **Sampler** with 16 slots:
  - **Slice mode** for breaks: chop into 4/8/16/32 equal slices or by detected transients, drag the slice markers, and use choke and tempo lock.
  - **Chromatic mode** for voices and stabs: root key, start/end, and a crossfaded loop.
  - Per slot: level, pan, tune, filter, ADSR and reverse. Grit adds SP-1200-style 12-bit, 26 kHz crunch.
  - Load WAV, FLAC, MP3 or OGG with Load… or by dragging the file onto the panel. Audio is saved inside the project file. Sample rates from 8 to 192 kHz are accepted; there is no resampling filter, so samples above 48 kHz, or pitched up an octave or more, may alias.
- **Editor dock:**
  - Every part's pattern sits in a resizable dock at the bottom, as stacked lanes on one shared time ruler. Snap runs from 1/4 to 1/64, with zoom and follow-playhead; the ruler doubles as the horizontal scrollbar.
  - Melodic lanes have a key strip and their own pitch scroll: chords on the FM synth and sampler, one note at a time on the bass synths with slides drawn as ties. Sampler rows are labelled with their slot and slice.
  - Drum lanes have eight labelled rows, and hits can sit at any snap.
  - Each lane has a header (collapse, pattern label, Strip toggle) and a handle to change its height.
  - An expression strip per lane: velocity, or accent and slide on the bass synths, or accent on the drums. The drum accent strip accents every hit on a step; double-click a hit to toggle its own accent.
  - Type notes with the keyboard: click a lane to focus it, then use the tracker layout (Z S X D C V G B H N J M and Q 2 W 3 E R 5 T 6 Y 7 U I). Arrows move the cursor and Backspace steps back and deletes. Clicking outside the dock turns keyboard entry off.
  - The rack above keeps the knobs, pads, Len/Trans and dice.
- **Patterns:** 128 per machine, in banks A–H × 16. Each pattern is up to 64 steps, and a pattern switch queues to the next loop.
- **Mixer and effects:**
  - Six channels with per-channel distortion (2× oversampled) and compressor, level, pan, mute and solo.
  - Delay sends (tempo-synced, ping-pong) and reverb sends.
  - Master compressor, an 8-bit lo-fi switch, and meters.
- **Song mode:**
  - Arrange bars, each with a pattern per machine plus mutes and transpose (the two synths and the FM synth).
  - Loop a range of bars.
  - **Record** pattern changes and knob moves. Only the knobs you touch get overwritten, and each record pass is one undo step.
- **Smart randomise:**
  - Synth dice for Notes, Accent, Slide, Rest and Octave, with scales, groove-aware note placement, and slides that chain.
  - Drum Template and Evolve.
- **Files and editing:**
  - 200-level undo, and pattern copy/paste.
  - `.sidacid` project files.
  - WAV export of a loop or the whole song.

## Build and run

You need Rust 1.88+ (edition 2024, let-chains) and ALSA development files on Linux.

```sh
cargo run --release -p sidacid                         # the app
cargo run --release -p sidacid -- render-demo out.wav  # render the demo groove offline
cargo test --workspace                                 # run the tests
```

### Audio drivers

Linux builds include ALSA (which covers PipeWire and PulseAudio) and JACK.
JACK is loaded at runtime: without `libjack` it just isn't offered. The
build only needs pkg-config to report "jack"; `.cargo/config.toml` points
it at a stub in `support/pkgconfig/` unless you set `PKG_CONFIG_PATH`
yourself.

Windows builds include WASAPI. For ASIO, build on Windows with MSVC:

1. Download the Steinberg ASIO SDK and unzip it; set `CPAL_ASIO_DIR` to
   that folder.
2. Install LLVM and set `LIBCLANG_PATH` to its `bin` folder.
3. `cargo build --release -p sidacid --features asio`

Pick the driver and device under **File → Audio Settings…**.

## Keys

| Key | Action |
|---|---|
| Space | Play / stop. Pressing Stop while stopped returns to the song start. |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+C / Ctrl+V | Copy / paste a pattern |
| Ctrl+S / Ctrl+O / Ctrl+N | Save / open / new |

## Layout

| Crate | What it is |
|---|---|
| `crates/sid-core` | The SID chip: oscillators, envelopes, filter and 3-voice chip. No dependencies. |
| `crates/acid-engine` | The engine: synths, drums, effects, mixer, sequencer, song mode, randomisers, project files and offline render. It has no GUI or audio-device dependencies. |
| `crates/app` | The `sidacid` binary: egui UI and cpal live audio, with a lock-free command/event bridge to the audio thread. |

Design specs and implementation plans are in `docs/superpowers/`.

## Credits

The SID engine is derived from the synthesiser in [Acid OS](https://github.com/NorfolkHacker/Acid-OS-V3), MIT licensed. See `THIRD_PARTY_NOTICES.md`.

SidAcid is MIT licensed; see `LICENSE`.
