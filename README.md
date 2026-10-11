# SidAcid

A ReBirth-style acid groovebox built on a C64 SID-style sound engine. It runs as a native desktop app and is laid out for the Steam Deck screen.

![SidAcid](docs/images/screenshot.png)

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
- **Two sequencer views per synth,** swappable at any time over the same pattern:
  - The classic **303 step editor**, with tracker-style step-write.
  - A **piano roll**: click to add, drag to move or stretch, right-click to delete, plus accent and slide lanes.
- **Patterns:** 128 per machine, in banks A–H × 16. Each pattern is up to 64 steps, and a pattern switch queues to the next loop.
- **Mixer and effects:**
  - Per-channel distortion (2× oversampled) and compressor, level, pan, mute and solo.
  - Delay sends (tempo-synced, ping-pong) and reverb sends.
  - Master compressor, an 8-bit lo-fi switch, and meters.
- **Song mode:**
  - Arrange bars, each with a pattern per machine plus mutes and transpose.
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

## Keys

| Key | Action |
|---|---|
| Space | Play / stop. Pressing Stop while stopped returns to the song start. |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+C / Ctrl+V | Copy / paste a pattern |
| Ctrl+S / Ctrl+O / Ctrl+N | Save / open / new |

**Step-write** starts when you click a note cell in the 303 view:

| Key | Action |
|---|---|
| Z-row (Z S X D C V G B H N J M) | One octave of notes |
| Q-row | The octave above |
| `[` / `]` | Octave down / up |
| A | Toggle accent |
| F | Toggle slide |
| `-` | Tie |
| Space | Rest |
| Esc | Leave step-write |

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
