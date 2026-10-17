# Getting started

This chapter covers building and starting SidAcid, choosing an audio driver and device, the layout of the window, the transport bar along the top, and a short walkthrough that gets the demo groove playing and changes it.

## Build and run

SidAcid is built from source with Rust's `cargo`. The README asks for Rust 1.88 or newer. On Linux you also need the ALSA development files and `pkg-config`.

From the top folder of the source tree:

```sh
cargo run --release -p sidacid                         # build and open the app
cargo run --release -p sidacid -- render-demo out.wav  # render the demo groove to a file
cargo test --workspace                                 # run the tests
```

The built program is `target/release/sidacid`. Run it with no arguments to open the window. It opens at 1280×800, and you can't make it smaller than 960×600.

`sidacid render-demo` renders the built-in demo groove to a WAV file without opening a window or an audio device. See [The render-demo command](files-and-export.md#the-render-demo-command).

### Audio drivers in the build

- **Linux** builds include ALSA, which also reaches PipeWire and PulseAudio, and JACK. JACK is loaded when the app starts. If the JACK library isn't installed, JACK isn't offered. The build only needs `pkg-config` to report "jack". The repository's `.cargo/config.toml` points `pkg-config` at a stub file in `support/pkgconfig/`, unless you set `PKG_CONFIG_PATH` yourself.
- **Windows** builds include WASAPI. ASIO is an optional build feature, Windows only. To build it:
  1. Download the ASIO SDK, unzip it, and set `CPAL_ASIO_DIR` to that folder.
  2. Install LLVM and set `LIBCLANG_PATH` to its `bin` folder.
  3. Build with `cargo build --release -p sidacid --features asio`.

## Audio settings

SidAcid starts its audio as soon as it opens. If you've never changed the audio settings, it uses the system's default driver and device at their own sample rate and buffer size.

To pick a driver and device, choose **File → Audio Settings…**, or click the **CPU** reading in the transport bar. The **Audio Settings** window shows the stream that's open now on its first line, for example `Now: ALSA / PipeWire · default · 48000 Hz · 256 frames (5.3 ms)`. Below that are these settings:

| Setting | What it does | Choices |
|---|---|---|
| Driver | The audio system to use. Picking a different driver resets the other three settings to Default. | Default, then every driver found, for example "ALSA / PipeWire", "JACK", "WASAPI", "ASIO" |
| Device | The output device. **Rescan** looks for devices again. **Show all devices** lists every ALSA device, including plugin devices such as resamplers and up/down-mixers. Without it, ALSA lists only `default`, `pipewire`, `pulse`, `jack` and devices whose names start with `sysdefault:`, `front:`, `hw:`, `plughw:` or `hdmi:`. | Default, then the devices found |
| Sample rate | The rate the stream runs at. Only the rates the device supports are listed. With JACK this reads "set by the JACK server". | Default (the device's own rate), 44100, 48000, 88200, 96000, 176400, 192000 Hz |
| Buffer | Frames per audio callback. Smaller buffers respond faster but use more CPU. The approximate latency is shown beside it (for example "≈ 5.3 ms"). With JACK this reads "set by the JACK server". | Default (the driver's own), 64, 128, 256, 512, 1024, 2048, 4096 frames, limited to what the device accepts |

The device list fills in a moment after the window opens. Until then it reads "Scanning devices…".

Click **Apply** to restart audio with the new settings. Playback stops. If the new settings fail to open, SidAcid goes back to the setup you had before, shows the error in red in the window, and saves nothing. If you click Apply without changing anything, the window just closes. **Cancel** or the window's close button throws away your changes.

### Where the settings are kept

Audio settings belong to your computer, not to a project. They're kept in `settings.ron`. On Linux that file is in `$XDG_CONFIG_HOME/sidacid/`, or `~/.config/sidacid/` when `XDG_CONFIG_HOME` isn't set. On Windows it's in `%APPDATA%\SidAcid\`. The same file also remembers the height of the editor dock.

When SidAcid starts and your saved driver, device, rate or buffer isn't available any more, it falls back step by step to what is there. Each fallback shows a short note at the right end of the transport bar, for example "Saved device '…' not found — using 'default'". The saved settings stay as they are, so they work again once the device is back.

### When there's no audio

If no stream can be opened, a red line appears under the transport bar: "No audio:" followed by the reason, with a **Retry audio** button. The app still works without audio: you can edit, save and export. If the audio device stops while you're working, SidAcid tries once to start it again by itself.

## The window

![The SidAcid window: transport bar, rack and editor dock](../images/screenshot.png)

From top to bottom, the window has three parts.

### Transport bar

The top row holds the **File** and **Edit** menus, the transport controls (see [The transport](#the-transport) below), and a performance readout:

| Reading | What it shows |
|---|---|
| CPU n% | The share of each audio callback's time spent making sound. It turns amber above 80 % and red at 100 %. Point at it to see the open driver, device, rate and buffer, plus the busiest machines. Click it to open Audio Settings. |
| pk n% | The worst callback in the last second, with the same colours. |
| MEM | The memory the app is using, against the computer's total. It turns amber, then red, as the system's free memory runs short. |
| ⚠ n | Shown only after audible dropouts. Point at it for details and click it to clear the count. |

Short notices, such as "Saving…", export progress and audio fallbacks, appear at the right end of this row. When the window is narrow, the row wraps onto a second line.

### The rack

The middle of the window is the rack: one panel per machine (the sound sources), in order, scrolling vertically. Each panel holds that machine's knobs, its pattern pads and its pattern tools. Below the last machine is **+ Add machine** with the machine count, then the **MIXER & FX** section, with a level meter for every channel and the master, and the **SONG** section. See [The rack and patterns](rack-and-patterns.md) and [Mixer and effects](mixer-and-effects.md).

Knobs and switches work the same way everywhere:

- Drag a knob up or down to turn it. Hold Shift while dragging for fine control.
- Scroll the mouse wheel over a knob to step it.
- Double-click a knob to reset it to its default.
- Point at a knob to see its value.
- Click a switch (a button showing its name and setting, such as "Lo-Fi Off") to step it to its next setting.

### Editor dock

The bottom part is the editor dock, where you edit notes. In Pattern mode, every machine's selected pattern is shown as a lane, stacked in rack order on one shared time ruler. In Song mode, the dock shows the song's lanes of pattern blocks instead. Drag the dock's top edge to resize it. It can take between 15 % and 85 % of the window height, and SidAcid remembers the size. See [The editor dock](editor-dock.md) and [Song mode](song-mode.md).

## The transport

The controls in the transport bar, left to right:

| Control | What it does | Range or positions |
|---|---|---|
| Play | Starts playback: in Pattern mode from the start of every pattern, in Song mode from the song start (see [Song mode](song-mode.md)). It's lit while playing. Clicking it while playing starts again from the same place. | |
| Stop | Stops playback. Clicking Stop while already stopped moves the song start back to the beginning of the song, or to the start of the loop range if the song has one. | |
| Fill | Turns fill on while you hold the button down. Shift-click latches it on, and Shift-click again turns it off. It's lit while on. Notes with the Fill or Not fill condition follow it. See [conditions](editor-dock.md). | Held, latched or off |
| Pattern / Song | Switches between Pattern mode, where each machine loops its selected pattern, and Song mode, where the song plays its blocks. Switching stops playback and turns Rec off. | Pattern, Song |
| Rec | Records pattern changes and knob moves into the song. It's available only in Song mode. See [Song mode](song-mode.md). | On, Off |
| Bar / Step | In Song mode only, shows the bar and step that's playing, or that playback will start from. | |
| BPM | The tempo. Drag it sideways, or click it and type a value. | 40–300 BPM, to one decimal place |
| Swing % | Delays every second 1/16 step by half the swing amount, as a share of a step. At the 75 % maximum, the delay is 3/8 of a step. | 0–75 % |
| Lo-Fi | A master switch that reduces the whole mix to 8-bit, 22.05 kHz sound. | Off (default), On |
| Master | The master output level. | 0–100 %, default 80 % |

Press **Space** to play or stop from anywhere, except while you're typing in a text field or step-writing notes into an acid synth's step grid. In the step grid, Space enters a rest.

A tempo, swing or knob drag is a single undo step. So is everything you type, or change with the arrow keys, in the BPM or Swing % field before you click away from it. See [Undo and redo](files-and-export.md#undo-and-redo).

## First sound in five minutes

SidAcid opens with a demo groove already loaded at 128 BPM with 15 % swing:

- **Synth 1** plays an acid line.
- **Synth 2** plays a higher counter-line.
- **Kit A** plays the beat.
- **Kit B** plays the hats.

Follow these steps:

1. Start SidAcid. If a red "No audio" line appears, open **File → Audio Settings…**, pick your device and click **Apply**.
2. Press **Space**, or click **Play**. The groove plays. Every machine loops its pattern **A01**.
3. In the **SYNTH 1** panel, drag the **Cutoff** knob up and down, then try **Reso** and **Env Mod**. Double-click a knob to put it back to its default.
4. In the same panel, click **Notes** in the **Dice** row. Synth 1's notes get new pitches, keeping their rhythm. Press **Ctrl+Z** to go back to the old one, or click again for another. See [Dice and randomisers](rack-and-patterns.md#dice-and-randomisers).
5. In the **KIT A DRUMS** panel, click pad **2** in bank **A**. While playing, the pad lights amber to show it's queued. Kit A switches to the empty pattern A02 when its current pattern reaches its end.
6. Click **Template** in the Kit A panel. A02 fills with a fresh beat. Click **Evolve** a few times for small changes, or **Template** again for a different beat.
7. Click pad **1** to go back to A01. Raise **Swing %** in the transport bar and listen to the groove shuffle.
8. Press **Ctrl+S** and choose a file name to save the project as a `.sidacid` file.

To start from an empty project instead, choose **File → New**. You get a rack of six machines with empty patterns at 125 BPM: Synth 1, Synth 2, Kit A, Kit B, FM and Sampler.

See also: [The rack and patterns](rack-and-patterns.md) · [Files and export](files-and-export.md) · [The editor dock](editor-dock.md) · [Mixer and effects](mixer-and-effects.md) · [Keyboard reference](keyboard-reference.md)
