//! Custom-painted rotary knob and click-to-cycle switch.

use super::step_edit::scroll_notches;
use super::theme;
use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, Vec2, pos2, vec2};
use std::f32::consts::PI;

pub const KNOB_SIZE: f32 = 36.0;
const CELL: Vec2 = vec2(KNOB_SIZE + 10.0, KNOB_SIZE + 16.0);
const COARSE_PER_PIXEL: f32 = 0.006;
const FINE_PER_PIXEL: f32 = 0.001;
const SCROLL_STEP: f32 = 0.02;
const ANGLE_MIN: f32 = 0.75 * PI;
const ANGLE_MAX: f32 = 2.25 * PI;

pub struct KnobOutput {
    /// The new value, if the user changed it this frame.
    pub value: Option<f32>,
    /// The drag ended this frame (closes an undo gesture).
    pub released: bool,
    /// The knob was dragged, scrolled or double-clicked this frame.
    pub touched: bool,
    /// The mouse wheel moved over the knob this frame.
    pub scrolled: bool,
}

pub fn drag_to_value(value: f32, dy: f32, fine: bool) -> f32 {
    let per_pixel = if fine {
        FINE_PER_PIXEL
    } else {
        COARSE_PER_PIXEL
    };
    (value - dy * per_pixel).clamp(0.0, 1.0)
}

/// Moves `value` by `notches` scroll steps.
pub fn notches_to_value(value: f32, notches: i32) -> f32 {
    (value + notches as f32 * SCROLL_STEP).clamp(0.0, 1.0)
}

/// A rotary knob: drag vertically (Shift = fine), scroll, double-click to
/// reset to `default`.
pub fn knob(
    ui: &mut egui::Ui,
    label: &str,
    value: f32,
    default: f32,
    tooltip: &str,
    automated: bool,
) -> KnobOutput {
    let (rect, response) = ui.allocate_exact_size(CELL, Sense::click_and_drag());
    let mut new_value = None;
    let mut touched = response.dragged();
    let mut scrolled = false;
    if response.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let v = drag_to_value(value, response.drag_delta().y, fine);
        if v != value {
            new_value = Some(v);
        }
    }
    if response.double_clicked() {
        new_value = Some(default);
        touched = true;
    }
    if response.hovered() {
        // Take the wheel so the rack behind the knob does not scroll, and
        // count whole notches so the step does not depend on frame rate.
        let delta = ui.input_mut(|i| {
            let d = i.raw_scroll_delta.y;
            i.raw_scroll_delta.y = 0.0;
            i.smooth_scroll_delta.y = 0.0;
            d
        });
        if delta != 0.0 {
            touched = true;
            scrolled = true;
            let notches = ui
                .data_mut(|d| scroll_notches(d.get_temp_mut_or_default::<f32>(response.id), delta));
            if notches != 0 {
                new_value = Some(notches_to_value(new_value.unwrap_or(value), notches));
            }
        }
    }

    let shown = new_value.unwrap_or(value);
    let painter = ui.painter();
    let center = pos2(rect.center().x, rect.top() + KNOB_SIZE / 2.0 + 2.0);
    let radius = KNOB_SIZE / 2.0 - 3.0;
    let arc = |from: f32, to: f32| -> Vec<egui::Pos2> {
        (0..=24)
            .map(|i| {
                let a = from + (to - from) * i as f32 / 24.0;
                center + vec2(a.cos(), a.sin()) * (radius + 3.0)
            })
            .collect()
    };
    painter.add(egui::Shape::line(
        arc(ANGLE_MIN, ANGLE_MAX),
        Stroke::new(2.0_f32, theme::KNOB_TRACK),
    ));
    let angle = ANGLE_MIN + (ANGLE_MAX - ANGLE_MIN) * shown;
    painter.add(egui::Shape::line(
        arc(ANGLE_MIN, angle),
        Stroke::new(2.5_f32, theme::VIOLET),
    ));
    painter.circle_filled(center, radius, theme::KNOB_BODY);
    if automated {
        painter.circle_stroke(center, radius + 6.0, Stroke::new(1.5_f32, theme::SLIDE));
    }
    painter.line_segment(
        [
            center,
            center + vec2(angle.cos(), angle.sin()) * (radius - 2.0),
        ],
        Stroke::new(2.0_f32, Color32::WHITE),
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 6.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(10.0),
        ui.visuals().text_color(),
    );
    let response = response.on_hover_text(tooltip);
    KnobOutput {
        value: new_value,
        released: response.drag_stopped(),
        touched,
        scrolled,
    }
}

/// A button that cycles through `names` (left click forward, right click
/// back). Returns the new index when it changes.
pub fn switch(ui: &mut egui::Ui, label: &str, index: usize, names: &[&str]) -> Option<usize> {
    let n = names.len().max(1);
    let index = index.min(n - 1);
    let text = egui::RichText::new(format!("{label}\n{}", names[index])).size(10.0);
    let on = index > 0;
    let response = ui.add_sized(CELL, egui::Button::new(text).selected(on));
    if response.clicked() {
        Some((index + 1) % n)
    } else if response.secondary_clicked() {
        Some((index + n - 1) % n)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_up_raises_the_value() {
        assert!(drag_to_value(0.5, -10.0, false) > 0.5);
        assert!(drag_to_value(0.5, 10.0, false) < 0.5);
    }

    #[test]
    fn fine_drag_is_slower() {
        let coarse = drag_to_value(0.5, -10.0, false) - 0.5;
        let fine = drag_to_value(0.5, -10.0, true) - 0.5;
        assert!(fine > 0.0 && fine < coarse / 3.0);
    }

    #[test]
    fn values_stay_in_range() {
        assert_eq!(drag_to_value(0.99, -1000.0, false), 1.0);
        assert_eq!(drag_to_value(0.01, 1000.0, false), 0.0);
    }

    #[test]
    fn notches_move_by_a_fixed_step_and_clamp() {
        assert!((notches_to_value(0.5, 2) - 0.54).abs() < 1e-6);
        assert!((notches_to_value(0.5, -1) - 0.48).abs() < 1e-6);
        assert_eq!(notches_to_value(0.99, 5), 1.0);
        assert_eq!(notches_to_value(0.01, -5), 0.0);
        assert_eq!(notches_to_value(0.5, 0), 0.5);
    }
}
