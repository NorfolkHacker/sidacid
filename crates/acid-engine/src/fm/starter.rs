//! The starter bank: 16 original voices (not Yamaha ROM sounds).
//! Operators are listed OP1..OP6; algorithms use DX7 numbering (1..=32).

use super::sysex::{Dx7Op, Dx7Voice};

/// An operator: ratio `coarse` (+`fine` %), output `level`, envelope.
fn op(coarse: u8, fine: u8, level: u8, rates: [u8; 4], levels: [u8; 4]) -> Dx7Op {
    Dx7Op {
        rates,
        levels,
        break_point: 39,
        left_depth: 0,
        right_depth: 0,
        left_curve: 0,
        right_curve: 0,
        rate_scaling: 2,
        amp_mod_sens: 0,
        vel_sens: 3,
        level,
        fixed: false,
        coarse,
        fine,
        detune: 7,
    }
}

fn detuned(mut o: Dx7Op, detune: u8) -> Dx7Op {
    o.detune = detune;
    o
}

fn velocity(mut o: Dx7Op, vel_sens: u8) -> Dx7Op {
    o.vel_sens = vel_sens;
    o
}

/// An operator that is silent (level 0) in this voice.
fn off() -> Dx7Op {
    op(1, 0, 0, [99, 99, 99, 99], [99, 99, 99, 0])
}

fn voice(name: &str, algorithm: u8, feedback: u8, ops: [Dx7Op; 6]) -> Dx7Voice {
    let mut v = Dx7Voice::init();
    v.name = name.into();
    v.algorithm = algorithm - 1;
    v.feedback = feedback;
    v.ops = ops;
    v
}

/// The 16 starter voices (original, not Yamaha ROM), in browser order.
/// Allocates: build once off the audio thread.
pub fn starter_bank() -> Vec<Dx7Voice> {
    let sustain = [99, 99, 99, 0];
    let mut kick = voice(
        "KICK FM",
        1,
        0,
        [
            op(0, 0, 99, [99, 55, 40, 70], [99, 0, 0, 0]),
            op(1, 0, 60, [99, 90, 60, 70], [99, 0, 0, 0]),
            off(),
            off(),
            off(),
            off(),
        ],
    );
    kick.pitch_rates = [75, 99, 99, 99];
    kick.pitch_levels = [50, 50, 50, 75];
    vec![
        voice(
            "E.PIANO 1",
            5,
            6,
            [
                velocity(op(1, 0, 99, [96, 25, 25, 67], [99, 75, 0, 0]), 2),
                velocity(op(14, 0, 58, [95, 50, 35, 78], [99, 75, 0, 0]), 7),
                velocity(op(1, 0, 99, [95, 20, 20, 50], [99, 95, 0, 0]), 2),
                velocity(op(1, 0, 89, [95, 29, 20, 50], [99, 95, 0, 0]), 6),
                op(1, 0, 99, [95, 20, 20, 50], [99, 95, 0, 0]),
                detuned(
                    velocity(op(1, 0, 79, [95, 29, 20, 50], [99, 95, 0, 0]), 6),
                    9,
                ),
            ],
        ),
        voice(
            "FM BASS",
            1,
            7,
            [
                op(1, 0, 99, [99, 50, 35, 60], [99, 80, 0, 0]),
                op(1, 0, 85, [99, 60, 40, 60], [99, 50, 0, 0]),
                op(1, 0, 94, [99, 45, 35, 60], [99, 80, 0, 0]),
                op(2, 0, 70, [99, 70, 40, 60], [99, 40, 0, 0]),
                op(1, 0, 60, [99, 60, 40, 60], [99, 50, 0, 0]),
                op(3, 0, 60, [99, 60, 40, 60], [99, 40, 0, 0]),
            ],
        ),
        voice(
            "ACID FM",
            1,
            6,
            [
                op(1, 0, 99, [99, 40, 30, 70], [99, 90, 85, 0]),
                velocity(op(1, 0, 90, [99, 55, 45, 70], [99, 30, 0, 0]), 6),
                off(),
                off(),
                off(),
                off(),
            ],
        ),
        voice(
            "BELLS",
            5,
            0,
            [
                op(1, 0, 99, [99, 30, 20, 30], [99, 0, 0, 0]),
                op(3, 50, 75, [99, 35, 20, 30], [99, 0, 0, 0]),
                op(2, 0, 90, [99, 28, 20, 30], [99, 0, 0, 0]),
                op(7, 0, 60, [99, 40, 20, 30], [99, 0, 0, 0]),
                op(0, 0, 85, [99, 25, 20, 30], [99, 0, 0, 0]),
                op(5, 41, 50, [99, 45, 20, 30], [99, 0, 0, 0]),
            ],
        ),
        voice(
            "MARIMBA",
            5,
            0,
            [
                op(1, 0, 99, [99, 50, 30, 50], [99, 0, 0, 0]),
                op(4, 0, 80, [99, 75, 50, 50], [99, 0, 0, 0]),
                op(1, 0, 80, [99, 50, 30, 50], [99, 0, 0, 0]),
                op(10, 0, 60, [99, 85, 50, 50], [99, 0, 0, 0]),
                off(),
                off(),
            ],
        ),
        voice(
            "BRASS",
            22,
            7,
            [
                op(1, 0, 99, [70, 60, 40, 60], [99, 92, 90, 0]),
                op(1, 0, 82, [65, 55, 40, 60], [99, 90, 85, 0]),
                detuned(op(1, 0, 99, [70, 60, 40, 60], [99, 92, 90, 0]), 8),
                detuned(op(1, 0, 99, [70, 60, 40, 60], [99, 92, 90, 0]), 6),
                op(1, 0, 99, [70, 60, 40, 60], [99, 92, 90, 0]),
                op(1, 0, 80, [60, 50, 40, 60], [99, 85, 80, 0]),
            ],
        ),
        voice(
            "STRINGS",
            1,
            4,
            [
                op(1, 0, 99, [45, 30, 25, 40], [99, 95, 95, 0]),
                op(1, 0, 72, [45, 30, 25, 40], [99, 90, 90, 0]),
                detuned(op(1, 0, 99, [45, 30, 25, 40], [99, 95, 95, 0]), 9),
                op(3, 0, 60, [45, 30, 25, 40], [99, 90, 90, 0]),
                op(1, 0, 55, [45, 30, 25, 40], [99, 90, 90, 0]),
                detuned(op(1, 0, 50, [45, 30, 25, 40], [99, 90, 90, 0]), 5),
            ],
        ),
        voice(
            "PAD",
            5,
            3,
            [
                detuned(op(1, 0, 99, [35, 25, 20, 30], [99, 95, 95, 0]), 4),
                op(2, 0, 55, [35, 25, 20, 30], [99, 90, 90, 0]),
                detuned(op(1, 0, 99, [35, 25, 20, 30], [99, 95, 95, 0]), 10),
                op(1, 0, 50, [35, 25, 20, 30], [99, 90, 90, 0]),
                op(0, 0, 90, [35, 25, 20, 30], [99, 95, 95, 0]),
                op(3, 0, 45, [35, 25, 20, 30], [99, 90, 90, 0]),
            ],
        ),
        voice(
            "ORGAN",
            32,
            0,
            [
                op(0, 0, 90, [99, 99, 99, 90], sustain),
                op(1, 0, 99, [99, 99, 99, 90], sustain),
                op(2, 0, 85, [99, 99, 99, 90], sustain),
                op(3, 0, 75, [99, 99, 99, 90], sustain),
                op(4, 0, 70, [99, 99, 99, 90], sustain),
                op(8, 0, 60, [99, 99, 99, 90], sustain),
            ],
        ),
        voice(
            "PLUCK",
            1,
            5,
            [
                op(1, 0, 99, [99, 55, 30, 70], [99, 0, 0, 0]),
                op(3, 0, 80, [99, 70, 30, 70], [99, 0, 0, 0]),
                op(1, 0, 80, [99, 60, 30, 70], [99, 0, 0, 0]),
                op(1, 0, 75, [99, 80, 30, 70], [99, 0, 0, 0]),
                op(5, 0, 60, [99, 75, 30, 70], [99, 0, 0, 0]),
                op(7, 0, 50, [99, 80, 30, 70], [99, 0, 0, 0]),
            ],
        ),
        voice(
            "LEAD",
            1,
            6,
            [
                op(1, 0, 99, [99, 40, 30, 60], [99, 95, 95, 0]),
                op(1, 0, 85, [99, 40, 30, 60], [99, 90, 90, 0]),
                op(2, 0, 80, [99, 40, 30, 60], [99, 95, 95, 0]),
                op(1, 0, 75, [99, 40, 30, 60], [99, 90, 90, 0]),
                op(1, 0, 65, [99, 40, 30, 60], [99, 90, 90, 0]),
                op(1, 0, 70, [99, 40, 30, 60], [99, 90, 90, 0]),
            ],
        ),
        voice(
            "CLAV",
            5,
            4,
            [
                op(1, 0, 99, [99, 70, 40, 80], [99, 60, 0, 0]),
                op(3, 0, 85, [99, 80, 40, 80], [99, 40, 0, 0]),
                op(1, 0, 90, [99, 70, 40, 80], [99, 60, 0, 0]),
                op(5, 0, 75, [99, 85, 40, 80], [99, 30, 0, 0]),
                op(2, 0, 80, [99, 70, 40, 80], [99, 60, 0, 0]),
                op(7, 0, 65, [99, 85, 40, 80], [99, 30, 0, 0]),
            ],
        ),
        kick,
        voice(
            "SNARE FM",
            2,
            7,
            [
                op(1, 0, 99, [99, 65, 50, 70], [99, 0, 0, 0]),
                op(11, 37, 95, [99, 70, 50, 70], [99, 0, 0, 0]),
                off(),
                off(),
                off(),
                off(),
            ],
        ),
        voice(
            "HAT FM",
            2,
            7,
            [
                op(9, 33, 99, [99, 85, 60, 85], [99, 0, 0, 0]),
                op(13, 61, 99, [99, 85, 60, 85], [99, 0, 0, 0]),
                off(),
                off(),
                off(),
                off(),
            ],
        ),
        voice(
            "TUBULAR",
            5,
            0,
            [
                op(1, 0, 99, [99, 25, 20, 25], [99, 0, 0, 0]),
                op(3, 50, 78, [99, 30, 20, 25], [99, 0, 0, 0]),
                op(1, 0, 95, [99, 25, 20, 25], [99, 0, 0, 0]),
                op(7, 0, 60, [99, 35, 20, 25], [99, 0, 0, 0]),
                op(2, 0, 90, [99, 25, 20, 25], [99, 0, 0, 0]),
                op(5, 0, 55, [99, 35, 20, 25], [99, 0, 0, 0]),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fm::FmSynth;
    use crate::params::ParamId;
    use crate::testutil::{all_finite, peak};

    fn load(s: &mut FmSynth, v: &Dx7Voice) {
        for (id, x) in v.to_params() {
            match id {
                ParamId::Fm(p) => s.set_param(p, x),
                ParamId::FmOp(op, p) => s.set_op_param(op as usize, p, x),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn sixteen_valid_uniquely_named_voices() {
        let bank = starter_bank();
        assert_eq!(bank.len(), 16);
        let names: std::collections::HashSet<_> = bank.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names.len(), 16);
        for v in &bank {
            assert!(v.is_valid(), "{}", v.name);
            assert!(!v.name.is_empty() && v.name.len() <= 10);
        }
    }

    #[test]
    fn every_voice_plays_and_stays_bounded() {
        for v in starter_bank() {
            let mut s = FmSynth::new(48_000.0);
            load(&mut s, &v);
            s.note_on(48, 110, false);
            let n = 24_000;
            let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
            s.render(&mut l, &mut r);
            assert!(all_finite(&l), "{}", v.name);
            assert!(peak(&l) > 0.01, "{} is silent", v.name);
            assert!(peak(&l) < 4.0, "{} is too loud: {}", v.name, peak(&l));
        }
    }
}
