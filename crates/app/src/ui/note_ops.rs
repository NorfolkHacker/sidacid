//! Pure operations on a note selection (indices into a `NotePattern`),
//! shared by every lane of the dock: pick, move, resize, delete, copy,
//! paste, duplicate, and settling edited notes against their neighbours;
//! setting one field (velocity, chance, ratchet, condition, accent, slide)
//! on the selection, and the labels the inspector and badges show.

use super::key_entry::LaneKind;
use super::poly_edit::make_mono;
use acid_engine::project::{Cond, MAX_NOTES, Note, NotePattern, STEPS, TICKS_PER_STEP};
use acid_engine::scale::Key;

/// Why a paste was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteError {
    TooMany,
    WrongKind,
}

/// What the user is told when an edit would pass `MAX_NOTES` (paste,
/// duplicate and Ctrl-drag alike).
pub fn too_many_notes() -> String {
    format!("That would put more than {MAX_NOTES} notes in the pattern")
}

fn end(n: &Note) -> u16 {
    n.start.saturating_add(n.len)
}

/// Ticks a pattern can hold (starts at or past this are never played).
fn limit() -> u16 {
    STEPS as u16 * TICKS_PER_STEP
}

fn max_pitch(kind: LaneKind) -> i32 {
    kind.top_pitch() as i32
}

/// Indices of the notes equal to the selected values; each selected value
/// claims the first unclaimed equal note. Sorted, unique.
pub fn resolve(p: &NotePattern, selected: &[Note]) -> Vec<usize> {
    let mut used = vec![false; p.notes.len()];
    for s in selected {
        if let Some(i) = (0..p.notes.len()).find(|&i| !used[i] && p.notes[i] == *s) {
            used[i] = true;
        }
    }
    (0..used.len()).filter(|&i| used[i]).collect()
}

/// The notes at `idx` (out-of-range indices are skipped).
pub fn values(p: &NotePattern, idx: &[usize]) -> Vec<Note> {
    idx.iter()
        .filter_map(|&i| p.notes.get(i).copied())
        .collect()
}

/// Notes (starting inside the visible pattern) overlapping the tick span
/// and pitch span of a box dragged between two corners, in any order.
pub fn in_box(p: &NotePattern, t0: u16, t1: u16, lo: u8, hi: u8) -> Vec<usize> {
    let (ta, tb) = (t0.min(t1), t0.max(t1));
    let (pa, pb) = (lo.min(hi), lo.max(hi));
    let visible = p.ticks();
    (0..p.notes.len())
        .filter(|&i| {
            let n = &p.notes[i];
            n.start < visible && n.start <= tb && end(n) > ta && (pa..=pb).contains(&n.pitch)
        })
        .collect()
}

/// Notes starting in `[t0, t1)` and inside the visible pattern.
pub fn in_range(p: &NotePattern, t0: u16, t1: u16) -> Vec<usize> {
    let visible = p.ticks();
    (0..p.notes.len())
        .filter(|&i| {
            let start = p.notes[i].start;
            start < visible && (t0..t1).contains(&start)
        })
        .collect()
}

/// Moves the group by `(dt, dp)` ticks and pitches, clamped as a whole so
/// no note starts before 0 or at/after the pattern limit and no pitch
/// leaves the lane's range. Positions in the vector don't change.
pub fn shift(p: &mut NotePattern, idx: &[usize], dt: i32, dp: i32, kind: LaneKind) -> Vec<usize> {
    let sel = values(p, idx);
    if sel.is_empty() {
        return idx.to_vec();
    }
    let ext = |f: fn(&Note) -> i32| {
        let v = sel.iter().map(f);
        (v.clone().min().unwrap_or(0), v.max().unwrap_or(0))
    };
    let (s_lo, s_hi) = ext(|n| n.start as i32);
    let (p_lo, p_hi) = ext(|n| n.pitch as i32);
    let dt = dt.max(-s_lo).min((limit() as i32 - 1 - s_hi).max(0));
    let dp = dp.max(-p_lo).min((max_pitch(kind) - p_hi).max(0));
    if (dt, dp) != (0, 0) {
        for &i in idx {
            if let Some(n) = p.notes.get_mut(i) {
                n.start = (n.start as i32 + dt) as u16;
                n.pitch = (n.pitch as i32 + dp) as u8;
            }
        }
    }
    idx.to_vec()
}

/// Moves each note at `idx` to the next (`up`) or previous in-scale pitch
/// of `key` (from an off-scale pitch: the nearest one that way). When any
/// note can't move (the end of the pitch range) none does, like `shift`.
/// Positions in the vector don't change.
pub fn transpose_in_key(p: &mut NotePattern, idx: &[usize], up: bool, key: Key) -> Vec<usize> {
    let stuck = values(p, idx)
        .iter()
        .any(|n| key.step(n.pitch, up) == n.pitch);
    if !stuck {
        for &i in idx {
            if let Some(n) = p.notes.get_mut(i) {
                n.pitch = key.step(n.pitch, up);
            }
        }
    }
    idx.to_vec()
}

/// Moves each note at `idx` to its nearest in-scale pitch (`Key::snap`).
pub fn snap_pitches(p: &mut NotePattern, idx: &[usize], key: Key) {
    for &i in idx {
        if let Some(n) = p.notes.get_mut(i) {
            n.pitch = key.snap(n.pitch);
        }
    }
}

/// Lengthens (or shortens) each note by `dlen` ticks: at least 1, and never
/// so long that its end passes `u16::MAX`.
pub fn resize(p: &mut NotePattern, idx: &[usize], dlen: i32) {
    for &i in idx {
        if let Some(n) = p.notes.get_mut(i) {
            let cap = (u16::MAX - n.start) as i32;
            n.len = (n.len as i32 + dlen).clamp(1, cap.max(1)) as u16;
        }
    }
}

/// Lengthens the pattern so the notes at `idx` end inside it, up to the
/// longest pattern (`STEPS` steps); a note running past that is cut there.
/// The pattern never shrinks.
pub fn grow_to_fit(p: &mut NotePattern, idx: &[usize]) {
    let max = STEPS as u16 * TICKS_PER_STEP;
    let mut end = 0;
    for &i in idx {
        if let Some(n) = p.notes.get_mut(i)
            && n.start < max
        {
            n.len = n.len.min(max - n.start);
            end = end.max(n.start + n.len);
        }
    }
    p.length = p.length.max(end.div_ceil(TICKS_PER_STEP) as u8);
}

/// Removes the notes at `idx`.
pub fn delete(p: &mut NotePattern, idx: &[usize]) {
    let mut i = 0;
    p.notes.retain(|_| {
        let keep = !idx.contains(&i);
        i += 1;
        keep
    });
}

/// One note field and the value to give it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Vel(u8),
    Chance(u8),
    Ratchet(u8),
    Cond(Cond),
    Accent(bool),
    Slide(bool),
}

/// Sets `f` on every note at `idx`, clamped to the field's range (an
/// invalid condition is ignored). True when any note changed. Positions in
/// the vector don't change.
pub fn set_field(p: &mut NotePattern, idx: &[usize], f: Field) -> bool {
    if let Field::Cond(c) = f
        && !c.is_valid()
    {
        return false;
    }
    let mut changed = false;
    for &i in idx {
        let Some(n) = p.notes.get_mut(i) else {
            continue;
        };
        let before = *n;
        match f {
            Field::Vel(v) => n.vel = v.clamp(1, 127),
            Field::Chance(c) => n.chance = c.min(100),
            Field::Ratchet(r) => n.ratchet = r.clamp(1, 8),
            Field::Cond(c) => n.cond = c,
            Field::Accent(a) => n.accent = a,
            Field::Slide(s) => n.slide = s,
        }
        changed |= *n != before;
    }
    changed
}

/// The value every note in `notes` shares, or None when they differ (or
/// there are none).
pub fn common<T: PartialEq>(notes: &[Note], get: impl Fn(&Note) -> T) -> Option<T> {
    let first = get(notes.first()?);
    notes.iter().all(|n| get(n) == first).then_some(first)
}

/// Every condition in the inspector's order: Always, the ratios 1:2, 2:2,
/// 1:3 … 8:8, then First, Not first, Fill, Not fill, Pre, Not pre.
pub fn conds() -> Vec<Cond> {
    let mut v = vec![Cond::Always];
    for b in 2..=8 {
        for a in 1..=b {
            v.push(Cond::Ratio(a, b));
        }
    }
    v.extend([
        Cond::First,
        Cond::NotFirst,
        Cond::Fill,
        Cond::NotFill,
        Cond::Pre,
        Cond::NotPre,
    ]);
    v
}

/// A condition as the inspector lists it.
pub fn cond_label(c: Cond) -> String {
    match c {
        Cond::Always => "Always".into(),
        Cond::Ratio(a, b) => format!("{a}:{b}"),
        Cond::First => "First".into(),
        Cond::NotFirst => "Not first".into(),
        Cond::Fill => "Fill".into(),
        Cond::NotFill => "Not fill".into(),
        Cond::Pre => "Pre".into(),
        Cond::NotPre => "Not pre".into(),
    }
}

/// The short text on a note's badge: its condition, chance and ratchet,
/// those not at their defaults, e.g. "1:4 50% ×3". Empty for a plain note.
pub fn badge_text(n: &Note) -> String {
    let cond = match n.cond {
        Cond::Always => None,
        Cond::Ratio(a, b) => Some(format!("{a}:{b}")),
        Cond::First => Some("1st".into()),
        Cond::NotFirst => Some("!1st".into()),
        Cond::Fill => Some("F".into()),
        Cond::NotFill => Some("!F".into()),
        Cond::Pre => Some("P".into()),
        Cond::NotPre => Some("!P".into()),
    };
    let chance = (n.chance < 100).then(|| format!("{}%", n.chance));
    let ratchet = (n.ratchet > 1).then(|| format!("×{}", n.ratchet));
    [cond, chance, ratchet]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The notes at `idx` with starts relative to `origin` (<= every start).
pub fn copy(p: &NotePattern, idx: &[usize], origin: u16) -> Vec<Note> {
    values(p, idx)
        .into_iter()
        .map(|n| Note {
            start: n.start.saturating_sub(origin),
            ..n
        })
        .collect()
}

/// Appends `notes` shifted to `at` (those starting at/after the pattern
/// limit are dropped), settles them against their neighbours and returns
/// their indices. Refuses, changing nothing, when the pattern would exceed
/// `MAX_NOTES` or a drum or pad lane would get a pitch with no row.
pub fn paste(
    p: &mut NotePattern,
    notes: &[Note],
    at: u16,
    kind: LaneKind,
) -> Result<Vec<usize>, PasteError> {
    if kind.is_rows() && notes.iter().any(|n| n.pitch > kind.top_pitch()) {
        return Err(PasteError::WrongKind);
    }
    let placed: Vec<Note> = notes
        .iter()
        .filter_map(|n| {
            let start = n.start as u32 + at as u32;
            (start < limit() as u32).then_some(Note {
                start: start as u16,
                ..*n
            })
        })
        .collect();
    if p.notes.len() + placed.len() > MAX_NOTES {
        return Err(PasteError::TooMany);
    }
    let first = p.notes.len();
    p.notes.extend(placed);
    let touched: Vec<usize> = (first..p.notes.len()).collect();
    Ok(settle(p, &touched, kind))
}

/// Pastes a copy of the notes at `idx` right after them: the offset is
/// their span (max end - min start) rounded up to a multiple of `snap_ticks`.
pub fn duplicate(
    p: &mut NotePattern,
    idx: &[usize],
    snap_ticks: u16,
    kind: LaneKind,
) -> Result<Vec<usize>, PasteError> {
    let sel = values(p, idx);
    let (Some(lo), Some(hi)) = (sel.iter().map(|n| n.start).min(), sel.iter().map(end).max())
    else {
        return Ok(Vec::new());
    };
    let snap = snap_ticks.max(1);
    let span = (hi - lo).div_ceil(snap).saturating_mul(snap);
    let clip = copy(p, idx, lo);
    paste(p, &clip, lo.saturating_add(span), kind)
}

/// New index of each old note after the ones where `alive` is false were
/// removed (`None` for the removed).
fn remap(alive: &[bool]) -> Vec<Option<usize>> {
    let mut next = 0;
    alive
        .iter()
        .map(|&a| {
            a.then(|| {
                next += 1;
                next - 1
            })
        })
        .collect()
}

/// Settles edited notes against their neighbours and returns the touched
/// notes' new indices. Acid: in descending start order each touched note
/// runs `make_mono` (cutting or removing what it overlaps), so a touched
/// note only ever cuts the touched note sounding into it and never removes
/// a later one; of touched notes sharing a start the first stored stays,
/// the rest go (one note at a time). Poly/Drums:
/// each touched note absorbs the same-pitch notes it overlaps (union span;
/// it keeps its own other fields).
pub fn settle(p: &mut NotePattern, touched: &[usize], kind: LaneKind) -> Vec<usize> {
    let map = settle_map(p, touched, kind);
    let mut out: Vec<usize> = touched
        .iter()
        .filter_map(|&i| map.get(i).copied().flatten())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// `settle`, returning where every note before it now is (`None` once
/// removed), touched or not.
pub fn settle_map(p: &mut NotePattern, touched: &[usize], kind: LaneKind) -> Vec<Option<usize>> {
    let mut whole: Vec<Option<usize>> = (0..p.notes.len()).map(Some).collect();
    let mut ids: Vec<usize> = touched
        .iter()
        .copied()
        .filter(|&i| i < p.notes.len())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if kind == LaneKind::Acid {
        ids.sort_by_key(|&i| (std::cmp::Reverse(p.notes[i].start), i));
    } else {
        ids.sort_by_key(|&i| (p.notes[i].start, i));
    }
    // Current index of each id, `None` once the note was removed.
    let mut at: Vec<Option<usize>> = ids.iter().map(|&i| Some(i)).collect();
    for k in 0..ids.len() {
        let Some(cur) = at[k] else { continue };
        let alive = if kind == LaneKind::Acid {
            let keep = p.notes[cur];
            let alive: Vec<bool> = p
                .notes
                .iter()
                .enumerate()
                .map(|(i, n)| i == cur || !(n.start >= keep.start && n.start < end(&keep)))
                .collect();
            make_mono(p, cur);
            alive
        } else {
            absorb(p, cur)
        };
        let map = remap(&alive);
        for slot in at.iter_mut().chain(whole.iter_mut()) {
            *slot = slot.and_then(|i| map[i]);
        }
    }
    whole
}

/// Merges every visible same-pitch note overlapping note `cur` into it
/// (repeating while the union reaches more). Returns which old indices
/// survive.
fn absorb(p: &mut NotePattern, cur: usize) -> Vec<bool> {
    let visible = p.ticks();
    let mut alive = vec![true; p.notes.len()];
    let (mut lo, mut hi) = (p.notes[cur].start, end(&p.notes[cur]));
    let pitch = p.notes[cur].pitch;
    loop {
        let mut grew = false;
        for (i, n) in p.notes.iter().enumerate() {
            if i != cur
                && alive[i]
                && n.pitch == pitch
                && n.start < visible
                && n.start < hi
                && lo < end(n)
            {
                alive[i] = false;
                lo = lo.min(n.start);
                hi = hi.max(end(n));
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    p.notes[cur].start = lo;
    p.notes[cur].len = hi - lo;
    let mut i = 0;
    p.notes.retain(|_| {
        i += 1;
        alive[i - 1]
    });
    alive
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::key_entry::LaneKind;
    use acid_engine::project::{Cond, MAX_NOTES, Note, NotePattern};
    use acid_engine::scale::{Key, Scale};

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
        let mut v: Vec<_> = p.notes.iter().map(|n| (n.start, n.len, n.pitch)).collect();
        v.sort();
        v
    }

    fn c_major() -> Key {
        Key {
            root: 0,
            scale: Scale::Major,
            snap: true,
        }
    }

    #[test]
    fn set_field_sets_each_note_at_the_indices_clamped() {
        let mut p = pat(&[(0, 4, 60), (4, 4, 62), (8, 4, 64)]);
        assert!(set_field(&mut p, &[0, 2], Field::Chance(50)));
        let chances: Vec<_> = p.notes.iter().map(|n| n.chance).collect();
        assert_eq!(chances, [50, 100, 50], "only the indexed notes");
        assert!(!set_field(&mut p, &[0, 2], Field::Chance(50)), "no change");
        set_field(&mut p, &[1], Field::Chance(200));
        assert_eq!(p.notes[1].chance, 100, "clamped");
        set_field(&mut p, &[0], Field::Ratchet(0));
        assert_eq!(p.notes[0].ratchet, 1);
        set_field(&mut p, &[0], Field::Ratchet(9));
        assert_eq!(p.notes[0].ratchet, 8);
        set_field(&mut p, &[0], Field::Vel(0));
        assert_eq!(p.notes[0].vel, 1);
        assert!(set_field(&mut p, &[1], Field::Cond(Cond::Ratio(1, 4))));
        assert!(
            !set_field(&mut p, &[1], Field::Cond(Cond::Ratio(5, 4))),
            "invalid"
        );
        assert_eq!(p.notes[1].cond, Cond::Ratio(1, 4));
        set_field(&mut p, &[1, 2], Field::Accent(true));
        set_field(&mut p, &[2, 7], Field::Slide(true));
        assert!(p.notes[1].accent && p.notes[2].accent && !p.notes[0].accent);
        assert!(
            p.notes[2].slide && !p.notes[1].slide,
            "out of range skipped"
        );
        assert_eq!(p.notes.len(), 3);
        assert_eq!(p.notes[0].start, 0, "positions stay");
    }

    #[test]
    fn common_values_and_labels() {
        let mut p = pat(&[(0, 4, 60), (4, 4, 62)]);
        assert_eq!(common(&p.notes, |n| n.chance), Some(100));
        p.notes[1].chance = 40;
        assert_eq!(common(&p.notes, |n| n.chance), None, "mixed");
        assert_eq!(common(&[], |n: &Note| n.chance), None);
        let all = conds();
        assert_eq!(all.len(), 1 + 35 + 6);
        assert!(all.iter().all(|c| c.is_valid()));
        assert_eq!(cond_label(all[1]), "1:2");
        assert_eq!(cond_label(Cond::NotFirst), "Not first");
        let mut n = Note::new(0, 4, 60, 100);
        assert_eq!(badge_text(&n), "");
        for (c, t) in [
            (Cond::Ratio(1, 4), "1:4"),
            (Cond::First, "1st"),
            (Cond::NotFirst, "!1st"),
            (Cond::Fill, "F"),
            (Cond::NotFill, "!F"),
            (Cond::Pre, "P"),
            (Cond::NotPre, "!P"),
        ] {
            n.cond = c;
            assert_eq!(badge_text(&n), t);
        }
        n.cond = Cond::Always;
        n.chance = 50;
        assert_eq!(badge_text(&n), "50%");
        n.ratchet = 3;
        n.cond = Cond::Fill;
        assert_eq!(badge_text(&n), "F 50% ×3");
    }

    #[test]
    fn transpose_in_key_moves_each_note_one_degree() {
        // E -> F (a semitone), F -> G (a tone), C# (off-scale) -> D.
        let mut p = pat(&[(0, 4, 64), (4, 4, 65), (8, 4, 61)]);
        let idx = transpose_in_key(&mut p, &[0, 1, 2], true, c_major());
        assert_eq!(idx, vec![0, 1, 2]);
        assert_eq!(spans(&p), vec![(0, 4, 65), (4, 4, 67), (8, 4, 62)]);
        transpose_in_key(&mut p, &[0, 1], false, c_major());
        assert_eq!(spans(&p), vec![(0, 4, 64), (4, 4, 65), (8, 4, 62)]);
        // An off-scale note goes to the nearest in-scale pitch below.
        let mut p = pat(&[(0, 4, 61)]);
        transpose_in_key(&mut p, &[0], false, c_major());
        assert_eq!(spans(&p), vec![(0, 4, 60)]);
    }

    #[test]
    fn transpose_in_key_holds_the_group_at_the_ends() {
        // 127 (G) has no in-scale pitch above: nobody moves.
        let mut p = pat(&[(0, 4, 127), (4, 4, 60)]);
        transpose_in_key(&mut p, &[0, 1], true, c_major());
        assert_eq!(spans(&p), vec![(0, 4, 127), (4, 4, 60)]);
        let mut p = pat(&[(0, 4, 0), (4, 4, 60)]);
        transpose_in_key(&mut p, &[0, 1], false, c_major());
        assert_eq!(spans(&p), vec![(0, 4, 0), (4, 4, 60)]);
    }

    #[test]
    fn snap_pitches_puts_each_note_on_the_scale() {
        let mut p = pat(&[(0, 4, 61), (4, 4, 66), (8, 4, 60)]);
        snap_pitches(&mut p, &[0, 1], c_major());
        assert_eq!(spans(&p), vec![(0, 4, 62), (4, 4, 67), (8, 4, 60)]);
    }

    #[test]
    fn selections_resolve_by_value() {
        let p = pat(&[(0, 4, 60), (4, 4, 62), (0, 4, 60)]);
        let sel = vec![Note::new(0, 4, 60, 100), Note::new(4, 4, 62, 100)];
        assert_eq!(resolve(&p, &sel), vec![0, 1]);
        let twice = vec![Note::new(0, 4, 60, 100); 2];
        assert_eq!(resolve(&p, &twice), vec![0, 2], "duplicates claim one each");
        assert_eq!(resolve(&p, &[Note::new(9, 1, 1, 1)]), Vec::<usize>::new());
    }

    #[test]
    fn box_and_range_selection() {
        let p = pat(&[(0, 4, 60), (8, 4, 64), (8, 4, 50), (70, 4, 60)]);
        assert_eq!(in_box(&p, 9, 2, 66, 58), vec![0, 1], "corners in any order");
        assert_eq!(in_range(&p, 4, 12), vec![1, 2]);
        assert_eq!(
            in_box(&p, 0, 63, 0, 127).len(),
            3,
            "a note past the end isn't boxed"
        );
    }

    #[test]
    fn a_group_moves_as_far_as_its_edges_allow() {
        let mut p = pat(&[(4, 4, 60), (12, 4, 126)]);
        shift(&mut p, &[0, 1], -8, 5, LaneKind::Poly);
        assert_eq!(
            spans(&p),
            vec![(0, 4, 61), (8, 4, 127)],
            "clamped to tick 0 and pitch 127"
        );
        let mut d = pat(&[(0, 4, 6)]);
        shift(&mut d, &[0], 0, 3, LaneKind::Drums);
        assert_eq!(d.notes[0].pitch, 7, "drum rows stop at the open hat");
        let mut pads = pat(&[(0, 4, 13)]);
        shift(&mut pads, &[0], 0, 5, LaneKind::Pads);
        assert_eq!(pads.notes[0].pitch, 15, "pad rows stop at pad 16");
    }

    #[test]
    fn growing_to_fit_lengthens_but_never_shrinks_and_caps_at_the_longest() {
        let mut p = pat(&[(56, 20, 60), (0, 4, 62)]);
        grow_to_fit(&mut p, &[0]);
        assert_eq!(p.length, 19, "ends at tick 76: 19 steps");
        let mut short = pat(&[(0, 4, 60)]);
        grow_to_fit(&mut short, &[0]);
        assert_eq!(short.length, 16, "never shrinks");
        let mut long = pat(&[(250, 40, 60)]);
        grow_to_fit(&mut long, &[0]);
        assert_eq!((long.length, spans(&long)), (64, vec![(250, 6, 60)]));
    }

    #[test]
    fn resize_delete_copy() {
        let mut p = pat(&[(0, 4, 60), (8, 2, 62), (20, 4, 64)]);
        resize(&mut p, &[0, 1], -3);
        assert_eq!(spans(&p)[..2], [(0, 1, 60), (8, 1, 62)]);
        let c = copy(&p, &[1, 2], 8);
        assert_eq!(
            c.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(),
            [(0, 62), (12, 64)]
        );
        delete(&mut p, &[0, 2]);
        assert_eq!(spans(&p), vec![(8, 1, 62)]);
    }

    #[test]
    fn paste_places_at_the_cursor_and_respects_the_cap() {
        let mut p = pat(&[]);
        let clip = vec![Note::new(0, 4, 60, 100), Note::new(4, 4, 62, 100)];
        let idx = paste(&mut p, &clip, 16, LaneKind::Poly).unwrap();
        assert_eq!(
            values(&p, &idx).iter().map(|n| n.start).collect::<Vec<_>>(),
            [16, 20]
        );
        let mut full = pat(&[]);
        full.notes = vec![Note::new(0, 1, 1, 1); MAX_NOTES - 1];
        assert_eq!(
            paste(&mut full, &clip, 0, LaneKind::Poly),
            Err(PasteError::TooMany)
        );
        assert_eq!(full.notes.len(), MAX_NOTES - 1, "nothing changed");
        let mut kit = pat(&[]);
        assert_eq!(
            paste(&mut kit, &clip, 0, LaneKind::Drums),
            Err(PasteError::WrongKind)
        );
        let mut pads = pat(&[]);
        let hits = vec![Note::new(0, 4, 15, 100), Note::new(4, 4, 20, 100)];
        assert_eq!(
            paste(&mut pads, &hits, 0, LaneKind::Pads),
            Err(PasteError::WrongKind),
            "pitch 20 has no pad"
        );
        assert!(
            paste(&mut pads, &hits[..1], 0, LaneKind::Pads).is_ok(),
            "pad 16 does"
        );
    }

    #[test]
    fn duplicate_goes_right_after_the_selection() {
        let mut p = pat(&[(0, 4, 60), (4, 3, 62)]);
        let idx = duplicate(&mut p, &[0, 1], 4, LaneKind::Poly).unwrap();
        // span 7 rounds up to 8
        assert_eq!(
            values(&p, &idx).iter().map(|n| n.start).collect::<Vec<_>>(),
            [8, 12]
        );
    }

    #[test]
    fn settling_an_acid_edit_keeps_the_touched_notes() {
        let mut p = pat(&[(0, 16, 40), (8, 4, 48)]);
        let idx = settle(&mut p, &[1], LaneKind::Acid);
        assert_eq!(spans(&p), vec![(0, 8, 40), (8, 4, 48)]);
        assert_eq!(p.notes[idx[0]].pitch, 48);
    }

    #[test]
    fn settling_a_poly_edit_merges_same_pitch_overlaps() {
        let mut p = pat(&[(0, 4, 60), (2, 4, 60), (2, 4, 64)]);
        let idx = settle(&mut p, &[1], LaneKind::Poly);
        assert_eq!(spans(&p), vec![(0, 6, 60), (2, 4, 64)]);
        assert_eq!(p.notes[idx[0]].start, 0);
    }

    #[test]
    fn settling_tracks_touched_indices_through_removals() {
        let mut p = pat(&[(0, 2, 40), (4, 4, 41), (6, 1, 42), (10, 2, 43)]);
        // touched 3 and 1: note 1 removes note 2 (starts inside it)
        let idx = settle(&mut p, &[3, 1], LaneKind::Acid);
        assert_eq!(idx.len(), 2);
        let ps: Vec<u8> = idx.iter().map(|&i| p.notes[i].pitch).collect();
        assert_eq!(ps, [41, 43]);
        assert_eq!(p.notes.len(), 3);
        let mut q = pat(&[(0, 4, 60), (9, 2, 62), (2, 4, 60), (3, 2, 62), (20, 1, 60)]);
        let idx = settle(&mut q, &[2, 3], LaneKind::Poly);
        let got: Vec<_> = idx
            .iter()
            .map(|&i| (q.notes[i].start, q.notes[i].len, q.notes[i].pitch))
            .collect();
        assert_eq!(got, [(0, 6, 60), (3, 2, 62)]);
        assert_eq!(q.notes.len(), 4);
    }

    #[test]
    fn resizing_a_legato_acid_run_keeps_every_note() {
        let mut p = pat(&[(0, 4, 40), (4, 4, 41), (8, 4, 42)]);
        resize(&mut p, &[0, 1, 2], 4);
        let idx = settle(&mut p, &[0, 1, 2], LaneKind::Acid);
        assert_eq!(spans(&p), vec![(0, 4, 40), (4, 4, 41), (8, 8, 42)]);
        assert_eq!(idx, [0, 1, 2], "every touched note stays selected");
        // Untouched notes are still cut and removed by touched ones.
        let mut q = pat(&[(0, 4, 40), (4, 2, 50), (8, 4, 41), (10, 2, 51)]);
        resize(&mut q, &[0, 2], 4);
        settle(&mut q, &[0, 2], LaneKind::Acid);
        assert_eq!(spans(&q), vec![(0, 8, 40), (8, 8, 41)]);
    }

    #[test]
    fn a_chord_pasted_into_an_acid_lane_keeps_one_note_per_start() {
        let mut p = pat(&[(0, 16, 36)]);
        let chord = vec![
            Note::new(0, 4, 60, 100),
            Note::new(0, 4, 64, 100),
            Note::new(4, 4, 67, 100),
        ];
        let idx = paste(&mut p, &chord, 8, LaneKind::Acid).unwrap();
        assert_eq!(spans(&p), vec![(0, 8, 36), (8, 4, 60), (12, 4, 67)]);
        let got: Vec<u8> = idx.iter().map(|&i| p.notes[i].pitch).collect();
        assert_eq!(got, [60, 67], "the first chord note wins its start");
    }

    #[test]
    fn a_range_skips_notes_past_the_pattern_end() {
        let p = pat(&[(0, 4, 60), (100, 4, 62)]);
        assert_eq!(in_range(&p, 0, 200), vec![0]);
    }

    #[test]
    fn settling_does_not_merge_touching_or_hidden_notes() {
        let mut p = pat(&[(0, 4, 60), (4, 4, 60)]);
        settle(&mut p, &[1], LaneKind::Poly);
        assert_eq!(spans(&p), vec![(0, 4, 60), (4, 4, 60)], "end == start");
        // 16 steps = 64 ticks; a note at 100 is past the end.
        let mut q = pat(&[(60, 50, 60), (100, 4, 60)]);
        settle(&mut q, &[0], LaneKind::Poly);
        assert_eq!(spans(&q), vec![(60, 50, 60), (100, 4, 60)], "hidden");
    }

    #[test]
    fn the_absorbing_note_keeps_its_own_fields() {
        let mut p = pat(&[(0, 4, 60), (2, 4, 60)]);
        p.notes[1].vel = 30;
        p.notes[1].accent = true;
        let idx = settle(&mut p, &[1], LaneKind::Poly);
        assert_eq!(p.notes.len(), 1);
        let n = p.notes[idx[0]];
        assert_eq!((n.start, n.len, n.vel, n.accent), (0, 6, 30, true));
    }

    #[test]
    fn a_group_stops_at_the_last_tick_and_an_outside_group_only_comes_back() {
        let mut p = pat(&[(250, 4, 60), (240, 4, 62)]);
        shift(&mut p, &[0, 1], 10, 0, LaneKind::Poly);
        assert_eq!(spans(&p), vec![(245, 4, 62), (255, 4, 60)], "held at 255");
        let mut q = pat(&[(10, 4, 60), (300, 4, 62)]);
        shift(&mut q, &[0, 1], 5, 0, LaneKind::Poly);
        assert_eq!(spans(&q), vec![(10, 4, 60), (300, 4, 62)], "not outward");
        shift(&mut q, &[0, 1], -5, 0, LaneKind::Poly);
        assert_eq!(spans(&q), vec![(5, 4, 60), (295, 4, 62)], "inward");
    }

    #[test]
    fn paste_drops_notes_past_the_last_tick_and_caps_only_the_kept() {
        let clip = vec![Note::new(0, 4, 60, 100), Note::new(10, 4, 62, 100)];
        let mut p = pat(&[]);
        let idx = paste(&mut p, &clip, 250, LaneKind::Poly).unwrap();
        assert_eq!(spans(&p), vec![(250, 4, 60)], "260 dropped");
        assert_eq!(idx, [0]);
        let mut edge = pat(&[]);
        paste(&mut edge, &clip, 246, LaneKind::Poly).unwrap();
        assert_eq!(edge.notes.len(), 1, "a start of exactly 256 is dropped");
        let mut full = pat(&[]);
        full.notes = vec![Note::new(0, 1, 1, 1); MAX_NOTES - 1];
        assert!(
            paste(&mut full, &clip, 250, LaneKind::Poly).is_ok(),
            "only the kept note counts"
        );
        assert_eq!(full.notes.len(), MAX_NOTES);
    }

    #[test]
    fn duplicate_without_a_snap_goes_straight_after() {
        for snap in [0, 1] {
            let mut p = pat(&[(0, 4, 60), (4, 3, 62)]);
            let idx = duplicate(&mut p, &[0, 1], snap, LaneKind::Poly).unwrap();
            assert_eq!(
                values(&p, &idx).iter().map(|n| n.start).collect::<Vec<_>>(),
                [7, 11],
                "snap {snap}"
            );
        }
    }
}
