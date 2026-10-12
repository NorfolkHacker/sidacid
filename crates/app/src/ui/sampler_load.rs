//! Loading, clearing and editing sampler slots. Files are decoded here,
//! on the UI thread; zones and the memory limit are checked before the
//! undoable edit is applied.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::params::{ParamId, SlotParam};
use acid_engine::sampler::decode::decode;
use acid_engine::sampler::slicing::{detect_slices, equal_slices, guess_bpm, mono_mix};
use acid_engine::sampler::slot::{SlotData, SlotMeta};
use acid_engine::sampler::{MEMORY_LIMIT, SLOTS};

/// Samples at least this long load as slice slots.
const SLICE_MIN_SECONDS: f32 = 1.0;
/// Keys a new chromatic zone covers.
const CHROMATIC_SPAN: u8 = 24;

impl SidAcidApp {
    pub(crate) fn load_sample_dialog(&mut self, slot: usize) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Audio",
                &["wav", "flac", "mp3", "ogg", "WAV", "FLAC", "MP3", "OGG"],
            )
            .pick_file()
        {
            self.load_sample_file(slot, &path);
        }
    }

    pub(crate) fn load_sample_file(&mut self, slot: usize, path: &std::path::Path) {
        match std::fs::read(path) {
            Ok(bytes) => {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_lowercase);
                self.load_sample_bytes(slot, &name, ext.as_deref(), &bytes);
            }
            Err(e) => self.view.message = Some(format!("Could not read {}:\n{e}", path.display())),
        }
    }

    pub(crate) fn load_sample_bytes(
        &mut self,
        slot: usize,
        name: &str,
        extension: Option<&str>,
        bytes: &[u8],
    ) {
        if slot >= SLOTS {
            return;
        }
        // A codec can panic on a malformed file; treat that as a failed load
        // rather than crashing the UI.
        let decoded = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode(bytes, extension)
        })) {
            Ok(Ok(d)) => d,
            Ok(Err(e)) => {
                self.view.message = Some(format!("Could not load the sample: {e}."));
                return;
            }
            Err(_) => {
                self.view.message =
                    Some("Could not load the sample: the file is malformed.".into());
                return;
            }
        };
        let sampler = &self.doc.project.sampler;
        let old = sampler.slots[slot].as_ref().map_or(0, |d| d.memory_bytes());
        if sampler.memory_bytes() - old + decoded.samples.len() * 4 > MEMORY_LIMIT {
            self.view.message =
                Some("Not enough sample memory left (64 MB in all). Clear a slot first.".into());
            return;
        }
        let frames = decoded.frames() as u32;
        let slice_mode = decoded.seconds() >= SLICE_MIN_SECONDS;
        let mut slices = Vec::new();
        if slice_mode {
            slices = detect_slices(
                &mono_mix(&decoded.samples, decoded.channels as usize),
                decoded.sample_rate,
                0.5,
            );
            if slices.len() < 2 {
                slices = equal_slices(frames, 8);
            }
        }
        let span = if slice_mode {
            slices.len() as u8
        } else {
            CHROMATIC_SPAN
        };
        let Some(base) = sampler.free_base_key(slot, span) else {
            self.view.message = Some(
                "No free keys left for this sample's zone. Move or clear another slot.".into(),
            );
            return;
        };
        let meta = if slice_mode {
            SlotMeta::sliced(base, slices, guess_bpm(frames, decoded.sample_rate), frames)
        } else {
            SlotMeta::chromatic(base, base + CHROMATIC_SPAN - 1, base, frames)
        };
        let short: String = name.chars().take(24).collect();
        let data = SlotData::new(
            &short,
            decoded.sample_rate,
            decoded.channels,
            &decoded.samples,
            meta,
        );
        let values = SlotParam::ALL
            .iter()
            .map(|&p| {
                let id = ParamId::SamplerSlot(slot as u8, p);
                let v = match p {
                    SlotParam::Choke | SlotParam::TempoLock if slice_mode => 1.0,
                    _ => id.info().default,
                };
                (id, v)
            })
            .collect();
        let cmds = self.doc.apply(
            Edit::SamplerSlot {
                slot,
                data: Some(Box::new(data)),
                values,
            },
            false,
        );
        self.send(cmds);
        self.view.sampler_slot = slot;
    }

    pub(crate) fn clear_slot(&mut self, slot: usize) {
        if self
            .doc
            .project
            .sampler
            .slots
            .get(slot)
            .is_some_and(Option::is_some)
        {
            let cmds = self.doc.apply(
                Edit::SamplerSlot {
                    slot,
                    data: None,
                    values: Vec::new(),
                },
                false,
            );
            self.send(cmds);
        }
    }

    /// Applies `change` to a copy of the slot's meta; refuses zone clashes and malformed metas with a message.
    pub(crate) fn edit_slot_meta(
        &mut self,
        slot: usize,
        merge: bool,
        change: impl FnOnce(&mut SlotMeta),
    ) -> bool {
        let sampler = &self.doc.project.sampler;
        let Some(data) = sampler.slots.get(slot).and_then(Option::as_ref) else {
            return false;
        };
        let mut meta = data.meta.clone();
        change(&mut meta);
        if meta == data.meta {
            return false;
        }
        if !meta.is_well_formed(data.frames()) {
            return false;
        }
        if sampler.zone_clash(slot, &meta) {
            self.view.message = Some("That zone overlaps another slot's keys.".into());
            return false;
        }
        let cmds = self.doc.apply(
            Edit::SlotMeta {
                slot,
                meta: Box::new(meta),
            },
            merge,
        );
        self.send(cmds);
        true
    }
}
