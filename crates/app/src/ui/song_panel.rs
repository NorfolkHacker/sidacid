//! Song panel: the song's length and the list of automated parameters.
//! Blocks are edited in the dock's Song mode.

use super::SidAcidApp;
use super::params_ui::param_label;
use crate::document::Edit;
use acid_engine::project::Song;
use acid_engine::song;
use eframe::egui;

impl SidAcidApp {
    pub(crate) fn apply_song(&mut self, song: Song, merge: bool) {
        let cmds = self.doc.apply(
            Edit::Song {
                song: Box::new(song),
            },
            merge,
        );
        self.send(cmds);
    }

    /// The song's length in 16-step bars, rounded up.
    pub(crate) fn song_bars(&self) -> u32 {
        let lens = song::lens_of(&self.doc.project);
        song::song_end(&self.doc.project.song, &lens).div_ceil(16)
    }

    pub(crate) fn song_panel(&mut self, ui: &mut egui::Ui) {
        let title = format!("SONG   {} bars", self.song_bars());
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt("song")
            .default_open(true)
            .show(ui, |ui| {
                self.automation_list(ui);
            });
    }

    fn automation_list(&mut self, ui: &mut egui::Ui) {
        let lanes = &self.doc.project.song.automation;
        if lanes.is_empty() {
            return;
        }
        // None = clear all; Some(param) = clear one lane.
        let mut clear = None;
        ui.label(egui::RichText::new("Automation").strong());
        for l in lanes {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{}  ({} steps)",
                    param_label(&self.doc.project, l.param),
                    l.points.len()
                ));
                if ui.button("Clear").clicked() {
                    clear = Some(Some(l.param));
                }
            });
        }
        if ui.button("Clear all automation").clicked() {
            clear = Some(None);
        }
        if let Some(which) = clear {
            let mut s = self.doc.project.song.clone();
            match which {
                Some(param) => song::clear_automation(&mut s, param),
                None => s.automation.clear(),
            }
            self.apply_song(s, false);
        }
    }
}
