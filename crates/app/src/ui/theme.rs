//! Colours and global visuals.

use eframe::egui::{self, Color32};

pub const ACID: Color32 = Color32::from_rgb(0xB8, 0xF5, 0x3A);
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
    visuals.selection.bg_fill = ACID.gamma_multiply(0.45);
    visuals.hyperlink_color = ACID;
    ctx.set_visuals(visuals);
}
