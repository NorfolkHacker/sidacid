//! Per-machine app settings (today: the audio device), kept out of
//! projects so a project opened on another computer does not try to use
//! this one's sound card.

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const SETTINGS_VERSION: u32 = 1;

#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
pub fn load(path: &Path) -> (Settings, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), None),
        Err(e) => {
            return (
                Settings::default(),
                Some(format!("Could not read audio settings: {e}")),
            );
        }
    };
    let bad = |e: ron::error::SpannedError| {
        (
            Settings::default(),
            Some(format!("Could not read audio settings: {e}")),
        )
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
#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sidacid-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| {
            vars.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
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
            Some(
                PathBuf::from("C:\\Users\\a\\AppData\\Roaming")
                    .join("SidAcid")
                    .join("settings.ron")
            )
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
