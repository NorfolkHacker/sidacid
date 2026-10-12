# SidAcid Plan 5 — FM Synth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the FM slot's placeholder sine voice with a DX7-style 6-operator FM synth modelled on the M-VAVE FM-1. It has 32 algorithms, DX7 envelopes and scaling, an LFO, a pitch EG, 12-voice poly or mono with glide, filter/chorus/phaser effects, a Vintage switch, DX7 SysEx import, a starter bank, and an FM panel with a live algorithm diagram.

**Architecture:** The new `acid_engine::fm` module is split into pure, testable units:
- algorithm table
- envelope and scaling math
- LFO and pitch EG
- decoded patch
- voice
- synth
- effects
- SysEx
- starter bank

Every FM control is an ordinary automatable `ParamId` (`ParamId::Fm` for globals, a new `ParamId::FmOp(op, OpParam)` for operators), so undo, song automation and record work unchanged. Loading a voice applies all of its params as one undoable edit. The app gets an FM panel; the sampler keeps its placeholder until Plan 6.

**Tech Stack:** Rust 2024 (1.88+), egui/eframe 0.31, serde + RON, rfd. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-12-fm-sampler-design.md`. Plan 5 covers section 2 ("FM synth") and the "FM panel" part of section 4. Read both before starting.

## Global Constraints

- `cargo test --workspace` passes at the end of every task. The baseline is 318 tests. Never delete or weaken an existing assertion unless the step says so explicitly; when it does, it gives the replacement.
- `cargo clippy --workspace --all-targets` reports 0 warnings, and `cargo fmt --all` is run before each commit.
- **Real-time rule:** `Engine::process`, everything it calls, and `EngineHost::process` must not allocate, lock or do I/O. All FM state is fixed-size and allocated in `FmSynth::new`, which runs off the audio thread. `crates/acid-engine/tests/no_alloc.rs` must keep passing.
- Machine order is fixed: 0 Synth 1, 1 Synth 2, 2 808, 3 909, 4 FM, 5 Sampler. The FM renders into mixer channel 4.
- The FM sequencer and audition paths are separate. A pattern note-off releases only pattern voices, and an audition note-off releases only audition voices. A note-off releases **every** voice of that pitch from its source; this is required because overlapping same-pitch pattern notes send two PolyOns and one PolyOff.
- `PROJECT_VERSION` stays `2`. New project fields use `#[serde(default)]`, so Plan 4 files still load.
- Yamaha ROM banks are never shipped. The starter bank is original.
- Every commit message ends with the trailer line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Deliberate deviations from the spec (keep them)

1. **Operator params are `ParamId::FmOp(op, OpParam)`**, a new `ParamId` variant, not `FmParam::Op(..)`. The param enums are fieldless.
2. **Operator mute is a saved, automatable `OpParam::On` switch.** The spec had a UI-only stored level. Loading a voice turns every operator on.
3. **Numeric list params are continuous 0..1 knobs that the synth rounds.** This covers algorithm, feedback, coarse, detune, sensitivities and transpose. It keeps knob dragging smooth; tooltips show the rounded value. Only named choices use the click-to-cycle switch: waves, curves, on/off, ratio/fixed, poly/mono, LP/HP.
4. **DX7 algorithms 4 and 6 use self-feedback on OP6.** On the DX7 the loop runs from OP4 or OP5 back into OP6; that loop is approximated.
5. **The envelope graph in the operator editor is display-only.** The R/L knobs next to it edit the envelope.
6. **Approximations:** LFO speed and delay, pitch-EG ranges, PMS/AMS depth tables and detune size (about 1 cent per step) approximate the DX7. Envelope rates and levels, output-level, keyboard-scaling and velocity math follow the published DX7 reverse-engineering (music-synthesizer-for-android / Dexed) closely.
7. **Mono-mode note handling:** in mono mode, audition and pattern notes share one held-key stack.

## File structure

| File | Status | Responsibility |
|---|---|---|
| `crates/acid-engine/src/params.rs` | modify | `FmParam` (34 globals), `OpParam` (22), `ParamId::FmOp`, `FM_OPS`, `fm_steps` |
| `crates/acid-engine/src/fm/mod.rs` | new | module root, `OPS`, `BLOCK` |
| `crates/acid-engine/src/fm/algorithms.rs` | new | the 32 algorithms |
| `crates/acid-engine/src/fm/env.rs` | new | DX7 envelope, output-level / keyboard / velocity scaling |
| `crates/acid-engine/src/fm/modulation.rs` | new | LFO, pitch EG, PMS/AMS tables |
| `crates/acid-engine/src/fm/patch.rs` | new | `FmPatch` / `OpPatch`: knob values decoded to DX7 units, operator frequency |
| `crates/acid-engine/src/fm/sine.rs` | new | sine table (clean / vintage read) |
| `crates/acid-engine/src/fm/voice.rs` | new | one 6-operator voice |
| `crates/acid-engine/src/fm/synth.rs` | new | `FmSynth`: 12 voices, stealing, mono/glide, sources, render |
| `crates/acid-engine/src/fm/fx.rs` | new | filter → chorus → phaser |
| `crates/acid-engine/src/fm/sysex.rs` | new | `Dx7Voice`, bank/single parse + write, params conversion |
| `crates/acid-engine/src/fm/starter.rs` | new | 16-voice starter bank |
| `crates/acid-engine/src/engine.rs` | modify | FM slot uses `FmSynth`; separate audition source |
| `crates/acid-engine/src/project.rs` | modify | `FmMachine.voice_name`, `FmMachine.bank` |
| `crates/app/src/document.rs` | modify | `Edit::FmVoice`, `Edit::FmBank` |
| `crates/app/src/ui/params_ui.rs` | modify | FM labels, named switches, value text in tooltips |
| `crates/app/src/ui/fm_voices.rs` | new | load voice, import .syx, browser state |
| `crates/app/src/ui/alg_diagram.rs` | new | algorithm diagram layout + drawing, envelope sketch |
| `crates/app/src/ui/fm_panel.rs` | new | the FM panel |
| `crates/app/src/ui/note_panel.rs`, `ui/mod.rs`, `ui/files.rs` | modify | shared pattern row, rack, view state |
| `THIRD_PARTY_NOTICES.md`, `README.md` | modify | attribution, features |

---

### Task 1: FM parameter registry and its UI text

**Files:**
- Modify: `crates/acid-engine/src/params.rs`
- Modify: `crates/app/src/ui/params_ui.rs`

**Interfaces:**
- Produces (in `acid_engine::params`):

```rust
pub const FM_OPS: usize = 6;
pub const PARAM_COUNT: usize = 380;
pub mod fm_steps { ALGORITHM=32, FEEDBACK=8, COARSE=32, DETUNE=15, RATE_SCALING=8,
                   AMP_MOD_SENS=4, VEL_SENS=8, PITCH_MOD_SENS=8, TRANSPOSE=49, CURVES=4, LFO_WAVES=6 }
pub enum FmParam { Volume, Algorithm, Feedback, OscSync, PitchR1..PitchR4, PitchL1..PitchL4,
                   LfoSpeed, LfoDelay, LfoPmd, LfoAmd, LfoSync, LfoWave, PitchModSens, Transpose,
                   Mono, Glide, Vintage, FilterOn, FilterMode, Cutoff, Resonance,
                   ChorusRate, ChorusDepth, ChorusMix, PhaserRate, PhaserDepth, PhaserFeedback, PhaserMix }
pub enum OpParam { R1..R4, L1..L4, BreakPoint, LeftDepth, RightDepth, LeftCurve, RightCurve,
                   RateScaling, AmpModSens, VelSens, Level, FixedFreq, Coarse, Fine, Detune, On }
ParamId::FmOp(u8 /* 0..6 = OP1..OP6 */, OpParam)
```

- Produces (app `params_ui`): `discrete_names(id)` returns `&[]` for a param that is not a named switch, and `pub fn value_text(id: ParamId, v: f32) -> String`.

Defaults follow the DX7 "INIT VOICE":
- **Operators:** OP1 level 99 and every other operator level 0. Rates are 99; levels are 99/99/99/0. Break point 39, coarse 1, detune centre.
- **Global:** algorithm 1, osc key sync on, pitch EG flat (levels 50), transpose centre, PMS 3.

- [ ] **Step 1: Write the failing tests** in the `params.rs` tests module

Replace the body of `registry_has_every_param_once`, and replace the whole test `fm_and_sampler_params_come_last` with `fm_params_follow_the_master_params` below. The old test pinned `Fm(Volume)` as the second-to-last param, which is no longer true.

```rust
    #[test]
    fn registry_has_every_param_once() {
        let all = ParamId::all();
        assert_eq!(
            all.len(),
            2 * 15 + 2 * 8 * 4 + 2 * 2 + 6 * 16 + 11 + 8 + 34 + 1 + 6 * 22
        );
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
    }

    #[test]
    fn fm_params_follow_the_master_params() {
        let all = ParamId::all();
        let first_fm = all.iter().position(|id| matches!(id, ParamId::Fm(_))).unwrap();
        assert_eq!(all[first_fm], ParamId::Fm(FmParam::Volume));
        assert!(matches!(all[first_fm - 1], ParamId::Master(_)));
        assert_eq!(all[all.len() - 1], ParamId::FmOp(5, OpParam::On));
        assert!(all.contains(&ParamId::Sampler(SamplerParam::Volume)));
        assert_eq!(ParamId::Fm(FmParam::Volume).info().default, 0.7);
    }

    #[test]
    fn fm_defaults_are_the_dx7_init_voice() {
        let d = |id: ParamId| id.info().default;
        assert_eq!(d(ParamId::FmOp(0, OpParam::Level)), 1.0);
        assert_eq!(d(ParamId::FmOp(1, OpParam::Level)), 0.0);
        assert_eq!(d(ParamId::FmOp(3, OpParam::R1)), 1.0);
        assert_eq!(d(ParamId::FmOp(3, OpParam::L4)), 0.0);
        assert!((d(ParamId::FmOp(2, OpParam::Coarse)) - 1.0 / 31.0).abs() < 1e-6);
        assert_eq!(d(ParamId::FmOp(2, OpParam::Detune)), 0.5);
        assert_eq!(d(ParamId::FmOp(5, OpParam::On)), 1.0);
        assert!((d(ParamId::Fm(FmParam::PitchL1)) - 50.0 / 99.0).abs() < 1e-6);
        assert_eq!(d(ParamId::Fm(FmParam::Transpose)), 0.5);
        assert_eq!(d(ParamId::Fm(FmParam::Algorithm)), 0.0);
        assert_eq!(ParamId::Fm(FmParam::LfoWave).info().steps, 6);
        assert_eq!(ParamId::FmOp(0, OpParam::LeftCurve).info().steps, 4);
        assert_eq!(ParamId::Fm(FmParam::Algorithm).info().steps, 0, "a rounded knob");
    }

    #[test]
    fn fm_operator_ids_are_bounded() {
        assert!(ParamId::FmOp(5, OpParam::Fine).is_valid());
        assert!(!ParamId::FmOp(6, OpParam::Fine).is_valid());
        assert_eq!(ParamId::FmOp(6, OpParam::Fine).index(), None);
    }
```

In `params_ui.rs` tests, add:

```rust
    #[test]
    fn fm_labels_switches_and_values() {
        use acid_engine::params::{FmParam, OpParam};
        assert_eq!(param_label(ParamId::FmOp(2, OpParam::Coarse)), "FM OP3 Coarse");
        assert_eq!(param_label(ParamId::Fm(FmParam::Cutoff)), "FM Cutoff");
        assert_eq!(discrete_names(ParamId::Fm(FmParam::LfoWave)).len(), 6);
        assert_eq!(discrete_names(ParamId::FmOp(0, OpParam::LeftCurve))[3], "+Lin");
        assert_eq!(discrete_names(ParamId::FmOp(0, OpParam::On)), &["Off", "On"]);
        assert!(discrete_names(ParamId::Fm(FmParam::Algorithm)).is_empty());
        assert_eq!(value_text(ParamId::Fm(FmParam::Algorithm), 1.0), "32");
        assert_eq!(value_text(ParamId::FmOp(0, OpParam::Detune), 0.0), "-7");
        assert_eq!(value_text(ParamId::Fm(FmParam::Transpose), 1.0), "+24");
        assert_eq!(value_text(ParamId::FmOp(0, OpParam::R1), 1.0), "99");
        assert_eq!(value_text(ParamId::Synth(0, SynthParam::Cutoff), 0.5), "50%");
    }
```

(Import `SynthParam` in that test module if it isn't already.)

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine params:: && cargo test -p sidacid params_ui`
Expected: compile errors (`OpParam`, `FmOp`, `value_text` missing).

- [ ] **Step 3: Implement the registry** in `params.rs`

```rust
/// Operators in the FM synth.
pub const FM_OPS: usize = 6;
/// Number of entries in `ParamId::all()`.
pub const PARAM_COUNT: usize = 380;

/// Positions of the FM controls that pick from a list. The numeric ones
/// are continuous knobs the synth rounds with `step_index(v, N)`.
pub mod fm_steps {
    pub const ALGORITHM: usize = 32;
    pub const FEEDBACK: usize = 8;
    pub const COARSE: usize = 32;
    pub const DETUNE: usize = 15;
    pub const RATE_SCALING: usize = 8;
    pub const AMP_MOD_SENS: usize = 4;
    pub const VEL_SENS: usize = 8;
    pub const PITCH_MOD_SENS: usize = 8;
    pub const TRANSPOSE: usize = 49;
    pub const CURVES: usize = 4;
    pub const LFO_WAVES: usize = 6;
}
```

Replace the existing `param_enum!(FmParam { Volume => "Volume" });` with:

```rust
param_enum!(FmParam {
    Volume => "Volume", Algorithm => "Algorithm", Feedback => "Feedback", OscSync => "Osc Sync",
    PitchR1 => "Pitch R1", PitchR2 => "Pitch R2", PitchR3 => "Pitch R3", PitchR4 => "Pitch R4",
    PitchL1 => "Pitch L1", PitchL2 => "Pitch L2", PitchL3 => "Pitch L3", PitchL4 => "Pitch L4",
    LfoSpeed => "LFO Speed", LfoDelay => "LFO Delay", LfoPmd => "LFO Pitch Depth",
    LfoAmd => "LFO Amp Depth", LfoSync => "LFO Sync", LfoWave => "LFO Wave",
    PitchModSens => "Pitch Mod Sens", Transpose => "Transpose",
    Mono => "Mono", Glide => "Glide", Vintage => "Vintage",
    FilterOn => "Filter", FilterMode => "Filter Mode", Cutoff => "Cutoff", Resonance => "Resonance",
    ChorusRate => "Chorus Rate", ChorusDepth => "Chorus Depth", ChorusMix => "Chorus Mix",
    PhaserRate => "Phaser Rate", PhaserDepth => "Phaser Depth", PhaserFeedback => "Phaser Feedback",
    PhaserMix => "Phaser Mix",
});
param_enum!(OpParam {
    R1 => "Rate 1", R2 => "Rate 2", R3 => "Rate 3", R4 => "Rate 4",
    L1 => "Level 1", L2 => "Level 2", L3 => "Level 3", L4 => "Level 4",
    BreakPoint => "Break Point", LeftDepth => "Left Depth", RightDepth => "Right Depth",
    LeftCurve => "Left Curve", RightCurve => "Right Curve", RateScaling => "Rate Scaling",
    AmpModSens => "Amp Mod Sens", VelSens => "Velocity", Level => "Level",
    FixedFreq => "Fixed Freq", Coarse => "Coarse", Fine => "Fine", Detune => "Detune", On => "On",
});
```

Add the variant at the end of `ParamId`: `FmOp(u8, OpParam),`.

Change the `ParamId::Fm(p)` arm of `info()`, and add the `FmOp` arm:

```rust
            ParamId::Fm(p) => {
                use FmParam as F;
                let n = p.name();
                let of99 = |x: f32| x / 99.0;
                match p {
                    F::Volume => cont(n, 0.7),
                    F::Algorithm | F::Feedback | F::LfoDelay | F::LfoPmd | F::LfoAmd => cont(n, 0.0),
                    F::OscSync | F::LfoSync => disc(n, 1.0, 2),
                    F::PitchR1 | F::PitchR2 | F::PitchR3 | F::PitchR4 => cont(n, 1.0),
                    F::PitchL1 | F::PitchL2 | F::PitchL3 | F::PitchL4 => cont(n, of99(50.0)),
                    F::LfoSpeed => cont(n, of99(35.0)),
                    F::LfoWave => disc(n, 0.0, fm_steps::LFO_WAVES as u8),
                    F::PitchModSens => cont(n, 3.0 / 7.0),
                    F::Transpose => cont(n, 0.5),
                    F::Mono | F::Vintage | F::FilterOn | F::FilterMode => disc(n, 0.0, 2),
                    F::Glide | F::Resonance | F::ChorusMix | F::PhaserMix => cont(n, 0.0),
                    F::Cutoff => cont(n, 1.0),
                    F::ChorusRate | F::PhaserRate | F::PhaserFeedback => cont(n, 0.3),
                    F::ChorusDepth | F::PhaserDepth => cont(n, 0.5),
                }
            }
            ParamId::FmOp(op, p) => {
                use OpParam as O;
                let n = p.name();
                match p {
                    O::R1 | O::R2 | O::R3 | O::R4 | O::L1 | O::L2 | O::L3 => cont(n, 1.0),
                    O::L4 | O::LeftDepth | O::RightDepth | O::Fine => cont(n, 0.0),
                    O::RateScaling | O::AmpModSens | O::VelSens => cont(n, 0.0),
                    O::BreakPoint => cont(n, 39.0 / 99.0),
                    O::LeftCurve | O::RightCurve => disc(n, 0.0, fm_steps::CURVES as u8),
                    O::Level => cont(n, if op == 0 { 1.0 } else { 0.0 }),
                    O::FixedFreq => disc(n, 0.0, 2),
                    O::Coarse => cont(n, 1.0 / 31.0),
                    O::Detune => cont(n, 0.5),
                    O::On => disc(n, 1.0, 2),
                }
            }
```

In `is_valid()`, add `ParamId::FmOp(op, _) => (op as usize) < FM_OPS,`.

In `index()`, after `sampler_base`:

```rust
        let fm_op_base = sampler_base + SamplerParam::ALL.len();
```

and add the arm:

```rust
            ParamId::FmOp(op, p) => fm_op_base + op as usize * OpParam::ALL.len() + p as usize,
```

In `all()`, after the sampler line:

```rust
        for op in 0..FM_OPS as u8 {
            v.extend(OpParam::ALL.iter().map(|&p| ParamId::FmOp(op, p)));
        }
```

- [ ] **Step 4: Implement the UI text** in `crates/app/src/ui/params_ui.rs`

Import `FmParam`, `OpParam`, `fm_steps` and `step_index` from `acid_engine::params`. Replace `discrete_names` with:

```rust
/// Display names for each position of a named switch; empty for a knob.
pub fn discrete_names(id: ParamId) -> &'static [&'static str] {
    match id {
        ParamId::Synth(_, SynthParam::Wave) => &["Saw", "Pulse"],
        ParamId::Synth(_, SynthParam::FilterMode) => &["LP", "BP", "HP"],
        ParamId::Fx(FxParam::DelayDivision) => &["1/16", "1/8", "3/16", "1/4", "3/8", "1/2"],
        ParamId::Fm(FmParam::LfoWave) => &["Tri", "Saw Dn", "Saw Up", "Square", "Sine", "S&H"],
        ParamId::Fm(FmParam::Mono) => &["Poly", "Mono"],
        ParamId::Fm(FmParam::FilterMode) => &["LP", "HP"],
        ParamId::FmOp(_, OpParam::LeftCurve | OpParam::RightCurve) => {
            &["-Lin", "-Exp", "+Exp", "+Lin"]
        }
        ParamId::FmOp(_, OpParam::FixedFreq) => &["Ratio", "Fixed"],
        _ if id.info().steps == 2 => &["Off", "On"],
        _ => &[],
    }
}

/// The value a knob shows in its tooltip: DX7 units for FM controls, a
/// percentage otherwise.
pub fn value_text(id: ParamId, v: f32) -> String {
    use FmParam as F;
    use OpParam as O;
    let pick = |n: usize| step_index(v, n as u8) as i32;
    let signed = |x: i32| if x > 0 { format!("+{x}") } else { x.to_string() };
    match id {
        ParamId::Fm(F::Algorithm) => (pick(fm_steps::ALGORITHM) + 1).to_string(),
        ParamId::Fm(F::Feedback) => pick(fm_steps::FEEDBACK).to_string(),
        ParamId::Fm(F::PitchModSens) => pick(fm_steps::PITCH_MOD_SENS).to_string(),
        ParamId::Fm(F::Transpose) => signed(pick(fm_steps::TRANSPOSE) - 24),
        ParamId::FmOp(_, O::Coarse) => pick(fm_steps::COARSE).to_string(),
        ParamId::FmOp(_, O::Detune) => signed(pick(fm_steps::DETUNE) - 7),
        ParamId::FmOp(_, O::RateScaling) => pick(fm_steps::RATE_SCALING).to_string(),
        ParamId::FmOp(_, O::AmpModSens) => pick(fm_steps::AMP_MOD_SENS).to_string(),
        ParamId::FmOp(_, O::VelSens) => pick(fm_steps::VEL_SENS).to_string(),
        ParamId::Fm(
            F::PitchR1 | F::PitchR2 | F::PitchR3 | F::PitchR4 | F::PitchL1 | F::PitchL2
            | F::PitchL3 | F::PitchL4 | F::LfoSpeed | F::LfoDelay | F::LfoPmd | F::LfoAmd,
        )
        | ParamId::FmOp(_, _) => format!("{:.0}", v.clamp(0.0, 1.0) * 99.0),
        _ => format!("{:.0}%", v * 100.0),
    }
}
```

In `param_knob`:
- Change the switch condition to `if info.steps >= 2 && !discrete_names(id).is_empty() {`.
- Change the tooltip to use `value_text`:

```rust
        let tip = format!(
            "{} {}{}  (drag, Shift = fine, double-click = reset)",
            info.name,
            value_text(id, value),
            if automated { "  \u{b7} automated in song" } else { "" }
        );
```

In `param_label`, add the arm `ParamId::FmOp(op, p) => format!("FM OP{} {}", op + 1, p.name()),`.

- [ ] **Step 5: Fix the remaining compile errors**

The engine's `apply_param` match is not exhaustive over the new variant only if it lacks a catch-all. It has `_ => self.mixer.set_param(id, v)`, and the mixer ignores unknown ids, so FM operator params reach nothing yet; Task 6 wires them. Run `cargo build --workspace --all-targets` and fix any non-exhaustive `match` on `ParamId` with an explicit `ParamId::FmOp(..)` arm that does nothing, commented "wired in the FM engine task". `params_ui::param_label` is already covered.

- [ ] **Step 6: Run the whole suite**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

- [ ] **Step 7: Commit**

```bash
git add -A crates
git commit -m "feat(engine): FM global and operator params

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Algorithms and DX7 envelope math

**Files:**
- Create: `crates/acid-engine/src/fm/mod.rs`, `fm/algorithms.rs`, `fm/env.rs`
- Modify: `crates/acid-engine/src/lib.rs` (`pub mod fm;`), `THIRD_PARTY_NOTICES.md`

**Interfaces:**
- Produces:

```rust
// fm/mod.rs
pub const OPS: usize = crate::params::FM_OPS;   // 6
pub const BLOCK: usize = 32;                     // samples per control block
// fm/algorithms.rs
pub struct Algorithm { pub mods: [u8; 6], pub carriers: u8, pub feedback: usize }
pub const ALGORITHMS: [Algorithm; 32];
// fm/env.rs
pub fn scale_out_level(x: f32) -> f32;
pub fn scale_level(note: i32, break_point: i32, left_depth: i32, right_depth: i32, left_curve: i32, right_curve: i32) -> i32;
pub fn scale_rate(note: i32, sensitivity: i32) -> i32;
pub fn scale_velocity(velocity: i32, sensitivity: i32) -> i32;
pub fn operator_out_level(level: f32, level_scaling: i32, velocity_scaling: i32) -> f32;
pub struct Env;  // new(sr), start(rates, levels, out_level, rate_scaling), release(),
                 // set_out_level(ol), tick() -> f32 (log2 amplitude, 0 = full), is_finished(), is_down()
```

In `mods[op]`, bit `m` set means OP(m+1) modulates OP(op+1). Operators are indexed 0..6 for OP1..OP6. Every modulator has a higher index than its target, so evaluating from OP6 down to OP1 is always valid.

- [ ] **Step 1: Create the module skeleton and the failing tests**

`fm/mod.rs`:

```rust
//! Six-operator FM synth in the style of the DX7 / M-VAVE FM-1:
//! 32 algorithms, DX7 envelopes and scaling, LFO, pitch EG, 12-voice
//! poly or mono+glide, filter/chorus/phaser, and DX7 SysEx voices.

// DSP loops index several parallel per-operator arrays at once.
#![allow(clippy::needless_range_loop)]

pub mod algorithms;
pub mod env;

/// Operators per voice.
pub const OPS: usize = crate::params::FM_OPS;
/// Samples per control block: envelopes, LFO and pitch update once per block.
pub const BLOCK: usize = 32;
```

Add `pub mod fm;` to `lib.rs`, in alphabetical order.

`fm/algorithms.rs`, tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carrier_counts_match_the_dx7() {
        let counts: Vec<u32> = ALGORITHMS.iter().map(|a| a.carriers.count_ones()).collect();
        assert_eq!(
            counts,
            [2, 2, 2, 2, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 3, 3, 4, 4, 4, 5, 5, 3, 3, 3, 4, 4, 5, 6]
        );
    }

    #[test]
    fn modulators_always_sit_above_their_targets() {
        for (i, a) in ALGORITHMS.iter().enumerate() {
            for op in 0..6 {
                let lower = (1u16 << (op + 1)) - 1; // bits 0..=op
                assert_eq!(a.mods[op] as u16 & lower, 0, "algorithm {}", i + 1);
            }
            assert!(a.feedback < 6);
        }
    }

    #[test]
    fn every_operator_is_heard_or_modulates() {
        for (i, a) in ALGORITHMS.iter().enumerate() {
            let modulates = a.mods.iter().fold(0u8, |acc, m| acc | m);
            assert_eq!(modulates & a.carriers, 0, "carriers never modulate (alg {})", i + 1);
            assert_eq!(modulates | a.carriers, 0b11_1111, "algorithm {}", i + 1);
        }
    }

    #[test]
    fn spot_checks() {
        let a5 = ALGORITHMS[4];
        assert_eq!(a5.carriers, 0b01_0101);
        assert_eq!(a5.mods[0], 0b00_0010);
        assert_eq!(a5.mods[4], 0b10_0000);
        assert_eq!(a5.feedback, 5);
        let a16 = ALGORITHMS[15];
        assert_eq!(a16.mods[0], 0b01_0110, "OP2, OP3 and OP5 into OP1");
        assert_eq!(ALGORITHMS[31].mods, [0; 6]);
        assert_eq!(ALGORITHMS[17].feedback, 2, "algorithm 18 feeds back on OP3");
    }
}
```

`fm/env.rs`, tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn blocks(seconds: f32) -> usize {
        (seconds * SR / BLOCK as f32).ceil() as usize
    }

    fn full_env(rates: [f32; 4], levels: [f32; 4]) -> Env {
        let mut e = Env::new(SR);
        e.start(rates, levels, operator_out_level(99.0, 0, 0), 0);
        e
    }

    #[test]
    fn output_level_scale() {
        assert_eq!(scale_out_level(99.0), 127.0);
        assert_eq!(scale_out_level(20.0), 48.0);
        assert_eq!(scale_out_level(0.0), 0.0);
        let mut last = -1.0;
        for i in 0..=990 {
            let s = scale_out_level(i as f32 / 10.0);
            assert!(s >= last);
            last = s;
        }
    }

    #[test]
    fn fast_attack_reaches_full_level_and_sustains() {
        let mut e = full_env([99.0; 4], [99.0, 99.0, 99.0, 0.0]);
        let mut level = f32::MIN;
        for _ in 0..blocks(0.02) {
            level = e.tick();
        }
        assert!(level.abs() < 0.05, "level {level}");
        for _ in 0..blocks(1.0) {
            level = e.tick();
        }
        assert!(level.abs() < 0.05, "sustains at L3");
        assert!(e.is_down() && !e.is_finished());
    }

    #[test]
    fn release_falls_silent_and_finishes() {
        let mut e = full_env([99.0; 4], [99.0, 99.0, 99.0, 0.0]);
        for _ in 0..blocks(0.05) {
            e.tick();
        }
        e.release();
        let mut level = 0.0;
        for _ in 0..blocks(0.05) {
            level = e.tick();
        }
        assert!(level < -12.0, "level {level}");
        assert!(e.is_finished());
    }

    #[test]
    fn slower_rates_take_longer() {
        let attack_blocks = |rate: f32| {
            let mut e = full_env([rate, 99.0, 99.0, 99.0], [99.0, 99.0, 99.0, 0.0]);
            (0..blocks(30.0)).position(|_| e.tick() > -0.1).unwrap()
        };
        assert!(attack_blocks(50.0) > attack_blocks(70.0));
        assert!(attack_blocks(70.0) > attack_blocks(99.0));
    }

    #[test]
    fn a_zero_level_is_silent() {
        let mut e = Env::new(SR);
        e.start([99.0; 4], [99.0, 99.0, 99.0, 0.0], operator_out_level(0.0, 0, 0), 0);
        let level = (0..blocks(0.05)).map(|_| e.tick()).last().unwrap();
        assert!(level < -13.0, "level {level}");
    }

    #[test]
    fn raising_the_output_level_while_sustaining_takes_effect() {
        let mut e = Env::new(SR);
        e.start([99.0; 4], [99.0, 99.0, 99.0, 0.0], operator_out_level(50.0, 0, 0), 0);
        let quiet = (0..blocks(0.1)).map(|_| e.tick()).last().unwrap();
        e.set_out_level(operator_out_level(99.0, 0, 0));
        let loud = e.tick();
        assert!(loud > quiet + 1.0, "{quiet} -> {loud}");
    }

    #[test]
    fn scaling_helpers() {
        assert_eq!(scale_rate(60, 0), 0);
        assert_eq!(scale_rate(127, 7), 27);
        assert_eq!(scale_velocity(127, 0), 0);
        assert_eq!(scale_velocity(127, 7), 224);
        assert!(scale_velocity(40, 7) < scale_velocity(100, 7));
        assert_eq!(scale_level(100, 39, 0, 0, 0, 0), 0);
        assert!(scale_level(100, 39, 0, 99, 0, 3) > 0, "+LIN raises above the break point");
        assert!(scale_level(100, 39, 0, 99, 0, 0) < 0, "-LIN lowers above the break point");
        assert!(scale_level(20, 39, 99, 0, 1, 0) < 0, "-EXP lowers below the break point");
        assert_eq!(scale_level(20, 39, 99, 0, 1, 0), scale_level(20, 39, 99, 99, 1, 3), "right side unused below");
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine fm::`
Expected: compile errors.

- [ ] **Step 3: Implement `algorithms.rs`** (above the tests)

```rust
//! The 32 DX7 algorithms: which operators modulate which, which are
//! carriers, and which operator has the feedback loop. Operators are
//! indexed 0..6 for OP1..OP6. Every modulator has a higher index than the
//! operator it modulates, so evaluating OP6 down to OP1 is always valid.
//! Algorithms 4 and 6 loop through several operators on the DX7; here
//! they use self-feedback on OP6.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Algorithm {
    /// Bit `m` of `mods[op]` set = OP(m+1) modulates OP(op+1).
    pub mods: [u8; 6],
    /// Bit `op` set = OP(op+1) is a carrier (is heard).
    pub carriers: u8,
    /// Index of the operator with self-feedback.
    pub feedback: usize,
}

/// `edges` are (modulator, target) and `carriers`/`feedback` are DX7
/// operator numbers (1..=6).
const fn alg(edges: &[(usize, usize)], carriers: &[usize], feedback: usize) -> Algorithm {
    let mut mods = [0u8; 6];
    let mut i = 0;
    while i < edges.len() {
        let (m, t) = edges[i];
        mods[t - 1] |= 1 << (m - 1);
        i += 1;
    }
    let mut car = 0u8;
    let mut j = 0;
    while j < carriers.len() {
        car |= 1 << (carriers[j] - 1);
        j += 1;
    }
    Algorithm {
        mods,
        carriers: car,
        feedback: feedback - 1,
    }
}

pub const ALGORITHMS: [Algorithm; 32] = [
    alg(&[(2, 1), (4, 3), (5, 4), (6, 5)], &[1, 3], 6),
    alg(&[(2, 1), (4, 3), (5, 4), (6, 5)], &[1, 3], 2),
    alg(&[(2, 1), (3, 2), (5, 4), (6, 5)], &[1, 4], 6),
    alg(&[(2, 1), (3, 2), (5, 4), (6, 5)], &[1, 4], 6),
    alg(&[(2, 1), (4, 3), (6, 5)], &[1, 3, 5], 6),
    alg(&[(2, 1), (4, 3), (6, 5)], &[1, 3, 5], 6),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 5)], &[1, 3], 6),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 5)], &[1, 3], 4),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 5)], &[1, 3], 2),
    alg(&[(2, 1), (3, 2), (5, 4), (6, 4)], &[1, 4], 3),
    alg(&[(2, 1), (3, 2), (5, 4), (6, 4)], &[1, 4], 6),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 3)], &[1, 3], 2),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 3)], &[1, 3], 6),
    alg(&[(2, 1), (4, 3), (5, 4), (6, 4)], &[1, 3], 6),
    alg(&[(2, 1), (4, 3), (5, 4), (6, 4)], &[1, 3], 2),
    alg(&[(2, 1), (3, 1), (4, 3), (5, 1), (6, 5)], &[1], 6),
    alg(&[(2, 1), (3, 1), (4, 3), (5, 1), (6, 5)], &[1], 2),
    alg(&[(2, 1), (3, 1), (4, 1), (5, 4), (6, 5)], &[1], 3),
    alg(&[(2, 1), (3, 2), (6, 4), (6, 5)], &[1, 4, 5], 6),
    alg(&[(3, 1), (3, 2), (5, 4), (6, 4)], &[1, 2, 4], 3),
    alg(&[(3, 1), (3, 2), (6, 4), (6, 5)], &[1, 2, 4, 5], 3),
    alg(&[(2, 1), (6, 3), (6, 4), (6, 5)], &[1, 3, 4, 5], 6),
    alg(&[(3, 2), (6, 4), (6, 5)], &[1, 2, 4, 5], 6),
    alg(&[(6, 3), (6, 4), (6, 5)], &[1, 2, 3, 4, 5], 6),
    alg(&[(6, 4), (6, 5)], &[1, 2, 3, 4, 5], 6),
    alg(&[(3, 2), (5, 4), (6, 4)], &[1, 2, 4], 6),
    alg(&[(3, 2), (5, 4), (6, 4)], &[1, 2, 4], 3),
    alg(&[(2, 1), (4, 3), (5, 4)], &[1, 3, 6], 5),
    alg(&[(4, 3), (6, 5)], &[1, 2, 3, 5], 6),
    alg(&[(4, 3), (5, 4)], &[1, 2, 3, 6], 5),
    alg(&[(6, 5)], &[1, 2, 3, 4, 5], 6),
    alg(&[], &[1, 2, 3, 4, 5, 6], 6),
];
```

- [ ] **Step 4: Implement `env.rs`** (above the tests)

```rust
//! DX7 operator envelope plus output-level, keyboard and velocity
//! scaling. The math follows the published reverse-engineering of the
//! DX7 in music-synthesizer-for-android (Apache-2.0, used by Dexed); see
//! THIRD_PARTY_NOTICES.md. Envelope levels live in a log2 domain: one
//! unit doubles the amplitude, and `tick` returns 0.0 at full scale.

use super::BLOCK;

const LEVEL_LUT: [f32; 20] = [
    0.0, 5.0, 9.0, 13.0, 17.0, 20.0, 23.0, 25.0, 27.0, 29.0, 31.0, 33.0, 35.0, 37.0, 39.0, 41.0,
    42.0, 43.0, 45.0, 46.0,
];
const VELOCITY_DATA: [u8; 64] = [
    0, 70, 86, 97, 106, 114, 121, 126, 132, 138, 142, 148, 152, 156, 160, 163, 166, 170, 173, 174,
    178, 181, 184, 186, 189, 190, 194, 196, 198, 200, 202, 205, 206, 209, 211, 214, 216, 218, 220,
    222, 224, 225, 227, 229, 230, 232, 233, 235, 237, 238, 240, 241, 242, 243, 244, 246, 246, 248,
    249, 250, 251, 252, 253, 254,
];
const EXP_SCALE: [u8; 33] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 14, 16, 19, 23, 27, 33, 39, 47, 56, 66, 80, 94, 110, 126,
    142, 158, 174, 190, 206, 222, 238, 250,
];
/// Envelope units at full amplitude: L=99 at output level 99 gives
/// (63 * 64 + 127 * 32 - 4256) / 256 = 15.0.
const FULL: f32 = 15.0;
/// Where a rising stage jumps to first (the DX7's fast attack onset).
const ATTACK_JUMP: f32 = 1716.0 / 256.0;

/// A DX7 0..99 level on the internal 0..127 scale (continuous).
pub fn scale_out_level(x: f32) -> f32 {
    let x = x.clamp(0.0, 99.0);
    if x >= 20.0 {
        return 28.0 + x;
    }
    let i = x.floor() as usize;
    let next = if i + 1 < LEVEL_LUT.len() { LEVEL_LUT[i + 1] } else { 48.0 };
    LEVEL_LUT[i] + (next - LEVEL_LUT[i]) * (x - i as f32)
}

fn scale_curve(group: i32, depth: i32, curve: i32) -> i32 {
    let scale = if curve == 0 || curve == 3 {
        (group * depth * 329) >> 12
    } else {
        let raw = EXP_SCALE[(group.max(0) as usize).min(EXP_SCALE.len() - 1)] as i32;
        (raw * depth * 329) >> 15
    };
    if curve < 2 { -scale } else { scale }
}

/// Keyboard level scaling for MIDI `note`. Curves: 0 -LIN, 1 -EXP, 2 +EXP, 3 +LIN.
pub fn scale_level(
    note: i32,
    break_point: i32,
    left_depth: i32,
    right_depth: i32,
    left_curve: i32,
    right_curve: i32,
) -> i32 {
    let offset = note - break_point - 17;
    if offset >= 0 {
        scale_curve((offset + 1) / 3, right_depth, right_curve)
    } else {
        scale_curve(-(offset - 1) / 3, left_depth, left_curve)
    }
}

/// Keyboard rate scaling: added to the envelope's rate index.
pub fn scale_rate(note: i32, sensitivity: i32) -> i32 {
    let x = (note / 3 - 7).clamp(0, 31);
    (sensitivity * x) >> 3
}

/// Velocity sensitivity, in output-level units.
pub fn scale_velocity(velocity: i32, sensitivity: i32) -> i32 {
    let v = VELOCITY_DATA[(velocity.clamp(0, 127) >> 1) as usize] as i32 - 239;
    ((sensitivity * v + 7) >> 3) << 4
}

/// An operator's output level in envelope units, from its 0..99 level
/// and its keyboard and velocity scaling.
pub fn operator_out_level(level: f32, level_scaling: i32, velocity_scaling: i32) -> f32 {
    let ol = (scale_out_level(level) + level_scaling as f32).min(127.0) * 32.0;
    (ol + velocity_scaling as f32).max(0.0)
}

/// A DX7 four-rate, four-level envelope, advanced once per `BLOCK`.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    rates: [f32; 4],
    levels: [f32; 4],
    out_level: f32,
    rate_scaling: i32,
    lvl: f32,
    target: f32,
    stage: usize,
    rising: bool,
    inc: f32,
    down: bool,
    /// Envelope units per raw DX7 increment, for one block at this rate.
    inc_scale: f32,
}

impl Env {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            rates: [99.0; 4],
            levels: [99.0, 99.0, 99.0, 0.0],
            out_level: 0.0,
            rate_scaling: 0,
            lvl: 0.0,
            target: 0.0,
            stage: 4,
            rising: false,
            inc: 0.0,
            down: false,
            inc_scale: BLOCK as f32 / 64.0 * 44_100.0 / sample_rate / (1u32 << 24) as f32,
        }
    }

    /// Key down: starts the attack from silence.
    pub fn start(&mut self, rates: [f32; 4], levels: [f32; 4], out_level: f32, rate_scaling: i32) {
        self.rates = rates;
        self.levels = levels;
        self.out_level = out_level;
        self.rate_scaling = rate_scaling;
        self.lvl = 0.0;
        self.down = true;
        self.advance(0);
    }

    /// Key up: moves to the release stage.
    pub fn release(&mut self) {
        if self.down {
            self.down = false;
            self.advance(3);
        }
    }

    /// A new output level (a knob moved while the note plays).
    pub fn set_out_level(&mut self, out_level: f32) {
        self.out_level = out_level;
        if self.down && self.stage == 3 {
            self.lvl = self.actual(self.levels[2]);
        } else if self.stage < 4 {
            self.target = self.actual(self.levels[self.stage]);
            self.rising = self.target > self.lvl;
        }
    }

    pub fn is_down(&self) -> bool {
        self.down
    }

    /// Released and through its release stage.
    pub fn is_finished(&self) -> bool {
        !self.down && self.stage >= 4
    }

    fn actual(&self, level: f32) -> f32 {
        let a = (scale_out_level(level) * 0.5).floor() * 64.0 + self.out_level - 4256.0;
        a.max(16.0) / 256.0
    }

    fn advance(&mut self, stage: usize) {
        self.stage = stage;
        if stage < 4 {
            self.target = self.actual(self.levels[stage]);
            self.rising = self.target > self.lvl;
            let q = ((self.rates[stage] * 41.0 / 64.0).floor() as i32 + self.rate_scaling).clamp(0, 63);
            let raw = (4 + (q & 3)) as f32 * 2f32.powi(2 + 6 + (q >> 2));
            self.inc = raw * self.inc_scale;
        }
    }

    /// Advances one block. Returns the level as log2 amplitude (0.0 = full scale).
    pub fn tick(&mut self) -> f32 {
        if self.stage < 3 || (self.stage < 4 && !self.down) {
            if self.rising {
                if self.lvl < ATTACK_JUMP {
                    self.lvl = ATTACK_JUMP;
                }
                self.lvl += (17.0 - self.lvl.floor()) * self.inc;
                if self.lvl >= self.target {
                    self.lvl = self.target;
                    self.advance(self.stage + 1);
                }
            } else {
                self.lvl -= self.inc;
                if self.lvl <= self.target {
                    self.lvl = self.target;
                    self.advance(self.stage + 1);
                }
            }
        }
        self.lvl - FULL
    }
}
```

Add `use super::BLOCK;` to the env tests module if `super::*` does not bring it in. It does, through the `use`.

- [ ] **Step 5: Add the attribution** to `THIRD_PARTY_NOTICES.md`:

```markdown

## DX7 envelope and scaling math

`crates/acid-engine/src/fm/env.rs` follows the DX7 envelope, output-level,
keyboard-scaling and velocity tables published in Google's
music-synthesizer-for-android (https://github.com/google/music-synthesizer-for-android),
used under the Apache License, Version 2.0
(https://www.apache.org/licenses/LICENSE-2.0). The code here is a new Rust
implementation; the tables and formulas are from that project.
```

- [ ] **Step 6: Run the tests, then the whole suite**

Run: `cargo test -p acid-engine fm::`, then `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings. Add `#[allow(dead_code)]` on nothing; every item here is `pub`.

- [ ] **Step 7: Commit**

```bash
git add -A crates THIRD_PARTY_NOTICES.md
git commit -m "feat(fm): DX7 algorithms and envelope math

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Patch decoding, LFO, pitch EG and sine table

**Files:**
- Create: `crates/acid-engine/src/fm/patch.rs`, `fm/modulation.rs`, `fm/sine.rs`
- Modify: `crates/acid-engine/src/fm/mod.rs` (`pub mod modulation; pub mod patch; pub mod sine;`)

**Interfaces:**
- Consumes: `FmParam`, `OpParam`, `fm_steps`, `step_index`, `on`, `exp` (Task 1).
- Produces:

```rust
// patch.rs
pub fn d99(v: f32) -> f32;                         // knob → 0..99
pub fn pick(v: f32, n: usize) -> usize;            // knob → 0..n
pub struct OpPatch { pub rates: [f32;4], pub levels: [f32;4], pub break_point: i32, pub left_depth: i32,
    pub right_depth: i32, pub left_curve: i32, pub right_curve: i32, pub rate_scaling: i32,
    pub amp_mod_sens: usize, pub vel_sens: i32, pub level: f32, pub fixed: bool, pub coarse: i32,
    pub fine: f32, pub detune: i32 /* -7..=7 */, pub on: bool }
impl OpPatch { pub fn set(&mut self, p: OpParam, v: f32); pub fn freq(&self, note_hz: f32) -> f32; }
pub struct FmPatch { pub ops: [OpPatch; 6], pub algorithm: usize, pub feedback: i32, pub osc_sync: bool,
    pub pitch_rates: [f32;4], pub pitch_levels: [f32;4], pub lfo_speed: f32, pub lfo_delay: f32,
    pub lfo_pmd: f32, pub lfo_amd: f32, pub lfo_sync: bool, pub lfo_wave: usize, pub pms: usize,
    pub transpose: i32, pub volume: f32, pub mono: bool, pub glide: f32 /* seconds */, pub vintage: bool,
    pub filter_on: bool, pub filter_hp: bool, pub cutoff: f32, pub resonance: f32,
    pub chorus_rate: f32, pub chorus_depth: f32, pub chorus_mix: f32,
    pub phaser_rate: f32, pub phaser_depth: f32, pub phaser_feedback: f32, pub phaser_mix: f32 }
impl FmPatch { pub fn set(&mut self, p: FmParam, v: f32); pub fn set_op(&mut self, op: usize, p: OpParam, v: f32); }
impl Default for FmPatch  // every FM param at its default = INIT VOICE
// modulation.rs
pub const PMS_SEMITONES: [f32; 8]; pub const AMS_DEPTH: [f32; 4];
pub fn lfo_hz(speed99: f32) -> f32; pub fn lfo_delay_seconds(delay99: f32) -> f32;
pub fn lfo_delay_gain(t: f32, delay_s: f32) -> f32;
pub struct Lfo; impl Lfo { pub fn new() -> Self; pub fn reset(&mut self); pub fn tick(&mut self, hz: f32, wave: usize, seconds: f32) -> f32 }
pub fn pitch_level_semitones(l99: f32) -> f32; pub fn pitch_rate(r99: f32) -> f32;
pub struct PitchEnv; impl PitchEnv { pub fn new() -> Self; pub fn start(&mut self, levels: &[f32;4]);
    pub fn release(&mut self); pub fn tick(&mut self, rates: &[f32;4], levels: &[f32;4], seconds: f32) -> f32 }
// sine.rs
pub struct Sine; impl Sine { pub fn new() -> Self; pub fn at(&self, phase_cycles: f32, vintage: bool) -> f32 }
```

- [ ] **Step 1: Write the failing tests**

`patch.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_patch_is_the_init_voice() {
        let p = FmPatch::default();
        assert_eq!(p.algorithm, 0);
        assert_eq!(p.ops[0].level, 99.0);
        assert_eq!(p.ops[1].level, 0.0);
        assert_eq!(p.ops[3].coarse, 1);
        assert_eq!(p.ops[3].detune, 0);
        assert!(p.ops.iter().all(|o| o.on && !o.fixed));
        assert_eq!(p.ops[2].rates, [99.0; 4]);
        assert_eq!(p.ops[2].levels, [99.0, 99.0, 99.0, 0.0]);
        assert_eq!(p.ops[0].break_point, 39);
        assert!(p.osc_sync && p.lfo_sync && !p.mono && !p.vintage);
        assert_eq!((p.transpose, p.pms, p.lfo_wave), (0, 3, 0));
        assert!((p.pitch_levels[0] - 50.0).abs() < 1e-3);
    }

    #[test]
    fn knobs_decode_to_dx7_units() {
        let mut p = FmPatch::default();
        p.set(FmParam::Algorithm, 1.0);
        p.set(FmParam::Transpose, 0.0);
        p.set(FmParam::Feedback, 1.0);
        assert_eq!((p.algorithm, p.transpose, p.feedback), (31, -24, 7));
        p.set_op(2, OpParam::Coarse, 1.0);
        p.set_op(2, OpParam::Detune, 1.0);
        p.set_op(2, OpParam::LeftCurve, 1.0);
        p.set_op(2, OpParam::On, 0.0);
        let o = p.ops[2];
        assert_eq!((o.coarse, o.detune, o.left_curve, o.on), (31, 7, 3, false));
        p.set(FmParam::Glide, 1.0);
        assert!((p.glide - 2.0).abs() < 1e-6);
        p.set_op(9, OpParam::Level, 1.0); // out of range: ignored
    }

    #[test]
    fn operator_frequencies() {
        let mut o = FmPatch::default().ops[0];
        assert!((o.freq(440.0) - 440.0).abs() < 1e-3);
        o.coarse = 0;
        assert!((o.freq(440.0) - 220.0).abs() < 1e-3);
        o.coarse = 1;
        o.fine = 50.0;
        assert!((o.freq(440.0) - 660.0).abs() < 1e-3);
        o.fine = 0.0;
        o.detune = 7;
        assert!((o.freq(440.0) - 440.0 * 2f32.powf(7.0 / 1200.0)).abs() < 1e-3);
        o.fixed = true;
        o.coarse = 2;
        o.fine = 0.0;
        assert!((o.freq(440.0) - 100.0).abs() < 1e-2, "fixed: ignores the note");
        o.coarse = 1;
        o.fine = 50.0;
        assert!((o.freq(55.0) - 10f32.powf(1.5)).abs() < 1e-2);
    }
}
```

`modulation.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lfo_waves_stay_in_range_and_have_their_shape() {
        for wave in 0..6 {
            let mut l = Lfo::new();
            for _ in 0..1000 {
                let v = l.tick(3.0, wave, 0.001);
                assert!((-1.0..=1.0).contains(&v), "wave {wave}: {v}");
            }
        }
        let mut sq = Lfo::new();
        assert_eq!(sq.tick(1.0, 3, 0.1), 1.0);
        assert_eq!(sq.tick(1.0, 3, 0.5), -1.0);
        let mut sh = Lfo::new();
        sh.tick(1.0, 5, 1.0);
        let held = sh.tick(1.0, 5, 0.1);
        assert_eq!(sh.tick(1.0, 5, 0.1), held, "S&H holds within a cycle");
    }

    #[test]
    fn lfo_speed_and_delay_maps() {
        assert!((lfo_hz(0.0) - 0.062).abs() < 1e-3);
        assert!(lfo_hz(99.0) > 45.0 && lfo_hz(99.0) < 60.0);
        assert!(lfo_hz(50.0) > lfo_hz(40.0));
        assert_eq!(lfo_delay_seconds(0.0), 0.0);
        assert_eq!(lfo_delay_gain(0.0, 0.0), 1.0);
        assert_eq!(lfo_delay_gain(0.1, 1.0), 0.0);
        assert_eq!(lfo_delay_gain(2.0, 1.0), 1.0);
    }

    #[test]
    fn flat_pitch_envelope_does_nothing() {
        let mut p = PitchEnv::new();
        let levels = [50.0; 4];
        p.start(&levels);
        for _ in 0..100 {
            assert_eq!(p.tick(&[99.0; 4], &levels, 0.01), 0.0);
        }
    }

    #[test]
    fn pitch_envelope_moves_from_l4_through_l1_and_releases() {
        let mut p = PitchEnv::new();
        let levels = [50.0, 50.0, 50.0, 75.0];
        p.start(&levels);
        let start = p.tick(&[99.0; 4], &levels, 0.0);
        assert!((start - 24.0).abs() < 0.01, "starts at L4: {start}");
        let later = (0..100).map(|_| p.tick(&[99.0; 4], &levels, 0.01)).last().unwrap();
        assert_eq!(later, 0.0, "sustains at L3");
        p.release();
        let released = (0..100).map(|_| p.tick(&[99.0; 4], &levels, 0.01)).last().unwrap();
        assert!((released - 24.0).abs() < 0.01, "releases to L4");
    }
}
```

`sine.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_sine_is_accurate_and_vintage_is_coarse() {
        let s = Sine::new();
        for i in 0..1000 {
            let ph = i as f32 / 997.0 - 3.0;
            let exact = (ph * std::f32::consts::TAU).sin();
            assert!((s.at(ph, false) - exact).abs() < 2e-5, "phase {ph}");
            assert!((s.at(ph, true) - exact).abs() < 0.01);
        }
        assert_eq!(s.at(0.25, false), 1.0);
        assert_eq!(s.at(0.1, true), s.at(0.1002, true), "vintage steps");
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine fm::`
Expected: compile errors.

- [ ] **Step 3: Implement `patch.rs`**

```rust
//! The FM patch: every FM knob decoded from its 0..1 value into DX7
//! units, plus each operator's frequency.

use super::OPS;
use crate::params::{FmParam, OpParam, ParamId, exp, fm_steps, on, step_index};

/// A 0..1 knob as a DX7 0..99 value.
pub fn d99(v: f32) -> f32 {
    v.clamp(0.0, 1.0) * 99.0
}

/// A 0..1 knob as one of `n` positions.
pub fn pick(v: f32, n: usize) -> usize {
    step_index(v, n as u8)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpPatch {
    pub rates: [f32; 4],
    pub levels: [f32; 4],
    pub break_point: i32,
    pub left_depth: i32,
    pub right_depth: i32,
    pub left_curve: i32,
    pub right_curve: i32,
    pub rate_scaling: i32,
    pub amp_mod_sens: usize,
    pub vel_sens: i32,
    pub level: f32,
    pub fixed: bool,
    pub coarse: i32,
    pub fine: f32,
    pub detune: i32,
    pub on: bool,
}

const BLANK_OP: OpPatch = OpPatch {
    rates: [0.0; 4],
    levels: [0.0; 4],
    break_point: 0,
    left_depth: 0,
    right_depth: 0,
    left_curve: 0,
    right_curve: 0,
    rate_scaling: 0,
    amp_mod_sens: 0,
    vel_sens: 0,
    level: 0.0,
    fixed: false,
    coarse: 0,
    fine: 0.0,
    detune: 0,
    on: false,
};

impl OpPatch {
    pub fn set(&mut self, p: OpParam, v: f32) {
        use OpParam as O;
        match p {
            O::R1 => self.rates[0] = d99(v),
            O::R2 => self.rates[1] = d99(v),
            O::R3 => self.rates[2] = d99(v),
            O::R4 => self.rates[3] = d99(v),
            O::L1 => self.levels[0] = d99(v),
            O::L2 => self.levels[1] = d99(v),
            O::L3 => self.levels[2] = d99(v),
            O::L4 => self.levels[3] = d99(v),
            O::BreakPoint => self.break_point = d99(v).round() as i32,
            O::LeftDepth => self.left_depth = d99(v).round() as i32,
            O::RightDepth => self.right_depth = d99(v).round() as i32,
            O::LeftCurve => self.left_curve = pick(v, fm_steps::CURVES) as i32,
            O::RightCurve => self.right_curve = pick(v, fm_steps::CURVES) as i32,
            O::RateScaling => self.rate_scaling = pick(v, fm_steps::RATE_SCALING) as i32,
            O::AmpModSens => self.amp_mod_sens = pick(v, fm_steps::AMP_MOD_SENS),
            O::VelSens => self.vel_sens = pick(v, fm_steps::VEL_SENS) as i32,
            O::Level => self.level = d99(v),
            O::FixedFreq => self.fixed = on(v),
            O::Coarse => self.coarse = pick(v, fm_steps::COARSE) as i32,
            O::Fine => self.fine = d99(v),
            O::Detune => self.detune = pick(v, fm_steps::DETUNE) as i32 - 7,
            O::On => self.on = on(v),
        }
    }

    /// Frequency in Hz for a note sounding at `note_hz`. Ratio mode:
    /// coarse 0 = ×0.5, fine adds up to +99 %, detune ±7 cents. Fixed
    /// mode: 10^(coarse mod 4 + fine/100) Hz.
    pub fn freq(&self, note_hz: f32) -> f32 {
        if self.fixed {
            10f32.powf(((self.coarse & 3) as f32 * 100.0 + self.fine) / 100.0)
        } else {
            let coarse = if self.coarse == 0 { 0.5 } else { self.coarse as f32 };
            note_hz * coarse * (1.0 + self.fine / 100.0) * 2f32.powf(self.detune as f32 / 1200.0)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FmPatch {
    pub ops: [OpPatch; OPS],
    pub algorithm: usize,
    pub feedback: i32,
    pub osc_sync: bool,
    pub pitch_rates: [f32; 4],
    pub pitch_levels: [f32; 4],
    pub lfo_speed: f32,
    pub lfo_delay: f32,
    pub lfo_pmd: f32,
    pub lfo_amd: f32,
    pub lfo_sync: bool,
    pub lfo_wave: usize,
    pub pms: usize,
    /// Semitones, -24..=24.
    pub transpose: i32,
    /// The volume knob, 0..1 (squared when applied).
    pub volume: f32,
    pub mono: bool,
    /// Glide time in seconds.
    pub glide: f32,
    pub vintage: bool,
    pub filter_on: bool,
    pub filter_hp: bool,
    /// Knob 0..1 (mapped to Hz by the filter).
    pub cutoff: f32,
    pub resonance: f32,
    pub chorus_rate: f32,
    pub chorus_depth: f32,
    pub chorus_mix: f32,
    pub phaser_rate: f32,
    pub phaser_depth: f32,
    pub phaser_feedback: f32,
    pub phaser_mix: f32,
}

const BLANK: FmPatch = FmPatch {
    ops: [BLANK_OP; OPS],
    algorithm: 0,
    feedback: 0,
    osc_sync: false,
    pitch_rates: [0.0; 4],
    pitch_levels: [0.0; 4],
    lfo_speed: 0.0,
    lfo_delay: 0.0,
    lfo_pmd: 0.0,
    lfo_amd: 0.0,
    lfo_sync: false,
    lfo_wave: 0,
    pms: 0,
    transpose: 0,
    volume: 0.0,
    mono: false,
    glide: 0.0,
    vintage: false,
    filter_on: false,
    filter_hp: false,
    cutoff: 0.0,
    resonance: 0.0,
    chorus_rate: 0.0,
    chorus_depth: 0.0,
    chorus_mix: 0.0,
    phaser_rate: 0.0,
    phaser_depth: 0.0,
    phaser_feedback: 0.0,
    phaser_mix: 0.0,
};

impl FmPatch {
    pub fn set(&mut self, p: FmParam, v: f32) {
        use FmParam as F;
        match p {
            F::Volume => self.volume = v.clamp(0.0, 1.0),
            F::Algorithm => self.algorithm = pick(v, fm_steps::ALGORITHM),
            F::Feedback => self.feedback = pick(v, fm_steps::FEEDBACK) as i32,
            F::OscSync => self.osc_sync = on(v),
            F::PitchR1 => self.pitch_rates[0] = d99(v),
            F::PitchR2 => self.pitch_rates[1] = d99(v),
            F::PitchR3 => self.pitch_rates[2] = d99(v),
            F::PitchR4 => self.pitch_rates[3] = d99(v),
            F::PitchL1 => self.pitch_levels[0] = d99(v),
            F::PitchL2 => self.pitch_levels[1] = d99(v),
            F::PitchL3 => self.pitch_levels[2] = d99(v),
            F::PitchL4 => self.pitch_levels[3] = d99(v),
            F::LfoSpeed => self.lfo_speed = d99(v),
            F::LfoDelay => self.lfo_delay = d99(v),
            F::LfoPmd => self.lfo_pmd = d99(v),
            F::LfoAmd => self.lfo_amd = d99(v),
            F::LfoSync => self.lfo_sync = on(v),
            F::LfoWave => self.lfo_wave = pick(v, fm_steps::LFO_WAVES),
            F::PitchModSens => self.pms = pick(v, fm_steps::PITCH_MOD_SENS),
            F::Transpose => self.transpose = pick(v, fm_steps::TRANSPOSE) as i32 - 24,
            F::Mono => self.mono = on(v),
            F::Glide => self.glide = v.clamp(0.0, 1.0).powi(2) * 2.0,
            F::Vintage => self.vintage = on(v),
            F::FilterOn => self.filter_on = on(v),
            F::FilterMode => self.filter_hp = on(v),
            F::Cutoff => self.cutoff = v.clamp(0.0, 1.0),
            F::Resonance => self.resonance = v.clamp(0.0, 1.0),
            F::ChorusRate => self.chorus_rate = v.clamp(0.0, 1.0),
            F::ChorusDepth => self.chorus_depth = v.clamp(0.0, 1.0),
            F::ChorusMix => self.chorus_mix = v.clamp(0.0, 1.0),
            F::PhaserRate => self.phaser_rate = v.clamp(0.0, 1.0),
            F::PhaserDepth => self.phaser_depth = v.clamp(0.0, 1.0),
            F::PhaserFeedback => self.phaser_feedback = v.clamp(0.0, 1.0),
            F::PhaserMix => self.phaser_mix = v.clamp(0.0, 1.0),
        }
    }

    /// Ignores `op >= OPS`.
    pub fn set_op(&mut self, op: usize, p: OpParam, v: f32) {
        if let Some(o) = self.ops.get_mut(op) {
            o.set(p, v);
        }
    }

    /// Filter cutoff in Hz.
    pub fn cutoff_hz(&self) -> f32 {
        exp(self.cutoff, 40.0, 18_000.0)
    }
}

impl Default for FmPatch {
    /// Every FM param at its default: the DX7 INIT VOICE.
    fn default() -> Self {
        let mut p = BLANK;
        for id in ParamId::all() {
            match id {
                ParamId::Fm(f) => p.set(f, id.info().default),
                ParamId::FmOp(op, o) => p.set_op(op as usize, o, id.info().default),
                _ => {}
            }
        }
        p
    }
}
```

- [ ] **Step 4: Implement `modulation.rs`**

```rust
//! The FM LFO and pitch envelope. Ranges approximate the DX7: LFO about
//! 0.06–50 Hz, pitch EG ±4 octaves, PMS and AMS as depth tables.

use std::f32::consts::TAU;

/// Semitones of LFO pitch swing at pitch depth 99, by pitch-mod sensitivity 0..7.
pub const PMS_SEMITONES: [f32; 8] = [0.0, 0.1, 0.2, 0.35, 0.6, 1.2, 2.4, 12.0];
/// Log2 attenuation at LFO amp depth 99, by amp-mod sensitivity 0..3.
pub const AMS_DEPTH: [f32; 4] = [0.0, 0.7, 1.7, 4.0];

pub fn lfo_hz(speed99: f32) -> f32 {
    0.062 * 2f32.powf(speed99.clamp(0.0, 99.0) / 99.0 * 9.66)
}

pub fn lfo_delay_seconds(delay99: f32) -> f32 {
    (delay99.clamp(0.0, 99.0) / 99.0).powi(2) * 5.0
}

/// How much of the LFO a note gets `t` seconds after it started: none
/// for the first half of the delay, fading in over the second half.
pub fn lfo_delay_gain(t: f32, delay_s: f32) -> f32 {
    if delay_s <= 0.0 {
        1.0
    } else {
        ((t - 0.5 * delay_s) / (0.5 * delay_s)).clamp(0.0, 1.0)
    }
}

/// One LFO shared by all voices. Waves: 0 tri, 1 saw down, 2 saw up,
/// 3 square, 4 sine, 5 sample & hold.
#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    phase: f32,
    held: f32,
    rng: u32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Lfo {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            held: 0.0,
            rng: 0x1234_5678,
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Advances by `seconds`; returns the value in -1..=1.
    pub fn tick(&mut self, hz: f32, wave: usize, seconds: f32) -> f32 {
        self.phase += hz * seconds;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.held = self.next_random();
        }
        let p = self.phase;
        match wave {
            0 => {
                if p < 0.5 {
                    4.0 * p - 1.0
                } else {
                    3.0 - 4.0 * p
                }
            }
            1 => 1.0 - 2.0 * p,
            2 => 2.0 * p - 1.0,
            3 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            4 => (TAU * p).sin(),
            _ => self.held,
        }
    }

    fn next_random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Pitch-EG level 0..99 in semitones (50 = no change, ±48 at the ends).
pub fn pitch_level_semitones(l99: f32) -> f32 {
    (l99.clamp(0.0, 99.0) - 50.0) / 50.0 * 48.0
}

/// Pitch-EG rate 0..99 in semitones per second.
pub fn pitch_rate(r99: f32) -> f32 {
    0.25 * 2f32.powf(r99.clamp(0.0, 99.0) / 99.0 * 13.0)
}

/// The pitch envelope: starts at L4, moves through L1..L3, holds L3
/// while the key is down, returns to L4 on release.
#[derive(Clone, Copy, Debug)]
pub struct PitchEnv {
    semis: f32,
    stage: usize,
    down: bool,
}

impl Default for PitchEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl PitchEnv {
    pub fn new() -> Self {
        Self {
            semis: 0.0,
            stage: 4,
            down: false,
        }
    }

    pub fn start(&mut self, levels: &[f32; 4]) {
        self.semis = pitch_level_semitones(levels[3]);
        self.stage = 0;
        self.down = true;
    }

    pub fn release(&mut self) {
        self.down = false;
        self.stage = 3;
    }

    /// Advances by `seconds`; returns the pitch offset in semitones.
    pub fn tick(&mut self, rates: &[f32; 4], levels: &[f32; 4], seconds: f32) -> f32 {
        if self.stage < 3 || (self.stage == 3 && !self.down) {
            let target = pitch_level_semitones(levels[self.stage]);
            let step = pitch_rate(rates[self.stage]) * seconds;
            if (target - self.semis).abs() <= step {
                self.semis = target;
                self.stage += 1;
            } else {
                self.semis += step.copysign(target - self.semis);
            }
        }
        self.semis
    }
}
```

In `flat_pitch_envelope_does_nothing`, every target is 0, so the value stays exactly 0.0.

- [ ] **Step 5: Implement `sine.rs`**

```rust
//! Sine lookup: a 4096-point table read with linear interpolation, or
//! (Vintage) a coarse 1024-point read without interpolation.

use std::f32::consts::TAU;

const SIZE: usize = 4096;

pub struct Sine {
    table: Box<[f32]>,
}

impl Default for Sine {
    fn default() -> Self {
        Self::new()
    }
}

impl Sine {
    /// Allocates the table: call off the audio thread.
    pub fn new() -> Self {
        Self {
            table: (0..=SIZE)
                .map(|i| (i as f32 / SIZE as f32 * TAU).sin())
                .collect(),
        }
    }

    /// sin(2π · phase) for a phase in cycles (any value).
    pub fn at(&self, phase: f32, vintage: bool) -> f32 {
        let p = phase - phase.floor();
        let x = p * SIZE as f32;
        let i = (x as usize).min(SIZE - 1);
        if vintage {
            self.table[i & !3]
        } else {
            let a = self.table[i];
            a + (self.table[i + 1] - a) * (x - i as f32)
        }
    }
}
```

The test asserts `s.at(0.25, false) == 1.0`. 0.25 × 4096 = 1024 exactly, and `table[1024]` is `sin(π/2)`, which is exactly 1.0 in f32. If it is off by an ulp on some platform, change that one assertion to `(s.at(0.25, false) - 1.0).abs() < 1e-6`.

- [ ] **Step 6: Add the modules, run the tests and the suite**

Add `pub mod modulation;`, `pub mod patch;` and `pub mod sine;` to `fm/mod.rs`.

Run: `cargo test -p acid-engine fm:: && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

- [ ] **Step 7: Commit**

```bash
git add -A crates
git commit -m "feat(fm): patch decoding, LFO, pitch EG and sine table

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: FM voice and synth core

**Files:**
- Create: `crates/acid-engine/src/fm/voice.rs`, `crates/acid-engine/src/fm/synth.rs`
- Modify: `crates/acid-engine/src/fm/mod.rs` (`mod voice; pub mod synth; pub use synth::FmSynth;`)

**Interfaces:**
- Consumes: everything from Tasks 2 and 3.
- Produces:

```rust
pub const VOICES: usize = 12;
pub struct FmSynth;
impl FmSynth {
    pub fn new(sample_rate: f32) -> Self;               // allocates: off the audio thread
    pub fn set_param(&mut self, p: FmParam, v: f32);
    pub fn set_op_param(&mut self, op: usize, p: OpParam, v: f32);
    pub fn patch(&self) -> &FmPatch;
    pub fn note_on(&mut self, pitch: u8, vel: u8, live: bool);   // live = audition
    pub fn note_off(&mut self, pitch: u8, live: bool);           // releases every voice of pitch from that source
    pub fn all_off(&mut self);
    pub fn reset(&mut self);                            // silence now
    pub fn active_voices(&self) -> usize;
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]);   // adds into both
}
```

- **Voice stealing:** a free voice is used first, then the oldest released voice, then the oldest voice. The stolen voice's last output fades out over about 2 ms (a declick offset), so the steal doesn't click.
- **Mono mode:** one voice with a held-key stack of 16. A new key while one is held glides there without retriggering the envelopes. Releasing the top key returns to the previous held key.
- **Vintage:** coarse sine reads, plus a 12-bit quantised mix before the volume.
- **Output:** in this task the dry mono mix goes to both sides. Task 5 routes it through the effects.

- [ ] **Step 1: Write the failing tests** at the bottom of `synth.rs` (create the file with the tests and a stub `pub struct FmSynth;`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{FmParam, OpParam};
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 48_000.0;

    fn render(s: &mut FmSynth, seconds: f32) -> Vec<f32> {
        let n = (seconds * SR) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        s.render(&mut l, &mut r);
        assert_eq!(l, r, "dry FM is mono");
        l
    }

    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    #[test]
    fn init_voice_is_a_sine_at_the_note() {
        let mut s = FmSynth::new(SR);
        s.note_on(69, 100, false);
        render(&mut s, 0.05);
        let x = render(&mut s, 0.1);
        assert!(all_finite(&x) && peak(&x) > 0.05 && peak(&x) < 1.0);
        let c = crossings(&x);
        assert!((86..=90).contains(&c), "440 Hz for 0.1 s: {c} crossings");
    }

    #[test]
    fn a_modulator_adds_harmonics() {
        let mut plain = FmSynth::new(SR);
        plain.note_on(57, 100, false);
        let a = render(&mut plain, 0.2);
        let mut fm = FmSynth::new(SR);
        fm.set_op_param(1, OpParam::Level, 0.9); // OP2 modulates OP1 in algorithm 1
        fm.note_on(57, 100, false);
        let b = render(&mut fm, 0.2);
        assert!(crossings(&b) as f32 > crossings(&a) as f32 * 1.3);
    }

    #[test]
    fn twelve_voices_and_the_oldest_is_stolen() {
        let mut s = FmSynth::new(SR);
        for p in 40..52 {
            s.note_on(p, 100, false);
            render(&mut s, 0.001);
        }
        assert_eq!(s.active_voices(), 12);
        s.note_on(80, 100, false);
        assert_eq!(s.active_voices(), 12);
        assert!(!s.sounding().contains(&40), "the oldest note was stolen");
        assert!(s.sounding().contains(&80));
    }

    #[test]
    fn release_frees_the_voice() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, false);
        render(&mut s, 0.05);
        s.note_off(60, false);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
        assert_eq!(peak(&render(&mut s, 0.01)), 0.0);
    }

    #[test]
    fn audition_and_pattern_notes_release_separately() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, true);
        s.note_on(60, 100, false);
        s.note_off(60, false);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 1, "the audition note still sounds");
        s.note_off(60, true);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn a_note_off_releases_every_voice_of_that_pitch() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, false);
        s.note_on(60, 100, false);
        s.note_off(60, false);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn mono_legato_returns_to_the_held_key() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::Mono, 1.0);
        s.note_on(60, 100, false);
        s.note_on(64, 100, false);
        render(&mut s, 0.01);
        assert_eq!(s.active_voices(), 1);
        assert_eq!(s.sounding(), vec![64]);
        s.note_off(64, false);
        assert_eq!(s.sounding(), vec![60]);
        s.note_off(60, false);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn glide_slides_between_mono_notes() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::Mono, 1.0);
        s.set_param(FmParam::Glide, 0.5); // 0.5 s
        s.note_on(60, 100, false);
        render(&mut s, 0.01);
        s.note_on(72, 100, false);
        render(&mut s, 0.05);
        let mid = s.voice_note(0);
        assert!(mid > 60.5 && mid < 71.5, "gliding: {mid}");
        render(&mut s, 5.0);
        assert!((s.voice_note(0) - 72.0).abs() < 0.01);
    }

    #[test]
    fn stealing_does_not_click() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::Mono, 1.0);
        s.note_on(45, 127, false);
        let before = render(&mut s, 0.2);
        s.note_off(45, false);
        s.note_on(45, 127, false); // restarts voice 0 while it still sounds
        let after = render(&mut s, 0.002);
        let jump = (after[0] - before[before.len() - 1]).abs();
        assert!(jump < 0.01, "jump {jump}");
    }

    #[test]
    fn vintage_quantises_to_12_bits() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::Vintage, 1.0);
        s.set_param(FmParam::Volume, 1.0);
        s.note_on(60, 100, false);
        let x = render(&mut s, 0.05);
        assert!(peak(&x) > 0.05);
        for v in x {
            let steps = v * 2048.0;
            assert!((steps - steps.round()).abs() < 1e-3, "{v}");
        }
    }

    #[test]
    fn switching_to_mono_releases_held_notes() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, false);
        s.note_on(64, 100, false);
        s.set_param(FmParam::Mono, 1.0);
        render(&mut s, 0.2);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn reset_silences_at_once_and_bad_input_is_ignored() {
        let mut s = FmSynth::new(SR);
        s.note_on(60, 100, false);
        s.note_on(200, 100, false);
        s.note_on(61, 0, false);
        assert_eq!(s.active_voices(), 1);
        s.reset();
        assert_eq!(s.active_voices(), 0);
        assert_eq!(peak(&render(&mut s, 0.01)), 0.0);
    }
}
```

`sounding()` and `voice_note()` are test-only helpers you add in Step 3.

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine fm::synth`
Expected: compile errors.

- [ ] **Step 3: Implement `voice.rs`**

```rust
//! One FM voice: six operators routed by an algorithm, each with a DX7
//! envelope, plus the pitch envelope, LFO delay and glide.

use super::algorithms::Algorithm;
use super::env::{Env, operator_out_level, scale_level, scale_rate, scale_velocity};
use super::modulation::{AMS_DEPTH, PMS_SEMITONES, PitchEnv, lfo_delay_gain, lfo_delay_seconds};
use super::patch::FmPatch;
use super::sine::Sine;
use super::OPS;

/// Output of one voice per full-level carrier, before the machine volume.
const VOICE_GAIN: f32 = 0.25;
/// Envelope level (log2) below which an operator is silent.
const SILENT: f32 = -13.0;
/// Time constant of the steal fade.
const DECLICK_SECONDS: f32 = 0.0005;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Voice {
    pub active: bool,
    /// Key held (not released).
    pub held: bool,
    /// Started by audition rather than the pattern.
    pub live: bool,
    pub pitch: u8,
    /// Start order, for stealing.
    pub age: u32,
    /// Current (gliding) MIDI note.
    pub note: f32,
    pub target_note: f32,
    vel: u8,
    phase: [f32; OPS],
    env: [Env; OPS],
    gain: [f32; OPS],
    out_level: [f32; OPS],
    fb: [f32; 2],
    pitch_env: PitchEnv,
    /// Seconds since the note started (for the LFO delay).
    time: f32,
    declick: f32,
    declick_coef: f32,
    last_out: f32,
    sample_rate: f32,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            active: false,
            held: false,
            live: false,
            pitch: 0,
            age: 0,
            note: 0.0,
            target_note: 0.0,
            vel: 0,
            phase: [0.0; OPS],
            env: [Env::new(sample_rate); OPS],
            gain: [0.0; OPS],
            out_level: [0.0; OPS],
            fb: [0.0; 2],
            pitch_env: PitchEnv::new(),
            time: 0.0,
            declick: 0.0,
            declick_coef: (-1.0 / (DECLICK_SECONDS * sample_rate)).exp(),
            last_out: 0.0,
            sample_rate,
        }
    }

    fn key(&self, patch: &FmPatch) -> i32 {
        (self.pitch as i32 + patch.transpose).clamp(0, 127)
    }

    fn op_out_level(&self, patch: &FmPatch, op: usize, key: i32) -> f32 {
        let o = &patch.ops[op];
        operator_out_level(
            o.level,
            scale_level(key, o.break_point, o.left_depth, o.right_depth, o.left_curve, o.right_curve),
            scale_velocity(self.vel as i32, o.vel_sens),
        )
    }

    /// Starts a note. A voice that is still sounding fades out over ~2 ms
    /// instead of being cut.
    pub fn start(&mut self, patch: &FmPatch, pitch: u8, vel: u8, live: bool, age: u32) {
        self.declick = if self.active { self.last_out } else { 0.0 };
        self.active = true;
        self.held = true;
        self.live = live;
        self.pitch = pitch;
        self.age = age;
        self.vel = vel;
        self.note = pitch as f32;
        self.target_note = self.note;
        self.time = 0.0;
        self.fb = [0.0; 2];
        let key = self.key(patch);
        for op in 0..OPS {
            let o = &patch.ops[op];
            if patch.osc_sync {
                self.phase[op] = 0.0;
            }
            let ol = self.op_out_level(patch, op, key);
            self.out_level[op] = ol;
            self.env[op].start(o.rates, o.levels, ol, scale_rate(key, o.rate_scaling));
            self.gain[op] = 0.0;
        }
        self.pitch_env.start(&patch.pitch_levels);
    }

    /// Mono legato: move to `pitch` without restarting the envelopes.
    pub fn glide_to(&mut self, patch: &FmPatch, pitch: u8) {
        self.pitch = pitch;
        self.target_note = pitch as f32;
        if patch.glide <= 0.0 {
            self.note = self.target_note;
        }
        self.held = true;
    }

    pub fn release(&mut self) {
        if self.held {
            self.held = false;
            for e in &mut self.env {
                e.release();
            }
            self.pitch_env.release();
        }
    }

    /// Silences the voice immediately.
    pub fn kill(&mut self) {
        *self = Voice::new(self.sample_rate);
    }

    /// Renders up to `BLOCK` samples, adding into `out`. `lfo` is the
    /// shared LFO value (-1..1) for this block.
    pub fn render(&mut self, patch: &FmPatch, alg: &Algorithm, lfo: f32, sine: &Sine, out: &mut [f32]) {
        let n = out.len();
        if n == 0 {
            return;
        }
        let dt = n as f32 / self.sample_rate;
        self.time += dt;
        let lfo = lfo * lfo_delay_gain(self.time, lfo_delay_seconds(patch.lfo_delay));
        if patch.glide > 0.0 && self.note != self.target_note {
            self.note += (self.target_note - self.note) * (1.0 - (-dt / patch.glide).exp());
            if (self.target_note - self.note).abs() < 0.001 {
                self.note = self.target_note;
            }
        } else {
            self.note = self.target_note;
        }
        let semis = self.pitch_env.tick(&patch.pitch_rates, &patch.pitch_levels, dt)
            + lfo * patch.lfo_pmd / 99.0 * PMS_SEMITONES[patch.pms.min(7)];
        let note_hz = 440.0 * 2f32.powf((self.note + patch.transpose as f32 + semis - 69.0) / 12.0);
        let amp_lfo = (lfo + 1.0) * 0.5 * patch.lfo_amd / 99.0;
        let key = self.key(patch);
        let mut inc = [0.0f32; OPS];
        let mut g0 = [0.0f32; OPS];
        let mut dg = [0.0f32; OPS];
        for op in 0..OPS {
            let o = &patch.ops[op];
            let ol = self.op_out_level(patch, op, key);
            if ol != self.out_level[op] {
                self.out_level[op] = ol;
                self.env[op].set_out_level(ol);
            }
            let level = self.env[op].tick() - amp_lfo * AMS_DEPTH[o.amp_mod_sens.min(3)];
            let target = if o.on && level > SILENT { 2f32.powf(level).min(2.0) } else { 0.0 };
            inc[op] = o.freq(note_hz) / self.sample_rate;
            g0[op] = self.gain[op];
            dg[op] = (target - self.gain[op]) / n as f32;
            self.gain[op] = target;
        }
        let fb_amount = if patch.feedback <= 0 { 0.0 } else { 2f32.powi(patch.feedback - 8) };
        for (i, s) in out.iter_mut().enumerate() {
            let mut outs = [0.0f32; OPS];
            let mut sum = 0.0;
            for op in (0..OPS).rev() {
                let mut m = 0.0;
                let mut bits = alg.mods[op];
                while bits != 0 {
                    m += outs[bits.trailing_zeros() as usize];
                    bits &= bits - 1;
                }
                if op == alg.feedback {
                    m += (self.fb[0] + self.fb[1]) * 0.5 * fb_amount;
                }
                let y = sine.at(self.phase[op] + m, patch.vintage) * (g0[op] + dg[op] * i as f32);
                if op == alg.feedback {
                    self.fb[1] = self.fb[0];
                    self.fb[0] = y;
                }
                outs[op] = y;
                self.phase[op] += inc[op];
                self.phase[op] -= self.phase[op].floor();
                if alg.carriers & (1 << op) != 0 {
                    sum += y;
                }
            }
            let v = sum * VOICE_GAIN + self.declick;
            self.declick *= self.declick_coef;
            *s += v;
            self.last_out = v;
        }
        let carriers_done = (0..OPS)
            .filter(|&op| alg.carriers & (1 << op) != 0)
            .all(|op| self.env[op].is_finished() && self.gain[op] == 0.0);
        if !self.held && carriers_done && self.declick.abs() < 1e-5 {
            self.active = false;
            self.last_out = 0.0;
        }
    }
}
```

- [ ] **Step 4: Implement `synth.rs`** (above the tests)

```rust
//! The FM machine: 12 voices (or one in mono mode), the shared LFO, and
//! the mix. Notes come from the pattern or from audition (`live`); a
//! note-off only releases voices from its own source, and releases every
//! voice of that pitch.

use super::algorithms::ALGORITHMS;
use super::modulation::{Lfo, lfo_hz};
use super::patch::FmPatch;
use super::sine::Sine;
use super::voice::Voice;
use super::{BLOCK, OPS};
use crate::params::{FmParam, OpParam};

pub const VOICES: usize = 12;
const MONO_STACK: usize = 16;

pub struct FmSynth {
    sample_rate: f32,
    patch: FmPatch,
    voices: [Voice; VOICES],
    lfo: Lfo,
    sine: Sine,
    clock: u32,
    stack: [u8; MONO_STACK],
    stack_len: usize,
    buf: [f32; BLOCK],
}

impl FmSynth {
    /// Allocates the sine table: call off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            patch: FmPatch::default(),
            voices: std::array::from_fn(|_| Voice::new(sample_rate)),
            lfo: Lfo::new(),
            sine: Sine::new(),
            clock: 0,
            stack: [0; MONO_STACK],
            stack_len: 0,
            buf: [0.0; BLOCK],
        }
    }

    pub fn patch(&self) -> &FmPatch {
        &self.patch
    }

    pub fn set_param(&mut self, p: FmParam, v: f32) {
        let was_mono = self.patch.mono;
        self.patch.set(p, v);
        if self.patch.mono != was_mono {
            self.all_off();
        }
    }

    pub fn set_op_param(&mut self, op: usize, p: OpParam, v: f32) {
        if op < OPS {
            self.patch.set_op(op, p, v);
        }
    }

    /// Pitches above 127 and velocity 0 are ignored.
    pub fn note_on(&mut self, pitch: u8, vel: u8, live: bool) {
        if pitch > 127 || vel == 0 {
            return;
        }
        self.clock = self.clock.wrapping_add(1);
        if self.patch.lfo_sync {
            self.lfo.reset();
        }
        if self.patch.mono {
            self.mono_on(pitch, vel.min(127), live);
        } else {
            self.poly_on(pitch, vel.min(127), live);
        }
    }

    fn poly_on(&mut self, pitch: u8, vel: u8, live: bool) {
        let clock = self.clock;
        let age = |v: &Voice| clock.wrapping_sub(v.age);
        let voices = &self.voices;
        let i = voices
            .iter()
            .position(|v| !v.active)
            .or_else(|| (0..VOICES).filter(|&i| !voices[i].held).max_by_key(|&i| age(&voices[i])))
            .unwrap_or_else(|| (0..VOICES).max_by_key(|&i| age(&voices[i])).unwrap_or(0));
        self.voices[i].start(&self.patch, pitch, vel, live, clock);
    }

    fn stack_top(&self) -> Option<u8> {
        self.stack[..self.stack_len].last().copied()
    }

    fn stack_remove(&mut self, pitch: u8) {
        if let Some(i) = self.stack[..self.stack_len].iter().position(|&p| p == pitch) {
            self.stack.copy_within(i + 1..self.stack_len, i);
            self.stack_len -= 1;
        }
    }

    fn mono_on(&mut self, pitch: u8, vel: u8, live: bool) {
        self.stack_remove(pitch);
        if self.stack_len == MONO_STACK {
            self.stack.copy_within(1.., 0);
            self.stack_len -= 1;
        }
        self.stack[self.stack_len] = pitch;
        self.stack_len += 1;
        let v = &mut self.voices[0];
        if v.active && v.held {
            v.glide_to(&self.patch, pitch);
            v.live = live;
        } else {
            v.start(&self.patch, pitch, vel, live, self.clock);
        }
    }

    pub fn note_off(&mut self, pitch: u8, live: bool) {
        if self.patch.mono {
            let top = self.stack_top();
            self.stack_remove(pitch);
            if top == Some(pitch) {
                match self.stack_top() {
                    Some(prev) => self.voices[0].glide_to(&self.patch, prev),
                    None => self.voices[0].release(),
                }
            }
            return;
        }
        for v in &mut self.voices {
            if v.active && v.held && v.pitch == pitch && v.live == live {
                v.release();
            }
        }
    }

    /// Releases every note (tails ring out).
    pub fn all_off(&mut self) {
        self.stack_len = 0;
        for v in &mut self.voices {
            v.release();
        }
    }

    /// Silences everything now.
    pub fn reset(&mut self) {
        self.stack_len = 0;
        for v in &mut self.voices {
            v.kill();
        }
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    /// Adds the machine's output into `left`/`right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let alg = ALGORITHMS[self.patch.algorithm.min(ALGORITHMS.len() - 1)];
        let total = left.len().min(right.len());
        let gain = self.patch.volume * self.patch.volume;
        let mut pos = 0;
        while pos < total {
            let n = (total - pos).min(BLOCK);
            let lfo = self.lfo.tick(
                lfo_hz(self.patch.lfo_speed),
                self.patch.lfo_wave,
                n as f32 / self.sample_rate,
            );
            let buf = &mut self.buf[..n];
            buf.fill(0.0);
            for v in self.voices.iter_mut().filter(|v| v.active) {
                v.render(&self.patch, &alg, lfo, &self.sine, buf);
            }
            for x in buf.iter_mut() {
                if self.patch.vintage {
                    *x = (x.clamp(-1.0, 1.0) * 2048.0).round() / 2048.0;
                }
                *x *= gain;
            }
            for (i, x) in buf.iter().enumerate() {
                left[pos + i] += x;
                right[pos + i] += x;
            }
            pos += n;
        }
    }

    #[cfg(test)]
    pub(crate) fn sounding(&self) -> Vec<u8> {
        self.voices.iter().filter(|v| v.active && v.held).map(|v| v.pitch).collect()
    }

    #[cfg(test)]
    pub(crate) fn voice_note(&self, i: usize) -> f32 {
        self.voices[i].note
    }
}
```

In `vintage_quantises_to_12_bits` the volume is 1.0, so gain = 1.0 and quantised values survive the multiply unchanged.

In `stealing_does_not_click`, mono voice 0 is still active during its release when the second note-on arrives (the key was just released). That triggers `start`, which carries `last_out` into `declick`, so the first new sample continues from the last old sample.

- [ ] **Step 5: Add the modules and run everything**

In `fm/mod.rs` add `pub mod synth;`, `mod voice;` and `pub use synth::FmSynth;`.

Run: `cargo test -p acid-engine fm:: && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

If `init_voice_is_a_sine_at_the_note` counts 86–90 crossings but sits right at an edge, check that the note is 440 Hz (note 69, transpose 0, flat pitch EG). Don't widen the range beyond 85..=91.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(fm): six-operator voice and 12-voice synth with mono glide

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: FM effects: filter, chorus, phaser

**Files:**
- Create: `crates/acid-engine/src/fm/fx.rs`
- Modify: `crates/acid-engine/src/fm/mod.rs` (`pub mod fx;`), `crates/acid-engine/src/fm/synth.rs` (route the mix through `FmFx`)

**Interfaces:**
- Consumes: `FmPatch` (Task 3); `sid_core::{Svf, FILTER_LP, FILTER_HP}`; `crate::params::exp`.
- Produces:

```rust
pub struct FmFx;
impl FmFx {
    pub fn new(sample_rate: f32) -> Self;      // allocates the chorus line
    pub fn reset(&mut self);
    /// Filters `mono` in place, then adds chorus/phaser stereo into left/right.
    pub fn process(&mut self, p: &FmPatch, mono: &mut [f32], left: &mut [f32], right: &mut [f32]);
}
```

With the filter off and both mixes at 0 (the defaults), `left[i] += mono[i]` and `right[i] += mono[i]` exactly, so the Task 4 tests keep passing.

- [ ] **Step 1: Write the failing tests** (bottom of `fx.rs`, with stub types first)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, rms};

    const SR: f32 = 48_000.0;

    fn tone(hz: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 / SR * hz * std::f32::consts::TAU).sin() * 0.5).collect()
    }

    fn run(p: &FmPatch, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut fx = FmFx::new(SR);
        let mut mono = input.to_vec();
        let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        for ((m, l), r) in mono.chunks_mut(32).zip(l.chunks_mut(32)).zip(r.chunks_mut(32)) {
            fx.process(p, m, l, r);
        }
        (l, r)
    }

    #[test]
    fn defaults_pass_the_dry_signal_through() {
        let x = tone(300.0, 4800);
        let (l, r) = run(&FmPatch::default(), &x);
        assert_eq!(l, x);
        assert_eq!(r, x);
    }

    #[test]
    fn low_pass_filter_tames_highs_and_high_pass_tames_lows() {
        let mut p = FmPatch::default();
        p.filter_on = true;
        p.cutoff = 0.3; // a few hundred Hz
        let hi = tone(6000.0, 9600);
        let (l, _) = run(&p, &hi);
        assert!(rms(&l[4800..]) < rms(&hi[4800..]) * 0.2);
        p.filter_hp = true;
        p.cutoff = 0.8;
        let lo = tone(100.0, 9600);
        let (l, _) = run(&p, &lo);
        assert!(rms(&l[4800..]) < rms(&lo[4800..]) * 0.2);
    }

    #[test]
    fn chorus_widens_and_phaser_colours() {
        let x = tone(440.0, 24_000);
        let mut p = FmPatch::default();
        p.chorus_mix = 1.0;
        let (l, r) = run(&p, &x);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(rms(&l.iter().zip(&r).map(|(a, b)| a - b).collect::<Vec<_>>()) > 0.01, "stereo");
        let mut p = FmPatch::default();
        p.phaser_mix = 1.0;
        p.phaser_feedback = 1.0;
        let (l, _) = run(&p, &x);
        assert!(all_finite(&l));
        assert!(rms(&l.iter().zip(&x).map(|(a, b)| a - b).collect::<Vec<_>>()) > 0.01);
        assert!(l.iter().all(|v| v.abs() < 4.0));
    }

    #[test]
    fn extreme_settings_stay_finite() {
        let mut p = FmPatch::default();
        p.filter_on = true;
        p.cutoff = 1.0;
        p.resonance = 1.0;
        p.chorus_mix = 1.0;
        p.chorus_depth = 1.0;
        p.chorus_rate = 1.0;
        p.phaser_mix = 1.0;
        p.phaser_feedback = 1.0;
        p.phaser_rate = 1.0;
        p.phaser_depth = 1.0;
        let (l, r) = run(&p, &tone(1000.0, 48_000));
        assert!(all_finite(&l) && all_finite(&r));
    }
}
```

In `synth.rs` tests, add:

```rust
    #[test]
    fn chorus_on_makes_the_output_stereo() {
        let mut s = FmSynth::new(SR);
        s.set_param(FmParam::ChorusMix, 1.0);
        s.note_on(60, 100, false);
        let n = 9600;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        s.render(&mut l, &mut r);
        assert_ne!(l, r);
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine fm::`
Expected: failures.

- [ ] **Step 3: Implement `fx.rs`**

```rust
//! The FM-1's built-in effects, in order: filter, chorus, phaser. They
//! take the synth's mono mix and add a stereo result into the outputs.
//! With the filter off and both mixes at 0 the signal passes untouched.

use super::patch::FmPatch;
use crate::params::exp;
use sid_core::{FILTER_HP, FILTER_LP, Svf};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Chorus delay line length: covers 20 ms at 192 kHz.
const CHORUS_LEN: usize = 4096;
const PHASER_STAGES: usize = 4;

pub struct Chorus {
    buf: Vec<f32>,
    write: usize,
    phase: f32,
    sample_rate: f32,
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            buf: vec![0.0; CHORUS_LEN],
            write: 0,
            phase: 0.0,
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.phase = 0.0;
    }

    fn read(&self, delay: f32) -> f32 {
        let pos = self.write as f32 - delay + CHORUS_LEN as f32;
        let i = pos.floor();
        let f = pos - i;
        let i = i as usize % CHORUS_LEN;
        let j = (i + 1) % CHORUS_LEN;
        self.buf[i] + (self.buf[j] - self.buf[i]) * f
    }

    /// Writes `x` and returns the (left, right) wet samples.
    pub fn process(&mut self, x: f32, rate: f32, depth: f32) -> (f32, f32) {
        self.buf[self.write] = x;
        self.phase += exp(rate, 0.05, 5.0) / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let base = 0.012 * self.sample_rate;
        let swing = 0.008 * self.sample_rate * depth.clamp(0.0, 1.0);
        let dl = base + swing * 0.5 * (1.0 + (TAU * self.phase).sin());
        let dr = base + swing * 0.5 * (1.0 + (TAU * self.phase + FRAC_PI_2).sin());
        let out = (self.read(dl), self.read(dr));
        self.write = (self.write + 1) % CHORUS_LEN;
        out
    }
}

pub struct Phaser {
    state: [[f32; PHASER_STAGES]; 2],
    last: [f32; 2],
    phase: f32,
    sample_rate: f32,
}

impl Phaser {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            state: [[0.0; PHASER_STAGES]; 2],
            last: [0.0; 2],
            phase: 0.0,
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; PHASER_STAGES]; 2];
        self.last = [0.0; 2];
    }

    /// Four swept all-pass stages per side; returns the all-passed pair.
    pub fn process(&mut self, x: [f32; 2], rate: f32, depth: f32, feedback: f32) -> [f32; 2] {
        self.phase += exp(rate, 0.05, 5.0) / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let mut out = [0.0; 2];
        for ch in 0..2 {
            let sweep = 0.5 * (1.0 + (TAU * (self.phase + ch as f32 * 0.25)).sin());
            let f = (200.0 * 20f32.powf(depth.clamp(0.0, 1.0) * sweep)).min(0.45 * self.sample_rate);
            let t = (PI * f / self.sample_rate).tan();
            let a = (t - 1.0) / (t + 1.0);
            let mut y = x[ch] + self.last[ch] * feedback.clamp(0.0, 1.0) * 0.7;
            for s in &mut self.state[ch] {
                let o = a * y + *s;
                *s = y - a * o;
                y = o;
            }
            self.last[ch] = y;
            out[ch] = y;
        }
        out
    }
}

pub struct FmFx {
    filter: Svf,
    chorus: Chorus,
    phaser: Phaser,
    sample_rate: f32,
}

impl FmFx {
    /// Allocates the chorus delay line: call off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            filter: Svf::new(sample_rate),
            chorus: Chorus::new(sample_rate),
            phaser: Phaser::new(sample_rate),
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.filter = Svf::new(self.sample_rate);
        self.chorus.reset();
        self.phaser.reset();
    }

    pub fn process(&mut self, p: &FmPatch, mono: &mut [f32], left: &mut [f32], right: &mut [f32]) {
        if p.filter_on {
            self.filter.mode = if p.filter_hp { FILTER_HP } else { FILTER_LP };
            self.filter.set(p.cutoff_hz(), p.resonance);
            for x in mono.iter_mut() {
                *x = self.filter.process(*x);
            }
        }
        for (i, &x) in mono.iter().enumerate() {
            let (wl, wr) = self.chorus.process(x, p.chorus_rate, p.chorus_depth);
            let mut s = [x + (wl - x) * p.chorus_mix, x + (wr - x) * p.chorus_mix];
            if p.phaser_mix > 0.0 {
                let w = self.phaser.process(s, p.phaser_rate, p.phaser_depth, p.phaser_feedback);
                for ch in 0..2 {
                    s[ch] += (w[ch] - s[ch]) * 0.5 * p.phaser_mix;
                }
            }
            left[i] += s[0];
            right[i] += s[1];
        }
    }
}
```

`Svf::new` doesn't allocate, so `reset` is real-time safe. In the test module, `rms` and `all_finite` come from `crate::testutil`.

- [ ] **Step 4: Route the synth through the effects**

In `synth.rs`:
- add the field `fx: FmFx`, initialised with `FmFx::new(sample_rate)`
- call `self.fx.reset();` at the end of `reset()`
- in `render`, replace the loop that adds `buf` into both sides with:

```rust
            self.fx.process(&self.patch, buf, &mut left[pos..pos + n], &mut right[pos..pos + n]);
```

(`buf` is `&mut self.buf[..n]`, a disjoint field borrow, so the call compiles.)

Change the module doc comment of `synth.rs` to mention that the mix runs through the filter, chorus and phaser.

- [ ] **Step 5: Run everything**

Run: `cargo test -p acid-engine fm:: && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings (Task 4's `assert_eq!(l, r)` holds with default mixes).

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(fm): filter, stereo chorus and phaser

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: The FM synth in the engine

**Files:**
- Modify: `crates/acid-engine/src/engine.rs`

**Interfaces:**
- Consumes: `FmSynth` (Tasks 4 and 5); `SineVoices` (Plan 4, kept for the sampler).
- Produces: unchanged public API (`Engine::note_on(machine, pitch, vel)` / `note_off(machine, pitch)` are the audition path). Internally:
  - pattern events call `self.poly_on(machine, pitch, vel, false)` / `self.poly_off(machine, pitch, false)`
  - audition calls them with `live = true`

- [ ] **Step 1: Write the failing tests** (in `engine.rs` tests)

```rust
    #[test]
    fn audition_survives_a_pattern_release_of_the_same_pitch() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm.patterns[0].notes.push(Note { start: 0, len: 4, pitch: 60, vel: 100 });
        let mut held = Engine::from_project(SR, &p);
        held.play();
        held.note_on(4, 60, 100);
        let (l, _) = render(&mut held, 0.5);
        assert!(peak(&l[l.len() - 2000..]) > 0.01, "audition note still sounds");
        let mut pattern_only = Engine::from_project(SR, &p);
        pattern_only.play();
        let (l, _) = render(&mut pattern_only, 0.5);
        assert!(peak(&l[l.len() - 2000..]) < 1e-3, "pattern note released");
    }

    #[test]
    fn fm_operator_params_reach_the_synth() {
        use crate::params::OpParam;
        use crate::project::Note;
        let mut p = Project::default();
        p.fm.patterns[0].notes.push(Note { start: 0, len: 32, pitch: 60, vel: 120 });
        p.params.insert(ParamId::FmOp(0, OpParam::Level), 0.0);
        let mut e = Engine::from_project(SR, &p);
        e.play();
        let (l, _) = render(&mut e, 0.3);
        assert_eq!(peak(&l), 0.0, "OP1 (the only carrier heard) is at level 0");
    }

    #[test]
    fn fm_algorithm_changes_the_sound() {
        use crate::params::{FmParam, OpParam};
        use crate::project::Note;
        let mut p = Project::default();
        p.fm.patterns[0].notes.push(Note { start: 0, len: 32, pitch: 57, vel: 120 });
        p.params.insert(ParamId::FmOp(1, OpParam::Level), 0.9);
        let mut a = Engine::from_project(SR, &p);
        p.params.insert(ParamId::Fm(FmParam::Algorithm), 1.0);
        let mut b = Engine::from_project(SR, &p);
        a.play();
        b.play();
        let (la, _) = render(&mut a, 0.3);
        let (lb, _) = render(&mut b, 0.3);
        assert_ne!(la, lb);
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine engine::`
Expected: `audition_survives…` and `fm_operator_params…` fail (the placeholder ignores op params and sources).

- [ ] **Step 3: Implement**

In `engine.rs`:
- Replace the field `note_machines: [SineVoices; NOTE_MACHINES]` with:

```rust
    fm: FmSynth,
    /// The sampler slot's placeholder until the sampler engine arrives.
    sampler: SineVoices,
```

  Initialise them with `fm: FmSynth::new(sample_rate)` and `sampler: SineVoices::new(sample_rate)`. Import `crate::fm::FmSynth` and `crate::machine::MachineId`. Drop `FIRST_NOTE_MACHINE` from the imports if it becomes unused (keep `NOTE_MACHINES`, used by `EVENT_RESERVE`).
- In `apply_param`, replace the two placeholder arms with:

```rust
            ParamId::Fm(p) => self.fm.set_param(p, v),
            ParamId::FmOp(op, p) => self.fm.set_op_param(op as usize, p, v),
            ParamId::Sampler(SamplerParam::Volume) => self.sampler.set_volume(v),
```

  Remove any `FmOp` placeholder arm added in Task 1.
- Replace `note_on` and `note_off` with:

```rust
    /// Audition: starts a note on note machine `machine` (4 = FM,
    /// 5 = Sampler) right away, playing or not. Other machines, pitches
    /// above 127 and velocity 0 are ignored.
    pub fn note_on(&mut self, machine: usize, pitch: u8, vel: u8) {
        self.poly_on(machine, pitch, vel, true);
    }

    /// Audition release: only releases auditioned notes.
    pub fn note_off(&mut self, machine: usize, pitch: u8) {
        self.poly_off(machine, pitch, true);
    }

    fn poly_on(&mut self, machine: usize, pitch: u8, vel: u8, live: bool) {
        if pitch > 127 || vel == 0 {
            return;
        }
        match MachineId::from_index(machine) {
            Some(MachineId::Fm) => self.fm.note_on(pitch, vel.min(127), live),
            Some(MachineId::Sampler) => self.sampler.note_on(pitch, vel.min(127)),
            _ => {}
        }
    }

    fn poly_off(&mut self, machine: usize, pitch: u8, live: bool) {
        match MachineId::from_index(machine) {
            Some(MachineId::Fm) => self.fm.note_off(pitch, live),
            Some(MachineId::Sampler) => self.sampler.note_off(pitch),
            _ => {}
        }
    }
```

- In `process_block`'s event match:

```rust
                        SeqEvent::PolyOn { machine, pitch, vel } => self.poly_on(machine, pitch, vel, false),
                        SeqEvent::PolyOff { machine, pitch } => self.poly_off(machine, pitch, false),
```

- Replace the note-machine render loop with:

```rust
            let c = &mut self.chans[MachineId::Fm.index()];
            self.fm.render(&mut c.l[span.clone()], &mut c.r[span.clone()]);
            let c = &mut self.chans[MachineId::Sampler.index()];
            self.sampler.render(&mut c.l[span.clone()], &mut c.r[span.clone()]);
```

- In `play()` and `stop()`, replace the loops over `note_machines` with `self.fm.all_off(); self.sampler.all_off();`.
- In the guard branch, use `self.fm.reset(); self.sampler.reset();`.
- Update the module doc comment so it no longer calls the FM slot a placeholder.

Existing engine tests that use machine 4 with the placeholder still hold, because the INIT voice is a sine on OP1. These include `note_patterns_play_on_their_own_channels`, `stop_releases_note_machines`, `fm_volume_knob_reaches_the_voice`, `restarting_play_releases_held_notes` and `deleting_a_sounding_note_while_playing_releases_it`. If one fails, read it carefully before changing anything. The only acceptable edits are to timings that depended on the placeholder's 80 ms release. The INIT voice's release (rate 99) is faster, so they should pass as written.

- [ ] **Step 4: Run everything, including the allocation test**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass (including `tests/no_alloc.rs` with real FM voices), 0 warnings.

If `no_alloc` fails, the FM path allocates on the audio thread. Find it with the debug trick in the comments of `tests/no_alloc.rs` and fix the FM code. Never loosen the test.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "feat(engine): the FM slot plays the FM synth, audition kept separate

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: DX7 SysEx voices

**Files:**
- Create: `crates/acid-engine/src/fm/sysex.rs`
- Modify: `crates/acid-engine/src/fm/mod.rs` (`pub mod sysex;`)

**Interfaces:**
- Produces:

```rust
pub const BANK_VOICES: usize = 32;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dx7Op { pub rates: [u8;4], pub levels: [u8;4], pub break_point: u8, pub left_depth: u8,
    pub right_depth: u8, pub left_curve: u8, pub right_curve: u8, pub rate_scaling: u8,
    pub amp_mod_sens: u8, pub vel_sens: u8, pub level: u8, pub fixed: bool, pub coarse: u8,
    pub fine: u8, pub detune: u8 /* 0..=14, 7 = centre */ }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dx7Voice { pub ops: [Dx7Op; 6] /* [0] = OP1 */, pub pitch_rates: [u8;4], pub pitch_levels: [u8;4],
    pub algorithm: u8 /* 0..=31 */, pub feedback: u8, pub osc_sync: bool, pub lfo_speed: u8,
    pub lfo_delay: u8, pub lfo_pmd: u8, pub lfo_amd: u8, pub lfo_sync: bool, pub lfo_wave: u8,
    pub pms: u8, pub transpose: u8 /* 0..=48, 24 = centre */, pub name: String }
impl Dx7Voice {
    pub fn init() -> Self;                      // "INIT VOICE"
    pub fn clamped(self) -> Self;
    pub fn is_valid(&self) -> bool;
    pub fn to_params(&self) -> Vec<(ParamId, f32)>;    // every DX7 voice param + OpParam::On = 1
    pub fn from_params(name: &str, value: impl Fn(ParamId) -> f32) -> Self;
}
pub enum SysexError { NotDx7, Truncated, Checksum }   // Display
pub fn parse(bytes: &[u8]) -> Result<Vec<Dx7Voice>, SysexError>;   // 32 for a bank, 1 for a single voice
pub fn bank_sysex(voices: &[Dx7Voice]) -> Vec<u8>;   // pads with INIT voices / truncates to 32
pub fn single_sysex(voice: &Dx7Voice) -> Vec<u8>;
pub fn pack_voice(v: &Dx7Voice) -> [u8; 128];
pub fn unpack_voice(b: &[u8; 128]) -> Dx7Voice;
```

**Formats.** In both formats the operators are stored OP6 first.

Packed 128-byte voice (bulk dump), 17 bytes per operator at offset `(5 - op) * 17`:

| Offset | Contents |
|---|---|
| 0–3 | rates |
| 4–7 | levels |
| 8 | break point |
| 9 | left depth |
| 10 | right depth |
| 11 | `lc \| rc<<2` |
| 12 | `rs \| det<<3` |
| 13 | `ams \| kvs<<2` |
| 14 | output level |
| 15 | `mode \| coarse<<1` |
| 16 | fine |

Then the global bytes:

| Offset | Contents |
|---|---|
| 102–105 | pitch rates |
| 106–109 | pitch levels |
| 110 | algorithm |
| 111 | `fb \| oks<<3` |
| 112 | LFO speed |
| 113 | LFO delay |
| 114 | LFO pitch depth |
| 115 | LFO amp depth |
| 116 | `lks \| wave<<1 \| pms<<4` |
| 117 | transpose |
| 118–127 | name |

Unpacked 155-byte voice (single dump), 21 bytes per operator at `(5 - op) * 21`:
- Operator bytes in order: R1–4, L1–4, BP, LD, RD, LC, RC, RS, AMS, KVS, OL, MODE, COARSE, FINE, DET.
- Then from offset 126: PR1–4, PL1–4, ALG, FB, OKS, LFS, LFD, LPMD, LAMD, LKS, LFW, LPMS, TRNSP, NAME(10).

Message envelopes:
- **Bank:** `F0 43 0n 09 20 00`, then 4096 data bytes, a checksum byte and `F7` (4104 bytes in all).
- **Single voice:** `F0 43 0n 00 01 1B`, then 155 data bytes, a checksum byte and `F7` (163 bytes in all).
- **Checksum:** `(128 − (sum of data bytes mod 128)) mod 128`.

- [ ] **Step 1: Write the failing tests** (bottom of `sysex.rs`, with stubs above)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn odd_voice(seed: u8) -> Dx7Voice {
        let mut v = Dx7Voice::init();
        v.name = format!("VOICE {seed}");
        v.algorithm = seed % 32;
        v.feedback = seed % 8;
        v.osc_sync = seed % 2 == 0;
        v.lfo_wave = seed % 6;
        v.pms = seed % 8;
        v.transpose = seed % 49;
        v.pitch_levels = [seed % 100, 50, 60, 70];
        for (i, o) in v.ops.iter_mut().enumerate() {
            let k = seed.wrapping_add(i as u8 * 7);
            o.rates = [k % 100, 99 - k % 100, 40, 60];
            o.levels = [99, k % 100, 50, 0];
            o.left_curve = k % 4;
            o.right_curve = (k + 1) % 4;
            o.rate_scaling = k % 8;
            o.detune = k % 15;
            o.amp_mod_sens = k % 4;
            o.vel_sens = k % 8;
            o.fixed = k % 3 == 0;
            o.coarse = k % 32;
            o.fine = k % 100;
            o.level = 99 - k % 50;
            o.break_point = k % 100;
            o.left_depth = k % 100;
            o.right_depth = (k + 3) % 100;
        }
        v
    }

    #[test]
    fn packed_voices_round_trip() {
        for seed in 0..40 {
            let v = odd_voice(seed);
            assert_eq!(unpack_voice(&pack_voice(&v)), v, "seed {seed}");
        }
        let init = Dx7Voice::init();
        assert_eq!(unpack_voice(&pack_voice(&init)), init);
    }

    #[test]
    fn banks_and_single_voices_round_trip() {
        let voices: Vec<Dx7Voice> = (0..32).map(odd_voice).collect();
        let bytes = bank_sysex(&voices);
        assert_eq!(bytes.len(), 4104);
        assert_eq!(&bytes[..6], &[0xF0, 0x43, 0x00, 0x09, 0x20, 0x00]);
        assert_eq!(*bytes.last().unwrap(), 0xF7);
        assert_eq!(parse(&bytes).unwrap(), voices);
        let one = single_sysex(&voices[5]);
        assert_eq!(one.len(), 163);
        assert_eq!(parse(&one).unwrap(), vec![voices[5].clone()]);
        let short_bank = bank_sysex(&voices[..3]);
        let parsed = parse(&short_bank).unwrap();
        assert_eq!(parsed.len(), 32);
        assert_eq!(parsed[3], Dx7Voice::init(), "padded with INIT voices");
    }

    #[test]
    fn bad_files_are_refused() {
        let mut bytes = bank_sysex(&[Dx7Voice::init()]);
        bytes[100] ^= 1;
        assert_eq!(parse(&bytes), Err(SysexError::Checksum));
        assert_eq!(parse(&bank_sysex(&[])[..2000]), Err(SysexError::Truncated));
        assert_eq!(parse(b"hello"), Err(SysexError::NotDx7));
        assert_eq!(parse(&[0xF0, 0x41, 0, 0, 0, 0, 0]), Err(SysexError::NotDx7));
        assert!(!SysexError::Checksum.to_string().is_empty());
    }

    #[test]
    fn out_of_range_bytes_are_clamped() {
        let mut b = pack_voice(&Dx7Voice::init());
        b[0] = 127; // OP6 rate 1
        b[116] = 0x7F; // wave 7, pms 7, sync
        b[110] = 0x7F; // algorithm
        let v = unpack_voice(&b);
        assert!(v.is_valid());
        assert_eq!(v.ops[5].rates[0], 99);
        assert_eq!(v.lfo_wave, 5);
        assert_eq!(v.algorithm, 31);
    }

    #[test]
    fn params_round_trip_and_cover_every_voice_control() {
        let v = odd_voice(11);
        let params = v.to_params();
        assert_eq!(params.len(), 19 + 6 * 22);
        let map: std::collections::HashMap<ParamId, f32> = params.into_iter().collect();
        assert_eq!(map[&ParamId::FmOp(3, crate::params::OpParam::On)], 1.0);
        let back = Dx7Voice::from_params(&v.name, |id| map[&id]);
        assert_eq!(back, v);
    }

    #[test]
    fn names_are_ascii_and_trimmed() {
        let mut v = Dx7Voice::init();
        v.name = "PIANO   ".into();
        let back = unpack_voice(&pack_voice(&v));
        assert_eq!(back.name, "PIANO");
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine fm::sysex`
Expected: failures (stubs).

- [ ] **Step 3: Implement `sysex.rs`**

```rust
//! DX7 voice data: the 32-voice bulk dump (4104 bytes of packed
//! 128-byte voices) and the single-voice dump (163 bytes, one unpacked
//! 155-byte voice), plus conversion to and from FM params.

use crate::params::{FmParam, OpParam, ParamId, fm_steps, step_index};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const BANK_VOICES: usize = 32;
const PACKED: usize = 128;
const UNPACKED: usize = 155;
const BANK_HEADER: [u8; 3] = [0x09, 0x20, 0x00];
const SINGLE_HEADER: [u8; 3] = [0x00, 0x01, 0x1B];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dx7Op {
    pub rates: [u8; 4],
    pub levels: [u8; 4],
    pub break_point: u8,
    pub left_depth: u8,
    pub right_depth: u8,
    pub left_curve: u8,
    pub right_curve: u8,
    pub rate_scaling: u8,
    pub amp_mod_sens: u8,
    pub vel_sens: u8,
    pub level: u8,
    pub fixed: bool,
    pub coarse: u8,
    pub fine: u8,
    /// 0..=14, 7 = centre.
    pub detune: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dx7Voice {
    /// `ops[0]` is OP1.
    pub ops: [Dx7Op; 6],
    pub pitch_rates: [u8; 4],
    pub pitch_levels: [u8; 4],
    /// 0..=31 (algorithm 1 is 0).
    pub algorithm: u8,
    pub feedback: u8,
    pub osc_sync: bool,
    pub lfo_speed: u8,
    pub lfo_delay: u8,
    pub lfo_pmd: u8,
    pub lfo_amd: u8,
    pub lfo_sync: bool,
    pub lfo_wave: u8,
    pub pms: u8,
    /// 0..=48, 24 = no transpose.
    pub transpose: u8,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysexError {
    NotDx7,
    Truncated,
    Checksum,
}

impl fmt::Display for SysexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SysexError::NotDx7 => "it is not a DX7 voice or 32-voice bank",
            SysexError::Truncated => "the file is cut short",
            SysexError::Checksum => "its checksum does not match (the file is damaged)",
        })
    }
}

impl std::error::Error for SysexError {}

fn init_op(level: u8) -> Dx7Op {
    Dx7Op {
        rates: [99; 4],
        levels: [99, 99, 99, 0],
        break_point: 39,
        left_depth: 0,
        right_depth: 0,
        left_curve: 0,
        right_curve: 0,
        rate_scaling: 0,
        amp_mod_sens: 0,
        vel_sens: 0,
        level,
        fixed: false,
        coarse: 1,
        fine: 0,
        detune: 7,
    }
}

impl Dx7Voice {
    pub fn init() -> Self {
        Self {
            ops: [init_op(99), init_op(0), init_op(0), init_op(0), init_op(0), init_op(0)],
            pitch_rates: [99; 4],
            pitch_levels: [50; 4],
            algorithm: 0,
            feedback: 0,
            osc_sync: true,
            lfo_speed: 35,
            lfo_delay: 0,
            lfo_pmd: 0,
            lfo_amd: 0,
            lfo_sync: true,
            lfo_wave: 0,
            pms: 3,
            transpose: 24,
            name: "INIT VOICE".into(),
        }
    }

    /// Every field forced into its DX7 range.
    pub fn clamped(mut self) -> Self {
        for o in &mut self.ops {
            for x in o.rates.iter_mut().chain(o.levels.iter_mut()) {
                *x = (*x).min(99);
            }
            o.break_point = o.break_point.min(99);
            o.left_depth = o.left_depth.min(99);
            o.right_depth = o.right_depth.min(99);
            o.left_curve = o.left_curve.min(3);
            o.right_curve = o.right_curve.min(3);
            o.rate_scaling = o.rate_scaling.min(7);
            o.amp_mod_sens = o.amp_mod_sens.min(3);
            o.vel_sens = o.vel_sens.min(7);
            o.level = o.level.min(99);
            o.coarse = o.coarse.min(31);
            o.fine = o.fine.min(99);
            o.detune = o.detune.min(14);
        }
        for x in self.pitch_rates.iter_mut().chain(self.pitch_levels.iter_mut()) {
            *x = (*x).min(99);
        }
        self.algorithm = self.algorithm.min(31);
        self.feedback = self.feedback.min(7);
        self.lfo_speed = self.lfo_speed.min(99);
        self.lfo_delay = self.lfo_delay.min(99);
        self.lfo_pmd = self.lfo_pmd.min(99);
        self.lfo_amd = self.lfo_amd.min(99);
        self.lfo_wave = self.lfo_wave.min(5);
        self.pms = self.pms.min(7);
        self.transpose = self.transpose.min(48);
        self.name = self.name.chars().filter(|c| (' '..='~').contains(c)).take(10).collect::<String>().trim_end().to_string();
        self
    }

    pub fn is_valid(&self) -> bool {
        self.clone().clamped() == *self
    }

    /// The voice as FM params (0..1 values). Loading a voice also turns
    /// every operator on.
    pub fn to_params(&self) -> Vec<(ParamId, f32)> {
        use FmParam as F;
        use OpParam as O;
        let n99 = |x: u8| x as f32 / 99.0;
        let step = |x: u8, n: usize| x as f32 / (n - 1) as f32;
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        let mut out = vec![
            (ParamId::Fm(F::Algorithm), step(self.algorithm, fm_steps::ALGORITHM)),
            (ParamId::Fm(F::Feedback), step(self.feedback, fm_steps::FEEDBACK)),
            (ParamId::Fm(F::OscSync), flag(self.osc_sync)),
            (ParamId::Fm(F::PitchR1), n99(self.pitch_rates[0])),
            (ParamId::Fm(F::PitchR2), n99(self.pitch_rates[1])),
            (ParamId::Fm(F::PitchR3), n99(self.pitch_rates[2])),
            (ParamId::Fm(F::PitchR4), n99(self.pitch_rates[3])),
            (ParamId::Fm(F::PitchL1), n99(self.pitch_levels[0])),
            (ParamId::Fm(F::PitchL2), n99(self.pitch_levels[1])),
            (ParamId::Fm(F::PitchL3), n99(self.pitch_levels[2])),
            (ParamId::Fm(F::PitchL4), n99(self.pitch_levels[3])),
            (ParamId::Fm(F::LfoSpeed), n99(self.lfo_speed)),
            (ParamId::Fm(F::LfoDelay), n99(self.lfo_delay)),
            (ParamId::Fm(F::LfoPmd), n99(self.lfo_pmd)),
            (ParamId::Fm(F::LfoAmd), n99(self.lfo_amd)),
            (ParamId::Fm(F::LfoSync), flag(self.lfo_sync)),
            (ParamId::Fm(F::LfoWave), step(self.lfo_wave, fm_steps::LFO_WAVES)),
            (ParamId::Fm(F::PitchModSens), step(self.pms, fm_steps::PITCH_MOD_SENS)),
            (ParamId::Fm(F::Transpose), step(self.transpose, fm_steps::TRANSPOSE)),
        ];
        for (op, o) in self.ops.iter().enumerate() {
            let id = |p| ParamId::FmOp(op as u8, p);
            out.extend([
                (id(O::R1), n99(o.rates[0])),
                (id(O::R2), n99(o.rates[1])),
                (id(O::R3), n99(o.rates[2])),
                (id(O::R4), n99(o.rates[3])),
                (id(O::L1), n99(o.levels[0])),
                (id(O::L2), n99(o.levels[1])),
                (id(O::L3), n99(o.levels[2])),
                (id(O::L4), n99(o.levels[3])),
                (id(O::BreakPoint), n99(o.break_point)),
                (id(O::LeftDepth), n99(o.left_depth)),
                (id(O::RightDepth), n99(o.right_depth)),
                (id(O::LeftCurve), step(o.left_curve, fm_steps::CURVES)),
                (id(O::RightCurve), step(o.right_curve, fm_steps::CURVES)),
                (id(O::RateScaling), step(o.rate_scaling, fm_steps::RATE_SCALING)),
                (id(O::AmpModSens), step(o.amp_mod_sens, fm_steps::AMP_MOD_SENS)),
                (id(O::VelSens), step(o.vel_sens, fm_steps::VEL_SENS)),
                (id(O::Level), n99(o.level)),
                (id(O::FixedFreq), flag(o.fixed)),
                (id(O::Coarse), step(o.coarse, fm_steps::COARSE)),
                (id(O::Fine), n99(o.fine)),
                (id(O::Detune), step(o.detune, fm_steps::DETUNE)),
                (id(O::On), 1.0),
            ]);
        }
        out
    }

    /// The voice the current FM params describe.
    pub fn from_params(name: &str, value: impl Fn(ParamId) -> f32) -> Self {
        use FmParam as F;
        use OpParam as O;
        let d = |id| (value(id).clamp(0.0, 1.0) * 99.0).round() as u8;
        let s = |id, n: usize| step_index(value(id), n as u8) as u8;
        let b = |id| value(id) >= 0.5;
        let g = |p| ParamId::Fm(p);
        let mut v = Dx7Voice::init();
        v.name = name.to_string();
        v.algorithm = s(g(F::Algorithm), fm_steps::ALGORITHM);
        v.feedback = s(g(F::Feedback), fm_steps::FEEDBACK);
        v.osc_sync = b(g(F::OscSync));
        v.pitch_rates = [d(g(F::PitchR1)), d(g(F::PitchR2)), d(g(F::PitchR3)), d(g(F::PitchR4))];
        v.pitch_levels = [d(g(F::PitchL1)), d(g(F::PitchL2)), d(g(F::PitchL3)), d(g(F::PitchL4))];
        v.lfo_speed = d(g(F::LfoSpeed));
        v.lfo_delay = d(g(F::LfoDelay));
        v.lfo_pmd = d(g(F::LfoPmd));
        v.lfo_amd = d(g(F::LfoAmd));
        v.lfo_sync = b(g(F::LfoSync));
        v.lfo_wave = s(g(F::LfoWave), fm_steps::LFO_WAVES);
        v.pms = s(g(F::PitchModSens), fm_steps::PITCH_MOD_SENS);
        v.transpose = s(g(F::Transpose), fm_steps::TRANSPOSE);
        for (op, o) in v.ops.iter_mut().enumerate() {
            let id = |p| ParamId::FmOp(op as u8, p);
            o.rates = [d(id(O::R1)), d(id(O::R2)), d(id(O::R3)), d(id(O::R4))];
            o.levels = [d(id(O::L1)), d(id(O::L2)), d(id(O::L3)), d(id(O::L4))];
            o.break_point = d(id(O::BreakPoint));
            o.left_depth = d(id(O::LeftDepth));
            o.right_depth = d(id(O::RightDepth));
            o.left_curve = s(id(O::LeftCurve), fm_steps::CURVES);
            o.right_curve = s(id(O::RightCurve), fm_steps::CURVES);
            o.rate_scaling = s(id(O::RateScaling), fm_steps::RATE_SCALING);
            o.amp_mod_sens = s(id(O::AmpModSens), fm_steps::AMP_MOD_SENS);
            o.vel_sens = s(id(O::VelSens), fm_steps::VEL_SENS);
            o.level = d(id(O::Level));
            o.fixed = b(id(O::FixedFreq));
            o.coarse = s(id(O::Coarse), fm_steps::COARSE);
            o.fine = d(id(O::Fine));
            o.detune = s(id(O::Detune), fm_steps::DETUNE);
        }
        v
    }
}

fn checksum(data: &[u8]) -> u8 {
    let sum = data.iter().fold(0u32, |a, &b| a + b as u32);
    ((128 - sum % 128) % 128) as u8
}

fn name_from(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| if (32..127).contains(&b) { b as char } else { ' ' })
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn name_bytes(name: &str) -> [u8; 10] {
    let mut out = [b' '; 10];
    for (o, c) in out.iter_mut().zip(name.bytes().filter(|b| (32..127).contains(b))) {
        *o = c;
    }
    out
}

pub fn pack_voice(v: &Dx7Voice) -> [u8; PACKED] {
    let mut b = [0u8; PACKED];
    for (op, o) in v.ops.iter().enumerate() {
        let k = (5 - op) * 17;
        b[k..k + 4].copy_from_slice(&o.rates);
        b[k + 4..k + 8].copy_from_slice(&o.levels);
        b[k + 8] = o.break_point;
        b[k + 9] = o.left_depth;
        b[k + 10] = o.right_depth;
        b[k + 11] = (o.left_curve & 3) | (o.right_curve & 3) << 2;
        b[k + 12] = (o.rate_scaling & 7) | (o.detune & 15) << 3;
        b[k + 13] = (o.amp_mod_sens & 3) | (o.vel_sens & 7) << 2;
        b[k + 14] = o.level;
        b[k + 15] = o.fixed as u8 | (o.coarse & 31) << 1;
        b[k + 16] = o.fine;
    }
    b[102..106].copy_from_slice(&v.pitch_rates);
    b[106..110].copy_from_slice(&v.pitch_levels);
    b[110] = v.algorithm & 31;
    b[111] = (v.feedback & 7) | (v.osc_sync as u8) << 3;
    b[112] = v.lfo_speed;
    b[113] = v.lfo_delay;
    b[114] = v.lfo_pmd;
    b[115] = v.lfo_amd;
    b[116] = v.lfo_sync as u8 | (v.lfo_wave & 7) << 1 | (v.pms & 7) << 4;
    b[117] = v.transpose;
    b[118..128].copy_from_slice(&name_bytes(&v.name));
    b
}

pub fn unpack_voice(b: &[u8; PACKED]) -> Dx7Voice {
    let mut v = Dx7Voice::init();
    for (op, o) in v.ops.iter_mut().enumerate() {
        let k = (5 - op) * 17;
        o.rates.copy_from_slice(&b[k..k + 4]);
        o.levels.copy_from_slice(&b[k + 4..k + 8]);
        o.break_point = b[k + 8];
        o.left_depth = b[k + 9];
        o.right_depth = b[k + 10];
        o.left_curve = b[k + 11] & 3;
        o.right_curve = (b[k + 11] >> 2) & 3;
        o.rate_scaling = b[k + 12] & 7;
        o.detune = (b[k + 12] >> 3) & 15;
        o.amp_mod_sens = b[k + 13] & 3;
        o.vel_sens = (b[k + 13] >> 2) & 7;
        o.level = b[k + 14];
        o.fixed = b[k + 15] & 1 != 0;
        o.coarse = (b[k + 15] >> 1) & 31;
        o.fine = b[k + 16];
    }
    v.pitch_rates.copy_from_slice(&b[102..106]);
    v.pitch_levels.copy_from_slice(&b[106..110]);
    v.algorithm = b[110] & 31;
    v.feedback = b[111] & 7;
    v.osc_sync = (b[111] >> 3) & 1 != 0;
    v.lfo_speed = b[112];
    v.lfo_delay = b[113];
    v.lfo_pmd = b[114];
    v.lfo_amd = b[115];
    v.lfo_sync = b[116] & 1 != 0;
    v.lfo_wave = (b[116] >> 1) & 7;
    v.pms = (b[116] >> 4) & 7;
    v.transpose = b[117];
    v.name = name_from(&b[118..128]);
    v.clamped()
}

fn to_unpacked(v: &Dx7Voice) -> [u8; UNPACKED] {
    let mut b = [0u8; UNPACKED];
    for (op, o) in v.ops.iter().enumerate() {
        let k = (5 - op) * 21;
        b[k..k + 4].copy_from_slice(&o.rates);
        b[k + 4..k + 8].copy_from_slice(&o.levels);
        b[k + 8..k + 21].copy_from_slice(&[
            o.break_point,
            o.left_depth,
            o.right_depth,
            o.left_curve,
            o.right_curve,
            o.rate_scaling,
            o.amp_mod_sens,
            o.vel_sens,
            o.level,
            o.fixed as u8,
            o.coarse,
            o.fine,
            o.detune,
        ]);
    }
    b[126..130].copy_from_slice(&v.pitch_rates);
    b[130..134].copy_from_slice(&v.pitch_levels);
    b[134..145].copy_from_slice(&[
        v.algorithm,
        v.feedback,
        v.osc_sync as u8,
        v.lfo_speed,
        v.lfo_delay,
        v.lfo_pmd,
        v.lfo_amd,
        v.lfo_sync as u8,
        v.lfo_wave,
        v.pms,
        v.transpose,
    ]);
    b[145..155].copy_from_slice(&name_bytes(&v.name));
    b
}

fn from_unpacked(b: &[u8; UNPACKED]) -> Dx7Voice {
    let mut v = Dx7Voice::init();
    for (op, o) in v.ops.iter_mut().enumerate() {
        let k = (5 - op) * 21;
        o.rates.copy_from_slice(&b[k..k + 4]);
        o.levels.copy_from_slice(&b[k + 4..k + 8]);
        o.break_point = b[k + 8];
        o.left_depth = b[k + 9];
        o.right_depth = b[k + 10];
        o.left_curve = b[k + 11];
        o.right_curve = b[k + 12];
        o.rate_scaling = b[k + 13];
        o.amp_mod_sens = b[k + 14];
        o.vel_sens = b[k + 15];
        o.level = b[k + 16];
        o.fixed = b[k + 17] & 1 != 0;
        o.coarse = b[k + 18];
        o.fine = b[k + 19];
        o.detune = b[k + 20];
    }
    v.pitch_rates.copy_from_slice(&b[126..130]);
    v.pitch_levels.copy_from_slice(&b[130..134]);
    v.algorithm = b[134];
    v.feedback = b[135];
    v.osc_sync = b[136] & 1 != 0;
    v.lfo_speed = b[137];
    v.lfo_delay = b[138];
    v.lfo_pmd = b[139];
    v.lfo_amd = b[140];
    v.lfo_sync = b[141] & 1 != 0;
    v.lfo_wave = b[142];
    v.pms = b[143];
    v.transpose = b[144];
    v.name = name_from(&b[145..155]);
    v.clamped()
}

fn message(header: [u8; 3], data: &[u8]) -> Vec<u8> {
    let mut out = vec![0xF0, 0x43, 0x00, header[0], header[1], header[2]];
    out.extend_from_slice(data);
    out.push(checksum(data));
    out.push(0xF7);
    out
}

/// A 32-voice bank dump; fewer voices are padded with INIT voices.
pub fn bank_sysex(voices: &[Dx7Voice]) -> Vec<u8> {
    let init = Dx7Voice::init();
    let mut data = Vec::with_capacity(BANK_VOICES * PACKED);
    for i in 0..BANK_VOICES {
        data.extend_from_slice(&pack_voice(voices.get(i).unwrap_or(&init)));
    }
    message(BANK_HEADER, &data)
}

pub fn single_sysex(voice: &Dx7Voice) -> Vec<u8> {
    message(SINGLE_HEADER, &to_unpacked(voice))
}

/// The voices in a DX7 bank (32) or single-voice (1) dump. Bytes after
/// the first message are ignored.
pub fn parse(bytes: &[u8]) -> Result<Vec<Dx7Voice>, SysexError> {
    if bytes.len() < 6 || bytes[0] != 0xF0 || bytes[1] != 0x43 || bytes[2] & 0xF0 != 0 {
        return Err(SysexError::NotDx7);
    }
    let header = [bytes[3], bytes[4], bytes[5]];
    let len = match header {
        BANK_HEADER => BANK_VOICES * PACKED,
        SINGLE_HEADER => UNPACKED,
        _ => return Err(SysexError::NotDx7),
    };
    if bytes.len() < 6 + len + 1 {
        return Err(SysexError::Truncated);
    }
    let data = &bytes[6..6 + len];
    if checksum(data) != bytes[6 + len] {
        return Err(SysexError::Checksum);
    }
    Ok(if header == BANK_HEADER {
        data.chunks_exact(PACKED)
            .map(|c| unpack_voice(c.try_into().expect("128-byte chunk")))
            .collect()
    } else {
        vec![from_unpacked(data.try_into().expect("155 bytes"))]
    })
}
```

In `names_are_ascii_and_trimmed`, `clamped()` trims trailing spaces, so a voice that round-trips must already be trimmed. That is why `odd_voice` names have no trailing spaces.

- [ ] **Step 4: Add the module and run everything**

Add `pub mod sysex;` to `fm/mod.rs`.

Run: `cargo test -p acid-engine fm::sysex && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "feat(fm): DX7 SysEx bank and single-voice import/export

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Starter bank, and FM voice storage in the project

**Files:**
- Create: `crates/acid-engine/src/fm/starter.rs`
- Modify: `crates/acid-engine/src/fm/mod.rs` (`pub mod starter;`), `crates/acid-engine/src/project.rs`

**Interfaces:**
- Consumes: `Dx7Voice`, `Dx7Op` (Task 7); `FmSynth` (Task 5).
- Produces:

```rust
pub fn starter_bank() -> Vec<Dx7Voice>;   // 16 original voices
// project.rs
pub struct FmMachine {
    pub patterns: Vec<NotePattern>,
    #[serde(default = "init_voice_name")] pub voice_name: String,   // "INIT VOICE"
    #[serde(default)] pub bank: Option<Vec<Dx7Voice>>,              // last imported bank
}
```

- [ ] **Step 1: Write the failing tests**

`starter.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::fm::FmSynth;
    use crate::params::ParamId;
    use crate::testutil::{all_finite, peak};

    fn load(s: &mut FmSynth, v: &Dx7Voice) {
        for (id, x) in v.to_params() {
            match id {
                ParamId::Fm(p) => s.set_param(p, x),
                ParamId::FmOp(op, p) => s.set_op_param(op as usize, p, x),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn sixteen_valid_uniquely_named_voices() {
        let bank = starter_bank();
        assert_eq!(bank.len(), 16);
        let names: std::collections::HashSet<_> = bank.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names.len(), 16);
        for v in &bank {
            assert!(v.is_valid(), "{}", v.name);
            assert!(!v.name.is_empty() && v.name.len() <= 10);
        }
    }

    #[test]
    fn every_voice_plays_and_stays_bounded() {
        for v in starter_bank() {
            let mut s = FmSynth::new(48_000.0);
            load(&mut s, &v);
            s.note_on(48, 110, false);
            let n = 24_000;
            let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
            s.render(&mut l, &mut r);
            assert!(all_finite(&l), "{}", v.name);
            assert!(peak(&l) > 0.01, "{} is silent", v.name);
            assert!(peak(&l) < 4.0, "{} is too loud: {}", v.name, peak(&l));
        }
    }
}
```

In `project.rs` tests, add:

```rust
    #[test]
    fn fm_voice_name_and_bank_round_trip_and_default() {
        let mut p = Project::default();
        assert_eq!(p.fm.voice_name, "INIT VOICE");
        p.fm.voice_name = "E.PIANO 1".into();
        p.fm.bank = Some(crate::fm::starter::starter_bank());
        let back = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(back, p);
        let old: String = Project::default()
            .to_ron()
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("voice_name:") && !t.starts_with("bank:")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let loaded = Project::from_ron(&old).unwrap();
        assert_eq!(loaded.fm.voice_name, "INIT VOICE");
        assert_eq!(loaded.fm.bank, None);
    }

    #[test]
    fn a_damaged_fm_bank_is_rejected() {
        let mut p = Project::default();
        let mut bad = crate::fm::sysex::Dx7Voice::init();
        bad.algorithm = 40;
        p.fm.bank = Some(vec![bad]);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        p.fm.bank = Some(vec![crate::fm::sysex::Dx7Voice::init(); 33]);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine`
Expected: compile errors.

- [ ] **Step 3: Implement `starter.rs`**

```rust
//! The starter bank: 16 original voices (not Yamaha ROM sounds).
//! Operators are listed OP1..OP6; algorithms use DX7 numbering (1..=32).

use super::sysex::{Dx7Op, Dx7Voice};

/// An operator: ratio `coarse` (+`fine` %), output `level`, envelope.
fn op(coarse: u8, fine: u8, level: u8, rates: [u8; 4], levels: [u8; 4]) -> Dx7Op {
    Dx7Op {
        rates,
        levels,
        break_point: 39,
        left_depth: 0,
        right_depth: 0,
        left_curve: 0,
        right_curve: 0,
        rate_scaling: 2,
        amp_mod_sens: 0,
        vel_sens: 3,
        level,
        fixed: false,
        coarse,
        fine,
        detune: 7,
    }
}

fn detuned(mut o: Dx7Op, detune: u8) -> Dx7Op {
    o.detune = detune;
    o
}

fn velocity(mut o: Dx7Op, vel_sens: u8) -> Dx7Op {
    o.vel_sens = vel_sens;
    o
}

/// An operator that is silent (level 0) in this voice.
fn off() -> Dx7Op {
    op(1, 0, 0, [99, 99, 99, 99], [99, 99, 99, 0])
}

fn voice(name: &str, algorithm: u8, feedback: u8, ops: [Dx7Op; 6]) -> Dx7Voice {
    let mut v = Dx7Voice::init();
    v.name = name.into();
    v.algorithm = algorithm - 1;
    v.feedback = feedback;
    v.ops = ops;
    v
}

pub fn starter_bank() -> Vec<Dx7Voice> {
    let sustain = [99, 99, 99, 0];
    let mut kick = voice(
        "KICK FM",
        1,
        0,
        [
            op(0, 0, 99, [99, 55, 40, 70], [99, 0, 0, 0]),
            op(1, 0, 60, [99, 90, 60, 70], [99, 0, 0, 0]),
            off(),
            off(),
            off(),
            off(),
        ],
    );
    kick.pitch_rates = [75, 99, 99, 99];
    kick.pitch_levels = [50, 50, 50, 75];
    vec![
        voice(
            "E.PIANO 1",
            5,
            6,
            [
                velocity(op(1, 0, 99, [96, 25, 25, 67], [99, 75, 0, 0]), 2),
                velocity(op(14, 0, 58, [95, 50, 35, 78], [99, 75, 0, 0]), 7),
                velocity(op(1, 0, 99, [95, 20, 20, 50], [99, 95, 0, 0]), 2),
                velocity(op(1, 0, 89, [95, 29, 20, 50], [99, 95, 0, 0]), 6),
                op(1, 0, 99, [95, 20, 20, 50], [99, 95, 0, 0]),
                detuned(velocity(op(1, 0, 79, [95, 29, 20, 50], [99, 95, 0, 0]), 6), 9),
            ],
        ),
        voice(
            "FM BASS",
            1,
            7,
            [
                op(1, 0, 99, [99, 50, 35, 60], [99, 80, 0, 0]),
                op(1, 0, 85, [99, 60, 40, 60], [99, 50, 0, 0]),
                op(1, 0, 94, [99, 45, 35, 60], [99, 80, 0, 0]),
                op(2, 0, 70, [99, 70, 40, 60], [99, 40, 0, 0]),
                op(1, 0, 60, [99, 60, 40, 60], [99, 50, 0, 0]),
                op(3, 0, 60, [99, 60, 40, 60], [99, 40, 0, 0]),
            ],
        ),
        voice(
            "ACID FM",
            1,
            6,
            [
                op(1, 0, 99, [99, 40, 30, 70], [99, 90, 85, 0]),
                velocity(op(1, 0, 90, [99, 55, 45, 70], [99, 30, 0, 0]), 6),
                off(),
                off(),
                off(),
                off(),
            ],
        ),
        voice(
            "BELLS",
            5,
            0,
            [
                op(1, 0, 99, [99, 30, 20, 30], [99, 0, 0, 0]),
                op(3, 50, 75, [99, 35, 20, 30], [99, 0, 0, 0]),
                op(2, 0, 90, [99, 28, 20, 30], [99, 0, 0, 0]),
                op(7, 0, 60, [99, 40, 20, 30], [99, 0, 0, 0]),
                op(0, 0, 85, [99, 25, 20, 30], [99, 0, 0, 0]),
                op(5, 41, 50, [99, 45, 20, 30], [99, 0, 0, 0]),
            ],
        ),
        voice(
            "MARIMBA",
            5,
            0,
            [
                op(1, 0, 99, [99, 50, 30, 50], [99, 0, 0, 0]),
                op(4, 0, 80, [99, 75, 50, 50], [99, 0, 0, 0]),
                op(1, 0, 80, [99, 50, 30, 50], [99, 0, 0, 0]),
                op(10, 0, 60, [99, 85, 50, 50], [99, 0, 0, 0]),
                off(),
                off(),
            ],
        ),
        voice(
            "BRASS",
            22,
            7,
            [
                op(1, 0, 99, [70, 60, 40, 60], [99, 92, 90, 0]),
                op(1, 0, 82, [65, 55, 40, 60], [99, 90, 85, 0]),
                detuned(op(1, 0, 99, [70, 60, 40, 60], [99, 92, 90, 0]), 8),
                detuned(op(1, 0, 99, [70, 60, 40, 60], [99, 92, 90, 0]), 6),
                op(1, 0, 99, [70, 60, 40, 60], [99, 92, 90, 0]),
                op(1, 0, 80, [60, 50, 40, 60], [99, 85, 80, 0]),
            ],
        ),
        voice(
            "STRINGS",
            1,
            4,
            [
                op(1, 0, 99, [45, 30, 25, 40], [99, 95, 95, 0]),
                op(1, 0, 72, [45, 30, 25, 40], [99, 90, 90, 0]),
                detuned(op(1, 0, 99, [45, 30, 25, 40], [99, 95, 95, 0]), 9),
                op(3, 0, 60, [45, 30, 25, 40], [99, 90, 90, 0]),
                op(1, 0, 55, [45, 30, 25, 40], [99, 90, 90, 0]),
                detuned(op(1, 0, 50, [45, 30, 25, 40], [99, 90, 90, 0]), 5),
            ],
        ),
        voice(
            "PAD",
            5,
            3,
            [
                detuned(op(1, 0, 99, [35, 25, 20, 30], [99, 95, 95, 0]), 4),
                op(2, 0, 55, [35, 25, 20, 30], [99, 90, 90, 0]),
                detuned(op(1, 0, 99, [35, 25, 20, 30], [99, 95, 95, 0]), 10),
                op(1, 0, 50, [35, 25, 20, 30], [99, 90, 90, 0]),
                op(0, 0, 90, [35, 25, 20, 30], [99, 95, 95, 0]),
                op(3, 0, 45, [35, 25, 20, 30], [99, 90, 90, 0]),
            ],
        ),
        voice(
            "ORGAN",
            32,
            0,
            [
                op(0, 0, 90, [99, 99, 99, 90], sustain),
                op(1, 0, 99, [99, 99, 99, 90], sustain),
                op(2, 0, 85, [99, 99, 99, 90], sustain),
                op(3, 0, 75, [99, 99, 99, 90], sustain),
                op(4, 0, 70, [99, 99, 99, 90], sustain),
                op(8, 0, 60, [99, 99, 99, 90], sustain),
            ],
        ),
        voice(
            "PLUCK",
            1,
            5,
            [
                op(1, 0, 99, [99, 55, 30, 70], [99, 0, 0, 0]),
                op(3, 0, 80, [99, 70, 30, 70], [99, 0, 0, 0]),
                op(1, 0, 80, [99, 60, 30, 70], [99, 0, 0, 0]),
                op(1, 0, 75, [99, 80, 30, 70], [99, 0, 0, 0]),
                op(5, 0, 60, [99, 75, 30, 70], [99, 0, 0, 0]),
                op(7, 0, 50, [99, 80, 30, 70], [99, 0, 0, 0]),
            ],
        ),
        voice(
            "LEAD",
            1,
            6,
            [
                op(1, 0, 99, [99, 40, 30, 60], [99, 95, 95, 0]),
                op(1, 0, 85, [99, 40, 30, 60], [99, 90, 90, 0]),
                op(2, 0, 80, [99, 40, 30, 60], [99, 95, 95, 0]),
                op(1, 0, 75, [99, 40, 30, 60], [99, 90, 90, 0]),
                op(1, 0, 65, [99, 40, 30, 60], [99, 90, 90, 0]),
                op(1, 0, 70, [99, 40, 30, 60], [99, 90, 90, 0]),
            ],
        ),
        voice(
            "CLAV",
            5,
            4,
            [
                op(1, 0, 99, [99, 70, 40, 80], [99, 60, 0, 0]),
                op(3, 0, 85, [99, 80, 40, 80], [99, 40, 0, 0]),
                op(1, 0, 90, [99, 70, 40, 80], [99, 60, 0, 0]),
                op(5, 0, 75, [99, 85, 40, 80], [99, 30, 0, 0]),
                op(2, 0, 80, [99, 70, 40, 80], [99, 60, 0, 0]),
                op(7, 0, 65, [99, 85, 40, 80], [99, 30, 0, 0]),
            ],
        ),
        kick,
        voice(
            "SNARE FM",
            2,
            7,
            [
                op(1, 0, 99, [99, 65, 50, 70], [99, 0, 0, 0]),
                op(11, 37, 95, [99, 70, 50, 70], [99, 0, 0, 0]),
                off(),
                off(),
                off(),
                off(),
            ],
        ),
        voice(
            "HAT FM",
            2,
            7,
            [
                op(9, 33, 99, [99, 85, 60, 85], [99, 0, 0, 0]),
                op(13, 61, 99, [99, 85, 60, 85], [99, 0, 0, 0]),
                off(),
                off(),
                off(),
                off(),
            ],
        ),
        voice(
            "TUBULAR",
            5,
            0,
            [
                op(1, 0, 99, [99, 25, 20, 25], [99, 0, 0, 0]),
                op(3, 50, 78, [99, 30, 20, 25], [99, 0, 0, 0]),
                op(1, 0, 95, [99, 25, 20, 25], [99, 0, 0, 0]),
                op(7, 0, 60, [99, 35, 20, 25], [99, 0, 0, 0]),
                op(2, 0, 90, [99, 25, 20, 25], [99, 0, 0, 0]),
                op(5, 0, 55, [99, 35, 20, 25], [99, 0, 0, 0]),
            ],
        ),
    ]
}
```

The order is E.PIANO 1, FM BASS, ACID FM, BELLS, MARIMBA, BRASS, STRINGS, PAD, ORGAN, PLUCK, LEAD, CLAV, KICK FM, SNARE FM, HAT FM, TUBULAR.

If `every_voice_plays_and_stays_bounded` reports one voice too loud (peak ≥ 4) or silent, adjust only that voice's levels. Keep its character, and say so in the report. Don't loosen the test.

- [ ] **Step 4: Store the voice name and bank in the project**

In `project.rs`, extend `FmMachine`:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FmMachine {
    pub patterns: Vec<NotePattern>,
    /// Name of the voice the FM params were last loaded from.
    #[serde(default = "init_voice_name")]
    pub voice_name: String,
    /// The last imported DX7 bank, shown in the voice browser.
    #[serde(default)]
    pub bank: Option<Vec<crate::fm::sysex::Dx7Voice>>,
}

fn init_voice_name() -> String {
    "INIT VOICE".into()
}
```

Set `voice_name: init_voice_name(), bank: None` in its `Default`.

In `validate`, after the note-machine checks:

```rust
        if let Some(bank) = &self.fm.bank
            && (bank.len() > crate::fm::sysex::BANK_VOICES || bank.iter().any(|v| !v.is_valid()))
        {
            return bad("FM voice bank".into());
        }
```

`migrate.rs` builds a v2 project from `Project::default()`, so it needs no change.

- [ ] **Step 5: Add the module and run everything**

Add `pub mod starter;` to `fm/mod.rs`.

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(fm): 16-voice starter bank; project keeps voice name and imported bank

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Undoable voice loading and .syx import in the app

**Files:**
- Modify: `crates/app/src/document.rs`
- Create: `crates/app/src/ui/fm_voices.rs`
- Modify: `crates/app/src/ui/mod.rs` (module, `ViewState` fields, tests)

**Interfaces:**
- Consumes: `Dx7Voice::to_params`, `sysex::parse`, `starter_bank` (Tasks 7 and 8); `FmMachine.voice_name` and `FmMachine.bank`.
- Produces:

```rust
// document.rs
Edit::FmVoice { name: String, values: Vec<(ParamId, f32)> }   // one undo step; inverse restores old name + values
Edit::FmBank { bank: Option<Vec<Dx7Voice>> }
// ui/fm_voices.rs
pub enum FmBrowser { Starter, Imported }   // Default = Starter
pub(crate) fn starter() -> &'static [Dx7Voice];             // built once
impl SidAcidApp {
    pub(crate) fn load_fm_voice(&mut self, voice: &Dx7Voice);
    pub(crate) fn import_syx(&mut self);                     // file dialog
    pub(crate) fn import_syx_bytes(&mut self, bytes: &[u8]);
}
// ViewState
pub fm_browser: fm_voices::FmBrowser,
/// Operator shown in the FM operator editor (0..6).
pub fm_op: usize,
```

- [ ] **Step 1: Write the failing tests**

In `document.rs` tests:

```rust
    #[test]
    fn loading_an_fm_voice_is_one_undo_step() {
        use acid_engine::params::{FmParam, OpParam};
        let mut d = doc();
        let alg = ParamId::Fm(FmParam::Algorithm);
        let lvl = ParamId::FmOp(2, OpParam::Level);
        let cmds = d.apply(
            Edit::FmVoice {
                name: "BRASS".into(),
                values: vec![(alg, 1.0), (lvl, 0.5)],
            },
            false,
        );
        assert_eq!(d.project.fm.voice_name, "BRASS");
        assert_eq!(d.project.param(alg), 1.0);
        assert_eq!(cmds.len(), 2);
        assert!(cmds.iter().all(|c| matches!(c, Command::SetParam(..))));
        d.undo();
        assert_eq!(d.project.fm.voice_name, "INIT VOICE");
        assert_eq!(d.project.param(alg), alg.info().default);
        assert_eq!(d.project.param(lvl), lvl.info().default);
    }
```

In `ui/mod.rs` tests (follow the headless setup the neighbouring tests use):

```rust
    #[test]
    fn importing_a_bank_and_loading_voices() {
        use acid_engine::fm::sysex::{bank_sysex, single_sysex};
        use acid_engine::params::{FmParam, ParamId};
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let bank = fm_voices::starter().to_vec();
        app.import_syx_bytes(&bank_sysex(&bank));
        assert_eq!(app.doc.project.fm.bank.as_ref().map(Vec::len), Some(32));
        assert_eq!(app.view.fm_browser, fm_voices::FmBrowser::Imported);
        app.doc.undo();
        assert_eq!(app.doc.project.fm.bank, None);

        app.import_syx_bytes(&single_sysex(&bank[5]));
        assert_eq!(app.doc.project.fm.voice_name, bank[5].name);
        let alg = ParamId::Fm(FmParam::Algorithm);
        assert!((app.doc.project.param(alg) - bank[5].algorithm as f32 / 31.0).abs() < 1e-6);

        let before = app.doc.project.clone();
        app.import_syx_bytes(b"not sysex");
        assert!(app.view.message.as_deref().unwrap_or("").contains("DX7"));
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn the_starter_bank_is_built_once() {
        assert!(std::ptr::eq(fm_voices::starter(), fm_voices::starter()));
        assert_eq!(fm_voices::starter().len(), 16);
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p sidacid`
Expected: compile errors.

- [ ] **Step 3: Implement the edits** in `document.rs`

Add the import `use acid_engine::fm::sysex::Dx7Voice;`. Add to `Edit`:

```rust
    /// Loading an FM voice: many params and the voice name, one undo step.
    FmVoice {
        name: String,
        values: Vec<(ParamId, f32)>,
    },
    FmBank {
        bank: Option<Vec<Dx7Voice>>,
    },
```

Add `MergeKey::FmVoice` and `MergeKey::FmBank`, with the `merge_key` arms `Edit::FmVoice { .. } => MergeKey::FmVoice` and `Edit::FmBank { .. } => MergeKey::FmBank`.

Add these `apply_raw` arms:

```rust
            Edit::FmVoice { name, values } => {
                let old_name = std::mem::replace(&mut p.fm.voice_name, name);
                let old_values = values
                    .into_iter()
                    .map(|(id, v)| {
                        let old = p.param(id);
                        p.params.insert(id, v.clamp(0.0, 1.0));
                        (id, old)
                    })
                    .collect();
                Edit::FmVoice {
                    name: old_name,
                    values: old_values,
                }
            }
            Edit::FmBank { bank } => Edit::FmBank {
                bank: std::mem::replace(&mut p.fm.bank, bank),
            },
```

Add these `commands_for` arms:

```rust
            Edit::FmVoice { values, .. } => values
                .iter()
                .map(|(id, _)| Command::SetParam(*id, p.param(*id)))
                .collect(),
            Edit::FmBank { .. } => Vec::new(),
```

- [ ] **Step 4: Implement `ui/fm_voices.rs`**

```rust
//! Loading FM voices: from the starter bank, an imported DX7 bank, or a
//! .syx file. A load is one undoable edit.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::fm::starter::starter_bank;
use acid_engine::fm::sysex::{self, Dx7Voice};
use acid_engine::machine::MachineId;
use std::sync::OnceLock;

/// Which list the voice browser shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FmBrowser {
    #[default]
    Starter,
    Imported,
}

/// The starter bank, built on first use.
pub(crate) fn starter() -> &'static [Dx7Voice] {
    static BANK: OnceLock<Vec<Dx7Voice>> = OnceLock::new();
    BANK.get_or_init(starter_bank)
}

impl SidAcidApp {
    pub(crate) fn load_fm_voice(&mut self, voice: &Dx7Voice) {
        let cmds = self.doc.apply(
            Edit::FmVoice {
                name: voice.name.clone(),
                values: voice.to_params(),
            },
            false,
        );
        self.send(cmds);
        self.view.focus = MachineId::Fm.index();
    }

    pub(crate) fn import_syx(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DX7 SysEx", &["syx", "SYX"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) => self.import_syx_bytes(&bytes),
            Err(e) => self.view.message = Some(format!("Could not read {}:\n{e}", path.display())),
        }
    }

    /// A single voice loads straight into the FM; a bank goes to the browser.
    pub(crate) fn import_syx_bytes(&mut self, bytes: &[u8]) {
        match sysex::parse(bytes) {
            Ok(voices) if voices.len() == 1 => self.load_fm_voice(&voices[0]),
            Ok(voices) => {
                let cmds = self.doc.apply(Edit::FmBank { bank: Some(voices) }, false);
                self.send(cmds);
                self.view.fm_browser = FmBrowser::Imported;
            }
            Err(e) => {
                self.view.message = Some(format!("Could not import the file: {e}. Use a DX7 .syx file."))
            }
        }
    }
}
```

The bad-file message contains "DX7" for every error, because the closing sentence names it. In `ui/mod.rs`, add `mod fm_voices;` (alphabetical) and the `ViewState` fields `pub fm_browser: fm_voices::FmBrowser,` and `pub fm_op: usize,` (doc comment "Operator shown in the FM operator editor (0..6)."). In `files.rs` `load_document`, add `self.view.fm_op = 0;` and `self.view.fm_browser = Default::default();`.

`fm_voices` items are used only by tests until Task 10 builds the panel. If clippy reports dead code, put `#![allow(dead_code)] // used by the FM panel (next task)` at the top of `fm_voices.rs`; Task 10 removes it.

- [ ] **Step 5: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(app): undoable FM voice loading and DX7 .syx import

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: The FM panel

**Files:**
- Create: `crates/app/src/ui/alg_diagram.rs`, `crates/app/src/ui/fm_panel.rs`
- Modify: `crates/app/src/ui/note_panel.rs` (extract the shared pattern row), `crates/app/src/ui/mod.rs` (modules, rack, tests), `crates/app/src/ui/fm_voices.rs` (remove any dead-code allow), `README.md`

**Interfaces:**
- Consumes: `ALGORITHMS` (Task 2); `fm_steps`, `step_index` (Task 1); `fm_voices::{starter, FmBrowser}`, `load_fm_voice`, `import_syx`, `ViewState.fm_op`/`fm_browser` (Task 9); `param_knob`, `pattern_pads`, `poly_roll` (existing).
- Produces:

```rust
// alg_diagram.rs (pure parts, unit-tested)
pub fn layout(alg: &Algorithm) -> [(f32, u8); 6];          // (column, row), row 0 = carriers
pub fn env_points(rates: [f32; 4], levels: [f32; 4]) -> [(f32, f32); 6];   // x 0..1 time, y 0..1 level
// app
impl SidAcidApp {
    pub(crate) fn fm_panel(&mut self, ui: &mut egui::Ui);
    pub(crate) fn note_pattern_row(&mut self, ui: &mut egui::Ui, m: usize);   // pads, length, snap
}
```

**Panel layout,** top to bottom. Everything uses the existing knob and switch widgets and theme.
- **Header:** "FM   A01   E.PIANO 1".
- **Top row:** the algorithm diagram (click an operator box to edit it), then Alg, Fdbk, Sync, Trans, Mono, Glide, Vintage, Volume.
- **Operator tabs:** "OP1 ×1.00" … "OP6 100Hz" (ratio or fixed frequency).
- **Selected operator's knobs:** On, Level, Mode, Coarse, Fine, Detune, Vel, AMS, Rate Sc, Break, L.Depth, R.Depth, L.Curve, R.Curve.
- **Envelope:** a sketch next to Attack, Decay 1, Decay 2, Release (rates) and Lvl 1..Lvl 4.
- **"LFO · Pitch EG · Effects"** (collapsed by default): LFO and PEG knobs, then the filter, chorus and phaser knobs.
- **Voice browser:** Starter / Imported, "Import .syx…", and the voice names in rows of 8.
- **Pattern row** (shared with the sampler) **and the poly roll.**

- [ ] **Step 1: Write the failing tests**

`alg_diagram.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::fm::algorithms::ALGORITHMS;

    #[test]
    fn every_algorithm_lays_out_without_overlaps() {
        for (i, alg) in ALGORITHMS.iter().enumerate() {
            let pos = layout(alg);
            for a in 0..6 {
                assert!(pos[a].0 >= 0.0 && pos[a].0 < 6.0, "alg {} op {}", i + 1, a + 1);
                assert_eq!(pos[a].1 == 0, alg.carriers & (1 << a) != 0, "alg {} op {}", i + 1, a + 1);
                for b in 0..a {
                    assert_ne!(pos[a], pos[b], "alg {} ops {} and {}", i + 1, a + 1, b + 1);
                }
                for m in 0..6 {
                    if alg.mods[a] & (1 << m) != 0 {
                        assert!(pos[m].1 > pos[a].1, "alg {}: OP{} sits above OP{}", i + 1, m + 1, a + 1);
                    }
                }
            }
        }
    }

    #[test]
    fn algorithm_32_is_one_row_of_six() {
        let pos = layout(&ALGORITHMS[31]);
        assert!(pos.iter().all(|p| p.1 == 0));
        let mut cols: Vec<f32> = pos.iter().map(|p| p.0).collect();
        cols.sort_by(f32::total_cmp);
        assert_eq!(cols, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn envelope_sketch() {
        let pts = env_points([99.0, 50.0, 50.0, 99.0], [99.0, 80.0, 60.0, 0.0]);
        assert_eq!(pts[0], (0.0, 0.0));
        assert!((pts[5].0 - 1.0).abs() < 1e-6);
        assert!(pts.windows(2).all(|w| w[1].0 > w[0].0));
        assert_eq!(pts[1].1, 1.0);
        assert!((pts[4].1 - 60.0 / 99.0).abs() < 1e-6, "sustain stretch holds L3");
        let slow = env_points([10.0, 50.0, 50.0, 99.0], [99.0, 80.0, 60.0, 0.0]);
        assert!(slow[1].0 > pts[1].0, "a slower attack takes more of the width");
    }
}
```

In `ui/mod.rs` tests:

```rust
    #[test]
    fn fm_panel_draws_headless_with_a_bank_and_each_operator() {
        let mut doc = Document::new(Project::default());
        doc.project.fm.bank = Some(fm_voices::starter().to_vec());
        let mut app = SidAcidApp::headless(doc);
        app.view.fm_browser = fm_voices::FmBrowser::Imported;
        for op in 0..6 {
            app.view.fm_op = op;
            run_frames(&mut app, 1);
        }
        assert!(app.view.poly_scrolled[0], "the FM roll is still drawn");
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p sidacid`
Expected: compile errors.

- [ ] **Step 3: Implement `alg_diagram.rs`**

```rust
//! The FM algorithm diagram (operator boxes and modulation lines,
//! carriers on the bottom row, like the FM-1's screen) and the operator
//! envelope sketch. Layout math is pure; drawing is in `fm_panel`.

use acid_engine::fm::algorithms::Algorithm;

/// Grid position of each operator: (column, row), row 0 = carriers.
/// Each modulator sits above the lowest-numbered operator it modulates;
/// siblings sit side by side, centred over their target.
pub fn layout(alg: &Algorithm) -> [(f32, u8); 6] {
    let mut parent = [None::<usize>; 6];
    for target in 0..6 {
        for m in 0..6 {
            if alg.mods[target] & (1 << m) != 0 && parent[m].is_none() {
                parent[m] = Some(target);
            }
        }
    }
    let mut width = [1u8; 6];
    for op in (0..6).rev() {
        let w: u8 = (0..6).filter(|&c| parent[c] == Some(op)).map(|c| width[c]).sum();
        width[op] = w.max(1);
    }
    let mut pos = [(0.0, 0u8); 6];
    let mut x = 0;
    for op in 0..6 {
        if parent[op].is_none() {
            place(op, x, 0, &parent, &width, &mut pos);
            x += width[op];
        }
    }
    pos
}

fn place(op: usize, x0: u8, row: u8, parent: &[Option<usize>; 6], width: &[u8; 6], pos: &mut [(f32, u8); 6]) {
    pos[op] = (x0 as f32 + (width[op] as f32 - 1.0) / 2.0, row);
    let mut x = x0;
    for child in 0..6 {
        if parent[child] == Some(op) {
            place(child, x, row + 1, parent, width, pos);
            x += width[child];
        }
    }
}

/// Corner points of an envelope sketch, x = time 0..1, y = level 0..1:
/// start (silence), the end of attack, decay 1 and decay 2, the end of a
/// fixed sustain stretch, and the end of the release.
pub fn env_points(rates: [f32; 4], levels: [f32; 4]) -> [(f32, f32); 6] {
    let seg = |r: f32| 0.05 + (99.0 - r.clamp(0.0, 99.0)) / 99.0;
    let times = [seg(rates[0]), seg(rates[1]), seg(rates[2]), 0.6, seg(rates[3])];
    let total: f32 = times.iter().sum();
    let y = |l: f32| l.clamp(0.0, 99.0) / 99.0;
    let ys = [y(levels[0]), y(levels[1]), y(levels[2]), y(levels[2]), y(levels[3])];
    let mut pts = [(0.0, 0.0); 6];
    let mut x = 0.0;
    for i in 0..5 {
        x += times[i] / total;
        pts[i + 1] = (x, ys[i]);
    }
    pts[5].0 = 1.0;
    pts
}
```

Children of a carrier are never carriers in a DX7 algorithm (Task 2 tests that), so only carriers get row 0.

- [ ] **Step 4: Extract the shared pattern row** in `note_panel.rs`

Move the `ScrollArea::horizontal()` block (pattern pads, separator, Len `DragValue` with its `Edit::NoteLength` handling, separator, Snap labels) out of `note_panel` into this method, unchanged inside:

```rust
    /// Pattern pads, length and snap for a note machine (FM or Sampler).
    pub(crate) fn note_pattern_row(&mut self, ui: &mut egui::Ui, m: usize) {
        let n = m - MachineId::Fm.index();
        let slot = self.doc.project.selected[m];
        egui::ScrollArea::horizontal()
            .id_salt(("note_pads", m))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // (the existing pads / Len / Snap code, unchanged)
                });
            });
    }
```

`note_panel` then calls `self.note_pattern_row(ui, m);` followed by `self.poly_roll(ui, m);`. Update `note_panel`'s module doc: it now draws only the Sampler panel, a placeholder until Plan 6. In `rack`, replace the loop over `[Fm, Sampler]` with:

```rust
        self.fm_panel(ui);
        ui.add_space(6.0);
        self.note_panel(ui, acid_engine::machine::MachineId::Sampler);
        ui.add_space(6.0);
```

- [ ] **Step 5: Implement `fm_panel.rs`**

```rust
//! The FM panel: algorithm diagram, global knobs, operator editor with
//! envelope sketch, LFO / pitch EG / effects, the voice browser, and the
//! pattern row with the poly piano roll.

use super::alg_diagram::{env_points, layout};
use super::fm_voices::{FmBrowser, starter};
use super::{SidAcidApp, theme};
use acid_engine::fm::algorithms::ALGORITHMS;
use acid_engine::machine::MachineId;
use acid_engine::params::{FmParam, OpParam, ParamId, fm_steps, step_index};
use acid_engine::project::PatternRef;
use eframe::egui::{self, Align2, FontId, Sense, Stroke, pos2, vec2};

const TOP: [(FmParam, &str); 8] = [
    (FmParam::Algorithm, "Alg"),
    (FmParam::Feedback, "Fdbk"),
    (FmParam::OscSync, "Sync"),
    (FmParam::Transpose, "Trans"),
    (FmParam::Mono, "Mono"),
    (FmParam::Glide, "Glide"),
    (FmParam::Vintage, "Vintage"),
    (FmParam::Volume, "Volume"),
];
const LFO: [(FmParam, &str); 7] = [
    (FmParam::LfoWave, "Wave"),
    (FmParam::LfoSpeed, "Speed"),
    (FmParam::LfoDelay, "Delay"),
    (FmParam::LfoPmd, "P.Depth"),
    (FmParam::LfoAmd, "A.Depth"),
    (FmParam::LfoSync, "Key Sync"),
    (FmParam::PitchModSens, "PMS"),
];
const PITCH_EG: [(FmParam, &str); 8] = [
    (FmParam::PitchR1, "P.R1"),
    (FmParam::PitchR2, "P.R2"),
    (FmParam::PitchR3, "P.R3"),
    (FmParam::PitchR4, "P.R4"),
    (FmParam::PitchL1, "P.L1"),
    (FmParam::PitchL2, "P.L2"),
    (FmParam::PitchL3, "P.L3"),
    (FmParam::PitchL4, "P.L4"),
];
const FX: [(FmParam, &str); 11] = [
    (FmParam::FilterOn, "Filter"),
    (FmParam::FilterMode, "Mode"),
    (FmParam::Cutoff, "Cutoff"),
    (FmParam::Resonance, "Reso"),
    (FmParam::ChorusRate, "Ch.Rate"),
    (FmParam::ChorusDepth, "Ch.Depth"),
    (FmParam::ChorusMix, "Chorus"),
    (FmParam::PhaserRate, "Ph.Rate"),
    (FmParam::PhaserDepth, "Ph.Depth"),
    (FmParam::PhaserFeedback, "Ph.Fdbk"),
    (FmParam::PhaserMix, "Phaser"),
];
const OP_KNOBS: [(OpParam, &str); 14] = [
    (OpParam::On, "On"),
    (OpParam::Level, "Level"),
    (OpParam::FixedFreq, "Mode"),
    (OpParam::Coarse, "Coarse"),
    (OpParam::Fine, "Fine"),
    (OpParam::Detune, "Detune"),
    (OpParam::VelSens, "Vel"),
    (OpParam::AmpModSens, "AMS"),
    (OpParam::RateScaling, "Rate Sc"),
    (OpParam::BreakPoint, "Break"),
    (OpParam::LeftDepth, "L.Depth"),
    (OpParam::RightDepth, "R.Depth"),
    (OpParam::LeftCurve, "L.Curve"),
    (OpParam::RightCurve, "R.Curve"),
];
const OP_ENV: [(OpParam, &str); 8] = [
    (OpParam::R1, "Attack"),
    (OpParam::R2, "Decay 1"),
    (OpParam::R3, "Decay 2"),
    (OpParam::R4, "Release"),
    (OpParam::L1, "Lvl 1"),
    (OpParam::L2, "Lvl 2"),
    (OpParam::L3, "Lvl 3"),
    (OpParam::L4, "Lvl 4"),
];
const CELL: f32 = 28.0;
const BOX: egui::Vec2 = vec2(22.0, 16.0);

impl SidAcidApp {
    pub(crate) fn fm_panel(&mut self, ui: &mut egui::Ui) {
        let m = MachineId::Fm.index();
        let slot = self.doc.project.selected[m];
        let title = format!(
            "FM   {}   {}",
            PatternRef(slot).label(),
            self.doc.project.fm.voice_name
        );
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt("fm")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    self.alg_diagram(ui);
                    ui.horizontal_wrapped(|ui| {
                        for (p, label) in TOP {
                            self.param_knob(ui, ParamId::Fm(p), label);
                        }
                    });
                });
                self.fm_operator_editor(ui);
                egui::CollapsingHeader::new("LFO · Pitch EG · Effects")
                    .id_salt("fm_more")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (p, label) in LFO.iter().chain(PITCH_EG.iter()) {
                                self.param_knob(ui, ParamId::Fm(*p), label);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            for (p, label) in FX {
                                self.param_knob(ui, ParamId::Fm(p), label);
                            }
                        });
                    });
                self.fm_voice_browser(ui);
                self.note_pattern_row(ui, m);
                self.poly_roll(ui, m);
            });
    }

    fn fm_value(&self, id: ParamId) -> f32 {
        self.doc.project.param(id)
    }

    /// "×1.00" for a ratio operator, "100Hz" for a fixed one.
    fn op_freq_text(&self, op: usize) -> String {
        let id = |p| ParamId::FmOp(op as u8, p);
        let coarse = step_index(self.fm_value(id(OpParam::Coarse)), fm_steps::COARSE as u8) as f32;
        let fine = self.fm_value(id(OpParam::Fine)) * 99.0;
        if self.fm_value(id(OpParam::FixedFreq)) >= 0.5 {
            let hz = 10f32.powf(((coarse as usize & 3) as f32 * 100.0 + fine) / 100.0);
            format!("{hz:.0}Hz")
        } else {
            let ratio = if coarse == 0.0 { 0.5 } else { coarse };
            format!("×{:.2}", ratio * (1.0 + fine / 100.0))
        }
    }

    fn alg_diagram(&mut self, ui: &mut egui::Ui) {
        let index = step_index(
            self.fm_value(ParamId::Fm(FmParam::Algorithm)),
            fm_steps::ALGORITHM as u8,
        );
        let alg = ALGORITHMS[index];
        let pos = layout(&alg);
        let rows = pos.iter().map(|p| p.1).max().unwrap_or(0) as f32 + 1.0;
        let (rect, resp) = ui.allocate_exact_size(vec2(6.0 * CELL + 8.0, rows * CELL + 18.0), Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, theme::GRID_DARK);
        painter.text(
            rect.left_top() + vec2(4.0, 2.0),
            Align2::LEFT_TOP,
            format!("ALG {}", index + 1),
            FontId::monospace(10.0),
            theme::ACID,
        );
        let centre = |op: usize| {
            pos2(
                rect.left() + 4.0 + (pos[op].0 + 0.5) * CELL,
                rect.bottom() - 4.0 - (pos[op].1 as f32 + 0.5) * CELL,
            )
        };
        for target in 0..6 {
            for m in 0..6 {
                if alg.mods[target] & (1 << m) != 0 {
                    painter.line_segment(
                        [centre(m) + vec2(0.0, BOX.y / 2.0), centre(target) - vec2(0.0, BOX.y / 2.0)],
                        Stroke::new(1.5, theme::KNOB_TRACK),
                    );
                }
            }
        }
        let fb = centre(alg.feedback);
        let loop_rect = egui::Rect::from_min_max(
            fb + vec2(BOX.x / 2.0, -BOX.y / 2.0 - 3.0),
            fb + vec2(BOX.x / 2.0 + 5.0, BOX.y / 2.0 - 3.0),
        );
        painter.rect_stroke(loop_rect, 1.0, Stroke::new(1.0, theme::SLIDE), egui::StrokeKind::Middle);
        let mut clicked = None;
        for op in 0..6 {
            let r = egui::Rect::from_center_size(centre(op), BOX);
            let carrier = alg.carriers & (1 << op) != 0;
            painter.rect_filled(r, 2.0, if carrier { theme::ACID } else { theme::KNOB_BODY });
            if op == self.view.fm_op {
                painter.rect_stroke(r, 2.0, Stroke::new(2.0, theme::ACCENT), egui::StrokeKind::Outside);
            }
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                format!("{}", op + 1),
                FontId::monospace(10.0),
                if carrier { egui::Color32::BLACK } else { egui::Color32::WHITE },
            );
            if resp.clicked()
                && let Some(p) = resp.interact_pointer_pos()
                && r.contains(p)
            {
                clicked = Some(op);
            }
        }
        if let Some(op) = clicked {
            self.view.fm_op = op;
        }
    }

    fn fm_operator_editor(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for op in 0..6 {
                let text = format!("OP{} {}", op + 1, self.op_freq_text(op));
                if ui.selectable_label(self.view.fm_op == op, text).clicked() {
                    self.view.fm_op = op;
                }
            }
        });
        let op = self.view.fm_op.min(5) as u8;
        ui.horizontal_wrapped(|ui| {
            for (p, label) in OP_KNOBS {
                self.param_knob(ui, ParamId::FmOp(op, p), label);
            }
        });
        ui.horizontal_wrapped(|ui| {
            let v = |p| self.fm_value(ParamId::FmOp(op, p)) * 99.0;
            let rates = [v(OpParam::R1), v(OpParam::R2), v(OpParam::R3), v(OpParam::R4)];
            let levels = [v(OpParam::L1), v(OpParam::L2), v(OpParam::L3), v(OpParam::L4)];
            env_sketch(ui, rates, levels);
            for (p, label) in OP_ENV {
                self.param_knob(ui, ParamId::FmOp(op, p), label);
            }
        });
    }

    fn fm_voice_browser(&mut self, ui: &mut egui::Ui) {
        let has_bank = self.doc.project.fm.bank.is_some();
        ui.horizontal(|ui| {
            ui.label("Voices");
            if ui
                .selectable_label(self.view.fm_browser == FmBrowser::Starter, "Starter")
                .clicked()
            {
                self.view.fm_browser = FmBrowser::Starter;
            }
            if ui
                .add_enabled(
                    has_bank,
                    egui::Button::selectable(self.view.fm_browser == FmBrowser::Imported, "Imported"),
                )
                .clicked()
            {
                self.view.fm_browser = FmBrowser::Imported;
            }
            if ui
                .button("Import .syx…")
                .on_hover_text("Load a DX7 32-voice bank or single voice")
                .clicked()
            {
                self.import_syx();
            }
        });
        let names: Vec<String> = match (self.view.fm_browser, &self.doc.project.fm.bank) {
            (FmBrowser::Imported, Some(bank)) => bank.iter().map(|v| v.name.clone()).collect(),
            _ => starter().iter().map(|v| v.name.clone()).collect(),
        };
        let mut picked = None;
        egui::Grid::new("fm_voices").spacing([4.0, 2.0]).show(ui, |ui| {
            for (i, name) in names.iter().enumerate() {
                let current = *name == self.doc.project.fm.voice_name;
                if ui.selectable_label(current, name.as_str()).clicked() {
                    picked = Some(i);
                }
                if i % 8 == 7 {
                    ui.end_row();
                }
            }
        });
        if let Some(i) = picked {
            let voice = match (self.view.fm_browser, &self.doc.project.fm.bank) {
                (FmBrowser::Imported, Some(bank)) => bank[i].clone(),
                _ => starter()[i].clone(),
            };
            self.load_fm_voice(&voice);
        }
    }
}

/// A small display-only drawing of an operator envelope.
fn env_sketch(ui: &mut egui::Ui, rates: [f32; 4], levels: [f32; 4]) {
    let (rect, _) = ui.allocate_exact_size(vec2(96.0, 44.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::GRID_DARK);
    let inner = rect.shrink(3.0);
    let points: Vec<egui::Pos2> = env_points(rates, levels)
        .iter()
        .map(|(x, y)| pos2(inner.left() + x * inner.width(), inner.bottom() - y * inner.height()))
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.5, theme::ACID)));
}
```

Notes:
- If `egui::Button::selectable` does not exist in the egui version in `Cargo.lock`, use `egui::Button::new("Imported").selected(self.view.fm_browser == FmBrowser::Imported)`.
- The browser builds a short `Vec<String>` of names per frame. That is fine on the UI thread.

In `ui/mod.rs`, add `mod alg_diagram;` and `mod fm_panel;` (alphabetical). Remove the `#![allow(dead_code)]` from `fm_voices.rs` if Task 9 added one.

- [ ] **Step 6: Update the README**

In `README.md`, replace the "FM synth and Sampler slots" bullet with:

```markdown
- **FM synth** in the style of the DX7 / M-VAVE FM-1:
  - 6 operators, 32 algorithms with a live algorithm diagram, DX7 envelopes, keyboard and velocity scaling.
  - LFO and pitch EG.
  - 12-voice poly, or mono with glide.
  - Filter, chorus and phaser, plus a Vintage switch for 12-bit grit.
  - A 16-voice starter bank, and **DX7 .syx import** (32-voice banks or single voices).
- **Sampler slot** (placeholder sine voice for now, the sampler is coming next).
- Both use a **polyphonic piano roll**: chords, 1/16 to 1/64 snap, a velocity lane, and click-to-audition keys.
```

- [ ] **Step 7: Run everything, then check by hand**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

Then run `cargo run --release -p sidacid` and check:
- the FM panel shows the diagram, and changing Alg redraws it
- clicking an operator box selects that operator's tab
- loading E.PIANO 1 from the browser changes the sound and the header name, and Ctrl+Z restores the INIT voice in one step
- Import .syx… loads a DX7 bank file if you have one
- a chord in the FM roll plays as a chord

If no display or audio is available, say so in the task report.

- [ ] **Step 8: Commit**

```bash
git add -A crates README.md
git commit -m "feat(app): FM panel with algorithm diagram, operator editor and voice browser

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Spec coverage (Plan 5 scope)

| Spec item (section 2 / section 4 FM panel) | Task |
|---|---|
| 6 sine operators, 32 algorithms, feedback | 2, 4 |
| Per-op ratio/fixed, coarse/fine, detune, level, velocity, keyboard level/rate scaling, 4-rate/4-level EG | 1, 2, 3, 4 |
| Global: algorithm, feedback, pitch EG, LFO (6 waves, delay, PMD/AMD, PMS, AMS), transpose, volume, poly/mono, glide | 1, 3, 4 |
| 12-voice poly, oldest-note steal with ~2 ms fade, mono legato + glide | 4 |
| Vintage switch (coarse sine, 12-bit output) | 3, 4 |
| Filter (LP/HP via sid-core Svf) → chorus → phaser | 5 |
| SysEx import (bank 4104 / single 163, checksum, refusal) | 7, 9 |
| Voice browser (starter / imported), load = one undoable edit, bank saved in project | 8, 9, 10 |
| Starter bank (~16 original voices) | 8 |
| Algorithm diagram, operator tabs with on/off, env graph + knobs, browser, import button | 10 |
| Tests: algorithm graphs, EG timing, SysEx round trip + refusal, 12-voice cap / oldest steal, glide, allocation-free | 2, 4, 6, 7 |
| Audition voices separate from pattern voices (Plan 4 deferred) | 4, 6 |
