//! A drum machine panel: per-instrument knobs, kit knobs, pattern pads,
//! length, Template and Evolve. The hits are edited in the kit's lane in
//! the dock.

use super::SidAcidApp;
use super::lane::INST_NAMES;
use crate::document::Edit;
use acid_engine::machine::MachineUid;
use acid_engine::params::{DrumParam, KitParam, MachineParam};
use acid_engine::project::{DrumPattern, PatternRef};
use eframe::egui;

const KNOB_ROWS: [(DrumParam, &str); 4] = [
    (DrumParam::Tune, "Tune"),
    (DrumParam::Decay, "Decay"),
    (DrumParam::Tone, "Tone"),
    (DrumParam::Level, "Level"),
];
impl SidAcidApp {
    /// Kit `uid`'s pattern `slot`, if `uid` is a drum kit in the rack.
    fn drum_pattern(&self, uid: MachineUid, slot: u8) -> Option<&DrumPattern> {
        Some(&self.doc.project.kit(uid)?.patterns[slot as usize])
    }

    pub(crate) fn drum_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{} DRUMS   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label()
        );
        self.machine_panel(ui, uid, "drums", title, |app, ui| {
            ui.horizontal(|ui| {
                egui::Grid::new(("drum_knobs", uid))
                    .spacing([0.0, 0.0])
                    .show(ui, |ui| {
                        ui.label("");
                        for name in INST_NAMES {
                            ui.label(egui::RichText::new(name).strong());
                        }
                        ui.end_row();
                        for (p, label) in KNOB_ROWS {
                            ui.label(label);
                            for inst in 0..8u8 {
                                app.param_knob(ui, uid.param(MachineParam::Drum(inst, p)), "");
                            }
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.vertical(|ui| {
                    app.param_knob(ui, uid.param(KitParam::Volume), "Volume");
                    app.param_knob(ui, uid.param(KitParam::Accent), "Accent");
                });
            });
            egui::ScrollArea::horizontal()
                .id_salt(("drum_pads", uid))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        app.pattern_pads(ui, uid);
                        ui.separator();
                        let mut length = app.drum_pattern(uid, slot).map_or(16, |p| p.roll.length);
                        let r = ui.add(
                            egui::DragValue::new(&mut length)
                                .range(1..=64)
                                .prefix("Len "),
                        );
                        if r.changed() {
                            app.set_drum_length(uid, slot, length, r.dragged());
                        }
                        if r.drag_stopped() || r.lost_focus() {
                            app.doc.end_gesture();
                        }
                        ui.separator();
                        if ui
                            .button("Template")
                            .on_hover_text("Replace with a fresh groove (undoable)")
                            .clicked()
                        {
                            app.drum_template(uid);
                        }
                        if ui
                            .button("Evolve")
                            .on_hover_text("Small random changes (undoable)")
                            .clicked()
                        {
                            app.evolve_drums(uid);
                        }
                    });
                });
        });
    }

    /// The Len field. `merge` is true only while dragging, so a drag is one
    /// undo step and each arrow key or typed value is its own.
    pub(crate) fn set_drum_length(&mut self, uid: MachineUid, slot: u8, length: u8, merge: bool) {
        self.tool_yield(uid, slot);
        let cmds = self
            .doc
            .apply(Edit::NoteLength { uid, slot, length }, merge);
        self.send(cmds);
        self.view.focus = Some(uid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rack::*;

    fn app() -> SidAcidApp {
        SidAcidApp::headless(crate::document::Document::new(
            acid_engine::demo::demo_project(),
        ))
    }

    #[test]
    fn drum_length_edits_focus_the_kit() {
        let mut app = app();
        let slot = app.doc.project.machine(KIT909).unwrap().selected;
        app.set_drum_length(KIT909, slot, 12, false);
        assert_eq!(
            app.doc.project.kit(KIT909).unwrap().patterns[slot as usize]
                .roll
                .length,
            12
        );
        assert_eq!(
            app.view.focus,
            Some(KIT909),
            "copy/paste now acts on the 909"
        );
    }

    #[test]
    fn keyboard_drum_length_edits_are_separate_undo_steps() {
        let mut app = app();
        let slot = app.doc.project.machine(KIT909).unwrap().selected;
        app.set_drum_length(KIT909, slot, 12, false);
        app.set_drum_length(KIT909, slot, 13, false);
        app.doc.undo();
        assert_eq!(
            app.doc.project.kit(KIT909).unwrap().patterns[slot as usize]
                .roll
                .length,
            12
        );
    }

    #[test]
    fn the_drum_panels_draw_headless() {
        let mut app = app();
        let before = app.doc.project.clone();
        super::super::run_frames(&mut app, 2);
        assert_eq!(app.doc.project, before, "drawing changes nothing");
        assert!(!app.doc.can_undo());
    }
}
