//! The transport's performance readout: smoothed audio load, its recent
//! peak, renders that overran their time budget, and memory in use.

use super::{SidAcidApp, theme};
use acid_engine::host::Status;
use eframe::egui;

/// Seconds a peak stays on show before a lower one may replace it.
const PEAK_HOLD: f64 = 1.0;
/// Seconds between reads of the process's memory use.
const MEMORY_EVERY: f64 = 1.0;
/// Load at which a reading turns amber.
const BUSY: f32 = 0.8;

#[derive(Default)]
pub struct PerfMeter {
    peak: f32,
    peak_at: f64,
    /// `Status::overruns` from the latest status, and the count it held
    /// when the user last cleared the warning.
    overruns_seen: u32,
    overrun_base: u32,
    /// The stream's driver-reported xruns, likewise.
    xruns_seen: u32,
    xrun_base: u32,
    last_overrun_at: Option<f64>,
    memory: Option<u64>,
    memory_at: Option<f64>,
}

impl PerfMeter {
    pub fn on_status(&mut self, status: &Status, now: f64) {
        if status.cpu_peak >= self.peak || now - self.peak_at >= PEAK_HOLD {
            self.peak = status.cpu_peak;
            self.peak_at = now;
        }
        if status.overruns != self.overruns_seen {
            self.overruns_seen = status.overruns;
            self.last_overrun_at = Some(now);
        }
    }

    /// The stream's driver-reported xrun count (JACK); 0 elsewhere.
    pub fn on_xruns(&mut self, count: u32, now: f64) {
        if count != self.xruns_seen {
            self.xruns_seen = count;
            self.last_overrun_at = Some(now);
        }
    }

    /// A new stream counts its overruns from zero.
    pub fn restart(&mut self) {
        *self = PerfMeter {
            memory: self.memory,
            memory_at: self.memory_at,
            ..PerfMeter::default()
        };
    }

    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// Overruns since the stream started or the user last cleared them.
    pub fn overruns(&self) -> u32 {
        self.overruns_seen.wrapping_sub(self.overrun_base)
    }

    pub fn xruns(&self) -> u32 {
        self.xruns_seen.wrapping_sub(self.xrun_base)
    }

    /// Everything the warning counts: our overruns plus driver xruns.
    pub fn dropouts(&self) -> u32 {
        self.overruns().wrapping_add(self.xruns())
    }

    pub fn breakdown(&self) -> String {
        format!(
            "{} render overruns · {} driver xruns",
            self.overruns(),
            self.xruns()
        )
    }

    pub fn clear_overruns(&mut self) {
        self.overrun_base = self.overruns_seen;
        self.xrun_base = self.xruns_seen;
        self.last_overrun_at = None;
    }

    /// Re-reads memory use through `read` once `MEMORY_EVERY` has passed.
    pub fn tick_memory(&mut self, now: f64, read: impl FnOnce() -> Option<u64>) {
        if self.memory_at.is_none_or(|at| now - at >= MEMORY_EVERY) {
            self.memory = read();
            self.memory_at = Some(now);
        }
    }

    pub fn memory(&self) -> Option<u64> {
        self.memory
    }
}

/// Resident memory of this process, in bytes.
pub fn process_memory() -> Option<u64> {
    memory_stats::memory_stats().map(|m| m.physical_mem as u64)
}

fn megabytes(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / (1024.0 * 1024.0))
}

fn load_colour(load: f32, normal: egui::Color32) -> egui::Color32 {
    if load >= 1.0 {
        theme::ACCENT
    } else if load > BUSY {
        theme::QUEUED
    } else {
        normal
    }
}

impl SidAcidApp {
    pub(crate) fn perf_readout(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        self.view.perf.tick_memory(now, process_memory);
        if let Some(audio) = &self.audio {
            self.view.perf.on_xruns(audio.xruns(), now);
        }
        let normal = ui.visuals().text_color();
        let perf = &self.view.perf;
        let cpu = self.status.cpu;
        let mut hover = String::new();
        if let Some(audio) = &self.audio {
            let o = audio.opened();
            hover.push_str(&format!(
                "{} · {} · {} Hz",
                crate::audio_select::driver_label(&o.driver),
                o.device,
                o.sample_rate
            ));
            match audio.buffer_frames() {
                0 => {}
                n => hover.push_str(&format!(" · {n}-frame buffer")),
            }
            hover.push('\n');
        }
        hover.push_str("CPU: share of each audio callback's time spent rendering\npk: worst callback in the last second");
        let open_settings = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(format!("CPU {:.0}%", cpu * 100.0))
                        .color(load_colour(cpu, normal)),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_text(format!("{hover}\nClick for Audio Settings"))
            .clicked();
        ui.label(
            egui::RichText::new(format!("pk {:.0}%", perf.peak() * 100.0))
                .color(load_colour(perf.peak(), normal)),
        )
        .on_hover_text(&hover);
        if let Some(bytes) = perf.memory() {
            ui.label(format!("MEM {}", megabytes(bytes)))
                .on_hover_text("Memory this app is using");
        }
        let samples = self.doc.project.sampler.memory_bytes() as u64;
        let limit = acid_engine::sampler::MEMORY_LIMIT as u64;
        let colour = load_colour(samples as f32 / limit as f32, normal);
        ui.label(
            egui::RichText::new(format!(
                "Smp {}/{}",
                megabytes(samples).trim_end_matches(" MB"),
                megabytes(limit)
            ))
            .color(colour),
        )
        .on_hover_text("Decoded sampler audio, against its limit");
        let dropouts = perf.dropouts();
        if dropouts > 0 {
            let ago = perf
                .last_overrun_at
                .map(|at| format!("\nLast one {:.0} s ago", (now - at).max(0.0)))
                .unwrap_or_default();
            let clicked = ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(format!("⚠ {dropouts}")).color(theme::ACCENT),
                    )
                    .frame(false),
                )
                .on_hover_text(format!(
                    "Audible dropouts: {}.{ago}\nClick to clear.",
                    perf.breakdown()
                ))
                .clicked();
            if clicked {
                self.view.perf.clear_overruns();
            }
        }
        if open_settings {
            self.open_audio_window();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(cpu_peak: f32, overruns: u32) -> Status {
        Status {
            cpu_peak,
            overruns,
            ..Status::default()
        }
    }

    #[test]
    fn a_peak_holds_for_a_second_unless_a_higher_one_arrives() {
        let mut p = PerfMeter::default();
        p.on_status(&status(0.6, 0), 10.0);
        p.on_status(&status(0.2, 0), 10.5);
        assert_eq!(p.peak(), 0.6);
        p.on_status(&status(0.7, 0), 10.6);
        assert_eq!(p.peak(), 0.7);
        p.on_status(&status(0.3, 0), 11.5);
        assert_eq!(p.peak(), 0.7);
        p.on_status(&status(0.3, 0), 11.6);
        assert_eq!(p.peak(), 0.3);
    }

    #[test]
    fn overruns_count_from_the_last_clear_and_a_restart() {
        let mut p = PerfMeter::default();
        p.on_status(&status(0.0, 3), 1.0);
        assert_eq!(p.overruns(), 3);
        assert_eq!(p.last_overrun_at, Some(1.0));
        p.clear_overruns();
        assert_eq!(p.overruns(), 0);
        assert_eq!(p.last_overrun_at, None);
        p.on_status(&status(0.0, 5), 2.0);
        assert_eq!(p.overruns(), 2);
        // The host's counter wraps; the difference still holds.
        p.on_status(&status(0.0, u32::MAX), 3.0);
        p.clear_overruns();
        p.on_status(&status(0.0, 1), 4.0);
        assert_eq!(p.overruns(), 2);
        p.restart();
        p.on_status(&status(0.0, 1), 5.0);
        assert_eq!(p.overruns(), 1);
    }

    #[test]
    fn xruns_add_to_the_warning_and_clear_with_it() {
        let mut p = PerfMeter::default();
        p.on_status(&status(0.0, 2), 1.0);
        p.on_xruns(5, 1.5);
        assert_eq!((p.overruns(), p.xruns()), (2, 5));
        assert_eq!(p.dropouts(), 7);
        assert_eq!(p.last_overrun_at, Some(1.5));
        assert_eq!(p.breakdown(), "2 render overruns · 5 driver xruns");
        p.clear_overruns();
        assert_eq!(p.dropouts(), 0);
        p.on_xruns(6, 2.0);
        assert_eq!(p.dropouts(), 1);
        p.restart();
        p.on_xruns(0, 3.0);
        assert_eq!(p.dropouts(), 0);
    }

    #[test]
    fn memory_is_read_once_a_second_and_kept_across_a_restart() {
        let mut p = PerfMeter::default();
        let mut reads = 0;
        let mut read = |v| {
            reads += 1;
            Some(v)
        };
        p.tick_memory(0.0, || read(100));
        p.tick_memory(0.5, || read(200));
        assert_eq!(p.memory(), Some(100));
        p.tick_memory(1.0, || read(300));
        assert_eq!(p.memory(), Some(300));
        assert_eq!(reads, 2);
        p.restart();
        assert_eq!(p.memory(), Some(300));
    }

    #[test]
    fn this_process_reports_some_memory() {
        assert!(process_memory().is_some_and(|b| b > 1024 * 1024));
    }
}
