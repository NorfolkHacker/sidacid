//! Choosing what audio stream to open, from plain data about what the
//! system offers. No cpal here: `audio_scan` builds the `HostInfo`s.
// Used from Tasks 4–6
#![allow(dead_code)]

use crate::settings::AudioSettings;

pub const JACK: &str = "JACK";
pub const ALSA: &str = "ALSA";
pub const RATES: [u32; 6] = [44100, 48000, 88200, 96000, 176400, 192000];
pub const BUFFERS: [u32; 7] = [64, 128, 256, 512, 1024, 2048, 4096];

/// One output device as the host reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    pub default_rate: u32,
    /// Inclusive sample-rate ranges for the device's default format and channel count.
    pub rates: Vec<(u32, u32)>,
    /// Inclusive buffer range in frames; None = the driver does not say.
    pub buffer: Option<(u32, u32)>,
}

/// One audio driver (cpal host) and the output devices it offers.
#[derive(Clone, Debug, PartialEq)]
pub struct HostInfo {
    pub id: String, // HostId::name()
    pub default_device: Option<String>,
    pub devices: Vec<DeviceInfo>,
}

/// The stream settings that will actually be opened.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub driver: String,
    pub device: Option<String>, // None = the host's default device (none found by name)
    pub sample_rate: Option<u32>, // None = device default
    pub buffer_frames: Option<u32>, // None = driver default
}

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
    dev.buffer
        .is_none_or(|(lo, hi)| (lo..=hi).contains(&frames))
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
            notes.push(format!(
                "{r} Hz is not supported — using {} Hz",
                d.default_rate
            ));
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

    fn saved(
        driver: Option<&str>,
        device: Option<&str>,
        rate: Option<u32>,
        buf: Option<u32>,
    ) -> AudioSettings {
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
            Choice {
                driver: "ALSA".into(),
                device: Some("default".into()),
                sample_rate: None,
                buffer_frames: None
            }
        );
        assert!(notes.is_empty());
    }

    #[test]
    fn a_fully_valid_saved_setup_is_kept() {
        let s = saved(Some("ALSA"), Some("pipewire"), Some(96000), Some(256));
        let (c, notes) = choose(&hosts(), "ALSA", &s).unwrap();
        assert_eq!(
            c,
            Choice {
                driver: "ALSA".into(),
                device: Some("pipewire".into()),
                sample_rate: Some(96000),
                buffer_frames: Some(256)
            }
        );
        assert!(notes.is_empty());
    }

    #[test]
    fn a_missing_driver_falls_back_to_the_default_host() {
        let s = saved(Some("ASIO"), None, None, None);
        let (c, notes) = choose(&hosts(), "ALSA", &s).unwrap();
        assert_eq!(c.driver, "ALSA");
        assert_eq!(
            notes,
            ["Driver 'ASIO' is not available — using 'ALSA / PipeWire'"]
        );
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
            Choice {
                driver: "JACK".into(),
                device: Some("cpal_client_out".into()),
                sample_rate: None,
                buffer_frames: None
            }
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
        assert_eq!(
            choose(&[], "ALSA", &AudioSettings::default()).unwrap_err(),
            "no audio driver available"
        );
        let empty = vec![HostInfo {
            id: "ALSA".into(),
            default_device: None,
            devices: vec![],
        }];
        assert_eq!(
            choose(&empty, "ALSA", &AudioSettings::default()).unwrap_err(),
            "no audio output device found"
        );
    }

    #[test]
    fn the_alsa_filter_keeps_real_outputs_from_the_deck_list() {
        let deck = [
            "sysdefault",
            "lavrate",
            "samplerate",
            "speexrate",
            "jack",
            "pipewire",
            "pulse",
            "speex",
            "upmix",
            "vdownmix",
            "default",
            "sysdefault:CARD=CODEC",
            "front:CARD=CODEC,DEV=0",
            "surround40:CARD=CODEC,DEV=0",
            "iec958:CARD=CODEC,DEV=0",
            "hdmi:CARD=Generic,DEV=0",
            "hw:CARD=USB,DEV=0",
            "plughw:CARD=USB,DEV=0",
        ];
        let kept: Vec<&str> = deck.iter().copied().filter(|n| alsa_listed(n)).collect();
        assert_eq!(
            kept,
            [
                "jack",
                "pipewire",
                "pulse",
                "default",
                "sysdefault:CARD=CODEC",
                "front:CARD=CODEC,DEV=0",
                "hdmi:CARD=Generic,DEV=0",
                "hw:CARD=USB,DEV=0",
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
        assert_eq!(
            listed_devices(&h[1], false).len(),
            2,
            "JACK is not filtered"
        );
    }

    #[test]
    fn rate_and_buffer_menus_follow_the_supported_ranges() {
        let d = dev("default");
        assert_eq!(rate_options(&d), [44100, 48000, 96000]);
        assert_eq!(buffer_options(&d), [64, 128, 256, 512, 1024, 2048]);
        let open = DeviceInfo {
            buffer: None,
            ..dev("x")
        };
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
