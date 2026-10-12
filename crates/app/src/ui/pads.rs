//! Bank (A–H) and pattern (1–16) pads for one machine.

use super::{SidAcidApp, theme};
use acid_engine::project::BANK_SIZE;
use eframe::egui;

pub fn bank_letter(bank: u8) -> String {
    ((b'A' + bank) as char).to_string()
}

impl SidAcidApp {
    pub(crate) fn pattern_pads(&mut self, ui: &mut egui::Ui, machine: usize) {
        let selected = self.doc.project.selected[machine];
        let playing = self.status.playing.then_some(self.status.patterns[machine]);
        let queued = self.status.queued[machine];
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                for bank in 0..8u8 {
                    let shown = self.view.bank[machine] == bank;
                    if ui.selectable_label(shown, bank_letter(bank)).clicked() {
                        self.view.bank[machine] = bank;
                    }
                }
            });
            ui.horizontal(|ui| {
                let bank = self.view.bank[machine];
                for i in 0..BANK_SIZE as u8 {
                    let slot = bank * BANK_SIZE as u8 + i;
                    let mut text = egui::RichText::new(format!("{:>2}", i + 1)).monospace();
                    if Some(slot) == queued {
                        text = text.color(theme::QUEUED);
                    } else if Some(slot) == playing {
                        text = text.color(theme::ACID).strong();
                    }
                    if ui.selectable_label(slot == selected, text).clicked() {
                        self.select_pad(machine, slot);
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
    pub(crate) fn select_pad(&mut self, machine: usize, slot: u8) {
        let changed = self.doc.project.selected[machine] != slot;
        let cmds = self.doc.select_pattern(machine, slot);
        self.send(cmds);
        self.view.focus = machine;
        if changed && machine < self.view.writers.len() {
            self.view.writers[machine] = None;
        }
        self.record_pattern_change(machine, slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::ui::step_edit::StepWriter;

    #[test]
    fn selecting_another_pattern_stops_that_synths_step_writer() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let slot = app.doc.project.selected[0];
        app.view.writers[0] = Some(StepWriter::new(3));
        app.select_pad(0, slot);
        assert!(
            app.view.writers[0].is_some(),
            "the same pattern: keep writing"
        );
        app.select_pad(2, slot + 1);
        assert!(
            app.view.writers[0].is_some(),
            "a drum pad leaves the synth alone"
        );
        app.select_pad(0, slot + 1);
        assert!(app.view.writers[0].is_none());
        assert_eq!(app.doc.project.selected[0], slot + 1);
        assert_eq!(app.view.focus, 0);
    }
}
