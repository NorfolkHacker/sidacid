# The rack and patterns

This chapter covers the rack: the machine types, how to add, duplicate, remove, move and rename machines, and what a machine panel's header shows. It also covers patterns: the 128 patterns each machine holds, selecting them, queued switching while playing, pattern length and transpose, copying patterns, and the dice that write or rewrite a whole pattern. Each machine's own knobs are covered in its own chapter.

## Machines

A *machine* is one sound source in the rack, with its own patterns, its own knobs and its own mixer channel. The rack lists the machines from top to bottom, and the same order is used for the lanes in the editor dock, the mixer strips and the song lanes.

These are the machine types, as the **+ Add machine** menu names them:

| Menu label | What it is | New machines are named | Chapter |
|---|---|---|---|
| Acid Synth | An 8-bit chip bass synth with classic acid-bass controls and a step grid. It plays one note at a time. | Synth 1, Synth 2, … | [Acid synth](acid-synth.md) |
| Kit A | An 8-bit chip drum machine with eight instruments in the style of one classic analog drum machine. | Kit A, Kit A 2, … | [Drum kits](drum-kits.md) |
| Kit B | The same, with a kit in the style of a second classic analog drum machine. | Kit B, Kit B 2, … | [Drum kits](drum-kits.md) |
| FM | A 6-operator FM synth that can play chords. | FM, FM 2, … | [FM synth](fm-synth.md) |
| Sampler | A 16-slot sampler for sliced breaks and pitched samples. | Sampler, Sampler 2, … | [Sampler](sampler.md) |
| Mono | A virtual-analog mono synth with an arpeggiator. | Mono, Mono 2, … | [Mono synth](mono-synth.md) |

A new project (**File → New**) starts with six machines: Synth 1, Synth 2, Kit A, Kit B, FM and Sampler.

### The 16-machine limit

The rack holds up to 16 machines. The count is shown next to **+ Add machine** as "n / 16". When the rack is full, **+ Add machine** and every panel's Duplicate button are greyed out, and pointing at them shows "Rack full (16)".

## Changing the rack

Every change in this section is a single undo step. See [Undo and redo](files-and-export.md#undo-and-redo).

### Adding a machine

Click **+ Add machine ⏷** below the last panel and pick a type. The new machine goes to the end of the rack with empty patterns and default knob settings, and it becomes the focused machine.

### Duplicating a machine

Click the duplicate button (🗐) in a panel's header. The copy goes directly below the original and is named after it with " copy" added, for example "Synth 1 copy". It gets everything the original has: every pattern, every knob, its mixer channel settings and, for a sampler, its loaded samples. It doesn't get the original's blocks in the song: its song lane starts empty. The copy becomes the focused machine.

Duplicating a sampler fails, with a message, if its samples won't fit in the project's free sample memory (256 MB of decoded audio, shared by every sampler).

### Removing a machine

Click the remove button (🗙) in a panel's header. The machine goes at once, without a question. The transport bar shows "Removed '*name*' — Ctrl+Z to undo". Removing a machine also removes its song lane, its song automation lanes, and any acid synth pattern's parameter lane that controlled one of its knobs. **Ctrl+Z** puts all of it back where it was.

### Moving a machine

Click ⬆ (Move up) or ⬇ (Move down) in a panel's header to move the machine one place. Move up is greyed out on the first machine, and Move down on the last.

### Renaming a machine

Double-click the machine's name in its panel header. Type the new name, then press **Enter** to keep it. **Esc**, or clicking somewhere else, cancels. If you clear the name and press Enter, the machine gets its type's default name back.

Names are shown in capitals in the panel header and as typed elsewhere, for example in the dock's lanes.

## Panel headers

Each machine panel starts with a header row. From left to right:

- **Collapse arrow.** Click it to fold the panel down to its header, or open it again. Only the arrow does this: clicking the name doesn't.
- **Title.** The machine's name in capitals, followed by the selected pattern, for example "SYNTH 1   A01". Drum kit titles add "DRUMS" after the name ("KIT A DRUMS   A01"). The FM and Mono titles end with the name of the voice or patch last loaded. The focused machine's title is lit in green.
- **CPU n%.** The share of the audio time this machine takes, shown while audio runs. It turns amber above 25 % and red above 50 %.
- **⬆ ⬇ 🗐 🗙.** Move up, Move down, Duplicate and Remove, as described above.

### The focused machine

Several commands act on the *focused* machine: the one you last worked on. Clicking a pattern pad, changing Len or Trans, or using a pattern dice button focuses that machine. Adding or duplicating a machine focuses the new one. If no machine has been focused, the first machine in the rack counts as focused.

## Patterns

A *pattern* is a loop of notes for one machine. Every machine has 128 patterns, arranged in eight banks, **A** to **H**, of 16 patterns each. A pattern is named by its bank and number: A01 is the first pattern and H16 the last. Each pattern holds up to 64 steps, where a step is a 1/16 note.

Each machine plays one pattern at a time: its *selected* pattern. In Pattern mode, every machine loops its selected pattern. In Song mode, the song's blocks choose the patterns. See [Song mode](song-mode.md).

### Selecting a pattern

Each panel has a row of bank buttons (**A**–**H**) above a row of pattern pads (**1**–**16**).

1. Click a bank letter to show that bank's 16 pads. This only changes which pads you see. It doesn't change what plays.
2. Click a pad to select that pattern. The machine becomes the focused machine.

The selected pad is highlighted. While playing, the pattern that's actually playing has its number lit in green, and a pattern waiting to start has its number lit in amber.

Selecting a pattern isn't an undo step, but it does count as an unsaved change, because the project file stores each machine's selected pattern.

### Queued switching

When you select a pattern while stopped, the change happens at once. When you select a pattern while playing in Pattern mode, the switch is *queued*: the machine finishes its current pattern and changes over when that pattern reaches its end. Each machine counts its own pattern's length, so machines switch independently.

In Song mode, the song decides what plays, so selecting a pad while the song plays doesn't change what you hear. With Rec on, the selection is recorded into the song. See [Song mode](song-mode.md).

### Length (Len)

The **Len** field beside the pads sets the selected pattern's length in steps, from 1 to 64. New patterns are 16 steps (one bar). Drag the field sideways, or click it and type a value.

Each machine loops its own pattern length. For example, a 12-step pattern against a 16-step pattern shifts against it on each pass, and both line up again every 48 steps.

Shortening a pattern doesn't delete the notes past its new end. They go silent and come back if you lengthen the pattern again. In Pattern mode, the dock's lane header shows "+N hidden" for them. See [The editor dock](editor-dock.md).

A Len drag is one undo step. Each typed value or arrow-key change is its own step.

### Transpose (Trans)

Acid synth panels also have a **Trans** field. It shifts the whole selected pattern up or down, from −12 to +12 semitones, without changing its notes. Each pattern has its own Trans setting. The other machine types don't have this field. Use the dock's transpose keys to move their notes. Song blocks have a separate transpose of their own (see [Song mode](song-mode.md)).

## Copying patterns

Copy a whole pattern from one machine or slot to another:

1. Select the pattern you want to copy, so its machine is focused.
2. Press **Ctrl+C**, or choose **Edit → Copy pattern**.
3. Select the pattern you want to replace. It can be on another machine.
4. Press **Ctrl+V**, or choose **Edit → Paste pattern**. The copied pattern replaces the selected one as a single undo step.

These keys copy whole patterns only when the editor dock doesn't have keyboard focus. With the dock focused, Ctrl+C and Ctrl+V copy and paste notes instead (song blocks in Song mode). Click anywhere outside the dock to give up its focus. See [The editor dock](editor-dock.md).

A pattern only pastes onto a matching kind of machine:

| Copied from | Pastes onto |
|---|---|
| Acid synth | Any acid synth, including Trans |
| Kit A or Kit B | Any drum kit, either kind |
| FM, Mono or Sampler | Any FM, Mono or Sampler |

Pasting onto another kind shows "The clipboard holds a different kind of pattern." The copied pattern stays available after **File → New** or **File → Open**, so you can paste it into another project.

## Dice and randomisers

Some buttons in the machine panels write a new pattern, or rewrite the selected one, from random choices. Each click gives a different result, and each click is a single undo step, so **Ctrl+Z** takes you back.

### Acid synth dice

Acid synth panels have a **Dice** row. Each button randomises part of the selected pattern:

| Button | What it changes |
|---|---|
| Notes | Gives the notes new pitches from a scale chosen at random (Minor, Melodic minor, Phrygian, Major or Hungarian minor), moving mostly in small steps around C. |
| Accent | Sets new accents. Steps where the paired drum kit has a kick or snare are more likely to get one. |
| Slide | Sets new slides. A slide makes the next slide more likely, so slides tend to chain. |
| Rest | Rewrites which 1/16 steps play and which rest. Steps on the paired kit's kick or snare are more likely to play. |
| Octave | Moves a few notes an octave up or down. |
| All | All of the above at once. |

The *paired drum kit* is the drum kit in the same position among the kits as the synth is among the acid synths: the first acid synth pairs with the first drum kit in the rack, the second with the second, and so on. The dice read that kit's selected pattern. With no paired kit, the dice still work, without favouring any steps.

The dice don't use the project's key and scale. Every die except Rest and All keeps each note's start and length. Rest and All rebuild the pattern on the 1/16 grid. After the Accent, Slide or All dice, a note that follows a slide never keeps an accent.

See [Acid synth](acid-synth.md) for the synth's other controls.

### Drum kit Template and Evolve

Drum kit panels have two buttons next to Len:

| Button | What it does |
|---|---|
| Template | Replaces the selected pattern with a fresh groove: one of several kick patterns, a snare or clap backbeat on steps 5 and 13, and one of several hi-hat patterns, repeated over the pattern's length. |
| Evolve | Makes one to three small changes: adds a hit (more often on the beat), moves a hit one step, or switches a hit between normal and accented. |

Both keep an open hat and a closed hat from sounding on the same step.

### Sound randomisers

The FM synth and the Mono synth each have a button that makes a new random sound rather than a new pattern. See [FM synth](fm-synth.md) and [Mono synth](mono-synth.md). The editor dock's Tools menu also has pattern generators, such as Randomize in scale and Euclidean, that follow the project key. See [The editor dock](editor-dock.md).

See also: [Getting started](getting-started.md) · [The editor dock](editor-dock.md) · [Song mode](song-mode.md) · [Mixer and effects](mixer-and-effects.md) · [Files and export](files-and-export.md)
