//! Built-in Mono patches: original sounds, stored as the knobs that differ
//! from the defaults (`MonoParam::info().default`).

use crate::params::MonoParam;
use crate::params::MonoParam::*;

pub struct MonoPreset {
    pub name: &'static str,
    pub values: &'static [(MonoParam, f32)],
}

impl MonoPreset {
    /// Every Mono param: the preset's value, else the default.
    pub fn to_params(&self) -> Vec<(MonoParam, f32)> {
        MonoParam::ALL
            .iter()
            .map(|&p| {
                let v = self.values.iter().find(|(q, _)| *q == p);
                (p, v.map_or(p.info().default, |&(_, v)| v))
            })
            .collect()
    }
}

// Switch positions as knob values: Range 16'/8'/4'/2' = 0, 0.333, 0.667, 1;
// three-way switches 0, 0.5, 1; Arp Rate 1/4…1/32 = 0, 0.2, … 1 (1/16 = 0.6).
#[rustfmt::skip]
pub const STARTER: &[MonoPreset] = &[
    MonoPreset { name: "Sub Bass", values: &[
        (Range, 0.0), (SawLevel, 0.0), (SubLevel, 0.9), (PulseLevel, 0.3), (Cutoff, 0.3),
        (Resonance, 0.1), (EnvAmount, 0.2), (Decay, 0.5), (Sustain, 0.7), (Release, 0.15),
    ] },
    MonoPreset { name: "Squelch", values: &[
        (Range, 0.333), (SawLevel, 0.9), (Cutoff, 0.25), (Resonance, 0.75), (EnvAmount, 0.6),
        (Decay, 0.5), (Sustain, 0.0), (Release, 0.15), (Portamento, 1.0), (PortaTime, 0.25),
    ] },
    MonoPreset { name: "Pluck Bass", values: &[
        (Range, 0.0), (SawLevel, 0.0), (PulseLevel, 0.8), (PulseWidth, 0.4), (SubLevel, 0.5),
        (Cutoff, 0.2), (Resonance, 0.3), (EnvAmount, 0.7), (Decay, 0.45), (Sustain, 0.0),
        (Release, 0.1),
    ] },
    MonoPreset { name: "Rubber Bass", values: &[
        (Range, 0.0), (SawLevel, 0.6), (SubLevel, 0.6), (SubWave, 0.5), (Cutoff, 0.3),
        (Resonance, 0.5), (EnvAmount, 0.5), (Attack, 0.1), (Decay, 0.35), (Sustain, 0.3),
        (Portamento, 0.5), (PortaTime, 0.3),
    ] },
    MonoPreset { name: "Saw Lead", values: &[
        (Range, 0.333), (SawLevel, 0.9), (Cutoff, 0.6), (Resonance, 0.3), (EnvAmount, 0.3),
        (Attack, 0.05), (Decay, 0.5), (Sustain, 0.8), (Release, 0.35), (VcoMod, 0.12),
        (LfoRate, 0.5), (Portamento, 1.0), (PortaTime, 0.3),
    ] },
    MonoPreset { name: "Square Lead", values: &[
        (Range, 0.333), (SawLevel, 0.0), (PulseLevel, 0.9), (Cutoff, 0.55), (Resonance, 0.2),
        (EnvAmount, 0.25), (Sustain, 0.9), (Release, 0.3),
    ] },
    MonoPreset { name: "PWM Lead", values: &[
        (Range, 0.333), (SawLevel, 0.0), (PulseLevel, 0.9), (PulseWidth, 0.7), (PwmSource, 0.5),
        (LfoRate, 0.35), (Cutoff, 0.6), (Resonance, 0.25), (EnvAmount, 0.2), (Sustain, 0.85),
        (Release, 0.4), (ChorusMix, 0.3),
    ] },
    MonoPreset { name: "Chorus Lead", values: &[
        (Range, 0.667), (SawLevel, 0.7), (PulseLevel, 0.5), (PulseWidth, 0.3), (Cutoff, 0.65),
        (Resonance, 0.35), (EnvAmount, 0.3), (Attack, 0.15), (Sustain, 0.8), (Release, 0.5),
        (ChorusMix, 0.7), (ChorusDepth, 0.7),
    ] },
    MonoPreset { name: "Blip", values: &[
        (Range, 0.667), (SawLevel, 0.0), (PulseLevel, 0.9), (PulseWidth, 0.6), (Cutoff, 0.4),
        (Resonance, 0.6), (EnvAmount, 0.7), (Decay, 0.4), (Sustain, 0.0), (Release, 0.08),
    ] },
    MonoPreset { name: "Zap", values: &[
        (Range, 0.667), (SawLevel, 0.9), (Cutoff, 0.2), (Resonance, 0.85), (EnvAmount, 1.0),
        (Decay, 0.5), (Sustain, 0.0), (Release, 0.1), (KeyFollow, 1.0),
    ] },
    MonoPreset { name: "Noise Snare", values: &[
        (Range, 0.667), (SawLevel, 0.0), (NoiseLevel, 0.9), (PulseLevel, 0.3), (Cutoff, 0.55),
        (Resonance, 0.3), (EnvAmount, 0.4), (Decay, 0.45), (Sustain, 0.0), (Release, 0.15),
        (KeyFollow, 0.0),
    ] },
    MonoPreset { name: "Arp Pluck", values: &[
        (Range, 0.333), (SawLevel, 0.6), (PulseLevel, 0.6), (PulseWidth, 0.5), (Cutoff, 0.45),
        (Resonance, 0.55), (EnvAmount, 0.6), (Decay, 0.5), (Sustain, 0.0), (Release, 0.12),
        (ArpOn, 1.0), (ArpMode, 0.5), (ArpRange, 0.5), (ArpRate, 0.6), (ChorusMix, 0.3),
    ] },
    MonoPreset { name: "Res Sweep", values: &[
        (Range, 0.333), (SawLevel, 0.8), (NoiseLevel, 0.15), (Cutoff, 0.15), (Resonance, 0.9),
        (EnvAmount, 0.8), (Attack, 0.6), (Decay, 0.75), (Sustain, 0.4), (Release, 0.6),
        (VcfMod, 0.2), (LfoRate, 0.3),
    ] },
    MonoPreset { name: "Drift Pad", values: &[
        (Range, 0.333), (SawLevel, 0.7), (PulseLevel, 0.4), (PulseWidth, 0.5), (PwmSource, 0.5),
        (LfoRate, 0.25), (Cutoff, 0.45), (Resonance, 0.2), (EnvAmount, 0.2), (Attack, 0.55),
        (Decay, 0.6), (Sustain, 0.8), (Release, 0.6), (Drift, 0.7), (ChorusMix, 0.6),
    ] },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::rms;
    use crate::va::MonoSynth;
    use std::collections::HashSet;

    #[test]
    fn the_bank_is_well_formed() {
        assert!((12..=16).contains(&STARTER.len()));
        let names: HashSet<_> = STARTER.iter().map(|p| p.name).collect();
        assert_eq!(names.len(), STARTER.len(), "unique names");
        for p in STARTER {
            let params: HashSet<_> = p.values.iter().map(|(m, _)| *m).collect();
            assert_eq!(
                params.len(),
                p.values.len(),
                "{}: a param listed twice",
                p.name
            );
            assert!(
                p.values.iter().all(|(_, v)| (0.0..=1.0).contains(v)),
                "{}",
                p.name
            );
            assert_eq!(p.to_params().len(), MonoParam::ALL.len());
        }
    }

    #[test]
    fn every_patch_makes_sound() {
        for p in STARTER {
            let mut s = MonoSynth::new(48_000.0);
            for (m, v) in p.to_params() {
                s.set_param(m, v);
            }
            s.note_on(48, 100, true);
            let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
            s.render(&mut l, &mut r);
            let loudest = (0..=24_000 - 4800)
                .step_by(480)
                .map(|i| rms(&l[i..i + 4800]))
                .fold(0.0_f32, f32::max);
            assert!(loudest > 0.01, "{} is too quiet: {}", p.name, loudest);
        }
    }
}
