//! Seeded pattern randomisers (spec §11). Pure functions: the same seed
//! always gives the same pattern, so tests are deterministic. Synth and
//! drum dice work on the 1/16 grid: a roll snaps off-grid notes to it.

use crate::drums::{BD, CH, CP, OH, SD};
use crate::fm::algorithms::ALGORITHMS;
use crate::fm::sysex::{Dx7Op, Dx7Voice};
use crate::params::DRUM_INSTRUMENTS;
use crate::params::MonoParam;
use crate::project::{DrumPattern, STEPS, SynthPattern, steps_of};
use crate::steps::{DrumGrid, DrumHit, Gate, SynthGrid};
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
fn groove_hits(groove: &DrumGrid, len: usize) -> Vec<bool> {
    let glen = steps_of(groove.length);
    (0..len)
        .map(|i| {
            let step = groove.steps[i % glen];
            step[BD] != DrumHit::Off || step[SD] != DrumHit::Off
        })
        .collect()
}

/// A rest has no pitch: each step the rest die turned on (`was_off`)
/// takes the pitch of the nearest earlier step that still plays a note it
/// had, wrapping within `len`; with none, it stays C.
fn repeat_earlier_pitches(grid: &mut SynthGrid, was_off: &[bool], len: usize) {
    let kept = |g: &SynthGrid, j: usize| !was_off[j] && g.steps[j].gate != Gate::Off;
    for i in 0..len {
        if !was_off[i] || grid.steps[i].gate == Gate::Off {
            continue;
        }
        if let Some(j) = (1..len)
            .map(|k| (i + len - k) % len)
            .find(|&j| kept(grid, j))
        {
            let src = grid.steps[j];
            grid.steps[i].note = src.note;
            grid.steps[i].octave = src.octave;
        }
    }
}

pub fn randomise_synth(
    p: &SynthPattern,
    dice: Dice,
    groove: &DrumPattern,
    seed: u64,
) -> SynthPattern {
    let mut rng = Pcg32::seed_from_u64(seed);
    let mut grid = SynthGrid::from_roll(&p.roll);
    let groove = DrumGrid::from_roll(&groove.roll);
    let len = steps_of(p.roll.length);
    let hits = groove_hits(&groove, len);
    if dice.rest {
        let was_off: Vec<bool> = grid.steps[..len]
            .iter()
            .map(|s| s.gate == Gate::Off)
            .collect();
        for i in 0..len {
            let prob = if hits[i] { 0.85 } else { 0.5 };
            grid.steps[i].gate = if rng.gen_bool(prob) {
                Gate::On
            } else {
                Gate::Off
            };
        }
        if !dice.notes {
            repeat_earlier_pitches(&mut grid, &was_off, len);
        }
    }
    if dice.notes {
        let scale = pick_scale(&mut rng);
        let mut degree = 0i32;
        let mut prev: Option<i32> = None;
        for step in grid.steps.iter_mut().take(len) {
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
            // The range clamp can undo the fold at the edges; repeating
            // the previous (in-scale, in-range) note keeps the leap legal.
            if let Some(p) = prev
                && (semis - p).abs() > MAX_LEAP
            {
                semis = p;
            }
            prev = Some(semis);
            step.octave = semis.div_euclid(12) as i8;
            step.note = semis.rem_euclid(12) as u8;
        }
    }
    if dice.octave {
        for step in grid.steps.iter_mut().take(len) {
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
        for step in grid.steps.iter_mut().take(len) {
            let on = rng.gen_bool(if prev { 0.5 } else { 0.15 });
            step.slide = on;
            prev = on;
        }
    }
    if dice.accent {
        for i in 0..len {
            grid.steps[i].accent = rng.gen_bool(if hits[i] { 0.35 } else { 0.2 });
        }
    }
    if dice.accent || dice.slide {
        for i in 1..len {
            if grid.steps[i - 1].slide {
                grid.steps[i].accent = false;
            }
        }
    }
    let mut out = p.clone();
    out.roll = grid.to_roll();
    out
}

/// Operator ratios a random voice picks from (DX7 coarse; 0 is ×0.5):
/// carriers stay near the played pitch, modulators range further.
const CARRIER_RATIOS: [u8; 5] = [0, 1, 1, 1, 2];
const MODULATOR_RATIOS: [u8; 9] = [0, 1, 1, 2, 2, 3, 4, 5, 7];
/// Bright voices also reach for the odd, clangy ratios.
const BRIGHT_RATIOS: [u8; 10] = [1, 2, 3, 3, 5, 7, 9, 11, 13, 17];
/// LFO waves that pulse rather than swell: saw down, saw up, square, S&H.
const PULSE_WAVES: [u8; 4] = [1, 2, 3, 5];

/// What a random voice is going for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Character {
    /// A clean tone: gentle modulation, little movement.
    Tone,
    /// Strong modulators, odd and fractional ratios, high feedback.
    Bright,
    /// A sustained body gated by a fast LFO, sometimes with stepped pitch
    /// or a slow fixed-frequency modulator wobbling the timbre.
    Rhythmic,
}

fn random_op(rng: &mut Pcg32, carrier: bool, character: Character) -> Dx7Op {
    use Character::*;
    let ratios: &[u8] = match (carrier, character) {
        (true, _) => &CARRIER_RATIOS,
        (false, Bright) => &BRIGHT_RATIOS,
        (false, _) => &MODULATOR_RATIOS,
    };
    // Rhythmic carriers hold their level so the LFO has something to gate;
    // bright modulators hold theirs so the brightness lasts.
    let sustain = (carrier && character == Rhythmic) || (!carrier && character == Bright);
    Dx7Op {
        // A carrier attacks quickly to full level so every note is heard;
        // every envelope ends at 0 so notes stop after release.
        rates: [
            if carrier {
                rng.gen_range(60..=99)
            } else {
                rng.gen_range(30..=99)
            },
            rng.gen_range(20..=99),
            rng.gen_range(20..=99),
            rng.gen_range(30..=80),
        ],
        levels: [
            if carrier { 99 } else { rng.gen_range(60..=99) },
            rng.gen_range(
                if sustain {
                    85
                } else if carrier {
                    60
                } else {
                    0
                }..=99,
            ),
            rng.gen_range(if sustain { 80 } else { 0 }..=99),
            0,
        ],
        // No keyboard level scaling: it could silence an operator in
        // part of the range.
        break_point: 39,
        left_depth: 0,
        right_depth: 0,
        left_curve: 0,
        right_curve: 0,
        rate_scaling: rng.gen_range(0..=if carrier { 3 } else { 7 }),
        // Rhythmic voices gate every carrier hard and pulse some modulators.
        amp_mod_sens: match (character, carrier) {
            (Rhythmic, true) => 3,
            (Rhythmic, false) => rng.gen_range(0..=3),
            (_, true) => rng.gen_range(0..=1),
            (_, false) => rng.gen_range(0..=3),
        },
        vel_sens: rng.gen_range(0..=if carrier { 3 } else { 7 }),
        // Levels are logarithmic: below about 60 a modulator is barely heard.
        level: match (carrier, character) {
            (true, _) => rng.gen_range(85..=99),
            (false, Tone) => rng.gen_range(0..=85),
            (false, Bright) => rng.gen_range(75..=97),
            (false, Rhythmic) => rng.gen_range(65..=92),
        },
        fixed: false,
        coarse: ratios[rng.gen_range(0..ratios.len())],
        // Bright modulators sometimes sit off the harmonic series.
        fine: if !carrier && character == Bright && rng.gen_bool(0.4) {
            rng.gen_range(1..=99)
        } else {
            0
        },
        detune: if carrier {
            rng.gen_range(4..=10)
        } else {
            rng.gen_range(3..=11)
        },
    }
}

/// Turns `op` into a fixed-frequency modulator: `low` gives 1..10 Hz (a
/// slow wobble), otherwise 100..1000 Hz (an inharmonic clang).
fn fix_op(rng: &mut Pcg32, op: &mut Dx7Op, low: bool) {
    op.fixed = true;
    op.coarse = if low { 0 } else { 2 };
    op.fine = rng.gen_range(0..=99);
    op.level = rng.gen_range(80..=95);
}

/// A fresh FM sound for the FM panel's Randomise: a new algorithm,
/// feedback, operators, LFO and pitch EG that always plays. Each voice is
/// a plain tone, a bright one or a rhythmic one. `current`'s osc sync and
/// transpose are kept.
pub fn randomise_voice(current: &Dx7Voice, seed: u64) -> Dx7Voice {
    use Character::*;
    let mut rng = Pcg32::seed_from_u64(seed);
    let character = match rng.gen_range(0..10) {
        0..=2 => Tone,
        3..=5 => Bright,
        _ => Rhythmic,
    };
    // A bright voice needs modulators: at most three carriers.
    let algorithm = loop {
        let a = rng.gen_range(0..ALGORITHMS.len()) as u8;
        if character != Bright || ALGORITHMS[a as usize].carriers.count_ones() <= 3 {
            break a;
        }
    };
    let alg = ALGORITHMS[algorithm as usize];
    let mut ops =
        std::array::from_fn(|op| random_op(&mut rng, alg.carriers & (1 << op) != 0, character));
    let modulators: Vec<usize> = (0..6).filter(|&op| alg.carriers & (1 << op) == 0).collect();
    if !modulators.is_empty() {
        let fixed = modulators[rng.gen_range(0..modulators.len())];
        match character {
            Bright if rng.gen_bool(0.3) => fix_op(&mut rng, &mut ops[fixed], false),
            Rhythmic if rng.gen_bool(0.3) => fix_op(&mut rng, &mut ops[fixed], true),
            _ => {}
        }
    }
    // Mostly no pitch envelope; sometimes a short blip that settles back
    // to the played pitch (level 50 is no shift).
    let (pitch_rates, pitch_levels) = if rng.gen_bool(0.2) {
        (
            [99, rng.gen_range(40..=99), 99, 99],
            [rng.gen_range(40..=60), 50, 50, 50],
        )
    } else {
        ([99; 4], [50; 4])
    };
    let mut v = Dx7Voice {
        ops,
        pitch_rates,
        pitch_levels,
        algorithm,
        feedback: if character == Bright {
            rng.gen_range(4..=7)
        } else {
            rng.gen_range(0..=7)
        },
        osc_sync: current.osc_sync,
        lfo_speed: rng.gen_range(0..=99),
        lfo_delay: rng.gen_range(0..=99),
        lfo_pmd: rng.gen_range(0..=10),
        lfo_amd: rng.gen_range(0..=20),
        lfo_sync: rng.gen_bool(0.5),
        lfo_wave: rng.gen_range(0..6),
        pms: rng.gen_range(0..=3),
        transpose: current.transpose,
        name: "RANDOM".into(),
    };
    if character == Rhythmic {
        // 2..10 Hz pulses that start with the note, restart on each key so
        // the rhythm lands the same way every time, and gate deeply.
        v.lfo_speed = rng.gen_range(55..=75);
        v.lfo_delay = 0;
        v.lfo_sync = true;
        v.lfo_wave = PULSE_WAVES[rng.gen_range(0..PULSE_WAVES.len())];
        v.lfo_amd = rng.gen_range(70..=99);
        // Sometimes the LFO steps the pitch too: trills, or octave jumps
        // on a square wave.
        if rng.gen_bool(0.35) {
            v.lfo_pmd = rng.gen_range(60..=99);
            v.pms = if v.lfo_wave == 3 && rng.gen_bool(0.5) {
                7
            } else {
                rng.gen_range(4..=6)
            };
        }
    }
    v
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
/// Upper bound on tries when looking for effective edits.
const MAX_EVOLVE_ATTEMPTS: u32 = 16;
/// Downbeats over offbeats over the rest.
const STEP_WEIGHTS: [f32; 4] = [3.0, 1.0, 2.0, 1.0];

fn open_mutes_closed(p: &mut DrumGrid, len: usize) {
    for step in &mut p.steps[..len] {
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

fn random_hit(rng: &mut Pcg32, p: &DrumGrid, len: usize) -> Option<(usize, usize)> {
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
    let mut p = DrumGrid {
        steps: vec![[DrumHit::Off; DRUM_INSTRUMENTS]; STEPS],
        length: len as u8,
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
    open_mutes_closed(&mut p, len);
    DrumPattern { roll: p.to_roll() }
}

pub fn evolve_drums(p: &DrumPattern, seed: u64) -> DrumPattern {
    let mut rng = Pcg32::seed_from_u64(seed);
    let mut out = DrumGrid::from_roll(&p.roll);
    let len = steps_of(p.roll.length);
    let step_weights: Vec<f32> = (0..len)
        .map(|i| STEP_WEIGHTS[i % STEP_WEIGHTS.len()])
        .collect();
    // Aim for 1..=3 edits that survive `open_mutes_closed`: a closed hat
    // on an open-hat step would be muted straight away, so it is skipped
    // (and retried) rather than counted.
    let target = rng.gen_range(1..=3);
    let (mut done, mut attempts) = (0, 0);
    while done < target && attempts < MAX_EVOLVE_ATTEMPTS {
        attempts += 1;
        let op: f32 = rng.r#gen();
        if op < 0.5 {
            let inst = weighted(&mut rng, &INST_WEIGHTS);
            let step = weighted(&mut rng, &step_weights);
            if out.steps[step][inst] == DrumHit::Off
                && !(inst == CH && out.steps[step][OH] != DrumHit::Off)
            {
                out.steps[step][inst] = DrumHit::On;
                done += 1;
            }
        } else if op < 0.8 {
            if let Some((step, inst)) = random_hit(&mut rng, &out, len) {
                let to = if rng.gen_bool(0.5) {
                    (step + 1) % len
                } else {
                    (step + len - 1) % len
                };
                if out.steps[to][inst] == DrumHit::Off
                    && !(inst == CH && out.steps[to][OH] != DrumHit::Off)
                {
                    out.steps[to][inst] = out.steps[step][inst];
                    out.steps[step][inst] = DrumHit::Off;
                    done += 1;
                }
            }
        } else if let Some((step, inst)) = random_hit(&mut rng, &out, len) {
            out.steps[step][inst] = match out.steps[step][inst] {
                DrumHit::On => DrumHit::Accent,
                _ => DrumHit::On,
            };
            done += 1;
        }
    }
    open_mutes_closed(&mut out, len);
    DrumPattern {
        roll: out.to_roll(),
    }
}

/// A new Mono sound from `seed`: one of four characters (bass, lead,
/// blip, sweep) with knob ranges that suit it, then guards so it always
/// plays. Volume is not returned (left alone); the arp, drift and chorus
/// are kept from `current`, except that a blip turns the arp on at 1/16.
pub fn randomise_mono(current: impl Fn(MonoParam) -> f32, seed: u64) -> Vec<(MonoParam, f32)> {
    use MonoParam as M;
    let mut rng = Pcg32::seed_from_u64(seed);
    let mut v: Vec<(MonoParam, f32)> = M::ALL
        .iter()
        .filter(|&&p| p != M::Volume)
        .map(|&p| (p, p.info().default))
        .collect();
    fn set(v: &mut [(MonoParam, f32)], p: MonoParam, x: f32) {
        if let Some(e) = v.iter_mut().find(|(q, _)| *q == p) {
            e.1 = x.clamp(0.0, 1.0);
        }
    }
    fn get(v: &[(MonoParam, f32)], p: MonoParam) -> f32 {
        v.iter().find(|(q, _)| *q == p).map_or(0.0, |e| e.1)
    }
    for p in [
        M::ArpOn,
        M::ArpMode,
        M::ArpRange,
        M::ArpRate,
        M::Drift,
        M::ChorusRate,
        M::ChorusDepth,
        M::ChorusMix,
    ] {
        set(&mut v, p, current(p));
    }
    let mut r = |lo: f32, hi: f32| rng.gen_range(lo..=hi);
    // (param, low, high) per character; switches pick a position.
    let ranges: &[(MonoParam, f32, f32)] = match r(0.0, 3.999) as u32 {
        0 => &[
            (M::Range, 0.0, 0.333),
            (M::SawLevel, 0.0, 1.0),
            (M::PulseLevel, 0.0, 0.8),
            (M::SubLevel, 0.4, 0.9),
            (M::PulseWidth, 0.0, 0.6),
            (M::Cutoff, 0.15, 0.45),
            (M::Resonance, 0.0, 0.7),
            (M::EnvAmount, 0.3, 0.8),
            (M::Attack, 0.0, 0.05),
            (M::Decay, 0.4, 0.65),
            (M::Sustain, 0.0, 0.4),
            (M::Release, 0.25, 0.45),
            (M::Portamento, 0.0, 1.0),
            (M::PortaTime, 0.1, 0.35),
        ],
        1 => &[
            (M::Range, 0.333, 0.667),
            (M::SawLevel, 0.3, 1.0),
            (M::PulseLevel, 0.0, 0.9),
            (M::PulseWidth, 0.0, 0.6),
            (M::PwmSource, 0.0, 0.5),
            (M::Cutoff, 0.35, 0.75),
            (M::Resonance, 0.0, 0.6),
            (M::EnvAmount, 0.1, 0.5),
            (M::Attack, 0.0, 0.2),
            (M::Decay, 0.3, 0.7),
            (M::Sustain, 0.5, 1.0),
            (M::Release, 0.2, 0.5),
            (M::VcoMod, 0.0, 0.15),
            (M::LfoRate, 0.45, 0.6),
            (M::Portamento, 0.0, 1.0),
            (M::PortaTime, 0.1, 0.4),
        ],
        2 => &[
            (M::Range, 0.667, 1.0),
            (M::SawLevel, 0.0, 0.5),
            (M::PulseLevel, 0.6, 1.0),
            (M::PulseWidth, 0.0, 0.7),
            (M::Cutoff, 0.3, 0.6),
            (M::Resonance, 0.4, 0.8),
            (M::EnvAmount, 0.5, 0.9),
            (M::Attack, 0.0, 0.0),
            (M::Decay, 0.3, 0.5),
            (M::Sustain, 0.0, 0.0),
            (M::Release, 0.25, 0.4),
            (M::ArpMode, 0.0, 1.0),
            (M::ArpRange, 0.0, 1.0),
        ],
        _ => &[
            (M::Range, 0.333, 0.333),
            (M::SawLevel, 0.5, 1.0),
            (M::NoiseLevel, 0.0, 0.3),
            (M::Cutoff, 0.1, 0.3),
            (M::Resonance, 0.7, 0.95),
            (M::EnvAmount, 0.6, 1.0),
            (M::Attack, 0.3, 0.7),
            (M::Decay, 0.5, 0.9),
            (M::Sustain, 0.2, 0.6),
            (M::Release, 0.4, 0.8),
            (M::VcfMod, 0.0, 0.3),
            (M::LfoRate, 0.2, 0.4),
        ],
    };
    let blip = ranges.iter().any(|&(p, _, _)| p == M::ArpMode);
    for &(p, lo, hi) in ranges {
        let x = r(lo, hi);
        let steps = p.info().steps;
        // Switches land exactly on a position.
        let x = if steps >= 2 {
            let n = (steps - 1) as f32;
            (x * n).round() / n
        } else {
            x
        };
        set(&mut v, p, x);
    }
    if blip {
        set(&mut v, M::ArpOn, 1.0);
        set(&mut v, M::ArpRate, 0.6);
    }
    // Guards: something to hear, a filter that opens, notes that last.
    if [M::PulseLevel, M::SawLevel, M::SubLevel]
        .iter()
        .all(|&p| get(&v, p) < 0.3)
    {
        set(&mut v, M::SawLevel, 0.7);
    }
    if get(&v, M::EnvAmount) < 0.2 && get(&v, M::Cutoff) < 0.35 {
        set(&mut v, M::Cutoff, 0.35);
    }
    if get(&v, M::Sustain) < 0.1 && get(&v, M::Decay) < 0.35 {
        set(&mut v, M::Decay, 0.35);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drums::BD;
    use crate::drums::{CH, CP, OH};
    use crate::machine::MachineUid;
    use crate::steps::SynthStep;

    fn grid(p: &DrumPattern) -> DrumGrid {
        DrumGrid::from_roll(&p.roll)
    }

    fn sgrid(p: &SynthPattern) -> SynthGrid {
        SynthGrid::from_roll(&p.roll)
    }

    /// An empty drum pattern of `length`, with `set` hits on it.
    fn drums(length: u8, set: impl FnOnce(&mut DrumGrid)) -> DrumPattern {
        let mut g = grid(&DrumPattern::default());
        g.length = length;
        set(&mut g);
        DrumPattern { roll: g.to_roll() }
    }

    /// A synth pattern of `length` with every step played, `set` on it.
    fn played(length: u8, set: impl Fn(usize, &mut SynthStep)) -> SynthPattern {
        let mut g = sgrid(&SynthPattern::default());
        g.length = length;
        for (i, s) in g.steps.iter_mut().enumerate() {
            s.gate = Gate::On;
            set(i, s);
        }
        SynthPattern {
            roll: g.to_roll(),
            ..SynthPattern::default()
        }
    }

    fn hits(p: &DrumPattern) -> usize {
        grid(p)
            .steps
            .iter()
            .flatten()
            .filter(|h| **h != DrumHit::Off)
            .count()
    }

    #[test]
    fn template_has_a_kick_on_one_and_a_backbeat() {
        for seed in 0..50 {
            let t = drum_template(seed, 16);
            assert_eq!(t.roll.length, 16);
            let p = grid(&t);
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
        let t = drum_template(3, 32);
        assert_eq!(t.roll.length, 32);
        let p = grid(&t);
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
            let evolved = evolve_drums(&base, seed);
            assert_eq!(evolved, evolve_drums(&base, seed));
            let e = grid(&evolved);
            let diff = grid(&base)
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

    #[test]
    fn evolving_an_empty_pattern_adds_hits_for_every_seed() {
        for seed in 0..50 {
            let e = evolve_drums(&DrumPattern::default(), seed);
            assert!(hits(&e) >= 1, "seed {seed}");
        }
    }

    fn open_hat_base(length: u8, set: impl FnOnce(&mut DrumGrid)) -> DrumPattern {
        drums(length, |p| {
            for s in [0, 4, 8, 12] {
                p.steps[s][CH] = DrumHit::On;
            }
            for s in [2, 6, 10, 14] {
                p.steps[s][OH] = DrumHit::On;
            }
            set(p);
        })
    }

    #[test]
    fn evolve_always_changes_a_pattern_with_open_hats() {
        let base = open_hat_base(16, |_| {});
        for seed in 0..200 {
            assert_ne!(evolve_drums(&base, seed), base, "seed {seed}");
        }
    }

    #[test]
    fn evolve_leaves_steps_beyond_the_length_alone() {
        let base = open_hat_base(8, |p| {
            p.steps[20][OH] = DrumHit::On;
            p.steps[20][CH] = DrumHit::On;
        });
        for seed in 0..100 {
            let e = evolve_drums(&base, seed);
            assert_eq!(grid(&e).steps[20], grid(&base).steps[20], "seed {seed}");
        }
    }

    #[test]
    fn templates_vary_across_seeds() {
        let ts: Vec<DrumPattern> = (0..50).map(|s| drum_template(s, 16)).collect();
        let unique = (0..ts.len()).filter(|&i| !ts[..i].contains(&ts[i])).count();
        assert!(unique >= 3, "only {unique} distinct templates");
        assert!(ts.iter().any(|p| grid(p).steps[4][SD] != DrumHit::Off));
        assert!(ts.iter().any(|p| grid(p).steps[4][CP] != DrumHit::Off));
    }

    #[test]
    fn short_lengths_stay_inside_the_length() {
        for seed in 0..50 {
            let t = drum_template(seed, 3);
            assert_eq!(t.roll.length, 3);
            assert!(
                grid(&t).steps[3..]
                    .iter()
                    .flatten()
                    .all(|h| *h == DrumHit::Off)
            );
            for len in [1u8, 3] {
                let base = drums(len, |p| p.steps[0][BD] = DrumHit::On);
                let e = grid(&evolve_drums(&base, seed));
                assert!(
                    e.steps[len as usize..]
                        .iter()
                        .flatten()
                        .all(|h| *h == DrumHit::Off),
                    "seed {seed} len {len}"
                );
            }
        }
    }

    #[test]
    fn slide_only_dice_clear_accents_after_slides() {
        let base = played(16, |_, s| s.accent = true);
        let g = groove_four_on_floor();
        let mut any_slide = false;
        for seed in 0..100 {
            let r = sgrid(&randomise_synth(
                &base,
                Dice {
                    slide: true,
                    ..Dice::default()
                },
                &g,
                seed,
            ));
            for i in 0..16 {
                let after_slide = i > 0 && r.steps[i - 1].slide;
                any_slide |= r.steps[i].slide;
                assert_eq!(r.steps[i].accent, !after_slide, "seed {seed} step {i}");
            }
        }
        assert!(any_slide);
    }

    #[test]
    fn octave_die_stays_within_one_octave_and_changes_something() {
        let mut changed = false;
        for seed in 0..100 {
            let base = played(16, |i, s| s.octave = [-1, 0, 1][i % 3]);
            let r = randomise_synth(
                &base,
                Dice {
                    octave: true,
                    ..Dice::default()
                },
                &groove_four_on_floor(),
                seed,
            );
            for (a, b) in sgrid(&base).steps.iter().zip(&sgrid(&r).steps).take(16) {
                assert!((-1..=1).contains(&b.octave), "seed {seed}");
                changed |= a.octave != b.octave;
            }
        }
        assert!(changed);
    }

    #[test]
    fn a_short_groove_wraps_over_a_longer_synth_pattern() {
        let g = drums(4, |p| p.steps[0][BD] = DrumHit::On);
        let mut on = [0u32; 16];
        for seed in 0..300 {
            let r = sgrid(&randomise_synth(
                &SynthPattern::default(),
                Dice {
                    rest: true,
                    ..Dice::default()
                },
                &g,
                seed,
            ));
            for (i, c) in on.iter_mut().enumerate() {
                if r.steps[i].gate != Gate::Off {
                    *c += 1;
                }
            }
        }
        for hit in [0, 4, 8, 12] {
            for off in [1, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15] {
                assert!(on[hit] > on[off] + 60, "step {hit} vs {off}: {on:?}");
            }
        }
    }

    fn groove_four_on_floor() -> DrumPattern {
        drums(16, |g| {
            for s in [0, 4, 8, 12] {
                g.steps[s][BD] = DrumHit::On;
            }
        })
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
        let base = crate::demo::demo_project()
            .synth(MachineUid(1))
            .unwrap()
            .patterns[0]
            .clone();
        assert_eq!(
            randomise_synth(&base, Dice::default(), &groove_four_on_floor(), 1),
            base
        );
    }

    #[test]
    fn each_die_touches_only_its_field() {
        let base = crate::demo::demo_project()
            .synth(MachineUid(1))
            .unwrap()
            .patterns[0]
            .clone();
        let g = groove_four_on_floor();
        let (a, len) = (sgrid(&base), 16);
        let mut newly_on = 0;
        for seed in 0..20 {
            let b = sgrid(&randomise_synth(
                &base,
                Dice {
                    rest: true,
                    ..Dice::default()
                },
                &g,
                seed,
            ));
            assert_eq!(&a.steps[len..], &b.steps[len..], "past the length");
            for i in 0..len {
                let (x, y) = (a.steps[i], b.steps[i]);
                match (x.gate != Gate::Off, y.gate != Gate::Off) {
                    // Kept: only the gate may change (a rest has no note).
                    (true, true) => assert_eq!(
                        (x.note, x.octave, x.accent, x.slide),
                        (y.note, y.octave, y.accent, y.slide),
                        "seed {seed} step {i}"
                    ),
                    // New: unaccented, unslid, an earlier kept pitch.
                    (false, true) => {
                        newly_on += 1;
                        assert!(!y.accent && !y.slide, "seed {seed} step {i}");
                        let kept = (1..len).map(|k| (i + len - k) % len).find(|&j| {
                            a.steps[j].gate != Gate::Off && b.steps[j].gate != Gate::Off
                        });
                        let want = kept.map_or((0, 0), |j| (b.steps[j].note, b.steps[j].octave));
                        assert_eq!((y.note, y.octave), want, "seed {seed} step {i}");
                    }
                    _ => {}
                }
            }
        }
        assert!(newly_on > 0);
        let r = randomise_synth(
            &base,
            Dice {
                notes: true,
                ..Dice::default()
            },
            &g,
            3,
        );
        for (a, b) in sgrid(&base).steps.iter().zip(&sgrid(&r).steps) {
            assert_eq!((a.gate, a.accent, a.slide), (b.gate, b.accent, b.slide));
        }
    }

    #[test]
    fn a_step_the_rest_die_turns_on_repeats_the_nearest_earlier_pitch() {
        // One note, on step 3: a new step takes its pitch, also on steps
        // 0..3 (found by wrapping); with step 3 resting, every new step is C.
        let mut base = sgrid(&SynthPattern::default());
        base.steps[3] = SynthStep {
            note: 7,
            octave: 1,
            gate: Gate::On,
            accent: true,
            slide: false,
        };
        let base = SynthPattern {
            roll: base.to_roll(),
            ..SynthPattern::default()
        };
        let (mut kept, mut rested) = (false, false);
        for seed in 0..100 {
            let r = sgrid(&randomise_synth(
                &base,
                Dice {
                    rest: true,
                    ..Dice::default()
                },
                &groove_four_on_floor(),
                seed,
            ));
            let want = if r.steps[3].gate != Gate::Off {
                kept = true;
                (7, 1)
            } else {
                rested = true;
                (0, 0)
            };
            for (i, s) in r.steps[..16].iter().enumerate() {
                if s.gate != Gate::Off {
                    assert_eq!((s.note, s.octave), want, "seed {seed} step {i}");
                }
            }
        }
        assert!(kept && rested);
    }

    #[test]
    fn notes_stay_in_range_and_leaps_are_folded() {
        let g = groove_four_on_floor();
        let all_on = played(16, |_, _| {});
        for seed in 0..500 {
            let r = sgrid(&randomise_synth(
                &SynthPattern::default(),
                Dice::ALL,
                &g,
                seed,
            ));
            for s in r.steps.iter().take(16) {
                assert!(
                    s.note < 12 && (-1..=1).contains(&s.octave),
                    "seed {seed}: {s:?}"
                );
            }
            let n = sgrid(&randomise_synth(
                &all_on,
                Dice {
                    notes: true,
                    ..Dice::default()
                },
                &g,
                seed,
            ));
            for w in n.steps[..16].windows(2) {
                assert!(
                    (semis(&w[1]) - semis(&w[0])).abs() <= MAX_LEAP,
                    "seed {seed}"
                );
            }
        }
    }

    #[test]
    fn never_an_accent_directly_after_a_slide() {
        let g = groove_four_on_floor();
        for seed in 0..300 {
            let r = sgrid(&randomise_synth(
                &SynthPattern::default(),
                Dice::ALL,
                &g,
                seed,
            ));
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
            let r = sgrid(&randomise_synth(
                &SynthPattern::default(),
                Dice {
                    rest: true,
                    ..Dice::default()
                },
                &g,
                seed,
            ));
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
        let base = played(8, |i, s| s.note = (i % 12) as u8);
        let r = randomise_synth(&base, Dice::ALL, &groove_four_on_floor(), 5);
        assert_eq!(&sgrid(&r).steps[8..], &sgrid(&base).steps[8..]);
    }

    fn load_voice(s: &mut crate::fm::FmSynth, v: &Dx7Voice) {
        use crate::params::MachineParam;
        for (id, x) in v.to_params() {
            match id {
                MachineParam::Fm(p) => s.set_param(p, x),
                MachineParam::FmOp(op, p) => s.set_op_param(op as usize, p, x),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn random_voices_are_deterministic_valid_and_vary() {
        let base = Dx7Voice::init();
        let a = randomise_voice(&base, 7);
        assert_eq!(a, randomise_voice(&base, 7));
        assert_ne!(a, randomise_voice(&base, 8));
        for seed in 0..200 {
            assert!(randomise_voice(&base, seed).is_valid(), "seed {seed}");
        }
    }

    #[test]
    fn random_voices_keep_osc_sync_and_transpose() {
        let mut base = Dx7Voice::init();
        base.osc_sync = false;
        base.transpose = 31;
        for seed in 0..50 {
            let v = randomise_voice(&base, seed);
            assert_eq!((v.osc_sync, v.transpose), (false, 31), "seed {seed}");
        }
    }

    /// A held middle C rendered for one second.
    fn hold_c(v: &Dx7Voice) -> Vec<f32> {
        let mut s = crate::fm::FmSynth::new(48_000.0);
        load_voice(&mut s, v);
        s.note_on(60, 100, false);
        let n = 48_000;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        s.render(&mut l, &mut r);
        l
    }

    /// How much the level swings from one 10 ms window to the next while
    /// the note is held (after the attack): ~0 for a steady tone.
    fn pulsing(x: &[f32]) -> f32 {
        let rms: Vec<f32> = x[4_800..]
            .chunks(480)
            .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
            .collect();
        let mean = rms.iter().sum::<f32>() / rms.len() as f32;
        let swing = rms.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
        swing / rms.len() as f32 / mean.max(1e-9)
    }

    /// Mean frequency relative to a sine at the note: 1 for a sine,
    /// higher for a brighter, richer tone.
    fn brightness(x: &[f32]) -> f32 {
        let x = &x[4_800..];
        let energy = |y: &mut dyn Iterator<Item = f32>| y.map(|v| v * v).sum::<f32>();
        let diff = energy(&mut x.windows(2).map(|w| w[1] - w[0]));
        let sig = energy(&mut x.iter().copied());
        let sine = std::f32::consts::TAU * 261.63 / 48_000.0;
        (diff / sig.max(1e-12)).sqrt() / sine
    }

    #[test]
    fn random_voices_are_a_mix_of_plain_bright_and_rhythmic() {
        let (mut plain, mut bright, mut rhythmic) = (0, 0, 0);
        let seeds = 100;
        for seed in 0..seeds {
            let x = hold_c(&randomise_voice(&Dx7Voice::init(), seed));
            let (p, b) = (pulsing(&x), brightness(&x));
            if p > 0.1 {
                rhythmic += 1;
            } else if b > 4.0 {
                bright += 1;
            } else if b < 2.0 {
                plain += 1;
            }
        }
        eprintln!("plain {plain} bright {bright} rhythmic {rhythmic}");
        assert!(rhythmic >= 30, "{rhythmic} rhythmic of {seeds}");
        assert!(bright >= 15, "{bright} bright of {seeds}");
        assert!(plain >= 10, "{plain} plain of {seeds}");
    }

    #[test]
    fn every_random_voice_plays_and_its_envelopes_end_at_zero() {
        use crate::testutil::{all_finite, peak};
        for seed in 0..100 {
            let v = randomise_voice(&Dx7Voice::init(), seed);
            for (note, vel) in [(36, 110), (60, 40), (84, 100)] {
                let mut s = crate::fm::FmSynth::new(48_000.0);
                load_voice(&mut s, &v);
                s.note_on(note, vel, false);
                let n = 12_000;
                let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
                s.render(&mut l, &mut r);
                assert!(all_finite(&l), "seed {seed}");
                assert!(peak(&l) > 0.01, "seed {seed} note {note}: silent");
                assert!(peak(&l) < 4.0, "seed {seed}: too loud {}", peak(&l));
            }
            for o in &v.ops {
                assert_eq!(o.levels[3], 0, "seed {seed}: release ends at 0");
            }
            assert_eq!(v.pitch_levels[3], 50, "seed {seed}: pitch settles");
        }
    }

    #[test]
    fn mono_randomise_is_deterministic_varied_and_playable() {
        use crate::params::MonoParam as M;
        let defaults = |p: M| p.info().default;
        assert_eq!(randomise_mono(defaults, 3), randomise_mono(defaults, 3));
        assert_ne!(randomise_mono(defaults, 3), randomise_mono(defaults, 4));
        for seed in 0..300 {
            let v = randomise_mono(defaults, seed);
            let get = |p: M| v.iter().find(|(q, _)| *q == p).unwrap().1;
            assert!(
                v.iter()
                    .all(|(p, x)| *p != M::Volume && (0.0..=1.0).contains(x))
            );
            assert_eq!(v.len(), M::ALL.len() - 1);
            assert!(
                [M::PulseLevel, M::SawLevel, M::SubLevel]
                    .iter()
                    .any(|&p| get(p) >= 0.3),
                "seed {seed}: silent mixer"
            );
            assert!(
                get(M::EnvAmount) >= 0.2 || get(M::Cutoff) >= 0.35,
                "seed {seed}"
            );
            assert!(
                get(M::Sustain) >= 0.1 || get(M::Decay) >= 0.35,
                "seed {seed}"
            );
            // The arp is off in `defaults`: only the Blip character turns it on, at 1/16.
            if get(M::ArpOn) >= 0.5 {
                assert_eq!(get(M::ArpRate), 0.6, "seed {seed}");
            }
            assert_eq!(get(M::Drift), 0.0, "drift is kept");
        }
    }
}
