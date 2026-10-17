//! Saving and loading a whole Pad Kit as a `.padkit` file. A load goes
//! through the same decompression budget and memory warning as opening a
//! project, and is one undo step.

use super::SidAcidApp;
use super::mem_warn::MemAction;
use crate::document::Edit;
use acid_engine::machine::{MachineType, MachineUid};
use acid_engine::padkit::file::{KIT_EXTENSION, PadKitFile};
use acid_engine::params::MachineParam;

fn kit_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Pad Kit", &[KIT_EXTENSION])
}

impl SidAcidApp {
    pub(crate) fn save_kit_dialog(&mut self, uid: MachineUid) {
        let Some(file) = self
            .doc
            .project
            .machine(uid)
            .and_then(PadKitFile::from_entry)
        else {
            return;
        };
        if let Some(path) = kit_dialog().save_file() {
            let has_it = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(KIT_EXTENSION));
            if has_it {
                self.write_kit(path, file.to_ron());
                return;
            }
            let mut name = path.into_os_string();
            name.push(".");
            name.push(KIT_EXTENSION);
            let path = std::path::PathBuf::from(name);
            if path.exists() {
                // The OS dialog's overwrite prompt was about the name
                // without the extension, so ask about this file.
                self.view.kit_overwrite = Some((path, file.to_ron()));
            } else {
                self.write_kit(path, file.to_ron());
            }
        }
    }

    fn write_kit(&mut self, path: std::path::PathBuf, text: String) {
        if let Err(e) = std::fs::write(&path, text) {
            self.view.message = Some(format!("Could not save the kit: {e}"));
        }
    }

    /// The "Replace file?" question for a kit saved under an appended name.
    pub(crate) fn kit_overwrite_ui(&mut self, ctx: &eframe::egui::Context) {
        let Some((path, _)) = &self.view.kit_overwrite else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut answer = None;
        eframe::egui::Window::new("Replace file?")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("\"{name}\" already exists. Replace it?"));
                ui.horizontal(|ui| {
                    if ui.button("Replace").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if let Some(replace) = answer {
            self.answer_kit_overwrite(replace);
        }
    }

    pub(crate) fn answer_kit_overwrite(&mut self, replace: bool) {
        if let Some((path, text)) = self.view.kit_overwrite.take()
            && replace
        {
            self.write_kit(path, text);
        }
    }

    pub(crate) fn load_kit_dialog(&mut self, uid: MachineUid) {
        if let Some(path) = kit_dialog().pick_file() {
            match std::fs::read_to_string(&path) {
                Ok(text) => self.load_kit_text(uid, &text),
                Err(e) => {
                    self.view.message = Some(format!("Could not load the kit: {e}."));
                }
            }
        }
    }

    /// Parses a kit file's text and loads it once the memory check passes
    /// or the user goes ahead. A bad file leaves the project as it was.
    pub(crate) fn load_kit_text(&mut self, uid: MachineUid, text: &str) {
        let Some(current) = self.doc.project.padkit(uid).map(|k| k.memory_bytes()) else {
            return;
        };
        self.mem = self.sysmem.snapshot();
        // As for a project: i16 PCM in all may not pass half of free memory.
        let budget = self.mem.map(|m| m.available / 2);
        match PadKitFile::from_ron_within(text, budget) {
            Ok(kit) => {
                // The kit, and the undo history's copy of it, less what it replaces.
                let extra = (kit.memory_bytes() as u64 * 2).saturating_sub(current as u64);
                self.with_memory_check(
                    extra,
                    MemAction::LoadKit {
                        uid,
                        kit: Box::new(kit),
                    },
                );
            }
            Err(e) => self.view.message = Some(format!("Could not load the kit: {e}.")),
        }
    }

    /// Replaces every pad and the kit's and pads' param values with the
    /// file's (params it leaves out go back to their defaults).
    pub(crate) fn load_kit_now(&mut self, uid: MachineUid, kit: PadKitFile) {
        if self.doc.project.padkit(uid).is_none() {
            return;
        }
        let values = MachineParam::all_for(MachineType::PadKit)
            .into_iter()
            .filter(|p| matches!(p, MachineParam::PadKit(_) | MachineParam::Pad(..)))
            .map(|p| {
                let id = uid.param(p);
                let v = kit
                    .params
                    .iter()
                    .find(|(q, _)| *q == p)
                    .map_or(id.info().default, |(_, v)| *v);
                (id, v)
            })
            .collect();
        let cmds = self.doc.apply(
            Edit::PadKitKit {
                uid,
                pads: Box::new(kit.pads),
                values,
            },
            false,
        );
        self.send(cmds);
        let panel = self.view.panel(uid);
        panel.pad_layer = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::padkit::pad::Layer;
    use acid_engine::params::PadParam;
    use acid_engine::project::Project;

    fn app_with_kit() -> (SidAcidApp, MachineUid) {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        (app, uid)
    }

    fn kit_text(uid: MachineUid, app: &SidAcidApp) -> String {
        PadKitFile::from_entry(app.doc.project.machine(uid).unwrap())
            .unwrap()
            .to_ron()
    }

    #[test]
    fn loading_a_kit_replaces_pads_and_params_in_one_undo_step() {
        let (mut src, s) = app_with_kit();
        let wav = crate::ui::tests::test_wav(0.1);
        src.load_pad_bytes(s, 3, "snare", Some("wav"), &wav);
        let tune = s.param(MachineParam::Pad(3, PadParam::Tune));
        src.doc.project.set_param(tune, 0.8);
        let text = kit_text(s, &src);

        let (mut app, uid) = app_with_kit();
        app.load_pad_bytes(uid, 0, "old", Some("wav"), &wav);
        let patterns = app.doc.project.note_patterns(uid).unwrap().to_vec();
        let before = app.doc.project.padkit(uid).unwrap().pads.clone();
        app.load_kit_text(uid, &text);
        let k = app.doc.project.padkit(uid).unwrap();
        assert!(k.pads[0].layers.is_empty());
        assert_eq!(k.pads[3].layers.len(), 1);
        let tune = uid.param(MachineParam::Pad(3, PadParam::Tune));
        assert_eq!(app.doc.project.param(tune), 0.8);
        assert_eq!(app.doc.project.note_patterns(uid).unwrap(), &patterns[..]);
        app.doc.undo();
        assert_eq!(app.doc.project.padkit(uid).unwrap().pads, before);
        assert_ne!(app.doc.project.param(tune), 0.8);
    }

    #[test]
    fn a_broken_kit_changes_nothing_and_says_why() {
        let (mut app, uid) = app_with_kit();
        let mut f = PadKitFile::from_entry(app.doc.project.machine(uid).unwrap()).unwrap();
        f.pads[2].layers.push(Layer::new("x", 44_100, 1, &[]));
        let before = app.doc.project.clone();
        app.load_kit_text(uid, &f.to_ron());
        let msg = app.view.message.clone().unwrap();
        assert!(msg.starts_with("Could not load the kit: pad 3: "), "{msg}");
        assert!(msg.ends_with('.'));
        assert_eq!(app.doc.project, before);
        app.load_kit_text(uid, "not a kit");
        assert!(
            app.view
                .message
                .unwrap()
                .starts_with("Could not load the kit: ")
        );
    }

    #[test]
    fn a_kit_saved_over_an_existing_file_asks_first() {
        let (app, uid) = app_with_kit();
        let mut app = app;
        let dir = std::env::temp_dir().join(format!("padkit-ask-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("drums.padkit");
        std::fs::write(&path, b"keep me").unwrap();
        let text = kit_text(uid, &app);
        app.view.kit_overwrite = Some((path.clone(), text));
        app.answer_kit_overwrite(false);
        assert_eq!(std::fs::read(&path).unwrap(), b"keep me");
        app.view.kit_overwrite = Some((path.clone(), kit_text(uid, &app)));
        app.answer_kit_overwrite(true);
        assert!(
            PadKitFile::from_ron_within(&std::fs::read_to_string(&path).unwrap(), None).is_ok()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_replace_question_holds_the_keyboard_and_goes_with_its_document() {
        let (mut app, uid) = app_with_kit();
        app.view.kit_overwrite = Some(("drums.padkit".into(), kit_text(uid, &app)));
        let ctx = eframe::egui::Context::default();
        let key = |pressed| eframe::egui::Event::Key {
            key: eframe::egui::Key::Space,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: eframe::egui::Modifiers::NONE,
        };
        for events in [vec![], vec![key(true), key(false)], vec![]] {
            let input = eframe::egui::RawInput {
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.ui(ctx));
        }
        assert!(
            !app.pending
                .iter()
                .chain(&app.flushed)
                .any(|c| matches!(c, acid_engine::host::Command::Play))
        );
        assert!(app.view.kit_overwrite.is_some(), "still asking");
        app.load_document(Document::new(Project::default()));
        assert!(app.view.kit_overwrite.is_none());
    }
}
