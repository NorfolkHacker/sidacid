//! The SidAcid window: transport bar on top, a scrolling rack of machine
//! panels below. The UI owns the project (`Document`); every change goes
//! to the audio thread as `Command`s through `AudioLink`.

mod alg_diagram;
mod audio_settings;
mod drum_panel;
mod files;
mod fm_panel;
mod fm_voices;
mod keys;
pub mod knob;
mod mixer_panel;
mod note_panel;
mod pads;
mod params_ui;
mod perf;
mod pianoroll;
mod poly_edit;
mod polyroll;
mod rack_header;
mod rack_sync;
mod record;
mod roll_edit;
mod sampler_load;
mod sampler_panel;
mod sampler_ui;
mod scroll;
mod song_panel;
mod step303;
mod step_edit;
mod synth_panel;
pub mod theme;
mod transport;

use crate::audio::AudioLink;
use crate::document::Document;
use acid_engine::host::{Command, Event, Status};
use acid_engine::machine::{MachineType, MachineUid};
use eframe::egui;
use std::collections::HashMap;
use std::time::Duration;

/// Cached waveform for one slot: (pcm pointer, pcm length, columns, peaks).
pub type WaveCacheEntry = (usize, usize, usize, Vec<(f32, f32)>);

/// Per-machine UI state, kept by uid (`ViewState::panels`) and made on
/// first use. Fields a machine type doesn't use stay at their defaults.
#[derive(Default)]
pub struct PanelState {
    /// Bank shown on the machine's pads (0..8).
    pub bank: u8,
    /// Active 303 step-writer (synths).
    pub writer: Option<step_edit::StepWriter>,
    /// Scroll distance accumulated toward the next pitch notch (synths).
    pub scroll_accum: f32,
    /// Open pitch-scroll gesture in the 303 editor (synths).
    pub scroll_at: Option<scroll::ScrollGesture>,
    /// Mouse drag in progress in the piano roll (synths).
    pub roll_drag: Option<roll_edit::RollDrag>,
    /// The synth pattern as it was when that drag began; every frame of
    /// the drag is recomputed from it (see `roll_edit::drag_from_origin`).
    pub roll_origin: Option<Box<roll_edit::RollSnapshot>>,
    /// Poly-roll drag (note machines).
    pub poly_drag: Option<poly_edit::PolyDrag>,
    pub poly_snap: poly_edit::Snap,
    /// Length in ticks of the last note drawn or resized (0 = one step).
    pub poly_len: u16,
    /// The roll has been scrolled to its starting pitch once.
    pub poly_scrolled: bool,
    /// Selected sampler slot (0..16; samplers).
    pub sampler_slot: usize,
    /// Operator shown in the FM operator editor (0..6; FM).
    pub fm_op: usize,
    /// List the FM voice browser shows.
    pub fm_browser: fm_voices::FmBrowser,
    /// Where the loaded FM voice came from: (browser, index in that list).
    /// Set when a voice is loaded from the browser; cleared by a single-voice
    /// import, an undo/redo of a voice load, a bank change (for an imported
    /// row) and on document load. It is kept across knob edits (decision
    /// #11, see `fm_panel::fm_voice_browser`).
    pub fm_voice_src: Option<(fm_voices::FmBrowser, usize)>,
}

#[derive(Default)]
pub struct ViewState {
    pub audio_window: audio_settings::AudioWindow,
    /// Machine that keyboard shortcuts act on; None (or a uid no longer in
    /// the rack) means the first machine (see `SidAcidApp::focused`).
    pub focus: Option<MachineUid>,
    pub message: Option<String>,
    /// Per-machine panel state, by uid.
    pub panels: HashMap<MachineUid, PanelState>,
    /// Open wheel gesture on a knob (its undo step closes after a rest).
    pub knob_scroll: Option<(acid_engine::params::ParamId, scroll::ScrollGesture)>,
    /// Egui time until which the "audio fault recovered" label shows.
    pub fault_until: f64,
    /// Action waiting on the unsaved-changes question.
    pub confirm: Option<files::Pending>,
    pub quit: bool,
    pub clipboard: Option<files::Clip>,
    pub export_open: bool,
    pub export_bars: u32,
    pub export_song: bool,
    pub export_progress: Option<std::sync::Arc<std::sync::atomic::AtomicU32>>,
    pub seed_counter: u64,
    pub export_rx: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    pub last_title: String,
    /// Text to place on the OS clipboard so Ctrl+V reaches the app.
    pub clip_text: Option<String>,
    /// Knob whose gesture (drag, scroll, reset) is still open.
    pub touched_knob: Option<acid_engine::params::ParamId>,
    /// Knob the engine was told is held (`Command::Touch`), while recording.
    pub engine_touch: Option<acid_engine::params::ParamId>,
    /// Non-blocking progress text shown at the right of the transport.
    pub status_line: Option<String>,
    /// Save waiting for its "Saving…" status to show.
    pub saving: Option<files::PendingSave>,
    /// Save waiting on "replace the existing file?" (the name had the
    /// extension appended after the OS dialog, so it never asked).
    pub overwrite: Option<files::OverwriteAsk>,
    /// The audio stream reported a failure that has not been handled yet.
    pub audio_lost: bool,
    /// One automatic restart was tried since audio last ran fine.
    pub auto_retried: bool,
    /// Pattern or Song transport mode (not saved).
    pub mode: acid_engine::sequencer::Mode,
    /// Record armed (song mode only).
    pub record: bool,
    /// Song bar playback starts from, and the bar the song editor shows.
    pub song_cursor: usize,
    /// Shift-click end of the song bar selection.
    pub song_sel_end: Option<usize>,
    /// Bars copied in the song editor.
    pub song_clip: Option<Vec<acid_engine::project::Bar>>,
    /// Knob values heard while recording (saved knobs stay unchanged).
    pub live: std::collections::HashMap<acid_engine::params::ParamId, f32>,
    /// Last step recorded for the held knob.
    pub rec_last: Option<(acid_engine::params::ParamId, u32)>,
    pub was_playing: bool,
    /// `Command::Stop`s sent to the current engine (wrapping), matched
    /// against `Status::stops` to spot a status from before the last Stop.
    pub stops_sent: u32,
    /// Note held by a click on a roll's key strip: (machine, pitch).
    pub audition: Option<(MachineUid, u8)>,
    /// Tests only: collapsing headers forced open or shut, by id salt.
    #[cfg(test)]
    pub force_open: Vec<(&'static str, bool)>,
    /// Cached waveform peaks per (sampler, slot): (pcm pointer, pcm
    /// length, columns, peaks).
    pub wave_cache: HashMap<(MachineUid, usize), WaveCacheEntry>,
    /// Marker being dragged in a waveform: (sampler, slot, marker).
    pub wave_drag: Option<(MachineUid, usize, sampler_ui::Marker)>,
    /// Transient sensitivity for "Detect" (None = 0.5).
    pub sampler_sensitivity: Option<f32>,
    /// Key held by a shift-click on a slot pad: (sampler, key).
    pub pad_note: Option<(MachineUid, u8)>,
    pub perf: perf::PerfMeter,
    /// Machine whose name is being edited, and the text typed so far.
    pub renaming: Option<(MachineUid, String)>,
    /// Rack control clicked while the rack was drawn; applied after it.
    pub rack_action: Option<rack_header::RackAction>,
}

pub struct SidAcidApp {
    pub(crate) doc: Document,
    pub(crate) audio: Option<AudioLink>,
    pub(crate) audio_error: Option<String>,
    /// This machine's settings, and the file they load from and save to
    /// (None when headless or the environment gives no home for it).
    pub(crate) settings: crate::settings::Settings,
    pub(crate) settings_path: Option<std::path::PathBuf>,
    pub(crate) status: Status,
    pub(crate) pending: Vec<Command>,
    pub(crate) view: ViewState,
    /// The machine order the engine has, as far as the UI knows (see
    /// `rack_sync`).
    pub(crate) engine_rack: Vec<MachineUid>,
    /// The rack generation (`Status::rack_gen`) a status describing
    /// `engine_rack` carries; it mirrors the host's count (see `rack_sync`).
    pub(crate) rack_expected: u32,
    /// Tests only: the commands a frame flushed with no audio device (a
    /// real app drops them), so a test can see what would reach the engine.
    #[cfg(test)]
    pub(crate) flushed: Vec<Command>,
}

impl ViewState {
    /// `uid`'s panel state, made on first use.
    pub(crate) fn panel(&mut self, uid: MachineUid) -> &mut PanelState {
        self.panels.entry(uid).or_default()
    }

    /// A collapsing header's forced state: tests can force one open or
    /// shut by its id salt; the app never does (`None` lets the user's
    /// clicks rule).
    pub(crate) fn forced_open(&self, salt: &str) -> Option<bool> {
        #[cfg(test)]
        {
            self.force_open
                .iter()
                .find(|(s, _)| *s == salt)
                .map(|&(_, open)| open)
        }
        #[cfg(not(test))]
        {
            let _ = salt;
            None
        }
    }
}

/// Opens the stream `saved` asks for, falling back as `audio_select::choose`
/// describes. Scans only the hosts the choice can need.
pub(crate) fn open_audio(
    project: &acid_engine::project::Project,
    saved: &crate::settings::AudioSettings,
) -> Result<(AudioLink, Vec<String>), String> {
    use crate::{audio_scan, audio_select};
    let default = audio_scan::default_host_name();
    let default_choice = audio_select::Choice {
        driver: default.to_string(),
        device: None,
        sample_rate: None,
        buffer_frames: None,
    };
    if *saved == crate::settings::AudioSettings::default() {
        // Nothing saved: skip the device scan (ALSA's takes ~0.5 s) and
        // let the driver pick its defaults.
        return Ok((AudioLink::open(project, &default_choice)?, Vec::new()));
    }
    let available = cpal::available_hosts();
    let scan = |name: &str| {
        audio_scan::host_id(name)
            .filter(|id| available.contains(id))
            .and_then(audio_scan::scan_host)
    };
    // The saved driver's host first; the default host only if the saved
    // one cannot be used.
    let mut hosts: Vec<_> = saved.driver.as_deref().and_then(scan).into_iter().collect();
    let saved_usable = hosts
        .first()
        .is_some_and(|h| !(h.devices.is_empty() && h.default_device.is_none()));
    if !saved_usable && saved.driver.as_deref() != Some(default) {
        hosts.extend(scan(default));
    }
    let (choice, notes) = audio_select::choose(&hosts, default, saved)?;
    open_with_default_fallback(&choice, &default_choice, notes, |c| {
        AudioLink::open(project, c)
    })
}

/// Opens `choice`; if that fails and it is not already the plain default,
/// retries `default_choice` with a notice. If both fail, the first error.
fn open_with_default_fallback<T>(
    choice: &crate::audio_select::Choice,
    default_choice: &crate::audio_select::Choice,
    mut notes: Vec<String>,
    mut open: impl FnMut(&crate::audio_select::Choice) -> Result<T, String>,
) -> Result<(T, Vec<String>), String> {
    match open(choice) {
        Ok(v) => Ok((v, notes)),
        Err(e) if choice != default_choice => {
            let v = open(default_choice).map_err(|_| e.clone())?;
            let what = choice
                .device
                .clone()
                .unwrap_or_else(|| crate::audio_select::driver_label(&choice.driver).to_string());
            notes.push(format!(
                "Could not open '{what}' ({e}) — using the default device"
            ));
            Ok((v, notes))
        }
        Err(e) => Err(e),
    }
}

/// Two status notes shown together, the earlier first.
fn join_notices(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (a, b) => a.or(b),
    }
}

impl SidAcidApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self::headless(Document::new(acid_engine::demo::demo_project()));
        app.settings_path = crate::settings::settings_path();
        if let Some(path) = &app.settings_path {
            let (settings, notice) = crate::settings::load(path);
            app.settings = settings;
            app.view.status_line = notice;
        }
        let load_notice = app.view.status_line.take();
        app.start_audio();
        app.view.status_line = join_notices(load_notice, app.view.status_line.take());
        app
    }

    /// An app with no audio device (tests, or before audio starts).
    pub fn headless(doc: Document) -> Self {
        Self {
            engine_rack: doc.project.rack_uids(),
            doc,
            audio: None,
            audio_error: None,
            settings: crate::settings::Settings::default(),
            settings_path: None,
            status: Status::default(),
            pending: Vec::new(),
            view: ViewState::default(),
            rack_expected: 0,
            #[cfg(test)]
            flushed: Vec::new(),
        }
    }

    pub(crate) fn start_audio(&mut self) {
        self.reset_for_new_host();
        match open_audio(&self.doc.project, &self.settings.audio) {
            Ok((link, notes)) => {
                self.audio = Some(link);
                self.audio_error = None;
                if !notes.is_empty() {
                    self.view.status_line = Some(notes.join("; "));
                }
            }
            Err(e) => self.audio_error = Some(e),
        }
    }

    /// Drops the stream and everything sent toward it: whatever happens
    /// next, the next engine is a fresh one.
    pub(crate) fn reset_for_new_host(&mut self) {
        self.pending.clear();
        self.reset_transport_view();
        self.view.perf.restart();
        // Release the old stream before opening another: JACK and ASIO
        // allow one client stream per device.
        self.audio = None;
        self.rack_for_new_host();
    }

    /// Queues commands for the audio thread. Repeated values for the same
    /// parameter collapse into the latest one, but only within the trailing
    /// run of `SetParam`s so ordering against other commands is kept.
    pub(crate) fn send(&mut self, cmds: Vec<Command>) {
        for cmd in cmds {
            // Every load, clear, undo and redo of a slot's audio comes through
            // here: its cached waveform peaks are stale.
            if let &Command::SamplerSample { uid, slot, .. } = &cmd {
                self.view.wave_cache.remove(&(uid, slot as usize));
            }
            if matches!(cmd, Command::Stop) {
                self.view.stops_sent = self.view.stops_sent.wrapping_add(1);
            }
            if let &Command::SetParam(id, v) = &cmd {
                let run = self
                    .pending
                    .iter()
                    .rev()
                    .take_while(|c| matches!(c, Command::SetParam(..)))
                    .count();
                let start = self.pending.len() - run;
                if let Some(Command::SetParam(_, pending)) = self.pending[start..]
                    .iter_mut()
                    .find(|c| matches!(c, Command::SetParam(p, _) if *p == id))
                {
                    *pending = v;
                    continue;
                }
            }
            self.pending.push(cmd);
        }
    }

    /// Undo, keeping the browser highlight honest: undoing a voice load
    /// restores a voice that did not come from the stored row (and a name
    /// guard can't tell same-named rows apart), and undoing a bank import
    /// puts other voices under the stored index.
    pub(crate) fn undo(&mut self) {
        self.view.voice_source_before(self.doc.next_undo());
        let cmds = self.doc.undo();
        self.send(cmds);
        self.sync_rack();
    }

    /// Redo; see `undo`.
    pub(crate) fn redo(&mut self) {
        self.view.voice_source_before(self.doc.next_redo());
        let cmds = self.doc.redo();
        self.send(cmds);
        self.sync_rack();
    }

    /// Sends queued commands to the audio thread. Without audio, commands
    /// are dropped: the engine is rebuilt from the project when audio starts.
    fn flush_pending(&mut self) {
        let Some(audio) = self.audio.as_mut() else {
            #[cfg(test)]
            self.flushed.append(&mut self.pending);
            self.pending.clear();
            return;
        };
        send_in_order(&mut self.pending, &mut |cmd| audio.send(cmd));
    }

    /// Reads events, then sends queued commands.
    fn pump(&mut self, now: f64) {
        if self.audio.is_none() {
            self.pending.clear();
            return;
        }
        while let Some(event) = self.audio.as_mut().and_then(AudioLink::poll) {
            if let Event::Status(status) = event {
                self.on_status(status, now);
            }
            // Dispose events drop here, on the UI thread.
        }
        let failed = self.audio.as_ref().is_some_and(AudioLink::has_failed);
        self.flush_pending();
        if failed {
            self.on_audio_failed();
        }
    }

    /// A status from the audio thread: audio is running fine.
    fn on_status(&mut self, mut status: Status, now: f64) {
        // Sent before the engine took our latest Stop: it is stopped by
        // now, so the optimistic stop stands (no flicker back to playing).
        if status.stops != self.view.stops_sent {
            status.playing = false;
        }
        if status.guard_fired {
            self.view.fault_until = now + 3.0;
        }
        // UI edits are bounded and loaded songs validated, so a
        // refused song means the UI and engine disagree: a bug.
        debug_assert!(!status.song_rejected, "the engine refused a song");
        // Likewise for a rack: the UI's idea of the engine's rack is wrong.
        debug_assert!(!status.rack_rejected, "the engine refused a rack");
        // The status came from the host just dropped: nothing in it holds.
        if status.rack_rejected && self.reload_after_refused_rack(&status) {
            return;
        }
        let fresh = status.rack_gen == self.rack_expected;
        self.view
            .perf
            .on_status(&status, now, &self.engine_rack, fresh);
        self.gate_status(&mut status);
        self.status = status;
        self.view.auto_retried = false;
    }

    /// The audio stream failed: drop it and report it.
    fn on_audio_failed(&mut self) {
        self.audio = None;
        self.status = Status::default();
        self.audio_error = Some("The audio device stopped.".into());
        self.view.audio_lost = true;
    }

    /// Takes a reported stream failure. True when audio should restart by
    /// itself: once, until a status shows audio running again.
    fn take_audio_retry(&mut self) -> bool {
        if !std::mem::take(&mut self.view.audio_lost) || self.view.auto_retried {
            return false;
        }
        self.view.auto_retried = true;
        true
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        self.pump(ctx.input(|i| i.time));
        self.run_queued_save();
        if self.take_audio_retry() {
            self.start_audio();
        }
        self.handle_step_write(ctx);
        self.handle_shortcuts(ctx);
        self.record_automation();
        if self.view.was_playing && !self.status.playing && self.view.record {
            self.stop_recording();
        }
        self.view.was_playing = self.status.playing;
        if let Some(t) = self.view.clip_text.take() {
            ctx.copy_text(t);
        }
        self.poll_export();
        if ctx.input(|i| i.viewport().close_requested()) && self.doc.is_dirty() && !self.view.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.view.confirm = Some(files::Pending::Quit);
        }
        if self.view.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let title = self.window_title();
        if title != self.view.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.view.last_title = title;
        }
        egui::TopBottomPanel::top("transport").show(ctx, |ui| self.transport(ui));
        if let Some(err) = self.audio_error.clone() {
            egui::TopBottomPanel::top("audio_banner").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(theme::ACCENT, format!("No audio: {err}"));
                    if ui.button("Retry audio").clicked() {
                        self.start_audio();
                    }
                });
            });
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| self.rack(ui));
        });
        self.dialogs(ctx);
        self.audio_settings_window(ctx);
        // A collapsed panel stops drawing its roll, so a key or drag held
        // when it collapsed would never see its release.
        self.release_stale_poly_input(ctx.input(|i| i.pointer.any_down()));
        self.tick_queued_save(ctx);
        if let Some(left) = self.tick_scroll_gestures(ctx.input(|i| i.time)) {
            // Keep frames coming so the gesture ends without more input.
            ctx.request_repaint_after(Duration::from_secs_f64(left));
        }
        self.flush_pending();
        if self.audio.is_some() {
            let ms = if self.status.playing { 16 } else { 150 };
            ctx.request_repaint_after(Duration::from_millis(ms));
        } else if self.view.export_rx.is_some() {
            // Keep the export progress moving even without audio.
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }

    /// The machines whose panels the rack draws, in rack order.
    fn rack_panels(&self) -> Vec<(MachineUid, MachineType)> {
        let machines = &self.doc.project.machines;
        machines.iter().map(|m| (m.uid, m.machine_type())).collect()
    }

    fn rack(&mut self, ui: &mut egui::Ui) {
        for (uid, ty) in self.rack_panels() {
            match ty {
                MachineType::Synth => self.synth_panel(ui, uid),
                MachineType::Kit808 | MachineType::Kit909 => self.drum_panel(ui, uid),
                MachineType::Fm => self.fm_panel(ui, uid),
                MachineType::Sampler => self.sampler_panel(ui, uid),
            }
            ui.add_space(6.0);
        }
        self.add_bar(ui);
        ui.add_space(6.0);
        self.mixer_panel(ui);
        self.song_panel(ui);
        self.rack_action_after_draw();
    }

    /// The name a machine's panel header shows (upper-cased there).
    pub(crate) fn panel_title(&self, uid: MachineUid) -> String {
        self.doc
            .project
            .machine(uid)
            .map_or_else(String::new, |m| m.name.clone())
    }

    /// The panel titles the rack draws, in order.
    #[cfg(test)]
    pub(crate) fn rack_panel_titles(&self) -> Vec<String> {
        self.rack_panels()
            .into_iter()
            .map(|(uid, _)| self.panel_title(uid))
            .collect()
    }

    /// The machine keyboard shortcuts act on: the focused one while it is
    /// in the rack, else the first machine.
    pub(crate) fn focused(&self) -> Option<MachineUid> {
        let p = &self.doc.project;
        self.view
            .focus
            .filter(|&uid| p.machine(uid).is_some())
            .or_else(|| p.machines.first().map(|m| m.uid))
    }

    /// `uid`'s selected pattern (0 for a machine not in the rack).
    pub(crate) fn selected(&self, uid: MachineUid) -> u8 {
        self.doc.project.machine(uid).map_or(0, |m| m.selected)
    }

    /// `uid`'s playhead step while it plays pattern `slot`. `Status`
    /// reports by rack position.
    pub(crate) fn playhead(&self, uid: MachineUid, slot: u8) -> Option<usize> {
        let i = self.doc.project.position(uid)?;
        (self.status.playing && self.status.patterns[i] == slot).then_some(self.status.playhead[i])
    }
}

impl eframe::App for SidAcidApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}

/// Sends commands from the front of `pending` until `send` refuses one;
/// that one and everything after it stay queued, in order.
fn send_in_order(
    pending: &mut Vec<Command>,
    send: &mut impl FnMut(Command) -> Result<(), Command>,
) {
    let mut unsent = Vec::new();
    for cmd in pending.drain(..) {
        if !unsent.is_empty() {
            unsent.push(cmd);
        } else if let Err(cmd) = send(cmd) {
            unsent.push(cmd);
        }
    }
    *pending = unsent;
}

#[cfg(test)]
pub(crate) fn run_frames(app: &mut SidAcidApp, frames: usize) {
    run_frames_in(&egui::Context::default(), app, frames);
}

/// Like `run_frames`, on a caller-kept `Context`; returns the last frame's
/// shape count (a rough measure of how much was drawn).
#[cfg(test)]
pub(crate) fn run_frames_in(ctx: &egui::Context, app: &mut SidAcidApp, frames: usize) -> usize {
    let mut shapes = 0;
    for _ in 0..frames {
        shapes = ctx
            .run(egui::RawInput::default(), |ctx| app.ui(ctx))
            .shapes
            .len();
    }
    shapes
}

#[cfg(test)]
mod tests {
    use crate::test_rack::*;
    use acid_engine::params::MachineParam;

    /// `f` of each listed machine's panel state, in order.
    fn panels<T>(
        app: &mut SidAcidApp,
        uids: &[MachineUid],
        f: impl Fn(&PanelState) -> T,
    ) -> Vec<T> {
        uids.iter().map(|&uid| f(app.view.panel(uid))).collect()
    }

    #[test]
    fn the_rack_draws_one_panel_per_machine_in_order() {
        let mut p = acid_engine::project::Project::default();
        p.machines.swap(0, 5);
        let app = SidAcidApp::headless(Document::new(p));
        let order: Vec<String> = app.rack_panel_titles();
        assert_eq!(order, ["Sampler", "Synth 2", "808", "909", "FM", "Synth 1"]);
    }

    #[test]
    fn a_reordered_rack_draws_and_reads_status_by_rack_position() {
        let mut p = acid_engine::project::Project::default();
        p.machines.swap(0, 5);
        let mut app = SidAcidApp::headless(Document::new(p));
        app.status.playing = true;
        // Position 0 is now the sampler, position 5 Synth 1.
        app.status.patterns[0] = 3;
        app.status.playhead[0] = 7;
        app.status.playhead[5] = 2;
        assert_eq!(app.playhead(SAMPLER, 3), Some(7));
        assert_eq!(app.playhead(SAMPLER, 0), None, "another pattern plays");
        assert_eq!(app.playhead(SYNTH1, 0), Some(2));
        assert_eq!(app.focused(), Some(SAMPLER), "the first machine");
        run_frames(&mut app, 2);
        assert!(!app.doc.can_undo(), "drawing changes nothing");
    }

    #[test]
    fn a_failed_open_retries_the_default_with_a_notice() {
        use crate::audio_select::Choice;
        let c = |dev: Option<&str>| Choice {
            driver: "ALSA".into(),
            device: dev.map(Into::into),
            sample_rate: None,
            buffer_frames: None,
        };
        let (default, busy) = (c(None), c(Some("hw:1")));
        let mut tried = Vec::new();
        let r = open_with_default_fallback(&busy, &default, vec![], |ch| {
            tried.push(ch.clone());
            if ch.device.is_some() {
                Err("busy".to_string())
            } else {
                Ok(7)
            }
        });
        assert_eq!(
            r,
            Ok((
                7,
                vec!["Could not open 'hw:1' (busy) — using the default device".to_string()]
            ))
        );
        assert_eq!(tried, [busy.clone(), default.clone()]);
        // Both fail: the original error.
        let r: Result<(u8, _), _> = open_with_default_fallback(&busy, &default, vec![], |ch| {
            Err(if ch.device.is_some() {
                "first"
            } else {
                "second"
            }
            .to_string())
        });
        assert_eq!(r, Err("first".to_string()));
        // The default itself failing is not retried.
        let mut n = 0;
        let r: Result<(u8, _), _> = open_with_default_fallback(&default, &default, vec![], |_| {
            n += 1;
            Err("x".to_string())
        });
        assert_eq!((r, n), (Err("x".to_string()), 1));
    }

    #[test]
    fn notices_join_with_the_earlier_first() {
        use super::join_notices as j;
        let s = |t: &str| Some(t.to_string());
        assert_eq!(j(s("a"), s("b")), s("a; b"));
        assert_eq!(j(s("a"), None), s("a"));
        assert_eq!(j(None, s("b")), s("b"));
        assert_eq!(j(None, None), None);
    }

    use super::*;
    use crate::document::Edit;
    use acid_engine::project::Project;

    #[test]
    fn dice_and_drum_buttons_are_single_undo_steps() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let before = app.doc.project.clone();
        app.randomise_synth(SYNTH1, acid_engine::random::Dice::ALL);
        assert_ne!(
            app.doc.project.synth(SYNTH1).unwrap().patterns[0],
            before.synth(SYNTH1).unwrap().patterns[0]
        );
        app.doc.undo();
        assert_eq!(app.doc.project, before);
        app.drum_template(KIT909);
        assert_ne!(
            app.doc.project.kit(KIT909).unwrap().patterns[0],
            before.kit(KIT909).unwrap().patterns[0]
        );
        app.doc.undo();
        // Evolve leaves some seeds' patterns as they were: use one that changes it.
        let groove = &before.kit(KIT808).unwrap().patterns
            [before.machine(KIT808).unwrap().selected as usize];
        let seed = (0..100)
            .find(|&s| acid_engine::random::evolve_drums(groove, s) != *groove)
            .expect("some seed evolves the groove");
        app.evolve_drums_seeded(KIT808, seed);
        assert_ne!(app.doc.project, before, "evolve changed the pattern");
        app.doc.undo();
        assert_eq!(app.doc.project, before);
        assert!(!app.doc.can_undo(), "each button was one undo step");
    }

    #[test]
    fn dice_pair_each_synth_with_its_drum_kits_selected_groove() {
        use acid_engine::random::Dice;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.doc.select_pattern(KIT909, 3);
        let p = app.doc.project.clone();
        let rest = Dice {
            rest: true,
            ..Dice::default()
        };
        let slot = p.machine(SYNTH2).unwrap().selected as usize;
        let expected = acid_engine::random::randomise_synth(
            &p.synth(SYNTH2).unwrap().patterns[slot],
            rest,
            &p.kit(KIT909).unwrap().patterns[3],
            42,
        );
        let other_groove = acid_engine::random::randomise_synth(
            &p.synth(SYNTH2).unwrap().patterns[slot],
            rest,
            &p.kit(KIT808).unwrap().patterns[p.machine(KIT808).unwrap().selected as usize],
            42,
        );
        assert_ne!(expected, other_groove, "the groove matters for this seed");
        app.randomise_synth_seeded(SYNTH2, rest, 42);
        assert_eq!(
            app.doc.project.synth(SYNTH2).unwrap().patterns[slot],
            expected
        );
    }

    #[test]
    fn guard_status_shows_the_fault_for_three_seconds() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.auto_retried = true;
        let status = Status {
            guard_fired: true,
            playing: true,
            ..Status::default()
        };
        app.on_status(status, 10.0);
        assert_eq!(app.view.fault_until, 13.0);
        assert!(
            !app.view.auto_retried,
            "audio ran fine: a later failure may retry"
        );
        assert_eq!(app.status, status);
        app.on_status(Status::default(), 11.0);
        assert_eq!(
            app.view.fault_until, 13.0,
            "a clean status leaves the fault shown"
        );
    }

    #[test]
    fn a_lost_stream_retries_once_until_audio_runs_again() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.status.playing = true;
        app.on_audio_failed();
        assert_eq!(app.status, Status::default());
        assert_eq!(
            app.audio_error.as_deref(),
            Some("The audio device stopped.")
        );
        assert!(app.audio.is_none());
        assert!(app.take_audio_retry(), "the first failure retries");
        assert!(!app.view.audio_lost);
        assert!(!app.take_audio_retry(), "nothing new to handle");
        app.on_audio_failed();
        assert!(!app.take_audio_retry(), "a second failure does not retry");
        app.on_status(Status::default(), 0.0);
        app.on_audio_failed();
        assert!(app.take_audio_retry(), "audio ran in between: retry again");
    }

    #[test]
    fn a_full_ring_keeps_the_unsent_commands_in_order() {
        let mut pending = vec![
            Command::Play,
            Command::SetBpm(1.0),
            Command::SetBpm(2.0),
            Command::Stop,
        ];
        let mut sent = Vec::new();
        let mut room = 2;
        let mut accept = |c: Command| {
            if room == 0 {
                return Err(c);
            }
            room -= 1;
            sent.push(c);
            Ok(())
        };
        send_in_order(&mut pending, &mut accept);
        assert!(matches!(
            sent.as_slice(),
            [Command::Play, Command::SetBpm(_)]
        ));
        assert!(matches!(
            pending.as_slice(),
            [Command::SetBpm(v), Command::Stop] if *v == 2.0
        ));
        // Room again: everything left goes, still in order, and nothing
        // jumps the queue past a refused command.
        let mut order = Vec::new();
        let mut refuse_first = true;
        let mut pending2 = vec![Command::Play, Command::Stop];
        send_in_order(&mut pending2, &mut |c: Command| {
            if std::mem::take(&mut refuse_first) {
                return Err(c);
            }
            order.push(c);
            Ok(())
        });
        assert!(order.is_empty(), "nothing after a refusal is sent");
        assert_eq!(pending2.len(), 2);
    }

    #[test]
    fn seeds_differ_between_clicks() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        assert_ne!(app.next_seed(), app.next_seed());
    }

    fn recording_app() -> SidAcidApp {
        let mut doc = Document::new(acid_engine::demo::demo_project());
        doc.project.song.bars = vec![acid_engine::project::Bar::default(); 4];
        let mut app = SidAcidApp::headless(doc);
        app.view.mode = acid_engine::sequencer::Mode::Song;
        app.view.record = true;
        app.status.playing = true;
        app
    }

    #[test]
    fn recording_writes_held_knob_values_as_one_undo_step() {
        use acid_engine::params::SynthParam;
        let id = SYNTH1.param(SynthParam::Cutoff);
        let mut app = recording_app();
        app.view.touched_knob = Some(id);
        app.view.live.insert(id, 0.7);
        app.status.song_step = 4;
        app.record_automation();
        app.view.live.insert(id, 0.9);
        app.status.song_step = 7;
        app.record_automation();
        let lane = acid_engine::song::lane(&app.doc.project.song, id).unwrap();
        let points: Vec<(u32, f32)> = lane.points.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(points, vec![(4, 0.7), (5, 0.9), (6, 0.9), (7, 0.9)]);
        assert_eq!(
            app.doc.project.param(id),
            acid_engine::demo::demo_project().param(id),
            "saved knob untouched"
        );
        app.stop_recording();
        app.doc.undo();
        assert!(app.doc.project.song.automation.is_empty());
    }

    /// The Touch commands queued so far, in order.
    fn touches(app: &SidAcidApp) -> Vec<bool> {
        app.pending
            .iter()
            .filter_map(|c| match c {
                Command::Touch(_, on) => Some(*on),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn switch_and_tempo_edits_join_the_record_pass_undo_step() {
        use acid_engine::params::SynthParam;
        let id = SYNTH1.param(SynthParam::Cutoff);
        let mut app = recording_app();
        let before = app.doc.project.clone();
        app.view.touched_knob = Some(id);
        app.view.live.insert(id, 0.7);
        app.status.song_step = 4;
        app.record_automation();
        app.edit_switch(SYNTH1.param(SynthParam::Wave), 1.0);
        app.edit_transport(Edit::Bpm(150.0));
        app.edit_transport(Edit::Swing(0.3));
        app.status.song_step = 6;
        app.record_automation();
        app.stop_recording();
        assert_eq!(app.doc.project.bpm, 150.0);
        app.doc.undo();
        assert_eq!(app.doc.project, before, "the whole pass is one undo step");
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn outside_a_record_pass_switches_and_tempo_are_their_own_steps() {
        use acid_engine::params::SynthParam;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.edit_switch(SYNTH1.param(SynthParam::Wave), 1.0);
        app.edit_transport(Edit::Bpm(150.0));
        app.edit_transport(Edit::Bpm(151.0));
        app.doc.end_gesture();
        app.doc.undo();
        assert_eq!(app.doc.project.bpm, 128.0, "a tempo drag is one step");
        assert_eq!(app.doc.project.param(SYNTH1.param(SynthParam::Wave)), 1.0);
        app.doc.undo();
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn stopping_a_pass_releases_a_held_knob_once_and_rearming_touches_it_again() {
        use acid_engine::params::SynthParam;
        let id = SYNTH1.param(SynthParam::Cutoff);
        let held = || knob::KnobOutput {
            value: None,
            released: false,
            touched: true,
            scrolled: false,
        };
        let mut app = recording_app();
        app.knob_gesture(id, held(), true, 0.0);
        app.stop_recording();
        assert_eq!(touches(&app), [true, false]);
        app.view.record = false;
        app.knob_gesture(id, held(), false, 0.1);
        app.release_knob(id);
        assert_eq!(touches(&app), [true, false], "no second release");

        app.pending.clear();
        app.view.record = true;
        app.knob_gesture(id, held(), true, 0.2);
        app.stop_recording();
        app.view.record = true;
        app.knob_gesture(id, held(), true, 0.3);
        assert_eq!(
            touches(&app),
            [true, false, true],
            "still held when recording resumes: touched again"
        );
        let mut idle = held();
        idle.touched = false;
        app.knob_gesture(id, idle, true, 0.4);
        assert_eq!(touches(&app), [true, false, true, false]);
        assert!(app.view.engine_touch.is_none());
    }

    #[test]
    fn stop_twice_without_a_status_in_between_returns_to_the_start() {
        use acid_engine::params::SynthParam;
        let id = SYNTH1.param(SynthParam::Cutoff);
        let mut app = recording_app();
        app.doc.project.song.loop_range = Some((1, 2));
        app.view.song_cursor = 3;
        app.stop();
        assert!(
            !app.status.playing,
            "assumed stopped until a status says otherwise"
        );
        app.view.touched_knob = Some(id);
        app.view.live.insert(id, 0.7);
        app.status.song_step = 5;
        app.record_automation();
        assert!(
            app.doc.project.song.automation.is_empty(),
            "nothing recorded after Stop"
        );
        app.stop();
        assert_eq!(
            app.view.song_cursor, 1,
            "the second stop returns to the start"
        );
    }

    #[test]
    fn a_status_from_before_a_stop_does_not_flip_the_transport_back() {
        let mut app = recording_app();
        app.stop();
        // Already in the ring before the engine took the Stop.
        app.on_status(
            Status {
                playing: true,
                ..Status::default()
            },
            0.0,
        );
        assert!(!app.status.playing, "a stale status");
        let caught_up = |playing| Status {
            playing,
            stops: 1,
            ..Status::default()
        };
        app.on_status(caught_up(false), 0.0);
        app.play();
        app.on_status(caught_up(true), 0.0);
        assert!(app.status.playing, "Play after the Stop is heard");
        app.set_mode(acid_engine::sequencer::Mode::Pattern);
        app.on_status(caught_up(true), 0.0);
        assert!(!app.status.playing, "a mode switch's Stop counts too");
        app.load_document(Document::new(Project::default()));
        app.on_status(caught_up(true), 0.0);
        assert!(!app.status.playing, "from the old engine");
        app.on_status(
            Status {
                playing: true,
                ..Status::default()
            },
            0.0,
        );
        assert!(app.status.playing, "a fresh engine has taken no Stop");
    }

    #[test]
    fn switching_mode_while_stopped_sends_no_stop() {
        let mut app = recording_app();
        app.status.playing = false;
        app.pending.clear();
        app.set_mode(acid_engine::sequencer::Mode::Pattern);
        assert!(matches!(
            app.pending.as_slice(),
            [Command::SetMode(acid_engine::sequencer::Mode::Pattern)]
        ));
        let mut app = recording_app();
        app.pending.clear();
        app.set_mode(acid_engine::sequencer::Mode::Pattern);
        assert!(!app.status.playing, "switching while playing stops");
        assert!(matches!(
            app.pending.as_slice(),
            [
                Command::Stop,
                Command::SetMode(acid_engine::sequencer::Mode::Pattern)
            ]
        ));
    }

    #[test]
    fn recording_the_same_value_at_the_same_step_is_not_an_edit() {
        use acid_engine::params::SynthParam;
        let id = SYNTH1.param(SynthParam::Cutoff);
        let mut app = recording_app();
        app.view.touched_knob = Some(id);
        app.view.live.insert(id, 0.5);
        app.status.song_step = 4;
        app.record_automation();
        app.doc.mark_saved();
        app.pending.clear();
        app.record_automation();
        assert!(app.pending.is_empty(), "no song sent again");
        assert!(!app.doc.is_dirty(), "no edit applied");
        assert_eq!(app.view.rec_last, Some((id, 4)));
    }

    #[test]
    fn a_record_write_cut_at_the_song_cap_says_so() {
        use acid_engine::params::SynthParam;
        use acid_engine::song::MAX_SONG_STEPS;
        let id = SYNTH1.param(SynthParam::Cutoff);
        let mut app = recording_app();
        app.view.touched_knob = Some(id);
        app.view.live.insert(id, 0.5);
        app.status.song_step = 4;
        app.record_automation();
        assert!(app.view.message.is_none(), "a normal write is quiet");
        // a bogus position far past the longest song
        app.status.song_step = MAX_SONG_STEPS as u32 + 100;
        app.record_automation();
        let lane = acid_engine::song::lane(&app.doc.project.song, id).unwrap();
        assert_eq!(
            lane.points.len(),
            MAX_SONG_STEPS + 1,
            "the cap from step 5 on"
        );
        assert!(app.view.message.is_some(), "the cut is reported");
    }

    #[test]
    fn nothing_is_recorded_unless_armed_in_song_mode_and_playing() {
        use acid_engine::params::SynthParam;
        let id = SYNTH1.param(SynthParam::Cutoff);
        for case in 0..3 {
            let mut app = recording_app();
            match case {
                0 => app.view.record = false,
                1 => app.view.mode = acid_engine::sequencer::Mode::Pattern,
                _ => app.status.playing = false,
            }
            app.view.touched_knob = Some(id);
            app.view.live.insert(id, 0.7);
            app.record_automation();
            assert!(app.doc.project.song.automation.is_empty(), "case {case}");
        }
    }

    #[test]
    fn recorded_pad_change_fills_following_bars() {
        let mut app = recording_app();
        app.status.song_bar = 1;
        app.record_pattern_change(SYNTH1, 9);
        let refs: Vec<_> = app
            .doc
            .project
            .song
            .bars
            .iter()
            .map(|b| b.cell(MachineUid(1)).pattern.map(|r| r.0))
            .collect();
        assert_eq!(refs, vec![None, Some(9), Some(9), Some(9)]);
    }

    #[test]
    fn switching_mode_stops_and_disarms() {
        let mut app = recording_app();
        app.pending.clear();
        app.set_mode(acid_engine::sequencer::Mode::Pattern);
        assert!(!app.view.record);
        assert!(matches!(
            app.pending.as_slice(),
            [
                Command::Stop,
                Command::SetMode(acid_engine::sequencer::Mode::Pattern)
            ]
        ));
    }

    #[test]
    fn stop_twice_returns_to_the_loop_start() {
        let mut app = recording_app();
        app.doc.project.song.loop_range = Some((1, 2));
        app.status.playing = false;
        app.view.song_cursor = 3;
        app.stop();
        assert_eq!(app.view.song_cursor, 1);
        app.doc.project.song.loop_range = None;
        app.stop();
        assert_eq!(app.view.song_cursor, 0);
    }

    #[test]
    fn loading_a_document_returns_the_transport_to_pattern_mode() {
        let mut app = recording_app();
        app.view.was_playing = true;
        app.view.touched_knob = Some(acid_engine::params::ParamId::Master(
            acid_engine::params::MasterParam::Level,
        ));
        app.view.rec_last = Some((
            acid_engine::params::ParamId::Master(acid_engine::params::MasterParam::Level),
            3,
        ));
        app.view.engine_touch = app.view.touched_knob;
        app.load_document(Document::new(acid_engine::project::Project::default()));
        assert_eq!(app.view.mode, acid_engine::sequencer::Mode::Pattern);
        assert!(!app.view.record);
        assert!(app.view.touched_knob.is_none() && app.view.rec_last.is_none());
        assert!(app.view.engine_touch.is_none());
        assert!(!app.view.was_playing);
    }

    #[test]
    fn transport_reset_closes_an_open_record_pass() {
        let mut app = recording_app();
        let mut song = app.doc.project.song.clone();
        song.bars.pop();
        app.apply_song(song.clone(), true);
        app.view.live.insert(
            acid_engine::params::ParamId::Master(acid_engine::params::MasterParam::Level),
            0.5,
        );
        app.reset_transport_view();
        assert!(app.view.live.is_empty());
        assert_eq!(app.view.mode, acid_engine::sequencer::Mode::Pattern);
        song.bars.pop();
        app.apply_song(song, true);
        app.doc.undo();
        assert_eq!(app.doc.project.song.bars.len(), 3, "separate undo steps");
    }

    #[test]
    fn song_play_clamps_the_start_bar_to_the_song() {
        let mut app = recording_app();
        app.status.playing = false;
        app.view.song_cursor = 9;
        app.pending.clear();
        app.play();
        assert_eq!(app.view.song_cursor, 3);
        assert!(matches!(
            app.pending.as_slice(),
            [Command::SetSongStart(3), Command::Play]
        ));
    }

    #[test]
    fn song_play_sends_the_start_bar_first() {
        let mut app = recording_app();
        app.status.playing = false;
        app.view.song_cursor = 2;
        app.pending.clear();
        app.play();
        assert!(matches!(
            app.pending.as_slice(),
            [Command::SetSongStart(2), Command::Play]
        ));
    }

    #[test]
    fn piano_roll_view_draws_headless() {
        let mut doc = Document::new(acid_engine::demo::demo_project());
        doc.project.synth_mut(SYNTH1).unwrap().view = acid_engine::project::SeqView::PianoRoll;
        let mut app = SidAcidApp::headless(doc);
        run_frames(&mut app, 2);
    }

    #[test]
    fn note_panels_and_poly_rolls_draw_headless() {
        use acid_engine::project::Note;
        let mut doc = Document::new(Project::default());
        doc.project.fm_mut(FM).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 8,
                pitch: 60,
                vel: 100,
            });
        doc.project.sampler_mut(SAMPLER).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 4,
                len: 300,
                pitch: 36,
                vel: 20,
            });
        doc.project.sampler_mut(SAMPLER).unwrap().patterns[0].length = 4;
        let mut app = SidAcidApp::headless(doc);
        run_frames(&mut app, 3);
        assert_eq!(
            panels(&mut app, &[FM, SAMPLER], |p| p.poly_scrolled),
            [true; 2]
        );
    }

    #[test]
    fn loading_a_document_resets_poly_roll_state() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        for uid in [FM, SAMPLER] {
            app.view.panel(uid).poly_drag = Some(poly_edit::PolyDrag::Velocity);
            app.view.panel(uid).poly_scrolled = true;
        }
        app.view.audition = Some((FM, 60));
        app.load_document(Document::new(Project::default()));
        assert_eq!(panels(&mut app, &[FM, SAMPLER], |p| p.poly_drag), [None; 2]);
        assert_eq!(
            panels(&mut app, &[FM, SAMPLER], |p| p.poly_scrolled),
            [false; 2]
        );
        assert_eq!(app.view.audition, None);
    }

    #[test]
    fn ui_draws_headless() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        run_frames(&mut app, 3);
        assert!(app.audio.is_none());
    }

    #[test]
    fn commands_without_audio_are_dropped_not_queued_forever() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.send(vec![Command::Play, Command::Stop]);
        run_frames(&mut app, 1);
        assert!(app.pending.is_empty());
    }

    #[test]
    fn repeated_param_commands_coalesce() {
        use acid_engine::params::SynthParam;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let id = SYNTH1.param(SynthParam::Cutoff);
        app.send(vec![
            Command::SetParam(id, 0.1),
            Command::SetParam(id, 0.2),
            Command::SetParam(id, 0.3),
        ]);
        assert_eq!(app.pending.len(), 1);
        assert!(matches!(app.pending[0], Command::SetParam(_, v) if v == 0.3));
    }

    #[test]
    fn copy_paste_between_synths_is_undoable() {
        use acid_engine::project::Gate;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.focus = Some(SYNTH1);
        app.copy_pattern();
        app.view.focus = Some(SYNTH2);
        app.doc.project.machine_mut(SYNTH2).unwrap().selected = 9;
        app.paste_pattern();
        assert_eq!(
            app.doc.project.synth(SYNTH2).unwrap().patterns[9],
            app.doc.project.synth(SYNTH1).unwrap().patterns[0]
        );
        let cmds = app.doc.undo();
        app.send(cmds);
        assert!(
            app.doc.project.synth(SYNTH2).unwrap().patterns[9]
                .steps
                .iter()
                .all(|s| s.gate == Gate::Off)
        );
    }

    #[test]
    fn note_patterns_copy_between_fm_and_sampler_but_not_onto_synths() {
        use acid_engine::project::Note;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.doc.project.fm_mut(FM).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 4,
                pitch: 60,
                vel: 100,
            });
        app.view.focus = Some(FM);
        app.copy_pattern();
        app.view.focus = Some(SAMPLER);
        app.paste_pattern();
        assert_eq!(
            app.doc.project.sampler(SAMPLER).unwrap().patterns[0]
                .notes
                .len(),
            1
        );
        app.doc.undo();
        assert!(
            app.doc.project.sampler(SAMPLER).unwrap().patterns[0]
                .notes
                .is_empty()
        );
        app.view.focus = Some(SYNTH1);
        assert!(app.view.message.is_none());
        let before = app.doc.project.clone();
        app.paste_pattern();
        assert!(app.view.message.is_some(), "refused onto a 303 pattern");
        assert_eq!(app.doc.project, before, "and nothing changed");
    }

    #[test]
    fn pasting_drums_onto_a_synth_is_refused() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.focus = Some(KIT808);
        app.copy_pattern();
        app.view.focus = Some(SYNTH1);
        let before = app.doc.project.clone();
        app.paste_pattern();
        assert_eq!(app.doc.project, before);
        assert!(app.view.message.is_some());
    }

    #[test]
    fn title_shows_name_and_dirty_mark() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        assert_eq!(app.window_title(), "Demo — SidAcid");
        app.doc.mark_dirty();
        app.doc.path = Some(std::path::PathBuf::from("/tmp/acid line.sidacid"));
        assert_eq!(app.window_title(), "acid line* — SidAcid");
    }

    #[test]
    fn new_with_unsaved_changes_asks_first() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.doc.mark_dirty();
        app.request(files::Pending::New);
        assert!(app.view.confirm.is_some());
        assert_eq!(app.doc.project.name, "Demo");
        app.view.confirm = None;
        app.doc.mark_saved();
        app.request(files::Pending::New);
        assert_eq!(app.doc.project.name, "Untitled");
    }

    #[test]
    fn dont_save_keeps_the_document_dirty_and_quit_closes_once() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.doc.mark_dirty();
        app.view.confirm = Some(files::Pending::Quit);
        app.dont_save(files::Pending::Quit);
        assert!(app.doc.is_dirty());
        assert!(app.view.confirm.is_none());
        assert!(app.view.quit);
    }

    #[test]
    fn copy_sets_clipboard_text() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.copy_pattern();
        assert!(
            app.view
                .clip_text
                .as_deref()
                .is_some_and(|t| t.starts_with("SidAcid pattern "))
        );
    }

    #[test]
    fn coalescing_stops_at_a_non_param_command() {
        use acid_engine::params::SynthParam;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let id = SYNTH1.param(SynthParam::Cutoff);
        app.send(vec![
            Command::SetParam(id, 0.1),
            Command::Play,
            Command::SetParam(id, 0.2),
        ]);
        assert_eq!(app.pending.len(), 3);
        assert!(matches!(app.pending[0], Command::SetParam(_, v) if v == 0.1));
        assert!(matches!(app.pending[1], Command::Play));
        assert!(matches!(app.pending[2], Command::SetParam(_, v) if v == 0.2));
    }

    #[test]
    fn plain_redraw_does_not_rewrite_swing_or_bpm() {
        let mut doc = Document::new(acid_engine::demo::demo_project());
        doc.project.swing = 0.153;
        doc.project.bpm = 133.37;
        let mut app = SidAcidApp::headless(doc);
        run_frames(&mut app, 3);
        assert!(!app.doc.is_dirty() && !app.doc.can_undo());
        assert_eq!(app.doc.project.swing, 0.153);
        assert_eq!(app.doc.project.bpm, 133.37);
    }

    #[test]
    fn loading_a_document_resyncs_banks_and_clears_gestures() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.touched_knob = Some(acid_engine::params::ParamId::Master(
            acid_engine::params::MasterParam::Level,
        ));
        let open = scroll::ScrollGesture {
            last: 0.0,
            serial: Some(0),
        };
        for uid in [SYNTH1, SYNTH2] {
            app.view.panel(uid).scroll_at = Some(open);
            app.view.panel(uid).scroll_accum = 12.0;
        }
        app.view.knob_scroll = Some((
            acid_engine::params::ParamId::Master(acid_engine::params::MasterParam::Level),
            open,
        ));
        let mut project = acid_engine::project::Project::default();
        for (m, s) in [17, 33, 0, 127, 0, 0].into_iter().enumerate() {
            project.machines[m].selected = s;
        }
        app.load_document(Document::new(project));
        let all = [SYNTH1, SYNTH2, KIT808, KIT909, FM, SAMPLER];
        assert_eq!(panels(&mut app, &all, |p| p.bank), [1, 2, 0, 7, 0, 0]);
        assert!(app.view.touched_knob.is_none());
        let synths = [SYNTH1, SYNTH2];
        assert_eq!(panels(&mut app, &synths, |p| p.scroll_at), [None; 2]);
        assert!(app.view.knob_scroll.is_none());
        assert_eq!(panels(&mut app, &synths, |p| p.scroll_accum), [0.0; 2]);
    }

    #[test]
    fn step_writer_is_dropped_when_its_synth_leaves_step_view() {
        let mut doc = Document::new(acid_engine::demo::demo_project());
        doc.project.synth_mut(SYNTH2).unwrap().view = acid_engine::project::SeqView::PianoRoll;
        let mut app = SidAcidApp::headless(doc);
        app.view.panel(SYNTH2).writer = Some(step_edit::StepWriter::new(0));
        run_frames(&mut app, 1);
        assert!(app.view.panel(SYNTH2).writer.is_none());
    }

    #[test]
    fn song_toolbar_actions_edit_the_song_as_undo_steps() {
        use song_panel::SongAction;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.song_action(SongAction::Add, None);
        app.song_action(SongAction::Add, None);
        assert_eq!(app.doc.project.song.bars.len(), 2);
        assert_eq!(app.view.song_cursor, 1);
        app.song_action(SongAction::Loop(0, 1), Some((0, 1)));
        assert_eq!(app.doc.project.song.loop_range, Some((0, 1)));
        app.song_action(SongAction::Duplicate(0, 1), Some((0, 1)));
        assert_eq!(app.doc.project.song.bars.len(), 4);
        app.song_action(SongAction::Delete(1, 3), Some((1, 3)));
        assert_eq!(app.doc.project.song.bars.len(), 1);
        assert_eq!(app.view.song_cursor, 0);
        let mut song = app.doc.project.song.clone();
        acid_engine::song::write_automation(
            &mut song,
            acid_engine::params::ParamId::Master(acid_engine::params::MasterParam::Level),
            0,
            3,
            0.5,
        );
        app.apply_song(song, false);
        run_frames(&mut app, 2);
        for _ in 0..6 {
            app.doc.undo();
        }
        assert!(app.doc.project.song.bars.is_empty());
    }

    #[test]
    fn song_insert_paste_and_no_loop_actions() {
        use acid_engine::project::{Bar, PatternRef};
        use song_panel::SongAction;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let bar = |n: u8| {
            let mut b = Bar::default();
            b.cell_mut(MachineUid(1)).pattern = Some(PatternRef(n));
            b
        };
        let mut song = app.doc.project.song.clone();
        song.bars = vec![bar(1), bar(2), bar(3)];
        app.apply_song(song, false);
        let start = app.doc.project.song.clone();
        app.song_action(SongAction::Insert, Some((1, 1)));
        assert_eq!(
            app.doc.project.song.bars,
            [bar(1), bar(2), bar(2), bar(3)],
            "Insert copies the selected bar in front of it"
        );
        app.view.song_clip = Some(vec![bar(7), bar(8)]);
        app.song_action(SongAction::Paste, Some((0, 0)));
        assert_eq!(
            app.doc.project.song.bars,
            [bar(1), bar(7), bar(8), bar(2), bar(2), bar(3)],
            "Paste goes after the selection"
        );
        app.song_action(SongAction::Paste, None);
        assert_eq!(
            app.doc.project.song.bars.len(),
            8,
            "no selection: at the end"
        );
        assert_eq!(app.doc.project.song.bars[6..], [bar(7), bar(8)]);
        app.song_action(SongAction::Insert, None);
        let bar = &app.doc.project.song.bars[8];
        for m in &app.doc.project.machines {
            assert_eq!(bar.cell(m.uid).pattern, Some(PatternRef(m.selected)));
        }
        app.song_action(SongAction::Loop(0, 2), Some((0, 2)));
        app.song_action(SongAction::NoLoop, None);
        assert_eq!(app.doc.project.song.loop_range, None);
        app.doc.undo();
        assert_eq!(app.doc.project.song.loop_range, Some((0, 2)));
        for _ in 0..5 {
            app.doc.undo();
        }
        assert_eq!(app.doc.project.song, start, "each action was one undo step");
    }

    #[test]
    fn fm_bar_transpose_is_an_undoable_song_edit() {
        use acid_engine::machine::MachineUid;
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let mut song = app.doc.project.song.clone();
        song.bars = vec![acid_engine::project::Bar::default(); 2];
        app.apply_song(song, false);
        let before = app.doc.project.song.clone();
        app.pending.clear();
        let fm = MachineUid(5);
        let at = app.doc.project.position(fm).unwrap();
        let mut song = before.clone();
        song.bars[1].cell_mut(fm).transpose = 5;
        app.apply_song(song, false);
        assert!(matches!(
            app.pending.as_slice(),
            [Command::Song(s)] if s.bars[1].transpose[at] == 5
        ));
        app.doc.undo();
        assert_eq!(app.doc.project.song, before);
    }

    #[test]
    fn song_panel_draws_and_bar_ops_are_undoable() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        run_frames(&mut app, 2);
        let mut song = app.doc.project.song.clone();
        song.bars = vec![acid_engine::project::Bar::default(); 3];
        app.apply_song(song, false);
        app.view.song_cursor = 0;
        app.view.song_sel_end = Some(1);
        assert_eq!(app.song_selection(), Some((0, 1)));
        run_frames(&mut app, 2);
        app.doc.undo();
        assert!(app.doc.project.song.bars.is_empty());
    }

    #[test]
    fn song_bars_carry_fm_and_sampler_patterns() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.doc.select_pattern(FM, 3);
        app.doc.select_pattern(SAMPLER, 17);
        app.song_action(song_panel::SongAction::Add, None);
        let bar = &app.doc.project.song.bars[0];
        use acid_engine::machine::MachineUid;
        assert_eq!(
            bar.cell(MachineUid(5)).pattern,
            Some(acid_engine::project::PatternRef(3))
        );
        assert_eq!(
            bar.cell(MachineUid(6)).pattern,
            Some(acid_engine::project::PatternRef(17))
        );
        run_frames(&mut app, 2);
    }

    #[test]
    fn importing_a_bank_and_loading_voices() {
        use acid_engine::fm::sysex::{bank_sysex, single_sysex};
        use acid_engine::params::FmParam;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let bank = fm_voices::starter().to_vec();
        app.import_syx_bytes(FM, &bank_sysex(&bank));
        assert_eq!(
            app.doc.project.fm(FM).unwrap().bank.as_ref().map(Vec::len),
            Some(32)
        );
        assert_eq!(
            app.view.panel(FM).fm_browser,
            fm_voices::FmBrowser::Imported
        );
        app.doc.undo();
        assert_eq!(app.doc.project.fm(FM).unwrap().bank, None);

        app.import_syx_bytes(FM, &single_sysex(&bank[5]));
        assert_eq!(app.doc.project.fm(FM).unwrap().voice_name, bank[5].name);
        let alg = FM.param(FmParam::Algorithm);
        assert!((app.doc.project.param(alg) - bank[5].algorithm as f32 / 31.0).abs() < 1e-6);

        let before = app.doc.project.clone();
        app.import_syx_bytes(FM, b"not sysex");
        assert!(app.view.message.as_deref().unwrap_or("").contains("DX7"));
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn fm_panel_draws_headless_with_a_bank_and_each_operator() {
        let mut doc = Document::new(Project::default());
        doc.project.fm_mut(FM).unwrap().bank = Some(fm_voices::starter().to_vec());
        let mut app = SidAcidApp::headless(doc);
        app.view.panel(FM).fm_browser = fm_voices::FmBrowser::Imported;
        for op in 0..6 {
            app.view.panel(FM).fm_op = op;
            run_frames(&mut app, 1);
        }
        assert!(
            app.view.panel(FM).poly_scrolled,
            "the FM roll is still drawn"
        );
    }

    #[test]
    fn undoing_an_import_returns_the_browser_to_the_starter_list() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let bank = fm_voices::starter().to_vec();
        app.import_syx_bytes(FM, &acid_engine::fm::sysex::bank_sysex(&bank));
        assert_eq!(
            app.view.panel(FM).fm_browser,
            fm_voices::FmBrowser::Imported
        );
        app.doc.undo();
        run_frames(&mut app, 1);
        assert_eq!(app.view.panel(FM).fm_browser, fm_voices::FmBrowser::Starter);
    }

    #[test]
    fn the_fm_panel_draws_with_imported_selected_and_no_bank() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.view.panel(FM).fm_browser = fm_voices::FmBrowser::Imported;
        run_frames(&mut app, 2);
        assert_eq!(app.view.panel(FM).fm_browser, fm_voices::FmBrowser::Starter);
    }

    #[test]
    fn the_fm_lfo_and_effects_section_draws_when_open() {
        let mut closed = SidAcidApp::headless(Document::new(Project::default()));
        let mut open = SidAcidApp::headless(Document::new(Project::default()));
        open.view.force_open.push(("fm_more", true));
        let closed_shapes = run_frames_in(&egui::Context::default(), &mut closed, 2);
        let open_shapes = run_frames_in(&egui::Context::default(), &mut open, 2);
        assert!(
            open_shapes > closed_shapes,
            "the section's knobs draw: {open_shapes} vs {closed_shapes}"
        );
    }

    #[test]
    fn a_voice_source_is_cleared_by_direct_loads_and_document_loads() {
        let mut doc = Document::new(Project::default());
        let mut bank = fm_voices::starter().to_vec();
        bank[1].name = bank[0].name.clone();
        doc.project.fm_mut(FM).unwrap().bank = Some(bank.clone());
        let mut app = SidAcidApp::headless(doc);
        app.view.panel(FM).fm_browser = fm_voices::FmBrowser::Imported;
        // A grid click left a source behind; a direct load must clear it.
        app.view.panel(FM).fm_voice_src = Some((fm_voices::FmBrowser::Imported, 0));
        app.load_fm_voice(FM, &bank[1]);
        assert_eq!(
            app.view.panel(FM).fm_voice_src,
            None,
            "a direct load has no source"
        );
        app.view.panel(FM).fm_voice_src = Some((fm_voices::FmBrowser::Imported, 1));
        app.load_document(Document::new(Project::default()));
        assert_eq!(
            app.view.panel(FM).fm_voice_src,
            None,
            "a document load clears it"
        );
    }

    #[test]
    fn undoing_a_voice_load_or_a_bank_import_drops_the_voice_source() {
        // Two rows with the same name: the name guard alone can't tell them apart.
        let mut bank = fm_voices::starter().to_vec();
        bank[2].name = bank[0].name.clone();
        let mut doc = Document::new(Project::default());
        doc.project.fm_mut(FM).unwrap().bank = Some(bank.clone());
        let mut app = SidAcidApp::headless(doc);
        app.view.panel(FM).fm_browser = fm_voices::FmBrowser::Imported;
        app.load_fm_voice(FM, &bank[0]);
        app.view.panel(FM).fm_voice_src = Some((fm_voices::FmBrowser::Imported, 0));
        app.load_fm_voice(FM, &bank[2]);
        app.view.panel(FM).fm_voice_src = Some((fm_voices::FmBrowser::Imported, 2));
        app.undo();
        assert_eq!(
            app.view.panel(FM).fm_voice_src,
            None,
            "row 0's voice is back, not row 2's"
        );
        app.redo();
        assert_eq!(
            app.view.panel(FM).fm_voice_src,
            None,
            "a redo has no grid source either"
        );

        // A knob edit after a grid load keeps the source through its undo.
        app.view.panel(FM).fm_voice_src = Some((fm_voices::FmBrowser::Imported, 2));
        let cmds = app.doc.apply(
            Edit::Param {
                id: FM.param(acid_engine::params::FmParam::Volume),
                value: 0.3,
            },
            false,
        );
        app.send(cmds);
        app.undo();
        assert_eq!(
            app.view.panel(FM).fm_voice_src,
            Some((fm_voices::FmBrowser::Imported, 2))
        );

        // Importing another bank: the stored index now points into new voices.
        let syx = acid_engine::fm::sysex::bank_sysex(&bank);
        app.import_syx_bytes(FM, &syx);
        assert_eq!(
            app.view.panel(FM).fm_voice_src,
            None,
            "a bank import drops it"
        );
        app.view.panel(FM).fm_voice_src = Some((fm_voices::FmBrowser::Imported, 2));
        app.undo();
        assert_eq!(
            app.view.panel(FM).fm_voice_src,
            None,
            "undoing a bank import drops it"
        );
    }

    #[test]
    fn the_starter_bank_is_built_once() {
        assert!(std::ptr::eq(fm_voices::starter(), fm_voices::starter()));
        assert_eq!(fm_voices::starter().len(), 16);
    }

    fn test_wav(seconds: f32) -> Vec<u8> {
        test_wav_at(22_050, seconds)
    }

    fn test_wav_at(rate: u32, seconds: f32) -> Vec<u8> {
        let mut cur = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::new(&mut cur, spec).unwrap();
        for i in 0..(seconds * rate as f32) as usize {
            let click = if i % 5512 < 200 { 12_000.0 } else { 0.0 };
            w.write_sample(((i as f32 * 0.3).sin() * click) as i16)
                .unwrap();
        }
        w.finalize().unwrap();
        cur.into_inner()
    }

    #[test]
    fn loading_samples_picks_modes_zones_and_params() {
        use acid_engine::params::SlotParam;
        use acid_engine::sampler::slot::SlotMode;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.load_sample_bytes(SAMPLER, 0, "Break", Some("wav"), &test_wav(2.0));
        let slot = app.doc.project.sampler(SAMPLER).unwrap().slots[0]
            .as_ref()
            .expect("loaded");
        assert_eq!(slot.meta.mode, SlotMode::Slice);
        assert_eq!(slot.meta.base_key, 24);
        assert!(slot.meta.slices.len() >= 2);
        assert_eq!(slot.meta.original_bpm, 120.0);
        assert_eq!(
            app.doc
                .project
                .param(SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::Choke))),
            1.0
        );
        assert_eq!(
            app.doc
                .project
                .param(SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::TempoLock))),
            1.0
        );
        let span = slot.meta.slices.len() as u8;

        app.load_sample_bytes(SAMPLER, 1, "Stab", Some("wav"), &test_wav(0.5));
        let stab = app.doc.project.sampler(SAMPLER).unwrap().slots[1]
            .as_ref()
            .unwrap();
        assert_eq!(stab.meta.mode, SlotMode::Chromatic);
        assert_eq!(stab.meta.base_key, 24 + span, "placed above the break");
        assert_eq!(
            app.doc
                .project
                .param(SAMPLER.param(MachineParam::SamplerSlot(1, SlotParam::Choke))),
            0.0
        );

        app.doc.undo();
        assert!(
            app.doc.project.sampler(SAMPLER).unwrap().slots[1].is_none(),
            "a load is one undo step"
        );

        let before = app.doc.project.clone();
        app.load_sample_bytes(SAMPLER, 2, "Bad", Some("wav"), b"junk");
        assert!(app.view.message.is_some());
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn a_sample_rate_the_project_cannot_reopen_is_refused() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let before = app.doc.project.clone();
        app.load_sample_bytes(SAMPLER, 0, "Low", Some("wav"), &test_wav_at(6_000, 0.5));
        let msg = app
            .view
            .message
            .clone()
            .expect("a message explains the refusal");
        assert!(msg.contains("sample rate"), "{msg}");
        assert_eq!(app.doc.project, before);
        let ron = app.doc.project.to_ron();
        assert!(Project::from_ron(&ron).is_ok(), "the project still reopens");
    }

    #[test]
    fn meta_edits_refuse_zone_clashes() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.load_sample_bytes(SAMPLER, 0, "A", Some("wav"), &test_wav(0.5));
        app.load_sample_bytes(SAMPLER, 1, "B", Some("wav"), &test_wav(0.5));
        let b_base = app.doc.project.sampler(SAMPLER).unwrap().slots[1]
            .as_ref()
            .unwrap()
            .meta
            .base_key;
        assert!(!app.edit_slot_meta(SAMPLER, 1, false, |m| m.base_key = 30));
        assert!(app.view.message.is_some());
        assert_eq!(
            app.doc.project.sampler(SAMPLER).unwrap().slots[1]
                .as_ref()
                .unwrap()
                .meta
                .base_key,
            b_base
        );
        app.view.message = None;
        assert!(app.edit_slot_meta(SAMPLER, 1, false, |m| {
            m.base_key = 100;
            m.high_key = 110;
        }));
        app.clear_slot(SAMPLER, 0);
        assert!(app.doc.project.sampler(SAMPLER).unwrap().slots[0].is_none());
    }

    #[test]
    fn malformed_meta_edits_explain_themselves_except_mid_drag() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.load_sample_bytes(SAMPLER, 0, "A", Some("wav"), &test_wav(0.5));
        let before = app.doc.project.clone();
        assert!(!app.edit_slot_meta(SAMPLER, 0, true, |m| m.start = m.end));
        assert!(app.view.message.is_none(), "no message while dragging");
        assert!(!app.edit_slot_meta(SAMPLER, 0, false, |m| m.start = m.end));
        assert_eq!(
            app.view.message.as_deref(),
            Some("That change doesn't fit this sample.")
        );
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn moving_a_zone_keeps_its_span_and_refuses_to_pass_the_top_key() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.load_sample_bytes(SAMPLER, 0, "A", Some("wav"), &test_wav(0.5));
        let range = |app: &SidAcidApp| {
            let m = &app.doc.project.sampler(SAMPLER).unwrap().slots[0]
                .as_ref()
                .unwrap()
                .meta;
            (m.base_key, m.high_key)
        };
        assert_eq!(range(&app), (24, 47));
        let before = app.doc.project.clone();
        assert!(!app.move_zone(SAMPLER, 0, 110, true));
        assert!(app.view.message.is_some());
        assert_eq!(app.doc.project, before);
        app.view.message = None;
        assert!(app.move_zone(SAMPLER, 0, 104, false));
        assert_eq!(range(&app), (104, 127));
        assert!(app.move_zone(SAMPLER, 0, 24, false));
        assert_eq!(range(&app), (24, 47), "the span survives a trip to the top");
    }

    #[test]
    fn the_sampler_roll_starts_at_the_lowest_zone() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        assert_eq!(app.roll_start_pitch(FM), 48, "FM starts at C3");
        assert_eq!(app.roll_start_pitch(SAMPLER), 24, "C1 with no slot loaded");
        app.load_sample_bytes(SAMPLER, 3, "A", Some("wav"), &test_wav(0.5));
        assert!(app.move_zone(SAMPLER, 3, 70, false));
        app.load_sample_bytes(SAMPLER, 5, "B", Some("wav"), &test_wav(0.5));
        let low = app.doc.project.sampler(SAMPLER).unwrap().slots[5]
            .as_ref()
            .unwrap()
            .meta
            .base_key;
        assert!(low < 70);
        assert_eq!(app.roll_start_pitch(SAMPLER), low);
        app.clear_slot(SAMPLER, 5);
        assert_eq!(app.roll_start_pitch(SAMPLER), 70);
    }

    #[test]
    fn sampler_panel_draws_empty_and_loaded_slots_in_both_modes() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        run_frames(&mut app, 1);
        app.load_sample_bytes(SAMPLER, 0, "Break", Some("wav"), &test_wav(2.0));
        app.load_sample_bytes(SAMPLER, 1, "Stab", Some("wav"), &test_wav(0.5));
        for slot in [0, 1, 7] {
            app.view.panel(SAMPLER).sampler_slot = slot;
            run_frames(&mut app, 2);
        }
        assert!(
            app.view.wave_cache.contains_key(&(SAMPLER, 0))
                && app.view.wave_cache.contains_key(&(SAMPLER, 1))
        );
        assert!(
            app.view.panel(SAMPLER).poly_scrolled,
            "the sampler roll is drawn"
        );
    }

    #[test]
    fn a_dropped_audio_file_loads_into_the_selected_slot() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.view.panel(SAMPLER).sampler_slot = 3;
        let file = egui::DroppedFile {
            name: "x.wav".into(),
            bytes: Some(test_wav(0.5).into()),
            ..Default::default()
        };
        app.load_dropped_file(SAMPLER, &file);
        let d = app.doc.project.sampler(SAMPLER).unwrap().slots[3]
            .as_ref()
            .expect("slot 3 is loaded");
        assert_eq!(d.name, "x");
        assert!(
            app.doc
                .project
                .sampler(SAMPLER)
                .unwrap()
                .slots
                .iter()
                .enumerate()
                .all(|(i, s)| i == 3 || s.is_none())
        );
    }

    #[test]
    fn a_truncated_file_loads_with_a_warning() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let full = test_wav(0.5);
        app.load_sample_bytes(SAMPLER, 0, "Whole", Some("wav"), &full);
        assert!(app.view.message.is_none(), "a whole file loads silently");
        let cut = &full[..full.len() - 1000];
        app.load_sample_bytes(SAMPLER, 1, "Cut", Some("wav"), cut);
        assert!(
            app.doc.project.sampler(SAMPLER).unwrap().slots[1].is_some(),
            "partial audio is kept"
        );
        let msg = app.view.message.clone().expect("the user is told");
        assert!(msg.contains("ends early") && msg.contains("Cut"), "{msg}");
    }

    #[test]
    fn a_load_past_the_memory_limit_is_refused() {
        use acid_engine::sampler::MEMORY_LIMIT;
        use acid_engine::sampler::slot::{SlotData, SlotMeta};
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let frames = 5_700_000usize;
        let leave = 1_000;
        let mut left = MEMORY_LIMIT / 4 - leave;
        // Filler slots that fill the memory: valid slots, their metas span the PCM.
        for slot in 0..3 {
            let n = left.min(frames);
            left -= n;
            let big = SlotData {
                name: "big".into(),
                meta: SlotMeta::chromatic(24 + 2 * slot as u8, 25 + 2 * slot as u8, 24, n as u32),
                sample_rate: 192_000,
                channels: 1,
                pcm: vec![0; n],
            };
            assert!(big.is_valid(), "{:?}", big.problem());
            app.doc.project.sampler_mut(SAMPLER).unwrap().slots[slot] = Some(big);
        }
        assert_eq!(
            app.doc.project.sampler(SAMPLER).unwrap().memory_bytes(),
            MEMORY_LIMIT - leave * 4
        );
        let before = app.doc.project.clone();
        app.load_sample_bytes(SAMPLER, 3, "Over", Some("wav"), &test_wav(0.5));
        let msg = app.view.message.clone().expect("a message explains it");
        assert!(msg.contains("Not enough sample memory"), "{msg}");
        assert_eq!(app.doc.project, before);
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn clearing_a_slot_and_loading_one_both_undo_and_redo() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.load_sample_bytes(SAMPLER, 0, "A", Some("wav"), &test_wav(0.5));
        let loaded = app.doc.project.clone();
        app.clear_slot(SAMPLER, 0);
        assert!(app.doc.project.sampler(SAMPLER).unwrap().slots[0].is_none());
        let cmds = app.doc.undo();
        app.send(cmds);
        assert_eq!(app.doc.project, loaded, "undo of a clear restores the slot");
        let cmds = app.doc.undo();
        app.send(cmds);
        assert!(
            app.doc.project.sampler(SAMPLER).unwrap().slots[0].is_none(),
            "undo of the load"
        );
        let cmds = app.doc.redo();
        assert!(cmds.iter().any(|c| matches!(
            c,
            Command::SamplerSample {
                uid: acid_engine::machine::MachineUid(6),
                slot: 0,
                sample: Some(_)
            }
        )));
        app.send(cmds);
        assert_eq!(
            app.doc.project, loaded,
            "redo of a load restores slot and params"
        );
    }

    #[test]
    fn a_waveform_drag_is_one_undo_step_and_re_sends_the_slot_every_move() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.load_sample_bytes(SAMPLER, 0, "A", Some("wav"), &test_wav(0.5));
        let before = app.doc.project.clone();
        app.pending.clear();
        for end in [10_000, 9_000, 8_000, 7_000] {
            assert!(app.edit_slot_meta(SAMPLER, 0, true, |m| m.loop_end = end));
        }
        app.doc.end_gesture();
        let plays = app
            .pending
            .iter()
            .filter(|c| matches!(c, Command::SamplerPlay { slot: 0, .. }))
            .count();
        assert_eq!(plays, 4, "each move reaches the engine");
        let cmds = app.doc.undo();
        app.send(cmds);
        assert_eq!(app.doc.project, before, "one undo undoes the whole drag");
    }

    #[test]
    fn slot_audio_commands_drop_the_cached_waveform() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.load_sample_bytes(SAMPLER, 0, "A", Some("wav"), &test_wav(0.5));
        run_frames(&mut app, 2);
        assert!(app.view.wave_cache.contains_key(&(SAMPLER, 0)));
        app.load_sample_bytes(SAMPLER, 0, "B", Some("wav"), &test_wav(0.5));
        assert!(!app.view.wave_cache.contains_key(&(SAMPLER, 0)), "reload");
        run_frames(&mut app, 2);
        app.clear_slot(SAMPLER, 0);
        assert!(!app.view.wave_cache.contains_key(&(SAMPLER, 0)), "clear");
        let cmds = app.doc.undo();
        run_frames(&mut app, 1);
        app.send(cmds);
        assert!(!app.view.wave_cache.contains_key(&(SAMPLER, 0)), "undo");
    }

    #[test]
    fn a_marker_drag_whose_slot_empties_ends_its_gesture() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        // A drag's merged step is open; then the shown slot has no sample.
        let cmds = app.doc.apply(Edit::Bpm(100.0), true);
        app.send(cmds);
        app.view.wave_drag = Some((SAMPLER, 0, sampler_ui::Marker::Start));
        assert!(app.doc.project.sampler(SAMPLER).unwrap().slots[0].is_none());
        run_frames(&mut app, 1);
        assert_eq!(app.view.wave_drag, None);
        let cmds = app.doc.apply(Edit::Bpm(110.0), true);
        app.send(cmds);
        app.doc.undo();
        assert_eq!(
            app.doc.project.bpm, 100.0,
            "the next edit is a step of its own"
        );
    }

    #[test]
    fn loading_a_document_releases_the_held_notes() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.view.pad_note = Some((SAMPLER, 36));
        app.view.audition = Some((FM, 60));
        app.load_document(Document::new(Project::default()));
        assert_eq!((app.view.pad_note, app.view.audition), (None, None));
        // They reach the old engine: flushed before the load drops `pending`.
        let offs: Vec<_> = app
            .flushed
            .iter()
            .filter_map(|c| match c {
                Command::NoteOff { uid, pitch } => Some((*uid, *pitch)),
                _ => None,
            })
            .collect();
        assert_eq!(offs, vec![(MachineUid(6), 36), (MachineUid(5), 60)]);
    }

    #[test]
    fn a_rack_edit_sends_one_set_rack_built_against_the_engine_order() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.pending.clear();
        let before = app.engine_rack.clone();
        app.add_machine(MachineType::Fm); // Task 5 adds the UI; this task adds the method (doc op + sync_rack)
        let swaps: Vec<_> = app
            .pending
            .iter()
            .filter(|c| matches!(c, Command::SetRack(_)))
            .collect();
        assert_eq!(swaps.len(), 1);
        if let Command::SetRack(s) = swaps[0] {
            assert_eq!(s.order, app.doc.project.rack_uids());
            assert_eq!(s.fresh.iter().filter(|f| f.is_some()).count(), 1);
        }
        assert_ne!(app.engine_rack, before);
        app.pending.clear();
        app.undo();
        assert!(
            app.pending.iter().any(|c| matches!(c, Command::SetRack(_))),
            "undo syncs too"
        );
        assert_eq!(app.engine_rack, before);
    }

    #[test]
    fn statuses_from_an_older_rack_do_not_move_panels() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Sampler);
        let mut old = Status {
            rack_gen: app.rack_expected.wrapping_sub(1),
            ..Status::default()
        };
        old.playhead[0] = 9;
        app.on_status(old, 1.0);
        assert_ne!(app.status.playhead[0], 9);
        let mut new = Status {
            rack_gen: app.rack_expected,
            ..Status::default()
        };
        new.playhead[0] = 9;
        app.on_status(new, 2.0);
        assert_eq!(app.status.playhead[0], 9);
    }

    #[test]
    fn machine_cpu_comes_from_statuses_of_the_engine_rack_and_goes_with_the_machine() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Sampler);
        let rack = app.engine_rack.clone();
        let mut old = Status {
            rack_gen: app.rack_expected.wrapping_sub(1),
            ..Status::default()
        };
        old.slot_cpu[0] = 0.3;
        app.on_status(old, 1.0);
        assert!(app.view.perf.machine_cpu(rack[0]).is_none());
        let mut new = Status {
            rack_gen: app.rack_expected,
            ..Status::default()
        };
        new.slot_cpu[0] = 0.3;
        new.slot_cpu[1] = 0.6;
        app.on_status(new, 2.0);
        assert_eq!(app.view.perf.machine_cpu(rack[0]), Some(0.3));
        assert_eq!(app.view.perf.machine_cpu(rack[1]), Some(0.6));
        run_frames(&mut app, 2); // headers draw their readings
        app.remove_machine(rack[1]);
        assert!(app.view.perf.machine_cpu(rack[1]).is_none());
        assert!(app.view.perf.machine_cpu(rack[0]).is_some());
    }

    #[test]
    fn removed_machines_leave_no_ui_state_behind() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.panel(MachineUid(6)).sampler_slot = 3;
        app.remove_machine(MachineUid(6));
        assert!(!app.view.panels.contains_key(&MachineUid(6)));
    }

    #[test]
    fn removing_prunes_every_uid_keyed_ui_state_and_says_how_to_undo() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.focus = Some(SAMPLER);
        app.view.audition = Some((SAMPLER, 60));
        app.view.pad_note = Some((SAMPLER, 61));
        app.view
            .wave_cache
            .insert((SAMPLER, 0), (0, 0, 0, Vec::new()));
        app.view.wave_drag = Some((SAMPLER, 0, sampler_ui::Marker::Start));
        app.view.wave_cache.insert((FM, 0), (0, 0, 0, Vec::new()));
        app.remove_machine(SAMPLER);
        assert_eq!(app.view.focus, None);
        assert_eq!(app.view.audition, None);
        assert_eq!(app.view.pad_note, None);
        assert!(app.view.wave_drag.is_none());
        assert_eq!(app.view.wave_cache.len(), 1, "other machines keep theirs");
        assert_eq!(
            app.view.status_line.as_deref(),
            Some("Removed 'Sampler' — Ctrl+Z to undo")
        );
        app.undo();
        assert_eq!(app.engine_rack, app.doc.project.rack_uids());
        run_frames(&mut app, 2);
    }

    #[test]
    fn the_expected_rack_generation_follows_the_host_counter() {
        // Headless: no host, but the count runs as if one took every rack.
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        assert_eq!(app.rack_expected, 0, "a new host starts at 0");
        assert_eq!(app.engine_rack, app.doc.project.rack_uids());
        app.add_machine(MachineType::Fm);
        assert_eq!(app.rack_expected, 1, "a SetRack: +1");
        let uid = app.doc.project.machines.last().unwrap().uid;
        app.move_machine(uid, 0);
        app.rename_machine(uid, "Lead");
        assert_eq!(app.rack_expected, 2, "a rename sends no SetRack");
        app.duplicate_machine(uid);
        assert_eq!(app.rack_expected, 3);
        app.load_document(Document::new(Project::default()));
        assert_eq!(app.rack_expected, 4, "a ReplaceEngine on the same host: +1");
        assert_eq!(app.engine_rack, app.doc.project.rack_uids());
        app.add_machine(MachineType::Synth);
        assert_eq!(app.rack_expected, 5);
        let out = app.apply_audio(
            crate::settings::AudioSettings {
                driver: Some("JACK".into()),
                ..Default::default()
            },
            &mut |_| Ok((None, vec![])),
        );
        assert_eq!(out, audio_settings::ApplyOutcome::Applied);
        assert_eq!(app.rack_expected, 0, "a new stream is a new host");
        assert_eq!(app.engine_rack, app.doc.project.rack_uids());
    }

    #[test]
    fn a_stale_status_keeps_the_rack_arrays_but_takes_the_transport() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.status.patterns[1] = 4;
        app.status.meters[1] = 0.5;
        app.add_machine(MachineType::Fm);
        let mut old = Status {
            rack_gen: app.rack_expected.wrapping_sub(1),
            playing: true,
            song_bar: 7,
            ..Status::default()
        };
        old.patterns[1] = 2;
        old.meters[1] = 0.9;
        old.meters[acid_engine::machine::MAX_MACHINES] = 0.25;
        app.on_status(old, 1.0);
        assert!(app.status.playing);
        assert_eq!(app.status.song_bar, 7);
        assert_eq!(app.status.patterns[1], 4, "per-machine arrays stay");
        assert_eq!(app.status.meters[1], 0.5);
        assert_eq!(
            app.status.meters[acid_engine::machine::MAX_MACHINES],
            0.25,
            "the master meter is not per machine"
        );
        assert_eq!(app.status.rack_gen, app.rack_expected);
    }

    #[test]
    fn a_rack_edit_moves_the_shown_status_with_the_machines() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.status.playing = true;
        app.status.playhead[5] = 3; // the sampler, last in the rack
        app.status.patterns[5] = 2;
        app.move_machine(SAMPLER, 0);
        assert_eq!(app.playhead(SAMPLER, 2), Some(3));
        assert_eq!(app.status.playhead[0], 3);
    }

    #[test]
    fn a_refused_rack_restarts_on_a_fresh_host() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Fm);
        app.add_machine(MachineType::Fm);
        let refused = Status {
            rack_gen: 1, // the host took the first SetRack only
            rack_rejected: true,
            ..Status::default()
        };
        assert!(app.reload_after_refused_rack(&refused));
        assert_eq!(app.rack_expected, 0, "a fresh host counts from 0");
        assert_eq!(app.engine_rack, app.doc.project.rack_uids());
        assert!(app.pending.is_empty(), "queued swaps were for the old rack");
        // Each refused status restarts once.
        app.add_machine(MachineType::Fm);
        assert!(!app.pending.is_empty());
        assert!(app.reload_after_refused_rack(&refused));
        assert_eq!(app.rack_expected, 0);
        assert!(app.pending.is_empty());
    }

    /// Fills sampler slots (5.7M frames at most each) with `frames` in all.
    fn sampler_with_frames(app: &mut SidAcidApp, mut frames: usize) {
        use acid_engine::sampler::slot::{SlotData, SlotMeta};
        for slot in 0..3 {
            let n = frames.min(5_700_000);
            frames -= n;
            if n == 0 {
                break;
            }
            let big = SlotData {
                name: "big".into(),
                meta: SlotMeta::chromatic(24 + 2 * slot as u8, 25 + 2 * slot as u8, 24, n as u32),
                sample_rate: 192_000,
                channels: 1,
                pcm: vec![0; n],
            };
            assert!(big.is_valid(), "{:?}", big.problem());
            app.doc.project.sampler_mut(SAMPLER).unwrap().slots[slot] = Some(big);
        }
    }

    #[test]
    fn duplicating_a_sampler_past_the_memory_limit_is_refused() {
        use acid_engine::sampler::MEMORY_LIMIT;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        sampler_with_frames(&mut app, MEMORY_LIMIT / 4 / 2 + 1_000);
        let count = app.doc.project.machines.len();
        let expected = app.rack_expected;
        assert!(app.duplicate_machine(SAMPLER).is_none());
        assert_eq!(app.doc.project.machines.len(), count);
        assert_eq!(app.rack_expected, expected);
        assert!(!app.pending.iter().any(|c| matches!(c, Command::SetRack(_))));
        assert!(!app.doc.can_undo());
        let msg = app.view.message.clone().expect("the user is told");
        assert!(
            msg.starts_with("Not enough sample memory to duplicate '"),
            "{msg}"
        );
        assert!(msg.ends_with("' (64 MB in all)"), "{msg}");
    }

    #[test]
    fn duplicating_a_small_sampler_works() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        sampler_with_frames(&mut app, 1_000);
        let count = app.doc.project.machines.len();
        assert!(app.duplicate_machine(SAMPLER).is_some());
        assert_eq!(app.doc.project.machines.len(), count + 1);
        assert!(app.view.message.is_none());
    }

    #[test]
    fn each_fm_and_sampler_panel_keeps_its_own_view_state() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Fm);
        app.add_machine(MachineType::Sampler);
        let (fm2, s2) = {
            let m = &app.doc.project.machines;
            (m[m.len() - 2].uid, m[m.len() - 1].uid)
        };
        app.view.panel(FM).fm_op = 3;
        app.view.panel(FM).fm_voice_src = Some((fm_voices::FmBrowser::Starter, 1));
        app.view.panel(SAMPLER).sampler_slot = 5;
        run_frames(&mut app, 2);
        assert_eq!(app.view.panel(fm2).fm_op, 0);
        assert_eq!(app.view.panel(fm2).fm_voice_src, None);
        assert_eq!(app.view.panel(s2).sampler_slot, 0);
        assert_eq!(app.view.panel(FM).fm_op, 3);
        // A voice load on one FM clears only that one's source.
        let voice = fm_voices::starter()[2].clone();
        app.view.panel(fm2).fm_voice_src = Some((fm_voices::FmBrowser::Starter, 4));
        app.load_fm_voice(FM, &voice);
        assert_eq!(app.view.panel(FM).fm_voice_src, None);
        assert_eq!(
            app.view.panel(fm2).fm_voice_src,
            Some((fm_voices::FmBrowser::Starter, 4))
        );
    }

    #[test]
    fn recording_a_pattern_change_for_a_missing_machine_does_nothing() {
        let mut app = recording_app();
        app.remove_machine(SYNTH2);
        let song = app.doc.project.song.clone();
        app.record_pattern_change(SYNTH2, 3);
        assert_eq!(app.doc.project.song, song);
    }
}
