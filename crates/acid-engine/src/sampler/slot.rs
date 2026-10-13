//! A sample slot as the project stores it: its zone, slices or chromatic
//! range, and 16-bit PCM (saved deflated + base64). `SlotPlay` is the
//! fixed-size copy the audio thread uses.

use super::sample::SampleData;
use super::{MAX_SECONDS, MAX_SLICES};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotMode {
    #[default]
    Slice,
    Chromatic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotMeta {
    pub mode: SlotMode,
    /// Lowest key of the zone; slice 0 in slice mode.
    pub base_key: u8,
    /// Highest key (chromatic mode; slice mode derives it from the slices).
    pub high_key: u8,
    /// Key that plays the sample at its own pitch (chromatic).
    pub root: u8,
    /// Slice start frames, strictly ascending (slice mode).
    pub slices: Vec<u32>,
    /// The sample's tempo, for tempo lock (slice mode).
    pub original_bpm: f32,
    /// Play range in frames, end exclusive (chromatic).
    pub start: u32,
    pub end: u32,
    pub loop_on: bool,
    pub loop_start: u32,
    pub loop_end: u32,
}

impl SlotMeta {
    pub fn sliced(base_key: u8, slices: Vec<u32>, original_bpm: f32, frames: u32) -> Self {
        Self {
            mode: SlotMode::Slice,
            base_key,
            high_key: base_key.saturating_add(23).min(127),
            root: base_key,
            slices,
            original_bpm,
            start: 0,
            end: frames,
            loop_on: false,
            loop_start: 0,
            loop_end: frames,
        }
    }

    pub fn chromatic(base_key: u8, high_key: u8, root: u8, frames: u32) -> Self {
        Self {
            mode: SlotMode::Chromatic,
            base_key,
            high_key,
            root,
            slices: vec![0],
            original_bpm: 120.0,
            start: 0,
            end: frames,
            loop_on: false,
            loop_start: 0,
            loop_end: frames,
        }
    }

    /// Lowest and highest key of the zone. A slice zone may run past 127;
    /// `is_well_formed` rejects that.
    pub fn key_range(&self) -> (u8, u8) {
        match self.mode {
            SlotMode::Slice => {
                let n = self.slices.len().max(1) as u16;
                (self.base_key, (self.base_key as u16 + n - 1).min(255) as u8)
            }
            SlotMode::Chromatic => (self.base_key, self.high_key),
        }
    }

    pub fn is_well_formed(&self, frames: u32) -> bool {
        let slices_ok = (1..=MAX_SLICES).contains(&self.slices.len())
            && self.slices.windows(2).all(|w| w[0] < w[1])
            && self.slices.iter().all(|&s| s < frames);
        let top = match self.mode {
            SlotMode::Slice => self.base_key as usize + self.slices.len().max(1) - 1,
            SlotMode::Chromatic => self.high_key as usize,
        };
        slices_ok
            && self.base_key <= self.high_key
            && top <= 127
            && self.root <= 127
            && self.start < self.end
            && self.end <= frames
            && self.start <= self.loop_start
            && self.loop_start < self.loop_end
            && self.loop_end <= self.end
            && self.original_bpm.is_finite()
            && (20.0..=400.0).contains(&self.original_bpm)
    }
}

/// Embedded PCM: 16-bit little-endian samples, deflated, base64.
pub mod pcm_codec {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
    use serde::{Deserialize, Deserializer, Serializer};
    use std::cell::Cell;
    use std::io::{Read, Write};

    thread_local! {
        /// PCM bytes still allowed in the project being parsed on this
        /// thread; `None` outside `with_budget`.
        static BUDGET: Cell<Option<u64>> = const { Cell::new(None) };
    }

    /// Runs `f` (a parse) with at most `bytes` of embedded PCM allowed in
    /// all, so a hostile file can't inflate more audio than a project may
    /// hold before validation sees it.
    pub fn with_budget<R>(bytes: u64, f: impl FnOnce() -> R) -> R {
        struct Restore(Option<u64>);
        impl Drop for Restore {
            fn drop(&mut self) {
                BUDGET.with(|b| b.set(self.0));
            }
        }
        let _restore = Restore(BUDGET.with(|b| b.replace(Some(bytes))));
        f()
    }

    pub fn encode(pcm: &[i16]) -> String {
        let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&bytes).expect("writing to a Vec cannot fail");
        STANDARD.encode(enc.finish().expect("writing to a Vec cannot fail"))
    }

    pub fn decode(text: &str) -> Result<Vec<i16>, String> {
        let packed = STANDARD.decode(text).map_err(|e| e.to_string())?;
        // Longest valid sample (stereo, top rate) in bytes; caps a deflate bomb.
        const MAX_BYTES: u64 = (crate::sampler::MAX_SECONDS * 192_000.0) as u64 * 2 * 2;
        let budget = BUDGET.with(Cell::get);
        let limit = budget.map_or(MAX_BYTES, |b| b.min(MAX_BYTES));
        let mut bytes = Vec::new();
        DeflateDecoder::new(&packed[..])
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let len = bytes.len() as u64;
        if len > MAX_BYTES {
            return Err("embedded audio is too large".into());
        }
        if let Some(b) = budget {
            if len > b {
                return Err("embedded audio is over the sample memory limit".into());
            }
            BUDGET.with(|c| c.set(Some(b - len)));
        }
        if bytes.len() % 2 != 0 {
            return Err("odd number of PCM bytes".into());
        }
        Ok(bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect())
    }

    pub fn serialize<S: Serializer>(pcm: &[i16], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(pcm))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<i16>, D::Error> {
        let text = String::deserialize(d)?;
        decode(&text).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotData {
    pub name: String,
    pub meta: SlotMeta,
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u8,
    /// Interleaved 16-bit samples.
    #[serde(with = "pcm_codec")]
    pub pcm: Vec<i16>,
}

impl SlotData {
    /// A slot from decoded audio (interleaved, -1..1).
    pub fn new(
        name: &str,
        sample_rate: u32,
        channels: u8,
        samples: &[f32],
        meta: SlotMeta,
    ) -> Self {
        Self {
            name: name.to_string(),
            meta,
            sample_rate,
            channels,
            pcm: samples
                .iter()
                .map(|x| (x.clamp(-1.0, 1.0) * 32_767.0).round() as i16)
                .collect(),
        }
    }

    pub fn frames(&self) -> u32 {
        (self.pcm.len() / self.channels.max(1) as usize) as u32
    }

    /// Size of the decoded f32 audio the engine holds.
    pub fn memory_bytes(&self) -> usize {
        self.pcm.len() * 4
    }

    /// Builds the engine's copy. Allocates: call off the audio thread.
    /// Expects a valid slot (`is_valid`); otherwise only whole frames are
    /// read, and a slot with more than two channels plays its first one.
    pub fn to_sample(&self) -> SampleData {
        let ch = self.channels.max(1) as usize;
        let whole = &self.pcm[..self.frames() as usize * ch];
        let side = |c: usize| -> Box<[f32]> {
            whole
                .iter()
                .skip(c)
                .step_by(ch)
                .map(|&s| s as f32 / 32_768.0)
                .collect()
        };
        SampleData {
            sample_rate: self.sample_rate,
            left: side(0),
            right: (ch == 2).then(|| side(1)),
        }
    }

    /// Why this slot could not be saved and reopened, if it could not.
    pub fn problem(&self) -> Option<&'static str> {
        let frames = self.frames();
        if !(1..=2).contains(&self.channels) {
            Some("unsupported channel count")
        } else if !self.pcm.len().is_multiple_of(self.channels as usize) {
            Some("the audio is not a whole number of frames")
        } else if !(8_000..=192_000).contains(&self.sample_rate) {
            Some("sample rate must be 8–192 kHz")
        } else if frames == 0 {
            Some("the sample is empty")
        } else if frames as f32 > MAX_SECONDS * self.sample_rate as f32 {
            Some("longer than 30 s")
        } else if !self.meta.is_well_formed(frames) {
            Some("slice/zone layout doesn't fit")
        } else {
            None
        }
    }

    pub fn is_valid(&self) -> bool {
        self.problem().is_none()
    }
}

/// The audio thread's fixed-size copy of a `SlotMeta`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotPlay {
    pub mode: SlotMode,
    pub base_key: u8,
    pub high_key: u8,
    pub root: u8,
    pub slice_count: usize,
    pub slices: [u32; MAX_SLICES],
    pub original_bpm: f32,
    pub start: u32,
    pub end: u32,
    pub loop_on: bool,
    pub loop_start: u32,
    pub loop_end: u32,
}

impl SlotPlay {
    pub const EMPTY: SlotPlay = SlotPlay {
        mode: SlotMode::Slice,
        base_key: 0,
        high_key: 0,
        root: 0,
        slice_count: 0,
        slices: [0; MAX_SLICES],
        original_bpm: 120.0,
        start: 0,
        end: 0,
        loop_on: false,
        loop_start: 0,
        loop_end: 0,
    };

    pub fn from_meta(m: &SlotMeta) -> Self {
        let mut slices = [0u32; MAX_SLICES];
        let count = m.slices.len().min(MAX_SLICES);
        slices[..count].copy_from_slice(&m.slices[..count]);
        Self {
            mode: m.mode,
            base_key: m.base_key,
            high_key: m.high_key,
            root: m.root,
            slice_count: count,
            slices,
            original_bpm: m.original_bpm,
            start: m.start,
            end: m.end,
            loop_on: m.loop_on,
            loop_start: m.loop_start,
            loop_end: m.loop_end,
        }
    }

    pub fn key_range(&self) -> (u8, u8) {
        match self.mode {
            SlotMode::Slice => {
                let n = self.slice_count.max(1) as u16;
                (self.base_key, (self.base_key as u16 + n - 1).min(255) as u8)
            }
            SlotMode::Chromatic => (self.base_key, self.high_key),
        }
    }

    /// Whether `other` has the same zone layout: mode and key range.
    /// Markers, slice positions, root and tempo may differ (a change in
    /// slice count changes the key range).
    pub fn same_zones(&self, other: &SlotPlay) -> bool {
        self.mode == other.mode && self.key_range() == other.key_range()
    }
}

impl Default for SlotPlay {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, channels: usize) -> Vec<f32> {
        (0..frames * channels)
            .map(|i| ((i as f32) * 0.01).sin() * 0.7)
            .collect()
    }

    #[test]
    fn problem_names_why_a_slot_is_invalid() {
        let good = |rate: u32, ch: u8, frames: usize| {
            SlotData::new(
                "t",
                rate,
                ch,
                &tone(frames, ch as usize),
                SlotMeta::chromatic(24, 48, 24, frames as u32),
            )
        };
        assert_eq!(good(22_050, 1, 1000).problem(), None);
        assert!(good(22_050, 1, 1000).is_valid());
        assert_eq!(
            good(22_050, 3, 1000).problem(),
            Some("unsupported channel count")
        );
        assert_eq!(
            good(6_000, 1, 1000).problem(),
            Some("sample rate must be 8–192 kHz")
        );
        assert_eq!(good(22_050, 1, 0).problem(), Some("the sample is empty"));
        assert_eq!(
            good(8_000, 1, 8_000 * 31).problem(),
            Some("longer than 30 s")
        );
        let mut bad_meta = good(22_050, 1, 1000);
        bad_meta.meta.end = 5000;
        assert_eq!(bad_meta.problem(), Some("slice/zone layout doesn't fit"));
        let mut odd = good(22_050, 2, 1000);
        odd.pcm.pop();
        assert_eq!(
            odd.problem(),
            Some("the audio is not a whole number of frames")
        );
        assert!(!odd.is_valid());
    }

    #[test]
    fn pcm_codec_round_trips_bit_exactly() {
        let pcm: Vec<i16> = (0..10_000)
            .map(|i| ((i * 7919) % 65_536 - 32_768) as i16)
            .collect();
        let text = pcm_codec::encode(&pcm);
        assert_eq!(pcm_codec::decode(&text).unwrap(), pcm);
        assert_eq!(
            pcm_codec::decode(&pcm_codec::encode(&[])).unwrap(),
            Vec::<i16>::new()
        );
        assert!(pcm_codec::decode("not base64 !!").is_err());
    }

    #[test]
    fn decode_refuses_a_deflate_bomb() {
        use base64::{Engine, engine::general_purpose::STANDARD};
        use flate2::{Compression, write::DeflateEncoder};
        use std::io::Write;
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
        let zeros = vec![0u8; 1 << 20];
        for _ in 0..(((MAX_SECONDS * 192_000.0) as usize * 4) >> 20) + 2 {
            enc.write_all(&zeros).unwrap();
        }
        let text = STANDARD.encode(enc.finish().unwrap());
        assert!(pcm_codec::decode(&text).is_err());
    }

    #[test]
    fn is_valid_rejects_malformed_slots() {
        let ok = || {
            SlotData::new(
                "t",
                44_100,
                1,
                &tone(100, 1),
                SlotMeta::chromatic(40, 50, 40, 100),
            )
        };
        assert!(ok().is_valid());
        let mut d = ok();
        d.channels = 0;
        assert!(!d.is_valid());
        d.channels = 3;
        assert!(!d.is_valid());
        let mut d = SlotData::new(
            "t",
            44_100,
            2,
            &tone(100, 2),
            SlotMeta::chromatic(40, 50, 40, 100),
        );
        d.pcm.pop();
        assert!(!d.is_valid(), "pcm not divisible by channels");
        let mut d = ok();
        d.sample_rate = 4000;
        assert!(!d.is_valid());
        let mut d = ok();
        d.meta.original_bpm = f32::NAN;
        assert!(!d.is_valid());
    }

    #[test]
    fn slot_data_converts_and_measures() {
        let d = SlotData::new(
            "Amen",
            44_100,
            2,
            &tone(1000, 2),
            SlotMeta::sliced(36, vec![0, 500], 120.0, 1000),
        );
        assert_eq!(d.frames(), 1000);
        assert_eq!(d.memory_bytes(), 1000 * 2 * 4);
        let s = d.to_sample();
        assert_eq!(s.frames(), 1000);
        assert!(s.right.is_some());
        assert!((s.left[10] - (10.0 * 2.0 * 0.01f32).sin() * 0.7).abs() < 1e-4);
        assert!(d.is_valid());
    }

    #[test]
    fn to_sample_drops_a_partial_trailing_frame() {
        let mut d = SlotData::new(
            "t",
            44_100,
            2,
            &tone(3, 2),
            SlotMeta::chromatic(40, 50, 40, 3),
        );
        d.pcm.truncate(5);
        let s = d.to_sample();
        assert_eq!(s.left.len(), 2);
        assert_eq!(s.right.as_ref().map(|r| r.len()), Some(2));
    }

    #[test]
    fn key_ranges_and_well_formedness() {
        let s = SlotMeta::sliced(36, vec![0, 100, 200, 300], 120.0, 1000);
        assert_eq!(s.key_range(), (36, 39));
        assert!(s.is_well_formed(1000));
        assert!(
            !SlotMeta::sliced(36, vec![0, 300, 200], 120.0, 1000).is_well_formed(1000),
            "unsorted"
        );
        assert!(
            !SlotMeta::sliced(36, vec![0, 1000], 120.0, 1000).is_well_formed(1000),
            "slice past the end"
        );
        assert!(
            !SlotMeta::sliced(36, vec![], 120.0, 1000).is_well_formed(1000),
            "no slices"
        );
        assert!(
            !SlotMeta::sliced(126, vec![0, 1, 2, 3], 120.0, 1000).is_well_formed(1000),
            "zone past 127"
        );
        let c = SlotMeta::chromatic(48, 71, 60, 1000);
        assert_eq!(c.key_range(), (48, 71));
        assert!(c.is_well_formed(1000));
        let mut bad = c.clone();
        bad.loop_on = true;
        bad.loop_start = 900;
        bad.loop_end = 800;
        assert!(!bad.is_well_formed(1000));
    }

    #[test]
    fn slot_play_copies_the_meta() {
        let m = SlotMeta::sliced(40, vec![0, 10, 20], 100.0, 50);
        let p = SlotPlay::from_meta(&m);
        assert_eq!(p.slice_count, 3);
        assert_eq!(&p.slices[..3], &[0, 10, 20]);
        assert_eq!(p.key_range(), (40, 42));
        assert_eq!(p.end, 50);
        assert_eq!(SlotPlay::EMPTY.slice_count, 0);
    }
}
