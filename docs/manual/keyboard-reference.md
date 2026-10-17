# Keyboard reference

Every keyboard shortcut and mouse modifier in SidAcid, gathered in one place. Which keys apply depends on where the keyboard is: the whole app, the editor dock (after you click a lane), Song mode's lanes, or an acid synth's step grid while you step-write. Each section links to the chapter that explains it in full.

## Anywhere

| Key | What it does |
|---|---|
| Space | Play or stop. Pressing Stop while stopped moves back to the start of the loop range if there is one, otherwise to the song's first step. |
| Ctrl+N | New project. |
| Ctrl+O | Open a project. |
| Ctrl+S | Save. |
| Ctrl+Z | Undo. |
| Ctrl+Y or Ctrl+Shift+Z | Redo. |
| Ctrl+C / Ctrl+V | Copy or paste the selected pattern, when the dock doesn't have the keyboard. See [The rack and patterns](rack-and-patterns.md). |

Space doesn't play or stop while you are typing in a text field or step-writing into an acid synth's grid.

## Knobs and buttons

| Action | What it does |
|---|---|
| Drag up or down | Turn a knob. |
| Shift+drag | Turn a knob finely. |
| Mouse wheel | Step a knob. |
| Double-click | Reset a knob to its default. |
| Click **Fill** | Fill is on while the button is held. |
| Shift+click **Fill** | Latch Fill on or off. |

See [Getting started](getting-started.md) and [conditions](editor-dock.md#conditions).

## Editor dock: entering notes

Click a lane to give the dock the keyboard; click outside the dock to take it away. See [Keyboard note entry](editor-dock.md#keyboard-note-entry).

| Key | What it does |
|---|---|
| Z S X D C V G B H N J M | Write C to B in the cursor's octave. |
| Q (or ,) 2 W 3 E R 5 T 6 Y 7 U | Write C to B one octave up. |
| I | Write C two octaves up. |
| Left / Right | Move the cursor one snap (when nothing is selected). |
| Up / Down | Move the cursor one row (when nothing is selected). |
| Backspace or Delete | Delete the note under the cursor, or step back and delete the one before. |

## Editor dock: the selection

| Key | What it does |
|---|---|
| Left / Right | Nudge one snap. With Alt, one tick (1/64). |
| Up / Down | Transpose a semitone (one scale degree with Snap to scale), or move a drum hit one row. |
| Shift+Up / Shift+Down | Transpose an octave (melodic lanes). |
| Delete or Backspace | Delete the selection. |
| Esc | Clear the selection, or the ruler range. Cancels an open tool. |
| Ctrl+A | Select every note in the lane. |
| Ctrl+C / Ctrl+X | Copy or cut the selection (or every lane in a ruler range). |
| Ctrl+V | Paste at the keyboard cursor. |
| Ctrl+D | Duplicate right after the selection. |
| Tab / Shift+Tab | With a Tools window open: move into the window, or out to its Cancel button. |

## Editor dock: the mouse

| Action | What it does |
|---|---|
| Click empty space | Add a note one snap long. |
| Click a note / Shift+click | Select it / add it to or remove it from the selection. |
| Drag on empty space | Box-select. Shift+drag adds to the selection. |
| Drag a note | Move the selection. Alt+drag moves in single ticks. |
| Ctrl+drag a note | Move copies; the originals stay. |
| Drag a note's right edge | Resize the selection. Past the pattern's end, the pattern grows (up to 64 steps). Not on drum lanes. |
| Right-click a note | Delete it (the whole selection if it's selected). |
| Double-click a drum hit | Toggle its accent. |
| Drag along the ruler | Select a time range across every lane. |

See [The editor dock](editor-dock.md).

## Song mode

Click a song lane to give the dock the keyboard. See [Song mode](song-mode.md#keyboard-shortcuts-in-song-mode).

| Key | What it does |
|---|---|
| Left / Right | Move the selected blocks one bar. With Alt, one step. |
| Delete or Backspace | Delete the selected blocks. |
| Esc | Clear the selection, or the ruler range. |
| Ctrl+A | Select every block on the lane. |
| Ctrl+C / Ctrl+X | Copy or cut the selected blocks (or every lane in a ruler range). |
| Ctrl+V | Paste at the song cursor. |
| Ctrl+D | Duplicate right after the selection. |

| Action | What it does |
|---|---|
| Double-click empty space | Place the machine's selected pattern. |
| Double-click a block | Open its pattern in Pattern mode. |
| Drag a block | Move by bars. Alt+drag moves by steps. Ctrl+drag copies. |
| Drag a block's right edge | Fix its length (longer loops the pattern, shorter plays its start). |
| Right-click a block | Delete it. |

## Acid synth step-writing

Click a cell in the step grid's Note row to start. See [Step-writing](acid-synth.md#step-writing).

| Key | What it does |
|---|---|
| Z S X D C V G B H N J M | C to B in the writer's octave. |
| Q (or ,) 2 W 3 E R 5 T 6 Y 7 U | C to B one octave up. |
| I | C two octaves up, but never above the grid's upper octave. |
| Space | Rest, then move right. |
| `-` | Tie to the note before, then move right. |
| A / F | Toggle accent / slide on this step. |
| Left / Right | Move the cursor. |
| Backspace | Step back and make it a rest. |
| `[` / `]` | Writer's octave down / up. |
| Esc | Stop step-writing. |

## Machine names

| Key | What it does |
|---|---|
| Double-click a name | Rename the machine. |
| Enter / Esc | Keep / cancel the new name. |

See [The rack and patterns](rack-and-patterns.md).

See also: [Getting started](getting-started.md), [The editor dock](editor-dock.md), [Song mode](song-mode.md).
