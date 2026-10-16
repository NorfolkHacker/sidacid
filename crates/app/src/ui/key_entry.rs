//! Keyboard note entry in the dock: the cursor, the tracker key layout and
//! what each key does to the focused lane's pattern. Pure; the dock feeds
//! it key presses while it has keyboard focus.
//!
//! Back deletes the note covering the cursor; failing that it steps the
//! cursor back one grid cell and deletes the note covering or starting in
//! that cell, so Back right after typing undoes the last note.

use super::poly_edit::{Snap, add, make_mono, note_at};
use acid_engine::params::DRUM_INSTRUMENTS;
use acid_engine::project::NotePattern;
use eframe::egui::{Key, Modifiers};

const ENTRY_VEL: u8 = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub tick: u16,
    /// A MIDI pitch, or an instrument (0..8) on a drum lane.
    pub pitch: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneKind {
    Acid,
    Poly,
    Drums,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKey {
    /// Semitones above C of the cursor's octave (0..=24).
    Note(u8),
    Left,
    Right,
    Up,
    Down,
    Back,
}

pub fn entry_key(key: Key, mods: Modifiers) -> Option<EntryKey> {
    if mods.command || mods.ctrl || mods.alt {
        return None;
    }
    use EntryKey::*;
    Some(match key {
        Key::Z => Note(0),
        Key::S => Note(1),
        Key::X => Note(2),
        Key::D => Note(3),
        Key::C => Note(4),
        Key::V => Note(5),
        Key::G => Note(6),
        Key::B => Note(7),
        Key::H => Note(8),
        Key::N => Note(9),
        Key::J => Note(10),
        Key::M => Note(11),
        Key::Comma | Key::Q => Note(12),
        Key::Num2 => Note(13),
        Key::W => Note(14),
        Key::Num3 => Note(15),
        Key::E => Note(16),
        Key::R => Note(17),
        Key::Num5 => Note(18),
        Key::T => Note(19),
        Key::Num6 => Note(20),
        Key::Y => Note(21),
        Key::Num7 => Note(22),
        Key::U => Note(23),
        Key::I => Note(24),
        Key::ArrowLeft => Left,
        Key::ArrowRight => Right,
        Key::ArrowUp => Up,
        Key::ArrowDown => Down,
        Key::Backspace | Key::Delete => Back,
        _ => return None,
    })
}

/// The start tick of grid cell `cell + by`, wrapping over the pattern's cells.
fn cell_tick(cell: u16, by: i32, cells: u16) -> u16 {
    (cell as i32 + by).rem_euclid(cells.max(1) as i32) as u16
}

pub fn apply_entry(
    roll: &mut NotePattern,
    cursor: &mut Cursor,
    key: EntryKey,
    kind: LaneKind,
    snap: Snap,
) -> bool {
    let ticks = roll.ticks();
    let s = snap.ticks().max(1);
    let here = snap.floor(cursor.tick.min(ticks - 1));
    // The cursor moves in whole grid cells; the last one may be partial.
    let cells = ticks.div_ceil(s);
    let cell = here / s;
    let top = if kind == LaneKind::Drums {
        DRUM_INSTRUMENTS as u8 - 1
    } else {
        127
    };
    match key {
        EntryKey::Left => {
            cursor.tick = cell_tick(cell, -1, cells) * s;
            false
        }
        EntryKey::Right => {
            cursor.tick = cell_tick(cell, 1, cells) * s;
            false
        }
        EntryKey::Up => {
            cursor.pitch = (cursor.pitch.min(top) + 1).min(top);
            false
        }
        EntryKey::Down => {
            cursor.pitch = cursor.pitch.min(top).saturating_sub(1);
            false
        }
        EntryKey::Note(semis) => {
            let pitch = match kind {
                LaneKind::Drums => cursor.pitch.min(top),
                _ => (cursor.pitch / 12 * 12).saturating_add(semis).min(127),
            };
            let Some(i) = add(roll, here, s.min(ticks - here).max(1), pitch, ENTRY_VEL) else {
                return false;
            };
            if kind == LaneKind::Acid {
                make_mono(roll, i);
            }
            cursor.pitch = pitch;
            cursor.tick = cell_tick(cell, 1, cells) * s;
            true
        }
        EntryKey::Back => {
            let pitch = cursor.pitch.min(top);
            let mut hit = note_at(roll, cursor.tick.min(ticks - 1), pitch);
            if hit.is_none() {
                let ct = cell_tick(cell, -1, cells) * s;
                cursor.tick = ct;
                hit = note_at(roll, ct, pitch).or_else(|| {
                    roll.notes
                        .iter()
                        .rposition(|n| n.pitch == pitch && n.start >= ct && n.start < ct + s)
                });
            }
            match hit {
                Some(i) => {
                    roll.notes.remove(i);
                    true
                }
                None => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::project::{Note, NotePattern};
    use eframe::egui::{Key, Modifiers};

    fn empty() -> NotePattern {
        NotePattern::default() // 16 steps = 64 ticks
    }

    #[test]
    fn the_tracker_layout_maps_two_octaves() {
        assert_eq!(entry_key(Key::Z, Modifiers::NONE), Some(EntryKey::Note(0)));
        assert_eq!(entry_key(Key::M, Modifiers::NONE), Some(EntryKey::Note(11)));
        assert_eq!(entry_key(Key::Q, Modifiers::NONE), Some(EntryKey::Note(12)));
        assert_eq!(
            entry_key(Key::Comma, Modifiers::NONE),
            Some(EntryKey::Note(12))
        );
        assert_eq!(entry_key(Key::I, Modifiers::NONE), Some(EntryKey::Note(24)));
        assert_eq!(
            entry_key(Key::ArrowLeft, Modifiers::NONE),
            Some(EntryKey::Left)
        );
        assert_eq!(
            entry_key(Key::Backspace, Modifiers::NONE),
            Some(EntryKey::Back)
        );
        assert_eq!(
            entry_key(Key::Z, Modifiers::COMMAND),
            None,
            "Ctrl+Z is undo"
        );
        assert_eq!(entry_key(Key::Z, Modifiers::ALT), None);
        assert_eq!(entry_key(Key::Space, Modifiers::NONE), None, "Space plays");
    }

    #[test]
    fn a_note_key_writes_one_snap_and_advances() {
        let mut roll = empty();
        let mut c = Cursor { tick: 5, pitch: 50 };
        assert!(apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Note(7),
            LaneKind::Poly,
            Snap::Step
        ));
        // C of the cursor's octave (48) + 7, at the snapped tick 4.
        assert_eq!(roll.notes, vec![Note::new(4, 4, 55, 100)]);
        assert_eq!(c, Cursor { tick: 8, pitch: 55 });
    }

    #[test]
    fn the_cursor_wraps_at_the_pattern_end() {
        let mut roll = empty();
        let mut c = Cursor {
            tick: 60,
            pitch: 48,
        };
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Note(0),
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(c.tick, 0);
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Left,
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(c.tick, 60);
        apply_entry(&mut roll, &mut c, EntryKey::Up, LaneKind::Poly, Snap::Step);
        assert_eq!(c.pitch, 49);
    }

    #[test]
    fn acid_entry_stays_monophonic() {
        let mut roll = empty();
        roll.notes.push(Note::new(0, 16, 40, 100));
        let mut c = Cursor { tick: 8, pitch: 48 };
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Note(0),
            LaneKind::Acid,
            Snap::Step,
        );
        let mut spans: Vec<_> = roll.notes.iter().map(|n| (n.start, n.len)).collect();
        spans.sort();
        assert_eq!(spans, vec![(0, 8), (8, 4)], "the long note is cut");
    }

    #[test]
    fn drum_entry_writes_on_the_cursor_row() {
        let mut roll = empty();
        let mut c = Cursor { tick: 0, pitch: 6 };
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Note(9),
            LaneKind::Drums,
            Snap::Half,
        );
        assert_eq!(roll.notes, vec![Note::new(0, 2, 6, 100)]);
        assert_eq!(c, Cursor { tick: 2, pitch: 6 });
        apply_entry(&mut roll, &mut c, EntryKey::Up, LaneKind::Drums, Snap::Half);
        apply_entry(&mut roll, &mut c, EntryKey::Up, LaneKind::Drums, Snap::Half);
        assert_eq!(c.pitch, 7, "clamped to the open hat");
    }

    #[test]
    fn back_deletes_under_or_just_before_the_cursor() {
        let mut roll = empty();
        roll.notes.push(Note::new(4, 4, 60, 100));
        let mut c = Cursor { tick: 6, pitch: 60 };
        assert!(apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Back,
            LaneKind::Poly,
            Snap::Step
        ));
        assert!(roll.notes.is_empty());
        roll.notes.push(Note::new(4, 2, 60, 100));
        c.tick = 8; // after the note ends, one cell on
        assert!(apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Back,
            LaneKind::Poly,
            Snap::Step
        ));
        assert!(roll.notes.is_empty());
        assert!(!apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Back,
            LaneKind::Poly,
            Snap::Step
        ));
    }

    #[test]
    fn back_after_typing_undoes_the_notes() {
        let mut roll = empty();
        let mut c = Cursor { tick: 4, pitch: 48 };
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Note(0),
            LaneKind::Poly,
            Snap::Step,
        );
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Note(0),
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(c.tick, 12);
        assert!(apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Back,
            LaneKind::Poly,
            Snap::Step
        ));
        assert!(apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Back,
            LaneKind::Poly,
            Snap::Step
        ));
        assert!(roll.notes.is_empty());
        assert_eq!(c.tick, 4);
    }

    #[test]
    fn back_on_an_empty_row_steps_back() {
        let mut roll = empty();
        let mut c = Cursor { tick: 8, pitch: 60 };
        assert!(!apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Back,
            LaneKind::Poly,
            Snap::Step
        ));
        assert_eq!(c.tick, 4);
    }

    #[test]
    fn delete_is_back() {
        assert_eq!(
            entry_key(Key::Delete, Modifiers::NONE),
            Some(EntryKey::Back)
        );
    }

    #[test]
    fn right_and_down_move_the_cursor() {
        let mut roll = empty();
        let mut c = Cursor { tick: 0, pitch: 1 };
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Right,
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(c.tick, 4);
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Down,
            LaneKind::Poly,
            Snap::Step,
        );
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Down,
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(c.pitch, 0);
    }

    #[test]
    fn a_high_note_clamps_to_127() {
        let mut roll = empty();
        let mut c = Cursor {
            tick: 0,
            pitch: 126,
        };
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Note(24),
            LaneKind::Poly,
            Snap::Step,
        );
        assert_eq!(c.pitch, 127);
        assert_eq!(roll.notes[0].pitch, 127);
    }

    #[test]
    fn a_partial_last_cell_does_not_trap_the_cursor() {
        // 12 ticks with eighth snap (8): cells at 0 and 8.
        let mut roll = NotePattern {
            length: 3,
            ..Default::default()
        };
        assert_eq!(roll.ticks(), 12);
        let mut c = Cursor { tick: 0, pitch: 48 };
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Left,
            LaneKind::Poly,
            Snap::Eighth,
        );
        assert_eq!(c.tick, 8);
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Left,
            LaneKind::Poly,
            Snap::Eighth,
        );
        assert_eq!(c.tick, 0);
        c.tick = 8;
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Right,
            LaneKind::Poly,
            Snap::Eighth,
        );
        assert_eq!(c.tick, 0);
        c.tick = 8;
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Note(0),
            LaneKind::Poly,
            Snap::Eighth,
        );
        assert_eq!(roll.notes, vec![Note::new(8, 4, 48, 100)]);
        assert_eq!(c.tick, 0);
    }

    #[test]
    fn a_snap_longer_than_the_pattern_writes_the_whole_pattern() {
        let mut roll = NotePattern {
            length: 1,
            ..Default::default()
        };
        let mut c = Cursor { tick: 0, pitch: 48 };
        apply_entry(
            &mut roll,
            &mut c,
            EntryKey::Note(0),
            LaneKind::Poly,
            Snap::Quarter,
        );
        assert_eq!(roll.notes, vec![Note::new(0, 4, 48, 100)]);
        assert_eq!(c.tick, 0);
    }
}
