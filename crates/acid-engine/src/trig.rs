//! When a note plays (condition, then chance) and how a ratchet splits it.
//! Pure and allocation-free: the sequencer calls it on the audio thread.

use crate::project::{Cond, Note};

/// Per-track state the decision reads and updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrackTrig {
    /// Completed passes of the current pattern (since Play or block start).
    pub loops: u32,
    /// The last conditional note's result; false after a reset.
    pub pre: bool,
}

// Fixed odd 64-bit multipliers (golden-ratio and splitmix-family constants)
// that spread seed, rack position, loop count and note index before mixing.
const K1: u64 = 0x9E37_79B9_7F4A_7C15;
const K2: u64 = 0xBF58_476D_1CE4_E5B9;
const K3: u64 = 0x94D0_49BB_1331_11EB;

/// The splitmix64 finaliser.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A note with a condition or a chance below 100 (it updates `pre`).
pub fn is_conditional(n: &Note) -> bool {
    n.cond != Cond::Always || n.chance < 100
}

/// Does the note at `index` play this pass? Condition first; only if it
/// passes is the chance rolled. A conditional note's result becomes `t.pre`.
pub fn plays(n: &Note, index: usize, pos: usize, seed: u32, fill: bool, t: &mut TrackTrig) -> bool {
    if !is_conditional(n) {
        return true;
    }
    let cond = match n.cond {
        Cond::Always => true,
        Cond::Ratio(a, b) => b != 0 && t.loops % b as u32 == (a as u32).wrapping_sub(1),
        Cond::First => t.loops == 0,
        Cond::NotFirst => t.loops > 0,
        Cond::Fill => fill,
        Cond::NotFill => !fill,
        Cond::Pre => t.pre,
        Cond::NotPre => !t.pre,
    };
    let ok = cond && (n.chance >= 100 || roll(seed, pos, t.loops, index) < n.chance as u64);
    t.pre = ok;
    ok
}

fn roll(seed: u32, pos: usize, loops: u32, index: usize) -> u64 {
    let x = (seed as u64)
        ^ (pos as u64).wrapping_mul(K1)
        ^ (loops as u64).wrapping_mul(K2)
        ^ (index as u64).wrapping_mul(K3);
    mix(x) % 100
}

/// Sub-ticks per tick: a ratchet's hits (1 to 8 over a whole number of
/// ticks) all land exactly on one, as 840 is divisible by 1 to 8.
pub const FINE: u32 = 840;

/// A note's ratchet: how many hits it splits into (1 to 8).
pub fn ratchet_count(n: &Note) -> u32 {
    n.ratchet.clamp(1, 8) as u32
}

/// The sub-hits of a note as `(start, len)` in sub-ticks (`FINE` per
/// tick): `ratchet` hits at exactly `start + k*len/n`, each `len/n` long
/// (a note shorter than a tick counts as one tick long).
pub fn sub_hits(n: &Note) -> impl Iterator<Item = (u32, u32)> + use<> {
    let count = ratchet_count(n);
    let start = n.start as u32 * FINE;
    let sub = n.len.max(1) as u32 * FINE / count;
    (0..count).map(move |k| (start + k * sub, sub))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note() -> Note {
        Note::new(10, 24, 60, 100)
    }

    fn with(chance: u8, cond: Cond) -> Note {
        Note {
            chance,
            cond,
            ..note()
        }
    }

    fn at_loop(n: &Note, loops: u32, fill: bool, pre: bool) -> (bool, TrackTrig) {
        let mut t = TrackTrig { loops, pre };
        (plays(n, 0, 0, 0, fill, &mut t), t)
    }

    #[test]
    fn default_notes_always_play_and_leave_pre() {
        for pre in [false, true] {
            let (p, t) = at_loop(&note(), 3, false, pre);
            assert!(p);
            assert_eq!(t.pre, pre);
        }
        assert!(!is_conditional(&note()));
    }

    #[test]
    fn chance_extremes() {
        for l in 0..200 {
            assert!(!at_loop(&with(0, Cond::Always), l, false, false).0);
            assert!(at_loop(&with(100, Cond::Always), l, false, false).0);
        }
    }

    #[test]
    fn chance_50_is_about_half_and_deterministic() {
        let n = with(50, Cond::Always);
        let run = || -> Vec<bool> { (0..1000).map(|l| at_loop(&n, l, false, false).0).collect() };
        let a = run();
        let hits = a.iter().filter(|&&b| b).count();
        assert!((400..=600).contains(&hits), "{hits}");
        assert_eq!(a, run());
        // The seed changes the pattern.
        let mut t = TrackTrig::default();
        let b: Vec<bool> = (0..1000)
            .map(|l| {
                t.loops = l;
                plays(&n, 0, 0, 7, false, &mut t)
            })
            .collect();
        assert_ne!(a, b);
    }

    #[test]
    fn ratios() {
        let r14 = with(100, Cond::Ratio(1, 4));
        for l in 0..12 {
            assert_eq!(at_loop(&r14, l, false, false).0, l % 4 == 0, "{l}");
        }
        let r44 = with(100, Cond::Ratio(4, 4));
        for l in 0..12 {
            assert_eq!(at_loop(&r44, l, false, false).0, l % 4 == 3, "{l}");
        }
    }

    #[test]
    fn first_and_fill() {
        assert!(at_loop(&with(100, Cond::First), 0, false, false).0);
        assert!(!at_loop(&with(100, Cond::First), 1, false, false).0);
        assert!(!at_loop(&with(100, Cond::NotFirst), 0, false, false).0);
        assert!(at_loop(&with(100, Cond::NotFirst), 2, false, false).0);
        assert!(at_loop(&with(100, Cond::Fill), 0, true, false).0);
        assert!(!at_loop(&with(100, Cond::Fill), 0, false, false).0);
        assert!(!at_loop(&with(100, Cond::NotFill), 0, true, false).0);
        assert!(at_loop(&with(100, Cond::NotFill), 0, false, false).0);
    }

    #[test]
    fn pre_follows_the_last_conditional_result() {
        let pre = with(100, Cond::Pre);
        let not_pre = with(100, Cond::NotPre);
        assert!(!at_loop(&pre, 0, false, false).0);
        assert!(at_loop(&pre, 0, false, true).0);
        assert!(at_loop(&not_pre, 0, false, false).0);
        assert!(!at_loop(&not_pre, 0, false, true).0);
        // The result is stored.
        assert!(at_loop(&not_pre, 0, false, false).1.pre);
        assert!(!at_loop(&pre, 0, false, false).1.pre);
        // A chance-only note is conditional too.
        assert!(!at_loop(&with(0, Cond::Always), 0, false, true).1.pre);
    }

    #[test]
    fn failing_condition_skips_the_roll() {
        // Chance 100 would play; the failing condition must win, and the
        // result stored is false.
        let (p, t) = at_loop(&with(100, Cond::First), 5, false, true);
        assert!(!p && !t.pre);
    }

    fn hits(ratchet: u8, start: u16, len: u16) -> Vec<(u32, u32)> {
        sub_hits(&Note {
            ratchet,
            start,
            len,
            ..note()
        })
        .collect()
    }

    #[test]
    fn ratchet_split() {
        const F: u32 = FINE;
        assert_eq!(hits(1, 10, 24), [(10 * F, 24 * F)]);
        assert_eq!(
            hits(3, 10, 24),
            [(10 * F, 8 * F), (18 * F, 8 * F), (26 * F, 8 * F)]
        );
        // Exact fractions of a tick, not snapped to the grid.
        assert_eq!(hits(3, 0, 4), [(0, 1120), (1120, 1120), (2240, 1120)]);
        let eight: Vec<u32> = hits(8, 0, 4).iter().map(|h| h.0).collect();
        assert_eq!(eight, (0..8).map(|k| k * F / 2).collect::<Vec<_>>());
        // Shorter than the ratchet: still 8 distinct, even hits.
        assert_eq!(hits(8, 4, 3).iter().map(|h| h.1).min(), Some(3 * F / 8));
        assert_eq!(hits(8, 4, 3)[7].0, 4 * F + 7 * 3 * F / 8);
        // Every ratchet divides every length exactly.
        for r in 1..=8u8 {
            for len in 1..=64u16 {
                let h = hits(r, 0, len);
                assert_eq!(h.len(), r as usize);
                assert_eq!(h.last().unwrap().0 + h[0].1, len as u32 * F, "{r} {len}");
            }
        }
    }
}
