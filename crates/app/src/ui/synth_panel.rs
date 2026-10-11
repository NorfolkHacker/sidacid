//! A synth panel: 303 knobs, pattern pads, length/transpose, editor.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::params::{ParamId, SynthParam};
use acid_engine::project::{PatternRef, SeqView};
use eframe::egui;

const KNOBS: [(SynthParam, &str); 15] = [
    (SynthParam::Tune, "Tune"),
    (SynthParam::Cutoff, "Cutoff"),
    (SynthParam::Resonance, "Reso"),
    (SynthParam::EnvMod, "Env Mod"),
    (SynthParam::Decay, "Decay"),
    (SynthParam::Accent, "Accent"),
    (SynthParam::Wave, "Wave"),
    (SynthParam::PulseWidth, "PW"),
    (SynthParam::FilterMode, "Mode"),
    (SynthParam::Sync, "Sync"),
    (SynthParam::SyncPitch, "Sync Pt"),
    (SynthParam::Unison, "Unison"),
    (SynthParam::Detune, "Detune"),
    (SynthParam::Spread, "Spread"),
    (SynthParam::Volume, "Volume"),
];

impl SidAcidApp {
    pub(crate) fn synth_panel(&mut self, ui: &mut egui::Ui, synth: usize) {
        let slot = self.doc.project.selected[synth];
        let title = format!("SYNTH {}   {}", synth + 1, PatternRef(slot).label());
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt(("synth", synth))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (p, label) in KNOBS {
                        self.param_knob(ui, ParamId::Synth(synth as u8, p), label);
                    }
                });
                ui.horizontal(|ui| {
                    self.pattern_pads(ui, synth);
                    ui.separator();
                    let pattern = &self.doc.project.synths[synth].patterns[slot as usize];
                    let mut length = pattern.length;
                    let mut transpose = pattern.transpose;
                    let r = ui.add(
                        egui::DragValue::new(&mut length)
                            .range(1..=64)
                            .prefix("Len "),
                    );
                    if r.changed() {
                        let cmds = self.doc.apply(
                            Edit::SynthLength {
                                synth,
                                slot,
                                length,
                            },
                            true,
                        );
                        self.send(cmds);
                    }
                    if r.drag_stopped() || r.lost_focus() {
                        self.doc.end_gesture();
                    }
                    let r2 = ui.add(
                        egui::DragValue::new(&mut transpose)
                            .range(-12..=12)
                            .prefix("Trans "),
                    );
                    if r2.changed() {
                        let cmds = self.doc.apply(
                            Edit::SynthTranspose {
                                synth,
                                slot,
                                transpose,
                            },
                            true,
                        );
                        self.send(cmds);
                    }
                    if r2.drag_stopped() || r2.lost_focus() {
                        self.doc.end_gesture();
                    }
                    ui.separator();
                    let view = self.doc.project.synths[synth].view;
                    for (v, label) in [
                        (SeqView::Step303, "303"),
                        (SeqView::PianoRoll, "Piano roll"),
                    ] {
                        if ui.selectable_label(view == v, label).clicked() && view != v {
                            let cmds = self.doc.apply(Edit::View { synth, view: v }, false);
                            self.send(cmds);
                            self.view.writers[synth] = None;
                        }
                    }
                    if let Some(w) = self.view.writers[synth] {
                        ui.label(format!(
                            "Step-write: step {}  octave {:+}  (Esc to stop)",
                            w.cursor + 1,
                            w.octave
                        ));
                    }
                });
                match self.doc.project.synths[synth].view {
                    SeqView::Step303 => self.step303(ui, synth),
                    SeqView::PianoRoll => self.piano_roll(ui, synth),
                }
            });
    }
}
