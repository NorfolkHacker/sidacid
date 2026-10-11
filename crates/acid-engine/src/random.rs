//! Seeded pattern randomisers (spec §11). Pure functions: the same seed
//! always gives the same pattern, so tests are deterministic.

use crate::drums::{BD, SD};
use crate::project::{DrumHit, DrumPattern, Gate, STEPS, SynthPattern};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dice {
    pub notes: bool,
    pub accent: bool,
    pub slide: bool,
    pub rest: bool,
    pub octave: bool,
}

impl Dice {
    pub const ALL: Dice = Dice {
        notes: true,
        accent: true,
        slide: true,
        rest: true,
        octave: true,
    };
}

/// (scale degrees in semitones, selection weight)
const SCALES: [(&[i32], f32); 5] = [
    (&[0, 2, 3, 5, 7, 8, 10], 2.0), // natural minor
    (&[0, 2, 3, 5, 7, 9, 11], 2.0), // melodic minor
    (&[0, 1, 3, 5, 7, 8, 10], 1.0), // phrygian
    (&[0, 2, 4, 5, 7, 9, 11], 1.0), // major
    (&[0, 2, 3, 6, 7, 8, 11], 0.5), // hungarian minor
];
const MAX_LEAP: i32 = 7;
const LOWEST: i32 = -12;
const HIGHEST: i32 = 23;

fn pick_scale(rng: &mut Pcg32) -> &'static [i32] {
    let total: f32 = SCALES.iter().map(|(_, w)| w).sum();
    let mut r = rng.gen_range(0.0..total);
    for (scale, weight) in SCALES {
        if r < weight {
            return scale;
        }
        r -= weight;
    }
    SCALES[0].0
}

fn degree_to_semitone(scale: &[i32], degree: i32) -> i32 {
    let n = scale.len() as i32;
    scale[degree.rem_euclid(n) as usize] + 12 * degree.div_euclid(n)
}

fn length_of(len: u8) -> usize {
    (len as usize).clamp(1, STEPS)
}

/// Steps where the paired drum pattern has a kick or snare.
fn groove_hits(groove: &DrumPattern, len: usize) -> Vec<bool> {
    let glen = length_of(groove.length);
    (0..len)
        .map(|i| {
            let step = groove.steps[i % glen];
            step[BD] != DrumHit::Off || step[SD] != DrumHit::Off
        })
        .collect()
}

pub fn randomise_synth(
    p: &SynthPattern,
    dice: Dice,
    groove: &DrumPattern,
    seed: u64,
) -> SynthPattern {
    let mut rng = Pcg32::seed_from_u64(seed);
    let mut out = p.clone();
    let len = length_of(p.length);
    let hits = groove_hits(groove, len);
    if dice.rest {
        for i in 0..len {
            let prob = if hits[i] { 0.85 } else { 0.5 };
            out.steps[i].gate = if rng.gen_bool(prob) {
                Gate::On
            } else {
                Gate::Off
            };
        }
    }
    if dice.notes {
        let scale = pick_scale(&mut rng);
        let mut degree = 0i32;
        let mut prev: Option<i32> = None;
        for step in out.steps.iter_mut().take(len) {
            let r: f32 = rng.r#gen();
            degree = if r < 0.35 {
                0
            } else if r < 0.55 {
                degree
            } else if r < 0.70 {
                degree - 1
            } else if r < 0.85 {
                degree + 2
            } else {
                rng.gen_range(-3..=7)
            };
            let mut semis = degree_to_semitone(scale, degree);
            if let Some(p) = prev {
                while semis - p > MAX_LEAP {
                    semis -= 12;
                }
                while p - semis > MAX_LEAP {
                    semis += 12;
                }
            }
            while semis < LOWEST {
                semis += 12;
            }
            while semis > HIGHEST {
                semis -= 12;
            }
            prev = Some(semis);
            step.octave = semis.div_euclid(12) as i8;
            step.note = semis.rem_euclid(12) as u8;
        }
    }
    if dice.octave {
        for step in out.steps.iter_mut().take(len) {
            let r: f32 = rng.r#gen();
            if r < 0.08 {
                step.octave = (step.octave + 1).min(1);
            } else if r < 0.12 {
                step.octave = (step.octave - 1).max(-1);
            }
        }
    }
    if dice.slide {
        let mut prev = false;
        for step in out.steps.iter_mut().take(len) {
            let on = rng.gen_bool(if prev { 0.5 } else { 0.15 });
            step.slide = on;
            prev = on;
        }
    }
    if dice.accent {
        for i in 0..len {
            out.steps[i].accent = rng.gen_bool(if hits[i] { 0.35 } else { 0.2 });
        }
    }
    if dice.accent || dice.slide {
        for i in 1..len {
            if out.steps[i - 1].slide {
                out.steps[i].accent = false;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drums::BD;
    use crate::project::SynthStep;

    fn groove_four_on_floor() -> DrumPattern {
        let mut g = DrumPattern::default();
        for s in [0, 4, 8, 12] {
            g.steps[s][BD] = DrumHit::On;
        }
        g
    }

    fn semis(s: &SynthStep) -> i32 {
        s.octave as i32 * 12 + s.note as i32
    }

    #[test]
    fn same_seed_same_pattern_different_seed_different() {
        let base = SynthPattern::default();
        let g = groove_four_on_floor();
        let a = randomise_synth(&base, Dice::ALL, &g, 7);
        assert_eq!(a, randomise_synth(&base, Dice::ALL, &g, 7));
        assert_ne!(a, randomise_synth(&base, Dice::ALL, &g, 8));
    }

    #[test]
    fn no_dice_changes_nothing() {
        let base = crate::demo::demo_project().synths[0].patterns[0].clone();
        assert_eq!(
            randomise_synth(&base, Dice::default(), &groove_four_on_floor(), 1),
            base
        );
    }

    #[test]
    fn each_die_touches_only_its_field() {
        let base = crate::demo::demo_project().synths[0].patterns[0].clone();
        let g = groove_four_on_floor();
        let r = randomise_synth(
            &base,
            Dice {
                rest: true,
                ..Dice::default()
            },
            &g,
            3,
        );
        for (a, b) in base.steps.iter().zip(&r.steps) {
            assert_eq!(
                (a.note, a.octave, a.accent, a.slide),
                (b.note, b.octave, b.accent, b.slide)
            );
        }
        let r = randomise_synth(
            &base,
            Dice {
                notes: true,
                ..Dice::default()
            },
            &g,
            3,
        );
        for (a, b) in base.steps.iter().zip(&r.steps) {
            assert_eq!((a.gate, a.accent, a.slide), (b.gate, b.accent, b.slide));
        }
    }

    #[test]
    fn notes_stay_in_range_and_leaps_are_folded() {
        let g = groove_four_on_floor();
        for seed in 0..200 {
            let r = randomise_synth(&SynthPattern::default(), Dice::ALL, &g, seed);
            for s in r.steps.iter().take(16) {
                assert!(
                    s.note < 12 && (-1..=1).contains(&s.octave),
                    "seed {seed}: {s:?}"
                );
            }
            let n = randomise_synth(
                &SynthPattern::default(),
                Dice {
                    notes: true,
                    ..Dice::default()
                },
                &g,
                seed,
            );
            for w in n.steps[..16].windows(2) {
                assert!((semis(&w[1]) - semis(&w[0])).abs() <= 12, "seed {seed}");
            }
        }
    }

    #[test]
    fn never_an_accent_directly_after_a_slide() {
        let g = groove_four_on_floor();
        for seed in 0..300 {
            let r = randomise_synth(&SynthPattern::default(), Dice::ALL, &g, seed);
            for i in 1..16 {
                assert!(
                    !(r.steps[i - 1].slide && r.steps[i].accent),
                    "seed {seed} step {i}"
                );
            }
        }
    }

    #[test]
    fn gates_lean_toward_the_groove() {
        let g = groove_four_on_floor();
        let (mut on_hit, mut on_off) = (0, 0);
        for seed in 0..300 {
            let r = randomise_synth(
                &SynthPattern::default(),
                Dice {
                    rest: true,
                    ..Dice::default()
                },
                &g,
                seed,
            );
            for i in 0..16 {
                if r.steps[i].gate != Gate::Off {
                    if i % 4 == 0 { on_hit += 1 } else { on_off += 1 }
                }
            }
        }
        let hit_rate = on_hit as f32 / (300.0 * 4.0);
        let off_rate = on_off as f32 / (300.0 * 12.0);
        assert!(hit_rate > off_rate + 0.2, "hit {hit_rate} off {off_rate}");
    }

    #[test]
    fn steps_beyond_the_length_are_untouched() {
        let base = SynthPattern {
            length: 8,
            ..SynthPattern::default()
        };
        let r = randomise_synth(&base, Dice::ALL, &groove_four_on_floor(), 5);
        assert_eq!(&r.steps[8..], &base.steps[8..]);
    }
}
