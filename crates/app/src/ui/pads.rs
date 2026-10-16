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
    /// while recording).
    pub(crate) fn select_pad(&mut self, uid: MachineUid, slot: u8) {
        let cmds = self.doc.select_pattern(uid, slot);
        self.send(cmds);
        self.view.focus = Some(uid);
        self.record_pattern_change(uid, slot);
    }
}
