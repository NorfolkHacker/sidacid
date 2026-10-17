# FM synth

The FM machine is a six-operator FM synth in the style of classic 6-operator FM synths. This chapter explains how FM makes sound, then goes through the panel from top to bottom: the 32 algorithms and the live algorithm diagram, the global knobs, the operator editor, the LFO, the pitch envelope, poly and mono play, the built-in filter, chorus and phaser, the Vintage switch, and the voice browser with its starter voices, Randomise and `.syx` import.

![The FM panel: algorithm diagram, operator editor and voice browser](../images/fm.png)

## How FM works

An **operator** is a sine-wave oscillator with its own volume envelope. The FM machine has six of them, OP1 to OP6, in every voice.

An operator can do one of two jobs:

- A **carrier** is heard. Its output goes to the machine's mix.
- A **modulator** is not heard. Its output bends the pitch of another operator very fast, which adds overtones to that operator's sound. The louder the modulator, the brighter and harsher the result. A modulator tuned to a whole-number ratio of its target (×1, ×2, ×3…) gives harmonic tones; a ratio that isn't a whole number gives bell-like or metallic tones.

Because each operator has its own envelope, the brightness can change over the course of a note: a modulator that decays quickly gives a bright attack that softens, as in a plucked or struck sound.

An **algorithm** decides which operators are carriers and which modulate which. One operator in each algorithm can also modulate itself; this is **feedback**, and it turns its sine towards a saw-like or noisy tone.

## The panel at a glance

The panel title shows the machine's name, the selected pattern (for example `A01`) and the name of the voice last loaded (`INIT VOICE` on a new machine). Below it, from top to bottom:

1. The algorithm diagram and the global knobs (Alg, Fdbk, Sync, Trans, Mono, Glide, Vintage, Volume).
2. The operator editor: six operator buttons, the selected operator's knobs, and its envelope.
3. **LFO · Pitch EG · Effects**, a section that is closed until you click its header.
4. The voice browser: **Starter**, **Imported**, **Import .syx…** and **Randomise**, above a grid of voice names.
5. The pattern row. You write the FM machine's notes in its lane in the editor dock; it plays chords. See [Editor dock](editor-dock.md) and [Rack and patterns](rack-and-patterns.md).

Knobs work as everywhere in SidAcid: drag to turn, hold Shift for fine moves, double-click to reset. Hover a knob to see its full name and value. A switch shows its position under its label: click it for the next position, right-click for the previous one.

Many knobs here step through a fixed set of positions (Alg 1–32, Fdbk 0–7 and so on). They still turn like knobs; the value jumps from one position to the next as you drag.

## Algorithms

The **Alg** knob picks one of 32 algorithms. Its tooltip shows the number, 1 to 32.

### The algorithm diagram

The diagram to the left of the global knobs always shows the current algorithm, labelled `ALG` and its number. It redraws as soon as you turn Alg.

- Each box is an operator, numbered 1 to 6.
- **Carriers** are the lime-green boxes on the bottom row. Modulators are the grey boxes above them.
- A line joins a modulator (above) to the operator it modulates (below).
- A small blue loop on the side of one box marks the operator with feedback.
- The selected operator has a red-orange outline. Click a box to select that operator in the operator editor.

### All 32 algorithms

"2 → 1" means OP2 modulates OP1. "4+5 → 3" means OP4 and OP5 both modulate OP3.

| Alg | Carriers | Modulation | Feedback on |
|---|---|---|---|
| 1 | 1, 3 | 2 → 1; 4 → 3; 5 → 4; 6 → 5 | OP6 |
| 2 | 1, 3 | 2 → 1; 4 → 3; 5 → 4; 6 → 5 | OP2 |
| 3 | 1, 4 | 2 → 1; 3 → 2; 5 → 4; 6 → 5 | OP6 |
| 4 | 1, 4 | 2 → 1; 3 → 2; 5 → 4; 6 → 5 | OP6 |
| 5 | 1, 3, 5 | 2 → 1; 4 → 3; 6 → 5 | OP6 |
| 6 | 1, 3, 5 | 2 → 1; 4 → 3; 6 → 5 | OP6 |
| 7 | 1, 3 | 2 → 1; 4+5 → 3; 6 → 5 | OP6 |
| 8 | 1, 3 | 2 → 1; 4+5 → 3; 6 → 5 | OP4 |
| 9 | 1, 3 | 2 → 1; 4+5 → 3; 6 → 5 | OP2 |
| 10 | 1, 4 | 2 → 1; 3 → 2; 5+6 → 4 | OP3 |
| 11 | 1, 4 | 2 → 1; 3 → 2; 5+6 → 4 | OP6 |
| 12 | 1, 3 | 2 → 1; 4+5+6 → 3 | OP2 |
| 13 | 1, 3 | 2 → 1; 4+5+6 → 3 | OP6 |
| 14 | 1, 3 | 2 → 1; 4 → 3; 5+6 → 4 | OP6 |
| 15 | 1, 3 | 2 → 1; 4 → 3; 5+6 → 4 | OP2 |
| 16 | 1 | 2+3+5 → 1; 4 → 3; 6 → 5 | OP6 |
| 17 | 1 | 2+3+5 → 1; 4 → 3; 6 → 5 | OP2 |
| 18 | 1 | 2+3+4 → 1; 5 → 4; 6 → 5 | OP3 |
| 19 | 1, 4, 5 | 2 → 1; 3 → 2; 6 → 4; 6 → 5 | OP6 |
| 20 | 1, 2, 4 | 3 → 1; 3 → 2; 5+6 → 4 | OP3 |
| 21 | 1, 2, 4, 5 | 3 → 1; 3 → 2; 6 → 4; 6 → 5 | OP3 |
| 22 | 1, 3, 4, 5 | 2 → 1; 6 → 3; 6 → 4; 6 → 5 | OP6 |
| 23 | 1, 2, 4, 5 | 3 → 2; 6 → 4; 6 → 5 | OP6 |
| 24 | 1, 2, 3, 4, 5 | 6 → 3; 6 → 4; 6 → 5 | OP6 |
| 25 | 1, 2, 3, 4, 5 | 6 → 4; 6 → 5 | OP6 |
| 26 | 1, 2, 4 | 3 → 2; 5+6 → 4 | OP6 |
| 27 | 1, 2, 4 | 3 → 2; 5+6 → 4 | OP3 |
| 28 | 1, 3, 6 | 2 → 1; 4 → 3; 5 → 4 | OP5 |
| 29 | 1, 2, 3, 5 | 4 → 3; 6 → 5 | OP6 |
| 30 | 1, 2, 3, 6 | 4 → 3; 5 → 4 | OP5 |
| 31 | 1, 2, 3, 4, 5 | 6 → 5 | OP6 |
| 32 | 1, 2, 3, 4, 5, 6 | none | OP6 |

On the original instruments, algorithms 4 and 6 feed back through a loop of several operators. In SidAcid they use OP6 feeding back on itself, so algorithm 4 sounds the same as algorithm 3, and algorithm 6 the same as algorithm 5.

## Global controls

These sit to the right of the diagram. The name in brackets is the full name, shown in the knob's tooltip and in automation.

| Control | What it does | Range or positions |
|---|---|---|
| Alg (Algorithm) | Picks the algorithm. | 1–32; default 1 |
| Fdbk (Feedback) | How strongly the feedback operator modulates itself. 0 is off. | 0–7; default 0 |
| Sync (Osc Sync) | On: every operator's sine restarts from the same point at the start of each note, so notes start the same way every time. Off: operators keep running, so each note starts slightly differently. | Off, On; default On |
| Trans (Transpose) | Shifts every note up or down. | −24 to +24 semitones; default 0 |
| Mono | Poly or mono play. See [Poly and mono](#poly-and-mono). | Poly, Mono; default Poly |
| Glide | Glide time between legato notes in Mono. | 0–2.00 s; default 0 |
| Vintage | A grittier, lower-resolution sound. See [Vintage](#vintage). | Off, On; default Off |
| Volume | The machine's output level. | 0–100%; default 70% |

## Operators

### Choosing an operator

The row under the diagram has six buttons, **OP1** to **OP6**. Each shows the operator's frequency: a ratio such as `×1.00` or `×2.50`, or a fixed frequency such as `100Hz`. Click a button (or a box in the diagram) to select that operator. The knobs below always belong to the selected operator.

### Operator knobs

| Control | What it does | Range or positions |
|---|---|---|
| On | Switches the operator off (silent) or on. Loading a voice turns every operator on. | Off, On; default On |
| Level | The operator's output level. For a carrier this is its volume; for a modulator it is how much it changes the sound. The scale is logarithmic: small changes near the top matter most. | 0–99; default 99 for OP1, 0 for the others |
| Mode (Fixed Freq) | Ratio: the operator follows the note played. Fixed: it stays at one frequency whatever note you play, useful for noises and drum sounds. | Ratio, Fixed; default Ratio |
| Coarse | Ratio mode: the frequency as a multiple of the note. 0 is ×0.5, 1 is ×1, 2 is ×2 and so on. Fixed mode: positions 0, 1, 2 and 3 give 1 Hz, 10 Hz, 100 Hz and 1000 Hz, and the pattern repeats for higher positions. | 0–31; default 1 |
| Fine | Ratio mode: adds up to 99% to the ratio (×1 with Fine 50 is ×1.50). Fixed mode: raises the frequency smoothly from the Coarse value towards the next decade (Fine 99 is almost ×10). | 0–99; default 0 |
| Detune | A small pitch offset, for a slow beating against other operators. Ratio mode only. | −7 to +7 cents; default 0 |
| Vel (Velocity) | How much a note's velocity changes the operator's level. 0 ignores velocity; higher settings make soft notes quieter. On a modulator this makes soft notes duller. | 0–7; default 0 |
| AMS (Amp Mod Sens) | How much the LFO's amplitude depth (A.Depth) reaches this operator. | 0–3; default 0 |
| Rate Sc (Rate Scaling) | Makes the envelope faster on higher notes, as on acoustic instruments. | 0–7; default 0 |
| Break (Break Point) | The key where keyboard level scaling splits into its left and right sides. Each step moves it up one key. | 0–99; default 39 |
| L.Depth (Left Depth) | How much the level changes for notes below the break point. | 0–99; default 0 |
| R.Depth (Right Depth) | How much the level changes for notes above the break point. | 0–99; default 0 |
| L.Curve (Left Curve) | The shape of the change below the break point. | -Lin, -Exp, +Exp, +Lin; default -Lin |
| R.Curve (Right Curve) | The shape of the change above the break point. | -Lin, -Exp, +Exp, +Lin; default -Lin |

### Keyboard level scaling

Keyboard level scaling makes an operator louder or quieter the further a note is from the break point. Use it to thin out a bass voice in the high notes, or to stop a bright modulator getting harsh at the top of the keyboard.

- The **minus** curves (-Lin, -Exp) lower the level away from the break point; the **plus** curves (+Lin, +Exp) raise it.
- **Lin** changes the level evenly with distance. **Exp** changes it little near the break point and much more further away.
- With both depths at 0 (the default), the curves do nothing.

### Envelope

Each operator has a **four-rate, four-level envelope**. It moves through four stages; each stage heads for its level at its rate.

1. Key down: the envelope starts from silence and moves to **Lvl 1** at the **Attack** rate.
2. It then moves to **Lvl 2** at the **Decay 1** rate.
3. Then to **Lvl 3** at the **Decay 2** rate. It holds Lvl 3 for as long as the note is held: Lvl 3 is the sustain level.
4. Key up: it moves to **Lvl 4** at the **Release** rate.

| Control | Full name | Range | Default |
|---|---|---|---|
| Attack | Rate 1 | 0–99 | 99 |
| Decay 1 | Rate 2 | 0–99 | 99 |
| Decay 2 | Rate 3 | 0–99 | 99 |
| Release | Rate 4 | 0–99 | 99 |
| Lvl 1 | Level 1 | 0–99 | 99 |
| Lvl 2 | Level 2 | 0–99 | 99 |
| Lvl 3 | Level 3 | 0–99 | 99 |
| Lvl 4 | Level 4 | 0–99 | 0 |

A **rate** is a speed, not a time: 99 is the fastest and 0 the slowest. A level can be higher or lower than the one before it, so a stage can rise or fall.

The small drawing to the left of the envelope knobs sketches the envelope's shape from the current rates and levels, with a short fixed stretch for the held part. It is for reading only.

If Lvl 4 is above 0, the operator keeps sounding after you let go. Set Lvl 4 to 0 for notes that end.

## LFO

Click **LFO · Pitch EG · Effects** to open the section with the LFO, pitch envelope and effects.

The **LFO** (low-frequency oscillator) is a slow wave that wobbles the pitch (vibrato) or the level (tremolo). One LFO is shared by all the voices.

| Control | What it does | Range or positions |
|---|---|---|
| Wave (LFO Wave) | The LFO's shape. S&H (sample and hold) jumps to a new random value each cycle. | Tri, Saw Dn, Saw Up, Square, Sine, S&H; default Tri |
| Speed (LFO Speed) | How fast the LFO runs, from about 0.06 Hz to about 50 Hz. | 0–99; default 35 |
| Delay (LFO Delay) | How long after a note starts before the LFO reaches it. For the first half of the delay there is no LFO; over the second half it fades in. Up to 5 s. | 0–99; default 0 |
| P.Depth (LFO Pitch Depth) | How much the LFO moves the pitch. | 0–99; default 0 |
| A.Depth (LFO Amp Depth) | How much the LFO moves the level of operators whose AMS is above 0. | 0–99; default 0 |
| Key Sync (LFO Sync) | On: the LFO restarts from the start of its cycle at every note. | Off, On; default On |
| PMS (Pitch Mod Sens) | Scales P.Depth. At P.Depth 99 the pitch swings by up to 0, 0.1, 0.2, 0.35, 0.6, 1.2, 2.4 or 12 semitones for PMS 0 to 7. | 0–7; default 3 |

Pitch vibrato needs both P.Depth and PMS above 0. Tremolo needs A.Depth above 0 and AMS above 0 on the operators you want to move.

## Pitch envelope

The **pitch envelope** (Pitch EG) bends the pitch of the whole voice over the course of a note. It works like an operator envelope, but its levels are pitches:

1. At key down the pitch starts at **P.L4**.
2. It moves to **P.L1** at rate **P.R1**, then to **P.L2** at **P.R2**, then to **P.L3** at **P.R3**.
3. It holds P.L3 while the note is held.
4. At key up it returns to P.L4 at rate **P.R4**.

| Control | Full name | Range | Default |
|---|---|---|---|
| P.R1 – P.R4 | Pitch R1 – Pitch R4 | 0–99 (99 is fastest) | 99 |
| P.L1 – P.L4 | Pitch L1 – Pitch L4 | 0–99 | 50 |

A pitch level of 50 is no change. 0 is four octaves down and 99 is just under four octaves up. With every level at 50 (the default) the pitch envelope does nothing. For a short pitch drop at the start of each note, as on a kick drum, set P.L4 above 50 and the other three levels to 50.

## Poly and mono

The **Mono** switch chooses how the machine plays notes. Switching it releases any notes that are sounding.

- **Poly** plays up to 12 notes at once. When all 12 voices are busy, a new note takes over a voice that is already releasing (the oldest such note), or, if every voice is still held, the oldest note.
- **Mono** plays one note at a time. The newest note wins; when you let it go while still holding an earlier one, the pitch returns to the earlier note. A note that overlaps the one before it is **legato**: it changes the pitch without restarting the envelopes, gliding there over the **Glide** time. A note after a gap starts fresh, with no glide. With Glide at 0 a legato note jumps straight to its pitch.

## Filter, chorus and phaser

The FM machine has its own effects, in the order filter, chorus, phaser. They act on the machine's whole output, after Volume. The chorus and phaser make the output stereo.

| Control | What it does | Range or positions |
|---|---|---|
| Filter | Switches the filter on. | Off, On; default Off |
| Mode (Filter Mode) | LP lets the lows through (darker as you lower Cutoff); HP lets the highs through (thinner as you raise it). | LP, HP; default LP |
| Cutoff | The filter's frequency. | 40 Hz–18000 Hz; default 18000 Hz |
| Reso (Resonance) | A peak at the cutoff frequency. | 0–100%; default 0% |
| Ch.Rate (Chorus Rate) | Speed of the chorus sweep, from 0.05 Hz to 5 Hz. | 0–100%; default 30% |
| Ch.Depth (Chorus Depth) | How far the chorus sweeps. | 0–100%; default 50% |
| Chorus (Chorus Mix) | How much chorus you hear. 0% is no chorus. | 0–100%; default 0% |
| Ph.Rate (Phaser Rate) | Speed of the phaser sweep, from 0.05 Hz to 5 Hz. | 0–100%; default 30% |
| Ph.Depth (Phaser Depth) | How wide the phaser sweeps. | 0–100%; default 50% |
| Ph.Fdbk (Phaser Feedback) | Makes the phaser's notches sharper and more pronounced. | 0–100%; default 30% |
| Phaser (Phaser Mix) | How much phaser you hear. 0% is no phaser. | 0–100%; default 0% |

With the filter off and both mixes at 0%, the sound passes through untouched. The machine's channel in the mixer has its own distortion, compressor and sends after these; see [Mixer and effects](mixer-and-effects.md).

## Vintage

**Vintage** gives the gritty, slightly noisy sound of early digital synths. When it is on, the operators use a coarser sine wave, and the machine's output is reduced to 12-bit resolution before Volume and the effects.

## Voices

A **voice** is a complete FM sound: the algorithm, feedback, Osc Sync, transpose, the LFO, the pitch envelope, PMS and all six operators. Volume, Mono, Glide, Vintage and the filter, chorus and phaser are not part of a voice, so loading or randomising a voice leaves them as they are.

The voice browser is at the bottom of the panel, under **Voices**. Click a name to load that voice. The loaded voice is highlighted and its name appears in the panel title. It stays highlighted while you edit it with the knobs. Loading a voice is one undo step.

### Starter voices

The **Starter** tab holds 16 voices that come with SidAcid:

| | | | |
|---|---|---|---|
| KICK FM | E.PIANO 1 | FM BASS | ACID FM |
| BELLS | MARIMBA | BRASS | STRINGS |
| PAD | ORGAN | PLUCK | LEAD |
| CLAV | SNARE FM | HAT FM | TUBULAR |

### Randomise

**Randomise** makes a new sound that always plays: a new algorithm, feedback, operators, LFO and pitch envelope. Each random voice aims for one of three characters: a clean tone, a bright metallic tone, or a rhythmic sound pulsed by a fast LFO.

Randomise keeps your Osc Sync and Transpose settings, and, as for any voice load, leaves Volume, Mono, Glide, Vintage and the effects alone. The new voice is called `RANDOM`. Press Undo to get the previous sound back.

### Importing .syx files

You can load voices made for classic 6-operator FM synths from `.syx` (SysEx) files. Two kinds of file work:

- A **32-voice bank**.
- A **single voice**.

To import a file:

1. Click **Import .syx…**.
2. Pick the `.syx` file.

A single voice loads straight into the machine; its name appears in the panel title.

A bank goes to the **Imported** tab, which opens to show its 32 voice names. Click a name to load that voice. The **Imported** tab is greyed out until you have imported a bank. Each FM machine keeps one imported bank: importing another bank replaces it. The bank is saved in the project file, and you can undo the import.

If the file is not a 6-operator FM voice or bank, is cut short, or fails its checksum, SidAcid shows a message saying so and nothing changes. If a file holds several SysEx messages, only the first is read.

See also: [Rack and patterns](rack-and-patterns.md) · [Editor dock](editor-dock.md) · [Mono synth](mono-synth.md) · [Mixer and effects](mixer-and-effects.md) · [Song mode](song-mode.md)
