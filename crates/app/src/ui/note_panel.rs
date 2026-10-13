//! The note-pattern row (pads, length, snap) shared by the FM and Sampler panels.

use super::SidAcidApp;
use super::poly_edit::Snap;
use crate::document::Edit;
use acid_engine::machine::MachineId;
use eframe::egui;

impl SidAcidApp {
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
                            // Only a drag merges into one undo step; an arrow key
                            // or typed value is its own step.
                            r.dragged(),
                        );
                        self.send(cmds);
                        self.view.focus = m;
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
