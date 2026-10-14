//! Poly piano roll for the FM synth and sampler: one column per tick,
//! every MIDI pitch, a velocity lane underneath. Click adds a note (with
//! the last length used), drag the body to move, drag the right edge to
//! stretch, right-click deletes, chords stack freely. Clicking the key
//! strip auditions the pitch.

use super::poly_edit::{
    PolyDrag, PolyGeom, ROWS, add, delete, move_to, note_at, resize, resolve_overlaps,
    set_velocity_in,
};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::machine::MachineUid;
use acid_engine::project::{NotePattern, TICKS_PER_STEP};
use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, pos2, vec2};

const TICK_W: f32 = 6.0;
/// Rows sized like the acid synth's piano roll: three octaves in view.
const ROW_H: f32 = 12.0;
const KEY_W: f32 = 30.0;
/// The sampler's key strip is wider to fit row labels like "Amen ·05".
const SAMPLER_KEY_W: f32 = 64.0;
const VEL_H: f32 = 32.0;
const VIEW_H: f32 = ROW_H * super::roll_edit::ROLL_ROWS as f32;
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
    /// Pitch the roll of note machine `uid` first centres on: for a
    /// sampler the lowest loaded zone's base key, or C1 with none; C3 for FM.
    pub(crate) fn roll_start_pitch(&self, uid: MachineUid) -> u8 {
        let Some(sampler) = self.doc.project.sampler(uid) else {
            return FM_START_PITCH;
        };
        sampler
            .slots
            .iter()
            .flatten()
            .map(|d| d.meta.base_key)
            .min()
            .unwrap_or(acid_engine::sampler::FIRST_KEY)
    }

    pub(crate) fn poly_roll(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let is_sampler = self.doc.project.sampler(uid).is_some();
        let key_w = if is_sampler { SAMPLER_KEY_W } else { KEY_W };
        let slot = self.selected(uid);
        let Some(bank) = self.doc.project.note_patterns(uid) else {
            return;
        };
        let mut pattern = bank[slot as usize].clone();
        let ticks = pattern.ticks();
        let panel = self.view.panel(uid);
        let snap = panel.poly_snap;
        let new_len = match panel.poly_len {
            0 => TICKS_PER_STEP,
            l => l,
        };
        let playhead = self.playhead(uid, slot);
        let width = key_w + TICK_W * ticks as f32;
        let mut changed = false;
        let mut finished = false;
        let mut key_down: Option<u8> = None;
        let mut dropped: Option<usize> = None;

        egui::ScrollArea::horizontal()
            .id_salt(("poly_h", uid))
            .show(ui, |ui| {
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt(("poly_v", uid))
                    .max_height(VIEW_H);
                if !self.view.panel(uid).poly_scrolled {
                    let pitch = self.roll_start_pitch(uid) as usize;
                    let y = (ROWS - 1 - pitch) as f32 * ROW_H - VIEW_H / 2.0;
                    scroll = scroll.vertical_scroll_offset(y.max(0.0));
                    self.view.panel(uid).poly_scrolled = true;
                }
                scroll.show(ui, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(
                        vec2(width, ROW_H * ROWS as f32),
                        Sense::click_and_drag(),
                    );
                    let geom = PolyGeom {
                        left: rect.left() + key_w,
                        top: rect.top(),
                        tick_w: TICK_W,
                        row_h: ROW_H,
                        ticks,
                    };
                    let painter = ui.painter_at(rect);
                    let held = self.view.audition.filter(|a| a.0 == uid).map(|a| a.1);
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
                        let sampler_row = if let Some(sampler) = self.doc.project.sampler(uid) {
                            super::sampler_ui::row_label(&sampler.slots, pitch)
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
                                FontId::monospace(8.0),
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
                        self.view.panel(uid).poly_drag = match note_at(&pattern, tick, pitch) {
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
                        && let Some(drag) = self.view.panel(uid).poly_drag
                    {
                        let (tick, pitch) = geom.cell_at_clamped(p.x, p.y);
                        match drag {
                            PolyDrag::Resize { index } => {
                                if let Some(start) = pattern.notes.get(index).map(|n| n.start) {
                                    let len = (snap.floor(tick) + snap.ticks())
                                        .saturating_sub(start)
                                        .max(1);
                                    if resize(&mut pattern, index, len) {
                                        self.view.panel(uid).poly_len = len;
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
                    let drag = self.view.panel(uid).poly_drag;
                    if resp.drag_stopped() && drag != Some(PolyDrag::Velocity) {
                        if let Some(PolyDrag::Move { index, .. } | PolyDrag::Resize { index }) =
                            drag
                        {
                            dropped = Some(index);
                        }
                        self.view.panel(uid).poly_drag = None;
                        finished = true;
                    }
                });

                let (lane, lresp) =
                    ui.allocate_exact_size(vec2(width, VEL_H), Sense::click_and_drag());
                let left = lane.left() + key_w;
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
                    self.view.panel(uid).poly_drag = Some(PolyDrag::Velocity);
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
                    self.view.panel(uid).poly_drag = None;
                    finished = true;
                }
            });

        self.audition(uid, key_down);
        self.commit_poly_frame(uid, slot, pattern, changed, dropped, finished);
    }

    /// Commits one frame of the poly roll. On the frame a move/resize is
    /// dropped (`dropped` = the note's index) same-pitch overlaps merge into
    /// it first, and the frame counts as still dragging: `poly_drag` is
    /// already cleared by then, but the drop's edits must merge into the
    /// drag's undo step, not push a second one.
    fn commit_poly_frame(
        &mut self,
        uid: MachineUid,
        slot: u8,
        mut pattern: NotePattern,
        mut changed: bool,
        dropped: Option<usize>,
        finished: bool,
    ) {
        if let Some(index) = dropped {
            let before = pattern.clone();
            resolve_overlaps(&mut pattern, index);
            changed |= pattern != before;
        }
        let dragging = dropped.is_some() || self.view.panel(uid).poly_drag.is_some();
        self.commit_poly(uid, slot, changed.then_some(pattern), dragging, finished);
    }

    /// Applies a frame's roll edit (merging into the open gesture while a
    /// drag is live) and closes the gesture when the edit is finished.
    pub(crate) fn commit_poly(
        &mut self,
        uid: MachineUid,
        slot: u8,
        pattern: Option<NotePattern>,
        dragging: bool,
        finished: bool,
    ) {
        if let Some(pattern) = pattern {
            let cmds = self.doc.apply(
                Edit::NotePattern {
                    uid,
                    slot,
                    pattern: Box::new(pattern),
                },
                dragging,
            );
            self.send(cmds);
            self.view.focus = Some(uid);
        }
        if finished {
            self.doc.end_gesture();
        }
    }

    /// Once per frame, after the panels: a held key or open drag whose roll
    /// was not drawn this frame (its panel collapsed mid-gesture) is
    /// released once the pointer is up, so no audition note keeps sounding
    /// and no undo gesture stays open.
    pub(crate) fn release_stale_poly_input(&mut self, pointer_down: bool) {
        if pointer_down {
            return;
        }
        if let Some((m, _)) = self.view.audition {
            self.audition(m, None);
        }
        let mut open = false;
        for panel in self.view.panels.values_mut() {
            open |= panel.poly_drag.take().is_some();
        }
        if open {
            self.doc.end_gesture();
        }
    }

    /// Follows the key-strip press: starts the pressed pitch, stops the old one.
    fn audition(&mut self, uid: MachineUid, pitch: Option<u8>) {
        let now = pitch.map(|p| (uid, p));
        let before = self.view.audition;
        if before == now || (now.is_none() && before.is_some_and(|(m, _)| m != uid)) {
            return;
        }
        let mut cmds = Vec::new();
        if let Some((m, p)) = before {
            cmds.push(Command::NoteOff { uid: m, pitch: p });
        }
        if let Some((m, p)) = now {
            cmds.push(Command::NoteOn {
                uid: m,
                pitch: p,
                vel: NEW_VEL,
            });
        }
        self.view.audition = now;
        self.send(cmds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::test_rack::*;
    use crate::ui::poly_edit::move_to;
    use acid_engine::project::Project;

    fn fm_app() -> SidAcidApp {
        SidAcidApp::headless(Document::new(Project::default()))
    }

    /// (on, machine uid, pitch); the FM machine is uid 5.
    fn cmd(c: &Command) -> (bool, u16, u8) {
        match c {
            Command::NoteOn { uid, pitch, .. } => (true, uid.0, *pitch),
            Command::NoteOff { uid, pitch } => (false, uid.0, *pitch),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn audition_pairs_note_on_with_note_off() {
        let mut app = fm_app();
        app.audition(FM, Some(60));
        app.audition(FM, Some(62));
        app.audition(FM, None);
        let seen: Vec<_> = app.pending.iter().map(cmd).collect();
        assert_eq!(
            seen,
            [(true, 5, 60), (false, 5, 60), (true, 5, 62), (false, 5, 62)]
        );
        assert_eq!(app.view.audition, None);
    }

    #[test]
    fn releasing_another_machine_sends_nothing() {
        let mut app = fm_app();
        app.audition(FM, Some(60));
        app.pending.clear();
        app.audition(SAMPLER, None);
        assert!(app.pending.is_empty());
        assert_eq!(app.view.audition, Some((FM, 60)));
    }

    #[test]
    fn a_stale_audition_and_drag_are_released_once_the_pointer_is_up() {
        let mut app = fm_app();
        app.audition(FM, Some(60));
        app.view.panel(FM).poly_drag = Some(PolyDrag::Resize { index: 0 });
        app.pending.clear();
        app.release_stale_poly_input(true);
        assert_eq!(app.view.audition, Some((FM, 60)), "pointer still down");
        assert!(app.view.panel(FM).poly_drag.is_some());
        app.release_stale_poly_input(false);
        assert_eq!(app.view.audition, None);
        assert!(app.view.panels.values().all(|p| p.poly_drag.is_none()));
        assert_eq!(
            app.pending.iter().map(cmd).collect::<Vec<_>>(),
            [(false, 5, 60)]
        );
    }

    #[test]
    fn a_collapsed_panel_frame_releases_a_held_key() {
        let mut app = fm_app();
        app.audition(FM, Some(60));
        app.view.panel(FM).poly_drag = Some(PolyDrag::Resize { index: 0 });
        app.doc.apply(Edit::Bpm(100.0), true);
        // The FM panel collapses: its roll isn't drawn, so only the
        // frame-level check can see that the pointer is up.
        app.view.force_open.push(("fm", false));
        crate::ui::run_frames(&mut app, 1);
        assert_eq!(app.view.audition, None);
        assert!(app.view.panels.values().all(|p| p.poly_drag.is_none()));
        let offs: Vec<_> = app
            .flushed
            .iter()
            .filter(|c| matches!(c, Command::NoteOff { .. }))
            .map(cmd)
            .collect();
        assert_eq!(offs, [(false, 5, 60)], "the held key's NoteOff is sent");
        // The drag's gesture is closed: the next merge is a new undo step.
        app.doc.apply(Edit::Bpm(110.0), true);
        app.doc.undo();
        assert_eq!(app.doc.project.bpm, 100.0);
    }

    #[test]
    fn dropping_a_move_onto_a_same_pitch_note_is_one_undo_step() {
        let mut app = fm_app();
        let mut p = app.doc.project.fm(FM).unwrap().patterns[0].clone();
        add(&mut p, 0, 4, 60, 100);
        let i = add(&mut p, 8, 4, 60, 50).unwrap();
        app.commit_poly(FM, 0, Some(p.clone()), false, true);
        let before = app.doc.project.fm(FM).unwrap().patterns[0].clone();
        // The drag: move onto the first note (a live drag merges).
        app.view.panel(FM).poly_drag = Some(PolyDrag::Move { index: i, grab: 0 });
        move_to(&mut p, i, 2, 60);
        app.commit_poly_frame(FM, 0, p.clone(), true, None, false);
        // The drop frame: the drag state is already cleared.
        app.view.panel(FM).poly_drag = None;
        app.commit_poly_frame(FM, 0, p, false, Some(i), true);
        assert_eq!(
            app.doc.project.fm(FM).unwrap().patterns[0].notes.len(),
            1,
            "merged"
        );
        app.doc.undo();
        assert_eq!(
            app.doc.project.fm(FM).unwrap().patterns[0],
            before,
            "one undo"
        );
    }

    #[test]
    fn poly_drag_is_one_undo_step() {
        let mut app = fm_app();
        let mut p = app.doc.project.fm(FM).unwrap().patterns[0].clone();
        add(&mut p, 0, 4, 60, 100);
        app.commit_poly(FM, 0, Some(p.clone()), false, true);
        let before = app.doc.project.fm(FM).unwrap().patterns[0].clone();
        assert_eq!(before, p);
        for start in [4, 8, 12] {
            move_to(&mut p, 0, start, 62);
            app.commit_poly(FM, 0, Some(p.clone()), true, false);
        }
        app.commit_poly(FM, 0, None, false, true);
        assert_eq!(
            app.doc.project.fm(FM).unwrap().patterns[0].notes[0].start,
            12
        );
        app.doc.undo();
        assert_eq!(
            app.doc.project.fm(FM).unwrap().patterns[0],
            before,
            "one undo"
        );
    }
}
