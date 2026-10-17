//! Loading, clearing and editing Pad Kit pads and their layers. Files go
//! through the same decode and memory warning as the Sampler's; each
//! change is one undoable `PadKitPad` edit.

use super::SidAcidApp;
use super::mem_warn::MemAction;
use super::sampler_load::decode_file;
use crate::document::Edit;
use acid_engine::machine::MachineUid;
use acid_engine::padkit::pad::{Layer, Pad};
use acid_engine::padkit::{MAX_LAYERS, PADS};
use acid_engine::sampler::decode::Decoded;
use eframe::egui;

/// How `decode_file` and the layer backstop start their messages.
const SAMPLE_PREFIX: &str = "Could not load the sample: ";

impl SidAcidApp {
    #[allow(dead_code)] // the pad panel uses it in a later task
    pub(crate) fn load_pad_dialog(&mut self, uid: MachineUid, pad: usize) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Audio",
                &["wav", "flac", "mp3", "ogg", "WAV", "FLAC", "MP3", "OGG"],
            )
            .pick_file()
        {
            self.load_pad_file(uid, pad, &path);
        }
    }

    fn load_pad_file(&mut self, uid: MachineUid, pad: usize, path: &std::path::Path) {
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
                self.load_pad_bytes(uid, pad, &name, ext.as_deref(), &bytes);
            }
            Err(e) => self.view.message = Some(format!("Could not read {}:\n{e}", path.display())),
        }
    }

    /// Decodes a file for pad `pad` of Pad Kit `uid` and loads it, once the
    /// memory check passes or the user goes ahead.
    pub(crate) fn load_pad_bytes(
        &mut self,
        uid: MachineUid,
        pad: usize,
        name: &str,
        ext: Option<&str>,
        bytes: &[u8],
    ) {
        if pad >= PADS {
            return;
        }
        let decoded = match decode_file(bytes, ext) {
            Ok(d) => d,
            Err(msg) => {
                self.view.message = Some(msg);
                return;
            }
        };
        // Twice the decoded audio: the pad, and the undo history's copy.
        let extra = (decoded.samples.len() * size_of::<f32>() * 2) as u64;
        self.with_memory_check(
            extra,
            MemAction::LoadPad {
                uid,
                pad,
                name: name.to_string(),
                decoded: Box::new(decoded),
            },
        );
    }

    /// Adds a layer to the pad (re-splitting the ranges), or replaces a
    /// layer when the pad is full (keeping the ranges); no memory check.
    #[allow(dead_code)] // the pad panel uses it in a later task
    pub(crate) fn load_pad_decoded_now(
        &mut self,
        uid: MachineUid,
        pad: usize,
        name: &str,
        decoded: Decoded,
    ) {
        match self.put_pad_layer(uid, pad, name, &decoded) {
            Ok(Some(notice)) => self.view.message = Some(notice),
            Ok(None) => {}
            Err(msg) => self.view.message = Some(msg),
        }
    }

    /// `load_pad_decoded_now` returning what to tell the user instead of
    /// showing it: Ok(a notice, if any) or the error.
    fn put_pad_layer(
        &mut self,
        uid: MachineUid,
        pad: usize,
        name: &str,
        decoded: &Decoded,
    ) -> Result<Option<String>, String> {
        let Some(old) = self.doc.project.padkit(uid).and_then(|k| k.pads.get(pad)) else {
            return Ok(None);
        };
        let short: String = name.chars().take(24).collect();
        let layer = Layer::new(
            &short,
            decoded.sample_rate,
            decoded.channels,
            &decoded.samples,
        );
        // Backstop: never put a layer in the project that would stop it reopening.
        if let Some(problem) = layer.problem() {
            return Err(format!("Could not load the sample: {problem}."));
        }
        let mut data = old.clone();
        let index = if data.layers.len() >= MAX_LAYERS {
            // The selected layer tab belongs to the selected pad only.
            let panel = self.view.panel(uid);
            let i = if panel.pad == pad {
                panel.pad_layer.min(MAX_LAYERS - 1)
            } else {
                MAX_LAYERS - 1
            };
            let l = &mut data.layers[i];
            let (lo, hi) = (l.vel_lo, l.vel_hi);
            *l = layer;
            l.vel_lo = lo;
            l.vel_hi = hi;
            i
        } else {
            data.layers.push(layer);
            data.split_evenly();
            data.layers.len() - 1
        };
        self.apply_pad(uid, pad, data, false);
        let panel = self.view.panel(uid);
        panel.pad = pad;
        panel.pad_layer = index;
        Ok(decoded.truncated.then(|| {
            format!(
                "Loaded \"{short}\", but the file ends early: only the audio that could be read was kept."
            )
        }))
    }

    /// Loads files decoded together by `drop_pad_files` onto consecutive
    /// pads from `first`, and tells the user what was skipped or cut short.
    pub(crate) fn load_pads_now(
        &mut self,
        uid: MachineUid,
        first: usize,
        files: Vec<(String, Box<Decoded>)>,
        skipped: Vec<String>,
        total: usize,
    ) {
        let mut lines = Vec::new();
        let mut loaded = 0;
        for ((name, decoded), pad) in files.iter().zip(first..PADS) {
            match self.put_pad_layer(uid, pad, name, decoded) {
                Ok(notice) => {
                    loaded += 1;
                    lines.extend(notice);
                }
                Err(e) if total == 1 => lines.push(e),
                Err(e) => lines.push(format!(
                    "Could not load \"{name}\": {}",
                    e.trim_start_matches(SAMPLE_PREFIX)
                )),
            }
        }
        lines.extend(skipped);
        if total > 1 && loaded < total {
            lines.insert(0, format!("Loaded {loaded} of {total} files."));
        }
        if !lines.is_empty() {
            self.view.message = Some(lines.join("\n"));
        }
    }

    /// Removes one layer, re-splitting the ranges of the rest.
    #[allow(dead_code)] // the pad panel uses it in a later task
    pub(crate) fn clear_pad_layer(&mut self, uid: MachineUid, pad: usize, layer: usize) {
        let Some(old) = self.doc.project.padkit(uid).and_then(|k| k.pads.get(pad)) else {
            return;
        };
        if layer >= old.layers.len() {
            return;
        }
        let mut data = old.clone();
        data.layers.remove(layer);
        data.split_evenly();
        let last = data.layers.len().saturating_sub(1);
        self.apply_pad(uid, pad, data, false);
        let panel = self.view.panel(uid);
        panel.pad_layer = panel.pad_layer.min(last);
    }

    /// Empties the pad: its layers and its name.
    #[allow(dead_code)] // the pad panel uses it in a later task
    pub(crate) fn clear_pad(&mut self, uid: MachineUid, pad: usize) {
        let Some(old) = self.doc.project.padkit(uid).and_then(|k| k.pads.get(pad)) else {
            return;
        };
        if *old != Pad::default() {
            self.apply_pad(uid, pad, Pad::default(), false);
        }
        self.view.panel(uid).pad_layer = 0;
    }

    /// An empty or blank name goes back to the label from the first layer.
    #[allow(dead_code)] // the pad panel uses it in a later task
    pub(crate) fn rename_pad(&mut self, uid: MachineUid, pad: usize, name: &str) {
        let Some(old) = self.doc.project.padkit(uid).and_then(|k| k.pads.get(pad)) else {
            return;
        };
        let name = name.trim();
        if old.name == name {
            return;
        }
        let mut data = old.clone();
        data.name = name.to_string();
        self.apply_pad(uid, pad, data, false);
    }

    /// Sets a layer's velocity range, each end within 1..=127 and `lo`
    /// no higher than `hi` (`hi` is clamped first, then `lo` to it).
    #[allow(dead_code)] // the pad panel uses it in a later task
    pub(crate) fn set_layer_range(
        &mut self,
        uid: MachineUid,
        pad: usize,
        layer: usize,
        lo: u8,
        hi: u8,
        merge: bool,
    ) {
        let Some(old) = self.doc.project.padkit(uid).and_then(|k| k.pads.get(pad)) else {
            return;
        };
        let Some(l) = old.layers.get(layer) else {
            return;
        };
        let hi = hi.clamp(1, 127);
        let lo = lo.clamp(1, hi);
        if (l.vel_lo, l.vel_hi) == (lo, hi) {
            return;
        }
        let mut data = old.clone();
        data.layers[layer].vel_lo = lo;
        data.layers[layer].vel_hi = hi;
        self.apply_pad(uid, pad, data, merge);
    }

    /// Several files dropped on the grid: one per pad from `first`, in
    /// order; extras past pad 16 are ignored. All are decoded first and
    /// checked against memory together, so the user is asked once; files
    /// that fail are skipped (the rest take the pads in order) and named in
    /// one message.
    #[allow(dead_code)] // the pad panel uses it in a later task
    pub(crate) fn drop_pad_files(
        &mut self,
        uid: MachineUid,
        first: usize,
        files: &[egui::DroppedFile],
    ) {
        let files = &files[..files.len().min(PADS.saturating_sub(first))];
        let mut decoded = Vec::new();
        let mut skipped = Vec::new();
        for file in files {
            let stem_ext = std::path::Path::new(&file.name);
            let read;
            let bytes: &[u8] = if let Some(p) = &file.path {
                match std::fs::read(p) {
                    Ok(b) => {
                        read = b;
                        &read
                    }
                    Err(e) => {
                        skipped.push(format!("Could not read {}:\n{e}", p.display()));
                        continue;
                    }
                }
            } else if let Some(b) = &file.bytes {
                b
            } else {
                continue;
            };
            let named = file.path.as_deref().unwrap_or(stem_ext);
            let name = named
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = named
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_lowercase);
            match decode_file(bytes, ext.as_deref()) {
                Ok(d) => decoded.push((name, Box::new(d))),
                Err(e) => skipped.push(format!(
                    "Could not load \"{}\": {}",
                    file.name,
                    e.trim_start_matches(SAMPLE_PREFIX)
                )),
            }
        }
        let total = files.len();
        if decoded.is_empty() {
            // A lone file reports exactly as a single load does.
            self.view.message = match (total, skipped.len()) {
                (_, 0) => None,
                (1, _) => Some(skipped.remove(0)),
                _ => Some(format!(
                    "Loaded 0 of {total} files.\n{}",
                    skipped.join("\n")
                )),
            };
            return;
        }
        // Twice the decoded audio: the pads, and the undo history's copies.
        let extra = decoded
            .iter()
            .map(|(_, d): &(String, Box<Decoded>)| (d.samples.len() * size_of::<f32>() * 2) as u64)
            .sum();
        self.with_memory_check(
            extra,
            MemAction::LoadPads {
                uid,
                first,
                files: decoded,
                skipped,
                total,
            },
        );
    }

    fn apply_pad(&mut self, uid: MachineUid, pad: usize, data: Pad, merge: bool) {
        let cmds = self.doc.apply(
            Edit::PadKitPad {
                uid,
                pad,
                data: Box::new(data),
                values: Vec::new(),
            },
            merge,
        );
        self.send(cmds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::machine::MachineType;
    use acid_engine::project::Project;

    fn app_with_kit() -> (SidAcidApp, MachineUid) {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        (app, uid)
    }

    fn pad(app: &SidAcidApp, uid: MachineUid, i: usize) -> &Pad {
        &app.doc.project.padkit(uid).unwrap().pads[i]
    }

    #[test]
    fn loads_add_layers_split_evenly_and_a_fifth_replaces_the_selected() {
        let (mut app, uid) = app_with_kit();
        let wav = crate::ui::tests::test_wav(0.1);
        for n in 0..4 {
            app.load_pad_bytes(uid, 0, &format!("l{n}"), Some("wav"), &wav);
        }
        let ranges: Vec<_> = pad(&app, uid, 0)
            .layers
            .iter()
            .map(|l| (l.vel_lo, l.vel_hi))
            .collect();
        assert_eq!(ranges, [(1, 31), (32, 63), (64, 95), (96, 127)]);
        app.view.panel(uid).pad_layer = 2;
        app.load_pad_bytes(uid, 0, "new", Some("wav"), &wav);
        let p = pad(&app, uid, 0);
        assert_eq!(p.layers.len(), 4);
        assert_eq!(p.layers[2].name, "new");
        assert_eq!(
            (p.layers[2].vel_lo, p.layers[2].vel_hi),
            (64, 95),
            "ranges kept"
        );
    }

    #[test]
    fn clear_rename_and_range_edits_each_undo() {
        let (mut app, uid) = app_with_kit();
        let wav = crate::ui::tests::test_wav(0.1);
        app.load_pad_bytes(uid, 1, "a", Some("wav"), &wav);
        app.load_pad_bytes(uid, 1, "b", Some("wav"), &wav);
        app.rename_pad(uid, 1, "Snare");
        assert_eq!(pad(&app, uid, 1).label(1), "Snare");
        app.set_layer_range(uid, 1, 0, 90, 10, false);
        assert_eq!(
            (
                pad(&app, uid, 1).layers[0].vel_lo,
                pad(&app, uid, 1).layers[0].vel_hi
            ),
            (10, 10)
        );
        app.clear_pad_layer(uid, 1, 0);
        assert_eq!(pad(&app, uid, 1).layers.len(), 1);
        assert_eq!(
            (
                pad(&app, uid, 1).layers[0].vel_lo,
                pad(&app, uid, 1).layers[0].vel_hi
            ),
            (1, 127)
        );
        for _ in 0..3 {
            app.doc.undo();
        }
        assert_eq!(pad(&app, uid, 1).label(1), "a");
        app.clear_pad(uid, 1);
        assert!(pad(&app, uid, 1).layers.is_empty());
    }

    #[test]
    fn a_blank_name_falls_back_and_range_drags_merge() {
        let (mut app, uid) = app_with_kit();
        let wav = crate::ui::tests::test_wav(0.1);
        app.load_pad_bytes(uid, 2, "kick", Some("wav"), &wav);
        app.rename_pad(uid, 2, "Boom");
        app.rename_pad(uid, 2, "   ");
        assert_eq!(pad(&app, uid, 2).name, "");
        assert_eq!(pad(&app, uid, 2).label(2), "kick");
        for hi in [100, 90, 80] {
            app.set_layer_range(uid, 2, 0, 1, hi, true);
        }
        assert_eq!(pad(&app, uid, 2).layers[0].vel_hi, 80);
        app.doc.undo();
        assert_eq!(pad(&app, uid, 2).layers[0].vel_hi, 127, "one step");
    }

    #[test]
    fn dropped_files_fill_pads_from_the_first_and_ignore_the_extras() {
        let (mut app, uid) = app_with_kit();
        let wav = crate::ui::tests::test_wav(0.1);
        let files: Vec<egui::DroppedFile> = (0..3)
            .map(|n| egui::DroppedFile {
                name: format!("f{n}.wav"),
                bytes: Some(wav.clone().into()),
                ..Default::default()
            })
            .collect();
        app.drop_pad_files(uid, 14, &files);
        assert_eq!(pad(&app, uid, 14).layers[0].name, "f0");
        assert_eq!(pad(&app, uid, 15).layers[0].name, "f1");
        assert!(
            app.doc.project.padkit(uid).unwrap().pads[..14]
                .iter()
                .all(|p| p.layers.is_empty())
        );
    }

    fn dropped(name: &str, bytes: &[u8]) -> egui::DroppedFile {
        egui::DroppedFile {
            name: name.into(),
            bytes: Some(bytes.to_vec().into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_garbage_file_in_a_drop_is_named_and_the_rest_take_the_pads_in_order() {
        let (mut app, uid) = app_with_kit();
        let wav = crate::ui::tests::test_wav(0.1);
        let files = [
            dropped("a.wav", &wav),
            dropped("x.wav", b"nope"),
            dropped("c.wav", &wav),
        ];
        app.drop_pad_files(uid, 4, &files);
        assert_eq!(pad(&app, uid, 4).layers[0].name, "a");
        assert_eq!(pad(&app, uid, 5).layers[0].name, "c");
        assert!(pad(&app, uid, 6).layers.is_empty());
        let msg = app.view.message.unwrap();
        assert!(msg.starts_with("Loaded 2 of 3 files."), "{msg}");
        assert!(msg.contains("\"x.wav\""), "{msg}");
    }

    #[test]
    fn a_two_file_drop_under_low_memory_asks_once() {
        use crate::sysmem::{GIB, MIB, MemSnapshot};
        let wav = crate::ui::tests::test_wav(0.1);
        let files = [dropped("a.wav", &wav), dropped("b.wav", &wav)];
        for go in [true, false] {
            let (mut app, uid) = app_with_kit();
            app.sysmem = Box::new(crate::sysmem::FakeMemory(Some(MemSnapshot {
                total: 4 * GIB,
                available: 512 * MIB,
            })));
            app.mem = None;
            app.drop_pad_files(uid, 0, &files);
            assert!(app.pending_warn.is_some());
            assert!(pad(&app, uid, 0).layers.is_empty(), "held");
            app.resolve_memory_warning(go);
            assert_eq!(pad(&app, uid, 0).layers.len(), go as usize);
            assert_eq!(pad(&app, uid, 1).layers.len(), go as usize);
        }
    }

    #[test]
    fn a_load_onto_a_full_pad_that_is_not_selected_replaces_its_last_layer() {
        let (mut app, uid) = app_with_kit();
        let wav = crate::ui::tests::test_wav(0.1);
        for n in 0..4 {
            app.load_pad_bytes(uid, 3, &format!("l{n}"), Some("wav"), &wav);
        }
        app.view.panel(uid).pad = 0;
        app.view.panel(uid).pad_layer = 1;
        app.load_pad_bytes(uid, 3, "new", Some("wav"), &wav);
        let names: Vec<_> = pad(&app, uid, 3)
            .layers
            .iter()
            .map(|l| l.name.clone())
            .collect();
        assert_eq!(names, ["l0", "l1", "l2", "new"]);
    }

    #[test]
    fn a_bad_file_shows_the_samplers_message() {
        let (mut app, uid) = app_with_kit();
        app.load_pad_bytes(uid, 0, "x", Some("wav"), b"nope");
        assert!(
            app.view
                .message
                .as_deref()
                .unwrap()
                .starts_with("Could not load the sample")
        );
        assert!(pad(&app, uid, 0).layers.is_empty());
    }
}
