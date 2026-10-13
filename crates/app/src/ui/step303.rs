//! The classic 303 step editor: one column per step; rows for note,
//! octave down/up, accent, slide and gate.

use super::step_edit::{
    ScrollGesture, StepWriter, WriteAction, gate_label, key_to_write, nudge_note, scroll_notches,
    step_label, toggle_row,
};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::project::Gate;
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
        synth: usize,
        slot: u8,
        ops: Vec<(usize, StepOp)>,
        now: f64,
    ) {
        for (step, op) in ops {
            let current = self.doc.project.synths[synth].patterns[slot as usize].steps[step];
            let (value, scroll) = match op {
                StepOp::Toggle(row) => (toggle_row(row, current), false),
                StepOp::Nudge(n) => (nudge_note(current, n), true),
            };
            let cmds = self.doc.apply(
                Edit::SynthStep {
                    synth,
                    slot,
                    step,
                    value,
                },
                scroll,
            );
            self.send(cmds);
            self.view.focus = synth;
            if scroll {
                self.view.scroll_at[synth] = Some(ScrollGesture {
                    last: now,
                    serial: Some(self.doc.edit_serial()),
                });
            }
        }
    }

    /// The wheel moved over a note cell at `now` without a whole notch.
    fn note_scroll_without_edit(&mut self, synth: usize, now: f64) {
        let serial = self.view.scroll_at[synth].and_then(|g| g.serial);
        self.view.scroll_at[synth] = Some(ScrollGesture { last: now, serial });
    }

    /// Ends wheel gestures (303 pitch, knobs) once the wheel has rested.
    /// A gesture's undo step is closed only if no other edit came after
    /// it, so another open gesture is never cut short. Returns the time
    /// until the next gesture ends, if one is still open.
    pub(crate) fn tick_scroll_gestures(&mut self, now: f64) -> Option<f64> {
        let mut next: Option<f64> = None;
        let mut wait = |left: f64| next = Some(next.map_or(left, |n: f64| n.min(left)));
        for synth in 0..2 {
            let Some(g) = self.view.scroll_at[synth] else {
                continue;
            };
            if let Some(left) = g.remaining(now) {
                wait(left);
            } else {
                self.view.scroll_at[synth] = None;
                self.view.scroll_accum[synth] = 0.0;
                self.close_scroll_step(g);
            }
        }
        if let Some((_, g)) = self.view.knob_scroll {
            if let Some(left) = g.remaining(now) {
                wait(left);
            } else {
                self.view.knob_scroll = None;
                self.close_scroll_step(g);
            }
        }
        next
    }

    /// Ends a wheel gesture's undo step, unless a later edit took over.
    pub(crate) fn close_scroll_step(&mut self, g: ScrollGesture) {
        if g.serial == Some(self.doc.edit_serial()) {
            self.doc.end_gesture();
        }
    }

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
                theme::note_grid_visuals(ui.visuals_mut());
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
            self.view.writers[synth] = Some(StepWriter::new(i));
            self.view.writers[1 - synth] = None;
            self.view.focus = synth;
        }
        let now = ui.input(|i| i.time);
        if scrolled {
            self.view.scroll_accum[synth] = accum;
            self.note_scroll_without_edit(synth, now);
        }
        self.apply_step_ops(synth, slot, edits, now);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

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
        let slot = app.doc.project.selected[0];
        let before = app.doc.project.synths[0].patterns[slot as usize].steps[2];
        app.apply_step_ops(
            0,
            slot,
            vec![(2, StepOp::Toggle(5)), (2, StepOp::Nudge(1))],
            0.0,
        );
        let after = app.doc.project.synths[0].patterns[slot as usize].steps[2];
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

    #[test]
    fn pitch_scrolls_close_together_are_one_undo_step() {
        let mut app = app();
        let slot = app.doc.project.selected[0];
        let start = app.doc.project.synths[0].patterns[slot as usize].steps[0];
        let frame = |app: &mut SidAcidApp, op: Option<StepOp>, now: f64| {
            if let Some(op) = op {
                app.apply_step_ops(0, slot, vec![(0, op)], now);
            }
            app.tick_scroll_gestures(now);
        };
        for t in [0.0, 0.1, 0.2] {
            frame(&mut app, Some(StepOp::Nudge(1)), t);
            frame(&mut app, None, t + 0.05);
        }
        frame(&mut app, None, 0.4);
        assert!(app.view.scroll_at[0].is_some(), "still open");
        frame(&mut app, None, 0.6);
        assert!(app.view.scroll_at[0].is_none());
        frame(&mut app, Some(StepOp::Nudge(1)), 1.6);
        frame(&mut app, None, 2.0);
        app.doc.undo();
        let mid = app.doc.project.synths[0].patterns[slot as usize].steps[0];
        assert_eq!(mid, nudge_note(start, 3), "three notches, one step");
        app.doc.undo();
        assert_eq!(
            app.doc.project.synths[0].patterns[slot as usize].steps[0],
            start
        );
        assert!(!app.doc.can_undo());
    }
}
