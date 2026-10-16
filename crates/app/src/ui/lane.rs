//! A lane: one machine's pattern as a piano roll, drawn at a given width
//! wherever the caller puts it. A header row (collapse arrow, name,
//! pattern, strip toggle), the grid (one column per tick, scrolled and
//! zoomed with the dock; one row per MIDI pitch, or per instrument on a
//! drum kit), an optional expression strip and a height handle.
//!
//! Click adds a note (with the last length used; a drum hit is one snap
//! long), drag the body to move, drag the right edge to stretch (not on
//! drum lanes), right-click deletes, chords stack freely. The acid synths
//! keep one note at a time and get accent and slide lanes instead of
//! velocity; drum lanes get an accent lane. Clicking the key strip of an
//! FM, Mono or sampler lane auditions the pitch.

use super::key_entry::{Cursor, LaneKind};
use super::lane_geom::{LaneGeom, Rows, ZOOMS, tick_w};
use super::poly_edit::{
    Flag, PolyDrag, add, delete, make_mono, move_to, note_at, resize, resolve_overlaps,
    set_velocity_in, toggle_flag,
};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::machine::MachineUid;
use acid_engine::params::DRUM_INSTRUMENTS;
use acid_engine::project::{
    BASE_MIDI, LaneView, MachineBody, NotePattern, PatternRef, TICKS_PER_STEP,
};
use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, FontId, Layout, Rect, RichText, Sense, Shape, Stroke,
    StrokeKind, UiBuilder, Vec2, pos2, vec2,
};

pub(crate) const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// Drum row labels, instrument 0 first.
pub(crate) const INST_NAMES: [&str; 8] = ["BD", "SD", "LT", "HT", "CP", "RS", "CH", "OH"];

/// Width of the key strip left of the grid.
pub(crate) const KEY_W: f32 = 30.0;
/// The sampler's key strip is wider to fit row labels like "Amen ·05".
pub(crate) const SAMPLER_KEY_W: f32 = 64.0;
/// Height of one pitch row.
pub(crate) const ROW_H: f32 = 9.0;
/// Height of one drum row: 8 of them fill a default lane.
pub(crate) const DRUM_ROW_H: f32 = 14.0;
pub(crate) const VEL_H: f32 = 32.0;
pub(crate) const FLAG_H: f32 = 12.0;
pub(crate) const HEADER_H: f32 = 20.0;
/// Height of the drag handle under a lane.
const HANDLE_H: f32 = 4.0;
/// Every MIDI pitch.
const PITCH_ROWS: usize = 128;
const NEW_VEL: u8 = 100;
/// Pitch the FM roll first scrolls to: C3 (MIDI 48).
const FM_START_PITCH: u8 = 48;
/// Pitch the Mono roll first scrolls to: C2 (MIDI 36), as it is mostly bass.
const MONO_START_PITCH: u8 = 36;
const BLACK_KEYS: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

fn vel_from_y(y: f32, top: f32) -> u8 {
    ((1.0 - (y - top) / VEL_H) * 127.0)
        .round()
        .clamp(1.0, 127.0) as u8
}

/// One frame's edits to a lane's pattern.
#[derive(Default)]
struct Frame {
    changed: bool,
    finished: bool,
    dropped: Option<usize>,
}

impl SidAcidApp {
    /// Pitch the roll of machine `uid` first centres on: for a sampler the
    /// lowest loaded zone's base key, or C1 with none; C2 for an acid synth
    /// or Mono; C3 for FM.
    pub(crate) fn roll_start_pitch(&self, uid: MachineUid) -> u8 {
        if self.doc.project.synth(uid).is_some() {
            return BASE_MIDI as u8;
        }
        if self.doc.project.mono(uid).is_some() {
            return MONO_START_PITCH;
        }
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

    /// How `uid`'s lane edits: acid synths, drum kits, or freely (the rest).
    pub(crate) fn lane_kind(&self, uid: MachineUid) -> LaneKind {
        let p = &self.doc.project;
        if p.synth(uid).is_some() {
            LaneKind::Acid
        } else if p.kit(uid).is_some() {
            LaneKind::Drums
        } else {
            LaneKind::Poly
        }
    }

    /// The key-strip width every lane uses, so the grids line up: wide
    /// enough for sampler row labels while any sampler is in the rack.
    pub(crate) fn lane_key_w(&self) -> f32 {
        let sampler = self
            .doc
            .project
            .machines
            .iter()
            .any(|m| matches!(m.body, MachineBody::Sampler(_)));
        if sampler { SAMPLER_KEY_W } else { KEY_W }
    }

    /// Draws machine `uid`'s lane `width` wide at the `ui` cursor: the
    /// header row, then (unless collapsed) the grid, the expression strip
    /// (if shown) and the height handle. Edits commit like the roll's.
    pub(crate) fn lane(&mut self, ui: &mut egui::Ui, uid: MachineUid, width: f32) {
        let Some(view) = self.doc.project.machine(uid).map(|m| m.lane) else {
            return;
        };
        let slot = self.selected(uid);
        let Some(pattern) = self.doc.project.roll(uid, slot).cloned() else {
            return;
        };
        let spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        let origin = ui.cursor().min;
        self.lane_header(ui, uid, slot, view, width);
        if !view.collapsed {
            self.lane_body(ui, uid, slot, pattern, view, width);
        }
        if self.focused() == Some(uid) {
            let outline = Rect::from_min_max(origin, pos2(origin.x + width, ui.cursor().top()));
            ui.painter().rect_stroke(
                outline,
                0.0,
                Stroke::new(1.0_f32, theme::ACID),
                StrokeKind::Inside,
            );
        }
        ui.spacing_mut().item_spacing.y = spacing;
    }

    /// The lane's header row: collapse arrow, name, pattern, strip toggle.
    /// The toggles change the machine's `LaneView` directly (not undoable).
    fn lane_header(
        &mut self,
        ui: &mut egui::Ui,
        uid: MachineUid,
        slot: u8,
        view: LaneView,
        width: f32,
    ) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, HEADER_H), Sense::hover());
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let (arrow, aresp) = row.allocate_exact_size(vec2(14.0, HEADER_H), Sense::click());
        let c = arrow.center();
        let points = if view.collapsed {
            vec![
                pos2(c.x - 3.0, c.y - 4.0),
                pos2(c.x + 4.0, c.y),
                pos2(c.x - 3.0, c.y + 4.0),
            ]
        } else {
            vec![
                pos2(c.x - 4.0, c.y - 3.0),
                pos2(c.x + 4.0, c.y - 3.0),
                pos2(c.x, c.y + 4.0),
            ]
        };
        let colour = if aresp.hovered() {
            row.visuals().strong_text_color()
        } else {
            row.visuals().text_color()
        };
        row.painter()
            .add(Shape::convex_polygon(points, colour, Stroke::NONE));
        let collapse = aresp
            .on_hover_text(if view.collapsed {
                "Show this lane"
            } else {
                "Fold this lane to its header"
            })
            .clicked();
        row.label(RichText::new(self.panel_title(uid)).strong());
        row.label(RichText::new(PatternRef(slot).label()).monospace().weak());
        let strip = row
            .with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.selectable_label(view.strip, RichText::new("Strip").small())
                    .on_hover_text("Show the velocity / accent strip")
                    .clicked()
            })
            .inner;
        if let Some(m) = self.doc.project.machine_mut(uid) {
            m.lane.collapsed ^= collapse;
            m.lane.strip ^= strip;
        }
    }

    /// The grid, the expression strip and the height handle.
    fn lane_body(
        &mut self,
        ui: &mut egui::Ui,
        uid: MachineUid,
        slot: u8,
        mut pattern: NotePattern,
        view: LaneView,
        width: f32,
    ) {
        let kind = self.lane_kind(uid);
        let drums = kind == LaneKind::Drums;
        let acid = kind == LaneKind::Acid;
        let key_w = self.lane_key_w();
        let height = view.clamped_height() as f32;
        let ticks = pattern.ticks();
        let snap = self.view.dock.snap;
        let tw = tick_w(ZOOMS[self.view.dock.zoom.min(ZOOMS.len() - 1)]);
        let new_len = match self.view.panel(uid).poly_len {
            0 => TICKS_PER_STEP,
            l => l,
        };
        let playhead = self.playhead(uid, slot);
        // Drum rows shrink to fit a short lane, so all 8 always show.
        let (rows, row_h) = if drums {
            let fit = height / DRUM_INSTRUMENTS as f32;
            (Rows::Drums, DRUM_ROW_H.min(fit))
        } else {
            (Rows::Pitches, ROW_H)
        };
        let max_scroll_y = (PITCH_ROWS as f32 * ROW_H - height).max(0.0);
        if !drums && self.view.panel(uid).lane_scroll_y.is_none() {
            let pitch = self.roll_start_pitch(uid).min(127);
            let y = ((127 - pitch) as f32 * ROW_H - height / 2.0).clamp(0.0, max_scroll_y);
            self.view.panel(uid).lane_scroll_y = Some(y);
        }
        let mut frame = Frame::default();
        let mut key_down: Option<u8> = None;

        let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
        if resp.hovered() {
            self.lane_wheel(ui, uid, !drums, max_scroll_y);
        }
        let scroll_y = if drums {
            0.0
        } else {
            self.view.panel(uid).lane_scroll_y.unwrap_or(0.0)
        };
        let geom = LaneGeom {
            left: rect.left() + key_w,
            top: rect.top(),
            tick_w: tw,
            row_h,
            scroll_x: self.view.dock.scroll_x,
            scroll_y,
            ticks,
            rows,
        };
        let grid_rect = Rect::from_min_max(pos2(geom.left, rect.top()), rect.max);
        let key_rect = Rect::from_min_max(rect.min, pos2(geom.left, rect.bottom()));
        let grid = ui.painter_at(grid_rect);
        let keys = ui.painter_at(key_rect);
        grid.rect_filled(grid_rect, 0.0, theme::GRID_DARK);
        keys.rect_filled(key_rect, 0.0, theme::GRID_DARK);

        // Rows and keys, only those in view.
        let held = self.view.audition.filter(|a| a.0 == uid).map(|a| a.1);
        let first = (scroll_y / row_h).floor().max(0.0) as usize;
        let last = (((scroll_y + height) / row_h).ceil().max(0.0) as usize).min(geom.row_count());
        for r in first..last {
            let Some(pitch) = geom.pitch_of_row(r) else {
                continue;
            };
            let y0 = geom.row_top(r);
            let row = Rect::from_min_max(pos2(geom.left, y0), pos2(rect.right(), y0 + row_h));
            let key = Rect::from_min_max(
                pos2(rect.left(), y0),
                pos2(geom.left - 1.0, y0 + row_h - 1.0),
            );
            if drums {
                let shade = if r % 2 == 0 {
                    theme::GRID_BG
                } else {
                    theme::GRID_DARK
                };
                grid.rect_filled(row, 0.0, shade);
                keys.rect_filled(key, 1.0, Color32::from_gray(190));
                keys.text(
                    pos2(rect.left() + 2.0, y0 + row_h / 2.0),
                    Align2::LEFT_CENTER,
                    INST_NAMES[pitch as usize],
                    FontId::monospace((row_h - 2.0).clamp(6.0, 9.0)),
                    Color32::BLACK,
                );
                continue;
            }
            let black = BLACK_KEYS[pitch as usize % 12];
            grid.rect_filled(
                row,
                0.0,
                if black {
                    theme::GRID_DARK
                } else {
                    theme::GRID_BG
                },
            );
            let sampler_row = self
                .doc
                .project
                .sampler(uid)
                .and_then(|s| super::sampler_ui::row_label(&s.slots, pitch));
            if let Some((slot, _)) = &sampler_row {
                grid.rect_filled(
                    row,
                    0.0,
                    super::sampler_ui::SLOT_TINTS[slot % 4].gamma_multiply(0.18),
                );
            }
            let key_colour = if held == Some(pitch) {
                theme::ACID
            } else if black {
                theme::GRID_DARK
            } else {
                Color32::from_gray(190)
            };
            keys.rect_filled(key, 1.0, key_colour);
            if let Some((_, text)) = &sampler_row {
                keys.text(
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
                keys.text(
                    pos2(rect.left() + 2.0, y0 + ROW_H / 2.0),
                    Align2::LEFT_CENTER,
                    format!("C{}", pitch as i32 / 12 - 1),
                    FontId::monospace(8.0),
                    Color32::BLACK,
                );
            }
        }

        // Step lines, the area past the pattern end, the playhead.
        for step in 0..=ticks / TICKS_PER_STEP {
            let x = geom.x_of(step * TICKS_PER_STEP);
            if x < grid_rect.left() || x > grid_rect.right() {
                continue;
            }
            let colour = if step % 4 == 0 {
                theme::KNOB_TRACK
            } else {
                theme::GRID_LINE
            };
            grid.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0_f32, colour),
            );
        }
        let end_x = geom.x_of(ticks).max(geom.left);
        if end_x < rect.right() {
            grid.rect_filled(
                Rect::from_min_max(pos2(end_x, rect.top()), rect.max),
                0.0,
                theme::GRID_DARK.gamma_multiply(0.85),
            );
        }
        if let Some(step) = playhead {
            let x = geom.x_of(step as u16 * TICKS_PER_STEP);
            grid.rect_filled(
                Rect::from_min_size(
                    pos2(x, rect.top()),
                    vec2(TICKS_PER_STEP as f32 * tw, height),
                ),
                0.0,
                theme::ACID.gamma_multiply(0.15),
            );
        }

        // Notes. A drum note whose pitch is not an instrument has no row.
        let drawn = |n: &acid_engine::project::Note| {
            n.start < ticks && (!drums || (n.pitch as usize) < DRUM_INSTRUMENTS)
        };
        for note in pattern.notes.iter().filter(|n| drawn(n)) {
            let r = geom.note_rect(note).shrink(0.5);
            let fill = if drums {
                if note.accent {
                    theme::ACCENT
                } else {
                    theme::ACID
                }
            } else {
                theme::ACID.gamma_multiply(0.35 + 0.65 * note.vel as f32 / 127.0)
            };
            grid.rect_filled(r, 2.0, fill);
            if acid
                && note.slide
                && let Some(next) = pattern
                    .notes
                    .iter()
                    .find(|m| m.start < ticks && m.start == note.start.saturating_add(note.len))
            {
                let nr = geom.note_rect(next).shrink(0.5);
                grid.line_segment(
                    [
                        pos2(r.right(), r.center().y),
                        pos2(nr.left(), nr.center().y),
                    ],
                    Stroke::new(2.0_f32, theme::ACCENT),
                );
            }
        }

        // The keyboard-entry cursor, one snap wide.
        if self.view.dock.keyboard && self.focused() == Some(uid) {
            let c = self.view.panel(uid).cursor;
            if let Some(r) = geom.row_of(c.pitch) {
                let cell = Rect::from_min_size(
                    pos2(geom.x_of(c.tick), geom.row_top(r)),
                    vec2(snap.ticks() as f32 * tw, row_h),
                );
                grid.rect_stroke(
                    cell,
                    0.0,
                    Stroke::new(2.0_f32, Color32::WHITE),
                    StrokeKind::Inside,
                );
            }
        }

        // Mouse editing.
        let pointer = resp.interact_pointer_pos();
        let cell_at = |p: egui::Pos2| Some((geom.tick_at(p.x)?, geom.pitch_at(p.y)?));
        if resp.is_pointer_button_down_on()
            && let Some(p) = pointer
            && p.x < geom.left
        {
            key_down = geom.pitch_at(p.y).filter(|_| kind == LaneKind::Poly);
        }
        if (resp.clicked() || resp.secondary_clicked() || resp.drag_started())
            && let Some(p) = pointer
            && p.x >= geom.left
        {
            self.view.focus = Some(uid);
            self.view.dock.keyboard = true;
            self.view.panel(uid).cursor = Cursor {
                tick: snap.floor(geom.tick_at_clamped(p.x)),
                pitch: geom.pitch_at_clamped(p.y),
            };
        }
        if resp.clicked()
            && let Some(p) = pointer
            && let Some((tick, pitch)) = cell_at(p)
            && note_at(&pattern, tick, pitch).is_none()
        {
            let len = if drums { snap.ticks() } else { new_len };
            let added = add(&mut pattern, snap.floor(tick), len, pitch, NEW_VEL);
            if acid && let Some(i) = added {
                make_mono(&mut pattern, i);
            }
            frame.changed |= added.is_some();
            frame.finished = true;
        }
        if resp.secondary_clicked()
            && let Some(p) = pointer
            && let Some((tick, pitch)) = cell_at(p)
            && let Some(i) = note_at(&pattern, tick, pitch)
        {
            frame.changed |= delete(&mut pattern, i);
            frame.finished = true;
        }
        if resp.drag_started()
            && let Some(p) = pointer
            && let Some((tick, pitch)) = cell_at(p)
        {
            self.view.panel(uid).poly_drag = match note_at(&pattern, tick, pitch) {
                Some(i) if !drums && geom.near_right_edge(&pattern.notes[i], p.x) => {
                    Some(PolyDrag::Resize { index: i })
                }
                Some(i) => Some(PolyDrag::Move {
                    index: i,
                    grab: tick - pattern.notes[i].start,
                }),
                None => {
                    let added = add(&mut pattern, snap.floor(tick), snap.ticks(), pitch, NEW_VEL);
                    frame.changed |= added.is_some();
                    added.map(|index| {
                        if drums {
                            // A new hit follows the pointer.
                            PolyDrag::Move { index, grab: 0 }
                        } else if acid {
                            PolyDrag::Resize {
                                index: make_mono(&mut pattern, index),
                            }
                        } else {
                            PolyDrag::Resize { index }
                        }
                    })
                }
            };
        }
        if resp.dragged()
            && let Some(p) = pointer
            && let Some(drag) = self.view.panel(uid).poly_drag
        {
            let (tick, pitch) = (geom.tick_at_clamped(p.x), geom.pitch_at_clamped(p.y));
            match drag {
                PolyDrag::Resize { index } => {
                    if let Some(start) = pattern.notes.get(index).map(|n| n.start) {
                        let len = (snap.floor(tick) + snap.ticks())
                            .saturating_sub(start)
                            .max(1);
                        if resize(&mut pattern, index, len) {
                            self.view.panel(uid).poly_len = len;
                            frame.changed = true;
                        }
                    }
                }
                PolyDrag::Move { index, grab } => {
                    let start = snap.floor(tick.saturating_sub(grab));
                    frame.changed |= move_to(&mut pattern, index, start, pitch);
                }
                PolyDrag::Velocity => {}
            }
        }
        let drag = self.view.panel(uid).poly_drag;
        if resp.drag_stopped() && drag != Some(PolyDrag::Velocity) {
            if let Some(PolyDrag::Move { index, .. } | PolyDrag::Resize { index }) = drag {
                frame.dropped = Some(index);
            }
            self.view.panel(uid).poly_drag = None;
            frame.finished = true;
        }

        if view.strip {
            match kind {
                LaneKind::Poly => {
                    self.velocity_strip(ui, uid, &geom, width, &mut pattern, &mut frame)
                }
                LaneKind::Acid => {
                    for (label, flag) in [("Acc", Flag::Accent), ("Slide", Flag::Slide)] {
                        flag_strip(ui, &geom, width, &mut pattern, label, flag, &mut frame);
                    }
                }
                LaneKind::Drums => {
                    let (label, flag) = ("Acc", Flag::Accent);
                    flag_strip(ui, &geom, width, &mut pattern, label, flag, &mut frame);
                }
            }
        }
        self.lane_handle(ui, uid, height, width);

        self.audition(uid, key_down);
        self.commit_poly_frame(
            uid,
            slot,
            pattern,
            frame.changed,
            frame.dropped,
            frame.finished,
        );
    }

    /// The mouse wheel over a lane's grid: vertical scrolls the pitches of
    /// a melodic lane, horizontal (or Shift+vertical) the dock. The wheel
    /// is consumed so the area the lane sits in does not scroll too.
    fn lane_wheel(&mut self, ui: &egui::Ui, uid: MachineUid, melodic: bool, max_y: f32) {
        let (delta, shift) = ui.input(|i| (i.raw_scroll_delta, i.modifiers.shift));
        let (mut dx, mut dy) = (delta.x, delta.y);
        if shift && dx == 0.0 {
            (dx, dy) = (dy, 0.0);
        }
        if dx != 0.0 {
            self.view.dock.scroll_x = (self.view.dock.scroll_x - dx).max(0.0);
        }
        if dy != 0.0 && melodic {
            let y = self.view.panel(uid).lane_scroll_y.get_or_insert(0.0);
            *y = (*y - dy).clamp(0.0, max_y);
        }
        ui.input_mut(|i| {
            i.raw_scroll_delta = Vec2::ZERO;
            i.smooth_scroll_delta = Vec2::ZERO;
        });
    }

    /// The velocity strip of a poly lane: click or drag sets the velocity
    /// of the notes starting in the snap cell under the pointer.
    fn velocity_strip(
        &mut self,
        ui: &mut egui::Ui,
        uid: MachineUid,
        geom: &LaneGeom,
        width: f32,
        pattern: &mut NotePattern,
        frame: &mut Frame,
    ) {
        let snap = self.view.dock.snap;
        let (lane, lresp) = strip_row(ui, geom, width, VEL_H, Sense::click_and_drag(), "Vel");
        let painter = ui.painter_at(lane);
        for note in pattern.notes.iter().filter(|n| n.start < geom.ticks) {
            let x = geom.x_of(note.start);
            let h = VEL_H * note.vel as f32 / 127.0;
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(x, lane.bottom() - h),
                    pos2(x + (geom.tick_w - 1.0).max(1.0), lane.bottom()),
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
            && p.x >= geom.left
        {
            let from = snap.floor(geom.tick_at_clamped(p.x));
            frame.changed |= set_velocity_in(
                pattern,
                from,
                from + snap.ticks(),
                vel_from_y(p.y, lane.top()),
            );
            frame.finished |= lresp.clicked();
        }
        if lresp.drag_stopped() {
            self.view.panel(uid).poly_drag = None;
            frame.finished = true;
        }
    }

    /// The strip under a lane that drags its height.
    fn lane_handle(&mut self, ui: &mut egui::Ui, uid: MachineUid, height: f32, width: f32) {
        let (handle, resp) = ui.allocate_exact_size(vec2(width, HANDLE_H), Sense::drag());
        let resp = resp.on_hover_cursor(CursorIcon::ResizeVertical);
        let active = resp.hovered() || resp.dragged();
        ui.painter().rect_filled(
            handle,
            0.0,
            if active {
                theme::KNOB_TRACK
            } else {
                theme::GRID_LINE
            },
        );
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
            let h = (height + p.y - handle.center().y).round();
            let h = h.clamp(LaneView::MIN_HEIGHT as f32, LaneView::MAX_HEIGHT as f32);
            if let Some(m) = self.doc.project.machine_mut(uid) {
                m.lane.height = h as u16;
            }
        }
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
            if self.doc.project.synth(uid).is_some() {
                make_mono(&mut pattern, index);
            } else {
                resolve_overlaps(&mut pattern, index);
            }
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

/// Allocates one expression-strip row `width` wide and `h` tall under
/// the grid, with its label in the key strip. Returns the row's grid part
/// (right of the key strip) and its response.
fn strip_row(
    ui: &mut egui::Ui,
    geom: &LaneGeom,
    width: f32,
    h: f32,
    sense: Sense,
    label: &str,
) -> (Rect, egui::Response) {
    let (row, resp) = ui.allocate_exact_size(vec2(width, h), sense);
    let grid = Rect::from_min_max(pos2(geom.left, row.top()), row.max);
    let painter = ui.painter_at(row);
    painter.rect_filled(grid, 0.0, theme::GRID_DARK);
    painter.text(
        pos2(row.left() + 2.0, row.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::monospace(9.0),
        ui.visuals().text_color(),
    );
    (grid, resp)
}

/// An accent or slide row: marks the flagged notes, a click toggles the
/// flag of the note sounding under the pointer.
fn flag_strip(
    ui: &mut egui::Ui,
    geom: &LaneGeom,
    width: f32,
    pattern: &mut NotePattern,
    label: &str,
    flag: Flag,
    frame: &mut Frame,
) {
    let (lane, lresp) = strip_row(ui, geom, width, FLAG_H, Sense::click(), label);
    let painter = ui.painter_at(lane);
    for note in pattern.notes.iter().filter(|n| n.start < geom.ticks) {
        let (on, tick, colour) = match flag {
            Flag::Accent => (note.accent, note.start, theme::ACCENT),
            Flag::Slide => (
                note.slide,
                note.start.saturating_add(note.len).saturating_sub(1),
                theme::ACID,
            ),
        };
        if on {
            let x = geom.x_of(tick);
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(x, lane.top() + 1.0),
                    pos2(x + geom.tick_w.max(2.0), lane.bottom() - 1.0),
                ),
                0.0,
                colour,
            );
        }
    }
    if lresp.clicked()
        && let Some(p) = lresp.interact_pointer_pos()
        && p.x >= geom.left
    {
        frame.changed |= toggle_flag(pattern, geom.tick_at_clamped(p.x), flag);
        frame.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::test_rack::*;
    use crate::ui::key_entry::LaneKind;
    use crate::ui::poly_edit::move_to;
    use acid_engine::project::Project;

    #[test]
    fn every_machine_kind_has_a_lane_kind() {
        let app = SidAcidApp::headless(Document::new(Project::default()));
        assert_eq!(app.lane_kind(SYNTH1), LaneKind::Acid);
        assert_eq!(app.lane_kind(KIT808), LaneKind::Drums);
        assert_eq!(app.lane_kind(FM), LaneKind::Poly);
        assert_eq!(app.lane_kind(SAMPLER), LaneKind::Poly);
    }

    #[test]
    fn lanes_draw_headless_for_every_machine() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(acid_engine::machine::MachineType::Mono);
        // Drawing each machine's lane on its own must not panic, and the
        // melodic lanes place their pitch scroll on first draw.
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for uid in app.doc.project.rack_uids() {
                    app.lane(ui, uid, 800.0);
                }
            });
        });
        for uid in app.doc.project.rack_uids() {
            let melodic = app.lane_kind(uid) != LaneKind::Drums;
            assert_eq!(app.view.panel(uid).lane_scroll_y.is_some(), melodic);
        }
    }

    #[test]
    fn a_collapsed_lane_draws_only_its_header() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.doc.project.machine_mut(FM).unwrap().lane.collapsed = true;
        let ctx = egui::Context::default();
        let mut used = 0.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let top = ui.cursor().top();
                app.lane(ui, FM, 800.0);
                used = ui.cursor().top() - top;
            });
        });
        assert!(used < 40.0, "only the header row: {used}");
    }

    #[test]
    fn the_acid_synth_roll_opens_on_c2() {
        let app = SidAcidApp::headless(Document::new(Project::default()));
        assert_eq!(app.roll_start_pitch(SYNTH1), 36);
    }

    #[test]
    fn the_mono_roll_opens_on_c2() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(acid_engine::machine::MachineType::Mono);
        let uid = app.doc.project.machines.last().unwrap().uid;
        assert_eq!(app.doc.project.mono(uid).map(|_| ()), Some(()));
        assert_eq!(app.roll_start_pitch(uid), 36);
    }

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
