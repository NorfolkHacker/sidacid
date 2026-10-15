//! The Mono panel: the synth's knobs in front-panel order, the patch
//! browser, and the pattern row with the poly piano roll.

use super::SidAcidApp;
use acid_engine::machine::MachineUid;
use acid_engine::project::{MonoMachine, PatternRef};
use eframe::egui;

impl SidAcidApp {
    /// Mono machine `uid`; it must be a Mono machine in the rack.
    fn mono_of(&self, uid: MachineUid) -> &MonoMachine {
        self.doc.project.mono(uid).expect("a Mono machine in the rack")
    }

    pub(crate) fn mono_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{}   {}   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label(),
            self.mono_of(uid).patch_name
        );
        self.machine_panel(ui, uid, "mono", title, |app, ui| {
            app.note_pattern_row(ui, uid);
            app.poly_roll(ui, uid);
        });
    }
}
