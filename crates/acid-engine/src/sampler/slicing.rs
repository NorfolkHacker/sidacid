//! Chopping a sample into slices: equal divisions or transient
//! detection, plus a tempo guess for tempo lock.

use super::MAX_SLICES;

/// Interleaved audio averaged to one channel.
pub fn mono_mix(samples: &[f32], channels: usize) -> Vec<f32> {
    let ch = channels.max(1);
    samples
        .chunks(ch)
        .map(|f| f.iter().sum::<f32>() / f.len() as f32)
        .collect()
}

/// `count` equal slices (at most `MAX_SLICES`, at most one per frame).
pub fn equal_slices(frames: u32, count: usize) -> Vec<u32> {
    let count = count.clamp(1, MAX_SLICES).min(frames.max(1) as usize);
    (0..count)
        .map(|i| (i as u64 * frames as u64 / count as u64) as u32)
        .collect()
}

/// Slice starts at the sample's transients: rises in short-term energy
/// above a threshold set by `sensitivity` (0 = only the strongest hits,
/// 1 = quiet ones too), at least 50 ms apart. Always starts with 0.
pub fn detect_slices(mono: &[f32], sample_rate: u32, sensitivity: f32) -> Vec<u32> {
    let hop = (sample_rate as usize / 200).max(32);
    let energy: Vec<f32> = mono
        .chunks(hop)
        .map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let flux: Vec<f32> = (0..energy.len())
        .map(|k| {
            if k == 0 {
                energy[0]
            } else {
                (energy[k] - energy[k - 1]).max(0.0)
            }
        })
        .collect();
    let peak = flux.iter().copied().fold(0.0f32, f32::max);
    let mut out = vec![0u32];
    if peak <= 0.0 {
        return out;
    }
    let threshold = peak * (0.05 + 0.6 * (1.0 - sensitivity.clamp(0.0, 1.0)));
    let min_gap = (0.05 * sample_rate as f32) as u32;
    for k in 1..flux.len() {
        let next = flux.get(k + 1).copied().unwrap_or(0.0);
        let is_peak = flux[k] >= threshold && flux[k] >= flux[k - 1] && flux[k] >= next;
        let frame = (k * hop) as u32;
        if is_peak && frame >= out[out.len() - 1] + min_gap && out.len() < MAX_SLICES {
            out.push(frame);
        }
    }
    out
}

/// A tempo for tempo lock: assumes the sample is a whole number of 4/4
/// bars (or a quarter or half bar) and picks the count that lands in
/// 90–180 BPM, else the one closest to that range.
pub fn guess_bpm(frames: u32, sample_rate: u32) -> f32 {
    let seconds = frames as f32 / sample_rate.max(1) as f32;
    if seconds <= 0.0 {
        return 120.0;
    }
    let distance = |b: f32| {
        if b < 90.0 {
            90.0 - b
        } else if b > 180.0 {
            b - 180.0
        } else {
            0.0
        }
    };
    let candidates = [0.25f32, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0].map(|bars| bars * 240.0 / seconds);
    candidates
        .iter()
        .copied()
        .find(|b| (90.0..=180.0).contains(b))
        .unwrap_or_else(|| {
            candidates
                .iter()
                .copied()
                .min_by(|a, b| distance(*a).total_cmp(&distance(*b)))
                .unwrap_or(120.0)
                .clamp(20.0, 400.0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 44_100;

    fn clicks(at: &[(f32, f32)], seconds: f32) -> Vec<f32> {
        let mut x = vec![0.0; (seconds * SR as f32) as usize];
        for &(t, amp) in at {
            let start = (t * SR as f32) as usize;
            for k in 0..400 {
                if let Some(s) = x.get_mut(start + k) {
                    *s = amp * (k as f32 * 0.3).sin() * (-(k as f32) / 120.0).exp();
                }
            }
        }
        x
    }

    #[test]
    fn equal_slices_divide_evenly() {
        assert_eq!(equal_slices(1000, 4), vec![0, 250, 500, 750]);
        assert_eq!(equal_slices(1000, 1), vec![0]);
        assert_eq!(
            equal_slices(10, 64).len(),
            10,
            "never more slices than frames"
        );
        assert_eq!(equal_slices(100_000, 500).len(), 64, "capped");
    }

    #[test]
    fn transients_are_found_near_the_clicks() {
        let x = clicks(&[(0.0, 0.8), (0.25, 0.8), (0.5, 0.8), (0.75, 0.8)], 1.0);
        let found = detect_slices(&x, SR, 0.5);
        assert_eq!(found.len(), 4, "{found:?}");
        for (f, t) in found.iter().zip([0.0f32, 0.25, 0.5, 0.75]) {
            let at = (t * SR as f32) as i64;
            assert!((*f as i64 - at).abs() <= 300, "{f} vs {at}");
        }
    }

    #[test]
    fn sensitivity_controls_quiet_hits() {
        let x = clicks(&[(0.0, 0.8), (0.3, 0.8), (0.6, 0.8), (0.9, 0.08)], 1.2);
        assert_eq!(detect_slices(&x, SR, 1.0).len(), 4);
        assert_eq!(detect_slices(&x, SR, 0.0).len(), 3);
        assert_eq!(
            detect_slices(&vec![0.0; 10_000], SR, 0.5),
            vec![0],
            "silence: one slice"
        );
    }

    #[test]
    fn bpm_guess_picks_a_bar_count_in_range() {
        assert_eq!(guess_bpm(2 * SR, SR), 120.0);
        assert_eq!(guess_bpm(4 * SR, SR), 120.0);
        assert!((guess_bpm((1.6 * SR as f32) as u32, SR) - 150.0).abs() < 0.01);
        assert!((guess_bpm(3 * SR, SR) - 160.0).abs() < 0.01);
        assert_eq!(guess_bpm(SR / 2, SR), 120.0);
    }

    #[test]
    fn mono_mix_averages_channels() {
        assert_eq!(mono_mix(&[1.0, -1.0, 0.5, 0.5], 2), vec![0.0, 0.5]);
        assert_eq!(mono_mix(&[0.2, 0.4], 1), vec![0.2, 0.4]);
    }
}
