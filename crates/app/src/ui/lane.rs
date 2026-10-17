//! A lane: one machine's pattern as a piano roll, drawn at a given width
//! wherever the caller puts it. A header row (collapse arrow, name,
//! pattern, strip toggle), the grid (one column per tick, scrolled and
//! zoomed with the dock; one row per MIDI pitch, or per instrument on a
//! drum kit), an optional expression strip and a height handle.
//!
//! Notes are edited as a selection. Click a note to select it (Shift
//! toggles it), click empty space to add a note one snap long, drag empty
//! space to box-select (Shift adds), drag a note to move the selection
//! (Ctrl moves copies, Alt moves by single ticks), drag a note's right
//! edge to stretch the selection (not on drum lanes); right-click deletes
//! the selection, or the note under the pointer when it is not selected.
//! Chords stack freely. The acid synths keep one note at a time and get
//! accent and slide lanes instead of velocity; drum lanes get an accent
//! lane, and double-clicking a drum hit toggles its accent. Clicking the
//! key strip of an FM, Mono or sampler lane auditions the pitch.
//! Melodic lanes show the project key (rows off its scale darker, root
//! rows marked and named) and, with snap to scale on, put clicked and
//! dragged notes on it; drum lanes ignore the key. A folded melodic lane
//! shows only the rows on the key's scale plus those holding notes.
//! The strip shows velocity (accent and slide on acid lanes, accent on
//! drums), or the notes' chance or ratchet (a selector in the header).
//! Notes with a chance below 100 or a condition get a dashed outline, and
//! a badge ("1:4 50% ×3") where the note is wide enough.

use super::key_entry::LaneKind;
use super::lane_geom::{FoldRows, LaneGeom, Rows, ZOOMS, tick_w};
use super::note_ops::{self, Field};
use super::poly_edit::Snap;
use super::poly_edit::{
    Flag, LaneDrag, add, note_at, set_velocity_in, toggle_accent_in, toggle_flag,
};
use super::{SidAcidApp, theme};
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::machine::MachineUid;
use acid_engine::project::{
    BASE_MIDI, LaneView, MAX_NOTES, MachineBody, NotePattern, PatternRef, STEPS, TICKS_PER_STEP,
};
use acid_engine::scale::Scale;
use acid_engine::sequencer::Mode;
use acid_engine::trig;
use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, FontId, Layout, Rect, RichText, Sense, Shape, Stroke,
    StrokeKind, UiBuilder, pos2, vec2,
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
const NEW_VEL: u8 = 100;
/// Pitch the FM roll first scrolls to: C3 (MIDI 48).
const FM_START_PITCH: u8 = 48;
/// Pitch the Mono roll first scrolls to: C2 (MIDI 36), as it is mostly bass.
const MONO_START_PITCH: u8 = 36;
/// Laid over rows off the project key's scale.
const OFF_SCALE: Color32 = Color32::from_black_alpha(70);
const BLACK_KEYS: [bool; 12] = [
    false, true, false, true, false, false, true, false, true, false, true, false,
];

/// What the expression strip shows (a view setting, not saved).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StripMode {
    /// Velocity; acid: accent and slide; drums: accent.
    #[default]
    Expr,
    Chance,
    Ratchet,
}

impl StripMode {
    pub(crate) const ALL: [StripMode; 3] = [StripMode::Expr, StripMode::Chance, StripMode::Ratchet];

    fn label(self, kind: LaneKind) -> &'static str {
        match (self, kind) {
            (StripMode::Expr, LaneKind::Poly | LaneKind::Pads) => "Vel",
            (StripMode::Expr, LaneKind::Acid) => "Acc/Slide",
            (StripMode::Expr, LaneKind::Drums) => "Acc",
            (StripMode::Chance, _) => "Chance",
            (StripMode::Ratchet, _) => "Ratchet",
        }
    }
}

/// A bar strip: one bar per note, its height the field's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bar {
    Vel,
    Chance,
    Ratchet,
}

impl Bar {
    fn label(self) -> &'static str {
        match self {
            Bar::Vel => "Vel",
            Bar::Chance => "Chc",
            Bar::Ratchet => "Rat",
        }
    }

    /// The lowest and highest value.
    fn range(self) -> (u8, u8) {
        match self {
            Bar::Vel => (1, 127),
            Bar::Chance => (0, 100),
            Bar::Ratchet => (1, 8),
        }
    }

    fn get(self, n: &acid_engine::project::Note) -> u8 {
        match self {
            Bar::Vel => n.vel,
            Bar::Chance => n.chance,
            Bar::Ratchet => n.ratchet,
        }
    }

    fn field(self, v: u8) -> Field {
        match self {
            Bar::Vel => Field::Vel(v),
            Bar::Chance => Field::Chance(v),
            Bar::Ratchet => Field::Ratchet(v),
        }
    }

    /// The value at height `y` in a strip whose top is `top`.
    fn value_at(self, y: f32, top: f32) -> u8 {
        let (lo, hi) = self.range();
        ((1.0 - (y - top) / VEL_H) * hi as f32)
            .round()
            .clamp(lo as f32, hi as f32) as u8
    }
}

/// One frame's edits to a lane's pattern.
#[derive(Default)]
struct Frame {
    changed: bool,
    finished: bool,
    /// A move or resize was dropped: the dragged notes' indices.
    dropped: Option<Vec<usize>>,
    /// The pointer went down in an expression strip.
    pressed: bool,
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

    /// How `uid`'s lane edits: acid synths, drum kits, Pad Kits, or freely
    /// (the rest).
    pub(crate) fn lane_kind(&self, uid: MachineUid) -> LaneKind {
        let p = &self.doc.project;
        if p.synth(uid).is_some() {
            LaneKind::Acid
        } else if p.kit(uid).is_some() {
            LaneKind::Drums
        } else if p.padkit(uid).is_some() {
            LaneKind::Pads
        } else {
            LaneKind::Poly
        }
    }

    /// The rows of `uid`'s lane: one per instrument on drums, one per pad
    /// on a Pad Kit, else one per pitch, or with `fold` only those of
    /// `FoldRows` for `pattern` and the project key.
    pub(crate) fn lane_rows(&self, uid: MachineUid, fold: bool, pattern: &NotePattern) -> Rows {
        match self.lane_kind(uid) {
            LaneKind::Drums => Rows::Drums,
            LaneKind::Pads => Rows::Pads,
            _ if fold => Rows::Folded(FoldRows::new(self.doc.project.key, pattern)),
            _ => Rows::Pitches,
        }
    }

    /// Turns `uid`'s fold over, keeping the pitch in the middle of its view
    /// in the middle.
    fn toggle_fold(&mut self, uid: MachineUid, view: LaneView) {
        let height = view.clamped_height() as f32;
        let pattern = self
            .doc
            .project
            .roll(uid, self.selected(uid))
            .cloned()
            .unwrap_or_default();
        let (old, new) = (
            self.lane_rows(uid, view.fold, &pattern),
            self.lane_rows(uid, !view.fold, &pattern),
        );
        if let Some(y) = self.view.panel(uid).lane_scroll_y {
            let middle = ((y + height / 2.0) / ROW_H).floor().max(0.0) as usize;
            let pitch = old
                .pitch_of_row(middle.min(old.count().saturating_sub(1)))
                .unwrap_or(0);
            self.view.panel(uid).lane_scroll_y = Some(centred_y(&new, height, pitch));
        }
        if let Some(m) = self.doc.project.machine_mut(uid) {
            m.lane.fold = !view.fold;
        }
        self.settle_fold_view(uid);
    }

    /// Whether `uid`'s lane is a folded, expanded, melodic Pattern-mode lane.
    fn folded_view(&self, uid: MachineUid) -> bool {
        self.view.mode == Mode::Pattern
            && !self.lane_kind(uid).is_rows()
            && self
                .doc
                .project
                .machine(uid)
                .is_some_and(|m| m.lane.fold && !m.lane.collapsed)
    }

    /// Settles every lane's fold view; see `settle_fold_view`.
    pub(crate) fn settle_folds(&mut self) {
        let uids: Vec<MachineUid> = self.doc.project.machines.iter().map(|m| m.uid).collect();
        for uid in uids {
            self.settle_fold_view(uid);
        }
    }

    /// The pitch in the middle of each folded lane's view, taken before a
    /// key change so `recentre_folds` can keep it there.
    pub(crate) fn fold_middles(&mut self) -> Vec<(MachineUid, u8)> {
        let uids: Vec<MachineUid> = self.doc.project.machines.iter().map(|m| m.uid).collect();
        let mut out = Vec::new();
        for uid in uids {
            if !self.folded_view(uid) {
                continue;
            }
            let Some(y) = self.view.panel(uid).lane_scroll_y else {
                continue;
            };
            let height = self
                .doc
                .project
                .machine(uid)
                .map_or(0.0, |m| m.lane.clamped_height() as f32);
            let pattern = self
                .doc
                .project
                .roll(uid, self.selected(uid))
                .cloned()
                .unwrap_or_default();
            let rows = self.lane_rows(uid, true, &pattern);
            let middle = ((y + height / 2.0) / ROW_H).floor().max(0.0) as usize;
            if let Some(p) = rows.pitch_of_row(middle.min(rows.count().saturating_sub(1))) {
                out.push((uid, p));
            }
        }
        out
    }

    /// After the key (and so the rows) changed: puts each folded lane's
    /// middle pitch back in the middle and settles its cursor.
    pub(crate) fn recentre_folds(&mut self, middles: Vec<(MachineUid, u8)>) {
        for (uid, pitch) in middles {
            if !self.folded_view(uid) {
                continue;
            }
            let height = self
                .doc
                .project
                .machine(uid)
                .map_or(0.0, |m| m.lane.clamped_height() as f32);
            let pattern = self
                .doc
                .project
                .roll(uid, self.selected(uid))
                .cloned()
                .unwrap_or_default();
            let rows = self.lane_rows(uid, true, &pattern);
            self.view.panel(uid).lane_scroll_y = Some(centred_y(&rows, height, pitch));
        }
        self.settle_folds();
    }

    /// On a folded, expanded, melodic Pattern-mode lane: moves the keyboard
    /// cursor off a hidden pitch onto the shown row that draws it (so typing
    /// writes where the cursor appears) and keeps the pitch scroll on the
    /// rows. Called where the cursor is read.
    pub(crate) fn settle_fold_view(&mut self, uid: MachineUid) {
        if !self.folded_view(uid) {
            return;
        }
        let pattern = self
            .doc
            .project
            .roll(uid, self.selected(uid))
            .cloned()
            .unwrap_or_default();
        let rows = self.lane_rows(uid, true, &pattern);
        let height = self
            .doc
            .project
            .machine(uid)
            .map_or(0.0, |m| m.lane.clamped_height() as f32);
        let panel = self.view.panel(uid);
        if let Some(p) = rows.pitch_of_row(rows.row_near(panel.cursor.pitch)) {
            panel.cursor.pitch = p;
        }
        if let Some(y) = panel.lane_scroll_y {
            let max = (rows.count() as f32 * ROW_H - height).max(0.0);
            panel.lane_scroll_y = Some(y.clamp(0.0, max));
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
        self.lane_header(ui, uid, slot, view, width, true);
        if !view.collapsed {
            self.lane_body(ui, uid, slot, pattern, view, width);
        }
        self.lane_outline(ui, uid, origin, width);
        ui.spacing_mut().item_spacing.y = spacing;
    }

    /// Outlines the focused machine's lane, drawn from `origin` down to
    /// the `ui` cursor.
    pub(super) fn lane_outline(
        &self,
        ui: &egui::Ui,
        uid: MachineUid,
        origin: egui::Pos2,
        width: f32,
    ) {
        if self.focused() == Some(uid) {
            let outline = Rect::from_min_max(origin, pos2(origin.x + width, ui.cursor().top()));
            ui.painter().rect_stroke(
                outline,
                0.0,
                Stroke::new(1.0_f32, theme::ACID),
                StrokeKind::Inside,
            );
        }
    }

    /// The lane's header row: collapse arrow, name, pattern, and (with
    /// `strip_toggle`, in Pattern mode) the strip toggle and on melodic
    /// lanes the fold toggle. The toggles change the machine's `LaneView`
    /// directly (not undoable).
    pub(super) fn lane_header(
        &mut self,
        ui: &mut egui::Ui,
        uid: MachineUid,
        slot: u8,
        view: LaneView,
        width: f32,
        strip_toggle: bool,
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
        // Notes past a shortened pattern's end are kept but silent and
        // off the grid: say so in Pattern mode.
        let hidden = self.doc.project.roll(uid, slot).map_or(0, |p| {
            let ticks = p.ticks();
            p.notes.iter().filter(|n| n.start >= ticks).count()
        });
        if strip_toggle && hidden > 0 {
            row.label(RichText::new(format!("+{hidden} hidden")).small().weak())
                .on_hover_text(format!(
                    "{hidden} note{} past this pattern's end: silent and not shown. \
                     They come back if the pattern is lengthened.",
                    if hidden == 1 { " starts" } else { "s start" }
                ));
        }
        let kind = self.lane_kind(uid);
        let melodic = !kind.is_rows();
        let mode = self.view.panel(uid).strip_mode;
        let mut picked = mode;
        let (strip, fold) = if strip_toggle {
            row.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let strip = ui
                    .selectable_label(view.strip, RichText::new("Strip").small())
                    .on_hover_text("Show the expression strip")
                    .clicked();
                let fold = melodic
                    && ui
                        .selectable_label(view.fold, RichText::new("Fold").small())
                        .on_hover_text(
                            "Show only the rows on the key's scale, and rows holding notes",
                        )
                        .clicked();
                if view.strip {
                    ui.separator();
                    // Right to left: the last is leftmost.
                    for m in StripMode::ALL.into_iter().rev() {
                        if ui
                            .selectable_label(mode == m, RichText::new(m.label(kind)).small())
                            .on_hover_text("What the strip shows")
                            .clicked()
                        {
                            picked = m;
                        }
                    }
                }
                (strip, fold)
            })
            .inner
        } else {
            (false, false)
        };
        self.view.panel(uid).strip_mode = picked;
        if fold {
            self.toggle_fold(uid, view);
        }
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
        // Drum and pad lanes have a row per instrument or pad.
        let drums = kind.is_rows();
        let acid = kind == LaneKind::Acid;
        // An open note tool previews into this pattern: the mouse and the
        // strip edit nothing here until it is applied or cancelled.
        let locked = self.tool_lane() == Some((uid, slot));
        let key_w = self.lane_key_w();
        let height = view.clamped_height() as f32;
        let ticks = pattern.ticks();
        let snap = self.view.dock.snap;
        let tw = tick_w(ZOOMS[self.view.dock.zoom.min(ZOOMS.len() - 1)]);
        let playhead = self.playhead(uid, slot);
        // Drum and pad rows shrink to fit a short lane, so all always show.
        // Under a move or resize the rows stay those of the drag's start:
        // rows coming and going with the dragged notes would move the
        // pitch under the pointer. They are rebuilt once it is dropped.
        let key = self.doc.project.key;
        let frozen = match &self.view.panel(uid).drag {
            Some(
                LaneDrag::Move {
                    origin, slot: s, ..
                }
                | LaneDrag::Resize {
                    origin, slot: s, ..
                },
            ) if *s == slot => Some(FoldRows::new(key, origin)),
            _ => None,
        };
        let rows = match self.lane_rows(uid, view.fold, &pattern) {
            Rows::Folded(_) if let Some(f) = frozen => Rows::Folded(f),
            rows => rows,
        };
        if frozen.is_none() {
            self.settle_fold_view(uid);
        }
        let row_h = if drums {
            DRUM_ROW_H.min(height / rows.count() as f32)
        } else {
            ROW_H
        };
        let max_scroll_y = (rows.count() as f32 * ROW_H - height).max(0.0);
        if !drums && self.view.panel(uid).lane_scroll_y.is_none() {
            let pitch = middle_pitch(&pattern)
                .unwrap_or_else(|| self.roll_start_pitch(uid))
                .min(127);
            self.view.panel(uid).lane_scroll_y = Some(centred_y(&rows, height, pitch));
        }
        // A lane made taller has a smaller maximum.
        if let Some(y) = &mut self.view.panel(uid).lane_scroll_y {
            *y = y.clamp(0.0, max_scroll_y);
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
        let pkey = self.doc.project.key;
        let scaled = pkey.scale != Scale::Chromatic;
        let held = self.view.audition.filter(|a| a.0 == uid).map(|a| a.1);
        let pad_labels: Vec<String> = self
            .doc
            .project
            .padkit(uid)
            .map(|k| k.pads.iter().enumerate().map(|(i, p)| p.label(i)).collect())
            .unwrap_or_default();
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
                let name = match kind {
                    LaneKind::Pads => pad_labels.get(pitch as usize).map_or("", String::as_str),
                    _ => INST_NAMES[pitch as usize],
                };
                keys.text(
                    pos2(rect.left() + 2.0, y0 + row_h / 2.0),
                    Align2::LEFT_CENTER,
                    name,
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
            // The key: rows off the scale darker, the root marked.
            let root = scaled && pkey.is_root(pitch);
            if scaled && !pkey.contains(pitch) {
                grid.rect_filled(row, 0.0, OFF_SCALE);
            }
            if root {
                let y = y0 + row_h - 0.5;
                grid.line_segment(
                    [pos2(geom.left, y), pos2(rect.right(), y)],
                    Stroke::new(1.0_f32, theme::ACID),
                );
            }
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
            } else if root || pitch % 12 == 0 {
                let name = NOTE_NAMES[pitch as usize % 12];
                keys.text(
                    pos2(rect.left() + 2.0, y0 + ROW_H / 2.0),
                    Align2::LEFT_CENTER,
                    format!("{name}{}", pitch as i32 / 12 - 1),
                    FontId::monospace(8.0),
                    if black {
                        Color32::from_gray(230)
                    } else {
                        Color32::BLACK
                    },
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

        if let Some((lo, hi)) = self.view.dock.range {
            let band = Rect::from_min_max(
                pos2(geom.x_of(lo).max(geom.left), rect.top()),
                pos2(geom.x_of(hi).min(rect.right()), rect.bottom()),
            );
            if band.width() > 0.0 {
                grid.rect_filled(band, 0.0, theme::ACID.gamma_multiply(0.12));
            }
        }

        // Notes. A drum or pad note whose pitch is not a row has none.
        let drawn = |n: &acid_engine::project::Note| n.start < ticks && n.pitch <= kind.top_pitch();
        let mut sel = self.selection(uid);
        for (i, note) in pattern.notes.iter().enumerate().filter(|(_, n)| drawn(n)) {
            let r = geom.note_rect(note).shrink(0.5);
            // Pad hits shade by velocity, as it picks the layer.
            let fill = if kind == LaneKind::Drums {
                if note.accent {
                    theme::ACCENT
                } else {
                    theme::ACID
                }
            } else {
                theme::ACID.gamma_multiply(0.35 + 0.65 * note.vel as f32 / 127.0)
            };
            grid.rect_filled(r, 2.0, fill);
            if (acid || kind == LaneKind::Pads) && note.accent {
                grid.line_segment(
                    [
                        pos2(r.left(), r.top() + 1.0),
                        pos2(r.right(), r.top() + 1.0),
                    ],
                    Stroke::new(2.0_f32, theme::ACCENT),
                );
            }
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
            if trig::is_conditional(note) {
                let d = r.shrink(0.5);
                let corners = [
                    d.left_top(),
                    d.right_top(),
                    d.right_bottom(),
                    d.left_bottom(),
                    d.left_top(),
                ];
                grid.extend(Shape::dashed_line(
                    &corners,
                    Stroke::new(1.0_f32, Color32::WHITE),
                    3.0,
                    2.0,
                ));
            }
            if sel.binary_search(&i).is_ok() {
                grid.rect_stroke(
                    r,
                    2.0,
                    Stroke::new(1.5_f32, theme::ACID_LIGHT),
                    StrokeKind::Inside,
                );
            }
            let badge = note_ops::badge_text(note);
            if !badge.is_empty() {
                let galley = grid.layout_no_wrap(badge, FontId::monospace(7.0), Color32::BLACK);
                if galley.size().x + 3.0 <= r.width() {
                    let at = pos2(r.left() + 2.0, r.center().y - galley.size().y / 2.0);
                    grid.galley(at, galley, Color32::BLACK);
                }
            }
        }

        // The keyboard-entry cursor, one snap wide.
        if self.view.dock.keyboard && self.focused() == Some(uid) {
            let c = self.view.panel(uid).cursor;
            // On a folded lane a hidden pitch shows on the row below it.
            let row = match geom.rows {
                Rows::Folded(_) => Some(geom.rows.row_near(c.pitch)),
                _ => geom.row_of(c.pitch),
            };
            if let Some(r) = row {
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
        let pointer = resp.interact_pointer_pos().filter(|_| !locked);
        if resp.is_pointer_button_down_on()
            && let Some(p) = pointer
            && p.x < geom.left
        {
            key_down = geom.pitch_at(p.y).filter(|_| kind == LaneKind::Poly);
        }
        let pressed = resp.clicked() || resp.secondary_clicked() || resp.drag_started();
        if let Some(p) = pointer
            && (pressed || (resp.is_pointer_button_down_on() && p.x < geom.left))
        {
            // A press anywhere focuses the lane; on the key strip it moves
            // only the cursor's pitch.
            self.view.focus = Some(uid);
            self.view.dock.keyboard = true;
            if p.x >= geom.left {
                // With the selections the range made in the other lanes.
                self.drop_range();
            }
            let cursor = &mut self.view.panel(uid).cursor;
            cursor.pitch = geom.pitch_at_clamped(p.y);
            if p.x >= geom.left {
                cursor.tick = snap.floor(geom.tick_at_clamped(p.x));
            }
        }
        if !locked {
            self.lane_mouse(ui, uid, &resp, &geom, &mut pattern, &mut sel, &mut frame);
        }
        if let Some(LaneDrag::Marquee { from, .. }) = &self.view.panel(uid).drag
            && let Some(p) = pointer
        {
            let to = (geom.tick_at_clamped(p.x), geom.pitch_at_clamped(p.y));
            let rows = [from.1, to.1].map(|q| geom.row_of(q).unwrap_or(0));
            let r = Rect::from_min_max(
                pos2(
                    geom.x_of(from.0.min(to.0)),
                    geom.row_top(rows[0].min(rows[1])),
                ),
                pos2(
                    geom.x_of(from.0.max(to.0) + 1),
                    geom.row_top(rows[0].max(rows[1]) + 1),
                ),
            );
            grid.rect_filled(r, 0.0, theme::ACID_LIGHT.gamma_multiply(0.12));
            grid.rect_stroke(
                r,
                0.0,
                Stroke::new(1.0_f32, theme::ACID_LIGHT),
                StrokeKind::Inside,
            );
        }

        if view.strip {
            // Strip edits change fields in place: the selection follows.
            let before = std::mem::take(&mut frame.changed);
            let g = &geom;
            match (self.view.panel(uid).strip_mode, kind) {
                (StripMode::Chance, _) => {
                    self.bar_strip(ui, uid, g, width, &mut pattern, &mut frame, Bar::Chance)
                }
                (StripMode::Ratchet, _) => {
                    self.bar_strip(ui, uid, g, width, &mut pattern, &mut frame, Bar::Ratchet)
                }
                (StripMode::Expr, LaneKind::Poly | LaneKind::Pads) => {
                    self.bar_strip(ui, uid, g, width, &mut pattern, &mut frame, Bar::Vel)
                }
                (StripMode::Expr, LaneKind::Acid) => {
                    for (label, flag) in [("Acc", Flag::Accent), ("Slide", Flag::Slide)] {
                        flag_strip(ui, g, width, &mut pattern, label, flag, &mut frame);
                    }
                }
                (StripMode::Expr, LaneKind::Drums) => {
                    drum_accent_strip(ui, g, width, snap, &mut pattern, &mut frame)
                }
            }
            if frame.changed && !locked {
                self.set_selection(uid, note_ops::values(&pattern, &sel));
            }
            frame.changed |= before;
        }
        if frame.pressed {
            // As on the grid: a press in a strip focuses the lane.
            self.view.focus = Some(uid);
            self.view.dock.keyboard = true;
        }
        self.lane_handle(ui, uid, height, width);

        self.audition(uid, key_down);
        if locked {
            // Whatever the strip did this frame is dropped.
            if matches!(self.view.panel(uid).drag, Some(LaneDrag::Velocity)) {
                self.view.panel(uid).drag = None;
            }
            return;
        }
        // A press here moved the focus off an open tool's lane: it is
        // cancelled before this lane's edit lands.
        if self.view.focus == Some(uid) && self.tool_lane().is_some_and(|(u, _)| u != uid) {
            self.cancel_tool();
        }
        self.commit_poly_frame(
            uid,
            slot,
            pattern,
            frame.changed,
            frame.dropped,
            frame.finished,
        );
    }

    /// The grid's clicks and drags, around the selection `sel` (sorted
    /// indices into `pattern`, kept up to date here). Edits go into
    /// `pattern` and `frame`; selection changes alone edit nothing.
    #[allow(clippy::too_many_arguments)]
    fn lane_mouse(
        &mut self,
        ui: &egui::Ui,
        uid: MachineUid,
        resp: &egui::Response,
        geom: &LaneGeom,
        pattern: &mut NotePattern,
        sel: &mut Vec<usize>,
        frame: &mut Frame,
    ) {
        let kind = self.lane_kind(uid);
        let snap = self.view.dock.snap;
        // Snap to scale, on melodic lanes only.
        let scale = self.snap_key(kind);
        let primary = egui::PointerButton::Primary;
        let mods = ui.input(|i| i.modifiers);
        let slot = self.selected(uid);
        // Another pattern was picked under a move or resize (a pad, a song
        // step): its origin belongs to the old one, so the drag is dropped
        // with nothing more committed, and its undo step is closed.
        if let Some(LaneDrag::Move { slot: s, .. } | LaneDrag::Resize { slot: s, .. }) =
            &self.view.panel(uid).drag
            && *s != slot
        {
            self.view.panel(uid).drag = None;
            self.doc.end_gesture();
        }
        let pointer = resp
            .interact_pointer_pos()
            .or_else(|| ui.input(|i| i.pointer.latest_pos()));
        // Drags hit-test where the press began, not where the pointer is
        // once the drag is recognised.
        let press = ui.input(|i| i.pointer.press_origin()).or(pointer);
        let cell_at = |p: egui::Pos2| Some((geom.tick_at(p.x)?, geom.pitch_at(p.y)?));
        let hit_at = |pattern: &NotePattern, p: egui::Pos2| {
            cell_at(p).and_then(|(tick, pitch)| note_at(pattern, tick, pitch))
        };
        let toggle = |sel: &mut Vec<usize>, i: usize| match sel.binary_search(&i) {
            Ok(k) => {
                sel.remove(k);
            }
            Err(k) => sel.insert(k, i),
        };

        // Before the click: the click then selects the toggled hit.
        if kind.is_rows()
            && resp.double_clicked()
            && let Some(p) = pointer
            && let Some(i) = hit_at(pattern, p)
        {
            pattern.notes[i].accent = !pattern.notes[i].accent;
            frame.changed = true;
            frame.finished = true;
        }
        if resp.clicked()
            && let Some(p) = pointer
            && let Some((tick, pitch)) = cell_at(p)
        {
            match note_at(pattern, tick, pitch) {
                Some(i) if mods.shift => toggle(sel, i),
                Some(i) => *sel = vec![i],
                // Shift-click on empty space is ignored.
                None if mods.shift => {}
                None => {
                    let pitch = scale.map_or(pitch, |k| k.snap(pitch));
                    if let Some(i) = add(pattern, snap.floor(tick), snap.ticks(), pitch, NEW_VEL) {
                        *sel = note_ops::settle(pattern, &[i], kind);
                        frame.changed = true;
                    }
                    frame.finished = true;
                }
            }
            self.set_selection(uid, note_ops::values(pattern, sel));
        }
        if resp.secondary_clicked()
            && let Some(p) = pointer
            && let Some(i) = hit_at(pattern, p)
        {
            if sel.contains(&i) {
                note_ops::delete(pattern, sel);
                sel.clear();
                self.set_selection(uid, Vec::new());
            } else {
                // The selection is kept by value, so it survives this.
                note_ops::delete(pattern, &[i]);
            }
            frame.changed = true;
            frame.finished = true;
        }

        if resp.drag_started_by(primary)
            && let Some(p) = press
            && p.x >= geom.left
        {
            let from = (geom.tick_at_clamped(p.x), geom.pitch_at_clamped(p.y));
            let drag = match hit_at(pattern, p) {
                None => Some(LaneDrag::Marquee {
                    from,
                    add: mods.shift,
                }),
                Some(i) => {
                    // The grabbed note joins (Shift) or replaces the selection.
                    if sel.binary_search(&i).is_err() {
                        if mods.shift {
                            toggle(sel, i);
                        } else {
                            *sel = vec![i];
                        }
                    }
                    let grabbed = pattern.notes[i];
                    if kind.resizable() && geom.near_right_edge(&grabbed, p.x) {
                        Some(LaneDrag::Resize {
                            origin: Box::new(pattern.clone()),
                            slot,
                            idx: sel.clone(),
                            grab: grabbed.start.saturating_add(grabbed.len),
                        })
                    } else if mods.command || mods.ctrl {
                        // Copies move; the originals stay.
                        let copies = note_ops::values(pattern, sel);
                        if pattern.notes.len() + copies.len() > MAX_NOTES {
                            self.view.message = Some(note_ops::too_many_notes());
                            None
                        } else {
                            let first = pattern.notes.len();
                            pattern.notes.extend(copies);
                            *sel = (first..pattern.notes.len()).collect();
                            frame.changed = true;
                            Some(LaneDrag::Move {
                                origin: Box::new(pattern.clone()),
                                slot,
                                idx: sel.clone(),
                                grab: from,
                            })
                        }
                    } else {
                        Some(LaneDrag::Move {
                            origin: Box::new(pattern.clone()),
                            slot,
                            idx: sel.clone(),
                            grab: from,
                        })
                    }
                }
            };
            if !matches!(drag, Some(LaneDrag::Marquee { .. })) {
                self.set_selection(uid, note_ops::values(pattern, sel));
            }
            self.view.panel(uid).drag = drag;
        }

        // Each frame recomputes the drag from its origin.
        if resp.dragged_by(primary)
            && let Some(p) = pointer
        {
            let (tick, pitch) = (geom.tick_at_clamped(p.x), geom.pitch_at_clamped(p.y));
            // Alt: single ticks; else whole snap cells.
            let floor = |t: u16| if mods.alt { t } else { snap.floor(t) };
            let unit = if mods.alt { 1 } else { snap.ticks() };
            let moved = match &self.view.panel(uid).drag {
                Some(LaneDrag::Move {
                    origin, idx, grab, ..
                }) => {
                    let mut p = (**origin).clone();
                    let dt = floor(tick) as i32 - floor(grab.0) as i32;
                    // With snap to scale the grabbed note follows the
                    // pointer's snapped pitch; the rest then snap too.
                    let to = scale.map_or(pitch, |k| k.snap(pitch));
                    let dp = to as i32 - grab.1 as i32;
                    note_ops::shift(&mut p, idx, dt, dp, kind);
                    if let Some(k) = scale {
                        note_ops::snap_pitches(&mut p, idx, k);
                    }
                    Some((p, idx.clone()))
                }
                Some(LaneDrag::Resize {
                    origin, idx, grab, ..
                }) => {
                    // A resize may reach past the pattern's end: the
                    // pattern grows (up to its longest) to hold the notes,
                    // and dragging back gives the length back.
                    let far = geom.tick_at_upto(p.x, STEPS as u16 * TICKS_PER_STEP);
                    let end = floor(far) + unit;
                    let mut p = (**origin).clone();
                    note_ops::resize(&mut p, idx, end as i32 - *grab as i32);
                    note_ops::grow_to_fit(&mut p, idx);
                    Some((p, idx.clone()))
                }
                _ => None,
            };
            if let Some((p, idx)) = moved {
                if p != *pattern {
                    *pattern = p;
                    frame.changed = true;
                }
                *sel = idx;
                self.set_selection(uid, note_ops::values(pattern, sel));
            }
        }

        let panel = self.view.panel(uid);
        if resp.drag_stopped_by(primary) && !matches!(panel.drag, Some(LaneDrag::Velocity)) {
            match panel.drag.take() {
                Some(LaneDrag::Move { idx, .. } | LaneDrag::Resize { idx, .. }) => {
                    frame.dropped = Some(idx);
                }
                Some(LaneDrag::Marquee { from, add }) => {
                    if let Some(p) = pointer {
                        let to = (geom.tick_at_clamped(p.x), geom.pitch_at_clamped(p.y));
                        let hits = note_ops::in_box(pattern, from.0, to.0, from.1, to.1);
                        if !add {
                            sel.clear();
                        }
                        sel.extend(hits);
                        sel.sort_unstable();
                        sel.dedup();
                        self.set_selection(uid, note_ops::values(pattern, sel));
                    }
                }
                Some(LaneDrag::Velocity) | None => {}
            }
            frame.finished = true;
        }
    }

    /// The mouse wheel over a lane's grid: vertical scrolls the pitches of
    /// a melodic lane, horizontal (or Shift+vertical) the dock. The wheel
    /// is consumed so the area the lane sits in does not scroll too.
    pub(super) fn lane_wheel(&mut self, ui: &egui::Ui, uid: MachineUid, melodic: bool, max_y: f32) {
        let (delta, shift) = ui.input(|i| (i.raw_scroll_delta, i.modifiers.shift));
        let (mut dx, mut dy) = (delta.x, delta.y);
        // Shift turns a vertical wheel horizontal (egui mostly does this
        // already); the vertical axis is then used up too.
        let shifted = shift && dx == 0.0 && dy != 0.0;
        if shifted {
            (dx, dy) = (dy, 0.0);
        }
        if dx != 0.0 {
            self.view.dock.scroll_x = (self.view.dock.scroll_x - dx).max(0.0);
        }
        if dy != 0.0 && melodic {
            let y = self.view.panel(uid).lane_scroll_y.get_or_insert(0.0);
            *y = (*y - dy).clamp(0.0, max_y);
        }
        // Consume only the axes used: the horizontal always, the vertical
        // not on a drum lane, which leaves it to the area it sits in.
        let take_y = melodic || shifted;
        ui.input_mut(|i| {
            i.raw_scroll_delta.x = 0.0;
            i.smooth_scroll_delta.x = 0.0;
            if take_y {
                i.raw_scroll_delta.y = 0.0;
                i.smooth_scroll_delta.y = 0.0;
            }
        });
    }

    /// A bar strip (velocity, chance or ratchet): click or drag sets the
    /// value of the notes starting in the snap cell under the pointer.
    #[allow(clippy::too_many_arguments)]
    fn bar_strip(
        &mut self,
        ui: &mut egui::Ui,
        uid: MachineUid,
        geom: &LaneGeom,
        width: f32,
        pattern: &mut NotePattern,
        frame: &mut Frame,
        bar: Bar,
    ) {
        let snap = self.view.dock.snap;
        let (lane, lresp) = strip_row(
            ui,
            geom,
            width,
            VEL_H,
            Sense::click_and_drag(),
            bar.label(),
            frame,
        );
        let painter = ui.painter_at(lane);
        let hi = bar.range().1 as f32;
        for note in pattern.notes.iter().filter(|n| n.start < geom.ticks) {
            let x = geom.x_of(note.start);
            // At least a stub, so a note at chance 0 still shows.
            let h = (VEL_H * bar.get(note) as f32 / hi).max(2.0);
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
            self.view.panel(uid).drag = Some(LaneDrag::Velocity);
        }
        if (lresp.clicked() || lresp.dragged())
            && let Some(p) = lresp.interact_pointer_pos()
            && let Some(tick) = geom.tick_at(p.x)
        {
            let from = snap.floor(tick);
            let to = (from + snap.ticks()).min(geom.ticks);
            let v = bar.value_at(p.y, lane.top());
            frame.changed |= if bar == Bar::Vel {
                set_velocity_in(pattern, from, to, v)
            } else {
                let idx: Vec<usize> = (0..pattern.notes.len())
                    .filter(|&i| (from..to).contains(&pattern.notes[i].start))
                    .collect();
                note_ops::set_field(pattern, &idx, bar.field(v))
            };
            frame.finished |= lresp.clicked();
        }
        if lresp.drag_stopped() {
            self.view.panel(uid).drag = None;
            frame.finished = true;
        }
    }

    /// The strip under a lane that drags its height.
    pub(super) fn lane_handle(
        &mut self,
        ui: &mut egui::Ui,
        uid: MachineUid,
        height: f32,
        width: f32,
    ) {
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

    /// Commits one frame of a lane. On the frame a move/resize is dropped
    /// (`dropped` = the dragged notes' indices) they settle against their
    /// neighbours first (`note_ops::settle`) and become the selection, and
    /// the frame counts as still dragging: `drag` is already cleared by
    /// then, but the drop's edits must merge into the drag's undo step, not
    /// push a second one.
    fn commit_poly_frame(
        &mut self,
        uid: MachineUid,
        slot: u8,
        mut pattern: NotePattern,
        mut changed: bool,
        dropped: Option<Vec<usize>>,
        finished: bool,
    ) {
        let dragging = dropped.is_some() || self.view.panel(uid).drag.is_some();
        if let Some(idx) = dropped {
            let before = pattern.clone();
            let kept = note_ops::settle(&mut pattern, &idx, self.lane_kind(uid));
            changed |= pattern != before;
            self.set_selection(uid, note_ops::values(&pattern, &kept));
        }
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
            open |= panel.drag.take().is_some();
            open |= panel.song_drag.take().is_some();
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
/// (right of the key strip) and its response. A press on the row is
/// noted in `frame`.
fn strip_row(
    ui: &mut egui::Ui,
    geom: &LaneGeom,
    width: f32,
    h: f32,
    sense: Sense,
    label: &str,
    frame: &mut Frame,
) -> (Rect, egui::Response) {
    let (row, resp) = ui.allocate_exact_size(vec2(width, h), sense);
    frame.pressed |= resp.is_pointer_button_down_on() || resp.clicked();
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
    let (lane, lresp) = strip_row(ui, geom, width, FLAG_H, Sense::click(), label, frame);
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
        && let Some(tick) = geom.tick_at(p.x)
    {
        frame.changed |= toggle_flag(pattern, tick, flag);
        frame.finished = true;
    }
}

/// A drum lane's accent row: a snap cell is marked when any hit starting
/// in it is accented; a click accents every hit starting in the cell, or
/// clears them when all already are.
fn drum_accent_strip(
    ui: &mut egui::Ui,
    geom: &LaneGeom,
    width: f32,
    snap: Snap,
    pattern: &mut NotePattern,
    frame: &mut Frame,
) {
    let (lane, lresp) = strip_row(ui, geom, width, FLAG_H, Sense::click(), "Acc", frame);
    let painter = ui.painter_at(lane);
    let cell_w = snap.ticks() as f32 * geom.tick_w;
    for note in pattern
        .notes
        .iter()
        .filter(|n| n.accent && n.start < geom.ticks)
    {
        let x = geom.x_of(snap.floor(note.start));
        painter.rect_filled(
            Rect::from_min_max(
                pos2(x, lane.top() + 1.0),
                pos2(x + (cell_w - 1.0).max(2.0), lane.bottom() - 1.0),
            ),
            0.0,
            theme::ACCENT,
        );
    }
    if lresp.clicked()
        && let Some(p) = lresp.interact_pointer_pos()
        && let Some(tick) = geom.tick_at(p.x)
    {
        let from = snap.floor(tick);
        frame.changed |= toggle_accent_in(pattern, from, from + snap.ticks());
        frame.finished = true;
    }
}

/// The middle of the pitch range of `pattern`'s notes that start inside
/// it, where a lane's first view centres (None with no such note).
fn middle_pitch(pattern: &NotePattern) -> Option<u8> {
    let ticks = pattern.ticks();
    let mut inside = pattern.notes.iter().filter(|n| n.start < ticks);
    let first = inside.next()?.pitch;
    let (lo, hi) = inside.fold((first, first), |(lo, hi), n| {
        (lo.min(n.pitch), hi.max(n.pitch))
    });
    Some(((lo as u16 + hi as u16) / 2) as u8)
}

/// The scroll that puts `pitch`'s row (or the one it is drawn on) in the
/// middle of a `height` view of `rows`.
fn centred_y(rows: &Rows, height: f32, pitch: u8) -> f32 {
    let row = rows.row_near(pitch);
    let max = (rows.count() as f32 * ROW_H - height).max(0.0);
    (row as f32 * ROW_H + ROW_H / 2.0 - height / 2.0).clamp(0.0, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::test_rack::*;
    use acid_engine::project::Project;

    /// Runs one frame of KIT808's lane in a 1000x800 screen at `time`;
    /// returns where the lane's top-left corner was drawn.
    fn kit_frame(
        app: &mut SidAcidApp,
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
    ) -> egui::Pos2 {
        lane_frame(app, ctx, KIT808, time, events)
    }

    /// Like `kit_frame`, for machine `uid`'s lane.
    fn lane_frame(
        app: &mut SidAcidApp,
        ctx: &egui::Context,
        uid: MachineUid,
        time: f64,
        events: Vec<egui::Event>,
    ) -> egui::Pos2 {
        lane_frame_mods(app, ctx, uid, time, events, egui::Modifiers::NONE)
    }

    /// Like `lane_frame`, with `mods` held.
    fn lane_frame_mods(
        app: &mut SidAcidApp,
        ctx: &egui::Context,
        uid: MachineUid,
        time: f64,
        events: Vec<egui::Event>,
        mods: egui::Modifiers,
    ) -> egui::Pos2 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
            time: Some(time),
            events,
            modifiers: mods,
            ..Default::default()
        };
        let mut origin = pos2(0.0, 0.0);
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                origin = ui.cursor().min;
                app.lane(ui, uid, 800.0);
            });
        });
        origin
    }

    /// Mouse gestures in one machine's lane, with egui's clock.
    struct Mouse {
        ctx: egui::Context,
        uid: MachineUid,
        time: f64,
        origin: egui::Pos2,
    }

    impl Mouse {
        /// Draws `uid`'s lane once (melodic lanes scrolled so `top` is the
        /// top row) to find where it sits.
        fn new(app: &mut SidAcidApp, uid: MachineUid, top: u8) -> Mouse {
            if !app.lane_kind(uid).is_rows() {
                app.view.panel(uid).lane_scroll_y = Some((127 - top) as f32 * ROW_H);
            }
            let ctx = egui::Context::default();
            let origin = lane_frame(app, &ctx, uid, 0.0, vec![]);
            Mouse {
                ctx,
                uid,
                time: 0.0,
                origin,
            }
        }

        /// Screen position of `tick` (fractions allowed) in `pitch`'s row.
        fn at(&self, app: &mut SidAcidApp, tick: f32, pitch: u8) -> egui::Pos2 {
            let tw = tick_w(ZOOMS[app.view.dock.zoom]);
            let x = self.origin.x + app.lane_key_w() + tick * tw - app.view.dock.scroll_x;
            let kind = app.lane_kind(self.uid);
            let (row, row_h, scroll) = if kind.is_rows() {
                // Rows shrink to fit the lane, as `lane_body` draws them.
                let rows = kind.top_pitch() as f32 + 1.0;
                let height = app
                    .doc
                    .project
                    .machine(self.uid)
                    .unwrap()
                    .lane
                    .clamped_height();
                let row_h = DRUM_ROW_H.min(height as f32 / rows);
                ((kind.top_pitch() - pitch) as usize, row_h, 0.0)
            } else {
                let scroll = app.view.panel(self.uid).lane_scroll_y.unwrap_or(0.0);
                (127 - pitch as usize, ROW_H, scroll)
            };
            let y = self.origin.y + HEADER_H + row as f32 * row_h - scroll + row_h / 2.0;
            pos2(x, y)
        }

        fn frame(&mut self, app: &mut SidAcidApp, events: Vec<egui::Event>, mods: egui::Modifiers) {
            // Well apart, so no two clicks make a double click.
            self.time += 0.5;
            lane_frame_mods(app, &self.ctx, self.uid, self.time, events, mods);
        }

        /// Like `frame`, with the app's shortcuts run first (as `ui` does).
        fn frame_keys(&mut self, app: &mut SidAcidApp, events: Vec<egui::Event>) {
            self.time += 0.5;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let uid = self.uid;
            let _ = self.ctx.run(input, |ctx| {
                app.handle_shortcuts(ctx);
                egui::CentralPanel::default().show(ctx, |ui| app.lane(ui, uid, 800.0));
            });
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

    /// An app whose `uid` pattern 0 holds only `notes` (start, len, pitch).
    fn app_with(uid: MachineUid, notes: &[(u16, u16, u8)]) -> SidAcidApp {
        let mut doc = Document::new(Project::default());
        doc.project.roll_mut(uid, 0).unwrap().notes = notes
            .iter()
            .map(|&(s, l, p)| acid_engine::project::Note::new(s, l, p, 100))
            .collect();
        SidAcidApp::headless(doc)
    }

    fn notes_of(app: &SidAcidApp, uid: MachineUid) -> Vec<(u16, u16, u8)> {
        let roll = app.doc.project.roll(uid, 0).unwrap();
        roll.notes
            .iter()
            .map(|n| (n.start, n.len, n.pitch))
            .collect()
    }

    const NONE: egui::Modifiers = egui::Modifiers::NONE;

    #[test]
    fn clicking_a_note_selects_it_and_shift_click_toggles() {
        let mut app = app_with(FM, &[(0, 4, 60), (8, 4, 62)]);
        let mut m = Mouse::new(&mut app, FM, 64);
        let (a, b) = (m.at(&mut app, 1.5, 60), m.at(&mut app, 9.5, 62));
        m.click(&mut app, a, NONE);
        assert_eq!(app.selection(FM), [0]);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        assert_eq!(app.selection(FM), [0, 1]);
        m.click(&mut app, a, egui::Modifiers::SHIFT);
        assert_eq!(app.selection(FM), [1]);
        // Shift-click on empty space adds nothing and keeps the selection.
        let empty = m.at(&mut app, 20.5, 61);
        m.click(&mut app, empty, egui::Modifiers::SHIFT);
        assert_eq!(app.selection(FM), [1]);
        m.click(&mut app, a, NONE);
        assert_eq!(app.selection(FM), [0]);
        assert_eq!(notes_of(&app, FM), [(0, 4, 60), (8, 4, 62)]);
        assert!(!app.doc.is_dirty(), "selecting edits nothing");
        assert!(!app.doc.can_undo());
        assert_eq!(app.view.focus, Some(FM));
    }

    #[test]
    fn clicking_empty_space_adds_a_selected_note_one_snap_long() {
        let mut app = app_with(FM, &[(0, 4, 60)]);
        let mut m = Mouse::new(&mut app, FM, 64);
        let at = m.at(&mut app, 9.5, 61);
        m.click(&mut app, at, NONE);
        assert_eq!(notes_of(&app, FM), [(0, 4, 60), (8, 4, 61)]);
        assert_eq!(app.selection(FM), [1]);
        app.doc.undo();
        assert_eq!(notes_of(&app, FM), [(0, 4, 60)], "one undo step");
    }

    fn snap_to(app: &mut SidAcidApp, scale: Scale) {
        app.doc.project.key = acid_engine::scale::Key {
            root: 0,
            scale,
            snap: true,
        };
    }

    #[test]
    fn with_snap_to_scale_a_click_adds_on_the_scale() {
        let mut app = app_with(FM, &[]);
        snap_to(&mut app, Scale::Major);
        let mut m = Mouse::new(&mut app, FM, 64);
        // C# is off C major: the note lands on D (ties go up).
        let at = m.at(&mut app, 9.5, 61);
        m.click(&mut app, at, NONE);
        assert_eq!(notes_of(&app, FM), [(8, 4, 62)]);
        // Chromatic snaps nothing.
        snap_to(&mut app, Scale::Chromatic);
        let at = m.at(&mut app, 17.5, 61);
        m.click(&mut app, at, NONE);
        assert_eq!(notes_of(&app, FM), [(8, 4, 62), (16, 4, 61)]);
    }

    #[test]
    fn with_snap_to_scale_a_drag_follows_the_snapped_pointer() {
        let mut app = app_with(FM, &[(0, 4, 60), (8, 4, 64)]);
        snap_to(&mut app, Scale::Major);
        let mut m = Mouse::new(&mut app, FM, 70);
        let (a, b) = (m.at(&mut app, 1.5, 60), m.at(&mut app, 9.5, 64));
        m.click(&mut app, a, NONE);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        // The pointer ends on C# (61), which snaps to D: the grabbed note
        // moves +2 to D, and the other (E + 2 = F#) snaps up to G.
        let to = m.at(&mut app, 1.5, 61);
        m.drag(&mut app, a, to, NONE);
        assert_eq!(notes_of(&app, FM), [(0, 4, 62), (8, 4, 67)]);
        app.doc.undo();
        assert_eq!(
            notes_of(&app, FM),
            [(0, 4, 60), (8, 4, 64)],
            "one undo step"
        );
    }

    #[test]
    fn drum_lanes_ignore_the_key() {
        let mut app = app_with(KIT808, &[]);
        snap_to(&mut app, Scale::Major);
        let mut m = Mouse::new(&mut app, KIT808, 0);
        // Instrument 1 would be C#, off C major: it stays put.
        let at = m.at(&mut app, 9.5, 1);
        m.click(&mut app, at, NONE);
        assert_eq!(notes_of(&app, KIT808), [(8, 4, 1)]);
        let from = m.at(&mut app, 9.5, 1);
        let to = m.at(&mut app, 9.5, 3);
        m.drag(&mut app, from, to, NONE);
        assert_eq!(
            notes_of(&app, KIT808),
            [(8, 4, 3)],
            "D# is off the scale too"
        );
    }

    #[test]
    fn dragging_on_empty_space_box_selects() {
        let mut app = app_with(FM, &[(0, 4, 60), (8, 4, 62), (24, 4, 60)]);
        let mut m = Mouse::new(&mut app, FM, 64);
        let (from, to) = (m.at(&mut app, 1.5, 63), m.at(&mut app, 12.5, 59));
        m.drag(&mut app, from, to, NONE);
        assert_eq!(app.selection(FM), [0, 1]);
        // Shift adds the box to the selection; without it the box replaces it.
        let (from, to) = (m.at(&mut app, 22.5, 61), m.at(&mut app, 30.5, 59));
        m.drag(&mut app, from, to, egui::Modifiers::SHIFT);
        assert_eq!(app.selection(FM), [0, 1, 2]);
        m.drag(&mut app, from, to, NONE);
        assert_eq!(app.selection(FM), [2]);
        assert_eq!(notes_of(&app, FM), [(0, 4, 60), (8, 4, 62), (24, 4, 60)]);
        assert!(!app.doc.is_dirty(), "a box edits nothing");
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn dragging_the_selection_moves_every_selected_note_in_one_undo_step() {
        let mut app = app_with(FM, &[(0, 4, 60), (8, 4, 62), (24, 4, 50)]);
        let mut m = Mouse::new(&mut app, FM, 64);
        let (a, b) = (m.at(&mut app, 1.5, 60), m.at(&mut app, 9.5, 62));
        m.click(&mut app, a, NONE);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        // Two steps right (snapped) and one semitone up.
        let to = m.at(&mut app, 10.5, 61);
        m.drag(&mut app, a, to, NONE);
        assert_eq!(notes_of(&app, FM), [(8, 4, 61), (16, 4, 63), (24, 4, 50)]);
        assert_eq!(app.selection(FM), [0, 1], "the moved notes stay selected");
        assert!(app.view.panel(FM).drag.is_none());
        app.doc.undo();
        assert_eq!(
            notes_of(&app, FM),
            [(0, 4, 60), (8, 4, 62), (24, 4, 50)],
            "one undo step"
        );
    }

    #[test]
    fn alt_dragging_moves_by_single_ticks() {
        let mut app = app_with(FM, &[(0, 4, 60)]);
        let mut m = Mouse::new(&mut app, FM, 64);
        let (from, to) = (m.at(&mut app, 1.5, 60), m.at(&mut app, 4.5, 60));
        m.drag(&mut app, from, to, egui::Modifiers::ALT);
        assert_eq!(notes_of(&app, FM), [(3, 4, 60)]);
    }

    #[test]
    fn ctrl_dragging_leaves_the_originals_and_moves_copies() {
        let mut app = app_with(FM, &[(0, 4, 60), (8, 4, 62)]);
        let mut m = Mouse::new(&mut app, FM, 64);
        let a = m.at(&mut app, 1.5, 60);
        m.click(&mut app, a, NONE);
        let to = m.at(&mut app, 17.5, 60);
        m.drag(&mut app, a, to, egui::Modifiers::CTRL);
        assert_eq!(notes_of(&app, FM), [(0, 4, 60), (8, 4, 62), (16, 4, 60)]);
        assert_eq!(app.selection(FM), [2], "the copy is selected");
        app.doc.undo();
        assert_eq!(
            notes_of(&app, FM),
            [(0, 4, 60), (8, 4, 62)],
            "one undo step"
        );
    }

    #[test]
    fn ctrl_dragging_past_the_note_cap_is_refused() {
        use acid_engine::project::{MAX_NOTES, Note};
        let mut app = app_with(FM, &[(0, 4, 60)]);
        let p = app.doc.project.roll_mut(FM, 0).unwrap();
        p.notes
            .extend((1..MAX_NOTES).map(|i| Note::new(8 + (i % 200) as u16, 1, 20, 100)));
        let before = app.doc.project.clone();
        let mut m = Mouse::new(&mut app, FM, 64);
        let (a, to) = (m.at(&mut app, 1.5, 60), m.at(&mut app, 1.5, 62));
        m.drag(&mut app, a, to, egui::Modifiers::CTRL);
        assert_eq!(
            app.view.message.as_deref(),
            Some("That would put more than 512 notes in the pattern")
        );
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn resizing_one_selected_note_resizes_them_all() {
        let mut app = app_with(FM, &[(0, 4, 60), (8, 4, 62)]);
        let mut m = Mouse::new(&mut app, FM, 64);
        let (a, b) = (m.at(&mut app, 1.5, 60), m.at(&mut app, 9.5, 62));
        m.click(&mut app, a, NONE);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        // From A's right edge to the step holding tick 9: A ends at 12.
        let (edge, to) = (m.at(&mut app, 3.7, 60), m.at(&mut app, 9.5, 60));
        m.drag(&mut app, edge, to, NONE);
        assert_eq!(notes_of(&app, FM), [(0, 12, 60), (8, 12, 62)]);
        assert_eq!(app.selection(FM), [0, 1]);
        app.doc.undo();
        assert_eq!(
            notes_of(&app, FM),
            [(0, 4, 60), (8, 4, 62)],
            "one undo step"
        );
    }

    #[test]
    fn pad_hits_resize_and_double_clicks_accent_them() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app
            .add_machine(acid_engine::machine::MachineType::PadKit)
            .unwrap();
        app.doc.project.roll_mut(uid, 0).unwrap().notes =
            vec![acid_engine::project::Note::new(0, 4, 12, 100)];
        let mut m = Mouse::new(&mut app, uid, 0);
        let (edge, to) = (m.at(&mut app, 3.7, 12), m.at(&mut app, 9.5, 12));
        m.drag(&mut app, edge, to, NONE);
        assert_eq!(notes_of(&app, uid), [(0, 12, 12)], "the right edge resizes");
        let hit = m.at(&mut app, 1.5, 12);
        let press = |pressed| Mouse::button(hit, egui::PointerButton::Primary, pressed, NONE);
        m.frame(&mut app, vec![egui::Event::PointerMoved(hit)], NONE);
        let t = m.time;
        for (k, pressed) in [true, false, true, false].into_iter().enumerate() {
            lane_frame(
                &mut app,
                &m.ctx,
                uid,
                t + 0.05 * (k + 1) as f64,
                vec![press(pressed)],
            );
        }
        lane_frame(&mut app, &m.ctx, uid, t + 0.3, vec![]);
        assert!(app.doc.project.roll(uid, 0).unwrap().notes[0].accent);
    }

    #[test]
    fn right_clicking_a_selected_note_deletes_the_selection() {
        let secondary = egui::PointerButton::Secondary;
        let mut app = app_with(FM, &[(0, 4, 60), (8, 4, 62), (24, 4, 52)]);
        let mut m = Mouse::new(&mut app, FM, 64);
        let (a, b, c) = (
            m.at(&mut app, 1.5, 60),
            m.at(&mut app, 9.5, 62),
            m.at(&mut app, 25.5, 52),
        );
        m.click(&mut app, a, NONE);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        // Another note: only it goes; the selection stays.
        m.click_with(&mut app, c, secondary, NONE);
        assert_eq!(notes_of(&app, FM), [(0, 4, 60), (8, 4, 62)]);
        assert_eq!(app.selection(FM), [0, 1]);
        app.doc.undo();
        assert_eq!(notes_of(&app, FM).len(), 3);
        // A selected note: the whole selection goes, in one undo step.
        m.click_with(&mut app, a, secondary, NONE);
        assert_eq!(notes_of(&app, FM), [(24, 4, 52)]);
        assert!(app.selection(FM).is_empty());
        app.doc.undo();
        assert_eq!(notes_of(&app, FM), [(0, 4, 60), (8, 4, 62), (24, 4, 52)]);
    }

    #[test]
    fn moving_onto_an_acid_note_keeps_the_moved_note() {
        let mut app = app_with(SYNTH1, &[(0, 8, 36), (16, 4, 40)]);
        let mut m = Mouse::new(&mut app, SYNTH1, 44);
        // Drag the note at 16 to start at tick 4, inside the first note.
        let (from, to) = (m.at(&mut app, 17.5, 40), m.at(&mut app, 5.5, 40));
        m.drag(&mut app, from, to, NONE);
        assert_eq!(notes_of(&app, SYNTH1), [(0, 4, 36), (4, 4, 40)], "cut");
        assert_eq!(app.selection(SYNTH1), [1]);
        app.doc.undo();
        assert_eq!(notes_of(&app, SYNTH1), [(0, 8, 36), (16, 4, 40)]);
    }

    #[test]
    fn resizing_past_the_pattern_end_lengthens_the_pattern() {
        let mut app = app_with(FM, &[(56, 8, 60)]);
        let mut m = Mouse::new(&mut app, FM, 64);
        let len = |app: &SidAcidApp| app.doc.project.roll(FM, 0).unwrap().length;
        // From the note's right edge, at the pattern's end, five steps on.
        let (edge, to) = (m.at(&mut app, 63.7, 60), m.at(&mut app, 81.5, 60));
        m.drag(&mut app, edge, to, NONE);
        assert_eq!(notes_of(&app, FM), [(56, 28, 60)]);
        assert_eq!(len(&app), 21, "the pattern grows to hold the note");
        app.doc.undo();
        assert_eq!(notes_of(&app, FM), [(56, 8, 60)]);
        assert_eq!(len(&app), 16, "one undo step puts the length back");
        // Out past the end and back within one drag: the length is kept.
        let (edge, back) = (m.at(&mut app, 63.7, 60), m.at(&mut app, 59.5, 60));
        m.drag(&mut app, edge, back, NONE);
        assert_eq!(notes_of(&app, FM), [(56, 4, 60)]);
        assert_eq!(len(&app), 16, "a shorter note never shrinks the pattern");
    }

    #[test]
    fn resizing_an_acid_run_keeps_every_note() {
        let mut app = app_with(SYNTH1, &[(0, 4, 40), (4, 4, 41), (8, 4, 42)]);
        let mut m = Mouse::new(&mut app, SYNTH1, 44);
        let (a, b, c) = (
            m.at(&mut app, 1.5, 40),
            m.at(&mut app, 5.5, 41),
            m.at(&mut app, 9.5, 42),
        );
        m.click(&mut app, a, NONE);
        m.click(&mut app, b, egui::Modifiers::SHIFT);
        m.click(&mut app, c, egui::Modifiers::SHIFT);
        // From the first note's right edge one step on: every note +4.
        let (edge, to) = (m.at(&mut app, 3.7, 40), m.at(&mut app, 5.5, 40));
        m.drag(&mut app, edge, to, NONE);
        assert_eq!(
            notes_of(&app, SYNTH1),
            [(0, 4, 40), (4, 4, 41), (8, 8, 42)],
            "each note is cut by the next, none removed"
        );
        assert_eq!(app.selection(SYNTH1), [0, 1, 2]);
    }

    fn key(key: egui::Key, mods: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: mods,
        }
    }

    #[test]
    fn keys_during_a_mouse_drag_do_nothing() {
        let cmd = egui::Modifiers::COMMAND;
        let mut app = app_with(FM, &[(0, 4, 60), (40, 4, 62)]);
        app.view.dock.keyboard = true;
        app.view.focus = Some(FM);
        let mut m = Mouse::new(&mut app, FM, 64);
        let (from, to) = (m.at(&mut app, 1.5, 60), m.at(&mut app, 17.5, 60));
        let primary = egui::PointerButton::Primary;
        m.frame_keys(&mut app, vec![egui::Event::PointerMoved(from)]);
        m.frame_keys(&mut app, vec![Mouse::button(from, primary, true, NONE)]);
        let mid = |k: f32| from + (to - from) * k;
        m.frame_keys(&mut app, vec![egui::Event::PointerMoved(mid(0.25))]);
        m.frame_keys(&mut app, vec![egui::Event::PointerMoved(mid(0.5))]);
        assert!(matches!(
            app.view.panel(FM).drag,
            Some(LaneDrag::Move { .. })
        ));
        for k in [
            key(egui::Key::Delete, NONE),
            key(egui::Key::Z, NONE),
            key(egui::Key::ArrowUp, NONE),
            key(egui::Key::Z, cmd),
            key(egui::Key::X, cmd),
            key(egui::Key::D, cmd),
            key(egui::Key::A, cmd),
            key(egui::Key::V, cmd),
        ] {
            m.frame_keys(&mut app, vec![egui::Event::PointerMoved(mid(0.75)), k]);
        }
        m.frame_keys(&mut app, vec![egui::Event::PointerMoved(to)]);
        m.frame_keys(&mut app, vec![Mouse::button(to, primary, false, NONE)]);
        m.frame_keys(&mut app, vec![]);
        assert_eq!(notes_of(&app, FM), [(16, 4, 60), (40, 4, 62)], "dropped");
        assert_eq!(app.doc.undo_depth(), 1, "one undo step");
        assert!(app.view.note_clip.is_none(), "nothing cut");
    }

    #[test]
    fn a_drag_is_dropped_when_the_pattern_changes_under_it() {
        let mut app = app_with(FM, &[(0, 4, 60)]);
        let other = app.doc.project.roll(FM, 1).unwrap().clone();
        let mut m = Mouse::new(&mut app, FM, 64);
        let (from, to) = (m.at(&mut app, 1.5, 60), m.at(&mut app, 17.5, 60));
        let primary = egui::PointerButton::Primary;
        m.frame(&mut app, vec![egui::Event::PointerMoved(from)], NONE);
        m.frame(
            &mut app,
            vec![Mouse::button(from, primary, true, NONE)],
            NONE,
        );
        let mid = from + (to - from) * 0.5;
        m.frame(&mut app, vec![egui::Event::PointerMoved(mid)], NONE);
        assert!(app.view.panel(FM).drag.is_some());
        // A pattern pad (or a song step) picks pattern 1 mid-drag.
        app.doc.project.machine_mut(FM).unwrap().selected = 1;
        m.frame(&mut app, vec![egui::Event::PointerMoved(to)], NONE);
        assert!(app.view.panel(FM).drag.is_none(), "the drag let go");
        m.frame(
            &mut app,
            vec![Mouse::button(to, primary, false, NONE)],
            NONE,
        );
        m.frame(&mut app, vec![], NONE);
        assert_eq!(app.doc.project.roll(FM, 1).unwrap(), &other, "untouched");
    }

    #[test]
    fn selected_notes_of_another_pattern_are_no_selection() {
        let mut app = app_with(FM, &[(0, 4, 60)]);
        let n = app.doc.project.roll(FM, 0).unwrap().notes[0];
        app.set_selection(FM, vec![n]);
        assert_eq!(app.selection(FM), [0]);
        app.view.panel(FM).sel_slot = 1;
        assert!(app.selection(FM).is_empty());
    }

    #[test]
    fn a_press_in_any_strip_focuses_the_lane() {
        // Empty patterns, so no strip click edits anything.
        for (uid, strip_h) in [(KIT808, FLAG_H), (SYNTH1, FLAG_H), (FM, VEL_H)] {
            let mut doc = Document::new(Project::default());
            doc.project.roll_mut(uid, 0).unwrap().notes.clear();
            let mut app = SidAcidApp::headless(doc);
            let other = if uid == FM { KIT808 } else { FM };
            app.view.focus = Some(other);
            let ctx = egui::Context::default();
            let origin = lane_frame(&mut app, &ctx, uid, 0.0, vec![]);
            let height = LaneView::default().clamped_height() as f32;
            let at = pos2(
                origin.x + app.lane_key_w() + 30.0,
                origin.y + HEADER_H + height + strip_h / 2.0,
            );
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            lane_frame(
                &mut app,
                &ctx,
                uid,
                1.0,
                vec![egui::Event::PointerMoved(at)],
            );
            lane_frame(&mut app, &ctx, uid, 1.05, vec![button(true)]);
            assert_eq!(app.view.focus, Some(uid), "{uid:?} focused on the press");
            assert!(app.view.dock.keyboard, "{uid:?} takes the keyboard");
            lane_frame(&mut app, &ctx, uid, 1.1, vec![button(false)]);
            assert!(!app.doc.is_dirty(), "{uid:?}: nothing edited");
        }
    }

    /// The pitches of `uid`'s selected pattern whose rows its lane shows.
    fn pitches_in_view(app: &mut SidAcidApp, uid: MachineUid) -> (usize, usize) {
        let pattern = app
            .doc
            .project
            .roll(uid, app.selected(uid))
            .unwrap()
            .clone();
        let fold = app.doc.project.machine(uid).unwrap().lane.fold;
        let rows = app.lane_rows(uid, fold, &pattern);
        let height = app.doc.project.machine(uid).unwrap().lane.clamped_height() as f32;
        let top = app.view.panel(uid).lane_scroll_y.unwrap();
        let shown = pattern
            .notes
            .iter()
            .filter(|n| {
                let y = rows.row_near(n.pitch) as f32 * ROW_H;
                y >= top && y + ROW_H <= top + height
            })
            .count();
        (shown, pattern.notes.len())
    }

    #[test]
    fn a_lane_first_shows_its_notes() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let ctx = egui::Context::default();
        lane_frame(&mut app, &ctx, SYNTH2, 0.0, vec![]);
        lane_frame(&mut app, &ctx, SYNTH1, 0.0, vec![]);
        let (shown, all) = pitches_in_view(&mut app, SYNTH2);
        assert!(
            all > 0 && shown > 0,
            "Synth 2: {shown} of {all} notes in view"
        );
        let (shown, all) = pitches_in_view(&mut app, SYNTH1);
        assert!(shown * 2 > all, "Synth 1: {shown} of {all} notes in view");
    }

    #[test]
    fn an_empty_lane_first_centres_on_its_start_pitch() {
        let mut app = fm_app();
        let ctx = egui::Context::default();
        lane_frame(&mut app, &ctx, FM, 0.0, vec![]);
        let height = app.doc.project.machine(FM).unwrap().lane.clamped_height() as f32;
        let pitch = app.roll_start_pitch(FM);
        assert_eq!(
            app.view.panel(FM).lane_scroll_y,
            Some(centred_y(&Rows::Pitches, height, pitch))
        );
    }

    #[test]
    fn a_taller_lane_clamps_its_pitch_scroll() {
        let mut app = fm_app();
        app.doc.project.machine_mut(FM).unwrap().lane.height = 480;
        app.view.panel(FM).lane_scroll_y = Some(5000.0);
        let ctx = egui::Context::default();
        lane_frame(&mut app, &ctx, FM, 0.0, vec![]);
        let max = Rows::Pitches.count() as f32 * ROW_H - 480.0;
        assert_eq!(app.view.panel(FM).lane_scroll_y, Some(max));
    }

    #[test]
    fn clicking_a_scrolled_drum_lane_adds_one_hit_under_the_pointer() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.view.dock.scroll_x = 22.0; // one step at zoom 1
        let before = app.doc.project.roll(KIT808, 0).unwrap().clone();
        let ctx = egui::Context::default();
        let origin = kit_frame(&mut app, &ctx, 0.0, vec![]);
        // Tick 9 (step 2, scrolled one step left), instrument 0 (bottom row).
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        let x = origin.x + app.lane_key_w() + 9.0 * tw - 22.0 + 1.0;
        let y = origin.y + HEADER_H + 7.0 * DRUM_ROW_H + DRUM_ROW_H / 2.0;
        let at = pos2(x, y);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        kit_frame(&mut app, &ctx, 0.1, vec![egui::Event::PointerMoved(at)]);
        kit_frame(&mut app, &ctx, 0.2, vec![button(true)]);
        kit_frame(&mut app, &ctx, 0.3, vec![button(false)]);
        kit_frame(&mut app, &ctx, 0.4, vec![]);
        let roll = app.doc.project.roll(KIT808, 0).unwrap();
        let added: Vec<_> = roll
            .notes
            .iter()
            .filter(|n| !before.notes.contains(n))
            .collect();
        assert_eq!(added.len(), 1, "one hit added: {:?}", roll.notes);
        assert_eq!((added[0].start, added[0].len), (8, 4), "snapped to step 2");
        assert_eq!(added[0].pitch, 0);
        assert_eq!(app.view.focus, Some(KIT808));
        assert!(app.view.dock.keyboard);
        app.doc.undo();
        assert_eq!(app.doc.project.roll(KIT808, 0).unwrap(), &before);
    }

    /// Clicks (`clicks` times, quickly) at `at` in KIT808's lane, from
    /// egui time `time` on.
    fn click_kit(
        app: &mut SidAcidApp,
        ctx: &egui::Context,
        at: egui::Pos2,
        clicks: usize,
        mut time: f64,
    ) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        kit_frame(app, ctx, time, vec![egui::Event::PointerMoved(at)]);
        for _ in 0..clicks {
            for pressed in [true, false] {
                time += 0.05;
                kit_frame(app, ctx, time, vec![button(pressed)]);
            }
        }
        kit_frame(app, ctx, time + 0.05, vec![]);
    }

    /// A kit app whose pattern 0 holds only hits at tick 8 on `insts`.
    fn kit_with_hits(insts: &[u8]) -> SidAcidApp {
        let mut doc = Document::new(Project::default());
        let roll = doc.project.roll_mut(KIT808, 0).unwrap();
        roll.notes = insts
            .iter()
            .map(|&i| acid_engine::project::Note::new(8, 4, i, 100))
            .collect();
        SidAcidApp::headless(doc)
    }

    #[test]
    fn right_clicking_a_drum_hit_deletes_it_in_one_undo_step() {
        let mut app = kit_with_hits(&[0, 1]);
        let ctx = egui::Context::default();
        let origin = kit_frame(&mut app, &ctx, 0.0, vec![]);
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        let at = pos2(
            origin.x + app.lane_key_w() + 9.0 * tw,
            origin.y + HEADER_H + 7.0 * DRUM_ROW_H + DRUM_ROW_H / 2.0,
        );
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let before = app.doc.project.roll(KIT808, 0).unwrap().clone();
        kit_frame(&mut app, &ctx, 1.0, vec![egui::Event::PointerMoved(at)]);
        kit_frame(&mut app, &ctx, 1.1, vec![button(true)]);
        kit_frame(&mut app, &ctx, 1.2, vec![button(false)]);
        kit_frame(&mut app, &ctx, 1.3, vec![]);
        let pitches: Vec<_> = app
            .doc
            .project
            .roll(KIT808, 0)
            .unwrap()
            .notes
            .iter()
            .map(|n| n.pitch)
            .collect();
        assert_eq!(pitches, [1], "only the BD under the pointer goes");
        assert_eq!(app.view.focus, Some(KIT808));
        app.doc.undo();
        assert_eq!(
            app.doc.project.roll(KIT808, 0).unwrap(),
            &before,
            "one undo step"
        );
    }

    #[test]
    fn double_clicking_a_drum_hit_toggles_its_accent() {
        let mut app = kit_with_hits(&[0, 1]);
        let ctx = egui::Context::default();
        let origin = kit_frame(&mut app, &ctx, 0.0, vec![]);
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        let at = pos2(
            origin.x + app.lane_key_w() + 9.0 * tw,
            origin.y + HEADER_H + 7.0 * DRUM_ROW_H + DRUM_ROW_H / 2.0,
        );
        click_kit(&mut app, &ctx, at, 2, 1.0);
        let accents = |app: &SidAcidApp| {
            let roll = app.doc.project.roll(KIT808, 0).unwrap();
            roll.notes.iter().map(|n| n.accent).collect::<Vec<_>>()
        };
        assert_eq!(
            accents(&app),
            [true, false],
            "only the BD under the pointer"
        );
        app.doc.undo();
        assert_eq!(accents(&app), [false, false], "one undo step");
    }

    #[test]
    fn the_drum_accent_strip_accents_every_hit_in_the_cell() {
        let mut app = kit_with_hits(&[0, 1]);
        // A hit in the last cell, which a click past the end must not reach.
        let last = acid_engine::project::Note::new(60, 4, 2, 100);
        app.doc
            .project
            .roll_mut(KIT808, 0)
            .unwrap()
            .notes
            .push(last);
        let ctx = egui::Context::default();
        let origin = kit_frame(&mut app, &ctx, 0.0, vec![]);
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        let height = LaneView::default().clamped_height() as f32;
        let at = pos2(
            origin.x + app.lane_key_w() + 9.0 * tw,
            origin.y + HEADER_H + height + FLAG_H / 2.0,
        );
        click_kit(&mut app, &ctx, at, 1, 1.0);
        let roll = app.doc.project.roll(KIT808, 0).unwrap();
        let accents: Vec<_> = roll.notes.iter().map(|n| n.accent).collect();
        assert_eq!(accents, [true, true, false], "both hits in the cell");
        // Past the pattern end the strip does nothing.
        let end = pos2(origin.x + app.lane_key_w() + 70.0 * tw, at.y);
        let before = app.doc.project.roll(KIT808, 0).unwrap().clone();
        click_kit(&mut app, &ctx, end, 1, 2.0);
        assert_eq!(app.doc.project.roll(KIT808, 0).unwrap(), &before);
    }

    #[test]
    fn every_machine_kind_has_a_lane_kind() {
        let app = SidAcidApp::headless(Document::new(Project::default()));
        assert_eq!(app.lane_kind(SYNTH1), LaneKind::Acid);
        assert_eq!(app.lane_kind(KIT808), LaneKind::Drums);
        assert_eq!(app.lane_kind(FM), LaneKind::Poly);
        assert_eq!(app.lane_kind(SAMPLER), LaneKind::Poly);
    }

    #[test]
    fn a_pad_kit_has_a_sixteen_row_pad_lane() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app
            .add_machine(acid_engine::machine::MachineType::PadKit)
            .unwrap();
        assert_eq!(app.lane_kind(uid), LaneKind::Pads);
        let rows = app.lane_rows(uid, true, &NotePattern::default());
        assert_eq!(rows, Rows::Pads, "pad lanes never fold");
        assert_eq!(rows.count(), 16);
        assert_eq!(
            (rows.row_of(0), rows.row_of(15)),
            (Some(15), Some(0)),
            "pad 1 at the bottom"
        );
        let k = LaneKind::Pads;
        assert_eq!(
            (k.top_pitch(), k.is_rows(), k.resizable()),
            (15, true, true)
        );
        assert_eq!(
            (LaneKind::Drums.top_pitch(), LaneKind::Drums.resizable()),
            (7, false)
        );
        assert_eq!(StripMode::Expr.label(LaneKind::Pads), "Vel");
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
            let melodic = !app.lane_kind(uid).is_rows();
            assert_eq!(app.view.panel(uid).lane_scroll_y.is_some(), melodic);
        }
    }

    /// Text and ACID line segments painted by one frame of `uid`'s lane.
    fn painted(app: &mut SidAcidApp, uid: MachineUid) -> (Vec<String>, usize) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.lane(ui, uid, 800.0));
        });
        let (mut texts, mut acid_lines) = (Vec::new(), 0);
        for c in &out.shapes {
            match &c.shape {
                Shape::Text(t) => texts.push(t.galley.text().to_string()),
                Shape::LineSegment { stroke, .. } if stroke.color == theme::ACID => acid_lines += 1,
                _ => {}
            }
        }
        (texts, acid_lines)
    }

    #[test]
    fn pad_rows_are_labelled_with_the_pad_names() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app
            .add_machine(acid_engine::machine::MachineType::PadKit)
            .unwrap();
        app.doc.project.padkit_mut(uid).unwrap().pads[4].name = "tom".into();
        let (texts, _) = painted(&mut app, uid);
        assert!(texts.iter().any(|t| t == "tom"));
        assert!(texts.iter().any(|t| t == "P1") && texts.iter().any(|t| t == "P16"));
        assert!(
            !texts.iter().any(|t| t == "P5"),
            "named pads show their name"
        );
    }

    #[test]
    fn the_header_counts_notes_hidden_past_the_end() {
        for uid in [FM, KIT808] {
            let mut app = app_with(uid, &[(0, 4, 0), (60, 4, 1)]);
            let (texts, _) = painted(&mut app, uid);
            assert!(!texts.iter().any(|t| t.contains("hidden")), "{uid:?}");
            // Shortened to 8 steps (32 ticks): the note at 60 is hidden.
            app.doc.project.roll_mut(uid, 0).unwrap().length = 8;
            let (texts, _) = painted(&mut app, uid);
            assert!(texts.iter().any(|t| t == "+1 hidden"), "{uid:?}");
        }
    }

    #[test]
    fn a_scale_marks_its_root_rows_in_melodic_lanes_only() {
        let mut app = app_with(FM, &[]);
        app.view.panel(FM).lane_scroll_y = Some((127 - 64) as f32 * ROW_H);
        let (texts, lines) = painted(&mut app, FM);
        assert!(!texts.iter().any(|t| t == "A3"), "Chromatic marks nothing");
        let before = lines;
        app.doc.project.key.root = 9;
        app.doc.project.key.scale = Scale::Minor;
        let (texts, lines) = painted(&mut app, FM);
        assert!(texts.iter().any(|t| t == "A3"), "the root row is named");
        assert!(texts.iter().any(|t| t == "C4"), "C rows keep their names");
        assert!(lines > before, "root rows get a line");
        let (texts, lines) = painted(&mut app, KIT808);
        assert!(!texts.iter().any(|t| t == "A3"));
        let (_, chromatic_kit) = {
            app.doc.project.key.scale = Scale::Chromatic;
            painted(&mut app, KIT808)
        };
        assert_eq!(lines, chromatic_kit, "drum lanes ignore the key");
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
        app.view.panel(FM).drag = Some(LaneDrag::Marquee {
            from: (0, 60),
            add: false,
        });
        app.pending.clear();
        app.release_stale_poly_input(true);
        assert_eq!(app.view.audition, Some((FM, 60)), "pointer still down");
        assert!(app.view.panel(FM).drag.is_some());
        app.release_stale_poly_input(false);
        assert_eq!(app.view.audition, None);
        assert!(app.view.panels.values().all(|p| p.drag.is_none()));
        assert_eq!(
            app.pending.iter().map(cmd).collect::<Vec<_>>(),
            [(false, 5, 60)]
        );
    }

    #[test]
    fn a_collapsed_lane_frame_releases_a_held_key() {
        let mut app = fm_app();
        app.audition(FM, Some(60));
        let origin = Box::new(app.doc.project.fm(FM).unwrap().patterns[0].clone());
        app.view.panel(FM).drag = Some(LaneDrag::Resize {
            origin,
            slot: 0,
            idx: vec![0],
            grab: 4,
        });
        app.doc.apply(Edit::Bpm(100.0), true);
        // The FM lane collapses: its grid isn't drawn, so only the
        // frame-level check can see that the pointer is up.
        app.doc.project.machine_mut(FM).unwrap().lane.collapsed = true;
        crate::ui::run_frames(&mut app, 1);
        assert_eq!(app.view.audition, None);
        assert!(app.view.panels.values().all(|p| p.drag.is_none()));
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
        app.view.panel(FM).drag = Some(LaneDrag::Move {
            origin: Box::new(p.clone()),
            slot: 0,
            idx: vec![i],
            grab: (8, 60),
        });
        note_ops::shift(&mut p, &[i], -6, 0, LaneKind::Poly);
        app.commit_poly_frame(FM, 0, p.clone(), true, None, false);
        // The drop frame: the drag state is already cleared.
        app.view.panel(FM).drag = None;
        app.commit_poly_frame(FM, 0, p, false, Some(vec![i]), true);
        let merged = app.doc.project.fm(FM).unwrap().patterns[0].clone();
        assert_eq!(merged.notes.len(), 1, "merged");
        assert_eq!(
            app.selection(FM),
            [0],
            "the merged note is selected: {merged:?}"
        );
        app.doc.undo();
        assert_eq!(
            app.doc.project.fm(FM).unwrap().patterns[0],
            before,
            "one undo"
        );
    }

    #[test]
    fn a_drag_is_one_undo_step() {
        let mut app = fm_app();
        let mut p = app.doc.project.fm(FM).unwrap().patterns[0].clone();
        add(&mut p, 0, 4, 60, 100);
        app.commit_poly(FM, 0, Some(p.clone()), false, true);
        let before = app.doc.project.fm(FM).unwrap().patterns[0].clone();
        assert_eq!(before, p);
        for _ in 0..3 {
            note_ops::shift(&mut p, &[0], 4, 0, LaneKind::Poly);
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

    /// `uid` folded to C major (snap off), scrolled so folded row `top`
    /// is the top row.
    fn fold_c_major(app: &mut SidAcidApp, uid: MachineUid, top: usize) {
        app.doc.project.key = acid_engine::scale::Key {
            root: 0,
            scale: Scale::Major,
            snap: false,
        };
        app.doc.project.machine_mut(uid).unwrap().lane.fold = true;
        app.view.panel(uid).lane_scroll_y = Some(top as f32 * ROW_H);
    }

    /// Screen position of `tick` in `pitch`'s row of FM's folded lane.
    fn folded_at(app: &mut SidAcidApp, m: &Mouse, tick: f32, pitch: u8) -> egui::Pos2 {
        let key = app.doc.project.key;
        let rows = FoldRows::new(key, app.doc.project.roll(FM, 0).unwrap());
        let row = rows.row_of(pitch).expect("a shown row");
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        let scroll = app.view.panel(FM).lane_scroll_y.unwrap_or(0.0);
        pos2(
            m.origin.x + app.lane_key_w() + tick * tw - app.view.dock.scroll_x,
            m.origin.y + HEADER_H + row as f32 * ROW_H - scroll + ROW_H / 2.0,
        )
    }

    /// Where `text` was painted in one frame of `uid`'s lane.
    fn text_at(app: &mut SidAcidApp, uid: MachineUid, text: &str) -> Option<egui::Pos2> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.lane(ui, uid, 800.0));
        });
        out.shapes.iter().find_map(|c| match &c.shape {
            Shape::Text(t) if t.galley.text() == text => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            _ => None,
        })
    }

    #[test]
    fn clicking_a_folded_row_adds_its_pitch() {
        let mut app = app_with(FM, &[(0, 4, 61)]);
        fold_c_major(&mut app, FM, 30);
        let mut m = Mouse::new(&mut app, FM, 0);
        fold_c_major(&mut app, FM, 30);
        // The row under E4 is D4: D#4 is folded away.
        let e = folded_at(&mut app, &m, 9.5, 64);
        m.click(&mut app, e, NONE);
        let below = e + vec2(4.0 * tick_w(ZOOMS[app.view.dock.zoom]) * 2.0, ROW_H);
        m.click(&mut app, below, NONE);
        assert_eq!(notes_of(&app, FM), [(0, 4, 61), (8, 4, 64), (16, 4, 62)]);
        // The held off-scale note keeps its row and can be selected.
        let held = folded_at(&mut app, &m, 1.5, 61);
        m.click(&mut app, held, NONE);
        assert_eq!(app.selection(FM), [0]);
    }

    #[test]
    fn a_folded_lane_draws_only_its_rows_and_clamps_its_scroll() {
        let mut app = app_with(FM, &[(0, 4, 61)]);
        fold_c_major(&mut app, FM, 0);
        app.view.panel(FM).lane_scroll_y = Some(5000.0);
        let (texts, _) = painted(&mut app, FM);
        let height = LaneView::DEFAULT_HEIGHT as f32;
        assert_eq!(
            app.view.panel(FM).lane_scroll_y,
            Some(76.0 * ROW_H - height),
            "75 C major rows plus the held C#"
        );
        assert!(texts.iter().any(|t| t == "C0"), "the bottom rows in view");
        assert!(texts.iter().any(|t| t == "Fold"));
    }

    #[test]
    fn fold_is_ignored_for_drums_and_collapsed_lanes() {
        let mut app = app_with(KIT808, &[]);
        fold_c_major(&mut app, KIT808, 0);
        assert_eq!(text_at(&mut app, KIT808, "Fold"), None, "no toggle");
        // Instrument 1 would be C#, off C major: its row is still there.
        let mut m = Mouse::new(&mut app, KIT808, 0);
        let at = m.at(&mut app, 9.5, 1);
        m.click(&mut app, at, NONE);
        assert_eq!(notes_of(&app, KIT808), [(8, 4, 1)]);
        let mut app = fm_app();
        fold_c_major(&mut app, FM, 0);
        app.doc.project.machine_mut(FM).unwrap().lane.collapsed = true;
        let (texts, _) = painted(&mut app, FM);
        assert!(!texts.iter().any(|t| t == "C4"), "only the header");
    }

    #[test]
    fn the_fold_toggle_keeps_the_middle_pitch_in_view_and_edits_nothing() {
        let mut app = app_with(FM, &[]);
        app.doc.project.key.scale = Scale::Major;
        let height = LaneView::DEFAULT_HEIGHT as f32;
        // E4 (row 63 unfolded) in the middle of the view.
        let centre = |row: usize| row as f32 * ROW_H + ROW_H / 2.0 - height / 2.0;
        app.view.panel(FM).lane_scroll_y = Some(centre(127 - 64));
        let ctx = egui::Context::default();
        let fold = text_at(&mut app, FM, "Fold").expect("a Fold toggle");
        let click = |app: &mut SidAcidApp, t: f64| {
            lane_frame(app, &ctx, FM, t, vec![egui::Event::PointerMoved(fold)]);
            let b = |pressed| egui::Event::PointerButton {
                pos: fold,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: NONE,
            };
            lane_frame(app, &ctx, FM, t + 0.1, vec![b(true)]);
            lane_frame(app, &ctx, FM, t + 0.2, vec![b(false)]);
        };
        click(&mut app, 0.0);
        assert!(app.doc.project.machine(FM).unwrap().lane.fold);
        let rows = FoldRows::new(app.doc.project.key, &NotePattern::default());
        let row = rows.row_of(64).unwrap();
        assert_eq!(app.view.panel(FM).lane_scroll_y, Some(centre(row)));
        assert!(!app.doc.is_dirty(), "a view setting");
        assert!(!app.doc.can_undo());
        click(&mut app, 1.0);
        assert!(!app.doc.project.machine(FM).unwrap().lane.fold);
        assert_eq!(app.view.panel(FM).lane_scroll_y, Some(centre(127 - 64)));
    }

    #[test]
    fn folding_moves_a_hidden_cursor_onto_a_shown_row() {
        let mut app = app_with(FM, &[]);
        app.doc.project.key = acid_engine::scale::Key {
            root: 0,
            scale: Scale::Major,
            snap: false,
        };
        app.view.panel(FM).cursor.pitch = 66; // F#4, not in C major
        app.doc.project.machine_mut(FM).unwrap().lane.fold = true;
        app.settle_fold_view(FM);
        assert_eq!(
            app.view.panel(FM).cursor.pitch,
            65,
            "the row drawn under F#4"
        );
        // A key change that hides the cursor's pitch moves it too.
        app.doc.project.key.root = 2; // D major has no F
        app.settle_fold_view(FM);
        assert_eq!(app.view.panel(FM).cursor.pitch, 64, "E, the row under F");
    }

    #[test]
    fn the_fold_toggle_moves_a_hidden_cursor() {
        let mut app = app_with(FM, &[]);
        app.doc.project.key = acid_engine::scale::Key {
            root: 0,
            scale: Scale::Major,
            snap: false,
        };
        app.view.panel(FM).cursor.pitch = 66; // F#4, off C major
        let ctx = egui::Context::default();
        let fold = text_at(&mut app, FM, "Fold").expect("a Fold toggle");
        lane_frame(
            &mut app,
            &ctx,
            FM,
            0.0,
            vec![egui::Event::PointerMoved(fold)],
        );
        let b = |pressed| egui::Event::PointerButton {
            pos: fold,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: NONE,
        };
        lane_frame(&mut app, &ctx, FM, 0.1, vec![b(true)]);
        lane_frame(&mut app, &ctx, FM, 0.2, vec![b(false)]);
        assert!(app.doc.project.machine(FM).unwrap().lane.fold);
        assert_eq!(app.view.panel(FM).cursor.pitch, 65, "the drawn row");
    }

    #[test]
    fn the_cursor_moves_between_folded_rows() {
        let mut app = app_with(FM, &[(0, 4, 61)]);
        fold_c_major(&mut app, FM, 0);
        app.view.focus = Some(FM);
        app.view.dock.keyboard = true;
        app.view.panel(FM).cursor.pitch = 60;
        let up = |app: &mut SidAcidApp, up: bool| {
            let key = if up {
                egui::Key::ArrowUp
            } else {
                egui::Key::ArrowDown
            };
            assert!(app.dock_key(key, NONE));
            app.view.panel(FM).cursor.pitch
        };
        assert_eq!(up(&mut app, true), 61, "onto the held C#");
        assert_eq!(up(&mut app, true), 62);
        assert_eq!(up(&mut app, true), 64, "past the folded D#");
        assert_eq!(up(&mut app, false), 62);
        // Unfolded, the cursor moves by semitones again.
        app.doc.project.machine_mut(FM).unwrap().lane.fold = false;
        assert_eq!(up(&mut app, true), 63);
    }

    #[test]
    fn a_group_drag_on_a_folded_lane_is_stable() {
        // D4 and E4 selected, D4 grabbed and dragged onto E4's row: +2, so
        // E4 lands on the hidden F#4. The rows stay those of the drag's
        // start until it is dropped, so the pointer keeps meaning E4.
        let mut app = app_with(FM, &[(0, 4, 62), (8, 4, 64)]);
        fold_c_major(&mut app, FM, 30);
        let mut m = Mouse::new(&mut app, FM, 0);
        fold_c_major(&mut app, FM, 30);
        let (d, e) = (
            folded_at(&mut app, &m, 1.5, 62),
            folded_at(&mut app, &m, 9.5, 64),
        );
        m.click(&mut app, d, NONE);
        m.click(&mut app, e, egui::Modifiers::SHIFT);
        let to = folded_at(&mut app, &m, 1.5, 64);
        let primary = egui::PointerButton::Primary;
        m.frame(&mut app, vec![egui::Event::PointerMoved(d)], NONE);
        m.frame(&mut app, vec![Mouse::button(d, primary, true, NONE)], NONE);
        for k in 1..=4 {
            let p = d + (to - d) * (k as f32 / 4.0);
            m.frame(&mut app, vec![egui::Event::PointerMoved(p)], NONE);
        }
        for _ in 0..4 {
            m.frame(&mut app, vec![egui::Event::PointerMoved(to)], NONE);
            assert_eq!(notes_of(&app, FM), [(0, 4, 64), (8, 4, 66)]);
        }
        m.frame(
            &mut app,
            vec![Mouse::button(to, primary, false, NONE)],
            NONE,
        );
        m.frame(&mut app, vec![], NONE);
        m.frame(&mut app, vec![], NONE);
        assert_eq!(notes_of(&app, FM), [(0, 4, 64), (8, 4, 66)]);
        // Dropped: F#4 now holds a note, so it has a row.
        let rows = FoldRows::new(app.doc.project.key, app.doc.project.roll(FM, 0).unwrap());
        assert!(rows.row_of(66).is_some());
    }

    #[test]
    fn the_cursor_on_a_folded_away_pitch_is_drawn_on_the_row_below() {
        let mut app = app_with(FM, &[]);
        fold_c_major(&mut app, FM, 30);
        let m = Mouse::new(&mut app, FM, 0);
        fold_c_major(&mut app, FM, 30);
        app.view.focus = Some(FM);
        app.view.dock.keyboard = true;
        app.view.panel(FM).cursor = crate::ui::key_entry::Cursor { tick: 0, pitch: 63 };
        // D#4 is folded away: the cursor shows on D4's row.
        let want = folded_at(&mut app, &m, 0.0, 62).y - ROW_H / 2.0;
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.lane(ui, FM, 800.0));
        });
        let cursor = out.shapes.iter().find_map(|c| match &c.shape {
            Shape::Rect(r) if r.stroke.color == Color32::WHITE => Some(r.rect.top()),
            _ => None,
        });
        assert_eq!(cursor, Some(want));
    }

    /// Where the strip of `uid`'s lane (drawn at `origin`) is `frac` of
    /// the way down, at `tick`.
    fn strip_at(app: &SidAcidApp, origin: egui::Pos2, tick: f32, frac: f32) -> egui::Pos2 {
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        let height = LaneView::default().clamped_height() as f32;
        pos2(
            origin.x + app.lane_key_w() + tick * tw - app.view.dock.scroll_x,
            origin.y + HEADER_H + height + VEL_H * frac,
        )
    }

    #[test]
    fn the_header_picks_what_the_strip_shows() {
        let mut app = app_with(FM, &[]);
        let at = text_at(&mut app, FM, "Chance").expect("a Chance choice");
        let mut m = Mouse::new(&mut app, FM, 64);
        m.click(&mut app, at, NONE);
        assert_eq!(app.view.panel(FM).strip_mode, StripMode::Chance);
        let (texts, _) = painted(&mut app, FM);
        assert!(texts.iter().any(|t| t == "Chc"), "the strip is labelled");
        assert!(!app.doc.can_undo(), "a view setting");
        // A shut strip offers no choice.
        app.doc.project.machine_mut(FM).unwrap().lane.strip = false;
        assert!(text_at(&mut app, FM, "Chance").is_none());
    }

    #[test]
    fn a_chance_strip_click_sets_the_chance_of_the_notes_under_it() {
        let mut app = app_with(FM, &[(0, 4, 60), (0, 4, 64), (8, 4, 62)]);
        let first = app.doc.project.roll(FM, 0).unwrap().notes[0];
        app.set_selection(FM, vec![first]);
        app.view.panel(FM).strip_mode = StripMode::Chance;
        let mut m = Mouse::new(&mut app, FM, 64);
        let at = strip_at(&app, m.origin, 1.0, 0.5);
        m.click(&mut app, at, NONE);
        let roll = app.doc.project.roll(FM, 0).unwrap();
        let chances: Vec<_> = roll.notes.iter().map(|n| n.chance).collect();
        assert_eq!(chances, [50, 50, 100], "the notes starting in the cell");
        assert_eq!(app.selection(FM), vec![0], "the selection follows");
        assert_eq!(app.doc.undo_depth(), 1);
    }

    #[test]
    fn a_ratchet_strip_drag_is_one_undo_step() {
        let mut app = app_with(FM, &[(0, 4, 60), (8, 4, 62)]);
        app.view.panel(FM).strip_mode = StripMode::Ratchet;
        let before = app.doc.project.clone();
        let mut m = Mouse::new(&mut app, FM, 64);
        let from = strip_at(&app, m.origin, 1.0, 0.05);
        let to = strip_at(&app, m.origin, 9.0, 0.05);
        m.drag(&mut app, from, to, NONE);
        let roll = app.doc.project.roll(FM, 0).unwrap();
        let ratchets: Vec<_> = roll.notes.iter().map(|n| n.ratchet).collect();
        assert_eq!(ratchets, [8, 8]);
        assert_eq!(app.doc.undo_depth(), 1, "one step for the drag");
        app.undo();
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn drum_and_acid_strips_show_chance_too() {
        for uid in [KIT808, SYNTH1] {
            let pitch = if uid == KIT808 { 0 } else { 36 };
            let mut app = app_with(uid, &[(0, 4, pitch)]);
            app.view.panel(uid).strip_mode = StripMode::Chance;
            let mut m = Mouse::new(&mut app, uid, 64);
            let at = strip_at(&app, m.origin, 1.0, 0.75);
            m.click(&mut app, at, NONE);
            let n = app.doc.project.roll(uid, 0).unwrap().notes[0];
            assert_eq!(n.chance, 25, "{uid:?}");
        }
    }

    /// White (dashed outline) line segments painted by a frame of `uid`'s lane.
    fn dashes(app: &mut SidAcidApp, uid: MachineUid) -> usize {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.lane(ui, uid, 800.0));
        });
        out.shapes
            .iter()
            .filter(|c| {
                matches!(&c.shape, Shape::LineSegment { stroke, .. } if stroke.color == Color32::WHITE)
            })
            .count()
    }

    #[test]
    fn conditional_notes_get_a_dashed_outline_and_a_badge() {
        use acid_engine::project::Cond;
        let mut app = app_with(FM, &[(0, 16, 48), (16, 16, 48), (32, 1, 48)]);
        assert_eq!(dashes(&mut app, FM), 0, "plain notes");
        let (texts, _) = painted(&mut app, FM);
        assert!(!texts.iter().any(|t| t.contains('%')));
        for (c, badge) in [
            (Cond::Ratio(1, 4), "1:4"),
            (Cond::First, "1st"),
            (Cond::NotFirst, "!1st"),
            (Cond::Fill, "F"),
            (Cond::NotFill, "!F"),
            (Cond::Pre, "P"),
            (Cond::NotPre, "!P"),
        ] {
            app.doc.project.roll_mut(FM, 0).unwrap().notes[0].cond = c;
            let (texts, _) = painted(&mut app, FM);
            assert!(texts.iter().any(|t| t == badge), "{badge}: {texts:?}");
        }
        assert!(dashes(&mut app, FM) > 0, "a condition is dashed");
        let p = app.doc.project.roll_mut(FM, 0).unwrap();
        p.notes[0].cond = Cond::Always;
        p.notes[1].chance = 50;
        p.notes[1].ratchet = 3;
        p.notes[2].cond = Cond::Fill;
        let (texts, _) = painted(&mut app, FM);
        assert!(texts.iter().any(|t| t == "50% ×3"), "{texts:?}");
        assert!(!texts.iter().any(|t| t == "F"), "too narrow for a badge");
        assert!(dashes(&mut app, FM) > 0, "a chance is dashed");
        let p = app.doc.project.roll_mut(FM, 0).unwrap();
        p.notes[1].chance = 100;
        p.notes[2].cond = Cond::Always;
        assert_eq!(dashes(&mut app, FM), 0, "a ratchet alone is not dashed");
        let (texts, _) = painted(&mut app, FM);
        assert!(texts.iter().any(|t| t == "×3"), "but badged");
    }

    #[test]
    fn a_strip_cell_stops_at_the_pattern_end() {
        let mut app = app_with(FM, &[(0, 4, 60), (12, 4, 62)]);
        app.doc.project.roll_mut(FM, 0).unwrap().length = 3;
        app.view.dock.snap = Snap::Quarter;
        app.view.panel(FM).strip_mode = StripMode::Chance;
        let mut m = Mouse::new(&mut app, FM, 64);
        let at = strip_at(&app, m.origin, 1.0, 0.5);
        m.click(&mut app, at, NONE);
        let roll = app.doc.project.roll(FM, 0).unwrap();
        let chances: Vec<_> = roll.notes.iter().map(|n| n.chance).collect();
        assert_eq!(chances, [50, 100], "the hidden note past the end stays");
    }

    #[test]
    fn a_muted_note_still_shows_in_the_chance_strip() {
        let mut app = app_with(FM, &[(0, 4, 60)]);
        app.view.panel(FM).strip_mode = StripMode::Chance;
        app.doc.project.roll_mut(FM, 0).unwrap().notes[0].chance = 0;
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))),
            ..Default::default()
        };
        let mut origin = pos2(0.0, 0.0);
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                origin = ui.cursor().min;
                app.lane(ui, FM, 800.0);
            });
        });
        let height = LaneView::default().clamped_height() as f32;
        let strip_bottom = origin.y + HEADER_H + height + VEL_H;
        let bar = out.shapes.iter().any(|c| {
            matches!(&c.shape, Shape::Rect(r)
                if r.fill == theme::ACID && (r.rect.bottom() - strip_bottom).abs() < 0.5
                    && r.rect.height() >= 1.0)
        });
        assert!(bar, "a stub for chance 0");
    }
}
