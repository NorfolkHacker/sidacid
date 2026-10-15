//! The Mono panel: the synth's knobs grouped in front-panel order (VCO,
//! Source, VCF, Env; then LFO, Porta, Arp, Drift, Chorus), an envelope
//! sketch, the patch browser, and the pattern row with the poly piano roll.

use super::mono_patches::STARTER;
use super::{SidAcidApp, theme};
use acid_engine::machine::MachineUid;
use acid_engine::params::MonoParam as M;
use acid_engine::project::{MonoMachine, PatternRef};
use eframe::egui::{self, Sense, Stroke, pos2, vec2};

type Group = (&'static str, &'static [(M, &'static str)]);

const FRONT: [Group; 4] = [
    ("VCO", &[(M::Range, "Range"), (M::Tune, "Tune"), (M::VcoMod, "Mod"), (M::PulseWidth, "PW"), (M::PwmSource, "PWM")]),
    ("SOURCE", &[(M::PulseLevel, "Pulse"), (M::SawLevel, "Saw"), (M::SubLevel, "Sub"), (M::SubWave, "Sub Wave"), (M::NoiseLevel, "Noise")]),
    ("VCF", &[(M::Cutoff, "Cutoff"), (M::Resonance, "Reso"), (M::EnvAmount, "Env"), (M::VcfMod, "Mod"), (M::KeyFollow, "Kybd"), (M::VelToVcf, "Vel")]),
    ("ENV", &[(M::Attack, "A"), (M::Decay, "D"), (M::Sustain, "S"), (M::Release, "R"), (M::Trigger, "Trig"), (M::VcaMode, "VCA"), (M::VelToVca, "Vel")]),
];
const SECOND: [Group; 6] = [
    ("LFO", &[(M::LfoWave, "Wave"), (M::LfoRate, "Rate")]),
    ("PORTA", &[(M::Portamento, "Mode"), (M::PortaTime, "Time")]),
    ("ARP", &[(M::ArpOn, "On"), (M::ArpMode, "Mode"), (M::ArpRange, "Range"), (M::ArpRate, "Rate")]),
    ("DRIFT", &[(M::Drift, "Drift")]),
    ("CHORUS", &[(M::ChorusRate, "Rate"), (M::ChorusDepth, "Depth"), (M::ChorusMix, "Mix")]),
    ("OUT", &[(M::Volume, "Volume")]),
];

impl SidAcidApp {
    /// Mono machine `uid`; it must be a Mono machine in the rack.
    fn mono_of(&self, uid: MachineUid) -> &MonoMachine {
        self.doc.project.mono(uid).expect("a Mono machine in the rack")
    }

    pub(crate) fn mono_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{}   {}   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label(),
            self.mono_of(uid).patch_name
        );
        self.machine_panel(ui, uid, "mono", title, |app, ui| {
            ui.horizontal_wrapped(|ui| {
                for group in FRONT {
                    app.mono_group(ui, uid, group);
                }
            });
            ui.horizontal_wrapped(|ui| {
                for group in SECOND {
                    app.mono_group(ui, uid, group);
                }
                app.mono_env_sketch(ui, uid);
            });
            app.mono_patch_browser(ui, uid);
            app.note_pattern_row(ui, uid);
            app.poly_roll(ui, uid);
        });
    }

    fn mono_group(&mut self, ui: &mut egui::Ui, uid: MachineUid, (label, knobs): Group) {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).small().color(theme::ACID));
                ui.horizontal(|ui| {
                    for &(p, name) in knobs {
                        self.param_knob(ui, uid.param(p), name);
                    }
                });
            });
        });
    }

    /// Display-only ADSR shape from the current knobs (times drawn on a
    /// log-ish scale so short and long envelopes both read).
    fn mono_env_sketch(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let v = |p: M| self.doc.project.param(uid.param(p));
        let (a, d, s, r) = (v(M::Attack), v(M::Decay), v(M::Sustain), v(M::Release));
        let (rect, _) = ui.allocate_exact_size(vec2(96.0, 44.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, theme::GRID_DARK);
        let inner = rect.shrink(3.0);
        // Segment widths: attack, decay, a fixed sustain hold, release.
        let w = [0.1 + a, 0.1 + d, 0.6, 0.1 + r];
        let total: f32 = w.iter().sum();
        let mut x = 0.0;
        let mut points = vec![(0.0, 0.0)];
        for (seg, level) in w.iter().zip([1.0, s, s, 0.0]) {
            x += seg / total;
            points.push((x, level));
        }
        let points: Vec<egui::Pos2> = points
            .iter()
            .map(|(x, y)| pos2(inner.left() + x * inner.width(), inner.bottom() - y * inner.height()))
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5_f32, theme::ACID)));
    }

    fn mono_patch_browser(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let current = self.mono_of(uid).patch_name.clone();
        ui.horizontal(|ui| {
            ui.label("Patches");
            if ui
                .button("Randomise")
                .on_hover_text("A new random sound that always plays (undoable)")
                .clicked()
            {
                self.randomise_mono_patch(uid);
            }
        });
        let mut picked = None;
        egui::Grid::new(("mono_patches", uid))
            .spacing([4.0, 2.0])
            .show(ui, |ui| {
                for (i, preset) in STARTER.iter().enumerate() {
                    if ui.selectable_label(preset.name == current, preset.name).clicked() {
                        picked = Some(i);
                    }
                    if i % 7 == 6 {
                        ui.end_row();
                    }
                }
            });
        if let Some(i) = picked {
            let preset = &STARTER[i];
            self.load_mono_patch(uid, preset.name, preset.to_params());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::machine::MachineType;

    #[test]
    fn the_panel_draws_its_sections_and_the_patch_list() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Mono);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.ui(ctx));
        let mut texts = Vec::new();
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        for want in ["VCO", "SOURCE", "VCF", "ENV", "LFO", "ARP", "Squelch", "Randomise"] {
            assert!(texts.iter().any(|t| t == want), "{want} not drawn");
        }
    }
}
