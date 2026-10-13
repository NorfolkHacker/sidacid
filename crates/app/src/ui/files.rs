//! New / Open / Save / Save As, loop export, the unsaved-changes guard,
//! pattern copy & paste and the window title.

use super::SidAcidApp;
use crate::document::{Document, Edit};
use acid_engine::host::Command;
use acid_engine::machine::{MachineKind, kind_of};
use acid_engine::project::{
    BANK_SIZE, DrumPattern, NotePattern, PatternRef, Project, SynthPattern,
};
use eframe::egui;
use std::path::PathBuf;
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
    Notes(NotePattern),
}

fn project_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("SidAcid project", &[EXTENSION])
}

/// `path` ending in `.sidacid`: appended unless the name already ends with
/// it (any case), so "beat.v2" saves as "beat.v2.sidacid". The OS dialog's
/// overwrite prompt never saw the appended name, so saving asks itself
/// before replacing such a file (`OverwriteAsk`).
pub fn with_project_extension(path: PathBuf) -> PathBuf {
    let has_it = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION));
    let Some(name) = path.file_name().filter(|_| !has_it) else {
        return path;
    };
    let mut name = name.to_os_string();
    name.push(".");
    name.push(EXTENSION);
    path.with_file_name(name)
}

/// A save waiting for its "Saving…" status to reach the screen.
pub struct PendingSave {
    path: PathBuf,
    /// Action to run once the save succeeds.
    then: Option<Pending>,
    /// Frames finished since the save was asked for.
    frames: u8,
    /// Status line to restore afterwards.
    prev_status: Option<String>,
}

/// A save that would replace an existing file the OS dialog never asked
/// about: the name picked had no `.sidacid`, so the save appends it.
pub struct OverwriteAsk {
    path: PathBuf,
    /// Action to run once the save succeeds.
    then: Option<Pending>,
}

pub fn export_status(song: bool, bars: u32, progress: Option<f32>) -> String {
    match (song, progress) {
        (true, Some(p)) => format!("Exporting song {:.0}%…", (p * 100.0).floor()),
        (true, None) => "Exporting song…".to_string(),
        (false, _) => format!("Exporting {bars} bars…"),
    }
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
            if ui
                .add_enabled(
                    self.view.export_rx.is_none(),
                    egui::Button::new("Export to WAV…"),
                )
                .clicked()
            {
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
                self.undo();
            }
            if ui
                .add_enabled(self.doc.can_redo(), egui::Button::new("Redo  (Ctrl+Y)"))
                .clicked()
            {
                ui.close_menu();
                self.redo();
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
        if self.doc.is_dirty() {
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

    /// Releases the held pad and roll-audition notes, so a note that is
    /// still sounding on an engine that outlives the load stops.
    pub(crate) fn release_held_notes(&mut self) {
        let mut cmds = Vec::new();
        if let Some(pitch) = self.view.pad_note.take() {
            cmds.push(Command::NoteOff {
                machine: acid_engine::machine::MachineId::Sampler.index() as u8,
                pitch,
            });
        }
        if let Some((machine, pitch)) = self.view.audition.take() {
            cmds.push(Command::NoteOff {
                machine: machine as u8,
                pitch,
            });
        }
        self.send(cmds);
    }

    pub(crate) fn load_document(&mut self, doc: Document) {
        // Ahead of the clear below: these go to the old engine first.
        self.release_held_notes();
        self.flush_pending();
        // A save queued for the old document runs now: it was asked for
        // (Ctrl+S, then New and "Don't save" within its two frames). Any
        // action it was to run next is overtaken by this load.
        if let Some(save) = self.view.saving.take() {
            self.view.status_line = save.prev_status;
            self.write_project(save.path);
        }
        self.doc = doc;
        // An empty imported bank is no bank (the Imported tab stays off).
        if self.doc.project.fm.bank.as_ref().is_some_and(Vec::is_empty) {
            self.doc.project.fm.bank = None;
        }
        self.view.overwrite = None;
        self.view.writers = [None; 2];
        self.view.roll_drag = [None; 2];
        self.view.roll_origin = [None, None];
        self.view.scroll_accum = [0.0; 2];
        self.view.scroll_at = [None; 2];
        self.view.knob_scroll = None;
        self.view.poly_drag = [None; 2];
        self.view.poly_scrolled = [false; 2];
        self.view.audition = None;
        self.view.fm_op = 0;
        self.view.sampler_slot = 0;
        self.view.wave_cache.clear();
        self.view.wave_drag = None;
        self.view.pad_note = None;
        self.view.fm_browser = Default::default();
        self.view.fm_voice_src = None;
        self.reset_transport_view();
        self.view.song_cursor = 0;
        self.view.song_sel_end = None;
        for m in 0..acid_engine::machine::MACHINES {
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

    /// Saves to the current path (or asks for one). Returns true when a
    /// save was queued: it runs a frame later, once "Saving…" shows.
    pub(crate) fn save(&mut self, save_as: bool) -> bool {
        self.queue_save(save_as, None)
    }

    /// The unsaved-changes question's "Save": save, then run `action`.
    pub(crate) fn save_then(&mut self, action: Pending) -> bool {
        let queued = self.queue_save(false, Some(action));
        if queued {
            self.view.confirm = None;
        }
        queued
    }

    /// Returns true when the save was queued, or is waiting on the
    /// overwrite question (which then owns `then`).
    fn queue_save(&mut self, save_as: bool, then: Option<Pending>) -> bool {
        if self.view.saving.is_some() || self.view.overwrite.is_some() {
            return false;
        }
        let path = if save_as || self.doc.path.is_none() {
            project_dialog()
                .set_file_name(format!("{}.{EXTENSION}", self.doc.project.name))
                .save_file()
        } else {
            self.doc.path.clone()
        };
        let Some(chosen) = path else { return false };
        let path = with_project_extension(chosen.clone());
        if path != chosen && path.exists() {
            // Any OS overwrite prompt was about `chosen`, not this file.
            self.view.overwrite = Some(OverwriteAsk { path, then });
        } else {
            self.queue_save_to(path, then);
        }
        true
    }

    fn queue_save_to(&mut self, path: PathBuf, then: Option<Pending>) {
        let prev_status = self.view.status_line.replace("Saving…".into());
        self.view.saving = Some(PendingSave {
            path,
            then,
            frames: 0,
            prev_status,
        });
    }

    /// The overwrite question's answer: save over the file, or give up (an
    /// unsaved-changes question that led here is asked again).
    pub(crate) fn answer_overwrite(&mut self, replace: bool) {
        let Some(ask) = self.view.overwrite.take() else {
            return;
        };
        if replace {
            self.queue_save_to(ask.path, ask.then);
        } else {
            self.view.confirm = ask.then;
        }
    }

    /// Start of a frame: runs a queued save once a finished frame has
    /// shown its status. Saving blocks the UI thread for a moment.
    pub(crate) fn run_queued_save(&mut self) {
        if self.view.saving.as_ref().is_none_or(|s| s.frames < 2) {
            return;
        }
        let Some(save) = self.view.saving.take() else {
            return;
        };
        self.view.status_line = save.prev_status;
        let saved = self.write_project(save.path);
        match save.then {
            Some(action) if saved => self.perform(action),
            // A failed save puts the unsaved-changes question back.
            Some(action) => self.view.confirm = Some(action),
            None => {}
        }
    }

    /// End of a frame: counts it toward a queued save and asks for the next.
    pub(crate) fn tick_queued_save(&mut self, ctx: &egui::Context) {
        if let Some(save) = self.view.saving.as_mut() {
            save.frames = save.frames.saturating_add(1);
            ctx.request_repaint();
        }
    }

    fn write_project(&mut self, path: PathBuf) -> bool {
        match self.doc.project.save(&path) {
            Ok(()) => {
                self.doc.path = Some(path);
                self.doc.mark_saved();
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
        self.view.clipboard = match kind_of(m) {
            Some(MachineKind::Synth(s)) => Some(Clip::Synth(
                self.doc.project.synths[s].patterns[slot].clone(),
            )),
            Some(MachineKind::Drum(k)) => {
                Some(Clip::Drum(self.doc.project.drums[k].patterns[slot].clone()))
            }
            Some(MachineKind::Notes(_)) => self
                .doc
                .project
                .note_patterns(m)
                .map(|p| Clip::Notes(p[slot].clone())),
            None => None,
        };
    }

    pub(crate) fn paste_pattern(&mut self) {
        let m = self.view.focus;
        let slot = self.doc.project.selected[m];
        let edit = match (&self.view.clipboard, kind_of(m)) {
            (None, _) => return,
            (Some(Clip::Synth(p)), Some(MachineKind::Synth(s))) => Edit::SynthPattern {
                synth: s,
                slot,
                pattern: Box::new(p.clone()),
            },
            (Some(Clip::Drum(p)), Some(MachineKind::Drum(k))) => Edit::DrumPattern {
                kit: k,
                slot,
                pattern: Box::new(p.clone()),
            },
            (Some(Clip::Notes(p)), Some(MachineKind::Notes(_))) => Edit::NotePattern {
                machine: m,
                slot,
                pattern: Box::new(p.clone()),
            },
            _ => {
                self.view.message = Some("The clipboard holds a different kind of pattern.".into());
                return;
            }
        };
        let cmds = self.doc.apply(edit, false);
        self.send(cmds);
    }

    /// The file's name without extension, or the project name if unsaved.
    pub(crate) fn document_name(&self) -> String {
        self.doc
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.doc.project.name.clone())
    }

    pub(crate) fn window_title(&self) -> String {
        let name = self.document_name();
        format!(
            "{name}{} — SidAcid",
            if self.doc.is_dirty() { "*" } else { "" }
        )
    }

    pub(crate) fn start_export(&mut self) {
        // One export at a time: a second would drop the first one's result.
        if self.view.export_rx.is_some() {
            self.view.message = Some("An export is already running.".into());
            return;
        }
        let song = self.view.export_song;
        let kind = if song { "song" } else { "loop" };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("WAV audio", &["wav"])
            .set_file_name(format!("{}-{kind}.wav", self.doc.project.name))
            .save_file()
        else {
            return;
        };
        let project = self.doc.project.clone();
        let bars = self.view.export_bars.clamp(1, 64);
        let (tx, rx) = mpsc::channel();
        if song {
            let progress = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            let shared = progress.clone();
            std::thread::spawn(move || {
                let result = acid_engine::render::render_song_wav(&project, &path, |f| {
                    shared.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed);
                })
                .map(|()| path)
                .map_err(|e| e.to_string());
                let _ = tx.send(result);
            });
            self.view.export_progress = Some(progress);
            *self.status_line_mut() = Some(export_status(true, 0, Some(0.0)));
        } else {
            std::thread::spawn(move || {
                let result = acid_engine::render::render_patterns_wav(&project, bars, &path)
                    .map(|()| path)
                    .map_err(|e| e.to_string());
                let _ = tx.send(result);
            });
            *self.status_line_mut() = Some(export_status(false, bars, None));
        }
        self.view.export_rx = Some(rx);
    }

    /// The status line to write: while a save is queued its "Saving…"
    /// stays up, and the text goes to the line it restores afterwards.
    pub(crate) fn status_line_mut(&mut self) -> &mut Option<String> {
        match self.view.saving.as_mut() {
            Some(save) => &mut save.prev_status,
            None => &mut self.view.status_line,
        }
    }

    pub(crate) fn poll_export(&mut self) {
        if self.view.export_rx.is_none() {
            return;
        }
        if let Some(p) = &self.view.export_progress {
            let f = f32::from_bits(p.load(std::sync::atomic::Ordering::Relaxed));
            *self.status_line_mut() = Some(export_status(true, 0, Some(f)));
        }
        let Some(rx) = &self.view.export_rx else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            self.view.export_rx = None;
            self.view.export_progress = None;
            *self.status_line_mut() = None;
            self.view.message = Some(match result {
                Ok(path) => format!("Exported {}", path.display()),
                Err(e) => format!("Export failed: {e}"),
            });
        }
    }

    pub(crate) fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.view.confirm {
            let name = self.document_name();
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Save changes to \"{name}\" first?"));
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.save_then(action);
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
        if let Some(ask) = &self.view.overwrite {
            let name = ask
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            egui::Window::new("Replace file?")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("\"{name}\" already exists. Replace it?"));
                    ui.horizontal(|ui| {
                        if ui.button("Replace").clicked() {
                            self.answer_overwrite(true);
                        }
                        if ui.button("Cancel").clicked() {
                            self.answer_overwrite(false);
                        }
                    });
                });
        }
        if self.view.export_open {
            let mut open = true;
            egui::Window::new("Export to WAV")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    let has_song = !self.doc.project.song.bars.is_empty();
                    if !has_song {
                        self.view.export_song = false;
                    }
                    ui.radio_value(
                        &mut self.view.export_song,
                        false,
                        "Loop of the selected patterns",
                    );
                    ui.add_enabled_ui(has_song, |ui| {
                        ui.radio_value(&mut self.view.export_song, true, "Whole song");
                    });
                    if !self.view.export_song {
                        ui.add(
                            egui::DragValue::new(&mut self.view.export_bars)
                                .range(1..=64)
                                .suffix(" bars"),
                        );
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_status_text() {
        assert_eq!(export_status(false, 8, None), "Exporting 8 bars…");
        assert_eq!(export_status(true, 0, Some(0.425)), "Exporting song 42%…");
        assert_eq!(export_status(true, 0, None), "Exporting song…");
    }

    use crate::document::Edit;
    use crate::ui::run_frames;
    use std::path::Path;

    fn app() -> SidAcidApp {
        SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()))
    }

    /// A fresh path in the temp directory, removed when dropped.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "sidacid-test-{}-{name}.{EXTENSION}",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn the_project_extension_is_appended_unless_present() {
        let ext = |p: &str| with_project_extension(PathBuf::from(p));
        assert_eq!(ext("/x/a"), PathBuf::from("/x/a.sidacid"));
        assert_eq!(ext("/x/a.v2"), PathBuf::from("/x/a.v2.sidacid"));
        assert_eq!(ext("/x/a.SIDACID"), PathBuf::from("/x/a.SIDACID"));
        assert_eq!(ext("/x/a.sidacid"), PathBuf::from("/x/a.sidacid"));
    }

    #[test]
    fn the_document_name_keeps_a_trailing_star() {
        let mut app = app();
        app.doc.project.name = "Beat*".into();
        app.doc.mark_dirty();
        assert_eq!(app.document_name(), "Beat*");
        app.doc.path = Some(PathBuf::from("/tmp/loop*.sidacid"));
        assert_eq!(app.document_name(), "loop*");
    }

    #[test]
    fn a_second_export_is_refused_while_one_runs() {
        let mut app = app();
        let (tx, rx) = mpsc::channel();
        app.view.export_rx = Some(rx);
        app.start_export();
        assert_eq!(
            app.view.message.as_deref(),
            Some("An export is already running.")
        );
        tx.send(Ok(PathBuf::from("first.wav"))).unwrap();
        let first = app.view.export_rx.as_ref().unwrap().try_recv();
        assert_eq!(first, Ok(Ok(PathBuf::from("first.wav"))), "same channel");
    }

    #[test]
    fn save_and_reload_round_trip() {
        let file = TempFile::new("round-trip");
        let mut app = app();
        app.doc.path = Some(file.0.clone());
        let cmds = app.doc.apply(Edit::Bpm(151.0), false);
        app.send(cmds);
        assert!(app.save(false));
        assert!(!file.0.exists(), "the save waits for the status to show");
        assert_eq!(app.view.status_line.as_deref(), Some("Saving…"));
        run_frames(&mut app, 3);
        assert!(file.0.exists());
        assert!(!app.doc.is_dirty());
        assert_eq!(app.view.status_line, None);
        let saved = app.doc.project.clone();
        let loaded = Project::load(&file.0).unwrap();
        assert_eq!(loaded, saved);
        let mut doc = Document::new(loaded);
        doc.path = Some(file.0.clone());
        app.load_document(doc);
        assert_eq!(app.doc.project, saved);
        assert!(!app.doc.is_dirty());
    }

    #[test]
    fn a_failed_save_reports_and_stays_dirty() {
        let mut app = app();
        app.doc.path = Some(PathBuf::from("/nonexistent-sidacid-dir/x.sidacid"));
        app.doc.mark_dirty();
        assert!(app.save(false));
        run_frames(&mut app, 3);
        assert!(
            app.view
                .message
                .as_deref()
                .is_some_and(|m| m.starts_with("Could not save"))
        );
        assert!(app.doc.is_dirty());
        assert!(!Path::new("/nonexistent-sidacid-dir").exists());
    }

    #[test]
    fn save_from_the_unsaved_changes_question_then_runs_the_action() {
        let file = TempFile::new("then-new");
        let mut app = app();
        app.doc.path = Some(file.0.clone());
        app.doc.mark_dirty();
        app.request(Pending::New);
        assert_eq!(app.view.confirm, Some(Pending::New));
        assert!(app.save_then(Pending::New));
        assert!(app.view.confirm.is_none());
        assert_eq!(app.doc.project.name, "Demo", "not before the save");
        run_frames(&mut app, 3);
        assert!(file.0.exists());
        assert_eq!(app.doc.project.name, "Untitled");
    }

    #[test]
    fn a_failed_save_from_the_question_asks_again() {
        let mut app = app();
        app.doc.path = Some(PathBuf::from("/nonexistent-sidacid-dir/x.sidacid"));
        app.doc.mark_dirty();
        app.view.confirm = Some(Pending::Quit);
        assert!(app.save_then(Pending::Quit));
        run_frames(&mut app, 3);
        assert!(app.view.message.is_some());
        assert_eq!(app.view.confirm, Some(Pending::Quit));
        assert!(!app.view.quit);
    }

    /// A document path without the extension, as if picked in the OS save
    /// dialog as "<name>", and the existing "<name>.sidacid" next to it.
    fn app_saving_over_an_existing_file(name: &str) -> (SidAcidApp, TempFile) {
        let existing = TempFile::new(name);
        std::fs::write(&existing.0, b"keep me").unwrap();
        let mut app = app();
        app.doc.path = Some(existing.0.with_extension(""));
        app.doc.mark_dirty();
        (app, existing)
    }

    #[test]
    fn saving_over_an_existing_file_by_appending_the_extension_asks_first() {
        let (mut app, existing) = app_saving_over_an_existing_file("overwrite-ask");
        assert!(app.save(false));
        run_frames(&mut app, 3);
        assert!(std::fs::read(&existing.0).unwrap() == b"keep me", "not yet");
        assert!(app.view.overwrite.is_some(), "the question is up");
        assert!(!app.save(false), "one question at a time");
        app.answer_overwrite(true);
        assert!(app.view.overwrite.is_none());
        run_frames(&mut app, 3);
        assert!(Project::load(&existing.0).is_ok(), "replaced");
        assert_eq!(app.doc.path.as_ref(), Some(&existing.0));
        assert!(!app.doc.is_dirty());
    }

    #[test]
    fn cancelling_the_overwrite_question_keeps_the_file() {
        let (mut app, existing) = app_saving_over_an_existing_file("overwrite-cancel");
        app.view.confirm = Some(Pending::New);
        assert!(app.save_then(Pending::New));
        assert!(
            app.view.confirm.is_none(),
            "the overwrite question replaces it"
        );
        app.answer_overwrite(false);
        run_frames(&mut app, 3);
        assert!(std::fs::read(&existing.0).unwrap() == b"keep me", "kept");
        assert!(app.doc.is_dirty());
        assert_eq!(app.view.confirm, Some(Pending::New), "asked again");
        assert_eq!(app.doc.project.name, "Demo", "New did not run");
    }

    #[test]
    fn saving_to_a_path_that_already_has_the_extension_does_not_ask() {
        let file = TempFile::new("no-ask");
        std::fs::write(&file.0, b"old").unwrap();
        let mut app = app();
        app.doc.path = Some(file.0.clone());
        assert!(app.save(false));
        assert!(app.view.overwrite.is_none());
        run_frames(&mut app, 3);
        assert!(Project::load(&file.0).is_ok());
    }

    #[test]
    fn a_save_queued_before_a_document_load_still_runs() {
        let file = TempFile::new("queued-then-load");
        let mut app = app();
        app.doc.path = Some(file.0.clone());
        app.doc.mark_dirty();
        assert!(app.save(false));
        // Within the save's two frames: New, "Don't save".
        app.load_document(Document::new(Project::default()));
        assert!(file.0.exists(), "the asked-for save is not dropped");
        assert_eq!(Project::load(&file.0).unwrap().name, "Demo");
        assert_eq!(app.view.status_line, None);
        run_frames(&mut app, 3);
        assert_eq!(app.doc.project.name, "Untitled");
    }

    #[test]
    fn an_export_started_while_a_save_is_queued_keeps_its_status() {
        let file = TempFile::new("save-then-export");
        let mut app = app();
        app.doc.path = Some(file.0.clone());
        assert!(app.save(false));
        *app.status_line_mut() = Some(export_status(false, 8, None));
        assert_eq!(app.view.status_line.as_deref(), Some("Saving…"));
        run_frames(&mut app, 3);
        assert!(file.0.exists());
        assert_eq!(app.view.status_line.as_deref(), Some("Exporting 8 bars…"));
    }

    #[test]
    fn a_one_voice_bank_draws_and_loads() {
        use acid_engine::fm::sysex::Dx7Voice;
        let voice: Dx7Voice = crate::ui::fm_voices::starter()[3].clone();
        let mut project = Project::default();
        project.fm.bank = Some(vec![voice.clone()]);
        let mut app = SidAcidApp::headless(Document::new(project));
        app.view.fm_browser = crate::ui::fm_voices::FmBrowser::Imported;
        run_frames(&mut app, 2);
        app.load_fm_voice(&app.doc.project.fm.bank.as_ref().unwrap()[0].clone());
        assert_eq!(app.doc.project.fm.voice_name, voice.name);
    }

    #[test]
    fn an_empty_bank_loads_as_no_bank() {
        let mut project = Project::default();
        project.fm.bank = Some(Vec::new());
        let mut app = app();
        app.load_document(Document::new(project));
        assert_eq!(app.doc.project.fm.bank, None);
        assert!(!app.doc.is_dirty());
        run_frames(&mut app, 1);
    }
}
