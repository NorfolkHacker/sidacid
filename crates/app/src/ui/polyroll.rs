//! Poly piano roll for the FM synth and sampler: one column per tick,
//! every MIDI pitch, a velocity lane underneath. Click adds a note (with
//! the last length used), drag the body to move, drag the right edge to
//! stretch, right-click deletes, chords stack freely. Clicking the key
//! strip auditions the pitch.

use super::poly_edit::{
    PolyDrag, PolyGeom, ROWS, add, delete, move_to, note_at, resize, set_velocity_in,
};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::machine::MachineId;
use acid_engine::project::TICKS_PER_STEP;
use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, pos2, vec2};

const TICK_W: f32 = 6.0;
const ROW_H: f32 = 8.0;
const KEY_W: f32 = 30.0;
const VEL_H: f32 = 32.0;
const VIEW_H: f32 = 260.0;
const NEW_VEL: u8 = 100;
/// Pitch the FM roll first scrolls to: C3 (MIDI 48).
const FM_START_PITCH: u8 = 48;
const BLACK_KEYS: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

fn vel_from_y(y: f32, top: f32) -> u8 {
    ((1.0 - (y - top) / VEL_H) * 127.0)
        .round()
        .clamp(1.0, 127.0) as u8
}

impl SidAcidApp {
    /// Pitch roll `n` (0 FM, 1 sampler) first centres on: C3 for FM; for
    /// the sampler the lowest loaded zone's base key, or C1 with none.
    pub(crate) fn roll_start_pitch(&self, n: usize) -> u8 {
        if n == 0 {
            return FM_START_PITCH;
        }
        self.doc
            .project
            .sampler
            .slots
            .iter()
            .flatten()
            .map(|d| d.meta.base_key)
            .min()
            .unwrap_or(acid_engine::sampler::FIRST_KEY)
    }

    pub(crate) fn poly_roll(&mut self, ui: &mut egui::Ui, machine: usize) {
        let n = machine - MachineId::Fm.index();
        let slot = self.doc.project.selected[machine];
        let Some(bank) = self.doc.project.note_patterns(machine) else {
            return;
        };
        let mut pattern = bank[slot as usize].clone();
        let ticks = pattern.ticks();
        let snap = self.view.poly_snap[n];
        let new_len = match self.view.poly_len[n] {
            0 => TICKS_PER_STEP,
            l => l,
        };
        let playhead = (self.status.playing && self.status.patterns[machine] == slot)
            .then_some(self.status.playhead[machine]);
        let width = KEY_W + TICK_W * ticks as f32;
        let mut changed = false;
        let mut finished = false;
        let mut key_down: Option<u8> = None;

        egui::ScrollArea::horizontal()
            .id_salt(("poly_h", machine))
            .show(ui, |ui| {
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(("poly_v", machine))
                    .max_height(VIEW_H);
                if !self.view.poly_scrolled[n] {
                    let pitch = self.roll_start_pitch(n) as usize;
                    let y = (ROWS - 1 - pitch) as f32 * ROW_H - VIEW_H / 2.0;
                    scroll = scroll.vertical_scroll_offset(y.max(0.0));
                    self.view.poly_scrolled[n] = true;
                }
                scroll.show(ui, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(
                        vec2(width, ROW_H * ROWS as f32),
                        Sense::click_and_drag(),
                    );
                    let geom = PolyGeom {
                        left: rect.left() + KEY_W,
                        top: rect.top(),
                        tick_w: TICK_W,
                        row_h: ROW_H,
                        ticks,
                    };
                    let painter = ui.painter_at(rect);
                    let held = self.view.audition.filter(|a| a.0 == machine).map(|a| a.1);
                    for pitch in 0..ROWS as u8 {
                        let y0 = geom.row_top(pitch);
                        let black = BLACK_KEYS[pitch as usize % 12];
                        let row = egui::Rect::from_min_max(
                            pos2(geom.left, y0),
                            pos2(rect.right(), y0 + ROW_H),
                        );
                        painter.rect_filled(
                            row,
                            0.0,
                            if black {
                                theme::GRID_DARK
                            } else {
                                theme::GRID_BG
                            },
                        );
                        let sampler_row = if machine == MachineId::Sampler.index() {
                            super::sampler_ui::row_label(&self.doc.project.sampler.slots, pitch)
                        } else {
                            None
                        };
                        if let Some((slot, _)) = &sampler_row {
                            painter.rect_filled(
                                row,
                                0.0,
                                super::sampler_ui::SLOT_TINTS[slot % 4].gamma_multiply(0.18),
                            );
                        }
                        let key = egui::Rect::from_min_max(
                            pos2(rect.left(), y0),
                            pos2(geom.left - 1.0, y0 + ROW_H - 1.0),
                        );
                        let key_colour = if held == Some(pitch) {
                            theme::ACID
                        } else if black {
                            theme::GRID_DARK
                        } else {
                            Color32::from_gray(190)
                        };
                        painter.rect_filled(key, 1.0, key_colour);
                        if let Some((_, text)) = &sampler_row {
                            painter.text(
                                pos2(rect.left() + 1.0, y0 + ROW_H / 2.0),
                                Align2::LEFT_CENTER,
                                text,
                                FontId::monospace(6.5),
                                if black {
                                    Color32::from_gray(230)
                                } else {
                                    Color32::BLACK
                                },
                            );
                        } else if pitch % 12 == 0 {
                            painter.text(
                                pos2(rect.left() + 2.0, y0 + ROW_H / 2.0),
                                Align2::LEFT_CENTER,
                                format!("C{}", pitch as i32 / 12 - 1),
                                FontId::monospace(8.0),
                                Color32::BLACK,
                            );
                        }
                    }
                    for step in 0..=ticks / TICKS_PER_STEP {
                        let x = geom.left + (step * TICKS_PER_STEP) as f32 * TICK_W;
                        let colour = if step % 4 == 0 {
                            theme::KNOB_TRACK
                        } else {
                            theme::GRID_LINE
                        };
                        painter.line_segment(
                            [pos2(x, rect.top()), pos2(x, rect.bottom())],
                            Stroke::new(1.0_f32, colour),
                        );
                    }
                    if let Some(step) = playhead {
                        let x = geom.left + (step as u16 * TICKS_PER_STEP) as f32 * TICK_W;
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                pos2(x, rect.top()),
                                pos2(x + TICKS_PER_STEP as f32 * TICK_W, rect.bottom()),
                            ),
                            0.0,
                            theme::ACID.gamma_multiply(0.15),
                        );
                    }
                    for note in pattern.notes.iter().filter(|n| n.start < ticks) {
                        let (x0, y0, x1, y1) = geom.note_rect(note);
                        let r = egui::Rect::from_min_max(
                            pos2(x0 + 0.5, y0 + 0.5),
                            pos2(x1 - 0.5, y1 - 0.5),
                        );
                        let shade = 0.35 + 0.65 * note.vel as f32 / 127.0;
                        painter.rect_filled(r, 2.0, theme::ACID.gamma_multiply(shade));
                    }

                    let pointer = resp.interact_pointer_pos();
                    if resp.is_pointer_button_down_on()
                        && let Some(p) = pointer
                        && p.x < geom.left
                    {
                        key_down = geom.pitch_at(p.y);
                    }
                    if resp.clicked()
                        && let Some(p) = pointer
                        && let Some((tick, pitch)) = geom.cell_at(p.x, p.y)
                        && note_at(&pattern, tick, pitch).is_none()
                    {
                        changed |=
                            add(&mut pattern, snap.floor(tick), new_len, pitch, NEW_VEL).is_some();
                        finished = true;
                    }
                    if resp.secondary_clicked()
                        && let Some(p) = pointer
                        && let Some((tick, pitch)) = geom.cell_at(p.x, p.y)
                        && let Some(i) = note_at(&pattern, tick, pitch)
                    {
                        changed |= delete(&mut pattern, i);
                        finished = true;
                    }
                    if resp.drag_started()
                        && let Some(p) = pointer
                        && let Some((tick, pitch)) = geom.cell_at(p.x, p.y)
                    {
                        self.view.poly_drag[n] = match note_at(&pattern, tick, pitch) {
                            Some(i) if geom.near_right_edge(&pattern.notes[i], p.x) => {
                                Some(PolyDrag::Resize { index: i })
                            }
                            Some(i) => Some(PolyDrag::Move {
                                index: i,
                                grab: tick - pattern.notes[i].start,
                            }),
                            None => {
                                let added = add(
                                    &mut pattern,
                                    snap.floor(tick),
                                    snap.ticks(),
                                    pitch,
                                    NEW_VEL,
                                );
                                changed |= added.is_some();
                                added.map(|index| PolyDrag::Resize { index })
                            }
                        };
                    }
                    if resp.dragged()
                        && let Some(p) = pointer
                        && let Some(drag) = self.view.poly_drag[n]
                    {
                        let (tick, pitch) = geom.cell_at_clamped(p.x, p.y);
                        match drag {
                            PolyDrag::Resize { index } => {
                                if let Some(start) = pattern.notes.get(index).map(|n| n.start) {
                                    let len = (snap.floor(tick) + snap.ticks())
                                        .saturating_sub(start)
                                        .max(1);
                                    if resize(&mut pattern, index, len) {
                                        self.view.poly_len[n] = len;
                                        changed = true;
                                    }
                                }
                            }
                            PolyDrag::Move { index, grab } => {
                                let start = snap.floor(tick.saturating_sub(grab));
                                changed |= move_to(&mut pattern, index, start, pitch);
                            }
                            PolyDrag::Velocity => {}
                        }
                    }
                    if resp.drag_stopped() && self.view.poly_drag[n] != Some(PolyDrag::Velocity) {
                        self.view.poly_drag[n] = None;
                        finished = true;
                    }
                });

                let (lane, lresp) =
                    ui.allocate_exact_size(vec2(width, VEL_H), Sense::click_and_drag());
                let left = lane.left() + KEY_W;
                let painter = ui.painter_at(lane);
                painter.rect_filled(
                    egui::Rect::from_min_max(pos2(left, lane.top()), lane.right_bottom()),
                    0.0,
                    theme::GRID_DARK,
                );
                painter.text(
                    pos2(lane.left() + 2.0, lane.center().y),
                    Align2::LEFT_CENTER,
                    "Vel",
                    FontId::monospace(9.0),
                    ui.visuals().text_color(),
                );
                for note in pattern.notes.iter().filter(|n| n.start < ticks) {
                    let x = left + note.start as f32 * TICK_W;
                    let h = VEL_H * note.vel as f32 / 127.0;
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            pos2(x, lane.bottom() - h),
                            pos2(x + TICK_W - 1.0, lane.bottom()),
                        ),
                        0.0,
                        theme::ACID,
                    );
                }
                if lresp.drag_started() {
                    self.view.poly_drag[n] = Some(PolyDrag::Velocity);
                }
                if (lresp.clicked() || lresp.dragged())
                    && let Some(p) = lresp.interact_pointer_pos()
                    && p.x >= left
                {
                    let tick = (((p.x - left) / TICK_W).floor() as u16).min(ticks - 1);
                    let from = snap.floor(tick);
                    changed |= set_velocity_in(
                        &mut pattern,
                        from,
                        from + snap.ticks(),
                        vel_from_y(p.y, lane.top()),
                    );
                    finished |= lresp.clicked();
                }
                if lresp.drag_stopped() {
                    self.view.poly_drag[n] = None;
                    finished = true;
                }
            });

        self.audition(machine, key_down);
        if changed {
            let dragging = self.view.poly_drag[n].is_some();
            let cmds = self.doc.apply(
                Edit::NotePattern {
                    machine,
                    slot,
                    pattern: Box::new(pattern),
                },
                dragging,
            );
            self.send(cmds);
            self.view.focus = machine;
        }
        if finished {
            self.doc.end_gesture();
        }
    }

    /// Follows the key-strip press: starts the pressed pitch, stops the old one.
    fn audition(&mut self, machine: usize, pitch: Option<u8>) {
        let now = pitch.map(|p| (machine, p));
        let before = self.view.audition;
        if before == now || (now.is_none() && before.is_some_and(|(m, _)| m != machine)) {
            return;
        }
        let mut cmds = Vec::new();
        if let Some((m, p)) = before {
            cmds.push(Command::NoteOff {
                machine: m as u8,
                pitch: p,
            });
        }
        if let Some((m, p)) = now {
            cmds.push(Command::NoteOn {
                machine: m as u8,
                pitch: p,
                vel: NEW_VEL,
            });
        }
        self.view.audition = now;
        self.send(cmds);
    }
}
