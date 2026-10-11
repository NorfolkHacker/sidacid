//! A built-in demo groove: a squelchy acid line on synth 1, a higher
//! counter-line on synth 2, an 808 beat and 909 hats.

use crate::drums::{BD, CH, CP, OH, SD};
use crate::params::{ChannelParam, FxParam, MasterParam, ParamId, SynthParam};
use crate::project::{DrumHit, Gate, Project, SynthStep};

fn step(note: u8, octave: i8, accent: bool, slide: bool) -> SynthStep {
    SynthStep { note, octave, gate: Gate::On, accent, slide }
}

const REST: SynthStep = SynthStep { note: 0, octave: 0, gate: Gate::Off, accent: false, slide: false };

pub fn demo_project() -> Project {
    let mut p = Project { name: "Demo".into(), bpm: 128.0, swing: 0.15, ..Project::default() };

    let acid = [
        step(0, 0, false, false), step(0, 1, true, false), step(0, 0, false, true), step(3, 0, false, false),
        step(0, 0, true, false), step(10, -1, false, true), REST, step(7, 0, true, false),
        step(0, 0, false, false), step(0, 1, false, true), step(0, 1, true, false), step(3, 0, false, false),
        step(5, 0, false, true), step(3, 0, true, false), REST, step(10, -1, false, false),
    ];
    p.synths[0].patterns[0].steps[..16].copy_from_slice(&acid);

    let counter = [
        step(7, 1, false, false), REST, REST, step(10, 1, true, true),
        step(7, 1, false, false), REST, step(3, 1, false, false), REST,
        step(7, 1, true, false), REST, REST, step(5, 1, false, true),
        step(3, 1, false, false), REST, step(0, 1, true, false), REST,
    ];
    p.synths[1].patterns[0].steps[..16].copy_from_slice(&counter);

    let kit808 = &mut p.drums[0].patterns[0];
    for s in [0, 4, 8, 12] {
        kit808.steps[s][BD] = DrumHit::On;
    }
    kit808.steps[4][SD] = DrumHit::On;
    kit808.steps[12][CP] = DrumHit::Accent;
    kit808.steps[15][SD] = DrumHit::On;

    let kit909 = &mut p.drums[1].patterns[0];
    for s in 0..16 {
        kit909.steps[s][CH] = if s % 4 == 2 { DrumHit::Accent } else { DrumHit::On };
    }
    for s in [2, 6, 10, 14] {
        kit909.steps[s][OH] = DrumHit::On;
        kit909.steps[s][CH] = DrumHit::Off;
    }

    let set = |p: &mut Project, id: ParamId, v: f32| {
        p.params.insert(id, v);
    };
    set(&mut p, ParamId::Synth(0, SynthParam::Cutoff), 0.3);
    set(&mut p, ParamId::Synth(0, SynthParam::Resonance), 0.85);
    set(&mut p, ParamId::Synth(0, SynthParam::EnvMod), 0.7);
    set(&mut p, ParamId::Synth(0, SynthParam::Decay), 0.35);
    set(&mut p, ParamId::Synth(0, SynthParam::Accent), 0.8);
    set(&mut p, ParamId::Synth(1, SynthParam::Wave), 1.0);
    set(&mut p, ParamId::Synth(1, SynthParam::Unison), 1.0);
    set(&mut p, ParamId::Synth(1, SynthParam::Cutoff), 0.45);
    set(&mut p, ParamId::Channel(0, ChannelParam::DistOn), 1.0);
    set(&mut p, ParamId::Channel(0, ChannelParam::DistDrive), 0.35);
    set(&mut p, ParamId::Channel(0, ChannelParam::DelaySend), 0.3);
    set(&mut p, ParamId::Channel(1, ChannelParam::Level), 0.5);
    set(&mut p, ParamId::Channel(1, ChannelParam::Pan), 0.65);
    set(&mut p, ParamId::Channel(1, ChannelParam::ReverbSend), 0.4);
    set(&mut p, ParamId::Channel(2, ChannelParam::ReverbSend), 0.1);
    set(&mut p, ParamId::Channel(3, ChannelParam::Level), 0.55);
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
        assert!(p.synths[0].patterns[0].steps.iter().any(|s| s.slide));
        assert!(p.synths[0].patterns[0].steps.iter().any(|s| s.accent));
    }

    #[test]
    fn demo_render_stays_below_full_scale() {
        let (l, r) = crate::render::render_patterns(&demo_project(), 8);
        let peak = l.iter().chain(&r).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.99, "peak = {peak}");
    }
}
