//! The transport bar: play/stop, tempo, swing, lo-fi, master, performance.

use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::params::{MasterParam, ParamId};
use acid_engine::sequencer::Mode;
use eframe::egui;

impl SidAcidApp {
    pub(crate) fn transport(&mut self, ui: &mut egui::Ui) {
        // Wraps onto a second row instead of clipping in narrow windows.
        ui.horizontal_wrapped(|ui| {
            self.file_menu(ui);
            ui.separator();
            ui.heading(egui::RichText::new("SidAcid").color(theme::ACID).strong());
            ui.separator();
            let playing = self.status.playing;
            if ui
                .add(egui::Button::new("Play").selected(playing))
                .clicked()
            {
                self.play();
            }
            if ui.button("Stop").clicked() {
                self.stop();
            }
            ui.separator();
            for (m, label) in [(Mode::Pattern, "Pattern"), (Mode::Song, "Song")] {
                if ui.selectable_label(self.view.mode == m, label).clicked() {
                    self.set_mode(m);
                }
            }
            let rec_text = egui::RichText::new("Rec").color(if self.view.record {
                theme::ACCENT
            } else {
                ui.visuals().text_color()
            });
            let rec = ui.add_enabled(
                self.view.mode == Mode::Song,
                egui::Button::new(rec_text).selected(self.view.record),
            );
            if rec
                .on_hover_text("Record pattern changes and knob moves into the song")
                .clicked()
            {
                self.set_record(!self.view.record);
            }
            if self.view.mode == Mode::Song {
                let bar = if self.status.playing {
                    self.status.song_bar as usize
                } else {
                    // The bar playback will start from (see `play`).
                    let last = self.doc.project.song.bars.len().saturating_sub(1);
                    self.view.song_cursor.min(last)
                };
                ui.label(format!("Bar {}", bar + 1));
            }
            ui.separator();
            let shown_bpm = self.doc.project.bpm;
            let mut bpm = shown_bpm;
            let r = ui.add(
                egui::DragValue::new(&mut bpm)
                    .range(40.0..=300.0)
                    .speed(0.2)
                    .max_decimals(1)
                    .suffix(" BPM"),
            );
            // Widget rounding must not turn a plain redraw into an edit.
            if r.changed() && (bpm - shown_bpm).abs() > 1e-3 {
                self.edit_transport(Edit::Bpm(bpm));
            }
            if r.drag_stopped() || r.lost_focus() {
                self.doc.end_gesture();
            }
            let shown_swing = self.doc.project.swing * 100.0;
            let mut swing = shown_swing;
            let r = ui.add(
                egui::Slider::new(&mut swing, 0.0..=75.0)
                    .text("Swing %")
                    .clamping(egui::SliderClamping::Edits)
                    .max_decimals(0),
            );
            if r.changed() && (swing - shown_swing).abs() > 1e-3 {
                self.edit_transport(Edit::Swing(swing / 100.0));
            }
            if r.drag_stopped() || r.lost_focus() {
                self.doc.end_gesture();
            }
            ui.separator();
            self.param_knob(ui, ParamId::Master(MasterParam::LoFi), "Lo-Fi");
            self.param_knob(ui, ParamId::Master(MasterParam::Level), "Master");
            ui.separator();
            self.perf_readout(ui);
            if let Some(line) = &self.view.status_line {
                ui.separator();
                ui.label(egui::RichText::new(line).small());
            }
        });
    }
}

impl SidAcidApp {
    /// A tempo or swing edit: one undo step per drag, or part of the
    /// record pass while recording.
    pub(crate) fn edit_transport(&mut self, edit: Edit) {
        let cmds = if self.recording() {
            self.doc.record(edit)
        } else {
            self.doc.apply(edit, true)
        };
        self.send(cmds);
    }
}
