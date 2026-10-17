# Files and export

This chapter covers project files: creating, opening and saving them, what a `.sidacid` file holds, the unsaved-changes question, and opening files from older versions. It also covers undo and redo, exporting a loop or the whole song as a WAV file, and the `sidacid render-demo` command.

## The File menu

| Item | Key | What it does |
|---|---|---|
| New | Ctrl+N | Starts an empty project: six machines (Synth 1, Synth 2, Kit A, Kit B, FM, Sampler) with empty patterns, at 125 BPM with no swing. |
| Open… | Ctrl+O | Opens a `.sidacid` project file. |
| Save | Ctrl+S | Saves to the project's file. If the project has never been saved, it asks for a file name first. |
| Save As… | | Always asks for a file name, then saves there. Later saves go to the new file. |
| Export to WAV… | | Renders a loop or the whole song to a WAV file. See [Exporting to WAV](#exporting-to-wav). |
| Audio Settings… | | Chooses the audio driver and device. See [Audio settings](getting-started.md#audio-settings). |
| Quit | | Closes SidAcid. |

New, Open and Quit ask first if there are unsaved changes. See [Unsaved changes](#unsaved-changes). The keyboard shortcuts don't work while you're typing in a text field.

SidAcid starts with the built-in demo groove loaded, not an empty project.

## Saving

The save dialog suggests the project's name with `.sidacid` added. That's "Demo.sidacid" for the demo and "Untitled.sidacid" for a new project. The name of the file you save to doesn't change this suggestion.

If the name you type doesn't end in `.sidacid`, SidAcid adds it. For example, "beat" saves as `beat.sidacid` and "beat.v2" saves as `beat.v2.sidacid`. If a file with the extended name already exists, SidAcid asks "Replace file?" with **Replace** and **Cancel**, because the system's own save dialog only checked the name you typed.

While a save runs, the transport bar shows "Saving…". The project is written to a temporary file first, then moved into place, so a crash during a save never leaves a half-written project. If the save fails, a message names the file and the reason ("Could not save …").

The window title shows the file name without its extension (or the project's name, if it has never been saved), followed by "— SidAcid". A `*` after the name means there are unsaved changes.

## What a project file holds

A `.sidacid` file is a text file that holds the whole project:

- The tempo, swing, and the project key and scale.
- The seed for the chance dice. See [chance](editor-dock.md).
- Every machine in rack order, each with:
  - Its type and name.
  - Its selected pattern.
  - All 128 of its patterns.
  - Its knob settings and mixer channel settings.
  - How its lane looks in the editor dock: height, folded, strip shown, and Fold.
- Each FM machine's last loaded voice name and its imported voice bank.
- Each Mono machine's last loaded patch name.
- Each sampler's slots, *including the sample audio itself*.
- The send effects and master settings.
- The song: its blocks, length, loop range and automation lanes.

Because the sample audio is inside the file, a project with samples opens on another computer without the original sample files. The audio is kept as 16-bit samples at the sample's own rate, mono or stereo, compressed. Samples loaded from 24-bit or 32-bit files are saved at 16 bits. Opening a project whose decoded audio is larger than the computer's free memory is refused. See [Sampler](sampler.md).

Some things aren't saved in the project:

- The audio driver and device, and the editor dock's height. These belong to your computer and are kept in a separate settings file. See [Where the settings are kept](getting-started.md#where-the-settings-are-kept).
- Whether each machine panel is folded in the rack.
- The undo history.

## Unsaved changes

A project has unsaved changes when it differs from the last save. Undoing back to the saved state counts as no change. Selecting a different pattern also counts as a change, because the file stores each machine's selected pattern.

When there are unsaved changes, New, Open, Quit and closing the window ask first: "Save changes to "*name*" first?"

| Button | What happens |
|---|---|
| Save | Saves (asking for a file name if needed), then carries on with New, Open or Quit. If the save fails or you cancel the file dialog, the question comes back. |
| Don't save | Carries on without saving. |
| Cancel | Does nothing. |

## Opening files

**File → Open…** shows only `.sidacid` files. If a file can't be opened, a message reads "Could not open" with the file's path, followed by the reason:

| Message | Meaning |
|---|---|
| file error: … | The file couldn't be read. |
| not a valid SidAcid project: … | The file isn't a SidAcid project, or it's damaged in a way that stops it being read. |
| project version *n* is newer than this SidAcid (6) | The file was saved by a newer SidAcid. Update SidAcid to open it. |
| project is damaged: … | The file was read, but its contents don't make a valid project. |

Opening a file replaces the project and its undo history. A pattern you copied with **Edit → Copy pattern** stays available to paste.

### Files from older versions

SidAcid opens project files from every earlier version and converts them as they load. Older songs convert and play the same. Drum kits that still carry the default names older versions gave them are renamed to today's defaults, Kit A and Kit B, keeping any number after the name (for example "Kit A 2"). A name you typed yourself is kept.

When you save, the file is written in the current format. An older SidAcid can't open it after that ("project version 6 is newer than this SidAcid"). To keep a copy that older versions can open, use **Save As…** to save the converted project under a new name, and leave the original file as it is.

## Undo and redo

| Command | Keys | Edit menu |
|---|---|---|
| Undo | Ctrl+Z | Undo  (Ctrl+Z) |
| Redo | Ctrl+Y or Ctrl+Shift+Z | Redo  (Ctrl+Y) |

SidAcid keeps the last 200 undo steps. New and Open start a fresh history.

### What counts as one step

- **A drag** of a knob, the BPM, Swing %, a Len or Trans field, or anything else you drag.
- **A run of mouse-wheel turns** on a knob, or on a note in an acid synth's step grid. The step ends once the wheel has rested for 0.3 seconds.
- **Each typed value, or each arrow-key change,** in a Len or Trans field. In the BPM or Swing % field, everything you type or change with the arrow keys is one step until you click away from it.
- **Each rack change:** adding, duplicating, removing, moving or renaming a machine.
- **Each dice, Template or Evolve click,** and each pattern paste.
- **Loading** an FM voice or bank, a Mono patch, or a sample into a slot (or clearing one).
- **Applying** a note tool from the dock's Tools menu. While a tool's window is open, undo and redo wait until you click Apply or Cancel.
- **Each record pass** in Song mode. Everything recorded between starting and stopping is one step.

Selecting a pattern isn't an undo step.

The undo history also keeps up to 128 MB of sample audio, so that sample loads can be undone. If it holds more than that, the oldest steps are dropped first. The newest step is always kept.

## Exporting to WAV

Choose **File → Export to WAV…** to open the **Export to WAV** window. Pick what to render:

| Option | What it renders |
|---|---|
| Loop of the selected patterns | Every machine's selected pattern, playing as in Pattern mode, for the number of bars you set beside it (1 to 64 bars, default 8). |
| Whole song | The song from its start to its end. It's greyed out when the song is empty. |

Then click **Export…** and choose a file name. The suggested name is the project's name with "-loop.wav" or "-song.wav" added.

The export runs in the background, so you can keep working. The transport bar shows "Exporting 8 bars…" for a loop, or "Exporting song n%…" for the song. When it's done, a message says "Exported" with the file's path, or "Export failed:" with the reason. Only one export runs at a time: Export to WAV… is greyed out until the current one finishes.

Details of the rendered file:

- It's a 16-bit stereo WAV at 44,100 Hz, whatever rate your audio device runs at.
- It's rendered from the project as it was when you confirmed the file name, with its tempo, swing, mixer and effects.
- A bar is 16 steps, so a loop export is exactly that many bars of 1/16 steps at the project's tempo.
- After the music ends, delay and reverb tails ring out for up to 10 more seconds. The export stops early once the tail is silent.
- A song export ignores the song's loop range so that it always ends. It always starts from the song's beginning, and it stops after at most one hour.
- Audio louder than full scale is clipped.

## The render-demo command

`sidacid render-demo` renders the built-in demo groove to a WAV file without opening a window or an audio device:

```sh
sidacid render-demo            # writes demo.wav in the current folder
sidacid render-demo out.wav    # writes out.wav
```

From the source tree, run it with `cargo run --release -p sidacid -- render-demo out.wav`.

It renders 8 bars of the demo's patterns at 128 BPM, plus the effects tail, in the same format as a loop export: 16-bit stereo WAV at 44,100 Hz. It prints "wrote *path*" when done, or "render failed:" with the reason. Any other argument prints a short usage message.

See also: [Getting started](getting-started.md) · [The rack and patterns](rack-and-patterns.md) · [Song mode](song-mode.md) · [Sampler](sampler.md) · [Keyboard reference](keyboard-reference.md)
