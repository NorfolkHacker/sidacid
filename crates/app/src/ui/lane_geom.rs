//! Pure lane geometry for the dock: ticks to x with the dock's shared
//! horizontal scroll, rows to y with the lane's own pitch scroll. Melodic
//! lanes have one row per MIDI pitch (127 on top); drum lanes one row per
//! instrument (the kick at the bottom). A folded melodic lane has one row
//! per pitch in its `FoldRows` table, highest on top.

use acid_engine::params::DRUM_INSTRUMENTS;
use acid_engine::project::{Note, NotePattern, TICKS_PER_STEP};
use acid_engine::scale::Key;
use eframe::egui::{Rect, pos2};

/// Width of one 1/16 step at zoom 1.
pub const STEP_W: f32 = 22.0;
pub const ZOOMS: [f32; 5] = [0.5, 0.75, 1.0, 1.5, 2.0];
const EDGE_PX: f32 = 5.0;

pub fn tick_w(zoom: f32) -> f32 {
    STEP_W * zoom / TICKS_PER_STEP as f32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rows {
    Pitches,
    Drums,
    /// A folded melodic lane: only the pitches in the table.
    Folded(FoldRows),
}

impl Rows {
    pub fn count(&self) -> usize {
        match self {
            Rows::Pitches => 128,
            Rows::Drums => DRUM_INSTRUMENTS,
            Rows::Folded(f) => f.len(),
        }
    }

    pub fn row_of(&self, pitch: u8) -> Option<usize> {
        if let Rows::Folded(f) = self {
            return f.row_of(pitch);
        }
        let n = self.count();
        ((pitch as usize) < n).then(|| n - 1 - pitch as usize)
    }

    pub fn pitch_of_row(&self, row: usize) -> Option<u8> {
        if let Rows::Folded(f) = self {
            return f.pitches().get(row).copied();
        }
        let n = self.count();
        (row < n).then(|| (n - 1 - row) as u8)
    }

    /// The row `pitch` is on, or for a pitch with no row (folded away) the
    /// row it would sit on: the first one below it. Clamped to the rows.
    pub fn row_near(&self, pitch: u8) -> usize {
        let n = self.count();
        match self {
            Rows::Folded(f) => f.pitches().iter().take_while(|&&q| q > pitch).count(),
            _ => n.saturating_sub(1 + pitch as usize),
        }
        .min(n.saturating_sub(1))
    }
}

/// The rows of a folded lane: pitches high to low, at most all 128.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldRows {
    pitches: [u8; 128],
    count: u8,
}

impl FoldRows {
    /// The pitches on `key`'s scale plus every pitch holding a note in
    /// `pattern`, so every note has a row. Chromatic keeps all 128.
    pub fn new(key: Key, pattern: &NotePattern) -> Self {
        let mut held = [false; 128];
        for n in &pattern.notes {
            if let Some(h) = held.get_mut(n.pitch as usize) {
                *h = true;
            }
        }
        let mut rows = FoldRows {
            pitches: [0; 128],
            count: 0,
        };
        for pitch in (0..128u8).rev() {
            if key.contains(pitch) || held[pitch as usize] {
                rows.pitches[rows.count as usize] = pitch;
                rows.count += 1;
            }
        }
        rows
    }

    pub fn len(&self) -> usize {
        self.count as usize
    }

    /// The shown pitches, highest first.
    pub fn pitches(&self) -> &[u8] {
        &self.pitches[..self.len()]
    }

    pub fn row_of(&self, pitch: u8) -> Option<usize> {
        // High to low: search on the reversed order.
        self.pitches().binary_search_by(|q| pitch.cmp(q)).ok()
    }

    /// The next shown pitch above (`up`) or below `pitch`; past the last
    /// row it stays on that row, and with no rows `pitch` is kept.
    pub fn step(&self, pitch: u8, up: bool) -> u8 {
        let p = self.pitches();
        let (Some(&top), Some(&bottom)) = (p.first(), p.last()) else {
            return pitch;
        };
        if up {
            p.iter()
                .rev()
                .find(|&&q| q > pitch)
                .copied()
                .unwrap_or(if pitch < top { top } else { pitch })
        } else {
            p.iter()
                .find(|&&q| q < pitch)
                .copied()
                .unwrap_or(if pitch > bottom { bottom } else { pitch })
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LaneGeom {
    pub left: f32,
    pub top: f32,
    pub tick_w: f32,
    pub row_h: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub ticks: u16,
    pub rows: Rows,
}

impl LaneGeom {
    pub fn row_count(&self) -> usize {
        self.rows.count()
    }

    pub fn row_of(&self, pitch: u8) -> Option<usize> {
        self.rows.row_of(pitch)
    }

    pub fn pitch_of_row(&self, row: usize) -> Option<u8> {
        self.rows.pitch_of_row(row)
    }

    pub fn x_of(&self, tick: u16) -> f32 {
        self.left + tick as f32 * self.tick_w - self.scroll_x
    }

    pub fn tick_at(&self, x: f32) -> Option<u16> {
        if x < self.left {
            return None;
        }
        let t = ((x - self.left + self.scroll_x) / self.tick_w).floor();
        (t >= 0.0 && (t as u32) < self.ticks as u32).then_some(t as u16)
    }

    pub fn tick_at_clamped(&self, x: f32) -> u16 {
        let t = ((x.max(self.left) - self.left + self.scroll_x) / self.tick_w).floor();
        (t.max(0.0) as u32).min(self.ticks.saturating_sub(1) as u32) as u16
    }

    pub fn row_top(&self, row: usize) -> f32 {
        self.top + row as f32 * self.row_h - self.scroll_y
    }

    pub fn pitch_at(&self, y: f32) -> Option<u8> {
        if y < self.top {
            return None;
        }
        let r = ((y - self.top + self.scroll_y) / self.row_h).floor();
        if r < 0.0 {
            return None;
        }
        self.pitch_of_row(r as usize)
    }

    pub fn pitch_at_clamped(&self, y: f32) -> u8 {
        let r = ((y.max(self.top) - self.top + self.scroll_y) / self.row_h).floor();
        let r = (r.max(0.0) as usize).min(self.row_count().saturating_sub(1));
        self.pitch_of_row(r).unwrap_or(0)
    }

    pub fn note_rect(&self, n: &Note) -> Rect {
        // A pitch with no row (folded away under a drag) draws on the row
        // it would sit on.
        let row = self
            .row_of(n.pitch)
            .unwrap_or_else(|| self.rows.row_near(n.pitch));
        let y0 = self.row_top(row);
        let end = n.start.saturating_add(n.len).min(self.ticks);
        Rect::from_min_max(
            pos2(self.x_of(n.start), y0),
            pos2(self.x_of(end), y0 + self.row_h),
        )
    }

    pub fn near_right_edge(&self, n: &Note, x: f32) -> bool {
        (self.note_rect(n).right() - x).abs() <= EDGE_PX
    }
}

/// The horizontal scroll that keeps a playhead at content x `playhead_x`
/// in a view `view_w` wide: unchanged while it sits left of the last
/// quarter of the view, else moved so it sits a quarter in.
pub fn follow_scroll(scroll_x: f32, view_w: f32, playhead_x: f32) -> f32 {
    let margin = view_w / 4.0;
    if playhead_x >= scroll_x && playhead_x <= scroll_x + view_w - margin {
        scroll_x
    } else {
        (playhead_x - margin).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::poly_edit::Snap;
    use acid_engine::project::Note;

    fn melodic() -> LaneGeom {
        LaneGeom {
            left: 100.0,
            top: 50.0,
            tick_w: 5.5,
            row_h: 9.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            ticks: 64,
            rows: Rows::Pitches,
        }
    }

    #[test]
    fn snaps_cover_quarter_to_sixty_fourth() {
        let ticks: Vec<u16> = Snap::ALL.iter().map(|s| s.ticks()).collect();
        assert_eq!(ticks, [16, 8, 4, 2, 1]);
        let labels: Vec<&str> = Snap::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, ["1/4", "1/8", "1/16", "1/32", "1/64"]);
        assert_eq!(Snap::Quarter.floor(37), 32);
        assert_eq!(Snap::default(), Snap::Step);
    }

    #[test]
    fn melodic_rows_put_pitch_127_on_top() {
        let g = melodic();
        assert_eq!(g.row_count(), 128);
        assert_eq!(g.row_of(127), Some(0));
        assert_eq!(g.row_of(0), Some(127));
        assert_eq!(g.pitch_of_row(127), Some(0));
        assert_eq!(g.pitch_at(50.0 + 9.0 * 67.5), Some(60));
        assert_eq!(g.pitch_at(49.0), None);
    }

    #[test]
    fn drum_rows_put_the_kick_at_the_bottom() {
        let g = LaneGeom {
            rows: Rows::Drums,
            ..melodic()
        };
        assert_eq!(g.row_count(), 8);
        assert_eq!(g.row_of(0), Some(7));
        assert_eq!(g.row_of(7), Some(0));
        assert_eq!(g.row_of(8), None, "not an instrument");
        assert_eq!(g.pitch_at(50.0 + 9.0 * 7.5), Some(0));
    }

    fn c_major_with(notes: &[u8]) -> FoldRows {
        use acid_engine::scale::Scale;
        let key = Key {
            root: 0,
            scale: Scale::Major,
            snap: false,
        };
        let p = NotePattern {
            notes: notes.iter().map(|&q| Note::new(0, 4, q, 100)).collect(),
            ..NotePattern::default()
        };
        FoldRows::new(key, &p)
    }

    #[test]
    fn fold_rows_keep_the_scale_and_every_held_pitch() {
        let f = c_major_with(&[61]);
        // 75 pitches of C major in 0..128, plus the held C#4.
        assert_eq!(f.len(), 76);
        assert_eq!(f.pitches()[0], 127, "G9 on top");
        assert_eq!(*f.pitches().last().unwrap(), 0);
        assert!(f.pitches().windows(2).all(|w| w[0] > w[1]), "high to low");
        assert!(f.row_of(61).is_some(), "the off-scale note keeps its row");
        assert_eq!(f.row_of(63), None, "D# is folded away");
        let all = FoldRows::new(Key::default(), &NotePattern::default());
        assert_eq!(all.len(), 128, "Chromatic folds nothing");
    }

    #[test]
    fn folded_rows_round_trip_with_pitches() {
        let g = LaneGeom {
            rows: Rows::Folded(c_major_with(&[61])),
            ..melodic()
        };
        assert_eq!(g.row_count(), 76);
        for row in 0..g.row_count() {
            let p = g.pitch_of_row(row).unwrap();
            assert_eq!(g.row_of(p), Some(row));
            let y = g.row_top(row) + 4.0;
            assert_eq!(g.pitch_at(y), Some(p));
        }
        assert_eq!(g.pitch_of_row(76), None);
        assert_eq!(g.pitch_at_clamped(9999.0), 0);
        assert_eq!(g.pitch_at_clamped(0.0), 127);
        let r = g.note_rect(&Note::new(0, 4, 61, 100));
        assert_eq!(r.top(), g.row_top(g.row_of(61).unwrap()));
        // C5 (72), B4, A4, G4, F4, E4, D4, C#4, C4: C#4 is row 7 below C5.
        assert_eq!(g.row_of(61), Some(g.row_of(72).unwrap() + 7));
    }

    #[test]
    fn fold_steps_between_shown_rows() {
        let f = c_major_with(&[61]);
        assert_eq!(f.step(60, true), 61, "onto the held C#");
        assert_eq!(f.step(61, true), 62);
        assert_eq!(f.step(64, true), 65, "E to F");
        assert_eq!(f.step(63, true), 64, "from a hidden pitch");
        assert_eq!(f.step(63, false), 62);
        assert_eq!(f.step(127, true), 127, "stays on the top row");
        assert_eq!(f.step(0, false), 0);
        // C# major: hidden pitches beyond
        // the ends stay put rather than jumping to the far row.
        let cs = FoldRows::new(
            Key {
                root: 1,
                scale: acid_engine::scale::Scale::Major,
                snap: false,
            },
            &NotePattern::default(),
        );
        let top = cs.pitches()[0];
        assert!(top < 127, "127 is off C# major");
        assert_eq!(cs.step(127, true), 127, "nothing above: not back down");
        assert_eq!(cs.step(127, false), top);
        let dm = FoldRows::new(
            Key {
                root: 2,
                scale: acid_engine::scale::Scale::Major,
                snap: false,
            },
            &NotePattern::default(),
        );
        assert_eq!(dm.pitches().last(), Some(&1), "C#, not C0");
        assert_eq!(dm.step(0, false), 0, "nothing below: not back up");
        assert_eq!(dm.step(0, true), 1);
        let rows = Rows::Folded(f);
        assert_eq!(rows.row_near(63), rows.row_near(62), "D#: the row below");
        assert_eq!(Rows::Pitches.row_near(60), 67);
        assert_eq!(Rows::Drums.row_near(9), 0, "clamped");
    }

    #[test]
    fn ticks_follow_the_shared_scroll() {
        let mut g = melodic();
        assert_eq!(g.x_of(4), 100.0 + 22.0);
        assert_eq!(g.tick_at(100.0 + 22.0), Some(4));
        assert_eq!(g.tick_at(99.0), None);
        assert_eq!(g.tick_at(100.0 + 5.5 * 64.0), None, "past the end");
        g.scroll_x = 22.0;
        assert_eq!(g.x_of(4), 100.0);
        assert_eq!(g.tick_at(100.0), Some(4));
        assert_eq!(g.tick_at_clamped(10_000.0), 63);
        assert_eq!(g.tick_at_clamped(0.0), 4, "the first visible tick");
    }

    #[test]
    fn pitch_scroll_moves_rows_up() {
        let mut g = melodic();
        g.scroll_y = 9.0 * 60.0;
        assert_eq!(g.row_top(60), 50.0);
        assert_eq!(g.pitch_at(50.0 + 4.0), Some(67));
    }

    #[test]
    fn note_rects_and_edges() {
        let g = melodic();
        let n = Note::new(4, 8, 127, 100);
        let r = g.note_rect(&n);
        assert_eq!((r.left(), r.right()), (122.0, 166.0));
        assert_eq!((r.top(), r.bottom()), (50.0, 59.0));
        assert!(g.near_right_edge(&n, 163.0));
        assert!(!g.near_right_edge(&n, 140.0));
    }

    /// Ported from the poly roll's `PolyGeom`.
    #[test]
    fn geometry_round_trips() {
        let g = LaneGeom {
            left: 30.0,
            top: 10.0,
            tick_w: 6.0,
            row_h: 8.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            ticks: 64,
            rows: Rows::Pitches,
        };
        assert_eq!((g.tick_at(30.0), g.pitch_at(10.0)), (Some(0), Some(127)));
        assert_eq!(g.tick_at(30.0 + 6.0 * 5.5), Some(5));
        assert_eq!(g.pitch_at(10.0 + 8.0 * 67.5), Some(60));
        assert_eq!(g.tick_at(29.0), None, "key strip");
        assert_eq!(g.tick_at(30.0 + 6.0 * 64.0), None, "past the end");
        assert_eq!((g.tick_at_clamped(0.0), g.pitch_at_clamped(9999.0)), (0, 0));
        let n = Note::new(4, 70, 60, 1);
        let r = g.note_rect(&n);
        let top = g.row_top(g.row_of(60).unwrap());
        assert_eq!((r.left(), r.top(), r.bottom()), (54.0, top, top + 8.0));
        assert_eq!(r.right(), 30.0 + 64.0 * 6.0, "drawn up to the pattern end");
        assert!(g.near_right_edge(&n, r.right() - 2.0));
        assert!(!g.near_right_edge(&n, r.left() + 2.0));
    }

    /// Ported from the poly roll's `PolyGeom`.
    #[test]
    fn note_rect_of_a_note_at_the_tick_limit_does_not_overflow() {
        let g = LaneGeom {
            tick_w: 1.0,
            row_h: 1.0,
            ..melodic()
        };
        let _ = g.note_rect(&Note::new(u16::MAX, 5, 60, 1));
    }

    #[test]
    fn zoom_scales_the_step_width() {
        assert_eq!(tick_w(1.0), 5.5);
        assert_eq!(tick_w(2.0), 11.0);
    }

    #[test]
    fn follow_keeps_the_playhead_in_view() {
        assert_eq!(follow_scroll(0.0, 400.0, 100.0), 0.0, "already in view");
        assert_eq!(
            follow_scroll(0.0, 400.0, 350.0),
            250.0,
            "near the right edge"
        );
        assert_eq!(
            follow_scroll(500.0, 400.0, 20.0),
            0.0,
            "wrapped to the start"
        );
    }
}
