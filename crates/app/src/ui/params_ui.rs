//! Binding knobs and switches to project parameters.

use super::{SidAcidApp, knob};
use crate::document::Edit;
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
        let tip = format!(
            "{} {:.0}%  (drag, Shift = fine, double-click = reset)",
            info.name,
            value * 100.0
        );
        let out = knob::knob(ui, label, value, info.default, &tip);
        if let Some(v) = out.value {
            let cmds = self.doc.apply(Edit::Param { id, value: v }, true);
            self.send(cmds);
        }
        if out.released {
            self.doc.end_gesture();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
