//! A drum machine panel: per-instrument knobs, kit knobs, pattern pads,
//! length, and the 8-row step grid (click cycles Off → On → Accent).

use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::params::{DrumParam, KitParam, ParamId};
use acid_engine::project::{DrumHit, PatternRef};
use eframe::egui;

pub const INST_NAMES: [&str; 8] = ["BD", "SD", "LT", "HT", "CP", "RS", "CH", "OH"];
const KIT_NAMES: [&str; 2] = ["808", "909"];
const KNOB_ROWS: [(DrumParam, &str); 4] = [
    (DrumParam::Tune, "Tune"),
    (DrumParam::Decay, "Decay"),
    (DrumParam::Tone, "Tone"),
    (DrumParam::Level, "Level"),
];
const CELL: [f32; 2] = [24.0, 18.0];

pub fn cycle_hit(hit: DrumHit) -> DrumHit {
    match hit {
        DrumHit::Off => DrumHit::On,
        DrumHit::On => DrumHit::Accent,
        DrumHit::Accent => DrumHit::Off,
    }
}

impl SidAcidApp {
    pub(crate) fn drum_panel(&mut self, ui: &mut egui::Ui, kit: usize) {
        let machine = 2 + kit;
        let slot = self.doc.project.selected[machine];
        let title = format!("{} DRUMS   {}", KIT_NAMES[kit], PatternRef(slot).label());
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt(("drums", kit))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    egui::Grid::new(("drum_knobs", kit))
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
                                    self.param_knob(ui, ParamId::Drum(kit as u8, inst, p), "");
                                }
                                ui.end_row();
                            }
                        });
                    ui.separator();
                    ui.vertical(|ui| {
                        self.param_knob(ui, ParamId::Kit(kit as u8, KitParam::Volume), "Volume");
                        self.param_knob(ui, ParamId::Kit(kit as u8, KitParam::Accent), "Accent");
                    });
                });
                egui::ScrollArea::horizontal()
                    .id_salt(("drum_pads", kit))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            self.pattern_pads(ui, machine);
                            ui.separator();
                            let mut length =
                                self.doc.project.drums[kit].patterns[slot as usize].length;
                            let r = ui.add(
                                egui::DragValue::new(&mut length)
                                    .range(1..=64)
                                    .prefix("Len "),
                            );
                            if r.changed() {
                                self.set_drum_length(kit, slot, length, r.dragged());
                            }
                            if r.drag_stopped() || r.lost_focus() {
                                self.doc.end_gesture();
                            }
                            ui.separator();
                            if ui
                                .button("Template")
                                .on_hover_text("Replace with a fresh groove (undoable)")
                                .clicked()
                            {
                                self.drum_template(kit);
                            }
                            if ui
                                .button("Evolve")
                                .on_hover_text("Small random changes (undoable)")
                                .clicked()
                            {
                                self.evolve_drums(kit);
                            }
                        });
                    });
                self.drum_grid(ui, kit, slot);
            });
    }

    fn drum_grid(&mut self, ui: &mut egui::Ui, kit: usize, slot: u8) {
        let machine = 2 + kit;
        let pattern = self.doc.project.drums[kit].patterns[slot as usize].clone();
        let len = pattern.length as usize;
        let playhead = (self.status.playing && self.status.patterns[machine] == slot)
            .then_some(self.status.playhead[machine]);
        let mut edits = Vec::new();
        egui::ScrollArea::horizontal()
            .id_salt(("drum_grid", kit))
            .show(ui, |ui| {
                theme::note_grid_visuals(ui.visuals_mut());
                egui::Grid::new(("drum_grid_cells", kit))
                    .spacing([2.0, 2.0])
                    .show(ui, |ui| {
                        ui.label("");
                        for i in 0..len {
                            let mut t = egui::RichText::new(format!("{}", i + 1)).size(10.0);
                            if Some(i) == playhead {
                                t = t.color(theme::ACID).strong();
                            }
                            ui.label(t);
                        }
                        ui.end_row();
                        for (inst, name) in INST_NAMES.iter().enumerate() {
                            ui.label(*name);
                            for step in 0..len {
                                let hit = pattern.steps[step][inst];
                                let (text, colour) = match hit {
                                    DrumHit::Off => ("·", ui.visuals().weak_text_color()),
                                    DrumHit::On => ("■", theme::ACID),
                                    DrumHit::Accent => ("■", theme::ACCENT),
                                };
                                let r = ui.add_sized(
                                    CELL,
                                    egui::Button::new(egui::RichText::new(text).color(colour))
                                        .selected(hit != DrumHit::Off),
                                );
                                if r.clicked() {
                                    edits.push((step, inst, None));
                                }
                                if r.secondary_clicked() {
                                    edits.push((step, inst, Some(DrumHit::Off)));
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        for (step, inst, hit) in edits {
            self.click_drum_cell(kit, slot, step, inst, hit);
        }
    }

    /// A grid click: `hit`, or with None the next hit after the current
    /// one (Off → On → Accent). One undo step.
    pub(crate) fn click_drum_cell(
        &mut self,
        kit: usize,
        slot: u8,
        step: usize,
        inst: usize,
        hit: Option<DrumHit>,
    ) {
        let current = self.doc.project.drums[kit].patterns[slot as usize].steps[step][inst];
        let hit = hit.unwrap_or_else(|| cycle_hit(current));
        let cmds = self.doc.apply(
            Edit::DrumHit {
                kit,
                slot,
                step,
                inst,
                hit,
            },
            false,
        );
        self.send(cmds);
        self.view.focus = 2 + kit;
    }

    /// The Len field. `merge` is true only while dragging, so a drag is one
    /// undo step and each arrow key or typed value is its own.
    pub(crate) fn set_drum_length(&mut self, kit: usize, slot: u8, length: u8, merge: bool) {
        let cmds = self
            .doc
            .apply(Edit::DrumLength { kit, slot, length }, merge);
        self.send(cmds);
        self.view.focus = 2 + kit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> SidAcidApp {
        SidAcidApp::headless(crate::document::Document::new(
            acid_engine::demo::demo_project(),
        ))
    }

    #[test]
    fn drum_cell_click_cycles_and_undoes() {
        let mut app = app();
        let slot = 5;
        let hit = |app: &SidAcidApp| app.doc.project.drums[1].patterns[slot as usize].steps[3][2];
        assert_eq!(hit(&app), DrumHit::Off);
        app.click_drum_cell(1, slot, 3, 2, None);
        assert_eq!(hit(&app), DrumHit::On);
        assert_eq!(app.view.focus, 3);
        app.click_drum_cell(1, slot, 3, 2, None);
        assert_eq!(hit(&app), DrumHit::Accent);
        app.click_drum_cell(1, slot, 3, 2, Some(DrumHit::Off));
        assert_eq!(hit(&app), DrumHit::Off, "a right-click clears");
        app.doc.undo();
        assert_eq!(hit(&app), DrumHit::Accent, "each click is one undo step");
        app.doc.undo();
        app.doc.undo();
        assert_eq!(hit(&app), DrumHit::Off);
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn drum_length_edits_focus_the_kit() {
        let mut app = app();
        let slot = app.doc.project.selected[3];
        app.set_drum_length(1, slot, 12, false);
        assert_eq!(app.doc.project.drums[1].patterns[slot as usize].length, 12);
        assert_eq!(app.view.focus, 3, "copy/paste now acts on the 909");
    }

    #[test]
    fn keyboard_drum_length_edits_are_separate_undo_steps() {
        let mut app = app();
        let slot = app.doc.project.selected[3];
        app.set_drum_length(1, slot, 12, false);
        app.set_drum_length(1, slot, 13, false);
        app.doc.undo();
        assert_eq!(app.doc.project.drums[1].patterns[slot as usize].length, 12);
    }

    #[test]
    fn the_drum_panels_draw_headless() {
        let mut app = app();
        let before = app.doc.project.clone();
        super::super::run_frames(&mut app, 2);
        assert_eq!(app.doc.project, before, "drawing changes nothing");
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn hits_cycle() {
        assert_eq!(cycle_hit(DrumHit::Off), DrumHit::On);
        assert_eq!(cycle_hit(DrumHit::On), DrumHit::Accent);
        assert_eq!(cycle_hit(DrumHit::Accent), DrumHit::Off);
    }
}
