//! Seeded pattern randomisers (spec §11). Pure functions: the same seed
//! always gives the same pattern, so tests are deterministic.

use crate::drums::{BD, CH, CP, OH, SD};
use crate::project::{DrumHit, DrumPattern, Gate, SynthPattern, steps_of};
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

/// Steps where the paired drum pattern has a kick or snare.
fn groove_hits(groove: &DrumPattern, len: usize) -> Vec<bool> {
    let glen = steps_of(groove.length);
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
    let len = steps_of(p.length);
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

const KICKS: [&[usize]; 4] = [&[0, 4, 8, 12], &[0, 3, 6, 10], &[0, 6, 8, 11], &[0, 7, 10]];
/// (closed hat steps, open hat steps)
const HATS: [(&[usize], &[usize]); 4] = [
    (&[0, 4, 8, 12], &[2, 6, 10, 14]),
    (&[0, 2, 4, 6, 8, 10, 12, 14], &[]),
    (&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15], &[]),
    (&[0, 3, 4, 7, 8, 11, 12, 15], &[14]),
];
/// How likely each instrument is to gain a hit when evolving.
const INST_WEIGHTS: [f32; 8] = [1.0, 0.8, 0.4, 0.4, 0.5, 0.4, 2.0, 1.0];
/// Downbeats over offbeats over the rest.
const STEP_WEIGHTS: [f32; 4] = [3.0, 1.0, 2.0, 1.0];

fn open_mutes_closed(p: &mut DrumPattern) {
    for step in &mut p.steps {
        if step[OH] != DrumHit::Off {
            step[CH] = DrumHit::Off;
        }
    }
}

fn weighted(rng: &mut Pcg32, weights: &[f32]) -> usize {
    let total: f32 = weights.iter().sum();
    let mut r = rng.gen_range(0.0..total);
    for (i, w) in weights.iter().enumerate() {
        if r < *w {
            return i;
        }
        r -= w;
    }
    weights.len() - 1
}

fn random_hit(rng: &mut Pcg32, p: &DrumPattern, len: usize) -> Option<(usize, usize)> {
    let count = p.steps[..len]
        .iter()
        .flatten()
        .filter(|h| **h != DrumHit::Off)
        .count();
    if count == 0 {
        return None;
    }
    let mut k = rng.gen_range(0..count);
    for (step, row) in p.steps[..len].iter().enumerate() {
        for (inst, hit) in row.iter().enumerate() {
            if *hit != DrumHit::Off {
                if k == 0 {
                    return Some((step, inst));
                }
                k -= 1;
            }
        }
    }
    None
}

pub fn drum_template(seed: u64, length: u8) -> DrumPattern {
    let mut rng = Pcg32::seed_from_u64(seed);
    let len = steps_of(length);
    let mut p = DrumPattern {
        length: len as u8,
        ..DrumPattern::default()
    };
    let kick = if rng.gen_bool(0.5) {
        KICKS[0]
    } else {
        KICKS[rng.gen_range(1..KICKS.len())]
    };
    let backbeat = if rng.gen_bool(0.5) { SD } else { CP };
    let (closed, open) = HATS[rng.gen_range(0..HATS.len())];
    for block in (0..len).step_by(16) {
        let mut put = |step: usize, inst: usize, hit: DrumHit| {
            if block + step < len {
                p.steps[block + step][inst] = hit;
            }
        };
        for &s in kick {
            put(s, BD, if s == 0 { DrumHit::Accent } else { DrumHit::On });
        }
        for s in [4, 12] {
            put(s, backbeat, DrumHit::On);
        }
        for &s in closed {
            put(s, CH, DrumHit::On);
        }
        for &s in open {
            put(s, OH, DrumHit::On);
        }
    }
    open_mutes_closed(&mut p);
    p
}

pub fn evolve_drums(p: &DrumPattern, seed: u64) -> DrumPattern {
    let mut rng = Pcg32::seed_from_u64(seed);
    let mut out = p.clone();
    let len = steps_of(p.length);
    let step_weights: Vec<f32> = (0..len)
        .map(|i| STEP_WEIGHTS[i % STEP_WEIGHTS.len()])
        .collect();
    for _ in 0..rng.gen_range(1..=3) {
        let op: f32 = rng.r#gen();
        if op < 0.5 {
            let inst = weighted(&mut rng, &INST_WEIGHTS);
            let step = weighted(&mut rng, &step_weights);
            if out.steps[step][inst] == DrumHit::Off {
                out.steps[step][inst] = DrumHit::On;
            }
        } else if op < 0.8 {
            if let Some((step, inst)) = random_hit(&mut rng, &out, len) {
                let to = if rng.gen_bool(0.5) {
                    (step + 1) % len
                } else {
                    (step + len - 1) % len
                };
                if out.steps[to][inst] == DrumHit::Off {
                    out.steps[to][inst] = out.steps[step][inst];
                    out.steps[step][inst] = DrumHit::Off;
                }
            }
        } else if let Some((step, inst)) = random_hit(&mut rng, &out, len) {
            out.steps[step][inst] = match out.steps[step][inst] {
                DrumHit::On => DrumHit::Accent,
                _ => DrumHit::On,
            };
        }
    }
    open_mutes_closed(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drums::BD;
    use crate::drums::{CH, CP, OH};
    use crate::project::SynthStep;

    fn hits(p: &DrumPattern) -> usize {
        p.steps
            .iter()
            .flatten()
            .filter(|h| **h != DrumHit::Off)
            .count()
    }

    #[test]
    fn template_has_a_kick_on_one_and_a_backbeat() {
        for seed in 0..50 {
            let p = drum_template(seed, 16);
            assert_eq!(p.length, 16);
            assert_eq!(p.steps[0][BD], DrumHit::Accent, "seed {seed}");
            for s in [4, 12] {
                assert!(
                    p.steps[s][SD] != DrumHit::Off || p.steps[s][CP] != DrumHit::Off,
                    "seed {seed}"
                );
            }
            assert!(
                p.steps
                    .iter()
                    .all(|s| !(s[OH] != DrumHit::Off && s[CH] != DrumHit::Off))
            );
        }
    }

    #[test]
    fn template_repeats_over_longer_patterns() {
        let p = drum_template(3, 32);
        assert_eq!(p.length, 32);
        assert_eq!(p.steps[16][BD], DrumHit::Accent);
        assert_eq!(&p.steps[..16], &p.steps[16..32]);
        assert!(p.steps[32..].iter().flatten().all(|h| *h == DrumHit::Off));
    }

    #[test]
    fn template_is_deterministic() {
        assert_eq!(drum_template(9, 16), drum_template(9, 16));
    }

    #[test]
    fn evolve_makes_small_deterministic_changes() {
        let base = drum_template(1, 16);
        let mut changed = 0;
        for seed in 0..50 {
            let e = evolve_drums(&base, seed);
            assert_eq!(e, evolve_drums(&base, seed));
            let diff = base
                .steps
                .iter()
                .flatten()
                .zip(e.steps.iter().flatten())
                .filter(|(a, b)| a != b)
                .count();
            assert!(diff <= 9, "seed {seed}: {diff} cells changed"); // 3 ops × (2 cells + a muted closed hat)
            if diff > 0 {
                changed += 1;
            }
            assert!(
                e.steps
                    .iter()
                    .all(|s| !(s[OH] != DrumHit::Off && s[CH] != DrumHit::Off))
            );
            assert!(
                e.steps[16..].iter().flatten().all(|h| *h == DrumHit::Off),
                "stays inside the length"
            );
        }
        assert!(
            changed >= 40,
            "evolve usually changes something ({changed}/50)"
        );
    }

    #[test]
    fn evolving_an_empty_pattern_adds_hits() {
        let e = evolve_drums(&DrumPattern::default(), 11);
        assert!(hits(&e) >= 1);
    }

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
