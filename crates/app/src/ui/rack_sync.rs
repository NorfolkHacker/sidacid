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
use acid_engine::host::{Command, Status};
use acid_engine::machine::{MAX_MACHINES, MachineType, MachineUid};
use acid_engine::params::ParamId;
use acid_engine::rack::RackSwap;

/// The rate fresh slots are built for when no stream runs (no engine to
/// match; the next stream builds its engine from the project).
const NO_AUDIO_RATE: f32 = 48_000.0;

impl SidAcidApp {
    /// Adds a fresh `ty` machine at the end of the rack (one undo step).
    /// None when the rack is full.
    #[allow(dead_code)] // called from the rack UI (Plan 11 Task 5)
    pub(crate) fn add_machine(&mut self, ty: MachineType) -> Option<MachineUid> {
        let (uid, cmds) = self.doc.add_machine(ty)?;
        self.send(cmds);
        self.sync_rack();
        Some(uid)
    }

    /// Adds a copy of `uid` directly below it (one undo step). None when
    /// the rack is full or `uid` is not in it.
    #[allow(dead_code)] // called from the rack UI (Plan 11 Task 5)
    pub(crate) fn duplicate_machine(&mut self, uid: MachineUid) -> Option<MachineUid> {
        let (copy, cmds) = self.doc.duplicate_machine(uid)?;
        self.send(cmds);
        self.sync_rack();
        Some(copy)
    }

    /// Removes `uid` at once (one undo step) and says how to get it back.
    #[allow(dead_code)] // called from the rack UI (Plan 11 Task 5)
    pub(crate) fn remove_machine(&mut self, uid: MachineUid) {
        let Some((name, cmds)) = self.doc.remove_machine(uid) else {
            return;
        };
        self.send(cmds);
        self.sync_rack();
        self.view.status_line = Some(format!("Removed '{name}' — Ctrl+Z to undo"));
    }

    /// Moves `uid` to rack position `to` (clamped).
    #[allow(dead_code)] // called from the rack UI (Plan 11 Task 5)
    pub(crate) fn move_machine(&mut self, uid: MachineUid, to: usize) {
        if self.doc.project.machine(uid).is_none() {
            return;
        }
        let cmds = self.doc.move_machine(uid, to);
        self.send(cmds);
        self.sync_rack();
    }

    /// Renames `uid`; an empty name becomes its type's default name.
    #[allow(dead_code)] // called from the rack UI (Plan 11 Task 5)
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
    /// engine's order and drops the UI state of machines that left.
    pub(crate) fn sync_rack(&mut self) {
        let rack = self.doc.project.rack_uids();
        if rack == self.engine_rack {
            return;
        }
        let rate = self.audio.as_ref().map_or(NO_AUDIO_RATE, |a| a.sample_rate);
        let swap = RackSwap::build(&self.doc.project, &self.engine_rack, rate);
        self.send(vec![Command::SetRack(swap)]);
        self.follow_rack(&rack);
        self.engine_rack = rack;
        self.rack_expected = self.rack_expected.wrapping_add(1);
        self.prune_ui_state();
    }

    /// A new host is coming (a new stream, or none): it starts with the
    /// project's rack at generation 0.
    pub(crate) fn rack_for_new_host(&mut self) {
        self.engine_rack = self.doc.project.rack_uids();
        self.rack_expected = 0;
        self.rack_reloaded_at = None;
    }

    /// A `ReplaceEngine` built from the project goes to the current host.
    pub(crate) fn rack_for_replaced_engine(&mut self) {
        self.engine_rack = self.doc.project.rack_uids();
        self.rack_expected = self.rack_expected.wrapping_add(1);
    }

    /// Clears `pending`, taking back the rack generations of the
    /// `SetRack`s that never reached the host.
    pub(crate) fn discard_pending(&mut self) {
        let swaps = self
            .pending
            .iter()
            .filter(|c| matches!(c, Command::SetRack(_)))
            .count() as u32;
        self.rack_expected = self.rack_expected.wrapping_sub(swaps);
        self.pending.clear();
    }

    /// The host refused a `SetRack` (the UI's idea of the engine's rack is
    /// wrong: a bug): rebuild the engine from the project, as a load does.
    /// Its generation is the refusing status's plus the replacement (the
    /// refused swap is assumed to be the last one sent). Further refusals
    /// reported at that same generation predate the reload and are ignored.
    pub(crate) fn reload_after_refused_rack(&mut self, status: &Status) {
        if self.rack_reloaded_at == Some(status.rack_gen) {
            return;
        }
        self.rack_reloaded_at = Some(status.rack_gen);
        // The new engine is built from the project: queued commands are in it.
        self.pending.clear();
        self.view.audition = None;
        self.view.pad_note = None;
        self.reset_transport_view();
        self.engine_rack = self.doc.project.rack_uids();
        self.rack_expected = status.rack_gen.wrapping_add(1);
        if let Some(audio) = self.audio.as_mut()
            && audio.load_project(&self.doc.project).is_err()
        {
            self.engine_busy_at_load();
        }
    }

    /// A status from a rack the engine has not caught up to (or has moved
    /// past) reports positions of another rack: keep the per-position
    /// arrays shown now and take the rest, master meter included.
    pub(crate) fn gate_status(&self, status: &mut Status) {
        if status.rack_gen == self.rack_expected {
            return;
        }
        let shown = &self.status;
        status.playhead = shown.playhead;
        status.patterns = shown.patterns;
        status.queued = shown.queued;
        let master = status.meters[MAX_MACHINES];
        status.meters = shown.meters;
        status.meters[MAX_MACHINES] = master;
        status.rack_gen = self.rack_expected;
    }

    /// Moves the shown per-position status from the engine's order to
    /// `rack`, so panels keep their playheads until the engine reports.
    fn follow_rack(&mut self, rack: &[MachineUid]) {
        let old = self.status;
        let s = &mut self.status;
        s.playhead = [0; MAX_MACHINES];
        s.patterns = [0; MAX_MACHINES];
        s.queued = [None; MAX_MACHINES];
        s.meters[..MAX_MACHINES].fill(0.0);
        for (i, uid) in rack.iter().enumerate().take(MAX_MACHINES) {
            if let Some(j) = self.engine_rack.iter().position(|u| u == uid) {
                s.playhead[i] = old.playhead[j];
                s.patterns[i] = old.patterns[j];
                s.queued[i] = old.queued[j];
                s.meters[i] = old.meters[j];
            }
        }
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
    }
}
