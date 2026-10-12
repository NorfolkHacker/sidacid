//! Binding knobs and switches to project parameters.

use super::{SidAcidApp, knob};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::params::{FmParam, FxParam, OpParam, ParamId, SynthParam, fm_steps, step_index};
use eframe::egui;

/// Display names for each position of a named switch; empty for a knob.
pub fn discrete_names(id: ParamId) -> &'static [&'static str] {
    match id {
        ParamId::Synth(_, SynthParam::Wave) => &["Saw", "Pulse"],
        ParamId::Synth(_, SynthParam::FilterMode) => &["LP", "BP", "HP"],
        ParamId::Fx(FxParam::DelayDivision) => &["1/16", "1/8", "3/16", "1/4", "3/8", "1/2"],
        ParamId::Fm(FmParam::LfoWave) => &["Tri", "Saw Dn", "Saw Up", "Square", "Sine", "S&H"],
        ParamId::Fm(FmParam::Mono) => &["Poly", "Mono"],
        ParamId::Fm(FmParam::FilterMode) => &["LP", "HP"],
        ParamId::FmOp(_, OpParam::LeftCurve | OpParam::RightCurve) => {
            &["-Lin", "-Exp", "+Exp", "+Lin"]
        }
        ParamId::FmOp(_, OpParam::FixedFreq) => &["Ratio", "Fixed"],
        _ if id.info().steps == 2 => &["Off", "On"],
        _ => &[],
    }
}

/// The value a knob shows in its tooltip: DX7 units for FM controls, a
/// percentage otherwise.
pub fn value_text(id: ParamId, v: f32) -> String {
    use FmParam as F;
    use OpParam as O;
    let pick = |n: usize| step_index(v, n as u8) as i32;
    let signed = |x: i32| {
        if x > 0 {
            format!("+{x}")
        } else {
            x.to_string()
        }
    };
    match id {
        ParamId::Fm(F::Algorithm) => (pick(fm_steps::ALGORITHM) + 1).to_string(),
        ParamId::Fm(F::Feedback) => pick(fm_steps::FEEDBACK).to_string(),
        ParamId::Fm(F::PitchModSens) => pick(fm_steps::PITCH_MOD_SENS).to_string(),
        ParamId::Fm(F::Transpose) => signed(pick(fm_steps::TRANSPOSE) - 24),
        ParamId::FmOp(_, O::Coarse) => pick(fm_steps::COARSE).to_string(),
        ParamId::FmOp(_, O::Detune) => signed(pick(fm_steps::DETUNE) - 7),
        ParamId::FmOp(_, O::RateScaling) => pick(fm_steps::RATE_SCALING).to_string(),
        ParamId::FmOp(_, O::AmpModSens) => pick(fm_steps::AMP_MOD_SENS).to_string(),
        ParamId::FmOp(_, O::VelSens) => pick(fm_steps::VEL_SENS).to_string(),
        ParamId::Fm(
            F::PitchR1
            | F::PitchR2
            | F::PitchR3
            | F::PitchR4
            | F::PitchL1
            | F::PitchL2
            | F::PitchL3
            | F::PitchL4
            | F::LfoSpeed
            | F::LfoDelay
            | F::LfoPmd
            | F::LfoAmd,
        )
        | ParamId::FmOp(_, _) => format!("{:.0}", v.clamp(0.0, 1.0) * 99.0),
        _ => format!("{:.0}%", v * 100.0),
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
        ParamId::Fm(p) => format!("FM {}", p.name()),
        ParamId::Sampler(p) => format!("Sampler {}", p.name()),
        ParamId::FmOp(op, p) => format!("FM OP{} {}", op + 1, p.name()),
    }
}

impl SidAcidApp {
    /// A knob (continuous) or switch (discrete) bound to `id`.
    pub(crate) fn param_knob(&mut self, ui: &mut egui::Ui, id: ParamId, label: &str) {
        let info = id.info();
        let value = self.doc.project.param(id);
        if info.steps >= 2 && !discrete_names(id).is_empty() {
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
            "{} {}{}  (drag, Shift = fine, double-click = reset)",
            info.name,
            value_text(id, value),
            if automated {
                "  \u{b7} automated in song"
            } else {
                ""
            }
        );
        let out = knob::knob(ui, label, value, info.default, &tip, automated);
        self.knob_gesture(id, out, recording);
    }

    /// Applies one frame of knob interaction: a new value, and the touch
    /// that keeps automation off the knob while it is held.
    pub(crate) fn knob_gesture(&mut self, id: ParamId, out: knob::KnobOutput, recording: bool) {
        if let Some(v) = out.value {
            if recording {
                self.view.live.insert(id, v);
                self.send(vec![Command::SetParamLive(id, v)]);
            } else {
                let cmds = self.doc.apply(Edit::Param { id, value: v }, true);
                self.send(cmds);
            }
        }
        // While recording, any edit (a scroll notch, a double-click reset)
        // holds the knob for this frame so the value is written.
        if out.touched || (recording && out.value.is_some()) {
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
    fn an_edit_without_a_drag_counts_as_a_touch_while_recording() {
        use crate::document::Document;
        use acid_engine::params::MasterParam;
        let id = ParamId::Master(MasterParam::Level);
        let mut doc = Document::new(acid_engine::demo::demo_project());
        doc.project.song.bars = vec![acid_engine::project::Bar::default(); 2];
        let mut app = SidAcidApp::headless(doc);
        app.view.mode = acid_engine::sequencer::Mode::Song;
        app.view.record = true;
        app.status.playing = true;
        app.status.song_step = 5;
        let edit = knob::KnobOutput {
            value: Some(0.25),
            released: false,
            touched: false,
        };
        app.knob_gesture(id, edit, true);
        assert_eq!(app.view.touched_knob, Some(id));
        assert!(
            app.pending
                .iter()
                .any(|c| matches!(c, Command::Touch(t, true) if *t == id))
        );
        app.record_automation();
        let lane = acid_engine::song::lane(&app.doc.project.song, id).unwrap();
        assert_eq!(lane.points.get(&5), Some(&0.25));
        let idle = knob::KnobOutput {
            value: None,
            released: false,
            touched: false,
        };
        app.knob_gesture(id, idle, true);
        assert_eq!(app.view.touched_knob, None, "released on the next frame");
        assert!(matches!(app.pending.last(), Some(Command::Touch(t, false)) if *t == id));
    }

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
        assert_eq!(
            param_label(ParamId::Fm(acid_engine::params::FmParam::Volume)),
            "FM Volume"
        );
        assert_eq!(
            param_label(ParamId::Channel(
                5,
                acid_engine::params::ChannelParam::Level
            )),
            "Sampler Level"
        );
    }

    #[test]
    fn fm_labels_switches_and_values() {
        use acid_engine::params::{FmParam, OpParam};
        assert_eq!(
            param_label(ParamId::FmOp(2, OpParam::Coarse)),
            "FM OP3 Coarse"
        );
        assert_eq!(param_label(ParamId::Fm(FmParam::Cutoff)), "FM Cutoff");
        assert_eq!(discrete_names(ParamId::Fm(FmParam::LfoWave)).len(), 6);
        assert_eq!(
            discrete_names(ParamId::FmOp(0, OpParam::LeftCurve))[3],
            "+Lin"
        );
        assert_eq!(
            discrete_names(ParamId::FmOp(0, OpParam::On)),
            &["Off", "On"]
        );
        assert!(discrete_names(ParamId::Fm(FmParam::Algorithm)).is_empty());
        assert_eq!(value_text(ParamId::Fm(FmParam::Algorithm), 1.0), "32");
        assert_eq!(value_text(ParamId::FmOp(0, OpParam::Detune), 0.0), "-7");
        assert_eq!(value_text(ParamId::Fm(FmParam::Transpose), 1.0), "+24");
        assert_eq!(value_text(ParamId::FmOp(0, OpParam::R1), 1.0), "99");
        assert_eq!(
            value_text(ParamId::Synth(0, SynthParam::Cutoff), 0.5),
            "50%"
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
