//! The transport bar: play/stop, tempo, swing, lo-fi, master, performance.

use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::params::{MasterParam, ParamId};
use acid_engine::sequencer::Mode;
use eframe::egui;

impl SidAcidApp {
    pub(crate) fn transport(&mut self, ui: &mut egui::Ui) {
        // Wraps onto a second row instead of clipping in narrow windows.
        ui.horizontal_wrapped(|ui| {
            self.file_menu(ui);
            ui.separator();
            ui.heading(egui::RichText::new("SidAcid").color(theme::ACID).strong());
            ui.separator();
            let playing = self.status.playing;
            if ui
                .add(egui::Button::new("Play").selected(playing))
                .clicked()
            {
                self.play();
            }
            if ui.button("Stop").clicked() {
                self.stop();
            }
            self.fill_button(ui);
            ui.separator();
            for (m, label) in [(Mode::Pattern, "Pattern"), (Mode::Song, "Song")] {
                if ui.selectable_label(self.view.mode == m, label).clicked() {
                    self.set_mode(m);
                }
            }
            let rec_text = egui::RichText::new("Rec").color(if self.view.record {
                theme::ACCENT
            } else {
                ui.visuals().text_color()
            });
            let rec = ui.add_enabled(
                self.view.mode == Mode::Song,
                egui::Button::new(rec_text).selected(self.view.record),
            );
            if rec
                .on_hover_text("Record pattern changes and knob moves into the song")
                .clicked()
            {
                self.set_record(!self.view.record);
            }
            if self.view.mode == Mode::Song {
                let step = if self.status.playing {
                    self.status.song_step
                } else {
                    // The step playback will start from (see `play`).
                    self.view.song_cursor.min(self.last_song_bar_start())
                };
                ui.label(format!("Bar {}  Step {}", step / 16 + 1, step % 16 + 1));
            }
            ui.separator();
            let shown_bpm = self.doc.project.bpm;
            let mut bpm = shown_bpm;
            let r = ui.add(
                egui::DragValue::new(&mut bpm)
                    .range(40.0..=300.0)
                    .speed(0.2)
                    .max_decimals(1)
                    .suffix(" BPM"),
            );
            // Widget rounding must not turn a plain redraw into an edit.
            if r.changed() && (bpm - shown_bpm).abs() > 1e-3 {
                self.edit_transport(Edit::Bpm(bpm));
            }
            if r.drag_stopped() || r.lost_focus() {
                self.doc.end_gesture();
            }
            let shown_swing = self.doc.project.swing * 100.0;
            let mut swing = shown_swing;
            let r = ui.add(
                egui::Slider::new(&mut swing, 0.0..=75.0)
                    .text("Swing %")
                    .clamping(egui::SliderClamping::Edits)
                    .max_decimals(0),
            );
            if r.changed() && (swing - shown_swing).abs() > 1e-3 {
                self.edit_transport(Edit::Swing(swing / 100.0));
            }
            if r.drag_stopped() || r.lost_focus() {
                self.doc.end_gesture();
            }
            ui.separator();
            self.param_knob(ui, ParamId::Master(MasterParam::LoFi), "Lo-Fi");
            self.param_knob(ui, ParamId::Master(MasterParam::Level), "Master");
            ui.separator();
            self.perf_readout(ui);
            if let Some(line) = &self.view.status_line {
                ui.separator();
                ui.label(egui::RichText::new(line).small());
            }
        });
    }
}

impl SidAcidApp {
    /// Fill is on: the button is held or latched.
    pub(crate) fn fill_on(&self) -> bool {
        self.view.fill_held || self.view.fill_latched
    }

    /// Fill: on while held; a Shift-click latches it on or off. Lit while
    /// on; the engine hears each change.
    fn fill_button(&mut self, ui: &mut egui::Ui) {
        let was = self.fill_on();
        let r = ui
            .add(egui::Button::new("Fill").selected(was))
            .on_hover_text("Hold to play fill notes; Shift-click to latch");
        self.view.fill_rect = Some(r.rect);
        let (pressed, down, shift) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.modifiers.shift,
            )
        });
        if pressed && r.hovered() {
            if shift {
                self.view.fill_latched = !self.view.fill_latched;
            } else {
                self.view.fill_held = true;
            }
        } else if self.view.fill_held && !down {
            self.view.fill_held = false;
        }
        let on = self.fill_on();
        if on != was {
            self.send(vec![Command::SetFill(on)]);
        }
    }

    /// A new engine starts with Fill off: tell it Fill is on, if it is.
    pub(crate) fn resend_fill(&mut self) {
        if self.fill_on() {
            self.send(vec![Command::SetFill(true)]);
        }
    }

    /// A tempo or swing edit: one undo step per drag, or part of the
    /// record pass while recording.
    pub(crate) fn edit_transport(&mut self, edit: Edit) {
        let cmds = if self.recording() {
            self.doc.record(edit)
        } else {
            self.doc.apply(edit, true)
        };
        self.send(cmds);
    }
}

#[cfg(test)]
mod tests {
    use crate::document::Document;
    use crate::ui::SidAcidApp;
    use acid_engine::host::Command;
    use acid_engine::project::Project;
    use eframe::egui;

    fn fills(cmds: &[Command]) -> Vec<bool> {
        cmds.iter()
            .filter_map(|c| match c {
                Command::SetFill(on) => Some(*on),
                _ => None,
            })
            .collect()
    }

    fn frame(app: &mut SidAcidApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
        // Held modifiers come from the events' own state.
        let modifiers = events.iter().find_map(|e| match e {
            egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
            _ => None,
        });
        let input = egui::RawInput {
            modifiers: modifiers.unwrap_or_default(),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
    }

    /// Moves to the Fill button, presses (with Shift if `shift`), runs a
    /// frame held down, releases; returns the fills each step sent.
    fn click_fill(
        app: &mut SidAcidApp,
        ctx: &egui::Context,
        time: f64,
        shift: bool,
    ) -> (Vec<bool>, Vec<bool>) {
        let at = app.view.fill_rect.expect("Fill was drawn").center();
        let modifiers = if shift {
            egui::Modifiers::SHIFT
        } else {
            egui::Modifiers::NONE
        };
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        app.flushed.clear();
        frame(app, ctx, time, vec![egui::Event::PointerMoved(at)]);
        frame(app, ctx, time + 0.05, vec![button(true)]);
        frame(app, ctx, time + 0.1, vec![]);
        let down = fills(&app.flushed);
        app.flushed.clear();
        frame(app, ctx, time + 0.15, vec![button(false)]);
        frame(app, ctx, time + 0.2, vec![]);
        (down, fills(&app.flushed))
    }

    #[test]
    fn the_fill_button_holds_and_latches() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        // Held: on while down, off on release.
        assert_eq!(
            click_fill(&mut app, &ctx, 1.0, false),
            (vec![true], vec![false])
        );
        assert!(!app.fill_on());
        // Shift-click latches: on, and the release keeps it.
        assert_eq!(click_fill(&mut app, &ctx, 2.0, true), (vec![true], vec![]));
        assert!(app.view.fill_latched && app.fill_on());
        // A plain press while latched changes nothing.
        assert_eq!(click_fill(&mut app, &ctx, 3.0, false), (vec![], vec![]));
        assert!(app.fill_on());
        // Shift-click again unlatches.
        assert_eq!(click_fill(&mut app, &ctx, 4.0, true), (vec![false], vec![]));
        assert!(!app.fill_on());
    }

    #[test]
    fn fill_is_sent_again_to_a_replaced_engine() {
        for (held, latched) in [(true, false), (false, true), (false, false)] {
            let mut app = SidAcidApp::headless(Document::new(Project::default()));
            app.view.fill_held = held;
            app.view.fill_latched = latched;
            app.load_document(Document::new(Project::default()));
            let want: &[bool] = if held || latched { &[true] } else { &[] };
            assert_eq!(
                fills(&app.pending),
                want,
                "load, held {held} latched {latched}"
            );
            app.pending.clear();
            app.reset_for_new_host();
            assert_eq!(
                fills(&app.pending),
                want,
                "new host, held {held} latched {latched}"
            );
        }
    }
}
