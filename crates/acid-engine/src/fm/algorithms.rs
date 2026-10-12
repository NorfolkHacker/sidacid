//! The 32 DX7 algorithms: which operators modulate which, which are
//! carriers, and which operator has the feedback loop. Operators are
//! indexed 0..6 for OP1..OP6. Every modulator has a higher index than the
//! operator it modulates, so evaluating OP6 down to OP1 is always valid.
//! Algorithms 4 and 6 loop through several operators on the DX7; here
//! they use self-feedback on OP6.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Algorithm {
    /// Bit `m` of `mods[op]` set = OP(m+1) modulates OP(op+1).
    pub mods: [u8; 6],
    /// Bit `op` set = OP(op+1) is a carrier (is heard).
    pub carriers: u8,
    /// Index of the operator with self-feedback.
    pub feedback: usize,
}

/// `edges` are (modulator, target) and `carriers`/`feedback` are DX7
/// operator numbers (1..=6).
const fn alg(edges: &[(usize, usize)], carriers: &[usize], feedback: usize) -> Algorithm {
    let mut mods = [0u8; 6];
    let mut i = 0;
    while i < edges.len() {
        let (m, t) = edges[i];
        mods[t - 1] |= 1 << (m - 1);
        i += 1;
    }
    let mut car = 0u8;
    let mut j = 0;
    while j < carriers.len() {
        car |= 1 << (carriers[j] - 1);
        j += 1;
    }
    Algorithm {
        mods,
        carriers: car,
        feedback: feedback - 1,
    }
}

pub const ALGORITHMS: [Algorithm; 32] = [
    alg(&[(2, 1), (4, 3), (5, 4), (6, 5)], &[1, 3], 6),
    alg(&[(2, 1), (4, 3), (5, 4), (6, 5)], &[1, 3], 2),
    alg(&[(2, 1), (3, 2), (5, 4), (6, 5)], &[1, 4], 6),
    alg(&[(2, 1), (3, 2), (5, 4), (6, 5)], &[1, 4], 6),
    alg(&[(2, 1), (4, 3), (6, 5)], &[1, 3, 5], 6),
    alg(&[(2, 1), (4, 3), (6, 5)], &[1, 3, 5], 6),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 5)], &[1, 3], 6),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 5)], &[1, 3], 4),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 5)], &[1, 3], 2),
    alg(&[(2, 1), (3, 2), (5, 4), (6, 4)], &[1, 4], 3),
    alg(&[(2, 1), (3, 2), (5, 4), (6, 4)], &[1, 4], 6),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 3)], &[1, 3], 2),
    alg(&[(2, 1), (4, 3), (5, 3), (6, 3)], &[1, 3], 6),
    alg(&[(2, 1), (4, 3), (5, 4), (6, 4)], &[1, 3], 6),
    alg(&[(2, 1), (4, 3), (5, 4), (6, 4)], &[1, 3], 2),
    alg(&[(2, 1), (3, 1), (4, 3), (5, 1), (6, 5)], &[1], 6),
    alg(&[(2, 1), (3, 1), (4, 3), (5, 1), (6, 5)], &[1], 2),
    alg(&[(2, 1), (3, 1), (4, 1), (5, 4), (6, 5)], &[1], 3),
    alg(&[(2, 1), (3, 2), (6, 4), (6, 5)], &[1, 4, 5], 6),
    alg(&[(3, 1), (3, 2), (5, 4), (6, 4)], &[1, 2, 4], 3),
    alg(&[(3, 1), (3, 2), (6, 4), (6, 5)], &[1, 2, 4, 5], 3),
    alg(&[(2, 1), (6, 3), (6, 4), (6, 5)], &[1, 3, 4, 5], 6),
    alg(&[(3, 2), (6, 4), (6, 5)], &[1, 2, 4, 5], 6),
    alg(&[(6, 3), (6, 4), (6, 5)], &[1, 2, 3, 4, 5], 6),
    alg(&[(6, 4), (6, 5)], &[1, 2, 3, 4, 5], 6),
    alg(&[(3, 2), (5, 4), (6, 4)], &[1, 2, 4], 6),
    alg(&[(3, 2), (5, 4), (6, 4)], &[1, 2, 4], 3),
    alg(&[(2, 1), (4, 3), (5, 4)], &[1, 3, 6], 5),
    alg(&[(4, 3), (6, 5)], &[1, 2, 3, 5], 6),
    alg(&[(4, 3), (5, 4)], &[1, 2, 3, 6], 5),
    alg(&[(6, 5)], &[1, 2, 3, 4, 5], 6),
    alg(&[], &[1, 2, 3, 4, 5, 6], 6),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carrier_counts_match_the_dx7() {
        let counts: Vec<u32> = ALGORITHMS.iter().map(|a| a.carriers.count_ones()).collect();
        assert_eq!(
            counts,
            [
                2, 2, 2, 2, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 3, 3, 4, 4, 4, 5, 5, 3, 3, 3,
                4, 4, 5, 6
            ]
        );
    }

    #[test]
    fn modulators_always_sit_above_their_targets() {
        for (i, a) in ALGORITHMS.iter().enumerate() {
            for op in 0..6 {
                let lower = (1u16 << (op + 1)) - 1; // bits 0..=op
                assert_eq!(a.mods[op] as u16 & lower, 0, "algorithm {}", i + 1);
            }
            assert!(a.feedback < 6);
        }
    }

    #[test]
    fn every_operator_is_heard_or_modulates() {
        for (i, a) in ALGORITHMS.iter().enumerate() {
            let modulates = a.mods.iter().fold(0u8, |acc, m| acc | m);
            assert_eq!(
                modulates & a.carriers,
                0,
                "carriers never modulate (alg {})",
                i + 1
            );
            assert_eq!(modulates | a.carriers, 0b11_1111, "algorithm {}", i + 1);
        }
    }

    #[test]
    fn spot_checks() {
        let a5 = ALGORITHMS[4];
        assert_eq!(a5.carriers, 0b01_0101);
        assert_eq!(a5.mods[0], 0b00_0010);
        assert_eq!(a5.mods[4], 0b10_0000);
        assert_eq!(a5.feedback, 5);
        let a16 = ALGORITHMS[15];
        assert_eq!(a16.mods[0], 0b01_0110, "OP2, OP3 and OP5 into OP1");
        assert_eq!(ALGORITHMS[31].mods, [0; 6]);
        assert_eq!(ALGORITHMS[17].feedback, 2, "algorithm 18 feeds back on OP3");
    }

    #[test]
    fn feedback_operators_match_the_dx7() {
        // From the DX7 algorithm chart (operators numbered 1-6). Algorithms
        // 4 and 6 have a two-operator feedback loop on the real DX7; here
        // they use OP6 self-feedback, as the module doc says.
        let want = [
            6, 2, 6, 6, 6, 6, 6, 4, 2, 3, 6, 2, 6, 6, 2, 6, 2, 3, 6, 3, 3, 6, 6, 6, 6, 6, 3, 5, 6,
            5, 6, 6,
        ];
        assert_eq!(ALGORITHMS.map(|a| a.feedback + 1), want);
    }
}
