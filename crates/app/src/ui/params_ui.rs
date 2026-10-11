//! Binding knobs and switches to project parameters.

use super::{SidAcidApp, knob};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::params::{FxParam, ParamId, SynthParam, step_index};
use eframe::egui;

/// Display names for each position of a discrete parameter.
pub fn discrete_names(id: ParamId) -> &'static [&'static str] {
    match id {
        ParamId::Synth(_, SynthParam::Wave) => &["Saw", "Pulse"],
        ParamId::Synth(_, SynthParam::FilterMode) => &["LP", "BP", "HP"],
        ParamId::Fx(FxParam::DelayDivision) => &["1/16", "1/8", "3/16", "1/4", "3/8", "1/2"],
        _ => &["Off", "On"],
    }
}

/// "Synth 1 Cutoff", "808 BD Tune", "909 Volume", "Synth 2 Level", "Feedback", "Master Ratio".
pub fn param_label(id: ParamId) -> String {
    const KITS: [&str; 2] = ["808", "909"];
    let pick = |names: &[&'static str], i: u8| names.get(i as usize).copied().unwrap_or("?");
    match id {
        ParamId::Synth(s, p) => format!("Synth {} {}", s + 1, p.name()),
        ParamId::Drum(k, i, p) => format!(
            "{} {} {}",
            pick(&KITS, k),
            pick(&super::drum_panel::INST_NAMES, i),
            p.name()
        ),
        ParamId::Kit(k, p) => format!("{} {}", pick(&KITS, k), p.name()),
        ParamId::Channel(c, p) => format!(
            "{} {}",
            pick(&super::mixer_panel::CHANNEL_NAMES, c),
            p.name()
        ),
        ParamId::Fx(p) => p.name().to_string(),
        ParamId::Master(p) => format!("Master {}", p.name()),
    }
}

impl SidAcidApp {
    /// A knob (continuous) or switch (discrete) bound to `id`.
    pub(crate) fn param_knob(&mut self, ui: &mut egui::Ui, id: ParamId, label: &str) {
        let info = id.info();
        let value = self.doc.project.param(id);
        if info.steps >= 2 {
            let names = discrete_names(id);
            if let Some(i) = knob::switch(ui, label, step_index(value, info.steps), names) {
                let v = i as f32 / (info.steps - 1) as f32;
                let cmds = self.doc.apply(Edit::Param { id, value: v }, false);
                self.send(cmds);
            }
            return;
        }
        let recording = self.recording();
        let value = if recording {
            self.view
                .live
                .get(&id)
                .copied()
                .unwrap_or_else(|| self.doc.project.param(id))
        } else {
            self.doc.project.param(id)
        };
        let automated = acid_engine::song::is_automated(&self.doc.project.song, id);
        let tip = format!(
            "{} {:.0}%{}  (drag, Shift = fine, double-click = reset)",
            info.name,
            value * 100.0,
            if automated {
                "  \u{b7} automated in song"
            } else {
                ""
            }
        );
        let out = knob::knob(ui, label, value, info.default, &tip, automated);
        if let Some(v) = out.value {
            if recording {
                self.view.live.insert(id, v);
                self.send(vec![Command::SetParamLive(id, v)]);
            } else {
                let cmds = self.doc.apply(Edit::Param { id, value: v }, true);
                self.send(cmds);
            }
        }
        if out.touched {
            if self.view.touched_knob != Some(id) && recording {
                self.send(vec![Command::Touch(id, true)]);
            }
            self.view.touched_knob = Some(id);
        } else if self.view.touched_knob == Some(id) {
            self.release_knob(id);
        }
        if out.released {
            self.release_knob(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_labels_name_the_machine() {
        use acid_engine::params::{DrumParam, MasterParam, SynthParam};
        assert_eq!(
            param_label(ParamId::Synth(0, SynthParam::Cutoff)),
            "Synth 1 Cutoff"
        );
        assert_eq!(
            param_label(ParamId::Drum(1, 0, DrumParam::Tune)),
            "909 BD Tune"
        );
        assert_eq!(
            param_label(ParamId::Master(MasterParam::CompRatio)),
            "Master Ratio"
        );
    }

    #[test]
    fn every_discrete_param_has_a_name_per_step() {
        for id in ParamId::all() {
            let steps = id.info().steps;
            if steps >= 2 {
                assert_eq!(discrete_names(id).len(), steps as usize, "{id:?}");
            }
        }
    }
}
