# SidAcid: Mono Synth — Design

Date: 2026-10-15
Status: approved in brainstorming, pending spec review

## Goal

Add a **Mono** machine: a monophonic virtual-analog synth inspired by the
classic early-80s single-VCO monosynths (one VCO, sub oscillator, noise,
24 dB resonant low-pass, one ADSR, LFO, portamento, a 3-mode
arpeggiator). It is sequenced with the existing poly piano roll and laid
out like the FM panel.

User-facing text (UI, README, preset names) never names a hardware product
or manufacturer. The machine is called "Mono".

## Decisions

| Question | Decision |
|---|---|
| Sequencing | The existing poly piano roll and `NotePattern`, played mono |
| Chords in the roll | They become the held keys: the arp plays them, and with the arp off the last note wins |
| Arpeggiator | Up / U&D / Down, Range 1–3 octaves, synced to song tempo (1/4 … 1/32, with triplets) |
| Sound | A new band-limited VA engine with a **Drift** knob (0 = rock stable) |
| Envelopes | One ADSR shared by the VCF and VCA, as on the original; the VCA can switch to Gate |
| Extras | Randomise button, starter presets, an in-machine chorus, velocity to VCF/VCA |
| Architecture | `MachineType::Mono` with its own `acid-engine/src/va/` module, following the FM pattern |

Out of scope: separate filter and amp envelopes, patch import, a step
sequencer mode, arp Hold (the roll holds notes already), poly mode, and an
LFO tempo sync.

## 1. Engine (`acid-engine/src/va/`)

### Signal path

```
VCO (pulse, saw) ─┐
Sub osc ──────────┼─ mixer ─ VCF (24 dB ladder) ─ VCA ─ chorus ─ out (stereo)
Noise ────────────┘
```

- **VCO:**
  - Range 16' / 8' / 4' / 2'.
  - Tune ±50 cents.
  - Mod: the LFO's depth on pitch.
  - Saw and pulse, both band-limited (PolyBLEP).
  - PW from 50% to a thin pulse, never reaching 0.
  - PWM source: Manual / LFO / Env.
- **Sub osc:** −1 oct square, −2 oct square, or −2 oct pulse (25%). Derived from the VCO's phase so it stays locked to it.
- **Noise:** white, from a per-machine PRNG.
- **Mixer:** levels for Pulse, Saw, Sub and Noise.
- **VCF:**
  - A zero-delay-feedback 4-pole ladder, with tanh saturation in the feedback path.
  - It self-oscillates at maximum Resonance; gain compensation keeps the passband from collapsing.
  - Cutoff is the sum, in octaves, of: the knob, Env × the ADSR, Mod × the LFO, Kybd × (note − C3), Vel × (velocity − 64), and Drift wander.
- **ENV:** one ADSR.
  - Trigger mode:
    - **Gate+Trig** retriggers on every new note, including legato notes.
    - **Gate** doesn't retrigger legato notes.
    - **LFO** retriggers on each LFO cycle while a note is held.
- **VCA:**
  - Mode Env (follows the ADSR) or Gate (on/off with a short de-click ramp).
  - Vel sets how much velocity scales the level; Vel = 0 means velocity is ignored.
- **LFO:**
  - Triangle / Square / Random (sample & hold) / Noise, 0.1–30 Hz, free running.
  - The original's exact wave list still needs checking against the hardware owner's manual before implementation. If it differs, follow the manual.
- **Portamento:**
  - Off / On (always glides) / Auto (glides only between overlapping notes).
  - Time sets the glide time constant.
- **Drift:** 0..1.
  - It adds three things: slow smoothed-random pitch wander (up to about ±8 cents at full), a small random pitch offset drawn per note, and slow cutoff wander (up to about ±0.15 oct).
  - At 0, the drift code contributes exactly nothing, so output stays bit-identical between runs.
- **Chorus:**
  - Reuses the FM's `Chorus`, which moves from `fm/fx.rs` to a shared `fx/chorus.rs`; FM imports it from there.
  - Rate, Depth and Mix; Mix 0 bypasses it.
- **Volume.**

Params update once per control block (16 samples), as `synth.rs` does,
with per-sample smoothing on cutoff and level so knob moves don't zipper.

### Note handling

- **Held stack:** `MonoSynth` keeps a stack of held notes, capped at 16. A note-on past the cap drops the oldest. It never allocates.
- **Arp off:** the last note has priority. Releasing the sounding note falls back to the most recent still-held note: it glides there if portamento allows and doesn't retrigger in Gate mode. Releasing the last held note starts the release stage.
- **Arp on:**
  - The held notes, sorted by pitch and expanded over Range octaves, form the sequence.
  - **Up** goes low to high; **Down** goes high to low.
  - **U&D** goes up and then down, without repeating the top and bottom notes.
  - Each step is a fresh note, gated for half a step. The envelope retriggers; portamento On glides between steps, while Auto doesn't (the gate has closed).
  - A single held note repeats at the arp rate.
  - When the held set changes, the arp keeps its position in the new sequence (clamped to its length) rather than restarting.
- **Notes off:** `all_off`, `pattern_off` and `reset` behave as the FM's do: no stuck notes on stop, pattern switch or mute.

### Arp clock

- **Where it runs:** inside the machine, timed in samples. The sequencer's 4-ticks-per-step grid can't place triplets on its own.
- **Rates:** 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32. Each is a whole number of thirds of a tick (48, 24, 16, 12, 8, 6), so step boundaries are found with integer maths.
- **Transport running:**
  - Before each sequencer tick, the engine reads the tick's index and length in samples, as `Sequencer::tick_clock() -> (u64, f64)`. After the tick it calls `MonoSynth::clock_tick(tick, tick_len)`.
  - If an arp step boundary falls inside that tick, the machine schedules it at the exact sample offset.
  - Because ticks carry the swung length, the arp follows the song's swing grid at every rate, and it is always in phase with the song position.
  - In Song mode the tick is counted from the current bar's start, so the arp's grid restarts on every bar, even after a bar shorter than 16 steps. In Pattern mode it is counted from Play.
  - While the transport runs, arp notes start only on step boundaries, as on hardware clocked externally.
- **Transport stopped:** live audition still arpeggiates. The first note plays at once, then the arp free-runs from the tempo (`set_bpm`).
- **Arp note length:** half an arp step, so every step re-opens the gate and retriggers the envelope.

### Machine integration

- **`MachineType::Mono`:**
  - Label "Mono", stem "Mono".
  - `transposes()` is true.
  - Note patterns: it pastes to and from FM and Sampler, but not to the 303 or drum patterns.
  - It isn't part of `standard_rack()`; it's added from the rack's Add menu.
- **Engine side:**
  - `MachineDsp::Mono(Box<MonoSynth>)`, with `SlotPatterns::Notes`.
  - Sequencer `NoteOn`/`NoteOff` and the audition commands route to it the same way as to the FM.
- **`MonoParam`** (a `param_enum!`):
  - Range, Tune, VcoMod, PulseWidth, PwmSource
  - PulseLevel, SawLevel, SubLevel, SubWave, NoiseLevel
  - Cutoff, Resonance, EnvAmount, VcfMod, KeyFollow, VelToVcf
  - Attack, Decay, Sustain, Release, Trigger, VcaMode, VelToVca
  - LfoWave, LfoRate
  - Portamento, PortaTime
  - ArpOn, ArpMode, ArpRange, ArpRate
  - Drift
  - ChorusRate, ChorusDepth, ChorusMix
  - Volume
- **Param plumbing:** `MachineParam::Mono(MonoParam)` fits only `MachineType::Mono`. Stepped params use `steps` so knobs click through their choices. All params are automatable and recordable through the existing paths.
- **`MonoMachine`** (a `MachineBody` variant): note patterns plus `patch_name: String`.
- **File format:** `PROJECT_VERSION = 4`. Version 3 files parse with the same `Project` struct, since the change only adds a variant. The bump makes older builds refuse files that have a Mono machine with the "newer than this SidAcid" message, instead of a parse error. `validate` checks Mono patterns like the FM's.

## 2. Presets and Randomise

- **Starter patches** (`va/starter.rs`):
  - There are about 14 original patches, each a name plus a `&[(MonoParam, f32)]` table. Params not listed take their defaults.
  - Basses: Sub Bass, Squelch, Pluck Bass, Rubber Bass.
  - Leads: Saw Lead, Square Lead, PWM Lead, Chorus Lead.
  - The rest: Blip, Zap, Noise Snare, Arp Pluck (arp on), Res Sweep, Drift Pad.
  - Names are generic, not hardware patch names.
- **Loading** a patch writes every `MonoParam` as one undoable edit and sets `patch_name`.
- **`randomise_mono(current, seed)`** in `random.rs`:
  - It's deterministic per seed and applied as one undo step.
  - It picks a character (Bass / Lead / Blip / Sweep) and draws params within ranges that suit it.
  - Guards:
    - at least one source is audible
    - with Env amount low, cutoff isn't near closed
    - Sustain or Decay is long enough that notes sound
    - Volume is left alone
  - Arp and Drift settings are kept, except that Blip turns the arp on at 1/16.
  - `patch_name` becomes "Random".

## 3. UI (`app/src/ui/mono_panel.rs`)

The panel uses the same `machine_panel` frame as the FM. Its title is
`MONO   A01   <patch name>`.

1. **Front-panel row:** labelled groups in `horizontal_wrapped`.
   - **VCO:** Range, Tune, Mod, PW, PWM
   - **Source:** Pulse, Saw, Sub, Sub Wave, Noise
   - **VCF:** Cutoff, Reso, Env, Mod, Kybd, Vel
   - **Env:** A, D, S, R, Trig, VCA, Vel
2. **Second row:**
   - **LFO:** Wave, Rate
   - **Porta:** Mode, Time
   - **Arp:** On, Mode, Range, Rate
   - **Drift**
   - **Chorus:** Rate, Depth, Mix
   - **Volume**
   - The ADSR sketch, using the existing `env_sketch`
3. **Preset browser:** the starter patch list, with the loaded patch highlighted, and a **Randomise** button. There's no import.
4. **Pattern row and poly roll:** `note_pattern_row` and `poly_roll` as they are now. The roll opens centred on C2, since the machine is bass-biased.

Target height: no taller than the FM panel with its collapsing header
closed, so the rack stays usable at 1280×800.

## 4. Tests

- **Engine (`va`):**
  - Saw aliasing at a high note stays below a threshold that the naive saw fails.
  - The filter self-oscillates at maximum Resonance with no input.
  - ADSR stage timing.
  - Last-note priority and fallback.
  - Portamento Auto glides only between overlapping notes.
  - Gate vs Gate+Trig retriggering.
  - Drift 0 is bit-identical across two runs; Drift 1 differs.
  - The step order of each arp mode, including the U&D no-repeat ends and Range 2/3.
  - Arp steps land on the tempo grid at 1/16 and 1/16T from `clock_tick`, and an engine test counts 16 arp steps in one bar at 1/16.
  - The arp follows a chord change without restarting.
  - No stuck notes after `pattern_off`.
  - Output stays finite for every param at both extremes.
- **Params:** `MonoParam` fits only Mono, and its indices are stable.
- **Randomise:** deterministic per seed, varied across seeds, and the guards hold over many seeds.
- **App:**
  - a headless draw of the Mono panel
  - preset load and Randomise are each one undo step
  - Randomise doesn't move the rack
  - adding a Mono machine from the rack menu works
- **Save and load:** a round-trip of a project with a Mono machine; v3 files still load; the FM chorus behaves exactly as before after the move.

## 5. Build order

One implementation plan, merged to master in stages:

1. Move `Chorus` to `fx/chorus.rs` (no behaviour change).
2. `va` DSP blocks: oscillators, ladder filter, ADSR, LFO, drift, each unit-tested.
3. `MonoSynth`: voice, note stack, portamento, arp and clock.
4. `MonoParam`, `MachineType::Mono`, rack slot, sequencer and audition routing, file format v4.
5. Starter patches and `randomise_mono`.
6. `mono_panel.rs` and the rack Add menu entry.
