//! The classic 303 step editor: one column per step; rows for note,
//! octave down/up, accent, slide and gate.

use super::scroll::ScrollGesture;
use super::step_edit::{
    StepWriter, WriteAction, gate_label, key_to_write, nudge_note, scroll_notches, step_label,
    toggle_row,
};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::machine::{MachineType, MachineUid};
use acid_engine::project::{Gate, SeqView, SynthPattern};
use eframe::egui;

const CELL: [f32; 2] = [28.0, 18.0];
const NOTE_TIP: &str = "Click: step-write from here  ·  Scroll: pitch  ·  Right-click: gate";
const ROWS: [&str; 6] = ["Note", "Down", "Up", "Acc", "Slide", "Gate"];

/// One click or scroll on a step, applied to the step as it is then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StepOp {
    /// A row click (see `toggle_row`).
    Toggle(usize),
    /// Scroll notches of pitch.
    Nudge(i32),
}

/// A mouse click on a note cell starts the step-writer. Space or Enter on
/// a focused cell (egui's keyboard click) must not: Space is the writer's
/// Rest key and would restart it at that cell.
fn starts_writer(r: &egui::Response) -> bool {
    r.clicked_by(egui::PointerButton::Primary)
}

impl SidAcidApp {
    /// Applies a frame's step clicks and pitch scrolls in order, each to
    /// the step as the previous one left it. Pitch scrolls at `now` keep
    /// one undo step open until the wheel rests (see `tick_scroll_gestures`).
    pub(crate) fn apply_step_ops(
        &mut self,
        uid: MachineUid,
        slot: u8,
        ops: Vec<(usize, StepOp)>,
        now: f64,
    ) {
        for (step, op) in ops {
            let Some(current) = self.synth_pattern(uid, slot).map(|p| p.steps[step]) else {
                return;
            };
            let (value, scroll) = match op {
                StepOp::Toggle(row) => (toggle_row(row, current), false),
                StepOp::Nudge(n) => (nudge_note(current, n), true),
            };
            let cmds = self.doc.apply(
                Edit::SynthStep {
                    uid,
                    slot,
                    step,
                    value,
                },
                scroll,
            );
            self.send(cmds);
            self.view.focus = Some(uid);
            if scroll {
                self.view.panel(uid).scroll_at = Some(ScrollGesture {
                    last: now,
                    serial: Some(self.doc.edit_serial()),
                });
            }
        }
    }

    /// The wheel moved over a note cell at `now` without a whole notch.
    fn note_scroll_without_edit(&mut self, uid: MachineUid, now: f64) {
        let panel = self.view.panel(uid);
        let serial = panel.scroll_at.and_then(|g| g.serial);
        panel.scroll_at = Some(ScrollGesture { last: now, serial });
    }

    /// Synth `uid`'s pattern `slot`, if `uid` is a synth in the rack.
    pub(crate) fn synth_pattern(&self, uid: MachineUid, slot: u8) -> Option<&SynthPattern> {
        Some(&self.doc.project.synth(uid)?.patterns[slot as usize])
    }

    pub(crate) fn step303(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let Some(pattern) = self.synth_pattern(uid, slot).cloned() else {
            return;
        };
        let len = pattern.length as usize;
        let playhead = self.playhead(uid, slot);
        let cursor = self.view.panel(uid).writer.map(|w| w.cursor);
        let mut edits = Vec::new();
        let mut start_writer = None;
        let mut scrolled = false;
        let mut accum = self.view.panel(uid).scroll_accum;
        egui::ScrollArea::horizontal()
            .id_salt(("s303", uid))
            .show(ui, |ui| {
                egui::Grid::new(("grid303", uid))
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
                                    if starts_writer(&r) {
                                        start_writer = Some(i);
                                    }
                                    if r.secondary_clicked() {
                                        edits.push((i, StepOp::Toggle(5)));
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
                                                edits.push((i, StepOp::Nudge(n)));
                                            }
                                        }
                                    }
                                    r.on_hover_text(NOTE_TIP);
                                } else if r.clicked() {
                                    edits.push((i, StepOp::Toggle(row)));
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(i) = start_writer {
            // One step-writer at a time.
            for panel in self.view.panels.values_mut() {
                panel.writer = None;
            }
            self.view.panel(uid).writer = Some(StepWriter::new(i));
            self.view.focus = Some(uid);
        }
        let now = ui.input(|i| i.time);
        if scrolled {
            self.view.panel(uid).scroll_accum = accum;
            self.note_scroll_without_edit(uid, now);
        }
        self.apply_step_ops(uid, slot, edits, now);
    }

    /// The synth whose step-writer is active, first in rack order.
    pub(crate) fn step_writing_synth(&self) -> Option<MachineUid> {
        self.doc
            .project
            .machines
            .iter()
            .filter(|m| m.machine_type() == MachineType::Synth)
            .map(|m| m.uid)
            .find(|uid| {
                self.view
                    .panels
                    .get(uid)
                    .is_some_and(|p| p.writer.is_some())
            })
    }

    /// Feeds key presses to the active step-writer, if any.
    pub(crate) fn handle_step_write(&mut self, ctx: &egui::Context) {
        let project = &self.doc.project;
        for (uid, panel) in &mut self.view.panels {
            if project.synth(*uid).map(|s| s.view) != Some(SeqView::Step303) {
                panel.writer = None;
            }
        }
        if self.view.confirm.is_some() || self.view.message.is_some() {
            return;
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let Some(uid) = self.step_writing_synth() else {
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
            let slot = self.selected(uid);
            let Some(mut writer) = self.view.panel(uid).writer else {
                return;
            };
            let Some(pattern) = self.synth_pattern(uid, slot) else {
                return;
            };
            let action = writer.handle(write_key, pattern);
            match action {
                WriteAction::Set { step, value } => {
                    self.view.panel(uid).writer = Some(writer);
                    let cmds = self.doc.apply(
                        Edit::SynthStep {
                            uid,
                            slot,
                            step,
                            value,
                        },
                        false,
                    );
                    self.send(cmds);
                }
                WriteAction::Moved => self.view.panel(uid).writer = Some(writer),
                WriteAction::Exit => {
                    self.view.panel(uid).writer = None;
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::test_rack::*;

    fn app() -> SidAcidApp {
        SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()))
    }

    /// Draws one button for a frame with `events`; returns its response
    /// flags (clicked, starts the writer).
    fn button_frame(ctx: &egui::Context, events: Vec<egui::Event>, focus: bool) -> (bool, bool) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut out = (false, false);
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = ui.button("C");
                if focus {
                    r.request_focus();
                }
                out = (r.clicked(), starts_writer(&r));
            });
        });
        out
    }

    #[test]
    fn space_on_a_focused_note_button_does_not_restart_the_writer() {
        let ctx = egui::Context::default();
        button_frame(&ctx, vec![], true);
        button_frame(&ctx, vec![], false);
        let space = egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let (clicked, starts) = button_frame(&ctx, vec![space], false);
        assert!(clicked, "egui turns Space on a focused button into a click");
        assert!(!starts, "but only a real mouse click starts the writer");
    }

    #[test]
    fn a_gate_click_and_a_scroll_on_one_step_in_one_frame_keep_both() {
        let mut app = app();
        let slot = app.doc.project.machine(SYNTH1).unwrap().selected;
        let before = app.doc.project.synth(SYNTH1).unwrap().patterns[slot as usize].steps[2];
        app.apply_step_ops(
            SYNTH1,
            slot,
            vec![(2, StepOp::Toggle(5)), (2, StepOp::Nudge(1))],
            0.0,
        );
        let after = app.doc.project.synth(SYNTH1).unwrap().patterns[slot as usize].steps[2];
        assert_eq!(
            after.gate,
            super::super::step_edit::cycle_gate(before.gate),
            "the gate change stays"
        );
        assert_eq!(
            after,
            nudge_note(toggle_row(5, before), 1),
            "and so does the pitch"
        );
    }
}
