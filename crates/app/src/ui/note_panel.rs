//! The note-pattern row (pads, length) shared by the FM, Sampler and Mono panels.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::machine::MachineUid;
use eframe::egui;

impl SidAcidApp {
    /// Pattern pads and length for a note machine (FM, Sampler or Mono).
    pub(crate) fn note_pattern_row(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        egui::ScrollArea::horizontal()
            .id_salt(("note_pads", uid))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    self.pattern_pads(ui, uid);
                    ui.separator();
                    let mut length = self
                        .doc
                        .project
                        .note_patterns(uid)
                        .map_or(16, |p| p[slot as usize].length);
                    let r = ui.add(
                        egui::DragValue::new(&mut length)
                            .range(1..=64)
                            .prefix("Len "),
                    );
                    if r.changed() {
                        let cmds = self.doc.apply(
                            Edit::NoteLength { uid, slot, length },
                            // Only a drag merges into one undo step; an arrow key
                            // or typed value is its own step.
                            r.dragged(),
                        );
                        self.send(cmds);
                        self.view.focus = Some(uid);
                    }
                    if r.drag_stopped() || r.lost_focus() {
                        self.doc.end_gesture();
                    }
                });
            });
    }
}
