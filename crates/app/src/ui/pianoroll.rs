//! Piano roll over the same mono pattern as the 303 view. Click to add a
//! note, drag its body to move, drag its right edge to lengthen,
//! right-click to delete. Accent/slide lanes sit underneath.

use super::roll_edit::{
    ROLL_ROWS, RollDrag, RollGeom, RollSnapshot, drag_from_origin, note_at, notes, place,
    toggle_accent, toggle_slide,
};
use super::step_edit::NOTE_NAMES;
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::project::SynthPattern;
use eframe::egui::{self, Align2, FontId, Sense, Stroke, pos2, vec2};

const CELL_W: f32 = 22.0;
const CELL_H: f32 = 9.0;
const KEY_W: f32 = 30.0;
const LANE_H: f32 = 12.0;
const BLACK_KEYS: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

impl SidAcidApp {
    pub(crate) fn piano_roll(&mut self, ui: &mut egui::Ui, synth: usize) {
        let slot = self.doc.project.selected[synth];
        let mut pattern = self.doc.project.synths[synth].patterns[slot as usize].clone();
        let len = pattern.length as usize;
        let playhead = (self.status.playing && self.status.patterns[synth] == slot)
            .then_some(self.status.playhead[synth]);
        let mut changed = false;
        let mut finished = false;
        egui::ScrollArea::horizontal()
            .id_salt(("roll", synth))
            .show(ui, |ui| {
                let size = vec2(
                    KEY_W + CELL_W * len as f32,
                    CELL_H * ROLL_ROWS as f32 + 2.0 * LANE_H + 4.0,
                );
                let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
                let geom = RollGeom {
                    left: rect.left() + KEY_W,
                    top: rect.top(),
                    cell_w: CELL_W,
                    cell_h: CELL_H,
                    steps: len,
                };
                let grid_bottom = rect.top() + CELL_H * ROLL_ROWS as f32;
                let accent_lane = grid_bottom + 2.0;
                let slide_lane = accent_lane + LANE_H + 2.0;
                let painter = ui.painter_at(rect);

                for row in 0..ROLL_ROWS {
                    let (_, y0, _, y1) = geom.note_rect(0, row, 1);
                    let bg = if BLACK_KEYS[row % 12] {
                        theme::GRID_DARK
                    } else {
                        theme::GRID_BG
                    };
                    painter.rect_filled(
                        egui::Rect::from_min_max(pos2(geom.left, y0), pos2(rect.right(), y1)),
                        0.0,
                        bg,
                    );
                    if row % 12 == 0 {
                        painter.text(
                            pos2(rect.left() + 2.0, (y0 + y1) / 2.0),
                            Align2::LEFT_CENTER,
                            format!("{}{}", NOTE_NAMES[0], row as i32 / 12 - 1),
                            FontId::monospace(9.0),
                            ui.visuals().text_color(),
                        );
                    }
                }
                for step in 0..=len {
                    let x = geom.left + step as f32 * CELL_W;
                    let colour = if step % 4 == 0 {
                        theme::KNOB_TRACK
                    } else {
                        theme::GRID_LINE
                    };
                    painter.line_segment(
                        [pos2(x, rect.top()), pos2(x, grid_bottom)],
                        Stroke::new(1.0_f32, colour),
                    );
                }
                if let Some(step) = playhead {
                    let x = geom.left + step as f32 * CELL_W;
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            pos2(x, rect.top()),
                            pos2(x + CELL_W, grid_bottom),
                        ),
                        0.0,
                        theme::ACID.gamma_multiply(0.15),
                    );
                }
                for n in notes(&pattern) {
                    let (x0, y0, x1, y1) = geom.note_rect(n.start, n.row, n.len);
                    let r = egui::Rect::from_min_max(
                        pos2(x0 + 1.0, y0 + 1.0),
                        pos2(x1 - 1.0, y1 - 1.0),
                    );
                    painter.rect_filled(r, 2.0, if n.accent { theme::ACCENT } else { theme::ACID });
                    if n.slide {
                        painter.rect_stroke(
                            r,
                            2.0,
                            Stroke::new(2.0_f32, theme::SLIDE),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let lane = |y: f32, on: bool, label: &str, colour| {
                        let lx = geom.left + n.start as f32 * CELL_W;
                        let lr = egui::Rect::from_min_size(
                            pos2(lx + 1.0, y),
                            vec2(CELL_W - 2.0, LANE_H),
                        );
                        painter.rect_filled(lr, 2.0, if on { colour } else { theme::GRID_LINE });
                        painter.text(
                            lr.center(),
                            Align2::CENTER_CENTER,
                            label,
                            FontId::monospace(9.0),
                            egui::Color32::BLACK,
                        );
                    };
                    lane(accent_lane, n.accent, "A", theme::ACCENT);
                    lane(slide_lane, n.slide, "S", theme::SLIDE);
                }

                let pointer = resp.interact_pointer_pos();
                let in_lane = |y: f32, top: f32| y >= top && y < top + LANE_H;
                if resp.clicked()
                    && let Some(p) = pointer
                {
                    if in_lane(p.y, accent_lane) || in_lane(p.y, slide_lane) {
                        // A click left of the grid (the key strip) hits no step.
                        if let Some(step) = geom.lane_step(p.x) {
                            if in_lane(p.y, accent_lane) {
                                toggle_accent(&mut pattern, step);
                            } else {
                                toggle_slide(&mut pattern, step);
                            }
                            changed = true;
                            finished = true;
                        }
                    } else if let Some((step, row)) = geom.cell_at(p.x, p.y)
                        && note_at(&pattern, step).is_none()
                    {
                        place(&mut pattern, step, 1, row);
                        changed = true;
                        finished = true;
                    }
                }
                if resp.secondary_clicked()
                    && let Some(p) = pointer
                    && let Some((step, _)) = geom.cell_at(p.x, p.y)
                {
                    changed |= super::roll_edit::delete_at(&mut pattern, step);
                    finished = true;
                }
                if resp.drag_started()
                    && let Some(p) = pointer
                    && let Some((step, row)) = geom.cell_at(p.x, p.y)
                {
                    let drag = match note_at(&pattern, step) {
                        Some(n) if geom.near_right_edge(&n, p.x) => {
                            RollDrag::Resize { start: n.start }
                        }
                        Some(n) => RollDrag::Move {
                            start: n.start,
                            grab: step - n.start,
                        },
                        None => {
                            place(&mut pattern, step, 1, row);
                            changed = true;
                            RollDrag::Resize { start: step }
                        }
                    };
                    self.view.roll_drag[synth] = Some(drag);
                    // Every frame of the drag is recomputed from here.
                    self.view.roll_origin[synth] = Some(Box::new(RollSnapshot {
                        slot,
                        origin: pattern.clone(),
                        last: pattern.clone(),
                    }));
                }
                if resp.dragged()
                    && let Some(p) = pointer
                {
                    let (step, row) = geom.cell_at_clamped(p.x, p.y);
                    if let Some(next) = self.roll_drag_to(synth, slot, &pattern, step, row)
                        && next != pattern
                    {
                        pattern = next;
                        changed = true;
                    }
                }
                if resp.drag_stopped() {
                    self.view.roll_drag[synth] = None;
                    self.view.roll_origin[synth] = None;
                    finished = true;
                }
            });
        let dragging = self.view.roll_drag[synth].is_some();
        self.commit_roll(synth, slot, changed.then_some(pattern), dragging, finished);
    }

    /// One frame of a live roll drag: the pattern for the pointer at
    /// (`step`, `row`), recomputed from the drag's origin. A drag whose
    /// pattern changed under it (an undo, or another slot selected
    /// mid-drag) is dropped instead, so it can't write the old origin back
    /// over the change.
    pub(crate) fn roll_drag_to(
        &mut self,
        synth: usize,
        slot: u8,
        current: &SynthPattern,
        step: usize,
        row: usize,
    ) -> Option<SynthPattern> {
        let drag = self.view.roll_drag[synth]?;
        let snap = self.view.roll_origin[synth].as_mut()?;
        if snap.slot != slot || snap.last != *current {
            self.view.roll_drag[synth] = None;
            self.view.roll_origin[synth] = None;
            self.doc.end_gesture();
            return None;
        }
        let next = drag_from_origin(&snap.origin, drag, step, row);
        snap.last = next.clone();
        Some(next)
    }

    /// Applies a frame's roll edit (merging into the open gesture while a
    /// drag is live) and closes the gesture when the edit is finished.
    pub(crate) fn commit_roll(
        &mut self,
        synth: usize,
        slot: u8,
        pattern: Option<SynthPattern>,
        dragging: bool,
        finished: bool,
    ) {
        if let Some(pattern) = pattern {
            let cmds = self.doc.apply(
                Edit::SynthPattern {
                    synth,
                    slot,
                    pattern: Box::new(pattern),
                },
                dragging,
            );
            self.send(cmds);
            self.view.focus = synth;
        }
        if finished {
            self.doc.end_gesture();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::ui::roll_edit::{drag_from_origin, place};
    use acid_engine::project::Project;

    #[test]
    fn roll_drag_is_one_undo_step() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let mut original = app.doc.project.synths[0].patterns[0].clone();
        place(&mut original, 0, 2, 12);
        app.commit_roll(0, 0, Some(original.clone()), false, true);
        let before = app.doc.project.synths[0].patterns[0].clone();
        assert_eq!(before, original);
        let drag = RollDrag::Move { start: 0, grab: 0 };
        for step in [1, 2, 3, 4] {
            let next = drag_from_origin(&before, drag, step, 12);
            app.commit_roll(0, 0, Some(next), true, false);
        }
        app.commit_roll(0, 0, None, false, true);
        assert_ne!(app.doc.project.synths[0].patterns[0], before);
        app.doc.undo();
        assert_eq!(app.doc.project.synths[0].patterns[0], before, "one undo");
    }

    /// Starts a Move drag of a two-step note at step 0, as a drag-start
    /// frame does, and runs one drag frame to step 2.
    fn app_dragging_a_note() -> SidAcidApp {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let mut pattern = app.doc.project.synths[0].patterns[0].clone();
        place(&mut pattern, 0, 2, 12);
        app.commit_roll(0, 0, Some(pattern.clone()), false, true);
        app.view.roll_drag[0] = Some(RollDrag::Move { start: 0, grab: 0 });
        app.view.roll_origin[0] = Some(Box::new(RollSnapshot {
            slot: 0,
            origin: pattern.clone(),
            last: pattern.clone(),
        }));
        let next = app
            .roll_drag_to(0, 0, &pattern, 2, 12)
            .expect("a live drag");
        app.commit_roll(0, 0, Some(next), true, false);
        app
    }

    #[test]
    fn a_drag_continues_while_its_pattern_is_unchanged() {
        let mut app = app_dragging_a_note();
        let current = app.doc.project.synths[0].patterns[0].clone();
        assert!(app.roll_drag_to(0, 0, &current, 3, 12).is_some());
    }

    #[test]
    fn an_undo_mid_drag_drops_the_drag() {
        let mut app = app_dragging_a_note();
        app.doc.undo();
        let current = app.doc.project.synths[0].patterns[0].clone();
        assert!(app.roll_drag_to(0, 0, &current, 3, 12).is_none());
        assert_eq!(app.view.roll_drag[0], None);
        assert!(app.view.roll_origin[0].is_none());
        assert_eq!(
            app.doc.project.synths[0].patterns[0], current,
            "the undo stands"
        );
    }

    #[test]
    fn selecting_another_slot_mid_drag_drops_the_drag() {
        let mut app = app_dragging_a_note();
        let other = app.doc.project.synths[0].patterns[1].clone();
        assert!(app.roll_drag_to(0, 1, &other, 3, 12).is_none());
        assert_eq!(app.view.roll_drag[0], None);
    }
}
