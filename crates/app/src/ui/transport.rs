//! The transport bar: play/stop, tempo, swing, lo-fi, master, CPU.

use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::params::{MasterParam, ParamId};
use eframe::egui;

impl SidAcidApp {
    pub(crate) fn transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            self.file_menu(ui);
            ui.separator();
            ui.heading(egui::RichText::new("SidAcid").color(theme::ACID).strong());
            ui.separator();
            let playing = self.status.playing;
            if ui
                .add(egui::Button::new("Play").selected(playing))
                .clicked()
            {
                self.send(vec![Command::Play]);
            }
            if ui.button("Stop").clicked() {
                self.send(vec![Command::Stop]);
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
                let cmds = self.doc.apply(Edit::Bpm(bpm), true);
                self.send(cmds);
            }
            if r.drag_stopped() || r.lost_focus() {
                self.doc.end_gesture();
            }
            let shown_swing = self.doc.project.swing * 100.0;
            let mut swing = shown_swing;
            let r = ui.add(
                egui::Slider::new(&mut swing, 0.0..=75.0)
                    .text("Swing %")
                    .max_decimals(0),
            );
            if r.changed() && (swing - shown_swing).abs() > 1e-3 {
                let cmds = self.doc.apply(Edit::Swing(swing / 100.0), true);
                self.send(cmds);
            }
            if r.drag_stopped() {
                self.doc.end_gesture();
            }
            ui.separator();
            self.param_knob(ui, ParamId::Master(MasterParam::LoFi), "Lo-Fi");
            self.param_knob(ui, ParamId::Master(MasterParam::Level), "Master");
            ui.separator();
            let cpu = self.status.cpu * 100.0;
            let colour = if cpu > 80.0 {
                theme::ACCENT
            } else {
                ui.visuals().text_color()
            };
            let cpu_label = ui.label(egui::RichText::new(format!("CPU {cpu:.0}%")).color(colour));
            if let Some(audio) = &self.audio {
                cpu_label.on_hover_text(audio.device_name.as_str());
            }
        });
    }
}
