//! The classic 303 step editor: one column per step; rows for note,
//! octave down/up, accent, slide and gate.

use super::step_edit::{
    StepWriter, WriteAction, gate_label, key_to_write, nudge_note, scroll_notches, step_label,
    toggle_row,
};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::project::Gate;
use eframe::egui;

const CELL: [f32; 2] = [28.0, 18.0];
const NOTE_TIP: &str = "Click: step-write from here  ·  Scroll: pitch  ·  Right-click: gate";
const ROWS: [&str; 6] = ["Note", "Down", "Up", "Acc", "Slide", "Gate"];

impl SidAcidApp {
    pub(crate) fn step303(&mut self, ui: &mut egui::Ui, synth: usize) {
        let slot = self.doc.project.selected[synth];
        let pattern = self.doc.project.synths[synth].patterns[slot as usize].clone();
        let len = pattern.length as usize;
        let playhead = (self.status.playing && self.status.patterns[synth] == slot)
            .then_some(self.status.playhead[synth]);
        let cursor = self.view.writers[synth].map(|w| w.cursor);
        let mut edits = Vec::new();
        let mut start_writer = None;
        let mut scrolled = false;
        let mut accum = self.view.scroll_accum[synth];
        egui::ScrollArea::horizontal()
            .id_salt(("s303", synth))
            .show(ui, |ui| {
                egui::Grid::new(("grid303", synth))
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
                        for (row, name) in ROWS.iter().enumerate() {
                            ui.label(*name);
                            for (i, s) in pattern.steps.iter().take(len).enumerate() {
                                let (text, on) = match row {
                                    0 => (step_label(s), Some(i) == cursor || s.gate != Gate::Off),
                                    1 => ("v", s.octave < 0),
                                    2 => ("^", s.octave > 0),
                                    3 => ("A", s.accent),
                                    4 => ("S", s.slide),
                                    _ => (gate_label(s.gate), s.gate != Gate::Off),
                                };
                                let mut rich = egui::RichText::new(text).monospace();
                                if row == 0 && Some(i) == cursor {
                                    rich = rich.color(theme::QUEUED).strong();
                                } else if row == 3 && s.accent {
                                    rich = rich.color(theme::ACCENT);
                                } else if row == 4 && s.slide {
                                    rich = rich.color(theme::SLIDE);
                                }
                                let r = ui.add_sized(CELL, egui::Button::new(rich).selected(on));
                                if row == 0 {
                                    if r.clicked() {
                                        start_writer = Some(i);
                                    }
                                    if r.secondary_clicked() {
                                        edits.push((i, toggle_row(5, *s), false));
                                    }
                                    if r.hovered() {
                                        let d = ui.input_mut(|inp| {
                                            let d = inp.raw_scroll_delta.y;
                                            inp.raw_scroll_delta.y = 0.0;
                                            inp.smooth_scroll_delta.y = 0.0;
                                            d
                                        });
                                        if d != 0.0 {
                                            scrolled = true;
                                            let n = scroll_notches(&mut accum, d);
                                            if n != 0 {
                                                edits.push((i, nudge_note(*s, n), true));
                                            }
                                        }
                                    }
                                    r.on_hover_text(NOTE_TIP);
                                } else if r.clicked() {
                                    edits.push((i, toggle_row(row, *s), false));
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(i) = start_writer {
            self.view.writers[synth] = Some(StepWriter::new(i));
            self.view.writers[1 - synth] = None;
            self.view.focus = synth;
        }
        if scrolled {
            self.view.scrolling[synth] = true;
            self.view.scroll_accum[synth] = accum;
        } else if self.view.scrolling[synth] {
            self.view.scrolling[synth] = false;
            self.view.scroll_accum[synth] = 0.0;
            self.doc.end_gesture();
        }
        for (step, value, merge) in edits {
            let cmds = self.doc.apply(
                Edit::SynthStep {
                    synth,
                    slot,
                    step,
                    value,
                },
                merge,
            );
            self.send(cmds);
            self.view.focus = synth;
        }
    }

    /// Feeds key presses to the active step-writer, if any.
    pub(crate) fn handle_step_write(&mut self, ctx: &egui::Context) {
        for s in 0..2 {
            if self.doc.project.synths[s].view != acid_engine::project::SeqView::Step303 {
                self.view.writers[s] = None;
            }
        }
        if self.view.confirm.is_some() || self.view.message.is_some() {
            return;
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let Some(synth) = (0..2).find(|&s| self.view.writers[s].is_some()) else {
            return;
        };
        let keys: Vec<egui::Key> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } if !modifiers.command => Some(*key),
                    _ => None,
                })
                .collect()
        });
        for key in keys {
            let Some(write_key) = key_to_write(key) else {
                continue;
            };
            let slot = self.doc.project.selected[synth];
            let Some(mut writer) = self.view.writers[synth] else {
                return;
            };
            let action = writer.handle(
                write_key,
                &self.doc.project.synths[synth].patterns[slot as usize],
            );
            match action {
                WriteAction::Set { step, value } => {
                    self.view.writers[synth] = Some(writer);
                    let cmds = self.doc.apply(
                        Edit::SynthStep {
                            synth,
                            slot,
                            step,
                            value,
                        },
                        false,
                    );
                    self.send(cmds);
                }
                WriteAction::Moved => self.view.writers[synth] = Some(writer),
                WriteAction::Exit => {
                    self.view.writers[synth] = None;
                    return;
                }
            }
        }
    }
}
