//! Pure editing of note patterns for the lanes' piano rolls (selection
//! edits are in `note_ops`). Notes stay in insertion order (the engine
//! scans them all), so an index stays valid for the whole of a drag.

use acid_engine::project::{MAX_NOTES, Note, NotePattern, TICKS_PER_STEP};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Snap {
    Quarter,
    Eighth,
    #[default]
    Step,
    Half,
    Tick,
}

impl Snap {
    pub const ALL: [Snap; 5] = [
        Snap::Quarter,
        Snap::Eighth,
        Snap::Step,
        Snap::Half,
        Snap::Tick,
    ];

    pub fn ticks(self) -> u16 {
        match self {
            Snap::Quarter => TICKS_PER_STEP * 4,
            Snap::Eighth => TICKS_PER_STEP * 2,
            Snap::Step => TICKS_PER_STEP,
            Snap::Half => TICKS_PER_STEP / 2,
            Snap::Tick => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Snap::Quarter => "1/4",
            Snap::Eighth => "1/8",
            Snap::Step => "1/16",
            Snap::Half => "1/32",
            Snap::Tick => "1/64",
        }
    }

    pub fn floor(self, tick: u16) -> u16 {
        tick - tick % self.ticks()
    }
}

/// A mouse drag in progress in a lane. Move and resize recompute the
/// pattern from `origin` (pattern `slot` when the drag started, after any
/// copies) every frame, so deltas never accumulate; they are dropped if
/// the machine selects another pattern meanwhile.
#[derive(Clone, Debug, PartialEq)]
pub enum LaneDrag {
    /// The notes at `idx` follow the pointer from `grab` (tick, row pitch).
    Move {
        origin: Box<NotePattern>,
        slot: u8,
        idx: Vec<usize>,
        grab: (u16, u8),
    },
    /// The notes at `idx` stretch with the grabbed note's end (`grab`).
    Resize {
        origin: Box<NotePattern>,
        slot: u8,
        idx: Vec<usize>,
        grab: u16,
    },
    /// A selection box from `from` (tick, pitch); `add` keeps the old
    /// selection.
    Marquee { from: (u16, u8), add: bool },
    /// A drag in the velocity strip.
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

/// The drum accent strip: flips accent on every hit starting in
/// `from..to`, clearing them all if they are all accented, else accenting
/// them all. False when no hit starts there.
pub fn toggle_accent_in(p: &mut NotePattern, from: u16, to: u16) -> bool {
    let inside = |n: &Note| (from..to).contains(&n.start);
    let mut hits = p.notes.iter().filter(|n| inside(n)).peekable();
    if hits.peek().is_none() {
        return false;
    }
    let accent = !hits.all(|n| n.accent);
    for n in p.notes.iter_mut().filter(|n| inside(n)) {
        n.accent = accent;
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
    fn accent_in_a_cell_accents_all_then_clears_all() {
        // Two hits start in 4..8; the long one at 0 covers it, not starting there.
        let mut p = notes_pat(&[(4, 4, 0), (6, 1, 2), (0, 12, 1)]);
        p.notes[1].accent = true;
        assert!(toggle_accent_in(&mut p, 4, 8), "mixed: all accented");
        assert!(p.notes[0].accent && p.notes[1].accent);
        assert!(!p.notes[2].accent, "a hit covering the cell is untouched");
        assert!(toggle_accent_in(&mut p, 4, 8), "all accented: cleared");
        assert!(!p.notes[0].accent && !p.notes[1].accent);
        assert!(!p.notes[2].accent);
        assert!(!toggle_accent_in(&mut p, 8, 12), "no hit starts there");
        assert!(!p.notes[2].accent);
    }

    #[test]
    fn snap_floors_to_its_grid() {
        assert_eq!(Snap::Step.floor(7), 4);
        assert_eq!(Snap::Half.floor(7), 6);
        assert_eq!(Snap::Tick.floor(7), 7);
        assert_eq!(Snap::ALL.map(Snap::ticks), [16, 8, 4, 2, 1]);
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
}
