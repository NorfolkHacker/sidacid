//! The dock's note tools. Timing and pitch: Quantize, Humanize,
//! Legato, Reverse, Invert and Strum (the generators below add and remove
//! notes; see their section). Each is a pure function from a
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
use acid_engine::project::{MAX_NOTES, Note, NotePattern};
use acid_engine::scale::{Key, Scale};

fn end(n: &Note) -> u16 {
    n.start.saturating_add(n.len)
}

/// `t` as the start of a note that stays visible in `p`: inside the
/// pattern length (a note starting at or past it is hidden and silent).
fn clamp_start(p: &NotePattern, t: i32) -> u16 {
    t.clamp(0, p.ticks().max(1) as i32 - 1) as u16
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
/// `t`). With `below` (a pattern length in ticks), the last line under it
/// when the nearest is at or after it.
fn toward_grid(t: i32, g: i32, strength: i32, below: Option<i32>) -> i32 {
    let mut target = (t + g / 2) / g * g;
    if let Some(b) = below
        && target >= b
    {
        target = ((b - 1) / g * g).max(0);
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
        let below = Some(p.ticks() as i32);
        let start = clamp_start(p, toward_grid(n.start as i32, g, strength, below));
        let e = if params.ends {
            toward_grid(end(n) as i32, g, strength, None)
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
        let start = clamp_start(p, n.start as i32 + offset(params.seed, i, 0, timing));
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

/// Reverse: the scope mirrors in time over its audible span (first start
/// to last end, cut at the pattern end): `start' = span_start + span_end -
/// end`, a note ringing past the pattern end mirroring from there.
pub fn reverse(p: &NotePattern, scope: &[usize], kind: LaneKind) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    let ticks = p.ticks() as i32;
    let audible = |n: &Note| (end(n) as i32).min(ticks);
    let Some(lo) = edit.iter().map(|&i| p.notes[i].start as i32).min() else {
        return (p.clone(), scope);
    };
    let hi = edit
        .iter()
        .map(|&i| audible(&p.notes[i]))
        .max()
        .unwrap_or(lo);
    let mut q = p.clone();
    for &i in &edit {
        let mirrored = lo + hi - audible(&q.notes[i]);
        let start = clamp_start(p, mirrored);
        let n = &mut q.notes[i];
        n.len = n.len.min(u16::MAX - start).max(1);
        n.start = start;
    }
    finish(p, q, &scope, kind)
}

/// Invert: pitches mirror within the scope's pitch range
/// (`pitch' = lo + hi - pitch`), then go to their nearest in-key pitch
/// when `key` snaps to a scale other than Chromatic. Melodic lanes only:
/// drums and pads are unchanged.
pub fn invert(
    p: &NotePattern,
    scope: &[usize],
    kind: LaneKind,
    key: Key,
) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    if kind.is_rows() {
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
    if key.snap && key.scale != Scale::Chromatic {
        note_ops::snap_pitches(&mut q, &edit, key);
    }
    finish(p, q, &scope, kind)
}

/// Strum: notes sharing a start are staggered `ticks` apart in pitch
/// order (`up` = low first); each keeps its end where it can (length at
/// least 1). Melodic lanes only: drums and pads are unchanged.
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
    if kind.is_rows() {
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
            let start = clamp_start(p, s as i32 + k as i32 * gap);
            n.start = start;
            n.len = (e - start as i32).clamp(1, (u16::MAX - start) as i32) as u16;
        }
    }
    finish(p, q, &scope, kind)
}

// --- generators ---
//
// Randomize in scale, Arpeggiate and Euclidean add and remove notes. They
// keep the vector's order where they can: surviving notes stay where they
// were (Arpeggiate edits a chord's notes in place), new notes are appended,
// and removed notes close up. New and changed notes are then settled for
// the lane kind; no tool grows the pattern past `MAX_NOTES`.

/// The time a generator works over: the whole pattern with `whole` (the
/// tool runs with nothing selected) or when the visible scope is empty,
/// else the visible scope's span (first start to last end, cut at the
/// pattern end).
fn span(p: &NotePattern, edit: &[usize], whole: bool) -> (u16, u16) {
    let ticks = p.ticks();
    if whole {
        return (0, ticks);
    }
    match edit.iter().map(|&i| p.notes[i].start).min() {
        Some(lo) => {
            let hi = edit.iter().map(|&i| end(&p.notes[i])).max().unwrap_or(lo);
            (lo, hi.min(ticks))
        }
        None => (0, ticks),
    }
}

/// The grid lines of `g` ticks from `lo` (inclusive) to `hi` (exclusive).
fn cells(lo: u16, hi: u16, g: u16) -> impl Iterator<Item = u16> {
    (lo.div_ceil(g) * g..hi).step_by(g as usize)
}

/// A seeded roll for cell (or step) `k`'s `field`.
fn roll(seed: u64, k: usize, field: u64) -> u64 {
    mix(seed ^ mix(((k as u64) << 2) | field))
}

/// Builds the result from `slots` (each old note's new value, `None` when
/// removed) and `added` (appended, up to `MAX_NOTES`), settles every
/// changed or added note, and returns it with the new indices of the
/// surviving `keep` notes followed by the added ones. When nothing changed
/// the pattern is returned as it was.
fn rebuild(
    p: &NotePattern,
    slots: Vec<Option<Note>>,
    added: Vec<Note>,
    keep: &[usize],
    kind: LaneKind,
) -> (NotePattern, Vec<usize>) {
    let mut q = p.clone();
    q.notes.clear();
    let mut at = vec![None; slots.len()];
    let mut touched = Vec::new();
    for (i, slot) in slots.into_iter().enumerate() {
        if let Some(n) = slot {
            if n != p.notes[i] {
                touched.push(q.notes.len());
            }
            at[i] = Some(q.notes.len());
            q.notes.push(n);
        }
    }
    let first = q.notes.len();
    q.notes
        .extend(added.into_iter().take(MAX_NOTES.saturating_sub(first)));
    touched.extend(first..q.notes.len());
    let sel: Vec<usize> = keep
        .iter()
        .filter_map(|&i| at[i])
        .chain(first..q.notes.len())
        .collect();
    if q.notes == p.notes {
        return (q, sel);
    }
    let map = note_ops::settle_map(&mut q, &touched, kind);
    let sel = sel.into_iter().filter_map(|i| map[i]).collect();
    (q, sel)
}

fn ordered(a: u8, b: u8) -> (u8, u8) {
    (a.min(b), a.max(b))
}

/// Randomize in scale: the visible scope's notes are replaced by new ones
/// on the `snap` grid across the scope's span (the whole pattern with
/// `whole`), one per grid line with a `density` % chance, pitched in `key`
/// within the pitch range (any pitch when the key is Chromatic; the
/// nearest in-key pitch when the range holds none) or a random instrument
/// or pad on drums or pads, `len_min..=len_max` grid steps long (one step
/// on drums; cut at the span end). A span with no grid line in it holds
/// one cell at its start. The same seed gives the same notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Randomize {
    /// 10..=100 %.
    pub density: u8,
    pub pitch_lo: u8,
    pub pitch_hi: u8,
    /// Lengths in snap units.
    pub len_min: u8,
    pub len_max: u8,
    pub seed: u64,
}

impl Default for Randomize {
    fn default() -> Self {
        Self {
            density: 50,
            pitch_lo: 48,
            pitch_hi: 72,
            len_min: 1,
            len_max: 2,
            seed: 1,
        }
    }
}

/// Returns the new notes' indices.
pub fn randomize(
    p: &NotePattern,
    scope: &[usize],
    whole: bool,
    params: &Randomize,
    kind: LaneKind,
    key: Key,
    snap: Snap,
) -> (NotePattern, Vec<usize>) {
    let (_, edit) = split(p, scope);
    let (lo, hi) = span(p, &edit, whole);
    let g = snap.ticks().max(1);
    let density = params.density.min(100) as u64;
    let (plo, phi) = ordered(params.pitch_lo.min(127), params.pitch_hi.min(127));
    let (lmin, lmax) = ordered(params.len_min.max(1), params.len_max.max(1));
    let pool: Vec<u8> = if kind.is_rows() {
        (0..=kind.top_pitch()).collect()
    } else {
        (plo..=phi).filter(|&x| key.contains(x)).collect()
    };
    let seed = params.seed;
    let mut grid: Vec<u16> = cells(lo, hi, g).collect();
    if grid.is_empty() && lo < hi {
        // A span shorter than a grid step holds one cell at its start.
        grid.push(lo);
    }
    let mut added = Vec::new();
    for (k, t) in grid.into_iter().enumerate() {
        if roll(seed, k, 0) % 100 >= density {
            continue;
        }
        let pitch = if pool.is_empty() {
            key.snap(plo + (roll(seed, k, 1) % (phi - plo + 1) as u64) as u8)
        } else {
            pool[(roll(seed, k, 1) % pool.len() as u64) as usize]
        };
        // A drum hit is one cell: longer ones would run into the row's
        // next hit and merge with it. Pad hits resize, so take lengths.
        let units = if !kind.resizable() {
            1
        } else {
            lmin as u64 + roll(seed, k, 2) % (lmax - lmin + 1) as u64
        };
        let len = (units * g as u64).min((hi - t) as u64).max(1) as u16;
        added.push(Note::new(t, len, pitch, 100));
    }
    let mut slots: Vec<Option<Note>> = p.notes.iter().copied().map(Some).collect();
    for &i in &edit {
        slots[i] = None;
    }
    rebuild(p, slots, added, &[], kind)
}

/// The order Arpeggiate walks a chord's pitches in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArpStyle {
    #[default]
    Up,
    Down,
    /// Up then down, not repeating the top and bottom.
    UpDown,
    /// A seeded random pitch each step.
    Random,
}

/// Arpeggiate: visible scope notes sharing a start (a chord of two or
/// more) become a sequence through the chord's pitches, one every `rate`
/// from the chord's start to its last end, each `gate` % of `rate` long
/// (at least 1 tick, cut at the chord's end) and copying the fields of the
/// chord note it takes its pitch from. The chord's notes are reused in
/// place for the first steps (unused ones are removed), further steps are
/// appended. Single notes are unchanged. Melodic lanes only: drums and
/// pads are unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arp {
    pub style: ArpStyle,
    pub rate: Snap,
    /// 10..=100 %.
    pub gate: u8,
    /// For `ArpStyle::Random`.
    pub seed: u64,
}

impl Default for Arp {
    fn default() -> Self {
        Self {
            style: ArpStyle::Up,
            rate: Snap::Step,
            gate: 50,
            seed: 1,
        }
    }
}

pub fn arpeggiate(
    p: &NotePattern,
    scope: &[usize],
    params: &Arp,
    kind: LaneKind,
) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    if kind.is_rows() {
        return (p.clone(), scope);
    }
    let rate = params.rate.ticks().max(1);
    let len = ((rate as u32 * params.gate.clamp(1, 100) as u32 / 100) as u16).max(1);
    let ticks = p.ticks();
    let mut slots: Vec<Option<Note>> = p.notes.iter().copied().map(Some).collect();
    let mut added = Vec::new();
    let mut starts: Vec<u16> = edit.iter().map(|&i| p.notes[i].start).collect();
    starts.sort_unstable();
    starts.dedup();
    for s in starts {
        // In vector order: the notes reused for the steps.
        let chord: Vec<usize> = edit
            .iter()
            .copied()
            .filter(|&i| p.notes[i].start == s)
            .collect();
        let n = chord.len();
        if n < 2 {
            continue;
        }
        let mut by_pitch = chord.clone();
        by_pitch.sort_by_key(|&i| (p.notes[i].pitch, i));
        let chord_end = chord
            .iter()
            .map(|&i| end(&p.notes[i]))
            .max()
            .unwrap_or(s)
            .min(ticks);
        let mut k = 0;
        let mut t = s;
        while t < chord_end {
            let j = match params.style {
                ArpStyle::Up => k % n,
                ArpStyle::Down => n - 1 - k % n,
                ArpStyle::UpDown => {
                    let period = 2 * n - 2;
                    let m = k % period;
                    if m < n { m } else { period - m }
                }
                ArpStyle::Random => (roll(params.seed ^ mix(s as u64), k, 3) % n as u64) as usize,
            };
            let mut note = p.notes[by_pitch[j]];
            note.start = t;
            note.len = len.min(chord_end - t);
            if k < n {
                slots[chord[k]] = Some(note);
            } else {
                added.push(note);
            }
            k += 1;
            t += rate;
        }
        for &i in &chord[k.min(n)..] {
            slots[i] = None;
        }
    }
    rebuild(p, slots, added, &scope, kind)
}

/// Euclidean: `hits` notes spread as evenly as they go over `steps` grid
/// cells of `snap` from the span's first grid line (Bjorklund: E(3,8) =
/// x..x..x.), the pattern shifted `rotation` cells later and repeated
/// across the whole span (cell `c` takes step `c % steps`), each one cell
/// long, on `pitch` (drums: that row, 0..8; pads: that pad, 0..16). `steps` is capped at the
/// cells the span holds. Visible scope notes on that pitch are replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Euclid {
    pub hits: u8,
    /// 1..=64.
    pub steps: u8,
    pub rotation: u8,
    /// A MIDI pitch, or the drum row.
    pub pitch: u8,
}

impl Default for Euclid {
    fn default() -> Self {
        Self {
            hits: 3,
            steps: 8,
            rotation: 0,
            pitch: 60,
        }
    }
}

/// Bjorklund's even spread of `hits` onsets over `steps`.
fn bjorklund(hits: usize, steps: usize) -> Vec<bool> {
    if hits >= steps {
        return vec![true; steps];
    }
    if hits == 0 {
        return vec![false; steps];
    }
    let mut a: Vec<Vec<bool>> = vec![vec![true]; hits];
    let mut b: Vec<Vec<bool>> = vec![vec![false]; steps - hits];
    while b.len() > 1 {
        let m = a.len().min(b.len());
        let rest = if a.len() > m {
            a[m..].to_vec()
        } else {
            b[m..].to_vec()
        };
        for (x, y) in a.iter_mut().zip(&b) {
            x.extend(y);
        }
        a.truncate(m);
        b = rest;
    }
    a.into_iter().chain(b).flatten().collect()
}

pub fn euclid(
    p: &NotePattern,
    scope: &[usize],
    whole: bool,
    params: &Euclid,
    kind: LaneKind,
    snap: Snap,
) -> (NotePattern, Vec<usize>) {
    let (scope, edit) = split(p, scope);
    let (lo, hi) = span(p, &edit, whole);
    let g = snap.ticks().max(1);
    let cells: Vec<u16> = cells(lo, hi, g).collect();
    let steps = (params.steps.clamp(1, 64) as usize).min(cells.len());
    let pitch = params.pitch.min(kind.top_pitch());
    let mut added = Vec::new();
    if steps > 0 {
        let onsets = bjorklund(params.hits as usize, steps);
        let rot = params.rotation as usize % steps;
        for (c, &t) in cells.iter().enumerate() {
            if onsets[(c % steps + steps - rot) % steps] {
                added.push(Note::new(t, g.min(hi - t).max(1), pitch, 100));
            }
        }
    }
    let mut slots: Vec<Option<Note>> = p.notes.iter().copied().map(Some).collect();
    for &i in &edit {
        if p.notes[i].pitch == pitch {
            slots[i] = None;
        }
    }
    rebuild(p, slots, added, &scope, kind)
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
        assert_eq!(invert(&p, &[], k, Key::default()).0, p);
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

    #[test]
    fn quantize_keeps_starts_inside_a_short_pattern() {
        // 62 on a 16-step pattern: the nearest quarter line is the end
        // (64), so it goes to the last one under it.
        let p = pat(&[(62, 2, 60)]);
        let params = Quantize {
            grid: Snap::Quarter,
            ..Quantize::default()
        };
        let (q, _) = quantize(&p, &[0], &params, LaneKind::Poly);
        assert_eq!(q.notes[0].start, 48);
        // A quarter grid on a 2-step pattern: only line 0 lies inside it.
        let mut p = pat(&[(6, 1, 60)]);
        p.length = 2;
        assert_eq!(
            quantize(&p, &[0], &params, LaneKind::Poly).0.notes[0].start,
            0
        );
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

    #[test]
    fn humanize_keeps_starts_inside_a_short_pattern() {
        // A 4-step pattern is 16 ticks long: tick 15 never moves past 15.
        let mut p = pat(&[(15, 1, 60), (14, 1, 62)]);
        p.length = 4;
        for seed in 0..50 {
            let h = Humanize {
                timing: 8,
                vel: 0,
                seed,
            };
            let (q, _) = humanize(&p, &all(&p), &h, LaneKind::Poly);
            assert!(q.notes.iter().all(|n| n.start < 16), "seed {seed}");
        }
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
    fn legato_lengthens_pad_hits() {
        let p = pat(&[(0, 1, 0), (4, 1, 15)]);
        let (q, _) = legato(&p, &all(&p), LaneKind::Pads);
        assert_eq!(spans(&q), vec![(0, 4, 0), (4, 60, 15)]);
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
            invert(&p, &[0, 1], k, Key::default()),
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

    #[test]
    fn reverse_keeps_a_note_ringing_past_the_end_inside() {
        // A 4-step pattern (16 ticks): note 1 rings on to 30, past the end.
        // The span mirrored is the audible one, 0..16.
        let mut p = pat(&[(0, 4, 60), (10, 20, 62)]);
        p.length = 4;
        let (q, _) = reverse(&p, &all(&p), LaneKind::Poly);
        assert!(q.notes.iter().all(|n| n.start < 16), "{:?}", spans(&q));
        assert_eq!(spans(&q), vec![(12, 4, 60), (0, 20, 62)]);
    }

    // --- invert ---

    #[test]
    fn invert_mirrors_pitches_within_the_range() {
        let p = pat(&[(0, 2, 60), (2, 2, 64), (4, 2, 67)]);
        let (q, idx) = invert(&p, &all(&p), LaneKind::Poly, Key::default());
        assert_eq!(spans(&q), vec![(0, 2, 67), (2, 2, 63), (4, 2, 60)]);
        assert_eq!(idx, vec![0, 1, 2]);
    }

    #[test]
    fn invert_single_note_and_drums_are_unchanged() {
        let p = pat(&[(0, 2, 60)]);
        assert_eq!(invert(&p, &[0], LaneKind::Acid, Key::default()).0, p);
        let d = pat(&[(0, 2, 0), (2, 2, 5)]);
        assert_eq!(
            invert(&d, &all(&d), LaneKind::Drums, Key::default()),
            (d.clone(), vec![0, 1])
        );
        assert_eq!(
            invert(&d, &all(&d), LaneKind::Pads, Key::default()),
            (d.clone(), vec![0, 1])
        );
    }

    #[test]
    fn invert_poly_merges_new_same_pitch_overlaps() {
        // In scope 60 and 64 swap; note 0 (now 64) overlaps note 2 (64,
        // outside the scope).
        let p = pat(&[(0, 4, 60), (1, 2, 64), (2, 4, 64)]);
        let (q, idx) = invert(&p, &[0, 1], LaneKind::Poly, Key::default());
        assert_eq!(spans(&q), vec![(0, 6, 64), (1, 2, 60)]);
        assert_eq!(idx, vec![0, 1]);
    }

    #[test]
    fn invert_snaps_to_the_key_when_snap_is_on() {
        // C D G in C major: 60 64 67 mirror to 67 63 60, and 63 (D#) is
        // off the key.
        let p = pat(&[(0, 2, 60), (2, 2, 64), (4, 2, 67)]);
        let mut key = Key {
            root: 0,
            scale: Scale::Major,
            snap: true,
        };
        let (q, _) = invert(&p, &all(&p), LaneKind::Poly, key);
        assert!(
            q.notes.iter().all(|n| key.contains(n.pitch)),
            "{:?}",
            spans(&q)
        );
        assert_eq!(q.notes[1].pitch, key.snap(63));
        // Snap off, or Chromatic: the plain mirror.
        key.snap = false;
        assert_eq!(
            invert(&p, &all(&p), LaneKind::Poly, key).0.notes[1].pitch,
            63
        );
        let chromatic = Key {
            snap: true,
            ..Key::default()
        };
        assert_eq!(
            invert(&p, &all(&p), LaneKind::Poly, chromatic).0.notes[1].pitch,
            63
        );
    }

    #[test]
    fn generators_over_the_whole_pattern() {
        // One note at 0..4: `whole` spreads E(4,16) and Randomize over the
        // pattern rather than the note's span.
        let p = pat(&[(0, 4, 50)]);
        let (q, _) = euclid(
            &p,
            &[0],
            true,
            &eu(4, 16, 0, 60),
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(starts(&q), vec![0, 0, 16, 32, 48]);
        let params = Randomize {
            density: 100,
            len_min: 1,
            len_max: 1,
            ..rnd(1)
        };
        let (q, _) = randomize(
            &p,
            &[0],
            true,
            &params,
            LaneKind::Drums,
            c_minor(),
            Snap::Step,
        );
        assert_eq!(starts(&q), (0..16).map(|k| k * 4).collect::<Vec<u16>>());
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

    #[test]
    fn strum_keeps_a_late_chord_inside_the_pattern() {
        // A 16-step pattern ends at 64: a chord at 62 stays under it.
        let p = pat(&[(62, 2, 60), (62, 2, 64), (62, 2, 67)]);
        let params = Strum { ticks: 4, up: true };
        let (q, _) = strum(&p, &all(&p), &params, LaneKind::Poly);
        assert!(q.notes.iter().all(|n| n.start <= 63), "{:?}", spans(&q));
    }

    // --- randomize ---

    fn c_minor() -> Key {
        Key {
            root: 0,
            scale: Scale::Minor,
            snap: false,
        }
    }

    fn rnd(seed: u64) -> Randomize {
        Randomize {
            density: 60,
            pitch_lo: 48,
            pitch_hi: 72,
            len_min: 1,
            len_max: 3,
            seed,
        }
    }

    #[test]
    fn randomize_is_the_same_for_the_same_seed() {
        let p = pat(&[]);
        let k = LaneKind::Poly;
        let a = randomize(&p, &[], false, &rnd(5), k, c_minor(), Snap::Step);
        assert!(!a.0.notes.is_empty());
        assert_eq!(
            a,
            randomize(&p, &[], false, &rnd(5), k, c_minor(), Snap::Step)
        );
        let b = randomize(&p, &[], false, &rnd(6), k, c_minor(), Snap::Step);
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn randomize_stays_in_the_key_the_range_and_the_grid() {
        let p = pat(&[]);
        for seed in 0..40 {
            let (q, idx) = randomize(
                &p,
                &[],
                false,
                &rnd(seed),
                LaneKind::Poly,
                c_minor(),
                Snap::Step,
            );
            assert_eq!(idx, all(&q));
            for n in &q.notes {
                assert!(c_minor().contains(n.pitch), "{} out of key", n.pitch);
                assert!((48..=72).contains(&n.pitch));
                assert_eq!(n.start % 4, 0);
                assert!(n.start < 64 && end(n) <= 64);
            }
        }
    }

    #[test]
    fn randomize_chromatic_uses_any_pitch_in_the_range() {
        let p = pat(&[]);
        let mut seen = [false; 128];
        for seed in 0..40 {
            let (q, _) = randomize(
                &p,
                &[],
                false,
                &rnd(seed),
                LaneKind::Poly,
                Key::default(),
                Snap::Step,
            );
            for n in &q.notes {
                assert!((48..=72).contains(&n.pitch));
                seen[n.pitch as usize] = true;
            }
        }
        assert!(
            (48..=72)
                .filter(|&p| !c_minor().contains(p))
                .any(|p| seen[p as usize])
        );
    }

    #[test]
    fn randomize_replaces_the_scope_over_its_span() {
        // Scope 0 and 1 spans 8..24 (four steps); note 2 stays.
        let p = pat(&[(8, 4, 60), (20, 4, 62), (40, 4, 64)]);
        let params = Randomize {
            density: 100,
            len_min: 1,
            len_max: 1,
            ..rnd(3)
        };
        let (q, idx) = randomize(
            &p,
            &[0, 1],
            false,
            &params,
            LaneKind::Poly,
            c_minor(),
            Snap::Step,
        );
        assert_eq!(q.notes.len(), 5);
        assert_eq!(spans(&q)[0], (40, 4, 64));
        let starts: Vec<u16> = q.notes[1..].iter().map(|n| n.start).collect();
        assert_eq!(starts, vec![8, 12, 16, 20]);
        assert!(q.notes[1..].iter().all(|n| n.len == 4));
        assert_eq!(idx, vec![1, 2, 3, 4]);
    }

    #[test]
    fn randomize_lengths_stay_in_the_range_and_the_span() {
        let p = pat(&[(0, 4, 60), (8, 6, 62)]);
        let params = Randomize {
            density: 100,
            len_min: 2,
            len_max: 3,
            ..rnd(11)
        };
        let (q, _) = randomize(
            &p,
            &[0, 1],
            false,
            &params,
            LaneKind::Poly,
            c_minor(),
            Snap::Step,
        );
        // Span 0..14: cells 0, 4, 8 and 12, 8 or 12 ticks long, cut at 14
        // (same-pitch overlaps merge into longer notes).
        assert!(!q.notes.is_empty());
        for n in &q.notes {
            assert_eq!(n.start % 4, 0);
            assert!(end(n) <= 14);
            assert!(n.len >= 8 || end(n) == 14);
        }
    }

    #[test]
    fn randomize_drums_uses_instrument_rows() {
        let p = pat(&[]);
        let mut rows = [false; 8];
        for seed in 0..20 {
            let (q, _) = randomize(
                &p,
                &[],
                false,
                &rnd(seed),
                LaneKind::Drums,
                c_minor(),
                Snap::Step,
            );
            for n in &q.notes {
                assert!(n.pitch < 8);
                rows[n.pitch as usize] = true;
            }
        }
        assert!(rows.iter().all(|&r| r));
    }

    #[test]
    fn randomize_acid_is_one_note_at_a_time() {
        let p = pat(&[]);
        let params = Randomize {
            density: 100,
            len_min: 2,
            len_max: 4,
            ..rnd(9)
        };
        let (q, idx) = randomize(
            &p,
            &[],
            false,
            &params,
            LaneKind::Acid,
            c_minor(),
            Snap::Step,
        );
        assert_eq!(idx, all(&q));
        let mut s = q.notes.clone();
        s.sort_by_key(|n| n.start);
        for w in s.windows(2) {
            assert!(end(&w[0]) <= w[1].start);
        }
    }

    #[test]
    fn randomize_stops_at_the_note_cap() {
        // Notes hidden past the end fill the pattern almost to the cap.
        let mut p = pat(&[]);
        p.notes = vec![Note::new(100, 1, 60, 100); MAX_NOTES - 2];
        let params = Randomize {
            density: 100,
            len_min: 1,
            len_max: 1,
            ..rnd(1)
        };
        let (q, idx) = randomize(
            &p,
            &[],
            false,
            &params,
            LaneKind::Drums,
            c_minor(),
            Snap::Step,
        );
        assert_eq!(q.notes.len(), MAX_NOTES);
        assert_eq!(idx, vec![MAX_NOTES - 2, MAX_NOTES - 1]);
    }

    #[test]
    fn randomize_empty_scope_fills_the_pattern() {
        let p = pat(&[]);
        let params = Randomize {
            density: 100,
            len_min: 1,
            len_max: 1,
            ..rnd(2)
        };
        let (q, _) = randomize(
            &p,
            &[],
            false,
            &params,
            LaneKind::Poly,
            c_minor(),
            Snap::Quarter,
        );
        let starts: Vec<u16> = q.notes.iter().map(|n| n.start).collect();
        assert_eq!(starts, vec![0, 16, 32, 48]);
    }

    #[test]
    fn randomize_short_span_places_one_cell_at_its_start() {
        // One note 5..7: shorter than a step and off the grid.
        let p = pat(&[(5, 2, 60)]);
        let params = Randomize {
            density: 100,
            ..rnd(2)
        };
        let (q, idx) = randomize(
            &p,
            &[0],
            false,
            &params,
            LaneKind::Poly,
            c_minor(),
            Snap::Step,
        );
        assert_eq!(q.notes.len(), 1);
        assert_eq!((q.notes[0].start, q.notes[0].len), (5, 2));
        assert!(c_minor().contains(q.notes[0].pitch));
        assert_eq!(idx, vec![0]);
        // Density still applies: some seed at 10 % leaves it empty.
        let empty = (0..50).any(|seed| {
            let r = Randomize {
                density: 10,
                ..rnd(seed)
            };
            randomize(&p, &[0], false, &r, LaneKind::Poly, c_minor(), Snap::Step)
                .0
                .notes
                .is_empty()
        });
        assert!(empty);
    }

    #[test]
    fn randomize_pads_uses_every_pad_and_varies_lengths() {
        let p = pat(&[]);
        let (mut rows, mut lens) = ([false; 16], std::collections::BTreeSet::new());
        for seed in 0..40 {
            let (q, _) = randomize(
                &p,
                &[],
                false,
                &rnd(seed),
                LaneKind::Pads,
                c_minor(),
                Snap::Step,
            );
            for n in &q.notes {
                rows[n.pitch as usize] = true;
                lens.insert(n.len);
            }
        }
        assert!(
            rows.iter().all(|&r| r),
            "pads 1 to 16, not the key's pitches"
        );
        assert!(
            lens.len() > 1,
            "pad hits take the Shortest to Longest lengths"
        );
    }

    #[test]
    fn randomize_drums_hits_one_cell_each() {
        // At 100 % every cell gets its own hit, even with long lengths set:
        // same-row neighbours never overlap and merge.
        let p = pat(&[]);
        let params = Randomize {
            density: 100,
            len_min: 2,
            len_max: 4,
            ..rnd(3)
        };
        let (q, idx) = randomize(
            &p,
            &[],
            false,
            &params,
            LaneKind::Drums,
            c_minor(),
            Snap::Step,
        );
        assert_eq!(q.notes.len(), 16);
        assert!(q.notes.iter().all(|n| n.len == 4));
        assert_eq!(idx, all(&q));
    }

    // --- arpeggiate ---

    fn arp(style: ArpStyle) -> Arp {
        Arp {
            style,
            rate: Snap::Step,
            gate: 50,
            seed: 1,
        }
    }

    #[test]
    fn arp_up_runs_through_the_chord_in_place() {
        let p = pat(&[(0, 16, 64), (0, 16, 60), (0, 16, 67), (16, 4, 62)]);
        let (q, idx) = arpeggiate(&p, &all(&p), &arp(ArpStyle::Up), LaneKind::Poly);
        assert_eq!(
            spans(&q),
            vec![(0, 2, 60), (4, 2, 64), (8, 2, 67), (16, 4, 62), (12, 2, 60)]
        );
        assert_eq!(idx, vec![0, 1, 2, 3, 4]);
    }

    fn pitches(q: &NotePattern) -> Vec<u8> {
        let mut n = q.notes.clone();
        n.sort_by_key(|n| n.start);
        n.iter().map(|n| n.pitch).collect()
    }

    #[test]
    fn arp_down_and_up_down() {
        let p = pat(&[(0, 24, 60), (0, 24, 64), (0, 24, 67)]);
        let k = LaneKind::Poly;
        let down = arpeggiate(&p, &all(&p), &arp(ArpStyle::Down), k).0;
        assert_eq!(pitches(&down), vec![67, 64, 60, 67, 64, 60]);
        let ud = arpeggiate(&p, &all(&p), &arp(ArpStyle::UpDown), k).0;
        assert_eq!(pitches(&ud), vec![60, 64, 67, 64, 60, 64]);
    }

    #[test]
    fn arp_random_is_seeded_and_uses_chord_pitches() {
        let p = pat(&[(0, 64, 60), (0, 64, 64), (0, 64, 67)]);
        let k = LaneKind::Poly;
        let a = arpeggiate(&p, &all(&p), &arp(ArpStyle::Random), k);
        assert_eq!(a, arpeggiate(&p, &all(&p), &arp(ArpStyle::Random), k));
        assert_eq!(a.0.notes.len(), 16);
        assert!(a.0.notes.iter().all(|n| [60, 64, 67].contains(&n.pitch)));
        let b = arpeggiate(
            &p,
            &all(&p),
            &Arp {
                seed: 2,
                ..arp(ArpStyle::Random)
            },
            k,
        );
        assert_ne!(pitches(&a.0), pitches(&b.0));
    }

    #[test]
    fn arp_short_chord_drops_unused_notes_and_keeps_fields() {
        let mut p = pat(&[(0, 8, 64), (0, 8, 60), (0, 8, 67)]);
        p.notes[1].vel = 50;
        let params = Arp {
            gate: 100,
            ..arp(ArpStyle::Up)
        };
        let (q, idx) = arpeggiate(&p, &all(&p), &params, LaneKind::Poly);
        assert_eq!(spans(&q), vec![(0, 4, 60), (4, 4, 64)]);
        assert_eq!(q.notes[0].vel, 50);
        assert_eq!(q.notes[1].vel, 100);
        assert_eq!(idx, vec![0, 1]);
    }

    #[test]
    fn arp_gate_keeps_a_tick_and_clips_to_the_chord() {
        let p = pat(&[(0, 6, 60), (0, 6, 64)]);
        let params = Arp {
            gate: 10,
            ..arp(ArpStyle::Up)
        };
        let (q, _) = arpeggiate(&p, &all(&p), &params, LaneKind::Poly);
        assert_eq!(spans(&q), vec![(0, 1, 60), (4, 1, 64)]);
        let full = Arp {
            gate: 100,
            ..arp(ArpStyle::Up)
        };
        let (q, _) = arpeggiate(&p, &all(&p), &full, LaneKind::Poly);
        assert_eq!(spans(&q), vec![(0, 4, 60), (4, 2, 64)]);
    }

    #[test]
    fn arp_leaves_drums_single_notes_and_empty_scopes() {
        let d = pat(&[(0, 8, 0), (0, 8, 1)]);
        let a = arp(ArpStyle::Up);
        assert_eq!(arpeggiate(&d, &all(&d), &a, LaneKind::Drums).0, d);
        let p = pat(&[(0, 8, 60), (8, 8, 64)]);
        assert_eq!(arpeggiate(&p, &all(&p), &a, LaneKind::Poly).0, p);
        let c = pat(&[(0, 8, 60), (0, 8, 64)]);
        assert_eq!(arpeggiate(&c, &[], &a, LaneKind::Poly).0, c);
    }

    // --- euclid ---

    fn eu(hits: u8, steps: u8, rotation: u8, pitch: u8) -> Euclid {
        Euclid {
            hits,
            steps,
            rotation,
            pitch,
        }
    }

    fn starts(q: &NotePattern) -> Vec<u16> {
        q.notes.iter().map(|n| n.start).collect()
    }

    #[test]
    fn euclid_three_in_eight() {
        let p = pat(&[]);
        let (q, idx) = euclid(&p, &[], false, &eu(3, 8, 0, 60), LaneKind::Poly, Snap::Step);
        // x..x..x. twice over the 16-step pattern.
        assert_eq!(
            spans(&q),
            vec![
                (0, 4, 60),
                (12, 4, 60),
                (24, 4, 60),
                (32, 4, 60),
                (44, 4, 60),
                (56, 4, 60)
            ]
        );
        assert_eq!(idx, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn euclid_repeats_its_cycle_across_the_span() {
        // Five cells of E(1,2): x.x.x
        let p = pat(&[(0, 4, 50), (16, 4, 50)]);
        let (q, _) = euclid(
            &p,
            &[0, 1],
            false,
            &eu(1, 2, 0, 60),
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(starts(&q), vec![0, 16, 0, 8, 16]);
        // E(1,3) rotated by one: .x. then .x -> cells 1 and 4.
        let (q, _) = euclid(
            &p,
            &[0, 1],
            false,
            &eu(1, 3, 1, 60),
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(starts(&q), vec![0, 16, 4, 16]);
    }

    #[test]
    fn euclid_five_in_eight() {
        let p = pat(&[]);
        let (q, _) = euclid(&p, &[], false, &eu(5, 8, 0, 60), LaneKind::Poly, Snap::Step);
        // x.xx.xx. twice.
        assert_eq!(starts(&q), vec![0, 8, 12, 20, 24, 32, 40, 44, 52, 56]);
    }

    #[test]
    fn euclid_rotation_shifts_later() {
        let p = pat(&[]);
        let (q, _) = euclid(&p, &[], false, &eu(3, 8, 1, 60), LaneKind::Poly, Snap::Step);
        // .x..x..x twice.
        assert_eq!(starts(&q), vec![4, 16, 28, 36, 48, 60]);
        let (q, _) = euclid(&p, &[], false, &eu(3, 8, 9, 60), LaneKind::Poly, Snap::Step);
        assert_eq!(starts(&q), vec![4, 16, 28, 36, 48, 60]);
    }

    #[test]
    fn euclid_drums_use_the_row() {
        let p = pat(&[]);
        let (q, _) = euclid(&p, &[], false, &eu(2, 4, 0, 3), LaneKind::Drums, Snap::Step);
        assert_eq!(&spans(&q)[..2], &[(0, 4, 3), (8, 4, 3)]);
        assert_eq!(starts(&q), (0..8).map(|k| k * 8).collect::<Vec<u16>>());
        assert!(q.notes.iter().all(|n| n.pitch == 3));
        let (q, _) = euclid(
            &p,
            &[],
            false,
            &eu(1, 4, 0, 12),
            LaneKind::Drums,
            Snap::Step,
        );
        assert_eq!(q.notes[0].pitch, 7);
    }

    #[test]
    fn euclid_pads_use_the_row_up_to_pad_16() {
        let p = pat(&[]);
        let (q, _) = euclid(&p, &[], false, &eu(1, 4, 0, 12), LaneKind::Pads, Snap::Step);
        assert!(q.notes.iter().all(|n| n.pitch == 12));
        let (q, _) = euclid(&p, &[], false, &eu(1, 4, 0, 60), LaneKind::Pads, Snap::Step);
        assert!(q.notes.iter().all(|n| n.pitch == 15));
    }

    #[test]
    fn euclid_replaces_only_that_pitch_in_the_span() {
        // Span 0..12 (three steps); the 60s go, 62 and the outside note stay.
        let p = pat(&[(0, 4, 60), (4, 4, 60), (8, 4, 62), (40, 4, 60)]);
        let (q, idx) = euclid(
            &p,
            &[0, 1, 2],
            false,
            &eu(1, 3, 0, 60),
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(spans(&q), vec![(8, 4, 62), (40, 4, 60), (0, 4, 60)]);
        assert_eq!(idx, vec![0, 2]);
    }

    #[test]
    fn euclid_caps_steps_at_the_cells() {
        let p = pat(&[(0, 4, 50), (12, 4, 50)]);
        let (q, _) = euclid(
            &p,
            &[0, 1],
            false,
            &eu(8, 8, 0, 60),
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(starts(&q), vec![0, 12, 0, 4, 8, 12]);
    }

    #[test]
    fn euclid_acid_settles_against_its_neighbours() {
        let p = pat(&[(2, 8, 62)]);
        let (q, idx) = euclid(
            &p,
            &[],
            false,
            &eu(2, 4, 0, 60),
            LaneKind::Acid,
            Snap::Quarter,
        );
        assert_eq!(spans(&q), vec![(0, 16, 60), (32, 16, 60)]);
        assert_eq!(idx, vec![0, 1]);
    }

    #[test]
    fn euclid_no_hits_changes_nothing() {
        let p = pat(&[(0, 4, 50)]);
        assert_eq!(
            euclid(
                &p,
                &[0],
                false,
                &eu(0, 8, 0, 60),
                LaneKind::Poly,
                Snap::Step
            ),
            (p.clone(), vec![0])
        );
    }

    #[test]
    fn generators_leave_hidden_selected_notes_alone() {
        // Note 1 is past the end (64): it stays, and the span is note 0's.
        let p = pat(&[(8, 8, 60), (70, 3, 60), (8, 8, 64)]);
        let r = Randomize {
            density: 100,
            len_min: 1,
            len_max: 1,
            ..rnd(4)
        };
        let k = LaneKind::Poly;
        let (q, _) = randomize(&p, &[0, 1, 2], false, &r, k, c_minor(), Snap::Step);
        assert_eq!(q.notes[0], p.notes[1]);
        assert_eq!(starts(&q), vec![70, 8, 12]);
        let (q, idx) = arpeggiate(&p, &[0, 1, 2], &arp(ArpStyle::Up), k);
        assert_eq!(q.notes[1], p.notes[1]);
        assert_eq!(idx, vec![0, 1, 2]);
        let (q, idx) = euclid(&p, &[0, 1, 2], false, &eu(2, 2, 0, 60), k, Snap::Step);
        assert_eq!(
            spans(&q),
            vec![(70, 3, 60), (8, 8, 64), (8, 4, 60), (12, 4, 60)]
        );
        assert_eq!(idx, vec![0, 1, 2, 3]);
    }
}
