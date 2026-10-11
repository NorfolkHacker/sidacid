//! A synth panel: 303 knobs, pattern pads, length/transpose, editor.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::params::{ParamId, SynthParam};
use acid_engine::project::PatternRef;
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
                    if ui
                        .add(
                            egui::DragValue::new(&mut length)
                                .range(1..=64)
                                .prefix("Len "),
                        )
                        .changed()
                    {
                        let cmds = self.doc.apply(
                            Edit::SynthLength {
                                synth,
                                slot,
                                length,
                            },
                            false,
                        );
                        self.send(cmds);
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut transpose)
                                .range(-12..=12)
                                .prefix("Trans "),
                        )
                        .changed()
                    {
                        let cmds = self.doc.apply(
                            Edit::SynthTranspose {
                                synth,
                                slot,
                                transpose,
                            },
                            false,
                        );
                        self.send(cmds);
                    }
                    if let Some(w) = self.view.writers[synth] {
                        ui.label(format!(
                            "Step-write: step {}  octave {:+}  (Esc to stop)",
                            w.cursor + 1,
                            w.octave
                        ));
                    }
                });
                self.step303(ui, synth);
            });
    }
}
