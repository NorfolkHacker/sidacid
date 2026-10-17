# The editor dock

The editor dock is the panel along the bottom of the window where you write and edit notes. Every machine in the rack has a lane in it, and all the lanes share one time ruler, so you can see and edit the whole groove at once. This chapter covers the dock's layout, adding and editing notes with the mouse and the keyboard, the project key and scale, the expression strips, the note inspector, chance, ratchet and conditions, and the Tools menu. In Song mode the same dock shows song blocks instead; see [Song mode](song-mode.md).

## Layout

From top to bottom the dock holds:

1. The header row: **Snap**, the **Key** picker and **Snap to scale** (Pattern mode only), **Zoom** and **Follow**. In Song mode the song tools take the place of Snap and Key (see [Song mode](song-mode.md)).
2. In Pattern mode, a second row: the **Tools** menu and the note inspector. This row is there with or without a selection, so the lanes don't jump when you select notes.
3. The ruler, with step numbers every four steps and a thin thumb strip along its bottom edge.
4. The lanes, one per machine, in rack order. When they don't all fit, a vertical scrollbar on the right scrolls the stack.

The lane of the focused machine has an outline. Clicking in a lane focuses its machine.

### Resizing the dock

Drag the dock's top edge up or down to change its height. The dock can take from 15% to 85% of the window's height. SidAcid keeps the dock's share of the window in its settings file, so the dock keeps its proportion when you resize the window and when you start SidAcid again.

## Lanes

A lane shows one machine's selected pattern as a piano roll: time runs left to right and each row is a pitch (or, on a drum lane, an instrument). The grid has a line at every 1/16 step and a stronger line every four steps. The part of the lane past the pattern's end is shaded. While the machine plays, its current step is lit.

### The lane header

Each lane starts with a header row:

| Control | What it does |
|---|---|
| Collapse arrow | Folds the lane down to its header, or shows it again. |
| Name | The machine's name. |
| Pattern label | The pattern the lane shows, for example "A01". |
| "+N hidden" | Shown when N notes start past the pattern's end (see [Notes past the pattern's end](#notes-past-the-patterns-end)). |
| Vel / Acc/Slide / Acc, Chance, Ratchet | What the expression strip shows. Only there while the strip is open. |
| Fold | Melodic lanes only: shows just the rows on the key's scale, plus rows that hold notes (see [Fold](#fold)). |
| Strip | Opens or closes the lane's expression strip (see [Expression strips](#expression-strips)). |

Under the lane is a thin handle. Drag it to change the lane's height, from 48 to 480 points (a new lane is 120 points tall). The lane's height and its collapse, Strip and Fold settings are saved with the project. Which strip view you picked (Vel, Chance or Ratchet) is not saved.

### Notes past the pattern's end

When you shorten a pattern, the notes that now start past its end are kept, but they don't play and the lane doesn't draw them. The header shows how many as "+N hidden" (hover over it for a reminder). Lengthen the pattern again and they come back. You can't add a note past the pattern's end by clicking there.

## Snap, zoom and scrolling

### Snap

**Snap** sets the grid that clicks, moves, resizes and the keyboard cursor use: **1/4**, **1/8**, **1/16**, **1/32** or **1/64** of a bar. The default is 1/16, one step.

A step is divided into four ticks, so 1/64 is a single tick, the finest position a note can have. Hold **Alt** while you drag notes, or while you nudge them with the arrow keys, to move by single ticks whatever the snap.

### Zoom and Follow

The **−** and **+** buttons next to **Zoom** step the horizontal zoom through ×0.5, ×0.75, ×1, ×1.5 and ×2. The default is ×1.

**Follow** scrolls the lanes with the playhead while playing. In Pattern mode it follows the focused machine's playhead. The view stays put until the playhead reaches the last quarter of it, then jumps so the playhead sits a quarter of the way in.

### Scrolling

- **The thumb strip** along the bottom of the ruler is the horizontal scrollbar for every lane. The lighter thumb shows which part of the longest pattern is in view; drag on the strip to scroll.
- **The mouse wheel** over a melodic lane scrolls its pitches up and down. Each melodic lane has its own pitch scroll.
- **A horizontal wheel, or Shift with the wheel**, over a lane scrolls every lane left and right.
- **The wheel over a drum lane** scrolls the stack of lanes, since a drum lane always shows all eight rows.

The first time a melodic lane is drawn, it centres on its notes. An empty lane centres on C2 for an acid synth or the Mono synth, C3 for the FM synth, and the lowest loaded key for the sampler.

## Melodic and drum lanes

### Melodic lanes

The acid synths, the FM synth, the Mono synth and the sampler have melodic lanes: one row per MIDI pitch, with pitch 127 at the top. A key strip down the left edge shows the keyboard, with every C named (for example "C3").

- **FM, Mono and sampler lanes** let notes overlap freely, so you can stack chords. (The Mono synth itself plays one note at a time; see [Mono synth](mono-synth.md).) Clicking and holding the key strip of one of these lanes plays that pitch so you can hear it.
- **Acid synth lanes** keep one note at a time. A note you add or move cuts the note sounding into its start and removes notes that start inside it. Accented notes have a coloured line along their top, and a note with slide on is joined to the next note by a line, drawn as a tie, when that next note starts exactly where it ends. Acid lanes have no velocity; they use accent and slide instead (see [Acid synth](acid-synth.md)).

Note colour gets brighter with velocity.

On FM, Mono, sampler and drum lanes, notes of the same pitch (or drum) that overlap after an edit merge into one note covering both.

### Sampler row labels

On a sampler lane, rows that play a loaded slot are tinted in that slot's colour, and the key strip names them: the first four letters of the sample's name, then the slice number in Slice mode (for example "Amen ·05") or the note name in Chromatic mode. While any sampler is in the rack, every lane's key strip is wider so the grids still line up. See [Sampler](sampler.md).

### Drum lanes

Kit A and Kit B have drum lanes with eight rows, labelled BD, SD, LT, HT, CP, RS, CH and OH, with the bass drum at the bottom. A hit can sit at any snap position. Drum hits keep their length: you can't resize them by dragging. A hit's colour shows whether it is accented. Drum lanes ignore the project key. See [Drum kits](drum-kits.md).

### Pad lanes

A Pad Kit has a pad lane with 16 rows, one per pad, labelled with the pads' names and with pad 1 at the bottom. It works like a drum lane, except that its notes can be resized (which matters for Gate pads) and have a velocity: the **Vel** strip shows it, and notes are shaded by velocity. Double-click a note to turn its accent on or off; an accented note has a coloured line along its top. The Invert tool isn't available, and the lane ignores the project key. See [Pad Kit](pad-kit.md#the-pad-lane).

## Editing notes with the mouse

| To | Do this |
|---|---|
| Add a note | Click an empty cell. The note is one snap long, starts on the snap grid and has velocity 100. |
| Select a note | Click it. |
| Add or remove a note from the selection | Shift-click it. |
| Box-select | Drag on empty space. Every note the box touches is selected. Hold Shift to add to the selection. |
| Move the selection | Drag a selected note (or drag an unselected note to select and move it). Hold Alt to move by single ticks. |
| Copy the selection | Ctrl-drag a note: copies move and the originals stay. |
| Resize the selection | Drag a note's right edge. Not on drum lanes (pad lanes can). |
| Delete | Right-click a note. If it is selected, the whole selection goes; otherwise just that note. |
| Toggle a drum hit's or pad note's accent | Double-click the hit. |

A press anywhere in a lane also focuses that machine, gives the dock the keyboard and puts the keyboard cursor where you clicked.

A move keeps the selection together: if one note would go before the start or off the top or bottom of the pitch range, the whole group stops there. With [Snap to scale](#snap-to-scale) on, the note you grab follows the pointer from scale degree to scale degree, and the rest of the selection snaps to the scale too.

A pattern holds at most 512 notes. An add, paste, duplicate or Ctrl-drag that would go past that is refused with the message "That would put more than 512 notes in the pattern".

### Resizing notes

Drag the right edge of any selected note to make every selected note longer or shorter by the same amount. The grabbed note's end follows the pointer on the snap grid (with Alt, single ticks). A note is never shorter than one tick.

You can drag a note's end past the pattern's end. The pattern then gets longer, in whole steps, so that it holds the notes, up to the longest pattern of 64 steps; a note can't run past step 64. Drag back within the same drag and the pattern goes back to its old length. The note change and the length change are one undo step.

A longer pattern also lengthens its blocks in the song, which can push later blocks along (see [Song mode](song-mode.md#when-a-pattern-gets-longer)).

## The ruler and range selection

In Pattern mode, drag along the ruler, above the thumb strip, to pick a time range across every lane. The range snaps to the current snap and shows as a band over the ruler and every lane. Every lane's notes that start inside the range become that lane's selection.

A click on the ruler removes the band but keeps the selections it made. Clicking in a lane drops the range and the selections it made in the other lanes.

With a range picked, Ctrl+C, Ctrl+X and Ctrl+D act on every lane at once (see [Copy, cut, paste and duplicate](#copy-cut-paste-and-duplicate)).

## Keyboard note entry

Click a lane to give the dock the keyboard. A white box, one snap wide, shows the keyboard cursor. Clicking anywhere outside the dock takes the keyboard away again. Space (play and stop) and Ctrl+Z still work while the dock has the keyboard.

The note keys use the tracker layout, two rows of a piano keyboard:

| Keys | Notes |
|---|---|
| Z S X D C V G B H N J M | C, C#, D, D#, E, F, F#, G, G#, A, A#, B in the cursor's octave |
| Q (or ,) 2 W 3 E R 5 T 6 Y 7 U | C to B one octave up |
| I | C two octaves up |

Each key writes a note one snap long at the cursor (cut short at the pattern's end), with velocity 100, then moves the cursor one snap to the right. The octave is the one the cursor's row is in; after a note, the cursor sits on the pitch you typed. On a drum lane every note key writes a hit on the cursor's row. On an acid lane the new note replaces what it overlaps, as a click does.

When nothing is selected:

| Key | What it does |
|---|---|
| Left / Right | Move the cursor one snap. It wraps around at the pattern's ends. |
| Up / Down | Move the cursor one row (one pitch, or one drum). |
| Backspace or Delete | Deletes the note under the cursor. If there is none, steps the cursor back one snap and deletes the note there, so pressing it again unwinds what you typed. |

On a folded lane, Up and Down go to the next shown row. With Snap to scale on, a typed note lands on the nearest scale degree.

## Keyboard shortcuts in the dock

These work while the dock has the keyboard and the focused lane has a selection. With no selection, the arrow keys and Backspace move the cursor and delete as in [Keyboard note entry](#keyboard-note-entry).

| Key | What it does |
|---|---|
| Left / Right | Nudge the selection one snap earlier or later. With Alt, one tick. |
| Up / Down | Transpose the selection a semitone, or on a drum lane move it one row. With Snap to scale on, a melodic selection moves one scale degree instead. |
| Shift+Up / Shift+Down | Transpose the selection an octave (melodic lanes only). |
| Delete or Backspace | Delete the selection. Holding the key down doesn't go on to delete more notes. |
| Esc | Clear the selection. With a ruler range, clears the range and every lane's selection. With a tool open, cancels the tool. |
| Ctrl+A | Select every note in the focused lane's pattern. |
| Ctrl+C | Copy (see below). |
| Ctrl+X | Cut. |
| Ctrl+V | Paste at the keyboard cursor. |
| Ctrl+D | Duplicate. |

The selection moves as one: when one of its notes can't go any further, none of them goes further.

Undo and redo wait while you are dragging notes. For every shortcut in SidAcid, see the [keyboard reference](keyboard-reference.md).

### Copy, cut, paste and duplicate

- **Ctrl+C** copies the focused lane's selection. With nothing selected it copies every note in the lane's pattern. With a ruler range it copies every lane's notes that start in the range.
- **Ctrl+X** copies and then deletes, as one undo step. It needs a selection or a range; with neither it does nothing.
- **Ctrl+V** pastes at the focused lane's keyboard cursor, and the pasted notes become the selection. Notes copied from one lane paste into the focused lane. Melodic notes paste onto any melodic lane, and drum hits only onto drum lanes; otherwise the paste is refused with a message. Notes copied with a range paste back into the machines they came from, at the cursor's position.
- **Ctrl+D** pastes a copy of the selection right after it: one selection length later, rounded up to the snap. With a range, it repeats every lane's notes in the range one range length later, and the range moves onto the copies, so pressing it again carries on.

Pasted notes that would start past step 64 are dropped. When the dock doesn't have the keyboard, Ctrl+C and Ctrl+V copy and paste whole patterns instead (see [The rack and patterns](rack-and-patterns.md)).

## Key and scale

The project has a key: a root and a scale. Pick them in the dock header (Pattern mode) under **Key**:

- **Root:** C, C#, D, D#, E, F, F#, G, G#, A, A# or B.
- **Scale:** Chromatic, Minor, Melodic minor, Phrygian, Major or Hungarian minor.

| Scale | Semitones above the root |
|---|---|
| Chromatic | all twelve |
| Minor | 0 2 3 5 7 8 10 |
| Melodic minor | 0 2 3 5 7 9 11 |
| Phrygian | 0 1 3 5 7 8 10 |
| Major | 0 2 4 5 7 9 11 |
| Hungarian minor | 0 2 3 6 7 8 11 |

With any scale but Chromatic, melodic lanes shade the rows off the scale darker, draw a line under each root row and name the root in the key strip. Drum lanes ignore the key. Changing the key is one undo step, and the key is saved with the project. The key also guides **Randomize in scale** in the Tools menu.

### Snap to scale

**Snap to scale** makes new, typed and dragged notes land on the key's scale, and makes Up and Down transpose by scale degrees (Shift+Up and Shift+Down still move a whole octave). It also makes **Invert** in the Tools menu snap its result to the scale. It only acts on melodic lanes, and does nothing while the scale is Chromatic. Snap to scale is saved with the project, but turning it on or off is not an undo step.

### Fold

The **Fold** toggle in a melodic lane's header hides the rows that are off the key's scale. Rows that hold notes stay visible, so every note keeps its row. With the Chromatic scale nothing is hidden. On a folded lane the keyboard cursor always sits on a shown row: a hidden pitch counts as the shown row below it. Fold is saved per lane with the project.

## Expression strips

Turn on **Strip** in a lane's header to show an expression strip under the grid. The selector in the header picks what it shows:

| View | Lanes | What it shows and does |
|---|---|---|
| Vel | FM, Mono, sampler, Pad Kit | One bar per note, its height the velocity (1 to 127). Click or drag to set the velocity of the notes starting in the snap cell under the pointer. |
| Acc/Slide | Acid synths | Two rows, Acc and Slide. A mark shows where a note has accent (at its start) or slide (at its end). Click a row to turn accent or slide on or off for the note sounding there. |
| Acc | Drum kits | A mark on each snap cell holding an accented hit. Click a cell to accent every hit starting in it, or, when they all are accented already, to clear them. |
| Chance | Every lane | One bar per note, its height the chance (0 to 100%). Click or drag to set it for the notes starting in the snap cell under the pointer. |
| Ratchet | Every lane | One bar per note, its height the ratchet (1 to 8). Click or drag as for Chance. |

The strip labels itself in the key-strip column: "Vel", "Chc" or "Rat" for the bar strips. A drag across a bar strip is one undo step.

## The note inspector

In Pattern mode, with notes selected in the focused lane, the second header row edits all of them at once. With nothing selected it says "Select notes to edit them here".

| Control | What it does | Range |
|---|---|---|
| Vel | Velocity. Not on acid synth lanes. | 1 to 127 |
| Chance | How likely each note is to play on a pass (see [Chance](#chance)). | 0 to 100% |
| Reroll | New dice for every chance in the whole project. | – |
| Ratchet | How many times each note re-triggers (see [Ratchet](#ratchet)). | ×1 to ×8 |
| Cond | The notes' condition (see [Conditions](#conditions)). | Always, 1:2 to 8:8, First, Not first, Fill, Not fill, Pre, Not pre |
| Accent | Accent on or off. Acid synth and drum lanes. | on / off |
| Slide | Slide on or off. Acid synth lanes. | on / off |

Drag a number or type into it. When the selected notes differ in a field, it shows "–" (Accent and Slide show "Accent –" and "Slide –"); setting it gives every selected note the same value. Each change is one undo step (a whole drag on a number is one step), and the notes stay selected. The inspector is greyed out while a tool is open.

## Chance, ratchet and conditions

Every note on every machine has a chance, a ratchet and a condition. When a note's start comes round, its condition is checked first. Only if the condition passes is its chance rolled. A note that fails either is skipped for that pass.

### Chance

Chance is how likely the note is to play each time its pattern comes round, from 0% (never) to 100% (always, the default).

The dice are fixed. Each roll is worked out from the project's seed, the machine's place in the rack, the pass number and the note's start and pitch. So the same project plays the same way every time you press Play, and renders and exports the same way too. Moving a note to another start or pitch, or moving its machine in the rack, gives it different dice.

**Reroll**, next to Chance in the inspector, gives the project a new seed, so every chance in the project rolls differently. It is one undo step, and the seed is saved with the project.

### Ratchet

Ratchet splits a note into 1 to 8 hits, evenly spaced over the note's length. Hits can land between ticks: a one-step note with ratchet 8 plays eight hits a 1/128 note apart. On the acid synths each hit is a new gate, and only the last one can slide into the next note.

### Conditions

A condition says on which passes a note may play. A pass is one time through the machine's pattern.

| Condition | The note plays |
|---|---|
| Always | on every pass (the default). |
| a:b (1:2, 2:2, 1:3 … 8:8) | on pass a of every b passes. 1:4 plays on passes 1, 5, 9 and so on; 2:2 plays on passes 2, 4, 6 and so on. |
| First | on the first pass only. |
| Not first | on every pass but the first. |
| Fill | only while Fill is on. |
| Not fill | only while Fill is off. |
| Pre | only if the machine's previous conditional note played. |
| Not pre | only if the machine's previous conditional note did not play. |

A conditional note is one with a condition other than Always, or a chance below 100%. Pre and Not pre look at the last conditional note on the same machine before this one; notes that always play don't count. Every conditional note, Pre and Not pre notes included, becomes the "previous" for the next one. After the pass count restarts, Pre counts as "did not play".

### How passes are counted

Each machine counts the passes of its own pattern. The count starts again:

- when you press Play,
- when a queued pattern switch takes over,
- in Song mode, at the start of each of the machine's blocks.

In Song mode, a block longer than its pattern plays the pattern several times, and each time counts as a pass. When playback starts in the middle of a block, or the song loop jumps back into one, the count is what it would be had the block played from its start. A machine added while the transport is running counts as if it had been playing from the start.

### The Fill button

The **Fill** button in the transport bar turns fill on while you hold it down. **Shift-click** it to latch fill on, and Shift-click again to unlatch it. The button lights while fill is on. Fill and Not fill notes follow it.

### Note badges

A note with a chance below 100% or a condition other than Always has a dashed white outline. Where the note is wide enough, a badge inside it shows what is set, for example "1:4 50% ×3":

| Badge | Meaning |
|---|---|
| 1:4 | condition a:b |
| 1st / !1st | First / Not first |
| F / !F | Fill / Not fill |
| P / !P | Pre / Not pre |
| 50% | chance |
| ×3 | ratchet |

A note with only a ratchet has a badge but no dashed outline.

## The Tools menu

In Pattern mode, the **Tools** menu at the start of the second header row runs a tool on the focused lane. A tool acts on the lane's selection. With nothing selected it acts on every note in the pattern, and the tools that make new notes (Randomize in scale and Euclidean) fill the whole pattern. Tools never touch notes hidden past the pattern's end, and the timing tools keep notes inside the pattern.

Picking a tool opens a small window with its settings. The lane shows the result live as you change them, and the result is selected.

- **Apply** keeps the result as one undo step.
- **Cancel**, closing the window, or **Esc** puts the pattern and the selection back as they were. Esc works even after a click in the tool's window. When a list in the window is open, the first Esc closes the list.

While a tool is open, nothing else edits that lane: the mouse, the strips, the inspector, the key picker, the clipboard keys and undo all wait. Focusing another lane, picking another pattern for the machine, or switching to Song mode cancels the tool. The window remembers each tool's settings for the next time during the session.

Hover over a menu item for a short description. A tool that can't work on the focused lane is greyed out, and hovering says why.

| Tool | Lanes |
|---|---|
| Quantize, Humanize, Legato, Reverse, Randomize in scale, Euclidean | every lane |
| Invert | melodic lanes ("Melodic lanes only") |
| Strum, Arpeggiate | FM, Mono and sampler lanes ("Only on lanes that play chords") |

### Quantize

Moves note starts toward the nearest grid line.

| Setting | What it does | Range |
|---|---|---|
| Grid | The grid to move toward. | 1/4, 1/8, 1/16 (default), 1/32, 1/64 |
| Strength | How far of the way each start moves. | 0 to 100% (default 100%) |
| Quantize ends | Moves note ends toward the grid as well. | on / off (default off) |

### Humanize

Adds small random shifts to each note's timing and velocity.

| Setting | What it does | Range |
|---|---|---|
| Timing ± | The most a start moves either way. | 0 to 8 ticks (default 2) |
| Velocity ± | The most a velocity changes either way. | 0 to 40 (default 10) |
| Seed | Which random shifts you get: the same seed gives the same result. **New seed** picks another. | 0 to 99999 |

### Legato

Each note runs to the start of the next note in the lane, at any pitch and selected or not; the last note runs to the pattern's end. It has no settings.

### Reverse

Plays the notes backwards: they mirror in time over the span from the first note's start to the last note's end. It has no settings.

### Invert

Mirrors the notes' pitches within their range, so the highest note becomes the lowest. With Snap to scale on (and a scale other than Chromatic), the results then move to the nearest scale degree. It has no settings.

### Strum

Staggers the notes of each chord (notes that start together), one after another in pitch order. Each note keeps its end where it can.

| Setting | What it does | Range |
|---|---|---|
| Ticks apart | The gap between one chord note and the next. | 1 to 8 (default 1) |
| Order | **Up** starts with the lowest note, **Down** with the highest. | Up (default), Down |

### Randomize in scale

Replaces the notes with new random ones. It works over the selected notes' span (or the whole pattern), with one possible note on every grid line of the dock's snap.

| Setting | What it does | Range |
|---|---|---|
| Density | The chance of a note on each grid line. | 10 to 100% (default 50%) |
| Lowest, Highest | The pitch range. Melodic lanes only. | C-1 to G9 (default C3 to C5) |
| Shortest, Longest | The note lengths, in units of the dock's snap. Melodic lanes only. | 1 to 16 (default 1 to 2) |
| Seed | Which notes you get: the same seed gives the same notes. **New seed** picks another. | 0 to 99999 |

Pitches come from the project key's scale within the range (any pitch when the scale is Chromatic). If the range holds no note of the scale, the nearest scale note is used. On drum lanes each new hit is one snap long on a random instrument. On an acid lane, the result is kept to one note at a time.

### Arpeggiate

Turns each chord (two or more notes starting together) into a run of its notes, from the chord's start to its last note's end. Single notes are left alone. Each note of the run copies the settings of the chord note whose pitch it takes.

| Setting | What it does | Range |
|---|---|---|
| Style | The order of the pitches. **Up-down** doesn't repeat the top and bottom notes. **Random** picks a pitch each time. | Up (default), Down, Up-down, Random |
| Rate | The time from one note of the run to the next. | 1/4, 1/8, 1/16 (default), 1/32, 1/64 |
| Gate | Each note's length, as a share of the rate. | 10 to 100% (default 50%) |
| Seed | Shown for the Random style: the same seed gives the same run. **New seed** picks another. | 0 to 99999 |

### Euclidean

Spreads a number of hits as evenly as they go over a number of grid cells, for example 3 hits over 8 cells gives x..x..x. (x is a hit). The cycle repeats across the selected notes' span, or the whole pattern. One cell is the dock's snap; the window reminds you of it. Each hit is one cell long with velocity 100. The notes the tool acts on (the selection, or every note) that sit on its pitch or row are replaced; other notes stay.

| Setting | What it does | Range |
|---|---|---|
| Steps | The cells in one cycle. It can't be more than the cells in the span. | 1 to 64 (default 8) |
| Hits | How many hits in one cycle. | 0 to Steps (default 3) |
| Rotation | Shifts the cycle later by this many cells. | 0 to Steps − 1 (default 0) |
| Pitch (melodic lanes) or Row (drum lanes) | Where the hits go. It starts on the keyboard cursor's pitch or row. | any pitch / BD to OH |

See also: [Song mode](song-mode.md), [The rack and patterns](rack-and-patterns.md), [Acid synth](acid-synth.md), [Drum kits](drum-kits.md), [Sampler](sampler.md), [Pad Kit](pad-kit.md), [Keyboard reference](keyboard-reference.md).
