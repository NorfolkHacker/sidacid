# SidAcid Plan 6 — Sampler Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Sampler slot's placeholder sine voice with a 16-slot sampler for chopped breaks and chromatically played samples. Audio is embedded in the project, plus a sampler panel with a waveform editor and labelled piano-roll rows.

**Architecture:** A new `acid_engine::sampler` module holds:
- slot data (meta + 16-bit PCM, saved deflated and base64-encoded in the `.sidacid` file)
- an immutable `SampleData` (f32) handed to the audio thread as `Arc` through the command queue and disposed on the UI thread
- a 16-voice `Sampler` with slice and chromatic playback, choke, tempo lock, ADSR, filter and Grit
- symphonia decoding and transient slicing (pure, off the audio thread)

Every per-slot control is an automatable `ParamId::SamplerSlot(slot, SlotParam)`. Loading or clearing a slot is one undoable edit.

**Tech Stack:** Rust 2024 (1.88+), egui/eframe 0.31, serde + RON, rfd. **New dependencies:** `symphonia 0.5` (wav, pcm, flac, mp3, ogg, vorbis), `flate2 1`, `base64 0.22`. All three are verified to resolve and build in this environment.

**Spec:** `docs/superpowers/specs/2026-10-12-fm-sampler-design.md`. Plan 6 covers section 3 ("Sampler") and the sampler parts of section 4: the sampler panel, and the roll's sampler row labels and tint. Read both before starting.

## Global Constraints

- `cargo test --workspace` passes at the end of every task. The baseline is 386. Never delete or weaken an existing assertion unless the step explicitly says so and gives the replacement.
- `cargo clippy --workspace --all-targets` reports 0 warnings, and `cargo fmt --all` is run before each commit.
- **Real-time rule:** `Engine::process`, everything it calls, and `EngineHost::process` must not allocate, lock or do I/O.
  - Sample audio reaches the engine as `Arc<SampleData>` through the command queue.
  - A replaced `Arc` goes back to the UI thread as `Event::DisposeSample`, so the audio thread never drops the last reference.
  - `crates/acid-engine/tests/no_alloc.rs` must keep passing.
- Machine order is fixed: 0 Synth 1, 1 Synth 2, 2 808, 3 909, 4 FM, 5 Sampler. The sampler renders into mixer channel 5. The sampler is never transposed by the song.
- **Notes:**
  - A pattern note-off releases only pattern voices; an audition note-off releases only audition voices.
  - A note-off releases every voice of that key from its source.
- **Limits:** 30 s per slot and 64 MB of decoded audio (f32) across the project; anything over is refused with a message.
- **Zones:** 16 slots, each owning a non-overlapping key zone. An edit that would overlap is refused with a message.
- **Project file:** `PROJECT_VERSION` stays `2`, and new project fields use `#[serde(default)]`. Embedded PCM round-trips bit-exactly at 16 bits.
- Every commit message ends with the trailer line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Deliberate deviations from the spec (keep them)

1. **No separate resampling step.** Samples stay at their source rate. A voice's playback rate already includes `source_rate / engine_rate`, and cubic Hermite interpolation does the conversion while playing.
2. **Files are decoded on the UI thread** when loaded. A 30 s file decodes in well under a second, and there is no worker thread.
3. **The waveform view has no playback cursor.** It shows the markers and the zone readout only.
4. **The default mode depends on length.** A loaded sample of 1 s or longer becomes a slice slot: transient detection at sensitivity 0.5, falling back to 8 equal slices if fewer than 2 are found, with Choke and Tempo Lock on. Anything shorter becomes a chromatic slot covering 24 keys, with its root on the lowest key.
5. **Grit has 4× headroom.** It quantises to 12 bits over a ±4 range before the volume, matching the FM Vintage fix, so chords don't clip.
6. **Per-slot params are `ParamId::SamplerSlot(slot, SlotParam)`**, a new variant.
7. **Pad audition is shift-click**, held while the mouse button is down.

## File structure

| File | Status | Responsibility |
|---|---|---|
| `crates/acid-engine/src/params.rs` | modify | `SamplerParam::Grit`, `SlotParam`, `ParamId::SamplerSlot`, `SAMPLER_SLOTS`, `sampler_map` |
| `crates/acid-engine/src/sampler/mod.rs` | new | constants, module list |
| `crates/acid-engine/src/sampler/sample.rs` | new | `SampleData` + cubic read |
| `crates/acid-engine/src/sampler/slot.rs` | new | `SlotMode`, `SlotMeta`, `SlotData`, PCM codec, `SlotPlay` |
| `crates/acid-engine/src/sampler/settings.rs` | new | `SlotSettings` (decoded per-slot knobs) |
| `crates/acid-engine/src/sampler/voice.rs` | new | `Segment`, `segment()`, one voice |
| `crates/acid-engine/src/sampler/machine.rs` | new | `Sampler`: 16 voices, zones, choke, Grit |
| `crates/acid-engine/src/sampler/decode.rs` | new | symphonia decode |
| `crates/acid-engine/src/sampler/slicing.rs` | new | equal / transient slicing, BPM guess |
| `crates/acid-engine/src/project.rs` | modify | `SamplerMachine.slots` + helpers + validation |
| `crates/acid-engine/src/engine.rs`, `host.rs` | modify | sampler slot, sample/meta commands, disposal |
| `crates/acid-engine/src/placeholder.rs` | delete | no longer used |
| `crates/acid-engine/tests/no_alloc.rs` | modify | the busy project plays real samples |
| `crates/app/src/document.rs` | modify | `Edit::SamplerSlot`, `Edit::SlotMeta` |
| `crates/app/src/ui/sampler_load.rs` | new | load / clear / meta edits with zone and memory checks |
| `crates/app/src/ui/sampler_ui.rs` | new | pure helpers: peaks, marker math, row labels |
| `crates/app/src/ui/sampler_panel.rs` | new | the sampler panel |
| `crates/app/src/ui/polyroll.rs`, `note_panel.rs`, `params_ui.rs`, `mod.rs`, `files.rs` | modify | roll labels, rack, view state |
| `Cargo.toml`, `crates/acid-engine/Cargo.toml`, `README.md` | modify | dependencies, features |

---

### Task 1: Sampler parameters

**Files:**
- Modify: `crates/acid-engine/src/params.rs`, `crates/app/src/ui/params_ui.rs`

**Interfaces:**
- Produces:

```rust
pub const SAMPLER_SLOTS: usize = 16;
pub const PARAM_COUNT: usize = 589;
pub enum SamplerParam { Volume, Grit }
pub enum SlotParam { Level, Pan, Tune, Fine, Cutoff, Resonance, Attack, Decay, Sustain, Release, Reverse, Choke, TempoLock }
ParamId::SamplerSlot(u8 /* 0..16 */, SlotParam)          // last variant, last in all()
pub mod sampler_map {
    pub fn level_gain(v: f32) -> f32;       // v² · 1.25
    pub fn tune_semitones(v: f32) -> i32;   // -24..=24 (49 steps)
    pub fn fine_cents(v: f32) -> f32;       // -50..=50, whole cents
    pub fn cutoff_hz(v: f32) -> f32;        // exp(v, 40, 18000)
    pub fn attack_seconds(v: f32) -> f32;   // exp(v, 0.001, 2.0)
    pub fn decay_seconds(v: f32) -> f32;    // exp(v, 0.005, 4.0)
    pub fn release_seconds(v: f32) -> f32;  // exp(v, 0.005, 4.0)
}
```

- Slot defaults:

| Param | Default |
|---|---|
| Level | 0.8 |
| Pan | 0.5 |
| Tune | 0.5 |
| Fine | 0.5 |
| Cutoff | 1.0 |
| Resonance | 0 |
| Attack | 0 |
| Decay | 0.5 |
| Sustain | 1.0 |
| Release | 0.3 |
| Reverse | 0 |
| Choke | 0 |
| TempoLock | 0 |

  Reverse, Choke and TempoLock are 2-step switches; the rest are continuous.
- `Grit` is a 2-step switch, default 0.

- [ ] **Step 1: Write the failing tests** in `params.rs` tests

Replace the body of `registry_has_every_param_once` with:

```rust
        let all = ParamId::all();
        assert_eq!(
            all.len(),
            2 * 15 + 2 * 8 * 4 + 2 * 2 + 6 * 16 + 11 + 8 + 34 + 2 + 6 * 22 + 16 * 13
        );
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
```

In `fm_params_follow_the_master_params`, replace the line `assert_eq!(all[all.len() - 1], ParamId::FmOp(5, OpParam::On));` with:

```rust
        assert!(all.contains(&ParamId::FmOp(5, OpParam::On)));
        assert_eq!(all[all.len() - 1], ParamId::SamplerSlot(15, SlotParam::TempoLock));
```

Add:

```rust
    #[test]
    fn sampler_params_and_maps() {
        assert!(ParamId::SamplerSlot(15, SlotParam::Level).is_valid());
        assert!(!ParamId::SamplerSlot(16, SlotParam::Level).is_valid());
        assert_eq!(ParamId::SamplerSlot(16, SlotParam::Level).index(), None);
        assert_eq!(ParamId::Sampler(SamplerParam::Grit).info().steps, 2);
        assert_eq!(ParamId::SamplerSlot(3, SlotParam::Choke).info().steps, 2);
        assert_eq!(ParamId::SamplerSlot(3, SlotParam::Release).info().default, 0.3);
        assert_eq!(sampler_map::tune_semitones(0.5), 0);
        assert_eq!(sampler_map::tune_semitones(1.0), 24);
        assert_eq!(sampler_map::tune_semitones(0.0), -24);
        assert_eq!(sampler_map::fine_cents(0.5), 0.0);
        assert_eq!(sampler_map::fine_cents(1.0), 50.0);
        assert!((sampler_map::level_gain(0.8) - 0.8).abs() < 1e-6);
        assert!((sampler_map::attack_seconds(0.0) - 0.001).abs() < 1e-6);
        assert!((sampler_map::release_seconds(1.0) - 4.0).abs() < 1e-3);
        assert!((sampler_map::cutoff_hz(1.0) - 18_000.0).abs() < 1.0);
    }
```

In `params_ui.rs` tests, add:

```rust
    #[test]
    fn sampler_labels_and_values() {
        use acid_engine::params::{SamplerParam, SlotParam};
        assert_eq!(param_label(ParamId::SamplerSlot(2, SlotParam::Tune)), "Sampler Slot 3 Tune");
        assert_eq!(param_label(ParamId::Sampler(SamplerParam::Grit)), "Sampler Grit");
        assert_eq!(discrete_names(ParamId::SamplerSlot(0, SlotParam::Reverse)), &["Off", "On"]);
        assert_eq!(value_text(ParamId::SamplerSlot(0, SlotParam::Tune), 1.0), "+24");
        assert_eq!(value_text(ParamId::SamplerSlot(0, SlotParam::Fine), 0.0), "-50 c");
        assert_eq!(value_text(ParamId::SamplerSlot(0, SlotParam::Attack), 0.0), "1 ms");
        assert_eq!(value_text(ParamId::SamplerSlot(0, SlotParam::Release), 1.0), "4.00 s");
        assert_eq!(value_text(ParamId::SamplerSlot(0, SlotParam::Cutoff), 1.0), "18000 Hz");
        assert_eq!(value_text(ParamId::SamplerSlot(0, SlotParam::Level), 0.5), "50%");
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine params:: ; cargo test -p sidacid params_ui`
Expected: compile errors.

- [ ] **Step 3: Implement** in `params.rs`

```rust
/// Sample slots in the sampler.
pub const SAMPLER_SLOTS: usize = 16;
```

Set `PARAM_COUNT` to `589`. Replace `param_enum!(SamplerParam { Volume => "Volume" });` with:

```rust
param_enum!(SamplerParam { Volume => "Volume", Grit => "Grit" });
param_enum!(SlotParam {
    Level => "Level", Pan => "Pan", Tune => "Tune", Fine => "Fine", Cutoff => "Cutoff",
    Resonance => "Resonance", Attack => "Attack", Decay => "Decay", Sustain => "Sustain",
    Release => "Release", Reverse => "Reverse", Choke => "Choke", TempoLock => "Tempo Lock",
});
```

Add `SamplerSlot(u8, SlotParam),` as the last `ParamId` variant. In `info()`, replace the `ParamId::Sampler(p)` arm and add the slot arm:

```rust
            ParamId::Sampler(p) => match p {
                SamplerParam::Volume => cont(p.name(), 0.7),
                SamplerParam::Grit => disc(p.name(), 0.0, 2),
            },
            ParamId::SamplerSlot(_, p) => {
                use SlotParam as S;
                let n = p.name();
                match p {
                    S::Level => cont(n, 0.8),
                    S::Pan | S::Tune | S::Fine | S::Decay => cont(n, 0.5),
                    S::Cutoff | S::Sustain => cont(n, 1.0),
                    S::Resonance | S::Attack => cont(n, 0.0),
                    S::Release => cont(n, 0.3),
                    S::Reverse | S::Choke | S::TempoLock => disc(n, 0.0, 2),
                }
            }
```

In `is_valid()`, add `ParamId::SamplerSlot(s, _) => (s as usize) < SAMPLER_SLOTS,`. In `index()`, after `fm_op_base`:

```rust
        let slot_base = fm_op_base + FM_OPS * OpParam::ALL.len();
```

Then add the arm:

```rust
            ParamId::SamplerSlot(s, p) => slot_base + s as usize * SlotParam::ALL.len() + p as usize,
```

In `all()`, after the FM operator loop:

```rust
        for s in 0..SAMPLER_SLOTS as u8 {
            v.extend(SlotParam::ALL.iter().map(|&p| ParamId::SamplerSlot(s, p)));
        }
```

Add the shared mappings (the engine and the tooltips both use them):

```rust
/// Sampler slot knobs in real units. The engine and the UI share these.
pub mod sampler_map {
    use super::{exp, step_index};

    pub const TUNE_STEPS: usize = 49;

    pub fn level_gain(v: f32) -> f32 {
        v.clamp(0.0, 1.0).powi(2) * 1.25
    }

    pub fn tune_semitones(v: f32) -> i32 {
        step_index(v, TUNE_STEPS as u8) as i32 - 24
    }

    pub fn fine_cents(v: f32) -> f32 {
        (v.clamp(0.0, 1.0) * 100.0 - 50.0).round()
    }

    pub fn cutoff_hz(v: f32) -> f32 {
        exp(v, 40.0, 18_000.0)
    }

    pub fn attack_seconds(v: f32) -> f32 {
        exp(v, 0.001, 2.0)
    }

    pub fn decay_seconds(v: f32) -> f32 {
        exp(v, 0.005, 4.0)
    }

    pub fn release_seconds(v: f32) -> f32 {
        exp(v, 0.005, 4.0)
    }
}
```

The engine's `apply_param` routes `ParamId::Sampler(SamplerParam::Volume)` to the placeholder. Any other `Sampler`/`SamplerSlot` id falls through to the mixer's ignore arm until Task 5. If an exhaustive `match` elsewhere fails to compile, add a no-op arm with the comment "wired in the sampler engine task".

- [ ] **Step 4: UI text** in `params_ui.rs`

Import `SlotParam` and `sampler_map`. In `param_label`, add:

```rust
        ParamId::SamplerSlot(s, p) => format!("Sampler Slot {} {}", s + 1, p.name()),
```

In `value_text`, add these arms before the final `_` arm:

```rust
        ParamId::SamplerSlot(_, SlotParam::Tune) => signed(sampler_map::tune_semitones(v)),
        ParamId::SamplerSlot(_, SlotParam::Fine) => format!("{} c", signed(sampler_map::fine_cents(v) as i32)),
        ParamId::SamplerSlot(_, SlotParam::Attack) => seconds_text(sampler_map::attack_seconds(v)),
        ParamId::SamplerSlot(_, SlotParam::Decay) => seconds_text(sampler_map::decay_seconds(v)),
        ParamId::SamplerSlot(_, SlotParam::Release) => seconds_text(sampler_map::release_seconds(v)),
        ParamId::SamplerSlot(_, SlotParam::Cutoff) => format!("{:.0} Hz", sampler_map::cutoff_hz(v)),
```

Add the helper next to `value_text`:

```rust
/// "12 ms" below a second, "1.50 s" from a second up.
fn seconds_text(s: f32) -> String {
    if s < 1.0 {
        format!("{:.0} ms", s * 1000.0)
    } else {
        format!("{s:.2} s")
    }
}
```

(The `signed` closure already exists in `value_text`. If it is a closure local to the function, these arms sit inside the same `match` and can use it.)

- [ ] **Step 5: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(engine): sampler slot params and Grit

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Slot data, embedded PCM and sample storage

**Files:**
- Modify: `Cargo.toml` (workspace deps), `crates/acid-engine/Cargo.toml`
- Create: `crates/acid-engine/src/sampler/mod.rs`, `sampler/sample.rs`, `sampler/slot.rs`
- Modify: `crates/acid-engine/src/lib.rs` (`pub mod sampler;`), `crates/acid-engine/src/project.rs`

**Interfaces:**
- Produces:

```rust
// sampler/mod.rs
pub const SLOTS: usize = crate::params::SAMPLER_SLOTS;  // 16
pub const VOICES: usize = 16;
pub const MAX_SLICES: usize = 64;
pub const MAX_SECONDS: f32 = 30.0;
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;       // bytes of decoded f32
pub const FIRST_KEY: u8 = 24;                           // C1
// sampler/sample.rs
pub struct SampleData { pub sample_rate: u32, pub left: Box<[f32]>, pub right: Option<Box<[f32]>> }
impl SampleData { pub fn frames(&self) -> usize; pub fn read(&self, pos: f64) -> (f32, f32) }  // cubic Hermite
// sampler/slot.rs
pub enum SlotMode { Slice, Chromatic }   // Default = Slice
pub struct SlotMeta { pub mode: SlotMode, pub base_key: u8, pub high_key: u8, pub root: u8,
    pub slices: Vec<u32>, pub original_bpm: f32, pub start: u32, pub end: u32,
    pub loop_on: bool, pub loop_start: u32, pub loop_end: u32 }
impl SlotMeta {
    pub fn sliced(base_key: u8, slices: Vec<u32>, original_bpm: f32, frames: u32) -> Self;
    pub fn chromatic(base_key: u8, high_key: u8, root: u8, frames: u32) -> Self;
    pub fn key_range(&self) -> (u8, u8);
    pub fn is_well_formed(&self, frames: u32) -> bool;
}
pub struct SlotData { pub name: String, pub meta: SlotMeta, pub sample_rate: u32, pub channels: u8, pub pcm: Vec<i16> }
impl SlotData {
    pub fn new(name: &str, sample_rate: u32, channels: u8, samples: &[f32], meta: SlotMeta) -> Self;
    pub fn frames(&self) -> u32; pub fn memory_bytes(&self) -> usize; pub fn to_sample(&self) -> SampleData;
    pub fn is_valid(&self) -> bool;
}
pub mod pcm_codec { pub fn encode(pcm: &[i16]) -> String; pub fn decode(s: &str) -> Result<Vec<i16>, String>; /* + serde with-fns */ }
pub struct SlotPlay { pub mode, pub base_key, pub high_key, pub root, pub slice_count: usize,
    pub slices: [u32; MAX_SLICES], pub original_bpm, pub start, pub end, pub loop_on, pub loop_start, pub loop_end }
impl SlotPlay { pub const EMPTY: SlotPlay; pub fn from_meta(m: &SlotMeta) -> Self; pub fn key_range(&self) -> (u8, u8) }
// project.rs
pub struct SamplerMachine { pub patterns: Vec<NotePattern>, #[serde(default = "empty_slots")] pub slots: Vec<Option<SlotData>> }
impl SamplerMachine {
    pub fn slot_for_key(&self, key: u8) -> Option<usize>;
    pub fn zone_clash(&self, slot: usize, meta: &SlotMeta) -> bool;
    pub fn memory_bytes(&self) -> usize;
    pub fn free_base_key(&self, slot: usize, span: u8) -> Option<u8>;
}
```

- [ ] **Step 1: Add the dependencies**

In the root `Cargo.toml`, under `[workspace.dependencies]`:

```toml
symphonia = { version = "0.5", default-features = false, features = ["wav", "pcm", "flac", "mp3", "ogg", "vorbis"] }
flate2 = "1"
base64 = "0.22"
```

In `crates/acid-engine/Cargo.toml` `[dependencies]`, add `symphonia.workspace = true`, `flate2.workspace = true` and `base64.workspace = true`. (`symphonia` is first used in Task 6. Adding it now keeps dependency changes in one commit.)

- [ ] **Step 2: Write the failing tests**

`sampler/sample.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> SampleData {
        SampleData {
            sample_rate: 44_100,
            left: (0..100).map(|i| i as f32 / 100.0).collect(),
            right: None,
        }
    }

    #[test]
    fn cubic_read_is_exact_on_frames_and_on_a_ramp() {
        let s = ramp();
        assert_eq!(s.frames(), 100);
        assert_eq!(s.read(10.0), (0.1, 0.1), "mono reads the same on both sides");
        let (l, _) = s.read(10.5);
        assert!((l - 0.105).abs() < 1e-6);
        assert_eq!(s.read(-5.0).0, 0.0, "clamped at the start");
        assert!((s.read(500.0).0 - 0.99).abs() < 1e-6, "clamped at the end");
    }

    #[test]
    fn stereo_reads_both_sides() {
        let s = SampleData {
            sample_rate: 48_000,
            left: vec![1.0; 8].into(),
            right: Some(vec![-1.0; 8].into()),
        };
        assert_eq!(s.read(3.3), (1.0, -1.0));
    }
}
```

`sampler/slot.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, channels: usize) -> Vec<f32> {
        (0..frames * channels).map(|i| ((i as f32) * 0.01).sin() * 0.7).collect()
    }

    #[test]
    fn pcm_codec_round_trips_bit_exactly() {
        let pcm: Vec<i16> = (0..10_000).map(|i| ((i * 7919) % 65_536 - 32_768) as i16).collect();
        let text = pcm_codec::encode(&pcm);
        assert_eq!(pcm_codec::decode(&text).unwrap(), pcm);
        assert_eq!(pcm_codec::decode(&pcm_codec::encode(&[])).unwrap(), Vec::<i16>::new());
        assert!(pcm_codec::decode("not base64 !!").is_err());
    }

    #[test]
    fn slot_data_converts_and_measures() {
        let d = SlotData::new("Amen", 44_100, 2, &tone(1000, 2), SlotMeta::sliced(36, vec![0, 500], 120.0, 1000));
        assert_eq!(d.frames(), 1000);
        assert_eq!(d.memory_bytes(), 1000 * 2 * 4);
        let s = d.to_sample();
        assert_eq!(s.frames(), 1000);
        assert!(s.right.is_some());
        assert!((s.left[10] - (10.0 * 2.0 * 0.01f32).sin() * 0.7).abs() < 1e-4);
        assert!(d.is_valid());
    }

    #[test]
    fn key_ranges_and_well_formedness() {
        let s = SlotMeta::sliced(36, vec![0, 100, 200, 300], 120.0, 1000);
        assert_eq!(s.key_range(), (36, 39));
        assert!(s.is_well_formed(1000));
        assert!(!SlotMeta::sliced(36, vec![0, 300, 200], 120.0, 1000).is_well_formed(1000), "unsorted");
        assert!(!SlotMeta::sliced(36, vec![0, 1000], 120.0, 1000).is_well_formed(1000), "slice past the end");
        assert!(!SlotMeta::sliced(36, vec![], 120.0, 1000).is_well_formed(1000), "no slices");
        assert!(!SlotMeta::sliced(126, vec![0, 1, 2, 3], 120.0, 1000).is_well_formed(1000), "zone past 127");
        let c = SlotMeta::chromatic(48, 71, 60, 1000);
        assert_eq!(c.key_range(), (48, 71));
        assert!(c.is_well_formed(1000));
        let mut bad = c.clone();
        bad.loop_on = true;
        bad.loop_start = 900;
        bad.loop_end = 800;
        assert!(!bad.is_well_formed(1000));
    }

    #[test]
    fn slot_play_copies_the_meta() {
        let m = SlotMeta::sliced(40, vec![0, 10, 20], 100.0, 50);
        let p = SlotPlay::from_meta(&m);
        assert_eq!(p.slice_count, 3);
        assert_eq!(&p.slices[..3], &[0, 10, 20]);
        assert_eq!(p.key_range(), (40, 42));
        assert_eq!(p.end, 50);
        assert_eq!(SlotPlay::EMPTY.slice_count, 0);
    }
}
```

In `project.rs` tests, add:

```rust
    #[test]
    fn sampler_slots_round_trip_validate_and_place_zones() {
        use crate::sampler::slot::{SlotData, SlotMeta};
        let mut p = Project::default();
        assert_eq!(p.sampler.slots.len(), crate::sampler::SLOTS);
        let tone: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.02).sin() * 0.5).collect();
        p.sampler.slots[0] = Some(SlotData::new("Amen", 22_050, 1, &tone, SlotMeta::sliced(24, vec![0, 500, 1000, 1500], 120.0, 2000)));
        p.sampler.slots[3] = Some(SlotData::new("Vox", 22_050, 1, &tone, SlotMeta::chromatic(40, 63, 40, 2000)));
        let back = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(back, p);
        assert_eq!(p.sampler.slot_for_key(25), Some(0));
        assert_eq!(p.sampler.slot_for_key(50), Some(3));
        assert_eq!(p.sampler.slot_for_key(30), None);
        assert_eq!(p.sampler.free_base_key(1, 4), Some(28), "first gap above C1 that fits");
        assert_eq!(p.sampler.free_base_key(1, 20), Some(64));
        assert_eq!(p.sampler.free_base_key(0, 4), Some(24), "a slot ignores its own zone");
        assert!(p.sampler.zone_clash(1, &SlotMeta::chromatic(26, 30, 26, 2000)));
        assert!(!p.sampler.zone_clash(0, &SlotMeta::chromatic(26, 30, 26, 2000)));
        assert_eq!(p.sampler.memory_bytes(), 2 * 2000 * 4);

        let mut clash = p.clone();
        clash.sampler.slots[5] = Some(SlotData::new("X", 22_050, 1, &tone, SlotMeta::chromatic(26, 30, 26, 2000)));
        assert!(matches!(clash.validate(), Err(ProjectError::Invalid(_))), "overlapping zones");
        let mut broken = p.clone();
        broken.sampler.slots.pop();
        assert!(matches!(broken.validate(), Err(ProjectError::Invalid(_))));
        let mut bad_meta = p.clone();
        bad_meta.sampler.slots[0].as_mut().unwrap().meta.slices = vec![0, 5000];
        assert!(matches!(bad_meta.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn projects_without_sampler_slots_still_load() {
        let text = Project::default().to_ron();
        let start = text.find("slots:").expect("slots field");
        let open = start + text[start..].find('[').expect("slots list");
        let mut depth = 0;
        let mut close = open;
        for (i, c) in text[open..].char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let old = format!("{}{}", &text[..start], text[close + 1..].trim_start_matches(','));
        let p = Project::from_ron(&old).unwrap();
        assert_eq!(p.sampler.slots.len(), crate::sampler::SLOTS);
        assert!(p.sampler.slots.iter().all(Option::is_none));
    }
```

- [ ] **Step 3: Run them to make sure they fail**

Run: `cargo test -p acid-engine sampler:: ; cargo test -p acid-engine project::`
Expected: compile errors.

- [ ] **Step 4: Implement `sampler/mod.rs`**

```rust
//! The sampler: 16 sample slots (chopped breaks or chromatic voices),
//! 16 voices, choke, tempo lock and Grit. Audio is embedded in the
//! project as 16-bit PCM and handed to the audio thread as `Arc`s.

// DSP loops index parallel arrays.
#![allow(clippy::needless_range_loop)]

pub mod sample;
pub mod slot;

/// Sample slots.
pub const SLOTS: usize = crate::params::SAMPLER_SLOTS;
/// Voices shared by all slots.
pub const VOICES: usize = 16;
/// Most slices one slot may have.
pub const MAX_SLICES: usize = 64;
/// Longest sample a slot accepts.
pub const MAX_SECONDS: f32 = 30.0;
/// Most decoded audio (f32 bytes) a project may hold.
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// New zones are placed upward from here (MIDI 24 = C1).
pub const FIRST_KEY: u8 = 24;
```

Add `pub mod sampler;` to `lib.rs` (alphabetical).

- [ ] **Step 5: Implement `sampler/sample.rs`** (above its tests)

```rust
//! Decoded sample audio as the audio thread reads it. Immutable once
//! built; shared with the engine through an `Arc`.

#[derive(Debug, PartialEq)]
pub struct SampleData {
    /// Rate the audio was recorded at (playback converts on the fly).
    pub sample_rate: u32,
    pub left: Box<[f32]>,
    /// `None` for a mono sample (the left side plays on both).
    pub right: Option<Box<[f32]>>,
}

fn hermite(xm1: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b = w + a;
    ((a * t - b) * t + c) * t + x0
}

impl SampleData {
    pub fn frames(&self) -> usize {
        self.left.len()
    }

    fn at(data: &[f32], i: isize) -> f32 {
        let last = data.len() as isize - 1;
        data[i.clamp(0, last.max(0)) as usize]
    }

    fn cubic(data: &[f32], pos: f64) -> f32 {
        if data.is_empty() {
            return 0.0;
        }
        let i = pos.floor();
        let t = (pos - i) as f32;
        let i = i as isize;
        hermite(
            Self::at(data, i - 1),
            Self::at(data, i),
            Self::at(data, i + 1),
            Self::at(data, i + 2),
            t,
        )
    }

    /// Cubic-interpolated (left, right) at fractional frame `pos`,
    /// clamped to the sample.
    pub fn read(&self, pos: f64) -> (f32, f32) {
        let l = Self::cubic(&self.left, pos);
        let r = match &self.right {
            Some(r) => Self::cubic(r, pos),
            None => l,
        };
        (l, r)
    }
}
```

In the ramp test, `read(-5.0)` clamps every index to 0, so it returns `left[0]` = 0.0. `read(500.0)` returns `left[99]` = 0.99. At 10.5 the Hermite of a straight line is exact.

- [ ] **Step 6: Implement `sampler/slot.rs`** (above its tests)

```rust
//! A sample slot as the project stores it: its zone, slices or chromatic
//! range, and 16-bit PCM (saved deflated + base64). `SlotPlay` is the
//! fixed-size copy the audio thread uses.

use super::sample::SampleData;
use super::{MAX_SECONDS, MAX_SLICES};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotMode {
    #[default]
    Slice,
    Chromatic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotMeta {
    pub mode: SlotMode,
    /// Lowest key of the zone; slice 0 in slice mode.
    pub base_key: u8,
    /// Highest key (chromatic mode; slice mode derives it from the slices).
    pub high_key: u8,
    /// Key that plays the sample at its own pitch (chromatic).
    pub root: u8,
    /// Slice start frames, strictly ascending (slice mode).
    pub slices: Vec<u32>,
    /// The sample's tempo, for tempo lock (slice mode).
    pub original_bpm: f32,
    /// Play range in frames, end exclusive (chromatic).
    pub start: u32,
    pub end: u32,
    pub loop_on: bool,
    pub loop_start: u32,
    pub loop_end: u32,
}

impl SlotMeta {
    pub fn sliced(base_key: u8, slices: Vec<u32>, original_bpm: f32, frames: u32) -> Self {
        Self {
            mode: SlotMode::Slice,
            base_key,
            high_key: base_key.saturating_add(23).min(127),
            root: base_key,
            slices,
            original_bpm,
            start: 0,
            end: frames,
            loop_on: false,
            loop_start: 0,
            loop_end: frames,
        }
    }

    pub fn chromatic(base_key: u8, high_key: u8, root: u8, frames: u32) -> Self {
        Self {
            mode: SlotMode::Chromatic,
            base_key,
            high_key,
            root,
            slices: vec![0],
            original_bpm: 120.0,
            start: 0,
            end: frames,
            loop_on: false,
            loop_start: 0,
            loop_end: frames,
        }
    }

    /// Lowest and highest key of the zone. A slice zone may run past 127;
    /// `is_well_formed` rejects that.
    pub fn key_range(&self) -> (u8, u8) {
        match self.mode {
            SlotMode::Slice => {
                let n = self.slices.len().max(1) as u16;
                (self.base_key, (self.base_key as u16 + n - 1).min(255) as u8)
            }
            SlotMode::Chromatic => (self.base_key, self.high_key),
        }
    }

    pub fn is_well_formed(&self, frames: u32) -> bool {
        let slices_ok = (1..=MAX_SLICES).contains(&self.slices.len())
            && self.slices.windows(2).all(|w| w[0] < w[1])
            && self.slices.iter().all(|&s| s < frames);
        let top = match self.mode {
            SlotMode::Slice => self.base_key as usize + self.slices.len().max(1) - 1,
            SlotMode::Chromatic => self.high_key as usize,
        };
        slices_ok
            && self.base_key <= self.high_key
            && top <= 127
            && self.root <= 127
            && self.start < self.end
            && self.end <= frames
            && self.start <= self.loop_start
            && self.loop_start < self.loop_end
            && self.loop_end <= self.end
            && self.original_bpm.is_finite()
            && (20.0..=400.0).contains(&self.original_bpm)
    }
}

/// Embedded PCM: 16-bit little-endian samples, deflated, base64.
pub mod pcm_codec {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
    use serde::{Deserialize, Deserializer, Serializer};
    use std::io::{Read, Write};

    pub fn encode(pcm: &[i16]) -> String {
        let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&bytes).expect("writing to a Vec cannot fail");
        STANDARD.encode(enc.finish().expect("writing to a Vec cannot fail"))
    }

    pub fn decode(text: &str) -> Result<Vec<i16>, String> {
        let packed = STANDARD.decode(text).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        DeflateDecoder::new(&packed[..])
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() % 2 != 0 {
            return Err("odd number of PCM bytes".into());
        }
        Ok(bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect())
    }

    pub fn serialize<S: Serializer>(pcm: &[i16], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(pcm))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<i16>, D::Error> {
        let text = String::deserialize(d)?;
        decode(&text).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotData {
    pub name: String,
    pub meta: SlotMeta,
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u8,
    /// Interleaved 16-bit samples.
    #[serde(with = "pcm_codec")]
    pub pcm: Vec<i16>,
}

impl SlotData {
    /// A slot from decoded audio (interleaved, -1..1).
    pub fn new(name: &str, sample_rate: u32, channels: u8, samples: &[f32], meta: SlotMeta) -> Self {
        Self {
            name: name.to_string(),
            meta,
            sample_rate,
            channels,
            pcm: samples
                .iter()
                .map(|x| (x.clamp(-1.0, 1.0) * 32_767.0).round() as i16)
                .collect(),
        }
    }

    pub fn frames(&self) -> u32 {
        (self.pcm.len() / self.channels.max(1) as usize) as u32
    }

    /// Size of the decoded f32 audio the engine holds.
    pub fn memory_bytes(&self) -> usize {
        self.pcm.len() * 4
    }

    /// Builds the engine's copy. Allocates: call off the audio thread.
    pub fn to_sample(&self) -> SampleData {
        let ch = self.channels.max(1) as usize;
        let side = |c: usize| -> Box<[f32]> {
            self.pcm
                .iter()
                .skip(c)
                .step_by(ch)
                .map(|&s| s as f32 / 32_768.0)
                .collect()
        };
        SampleData {
            sample_rate: self.sample_rate,
            left: side(0),
            right: (ch == 2).then(|| side(1)),
        }
    }

    pub fn is_valid(&self) -> bool {
        let frames = self.frames();
        (1..=2).contains(&self.channels)
            && self.pcm.len() % self.channels as usize == 0
            && (8_000..=192_000).contains(&self.sample_rate)
            && frames > 0
            && frames as f32 <= MAX_SECONDS * self.sample_rate as f32
            && self.meta.is_well_formed(frames)
    }
}

/// The audio thread's fixed-size copy of a `SlotMeta`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotPlay {
    pub mode: SlotMode,
    pub base_key: u8,
    pub high_key: u8,
    pub root: u8,
    pub slice_count: usize,
    pub slices: [u32; MAX_SLICES],
    pub original_bpm: f32,
    pub start: u32,
    pub end: u32,
    pub loop_on: bool,
    pub loop_start: u32,
    pub loop_end: u32,
}

impl SlotPlay {
    pub const EMPTY: SlotPlay = SlotPlay {
        mode: SlotMode::Slice,
        base_key: 0,
        high_key: 0,
        root: 0,
        slice_count: 0,
        slices: [0; MAX_SLICES],
        original_bpm: 120.0,
        start: 0,
        end: 0,
        loop_on: false,
        loop_start: 0,
        loop_end: 0,
    };

    pub fn from_meta(m: &SlotMeta) -> Self {
        let mut slices = [0u32; MAX_SLICES];
        let count = m.slices.len().min(MAX_SLICES);
        slices[..count].copy_from_slice(&m.slices[..count]);
        Self {
            mode: m.mode,
            base_key: m.base_key,
            high_key: m.high_key,
            root: m.root,
            slice_count: count,
            slices,
            original_bpm: m.original_bpm,
            start: m.start,
            end: m.end,
            loop_on: m.loop_on,
            loop_start: m.loop_start,
            loop_end: m.loop_end,
        }
    }

    pub fn key_range(&self) -> (u8, u8) {
        match self.mode {
            SlotMode::Slice => {
                let n = self.slice_count.max(1) as u16;
                (self.base_key, (self.base_key as u16 + n - 1).min(255) as u8)
            }
            SlotMode::Chromatic => (self.base_key, self.high_key),
        }
    }
}

impl Default for SlotPlay {
    fn default() -> Self {
        Self::EMPTY
    }
}
```

Add `pub mod slot;` and `pub mod sample;` to `sampler/mod.rs` (already listed above).

- [ ] **Step 7: Extend the project** in `project.rs`

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SamplerMachine {
    pub patterns: Vec<NotePattern>,
    /// One entry per slot; `None` = empty.
    #[serde(default = "empty_slots")]
    pub slots: Vec<Option<crate::sampler::slot::SlotData>>,
}

fn empty_slots() -> Vec<Option<crate::sampler::slot::SlotData>> {
    vec![None; crate::sampler::SLOTS]
}
```

Set `slots: empty_slots()` in its `Default`. Then add:

```rust
impl SamplerMachine {
    fn range(&self, slot: usize) -> Option<(u8, u8)> {
        self.slots.get(slot)?.as_ref().map(|d| d.meta.key_range())
    }

    /// The first loaded slot whose zone contains `key`.
    pub fn slot_for_key(&self, key: u8) -> Option<usize> {
        (0..self.slots.len()).find(|&i| self.range(i).is_some_and(|(lo, hi)| (lo..=hi).contains(&key)))
    }

    /// True when `meta`'s zone would overlap another loaded slot's zone.
    pub fn zone_clash(&self, slot: usize, meta: &crate::sampler::slot::SlotMeta) -> bool {
        let (lo, hi) = meta.key_range();
        (0..self.slots.len())
            .filter(|&i| i != slot)
            .filter_map(|i| self.range(i))
            .any(|(a, b)| lo <= b && a <= hi)
    }

    /// Decoded audio the engine will hold, in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.slots.iter().flatten().map(|d| d.memory_bytes()).sum()
    }

    /// Lowest key from C1 up where a `span`-key zone fits without
    /// overlapping another slot's zone (`slot`'s own zone is ignored).
    pub fn free_base_key(&self, slot: usize, span: u8) -> Option<u8> {
        let span = span.max(1) as u16;
        (crate::sampler::FIRST_KEY as u16..=128 - span).map(|k| k as u8).find(|&k| {
            let hi = (k as u16 + span - 1) as u8;
            !(0..self.slots.len())
                .filter(|&i| i != slot)
                .filter_map(|i| self.range(i))
                .any(|(a, b)| k <= b && a <= hi)
        })
    }
}
```

In `validate`, after the FM bank check:

```rust
        if self.sampler.slots.len() != crate::sampler::SLOTS {
            return bad(format!("sampler has {} slots", self.sampler.slots.len()));
        }
        for (i, slot) in self.sampler.slots.iter().enumerate() {
            if let Some(d) = slot {
                if !d.is_valid() {
                    return bad(format!("sampler slot {}", i + 1));
                }
                if self.sampler.zone_clash(i, &d.meta) {
                    return bad(format!("sampler slot {} overlaps another zone", i + 1));
                }
            }
        }
```

`migrate.rs` builds from `Project::default()`, so it is unchanged.

In `free_base_key(1, 4)` with slot 0 on keys 24–27 and slot 3 on keys 40–63, the first fitting base from 24 is 28 (28–31). A 20-key span needs to clear 40–63, so it starts at 64.

- [ ] **Step 8: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

- [ ] **Step 9: Commit**

```bash
git add -A Cargo.toml Cargo.lock crates
git commit -m "feat(sampler): slot data with embedded PCM, sample storage, zones

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Slot settings, segments and the sampler voice

**Files:**
- Create: `crates/acid-engine/src/sampler/settings.rs`, `crates/acid-engine/src/sampler/voice.rs`
- Modify: `crates/acid-engine/src/sampler/mod.rs` (`pub mod settings; pub mod voice;`)

**Interfaces:**
- Consumes: `SampleData`, `SlotPlay`, `SlotMode` (Task 2); `SlotParam`, `sampler_map`, `on` (Task 1); `sid_core::Svf`.
- Produces:

```rust
// settings.rs
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotSettings { pub level_gain: f32, pub pan_l: f32, pub pan_r: f32, pub tune: i32, pub fine_cents: f32,
    pub cutoff_knob: f32, pub cutoff_hz: f32, pub resonance: f32, pub attack_s: f32, pub decay_s: f32,
    pub sustain: f32, pub release_s: f32, pub reverse: bool, pub choke: bool, pub tempo_lock: bool }
impl SlotSettings { pub fn set(&mut self, p: SlotParam, v: f32) }   // Default = every SlotParam default
// voice.rs
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment { pub start: f64, pub end: f64, pub reverse: bool, pub looping: Option<(f64, f64)>, pub rate: f64 }
pub fn segment(play: &SlotPlay, key: u8, s: &SlotSettings, sample_rate: u32, frames: usize, out_rate: f32, bpm: f32) -> Option<Segment>;
pub struct Voice;  // pub fields: active, held, live, slot, key, age
impl Voice {
    pub fn new(sample_rate: f32) -> Self;
    pub fn start(&mut self, seg: &Segment, s: &SlotSettings, slot: usize, key: u8, vel: u8, live: bool, age: u32);
    pub fn release(&mut self, seconds: f32);
    pub fn choke(&mut self);                // 5 ms release
    pub fn kill(&mut self);
    pub fn render(&mut self, sample: &SampleData, s: &SlotSettings, out_l: &mut [f32], out_r: &mut [f32]);  // adds
}
```

**Segment rules:**
- **Slice mode:** key `base + i` plays slice `i`, from `slices[i]` to `slices[i+1]` (or the sample end). The rate is `src/out · 2^((tune + fine/100)/12)`, times `bpm/original_bpm` when Tempo Lock is on.
- **Chromatic mode:** plays `start..end`, at an extra `2^((key − root)/12)`. It loops `loop_start..loop_end` with a 10 ms crossfade when the loop is on and Reverse is off.
- **Reverse** plays the segment backwards.
- **Fades:** a voice fades over 3 ms before its segment ends. Choke releases over 5 ms.

- [ ] **Step 1: Write the failing tests** at the bottom of `voice.rs` (with stubs above)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::slot::{SlotMeta, SlotPlay};

    const SR: f32 = 48_000.0;

    fn ramp_sample(frames: usize) -> SampleData {
        SampleData {
            sample_rate: 48_000,
            left: (0..frames).map(|i| i as f32 / frames as f32).collect(),
            right: None,
        }
    }

    fn sine_sample(frames: usize) -> SampleData {
        SampleData {
            sample_rate: 48_000,
            left: (0..frames).map(|i| (i as f32 * 0.05).sin() * 0.8).collect(),
            right: None,
        }
    }

    fn run(v: &mut Voice, sample: &SampleData, s: &SlotSettings, n: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        v.render(sample, s, &mut l, &mut r);
        assert_eq!(l, r, "centred mono");
        l
    }

    #[test]
    fn slices_map_keys_to_segments() {
        let play = SlotPlay::from_meta(&SlotMeta::sliced(36, vec![0, 1000, 3000], 120.0, 4000));
        let s = SlotSettings::default();
        let seg = segment(&play, 37, &s, 48_000, 4000, SR, 120.0).unwrap();
        assert_eq!((seg.start, seg.end, seg.rate), (1000.0, 3000.0, 1.0));
        let last = segment(&play, 38, &s, 48_000, 4000, SR, 120.0).unwrap();
        assert_eq!((last.start, last.end), (3000.0, 4000.0));
        assert!(segment(&play, 39, &s, 48_000, 4000, SR, 120.0).is_none(), "past the slices");
        assert!(segment(&play, 35, &s, 48_000, 4000, SR, 120.0).is_none(), "below the zone");
    }

    #[test]
    fn tempo_lock_and_rates() {
        let play = SlotPlay::from_meta(&SlotMeta::sliced(36, vec![0], 100.0, 4000));
        let mut s = SlotSettings::default();
        assert_eq!(segment(&play, 36, &s, 48_000, 4000, SR, 120.0).unwrap().rate, 1.0, "lock off");
        s.tempo_lock = true;
        let r = segment(&play, 36, &s, 48_000, 4000, SR, 120.0).unwrap().rate;
        assert!((r - 1.2).abs() < 1e-9);
        let r = segment(&play, 36, &s, 24_000, 4000, SR, 100.0).unwrap().rate;
        assert!((r - 0.5).abs() < 1e-9, "source rate is converted on the fly");
        s.tempo_lock = false;
        s.tune = 12;
        let r = segment(&play, 36, &s, 48_000, 4000, SR, 120.0).unwrap().rate;
        assert!((r - 2.0).abs() < 1e-9);
    }

    #[test]
    fn chromatic_keys_and_loops() {
        let mut meta = SlotMeta::chromatic(48, 71, 60, 8000);
        meta.loop_on = true;
        meta.loop_start = 2000;
        meta.loop_end = 6000;
        let play = SlotPlay::from_meta(&meta);
        let s = SlotSettings::default();
        let seg = segment(&play, 72 - 12, &s, 48_000, 8000, SR, 120.0).unwrap();
        assert_eq!(seg.rate, 1.0);
        assert_eq!(seg.looping, Some((2000.0, 6000.0)));
        assert!((segment(&play, 71, &s, 48_000, 8000, SR, 120.0).unwrap().rate - 2f64.powf(11.0 / 12.0)).abs() < 1e-9);
        let mut rev = s;
        rev.reverse = true;
        assert_eq!(segment(&play, 60, &rev, 48_000, 8000, SR, 120.0).unwrap().looping, None, "no loop backwards");
    }

    #[test]
    fn a_segment_plays_to_its_end_then_stops_with_a_fade() {
        let sample = sine_sample(4000);
        let s = SlotSettings::default();
        let seg = Segment { start: 1000.0, end: 2000.0, reverse: false, looping: None, rate: 1.0 };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 36, 127, false, 1);
        let out = run(&mut v, &sample, &s, 1200);
        assert!(!v.active, "ended after 1000 frames");
        let peak = out[..900].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.3);
        assert!(out[998].abs() < 0.05 * peak, "faded at the end: {}", out[998]);
        assert!(out[1100..].iter().all(|x| *x == 0.0));
    }

    #[test]
    fn reverse_plays_backwards() {
        let sample = ramp_sample(1000);
        let mut s = SlotSettings::default();
        s.reverse = true;
        let seg = Segment { start: 0.0, end: 1000.0, reverse: true, looping: None, rate: 1.0 };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 36, 127, false, 1);
        let out = run(&mut v, &sample, &s, 600);
        assert!(out[200] > out[400] && out[400] > out[550], "falling ramp");
    }

    #[test]
    fn double_rate_halves_the_length() {
        let sample = sine_sample(4000);
        let s = SlotSettings::default();
        let seg = Segment { start: 0.0, end: 2000.0, reverse: false, looping: None, rate: 2.0 };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 36, 127, false, 1);
        run(&mut v, &sample, &s, 990);
        assert!(v.active);
        run(&mut v, &sample, &s, 20);
        assert!(!v.active);
    }

    #[test]
    fn loops_hold_while_the_key_is_down_and_crossfade_smoothly() {
        let sample = sine_sample(8000);
        let s = SlotSettings::default();
        let seg = Segment { start: 0.0, end: 8000.0, reverse: false, looping: Some((2000.0, 3000.0)), rate: 1.0 };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 60, 127, false, 1);
        let out = run(&mut v, &sample, &s, 20_000);
        assert!(v.active, "still looping");
        let max_step = out[5000..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(max_step < 0.1, "no jumps at the loop point: {max_step}");
    }

    #[test]
    fn release_and_choke_end_the_voice() {
        let sample = sine_sample(48_000);
        let s = SlotSettings::default();
        let seg = Segment { start: 0.0, end: 48_000.0, reverse: false, looping: None, rate: 1.0 };
        let mut v = Voice::new(SR);
        v.start(&seg, &s, 0, 36, 127, false, 1);
        run(&mut v, &sample, &s, 100);
        v.release(0.01);
        assert!(!v.held);
        run(&mut v, &sample, &s, 6000);
        assert!(!v.active);
        let mut c = Voice::new(SR);
        c.start(&seg, &s, 0, 36, 127, false, 1);
        run(&mut c, &sample, &s, 100);
        c.choke();
        run(&mut c, &sample, &s, 3000);
        assert!(!c.active, "choked within 65 ms");
    }

    #[test]
    fn settings_default_and_decode() {
        let mut s = SlotSettings::default();
        assert_eq!((s.tune, s.fine_cents, s.sustain), (0, 0.0, 1.0));
        assert!(!s.reverse && !s.choke && !s.tempo_lock);
        assert!((s.pan_l - 1.0).abs() < 1e-6 && (s.pan_r - 1.0).abs() < 1e-6, "centre");
        s.set(SlotParam::Pan, 0.0);
        assert!(s.pan_r.abs() < 1e-6 && s.pan_l > 1.3);
        s.set(SlotParam::Choke, 1.0);
        assert!(s.choke);
    }
}
```

(Put `settings_default_and_decode` in `settings.rs`'s own test module if you prefer. Then import `SlotParam` there.)

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine sampler::`
Expected: compile errors.

- [ ] **Step 3: Implement `settings.rs`**

```rust
//! A slot's knobs in real units.

use crate::params::{ParamId, SlotParam, on, sampler_map};
use std::f32::consts::{FRAC_PI_2, SQRT_2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotSettings {
    pub level_gain: f32,
    pub pan_l: f32,
    pub pan_r: f32,
    pub tune: i32,
    pub fine_cents: f32,
    pub cutoff_knob: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,
    pub reverse: bool,
    pub choke: bool,
    pub tempo_lock: bool,
}

impl SlotSettings {
    pub fn set(&mut self, p: SlotParam, v: f32) {
        let v = v.clamp(0.0, 1.0);
        match p {
            SlotParam::Level => self.level_gain = sampler_map::level_gain(v),
            SlotParam::Pan => {
                let angle = v * FRAC_PI_2;
                self.pan_l = angle.cos() * SQRT_2;
                self.pan_r = angle.sin() * SQRT_2;
            }
            SlotParam::Tune => self.tune = sampler_map::tune_semitones(v),
            SlotParam::Fine => self.fine_cents = sampler_map::fine_cents(v),
            SlotParam::Cutoff => {
                self.cutoff_knob = v;
                self.cutoff_hz = sampler_map::cutoff_hz(v);
            }
            SlotParam::Resonance => self.resonance = v,
            SlotParam::Attack => self.attack_s = sampler_map::attack_seconds(v),
            SlotParam::Decay => self.decay_s = sampler_map::decay_seconds(v),
            SlotParam::Sustain => self.sustain = v,
            SlotParam::Release => self.release_s = sampler_map::release_seconds(v),
            SlotParam::Reverse => self.reverse = on(v),
            SlotParam::Choke => self.choke = on(v),
            SlotParam::TempoLock => self.tempo_lock = on(v),
        }
    }
}

impl Default for SlotSettings {
    /// Every slot knob at its default.
    fn default() -> Self {
        let mut s = SlotSettings {
            level_gain: 0.0,
            pan_l: 1.0,
            pan_r: 1.0,
            tune: 0,
            fine_cents: 0.0,
            cutoff_knob: 1.0,
            cutoff_hz: 18_000.0,
            resonance: 0.0,
            attack_s: 0.001,
            decay_s: 0.1,
            sustain: 1.0,
            release_s: 0.05,
            reverse: false,
            choke: false,
            tempo_lock: false,
        };
        for p in SlotParam::ALL {
            s.set(p, ParamId::SamplerSlot(0, p).info().default);
        }
        s
    }
}
```

At pan 0.5 the angle is π/4, so cos·√2 and sin·√2 are 1.0 within f32 rounding; the test uses a 1e-6 tolerance.

- [ ] **Step 4: Implement `voice.rs`** (above its tests)

```rust
//! One sampler voice: plays a segment of a sample forwards or backwards,
//! optionally looping with a crossfade, through an attack/decay/release
//! envelope and an optional low-pass filter.

use super::sample::SampleData;
use super::settings::SlotSettings;
use super::slot::{SlotMode, SlotPlay};
use sid_core::Svf;

/// Fade at the end of a segment (avoids a click when a slice stops).
const END_FADE_S: f32 = 0.003;
/// Release used when a newer note in the same slot chokes this one.
const CHOKE_S: f32 = 0.005;
/// Loop crossfade length.
const LOOP_XFADE_S: f64 = 0.010;
/// Envelope level at which a releasing voice stops.
const SILENT: f32 = 1e-4;

/// What a note plays: a range of source frames, its direction, an
/// optional loop, and the playback rate in source frames per output sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub reverse: bool,
    pub looping: Option<(f64, f64)>,
    pub rate: f64,
}

/// The segment `key` plays in a slot, or `None` outside its zone.
pub fn segment(
    play: &SlotPlay,
    key: u8,
    s: &SlotSettings,
    sample_rate: u32,
    frames: usize,
    out_rate: f32,
    bpm: f32,
) -> Option<Segment> {
    let (lo, hi) = play.key_range();
    if key < lo || key > hi || frames == 0 {
        return None;
    }
    let frames = frames as u32;
    let base = sample_rate as f64 / out_rate as f64;
    let tuned = (s.tune as f64 + s.fine_cents as f64 / 100.0) / 12.0;
    let (start, end, rate, looping) = match play.mode {
        SlotMode::Slice => {
            let i = (key - lo) as usize;
            if i >= play.slice_count {
                return None;
            }
            let start = play.slices[i];
            let end = if i + 1 < play.slice_count { play.slices[i + 1] } else { frames };
            let tempo = if s.tempo_lock && play.original_bpm > 0.0 {
                bpm as f64 / play.original_bpm as f64
            } else {
                1.0
            };
            (start, end, base * 2f64.powf(tuned) * tempo, None)
        }
        SlotMode::Chromatic => {
            let end = play.end.min(frames);
            let semis = tuned + (key as f64 - play.root as f64) / 12.0;
            let looping = (play.loop_on && !s.reverse && play.loop_start < play.loop_end.min(end))
                .then(|| (play.loop_start as f64, play.loop_end.min(end) as f64));
            (play.start, end, base * 2f64.powf(semis), looping)
        }
    };
    (start < end).then_some(Segment {
        start: start as f64,
        end: end as f64,
        reverse: s.reverse,
        looping,
        rate,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Release,
    Off,
}

pub struct Voice {
    pub active: bool,
    pub held: bool,
    pub live: bool,
    pub slot: usize,
    pub key: u8,
    pub age: u32,
    pos: f64,
    step: f64,
    end: f64,
    reverse: bool,
    looping: bool,
    loop_start: f64,
    loop_end: f64,
    env: f32,
    stage: Stage,
    release_coef: f32,
    gain: f32,
    filter: [Svf; 2],
    sample_rate: f32,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            active: false,
            held: false,
            live: false,
            slot: 0,
            key: 0,
            age: 0,
            pos: 0.0,
            step: 0.0,
            end: 0.0,
            reverse: false,
            looping: false,
            loop_start: 0.0,
            loop_end: 0.0,
            env: 0.0,
            stage: Stage::Off,
            release_coef: 0.0,
            gain: 0.0,
            filter: [Svf::new(sample_rate), Svf::new(sample_rate)],
            sample_rate,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start(&mut self, seg: &Segment, s: &SlotSettings, slot: usize, key: u8, vel: u8, live: bool, age: u32) {
        self.active = true;
        self.held = true;
        self.live = live;
        self.slot = slot;
        self.key = key;
        self.age = age;
        self.reverse = seg.reverse;
        if seg.reverse {
            self.pos = seg.end - 1.0;
            self.step = -seg.rate;
            self.end = seg.start;
        } else {
            self.pos = seg.start;
            self.step = seg.rate;
            self.end = seg.end;
        }
        match seg.looping {
            Some((a, b)) if !seg.reverse && a < b => {
                self.looping = true;
                self.loop_start = a;
                self.loop_end = b;
            }
            _ => self.looping = false,
        }
        self.env = 0.0;
        self.stage = Stage::Attack;
        self.gain = vel.min(127) as f32 / 127.0 * s.level_gain;
        self.filter = [Svf::new(self.sample_rate), Svf::new(self.sample_rate)];
    }

    pub fn release(&mut self, seconds: f32) {
        if self.active && self.stage != Stage::Off {
            self.held = false;
            self.stage = Stage::Release;
            self.release_coef = (-1.0 / (seconds.max(1e-4) * self.sample_rate)).exp();
        }
    }

    pub fn choke(&mut self) {
        self.release(CHOKE_S);
    }

    pub fn kill(&mut self) {
        self.active = false;
        self.held = false;
        self.stage = Stage::Off;
    }

    /// Adds this voice into `out_l`/`out_r`.
    pub fn render(&mut self, sample: &SampleData, s: &SlotSettings, out_l: &mut [f32], out_r: &mut [f32]) {
        if !self.active {
            return;
        }
        let sr = self.sample_rate;
        let attack_inc = 1.0 / (s.attack_s * sr).max(1.0);
        let decay_coef = (-1.0 / (s.decay_s * sr).max(1.0)).exp();
        let end_fade = (END_FADE_S * sr).max(1.0);
        let loop_len = self.loop_end - self.loop_start;
        let xfade = (LOOP_XFADE_S * sample.sample_rate as f64).min(loop_len * 0.5).max(1.0);
        let filter_on = s.cutoff_knob < 0.999 || s.resonance > 0.0;
        if filter_on {
            for f in &mut self.filter {
                f.set(s.cutoff_hz, s.resonance);
            }
        }
        for i in 0..out_l.len().min(out_r.len()) {
            match self.stage {
                Stage::Attack => {
                    self.env += attack_inc;
                    if self.env >= 1.0 {
                        self.env = 1.0;
                        self.stage = Stage::Decay;
                    }
                }
                Stage::Decay => self.env = s.sustain + (self.env - s.sustain) * decay_coef,
                Stage::Release => {
                    self.env *= self.release_coef;
                    if self.env < SILENT {
                        self.kill();
                        return;
                    }
                }
                Stage::Off => return,
            }
            let past_end = if self.reverse { self.pos < self.end } else { self.pos >= self.end };
            if past_end && !self.looping {
                self.kill();
                return;
            }
            let (mut l, mut r) = sample.read(self.pos);
            if self.looping && self.pos > self.loop_end - xfade {
                let a = ((self.loop_end - self.pos) / xfade) as f32;
                let (l2, r2) = sample.read(self.pos - loop_len);
                l = l * a + l2 * (1.0 - a);
                r = r * a + r2 * (1.0 - a);
            }
            let edge = if self.looping {
                1.0
            } else {
                ((((self.pos - self.end).abs() / self.step.abs()) as f32) / end_fade).min(1.0)
            };
            if filter_on {
                l = self.filter[0].process(l);
                r = self.filter[1].process(r);
            }
            let g = self.env * edge * self.gain;
            out_l[i] += l * g * s.pan_l;
            out_r[i] += r * g * s.pan_r;
            self.pos += self.step;
            if self.looping && self.pos >= self.loop_end {
                self.pos -= loop_len;
            }
        }
    }
}
```

**Timing notes:**
- In `a_segment_plays_to_its_end_then_stops_with_a_fade`, sample 998 is 2 frames before the end, so `edge` = 2/144. The voice stops at sample 1000. A 10 ms release falls below 1e-4 in about 4400 samples at 48 kHz, and the 5 ms choke in about 2200.

- [ ] **Step 5: Add the modules and run everything**

Add `pub mod settings;` and `pub mod voice;` to `sampler/mod.rs`.

Run: `cargo test -p acid-engine sampler:: && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(sampler): slot settings, segments and the sampler voice

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The Sampler machine

**Files:**
- Create: `crates/acid-engine/src/sampler/machine.rs`
- Modify: `crates/acid-engine/src/sampler/mod.rs` (`pub mod machine; pub use machine::Sampler;`)

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces:

```rust
pub struct Sampler;
impl Sampler {
    pub fn new(sample_rate: f32) -> Self;
    pub fn set_param(&mut self, p: SamplerParam, v: f32);
    pub fn set_slot_param(&mut self, slot: usize, p: SlotParam, v: f32);
    pub fn set_bpm(&mut self, bpm: f32);
    pub fn swap_sample(&mut self, slot: usize, sample: &mut Option<Arc<SampleData>>) -> bool;  // RT-safe swap
    pub fn swap_play(&mut self, slot: usize, play: &mut SlotPlay) -> bool;
    pub fn note_on(&mut self, key: u8, vel: u8, live: bool);
    pub fn note_off(&mut self, key: u8, live: bool);
    pub fn all_off(&mut self);
    pub fn reset(&mut self);
    pub fn active_voices(&self) -> usize;
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]);   // adds
}
```

**Behaviour:**
- **Note routing:** a note goes to the first slot that holds a sample and whose zone contains the key.
- **Choke:** a slot with Choke on releases its other sounding voices over 5 ms when a new note starts.
- **Stealing:** a free voice is used first, then the oldest released voice, then the oldest voice.
- **Swapping:** swapping a slot's sample or meta kills that slot's voices.
- **Grit:** sample-and-hold at 26.04 kHz, then 12-bit quantisation over ±4, applied before the volume.
- **Volume:** the volume knob is squared.

- [ ] **Step 1: Write the failing tests** (bottom of `machine.rs`, with stubs)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampler::slot::{SlotMeta, SlotPlay};
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 48_000.0;

    fn sine(frames: usize) -> Arc<SampleData> {
        Arc::new(SampleData {
            sample_rate: 48_000,
            left: (0..frames).map(|i| (i as f32 * 0.05).sin() * 0.8).collect(),
            right: None,
        })
    }

    fn loaded() -> Sampler {
        let mut s = Sampler::new(SR);
        let mut a = Some(sine(48_000));
        assert!(s.swap_sample(0, &mut a));
        assert!(a.is_none(), "got the empty slot back");
        let mut play = SlotPlay::from_meta(&SlotMeta::sliced(36, vec![0, 12_000, 24_000, 36_000], 120.0, 48_000));
        assert!(s.swap_play(0, &mut play));
        let mut b = Some(sine(48_000));
        s.swap_sample(1, &mut b);
        let mut chrom = SlotPlay::from_meta(&SlotMeta::chromatic(60, 83, 60, 48_000));
        s.swap_play(1, &mut chrom);
        s
    }

    fn render(s: &mut Sampler, n: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        s.render(&mut l, &mut r);
        (l, r)
    }

    #[test]
    fn notes_in_a_zone_play_and_others_are_silent() {
        let mut s = loaded();
        s.note_on(30, 100, false);
        s.note_on(50, 100, false);
        assert_eq!(s.active_voices(), 0, "no zone there");
        s.note_on(37, 100, false);
        s.note_on(64, 100, false);
        assert_eq!(s.active_voices(), 2);
        let (l, r) = render(&mut s, 4800);
        assert!(all_finite(&l) && peak(&l) > 0.1 && peak(&r) > 0.1);
    }

    #[test]
    fn choke_cuts_the_slots_previous_note() {
        let mut s = loaded();
        s.set_slot_param(0, SlotParam::Choke, 1.0);
        s.note_on(36, 100, false);
        render(&mut s, 480);
        s.note_on(37, 100, false);
        render(&mut s, 3000);
        assert_eq!(s.active_voices(), 1, "the first slice was choked");
        let mut free = loaded();
        free.note_on(36, 100, false);
        render(&mut free, 480);
        free.note_on(37, 100, false);
        render(&mut free, 3000);
        assert_eq!(free.active_voices(), 2, "without choke both ring");
    }

    #[test]
    fn audition_and_pattern_release_separately() {
        let mut s = loaded();
        s.note_on(60, 100, true);
        s.note_on(60, 100, false);
        s.note_off(60, false);
        render(&mut s, 24_000);
        assert_eq!(s.active_voices(), 1);
        s.note_off(60, true);
        render(&mut s, 24_000);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn swapping_a_sample_stops_its_voices_and_returns_the_old_one() {
        let mut s = loaded();
        s.note_on(36, 100, false);
        let mut none: Option<Arc<SampleData>> = None;
        assert!(s.swap_sample(0, &mut none));
        assert!(none.is_some(), "old sample handed back for disposal");
        assert_eq!(s.active_voices(), 0);
        s.note_on(36, 100, false);
        assert_eq!(s.active_voices(), 0, "empty slot");
        assert!(!s.swap_sample(16, &mut None));
    }

    #[test]
    fn sixteen_voices_and_the_oldest_is_stolen() {
        let mut s = loaded();
        for k in 60..76 {
            s.note_on(k, 100, false);
        }
        assert_eq!(s.active_voices(), 16);
        s.note_on(80, 100, false);
        assert_eq!(s.active_voices(), 16);
    }

    #[test]
    fn grit_quantises_and_volume_scales() {
        let mut s = loaded();
        s.set_param(SamplerParam::Grit, 1.0);
        s.set_param(SamplerParam::Volume, 1.0);
        s.note_on(60, 127, false);
        let (l, _) = render(&mut s, 4800);
        assert!(peak(&l) > 0.1);
        for v in &l {
            let steps = v / 4.0 * 2048.0;
            assert!((steps - steps.round()).abs() < 1e-3, "{v}");
        }
        let held = l.windows(2).filter(|w| w[0] == w[1]).count();
        assert!(held > 1000, "sample-and-hold repeats values: {held}");
        let mut quiet = loaded();
        quiet.set_param(SamplerParam::Volume, 0.0);
        quiet.note_on(60, 127, false);
        assert_eq!(peak(&render(&mut quiet, 480).0), 0.0);
    }

    #[test]
    fn all_off_releases_and_reset_silences() {
        let mut s = loaded();
        s.note_on(60, 100, false);
        s.note_on(61, 100, true);
        s.all_off();
        render(&mut s, 24_000);
        assert_eq!(s.active_voices(), 0);
        s.note_on(60, 100, false);
        s.reset();
        assert_eq!(s.active_voices(), 0);
        assert_eq!(peak(&render(&mut s, 100).0), 0.0);
    }
}
```

(The default Release, 0.3, is about 37 ms. That reaches silence in about 0.35 s, so the tests wait 24,000 samples for released voices.)

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine sampler::machine`
Expected: compile errors.

- [ ] **Step 3: Implement `machine.rs`** (above the tests)

```rust
//! The sampler machine: 16 slots, 16 voices, choke, tempo lock and Grit.
//! Real-time safe: samples arrive and leave as `Arc`s through `swap_*`,
//! so nothing is allocated or freed while rendering.

use super::sample::SampleData;
use super::settings::SlotSettings;
use super::slot::SlotPlay;
use super::voice::{Voice, segment};
use super::{SLOTS, VOICES};
use crate::params::{SamplerParam, SlotParam, on};
use std::sync::Arc;

const CHUNK: usize = 64;
/// Grit's sample-and-hold rate (an SP-1200-style 26.04 kHz).
const GRIT_RATE: f32 = 26_040.0;
/// Grit quantises over ±this range so chords don't clip.
const GRIT_RANGE: f32 = 4.0;

pub struct Sampler {
    sample_rate: f32,
    bpm: f32,
    samples: [Option<Arc<SampleData>>; SLOTS],
    plays: [SlotPlay; SLOTS],
    settings: [SlotSettings; SLOTS],
    voices: [Voice; VOICES],
    clock: u32,
    volume: f32,
    grit: bool,
    grit_phase: f32,
    grit_hold: [f32; 2],
    buf_l: [f32; CHUNK],
    buf_r: [f32; CHUNK],
}

fn grit_quantise(x: f32) -> f32 {
    ((x / GRIT_RANGE).clamp(-1.0, 1.0) * 2048.0).round() / 2048.0 * GRIT_RANGE
}

impl Sampler {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            bpm: 120.0,
            samples: std::array::from_fn(|_| None),
            plays: [SlotPlay::EMPTY; SLOTS],
            settings: [SlotSettings::default(); SLOTS],
            voices: std::array::from_fn(|_| Voice::new(sample_rate)),
            clock: 0,
            volume: 0.49,
            grit: false,
            grit_phase: 0.0,
            grit_hold: [0.0; 2],
            buf_l: [0.0; CHUNK],
            buf_r: [0.0; CHUNK],
        }
    }

    pub fn set_param(&mut self, p: SamplerParam, v: f32) {
        match p {
            SamplerParam::Volume => self.volume = v.clamp(0.0, 1.0).powi(2),
            SamplerParam::Grit => self.grit = on(v),
        }
    }

    pub fn set_slot_param(&mut self, slot: usize, p: SlotParam, v: f32) {
        if let Some(s) = self.settings.get_mut(slot) {
            s.set(p, v);
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        if bpm.is_finite() && bpm > 0.0 {
            self.bpm = bpm;
        }
    }

    fn kill_slot(&mut self, slot: usize) {
        for v in self.voices.iter_mut().filter(|v| v.slot == slot) {
            v.kill();
        }
    }

    /// Exchanges a slot's sample with `sample`. The caller disposes of
    /// what comes back off the audio thread. False for a bad slot.
    pub fn swap_sample(&mut self, slot: usize, sample: &mut Option<Arc<SampleData>>) -> bool {
        if slot >= SLOTS {
            return false;
        }
        self.kill_slot(slot);
        std::mem::swap(&mut self.samples[slot], sample);
        true
    }

    pub fn swap_play(&mut self, slot: usize, play: &mut SlotPlay) -> bool {
        if slot >= SLOTS {
            return false;
        }
        self.kill_slot(slot);
        std::mem::swap(&mut self.plays[slot], play);
        true
    }

    pub fn note_on(&mut self, key: u8, vel: u8, live: bool) {
        if key > 127 || vel == 0 {
            return;
        }
        let Some(slot) = (0..SLOTS).find(|&i| {
            let (lo, hi) = self.plays[i].key_range();
            self.samples[i].is_some() && (lo..=hi).contains(&key)
        }) else {
            return;
        };
        let Some(sample) = self.samples[slot].as_deref() else {
            return;
        };
        let s = self.settings[slot];
        let Some(seg) = segment(
            &self.plays[slot],
            key,
            &s,
            sample.sample_rate,
            sample.frames(),
            self.sample_rate,
            self.bpm,
        ) else {
            return;
        };
        if s.choke {
            for v in self.voices.iter_mut().filter(|v| v.active && v.slot == slot) {
                v.choke();
            }
        }
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        let voices = &self.voices;
        let age = |v: &Voice| clock.wrapping_sub(v.age);
        let i = voices
            .iter()
            .position(|v| !v.active)
            .or_else(|| (0..VOICES).filter(|&i| !voices[i].held).max_by_key(|&i| age(&voices[i])))
            .unwrap_or_else(|| (0..VOICES).max_by_key(|&i| age(&voices[i])).unwrap_or(0));
        self.voices[i].start(&seg, &s, slot, key, vel.min(127), live, clock);
    }

    pub fn note_off(&mut self, key: u8, live: bool) {
        for v in &mut self.voices {
            if v.active && v.held && v.key == key && v.live == live {
                let release = self.settings[v.slot].release_s;
                v.release(release);
            }
        }
    }

    pub fn all_off(&mut self) {
        for v in &mut self.voices {
            let release = self.settings[v.slot].release_s;
            v.release(release);
        }
    }

    pub fn reset(&mut self) {
        for v in &mut self.voices {
            v.kill();
        }
        self.grit_hold = [0.0; 2];
        self.grit_phase = 0.0;
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let total = left.len().min(right.len());
        let mut pos = 0;
        while pos < total {
            let n = (total - pos).min(CHUNK);
            self.buf_l[..n].fill(0.0);
            self.buf_r[..n].fill(0.0);
            for v in self.voices.iter_mut().filter(|v| v.active) {
                match self.samples[v.slot].as_deref() {
                    Some(sample) => v.render(sample, &self.settings[v.slot], &mut self.buf_l[..n], &mut self.buf_r[..n]),
                    None => v.kill(),
                }
            }
            for i in 0..n {
                let (mut l, mut r) = (self.buf_l[i], self.buf_r[i]);
                if self.grit {
                    self.grit_phase += GRIT_RATE / self.sample_rate;
                    if self.grit_phase >= 1.0 {
                        self.grit_phase -= self.grit_phase.floor();
                        self.grit_hold = [grit_quantise(l), grit_quantise(r)];
                    }
                    [l, r] = self.grit_hold;
                }
                left[pos + i] += l * self.volume;
                right[pos + i] += r * self.volume;
            }
            pos += n;
        }
    }
}
```

`grit_quantises_and_volume_scales` uses volume 1.0, so the held values stay on the 4/2048 grid.

- [ ] **Step 4: Add the module and run everything**

Add `pub mod machine;` and `pub use machine::Sampler;` to `sampler/mod.rs`.

Run: `cargo test -p acid-engine sampler:: && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "feat(sampler): 16-voice sampler with zones, choke, tempo lock and Grit

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: The sampler in the engine

**Files:**
- Modify: `crates/acid-engine/src/engine.rs`, `crates/acid-engine/src/host.rs`, `crates/acid-engine/src/lib.rs`, `crates/acid-engine/tests/no_alloc.rs`
- Delete: `crates/acid-engine/src/placeholder.rs`

**Interfaces:**
- Consumes: `Sampler`, `SampleData`, `SlotPlay`, `SlotData::to_sample` (Tasks 2–4).
- Produces:

```rust
// engine.rs
pub fn swap_sampler_sample(&mut self, slot: usize, sample: &mut Option<Arc<SampleData>>) -> bool;
pub fn swap_sampler_play(&mut self, slot: usize, play: &mut SlotPlay) -> bool;
// host.rs
Command::SamplerSample { slot: u8, sample: Option<Arc<SampleData>> }
Command::SamplerPlay { slot: u8, play: Box<SlotPlay> }
Event::DisposeSample(Option<Arc<SampleData>>)
Event::DisposeSlotPlay(Box<SlotPlay>)
```

- [ ] **Step 1: Write the failing tests**

In `engine.rs` tests:

```rust
    fn sampler_project() -> Project {
        use crate::project::Note;
        use crate::sampler::slot::{SlotData, SlotMeta};
        let mut p = Project::default();
        let tone: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
        p.sampler.slots[0] = Some(SlotData::new(
            "Tone",
            44_100,
            1,
            &tone,
            SlotMeta::sliced(36, vec![0, 11_025, 22_050, 33_075], 120.0, 44_100),
        ));
        p.sampler.patterns[0].notes.push(Note { start: 0, len: 4, pitch: 37, vel: 120 });
        p
    }

    #[test]
    fn the_sampler_plays_loaded_slots_on_channel_5() {
        let mut e = Engine::from_project(SR, &sampler_project());
        e.play();
        render(&mut e, 0.2);
        let m = e.take_meters();
        assert!(m[5] > 0.01, "{m:?}");
        assert!(m[..5].iter().all(|v| *v == 0.0), "{m:?}");
    }

    #[test]
    fn sampler_audition_and_slot_swaps() {
        let mut e = Engine::from_project(SR, &sampler_project());
        e.note_on(5, 38, 100);
        let (l, _) = render(&mut e, 0.1);
        assert!(peak(&l) > 0.01, "audition plays slice 3");
        let mut none = None;
        assert!(e.swap_sampler_sample(0, &mut none));
        assert!(none.is_some());
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-3, "slot emptied");
        let mut play = crate::sampler::slot::SlotPlay::EMPTY;
        assert!(e.swap_sampler_play(0, &mut play));
        assert!(!e.swap_sampler_play(16, &mut play));
    }

    #[test]
    fn stopping_twice_silences_sampler_loops() {
        use crate::sampler::slot::{SlotData, SlotMeta};
        let mut p = Project::default();
        let tone: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
        let mut meta = SlotMeta::chromatic(48, 71, 60, 44_100);
        meta.loop_on = true;
        meta.loop_start = 10_000;
        meta.loop_end = 30_000;
        p.sampler.slots[0] = Some(SlotData::new("Loop", 44_100, 1, &tone, meta));
        p.params.insert(ParamId::SamplerSlot(0, crate::params::SlotParam::Release), 1.0);
        let mut e = Engine::from_project(SR, &p);
        e.note_on(5, 60, 100);
        render(&mut e, 0.1);
        e.stop();
        e.stop();
        let (l, _) = render(&mut e, 0.05);
        assert_eq!(peak(&l), 0.0);
    }
```

In `host.rs` tests, add a test that pushes `Command::SamplerSample { slot: 0, sample: Some(Arc::new(..)) }` and then a second one, and asserts that one `Event::DisposeSample(Some(_))` comes back. It also pushes `Command::SamplerPlay { slot: 0, play: Box::new(SlotPlay::EMPTY) }` and asserts that an `Event::DisposeSlotPlay(_)` comes back. Mirror the existing `DisposeSynthPattern` test in that file: same harness, same polling of the event consumer.

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine`
Expected: compile errors.

- [ ] **Step 3: Implement**

`engine.rs`:
- Replace the field `sampler: SineVoices` with `sampler: Sampler` and update its doc comment. Initialise it with `Sampler::new(sample_rate)`. Import `crate::sampler::{Sampler, SLOTS, sample::SampleData, slot::SlotPlay}` and `std::sync::Arc`, and remove the `placeholder` import.
- In `apply_param`, replace the sampler arm with:

```rust
            ParamId::Sampler(p) => self.sampler.set_param(p, v),
            ParamId::SamplerSlot(s, p) => self.sampler.set_slot_param(s as usize, p, v),
```

- In `poly_on`/`poly_off`, the sampler arms become `self.sampler.note_on(pitch, vel.min(127), live)` and `self.sampler.note_off(pitch, live)`.
- In `set_bpm`, add `self.sampler.set_bpm(self.seq.bpm());`.
- In `stop()`'s stopped branch, call `self.sampler.reset();` next to `self.fm.reset();`. `end_playback` keeps `self.sampler.all_off()`, and the guard branch keeps `self.sampler.reset()`.
- In `load_project`, after the note patterns:

```rust
        for i in 0..SLOTS {
            let data = p.sampler.slots.get(i).and_then(Option::as_ref);
            let mut sample = data.map(|d| Arc::new(d.to_sample()));
            self.sampler.swap_sample(i, &mut sample);
            let mut play = data.map_or(SlotPlay::EMPTY, |d| SlotPlay::from_meta(&d.meta));
            self.sampler.swap_play(i, &mut play);
        }
```

- Add:

```rust
    /// Exchanges a sampler slot's audio (real-time safe; the old sample
    /// comes back for disposal off the audio thread).
    pub fn swap_sampler_sample(&mut self, slot: usize, sample: &mut Option<Arc<SampleData>>) -> bool {
        self.sampler.swap_sample(slot, sample)
    }

    pub fn swap_sampler_play(&mut self, slot: usize, play: &mut SlotPlay) -> bool {
        self.sampler.swap_play(slot, play)
    }
```

- Update the module doc comment: there is no sampler placeholder any more.

`host.rs`: add the two `Command` variants and the two `Event` variants from Interfaces, plus these `drain_commands` arms:

```rust
                Command::SamplerSample { slot, mut sample } => {
                    self.engine.swap_sampler_sample(slot as usize, &mut sample);
                    self.dispose(Event::DisposeSample(sample));
                }
                Command::SamplerPlay { slot, mut play } => {
                    self.engine.swap_sampler_play(slot as usize, &mut play);
                    self.dispose(Event::DisposeSlotPlay(play));
                }
```

Delete `crates/acid-engine/src/placeholder.rs` and its `pub mod placeholder;` line in `lib.rs`.

`tests/no_alloc.rs`: in `busy_project()`, load a slot so the existing sampler pattern notes (pitches 36–47) play real samples:

```rust
    let tone: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.03).sin() * 0.6).collect();
    let slices: Vec<u32> = (0..12).map(|i| i * 3675).collect();
    p.sampler.slots[0] = Some(acid_engine::sampler::slot::SlotData::new(
        "Busy",
        44_100,
        1,
        &tone,
        acid_engine::sampler::slot::SlotMeta::sliced(36, slices, 120.0, 44_100),
    ));
    p.params.insert(ParamId::SamplerSlot(0, acid_engine::params::SlotParam::Choke), 1.0);
    p.params.insert(ParamId::Sampler(acid_engine::params::SamplerParam::Grit), 1.0);
```

Every existing no_alloc test must still report 0 allocations.

- [ ] **Step 4: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings. If `no_alloc` fails, the sampler path allocates; fix it, never the test.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "feat(engine): the Sampler slot plays the sampler; samples arrive as Arcs

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Decoding and slicing

**Files:**
- Create: `crates/acid-engine/src/sampler/decode.rs`, `crates/acid-engine/src/sampler/slicing.rs`
- Modify: `crates/acid-engine/src/sampler/mod.rs` (`pub mod decode; pub mod slicing;`)

**Interfaces:**
- Produces:

```rust
// decode.rs
pub struct Decoded { pub sample_rate: u32, pub channels: u8, pub samples: Vec<f32> /* interleaved, 1–2 ch */ }
impl Decoded { pub fn frames(&self) -> usize; pub fn seconds(&self) -> f32; }
pub enum DecodeError { Unsupported(String), Empty, TooLong }   // Display
pub fn decode(bytes: &[u8], extension: Option<&str>) -> Result<Decoded, DecodeError>;
// slicing.rs
pub fn mono_mix(samples: &[f32], channels: usize) -> Vec<f32>;
pub fn equal_slices(frames: u32, count: usize) -> Vec<u32>;
pub fn detect_slices(mono: &[f32], sample_rate: u32, sensitivity: f32) -> Vec<u32>;
pub fn guess_bpm(frames: u32, sample_rate: u32) -> f32;
```

- [ ] **Step 1: Write the failing tests**

`decode.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, frames: usize) -> Vec<u8> {
        let mut cur = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::new(&mut cur, spec).unwrap();
        for i in 0..frames {
            for c in 0..channels {
                let v = ((i as f32 * 0.05).sin() * 16_000.0) as i16;
                w.write_sample(if c == 0 { v } else { -v }).unwrap();
            }
        }
        w.finalize().unwrap();
        cur.into_inner()
    }

    #[test]
    fn decodes_mono_and_stereo_wav() {
        let d = decode(&wav(1, 22_050, 11_025), Some("wav")).unwrap();
        assert_eq!((d.sample_rate, d.channels, d.frames()), (22_050, 1, 11_025));
        assert!((d.seconds() - 0.5).abs() < 1e-6);
        assert!((d.samples[20] - (20.0f32 * 0.05).sin() * 16_000.0 / 32_768.0).abs() < 1e-3);
        let s = decode(&wav(2, 44_100, 1000), None).unwrap();
        assert_eq!((s.channels, s.frames()), (2, 1000));
        assert!((s.samples[41] + s.samples[40]).abs() < 1e-4, "right is the negated left");
    }

    #[test]
    fn refuses_garbage_empty_and_long_files() {
        assert!(matches!(decode(b"definitely not audio", Some("wav")), Err(DecodeError::Unsupported(_))));
        assert_eq!(decode(&wav(1, 8_000, 0), Some("wav")), Err(DecodeError::Empty));
        assert_eq!(decode(&wav(1, 8_000, 8_000 * 31), Some("wav")), Err(DecodeError::TooLong));
        assert!(!DecodeError::TooLong.to_string().is_empty());
    }
}
```

`slicing.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 44_100;

    fn clicks(at: &[(f32, f32)], seconds: f32) -> Vec<f32> {
        let mut x = vec![0.0; (seconds * SR as f32) as usize];
        for &(t, amp) in at {
            let start = (t * SR as f32) as usize;
            for k in 0..400 {
                if let Some(s) = x.get_mut(start + k) {
                    *s = amp * (k as f32 * 0.3).sin() * (-(k as f32) / 120.0).exp();
                }
            }
        }
        x
    }

    #[test]
    fn equal_slices_divide_evenly() {
        assert_eq!(equal_slices(1000, 4), vec![0, 250, 500, 750]);
        assert_eq!(equal_slices(1000, 1), vec![0]);
        assert_eq!(equal_slices(10, 64).len(), 10, "never more slices than frames");
        assert_eq!(equal_slices(100_000, 500).len(), 64, "capped");
    }

    #[test]
    fn transients_are_found_near_the_clicks() {
        let x = clicks(&[(0.0, 0.8), (0.25, 0.8), (0.5, 0.8), (0.75, 0.8)], 1.0);
        let found = detect_slices(&x, SR, 0.5);
        assert_eq!(found.len(), 4, "{found:?}");
        for (f, t) in found.iter().zip([0.0f32, 0.25, 0.5, 0.75]) {
            let at = (t * SR as f32) as i64;
            assert!((*f as i64 - at).abs() <= 300, "{f} vs {at}");
        }
    }

    #[test]
    fn sensitivity_controls_quiet_hits() {
        let x = clicks(&[(0.0, 0.8), (0.3, 0.8), (0.6, 0.8), (0.9, 0.08)], 1.2);
        assert_eq!(detect_slices(&x, SR, 1.0).len(), 4);
        assert_eq!(detect_slices(&x, SR, 0.0).len(), 3);
        assert_eq!(detect_slices(&vec![0.0; 10_000], SR, 0.5), vec![0], "silence: one slice");
    }

    #[test]
    fn bpm_guess_picks_a_bar_count_in_range() {
        assert_eq!(guess_bpm(2 * SR, SR), 120.0);
        assert_eq!(guess_bpm(4 * SR, SR), 120.0);
        assert!((guess_bpm((1.6 * SR as f32) as u32, SR) - 150.0).abs() < 0.01);
        assert!((guess_bpm(3 * SR, SR) - 160.0).abs() < 0.01);
        assert_eq!(guess_bpm(SR / 2, SR), 120.0);
    }

    #[test]
    fn mono_mix_averages_channels() {
        assert_eq!(mono_mix(&[1.0, -1.0, 0.5, 0.5], 2), vec![0.0, 0.5]);
        assert_eq!(mono_mix(&[0.2, 0.4], 1), vec![0.2, 0.4]);
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p acid-engine sampler::`
Expected: compile errors.

- [ ] **Step 3: Implement `decode.rs`** (verified against symphonia 0.5.5)

```rust
//! Decoding audio files (WAV, FLAC, MP3, OGG Vorbis) with symphonia.
//! Not real-time: called on the UI thread when a sample is loaded.

use super::MAX_SECONDS;
use std::fmt;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    pub sample_rate: u32,
    /// 1 or 2 (more channels are cut to the first two).
    pub channels: u8,
    /// Interleaved, -1..1.
    pub samples: Vec<f32>,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.sample_rate.max(1) as f32
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Unsupported(String),
    Empty,
    TooLong,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Unsupported(e) => write!(f, "the file could not be read as audio ({e})"),
            DecodeError::Empty => f.write_str("the file has no audio"),
            DecodeError::TooLong => write!(f, "the sample is longer than {MAX_SECONDS:.0} seconds"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub fn decode(bytes: &[u8], extension: Option<&str>) -> Result<Decoded, DecodeError> {
    let unsupported = |e: Error| DecodeError::Unsupported(e.to_string());
    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes.to_vec())), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(unsupported)?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| DecodeError::Unsupported("no audio track".into()))?;
    let track_id = track.id;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(unsupported)?;
    let mut channels = 0usize;
    let mut samples: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(unsupported(e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                let spec = *audio.spec();
                sample_rate = spec.rate;
                let ch = spec.channels.count();
                let mut buf = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
                buf.copy_interleaved_ref(audio);
                let keep = ch.min(2);
                channels = keep;
                for frame in buf.samples().chunks(ch.max(1)) {
                    samples.extend_from_slice(&frame[..keep.min(frame.len())]);
                }
                if sample_rate > 0 && samples.len() / keep.max(1) > (MAX_SECONDS * sample_rate as f32) as usize {
                    return Err(DecodeError::TooLong);
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(unsupported(e)),
        }
    }
    if channels == 0 || samples.is_empty() || sample_rate == 0 {
        return Err(DecodeError::Empty);
    }
    Ok(Decoded {
        sample_rate,
        channels: channels as u8,
        samples,
    })
}
```

If symphonia reports a zero-frame WAV as an error rather than an empty stream, map it to `DecodeError::Empty` when the probe succeeded but no packet decoded. Do that by tracking `decoded_any: bool` and returning `Empty` instead of `Unsupported` for errors after a successful probe with no audio yet. The test `decode(&wav(1, 8_000, 0)) == Err(Empty)` pins this behaviour.

- [ ] **Step 4: Implement `slicing.rs`**

```rust
//! Chopping a sample into slices: equal divisions or transient
//! detection, plus a tempo guess for tempo lock.

use super::MAX_SLICES;

/// Interleaved audio averaged to one channel.
pub fn mono_mix(samples: &[f32], channels: usize) -> Vec<f32> {
    let ch = channels.max(1);
    samples.chunks(ch).map(|f| f.iter().sum::<f32>() / f.len() as f32).collect()
}

/// `count` equal slices (at most `MAX_SLICES`, at most one per frame).
pub fn equal_slices(frames: u32, count: usize) -> Vec<u32> {
    let count = count.clamp(1, MAX_SLICES).min(frames.max(1) as usize);
    (0..count)
        .map(|i| (i as u64 * frames as u64 / count as u64) as u32)
        .collect()
}

/// Slice starts at the sample's transients: rises in short-term energy
/// above a threshold set by `sensitivity` (0 = only the strongest hits,
/// 1 = quiet ones too), at least 50 ms apart. Always starts with 0.
pub fn detect_slices(mono: &[f32], sample_rate: u32, sensitivity: f32) -> Vec<u32> {
    let hop = (sample_rate as usize / 200).max(32);
    let energy: Vec<f32> = mono
        .chunks(hop)
        .map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let flux: Vec<f32> = (0..energy.len())
        .map(|k| if k == 0 { energy[0] } else { (energy[k] - energy[k - 1]).max(0.0) })
        .collect();
    let peak = flux.iter().copied().fold(0.0f32, f32::max);
    let mut out = vec![0u32];
    if peak <= 0.0 {
        return out;
    }
    let threshold = peak * (0.05 + 0.6 * (1.0 - sensitivity.clamp(0.0, 1.0)));
    let min_gap = (0.05 * sample_rate as f32) as u32;
    for k in 1..flux.len() {
        let next = flux.get(k + 1).copied().unwrap_or(0.0);
        let is_peak = flux[k] >= threshold && flux[k] >= flux[k - 1] && flux[k] >= next;
        let frame = (k * hop) as u32;
        if is_peak && frame >= out[out.len() - 1] + min_gap && out.len() < MAX_SLICES {
            out.push(frame);
        }
    }
    out
}

/// A tempo for tempo lock: assumes the sample is a whole number of 4/4
/// bars (or a quarter or half bar) and picks the count that lands in
/// 90–180 BPM, else the one closest to that range.
pub fn guess_bpm(frames: u32, sample_rate: u32) -> f32 {
    let seconds = frames as f32 / sample_rate.max(1) as f32;
    if seconds <= 0.0 {
        return 120.0;
    }
    let distance = |b: f32| {
        if b < 90.0 {
            90.0 - b
        } else if b > 180.0 {
            b - 180.0
        } else {
            0.0
        }
    };
    let candidates = [0.25f32, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0].map(|bars| bars * 240.0 / seconds);
    candidates
        .iter()
        .copied()
        .find(|b| (90.0..=180.0).contains(b))
        .unwrap_or_else(|| {
            candidates
                .iter()
                .copied()
                .min_by(|a, b| distance(*a).total_cmp(&distance(*b)))
                .unwrap_or(120.0)
                .clamp(20.0, 400.0)
        })
}
```

The first click at t=0 shows up as `flux[0]`. That slot is already covered by the initial `0`, and the loop starts at k = 1. In `sensitivity_controls_quiet_hits` the quiet click has about 1/10 the flux. At sensitivity 1.0 the threshold is 0.05 × peak, so it is detected; at sensitivity 0.0 the threshold is 0.65 × peak, so it is not.

`guess_bpm(3 * SR, SR)` gives 0.25/0.5/1/2 bars → 20/40/80/160. 160 is the first in range.

- [ ] **Step 5: Add the modules and run everything**

Add `pub mod decode;` and `pub mod slicing;` to `sampler/mod.rs`.

Run: `cargo test -p acid-engine sampler:: && cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings. If a slicing test misses by a little because of the click shape, adjust only the test's click envelope, not the algorithm's constants, and say so in the report.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(sampler): decode audio files, slice by transients or equally, guess tempo

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Loading, clearing and editing slots in the app

**Files:**
- Modify: `crates/app/src/document.rs`, `crates/app/src/ui/mod.rs` (module, `ViewState`, tests), `crates/app/src/ui/files.rs`
- Create: `crates/app/src/ui/sampler_load.rs`

**Interfaces:**
- Consumes: `SlotData`, `SlotMeta`, `SlotPlay`, `SamplerMachine` helpers (Task 2); `decode`, `slicing` (Task 6); `Command::SamplerSample`/`SamplerPlay` (Task 5); `SlotParam` (Task 1).
- Produces:

```rust
// document.rs
Edit::SamplerSlot { slot: usize, data: Option<Box<SlotData>>, values: Vec<(ParamId, f32)> }   // load / clear: one undo step
Edit::SlotMeta { slot: usize, meta: Box<SlotMeta> }                                         // marker drags merge
// ui/sampler_load.rs
impl SidAcidApp {
    pub(crate) fn load_sample_dialog(&mut self, slot: usize);
    pub(crate) fn load_sample_file(&mut self, slot: usize, path: &std::path::Path);
    pub(crate) fn load_sample_bytes(&mut self, slot: usize, name: &str, extension: Option<&str>, bytes: &[u8]);
    pub(crate) fn clear_slot(&mut self, slot: usize);
    /// Applies `change` to a copy of the slot's meta; refuses zone clashes and malformed metas with a message.
    pub(crate) fn edit_slot_meta(&mut self, slot: usize, merge: bool, change: impl FnOnce(&mut SlotMeta)) -> bool;
}
// ViewState
pub sampler_slot: usize,      // selected slot (0..16)
```

`commands_for`:
- `SamplerSlot` produces `SamplerSample` (`Arc` built from the slot's PCM, or `None`), then `SamplerPlay` (from the meta, or `EMPTY`), then one `SetParam` per value.
- `SlotMeta` produces `SamplerPlay` only, so marker drags don't rebuild the audio.

- [ ] **Step 1: Write the failing tests**

In `document.rs` tests:

```rust
    #[test]
    fn sampler_slot_edits_undo_and_sync_the_engine() {
        use acid_engine::params::SlotParam;
        use acid_engine::sampler::slot::{SlotData, SlotMeta};
        let mut d = doc();
        let tone: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        let data = SlotData::new("Hit", 22_050, 1, &tone, SlotMeta::sliced(24, vec![0, 500], 120.0, 1000));
        let choke = ParamId::SamplerSlot(2, SlotParam::Choke);
        let cmds = d.apply(
            Edit::SamplerSlot {
                slot: 2,
                data: Some(Box::new(data.clone())),
                values: vec![(choke, 1.0)],
            },
            false,
        );
        assert_eq!(d.project.sampler.slots[2], Some(data));
        assert!(matches!(cmds[0], Command::SamplerSample { slot: 2, sample: Some(_) }));
        assert!(matches!(cmds[1], Command::SamplerPlay { slot: 2, .. }));
        assert!(matches!(cmds[2], Command::SetParam(id, v) if id == choke && v == 1.0));
        let mut meta = d.project.sampler.slots[2].as_ref().unwrap().meta.clone();
        meta.slices = vec![0, 300];
        let cmds = d.apply(Edit::SlotMeta { slot: 2, meta: Box::new(meta) }, false);
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], Command::SamplerPlay { slot: 2, .. }));
        d.undo();
        assert_eq!(d.project.sampler.slots[2].as_ref().unwrap().meta.slices, vec![0, 500]);
        d.undo();
        assert_eq!(d.project.sampler.slots[2], None);
        assert_eq!(d.project.param(choke), 0.0);
    }
```

In `ui/mod.rs` tests (headless setup as the neighbours use):

```rust
    fn test_wav(seconds: f32) -> Vec<u8> {
        let rate = 22_050;
        let mut cur = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::new(&mut cur, spec).unwrap();
        for i in 0..(seconds * rate as f32) as usize {
            let click = if i % 5512 < 200 { 12_000.0 } else { 0.0 };
            w.write_sample(((i as f32 * 0.3).sin() * click) as i16).unwrap();
        }
        w.finalize().unwrap();
        cur.into_inner()
    }

    #[test]
    fn loading_samples_picks_modes_zones_and_params() {
        use acid_engine::params::{ParamId, SlotParam};
        use acid_engine::sampler::slot::SlotMode;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.load_sample_bytes(0, "Break", Some("wav"), &test_wav(2.0));
        let slot = app.doc.project.sampler.slots[0].as_ref().expect("loaded");
        assert_eq!(slot.meta.mode, SlotMode::Slice);
        assert_eq!(slot.meta.base_key, 24);
        assert!(slot.meta.slices.len() >= 2);
        assert_eq!(slot.meta.original_bpm, 120.0);
        assert_eq!(app.doc.project.param(ParamId::SamplerSlot(0, SlotParam::Choke)), 1.0);
        assert_eq!(app.doc.project.param(ParamId::SamplerSlot(0, SlotParam::TempoLock)), 1.0);
        let span = slot.meta.slices.len() as u8;

        app.load_sample_bytes(1, "Stab", Some("wav"), &test_wav(0.5));
        let stab = app.doc.project.sampler.slots[1].as_ref().unwrap();
        assert_eq!(stab.meta.mode, SlotMode::Chromatic);
        assert_eq!(stab.meta.base_key, 24 + span, "placed above the break");
        assert_eq!(app.doc.project.param(ParamId::SamplerSlot(1, SlotParam::Choke)), 0.0);

        app.doc.undo();
        assert!(app.doc.project.sampler.slots[1].is_none(), "a load is one undo step");

        let before = app.doc.project.clone();
        app.load_sample_bytes(2, "Bad", Some("wav"), b"junk");
        assert!(app.view.message.is_some());
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn meta_edits_refuse_zone_clashes() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.load_sample_bytes(0, "A", Some("wav"), &test_wav(0.5));
        app.load_sample_bytes(1, "B", Some("wav"), &test_wav(0.5));
        let b_base = app.doc.project.sampler.slots[1].as_ref().unwrap().meta.base_key;
        assert!(!app.edit_slot_meta(1, false, |m| m.base_key = 30));
        assert!(app.view.message.is_some());
        assert_eq!(app.doc.project.sampler.slots[1].as_ref().unwrap().meta.base_key, b_base);
        app.view.message = None;
        assert!(app.edit_slot_meta(1, false, |m| {
            m.base_key = 100;
            m.high_key = 110;
        }));
        app.clear_slot(0);
        assert!(app.doc.project.sampler.slots[0].is_none());
    }
```

The app crate's dev-dependencies need `hound`. Add `[dev-dependencies] hound.workspace = true` to `crates/app/Cargo.toml`.

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p sidacid`
Expected: compile errors.

- [ ] **Step 3: Implement the edits** in `document.rs`

Imports: `acid_engine::sampler::slot::{SlotData, SlotMeta, SlotPlay}` and `std::sync::Arc`. Add these `Edit` variants:

```rust
    /// Loading or clearing a sampler slot, with its knob values: one undo step.
    SamplerSlot {
        slot: usize,
        data: Option<Box<SlotData>>,
        values: Vec<(ParamId, f32)>,
    },
    /// Changing a loaded slot's zone, slices, mode or markers.
    SlotMeta {
        slot: usize,
        meta: Box<SlotMeta>,
    },
```

Add `MergeKey::SamplerSlot(usize)` and `MergeKey::SlotMeta(usize)` with matching `merge_key` arms. Add these `apply_raw` arms:

```rust
            Edit::SamplerSlot { slot, data, values } => {
                let old = std::mem::replace(&mut p.sampler.slots[slot], data.map(|d| *d));
                let old_values = values
                    .into_iter()
                    .map(|(id, v)| {
                        let before = p.param(id);
                        p.params.insert(id, v.clamp(0.0, 1.0));
                        (id, before)
                    })
                    .collect();
                Edit::SamplerSlot {
                    slot,
                    data: old.map(Box::new),
                    values: old_values,
                }
            }
            Edit::SlotMeta { slot, meta } => match p.sampler.slots[slot].as_mut() {
                Some(d) => Edit::SlotMeta {
                    slot,
                    meta: Box::new(std::mem::replace(&mut d.meta, *meta)),
                },
                None => Edit::SlotMeta { slot, meta },
            },
```

Add these `commands_for` arms:

```rust
            Edit::SamplerSlot { slot, values, .. } => {
                let data = p.sampler.slots[*slot].as_ref();
                let mut cmds = vec![
                    Command::SamplerSample {
                        slot: *slot as u8,
                        sample: data.map(|d| Arc::new(d.to_sample())),
                    },
                    Command::SamplerPlay {
                        slot: *slot as u8,
                        play: Box::new(data.map_or(SlotPlay::EMPTY, |d| SlotPlay::from_meta(&d.meta))),
                    },
                ];
                cmds.extend(values.iter().map(|(id, _)| Command::SetParam(*id, p.param(*id))));
                cmds
            }
            Edit::SlotMeta { slot, .. } => vec![Command::SamplerPlay {
                slot: *slot as u8,
                play: Box::new(
                    p.sampler.slots[*slot]
                        .as_ref()
                        .map_or(SlotPlay::EMPTY, |d| SlotPlay::from_meta(&d.meta)),
                ),
            }],
```

The app's `send` coalesces only `SetParam` runs, so these commands pass through untouched.

- [ ] **Step 4: Implement `ui/sampler_load.rs`**

```rust
//! Loading, clearing and editing sampler slots. Files are decoded here,
//! on the UI thread; zones and the memory limit are checked before the
//! undoable edit is applied.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::params::{ParamId, SlotParam};
use acid_engine::sampler::decode::decode;
use acid_engine::sampler::slicing::{detect_slices, equal_slices, guess_bpm, mono_mix};
use acid_engine::sampler::slot::{SlotData, SlotMeta};
use acid_engine::sampler::{MEMORY_LIMIT, SLOTS};

/// Samples at least this long load as slice slots.
const SLICE_MIN_SECONDS: f32 = 1.0;
/// Keys a new chromatic zone covers.
const CHROMATIC_SPAN: u8 = 24;

impl SidAcidApp {
    pub(crate) fn load_sample_dialog(&mut self, slot: usize) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "flac", "mp3", "ogg", "WAV", "FLAC", "MP3", "OGG"])
            .pick_file()
        {
            self.load_sample_file(slot, &path);
        }
    }

    pub(crate) fn load_sample_file(&mut self, slot: usize, path: &std::path::Path) {
        match std::fs::read(path) {
            Ok(bytes) => {
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase);
                self.load_sample_bytes(slot, &name, ext.as_deref(), &bytes);
            }
            Err(e) => self.view.message = Some(format!("Could not read {}:\n{e}", path.display())),
        }
    }

    pub(crate) fn load_sample_bytes(&mut self, slot: usize, name: &str, extension: Option<&str>, bytes: &[u8]) {
        if slot >= SLOTS {
            return;
        }
        let decoded = match decode(bytes, extension) {
            Ok(d) => d,
            Err(e) => {
                self.view.message = Some(format!("Could not load the sample: {e}."));
                return;
            }
        };
        let sampler = &self.doc.project.sampler;
        let old = sampler.slots[slot].as_ref().map_or(0, |d| d.memory_bytes());
        if sampler.memory_bytes() - old + decoded.samples.len() * 4 > MEMORY_LIMIT {
            self.view.message = Some("Not enough sample memory left (64 MB in all). Clear a slot first.".into());
            return;
        }
        let frames = decoded.frames() as u32;
        let slice_mode = decoded.seconds() >= SLICE_MIN_SECONDS;
        let mut slices = Vec::new();
        if slice_mode {
            slices = detect_slices(&mono_mix(&decoded.samples, decoded.channels as usize), decoded.sample_rate, 0.5);
            if slices.len() < 2 {
                slices = equal_slices(frames, 8);
            }
        }
        let span = if slice_mode { slices.len() as u8 } else { CHROMATIC_SPAN };
        let Some(base) = sampler.free_base_key(slot, span) else {
            self.view.message = Some("No free keys left for this sample's zone. Move or clear another slot.".into());
            return;
        };
        let meta = if slice_mode {
            SlotMeta::sliced(base, slices, guess_bpm(frames, decoded.sample_rate), frames)
        } else {
            SlotMeta::chromatic(base, base + CHROMATIC_SPAN - 1, base, frames)
        };
        let short: String = name.chars().take(24).collect();
        let data = SlotData::new(&short, decoded.sample_rate, decoded.channels, &decoded.samples, meta);
        let values = SlotParam::ALL
            .iter()
            .map(|&p| {
                let id = ParamId::SamplerSlot(slot as u8, p);
                let v = match p {
                    SlotParam::Choke | SlotParam::TempoLock if slice_mode => 1.0,
                    _ => id.info().default,
                };
                (id, v)
            })
            .collect();
        let cmds = self.doc.apply(
            Edit::SamplerSlot {
                slot,
                data: Some(Box::new(data)),
                values,
            },
            false,
        );
        self.send(cmds);
        self.view.sampler_slot = slot;
    }

    pub(crate) fn clear_slot(&mut self, slot: usize) {
        if self.doc.project.sampler.slots.get(slot).is_some_and(Option::is_some) {
            let cmds = self.doc.apply(
                Edit::SamplerSlot {
                    slot,
                    data: None,
                    values: Vec::new(),
                },
                false,
            );
            self.send(cmds);
        }
    }

    pub(crate) fn edit_slot_meta(&mut self, slot: usize, merge: bool, change: impl FnOnce(&mut SlotMeta)) -> bool {
        let sampler = &self.doc.project.sampler;
        let Some(data) = sampler.slots.get(slot).and_then(Option::as_ref) else {
            return false;
        };
        let mut meta = data.meta.clone();
        change(&mut meta);
        if meta == data.meta {
            return false;
        }
        if !meta.is_well_formed(data.frames()) {
            return false;
        }
        if sampler.zone_clash(slot, &meta) {
            self.view.message = Some("That zone overlaps another slot's keys.".into());
            return false;
        }
        let cmds = self.doc.apply(
            Edit::SlotMeta {
                slot,
                meta: Box::new(meta),
            },
            merge,
        );
        self.send(cmds);
        true
    }
}
```

In `ui/mod.rs`, add `mod sampler_load;` and the `ViewState` field `pub sampler_slot: usize,` (doc comment "Selected sampler slot (0..16)."). In `files.rs` `load_document`, add `self.view.sampler_slot = 0;`.

In `loading_samples_picks_modes_zones_and_params`, the 2.0 s test WAV has clicks every 0.25 s (5512 frames), so detection finds several slices. `guess_bpm` of 2.0 s is 120.

- [ ] **Step 5: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings. If clippy reports dead code for `load_sample_dialog`/`load_sample_file` before the panel uses them in Task 9, add `#[allow(dead_code)] // used by the sampler panel (Task 9)` on those two methods only. Task 9 removes it.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(app): load, clear and edit sampler slots as undoable edits

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Waveform and marker helpers, and sampler rows in the roll

**Files:**
- Create: `crates/app/src/ui/sampler_ui.rs`
- Modify: `crates/app/src/ui/mod.rs` (`mod sampler_ui;`), `crates/app/src/ui/polyroll.rs`

**Interfaces:**
- Consumes: `SlotData`, `SlotMeta`, `SlotMode` (Task 2); `step_edit::NOTE_NAMES` (existing).
- Produces:

```rust
pub fn peaks(pcm: &[i16], channels: usize, columns: usize) -> Vec<(f32, f32)>;   // (min, max) per column, -1..1
pub fn frame_to_x(frame: u32, frames: u32, left: f32, width: f32) -> f32;
pub fn x_to_frame(x: f32, frames: u32, left: f32, width: f32) -> u32;             // clamped 0..=frames
pub fn nearest_marker(xs: &[f32], x: f32, tolerance: f32) -> Option<usize>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker { Slice(usize), Start, End, LoopStart, LoopEnd }
pub fn markers(meta: &SlotMeta) -> Vec<(Marker, u32)>;
pub fn set_marker(meta: &mut SlotMeta, marker: Marker, frame: u32, frames: u32) -> bool;  // keeps the meta well-formed
pub fn key_name(key: u8) -> String;                                                // 24 → "C1"
pub fn zone_text(meta: &SlotMeta) -> String;                                       // "Keys C1–D#2"
pub fn row_label(slots: &[Option<SlotData>], key: u8) -> Option<(usize, String)>; // (slot, "Amen ·05" | "Vox C3")
pub const SLOT_TINTS: [egui::Color32; 4];
```

- [ ] **Step 1: Write the failing tests** (bottom of `sampler_ui.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::sampler::slot::{SlotData, SlotMeta};

    #[test]
    fn peaks_cover_each_column() {
        let pcm: Vec<i16> = (0..1000).map(|i| if i < 500 { 16_384 } else { -32_768 }).collect();
        let p = peaks(&pcm, 1, 10);
        assert_eq!(p.len(), 10);
        assert_eq!(p[0], (0.5, 0.5));
        assert_eq!(p[9], (-1.0, -1.0));
        assert!(peaks(&[], 1, 10).iter().all(|c| *c == (0.0, 0.0)));
    }

    #[test]
    fn frames_and_pixels_round_trip() {
        assert_eq!(frame_to_x(500, 1000, 10.0, 200.0), 110.0);
        assert_eq!(x_to_frame(110.0, 1000, 10.0, 200.0), 500);
        assert_eq!(x_to_frame(-50.0, 1000, 10.0, 200.0), 0);
        assert_eq!(x_to_frame(9999.0, 1000, 10.0, 200.0), 1000);
        assert_eq!(nearest_marker(&[10.0, 50.0, 52.0], 51.5, 4.0), Some(2));
        assert_eq!(nearest_marker(&[10.0], 30.0, 4.0), None);
    }

    #[test]
    fn markers_move_within_their_neighbours() {
        let mut m = SlotMeta::sliced(36, vec![0, 100, 200, 300], 120.0, 1000);
        assert!(set_marker(&mut m, Marker::Slice(2), 150, 1000));
        assert_eq!(m.slices, vec![0, 100, 150, 300]);
        assert!(set_marker(&mut m, Marker::Slice(2), 50, 1000));
        assert_eq!(m.slices[2], 101, "clamped above the previous slice");
        assert!(!set_marker(&mut m, Marker::Slice(0), 10, 1000), "slice 0 stays at the start");
        let mut c = SlotMeta::chromatic(48, 71, 60, 1000);
        c.loop_on = true;
        assert!(set_marker(&mut c, Marker::Start, 200, 1000));
        assert!(!set_marker(&mut c, Marker::LoopStart, 100, 1000), "already clamped to the start");
        assert_eq!(c.loop_start, 200, "the loop stays inside the play range");
        assert!(set_marker(&mut c, Marker::End, 900, 1000));
        assert_eq!(c.loop_end, 900, "loop end follows a shorter end");
        assert!(c.is_well_formed(1000));
        assert_eq!(markers(&c).len(), 4);
        assert_eq!(markers(&m).len(), 4);
    }

    #[test]
    fn names_zones_and_row_labels() {
        assert_eq!(key_name(24), "C1");
        assert_eq!(key_name(61), "C#4");
        let tone = vec![0.1f32; 1000];
        let mut slots: Vec<Option<SlotData>> = vec![None; 16];
        slots[0] = Some(SlotData::new("Amen Brother", 22_050, 1, &tone, SlotMeta::sliced(24, vec![0, 100, 200, 300, 400], 120.0, 1000)));
        slots[2] = Some(SlotData::new("Vox", 22_050, 1, &tone, SlotMeta::chromatic(48, 59, 48, 1000)));
        assert_eq!(zone_text(&slots[0].as_ref().unwrap().meta), "Keys C1–E1");
        assert_eq!(row_label(&slots, 28), Some((0, "Amen ·05".to_string())));
        assert_eq!(row_label(&slots, 52), Some((2, "Vox E3".to_string())));
        assert_eq!(row_label(&slots, 40), None);
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p sidacid sampler_ui`
Expected: compile errors.

- [ ] **Step 3: Implement `sampler_ui.rs`** (above the tests)

```rust
//! Pure helpers for the sampler panel and the roll: waveform peaks,
//! frame/pixel mapping, marker editing, key names and row labels.

use super::step_edit::NOTE_NAMES;
use acid_engine::sampler::slot::{SlotData, SlotMeta, SlotMode};
use eframe::egui::Color32;

/// Row tints for sampler zones in the roll, cycled by slot.
pub const SLOT_TINTS: [Color32; 4] = [
    Color32::from_rgb(0xB8, 0xF5, 0x3A),
    Color32::from_rgb(0x4C, 0xB8, 0xFF),
    Color32::from_rgb(0xFF, 0xC8, 0x3C),
    Color32::from_rgb(0xFF, 0x50, 0x3C),
];

/// (min, max) of the samples in each of `columns` equal spans, -1..1.
pub fn peaks(pcm: &[i16], channels: usize, columns: usize) -> Vec<(f32, f32)> {
    let ch = channels.max(1);
    let frames = pcm.len() / ch;
    (0..columns)
        .map(|c| {
            let a = c * frames / columns.max(1);
            let b = ((c + 1) * frames / columns.max(1)).max(a + 1).min(frames);
            if a >= frames {
                return (0.0, 0.0);
            }
            let span = &pcm[a * ch..b * ch];
            let lo = span.iter().copied().min().unwrap_or(0) as f32 / 32_768.0;
            let hi = span.iter().copied().max().unwrap_or(0) as f32 / 32_768.0;
            (lo, hi)
        })
        .collect()
}

pub fn frame_to_x(frame: u32, frames: u32, left: f32, width: f32) -> f32 {
    left + frame as f32 / frames.max(1) as f32 * width
}

pub fn x_to_frame(x: f32, frames: u32, left: f32, width: f32) -> u32 {
    (((x - left) / width.max(1.0)).clamp(0.0, 1.0) * frames as f32).round() as u32
}

/// The marker nearest `x` within `tolerance` pixels.
pub fn nearest_marker(xs: &[f32], x: f32, tolerance: f32) -> Option<usize> {
    xs.iter()
        .enumerate()
        .map(|(i, m)| (i, (m - x).abs()))
        .filter(|(_, d)| *d <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Slice(usize),
    Start,
    End,
    LoopStart,
    LoopEnd,
}

/// The draggable markers of a slot and their frames.
pub fn markers(meta: &SlotMeta) -> Vec<(Marker, u32)> {
    match meta.mode {
        SlotMode::Slice => meta.slices.iter().enumerate().map(|(i, &f)| (Marker::Slice(i), f)).collect(),
        SlotMode::Chromatic => {
            let mut v = vec![(Marker::Start, meta.start), (Marker::End, meta.end)];
            if meta.loop_on {
                v.push((Marker::LoopStart, meta.loop_start));
                v.push((Marker::LoopEnd, meta.loop_end));
            }
            v
        }
    }
}

/// Moves a marker, clamping it so the meta stays well-formed. Returns
/// whether anything changed. Slice 0 never moves.
pub fn set_marker(meta: &mut SlotMeta, marker: Marker, frame: u32, frames: u32) -> bool {
    let before = meta.clone();
    match marker {
        Marker::Slice(0) => return false,
        Marker::Slice(i) => {
            if i >= meta.slices.len() {
                return false;
            }
            let lo = meta.slices[i - 1] + 1;
            let hi = meta.slices.get(i + 1).map_or(frames.saturating_sub(1), |&n| n - 1);
            if lo > hi {
                return false;
            }
            meta.slices[i] = frame.clamp(lo, hi);
        }
        Marker::Start => {
            meta.start = frame.min(meta.end.saturating_sub(2));
            meta.loop_start = meta.loop_start.max(meta.start);
            meta.loop_end = meta.loop_end.max(meta.loop_start + 1);
        }
        Marker::End => {
            meta.end = frame.clamp(meta.start + 2, frames);
            meta.loop_end = meta.loop_end.min(meta.end);
            meta.loop_start = meta.loop_start.min(meta.loop_end - 1);
        }
        Marker::LoopStart => meta.loop_start = frame.clamp(meta.start, meta.loop_end - 1),
        Marker::LoopEnd => meta.loop_end = frame.clamp(meta.loop_start + 1, meta.end),
    }
    *meta != before
}

/// "C1" for MIDI 24.
pub fn key_name(key: u8) -> String {
    format!("{}{}", NOTE_NAMES[key as usize % 12], key as i32 / 12 - 1)
}

/// "Keys C1–D#2".
pub fn zone_text(meta: &SlotMeta) -> String {
    let (lo, hi) = meta.key_range();
    format!("Keys {}–{}", key_name(lo), key_name(hi.min(127)))
}

/// The roll's label for a sampler row: slot index and "Name ·05"
/// (slice 5) or "Name C3" (chromatic key).
pub fn row_label(slots: &[Option<SlotData>], key: u8) -> Option<(usize, String)> {
    slots.iter().enumerate().find_map(|(i, s)| {
        let d = s.as_ref()?;
        let (lo, hi) = d.meta.key_range();
        if key < lo || key > hi {
            return None;
        }
        let short: String = d.name.chars().take(4).collect::<String>().trim_end().to_string();
        Some((
            i,
            match d.meta.mode {
                SlotMode::Slice => format!("{short} ·{:02}", key - lo + 1),
                SlotMode::Chromatic => format!("{short} {}", key_name(key)),
            },
        ))
    })
}
```

**Notes on the code and tests:**
- In `markers_move_within_their_neighbours`, setting `LoopStart` to 100 after `Start` moved to 200 clamps it to 200. `End` → 900 pulls `loop_end` from 1000 to 900.
- If `NOTE_NAMES` in `step_edit.rs` is private, make it `pub(crate)`.
- `NOTE_NAMES` uses "C#"-style names. If it uses another spelling (e.g. "C♯"), adapt the two `key_name` expectations to that spelling and say so in the report.

- [ ] **Step 4: Label and tint sampler rows in the roll**

In `polyroll.rs`'s row loop, replace the `if pitch % 12 == 0 { … "C{}" … }` label block with:

```rust
                        let sampler_row = if machine == MachineId::Sampler.index() {
                            super::sampler_ui::row_label(&self.doc.project.sampler.slots, pitch)
                        } else {
                            None
                        };
                        if let Some((slot, text)) = &sampler_row {
                            painter.rect_filled(
                                row,
                                0.0,
                                super::sampler_ui::SLOT_TINTS[slot % 4].gamma_multiply(0.18),
                            );
                            painter.text(
                                pos2(rect.left() + 1.0, y0 + ROW_H / 2.0),
                                Align2::LEFT_CENTER,
                                text,
                                FontId::monospace(6.5),
                                Color32::BLACK,
                            );
                        } else if pitch % 12 == 0 {
                            painter.text(
                                pos2(rect.left() + 2.0, y0 + ROW_H / 2.0),
                                Align2::LEFT_CENTER,
                                format!("C{}", pitch as i32 / 12 - 1),
                                FontId::monospace(8.0),
                                Color32::BLACK,
                            );
                        }
```

The tint is drawn after the row background and before the key rectangle, so place this block between them. Check that `row` (the row rect) is still in scope there.

- [ ] **Step 5: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings. If clippy reports `sampler_ui` items unused before Task 9, add `#![allow(dead_code)] // used by the sampler panel (Task 9)` at the top of `sampler_ui.rs`; Task 9 removes it.

- [ ] **Step 6: Commit**

```bash
git add -A crates
git commit -m "feat(app): waveform/marker helpers and labelled sampler rows in the roll

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: The sampler panel

**Files:**
- Create: `crates/app/src/ui/sampler_panel.rs`
- Modify: `crates/app/src/ui/note_panel.rs` (remove `note_panel`, keep `note_pattern_row`), `crates/app/src/ui/mod.rs` (module, rack, `ViewState`, tests), `crates/app/src/ui/files.rs`, `crates/app/src/ui/sampler_ui.rs` and `sampler_load.rs` (remove dead-code allows), `README.md`

**Interfaces:**
- Consumes: everything from Tasks 7 and 8; `param_knob`, `note_pattern_row`, `poly_roll`, `send`.
- Produces: `SidAcidApp::sampler_panel(&mut self, ui: &mut egui::Ui)`.
- New `ViewState` fields:

```rust
    /// Cached waveform peaks per slot: (pcm pointer, pcm length, columns, peaks).
    pub wave_cache: std::collections::HashMap<usize, (usize, usize, usize, Vec<(f32, f32)>)>,
    /// Marker being dragged in the waveform: (slot, marker).
    pub wave_drag: Option<(usize, sampler_ui::Marker)>,
    /// Transient sensitivity for "Detect" (None = 0.5).
    pub sampler_sensitivity: Option<f32>,
    /// Key held by a shift-click on a slot pad.
    pub pad_note: Option<u8>,
```

**Panel layout,** top to bottom:
- **Header:** "SAMPLER   A01".
- **Global row:** Volume, Grit.
- **Pads:** 16 slot pads in 2 rows of 8, each "3 Amen" or "3 ·". Click selects; shift-click auditions, held while the mouse button is down.
- **Selected slot row:** name, Load…, Clear, the Slice | Chromatic mode switch, the zone base key (DragValue) and the zone readout.
  - In slice mode: Equal 4/8/16/32, Detect with a sensitivity slider, and Orig BPM.
  - In chromatic mode: root key, top key and a Loop checkbox.
- **Waveform:** 100 px high with draggable markers. Slice markers are ACID, start/end are ACCENT, loop markers are SLIDE.
- **Selected slot's knobs:** Level, Pan, Tune, Fine, Cutoff, Reso, Attack, Decay, Sustain, Release, Reverse, Choke, Tempo.
- **Pattern row and poly roll.**
- **Drag and drop:** an audio file dropped while the pointer is over the panel loads into the selected slot.

- [ ] **Step 1: Write the failing tests** (in `ui/mod.rs` tests; reuse `test_wav` from Task 7)

```rust
    #[test]
    fn sampler_panel_draws_empty_and_loaded_slots_in_both_modes() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        run_frames(&mut app, 1);
        app.load_sample_bytes(0, "Break", Some("wav"), &test_wav(2.0));
        app.load_sample_bytes(1, "Stab", Some("wav"), &test_wav(0.5));
        for slot in [0, 1, 7] {
            app.view.sampler_slot = slot;
            run_frames(&mut app, 2);
        }
        assert!(app.view.wave_cache.contains_key(&0) && app.view.wave_cache.contains_key(&1));
        assert!(app.view.poly_scrolled[1], "the sampler roll is drawn");
    }
```

- [ ] **Step 2: Run it to make sure it fails**

Run: `cargo test -p sidacid sampler_panel`
Expected: compile errors.

- [ ] **Step 3: Implement `sampler_panel.rs`**

```rust
//! The sampler panel: global knobs, 16 slot pads, the selected slot's
//! controls and waveform with draggable markers, its knobs, and the
//! pattern row with the poly roll. Audio files can be dropped on it.

use super::sampler_ui::{Marker, frame_to_x, key_name, markers, nearest_marker, peaks, set_marker, x_to_frame, zone_text};
use super::{SidAcidApp, theme};
use acid_engine::host::Command;
use acid_engine::machine::MachineId;
use acid_engine::params::{ParamId, SamplerParam, SlotParam};
use acid_engine::project::PatternRef;
use acid_engine::sampler::SLOTS;
use acid_engine::sampler::slicing::{detect_slices, equal_slices, mono_mix};
use acid_engine::sampler::slot::SlotMode;
use eframe::egui::{self, Sense, Stroke, pos2, vec2};

const SLOT_KNOBS: [(SlotParam, &str); 13] = [
    (SlotParam::Level, "Level"),
    (SlotParam::Pan, "Pan"),
    (SlotParam::Tune, "Tune"),
    (SlotParam::Fine, "Fine"),
    (SlotParam::Cutoff, "Cutoff"),
    (SlotParam::Resonance, "Reso"),
    (SlotParam::Attack, "Attack"),
    (SlotParam::Decay, "Decay"),
    (SlotParam::Sustain, "Sustain"),
    (SlotParam::Release, "Release"),
    (SlotParam::Reverse, "Reverse"),
    (SlotParam::Choke, "Choke"),
    (SlotParam::TempoLock, "Tempo"),
];
const WAVE_H: f32 = 100.0;
const MARKER_GRAB_PX: f32 = 5.0;

impl SidAcidApp {
    pub(crate) fn sampler_panel(&mut self, ui: &mut egui::Ui) {
        let m = MachineId::Sampler.index();
        let slot = self.doc.project.selected[m];
        let title = format!("SAMPLER   {}", PatternRef(slot).label());
        let resp = egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt("sampler")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    self.param_knob(ui, ParamId::Sampler(SamplerParam::Volume), "Volume");
                    self.param_knob(ui, ParamId::Sampler(SamplerParam::Grit), "Grit");
                });
                self.sampler_pads(ui);
                self.sampler_slot_row(ui);
                self.sampler_waveform(ui);
                let s = self.view.sampler_slot.min(SLOTS - 1) as u8;
                ui.horizontal_wrapped(|ui| {
                    for (p, label) in SLOT_KNOBS {
                        self.param_knob(ui, ParamId::SamplerSlot(s, p), label);
                    }
                });
                self.note_pattern_row(ui, m);
                self.poly_roll(ui, m);
            });
        self.sampler_drop(ui, resp.header_response.rect.union(resp.body_response.map_or(egui::Rect::NOTHING, |b| b.rect)));
        self.release_pad_note(ui);
    }

    fn sampler_pads(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("sampler_pads").spacing([3.0, 3.0]).show(ui, |ui| {
            for i in 0..SLOTS {
                let name = self.doc.project.sampler.slots[i]
                    .as_ref()
                    .map_or("·".to_string(), |d| d.name.chars().take(6).collect());
                let r = ui.add_sized(
                    [64.0, 22.0],
                    egui::Button::new(format!("{} {}", i + 1, name)).selected(self.view.sampler_slot == i),
                );
                if r.clicked() {
                    self.view.sampler_slot = i;
                }
                let shift = ui.input(|inp| inp.modifiers.shift);
                if shift && r.is_pointer_button_down_on() && self.view.pad_note.is_none()
                    && let Some(d) = self.doc.project.sampler.slots[i].as_ref()
                {
                    let key = match d.meta.mode {
                        SlotMode::Slice => d.meta.base_key,
                        SlotMode::Chromatic => d.meta.root,
                    };
                    self.send(vec![Command::NoteOn { machine: MachineId::Sampler.index() as u8, pitch: key, vel: 110 }]);
                    self.view.pad_note = Some(key);
                }
                if i % 8 == 7 {
                    ui.end_row();
                }
            }
        });
    }

    fn release_pad_note(&mut self, ui: &egui::Ui) {
        if let Some(key) = self.view.pad_note
            && !ui.input(|i| i.pointer.primary_down())
        {
            self.send(vec![Command::NoteOff { machine: MachineId::Sampler.index() as u8, pitch: key }]);
            self.view.pad_note = None;
        }
    }

    fn sampler_slot_row(&mut self, ui: &mut egui::Ui) {
        let s = self.view.sampler_slot.min(SLOTS - 1);
        let data = self.doc.project.sampler.slots[s].clone();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("Slot {}", s + 1)).strong());
            if ui.button("Load…").clicked() {
                self.load_sample_dialog(s);
            }
            let Some(d) = data else {
                ui.weak("Empty — load or drop an audio file (WAV, FLAC, MP3, OGG).");
                return;
            };
            ui.label(&d.name);
            if ui.button("Clear").clicked() {
                self.clear_slot(s);
                return;
            }
            ui.separator();
            for (mode, label) in [(SlotMode::Slice, "Slice"), (SlotMode::Chromatic, "Chromatic")] {
                if ui.selectable_label(d.meta.mode == mode, label).clicked() && d.meta.mode != mode {
                    self.edit_slot_meta(s, false, |m| {
                        m.mode = mode;
                        if mode == SlotMode::Chromatic {
                            m.high_key = m.base_key.saturating_add(23).min(127);
                            m.root = m.base_key;
                        }
                    });
                }
            }
            let mut base = d.meta.base_key;
            let r = ui.add(egui::DragValue::new(&mut base).range(0..=127).prefix("Zone "));
            if r.changed() {
                let shift = base as i16 - d.meta.base_key as i16;
                self.edit_slot_meta(s, r.dragged(), |m| {
                    m.base_key = base;
                    m.high_key = (m.high_key as i16 + shift).clamp(base as i16, 127) as u8;
                    m.root = (m.root as i16 + shift).clamp(0, 127) as u8;
                });
            }
            if r.drag_stopped() || r.lost_focus() {
                self.doc.end_gesture();
            }
            ui.label(zone_text(&d.meta));
            ui.separator();
            match d.meta.mode {
                SlotMode::Slice => {
                    for n in [4usize, 8, 16, 32] {
                        if ui.button(format!("÷{n}")).clicked() {
                            let frames = d.frames();
                            self.edit_slot_meta(s, false, |m| m.slices = equal_slices(frames, n));
                        }
                    }
                    let mut sens = self.view.sampler_sensitivity.unwrap_or(0.5);
                    if ui.add(egui::Slider::new(&mut sens, 0.0..=1.0).text("Sens")).changed() {
                        self.view.sampler_sensitivity = Some(sens);
                    }
                    if ui.button("Detect").clicked() {
                        let mono = mono_mix(
                            &d.pcm.iter().map(|&x| x as f32 / 32_768.0).collect::<Vec<_>>(),
                            d.channels as usize,
                        );
                        let slices = detect_slices(&mono, d.sample_rate, sens);
                        self.edit_slot_meta(s, false, |m| m.slices = slices);
                    }
                    let mut bpm = d.meta.original_bpm;
                    let r = ui.add(egui::DragValue::new(&mut bpm).range(20.0..=400.0).speed(0.1).prefix("Orig BPM "));
                    if r.changed() {
                        self.edit_slot_meta(s, r.dragged(), |m| m.original_bpm = bpm);
                    }
                    if r.drag_stopped() || r.lost_focus() {
                        self.doc.end_gesture();
                    }
                }
                SlotMode::Chromatic => {
                    let mut root = d.meta.root;
                    let r = ui.add(egui::DragValue::new(&mut root).range(0..=127).custom_formatter(|v, _| key_name(v as u8)).prefix("Root "));
                    if r.changed() {
                        self.edit_slot_meta(s, r.dragged(), |m| m.root = root);
                    }
                    let mut top = d.meta.high_key;
                    let r2 = ui.add(egui::DragValue::new(&mut top).range(d.meta.base_key as u32..=127).custom_formatter(|v, _| key_name(v as u8)).prefix("Top "));
                    if r2.changed() {
                        self.edit_slot_meta(s, r2.dragged(), |m| m.high_key = top);
                    }
                    if r.drag_stopped() || r.lost_focus() || r2.drag_stopped() || r2.lost_focus() {
                        self.doc.end_gesture();
                    }
                    let mut looping = d.meta.loop_on;
                    if ui.checkbox(&mut looping, "Loop").changed() {
                        self.edit_slot_meta(s, false, |m| m.loop_on = looping);
                    }
                }
            }
        });
    }

    fn sampler_waveform(&mut self, ui: &mut egui::Ui) {
        let s = self.view.sampler_slot.min(SLOTS - 1);
        let Some(d) = self.doc.project.sampler.slots[s].as_ref() else {
            return;
        };
        let width = ui.available_width().clamp(200.0, 900.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, WAVE_H), Sense::click_and_drag());
        let columns = width as usize;
        let key = (d.pcm.as_ptr() as usize, d.pcm.len(), columns);
        let fresh = self.view.wave_cache.get(&s).is_some_and(|c| (c.0, c.1, c.2) == key);
        if !fresh {
            let p = peaks(&d.pcm, d.channels as usize, columns);
            self.view.wave_cache.insert(s, (key.0, key.1, key.2, p));
        }
        let Some(d) = self.doc.project.sampler.slots[s].as_ref() else {
            return;
        };
        let frames = d.frames();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, theme::GRID_DARK);
        let mid = rect.center().y;
        let half = rect.height() * 0.45;
        if let Some((_, _, _, cols)) = self.view.wave_cache.get(&s) {
            for (i, (lo, hi)) in cols.iter().enumerate() {
                let x = rect.left() + i as f32 + 0.5;
                painter.line_segment([pos2(x, mid - hi * half), pos2(x, mid - lo * half)], Stroke::new(1.0_f32, theme::KNOB_TRACK));
            }
        }
        let marks = markers(&d.meta);
        let xs: Vec<f32> = marks.iter().map(|(_, f)| frame_to_x(*f, frames, rect.left(), rect.width())).collect();
        for ((m, _), x) in marks.iter().zip(&xs) {
            let colour = match m {
                Marker::Slice(_) => theme::ACID,
                Marker::Start | Marker::End => theme::ACCENT,
                Marker::LoopStart | Marker::LoopEnd => theme::SLIDE,
            };
            painter.line_segment([pos2(*x, rect.top()), pos2(*x, rect.bottom())], Stroke::new(1.5_f32, colour));
        }
        let pointer = resp.interact_pointer_pos();
        if resp.drag_started()
            && let Some(p) = pointer
            && let Some(i) = nearest_marker(&xs, p.x, MARKER_GRAB_PX)
        {
            self.view.wave_drag = Some((s, marks[i].0));
        }
        if resp.dragged()
            && let Some(p) = pointer
            && let Some((ds, marker)) = self.view.wave_drag
            && ds == s
        {
            let frame = x_to_frame(p.x, frames, rect.left(), rect.width());
            self.edit_slot_meta(s, true, |m| {
                set_marker(m, marker, frame, frames);
            });
        }
        if resp.drag_stopped() {
            self.view.wave_drag = None;
            self.doc.end_gesture();
        }
    }

    fn sampler_drop(&mut self, ui: &egui::Ui, panel: egui::Rect) {
        let dropped = ui.ctx().input(|i| i.raw.dropped_files.clone());
        let over = ui.ctx().input(|i| i.pointer.hover_pos()).is_some_and(|p| panel.contains(p));
        let Some(file) = dropped.into_iter().next() else {
            return;
        };
        if !over {
            return;
        }
        let slot = self.view.sampler_slot.min(SLOTS - 1);
        if let Some(path) = &file.path {
            self.load_sample_file(slot, path);
        } else if let Some(bytes) = &file.bytes {
            let ext = std::path::Path::new(&file.name).extension().and_then(|e| e.to_str()).map(str::to_lowercase);
            let stem = std::path::Path::new(&file.name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            self.load_sample_bytes(slot, &stem, ext.as_deref(), bytes);
        }
    }
}
```

**Notes:**
- The waveform code re-borrows `d` after updating the cache. That avoids holding a borrow of `self.doc` across the `self.view.wave_cache` mutation.
- If egui 0.31's `DragValue::custom_formatter` signature differs, adapt it to the version's signature and say so.
- If `CollapsingResponse` field names differ (`header_response`, `body_response`), compute the panel rect another way, for example `ui.min_rect()` captured around the header. Say so in the report.
- `Detect` converts the PCM to f32 once per click, which is fine on the UI thread.

- [ ] **Step 4: Wire it in**

- `note_panel.rs`: delete `note_panel` (the Sampler placeholder panel) and keep `note_pattern_row`. Change the module doc to "The note-pattern row (pads, length, snap) shared by the FM and Sampler panels." Remove imports that become unused.
- `ui/mod.rs`:
  - add `mod sampler_panel;` (alphabetical)
  - add the four `ViewState` fields from Interfaces
  - in `rack`, replace `self.note_panel(ui, acid_engine::machine::MachineId::Sampler);` with `self.sampler_panel(ui);`
- `files.rs` `load_document`: add `self.view.wave_cache.clear(); self.view.wave_drag = None; self.view.pad_note = None;`.
- Remove the temporary `#[allow(dead_code)]`/`#![allow(dead_code)]` lines added in Tasks 7 and 8.

- [ ] **Step 5: README**

In `README.md`, replace the "Sampler slot (placeholder…)" bullet with:

```markdown
- **Sampler** with 16 slots:
  - **Slice mode** for breaks: chop into 4/8/16/32 equal slices or by detected transients, drag the slice markers, and use choke and tempo lock.
  - **Chromatic mode** for voices and stabs: root key, start/end, and a crossfaded loop.
  - Per slot: level, pan, tune, filter, ADSR and reverse. Grit adds SP-1200-style 12-bit, 26 kHz crunch.
  - Load WAV, FLAC, MP3 or OGG with Load… or by dragging the file onto the panel. Audio is saved inside the project file.
  - The piano roll labels each sampler row with its slot and slice.
```

- [ ] **Step 6: Run everything, then check by hand**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: all pass, 0 warnings.

Then `cargo run --release -p sidacid` and check:
- loading a drum break (Load… or drag and drop) gives slices on the waveform
- dragging a slice marker moves it
- notes on the labelled rows in the sampler roll play the slices in time with the song tempo, with Tempo on
- a short sample loads as chromatic and plays up the keyboard
- Choke cuts overlapping slices
- Grit crunches
- save, reopen, and the sample is still there

If no display or audio is available, say so in the report.

- [ ] **Step 7: Commit**

```bash
git add -A crates README.md
git commit -m "feat(app): sampler panel with slot pads, waveform markers and drag-and-drop

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Spec coverage (Plan 6 scope)

| Spec item (section 3 / section 4 sampler) | Task |
|---|---|
| 16 slots, each owning a non-overlapping key zone; auto-assign from C1; overlap refused | 2, 7 |
| Slice: equal 4/8/16/32 or transient detect (sensitivity, cap 64); draggable markers | 6, 8, 9 |
| Slice plays to next marker; note-off → release; choke on by default; tempo lock with guessed BPM | 3, 4, 6, 7 |
| Chromatic: root, ADSR, start/end, loop with 10 ms crossfade | 3, 8, 9 |
| Per-slot params (Level, Pan, Tune, Fine, Cutoff, Reso, Attack, Decay, Sustain, Release, Reverse, Choke, TempoLock), automatable | 1, 3 |
| Grit (≈26 kHz S&H + 12-bit) | 4 |
| 16-voice poly, cubic Hermite, stereo | 2, 3, 4 |
| symphonia decode WAV/FLAC/MP3/OGG; limits 30 s / 64 MB | 6, 7 |
| `Arc` sample handoff, disposal off the audio thread | 5 |
| Embedded 16-bit PCM, bit-exact round trip | 2 |
| Sampler panel: pads (select / audition), waveform + markers, Load + drag-and-drop, slice controls, knobs, Grit | 9 |
| Roll rows labelled "Amen ·05" / "Vox C3", tinted per zone | 8 |
| Tests: slicing, detection, zones/overlap, tempo-lock math, choke, reverse, PCM round trip, allocation-free | 2, 3, 4, 5, 6 |
