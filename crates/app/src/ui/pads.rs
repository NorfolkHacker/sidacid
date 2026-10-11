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
                        let cmds = self.doc.select_pattern(machine, slot);
                        self.send(cmds);
                        self.view.focus = machine;
                    }
                }
            });
        });
    }
}
