# Song mode

Song mode turns your patterns into a whole track. Each machine gets a lane in the editor dock, and you arrange blocks along it; each block plays one of that machine's patterns. This chapter covers switching modes, placing and editing blocks, the song ruler and its range tools, how pattern lengths and block lengths work together, recording pattern changes and knob moves, automation, and how songs from older versions of SidAcid are converted.

## Pattern mode and Song mode

The **Pattern** and **Song** buttons in the transport bar switch modes.

- In **Pattern mode** each machine loops its selected pattern, and the dock edits notes (see [The editor dock](editor-dock.md)).
- In **Song mode** the machines play the song, and the dock edits its blocks.

Switching modes stops playback if it is running, turns **Rec** off, and clears the dock's ruler range and every selection.

In Song mode the transport bar also shows where playback is, or where it will start, as "Bar N  Step M". A bar here is 16 steps.

## The song dock

In Song mode the dock keeps its lanes, one per machine in rack order, with the same header (collapse arrow, name and the machine's selected pattern) and the same height handle as in Pattern mode. The Strip and Fold toggles are Pattern-mode only.

The dock header keeps **Zoom** and **Follow** (Follow now tracks the song playhead) and adds the song tools: **Capture**, **Loop**, **Insert time**, **Delete time**, **Clear loop** when there is a loop, and the block tools when one block is selected. Snap, the key picker, the Tools menu and the note inspector are hidden: in Song mode everything moves by bars.

Each lane shows its blocks over a grid with a line at every bar (and every four steps when zoomed in far enough). The lanes run on past the song's end, so there is room to place more.

## Blocks

A block plays one pattern of its machine from a given step. Each block is drawn as a rounded box in its machine's colour (see [Machines](rack-and-patterns.md#machines)), labelled with its pattern (for example "A03"), with a small picture of the pattern's notes inside. Muted blocks are dimmed.

Blocks on a lane never overlap. A gap between blocks is silence for that machine.

### Block length

A new block **follows its pattern's length**: it lasts as long as the pattern, and when you lengthen or shorten the pattern, the block grows or shrinks with it.

Drag a block's right edge to **fix its length** instead. The end snaps to the end of a bar; hold **Alt** to snap to single steps. A block is at least one step long and can't run into the next block.

- A fixed length **longer** than the pattern loops it. Thin lines inside the block mark where each repeat starts.
- A fixed length **shorter** than the pattern plays only its start (a cut).

Dragging the edge of a block in a selection resizes every selected block by the same amount. A block too narrow on screen to have a separate edge just moves; zoom in to resize it.

### When a pattern gets longer

When a pattern gets longer (from its **Len** box, by [dragging a note past its end](editor-dock.md#resizing-notes), or any other way), every block on that machine's lane that follows the pattern's length grows with it. Later blocks on the lane move right only as far as they need to: a gap after the block takes up the growth first. Blocks on other machines' lanes don't move. The pattern edit and the moved blocks are one undo step.

Shortening a pattern shortens its blocks and leaves gaps; nothing moves back. Blocks with a fixed length never grow.

If the growth would push a block past the song's maximum length, the pattern edit is refused with the message "That would push the song past its maximum length".

## Editing blocks with the mouse

| To | Do this |
|---|---|
| Place a block | Double-click empty space in a lane. The machine's selected pattern is placed at the start of that bar. If those steps are taken, it goes to the nearest free place. |
| Open a block's pattern | Double-click the block. SidAcid switches to Pattern mode with that pattern selected. |
| Select a block | Click it. Clicking empty space clears the lane's selection. |
| Add or remove a block from the selection | Shift-click it. |
| Box-select | Drag on empty space. Every block the box touches is selected. Hold Shift to add to the selection. |
| Move the selection | Drag a block. It moves by whole bars; hold Alt to move by single steps. |
| Copy the selection | Ctrl-drag a block: copies move and the originals stay. |
| Fix the selection's length | Drag a block's right edge (see [Block length](#block-length)). |
| Delete | Right-click a block. If it is selected, the whole selection goes; otherwise just that block. |

Selected blocks move together and keep their spacing. If they don't fit where you drag them, they go together to the nearest place on the bar grid (or the step grid, with Alt) where they all fit. A move dropped back where it started is not an undo step.

If a placed block fits nowhere on the lane, SidAcid says "There is no room on the song for that pattern".

## Keyboard shortcuts in Song mode

Click a lane to give the dock the keyboard. These keys act on the focused lane's selected blocks:

| Key | What it does |
|---|---|
| Left / Right | Move the selection one bar earlier or later. With Alt, one step. |
| Delete or Backspace | Delete the selection. |
| Esc | Clear the selection. With a ruler range, clears the range and every lane's selection. |
| Ctrl+A | Select every block on the focused lane. |
| Ctrl+C | Copy the selection. With a ruler range, copy every lane's blocks that start in it. |
| Ctrl+X | Cut: copy, then delete, as one undo step. |
| Ctrl+V | Paste at the song cursor (see [The ruler](#the-ruler)). |
| Ctrl+D | Duplicate. |

Blocks have their own clipboard, separate from the notes clipboard of Pattern mode.

- **Paste** puts blocks copied from one lane onto the focused lane. Blocks copied with a ruler range go back onto the machines they came from (machines no longer in the rack are skipped). Everything goes by one offset: at the song cursor, or the nearest bar where it all fits. A pasted block that follows its pattern's length and would now run into the next pasted block is cut to fit, and the status line says so. A paste that fits nowhere is refused with "There is no room on the song for those blocks".
- **Duplicate** pastes a copy of the selection right after it, one selection length later rounded up to whole bars. With a ruler range, it repeats every lane's blocks in the range one range length later, and the range moves onto the copies.

The pasted or duplicated blocks become the selection. Each paste, cut and duplicate is one undo step.

## The selected block

When exactly one block is selected (over all lanes), the dock header shows its tools:

| Control | What it does | Range |
|---|---|---|
| Pattern | Two lists: the bank letter and the pattern number. The block plays the pattern you pick, from the same start. | A to H, 01 to 16 |
| Mute | Silences the block. Muted blocks are dimmed. | on / off |
| Trans (− and +) | Transposes the block a semitone at a time. | −12 to +12 |

**Trans** only appears for machines that transpose: the acid synths, the FM synth and the Mono synth. Drum kits and the sampler have no song transpose.

If a block that follows its pattern's length gets a longer pattern that would run into the next block, it is given a fixed length that ends there, and the status line says "The block was shortened to fit before the next one".

Each change is one undo step.

## The ruler

In Song mode the ruler shows bar numbers. Along it you can see:

- the **song cursor**, a small triangle at the top: where playback starts,
- the **loop**, a coloured line along the top edge,
- the **End** marker, where playback stops,
- the playhead while playing,
- the picked range, as a band.

**Click** the ruler to move the song cursor to the start of that bar; hold **Alt** to put it on a step. When you press Play, the song starts from the cursor (or from the start of the song's last bar, if the cursor is past it). Pressing **Stop** while already stopped moves the cursor back to the loop's start, or to the start of the song when there is no loop.

**Drag** along the ruler, above the thumb strip, to pick a range of whole bars. The blocks starting inside it on every lane become the selection. A click on the ruler clears the range. The thumb strip along the bottom of the ruler scrolls the lanes, as in Pattern mode.

### Range tools

These buttons need a range; until you pick one they are greyed out.

| Button | What it does |
|---|---|
| Loop | Loops the picked bars. Playback that reaches the loop's end jumps back to its start. |
| Insert time | Pushes everything from the range's start right by the range's length, on every lane: blocks, automation, the loop and the song's end. |
| Delete time | Removes the picked bars from every lane and closes the gap. |

**Clear loop** removes the loop.

Some details:

- A loop reaching past the song's end makes the song longer, so it still loops instead of stopping.
- A playback started at or past the loop's end plays on to the song's end without looping.
- **Insert time** doesn't split a block that the range starts inside: that block stays where it is, and the new time opens after it. Only as much time as fits before the song's maximum length is inserted; the status line says when only part of it fit.
- **Delete time** removes blocks that start inside the range, cuts a block that runs into it, and shortens a block that runs right through it. Automation inside the range is removed. A loop whose start or last step is inside the range is cleared.

### Capture

**Capture** places every machine's selected pattern at the song cursor, each on the nearest free start of its lane, as one undo step. The placed blocks become the selection. If one of them fits nowhere, nothing is placed and SidAcid says so.

## The song's end

The song ends at the end of its last block, on any lane. A song converted from an older version may have its own, later end (see [Older songs](#older-songs)), and a loop past the last block also counts. The ruler marks the end with **End**. When playback reaches it, every machine is released and the transport stops.

A song can be at most 63,936 steps long (3,996 bars of 16 steps).

The **Song** panel at the bottom of the rack shows the song's length in bars, as "SONG   N bars".

## Recording

You can play the song and record into it: pattern changes become blocks, and knob moves become automation.

1. Switch to **Song** mode. **Rec** only works in Song mode.
2. Click **Rec** so it lights.
3. Press **Play**. Recording happens while Song mode, Rec and playback are all on.
4. Click a machine's pattern pads to record pattern changes, and hold and move knobs to record automation.
5. Press **Stop**, or click **Rec** again, to end the pass.

Everything one pass records (pattern changes, knob moves, and any tempo or swing changes you make during it) is one undo step.

### Recording pattern changes

Clicking a pattern pad while recording writes that pattern into the song at the start of the current 16-step bar:

- If a block covers that point, it is split there. The left part keeps its pattern with a fixed length. The right part switches to the new pattern, and so does each following block that runs straight on from it with the same old pattern. They keep their positions and lengths.
- In a gap, a new block of the pattern goes there, cut short if it would run into the next block.

### Recording knob moves

While you hold a knob during a pass, its value is written at every step that plays, for as long as you hold it. Only the knobs you touch are written; the automation of every other knob is left alone. While you hold a knob during a pass, its existing automation doesn't move it.

### Automation

Each automated knob has an automation lane: a value at each step that was recorded. When the song plays, a knob takes its lane's value at each recorded step and keeps the last value until the next one. Before its lane's first recorded step, the knob is at its own setting. When playback starts, or the loop jumps back, every automated knob is set to the value it should have at that point.

The **Song** panel in the rack lists the automated knobs, with how many steps each lane holds. **Clear** next to a knob removes its lane, and **Clear all automation** removes every lane. Each is one undo step.

## The arpeggiator in Song mode

In Song mode the Mono synth's arpeggiator restarts at the start of each block, so its pattern lines up with the block. In Pattern mode it runs from when you press Play. See [Mono synth](mono-synth.md).

## Older songs

Older versions of SidAcid stored a song as a row of bars, each naming a pattern for every machine. When you open such a project, its song is converted to lanes of blocks, and it plays the same:

- Each bar becomes a block on each machine that had a pattern in it, keeping that bar's mute and transpose.
- A bar whose length differed from its pattern's length becomes a block with that fixed length.
- The song keeps its old end, so empty bars at the end still play.
- The loop and the automation carry over.

When you save the project, it is saved in the current format.

See also: [The editor dock](editor-dock.md), [The rack and patterns](rack-and-patterns.md), [Mono synth](mono-synth.md), [Files and export](files-and-export.md), [Keyboard reference](keyboard-reference.md).
