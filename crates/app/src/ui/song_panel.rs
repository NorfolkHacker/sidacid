//! Song editor: bar timeline (click selects, Shift-click extends), the
//! selected bar's patterns/mutes/transpose, bar operations, loop markers,
//! and the list of automated parameters.

use super::params_ui::param_label;
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::project::{Bar, PatternRef, Song};
use acid_engine::sequencer::Mode;
use acid_engine::song;
use eframe::egui;

const MACHINES: [&str; 4] = ["Synth 1", "Synth 2", "808", "909"];

impl SidAcidApp {
    /// Selected bar range (first, last), clamped to the song.
    pub(crate) fn song_selection(&self) -> Option<(usize, usize)> {
        let n = self.doc.project.song.bars.len();
        if n == 0 {
            return None;
        }
        let a = self.view.song_cursor.min(n - 1);
        let b = self.view.song_sel_end.unwrap_or(a).min(n - 1);
        Some((a.min(b), a.max(b)))
    }

    pub(crate) fn apply_song(&mut self, song: Song, merge: bool) {
        let cmds = self.doc.apply(
            Edit::Song {
                song: Box::new(song),
            },
            merge,
        );
        self.send(cmds);
    }

    fn bar_from_selected_patterns(&self) -> Bar {
        Bar {
            patterns: self.doc.project.selected.map(|s| Some(PatternRef(s))),
            ..Bar::default()
        }
    }

    pub(crate) fn song_panel(&mut self, ui: &mut egui::Ui) {
        let title = format!("SONG   {} bars", self.doc.project.song.bars.len());
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt("song")
            .default_open(true)
            .show(ui, |ui| {
                self.song_toolbar(ui);
                self.song_timeline(ui);
                self.bar_editor(ui);
                self.automation_list(ui);
            });
    }

    fn song_toolbar(&mut self, ui: &mut egui::Ui) {
        let project = self.doc.project.clone();
        let (synths, drums) = song::banks(&project);
        let sel = self.song_selection();
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Add bar")
                .on_hover_text("Append a bar using each machine's selected pattern")
                .clicked()
            {
                let mut s = project.song.clone();
                let at = s.bars.len();
                song::insert_bars(
                    &mut s,
                    at,
                    &[self.bar_from_selected_patterns()],
                    synths,
                    drums,
                );
                self.view.song_cursor = at;
                self.view.song_sel_end = None;
                self.apply_song(s, false);
            }
            if ui.button("Insert").clicked() {
                let mut s = project.song.clone();
                let at = sel.map_or(s.bars.len(), |(a, _)| a);
                let bar = sel.map_or_else(|| self.bar_from_selected_patterns(), |(a, _)| s.bars[a]);
                song::insert_bars(&mut s, at, &[bar], synths, drums);
                self.apply_song(s, false);
            }
            if let Some((a, b)) = sel {
                if ui.button("Duplicate").clicked() {
                    let mut s = project.song.clone();
                    song::duplicate_bars(&mut s, a, b, synths, drums);
                    self.apply_song(s, false);
                }
                if ui.button("Delete").clicked() {
                    let mut s = project.song.clone();
                    song::delete_bars(&mut s, a, b, synths, drums);
                    self.view.song_cursor = a.min(s.bars.len().saturating_sub(1));
                    self.view.song_sel_end = None;
                    self.apply_song(s, false);
                }
                if ui.button("Copy").clicked() {
                    self.view.song_clip = Some(project.song.bars[a..=b].to_vec());
                }
                if ui.button("Loop selection").clicked() {
                    let mut s = project.song.clone();
                    s.loop_range = Some((a as u16, b as u16));
                    self.apply_song(s, false);
                }
            }
            if let Some(clip) = self.view.song_clip.clone()
                && ui.button(format!("Paste {} bars", clip.len())).clicked()
            {
                let mut s = project.song.clone();
                let at = sel.map_or(s.bars.len(), |(_, b)| b + 1);
                song::insert_bars(&mut s, at, &clip, synths, drums);
                self.apply_song(s, false);
            }
            if project.song.loop_range.is_some() && ui.button("No loop").clicked() {
                let mut s = project.song.clone();
                s.loop_range = None;
                self.apply_song(s, false);
            }
        });
    }

    fn song_timeline(&mut self, ui: &mut egui::Ui) {
        let bars = self.doc.project.song.bars.clone();
        let loop_range = self.doc.project.song.loop_range;
        let sel = self.song_selection();
        let playing_bar = (self.status.playing && self.view.mode == Mode::Song)
            .then_some(self.status.song_bar as usize);
        let mut clicked = None;
        egui::ScrollArea::horizontal()
            .id_salt("song_timeline")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if bars.is_empty() {
                        ui.weak("No bars yet — press \"Add bar\".");
                    }
                    for (i, bar) in bars.iter().enumerate() {
                        let cell = |m: usize| {
                            let label = bar.patterns[m].map_or("—".to_string(), |r| r.label());
                            if bar.mutes[m] {
                                format!("{label}×")
                            } else {
                                label
                            }
                        };
                        let text = format!(
                            "{}\n{}\n{}\n{}\n{}",
                            i + 1,
                            cell(0),
                            cell(1),
                            cell(2),
                            cell(3)
                        );
                        let mut rich = egui::RichText::new(text).monospace().size(10.0);
                        if Some(i) == playing_bar {
                            rich = rich.color(theme::ACID).strong();
                        } else if loop_range
                            .is_some_and(|(a, b)| (a as usize..=b as usize).contains(&i))
                        {
                            rich = rich.color(theme::QUEUED);
                        }
                        let selected = sel.is_some_and(|(a, b)| (a..=b).contains(&i));
                        let r =
                            ui.add_sized([44.0, 84.0], egui::Button::new(rich).selected(selected));
                        if r.clicked() {
                            clicked = Some((i, ui.input(|inp| inp.modifiers.shift)));
                        }
                    }
                });
            });
        if let Some((i, extend)) = clicked {
            if extend {
                self.view.song_sel_end = Some(i);
            } else {
                self.view.song_cursor = i;
                self.view.song_sel_end = None;
                if !self.status.playing {
                    self.send(vec![Command::SetSongStart(i as u16)]);
                }
            }
        }
    }

    fn bar_editor(&mut self, ui: &mut egui::Ui) {
        let Some((a, _)) = self.song_selection() else {
            return;
        };
        let mut bar = self.doc.project.song.bars[a];
        let selected = self.doc.project.selected;
        let mut changed = false;
        let mut merge = false;
        let mut finished = false;
        ui.label(egui::RichText::new(format!("Bar {}", a + 1)).strong());
        egui::Grid::new("bar_editor").striped(true).show(ui, |ui| {
            for m in 0..4 {
                ui.label(MACHINES[m]);
                ui.monospace(bar.patterns[m].map_or("—".to_string(), |r| r.label()));
                if ui
                    .button(format!("Use {}", PatternRef(selected[m]).label()))
                    .clicked()
                {
                    bar.patterns[m] = Some(PatternRef(selected[m]));
                    changed = true;
                }
                if ui.button("Clear").clicked() {
                    bar.patterns[m] = None;
                    changed = true;
                }
                if ui.checkbox(&mut bar.mutes[m], "Mute").changed() {
                    changed = true;
                }
                if m < 2 {
                    let r = ui.add(
                        egui::DragValue::new(&mut bar.transpose[m])
                            .range(-12..=12)
                            .prefix("Trans "),
                    );
                    if r.changed() {
                        changed = true;
                        merge = true;
                    }
                    finished |= r.drag_stopped() || r.lost_focus();
                }
                ui.end_row();
            }
        });
        if changed {
            let mut s = self.doc.project.song.clone();
            s.bars[a] = bar;
            self.apply_song(s, merge);
        }
        if finished {
            self.doc.end_gesture();
        }
    }

    fn automation_list(&mut self, ui: &mut egui::Ui) {
        let lanes: Vec<_> = self
            .doc
            .project
            .song
            .automation
            .iter()
            .map(|l| (l.param, l.points.len()))
            .collect();
        if lanes.is_empty() {
            return;
        }
        ui.separator();
        ui.label(egui::RichText::new("Automation").strong());
        for (param, points) in lanes {
            ui.horizontal(|ui| {
                ui.label(format!("{}  ({points} steps)", param_label(param)));
                if ui.button("Clear").clicked() {
                    let mut s = self.doc.project.song.clone();
                    song::clear_automation(&mut s, param);
                    self.apply_song(s, false);
                }
            });
        }
        if ui.button("Clear all automation").clicked() {
            let mut s = self.doc.project.song.clone();
            s.automation.clear();
            self.apply_song(s, false);
        }
    }
}
