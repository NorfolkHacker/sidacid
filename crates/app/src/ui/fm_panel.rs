//! The FM panel: algorithm diagram, global knobs, operator editor with
//! envelope sketch, LFO / pitch EG / effects, the voice browser, and the
//! pattern row with the poly piano roll.

use super::alg_diagram::{env_points, layout};
use super::fm_voices::{FmBrowser, starter};
use super::{SidAcidApp, theme};
use acid_engine::fm::algorithms::ALGORITHMS;
use acid_engine::machine::MachineUid;
use acid_engine::params::{FmParam, MachineParam, OpParam, ParamId, fm_steps, step_index};
use acid_engine::project::{FmMachine, PatternRef};
use eframe::egui::{self, Align2, FontId, Sense, Stroke, pos2, vec2};

const TOP: [(FmParam, &str); 8] = [
    (FmParam::Algorithm, "Alg"),
    (FmParam::Feedback, "Fdbk"),
    (FmParam::OscSync, "Sync"),
    (FmParam::Transpose, "Trans"),
    (FmParam::Mono, "Mono"),
    (FmParam::Glide, "Glide"),
    (FmParam::Vintage, "Vintage"),
    (FmParam::Volume, "Volume"),
];
const LFO: [(FmParam, &str); 7] = [
    (FmParam::LfoWave, "Wave"),
    (FmParam::LfoSpeed, "Speed"),
    (FmParam::LfoDelay, "Delay"),
    (FmParam::LfoPmd, "P.Depth"),
    (FmParam::LfoAmd, "A.Depth"),
    (FmParam::LfoSync, "Key Sync"),
    (FmParam::PitchModSens, "PMS"),
];
const PITCH_EG: [(FmParam, &str); 8] = [
    (FmParam::PitchR1, "P.R1"),
    (FmParam::PitchR2, "P.R2"),
    (FmParam::PitchR3, "P.R3"),
    (FmParam::PitchR4, "P.R4"),
    (FmParam::PitchL1, "P.L1"),
    (FmParam::PitchL2, "P.L2"),
    (FmParam::PitchL3, "P.L3"),
    (FmParam::PitchL4, "P.L4"),
];
const FX: [(FmParam, &str); 11] = [
    (FmParam::FilterOn, "Filter"),
    (FmParam::FilterMode, "Mode"),
    (FmParam::Cutoff, "Cutoff"),
    (FmParam::Resonance, "Reso"),
    (FmParam::ChorusRate, "Ch.Rate"),
    (FmParam::ChorusDepth, "Ch.Depth"),
    (FmParam::ChorusMix, "Chorus"),
    (FmParam::PhaserRate, "Ph.Rate"),
    (FmParam::PhaserDepth, "Ph.Depth"),
    (FmParam::PhaserFeedback, "Ph.Fdbk"),
    (FmParam::PhaserMix, "Phaser"),
];
const OP_KNOBS: [(OpParam, &str); 14] = [
    (OpParam::On, "On"),
    (OpParam::Level, "Level"),
    (OpParam::FixedFreq, "Mode"),
    (OpParam::Coarse, "Coarse"),
    (OpParam::Fine, "Fine"),
    (OpParam::Detune, "Detune"),
    (OpParam::VelSens, "Vel"),
    (OpParam::AmpModSens, "AMS"),
    (OpParam::RateScaling, "Rate Sc"),
    (OpParam::BreakPoint, "Break"),
    (OpParam::LeftDepth, "L.Depth"),
    (OpParam::RightDepth, "R.Depth"),
    (OpParam::LeftCurve, "L.Curve"),
    (OpParam::RightCurve, "R.Curve"),
];
const OP_ENV: [(OpParam, &str); 8] = [
    (OpParam::R1, "Attack"),
    (OpParam::R2, "Decay 1"),
    (OpParam::R3, "Decay 2"),
    (OpParam::R4, "Release"),
    (OpParam::L1, "Lvl 1"),
    (OpParam::L2, "Lvl 2"),
    (OpParam::L3, "Lvl 3"),
    (OpParam::L4, "Lvl 4"),
];
const CELL: f32 = 28.0;
const BOX: egui::Vec2 = vec2(22.0, 16.0);

impl SidAcidApp {
    /// FM machine `uid`; it must be an FM machine in the rack.
    fn fm_of(&self, uid: MachineUid) -> &FmMachine {
        self.doc.project.fm(uid).expect("an FM machine in the rack")
    }

    pub(crate) fn fm_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{}   {}   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label(),
            self.fm_of(uid).voice_name
        );
        self.machine_panel(ui, uid, "fm", title, |app, ui| {
            ui.horizontal_top(|ui| {
                app.alg_diagram(ui, uid);
                ui.horizontal_wrapped(|ui| {
                    for (p, label) in TOP {
                        app.param_knob(ui, uid.param(p), label);
                    }
                });
            });
            app.fm_operator_editor(ui, uid);
            egui::CollapsingHeader::new("LFO · Pitch EG · Effects")
                .id_salt(("fm_more", uid))
                .default_open(false)
                .open(app.view.forced_open("fm_more"))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (p, label) in LFO.iter().chain(PITCH_EG.iter()) {
                            app.param_knob(ui, uid.param(*p), label);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (p, label) in FX {
                            app.param_knob(ui, uid.param(p), label);
                        }
                    });
                });
            app.fm_voice_browser(ui, uid);
            app.note_pattern_row(ui, uid);
            let width = ui.available_width();
            app.lane(ui, uid, width);
        });
    }

    fn fm_value(&self, id: ParamId) -> f32 {
        self.doc.project.param(id)
    }

    /// "×1.00" for a ratio operator, "100Hz" for a fixed one.
    fn op_freq_text(&self, uid: MachineUid, op: usize) -> String {
        let id = |p| uid.param(MachineParam::FmOp(op as u8, p));
        let coarse = step_index(self.fm_value(id(OpParam::Coarse)), fm_steps::COARSE as u8) as f32;
        let fine = self.fm_value(id(OpParam::Fine)) * 99.0;
        if self.fm_value(id(OpParam::FixedFreq)) >= 0.5 {
            let hz = 10f32.powf(((coarse as usize & 3) as f32 * 100.0 + fine) / 100.0);
            format!("{hz:.0}Hz")
        } else {
            let ratio = if coarse == 0.0 { 0.5 } else { coarse };
            format!("×{:.2}", ratio * (1.0 + fine / 100.0))
        }
    }

    fn alg_diagram(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let index = step_index(
            self.fm_value(uid.param(FmParam::Algorithm)),
            fm_steps::ALGORITHM as u8,
        );
        let alg = ALGORITHMS[index];
        let pos = layout(&alg);
        let rows = pos.iter().map(|p| p.1).max().unwrap_or(0) as f32 + 1.0;
        let (rect, resp) =
            ui.allocate_exact_size(vec2(6.0 * CELL + 8.0, rows * CELL + 18.0), Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, theme::GRID_DARK);
        painter.text(
            rect.left_top() + vec2(4.0, 2.0),
            Align2::LEFT_TOP,
            format!("ALG {}", index + 1),
            FontId::monospace(10.0),
            theme::ACID,
        );
        let centre = |op: usize| {
            pos2(
                rect.left() + 4.0 + (pos[op].0 + 0.5) * CELL,
                rect.bottom() - 4.0 - (pos[op].1 as f32 + 0.5) * CELL,
            )
        };
        for target in 0..6 {
            for m in 0..6 {
                if alg.mods[target] & (1 << m) != 0 {
                    painter.line_segment(
                        [
                            centre(m) + vec2(0.0, BOX.y / 2.0),
                            centre(target) - vec2(0.0, BOX.y / 2.0),
                        ],
                        Stroke::new(1.5_f32, theme::KNOB_TRACK),
                    );
                }
            }
        }
        let fb = centre(alg.feedback);
        let loop_rect = egui::Rect::from_min_max(
            fb + vec2(BOX.x / 2.0, -BOX.y / 2.0 - 3.0),
            fb + vec2(BOX.x / 2.0 + 5.0, BOX.y / 2.0 - 3.0),
        );
        painter.rect_stroke(
            loop_rect,
            1.0,
            Stroke::new(1.0_f32, theme::SLIDE),
            egui::StrokeKind::Middle,
        );
        let mut clicked = None;
        for op in 0..6 {
            let r = egui::Rect::from_center_size(centre(op), BOX);
            let carrier = alg.carriers & (1 << op) != 0;
            painter.rect_filled(
                r,
                2.0,
                if carrier {
                    theme::ACID
                } else {
                    theme::KNOB_BODY
                },
            );
            if op == self.view.panel(uid).fm_op {
                painter.rect_stroke(
                    r,
                    2.0,
                    Stroke::new(2.0_f32, theme::ACCENT),
                    egui::StrokeKind::Outside,
                );
            }
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                format!("{}", op + 1),
                FontId::monospace(10.0),
                if carrier {
                    egui::Color32::BLACK
                } else {
                    egui::Color32::WHITE
                },
            );
            if resp.clicked()
                && let Some(p) = resp.interact_pointer_pos()
                && r.contains(p)
            {
                clicked = Some(op);
            }
        }
        if let Some(op) = clicked {
            self.view.panel(uid).fm_op = op;
        }
    }

    fn fm_operator_editor(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let id = |op, p| uid.param(MachineParam::FmOp(op, p));
        ui.horizontal(|ui| {
            for op in 0..6 {
                let text = format!("OP{} {}", op + 1, self.op_freq_text(uid, op));
                if ui
                    .selectable_label(self.view.panel(uid).fm_op == op, text)
                    .clicked()
                {
                    self.view.panel(uid).fm_op = op;
                }
            }
        });
        let op = self.view.panel(uid).fm_op.min(5) as u8;
        ui.horizontal_wrapped(|ui| {
            for (p, label) in OP_KNOBS {
                self.param_knob(ui, id(op, p), label);
            }
        });
        ui.horizontal_wrapped(|ui| {
            let v = |p| self.fm_value(id(op, p)) * 99.0;
            let rates = [
                v(OpParam::R1),
                v(OpParam::R2),
                v(OpParam::R3),
                v(OpParam::R4),
            ];
            let levels = [
                v(OpParam::L1),
                v(OpParam::L2),
                v(OpParam::L3),
                v(OpParam::L4),
            ];
            env_sketch(ui, rates, levels);
            for (p, label) in OP_ENV {
                self.param_knob(ui, id(op, p), label);
            }
        });
    }

    fn fm_voice_browser(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let has_bank = self.fm_of(uid).bank.is_some();
        // Undoing an import leaves no bank: show the starter list, so the
        // tab buttons and the grid agree.
        if !has_bank {
            self.view.panel(uid).fm_browser = FmBrowser::Starter;
        }
        ui.horizontal(|ui| {
            ui.label("Voices");
            if ui
                .selectable_label(
                    self.view.panel(uid).fm_browser == FmBrowser::Starter,
                    "Starter",
                )
                .clicked()
            {
                self.view.panel(uid).fm_browser = FmBrowser::Starter;
            }
            if ui
                .add_enabled(
                    has_bank,
                    egui::Button::new("Imported")
                        .selected(self.view.panel(uid).fm_browser == FmBrowser::Imported),
                )
                .clicked()
            {
                self.view.panel(uid).fm_browser = FmBrowser::Imported;
            }
            if ui
                .button("Import .syx…")
                .on_hover_text("Load a DX7 32-voice bank or single voice")
                .clicked()
            {
                self.import_syx(uid);
            }
            if ui
                .button("Randomise")
                .on_hover_text("A new random sound that always plays (undoable)")
                .clicked()
            {
                self.randomise_fm_voice(uid);
            }
        });
        let names: Vec<String> = match (self.view.panel(uid).fm_browser, &self.fm_of(uid).bank) {
            (FmBrowser::Imported, Some(bank)) => bank.iter().map(|v| v.name.clone()).collect(),
            _ => starter().iter().map(|v| v.name.clone()).collect(),
        };
        // Decision #11: the loaded voice is highlighted by where it was loaded
        // from, `(browser, index)` in the panel's `fm_voice_src`, not by name (duplicate
        // names in a bank all lit up). The source is KEPT across knob edits until
        // the next load (a tweaked voice is still "that voice, edited") and is
        // cleared on a document load, a single-voice import, an undo or redo of
        // a voice load, and (for an imported row) a bank import or its undo. As
        // a backstop it is trusted only while the project's voice name still
        // equals the source entry's name.
        let highlighted_at = highlighted_voice(
            self.view.panel(uid).fm_voice_src,
            self.view.panel(uid).fm_browser,
            &names,
            &self.fm_of(uid).voice_name,
        );
        let mut picked = None;
        egui::Grid::new(("fm_voices", uid))
            .spacing([4.0, 2.0])
            .show(ui, |ui| {
                for (i, name) in names.iter().enumerate() {
                    let current = highlighted_at == Some(i);
                    if ui.selectable_label(current, name.as_str()).clicked() {
                        picked = Some(i);
                    }
                    if i % 8 == 7 {
                        ui.end_row();
                    }
                }
            });
        if let Some(i) = picked {
            let voice = match (self.view.panel(uid).fm_browser, &self.fm_of(uid).bank) {
                (FmBrowser::Imported, Some(bank)) => bank[i].clone(),
                _ => starter()[i].clone(),
            };
            self.load_fm_voice(uid, &voice);
            let panel = self.view.panel(uid);
            panel.fm_voice_src = Some((panel.fm_browser, i));
        }
    }
}

/// Index of the browser row to highlight: the load source when it is in the
/// list being shown and its entry still carries the project's voice name.
fn highlighted_voice(
    src: Option<(FmBrowser, usize)>,
    shown: FmBrowser,
    names: &[String],
    voice_name: &str,
) -> Option<usize> {
    let (browser, i) = src?;
    (browser == shown && names.get(i).is_some_and(|n| n == voice_name)).then_some(i)
}

/// A small display-only drawing of an operator envelope.
fn env_sketch(ui: &mut egui::Ui, rates: [f32; 4], levels: [f32; 4]) {
    let (rect, _) = ui.allocate_exact_size(vec2(96.0, 44.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::GRID_DARK);
    let inner = rect.shrink(3.0);
    let points: Vec<egui::Pos2> = env_points(rates, levels)
        .iter()
        .map(|(x, y)| {
            pos2(
                inner.left() + x * inner.width(),
                inner.bottom() - y * inner.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.5_f32, theme::ACID)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_loaded_row_highlights_even_with_duplicate_names() {
        let names: Vec<String> = ["A", "B", "A"].map(String::from).to_vec();
        let src = Some((FmBrowser::Starter, 2));
        assert_eq!(
            highlighted_voice(src, FmBrowser::Starter, &names, "A"),
            Some(2)
        );
        assert_eq!(
            highlighted_voice(src, FmBrowser::Imported, &names, "A"),
            None
        );
        assert_eq!(
            highlighted_voice(src, FmBrowser::Starter, &names, "B"),
            None
        );
        assert_eq!(
            highlighted_voice(None, FmBrowser::Starter, &names, "A"),
            None
        );
        assert_eq!(
            highlighted_voice(
                Some((FmBrowser::Starter, 9)),
                FmBrowser::Starter,
                &names,
                "A"
            ),
            None
        );
    }
}
