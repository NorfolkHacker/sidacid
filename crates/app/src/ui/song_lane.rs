//! A song lane: one machine's blocks of the song, drawn by the dock in
//! Song mode under the same header and height handle as its note lane.
//! Each block is a rounded box labelled with its pattern, with a small
//! picture of the pattern's notes inside (looped repeats of a fixed
//! length marked by thin dividers); muted blocks are dimmed.
//!
//! Blocks are edited as a selection, as notes are in Pattern mode: click
//! a block to select it (Shift toggles), drag empty space to box-select
//! (Shift adds), drag a block to move the selection by whole bars (Alt:
//! single steps; Ctrl moves copies), drag a block's right edge to fix the
//! selection's lengths; right-click deletes the selection, or the block
//! under the pointer when it is not selected. Double-click empty space to
//! place the machine's selected pattern at that bar, or a block to open
//! its pattern in Pattern mode. Blocks never overlap: a placed block that
//! lands on taken steps goes to the nearest free spot, and a moved or
//! copied selection goes together to the nearest offset where it fits.

use super::lane_geom::{ZOOMS, tick_w};
use super::song_ops::{self, BAR};
use super::{SidAcidApp, theme};
use acid_engine::machine::MachineUid;
use acid_engine::params::DRUM_INSTRUMENTS;
use acid_engine::project::{Block, LaneView, PatternRef, TICKS_PER_STEP};
use acid_engine::sequencer::Mode;
use acid_engine::song::{self, Lens, MAX_SONG_STEPS};
use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Rect, Sense, Stroke, StrokeKind, pos2, vec2,
};

/// How close to a block's right edge a press resizes it, in pixels.
const EDGE_PX: f32 = 5.0;
/// Room for the pattern label along a block's top.
const LABEL_H: f32 = 13.0;
/// Gap above and below the blocks inside the lane.
const PAD_Y: f32 = 3.0;

/// A song lane's mouse drag, recomputed each frame from where it began.
pub enum SongDrag {
    /// The blocks at `idx` of `origin` (the lane at the press) follow the
    /// pointer from step `grab`; with `copy` their copies do.
    Move {
        origin: Vec<Block>,
        idx: Vec<usize>,
        grab: u32,
        copy: bool,
    },
    /// The blocks at `idx` of `origin` stretch with the grabbed block's
    /// end (`grab`).
    Resize {
        origin: Vec<Block>,
        idx: Vec<usize>,
        grab: u32,
    },
    /// A selection box from step `from`; `add` keeps the old selection.
    Marquee { from: u32, add: bool },
}

/// Width of one step in Song mode at dock zoom index `zoom`.
pub(crate) fn step_w(zoom: usize) -> f32 {
    tick_w(ZOOMS[zoom.min(ZOOMS.len() - 1)]) * TICKS_PER_STEP as f32
}

/// Steps to x with the dock's horizontal scroll.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SongGeom {
    pub left: f32,
    pub step_w: f32,
    pub scroll_x: f32,
}

impl SongGeom {
    pub fn x_of(&self, step: u32) -> f32 {
        self.left + step as f32 * self.step_w - self.scroll_x
    }

    /// x of a tick from the song start (a fraction of a step).
    fn x_of_tick(&self, tick: u32) -> f32 {
        self.left + tick as f32 * self.step_w / TICKS_PER_STEP as f32 - self.scroll_x
    }

    /// The step under `x`, held to the song (left of the grid is step 0).
    pub fn step_at(&self, x: f32) -> u32 {
        let s = ((x - self.left + self.scroll_x) / self.step_w)
            .floor()
            .max(0.0);
        (s as u32).min(MAX_SONG_STEPS as u32 - 1)
    }

    /// The steps in view between `left` and `right`.
    fn visible(&self, right: f32) -> (u32, u32) {
        (self.step_at(self.left), self.step_at(right) + 1)
    }
}

/// `step` floored to its bar, unless `fine`.
pub(crate) fn snap_bar(step: u32, fine: bool) -> u32 {
    if fine { step } else { step / BAR * BAR }
}

/// The whole bars a ruler drag from step `a` to step `b` picks (either
/// order): from the first one's start to the last one's end, held to the
/// song limit (`MAX_SONG_STEPS` is a whole number of bars).
pub(crate) fn bar_range(a: u32, b: u32) -> (u16, u16) {
    let last = MAX_SONG_STEPS as u32 - BAR;
    let lo = snap_bar(a.min(b), false).min(last);
    let hi = snap_bar(a.max(b), false).min(last) + BAR;
    (lo as u16, hi as u16)
}

impl SidAcidApp {
    /// Runs `f` with the project's pattern lengths.
    pub(crate) fn with_lens<R>(&self, f: impl FnOnce(Lens<'_>) -> R) -> R {
        let lens = song::lens_of(&self.doc.project);
        f(&lens)
    }

    /// Indices of `uid`'s selected blocks in its song lane.
    pub(crate) fn block_selection(&self, uid: MachineUid) -> Vec<usize> {
        let sel = self.view.panels.get(&uid).map_or(&[][..], |p| &p.song_sel);
        song_ops::resolve(self.doc.project.song.lane(uid), sel)
    }

    /// Makes the blocks at `idx` of `lane` `uid`'s block selection.
    pub(crate) fn set_block_selection(&mut self, uid: MachineUid, lane: &[Block], idx: &[usize]) {
        self.view.panel(uid).song_sel = song_ops::values(lane, idx);
    }

    /// Makes `lane` `uid`'s song lane: one `Edit::Song` (merged into an
    /// open drag's step with `merge`; else the step is closed). A lane
    /// equal to the current one changes nothing.
    pub(crate) fn commit_song_lane(&mut self, uid: MachineUid, lane: Vec<Block>, merge: bool) {
        if lane != self.doc.project.song.lane(uid) {
            let mut s = self.doc.project.song.clone();
            *s.lane_mut(uid) = lane;
            self.apply_song(s, merge);
        }
        if !merge {
            self.doc.end_gesture();
        }
    }

    /// Sets the song cursor; while stopped the engine starts from there.
    pub(crate) fn set_song_cursor(&mut self, step: u32) {
        self.view.song_cursor = step;
        if !self.status.playing {
            self.send(vec![acid_engine::host::Command::SetSongStart(step)]);
        }
    }

    /// Draws machine `uid`'s song lane `width` wide at the `ui` cursor:
    /// the header row, then (unless collapsed) its blocks and the height
    /// handle.
    pub(crate) fn song_lane(&mut self, ui: &mut egui::Ui, uid: MachineUid, width: f32) {
        let Some(view) = self.doc.project.machine(uid).map(|m| m.lane) else {
            return;
        };
        let slot = self.selected(uid);
        let spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        let origin = ui.cursor().min;
        self.lane_header(ui, uid, slot, view, width, false);
        if !view.collapsed {
            self.song_lane_body(ui, uid, view, width);
        }
        self.lane_outline(ui, uid, origin, width);
        ui.spacing_mut().item_spacing.y = spacing;
    }

    fn song_lane_body(&mut self, ui: &mut egui::Ui, uid: MachineUid, view: LaneView, width: f32) {
        let height = view.clamped_height() as f32;
        let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
        if resp.hovered() {
            self.lane_wheel(ui, uid, false, 0.0);
        }
        let geom = SongGeom {
            left: rect.left() + self.lane_key_w(),
            step_w: step_w(self.view.dock.zoom),
            scroll_x: self.view.dock.scroll_x,
        };
        let grid_rect = Rect::from_min_max(pos2(geom.left, rect.top()), rect.max);
        let grid = ui.painter_at(grid_rect);
        ui.painter_at(rect).rect_filled(rect, 0.0, theme::GRID_DARK);
        grid.rect_filled(grid_rect, 0.0, theme::GRID_BG);
        let (lo, hi) = geom.visible(grid_rect.right());

        // Bar lines (beats too when they are far enough apart).
        let beat = if geom.step_w * 4.0 >= 8.0 { 4 } else { BAR };
        for step in (lo / beat * beat..=hi).step_by(beat as usize) {
            let colour = if step % BAR == 0 {
                theme::KNOB_TRACK
            } else {
                theme::GRID_LINE
            };
            let x = geom.x_of(step);
            grid.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0_f32, colour),
            );
        }
        if let Some((a, b)) = self.view.dock.range {
            let band = Rect::from_min_max(
                pos2(geom.x_of(a as u32).max(geom.left), rect.top()),
                pos2(geom.x_of(b as u32).min(rect.right()), rect.bottom()),
            );
            if band.width() > 0.0 {
                grid.rect_filled(band, 0.0, theme::ACID.gamma_multiply(0.12));
            }
        }

        let blocks = self.doc.project.song.lane(uid).to_vec();
        let sel = self.block_selection(uid);
        let body = Rect::from_min_max(
            pos2(rect.left(), rect.top() + PAD_Y),
            pos2(rect.right(), rect.bottom() - PAD_Y),
        );
        self.with_lens(|lens| {
            for (i, b) in blocks.iter().enumerate() {
                let end = song::block_end(b, uid, lens);
                if end <= lo || b.start >= hi {
                    continue;
                }
                let selected = sel.binary_search(&i).is_ok();
                self.draw_block(&grid, &geom, uid, b, end, body, selected, lens);
            }
        });

        // The song cursor, and the playhead while playing.
        let cursor_x = geom.x_of(self.view.song_cursor);
        grid.line_segment(
            [pos2(cursor_x, rect.top()), pos2(cursor_x, rect.bottom())],
            Stroke::new(1.0_f32, theme::QUEUED.gamma_multiply(0.6)),
        );
        if self.status.playing {
            let x = geom.x_of(self.status.song_step);
            grid.rect_filled(
                Rect::from_min_size(pos2(x, rect.top()), vec2(geom.step_w, height)),
                0.0,
                theme::ACID.gamma_multiply(0.25),
            );
        }

        if self.song_lane_mouse(ui, uid, &resp, &geom, &blocks) {
            // Switched to Pattern mode: this lane is gone.
            return;
        }
        if let Some(SongDrag::Marquee { from, .. }) = &self.view.panel(uid).song_drag
            && let Some(p) = resp
                .interact_pointer_pos()
                .or_else(|| ui.input(|i| i.pointer.latest_pos()))
        {
            let to = geom.step_at(p.x);
            let r = Rect::from_min_max(
                pos2(geom.x_of((*from).min(to)), rect.top()),
                pos2(geom.x_of((*from).max(to) + 1), rect.bottom()),
            );
            grid.rect_filled(r, 0.0, theme::ACID_LIGHT.gamma_multiply(0.12));
            grid.rect_stroke(
                r,
                0.0,
                Stroke::new(1.0_f32, theme::ACID_LIGHT),
                StrokeKind::Inside,
            );
        }
        self.lane_handle(ui, uid, height, width);
    }

    /// One block: its box, looped-repeat dividers, note picture and label.
    #[allow(clippy::too_many_arguments)]
    fn draw_block(
        &self,
        grid: &Painter,
        geom: &SongGeom,
        uid: MachineUid,
        b: &Block,
        end: u32,
        body: Rect,
        selected: bool,
        lens: Lens<'_>,
    ) {
        let r = Rect::from_min_max(
            pos2(geom.x_of(b.start) + 0.5, body.top()),
            pos2(geom.x_of(end) - 0.5, body.bottom()),
        );
        let painter = grid.with_clip_rect(r.intersect(grid.clip_rect()));
        let (fill, ink) = if b.mute {
            (theme::GRID_DARK, theme::KNOB_TRACK)
        } else {
            (theme::ACID.gamma_multiply(0.18), theme::ACID)
        };
        painter.rect_filled(r, 3.0, fill);
        let period = lens(uid, b.pattern.0).max(1);
        if b.len.is_some() {
            let (lo, hi) = geom.visible(grid.clip_rect().right());
            // The first repeat start in view.
            let first = lo.saturating_sub(b.start) / period;
            let mut s = b.start + period * first.max(1);
            while s < end.min(hi) {
                let x = geom.x_of(s);
                painter.line_segment(
                    [pos2(x, r.top() + 2.0), pos2(x, r.bottom() - 2.0)],
                    Stroke::new(1.0_f32, ink.gamma_multiply(0.5)),
                );
                s += period;
            }
        }
        let area = Rect::from_min_max(
            pos2(r.left() + 1.0, r.top() + LABEL_H),
            pos2(r.right() - 1.0, r.bottom() - 2.0),
        );
        if area.height() >= 4.0 {
            self.block_preview(&painter, geom, uid, b, end, period, area, ink);
        }
        painter.text(
            pos2(r.left() + 3.0, r.top() + 1.0),
            Align2::LEFT_TOP,
            b.pattern.label(),
            FontId::monospace(10.0),
            if b.mute {
                theme::KNOB_TRACK
            } else {
                Color32::from_gray(220)
            },
        );
        let stroke = if selected {
            Stroke::new(1.5_f32, theme::ACID_LIGHT)
        } else {
            Stroke::new(1.0_f32, ink.gamma_multiply(0.6))
        };
        grid.rect_stroke(r, 3.0, stroke, StrokeKind::Inside);
    }

    /// The pattern's notes inside a block, repeated every `period` steps
    /// up to its `end`: pitches scaled to the area's height, drum hits as
    /// dots on their instrument's row. Only what is in view is drawn.
    #[allow(clippy::too_many_arguments)]
    fn block_preview(
        &self,
        painter: &Painter,
        geom: &SongGeom,
        uid: MachineUid,
        b: &Block,
        end: u32,
        period: u32,
        area: Rect,
        ink: Color32,
    ) {
        let Some(roll) = self.doc.project.roll(uid, b.pattern.0) else {
            return;
        };
        let visible = roll.ticks();
        let notes = || roll.notes.iter().filter(|n| n.start < visible);
        let drums = self.doc.project.kit(uid).is_some();
        let (lo, hi) = if drums {
            (0, DRUM_INSTRUMENTS as u8 - 1)
        } else {
            let lo = notes().map(|n| n.pitch).min();
            let hi = notes().map(|n| n.pitch).max();
            let (Some(lo), Some(hi)) = (lo, hi) else {
                return;
            };
            (lo, hi)
        };
        let row_h = area.height() / (hi - lo + 1) as f32;
        let bar_h = row_h.clamp(1.0, 3.0);
        let colour = ink.gamma_multiply(0.85);
        let tpb = TICKS_PER_STEP as u32;
        let end_tick = end * tpb;
        let right = painter.clip_rect().right();
        let (lo_step, _) = geom.visible(right);
        let mut rep = b.start + period * (lo_step.saturating_sub(b.start) / period);
        while rep < end && geom.x_of(rep) <= right {
            for n in notes() {
                let start = rep * tpb + n.start as u32;
                if start >= end_tick {
                    continue;
                }
                let stop = (start + n.len.max(1) as u32).min(end_tick);
                let (x0, x1) = (geom.x_of_tick(start), geom.x_of_tick(stop));
                if x1 < area.left() || x0 > area.right() {
                    continue;
                }
                if drums {
                    if n.pitch > hi {
                        continue;
                    }
                    let y = area.top() + ((hi - n.pitch) as f32 + 0.5) * row_h;
                    painter.circle_filled(pos2(x0 + 1.5, y), (row_h / 2.0).clamp(1.0, 1.5), colour);
                } else {
                    let y = area.top() + (hi - n.pitch) as f32 * row_h + (row_h - bar_h) / 2.0;
                    painter.rect_filled(
                        Rect::from_min_max(pos2(x0, y), pos2(x1.max(x0 + 1.0), y + bar_h)),
                        0.0,
                        colour,
                    );
                }
            }
            rep += period;
        }
    }

    /// The lane's clicks and drags on the blocks of `blocks` (the lane as
    /// drawn this frame). True when a double-click opened a pattern (the
    /// dock is in Pattern mode from here on).
    fn song_lane_mouse(
        &mut self,
        ui: &egui::Ui,
        uid: MachineUid,
        resp: &egui::Response,
        geom: &SongGeom,
        blocks: &[Block],
    ) -> bool {
        let primary = egui::PointerButton::Primary;
        let mods = ui.input(|i| i.modifiers);
        let pointer = resp
            .interact_pointer_pos()
            .or_else(|| ui.input(|i| i.pointer.latest_pos()));
        // Drags hit-test where the press began.
        let press = ui.input(|i| i.pointer.press_origin()).or(pointer);
        let on_grid = |p: &egui::Pos2| p.x >= geom.left;
        let hit = |app: &Self, p: egui::Pos2| {
            let step = geom.step_at(p.x);
            app.with_lens(|lens| song::block_at(blocks, uid, step, lens))
        };
        let mut sel = self.block_selection(uid);
        let toggle = |sel: &mut Vec<usize>, i: usize| match sel.binary_search(&i) {
            Ok(k) => {
                sel.remove(k);
            }
            Err(k) => sel.insert(k, i),
        };

        let pressed = resp.clicked() || resp.secondary_clicked() || resp.drag_started();
        if pressed && pointer.is_some_and(|p| on_grid(&p)) {
            self.view.focus = Some(uid);
            self.view.dock.keyboard = true;
            // With the selections the range made in the other lanes.
            self.drop_range();
        }

        if resp.clicked()
            && let Some(p) = pointer.filter(on_grid)
        {
            match hit(self, p) {
                Some(i) if mods.shift => toggle(&mut sel, i),
                Some(i) => sel = vec![i],
                None if mods.shift => {}
                None => sel.clear(),
            }
            self.set_block_selection(uid, blocks, &sel);
        }
        if resp.double_clicked()
            && let Some(p) = pointer.filter(on_grid)
        {
            match hit(self, p) {
                Some(i) => {
                    let slot = blocks[i].pattern.0;
                    self.set_mode(Mode::Pattern);
                    self.select_pad(uid, slot);
                    return true;
                }
                None => {
                    let at = snap_bar(geom.step_at(p.x), false);
                    let block = Block::new(at, PatternRef(self.selected(uid)));
                    let mut lane = blocks.to_vec();
                    match self.with_lens(|lens| song::place(&mut lane, uid, block, lens)) {
                        Some(i) => {
                            self.set_block_selection(uid, &lane, &[i]);
                            self.commit_song_lane(uid, lane, false);
                        }
                        None => {
                            self.view.message =
                                Some("There is no room on the song for that pattern".into());
                        }
                    }
                }
            }
        }
        if resp.secondary_clicked()
            && let Some(p) = pointer.filter(on_grid)
            && let Some(i) = hit(self, p)
        {
            let mut lane = blocks.to_vec();
            if sel.contains(&i) {
                song_ops::delete(&mut lane, &sel);
                self.view.panel(uid).song_sel.clear();
            } else {
                // The selection is kept by value, so it survives this.
                song_ops::delete(&mut lane, &[i]);
            }
            self.commit_song_lane(uid, lane, false);
        }

        if resp.drag_started_by(primary)
            && let Some(p) = press.filter(on_grid)
        {
            let step = geom.step_at(p.x);
            let drag = match hit(self, p) {
                None => SongDrag::Marquee {
                    from: step,
                    add: mods.shift,
                },
                Some(i) => {
                    // The grabbed block joins (Shift) or replaces the selection.
                    if sel.binary_search(&i).is_err() {
                        if mods.shift {
                            toggle(&mut sel, i);
                        } else {
                            sel = vec![i];
                        }
                    }
                    self.set_block_selection(uid, blocks, &sel);
                    let end = self.with_lens(|lens| song::block_end(&blocks[i], uid, lens));
                    // A block too narrow for an edge and a middle moves;
                    // a narrowish one keeps most of itself for moving.
                    let w = geom.x_of(end) - geom.x_of(blocks[i].start);
                    let edge = EDGE_PX.min(w / 4.0);
                    if w >= 2.0 * EDGE_PX && (geom.x_of(end) - p.x).abs() <= edge {
                        SongDrag::Resize {
                            origin: blocks.to_vec(),
                            idx: sel.clone(),
                            grab: end,
                        }
                    } else {
                        SongDrag::Move {
                            origin: blocks.to_vec(),
                            idx: sel.clone(),
                            grab: step,
                            copy: mods.command || mods.ctrl,
                        }
                    }
                }
            };
            self.view.panel(uid).song_drag = Some(drag);
        }

        // Each frame recomputes the drag from its origin.
        if resp.dragged_by(primary)
            && let Some(p) = pointer
        {
            let step = geom.step_at(p.x);
            let fine = mods.alt;
            let unit = if fine { 1 } else { BAR };
            let moved = match self
                .view
                .panels
                .get(&uid)
                .and_then(|p| p.song_drag.as_ref())
            {
                Some(SongDrag::Move {
                    origin,
                    idx,
                    grab,
                    copy,
                }) => {
                    let mut lane = origin.clone();
                    let dt = snap_bar(step, fine) as i64 - snap_bar(*grab, fine) as i64;
                    let new = self.with_lens(|lens| {
                        if !*copy {
                            song_ops::shift(&mut lane, uid, idx, dt, unit, lens)
                        } else if dt == 0 {
                            Some(idx.clone())
                        } else {
                            song_ops::shift_copies(&mut lane, uid, idx, dt, unit, lens)
                        }
                    });
                    // A copy that fits nowhere: stay where the last fit was.
                    new.map(|new| (lane, new))
                }
                Some(SongDrag::Resize { origin, idx, grab }) => {
                    let mut lane = origin.clone();
                    let end = if fine {
                        step + 1
                    } else {
                        snap_bar(step, false) + BAR
                    };
                    let dlen = end as i64 - *grab as i64;
                    self.with_lens(|lens| song_ops::resize(&mut lane, uid, idx, dlen, lens));
                    Some((lane, idx.clone()))
                }
                _ => None,
            };
            if let Some((lane, new)) = moved {
                self.set_block_selection(uid, &lane, &new);
                self.commit_song_lane(uid, lane, true);
            }
        }

        if resp.drag_stopped_by(primary) {
            match self.view.panel(uid).song_drag.take() {
                Some(SongDrag::Move { .. } | SongDrag::Resize { .. }) => {
                    // Dropped back where it began: no undo step, not dirty.
                    self.doc.end_gesture();
                    self.doc.drop_no_op_song_step();
                }
                Some(SongDrag::Marquee { from, add }) => {
                    if let Some(p) = pointer {
                        let to = geom.step_at(p.x);
                        let hits =
                            self.with_lens(|lens| song_ops::in_box(blocks, uid, from, to, lens));
                        if !add {
                            sel.clear();
                        }
                        sel.extend(hits);
                        sel.sort_unstable();
                        sel.dedup();
                        self.set_block_selection(uid, blocks, &sel);
                    }
                }
                None => {}
            }
            self.doc.end_gesture();
        }
        false
    }

    /// Makes the blocks starting in steps `[a, b)` every lane's selection.
    pub(crate) fn select_song_range(&mut self, a: u32, b: u32) {
        for uid in self.doc.project.rack_uids() {
            let lane = self.doc.project.song.lane(uid).to_vec();
            let idx = song_ops::in_range(&lane, a, b);
            self.set_block_selection(uid, &lane, &idx);
        }
    }
}

/// Where a song lane's block area starts below the lane's top.
#[cfg(test)]
pub(crate) const BLOCKS_TOP: f32 = super::lane::HEADER_H + PAD_Y;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::test_rack::*;
    use acid_engine::project::Project;

    const NONE: egui::Modifiers = egui::Modifiers::NONE;
    const W: f32 = 800.0;

    #[test]
    fn a_ruler_range_stops_at_the_song_limit() {
        let max = MAX_SONG_STEPS as u32;
        assert_eq!(bar_range(40, 3), (0, 48), "whole bars, either order");
        assert_eq!(bar_range(16, 65_000), (16, max as u16), "end clamped");
        assert_eq!(bar_range(16, max - 1), (16, max as u16), "the last bar");
        let (lo, hi) = bar_range(65_000, 70_000);
        assert!(lo < hi && hi as u32 == max, "past the limit: the last bar");
        assert_eq!(lo as u32, max - BAR);
    }

    /// Mouse gestures on one machine's song lane, with egui's clock.
    struct Mouse {
        ctx: egui::Context,
        uid: MachineUid,
        time: f64,
        origin: egui::Pos2,
    }

    impl Mouse {
        fn new(app: &mut SidAcidApp, uid: MachineUid) -> Mouse {
            let mut m = Mouse {
                ctx: egui::Context::default(),
                uid,
                time: 0.0,
                origin: pos2(0.0, 0.0),
            };
            m.run(app, vec![], NONE);
            m
        }

        fn run(&mut self, app: &mut SidAcidApp, events: Vec<egui::Event>, mods: egui::Modifiers) {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
                time: Some(self.time),
                events,
                modifiers: mods,
                ..Default::default()
            };
            let uid = self.uid;
            let mut origin = self.origin;
            let _ = self.ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    origin = ui.cursor().min;
                    app.song_lane(ui, uid, W);
                });
            });
            self.origin = origin;
        }

        /// Screen position of `step` (fractions allowed), inside the blocks.
        fn at(&self, app: &SidAcidApp, step: f32) -> egui::Pos2 {
            let x = self.origin.x + app.lane_key_w() + step * step_w(app.view.dock.zoom)
                - app.view.dock.scroll_x;
            pos2(x, self.origin.y + BLOCKS_TOP + 30.0)
        }

        fn frame(&mut self, app: &mut SidAcidApp, events: Vec<egui::Event>, mods: egui::Modifiers) {
            // Well apart, so no two clicks make a double click.
            self.time += 0.5;
            self.run(app, events, mods);
        }

        fn button(
            at: egui::Pos2,
            button: egui::PointerButton,
            pressed: bool,
            modifiers: egui::Modifiers,
        ) -> egui::Event {
            egui::Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers,
            }
        }

        fn click_with(
            &mut self,
            app: &mut SidAcidApp,
            at: egui::Pos2,
            button: egui::PointerButton,
            mods: egui::Modifiers,
        ) {
            self.frame(app, vec![egui::Event::PointerMoved(at)], mods);
            self.frame(app, vec![Self::button(at, button, true, mods)], mods);
            self.frame(app, vec![Self::button(at, button, false, mods)], mods);
            self.frame(app, vec![], mods);
        }

        fn click(&mut self, app: &mut SidAcidApp, at: egui::Pos2, mods: egui::Modifiers) {
            self.click_with(app, at, egui::PointerButton::Primary, mods);
        }

        fn double_click(&mut self, app: &mut SidAcidApp, at: egui::Pos2) {
            let primary = egui::PointerButton::Primary;
            self.frame(app, vec![egui::Event::PointerMoved(at)], NONE);
            for _ in 0..2 {
                for pressed in [true, false] {
                    self.time += 0.05;
                    self.run(app, vec![Self::button(at, primary, pressed, NONE)], NONE);
                }
            }
            self.frame(app, vec![], NONE);
        }

        fn drag(
            &mut self,
            app: &mut SidAcidApp,
            from: egui::Pos2,
            to: egui::Pos2,
            mods: egui::Modifiers,
        ) {
            let primary = egui::PointerButton::Primary;
            self.frame(app, vec![egui::Event::PointerMoved(from)], mods);
            self.frame(app, vec![Self::button(from, primary, true, mods)], mods);
            for k in 1..=4 {
                let p = from + (to - from) * (k as f32 / 4.0);
                self.frame(app, vec![egui::Event::PointerMoved(p)], mods);
            }
            self.frame(app, vec![Self::button(to, primary, false, mods)], mods);
            self.frame(app, vec![], mods);
        }
    }

    /// A Song-mode app (zoomed out, so 60 steps fit the lane) whose Synth 1
    /// lane holds `blocks` (start, pattern); every pattern lasts 16 steps.
    fn app_with(blocks: &[(u32, u8)]) -> SidAcidApp {
        let mut doc = Document::new(Project::default());
        doc.project
            .song
            .lane_mut(SYNTH1)
            .extend(blocks.iter().map(|&(s, p)| Block::new(s, PatternRef(p))));
        let mut app = SidAcidApp::headless(doc);
        app.view.mode = Mode::Song;
        app.view.dock.zoom = 0;
        app
    }

    fn lane(app: &SidAcidApp) -> Vec<(u32, u8, Option<u16>)> {
        app.doc
            .project
            .song
            .lane(SYNTH1)
            .iter()
            .map(|b| (b.start, b.pattern.0, b.len))
            .collect()
    }

    fn selected(app: &SidAcidApp) -> Vec<u32> {
        let lane = app.doc.project.song.lane(SYNTH1);
        app.block_selection(SYNTH1)
            .iter()
            .map(|&i| lane[i].start)
            .collect()
    }

    #[test]
    fn double_clicking_empty_space_places_the_selected_pattern_at_the_bar() {
        let mut app = app_with(&[(0, 0)]);
        app.doc.project.machine_mut(SYNTH1).unwrap().selected = 2;
        let mut m = Mouse::new(&mut app, SYNTH1);
        let at = m.at(&app, 37.5);
        m.double_click(&mut app, at);
        assert_eq!(lane(&app), [(0, 0, None), (32, 2, None)]);
        assert_eq!(selected(&app), [32], "the new block is selected");
        assert_eq!(app.view.mode, Mode::Song);
        app.undo();
        assert_eq!(lane(&app), [(0, 0, None)], "one undo step");
    }

    #[test]
    fn a_placed_block_lands_on_the_nearest_free_spot() {
        // Bar 2 (16..32) is free only from 24: the new block goes after.
        let mut app = app_with(&[(0, 0), (8, 1)]);
        app.doc.project.roll_mut(SYNTH1, 0).unwrap().length = 8;
        app.doc.project.roll_mut(SYNTH1, 1).unwrap().length = 16;
        let mut m = Mouse::new(&mut app, SYNTH1);
        let at = m.at(&app, 26.5);
        m.double_click(&mut app, at);
        assert_eq!(lane(&app), [(0, 0, None), (8, 1, None), (24, 0, None)]);
    }

    #[test]
    fn clicks_select_blocks_and_make_no_undo_step() {
        let mut app = app_with(&[(0, 0), (16, 1), (48, 2)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        let (a, b, gap) = (m.at(&app, 4.5), m.at(&app, 20.5), m.at(&app, 40.5));
        m.click(&mut app, a, NONE);
        assert_eq!(selected(&app), [0]);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        assert_eq!(selected(&app), [0, 16], "Shift adds");
        m.click(&mut app, a, egui::Modifiers::SHIFT);
        assert_eq!(selected(&app), [16], "Shift toggles");
        m.click(&mut app, gap, NONE);
        assert!(selected(&app).is_empty(), "empty space clears");
        assert_eq!(app.view.focus, Some(SYNTH1));
        assert!(!app.doc.is_dirty());
        assert_eq!(app.doc.undo_depth(), 0);
    }

    #[test]
    fn dragging_empty_space_box_selects() {
        let mut app = app_with(&[(0, 0), (16, 1), (48, 2)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        // From the gap at 32..48 back into the block at 16.
        let (from, to) = (m.at(&app, 40.5), m.at(&app, 20.5));
        m.drag(&mut app, from, to, NONE);
        assert_eq!(selected(&app), [16], "the blocks the box touches");
        let (from, to) = (m.at(&app, 40.5), m.at(&app, 50.5));
        m.drag(&mut app, from, to, egui::Modifiers::SHIFT);
        assert_eq!(selected(&app), [16, 48], "Shift adds");
        assert_eq!(app.doc.undo_depth(), 0);
    }

    #[test]
    fn dragging_a_block_moves_the_selection_by_bars_in_one_undo_step() {
        let mut app = app_with(&[(0, 0), (16, 1)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        let (a, b) = (m.at(&app, 4.5), m.at(&app, 20.5));
        m.click(&mut app, a, NONE);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        // 4.5 → 27.5: one bar on (whole bars, whatever the steps).
        let to = m.at(&app, 27.5);
        m.drag(&mut app, a, to, NONE);
        assert_eq!(lane(&app), [(16, 0, None), (32, 1, None)]);
        assert_eq!(selected(&app), [16, 32], "they stay selected");
        assert_eq!(app.doc.undo_depth(), 1);
        app.undo();
        assert_eq!(lane(&app), [(0, 0, None), (16, 1, None)]);
    }

    #[test]
    fn a_two_block_move_onto_taken_space_keeps_its_spacing() {
        let mut app = app_with(&[(0, 0), (16, 1), (32, 2)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        let (a, b) = (m.at(&app, 4.5), m.at(&app, 20.5));
        m.click(&mut app, a, NONE);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        // Two bars on would sit on the block at 32: the pair goes one bar
        // further, together, with nothing taken in between.
        let to = m.at(&app, 36.5);
        m.drag(&mut app, a, to, NONE);
        assert_eq!(lane(&app), [(32, 2, None), (48, 0, None), (64, 1, None)]);
        assert_eq!(selected(&app), [48, 64]);
        // One bar on: no offset nearer than none at all fits.
        let mut app = app_with(&[(0, 0), (16, 1), (32, 2)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        m.click(&mut app, a, NONE);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        let to = m.at(&app, 20.5);
        m.drag(&mut app, a, to, NONE);
        assert_eq!(lane(&app), [(0, 0, None), (16, 1, None), (32, 2, None)]);
        assert!(!app.doc.is_dirty(), "nothing moved");
    }

    #[test]
    fn a_drag_dropped_where_it_began_leaves_no_undo_step() {
        let mut app = app_with(&[(0, 0)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        let primary = egui::PointerButton::Primary;
        let (a, far) = (m.at(&app, 4.5), m.at(&app, 40.5));
        m.frame(&mut app, vec![egui::Event::PointerMoved(a)], NONE);
        m.frame(&mut app, vec![Mouse::button(a, primary, true, NONE)], NONE);
        m.frame(&mut app, vec![egui::Event::PointerMoved(far)], NONE);
        assert_eq!(lane(&app), [(32, 0, None)], "moved mid-drag");
        m.frame(&mut app, vec![egui::Event::PointerMoved(a)], NONE);
        m.frame(&mut app, vec![Mouse::button(a, primary, false, NONE)], NONE);
        m.frame(&mut app, vec![], NONE);
        assert_eq!(lane(&app), [(0, 0, None)]);
        assert_eq!(app.doc.undo_depth(), 0, "no step");
        assert!(!app.doc.is_dirty());
    }

    #[test]
    fn grabbing_a_narrow_block_moves_it() {
        let mut app = app_with(&[]);
        app.doc.project.song.lane_mut(SYNTH1).push(Block {
            len: Some(1),
            ..Block::new(0, PatternRef(0))
        });
        let mut m = Mouse::new(&mut app, SYNTH1);
        // 11 px wide: a press 4 px left of its right edge moves it.
        let (from, to) = (m.at(&app, 0.6), m.at(&app, 16.6));
        m.drag(&mut app, from, to, NONE);
        assert_eq!(lane(&app), [(16, 0, Some(1))]);
    }

    #[test]
    fn alt_dragging_moves_by_single_steps() {
        let mut app = app_with(&[(0, 0)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        let (from, to) = (m.at(&app, 4.5), m.at(&app, 9.5));
        m.drag(&mut app, from, to, egui::Modifiers::ALT);
        assert_eq!(lane(&app), [(5, 0, None)]);
    }

    #[test]
    fn a_move_onto_a_block_lands_on_the_nearest_free_spot() {
        let mut app = app_with(&[(0, 0), (32, 1)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        // Two bars on would sit on the block at 32: it goes just after it
        // (16 is as near, and later wins).
        let (from, to) = (m.at(&app, 4.5), m.at(&app, 36.5));
        m.drag(&mut app, from, to, NONE);
        assert_eq!(lane(&app), [(32, 1, None), (48, 0, None)]);
        assert_eq!(selected(&app), [48]);
    }

    #[test]
    fn ctrl_dragging_moves_copies_and_leaves_the_originals() {
        let mut app = app_with(&[(0, 0)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        let (from, to) = (m.at(&app, 4.5), m.at(&app, 36.5));
        m.drag(&mut app, from, to, egui::Modifiers::COMMAND);
        assert_eq!(lane(&app), [(0, 0, None), (32, 0, None)]);
        assert_eq!(selected(&app), [32], "the copy is selected");
        app.undo();
        assert_eq!(lane(&app), [(0, 0, None)], "one undo step");
    }

    #[test]
    fn dragging_a_right_edge_fixes_the_selections_length_in_bars() {
        let mut app = app_with(&[(0, 0), (40, 1)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        // The edge at step 16, dragged into bar 2 (16..32): ends at 32.
        let (from, to) = (m.at(&app, 15.8), m.at(&app, 25.5));
        m.drag(&mut app, from, to, NONE);
        assert_eq!(lane(&app), [(0, 0, Some(32)), (40, 1, None)]);
        assert_eq!(app.doc.undo_depth(), 1);
        // Into bar 3: held at the next block.
        let (from, to) = (m.at(&app, 31.8), m.at(&app, 45.5));
        m.drag(&mut app, from, to, NONE);
        assert_eq!(lane(&app)[0], (0, 0, Some(40)));
        // Alt: single steps.
        let (from, to) = (m.at(&app, 39.8), m.at(&app, 6.5));
        m.drag(&mut app, from, to, egui::Modifiers::ALT);
        assert_eq!(lane(&app)[0], (0, 0, Some(7)));
        assert_eq!(app.doc.undo_depth(), 3);
    }

    #[test]
    fn right_clicking_deletes_the_selection_or_the_block_under_the_pointer() {
        let mut app = app_with(&[(0, 0), (16, 1), (48, 2)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        let (a, b, c) = (m.at(&app, 4.5), m.at(&app, 20.5), m.at(&app, 50.5));
        m.click(&mut app, a, NONE);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        let secondary = egui::PointerButton::Secondary;
        // Not selected: only it goes, the selection stays.
        m.click_with(&mut app, c, secondary, NONE);
        assert_eq!(lane(&app), [(0, 0, None), (16, 1, None)]);
        assert_eq!(selected(&app), [0, 16]);
        m.click_with(&mut app, a, secondary, NONE);
        assert!(lane(&app).is_empty(), "the whole selection goes");
        assert_eq!(app.doc.undo_depth(), 2);
        app.undo();
        assert_eq!(lane(&app), [(0, 0, None), (16, 1, None)]);
    }

    #[test]
    fn double_clicking_a_block_opens_its_pattern() {
        let mut app = app_with(&[(0, 0), (16, 3)]);
        let mut m = Mouse::new(&mut app, SYNTH1);
        let at = m.at(&app, 20.5);
        m.double_click(&mut app, at);
        assert_eq!(app.view.mode, Mode::Pattern);
        assert_eq!(app.selected(SYNTH1), 3);
        assert_eq!(app.view.focus, Some(SYNTH1));
        assert_eq!(
            lane(&app),
            [(0, 0, None), (16, 3, None)],
            "the song is untouched"
        );
        assert!(
            app.block_selection(SYNTH1).is_empty(),
            "selections go with the mode"
        );
    }

    #[test]
    fn a_song_drag_holds_keys_and_undo_until_it_is_dropped() {
        let mut app = app_with(&[(0, 0)]);
        app.view.panel(SYNTH1).song_drag = Some(SongDrag::Move {
            origin: Vec::new(),
            idx: Vec::new(),
            grab: 0,
            copy: false,
        });
        assert!(app.mouse_editing());
        app.release_stale_poly_input(false);
        assert!(!app.mouse_editing(), "released once the pointer is up");
    }

    #[test]
    fn blocks_draw_with_previews_for_every_machine() {
        let mut doc = Document::new(acid_engine::demo::demo_project());
        let uids = doc.project.rack_uids();
        for (k, uid) in uids.iter().enumerate() {
            let lane = doc.project.song.lane_mut(*uid);
            lane.clear();
            lane.push(Block::new(0, PatternRef(0)));
            lane.push(Block {
                len: Some(40),
                mute: k % 2 == 0,
                ..Block::new(16, PatternRef(1))
            });
        }
        let mut app = SidAcidApp::headless(doc);
        app.view.mode = Mode::Song;
        for uid in uids {
            Mouse::new(&mut app, uid);
        }
    }
}
