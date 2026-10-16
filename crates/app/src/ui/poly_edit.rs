//! Pure editing of note patterns for the poly piano roll, plus its
//! geometry. Notes stay in insertion order (the engine scans them all),
//! so an index stays valid for the whole of a drag.

use acid_engine::project::{MAX_NOTES, Note, NotePattern, TICKS_PER_STEP};

/// Every MIDI pitch, top row = 127.
pub const ROWS: usize = 128;
const EDGE_PX: f32 = 5.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Snap {
    #[default]
    Step,
    Half,
    Tick,
}

impl Snap {
    pub const ALL: [Snap; 3] = [Snap::Step, Snap::Half, Snap::Tick];

    pub fn ticks(self) -> u16 {
        match self {
            Snap::Step => TICKS_PER_STEP,
            Snap::Half => TICKS_PER_STEP / 2,
            Snap::Tick => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Snap::Step => "1/16",
            Snap::Half => "1/32",
            Snap::Tick => "1/64",
        }
    }

    pub fn floor(self, tick: u16) -> u16 {
        tick - tick % self.ticks()
    }
}

/// A mouse drag in progress in the roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolyDrag {
    /// `grab` = ticks between the note start and where it was grabbed.
    Move {
        index: usize,
        grab: u16,
    },
    Resize {
        index: usize,
    },
    Velocity,
}

fn end(n: &Note) -> u16 {
    n.start.saturating_add(n.len)
}

/// The last-drawn (topmost) note at `pitch` covering `tick`.
pub fn note_at(p: &NotePattern, tick: u16, pitch: u8) -> Option<usize> {
    p.notes
        .iter()
        .rposition(|n| n.pitch == pitch && tick >= n.start && tick < end(n))
}

/// Adds a note, replacing notes of the same pitch it overlaps. Refused
/// (None) when it starts at or past the pattern end, or when the pattern
/// would exceed `MAX_NOTES`. Returns the new note's index.
pub fn add(p: &mut NotePattern, start: u16, len: u16, pitch: u8, vel: u8) -> Option<usize> {
    if start >= p.ticks() || pitch > 127 {
        return None;
    }
    let len = len.max(1);
    let stop = start.saturating_add(len);
    // Only visible notes are replaced: one hidden past a shortened pattern
    // end survives (lengthening the pattern brings it back; the engine
    // tolerates same-pitch overlaps).
    let visible = p.ticks();
    let overlaps =
        |n: &Note| n.pitch == pitch && n.start < visible && n.start < stop && start < end(n);
    let replaced = p.notes.iter().filter(|n| overlaps(n)).count();
    if p.notes.len() - replaced >= MAX_NOTES {
        return None;
    }
    p.notes.retain(|n| !overlaps(n));
    p.notes
        .push(Note::new(start, len, pitch, vel.clamp(1, 127)));
    Some(p.notes.len() - 1)
}

pub fn delete(p: &mut NotePattern, index: usize) -> bool {
    if index < p.notes.len() {
        p.notes.remove(index);
        true
    } else {
        false
    }
}

/// Moves a note, keeping its start inside the pattern and its pitch in MIDI range.
pub fn move_to(p: &mut NotePattern, index: usize, start: u16, pitch: u8) -> bool {
    let last = p.ticks() - 1;
    let Some(n) = p.notes.get_mut(index) else {
        return false;
    };
    let (start, pitch) = (start.min(last), pitch.min(127));
    if n.start == start && n.pitch == pitch {
        return false;
    }
    n.start = start;
    n.pitch = pitch;
    true
}

/// Run when a move or resize drag is dropped: absorbs every other visible
/// same-pitch note the dragged note overlaps into it (the union span; the
/// dragged note keeps its velocity). Returns the dragged note's new index.
/// Not run mid-drag, so an index stays valid for the whole drag.
pub fn resolve_overlaps(p: &mut NotePattern, index: usize) -> usize {
    let Some(&dragged) = p.notes.get(index) else {
        return index;
    };
    let visible = p.ticks();
    let hit = |n: &Note| {
        n.pitch == dragged.pitch
            && n.start < visible
            && n.start < end(&dragged)
            && dragged.start < end(n)
    };
    let (mut lo, mut hi) = (dragged.start, end(&dragged));
    let mut kept = Vec::with_capacity(p.notes.len());
    let mut at = 0;
    for (k, n) in p.notes.iter().enumerate() {
        if k == index {
            at = kept.len();
            kept.push(*n);
        } else if hit(n) {
            lo = lo.min(n.start);
            hi = hi.max(end(n));
        } else {
            kept.push(*n);
        }
    }
    kept[at].start = lo;
    kept[at].len = hi - lo;
    p.notes = kept;
    at
}

pub fn resize(p: &mut NotePattern, index: usize, len: u16) -> bool {
    let Some(n) = p.notes.get_mut(index) else {
        return false;
    };
    let len = len.max(1);
    if n.len == len {
        return false;
    }
    n.len = len;
    true
}

/// Sets the velocity of every note starting in `from..to`.
pub fn set_velocity_in(p: &mut NotePattern, from: u16, to: u16, vel: u8) -> bool {
    let vel = vel.clamp(1, 127);
    let mut changed = false;
    for n in p.notes.iter_mut().filter(|n| (from..to).contains(&n.start)) {
        changed |= n.vel != vel;
        n.vel = vel;
    }
    changed
}

/// Screen layout of the roll grid: one column per tick, one row per
/// pitch, pitch 127 on top. `left` is the grid's left edge (right of the
/// key strip).
#[derive(Clone, Copy, Debug)]
pub struct PolyGeom {
    pub left: f32,
    pub top: f32,
    pub tick_w: f32,
    pub row_h: f32,
    pub ticks: u16,
}

impl PolyGeom {
    pub fn pitch_at(&self, y: f32) -> Option<u8> {
        let r = ((y - self.top) / self.row_h).floor();
        (r >= 0.0 && (r as usize) < ROWS).then(|| (ROWS - 1 - r as usize) as u8)
    }

    pub fn cell_at(&self, x: f32, y: f32) -> Option<(u16, u8)> {
        let col = ((x - self.left) / self.tick_w).floor();
        if col < 0.0 || col as u16 >= self.ticks {
            return None;
        }
        Some((col as u16, self.pitch_at(y)?))
    }

    pub fn cell_at_clamped(&self, x: f32, y: f32) -> (u16, u8) {
        let col = ((x - self.left) / self.tick_w)
            .floor()
            .clamp(0.0, (self.ticks.max(1) - 1) as f32) as u16;
        let r = ((y - self.top) / self.row_h)
            .floor()
            .clamp(0.0, (ROWS - 1) as f32) as usize;
        (col, (ROWS - 1 - r) as u8)
    }

    pub fn row_top(&self, pitch: u8) -> f32 {
        self.top + (ROWS - 1 - pitch.min(127) as usize) as f32 * self.row_h
    }

    /// (x0, y0, x1, y1); notes running past the pattern end are drawn to it.
    pub fn note_rect(&self, n: &Note) -> (f32, f32, f32, f32) {
        let x0 = self.left + n.start as f32 * self.tick_w;
        let stop = end(n).min(self.ticks.max(n.start.saturating_add(1)));
        let x1 = self.left + stop as f32 * self.tick_w;
        let y0 = self.row_top(n.pitch);
        (x0, y0, x1, y0 + self.row_h)
    }

    pub fn near_right_edge(&self, n: &Note, x: f32) -> bool {
        let (_, _, x1, _) = self.note_rect(n);
        x >= x1 - EDGE_PX && x <= x1 + 1.0
    }
}

/// Keeps an acid pattern monophonic around note `keep` after an edit:
/// other notes starting inside it are deleted, a note sounding into its
/// start is cut there. Returns `keep`'s new index.
pub fn make_mono(p: &mut NotePattern, keep: usize) -> usize {
    let Some(&k) = p.notes.get(keep) else {
        return keep;
    };
    let k_end = end(&k);
    let mut kept = Vec::with_capacity(p.notes.len());
    let mut at = 0;
    for (i, n) in p.notes.iter().enumerate() {
        if i == keep {
            at = kept.len();
            kept.push(*n);
        } else if n.start >= k.start && n.start < k_end {
            continue;
        } else if n.start < k.start && end(n) > k.start {
            kept.push(Note {
                len: k.start - n.start,
                ..*n
            });
        } else {
            kept.push(*n);
        }
    }
    p.notes = kept;
    at
}

/// The note sounding at `tick` (the first stored).
pub fn note_covering(p: &NotePattern, tick: u16) -> Option<usize> {
    p.notes
        .iter()
        .position(|n| n.start <= tick && tick < end(n))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    Accent,
    Slide,
}

/// Flips accent or slide on the note sounding at `tick`.
pub fn toggle_flag(p: &mut NotePattern, tick: u16, flag: Flag) -> bool {
    let Some(i) = note_covering(p, tick) else {
        return false;
    };
    let n = &mut p.notes[i];
    match flag {
        Flag::Accent => n.accent = !n.accent,
        Flag::Slide => n.slide = !n.slide,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pat() -> NotePattern {
        NotePattern::default()
    }

    fn notes_pat(notes: &[(u16, u16, u8)]) -> NotePattern {
        NotePattern {
            notes: notes
                .iter()
                .map(|&(s, l, p)| Note::new(s, l, p, 100))
                .collect(),
            length: 16,
        }
    }

    #[test]
    fn make_mono_keeps_the_edited_note_and_cuts_around_it() {
        // 0..8 sounds into the kept note at 4; 6 and 8 start inside it;
        // 20 is clear of it.
        let mut p = notes_pat(&[(0, 8, 40), (4, 8, 50), (6, 2, 41), (8, 4, 42), (20, 4, 43)]);
        let kept = make_mono(&mut p, 1);
        assert_eq!(p.notes[kept], Note::new(4, 8, 50, 100));
        let mut starts: Vec<_> = p.notes.iter().map(|n| (n.start, n.len)).collect();
        starts.sort();
        assert_eq!(starts, vec![(0, 4), (4, 8), (20, 4)]);
    }

    #[test]
    fn make_mono_drops_a_note_with_the_same_start() {
        let mut p = notes_pat(&[(4, 4, 40), (4, 2, 52)]);
        let kept = make_mono(&mut p, 1);
        assert_eq!(p.notes.len(), 1);
        assert_eq!(p.notes[kept].pitch, 52);
    }

    #[test]
    fn flags_toggle_on_the_note_under_the_tick() {
        let mut p = notes_pat(&[(4, 8, 40)]);
        assert!(toggle_flag(&mut p, 9, Flag::Accent));
        assert!(p.notes[0].accent);
        assert!(toggle_flag(&mut p, 4, Flag::Slide));
        assert!(p.notes[0].slide);
        assert!(toggle_flag(&mut p, 11, Flag::Accent));
        assert!(!p.notes[0].accent);
        assert!(!toggle_flag(&mut p, 12, Flag::Slide), "no note at 12");
        assert_eq!(note_covering(&p, 3), None);
    }

    #[test]
    fn snap_floors_to_its_grid() {
        assert_eq!(Snap::Step.floor(7), 4);
        assert_eq!(Snap::Half.floor(7), 6);
        assert_eq!(Snap::Tick.floor(7), 7);
        assert_eq!(Snap::ALL.map(Snap::ticks), [4, 2, 1]);
        assert_eq!(Snap::default(), Snap::Step);
    }

    #[test]
    fn chords_stack_and_same_pitch_overlaps_are_replaced() {
        let mut p = pat();
        for pitch in [60, 64, 67] {
            assert!(add(&mut p, 0, 4, pitch, 100).is_some());
        }
        assert_eq!(p.notes.len(), 3);
        let i = add(&mut p, 2, 4, 64, 80).unwrap();
        assert_eq!(p.notes.len(), 3, "the old 64 was replaced");
        assert_eq!(p.notes[i], Note::new(2, 4, 64, 80));
        assert!(
            add(&mut p, 4, 4, 60, 100).is_some(),
            "touching, not overlapping"
        );
        assert_eq!(p.notes.len(), 4);
    }

    #[test]
    fn add_keeps_a_same_pitch_note_hidden_past_the_pattern_end() {
        let mut p = pat();
        add(&mut p, 20, 4, 60, 100).unwrap();
        p.length = 1; // 4 ticks: the note at tick 20 is hidden, not gone
        assert!(add(&mut p, 0, 40, 60, 100).is_some());
        assert_eq!(p.notes.len(), 2, "the hidden note survives");
        assert!(p.notes.iter().any(|n| n.start == 20));
    }

    #[test]
    fn note_rect_of_a_note_at_the_tick_limit_does_not_overflow() {
        let g = PolyGeom {
            left: 0.0,
            top: 0.0,
            tick_w: 1.0,
            row_h: 1.0,
            ticks: 64,
        };
        let n = Note::new(u16::MAX, 5, 60, 1);
        let _ = g.note_rect(&n);
    }

    #[test]
    fn resolving_overlaps_merges_same_pitch_notes_into_the_dragged_one() {
        let mut p = pat();
        add(&mut p, 0, 4, 60, 100);
        let i = add(&mut p, 8, 4, 60, 50).unwrap();
        add(&mut p, 2, 4, 62, 100);
        assert!(move_to(&mut p, i, 2, 60), "moved onto the first note");
        assert_eq!(p.notes.len(), 3, "move alone leaves the overlap");
        let at = resolve_overlaps(&mut p, i);
        assert_eq!(p.notes.len(), 2, "the other 60 was absorbed");
        let merged = p.notes[at];
        assert_eq!((merged.start, merged.len, merged.pitch), (0, 6, 60));
        assert_eq!(merged.vel, 50, "the dragged note keeps its velocity");
        assert!(p.notes.iter().any(|n| n.pitch == 62), "other pitches stay");
    }

    #[test]
    fn resolving_overlaps_ignores_touching_hidden_and_other_pitches() {
        let mut p = pat();
        add(&mut p, 0, 4, 60, 100);
        let i = add(&mut p, 4, 4, 60, 100).unwrap();
        add(&mut p, 20, 4, 60, 100);
        p.length = 1;
        let before = p.clone();
        resolve_overlaps(&mut p, i);
        assert_eq!(p, before);
    }

    #[test]
    fn add_refuses_past_the_end_and_over_the_cap() {
        let mut p = pat();
        let end_tick = p.ticks();
        assert_eq!(add(&mut p, end_tick, 1, 60, 100), None);
        for k in 0..MAX_NOTES {
            add(&mut p, (k % 64) as u16, 1, (k / 64) as u8, 100).unwrap();
        }
        assert_eq!(p.notes.len(), MAX_NOTES);
        assert_eq!(add(&mut p, 0, 1, 100, 100), None);
        assert!(
            add(&mut p, 0, 1, 0, 50).is_some(),
            "replacing at the cap is fine"
        );
        assert_eq!(p.notes.len(), MAX_NOTES);
    }

    #[test]
    fn velocity_and_length_are_clamped() {
        let mut p = pat();
        let i = add(&mut p, 0, 0, 60, 0).unwrap();
        assert_eq!((p.notes[i].len, p.notes[i].vel), (1, 1));
        assert!(!resize(&mut p, i, 0), "already length 1");
        assert!(resize(&mut p, i, 9));
        assert_eq!(p.notes[i].len, 9);
        assert!(!resize(&mut p, 5, 3), "bad index");
    }

    #[test]
    fn note_at_finds_the_topmost_note_under_the_pointer() {
        let mut p = pat();
        add(&mut p, 0, 8, 60, 100);
        add(&mut p, 4, 2, 62, 100);
        assert_eq!(note_at(&p, 7, 60), Some(0));
        assert_eq!(note_at(&p, 8, 60), None);
        assert_eq!(note_at(&p, 5, 62), Some(1));
        assert_eq!(note_at(&p, 5, 61), None);
    }

    #[test]
    fn move_and_delete_keep_other_indices() {
        let mut p = pat();
        add(&mut p, 0, 4, 60, 100);
        add(&mut p, 8, 4, 62, 100);
        assert!(move_to(&mut p, 0, 200, 130));
        assert_eq!((p.notes[0].start, p.notes[0].pitch), (p.ticks() - 1, 127));
        let last = p.ticks() - 1;
        assert!(!move_to(&mut p, 0, last, 127), "no change");
        assert!(delete(&mut p, 0));
        assert_eq!(p.notes[0].pitch, 62);
        assert!(!delete(&mut p, 3));
    }

    #[test]
    fn velocity_lane_sets_notes_starting_in_the_column() {
        let mut p = pat();
        add(&mut p, 4, 1, 60, 100);
        add(&mut p, 5, 1, 64, 100);
        add(&mut p, 8, 1, 67, 100);
        assert!(set_velocity_in(&mut p, 4, 8, 30));
        assert_eq!(
            p.notes.iter().map(|n| n.vel).collect::<Vec<_>>(),
            [30, 30, 100]
        );
        assert!(!set_velocity_in(&mut p, 4, 8, 30), "no change");
        set_velocity_in(&mut p, 8, 9, 0);
        assert_eq!(p.notes[2].vel, 1);
    }

    #[test]
    fn geometry_round_trips() {
        let g = PolyGeom {
            left: 30.0,
            top: 10.0,
            tick_w: 6.0,
            row_h: 8.0,
            ticks: 64,
        };
        assert_eq!(g.cell_at(30.0, 10.0), Some((0, 127)));
        assert_eq!(
            g.cell_at(30.0 + 6.0 * 5.5, 10.0 + 8.0 * 67.5),
            Some((5, 60))
        );
        assert_eq!(g.cell_at(29.0, 10.0), None, "key strip");
        assert_eq!(g.cell_at(30.0 + 6.0 * 64.0, 10.0), None, "past the end");
        assert_eq!(g.cell_at_clamped(0.0, 9999.0), (0, 0));
        assert_eq!(g.pitch_at(10.0 + 8.0 * 67.5), Some(60));
        let n = Note::new(4, 70, 60, 1);
        let (x0, y0, x1, y1) = g.note_rect(&n);
        assert_eq!((x0, y0, y1), (54.0, g.row_top(60), g.row_top(60) + 8.0));
        assert_eq!(x1, 30.0 + 64.0 * 6.0, "drawn up to the pattern end");
        assert!(g.near_right_edge(&n, x1 - 2.0));
        assert!(!g.near_right_edge(&n, x0 + 2.0));
    }
}
