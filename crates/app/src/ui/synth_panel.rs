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
                egui::ScrollArea::horizontal()
                    .id_salt(("synth_pads", synth))
                    .show(ui, |ui| {
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
                                self.set_synth_length(synth, slot, length);
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
                                self.set_synth_transpose(synth, slot, transpose);
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
                    });
                ui.horizontal(|ui| {
                    use acid_engine::random::Dice;
                    ui.label("Dice");
                    let dice = [
                        (
                            "Notes",
                            Dice {
                                notes: true,
                                ..Dice::default()
                            },
                        ),
                        (
                            "Accent",
                            Dice {
                                accent: true,
                                ..Dice::default()
                            },
                        ),
                        (
                            "Slide",
                            Dice {
                                slide: true,
                                ..Dice::default()
                            },
                        ),
                        (
                            "Rest",
                            Dice {
                                rest: true,
                                ..Dice::default()
                            },
                        ),
                        (
                            "Octave",
                            Dice {
                                octave: true,
                                ..Dice::default()
                            },
                        ),
                        ("All", Dice::ALL),
                    ];
                    for (label, d) in dice {
                        if ui
                            .button(label)
                            .on_hover_text("Randomise the selected pattern (undoable)")
                            .clicked()
                        {
                            self.randomise_synth(synth, d);
                        }
                    }
                });
                match self.doc.project.synths[synth].view {
                    SeqView::Step303 => self.step303(ui, synth),
                    SeqView::PianoRoll => self.piano_roll(ui, synth),
                }
            });
    }
}

impl SidAcidApp {
    /// The Len field: one undo step per drag.
    pub(crate) fn set_synth_length(&mut self, synth: usize, slot: u8, length: u8) {
        let cmds = self.doc.apply(
            Edit::SynthLength {
                synth,
                slot,
                length,
            },
            true,
        );
        self.send(cmds);
        self.view.focus = synth;
    }

    /// The Trans field: one undo step per drag.
    pub(crate) fn set_synth_transpose(&mut self, synth: usize, slot: u8, transpose: i8) {
        let cmds = self.doc.apply(
            Edit::SynthTranspose {
                synth,
                slot,
                transpose,
            },
            true,
        );
        self.send(cmds);
        self.view.focus = synth;
    }

    /// A fresh seed per click (time mixed with a counter).
    pub(crate) fn next_seed(&mut self) -> u64 {
        self.view.seed_counter = self.view.seed_counter.wrapping_add(1);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        nanos ^ self.view.seed_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    pub(crate) fn randomise_synth(&mut self, synth: usize, dice: acid_engine::random::Dice) {
        let seed = self.next_seed();
        self.randomise_synth_seeded(synth, dice, seed);
    }

    /// Dice the selected pattern of `synth`, grooving with the selected
    /// pattern of the drum kit of the same number.
    pub(crate) fn randomise_synth_seeded(
        &mut self,
        synth: usize,
        dice: acid_engine::random::Dice,
        seed: u64,
    ) {
        let p = &self.doc.project;
        let slot = p.selected[synth];
        let groove = &p.drums[synth].patterns[p.selected[2 + synth] as usize];
        let pattern = acid_engine::random::randomise_synth(
            &p.synths[synth].patterns[slot as usize],
            dice,
            groove,
            seed,
        );
        let cmds = self.doc.apply(
            crate::document::Edit::SynthPattern {
                synth,
                slot,
                pattern: Box::new(pattern),
            },
            false,
        );
        self.send(cmds);
        self.view.focus = synth;
    }

    pub(crate) fn drum_template(&mut self, kit: usize) {
        let seed = self.next_seed();
        let slot = self.doc.project.selected[2 + kit];
        let length = self.doc.project.drums[kit].patterns[slot as usize].length;
        let pattern = acid_engine::random::drum_template(seed, length);
        let cmds = self.doc.apply(
            crate::document::Edit::DrumPattern {
                kit,
                slot,
                pattern: Box::new(pattern),
            },
            false,
        );
        self.send(cmds);
        self.view.focus = 2 + kit;
    }

    pub(crate) fn evolve_drums(&mut self, kit: usize) {
        let seed = self.next_seed();
        self.evolve_drums_seeded(kit, seed);
    }

    pub(crate) fn evolve_drums_seeded(&mut self, kit: usize, seed: u64) {
        let slot = self.doc.project.selected[2 + kit];
        let pattern = acid_engine::random::evolve_drums(
            &self.doc.project.drums[kit].patterns[slot as usize],
            seed,
        );
        let cmds = self.doc.apply(
            crate::document::Edit::DrumPattern {
                kit,
                slot,
                pattern: Box::new(pattern),
            },
            false,
        );
        self.send(cmds);
        self.view.focus = 2 + kit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    #[test]
    fn length_and_transpose_edits_focus_the_synth() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.focus = 4;
        let slot = app.doc.project.selected[1];
        app.set_synth_length(1, slot, 7);
        assert_eq!(app.view.focus, 1);
        app.view.focus = 4;
        app.set_synth_transpose(1, slot, -3);
        assert_eq!(app.view.focus, 1);
        let p = &app.doc.project.synths[1].patterns[slot as usize];
        assert_eq!((p.length, p.transpose), (7, -3));
    }
}
