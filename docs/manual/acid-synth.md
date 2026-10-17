# Acid synth

The acid synth is SidAcid's bass line machine: a monophonic synth in the style of the classic acid bass line, played on a sound engine modelled on a classic 8-bit home-computer sound chip. This chapter covers its knobs and switches, how accent and slide shape the sound, the step grid in its panel, the Dice buttons that randomise a pattern, and the pattern's length and transpose.

A new project's rack has two acid synths, **Synth 1** and **Synth 2**. You can add more from the rack's **+ Add machine** menu, where the type is called **Acid Synth** (see [Rack and patterns](rack-and-patterns.md)).

![The rack with an acid synth panel and its step grid](../images/screenshot.png)

## The panel

An acid synth's panel shows, from top to bottom:

- the header, with the machine's name and the selected pattern (for example "SYNTH 1   A01");
- a row of 15 knobs and switches;
- the pattern pads (banks A–H, patterns 1–16), the **Len** and **Trans** fields and, while you step-write, a status line;
- the **Dice** buttons;
- the step grid.

The notes themselves also appear in the synth's lane in the editor dock below the rack, as a piano roll. See [The step grid and the roll](#the-step-grid-and-the-roll).

## Knobs and switches

Drag a knob up or down to turn it (hold Shift for fine control), or use the mouse wheel over it. Double-click a knob to reset it to its default. Hover over a knob to see its full name and value; for the acid synth the value is shown as a percentage of the knob's travel. The tables below give what that travel means.

A switch is a button that shows its current position under its label. Click it to step to the next position, right-click to step back.

### Pitch and level

| Control | What it does | Range or positions | Default |
|---|---|---|---|
| Tune | Shifts the pitch of every note. The knob is continuous, so you can set it between semitones. | −12 to +12 semitones | 0 |
| Volume | The synth's output level, before its mixer channel. | 0–100 % | 70 % |

### Filter and envelope

Every note passes through one resonant filter. A filter envelope opens the filter when a note starts and closes it again over the decay time.

| Control | What it does | Range or positions | Default |
|---|---|---|---|
| Cutoff | The filter's base cutoff frequency. The envelope and accent sweep raise the cutoff from here. | 20 Hz to 18 kHz (exponential) | about 216 Hz |
| Reso | Resonance: how strongly the filter rings at the cutoff. High settings give the squelchy, self-limiting acid sound. | 0–100 % | 60 % |
| Env Mod | How far the filter envelope opens the cutoff on each note. At full, the envelope's peak is four octaves above Cutoff. | 0–100 % | 50 % |
| Decay | How long the filter envelope takes to die away on unaccented notes. Accented notes ignore it (see [Accent](#accent)). | 30 ms to 2.5 s | about 176 ms |
| Mode | The filter type: low-pass, band-pass or high-pass. | LP, BP, HP | LP |
| Accent | The strength of accented notes (see [Accent](#accent)). | 0–100 % | 60 % |

The cutoff never goes above 18 kHz or below 20 Hz, however far Env Mod and the accent sweep push it.

### Oscillator

| Control | What it does | Range or positions | Default |
|---|---|---|---|
| Wave | The oscillator waveform. | Saw, Pulse | Saw |
| PW | Pulse width. Only heard when Wave is Pulse. 50 % is a square wave; the extremes are thin and nasal. | 5 % to 95 % | 50 % |
| Sync | Hard sync on or off (see [Hard sync](#hard-sync)). | Off, On | Off |
| Sync Pt | Sync pitch: how far above the note the synced oscillator runs. Only heard when Sync is On. | 0 to +24 semitones | +12 semitones |

### Unison controls

| Control | What it does | Range or positions | Default |
|---|---|---|---|
| Unison | Plays each note on three voices instead of one (see [Unison](#unison)). | Off, On | Off |
| Detune | How far the outer two unison voices are tuned away from the middle one, one down and one up. | 0 to 50 cents | 15 cents |
| Spread | How far the outer two unison voices are panned, one left and one right. | 0–100 % | 60 % |

### Hard sync

With **Sync** on, each voice has a second, silent oscillator running at the note's pitch. Every time that hidden oscillator starts a new cycle, it restarts the oscillator you hear. The oscillator you hear runs **Sync Pt** semitones above the note, so you get the note's pitch with a bright, tearing timbre that changes as you turn Sync Pt. At a Sync Pt of 0 the two oscillators run together and you hear little difference.

### Unison

With **Unison** on, every note plays on three voices at once: one at the note's pitch, one lowered and one raised by **Detune**. **Spread** pans the lowered voice left and the raised voice right, so the sound gets wider in stereo. The middle voice stays centred. Each voice plays a little quieter in unison, so turning Unison on doesn't make the synth three times as loud.

With Unison off, the synth plays one voice, centred, and Detune and Spread do nothing.

## Accent

Mark a note as accented in the grid's **Acc** row, in the dock's accent strip, or with the dock's note inspector. An accented note:

- plays louder: its level is raised by up to double, set by the **Accent** knob;
- uses a short, fixed filter decay of 200 ms instead of the **Decay** knob;
- adds the accent sweep to the filter cutoff.

### The accent sweep

The accent sweep is an extra filter boost that builds up over consecutive accented notes. While an accented note plays, the sweep charges up quickly towards the filter envelope's level. Between accents it drains away more slowly. When accented notes come close together, each one starts before the sweep has drained, so the cutoff climbs higher with every accent in the run: the rising "wow" of a classic acid line. A single accent on its own gives only a small sweep.

The **Accent** knob sets how far the sweep can push the cutoff, up to three octaves at full. With Accent at 0, accented notes sound like any other note except for the shorter filter decay.

## Slide and tie

An acid note's gate closes half a step before the note's written end, so a one-step note sounds for half a step and leaves a short gap before the next note.

**Slide** changes that. When a note has slide on and another note starts exactly where it ends, the gate stays open and the synth glides to the new note's pitch. The glide settles in about 60 ms. The filter envelope is not restarted, so the second note continues the first one's filter shape instead of plucking again. A slide into a rest does nothing: the note ends half a step early as usual.

A slide on a note that ends at the pattern's end carries into a note on step 1 of the next pass.

A **tie** joins a step onto the note before it, making that note one step longer without restarting it. In the step grid, a tie is the gate row's third position. In the dock, a tied note is simply a longer note.

## Filter and sound engine

The acid synth runs on one instance of the chip-style engine, which has three voices. Without Unison the synth uses one of them; with Unison it uses all three. All voices pass through the same filter, a two-pole multimode filter whose resonance is soft-clipped so it squelches instead of blowing up.

## The step grid

The step grid in the panel shows the selected pattern one 1/16 step per column, numbered from 1 up to the pattern's length. While the pattern plays, the current step's number lights up. If the pattern is longer than the panel, scroll the grid sideways.

The grid has six rows:

| Row | Shows | Click |
|---|---|---|
| Note | The note name on steps that play, `·` on rests. | Starts step-writing from this step (see [Step-writing](#step-writing)). |
| Down | `v`, lit when the step's note is in the lower octave. | Moves the note down to the lower octave, or back to the middle one. |
| Up | `^`, lit when the step's note is in the upper octave. | Moves the note up to the upper octave, or back to the middle one. |
| Acc | `A`, lit when the step is accented. | Turns accent on or off. |
| Slide | `S`, lit when the step slides. | Turns slide on or off. |
| Gate | `·` for a rest, `■` for a new note, `—` for a tie. | Cycles rest → note → tie → rest. |

On a **Note** cell:

- **Scroll** the mouse wheel to change the step's pitch a semitone per notch, across all three octaves. Wheeling up raises the pitch. One continuous wheel movement is one undo step.
- **Right-click** to cycle the gate, the same as clicking the Gate row.

The grid covers three octaves: the lower octave (Down lit), the middle octave (neither lit, starting two octaves below middle C) and the upper octave (Up lit). A rest has no pitch, so when you turn a rest into a note it starts as C in the middle octave.

Down, Up, Acc, Slide and scrolling only change steps that play. On a rest they do nothing.

### Steps and notes

A note that lasts several steps shows as a note step followed by tie steps. A few rules follow from that:

- **Pitch** belongs to the whole note. Changing the pitch, or the octave, on any of its steps moves the whole note.
- **Accent** is read from the note's first step. Turning accent on or off on a tie step does nothing.
- **Slide** is read from the note's last step. Turning slide on or off on any other step of the note does nothing.
- Setting a step in the middle of a note to **rest** splits the note around it. Setting it to a new **note** splits the note there and starts a new one. Setting a step to **tie** joins it onto the note before; if there is no note before it, the tie starts a new note.

### The step grid and the roll

The grid and the synth's lane in the [editor dock](editor-dock.md) show the same pattern. The roll holds the notes; the grid is a 1/16-step view of them. In the grid:

- a note that starts between steps shows on the step its start falls in;
- a note whose length isn't a whole number of steps is rounded up to whole steps;
- a pitch outside the grid's three octaves is shown folded into them by octaves;
- notes past the pattern's length are not shown.

A grid edit changes only the notes under the step you edit. Everything the grid can't show stays as it was: a note's chance, ratchet and condition, an off-grid start or length, and a pitch outside the grid's range (a pitch change moves such a note by the same interval). So you can sketch a line in the grid and refine it in the roll, or the other way round, without losing work.

## Step-writing

Step-writing lets you type a pattern from the computer keyboard, one step at a time.

1. Click a cell in the grid's **Note** row. Step-writing starts there, the cell is highlighted as the cursor, and the panel shows "Step-write: step N  octave +0  (Esc to stop)".
2. Type notes. Each note key writes a note on the cursor's step and moves the cursor one step right.
3. Press **Esc** to stop.

The keys use a tracker layout: the bottom letter row plays one octave and the top letter row the octave above.

| Key | Does |
|---|---|
| Z S X D C V G B H N J M | C up to B in the writer's octave |
| Q 2 W 3 E R 5 T 6 Y 7 U | C up to B one octave higher (`,` is also the higher C) |
| I | C two octaves up |
| Space | Rest on this step, then move right |
| `-` | Tie this step to the note before, then move right |
| A | Turn accent on or off on this step (the cursor stays) |
| F | Turn slide on or off on this step (the cursor stays) |
| Left / Right | Move the cursor without changing anything |
| Backspace | Move back one step and make it a rest |
| `[` / `]` | Writer's octave down / up (−1, 0 or +1) |
| Esc | Stop step-writing |

A note keeps the accent and slide already on its step. Notes always land in the grid's three octaves: a key that would go above the upper octave or below the lower one stays in the nearest octave. The cursor wraps from the last step of the pattern back to the first.

While you step-write, these keys belong to the writer: Space writes a rest instead of starting or stopping playback. Key combinations with Ctrl still reach their usual shortcuts. Step-writing pauses while a text field or a dialog has the keyboard, and stops when:

- you press Esc;
- you click a lane in the dock (the dock takes the keyboard);
- you select a different pattern on this synth's pads;
- you start step-writing on another synth (only one synth step-writes at a time).

Each step written is its own undo step.

## Pattern length and transpose

Next to the pattern pads:

- **Len** sets the selected pattern's length in 1/16 steps, from 1 to 64. Drag the field, or click it and type a value. A drag is one undo step; each arrow-key press or typed value is its own. Shortening a pattern doesn't delete the notes past the new end: they stay hidden and come back when you lengthen it again.
- **Trans** transposes the selected pattern when it plays, from −12 to +12 semitones. It doesn't change the notes themselves, so the grid and the roll still show them untransposed.

Patterns of different lengths on different machines loop independently. A new pattern is 16 steps long. You can also lengthen a pattern by resizing a note past its end in the dock (see [Editor dock](editor-dock.md)).

## Dice

The **Dice** row randomises the selected pattern. Each click uses fresh random numbers, so clicking again gives a different result. Every click is one undo step: press Ctrl+Z to go back.

The dice "groove" with a drum kit. The first acid synth in the rack pairs with the first drum kit (Kit A or Kit B), the second synth with the second kit, and so on, and the dice look at the kit's selected pattern. Steps where that pattern has a kick (BD) or snare (SD) are more likely to get notes and accents. A synth with no kit to pair with grooves with an empty pattern.

| Button | What it changes |
|---|---|
| Notes | Gives every note a new pitch from a randomly chosen scale on C: minor, melodic minor, Phrygian, major or Hungarian minor (minor and melodic minor are picked most often, Hungarian minor least). The line favours the root, repeated notes and small steps, never leaps more than seven semitones, and stays in the grid's three octaves. |
| Accent | Accents about one note in three on kick and snare steps, and one in five elsewhere. |
| Slide | Turns slide on for about one step in seven, and on half the steps that follow a slide, so slides tend to come in chains. |
| Rest | Decides again which steps play: about 85 % of kick and snare steps and half of the others. |
| Octave | Moves some notes up an octave (about 8 %) or down an octave (about 4 %), staying within the grid's three octaves. |
| All | All five at once. |

A note that follows a slide never gets an accent from the Accent or Slide dice.

The Notes die doesn't follow the project key: its scale is always on C.

Notes, Accent, Slide and Octave change the existing notes in place. Each note keeps its start, length, chance, ratchet and condition. Notes past the pattern's length are left alone.

**Rest** rebuilds the pattern on the 1/16 grid instead. Every step that plays becomes a one-step note, so ties are lost, off-grid timing snaps to the step, and chance, ratchet and conditions are reset. When you roll Rest without Notes, a step that was a rest and now plays takes the pitch of the nearest earlier note that was already there.

## See also

[Rack and patterns](rack-and-patterns.md) · [Editor dock](editor-dock.md) · [Drum kits](drum-kits.md) · [Mixer and effects](mixer-and-effects.md) · [Song mode](song-mode.md) · [Keyboard reference](keyboard-reference.md)
