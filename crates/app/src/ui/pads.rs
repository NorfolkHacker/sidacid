//! Bank (A–H) and pattern (1–16) pads for one machine.

use super::{SidAcidApp, theme};
use acid_engine::machine::MachineUid;
use acid_engine::project::BANK_SIZE;
use eframe::egui;

pub fn bank_letter(bank: u8) -> String {
    ((b'A' + bank) as char).to_string()
}

impl SidAcidApp {
    pub(crate) fn pattern_pads(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let selected = self.selected(uid);
        // `Status` reports by rack position.
        let pos = self.doc.project.position(uid);
        let playing = pos
            .filter(|_| self.status.playing)
            .map(|i| self.status.patterns[i]);
        let queued = pos.and_then(|i| self.status.queued[i]);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                for bank in 0..8u8 {
                    let shown = self.view.panel(uid).bank == bank;
                    if ui.selectable_label(shown, bank_letter(bank)).clicked() {
                        self.view.panel(uid).bank = bank;
                    }
                }
            });
            ui.horizontal(|ui| {
                let bank = self.view.panel(uid).bank;
                for i in 0..BANK_SIZE as u8 {
                    let slot = bank * BANK_SIZE as u8 + i;
                    let mut text = egui::RichText::new(format!("{:>2}", i + 1)).monospace();
                    if Some(slot) == queued {
                        text = text.color(theme::QUEUED);
                    } else if Some(slot) == playing {
                        text = text.color(theme::ACID).strong();
                    }
                    if ui.selectable_label(slot == selected, text).clicked() {
                        self.select_pad(uid, slot);
                    }
                }
            });
        });
    }
}

impl SidAcidApp {
    /// A pattern pad click: select the pattern (recorded into the song
    /// while recording). A synth's step-writer stops when another pattern
    /// is selected, so keys never land in a pattern the user did not click.
    pub(crate) fn select_pad(&mut self, uid: MachineUid, slot: u8) {
        if self.selected(uid) != slot
            && let Some(panel) = self.view.panels.get_mut(&uid)
        {
            panel.writer = None;
        }
        let cmds = self.doc.select_pattern(uid, slot);
        self.send(cmds);
        // A ruler range picked notes from the old patterns.
        self.view.dock.range = None;
        self.view.focus = Some(uid);
        self.record_pattern_change(uid, slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::test_rack::*;
    use crate::ui::step_edit::StepWriter;

    #[test]
    fn selecting_another_pattern_stops_that_synths_step_writer() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let slot = app.doc.project.machine(SYNTH1).unwrap().selected;
        app.view.panel(SYNTH1).writer = Some(StepWriter::new(3));
        app.select_pad(SYNTH1, slot);
        assert!(
            app.view.panel(SYNTH1).writer.is_some(),
            "the same pattern: keep writing"
        );
        app.select_pad(KIT808, slot + 1);
        assert!(
            app.view.panel(SYNTH1).writer.is_some(),
            "a drum pad leaves the synth alone"
        );
        app.select_pad(SYNTH1, slot + 1);
        assert!(app.view.panel(SYNTH1).writer.is_none());
        assert_eq!(app.doc.project.machine(SYNTH1).unwrap().selected, slot + 1);
        assert_eq!(app.view.focus, Some(SYNTH1));
    }
}
