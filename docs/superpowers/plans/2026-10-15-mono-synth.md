# Mono Synth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Mono" rack machine: a monophonic virtual-analog synth (one VCO, sub osc, noise, a 24 dB ladder filter, one ADSR, LFO, portamento, drift, chorus) with a tempo-synced Up / U&D / Down arpeggiator. It is sequenced by the existing poly piano roll and laid out like the FM panel.

**Architecture:** There is a new `acid-engine/src/va/` module holding the DSP blocks (osc, ladder, adsr, lfo, arp), plus `MonoPatch` (knob values decoded into real units) and `MonoSynth`. The rack gains `MachineType::Mono`, `MachineParam::Mono(MonoParam)`, `MachineBody::Mono(MonoMachine)` and `MachineDsp::Mono`, routed like the FM. The engine clocks the arp once per sequencer tick (`Sequencer::tick_clock` → `MonoSynth::clock_tick`). The app gets `ui/mono_panel.rs` and `ui/mono_patches.rs`, mirroring `fm_panel.rs` and `fm_voices.rs`.

**Tech Stack:** Rust (workspace crates `acid-engine`, `app`), egui/eframe, serde + RON, `rand` 0.8 (`gen_range`, `gen::<f32>()`) with `rand_pcg::Pcg32`.

**Spec:** `docs/superpowers/specs/2026-10-15-mono-synth-design.md`

## Global Constraints

- User-facing text (UI labels, preset names, README) never names a hardware product or manufacturer. The machine is called "Mono" (menu label "Mono", name stem "Mono").
- `process()` / `render()` paths stay allocation-, lock- and I/O-free. All buffers are sized in `new`.
- Every new param goes through `MachineParam` so it is automatable and recordable with no extra UI work.
- `PROJECT_VERSION` becomes `4`. Version-3 files load unchanged.
- Drift 0 must add exactly nothing (output bit-identical between runs).
- Arp rates are thirds of a sequencer tick: `[48, 24, 16, 12, 8, 6]` for 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32.
- MIDI pitch convention as in the FM: 48 = C3, `hz = 440 · 2^((n − 69)/12)`.
- Match the surrounding code: `//!` module docs, `///` on pub items, comments that explain the why, no comment noise.
- Commits: conventional style (`feat(engine): …`, `feat(app): …`, `refactor(engine): …`), each ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Git has no global identity here, so commit with
  `git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit …`.
- Run the full check before each commit: `cargo test -p acid-engine && cargo test -p app && cargo clippy --workspace --all-targets -- -D warnings`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/acid-engine/src/fx/chorus.rs` (new, moved) | Stereo chorus shared by FM and Mono |
| `crates/acid-engine/src/va/mod.rs` (new) | Module root; re-exports `MonoSynth` |
| `crates/acid-engine/src/va/osc.rs` (new) | PolyBLEP saw/pulse VCO + phase-locked sub osc |
| `crates/acid-engine/src/va/ladder.rs` (new) | ZDF 4-pole ladder low-pass with tanh feedback |
| `crates/acid-engine/src/va/adsr.rs` (new) | ADSR envelope |
| `crates/acid-engine/src/va/lfo.rs` (new) | LFO, white-noise source, Drift wander |
| `crates/acid-engine/src/va/patch.rs` (new) | `MonoPatch`: knob values in real units, enums for switches |
| `crates/acid-engine/src/va/arp.rs` (new) | Arp note order and step clock |
| `crates/acid-engine/src/va/synth.rs` (new) | `MonoSynth`: note stack, voice, render, arp glue |
| `crates/acid-engine/src/va/starter.rs` (new) | Built-in starter patches |
| `crates/acid-engine/src/params.rs` | `MonoParam`, `mono_steps`, `MachineParam::Mono` |
| `crates/acid-engine/src/machine.rs` | `MachineType::Mono` |
| `crates/acid-engine/src/project.rs` | `MonoMachine`, `MachineBody::Mono`, v4 |
| `crates/acid-engine/src/rack.rs`, `engine.rs`, `sequencer.rs`, `song.rs` | Routing and clocking |
| `crates/acid-engine/src/random.rs` | `randomise_mono` |
| `crates/app/src/document.rs` | `Edit::MonoPatch` |
| `crates/app/src/ui/mono_panel.rs` (new) | The panel |
| `crates/app/src/ui/mono_patches.rs` (new) | Patch load and Randomise (one undo step) |
| `crates/app/src/ui/{mod.rs,files.rs,params_ui.rs,polyroll.rs,rack_header.rs}` | Dispatch, clipboard, knob names, roll start pitch, menu test |

---

### Task 1: Move the chorus into `fx/chorus.rs`

**Files:**
- Create: `crates/acid-engine/src/fx/chorus.rs`
- Modify: `crates/acid-engine/src/fx/mod.rs`, `crates/acid-engine/src/fm/fx.rs`

**Interfaces:**
- Produces: `acid_engine::fx::Chorus` with `new(sample_rate: f32) -> Self`, `reset(&mut self)`, `process(&mut self, x: f32, rate: f32, depth: f32) -> (f32, f32)`. `rate` and `depth` are normalised 0..1. This is unchanged from today's `fm::fx::Chorus`.

- [ ] **Step 1: Move the code.** Cut `pub struct Chorus` and its whole `impl Chorus` block out of `crates/acid-engine/src/fm/fx.rs` and paste them into the new file `crates/acid-engine/src/fx/chorus.rs` with this header:

```rust
//! Stereo chorus: one modulated delay line read at two LFO phases 90°
//! apart. Shared by the FM and Mono machines.

use crate::params::exp;
use std::f32::consts::{FRAC_PI_2, TAU};
```

- [ ] **Step 2: Wire the module.** In `crates/acid-engine/src/fx/mod.rs` add `pub mod chorus;` (alphabetically, first) and `pub use chorus::Chorus;`. In `crates/acid-engine/src/fm/fx.rs` add `use crate::fx::Chorus;`. Then remove imports that are now unused there: run `cargo build -p acid-engine` and drop whatever the warnings name (likely `FRAC_PI_2`).

- [ ] **Step 3: Run the existing tests (behaviour must not change).**

Run: `cargo test -p acid-engine fm::fx`
Expected: PASS, including `chorus_widens_and_phaser_colours`.

- [ ] **Step 4: Commit.**

```bash
git add crates/acid-engine/src/fx crates/acid-engine/src/fm/fx.rs
git commit -m "refactor(engine): the chorus moves to fx so other machines can use it"
```

---

### Task 2: VCO with PolyBLEP and a phase-locked sub osc

**Files:**
- Create: `crates/acid-engine/src/va/mod.rs`, `crates/acid-engine/src/va/osc.rs`
- Modify: `crates/acid-engine/src/lib.rs` (add `pub mod va;` after `pub mod synth;`)

**Interfaces:**
- Produces: `va::osc::{Vco, VcoOut, SubWave}`.
  - `Vco::new() -> Vco`, `reset(&mut self)`.
  - `next(&mut self, dt: f32, duty: f32, sub: SubWave) -> VcoOut`. `dt` = hz / sample rate; `duty` is 0.05..=0.5.
  - `VcoOut { saw: f32, pulse: f32, sub: f32 }`, each in about −1..1.
  - `SubWave::{Oct1Square, Oct2Square, Oct2Pulse}`.

- [ ] **Step 1: Create the module root** `crates/acid-engine/src/va/mod.rs`:

```rust
//! The Mono machine: a monophonic virtual-analog synth (VCO with sub
//! osc and noise, 24 dB ladder, one ADSR, LFO, portamento, drift,
//! chorus) with a tempo-synced arpeggiator.

pub mod osc;
```

Add `pub mod va;` to `crates/acid-engine/src/lib.rs` after `pub mod synth;`.

- [ ] **Step 2: Write the failing tests** at the bottom of the new `crates/acid-engine/src/va/osc.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Magnitude of `hz` in `x` (one DFT bin).
    fn magnitude(x: &[f32], hz: f64) -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, v) in x.iter().enumerate() {
            let a = std::f64::consts::TAU * hz * n as f64 / SR as f64;
            re += *v as f64 * a.cos();
            im -= *v as f64 * a.sin();
        }
        (re * re + im * im).sqrt() / x.len() as f64
    }

    #[test]
    fn the_saw_aliases_far_less_than_a_naive_saw() {
        // 2950 Hz: harmonic 9 (26550 Hz) folds to 21450 Hz.
        let f = 2950.0;
        let mut vco = Vco::new();
        let blep: Vec<f32> = (0..SR as usize)
            .map(|_| vco.next(f / SR, 0.5, SubWave::Oct1Square).saw)
            .collect();
        let naive: Vec<f32> = (0..SR as usize)
            .map(|n| 2.0 * ((n as f32 * f / SR) % 1.0) - 1.0)
            .collect();
        let ratio = |x: &[f32]| magnitude(x, 21_450.0) / magnitude(x, f as f64);
        assert!(ratio(&blep) < 0.5 * ratio(&naive), "{} vs {}", ratio(&blep), ratio(&naive));
    }

    fn sign_changes(x: &[f32]) -> usize {
        x.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count()
    }

    #[test]
    fn the_sub_is_an_octave_below_and_locked() {
        let mut vco = Vco::new();
        let sub: Vec<f32> = (0..SR as usize)
            .map(|_| vco.next(1000.0 / SR, 0.5, SubWave::Oct1Square).sub)
            .collect();
        // A 500 Hz square changes sign 1000 times a second.
        assert!((sign_changes(&sub) as i32 - 1000).abs() <= 2, "{}", sign_changes(&sub));
    }

    #[test]
    fn the_two_octave_pulse_is_high_a_quarter_of_the_time() {
        let mut vco = Vco::new();
        let sub: Vec<f32> = (0..SR as usize)
            .map(|_| vco.next(1000.0 / SR, 0.5, SubWave::Oct2Pulse).sub)
            .collect();
        let high = sub.iter().filter(|v| **v > 0.0).count() as f32 / sub.len() as f32;
        assert!((high - 0.25).abs() < 0.01, "{high}");
    }

    #[test]
    fn pulse_width_sets_the_duty_cycle() {
        for (duty, mean) in [(0.5, 0.0), (0.1, -0.8)] {
            let mut vco = Vco::new();
            let x: Vec<f32> = (0..SR as usize)
                .map(|_| vco.next(220.0 / SR, duty, SubWave::Oct1Square).pulse)
                .collect();
            let m = x.iter().sum::<f32>() / x.len() as f32;
            assert!((m - mean).abs() < 0.02, "duty {duty}: mean {m}");
        }
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail.**

Run: `cargo test -p acid-engine va::osc`
Expected: FAIL to compile (`Vco` not found).

- [ ] **Step 4: Implement** above the tests in `va/osc.rs`:

```rust
//! The Mono synth's VCO: band-limited (PolyBLEP) saw and pulse, plus a
//! sub oscillator that counts the VCO's cycles so it stays phase-locked.

/// Sub oscillator shapes, in `MonoParam::SubWave` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubWave {
    Oct1Square,
    Oct2Square,
    /// Two octaves down, high one cycle in four (25%).
    Oct2Pulse,
}

/// PolyBLEP residual at phase `t` for a step of −2 at phase 0. For a step
/// of height `h`, add `h / 2 · poly_blep`.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VcoOut {
    pub saw: f32,
    pub pulse: f32,
    pub sub: f32,
}

pub struct Vco {
    phase: f32,
    /// VCO cycles completed; the sub osc's shape is read from its low bits.
    cycles: u32,
}

impl Default for Vco {
    fn default() -> Self {
        Self::new()
    }
}

impl Vco {
    pub fn new() -> Self {
        Self { phase: 0.0, cycles: 0 }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    fn sub_value(wave: SubWave, cycles: u32) -> f32 {
        let high = match wave {
            SubWave::Oct1Square => cycles & 1 == 0,
            SubWave::Oct2Square => cycles & 2 == 0,
            SubWave::Oct2Pulse => cycles & 3 == 0,
        };
        if high { 1.0 } else { -1.0 }
    }

    /// One sample. `dt` is frequency / sample rate (clamped to 0..=0.45);
    /// `duty` is the pulse's high fraction (clamped to 0.05..=0.5).
    pub fn next(&mut self, dt: f32, duty: f32, sub: SubWave) -> VcoOut {
        let dt = dt.clamp(0.0, 0.45);
        let duty = duty.clamp(0.05, 0.5);
        let t = self.phase;
        let saw = 2.0 * t - 1.0 - poly_blep(t, dt);
        let naive = if t < duty { 1.0 } else { -1.0 };
        let pulse = naive + poly_blep(t, dt) - poly_blep((t - duty).rem_euclid(1.0), dt);
        // The sub only steps at the VCO's wrap, so it shares the saw's blep
        // window: just after the wrap it smooths the step it took, just
        // before it the step it is about to take.
        let cur = Self::sub_value(sub, self.cycles);
        let mut s = cur;
        if t < dt {
            let prev = Self::sub_value(sub, self.cycles.wrapping_sub(1));
            s += 0.5 * (cur - prev) * poly_blep(t, dt);
        } else if t > 1.0 - dt {
            let next = Self::sub_value(sub, self.cycles.wrapping_add(1));
            s += 0.5 * (next - cur) * poly_blep(t, dt);
        }
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.cycles = self.cycles.wrapping_add(1);
        }
        VcoOut { saw, pulse, sub: s }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass.**

Run: `cargo test -p acid-engine va::osc`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit.**

```bash
git add crates/acid-engine/src/va crates/acid-engine/src/lib.rs
git commit -m "feat(engine): Mono VCO with band-limited saw and pulse and a locked sub osc"
```

---

### Task 3: Ladder filter

**Files:**
- Create: `crates/acid-engine/src/va/ladder.rs`
- Modify: `crates/acid-engine/src/va/mod.rs` (add `pub mod ladder;`)

**Interfaces:**
- Produces: `va::ladder::Ladder`.
  - `new(sample_rate: f32) -> Ladder`, `reset(&mut self)`.
  - `set(&mut self, cutoff_hz: f32, k: f32)`: `k` is 0..=4.2, and 4 is the self-oscillation threshold.
  - `process(&mut self, x: f32) -> f32`.

- [ ] **Step 1: Write the failing tests** at the bottom of `va/ladder.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{peak, rms};

    const SR: f32 = 48_000.0;

    fn ring(k: f32) -> Vec<f32> {
        let mut f = Ladder::new(SR);
        f.set(1000.0, k);
        (0..SR as usize)
            .map(|n| f.process(if n == 0 { 1.0 } else { 0.0 }))
            .collect()
    }

    #[test]
    fn full_resonance_self_oscillates_near_the_cutoff_and_stays_bounded() {
        let y = ring(4.2);
        let tail = &y[y.len() - 24_000..];
        assert!(rms(tail) > 0.05, "{}", rms(tail));
        assert!(peak(&y) < 2.0);
        let crossings = tail.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count();
        // 1000 Hz over half a second: 1000 sign changes.
        assert!((950..=1050).contains(&crossings), "{crossings}");
    }

    #[test]
    fn below_the_threshold_it_rings_out() {
        let y = ring(3.0);
        assert!(rms(&y[y.len() - 4800..]) < 1e-4);
    }

    #[test]
    fn it_is_a_steep_low_pass() {
        let tone = |hz: f32| -> (f32, f32) {
            let mut f = Ladder::new(SR);
            f.set(500.0, 0.0);
            let x: Vec<f32> = (0..SR as usize)
                .map(|n| 0.1 * (std::f32::consts::TAU * hz * n as f32 / SR).sin())
                .collect();
            let y: Vec<f32> = x.iter().map(|v| f.process(*v)).collect();
            (rms(&x[4800..]), rms(&y[4800..]))
        };
        let (inp, out) = tone(100.0);
        assert!(out > 0.9 * inp, "passband {out} of {inp}");
        let (inp, out) = tone(5000.0);
        assert!(out < 0.01 * inp, "stopband {out} of {inp}");
    }
}
```

- [ ] **Step 2: Run them to verify they fail.**

Run: `cargo test -p acid-engine va::ladder`
Expected: FAIL to compile (`Ladder` not found).

- [ ] **Step 3: Implement** in `va/ladder.rs` above the tests:

```rust
//! 24 dB/oct resonant low-pass: four zero-delay-feedback (TPT) one-pole
//! stages in a global feedback loop. The loop's input goes through tanh,
//! so at full resonance the filter self-oscillates at a bounded level.

use std::f32::consts::PI;

pub struct Ladder {
    s: [f32; 4],
    g: f32,
    /// g / (1 + g): each stage's instantaneous gain.
    big_g: f32,
    k: f32,
    sample_rate: f32,
}

impl Ladder {
    pub fn new(sample_rate: f32) -> Self {
        let mut l = Self {
            s: [0.0; 4],
            g: 0.0,
            big_g: 0.0,
            k: 0.0,
            sample_rate,
        };
        l.set(1000.0, 0.0);
        l
    }

    pub fn reset(&mut self) {
        self.s = [0.0; 4];
    }

    /// `cutoff_hz` is clamped to 10 Hz..0.45 × sample rate, `k` to 0..=4.2.
    pub fn set(&mut self, cutoff_hz: f32, k: f32) {
        let fc = cutoff_hz.clamp(10.0, 0.45 * self.sample_rate);
        self.g = (PI * fc / self.sample_rate).tan();
        self.big_g = self.g / (1.0 + self.g);
        self.k = k.clamp(0.0, 4.2);
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let gg = self.big_g;
        let b = 1.0 / (1.0 + self.g);
        // Linear solve of the loop: y4 = (G⁴·u + S) / (1 + k·G⁴), where S
        // is what the stage states contribute to the last stage's output.
        let s = &self.s;
        let s_sum = gg * gg * gg * b * s[0] + gg * gg * b * s[1] + gg * b * s[2] + b * s[3];
        let g4 = gg * gg * gg * gg;
        // Makeup so raising the resonance doesn't hollow out the passband.
        let input = x * (1.0 + 0.5 * self.k);
        let y4 = (g4 * input + s_sum) / (1.0 + self.k * g4);
        let mut u = (input - self.k * y4).tanh();
        for s in &mut self.s {
            let v = (u - *s) * gg;
            let y = v + *s;
            *s = y + v;
            u = y;
        }
        u
    }
}
```

Add `pub mod ladder;` to `va/mod.rs`.

- [ ] **Step 4: Run the tests.**

Run: `cargo test -p acid-engine va::ladder`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit.**

```bash
git add crates/acid-engine/src/va
git commit -m "feat(engine): 24 dB ladder filter that self-oscillates at full resonance"
```

---

### Task 4: ADSR, LFO, noise and drift

**Files:**
- Create: `crates/acid-engine/src/va/adsr.rs`, `crates/acid-engine/src/va/lfo.rs`
- Modify: `crates/acid-engine/src/va/mod.rs` (add `pub mod adsr;`, `pub mod lfo;`)

**Interfaces:**
- Produces:
  - `va::adsr::{Adsr, Stage}`:
    - `Adsr::new(sample_rate: f32)`
    - `set(&mut self, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32)`
    - `gate_on(&mut self)`, `gate_off(&mut self)`, `reset(&mut self)`
    - `next(&mut self) -> f32`, `stage(&self) -> Stage`, `level(&self) -> f32`
    - `Stage::{Idle, Attack, Decay, Sustain, Release}`
  - `va::lfo::{Lfo, LfoWave, Noise, Drift}`:
    - `Lfo::new(sample_rate: f32)`, `reset`, `next(&mut self, wave: LfoWave, hz: f32) -> (f32, bool)`: the value in −1..=1, and true when a cycle just began.
    - `LfoWave::{Triangle, Square, Random, Noise}`
    - `Noise::new(seed: u32)`, `next(&mut self) -> f32` (−1..=1)
    - `Drift::new(sample_rate: f32)`, `reset`, `next(&mut self) -> [f32; 2]` (pitch and cutoff wander, each −1..=1), `note_offset(&mut self) -> f32` (−1..=1)

- [ ] **Step 1: Write the failing ADSR tests** at the bottom of `va/adsr.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn run(e: &mut Adsr, n: usize) -> f32 {
        let mut v = 0.0;
        for _ in 0..n {
            v = e.next();
        }
        v
    }

    #[test]
    fn stages_take_their_times() {
        let mut e = Adsr::new(SR);
        e.set(0.01, 0.1, 0.5, 0.1);
        e.gate_on();
        assert!((run(&mut e, 240) - 0.5).abs() < 0.01, "half way up the attack");
        run(&mut e, 240);
        assert_eq!(e.stage(), Stage::Decay);
        let after_decay = run(&mut e, 4800);
        assert!(after_decay - 0.5 <= 0.5 * 0.011, "{after_decay}");
        run(&mut e, 4800);
        assert_eq!(e.stage(), Stage::Sustain);
        e.gate_off();
        assert!(run(&mut e, 4800) <= 0.5 * 0.011);
    }

    #[test]
    fn retriggering_starts_the_attack_from_the_current_level() {
        let mut e = Adsr::new(SR);
        e.set(0.05, 0.1, 0.5, 1.0);
        e.gate_on();
        run(&mut e, 48_000);
        e.gate_off();
        let before = run(&mut e, 4800);
        e.gate_on();
        assert!(e.next() >= before, "no drop to zero (no click)");
        assert_eq!(e.stage(), Stage::Attack);
    }

    #[test]
    fn it_ends_idle_after_release() {
        let mut e = Adsr::new(SR);
        e.set(0.001, 0.01, 0.5, 0.01);
        e.gate_on();
        run(&mut e, 4800);
        e.gate_off();
        assert_eq!(run(&mut e, 48_000), 0.0);
        assert_eq!(e.stage(), Stage::Idle);
    }
}
```

- [ ] **Step 2: Write the failing LFO and drift tests** at the bottom of `va/lfo.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    #[test]
    fn the_triangle_spans_minus_one_to_one_and_wraps_once_a_cycle() {
        let mut l = Lfo::new(SR);
        let (mut lo, mut hi, mut wraps) = (f32::MAX, f32::MIN, 0);
        for _ in 0..2 * SR as usize {
            let (v, w) = l.next(LfoWave::Triangle, 1.0);
            lo = lo.min(v);
            hi = hi.max(v);
            wraps += w as usize;
        }
        assert!(lo < -0.99 && hi > 0.99, "{lo}..{hi}");
        assert_eq!(wraps, 2);
    }

    #[test]
    fn random_holds_its_value_for_a_cycle() {
        let mut l = Lfo::new(SR);
        let mut changes = 0;
        let mut last = l.next(LfoWave::Random, 10.0).0;
        for _ in 0..SR as usize {
            let v = l.next(LfoWave::Random, 10.0).0;
            changes += (v != last) as usize;
            last = v;
        }
        assert!((9..=11).contains(&changes), "{changes}");
    }

    #[test]
    fn drift_wanders_slowly_and_deterministically() {
        let (mut a, mut b) = (Drift::new(SR), Drift::new(SR));
        let mut moved = false;
        let mut prev = a.next();
        b.next();
        for _ in 0..SR as usize {
            let v = a.next();
            assert_eq!(v, b.next());
            assert!(v.iter().all(|x| (-1.0..=1.0).contains(x)));
            assert!((v[0] - prev[0]).abs() < 0.01, "slow");
            moved |= v != prev;
            prev = v;
        }
        assert!(moved);
    }
}
```

- [ ] **Step 3: Run them to verify they fail.**

Run: `cargo test -p acid-engine va::adsr va::lfo`
Expected: FAIL to compile.

- [ ] **Step 4: Implement `va/adsr.rs`:**

```rust
//! The Mono synth's one envelope: linear attack, exponential (RC-style)
//! decay and release. A retrigger restarts the attack from the current
//! level, so it never clicks down to zero.

/// Decay and release cover 99% of their way in their set time.
const LN_100: f32 = 4.605_17;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub struct Adsr {
    stage: Stage,
    level: f32,
    attack_step: f32,
    decay_coef: f32,
    release_coef: f32,
    sustain: f32,
    sample_rate: f32,
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        let mut e = Self {
            stage: Stage::Idle,
            level: 0.0,
            attack_step: 0.0,
            decay_coef: 0.0,
            release_coef: 0.0,
            sustain: 0.0,
            sample_rate,
        };
        e.set(0.005, 0.3, 0.6, 0.2);
        e
    }

    pub fn set(&mut self, attack_s: f32, decay_s: f32, sustain: f32, release_s: f32) {
        let sr = self.sample_rate;
        self.attack_step = 1.0 / (attack_s.max(1e-4) * sr);
        self.decay_coef = (-LN_100 / (decay_s.max(1e-4) * sr)).exp();
        self.release_coef = (-LN_100 / (release_s.max(1e-4) * sr)).exp();
        self.sustain = sustain.clamp(0.0, 1.0);
    }

    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level = self.sustain + (self.level - self.sustain) * self.decay_coef;
                if (self.level - self.sustain).abs() < 1e-4 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            // Follows Sustain knob moves while held.
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level *= self.release_coef;
                if self.level < 1e-5 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}
```

- [ ] **Step 5: Implement `va/lfo.rs`:**

```rust
//! The Mono synth's modulation sources: the LFO, a white-noise source
//! (also the noise oscillator) and Drift, a slow analog-style wander.

/// Tiny deterministic noise source (xorshift32), so renders repeat exactly.
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// White noise in −1..=1.
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// LFO shapes, in `MonoParam::LfoWave` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoWave {
    Triangle,
    Square,
    /// Sample & hold: a new random value each cycle.
    Random,
    Noise,
}

pub struct Lfo {
    phase: f32,
    held: f32,
    noise: Noise,
    sample_rate: f32,
}

impl Lfo {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            held: 0.0,
            noise: Noise::new(0x1234_5678),
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.held = 0.0;
    }

    /// One sample at `hz`: the value in −1..=1, and true when a cycle just
    /// began (the envelope's LFO trigger).
    pub fn next(&mut self, wave: LfoWave, hz: f32) -> (f32, bool) {
        self.phase += hz / self.sample_rate;
        let wrapped = self.phase >= 1.0;
        if wrapped {
            self.phase -= self.phase.floor();
            self.held = self.noise.next();
        }
        let p = self.phase;
        let v = match wave {
            LfoWave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
            LfoWave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoWave::Random => self.held,
            LfoWave::Noise => self.noise.next(),
        };
        (v, wrapped)
    }
}

/// Samples between Drift's new random targets (~85 ms at 48 kHz).
const DRIFT_HOLD: u32 = 4096;
/// Drift's smoothing time constant, in seconds.
const DRIFT_TAU_S: f32 = 0.4;

/// Two smoothed random walks (pitch, cutoff). The caller scales them by
/// the Drift knob, so Drift 0 adds exactly nothing.
pub struct Drift {
    noise: Noise,
    targets: [f32; 2],
    values: [f32; 2],
    count: u32,
    coef: f32,
}

impl Drift {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            noise: Noise::new(0x9e37_79b9),
            targets: [0.0; 2],
            values: [0.0; 2],
            count: 0,
            coef: 1.0 - (-1.0 / (DRIFT_TAU_S * sample_rate)).exp(),
        }
    }

    pub fn reset(&mut self) {
        self.targets = [0.0; 2];
        self.values = [0.0; 2];
        self.count = 0;
    }

    /// (pitch, cutoff) wander, each in −1..=1.
    pub fn next(&mut self) -> [f32; 2] {
        if self.count == 0 {
            self.targets = [self.noise.next(), self.noise.next()];
            self.count = DRIFT_HOLD;
        }
        self.count -= 1;
        for i in 0..2 {
            self.values[i] += (self.targets[i] - self.values[i]) * self.coef;
        }
        self.values
    }

    /// A fresh per-note offset in −1..=1.
    pub fn note_offset(&mut self) -> f32 {
        self.noise.next()
    }
}
```

Add `pub mod adsr;` and `pub mod lfo;` to `va/mod.rs`.

- [ ] **Step 6: Run the tests.**

Run: `cargo test -p acid-engine va::`
Expected: PASS (all va tests so far).

- [ ] **Step 7: Commit.**

```bash
git add crates/acid-engine/src/va
git commit -m "feat(engine): Mono envelope, LFO, noise and drift"
```

---

### Task 5: `MonoParam` and `MonoPatch`

**Files:**
- Modify: `crates/acid-engine/src/params.rs` (add `mono_steps`, `MonoParam`, `MonoParam::info`)
- Create: `crates/acid-engine/src/va/patch.rs`
- Modify: `crates/acid-engine/src/va/mod.rs` (add `pub mod patch;`)

**Interfaces:**
- Consumes: `SubWave` (Task 2), `LfoWave` (Task 4).
- Produces:
  - `params::MonoParam`, a `param_enum!` with exactly these variants in this order: Range, Tune, VcoMod, PulseWidth, PwmSource, PulseLevel, SawLevel, SubLevel, SubWave, NoiseLevel, Cutoff, Resonance, EnvAmount, VcfMod, KeyFollow, VelToVcf, Attack, Decay, Sustain, Release, Trigger, VcaMode, VelToVca, LfoWave, LfoRate, Portamento, PortaTime, ArpOn, ArpMode, ArpRange, ArpRate, Drift, ChorusRate, ChorusDepth, ChorusMix, Volume.
  - `MonoParam::info(self) -> ParamInfo`.
  - `params::mono_steps::{RANGE, PWM_SOURCE, SUB_WAVE, TRIGGER, VCA_MODE, LFO_WAVES, PORTAMENTO, ARP_MODE, ARP_RANGE, ARP_RATE}`.
  - `va::patch::{MonoPatch, PwmSource, Trigger, Porta, ArpMode, ARP_THIRDS}`, with `MonoPatch::default()` and `MonoPatch::set(&mut self, p: MonoParam, v: f32)`. The field names are listed in Step 4.

- [ ] **Step 1: Write the failing tests** at the bottom of `va/patch.rs` (create the file with only the test module for now):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MonoParam as M;

    #[test]
    fn defaults_are_a_playable_eight_foot_saw() {
        let p = MonoPatch::default();
        assert_eq!(p.octave_semis, 0.0);
        assert_eq!(p.saw, 0.8);
        assert_eq!(p.arp_thirds, 12, "1/16");
        assert!(!p.arp_on);
        assert_eq!(p.porta, Porta::Off);
        assert_eq!(p.trigger, Trigger::GateTrig);
        assert_eq!(p.sub_wave, SubWave::Oct1Square);
    }

    #[test]
    fn switches_pick_every_position() {
        let mut p = MonoPatch::default();
        p.set(M::Range, 0.0);
        assert_eq!(p.octave_semis, -12.0);
        p.set(M::Range, 1.0);
        assert_eq!(p.octave_semis, 24.0);
        p.set(M::ArpRate, 0.0);
        assert_eq!(p.arp_thirds, 48);
        p.set(M::ArpRate, 1.0);
        assert_eq!(p.arp_thirds, 6);
        p.set(M::ArpRange, 1.0);
        assert_eq!(p.arp_octaves, 3);
        p.set(M::Portamento, 1.0);
        assert_eq!(p.porta, Porta::Auto);
        p.set(M::PwmSource, 0.5);
        assert_eq!(p.pwm_source, PwmSource::Lfo);
        p.set(M::LfoWave, 1.0);
        assert_eq!(p.lfo_wave, LfoWave::Noise);
    }

    #[test]
    fn every_param_has_a_default_in_range() {
        for m in M::ALL {
            let i = m.info();
            assert!((0.0..=1.0).contains(&i.default), "{m:?}");
        }
        assert_eq!(M::ALL.len(), 36);
    }

    #[test]
    fn out_of_range_values_clamp() {
        let mut p = MonoPatch::default();
        p.set(M::Cutoff, 5.0);
        assert!((p.cutoff_hz - 18_000.0).abs() < 1.0);
        p.set(M::Resonance, -1.0);
        assert_eq!(p.resonance_k, 0.0);
    }
}
```

Add `pub mod patch;` to `va/mod.rs`.

- [ ] **Step 2: Run them to verify they fail.**

Run: `cargo test -p acid-engine va::patch`
Expected: FAIL to compile.

- [ ] **Step 3: Add the params** to `crates/acid-engine/src/params.rs`. Put `mono_steps` after `mod fm_steps`, and `MonoParam` after `param_enum!(MasterParam …)`:

```rust
/// Positions of the Mono synth's switches.
pub mod mono_steps {
    pub const RANGE: usize = 4;
    pub const PWM_SOURCE: usize = 3;
    pub const SUB_WAVE: usize = 3;
    pub const TRIGGER: usize = 3;
    pub const VCA_MODE: usize = 2;
    pub const LFO_WAVES: usize = 4;
    pub const PORTAMENTO: usize = 3;
    pub const ARP_MODE: usize = 3;
    pub const ARP_RANGE: usize = 3;
    pub const ARP_RATE: usize = 6;
}
```

```rust
param_enum!(MonoParam {
    Range => "Range", Tune => "Tune", VcoMod => "VCO Mod", PulseWidth => "Pulse Width",
    PwmSource => "PWM Source", PulseLevel => "Pulse", SawLevel => "Saw", SubLevel => "Sub",
    SubWave => "Sub Wave", NoiseLevel => "Noise", Cutoff => "Cutoff", Resonance => "Resonance",
    EnvAmount => "Env", VcfMod => "VCF Mod", KeyFollow => "Key Follow", VelToVcf => "Vel > VCF",
    Attack => "Attack", Decay => "Decay", Sustain => "Sustain", Release => "Release",
    Trigger => "Trigger", VcaMode => "VCA Mode", VelToVca => "Vel > VCA",
    LfoWave => "LFO Wave", LfoRate => "LFO Rate", Portamento => "Portamento",
    PortaTime => "Porta Time", ArpOn => "Arp", ArpMode => "Arp Mode", ArpRange => "Arp Range",
    ArpRate => "Arp Rate", Drift => "Drift", ChorusRate => "Chorus Rate",
    ChorusDepth => "Chorus Depth", ChorusMix => "Chorus Mix", Volume => "Volume",
});

impl MonoParam {
    pub fn info(self) -> ParamInfo {
        use MonoParam as M;
        use mono_steps as s;
        let n = self.name();
        // A switch resting on position `at` of `steps`.
        let switch = |steps: usize, at: usize| disc(n, at as f32 / (steps - 1) as f32, steps as u8);
        match self {
            M::Range => switch(s::RANGE, 1),
            M::PwmSource => switch(s::PWM_SOURCE, 0),
            M::SubWave => switch(s::SUB_WAVE, 0),
            M::Trigger => switch(s::TRIGGER, 0),
            M::VcaMode => switch(s::VCA_MODE, 0),
            M::LfoWave => switch(s::LFO_WAVES, 0),
            M::Portamento => switch(s::PORTAMENTO, 0),
            M::ArpOn => switch(2, 0),
            M::ArpMode => switch(s::ARP_MODE, 0),
            M::ArpRange => switch(s::ARP_RANGE, 0),
            M::ArpRate => switch(s::ARP_RATE, 3),
            M::Tune | M::KeyFollow | M::Cutoff | M::ChorusDepth => cont(n, 0.5),
            M::SawLevel => cont(n, 0.8),
            M::Resonance => cont(n, 0.2),
            M::EnvAmount | M::Decay | M::LfoRate => cont(n, 0.4),
            M::Sustain => cont(n, 0.6),
            M::Release => cont(n, 0.2),
            M::PortaTime | M::ChorusRate => cont(n, 0.3),
            M::Volume => cont(n, 0.7),
            M::VcoMod
            | M::PulseWidth
            | M::PulseLevel
            | M::SubLevel
            | M::NoiseLevel
            | M::VcfMod
            | M::VelToVcf
            | M::Attack
            | M::VelToVca
            | M::Drift
            | M::ChorusMix => cont(n, 0.0),
        }
    }
}
```

- [ ] **Step 4: Implement `MonoPatch`** above the tests in `va/patch.rs`:

```rust
//! The Mono synth's knobs in real units. `MonoSynth::set_param` decodes
//! each knob into this struct, so render reads ready-to-use values.

use super::lfo::LfoWave;
use super::osc::SubWave;
use crate::params::{MonoParam, exp, mono_steps, on, step_index};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwmSource {
    Manual,
    Lfo,
    Env,
}

/// What restarts the envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// Every new note, legato or not.
    GateTrig,
    /// Only a note after a gap.
    Gate,
    /// Each LFO cycle while a note is held (and a note after a gap).
    Lfo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Porta {
    Off,
    On,
    /// Glides only between overlapping notes.
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpMode {
    Up,
    UpDown,
    Down,
}

/// Arp step length per `ArpRate` position, in thirds of a sequencer tick
/// (a tick is a 1/64 note): 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32.
pub const ARP_THIRDS: [u32; mono_steps::ARP_RATE] = [48, 24, 16, 12, 8, 6];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonoPatch {
    pub octave_semis: f32,
    pub tune_semis: f32,
    /// LFO → pitch depth at full LFO swing.
    pub vco_mod_semis: f32,
    /// 0 = square; 1 = the thinnest pulse (or full PWM depth).
    pub pw: f32,
    pub pwm_source: PwmSource,
    pub pulse: f32,
    pub saw: f32,
    pub sub: f32,
    pub sub_wave: SubWave,
    pub noise: f32,
    pub cutoff_hz: f32,
    /// Ladder feedback, 0..=4.2.
    pub resonance_k: f32,
    pub env_oct: f32,
    pub vcf_mod_oct: f32,
    /// Cutoff tracking of the note: 1 = full (an octave per octave).
    pub key_follow: f32,
    pub vel_vcf_oct: f32,
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,
    pub trigger: Trigger,
    /// VCA follows the gate instead of the envelope.
    pub vca_gate: bool,
    pub vel_vca: f32,
    pub lfo_wave: LfoWave,
    pub lfo_hz: f32,
    pub porta: Porta,
    /// Glide time constant.
    pub porta_s: f32,
    pub arp_on: bool,
    pub arp_mode: ArpMode,
    pub arp_octaves: u8,
    pub arp_thirds: u32,
    pub drift: f32,
    pub chorus_rate: f32,
    pub chorus_depth: f32,
    pub chorus_mix: f32,
    /// Output gain (the knob squared).
    pub volume: f32,
}

impl Default for MonoPatch {
    fn default() -> Self {
        let mut p = Self {
            octave_semis: 0.0,
            tune_semis: 0.0,
            vco_mod_semis: 0.0,
            pw: 0.0,
            pwm_source: PwmSource::Manual,
            pulse: 0.0,
            saw: 0.0,
            sub: 0.0,
            sub_wave: SubWave::Oct1Square,
            noise: 0.0,
            cutoff_hz: 1000.0,
            resonance_k: 0.0,
            env_oct: 0.0,
            vcf_mod_oct: 0.0,
            key_follow: 0.0,
            vel_vcf_oct: 0.0,
            attack_s: 0.0,
            decay_s: 0.0,
            sustain: 0.0,
            release_s: 0.0,
            trigger: Trigger::GateTrig,
            vca_gate: false,
            vel_vca: 0.0,
            lfo_wave: LfoWave::Triangle,
            lfo_hz: 1.0,
            porta: Porta::Off,
            porta_s: 0.0,
            arp_on: false,
            arp_mode: ArpMode::Up,
            arp_octaves: 1,
            arp_thirds: 12,
            drift: 0.0,
            chorus_rate: 0.0,
            chorus_depth: 0.0,
            chorus_mix: 0.0,
            volume: 0.0,
        };
        for m in MonoParam::ALL {
            p.set(m, m.info().default);
        }
        p
    }
}

impl MonoPatch {
    /// Decodes knob `p` at normalised `v` (clamped to 0..=1).
    pub fn set(&mut self, p: MonoParam, v: f32) {
        use MonoParam as M;
        use mono_steps as s;
        let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let pick = |n: usize| step_index(v, n as u8);
        match p {
            M::Range => self.octave_semis = [-12.0, 0.0, 12.0, 24.0][pick(s::RANGE)],
            M::Tune => self.tune_semis = v - 0.5,
            M::VcoMod => self.vco_mod_semis = v * v * 12.0,
            M::PulseWidth => self.pw = v,
            M::PwmSource => {
                self.pwm_source =
                    [PwmSource::Manual, PwmSource::Lfo, PwmSource::Env][pick(s::PWM_SOURCE)]
            }
            M::PulseLevel => self.pulse = v,
            M::SawLevel => self.saw = v,
            M::SubLevel => self.sub = v,
            M::SubWave => {
                self.sub_wave =
                    [SubWave::Oct1Square, SubWave::Oct2Square, SubWave::Oct2Pulse][pick(s::SUB_WAVE)]
            }
            M::NoiseLevel => self.noise = v,
            M::Cutoff => self.cutoff_hz = exp(v, 20.0, 18_000.0),
            M::Resonance => self.resonance_k = v * 4.2,
            M::EnvAmount => self.env_oct = v * 8.0,
            M::VcfMod => self.vcf_mod_oct = v * 4.0,
            M::KeyFollow => self.key_follow = v,
            M::VelToVcf => self.vel_vcf_oct = v * 3.0,
            M::Attack => self.attack_s = exp(v, 0.0015, 4.0),
            M::Decay => self.decay_s = exp(v, 0.002, 8.0),
            M::Sustain => self.sustain = v,
            M::Release => self.release_s = exp(v, 0.002, 8.0),
            M::Trigger => {
                self.trigger = [Trigger::GateTrig, Trigger::Gate, Trigger::Lfo][pick(s::TRIGGER)]
            }
            M::VcaMode => self.vca_gate = on(v),
            M::VelToVca => self.vel_vca = v,
            M::LfoWave => {
                self.lfo_wave = [
                    LfoWave::Triangle,
                    LfoWave::Square,
                    LfoWave::Random,
                    LfoWave::Noise,
                ][pick(s::LFO_WAVES)]
            }
            M::LfoRate => self.lfo_hz = exp(v, 0.1, 30.0),
            M::Portamento => self.porta = [Porta::Off, Porta::On, Porta::Auto][pick(s::PORTAMENTO)],
            M::PortaTime => self.porta_s = exp(v, 0.001, 1.5),
            M::ArpOn => self.arp_on = on(v),
            M::ArpMode => {
                self.arp_mode = [ArpMode::Up, ArpMode::UpDown, ArpMode::Down][pick(s::ARP_MODE)]
            }
            M::ArpRange => self.arp_octaves = 1 + pick(s::ARP_RANGE) as u8,
            M::ArpRate => self.arp_thirds = ARP_THIRDS[pick(s::ARP_RATE)],
            M::Drift => self.drift = v,
            M::ChorusRate => self.chorus_rate = v,
            M::ChorusDepth => self.chorus_depth = v,
            M::ChorusMix => self.chorus_mix = v,
            M::Volume => self.volume = v * v,
        }
    }
}
```

- [ ] **Step 5: Run the tests.**

Run: `cargo test -p acid-engine va::patch`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit.**

```bash
git add crates/acid-engine/src/params.rs crates/acid-engine/src/va
git commit -m "feat(engine): Mono params and their real-unit patch"
```

---

### Task 6: `MonoSynth` voice and note handling (no arp yet)

**Files:**
- Create: `crates/acid-engine/src/va/synth.rs`
- Modify: `crates/acid-engine/src/va/mod.rs` (add `mod synth; pub use synth::MonoSynth;`)

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces: `va::MonoSynth`, whose methods match `FmSynth` so the engine can route the same way:
  - `new(sample_rate: f32) -> Self`. It allocates the chorus line, so call it off the audio thread.
  - `set_param(&mut self, p: MonoParam, v: f32)`
  - `note_on(&mut self, pitch: u8, vel: u8, live: bool)`, `note_off(&mut self, pitch: u8, live: bool)`
  - `pattern_off(&mut self)`: releases sequencer notes and keeps auditions.
  - `all_off(&mut self)`, `reset(&mut self)`
  - `render(&mut self, left: &mut [f32], right: &mut [f32])`: adds into the buffers, like `FmSynth::render`.
  - `sounding(&self) -> Option<u8>`: the gated pitch, if any.

- [ ] **Step 1: Write the failing tests** at the bottom of `va/synth.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MonoParam as M;
    use crate::testutil::{all_finite, peak, rms};
    use crate::va::adsr::Stage;

    const SR: f32 = 48_000.0;

    fn render(s: &mut MonoSynth, n: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        s.render(&mut l, &mut r);
        l
    }

    #[test]
    fn silent_until_a_note_then_sounding() {
        let mut s = MonoSynth::new(SR);
        assert!(render(&mut s, 1000).iter().all(|v| *v == 0.0));
        s.note_on(48, 100, false);
        let y = render(&mut s, 4800);
        assert!(all_finite(&y));
        assert!(rms(&y) > 0.01 && peak(&y) < 1.5, "rms {} peak {}", rms(&y), peak(&y));
    }

    #[test]
    fn the_last_note_wins_and_releasing_falls_back() {
        let mut s = MonoSynth::new(SR);
        s.note_on(48, 100, false);
        s.note_on(60, 100, false);
        assert_eq!(s.sounding(), Some(60));
        s.note_off(48, false);
        assert_eq!(s.sounding(), Some(60), "a held-under note leaves the sound alone");
        s.note_on(48, 100, false);
        s.note_off(48, false);
        assert_eq!(s.sounding(), Some(60));
        s.note_off(60, false);
        assert_eq!(s.sounding(), None);
        assert_eq!(s.env.stage(), Stage::Release);
    }

    #[test]
    fn auto_portamento_glides_only_between_overlapping_notes() {
        let mut s = MonoSynth::new(SR);
        s.set_param(M::Portamento, 1.0);
        s.set_param(M::PortaTime, 1.0);
        s.note_on(48, 100, false);
        render(&mut s, 100);
        s.note_on(60, 100, false);
        render(&mut s, 100);
        assert!(s.pitch < 59.0, "gliding: {}", s.pitch);
        s.all_off();
        s.note_on(48, 100, false);
        assert_eq!(s.pitch, 48.0, "after a gap: no glide");
    }

    #[test]
    fn gate_mode_does_not_retrigger_legato_notes() {
        for (trigger, legato_stage) in [(0.0, Stage::Attack), (0.5, Stage::Sustain)] {
            let mut s = MonoSynth::new(SR);
            s.set_param(M::Trigger, trigger);
            s.set_param(M::Attack, 0.0);
            s.set_param(M::Decay, 0.0);
            s.note_on(48, 100, false);
            render(&mut s, 4800);
            assert_eq!(s.env.stage(), Stage::Sustain);
            s.note_on(50, 100, false);
            assert_eq!(s.env.stage(), legato_stage, "trigger {trigger}");
        }
    }

    #[test]
    fn drift_zero_is_exactly_repeatable_and_drift_changes_the_sound() {
        let play = |drift: f32| {
            let mut s = MonoSynth::new(SR);
            s.set_param(M::Drift, drift);
            s.note_on(48, 100, false);
            render(&mut s, 9600)
        };
        assert_eq!(play(0.0), play(0.0));
        assert_ne!(play(0.0), play(1.0));
    }

    #[test]
    fn pattern_off_keeps_auditions() {
        let mut s = MonoSynth::new(SR);
        s.note_on(48, 100, true);
        s.note_on(60, 100, false);
        s.pattern_off();
        assert_eq!(s.sounding(), Some(48));
        s.note_off(48, false);
        assert_eq!(s.sounding(), Some(48), "a pattern NoteOff never releases an audition");
    }

    #[test]
    fn every_knob_at_both_ends_stays_finite_and_bounded() {
        for m in M::ALL {
            for v in [0.0, 1.0] {
                let mut s = MonoSynth::new(SR);
                s.set_param(M::SawLevel, 1.0);
                s.set_param(M::PulseLevel, 1.0);
                s.set_param(M::SubLevel, 1.0);
                s.set_param(M::NoiseLevel, 1.0);
                s.set_param(m, v);
                s.note_on(48, 127, false);
                let y = render(&mut s, 9600);
                assert!(all_finite(&y), "{m:?}={v}");
                assert!(peak(&y) < 4.0, "{m:?}={v}: {}", peak(&y));
            }
        }
    }
}
```

`crate::testutil` already has `rms`, `peak` and `all_finite` (see `crates/acid-engine/src/lib.rs`).

- [ ] **Step 2: Run them to verify they fail.**

Run: `cargo test -p acid-engine va::synth`
Expected: FAIL to compile.

- [ ] **Step 3: Implement** above the tests in `va/synth.rs`:

```rust
//! The Mono machine's voice: one VCO + sub + noise through the ladder
//! and VCA, with last-note priority over a stack of held notes, the
//! 101-style trigger and portamento modes, drift and a chorus.

use super::adsr::Adsr;
use super::ladder::Ladder;
use super::lfo::{Drift, Lfo, Noise};
use super::osc::Vco;
use super::patch::{MonoPatch, Porta, PwmSource, Trigger};
use crate::fx::Chorus;
use crate::params::MonoParam;

/// Held notes remembered for fallback (and the arp).
pub(super) const MAX_HELD: usize = 16;
/// Mixer sum → filter input scale, leaving headroom before the tanh.
const MIX_LEVEL: f32 = 0.5;
const OUT_LEVEL: f32 = 0.6;
/// Drift at full: slow pitch wander, per-note offset, cutoff wander.
const DRIFT_SEMIS: f32 = 0.08;
const DRIFT_NOTE_SEMIS: f32 = 0.03;
const DRIFT_CUTOFF_OCT: f32 = 0.15;
/// Gate-mode VCA ramp, so on/off doesn't click.
const VCA_RAMP_S: f32 = 0.002;

fn midi_hz(semis: f32) -> f32 {
    440.0 * ((semis - 69.0) / 12.0).exp2()
}

#[derive(Clone, Copy, Default)]
struct Held {
    pitch: u8,
    vel: u8,
    /// An audition (piano-roll key), not a sequencer note.
    live: bool,
}

pub struct MonoSynth {
    sample_rate: f32,
    patch: MonoPatch,
    vco: Vco,
    filter: Ladder,
    pub(super) env: Adsr,
    lfo: Lfo,
    drift: Drift,
    noise: Noise,
    chorus: Chorus,
    held: [Held; MAX_HELD],
    held_len: usize,
    /// The gated pitch.
    sounding: Option<u8>,
    gate: bool,
    /// Current (gliding) pitch and where it is going, in MIDI semitones.
    pub(super) pitch: f32,
    target: f32,
    /// A pitch has been played since the last reset (portamento On glides from it).
    has_pitch: bool,
    note_offset: f32,
    /// Velocity of the sounding note, 0..=1.
    vel: f32,
    vca: f32,
    vca_coef: f32,
    glide_coef: f32,
}

impl MonoSynth {
    /// Allocates the chorus line: call off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            sample_rate,
            patch: MonoPatch::default(),
            vco: Vco::new(),
            filter: Ladder::new(sample_rate),
            env: Adsr::new(sample_rate),
            lfo: Lfo::new(sample_rate),
            drift: Drift::new(sample_rate),
            noise: Noise::new(0x2545_f491),
            chorus: Chorus::new(sample_rate),
            held: [Held::default(); MAX_HELD],
            held_len: 0,
            sounding: None,
            gate: false,
            pitch: 48.0,
            target: 48.0,
            has_pitch: false,
            note_offset: 0.0,
            vel: 1.0,
            vca: 0.0,
            vca_coef: 1.0 - (-1.0 / (VCA_RAMP_S * sample_rate)).exp(),
            glide_coef: 1.0,
        };
        s.apply_patch();
        s
    }

    pub fn set_param(&mut self, p: MonoParam, v: f32) {
        self.patch.set(p, v);
        self.apply_patch();
    }

    fn apply_patch(&mut self) {
        let p = &self.patch;
        self.env.set(p.attack_s, p.decay_s, p.sustain, p.release_s);
        self.glide_coef = 1.0 - (-1.0 / (p.porta_s * self.sample_rate)).exp();
    }

    pub fn sounding(&self) -> Option<u8> {
        self.sounding
    }

    fn top(&self) -> Option<Held> {
        self.held[..self.held_len].last().copied()
    }

    fn remove_at(&mut self, i: usize) {
        self.held.copy_within(i + 1..self.held_len, i);
        self.held_len -= 1;
    }

    pub fn note_on(&mut self, pitch: u8, vel: u8, live: bool) {
        if let Some(i) = self.held[..self.held_len].iter().position(|h| h.pitch == pitch) {
            self.remove_at(i);
        }
        if self.held_len == MAX_HELD {
            self.remove_at(0);
        }
        self.held[self.held_len] = Held { pitch, vel, live };
        self.held_len += 1;
        self.sound(pitch, vel);
    }

    /// Releases `pitch` if it was started the same way (`live` or not).
    pub fn note_off(&mut self, pitch: u8, live: bool) {
        let found = self.held[..self.held_len]
            .iter()
            .position(|h| h.pitch == pitch && h.live == live);
        if let Some(i) = found {
            self.remove_at(i);
            self.follow_held();
        }
    }

    /// Sequencer notes off; auditions keep sounding.
    pub fn pattern_off(&mut self) {
        let mut kept = 0;
        for i in 0..self.held_len {
            if self.held[i].live {
                self.held[kept] = self.held[i];
                kept += 1;
            }
        }
        self.held_len = kept;
        self.follow_held();
    }

    pub fn all_off(&mut self) {
        self.held_len = 0;
        self.follow_held();
    }

    /// Clears notes, filter and modulation state (NaN guard, hard stop).
    pub fn reset(&mut self) {
        self.held_len = 0;
        self.sounding = None;
        self.gate = false;
        self.has_pitch = false;
        self.vca = 0.0;
        self.vco.reset();
        self.filter.reset();
        self.env.reset();
        self.lfo.reset();
        self.drift.reset();
        self.chorus.reset();
    }

    /// After the held set changed: sound the newest held note (legato) or
    /// close the gate.
    fn follow_held(&mut self) {
        match self.top() {
            Some(h) if self.sounding != Some(h.pitch) => self.sound(h.pitch, h.vel),
            Some(_) => {}
            None => self.close_gate(),
        }
    }

    fn sound(&mut self, pitch: u8, vel: u8) {
        let legato = self.gate;
        let glide = match self.patch.porta {
            Porta::Off => false,
            Porta::On => self.has_pitch,
            Porta::Auto => legato,
        };
        self.target = pitch as f32;
        if !glide {
            self.pitch = self.target;
        }
        self.has_pitch = true;
        self.sounding = Some(pitch);
        self.vel = vel as f32 / 127.0;
        self.note_offset = self.patch.drift * DRIFT_NOTE_SEMIS * self.drift.note_offset();
        if !legato || self.patch.trigger == Trigger::GateTrig {
            self.env.gate_on();
        }
        self.gate = true;
    }

    fn close_gate(&mut self) {
        self.gate = false;
        self.sounding = None;
        self.env.gate_off();
    }

    /// Adds the machine's output into `left`/`right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        for i in 0..n {
            let (l, r) = self.sample();
            left[i] += l;
            right[i] += r;
        }
    }

    fn sample(&mut self) -> (f32, f32) {
        let p = self.patch;
        let (lfo, wrapped) = self.lfo.next(p.lfo_wave, p.lfo_hz);
        if wrapped && self.gate && p.trigger == Trigger::Lfo {
            self.env.gate_on();
        }
        let env = self.env.next();
        let [drift_pitch, drift_cutoff] = self.drift.next();
        self.pitch += (self.target - self.pitch) * self.glide_coef;
        let semis = self.pitch
            + p.octave_semis
            + p.tune_semis
            + p.vco_mod_semis * lfo
            + p.drift * DRIFT_SEMIS * drift_pitch
            + self.note_offset;
        let pwm = match p.pwm_source {
            PwmSource::Manual => p.pw,
            PwmSource::Lfo => p.pw * 0.5 * (1.0 + lfo),
            PwmSource::Env => p.pw * env,
        };
        let o = self
            .vco
            .next(midi_hz(semis) / self.sample_rate, 0.5 - 0.45 * pwm, p.sub_wave);
        let mix = (o.pulse * p.pulse
            + o.saw * p.saw
            + o.sub * p.sub
            + self.noise.next() * p.noise)
            * MIX_LEVEL;
        let octaves = p.env_oct * env
            + p.vcf_mod_oct * lfo
            + p.key_follow * (self.pitch - 48.0) / 12.0
            + p.vel_vcf_oct * (self.vel * 2.0 - 1.0)
            + p.drift * DRIFT_CUTOFF_OCT * drift_cutoff;
        self.filter.set(p.cutoff_hz * octaves.exp2(), p.resonance_k);
        let y = self.filter.process(mix);
        if p.vca_gate {
            let to = if self.gate { 1.0 } else { 0.0 };
            self.vca += (to - self.vca) * self.vca_coef;
            if !self.gate && self.vca < 1e-5 {
                self.vca = 0.0;
            }
        } else {
            self.vca = env;
        }
        let vel_gain = 1.0 - p.vel_vca * (1.0 - self.vel);
        let x = y * self.vca * vel_gain * p.volume * OUT_LEVEL;
        if p.chorus_mix > 0.0 {
            let (wl, wr) = self.chorus.process(x, p.chorus_rate, p.chorus_depth);
            (x + (wl - x) * p.chorus_mix, x + (wr - x) * p.chorus_mix)
        } else {
            (x, x)
        }
    }
}
```

In `va/mod.rs` add:

```rust
mod synth;

pub use synth::MonoSynth;
```

- [ ] **Step 4: Run the tests.**

Run: `cargo test -p acid-engine va::`
Expected: PASS. If `every_knob_at_both_ends_stays_finite_and_bounded` fails on the peak bound, report the param and peak value; don't just raise the bound.

- [ ] **Step 5: Commit.**

```bash
git add crates/acid-engine/src/va
git commit -m "feat(engine): Mono voice with last-note priority, trigger modes, glide and drift"
```

---

### Task 7: Arpeggiator

**Files:**
- Create: `crates/acid-engine/src/va/arp.rs`
- Modify: `crates/acid-engine/src/va/mod.rs` (add `pub mod arp;`), `crates/acid-engine/src/va/synth.rs`

**Interfaces:**
- Consumes: `ArpMode`, `ARP_THIRDS` (Task 5), `MonoSynth` (Task 6).
- Produces:
  - `va::arp::{Arp, ArpEvent}`:
    - `Arp::new()`
    - `set_notes(&mut self, held: &[u8], octaves: u8)`, `is_empty(&self) -> bool`
    - `next_pitch(&mut self, mode: ArpMode) -> Option<u8>`, `restart(&mut self)`
    - `clock_tick(&mut self, tick: u64, tick_len: f64, thirds: u32)`, `stop_transport(&mut self)`
    - `set_bpm(&mut self, bpm: f32, sample_rate: f32)`, `start_free(&mut self)`
    - `advance(&mut self, thirds: u32) -> ArpEvent`, `steps(&self) -> u64`
  - New `MonoSynth` methods for the engine (Task 8):
    - `clock_tick(&mut self, tick: u64, tick_len: f64)`
    - `stop_transport(&mut self)`
    - `set_bpm(&mut self, bpm: f32)`
    - `arp_steps(&self) -> u64`

- [ ] **Step 1: Write the failing arp tests** at the bottom of `va/arp.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ArpMode::*;

    fn take(a: &mut Arp, mode: ArpMode, n: usize) -> Vec<u8> {
        (0..n).map(|_| a.next_pitch(mode).unwrap()).collect()
    }

    fn arp(notes: &[u8], octaves: u8) -> Arp {
        let mut a = Arp::new();
        a.set_notes(notes, octaves);
        a
    }

    #[test]
    fn up_down_and_up_down_orders() {
        assert_eq!(take(&mut arp(&[60, 48, 55], 1), Up, 4), [48, 55, 60, 48]);
        assert_eq!(take(&mut arp(&[60, 48, 55], 1), Down, 4), [60, 55, 48, 60]);
        assert_eq!(take(&mut arp(&[48, 55, 60], 1), UpDown, 6), [48, 55, 60, 55, 48, 55]);
        assert_eq!(take(&mut arp(&[48], 1), UpDown, 2), [48, 48]);
        assert_eq!(arp(&[], 1).next_pitch(Up), None);
    }

    #[test]
    fn range_adds_octaves() {
        assert_eq!(take(&mut arp(&[48, 55], 2), Up, 5), [48, 55, 60, 67, 48]);
        assert_eq!(take(&mut arp(&[120], 3), Up, 2), [120, 120], "past 127 is dropped");
    }

    #[test]
    fn a_chord_change_keeps_the_place() {
        let mut a = arp(&[48, 55, 60], 1);
        take(&mut a, Up, 2);
        a.set_notes(&[48, 55, 60, 64], 1);
        assert_eq!(a.next_pitch(Up), Some(60));
    }

    #[test]
    fn sixteenths_and_triplets_land_on_the_tick_grid() {
        let mut a = Arp::new();
        let at = |a: &mut Arp, tick: u64, thirds: u32| {
            a.clock_tick(tick, 300.0, thirds);
            a.next_in
        };
        assert_eq!(at(&mut a, 0, 12), Some(0.0));
        assert_eq!(at(&mut a, 1, 12), None);
        assert_eq!(at(&mut a, 4, 12), Some(0.0));
        // 1/16T: steps at 0, 8/3 and 16/3 ticks.
        assert_eq!(at(&mut a, 1, 8), None);
        assert_eq!(at(&mut a, 2, 8), Some(200.0));
        assert_eq!(at(&mut a, 5, 8), Some(100.0));
    }

    #[test]
    fn clocked_steps_fire_on_time_and_gate_for_half_a_step() {
        let mut a = arp(&[48], 1);
        a.clock_tick(0, 100.0, 12);
        assert_eq!(a.advance(12), ArpEvent::Step);
        let gate_off = (1..400).find(|_| a.advance(12) == ArpEvent::GateOff);
        assert_eq!(gate_off, Some(200));
        assert_eq!(a.steps(), 1);
    }

    #[test]
    fn stopped_it_free_runs_from_the_tempo() {
        let mut a = arp(&[48], 1);
        a.set_bpm(120.0, 48_000.0);
        a.start_free();
        assert_eq!(a.advance(12), ArpEvent::Step);
        // 1/16 at 120 BPM is 6000 samples.
        let next = (1..7000).find(|_| a.advance(12) == ArpEvent::Step);
        assert_eq!(next, Some(6000));
    }
}
```

- [ ] **Step 2: Run them to verify they fail.**

Run: `cargo test -p acid-engine va::arp`
Expected: FAIL to compile.

- [ ] **Step 3: Implement** above the tests in `va/arp.rs`:

```rust
//! The arpeggiator: which note plays next (Up, Up & Down, Down over 1–3
//! octaves of the held notes) and when. While the transport runs, steps
//! fall on the song's tick grid (the engine reports each tick); stopped,
//! it free-runs from the tempo. Step lengths are in thirds of a tick, so
//! triplets land exactly.

use super::patch::ArpMode;
use super::synth::MAX_HELD;

const MAX_NOTES: usize = MAX_HELD * 3;
/// Sequencer ticks per beat (4 per 1/16 step).
const TICKS_PER_BEAT: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpEvent {
    Idle,
    /// Play the next note now.
    Step,
    /// The current arp note's gate closes now.
    GateOff,
}

pub struct Arp {
    /// Held pitches, ascending, repeated up `octaves` octaves.
    notes: [u8; MAX_NOTES],
    len: usize,
    pos: usize,
    transport: bool,
    /// Samples until the step scheduled in the current tick.
    pub(super) next_in: Option<f64>,
    tick_len: f64,
    free_tick_len: f64,
    free_left: f64,
    gate_left: f64,
    steps: u64,
}

impl Default for Arp {
    fn default() -> Self {
        Self::new()
    }
}

impl Arp {
    pub fn new() -> Self {
        Self {
            notes: [0; MAX_NOTES],
            len: 0,
            pos: 0,
            transport: false,
            next_in: None,
            tick_len: 0.0,
            free_tick_len: 1500.0,
            free_left: 0.0,
            gate_left: 0.0,
            steps: 0,
        }
    }

    /// Rebuilds the sequence from the held pitches (any order, no
    /// duplicates), keeping the current place in it.
    pub fn set_notes(&mut self, held: &[u8], octaves: u8) {
        let mut base = [0u8; MAX_HELD];
        let n = held.len().min(MAX_HELD);
        base[..n].copy_from_slice(&held[..n]);
        base[..n].sort_unstable();
        self.len = 0;
        for oct in 0..octaves.max(1) {
            for &p in &base[..n] {
                let q = p as u16 + 12 * oct as u16;
                if q <= 127 && self.len < MAX_NOTES {
                    self.notes[self.len] = q as u8;
                    self.len += 1;
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn restart(&mut self) {
        self.pos = 0;
    }

    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// The next pitch in `mode`, advancing; `None` with no notes.
    pub fn next_pitch(&mut self, mode: ArpMode) -> Option<u8> {
        let len = self.len;
        if len == 0 {
            return None;
        }
        let period = match mode {
            ArpMode::UpDown if len >= 2 => 2 * len - 2,
            _ => len,
        };
        let i = self.pos % period;
        self.pos = (i + 1) % period;
        Some(match mode {
            ArpMode::Up => self.notes[i],
            ArpMode::Down => self.notes[len - 1 - i],
            ArpMode::UpDown if i < len => self.notes[i],
            ArpMode::UpDown => self.notes[2 * len - 2 - i],
        })
    }

    /// Sequencer tick number `tick` (`tick_len` samples long) starts now:
    /// schedules the step boundary inside it, if there is one.
    pub fn clock_tick(&mut self, tick: u64, tick_len: f64, thirds: u32) {
        self.transport = true;
        self.tick_len = tick_len;
        let t3 = tick * 3;
        let d = thirds.max(1) as u64;
        let off = t3.div_ceil(d) * d - t3;
        self.next_in = (off < 3).then(|| off as f64 / 3.0 * tick_len);
    }

    pub fn stop_transport(&mut self) {
        self.transport = false;
        self.next_in = None;
    }

    pub fn set_bpm(&mut self, bpm: f32, sample_rate: f32) {
        self.free_tick_len = 60.0 / bpm.max(1.0) as f64 / TICKS_PER_BEAT * sample_rate as f64;
    }

    /// Transport stopped and the first note just arrived: step on the next
    /// sample, then every step length after it.
    pub fn start_free(&mut self) {
        self.free_left = 1.0;
    }

    /// One sample on.
    pub fn advance(&mut self, thirds: u32) -> ArpEvent {
        let tick = if self.transport { self.tick_len } else { self.free_tick_len };
        let step_len = thirds as f64 / 3.0 * tick;
        let step = if self.transport {
            match self.next_in {
                Some(n) if n <= 0.5 => {
                    self.next_in = None;
                    true
                }
                Some(n) => {
                    self.next_in = Some(n - 1.0);
                    false
                }
                None => false,
            }
        } else {
            self.free_left -= 1.0;
            let due = self.free_left <= 0.0;
            if due {
                self.free_left += step_len;
            }
            due
        };
        if step {
            self.steps += 1;
            self.gate_left = step_len * 0.5;
            return ArpEvent::Step;
        }
        if self.gate_left > 0.0 {
            self.gate_left -= 1.0;
            if self.gate_left <= 0.0 {
                return ArpEvent::GateOff;
            }
        }
        ArpEvent::Idle
    }
}
```

- [ ] **Step 4: Run the arp tests.**

Run: `cargo test -p acid-engine va::arp`
Expected: PASS (6 tests).

- [ ] **Step 5: Write the failing `MonoSynth` arp tests.** Add them to the test module in `va/synth.rs`:

```rust
    #[test]
    fn the_arp_steps_through_held_notes_from_the_tempo_when_stopped() {
        let mut s = MonoSynth::new(SR);
        s.set_bpm(120.0);
        s.set_param(M::ArpOn, 1.0);
        for p in [55, 48, 52] {
            s.note_on(p, 100, true);
        }
        let mut seen = Vec::new();
        for _ in 0..3 {
            render(&mut s, 1);
            seen.push(s.sounding());
            render(&mut s, 5999);
        }
        assert_eq!(seen, [Some(48), Some(52), Some(55)]);
        assert_eq!(s.arp_steps(), 3);
    }

    #[test]
    fn releasing_every_key_stops_the_arp() {
        let mut s = MonoSynth::new(SR);
        s.set_param(M::ArpOn, 1.0);
        s.note_on(48, 100, true);
        render(&mut s, 100);
        s.note_off(48, true);
        assert_eq!(s.env.stage(), Stage::Release);
        render(&mut s, 48_000);
        assert_eq!(s.arp_steps(), 1);
    }

    #[test]
    fn turning_the_arp_off_plays_the_newest_held_note() {
        let mut s = MonoSynth::new(SR);
        s.set_param(M::ArpOn, 1.0);
        s.note_on(48, 100, true);
        s.note_on(60, 100, true);
        render(&mut s, 10);
        s.set_param(M::ArpOn, 0.0);
        assert_eq!(s.sounding(), Some(60));
    }

    #[test]
    fn clocked_arp_waits_for_the_grid() {
        let mut s = MonoSynth::new(SR);
        s.set_param(M::ArpOn, 1.0);
        s.clock_tick(1, 100.0);
        s.note_on(48, 100, false);
        render(&mut s, 250);
        assert_eq!(s.arp_steps(), 0, "tick 1 has no 1/16 boundary");
        s.clock_tick(4, 100.0);
        render(&mut s, 1);
        assert_eq!(s.arp_steps(), 1);
        assert_eq!(s.sounding(), Some(48));
    }
```

- [ ] **Step 6: Run them to verify they fail.**

Run: `cargo test -p acid-engine va::synth`
Expected: FAIL to compile (`set_bpm`, `arp_steps` and `clock_tick` don't exist yet).

- [ ] **Step 7: Wire the arp into `MonoSynth`** (`va/synth.rs`):

  1. Imports: add `use super::arp::{Arp, ArpEvent};`.
  2. Add the field `arp: Arp,` to `MonoSynth`, and `arp: Arp::new(),` in `new`. After `s.apply_patch();` in `new`, add `s.arp.set_bpm(120.0, sample_rate);`.
  3. Add the engine-facing methods:

```rust
    /// Sequencer tick `tick` (`tick_len` samples) starts now (transport running).
    pub fn clock_tick(&mut self, tick: u64, tick_len: f64) {
        self.arp.clock_tick(tick, tick_len, self.patch.arp_thirds);
    }

    pub fn stop_transport(&mut self) {
        self.arp.stop_transport();
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.arp.set_bpm(bpm, self.sample_rate);
    }

    /// Arp steps played since construction.
    pub fn arp_steps(&self) -> u64 {
        self.arp.steps()
    }

    /// Rebuilds the arp's notes from the held stack; a first note restarts
    /// the pattern (and, stopped, steps at once).
    fn arp_follow_held(&mut self) {
        let was_empty = self.arp.is_empty();
        let mut pitches = [0u8; MAX_HELD];
        for (p, h) in pitches.iter_mut().zip(&self.held[..self.held_len]) {
            *p = h.pitch;
        }
        self.arp.set_notes(&pitches[..self.held_len], self.patch.arp_octaves);
        if self.arp.is_empty() {
            self.close_gate();
        } else if was_empty {
            self.arp.restart();
            self.arp.start_free();
        }
    }
```

  4. In `note_on`, replace the final `self.sound(pitch, vel);` with:

```rust
        if self.patch.arp_on {
            self.arp_follow_held();
        } else {
            self.sound(pitch, vel);
        }
```

  5. In `follow_held`, start with:

```rust
        if self.patch.arp_on {
            self.arp_follow_held();
            return;
        }
```

  6. In `set_param`, after `self.apply_patch();`:

```rust
        match p {
            MonoParam::ArpOn => {
                // Off → on starts the pattern afresh; on → off re-sounds the
                // newest held note (forgetting what the arp last played).
                self.arp.set_notes(&[], 1);
                self.sounding = None;
                self.follow_held();
            }
            MonoParam::ArpRange if self.patch.arp_on => self.arp_follow_held(),
            _ => {}
        }
```

  7. In `reset`, add `self.arp.set_notes(&[], 1);`.

  8. In `render`, run the arp before each sample:

```rust
        for i in 0..n {
            if self.patch.arp_on && !self.arp.is_empty() {
                match self.arp.advance(self.patch.arp_thirds) {
                    ArpEvent::Step => {
                        let vel = self.top().map_or(100, |h| h.vel);
                        if let Some(p) = self.arp.next_pitch(self.patch.arp_mode) {
                            // Each arp note is a fresh note: the envelope retriggers.
                            self.gate = false;
                            self.sound(p, vel);
                        }
                    }
                    ArpEvent::GateOff => self.close_gate(),
                    ArpEvent::Idle => {}
                }
            }
            let (l, r) = self.sample();
            left[i] += l;
            right[i] += r;
        }
```

- [ ] **Step 8: Run all va tests.**

Run: `cargo test -p acid-engine va::`
Expected: PASS.

- [ ] **Step 9: Commit.**

```bash
git add crates/acid-engine/src/va
git commit -m "feat(engine): Mono arpeggiator, Up / U&D / Down over 1-3 octaves, tick-synced"
```

---

### Task 8: Put Mono in the rack (engine, project, file format, app plumbing)

**Files:**
- Modify (engine):
  - `crates/acid-engine/src/machine.rs`
  - `crates/acid-engine/src/params.rs`
  - `crates/acid-engine/src/project.rs`
  - `crates/acid-engine/src/rack.rs`
  - `crates/acid-engine/src/engine.rs`
  - `crates/acid-engine/src/sequencer.rs`
  - `crates/acid-engine/src/song.rs`
  - `crates/acid-engine/tests/no_alloc.rs`
- Modify (app):
  - `crates/app/src/ui/mod.rs`
  - `crates/app/src/ui/files.rs`
  - `crates/app/src/ui/rack_header.rs` (test)
- Create: `crates/app/src/ui/mono_panel.rs` (a minimal panel, filled in by Task 10)

**Interfaces:**
- Consumes: `MonoSynth` and its methods (Tasks 6–7), `MonoParam` (Task 5).
- Produces:
  - `MachineType::Mono`
  - `MachineParam::Mono(MonoParam)`
  - `impl From<MonoParam> for MachineParam`
  - `project::MonoMachine { patterns: Vec<NotePattern>, patch_name: String }`
  - `MachineBody::Mono(MonoMachine)`
  - `Project::mono(uid) -> Option<&MonoMachine>` and `Project::mono_mut`
  - `rack::MachineDsp::Mono(Box<MonoSynth>)`
  - `Sequencer::tick_clock(&self) -> (u64, f64)`
  - `PROJECT_VERSION = 4`
  - `SidAcidApp::mono_panel(&mut self, ui, uid)`

- [ ] **Step 1: Write the failing engine-side tests.**

In `crates/acid-engine/src/params.rs` tests:

```rust
    #[test]
    fn mono_params_fit_only_mono_machines() {
        use crate::machine::MachineType as T;
        let p = MachineParam::Mono(MonoParam::Cutoff);
        assert!(p.fits(T::Mono));
        assert!(!p.fits(T::Fm));
        assert!(!MachineParam::Fm(FmParam::Volume).fits(T::Mono));
        let all = MachineParam::all_for(T::Mono);
        assert_eq!(all.len(), MachineParam::count_for(T::Mono));
        for (i, q) in all.iter().enumerate() {
            assert_eq!(q.index_for(T::Mono), Some(i), "{q:?}");
        }
        assert_eq!(p.info(), MonoParam::Cutoff.info());
    }
```

In `crates/acid-engine/src/project.rs` tests:

```rust
    #[test]
    fn a_project_with_a_mono_machine_round_trips() {
        let mut p = Project::default();
        let m = p.new_machine(MachineType::Mono);
        let uid = m.uid;
        p.machines.push(m);
        p.mono_mut(uid).unwrap().patch_name = "Squelch".into();
        p.validate().unwrap();
        let back = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.mono(uid).unwrap().patterns.len(), PATTERNS);
    }

    #[test]
    fn version_3_files_still_load() {
        let text = Project::default().to_ron().replace("version: 4", "version: 3");
        let p = Project::from_ron(&text).unwrap();
        assert_eq!(p.version, PROJECT_VERSION);
    }
```

In `crates/acid-engine/src/engine.rs` tests:

```rust
    #[test]
    fn a_mono_arp_plays_sixteen_sixteenths_in_a_bar() {
        use crate::params::{MachineParam, MonoParam};
        use crate::project::{MachineBody, Note};
        let mut p = Project::default();
        p.bpm = 120.0;
        let mut m = p.new_machine(MachineType::Mono);
        m.params.insert(MachineParam::Mono(MonoParam::ArpOn), 1.0);
        if let MachineBody::Mono(mono) = &mut m.body {
            let pat = &mut mono.patterns[0];
            pat.length = 16;
            pat.notes = [48, 52, 55]
                .map(|pitch| Note { start: 0, len: 64, pitch, vel: 100 })
                .to_vec();
        }
        let uid = m.uid;
        p.machines.push(m);
        let mut eng = Engine::from_project(SR, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        // 120 BPM at 48 kHz: a 1/16 is 6000 samples, the bar 96000 = 1500 blocks.
        for _ in 0..1500 {
            eng.process(&mut l, &mut r);
        }
        let pos = eng.position(uid).unwrap();
        let MachineDsp::Mono(mono) = &eng.slots[pos].dsp else {
            panic!("not a Mono slot")
        };
        assert_eq!(mono.arp_steps(), 16);
    }
```

Before writing it, check the engine test module's `SR` constant and that `Engine::position` exists, using `grep -n "const SR\|fn position" crates/acid-engine/src/engine.rs`. If `SR` isn't 48 000, change the block count to `(SR as usize * 2) / 64` (one bar at 120 BPM is 2 s) and keep the 16.

- [ ] **Step 2: Run them to verify they fail.**

Run: `cargo test -p acid-engine mono`
Expected: FAIL to compile (`MachineType::Mono` missing).

- [ ] **Step 3: `machine.rs`.** Add `Mono` as the last variant of `MachineType`. Make `ALL` `[MachineType; 6]` with `MachineType::Mono` appended. Add `MachineType::Mono => "Mono"` to both `label` and `stem`. Make `transposes` `matches!(self, MachineType::Synth | MachineType::Fm | MachineType::Mono)`.

- [ ] **Step 4: `params.rs`.**
  - Add `Mono(MonoParam),` as the last variant of `MachineParam`.
  - `info`: `MachineParam::Mono(p) => p.info(),`
  - `fits`: `MachineParam::Mono(_) => ty == MachineType::Mono,`
  - `index_for`: `MachineParam::Mono(p) => base + p as usize,`
  - `all_for`: `MachineType::Mono => v.extend(MonoParam::ALL.iter().map(|&p| MachineParam::Mono(p))),`
  - `count_for`: `MachineType::Mono => MonoParam::ALL.len(),`
  - Add `MonoParam => Mono,` to the `machine_param_from!` list.

- [ ] **Step 5: `project.rs`.**
  - Set `pub const PROJECT_VERSION: u32 = 4;`.
  - After `FmMachine`'s `Default` impl, add:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MonoMachine {
    pub patterns: Vec<NotePattern>,
    /// Name of the patch the Mono params were last loaded from.
    #[serde(default = "init_patch_name")]
    pub patch_name: String,
}

fn init_patch_name() -> String {
    "Init".into()
}

impl Default for MonoMachine {
    fn default() -> Self {
        Self {
            patterns: vec![NotePattern::default(); PATTERNS],
            patch_name: init_patch_name(),
        }
    }
}
```

  - Add `Mono(MonoMachine),` to `MachineBody`, `MachineType::Mono => MachineBody::Mono(MonoMachine::default()),` to `MachineBody::new`, and `MachineBody::Mono(_) => MachineType::Mono,` to `machine_type`.
  - Add `body_accessors!(mono, mono_mut, Mono, MonoMachine);`.
  - In `note_patterns` and `note_patterns_mut`, add `MachineBody::Mono(m) =>` to the Fm/Sampler arms. Update their doc comment to "Note patterns of an FM, Mono or Sampler machine."
  - In `validate`'s pattern count, add `MachineBody::Mono(m) => m.patterns.len(),`. In the per-body check, add:

```rust
            MachineBody::Mono(m) => {
                if let Some(j) = m.patterns.iter().position(|p| !p.is_well_formed()) {
                    return bad(format!("pattern {}", label(j)));
                }
            }
```

  - In `from_ron`, replace the `PROJECT_VERSION =>` arm with:

```rust
            // v4 only added the Mono machine: a v3 file reads as it is.
            3 | PROJECT_VERSION => {
                let mut p = with_pcm_budget(|| ron::from_str::<Project>(text)).map_err(parse)?;
                p.version = PROJECT_VERSION;
                p
            }
```

  - If `migrate.rs` (or a test) hard-codes the number 3 as "current", the compile and test run will show it. Update it to `PROJECT_VERSION`.

- [ ] **Step 6: `rack.rs`.**
  - `use crate::va::MonoSynth;`
  - Add `Mono(Box<MonoSynth>),` to `MachineDsp`.
  - In `Slot::from_entry`: `MachineBody::Mono(m) => (MachineDsp::Mono(Box::new(MonoSynth::new(sample_rate))), SlotPatterns::Notes(m.patterns.clone())),`
  - In `apply`: `(MachineParam::Mono(p), MachineDsp::Mono(m)) => m.set_param(p, v),`
  - In `render`: `MachineDsp::Mono(m) => m.render(l, r),`
  - In `reset`: `MachineDsp::Mono(m) => m.reset(),`

- [ ] **Step 7: `song.rs`.** In `Banks::of`, add `MachineBody::Mono(x) => Bank::Notes(&x.patterns),`.

- [ ] **Step 8: `sequencer.rs`.** Add, after `samples_to_next_tick`:

```rust
    /// The tick about to run (its number since play, at 4 per step) and its
    /// length in samples, swing included. The Mono arp clocks from these.
    pub fn tick_clock(&self) -> (u64, f64) {
        (
            self.global_step * SUBS as u64 + self.sub as u64,
            self.step_len(self.global_step) * 0.25,
        )
    }
```

- [ ] **Step 9: `engine.rs`.** Route Mono everywhere FM is routed:
  - `note_machines_pattern_off`: `MachineDsp::Mono(m) => m.pattern_off(),`
  - `poly_on`: `Some(MachineDsp::Mono(m)) => m.note_on(pitch, vel.min(127), live),`
  - `poly_off`: `Some(MachineDsp::Mono(m)) => m.note_off(pitch, live),`
  - `stop`: `matches!(s.dsp, MachineDsp::Fm(_) | MachineDsp::Sampler(_) | MachineDsp::Mono(_))`. Update its doc comment ("silences the FM, Mono and Sampler machines").
  - `end_playback`: after the `MachineDsp::Synth` loop, add:

```rust
        for s in &mut self.slots {
            if let MachineDsp::Mono(m) = &mut s.dsp {
                m.stop_transport();
            }
        }
```

  - `set_bpm`: after the samplers loop, add:

```rust
        for s in &mut self.slots {
            if let MachineDsp::Mono(m) = &mut s.dsp {
                m.set_bpm(bpm);
            }
        }
```

  - `set_rack`, where fresh slots get the tempo:

```rust
                Some(mut s) => {
                    match &mut s.dsp {
                        MachineDsp::Sampler(x) => x.set_bpm(bpm),
                        MachineDsp::Mono(m) => m.set_bpm(bpm),
                        _ => {}
                    }
                    s
                }
```

  - `from_project`: if it applies the tempo through `self.set_bpm(p.bpm)` (line ~165), nothing more is needed. Check that it does.
  - `process_block`, in the `until == 0` branch: read the clock before the tick, and clock the Mono machines after its events:

```rust
                ticks_here += 1;
                let (tick, tick_len) = self.seq.tick_clock();
                self.events.clear();
                self.seq
                    .tick(Banks::of_slots(&self.slots), &self.song, &mut self.events);
                for i in 0..self.events.len() {
                    let ev = self.events[i];
                    self.handle_event(ev);
                }
                // After this tick's notes, so a note on a step boundary sounds on it.
                for s in &mut self.slots {
                    if let MachineDsp::Mono(m) = &mut s.dsp {
                        m.clock_tick(tick, tick_len);
                    }
                }
                if self.seq.take_ended() {
```

  - Engine tests: the `dense_rack` helper (around line 967) matches `MachineBody::Fm(… { patterns, .. }) | MachineBody::Sampler(…)`. Add `| MachineBody::Mono(crate::project::MonoMachine { patterns, .. })` to that arm.

- [ ] **Step 10: `tests/no_alloc.rs`.** In `machines_joining_mid_pattern_do_not_allocate`, change the list to `[(7, MachineType::Synth), (8, MachineType::Fm), (9, MachineType::Mono)]` and set `q.next_uid = 10;`.

- [ ] **Step 11: Run the engine tests.**

Run: `cargo test -p acid-engine`
Expected: PASS. Any remaining `non-exhaustive patterns` error names a match that needs a Mono arm: give it the same behaviour as the `Fm` arm.

- [ ] **Step 12: App plumbing.**
  - `crates/app/src/ui/files.rs`, in copy: `MachineBody::Mono(m) => Clip::Notes(m.patterns[slot].clone()),`. In paste, change the arm to `(Some(Clip::Notes(p)), MachineBody::Fm(_) | MachineBody::Mono(_) | MachineBody::Sampler(_))`, and the doc comment to "notes onto FM, Mono or Sampler".
  - `crates/app/src/ui/mod.rs`, in `rack`: `MachineType::Mono => self.mono_panel(ui, uid),`. Add `mod mono_panel;` next to `mod fm_panel;`.
  - `crates/app/src/ui/rack_header.rs` test `the_add_menu_lists_every_type_in_order`: expect `["303 Synth", "808 Kit", "909 Kit", "FM", "Sampler", "Mono"]`.
  - Create `crates/app/src/ui/mono_panel.rs`:

```rust
//! The Mono panel: the synth's knobs in front-panel order, the patch
//! browser, and the pattern row with the poly piano roll.

use super::SidAcidApp;
use acid_engine::machine::MachineUid;
use acid_engine::project::{MonoMachine, PatternRef};
use eframe::egui;

impl SidAcidApp {
    /// Mono machine `uid`; it must be a Mono machine in the rack.
    fn mono_of(&self, uid: MachineUid) -> &MonoMachine {
        self.doc.project.mono(uid).expect("a Mono machine in the rack")
    }

    pub(crate) fn mono_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{}   {}   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label(),
            self.mono_of(uid).patch_name
        );
        self.machine_panel(ui, uid, "mono", title, |app, ui| {
            app.note_pattern_row(ui, uid);
            app.poly_roll(ui, uid);
        });
    }
}
```

  Then run `cargo build -p app` and fix any other non-exhaustive match on `MachineType` or `MachineBody` by mirroring its FM arm.

- [ ] **Step 13: Add an app test** in `crates/app/src/ui/rack_header.rs` tests, next to the existing `add_machine(MachineType::Fm)` test:

```rust
    #[test]
    fn a_mono_machine_can_be_added_and_drawn() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        assert!(app.add_machine(MachineType::Mono).is_some());
        let m = app
            .doc
            .project
            .machines
            .iter()
            .find(|m| m.machine_type() == MachineType::Mono)
            .unwrap();
        assert_eq!(m.name, "Mono");
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    }
```

Check the imports used by the neighbouring tests in that module (`Document`, `egui`) and match them. If `add_machine` returns a type other than an `Option`, adapt the first assertion to it; `rack_header.rs:358-362` shows the return shape.

- [ ] **Step 14: Run everything.**

Run: `cargo test -p acid-engine && cargo test -p app && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS, with no warnings.

- [ ] **Step 15: Commit.**

```bash
git add crates
git commit -m "feat(engine,app): Mono machine joins the rack, clocked by the sequencer; file format v4"
```

---

### Task 9: Starter patches, Randomise, and one-step undo

**Files:**
- Create: `crates/acid-engine/src/va/starter.rs`, `crates/app/src/ui/mono_patches.rs`
- Modify:
  - `crates/acid-engine/src/va/mod.rs` (add `pub mod starter;`)
  - `crates/acid-engine/src/random.rs`
  - `crates/app/src/document.rs`
  - `crates/app/src/ui/mod.rs` (add `mod mono_patches;`)

**Interfaces:**
- Produces:
  - `va::starter::{MonoPreset, STARTER}`, with `MonoPreset { name: &'static str, values: &'static [(MonoParam, f32)] }` and `MonoPreset::to_params(&self) -> Vec<(MonoParam, f32)>` (every `MonoParam`).
  - `random::randomise_mono(current: impl Fn(MonoParam) -> f32, seed: u64) -> Vec<(MonoParam, f32)>` (every `MonoParam` except `Volume`).
  - `document::Edit::MonoPatch { uid: MachineUid, name: String, values: Vec<(ParamId, f32)> }`.
  - `SidAcidApp::load_mono_patch(&mut self, uid, name: &str, values: Vec<(MonoParam, f32)>)`, `randomise_mono_patch(&mut self, uid)`, `randomise_mono_patch_seeded(&mut self, uid, seed: u64)`.

- [ ] **Step 1: Write the failing starter tests** at the bottom of `va/starter.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::rms;
    use crate::va::MonoSynth;
    use std::collections::HashSet;

    #[test]
    fn the_bank_is_well_formed() {
        assert!((12..=16).contains(&STARTER.len()));
        let names: HashSet<_> = STARTER.iter().map(|p| p.name).collect();
        assert_eq!(names.len(), STARTER.len(), "unique names");
        for p in STARTER {
            let params: HashSet<_> = p.values.iter().map(|(m, _)| *m).collect();
            assert_eq!(params.len(), p.values.len(), "{}: a param listed twice", p.name);
            assert!(p.values.iter().all(|(_, v)| (0.0..=1.0).contains(v)), "{}", p.name);
            assert_eq!(p.to_params().len(), MonoParam::ALL.len());
        }
    }

    #[test]
    fn every_patch_makes_sound() {
        for p in STARTER {
            let mut s = MonoSynth::new(48_000.0);
            for (m, v) in p.to_params() {
                s.set_param(m, v);
            }
            s.note_on(48, 100, true);
            let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
            s.render(&mut l, &mut r);
            assert!(rms(&l) > 0.01, "{} is too quiet: {}", p.name, rms(&l));
        }
    }
}
```

- [ ] **Step 2: Write the failing Randomise tests** in `random.rs` tests:

```rust
    #[test]
    fn mono_randomise_is_deterministic_varied_and_playable() {
        use crate::params::MonoParam as M;
        let defaults = |p: M| p.info().default;
        assert_eq!(randomise_mono(defaults, 3), randomise_mono(defaults, 3));
        assert_ne!(randomise_mono(defaults, 3), randomise_mono(defaults, 4));
        for seed in 0..300 {
            let v = randomise_mono(defaults, seed);
            let get = |p: M| v.iter().find(|(q, _)| *q == p).unwrap().1;
            assert!(v.iter().all(|(p, x)| *p != M::Volume && (0.0..=1.0).contains(x)));
            assert_eq!(v.len(), M::ALL.len() - 1);
            assert!(
                [M::PulseLevel, M::SawLevel, M::SubLevel].iter().any(|&p| get(p) >= 0.3),
                "seed {seed}: silent mixer"
            );
            assert!(get(M::EnvAmount) >= 0.2 || get(M::Cutoff) >= 0.35, "seed {seed}");
            assert!(get(M::Sustain) >= 0.1 || get(M::Decay) >= 0.12, "seed {seed}");
            // The arp is off in `defaults`: only the Blip character turns it on, at 1/16.
            if get(M::ArpOn) >= 0.5 {
                assert_eq!(get(M::ArpRate), 0.6, "seed {seed}");
            }
            assert_eq!(get(M::Drift), 0.0, "drift is kept");
        }
    }
```

- [ ] **Step 3: Write the failing app tests** at the bottom of the new `crates/app/src/ui/mono_patches.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::machine::MachineType;
    use acid_engine::params::MonoParam;

    fn app_with_mono() -> (SidAcidApp, MachineUid) {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Mono);
        let uid = app
            .doc
            .project
            .machines
            .iter()
            .find(|m| m.machine_type() == MachineType::Mono)
            .unwrap()
            .uid;
        (app, uid)
    }

    #[test]
    fn loading_a_starter_patch_is_one_undo_step() {
        let (mut app, uid) = app_with_mono();
        let before = app.doc.project.clone();
        let preset = &STARTER[1];
        app.load_mono_patch(uid, preset.name, preset.to_params());
        assert_eq!(app.doc.project.mono(uid).unwrap().patch_name, preset.name);
        for (p, v) in preset.to_params() {
            assert!((app.doc.project.param(uid.param(p)) - v).abs() < 1e-6, "{p:?}");
        }
        app.doc.undo();
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn randomising_keeps_volume_and_the_sequence_and_undoes_in_one_step() {
        let (mut app, uid) = app_with_mono();
        let before = app.doc.project.clone();
        app.randomise_mono_patch_seeded(uid, 7);
        let p = &app.doc.project;
        assert_eq!(p.mono(uid).unwrap().patch_name, "Random");
        assert_eq!(
            p.param(uid.param(MonoParam::Volume)),
            before.param(uid.param(MonoParam::Volume))
        );
        assert_eq!(p.mono(uid).unwrap().patterns, before.mono(uid).unwrap().patterns);
        app.doc.undo();
        assert_eq!(app.doc.project, before);
    }
}
```

- [ ] **Step 4: Run them to verify they fail.**

Run: `cargo test -p acid-engine starter mono_randomise; cargo test -p app mono_patches`
Expected: FAIL to compile.

- [ ] **Step 5: Implement `va/starter.rs`** above its tests:

```rust
//! Built-in Mono patches: original sounds, stored as the knobs that differ
//! from the defaults (`MonoParam::info().default`).

use crate::params::MonoParam;
use crate::params::MonoParam::*;

pub struct MonoPreset {
    pub name: &'static str,
    pub values: &'static [(MonoParam, f32)],
}

impl MonoPreset {
    /// Every Mono param: the preset's value, else the default.
    pub fn to_params(&self) -> Vec<(MonoParam, f32)> {
        MonoParam::ALL
            .iter()
            .map(|&p| {
                let v = self.values.iter().find(|(q, _)| *q == p);
                (p, v.map_or(p.info().default, |&(_, v)| v))
            })
            .collect()
    }
}

// Switch positions as knob values: Range 16'/8'/4'/2' = 0, 0.333, 0.667, 1;
// three-way switches 0, 0.5, 1; Arp Rate 1/4…1/32 = 0, 0.2, … 1 (1/16 = 0.6).
pub const STARTER: &[MonoPreset] = &[
    MonoPreset { name: "Sub Bass", values: &[
        (Range, 0.0), (SawLevel, 0.0), (SubLevel, 0.9), (PulseLevel, 0.3), (Cutoff, 0.3),
        (Resonance, 0.1), (EnvAmount, 0.2), (Decay, 0.5), (Sustain, 0.7), (Release, 0.15),
    ] },
    MonoPreset { name: "Squelch", values: &[
        (Range, 0.333), (SawLevel, 0.9), (Cutoff, 0.25), (Resonance, 0.75), (EnvAmount, 0.6),
        (Decay, 0.3), (Sustain, 0.0), (Release, 0.15), (Portamento, 1.0), (PortaTime, 0.25),
    ] },
    MonoPreset { name: "Pluck Bass", values: &[
        (Range, 0.0), (SawLevel, 0.0), (PulseLevel, 0.8), (PulseWidth, 0.4), (SubLevel, 0.5),
        (Cutoff, 0.2), (Resonance, 0.3), (EnvAmount, 0.7), (Decay, 0.25), (Sustain, 0.0),
        (Release, 0.1),
    ] },
    MonoPreset { name: "Rubber Bass", values: &[
        (Range, 0.0), (SawLevel, 0.6), (SubLevel, 0.6), (SubWave, 0.5), (Cutoff, 0.3),
        (Resonance, 0.5), (EnvAmount, 0.5), (Attack, 0.1), (Decay, 0.35), (Sustain, 0.3),
        (Portamento, 0.5), (PortaTime, 0.3),
    ] },
    MonoPreset { name: "Saw Lead", values: &[
        (Range, 0.333), (SawLevel, 0.9), (Cutoff, 0.6), (Resonance, 0.3), (EnvAmount, 0.3),
        (Attack, 0.05), (Decay, 0.5), (Sustain, 0.8), (Release, 0.35), (VcoMod, 0.12),
        (LfoRate, 0.5), (Portamento, 1.0), (PortaTime, 0.3),
    ] },
    MonoPreset { name: "Square Lead", values: &[
        (Range, 0.333), (SawLevel, 0.0), (PulseLevel, 0.9), (Cutoff, 0.55), (Resonance, 0.2),
        (EnvAmount, 0.25), (Sustain, 0.9), (Release, 0.3),
    ] },
    MonoPreset { name: "PWM Lead", values: &[
        (Range, 0.333), (SawLevel, 0.0), (PulseLevel, 0.9), (PulseWidth, 0.7), (PwmSource, 0.5),
        (LfoRate, 0.35), (Cutoff, 0.6), (Resonance, 0.25), (EnvAmount, 0.2), (Sustain, 0.85),
        (Release, 0.4), (ChorusMix, 0.3),
    ] },
    MonoPreset { name: "Chorus Lead", values: &[
        (Range, 0.667), (SawLevel, 0.7), (PulseLevel, 0.5), (PulseWidth, 0.3), (Cutoff, 0.65),
        (Resonance, 0.35), (EnvAmount, 0.3), (Attack, 0.15), (Sustain, 0.8), (Release, 0.5),
        (ChorusMix, 0.7), (ChorusDepth, 0.7),
    ] },
    MonoPreset { name: "Blip", values: &[
        (Range, 0.667), (SawLevel, 0.0), (PulseLevel, 0.9), (PulseWidth, 0.6), (Cutoff, 0.4),
        (Resonance, 0.6), (EnvAmount, 0.7), (Decay, 0.15), (Sustain, 0.0), (Release, 0.08),
    ] },
    MonoPreset { name: "Zap", values: &[
        (Range, 0.667), (SawLevel, 0.9), (Cutoff, 0.2), (Resonance, 0.85), (EnvAmount, 1.0),
        (Decay, 0.2), (Sustain, 0.0), (Release, 0.1), (KeyFollow, 1.0),
    ] },
    MonoPreset { name: "Noise Snare", values: &[
        (Range, 0.667), (SawLevel, 0.0), (NoiseLevel, 0.9), (PulseLevel, 0.3), (Cutoff, 0.55),
        (Resonance, 0.3), (EnvAmount, 0.4), (Decay, 0.22), (Sustain, 0.0), (Release, 0.15),
        (KeyFollow, 0.0),
    ] },
    MonoPreset { name: "Arp Pluck", values: &[
        (Range, 0.333), (SawLevel, 0.6), (PulseLevel, 0.6), (PulseWidth, 0.5), (Cutoff, 0.35),
        (Resonance, 0.55), (EnvAmount, 0.6), (Decay, 0.2), (Sustain, 0.0), (Release, 0.12),
        (ArpOn, 1.0), (ArpMode, 0.5), (ArpRange, 0.5), (ArpRate, 0.6), (ChorusMix, 0.3),
    ] },
    MonoPreset { name: "Res Sweep", values: &[
        (Range, 0.333), (SawLevel, 0.8), (NoiseLevel, 0.15), (Cutoff, 0.15), (Resonance, 0.9),
        (EnvAmount, 0.8), (Attack, 0.6), (Decay, 0.75), (Sustain, 0.4), (Release, 0.6),
        (VcfMod, 0.2), (LfoRate, 0.3),
    ] },
    MonoPreset { name: "Drift Pad", values: &[
        (Range, 0.333), (SawLevel, 0.7), (PulseLevel, 0.4), (PulseWidth, 0.5), (PwmSource, 0.5),
        (LfoRate, 0.25), (Cutoff, 0.45), (Resonance, 0.2), (EnvAmount, 0.2), (Attack, 0.55),
        (Decay, 0.6), (Sustain, 0.8), (Release, 0.6), (Drift, 0.7), (ChorusMix, 0.6),
    ] },
];
```

Add `#[rustfmt::skip]` above `pub const STARTER` if `cargo fmt` explodes the table, so it stays readable.

- [ ] **Step 6: Implement `randomise_mono`** in `random.rs`. Put it after `randomise_voice`, with `use crate::params::MonoParam;` added to the imports:

```rust
/// A new Mono sound from `seed`: one of four characters (bass, lead,
/// blip, sweep) with knob ranges that suit it, then guards so it always
/// plays. Volume is not returned (left alone); the arp, drift and chorus
/// are kept from `current`, except that a blip turns the arp on at 1/16.
pub fn randomise_mono(current: impl Fn(MonoParam) -> f32, seed: u64) -> Vec<(MonoParam, f32)> {
    use MonoParam as M;
    let mut rng = Pcg32::seed_from_u64(seed);
    let mut v: Vec<(MonoParam, f32)> = M::ALL
        .iter()
        .filter(|&&p| p != M::Volume)
        .map(|&p| (p, p.info().default))
        .collect();
    fn set(v: &mut [(MonoParam, f32)], p: MonoParam, x: f32) {
        if let Some(e) = v.iter_mut().find(|(q, _)| *q == p) {
            e.1 = x.clamp(0.0, 1.0);
        }
    }
    fn get(v: &[(MonoParam, f32)], p: MonoParam) -> f32 {
        v.iter().find(|(q, _)| *q == p).map_or(0.0, |e| e.1)
    }
    for p in [
        M::ArpOn, M::ArpMode, M::ArpRange, M::ArpRate,
        M::Drift, M::ChorusRate, M::ChorusDepth, M::ChorusMix,
    ] {
        set(&mut v, p, current(p));
    }
    let mut r = |lo: f32, hi: f32| rng.gen_range(lo..=hi);
    // (param, low, high) per character; switches pick a position.
    let ranges: &[(MonoParam, f32, f32)] = match r(0.0, 3.999) as u32 {
        0 => &[
            (M::Range, 0.0, 0.333), (M::SawLevel, 0.0, 1.0), (M::PulseLevel, 0.0, 0.8),
            (M::SubLevel, 0.4, 0.9), (M::PulseWidth, 0.0, 0.6), (M::Cutoff, 0.15, 0.45),
            (M::Resonance, 0.0, 0.7), (M::EnvAmount, 0.3, 0.8), (M::Attack, 0.0, 0.05),
            (M::Decay, 0.15, 0.5), (M::Sustain, 0.0, 0.4), (M::Release, 0.05, 0.25),
            (M::Portamento, 0.0, 1.0), (M::PortaTime, 0.1, 0.35),
        ],
        1 => &[
            (M::Range, 0.333, 0.667), (M::SawLevel, 0.3, 1.0), (M::PulseLevel, 0.0, 0.9),
            (M::PulseWidth, 0.0, 0.6), (M::PwmSource, 0.0, 0.5), (M::Cutoff, 0.35, 0.75),
            (M::Resonance, 0.0, 0.6), (M::EnvAmount, 0.1, 0.5), (M::Attack, 0.0, 0.2),
            (M::Decay, 0.3, 0.7), (M::Sustain, 0.5, 1.0), (M::Release, 0.2, 0.5),
            (M::VcoMod, 0.0, 0.15), (M::LfoRate, 0.45, 0.6), (M::Portamento, 0.0, 1.0),
            (M::PortaTime, 0.1, 0.4),
        ],
        2 => &[
            (M::Range, 0.667, 1.0), (M::SawLevel, 0.0, 0.5), (M::PulseLevel, 0.6, 1.0),
            (M::PulseWidth, 0.0, 0.7), (M::Cutoff, 0.3, 0.6), (M::Resonance, 0.4, 0.8),
            (M::EnvAmount, 0.5, 0.9), (M::Attack, 0.0, 0.0), (M::Decay, 0.05, 0.25),
            (M::Sustain, 0.0, 0.0), (M::Release, 0.05, 0.15), (M::ArpMode, 0.0, 1.0),
            (M::ArpRange, 0.0, 1.0),
        ],
        _ => &[
            (M::Range, 0.333, 0.333), (M::SawLevel, 0.5, 1.0), (M::NoiseLevel, 0.0, 0.3),
            (M::Cutoff, 0.1, 0.3), (M::Resonance, 0.7, 0.95), (M::EnvAmount, 0.6, 1.0),
            (M::Attack, 0.3, 0.7), (M::Decay, 0.5, 0.9), (M::Sustain, 0.2, 0.6),
            (M::Release, 0.4, 0.8), (M::VcfMod, 0.0, 0.3), (M::LfoRate, 0.2, 0.4),
        ],
    };
    let blip = ranges.iter().any(|&(p, _, _)| p == M::ArpMode);
    for &(p, lo, hi) in ranges {
        let x = r(lo, hi);
        let steps = p.info().steps;
        // Switches land exactly on a position.
        let x = if steps >= 2 {
            let n = (steps - 1) as f32;
            (x * n).round() / n
        } else {
            x
        };
        set(&mut v, p, x);
    }
    if blip {
        set(&mut v, M::ArpOn, 1.0);
        set(&mut v, M::ArpRate, 0.6);
    }
    // Guards: something to hear, a filter that opens, notes that last.
    if [M::PulseLevel, M::SawLevel, M::SubLevel].iter().all(|&p| get(&v, p) < 0.3) {
        set(&mut v, M::SawLevel, 0.7);
    }
    if get(&v, M::EnvAmount) < 0.2 && get(&v, M::Cutoff) < 0.35 {
        set(&mut v, M::Cutoff, 0.35);
    }
    if get(&v, M::Sustain) < 0.1 && get(&v, M::Decay) < 0.12 {
        set(&mut v, M::Decay, 0.12);
    }
    v
}
```

`rng` is borrowed mutably by the `r` closure, which is only used before the guards, so no borrow conflict arises. If the borrow checker disagrees, replace the closure with a nested `fn r(rng: &mut Pcg32, lo: f32, hi: f32) -> f32`.

Note: the 0.6 for `ArpRate` is exactly representable after `round`, since 3/5 = 0.6 in f32, so the test's `assert_eq!(…, 0.6)` holds.

- [ ] **Step 7: Add `Edit::MonoPatch`** in `crates/app/src/document.rs`, mirroring `FmVoice` exactly:
  - In the `Edit` enum after `FmBank`: `/// Loading a Mono patch: its name and every knob, one undo step.` followed by `MonoPatch { uid: MachineUid, name: String, values: Vec<(ParamId, f32)> },`
  - In `MergeKey`: `MonoPatch(MachineUid),`. In `merge_key`: `Edit::MonoPatch { uid, .. } => MergeKey::MonoPatch(*uid),`
  - Next to `const NO_FM`: `const NO_MONO: &str = "the edit names a Mono machine in the rack";`
  - In apply (next to `Edit::FmVoice { uid, name, values } =>`, ~line 732):

```rust
            Edit::MonoPatch { uid, name, values } => {
                let old_name =
                    std::mem::replace(&mut p.mono_mut(uid).expect(NO_MONO).patch_name, name);
                let mut old_values: Vec<_> = values
                    .into_iter()
                    .map(|(id, v)| {
                        let old = p.param(id);
                        p.set_param(id, v.clamp(0.0, 1.0));
                        (id, old)
                    })
                    .collect();
                old_values.reverse();
                Edit::MonoPatch {
                    uid,
                    name: old_name,
                    values: old_values,
                }
            }
```

  - In the commands function (~line 861): change `Edit::FmVoice { values, .. } =>` to `Edit::FmVoice { values, .. } | Edit::MonoPatch { values, .. } =>`.
  - Any other exhaustive `match edit` that the compiler flags: give `MonoPatch` the same arm as `FmVoice`.

- [ ] **Step 8: Implement `crates/app/src/ui/mono_patches.rs`** above its tests:

```rust
//! Loading Mono patches: from the starter bank or Randomise. A load is
//! one undoable edit.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::machine::MachineUid;
use acid_engine::params::MonoParam;
pub(crate) use acid_engine::va::starter::STARTER;

impl SidAcidApp {
    pub(crate) fn load_mono_patch(
        &mut self,
        uid: MachineUid,
        name: &str,
        values: Vec<(MonoParam, f32)>,
    ) {
        let values = values.into_iter().map(|(p, v)| (uid.param(p), v)).collect();
        let cmds = self.doc.apply(
            Edit::MonoPatch {
                uid,
                name: name.into(),
                values,
            },
            false,
        );
        self.send(cmds);
        self.view.focus = Some(uid);
    }

    /// The Mono panel's Randomise: a new sound that always plays, as one
    /// undo step. Volume, arp, drift and chorus are kept.
    pub(crate) fn randomise_mono_patch(&mut self, uid: MachineUid) {
        let seed = self.next_seed();
        self.randomise_mono_patch_seeded(uid, seed);
    }

    pub(crate) fn randomise_mono_patch_seeded(&mut self, uid: MachineUid, seed: u64) {
        let project = &self.doc.project;
        if project.mono(uid).is_none() {
            return;
        }
        let values =
            acid_engine::random::randomise_mono(|p| project.param(uid.param(p)), seed);
        self.load_mono_patch(uid, "Random", values);
    }
}
```

`uid.param(p)` needs `MonoParam: Into<MachineParam>`, which Task 8's `machine_param_from!` entry provides. Add `mod mono_patches;` to `crates/app/src/ui/mod.rs`. Add `pub mod starter;` to `va/mod.rs`.

- [ ] **Step 9: Run the tests.**

Run: `cargo test -p acid-engine && cargo test -p app`
Expected: PASS.

- [ ] **Step 10: Commit.**

```bash
git add crates
git commit -m "feat(engine,app): Mono starter patches and Randomise, each one undo step"
```

---

### Task 10: The Mono panel

**Files:**
- Modify:
  - `crates/app/src/ui/mono_panel.rs`
  - `crates/app/src/ui/params_ui.rs` (`discrete_names`)
  - `crates/app/src/ui/polyroll.rs` (`roll_start_pitch`)

**Interfaces:**
- Consumes: `param_knob(ui, id, label)` (`params_ui.rs:140`), `machine_panel`, `note_pattern_row`, `poly_roll`, `STARTER`, `load_mono_patch`, `randomise_mono_patch`.

- [ ] **Step 1: Write the failing tests.**

In `crates/app/src/ui/params_ui.rs` tests (or a new `#[cfg(test)] mod tests` there, if it has none):

```rust
    #[test]
    fn mono_switches_have_names_for_every_position() {
        use acid_engine::machine::MachineUid;
        use acid_engine::params::MonoParam;
        let uid = MachineUid(9);
        for p in MonoParam::ALL {
            let id = uid.param(p);
            let steps = id.info().steps as usize;
            if steps >= 2 {
                assert_eq!(discrete_names(id).len(), steps, "{p:?}");
            }
        }
    }
```

In `crates/app/src/ui/polyroll.rs` tests:

```rust
    #[test]
    fn the_mono_roll_opens_on_c2() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(acid_engine::machine::MachineType::Mono);
        let uid = app.doc.project.machines.last().unwrap().uid;
        assert_eq!(app.doc.project.mono(uid).map(|_| ()), Some(()));
        assert_eq!(app.roll_start_pitch(uid), 36);
    }
```

Before relying on `machines.last()`, check that `add_machine` appends: read `add_machine` in `crates/app/src/ui/mod.rs`. If it inserts elsewhere, find the uid by type as `mono_patches.rs` does. Match the module's existing imports for `Document`.

In `crates/app/src/ui/mono_panel.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::machine::MachineType;

    #[test]
    fn the_panel_draws_its_sections_and_the_patch_list() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Mono);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.ui(ctx));
        let mut texts = Vec::new();
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        for want in ["VCO", "SOURCE", "VCF", "ENV", "LFO", "ARP", "Squelch", "Randomise"] {
            assert!(texts.iter().any(|t| t == want), "{want} not drawn");
        }
    }
}
```

- [ ] **Step 2: Run them to verify they fail.**

Run: `cargo test -p app mono`
Expected: FAIL (no names, roll starts at 48, no sections drawn).

- [ ] **Step 3: Switch names.** In `discrete_names` (`params_ui.rs`), before the `_ if id.info().steps == 2` fallback, add:

```rust
        ParamId::Machine(_, M::Mono(p)) => match p {
            MonoParam::Range => &["16'", "8'", "4'", "2'"],
            MonoParam::PwmSource => &["Manual", "LFO", "Env"],
            MonoParam::SubWave => &["-1 Sq", "-2 Sq", "-2 Pls"],
            MonoParam::Trigger => &["Gate+Trig", "Gate", "LFO"],
            MonoParam::VcaMode => &["Env", "Gate"],
            MonoParam::LfoWave => &["Tri", "Square", "Random", "Noise"],
            MonoParam::Portamento => &["Off", "On", "Auto"],
            MonoParam::ArpMode => &["Up", "U&D", "Down"],
            MonoParam::ArpRange => &["1 Oct", "2 Oct", "3 Oct"],
            MonoParam::ArpRate => &["1/4", "1/8", "1/8T", "1/16", "1/16T", "1/32"],
            _ if id.info().steps == 2 => &["Off", "On"],
            _ => &[],
        },
```

Add `MonoParam` to that file's `acid_engine::params` import.

- [ ] **Step 4: Roll start pitch.** In `polyroll.rs`, add `/// Pitch the Mono roll first scrolls to: C2 (MIDI 36), as it is mostly bass.` and `const MONO_START_PITCH: u8 = 36;`. In `roll_start_pitch`, add as the first line:

```rust
        if self.doc.project.mono(uid).is_some() {
            return MONO_START_PITCH;
        }
```

- [ ] **Step 5: Build the panel.** Replace the body of `mono_panel.rs` (keep the test module):

```rust
//! The Mono panel: the synth's knobs grouped in front-panel order (VCO,
//! Source, VCF, Env; then LFO, Porta, Arp, Drift, Chorus), an envelope
//! sketch, the patch browser, and the pattern row with the poly piano roll.

use super::mono_patches::STARTER;
use super::{SidAcidApp, theme};
use acid_engine::machine::MachineUid;
use acid_engine::params::MonoParam as M;
use acid_engine::project::{MonoMachine, PatternRef};
use eframe::egui::{self, Sense, Stroke, pos2, vec2};

type Group = (&'static str, &'static [(M, &'static str)]);

const FRONT: [Group; 4] = [
    ("VCO", &[(M::Range, "Range"), (M::Tune, "Tune"), (M::VcoMod, "Mod"), (M::PulseWidth, "PW"), (M::PwmSource, "PWM")]),
    ("SOURCE", &[(M::PulseLevel, "Pulse"), (M::SawLevel, "Saw"), (M::SubLevel, "Sub"), (M::SubWave, "Sub Wave"), (M::NoiseLevel, "Noise")]),
    ("VCF", &[(M::Cutoff, "Cutoff"), (M::Resonance, "Reso"), (M::EnvAmount, "Env"), (M::VcfMod, "Mod"), (M::KeyFollow, "Kybd"), (M::VelToVcf, "Vel")]),
    ("ENV", &[(M::Attack, "A"), (M::Decay, "D"), (M::Sustain, "S"), (M::Release, "R"), (M::Trigger, "Trig"), (M::VcaMode, "VCA"), (M::VelToVca, "Vel")]),
];
const SECOND: [Group; 6] = [
    ("LFO", &[(M::LfoWave, "Wave"), (M::LfoRate, "Rate")]),
    ("PORTA", &[(M::Portamento, "Mode"), (M::PortaTime, "Time")]),
    ("ARP", &[(M::ArpOn, "On"), (M::ArpMode, "Mode"), (M::ArpRange, "Range"), (M::ArpRate, "Rate")]),
    ("DRIFT", &[(M::Drift, "Drift")]),
    ("CHORUS", &[(M::ChorusRate, "Rate"), (M::ChorusDepth, "Depth"), (M::ChorusMix, "Mix")]),
    ("OUT", &[(M::Volume, "Volume")]),
];

impl SidAcidApp {
    /// Mono machine `uid`; it must be a Mono machine in the rack.
    fn mono_of(&self, uid: MachineUid) -> &MonoMachine {
        self.doc.project.mono(uid).expect("a Mono machine in the rack")
    }

    pub(crate) fn mono_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{}   {}   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label(),
            self.mono_of(uid).patch_name
        );
        self.machine_panel(ui, uid, "mono", title, |app, ui| {
            ui.horizontal_wrapped(|ui| {
                for group in FRONT {
                    app.mono_group(ui, uid, group);
                }
            });
            ui.horizontal_wrapped(|ui| {
                for group in SECOND {
                    app.mono_group(ui, uid, group);
                }
                app.mono_env_sketch(ui, uid);
            });
            app.mono_patch_browser(ui, uid);
            app.note_pattern_row(ui, uid);
            app.poly_roll(ui, uid);
        });
    }

    fn mono_group(&mut self, ui: &mut egui::Ui, uid: MachineUid, (label, knobs): Group) {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).small().color(theme::ACID));
                ui.horizontal(|ui| {
                    for &(p, name) in knobs {
                        self.param_knob(ui, uid.param(p), name);
                    }
                });
            });
        });
    }

    /// Display-only ADSR shape from the current knobs (times drawn on a
    /// log-ish scale so short and long envelopes both read).
    fn mono_env_sketch(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let v = |p: M| self.doc.project.param(uid.param(p));
        let (a, d, s, r) = (v(M::Attack), v(M::Decay), v(M::Sustain), v(M::Release));
        let (rect, _) = ui.allocate_exact_size(vec2(96.0, 44.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, theme::GRID_DARK);
        let inner = rect.shrink(3.0);
        // Segment widths: attack, decay, a fixed sustain hold, release.
        let w = [0.1 + a, 0.1 + d, 0.6, 0.1 + r];
        let total: f32 = w.iter().sum();
        let mut x = 0.0;
        let mut points = vec![(0.0, 0.0)];
        for (seg, level) in w.iter().zip([1.0, s, s, 0.0]) {
            x += seg / total;
            points.push((x, level));
        }
        let points: Vec<egui::Pos2> = points
            .iter()
            .map(|(x, y)| pos2(inner.left() + x * inner.width(), inner.bottom() - y * inner.height()))
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5_f32, theme::ACID)));
    }

    fn mono_patch_browser(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let current = self.mono_of(uid).patch_name.clone();
        ui.horizontal(|ui| {
            ui.label("Patches");
            if ui
                .button("Randomise")
                .on_hover_text("A new random sound that always plays (undoable)")
                .clicked()
            {
                self.randomise_mono_patch(uid);
            }
        });
        let mut picked = None;
        egui::Grid::new(("mono_patches", uid))
            .spacing([4.0, 2.0])
            .show(ui, |ui| {
                for (i, preset) in STARTER.iter().enumerate() {
                    if ui.selectable_label(preset.name == current, preset.name).clicked() {
                        picked = Some(i);
                    }
                    if i % 7 == 6 {
                        ui.end_row();
                    }
                }
            });
        if let Some(i) = picked {
            let preset = &STARTER[i];
            self.load_mono_patch(uid, preset.name, preset.to_params());
        }
    }
}
```

`theme::ACID` and `theme::GRID_DARK` are the colours the FM panel uses (`fm_panel.rs`). The starter names are unique (Task 9's test), so highlighting by name is unambiguous.

- [ ] **Step 6: Run the tests.**

Run: `cargo test -p app && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS. If `the_panel_draws_its_sections_and_the_patch_list` fails because a section label isn't drawn, check that the Mono panel isn't collapsed by default in the headless run (`machine_panel` uses `load_with_default_open(…, true)`) and that the screen is tall enough to reach it.

- [ ] **Step 7: See it in the real app.** Use the `run` skill (or the project's screenshot memory, which uses PIL ImageGrab and XTest scrolling) to launch SidAcid. Add a Mono from the rack's Add menu, load "Arp Pluck", draw a three-note chord in the roll and press play.
  - **Check:** the arp plays in time with the drums; the panel fits at 1280 px wide; the knobs wrap rather than overflow.
  - Take a screenshot as evidence.
  - If the panel overflows at 1280×800, reduce the gaps between groups rather than dropping controls.

- [ ] **Step 8: Commit.**

```bash
git add crates/app
git commit -m "feat(app): Mono panel with front-panel knob groups, envelope sketch and patch browser"
```

---

## Self-Review Notes

Spec coverage:

| Spec item | Task |
|---|---|
| VCO, sub, noise, mixer | 2, 6 |
| VCF | 3, 6 |
| ENV and trigger modes, VCA modes | 4, 6 |
| LFO | 4 |
| Portamento | 6 |
| Drift | 4, 6 |
| Chorus move | 1 |
| Note stack | 6 |
| Arp order, octave range and clock | 7, 8 |
| Swing via swung ticks | 8 |
| Machine integration, v4 | 8 |
| Starter patches, Randomise | 9 |
| Panel, roll on C2 | 10 |
| Tests | Every task |

Not covered by an automated test: "panel no taller than the FM panel". It is checked by eye in Task 10, Step 7.
