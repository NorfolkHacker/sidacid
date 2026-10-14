//! The sampler panel: global knobs, 16 slot pads, the selected slot's
//! controls and waveform with draggable markers, its knobs, and the
//! pattern row with the poly roll. Audio files can be dropped on it.

use super::sampler_ui::{
    Marker, frame_to_x, key_name, markers, nearest_marker, peaks, set_marker, x_to_frame, zone_text,
};
use super::{SidAcidApp, theme};
use acid_engine::host::Command;
use acid_engine::machine::{MachineType, MachineUid};
use acid_engine::params::{MachineParam, SamplerParam, SlotParam};
use acid_engine::project::{PatternRef, SamplerMachine};
use acid_engine::sampler::SLOTS;
use acid_engine::sampler::slicing::{detect_slices, equal_slices, mono_mix};
use acid_engine::sampler::slot::SlotMode;
use eframe::egui::{self, Sense, Stroke, pos2, vec2};

const SLOT_KNOBS: [(SlotParam, &str); 13] = [
    (SlotParam::Level, "Level"),
    (SlotParam::Pan, "Pan"),
    (SlotParam::Tune, "Tune"),
    (SlotParam::Fine, "Fine"),
    (SlotParam::Cutoff, "Cutoff"),
    (SlotParam::Resonance, "Reso"),
    (SlotParam::Attack, "Attack"),
    (SlotParam::Decay, "Decay"),
    (SlotParam::Sustain, "Sustain"),
    (SlotParam::Release, "Release"),
    (SlotParam::Reverse, "Reverse"),
    (SlotParam::Choke, "Choke"),
    (SlotParam::TempoLock, "Tempo"),
];
const WAVE_H: f32 = 100.0;
const MARKER_GRAB_PX: f32 = 5.0;

impl SidAcidApp {
    /// Sampler `uid`; it must be a sampler in the rack.
    pub(crate) fn sampler_of(&self, uid: MachineUid) -> &SamplerMachine {
        self.doc
            .project
            .sampler(uid)
            .expect("a sampler in the rack")
    }

    pub(crate) fn sampler_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{}   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label()
        );
        egui::CollapsingHeader::new(egui::RichText::new(title).strong())
            .id_salt(("sampler", uid))
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    self.param_knob(ui, uid.param(SamplerParam::Volume), "Volume");
                    self.param_knob(ui, uid.param(SamplerParam::Grit), "Grit");
                });
                self.sampler_pads(ui, uid);
                self.sampler_slot_row(ui, uid);
                self.sampler_waveform(ui, uid);
                let s = self.view.sampler_slot.min(SLOTS - 1) as u8;
                ui.horizontal_wrapped(|ui| {
                    for (p, label) in SLOT_KNOBS {
                        self.param_knob(ui, uid.param(MachineParam::SamplerSlot(s, p)), label);
                    }
                });
                self.note_pattern_row(ui, uid);
                self.poly_roll(ui, uid);
            });
        self.sampler_drop(ui, uid);
        self.release_pad_note(ui);
    }

    fn sampler_pads(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        egui::Grid::new(("sampler_pads", uid))
            .spacing([3.0, 3.0])
            .show(ui, |ui| {
                for i in 0..SLOTS {
                    let name = self.sampler_of(uid).slots[i]
                        .as_ref()
                        .map_or("·".to_string(), |d| d.name.chars().take(6).collect());
                    let r = ui.add_sized(
                        [64.0, 22.0],
                        egui::Button::new(format!("{} {}", i + 1, name))
                            .selected(self.view.sampler_slot == i),
                    );
                    if r.clicked() {
                        self.view.sampler_slot = i;
                    }
                    let shift = ui.input(|inp| inp.modifiers.shift);
                    if shift
                        && r.is_pointer_button_down_on()
                        && ui.input(|inp| inp.pointer.primary_down())
                        && self.view.pad_note.is_none()
                        && let Some(d) = self.sampler_of(uid).slots[i].as_ref()
                    {
                        let key = match d.meta.mode {
                            SlotMode::Slice => d.meta.base_key,
                            SlotMode::Chromatic => d.meta.root,
                        };
                        self.send(vec![Command::NoteOn {
                            uid,
                            pitch: key,
                            vel: 110,
                        }]);
                        self.view.pad_note = Some((uid, key));
                    }
                    if i % 8 == 7 {
                        ui.end_row();
                    }
                }
            });
    }

    fn release_pad_note(&mut self, ui: &egui::Ui) {
        if let Some((uid, key)) = self.view.pad_note
            && !ui.input(|i| i.pointer.primary_down())
        {
            self.send(vec![Command::NoteOff { uid, pitch: key }]);
            self.view.pad_note = None;
        }
    }

    fn sampler_slot_row(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let s = self.view.sampler_slot.min(SLOTS - 1);
        let info = self.sampler_of(uid).slots[s].as_ref().map(|d| {
            (
                d.name.clone(),
                d.meta.clone(),
                d.frames(),
                d.channels,
                d.sample_rate,
            )
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("Slot {}", s + 1)).strong());
            if ui.button("Load…").clicked() {
                self.load_sample_dialog(uid, s);
            }
            let Some((name, meta, frames, channels, sample_rate)) = info else {
                ui.weak("Empty — load or drop an audio file (WAV, FLAC, MP3, OGG).");
                return;
            };
            ui.label(&name);
            if ui.button("Clear").clicked() {
                self.clear_slot(uid, s);
                return;
            }
            ui.separator();
            for (mode, label) in [
                (SlotMode::Slice, "Slice"),
                (SlotMode::Chromatic, "Chromatic"),
            ] {
                if ui.selectable_label(meta.mode == mode, label).clicked() && meta.mode != mode {
                    self.edit_slot_meta(uid, s, false, |m| {
                        m.mode = mode;
                        if mode == SlotMode::Chromatic {
                            m.high_key = m.base_key.saturating_add(23).min(127);
                            m.root = m.base_key;
                        }
                    });
                }
            }
            let mut base = meta.base_key;
            let r = ui.add(
                egui::DragValue::new(&mut base)
                    .range(0..=127)
                    .prefix("Zone "),
            );
            if r.changed() {
                self.move_zone(uid, s, base, r.dragged());
            }
            if r.drag_stopped() || r.lost_focus() {
                self.doc.end_gesture();
            }
            ui.label(zone_text(&meta));
            ui.separator();
            match meta.mode {
                SlotMode::Slice => {
                    for n in [4usize, 8, 16, 32] {
                        if ui.button(format!("÷{n}")).clicked() {
                            self.edit_slot_meta(uid, s, false, |m| {
                                m.slices = equal_slices(frames, n)
                            });
                        }
                    }
                    let mut sens = self.view.sampler_sensitivity.unwrap_or(0.5);
                    if ui
                        .add(egui::Slider::new(&mut sens, 0.0..=1.0).text("Sens"))
                        .changed()
                    {
                        self.view.sampler_sensitivity = Some(sens);
                    }
                    if ui.button("Detect").clicked()
                        && let Some(d) = self.sampler_of(uid).slots[s].as_ref()
                    {
                        let pcm: Vec<f32> = d.pcm.iter().map(|&x| x as f32 / 32_768.0).collect();
                        let slices =
                            detect_slices(&mono_mix(&pcm, channels as usize), sample_rate, sens);
                        self.edit_slot_meta(uid, s, false, |m| m.slices = slices);
                    }
                    let mut bpm = meta.original_bpm;
                    let r = ui.add(
                        egui::DragValue::new(&mut bpm)
                            .range(20.0..=400.0)
                            .speed(0.1)
                            .prefix("Orig BPM "),
                    );
                    if r.changed() {
                        self.edit_slot_meta(uid, s, r.dragged(), |m| m.original_bpm = bpm);
                    }
                    if r.drag_stopped() || r.lost_focus() {
                        self.doc.end_gesture();
                    }
                }
                SlotMode::Chromatic => {
                    let mut root = meta.root;
                    let r = ui.add(
                        egui::DragValue::new(&mut root)
                            .range(0..=127)
                            .custom_formatter(|v, _| key_name(v as u8))
                            .prefix("Root "),
                    );
                    if r.changed() {
                        self.edit_slot_meta(uid, s, r.dragged(), |m| m.root = root);
                    }
                    let mut top = meta.high_key;
                    let r2 = ui.add(
                        egui::DragValue::new(&mut top)
                            .range(meta.base_key as u32..=127)
                            .custom_formatter(|v, _| key_name(v as u8))
                            .prefix("Top "),
                    );
                    if r2.changed() {
                        self.edit_slot_meta(uid, s, r2.dragged(), |m| m.high_key = top);
                    }
                    if r.drag_stopped() || r.lost_focus() || r2.drag_stopped() || r2.lost_focus() {
                        self.doc.end_gesture();
                    }
                    let mut looping = meta.loop_on;
                    if ui.checkbox(&mut looping, "Loop").changed() {
                        self.edit_slot_meta(uid, s, false, |m| m.loop_on = looping);
                    }
                }
            }
        });
    }

    /// Takes the open waveform marker drag if it is on sampler `uid`.
    fn take_wave_drag(&mut self, uid: MachineUid) -> bool {
        let mine = self.view.wave_drag.is_some_and(|(u, _, _)| u == uid);
        if mine {
            self.view.wave_drag = None;
        }
        mine
    }

    fn sampler_waveform(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let s = self.view.sampler_slot.min(SLOTS - 1);
        let Some(d) = self.sampler_of(uid).slots[s].as_ref() else {
            // A drag whose slot emptied under it still closes its undo step.
            if self.take_wave_drag(uid) {
                self.doc.end_gesture();
            }
            return;
        };
        let width = ui.available_width().clamp(200.0, 900.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, WAVE_H), Sense::click_and_drag());
        let columns = width as usize;
        let key = (d.pcm.as_ptr() as usize, d.pcm.len(), columns);
        let fresh = self
            .view
            .wave_cache
            .get(&(uid, s))
            .is_some_and(|c| (c.0, c.1, c.2) == key);
        if !fresh {
            let p = peaks(&d.pcm, d.channels as usize, columns);
            self.view
                .wave_cache
                .insert((uid, s), (key.0, key.1, key.2, p));
        }
        let Some(d) = self.sampler_of(uid).slots[s].as_ref() else {
            return;
        };
        let frames = d.frames();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, theme::GRID_DARK);
        let mid = rect.center().y;
        let half = rect.height() * 0.45;
        if let Some((_, _, _, cols)) = self.view.wave_cache.get(&(uid, s)) {
            for (i, (lo, hi)) in cols.iter().enumerate() {
                let x = rect.left() + i as f32 + 0.5;
                painter.line_segment(
                    [pos2(x, mid - hi * half), pos2(x, mid - lo * half)],
                    Stroke::new(1.0_f32, theme::KNOB_TRACK),
                );
            }
        }
        let marks = markers(&d.meta);
        let xs: Vec<f32> = marks
            .iter()
            .map(|(_, f)| frame_to_x(*f, frames, rect.left(), rect.width()))
            .collect();
        for ((m, _), x) in marks.iter().zip(&xs) {
            let colour = match m {
                Marker::Slice(_) => theme::ACID,
                Marker::Start | Marker::End => theme::ACCENT,
                Marker::LoopStart | Marker::LoopEnd => theme::SLIDE,
            };
            painter.line_segment(
                [pos2(*x, rect.top()), pos2(*x, rect.bottom())],
                Stroke::new(1.5_f32, colour),
            );
        }
        let pointer = resp.interact_pointer_pos();
        if resp.drag_started() {
            self.view.wave_drag = pointer
                .and_then(|p| nearest_marker(&xs, p.x, MARKER_GRAB_PX))
                .map(|i| (uid, s, marks[i].0));
        }
        if resp.dragged()
            && let Some(p) = pointer
            && let Some((du, ds, marker)) = self.view.wave_drag
            && (du, ds) == (uid, s)
        {
            let frame = x_to_frame(p.x, frames, rect.left(), rect.width());
            self.edit_slot_meta(uid, s, true, |m| {
                set_marker(m, marker, frame, frames);
            });
        }
        if resp.drag_stopped() && self.take_wave_drag(uid) {
            self.doc.end_gesture();
        }
    }

    fn sampler_drop(&mut self, ui: &egui::Ui, uid: MachineUid) {
        // Samplers are the app's only drop targets, and OS file drags
        // rarely report a pointer position, so any drop loads into one
        // sampler: `drop_target`.
        if self.drop_target() != Some(uid) {
            return;
        }
        let file = ui.ctx().input(|i| i.raw.dropped_files.first().cloned());
        if let Some(file) = file {
            self.load_dropped_file(uid, &file);
        }
    }

    /// The sampler a dropped file loads into: the focused machine if it is
    /// a sampler, else the first sampler in the rack.
    pub(crate) fn drop_target(&self) -> Option<MachineUid> {
        let machines = &self.doc.project.machines;
        let is_sampler = |uid: &MachineUid| self.doc.project.sampler(*uid).is_some();
        self.view.focus.filter(is_sampler).or_else(|| {
            machines
                .iter()
                .find(|m| m.machine_type() == MachineType::Sampler)
                .map(|m| m.uid)
        })
    }

    /// Loads a file dropped on the window into sampler `uid`'s selected slot.
    pub(crate) fn load_dropped_file(&mut self, uid: MachineUid, file: &egui::DroppedFile) {
        let slot = self.view.sampler_slot.min(SLOTS - 1);
        if let Some(path) = &file.path {
            self.load_sample_file(uid, slot, path);
        } else if let Some(bytes) = &file.bytes {
            let ext = std::path::Path::new(&file.name)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_lowercase);
            let stem = std::path::Path::new(&file.name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.load_sample_bytes(uid, slot, &stem, ext.as_deref(), bytes);
        }
    }
}
