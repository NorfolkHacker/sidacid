//! Colours and global visuals.

use eframe::egui::{self, Color32};

/// Live signals: knob arcs, notes, lit steps, playheads.
pub const ACID: Color32 = Color32::from_rgb(0xB8, 0xF5, 0x3A);
/// Outline of a lit note-grid cell.
pub const ACID_LIGHT: Color32 = Color32::from_rgb(0xDC, 0xFA, 0x9D);
/// Controls: selected and toggled buttons, pickers, menus.
pub const VIOLET: Color32 = Color32::from_rgb(0xA6, 0x4B, 0xFF);
/// Outline of a selected control.
pub const VIOLET_LIGHT: Color32 = Color32::from_rgb(0xD3, 0xA5, 0xFF);
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x50, 0x3C);
pub const SLIDE: Color32 = Color32::from_rgb(0x4C, 0xB8, 0xFF);
pub const QUEUED: Color32 = Color32::from_rgb(0xFF, 0xC8, 0x3C);
pub const KNOB_BODY: Color32 = Color32::from_rgb(0x3A, 0x3E, 0x44);
pub const KNOB_TRACK: Color32 = Color32::from_rgb(0x55, 0x5A, 0x60);
pub const GRID_BG: Color32 = Color32::from_rgb(0x1A, 0x1C, 0x20);
pub const GRID_DARK: Color32 = Color32::from_rgb(0x14, 0x15, 0x18);
pub const GRID_LINE: Color32 = Color32::from_rgb(0x30, 0x33, 0x38);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    control_visuals(&mut visuals);
    visuals.hyperlink_color = VIOLET;
    ctx.set_visuals(visuals);
}

/// Selected controls light violet.
pub fn control_visuals(visuals: &mut egui::Visuals) {
    visuals.selection.bg_fill = VIOLET.gamma_multiply(0.45);
    visuals.selection.stroke.color = VIOLET_LIGHT;
}

/// Step and drum grids light their cells green: those cells are the
/// notes, so they keep the signal colour rather than the control one.
pub fn note_grid_visuals(visuals: &mut egui::Visuals) {
    visuals.selection.bg_fill = ACID.gamma_multiply(0.45);
    visuals.selection.stroke.color = ACID_LIGHT;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_select_in_violet() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let v = ctx.style().visuals.clone();
        assert_eq!(v.selection.bg_fill, VIOLET.gamma_multiply(0.45));
        assert_eq!(v.selection.stroke.color, VIOLET_LIGHT);
    }

    #[test]
    fn note_grids_select_in_green() {
        let mut v = egui::Visuals::dark();
        note_grid_visuals(&mut v);
        assert_eq!(v.selection.bg_fill, ACID.gamma_multiply(0.45));
        assert_eq!(v.selection.stroke.color, ACID_LIGHT);
    }

    #[test]
    fn control_visuals_undo_a_note_grid() {
        let mut v = egui::Visuals::dark();
        note_grid_visuals(&mut v);
        control_visuals(&mut v);
        assert_eq!(v.selection.bg_fill, VIOLET.gamma_multiply(0.45));
        assert_eq!(v.selection.stroke.color, VIOLET_LIGHT);
    }
}
