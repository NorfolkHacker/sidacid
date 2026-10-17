# Mono synth

The Mono machine is a virtual-analog monophonic synth: one oscillator with a sub oscillator and noise, a resonant low-pass filter, one envelope, an LFO, glide, an arpeggiator, drift and a chorus. This chapter explains how to add one to the rack, goes through every control in the order the panel shows them, and covers the 14 starter patches and Randomise.

## Adding a Mono machine

The Mono machine is not in the default rack. To add one:

1. Scroll to the bottom of the rack, below the last machine.
2. Click **+ Add machine ⏷**.
3. Choose **Mono**.

The new machine appears at the end of the rack, named `Mono` (or `Mono 2`, `Mono 3` and so on if that name is taken), with the patch `Init`: every control at its default. The rack holds up to 16 machines; the count next to the Add button shows how many you have, for example `7 / 16`. See [Rack and patterns](rack-and-patterns.md) for moving, duplicating and removing machines.

You write the Mono machine's notes in its lane in the editor dock. See [Editor dock](editor-dock.md).

## How it plays

The Mono machine plays one note at a time. When notes overlap, the newest one sounds. When you let go of it while an earlier note is still held, the pitch goes back to the earlier note. It remembers up to 16 held notes.

Two notes that overlap are **legato**. Legato matters for three controls: the envelope's **Trig** switch, glide in **Auto** mode, and the arpeggiator, which plays every held note in turn.

## The panel

The panel title shows the machine's name, the selected pattern (for example `A01`) and the name of the patch last loaded.

The knobs are in framed groups, in two rows:

- First row: **VCO**, **SOURCE**, **VCF**, **ENV**.
- Second row: **LFO**, **PORTA**, **ARP**, **DRIFT**, **CHORUS**, **OUT**, and a small drawing of the envelope's shape.

Below them are the patch browser (**Patches**, **Randomise** and the starter patches) and the pattern row.

Knobs work as everywhere in SidAcid: drag to turn, hold Shift for fine moves, double-click to reset. Hover a knob to see its full name and its value as a percentage. A switch shows its position under its label: click it for the next position, right-click for the previous one.

In the tables below, the name in brackets is the control's full name, which the tooltip and automation use. Where the panel only shows a percentage, the real range is given in the "What it does" column.

## VCO

The **VCO** (voltage-controlled oscillator) makes the basic tone: a saw wave and a pulse wave at the same pitch, both band-limited so high notes stay clean.

| Control | What it does | Range or positions |
|---|---|---|
| Range | The oscillator's octave, in organ-pipe footage: 16' is an octave down, 8' is normal pitch, 4' an octave up, 2' two octaves up. | 16', 8', 4', 2'; default 8' |
| Tune | Fine tuning, up to half a semitone (50 cents) either way. | 0–100%; default 50% (in tune) |
| Mod (VCO Mod) | How much the LFO moves the pitch (vibrato), up to 12 semitones either way at full. The knob is gentle at the bottom of its range for subtle vibrato. | 0–100%; default 0% |
| PW (Pulse Width) | With PWM on Manual: the pulse wave's shape, from a square wave at 0% to a thin pulse at 100%. With PWM on LFO or Env: how far the shape is moved. | 0–100%; default 0% |
| PWM (PWM Source) | What moves the pulse width. Manual: the PW knob alone sets it. LFO: the LFO sweeps it between square and the PW setting. Env: the envelope sweeps it the same way. | Manual, LFO, Env; default Manual |

## Source

The **SOURCE** group is the mixer: one level for each sound source.

| Control | What it does | Range or positions |
|---|---|---|
| Pulse | Level of the pulse wave. | 0–100%; default 0% |
| Saw | Level of the saw wave. | 0–100%; default 80% |
| Sub | Level of the sub oscillator, which follows the VCO below its pitch. | 0–100%; default 0% |
| Sub Wave | The sub oscillator's shape. -1 Sq: a square wave one octave down. -2 Sq: a square wave two octaves down. -2 Pls: a narrow pulse (high a quarter of the time) two octaves down. | -1 Sq, -2 Sq, -2 Pls; default -1 Sq |
| Noise | Level of white noise. | 0–100%; default 0% |

The sub oscillator counts the VCO's cycles, so it always stays locked in phase with it.

## VCF

The **VCF** (voltage-controlled filter) is a 4-pole (24 dB per octave) resonant low-pass ladder filter. It lets the lows through and cuts the highs above the cutoff frequency. At full resonance it oscillates by itself, giving a whistling tone at the cutoff.

| Control | What it does | Range or positions |
|---|---|---|
| Cutoff | The filter's frequency, from 20 Hz to 18 kHz. | 0–100%; default 50% (about 600 Hz) |
| Reso (Resonance) | A peak at the cutoff; at full, the filter self-oscillates. | 0–100%; default 20% |
| Env | How far the envelope opens the filter, up to 8 octaves above Cutoff at full. | 0–100%; default 40% |
| Mod (VCF Mod) | How much the LFO moves the cutoff, up to 4 octaves either way at full. | 0–100%; default 0% |
| Kybd (Key Follow) | How much the cutoff follows the note you play. At 100% the cutoff moves an octave for each octave of the note, so every note has the same brightness; at 0% it stays put, so high notes sound duller. Notes are measured from C3 (MIDI note 48). | 0–100%; default 50% |
| Vel (Vel > VCF) | How much a note's velocity moves the cutoff, up to 3 octaves either way at full: hard notes are brighter, soft notes darker. A velocity in the middle of the range leaves it unchanged. | 0–100%; default 0% |

## Env

The **ENV** group is the one envelope. It shapes the volume (unless VCA is set to Gate) and, through the Env knob in the VCF group, the filter. It can also move the pulse width.

| Control | What it does | Range or positions |
|---|---|---|
| A (Attack) | Time to rise from silence to full, 1.5 ms to 4 s. | 0–100%; default 0% |
| D (Decay) | Time to fall from full to the sustain level, 2 ms to 8 s. | 0–100%; default 40% |
| S (Sustain) | The level held while the note is held. | 0–100%; default 60% |
| R (Release) | Time to fall to silence after the note ends, 2 ms to 8 s. | 0–100%; default 20% |
| Trig (Trigger) | What restarts the envelope. See below. | Gate+Trig, Gate, LFO; default Gate+Trig |
| VCA (VCA Mode) | Env: the envelope shapes the volume. Gate: the volume is simply on while a note is held and off when it ends (with a 2 ms fade so it doesn't click); the envelope still moves the filter and pulse width. | Env, Gate; default Env |
| Vel (Vel > VCA) | How much a note's velocity sets its volume. At 0% every note is equally loud; at 100% the volume follows the velocity fully. | 0–100%; default 0% |

The **Trig** positions:

- **Gate+Trig** restarts the envelope on every new note, legato or not.
- **Gate** restarts it only on a note after a gap. Legato notes change the pitch but carry on the same envelope.
- **LFO** restarts it at the start of every LFO cycle while a note is held (and on a note after a gap), so a held note pulses at the LFO rate.

A restarted envelope attacks from wherever it is, not from silence, so it never clicks.

## LFO

The **LFO** (low-frequency oscillator) is a slow, free-running wave. It drives the VCO's Mod, the VCF's Mod, PWM when PWM is set to LFO, and the envelope when Trig is set to LFO.

| Control | What it does | Range or positions |
|---|---|---|
| Wave (LFO Wave) | Tri: a triangle wave. Square: jumps between high and low. Random: a new random level each cycle, held until the next (sample and hold). Noise: random at every sample. | Tri, Square, Random, Noise; default Tri |
| Rate (LFO Rate) | Speed, from 0.1 Hz to 30 Hz. | 0–100%; default 40% |

## Porta

**PORTA** (portamento) is glide: the pitch slides from one note to the next instead of jumping.

| Control | What it does | Range or positions |
|---|---|---|
| Mode (Portamento) | Off: every note starts at its own pitch. On: every note glides from the note before it, even after a gap. Auto: only legato notes glide. | Off, On, Auto; default Off |
| Time (Porta Time) | How long the glide takes, from 1 ms to 1.5 s. | 0–100%; default 30% |

## Arp

The **ARP** (arpeggiator) plays the held notes one after another instead of together. Hold a chord (write overlapping notes in the lane) and the arp turns it into a repeating run.

| Control | What it does | Range or positions |
|---|---|---|
| On (Arp) | Turns the arpeggiator on. | Off, On; default Off |
| Mode (Arp Mode) | Up: lowest to highest, then again from the lowest. U&D: up, then back down, without playing the top and bottom notes twice. Down: highest to lowest. | Up, U&D, Down; default Up |
| Range (Arp Range) | How many octaves the run covers. With 2 Oct or 3 Oct the held notes repeat one or two octaves higher. | 1 Oct, 2 Oct, 3 Oct; default 1 Oct |
| Rate (Arp Rate) | The length of each arp step, as a note value. T is a triplet. | 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32; default 1/16 |

How the arp plays:

- Each arp note is a fresh note: the envelope restarts on every step, and the note sounds for half the step.
- Every arp note uses the velocity of the newest held note.
- When a note arrives with no other notes held, the run starts again from the beginning.
- Turning the arp on starts the run afresh. Turning it off leaves the newest held note sounding.

### Timing and the transport

While the transport is playing, arp steps land on the song's beat grid at the project tempo, swing included.

- In Pattern mode the grid counts from when you pressed Play.
- In Song mode the grid starts again at the start of every block in the Mono machine's lane. A block whose length is not a whole number of arp steps doesn't push the arp off the beat in the block after it.

While the transport is stopped (for example, when you audition notes from the lane's key strip), the arp runs freely at the project tempo, and the first note plays straight away.

See [Song mode](song-mode.md) for blocks and the transport.

## Drift

| Control | What it does | Range or positions |
|---|---|---|
| Drift | Adds the small instabilities of an analog synth: a slow random wander in pitch and cutoff, and a slightly different pitch for each note. At 0% there is none. Even at 100% the pitch moves only a fraction of a semitone. | 0–100%; default 0% |

## Chorus

A stereo chorus. With Mix at 0% the Mono machine's output is the same in both channels; with chorus it becomes stereo.

| Control | What it does | Range or positions |
|---|---|---|
| Rate (Chorus Rate) | Speed of the chorus sweep, from 0.05 Hz to 5 Hz. | 0–100%; default 30% |
| Depth (Chorus Depth) | How far the chorus sweeps. | 0–100%; default 50% |
| Mix (Chorus Mix) | How much chorus you hear. 0% is no chorus. | 0–100%; default 0% |

## Out

| Control | What it does | Range or positions |
|---|---|---|
| Volume | The machine's output level. | 0–100%; default 70% |

The machine's channel in the mixer has its own level, pan, distortion, compressor and sends after this; see [Mixer and effects](mixer-and-effects.md).

## Envelope sketch

At the end of the second row, a small drawing shows the shape of the envelope from the current A, D, S and R settings, with a short fixed stretch for the held part. It is for reading only.

## Patches

### Starter patches

The patch browser under **Patches** holds 14 starter patches. Click a name to load it. The loaded patch is highlighted and its name appears in the panel title.

| | | | | | | |
|---|---|---|---|---|---|---|
| Sub Bass | Squelch | Pluck Bass | Rubber Bass | Saw Lead | Square Lead | PWM Lead |
| Chorus Lead | Blip | Zap | Noise Snare | Arp Pluck | Res Sweep | Drift Pad |

Loading a patch sets every control on the panel, Volume included (none of the starter patches change Volume from its default of 70%). **Arp Pluck** is the only starter patch with the arpeggiator on. You can undo a patch load.

### Randomise

**Randomise** makes a new sound that always plays. Each random sound is one of four kinds: a bass, a lead, a blip or a filter sweep. It sets the oscillator, mixer, filter, envelope, LFO and glide controls to suit that kind, and checks that there is something to hear, that the filter opens and that notes last long enough.

Randomise keeps your:

- **Volume**,
- arpeggiator settings (**On**, **Mode**, **Range**, **Rate**),
- **Drift**,
- chorus settings (**Rate**, **Depth**, **Mix**).

The one exception is a blip: Randomise then turns the arp on at **1/16**, with a random Mode and Range.

It also puts eight controls back to their defaults: **Tune**, **Sub Wave**, **Kybd**, **Vel** (Vel > VCF), **Trig**, **VCA**, **Vel** (Vel > VCA) and the LFO **Wave**.

The new patch is called `Random`. Press Undo to get the previous sound back.

See also: [Rack and patterns](rack-and-patterns.md) · [Editor dock](editor-dock.md) · [Song mode](song-mode.md) · [FM synth](fm-synth.md) · [Acid synth](acid-synth.md) · [Mixer and effects](mixer-and-effects.md)
