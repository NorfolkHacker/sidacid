//! The whole groovebox: a rack of up to `MAX_MACHINES` slots (each a
//! machine's DSP, knobs, patterns and channel strip), the shared send
//! effects and master bus, and the sequencer, rendered sample-accurately.
//! process() is real-time safe: no allocation, locking or I/O.

use crate::machine::{MACHINES, MAX_MACHINES, MachineUid, standard_rack};
use crate::mixer::Mixer;
use crate::params::{GlobalParam, MachineParam, ParamId};
use crate::project::{DrumPattern, MAX_NOTES, NotePattern, PATTERNS, Project, Song, SynthPattern};
use crate::rack::{MachineDsp, Slot, SlotPatterns};
use crate::sampler::Sampler;
use crate::sampler::sample::SampleData;
use crate::sampler::slot::SlotPlay;
use crate::sequencer::{Mode, SeqEvent, Sequencer};
use crate::song::{Banks, MAX_BARS};
use std::sync::Arc;

/// Upper bound on sequencer ticks fired without time advancing; guards
/// against a sequencer that never moves forward.
const MAX_TICKS_PER_POSITION: usize = 64;
const GLOBALS: usize = GlobalParam::COUNT;

/// Where a param's knob lives.
#[derive(Clone, Copy)]
enum Loc {
    /// Slot index, knob index.
    Slot(usize, usize),
    Global(usize),
}

/// The standard six slots' banks, for the six-track sequencer (interim
/// until the sequencer is keyed by uid). Only valid for the standard rack.
fn standard_banks(slots: &[Slot]) -> Banks<'_> {
    fn synth(s: &Slot) -> &[SynthPattern] {
        match &s.patterns {
            SlotPatterns::Synth(v) => v,
            _ => &[],
        }
    }
    fn drum(s: &Slot) -> &[DrumPattern] {
        match &s.patterns {
            SlotPatterns::Drum(v) => v,
            _ => &[],
        }
    }
    fn notes(s: &Slot) -> &[NotePattern] {
        match &s.patterns {
            SlotPatterns::Notes(v) => v,
            _ => &[],
        }
    }
    Banks {
        synths: [synth(&slots[0]), synth(&slots[1])],
        drums: [drum(&slots[2]), drum(&slots[3])],
        notes: [notes(&slots[4]), notes(&slots[5])],
    }
}

pub struct Engine {
    sample_rate: f32,
    /// The rack, in order; capacity `MAX_MACHINES` so it never grows.
    slots: Vec<Slot>,
    /// Send effects and master bus.
    mixer: Mixer,
    seq: Sequencer,
    events: Vec<SeqEvent>,
    /// `GlobalParam::all()`, so globals re-apply without allocating.
    global_ids: [GlobalParam; GLOBALS],
    global_knobs: [f32; GLOBALS],
    global_base: [f32; GLOBALS],
    global_touched: [bool; GLOBALS],
    /// Every knob in the rack plus the globals: bounds song lanes.
    param_count: usize,
    /// Interim (until the sequencer is keyed by uid): the rack is today's
    /// six machines in order, so the six-track sequencer can play it.
    /// Otherwise slots render but no patterns play.
    standard_layout: bool,
    guard_fired: bool,
    song: Song,
    /// The song ended by itself and no transport command came since: the
    /// next `stop` counts as the first one (it only releases).
    ended_by_song: bool,
}

impl Engine {
    pub const MAX_BLOCK: usize = 512;

    pub fn new(sample_rate: f32) -> Self {
        Self::from_project(sample_rate, &Project::default())
    }

    /// Builds an engine with a project loaded. Allocates; call off the
    /// audio thread.
    pub fn from_project(sample_rate: f32, p: &Project) -> Engine {
        let global_ids: [GlobalParam; GLOBALS] = std::array::from_fn(|i| GlobalParam::all()[i]);
        let defaults = global_ids.map(|g| g.info().default);
        let mut e = Self {
            sample_rate,
            slots: Vec::with_capacity(MAX_MACHINES),
            mixer: Mixer::new(sample_rate, Self::MAX_BLOCK),
            seq: Sequencer::new(sample_rate),
            events: Vec::new(),
            global_ids,
            global_knobs: defaults,
            global_base: defaults,
            global_touched: [false; GLOBALS],
            param_count: GLOBALS,
            standard_layout: false,
            guard_fired: false,
            song: Song::default(),
            ended_by_song: false,
        };
        e.load_project(p);
        e
    }

    /// Rebuilds the rack from `p` (one slot per machine, in rack order, at
    /// most `MAX_MACHINES`) and loads its globals, song, tempo and pattern
    /// selection. Not real-time safe: call while stopped or from a
    /// non-audio thread before handing the engine to the audio thread.
    pub fn load_project(&mut self, p: &Project) {
        self.slots.clear();
        self.slots.extend(
            p.machines
                .iter()
                .take(MAX_MACHINES)
                .map(|m| Slot::from_entry(m, self.sample_rate)),
        );
        let standard = standard_rack();
        self.standard_layout = self.slots.len() == standard.len()
            && self
                .slots
                .iter()
                .zip(standard)
                .all(|(s, (uid, ty, _))| s.uid == uid && s.ty == ty);
        self.param_count = GLOBALS + self.slots.iter().map(|s| s.knobs.len()).sum::<usize>();
        // Room for every param (automation and lane events), plus per
        // machine 128 releases and MAX_NOTES starts: never grows on the
        // audio thread.
        let reserve = 64 + 2 * self.param_count + MAX_MACHINES * (128 + MAX_NOTES);
        self.events = Vec::with_capacity(reserve);
        for i in 0..GLOBALS {
            let id = ParamId::from(self.global_ids[i]);
            self.set_param(id, p.param(id));
        }
        self.song = p.song.clone();
        self.set_bpm(p.bpm);
        self.set_swing(p.swing);
        if self.standard_layout {
            for (i, m) in p.machines.iter().enumerate() {
                self.seq.select(i, m.selected);
            }
        }
    }

    /// The rack's uids in order.
    pub fn slot_uids(&self) -> Vec<MachineUid> {
        self.slots.iter().map(|s| s.uid).collect()
    }

    /// How many slots the rack holds without reallocating.
    pub fn slot_capacity(&self) -> usize {
        self.slots.capacity()
    }

    /// The saved knob position of a param; `None` for a uid not in the
    /// rack or a param that doesn't fit its machine.
    pub fn knob(&self, id: ParamId) -> Option<f32> {
        Some(match self.locate(id)? {
            Loc::Slot(s, i) => self.slots[s].knobs[i],
            Loc::Global(g) => self.global_knobs[g],
        })
    }

    fn locate(&self, id: ParamId) -> Option<Loc> {
        match id {
            ParamId::Machine(uid, p) => {
                let s = self.slots.iter().position(|s| s.uid == uid)?;
                Some(Loc::Slot(s, self.slots[s].index_of(p)?))
            }
            ParamId::Fx(_) | ParamId::Master(_) => Some(Loc::Global(id.global()?.index())),
        }
    }

    fn base_mut(&mut self, loc: Loc) -> &mut f32 {
        match loc {
            Loc::Slot(s, i) => &mut self.slots[s].base[i],
            Loc::Global(g) => &mut self.global_base[g],
        }
    }

    fn knob_at(&self, loc: Loc) -> f32 {
        match loc {
            Loc::Slot(s, i) => self.slots[s].knobs[i],
            Loc::Global(g) => self.global_knobs[g],
        }
    }

    fn touched_at(&self, loc: Loc) -> bool {
        match loc {
            Loc::Slot(s, i) => self.slots[s].touched[i],
            Loc::Global(g) => self.global_touched[g],
        }
    }

    /// Out-of-range ids and non-finite values are ignored; values are
    /// clamped to 0..=1. The value is remembered as the knob position.
    pub fn set_param(&mut self, id: ParamId, v: f32) {
        let Some(loc) = self.locate(id) else { return };
        if !v.is_finite() {
            return;
        }
        let v = v.clamp(0.0, 1.0);
        match loc {
            Loc::Slot(s, i) => self.slots[s].knobs[i] = v,
            Loc::Global(g) => self.global_knobs[g] = v,
        }
        *self.base_mut(loc) = v;
        self.apply_at(loc, v);
    }

    /// Sends a (clamped) value to the slot's DSP or strip, or the mixer.
    fn apply_at(&mut self, loc: Loc, v: f32) {
        match loc {
            Loc::Slot(s, i) => {
                let slot = &mut self.slots[s];
                let p: MachineParam = slot.params[i];
                slot.apply(p, v);
            }
            Loc::Global(g) => self.mixer.set_global(self.global_ids[g], v),
        }
    }

    /// Applies a value without remembering it (param-lane overrides).
    fn apply_param(&mut self, id: ParamId, v: f32) {
        let Some(loc) = self.locate(id) else { return };
        if !v.is_finite() {
            return;
        }
        self.apply_at(loc, v.clamp(0.0, 1.0));
    }

    fn restore_param(&mut self, id: ParamId) {
        if let Some(loc) = self.locate(id) {
            let v = *self.base_mut(loc);
            self.apply_at(loc, v);
        }
    }

    fn restore_all_knobs(&mut self) {
        for s in 0..self.slots.len() {
            for i in 0..self.slots[s].knobs.len() {
                let v = self.slots[s].knobs[i];
                self.apply_at(Loc::Slot(s, i), v);
            }
        }
        for g in 0..GLOBALS {
            self.apply_at(Loc::Global(g), self.global_knobs[g]);
        }
    }

    fn samplers(&mut self) -> impl Iterator<Item = &mut Sampler> {
        self.slots.iter_mut().filter_map(|s| match &mut s.dsp {
            MachineDsp::Sampler(x) => Some(&mut **x),
            _ => None,
        })
    }

    /// The first Sampler in the rack (today's single Sampler machine).
    fn first_sampler(&mut self) -> Option<&mut Sampler> {
        self.samplers().next()
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.seq.set_bpm(bpm);
        let bpm = self.seq.bpm();
        self.mixer.set_bpm(bpm);
        for s in self.samplers() {
            s.set_bpm(bpm);
        }
    }

    /// Exchanges a slot's audio on the rack's first Sampler (real-time
    /// safe; the old sample comes back for disposal off the audio thread).
    pub fn swap_sampler_sample(
        &mut self,
        slot: usize,
        sample: &mut Option<Arc<SampleData>>,
    ) -> bool {
        self.first_sampler()
            .is_some_and(|s| s.swap_sample(slot, sample))
    }

    pub fn swap_sampler_play(&mut self, slot: usize, play: &mut SlotPlay) -> bool {
        self.first_sampler()
            .is_some_and(|s| s.swap_play(slot, play))
    }

    pub fn set_swing(&mut self, swing: f32) {
        self.seq.set_swing(swing);
    }

    /// Releases the note machines' pattern notes (auditions keep sounding).
    fn note_machines_pattern_off(&mut self) {
        for s in &mut self.slots {
            match &mut s.dsp {
                MachineDsp::Fm(f) => f.pattern_off(),
                MachineDsp::Sampler(x) => x.pattern_off(),
                _ => {}
            }
        }
    }

    /// Starts (or restarts) the transport. Pattern notes still sounding
    /// are released; auditioned notes keep sounding.
    pub fn play(&mut self) {
        self.ended_by_song = false;
        self.seq.play();
        // The sequencer drops its held-note masks without emitting releases.
        self.note_machines_pattern_off();
    }

    /// Audition: starts a note on the FM or Sampler machine at rack
    /// position `machine` right away, playing or not. Other machines,
    /// pitches above 127 and velocity 0 are ignored.
    pub fn note_on(&mut self, machine: usize, pitch: u8, vel: u8) {
        self.poly_on(machine, pitch, vel, true);
    }

    /// Audition release: only releases auditioned notes.
    pub fn note_off(&mut self, machine: usize, pitch: u8) {
        self.poly_off(machine, pitch, true);
    }

    fn poly_on(&mut self, machine: usize, pitch: u8, vel: u8, live: bool) {
        if pitch > 127 || vel == 0 {
            return;
        }
        match self.slots.get_mut(machine).map(|s| &mut s.dsp) {
            Some(MachineDsp::Fm(f)) => f.note_on(pitch, vel.min(127), live),
            Some(MachineDsp::Sampler(s)) => s.note_on(pitch, vel.min(127), live),
            _ => {}
        }
    }

    fn poly_off(&mut self, machine: usize, pitch: u8, live: bool) {
        match self.slots.get_mut(machine).map(|s| &mut s.dsp) {
            Some(MachineDsp::Fm(f)) => f.note_off(pitch, live),
            Some(MachineDsp::Sampler(s)) => s.note_off(pitch, live),
            _ => {}
        }
    }

    /// Stops the transport and releases the pattern's notes (tails ring
    /// out; auditioned notes keep sounding). Stopping while already stopped
    /// also silences the FM and Sampler machines at once, auditions
    /// included, so drones and loops can be cut. The first stop after a
    /// song ends by itself still only releases: the song end is not a stop.
    pub fn stop(&mut self) {
        let cut = !self.seq.is_playing() && !self.ended_by_song;
        self.ended_by_song = false;
        if cut {
            for s in &mut self.slots {
                if matches!(s.dsp, MachineDsp::Fm(_) | MachineDsp::Sampler(_)) {
                    s.reset();
                }
            }
        }
        self.end_playback();
    }

    /// Stops the transport, closes the bass synths' gates and releases the
    /// note machines' pattern notes (auditions keep sounding).
    fn end_playback(&mut self) {
        self.seq.stop();
        for s in &mut self.slots {
            if let MachineDsp::Synth(x) = &mut s.dsp {
                x.note_off();
            }
        }
        self.note_machines_pattern_off();
        for s in &mut self.slots {
            s.base.copy_from_slice(&s.knobs);
            s.touched.fill(false);
        }
        self.global_base = self.global_knobs;
        self.global_touched = [false; GLOBALS];
        self.restore_all_knobs();
    }

    /// Plays a value without changing the saved knob (record mode).
    pub fn set_param_live(&mut self, id: ParamId, v: f32) {
        let Some(loc) = self.locate(id) else { return };
        if !v.is_finite() {
            return;
        }
        let v = v.clamp(0.0, 1.0);
        *self.base_mut(loc) = v;
        self.apply_at(loc, v);
    }

    /// While touched, song automation for `id` is not applied.
    pub fn touch(&mut self, id: ParamId, on: bool) {
        match self.locate(id) {
            Some(Loc::Slot(s, i)) => self.slots[s].touched[i] = on,
            Some(Loc::Global(g)) => self.global_touched[g] = on,
            None => {}
        }
    }

    fn apply_automation(&mut self, id: ParamId, v: f32) {
        let Some(loc) = self.locate(id) else { return };
        if self.touched_at(loc) || !v.is_finite() {
            return;
        }
        let v = v.clamp(0.0, 1.0);
        *self.base_mut(loc) = v;
        self.apply_at(loc, v);
    }

    /// No automation applies here yet: back to the knob (unless held).
    fn reset_automation(&mut self, id: ParamId) {
        let Some(loc) = self.locate(id) else { return };
        if self.touched_at(loc) {
            return;
        }
        let v = self.knob_at(loc);
        *self.base_mut(loc) = v;
        self.apply_at(loc, v);
    }

    /// Swaps in a new song (real-time safe). An invalid song leaves both
    /// sides untouched and returns false.
    pub fn swap_song(&mut self, song: &mut Song) -> bool {
        let refs_ok = song
            .bars
            .iter()
            .flat_map(|b| &b.cells)
            .filter_map(|(_, c)| c.pattern)
            .all(|r| (r.0 as usize) < PATTERNS);
        let lanes_ok = song.automation.len() <= self.param_count
            && song
                .automation
                .iter()
                .all(|l| self.locate(l.param).is_some());
        if song.bars.len() > MAX_BARS || !refs_ok || !lanes_ok {
            return false;
        }
        std::mem::swap(&mut self.song, song);
        self.seq.resync();
        true
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.seq.set_mode(mode);
    }

    pub fn mode(&self) -> Mode {
        self.seq.mode()
    }

    pub fn set_song_start(&mut self, bar: usize) {
        self.seq.set_song_start(bar);
    }

    pub fn song_bar(&self) -> usize {
        self.seq.song_bar()
    }

    pub fn song_step(&self) -> u32 {
        self.seq.song_step()
    }

    /// The value a param currently plays at (knob, automation or live).
    #[cfg(test)]
    pub(crate) fn live_param(&self, id: ParamId) -> f32 {
        match self.locate(id) {
            Some(Loc::Slot(s, i)) => self.slots[s].base[i],
            Some(Loc::Global(g)) => self.global_base[g],
            None => 0.0,
        }
    }

    /// Whether a knob is held against automation.
    #[cfg(test)]
    pub(crate) fn is_touched(&self, id: ParamId) -> bool {
        self.locate(id).is_some_and(|l| self.touched_at(l))
    }

    #[cfg(test)]
    pub(crate) fn swing(&self) -> f32 {
        self.seq.swing()
    }

    pub fn is_playing(&self) -> bool {
        self.seq.is_playing()
    }

    /// Ignores `machine >= MACHINES` and `slot >= 128`.
    pub fn select_pattern(&mut self, machine: usize, slot: u8) {
        if machine < MACHINES && (slot as usize) < PATTERNS {
            self.seq.select(machine, slot);
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// The `n`th slot (in rack order) whose patterns are of the kind
    /// `pick` accepts.
    fn nth_bank<T>(
        &mut self,
        n: usize,
        pick: fn(&mut SlotPatterns) -> Option<&mut Vec<T>>,
    ) -> Option<&mut Vec<T>> {
        self.slots
            .iter_mut()
            .filter_map(|s| pick(&mut s.patterns))
            .nth(n)
    }

    /// Swaps `p` with the stored pattern of the `synth`th 303 (real-time
    /// safe). An invalid target or malformed pattern leaves both untouched
    /// and returns false.
    pub fn swap_synth_pattern(&mut self, synth: usize, slot: u8, p: &mut SynthPattern) -> bool {
        if slot as usize >= PATTERNS || !p.is_well_formed() {
            return false;
        }
        let bank = self.nth_bank(synth, |b| match b {
            SlotPatterns::Synth(v) => Some(v),
            _ => None,
        });
        let Some(stored) = bank.and_then(|b| b.get_mut(slot as usize)) else {
            return false;
        };
        std::mem::swap(stored, p);
        true
    }

    /// Like `swap_synth_pattern`, on the `kit`th drum kit.
    pub fn swap_drum_pattern(&mut self, kit: usize, slot: u8, p: &mut DrumPattern) -> bool {
        if slot as usize >= PATTERNS || !p.is_well_formed() {
            return false;
        }
        let bank = self.nth_bank(kit, |b| match b {
            SlotPatterns::Drum(v) => Some(v),
            _ => None,
        });
        let Some(stored) = bank.and_then(|b| b.get_mut(slot as usize)) else {
            return false;
        };
        std::mem::swap(stored, p);
        true
    }

    /// Stores `p` and returns the old pattern. An invalid target or a
    /// malformed pattern is not stored: `p` is handed back unchanged.
    pub fn set_synth_pattern(
        &mut self,
        synth: usize,
        slot: u8,
        mut p: SynthPattern,
    ) -> SynthPattern {
        self.swap_synth_pattern(synth, slot, &mut p);
        p
    }

    pub fn set_drum_pattern(&mut self, kit: usize, slot: u8, mut p: DrumPattern) -> DrumPattern {
        self.swap_drum_pattern(kit, slot, &mut p);
        p
    }

    /// Swaps `p` into the note machine (FM or Sampler) at rack position
    /// `machine`. Real-time safe. An invalid target or malformed pattern
    /// leaves both untouched and returns false.
    pub fn swap_note_pattern(&mut self, machine: usize, slot: u8, p: &mut NotePattern) -> bool {
        if slot as usize >= PATTERNS || !p.is_well_formed() {
            return false;
        }
        let Some(SlotPatterns::Notes(bank)) = self.slots.get_mut(machine).map(|s| &mut s.patterns)
        else {
            return false;
        };
        let Some(stored) = bank.get_mut(slot as usize) else {
            return false;
        };
        std::mem::swap(stored, p);
        true
    }

    pub fn set_note_pattern(
        &mut self,
        machine: usize,
        slot: u8,
        mut p: NotePattern,
    ) -> NotePattern {
        self.swap_note_pattern(machine, slot, &mut p);
        p
    }

    pub fn queued_patterns(&self) -> [Option<u8>; MACHINES] {
        std::array::from_fn(|m| self.seq.queued_pattern(m))
    }

    /// See `Mixer::take_meters`: by rack position, master at `MAX_MACHINES`.
    pub fn take_meters(&mut self) -> [f32; MAX_MACHINES + 1] {
        self.mixer.take_meters()
    }

    /// True once if the mixer's NaN guard fired since the last call.
    pub fn take_guard_fired(&mut self) -> bool {
        std::mem::take(&mut self.guard_fired)
    }

    pub fn playhead(&self) -> [usize; MACHINES] {
        std::array::from_fn(|m| self.seq.playhead(m))
    }

    pub fn current_patterns(&self) -> [u8; MACHINES] {
        std::array::from_fn(|m| self.seq.current_pattern(m))
    }

    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        debug_assert_eq!(out_l.len(), out_r.len());
        let total = out_l.len().min(out_r.len());
        let mut offset = 0;
        while offset < total {
            let n = (total - offset).min(Self::MAX_BLOCK);
            self.process_block(
                &mut out_l[offset..offset + n],
                &mut out_r[offset..offset + n],
            );
            offset += n;
        }
    }

    /// Applies one sequencer event. Machine indices are rack positions of
    /// the standard layout (the only layout the sequencer plays for now).
    fn handle_event(&mut self, ev: SeqEvent) {
        match ev {
            SeqEvent::Note {
                synth,
                midi,
                accent,
                legato,
            } => {
                if let Some(MachineDsp::Synth(s)) = self.slots.get_mut(synth).map(|s| &mut s.dsp) {
                    s.note_on(midi, accent, legato);
                }
            }
            SeqEvent::NoteOff { synth } => {
                if let Some(MachineDsp::Synth(s)) = self.slots.get_mut(synth).map(|s| &mut s.dsp) {
                    s.note_off();
                }
            }
            SeqEvent::Drum { kit, inst, accent } => {
                if let Some(MachineDsp::Kit(k)) = self.slots.get_mut(2 + kit).map(|s| &mut s.dsp) {
                    k.trigger(inst, accent);
                }
            }
            SeqEvent::Param { id, value } => self.apply_param(id, value),
            SeqEvent::ParamRestore { id } => self.restore_param(id),
            SeqEvent::Automation { id, value } => self.apply_automation(id, value),
            SeqEvent::AutomationReset { id } => self.reset_automation(id),
            SeqEvent::PolyOn {
                machine,
                pitch,
                vel,
            } => self.poly_on(machine, pitch, vel, false),
            SeqEvent::PolyOff { machine, pitch } => self.poly_off(machine, pitch, false),
        }
    }

    fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len();
        for s in &mut self.slots {
            s.buf.l[..n].fill(0.0);
            s.buf.r[..n].fill(0.0);
        }
        let mut pos = 0;
        let mut ticks_here = 0;
        while pos < n {
            if !self.standard_layout {
                // Interim: the six-track sequencer can't play this rack.
                for s in &mut self.slots {
                    s.render(pos..n);
                }
                break;
            }
            let until = self.seq.samples_to_next_tick();
            if until == 0 && ticks_here < MAX_TICKS_PER_POSITION {
                ticks_here += 1;
                self.events.clear();
                self.seq
                    .tick(standard_banks(&self.slots), &self.song, &mut self.events);
                for i in 0..self.events.len() {
                    let ev = self.events[i];
                    self.handle_event(ev);
                }
                if self.seq.take_ended() {
                    self.end_playback();
                    self.ended_by_song = true;
                }
                continue;
            }
            // Past the tick bound (a broken tempo) just render on.
            ticks_here = 0;
            let run = if until == 0 {
                n - pos
            } else {
                until.min(n - pos)
            };
            for s in &mut self.slots {
                s.render(pos..pos + run);
            }
            self.seq.advance(run);
            pos += run;
        }
        if self.mixer.process(&mut self.slots, n, out_l, out_r) {
            self.guard_fired = true;
            for s in &mut self.slots {
                s.reset();
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one field of a default pattern
mod tests {
    use super::*;
    use crate::fm::FmSynth;
    use crate::machine::{MAX_MACHINES, MachineId, MachineType, MachineUid};
    use crate::params::{
        ChannelParam, DrumParam, FmParam, KitParam, MasterParam, OpParam, SynthParam,
        channel_param, drum_param, fm_op_param, fm_param, kit_param, slot_param, synth_param,
    };
    use crate::project::MachineEntry;
    use crate::testutil::{all_finite, peak, rms};

    #[test]
    fn every_param_of_the_standard_rack_has_its_own_knob() {
        let mut e = Engine::new(SR);
        let ids: Vec<ParamId> = crate::machine::standard_rack()
            .into_iter()
            .flat_map(|(uid, ty, _)| {
                MachineParam::all_for(ty)
                    .into_iter()
                    .map(move |p| ParamId::Machine(uid, p))
            })
            .chain(GlobalParam::all().into_iter().map(ParamId::from))
            .collect();
        assert_eq!(ids.len(), 589, "the old flat table's size");
        assert_eq!(e.param_count, ids.len());
        for (n, &id) in ids.iter().enumerate() {
            e.set_param(id, (n % 97) as f32 / 97.0);
        }
        for (n, &id) in ids.iter().enumerate() {
            assert_eq!(e.knob(id), Some((n % 97) as f32 / 97.0), "{id:?}");
        }
        assert_eq!(e.knob(synth_param(2, SynthParam::Tune)), None);
        assert_eq!(e.knob(drum_param(0, 8, DrumParam::Tune)), None);
        assert_eq!(e.knob(fm_op_param(6, OpParam::On)), None);
        assert_eq!(e.knob(channel_param(6, ChannelParam::Level)), None);
    }

    #[test]
    fn a_non_standard_rack_renders_but_plays_no_patterns() {
        let mut p = crate::demo::demo_project();
        p.machines.swap(0, 1);
        let mut e = Engine::from_project(SR, &p);
        assert!(!e.standard_layout);
        e.play();
        let (l, r) = render(&mut e, 0.5);
        assert!(all_finite(&l) && all_finite(&r));
        assert_eq!(
            peak(&l),
            0.0,
            "no patterns play until the sequencer is keyed by uid"
        );
        let mut s = Engine::from_project(SR, &crate::demo::demo_project());
        assert!(s.standard_layout);
        s.play();
        let (l, _) = render(&mut s, 0.5);
        assert!(peak(&l) > 0.01, "the standard rack sequences");
    }

    #[test]
    fn any_rack_layout_builds_and_renders() {
        let mut p = Project::default();
        p.machines.swap(0, 3); // 909 first
        let e = MachineEntry::new(MachineUid(7), MachineType::Sampler, "Sampler 2".into());
        p.machines.push(e);
        p.next_uid = 8;
        p.validate().unwrap();
        let mut eng = Engine::from_project(48000.0, &p);
        assert_eq!(
            eng.slot_uids(),
            vec![4, 2, 3, 1, 5, 6, 7]
                .into_iter()
                .map(MachineUid)
                .collect::<Vec<_>>()
        );
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        eng.process(&mut l, &mut r);
    }

    #[test]
    fn params_reach_the_slot_with_that_uid() {
        let mut eng = Engine::new(48000.0);
        let id = ParamId::Machine(MachineUid(2), MachineParam::Synth(SynthParam::Cutoff));
        eng.set_param(id, 0.123);
        assert_eq!(eng.knob(id), Some(0.123));
        let other = ParamId::Machine(MachineUid(1), MachineParam::Synth(SynthParam::Cutoff));
        assert_ne!(eng.knob(other), Some(0.123));
        assert_eq!(
            eng.knob(ParamId::Machine(
                MachineUid(99),
                MachineParam::Synth(SynthParam::Cutoff)
            )),
            None
        );
        // A param that doesn't fit the machine type is ignored.
        eng.set_param(
            ParamId::Machine(MachineUid(2), MachineParam::Fm(FmParam::Volume)),
            0.5,
        );
    }

    #[test]
    fn sixteen_machines_fit_without_reallocating() {
        let mut p = Project::default();
        for u in 7..=16u16 {
            p.machines.push(MachineEntry::new(
                MachineUid(u),
                MachineType::Synth,
                format!("Synth {u}"),
            ));
        }
        p.next_uid = 17;
        p.validate().unwrap();
        let eng = Engine::from_project(48000.0, &p);
        assert_eq!(eng.slot_capacity(), MAX_MACHINES);
        let mut too_many = p.clone();
        too_many.machines.push(MachineEntry::new(
            MachineUid(17),
            MachineType::Fm,
            "FM 2".into(),
        ));
        too_many.next_uid = 18;
        assert!(too_many.validate().is_err());
    }

    const SR: f32 = 44_100.0;

    impl Engine {
        fn fm(&self) -> &FmSynth {
            self.slots
                .iter()
                .find_map(|s| match &s.dsp {
                    MachineDsp::Fm(f) => Some(&**f),
                    _ => None,
                })
                .unwrap()
        }

        fn sampler(&self) -> &Sampler {
            self.slots
                .iter()
                .find_map(|s| match &s.dsp {
                    MachineDsp::Sampler(x) => Some(&**x),
                    _ => None,
                })
                .unwrap()
        }
    }

    fn render(e: &mut Engine, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        e.process(&mut l, &mut r);
        (l, r)
    }

    fn demo_engine() -> Engine {
        let mut e = Engine::new(SR);
        e.load_project(&crate::demo::demo_project());
        e
    }

    /// A one-second mono 44.1 kHz sine slot laid out by `meta`.
    fn tone_slot(meta: crate::sampler::slot::SlotMeta) -> crate::sampler::slot::SlotData {
        let tone: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
        crate::sampler::slot::SlotData::new("Tone", 44_100, 1, &tone, meta)
    }

    fn sampler_project() -> Project {
        use crate::project::Note;
        use crate::sampler::slot::SlotMeta;
        let mut p = Project::default();
        p.sampler_mut(MachineId::Sampler.uid()).unwrap().slots[0] = Some(tone_slot(
            SlotMeta::sliced(36, vec![0, 11_025, 22_050, 33_075], 120.0, 44_100),
        ));
        p.sampler_mut(MachineId::Sampler.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 4,
                pitch: 37,
                vel: 120,
            });
        p
    }

    #[test]
    fn the_sampler_plays_loaded_slots_on_channel_5() {
        let mut e = Engine::from_project(SR, &sampler_project());
        e.play();
        render(&mut e, 0.2);
        let m = e.take_meters();
        assert!(m[5] > 0.01, "{m:?}");
        assert!(m[..5].iter().all(|v| *v == 0.0), "{m:?}");
    }

    #[test]
    fn sampler_audition_and_slot_swaps() {
        let mut e = Engine::from_project(SR, &sampler_project());
        e.note_on(5, 38, 100);
        let (l, _) = render(&mut e, 0.1);
        assert!(peak(&l) > 0.01, "audition plays slice 3");
        let mut none = None;
        assert!(e.swap_sampler_sample(0, &mut none));
        assert!(none.is_some());
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-3, "slot emptied");
        let mut play = crate::sampler::slot::SlotPlay::EMPTY;
        assert!(e.swap_sampler_play(0, &mut play));
        assert!(!e.swap_sampler_play(16, &mut play));
    }

    #[test]
    fn stopping_twice_silences_sampler_loops() {
        use crate::sampler::slot::SlotMeta;
        let mut p = Project::default();
        let mut meta = SlotMeta::chromatic(48, 71, 60, 44_100);
        meta.loop_on = true;
        meta.loop_start = 10_000;
        meta.loop_end = 30_000;
        p.sampler_mut(MachineId::Sampler.uid()).unwrap().slots[0] = Some(tone_slot(meta));
        p.set_param(slot_param(0, crate::params::SlotParam::Release), 1.0);
        let mut e = Engine::from_project(SR, &p);
        e.note_on(5, 60, 100);
        render(&mut e, 0.1);
        e.stop();
        e.stop();
        let (l, _) = render(&mut e, 0.05);
        assert_eq!(peak(&l), 0.0);
    }

    /// A project whose sampler slot 0 holds a chromatic tone (keys 36..=96).
    fn tone_slot_project() -> Project {
        let mut p = Project::default();
        p.sampler_mut(MachineId::Sampler.uid()).unwrap().slots[0] = Some(tone_slot(
            crate::sampler::slot::SlotMeta::chromatic(36, 96, 60, 44_100),
        ));
        p
    }

    #[test]
    fn note_patterns_play_on_their_own_channels() {
        use crate::project::Note;
        let mut p = tone_slot_project();
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 32,
                pitch: 60,
                vel: 120,
            });
        p.sampler_mut(MachineId::Sampler.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 32,
                pitch: 72,
                vel: 120,
            });
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.5);
        let m = e.take_meters();
        assert!(m[4] > 0.01 && m[5] > 0.01, "{m:?}");
        assert!(m[..4].iter().all(|v| *v == 0.0), "{m:?}");
    }

    #[test]
    fn live_notes_audition_while_stopped() {
        let mut e = Engine::from_project(SR, &tone_slot_project());
        e.note_on(0, 60, 100);
        e.note_on(4, 200, 100);
        e.note_on(5, 60, 0);
        let (l, _) = render(&mut e, 0.05);
        assert_eq!(peak(&l), 0.0, "bad machine, pitch and velocity are ignored");
        e.note_on(5, 60, 100);
        let (l, _) = render(&mut e, 0.1);
        assert!(peak(&l) > 0.01);
        e.note_off(5, 60);
        render(&mut e, 0.3);
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-4);
    }

    #[test]
    fn stop_releases_note_machines() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 64,
                pitch: 60,
                vel: 120,
            });
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.2);
        e.stop();
        render(&mut e, 0.3);
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-4);
    }

    #[test]
    fn stopping_twice_silences_fm_drones() {
        use crate::params::OpParam;
        let mut p = Project::default();
        // OP1's L4 at full: a released note keeps sounding.
        p.set_param(fm_op_param(0, OpParam::L4), 1.0);
        let mut e = Engine::from_project(SR, &p);
        e.play();
        e.note_on(4, 60, 100);
        render(&mut e, 0.1);
        e.note_off(4, 60);
        e.stop();
        render(&mut e, 0.3);
        let (l, _) = render(&mut e, 0.05);
        assert!(
            peak(&l) > 0.01,
            "precondition: the drone rings after one stop"
        );
        e.stop();
        let (l, _) = render(&mut e, 0.05);
        assert_eq!(peak(&l), 0.0, "a second stop silences the FM machine");
    }

    #[test]
    fn restarting_play_releases_held_notes() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 64,
                pitch: 60,
                vel: 120,
            });
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.2);
        e.play();
        let mut empty = NotePattern::default();
        assert!(e.swap_note_pattern(4, 0, &mut empty));
        render(&mut e, 0.3);
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-4, "no voice hangs after a restart");
    }

    #[test]
    fn deleting_a_sounding_note_while_playing_releases_it() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0].length = 64;
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 8,
                pitch: 60,
                vel: 120,
            });
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.1);
        let mut empty = NotePattern {
            length: 64,
            ..NotePattern::default()
        };
        assert!(e.swap_note_pattern(4, 0, &mut empty));
        render(&mut e, 0.5);
        let (l, _) = render(&mut e, 0.05);
        assert!(
            peak(&l) < 1e-4,
            "the deleted note stops well before the loop end"
        );
    }

    #[test]
    fn fm_volume_knob_reaches_the_voice() {
        use crate::params::FmParam;
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 32,
                pitch: 60,
                vel: 120,
            });
        p.set_param(fm_param(FmParam::Volume), 0.0);
        let mut e = Engine::from_project(SR, &p);
        e.play();
        let (l, _) = render(&mut e, 0.3);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn stopped_engine_is_silent() {
        let mut e = demo_engine();
        let (l, _) = render(&mut e, 0.5);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn playing_demo_makes_bounded_sound() {
        let mut e = demo_engine();
        e.play();
        let (l, r) = render(&mut e, 4.0);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(rms(&l) > 0.02, "rms = {}", rms(&l));
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn odd_buffer_sizes_render_identically() {
        let mut a = demo_engine();
        let mut b = demo_engine();
        a.play();
        b.play();
        let (la, _) = render(&mut a, 1.0);
        let mut lb = Vec::new();
        let mut remaining = la.len();
        while remaining > 0 {
            let n = remaining.min(37);
            let mut l = vec![0.0; n];
            let mut r = vec![0.0; n];
            b.process(&mut l, &mut r);
            lb.extend(l);
            remaining -= n;
        }
        for (x, y) in la.iter().zip(&lb) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn master_level_zero_is_silent() {
        let mut e = demo_engine();
        e.set_param(ParamId::Master(MasterParam::Level), 0.0);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn stop_closes_synth_gates_and_tails_die() {
        let mut e = demo_engine();
        for c in 0..4 {
            e.set_param(channel_param(c, ChannelParam::DelaySend), 0.0);
            e.set_param(channel_param(c, ChannelParam::ReverbSend), 0.0);
        }
        e.play();
        render(&mut e, 1.0);
        e.stop();
        render(&mut e, 3.0);
        let (l, _) = render(&mut e, 0.2);
        assert!(peak(&l) < 1e-3, "peak = {}", peak(&l));
    }

    #[test]
    fn pattern_replacement_returns_old_pattern() {
        let mut e = demo_engine();
        let old = e.set_synth_pattern(0, 0, SynthPattern::default());
        assert!(
            old.steps
                .iter()
                .any(|s| s.gate != crate::project::Gate::Off)
        );
        let old_drums = e.set_drum_pattern(0, 0, DrumPattern::default());
        assert!(
            old_drums
                .steps
                .iter()
                .flatten()
                .any(|h| *h != crate::project::DrumHit::Off)
        );
    }

    #[test]
    fn nan_tempo_cannot_hang_or_poison() {
        let mut e = demo_engine();
        e.set_bpm(f32::NAN);
        e.set_swing(f32::NAN);
        e.play();
        let (l, r) = render(&mut e, 0.5);
        assert!(all_finite(&l) && all_finite(&r));
    }

    #[test]
    fn broken_clock_cannot_hang_process() {
        let mut e = demo_engine();
        e.play();
        e.seq.poison_clock();
        let (l, r) = render(&mut e, 0.2);
        assert_eq!(l.len(), r.len());
        assert!(all_finite(&l) && all_finite(&r));
    }

    #[test]
    fn out_of_range_indices_are_ignored() {
        let mut e = demo_engine();
        e.play();
        e.select_pattern(0, 200);
        e.select_pattern(9, 0);
        e.set_param(drum_param(0, 9, DrumParam::Tune), 0.5);
        e.set_param(synth_param(7, SynthParam::Cutoff), 0.5);
        e.set_param(channel_param(6, ChannelParam::Level), 0.5);
        e.set_param(kit_param(2, KitParam::Volume), 0.5);
        let (l, _) = render(&mut e, 0.5);
        assert!(all_finite(&l));
        assert_eq!(e.current_patterns(), [0; MACHINES]);
    }

    #[test]
    fn malformed_or_misplaced_patterns_are_rejected() {
        let mut e = demo_engine();
        e.play();
        let mut short = SynthPattern::default();
        short.steps.truncate(3);
        let back = e.set_synth_pattern(0, 0, short.clone());
        assert_eq!(back, short);
        let mut zero = DrumPattern::default();
        zero.length = 0;
        assert_eq!(e.set_drum_pattern(0, 0, zero.clone()), zero);
        let mut lane = SynthPattern::default();
        lane.param_lane = Some(crate::project::ParamLane {
            param: ParamId::Fx(crate::params::FxParam::DelayMs),
            values: vec![None; 2],
        });
        assert_eq!(e.set_synth_pattern(0, 0, lane.clone()), lane);
        assert_eq!(
            e.set_synth_pattern(2, 0, SynthPattern::default()),
            SynthPattern::default()
        );
        assert_eq!(
            e.set_drum_pattern(0, 200, DrumPattern::default()),
            DrumPattern::default()
        );
        let (l, _) = render(&mut e, 1.0);
        assert!(all_finite(&l) && rms(&l) > 0.02, "still playing the demo");
    }

    #[test]
    fn non_finite_param_is_dropped_and_values_clamped() {
        let mut e = demo_engine();
        e.set_param(synth_param(0, SynthParam::Cutoff), f32::NAN);
        e.set_param(channel_param(0, ChannelParam::Level), 50.0);
        e.play();
        let (l, _) = render(&mut e, 0.1);
        assert!(all_finite(&l));
        e.set_param(synth_param(0, SynthParam::Cutoff), 0.5);
        let (l, _) = render(&mut e, 3.0);
        assert!(all_finite(&l) && peak(&l) > 0.01);
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn playhead_advances() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.3);
        assert!(e.playhead()[2] >= 2);
        assert_eq!(e.current_patterns(), [0; MACHINES]);
    }

    #[test]
    fn lane_value_overrides_only_its_step() {
        use crate::project::ParamLane;
        let mut e = demo_engine();
        let mut p = crate::demo::demo_project()
            .synth(MachineId::Synth1.uid())
            .unwrap()
            .patterns[0]
            .clone();
        let mut values = vec![None; 64];
        values[0] = Some(0.0);
        p.param_lane = Some(ParamLane {
            param: ParamId::Master(MasterParam::Level),
            values,
        });
        e.set_synth_pattern(0, 0, p);
        e.play();
        let (l, _) = render(&mut e, 0.5);
        // Step 0 (about 5500 samples at the demo's 128 BPM + swing) is silent...
        assert_eq!(peak(&l[64..5000]), 0.0);
        // ...then the knob value comes back.
        assert!(peak(&l[6000..20_000]) > 0.01);
    }

    #[test]
    fn from_project_matches_load_project() {
        let p = crate::demo::demo_project();
        let mut a = Engine::from_project(SR, &p);
        let mut b = Engine::new(SR);
        b.load_project(&p);
        a.play();
        b.play();
        let (la, _) = render(&mut a, 0.5);
        let (lb, _) = render(&mut b, 0.5);
        assert_eq!(la, lb);
        assert_eq!(a.sample_rate(), SR);
    }

    #[test]
    fn swap_exchanges_patterns_and_rejects_bad_ones() {
        let mut e = demo_engine();
        let mut p = SynthPattern::default();
        assert!(e.swap_synth_pattern(0, 0, &mut p));
        assert!(
            p.steps.iter().any(|s| s.gate != crate::project::Gate::Off),
            "got the old pattern back"
        );
        let mut bad = SynthPattern::default();
        bad.steps.pop();
        assert!(!e.swap_synth_pattern(0, 1, &mut bad));
        assert_eq!(bad.steps.len(), 63, "rejected pattern is untouched");
        let mut d = DrumPattern::default();
        assert!(e.swap_drum_pattern(1, 0, &mut d));
        assert!(!e.swap_drum_pattern(2, 0, &mut d));
    }

    #[test]
    fn meters_report_and_reset() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.5);
        let m = e.take_meters();
        assert!(
            m[..4].iter().all(|v| *v > 0.01),
            "both synths and kits: {m:?}"
        );
        assert!(m[MAX_MACHINES] > 0.01, "{m:?}");
        assert!(m[MAX_MACHINES] <= 1.0 + 1e-6, "master is post-clip");
        e.stop();
        render(&mut e, 5.0);
        e.take_meters();
        render(&mut e, 0.05);
        assert!(e.take_meters().iter().all(|v| *v < 1e-3));
        assert!(!e.take_guard_fired());
    }

    #[test]
    fn guard_fires_and_recovers() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.1);
        assert!(!e.take_guard_fired());
        match &mut e.slots[0].dsp {
            MachineDsp::Synth(s) => s.poison(),
            _ => unreachable!(),
        }
        let (l, r) = render(&mut e, 0.05);
        assert!(
            all_finite(&l) && all_finite(&r),
            "the guard mutes the block"
        );
        assert!(e.take_guard_fired(), "the guard fired");
        assert!(!e.take_guard_fired(), "reported once");
        let (l, r) = render(&mut e, 0.1);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(peak(&l) > 0.01, "sound comes back after the reset");
        assert!(!e.take_guard_fired(), "the reset synth renders cleanly");
    }

    #[test]
    fn queued_patterns_are_reported() {
        let mut e = demo_engine();
        e.play();
        e.select_pattern(3, 5);
        assert_eq!(e.queued_patterns(), [None, None, None, Some(5), None, None]);
    }

    #[test]
    fn stop_restores_knobs_overridden_by_a_lane() {
        use crate::project::ParamLane;
        let mut e = demo_engine();
        let mut p = crate::demo::demo_project()
            .synth(MachineId::Synth1.uid())
            .unwrap()
            .patterns[0]
            .clone();
        p.param_lane = Some(ParamLane {
            param: ParamId::Master(MasterParam::Level),
            values: vec![Some(0.0); 64],
        });
        e.set_synth_pattern(0, 0, p);
        e.play();
        render(&mut e, 0.3);
        e.stop();
        e.set_synth_pattern(
            0,
            0,
            crate::demo::demo_project()
                .synth(MachineId::Synth1.uid())
                .unwrap()
                .patterns[0]
                .clone(),
        );
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01, "master level was restored by stop()");
    }

    fn two_bar_demo() -> Project {
        let mut p = crate::demo::demo_project();
        let bar = crate::project::Bar {
            cells: crate::machine::MachineId::ALL
                .iter()
                .map(|m| {
                    let pattern = Some(crate::project::PatternRef(0));
                    (
                        m.uid(),
                        crate::project::Cell {
                            pattern,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        };
        p.song.bars = vec![bar.clone(), bar];
        p
    }

    #[test]
    fn a_song_plays_and_stops_by_itself() {
        let mut e = Engine::from_project(SR, &two_bar_demo());
        e.set_mode(crate::sequencer::Mode::Song);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01);
        render(&mut e, 4.0); // two bars at 128 BPM ≈ 3.75 s
        assert!(!e.is_playing(), "song ended");
    }

    #[test]
    fn automation_plays_in_song_mode_and_stop_restores_the_knob() {
        let mut p = two_bar_demo();
        crate::song::write_automation(&mut p.song, ParamId::Master(MasterParam::Level), 0, 0, 0.0);
        let mut e = Engine::from_project(SR, &p);
        e.set_mode(crate::sequencer::Mode::Song);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert_eq!(peak(&l[64..]), 0.0, "automation holds master at 0");
        e.stop();
        e.set_mode(crate::sequencer::Mode::Pattern);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01, "knob value restored by stop");
    }

    #[test]
    fn a_touched_param_ignores_automation() {
        let mut p = two_bar_demo();
        crate::song::write_automation(
            &mut p.song,
            ParamId::Master(MasterParam::Level),
            0,
            200,
            0.0,
        );
        let mut e = Engine::from_project(SR, &p);
        e.set_mode(crate::sequencer::Mode::Song);
        e.touch(ParamId::Master(MasterParam::Level), true);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01);
    }

    #[test]
    fn live_values_do_not_move_the_knob() {
        let mut e = demo_engine();
        e.play();
        e.set_param_live(ParamId::Master(MasterParam::Level), 0.0);
        let (l, _) = render(&mut e, 0.5);
        assert_eq!(peak(&l[64..]), 0.0);
        e.stop();
        e.play();
        let (l, _) = render(&mut e, 0.5);
        assert!(peak(&l) > 0.01, "stop restored the saved knob");
    }

    #[test]
    fn swap_song_validates() {
        let mut e = demo_engine();
        let mut good = two_bar_demo().song;
        assert!(e.swap_song(&mut good));
        let mut bad = crate::project::Song {
            bars: vec![Default::default(); crate::song::MAX_BARS + 1],
            ..Default::default()
        };
        assert!(!e.swap_song(&mut bad));
        assert_eq!(
            bad.bars.len(),
            crate::song::MAX_BARS + 1,
            "rejected song is untouched"
        );
    }

    #[test]
    fn looping_back_before_the_first_point_restores_the_knob() {
        let mut p = two_bar_demo();
        p.song.loop_range = Some((0, 1));
        crate::song::write_automation(
            &mut p.song,
            ParamId::Master(MasterParam::Level),
            20,
            20,
            0.0,
        );
        let mut e = Engine::from_project(SR, &p);
        e.set_mode(crate::sequencer::Mode::Song);
        e.play();
        let (l, _) = render(&mut e, 3.5); // two bars at 128 BPM ≈ 3.75 s
        assert!(peak(&l[..40_000]) > 0.01, "bar 0 plays before the point");
        assert!(
            peak(&l[110_000..]) < 1e-3,
            "master held at 0 from step 20 on"
        );
        render(&mut e, 0.5);
        assert!(e.is_playing() && e.song_bar() == 0, "looped to bar 0");
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01, "second pass through bar 0 is audible");
    }

    #[test]
    fn swapping_the_song_mid_bar_resyncs_the_position() {
        let mut e = Engine::from_project(SR, &two_bar_demo());
        let mut long = crate::demo::demo_project()
            .synth(MachineId::Synth1.uid())
            .unwrap()
            .patterns[0]
            .clone();
        long.length = 32;
        e.set_synth_pattern(0, 7, long);
        e.set_mode(crate::sequencer::Mode::Song);
        e.play();
        let mut l = [0.0; 64];
        let mut r = [0.0; 64];
        for _ in 0..10_000 {
            if e.song_step() == 19 {
                break;
            }
            e.process(&mut l, &mut r);
        }
        assert_eq!((e.song_bar(), e.song_step()), (1, 19));
        let mut song = two_bar_demo().song;
        song.bars[0].cell_mut(MachineId::Synth1.uid()).pattern =
            Some(crate::project::PatternRef(7));
        assert!(e.swap_song(&mut song));
        for _ in 0..10_000 {
            if e.song_step() > 20 {
                break;
            }
            e.process(&mut l, &mut r);
        }
        assert_eq!(e.song_step(), 32 + 3, "bar 0 is now 32 steps long");
        assert_eq!(e.song_bar(), 1);
    }

    #[test]
    fn song_start_and_position() {
        let mut e = Engine::from_project(SR, &two_bar_demo());
        e.set_mode(crate::sequencer::Mode::Song);
        e.set_song_start(1);
        e.play();
        render(&mut e, 0.01);
        assert_eq!(e.song_bar(), 1);
        assert_eq!(e.song_step(), 16);
        assert_eq!(e.mode(), crate::sequencer::Mode::Song);
    }

    #[test]
    fn note_patterns_swap_and_report() {
        use crate::project::{Note, NotePattern};
        let mut e = demo_engine();
        let mut p = NotePattern::default();
        p.notes.push(Note {
            start: 0,
            len: 4,
            pitch: 60,
            vel: 100,
        });
        assert!(e.swap_note_pattern(4, 2, &mut p));
        assert!(p.notes.is_empty(), "got the old, empty pattern back");
        assert!(!e.swap_note_pattern(0, 2, &mut p), "not a note machine");
        let mut bad = NotePattern {
            length: 0,
            ..NotePattern::default()
        };
        assert!(!e.swap_note_pattern(5, 0, &mut bad));
        e.play();
        e.select_pattern(5, 9);
        assert_eq!(e.queued_patterns()[5], Some(9));
        assert_eq!(e.current_patterns().len(), crate::machine::MACHINES);
    }

    #[test]
    fn audition_survives_a_pattern_release_of_the_same_pitch() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 4,
                pitch: 60,
                vel: 100,
            });
        let mut held = Engine::from_project(SR, &p);
        held.play();
        held.note_on(4, 60, 100);
        let (l, _) = render(&mut held, 0.5);
        assert!(
            peak(&l[l.len() - 2000..]) > 0.01,
            "audition note still sounds"
        );
        let mut pattern_only = Engine::from_project(SR, &p);
        pattern_only.play();
        let (l, _) = render(&mut pattern_only, 0.5);
        assert!(peak(&l[l.len() - 2000..]) < 1e-3, "pattern note released");
    }

    #[test]
    fn fm_operator_params_reach_the_synth() {
        use crate::params::OpParam;
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 32,
                pitch: 60,
                vel: 120,
            });
        p.set_param(fm_op_param(0, OpParam::Level), 0.0);
        let mut e = Engine::from_project(SR, &p);
        e.play();
        let (l, _) = render(&mut e, 0.3);
        assert!(
            peak(&l) < 1e-6,
            "OP1 (the only carrier heard) is at level 0"
        );
    }

    #[test]
    fn fm_algorithm_changes_the_sound() {
        use crate::params::{FmParam, OpParam};
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 32,
                pitch: 57,
                vel: 120,
            });
        p.set_param(fm_op_param(1, OpParam::Level), 0.9);
        let mut a = Engine::from_project(SR, &p);
        p.set_param(fm_param(FmParam::Algorithm), 1.0);
        let mut b = Engine::from_project(SR, &p);
        a.play();
        b.play();
        let (la, _) = render(&mut a, 0.3);
        let (lb, _) = render(&mut b, 0.3);
        assert_ne!(la, lb);
    }

    /// The tail of a render: the last ~45 ms.
    fn tail(l: &[f32]) -> f32 {
        peak(&l[l.len() - 2000..])
    }

    #[test]
    fn play_and_stop_keep_auditions_sounding() {
        for machine in [4, 5] {
            let mut e = Engine::from_project(SR, &tone_slot_project());
            e.note_on(machine, 60, 100);
            render(&mut e, 0.05);
            e.play();
            let (l, _) = render(&mut e, 0.3);
            assert!(tail(&l) > 0.01, "machine {machine}: audition survives play");
            e.stop();
            let (l, _) = render(&mut e, 0.3);
            assert!(tail(&l) > 0.01, "machine {machine}: audition survives stop");
            e.note_off(machine, 60);
            render(&mut e, 0.5);
            let (l, _) = render(&mut e, 0.05);
            assert!(
                peak(&l) < 1e-4,
                "machine {machine}: its own release ends it"
            );
        }
    }

    #[test]
    fn play_and_stop_release_pattern_notes_under_an_audition() {
        let mut e = Engine::from_project(SR, &tone_slot_project());
        for machine in [4, 5] {
            e.note_on(machine, 72, 100);
            e.poly_on(machine, 60, 100, false);
        }
        e.play();
        assert_eq!(
            e.fm().sounding(),
            vec![72],
            "play released the pattern note"
        );
        assert_eq!(e.sampler().held_keys(), vec![72], "on the sampler too");
        e.poly_on(4, 60, 100, false);
        e.poly_on(5, 60, 100, false);
        e.stop();
        assert_eq!(
            e.fm().sounding(),
            vec![72],
            "stop released the pattern note"
        );
        assert_eq!(e.sampler().held_keys(), vec![72]);
    }

    #[test]
    fn fm_mono_play_keeps_the_audition_and_forgets_pattern_keys() {
        let mut p = tone_slot_project();
        p.set_param(fm_param(crate::params::FmParam::Mono), 1.0);
        let mut e = Engine::from_project(SR, &p);
        e.poly_on(4, 48, 100, false);
        e.note_on(4, 72, 100);
        e.play();
        assert_eq!(e.fm().sounding(), vec![72], "the audition keeps sounding");
        e.note_off(4, 72);
        assert!(
            e.fm().sounding().is_empty(),
            "no glide back to a pattern key the sequencer forgot"
        );
    }

    /// A one-bar song: an FM drone (OP1 L4 at full, so released notes
    /// keep sounding) and nothing else.
    fn fm_drone_song() -> Project {
        use crate::params::OpParam;
        use crate::project::Note;
        let mut p = Project::default();
        p.set_param(fm_op_param(0, OpParam::L4), 1.0);
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: 0,
                len: 8,
                pitch: 60,
                vel: 120,
            });
        let mut bar = crate::project::Bar::default();
        bar.cell_mut(MachineId::Fm.uid()).pattern = Some(crate::project::PatternRef(0));
        p.song.bars = vec![bar];
        p
    }

    #[test]
    fn the_first_stop_after_a_song_ends_only_releases() {
        let mut e = Engine::from_project(SR, &fm_drone_song());
        e.set_mode(Mode::Song);
        e.play();
        render(&mut e, 3.0);
        assert!(!e.is_playing(), "precondition: the song ended by itself");
        e.stop();
        let (l, _) = render(&mut e, 0.2);
        assert!(tail(&l) > 0.01, "the first stop lets the tail ring");
        e.stop();
        let (l, _) = render(&mut e, 0.05);
        assert_eq!(peak(&l), 0.0, "a second stop cuts it");
    }

    #[test]
    fn a_stop_after_replaying_a_finished_song_is_a_plain_stop() {
        let mut e = Engine::from_project(SR, &fm_drone_song());
        e.set_mode(Mode::Song);
        e.play();
        render(&mut e, 3.0);
        e.play();
        render(&mut e, 0.2);
        e.stop();
        e.stop();
        let (l, _) = render(&mut e, 0.05);
        assert_eq!(peak(&l), 0.0, "stop-twice still cuts after a replay");
    }

    #[test]
    fn audition_release_leaves_the_pattern_note_of_that_pitch() {
        let mut e = Engine::from_project(SR, &tone_slot_project());
        for machine in [4, 5] {
            e.poly_on(machine, 60, 100, false);
            e.note_on(machine, 60, 100);
            e.note_off(machine, 60);
        }
        assert_eq!(e.fm().sounding(), vec![60], "the pattern voice is held");
        assert_eq!(e.fm().active_voices(), 2, "the audition voice is releasing");
        assert_eq!(e.sampler().held_keys(), vec![60]);
        let (l, _) = render(&mut e, 0.5);
        assert!(tail(&l) > 0.01, "the pattern notes still sound");
    }

    #[test]
    fn double_note_on_of_a_pitch_releases_both_on_one_off() {
        let mut e = Engine::from_project(SR, &tone_slot_project());
        for machine in [4, 5] {
            e.poly_on(machine, 60, 100, false);
            e.poly_on(machine, 60, 100, false);
        }
        assert_eq!(e.fm().sounding(), vec![60, 60]);
        assert_eq!(e.sampler().held_keys(), vec![60, 60]);
        for machine in [4, 5] {
            e.poly_off(machine, 60, false);
        }
        assert!(e.fm().sounding().is_empty(), "one PolyOff releases both");
        assert!(
            e.sampler().held_keys().is_empty(),
            "one PolyOff releases both"
        );
        render(&mut e, 1.0);
        assert_eq!(e.fm().active_voices() + e.sampler().active_voices(), 0);
    }
}
