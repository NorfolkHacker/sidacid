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

#[allow(dead_code)] // used by the settings UI (Task 6)
pub fn scan_all() -> Vec<HostInfo> {
    cpal::available_hosts()
        .into_iter()
        .filter_map(scan_host)
        .collect()
}

pub fn scan_host(id: cpal::HostId) -> Option<HostInfo> {
    let host = cpal::host_from_id(id).ok()?;
    let default_device = host.default_output_device().and_then(|d| d.name().ok());
    let devices = host
        .output_devices()
        .map(|it| it.filter_map(|d| device_info(&d)).collect())
        .unwrap_or_default();
    Some(HostInfo {
        id: id.name().to_string(),
        default_device,
        devices,
    })
}

fn device_info(device: &cpal::Device) -> Option<DeviceInfo> {
    let name = device.name().ok()?;
    let default = device.default_output_config().ok()?;
    let (format, channels) = (default.sample_format(), default.channels());
    let default_rate = default.sample_rate().0;
    let ranges = device
        .supported_output_configs()
        .map(|it| {
            it.filter(|c| c.sample_format() == format && c.channels() == channels)
                .map(|c| (c.min_sample_rate().0, c.max_sample_rate().0))
                .collect()
        })
        .unwrap_or_default();
    Some(DeviceInfo {
        name,
        default_rate,
        rates: rates_or_default(ranges, default_rate),
        buffer: buffer_range(default.buffer_size()),
    })
}

/// A device always offers at least its default rate.
fn rates_or_default(ranges: Vec<(u32, u32)>, default_rate: u32) -> Vec<(u32, u32)> {
    if ranges.is_empty() {
        vec![(default_rate, default_rate)]
    } else {
        ranges
    }
}

pub(crate) fn buffer_range(b: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match *b {
        cpal::SupportedBufferSize::Range { min, max } => Some((min, max)),
        cpal::SupportedBufferSize::Unknown => None,
    }
}

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
        assert_eq!(
            buffer_range(&cpal::SupportedBufferSize::Range { min: 64, max: 2048 }),
            Some((64, 2048))
        );
        assert_eq!(buffer_range(&cpal::SupportedBufferSize::Unknown), None);
    }

    #[test]
    fn empty_rate_ranges_fall_back_to_the_default_rate() {
        assert_eq!(rates_or_default(vec![], 48000), vec![(48000, 48000)]);
        assert_eq!(
            rates_or_default(vec![(44100, 96000)], 48000),
            vec![(44100, 96000)]
        );
    }
}
