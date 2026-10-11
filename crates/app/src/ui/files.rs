//! New / Open / Save / Save As, loop export, the unsaved-changes guard,
//! pattern copy & paste and the window title.

use super::SidAcidApp;
use crate::document::{Document, Edit};
use acid_engine::project::{BANK_SIZE, DrumPattern, PatternRef, Project, SynthPattern};
use eframe::egui;
use std::sync::mpsc;

const EXTENSION: &str = "sidacid";
const DEFAULT_EXPORT_BARS: u32 = 8;

/// An action waiting on the unsaved-changes question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    New,
    Open,
    Quit,
}

pub enum Clip {
    Synth(SynthPattern),
    Drum(DrumPattern),
}

fn project_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("SidAcid project", &[EXTENSION])
}

impl SidAcidApp {
    pub(crate) fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            if ui.button("New").clicked() {
                ui.close_menu();
                self.request(Pending::New);
            }
            if ui.button("Open…").clicked() {
                ui.close_menu();
                self.request(Pending::Open);
            }
            if ui.button("Save").clicked() {
                ui.close_menu();
                self.save(false);
            }
            if ui.button("Save As…").clicked() {
                ui.close_menu();
                self.save(true);
            }
            ui.separator();
            if ui.button("Export loop to WAV…").clicked() {
                ui.close_menu();
                if self.view.export_bars == 0 {
                    self.view.export_bars = DEFAULT_EXPORT_BARS;
                }
                self.view.export_open = true;
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                ui.close_menu();
                self.request(Pending::Quit);
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui
                .add_enabled(self.doc.can_undo(), egui::Button::new("Undo  (Ctrl+Z)"))
                .clicked()
            {
                ui.close_menu();
                let cmds = self.doc.undo();
                self.send(cmds);
            }
            if ui
                .add_enabled(self.doc.can_redo(), egui::Button::new("Redo  (Ctrl+Y)"))
                .clicked()
            {
                ui.close_menu();
                let cmds = self.doc.redo();
                self.send(cmds);
            }
            ui.separator();
            if ui.button("Copy pattern  (Ctrl+C)").clicked() {
                ui.close_menu();
                self.copy_pattern();
            }
            if ui.button("Paste pattern  (Ctrl+V)").clicked() {
                ui.close_menu();
                self.paste_pattern();
            }
        });
    }

    /// Runs `action` now, or asks first if there are unsaved changes.
    pub(crate) fn request(&mut self, action: Pending) {
        if self.doc.dirty {
            self.view.confirm = Some(action);
        } else {
            self.perform(action);
        }
    }

    pub(crate) fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => self.load_document(Document::new(Project::default())),
            Pending::Open => {
                if let Some(path) = project_dialog().pick_file() {
                    match Project::load(&path) {
                        Ok(project) => {
                            let mut doc = Document::new(project);
                            doc.path = Some(path);
                            self.load_document(doc);
                        }
                        Err(e) => {
                            self.view.message =
                                Some(format!("Could not open {}:\n{e}", path.display()))
                        }
                    }
                }
            }
            Pending::Quit => self.view.quit = true,
        }
    }

    pub(crate) fn load_document(&mut self, doc: Document) {
        self.doc = doc;
        self.view.writers = [None; 2];
        self.view.roll_drag = [None; 2];
        self.view.scroll_accum = [0.0; 2];
        self.view.scrolling = [false; 2];
        self.view.touched_knob = None;
        for m in 0..4 {
            self.view.bank[m] = self.doc.project.selected[m] / BANK_SIZE as u8;
        }
        self.pending.clear();
        if let Some(audio) = self.audio.as_mut() {
            // Stale events (disposals, old status) belong to the old engine.
            while audio.poll().is_some() {}
        }
        if let Some(audio) = self.audio.as_mut()
            && audio.load_project(&self.doc.project).is_err()
        {
            self.view.message =
                Some("The audio engine is busy; the project will play after Retry audio.".into());
            self.audio = None;
            self.audio_error = Some("engine busy while loading".into());
        }
    }

    /// Saves to the current path (or asks for one). Returns true on success.
    pub(crate) fn save(&mut self, save_as: bool) -> bool {
        let path = if save_as || self.doc.path.is_none() {
            project_dialog()
                .set_file_name(format!("{}.{EXTENSION}", self.doc.project.name))
                .save_file()
        } else {
            self.doc.path.clone()
        };
        let Some(mut path) = path else { return false };
        if path.extension().is_none() {
            path.set_extension(EXTENSION);
        }
        match self.doc.project.save(&path) {
            Ok(()) => {
                self.doc.path = Some(path);
                self.doc.dirty = false;
                true
            }
            Err(e) => {
                self.view.message = Some(format!("Could not save {}:\n{e}", path.display()));
                false
            }
        }
    }

    /// "Don't save": drop the question and run the action. The document stays
    /// dirty until the action actually replaces it.
    pub(crate) fn dont_save(&mut self, action: Pending) {
        self.view.confirm = None;
        self.perform(action);
    }

    pub(crate) fn copy_pattern(&mut self) {
        let m = self.view.focus;
        let slot = self.doc.project.selected[m] as usize;
        self.view.clip_text = Some(format!(
            "SidAcid pattern {}",
            PatternRef(slot as u8).label()
        ));
        self.view.clipboard = Some(if m < 2 {
            Clip::Synth(self.doc.project.synths[m].patterns[slot].clone())
        } else {
            Clip::Drum(self.doc.project.drums[m - 2].patterns[slot].clone())
        });
    }

    pub(crate) fn paste_pattern(&mut self) {
        let m = self.view.focus;
        let slot = self.doc.project.selected[m];
        let edit = match (&self.view.clipboard, m < 2) {
            (Some(Clip::Synth(p)), true) => Edit::SynthPattern {
                synth: m,
                slot,
                pattern: Box::new(p.clone()),
            },
            (Some(Clip::Drum(p)), false) => Edit::DrumPattern {
                kit: m - 2,
                slot,
                pattern: Box::new(p.clone()),
            },
            (None, _) => return,
            _ => {
                self.view.message = Some("The clipboard holds a different kind of pattern.".into());
                return;
            }
        };
        let cmds = self.doc.apply(edit, false);
        self.send(cmds);
    }

    pub(crate) fn window_title(&self) -> String {
        let name = self
            .doc
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.doc.project.name.clone());
        format!("{name}{} — SidAcid", if self.doc.dirty { "*" } else { "" })
    }

    pub(crate) fn start_export(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("WAV audio", &["wav"])
            .set_file_name(format!("{}-loop.wav", self.doc.project.name))
            .save_file()
        else {
            return;
        };
        let project = self.doc.project.clone();
        let bars = self.view.export_bars.clamp(1, 64);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = acid_engine::render::render_patterns_wav(&project, bars, &path)
                .map(|()| path)
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
        self.view.export_rx = Some(rx);
        self.view.status_line = Some(format!("Exporting {bars} bars…"));
    }

    pub(crate) fn poll_export(&mut self) {
        let Some(rx) = &self.view.export_rx else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            self.view.export_rx = None;
            self.view.status_line = None;
            self.view.message = Some(match result {
                Ok(path) => format!("Exported {}", path.display()),
                Err(e) => format!("Export failed: {e}"),
            });
        }
    }

    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.view.confirm {
            let name = self
                .window_title()
                .trim_end_matches(" — SidAcid")
                .trim_end_matches('*')
                .to_string();
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Save changes to \"{name}\" first?"));
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() && self.save(false) {
                            self.view.confirm = None;
                            self.perform(action);
                        }
                        if ui.button("Don't save").clicked() {
                            self.dont_save(action);
                        }
                        if ui.button("Cancel").clicked() {
                            self.view.confirm = None;
                        }
                    });
                });
        }
        if self.view.export_open {
            let mut open = true;
            egui::Window::new("Export loop")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Renders the selected pattern of every machine.");
                    ui.add(
                        egui::DragValue::new(&mut self.view.export_bars)
                            .range(1..=64)
                            .suffix(" bars"),
                    );
                    if ui.button("Export…").clicked() {
                        self.view.export_open = false;
                        self.start_export();
                    }
                });
            if !open {
                self.view.export_open = false;
            }
        }
        if let Some(msg) = self.view.message.clone() {
            egui::Window::new("SidAcid")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(msg);
                    if ui.button("OK").clicked() {
                        self.view.message = None;
                    }
                });
        }
    }
}
