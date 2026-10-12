//! The Sampler panel, a placeholder until Plan 6: it plays a sine voice
//! and shows a volume knob, the note pattern row and the poly piano roll.
//! The pattern row (pads, length, snap) is shared with the FM panel.

use super::SidAcidApp;
use super::poly_edit::Snap;
use crate::document::Edit;
use acid_engine::machine::MachineId;
use acid_engine::params::{ParamId, SamplerParam};
use acid_engine::project::PatternRef;
use eframe::egui;

impl SidAcidApp {
    pub(crate) fn note_panel(&mut self, ui: &mut egui::Ui, machine: MachineId) {
        let m = machine.index();
        let slot = self.doc.project.selected[m];
        let title = format!(
            "{}   {}",
            machine.name().to_uppercase(),
            PatternRef(slot).label()
        );
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt(("notes", m))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    self.param_knob(ui, ParamId::Sampler(SamplerParam::Volume), "Volume");
                    ui.weak("Placeholder sine voice until the real engine arrives.");
                });
                self.note_pattern_row(ui, m);
                self.poly_roll(ui, m);
            });
    }

    /// Pattern pads, length and snap for a note machine (FM or Sampler).
    pub(crate) fn note_pattern_row(&mut self, ui: &mut egui::Ui, m: usize) {
        let n = m - MachineId::Fm.index();
        let slot = self.doc.project.selected[m];
        egui::ScrollArea::horizontal()
            .id_salt(("note_pads", m))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    self.pattern_pads(ui, m);
                    ui.separator();
                    let mut length = self
                        .doc
                        .project
                        .note_patterns(m)
                        .map_or(16, |p| p[slot as usize].length);
                    let r = ui.add(
                        egui::DragValue::new(&mut length)
                            .range(1..=64)
                            .prefix("Len "),
                    );
                    if r.changed() {
                        let cmds = self.doc.apply(
                            Edit::NoteLength {
                                machine: m,
                                slot,
                                length,
                            },
                            true,
                        );
                        self.send(cmds);
                    }
                    if r.drag_stopped() || r.lost_focus() {
                        self.doc.end_gesture();
                    }
                    ui.separator();
                    ui.label("Snap");
                    for s in Snap::ALL {
                        if ui
                            .selectable_label(self.view.poly_snap[n] == s, s.label())
                            .clicked()
                        {
                            self.view.poly_snap[n] = s;
                        }
                    }
                });
            });
    }
}
