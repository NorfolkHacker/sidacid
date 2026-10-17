//! The Pad Kit panel: its title and pattern row for now.

use super::SidAcidApp;
use acid_engine::machine::MachineUid;
use acid_engine::project::PatternRef;
use eframe::egui;

impl SidAcidApp {
    pub(crate) fn padkit_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{} PADS   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label()
        );
        self.machine_panel(ui, uid, "padkit", title, |app, ui| {
            app.note_pattern_row(ui, uid);
        });
    }
}
