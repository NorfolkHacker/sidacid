//! The Pad Kit panel: kit knobs, the 4×4 pad grid, the selected pad's
//! row (name, loads, layer tabs and ranges), its waveform and knobs, and
//! the pattern row. The hits are edited in the kit's lane in the dock.

use super::{SidAcidApp, theme};
use acid_engine::host::Command;
use acid_engine::machine::{MachineType, MachineUid};
use acid_engine::padkit::pad::Pad;
use acid_engine::padkit::{MAX_LAYERS, PADS};
use acid_engine::params::{MachineParam, PadKitParam, PadParam};
use acid_engine::project::{PatternRef, TICKS_PER_STEP};
use eframe::egui::{self, Rect, Stroke, StrokeKind};

const PAD_KNOBS: [(PadParam, &str); 11] = [
    (PadParam::Tune, "Tune"),
    (PadParam::Decay, "Decay"),
    (PadParam::Level, "Level"),
    (PadParam::Pan, "Pan"),
    (PadParam::Cutoff, "Cutoff"),
    (PadParam::Resonance, "Res"),
    (PadParam::Start, "Start"),
    (PadParam::End, "End"),
    (PadParam::Reverse, "Reverse"),
    (PadParam::Mode, "Mode"),
    (PadParam::Group, "Mute"),
];
/// The audition velocity of a click on a pad.
const AUDITION_VEL: u8 = 100;

/// The pad at grid `row` (0 = top) and `col`: pad 1 is bottom left, pad
/// 13 top left.
pub(crate) fn grid_pad(row: usize, col: usize) -> usize {
    (3 - row.min(3)) * 4 + col.min(3)
}

/// A pad's button text: its number, then the first 8 characters of its
/// label and a dot per layer, or "·" when it is empty.
fn pad_text(pad: &Pad, i: usize) -> String {
    if pad.layers.is_empty() {
        return format!("{}\n·", i + 1);
    }
    let name: String = pad.label(i).chars().take(8).collect();
    format!("{}\n{name}\n{}", i + 1, "●".repeat(pad.layers.len()))
}

impl SidAcidApp {
    pub(crate) fn padkit_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{}   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label()
        );
        self.machine_panel(ui, uid, "padkit", title, |app, ui| {
            ui.horizontal_wrapped(|ui| {
                app.param_knob(ui, uid.param(PadKitParam::Volume), "Volume");
                app.param_knob(ui, uid.param(PadKitParam::Accent), "Accent");
                if ui.button("Save kit…").clicked() {
                    app.save_kit_dialog(uid);
                }
                if ui.button("Load kit…").clicked() {
                    app.load_kit_dialog(uid);
                }
            });
            let rects = app.padkit_grid(ui, uid);
            // Drops are resolved after the rack draws (`route_drop`).
            app.view.pad_grids.push((uid, rects));
            app.padkit_pad_row(ui, uid);
            app.padkit_waveform(ui, uid);
            let p = app.view.panel(uid).pad.min(PADS - 1) as u8;
            ui.horizontal_wrapped(|ui| {
                for (param, label) in PAD_KNOBS {
                    app.param_knob(ui, uid.param(MachineParam::Pad(p, param)), label);
                }
            });
            app.note_pattern_row(ui, uid);
        });
        self.release_pad_note(ui);
    }

    /// Pads sounding now: the panel's own audition, and the notes of the
    /// shown pattern under the playhead.
    fn sounding_pads(&self, uid: MachineUid) -> [bool; PADS] {
        let mut on = [false; PADS];
        if let Some((u, key)) = self.view.pad_note
            && u == uid
            && let Some(s) = on.get_mut(key as usize)
        {
            *s = true;
        }
        let slot = self.selected(uid);
        if let Some(step) = self.playhead(uid, slot)
            && let Some(p) = self.doc.project.note_patterns(uid)
        {
            let tick = step as u32 * TICKS_PER_STEP as u32;
            for n in &p[slot as usize].notes {
                let (a, len) = (n.start as u32, n.len as u32);
                if (a..a + len).contains(&tick)
                    && let Some(s) = on.get_mut(n.pitch as usize)
                {
                    *s = true;
                }
            }
        }
        on
    }

    /// The 4×4 grid; returns each pad's visible rect for file drops.
    fn padkit_grid(&mut self, ui: &mut egui::Ui, uid: MachineUid) -> [Rect; PADS] {
        let mut rects = [Rect::NOTHING; PADS];
        let sounding = self.sounding_pads(uid);
        egui::Grid::new(("padkit_pads", uid))
            .spacing([3.0, 3.0])
            .show(ui, |ui| {
                for row in 0..4 {
                    for col in 0..4 {
                        let i = grid_pad(row, col);
                        let Some(pad) = self.doc.project.padkit(uid).map(|k| &k.pads[i]) else {
                            return;
                        };
                        let text = egui::RichText::new(pad_text(pad, i)).size(11.0);
                        let empty = pad.layers.is_empty();
                        let r = ui.add_sized(
                            [76.0, 48.0],
                            egui::Button::new(text).selected(self.view.panel(uid).pad == i),
                        );
                        rects[i] = r.rect.intersect(ui.clip_rect());
                        if sounding[i] {
                            ui.painter().rect_stroke(
                                r.rect,
                                3.0,
                                Stroke::new(2.0, theme::ACID),
                                StrokeKind::Inside,
                            );
                        }
                        if r.clicked() {
                            self.select_kit_pad(uid, i);
                        }
                        if !empty
                            && r.is_pointer_button_down_on()
                            && ui.input(|inp| inp.pointer.primary_down())
                            && self.view.pad_note.is_none()
                        {
                            self.send(vec![Command::NoteOn {
                                uid,
                                pitch: i as u8,
                                vel: AUDITION_VEL,
                            }]);
                            self.view.pad_note = Some((uid, i as u8));
                        }
                    }
                    ui.end_row();
                }
            });
        rects
    }

    /// Selects pad `i` (and its first layer), and focuses the kit.
    fn select_kit_pad(&mut self, uid: MachineUid, i: usize) {
        let panel = self.view.panel(uid);
        if panel.pad != i {
            panel.pad = i;
            panel.pad_layer = 0;
        }
        self.view.focus = Some(uid);
    }

    fn padkit_pad_row(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let p = self.view.panel(uid).pad.min(PADS - 1);
        let layer = self.view.panel(uid).pad_layer.min(MAX_LAYERS - 1);
        // Only what the row shows: the pad's audio is not copied.
        let Some((label, layers, range)) = self.doc.project.padkit(uid).map(|k| {
            let pad = &k.pads[p];
            let range = pad.layers.get(layer).map(|l| (l.vel_lo, l.vel_hi));
            (pad.label(p), pad.layers.len(), range)
        }) else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("Pad {}", p + 1)).strong());
            self.pad_name_field(ui, uid, p, &label);
            if ui.button("Load…").clicked() {
                self.load_pad_dialog(uid, p);
            }
            let has = layer < layers;
            if ui
                .add_enabled(has, egui::Button::new("Clear"))
                .on_hover_text("Clear the selected layer")
                .clicked()
            {
                self.clear_pad_layer(uid, p, layer);
            }
            if ui
                .add_enabled(layers > 0, egui::Button::new("Clear pad"))
                .clicked()
            {
                self.clear_pad(uid, p);
            }
            ui.separator();
            for t in 0..MAX_LAYERS {
                let r = ui.add_enabled(
                    t < layers,
                    egui::Button::new(format!("{}", t + 1)).selected(t == layer && has),
                );
                if r.clicked() {
                    self.view.panel(uid).pad_layer = t;
                }
            }
            let Some((mut lo, mut hi)) = range else {
                ui.weak("Empty — load or drop audio files (WAV, FLAC, MP3, OGG).");
                return;
            };
            let r1 = ui.add(egui::DragValue::new(&mut lo).range(1..=127).prefix("Vel "));
            let r2 = ui.add(egui::DragValue::new(&mut hi).range(1..=127).prefix("– "));
            // `set_layer_range` keeps lo ≤ hi by moving lo, so a low end
            // dragged past the top stops at it.
            if r1.changed() {
                self.set_layer_range(uid, p, layer, lo.min(hi), hi, r1.dragged());
            }
            if r2.changed() {
                self.set_layer_range(uid, p, layer, lo, hi, r2.dragged());
            }
            if r1.drag_stopped() || r1.lost_focus() || r2.drag_stopped() || r2.lost_focus() {
                self.doc.end_gesture();
            }
        });
    }

    /// The pad's name, edited in place: Enter or clicking away renames,
    /// Escape cancels. A click on another pad still renames the one typed in.
    fn pad_name_field(&mut self, ui: &mut egui::Ui, uid: MachineUid, p: usize, label: &str) {
        let typed = self.view.pad_name.clone().filter(|(u, ..)| *u == uid);
        let mut text = match &typed {
            Some((_, i, t)) if *i == p => t.clone(),
            _ => label.to_string(),
        };
        let r = ui.add(
            egui::TextEdit::singleline(&mut text)
                .id_salt(("pad_name", uid))
                .desired_width(120.0),
        );
        if r.lost_focus() {
            self.view.pad_name = None;
            let Some((_, i, t)) = typed else {
                return;
            };
            let t = if i == p { text } else { t };
            let old = self.doc.project.padkit(uid).map(|k| k.pads[i].label(i));
            if !ui.input(|inp| inp.key_pressed(egui::Key::Escape))
                && old.is_some_and(|o| o != t.trim())
            {
                self.rename_pad(uid, i, &t);
            }
        } else if r.has_focus() {
            self.view.pad_name = Some((uid, p, text));
        }
    }
}

/// Whether `ty` takes dropped audio files.
pub(crate) fn takes_drops(ty: MachineType) -> bool {
    matches!(ty, MachineType::Sampler | MachineType::PadKit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::machine::MachineType;
    use acid_engine::project::Project;

    #[test]
    fn grid_positions_put_pad_one_bottom_left() {
        assert_eq!(grid_pad(3, 0), 0); // bottom row, first column
        assert_eq!(grid_pad(0, 0), 12); // top row, first column
        assert_eq!(grid_pad(0, 3), 15);
    }

    #[test]
    fn the_panel_draws_with_pads_loaded_and_empty() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        app.load_pad_bytes(
            uid,
            0,
            "kick",
            Some("wav"),
            &crate::ui::tests::test_wav(0.1),
        );
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
        }
        assert!(app.view.wave_cache.contains_key(&(uid, 0)), "the waveform");
        let before = app.doc.project.clone();
        app.view.panel(uid).pad = 5;
        crate::ui::run_frames_in(&ctx, &mut app, 2);
        assert_eq!(app.doc.project, before, "drawing changes nothing");
    }

    #[test]
    fn pads_show_number_name_and_a_dot_per_layer() {
        let mut pad = Pad::default();
        assert_eq!(pad_text(&pad, 0), "1\n·");
        let layer = acid_engine::padkit::pad::Layer::new("Kickdrum01", 44_100, 1, &[0.1; 10]);
        pad.layers = vec![layer.clone(), layer];
        assert_eq!(pad_text(&pad, 12), "13\nKickdrum\n●●");
    }

    #[test]
    fn auditions_and_notes_at_the_playhead_light_their_pads() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        let lit = |app: &SidAcidApp| -> Vec<usize> {
            let on = app.sounding_pads(uid);
            (0..PADS).filter(|&i| on[i]).collect()
        };
        app.view.pad_note = Some((uid, 3));
        assert_eq!(lit(&app), [3]);
        app.view.pad_note = None;
        // Pad 7 from step 2 for two steps, in the shown pattern 0.
        app.doc.project.note_patterns_mut(uid).unwrap()[0]
            .notes
            .push(acid_engine::project::Note::new(
                2 * TICKS_PER_STEP,
                2 * TICKS_PER_STEP,
                7,
                100,
            ));
        let n = app.doc.project.machines.len();
        app.status.playing = true;
        app.shown.p.patterns = vec![0; n];
        for (step, want) in [(1, &[][..]), (2, &[7]), (3, &[7]), (4, &[])] {
            app.shown.p.playhead = vec![step; n];
            assert_eq!(lit(&app), want, "step {step}");
        }
    }

    fn wav_file(name: &str) -> egui::DroppedFile {
        egui::DroppedFile {
            name: name.into(),
            bytes: Some(crate::ui::tests::test_wav(0.1).into()),
            ..Default::default()
        }
    }

    fn sampler_is_empty(app: &SidAcidApp) -> bool {
        let s = app.doc.project.sampler(crate::test_rack::SAMPLER).unwrap();
        s.slots.iter().all(Option::is_none)
    }

    #[test]
    fn a_drop_over_the_pad_grid_lands_on_the_pad_under_the_pointer() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        app.view.focus = Some(crate::test_rack::SAMPLER);
        let ctx = egui::Context::default();
        crate::ui::run_frames_in(&ctx, &mut app, 2);
        let (_, rects) = app.view.pad_grids.iter().find(|(u, _)| *u == uid).unwrap();
        let at = rects[4].center();
        let input = egui::RawInput {
            events: vec![egui::Event::PointerMoved(at)],
            dropped_files: vec![wav_file("tom.wav")],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
        assert_eq!(app.doc.project.padkit(uid).unwrap().pads[4].label(4), "tom");
        assert!(sampler_is_empty(&app), "not the focused sampler");
    }

    #[test]
    fn a_drop_with_no_position_and_nothing_focused_goes_to_the_first_sampler() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        app.move_machine(uid, 0);
        app.view.focus = None;
        let input = egui::RawInput {
            dropped_files: vec![wav_file("hat.wav")],
            ..Default::default()
        };
        let _ = egui::Context::default().run(input, |ctx| app.ui(ctx));
        assert!(!sampler_is_empty(&app), "the sampler, as before");
        let pads = &app.doc.project.padkit(uid).unwrap().pads;
        assert!(pads.iter().all(|p| p.layers.is_empty()));
    }

    #[test]
    fn a_drop_on_the_focused_kit_loads_from_the_selected_pad() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        app.view.focus = Some(uid);
        assert_eq!(app.drop_target(), Some(uid), "not the sampler");
        app.view.panel(uid).pad = 9;
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            dropped_files: vec![egui::DroppedFile {
                name: "snare.wav".into(),
                bytes: Some(crate::ui::tests::test_wav(0.1).into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
        let pads = &app.doc.project.padkit(uid).unwrap().pads;
        assert_eq!(pads[9].label(9), "snare");
        let sampler = app
            .doc
            .project
            .machines
            .iter()
            .find(|m| m.machine_type() == MachineType::Sampler);
        if let Some(m) = sampler {
            let s = app.doc.project.sampler(m.uid).unwrap();
            assert!(
                s.slots.iter().all(Option::is_none),
                "the sampler took nothing"
            );
        }
    }

    #[test]
    fn a_marker_drag_sets_the_start_param_and_keeps_it_before_end() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        let end = uid.param(MachineParam::Pad(0, PadParam::End));
        app.set_pad_marker(uid, 0, PadParam::End, 0.4);
        app.set_pad_marker(uid, 0, PadParam::Start, 0.9);
        let start = uid.param(MachineParam::Pad(0, PadParam::Start));
        assert!(app.doc.project.param(start) <= app.doc.project.param(end));
    }
}
