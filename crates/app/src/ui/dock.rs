//! The editor dock at the bottom of the window: a header row (snap, zoom,
//! follow), a ruler that doubles as the shared horizontal scrollbar, and
//! every machine's lane stacked in rack order. Its height is a share of
//! the window kept in the settings file.

use super::lane_geom::{ZOOMS, follow_scroll, tick_w};
use super::poly_edit::Snap;
use super::{SidAcidApp, theme};
use acid_engine::project::TICKS_PER_STEP;
use eframe::egui::{self, Align2, FontId, Rect, RichText, Sense, Stroke, pos2, vec2};

/// Height of the ruler above the lanes.
const RULER_H: f32 = 16.0;
/// Gap between stacked lanes.
const LANE_GAP: f32 = 2.0;

impl SidAcidApp {
    /// Draws the dock as a bottom panel; call before the central panel so
    /// the rack takes what is left.
    pub(crate) fn dock(&mut self, ctx: &egui::Context) {
        let window_h = ctx.screen_rect().height().max(0.0);
        let frac = self.settings.ui.clamped_dock_frac();
        let panel = egui::TopBottomPanel::bottom("dock")
            .resizable(true)
            .default_height(window_h * frac)
            .height_range(window_h * 0.15..=window_h * 0.85)
            .show(ctx, |ui| {
                self.dock_header(ui);
                self.dock_body(ui);
            });
        let rect = panel.response.rect;
        self.track_dock_resize(ctx, rect, window_h, frac);
        self.view.dock.rect = Some(rect);
    }

    /// Watches the panel's height while its top edge is dragged; when the
    /// drag ends the new share of the window is kept.
    fn track_dock_resize(&mut self, ctx: &egui::Context, rect: Rect, window_h: f32, frac: f32) {
        let grab = ctx.style().interaction.resize_grab_radius_side + 1.0;
        let (down, on_edge) = ctx.input(|i| {
            let on_edge = i
                .pointer
                .press_origin()
                .is_some_and(|p| (p.y - rect.top()).abs() <= grab);
            (i.pointer.any_down(), on_edge)
        });
        if down {
            if on_edge && (rect.height() - window_h * frac).abs() > 1.0 {
                self.view.dock.resizing = true;
            }
        } else if std::mem::take(&mut self.view.dock.resizing) && window_h > 0.0 {
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

    /// Snap choices, zoom buttons and the follow toggle.
    fn dock_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Snap").small().color(theme::ACID));
            for snap in Snap::ALL {
                let on = self.view.dock.snap == snap;
                if ui.selectable_label(on, snap.label()).clicked() {
                    self.view.dock.snap = snap;
                }
            }
            ui.separator();
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
        });
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

    /// The ruler, then the lanes.
    fn dock_body(&mut self, ui: &mut egui::Ui) {
        let tw = tick_w(ZOOMS[self.view.dock.zoom.min(ZOOMS.len() - 1)]);
        let key_w = self.lane_key_w();
        let view_w = (ui.available_width() - key_w).max(0.0);
        let content_w = self.longest_ticks() as f32 * tw;
        if self.view.dock.follow
            && let Some(uid) = self.focused()
            && let Some(step) = self.playhead(uid, self.selected(uid))
        {
            let x = step as f32 * TICKS_PER_STEP as f32 * tw;
            self.view.dock.scroll_x = follow_scroll(self.view.dock.scroll_x, view_w, x);
        }
        self.ruler(ui, key_w, tw, view_w, content_w);
        let max_scroll = (content_w - view_w).max(0.0);
        self.view.dock.scroll_x = self.view.dock.scroll_x.clamp(0.0, max_scroll);
        egui::ScrollArea::vertical()
            .id_salt("dock_lanes")
            .auto_shrink(false)
            .show(ui, |ui| {
                for uid in self.doc.project.rack_uids() {
                    let width = ui.available_width();
                    self.lane(ui, uid, width);
                    ui.add_space(LANE_GAP);
                }
            });
    }

    /// Step numbers every 4 steps over the grids' columns, and a thumb
    /// showing the visible part of the longest pattern; dragging on it
    /// scrolls every lane.
    fn ruler(&mut self, ui: &mut egui::Ui, key_w: f32, tw: f32, view_w: f32, content_w: f32) {
        let width = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(width, RULER_H), Sense::drag());
        if resp.dragged() {
            self.view.dock.scroll_x = (self.view.dock.scroll_x + resp.drag_delta().x).max(0.0);
        }
        let left = rect.left() + key_w;
        let strip = Rect::from_min_max(pos2(left, rect.top()), rect.max);
        let painter = ui.painter_at(strip);
        painter.rect_filled(strip, 0.0, theme::GRID_DARK);
        let scroll_x = self.view.dock.scroll_x;
        if content_w > 0.0 && view_w > 0.0 {
            let shown = (view_w / content_w).min(1.0);
            let x0 = left + (scroll_x / content_w).min(1.0) * view_w;
            let thumb = Rect::from_min_max(
                pos2(x0, rect.bottom() - 4.0),
                pos2((x0 + shown * view_w).min(strip.right()), rect.bottom()),
            );
            painter.rect_filled(thumb, 2.0, theme::KNOB_TRACK);
        }
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
        resp.on_hover_text("Drag to scroll the lanes");
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
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
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
}
