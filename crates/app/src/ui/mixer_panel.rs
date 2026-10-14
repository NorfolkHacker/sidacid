//! Mixer strips, delay and reverb returns, master compressor, meters.

use super::{SidAcidApp, theme};
use acid_engine::machine::MAX_MACHINES;
use acid_engine::params::{ChannelParam, FxParam, MasterParam, ParamId};
use eframe::egui::{self, Sense, vec2};

const METER_FLOOR_DB: f32 = -48.0;

const STRIP: [(ChannelParam, &str); 16] = [
    (ChannelParam::Level, "Level"),
    (ChannelParam::Pan, "Pan"),
    (ChannelParam::Mute, "Mute"),
    (ChannelParam::Solo, "Solo"),
    (ChannelParam::DistOn, "Dist"),
    (ChannelParam::DistDrive, "Drive"),
    (ChannelParam::DistTone, "Tone"),
    (ChannelParam::DistMix, "Mix"),
    (ChannelParam::CompOn, "Comp"),
    (ChannelParam::CompThreshold, "Thresh"),
    (ChannelParam::CompRatio, "Ratio"),
    (ChannelParam::CompAttack, "Attack"),
    (ChannelParam::CompRelease, "Release"),
    (ChannelParam::CompMakeup, "Makeup"),
    (ChannelParam::DelaySend, "Delay"),
    (ChannelParam::ReverbSend, "Reverb"),
];
const DELAY: [(FxParam, &str); 7] = [
    (FxParam::DelaySync, "Sync"),
    (FxParam::DelayDivision, "Time"),
    (FxParam::DelayMs, "ms"),
    (FxParam::DelayFeedback, "Fdbk"),
    (FxParam::DelayTone, "Tone"),
    (FxParam::DelayPingPong, "Ping"),
    (FxParam::DelayReturn, "Return"),
];
const REVERB: [(FxParam, &str); 4] = [
    (FxParam::ReverbSize, "Size"),
    (FxParam::ReverbDamping, "Damp"),
    (FxParam::ReverbWidth, "Width"),
    (FxParam::ReverbReturn, "Return"),
];
const MASTER: [(MasterParam, &str); 6] = [
    (MasterParam::CompOn, "Comp"),
    (MasterParam::CompThreshold, "Thresh"),
    (MasterParam::CompRatio, "Ratio"),
    (MasterParam::CompAttack, "Attack"),
    (MasterParam::CompRelease, "Release"),
    (MasterParam::CompMakeup, "Makeup"),
];

/// Peak (linear) to meter height 0..1 on a -48..0 dB scale.
pub fn meter_fraction(peak: f32) -> f32 {
    // NaN would pass through `clamp`.
    if peak.is_nan() || peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0)
}

fn meter(ui: &mut egui::Ui, peak: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 80.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, theme::GRID_DARK);
    let f = meter_fraction(peak);
    let mut lit = rect;
    lit.set_top(rect.bottom() - rect.height() * f);
    painter.rect_filled(
        lit,
        1.0,
        if peak >= 0.99 {
            theme::ACCENT
        } else {
            theme::ACID
        },
    );
}

impl SidAcidApp {
    pub(crate) fn mixer_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(egui::RichText::new("MIXER & FX").strong())
            .id_salt("mixer")
            .default_open(true)
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("mixer_row")
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            // One strip per machine in rack order; `Status`
                            // meters are by rack position.
                            let strips: Vec<_> = self
                                .doc
                                .project
                                .machines
                                .iter()
                                .map(|m| (m.uid, m.name.clone()))
                                .collect();
                            for (pos, (uid, name)) in strips.into_iter().enumerate() {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.strong(name);
                                        meter(ui, self.status.meters[pos]);
                                    });
                                    egui::Grid::new(("strip", uid)).spacing([0.0, 0.0]).show(
                                        ui,
                                        |ui| {
                                            for (i, (p, label)) in STRIP.iter().enumerate() {
                                                self.param_knob(ui, uid.param(*p), label);
                                                if i % 4 == 3 {
                                                    ui.end_row();
                                                }
                                            }
                                        },
                                    );
                                });
                                ui.separator();
                            }
                            ui.vertical(|ui| {
                                ui.strong("Delay");
                                egui::Grid::new("delay").spacing([0.0, 0.0]).show(ui, |ui| {
                                    for (i, (p, label)) in DELAY.iter().enumerate() {
                                        self.param_knob(ui, ParamId::Fx(*p), label);
                                        if i % 2 == 1 {
                                            ui.end_row();
                                        }
                                    }
                                });
                                ui.strong("Reverb");
                                egui::Grid::new("reverb")
                                    .spacing([0.0, 0.0])
                                    .show(ui, |ui| {
                                        for (i, (p, label)) in REVERB.iter().enumerate() {
                                            self.param_knob(ui, ParamId::Fx(*p), label);
                                            if i % 2 == 1 {
                                                ui.end_row();
                                            }
                                        }
                                    });
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.strong("Master");
                                    meter(ui, self.status.meters[MAX_MACHINES]);
                                });
                                egui::Grid::new("master")
                                    .spacing([0.0, 0.0])
                                    .show(ui, |ui| {
                                        for (i, (p, label)) in MASTER.iter().enumerate() {
                                            self.param_knob(ui, ParamId::Master(*p), label);
                                            if i % 2 == 1 {
                                                ui.end_row();
                                            }
                                        }
                                    });
                                if ui.input(|i| i.time) < self.view.fault_until {
                                    ui.colored_label(theme::ACCENT, "Audio fault recovered");
                                }
                            });
                        });
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_uses_a_48_db_scale() {
        assert_eq!(meter_fraction(0.0), 0.0);
        assert_eq!(meter_fraction(1.0), 1.0);
        assert!((meter_fraction(0.5) - (1.0 - 6.02 / 48.0)).abs() < 0.01);
        assert_eq!(meter_fraction(0.001), 0.0);
        assert_eq!(meter_fraction(4.0), 1.0);
        assert_eq!(
            meter_fraction(f32::NAN),
            0.0,
            "a NaN peak shows an empty meter"
        );
    }
}
