//! Colours and global visuals.

use eframe::egui::{self, Color32};

/// Buttons, pickers, sliders, playheads.
pub const ACID: Color32 = Color32::from_rgb(0xB8, 0xF5, 0x3A);
/// Outline of a selected button.
pub const ACID_LIGHT: Color32 = Color32::from_rgb(0xDC, 0xFA, 0x9D);
/// Knob arcs and the switches that sit among the knobs.
pub const VIOLET: Color32 = Color32::from_rgb(0xA6, 0x4B, 0xFF);
/// Outline of a switch that is on.
pub const VIOLET_LIGHT: Color32 = Color32::from_rgb(0xD3, 0xA5, 0xFF);
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x50, 0x3C);
pub const SLIDE: Color32 = Color32::from_rgb(0x4C, 0xB8, 0xFF);
pub const QUEUED: Color32 = Color32::from_rgb(0xFF, 0xC8, 0x3C);
pub const KNOB_BODY: Color32 = Color32::from_rgb(0x3A, 0x3E, 0x44);
pub const KNOB_TRACK: Color32 = Color32::from_rgb(0x55, 0x5A, 0x60);
pub const GRID_BG: Color32 = Color32::from_rgb(0x1A, 0x1C, 0x20);
pub const GRID_DARK: Color32 = Color32::from_rgb(0x14, 0x15, 0x18);
pub const GRID_LINE: Color32 = Color32::from_rgb(0x30, 0x33, 0x38);
/// The machine colours, by a machine's `colour` index: its notes in the
/// dock and Song mode, its panel title and frame, its mixer strip.
pub const MACHINE_COLOURS: [Color32; 5] = [
    Color32::from_rgb(0x00, 0xFF, 0x41),
    Color32::from_rgb(0x00, 0xE5, 0xFF),
    Color32::from_rgb(0xFF, 0x00, 0xFF),
    Color32::from_rgb(0xFF, 0xFF, 0x00),
    Color32::from_rgb(0x00, 0x80, 0xFF),
];

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    green_selection(&mut visuals);
    visuals.hyperlink_color = ACID;
    ctx.set_visuals(visuals);
}

/// Selected buttons and slider trails light green (the app default).
pub fn green_selection(visuals: &mut egui::Visuals) {
    visuals.selection.bg_fill = ACID.gamma_multiply(0.45);
    visuals.selection.stroke.color = ACID_LIGHT;
}

/// Switches in the knob rows (Wave, Mode, Unison, ...) light violet.
pub fn violet_selection(visuals: &mut egui::Visuals) {
    visuals.selection.bg_fill = VIOLET.gamma_multiply(0.45);
    visuals.selection.stroke.color = VIOLET_LIGHT;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_select_in_green() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let v = ctx.style().visuals.clone();
        assert_eq!(v.selection.bg_fill, ACID.gamma_multiply(0.45));
        assert_eq!(v.selection.stroke.color, ACID_LIGHT);
    }

    #[test]
    fn switches_select_in_violet() {
        let mut v = egui::Visuals::dark();
        violet_selection(&mut v);
        assert_eq!(v.selection.bg_fill, VIOLET.gamma_multiply(0.45));
        assert_eq!(v.selection.stroke.color, VIOLET_LIGHT);
    }

    #[test]
    fn the_machine_palette_is_the_engines_five() {
        let hex = |c: Color32| (c.r(), c.g(), c.b());
        let want = [
            (0x00, 0xFF, 0x41),
            (0x00, 0xE5, 0xFF),
            (0xFF, 0x00, 0xFF),
            (0xFF, 0xFF, 0x00),
            (0x00, 0x80, 0xFF),
        ];
        assert_eq!(MACHINE_COLOURS.map(hex), want);
        assert_eq!(MACHINE_COLOURS.len(), acid_engine::machine::MACHINE_COLOURS);
    }
}
