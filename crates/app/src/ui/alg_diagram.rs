//! The FM algorithm diagram (operator boxes and modulation lines,
//! carriers on the bottom row, like the FM-1's screen) and the operator
//! envelope sketch. Layout math is pure; drawing is in `fm_panel`.

use acid_engine::fm::algorithms::Algorithm;

/// Grid position of each operator: (column, row), row 0 = carriers.
/// Each modulator sits above the lowest-numbered operator it modulates;
/// siblings sit side by side, centred over their target.
pub fn layout(alg: &Algorithm) -> [(f32, u8); 6] {
    let mut parent = [None::<usize>; 6];
    for target in 0..6 {
        for (m, p) in parent.iter_mut().enumerate() {
            if alg.mods[target] & (1 << m) != 0 && p.is_none() {
                *p = Some(target);
            }
        }
    }
    let mut width = [1u8; 6];
    for op in (0..6).rev() {
        let w: u8 = (0..6)
            .filter(|&c| parent[c] == Some(op))
            .map(|c| width[c])
            .sum();
        width[op] = w.max(1);
    }
    let mut pos = [(0.0, 0u8); 6];
    let mut x = 0;
    for op in 0..6 {
        if parent[op].is_none() {
            place(op, x, 0, &parent, &width, &mut pos);
            x += width[op];
        }
    }
    pos
}

fn place(
    op: usize,
    x0: u8,
    row: u8,
    parent: &[Option<usize>; 6],
    width: &[u8; 6],
    pos: &mut [(f32, u8); 6],
) {
    pos[op] = (x0 as f32 + (width[op] as f32 - 1.0) / 2.0, row);
    let mut x = x0;
    for child in 0..6 {
        if parent[child] == Some(op) {
            place(child, x, row + 1, parent, width, pos);
            x += width[child];
        }
    }
}

/// Corner points of an envelope sketch, x = time 0..1, y = level 0..1:
/// start (silence), the end of attack, decay 1 and decay 2, the end of a
/// fixed sustain stretch, and the end of the release.
pub fn env_points(rates: [f32; 4], levels: [f32; 4]) -> [(f32, f32); 6] {
    let seg = |r: f32| 0.05 + (99.0 - r.clamp(0.0, 99.0)) / 99.0;
    let times = [
        seg(rates[0]),
        seg(rates[1]),
        seg(rates[2]),
        0.6,
        seg(rates[3]),
    ];
    let total: f32 = times.iter().sum();
    let y = |l: f32| l.clamp(0.0, 99.0) / 99.0;
    let ys = [
        y(levels[0]),
        y(levels[1]),
        y(levels[2]),
        y(levels[2]),
        y(levels[3]),
    ];
    let mut pts = [(0.0, 0.0); 6];
    let mut x = 0.0;
    for i in 0..5 {
        x += times[i] / total;
        pts[i + 1] = (x, ys[i]);
    }
    pts[5].0 = 1.0;
    pts
}

#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::fm::algorithms::ALGORITHMS;

    #[test]
    fn every_algorithm_lays_out_without_overlaps() {
        for (i, alg) in ALGORITHMS.iter().enumerate() {
            let pos = layout(alg);
            for a in 0..6 {
                assert!(
                    pos[a].0 >= 0.0 && pos[a].0 < 6.0,
                    "alg {} op {}",
                    i + 1,
                    a + 1
                );
                assert_eq!(
                    pos[a].1 == 0,
                    alg.carriers & (1 << a) != 0,
                    "alg {} op {}",
                    i + 1,
                    a + 1
                );
                for b in 0..a {
                    assert_ne!(pos[a], pos[b], "alg {} ops {} and {}", i + 1, a + 1, b + 1);
                }
                for m in 0..6 {
                    if alg.mods[a] & (1 << m) != 0 {
                        assert!(
                            pos[m].1 > pos[a].1,
                            "alg {}: OP{} sits above OP{}",
                            i + 1,
                            m + 1,
                            a + 1
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn algorithm_32_is_one_row_of_six() {
        let pos = layout(&ALGORITHMS[31]);
        assert!(pos.iter().all(|p| p.1 == 0));
        let mut cols: Vec<f32> = pos.iter().map(|p| p.0).collect();
        cols.sort_by(f32::total_cmp);
        assert_eq!(cols, [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn envelope_sketch() {
        let pts = env_points([99.0, 50.0, 50.0, 99.0], [99.0, 80.0, 60.0, 0.0]);
        assert_eq!(pts[0], (0.0, 0.0));
        assert!((pts[5].0 - 1.0).abs() < 1e-6);
        assert!(pts.windows(2).all(|w| w[1].0 > w[0].0));
        assert_eq!(pts[1].1, 1.0);
        assert!(
            (pts[4].1 - 60.0 / 99.0).abs() < 1e-6,
            "sustain stretch holds L3"
        );
        let slow = env_points([10.0, 50.0, 50.0, 99.0], [99.0, 80.0, 60.0, 0.0]);
        assert!(
            slow[1].0 > pts[1].0,
            "a slower attack takes more of the width"
        );
    }
}
