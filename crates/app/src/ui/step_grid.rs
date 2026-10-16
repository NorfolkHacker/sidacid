//! The acid synth's step grid: one column per 1/16 step; rows for note,
//! octave down/up, accent, slide and gate. A view of the pattern's notes
//! (`SynthGrid`); each edit changes only the notes under its step
//! (`set_synth_step`), so the dock's roll and the grid stay one pattern.

use super::scroll::{ScrollGesture, scroll_notches};
use super::step_edit::{
    StepWriter, WriteAction, gate_label, key_to_write, nudge_note, step_label, toggle_row,
};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::machine::{MachineType, MachineUid};
use acid_engine::steps::{Gate, SynthGrid, SynthStep, set_synth_step};
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
            let Some(current) = self.synth_grid(uid, slot).map(|g| g.steps[step]) else {
                return;
            };
            let (value, scroll) = match op {
                StepOp::Toggle(row) => (toggle_row(row, current), false),
                StepOp::Nudge(n) => (nudge_note(current, n), true),
            };
            self.set_step(uid, slot, step, value, scroll);
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

    /// Synth `uid`'s pattern `slot` on the step grid, if `uid` is a synth
    /// in the rack.
    pub(crate) fn synth_grid(&self, uid: MachineUid, slot: u8) -> Option<SynthGrid> {
        let pattern = &self.doc.project.synth(uid)?.patterns[slot as usize];
        Some(SynthGrid::from_roll(&pattern.roll))
    }

    /// Sets one step of synth `uid`'s pattern `slot` (`merge`: into the
    /// open wheel gesture). An open note tool on the pattern is cancelled
    /// first, so the edit lands on the pattern as it was.
    fn set_step(&mut self, uid: MachineUid, slot: u8, step: usize, value: SynthStep, merge: bool) {
        self.tool_yield(uid, slot);
        let Some(synth) = self.doc.project.synth(uid) else {
            return;
        };
        let mut roll = synth.patterns[slot as usize].roll.clone();
        if set_synth_step(&mut roll, step, value) {
            let cmds = self.doc.apply(
                Edit::NotePattern {
                    uid,
                    slot,
                    pattern: Box::new(roll),
                },
                merge,
            );
            self.send(cmds);
        }
        self.view.focus = Some(uid);
    }

    pub(crate) fn step_grid(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let Some(pattern) = self.synth_grid(uid, slot) else {
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
            .id_salt(("step_grid", uid))
            .show(ui, |ui| {
                egui::Grid::new(("step_grid_cells", uid))
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
            // One step-writer at a time, and the keys are its, not the dock's.
            for panel in self.view.panels.values_mut() {
                panel.writer = None;
            }
            self.view.panel(uid).writer = Some(StepWriter::new(i));
            self.view.dock.keyboard = false;
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

    /// The keys are the step-writer's: one is active and nothing else
    /// (a text field, a dialog, the dock) has the keyboard.
    pub(crate) fn step_writing(&self, ctx: &egui::Context) -> bool {
        self.step_writing_synth().is_some()
            && !self.view.dock.keyboard
            && self.view.confirm.is_none()
            && self.view.message.is_none()
            && !ctx.wants_keyboard_input()
    }

    /// Feeds key presses to the active step-writer, if any, and takes the
    /// used ones out of the frame so no widget or shortcut acts on them.
    /// The dock taking the keyboard stops the writer.
    pub(crate) fn handle_step_write(&mut self, ctx: &egui::Context) {
        let project = &self.doc.project;
        let dock = self.view.dock.keyboard;
        for (uid, panel) in &mut self.view.panels {
            if dock || project.synth(*uid).is_none() {
                panel.writer = None;
            }
        }
        if !self.step_writing(ctx) {
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
        let mut used = Vec::new();
        for key in keys {
            let Some(write_key) = key_to_write(key) else {
                continue;
            };
            used.push(key);
            let slot = self.selected(uid);
            let Some(mut writer) = self.view.panel(uid).writer else {
                break;
            };
            let Some(grid) = self.synth_grid(uid, slot) else {
                break;
            };
            match writer.handle(write_key, &grid) {
                WriteAction::Set { step, value } => {
                    self.view.panel(uid).writer = Some(writer);
                    self.set_step(uid, slot, step, value, false);
                }
                WriteAction::Moved => self.view.panel(uid).writer = Some(writer),
                WriteAction::Exit => {
                    self.view.panel(uid).writer = None;
                    break;
                }
            }
        }
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(e, egui::Event::Key { key, modifiers, .. }
                    if !modifiers.command && used.contains(key))
            })
        });
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

    /// A press and release of `key` (egui counts a press of a key still
    /// held as a repeat).
    fn tap(key: egui::Key) -> [egui::Event; 2] {
        [true, false].map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
    }

    /// One whole app frame with `events`.
    fn frame(app: &mut SidAcidApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
    }

    /// Play was sent (queued or flushed).
    fn played(app: &SidAcidApp) -> bool {
        let play = |c: &acid_engine::host::Command| matches!(c, acid_engine::host::Command::Play);
        app.pending.iter().chain(&app.flushed).any(play)
    }

    fn writing_app() -> SidAcidApp {
        use acid_engine::project::Project;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.view.panel(SYNTH1).writer = Some(StepWriter::new(0));
        app
    }

    #[test]
    fn the_step_writer_types_notes_and_space_rests_instead_of_playing() {
        use acid_engine::project::Note;
        use acid_engine::steps::ACID_VEL;
        let mut app = writing_app();
        let ctx = egui::Context::default();
        frame(
            &mut app,
            &ctx,
            [egui::Key::Z, egui::Key::Space, egui::Key::C]
                .into_iter()
                .flat_map(tap)
                .collect(),
        );
        let slot = app.selected(SYNTH1);
        assert_eq!(
            app.doc.project.synth(SYNTH1).unwrap().patterns[slot as usize]
                .roll
                .notes,
            vec![Note::new(0, 4, 36, ACID_VEL), Note::new(8, 4, 40, ACID_VEL)]
        );
        assert_eq!(app.view.panel(SYNTH1).writer.map(|w| w.cursor), Some(3));
        assert!(!played(&app), "Space was the writer's rest");
        frame(&mut app, &ctx, tap(egui::Key::Escape).into());
        assert!(app.view.panel(SYNTH1).writer.is_none());
        frame(&mut app, &ctx, tap(egui::Key::Space).into());
        assert!(played(&app), "with the writer stopped Space plays again");
    }

    #[test]
    fn the_dock_taking_the_keyboard_stops_the_step_writer() {
        let mut app = writing_app();
        app.view.dock.keyboard = true;
        frame(
            &mut app,
            &egui::Context::default(),
            tap(egui::Key::Z).into(),
        );
        assert!(app.view.panel(SYNTH1).writer.is_none());
        let slot = app.selected(SYNTH1);
        assert!(
            app.doc.project.synth(SYNTH1).unwrap().patterns[slot as usize]
                .roll
                .notes
                .iter()
                .all(|n| n.start != 0 || n.pitch != 36),
            "the key was not the writer's"
        );
    }

    #[test]
    fn a_gate_click_and_a_scroll_on_one_step_in_one_frame_keep_both() {
        let mut app = app();
        let slot = app.doc.project.machine(SYNTH1).unwrap().selected;
        let grid = |app: &SidAcidApp| app.synth_grid(SYNTH1, slot).unwrap().steps[2];
        let before = grid(&app);
        let mut gate_only = self::app();
        gate_only.apply_step_ops(SYNTH1, slot, vec![(2, StepOp::Toggle(5))], 0.0);
        app.apply_step_ops(
            SYNTH1,
            slot,
            vec![(2, StepOp::Toggle(5)), (2, StepOp::Nudge(1))],
            0.0,
        );
        let after = grid(&app);
        assert_eq!(
            after.gate,
            super::super::step_edit::cycle_gate(before.gate),
            "the gate change stays"
        );
        assert_eq!(
            after,
            nudge_note(grid(&gate_only), 1),
            "and so does the pitch"
        );
    }
}
