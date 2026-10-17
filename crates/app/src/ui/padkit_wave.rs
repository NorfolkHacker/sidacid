//! The Pad Kit's waveform: the selected layer, with Start and End markers
//! that drag like knobs (they record and undo as the Start and End params).
//! It is mirrored while Reverse is on, so it reads in playing order.

use super::sampler_ui::{Marker, nearest_marker, peaks};
use super::{SidAcidApp, knob, theme};
use acid_engine::machine::MachineUid;
use acid_engine::padkit::{MAX_LAYERS, PADS};
use acid_engine::params::{MachineParam, PadParam, ParamId, on};
use eframe::egui::{self, Color32, Sense, Stroke, pos2, vec2};

const WAVE_H: f32 = 90.0;
const MARKER_GRAB_PX: f32 = 5.0;

/// The x of fraction `f` across a waveform, right to left when `reversed`.
pub fn frac_to_x(f: f32, left: f32, width: f32, reversed: bool) -> f32 {
    let f = f.clamp(0.0, 1.0);
    left + if reversed { 1.0 - f } else { f } * width
}

/// The fraction at `x`, the inverse of `frac_to_x`.
pub fn x_to_frac(x: f32, left: f32, width: f32, reversed: bool) -> f32 {
    let f = ((x - left) / width.max(1.0)).clamp(0.0, 1.0);
    if reversed { 1.0 - f } else { f }
}

impl SidAcidApp {
    /// `id`'s value as shown: the live value while recording.
    fn shown_param(&self, id: ParamId) -> f32 {
        if self.recording()
            && let Some(&v) = self.view.live.get(&id)
        {
            return v;
        }
        self.doc.project.param(id)
    }

    /// Moves pad `pad`'s Start or End marker to fraction `v`, kept on its
    /// side of the other one, as one frame of a knob drag.
    pub(crate) fn set_pad_marker(&mut self, uid: MachineUid, pad: u8, which: PadParam, v: f32) {
        let start = uid.param(MachineParam::Pad(pad, PadParam::Start));
        let end = uid.param(MachineParam::Pad(pad, PadParam::End));
        let (id, v) = match which {
            PadParam::Start => (start, v.clamp(0.0, self.shown_param(end))),
            PadParam::End => (end, v.clamp(self.shown_param(start), 1.0)),
            _ => return,
        };
        let out = knob::KnobOutput {
            value: Some(v),
            released: false,
            touched: true,
            scrolled: false,
        };
        // `now` only times wheel gestures; a marker never scrolls.
        let recording = self.recording();
        self.knob_gesture(id, out, recording, 0.0);
    }

    /// Ends a marker drag on kit `uid`, if one is open.
    fn end_pad_marker_drag(&mut self, uid: MachineUid) {
        let Some((u, pad, marker)) = self.view.wave_drag else {
            return;
        };
        if u != uid {
            return;
        }
        self.view.wave_drag = None;
        let which = if marker == Marker::Start {
            PadParam::Start
        } else {
            PadParam::End
        };
        let out = knob::KnobOutput {
            value: None,
            released: true,
            touched: false,
            scrolled: false,
        };
        let recording = self.recording();
        self.knob_gesture(
            uid.param(MachineParam::Pad(pad as u8, which)),
            out,
            recording,
            0.0,
        );
    }

    pub(crate) fn padkit_waveform(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let p = self.view.panel(uid).pad.min(PADS - 1);
        let l = self.view.panel(uid).pad_layer.min(MAX_LAYERS - 1);
        let Some(layer) = self
            .doc
            .project
            .padkit(uid)
            .and_then(|k| k.pads[p].layers.get(l))
        else {
            // A drag whose layer emptied under it still closes its undo step.
            self.end_pad_marker_drag(uid);
            return;
        };
        let width = ui.available_width().clamp(200.0, 900.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, WAVE_H), Sense::click_and_drag());
        let columns = width as usize;
        let cache = (uid, p * MAX_LAYERS + l);
        let key = (layer.pcm.as_ptr() as usize, layer.pcm.len(), columns);
        let fresh = self
            .view
            .wave_cache
            .get(&cache)
            .is_some_and(|c| (c.0, c.1, c.2) == key);
        if !fresh {
            let cols = peaks(&layer.pcm, layer.channels as usize, columns);
            self.view
                .wave_cache
                .insert(cache, (key.0, key.1, key.2, cols));
        }
        let param = |q| uid.param(MachineParam::Pad(p as u8, q));
        let reversed = on(self.shown_param(param(PadParam::Reverse)));
        let start = self.shown_param(param(PadParam::Start));
        let end = self.shown_param(param(PadParam::End));
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, theme::GRID_DARK);
        let mid = rect.center().y;
        let half = rect.height() * 0.45;
        if let Some((_, _, _, cols)) = self.view.wave_cache.get(&cache) {
            for (i, (lo, hi)) in cols.iter().enumerate() {
                let i = if reversed { columns - 1 - i } else { i };
                let x = rect.left() + i as f32 + 0.5;
                painter.line_segment(
                    [pos2(x, mid - hi * half), pos2(x, mid - lo * half)],
                    Stroke::new(1.0_f32, theme::KNOB_TRACK),
                );
            }
        }
        let xs = [start, end].map(|f| frac_to_x(f, rect.left(), rect.width(), reversed));
        // Dim what the markers trim off.
        let (a, b) = (xs[0].min(xs[1]), xs[0].max(xs[1]));
        let shade = Color32::from_black_alpha(140);
        painter.rect_filled(
            egui::Rect::from_x_y_ranges(rect.left()..=a, rect.y_range()),
            0.0,
            shade,
        );
        painter.rect_filled(
            egui::Rect::from_x_y_ranges(b..=rect.right(), rect.y_range()),
            0.0,
            shade,
        );
        for x in xs {
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.5_f32, theme::ACCENT),
            );
        }
        let pointer = resp.interact_pointer_pos();
        if resp.drag_started()
            && let Some(pos) = pointer
            && let Some(i) = nearest_marker(&xs, pos.x, MARKER_GRAB_PX)
        {
            // Markers on top of each other: take the one that can move
            // toward the middle.
            let marker = if (xs[0] - xs[1]).abs() < 0.5 {
                if start < 0.5 {
                    Marker::End
                } else {
                    Marker::Start
                }
            } else if i == 0 {
                Marker::Start
            } else {
                Marker::End
            };
            self.view.wave_drag = Some((uid, p, marker));
        }
        if resp.dragged()
            && let Some(pos) = pointer
            && let Some((du, dp, marker)) = self.view.wave_drag
            && (du, dp) == (uid, p)
        {
            let which = if marker == Marker::Start {
                PadParam::Start
            } else {
                PadParam::End
            };
            let v = x_to_frac(pos.x, rect.left(), rect.width(), reversed);
            self.set_pad_marker(uid, p as u8, which, v);
        }
        if resp.drag_stopped() {
            self.end_pad_marker_drag(uid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::machine::MachineType;
    use acid_engine::project::Project;

    #[test]
    fn a_marker_drag_is_one_undo_step_and_its_switches_have_names() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        let end = uid.param(MachineParam::Pad(2, PadParam::End));
        for v in [0.9, 0.7, 0.5] {
            app.set_pad_marker(uid, 2, PadParam::End, v);
        }
        app.view.wave_drag = Some((uid, 2, Marker::End));
        app.end_pad_marker_drag(uid);
        assert_eq!(app.doc.project.param(end), 0.5);
        app.set_pad_marker(uid, 2, PadParam::Start, 0.8);
        assert_eq!(
            app.doc
                .project
                .param(uid.param(MachineParam::Pad(2, PadParam::Start))),
            0.5,
            "Start stops at End"
        );
        app.doc.undo();
        app.doc.undo();
        assert_eq!(app.doc.project.param(end), 1.0, "the drag undoes at once");
        for p in PadParam::ALL {
            let id = uid.param(MachineParam::Pad(0, p));
            let steps = id.info().steps;
            if steps >= 2 {
                let names = super::super::params_ui::discrete_names(id);
                assert_eq!(names.len(), steps as usize, "{p:?}");
            }
        }
    }

    #[test]
    fn fractions_and_pixels_round_trip_mirrored_or_not() {
        assert_eq!(frac_to_x(0.25, 10.0, 200.0, false), 60.0);
        assert_eq!(frac_to_x(0.25, 10.0, 200.0, true), 160.0);
        for rev in [false, true] {
            let x = frac_to_x(0.4, 10.0, 200.0, rev);
            assert!((x_to_frac(x, 10.0, 200.0, rev) - 0.4).abs() < 1e-6);
        }
        assert_eq!(x_to_frac(-5.0, 10.0, 200.0, false), 0.0);
        assert_eq!(x_to_frac(-5.0, 10.0, 200.0, true), 1.0);
    }
}
