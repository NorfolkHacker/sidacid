//! A built-in demo groove: a squelchy acid line on synth 1, a higher
//! counter-line on synth 2, an 808 beat and 909 hats.

use crate::drums::{BD, CH, CP, OH, SD};
use crate::machine::{MachineUid, standard_rack};
use crate::params::{ChannelParam, FxParam, MasterParam, ParamId, SynthParam};
use crate::project::{DrumMachine, Project, SynthMachine};
use crate::steps::{DrumGrid, DrumHit, Gate, SynthGrid, SynthStep};

fn step(note: u8, octave: i8, accent: bool, slide: bool) -> SynthStep {
    SynthStep {
        note,
        octave,
        gate: Gate::On,
        accent,
        slide,
    }
}

const REST: SynthStep = SynthStep {
    note: 0,
    octave: 0,
    gate: Gate::Off,
    accent: false,
    slide: false,
};

fn synth(p: &mut Project, uid: MachineUid) -> &mut SynthMachine {
    p.synth_mut(uid).expect("standard rack synth")
}

fn kit(p: &mut Project, uid: MachineUid) -> &mut DrumMachine {
    p.kit_mut(uid).expect("standard rack kit")
}

pub fn demo_project() -> Project {
    let [s1, s2, k808, k909, ..] = standard_rack().map(|m| m.0);
    let mut p = Project {
        name: "Demo".into(),
        bpm: 128.0,
        swing: 0.15,
        ..Project::default()
    };

    let acid = [
        step(0, 0, false, false),
        step(0, 1, true, false),
        step(0, 0, false, true),
        step(3, 0, false, false),
        step(0, 0, true, false),
        step(10, -1, false, true),
        REST,
        step(7, 0, true, false),
        step(0, 0, false, false),
        step(0, 1, false, true),
        step(0, 1, true, false),
        step(3, 0, false, false),
        step(5, 0, false, true),
        step(3, 0, true, false),
        REST,
        step(10, -1, false, false),
    ];
    let pattern = &mut synth(&mut p, s1).patterns[0];
    let mut grid = SynthGrid::from_roll(&pattern.roll);
    grid.steps[..16].copy_from_slice(&acid);
    pattern.roll = grid.to_roll();

    let counter = [
        step(7, 1, false, false),
        REST,
        REST,
        step(10, 1, true, true),
        step(7, 1, false, false),
        REST,
        step(3, 1, false, false),
        REST,
        step(7, 1, true, false),
        REST,
        REST,
        step(5, 1, false, true),
        step(3, 1, false, false),
        REST,
        step(0, 1, true, false),
        REST,
    ];
    let pattern = &mut synth(&mut p, s2).patterns[0];
    let mut grid = SynthGrid::from_roll(&pattern.roll);
    grid.steps[..16].copy_from_slice(&counter);
    pattern.roll = grid.to_roll();

    let pattern = &mut kit(&mut p, k808).patterns[0];
    let mut kit808 = DrumGrid::from_roll(&pattern.roll);
    for s in [0, 4, 8, 12] {
        kit808.steps[s][BD] = DrumHit::On;
    }
    kit808.steps[4][SD] = DrumHit::On;
    kit808.steps[12][CP] = DrumHit::Accent;
    kit808.steps[15][SD] = DrumHit::On;
    pattern.roll = kit808.to_roll();

    let pattern = &mut kit(&mut p, k909).patterns[0];
    let mut kit909 = DrumGrid::from_roll(&pattern.roll);
    for s in 0..16 {
        kit909.steps[s][CH] = if s % 4 == 2 {
            DrumHit::Accent
        } else {
            DrumHit::On
        };
    }
    for s in [2, 6, 10, 14] {
        kit909.steps[s][OH] = DrumHit::On;
        kit909.steps[s][CH] = DrumHit::Off;
    }
    pattern.roll = kit909.to_roll();

    let set = |p: &mut Project, id: ParamId, v: f32| p.set_param(id, v);
    set(&mut p, s1.param(SynthParam::Cutoff), 0.3);
    set(&mut p, s1.param(SynthParam::Resonance), 0.85);
    set(&mut p, s1.param(SynthParam::EnvMod), 0.7);
    set(&mut p, s1.param(SynthParam::Decay), 0.35);
    set(&mut p, s1.param(SynthParam::Accent), 0.8);
    set(&mut p, s2.param(SynthParam::Wave), 1.0);
    set(&mut p, s2.param(SynthParam::Unison), 1.0);
    set(&mut p, s2.param(SynthParam::Cutoff), 0.45);
    set(&mut p, s1.param(ChannelParam::DistOn), 1.0);
    set(&mut p, s1.param(ChannelParam::DistDrive), 0.35);
    set(&mut p, s1.param(ChannelParam::DelaySend), 0.3);
    set(&mut p, s2.param(ChannelParam::Level), 0.5);
    set(&mut p, s2.param(ChannelParam::Pan), 0.65);
    set(&mut p, s2.param(ChannelParam::ReverbSend), 0.4);
    set(&mut p, k808.param(ChannelParam::ReverbSend), 0.1);
    set(&mut p, k909.param(ChannelParam::Level), 0.55);
    set(&mut p, ParamId::Fx(FxParam::DelayFeedback), 0.45);
    set(&mut p, ParamId::Master(MasterParam::Level), 0.7);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_is_valid_and_nonempty() {
        let p = demo_project();
        p.validate().unwrap();
        let s = &p.synth(standard_rack()[0].0).unwrap().patterns[0];
        assert!(s.roll.notes.iter().any(|n| n.slide));
        assert!(s.roll.notes.iter().any(|n| n.accent));
    }

    #[test]
    fn demo_render_stays_below_full_scale() {
        let (l, r) = crate::render::render_patterns(&demo_project(), 8);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.99, "peak = {peak}");
    }
}
