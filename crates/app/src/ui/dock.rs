//! The editor dock at the bottom of the window: a header row (snap, key,
//! zoom, follow, and in Pattern mode the Tools menu (`tool_menu`) and the
//! note inspector for the focused lane's selection; it wraps when narrow), a ruler that doubles as the
//! shared horizontal scrollbar, and every machine's lane stacked in rack
//! order. Its height is a share of the window kept in the settings file.

use super::key_entry::LaneKind;
use super::lane_geom::{ZOOMS, follow_scroll, tick_w};
use super::note_ops::{self, Field, PasteError};
use super::poly_edit::Snap;
use super::{NoteClip, SidAcidApp, song_lane, song_ops, theme};
use crate::document::Edit;
use acid_engine::machine::MachineUid;
use acid_engine::project::{Note, NotePattern, TICKS_PER_STEP};
use acid_engine::scale::{ROOT_NAMES, Scale};
use acid_engine::sequencer::Mode;
use acid_engine::song;
use eframe::egui::{self, Align2, FontId, Rect, RichText, Sense, Stroke, pos2, vec2};

/// Height of the ruler above the lanes.
const RULER_H: f32 = 16.0;
/// Height of the thumb strip along the ruler's bottom.
const THUMB_H: f32 = 4.0;
/// Gap between stacked lanes.
const LANE_GAP: f32 = 2.0;

impl SidAcidApp {
    /// Draws the dock as a bottom panel; call before the central panel so
    /// the rack takes what is left.
    pub(crate) fn dock(&mut self, ctx: &egui::Context) {
        self.check_tool();
        let window_h = ctx.screen_rect().height().max(0.0);
        let frac = self.settings.ui.clamped_dock_frac();
        self.latch_dock_resize(ctx);
        let mut panel = egui::TopBottomPanel::bottom("dock")
            .resizable(true)
            .default_height(window_h * frac)
            .height_range(window_h * 0.15..=window_h * 0.85);
        if self.window_resized(window_h) {
            // egui keeps the panel's height in points; put it back to its
            // share of the new window for this one frame.
            panel = panel.exact_height(window_h * frac);
        }
        let panel = panel.show(ctx, |ui| {
            self.dock_header(ui);
            self.dock_body(ui);
        });
        let rect = panel.response.rect;
        self.track_dock_resize(ctx, rect, window_h, frac);
    }

    /// True on the frame the window's height changes, unless the dock's
    /// edge is being dragged (that drag then sets the share itself; the
    /// change is picked up on the frame after it).
    fn window_resized(&mut self, window_h: f32) -> bool {
        let dock = &mut self.view.dock;
        if dock.resizing {
            return false;
        }
        let last = dock.window_h.replace(window_h);
        last.is_some_and(|h| (h - window_h).abs() > 0.5)
    }

    /// On the frame a press starts, before the panel is drawn: a press on
    /// the dock's top edge (as last drawn, which is what egui's own resize
    /// handle hit-tests against) starts a resize, latched until release.
    /// Deciding once at the press keeps it true however fast the edge
    /// then moves with the pointer.
    fn latch_dock_resize(&mut self, ctx: &egui::Context) {
        let Some(last) = self.view.dock.rect else {
            return;
        };
        let grab = ctx.style().interaction.resize_grab_radius_side + 1.0;
        let on_edge = ctx.input(|i| {
            i.pointer.primary_pressed()
                && i.pointer
                    .press_origin()
                    .is_some_and(|p| (p.y - last.top()).abs() <= grab)
        });
        if on_edge {
            self.view.dock.resizing = true;
        }
    }

    /// After the panel is drawn: when a latched resize's pointer is
    /// released, the panel's new share of the window is kept (if it
    /// moved). Stores this frame's rect for the next press.
    fn track_dock_resize(&mut self, ctx: &egui::Context, rect: Rect, window_h: f32, frac: f32) {
        self.view.dock.rect = Some(rect);
        if ctx.input(|i| i.pointer.any_down()) || !std::mem::take(&mut self.view.dock.resizing) {
            return;
        }
        if window_h > 0.0 && (rect.height() - window_h * frac).abs() > 1.0 {
            self.finish_dock_resize(rect.height() / window_h);
        }
    }

    /// Keeps `frac` as the dock's share of the window and saves the
    /// settings file (when there is one); a failed save shows in the
    /// status line.
    pub(crate) fn finish_dock_resize(&mut self, frac: f32) {
        self.settings.ui.dock_frac = frac;
        self.settings.ui.dock_frac = self.settings.ui.clamped_dock_frac();
        if let Some(path) = &self.settings_path
            && let Err(e) = crate::settings::save(path, &self.settings)
        {
            self.view.status_line = Some(format!("Could not save settings: {e}"));
        }
    }

    /// Steps the zoom `delta` levels through `ZOOMS`, held to its ends.
    /// The tick at the left edge of the view stays put.
    pub(crate) fn zoom_dock(&mut self, delta: i32) {
        let dock = &mut self.view.dock;
        let old = dock.zoom.min(ZOOMS.len() - 1);
        let new = (old as i32 + delta).clamp(0, ZOOMS.len() as i32 - 1) as usize;
        dock.scroll_x *= ZOOMS[new] / ZOOMS[old];
        dock.zoom = new;
    }

    /// Snap choices and the key picker (Pattern mode), zoom buttons and
    /// the follow toggle; in Pattern mode then a row with the Tools menu
    /// and the inspector, in Song mode the song tools (`song_tools`).
    fn dock_header(&mut self, ui: &mut egui::Ui) {
        #[cfg(test)]
        self.view.dock.tool_rects.clear();
        let song = self.view.mode == Mode::Song;
        ui.horizontal_wrapped(|ui| {
            // Song mode moves by bars (Alt: steps); the note snap is idle.
            if !song {
                ui.label(RichText::new("Snap").small().color(theme::ACID));
                for snap in Snap::ALL {
                    let on = self.view.dock.snap == snap;
                    if ui.selectable_label(on, snap.label()).clicked() {
                        self.view.dock.snap = snap;
                    }
                }
                ui.separator();
                // A key change is an edit of its own: not under an open tool.
                let free = self.view.tool.is_none();
                ui.add_enabled_ui(free, |ui| self.key_picker(ui));
                ui.separator();
            }
            ui.label(RichText::new("Zoom").small().color(theme::ACID));
            let zoom = self.view.dock.zoom.min(ZOOMS.len() - 1);
            if ui
                .add_enabled(zoom > 0, egui::Button::new("−"))
                .on_hover_text("Zoom out")
                .clicked()
            {
                self.zoom_dock(-1);
            }
            ui.label(RichText::new(format!("×{}", ZOOMS[zoom])).monospace());
            if ui
                .add_enabled(zoom + 1 < ZOOMS.len(), egui::Button::new("+"))
                .on_hover_text("Zoom in")
                .clicked()
            {
                self.zoom_dock(1);
            }
            ui.separator();
            let follow = self.view.dock.follow;
            if ui
                .selectable_label(follow, "Follow")
                .on_hover_text("Scroll with the playhead while playing")
                .clicked()
            {
                self.view.dock.follow = !follow;
            }
            if song {
                ui.separator();
                self.song_tools(ui);
            }
        });
        if !song {
            // A row of its own, there with or without a selection, so the
            // lanes don't move when the inspector comes and goes.
            ui.horizontal_wrapped(|ui| {
                ui.set_min_height(ui.spacing().interact_size.y);
                self.tools_menu(ui);
                ui.separator();
                // Under an open tool the lane holds its preview: no edits.
                let free = self.view.tool.is_none();
                ui.add_enabled_ui(free, |ui| self.inspector(ui));
            });
        } else {
            self.inspector_closed(None);
        }
    }

    /// Closes the undo step of an inspector number that was being dragged
    /// or typed into (`view.dock.inspector_open`) once it no longer is
    /// (`open`): released, unfocused, or no longer drawn.
    fn inspector_closed(&mut self, open: Option<egui::Id>) {
        let was = std::mem::replace(&mut self.view.dock.inspector_open, open);
        if was.is_some() && was != open {
            self.doc.end_gesture();
        }
    }

    /// The note inspector: the focused lane's selected notes' velocity
    /// (not on acid lanes), chance (with Reroll), ratchet and condition,
    /// and accent (acid, drums) and slide (acid). A field the notes differ
    /// in shows "–". A change applies to every selected note as one undo
    /// step (a drag on a number is one step), and they stay selected.
    fn inspector(&mut self, ui: &mut egui::Ui) {
        let uid = self.focused();
        let idx = uid.map(|u| self.selection(u)).unwrap_or_default();
        let (Some(uid), false) = (uid, idx.is_empty()) else {
            ui.label(
                RichText::new("Select notes to edit them here")
                    .small()
                    .weak(),
            );
            return self.inspector_closed(None);
        };
        let slot = self.selected(uid);
        let Some(pattern) = self.doc.project.roll(uid, slot) else {
            return self.inspector_closed(None);
        };
        let notes = note_ops::values(pattern, &idx);
        let kind = self.lane_kind(uid);
        // Test builds note where each tool sits.
        #[cfg(test)]
        let mut rects: Vec<(String, Rect)> = Vec::new();
        #[cfg(test)]
        let mut note = |name: &str, r: &egui::Response| rects.push((name.to_string(), r.rect));
        #[cfg(not(test))]
        let note = |_: &str, _: &egui::Response| {};
        let mut edit: Option<Field> = None;
        // The number that changed, and the one being dragged or typed into.
        let (mut changed_id, mut open): (Option<egui::Id>, Option<egui::Id>) = (None, None);
        let mut number = |ui: &mut egui::Ui,
                          name: &str,
                          get: fn(&Note) -> u8,
                          (lo, hi): (u8, u8),
                          (prefix, suffix): (&str, &str),
                          field: fn(u8) -> Field|
         -> egui::Response {
            let shared = note_ops::common(&notes, get);
            let mut v = shared.unwrap_or_else(|| get(&notes[0]));
            ui.label(RichText::new(name).small().color(theme::ACID));
            let dv = egui::DragValue::new(&mut v).range(lo..=hi).speed(0.2);
            // Mixed: a bare dash (the formatter's text gets the affixes too).
            let dv = if shared.is_some() {
                dv.prefix(prefix).suffix(suffix)
            } else {
                dv.custom_formatter(|_, _| "–".into())
            };
            let r = ui.add(dv);
            if r.changed() {
                edit = Some(field(v));
                changed_id = Some(r.id);
            }
            if r.dragged() || r.has_focus() {
                open = Some(r.id);
            }
            r
        };
        if kind != LaneKind::Acid {
            let r = number(ui, "Vel", |n| n.vel, (1, 127), ("", ""), Field::Vel);
            note("Vel", &r);
        }
        let r = number(
            ui,
            "Chance",
            |n| n.chance,
            (0, 100),
            ("", "%"),
            Field::Chance,
        );
        note("Chance", &r);
        let reroll = ui
            .button(RichText::new("Reroll").small())
            .on_hover_text("New dice for every chance in the project");
        note("Reroll", &reroll);
        let r = number(
            ui,
            "Ratchet",
            |n| n.ratchet,
            (1, 8),
            ("×", ""),
            Field::Ratchet,
        );
        note("Ratchet", &r);
        ui.label(RichText::new("Cond").small().color(theme::ACID));
        let cond = note_ops::common(&notes, |n| n.cond);
        let combo = egui::ComboBox::from_id_salt("note_cond")
            .width(70.0)
            .selected_text(cond.map_or("–".into(), note_ops::cond_label))
            .show_ui(ui, |ui| {
                for c in note_ops::conds() {
                    let item = ui.selectable_label(cond == Some(c), note_ops::cond_label(c));
                    note(&format!("Cond {}", note_ops::cond_label(c)), &item);
                    if item.clicked() {
                        edit = Some(Field::Cond(c));
                    }
                }
            });
        note("Cond", &combo.response);
        let mut flag =
            |ui: &mut egui::Ui, name: &str, get: fn(&Note) -> bool, field: fn(bool) -> Field| {
                let shared = note_ops::common(&notes, get);
                let text = if shared.is_none() {
                    format!("{name} –")
                } else {
                    name.to_string()
                };
                let r = ui.selectable_label(shared == Some(true), text);
                if r.clicked() {
                    edit = Some(field(shared != Some(true)));
                }
                r
            };
        if kind != LaneKind::Poly {
            let r = flag(ui, "Accent", |n| n.accent, Field::Accent);
            note("Accent", &r);
        }
        if kind == LaneKind::Acid {
            let r = flag(ui, "Slide", |n| n.slide, Field::Slide);
            note("Slide", &r);
        }
        #[cfg(test)]
        self.view.dock.tool_rects.extend(rects);
        let mut p = pattern.clone();
        let changed = edit.is_some_and(|f| note_ops::set_field(&mut p, &idx, f));
        if changed {
            self.set_selection(uid, note_ops::values(&p, &idx));
        }
        // A change to the open number (or, on the frame it is let go of,
        // to the one that was open) merges into its step; any other change
        // is a step of its own.
        let was = self.view.dock.inspector_open;
        let merge = changed_id.is_some() && (changed_id == open || changed_id == was);
        if was.is_some() && was != open && changed_id != was {
            self.doc.end_gesture();
        }
        self.commit_poly(uid, slot, changed.then_some(p), merge, changed && !merge);
        self.inspector_closed(open);
        if reroll.clicked() {
            self.reroll();
        }
    }

    /// A new chance seed for the project, one undo step.
    pub(crate) fn reroll(&mut self) {
        let old = self.doc.project.seed;
        let mut seed = self.next_seed() as u32;
        if seed == old {
            seed ^= 1;
        }
        let cmds = self.doc.apply(Edit::Seed(seed), false);
        self.send(cmds);
        self.doc.end_gesture();
    }

    /// The project key: root and scale combos (one undo step each) and
    /// the snap to scale toggle (a view setting, set directly).
    fn key_picker(&mut self, ui: &mut egui::Ui) {
        let key = self.doc.project.key;
        let mut picked = key;
        // Test builds note where each combo and its open items sit.
        #[cfg(test)]
        let mut rects: Vec<(String, Rect)> = Vec::new();
        #[cfg(test)]
        let mut note = |name: String, r: &egui::Response| rects.push((name, r.rect));
        #[cfg(not(test))]
        let note = |_: String, _: &egui::Response| {};
        ui.label(RichText::new("Key").small().color(theme::ACID));
        let root = egui::ComboBox::from_id_salt("key_root")
            .width(44.0)
            .selected_text(ROOT_NAMES[key.root as usize % 12])
            .show_ui(ui, |ui| {
                for (i, name) in ROOT_NAMES.iter().enumerate() {
                    let item = ui.selectable_value(&mut picked.root, i as u8, *name);
                    note(format!("Root {name}"), &item);
                }
            });
        note("Key root".into(), &root.response);
        let scale = egui::ComboBox::from_id_salt("key_scale")
            .width(110.0)
            .selected_text(key.scale.label())
            .show_ui(ui, |ui| {
                for s in Scale::ALL {
                    let item = ui.selectable_value(&mut picked.scale, s, s.label());
                    note(format!("Scale {}", s.label()), &item);
                }
            });
        note("Key scale".into(), &scale.response);
        let toggle = ui
            .selectable_label(key.snap, "Snap to scale")
            .on_hover_text("New, moved and transposed notes land on the key's scale");
        note("Snap to scale".into(), &toggle);
        #[cfg(test)]
        self.view.dock.tool_rects.extend(rects);
        if picked != key {
            let middles = self.fold_middles();
            let cmds = self.doc.apply(Edit::Key { key: picked }, false);
            self.send(cmds);
            self.doc.end_gesture();
            self.recentre_folds(middles);
        }
        if toggle.clicked() {
            self.doc.project.key.snap = !key.snap;
        }
    }

    /// Ticks in the longest selected pattern in the rack.
    fn longest_ticks(&self) -> u16 {
        let p = &self.doc.project;
        p.machines
            .iter()
            .filter_map(|m| p.roll(m.uid, m.selected))
            .map(|r| r.ticks())
            .max()
            .unwrap_or(0)
    }

    /// The ruler, then the lanes (in Song mode, the song lanes).
    fn dock_body(&mut self, ui: &mut egui::Ui) {
        let song = self.view.mode == Mode::Song;
        let tw = tick_w(ZOOMS[self.view.dock.zoom.min(ZOOMS.len() - 1)]);
        let sw = song_lane::step_w(self.view.dock.zoom);
        let key_w = self.lane_key_w();
        let view_w = (ui.available_width() - key_w).max(0.0);
        let content_w = if song {
            // Room after the song's end to place more.
            let end = self.with_lens(|lens| song::song_end(&self.doc.project.song, lens));
            (end + SONG_TAIL) as f32 * sw
        } else {
            self.longest_ticks() as f32 * tw
        };
        if self.view.dock.follow {
            let playhead = if song {
                self.status
                    .playing
                    .then_some(self.status.song_step as f32 * sw)
            } else {
                self.focused()
                    .and_then(|uid| self.playhead(uid, self.selected(uid)))
                    .map(|step| step as f32 * TICKS_PER_STEP as f32 * tw)
            };
            if let Some(x) = playhead {
                self.view.dock.scroll_x = follow_scroll(self.view.dock.scroll_x, view_w, x);
            }
        }
        let max_scroll = (content_w - view_w).max(0.0);
        self.view.dock.scroll_x = self.view.dock.scroll_x.clamp(0.0, max_scroll);
        if song {
            self.song_ruler(ui, key_w, sw, view_w, content_w);
        } else {
            self.ruler(ui, key_w, tw, view_w, content_w);
        }
        // A solid, always-shown bar, so a scrolled stack is plain to see.
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        let out = egui::ScrollArea::vertical()
            .id_salt("dock_lanes")
            .auto_shrink(false)
            // A drag on a header or gap would pan (and fling) the stack.
            .drag_to_scroll(false)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
                for uid in self.doc.project.rack_uids() {
                    let width = ui.available_width();
                    // A double-click on a block switches mode mid-stack.
                    if self.view.mode == Mode::Song {
                        self.song_lane(ui, uid, width);
                    } else {
                        self.lane(ui, uid, width);
                    }
                    ui.add_space(LANE_GAP);
                }
            });
        self.view.dock.lanes = Some(out.inner_rect);
        self.view.dock.lanes_offset = out.state.offset.y;
    }

    /// Step numbers every 4 steps over the grids' columns, and a thumb
    /// strip along the bottom showing the visible part of the longest
    /// pattern; dragging on the strip scrolls every lane. The rest of the
    /// ruler is left for selecting a time range.
    fn ruler(&mut self, ui: &mut egui::Ui, key_w: f32, tw: f32, view_w: f32, content_w: f32) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, RULER_H), Sense::hover());
        self.view.dock.ruler = Some(rect);
        let left = rect.left() + key_w;
        let strip = Rect::from_min_max(pos2(left, rect.top()), rect.max);
        let bar = Rect::from_min_max(pos2(left, rect.bottom() - THUMB_H), rect.max);
        let resp = self.ruler_thumb_drag(ui, bar, view_w, content_w);
        // Above the thumb strip: a drag picks a time range, a click clears it.
        let numbers = Rect::from_min_max(strip.min, pos2(strip.right(), bar.top()));
        let picker = ui.interact(
            numbers,
            ui.id().with("dock_ruler_range"),
            Sense::click_and_drag(),
        );
        let tick_at = |x: f32, scroll_x: f32| {
            (((x - left + scroll_x) / tw).max(0.0) as u32).min(u16::MAX as u32) as u16
        };
        // The press tick, taken with this frame's scroll: later frames
        // may scroll (follow) under the drag.
        let pressed_on = ui.input(|i| {
            i.pointer
                .primary_pressed()
                .then(|| i.pointer.press_origin())
                .flatten()
                .filter(|o| numbers.contains(*o))
        });
        if let Some(o) = pressed_on {
            self.view.dock.range_from = Some(tick_at(o.x, self.view.dock.scroll_x));
        }
        if picker.clicked() {
            // Only the band goes: the selections it made stay (the plan),
            // so a click on the ruler doesn't lose them.
            self.view.dock.range = None;
        } else if picker.dragged()
            && let (Some(from), Some(p)) =
                (self.view.dock.range_from, picker.interact_pointer_pos())
        {
            let snap = self.view.dock.snap;
            let scroll = self.view.dock.scroll_x;
            let a = snap.floor(from);
            let b = snap.floor(tick_at(p.x, scroll));
            let lo = a.min(b);
            let hi = a.max(b).saturating_add(snap.ticks());
            self.view.dock.range = Some((lo, hi));
            self.select_range(lo, hi);
        }
        let painter = ui.painter_at(strip);
        painter.rect_filled(strip, 0.0, theme::GRID_DARK);
        let scroll_x = self.view.dock.scroll_x;
        draw_thumb(&painter, bar, scroll_x, view_w, content_w);
        let step_w = TICKS_PER_STEP as f32 * tw;
        let steps = (content_w / step_w).round() as usize;
        for step in (0..steps).step_by(4) {
            let x = left + step as f32 * step_w - scroll_x;
            if x > strip.right() {
                break;
            }
            if x + 4.0 * step_w < left {
                continue;
            }
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0_f32, theme::GRID_LINE),
            );
            painter.text(
                pos2(x + 2.0, rect.center().y),
                Align2::LEFT_CENTER,
                format!("{}", step + 1),
                FontId::monospace(10.0),
                ui.visuals().text_color(),
            );
        }
        if let Some((lo, hi)) = self.view.dock.range {
            let band = Rect::from_min_max(
                pos2(left + lo as f32 * tw - scroll_x, rect.top()),
                pos2(left + hi as f32 * tw - scroll_x, bar.top()),
            );
            painter.rect_filled(band, 0.0, theme::ACID.gamma_multiply(0.35));
        }
        resp.on_hover_text("Drag to scroll the lanes");
    }

    /// The thumb strip `bar` along the ruler's bottom: dragging it scrolls
    /// every lane.
    fn ruler_thumb_drag(
        &mut self,
        ui: &egui::Ui,
        bar: Rect,
        view_w: f32,
        content_w: f32,
    ) -> egui::Response {
        let resp = ui.interact(bar, ui.id().with("dock_ruler_thumb"), Sense::drag());
        if resp.dragged() {
            let max_scroll = (content_w - view_w).max(0.0);
            let x = self.view.dock.scroll_x + resp.drag_delta().x;
            self.view.dock.scroll_x = x.clamp(0.0, max_scroll);
        }
        resp
    }

    /// The Song-mode ruler: bar numbers every 16 steps, the loop along the
    /// top, the song's end, the song cursor and the playhead, over the same thumb strip as
    /// in Pattern mode. A click sets the song cursor (on a bar; Alt: on a
    /// step) and clears the range; a drag picks a range of whole bars and
    /// selects the blocks starting in it on every lane.
    fn song_ruler(&mut self, ui: &mut egui::Ui, key_w: f32, sw: f32, view_w: f32, content_w: f32) {
        use song_lane::{SongGeom, snap_bar};
        use song_ops::BAR;
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(vec2(width, RULER_H), Sense::hover());
        self.view.dock.ruler = Some(rect);
        let left = rect.left() + key_w;
        let strip = Rect::from_min_max(pos2(left, rect.top()), rect.max);
        let bar = Rect::from_min_max(pos2(left, rect.bottom() - THUMB_H), rect.max);
        let resp = self.ruler_thumb_drag(ui, bar, view_w, content_w);
        let numbers = Rect::from_min_max(strip.min, pos2(strip.right(), bar.top()));
        let picker = ui.interact(
            numbers,
            ui.id().with("dock_ruler_range"),
            Sense::click_and_drag(),
        );
        let geom = |scroll_x| SongGeom {
            left,
            step_w: sw,
            scroll_x,
        };
        let pressed_on = ui.input(|i| {
            i.pointer
                .primary_pressed()
                .then(|| i.pointer.press_origin())
                .flatten()
                .filter(|o| numbers.contains(*o))
        });
        if let Some(o) = pressed_on {
            let step = geom(self.view.dock.scroll_x).step_at(o.x);
            self.view.dock.range_from = Some(step.min(u16::MAX as u32) as u16);
        }
        let alt = ui.input(|i| i.modifiers.alt);
        if picker.clicked()
            && let Some(p) = picker.interact_pointer_pos()
        {
            self.view.dock.range = None;
            let step = geom(self.view.dock.scroll_x).step_at(p.x);
            self.set_song_cursor(snap_bar(step, alt));
        } else if picker.dragged()
            && let (Some(from), Some(p)) =
                (self.view.dock.range_from, picker.interact_pointer_pos())
        {
            let to = geom(self.view.dock.scroll_x).step_at(p.x);
            let (lo, hi) = song_lane::bar_range(from as u32, to);
            self.view.dock.range = Some((lo, hi));
            self.select_song_range(lo as u32, hi as u32);
        }
        let painter = ui.painter_at(strip);
        painter.rect_filled(strip, 0.0, theme::GRID_DARK);
        let g = geom(self.view.dock.scroll_x);
        draw_thumb(&painter, bar, g.scroll_x, view_w, content_w);
        let first = g.step_at(left) / BAR * BAR;
        let last = g.step_at(strip.right());
        for step in (first..=last).step_by(BAR as usize) {
            let x = g.x_of(step);
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0_f32, theme::GRID_LINE),
            );
            painter.text(
                pos2(x + 2.0, rect.center().y),
                Align2::LEFT_CENTER,
                format!("{}", step / BAR + 1),
                FontId::monospace(10.0),
                ui.visuals().text_color(),
            );
        }
        if let Some((a, b)) = self.doc.project.song.loop_range {
            let r = Rect::from_min_max(
                pos2(g.x_of(a), rect.top()),
                pos2(g.x_of(b), rect.top() + 2.0),
            );
            painter.rect_filled(r, 0.0, theme::VIOLET);
        }
        // Where playback stops: the last block's end, or a converted
        // song's own end or the loop's end when later.
        let end = self.with_lens(|lens| song::song_end(&self.doc.project.song, lens));
        if end > 0 {
            let x = g.x_of(end);
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, bar.top())],
                Stroke::new(1.0_f32, theme::ACCENT),
            );
            painter.text(
                pos2(x - 2.0, rect.top() + 1.0),
                Align2::RIGHT_TOP,
                "End",
                FontId::monospace(9.0),
                theme::ACCENT,
            );
        }
        if let Some((lo, hi)) = self.view.dock.range {
            let band = Rect::from_min_max(
                pos2(g.x_of(lo as u32), rect.top()),
                pos2(g.x_of(hi as u32), bar.top()),
            );
            painter.rect_filled(band, 0.0, theme::ACID.gamma_multiply(0.35));
        }
        let cx = g.x_of(self.view.song_cursor);
        let top = rect.top() + 2.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(cx - 4.0, top),
                pos2(cx + 4.0, top),
                pos2(cx, top + 6.0),
            ],
            theme::QUEUED,
            Stroke::NONE,
        ));
        if self.status.playing {
            let x = g.x_of(self.status.song_step);
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, bar.top())],
                Stroke::new(2.0_f32, theme::ACID),
            );
        }
        picker.on_hover_text("Click to set the song start; drag to pick bars");
        resp.on_hover_text("Drag to scroll the lanes");
    }

    /// Makes the notes starting in `[t0, t1)` every lane's selection.
    fn select_range(&mut self, t0: u16, t1: u16) {
        use super::note_ops::{in_range, values};
        for uid in self.doc.project.rack_uids() {
            let slot = self.selected(uid);
            let Some(p) = self.doc.project.roll(uid, slot) else {
                continue;
            };
            let notes = values(p, &in_range(p, t0, t1));
            self.set_selection(uid, notes);
        }
    }

    /// After the dock and the rack: a press anywhere outside the dock
    /// takes keyboard focus away from it.
    pub(crate) fn dock_focus_after_draw(&mut self, ctx: &egui::Context) {
        let Some(rect) = self.view.dock.rect else {
            return;
        };
        // The resize edge above the panel counts as the dock.
        let grab = ctx.style().interaction.resize_grab_radius_side + 1.0;
        let rect = rect.with_min_y(rect.top() - grab);
        let outside = ctx.input(|i| {
            i.pointer.primary_pressed()
                && i.pointer
                    .press_origin()
                    .or(i.pointer.interact_pos())
                    .is_some_and(|p| !rect.contains(p))
        });
        if outside {
            self.view.dock.keyboard = false;
        }
    }
}

/// Steps shown after the song's end in Song mode, to place more.
const SONG_TAIL: u32 = 64;

/// The visible part of the content, as a thumb on the ruler's strip `bar`.
fn draw_thumb(painter: &egui::Painter, bar: Rect, scroll_x: f32, view_w: f32, content_w: f32) {
    if content_w > 0.0 && view_w > 0.0 {
        let shown = (view_w / content_w).min(1.0);
        let x0 = bar.left() + (scroll_x / content_w).min(1.0) * view_w;
        let thumb = Rect::from_min_max(
            pos2(x0, bar.top()),
            pos2((x0 + shown * view_w).min(bar.right()), bar.bottom()),
        );
        painter.rect_filled(thumb, 2.0, theme::KNOB_TRACK);
    }
}

impl SidAcidApp {
    /// Apply one key press to the focused machine's selected pattern when
    /// the dock has the keyboard: first as a selection key (when the lane
    /// has a selection), else as keyboard entry. True when the key was
    /// used, even if it only moved the cursor.
    pub(crate) fn dock_key(&mut self, key: egui::Key, mods: egui::Modifiers) -> bool {
        use super::key_entry::{EntryKey, apply_entry, entry_key};
        if !self.view.dock.keyboard {
            return false;
        }
        let Some(uid) = self.focused() else {
            return false;
        };
        // Esc closes an open note tool as Cancel does.
        if key == egui::Key::Escape && self.view.tool.is_some() {
            self.cancel_tool();
            return true;
        }
        // A mouse move or resize redoes its edit every frame; a key's edit
        // would be overwritten and split the undo step. Used, so no widget
        // acts on it either.
        if self.mouse_editing() {
            return true;
        }
        // Song mode's lanes hold blocks: block keys, no note entry.
        if self.view.mode == Mode::Song {
            return self.song_key(uid, key, mods);
        }
        // Ahead of `entry_key`, which turns down Alt.
        if let Some(used) = self.selection_key(uid, key, mods) {
            return used;
        }
        let Some(entry) = entry_key(key, mods) else {
            return false;
        };
        let slot = self.selected(uid);
        let Some(mut pattern) = self.doc.project.roll(uid, slot).cloned() else {
            return false;
        };
        let kind = self.lane_kind(uid);
        let snap = self.view.dock.snap;
        self.settle_fold_view(uid);
        let mut cursor = self.view.panel(uid).cursor;
        let scale = self.snap_key(kind);
        let from = cursor.pitch;
        let changed = apply_entry(&mut pattern, &mut cursor, entry, kind, snap, scale);
        // On a folded lane up/down go to the next shown row.
        let up = match entry {
            EntryKey::Up => Some(true),
            EntryKey::Down => Some(false),
            _ => None,
        };
        let fold = self
            .doc
            .project
            .machine(uid)
            .is_some_and(|m| m.lane.fold && !m.lane.collapsed);
        if let Some(up) = up
            && fold
            && let super::lane_geom::Rows::Folded(rows) = self.lane_rows(uid, true, &pattern)
        {
            cursor.pitch = rows.step(from, up);
        }
        self.view.panel(uid).cursor = cursor;
        if changed {
            self.commit_poly(uid, slot, Some(pattern), false, true);
        }
        true
    }

    /// The project key when snap to scale applies to a lane of `kind`:
    /// snap is on, the scale isn't Chromatic and the lane is melodic.
    pub(crate) fn snap_key(&self, kind: LaneKind) -> Option<acid_engine::scale::Key> {
        let key = self.doc.project.key;
        (key.snap && key.scale != Scale::Chromatic && kind != LaneKind::Drums).then_some(key)
    }

    /// A lane's mouse move or resize is in progress, or a note tool is
    /// open (its preview is redone every frame): keyboard, clipboard and
    /// undo edits wait until it is dropped, applied or cancelled.
    pub(crate) fn mouse_editing(&self) -> bool {
        use super::poly_edit::LaneDrag;
        use super::song_lane::SongDrag;
        self.view.tool.is_some()
            || self.view.panels.values().any(|p| {
                matches!(
                    p.drag,
                    Some(LaneDrag::Move { .. } | LaneDrag::Resize { .. })
                ) || matches!(
                    p.song_drag,
                    Some(SongDrag::Move { .. } | SongDrag::Resize { .. })
                )
            })
    }

    /// Forgets the ruler range and the selections it made in the other
    /// lanes; the focused lane keeps its own, which the caller handles.
    pub(crate) fn drop_range(&mut self) {
        if self.view.dock.range.take().is_none() {
            return;
        }
        let focus = self.focused();
        for (uid, panel) in self.view.panels.iter_mut() {
            if Some(*uid) != focus {
                panel.selected.clear();
                panel.song_sel.clear();
            }
        }
    }

    /// A key acting on `uid`'s selection: arrows move it (Alt: by ticks;
    /// Shift+up/down: by octaves on melodic lanes), Delete/Backspace remove
    /// it, Esc clears it and the ruler range. `None` when the key is not
    /// one of these or there is nothing selected, so it goes on to entry.
    fn selection_key(
        &mut self,
        uid: MachineUid,
        key: egui::Key,
        mods: egui::Modifiers,
    ) -> Option<bool> {
        use super::note_ops::{delete, settle, shift, transpose_in_key, values};
        use egui::Key;
        if mods.command || mods.ctrl {
            return None;
        }
        let sel = self.selection(uid);
        if key == Key::Escape {
            if self.view.dock.range.take().is_some() {
                for panel in self.view.panels.values_mut() {
                    panel.selected.clear();
                }
            } else if sel.is_empty() {
                return None;
            }
            self.set_selection(uid, Vec::new());
            return Some(true);
        }
        if sel.is_empty() {
            return None;
        }
        let kind = self.lane_kind(uid);
        let step = if mods.alt {
            1
        } else {
            self.view.dock.snap.ticks() as i32
        };
        let octave = if kind == LaneKind::Drums { 0 } else { 12 };
        let pitch = if mods.shift { octave } else { 1 };
        let (dt, dp) = match key {
            Key::ArrowLeft => (-step, 0),
            Key::ArrowRight => (step, 0),
            Key::ArrowUp => (0, pitch),
            Key::ArrowDown => (0, -pitch),
            Key::Delete | Key::Backspace => (0, 0),
            _ => return None,
        };
        let slot = self.selected(uid);
        let original = self.doc.project.roll(uid, slot)?;
        let mut pattern = original.clone();
        if matches!(key, Key::Delete | Key::Backspace) {
            delete(&mut pattern, &sel);
            self.set_selection(uid, Vec::new());
        } else if (dt, dp) == (0, 0) {
            return Some(true);
        } else {
            let key = self.snap_key(kind);
            let moved = match key {
                // With snap to scale, up/down step by scale degrees.
                Some(key) if dt == 0 && !mods.shift => {
                    transpose_in_key(&mut pattern, &sel, dp > 0, key)
                }
                _ => shift(&mut pattern, &sel, dt, dp, kind),
            };
            let kept = settle(&mut pattern, &moved, kind);
            if pattern == *original {
                // Clamped to nothing: no edit, and the range still holds.
                return Some(true);
            }
            self.set_selection(uid, values(&pattern, &kept));
        }
        // The range no longer describes what is selected.
        self.drop_range();
        self.commit_poly(uid, slot, Some(pattern), false, true);
        Some(true)
    }

    /// Applies one undo step of note pattern edits (a group when there
    /// are several) and sends their commands.
    fn apply_note_edits(&mut self, mut edits: Vec<(MachineUid, u8, NotePattern)>) {
        let edit = |(uid, slot, p): (MachineUid, u8, NotePattern)| Edit::NotePattern {
            uid,
            slot,
            pattern: Box::new(p),
        };
        let edit = match edits.len() {
            0 => return,
            1 => edit(edits.remove(0)),
            _ => Edit::Group(edits.into_iter().map(edit).collect()),
        };
        let cmds = self.doc.apply(edit, false);
        self.send(cmds);
        self.doc.end_gesture();
    }

    /// Shows why a paste was refused.
    fn paste_refused(&mut self, e: PasteError) {
        self.view.message = Some(match e {
            PasteError::TooMany => super::note_ops::too_many_notes(),
            PasteError::WrongKind => "Melodic notes can't be pasted onto drums".into(),
        });
    }

    /// Copies to the note clipboard: with a ruler range, every lane's notes
    /// starting in it (relative to its start); else the focused lane's
    /// selection, or the whole visible lane when nothing is selected
    /// (relative to the earliest start). With `cut` the copied notes are
    /// then removed, in one undo step; a cut with nothing selected and no
    /// range does nothing. Nothing to copy leaves the clipboard as it was.
    pub(crate) fn copy_notes(&mut self, cut: bool) {
        use super::note_ops::{copy, delete, in_range};
        if self.mouse_editing() {
            return;
        }
        let mut lanes = Vec::new();
        let mut edits = Vec::new();
        let mut take = |app: &mut Self, uid: MachineUid, idx: Vec<usize>, origin: u16| {
            let slot = app.selected(uid);
            let Some(p) = app.doc.project.roll(uid, slot) else {
                return;
            };
            lanes.push((uid, app.lane_kind(uid), copy(p, &idx, origin)));
            if cut && !idx.is_empty() {
                let mut p = p.clone();
                delete(&mut p, &idx);
                edits.push((uid, slot, p));
                app.set_selection(uid, Vec::new());
            }
        };
        if let Some((lo, hi)) = self.view.dock.range {
            for uid in self.doc.project.rack_uids() {
                let Some(p) = self.doc.project.roll(uid, self.selected(uid)) else {
                    continue;
                };
                let idx = in_range(p, lo, hi);
                take(self, uid, idx, lo);
            }
        } else {
            let Some(uid) = self.focused() else {
                return;
            };
            let Some(p) = self.doc.project.roll(uid, self.selected(uid)) else {
                return;
            };
            let mut idx = self.selection(uid);
            if idx.is_empty() {
                if cut {
                    // Cut needs a selection; it never takes the whole lane.
                    return;
                }
                let visible = p.ticks();
                idx = (0..p.notes.len())
                    .filter(|&i| p.notes[i].start < visible)
                    .collect();
            }
            let Some(origin) = idx.iter().map(|&i| p.notes[i].start).min() else {
                return;
            };
            take(self, uid, idx, origin);
        }
        if lanes.iter().all(|(_, _, notes)| notes.is_empty()) {
            return;
        }
        self.view.note_clip = Some(NoteClip { lanes });
        if cut && self.view.dock.range.is_some() {
            // The selected notes are gone, in every lane.
            self.drop_range();
            if let Some(uid) = self.focused() {
                self.set_selection(uid, Vec::new());
            }
        }
        self.apply_note_edits(edits);
    }

    /// Pastes the note clipboard at the focused lane's cursor: a one-lane
    /// clip into the focused lane (melodic onto melodic, drums onto
    /// drums), a range clip back into the machines it came from (those no
    /// longer in the rack are skipped). One undo step; the pasted notes
    /// become each lane's selection. A refused paste changes nothing.
    pub(crate) fn paste_notes(&mut self) {
        use super::note_ops::{paste, values};
        if self.mouse_editing() {
            return;
        }
        let (Some(clip), Some(focus)) = (self.view.note_clip.clone(), self.focused()) else {
            return;
        };
        let melodic = |k: LaneKind| k != LaneKind::Drums;
        let single = clip.lanes.len() == 1;
        let at = self.view.panel(focus).cursor.tick;
        let targets: Vec<(MachineUid, &Vec<Note>)> = if let [(_, kind, notes)] = &clip.lanes[..] {
            let to = self.lane_kind(focus);
            if melodic(*kind) != melodic(to) {
                self.view.message = Some(
                    if melodic(to) {
                        "Drum notes can't be pasted onto a melodic part"
                    } else {
                        "Melodic notes can't be pasted onto drums"
                    }
                    .into(),
                );
                return;
            }
            vec![(focus, notes)]
        } else {
            clip.lanes
                .iter()
                .filter(|(uid, kind, _)| {
                    self.doc.project.machine(*uid).is_some()
                        && melodic(*kind) == melodic(self.lane_kind(*uid))
                })
                .map(|(uid, _, notes)| (*uid, notes))
                .collect()
        };
        let mut edits = Vec::new();
        let mut selections = Vec::new();
        for (uid, notes) in targets {
            let slot = self.selected(uid);
            let Some(mut p) = self.doc.project.roll(uid, slot).cloned() else {
                continue;
            };
            if notes.is_empty() {
                continue;
            }
            match paste(&mut p, notes, at, self.lane_kind(uid)) {
                Ok(idx) => {
                    selections.push((uid, values(&p, &idx)));
                    edits.push((uid, slot, p));
                }
                Err(e) => return self.paste_refused(e),
            }
        }
        if single {
            self.drop_range();
        } else {
            self.view.dock.range = None;
        }
        for (uid, notes) in selections {
            self.set_selection(uid, notes);
        }
        self.apply_note_edits(edits);
    }

    /// Pastes a copy of the focused lane's selection right after it (its
    /// span rounded up to the snap). With a ruler range, every lane's
    /// notes in the range are repeated one range length later instead,
    /// and the range moves onto the copies. One undo step.
    pub(crate) fn duplicate_notes(&mut self) {
        use super::note_ops::{copy, duplicate, in_range, paste, values};
        if self.mouse_editing() {
            return;
        }
        let mut edits = Vec::new();
        let mut selections = Vec::new();
        if let Some((lo, hi)) = self.view.dock.range {
            for uid in self.doc.project.rack_uids() {
                let slot = self.selected(uid);
                let Some(mut p) = self.doc.project.roll(uid, slot).cloned() else {
                    continue;
                };
                let idx = in_range(&p, lo, hi);
                if idx.is_empty() {
                    continue;
                }
                let clip = copy(&p, &idx, lo);
                match paste(&mut p, &clip, hi, self.lane_kind(uid)) {
                    Ok(idx) => {
                        selections.push((uid, values(&p, &idx)));
                        edits.push((uid, slot, p));
                    }
                    Err(e) => return self.paste_refused(e),
                }
            }
            self.view.dock.range = Some((hi, hi.saturating_add(hi - lo)));
        } else {
            let Some(uid) = self.focused() else {
                return;
            };
            let sel = self.selection(uid);
            let slot = self.selected(uid);
            let Some(mut p) = self.doc.project.roll(uid, slot).cloned() else {
                return;
            };
            if sel.is_empty() {
                return;
            }
            let snap = self.view.dock.snap.ticks();
            match duplicate(&mut p, &sel, snap, self.lane_kind(uid)) {
                Ok(idx) => {
                    selections.push((uid, values(&p, &idx)));
                    edits.push((uid, slot, p));
                }
                Err(e) => return self.paste_refused(e),
            }
        }
        for (uid, notes) in selections {
            self.set_selection(uid, notes);
        }
        self.apply_note_edits(edits);
    }

    /// Selects every note of the focused lane's pattern that starts inside
    /// it (and drops the ruler range).
    pub(crate) fn select_all_notes(&mut self) {
        if self.mouse_editing() {
            return;
        }
        let Some(uid) = self.focused() else {
            return;
        };
        let Some(p) = self.doc.project.roll(uid, self.selected(uid)) else {
            return;
        };
        // Not the notes hidden past the pattern's end.
        let visible = p.ticks();
        let notes: Vec<Note> = p
            .notes
            .iter()
            .filter(|n| n.start < visible)
            .copied()
            .collect();
        self.drop_range();
        self.set_selection(uid, notes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::project::Project;

    #[test]
    fn the_whole_app_draws_with_the_dock() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(acid_engine::machine::MachineType::Mono);
        crate::ui::run_frames(&mut app, 3);
        for uid in app.doc.project.rack_uids() {
            let melodic = app.lane_kind(uid) != crate::ui::key_entry::LaneKind::Drums;
            assert_eq!(
                app.view.panel(uid).lane_scroll_y.is_some(),
                melodic,
                "{uid:?} lane drawn"
            );
        }
    }

    #[test]
    fn note_keys_write_into_the_focused_lane_only_with_dock_focus() {
        use crate::test_rack::FM;
        use eframe::egui::{Key, Modifiers};
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.view.focus = Some(FM);
        app.view.panel(FM).cursor = crate::ui::key_entry::Cursor { tick: 0, pitch: 60 };
        assert!(!app.dock_key(Key::Z, Modifiers::NONE), "no dock focus");
        assert!(app.doc.project.roll(FM, 0).unwrap().notes.is_empty());
        app.view.dock.keyboard = true;
        assert!(app.dock_key(Key::Z, Modifiers::NONE));
        assert!(app.dock_key(Key::X, Modifiers::NONE));
        let notes = &app.doc.project.roll(FM, 0).unwrap().notes;
        assert_eq!(
            notes.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(),
            [(0, 60), (4, 62)]
        );
        assert!(
            !app.dock_key(Key::Z, Modifiers::COMMAND),
            "Ctrl+Z is not a note"
        );
        app.undo();
        assert_eq!(
            app.doc.project.roll(FM, 0).unwrap().notes.len(),
            1,
            "one undo per key"
        );
    }

    #[test]
    fn arrows_move_the_cursor_without_editing() {
        use crate::test_rack::KIT808;
        use eframe::egui::{Key, Modifiers};
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.view.focus = Some(KIT808);
        app.view.dock.keyboard = true;
        assert!(app.dock_key(Key::ArrowRight, Modifiers::NONE));
        assert!(app.dock_key(Key::ArrowUp, Modifiers::NONE));
        assert_eq!(
            app.view.panel(KIT808).cursor,
            crate::ui::key_entry::Cursor { tick: 4, pitch: 1 }
        );
        assert!(app.doc.project.roll(KIT808, 0).unwrap().notes.is_empty());
        assert!(!app.doc.is_dirty());
    }

    /// Run one frame that calls `handle_shortcuts` on `events`; returns the
    /// events left in the frame afterwards.
    fn shortcut_frame(app: &mut SidAcidApp, events: Vec<egui::Event>) -> Vec<egui::Event> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut left = Vec::new();
        let _ = ctx.run(input, |ctx| {
            app.handle_shortcuts(ctx);
            left = ctx.input(|i| i.events.clone());
        });
        left
    }

    fn key_event(key: egui::Key, pressed: bool, repeat: bool, command: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat,
            modifiers: if command {
                egui::Modifiers::COMMAND
            } else {
                egui::Modifiers::NONE
            },
        }
    }

    fn typing_app() -> SidAcidApp {
        use crate::test_rack::FM;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.view.focus = Some(FM);
        app.view.dock.keyboard = true;
        app.view.panel(FM).cursor = crate::ui::key_entry::Cursor { tick: 0, pitch: 60 };
        app
    }

    #[test]
    fn frame_key_presses_and_repeats_write_notes_and_are_stripped() {
        use crate::test_rack::FM;
        use egui::Key;
        let mut app = typing_app();
        let left = shortcut_frame(
            &mut app,
            vec![
                key_event(Key::Z, true, false, false),
                key_event(Key::Z, true, true, false),
                key_event(Key::Z, false, false, false),
            ],
        );
        assert_eq!(
            app.doc.project.roll(FM, 0).unwrap().notes.len(),
            2,
            "repeat counts"
        );
        assert_eq!(left.len(), 1, "presses gone: {left:?}");
        assert!(matches!(left[0], egui::Event::Key { pressed: false, .. }));
    }

    #[test]
    fn space_and_ctrl_z_still_work_with_dock_focus() {
        use crate::test_rack::FM;
        use acid_engine::host::Command;
        use egui::Key;
        let mut app = typing_app();
        app.status.playing = false;
        app.pending.clear();
        shortcut_frame(&mut app, vec![key_event(Key::Space, true, false, false)]);
        assert!(app.pending.iter().any(|c| matches!(c, Command::Play)));
        assert!(app.doc.project.roll(FM, 0).unwrap().notes.is_empty());
        shortcut_frame(&mut app, vec![key_event(Key::Z, true, false, false)]);
        assert_eq!(app.doc.project.roll(FM, 0).unwrap().notes.len(), 1);
        shortcut_frame(&mut app, vec![key_event(Key::Z, true, false, true)]);
        assert!(
            app.doc.project.roll(FM, 0).unwrap().notes.is_empty(),
            "undone"
        );
    }

    #[test]
    fn a_message_up_leaves_keys_alone() {
        use crate::test_rack::FM;
        let mut app = typing_app();
        app.view.message = Some("hello".into());
        let left = shortcut_frame(&mut app, vec![key_event(egui::Key::Z, true, false, false)]);
        assert!(app.doc.project.roll(FM, 0).unwrap().notes.is_empty());
        assert_eq!(left.len(), 1, "event left in the frame");
    }

    #[test]
    fn zoom_steps_through_the_zoom_levels() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        assert_eq!(app.view.dock.zoom, 2);
        app.zoom_dock(1);
        app.zoom_dock(1);
        app.zoom_dock(1);
        assert_eq!(app.view.dock.zoom, 4, "clamped at the top");
        app.zoom_dock(-9);
        assert_eq!(app.view.dock.zoom, 0);
    }

    #[test]
    fn a_dock_resize_is_remembered_in_settings() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.finish_dock_resize(0.6);
        assert_eq!(app.settings.ui.dock_frac, 0.6);
    }

    /// One 1280x800 frame of the whole app at `time` with `events`.
    fn frame(app: &mut SidAcidApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
        frame_in(app, ctx, 800.0, time, events);
    }

    /// One frame of the whole app in a 1280 x `window_h` window.
    fn frame_in(
        app: &mut SidAcidApp,
        ctx: &egui::Context,
        window_h: f32,
        time: f64,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, window_h))),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
    }

    /// Presses at `from`, moves by `by` over `steps` frames, releases, then
    /// lets a second of frames go by (for any fling to run out). All in a
    /// 1280x800 window from egui time `time` on.
    fn drag_by(
        app: &mut SidAcidApp,
        ctx: &egui::Context,
        from: egui::Pos2,
        by: egui::Vec2,
        steps: usize,
        time: f64,
    ) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut t = time;
        let mut tick = |app: &mut SidAcidApp, events| {
            frame(app, ctx, t, events);
            t += 1.0 / 60.0;
        };
        tick(app, vec![egui::Event::PointerMoved(from)]);
        tick(app, vec![button(from, true)]);
        for i in 1..=steps {
            let at = from + by * (i as f32 / steps as f32);
            tick(app, vec![egui::Event::PointerMoved(at)]);
        }
        tick(app, vec![button(from + by, false)]);
        for _ in 0..60 {
            tick(app, vec![]);
        }
    }

    #[test]
    fn a_drag_on_a_lane_header_does_not_pan_the_lane_stack() {
        use crate::test_rack::SYNTH1;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        assert_eq!(app.doc.project.rack_uids()[0], SYNTH1, "Synth 1 on top");
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let lanes = app.view.dock.lanes.expect("the lanes were drawn");
        // Synth 1's header row, between its labels and its Strip toggle.
        let from = pos2(
            lanes.center().x,
            lanes.top() + crate::ui::lane::HEADER_H / 2.0,
        );
        drag_by(&mut app, &ctx, from, vec2(0.0, -150.0), 6, 1.0);
        assert_eq!(app.view.dock.lanes_offset, 0.0, "the stack did not move");
    }

    #[test]
    fn the_dock_keeps_its_share_when_the_window_is_resized() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let ctx = egui::Context::default();
        frame_in(&mut app, &ctx, 800.0, 0.0, vec![]);
        frame_in(&mut app, &ctx, 800.0, 0.1, vec![]);
        frame_in(&mut app, &ctx, 1044.0, 0.2, vec![]);
        frame_in(&mut app, &ctx, 1044.0, 0.3, vec![]);
        let h = app.view.dock.rect.unwrap().height();
        assert!((h - 0.45 * 1044.0).abs() < 2.0, "matched the share: {h}");
        assert_eq!(app.settings.ui.dock_frac, 0.45, "nothing saved");
        // Still resizable by hand afterwards.
        let top = app.view.dock.rect.unwrap().top();
        let to = pos2(640.0, top - 30.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let from = pos2(640.0, top);
        frame_in(
            &mut app,
            &ctx,
            1044.0,
            1.0,
            vec![egui::Event::PointerMoved(from)],
        );
        frame_in(&mut app, &ctx, 1044.0, 1.05, vec![button(from, true)]);
        frame_in(
            &mut app,
            &ctx,
            1044.0,
            1.1,
            vec![egui::Event::PointerMoved(to)],
        );
        frame_in(&mut app, &ctx, 1044.0, 1.15, vec![button(to, false)]);
        frame_in(&mut app, &ctx, 1044.0, 1.2, vec![]);
        let grown = app.view.dock.rect.unwrap().height();
        assert!((grown - (h + 30.0)).abs() < 2.0, "dragged: {grown}");
        let frac = app.settings.ui.dock_frac;
        assert!((frac - grown / 1044.0).abs() < 1e-3, "kept: {frac}");
    }

    #[test]
    fn the_lane_stack_shows_a_solid_scroll_bar() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        frame(&mut app, &ctx, 0.1, vec![]);
        let (lanes, ruler) = (app.view.dock.lanes.unwrap(), app.view.dock.ruler.unwrap());
        // A floating bar takes no room; a solid one does.
        assert!(
            lanes.right() < ruler.right() - 5.0,
            "room kept for the bar: {lanes:?} {ruler:?}"
        );
    }

    #[test]
    fn only_the_ruler_thumb_strip_drags_the_lanes_sideways() {
        use crate::test_rack::FM;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.doc.project.roll_mut(FM, 0).unwrap().length = 64;
        app.view.dock.follow = false;
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let ruler = app.view.dock.ruler.unwrap();
        let left = ruler.left() + app.lane_key_w();
        // On the step numbers: nothing (kept for selecting a time range).
        let numbers = pos2(left + 40.0, ruler.top() + 4.0);
        drag_by(&mut app, &ctx, numbers, vec2(60.0, 0.0), 4, 1.0);
        assert_eq!(app.view.dock.scroll_x, 0.0, "the numbers do not scroll");
        // On the thumb strip along the bottom: scrolls.
        let thumb = pos2(left + 10.0, ruler.bottom() - 2.0);
        drag_by(&mut app, &ctx, thumb, vec2(60.0, 0.0), 4, 3.0);
        let x = app.view.dock.scroll_x;
        assert!((x - 60.0).abs() < 1.0, "the thumb scrolls: {x}");
    }

    fn range_app() -> SidAcidApp {
        use crate::test_rack::{FM, SYNTH1};
        use acid_engine::project::Note;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        for uid in [FM, SYNTH1] {
            let p = app.doc.project.roll_mut(uid, 0).unwrap();
            p.length = 64;
            p.notes = vec![
                Note::new(0, 4, 60, 100),
                Note::new(16, 4, 62, 100),
                Note::new(40, 4, 64, 100),
            ];
        }
        app.view.dock.follow = false;
        app
    }

    #[test]
    fn a_ruler_drag_selects_the_range_in_every_lane() {
        use crate::test_rack::{FM, SYNTH1};
        let mut app = range_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let before = app.doc.project.clone();
        let ruler = app.view.dock.ruler.unwrap();
        let left = ruler.left() + app.lane_key_w();
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        // Ticks 14..30 (snap is a step of 4): press inside 12, release inside 28.
        let from = pos2(left + 14.0 * tw, ruler.top() + 4.0);
        drag_by(&mut app, &ctx, from, vec2(14.0 * tw, 0.0), 4, 1.0);
        assert_eq!(app.view.dock.range, Some((12, 32)));
        for uid in [FM, SYNTH1] {
            assert_eq!(app.selection(uid), vec![1], "the note at 16 only");
        }
        assert_eq!(app.doc.project, before, "no edit to the project");
    }

    #[test]
    fn a_ruler_range_keeps_its_start_when_the_lanes_scroll_mid_drag() {
        let mut app = range_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let ruler = app.view.dock.ruler.unwrap();
        let left = ruler.left() + app.lane_key_w();
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        let y = ruler.top() + 4.0;
        let button = |x: f32, pressed| egui::Event::PointerButton {
            pos: pos2(x, y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let moved = |x: f32| egui::Event::PointerMoved(pos2(x, y));
        let x0 = left + 14.0 * tw;
        frame(&mut app, &ctx, 1.0, vec![moved(x0)]);
        frame(&mut app, &ctx, 1.05, vec![button(x0, true)]);
        frame(&mut app, &ctx, 1.1, vec![moved(left + 20.0 * tw)]);
        assert_eq!(app.view.dock.range, Some((12, 24)));
        // Follow (or anything) scrolls the lanes 32 ticks on mid-drag.
        app.view.dock.scroll_x = 32.0 * tw;
        let x1 = left + 28.0 * tw;
        frame(&mut app, &ctx, 1.15, vec![moved(x1)]);
        frame(&mut app, &ctx, 1.2, vec![button(x1, false)]);
        assert_eq!(app.view.dock.range, Some((12, 64)), "the start stays at 12");
    }

    #[test]
    fn a_ruler_click_clears_the_range() {
        use crate::test_rack::FM;
        let mut app = range_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let ruler = app.view.dock.ruler.unwrap();
        let left = ruler.left() + app.lane_key_w();
        app.view.dock.range = Some((0, 8));
        app.set_selection(FM, vec![acid_engine::project::Note::new(0, 4, 60, 100)]);
        press(&mut app, &ctx, pos2(left + 30.0, ruler.top() + 4.0), 1.0);
        assert_eq!(app.view.dock.range, None);
        assert_eq!(app.selection(FM), vec![0], "the selection stays");
        // A click in a lane grid clears a range too.
        app.view.dock.range = Some((0, 8));
        let lanes = app.view.dock.lanes.unwrap();
        press(&mut app, &ctx, pos2(left + 200.0, lanes.top() + 50.0), 3.0);
        assert_eq!(app.view.dock.range, None, "a lane click clears it");
    }

    #[test]
    fn a_lane_press_drops_the_selections_the_range_made() {
        use crate::test_rack::{FM, SYNTH1};
        let mut app = range_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        app.view.dock.range = Some((12, 32));
        app.select_range(12, 32);
        assert_eq!(app.selection(FM), vec![1]);
        // A press in Synth 1's grid (the top lane), clear of its notes.
        let left = app.view.dock.ruler.unwrap().left() + app.lane_key_w();
        let lanes = app.view.dock.lanes.unwrap();
        press(&mut app, &ctx, pos2(left + 200.0, lanes.top() + 50.0), 1.0);
        assert_eq!(app.view.focus, Some(SYNTH1));
        assert_eq!(app.view.dock.range, None);
        assert!(app.selection(FM).is_empty(), "the other lane let go");
    }

    #[test]
    fn the_thumb_strip_still_scrolls() {
        let mut app = range_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let ruler = app.view.dock.ruler.unwrap();
        let left = ruler.left() + app.lane_key_w();
        let thumb = pos2(left + 10.0, ruler.bottom() - 2.0);
        drag_by(&mut app, &ctx, thumb, vec2(60.0, 0.0), 4, 1.0);
        let x = app.view.dock.scroll_x;
        assert!((x - 60.0).abs() < 1.0, "scrolled: {x}");
        assert_eq!(app.view.dock.range, None, "no range from the strip");
    }

    fn song_app() -> SidAcidApp {
        use crate::test_rack::SYNTH1;
        use acid_engine::project::{Block, PatternRef};
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.doc.project.song.lane_mut(SYNTH1).extend([
            Block::new(0, PatternRef(0)),
            Block::new(16, PatternRef(1)),
            Block::new(48, PatternRef(2)),
        ]);
        app.view.mode = Mode::Song;
        app.view.dock.zoom = 0;
        app.view.dock.follow = false;
        app
    }

    #[test]
    fn the_song_ruler_draws_a_converted_songs_end() {
        let mut app = song_app();
        app.doc.project.song.end = Some(160);
        app.doc.project.song.loop_range = Some((16, 192));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        app.doc.project.song = Default::default();
        frame(&mut app, &ctx, 0.1, vec![]);
    }

    #[test]
    fn a_song_ruler_click_sets_the_song_cursor_on_a_bar() {
        use acid_engine::host::Command;
        let mut app = song_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let ruler = app.view.dock.ruler.unwrap();
        let left = ruler.left() + app.lane_key_w();
        let sw = song_lane::step_w(app.view.dock.zoom);
        app.view.dock.range = Some((0, 16));
        app.flushed.clear();
        press(
            &mut app,
            &ctx,
            pos2(left + 20.5 * sw, ruler.top() + 4.0),
            1.0,
        );
        assert_eq!(app.view.song_cursor, 16);
        assert_eq!(app.view.dock.range, None, "the range goes");
        assert!(
            app.flushed
                .iter()
                .any(|c| matches!(c, Command::SetSongStart(16))),
            "the engine starts there"
        );
        assert!(!app.doc.is_dirty());
    }

    #[test]
    fn a_song_ruler_drag_picks_bars_and_selects_their_blocks() {
        use crate::test_rack::SYNTH1;
        let mut app = song_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let before = app.doc.project.clone();
        let ruler = app.view.dock.ruler.unwrap();
        let left = ruler.left() + app.lane_key_w();
        let sw = song_lane::step_w(app.view.dock.zoom);
        let from = pos2(left + 20.5 * sw, ruler.top() + 4.0);
        drag_by(&mut app, &ctx, from, vec2(16.0 * sw, 0.0), 4, 1.0);
        assert_eq!(app.view.dock.range, Some((16, 48)), "whole bars, in steps");
        let lane = app.doc.project.song.lane(SYNTH1);
        let starts: Vec<u32> = app
            .block_selection(SYNTH1)
            .iter()
            .map(|&i| lane[i].start)
            .collect();
        assert_eq!(starts, [16], "the block starting in the range");
        assert_eq!(app.doc.project, before, "no edit");
    }

    #[test]
    fn note_keys_and_note_entry_do_nothing_in_song_mode() {
        use crate::test_rack::{FM, SYNTH1};
        use acid_engine::project::Note;
        let mut app = song_app();
        for uid in [FM, SYNTH1] {
            let p = app.doc.project.roll_mut(uid, 0).unwrap();
            p.length = 64;
            p.notes = vec![Note::new(16, 4, 60, 100), Note::new(40, 4, 62, 100)];
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let ruler = app.view.dock.ruler.unwrap();
        let left = ruler.left() + app.lane_key_w();
        let sw = song_lane::step_w(app.view.dock.zoom);
        // A range of bars 1..3 (steps 0..48) on the ruler, dock focused.
        let from = pos2(left + 4.5 * sw, ruler.top() + 4.0);
        drag_by(&mut app, &ctx, from, vec2(32.0 * sw, 0.0), 4, 1.0);
        assert_eq!(app.view.dock.range, Some((0, 48)));
        app.view.dock.keyboard = true;
        app.view.focus = Some(FM);
        let before = app.doc.project.clone();
        let cmd = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        use egui::Key;
        for (t, ev) in [
            cmd(Key::X),
            egui::Event::Cut,
            cmd(Key::D),
            cmd(Key::V),
            key_event(Key::Z, true, false, false),
            key_event(Key::Delete, true, false, false),
        ]
        .into_iter()
        .enumerate()
        {
            frame(&mut app, &ctx, 3.0 + t as f64, vec![ev]);
        }
        assert_eq!(
            app.doc.project.machines, before.machines,
            "no pattern changed"
        );
        assert!(app.view.note_clip.is_none());
        assert!(app.view.block_clip.is_some(), "the blocks were cut instead");
    }

    #[test]
    fn the_whole_app_draws_in_song_mode_while_playing() {
        let mut app = song_app();
        app.status.playing = true;
        app.status.song_step = 20;
        app.doc.project.song.loop_range = Some((16, 32));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        frame(&mut app, &ctx, 0.1, vec![]);
        assert!(app.view.dock.lanes.is_some());
    }

    fn press(app: &mut SidAcidApp, ctx: &egui::Context, at: egui::Pos2, time: f64) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(app, ctx, time, vec![egui::Event::PointerMoved(at)]);
        frame(app, ctx, time + 0.05, vec![button(true)]);
        frame(app, ctx, time + 0.1, vec![button(false)]);
    }

    #[test]
    fn the_dock_takes_its_share_of_the_window_at_the_bottom() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let rect = app.view.dock.rect.expect("the dock was drawn");
        assert!((rect.bottom() - 800.0).abs() < 1.0, "{rect:?}");
        assert!((rect.height() - 800.0 * 0.45).abs() < 2.0, "{rect:?}");
    }

    #[test]
    fn a_press_outside_the_dock_takes_its_keyboard_focus() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let rect = app.view.dock.rect.unwrap();
        app.view.dock.keyboard = true;
        // In the dock, on its header row clear of the buttons: kept.
        press(
            &mut app,
            &ctx,
            pos2(rect.right() - 4.0, rect.top() + 12.0),
            1.0,
        );
        assert!(app.view.dock.keyboard, "a press inside keeps it");
        // In the rack's left gutter: gone.
        press(&mut app, &ctx, pos2(4.0, rect.top() - 40.0), 2.0);
        assert!(!app.view.dock.keyboard, "a press outside clears it");
    }

    #[test]
    fn follow_keeps_the_focused_playhead_in_view() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.focused().unwrap();
        app.doc.project.roll_mut(uid, 0).unwrap().length = 64;
        app.status.playing = true;
        app.status.playhead[0] = 60;
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        let x = 60.0 * TICKS_PER_STEP as f32 * tw;
        let scroll = app.view.dock.scroll_x;
        assert!(
            scroll > 0.0 && scroll < x,
            "scrolled to the playhead: {scroll}"
        );
        app.view.dock.follow = false;
        app.view.dock.scroll_x = 0.0;
        frame(&mut app, &ctx, 0.1, vec![]);
        assert_eq!(app.view.dock.scroll_x, 0.0, "no follow, no scroll");
    }

    /// Presses at `from`, moves `dy` in one frame, releases; returns the
    /// dock's height after.
    fn drag(app: &mut SidAcidApp, ctx: &egui::Context, from: egui::Pos2, dy: f32) -> f32 {
        let to = from + vec2(0.0, dy);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(app, ctx, 1.0, vec![egui::Event::PointerMoved(from)]);
        frame(app, ctx, 1.05, vec![button(from, true)]);
        frame(app, ctx, 1.1, vec![egui::Event::PointerMoved(to)]);
        frame(app, ctx, 1.15, vec![button(to, false)]);
        frame(app, ctx, 1.2, vec![]);
        app.view.dock.rect.unwrap().height()
    }

    #[test]
    fn a_fast_drag_on_the_dock_edge_saves_the_new_size() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let top = app.view.dock.rect.unwrap().top();
        let h = drag(&mut app, &ctx, pos2(640.0, top), -30.0);
        assert!((h - (360.0 + 30.0)).abs() < 2.0, "the dock grew: {h}");
        let frac = app.settings.ui.dock_frac;
        assert!((frac - h / 800.0).abs() < 1e-3, "kept: {frac} for {h}");
        assert!(!app.view.dock.resizing);
    }

    #[test]
    fn a_drag_inside_the_dock_keeps_its_size() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let rect = app.view.dock.rect.unwrap();
        drag(&mut app, &ctx, pos2(640.0, rect.top() + 120.0), -30.0);
        assert_eq!(app.settings.ui.dock_frac, 0.45);
        assert!(!app.view.dock.resizing);
    }

    fn notes_of(app: &SidAcidApp, uid: acid_engine::machine::MachineUid) -> Vec<(u16, u16, u8)> {
        app.doc
            .project
            .roll(uid, 0)
            .unwrap()
            .notes
            .iter()
            .map(|n| (n.start, n.len, n.pitch))
            .collect()
    }

    /// The test rack with `notes` in `uid`'s first pattern, dock focus on
    /// `uid`, and `sel` (indices) selected.
    fn sel_app(
        uid: acid_engine::machine::MachineUid,
        notes: &[(u16, u16, u8)],
        sel: &[usize],
    ) -> SidAcidApp {
        use acid_engine::project::Note;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let p = app.doc.project.roll_mut(uid, 0).unwrap();
        p.length = 64;
        p.notes = notes
            .iter()
            .map(|&(s, l, pi)| Note::new(s, l, pi, 100))
            .collect();
        let chosen = sel.iter().map(|&i| p.notes[i]).collect();
        app.view.focus = Some(uid);
        app.view.dock.keyboard = true;
        app.set_selection(uid, chosen);
        app
    }

    #[test]
    fn arrows_alt_and_shift_move_and_transpose_the_selection() {
        use crate::test_rack::FM;
        use egui::{Key, Modifiers};
        let mut app = sel_app(FM, &[(0, 4, 60), (16, 4, 62)], &[0]);
        let before = app.doc.project.clone();
        assert!(app.dock_key(Key::ArrowRight, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM)[0], (4, 4, 60), "one snap");
        assert!(app.dock_key(Key::ArrowRight, Modifiers::ALT));
        assert_eq!(notes_of(&app, FM)[0], (5, 4, 60), "Alt: one tick");
        assert!(app.dock_key(Key::ArrowLeft, Modifiers::ALT));
        assert!(app.dock_key(Key::ArrowUp, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM)[0], (4, 4, 61), "a semitone");
        assert!(app.dock_key(Key::ArrowUp, Modifiers::SHIFT));
        assert_eq!(notes_of(&app, FM)[0], (4, 4, 73), "an octave");
        assert!(app.dock_key(Key::ArrowDown, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM)[0], (4, 4, 72));
        assert_eq!(app.selection(FM), vec![0], "still selected");
        assert_eq!(notes_of(&app, FM)[1], (16, 4, 62), "the other note stays");
        assert_eq!(app.view.panel(FM).cursor.tick, 0, "the cursor stays");
        for _ in 0..6 {
            app.undo();
        }
        assert_eq!(app.doc.project, before, "one undo per key");
    }

    #[test]
    fn with_snap_to_scale_up_and_down_step_by_scale_degrees() {
        use crate::test_rack::FM;
        use acid_engine::scale::Key as ScaleKey;
        use egui::{Key, Modifiers};
        // E, and C# (off C major).
        let mut app = sel_app(FM, &[(0, 4, 64), (16, 4, 61)], &[0, 1]);
        app.doc.project.key = ScaleKey {
            root: 0,
            scale: Scale::Major,
            snap: true,
        };
        assert!(app.dock_key(Key::ArrowUp, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM), [(0, 4, 65), (16, 4, 62)], "E->F, C#->D");
        assert!(app.dock_key(Key::ArrowUp, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM), [(0, 4, 67), (16, 4, 64)], "F->G, D->E");
        assert!(app.dock_key(Key::ArrowDown, Modifiers::SHIFT));
        assert_eq!(notes_of(&app, FM), [(0, 4, 55), (16, 4, 52)], "an octave");
        assert_eq!(app.selection(FM), vec![0, 1], "still selected");
        app.undo();
        app.undo();
        assert_eq!(
            notes_of(&app, FM),
            [(0, 4, 65), (16, 4, 62)],
            "one undo per key"
        );
        // Snap off: semitones again. (Undo left the old values unselected.)
        let all = app.doc.project.roll(FM, 0).unwrap().notes.clone();
        app.set_selection(FM, all);
        app.doc.project.key.snap = false;
        assert!(app.dock_key(Key::ArrowUp, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM), [(0, 4, 66), (16, 4, 63)]);
    }

    #[test]
    fn with_snap_to_scale_typed_notes_land_on_the_scale() {
        use crate::test_rack::FM;
        use egui::{Key, Modifiers};
        let mut app = sel_app(FM, &[], &[]);
        app.view.panel(FM).cursor.pitch = 48;
        app.doc.project.key.scale = Scale::Major;
        app.doc.project.key.snap = true;
        // S types C# of the cursor's octave: it lands on D.
        assert!(app.dock_key(Key::S, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM), [(0, 4, 50)]);
    }

    #[test]
    fn with_snap_to_scale_drums_still_move_by_one_row() {
        use crate::test_rack::KIT808;
        use egui::{Key, Modifiers};
        let mut app = sel_app(KIT808, &[(0, 4, 2)], &[0]);
        app.doc.project.key.scale = Scale::Major;
        app.doc.project.key.snap = true;
        assert!(app.dock_key(Key::ArrowUp, Modifiers::NONE));
        assert_eq!(notes_of(&app, KIT808), [(0, 4, 3)]);
    }

    #[test]
    fn the_header_picks_the_key_and_toggles_snap() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let ctx = egui::Context::default();
        click_tool(&mut app, &ctx, "Key root", 0.0);
        click_tool(&mut app, &ctx, "Root A", 1.0);
        assert_eq!(app.doc.project.key.root, 9);
        click_tool(&mut app, &ctx, "Key scale", 2.0);
        click_tool(&mut app, &ctx, "Scale Minor", 3.0);
        assert_eq!(app.doc.project.key.scale, Scale::Minor);
        assert_eq!(app.doc.undo_depth(), 2, "one undo step per change");
        assert!(app.doc.is_dirty());
        app.undo();
        assert_eq!(app.doc.project.key.scale, Scale::Chromatic);
        assert_eq!(app.doc.project.key.root, 9);
        app.undo();
        assert!(!app.doc.is_dirty());
        click_tool(&mut app, &ctx, "Snap to scale", 4.0);
        assert!(app.doc.project.key.snap);
        assert!(!app.doc.can_undo(), "the snap toggle is no undo step");
        assert!(!app.doc.is_dirty());
        click_tool(&mut app, &ctx, "Snap to scale", 5.0);
        assert!(!app.doc.project.key.snap);
        // Song mode has no key picker.
        app.view.mode = Mode::Song;
        frame(&mut app, &ctx, 6.0, vec![]);
        assert!(tool(&app, "Key root").is_none());
        assert!(tool(&app, "Snap to scale").is_none());
    }

    /// FM's lane folded in `root` major with the keyboard cursor on `pitch`.
    fn folded_key_app(root: u8, pitch: u8, notes: &[(u16, u16, u8)]) -> SidAcidApp {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, notes, &[]);
        app.doc.project.key = acid_engine::scale::Key {
            root,
            scale: Scale::Major,
            snap: false,
        };
        app.doc.project.machine_mut(FM).unwrap().lane.fold = true;
        app.view.panel(FM).cursor.pitch = pitch;
        app
    }

    #[test]
    fn undoing_a_key_change_leaves_the_cursor_on_a_shown_row() {
        use crate::test_rack::FM;
        use egui::{Key, Modifiers};
        // D major, cursor on C#4; C major hides C#4 (cursor to C4); undo
        // brings D major back, which hides C4: B3 is the drawn row.
        let mut app = folded_key_app(2, 61, &[]);
        let ctx = egui::Context::default();
        click_tool(&mut app, &ctx, "Key root", 0.0);
        click_tool(&mut app, &ctx, "Root C", 1.0);
        assert_eq!(app.doc.project.key.root, 0);
        assert_eq!(app.view.panel(FM).cursor.pitch, 60, "C4 is shown in C");
        app.undo();
        assert_eq!(app.doc.project.key.root, 2);
        assert_eq!(app.view.panel(FM).cursor.pitch, 59);
        // Typing C writes in the octave the cursor is drawn in.
        assert!(app.dock_key(Key::Z, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM), [(0, 4, 48)]);
    }

    #[test]
    fn removing_the_held_note_under_the_cursor_leaves_typing_on_the_drawn_row() {
        use crate::test_rack::FM;
        use egui::{Key, Modifiers};
        // C4 is off D major but held, so it has a row; Backspace removes
        // the note and the row, and the cursor is drawn on B3.
        let mut app = folded_key_app(2, 60, &[(0, 4, 60)]);
        assert!(app.dock_key(Key::Backspace, Modifiers::NONE));
        assert!(notes_of(&app, FM).is_empty());
        assert!(app.dock_key(Key::Z, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM), [(0, 4, 48)], "the octave of B3");
    }

    #[test]
    fn a_key_change_keeps_a_folded_lanes_middle_pitch_in_view() {
        use crate::test_rack::FM;
        use crate::ui::lane_geom::FoldRows;
        let mut app = folded_key_app(0, 50, &[]);
        let height = acid_engine::project::LaneView::DEFAULT_HEIGHT as f32;
        let rh = crate::ui::lane::ROW_H;
        let centre = |rows: &FoldRows, p: u8| {
            let row = rows.row_of(p).unwrap();
            row as f32 * rh + rh / 2.0 - height / 2.0
        };
        let c = FoldRows::new(app.doc.project.key, &Default::default());
        let start = centre(&c, 50);
        app.view.panel(FM).lane_scroll_y = Some(start);
        let ctx = egui::Context::default();
        click_tool(&mut app, &ctx, "Key root", 0.0);
        click_tool(&mut app, &ctx, "Root A", 1.0);
        let d = FoldRows::new(app.doc.project.key, &Default::default());
        let want = centre(&d, 50);
        assert_ne!(want, start, "the row of D3 moved");
        assert_eq!(app.view.panel(FM).lane_scroll_y, Some(want));
        app.undo();
        assert_eq!(app.view.panel(FM).lane_scroll_y, Some(start), "and back");
    }

    #[test]
    fn shift_arrows_do_not_transpose_drums() {
        use crate::test_rack::KIT808;
        use egui::{Key, Modifiers};
        let mut app = sel_app(KIT808, &[(0, 4, 2)], &[0]);
        assert!(app.dock_key(Key::ArrowUp, Modifiers::SHIFT));
        assert_eq!(notes_of(&app, KIT808), [(0, 4, 2)]);
        assert!(app.dock_key(Key::ArrowUp, Modifiers::NONE));
        assert_eq!(notes_of(&app, KIT808), [(0, 4, 3)], "one row up");
    }

    #[test]
    fn delete_removes_the_selection_in_one_undo() {
        use crate::test_rack::FM;
        use egui::{Key, Modifiers};
        let mut app = sel_app(FM, &[(0, 4, 60), (16, 4, 62), (32, 4, 64)], &[0, 2]);
        assert!(app.dock_key(Key::Delete, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM), [(16, 4, 62)]);
        assert!(app.selection(FM).is_empty());
        app.undo();
        assert_eq!(notes_of(&app, FM).len(), 3);
        app.set_selection(FM, vec![app.doc.project.roll(FM, 0).unwrap().notes[1]]);
        assert!(app.dock_key(Key::Backspace, Modifiers::NONE));
        assert_eq!(notes_of(&app, FM), [(0, 4, 60), (32, 4, 64)]);
    }

    #[test]
    fn holding_delete_after_deleting_the_selection_erases_nothing_more() {
        use crate::test_rack::FM;
        use egui::Key;
        let mut app = sel_app(FM, &[(0, 4, 60), (4, 4, 60), (16, 4, 62)], &[2]);
        app.view.panel(FM).cursor = crate::ui::key_entry::Cursor { tick: 8, pitch: 60 };
        // One context throughout: egui tracks which keys are down.
        let ctx = egui::Context::default();
        let run = |app: &mut SidAcidApp, events| {
            let mut left = Vec::new();
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                app.handle_shortcuts(ctx);
                left = ctx.input(|i| i.events.clone());
            });
            left
        };
        let delete = |pressed, repeat| key_event(Key::Delete, pressed, repeat, false);
        let left = run(&mut app, vec![delete(true, false), delete(true, true)]);
        assert_eq!(
            notes_of(&app, FM),
            [(0, 4, 60), (4, 4, 60)],
            "only the selection"
        );
        assert!(left.is_empty(), "the repeat is used up too: {left:?}");
        run(&mut app, vec![delete(true, true)]);
        assert_eq!(notes_of(&app, FM).len(), 2, "still held: nothing more");
        // Let go and press again: keyboard entry erases as usual.
        run(&mut app, vec![delete(false, false), delete(true, false)]);
        assert_eq!(notes_of(&app, FM), [(0, 4, 60)]);
    }

    #[test]
    fn esc_clears_the_selection_and_the_range() {
        use crate::test_rack::{FM, SYNTH1};
        use egui::{Key, Modifiers};
        let mut app = sel_app(FM, &[(0, 4, 60)], &[0]);
        assert!(app.dock_key(Key::Escape, Modifiers::NONE));
        assert!(app.selection(FM).is_empty());
        assert!(
            !app.dock_key(Key::Escape, Modifiers::NONE),
            "nothing to clear"
        );
        // A range: every lane's selection goes with it.
        app.view.dock.range = Some((0, 8));
        app.select_range(0, 8);
        assert_eq!(app.selection(FM), vec![0]);
        assert!(app.dock_key(Key::Escape, Modifiers::NONE));
        assert_eq!(app.view.dock.range, None);
        assert!(app.selection(FM).is_empty() && app.selection(SYNTH1).is_empty());
        assert!(!app.doc.is_dirty());
    }

    /// `range_app` with a range selected over (12, 32), focus on FM.
    fn ranged_app() -> SidAcidApp {
        let mut app = range_app();
        app.view.focus = Some(crate::test_rack::FM);
        app.view.dock.keyboard = true;
        app.view.dock.range = Some((12, 32));
        app.select_range(12, 32);
        app
    }

    #[test]
    fn a_keyboard_move_drops_the_range_and_the_other_lanes_selections() {
        use crate::test_rack::{FM, SYNTH1};
        use egui::{Key, Modifiers};
        let mut app = ranged_app();
        assert!(!app.selection(SYNTH1).is_empty());
        assert!(app.dock_key(Key::ArrowRight, Modifiers::NONE));
        assert_eq!(app.view.dock.range, None);
        assert!(app.selection(SYNTH1).is_empty(), "the other lane let go");
        assert!(!app.selection(FM).is_empty(), "the focused lane keeps it");
    }

    #[test]
    fn a_range_cut_clears_the_range_and_every_selection() {
        use crate::test_rack::{FM, SYNTH1};
        let mut app = ranged_app();
        app.copy_notes(true);
        assert_eq!(app.view.dock.range, None);
        assert!(app.selection(FM).is_empty() && app.selection(SYNTH1).is_empty());
    }

    #[test]
    fn a_no_op_move_keeps_the_range_and_commits_nothing() {
        use crate::test_rack::KIT808;
        use egui::{Key, Modifiers};
        let mut app = sel_app(KIT808, &[(0, 4, 2)], &[0]);
        app.view.dock.range = Some((0, 8));
        assert!(app.dock_key(Key::ArrowUp, Modifiers::SHIFT));
        assert_eq!(app.view.dock.range, Some((0, 8)));
        assert!(!app.doc.can_undo(), "nothing committed");
    }

    #[test]
    fn without_a_selection_keys_still_enter_notes() {
        use crate::test_rack::FM;
        use egui::{Key, Modifiers};
        let mut app = sel_app(FM, &[(0, 4, 60)], &[]);
        assert!(app.dock_key(Key::ArrowRight, Modifiers::NONE));
        assert_eq!(app.view.panel(FM).cursor.tick, 4, "the cursor moved");
        assert_eq!(notes_of(&app, FM), [(0, 4, 60)]);
        assert!(
            !app.dock_key(Key::ArrowRight, Modifiers::ALT),
            "Alt is not entry"
        );
    }

    #[test]
    fn a_frame_arrow_press_nudges_the_selection_and_is_stripped() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60)], &[0]);
        let left = shortcut_frame(
            &mut app,
            vec![
                key_event(egui::Key::ArrowRight, true, false, false),
                key_event(egui::Key::ArrowRight, false, false, false),
            ],
        );
        assert_eq!(notes_of(&app, FM), [(4, 4, 60)]);
        assert_eq!(left.len(), 1, "the press is gone: {left:?}");
    }

    #[test]
    fn frame_ctrl_c_then_ctrl_v_pastes_at_the_cursor() {
        use crate::test_rack::FM;
        use egui::Key;
        let mut app = sel_app(FM, &[(4, 4, 60), (8, 2, 62), (40, 4, 64)], &[0, 1]);
        shortcut_frame(&mut app, vec![key_event(Key::C, true, false, true)]);
        assert!(app.view.clipboard.is_none(), "not the pattern clipboard");
        app.view.panel(FM).cursor.tick = 20;
        // A backend may send the key and a paste event for one press.
        shortcut_frame(
            &mut app,
            vec![
                key_event(Key::V, true, false, true),
                egui::Event::Paste(String::new()),
            ],
        );
        let notes = notes_of(&app, FM);
        assert_eq!(notes.len(), 5, "pasted once: {notes:?}");
        assert!(notes.contains(&(20, 4, 60)) && notes.contains(&(24, 2, 62)));
        let sel: Vec<_> = app.selection(FM).iter().map(|&i| notes[i]).collect();
        assert_eq!(sel, [(20, 4, 60), (24, 2, 62)], "the pasted notes selected");
        app.undo();
        assert_eq!(notes_of(&app, FM).len(), 3, "one undo");
    }

    #[test]
    fn copy_without_a_selection_takes_the_whole_lane() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(4, 4, 60), (8, 2, 62)], &[]);
        app.copy_notes(false);
        let clip = app.view.note_clip.as_ref().unwrap();
        assert_eq!(clip.lanes.len(), 1);
        assert_eq!(
            clip.lanes[0].2.iter().map(|n| n.start).collect::<Vec<_>>(),
            [0, 4],
            "relative to the earliest start"
        );
    }

    #[test]
    fn notes_hidden_past_the_pattern_end_are_never_selected_or_cut() {
        use crate::test_rack::{FM, SYNTH1};
        use egui::Key;
        let mut app = sel_app(FM, &[(0, 4, 60), (100, 4, 62)], &[]);
        app.doc.project.roll_mut(FM, 0).unwrap().length = 16;
        shortcut_frame(&mut app, vec![key_event(Key::A, true, false, true)]);
        assert_eq!(app.selection(FM), vec![0], "Ctrl+A: the visible note");
        shortcut_frame(&mut app, vec![key_event(Key::Delete, true, false, false)]);
        assert_eq!(notes_of(&app, FM), [(100, 4, 62)], "the hidden note stays");
        // The whole-lane copy leaves it out too.
        app.doc
            .project
            .roll_mut(FM, 0)
            .unwrap()
            .notes
            .insert(0, acid_engine::project::Note::new(8, 4, 60, 100));
        app.copy_notes(false);
        let clip = app.view.note_clip.as_ref().unwrap();
        assert_eq!(clip.lanes[0].2.len(), 1, "{:?}", clip.lanes);
        // A range cut over it leaves it be.
        let mut app = range_app();
        let p = app.doc.project.roll_mut(FM, 0).unwrap();
        p.length = 16;
        p.notes
            .push(acid_engine::project::Note::new(100, 4, 66, 100));
        app.view.focus = Some(FM);
        app.view.dock.range = Some((0, 200));
        app.copy_notes(true);
        assert_eq!(notes_of(&app, FM), [(100, 4, 66)]);
        assert!(notes_of(&app, SYNTH1).is_empty());
    }

    #[test]
    fn cut_with_nothing_selected_cuts_nothing() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(4, 4, 60), (8, 2, 62)], &[]);
        shortcut_frame(&mut app, vec![key_event(egui::Key::X, true, false, true)]);
        assert_eq!(notes_of(&app, FM), [(4, 4, 60), (8, 2, 62)]);
        assert!(app.view.note_clip.is_none(), "nothing copied either");
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn cut_is_one_undo() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(4, 4, 60), (8, 2, 62), (40, 4, 64)], &[0, 1]);
        let before = app.doc.project.clone();
        app.copy_notes(true);
        assert_eq!(notes_of(&app, FM), [(40, 4, 64)]);
        assert!(app.view.note_clip.is_some());
        app.undo();
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn melodic_and_drum_notes_do_not_paste_onto_each_other() {
        use crate::test_rack::{FM, KIT808, SYNTH1};
        let mut app = sel_app(FM, &[(4, 4, 60)], &[0]);
        app.copy_notes(false);
        app.view.focus = Some(KIT808);
        let before = app.doc.project.clone();
        app.paste_notes();
        assert_eq!(
            app.view.message.as_deref(),
            Some("Melodic notes can't be pasted onto drums")
        );
        assert_eq!(app.doc.project, before);
        // Poly onto acid is melodic onto melodic.
        app.view.message = None;
        app.view.focus = Some(SYNTH1);
        app.paste_notes();
        assert!(app.view.message.is_none());
        assert_eq!(notes_of(&app, SYNTH1).len(), 1);
        // Drums onto a melodic part.
        app.doc.project.roll_mut(KIT808, 0).unwrap().notes =
            vec![acid_engine::project::Note::new(0, 4, 1, 100)];
        app.view.focus = Some(KIT808);
        app.select_all_notes();
        app.copy_notes(false);
        app.view.focus = Some(FM);
        let before = app.doc.project.clone();
        app.paste_notes();
        assert_eq!(
            app.view.message.as_deref(),
            Some("Drum notes can't be pasted onto a melodic part")
        );
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn a_range_copy_pastes_into_the_same_machines_in_one_undo() {
        use crate::test_rack::{FM, KIT808, SYNTH1};
        let mut app = range_app();
        app.doc.project.roll_mut(KIT808, 0).unwrap().notes =
            vec![acid_engine::project::Note::new(20, 1, 3, 100)];
        app.view.dock.keyboard = true;
        app.view.dock.range = Some((12, 32));
        app.select_range(12, 32);
        app.copy_notes(false);
        assert!(app.view.note_clip.as_ref().unwrap().lanes.len() > 1);
        app.view.focus = Some(FM);
        app.view.panel(FM).cursor.tick = 48;
        let before = app.doc.project.clone();
        app.paste_notes();
        for uid in [FM, SYNTH1] {
            assert!(notes_of(&app, uid).contains(&(52, 4, 62)), "{uid:?}");
            assert_eq!(notes_of(&app, uid).len(), 4, "{uid:?}");
        }
        assert!(notes_of(&app, KIT808).contains(&(56, 1, 3)), "the kit too");
        assert_eq!(app.view.dock.range, None);
        app.undo();
        assert_eq!(app.doc.project, before, "one undo for every lane");
    }

    #[test]
    fn a_range_cut_is_one_undo_across_lanes() {
        use crate::test_rack::{FM, SYNTH1};
        let mut app = range_app();
        app.view.dock.range = Some((12, 32));
        let before = app.doc.project.clone();
        app.copy_notes(true);
        for uid in [FM, SYNTH1] {
            assert_eq!(notes_of(&app, uid), [(0, 4, 60), (40, 4, 64)]);
        }
        app.undo();
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn a_paste_past_the_note_cap_is_refused() {
        use crate::test_rack::FM;
        use acid_engine::project::{MAX_NOTES, Note};
        let mut app = sel_app(FM, &[(4, 4, 60), (8, 2, 62)], &[0, 1]);
        app.copy_notes(false);
        app.doc.project.roll_mut(FM, 0).unwrap().notes = (0..MAX_NOTES - 1)
            .map(|i| Note::new((i % 200) as u16, 1, 20 + (i / 200) as u8, 100))
            .collect();
        let before = app.doc.project.clone();
        app.paste_notes();
        let cap = "That would put more than 512 notes in the pattern";
        assert_eq!(app.view.message.as_deref(), Some(cap));
        assert_eq!(app.doc.project, before);
        // Duplicate says the same.
        app.view.message = None;
        let notes = app.doc.project.roll(FM, 0).unwrap().notes[..2].to_vec();
        app.set_selection(FM, notes);
        app.duplicate_notes();
        assert_eq!(app.view.message.as_deref(), Some(cap), "duplicate");
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn frame_ctrl_d_duplicates_after_the_selection() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60), (4, 2, 62)], &[0, 1]);
        shortcut_frame(&mut app, vec![key_event(egui::Key::D, true, false, true)]);
        let notes = notes_of(&app, FM);
        assert_eq!(notes.len(), 4, "{notes:?}");
        assert!(notes.contains(&(8, 4, 60)) && notes.contains(&(12, 2, 62)));
        let sel: Vec<_> = app.selection(FM).iter().map(|&i| notes[i]).collect();
        assert_eq!(sel, [(8, 4, 60), (12, 2, 62)], "the copies selected");
        app.undo();
        assert_eq!(notes_of(&app, FM).len(), 2, "one undo");
    }

    #[test]
    fn a_range_duplicate_repeats_every_lane_after_the_range() {
        use crate::test_rack::{FM, SYNTH1};
        let mut app = range_app();
        app.view.dock.range = Some((12, 32));
        app.duplicate_notes();
        for uid in [FM, SYNTH1] {
            assert!(notes_of(&app, uid).contains(&(36, 4, 62)), "{uid:?}");
        }
        assert_eq!(app.view.dock.range, Some((32, 52)), "the range moves on");
        app.undo();
        assert_eq!(notes_of(&app, FM).len(), 3);
        assert_eq!(notes_of(&app, SYNTH1).len(), 3, "one undo");
    }

    #[test]
    fn frame_ctrl_a_selects_the_whole_lane_and_clears_the_range() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60), (16, 4, 62)], &[]);
        app.view.dock.range = Some((40, 48));
        shortcut_frame(&mut app, vec![key_event(egui::Key::A, true, false, true)]);
        assert_eq!(app.selection(FM), vec![0, 1]);
        assert_eq!(app.view.dock.range, None);
    }

    #[test]
    fn without_dock_focus_ctrl_c_copies_the_whole_pattern() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60)], &[0]);
        app.view.dock.keyboard = false;
        shortcut_frame(
            &mut app,
            vec![
                key_event(egui::Key::C, true, false, true),
                key_event(egui::Key::X, true, false, true),
                key_event(egui::Key::D, true, false, true),
            ],
        );
        assert!(app.view.clipboard.is_some(), "the pattern was copied");
        assert!(app.view.note_clip.is_none());
        assert_eq!(notes_of(&app, FM), [(0, 4, 60)], "Ctrl+X/D do nothing");
    }

    #[test]
    fn a_pattern_pad_or_a_load_clears_the_range() {
        use crate::test_rack::FM;
        let mut app = range_app();
        app.view.dock.range = Some((0, 8));
        app.select_pad(FM, 1);
        assert_eq!(app.view.dock.range, None, "pad");
        app.view.dock.range = Some((0, 8));
        app.select_range(0, 8);
        app.load_document(Document::new(Project::default()));
        assert_eq!(app.view.dock.range, None, "load");
        assert!(app.selection(FM).is_empty());
    }

    // Song mode's header tools, block keys and block clipboard.

    /// `song_app`'s SYNTH1 lane as (start, pattern, len).
    fn synth1(app: &SidAcidApp) -> Vec<(u32, u8, Option<u16>)> {
        song_lane_of(app, crate::test_rack::SYNTH1)
    }

    fn song_lane_of(
        app: &SidAcidApp,
        uid: acid_engine::machine::MachineUid,
    ) -> Vec<(u32, u8, Option<u16>)> {
        app.doc
            .project
            .song
            .lane(uid)
            .iter()
            .map(|b| (b.start, b.pattern.0, b.len))
            .collect()
    }

    /// Starts of `uid`'s selected blocks.
    fn picked(app: &SidAcidApp, uid: acid_engine::machine::MachineUid) -> Vec<u32> {
        let lane = app.doc.project.song.lane(uid);
        app.block_selection(uid)
            .iter()
            .map(|&i| lane[i].start)
            .collect()
    }

    /// Selects `uid`'s blocks starting at `starts`.
    fn pick(app: &mut SidAcidApp, uid: acid_engine::machine::MachineUid, starts: &[u32]) {
        let lane = app.doc.project.song.lane(uid).to_vec();
        app.view.panel(uid).song_sel = lane
            .into_iter()
            .filter(|b| starts.contains(&b.start))
            .collect();
    }

    /// Where header tool `name` was drawn last frame.
    fn tool(app: &SidAcidApp, name: &str) -> Option<Rect> {
        app.view
            .dock
            .tool_rects
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, r)| *r)
    }

    /// Draws a frame, then clicks header tool `name` (which must be there).
    fn click_tool(app: &mut SidAcidApp, ctx: &egui::Context, name: &str, time: f64) {
        frame(app, ctx, time, vec![]);
        let r = tool(app, name).unwrap_or_else(|| panic!("no {name} tool"));
        press(app, ctx, r.center(), time + 0.1);
    }

    /// One shortcut frame of a key press with `mods`.
    fn key_with(app: &mut SidAcidApp, key: egui::Key, mods: egui::Modifiers) {
        shortcut_frame(
            app,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: mods,
            }],
        );
    }

    /// `song_app` with the dock focused on SYNTH1.
    fn song_keys_app() -> SidAcidApp {
        let mut app = song_app();
        app.view.dock.keyboard = true;
        app.view.focus = Some(crate::test_rack::SYNTH1);
        app
    }

    #[test]
    fn capture_places_every_machines_selected_pattern_at_the_cursor() {
        use crate::test_rack::*;
        let mut app = song_app();
        app.doc.project.machine_mut(SYNTH2).unwrap().selected = 3;
        app.view.song_cursor = 64;
        let before = app.doc.project.song.clone();
        let ctx = egui::Context::default();
        click_tool(&mut app, &ctx, "Capture", 0.0);
        for m in &app.doc.project.machines {
            let lane = app.doc.project.song.lane(m.uid);
            assert!(
                lane.iter()
                    .any(|b| b.start == 64 && b.pattern.0 == m.selected),
                "{:?} captured",
                m.uid
            );
            assert_eq!(picked(&app, m.uid), [64], "the new blocks are selected");
        }
        assert_eq!(synth1(&app).len(), 4);
        app.undo();
        assert_eq!(app.doc.project.song, before, "one undo step");
    }

    #[test]
    fn capture_lands_on_the_nearest_free_start_or_is_refused_whole() {
        use crate::test_rack::*;
        let mut app = song_app();
        // SYNTH1 is taken at 16..32; the next free start is 32.
        app.view.song_cursor = 20;
        app.capture();
        assert!(synth1(&app).contains(&(32, 0, None)));
        assert_eq!(song_lane_of(&app, FM), [(20, 0, None)]);
        // A full FM lane: nothing is placed anywhere.
        let max = acid_engine::song::MAX_SONG_STEPS as u32;
        let full: Vec<_> = (0..max / 16)
            .map(|i| acid_engine::project::Block::new(i * 16, acid_engine::project::PatternRef(0)))
            .collect();
        *app.doc.project.song.lane_mut(FM) = full;
        let before = app.doc.project.song.clone();
        app.view.song_cursor = 96;
        app.capture();
        assert_eq!(app.doc.project.song, before);
        assert!(app.view.message.is_some(), "the refusal is explained");
    }

    #[test]
    fn range_tools_are_disabled_without_a_range() {
        let mut app = song_app();
        let ctx = egui::Context::default();
        click_tool(&mut app, &ctx, "Loop", 0.0);
        click_tool(&mut app, &ctx, "Insert time", 1.0);
        click_tool(&mut app, &ctx, "Delete time", 2.0);
        assert_eq!(app.doc.project.song.loop_range, None);
        assert!(!app.doc.can_undo(), "nothing happened");
        assert!(tool(&app, "Clear loop").is_none(), "no loop to clear");
    }

    #[test]
    fn loop_and_clear_loop_from_the_header() {
        let mut app = song_app();
        let ctx = egui::Context::default();
        app.view.dock.range = Some((16, 48));
        click_tool(&mut app, &ctx, "Loop", 0.0);
        assert_eq!(app.doc.project.song.loop_range, Some((16, 48)));
        click_tool(&mut app, &ctx, "Clear loop", 1.0);
        assert_eq!(app.doc.project.song.loop_range, None);
        app.undo();
        assert_eq!(app.doc.project.song.loop_range, Some((16, 48)));
        app.undo();
        assert_eq!(app.doc.project.song.loop_range, None, "one step each");
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn insert_time_pushes_everything_from_the_range_start_right() {
        let mut app = song_app();
        let ctx = egui::Context::default();
        app.view.dock.range = Some((16, 32));
        app.select_song_range(16, 32);
        let before = app.doc.project.song.clone();
        click_tool(&mut app, &ctx, "Insert time", 0.0);
        assert_eq!(synth1(&app), [(0, 0, None), (32, 1, None), (64, 2, None)]);
        assert_eq!(app.view.dock.range, Some((16, 32)), "the range stays");
        assert!(picked(&app, crate::test_rack::SYNTH1).is_empty());
        app.undo();
        assert_eq!(app.doc.project.song, before, "one undo step");
    }

    #[test]
    fn delete_time_removes_the_range_and_closes_the_gap() {
        let mut app = song_app();
        let ctx = egui::Context::default();
        app.view.dock.range = Some((16, 32));
        let before = app.doc.project.song.clone();
        click_tool(&mut app, &ctx, "Delete time", 0.0);
        assert_eq!(synth1(&app), [(0, 0, None), (32, 2, None)]);
        assert_eq!(app.view.dock.range, None);
        app.undo();
        assert_eq!(app.doc.project.song, before, "one undo step");
    }

    #[test]
    fn block_tools_show_only_for_exactly_one_selected_block() {
        use crate::test_rack::*;
        let mut app = song_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        assert!(tool(&app, "Mute").is_none(), "nothing selected");
        pick(&mut app, SYNTH1, &[0, 16]);
        frame(&mut app, &ctx, 0.5, vec![]);
        assert!(tool(&app, "Mute").is_none(), "two selected");
        pick(&mut app, SYNTH1, &[16]);
        frame(&mut app, &ctx, 1.0, vec![]);
        assert!(tool(&app, "Mute").is_some());
        assert!(tool(&app, "Trans +").is_some(), "the synth transposes");
        // A drum machine's block: no transpose.
        app.doc
            .project
            .song
            .lane_mut(KIT808)
            .push(acid_engine::project::Block::new(
                0,
                acid_engine::project::PatternRef(0),
            ));
        app.view.panel(SYNTH1).song_sel.clear();
        pick(&mut app, KIT808, &[0]);
        frame(&mut app, &ctx, 1.5, vec![]);
        assert!(tool(&app, "Mute").is_some());
        assert!(tool(&app, "Trans +").is_none(), "drums don't transpose");
    }

    #[test]
    fn mute_and_transpose_edit_the_selected_block() {
        use crate::test_rack::SYNTH1;
        let mut app = song_app();
        let ctx = egui::Context::default();
        pick(&mut app, SYNTH1, &[16]);
        click_tool(&mut app, &ctx, "Mute", 0.0);
        click_tool(&mut app, &ctx, "Trans +", 1.0);
        click_tool(&mut app, &ctx, "Trans +", 2.0);
        click_tool(&mut app, &ctx, "Trans -", 3.0);
        let b = app.doc.project.song.lane(SYNTH1)[1];
        assert!(b.mute);
        assert_eq!(b.transpose, 1);
        assert_eq!(picked(&app, SYNTH1), [16], "still selected");
        app.undo();
        assert_eq!(app.doc.project.song.lane(SYNTH1)[1].transpose, 2);
        app.undo();
        app.undo();
        let b = app.doc.project.song.lane(SYNTH1)[1];
        assert_eq!((b.mute, b.transpose), (true, 0), "one step per click");
        app.view.panel(SYNTH1).song_sel = vec![b];
        for _ in 0..20 {
            app.transpose_block(1);
        }
        assert_eq!(app.doc.project.song.lane(SYNTH1)[1].transpose, 12, "held");
    }

    #[test]
    fn the_pattern_pickers_switch_the_blocks_pattern_and_cut_it_to_fit() {
        use crate::test_rack::SYNTH1;
        let mut app = song_app();
        // Pattern 5 lasts 32 steps; the block at 0 has a neighbour at 16.
        app.doc.project.roll_mut(SYNTH1, 5).unwrap().length = 32;
        let ctx = egui::Context::default();
        pick(&mut app, SYNTH1, &[0]);
        click_tool(&mut app, &ctx, "number", 0.0);
        frame(&mut app, &ctx, 0.5, vec![]);
        let item = tool(&app, "number 6").expect("the number list is open");
        press(&mut app, &ctx, item.center(), 0.6);
        assert_eq!(
            synth1(&app),
            [(0, 5, Some(16)), (16, 1, None), (48, 2, None)],
            "same start, cut to end at the next block"
        );
        click_tool(&mut app, &ctx, "bank", 2.0);
        frame(&mut app, &ctx, 2.5, vec![]);
        let item = tool(&app, "bank C").expect("the bank list is open");
        press(&mut app, &ctx, item.center(), 2.6);
        assert_eq!(synth1(&app)[0], (0, 37, Some(16)), "C06, number kept");
        // The block at 16 has room up to 48: a 32-step pattern fits whole.
        app.view.panel(SYNTH1).song_sel.clear();
        pick(&mut app, SYNTH1, &[16]);
        app.set_block_pattern(5);
        assert_eq!(synth1(&app)[1], (16, 5, None));
        app.undo();
        app.undo();
        app.undo();
        assert_eq!(synth1(&app)[0], (0, 0, None), "one undo step each");
    }

    #[test]
    fn a_pattern_cut_to_fit_says_so_and_one_that_fits_does_not() {
        use crate::test_rack::SYNTH1;
        let mut app = song_app();
        app.doc.project.roll_mut(SYNTH1, 5).unwrap().length = 32;
        pick(&mut app, SYNTH1, &[16]);
        app.set_block_pattern(5);
        assert_eq!(synth1(&app)[1], (16, 5, None), "room for it whole");
        assert_eq!(app.view.status_line, None);
        pick(&mut app, SYNTH1, &[0]);
        app.set_block_pattern(5);
        assert_eq!(synth1(&app)[0], (0, 5, Some(16)));
        assert_eq!(
            app.view.status_line.as_deref(),
            Some("The block was shortened to fit before the next one")
        );
    }

    #[test]
    fn insert_time_near_the_limit_says_only_part_fit() {
        use crate::test_rack::SYNTH1;
        use acid_engine::project::{Block, PatternRef};
        let max = acid_engine::song::MAX_SONG_STEPS as u32;
        let mut app = song_app();
        let ctx = egui::Context::default();
        *app.doc.project.song.lane_mut(SYNTH1) = vec![Block::new(max - 32, PatternRef(0))];
        app.view.dock.range = Some((0, 100));
        click_tool(&mut app, &ctx, "Insert time", 0.0);
        assert_eq!(synth1(&app), [(max - 16, 0, None)], "only the room left");
        assert!(app.view.status_line.as_deref().unwrap().contains("part"));
        // A whole insert is silent.
        let mut app = song_app();
        app.view.dock.range = Some((16, 32));
        click_tool(&mut app, &ctx, "Insert time", 10.0);
        assert_eq!(app.view.status_line, None);
    }

    #[test]
    fn a_range_duplicate_that_fits_only_earlier_keeps_the_range_on_the_song() {
        use crate::test_rack::SYNTH1;
        use acid_engine::project::{Block, PatternRef};
        use egui::{Key, Modifiers};
        let max = acid_engine::song::MAX_SONG_STEPS as u32;
        let mut app = song_keys_app();
        let mut a = Block::new(16, PatternRef(1));
        a.len = Some(32);
        let mut z = Block::new(64, PatternRef(2));
        z.len = Some((max - 64) as u16);
        *app.doc.project.song.lane_mut(SYNTH1) = vec![a, Block::new(48, PatternRef(0)), z];
        // The range holds only the block at 48 (1 bar after its start).
        app.view.dock.range = Some((32, 64));
        key_with(&mut app, Key::D, Modifiers::COMMAND);
        assert_eq!(synth1(&app)[0], (0, 0, None), "the copy fits only at 0");
        assert_eq!(app.view.dock.range, Some((0, 32)), "no wrap below zero");
    }

    #[test]
    fn arrows_move_the_block_selection_by_bars_or_alt_steps() {
        use crate::test_rack::SYNTH1;
        use egui::{Key, Modifiers};
        let mut app = song_keys_app();
        pick(&mut app, SYNTH1, &[48]);
        key_with(&mut app, Key::ArrowRight, Modifiers::NONE);
        assert_eq!(synth1(&app)[2].0, 64);
        key_with(&mut app, Key::ArrowRight, Modifiers::ALT);
        assert_eq!(synth1(&app)[2].0, 65);
        key_with(&mut app, Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(synth1(&app)[2].0, 49);
        assert_eq!(picked(&app, SYNTH1), [49], "the selection follows");
        app.undo();
        assert_eq!(synth1(&app)[2].0, 65, "one undo step per key");
        // Onto a taken bar: the nearest free one on the bar grid.
        pick(&mut app, SYNTH1, &[0]);
        key_with(&mut app, Key::ArrowRight, Modifiers::NONE);
        assert_eq!(synth1(&app)[1], (32, 0, None), "over the block at 16");
    }

    #[test]
    fn a_held_block_makes_no_undo_step() {
        use crate::test_rack::SYNTH1;
        let mut app = song_keys_app();
        pick(&mut app, SYNTH1, &[0]);
        key_with(&mut app, egui::Key::ArrowLeft, egui::Modifiers::NONE);
        assert_eq!(synth1(&app)[0].0, 0);
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn delete_and_esc_on_blocks() {
        use crate::test_rack::SYNTH1;
        use egui::{Key, Modifiers};
        let mut app = song_keys_app();
        pick(&mut app, SYNTH1, &[0, 48]);
        key_with(&mut app, Key::Delete, Modifiers::NONE);
        assert_eq!(synth1(&app), [(16, 1, None)]);
        assert!(picked(&app, SYNTH1).is_empty());
        app.undo();
        assert_eq!(synth1(&app).len(), 3, "one undo step");
        pick(&mut app, SYNTH1, &[16]);
        key_with(&mut app, Key::Backspace, Modifiers::NONE);
        assert_eq!(synth1(&app).len(), 2);
        // Esc: the range and every lane's selection go; no edit.
        app.view.dock.range = Some((0, 64));
        app.select_song_range(0, 64);
        let undo_before = app.doc.edit_serial();
        key_with(&mut app, Key::Escape, Modifiers::NONE);
        assert_eq!(app.view.dock.range, None);
        assert!(picked(&app, SYNTH1).is_empty());
        assert_eq!(app.doc.edit_serial(), undo_before);
    }

    #[test]
    fn ctrl_c_and_ctrl_v_paste_blocks_at_the_song_cursor() {
        use crate::test_rack::{FM, SYNTH1};
        use egui::{Key, Modifiers};
        let mut app = song_keys_app();
        pick(&mut app, SYNTH1, &[16]);
        key_with(&mut app, Key::C, Modifiers::COMMAND);
        assert!(app.view.note_clip.is_none(), "the note clipboard is apart");
        app.view.song_cursor = 96;
        key_with(&mut app, Key::V, Modifiers::COMMAND);
        assert_eq!(synth1(&app)[3], (96, 1, None));
        assert_eq!(picked(&app, SYNTH1), [96], "the paste is selected");
        // Onto taken steps: the nearest bar where it fits (32 is free).
        app.view.song_cursor = 16;
        key_with(&mut app, Key::V, Modifiers::COMMAND);
        assert!(synth1(&app).contains(&(32, 1, None)));
        // A one-lane clip pastes into the focused lane.
        app.view.focus = Some(FM);
        app.view.song_cursor = 0;
        key_with(&mut app, Key::V, Modifiers::COMMAND);
        assert_eq!(song_lane_of(&app, FM), [(0, 1, None)]);
        app.undo();
        assert!(song_lane_of(&app, FM).is_empty(), "one undo step");
    }

    /// The song still validates (it would save and load back).
    fn assert_song_valid(app: &SidAcidApp) {
        app.doc.project.validate().expect("the song validates");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "a song edit left lanes that don't validate")]
    fn a_song_edit_with_overlapping_blocks_is_caught_in_debug_builds() {
        let mut app = song_app();
        let mut s = app.doc.project.song.clone();
        s.lane_mut(crate::test_rack::SYNTH1)[1].start = 8;
        app.apply_song(s, false);
    }

    #[test]
    fn pasting_onto_a_machine_with_longer_patterns_cuts_the_clip_to_fit() {
        use crate::test_rack::{FM, SYNTH1};
        use egui::{Key, Modifiers};
        let mut app = song_keys_app();
        app.doc.project.roll_mut(FM, 0).unwrap().length = 32;
        pick(&mut app, SYNTH1, &[0, 16]);
        key_with(&mut app, Key::C, Modifiers::COMMAND);
        app.view.focus = Some(FM);
        app.view.song_cursor = 0;
        key_with(&mut app, Key::V, Modifiers::COMMAND);
        assert_eq!(song_lane_of(&app, FM), [(0, 0, Some(16)), (16, 1, None)]);
        assert_eq!(
            app.view.status_line.as_deref(),
            Some("A pasted block was shortened to fit before the next one")
        );
        assert_song_valid(&app);
    }

    #[test]
    fn pasting_after_a_pattern_grew_cuts_the_clip_to_fit() {
        use crate::test_rack::SYNTH1;
        use acid_engine::project::{Block, PatternRef};
        use egui::{Key, Modifiers};
        let mut app = song_keys_app();
        pick(&mut app, SYNTH1, &[0, 16]);
        key_with(&mut app, Key::C, Modifiers::COMMAND);
        // Pattern 0 grows to 24 steps; the song ripples, the clip doesn't.
        app.doc.project.roll_mut(SYNTH1, 0).unwrap().length = 24;
        *app.doc.project.song.lane_mut(SYNTH1) = vec![
            Block::new(0, PatternRef(0)),
            Block::new(24, PatternRef(1)),
            Block::new(56, PatternRef(2)),
        ];
        app.view.song_cursor = 128;
        key_with(&mut app, Key::V, Modifiers::COMMAND);
        assert_eq!(&synth1(&app)[3..], [(128, 0, Some(16)), (144, 1, None)]);
        assert_song_valid(&app);
    }

    #[test]
    fn ctrl_x_cuts_blocks_in_one_undo_step() {
        use crate::test_rack::SYNTH1;
        let mut app = song_keys_app();
        let before = app.doc.project.song.clone();
        pick(&mut app, SYNTH1, &[0, 16]);
        key_with(&mut app, egui::Key::X, egui::Modifiers::COMMAND);
        assert_eq!(synth1(&app), [(48, 2, None)]);
        let clip = &app.view.block_clip.as_ref().unwrap().lanes;
        assert_eq!(clip.len(), 1);
        assert_eq!(
            clip[0].1.iter().map(|b| b.start).collect::<Vec<_>>(),
            [0, 16]
        );
        app.undo();
        assert_eq!(app.doc.project.song, before);
        // Nothing selected: nothing is cut or copied.
        app.view.block_clip = None;
        key_with(&mut app, egui::Key::X, egui::Modifiers::COMMAND);
        assert!(app.view.block_clip.is_none());
        assert_eq!(app.doc.project.song, before);
        assert!(app.doc.can_redo(), "no edit was made");
    }

    #[test]
    fn ctrl_d_duplicates_and_ctrl_a_selects_the_lane() {
        use crate::test_rack::SYNTH1;
        use egui::{Key, Modifiers};
        let mut app = song_keys_app();
        key_with(&mut app, Key::A, Modifiers::COMMAND);
        assert_eq!(picked(&app, SYNTH1), [0, 16, 48]);
        assert!(!app.doc.can_undo(), "selecting is no edit");
        pick(&mut app, SYNTH1, &[48]);
        key_with(&mut app, Key::D, Modifiers::COMMAND);
        assert_eq!(synth1(&app)[3], (64, 2, None));
        assert_eq!(picked(&app, SYNTH1), [64]);
        app.undo();
        assert_eq!(synth1(&app).len(), 3, "one undo step");
    }

    /// `song_app` with SYNTH2 blocks too, and a range over bars 1–2.
    fn two_lane_app() -> SidAcidApp {
        use crate::test_rack::SYNTH2;
        use acid_engine::project::{Block, PatternRef};
        let mut app = song_keys_app();
        app.doc
            .project
            .song
            .lane_mut(SYNTH2)
            .extend([Block::new(0, PatternRef(4)), Block::new(64, PatternRef(5))]);
        app.view.dock.range = Some((0, 32));
        app.select_song_range(0, 32);
        app
    }

    #[test]
    fn a_range_clip_pastes_back_into_the_same_machines_in_one_undo_step() {
        use crate::test_rack::{FM, SYNTH1, SYNTH2};
        use egui::{Key, Modifiers};
        let mut app = two_lane_app();
        let before = app.doc.project.song.clone();
        key_with(&mut app, Key::C, Modifiers::COMMAND);
        app.view.focus = Some(FM);
        app.view.song_cursor = 96;
        key_with(&mut app, Key::V, Modifiers::COMMAND);
        assert!(synth1(&app).ends_with(&[(96, 0, None), (112, 1, None)]));
        assert!(song_lane_of(&app, SYNTH2).ends_with(&[(96, 4, None)]));
        assert!(song_lane_of(&app, FM).is_empty(), "not the focused lane");
        assert_eq!(app.view.dock.range, None);
        assert_eq!(picked(&app, SYNTH1), [96, 112]);
        app.undo();
        assert_eq!(app.doc.project.song, before, "one undo step");
        // Every lane goes by one offset: at 48 SYNTH1 is taken, at 64
        // SYNTH2 is, so both land at 80.
        app.view.song_cursor = 48;
        key_with(&mut app, Key::V, Modifiers::COMMAND);
        assert!(synth1(&app).ends_with(&[(80, 0, None), (96, 1, None)]));
        assert!(song_lane_of(&app, SYNTH2).ends_with(&[(64, 5, None), (80, 4, None)]));
    }

    #[test]
    fn a_range_cut_and_duplicate_act_on_every_lane() {
        use crate::test_rack::{SYNTH1, SYNTH2};
        use egui::{Key, Modifiers};
        let mut app = two_lane_app();
        let before = app.doc.project.song.clone();
        // Right after the range is free on SYNTH2 but not on SYNTH1 (its
        // block at 48), and 64 is taken on SYNTH2: both go to 80.
        key_with(&mut app, Key::D, Modifiers::COMMAND);
        assert!(synth1(&app).ends_with(&[(80, 0, None), (96, 1, None)]));
        assert!(song_lane_of(&app, SYNTH2).ends_with(&[(80, 4, None)]));
        assert_eq!(app.view.dock.range, Some((80, 112)), "onto the copies");
        assert_eq!(picked(&app, SYNTH1), [80, 96]);
        app.undo();
        assert_eq!(app.doc.project.song, before);
        app.view.dock.range = Some((0, 32));
        key_with(&mut app, Key::X, Modifiers::COMMAND);
        assert_eq!(synth1(&app), [(48, 2, None)]);
        assert_eq!(song_lane_of(&app, SYNTH2), [(64, 5, None)]);
        assert_eq!(app.view.dock.range, None);
        app.undo();
        assert_eq!(app.doc.project.song, before, "one undo step");
    }

    #[test]
    fn pasted_song_blocks_drop_lanes_of_machines_not_in_the_rack() {
        use crate::test_rack::SYNTH2;
        use egui::{Key, Modifiers};
        let mut app = two_lane_app();
        key_with(&mut app, Key::C, Modifiers::COMMAND);
        app.remove_machine(SYNTH2);
        app.view.song_cursor = 96;
        key_with(&mut app, Key::V, Modifiers::COMMAND);
        assert!(synth1(&app).contains(&(96, 0, None)));
        assert!(
            app.doc.project.song.lanes.iter().all(|(u, _)| *u != SYNTH2),
            "no lane for the removed machine"
        );
        assert!(app.doc.project.validate().is_ok());
    }

    #[test]
    fn a_paste_that_fits_nowhere_is_refused() {
        use crate::test_rack::SYNTH1;
        use acid_engine::project::{Block, PatternRef};
        let mut app = song_keys_app();
        let max = acid_engine::song::MAX_SONG_STEPS as u32;
        *app.doc.project.song.lane_mut(SYNTH1) = (0..max / 16)
            .map(|i| Block::new(i * 16, PatternRef(0)))
            .collect();
        app.view.block_clip = Some(crate::ui::song_tools::BlockClip {
            lanes: vec![(SYNTH1, vec![Block::new(0, PatternRef(1))])],
        });
        let before = app.doc.project.song.clone();
        app.paste_blocks();
        assert_eq!(app.doc.project.song, before);
        assert!(app.view.message.is_some());
    }

    /// Every text painted by one 1280x800 frame of the whole app.
    fn texts(app: &mut SidAcidApp, ctx: &egui::Context, time: f64) -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
            time: Some(time),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.ui(ctx));
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    fn chances(app: &SidAcidApp, uid: acid_engine::machine::MachineUid) -> Vec<u8> {
        let roll = app.doc.project.roll(uid, 0).unwrap();
        roll.notes.iter().map(|n| n.chance).collect()
    }

    #[test]
    fn the_inspector_shows_only_with_a_selection_in_pattern_mode() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60)], &[]);
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        assert!(tool(&app, "Chance").is_none(), "no selection, no inspector");
        let all = app.doc.project.roll(FM, 0).unwrap().notes.clone();
        app.set_selection(FM, all);
        frame(&mut app, &ctx, 1.0, vec![]);
        for name in ["Vel", "Chance", "Reroll", "Ratchet", "Cond"] {
            assert!(tool(&app, name).is_some(), "{name}");
        }
        assert!(tool(&app, "Accent").is_none(), "no accent on a poly lane");
        app.view.mode = Mode::Song;
        frame(&mut app, &ctx, 2.0, vec![]);
        assert!(tool(&app, "Chance").is_none(), "not in Song mode");
    }

    #[test]
    fn a_chance_drag_edits_every_selected_note_in_one_undo_step() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60), (8, 4, 62), (16, 4, 64)], &[0, 2]);
        let before = app.doc.project.clone();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let at = tool(&app, "Chance").unwrap().center();
        drag_by(&mut app, &ctx, at, vec2(-100.0, 0.0), 10, 1.0);
        let c = chances(&app, FM);
        assert!(c[0] < 100 && c[0] == c[2], "both selected notes: {c:?}");
        assert_eq!(c[1], 100, "the unselected note stays");
        assert_eq!(app.selection(FM), vec![0, 2], "still selected");
        assert_eq!(app.doc.undo_depth(), 1, "one undo step for the drag");
        app.undo();
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn mixed_values_show_a_dash() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60), (8, 4, 62)], &[0, 1]);
        let ctx = egui::Context::default();
        let t = texts(&mut app, &ctx, 0.0);
        assert!(!t.iter().any(|t| t == "–"), "shared values: {t:?}");
        assert!(t.iter().any(|t| t == "100%"), "the shared chance");
        assert!(t.iter().any(|t| t == "Always"), "the shared condition");
        let p = app.doc.project.roll_mut(FM, 0).unwrap();
        p.notes[1].chance = 50;
        p.notes[1].cond = acid_engine::project::Cond::Fill;
        let all = p.notes.clone();
        app.set_selection(FM, all);
        let t = texts(&mut app, &ctx, 1.0);
        let dashes = t.iter().filter(|t| *t == "–").count();
        assert_eq!(dashes, 2, "chance and condition differ: {t:?}");
        assert!(t.iter().any(|t| t == "×1"), "the ratchet is shared");
    }

    #[test]
    fn picking_a_condition_sets_it_on_the_selection() {
        use crate::test_rack::FM;
        use acid_engine::project::Cond;
        let mut app = sel_app(FM, &[(0, 4, 60), (8, 4, 62), (16, 4, 64)], &[1, 2]);
        let ctx = egui::Context::default();
        click_tool(&mut app, &ctx, "Cond", 0.0);
        click_tool(&mut app, &ctx, "Cond 1:4", 1.0);
        let roll = app.doc.project.roll(FM, 0).unwrap();
        let conds: Vec<_> = roll.notes.iter().map(|n| n.cond).collect();
        assert_eq!(conds, [Cond::Always, Cond::Ratio(1, 4), Cond::Ratio(1, 4)]);
        assert_eq!(app.selection(FM), vec![1, 2], "still selected");
        assert_eq!(app.doc.undo_depth(), 1);
    }

    #[test]
    fn drum_and_acid_inspectors_toggle_accent_and_slide() {
        use crate::test_rack::{KIT808, SYNTH1};
        let mut app = sel_app(KIT808, &[(0, 4, 0), (4, 4, 1)], &[0, 1]);
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        assert!(tool(&app, "Vel").is_some(), "drums keep velocity");
        assert!(tool(&app, "Slide").is_none(), "no slide on drums");
        click_tool(&mut app, &ctx, "Accent", 1.0);
        let roll = app.doc.project.roll(KIT808, 0).unwrap();
        assert!(roll.notes.iter().all(|n| n.accent), "every selected hit");
        assert_eq!(app.selection(KIT808), vec![0, 1]);
        assert_eq!(app.doc.undo_depth(), 1);

        let mut app = sel_app(SYNTH1, &[(0, 4, 36), (4, 4, 38)], &[0]);
        app.doc.project.roll_mut(SYNTH1, 0).unwrap().notes[1].accent = true;
        frame(&mut app, &ctx, 2.0, vec![]);
        assert!(tool(&app, "Vel").is_none(), "acid lanes have no velocity");
        click_tool(&mut app, &ctx, "Slide", 3.0);
        click_tool(&mut app, &ctx, "Accent", 4.0);
        let roll = app.doc.project.roll(SYNTH1, 0).unwrap();
        assert!(roll.notes[0].slide && roll.notes[0].accent);
        assert!(!roll.notes[1].slide, "the unselected note stays");
        assert_eq!(app.doc.undo_depth(), 2, "one step per change");
    }

    #[test]
    fn reroll_changes_the_seed_in_one_undo_step() {
        use crate::test_rack::FM;
        use acid_engine::host::Command;
        let mut app = sel_app(FM, &[(0, 4, 60)], &[0]);
        let seed = app.doc.project.seed;
        let ctx = egui::Context::default();
        click_tool(&mut app, &ctx, "Reroll", 0.0);
        let new = app.doc.project.seed;
        assert_ne!(new, seed);
        let told = app
            .flushed
            .iter()
            .chain(&app.pending)
            .any(|c| matches!(c, Command::SetSeed(s) if *s == new));
        assert!(told, "the engine is told");
        assert_eq!(app.doc.undo_depth(), 1);
        app.undo();
        assert_eq!(app.doc.project.seed, seed);
        assert!(!app.doc.is_dirty());
    }

    #[test]
    fn a_narrow_header_wraps_the_inspector_instead_of_clipping_it() {
        use crate::test_rack::SYNTH1;
        let mut app = sel_app(SYNTH1, &[(0, 4, 36)], &[0]);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 800.0))),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
        for name in [
            "Snap to scale",
            "Chance",
            "Reroll",
            "Cond",
            "Accent",
            "Slide",
        ] {
            let r = tool(&app, name).unwrap_or_else(|| panic!("{name}"));
            assert!(r.right() <= 640.0, "{name} fits: {r:?}");
        }
        let rows = [tool(&app, "Snap to scale"), tool(&app, "Slide")].map(|r| r.unwrap().top());
        assert!(rows[1] > rows[0], "the header wrapped onto a second row");
    }

    /// One frame of the whole app in a `w` x 800 window.
    fn frame_w(
        app: &mut SidAcidApp,
        ctx: &egui::Context,
        w: f32,
        time: f64,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, 800.0))),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
    }

    #[test]
    fn selecting_a_note_does_not_move_the_lanes() {
        use crate::test_rack::SYNTH1;
        let mut app = sel_app(SYNTH1, &[(0, 4, 36)], &[]);
        app.view.dock.follow = false;
        let ctx = egui::Context::default();
        frame_w(&mut app, &ctx, 1024.0, 0.0, vec![]);
        let top = app.view.dock.lanes.unwrap().top();
        let all = app.doc.project.roll(SYNTH1, 0).unwrap().notes.clone();
        app.set_selection(SYNTH1, all);
        frame_w(&mut app, &ctx, 1024.0, 1.0, vec![]);
        assert!(tool(&app, "Chance").is_some(), "the inspector shows");
        assert_eq!(app.view.dock.lanes.unwrap().top(), top, "selected");
        app.set_selection(SYNTH1, Vec::new());
        frame_w(&mut app, &ctx, 1024.0, 2.0, vec![]);
        assert_eq!(app.view.dock.lanes.unwrap().top(), top, "cleared");
    }

    #[test]
    fn a_level_drag_of_a_fresh_note_keeps_its_pitch_at_1024() {
        use crate::test_rack::SYNTH1;
        use crate::ui::lane::{HEADER_H, ROW_H};
        let mut app = sel_app(SYNTH1, &[(0, 4, 36)], &[]);
        app.view.dock.follow = false;
        let ctx = egui::Context::default();
        frame_w(&mut app, &ctx, 1024.0, 0.0, vec![]);
        let left = app.view.dock.ruler.unwrap().left() + app.lane_key_w();
        let lanes = app.view.dock.lanes.unwrap();
        let scroll = app.view.panel(SYNTH1).lane_scroll_y.unwrap();
        let tw = tick_w(ZOOMS[app.view.dock.zoom]);
        let from = pos2(
            left + 1.5 * tw,
            lanes.top() + HEADER_H + (127 - 36) as f32 * ROW_H - scroll + ROW_H / 2.0,
        );
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut t = 1.0;
        let mut tick = |app: &mut SidAcidApp, events| {
            frame_w(app, &ctx, 1024.0, t, events);
            t += 1.0 / 60.0;
        };
        tick(&mut app, vec![egui::Event::PointerMoved(from)]);
        tick(&mut app, vec![button(from, true)]);
        for i in 1..=8 {
            tick(
                &mut app,
                vec![egui::Event::PointerMoved(from + vec2(5.0 * i as f32, 0.0))],
            );
        }
        tick(&mut app, vec![button(from + vec2(40.0, 0.0), false)]);
        tick(&mut app, vec![]);
        let n = app.doc.project.roll(SYNTH1, 0).unwrap().notes[0];
        assert_eq!(n.pitch, 36, "moved sideways only");
        assert!(n.start > 0, "it moved");
    }

    #[test]
    fn losing_the_selection_mid_drag_closes_the_inspector_step() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60), (8, 4, 62)], &[0]);
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let at = tool(&app, "Chance").unwrap().center();
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut app, &ctx, 1.0, vec![egui::Event::PointerMoved(at)]);
        frame(&mut app, &ctx, 1.1, vec![button(at, true)]);
        for i in 1..=4 {
            let p = at - vec2(20.0 * i as f32, 0.0);
            frame(
                &mut app,
                &ctx,
                1.1 + 0.05 * i as f64,
                vec![egui::Event::PointerMoved(p)],
            );
        }
        assert!(app.doc.project.roll(FM, 0).unwrap().notes[0].chance < 100);
        assert_eq!(app.doc.undo_depth(), 1);
        // The selection goes while the button is still down.
        app.set_selection(FM, Vec::new());
        frame(&mut app, &ctx, 1.5, vec![]);
        frame(
            &mut app,
            &ctx,
            1.6,
            vec![button(at - vec2(80.0, 0.0), false)],
        );
        // A merging edit of the same pattern must start its own step.
        let mut p = app.doc.project.roll(FM, 0).unwrap().clone();
        p.notes[1].vel = 10;
        app.commit_poly(FM, 0, Some(p), true, true);
        assert_eq!(app.doc.undo_depth(), 2, "the drag's step was closed");
    }

    #[test]
    fn typing_a_chance_is_one_undo_step() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60), (8, 4, 62)], &[0, 1]);
        let ctx = egui::Context::default();
        click_tool(&mut app, &ctx, "Chance", 0.0);
        let key = |key, pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let select_all = egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        frame(&mut app, &ctx, 1.1, vec![select_all]);
        frame(&mut app, &ctx, 1.2, vec![egui::Event::Text("4".into())]);
        frame(&mut app, &ctx, 1.3, vec![egui::Event::Text("2".into())]);
        frame(
            &mut app,
            &ctx,
            1.4,
            vec![key(egui::Key::Enter, true), key(egui::Key::Enter, false)],
        );
        frame(&mut app, &ctx, 1.5, vec![]);
        assert_eq!(chances(&app, FM), [42, 42]);
        assert_eq!(app.doc.undo_depth(), 1, "one step for the typed value");
    }

    #[test]
    fn two_chance_drags_are_two_undo_steps() {
        use crate::test_rack::FM;
        let mut app = sel_app(FM, &[(0, 4, 60)], &[0]);
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let at = tool(&app, "Chance").unwrap().center();
        drag_by(&mut app, &ctx, at, vec2(-60.0, 0.0), 6, 1.0);
        let first = chances(&app, FM)[0];
        let at = tool(&app, "Chance").unwrap().center();
        drag_by(&mut app, &ctx, at, vec2(-60.0, 0.0), 6, 5.0);
        assert!(chances(&app, FM)[0] < first, "the second drag moved it on");
        assert_eq!(app.doc.undo_depth(), 2);
    }
}
