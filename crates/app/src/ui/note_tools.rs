//! The dock's note tools that change timing or pitch: Quantize, Humanize,
//! Legato, Reverse, Invert and Strum. Each is a pure function from a
//! pattern and a scope (indices of the notes it acts on) to the new
//! pattern and the scope notes' new indices. Notes are edited in place, so
//! the vector's order never changes (the engine keys its played-record
//! and chance hash by index); afterwards the notes a tool changed are
//! settled against their neighbours for the lane kind (`note_ops::settle`),
//! which may cut or remove notes. Selected notes hidden past the pattern
//! end are left as they are. A tool that changes nothing returns the
//! pattern as it was.

use super::key_entry::LaneKind;
use super::note_ops;
use super::poly_edit::Snap;
use acid_engine::project::{Note, NotePattern, STEPS, TICKS_PER_STEP};

/// Ticks a pattern can hold: starts stay below this.
fn limit() -> u16 {
    STEPS as u16 * TICKS_PER_STEP
}

fn end(n: &Note) -> u16 {
    n.start.saturating_add(n.len)
}

fn clamp_start(t: i32) -> u16 {
    t.clamp(0, limit() as i32 - 1) as u16
}

/// What a tool acts on when nothing is selected: every note starting
/// inside the pattern length (never notes hidden past the end).
pub fn default_scope(p: &NotePattern) -> Vec<usize> {
    let visible = p.ticks();
    (0..p.notes.len())
        .filter(|&i| p.notes[i].start < visible)
        .collect()
}

/// The valid indices of `scope`, sorted and unique, and those of them a
/// tool edits: the notes starting inside the pattern length (a selected
/// note hidden past the end is left as it is).
fn split(p: &NotePattern, scope: &[usize]) -> (Vec<usize>, Vec<usize>) {
    let mut s: Vec<usize> = scope
        .iter()
        .copied()
        .filter(|&i| i < p.notes.len())
        .collect();
    s.sort_unstable();
    s.dedup();
    let visible = p.ticks();
    let edit = s
        .iter()
        .copied()
        .filter(|&i| p.notes[i].start < visible)
        .collect();
    (s, edit)
}

/// Settles the notes the tool changed in `q` (an edited copy of `p`, same
/// order and count) and returns `q` with the scope notes' new indices.
/// Unchanged notes are never settled, so a tool that changed nothing
/// returns the pattern as it was.
fn finish(
    p: &NotePattern,
    mut q: NotePattern,
    scope: &[usize],
    kind: LaneKind,
) -> (NotePattern, Vec<usize>) {
    let changed: Vec<usize> = scope
        .iter()
        .copied()
        .filter(|&i| q.notes[i] != p.notes[i])
        .collect();
    if changed.is_empty() {
        return (q, scope.to_vec());
    }
    let map = note_ops::settle_map(&mut q, &changed, kind);
    let idx = scope.iter().filter_map(|&i| map[i]).collect();
    (q, idx)
}

/// Quantize: each start moves `strength` % of the way to its nearest grid
/// line; with `ends`, each end too (length at least 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quantize {
    pub grid: Snap,
    /// 0..=100.
    pub strength: u8,
    pub ends: bool,
}

impl Default for Quantize {
    fn default() -> Self {
        Self {
            grid: Snap::Step,
            strength: 100,
            ends: false,
        }
    }
}

/// `t` moved `strength` % of the way to its nearest multiple of `g`
/// (halfway goes later), rounded to the nearest tick (halves away from
/// `t`). With `below`, the nearest line under `limit()` instead.
fn toward_grid(t: i32, g: i32, strength: i32, below: bool) -> i32 {
    let mut target = (t + g / 2) / g * g;
    if below && target >= limit() as i32 {
        target -= g;
    }
    let d = (target - t) * strength;
    t + (d + d.signum() * 50) / 100
}

pub fn quantize(
    p: &NotePattern,
    scope: &[usize],
    params: &Quantize,
    kind: LaneKind,
) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    let g = params.grid.ticks().max(1) as i32;
    let strength = params.strength.min(100) as i32;
    let mut q = p.clone();
    for &i in &edit {
        let n = &mut q.notes[i];
        let start = clamp_start(toward_grid(n.start as i32, g, strength, true));
        let e = if params.ends {
            toward_grid(end(n) as i32, g, strength, false)
        } else {
            start as i32 + n.len as i32
        };
        n.start = start;
        n.len = (e - start as i32).clamp(1, (u16::MAX - start) as i32) as u16;
    }
    finish(p, q, &scope, kind)
}

/// Humanize: uniform random offsets per note, `timing` ticks and `vel`
/// either way, the same for the same seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Humanize {
    /// 0..=8 ticks.
    pub timing: u8,
    /// 0..=40.
    pub vel: u8,
    pub seed: u64,
}

impl Default for Humanize {
    fn default() -> Self {
        Self {
            timing: 2,
            vel: 10,
            seed: 1,
        }
    }
}

/// A well-mixed 64-bit hash (splitmix64's finaliser).
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// A uniform offset in `-r..=r` for note `i`'s `field` under `seed`.
fn offset(seed: u64, i: usize, field: u64, r: u8) -> i32 {
    if r == 0 {
        return 0;
    }
    let h = mix(seed ^ mix(((i as u64) << 2) | field));
    (h % (2 * r as u64 + 1)) as i32 - r as i32
}

pub fn humanize(
    p: &NotePattern,
    scope: &[usize],
    params: &Humanize,
    kind: LaneKind,
) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    let (timing, vel) = (params.timing.min(8), params.vel.min(40));
    let mut q = p.clone();
    for &i in &edit {
        let n = &mut q.notes[i];
        let start = clamp_start(n.start as i32 + offset(params.seed, i, 0, timing));
        n.len = n.len.min(u16::MAX - start).max(1);
        n.start = start;
        n.vel = (n.vel as i32 + offset(params.seed, i, 1, vel)).clamp(1, 127) as u8;
    }
    finish(p, q, &scope, kind)
}

/// Legato: each note runs to the next start strictly after its own among
/// every visible note of the lane, selected or not and at any pitch (so a
/// note stops at its neighbour, nothing is cut, and a chord stretches
/// together); the last runs to the pattern end.
pub fn legato(p: &NotePattern, scope: &[usize], kind: LaneKind) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    let ticks = p.ticks();
    let mut q = p.clone();
    for &i in &edit {
        let start = p.notes[i].start;
        let next = p
            .notes
            .iter()
            .map(|n| n.start)
            .filter(|&s| s > start && s < ticks)
            .min()
            .unwrap_or(ticks);
        q.notes[i].len = next.saturating_sub(start).max(1);
    }
    finish(p, q, &scope, kind)
}

/// Reverse: the scope mirrors in time over its span (first start to last
/// end): `start' = span_start + span_end - end`.
pub fn reverse(p: &NotePattern, scope: &[usize], kind: LaneKind) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    let Some(lo) = edit.iter().map(|&i| p.notes[i].start as i32).min() else {
        return (p.clone(), scope);
    };
    let hi = edit
        .iter()
        .map(|&i| end(&p.notes[i]) as i32)
        .max()
        .unwrap_or(lo);
    let mut q = p.clone();
    for &i in &edit {
        let n = &mut q.notes[i];
        let start = clamp_start(lo + hi - end(n) as i32);
        n.len = n.len.min(u16::MAX - start).max(1);
        n.start = start;
    }
    finish(p, q, &scope, kind)
}

/// Invert: pitches mirror within the scope's pitch range
/// (`pitch' = lo + hi - pitch`). Melodic lanes only: drums are unchanged.
pub fn invert(p: &NotePattern, scope: &[usize], kind: LaneKind) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    if kind == LaneKind::Drums {
        return (p.clone(), scope);
    }
    let Some(lo) = edit.iter().map(|&i| p.notes[i].pitch as i32).min() else {
        return (p.clone(), scope);
    };
    let hi = edit
        .iter()
        .map(|&i| p.notes[i].pitch as i32)
        .max()
        .unwrap_or(lo);
    let mut q = p.clone();
    for &i in &edit {
        let n = &mut q.notes[i];
        n.pitch = (lo + hi - n.pitch as i32).clamp(0, 127) as u8;
    }
    finish(p, q, &scope, kind)
}

/// Strum: notes sharing a start are staggered `ticks` apart in pitch
/// order (`up` = low first); each keeps its end where it can (length at
/// least 1). Melodic lanes only: drums are unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strum {
    /// 1..=8.
    pub ticks: u8,
    pub up: bool,
}

impl Default for Strum {
    fn default() -> Self {
        Self { ticks: 1, up: true }
    }
}

pub fn strum(
    p: &NotePattern,
    scope: &[usize],
    params: &Strum,
    kind: LaneKind,
) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    if kind == LaneKind::Drums {
        return (p.clone(), scope);
    }
    let gap = params.ticks.clamp(1, 8) as i32;
    let mut q = p.clone();
    let mut starts: Vec<u16> = edit.iter().map(|&i| p.notes[i].start).collect();
    starts.sort_unstable();
    starts.dedup();
    for s in starts {
        let mut chord: Vec<usize> = edit
            .iter()
            .copied()
            .filter(|&i| p.notes[i].start == s)
            .collect();
        // Pitch order, ties by index so the result is stable.
        chord.sort_by_key(|&i| (p.notes[i].pitch, i));
        if !params.up {
            chord.reverse();
        }
        for (k, &i) in chord.iter().enumerate() {
            let n = &mut q.notes[i];
            let e = end(n) as i32;
            let start = clamp_start(s as i32 + k as i32 * gap);
            n.start = start;
            n.len = (e - start as i32).clamp(1, (u16::MAX - start) as i32) as u16;
        }
    }
    finish(p, q, &scope, kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pat(notes: &[(u16, u16, u8)]) -> NotePattern {
        NotePattern {
            notes: notes
                .iter()
                .map(|&(s, l, p)| Note::new(s, l, p, 100))
                .collect(),
            length: 16,
        }
    }

    fn spans(p: &NotePattern) -> Vec<(u16, u16, u8)> {
        p.notes.iter().map(|n| (n.start, n.len, n.pitch)).collect()
    }

    fn all(p: &NotePattern) -> Vec<usize> {
        (0..p.notes.len()).collect()
    }

    // --- scope ---

    #[test]
    fn default_scope_skips_notes_past_the_end() {
        let p = pat(&[(0, 4, 60), (64, 4, 62), (63, 4, 64)]);
        assert_eq!(default_scope(&p), vec![0, 2]);
    }

    #[test]
    fn every_tool_leaves_an_empty_scope_alone() {
        let p = pat(&[(1, 4, 60), (7, 2, 64)]);
        let k = LaneKind::Poly;
        assert_eq!(quantize(&p, &[], &Quantize::default(), k).0, p);
        assert_eq!(humanize(&p, &[], &Humanize::default(), k).0, p);
        assert_eq!(legato(&p, &[], k).0, p);
        assert_eq!(reverse(&p, &[], k).0, p);
        assert_eq!(invert(&p, &[], k).0, p);
        assert_eq!(strum(&p, &[], &Strum::default(), k).0, p);
    }

    #[test]
    fn out_of_range_scope_indices_are_ignored() {
        let p = pat(&[(1, 4, 60)]);
        let (q, idx) = quantize(&p, &[0, 9], &Quantize::default(), LaneKind::Poly);
        assert_eq!(spans(&q), vec![(0, 4, 60)]);
        assert_eq!(idx, vec![0]);
    }

    // --- quantize ---

    #[test]
    fn quantize_moves_starts_to_the_nearest_line() {
        let p = pat(&[(1, 4, 60), (6, 3, 62), (10, 1, 64)]);
        let (q, idx) = quantize(&p, &all(&p), &Quantize::default(), LaneKind::Poly);
        assert_eq!(spans(&q), vec![(0, 4, 60), (8, 3, 62), (12, 1, 64)]);
        assert_eq!(idx, vec![0, 1, 2]);
    }

    #[test]
    fn quantize_strength_moves_part_of_the_way() {
        let p = pat(&[(13, 4, 60), (3, 4, 62)]);
        let params = Quantize {
            grid: Snap::Quarter,
            strength: 50,
            ends: false,
        };
        let (q, _) = quantize(&p, &all(&p), &params, LaneKind::Poly);
        // 13 → 16 halfway is 14.5 and 3 → 0 is 1.5: halves round toward
        // the line.
        assert_eq!(spans(&q), vec![(15, 4, 60), (1, 4, 62)]);
        let zero = Quantize {
            strength: 0,
            ..params
        };
        assert_eq!(quantize(&p, &all(&p), &zero, LaneKind::Poly).0, p);
    }

    #[test]
    fn quantize_ends_snaps_the_end_and_keeps_a_tick() {
        let p = pat(&[(1, 6, 60), (8, 1, 62)]);
        let params = Quantize {
            ends: true,
            ..Quantize::default()
        };
        let (q, _) = quantize(&p, &all(&p), &params, LaneKind::Poly);
        // 1..7 → 0..8; 8..9 → 8..8, kept 1 long.
        assert_eq!(spans(&q), vec![(0, 8, 60), (8, 1, 62)]);
    }

    #[test]
    fn quantize_never_starts_at_the_pattern_limit() {
        let mut p = pat(&[(255, 1, 60)]);
        p.length = 64;
        let params = Quantize {
            grid: Snap::Quarter,
            ..Quantize::default()
        };
        let (q, _) = quantize(&p, &[0], &params, LaneKind::Poly);
        assert_eq!(q.notes[0].start, 240);
    }

    #[test]
    fn quantize_only_touches_the_scope() {
        let p = pat(&[(1, 2, 60), (5, 2, 62)]);
        let (q, idx) = quantize(&p, &[1], &Quantize::default(), LaneKind::Poly);
        assert_eq!(spans(&q), vec![(1, 2, 60), (4, 2, 62)]);
        assert_eq!(idx, vec![1]);
    }

    #[test]
    fn quantize_acid_keeps_one_note_at_a_time() {
        // Both land on tick 4: the first stored stays, the other goes.
        let p = pat(&[(3, 4, 60), (5, 4, 62)]);
        let (q, idx) = quantize(&p, &all(&p), &Quantize::default(), LaneKind::Acid);
        assert_eq!(spans(&q), vec![(4, 4, 60)]);
        assert_eq!(idx, vec![0]);
    }

    #[test]
    fn quantize_poly_merges_same_pitch_overlaps() {
        let p = pat(&[(3, 4, 60), (5, 6, 60), (5, 2, 64)]);
        let (q, idx) = quantize(&p, &all(&p), &Quantize::default(), LaneKind::Poly);
        assert_eq!(spans(&q), vec![(4, 6, 60), (4, 2, 64)]);
        assert_eq!(idx, vec![0, 1]);
    }

    #[test]
    fn quantize_keeps_the_vector_order() {
        let p = pat(&[(9, 2, 60), (1, 2, 62)]);
        let (q, _) = quantize(&p, &all(&p), &Quantize::default(), LaneKind::Poly);
        assert_eq!(spans(&q), vec![(8, 2, 60), (0, 2, 62)]);
    }

    // --- humanize ---

    #[test]
    fn humanize_is_the_same_for_the_same_seed() {
        let p = pat(&[(8, 4, 60), (16, 4, 62), (24, 4, 64), (32, 4, 65)]);
        let h = Humanize {
            timing: 3,
            vel: 20,
            seed: 42,
        };
        let a = humanize(&p, &all(&p), &h, LaneKind::Poly);
        assert_eq!(a, humanize(&p, &all(&p), &h, LaneKind::Poly));
        let other = humanize(&p, &all(&p), &Humanize { seed: 43, ..h }, LaneKind::Poly);
        assert_ne!(a.0, other.0);
    }

    #[test]
    fn humanize_stays_within_its_ranges() {
        let notes: Vec<(u16, u16, u8)> = (0..32).map(|i| (8 + i * 6, 2, 40 + i as u8)).collect();
        let p = pat(&notes);
        let h = Humanize {
            timing: 2,
            vel: 10,
            seed: 7,
        };
        let (q, idx) = humanize(&p, &all(&p), &h, LaneKind::Poly);
        assert_eq!(idx, all(&p));
        let mut moved = false;
        for (a, b) in p.notes.iter().zip(&q.notes) {
            assert!((a.start as i32 - b.start as i32).abs() <= 2);
            assert!((a.vel as i32 - b.vel as i32).abs() <= 10);
            assert_eq!((a.len, a.pitch), (b.len, b.pitch));
            moved |= a != b;
        }
        assert!(moved);
    }

    #[test]
    fn humanize_clamps_start_and_velocity() {
        let mut p = pat(&[(0, 2, 60), (255, 1, 62)]);
        p.length = 64;
        p.notes[0].vel = 1;
        p.notes[1].vel = 127;
        for seed in 0..50 {
            let h = Humanize {
                timing: 8,
                vel: 40,
                seed,
            };
            let (q, _) = humanize(&p, &all(&p), &h, LaneKind::Drums);
            for n in &q.notes {
                assert!(n.start < 256);
                assert!((1..=127).contains(&n.vel));
            }
        }
    }

    #[test]
    fn humanize_at_zero_changes_nothing() {
        let p = pat(&[(4, 2, 60)]);
        let h = Humanize {
            timing: 0,
            vel: 0,
            seed: 9,
        };
        assert_eq!(humanize(&p, &[0], &h, LaneKind::Poly), (p.clone(), vec![0]));
    }

    // --- legato ---

    #[test]
    fn legato_runs_each_note_to_the_next_start() {
        let p = pat(&[(0, 1, 60), (4, 1, 67), (10, 1, 62)]);
        let (q, idx) = legato(&p, &all(&p), LaneKind::Acid);
        assert_eq!(spans(&q), vec![(0, 4, 60), (4, 6, 67), (10, 54, 62)]);
        assert_eq!(idx, vec![0, 1, 2]);
    }

    #[test]
    fn legato_single_note_runs_to_the_pattern_end() {
        let p = pat(&[(8, 2, 60)]);
        assert_eq!(
            spans(&legato(&p, &[0], LaneKind::Poly).0),
            vec![(8, 56, 60)]
        );
    }

    #[test]
    fn legato_stretches_a_chord_together() {
        let p = pat(&[(0, 2, 60), (0, 1, 64), (8, 2, 62)]);
        let (q, _) = legato(&p, &all(&p), LaneKind::Poly);
        assert_eq!(spans(&q), vec![(0, 8, 60), (0, 8, 64), (8, 56, 62)]);
    }

    #[test]
    fn legato_on_drums_uses_any_row() {
        let p = pat(&[(0, 1, 0), (4, 1, 3)]);
        let (q, _) = legato(&p, &all(&p), LaneKind::Drums);
        assert_eq!(spans(&q), vec![(0, 4, 0), (4, 60, 3)]);
    }

    #[test]
    fn legato_stops_at_an_unselected_neighbour() {
        // Note 1 is not in scope: it survives, and note 0 ends at its start.
        let p = pat(&[(0, 1, 60), (4, 1, 62), (8, 1, 64)]);
        let (q, idx) = legato(&p, &[0, 2], LaneKind::Acid);
        assert_eq!(spans(&q), vec![(0, 4, 60), (4, 1, 62), (8, 56, 64)]);
        assert_eq!(idx, vec![0, 2]);
    }

    #[test]
    fn legato_poly_stops_at_any_visible_start() {
        // The chord stops at the unselected 67; a hidden note past the end
        // is no neighbour.
        let p = pat(&[(0, 1, 60), (0, 1, 64), (4, 2, 67), (8, 1, 62), (70, 1, 50)]);
        let (q, idx) = legato(&p, &[0, 1, 3], LaneKind::Poly);
        assert_eq!(
            spans(&q),
            vec![(0, 4, 60), (0, 4, 64), (4, 2, 67), (8, 56, 62), (70, 1, 50)]
        );
        assert_eq!(idx, vec![0, 1, 3]);
    }

    #[test]
    fn hidden_selected_notes_are_left_alone() {
        // Note 1 starts past the pattern end (64) but is in scope.
        let p = pat(&[(1, 1, 60), (70, 3, 62)]);
        let k = LaneKind::Poly;
        let h = Humanize {
            timing: 8,
            vel: 40,
            seed: 3,
        };
        let s = Strum::default();
        let results = [
            quantize(&p, &[0, 1], &Quantize::default(), k),
            humanize(&p, &[0, 1], &h, k),
            legato(&p, &[0, 1], k),
            reverse(&p, &[0, 1], k),
            invert(&p, &[0, 1], k),
            strum(&p, &[0, 1], &s, k),
        ];
        for (q, idx) in &results {
            assert_eq!(q.notes[1], p.notes[1]);
            assert_eq!(idx, &vec![0, 1]);
        }
        // Spans and pitch ranges ignore it too.
        assert_eq!(results[2].0.notes[0].len, 63);
        assert_eq!(results[3].0.notes[0], p.notes[0]);
        assert_eq!(results[4].0.notes[0], p.notes[0]);
    }

    #[test]
    fn only_changed_notes_are_settled() {
        // Notes 0 and 1 overlap on one pitch already and don't move; only
        // note 2 does, so the old overlap is not merged.
        let p = pat(&[(0, 6, 60), (4, 4, 60), (9, 2, 64)]);
        let (q, idx) = quantize(&p, &all(&p), &Quantize::default(), LaneKind::Poly);
        assert_eq!(spans(&q), vec![(0, 6, 60), (4, 4, 60), (8, 2, 64)]);
        assert_eq!(idx, vec![0, 1, 2]);
    }

    #[test]
    fn settle_map_tracks_untouched_notes() {
        // Acid: note 2 (4..8) removes note 1 starting inside it.
        let mut p = pat(&[(0, 2, 60), (5, 1, 62), (4, 4, 64)]);
        let map = note_ops::settle_map(&mut p, &[2], LaneKind::Acid);
        assert_eq!(map, vec![Some(0), None, Some(1)]);
        assert_eq!(spans(&p), vec![(0, 2, 60), (4, 4, 64)]);
    }

    // --- reverse ---

    #[test]
    fn reverse_mirrors_over_the_span() {
        // Span 2..12.
        let p = pat(&[(2, 2, 60), (6, 6, 62)]);
        let (q, idx) = reverse(&p, &all(&p), LaneKind::Poly);
        assert_eq!(spans(&q), vec![(10, 2, 60), (2, 6, 62)]);
        assert_eq!(idx, vec![0, 1]);
    }

    #[test]
    fn reverse_single_note_stays_put() {
        let p = pat(&[(5, 3, 60)]);
        assert_eq!(reverse(&p, &[0], LaneKind::Acid), (p.clone(), vec![0]));
    }

    #[test]
    fn reverse_acid_settles_overlaps() {
        // Span 0..8: note 0 (0..8) stays, note 1 (2..4) moves to 4..6 and
        // cuts note 0 sounding into it (one note at a time).
        let p = pat(&[(0, 8, 60), (2, 2, 62)]);
        let (q, idx) = reverse(&p, &all(&p), LaneKind::Acid);
        assert_eq!(spans(&q), vec![(0, 4, 60), (4, 2, 62)]);
        assert_eq!(idx, vec![0, 1]);
    }

    // --- invert ---

    #[test]
    fn invert_mirrors_pitches_within_the_range() {
        let p = pat(&[(0, 2, 60), (2, 2, 64), (4, 2, 67)]);
        let (q, idx) = invert(&p, &all(&p), LaneKind::Poly);
        assert_eq!(spans(&q), vec![(0, 2, 67), (2, 2, 63), (4, 2, 60)]);
        assert_eq!(idx, vec![0, 1, 2]);
    }

    #[test]
    fn invert_single_note_and_drums_are_unchanged() {
        let p = pat(&[(0, 2, 60)]);
        assert_eq!(invert(&p, &[0], LaneKind::Acid).0, p);
        let d = pat(&[(0, 2, 0), (2, 2, 5)]);
        assert_eq!(
            invert(&d, &all(&d), LaneKind::Drums),
            (d.clone(), vec![0, 1])
        );
    }

    #[test]
    fn invert_poly_merges_new_same_pitch_overlaps() {
        // In scope 60 and 64 swap; note 0 (now 64) overlaps note 2 (64,
        // outside the scope).
        let p = pat(&[(0, 4, 60), (1, 2, 64), (2, 4, 64)]);
        let (q, idx) = invert(&p, &[0, 1], LaneKind::Poly);
        assert_eq!(spans(&q), vec![(0, 6, 64), (1, 2, 60)]);
        assert_eq!(idx, vec![0, 1]);
    }

    // --- strum ---

    #[test]
    fn strum_up_staggers_low_first_keeping_ends() {
        let p = pat(&[(0, 8, 67), (0, 8, 60), (0, 8, 64), (8, 4, 62)]);
        let params = Strum { ticks: 2, up: true };
        let (q, idx) = strum(&p, &all(&p), &params, LaneKind::Poly);
        assert_eq!(
            spans(&q),
            vec![(4, 4, 67), (0, 8, 60), (2, 6, 64), (8, 4, 62)]
        );
        assert_eq!(idx, vec![0, 1, 2, 3]);
    }

    #[test]
    fn strum_down_staggers_high_first_and_keeps_a_tick() {
        let p = pat(&[(0, 2, 60), (0, 2, 64), (0, 2, 67)]);
        let params = Strum {
            ticks: 3,
            up: false,
        };
        let (q, _) = strum(&p, &all(&p), &params, LaneKind::Poly);
        assert_eq!(spans(&q), vec![(6, 1, 60), (3, 1, 64), (0, 2, 67)]);
    }

    #[test]
    fn strum_clamps_at_the_pattern_limit() {
        let mut p = pat(&[(254, 2, 60), (254, 2, 64), (254, 2, 67)]);
        p.length = 64;
        let params = Strum { ticks: 8, up: true };
        let (q, _) = strum(&p, &all(&p), &params, LaneKind::Poly);
        // 254, 262 and 270 clamp to 255 (ends kept where they can be).
        assert_eq!(spans(&q), vec![(254, 2, 60), (255, 1, 64), (255, 1, 67)]);
    }

    #[test]
    fn strum_is_off_on_drums_and_a_lone_note_stays() {
        let d = pat(&[(0, 2, 0), (0, 2, 1)]);
        assert_eq!(strum(&d, &all(&d), &Strum::default(), LaneKind::Drums).0, d);
        let p = pat(&[(4, 2, 60)]);
        assert_eq!(strum(&p, &[0], &Strum::default(), LaneKind::Acid).0, p);
    }
}
