# Audio driver & device settings — design

Date: 2026-10-14. Step 2 of 3 in the "modular SidAcid" series:
1. Compact performance meter (done, branch `perf-meter`, cc394e1).
2. **Audio driver/device settings (this spec).**
3. Modular rack (add/remove instruments; per-machine CPU) — own spec later.

## Goal

Let the user choose the output driver, device, sample rate and buffer size,
remember the choice per machine, and survive missing devices gracefully.
Linux builds get JACK alongside ALSA/PipeWire; Windows builds get ASIO
(opt-in build feature) alongside WASAPI. The README also notes the JACK stub.

Out of scope: audio input, JACK port naming/manual connections, the ASIO
control panel, hot-plug auto-switching, per-project device choice.

## Approach

Everything goes through cpal 0.15 hosts (no direct `jack`/`asio-sys` code).
One callback path stays in `crates/app/src/audio.rs`.

## 1. Build & drivers

- `crates/app/Cargo.toml`: Linux-only target dependency enabling cpal's
  `jack` feature. JACK is on by default.
- `jack-sys` 0.5.1's build script requires pkg-config to find `jack` even
  though `jack` 0.11 loads `libjack` at runtime (`dynamic_loading`, default).
  SteamOS ships `libjack` (PipeWire's) without `jack.pc`. Commit a stub
  `support/pkgconfig/jack.pc` and set `PKG_CONFIG_PATH` to it in
  a new `.cargo/config.toml` `[env]` (relative, not forced — a user's own
  `PKG_CONFIG_PATH` wins). Verified by spike on this Deck: builds, and the
  JACK host lists `cpal_client_out` (48 kHz, 512 frames, server-fixed).
- No `libjack` at runtime: JACK simply isn't in `available_hosts()`.
- Windows: app feature `asio = ["cpal/asio"]`. Without it, WASAPI only.
  ASIO-specific code limited to host selection. README "Build and run" gains a
  Windows section: Steinberg ASIO SDK + `CPAL_ASIO_DIR`, LLVM +
  `LIBCLANG_PATH`, MSVC toolchain, `cargo build --release --features asio`.
  The ASIO path cannot be compiled on the Deck; the user does the first
  Windows build.
- Driver list = `cpal::available_hosts()`. Display names: "ALSA / PipeWire",
  "JACK", "WASAPI", "ASIO" (others: the cpal `HostId` name).

## 2. Settings model & persistence

New `crates/app/src/settings.rs`:

```rust
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
pub struct Settings { pub version: u32, pub audio: AudioSettings }

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
pub struct AudioSettings {
    pub driver: Option<String>,     // cpal HostId name; None = platform default
    pub device: Option<String>,     // device name; None = driver default
    pub sample_rate: Option<u32>,   // None = device default
    pub buffer_frames: Option<u32>, // None = driver default
}
```

- Machine settings, never stored in `.sidacid` projects.
- Path: Linux `$XDG_CONFIG_HOME/sidacid/settings.ron`, else
  `~/.config/sidacid/settings.ron`; Windows `%APPDATA%\SidAcid\settings.ron`.
  Small helper, no `dirs` crate. App crate gains `serde` + `ron` deps.
- RON; written atomically (`settings.ron.tmp` then rename); written only on
  **Apply**.
- Load outcomes: missing file → defaults, silent. Unreadable/unparseable →
  defaults + notice; replaced on next Apply. `version` newer than known →
  defaults + notice (file left untouched until Apply).

### Fallback chain (at every stream open)

1. Saved driver unavailable → platform default host.
2. Saved device not found → that host's default output device.
3. Sample rate unsupported → device default rate.
4. Buffer outside the supported range → driver default. JACK: rate and
   buffer are always the server's; saved values ignored silently.

Each fallback (except the JACK case) yields a notice for the status line,
e.g. `Saved device 'Scarlett 2i2' not found — using 'default'`. Saved
settings are **not** modified by a fallback.

The chain is a pure function over a description of what is available
(hosts → devices → supported rate ranges/buffer ranges/default config),
returning the chosen open-config plus notices, so it is testable without
cpal. A thin adapter builds that description from cpal.

## 3. Settings window

- Opened by **File → Audio Settings…** and by clicking the CPU readout.
- egui window:

```
 Audio Settings
 Now:     JACK · cpal_client_out · 48 kHz · 512 frames (10.7 ms)
 Driver:      [ JACK            ▾]
 Device:      [ cpal_client_out ▾]  [Rescan]  ☐ Show all devices
 Sample rate: [ Default (48000) ▾]
 Buffer:      [ Default         ▾]  ≈ 10.7 ms
                                    [Cancel] [Apply]
```

- "Now:" shows the stream actually opened (post-fallback), not the saved
  settings.
- Device scan (~450 ms measured on the Deck) runs on a worker thread when
  the window opens and on Rescan; results return over a channel. Lists show
  "Scanning devices…" meanwhile; current values stay visible.
- ALSA filter (default on): show only `default`, `pipewire`, `pulse`, `jack`
  and names starting `sysdefault:`, `front:`, `hw:`, `plughw:`, `hdmi:`.
  "Show all devices" bypasses it (not persisted). Other hosts unfiltered.
- Sample rate: "Default (n)" + those of 44100/48000/88200/96000/176400/192000
  inside any supported range of the selected device.
- Buffer: "Default" + 64…4096 (powers of two) inside the supported range;
  if the range is unknown, offer all. Latency hint = frames / rate.
- JACK selected: rate and buffer disabled, labelled "set by the JACK server".
- Apply with unchanged settings: closes, no restart. Apply with changes:
  save + restart (section 4). Cancel: discard edits, close.
- Apply failure: error shown in the window, window stays open, previous
  working stream restored, nothing saved.

## 4. Stream lifecycle & errors

- `AudioLink::start(project, &AudioSettings)` runs the fallback chain and
  returns the link, an `OpenedStream { driver, device, sample_rate,
  buffer_frames: Option<u32> }` and the notices.
- Apply: drop the old stream first (JACK/ASIO allow one stream per device),
  build a fresh `Engine::from_project` at the new rate (as `start_audio`
  does today), open the new stream. On failure reopen the last working
  `OpenedStream` and report. A restart stops playback; the project is
  untouched.
- Error callback (not the audio thread):
  - `StreamError::BackendSpecific` whose description contains `xrun` →
    increment shared `AtomicU32` xrun counter; stream keeps running.
  - Anything else → `failed` as today ("audio device stopped" + one
    automatic retry), the retry going through the fallback chain.
- Meter: ⚠ count = render overruns (`Status::overruns`) + driver xruns.
  Hover lists both (`3 render overruns · 5 JACK xruns`). Click clears both
  (UI-side baselines). A stream restart resets both.
- Real-time: the audio callback is unchanged apart from the existing atomic
  buffer-size store. The counting-allocator test keeps covering
  `EngineHost::process`.

## 5. Testing

Unit (no device):
- `settings.rs`: RON round trip; path resolution (XDG set/unset, APPDATA);
  atomic write leaves no `.tmp`; missing → silent defaults; corrupt →
  defaults + notice; newer `version` → defaults + notice.
- Fallback chain over fake availability: each step, JACK ignoring
  rate/buffer, notices' wording, saved settings unchanged.
- ALSA filter vs the Deck's real device list; show-all bypass.
- Rate/buffer menu building, including unknown ranges.
- Error classification: xrun counts, not fails; others fail.
- Meter: overrun + xrun sum, hover breakdown, clear, restart reset.
- Window logic: unchanged Apply doesn't restart; Cancel discards; failed
  Apply keeps window, restores, doesn't save.

Manual (Deck, done by Claude): switch ALSA default → JACK → ALSA `pipewire`
while the demo plays; force JACK xruns with a tiny buffer and confirm the
count rises while playback continues; bogus saved device → fallback notice.

Manual (user, Windows): build with `--features asio` per the README,
select the ASIO interface, play the demo.
