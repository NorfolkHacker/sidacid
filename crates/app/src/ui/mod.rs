//! The SidAcid window: transport bar on top, a scrolling rack of machine
//! panels below. The UI owns the project (`Document`); every change goes
//! to the audio thread as `Command`s through `AudioLink`.

mod drum_panel;
mod files;
mod keys;
pub mod knob;
mod mixer_panel;
mod pads;
mod params_ui;
mod pianoroll;
mod roll_edit;
mod step303;
mod step_edit;
mod synth_panel;
pub mod theme;
mod transport;

use crate::audio::AudioLink;
use crate::document::Document;
use acid_engine::host::{Command, Event, Status};
use eframe::egui;
use std::time::Duration;

#[derive(Default)]
pub struct ViewState {
    /// Machine that keyboard shortcuts act on (0..4).
    pub focus: usize,
    pub message: Option<String>,
    /// Bank shown on each machine's pads (0..8).
    pub bank: [u8; 4],
    /// Active 303 step-writer per synth.
    pub writers: [Option<step_edit::StepWriter>; 2],
    /// Scroll distance accumulated per synth toward the next pitch notch.
    pub scroll_accum: [f32; 2],
    /// Whether a note cell of each synth was scrolled on the previous frame.
    pub scrolling: [bool; 2],
    /// Mouse drag in progress in each synth's piano roll.
    pub roll_drag: [Option<roll_edit::RollDrag>; 2],
    /// Egui time until which the "audio fault recovered" label shows.
    pub fault_until: f64,
    /// Action waiting on the unsaved-changes question.
    pub confirm: Option<files::Pending>,
    pub quit: bool,
    pub clipboard: Option<files::Clip>,
    pub export_open: bool,
    pub export_bars: u32,
    pub export_rx: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    pub last_title: String,
    /// Text to place on the OS clipboard so Ctrl+V reaches the app.
    pub clip_text: Option<String>,
}

pub struct SidAcidApp {
    pub(crate) doc: Document,
    pub(crate) audio: Option<AudioLink>,
    pub(crate) audio_error: Option<String>,
    pub(crate) status: Status,
    pub(crate) pending: Vec<Command>,
    pub(crate) view: ViewState,
}

impl SidAcidApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self::headless(Document::new(acid_engine::demo::demo_project()));
        app.start_audio();
        app
    }

    /// An app with no audio device (tests, or before audio starts).
    pub fn headless(doc: Document) -> Self {
        Self {
            doc,
            audio: None,
            audio_error: None,
            status: Status::default(),
            pending: Vec::new(),
            view: ViewState::default(),
        }
    }

    pub(crate) fn start_audio(&mut self) {
        self.pending.clear();
        match AudioLink::start(&self.doc.project) {
            Ok(link) => {
                self.audio = Some(link);
                self.audio_error = None;
            }
            Err(e) => {
                self.audio = None;
                self.audio_error = Some(e);
            }
        }
    }

    /// Queues commands for the audio thread. Repeated values for the same
    /// parameter collapse into the latest one.
    pub(crate) fn send(&mut self, cmds: Vec<Command>) {
        for cmd in cmds {
            if let &Command::SetParam(id, v) = &cmd
                && let Some(Command::SetParam(_, pending)) = self
                    .pending
                    .iter_mut()
                    .find(|c| matches!(c, Command::SetParam(p, _) if *p == id))
            {
                *pending = v;
                continue;
            }
            self.pending.push(cmd);
        }
    }

    /// Sends queued commands and reads events. Without audio, commands are
    /// dropped: the engine is rebuilt from the project when audio starts.
    fn pump(&mut self, now: f64) {
        let Some(audio) = self.audio.as_mut() else {
            self.pending.clear();
            return;
        };
        let mut unsent = Vec::new();
        for cmd in self.pending.drain(..) {
            if !unsent.is_empty() {
                unsent.push(cmd);
            } else if let Err(cmd) = audio.send(cmd) {
                unsent.push(cmd);
            }
        }
        self.pending = unsent;
        while let Some(event) = audio.poll() {
            if let Event::Status(status) = event {
                if status.guard_fired {
                    self.view.fault_until = now + 3.0;
                }
                self.status = status;
            }
            // Dispose events drop here, on the UI thread.
        }
        if audio.has_failed() {
            self.audio = None;
            self.status = Status::default();
            self.audio_error = Some("The audio device stopped.".into());
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        self.pump(ctx.input(|i| i.time));
        self.handle_step_write(ctx);
        self.handle_shortcuts(ctx);
        if let Some(t) = self.view.clip_text.take() {
            ctx.copy_text(t);
        }
        self.poll_export();
        if ctx.input(|i| i.viewport().close_requested()) && self.doc.dirty && !self.view.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.view.confirm = Some(files::Pending::Quit);
        }
        if self.view.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let title = self.window_title();
        if title != self.view.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.view.last_title = title;
        }
        egui::TopBottomPanel::top("transport").show(ctx, |ui| self.transport(ui));
        if let Some(err) = self.audio_error.clone() {
            egui::TopBottomPanel::top("audio_banner").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(theme::ACCENT, format!("No audio: {err}"));
                    if ui.button("Retry audio").clicked() {
                        self.start_audio();
                    }
                });
            });
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| self.rack(ui));
        });
        self.dialogs(ctx);
        if self.status.playing {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn rack(&mut self, ui: &mut egui::Ui) {
        for synth in 0..2 {
            self.synth_panel(ui, synth);
            ui.add_space(6.0);
        }
        for kit in 0..2 {
            self.drum_panel(ui, kit);
            ui.add_space(6.0);
        }
        self.mixer_panel(ui);
    }
}

impl eframe::App for SidAcidApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}

#[cfg(test)]
pub(crate) fn run_frames(app: &mut SidAcidApp, frames: usize) {
    let ctx = egui::Context::default();
    for _ in 0..frames {
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piano_roll_view_draws_headless() {
        let mut doc = Document::new(acid_engine::demo::demo_project());
        doc.project.synths[0].view = acid_engine::project::SeqView::PianoRoll;
        let mut app = SidAcidApp::headless(doc);
        run_frames(&mut app, 2);
    }

    #[test]
    fn ui_draws_headless() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        run_frames(&mut app, 3);
        assert!(app.audio.is_none());
    }

    #[test]
    fn commands_without_audio_are_dropped_not_queued_forever() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.send(vec![Command::Play, Command::Stop]);
        run_frames(&mut app, 1);
        assert!(app.pending.is_empty());
    }

    #[test]
    fn repeated_param_commands_coalesce() {
        use acid_engine::params::{ParamId, SynthParam};
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        app.send(vec![
            Command::SetParam(id, 0.1),
            Command::SetParam(id, 0.2),
            Command::SetParam(id, 0.3),
        ]);
        assert_eq!(app.pending.len(), 1);
        assert!(matches!(app.pending[0], Command::SetParam(_, v) if v == 0.3));
    }

    #[test]
    fn copy_paste_between_synths_is_undoable() {
        use acid_engine::project::Gate;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.focus = 0;
        app.copy_pattern();
        app.view.focus = 1;
        app.doc.project.selected[1] = 9;
        app.paste_pattern();
        assert_eq!(
            app.doc.project.synths[1].patterns[9],
            app.doc.project.synths[0].patterns[0]
        );
        let cmds = app.doc.undo();
        app.send(cmds);
        assert!(
            app.doc.project.synths[1].patterns[9]
                .steps
                .iter()
                .all(|s| s.gate == Gate::Off)
        );
    }

    #[test]
    fn pasting_drums_onto_a_synth_is_refused() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.focus = 2;
        app.copy_pattern();
        app.view.focus = 0;
        let before = app.doc.project.clone();
        app.paste_pattern();
        assert_eq!(app.doc.project, before);
        assert!(app.view.message.is_some());
    }

    #[test]
    fn title_shows_name_and_dirty_mark() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        assert_eq!(app.window_title(), "Demo — SidAcid");
        app.doc.dirty = true;
        app.doc.path = Some(std::path::PathBuf::from("/tmp/acid line.sidacid"));
        assert_eq!(app.window_title(), "acid line* — SidAcid");
    }

    #[test]
    fn new_with_unsaved_changes_asks_first() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.doc.dirty = true;
        app.request(files::Pending::New);
        assert!(app.view.confirm.is_some());
        assert_eq!(app.doc.project.name, "Demo");
        app.view.confirm = None;
        app.doc.dirty = false;
        app.request(files::Pending::New);
        assert_eq!(app.doc.project.name, "Untitled");
    }

    #[test]
    fn dont_save_keeps_the_document_dirty_and_quit_closes_once() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.doc.dirty = true;
        app.view.confirm = Some(files::Pending::Quit);
        app.dont_save(files::Pending::Quit);
        assert!(app.doc.dirty);
        assert!(app.view.confirm.is_none());
        assert!(app.view.quit);
    }

    #[test]
    fn copy_sets_clipboard_text() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.copy_pattern();
        assert!(
            app.view
                .clip_text
                .as_deref()
                .is_some_and(|t| t.starts_with("SidAcid pattern "))
        );
    }
}
