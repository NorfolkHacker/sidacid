//! Binding knobs and switches to project parameters.

use super::scroll::ScrollGesture;
use super::{SidAcidApp, knob};
use crate::document::Edit;
use acid_engine::fm::patch;
use acid_engine::host::Command;
use acid_engine::params::{
    FmParam, FxParam, MachineParam, MonoParam, OpParam, ParamId, SlotParam, SynthParam, fm_steps,
    sampler_map, step_index,
};
use eframe::egui;

/// Display names for each position of a named switch; empty for a knob.
pub fn discrete_names(id: ParamId) -> &'static [&'static str] {
    use MachineParam as M;
    match id {
        ParamId::Machine(_, M::Synth(SynthParam::Wave)) => &["Saw", "Pulse"],
        ParamId::Machine(_, M::Synth(SynthParam::FilterMode)) => &["LP", "BP", "HP"],
        ParamId::Fx(FxParam::DelayDivision) => &["1/16", "1/8", "3/16", "1/4", "3/8", "1/2"],
        ParamId::Machine(_, M::Fm(FmParam::LfoWave)) => {
            &["Tri", "Saw Dn", "Saw Up", "Square", "Sine", "S&H"]
        }
        ParamId::Machine(_, M::Fm(FmParam::Mono)) => &["Poly", "Mono"],
        ParamId::Machine(_, M::Fm(FmParam::FilterMode)) => &["LP", "HP"],
        ParamId::Machine(_, M::FmOp(_, OpParam::LeftCurve | OpParam::RightCurve)) => {
            &["-Lin", "-Exp", "+Exp", "+Lin"]
        }
        ParamId::Machine(_, M::FmOp(_, OpParam::FixedFreq)) => &["Ratio", "Fixed"],
        ParamId::Machine(_, M::Mono(p)) => match p {
            MonoParam::Range => &["16'", "8'", "4'", "2'"],
            MonoParam::PwmSource => &["Manual", "LFO", "Env"],
            MonoParam::SubWave => &["-1 Sq", "-2 Sq", "-2 Pls"],
            MonoParam::Trigger => &["Gate+Trig", "Gate", "LFO"],
            MonoParam::VcaMode => &["Env", "Gate"],
            MonoParam::LfoWave => &["Tri", "Square", "Random", "Noise"],
            MonoParam::Portamento => &["Off", "On", "Auto"],
            MonoParam::ArpMode => &["Up", "U&D", "Down"],
            MonoParam::ArpRange => &["1 Oct", "2 Oct", "3 Oct"],
            MonoParam::ArpRate => &["1/4", "1/8", "1/8T", "1/16", "1/16T", "1/32"],
            _ if id.info().steps == 2 => &["Off", "On"],
            _ => &[],
        },
        _ if id.info().steps == 2 => &["Off", "On"],
        _ => &[],
    }
}

/// "12 ms" below a second, "1.50 s" from a second up.
fn seconds_text(s: f32) -> String {
    if s < 1.0 {
        format!("{:.0} ms", s * 1000.0)
    } else {
        format!("{s:.2} s")
    }
}

/// The value a knob shows in its tooltip: DX7 units for FM controls
/// (seconds for Glide, Hz for Cutoff), a percentage otherwise.
pub fn value_text(id: ParamId, v: f32) -> String {
    use FmParam as F;
    use MachineParam as M;
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
        ParamId::Machine(_, M::Fm(F::Algorithm)) => (pick(fm_steps::ALGORITHM) + 1).to_string(),
        ParamId::Machine(_, M::Fm(F::Feedback)) => pick(fm_steps::FEEDBACK).to_string(),
        ParamId::Machine(_, M::Fm(F::PitchModSens)) => pick(fm_steps::PITCH_MOD_SENS).to_string(),
        ParamId::Machine(_, M::Fm(F::Transpose)) => signed(pick(fm_steps::TRANSPOSE) - 24),
        ParamId::Machine(_, M::FmOp(_, O::Coarse)) => pick(fm_steps::COARSE).to_string(),
        ParamId::Machine(_, M::FmOp(_, O::Detune)) => signed(pick(fm_steps::DETUNE) - 7),
        ParamId::Machine(_, M::FmOp(_, O::RateScaling)) => pick(fm_steps::RATE_SCALING).to_string(),
        ParamId::Machine(_, M::FmOp(_, O::AmpModSens)) => pick(fm_steps::AMP_MOD_SENS).to_string(),
        ParamId::Machine(_, M::FmOp(_, O::VelSens)) => pick(fm_steps::VEL_SENS).to_string(),
        ParamId::Machine(_, M::SamplerSlot(_, SlotParam::Tune)) => {
            signed(sampler_map::tune_semitones(v))
        }
        ParamId::Machine(_, M::SamplerSlot(_, SlotParam::Fine)) => {
            format!("{} c", signed(sampler_map::fine_cents(v) as i32))
        }
        ParamId::Machine(_, M::SamplerSlot(_, SlotParam::Attack)) => {
            seconds_text(sampler_map::attack_seconds(v))
        }
        ParamId::Machine(_, M::SamplerSlot(_, SlotParam::Decay)) => {
            seconds_text(sampler_map::decay_seconds(v))
        }
        ParamId::Machine(_, M::SamplerSlot(_, SlotParam::Release)) => {
            seconds_text(sampler_map::release_seconds(v))
        }
        ParamId::Machine(_, M::SamplerSlot(_, SlotParam::Cutoff)) => {
            format!("{:.0} Hz", sampler_map::cutoff_hz(v))
        }
        ParamId::Machine(_, M::Fm(F::Glide)) => format!("{:.2} s", patch::glide_seconds(v)),
        ParamId::Machine(_, M::Fm(F::Cutoff)) => format!("{:.0} Hz", patch::cutoff_hz(v)),
        ParamId::Machine(
            _,
            M::Fm(
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
            | M::FmOp(_, _),
        ) => format!("{:.0}", patch::d99(v)),
        _ => format!("{:.0}%", v * 100.0),
    }
}

/// "Synth 1 Cutoff", "808 BD Tune", "909 Volume", "Synth 2 Level", "Feedback",
/// "Master Ratio": a machine param is prefixed with its machine's name.
pub fn param_label(project: &acid_engine::project::Project, id: ParamId) -> String {
    let (uid, p) = match id {
        ParamId::Fx(p) => return p.name().to_string(),
        ParamId::Master(p) => return format!("Master {}", p.name()),
        ParamId::Machine(uid, p) => (uid, p),
    };
    let machine = project.machine(uid).map_or("?", |m| m.name.as_str());
    match p {
        MachineParam::Synth(p) => format!("{machine} {}", p.name()),
        MachineParam::Drum(i, p) => format!(
            "{machine} {} {}",
            super::lane::INST_NAMES
                .get(i as usize)
                .copied()
                .unwrap_or("?"),
            p.name()
        ),
        MachineParam::Kit(p) => format!("{machine} {}", p.name()),
        MachineParam::Channel(p) => format!("{machine} {}", p.name()),
        MachineParam::Fm(p) => format!("{machine} {}", p.name()),
        MachineParam::Mono(p) => format!("{machine} {}", p.name()),
        MachineParam::Sampler(p) => format!("{machine} {}", p.name()),
        MachineParam::FmOp(op, p) => format!("{machine} OP{} {}", op + 1, p.name()),
        MachineParam::SamplerSlot(s, p) => format!("{machine} Slot {} {}", s + 1, p.name()),
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
                self.edit_switch(id, v);
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
        self.knob_gesture(id, out, recording, ui.input(|i| i.time));
    }

    /// Applies one frame of knob interaction: a new value, and the touch
    /// that keeps automation off the knob while it is held.
    pub(crate) fn knob_gesture(
        &mut self,
        id: ParamId,
        out: knob::KnobOutput,
        recording: bool,
        now: f64,
    ) {
        // Any other knob edit, or the wheel moving on to another knob
        // (even by less than a notch), ends a resting wheel gesture first,
        // so nothing later merges into the wheel's undo step.
        if (out.value.is_some() || out.scrolled)
            && !(out.scrolled && self.knob_scroll_open(id))
            && let Some((_, g)) = self.view.knob_scroll.take()
        {
            self.close_scroll_step(g);
        }
        let serial_before = self.doc.edit_serial();
        if let Some(v) = out.value {
            if recording {
                self.view.live.insert(id, v);
                self.send(vec![Command::SetParamLive(id, v)]);
            } else {
                let cmds = self.doc.apply(Edit::Param { id, value: v }, true);
                self.send(cmds);
            }
        }
        if out.scrolled && !recording {
            // Its undo step stays open until the wheel rests. A notch the
            // knob's limit turned into a no-op made no step of its own.
            let serial = if self.doc.edit_serial() != serial_before {
                Some(self.doc.edit_serial())
            } else {
                self.view
                    .knob_scroll
                    .filter(|(k, _)| *k == id)
                    .and_then(|(_, g)| g.serial)
            };
            self.view.knob_scroll = Some((id, ScrollGesture { last: now, serial }));
        }
        // While recording, any edit (a scroll notch, a double-click reset)
        // holds the knob for this frame so the value is written.
        if out.touched || (recording && out.value.is_some()) {
            if recording {
                self.engine_touch(id);
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

impl SidAcidApp {
    /// A wheel gesture on knob `id` is waiting for the wheel to rest.
    pub(crate) fn knob_scroll_open(&self, id: ParamId) -> bool {
        self.view.knob_scroll.is_some_and(|(k, _)| k == id)
    }

    /// A switch change: part of the record pass while recording (switches
    /// are not automated, but their saved value still changes).
    pub(crate) fn edit_switch(&mut self, id: ParamId, value: f32) {
        let edit = Edit::Param { id, value };
        let cmds = if self.recording() {
            self.doc.record(edit)
        } else {
            self.doc.apply(edit, false)
        };
        self.send(cmds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rack::*;

    #[test]
    fn mono_switches_have_names_for_every_position() {
        use acid_engine::machine::MachineUid;
        use acid_engine::params::MonoParam;
        let uid = MachineUid(9);
        for p in MonoParam::ALL {
            let id = uid.param(p);
            let steps = id.info().steps as usize;
            if steps >= 2 {
                assert_eq!(discrete_names(id).len(), steps, "{p:?}");
            }
        }
    }

    #[test]
    fn an_edit_without_a_drag_counts_as_a_touch_while_recording() {
        use crate::document::Document;
        use acid_engine::params::MasterParam;
        let id = ParamId::Master(MasterParam::Level);
        let mut doc = Document::new(acid_engine::demo::demo_project());
        doc.project.song.lanes.clear();
        doc.project.song.end = Some(32);
        let mut app = SidAcidApp::headless(doc);
        app.view.mode = acid_engine::sequencer::Mode::Song;
        app.view.record = true;
        app.status.playing = true;
        app.status.song_step = 5;
        let edit = knob::KnobOutput {
            value: Some(0.25),
            released: false,
            touched: false,
            scrolled: false,
        };
        app.knob_gesture(id, edit, true, 0.0);
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
            scrolled: false,
        };
        app.knob_gesture(id, idle, true, 0.1);
        assert_eq!(app.view.touched_knob, None, "released on the next frame");
        assert!(matches!(app.pending.last(), Some(Command::Touch(t, false)) if *t == id));
    }

    fn scroll(value: f32) -> knob::KnobOutput {
        knob::KnobOutput {
            value: Some(value),
            released: false,
            touched: true,
            scrolled: true,
        }
    }

    fn idle() -> knob::KnobOutput {
        knob::KnobOutput {
            value: None,
            released: false,
            touched: false,
            scrolled: false,
        }
    }

    /// One UI frame of knob `id` at time `now` (the gesture, then the
    /// per-frame scroll check `ui` runs).
    fn frame(app: &mut SidAcidApp, id: ParamId, out: knob::KnobOutput, now: f64) {
        app.knob_gesture(id, out, false, now);
        app.tick_scroll_gestures(now);
    }

    #[test]
    fn a_notch_at_the_knob_limit_takes_no_other_edits_serial() {
        use crate::document::Document;
        let id = SYNTH1.param(SynthParam::Cutoff);
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let cmds = app.doc.apply(Edit::Param { id, value: 1.0 }, false);
        app.send(cmds);
        // Another edit, then a notch that the limit turns into a no-op.
        let cmds = app.doc.apply(Edit::Bpm(100.0), true);
        app.send(cmds);
        app.knob_gesture(id, scroll(1.0), false, 0.0);
        let (_, g) = app.view.knob_scroll.expect("the wheel gesture is open");
        assert_eq!(g.serial, None, "the BPM edit is not this wheel's step");
    }

    #[test]
    fn wheel_notches_close_together_are_one_undo_step() {
        use crate::document::Document;
        let id = SYNTH1.param(SynthParam::Cutoff);
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let start = app.doc.project.param(id);
        // Notches 100 ms apart, idle frames in between.
        for (i, t) in [0.0, 0.1, 0.2].into_iter().enumerate() {
            frame(&mut app, id, scroll(0.1 * (i + 1) as f32), t);
            frame(&mut app, id, idle(), t + 0.05);
        }
        frame(&mut app, id, idle(), 0.4);
        assert!(
            app.view.knob_scroll.is_some(),
            "still open 200 ms after the last notch"
        );
        frame(&mut app, id, idle(), 0.6);
        assert!(
            app.view.knob_scroll.is_none(),
            "closed after 300 ms of rest"
        );
        // A notch a second later is a new step.
        frame(&mut app, id, scroll(0.9), 1.6);
        frame(&mut app, id, idle(), 2.0);
        app.doc.undo();
        assert_eq!(app.doc.project.param(id), 0.3);
        app.doc.undo();
        assert_eq!(app.doc.project.param(id), start);
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn another_edit_during_the_rest_ends_the_scroll_step_first() {
        use crate::document::Document;
        let id = SYNTH1.param(SynthParam::Cutoff);
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let start = app.doc.project.param(id);
        frame(&mut app, id, scroll(0.2), 0.0);
        frame(&mut app, id, idle(), 0.05);
        // A double-click reset on the same knob within the rest.
        let reset = knob::KnobOutput {
            value: Some(0.7),
            released: false,
            touched: true,
            scrolled: false,
        };
        frame(&mut app, id, reset, 0.1);
        frame(&mut app, id, idle(), 0.15);
        // Another knob's drag is open when the old scroll would have closed.
        let other = SYNTH1.param(SynthParam::Resonance);
        let drag = |v| knob::KnobOutput {
            value: Some(v),
            released: false,
            touched: true,
            scrolled: false,
        };
        frame(&mut app, other, drag(0.1), 0.2);
        frame(&mut app, other, drag(0.2), 0.5);
        frame(&mut app, other, drag(0.3), 0.8);
        let mut done = drag(0.4);
        done.released = true;
        frame(&mut app, other, done, 0.9);
        app.doc.undo();
        assert_eq!(app.doc.project.param(id), 0.7, "the drag is one step");
        app.doc.undo();
        assert_eq!(app.doc.project.param(id), 0.2, "the reset is its own step");
        app.doc.undo();
        assert_eq!(app.doc.project.param(id), start);
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn a_sub_notch_wheel_on_another_knob_ends_the_first_knobs_scroll_step() {
        use crate::document::Document;
        let a = SYNTH1.param(SynthParam::Cutoff);
        let b = SYNTH1.param(SynthParam::Resonance);
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let start = app.doc.project.param(a);
        frame(&mut app, a, scroll(0.2), 0.0);
        frame(&mut app, a, idle(), 0.05);
        // Less than a notch over B: no value, but the wheel moved on.
        let mut nudge = scroll(0.0);
        nudge.value = None;
        frame(&mut app, b, nudge, 0.1);
        frame(&mut app, b, idle(), 0.15);
        frame(&mut app, b, idle(), 1.0);
        // Much later, a drag on A.
        let drag = |v| knob::KnobOutput {
            value: Some(v),
            released: false,
            touched: true,
            scrolled: false,
        };
        frame(&mut app, a, drag(0.6), 2.0);
        let mut done = drag(0.7);
        done.released = true;
        frame(&mut app, a, done, 2.1);
        app.doc.undo();
        assert_eq!(app.doc.project.param(a), 0.2, "the drag is its own step");
        app.doc.undo();
        assert_eq!(app.doc.project.param(a), start);
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn sampler_labels_and_values() {
        use acid_engine::params::SamplerParam;
        assert_eq!(
            param_label(
                &acid_engine::project::Project::default(),
                SAMPLER.param(MachineParam::SamplerSlot(2, SlotParam::Tune))
            ),
            "Sampler Slot 3 Tune"
        );
        assert_eq!(
            param_label(
                &acid_engine::project::Project::default(),
                SAMPLER.param(SamplerParam::Grit)
            ),
            "Sampler Grit"
        );
        assert_eq!(
            discrete_names(SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::Reverse))),
            &["Off", "On"]
        );
        assert_eq!(
            value_text(
                SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::Tune)),
                1.0
            ),
            "+24"
        );
        assert_eq!(
            value_text(
                SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::Fine)),
                0.0
            ),
            "-50 c"
        );
        assert_eq!(
            value_text(
                SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::Attack)),
                0.0
            ),
            "1 ms"
        );
        assert_eq!(
            value_text(
                SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::Release)),
                1.0
            ),
            "4.00 s"
        );
        assert_eq!(
            value_text(
                SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::Cutoff)),
                1.0
            ),
            "18000 Hz"
        );
        assert_eq!(
            value_text(
                SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::Level)),
                0.5
            ),
            "50%"
        );
    }

    #[test]
    fn param_labels_name_the_machine() {
        use acid_engine::params::{DrumParam, MasterParam, SynthParam};
        assert_eq!(
            param_label(
                &acid_engine::project::Project::default(),
                SYNTH1.param(SynthParam::Cutoff)
            ),
            "Synth 1 Cutoff"
        );
        assert_eq!(
            param_label(
                &acid_engine::project::Project::default(),
                KIT909.param(MachineParam::Drum(0, DrumParam::Tune))
            ),
            "909 BD Tune"
        );
        assert_eq!(
            param_label(
                &acid_engine::project::Project::default(),
                ParamId::Master(MasterParam::CompRatio)
            ),
            "Master Ratio"
        );
        assert_eq!(
            param_label(
                &acid_engine::project::Project::default(),
                FM.param(acid_engine::params::FmParam::Volume)
            ),
            "FM Volume"
        );
        assert_eq!(
            param_label(
                &acid_engine::project::Project::default(),
                SAMPLER.param(acid_engine::params::ChannelParam::Level)
            ),
            "Sampler Level"
        );
    }

    #[test]
    fn fm_labels_switches_and_values() {
        use acid_engine::params::{FmParam, OpParam};
        assert_eq!(
            param_label(
                &acid_engine::project::Project::default(),
                FM.param(MachineParam::FmOp(2, OpParam::Coarse))
            ),
            "FM OP3 Coarse"
        );
        assert_eq!(
            param_label(
                &acid_engine::project::Project::default(),
                FM.param(FmParam::Cutoff)
            ),
            "FM Cutoff"
        );
        assert_eq!(discrete_names(FM.param(FmParam::LfoWave)).len(), 6);
        assert_eq!(
            discrete_names(FM.param(MachineParam::FmOp(0, OpParam::LeftCurve)))[3],
            "+Lin"
        );
        assert_eq!(
            discrete_names(FM.param(MachineParam::FmOp(0, OpParam::On))),
            &["Off", "On"]
        );
        assert!(discrete_names(FM.param(FmParam::Algorithm)).is_empty());
        assert_eq!(value_text(FM.param(FmParam::Algorithm), 1.0), "32");
        assert_eq!(
            value_text(FM.param(MachineParam::FmOp(0, OpParam::Detune)), 0.0),
            "-7"
        );
        assert_eq!(value_text(FM.param(FmParam::Transpose), 1.0), "+24");
        assert_eq!(
            value_text(FM.param(MachineParam::FmOp(0, OpParam::R1)), 1.0),
            "99"
        );
        assert_eq!(
            value_text(FM.param(MachineParam::FmOp(0, OpParam::L1)), 0.5),
            "50"
        );
        assert_eq!(value_text(FM.param(FmParam::Glide), 0.5), "0.50 s");
        assert_eq!(value_text(FM.param(FmParam::Glide), 1.0), "2.00 s");
        assert_eq!(value_text(FM.param(FmParam::Cutoff), 0.0), "40 Hz");
        assert_eq!(value_text(FM.param(FmParam::Cutoff), 1.0), "18000 Hz");
        assert_eq!(value_text(SYNTH1.param(SynthParam::Cutoff), 0.5), "50%");
    }

    #[test]
    fn every_discrete_param_has_a_name_per_step() {
        let p = acid_engine::project::Project::default();
        let machines = p.machines.iter().flat_map(|m| {
            m.params
                .keys()
                .map(|&mp| acid_engine::params::ParamId::Machine(m.uid, mp))
        });
        let globals = p
            .globals
            .keys()
            .map(|&g| acid_engine::params::ParamId::from(g));
        for id in machines.chain(globals) {
            let steps = id.info().steps;
            if steps >= 2 {
                assert_eq!(discrete_names(id).len(), steps as usize, "{id:?}");
            }
        }
    }
}
