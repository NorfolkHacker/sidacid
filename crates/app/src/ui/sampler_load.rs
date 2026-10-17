//! Loading, clearing and editing sampler slots. Files are decoded here,
//! on the UI thread; zones are checked before the
//! undoable edit is applied.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::machine::MachineUid;
use acid_engine::params::{MachineParam, SlotParam};
use acid_engine::sampler::SLOTS;
use acid_engine::sampler::decode::decode;
use acid_engine::sampler::slicing::{detect_slices, equal_slices, guess_bpm, mono_mix};
use acid_engine::sampler::slot::{SlotData, SlotMeta};

/// Samples at least this long load as slice slots.
const SLICE_MIN_SECONDS: f32 = 1.0;
/// Keys a new chromatic zone covers.
const CHROMATIC_SPAN: u8 = 24;

impl SidAcidApp {
    pub(crate) fn load_sample_dialog(&mut self, uid: MachineUid, slot: usize) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Audio",
                &["wav", "flac", "mp3", "ogg", "WAV", "FLAC", "MP3", "OGG"],
            )
            .pick_file()
        {
            self.load_sample_file(uid, slot, &path);
        }
    }

    pub(crate) fn load_sample_file(
        &mut self,
        uid: MachineUid,
        slot: usize,
        path: &std::path::Path,
    ) {
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
                self.load_sample_bytes(uid, slot, &name, ext.as_deref(), &bytes);
            }
            Err(e) => self.view.message = Some(format!("Could not read {}:\n{e}", path.display())),
        }
    }

    /// Loads a decoded file into slot `slot` of sampler `uid`.
    pub(crate) fn load_sample_bytes(
        &mut self,
        uid: MachineUid,
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
        let Some(sampler) = self.doc.project.sampler(uid) else {
            return;
        };
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
        // Backstop: never put a slot in the project that would stop it reopening.
        if let Some(problem) = data.problem() {
            self.view.message = Some(format!("Could not load the sample: {problem}."));
            return;
        }
        let values = SlotParam::ALL
            .iter()
            .map(|&p| {
                let id = uid.param(MachineParam::SamplerSlot(slot as u8, p));
                let v = match p {
                    SlotParam::Choke | SlotParam::TempoLock if slice_mode => 1.0,
                    _ => id.info().default,
                };
                (id, v)
            })
            .collect();
        let cmds = self.doc.apply(
            Edit::SamplerSlot {
                uid,
                slot,
                data: Some(Box::new(data)),
                values,
            },
            false,
        );
        self.send(cmds);
        self.view.panel(uid).sampler_slot = slot;
        if decoded.truncated {
            self.view.message = Some(format!(
                "Loaded \"{short}\", but the file ends early: only the audio that could be read was kept."
            ));
        }
    }

    pub(crate) fn clear_slot(&mut self, uid: MachineUid, slot: usize) {
        if self
            .doc
            .project
            .sampler(uid)
            .and_then(|s| s.slots.get(slot))
            .is_some_and(Option::is_some)
        {
            let cmds = self.doc.apply(
                Edit::SamplerSlot {
                    uid,
                    slot,
                    data: None,
                    values: Vec::new(),
                },
                false,
            );
            self.send(cmds);
        }
    }

    /// Moves a slot's zone to start at `base`, keeping its span; refuses
    /// (with a message) a move that would push the top past key 127.
    pub(crate) fn move_zone(
        &mut self,
        uid: MachineUid,
        slot: usize,
        base: u8,
        merge: bool,
    ) -> bool {
        let Some(meta) = self
            .doc
            .project
            .sampler(uid)
            .and_then(|s| s.slots.get(slot))
            .and_then(Option::as_ref)
            .map(|d| &d.meta)
        else {
            return false;
        };
        let shift = base as i16 - meta.base_key as i16;
        if meta.key_range().1 as i16 + shift > 127 {
            self.view.message =
                Some("The zone can't move that high: its top key would pass G9.".into());
            return false;
        }
        self.edit_slot_meta(uid, slot, merge, |m| {
            m.base_key = base;
            // In slice mode high_key is unused; keep it at or above the base.
            m.high_key = (m.high_key as i16 + shift).clamp(base as i16, 127) as u8;
            m.root = (m.root as i16 + shift).clamp(0, 127) as u8;
        })
    }

    /// Applies `change` to a copy of the slot's meta; refuses zone clashes
    /// and malformed metas with a message (a malformed step of a drag,
    /// `merge`, is refused silently so the message doesn't flicker).
    pub(crate) fn edit_slot_meta(
        &mut self,
        uid: MachineUid,
        slot: usize,
        merge: bool,
        change: impl FnOnce(&mut SlotMeta),
    ) -> bool {
        let Some(sampler) = self.doc.project.sampler(uid) else {
            return false;
        };
        let Some(data) = sampler.slots.get(slot).and_then(Option::as_ref) else {
            return false;
        };
        let mut meta = data.meta.clone();
        change(&mut meta);
        if meta == data.meta {
            return false;
        }
        if !meta.is_well_formed(data.frames()) {
            if !merge {
                self.view.message = Some("That change doesn't fit this sample.".into());
            }
            return false;
        }
        if sampler.zone_clash(slot, &meta) {
            self.view.message = Some("That zone overlaps another slot's keys.".into());
            return false;
        }
        let cmds = self.doc.apply(
            Edit::SlotMeta {
                uid,
                slot,
                meta: Box::new(meta),
            },
            merge,
        );
        self.send(cmds);
        true
    }
}
