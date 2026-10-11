//! Pure piano-roll editing on the shared mono pattern, plus geometry.
//! A note is an On step followed by Tie steps; accent is on its first
//! step and slide on its last (the slide into the following note).

use acid_engine::project::{Gate, SynthPattern, SynthStep};

/// Three octaves: row 0 = C at octave -1, row 35 = B at octave +1.
pub const ROLL_ROWS: usize = 36;
const EDGE_PX: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollNote {
    pub start: usize,
    pub len: usize,
    pub row: usize,
    pub accent: bool,
    pub slide: bool,
}

/// An in-progress mouse drag in the roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollDrag {
    Move { start: usize, grab: usize },
    Resize { start: usize },
}

pub fn row_of(step: &SynthStep) -> usize {
    (step.octave.clamp(-1, 1) + 1) as usize * 12 + (step.note % 12) as usize
}

fn step_for(row: usize, gate: Gate) -> SynthStep {
    SynthStep {
        note: (row % 12) as u8,
        octave: (row / 12) as i8 - 1,
        gate,
        accent: false,
        slide: false,
    }
}

fn length_of(p: &SynthPattern) -> usize {
    (p.length as usize).clamp(1, p.steps.len())
}

pub fn notes(p: &SynthPattern) -> Vec<RollNote> {
    let len = length_of(p);
    let mut out = Vec::new();
    let mut i = 0;
    while i < len {
        if p.steps[i].gate == Gate::Off {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        while end < len && p.steps[end].gate == Gate::Tie {
            end += 1;
        }
        out.push(RollNote {
            start,
            len: end - start,
            row: row_of(&p.steps[start]),
            accent: p.steps[start].accent,
            slide: p.steps[end - 1].slide,
        });
        i = end;
    }
    out
}

pub fn note_at(p: &SynthPattern, step: usize) -> Option<RollNote> {
    notes(p)
        .into_iter()
        .find(|n| step >= n.start && step < n.start + n.len)
}

fn erase(p: &mut SynthPattern, n: RollNote) {
    for s in &mut p.steps[n.start..n.start + n.len] {
        *s = SynthStep {
            gate: Gate::Off,
            accent: false,
            slide: false,
            ..*s
        };
    }
}

/// Writes a note (On + Ties), removing any note it overlaps. Starts at or
/// beyond the pattern length are ignored; the note is clipped to it.
pub fn place(p: &mut SynthPattern, start: usize, len: usize, row: usize) {
    let plen = length_of(p);
    if start >= plen {
        return;
    }
    let len = len.clamp(1, plen - start);
    let row = row.min(ROLL_ROWS - 1);
    for n in notes(p) {
        if n.start < start + len && start < n.start + n.len {
            erase(p, n);
        }
    }
    for k in 0..len {
        p.steps[start + k] = step_for(row, if k == 0 { Gate::On } else { Gate::Tie });
    }
    if start + len < plen && p.steps[start + len].gate == Gate::Tie {
        p.steps[start + len].gate = Gate::On;
    }
}

pub fn delete_at(p: &mut SynthPattern, step: usize) -> bool {
    match note_at(p, step) {
        Some(n) => {
            erase(p, n);
            true
        }
        None => false,
    }
}

fn restore_marks(p: &mut SynthPattern, start: usize, accent: bool, slide: bool) {
    if let Some(n) = note_at(p, start) {
        p.steps[n.start].accent = accent;
        p.steps[n.start + n.len - 1].slide = slide;
    }
}

/// Changes the length of the note starting at `start`, keeping its marks.
pub fn resize(p: &mut SynthPattern, start: usize, new_len: usize) {
    if let Some(n) = note_at(p, start)
        && n.start == start
    {
        erase(p, n);
        place(p, start, new_len, n.row);
        restore_marks(p, start, n.accent, n.slide);
    }
}

/// Moves the note starting at `start` to `new_start` / `new_row`.
pub fn move_note(p: &mut SynthPattern, start: usize, new_start: usize, new_row: usize) {
    if let Some(n) = note_at(p, start)
        && n.start == start
    {
        erase(p, n);
        place(p, new_start, n.len, new_row);
        restore_marks(p, new_start, n.accent, n.slide);
    }
}

pub fn toggle_accent(p: &mut SynthPattern, step: usize) {
    if let Some(n) = note_at(p, step) {
        p.steps[n.start].accent = !p.steps[n.start].accent;
    }
}

pub fn toggle_slide(p: &mut SynthPattern, step: usize) {
    if let Some(n) = note_at(p, step) {
        let last = n.start + n.len - 1;
        p.steps[last].slide = !p.steps[last].slide;
    }
}

/// Grid placement in screen pixels. Row `ROLL_ROWS - 1` is at the top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RollGeom {
    pub left: f32,
    pub top: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub steps: usize,
}

impl RollGeom {
    pub fn cell_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let col = ((x - self.left) / self.cell_w).floor();
        let r = ((y - self.top) / self.cell_h).floor();
        if col < 0.0 || r < 0.0 || col as usize >= self.steps || r as usize >= ROLL_ROWS {
            return None;
        }
        Some((col as usize, ROLL_ROWS - 1 - r as usize))
    }

    pub fn cell_at_clamped(&self, x: f32, y: f32) -> (usize, usize) {
        let col = ((x - self.left) / self.cell_w)
            .floor()
            .clamp(0.0, (self.steps.max(1) - 1) as f32) as usize;
        let r = ((y - self.top) / self.cell_h)
            .floor()
            .clamp(0.0, (ROLL_ROWS - 1) as f32) as usize;
        (col, ROLL_ROWS - 1 - r)
    }

    /// (x0, y0, x1, y1) of a note.
    pub fn note_rect(&self, start: usize, row: usize, len: usize) -> (f32, f32, f32, f32) {
        let x0 = self.left + start as f32 * self.cell_w;
        let y0 = self.top + (ROLL_ROWS - 1 - row) as f32 * self.cell_h;
        (x0, y0, x0 + len as f32 * self.cell_w, y0 + self.cell_h)
    }

    pub fn near_right_edge(&self, note: &RollNote, x: f32) -> bool {
        let (_, _, x1, _) = self.note_rect(note.start, note.row, note.len);
        x >= x1 - EDGE_PX && x <= x1 + 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(len: u8) -> SynthPattern {
        SynthPattern {
            length: len,
            ..SynthPattern::default()
        }
    }

    #[test]
    fn notes_group_on_and_ties() {
        let mut p = pattern(16);
        place(&mut p, 2, 3, 14);
        place(&mut p, 8, 1, 0);
        let n = notes(&p);
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].start, n[0].len, n[0].row), (2, 3, 14));
        assert_eq!(p.steps[2].gate, Gate::On);
        assert_eq!(p.steps[3].gate, Gate::Tie);
        assert_eq!((p.steps[2].note, p.steps[2].octave), (2, 0));
        assert_eq!((n[1].start, n[1].row), (8, 0));
        assert_eq!(p.steps[8].octave, -1);
    }

    #[test]
    fn placing_over_a_note_replaces_it() {
        let mut p = pattern(16);
        place(&mut p, 2, 4, 10);
        place(&mut p, 4, 1, 20);
        let n = notes(&p);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].start, 4);
        assert_eq!(p.steps[2].gate, Gate::Off);
    }

    #[test]
    fn placing_before_a_tie_keeps_the_notes_separate() {
        let mut p = pattern(16);
        place(&mut p, 4, 3, 10);
        place(&mut p, 2, 3, 12);
        assert_eq!(notes(&p).len(), 1, "overlapping note removed");
        let mut p = pattern(16);
        p.steps[5].gate = Gate::Tie;
        place(&mut p, 3, 2, 12);
        assert_eq!(p.steps[5].gate, Gate::On);
    }

    #[test]
    fn notes_are_clipped_to_the_pattern_length() {
        let mut p = pattern(8);
        place(&mut p, 6, 10, 5);
        assert_eq!(notes(&p)[0].len, 2);
        place(&mut p, 9, 1, 5);
        assert_eq!(notes(&p).len(), 1, "start beyond length is ignored");
    }

    #[test]
    fn delete_resize_move() {
        let mut p = pattern(16);
        place(&mut p, 0, 2, 12);
        toggle_accent(&mut p, 1);
        toggle_slide(&mut p, 0);
        assert!(p.steps[0].accent && p.steps[1].slide);
        resize(&mut p, 0, 4);
        assert_eq!(note_at(&p, 3).unwrap().len, 4);
        assert!(
            p.steps[0].accent && p.steps[3].slide,
            "marks kept on resize"
        );
        move_note(&mut p, 0, 6, 20);
        let n = note_at(&p, 6).unwrap();
        assert_eq!((n.start, n.len, n.row), (6, 4, 20));
        assert!(p.steps[6].accent && p.steps[9].slide, "marks kept on move");
        assert!(delete_at(&mut p, 8));
        assert!(notes(&p).is_empty());
        assert!(!delete_at(&mut p, 8));
    }

    #[test]
    fn geometry_maps_cells() {
        let g = RollGeom {
            left: 100.0,
            top: 50.0,
            cell_w: 20.0,
            cell_h: 10.0,
            steps: 16,
        };
        assert_eq!(g.cell_at(100.0, 50.0), Some((0, ROLL_ROWS - 1)));
        assert_eq!(g.cell_at(145.0, 50.0 + 10.0 * 35.5), Some((2, 0)));
        assert_eq!(g.cell_at(99.0, 60.0), None);
        assert_eq!(g.cell_at(100.0 + 20.0 * 16.0, 60.0), None);
        assert_eq!(g.cell_at_clamped(1000.0, -5.0), (15, ROLL_ROWS - 1));
        let (x0, y0, x1, y1) = g.note_rect(2, 0, 3);
        assert_eq!((x0, x1), (140.0, 200.0));
        assert_eq!((y0, y1), (50.0 + 350.0, 50.0 + 360.0));
        assert!(g.near_right_edge(
            &RollNote {
                start: 2,
                len: 3,
                row: 0,
                accent: false,
                slide: false
            },
            198.0
        ));
        assert!(!g.near_right_edge(
            &RollNote {
                start: 2,
                len: 3,
                row: 0,
                accent: false,
                slide: false
            },
            170.0
        ));
    }
}
