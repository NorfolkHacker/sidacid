# Audio Driver & Device Settings Implementation Plan (Plan 9)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user pick output driver (ALSA/JACK on Linux, WASAPI/ASIO on Windows), device, sample rate and buffer size; persist per machine; fall back gracefully.

**Architecture:** All drivers go through cpal 0.15 hosts. A pure "fallback chain" (`audio_select.rs`) chooses what to open from a plain-data description of what is available; a thin cpal adapter (`audio_scan.rs`) builds that description and `audio.rs` opens the chosen stream. Settings live in a per-machine RON file (`settings.rs`). A new egui window (`ui/audio_settings.rs`) edits them; device scans run on a worker thread.

**Tech Stack:** Rust 2024, cpal 0.15.3 (+`jack` on Linux, opt-in `asio` on Windows), egui/eframe 0.31, serde + ron 0.8, rtrb.

**Spec:** `docs/superpowers/specs/2026-10-14-audio-settings-design.md`

## Global Constraints

- No allocation, locking or blocking inside the cpal data callback or `EngineHost::process` (the counting-allocator test must keep passing).
- Settings are per machine, never written into `.sidacid` projects.
- Settings path: Linux `$XDG_CONFIG_HOME/sidacid/settings.ron`, else `~/.config/sidacid/settings.ron`; Windows `%APPDATA%\SidAcid\settings.ron`. No `dirs` crate.
- Settings are written only on Apply, atomically (`settings.ron.tmp` then rename).
- A fallback never modifies the saved settings.
- JACK: rate and buffer are the server's; saved rate/buffer are ignored silently.
- Driver labels: "ALSA / PipeWire" (ALSA), "JACK", "WASAPI", "ASIO"; anything else uses `HostId::name()`.
- ALSA device allow-list: exactly `default`, `pipewire`, `pulse`, `jack`, or names starting `sysdefault:`, `front:`, `hw:`, `plughw:`, `hdmi:`.
- Rate menu candidates: 44100, 48000, 88200, 96000, 176400, 192000. Buffer menu candidates: 64, 128, 256, 512, 1024, 2048, 4096.
- Tests: `cargo test --workspace`; lint: `cargo clippy --workspace --all-targets` (no warnings); `cargo fmt --all`.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Branch: continue on `perf-meter` (it already holds the meter, cc394e1, and the spec).

## File map

| File | Responsibility |
|---|---|
| `support/pkgconfig/jack.pc` (new) | Stub so `jack-sys`'s build script finds "jack"; the library is dlopened at runtime |
| `.cargo/config.toml` (new) | Points `PKG_CONFIG_PATH` at the stub unless the user set it |
| `crates/app/Cargo.toml` | Linux `cpal/jack`; `asio` feature; serde + ron deps |
| `Cargo.toml` | nothing new (serde, ron already in workspace deps) |
| `README.md` | Build notes: JACK stub, Windows ASIO steps |
| `crates/app/src/settings.rs` (new) | `Settings`/`AudioSettings`, path, load, atomic save |
| `crates/app/src/audio_select.rs` (new) | Pure: availability model, fallback chain, ALSA filter, menu lists, labels |
| `crates/app/src/audio_scan.rs` (new) | cpal → `HostInfo`/`DeviceInfo`; background scan |
| `crates/app/src/audio.rs` | Open a `Choice`; `OpenedStream`; xrun classification and counter |
| `crates/app/src/ui/perf.rs` | ⚠ = overruns + xruns, hover breakdown |
| `crates/app/src/ui/audio_settings.rs` (new) | Settings window + apply/cancel/revert |
| `crates/app/src/ui/mod.rs` | App owns `Settings`, path; `start_audio` uses them |
| `crates/app/src/ui/files.rs` | File → Audio Settings… |
| `crates/app/src/main.rs` | `mod settings; mod audio_select; mod audio_scan;` |

---

### Task 1: JACK and ASIO in the build

**Files:**
- Create: `support/pkgconfig/jack.pc`, `.cargo/config.toml`
- Modify: `crates/app/Cargo.toml`, `README.md` ("Build and run" section, ~line 56), `crates/app/src/audio.rs` (test module)

**Interfaces:**
- Produces: cargo feature `asio` on package `sidacid`; on Linux `cpal::HostId::Jack` exists.

- [ ] **Step 1: Write the failing test** — append to `mod tests` in `crates/app/src/audio.rs`:

```rust
    #[cfg(target_os = "linux")]
    #[test]
    fn jack_is_compiled_in_on_linux() {
        assert!(cpal::ALL_HOSTS.contains(&cpal::HostId::Jack));
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p sidacid jack_is_compiled_in`
Expected: compile error `no variant ... named Jack`.

- [ ] **Step 3: Add the stub, config and features**

`support/pkgconfig/jack.pc`:
```
# Stub for jack-sys's build script, which insists on finding "jack" through
# pkg-config. cpal's JACK support loads libjack at runtime (jack's
# dynamic_loading feature), so nothing is linked from here. A real jack.pc
# found through your own PKG_CONFIG_PATH works just as well.
prefix=/usr
libdir=${prefix}/lib
includedir=${prefix}/include

Name: jack
Description: JACK Audio Connection Kit (stub; loaded at runtime)
Version: 1.9.17
Libs:
Cflags:
```

`.cargo/config.toml`:
```toml
# Lets the JACK backend build where libjack has no pkg-config file
# (SteamOS, distros without JACK dev packages). Not forced: a
# PKG_CONFIG_PATH you set yourself wins.
[env]
PKG_CONFIG_PATH = { value = "support/pkgconfig", relative = true }
```

Append to `crates/app/Cargo.toml`:
```toml
serde.workspace = true
ron.workspace = true
```
(inside `[dependencies]`), and at the end:
```toml
[features]
# Windows only: ASIO through cpal. Needs the Steinberg ASIO SDK (see README).
asio = ["cpal/asio"]

[target.'cfg(target_os = "linux")'.dependencies]
cpal = { workspace = true, features = ["jack"] }
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p sidacid jack_is_compiled_in`
Expected: PASS.

- [ ] **Step 5: README** — in "Build and run", after the existing commands, add:

```markdown
### Audio drivers

Linux builds include ALSA (which covers PipeWire and PulseAudio) and JACK.
JACK is loaded at runtime: without `libjack` it just isn't offered. The
build only needs pkg-config to report "jack"; `.cargo/config.toml` points
it at a stub in `support/pkgconfig/` unless you set `PKG_CONFIG_PATH`
yourself.

Windows builds include WASAPI. For ASIO, build on Windows with MSVC:

1. Download the Steinberg ASIO SDK and unzip it; set `CPAL_ASIO_DIR` to
   that folder.
2. Install LLVM and set `LIBCLANG_PATH` to its `bin` folder.
3. `cargo build --release -p sidacid --features asio`

Pick the driver and device under **File → Audio Settings…**.
```

- [ ] **Step 6: Full check and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace`
Expected: no warnings, all pass.

```bash
git add support .cargo crates/app/Cargo.toml Cargo.lock README.md crates/app/src/audio.rs
git commit -m "build(app): JACK on Linux (runtime-loaded, pkg-config stub), opt-in ASIO feature"
```

---

### Task 2: Settings file

**Files:**
- Create: `crates/app/src/settings.rs`
- Modify: `crates/app/src/main.rs` (add `mod settings;`)

**Interfaces:**
- Produces:
  - `pub struct Settings { pub version: u32, pub audio: AudioSettings }` (`Default` has `version: SETTINGS_VERSION`)
  - `pub struct AudioSettings { pub driver: Option<String>, pub device: Option<String>, pub sample_rate: Option<u32>, pub buffer_frames: Option<u32> }`
  - `pub const SETTINGS_VERSION: u32 = 1;`
  - `pub fn settings_path() -> Option<PathBuf>`
  - `pub fn load(path: &Path) -> (Settings, Option<String>)` — notice when defaults replace a bad file
  - `pub fn save(path: &Path, settings: &Settings) -> Result<(), String>`

- [ ] **Step 1: Write the failing tests** — create `crates/app/src/settings.rs` with only the test module plus `mod settings;` in `main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sidacid-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn linux_path_prefers_xdg_then_home() {
        let p = path_from(false, env(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/h")]));
        assert_eq!(p, Some(PathBuf::from("/x/sidacid/settings.ron")));
        let p = path_from(false, env(&[("XDG_CONFIG_HOME", ""), ("HOME", "/h")]));
        assert_eq!(p, Some(PathBuf::from("/h/.config/sidacid/settings.ron")));
        assert_eq!(path_from(false, env(&[])), None);
    }

    #[test]
    fn windows_path_uses_appdata() {
        let p = path_from(true, env(&[("APPDATA", "C:\\Users\\a\\AppData\\Roaming")]));
        assert_eq!(
            p,
            Some(PathBuf::from("C:\\Users\\a\\AppData\\Roaming").join("SidAcid").join("settings.ron"))
        );
        assert_eq!(path_from(true, env(&[])), None);
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_no_temp_file() {
        let dir = scratch("roundtrip");
        let path = dir.join("sub").join("settings.ron");
        let s = Settings {
            audio: AudioSettings {
                driver: Some("JACK".into()),
                device: Some("cpal_client_out".into()),
                sample_rate: Some(48000),
                buffer_frames: Some(256),
            },
            ..Settings::default()
        };
        save(&path, &s).unwrap();
        assert!(!path.with_extension("ron.tmp").exists());
        assert_eq!(load(&path), (s, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_silent_defaults() {
        let dir = scratch("missing");
        assert_eq!(load(&dir.join("settings.ron")), (Settings::default(), None));
    }

    #[test]
    fn a_corrupt_file_gives_defaults_and_a_notice() {
        let dir = scratch("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.ron");
        std::fs::write(&path, "(version: 1, audio: (driver: Some(").unwrap();
        let (s, notice) = load(&path);
        assert_eq!(s, Settings::default());
        assert!(notice.unwrap().starts_with("Could not read audio settings"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_version_gives_defaults_and_a_notice() {
        let dir = scratch("newer");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.ron");
        std::fs::write(&path, "(version: 99, audio: (), future: 3)").unwrap();
        let (s, notice) = load(&path);
        assert_eq!(s, Settings::default());
        assert_eq!(
            notice.as_deref(),
            Some("Settings are from a newer SidAcid; using defaults")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sidacid settings::`
Expected: compile errors (`path_from`, `Settings`, … not found).

- [ ] **Step 3: Implement** — above the test module:

```rust
//! Per-machine app settings (today: the audio device), kept out of
//! projects so a project opened on another computer does not try to use
//! this one's sound card.

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Settings {
    pub version: u32,
    #[serde(default)]
    pub audio: AudioSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            audio: AudioSettings::default(),
        }
    }
}

/// `None` everywhere means "whatever the system picks".
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Eq, Debug)]
pub struct AudioSettings {
    /// A cpal `HostId::name()`, e.g. "ALSA", "JACK", "WASAPI", "ASIO".
    #[serde(default)]
    pub driver: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub buffer_frames: Option<u32>,
}

#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

/// Where this machine's settings live, or `None` when the environment
/// gives no home for them.
pub fn settings_path() -> Option<PathBuf> {
    path_from(cfg!(windows), |k| std::env::var_os(k))
}

fn path_from(windows: bool, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if windows {
        return set("APPDATA").map(|d| d.join("SidAcid").join("settings.ron"));
    }
    let base = set("XDG_CONFIG_HOME").or_else(|| set("HOME").map(|h| h.join(".config")))?;
    Some(base.join("sidacid").join("settings.ron"))
}

/// Reads the settings. A missing file is plain defaults; an unreadable,
/// unparseable or newer file is defaults plus a notice for the user.
pub fn load(path: &Path) -> (Settings, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
        Err(e) => return (Settings::default(), Some(format!("Could not read audio settings: {e}"))),
    };
    let bad = |e: ron::error::SpannedError| {
        (Settings::default(), Some(format!("Could not read audio settings: {e}")))
    };
    match ron::from_str::<VersionOnly>(&text) {
        Err(e) => bad(e),
        Ok(v) if v.version > SETTINGS_VERSION => (
            Settings::default(),
            Some("Settings are from a newer SidAcid; using defaults".into()),
        ),
        Ok(_) => match ron::from_str::<Settings>(&text) {
            Ok(s) => (s, None),
            Err(e) => bad(e),
        },
    }
}

/// Writes `settings` through a temporary file so a crash mid-write never
/// leaves a half-written file behind.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let text = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default())
        .map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}
```

Note: the "newer version" test file contains an unknown field `future`; `VersionOnly` must ignore unknown fields (serde's default), which it does.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sidacid settings::`
Expected: 6 passed. (`settings_path` is used in Task 5; until then add `#[allow(dead_code)]` only if clippy complains — remove it in Task 5.)

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets
git add crates/app/src/settings.rs crates/app/src/main.rs
git commit -m "feat(app): per-machine settings file (audio device), atomic save"
```

---

### Task 3: Fallback chain and menu lists (pure)

**Files:**
- Create: `crates/app/src/audio_select.rs`
- Modify: `crates/app/src/main.rs` (add `mod audio_select;`)

**Interfaces:**
- Consumes: `crate::settings::AudioSettings` (Task 2).
- Produces:
```rust
pub const JACK: &str = "JACK";
pub const ALSA: &str = "ALSA";
pub const RATES: [u32; 6] = [44100, 48000, 88200, 96000, 176400, 192000];
pub const BUFFERS: [u32; 7] = [64, 128, 256, 512, 1024, 2048, 4096];

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    pub default_rate: u32,
    /// Inclusive sample-rate ranges for the device's default format and channel count.
    pub rates: Vec<(u32, u32)>,
    /// Inclusive buffer range in frames; None = the driver does not say.
    pub buffer: Option<(u32, u32)>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct HostInfo {
    pub id: String,                     // HostId::name()
    pub default_device: Option<String>,
    pub devices: Vec<DeviceInfo>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub driver: String,
    pub device: Option<String>,         // None = the host's default device (none found by name)
    pub sample_rate: Option<u32>,       // None = device default
    pub buffer_frames: Option<u32>,     // None = driver default
}
pub fn choose(hosts: &[HostInfo], default_host: &str, saved: &AudioSettings) -> Result<(Choice, Vec<String>), String>;
pub fn driver_label(id: &str) -> &str;
pub fn alsa_listed(name: &str) -> bool;
pub fn listed_devices<'a>(host: &'a HostInfo, show_all: bool) -> Vec<&'a DeviceInfo>;
pub fn rate_options(dev: &DeviceInfo) -> Vec<u32>;
pub fn buffer_options(dev: &DeviceInfo) -> Vec<u32>;
pub fn latency_ms(frames: u32, rate: u32) -> f32;
```

`choose` contract:
1. Host = `saved.driver` if a host with that `id` is in `hosts`; else `default_host` (notice `Driver '{d}' is not available — using '{label(default_host)}'` when `saved.driver` was `Some`). If `default_host` is not in `hosts` either → `Err("no audio driver available")`.
2. Device = `saved.device` if a device of that name is in the host; else the host's `default_device` (notice `Saved device '{d}' not found — using '{default or "default"}'` when `saved.device` was `Some`). If the host has no devices and no default → `Err("no audio output device found")`. If `saved.device` is `None`, `Choice.device = host.default_device` without a notice.
3. Host is JACK → `sample_rate: None, buffer_frames: None`, no notices for rate/buffer.
4. Rate: `saved.sample_rate` kept if inside any of the device's `rates`; else `None` + notice `{r} Hz is not supported — using {default_rate} Hz`.
5. Buffer: `saved.buffer_frames` kept if `buffer` is `None` or contains it; else `None` + notice `A {n}-frame buffer is not supported — using the driver default`.
6. When the device fell back (step 2), the saved rate/buffer are still checked against the fallback device.

- [ ] **Step 1: Write the failing tests** (`crates/app/src/audio_select.rs`, test module only + `mod audio_select;`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AudioSettings;

    fn dev(name: &str) -> DeviceInfo {
        DeviceInfo {
            name: name.into(),
            default_rate: 48000,
            rates: vec![(44100, 48000), (96000, 96000)],
            buffer: Some((64, 2048)),
        }
    }

    fn hosts() -> Vec<HostInfo> {
        vec![
            HostInfo {
                id: "ALSA".into(),
                default_device: Some("default".into()),
                devices: vec![dev("default"), dev("pipewire"), dev("hw:CARD=USB,DEV=0")],
            },
            HostInfo {
                id: "JACK".into(),
                default_device: Some("cpal_client_out".into()),
                devices: vec![DeviceInfo {
                    name: "cpal_client_out".into(),
                    default_rate: 48000,
                    rates: vec![(48000, 48000)],
                    buffer: Some((512, 512)),
                }],
            },
        ]
    }

    fn saved(driver: Option<&str>, device: Option<&str>, rate: Option<u32>, buf: Option<u32>) -> AudioSettings {
        AudioSettings {
            driver: driver.map(Into::into),
            device: device.map(Into::into),
            sample_rate: rate,
            buffer_frames: buf,
        }
    }

    #[test]
    fn defaults_choose_the_default_host_and_device_silently() {
        let (c, notes) = choose(&hosts(), "ALSA", &AudioSettings::default()).unwrap();
        assert_eq!(
            c,
            Choice { driver: "ALSA".into(), device: Some("default".into()), sample_rate: None, buffer_frames: None }
        );
        assert!(notes.is_empty());
    }

    #[test]
    fn a_fully_valid_saved_setup_is_kept() {
        let s = saved(Some("ALSA"), Some("pipewire"), Some(96000), Some(256));
        let (c, notes) = choose(&hosts(), "ALSA", &s).unwrap();
        assert_eq!(
            c,
            Choice { driver: "ALSA".into(), device: Some("pipewire".into()), sample_rate: Some(96000), buffer_frames: Some(256) }
        );
        assert!(notes.is_empty());
    }

    #[test]
    fn a_missing_driver_falls_back_to_the_default_host() {
        let s = saved(Some("ASIO"), None, None, None);
        let (c, notes) = choose(&hosts(), "ALSA", &s).unwrap();
        assert_eq!(c.driver, "ALSA");
        assert_eq!(notes, ["Driver 'ASIO' is not available — using 'ALSA / PipeWire'"]);
    }

    #[test]
    fn a_missing_device_falls_back_and_still_checks_rate_and_buffer() {
        let s = saved(Some("ALSA"), Some("Scarlett 2i2"), Some(192000), Some(8192));
        let (c, notes) = choose(&hosts(), "ALSA", &s).unwrap();
        assert_eq!(c.device.as_deref(), Some("default"));
        assert_eq!(c.sample_rate, None);
        assert_eq!(c.buffer_frames, None);
        assert_eq!(
            notes,
            [
                "Saved device 'Scarlett 2i2' not found — using 'default'",
                "192000 Hz is not supported — using 48000 Hz",
                "A 8192-frame buffer is not supported — using the driver default",
            ]
        );
    }

    #[test]
    fn jack_ignores_saved_rate_and_buffer_silently() {
        let s = saved(Some("JACK"), None, Some(44100), Some(64));
        let (c, notes) = choose(&hosts(), "ALSA", &s).unwrap();
        assert_eq!(
            c,
            Choice { driver: "JACK".into(), device: Some("cpal_client_out".into()), sample_rate: None, buffer_frames: None }
        );
        assert!(notes.is_empty());
    }

    #[test]
    fn an_unknown_buffer_range_accepts_any_buffer() {
        let mut h = hosts();
        h[0].devices[0].buffer = None;
        let s = saved(None, None, None, Some(128));
        assert_eq!(choose(&h, "ALSA", &s).unwrap().0.buffer_frames, Some(128));
    }

    #[test]
    fn nothing_to_open_is_an_error() {
        assert_eq!(choose(&[], "ALSA", &AudioSettings::default()).unwrap_err(), "no audio driver available");
        let empty = vec![HostInfo { id: "ALSA".into(), default_device: None, devices: vec![] }];
        assert_eq!(
            choose(&empty, "ALSA", &AudioSettings::default()).unwrap_err(),
            "no audio output device found"
        );
    }

    #[test]
    fn the_alsa_filter_keeps_real_outputs_from_the_deck_list() {
        let deck = [
            "sysdefault", "lavrate", "samplerate", "speexrate", "jack", "pipewire", "pulse",
            "speex", "upmix", "vdownmix", "default", "sysdefault:CARD=CODEC",
            "front:CARD=CODEC,DEV=0", "surround40:CARD=CODEC,DEV=0", "iec958:CARD=CODEC,DEV=0",
            "hdmi:CARD=Generic,DEV=0", "hw:CARD=USB,DEV=0", "plughw:CARD=USB,DEV=0",
        ];
        let kept: Vec<&str> = deck.iter().copied().filter(|n| alsa_listed(n)).collect();
        assert_eq!(
            kept,
            [
                "jack", "pipewire", "pulse", "default", "sysdefault:CARD=CODEC",
                "front:CARD=CODEC,DEV=0", "hdmi:CARD=Generic,DEV=0", "hw:CARD=USB,DEV=0",
                "plughw:CARD=USB,DEV=0",
            ]
        );
    }

    #[test]
    fn listed_devices_filters_alsa_only_unless_show_all() {
        let mut h = hosts();
        h[0].devices.push(dev("speex"));
        assert_eq!(listed_devices(&h[0], false).len(), 3);
        assert_eq!(listed_devices(&h[0], true).len(), 4);
        h[1].devices.push(dev("speex"));
        assert_eq!(listed_devices(&h[1], false).len(), 2, "JACK is not filtered");
    }

    #[test]
    fn rate_and_buffer_menus_follow_the_supported_ranges() {
        let d = dev("default");
        assert_eq!(rate_options(&d), [44100, 48000, 96000]);
        assert_eq!(buffer_options(&d), [64, 128, 256, 512, 1024, 2048]);
        let open = DeviceInfo { buffer: None, ..dev("x") };
        assert_eq!(buffer_options(&open), BUFFERS);
    }

    #[test]
    fn labels_and_latency() {
        assert_eq!(driver_label("ALSA"), "ALSA / PipeWire");
        assert_eq!(driver_label("JACK"), "JACK");
        assert_eq!(driver_label("CoreAudio"), "CoreAudio");
        assert!((latency_ms(512, 48000) - 10.666_667).abs() < 1e-3);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sidacid audio_select::`
Expected: compile errors.

- [ ] **Step 3: Implement** (above the tests):

```rust
//! Choosing what audio stream to open, from plain data about what the
//! system offers. No cpal here: `audio_scan` builds the `HostInfo`s.

use crate::settings::AudioSettings;

pub const JACK: &str = "JACK";
pub const ALSA: &str = "ALSA";
pub const RATES: [u32; 6] = [44100, 48000, 88200, 96000, 176400, 192000];
pub const BUFFERS: [u32; 7] = [64, 128, 256, 512, 1024, 2048, 4096];

// (DeviceInfo, HostInfo, Choice exactly as in Interfaces above,
//  each with a one-line doc comment.)

pub fn driver_label(id: &str) -> &str {
    match id {
        ALSA => "ALSA / PipeWire",
        other => other,
    }
}

/// ALSA lists many plugin pseudo-devices (resamplers, up/down-mixers);
/// only these name real or system-routed outputs.
pub fn alsa_listed(name: &str) -> bool {
    matches!(name, "default" | "pipewire" | "pulse" | "jack")
        || ["sysdefault:", "front:", "hw:", "plughw:", "hdmi:"]
            .iter()
            .any(|p| name.starts_with(p))
}

pub fn listed_devices(host: &HostInfo, show_all: bool) -> Vec<&DeviceInfo> {
    host.devices
        .iter()
        .filter(|d| show_all || host.id != ALSA || alsa_listed(&d.name))
        .collect()
}

fn rate_ok(dev: &DeviceInfo, rate: u32) -> bool {
    dev.rates.iter().any(|&(lo, hi)| (lo..=hi).contains(&rate))
}

fn buffer_ok(dev: &DeviceInfo, frames: u32) -> bool {
    dev.buffer.is_none_or(|(lo, hi)| (lo..=hi).contains(&frames))
}

pub fn rate_options(dev: &DeviceInfo) -> Vec<u32> {
    RATES.into_iter().filter(|&r| rate_ok(dev, r)).collect()
}

pub fn buffer_options(dev: &DeviceInfo) -> Vec<u32> {
    BUFFERS.into_iter().filter(|&b| buffer_ok(dev, b)).collect()
}

pub fn latency_ms(frames: u32, rate: u32) -> f32 {
    frames as f32 * 1000.0 / rate.max(1) as f32
}

/// Picks what to open for `saved`, falling back step by step (driver,
/// device, rate, buffer) to what is there. Never changes `saved`; each
/// fallback adds a notice for the user.
pub fn choose(
    hosts: &[HostInfo],
    default_host: &str,
    saved: &AudioSettings,
) -> Result<(Choice, Vec<String>), String> {
    let mut notes = Vec::new();
    let find = |id: &str| hosts.iter().find(|h| h.id == id);
    let host = match saved.driver.as_deref().and_then(find) {
        Some(h) => h,
        None => {
            let h = find(default_host).ok_or("no audio driver available")?;
            if let Some(d) = &saved.driver {
                notes.push(format!(
                    "Driver '{d}' is not available — using '{}'",
                    driver_label(default_host)
                ));
            }
            h
        }
    };
    let by_name = |n: &str| host.devices.iter().find(|d| d.name == n);
    let device = match saved.device.as_deref().and_then(by_name) {
        Some(d) => Some(d),
        None => {
            if host.devices.is_empty() && host.default_device.is_none() {
                return Err("no audio output device found".into());
            }
            if let Some(d) = &saved.device {
                notes.push(format!(
                    "Saved device '{d}' not found — using '{}'",
                    host.default_device.as_deref().unwrap_or("default")
                ));
            }
            host.default_device.as_deref().and_then(by_name)
        }
    };
    let device_name = device
        .map(|d| d.name.clone())
        .or_else(|| host.default_device.clone());
    let mut choice = Choice {
        driver: host.id.clone(),
        device: device_name,
        sample_rate: None,
        buffer_frames: None,
    };
    if host.id == JACK {
        return Ok((choice, notes));
    }
    if let (Some(r), Some(d)) = (saved.sample_rate, device) {
        if rate_ok(d, r) {
            choice.sample_rate = Some(r);
        } else {
            notes.push(format!("{r} Hz is not supported — using {} Hz", d.default_rate));
        }
    }
    if let Some(n) = saved.buffer_frames {
        if device.is_none_or(|d| buffer_ok(d, n)) {
            choice.buffer_frames = Some(n);
        } else {
            notes.push(format!(
                "A {n}-frame buffer is not supported — using the driver default"
            ));
        }
    }
    Ok((choice, notes))
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sidacid audio_select::`
Expected: 11 passed.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets
git add crates/app/src/audio_select.rs crates/app/src/main.rs
git commit -m "feat(app): audio fallback chain, ALSA device filter, rate/buffer menus"
```

(Dead-code warnings for items used only in later tasks: add `#![allow(dead_code)]` at the top of `audio_select.rs` with a `// Used from Task 5/6` note, and remove it in Task 6.)

---

### Task 4: cpal adapter, opening a Choice, xruns

**Files:**
- Create: `crates/app/src/audio_scan.rs`
- Modify: `crates/app/src/audio.rs`, `crates/app/src/main.rs` (`mod audio_scan;`)

**Interfaces:**
- Consumes: `HostInfo`, `DeviceInfo`, `Choice` (Task 3).
- Produces:
  - `audio_scan::scan_host(id: cpal::HostId) -> Option<HostInfo>`
  - `audio_scan::scan_all() -> Vec<HostInfo>` — every `cpal::available_hosts()` host that opens
  - `audio_scan::host_id(name: &str) -> Option<cpal::HostId>` — from `HostId::name()`
  - `audio_scan::default_host_name() -> &'static str` — `cpal::default_host().id().name()`
  - `audio::OpenedStream { pub driver: String, pub device: String, pub sample_rate: u32, pub buffer_frames: Option<u32> }` (`Clone, Debug, PartialEq`)
  - `AudioLink::open(project: &Project, choice: &Choice) -> Result<AudioLink, String>`
  - `AudioLink::opened(&self) -> &OpenedStream`
  - `AudioLink::xruns(&self) -> u32` — driver xruns since this stream opened
  - `audio::is_xrun(err: &cpal::StreamError) -> bool`
  - `AudioLink::start(project)` is **removed** (callers move to `open` in Task 5).

- [ ] **Step 1: Write the failing tests** — in `audio.rs` tests:

```rust
    #[test]
    fn jack_xruns_are_counted_not_fatal() {
        let xrun = cpal::StreamError::BackendSpecific {
            err: cpal::BackendSpecificError { description: "xrun (buffer over or under run)".into() },
        };
        assert!(is_xrun(&xrun));
        let other = cpal::StreamError::BackendSpecific {
            err: cpal::BackendSpecificError { description: "server shut down".into() },
        };
        assert!(!is_xrun(&other));
        assert!(!is_xrun(&cpal::StreamError::DeviceNotAvailable));
    }
```

and in `audio_scan.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names_round_trip() {
        for id in cpal::ALL_HOSTS {
            assert_eq!(host_id(id.name()), Some(*id));
        }
        assert_eq!(host_id("Nope"), None);
    }

    #[test]
    fn buffer_ranges_convert() {
        assert_eq!(buffer_range(&cpal::SupportedBufferSize::Range { min: 64, max: 2048 }), Some((64, 2048)));
        assert_eq!(buffer_range(&cpal::SupportedBufferSize::Unknown), None);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sidacid -- is_xrun host_names buffer_ranges`
Expected: compile errors.

- [ ] **Step 3: Implement `audio_scan.rs`**

```rust
//! Builds `audio_select`'s plain-data view of the system from cpal.
//! Scanning a host can take ~0.5 s (ALSA probes every device): run
//! `scan_all` off the UI thread.

use crate::audio_select::{DeviceInfo, HostInfo};
use cpal::traits::{DeviceTrait, HostTrait};

pub fn host_id(name: &str) -> Option<cpal::HostId> {
    cpal::ALL_HOSTS.iter().copied().find(|h| h.name() == name)
}

pub fn default_host_name() -> &'static str {
    cpal::default_host().id().name()
}

pub fn scan_all() -> Vec<HostInfo> {
    cpal::available_hosts().into_iter().filter_map(scan_host).collect()
}

pub fn scan_host(id: cpal::HostId) -> Option<HostInfo> {
    let host = cpal::host_from_id(id).ok()?;
    let default_device = host.default_output_device().and_then(|d| d.name().ok());
    let devices = host
        .output_devices()
        .map(|it| it.filter_map(|d| device_info(&d)).collect())
        .unwrap_or_default();
    Some(HostInfo { id: id.name().to_string(), default_device, devices })
}

fn device_info(device: &cpal::Device) -> Option<DeviceInfo> {
    let name = device.name().ok()?;
    let default = device.default_output_config().ok()?;
    let (format, channels) = (default.sample_format(), default.channels());
    let rates = device
        .supported_output_configs()
        .map(|it| {
            it.filter(|c| c.sample_format() == format && c.channels() == channels)
                .map(|c| (c.min_sample_rate().0, c.max_sample_rate().0))
                .collect()
        })
        .unwrap_or_default();
    Some(DeviceInfo {
        name,
        default_rate: default.sample_rate().0,
        rates,
        buffer: buffer_range(default.buffer_size()),
    })
}

pub(crate) fn buffer_range(b: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match *b {
        cpal::SupportedBufferSize::Range { min, max } => Some((min, max)),
        cpal::SupportedBufferSize::Unknown => None,
    }
}
```

- [ ] **Step 4: Rework `AudioLink` in `audio.rs`**

Replace `start` with `open`; add fields `opened: OpenedStream`, `xruns: Arc<AtomicU32>`; keep `sample_rate` and `device_name` public fields (set from `opened`) so existing callers compile. Pass `xruns` into `build` alongside `failed`/`buffer_frames` (extend the tuple to `(Arc<AtomicBool>, Arc<AtomicU32>, Arc<AtomicU32>)`), and change the error callback to:

```rust
            move |err| {
                if is_xrun(&err) {
                    xruns.fetch_add(1, Ordering::Relaxed);
                } else {
                    failed.store(true, Ordering::Relaxed);
                }
            },
```

New code:

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct OpenedStream {
    pub driver: String,
    pub device: String,
    pub sample_rate: u32,
    /// The buffer asked for; None = the driver's default (the meter shows
    /// the size callbacks actually get).
    pub buffer_frames: Option<u32>,
}

/// cpal's JACK host reports xruns through the error callback; they are
/// dropouts, not a dead stream.
pub fn is_xrun(err: &cpal::StreamError) -> bool {
    matches!(err, cpal::StreamError::BackendSpecific { err } if err.description.contains("xrun"))
}

impl AudioLink {
    /// Opens `choice` (from `audio_select::choose`) and starts the engine
    /// on it with `project` loaded.
    pub fn open(project: &Project, choice: &Choice) -> Result<AudioLink, String> {
        let id = crate::audio_scan::host_id(&choice.driver)
            .ok_or_else(|| format!("driver '{}' is not built in", choice.driver))?;
        let host = cpal::host_from_id(id).map_err(|e| format!("{}: {e}", choice.driver))?;
        let device = match &choice.device {
            Some(name) => host
                .output_devices()
                .map_err(|e| format!("audio devices: {e}"))?
                .find(|d| d.name().is_ok_and(|n| &n == name))
                .ok_or_else(|| format!("device '{name}' not found"))?,
            None => host.default_output_device().ok_or("no audio output device found")?,
        };
        let device_name = device.name().unwrap_or_else(|_| "audio device".into());
        let supported = device
            .default_output_config()
            .map_err(|e| format!("audio device: {e}"))?;
        let format = supported.sample_format();
        let mut config: cpal::StreamConfig = supported.into();
        if let Some(rate) = choice.sample_rate {
            config.sample_rate = cpal::SampleRate(rate);
        }
        if let Some(frames) = choice.buffer_frames {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
        let sample_rate = config.sample_rate.0 as f32;
        let (engine_host, commands, events) = channel(Engine::from_project(sample_rate, project));
        let failed = Arc::new(AtomicBool::new(false));
        let buffer_frames = Arc::new(AtomicU32::new(0));
        let xruns = Arc::new(AtomicU32::new(0));
        let shared = (failed.clone(), buffer_frames.clone(), xruns.clone());
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, engine_host, shared),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, engine_host, shared),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, engine_host, shared),
            other => Err(format!("unsupported sample format {other:?}")),
        }?;
        stream.play().map_err(|e| format!("could not start audio: {e}"))?;
        let opened = OpenedStream {
            driver: choice.driver.clone(),
            device: device_name.clone(),
            sample_rate: config.sample_rate.0,
            buffer_frames: choice.buffer_frames,
        };
        Ok(AudioLink {
            _stream: stream,
            commands,
            events,
            failed,
            buffer_frames,
            xruns,
            sample_rate,
            device_name,
            opened,
        })
    }

    pub fn opened(&self) -> &OpenedStream {
        &self.opened
    }

    pub fn xruns(&self) -> u32 {
        self.xruns.load(Ordering::Relaxed)
    }
}
```

Temporarily keep the app compiling: in `ui/mod.rs::start_audio`, replace `AudioLink::start(&self.doc.project)` with

```rust
        let opened = crate::audio_scan::scan_host(cpal::default_host().id())
            .ok_or_else(|| "no audio driver available".to_string())
            .and_then(|h| {
                let name = h.id.clone();
                crate::audio_select::choose(&[h], &name, &Default::default())
            })
            .and_then(|(choice, _)| AudioLink::open(&self.doc.project, &choice));
        match opened {
```
(Task 5 replaces this with the settings-driven path.)

- [ ] **Step 5: Run the tests**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace`
Expected: all pass, no warnings.

- [ ] **Step 6: Smoke run** — `cargo run -p sidacid`, press Play: demo sounds; close.

- [ ] **Step 7: Commit**

```bash
git add crates/app/src/audio_scan.rs crates/app/src/audio.rs crates/app/src/main.rs crates/app/src/ui/mod.rs
git commit -m "feat(app): open a chosen driver/device/rate/buffer; JACK xruns count, not fail"
```

---

### Task 5: The app opens from saved settings; meter adds xruns

**Files:**
- Modify: `crates/app/src/ui/mod.rs`, `crates/app/src/ui/perf.rs`, `crates/app/src/ui/files.rs` (status line only if needed)

**Interfaces:**
- Consumes: `settings::{Settings, load, settings_path}`, `audio_select::choose`, `audio_scan::{scan_host, host_id, default_host_name}`, `AudioLink::{open, xruns, opened}`.
- Produces:
  - `SidAcidApp.settings: Settings`, `SidAcidApp.settings_path: Option<PathBuf>` (None in `headless`)
  - `fn open_audio(project: &Project, saved: &AudioSettings) -> Result<(AudioLink, Vec<String>), String>` (free function in `ui/mod.rs`)
  - `PerfMeter::on_xruns(&mut self, count: u32, now: f64)`, `PerfMeter::xruns(&self) -> u32`; `overruns()` stays render-only; `clear_overruns` clears both; `restart` resets both.

Startup rule: when `saved.driver` and `saved.device` are both `None`, scan only `cpal::default_host()` (cheap path, ~ as today). Otherwise scan only the host `choose` will need: `saved.driver`'s host if `host_id` finds it in `cpal::available_hosts()`, plus the default host (for the fallback). Never `scan_all` at startup.

- [ ] **Step 1: Write the failing meter tests** — add to `ui/perf.rs` tests:

```rust
    #[test]
    fn xruns_add_to_the_warning_and_clear_with_it() {
        let mut p = PerfMeter::default();
        p.on_status(&status(0.0, 2), 1.0);
        p.on_xruns(5, 1.5);
        assert_eq!((p.overruns(), p.xruns()), (2, 5));
        assert_eq!(p.dropouts(), 7);
        assert_eq!(p.last_overrun_at, Some(1.5));
        assert_eq!(p.breakdown(), "2 render overruns · 5 driver xruns");
        p.clear_overruns();
        assert_eq!(p.dropouts(), 0);
        p.on_xruns(6, 2.0);
        assert_eq!(p.dropouts(), 1);
        p.restart();
        p.on_xruns(0, 3.0);
        assert_eq!(p.dropouts(), 0);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p sidacid perf::`
Expected: compile errors (`on_xruns`, `xruns`, `dropouts`, `breakdown`).

- [ ] **Step 3: Implement in `perf.rs`**

Add fields `xruns_seen: u32, xrun_base: u32`. Methods:

```rust
    /// The stream's driver-reported xrun count (JACK); 0 elsewhere.
    pub fn on_xruns(&mut self, count: u32, now: f64) {
        if count != self.xruns_seen {
            self.xruns_seen = count;
            self.last_overrun_at = Some(now);
        }
    }

    pub fn xruns(&self) -> u32 {
        self.xruns_seen.wrapping_sub(self.xrun_base)
    }

    /// Everything the ⚠ counts: our overruns plus driver xruns.
    pub fn dropouts(&self) -> u32 {
        self.overruns().wrapping_add(self.xruns())
    }

    pub fn breakdown(&self) -> String {
        format!("{} render overruns · {} driver xruns", self.overruns(), self.xruns())
    }
```

`clear_overruns` also sets `self.xrun_base = self.xruns_seen`. In `perf_readout`: call `self.view.perf.on_xruns(audio.xruns(), now)` when audio is present (before borrowing `perf`), show `⚠ {dropouts}` when `dropouts() > 0`, and use `breakdown()` in the hover instead of the overrun-only sentence:

```rust
                .on_hover_text(format!(
                    "Audible dropouts: {}.{ago}\nClick to clear.",
                    perf.breakdown()
                ))
```

The CPU/peak hover uses `audio.opened()`:

```rust
            let o = audio.opened();
            hover.push_str(&format!(
                "{} · {} · {} Hz",
                crate::audio_select::driver_label(&o.driver),
                o.device,
                o.sample_rate
            ));
```
(then the existing buffer-frames part).

- [ ] **Step 4: Run the meter tests** — `cargo test -p sidacid perf::` → PASS.

- [ ] **Step 5: Settings-driven startup in `ui/mod.rs`**

Add to `SidAcidApp`: `pub(crate) settings: crate::settings::Settings`, `pub(crate) settings_path: Option<std::path::PathBuf>`. `headless` sets `Settings::default()` / `None`. `new()`:

```rust
        let mut app = Self::headless(Document::new(acid_engine::demo::demo_project()));
        app.settings_path = crate::settings::settings_path();
        if let Some(path) = &app.settings_path {
            let (settings, notice) = crate::settings::load(path);
            app.settings = settings;
            app.view.status_line = notice;
        }
        app.start_audio();
```

Free function:

```rust
/// Opens the stream `saved` asks for, falling back as `audio_select::choose`
/// describes. Scans only the hosts the choice can need.
pub(crate) fn open_audio(
    project: &acid_engine::project::Project,
    saved: &crate::settings::AudioSettings,
) -> Result<(AudioLink, Vec<String>), String> {
    use crate::{audio_scan, audio_select};
    let default = audio_scan::default_host_name();
    let mut wanted = vec![default];
    if let Some(d) = saved.driver.as_deref() {
        if d != default {
            wanted.push(d);
        }
    }
    let available = cpal::available_hosts();
    let hosts: Vec<_> = wanted
        .into_iter()
        .filter_map(audio_scan::host_id)
        .filter(|id| available.contains(id))
        .filter_map(audio_scan::scan_host)
        .collect();
    let (choice, notes) = audio_select::choose(&hosts, default, saved)?;
    Ok((AudioLink::open(project, &choice)?, notes))
}
```

(The "both None → default host only" rule falls out: `wanted` is just the default host.)

`start_audio` becomes:

```rust
    pub(crate) fn start_audio(&mut self) {
        self.pending.clear();
        self.reset_transport_view();
        self.view.perf.restart();
        // Release the old stream before opening another: JACK and ASIO
        // allow one client stream per device.
        self.audio = None;
        match open_audio(&self.doc.project, &self.settings.audio) {
            Ok((link, notes)) => {
                self.audio = Some(link);
                self.audio_error = None;
                if !notes.is_empty() {
                    self.view.status_line = Some(notes.join("; "));
                }
            }
            Err(e) => self.audio_error = Some(e),
        }
    }
```

Add `cpal.workspace = true` is already a dependency of the app — `cpal::available_hosts` is usable from `ui/mod.rs`.

- [ ] **Step 6: Run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace`
Expected: all pass, no warnings.

- [ ] **Step 7: Manual check on the Deck**

1. `mkdir -p ~/.config/sidacid && printf '(version: 1, audio: (driver: Some("ALSA"), device: Some("Scarlett 2i2")))' > ~/.config/sidacid/settings.ron`
2. `cargo run -p sidacid` → status line reads `Saved device 'Scarlett 2i2' not found — using 'default'`; Play sounds; the file is unchanged afterwards.
3. `printf '(version: 1, audio: (driver: Some("JACK")))' > ~/.config/sidacid/settings.ron`; run → hover on CPU shows `JACK · cpal_client_out · 48000 Hz`; Play sounds.
4. `rm ~/.config/sidacid/settings.ron`.

- [ ] **Step 8: Commit**

```bash
git add crates/app/src/ui/mod.rs crates/app/src/ui/perf.rs
git commit -m "feat(app): open audio from saved settings with fallbacks; meter counts JACK xruns"
```

---

### Task 6: Audio Settings window

**Files:**
- Create: `crates/app/src/ui/audio_settings.rs`
- Modify: `crates/app/src/ui/mod.rs` (`mod audio_settings;`, `ViewState.audio_window`, call `self.audio_settings_window(ctx)` from `ui()` next to `self.dialogs(ctx)`), `crates/app/src/ui/files.rs` (menu item), `crates/app/src/ui/perf.rs` (CPU label click opens it)

**Interfaces:**
- Consumes: Task 2–5 items; `open_audio`.
- Produces:
```rust
#[derive(Default)]
pub struct AudioWindow {
    pub open: bool,
    /// The settings being edited (copied from `app.settings.audio` on open).
    pub edit: AudioSettings,
    pub hosts: Option<Vec<HostInfo>>,
    pub scan_rx: Option<std::sync::mpsc::Receiver<Vec<HostInfo>>>,
    pub show_all: bool,
    pub error: Option<String>,
}
#[derive(Debug, PartialEq)]
pub enum ApplyOutcome { Unchanged, Applied, Failed(String) }

impl SidAcidApp {
    pub(crate) fn open_audio_window(&mut self);        // copies settings, starts a scan
    pub(crate) fn audio_settings_window(&mut self, ctx: &egui::Context);
    /// Applies `new`; `open` stands in for `open_audio` in tests.
    pub(crate) fn apply_audio(
        &mut self,
        new: AudioSettings,
        open: &mut dyn FnMut(&AudioSettings) -> Result<(Option<AudioLink>, Vec<String>), String>,
    ) -> ApplyOutcome;
}
```

`apply_audio` contract:
- `new == self.settings.audio` → `Unchanged`; nothing restarts, window closes.
- Otherwise: `reset_transport_view()`, `perf.restart()`, `self.audio = None` (old stream released first), call `open(&new)`.
  - `Ok((link, notes))`: `self.audio = link`, `audio_error = None`, `settings.audio = new`, save via `settings::save` when `settings_path` is `Some` (a save error goes to `status_line` as `Could not save settings: {e}`, still `Applied`), notes → `status_line`, window closes → `Applied`.
  - `Err(e)`: call `open(&self.settings.audio)` (previous settings) to restore; on that failing too set `audio_error`. Settings unchanged, nothing saved, window stays open with `error = Some(e)` → `Failed(e)`.
- Production call: `self.apply_audio(edit, &mut |s| open_audio(&project, s).map(|(l, n)| (Some(l), n)))` where `project` is a clone of `self.doc.project` taken before the call (the closure must not borrow `self`).

- [ ] **Step 1: Write the failing tests** (`audio_settings.rs` test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    fn app() -> SidAcidApp {
        SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()))
    }

    fn jack() -> AudioSettings {
        AudioSettings { driver: Some("JACK".into()), ..AudioSettings::default() }
    }

    #[test]
    fn applying_unchanged_settings_does_not_reopen() {
        let mut app = app();
        let mut calls = 0;
        let out = app.apply_audio(AudioSettings::default(), &mut |_| {
            calls += 1;
            Ok((None, vec![]))
        });
        assert_eq!(out, ApplyOutcome::Unchanged);
        assert_eq!(calls, 0);
    }

    #[test]
    fn a_good_apply_keeps_the_settings_and_shows_notes() {
        let mut app = app();
        app.view.audio_window.open = true;
        let out = app.apply_audio(jack(), &mut |_| Ok((None, vec!["note".into()])));
        assert_eq!(out, ApplyOutcome::Applied);
        assert_eq!(app.settings.audio, jack());
        assert_eq!(app.view.status_line.as_deref(), Some("note"));
        assert!(!app.view.audio_window.open);
    }

    #[test]
    fn a_failed_apply_restores_the_old_setup_and_saves_nothing() {
        let mut app = app();
        app.view.audio_window.open = true;
        let mut tried = Vec::new();
        let out = app.apply_audio(jack(), &mut |s| {
            tried.push(s.clone());
            if s.driver.is_some() { Err("busy".into()) } else { Ok((None, vec![])) }
        });
        assert_eq!(out, ApplyOutcome::Failed("busy".into()));
        assert_eq!(tried, [jack(), AudioSettings::default()]);
        assert_eq!(app.settings.audio, AudioSettings::default());
        assert!(app.view.audio_window.open);
        assert_eq!(app.view.audio_window.error.as_deref(), Some("busy"));
        assert_eq!(app.audio_error, None);
    }

    #[test]
    fn a_failed_apply_whose_restore_fails_reports_no_audio() {
        let mut app = app();
        let out = app.apply_audio(jack(), &mut |_| Err("gone".into()));
        assert_eq!(out, ApplyOutcome::Failed("gone".into()));
        assert_eq!(app.audio_error.as_deref(), Some("gone"));
    }

    #[test]
    fn opening_the_window_copies_the_saved_settings() {
        let mut app = app();
        app.settings.audio = jack();
        app.open_audio_window();
        assert!(app.view.audio_window.open);
        assert_eq!(app.view.audio_window.edit, jack());
        assert!(app.view.audio_window.scan_rx.is_some());
        assert_eq!(app.view.audio_window.error, None);
    }
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p sidacid audio_settings::` → compile errors.

- [ ] **Step 3: Implement logic** (`open_audio_window`, `apply_audio`) per the contract above. `open_audio_window`:

```rust
    pub(crate) fn open_audio_window(&mut self) {
        let w = &mut self.view.audio_window;
        w.open = true;
        w.edit = self.settings.audio.clone();
        w.error = None;
        w.rescan();
    }
```

with

```rust
impl AudioWindow {
    /// Device scans take ~0.5 s: run them on a worker thread.
    fn rescan(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::audio_scan::scan_all());
        });
        self.scan_rx = Some(rx);
    }

    fn poll_scan(&mut self) {
        if let Some(hosts) = self.scan_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.hosts = Some(hosts);
            self.scan_rx = None;
        }
    }
}
```

- [ ] **Step 4: Run the tests** — `cargo test -p sidacid audio_settings::` → 5 passed.

- [ ] **Step 5: Implement the window UI** (`audio_settings_window`):

```rust
    pub(crate) fn audio_settings_window(&mut self, ctx: &egui::Context) {
        if !self.view.audio_window.open {
            return;
        }
        self.view.audio_window.poll_scan();
        if self.view.audio_window.scan_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let now_line = match &self.audio {
            Some(a) => {
                let o = a.opened();
                let frames = match a.buffer_frames() {
                    0 => String::new(),
                    n => format!(" · {n} frames ({:.1} ms)", latency_ms(n, o.sample_rate)),
                };
                format!("{} · {} · {} Hz{frames}", driver_label(&o.driver), o.device, o.sample_rate)
            }
            None => "no audio".into(),
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Audio Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let w = &mut self.view.audio_window;
                ui.label(format!("Now: {now_line}"));
                ui.separator();
                egui::Grid::new("audio_settings_grid").num_columns(2).show(ui, |ui| {
                    driver_row(ui, w);
                    ui.end_row();
                    device_row(ui, w);
                    ui.end_row();
                    rate_and_buffer_rows(ui, w);
                });
                if let Some(e) = &w.error {
                    ui.colored_label(theme::ACCENT, e);
                }
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                });
            });
        if !open || cancel {
            self.view.audio_window.open = false;
            return;
        }
        if apply {
            let new = self.view.audio_window.edit.clone();
            let project = self.doc.project.clone();
            self.apply_audio(new, &mut |s| open_audio(&project, s).map(|(l, n)| (Some(l), n)));
        }
    }
```

Row helpers (free functions in the same file):

```rust
fn selected_host<'a>(w: &'a AudioWindow) -> Option<&'a HostInfo> {
    let hosts = w.hosts.as_ref()?;
    let id = w.edit.driver.clone().unwrap_or_else(|| crate::audio_scan::default_host_name().into());
    hosts.iter().find(|h| h.id == id)
}

fn driver_row(ui: &mut egui::Ui, w: &mut AudioWindow) {
    ui.label("Driver:");
    let current = w.edit.driver.as_deref().map_or("Default".to_string(), |d| driver_label(d).to_string());
    egui::ComboBox::from_id_salt("audio_driver").selected_text(current).show_ui(ui, |ui| {
        if ui.selectable_label(w.edit.driver.is_none(), "Default").clicked() {
            w.edit = AudioSettings::default();
        }
        for id in w.hosts.iter().flatten().map(|h| h.id.clone()).collect::<Vec<_>>() {
            let on = w.edit.driver.as_deref() == Some(&id);
            if ui.selectable_label(on, driver_label(&id)).clicked() && !on {
                // A new driver starts from its defaults.
                w.edit = AudioSettings { driver: Some(id), ..AudioSettings::default() };
            }
        }
    });
}

fn device_row(ui: &mut egui::Ui, w: &mut AudioWindow) {
    ui.label("Device:");
    ui.horizontal(|ui| {
        let scanning = w.scan_rx.is_some();
        let text = if scanning && w.hosts.is_none() {
            "Scanning devices…".to_string()
        } else {
            w.edit.device.clone().unwrap_or_else(|| "Default".into())
        };
        let names: Vec<String> = selected_host(w)
            .map(|h| listed_devices(h, w.show_all).iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("audio_device").selected_text(text).show_ui(ui, |ui| {
            if ui.selectable_label(w.edit.device.is_none(), "Default").clicked() {
                w.edit.device = None;
            }
            for n in names {
                let on = w.edit.device.as_deref() == Some(&n);
                if ui.selectable_label(on, &n).clicked() {
                    w.edit.device = Some(n);
                }
            }
        });
        if ui.add_enabled(!scanning, egui::Button::new("Rescan")).clicked() {
            w.rescan();
        }
        ui.checkbox(&mut w.show_all, "Show all devices");
    });
}

fn rate_and_buffer_rows(ui: &mut egui::Ui, w: &mut AudioWindow) {
    let host = selected_host(w);
    let is_jack = host.is_some_and(|h| h.id == JACK);
    let dev = host.and_then(|h| {
        let name = w.edit.device.clone().or_else(|| h.default_device.clone())?;
        h.devices.iter().find(|d| d.name == name).cloned()
    });
    ui.label("Sample rate:");
    if is_jack {
        ui.weak("set by the JACK server");
    } else {
        let default = dev.as_ref().map_or("Default".into(), |d| format!("Default ({})", d.default_rate));
        let text = w.edit.sample_rate.map_or(default.clone(), |r| format!("{r} Hz"));
        let rates = dev.as_ref().map(rate_options).unwrap_or_default();
        egui::ComboBox::from_id_salt("audio_rate").selected_text(text).show_ui(ui, |ui| {
            if ui.selectable_label(w.edit.sample_rate.is_none(), default).clicked() {
                w.edit.sample_rate = None;
            }
            for r in rates {
                if ui.selectable_label(w.edit.sample_rate == Some(r), format!("{r} Hz")).clicked() {
                    w.edit.sample_rate = Some(r);
                }
            }
        });
    }
    ui.end_row();
    ui.label("Buffer:");
    if is_jack {
        ui.weak("set by the JACK server");
    } else {
        ui.horizontal(|ui| {
            let text = w.edit.buffer_frames.map_or("Default".into(), |n| format!("{n} frames"));
            let options = dev.as_ref().map(buffer_options).unwrap_or(BUFFERS.to_vec());
            egui::ComboBox::from_id_salt("audio_buffer").selected_text(text).show_ui(ui, |ui| {
                if ui.selectable_label(w.edit.buffer_frames.is_none(), "Default").clicked() {
                    w.edit.buffer_frames = None;
                }
                for n in options {
                    if ui.selectable_label(w.edit.buffer_frames == Some(n), format!("{n} frames")).clicked() {
                        w.edit.buffer_frames = Some(n);
                    }
                }
            });
            if let Some(n) = w.edit.buffer_frames {
                let rate = w.edit.sample_rate.or(dev.as_ref().map(|d| d.default_rate)).unwrap_or(48000);
                ui.weak(format!("≈ {:.1} ms", latency_ms(n, rate)));
            }
        });
    }
    ui.end_row();
}
```

Imports at the top of the file:

```rust
//! File → Audio Settings…: driver, device, sample rate and buffer.

use super::{SidAcidApp, open_audio, theme};
use crate::audio::AudioLink;
use crate::audio_select::{
    BUFFERS, HostInfo, JACK, buffer_options, driver_label, latency_ms, listed_devices, rate_options,
};
use crate::settings::AudioSettings;
use eframe::egui;
```

Hook-ups:
- `ui/mod.rs`: `mod audio_settings;`; `ViewState` gains `pub audio_window: audio_settings::AudioWindow,`; in `ui()`, right after `self.dialogs(ctx);` add `self.audio_settings_window(ctx);`.
- `ui/files.rs` `file_menu`, after the Export item's separator block, before Quit's separator:
```rust
            if ui.button("Audio Settings…").clicked() {
                ui.close_menu();
                self.open_audio_window();
            }
```
- `ui/perf.rs`: make the CPU label clickable — `ui.add(egui::Label::new(...).sense(egui::Sense::click()))`, and `if r.clicked() { self.open_audio_window(); }` (collect the click into a bool while `perf` is borrowed; call after).
- Remove the `#![allow(dead_code)]` added in Task 3 (and Task 2 if any).

- [ ] **Step 6: Run everything**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace`
Expected: all pass, no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/app/src/ui crates/app/src/audio_select.rs crates/app/src/settings.rs
git commit -m "feat(app): Audio Settings window — driver, device, rate, buffer; revert on failure"
```

---

### Task 7: Manual verification on the Deck (controller does this)

- [ ] **Step 1:** `cargo run --release -p sidacid`. Open **File → Audio Settings…**: "Scanning devices…" then lists fill; ALSA list shows only allowed names; **Show all devices** shows the plugin devices.
- [ ] **Step 2:** With the demo playing: Apply ALSA `default` → JACK → ALSA `pipewire`. Sound each time (playback stops on each Apply; press Play). The CPU hover shows the opened stream each time.
- [ ] **Step 3:** Pick JACK: rate/buffer rows read "set by the JACK server".
- [ ] **Step 4:** Force xruns: `pw-metadata -n settings 0 clock.force-quantum 64` while playing on JACK; ⚠ count rises, the hover shows driver xruns, playback continues. Restore: `pw-metadata -n settings 0 clock.force-quantum 0`.
- [ ] **Step 5:** `cat ~/.config/sidacid/settings.ron` reflects the last Apply. Restart app: same stream opens.
- [ ] **Step 6:** Edit the file to a bogus device; restart: fallback notice in the status line, sound plays, file unchanged.
- [ ] **Step 7:** Record results in `docs/superpowers/plans/2026-10-14-sidacid-plan9-deferred.txt` (anything not fixed), commit.

### Owed to the user (Windows)

Build with `--features asio` per README on the Windows PC, choose ASIO in Audio Settings, play the demo; report any compile errors (the ASIO path was not compiled on Linux).
