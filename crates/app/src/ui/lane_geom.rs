//! Pure lane geometry for the dock: ticks to x with the dock's shared
//! horizontal scroll, rows to y with the lane's own pitch scroll. Melodic
//! lanes have one row per MIDI pitch (127 on top); drum lanes one row per
//! instrument (the kick at the bottom).

use acid_engine::params::DRUM_INSTRUMENTS;
use acid_engine::project::{Note, TICKS_PER_STEP};
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
        match self.rows {
            Rows::Pitches => 128,
            Rows::Drums => DRUM_INSTRUMENTS,
        }
    }

    pub fn row_of(&self, pitch: u8) -> Option<usize> {
        let n = self.row_count();
        ((pitch as usize) < n).then(|| n - 1 - pitch as usize)
    }

    pub fn pitch_of_row(&self, row: usize) -> Option<u8> {
        let n = self.row_count();
        (row < n).then(|| (n - 1 - row) as u8)
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
        let r = (r.max(0.0) as usize).min(self.row_count() - 1);
        self.pitch_of_row(r).unwrap_or(0)
    }

    pub fn note_rect(&self, n: &Note) -> Rect {
        let row = self.row_of(n.pitch).unwrap_or(0);
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
#[allow(dead_code)] // used by the dock (phase 2, tasks 5-6)
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
