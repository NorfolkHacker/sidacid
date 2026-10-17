//! Keeping the engine's rack in step with the document: rack edits go to
//! the engine as one `Command::SetRack` each, statuses from an older rack
//! are kept out of the panels, and UI state of machines that left the rack
//! is dropped.
//!
//! `rack_expected` mirrors the host's rack counter (`Status::rack_gen`):
//! a new `AudioLink` is a new host (0); every `ReplaceEngine` and every
//! `SetRack` handed to the current host adds 1 (wrapping). With no audio
//! there is no host, but the count runs the same way (commands dropped
//! for want of a device count as taken): the next stream starts at 0
//! anyway. Commands discarded from `pending` before reaching a running
//! host are taken back out (`discard_pending`).

use super::SidAcidApp;
use super::sampler_load::{pool_message, sample_memory};
use acid_engine::host::{Command, Status};
use acid_engine::machine::{MachineType, MachineUid, rack_capacity};
use acid_engine::params::ParamId;
use acid_engine::project::BANK_SIZE;
use acid_engine::rack::RackSwap;
use acid_engine::sampler::SAMPLE_POOL;

/// The rate fresh slots are built for when no stream runs (no engine to
/// match; the next stream builds its engine from the project).
const NO_AUDIO_RATE: f32 = 48_000.0;

impl SidAcidApp {
    /// Adds a fresh `ty` machine at the end of the rack (one undo step)
    /// and focuses it. None when the rack has no free machine ids.
    pub(crate) fn add_machine(&mut self, ty: MachineType) -> Option<MachineUid> {
        let (uid, cmds) = self.doc.add_machine(ty)?;
        self.send(cmds);
        self.sync_rack();
        self.view.focus = Some(uid);
        Some(uid)
    }

    /// Adds a copy of `uid` directly below it (one undo step) and focuses
    /// it. None when the rack has no free machine ids or `uid` is not in it.
    pub(crate) fn duplicate_machine(&mut self, uid: MachineUid) -> Option<MachineUid> {
        if let Some(src) = self.doc.project.sampler(uid) {
            let needed = src.memory_bytes();
            let free = SAMPLE_POOL.saturating_sub(sample_memory(&self.doc.project));
            if needed > free {
                self.view.message = Some(pool_message(needed, free));
                return None;
            }
        }
        let (copy, cmds) = self.doc.duplicate_machine(uid)?;
        self.send(cmds);
        self.sync_rack();
        self.view.focus = Some(copy);
        Some(copy)
    }

    /// Removes `uid` at once (one undo step) and says how to get it back.
    pub(crate) fn remove_machine(&mut self, uid: MachineUid) {
        // A note tool open on it is cancelled first, so the removal (and
        // its undo) holds the pattern as it was.
        if self.tool_lane().is_some_and(|(u, _)| u == uid) {
            self.cancel_tool();
        }
        let Some((name, cmds)) = self.doc.remove_machine(uid) else {
            return;
        };
        self.send(cmds);
        self.sync_rack();
        self.view.status_line = Some(format!("Removed '{name}' — Ctrl+Z to undo"));
    }

    /// Moves `uid` to rack position `to` (clamped).
    pub(crate) fn move_machine(&mut self, uid: MachineUid, to: usize) {
        if self.doc.project.machine(uid).is_none() {
            return;
        }
        let cmds = self.doc.move_machine(uid, to);
        self.send(cmds);
        self.sync_rack();
    }

    /// Renames `uid`; an empty name becomes its type's default name.
    pub(crate) fn rename_machine(&mut self, uid: MachineUid, name: &str) {
        if self.doc.project.machine(uid).is_none() {
            return;
        }
        let cmds = self.doc.rename_machine(uid, name);
        self.send(cmds);
        self.sync_rack();
    }

    /// After any document change that can change the rack: if the rack
    /// differs from the engine's, sends one `SetRack` built against the
    /// engine's order, drops the UI state of machines that left and shows
    /// machines that (re)appear (added, duplicated, back from an undo) on
    /// the bank of their selected pattern, as a load does.
    pub(crate) fn sync_rack(&mut self) {
        let rack = self.doc.project.rack_uids();
        if rack == self.engine_rack {
            return;
        }
        let rate = self.audio.as_ref().map_or(NO_AUDIO_RATE, |a| a.sample_rate);
        let swap = RackSwap::build(
            &self.doc.project,
            &self.engine_rack,
            rate,
            self.engine_capacity,
        );
        self.engine_capacity = swap.capacity;
        self.pool.set_capacity(swap.capacity);
        self.send(vec![Command::SetRack(swap)]);
        self.follow_rack(&rack);
        for &uid in rack.iter().filter(|u| !self.engine_rack.contains(u)) {
            let bank = self.selected(uid) / BANK_SIZE as u8;
            self.view.panel(uid).bank = bank;
        }
        self.engine_rack = rack;
        self.rack_expected = self.rack_expected.wrapping_add(1);
        self.prune_ui_state();
    }

    /// A new host is coming (a new stream, or none): it starts with the
    /// project's rack at generation 0.
    pub(crate) fn rack_for_new_host(&mut self) {
        self.engine_rack = self.doc.project.rack_uids();
        self.rack_expected = 0;
        self.follow_engine_capacity();
        self.pool.reset();
    }

    /// A `ReplaceEngine` built from the project goes to the current host.
    pub(crate) fn rack_for_replaced_engine(&mut self) {
        self.engine_rack = self.doc.project.rack_uids();
        self.rack_expected = self.rack_expected.wrapping_add(1);
        self.follow_engine_capacity();
    }

    /// An engine built from the project holds the capacity of its rack;
    /// status buffers are sized to match (smaller spares come back and
    /// are replaced).
    fn follow_engine_capacity(&mut self) {
        self.engine_capacity = rack_capacity(self.doc.project.machines.len());
        self.pool.set_capacity(self.engine_capacity);
    }

    /// Clears `pending`, taking back the rack generations of the
    /// `SetRack`s that never reached the host. Status buffers stay: the
    /// host keeps them across engines, and the pool counts them as sent.
    pub(crate) fn discard_pending(&mut self) {
        let swaps = self
            .pending
            .iter()
            .filter(|c| matches!(c, Command::SetRack(_)))
            .count() as u32;
        self.rack_expected = self.rack_expected.wrapping_sub(swaps);
        self.pending
            .retain(|c| matches!(c, Command::StatusBuffer(_)));
    }

    /// The host refused a `SetRack` (the UI's idea of the engine's rack is
    /// wrong: a bug): restart audio, so a fresh host builds its engine from
    /// the project and the host's rack count and ours both start again at
    /// 0. Without a stream (tests) the same reset happens with no device.
    /// The old host's statuses are gone after the restart, so every refusal
    /// seen is a new host's own and restarts once.
    pub(crate) fn reload_after_refused_rack(&mut self, _status: &Status) -> bool {
        self.view.audition = None;
        self.view.pad_note = None;
        if self.audio.is_some() {
            self.start_audio();
        } else {
            self.reset_for_new_host();
        }
        true
    }

    /// A status from a rack the engine has not caught up to (or has moved
    /// past) reports positions of another rack: `on_status` keeps the
    /// per-position values shown now and takes the rest, master meter
    /// included.
    pub(crate) fn gate_status(&self, status: &mut Status) {
        status.rack_gen = self.rack_expected;
    }

    /// Moves the shown per-position status from the engine's order to
    /// `rack`, so panels keep their playheads until the engine reports.
    fn follow_rack(&mut self, rack: &[MachineUid]) {
        self.shown.follow(&self.engine_rack, rack);
    }

    /// Drops UI state kept for machines no longer in the rack.
    fn prune_ui_state(&mut self) {
        let project = &self.doc.project;
        let live = |uid: MachineUid| project.machine(uid).is_some();
        let live_param = |id: &ParamId| match id {
            ParamId::Machine(uid, _) => live(*uid),
            _ => true,
        };
        let v = &mut self.view;
        v.panels.retain(|&uid, _| live(uid));
        v.wave_cache.retain(|&(uid, _), _| live(uid));
        v.wave_drag = v.wave_drag.filter(|&(uid, ..)| live(uid));
        v.focus = v.focus.filter(|&uid| live(uid));
        v.audition = v.audition.filter(|&(uid, _)| live(uid));
        v.pad_note = v.pad_note.filter(|&(uid, _)| live(uid));
        v.knob_scroll = v.knob_scroll.take().filter(|(id, _)| live_param(id));
        v.touched_knob = v.touched_knob.filter(live_param);
        v.engine_touch = v.engine_touch.filter(live_param);
        v.rec_last = v.rec_last.filter(|(id, _)| live_param(id));
        v.live.retain(|id, _| live_param(id));
        v.renaming = v.renaming.take().filter(|(uid, _)| live(*uid));
        v.perf.retain_machines(live);
    }
}
