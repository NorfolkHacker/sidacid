# SidAcid Plan 1 — Sound Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the complete SidAcid sound engine — forked SID chip, two 303-style SID bass synths, an 808-style and a 909-style SID drum kit, FX and mixer, a pattern-mode sequencer, the project file format — and a `sidacid render-demo out.wav` command that proves it all by ear.

**Architecture:** A Cargo workspace with three crates. `sid-core` is a dependency-free SID chip (oscillators, envelopes, filter, 3-voice chip). `acid-engine` builds every instrument, effect and the sequencer on top of it, and has no GUI or audio-device dependencies, so everything is tested by rendering offline. `app` is the binary crate. In this plan it only has the `render-demo` CLI; Plan 2 adds the GUI.

**Tech Stack:** Rust 1.99 (edition 2024), serde 1 + ron 0.8 (project files), hound 3.5 (WAV).

**Spec:** `docs/superpowers/specs/2026-10-11-sid-acid-design.md`

**Plan series:** This is Plan 1 of 3.
- Plan 2: GUI app, live cpal audio, command queue, 303 and piano-roll editors, pattern banks, undo, save/load.
- Plan 3: song mode and automation recording, smart randomise, WAV export of songs.

## Global Constraints

- Standalone project. No dependency on, or files read from, Acid OS at build time or at runtime.
- Code derived from Acid OS `acid-synth` keeps the attribution header and `THIRD_PARTY_NOTICES.md` (MIT, © 2026 NorfolkHacker).
- Rust edition 2024, workspace resolver 3. `sid-core` has **zero** dependencies.
- `acid-engine` must not depend on any GUI or audio-device crate.
- All DSP is f32, at the sample rate passed in (tests use 44 100 or 48 000).
- Each project has 2 synths, 2 drum kits, 4 mixer channels, 128 patterns per machine (8 banks × 16) and 64 steps per pattern; pattern length is 1–64.
- Real-time rule for the audio path (`Engine::process` and everything it calls): no allocation, no locks, no I/O. Allocate in constructors and `load_project` only.
- Test command, run from the repo root: `cargo test --workspace`.

### Deliberate deviations from the spec (recorded here so reviewers don't flag them)

1. **Filter (spec §3):** uses a zero-delay-feedback (TPT) state-variable filter with a soft clip inside the loop, instead of an oversampled Chamberlin SVF. It meets the same goal: stable up to near self-oscillation at 44.1/48 kHz. It runs as a stereo pair with shared settings, so unison voices can be panned before the filter.
2. **Knob storage (spec §7):** knobs are stored as `params: BTreeMap<ParamId, f32>` (0–1 normalised) instead of separate `SynthKnobs`/`MixerState`/`FxState` structs. It is the same information, and song automation in Plan 3 can address it by `ParamId`.
3. **Fixed-size arrays (spec §7):** fixed-size step and pattern arrays are stored as `Vec`s that always have exactly 64 or 128 entries. Serde can't derive arrays longer than 32. Validation runs on load.
4. **Delay time (spec §6):** a `DelaySync` toggle switches between tempo divisions (1/16, 1/8, 3/16, 1/4, 3/8, 1/2) and free time (10–2000 ms).
5. **Engine interface:** the `Engine` exposes direct methods. The lock-free command queue wraps them in Plan 2.

---

## File Structure

```
sidsynth/
  Cargo.toml                         workspace
  LICENSE                            MIT © 2026 NorfolkHacker
  THIRD_PARTY_NOTICES.md             acid-synth derivation notice
  .gitignore
  crates/sid-core/
    Cargo.toml
    src/lib.rs                       re-exports
    src/osc.rs                       Oscillator, Waveform (phase accumulator, waveforms, noise LFSR)
    src/env.rs                       Envelope, EnvStage (linear attack, exponential decay/release)
    src/svf.rs                       Svf (TPT state-variable filter, LP/BP/HP)
    src/chip.rs                      SidChip, Voice, SyncMode (3 voices, ring, sync, pan, stereo filter)
  crates/acid-engine/
    Cargo.toml
    src/lib.rs                       module list + test helpers
    src/params.rs                    ParamId registry, defaults, value mapping helpers
    src/project.rs                   Project, patterns, song types, RON save/load
    src/synth.rs                     BassSynth (303 control layer over SidChip)
    src/drums/mod.rs                 DrumPatch, DrumVoice, DrumKit (voice pool, choke)
    src/drums/kit808.rs              808-style patch builder
    src/drums/kit909.rs              909-style patch builder
    src/fx/mod.rs                    fx module list
    src/fx/distortion.rs
    src/fx/compressor.rs
    src/fx/delay.rs
    src/fx/reverb.rs
    src/fx/lofi.rs
    src/mixer.rs                     StereoBuf, ChannelStrip, Mixer
    src/sequencer.rs                 Sequencer, SeqEvent (clock, swing, pattern playback)
    src/engine.rs                    Engine (ties it all together)
    src/demo.rs                      demo_project()
    src/render.rs                    offline render + WAV writer
  crates/app/
    Cargo.toml
    src/main.rs                      `sidacid render-demo [out.wav]`
```

---

### Task 1: Workspace scaffold and SID oscillator

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `LICENSE`, `THIRD_PARTY_NOTICES.md`
- Create: `crates/sid-core/Cargo.toml`, `crates/sid-core/src/lib.rs`, `crates/sid-core/src/osc.rs`

**Interfaces:**
- Produces:
  - `sid_core::Waveform { Pulse, Saw, Triangle, Noise }`, which derives `Default` (Pulse) and `Copy`.
  - `sid_core::Oscillator` with pub fields `waveform`, `phase: u32`, `increment: u32`, `pulse_threshold: u32` and `lfsr: u32`, plus these methods:
    - `new(seed: u32) -> Self`
    - `set_freq(&mut self, hz: f32, sample_rate: f32)`
    - `freq(&self, sample_rate: f32) -> f32`
    - `set_pulse_width(&mut self, width: f32)`
    - `msb(&self) -> bool`
    - `sample(&self, ring_msb: bool) -> f32`, returning −1..1
    - `advance(&mut self) -> bool`, returning true when the phase wrapped
    - `reset_phase(&mut self)`

- [ ] **Step 1: Create the workspace files**

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/sid-core", "crates/acid-engine", "crates/app"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
ron = "0.8"
hound = "3.5"

# Audio tests render seconds of sound; unoptimised builds are too slow.
[profile.dev]
opt-level = 2
```

`.gitignore`:
```
/target
*.wav
```

`LICENSE`: the standard MIT licence text with the line `Copyright (c) 2026 NorfolkHacker`. Generate it by copying the Acid OS licence, which already has exactly that copyright line:
```bash
cp /home/deck/Acid-OS-V3/LICENSE LICENSE
```

`THIRD_PARTY_NOTICES.md`:
```markdown
# Third-party notices

`crates/sid-core` is derived from the Acid OS synthesiser (`acid-synth`),
https://github.com/NorfolkHacker/Acid-OS-V3, used under the MIT licence:

    MIT License — Copyright (c) 2026 NorfolkHacker
    (full text identical to LICENSE in this repository)
```

Temporarily make the workspace build with only sid-core. Create placeholder crates so the `members` list resolves; Tasks 5 and 14 fill them in:

`crates/acid-engine/Cargo.toml`:
```toml
[package]
name = "acid-engine"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
sid-core = { path = "../sid-core" }
```
`crates/acid-engine/src/lib.rs`:
```rust
//! SidAcid sound engine.
```
`crates/app/Cargo.toml`:
```toml
[package]
name = "sidacid"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "sidacid"
path = "src/main.rs"

[dependencies]
acid-engine = { path = "../acid-engine" }
```
`crates/app/src/main.rs`:
```rust
fn main() {}
```

`crates/sid-core/Cargo.toml`:
```toml
[package]
name = "sid-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
```

`crates/sid-core/src/lib.rs`:
```rust
//! SID-style sound chip for SidAcid.
//!
//! Derived from the Acid OS synthesiser (`acid-synth`), MIT licence,
//! Copyright (c) 2026 NorfolkHacker. See THIRD_PARTY_NOTICES.md.

mod osc;

pub use osc::{Oscillator, Waveform};
```

- [ ] **Step 2: Write the failing oscillator tests**

`crates/sid-core/src/osc.rs`, with the tests first and an empty implementation:
```rust
//! Oscillator: 32-bit phase accumulator, the four SID waveforms and the
//! noise LFSR. Waveform shapes and LFSR taps come from Acid OS acid-synth.

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn frequency_is_accurate() {
        let mut o = Oscillator::new(0);
        o.set_freq(441.0, 44_100.0);
        let wraps = (0..44_100).filter(|_| o.advance()).count();
        assert!((440..=441).contains(&wraps), "wraps = {wraps}");
        assert!((o.freq(44_100.0) - 441.0).abs() < 0.01);
    }

    #[test]
    fn frequency_is_clamped_below_nyquist() {
        let mut o = Oscillator::new(0);
        o.set_freq(1.0e9, 44_100.0);
        assert!(o.increment < 1 << 31);
    }

    #[test]
    fn saw_shape() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Saw;
        o.phase = 0;
        assert!(approx(o.sample(false), -1.0));
        o.phase = 0x8000_0000;
        assert!(approx(o.sample(false), 0.0));
    }

    #[test]
    fn triangle_shape_and_ring_mod() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Triangle;
        o.phase = 0x2000_0000;
        assert!(approx(o.sample(false), -0.5));
        // Ring mod flips the half of the cycle when the partner's msb is set.
        assert!(approx(o.sample(true), 0.5));
        o.phase = 0x4000_0000;
        assert!(approx(o.sample(false), 0.0));
    }

    #[test]
    fn pulse_width() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Pulse;
        o.set_pulse_width(0.25);
        o.phase = 0x1000_0000;
        assert_eq!(o.sample(false), 1.0);
        o.phase = 0x5000_0000;
        assert_eq!(o.sample(false), -1.0);
    }

    #[test]
    fn noise_lfsr_taps_match_acid_synth() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Noise;
        o.increment = 1 << 28; // exactly one LFSR clock per sample
        o.advance();
        // 0xACE1 >> 1 = 0x5670, lsb was 1, xor 0xB400 -> 0xE270.
        assert_eq!(o.lfsr, 0xE270);
        assert!(approx(o.sample(false), 0x70 as f32 / 127.5 - 1.0));
    }

    #[test]
    fn noise_clocks_sixteen_times_per_cycle() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Noise;
        o.set_freq(1000.0, 48_000.0);
        let mut changes = 0;
        let mut prev = o.lfsr;
        for _ in 0..48_000 {
            o.advance();
            if o.lfsr != prev {
                changes += 1;
                prev = o.lfsr;
            }
        }
        assert!((15_990..=16_010).contains(&changes), "changes = {changes}");
    }

    #[test]
    fn reset_phase_zeroes() {
        let mut o = Oscillator::new(0);
        o.phase = 1234;
        o.reset_phase();
        assert_eq!(o.phase, 0);
    }
}
```

- [ ] **Step 3: Run the tests to confirm they fail**

Run: `cargo test -p sid-core`
Expected: compile errors (`cannot find type Oscillator`).

- [ ] **Step 4: Implement the oscillator**

Insert above the `#[cfg(test)]` block in `osc.rs`:
```rust
const PHASE_SCALE: f64 = 4_294_967_296.0;
/// Maps a 24-bit value to 0..2.
const TO_UNIT: f32 = 2.0 / 16_777_216.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Waveform {
    #[default]
    Pulse,
    Saw,
    Triangle,
    Noise,
}

#[derive(Clone, Debug)]
pub struct Oscillator {
    pub waveform: Waveform,
    pub phase: u32,
    pub increment: u32,
    pub pulse_threshold: u32,
    pub lfsr: u32,
    noise_out: f32,
}

impl Oscillator {
    pub fn new(seed: u32) -> Self {
        Self {
            waveform: Waveform::Pulse,
            phase: 0,
            increment: 0,
            pulse_threshold: 1 << 31,
            lfsr: 0xACE1 + seed,
            noise_out: 0.0,
        }
    }

    pub fn set_freq(&mut self, hz: f32, sample_rate: f32) {
        let inc = hz.max(0.0) as f64 / sample_rate as f64 * PHASE_SCALE;
        self.increment = inc.min(PHASE_SCALE / 2.0 - 1.0) as u32;
    }

    pub fn freq(&self, sample_rate: f32) -> f32 {
        (self.increment as f64 * sample_rate as f64 / PHASE_SCALE) as f32
    }

    /// Duty cycle 0.01..0.99 (0.5 = square).
    pub fn set_pulse_width(&mut self, width: f32) {
        self.pulse_threshold = (width.clamp(0.01, 0.99) as f64 * PHASE_SCALE) as u32;
    }

    pub fn msb(&self) -> bool {
        self.phase >> 31 != 0
    }

    /// Current output, -1..1. `ring_msb` is the ring partner's msb when
    /// ring modulation is on (triangle only, as on the SID).
    pub fn sample(&self, ring_msb: bool) -> f32 {
        match self.waveform {
            Waveform::Saw => (self.phase >> 8) as f32 * TO_UNIT - 1.0,
            Waveform::Triangle => {
                let msb = self.msb() ^ ring_msb;
                let lower = self.phase << 1;
                let tri = if msb { !lower } else { lower };
                (tri >> 8) as f32 * TO_UNIT - 1.0
            }
            Waveform::Pulse => {
                if self.phase < self.pulse_threshold { 1.0 } else { -1.0 }
            }
            Waveform::Noise => self.noise_out,
        }
    }

    /// Advances one sample; returns true when the phase wrapped. Noise
    /// clocks its LFSR every 1/16 cycle (each time bit 28 carries).
    pub fn advance(&mut self) -> bool {
        let old = self.phase;
        let sum = old as u64 + self.increment as u64;
        self.phase = sum as u32;
        if self.waveform == Waveform::Noise {
            let clocks = (sum >> 28) - (old as u64 >> 28);
            for _ in 0..clocks {
                self.clock_noise();
            }
        }
        sum > u32::MAX as u64
    }

    pub fn reset_phase(&mut self) {
        self.phase = 0;
    }

    fn clock_noise(&mut self) {
        noise_advance(&mut self.lfsr);
        self.noise_out = (self.lfsr & 0xFF) as f32 / 127.5 - 1.0;
    }
}

fn noise_advance(lfsr: &mut u32) {
    let mut v = *lfsr & 0xFFFF;
    let lsb = v & 1;
    v >>= 1;
    if lsb != 0 {
        v ^= 0xB400;
    }
    *lfsr = v;
}
```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test -p sid-core`
Expected: 8 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml .gitignore LICENSE THIRD_PARTY_NOTICES.md crates
git commit -m "feat(sid-core): workspace scaffold and SID oscillator"
```

---

### Task 2: Envelope (linear attack, exponential decay and release)

**Files:**
- Create: `crates/sid-core/src/env.rs`
- Modify: `crates/sid-core/src/lib.rs`

**Interfaces:**
- Produces:
  - `sid_core::EnvStage { Off, Attack, Decay, Sustain, Release }`.
  - `sid_core::Envelope` with pub fields `stage` and `level: f32`, plus these methods:
    - `new() -> Self`
    - `set_adsr(&mut self, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32, sample_rate: f32)`
    - `gate_on(&mut self)`, which starts the attack from the current level
    - `gate_off(&mut self)`
    - `tick(&mut self) -> f32`, returning a level from 0 to 1
- Decay and release times are the time taken to fall by 60 dB.

- [ ] **Step 1: Write the failing tests**

`crates/sid-core/src/env.rs`:
```rust
//! ADSR envelope: linear attack, exponential decay and release (times are
//! to -60 dB). gate_on restarts the attack from the current level, so
//! retriggering never clicks.

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 1000.0;

    #[test]
    fn new_envelope_is_off_and_silent() {
        let mut e = Envelope::new();
        assert_eq!(e.stage, EnvStage::Off);
        assert_eq!(e.tick(), 0.0);
    }

    #[test]
    fn linear_attack_takes_attack_time() {
        let mut e = Envelope::new();
        e.set_adsr(0.008, 0.0, 1.0, 0.0, SR); // 8 samples, step 0.125 (exact in f32)
        e.gate_on();
        for _ in 0..7 {
            e.tick();
        }
        assert!(e.level < 1.0 && e.level > 0.85);
        e.tick();
        assert_eq!(e.level, 1.0);
        assert_eq!(e.stage, EnvStage::Decay);
    }

    #[test]
    fn decay_is_exponential_to_minus_60_db() {
        let mut e = Envelope::new();
        e.set_adsr(0.0, 0.1, 0.0, 0.0, SR);
        e.gate_on();
        e.tick(); // instant attack -> 1.0
        for _ in 0..50 {
            e.tick();
        }
        // Halfway through a -60 dB decay is -30 dB = 0.0316.
        assert!((e.level - 0.0316).abs() < 0.003, "level = {}", e.level);
        for _ in 0..50 {
            e.tick();
        }
        assert!((e.level - 0.001).abs() < 0.0003, "level = {}", e.level);
    }

    #[test]
    fn decay_settles_on_sustain() {
        let mut e = Envelope::new();
        e.set_adsr(0.0, 0.01, 0.5, 0.0, SR);
        e.gate_on();
        for _ in 0..200 {
            e.tick();
        }
        assert_eq!(e.stage, EnvStage::Sustain);
        assert_eq!(e.level, 0.5);
    }

    #[test]
    fn release_reaches_off() {
        let mut e = Envelope::new();
        e.set_adsr(0.0, 0.0, 1.0, 0.05, SR);
        e.gate_on();
        e.tick();
        e.gate_off();
        assert_eq!(e.stage, EnvStage::Release);
        for _ in 0..200 {
            e.tick();
        }
        assert_eq!(e.stage, EnvStage::Off);
        assert_eq!(e.level, 0.0);
    }

    #[test]
    fn gate_off_when_off_stays_off() {
        let mut e = Envelope::new();
        e.gate_off();
        assert_eq!(e.stage, EnvStage::Off);
    }

    #[test]
    fn retrigger_attacks_from_current_level() {
        let mut e = Envelope::new();
        e.set_adsr(0.010, 0.0, 1.0, 0.1, SR);
        e.gate_on();
        for _ in 0..20 {
            e.tick();
        }
        e.gate_off();
        for _ in 0..10 {
            e.tick();
        }
        let before = e.level;
        assert!(before > 0.1 && before < 1.0);
        e.gate_on();
        e.tick();
        assert!(e.level > before, "attack continues upward from {before}");
    }
}
```

Add `mod env;` and `pub use env::{EnvStage, Envelope};` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p sid-core env`
Expected: compile errors (`cannot find type Envelope`).

- [ ] **Step 3: Implement the envelope**

Insert above the tests in `env.rs`:
```rust
/// Level treated as silence (-100 dB).
const SILENCE: f32 = 1.0e-5;
/// ln(0.001): an exponential that falls 60 dB.
const LN_MINUS_60_DB: f32 = -6.907_755;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvStage {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Debug)]
pub struct Envelope {
    pub stage: EnvStage,
    pub level: f32,
    attack_step: f32,
    decay_coef: f32,
    sustain: f32,
    release_coef: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-sample multiplier that falls 60 dB in `seconds` (0 = instant).
fn fall_coef(seconds: f32, sample_rate: f32) -> f32 {
    if seconds <= 0.0 { 0.0 } else { (LN_MINUS_60_DB / (seconds * sample_rate)).exp() }
}

impl Envelope {
    pub fn new() -> Self {
        Self { stage: EnvStage::Off, level: 0.0, attack_step: 1.0, decay_coef: 0.0, sustain: 1.0, release_coef: 0.0 }
    }

    pub fn set_adsr(&mut self, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32, sample_rate: f32) {
        self.attack_step = if attack_s <= 0.0 { 1.0 } else { 1.0 / (attack_s * sample_rate) };
        self.decay_coef = fall_coef(decay_s, sample_rate);
        self.sustain = sustain.clamp(0.0, 1.0);
        self.release_coef = fall_coef(release_s, sample_rate);
    }

    pub fn gate_on(&mut self) {
        self.stage = EnvStage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != EnvStage::Off {
            self.stage = EnvStage::Release;
        }
    }

    pub fn tick(&mut self) -> f32 {
        match self.stage {
            EnvStage::Off => self.level = 0.0,
            EnvStage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
            }
            EnvStage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_coef;
                if self.level - self.sustain <= SILENCE {
                    self.level = self.sustain;
                    self.stage = EnvStage::Sustain;
                }
            }
            EnvStage::Sustain => self.level = self.sustain,
            EnvStage::Release => {
                self.level *= self.release_coef;
                if self.level <= SILENCE {
                    self.level = 0.0;
                    self.stage = EnvStage::Off;
                }
            }
        }
        self.level
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p sid-core`
Expected: all pass (8 oscillator + 7 envelope tests).

- [ ] **Step 5: Commit**

```bash
git add crates/sid-core
git commit -m "feat(sid-core): exponential ADSR envelope"
```

---

### Task 3: State-variable filter

**Files:**
- Create: `crates/sid-core/src/svf.rs`
- Modify: `crates/sid-core/src/lib.rs`

**Interfaces:**
- Produces:
  - Constants `sid_core::{FILTER_LP: u8 = 1, FILTER_BP: u8 = 2, FILTER_HP: u8 = 4}`.
  - `sid_core::Svf` with a pub field `mode: u8` (a bitmask), plus these methods:
    - `new(sample_rate: f32) -> Self`
    - `set(&mut self, cutoff_hz: f32, resonance: f32)`, where resonance runs from 0 to 1
    - `process(&mut self, x: f32) -> f32`
    - `reset(&mut self)`

- [ ] **Step 1: Write the failing tests**

`crates/sid-core/src/svf.rs`:
```rust
//! Multimode state-variable filter (TPT / zero-delay-feedback form, after
//! Andrew Simper) with a soft clip on the band-pass state. Unconditionally
//! stable at any cutoff below Nyquist; the clip gives the squelchy,
//! self-limiting resonance a SID/303 filter wants.

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: f32 = 44_100.0;

    fn sine(freq: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (TAU * freq * i as f32 / SR).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn run(f: &mut Svf, input: &[f32]) -> Vec<f32> {
        input.iter().map(|&x| f.process(x)).collect()
    }

    #[test]
    fn lowpass_passes_dc() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(1000.0, 0.0);
        let out = run(&mut f, &vec![0.5; 4410]);
        assert!((out[4409] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn lowpass_attenuates_highs() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(200.0, 0.0);
        let out = run(&mut f, &sine(8000.0, 8820));
        assert!(rms(&out[4410..]) < 0.01, "rms = {}", rms(&out[4410..]));
    }

    #[test]
    fn highpass_blocks_dc() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_HP;
        f.set(500.0, 0.0);
        let out = run(&mut f, &vec![0.5; 44_100]);
        assert!(out[44_099].abs() < 1e-3);
    }

    #[test]
    fn bandpass_peaks_at_cutoff() {
        let mut at = Svf::new(SR);
        at.mode = FILTER_BP;
        at.set(1000.0, 0.5);
        let mut away = at.clone();
        let on = run(&mut at, &sine(1000.0, 8820));
        let off = run(&mut away, &sine(8000.0, 8820));
        assert!(rms(&on[4410..]) > 4.0 * rms(&off[4410..]));
    }

    #[test]
    fn max_resonance_is_bounded_and_finite() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(18_000.0, 1.0);
        let mut x = 0x1234_5678u32;
        for _ in 0..SR as usize {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let n = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
            let y = f.process(n);
            assert!(y.is_finite() && y.abs() < 10.0, "y = {y}");
        }
    }

    #[test]
    fn resonance_rings_after_impulse() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(500.0, 0.95);
        let mut input = vec![0.0; 4410];
        input[0] = 1.0;
        let out = run(&mut f, &input);
        assert!(rms(&out[441..882]) > 1e-3, "still ringing 10 ms later");
    }

    #[test]
    fn cutoff_is_clamped_to_stable_range() {
        let mut f = Svf::new(SR);
        f.mode = FILTER_LP;
        f.set(1.0e6, 0.5);
        let y = run(&mut f, &sine(100.0, 1000));
        assert!(y.iter().all(|v| v.is_finite()));
    }
}
```

Add `mod svf;` and `pub use svf::{Svf, FILTER_BP, FILTER_HP, FILTER_LP};` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p sid-core svf`
Expected: compile errors (`cannot find type Svf`).

- [ ] **Step 3: Implement the filter**

Insert above the tests in `svf.rs`:
```rust
use std::f32::consts::PI;

pub const FILTER_LP: u8 = 1;
pub const FILTER_BP: u8 = 2;
pub const FILTER_HP: u8 = 4;

/// Band-pass state limit; lower = earlier, harder resonance squelch.
const STATE_LIMIT: f32 = 3.0;

#[derive(Clone, Debug)]
pub struct Svf {
    pub mode: u8,
    sample_rate: f32,
    cutoff: f32,
    resonance: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic1: f32,
    ic2: f32,
}

impl Svf {
    pub fn new(sample_rate: f32) -> Self {
        let mut f = Self {
            mode: FILTER_LP,
            sample_rate,
            cutoff: -1.0,
            resonance: -1.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            ic1: 0.0,
            ic2: 0.0,
        };
        f.set(1000.0, 0.0);
        f
    }

    /// cutoff in Hz (clamped 20 Hz .. min(18 kHz, 0.45 * sample rate)),
    /// resonance 0..1 (Q 0.707 .. ~50).
    pub fn set(&mut self, cutoff_hz: f32, resonance: f32) {
        if cutoff_hz == self.cutoff && resonance == self.resonance {
            return;
        }
        self.cutoff = cutoff_hz;
        self.resonance = resonance;
        let fc = cutoff_hz.clamp(20.0, (0.45 * self.sample_rate).min(18_000.0));
        let g = (PI * fc / self.sample_rate).tan();
        self.k = 1.414 - 1.394 * resonance.clamp(0.0, 1.0);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = soft_clip(2.0 * v1 - self.ic1);
        self.ic2 = 2.0 * v2 - self.ic2;
        if self.ic1.abs() < 1.0e-20 {
            self.ic1 = 0.0;
        }
        if self.ic2.abs() < 1.0e-20 {
            self.ic2 = 0.0;
        }
        let mut out = 0.0;
        if self.mode & FILTER_LP != 0 {
            out += v2;
        }
        if self.mode & FILTER_BP != 0 {
            out += v1;
        }
        if self.mode & FILTER_HP != 0 {
            out += x - self.k * v1 - v2;
        }
        out
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

fn soft_clip(x: f32) -> f32 {
    STATE_LIMIT * (x / STATE_LIMIT).tanh()
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p sid-core`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sid-core
git commit -m "feat(sid-core): TPT state-variable filter with soft-clipped resonance"
```

---

### Task 4: SidChip (3 voices, ring mod, hard sync, pan, stereo filter)

**Files:**
- Create: `crates/sid-core/src/chip.rs`
- Modify: `crates/sid-core/src/lib.rs`

**Interfaces:**
- Consumes: `Oscillator`, `Waveform`, `Envelope`, `Svf`, `FILTER_*` from Tasks 1–3.
- Produces:
  - `sid_core::SyncMode { Off, Classic, Hidden }`.
  - `sid_core::Voice` with pub fields `osc: Oscillator`, `env: Envelope`, `master: Oscillator`, `ring: bool`, `sync: SyncMode`, `filter_route: bool` and `level: f32`.
  - `sid_core::SidChip` with the constant `SidChip::VOICES: usize = 3`. All voice indices are `usize` in `0..3`, and an out-of-range index panics. Methods:
    - `new(sample_rate: f32) -> Self` and `sample_rate(&self) -> f32`
    - `voice(&self, v) -> &Voice` and `voice_mut(&mut self, v) -> &mut Voice`
    - `set_waveform(v, Waveform)`, `set_freq_hz(v, f32)`, `freq_hz(&self, v) -> f32` and `set_pulse_width(v, f32)`
    - `set_adsr(v, attack_s, decay_s, sustain, release_s)`, `gate_on(v)` and `gate_off(v)`
    - `set_ring(v, bool)`, `set_sync(v, SyncMode)` and `set_sync_master_hz(v, f32)`
    - `set_filter_route(v, bool)`, `set_pan(v, f32 −1..1)` and `set_level(v, f32)`
    - `set_filter(cutoff_hz, resonance)` and `set_filter_mode(u8)`
    - `is_silent(&self) -> bool`
    - `render_frame(&mut self) -> (f32, f32)`
    - `render_add(&mut self, left: &mut [f32], right: &mut [f32])`
- Semantics:
  - **Ring mod and Classic sync** use the previous voice, `(v + 2) % 3`, as on the real SID (voice 0 is paired with voice 2).
  - **Hidden sync** resets the voice's phase whenever its own silent `master` oscillator wraps.

- [ ] **Step 1: Write the failing tests**

`crates/sid-core/src/chip.rs`:
```rust
//! A 3-voice SID chip: oscillator + envelope per voice, ring modulation and
//! hard sync from the previous voice, a per-voice hidden sync master (so
//! every voice can be synced at once), constant-power panning, and one
//! multimode filter run as a stereo pair with shared settings.

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;

    fn render(chip: &mut SidChip, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        chip.render_add(&mut l, &mut r);
        (l, r)
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    /// Voice v: instant attack, full sustain, short release, unfiltered.
    fn open_voice(chip: &mut SidChip, v: usize, wave: Waveform, hz: f32) {
        chip.set_waveform(v, wave);
        chip.set_freq_hz(v, hz);
        chip.set_adsr(v, 0.0, 0.0, 1.0, 0.005);
        chip.set_filter_route(v, false);
        chip.gate_on(v);
    }

    #[test]
    fn new_chip_is_silent() {
        let mut chip = SidChip::new(SR);
        assert!(chip.is_silent());
        let (l, r) = render(&mut chip, 512);
        assert_eq!(peak(&l) + peak(&r), 0.0);
    }

    #[test]
    fn gated_voice_sounds_and_release_silences() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Saw, 220.0);
        let (l, _) = render(&mut chip, 2205);
        assert!(peak(&l) > 0.5);
        chip.gate_off(0);
        render(&mut chip, 4410);
        assert!(chip.is_silent());
        let (l, _) = render(&mut chip, 512);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn freq_round_trips() {
        let mut chip = SidChip::new(SR);
        chip.set_freq_hz(1, 110.0);
        assert!((chip.freq_hz(1) - 110.0).abs() < 0.01);
    }

    #[test]
    fn centre_pan_is_equal_power() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Pulse, 100.0);
        let (l, r) = chip.render_frame();
        assert!((l - r).abs() < 1e-6);
        assert!((l.abs() - 0.7071).abs() < 1e-3);
    }

    #[test]
    fn hard_left_pan_silences_right() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Pulse, 100.0);
        chip.set_pan(0, -1.0);
        let (l, r) = render(&mut chip, 512);
        assert!(peak(&l) > 0.9);
        assert!(peak(&r) < 1e-6);
    }

    #[test]
    fn ring_mod_flips_triangle_using_previous_voice() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Triangle, 0.0);
        chip.voice_mut(0).osc.phase = 0x2000_0000; // triangle = -0.5
        chip.voice_mut(2).osc.phase = 0x8000_0000; // partner msb set (voice 2 is silent)
        let (plain, _) = chip.render_frame();
        chip.voice_mut(0).osc.phase = 0x2000_0000;
        chip.set_ring(0, true);
        let (ringed, _) = chip.render_frame();
        assert!((plain + 0.5 * 0.7071).abs() < 1e-3, "plain = {plain}");
        assert!((ringed - 0.5 * 0.7071).abs() < 1e-3, "ringed = {ringed}");
    }

    #[test]
    fn classic_sync_resets_when_previous_voice_wraps() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Saw, 100.0);
        open_voice(&mut chip, 1, Waveform::Saw, 370.0);
        chip.set_sync(1, SyncMode::Classic);
        let mut resets = 0;
        for _ in 0..SR as usize {
            let before = chip.voice(0).osc.phase;
            chip.render_frame();
            if chip.voice(0).osc.phase < before {
                assert_eq!(chip.voice(1).osc.phase, 0);
                resets += 1;
            }
        }
        assert!((99..=100).contains(&resets));
    }

    #[test]
    fn hidden_sync_resets_on_master_wrap() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Saw, 350.0);
        chip.set_sync(0, SyncMode::Hidden);
        chip.set_sync_master_hz(0, 100.0);
        let mut resets = 0;
        for _ in 0..SR as usize {
            let before = chip.voice(0).master.phase;
            chip.render_frame();
            if chip.voice(0).master.phase < before {
                assert_eq!(chip.voice(0).osc.phase, 0);
                resets += 1;
            }
        }
        assert!((99..=100).contains(&resets));
    }

    #[test]
    fn filter_route_applies_filter() {
        let mut dry = SidChip::new(SR);
        open_voice(&mut dry, 0, Waveform::Saw, 3000.0);
        let mut wet = SidChip::new(SR);
        open_voice(&mut wet, 0, Waveform::Saw, 3000.0);
        wet.set_filter_route(0, true);
        wet.set_filter_mode(crate::FILTER_LP);
        wet.set_filter(100.0, 0.0);
        let (d, _) = render(&mut dry, 4410);
        let (w, _) = render(&mut wet, 4410);
        assert!(peak(&w[2205..]) < 0.25 * peak(&d[2205..]));
    }

    #[test]
    fn level_scales_output() {
        let mut chip = SidChip::new(SR);
        open_voice(&mut chip, 0, Waveform::Pulse, 100.0);
        chip.set_level(0, 0.5);
        let (l, _) = chip.render_frame();
        assert!((l.abs() - 0.5 * 0.7071).abs() < 1e-3);
    }
}
```

Add `mod chip;` and `pub use chip::{SidChip, SyncMode, Voice};` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p sid-core chip`
Expected: compile errors (`cannot find type SidChip`).

- [ ] **Step 3: Implement the chip**

Insert above the tests in `chip.rs`:
```rust
use crate::{EnvStage, Envelope, Oscillator, Svf, Waveform, FILTER_LP};
use std::f32::consts::FRAC_PI_4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncMode {
    #[default]
    Off,
    /// Reset when the previous voice ((v + 2) % 3) wraps, as on the SID.
    Classic,
    /// Reset when this voice's own silent `master` oscillator wraps.
    Hidden,
}

#[derive(Clone, Debug)]
pub struct Voice {
    pub osc: Oscillator,
    pub env: Envelope,
    pub master: Oscillator,
    pub ring: bool,
    pub sync: SyncMode,
    pub filter_route: bool,
    pub level: f32,
    gain_l: f32,
    gain_r: f32,
}

impl Voice {
    fn new(index: usize) -> Self {
        Self {
            osc: Oscillator::new(index as u32),
            env: Envelope::new(),
            master: Oscillator::new(index as u32 + 16),
            ring: false,
            sync: SyncMode::Off,
            filter_route: false,
            level: 1.0,
            gain_l: FRAC_PI_4.cos(),
            gain_r: FRAC_PI_4.sin(),
        }
    }
}

pub struct SidChip {
    sample_rate: f32,
    voices: [Voice; 3],
    filters: [Svf; 2],
}

impl SidChip {
    pub const VOICES: usize = 3;

    pub fn new(sample_rate: f32) -> Self {
        let mut filters = [Svf::new(sample_rate), Svf::new(sample_rate)];
        for f in &mut filters {
            f.mode = FILTER_LP;
        }
        Self { sample_rate, voices: std::array::from_fn(Voice::new), filters }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn voice(&self, v: usize) -> &Voice {
        &self.voices[v]
    }

    pub fn voice_mut(&mut self, v: usize) -> &mut Voice {
        &mut self.voices[v]
    }

    pub fn set_waveform(&mut self, v: usize, wave: Waveform) {
        self.voices[v].osc.waveform = wave;
    }

    pub fn set_freq_hz(&mut self, v: usize, hz: f32) {
        let sr = self.sample_rate;
        self.voices[v].osc.set_freq(hz, sr);
    }

    pub fn freq_hz(&self, v: usize) -> f32 {
        self.voices[v].osc.freq(self.sample_rate)
    }

    pub fn set_pulse_width(&mut self, v: usize, width: f32) {
        self.voices[v].osc.set_pulse_width(width);
    }

    pub fn set_adsr(&mut self, v: usize, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32) {
        let sr = self.sample_rate;
        self.voices[v].env.set_adsr(attack_s, decay_s, sustain, release_s, sr);
    }

    pub fn gate_on(&mut self, v: usize) {
        self.voices[v].env.gate_on();
    }

    pub fn gate_off(&mut self, v: usize) {
        self.voices[v].env.gate_off();
    }

    pub fn set_ring(&mut self, v: usize, on: bool) {
        self.voices[v].ring = on;
    }

    pub fn set_sync(&mut self, v: usize, mode: SyncMode) {
        self.voices[v].sync = mode;
    }

    pub fn set_sync_master_hz(&mut self, v: usize, hz: f32) {
        let sr = self.sample_rate;
        self.voices[v].master.set_freq(hz, sr);
    }

    pub fn set_filter_route(&mut self, v: usize, routed: bool) {
        self.voices[v].filter_route = routed;
    }

    /// Constant-power pan, -1 (left) .. 1 (right).
    pub fn set_pan(&mut self, v: usize, pan: f32) {
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        self.voices[v].gain_l = angle.cos();
        self.voices[v].gain_r = angle.sin();
    }

    pub fn set_level(&mut self, v: usize, level: f32) {
        self.voices[v].level = level;
    }

    pub fn set_filter(&mut self, cutoff_hz: f32, resonance: f32) {
        for f in &mut self.filters {
            f.set(cutoff_hz, resonance);
        }
    }

    pub fn set_filter_mode(&mut self, mode: u8) {
        for f in &mut self.filters {
            f.mode = mode;
        }
    }

    pub fn is_silent(&self) -> bool {
        self.voices.iter().all(|v| v.env.stage == EnvStage::Off)
    }

    pub fn render_frame(&mut self) -> (f32, f32) {
        let msbs = [self.voices[0].osc.msb(), self.voices[1].osc.msb(), self.voices[2].osc.msb()];
        let (mut fl, mut fr, mut dl, mut dr) = (0.0, 0.0, 0.0, 0.0);
        for v in 0..3 {
            let voice = &mut self.voices[v];
            let ring_msb = voice.ring && msbs[(v + 2) % 3];
            let s = voice.osc.sample(ring_msb) * voice.env.tick() * voice.level;
            if voice.filter_route {
                fl += s * voice.gain_l;
                fr += s * voice.gain_r;
            } else {
                dl += s * voice.gain_l;
                dr += s * voice.gain_r;
            }
        }
        let mut wrapped = [false; 3];
        for v in 0..3 {
            wrapped[v] = self.voices[v].osc.advance();
        }
        for v in 0..3 {
            match self.voices[v].sync {
                SyncMode::Off => {}
                SyncMode::Classic => {
                    if wrapped[(v + 2) % 3] {
                        self.voices[v].osc.reset_phase();
                    }
                }
                SyncMode::Hidden => {
                    if self.voices[v].master.advance() {
                        self.voices[v].osc.reset_phase();
                    }
                }
            }
        }
        (self.filters[0].process(fl) + dl, self.filters[1].process(fr) + dr)
    }

    pub fn render_add(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (a, b) = self.render_frame();
            *l += a;
            *r += b;
        }
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p sid-core`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sid-core
git commit -m "feat(sid-core): 3-voice SidChip with ring mod, classic and hidden sync, stereo filter"
```

---

### Task 5: acid-engine scaffold and parameter registry

**Files:**
- Modify: `crates/acid-engine/Cargo.toml`, `crates/acid-engine/src/lib.rs`
- Create: `crates/acid-engine/src/params.rs`

**Interfaces:**
- Produces, in `acid_engine::params`:
  - Enums, each with `pub const ALL: [Self; N]` and `name(self) -> &'static str`:
    - `SynthParam` (15): Tune, Cutoff, Resonance, EnvMod, Decay, Accent, Wave, PulseWidth, FilterMode, Sync, SyncPitch, Unison, Detune, Spread, Volume.
    - `DrumParam` (4): Tune, Decay, Tone, Level.
    - `KitParam` (2): Volume, Accent.
    - `ChannelParam` (16): Level, Pan, Mute, Solo, DistOn, DistDrive, DistTone, DistMix, CompOn, CompThreshold, CompRatio, CompAttack, CompRelease, CompMakeup, DelaySend, ReverbSend.
    - `FxParam` (11): DelaySync, DelayDivision, DelayMs, DelayFeedback, DelayTone, DelayPingPong, DelayReturn, ReverbSize, ReverbDamping, ReverbWidth, ReverbReturn.
    - `MasterParam` (8): CompOn, CompThreshold, CompRatio, CompAttack, CompRelease, CompMakeup, Level, LoFi.
  - `ParamId` variants:
    - `Synth(u8 synth, SynthParam)`
    - `Drum(u8 kit, u8 inst, DrumParam)`
    - `Kit(u8 kit, KitParam)`
    - `Channel(u8 ch, ChannelParam)`
    - `Fx(FxParam)`
    - `Master(MasterParam)`
  - All of these derive `Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize`.
  - `ParamInfo { name: &'static str, default: f32, steps: u8 }`, where steps 0 means continuous and n means n discrete positions.
  - `ParamId::info(self) -> ParamInfo` and `ParamId::all() -> Vec<ParamId>`, which returns 181 entries.
  - Helpers:
    - `lin(v, lo, hi) -> f32`
    - `exp(v, lo, hi) -> f32`, where lo must be greater than 0
    - `step_index(v, steps: u8) -> usize`
    - `on(v) -> bool`, true when v ≥ 0.5
    - `db_to_gain(db) -> f32`
  - Constants `SYNTHS = 2`, `KITS = 2`, `CHANNELS = 4`, `DRUM_INSTRUMENTS = 8`.
  - A test helper in the crate root: `#[cfg(test)] pub(crate) mod testutil { rms, peak, all_finite }`.

- [ ] **Step 1: Set up the crate**

`crates/acid-engine/Cargo.toml`:
```toml
[package]
name = "acid-engine"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
sid-core = { path = "../sid-core" }
serde.workspace = true
ron.workspace = true
hound.workspace = true
```

`crates/acid-engine/src/lib.rs`:
```rust
//! SidAcid sound engine: instruments, effects, sequencer and project data.
//! No GUI or audio-device dependencies; everything renders offline.

pub mod params;

#[cfg(test)]
pub(crate) mod testutil {
    pub fn rms(x: &[f32]) -> f32 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    pub fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    pub fn all_finite(x: &[f32]) -> bool {
        x.iter().all(|v| v.is_finite())
    }
}
```

- [ ] **Step 2: Write the failing tests**

`crates/acid-engine/src/params.rs`, tests first:
```rust
//! Every automatable control, its name, default (0..1 normalised) and
//! number of discrete positions, plus helpers mapping 0..1 to real units.

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_has_every_param_once() {
        let all = ParamId::all();
        assert_eq!(all.len(), 2 * 15 + 2 * 8 * 4 + 2 * 2 + 4 * 16 + 11 + 8);
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
    }

    #[test]
    fn defaults_are_normalised_and_names_nonempty() {
        for id in ParamId::all() {
            let info = id.info();
            assert!((0.0..=1.0).contains(&info.default), "{id:?}");
            assert!(!info.name.is_empty());
        }
    }

    #[test]
    fn discrete_params_report_steps() {
        assert_eq!(ParamId::Synth(0, SynthParam::Wave).info().steps, 2);
        assert_eq!(ParamId::Synth(0, SynthParam::FilterMode).info().steps, 3);
        assert_eq!(ParamId::Fx(FxParam::DelayDivision).info().steps, 6);
        assert_eq!(ParamId::Synth(0, SynthParam::Cutoff).info().steps, 0);
    }

    #[test]
    fn mapping_helpers() {
        assert_eq!(lin(0.5, -12.0, 12.0), 0.0);
        assert!((exp(0.0, 20.0, 18_000.0) - 20.0).abs() < 1e-3);
        assert!((exp(1.0, 20.0, 18_000.0) - 18_000.0).abs() < 0.5);
        assert!((exp(0.5, 100.0, 10_000.0) - 1000.0).abs() < 0.1);
        assert_eq!(step_index(0.0, 3), 0);
        assert_eq!(step_index(0.5, 3), 1);
        assert_eq!(step_index(1.0, 3), 2);
        assert_eq!(step_index(7.0, 3), 2);
        assert!(on(0.5) && !on(0.49));
        assert!((db_to_gain(-6.0) - 0.501).abs() < 1e-3);
    }
}
```

Add nothing to `lib.rs` (`pub mod params;` is already there).

- [ ] **Step 3: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine params`
Expected: compile errors (`cannot find type ParamId`).

- [ ] **Step 4: Implement the registry**

Insert above the tests in `params.rs`:
```rust
use serde::{Deserialize, Serialize};

pub const SYNTHS: usize = 2;
pub const KITS: usize = 2;
pub const CHANNELS: usize = 4;
pub const DRUM_INSTRUMENTS: usize = 8;

macro_rules! param_enum {
    ($name:ident { $($var:ident => $label:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum $name { $($var),+ }
        impl $name {
            pub const ALL: [$name; [$($name::$var),+].len()] = [$($name::$var),+];
            pub fn name(self) -> &'static str {
                match self { $($name::$var => $label),+ }
            }
        }
    };
}

param_enum!(SynthParam {
    Tune => "Tune", Cutoff => "Cutoff", Resonance => "Resonance", EnvMod => "Env Mod",
    Decay => "Decay", Accent => "Accent", Wave => "Wave", PulseWidth => "Pulse Width",
    FilterMode => "Filter Mode", Sync => "Sync", SyncPitch => "Sync Pitch", Unison => "Unison",
    Detune => "Detune", Spread => "Spread", Volume => "Volume",
});
param_enum!(DrumParam { Tune => "Tune", Decay => "Decay", Tone => "Tone", Level => "Level" });
param_enum!(KitParam { Volume => "Volume", Accent => "Accent" });
param_enum!(ChannelParam {
    Level => "Level", Pan => "Pan", Mute => "Mute", Solo => "Solo",
    DistOn => "Dist On", DistDrive => "Drive", DistTone => "Dist Tone", DistMix => "Dist Mix",
    CompOn => "Comp On", CompThreshold => "Threshold", CompRatio => "Ratio",
    CompAttack => "Attack", CompRelease => "Release", CompMakeup => "Makeup",
    DelaySend => "Delay Send", ReverbSend => "Reverb Send",
});
param_enum!(FxParam {
    DelaySync => "Delay Sync", DelayDivision => "Delay Time", DelayMs => "Delay ms",
    DelayFeedback => "Feedback", DelayTone => "Delay Tone", DelayPingPong => "Ping-Pong",
    DelayReturn => "Delay Return", ReverbSize => "Size", ReverbDamping => "Damping",
    ReverbWidth => "Width", ReverbReturn => "Reverb Return",
});
param_enum!(MasterParam {
    CompOn => "Comp On", CompThreshold => "Threshold", CompRatio => "Ratio",
    CompAttack => "Attack", CompRelease => "Release", CompMakeup => "Makeup",
    Level => "Master", LoFi => "Lo-Fi",
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ParamId {
    Synth(u8, SynthParam),
    Drum(u8, u8, DrumParam),
    Kit(u8, KitParam),
    Channel(u8, ChannelParam),
    Fx(FxParam),
    Master(MasterParam),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamInfo {
    pub name: &'static str,
    pub default: f32,
    /// 0 = continuous; n = n discrete positions.
    pub steps: u8,
}

const fn cont(name: &'static str, default: f32) -> ParamInfo {
    ParamInfo { name, default, steps: 0 }
}

const fn disc(name: &'static str, default: f32, steps: u8) -> ParamInfo {
    ParamInfo { name, default, steps }
}

impl ParamId {
    pub fn info(self) -> ParamInfo {
        use ChannelParam as C;
        use FxParam as F;
        use MasterParam as M;
        use SynthParam as S;
        match self {
            ParamId::Synth(_, p) => {
                let n = p.name();
                match p {
                    S::Tune => cont(n, 0.5),
                    S::Cutoff => cont(n, 0.35),
                    S::Resonance => cont(n, 0.6),
                    S::EnvMod => cont(n, 0.5),
                    S::Decay => cont(n, 0.4),
                    S::Accent => cont(n, 0.6),
                    S::Wave => disc(n, 0.0, 2),
                    S::PulseWidth => cont(n, 0.5),
                    S::FilterMode => disc(n, 0.0, 3),
                    S::Sync => disc(n, 0.0, 2),
                    S::SyncPitch => cont(n, 0.5),
                    S::Unison => disc(n, 0.0, 2),
                    S::Detune => cont(n, 0.3),
                    S::Spread => cont(n, 0.6),
                    S::Volume => cont(n, 0.7),
                }
            }
            ParamId::Drum(_, _, p) => cont(p.name(), if p == DrumParam::Level { 0.8 } else { 0.5 }),
            ParamId::Kit(_, p) => cont(p.name(), if p == KitParam::Volume { 0.7 } else { 0.5 }),
            ParamId::Channel(_, p) => {
                let n = p.name();
                match p {
                    C::Level => cont(n, 0.7),
                    C::Pan => cont(n, 0.5),
                    C::Mute | C::Solo | C::DistOn | C::CompOn => disc(n, 0.0, 2),
                    C::DistDrive => cont(n, 0.3),
                    C::DistTone => cont(n, 0.6),
                    C::DistMix => cont(n, 1.0),
                    C::CompThreshold => cont(n, 0.7),
                    C::CompRatio => cont(n, 0.25),
                    C::CompAttack => cont(n, 0.2),
                    C::CompRelease => cont(n, 0.4),
                    C::CompMakeup => cont(n, 0.0),
                    C::DelaySend | C::ReverbSend => cont(n, 0.0),
                }
            }
            ParamId::Fx(p) => {
                let n = p.name();
                match p {
                    F::DelaySync => disc(n, 1.0, 2),
                    F::DelayDivision => disc(n, 0.4, 6),
                    F::DelayMs => cont(n, 0.3),
                    F::DelayFeedback => cont(n, 0.4),
                    F::DelayTone => cont(n, 0.6),
                    F::DelayPingPong => disc(n, 1.0, 2),
                    F::DelayReturn => cont(n, 0.7),
                    F::ReverbSize => cont(n, 0.6),
                    F::ReverbDamping => cont(n, 0.5),
                    F::ReverbWidth => cont(n, 1.0),
                    F::ReverbReturn => cont(n, 0.6),
                }
            }
            ParamId::Master(p) => {
                let n = p.name();
                match p {
                    M::CompOn => disc(n, 1.0, 2),
                    M::CompThreshold => cont(n, 0.8),
                    M::CompRatio => cont(n, 0.15),
                    M::CompAttack => cont(n, 0.3),
                    M::CompRelease => cont(n, 0.4),
                    M::CompMakeup => cont(n, 0.0),
                    M::Level => cont(n, 0.8),
                    M::LoFi => disc(n, 0.0, 2),
                }
            }
        }
    }

    pub fn all() -> Vec<ParamId> {
        let mut v = Vec::new();
        for s in 0..SYNTHS as u8 {
            v.extend(SynthParam::ALL.iter().map(|&p| ParamId::Synth(s, p)));
        }
        for k in 0..KITS as u8 {
            for i in 0..DRUM_INSTRUMENTS as u8 {
                v.extend(DrumParam::ALL.iter().map(|&p| ParamId::Drum(k, i, p)));
            }
            v.extend(KitParam::ALL.iter().map(|&p| ParamId::Kit(k, p)));
        }
        for c in 0..CHANNELS as u8 {
            v.extend(ChannelParam::ALL.iter().map(|&p| ParamId::Channel(c, p)));
        }
        v.extend(FxParam::ALL.iter().map(|&p| ParamId::Fx(p)));
        v.extend(MasterParam::ALL.iter().map(|&p| ParamId::Master(p)));
        v
    }
}

pub fn lin(v: f32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * v.clamp(0.0, 1.0)
}

/// Exponential map; `lo` must be > 0.
pub fn exp(v: f32, lo: f32, hi: f32) -> f32 {
    lo * (hi / lo).powf(v.clamp(0.0, 1.0))
}

pub fn step_index(v: f32, steps: u8) -> usize {
    (v.clamp(0.0, 1.0) * (steps.max(1) - 1) as f32).round() as usize
}

pub fn on(v: f32) -> bool {
    v >= 0.5
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}
```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test -p acid-engine`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): parameter registry and value mapping helpers"
```

---

### Task 6: Project data model and RON save/load

**Files:**
- Create: `crates/acid-engine/src/project.rs`
- Modify: `crates/acid-engine/src/lib.rs` (add `pub mod project;`)

**Interfaces:**
- Consumes: `ParamId` from Task 5.
- Produces, in `acid_engine::project`:
  - Constants `PROJECT_VERSION: u32 = 1`, `STEPS = 64`, `PATTERNS = 128`, `BANK_SIZE = 16`, `BASE_MIDI: i32 = 36`.
  - `Gate { Off, On, Tie }`.
  - `SynthStep { note: u8, octave: i8, gate: Gate, accent: bool, slide: bool }` with `midi(&self, transpose: i8) -> u8`. Notes run 0–11 within the octave, and octave is −1, 0 or +1.
  - `ParamLane { param: ParamId, values: Vec<Option<f32>> }`, which always has 64 values.
  - `SynthPattern { steps: Vec<SynthStep>, length: u8, transpose: i8, param_lane: Option<ParamLane> }`, which implements Default with 64 Off steps and a length of 16.
  - `DrumHit { Off, On, Accent }`.
  - `DrumPattern { steps: Vec<[DrumHit; 8]>, length: u8 }`, which implements Default with 64 steps and a length of 16.
  - `SeqView { Step303, PianoRoll }`.
  - `SynthMachine { patterns: Vec<SynthPattern>, view: SeqView }` and `DrumMachine { patterns: Vec<DrumPattern> }`, each holding 128 patterns.
  - `PatternRef(pub u8)` with `label(self) -> String`, giving "A01" to "H16".
  - `Bar { patterns: [Option<PatternRef>; 4], mutes: [bool; 4], transpose: [i8; 2] }`.
  - `AutoLane { param: ParamId, points: BTreeMap<u32, f32> }`.
  - `Song { bars: Vec<Bar>, loop_range: Option<(u16, u16)>, automation: Vec<AutoLane> }`, which implements Default with no bars.
  - `Project` with fields:
    - `version: u32`, `name: String`, `bpm: f32`, `swing: f32`
    - `synths: [SynthMachine; 2]`, `drums: [DrumMachine; 2]`
    - `params: BTreeMap<ParamId, f32>`, `selected: [u8; 4]`, `song: Song`
  - `Project` methods:
    - `Default`, with every param at its default value, bpm 125 and swing 0
    - `param(&self, id) -> f32`
    - `validate(&self) -> Result<(), ProjectError>`
    - `to_ron(&self) -> String` and `from_ron(&str) -> Result<Project, ProjectError>`
    - `save(&self, path: &Path) -> Result<(), ProjectError>`, which writes atomically
    - `load(path: &Path) -> Result<Project, ProjectError>`
  - `ProjectError { Io(std::io::Error), Parse(String), TooNew(u32), Invalid(String) }`, which implements `Display` and `std::error::Error`.

- [ ] **Step 1: Write the failing tests**

`crates/acid-engine/src/project.rs`, tests first:
```rust
//! Project data: patterns for all four machines, knob values, song. Saved
//! as RON (`.sidacid`). Fixed-size arrays are Vecs of exact length, checked
//! by `validate` on load.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::SynthParam;

    #[test]
    fn default_project_shape() {
        let p = Project::default();
        assert_eq!(p.version, PROJECT_VERSION);
        assert_eq!(p.synths[0].patterns.len(), PATTERNS);
        assert_eq!(p.drums[1].patterns.len(), PATTERNS);
        assert_eq!(p.synths[0].patterns[0].steps.len(), STEPS);
        assert_eq!(p.drums[0].patterns[0].steps.len(), STEPS);
        assert_eq!(p.synths[0].patterns[0].length, 16);
        assert_eq!(p.params.len(), ParamId::all().len());
        p.validate().unwrap();
    }

    #[test]
    fn step_midi_uses_octave_note_and_transpose() {
        let s = SynthStep { note: 3, octave: 1, gate: Gate::On, accent: false, slide: false };
        assert_eq!(s.midi(0), 36 + 12 + 3);
        assert_eq!(s.midi(-2), 36 + 12 + 1);
        let low = SynthStep { note: 0, octave: -1, ..s };
        assert_eq!(low.midi(0), 24);
    }

    #[test]
    fn pattern_ref_labels() {
        assert_eq!(PatternRef(0).label(), "A01");
        assert_eq!(PatternRef(37).label(), "C06");
        assert_eq!(PatternRef(127).label(), "H16");
    }

    #[test]
    fn param_lookup_falls_back_to_default() {
        let mut p = Project::default();
        let id = ParamId::Synth(1, SynthParam::Cutoff);
        p.params.remove(&id);
        assert_eq!(p.param(id), id.info().default);
        p.params.insert(id, 0.9);
        assert_eq!(p.param(id), 0.9);
    }

    #[test]
    fn ron_round_trip() {
        let mut p = Project::default();
        p.name = "Test".into();
        p.bpm = 133.0;
        p.synths[1].patterns[5].steps[3] = SynthStep { note: 7, octave: -1, gate: Gate::On, accent: true, slide: true };
        p.drums[0].patterns[2].steps[4][0] = DrumHit::Accent;
        p.params.insert(ParamId::Synth(0, SynthParam::Resonance), 0.95);
        p.song.bars.push(Bar { patterns: [Some(PatternRef(5)), None, Some(PatternRef(2)), None], mutes: [false, true, false, false], transpose: [0, 3] });
        let text = p.to_ron();
        let back = Project::from_ron(&text).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn newer_version_is_rejected() {
        let mut p = Project::default();
        p.version = PROJECT_VERSION + 1;
        let text = p.to_ron();
        match Project::from_ron(&text) {
            Err(ProjectError::TooNew(v)) => assert_eq!(v, PROJECT_VERSION + 1),
            other => panic!("expected TooNew, got {other:?}"),
        }
    }

    #[test]
    fn invalid_shapes_are_rejected() {
        let mut p = Project::default();
        p.synths[0].patterns[0].length = 0;
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.drums[1].patterns.pop();
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.selected[2] = 200;
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn garbage_is_a_parse_error() {
        assert!(matches!(Project::from_ron("not ron"), Err(ProjectError::Parse(_))));
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("sidacid-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.sidacid");
        let mut p = Project::default();
        p.name = "Saved".into();
        p.save(&path).unwrap();
        assert!(!path.with_extension("sidacid.tmp").exists());
        let back = Project::load(&path).unwrap();
        assert_eq!(back.name, "Saved");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
```

Add `pub mod project;` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine project`
Expected: compile errors (`cannot find type Project`).

- [ ] **Step 3: Implement the data model**

Insert above the tests in `project.rs`:
```rust
use crate::params::{ParamId, DRUM_INSTRUMENTS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub const PROJECT_VERSION: u32 = 1;
pub const STEPS: usize = 64;
pub const PATTERNS: usize = 128;
pub const BANK_SIZE: usize = 16;
/// MIDI note of step note 0 at octave 0 (C2).
pub const BASE_MIDI: i32 = 36;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gate {
    #[default]
    Off,
    On,
    /// Continue the previous note without retriggering.
    Tie,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthStep {
    pub note: u8,
    pub octave: i8,
    pub gate: Gate,
    pub accent: bool,
    pub slide: bool,
}

impl SynthStep {
    pub fn midi(&self, transpose: i8) -> u8 {
        (BASE_MIDI + 12 * self.octave as i32 + self.note as i32 + transpose as i32).clamp(0, 127) as u8
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamLane {
    pub param: ParamId,
    pub values: Vec<Option<f32>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthPattern {
    pub steps: Vec<SynthStep>,
    pub length: u8,
    pub transpose: i8,
    pub param_lane: Option<ParamLane>,
}

impl Default for SynthPattern {
    fn default() -> Self {
        Self { steps: vec![SynthStep::default(); STEPS], length: 16, transpose: 0, param_lane: None }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrumHit {
    #[default]
    Off,
    On,
    Accent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrumPattern {
    pub steps: Vec<[DrumHit; DRUM_INSTRUMENTS]>,
    pub length: u8,
}

impl Default for DrumPattern {
    fn default() -> Self {
        Self { steps: vec![[DrumHit::Off; DRUM_INSTRUMENTS]; STEPS], length: 16 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeqView {
    #[default]
    Step303,
    PianoRoll,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthMachine {
    pub patterns: Vec<SynthPattern>,
    pub view: SeqView,
}

impl Default for SynthMachine {
    fn default() -> Self {
        Self { patterns: vec![SynthPattern::default(); PATTERNS], view: SeqView::Step303 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrumMachine {
    pub patterns: Vec<DrumPattern>,
}

impl Default for DrumMachine {
    fn default() -> Self {
        Self { patterns: vec![DrumPattern::default(); PATTERNS] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternRef(pub u8);

impl PatternRef {
    /// "A01" .. "H16".
    pub fn label(self) -> String {
        let bank = (b'A' + self.0 / BANK_SIZE as u8) as char;
        format!("{bank}{:02}", self.0 as usize % BANK_SIZE + 1)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bar {
    pub patterns: [Option<PatternRef>; 4],
    pub mutes: [bool; 4],
    pub transpose: [i8; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoLane {
    pub param: ParamId,
    pub points: BTreeMap<u32, f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub bars: Vec<Bar>,
    pub loop_range: Option<(u16, u16)>,
    pub automation: Vec<AutoLane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub synths: [SynthMachine; 2],
    pub drums: [DrumMachine; 2],
    pub params: BTreeMap<ParamId, f32>,
    /// Selected pattern per machine: synth 1, synth 2, 808, 909.
    pub selected: [u8; 4],
    pub song: Song,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: PROJECT_VERSION,
            name: "Untitled".into(),
            bpm: 125.0,
            swing: 0.0,
            synths: Default::default(),
            drums: Default::default(),
            params: ParamId::all().into_iter().map(|id| (id, id.info().default)).collect(),
            selected: [0; 4],
            song: Song::default(),
        }
    }
}

#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Parse(String),
    TooNew(u32),
    Invalid(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "file error: {e}"),
            ProjectError::Parse(e) => write!(f, "not a valid SidAcid project: {e}"),
            ProjectError::TooNew(v) => write!(f, "project version {v} is newer than this SidAcid ({PROJECT_VERSION})"),
            ProjectError::Invalid(e) => write!(f, "project is damaged: {e}"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        ProjectError::Io(e)
    }
}

#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

impl Project {
    pub fn param(&self, id: ParamId) -> f32 {
        self.params.get(&id).copied().unwrap_or(id.info().default)
    }

    pub fn validate(&self) -> Result<(), ProjectError> {
        let bad = |m: String| Err(ProjectError::Invalid(m));
        for (i, m) in self.synths.iter().enumerate() {
            if m.patterns.len() != PATTERNS {
                return bad(format!("synth {} has {} patterns", i + 1, m.patterns.len()));
            }
            for (j, p) in m.patterns.iter().enumerate() {
                if p.steps.len() != STEPS || !(1..=STEPS as u8).contains(&p.length) {
                    return bad(format!("synth {} pattern {}", i + 1, PatternRef(j as u8).label()));
                }
                if let Some(lane) = &p.param_lane {
                    if lane.values.len() != STEPS {
                        return bad(format!("synth {} pattern {} param lane", i + 1, PatternRef(j as u8).label()));
                    }
                }
            }
        }
        for (i, m) in self.drums.iter().enumerate() {
            if m.patterns.len() != PATTERNS {
                return bad(format!("drum machine {} has {} patterns", i + 1, m.patterns.len()));
            }
            for (j, p) in m.patterns.iter().enumerate() {
                if p.steps.len() != STEPS || !(1..=STEPS as u8).contains(&p.length) {
                    return bad(format!("drum machine {} pattern {}", i + 1, PatternRef(j as u8).label()));
                }
            }
        }
        if self.selected.iter().any(|&s| s as usize >= PATTERNS) {
            return bad("selected pattern out of range".into());
        }
        if self.song.bars.iter().flat_map(|b| b.patterns).flatten().any(|r| r.0 as usize >= PATTERNS) {
            return bad("song references a pattern out of range".into());
        }
        Ok(())
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("project serialises")
    }

    pub fn from_ron(text: &str) -> Result<Project, ProjectError> {
        let head: VersionOnly = ron::from_str(text).map_err(|e| ProjectError::Parse(e.to_string()))?;
        if head.version > PROJECT_VERSION {
            return Err(ProjectError::TooNew(head.version));
        }
        let p: Project = ron::from_str(text).map_err(|e| ProjectError::Parse(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    /// Writes `path.tmp` then renames over `path`, so a crash never leaves
    /// a half-written project.
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        let tmp = path.with_extension("sidacid.tmp");
        std::fs::write(&tmp, self.to_ron())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Project, ProjectError> {
        Project::from_ron(&std::fs::read_to_string(path)?)
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p acid-engine`
Expected: all pass.

If `newer_version_is_rejected` fails with a `Parse` error, the `VersionOnly` read is rejecting unknown fields. ron 0.8 ignores unknown struct fields by default, so check that `to_ron` is not writing struct names (`PrettyConfig::default()` doesn't) and that `VersionOnly` has no `deny_unknown_fields` attribute.

- [ ] **Step 5: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): project data model with RON save/load and validation"
```

---

### Task 7: BassSynth (303 control layer)

**Files:**
- Create: `crates/acid-engine/src/synth.rs`
- Modify: `crates/acid-engine/src/lib.rs` (add `pub mod synth;`)

**Interfaces:**
- Consumes:
  - `sid_core::{SidChip, Waveform, SyncMode, FILTER_LP, FILTER_BP, FILTER_HP}`.
  - `params::{SynthParam, ParamId, lin, exp, step_index, on}`.
- Produces `acid_engine::synth::BassSynth` with these methods:
  - `new(sample_rate: f32) -> Self`, which applies every SynthParam default
  - `set_param(&mut self, p: SynthParam, v: f32)`, where v is normalised 0–1
  - `note_on(&mut self, midi: u8, accent: bool, legato: bool)`
  - `note_off(&mut self)`
  - `gate_open(&self) -> bool`
  - `render(&mut self, left: &mut [f32], right: &mut [f32])`, which adds into the buffers
  - Inspection helpers: `chip(&self) -> &SidChip`, `filter_env(&self) -> f32`, `accent_cap(&self) -> f32`
- Semantics:
  - `legato == true` while the gate is open means slide: the pitch glides and the envelopes do not retrigger.
  - The sequencer decides when a note is legato.

- [ ] **Step 1: Write the failing tests**

`crates/acid-engine/src/synth.rs`, tests first:
```rust
//! The 303-style bass synth: one SID chip, a filter envelope with Env Mod
//! and Decay, accent (level + short decay + the "accent wow" sweep that
//! builds over consecutive accents), slide, and 3-voice detuned unison.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak, rms};

    const SR: f32 = 44_100.0;

    fn render(s: &mut BassSynth, seconds: f32) -> Vec<f32> {
        let n = (seconds * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        s.render(&mut l, &mut r);
        l
    }

    #[test]
    fn silent_without_notes() {
        let mut s = BassSynth::new(SR);
        assert_eq!(peak(&render(&mut s, 0.1)), 0.0);
    }

    #[test]
    fn note_sets_pitch_and_sounds() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false); // A2 = 110 Hz
        assert!((s.chip().freq_hz(0) - 110.0).abs() < 0.1);
        assert!(rms(&render(&mut s, 0.1)) > 0.01);
    }

    #[test]
    fn tune_shifts_pitch() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Tune, 1.0); // +12 semitones
        s.note_on(45, false, false);
        assert!((s.chip().freq_hz(0) - 220.0).abs() < 0.2);
    }

    #[test]
    fn note_off_releases_to_silence() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false);
        render(&mut s, 0.1);
        s.note_off();
        assert!(!s.gate_open());
        render(&mut s, 0.1);
        assert!(peak(&render(&mut s, 0.05)) < 1e-4);
    }

    #[test]
    fn new_note_retriggers_filter_envelope() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false);
        render(&mut s, 0.3);
        assert!(s.filter_env() < 0.5);
        s.note_on(48, false, false);
        assert_eq!(s.filter_env(), 1.0);
    }

    #[test]
    fn legato_glides_without_retrigger() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false);
        render(&mut s, 0.3);
        let env = s.filter_env();
        s.note_on(57, false, true); // up an octave, sliding
        assert!(s.filter_env() <= env, "envelope not retriggered");
        render(&mut s, 0.005);
        let mid = s.chip().freq_hz(0);
        assert!(mid > 111.0 && mid < 215.0, "gliding, mid = {mid}");
        render(&mut s, 0.15);
        assert!((s.chip().freq_hz(0) - 220.0).abs() < 1.0);
    }

    #[test]
    fn legato_with_closed_gate_is_a_normal_note() {
        let mut s = BassSynth::new(SR);
        s.note_on(57, false, true);
        assert!((s.chip().freq_hz(0) - 220.0).abs() < 0.2);
        assert_eq!(s.filter_env(), 1.0);
    }

    #[test]
    fn accent_is_louder() {
        let mut plain = BassSynth::new(SR);
        let mut acc = BassSynth::new(SR);
        for s in [&mut plain, &mut acc] {
            s.set_param(SynthParam::Accent, 1.0);
        }
        plain.note_on(45, false, false);
        acc.note_on(45, true, false);
        let a = rms(&render(&mut acc, 0.1));
        let p = rms(&render(&mut plain, 0.1));
        assert!(a > 1.3 * p, "accent {a} vs plain {p}");
    }

    #[test]
    fn accent_sweep_builds_over_consecutive_accents() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Accent, 1.0);
        let peak_cap = |s: &mut BassSynth| {
            let mut m: f32 = 0.0;
            for _ in 0..(0.125 * SR) as usize / 64 {
                render_n(s, 64);
                m = m.max(s.accent_cap());
            }
            m
        };
        s.note_on(45, true, false);
        let first = peak_cap(&mut s);
        s.note_on(45, true, false);
        let second = peak_cap(&mut s);
        assert!(second > first * 1.05, "first {first}, second {second}");
    }

    fn render_n(s: &mut BassSynth, n: usize) {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        s.render(&mut l, &mut r);
    }

    #[test]
    fn unison_plays_three_detuned_voices() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Unison, 1.0);
        s.set_param(SynthParam::Detune, 1.0); // 50 cents
        s.note_on(45, false, false);
        let f: Vec<f32> = (0..3).map(|v| s.chip().freq_hz(v)).collect();
        assert!(f[0] < f[1] && f[1] < f[2], "{f:?}");
        assert!((f[1] - 110.0).abs() < 0.1);
        assert!((f[2] / f[1] - 2f32.powf(0.5 / 12.0)).abs() < 1e-3);
        assert!((0..3).all(|v| s.chip().voice(v).level > 0.0));
    }

    #[test]
    fn mono_mode_uses_one_voice() {
        let mut s = BassSynth::new(SR);
        s.note_on(45, false, false);
        assert!(s.chip().voice(0).level > 0.0);
        assert_eq!(s.chip().voice(1).level, 0.0);
        assert_eq!(s.chip().voice(2).level, 0.0);
    }

    #[test]
    fn unison_spread_is_stereo() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Unison, 1.0);
        s.set_param(SynthParam::Spread, 1.0);
        s.set_param(SynthParam::Detune, 1.0);
        s.note_on(45, false, false);
        let n = (0.2 * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        s.render(&mut l, &mut r);
        let diff: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
        assert!(rms(&diff) > 0.01);
    }

    #[test]
    fn sync_uses_hidden_master_at_note_pitch() {
        let mut s = BassSynth::new(SR);
        s.set_param(SynthParam::Sync, 1.0);
        s.set_param(SynthParam::SyncPitch, 0.5); // +12 st
        s.note_on(45, false, false);
        assert_eq!(s.chip().voice(0).sync, sid_core::SyncMode::Hidden);
        assert!((s.chip().voice(0).master.freq(SR) - 110.0).abs() < 0.1);
        assert!((s.chip().freq_hz(0) - 220.0).abs() < 0.2);
    }

    #[test]
    fn extreme_settings_stay_finite_and_bounded() {
        let mut s = BassSynth::new(SR);
        for p in SynthParam::ALL {
            s.set_param(p, 1.0);
        }
        s.note_on(30, true, false);
        let out = render(&mut s, 1.0);
        assert!(all_finite(&out));
        assert!(peak(&out) < 4.0);
    }
}
```

Add `pub mod synth;` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine synth`
Expected: compile errors (`cannot find type BassSynth`).

- [ ] **Step 3: Implement BassSynth**

Insert above the tests in `synth.rs`:
```rust
use crate::params::{exp, lin, on, step_index, ParamId, SynthParam};
use sid_core::{SidChip, SyncMode, Waveform, FILTER_BP, FILTER_HP, FILTER_LP};

/// Pitch, cutoff and envelope update every this many samples.
const CONTROL_BLOCK: usize = 16;
const BASE_LEVEL: f32 = 0.35;
/// Accented notes use this fixed short filter decay, as on the 303.
const ACCENT_DECAY_S: f32 = 0.2;
/// Glide time constant; settles in about 60 ms.
const SLIDE_TAU_S: f32 = 0.02;
const ACCENT_CHARGE_TAU_S: f32 = 0.03;
const ACCENT_DISCHARGE_TAU_S: f32 = 0.15;
const ENV_MOD_OCTAVES: f32 = 4.0;
const ACCENT_OCTAVES: f32 = 3.0;
const LN_MINUS_60_DB: f32 = -6.907_755;

fn midi_to_hz(semis: f32) -> f32 {
    440.0 * 2f32.powf((semis - 69.0) / 12.0)
}

pub struct BassSynth {
    chip: SidChip,
    sample_rate: f32,
    tune: f32,
    cutoff_hz: f32,
    resonance: f32,
    env_mod: f32,
    decay_s: f32,
    accent_amt: f32,
    wave: Waveform,
    pulse_width: f32,
    sync: bool,
    sync_semis: f32,
    unison: bool,
    detune_cents: f32,
    spread: f32,
    volume: f32,
    gate: bool,
    accent: bool,
    note_semis: f32,
    target_semis: f32,
    filter_env: f32,
    accent_cap: f32,
    control_counter: usize,
}

impl BassSynth {
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            chip: SidChip::new(sample_rate),
            sample_rate,
            tune: 0.0,
            cutoff_hz: 1000.0,
            resonance: 0.0,
            env_mod: 0.0,
            decay_s: 0.3,
            accent_amt: 0.0,
            wave: Waveform::Saw,
            pulse_width: 0.5,
            sync: false,
            sync_semis: 0.0,
            unison: false,
            detune_cents: 0.0,
            spread: 0.0,
            volume: 0.7,
            gate: false,
            accent: false,
            note_semis: 45.0,
            target_semis: 45.0,
            filter_env: 0.0,
            accent_cap: 0.0,
            control_counter: 0,
        };
        for p in SynthParam::ALL {
            s.set_param(p, ParamId::Synth(0, p).info().default);
        }
        s.apply_voice_setup();
        s
    }

    pub fn chip(&self) -> &SidChip {
        &self.chip
    }

    pub fn filter_env(&self) -> f32 {
        self.filter_env
    }

    pub fn accent_cap(&self) -> f32 {
        self.accent_cap
    }

    pub fn gate_open(&self) -> bool {
        self.gate
    }

    pub fn set_param(&mut self, p: SynthParam, v: f32) {
        match p {
            SynthParam::Tune => self.tune = lin(v, -12.0, 12.0),
            SynthParam::Cutoff => self.cutoff_hz = exp(v, 20.0, 18_000.0),
            SynthParam::Resonance => self.resonance = v.clamp(0.0, 1.0),
            SynthParam::EnvMod => self.env_mod = v.clamp(0.0, 1.0),
            SynthParam::Decay => self.decay_s = exp(v, 0.03, 2.5),
            SynthParam::Accent => self.accent_amt = v.clamp(0.0, 1.0),
            SynthParam::Wave => {
                self.wave = if step_index(v, 2) == 0 { Waveform::Saw } else { Waveform::Pulse };
                self.apply_voice_setup();
            }
            SynthParam::PulseWidth => {
                self.pulse_width = lin(v, 0.05, 0.95);
                self.apply_voice_setup();
            }
            SynthParam::FilterMode => {
                let mode = [FILTER_LP, FILTER_BP, FILTER_HP][step_index(v, 3)];
                self.chip.set_filter_mode(mode);
            }
            SynthParam::Sync => {
                self.sync = on(v);
                self.apply_voice_setup();
            }
            SynthParam::SyncPitch => self.sync_semis = lin(v, 0.0, 24.0),
            SynthParam::Unison => {
                self.unison = on(v);
                self.apply_voice_setup();
            }
            SynthParam::Detune => self.detune_cents = lin(v, 0.0, 50.0),
            SynthParam::Spread => {
                self.spread = v.clamp(0.0, 1.0);
                self.apply_voice_setup();
            }
            SynthParam::Volume => {
                self.volume = v.clamp(0.0, 1.0);
                self.update_levels();
            }
        }
        self.control(false);
    }

    fn voice_count(&self) -> usize {
        if self.unison { 3 } else { 1 }
    }

    fn apply_voice_setup(&mut self) {
        let count = self.voice_count();
        for v in 0..SidChip::VOICES {
            self.chip.set_waveform(v, self.wave);
            self.chip.set_pulse_width(v, self.pulse_width);
            self.chip.set_filter_route(v, true);
            self.chip.set_sync(v, if self.sync { SyncMode::Hidden } else { SyncMode::Off });
            self.chip.set_adsr(v, 0.003, 0.0, 1.0, 0.012);
            let pan = if self.unison { [-self.spread, 0.0, self.spread][v] } else { 0.0 };
            self.chip.set_pan(v, pan);
            if self.gate {
                if v < count {
                    self.chip.gate_on(v);
                } else {
                    self.chip.gate_off(v);
                }
            }
        }
        self.update_levels();
    }

    fn update_levels(&mut self) {
        let accent_gain = if self.accent { 1.0 + self.accent_amt } else { 1.0 };
        let per_voice = BASE_LEVEL * self.volume * accent_gain / if self.unison { 1.6 } else { 1.0 };
        let count = self.voice_count();
        for v in 0..SidChip::VOICES {
            self.chip.set_level(v, if v < count { per_voice } else { 0.0 });
        }
    }

    pub fn note_on(&mut self, midi: u8, accent: bool, legato: bool) {
        self.target_semis = midi as f32;
        self.accent = accent;
        if !(legato && self.gate) {
            self.note_semis = self.target_semis;
            self.filter_env = 1.0;
            let count = self.voice_count();
            for v in 0..SidChip::VOICES {
                if v < count {
                    self.chip.gate_on(v);
                } else {
                    self.chip.gate_off(v);
                }
            }
        }
        self.gate = true;
        self.update_levels();
        self.control(false);
    }

    pub fn note_off(&mut self) {
        self.gate = false;
        for v in 0..SidChip::VOICES {
            self.chip.gate_off(v);
        }
    }

    /// Recomputes pitch and cutoff. With `advance`, first moves glide,
    /// filter envelope and accent sweep forward by one control block.
    fn control(&mut self, advance: bool) {
        if advance {
            let n = CONTROL_BLOCK as f32;
            let sr = self.sample_rate;
            let glide_k = 1.0 - (-n / (SLIDE_TAU_S * sr)).exp();
            self.note_semis += (self.target_semis - self.note_semis) * glide_k;
            let decay = if self.accent { ACCENT_DECAY_S } else { self.decay_s };
            self.filter_env *= (LN_MINUS_60_DB * n / (decay * sr)).exp();
            let input = if self.accent && self.gate { self.filter_env } else { 0.0 };
            let tau = if input > self.accent_cap { ACCENT_CHARGE_TAU_S } else { ACCENT_DISCHARGE_TAU_S };
            self.accent_cap += (input - self.accent_cap) * (1.0 - (-n / (tau * sr)).exp());
        }
        let octaves = self.env_mod * self.filter_env * ENV_MOD_OCTAVES + self.accent_amt * self.accent_cap * ACCENT_OCTAVES;
        let cutoff = (self.cutoff_hz * 2f32.powf(octaves)).clamp(20.0, 18_000.0);
        self.chip.set_filter(cutoff, self.resonance);
        let base = self.note_semis + self.tune;
        for v in 0..self.voice_count() {
            let detune = if self.unison { [-1.0, 0.0, 1.0][v] * self.detune_cents / 100.0 } else { 0.0 };
            let hz = midi_to_hz(base + detune);
            if self.sync {
                self.chip.set_sync_master_hz(v, hz);
                self.chip.set_freq_hz(v, hz * 2f32.powf(self.sync_semis / 12.0));
            } else {
                self.chip.set_freq_hz(v, hz);
            }
        }
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.control_counter == 0 {
                self.control(true);
                self.control_counter = CONTROL_BLOCK;
            }
            self.control_counter -= 1;
            let (a, b) = self.chip.render_frame();
            *l += a;
            *r += b;
        }
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p acid-engine synth`
Expected: all pass.

If `accent_sweep_builds_over_consecutive_accents` fails, print `first` and `second`. The model only needs the discharge to be slower than the charge, so don't loosen the test; adjust `ACCENT_DISCHARGE_TAU_S` upward (up to 0.25) instead.

- [ ] **Step 5: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): 303-style BassSynth with accent sweep, slide and unison"
```

---

### Task 8: Drum engine, voice pool and 808 kit

**Files:**
- Create: `crates/acid-engine/src/drums/mod.rs`, `crates/acid-engine/src/drums/kit808.rs`
- Create: `crates/acid-engine/src/drums/kit909.rs`. This task only creates a stub that returns `kit808::patch`; Task 9 replaces it.
- Modify: `crates/acid-engine/src/lib.rs` (add `pub mod drums;`)

**Interfaces:**
- Consumes:
  - `sid_core::{Oscillator, Svf, Waveform, FILTER_*}`.
  - `params::{DrumParam, KitParam, DRUM_INSTRUMENTS}`.
- Produces, in `acid_engine::drums`:
  - Instrument index constants `BD=0, SD=1, LT=2, HT=3, CP=4, RS=5, CH=6, OH=7` and `POOL_SIZE = 6`.
  - `Osc2Mode { Off, Ring, Add }`.
  - `DrumPatch` with pub fields:
    - `tone_wave`, `tone_start_hz`, `tone_end_hz`, `pitch_decay_s`, `tone_decay_s`, `tone_level`, `pulse_width`
    - `click_wave: Option<Waveform>`, `click_s`
    - `osc2_mode`, `osc2_ratio`
    - `noise_level`, `noise_decay_s`, `noise_rate`, `noise_filter_mode: u8`, `noise_cutoff_hz`, `noise_res`
    - `bursts: u8`, `burst_spacing_s`
  - `DrumPatch` implements `Default`, which is silent.
  - `KitKind { K808, K909 }` with `patch(self, inst, tune, decay, tone) -> DrumPatch`.
  - `DrumKit` with these methods:
    - `new(kind, sample_rate)`
    - `set_param(&mut self, inst: usize, p: DrumParam, v: f32)`
    - `set_kit_param(&mut self, p: KitParam, v: f32)`
    - `trigger(&mut self, inst: usize, accent: bool)`
    - `active_voices(&self) -> usize`
    - `is_playing(&self, inst: usize) -> bool`
    - `render(&mut self, left: &mut [f32], right: &mut [f32])`, which adds into the buffers
  - `kit808::patch(inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch`.

- [ ] **Step 1: Write the failing tests**

`crates/acid-engine/src/drums/mod.rs`, tests first:
```rust
//! SID drum machines. Each drum is a `DrumPatch`: a small program that
//! sweeps an oscillator's pitch (optionally switching waveform for an
//! attack click, ring-modding or adding a second oscillator) and mixes in
//! filtered LFSR noise with an optional multi-burst (clap) envelope. A kit
//! turns its 4 knobs per instrument into patches and plays them on a pool
//! of 6 voices, each with its own filter.

pub mod kit808;
pub mod kit909;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 44_100.0;

    fn render(k: &mut DrumKit, seconds: f32) -> Vec<f32> {
        let n = (seconds * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        k.render(&mut l, &mut r);
        l
    }

    #[test]
    fn every_instrument_sounds_and_ends() {
        for kind in [KitKind::K808, KitKind::K909] {
            for inst in 0..DRUM_INSTRUMENTS {
                let mut k = DrumKit::new(kind, SR);
                k.set_param(inst, DrumParam::Decay, 1.0); // longest decay
                k.trigger(inst, false);
                let out = render(&mut k, 3.0);
                assert!(all_finite(&out), "{kind:?} {inst}");
                assert!(peak(&out) > 0.01, "{kind:?} {inst} is silent");
                assert!(peak(&out) < 2.0, "{kind:?} {inst} too loud");
                assert_eq!(k.active_voices(), 0, "{kind:?} {inst} never ends");
            }
        }
    }

    #[test]
    fn retrigger_reuses_the_instruments_voice() {
        let mut k = DrumKit::new(KitKind::K808, SR);
        k.trigger(BD, false);
        render(&mut k, 0.01);
        k.trigger(BD, false);
        assert_eq!(k.active_voices(), 1);
    }

    #[test]
    fn pool_is_limited_to_six_voices() {
        let mut k = DrumKit::new(KitKind::K808, SR);
        for inst in 0..DRUM_INSTRUMENTS {
            k.trigger(inst, false);
        }
        assert_eq!(k.active_voices(), POOL_SIZE);
    }

    #[test]
    fn closed_hat_chokes_open_hat() {
        let mut k = DrumKit::new(KitKind::K808, SR);
        k.set_param(OH, DrumParam::Decay, 1.0);
        k.trigger(OH, false);
        render(&mut k, 0.05);
        assert!(k.is_playing(OH));
        k.trigger(CH, false);
        render(&mut k, 0.1);
        assert!(!k.is_playing(OH));
    }

    #[test]
    fn accent_is_louder() {
        let mut a = DrumKit::new(KitKind::K808, SR);
        let mut p = DrumKit::new(KitKind::K808, SR);
        a.set_kit_param(KitParam::Accent, 1.0);
        p.set_kit_param(KitParam::Accent, 1.0);
        a.trigger(SD, true);
        p.trigger(SD, false);
        assert!(peak(&render(&mut a, 0.2)) > 1.5 * peak(&render(&mut p, 0.2)));
    }

    #[test]
    fn level_knob_zero_is_silent() {
        let mut k = DrumKit::new(KitKind::K808, SR);
        k.set_param(BD, DrumParam::Level, 0.0);
        k.trigger(BD, false);
        assert_eq!(peak(&render(&mut k, 0.2)), 0.0);
    }

    #[test]
    fn tune_raises_kick_pitch() {
        let low = kit808::patch(BD, 0.0, 0.5, 0.5);
        let high = kit808::patch(BD, 1.0, 0.5, 0.5);
        assert!(high.tone_end_hz > 3.0 * low.tone_end_hz);
    }

    #[test]
    fn clap_uses_bursts() {
        let p = kit808::patch(CP, 0.5, 0.5, 0.5);
        assert!(p.bursts >= 3);
        assert!(p.noise_level > 0.0);
    }
}
```

Create `crates/acid-engine/src/drums/kit909.rs` as a stub for now:
```rust
//! 909-flavoured SID drum patches. (Stub until Task 9.)

use super::DrumPatch;

pub fn patch(inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch {
    super::kit808::patch(inst, tune, decay, tone)
}
```

Add `pub mod drums;` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine drums`
Expected: compile errors (`cannot find type DrumKit`).

- [ ] **Step 3: Implement the drum engine**

Insert above the tests in `drums/mod.rs`:
```rust
use crate::params::{DrumParam, KitParam, DRUM_INSTRUMENTS};
use sid_core::{Oscillator, Svf, Waveform, FILTER_HP};

pub const BD: usize = 0;
pub const SD: usize = 1;
pub const LT: usize = 2;
pub const HT: usize = 3;
pub const CP: usize = 4;
pub const RS: usize = 5;
pub const CH: usize = 6;
pub const OH: usize = 7;
pub const POOL_SIZE: usize = 6;

const LN_MINUS_60_DB: f32 = -6.907_755;
const END_LEVEL: f32 = 1.0e-4;
const CHOKE_TAU_S: f32 = 0.005;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Osc2Mode {
    #[default]
    Off,
    /// Oscillator 1 (triangle) ring-modulated by oscillator 2.
    Ring,
    /// Oscillator 2 (same waveform) mixed with oscillator 1.
    Add,
}

#[derive(Clone, Copy, Debug)]
pub struct DrumPatch {
    pub tone_wave: Waveform,
    pub tone_start_hz: f32,
    pub tone_end_hz: f32,
    /// Time constant of the exponential pitch drop.
    pub pitch_decay_s: f32,
    /// Tone amplitude time to -60 dB.
    pub tone_decay_s: f32,
    pub tone_level: f32,
    pub pulse_width: f32,
    /// Waveform used for the first `click_s` seconds.
    pub click_wave: Option<Waveform>,
    pub click_s: f32,
    pub osc2_mode: Osc2Mode,
    pub osc2_ratio: f32,
    pub noise_level: f32,
    /// Noise tail time to -60 dB (after any bursts).
    pub noise_decay_s: f32,
    /// LFSR clocks per sample, 0..1.
    pub noise_rate: f32,
    pub noise_filter_mode: u8,
    pub noise_cutoff_hz: f32,
    pub noise_res: f32,
    pub bursts: u8,
    pub burst_spacing_s: f32,
}

impl Default for DrumPatch {
    fn default() -> Self {
        Self {
            tone_wave: Waveform::Triangle,
            tone_start_hz: 100.0,
            tone_end_hz: 100.0,
            pitch_decay_s: 0.01,
            tone_decay_s: 0.1,
            tone_level: 0.0,
            pulse_width: 0.5,
            click_wave: None,
            click_s: 0.0,
            osc2_mode: Osc2Mode::Off,
            osc2_ratio: 1.0,
            noise_level: 0.0,
            noise_decay_s: 0.1,
            noise_rate: 1.0,
            noise_filter_mode: FILTER_HP,
            noise_cutoff_hz: 1000.0,
            noise_res: 0.0,
            bursts: 0,
            burst_spacing_s: 0.01,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KitKind {
    K808,
    K909,
}

impl KitKind {
    pub fn patch(self, inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch {
        match self {
            KitKind::K808 => kit808::patch(inst, tune, decay, tone),
            KitKind::K909 => kit909::patch(inst, tune, decay, tone),
        }
    }
}

struct DrumVoice {
    active: bool,
    inst: usize,
    age: u64,
    patch: DrumPatch,
    osc1: Oscillator,
    osc2: Oscillator,
    noise: Oscillator,
    filter: Svf,
    sample_rate: f32,
    t: f32,
    gain: f32,
    pitch_env: f32,
    pitch_coef: f32,
    tone_amp: f32,
    tone_coef: f32,
    noise_amp: f32,
    noise_coef: f32,
    choking: bool,
    choke: f32,
    choke_coef: f32,
}

impl DrumVoice {
    fn new(sample_rate: f32, seed: u32) -> Self {
        let mut noise = Oscillator::new(seed * 7 + 3);
        noise.waveform = Waveform::Noise;
        Self {
            active: false,
            inst: 0,
            age: 0,
            patch: DrumPatch::default(),
            osc1: Oscillator::new(seed),
            osc2: Oscillator::new(seed + 1),
            noise,
            filter: Svf::new(sample_rate),
            sample_rate,
            t: 0.0,
            gain: 0.0,
            pitch_env: 1.0,
            pitch_coef: 0.0,
            tone_amp: 1.0,
            tone_coef: 0.0,
            noise_amp: 1.0,
            noise_coef: 0.0,
            choking: false,
            choke: 1.0,
            choke_coef: (-1.0 / (CHOKE_TAU_S * sample_rate)).exp(),
        }
    }

    fn start(&mut self, inst: usize, patch: DrumPatch, gain: f32, age: u64) {
        let sr = self.sample_rate;
        self.active = true;
        self.inst = inst;
        self.age = age;
        self.patch = patch;
        self.gain = gain;
        self.t = 0.0;
        self.pitch_env = 1.0;
        self.pitch_coef = (-1.0 / (patch.pitch_decay_s.max(1.0e-4) * sr)).exp();
        self.tone_amp = 1.0;
        self.tone_coef = (LN_MINUS_60_DB / (patch.tone_decay_s.max(1.0e-3) * sr)).exp();
        self.noise_amp = 1.0;
        self.noise_coef = (LN_MINUS_60_DB / (patch.noise_decay_s.max(1.0e-3) * sr)).exp();
        self.choking = false;
        self.choke = 1.0;
        self.osc1.reset_phase();
        self.osc2.reset_phase();
        self.osc1.set_pulse_width(patch.pulse_width);
        self.osc2.set_pulse_width(patch.pulse_width);
        self.osc2.waveform = patch.tone_wave;
        self.noise.set_freq(patch.noise_rate.clamp(0.0, 1.0) * sr / 16.0, sr);
        self.filter.reset();
        self.filter.mode = patch.noise_filter_mode;
        self.filter.set(patch.noise_cutoff_hz, patch.noise_res);
    }

    fn tick(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }
        let p = self.patch;
        let sr = self.sample_rate;

        let hz = p.tone_end_hz + (p.tone_start_hz - p.tone_end_hz) * self.pitch_env;
        self.pitch_env *= self.pitch_coef;
        self.osc1.waveform = match p.click_wave {
            Some(w) if self.t < p.click_s => w,
            _ => p.tone_wave,
        };
        self.osc1.set_freq(hz, sr);
        let raw_tone = match p.osc2_mode {
            Osc2Mode::Off => self.osc1.sample(false),
            Osc2Mode::Ring => {
                self.osc2.set_freq(hz * p.osc2_ratio, sr);
                self.osc1.sample(self.osc2.msb())
            }
            Osc2Mode::Add => {
                self.osc2.set_freq(hz * p.osc2_ratio, sr);
                0.5 * (self.osc1.sample(false) + self.osc2.sample(false))
            }
        };
        self.osc1.advance();
        self.osc2.advance();
        let tone = raw_tone * self.tone_amp * p.tone_level;
        self.tone_amp *= self.tone_coef;

        let burst_total = p.bursts as f32 * p.burst_spacing_s;
        let noise_env = if self.t < burst_total {
            let phase = self.t % p.burst_spacing_s;
            (-phase / (p.burst_spacing_s / 3.0)).exp()
        } else {
            let a = self.noise_amp;
            self.noise_amp *= self.noise_coef;
            a
        };
        let n = self.noise.sample(false);
        self.noise.advance();
        let noise = self.filter.process(n) * noise_env * p.noise_level;

        self.t += 1.0 / sr;
        if self.choking {
            self.choke *= self.choke_coef;
        }
        let out = (tone + noise) * self.gain * self.choke;

        let tone_done = self.tone_amp * p.tone_level < END_LEVEL;
        let noise_done = self.t > burst_total && noise_env * p.noise_level < END_LEVEL;
        if (tone_done && noise_done) || self.choke < END_LEVEL {
            self.active = false;
        }
        out
    }
}

pub struct DrumKit {
    kind: KitKind,
    voices: Vec<DrumVoice>,
    /// Normalised Tune, Decay, Tone, Level per instrument.
    knobs: [[f32; 4]; DRUM_INSTRUMENTS],
    volume: f32,
    accent_amt: f32,
    age: u64,
}

impl DrumKit {
    pub fn new(kind: KitKind, sample_rate: f32) -> Self {
        Self {
            kind,
            voices: (0..POOL_SIZE as u32).map(|i| DrumVoice::new(sample_rate, i * 3)).collect(),
            knobs: [[0.5, 0.5, 0.5, 0.8]; DRUM_INSTRUMENTS],
            volume: 0.7,
            accent_amt: 0.5,
            age: 0,
        }
    }

    pub fn set_param(&mut self, inst: usize, p: DrumParam, v: f32) {
        self.knobs[inst][p as usize] = v.clamp(0.0, 1.0);
    }

    pub fn set_kit_param(&mut self, p: KitParam, v: f32) {
        match p {
            KitParam::Volume => self.volume = v.clamp(0.0, 1.0),
            KitParam::Accent => self.accent_amt = v.clamp(0.0, 1.0),
        }
    }

    pub fn trigger(&mut self, inst: usize, accent: bool) {
        if inst == CH {
            for v in self.voices.iter_mut().filter(|v| v.active && v.inst == OH) {
                v.choking = true;
            }
        }
        let idx = self
            .voices
            .iter()
            .position(|v| v.active && v.inst == inst)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .unwrap_or_else(|| {
                (0..self.voices.len()).min_by_key(|&i| self.voices[i].age).expect("pool is not empty")
            });
        let [tune, decay, tone, level] = self.knobs[inst];
        let patch = self.kind.patch(inst, tune, decay, tone);
        let gain = level * level * if accent { 1.0 + self.accent_amt } else { 1.0 };
        self.age += 1;
        self.voices[idx].start(inst, patch, gain, self.age);
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn is_playing(&self, inst: usize) -> bool {
        self.voices.iter().any(|v| v.active && v.inst == inst)
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let master = self.volume * self.volume * 1.5;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mut s = 0.0;
            for v in &mut self.voices {
                s += v.tick();
            }
            s *= master;
            *l += s;
            *r += s;
        }
    }
}
```

`crates/acid-engine/src/drums/kit808.rs`:
```rust
//! 808-flavoured SID drum patches: long triangle booms, two-tone snare,
//! burst clap, noise hats. Knobs are normalised 0..1.

use super::{DrumPatch, Osc2Mode, BD, CH, CP, HT, LT, OH, RS, SD};
use sid_core::{Waveform, FILTER_BP, FILTER_HP};

/// Tune knob: one octave down to one octave up.
fn tune_factor(tune: f32) -> f32 {
    2f32.powf((tune - 0.5) * 2.0)
}

pub fn patch(inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch {
    let tf = tune_factor(tune);
    let base = DrumPatch::default();
    match inst {
        BD => DrumPatch {
            tone_wave: Waveform::Triangle,
            tone_end_hz: 50.0 * tf,
            tone_start_hz: 50.0 * tf * (1.5 + tone * 3.0),
            pitch_decay_s: 0.03,
            tone_decay_s: 0.15 + decay * 1.2,
            tone_level: 1.0,
            ..base
        },
        SD => DrumPatch {
            tone_wave: Waveform::Triangle,
            tone_start_hz: 330.0 * tf,
            tone_end_hz: 180.0 * tf,
            pitch_decay_s: 0.01,
            tone_decay_s: 0.12 + decay * 0.1,
            tone_level: 0.8 * (1.0 - tone * 0.6),
            osc2_mode: Osc2Mode::Add,
            osc2_ratio: 1.48,
            noise_level: 0.4 + tone * 0.6,
            noise_decay_s: 0.1 + decay * 0.25,
            noise_filter_mode: FILTER_HP,
            noise_cutoff_hz: 1500.0,
            noise_res: 0.2,
            ..base
        },
        LT | HT => {
            let f = if inst == LT { 110.0 } else { 175.0 } * tf;
            DrumPatch {
                tone_wave: Waveform::Triangle,
                tone_start_hz: f * (1.3 + tone * 0.7),
                tone_end_hz: f,
                pitch_decay_s: 0.05,
                tone_decay_s: 0.2 + decay * 0.6,
                tone_level: 0.9,
                noise_level: 0.05,
                noise_decay_s: 0.05,
                noise_cutoff_hz: 3000.0,
                ..base
            }
        }
        CP => DrumPatch {
            tone_level: 0.0,
            noise_level: 0.9,
            noise_decay_s: 0.1 + decay * 0.4,
            noise_filter_mode: FILTER_BP,
            noise_cutoff_hz: 1000.0 + tone * 1500.0,
            noise_res: 0.3,
            bursts: 3,
            burst_spacing_s: 0.011,
            ..base
        },
        RS => DrumPatch {
            tone_wave: Waveform::Pulse,
            tone_start_hz: 1700.0 * tf,
            tone_end_hz: 1700.0 * tf,
            tone_decay_s: 0.025 + decay * 0.02,
            tone_level: 0.5 + tone * 0.3,
            ..base
        },
        CH | OH => DrumPatch {
            tone_level: 0.0,
            noise_level: 0.7,
            noise_decay_s: if inst == CH { 0.03 + decay * 0.08 } else { 0.25 + decay * 0.8 },
            noise_filter_mode: FILTER_HP,
            noise_cutoff_hz: (6000.0 + tone * 5000.0) * tf.sqrt(),
            noise_res: 0.1,
            ..base
        },
        _ => base,
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p acid-engine drums`
Expected: all pass. Until Task 9, the 909 runs through the 808 patches.

- [ ] **Step 5: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): SID drum engine with voice pool, choke and 808 kit"
```

---

### Task 9: 909 kit

**Files:**
- Modify: `crates/acid-engine/src/drums/kit909.rs` (replace the stub)

**Interfaces:**
- Consumes: `DrumPatch`, `Osc2Mode` and the instrument constants from Task 8.
- Produces: `kit909::patch(inst, tune, decay, tone) -> DrumPatch`, which has different algorithms from the 808 kit.

- [ ] **Step 1: Write the failing tests**

Replace `crates/acid-engine/src/drums/kit909.rs` with the tests and the old stub body:
```rust
//! 909-flavoured SID drum patches: pulse-click punchy kick, snappy snare,
//! tighter clap, ring-mod rimshot and metallic ring-mod hats.

use super::DrumPatch;

pub fn patch(inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch {
    super::kit808::patch(inst, tune, decay, tone)
}

#[cfg(test)]
mod tests {
    use super::super::{kit808, Osc2Mode, BD, CH, CP, RS, SD};
    use super::*;
    use sid_core::Waveform;

    #[test]
    fn kick_has_pulse_click_and_fast_sweep() {
        let p = patch(BD, 0.5, 0.5, 0.5);
        assert_eq!(p.click_wave, Some(Waveform::Pulse));
        assert!(p.pitch_decay_s < kit808::patch(BD, 0.5, 0.5, 0.5).pitch_decay_s);
    }

    #[test]
    fn snare_noise_is_brighter_and_snappier_than_808() {
        let p = patch(SD, 0.5, 0.5, 1.0);
        assert!(p.noise_level >= 0.9);
    }

    #[test]
    fn hats_and_rimshot_use_ring_mod() {
        assert_eq!(patch(CH, 0.5, 0.5, 0.5).osc2_mode, Osc2Mode::Ring);
        assert_eq!(patch(RS, 0.5, 0.5, 0.5).osc2_mode, Osc2Mode::Ring);
    }

    #[test]
    fn clap_is_tighter_than_808() {
        let p = patch(CP, 0.5, 0.5, 0.5);
        assert!(p.burst_spacing_s < kit808::patch(CP, 0.5, 0.5, 0.5).burst_spacing_s);
        assert_eq!(p.bursts, 4);
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine kit909`
Expected: FAIL. With the stub, `click_wave` is `None`, and the other assertions fail too.

- [ ] **Step 3: Implement the 909 patches**

Replace the stub `patch` function and the `use` line above the tests with:
```rust
use super::{DrumPatch, Osc2Mode, BD, CH, CP, HT, LT, OH, RS, SD};
use sid_core::{Waveform, FILTER_BP, FILTER_HP};

fn tune_factor(tune: f32) -> f32 {
    2f32.powf((tune - 0.5) * 2.0)
}

pub fn patch(inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch {
    let tf = tune_factor(tune);
    let base = DrumPatch::default();
    match inst {
        BD => DrumPatch {
            tone_wave: Waveform::Triangle,
            click_wave: Some(Waveform::Pulse),
            click_s: 0.004,
            tone_end_hz: 50.0 * tf,
            tone_start_hz: 50.0 * tf * (3.0 + tone * 4.0),
            pitch_decay_s: 0.012,
            tone_decay_s: 0.15 + decay * 0.6,
            tone_level: 1.0,
            ..base
        },
        SD => DrumPatch {
            tone_wave: Waveform::Triangle,
            tone_start_hz: 260.0 * tf,
            tone_end_hz: 190.0 * tf,
            pitch_decay_s: 0.015,
            tone_decay_s: 0.08 + decay * 0.06,
            tone_level: 0.7,
            noise_level: 0.6 + tone * 0.4,
            noise_decay_s: 0.08 + decay * 0.35,
            noise_filter_mode: FILTER_HP,
            noise_cutoff_hz: 800.0,
            noise_res: 0.0,
            ..base
        },
        LT | HT => {
            let f = if inst == LT { 100.0 } else { 160.0 } * tf;
            DrumPatch {
                tone_wave: Waveform::Triangle,
                click_wave: Some(Waveform::Pulse),
                click_s: 0.003,
                tone_start_hz: f * (1.6 + tone * 1.0),
                tone_end_hz: f,
                pitch_decay_s: 0.06,
                tone_decay_s: 0.15 + decay * 0.5,
                tone_level: 0.9,
                ..base
            }
        }
        CP => DrumPatch {
            tone_level: 0.0,
            noise_level: 0.9,
            noise_decay_s: 0.08 + decay * 0.3,
            noise_filter_mode: FILTER_BP,
            noise_cutoff_hz: 1200.0 + tone * 1000.0,
            noise_res: 0.4,
            bursts: 4,
            burst_spacing_s: 0.008,
            ..base
        },
        RS => DrumPatch {
            tone_wave: Waveform::Triangle,
            tone_start_hz: 1100.0 * tf,
            tone_end_hz: 1100.0 * tf,
            tone_decay_s: 0.02 + decay * 0.02,
            tone_level: 0.7,
            osc2_mode: Osc2Mode::Ring,
            osc2_ratio: 1.37 + tone * 0.5,
            ..base
        },
        CH | OH => {
            let d = if inst == CH { 0.04 + decay * 0.1 } else { 0.2 + decay * 0.7 };
            DrumPatch {
                tone_wave: Waveform::Triangle,
                tone_start_hz: 3000.0 * tf,
                tone_end_hz: 3000.0 * tf,
                tone_decay_s: d,
                tone_level: 0.35,
                osc2_mode: Osc2Mode::Ring,
                osc2_ratio: 1.4142,
                noise_level: 0.7,
                noise_decay_s: d,
                noise_filter_mode: FILTER_HP,
                noise_cutoff_hz: 7000.0 + tone * 4000.0,
                noise_res: 0.1,
                ..base
            }
        }
        _ => base,
    }
}
```

- [ ] **Step 4: Run all the drum tests to confirm they pass**

Run: `cargo test -p acid-engine drums`
Expected: all pass, including `every_instrument_sounds_and_ends`, which now exercises the real 909 patches.

- [ ] **Step 5: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): 909-style SID drum kit"
```

---

### Task 10: Distortion and compressor

**Files:**
- Create: `crates/acid-engine/src/fx/mod.rs`, `crates/acid-engine/src/fx/distortion.rs`, `crates/acid-engine/src/fx/compressor.rs`
- Modify: `crates/acid-engine/src/lib.rs` (add `pub mod fx;`)

**Interfaces:**
- Consumes: `params::{exp, lin, db_to_gain}`.
- Produces:
  - `fx::Distortion` with a pub field `on: bool` and these methods:
    - `new(sample_rate)`
    - `set_drive(v)`, `set_tone(v)` and `set_mix(v)`, all normalised
    - `process(&mut self, l: &mut [f32], r: &mut [f32])`, which works in place and does nothing when off
  - `fx::Compressor` with a pub field `on: bool` and these methods:
    - `new(sample_rate)`
    - `set_threshold(v)`, `set_ratio(v)`, `set_attack(v)`, `set_release(v)` and `set_makeup(v)`, all normalised
    - `process(&mut self, l: &mut [f32], r: &mut [f32])`, which works in place on a stereo-linked signal and does nothing when off
    - `gain_reduction_db(&self) -> f32`, for meters
  - Both effects also have `reset(&mut self)`, which clears internal state. The mixer's NaN guard uses it.

- [ ] **Step 1: Write the failing tests**

`crates/acid-engine/src/fx/mod.rs`:
```rust
//! Effects. Every process() works on stereo slices in place (inserts) or
//! adds a wet signal into output slices (sends).

pub mod compressor;
pub mod distortion;

pub use compressor::Compressor;
pub use distortion::Distortion;
```

`crates/acid-engine/src/fx/distortion.rs`, tests first:
```rust
//! Waveshaping distortion: asymmetric tanh with 2x oversampling, a tone
//! low-pass and dry/wet mix.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};
    use std::f32::consts::TAU;

    const SR: f32 = 44_100.0;

    fn sine(amp: f32) -> Vec<f32> {
        (0..4410).map(|i| amp * (TAU * 110.0 * i as f32 / SR).sin()).collect()
    }

    #[test]
    fn off_is_passthrough() {
        let mut d = Distortion::new(SR);
        let mut l = sine(0.5);
        let mut r = l.clone();
        let orig = l.clone();
        d.process(&mut l, &mut r);
        assert_eq!(l, orig);
    }

    #[test]
    fn high_drive_saturates_and_stays_bounded() {
        let mut d = Distortion::new(SR);
        d.on = true;
        d.set_drive(1.0);
        d.set_tone(1.0);
        d.set_mix(1.0);
        let mut l = sine(1.0);
        let mut r = l.clone();
        d.process(&mut l, &mut r);
        assert!(all_finite(&l));
        assert!(peak(&l) < 1.3);
        // Saturated: most of the waveform sits near the rails.
        let near_rail = l.iter().filter(|v| v.abs() > 0.7).count();
        assert!(near_rail > l.len() / 2, "near_rail = {near_rail}");
    }

    #[test]
    fn zero_mix_is_dry() {
        let mut d = Distortion::new(SR);
        d.on = true;
        d.set_drive(1.0);
        d.set_mix(0.0);
        let mut l = sine(0.5);
        let mut r = l.clone();
        let orig = l.clone();
        d.process(&mut l, &mut r);
        for (a, b) in l.iter().zip(&orig) {
            assert!((a - b).abs() < 1e-6);
        }
    }
}
```

`crates/acid-engine/src/fx/compressor.rs`, tests first:
```rust
//! Feed-forward stereo-linked peak compressor with a 6 dB soft knee.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::peak;
    use std::f32::consts::TAU;

    const SR: f32 = 44_100.0;

    fn sine(amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (TAU * 220.0 * i as f32 / SR).sin()).collect()
    }

    fn comp(threshold: f32, ratio: f32) -> Compressor {
        let mut c = Compressor::new(SR);
        c.on = true;
        c.set_threshold(threshold);
        c.set_ratio(ratio);
        c.set_attack(0.2);
        c.set_release(0.5);
        c.set_makeup(0.0);
        c
    }

    #[test]
    fn below_threshold_is_unity() {
        let mut c = comp(0.75, 1.0); // -10 dB threshold
        let mut l = sine(0.05, 22_050);
        let mut r = l.clone();
        let orig = l.clone();
        c.process(&mut l, &mut r);
        assert!((peak(&l[11_025..]) - peak(&orig[11_025..])).abs() < 1e-3);
    }

    #[test]
    fn loud_signal_is_reduced_toward_threshold() {
        let mut c = comp(0.5, 1.0); // -20 dB threshold, ratio 20:1
        let mut l = sine(1.0, 44_100);
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        let p = peak(&l[33_075..]);
        assert!(p > 0.08 && p < 0.2, "peak = {p}");
        assert!(c.gain_reduction_db() < -15.0);
    }

    #[test]
    fn off_is_passthrough() {
        let mut c = Compressor::new(SR);
        let mut l = sine(1.0, 1000);
        let mut r = l.clone();
        let orig = l.clone();
        c.process(&mut l, &mut r);
        assert_eq!(l, orig);
    }
}
```

Add `pub mod fx;` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine fx`
Expected: compile errors.

- [ ] **Step 3: Implement both effects**

Insert above the tests in `distortion.rs`:
```rust
use crate::params::{exp, lin};
use std::f32::consts::TAU;

const BIAS: f32 = 0.15;

pub struct Distortion {
    pub on: bool,
    sample_rate: f32,
    drive: f32,
    mix: f32,
    tone_coef: f32,
    lp: [f32; 2],
    prev: [f32; 2],
}

fn shape(x: f32, drive: f32) -> f32 {
    (drive * x + BIAS).tanh() - BIAS.tanh()
}

impl Distortion {
    pub fn new(sample_rate: f32) -> Self {
        let mut d = Self { on: false, sample_rate, drive: 1.0, mix: 1.0, tone_coef: 1.0, lp: [0.0; 2], prev: [0.0; 2] };
        d.set_drive(0.3);
        d.set_tone(0.6);
        d
    }

    pub fn set_drive(&mut self, v: f32) {
        self.drive = exp(v, 1.0, 40.0);
    }

    pub fn set_tone(&mut self, v: f32) {
        let fc = exp(v, 800.0, 12_000.0);
        self.tone_coef = 1.0 - (-TAU * fc / self.sample_rate).exp();
    }

    pub fn set_mix(&mut self, v: f32) {
        self.mix = lin(v, 0.0, 1.0);
    }

    /// Clears filter state (used by the mixer's NaN guard).
    pub fn reset(&mut self) {
        self.lp = [0.0; 2];
        self.prev = [0.0; 2];
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if !self.on {
            return;
        }
        for (ch, buf) in [l, r].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let dry = *x;
                let mid = 0.5 * (self.prev[ch] + dry);
                self.prev[ch] = dry;
                let wet = 0.5 * (shape(mid, self.drive) + shape(dry, self.drive));
                self.lp[ch] += (wet - self.lp[ch]) * self.tone_coef;
                *x = dry + (self.lp[ch] - dry) * self.mix;
            }
        }
    }
}
```

Insert above the tests in `compressor.rs`:
```rust
use crate::params::{db_to_gain, exp, lin};

const KNEE_DB: f32 = 6.0;

pub struct Compressor {
    pub on: bool,
    sample_rate: f32,
    threshold_db: f32,
    ratio: f32,
    attack_coef: f32,
    release_coef: f32,
    makeup: f32,
    gr_db: f32,
}

impl Compressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut c = Self {
            on: false,
            sample_rate,
            threshold_db: -12.0,
            ratio: 4.0,
            attack_coef: 0.0,
            release_coef: 0.0,
            makeup: 1.0,
            gr_db: 0.0,
        };
        c.set_attack(0.2);
        c.set_release(0.4);
        c
    }

    /// -40 .. 0 dB.
    pub fn set_threshold(&mut self, v: f32) {
        self.threshold_db = lin(v, -40.0, 0.0);
    }

    /// 1:1 .. 20:1.
    pub fn set_ratio(&mut self, v: f32) {
        self.ratio = exp(v, 1.0, 20.0);
    }

    /// 0.1 .. 100 ms.
    pub fn set_attack(&mut self, v: f32) {
        self.attack_coef = (-1.0 / (exp(v, 0.0001, 0.1) * self.sample_rate)).exp();
    }

    /// 10 .. 1000 ms.
    pub fn set_release(&mut self, v: f32) {
        self.release_coef = (-1.0 / (exp(v, 0.01, 1.0) * self.sample_rate)).exp();
    }

    /// 0 .. +24 dB.
    pub fn set_makeup(&mut self, v: f32) {
        self.makeup = db_to_gain(lin(v, 0.0, 24.0));
    }

    pub fn gain_reduction_db(&self) -> f32 {
        self.gr_db
    }

    /// Clears detector state (used by the mixer's NaN guard).
    pub fn reset(&mut self) {
        self.gr_db = 0.0;
    }

    fn target_gr_db(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold_db;
        let slope = 1.0 / self.ratio - 1.0;
        if over <= -KNEE_DB / 2.0 {
            0.0
        } else if over < KNEE_DB / 2.0 {
            slope * (over + KNEE_DB / 2.0).powi(2) / (2.0 * KNEE_DB)
        } else {
            slope * over
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if !self.on {
            return;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let peak = a.abs().max(b.abs()).max(1.0e-6);
            let target = self.target_gr_db(20.0 * peak.log10());
            let coef = if target < self.gr_db { self.attack_coef } else { self.release_coef };
            self.gr_db = target + (self.gr_db - target) * coef;
            let g = db_to_gain(self.gr_db) * self.makeup;
            *a *= g;
            *b *= g;
        }
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p acid-engine fx`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): distortion and compressor effects"
```

---

### Task 11: Delay, reverb and lo-fi

**Files:**
- Create: `crates/acid-engine/src/fx/delay.rs`, `crates/acid-engine/src/fx/reverb.rs`, `crates/acid-engine/src/fx/lofi.rs`
- Modify: `crates/acid-engine/src/fx/mod.rs`

**Interfaces:**
- Consumes: `params::{exp, lin}`.
- Produces:
  - `fx::Delay` with a pub field `ping_pong: bool`, the constant `Delay::DIVISION_STEPS: [f32; 6] = [1.0, 2.0, 3.0, 4.0, 6.0, 8.0]` (in 16th notes), and these methods:
    - `new(sample_rate)`
    - `set_bpm(bpm)`, `set_sync(bool)`, `set_division(idx)` and `set_ms(ms)`
    - `set_feedback(v)` and `set_tone(v)`, both normalised
    - `delay_samples(&self) -> usize`
    - `process(&mut self, in_l, in_r, out_l, out_r)`, which adds the wet signal into the outputs
    - `reset(&mut self)`
  - `fx::Reverb` with these methods:
    - `new(sample_rate)`
    - `set_size(v)`, `set_damping(v)` and `set_width(v)`
    - `process(&mut self, in_l, in_r, out_l, out_r)`, which adds the wet signal into the outputs
    - `reset(&mut self)`
  - `fx::LoFi` with a pub field `on: bool` and these methods:
    - `new(sample_rate)`
    - `process(&mut self, l, r)`, which works in place

- [ ] **Step 1: Write the failing tests**

Update `fx/mod.rs`:
```rust
//! Effects. Every process() works on stereo slices in place (inserts) or
//! adds a wet signal into output slices (sends).

pub mod compressor;
pub mod delay;
pub mod distortion;
pub mod lofi;
pub mod reverb;

pub use compressor::Compressor;
pub use delay::Delay;
pub use distortion::Distortion;
pub use lofi::LoFi;
pub use reverb::Reverb;
```

`fx/delay.rs`, tests first:
```rust
//! Stereo send delay: tempo-synced or free time, feedback through a tone
//! low-pass, optional ping-pong. Buffers are allocated once (2 s).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 44_100.0;

    fn impulse_response(d: &mut Delay, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut il = vec![0.0; n];
        let mut ir = vec![0.0; n];
        il[0] = 1.0;
        ir[0] = 1.0;
        let mut ol = vec![0.0; n];
        let mut or = vec![0.0; n];
        d.process(&il, &ir, &mut ol, &mut or);
        (ol, or)
    }

    #[test]
    fn synced_time_matches_tempo() {
        let mut d = Delay::new(SR);
        d.set_bpm(120.0);
        d.set_sync(true);
        d.set_division(0); // one 16th = 0.125 s
        assert_eq!(d.delay_samples(), 5512);
        d.set_division(2); // 3/16
        assert_eq!(d.delay_samples(), 16_537);
    }

    #[test]
    fn free_time_in_ms() {
        let mut d = Delay::new(SR);
        d.set_sync(false);
        d.set_ms(100.0);
        assert_eq!(d.delay_samples(), 4410);
    }

    #[test]
    fn echo_arrives_after_delay_time() {
        let mut d = Delay::new(SR);
        d.ping_pong = false;
        d.set_sync(false);
        d.set_ms(100.0);
        d.set_feedback(0.0);
        d.set_tone(1.0);
        let (l, _) = impulse_response(&mut d, 9000);
        assert!(peak(&l[..4400]) < 1e-6);
        assert!(peak(&l[4410..4420]) > 0.3);
    }

    #[test]
    fn ping_pong_alternates_sides() {
        let mut d = Delay::new(SR);
        d.ping_pong = true;
        d.set_sync(false);
        d.set_ms(100.0);
        d.set_feedback(0.5);
        d.set_tone(1.0);
        let (l, r) = impulse_response(&mut d, 13_300);
        assert!(peak(&l[4410..4430]) > 0.3 && peak(&r[4410..4430]) < 1e-3);
        assert!(peak(&r[8820..8850]) > 0.1 && peak(&l[8820..8850]) < 1e-3);
    }

    #[test]
    fn max_feedback_is_stable() {
        let mut d = Delay::new(SR);
        d.set_sync(false);
        d.set_ms(50.0);
        d.set_feedback(1.0);
        let (l, r) = impulse_response(&mut d, 10 * SR as usize);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(peak(&l) < 2.0);
        assert!(peak(&l[9 * SR as usize..]) < peak(&l[..SR as usize]));
    }
}
```

`fx/reverb.rs`, tests first:
```rust
//! Freeverb-style stereo reverb (8 combs + 4 allpasses per side).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 44_100.0;

    fn impulse_response(r: &mut Reverb, n: usize) -> Vec<f32> {
        let mut il = vec![0.0; n];
        il[0] = 1.0;
        let ir = il.clone();
        let mut ol = vec![0.0; n];
        let mut or = vec![0.0; n];
        r.process(&il, &ir, &mut ol, &mut or);
        ol
    }

    #[test]
    fn has_a_tail_that_decays() {
        let mut r = Reverb::new(SR);
        r.set_size(0.5);
        r.set_damping(0.5);
        let out = impulse_response(&mut r, 10 * SR as usize);
        assert!(peak(&out[2205..4410]) > 1e-3, "tail present at 50-100 ms");
        assert!(peak(&out[9 * SR as usize..]) < 1e-3, "tail gone by 9 s");
    }

    #[test]
    fn max_size_is_stable() {
        let mut r = Reverb::new(SR);
        r.set_size(1.0);
        r.set_damping(0.0);
        let out = impulse_response(&mut r, 5 * SR as usize);
        assert!(all_finite(&out));
        assert!(peak(&out) < 1.0);
    }

    #[test]
    fn works_at_48k() {
        let mut r = Reverb::new(48_000.0);
        let out = impulse_response(&mut r, 48_000);
        assert!(all_finite(&out) && peak(&out) > 1e-4);
    }
}
```

`fx/lofi.rs`, tests first:
```rust
//! Lo-fi: sample-and-hold down to 22 050 Hz and 8-bit quantisation,
//! recalling the original Acid OS output.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantises_to_8_bits_and_holds() {
        let mut f = LoFi::new(44_100.0);
        f.on = true;
        let mut l: Vec<f32> = (0..100).map(|i| (i as f32 * 0.37).sin() * 0.9).collect();
        let mut r = l.clone();
        f.process(&mut l, &mut r);
        for v in &l {
            let q = v * 127.0;
            assert!((q - q.round()).abs() < 1e-4, "{v} not on an 8-bit step");
        }
        let repeats = l.windows(2).filter(|w| w[0] == w[1]).count();
        assert!(repeats >= 45, "held at half rate, repeats = {repeats}");
    }

    #[test]
    fn off_is_passthrough() {
        let mut f = LoFi::new(44_100.0);
        let mut l = vec![0.123, 0.456];
        let mut r = l.clone();
        f.process(&mut l, &mut r);
        assert_eq!(l, vec![0.123, 0.456]);
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine fx`
Expected: compile errors.

- [ ] **Step 3: Implement the delay**

Insert above the tests in `delay.rs`:
```rust
use crate::params::{exp, lin};
use std::f32::consts::TAU;

const MAX_SECONDS: f32 = 2.0;

pub struct Delay {
    pub ping_pong: bool,
    sample_rate: f32,
    buf: [Vec<f32>; 2],
    write: usize,
    delay: usize,
    feedback: f32,
    tone_coef: f32,
    lp: [f32; 2],
    sync: bool,
    division: usize,
    ms: f32,
    bpm: f32,
}

impl Delay {
    /// Delay lengths in 16th notes: 1/16, 1/8, 3/16, 1/4, 3/8, 1/2.
    pub const DIVISION_STEPS: [f32; 6] = [1.0, 2.0, 3.0, 4.0, 6.0, 8.0];

    pub fn new(sample_rate: f32) -> Self {
        let len = (sample_rate * MAX_SECONDS) as usize + 1;
        let mut d = Self {
            ping_pong: true,
            sample_rate,
            buf: [vec![0.0; len], vec![0.0; len]],
            write: 0,
            delay: 1,
            feedback: 0.4,
            tone_coef: 1.0,
            lp: [0.0; 2],
            sync: true,
            division: 2,
            ms: 300.0,
            bpm: 125.0,
        };
        d.set_tone(0.6);
        d.recompute();
        d
    }

    fn recompute(&mut self) {
        let seconds = if self.sync {
            Self::DIVISION_STEPS[self.division] * 60.0 / self.bpm / 4.0
        } else {
            self.ms / 1000.0
        };
        let len = self.buf[0].len();
        self.delay = ((seconds * self.sample_rate) as usize).clamp(1, len - 1);
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.bpm = bpm.max(1.0);
        self.recompute();
    }

    pub fn set_sync(&mut self, sync: bool) {
        self.sync = sync;
        self.recompute();
    }

    pub fn set_division(&mut self, idx: usize) {
        self.division = idx.min(Self::DIVISION_STEPS.len() - 1);
        self.recompute();
    }

    pub fn set_ms(&mut self, ms: f32) {
        self.ms = ms;
        self.recompute();
    }

    pub fn set_feedback(&mut self, v: f32) {
        self.feedback = lin(v, 0.0, 0.95);
    }

    pub fn set_tone(&mut self, v: f32) {
        let fc = exp(v, 500.0, 16_000.0);
        self.tone_coef = 1.0 - (-TAU * fc / self.sample_rate).exp();
    }

    pub fn delay_samples(&self) -> usize {
        self.delay
    }

    pub fn reset(&mut self) {
        for b in &mut self.buf {
            b.fill(0.0);
        }
        self.lp = [0.0; 2];
    }

    pub fn process(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let len = self.buf[0].len();
        for i in 0..in_l.len() {
            let read = (self.write + len - self.delay) % len;
            let dl = self.buf[0][read];
            let dr = self.buf[1][read];
            self.lp[0] += (dl - self.lp[0]) * self.tone_coef;
            self.lp[1] += (dr - self.lp[1]) * self.tone_coef;
            let fb_l = self.lp[0] * self.feedback;
            let fb_r = self.lp[1] * self.feedback;
            if self.ping_pong {
                self.buf[0][self.write] = 0.5 * (in_l[i] + in_r[i]) + fb_r;
                self.buf[1][self.write] = fb_l;
            } else {
                self.buf[0][self.write] = in_l[i] + fb_l;
                self.buf[1][self.write] = in_r[i] + fb_r;
            }
            out_l[i] += dl;
            out_r[i] += dr;
            self.write = (self.write + 1) % len;
        }
    }
}
```

The ping-pong test expects an echo above 0.3 on the left at 4410. The ping-pong input is `0.5 * (1 + 1) = 1.0`, and with tone at 1.0 the low-pass lets it straight through, so the expectation holds.

- [ ] **Step 4: Implement the reverb**

Insert above the tests in `reverb.rs`:
```rust
use crate::params::lin;

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const INPUT_GAIN: f32 = 0.015;

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0, store: 0.0 }
    }

    fn process(&mut self, input: f32, feedback: f32, damp1: f32, damp2: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * damp2 + self.store * damp1;
        if self.store.abs() < 1.0e-20 {
            self.store = 0.0;
        }
        self.buf[self.idx] = input + self.store * feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0 }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buf[self.idx];
        let out = buffered - input;
        self.buf[self.idx] = input + buffered * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    feedback: f32,
    damp1: f32,
    damp2: f32,
    wet1: f32,
    wet2: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let sized = |n: usize, spread: usize| ((n + spread) as f32 * scale) as usize;
        let side = |spread: usize| {
            (
                COMB_TUNING.iter().map(|&n| Comb::new(sized(n, spread))).collect::<Vec<_>>(),
                ALLPASS_TUNING.iter().map(|&n| Allpass::new(sized(n, spread))).collect::<Vec<_>>(),
            )
        };
        let (cl, al) = side(0);
        let (cr, ar) = side(STEREO_SPREAD);
        let mut r = Self { combs: [cl, cr], allpasses: [al, ar], feedback: 0.84, damp1: 0.2, damp2: 0.8, wet1: 1.0, wet2: 0.0 };
        r.set_size(0.6);
        r.set_damping(0.5);
        r.set_width(1.0);
        r
    }

    pub fn set_size(&mut self, v: f32) {
        self.feedback = lin(v, 0.7, 0.98);
    }

    pub fn set_damping(&mut self, v: f32) {
        self.damp1 = lin(v, 0.0, 0.4);
        self.damp2 = 1.0 - self.damp1;
    }

    pub fn set_width(&mut self, v: f32) {
        let w = v.clamp(0.0, 1.0);
        self.wet1 = w / 2.0 + 0.5;
        self.wet2 = (1.0 - w) / 2.0;
    }

    pub fn reset(&mut self) {
        for side in &mut self.combs {
            for c in side {
                c.buf.fill(0.0);
                c.store = 0.0;
            }
        }
        for side in &mut self.allpasses {
            for a in side {
                a.buf.fill(0.0);
            }
        }
    }

    pub fn process(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        for i in 0..in_l.len() {
            let input = (in_l[i] + in_r[i]) * INPUT_GAIN;
            let mut side_out = [0.0f32; 2];
            for s in 0..2 {
                let mut acc = 0.0;
                for c in &mut self.combs[s] {
                    acc += c.process(input, self.feedback, self.damp1, self.damp2);
                }
                for a in &mut self.allpasses[s] {
                    acc = a.process(acc);
                }
                side_out[s] = acc;
            }
            out_l[i] += side_out[0] * self.wet1 + side_out[1] * self.wet2;
            out_r[i] += side_out[1] * self.wet1 + side_out[0] * self.wet2;
        }
    }
}
```

- [ ] **Step 5: Implement lo-fi**

Insert above the tests in `lofi.rs`:
```rust
const TARGET_RATE: f32 = 22_050.0;

pub struct LoFi {
    pub on: bool,
    step: f32,
    phase: f32,
    held: [f32; 2],
}

fn quantise(x: f32) -> f32 {
    (x.clamp(-1.0, 1.0) * 127.0).round() / 127.0
}

impl LoFi {
    pub fn new(sample_rate: f32) -> Self {
        Self { on: false, step: (TARGET_RATE / sample_rate).min(1.0), phase: 1.0, held: [0.0; 2] }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if !self.on {
            return;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            self.phase += self.step;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.held = [quantise(*a), quantise(*b)];
            }
            *a = self.held[0];
            *b = self.held[1];
        }
    }
}
```

- [ ] **Step 6: Run the tests to confirm they pass**

Run: `cargo test -p acid-engine fx`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): tempo-synced delay, freeverb reverb and lo-fi effects"
```

---

### Task 12: Mixer

**Files:**
- Create: `crates/acid-engine/src/mixer.rs`
- Modify: `crates/acid-engine/src/lib.rs` (add `pub mod mixer;`)

**Interfaces:**
- Consumes:
  - `fx::{Distortion, Compressor, Delay, Reverb, LoFi}`.
  - `params::{ParamId, ChannelParam, FxParam, MasterParam, CHANNELS, on, step_index, exp}`.
- Produces:
  - `mixer::StereoBuf { pub l: Vec<f32>, pub r: Vec<f32> }` with `new(len)`.
  - `mixer::Mixer` with these methods:
    - `new(sample_rate, max_block: usize)`
    - `set_param(&mut self, id: ParamId, v: f32)`. It handles `Channel`, `Fx` and `Master` params and ignores the rest.
    - `set_bpm(&mut self, bpm: f32)`
    - `process(&mut self, chans: &mut [StereoBuf; 4], n: usize, out_l: &mut [f32], out_r: &mut [f32])`. It overwrites the output and processes the channel buffers in place.
  - When `Mixer::new` returns, every Channel, Fx and Master param is already at its registry default.

- [ ] **Step 1: Write the failing tests**

`crates/acid-engine/src/mixer.rs`, tests first:
```rust
//! Four channel strips (distortion -> compressor -> level/pan/mute/solo,
//! post-fader delay and reverb sends), send returns, master compressor,
//! master level, optional lo-fi, final clip and NaN guard.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak, rms};

    const SR: f32 = 44_100.0;
    const N: usize = 512;

    fn chans_with(ch: usize, value: f32) -> [StereoBuf; 4] {
        let mut c: [StereoBuf; 4] = std::array::from_fn(|_| StereoBuf::new(N));
        c[ch].l.fill(value);
        c[ch].r.fill(value);
        c
    }

    fn run(m: &mut Mixer, chans: &mut [StereoBuf; 4]) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; N];
        let mut r = vec![0.0; N];
        m.process(chans, N, &mut l, &mut r);
        (l, r)
    }

    fn quiet_mixer() -> Mixer {
        let mut m = Mixer::new(SR, N);
        m.set_param(ParamId::Master(MasterParam::CompOn), 0.0);
        m
    }

    #[test]
    fn channel_reaches_output() {
        let mut m = quiet_mixer();
        let (l, r) = run(&mut m, &mut chans_with(0, 0.25));
        assert!(rms(&l) > 0.1 && rms(&r) > 0.1);
    }

    #[test]
    fn mute_silences_channel() {
        let mut m = quiet_mixer();
        m.set_param(ParamId::Channel(1, ChannelParam::Mute), 1.0);
        let (l, _) = run(&mut m, &mut chans_with(1, 0.25));
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn solo_isolates_channel() {
        let mut m = quiet_mixer();
        m.set_param(ParamId::Channel(2, ChannelParam::Solo), 1.0);
        let (l, _) = run(&mut m, &mut chans_with(0, 0.25));
        assert_eq!(peak(&l), 0.0);
        let (l, _) = run(&mut m, &mut chans_with(2, 0.25));
        assert!(peak(&l) > 0.1);
    }

    #[test]
    fn hard_left_pan() {
        let mut m = quiet_mixer();
        m.set_param(ParamId::Channel(0, ChannelParam::Pan), 0.0);
        let (l, r) = run(&mut m, &mut chans_with(0, 0.25));
        assert!(peak(&l) > 0.1);
        assert!(peak(&r) < 1e-6);
    }

    #[test]
    fn delay_send_produces_echo() {
        let mut m = quiet_mixer();
        m.set_param(ParamId::Channel(0, ChannelParam::DelaySend), 1.0);
        m.set_param(ParamId::Fx(FxParam::DelaySync), 0.0);
        m.set_param(ParamId::Fx(FxParam::DelayMs), 0.0); // 10 ms = 441 samples
        let mut chans: [StereoBuf; 4] = std::array::from_fn(|_| StereoBuf::new(N));
        chans[0].l[0] = 0.5;
        chans[0].r[0] = 0.5;
        let (l, _) = run(&mut m, &mut chans);
        assert!(peak(&l[430..460]) > 0.05);
    }

    #[test]
    fn output_is_clipped_and_finite() {
        let mut m = quiet_mixer();
        for c in 0..4 {
            m.set_param(ParamId::Channel(c, ChannelParam::Level), 1.0);
        }
        let mut chans: [StereoBuf; 4] = std::array::from_fn(|_| StereoBuf::new(N));
        for c in &mut chans {
            c.l.fill(5.0);
            c.r.fill(5.0);
        }
        let (l, r) = run(&mut m, &mut chans);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn nan_input_is_contained() {
        let mut m = quiet_mixer();
        m.set_param(ParamId::Channel(0, ChannelParam::ReverbSend), 1.0);
        let mut chans = chans_with(0, f32::NAN);
        let (l, _) = run(&mut m, &mut chans);
        assert!(all_finite(&l));
        let (l, _) = run(&mut m, &mut chans_with(0, 0.0));
        assert!(all_finite(&l), "reverb state was reset");
    }
}
```

Add `pub mod mixer;` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine mixer`
Expected: compile errors.

- [ ] **Step 3: Implement the mixer**

Insert above the tests in `mixer.rs`:
```rust
use crate::fx::{Compressor, Delay, Distortion, LoFi, Reverb};
use crate::params::{exp, on, step_index, ChannelParam, FxParam, MasterParam, ParamId, CHANNELS};
use std::f32::consts::{FRAC_PI_4, SQRT_2};

pub struct StereoBuf {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl StereoBuf {
    pub fn new(len: usize) -> Self {
        Self { l: vec![0.0; len], r: vec![0.0; len] }
    }
}

struct ChannelStrip {
    dist: Distortion,
    comp: Compressor,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    mute: bool,
    solo: bool,
    delay_send: f32,
    reverb_send: f32,
}

impl ChannelStrip {
    fn new(sample_rate: f32) -> Self {
        Self {
            dist: Distortion::new(sample_rate),
            comp: Compressor::new(sample_rate),
            gain: 1.0,
            pan_l: 1.0,
            pan_r: 1.0,
            mute: false,
            solo: false,
            delay_send: 0.0,
            reverb_send: 0.0,
        }
    }

    fn set(&mut self, p: ChannelParam, v: f32) {
        match p {
            ChannelParam::Level => self.gain = v * v * 2.0,
            ChannelParam::Pan => {
                let angle = v.clamp(0.0, 1.0) * 2.0 * FRAC_PI_4;
                self.pan_l = angle.cos() * SQRT_2;
                self.pan_r = angle.sin() * SQRT_2;
            }
            ChannelParam::Mute => self.mute = on(v),
            ChannelParam::Solo => self.solo = on(v),
            ChannelParam::DistOn => self.dist.on = on(v),
            ChannelParam::DistDrive => self.dist.set_drive(v),
            ChannelParam::DistTone => self.dist.set_tone(v),
            ChannelParam::DistMix => self.dist.set_mix(v),
            ChannelParam::CompOn => self.comp.on = on(v),
            ChannelParam::CompThreshold => self.comp.set_threshold(v),
            ChannelParam::CompRatio => self.comp.set_ratio(v),
            ChannelParam::CompAttack => self.comp.set_attack(v),
            ChannelParam::CompRelease => self.comp.set_release(v),
            ChannelParam::CompMakeup => self.comp.set_makeup(v),
            ChannelParam::DelaySend => self.delay_send = v,
            ChannelParam::ReverbSend => self.reverb_send = v,
        }
    }
}

pub struct Mixer {
    strips: Vec<ChannelStrip>,
    delay: Delay,
    reverb: Reverb,
    master_comp: Compressor,
    lofi: LoFi,
    master_gain: f32,
    delay_return: f32,
    reverb_return: f32,
    delay_send: StereoBuf,
    reverb_send: StereoBuf,
    wet: StereoBuf,
}

impl Mixer {
    pub fn new(sample_rate: f32, max_block: usize) -> Self {
        let mut m = Self {
            strips: (0..CHANNELS).map(|_| ChannelStrip::new(sample_rate)).collect(),
            delay: Delay::new(sample_rate),
            reverb: Reverb::new(sample_rate),
            master_comp: Compressor::new(sample_rate),
            lofi: LoFi::new(sample_rate),
            master_gain: 1.0,
            delay_return: 0.7,
            reverb_return: 0.6,
            delay_send: StereoBuf::new(max_block),
            reverb_send: StereoBuf::new(max_block),
            wet: StereoBuf::new(max_block),
        };
        for id in ParamId::all() {
            m.set_param(id, id.info().default);
        }
        m
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.delay.set_bpm(bpm);
    }

    pub fn set_param(&mut self, id: ParamId, v: f32) {
        match id {
            ParamId::Channel(c, p) => self.strips[c as usize].set(p, v),
            ParamId::Fx(p) => match p {
                FxParam::DelaySync => self.delay.set_sync(on(v)),
                FxParam::DelayDivision => self.delay.set_division(step_index(v, 6)),
                FxParam::DelayMs => self.delay.set_ms(exp(v, 10.0, 2000.0)),
                FxParam::DelayFeedback => self.delay.set_feedback(v),
                FxParam::DelayTone => self.delay.set_tone(v),
                FxParam::DelayPingPong => self.delay.ping_pong = on(v),
                FxParam::DelayReturn => self.delay_return = v,
                FxParam::ReverbSize => self.reverb.set_size(v),
                FxParam::ReverbDamping => self.reverb.set_damping(v),
                FxParam::ReverbWidth => self.reverb.set_width(v),
                FxParam::ReverbReturn => self.reverb_return = v,
            },
            ParamId::Master(p) => match p {
                MasterParam::CompOn => self.master_comp.on = on(v),
                MasterParam::CompThreshold => self.master_comp.set_threshold(v),
                MasterParam::CompRatio => self.master_comp.set_ratio(v),
                MasterParam::CompAttack => self.master_comp.set_attack(v),
                MasterParam::CompRelease => self.master_comp.set_release(v),
                MasterParam::CompMakeup => self.master_comp.set_makeup(v),
                MasterParam::Level => self.master_gain = v * v * 1.5,
                MasterParam::LoFi => self.lofi.on = on(v),
            },
            _ => {}
        }
    }

    pub fn process(&mut self, chans: &mut [StereoBuf; 4], n: usize, out_l: &mut [f32], out_r: &mut [f32]) {
        let (out_l, out_r) = (&mut out_l[..n], &mut out_r[..n]);
        out_l.fill(0.0);
        out_r.fill(0.0);
        for b in [&mut self.delay_send, &mut self.reverb_send] {
            b.l[..n].fill(0.0);
            b.r[..n].fill(0.0);
        }
        let any_solo = self.strips.iter().any(|s| s.solo);
        for (strip, buf) in self.strips.iter_mut().zip(chans.iter_mut()) {
            let (l, r) = (&mut buf.l[..n], &mut buf.r[..n]);
            strip.dist.process(l, r);
            strip.comp.process(l, r);
            if strip.mute || (any_solo && !strip.solo) {
                continue;
            }
            let (gl, gr) = (strip.gain * strip.pan_l, strip.gain * strip.pan_r);
            for i in 0..n {
                let sl = l[i] * gl;
                let sr = r[i] * gr;
                out_l[i] += sl;
                out_r[i] += sr;
                self.delay_send.l[i] += sl * strip.delay_send;
                self.delay_send.r[i] += sr * strip.delay_send;
                self.reverb_send.l[i] += sl * strip.reverb_send;
                self.reverb_send.r[i] += sr * strip.reverb_send;
            }
        }
        self.wet.l[..n].fill(0.0);
        self.wet.r[..n].fill(0.0);
        self.delay.process(&self.delay_send.l[..n], &self.delay_send.r[..n], &mut self.wet.l[..n], &mut self.wet.r[..n]);
        for i in 0..n {
            out_l[i] += self.wet.l[i] * self.delay_return;
            out_r[i] += self.wet.r[i] * self.delay_return;
        }
        self.wet.l[..n].fill(0.0);
        self.wet.r[..n].fill(0.0);
        self.reverb.process(&self.reverb_send.l[..n], &self.reverb_send.r[..n], &mut self.wet.l[..n], &mut self.wet.r[..n]);
        for i in 0..n {
            out_l[i] += self.wet.l[i] * self.reverb_return;
            out_r[i] += self.wet.r[i] * self.reverb_return;
        }
        self.master_comp.process(out_l, out_r);
        for i in 0..n {
            out_l[i] *= self.master_gain;
            out_r[i] *= self.master_gain;
        }
        self.lofi.process(out_l, out_r);
        if out_l.iter().chain(out_r.iter()).any(|v| !v.is_finite()) {
            out_l.fill(0.0);
            out_r.fill(0.0);
            self.delay.reset();
            self.reverb.reset();
            self.master_comp.reset();
            for s in &mut self.strips {
                s.comp.reset();
                s.dist.reset();
            }
            return;
        }
        for v in out_l.iter_mut().chain(out_r.iter_mut()) {
            *v = v.clamp(-1.0, 1.0);
        }
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p acid-engine`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): 4-channel mixer with inserts, sends, master bus and NaN guard"
```

---

### Task 13: Sequencer (clock, swing, pattern playback)

**Files:**
- Create: `crates/acid-engine/src/sequencer.rs`
- Modify: `crates/acid-engine/src/lib.rs` (add `pub mod sequencer;`)

**Interfaces:**
- Consumes: `project::{SynthPattern, DrumPattern, Gate, DrumHit}`, `params::ParamId`.
- Produces:
  - `sequencer::SeqEvent`, which derives `Clone, Copy, Debug, PartialEq`. Variants:
    - `Note { synth: usize, midi: u8, accent: bool, legato: bool }`
    - `NoteOff { synth: usize }`
    - `Drum { kit: usize, inst: usize, accent: bool }`
    - `Param { id: ParamId, value: f32 }`
  - `sequencer::Sequencer` with these methods:
    - `new(sample_rate)`
    - `set_bpm(f32)`, clamped to 40–300, and `bpm(&self) -> f32`
    - `set_swing(f32)`, clamped to 0–0.75
    - `play(&mut self)` and `stop(&mut self)`
    - `is_playing(&self) -> bool`
    - `select(&mut self, machine: usize, slot: u8)`, where machines 0 and 1 are the synths and 2 and 3 are the drum kits
    - `current_pattern(&self, machine) -> u8`
    - `queued_pattern(&self, machine) -> Option<u8>`
    - `playhead(&self, machine) -> usize`
    - `samples_to_next_tick(&self) -> usize`, which returns `usize::MAX` when stopped
    - `advance(&mut self, n: usize)`
    - `tick(&mut self, synths: [&[SynthPattern]; 2], drums: [&[DrumPattern]; 2], out: &mut Vec<SeqEvent>)`
- Timing contract:
  - A step is a 16th note. Each step has two ticks: the step start, and the gate-off check at 50 % of the step.
  - Swing lengthens even steps to `base × (1 + swing/2)` and shortens odd steps to `base × (1 − swing/2)`.
  - A tick fires on the first whole sample at or after its exact time.

- [ ] **Step 1: Write the failing tests**

`crates/acid-engine/src/sequencer.rs`, tests first:
```rust
//! Pattern-mode sequencer. Sample-accurate: the engine asks how many
//! samples until the next tick, renders that many, then calls tick().
//! Each 16th step has a start tick (notes, drums, param lane) and a mid
//! tick (gate off unless sliding/tied into the next step).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{DrumHit, DrumPattern, Gate, SynthPattern, SynthStep};

    const SR: f32 = 44_100.0;

    struct Fixture {
        synths: [Vec<SynthPattern>; 2],
        drums: [Vec<DrumPattern>; 2],
    }

    impl Fixture {
        fn new() -> Self {
            Self { synths: [vec![SynthPattern::default(); 4], vec![SynthPattern::default(); 4]], drums: [vec![DrumPattern::default(); 4], vec![DrumPattern::default(); 4]] }
        }

        fn run(&self, seq: &mut Sequencer, total: usize) -> Vec<(usize, SeqEvent)> {
            let mut pos = 0;
            let mut buf = Vec::with_capacity(64);
            let mut out = Vec::new();
            while pos < total {
                let n = seq.samples_to_next_tick().min(total - pos);
                if n == 0 {
                    buf.clear();
                    seq.tick([&self.synths[0], &self.synths[1]], [&self.drums[0], &self.drums[1]], &mut buf);
                    out.extend(buf.iter().map(|e| (pos, *e)));
                    continue;
                }
                seq.advance(n);
                pos += n;
            }
            out
        }
    }

    fn on(note: u8) -> SynthStep {
        SynthStep { note, octave: 0, gate: Gate::On, accent: false, slide: false }
    }

    fn new_seq() -> Sequencer {
        let mut s = Sequencer::new(SR);
        s.set_bpm(120.0); // 16th = 5512.5 samples
        s
    }

    #[test]
    fn stopped_sequencer_never_ticks() {
        let s = new_seq();
        assert_eq!(s.samples_to_next_tick(), usize::MAX);
    }

    #[test]
    fn four_on_the_floor_is_sample_accurate() {
        let mut f = Fixture::new();
        for st in [0, 4, 8, 12] {
            f.drums[0][0].steps[st][0] = DrumHit::On;
        }
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 88_200);
        let kicks: Vec<usize> = ev.iter().filter(|(_, e)| matches!(e, SeqEvent::Drum { kit: 0, inst: 0, .. })).map(|(p, _)| *p).collect();
        assert_eq!(kicks, vec![0, 22_050, 44_100, 66_150]);
    }

    #[test]
    fn odd_steps_land_on_the_next_whole_sample() {
        let mut f = Fixture::new();
        f.drums[1][0].steps[1][6] = DrumHit::Accent;
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 11_000);
        assert_eq!(ev, vec![(5513, SeqEvent::Drum { kit: 1, inst: 6, accent: true })]);
    }

    #[test]
    fn swing_delays_odd_steps() {
        let mut f = Fixture::new();
        f.drums[0][0].steps[1][0] = DrumHit::On;
        f.drums[0][0].steps[2][0] = DrumHit::On;
        let mut s = new_seq();
        s.set_swing(0.5);
        s.play();
        let ev = f.run(&mut s, 12_000);
        let pos: Vec<usize> = ev.iter().map(|(p, _)| *p).collect();
        assert_eq!(pos, vec![6891, 11_025]); // 5512.5 * 1.25 = 6890.6
    }

    #[test]
    fn plain_note_releases_at_half_step() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(0);
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 5000);
        assert_eq!(ev, vec![
            (0, SeqEvent::Note { synth: 0, midi: 36, accent: false, legato: false }),
            (2757, SeqEvent::NoteOff { synth: 0 }),
        ]);
    }

    #[test]
    fn slide_holds_gate_and_marks_next_note_legato() {
        let mut f = Fixture::new();
        f.synths[1][0].steps[0] = SynthStep { slide: true, ..on(0) };
        f.synths[1][0].steps[1] = on(7);
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 11_000);
        assert_eq!(ev, vec![
            (0, SeqEvent::Note { synth: 1, midi: 36, accent: false, legato: false }),
            (5513, SeqEvent::Note { synth: 1, midi: 43, accent: false, legato: true }),
            (8269, SeqEvent::NoteOff { synth: 1 }),
        ]);
    }

    #[test]
    fn tie_extends_without_retrigger() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = on(5);
        f.synths[0][0].steps[1] = SynthStep { gate: Gate::Tie, ..on(9) };
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 11_000);
        assert_eq!(ev, vec![
            (0, SeqEvent::Note { synth: 0, midi: 41, accent: false, legato: false }),
            (8269, SeqEvent::NoteOff { synth: 0 }),
        ]);
    }

    #[test]
    fn slide_into_rest_releases_at_half_step() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = SynthStep { slide: true, ..on(0) };
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 5000);
        assert_eq!(ev[1], (2757, SeqEvent::NoteOff { synth: 0 }));
    }

    #[test]
    fn accent_and_transpose_pass_through() {
        let mut f = Fixture::new();
        f.synths[0][0].steps[0] = SynthStep { accent: true, octave: 1, ..on(2) };
        f.synths[0][0].transpose = -3;
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 10);
        assert_eq!(ev[0].1, SeqEvent::Note { synth: 0, midi: 36 + 12 + 2 - 3, accent: true, legato: false });
    }

    #[test]
    fn pattern_length_wraps_and_polymeter() {
        let mut f = Fixture::new();
        f.synths[0][0].length = 3;
        f.synths[0][0].steps[0] = on(0);
        f.drums[0][0].length = 4;
        f.drums[0][0].steps[0][0] = DrumHit::On;
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 12 * 5512); // steps 0..=11 (step 12 starts at 66 150)
        let notes = ev.iter().filter(|(_, e)| matches!(e, SeqEvent::Note { .. })).count();
        let kicks = ev.iter().filter(|(_, e)| matches!(e, SeqEvent::Drum { .. })).count();
        assert_eq!(notes, 4); // steps 0, 3, 6, 9
        assert_eq!(kicks, 3); // steps 0, 4, 8
    }

    #[test]
    fn selecting_while_playing_queues_until_wrap() {
        let mut f = Fixture::new();
        f.drums[0][1].steps[0][1] = DrumHit::On; // pattern 1 has a snare on step 0
        let mut s = new_seq();
        s.play();
        f.run(&mut s, 3 * 5513);
        s.select(2, 1);
        assert_eq!(s.current_pattern(2), 0);
        assert_eq!(s.queued_pattern(2), Some(1));
        let ev = f.run(&mut s, 14 * 5513);
        let snare = ev.iter().find(|(_, e)| matches!(e, SeqEvent::Drum { inst: 1, .. }));
        assert!(snare.is_some());
        assert_eq!(s.current_pattern(2), 1);
    }

    #[test]
    fn selecting_while_stopped_is_immediate() {
        let mut s = new_seq();
        s.select(0, 9);
        assert_eq!(s.current_pattern(0), 9);
        assert_eq!(s.queued_pattern(0), None);
    }

    #[test]
    fn param_lane_emits_param_events() {
        use crate::params::{ParamId, SynthParam};
        use crate::project::ParamLane;
        let mut f = Fixture::new();
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        let mut values = vec![None; 64];
        values[0] = Some(0.8);
        f.synths[0][0].param_lane = Some(ParamLane { param: id, values });
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 10);
        assert_eq!(ev, vec![(0, SeqEvent::Param { id, value: 0.8 })]);
    }

    #[test]
    fn stop_resets_playheads() {
        let f = Fixture::new();
        let mut s = new_seq();
        s.play();
        f.run(&mut s, 5 * 5513);
        assert_eq!(s.playhead(0), 5);
        s.stop();
        assert!(!s.is_playing());
        assert_eq!(s.playhead(0), 0);
    }
}
```

Add `pub mod sequencer;` to `lib.rs`.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine sequencer`
Expected: compile errors.

- [ ] **Step 3: Implement the sequencer**

Insert above the tests in `sequencer.rs`:
```rust
use crate::params::ParamId;
use crate::project::{DrumHit, DrumPattern, Gate, SynthPattern};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SeqEvent {
    Note { synth: usize, midi: u8, accent: bool, legato: bool },
    NoteOff { synth: usize },
    Drum { kit: usize, inst: usize, accent: bool },
    Param { id: ParamId, value: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    StepStart,
    Mid,
}

#[derive(Clone, Copy, Debug, Default)]
struct Track {
    step: usize,
    pattern: u8,
    queued: Option<u8>,
    gate_open: bool,
    prev_slide: bool,
}

pub struct Sequencer {
    sample_rate: f32,
    bpm: f32,
    swing: f32,
    playing: bool,
    global_step: u64,
    phase: Phase,
    samples_to_next: f64,
    /// 0, 1 = synths; 2, 3 = drum kits.
    tracks: [Track; 4],
}

fn len_of(length: u8) -> usize {
    (length as usize).clamp(1, crate::project::STEPS)
}

impl Sequencer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            bpm: 125.0,
            swing: 0.0,
            playing: false,
            global_step: 0,
            phase: Phase::StepStart,
            samples_to_next: 0.0,
            tracks: [Track::default(); 4],
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.bpm = bpm.clamp(40.0, 300.0);
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    pub fn set_swing(&mut self, swing: f32) {
        self.swing = swing.clamp(0.0, 0.75);
    }

    fn step_len(&self, step: u64) -> f64 {
        let base = 60.0 / self.bpm as f64 / 4.0 * self.sample_rate as f64;
        let s = self.swing as f64 * 0.5;
        if step % 2 == 0 { base * (1.0 + s) } else { base * (1.0 - s) }
    }

    pub fn play(&mut self) {
        self.playing = true;
        self.global_step = 0;
        self.phase = Phase::StepStart;
        self.samples_to_next = 0.0;
        for t in &mut self.tracks {
            t.step = 0;
            t.gate_open = false;
            t.prev_slide = false;
            if let Some(q) = t.queued.take() {
                t.pattern = q;
            }
        }
    }

    pub fn stop(&mut self) {
        self.playing = false;
        for t in &mut self.tracks {
            t.step = 0;
            t.gate_open = false;
            t.prev_slide = false;
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn select(&mut self, machine: usize, slot: u8) {
        let t = &mut self.tracks[machine];
        if self.playing {
            t.queued = Some(slot);
        } else {
            t.pattern = slot;
            t.queued = None;
        }
    }

    pub fn current_pattern(&self, machine: usize) -> u8 {
        self.tracks[machine].pattern
    }

    pub fn queued_pattern(&self, machine: usize) -> Option<u8> {
        self.tracks[machine].queued
    }

    pub fn playhead(&self, machine: usize) -> usize {
        self.tracks[machine].step
    }

    pub fn samples_to_next_tick(&self) -> usize {
        if !self.playing {
            return usize::MAX;
        }
        self.samples_to_next.max(0.0).ceil() as usize
    }

    pub fn advance(&mut self, n: usize) {
        if self.playing {
            self.samples_to_next -= n as f64;
        }
    }

    pub fn tick(&mut self, synths: [&[SynthPattern]; 2], drums: [&[DrumPattern]; 2], out: &mut Vec<SeqEvent>) {
        if !self.playing {
            return;
        }
        let half = self.step_len(self.global_step) * 0.5;
        match self.phase {
            Phase::StepStart => {
                for s in 0..2 {
                    let t = &mut self.tracks[s];
                    let pat = &synths[s][t.pattern as usize];
                    if t.step >= len_of(pat.length) {
                        t.step = 0;
                    }
                    let step = pat.steps[t.step];
                    match step.gate {
                        Gate::On => {
                            let legato = t.prev_slide && t.gate_open;
                            out.push(SeqEvent::Note { synth: s, midi: step.midi(pat.transpose), accent: step.accent, legato });
                            t.gate_open = true;
                        }
                        Gate::Tie => {
                            if !t.gate_open {
                                out.push(SeqEvent::Note { synth: s, midi: step.midi(pat.transpose), accent: step.accent, legato: false });
                                t.gate_open = true;
                            }
                        }
                        Gate::Off => {
                            if t.gate_open {
                                out.push(SeqEvent::NoteOff { synth: s });
                                t.gate_open = false;
                            }
                        }
                    }
                    t.prev_slide = step.slide && step.gate != Gate::Off;
                    if let Some(lane) = &pat.param_lane {
                        if let Some(value) = lane.values[t.step] {
                            out.push(SeqEvent::Param { id: lane.param, value });
                        }
                    }
                }
                for k in 0..2 {
                    let t = &mut self.tracks[2 + k];
                    let pat = &drums[k][t.pattern as usize];
                    if t.step >= len_of(pat.length) {
                        t.step = 0;
                    }
                    for (inst, hit) in pat.steps[t.step].iter().enumerate() {
                        match hit {
                            DrumHit::Off => {}
                            DrumHit::On => out.push(SeqEvent::Drum { kit: k, inst, accent: false }),
                            DrumHit::Accent => out.push(SeqEvent::Drum { kit: k, inst, accent: true }),
                        }
                    }
                }
                self.phase = Phase::Mid;
            }
            Phase::Mid => {
                for s in 0..2 {
                    let t = &mut self.tracks[s];
                    let pat = &synths[s][t.pattern as usize];
                    let len = len_of(pat.length);
                    if t.gate_open {
                        let step = pat.steps[t.step];
                        let next = pat.steps[(t.step + 1) % len];
                        let hold = (step.slide && next.gate != Gate::Off) || next.gate == Gate::Tie;
                        if !hold {
                            out.push(SeqEvent::NoteOff { synth: s });
                            t.gate_open = false;
                        }
                    }
                }
                for m in 0..4 {
                    let len = if m < 2 {
                        len_of(synths[m][self.tracks[m].pattern as usize].length)
                    } else {
                        len_of(drums[m - 2][self.tracks[m].pattern as usize].length)
                    };
                    let t = &mut self.tracks[m];
                    t.step += 1;
                    if t.step >= len {
                        t.step = 0;
                        if let Some(q) = t.queued.take() {
                            t.pattern = q;
                        }
                    }
                }
                self.global_step += 1;
                self.phase = Phase::StepStart;
            }
        }
        self.samples_to_next += half;
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p acid-engine sequencer`
Expected: all pass.

Hand-checked positions:
- Step 0 mid: 2756.25, which rounds up to 2757.
- Step 1 start: 5512.5, which rounds up to 5513.
- Step 1 mid: 8268.75, which rounds up to 8269.
- With swing 0.5, step 0 is 6890.625 samples long. Step 1 therefore starts at 6891, and step 2 starts at 6890.625 + 4134.375 = 11 025.

- [ ] **Step 5: Commit**

```bash
git add crates/acid-engine
git commit -m "feat(engine): sample-accurate pattern sequencer with swing, slide, tie and queueing"
```

---

### Task 14: Engine, offline render, demo project and `render-demo` CLI

**Files:**
- Create: `crates/acid-engine/src/engine.rs`, `crates/acid-engine/src/render.rs`, `crates/acid-engine/src/demo.rs`
- Modify: `crates/acid-engine/src/lib.rs`, `crates/app/src/main.rs`

**Interfaces:**
- Consumes: everything from Tasks 5–13.
- Produces:
  - `engine::Engine` with the constant `Engine::MAX_BLOCK: usize = 512` and these methods:
    - `new(sample_rate: f32) -> Self`
    - `load_project(&mut self, p: &Project)`
    - `set_param(&mut self, id: ParamId, v: f32)`
    - `set_bpm(&mut self, bpm: f32)` and `set_swing(&mut self, swing: f32)`
    - `play(&mut self)`, `stop(&mut self)` and `is_playing(&self) -> bool`
    - `select_pattern(&mut self, machine: usize, slot: u8)`
    - `set_synth_pattern(&mut self, synth: usize, slot: u8, p: SynthPattern) -> SynthPattern`
    - `set_drum_pattern(&mut self, kit: usize, slot: u8, p: DrumPattern) -> DrumPattern`
    - `playhead(&self) -> [usize; 4]` and `current_patterns(&self) -> [u8; 4]`
    - `process(&mut self, out_l: &mut [f32], out_r: &mut [f32])`, which accepts any length
  - `render::EXPORT_SAMPLE_RATE: u32 = 44_100`.
  - `render::render_patterns(project: &Project, bars: u32) -> (Vec<f32>, Vec<f32>)`.
  - `render::write_wav(path: &Path, l: &[f32], r: &[f32]) -> Result<(), hound::Error>`.
  - `render::render_patterns_wav(project: &Project, bars: u32, path: &Path) -> Result<(), hound::Error>`.
  - `demo::demo_project() -> Project`.
  - The binary: `sidacid render-demo [out.wav]`.

- [ ] **Step 1: Write the failing tests**

`crates/acid-engine/src/engine.rs`, tests first:
```rust
//! The whole groovebox: two bass synths, two drum kits, the mixer and the
//! sequencer, rendered sample-accurately. process() is real-time safe:
//! no allocation, locking or I/O.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{ChannelParam, MasterParam};
    use crate::testutil::{all_finite, peak, rms};

    const SR: f32 = 44_100.0;

    fn render(e: &mut Engine, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        e.process(&mut l, &mut r);
        (l, r)
    }

    fn demo_engine() -> Engine {
        let mut e = Engine::new(SR);
        e.load_project(&crate::demo::demo_project());
        e
    }

    #[test]
    fn stopped_engine_is_silent() {
        let mut e = demo_engine();
        let (l, _) = render(&mut e, 0.5);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn playing_demo_makes_bounded_sound() {
        let mut e = demo_engine();
        e.play();
        let (l, r) = render(&mut e, 4.0);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(rms(&l) > 0.02, "rms = {}", rms(&l));
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn odd_buffer_sizes_render_identically() {
        let mut a = demo_engine();
        let mut b = demo_engine();
        a.play();
        b.play();
        let (la, _) = render(&mut a, 1.0);
        let mut lb = Vec::new();
        let mut remaining = la.len();
        while remaining > 0 {
            let n = remaining.min(37);
            let mut l = vec![0.0; n];
            let mut r = vec![0.0; n];
            b.process(&mut l, &mut r);
            lb.extend(l);
            remaining -= n;
        }
        for (x, y) in la.iter().zip(&lb) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn master_level_zero_is_silent() {
        let mut e = demo_engine();
        e.set_param(ParamId::Master(MasterParam::Level), 0.0);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn stop_closes_synth_gates_and_tails_die() {
        let mut e = demo_engine();
        for c in 0..4 {
            e.set_param(ParamId::Channel(c, ChannelParam::DelaySend), 0.0);
            e.set_param(ParamId::Channel(c, ChannelParam::ReverbSend), 0.0);
        }
        e.play();
        render(&mut e, 1.0);
        e.stop();
        render(&mut e, 3.0);
        let (l, _) = render(&mut e, 0.2);
        assert!(peak(&l) < 1e-3, "peak = {}", peak(&l));
    }

    #[test]
    fn pattern_replacement_returns_old_pattern() {
        let mut e = demo_engine();
        let old = e.set_synth_pattern(0, 0, SynthPattern::default());
        assert!(old.steps.iter().any(|s| s.gate != crate::project::Gate::Off));
        let old_drums = e.set_drum_pattern(0, 0, DrumPattern::default());
        assert!(old_drums.steps.iter().flatten().any(|h| *h != crate::project::DrumHit::Off));
    }

    #[test]
    fn playhead_advances() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.3);
        assert!(e.playhead()[2] >= 2);
        assert_eq!(e.current_patterns(), [0, 0, 0, 0]);
    }
}
```

`crates/acid-engine/src/render.rs`, tests first:
```rust
//! Offline rendering to buffers and 16-bit stereo WAV files.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::rms;

    #[test]
    fn render_length_covers_bars_plus_tail() {
        let mut p = crate::demo::demo_project();
        p.bpm = 120.0; // 1 bar = 2 s
        let (l, r) = render_patterns(&p, 1);
        assert_eq!(l.len(), r.len());
        assert!(l.len() >= 2 * EXPORT_SAMPLE_RATE as usize);
        assert!(l.len() <= 12 * EXPORT_SAMPLE_RATE as usize + 4410);
        assert!(rms(&l[..88_200]) > 0.02);
    }

    #[test]
    fn wav_file_is_written() {
        let path = std::env::temp_dir().join(format!("sidacid-render-{}.wav", std::process::id()));
        let p = crate::demo::demo_project();
        render_patterns_wav(&p, 1, &path).unwrap();
        let reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, EXPORT_SAMPLE_RATE);
        assert_eq!(spec.bits_per_sample, 16);
        assert!(reader.len() > 0);
        std::fs::remove_file(&path).unwrap();
    }
}
```

`crates/acid-engine/src/demo.rs`, test first:
```rust
//! A built-in demo groove: a squelchy acid line on synth 1, a higher
//! counter-line on synth 2, an 808 beat and 909 hats.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_is_valid_and_nonempty() {
        let p = demo_project();
        p.validate().unwrap();
        assert!(p.synths[0].patterns[0].steps.iter().any(|s| s.slide));
        assert!(p.synths[0].patterns[0].steps.iter().any(|s| s.accent));
    }
}
```

Update `lib.rs` so its module list reads:
```rust
pub mod demo;
pub mod drums;
pub mod engine;
pub mod fx;
pub mod mixer;
pub mod params;
pub mod project;
pub mod render;
pub mod sequencer;
pub mod synth;
```
Keep the `testutil` module below the list.

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p acid-engine`
Expected: compile errors (`Engine`, `render_patterns` and `demo_project` are missing).

- [ ] **Step 3: Implement the engine**

Insert above the tests in `engine.rs`:
```rust
use crate::drums::{DrumKit, KitKind};
use crate::mixer::{Mixer, StereoBuf};
use crate::params::ParamId;
use crate::project::{DrumPattern, Project, SynthPattern, PATTERNS};
use crate::sequencer::{SeqEvent, Sequencer};
use crate::synth::BassSynth;

pub struct Engine {
    synths: [BassSynth; 2],
    kits: [DrumKit; 2],
    mixer: Mixer,
    seq: Sequencer,
    synth_patterns: [Vec<SynthPattern>; 2],
    drum_patterns: [Vec<DrumPattern>; 2],
    chans: [StereoBuf; 4],
    events: Vec<SeqEvent>,
}

impl Engine {
    pub const MAX_BLOCK: usize = 512;

    pub fn new(sample_rate: f32) -> Self {
        Self {
            synths: [BassSynth::new(sample_rate), BassSynth::new(sample_rate)],
            kits: [DrumKit::new(KitKind::K808, sample_rate), DrumKit::new(KitKind::K909, sample_rate)],
            mixer: Mixer::new(sample_rate, Self::MAX_BLOCK),
            seq: Sequencer::new(sample_rate),
            synth_patterns: [vec![SynthPattern::default(); PATTERNS], vec![SynthPattern::default(); PATTERNS]],
            drum_patterns: [vec![DrumPattern::default(); PATTERNS], vec![DrumPattern::default(); PATTERNS]],
            chans: std::array::from_fn(|_| StereoBuf::new(Self::MAX_BLOCK)),
            events: Vec::with_capacity(64),
        }
    }

    /// Not real-time safe (clones patterns); call while stopped or from
    /// a non-audio thread before handing the engine to the audio thread.
    pub fn load_project(&mut self, p: &Project) {
        for s in 0..2 {
            self.synth_patterns[s] = p.synths[s].patterns.clone();
            self.drum_patterns[s] = p.drums[s].patterns.clone();
        }
        for id in ParamId::all() {
            self.set_param(id, p.param(id));
        }
        self.set_bpm(p.bpm);
        self.set_swing(p.swing);
        for (m, &slot) in p.selected.iter().enumerate() {
            self.seq.select(m, slot);
        }
    }

    pub fn set_param(&mut self, id: ParamId, v: f32) {
        match id {
            ParamId::Synth(s, p) => self.synths[s as usize].set_param(p, v),
            ParamId::Drum(k, i, p) => self.kits[k as usize].set_param(i as usize, p, v),
            ParamId::Kit(k, p) => self.kits[k as usize].set_kit_param(p, v),
            _ => self.mixer.set_param(id, v),
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.seq.set_bpm(bpm);
        self.mixer.set_bpm(self.seq.bpm());
    }

    pub fn set_swing(&mut self, swing: f32) {
        self.seq.set_swing(swing);
    }

    pub fn play(&mut self) {
        self.seq.play();
    }

    pub fn stop(&mut self) {
        self.seq.stop();
        for s in &mut self.synths {
            s.note_off();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.seq.is_playing()
    }

    pub fn select_pattern(&mut self, machine: usize, slot: u8) {
        self.seq.select(machine, slot);
    }

    pub fn set_synth_pattern(&mut self, synth: usize, slot: u8, p: SynthPattern) -> SynthPattern {
        std::mem::replace(&mut self.synth_patterns[synth][slot as usize], p)
    }

    pub fn set_drum_pattern(&mut self, kit: usize, slot: u8, p: DrumPattern) -> DrumPattern {
        std::mem::replace(&mut self.drum_patterns[kit][slot as usize], p)
    }

    pub fn playhead(&self) -> [usize; 4] {
        std::array::from_fn(|m| self.seq.playhead(m))
    }

    pub fn current_patterns(&self) -> [u8; 4] {
        std::array::from_fn(|m| self.seq.current_pattern(m))
    }

    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        assert_eq!(out_l.len(), out_r.len());
        let mut offset = 0;
        while offset < out_l.len() {
            let n = (out_l.len() - offset).min(Self::MAX_BLOCK);
            self.process_block(&mut out_l[offset..offset + n], &mut out_r[offset..offset + n]);
            offset += n;
        }
    }

    fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len();
        for c in &mut self.chans {
            c.l[..n].fill(0.0);
            c.r[..n].fill(0.0);
        }
        let mut pos = 0;
        while pos < n {
            let until = self.seq.samples_to_next_tick();
            if until == 0 {
                self.events.clear();
                self.seq.tick(
                    [&self.synth_patterns[0], &self.synth_patterns[1]],
                    [&self.drum_patterns[0], &self.drum_patterns[1]],
                    &mut self.events,
                );
                for i in 0..self.events.len() {
                    match self.events[i] {
                        SeqEvent::Note { synth, midi, accent, legato } => self.synths[synth].note_on(midi, accent, legato),
                        SeqEvent::NoteOff { synth } => self.synths[synth].note_off(),
                        SeqEvent::Drum { kit, inst, accent } => self.kits[kit].trigger(inst, accent),
                        SeqEvent::Param { id, value } => self.set_param(id, value),
                    }
                }
                continue;
            }
            let run = until.min(n - pos);
            let span = pos..pos + run;
            for s in 0..2 {
                let c = &mut self.chans[s];
                self.synths[s].render(&mut c.l[span.clone()], &mut c.r[span.clone()]);
            }
            for k in 0..2 {
                let c = &mut self.chans[2 + k];
                self.kits[k].render(&mut c.l[span.clone()], &mut c.r[span.clone()]);
            }
            self.seq.advance(run);
            pos += run;
        }
        self.mixer.process(&mut self.chans, n, out_l, out_r);
    }
}
```

The `Param` event dispatch calls `self.set_param` while iterating `self.events` by index. Indexing copies each `SeqEvent` out of the `Vec`, so there's no overlapping borrow.

- [ ] **Step 4: Implement render and the demo**

Insert above the tests in `render.rs`:
```rust
use crate::engine::Engine;
use crate::project::Project;
use std::path::Path;

pub const EXPORT_SAMPLE_RATE: u32 = 44_100;
const CHUNK: usize = 4410;
const MAX_TAIL_SECONDS: usize = 10;
const SILENCE: f32 = 1.0e-4;

/// Renders the selected patterns for `bars` bars of 16 steps, then lets
/// FX tails ring out (up to 10 s, stopping after 0.1 s of silence).
pub fn render_patterns(project: &Project, bars: u32) -> (Vec<f32>, Vec<f32>) {
    let sr = EXPORT_SAMPLE_RATE as f32;
    let mut engine = Engine::new(sr);
    engine.load_project(project);
    engine.play();
    let step_seconds = 60.0 / project.bpm.clamp(40.0, 300.0) as f64 / 4.0;
    let total = (bars as f64 * 16.0 * step_seconds * sr as f64).round() as usize;
    let mut l = vec![0.0; total];
    let mut r = vec![0.0; total];
    engine.process(&mut l, &mut r);
    engine.stop();
    let mut cl = vec![0.0; CHUNK];
    let mut cr = vec![0.0; CHUNK];
    for _ in 0..(MAX_TAIL_SECONDS * EXPORT_SAMPLE_RATE as usize / CHUNK) {
        engine.process(&mut cl, &mut cr);
        l.extend_from_slice(&cl);
        r.extend_from_slice(&cr);
        if cl.iter().chain(cr.iter()).all(|v| v.abs() < SILENCE) {
            break;
        }
    }
    (l, r)
}

pub fn write_wav(path: &Path, l: &[f32], r: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: EXPORT_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for (a, b) in l.iter().zip(r) {
        w.write_sample((a.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        w.write_sample((b.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    w.finalize()
}

pub fn render_patterns_wav(project: &Project, bars: u32, path: &Path) -> Result<(), hound::Error> {
    let (l, r) = render_patterns(project, bars);
    write_wav(path, &l, &r)
}
```

Insert above the test in `demo.rs`:
```rust
use crate::drums::{BD, CH, CP, OH, SD};
use crate::params::{ChannelParam, FxParam, ParamId, SynthParam};
use crate::project::{DrumHit, Gate, Project, SynthStep};

fn step(note: u8, octave: i8, accent: bool, slide: bool) -> SynthStep {
    SynthStep { note, octave, gate: Gate::On, accent, slide }
}

const REST: SynthStep = SynthStep { note: 0, octave: 0, gate: Gate::Off, accent: false, slide: false };

pub fn demo_project() -> Project {
    let mut p = Project::default();
    p.name = "Demo".into();
    p.bpm = 128.0;
    p.swing = 0.15;

    let acid = [
        step(0, 0, false, false), step(0, 1, true, false), step(0, 0, false, true), step(3, 0, false, false),
        step(0, 0, true, false), step(10, -1, false, true), REST, step(7, 0, true, false),
        step(0, 0, false, false), step(0, 1, false, true), step(0, 1, true, false), step(3, 0, false, false),
        step(5, 0, false, true), step(3, 0, true, false), REST, step(10, -1, false, false),
    ];
    p.synths[0].patterns[0].steps[..16].copy_from_slice(&acid);

    let counter = [
        step(7, 1, false, false), REST, REST, step(10, 1, true, true),
        step(7, 1, false, false), REST, step(3, 1, false, false), REST,
        step(7, 1, true, false), REST, REST, step(5, 1, false, true),
        step(3, 1, false, false), REST, step(0, 1, true, false), REST,
    ];
    p.synths[1].patterns[0].steps[..16].copy_from_slice(&counter);

    let kit808 = &mut p.drums[0].patterns[0];
    for s in [0, 4, 8, 12] {
        kit808.steps[s][BD] = DrumHit::On;
    }
    kit808.steps[4][SD] = DrumHit::On;
    kit808.steps[12][CP] = DrumHit::Accent;
    kit808.steps[15][SD] = DrumHit::On;

    let kit909 = &mut p.drums[1].patterns[0];
    for s in 0..16 {
        kit909.steps[s][CH] = if s % 4 == 2 { DrumHit::Accent } else { DrumHit::On };
    }
    for s in [2, 6, 10, 14] {
        kit909.steps[s][OH] = DrumHit::On;
        kit909.steps[s][CH] = DrumHit::Off;
    }

    let set = |p: &mut Project, id: ParamId, v: f32| {
        p.params.insert(id, v);
    };
    set(&mut p, ParamId::Synth(0, SynthParam::Cutoff), 0.3);
    set(&mut p, ParamId::Synth(0, SynthParam::Resonance), 0.85);
    set(&mut p, ParamId::Synth(0, SynthParam::EnvMod), 0.7);
    set(&mut p, ParamId::Synth(0, SynthParam::Decay), 0.35);
    set(&mut p, ParamId::Synth(0, SynthParam::Accent), 0.8);
    set(&mut p, ParamId::Synth(1, SynthParam::Wave), 1.0);
    set(&mut p, ParamId::Synth(1, SynthParam::Unison), 1.0);
    set(&mut p, ParamId::Synth(1, SynthParam::Cutoff), 0.45);
    set(&mut p, ParamId::Channel(0, ChannelParam::DistOn), 1.0);
    set(&mut p, ParamId::Channel(0, ChannelParam::DistDrive), 0.35);
    set(&mut p, ParamId::Channel(0, ChannelParam::DelaySend), 0.3);
    set(&mut p, ParamId::Channel(1, ChannelParam::Level), 0.5);
    set(&mut p, ParamId::Channel(1, ChannelParam::Pan), 0.65);
    set(&mut p, ParamId::Channel(1, ChannelParam::ReverbSend), 0.4);
    set(&mut p, ParamId::Channel(2, ChannelParam::ReverbSend), 0.1);
    set(&mut p, ParamId::Channel(3, ChannelParam::Level), 0.55);
    set(&mut p, ParamId::Fx(FxParam::DelayFeedback), 0.45);
    p
}
```

- [ ] **Step 5: Implement the CLI**

`crates/app/src/main.rs`:
```rust
//! SidAcid. Plan 1 ships only the offline demo renderer; the GUI arrives
//! in Plan 2.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("render-demo") => {
            let path = args.get(2).map(String::as_str).unwrap_or("demo.wav");
            let project = acid_engine::demo::demo_project();
            match acid_engine::render::render_patterns_wav(&project, 8, Path::new(path)) {
                Ok(()) => {
                    println!("wrote {path}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("render failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: sidacid render-demo [out.wav]");
            ExitCode::from(2)
        }
    }
}
```

- [ ] **Step 6: Run the full suite**

Run: `cargo test --workspace`
Expected: all tests pass across `sid-core` and `acid-engine`.

- [ ] **Step 7: Smoke test by ear**

Run: `cargo run --release -p sidacid -- render-demo /tmp/sidacid-demo.wav`
Expected output: `wrote /tmp/sidacid-demo.wav`, about 15 s of audio.

Play it with `aplay /tmp/sidacid-demo.wav` or `pw-play /tmp/sidacid-demo.wav`. Listen for:
- a resonant acid line with audible slides and accent "wow"
- a detuned counter-line panned slightly right
- an 808 kick, snare and clap
- 909 hats that alternate open and closed
- a delay echo and a little reverb

Ask the user to listen and give feedback. The sound design numbers in Tasks 7–9 are starting points, so tune them by ear. If you retune any constants, re-run `cargo test --workspace`.

- [ ] **Step 8: Commit**

```bash
git add crates
git commit -m "feat: engine, offline WAV render, demo project and render-demo CLI"
```

---

## Spec coverage (Plan 1)

| Spec section | Covered by |
|---|---|
| §2 Workspace layout, sid-core / acid-engine / app | Tasks 1, 5, 14 |
| §3 sid-core: 3 voices, f32, Hz pitch, hard sync, ring, exp envelopes, filter, waveforms | Tasks 1–4 |
| §4 Bass synth controls and behaviour (accent sweep, slide, unison, sync) | Task 7 |
| §5 Drum machines (pool of 6, programs, choke, 808/909, knobs) | Tasks 8–9 |
| §6 Mixer and FX (distortion, compressor, delay, reverb, lo-fi, strips, NaN guard) | Tasks 10–12 |
| §7 Data model and project files | Task 6 |
| §8 Sequencer pattern mode, swing, polymeter, queueing, sample-accurate | Task 13 |
| §9 Param lane playback | Task 13 |
| §12 Offline render to WAV (patterns) | Task 14 |
| §15 Testing incl. smoke `render-demo` | All tasks, Task 14 |
| §8 song-mode playback, §10 song and automation, §11 randomise and undo, §12 song export | **Plan 3** |
| §9 editors, §13 UI, §14 audio-device errors, command queue | **Plan 2** |
