//! Piano roll over the same mono pattern as the 303 view. Click to add a
//! note, drag its body to move, drag its right edge to lengthen,
//! right-click to delete. Accent/slide lanes sit underneath.

use super::roll_edit::{
    ROLL_ROWS, RollDrag, RollGeom, move_note, note_at, notes, place, resize, toggle_accent,
    toggle_slide,
};
use super::step_edit::NOTE_NAMES;
use super::{SidAcidApp, theme};
use crate::document::Edit;
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
                    let (step, _) = geom.cell_at_clamped(p.x, p.y);
                    if in_lane(p.y, accent_lane) {
                        toggle_accent(&mut pattern, step);
                        changed = true;
                        finished = true;
                    } else if in_lane(p.y, slide_lane) {
                        toggle_slide(&mut pattern, step);
                        changed = true;
                        finished = true;
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
                    self.view.roll_drag[synth] = Some(match note_at(&pattern, step) {
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
                    });
                }
                if resp.dragged()
                    && let Some(p) = pointer
                    && let Some(drag) = self.view.roll_drag[synth]
                {
                    let (step, row) = geom.cell_at_clamped(p.x, p.y);
                    match drag {
                        RollDrag::Resize { start } => {
                            let new_len = step.saturating_sub(start) + 1;
                            if note_at(&pattern, start)
                                .is_some_and(|n| n.start == start && n.len != new_len)
                            {
                                resize(&mut pattern, start, new_len);
                                changed = true;
                            }
                        }
                        RollDrag::Move { start, grab } => {
                            let new_start = step.saturating_sub(grab);
                            if let Some(n) = note_at(&pattern, start)
                                && n.start == start
                                && (new_start != start || row != n.row)
                            {
                                move_note(&mut pattern, start, new_start, row);
                                self.view.roll_drag[synth] = Some(RollDrag::Move {
                                    start: new_start,
                                    grab,
                                });
                                changed = true;
                            }
                        }
                    }
                }
                if resp.drag_stopped() {
                    self.view.roll_drag[synth] = None;
                    finished = true;
                }
            });
        if changed {
            let dragging = self.view.roll_drag[synth].is_some();
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
