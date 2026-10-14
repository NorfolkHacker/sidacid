//! File → Audio Settings…: driver, device, sample rate and buffer.

use super::{SidAcidApp, open_audio, theme};
use crate::audio::AudioLink;
use crate::audio_select::{
    BUFFERS, HostInfo, JACK, buffer_options, driver_label, latency_ms, listed_devices, rate_options,
};
use crate::settings::AudioSettings;
use eframe::egui;

#[derive(Default)]
pub struct AudioWindow {
    pub open: bool,
    /// The settings being edited (copied from `app.settings.audio` on open).
    pub edit: AudioSettings,
    pub hosts: Option<Vec<HostInfo>>,
    pub scan_rx: Option<std::sync::mpsc::Receiver<Vec<HostInfo>>>,
    pub show_all: bool,
    pub error: Option<String>,
}

/// Opens a stream for the settings: the link (if any) and fallback notes.
type OpenFn<'a> =
    dyn FnMut(&AudioSettings) -> Result<(Option<AudioLink>, Vec<String>), String> + 'a;

#[derive(Debug, PartialEq)]
pub enum ApplyOutcome {
    Unchanged,
    Applied,
    Failed(String),
}

impl AudioWindow {
    /// Device scans take ~0.5 s: run them on a worker thread.
    fn rescan(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::audio_scan::scan_all());
        });
        self.scan_rx = Some(rx);
    }

    fn poll_scan(&mut self) {
        if let Some(hosts) = self.scan_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.hosts = Some(hosts);
            self.scan_rx = None;
        }
    }
}

impl SidAcidApp {
    pub(crate) fn open_audio_window(&mut self) {
        let w = &mut self.view.audio_window;
        w.open = true;
        w.edit = self.settings.audio.clone();
        w.error = None;
        w.rescan();
    }

    /// Applies `new`; `open` stands in for `open_audio` in tests.
    pub(crate) fn apply_audio(&mut self, new: AudioSettings, open: &mut OpenFn) -> ApplyOutcome {
        if new == self.settings.audio {
            self.view.audio_window.open = false;
            return ApplyOutcome::Unchanged;
        }
        self.pending.clear();
        self.reset_transport_view();
        self.view.perf.restart();
        // Release the old stream first: JACK and ASIO allow one per device.
        self.audio = None;
        match open(&new) {
            Ok((link, notes)) => {
                self.audio = link;
                self.audio_error = None;
                self.settings.audio = new;
                if let Some(path) = &self.settings_path
                    && let Err(e) = crate::settings::save(path, &self.settings)
                {
                    self.view.status_line = Some(format!("Could not save settings: {e}"));
                }
                if !notes.is_empty() {
                    self.view.status_line = Some(notes.join("; "));
                }
                self.view.audio_window.open = false;
                ApplyOutcome::Applied
            }
            Err(e) => {
                let previous = self.settings.audio.clone();
                match open(&previous) {
                    Ok((link, _)) => {
                        self.audio = link;
                        self.audio_error = None;
                    }
                    Err(e2) => self.audio_error = Some(e2),
                }
                self.view.audio_window.error = Some(e.clone());
                ApplyOutcome::Failed(e)
            }
        }
    }

    pub(crate) fn audio_settings_window(&mut self, ctx: &egui::Context) {
        if !self.view.audio_window.open {
            return;
        }
        self.view.audio_window.poll_scan();
        if self.view.audio_window.scan_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let now_line = match &self.audio {
            Some(a) => {
                let o = a.opened();
                let frames = match a.buffer_frames() {
                    0 => String::new(),
                    n => format!(" · {n} frames ({:.1} ms)", latency_ms(n, o.sample_rate)),
                };
                format!(
                    "{} · {} · {} Hz{frames}",
                    driver_label(&o.driver),
                    o.device,
                    o.sample_rate
                )
            }
            None => "no audio".into(),
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Audio Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let w = &mut self.view.audio_window;
                ui.label(format!("Now: {now_line}"));
                ui.separator();
                egui::Grid::new("audio_settings_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        driver_row(ui, w);
                        ui.end_row();
                        device_row(ui, w);
                        ui.end_row();
                        rate_and_buffer_rows(ui, w);
                    });
                if let Some(e) = &w.error {
                    ui.colored_label(theme::ACCENT, e);
                }
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                });
            });
        if !open || cancel {
            self.view.audio_window.open = false;
            return;
        }
        if apply {
            let new = self.view.audio_window.edit.clone();
            let project = self.doc.project.clone();
            self.apply_audio(new, &mut |s| {
                open_audio(&project, s).map(|(l, n)| (Some(l), n))
            });
        }
    }
}

fn selected_host(w: &AudioWindow) -> Option<&HostInfo> {
    let hosts = w.hosts.as_ref()?;
    let id = w
        .edit
        .driver
        .clone()
        .unwrap_or_else(|| crate::audio_scan::default_host_name().into());
    hosts.iter().find(|h| h.id == id)
}

fn driver_row(ui: &mut egui::Ui, w: &mut AudioWindow) {
    ui.label("Driver:");
    let current = w
        .edit
        .driver
        .as_deref()
        .map_or("Default".to_string(), |d| driver_label(d).to_string());
    egui::ComboBox::from_id_salt("audio_driver")
        .selected_text(current)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(w.edit.driver.is_none(), "Default")
                .clicked()
            {
                w.edit = AudioSettings::default();
            }
            for id in w
                .hosts
                .iter()
                .flatten()
                .map(|h| h.id.clone())
                .collect::<Vec<_>>()
            {
                let on = w.edit.driver.as_deref() == Some(&id);
                if ui.selectable_label(on, driver_label(&id)).clicked() && !on {
                    // A new driver starts from its defaults.
                    w.edit = AudioSettings {
                        driver: Some(id),
                        ..AudioSettings::default()
                    };
                }
            }
        });
}

fn device_row(ui: &mut egui::Ui, w: &mut AudioWindow) {
    ui.label("Device:");
    ui.horizontal(|ui| {
        let scanning = w.scan_rx.is_some();
        let text = if scanning && w.hosts.is_none() {
            "Scanning devices…".to_string()
        } else {
            w.edit.device.clone().unwrap_or_else(|| "Default".into())
        };
        let names: Vec<String> = selected_host(w)
            .map(|h| {
                listed_devices(h, w.show_all)
                    .iter()
                    .map(|d| d.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        egui::ComboBox::from_id_salt("audio_device")
            .selected_text(text)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(w.edit.device.is_none(), "Default")
                    .clicked()
                {
                    w.edit.device = None;
                }
                for n in names {
                    let on = w.edit.device.as_deref() == Some(&n);
                    if ui.selectable_label(on, &n).clicked() {
                        w.edit.device = Some(n);
                    }
                }
            });
        if ui
            .add_enabled(!scanning, egui::Button::new("Rescan"))
            .clicked()
        {
            w.rescan();
        }
        ui.checkbox(&mut w.show_all, "Show all devices");
    });
}

fn rate_and_buffer_rows(ui: &mut egui::Ui, w: &mut AudioWindow) {
    let host = selected_host(w);
    let is_jack = host.is_some_and(|h| h.id == JACK);
    let dev = host.and_then(|h| {
        let name = w.edit.device.clone().or_else(|| h.default_device.clone())?;
        h.devices.iter().find(|d| d.name == name).cloned()
    });
    ui.label("Sample rate:");
    if is_jack {
        ui.weak("set by the JACK server");
    } else {
        let default = dev.as_ref().map_or("Default".into(), |d| {
            format!("Default ({})", d.default_rate)
        });
        let text = w
            .edit
            .sample_rate
            .map_or(default.clone(), |r| format!("{r} Hz"));
        let rates = dev.as_ref().map(rate_options).unwrap_or_default();
        egui::ComboBox::from_id_salt("audio_rate")
            .selected_text(text)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(w.edit.sample_rate.is_none(), default)
                    .clicked()
                {
                    w.edit.sample_rate = None;
                }
                for r in rates {
                    if ui
                        .selectable_label(w.edit.sample_rate == Some(r), format!("{r} Hz"))
                        .clicked()
                    {
                        w.edit.sample_rate = Some(r);
                    }
                }
            });
    }
    ui.end_row();
    ui.label("Buffer:");
    if is_jack {
        ui.weak("set by the JACK server");
    } else {
        ui.horizontal(|ui| {
            let text = w
                .edit
                .buffer_frames
                .map_or("Default".into(), |n| format!("{n} frames"));
            let options = dev.as_ref().map(buffer_options).unwrap_or(BUFFERS.to_vec());
            egui::ComboBox::from_id_salt("audio_buffer")
                .selected_text(text)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(w.edit.buffer_frames.is_none(), "Default")
                        .clicked()
                    {
                        w.edit.buffer_frames = None;
                    }
                    for n in options {
                        if ui
                            .selectable_label(
                                w.edit.buffer_frames == Some(n),
                                format!("{n} frames"),
                            )
                            .clicked()
                        {
                            w.edit.buffer_frames = Some(n);
                        }
                    }
                });
            if let Some(n) = w.edit.buffer_frames {
                let rate = w
                    .edit
                    .sample_rate
                    .or(dev.as_ref().map(|d| d.default_rate))
                    .unwrap_or(48000);
                ui.weak(format!("≈ {:.1} ms", latency_ms(n, rate)));
            }
        });
    }
    ui.end_row();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;

    fn app() -> SidAcidApp {
        SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()))
    }

    fn jack() -> AudioSettings {
        AudioSettings {
            driver: Some("JACK".into()),
            ..AudioSettings::default()
        }
    }

    #[test]
    fn applying_unchanged_settings_does_not_reopen() {
        let mut app = app();
        let mut calls = 0;
        let out = app.apply_audio(AudioSettings::default(), &mut |_| {
            calls += 1;
            Ok((None, vec![]))
        });
        assert_eq!(out, ApplyOutcome::Unchanged);
        assert_eq!(calls, 0);
    }

    #[test]
    fn a_good_apply_keeps_the_settings_and_shows_notes() {
        let mut app = app();
        app.view.audio_window.open = true;
        let out = app.apply_audio(jack(), &mut |_| Ok((None, vec!["note".into()])));
        assert_eq!(out, ApplyOutcome::Applied);
        assert_eq!(app.settings.audio, jack());
        assert_eq!(app.view.status_line.as_deref(), Some("note"));
        assert!(!app.view.audio_window.open);
    }

    #[test]
    fn a_failed_apply_restores_the_old_setup_and_saves_nothing() {
        let mut app = app();
        app.view.audio_window.open = true;
        let mut tried = Vec::new();
        let out = app.apply_audio(jack(), &mut |s| {
            tried.push(s.clone());
            if s.driver.is_some() {
                Err("busy".into())
            } else {
                Ok((None, vec![]))
            }
        });
        assert_eq!(out, ApplyOutcome::Failed("busy".into()));
        assert_eq!(tried, [jack(), AudioSettings::default()]);
        assert_eq!(app.settings.audio, AudioSettings::default());
        assert!(app.view.audio_window.open);
        assert_eq!(app.view.audio_window.error.as_deref(), Some("busy"));
        assert_eq!(app.audio_error, None);
    }

    #[test]
    fn a_failed_apply_whose_restore_fails_reports_no_audio() {
        let mut app = app();
        let out = app.apply_audio(jack(), &mut |_| Err("gone".into()));
        assert_eq!(out, ApplyOutcome::Failed("gone".into()));
        assert_eq!(app.audio_error.as_deref(), Some("gone"));
    }

    #[test]
    fn opening_the_window_copies_the_saved_settings() {
        let mut app = app();
        app.settings.audio = jack();
        app.open_audio_window();
        assert!(app.view.audio_window.open);
        assert_eq!(app.view.audio_window.edit, jack());
        assert!(app.view.audio_window.scan_rx.is_some());
        assert_eq!(app.view.audio_window.error, None);
    }
}
