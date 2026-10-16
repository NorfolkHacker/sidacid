//! A synth panel: 303 knobs, pattern pads, length/transpose, editor.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::machine::{MachineType, MachineUid};
use acid_engine::params::SynthParam;
use acid_engine::project::{DrumPattern, PatternRef, SynthMachine};
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
    /// Synth `uid`'s sound and patterns; it must be a synth in the rack.
    fn synth_of(&self, uid: MachineUid) -> &SynthMachine {
        self.doc.project.synth(uid).expect("a synth in the rack")
    }

    pub(crate) fn synth_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{}   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label()
        );
        self.machine_panel(ui, uid, "synth", title, |app, ui| {
            ui.horizontal_wrapped(|ui| {
                for (p, label) in KNOBS {
                    app.param_knob(ui, uid.param(p), label);
                }
            });
            egui::ScrollArea::horizontal()
                .id_salt(("synth_pads", uid))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        app.pattern_pads(ui, uid);
                        ui.separator();
                        let pattern = &app.synth_of(uid).patterns[slot as usize];
                        let mut length = pattern.roll.length;
                        let mut transpose = pattern.transpose;
                        let r = ui.add(
                            egui::DragValue::new(&mut length)
                                .range(1..=64)
                                .prefix("Len "),
                        );
                        if r.changed() {
                            app.set_synth_length(uid, slot, length, r.dragged());
                        }
                        if r.drag_stopped() || r.lost_focus() {
                            app.doc.end_gesture();
                        }
                        let r2 = ui.add(
                            egui::DragValue::new(&mut transpose)
                                .range(-12..=12)
                                .prefix("Trans "),
                        );
                        if r2.changed() {
                            app.set_synth_transpose(uid, slot, transpose, r2.dragged());
                        }
                        if r2.drag_stopped() || r2.lost_focus() {
                            app.doc.end_gesture();
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
                        app.randomise_synth(uid, d);
                    }
                }
            });
            app.poly_roll(ui, uid);
        });
    }
}

impl SidAcidApp {
    /// The Len field. `merge` is true only while dragging, so a drag is one
    /// undo step and each arrow key or typed value is its own.
    pub(crate) fn set_synth_length(&mut self, uid: MachineUid, slot: u8, length: u8, merge: bool) {
        let cmds = self
            .doc
            .apply(Edit::NoteLength { uid, slot, length }, merge);
        self.send(cmds);
        self.view.focus = Some(uid);
    }

    /// The Trans field. `merge` is true only while dragging, so a drag is one
    /// undo step and each arrow key or typed value is its own.
    pub(crate) fn set_synth_transpose(
        &mut self,
        uid: MachineUid,
        slot: u8,
        transpose: i8,
        merge: bool,
    ) {
        let cmds = self.doc.apply(
            Edit::SynthTranspose {
                uid,
                slot,
                transpose,
            },
            merge,
        );
        self.send(cmds);
        self.view.focus = Some(uid);
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

    pub(crate) fn randomise_synth(&mut self, uid: MachineUid, dice: acid_engine::random::Dice) {
        let seed = self.next_seed();
        self.randomise_synth_seeded(uid, dice, seed);
    }

    /// The drum kit a synth grooves with: the n-th kit in the rack for the
    /// n-th synth (Synth 1 with the 808, Synth 2 with the 909).
    fn groove_kit(&self, synth: MachineUid) -> Option<MachineUid> {
        let machines = &self.doc.project.machines;
        let of = |kit: bool| {
            machines.iter().filter(move |m| {
                let ty = m.machine_type();
                if kit {
                    matches!(ty, MachineType::Kit808 | MachineType::Kit909)
                } else {
                    ty == MachineType::Synth
                }
            })
        };
        let n = of(false).position(|m| m.uid == synth)?;
        of(true).nth(n).map(|m| m.uid)
    }

    /// Dice the selected pattern of synth `uid`, grooving with the selected
    /// pattern of its drum kit (`groove_kit`; none: an empty groove).
    pub(crate) fn randomise_synth_seeded(
        &mut self,
        uid: MachineUid,
        dice: acid_engine::random::Dice,
        seed: u64,
    ) {
        let p = &self.doc.project;
        let slot = self.selected(uid);
        let empty = DrumPattern::default();
        let groove = self
            .groove_kit(uid)
            .and_then(|k| Some(&p.kit(k)?.patterns[self.selected(k) as usize]))
            .unwrap_or(&empty);
        let pattern = acid_engine::random::randomise_synth(
            &self.synth_of(uid).patterns[slot as usize],
            dice,
            groove,
            seed,
        );
        let cmds = self.doc.apply(
            crate::document::Edit::SynthPattern {
                uid,
                slot,
                pattern: Box::new(pattern),
            },
            false,
        );
        self.send(cmds);
        self.view.focus = Some(uid);
    }

    pub(crate) fn drum_template(&mut self, uid: MachineUid) {
        let seed = self.next_seed();
        let slot = self.selected(uid);
        let Some(kit) = self.doc.project.kit(uid) else {
            return;
        };
        let length = kit.patterns[slot as usize].roll.length;
        let pattern = acid_engine::random::drum_template(seed, length);
        let cmds = self.doc.apply(
            crate::document::Edit::DrumPattern {
                uid,
                slot,
                pattern: Box::new(pattern),
            },
            false,
        );
        self.send(cmds);
        self.view.focus = Some(uid);
    }

    pub(crate) fn evolve_drums(&mut self, uid: MachineUid) {
        let seed = self.next_seed();
        self.evolve_drums_seeded(uid, seed);
    }

    pub(crate) fn evolve_drums_seeded(&mut self, uid: MachineUid, seed: u64) {
        let slot = self.selected(uid);
        let Some(kit) = self.doc.project.kit(uid) else {
            return;
        };
        let pattern = acid_engine::random::evolve_drums(&kit.patterns[slot as usize], seed);
        let cmds = self.doc.apply(
            crate::document::Edit::DrumPattern {
                uid,
                slot,
                pattern: Box::new(pattern),
            },
            false,
        );
        self.send(cmds);
        self.view.focus = Some(uid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::test_rack::*;

    #[test]
    fn length_and_transpose_edits_focus_the_synth() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.focus = Some(FM);
        let slot = app.doc.project.machine(SYNTH2).unwrap().selected;
        app.set_synth_length(SYNTH2, slot, 7, false);
        assert_eq!(app.view.focus, Some(SYNTH2));
        app.view.focus = Some(FM);
        app.set_synth_transpose(SYNTH2, slot, -3, false);
        assert_eq!(app.view.focus, Some(SYNTH2));
        let p = &app.doc.project.synth(SYNTH2).unwrap().patterns[slot as usize];
        assert_eq!((p.roll.length, p.transpose), (7, -3));
    }

    #[test]
    fn keyboard_length_and_transpose_edits_are_separate_undo_steps() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let slot = app.doc.project.machine(SYNTH1).unwrap().selected;
        let before = app.doc.project.synth(SYNTH1).unwrap().patterns[slot as usize].clone();
        // Two arrow-key presses (not a drag): two undo steps.
        app.set_synth_length(SYNTH1, slot, 7, false);
        app.set_synth_length(SYNTH1, slot, 8, false);
        app.set_synth_transpose(SYNTH1, slot, 2, false);
        app.set_synth_transpose(SYNTH1, slot, 3, false);
        app.doc.undo();
        let p = &app.doc.project.synth(SYNTH1).unwrap().patterns[slot as usize];
        assert_eq!(
            (p.roll.length, p.transpose),
            (8, 2),
            "one undo reverts one key press"
        );
        app.doc.undo();
        app.doc.undo();
        let p = &app.doc.project.synth(SYNTH1).unwrap().patterns[slot as usize];
        assert_eq!((p.roll.length, p.transpose), (7, before.transpose));
        // A drag (merge) is one step.
        app.doc.undo();
        app.set_synth_length(SYNTH1, slot, 9, true);
        app.set_synth_length(SYNTH1, slot, 10, true);
        app.doc.end_gesture();
        app.doc.undo();
        assert_eq!(
            app.doc.project.synth(SYNTH1).unwrap().patterns[slot as usize],
            before
        );
    }
}
