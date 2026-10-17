//! The whole groovebox: a rack of slots (each a
//! machine's DSP, knobs, patterns and channel strip), the shared send
//! effects and master bus, and the sequencer, rendered sample-accurately.
//! process() is real-time safe: no allocation, locking or I/O.

use crate::host::{PatternData, PositionStatus};
use crate::machine::{MachineType, MachineUid, rack_capacity};
use crate::mixer::Mixer;
use crate::params::{DRUM_INSTRUMENTS, GlobalParam, MachineParam, ParamId};
use crate::project::{MAX_NOTES, PATTERNS, Project};
use crate::rack::{CompiledSong, MachineDsp, RackSwap, Slot, SlotPatterns};
use crate::sampler::Sampler;
use crate::sampler::sample::SampleData;
use crate::sampler::slot::SlotPlay;
use crate::sequencer::{Mode, PENDING_PER_TRACK, SeqBuffers, SeqEvent, Sequencer};
use crate::song::{Banks, MAX_SONG_STEPS};
use std::sync::Arc;
use std::time::Instant;

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

/// Most sequencer events one slot can add to a tick: a 303's note (or
/// gate off), lane restore and lane value; a kit's every instrument; a
/// note machine's 128 releases plus `MAX_NOTES` starts.
const SLOT_EVENTS: usize = {
    let synth = 3;
    let kit = DRUM_INSTRUMENTS;
    let notes = 128 + MAX_NOTES;
    let m = if synth > kit { synth } else { kit };
    if m > notes { m } else { notes }
};

/// Room in the sequencer event buffer for the worst tick of any rack of
/// up to `capacity` machines: every automation lane (a song holds at most
/// one lane per param, so at most `max_param_count(capacity)`) plus every
/// slot at its worst. Reserved up front so the buffer never grows on the
/// audio thread; a rack past `capacity` brings a bigger one (`Growth`).
pub(crate) fn event_reserve(capacity: usize) -> usize {
    max_param_count(capacity) + capacity * SLOT_EVENTS
}

// Held-back events come out of the same buffer, a batch at a time.
const _: () = assert!(PENDING_PER_TRACK <= SLOT_EVENTS);

/// The most params a rack of `capacity` machines can have: that many of
/// the type with the most knobs, plus the globals. The event buffer is
/// reserved for it so a rack edit within the capacity never needs a
/// bigger one. Allocates (construction only).
fn max_param_count(capacity: usize) -> usize {
    let per_slot = MachineType::ALL
        .iter()
        .map(|&t| MachineParam::all_for(t).len())
        .max()
        .unwrap_or(0);
    GLOBALS + capacity * per_slot
}

/// Whether `song` can play on a rack of `capacity` positions with
/// `param_count` params, where `fits` tells which params the rack has: a
/// lane per position at most, each sorted by start with starts and the
/// end within `MAX_SONG_STEPS`, pattern refs within the bank, and at most
/// one automation lane per param, each on a param the rack has.
fn song_fits(
    song: &CompiledSong,
    capacity: usize,
    param_count: usize,
    fits: impl Fn(ParamId) -> bool,
) -> bool {
    let max = MAX_SONG_STEPS as u32;
    let blocks_ok = song.lanes.len() <= capacity
        && song.lanes.iter().all(|lane| {
            lane.windows(2).all(|w| w[0].start < w[1].start)
                && lane
                    .iter()
                    .all(|b| b.start < max && (b.pattern.0 as usize) < PATTERNS)
        });
    let end_ok = song.end.is_none_or(|e| e <= max);
    let lanes_ok =
        song.automation.len() <= param_count && song.automation.iter().all(|l| fits(l.param));
    blocks_ok && end_ok && lanes_ok
}

pub struct Engine {
    sample_rate: f32,
    /// The rack, in order; room for the rack capacity so it never grows.
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
    guard_fired: bool,
    /// The song, resolved to this rack's positions.
    song: CompiledSong,
    /// The song ended by itself and no transport command came since: the
    /// next `stop` counts as the first one (it only releases).
    ended_by_song: bool,
    /// Nanoseconds of the rest of `process` (sequencing, sends, master)
    /// since the last `take_shared_ns`.
    shared_ns: u64,
}

impl Engine {
    pub const MAX_BLOCK: usize = 512;

    pub fn new(sample_rate: f32) -> Self {
        Self::from_project(sample_rate, &Project::default())
    }

    /// Builds an engine with a project loaded. Allocates; call off the
    /// audio thread.
    pub fn from_project(sample_rate: f32, p: &Project) -> Engine {
        let capacity = rack_capacity(p.machines.len());
        let global_ids: [GlobalParam; GLOBALS] = std::array::from_fn(|i| GlobalParam::all()[i]);
        let defaults = global_ids.map(|g| g.info().default);
        let mut e = Self {
            sample_rate,
            slots: Vec::with_capacity(capacity),
            mixer: Mixer::new(sample_rate, Self::MAX_BLOCK),
            seq: Sequencer::new(sample_rate, capacity),
            events: Vec::new(),
            global_ids,
            global_knobs: defaults,
            global_base: defaults,
            global_touched: [false; GLOBALS],
            param_count: GLOBALS,
            guard_fired: false,
            song: CompiledSong::default(),
            ended_by_song: false,
            shared_ns: 0,
        };
        e.load_project(p);
        e
    }

    /// Rebuilds the rack from `p` (one slot per machine, in rack order),
    /// growing the rack capacity if `p` needs more, and loads its globals,
    /// song, tempo and pattern selection. Not real-time safe: call while
    /// stopped or from a non-audio thread before handing the engine to the
    /// audio thread.
    pub(crate) fn load_project(&mut self, p: &Project) {
        let capacity = rack_capacity(p.machines.len()).max(self.capacity());
        if capacity > self.capacity() {
            self.seq.grow(&mut SeqBuffers::new(capacity));
        }
        self.slots.clear();
        self.slots.reserve(capacity);
        self.take_shared_ns();
        self.slots.extend(
            p.machines
                .iter()
                .map(|m| Slot::from_entry(m, self.sample_rate)),
        );
        self.param_count = GLOBALS + self.slots.iter().map(|s| s.knobs.len()).sum::<usize>();
        self.events = Vec::with_capacity(event_reserve(capacity));
        for i in 0..GLOBALS {
            let id = ParamId::from(self.global_ids[i]);
            self.set_param(id, p.param(id));
        }
        self.song = CompiledSong::compile(&p.song, &self.slot_uids());
        self.set_bpm(p.bpm);
        self.set_swing(p.swing);
        self.set_seed(p.seed);
        // Every position takes its machine's selection; empty ones go back
        // to the first pattern.
        for pos in 0..capacity {
            let selected = p
                .machines
                .get(pos)
                .filter(|_| pos < self.slots.len())
                .map_or(0, |m| m.selected);
            self.seq.select(pos, selected);
        }
    }

    /// The rack's uids in order.
    pub fn slot_uids(&self) -> Vec<MachineUid> {
        self.slots.iter().map(|s| s.uid).collect()
    }

    /// Machines in the rack.
    pub fn rack_len(&self) -> usize {
        self.slots.len()
    }

    /// Rack positions the engine holds without a `Growth`.
    pub fn capacity(&self) -> usize {
        self.seq.capacity()
    }

    /// Where each new position's slot comes from, and the new rack's
    /// types, if `swap` is well formed: `order` within the capacity after
    /// the swap without repeats, `fresh` as long as `order` with each slot
    /// under its own uid, every other uid in the rack now, an empty `into`
    /// with room for the new rack and the capacity now, a growth (if any)
    /// sized for a bigger capacity than now, and a song that fits the new
    /// rack.
    fn check_rack(&self, swap: &mut RackSwap) -> bool {
        let n = swap.order.len();
        let capacity = swap.growth.as_ref().map_or(self.capacity(), |g| g.capacity);
        let events_room = swap
            .growth
            .as_ref()
            .map_or(self.events.capacity(), |g| g.events.capacity());
        // Its events buffer is checked against the new rack below
        // (`event_reserve` allocates, so it can't be asked here).
        let growth_ok = swap.growth.as_ref().is_none_or(|g| {
            g.capacity > self.capacity() && g.seq.fits(g.capacity) && g.events.is_empty()
        });
        if n > capacity
            || !growth_ok
            || swap.fresh.len() != n
            || swap.map.len() != n
            || swap.tys.len() != n
            || swap.selected.len() != n
            || !swap.into.is_empty()
            || swap.into.capacity() < n.max(self.capacity())
        {
            return false;
        }
        swap.map.fill(None);
        swap.tys.fill(None);
        let mut param_count = GLOBALS;
        for (i, &uid) in swap.order.iter().enumerate() {
            if swap.order[..i].contains(&uid) {
                return false;
            }
            let slot = match &swap.fresh[i] {
                Some(s) if s.uid == uid => s,
                Some(_) => return false,
                None => {
                    let Some(old) = self.position(uid) else {
                        return false;
                    };
                    swap.map[i] = Some(old);
                    &self.slots[old]
                }
            };
            swap.tys[i] = Some(slot.ty);
            param_count += slot.knobs.len();
        }
        let fits = |id: ParamId| match id {
            ParamId::Machine(uid, p) => swap
                .order
                .iter()
                .position(|&u| u == uid)
                .and_then(|i| swap.tys[i])
                .is_some_and(|ty| p.index_for(ty).is_some()),
            ParamId::Fx(_) | ParamId::Master(_) => id.global().is_some(),
        };
        // Every automation lane (at most one per param) plus every
        // position at its worst.
        let fits_events = param_count + capacity * SLOT_EVENTS <= events_room;
        fits_events && song_fits(&swap.song, capacity, param_count, fits)
    }

    /// Swaps in a new rack (real-time safe: nothing is built, dropped or
    /// allocated). Surviving machines move to their new positions with
    /// their voices, knobs, patterns and sequencer tracks; fresh slots
    /// come in on their selected pattern (on their song block while a song
    /// plays), in phase with the running tracks. Afterwards `swap.into` holds the
    /// old slot `Vec` with the removed machines, `swap.fresh` any unused
    /// slots and `swap.song` the old song, all for disposal off the audio
    /// thread. A malformed swap changes nothing and returns `false`;
    /// otherwise `swap.map` says where each new position came from.
    pub fn set_rack(&mut self, swap: &mut RackSwap) -> bool {
        if !self.check_rack(swap) {
            return false;
        }
        // Checked above: bigger buffers, empty, the old ones going back.
        if let Some(g) = swap.growth.as_mut() {
            self.seq.grow(&mut g.seq);
            self.events.clear();
            std::mem::swap(&mut self.events, &mut g.events);
        }
        let bpm = self.seq.bpm();
        for i in 0..swap.order.len() {
            let slot = match swap.fresh[i].take() {
                Some(mut s) => {
                    match &mut s.dsp {
                        MachineDsp::Sampler(x) => x.set_bpm(bpm),
                        MachineDsp::Mono(m) => m.set_bpm(bpm),
                        _ => {}
                    }
                    s
                }
                None => {
                    // Checked above: the uid is in the rack exactly once.
                    let Some(j) = self.position(swap.order[i]) else {
                        continue;
                    };
                    self.slots.swap_remove(j)
                }
            };
            // Capacity checked above: never grows.
            swap.into.push(slot);
        }
        std::mem::swap(&mut self.slots, &mut swap.into);
        self.param_count = GLOBALS + self.slots.iter().map(|s| s.knobs.len()).sum::<usize>();
        std::mem::swap(&mut self.song, &mut swap.song);
        self.seq.rearrange(
            &swap.map,
            Banks::of_slots(&self.slots),
            &self.song,
            &swap.selected,
        );
        self.seq.resync();
        true
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

    /// Rack position of machine `uid`.
    fn position(&self, uid: MachineUid) -> Option<usize> {
        self.slots.iter().position(|s| s.uid == uid)
    }

    /// The Sampler machine `uid`; `None` for an unknown uid or another type.
    fn sampler_with(&mut self, uid: MachineUid) -> Option<&mut Sampler> {
        let pos = self.position(uid)?;
        match &mut self.slots[pos].dsp {
            MachineDsp::Sampler(x) => Some(&mut **x),
            _ => None,
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.seq.set_bpm(bpm);
        let bpm = self.seq.bpm();
        self.mixer.set_bpm(bpm);
        for s in self.samplers() {
            s.set_bpm(bpm);
        }
        for s in &mut self.slots {
            if let MachineDsp::Mono(m) = &mut s.dsp {
                m.set_bpm(bpm);
            }
        }
    }

    /// Exchanges a slot's audio on Sampler machine `uid` (real-time safe;
    /// the old sample comes back for disposal off the audio thread). An
    /// unknown uid, another machine type or a bad slot leaves `sample`
    /// unchanged and returns false.
    pub fn swap_sampler_sample(
        &mut self,
        uid: MachineUid,
        slot: usize,
        sample: &mut Option<Arc<SampleData>>,
    ) -> bool {
        self.sampler_with(uid)
            .is_some_and(|s| s.swap_sample(slot, sample))
    }

    /// Like `swap_sampler_sample`, for a slot's play data.
    pub fn swap_sampler_play(&mut self, uid: MachineUid, slot: usize, play: &mut SlotPlay) -> bool {
        self.sampler_with(uid)
            .is_some_and(|s| s.swap_play(slot, play))
    }

    pub fn set_swing(&mut self, swing: f32) {
        self.seq.set_swing(swing);
    }

    /// Fill on or off: `Fill` notes play while on, `NotFill` ones while
    /// off. Real-time safe.
    pub fn set_fill(&mut self, fill: bool) {
        self.seq.set_fill(fill);
    }

    /// The seed chance rolls hash (`Project.seed`). Real-time safe.
    pub fn set_seed(&mut self, seed: u32) {
        self.seq.set_seed(seed);
    }

    pub fn fill(&self) -> bool {
        self.seq.fill()
    }

    pub fn seed(&self) -> u32 {
        self.seq.seed()
    }

    /// Releases the note machines' pattern notes (auditions keep sounding).
    fn note_machines_pattern_off(&mut self) {
        for s in &mut self.slots {
            match &mut s.dsp {
                MachineDsp::Fm(f) => f.pattern_off(),
                MachineDsp::Sampler(x) => x.pattern_off(),
                MachineDsp::Mono(m) => m.pattern_off(),
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

    /// Audition: starts a note on FM or Sampler machine `uid` right away,
    /// playing or not. Unknown uids, other machines, pitches above 127 and
    /// velocity 0 are ignored.
    pub fn note_on(&mut self, uid: MachineUid, pitch: u8, vel: u8) {
        if let Some(pos) = self.position(uid) {
            self.poly_on(pos, pitch, vel, true);
        }
    }

    /// Audition release: only releases auditioned notes.
    pub fn note_off(&mut self, uid: MachineUid, pitch: u8) {
        if let Some(pos) = self.position(uid) {
            self.poly_off(pos, pitch, true);
        }
    }

    fn poly_on(&mut self, machine: usize, pitch: u8, vel: u8, live: bool) {
        if pitch > 127 || vel == 0 {
            return;
        }
        match self.slots.get_mut(machine).map(|s| &mut s.dsp) {
            Some(MachineDsp::Fm(f)) => f.note_on(pitch, vel.min(127), live),
            Some(MachineDsp::Sampler(s)) => s.note_on(pitch, vel.min(127), live),
            Some(MachineDsp::Mono(m)) => m.note_on(pitch, vel.min(127), live),
            _ => {}
        }
    }

    fn poly_off(&mut self, machine: usize, pitch: u8, live: bool) {
        match self.slots.get_mut(machine).map(|s| &mut s.dsp) {
            Some(MachineDsp::Fm(f)) => f.note_off(pitch, live),
            Some(MachineDsp::Sampler(s)) => s.note_off(pitch, live),
            Some(MachineDsp::Mono(m)) => m.note_off(pitch, live),
            _ => {}
        }
    }

    /// Stops the transport and releases the pattern's notes (tails ring
    /// out; auditioned notes keep sounding). Stopping while already stopped
    /// also silences the FM, Mono and Sampler machines at once, auditions
    /// included, so drones and loops can be cut. The first stop after a
    /// song ends by itself still only releases: the song end is not a stop.
    pub fn stop(&mut self) {
        let cut = !self.seq.is_playing() && !self.ended_by_song;
        self.ended_by_song = false;
        if cut {
            for s in &mut self.slots {
                if matches!(
                    s.dsp,
                    MachineDsp::Fm(_) | MachineDsp::Sampler(_) | MachineDsp::Mono(_)
                ) {
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
        for s in &mut self.slots {
            if let MachineDsp::Mono(m) = &mut s.dsp {
                m.stop_transport();
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
    pub fn swap_song(&mut self, song: &mut CompiledSong) -> bool {
        let fits = |id| self.locate(id).is_some();
        if !song_fits(song, self.capacity(), self.param_count, fits) {
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

    /// The step Song-mode Play starts from.
    pub fn set_song_start(&mut self, step: u32) {
        self.seq.set_song_start(step);
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

    /// Selects pattern `slot` for machine `uid` (queued while playing).
    /// Ignores unknown uids and `slot >= 128`.
    pub fn select_pattern(&mut self, uid: MachineUid, slot: u8) {
        if let Some(pos) = self.position(uid)
            && (slot as usize) < PATTERNS
        {
            self.seq.select(pos, slot);
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Swaps `p` with pattern `index` of machine `uid`'s bank (real-time
    /// safe). An unknown uid, a pattern of another type than the machine
    /// plays, `index >= PATTERNS` or a malformed pattern leaves both
    /// untouched and returns false.
    pub fn swap_pattern(&mut self, uid: MachineUid, index: u8, p: &mut PatternData) -> bool {
        let i = index as usize;
        let Some(pos) = self.position(uid).filter(|_| i < PATTERNS) else {
            return false;
        };
        // The track's per-note state follows the notes to their new
        // indices (`p` holds the old pattern after the swap).
        let seq = &mut self.seq;
        let swapped = match (&mut self.slots[pos].patterns, p) {
            (SlotPatterns::Synth(bank), PatternData::Synth(p)) if p.is_well_formed() => {
                bank.get_mut(i).map(|stored| {
                    std::mem::swap(stored, p);
                    seq.follow_notes(pos, index, &p.roll.notes, &stored.roll.notes);
                })
            }
            (SlotPatterns::Drum(bank), PatternData::Drum(p)) if p.is_well_formed() => {
                bank.get_mut(i).map(|stored| {
                    std::mem::swap(stored, p);
                    seq.follow_notes(pos, index, &p.roll.notes, &stored.roll.notes);
                })
            }
            (SlotPatterns::Notes(bank), PatternData::Notes(p)) if p.is_well_formed() => {
                bank.get_mut(i).map(|stored| {
                    std::mem::swap(stored, p);
                    seq.follow_notes(pos, index, &p.notes, &stored.notes);
                })
            }
            _ => None,
        };
        swapped.is_some()
    }

    /// The `length` of pattern `index` of machine `uid`.
    pub fn pattern_len(&self, uid: MachineUid, index: u8) -> Option<u8> {
        let i = index as usize;
        match &self.slots[self.position(uid)?].patterns {
            SlotPatterns::Synth(bank) => bank.get(i).map(|p| p.roll.length),
            SlotPatterns::Drum(bank) => bank.get(i).map(|p| p.roll.length),
            SlotPatterns::Notes(bank) => bank.get(i).map(|p| p.length),
        }
    }

    /// See `Mixer::take_master_meter`.
    pub fn take_master_meter(&mut self) -> f32 {
        self.mixer.take_master_meter()
    }

    /// Nanoseconds of shared work (sequencing, sends, master) since the
    /// last call; resets it.
    pub fn take_shared_ns(&mut self) -> u64 {
        std::mem::take(&mut self.shared_ns)
    }

    /// Calls `f(pos, meter, cpu_ns)` for each slot in rack order and resets
    /// both: its post-fader peak and its render and strip time.
    pub fn take_slot_stats(&mut self, mut f: impl FnMut(usize, f32, u64)) {
        for (pos, s) in self.slots.iter_mut().enumerate() {
            f(
                pos,
                std::mem::take(&mut s.meter),
                std::mem::take(&mut s.cpu_ns),
            );
        }
    }

    /// True once if the mixer's NaN guard fired since the last call.
    pub fn take_guard_fired(&mut self) -> bool {
        std::mem::take(&mut self.guard_fired)
    }

    /// Appends one entry per rack position (within capacity: never grows);
    /// takes the slots' meters and CPU time. `share` turns ns into a share.
    pub fn fill_positions(&mut self, out: &mut PositionStatus, share: impl Fn(u64) -> f32) {
        debug_assert!(out.fits(self.slots.len()));
        for pos in 0..self.slots.len().min(out.playhead.capacity()) {
            out.playhead.push(self.seq.playhead(pos));
            out.patterns.push(self.seq.current_pattern(pos));
            out.queued.push(self.seq.queued_pattern(pos));
        }
        self.take_slot_stats(|pos, m, ns| {
            if pos < out.meters.capacity() {
                out.meters.push(m);
                out.slot_cpu.push(share(ns));
            }
        });
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

    /// Applies one sequencer event; `slot`s are rack positions.
    fn handle_event(&mut self, ev: SeqEvent) {
        match ev {
            SeqEvent::Note {
                slot,
                midi,
                accent,
                legato,
            } => {
                if let Some(MachineDsp::Synth(s)) = self.slots.get_mut(slot).map(|s| &mut s.dsp) {
                    s.note_on(midi, accent, legato);
                }
            }
            SeqEvent::NoteOff { slot } => {
                if let Some(MachineDsp::Synth(s)) = self.slots.get_mut(slot).map(|s| &mut s.dsp) {
                    s.note_off();
                }
            }
            SeqEvent::Drum { slot, inst, accent } => {
                if let Some(MachineDsp::Kit(k)) = self.slots.get_mut(slot).map(|s| &mut s.dsp) {
                    k.trigger(inst, accent);
                }
            }
            SeqEvent::Param { id, value } => self.apply_param(id, value),
            SeqEvent::ParamRestore { id } => self.restore_param(id),
            SeqEvent::Automation { id, value } => self.apply_automation(id, value),
            SeqEvent::AutomationReset { id } => self.reset_automation(id),
            SeqEvent::PolyOn { slot, pitch, vel } => self.poly_on(slot, pitch, vel, false),
            SeqEvent::PolyOff { slot, pitch } => self.poly_off(slot, pitch, false),
        }
    }

    fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let block_start = Instant::now();
        // Slot time in this block, to leave the shared remainder.
        let mut slot_spent = 0u64;
        let n = out_l.len();
        for s in &mut self.slots {
            s.buf.l[..n].fill(0.0);
            s.buf.r[..n].fill(0.0);
        }
        let mut pos = 0;
        let mut ticks_here = 0;
        while pos < n {
            // Hits a tick held back (ratchets between ticks) go at their
            // own sample, before a tick on the same one.
            let due = self.seq.samples_to_due();
            if due == 0 {
                self.events.clear();
                self.seq.take_due(&mut self.events);
                for i in 0..self.events.len() {
                    let ev = self.events[i];
                    self.handle_event(ev);
                }
                continue;
            }
            let until = self.seq.samples_to_next_tick();
            if until == 0 && ticks_here < MAX_TICKS_PER_POSITION {
                ticks_here += 1;
                self.events.clear();
                self.seq
                    .tick(Banks::of_slots(&self.slots), &self.song, &mut self.events);
                for i in 0..self.events.len() {
                    let ev = self.events[i];
                    self.handle_event(ev);
                }
                // After this tick's notes, so a note on a step boundary
                // sounds on it. Each Mono clocks from its own block.
                let clocks = self.seq.tick_clocks();
                for (s, &(tick, tick_len)) in self.slots.iter_mut().zip(clocks) {
                    if let MachineDsp::Mono(m) = &mut s.dsp {
                        m.clock_tick(tick, tick_len);
                    }
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
            }
            .min(due);
            for s in self.slots.iter_mut() {
                let start = Instant::now();
                s.render(pos..pos + run);
                let spent = start.elapsed().as_nanos() as u64;
                s.cpu_ns = s.cpu_ns.saturating_add(spent);
                slot_spent = slot_spent.saturating_add(spent);
            }
            self.seq.advance(run);
            pos += run;
        }
        let before: u64 = self.slots.iter().map(|s| s.cpu_ns).sum();
        let fired = self.mixer.process(&mut self.slots, n, out_l, out_r);
        let after: u64 = self.slots.iter().map(|s| s.cpu_ns).sum();
        slot_spent = slot_spent.saturating_add(after.saturating_sub(before));
        if fired {
            self.guard_fired = true;
            for s in &mut self.slots {
                s.reset();
            }
        }
        let block = block_start.elapsed().as_nanos() as u64;
        self.shared_ns = self
            .shared_ns
            .saturating_add(block.saturating_sub(slot_spent));
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one field of a default pattern
mod tests {
    use super::*;
    use crate::fm::FmSynth;
    use crate::machine::{MachineType, MachineUid, RACK_CHUNK};
    use crate::params::{
        ChannelParam, DrumParam, FmParam, KitParam, MasterParam, OpParam, SynthParam,
    };
    use crate::project::{DrumPattern, NotePattern, SynthPattern};
    use crate::project::{MachineBody, MachineEntry};
    use crate::rack::{Growth, RackSwap};
    use crate::testutil::{all_finite, peak, rms};

    #[test]
    fn a_rack_edit_past_the_capacity_grows_it_and_keeps_playing_state() {
        let mut p = Project::default();
        while p.machines.len() < 16 {
            let m = p.new_machine(MachineType::Kit909).unwrap();
            p.insert_machine(p.machines.len(), m).unwrap();
        }
        let mut e = Engine::from_project(SR, &p);
        assert_eq!(e.capacity(), 16);
        e.play();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..100 {
            e.process(&mut l, &mut r);
        }
        let before = e.positions();
        let mut q = p.clone();
        let m = q.new_machine(MachineType::Fm).unwrap();
        q.insert_machine(q.machines.len(), m).unwrap();
        let mut swap = RackSwap::build(&q, &e.slot_uids(), SR, e.capacity());
        assert_eq!(swap.capacity, 32);
        assert!(swap.growth.is_some());
        assert!(e.set_rack(&mut swap));
        assert_eq!(e.capacity(), 32);
        let after = e.positions();
        assert_eq!(
            after.playhead[..16],
            before.playhead[..16],
            "tracks kept their place"
        );
        assert_eq!(after.len(), 17);
        let g = swap.growth.as_ref().unwrap();
        assert_eq!(
            g.seq.tracks.len(),
            16,
            "the old buffers come back for disposal"
        );
    }

    #[test]
    fn a_swap_beyond_the_engines_capacity_without_growth_is_refused() {
        let p = Project::default();
        let mut e = Engine::from_project(SR, &p);
        let mut q = p.clone();
        for _ in 0..12 {
            let m = q.new_machine(MachineType::Fm).unwrap();
            q.insert_machine(q.machines.len(), m).unwrap();
        }
        let mut swap = RackSwap::build(&q, &e.slot_uids(), SR, e.capacity());
        swap.growth = None;
        assert!(!e.set_rack(&mut swap));
        assert_eq!(e.slot_uids(), p.rack_uids());
    }

    #[test]
    fn growth_with_the_same_rack_moves_state_into_bigger_buffers() {
        let p = Project::default();
        let mut e = Engine::from_project(SR, &p);
        e.play();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..100 {
            e.process(&mut l, &mut r);
        }
        let before = e.positions();
        let mut q = p.clone();
        q.machines.swap(0, 1); // a real edit, so the swap is sent
        let mut swap = RackSwap::build(&q, &e.slot_uids(), SR, 16);
        swap.capacity = 32;
        swap.growth = Some(Growth::new(32));
        assert!(e.set_rack(&mut swap));
        assert_eq!(e.capacity(), 32);
        let after = e.positions();
        assert_eq!(after.playhead[0], before.playhead[1]);
    }

    /// Uid of the standard rack's machine at position `pos`.
    fn pos_uid(pos: usize) -> MachineUid {
        MachineUid(pos as u16 + 1)
    }

    /// Stores `p` and returns the old pattern, or `p` itself if refused.
    fn set_pattern(e: &mut Engine, uid: MachineUid, index: u8, p: PatternData) -> PatternData {
        let mut p = p;
        e.swap_pattern(uid, index, &mut p);
        p
    }

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
        assert_eq!(e.knob(MachineUid(3).param(SynthParam::Tune)), None);
        assert_eq!(
            e.knob(MachineUid(3).param(MachineParam::Drum(8, DrumParam::Tune))),
            None
        );
        assert_eq!(
            e.knob(MachineUid(5).param(MachineParam::FmOp(6, OpParam::On))),
            None
        );
        assert_eq!(e.knob(MachineUid(7).param(ChannelParam::Level)), None);
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
        let non_fitting_id = ParamId::Machine(MachineUid(2), MachineParam::Fm(FmParam::Volume));
        eng.set_param(non_fitting_id, 0.5);
        assert_eq!(eng.knob(non_fitting_id), None);
    }

    #[test]
    fn rebuilding_the_rack_resets_selections_from_the_project() {
        let mut p = Project::default();
        p.machines[1].selected = 4;
        let mut eng = Engine::from_project(SR, &p);
        eng.select_pattern(MachineUid(1), 9);
        eng.select_pattern(MachineUid(6), 7);
        let mut q = Project::default();
        q.machines.truncate(3);
        q.machines[2].selected = 2;
        eng.load_project(&q);
        let patterns = |eng: &Engine| -> Vec<u8> {
            (0..RACK_CHUNK)
                .map(|i| eng.seq.current_pattern(i))
                .collect()
        };
        let mut want = vec![0; RACK_CHUNK];
        want[2] = 2;
        assert_eq!(patterns(&eng), want, "positions past the rack reset too");
        eng.load_project(&p);
        want[2] = 0;
        want[1] = 4;
        assert_eq!(patterns(&eng), want);
    }

    /// A full rack of `types` at its busiest: dense patterns on every slot
    /// (every drum hit; on note machines 512 notes over 128 pitches that
    /// all end and restart on each wrap; synth lanes) and a song lane on
    /// every param.
    fn dense_rack(types: &[MachineType]) -> Project {
        use crate::params::DRUM_INSTRUMENTS;
        use crate::project::{MachineBody, Note, ParamLane};
        use crate::steps::{ACID_VEL, DrumGrid, DrumHit};
        let mut p = Project::default();
        p.machines = (1..=RACK_CHUNK as u16)
            .zip(types.iter().cycle())
            .map(|(u, &ty)| MachineEntry::new(MachineUid(u), ty, format!("M{u}")))
            .collect();
        p.next_uid = RACK_CHUNK as u16 + 1;
        for m in &mut p.machines {
            let uid = m.uid;
            match &mut m.body {
                MachineBody::Synth(s) => {
                    let pat = &mut s.patterns[0];
                    pat.roll.length = 1;
                    pat.roll.notes.push(Note::new(0, 4, 36, ACID_VEL));
                    pat.param_lane = Some(ParamLane {
                        param: ParamId::Machine(uid, MachineParam::Synth(SynthParam::Cutoff)),
                        values: vec![Some(0.5); crate::project::STEPS],
                    });
                }
                MachineBody::Kit(k) => {
                    k.patterns[0].roll = DrumGrid {
                        steps: vec![[DrumHit::Accent; DRUM_INSTRUMENTS]; crate::project::STEPS],
                        length: 16,
                    }
                    .to_roll();
                }
                MachineBody::Fm(crate::project::FmMachine { patterns, .. })
                | MachineBody::Sampler(crate::project::SamplerMachine { patterns, .. })
                | MachineBody::Mono(crate::project::MonoMachine { patterns, .. }) => {
                    let pat = &mut patterns[0];
                    pat.length = 1;
                    pat.notes = (0..MAX_NOTES)
                        .map(|i| Note::new(0, 4, (i % 128) as u8, 100))
                        .collect();
                }
            }
        }
        let bar: Vec<_> = p.machines.iter().map(|m| (m.uid, 0)).collect();
        p.song = crate::testutil::bar_song(&p, &[bar.clone(), bar]);
        p.song.loop_range = Some((0, 32));
        let ids: Vec<ParamId> = p
            .machines
            .iter()
            .flat_map(|m| {
                let uid = m.uid;
                MachineParam::all_for(m.machine_type())
                    .into_iter()
                    .map(move |mp| ParamId::Machine(uid, mp))
            })
            .chain(GlobalParam::all().into_iter().map(ParamId::from))
            .collect();
        for id in ids {
            crate::song::write_automation(&mut p.song, id, 0, 0, 0.5);
        }
        p.validate().unwrap();
        p
    }

    /// Plays `p` for a second in each mode; returns the most events one
    /// tick produced, asserting the event buffer never grew.
    fn busiest_tick(p: &Project) -> usize {
        let mut eng = Engine::from_project(SR, p);
        assert_eq!(eng.slots.len(), RACK_CHUNK);
        assert_eq!(eng.slot_capacity(), RACK_CHUNK);
        assert_eq!(p.song.automation.len(), eng.param_count, "a lane per param");
        let reserve = eng.events.capacity();
        assert!(reserve >= event_reserve(eng.capacity()));
        let mut most = 0;
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        for mode in [Mode::Pattern, Mode::Song] {
            eng.stop();
            eng.set_mode(mode);
            eng.play();
            for _ in 0..(SR as usize / 64) {
                eng.process(&mut l, &mut r);
                most = most.max(eng.events.len());
            }
        }
        assert_eq!(eng.events.capacity(), reserve, "the buffer never grew");
        most
    }

    #[test]
    fn a_full_rack_tick_fits_the_event_reserve() {
        let mixed = busiest_tick(&dense_rack(&MachineType::ALL));
        assert!(mixed > RACK_CHUNK * 128, "{mixed}");
        let notes = busiest_tick(&dense_rack(&[MachineType::Fm, MachineType::Sampler]));
        assert!(
            notes >= RACK_CHUNK * (128 + MAX_NOTES),
            "every note slot releases 128 and starts MAX_NOTES on a wrap: {notes}"
        );
    }

    #[test]
    fn a_reordered_rack_plays_the_same_patterns() {
        // Same machines, reversed order: every machine still plays its own pattern.
        let p = crate::demo::demo_project();
        let mut q = p.clone();
        q.machines.reverse();
        let a = crate::render::render_patterns(&p, 2);
        let b = crate::render::render_patterns(&q, 2);
        // Summation order differs, so compare energy, not bits.
        let e = |x: &(Vec<f32>, Vec<f32>)| x.0.iter().map(|s| (s * s) as f64).sum::<f64>();
        assert!((e(&a) - e(&b)).abs() / e(&a) < 1e-3);
    }

    #[test]
    fn a_second_303_sequences_its_own_patterns() {
        use crate::project::MachineBody;
        let mut p = Project::default();
        let mut e = MachineEntry::new(MachineUid(7), MachineType::Synth, "Synth 3".into());
        if let MachineBody::Synth(s) = &mut e.body {
            s.patterns[0] = p.synth(MachineUid(1)).unwrap().patterns[0].clone();
        }
        p.machines.push(e);
        p.next_uid = 8;
        let mut eng = Engine::from_project(48000.0, &p);
        eng.play();
        // Past the first step (5760 samples): the playhead is the step
        // of the last tick run.
        let (mut l, mut r) = (vec![0.0; 8192], vec![0.0; 8192]);
        eng.process(&mut l, &mut r);
        assert!(eng.positions().playhead[6] > 0, "slot 7 advanced");
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
        assert_eq!(eng.slot_capacity(), RACK_CHUNK);
        let mut too_many = p.clone();
        too_many.machines.push(MachineEntry::new(
            MachineUid(17),
            MachineType::Fm,
            "FM 2".into(),
        ));
        too_many.next_uid = 18;
        too_many.validate().unwrap();
        let eng = Engine::from_project(48000.0, &too_many);
        assert_eq!(eng.slot_capacity(), 2 * RACK_CHUNK);
    }

    #[test]
    fn a_rack_swap_keeps_surviving_machines_playing() {
        let p = crate::demo::demo_project();
        let mut eng = Engine::from_project(48000.0, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 2048], vec![0.0; 2048]);
        eng.process(&mut l, &mut r);
        let head = eng.positions().playhead[0];
        // Remove the 909 (uid 4), move Synth 1 last, add an FM.
        let mut q = p.clone();
        q.machines.retain(|m| m.uid != MachineUid(4));
        let s1 = q.machines.remove(0);
        q.machines.push(s1);
        q.machines.push(MachineEntry::new(
            MachineUid(7),
            MachineType::Fm,
            "FM 2".into(),
        ));
        q.next_uid = 8;
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0, eng.capacity());
        eng.set_rack(&mut swap);
        let order: Vec<u16> = eng.slot_uids().iter().map(|u| u.0).collect();
        assert_eq!(order, [2, 3, 5, 6, 1, 7]);
        assert_eq!(
            eng.positions().playhead[4],
            head,
            "Synth 1 kept its sequencer position"
        );
        assert_eq!(swap.into.len(), 1, "the 909 came back for disposal");
        assert_eq!(swap.into[0].uid, MachineUid(4));
    }

    #[test]
    fn a_machine_added_while_playing_joins_in_phase() {
        let p = crate::demo::demo_project();
        let mut eng = Engine::from_project(48000.0, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 48000], vec![0.0; 48000]); // ~1 s: well past step 0
        eng.process(&mut l, &mut r);
        let mut q = p.clone();
        let mut e = MachineEntry::new(MachineUid(7), MachineType::Synth, "Synth 3".into());
        if let MachineBody::Synth(s) = &mut e.body {
            s.patterns[0] = q.synth(MachineUid(1)).unwrap().patterns[0].clone();
        }
        q.machines.push(e);
        q.next_uid = 8;
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0, eng.capacity());
        eng.set_rack(&mut swap);
        assert_eq!(
            eng.positions().playhead[6],
            eng.positions().playhead[0],
            "same pattern length, same step"
        );
    }

    #[test]
    fn a_note_machine_added_while_playing_joins_in_phase() {
        let p = crate::demo::demo_project();
        let mut eng = Engine::from_project(48000.0, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 30000], vec![0.0; 30000]);
        eng.process(&mut l, &mut r);
        let mut q = p.clone();
        q.machines.push(MachineEntry::new(
            MachineUid(7),
            MachineType::Fm,
            "FM 2".into(),
        ));
        q.next_uid = 8;
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0, eng.capacity());
        assert!(eng.set_rack(&mut swap));
        // Position 4 is the demo's FM, on the same default pattern.
        assert_ne!(eng.positions().playhead[4], 0, "past step 0");
        assert_eq!(eng.positions().playhead[6], eng.positions().playhead[4]);
        // And they stay together.
        eng.process(&mut l, &mut r);
        assert_eq!(eng.positions().playhead[6], eng.positions().playhead[4]);
    }

    #[test]
    fn a_machine_added_during_a_song_plays_its_block_at_once() {
        let mut p = crate::demo::demo_project();
        let bar = vec![(MachineUid(1), 0)];
        p.song = crate::testutil::bar_song(&p, &[bar.clone(), bar]);
        let mut eng = Engine::from_project(48000.0, &p);
        eng.set_mode(Mode::Song);
        eng.play();
        // ~1 s at 128 bpm: mid first bar.
        let (mut l, mut r) = (vec![0.0; 48000], vec![0.0; 48000]);
        eng.process(&mut l, &mut r);
        assert_eq!(eng.song_bar(), 0);
        let mut q = p.clone();
        let mut e = MachineEntry::new(MachineUid(7), MachineType::Synth, "Synth 3".into());
        if let MachineBody::Synth(s) = &mut e.body {
            s.patterns[2] = q.synth(MachineUid(1)).unwrap().patterns[0].clone();
        }
        q.machines.push(e);
        q.next_uid = 8;
        q.song.lane_mut(MachineUid(7)).extend(
            [0, 16].map(|start| crate::project::Block::new(start, crate::project::PatternRef(2))),
        );
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0, eng.capacity());
        assert!(eng.set_rack(&mut swap));
        assert_eq!(
            eng.positions().patterns[6],
            2,
            "the block's pattern, not the selection"
        );
        assert_eq!(eng.positions().playhead[6], eng.positions().playhead[0]);
        eng.take_all_meters();
        let (mut l, mut r) = (vec![0.0; 12000], vec![0.0; 12000]);
        eng.process(&mut l, &mut r);
        assert_eq!(eng.song_bar(), 0, "still the same block");
        assert!(
            eng.take_all_meters()[6] > 0.0,
            "sounds before the next block"
        );
    }

    #[test]
    fn an_empty_rack_renders_silence() {
        let mut p = Project::default();
        p.machines.clear();
        p.validate().unwrap();
        let (l, r) = crate::render::render_patterns(&p, 1);
        assert!(!l.is_empty() && l.iter().chain(&r).all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn a_surviving_voice_keeps_sounding_through_a_swap() {
        let p = Project::default();
        let mut eng = Engine::from_project(48000.0, &p);
        eng.note_on(MachineUid(5), 60, 100); // FM audition (Task 7 API)
        let (mut l, mut r) = (vec![0.0; 1024], vec![0.0; 1024]);
        eng.process(&mut l, &mut r);
        let mut q = p.clone();
        q.machines.retain(|m| m.uid != MachineUid(1));
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0, eng.capacity());
        eng.set_rack(&mut swap);
        eng.process(&mut l, &mut r);
        assert!(l.iter().any(|s| s.abs() > 1e-4), "FM still sounding");
    }

    #[test]
    fn a_rack_swap_moves_track_state_and_selects_new_patterns_at_once() {
        let mut p = Project::default();
        p.machines[2].selected = 3; // the 808
        let mut eng = Engine::from_project(SR, &p);
        eng.select_pattern(MachineUid(2), 5);
        eng.play();
        // Queued while playing: the queue moves with Synth 2.
        eng.select_pattern(MachineUid(2), 6);
        let mut q = p.clone();
        q.machines.swap(0, 1); // Synth 2 first
        let mut e = MachineEntry::new(MachineUid(7), MachineType::Synth, "Synth 3".into());
        e.selected = 9;
        q.machines.insert(0, e);
        q.next_uid = 8;
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), SR, eng.capacity());
        assert!(swap.fresh[0].is_some() && swap.fresh[1..].iter().all(Option::is_none));
        assert!(eng.set_rack(&mut swap));
        let pats = eng.positions().patterns;
        assert_eq!(&pats[..4], &[9, 5, 0, 3], "new slot switched, not queued");
        assert_eq!(eng.positions().queued[0], None);
        assert_eq!(eng.positions().queued[1], Some(6));
        assert!(swap.into.is_empty());
    }

    #[test]
    fn a_malformed_rack_swap_changes_nothing() {
        let p = Project::default();
        let mut eng = Engine::from_project(SR, &p);
        let before = eng.slot_uids();
        let mut q = p.clone();
        q.machines.push(MachineEntry::new(
            MachineUid(7),
            MachineType::Fm,
            "FM 2".into(),
        ));
        q.next_uid = 8;
        let good = || RackSwap::build(&q, &before, SR, 16);
        // Duplicate uid.
        let mut dup = good();
        dup.order[1] = dup.order[0];
        // A uid neither fresh nor in the rack.
        let mut unknown = good();
        unknown.order[2] = MachineUid(42);
        // Longer than the rack cap.
        let mut long = good();
        long.order = (1..=17).map(MachineUid).collect();
        long.fresh = (0..17).map(|_| None).collect();
        // Fresh list of the wrong length.
        let mut short = good();
        short.fresh.pop();
        // A growth no bigger than the engine holds.
        let mut same = good();
        same.growth = Some(Growth::new(16));
        // A growth whose buffers are smaller than its capacity.
        let mut odd = good();
        let mut g = Growth::new(32);
        g.capacity = 48;
        (odd.capacity, odd.growth) = (48, Some(g));
        odd.into = Vec::with_capacity(48);
        // A growth with no room for events.
        let mut mute = good();
        let mut g = Growth::new(32);
        g.events = Vec::new();
        (mute.capacity, mute.growth) = (32, Some(g));
        for mut swap in [dup, unknown, long, short, same, odd, mute] {
            let fresh = swap.fresh.iter().filter(|s| s.is_some()).count();
            assert!(!eng.set_rack(&mut swap));
            assert_eq!(eng.slot_uids(), before);
            assert!(swap.into.is_empty());
            assert_eq!(swap.fresh.iter().filter(|s| s.is_some()).count(), fresh);
        }
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        eng.process(&mut l, &mut r);
    }

    #[test]
    fn a_slots_meter_follows_it_through_a_rack_edit() {
        // Two identical engines render the same audio; one then moves its
        // first machine to the end. Meters, keyed by uid, must agree.
        let p = crate::demo::demo_project(); // the default project is silent
        let (mut a, mut b) = (Engine::from_project(SR, &p), Engine::from_project(SR, &p));
        a.play();
        b.play();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..200 {
            a.process(&mut l, &mut r);
            b.process(&mut l, &mut r);
        }
        let mut q = p.clone();
        let first = q.machines.remove(0);
        q.machines.push(first);
        let mut swap = RackSwap::build(&q, &b.slot_uids(), SR, b.capacity());
        assert!(b.set_rack(&mut swap));
        let by_uid = |e: &mut Engine| {
            let uids = e.slot_uids();
            let mut v = Vec::new();
            e.take_slot_stats(|pos, m, ns| v.push((uids[pos], m, ns > 0)));
            v.sort_by_key(|x| x.0);
            v
        };
        let (sa, sb) = (by_uid(&mut a), by_uid(&mut b));
        assert!(sa.iter().any(|x| x.1 > 0.0), "something played: {sa:?}");
        assert_eq!(sa, sb, "each slot kept its own meter and time");
        assert!(
            by_uid(&mut a).iter().all(|x| x.1 == 0.0 && !x.2),
            "taking resets"
        );
    }

    #[test]
    fn meters_follow_their_machine_through_a_swap() {
        let mut p = Project::default();
        p.machines.truncate(5); // drop the sampler
        let mut eng = Engine::from_project(SR, &p);
        eng.note_on(MachineUid(5), 60, 127);
        let (mut l, mut r) = (vec![0.0; 1024], vec![0.0; 1024]);
        eng.process(&mut l, &mut r);
        let mut q = p.clone();
        let fm = q.machines.remove(4);
        q.machines.insert(0, fm);
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), SR, eng.capacity());
        assert!(eng.set_rack(&mut swap));
        let map = &swap.map;
        assert_eq!(map[0], Some(4));
        assert_eq!(map[1], Some(0));
        let meters = eng.take_all_meters();
        assert!(meters[0] > 0.0, "the FM's peak moved with it");
        assert_eq!(meters[4], 0.0);
    }

    #[test]
    fn a_rack_swap_reports_where_each_slot_came_from() {
        let p = Project::default();
        let mut e = Engine::from_project(SR, &p);
        let mut q = p.clone();
        q.machines.swap(0, 2);
        let added = q.new_machine(MachineType::Fm).unwrap();
        q.insert_machine(q.machines.len(), added).unwrap();
        let mut swap = RackSwap::build(&q, &e.slot_uids(), SR, e.capacity());
        assert!(e.set_rack(&mut swap));
        assert_eq!(
            swap.map,
            vec![Some(2), Some(1), Some(0), Some(3), Some(4), Some(5), None]
        );
    }

    /// Gives `uid`'s sampler slot 0 about two seconds of noise played
    /// chromatically over keys 36..=96.
    fn load_long_sample(p: &mut Project, uid: MachineUid) {
        use crate::sampler::slot::{SlotData, SlotMeta};
        let frames = 96_000u32;
        let mut x = 0x1234_5678u32;
        let noise: Vec<f32> = (0..frames)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32 - 0.5) * 0.5
            })
            .collect();
        let meta = SlotMeta::chromatic(36, 96, 60, frames);
        p.sampler_mut(uid).unwrap().slots[0] =
            Some(SlotData::new("Noise", 48_000, 1, &noise, meta));
    }

    #[test]
    fn a_busy_sampler_costs_more_than_an_idle_synth() {
        // Rack: Synth 1 (no notes playing) and a Sampler with a long sample and 16 held notes.
        let mut p = Project::default();
        p.machines
            .retain(|m| m.uid == MachineUid(1) || m.machine_type() == MachineType::Sampler);
        let sampler_uid = p
            .machines
            .iter()
            .find(|m| m.machine_type() == MachineType::Sampler)
            .unwrap()
            .uid;
        load_long_sample(&mut p, sampler_uid);
        let mut eng = Engine::from_project(48000.0, &p);
        for k in 0..16 {
            eng.note_on(sampler_uid, 40 + k, 100);
        }
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..200 {
            eng.process(&mut l, &mut r);
        }
        let mut ns = [0u64; RACK_CHUNK];
        eng.take_slot_stats(|pos, _, t| ns[pos] = t);
        let pos_s = p.position(sampler_uid).unwrap();
        let pos_y = 1 - pos_s;
        assert!(
            ns[pos_s] > ns[pos_y],
            "sampler {} ns vs idle synth {} ns",
            ns[pos_s],
            ns[pos_y]
        );
        eng.take_slot_stats(|_, _, t| assert_eq!(t, 0, "taking resets"));
    }

    #[test]
    fn cpu_accumulators_follow_their_machine_through_a_reorder() {
        let p = crate::demo::demo_project();
        let mut eng = Engine::from_project(48000.0, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..50 {
            eng.process(&mut l, &mut r);
        }
        let mut q = p.clone();
        q.machines.reverse();
        let before = eng.peek_cpu_ns();
        let mut swap = RackSwap::build(&q, &eng.slot_uids(), 48000.0, eng.capacity());
        eng.set_rack(&mut swap);
        let after = eng.peek_cpu_ns();
        for i in 0..6 {
            assert_eq!(after.0[5 - i], before.0[i]);
        }
        assert!(before.0[..6].iter().all(|&ns| ns > 0), "{before:?}");
        assert_eq!(after.1, before.1, "shared work stays put");
    }

    const SR: f32 = 44_100.0;

    impl Engine {
        /// The CPU accumulators, without resetting them.
        fn peek_cpu_ns(&self) -> (Vec<u64>, u64) {
            (
                self.slots.iter().map(|s| s.cpu_ns).collect(),
                self.shared_ns,
            )
        }

        /// A fresh buffer the size of the rack, filled; takes the slots'
        /// meters and CPU time.
        pub(crate) fn positions(&mut self) -> Box<PositionStatus> {
            let mut b = PositionStatus::with_capacity(self.slots.len());
            self.fill_positions(&mut b, |_| 0.0);
            b
        }

        /// Slot peaks by rack position, the master last; resets them.
        fn take_all_meters(&mut self) -> Vec<f32> {
            let mut m = vec![0.0; self.slots.len()];
            self.take_slot_stats(|pos, v, _| m[pos] = v);
            m.push(self.take_master_meter());
            m
        }

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

    /// `song` compiled for the standard rack.
    fn compiled(song: &crate::project::Song) -> CompiledSong {
        CompiledSong::compile(song, &Project::default().rack_uids())
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
        p.sampler_mut(MachineUid(6)).unwrap().slots[0] = Some(tone_slot(SlotMeta::sliced(
            36,
            vec![0, 11_025, 22_050, 33_075],
            120.0,
            44_100,
        )));
        p.sampler_mut(MachineUid(6)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 4, 37, 120));
        p
    }

    #[test]
    fn the_sampler_plays_loaded_slots_on_channel_5() {
        let mut e = Engine::from_project(SR, &sampler_project());
        e.play();
        render(&mut e, 0.2);
        let m = e.take_all_meters();
        assert!(m[5] > 0.01, "{m:?}");
        assert!(m[..5].iter().all(|v| *v == 0.0), "{m:?}");
    }

    #[test]
    fn sampler_audition_and_slot_swaps() {
        let mut e = Engine::from_project(SR, &sampler_project());
        e.note_on(pos_uid(5), 38, 100);
        let (l, _) = render(&mut e, 0.1);
        assert!(peak(&l) > 0.01, "audition plays slice 3");
        let mut none = None;
        assert!(
            !e.swap_sampler_sample(pos_uid(4), 0, &mut none),
            "not a sampler"
        );
        assert!(!e.swap_sampler_sample(MachineUid(99), 0, &mut none));
        assert!(none.is_none(), "refused swaps leave the sample alone");
        assert!(e.swap_sampler_sample(pos_uid(5), 0, &mut none));
        assert!(none.is_some());
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-3, "slot emptied");
        let mut play = crate::sampler::slot::SlotPlay::EMPTY;
        assert!(e.swap_sampler_play(pos_uid(5), 0, &mut play));
        assert!(!e.swap_sampler_play(pos_uid(5), 16, &mut play));
        assert!(
            !e.swap_sampler_play(pos_uid(0), 0, &mut play),
            "not a sampler"
        );
    }

    #[test]
    fn stopping_twice_silences_sampler_loops() {
        use crate::sampler::slot::SlotMeta;
        let mut p = Project::default();
        let mut meta = SlotMeta::chromatic(48, 71, 60, 44_100);
        meta.loop_on = true;
        meta.loop_start = 10_000;
        meta.loop_end = 30_000;
        p.sampler_mut(MachineUid(6)).unwrap().slots[0] = Some(tone_slot(meta));
        p.set_param(
            MachineUid(6).param(MachineParam::SamplerSlot(
                0,
                crate::params::SlotParam::Release,
            )),
            1.0,
        );
        let mut e = Engine::from_project(SR, &p);
        e.note_on(pos_uid(5), 60, 100);
        render(&mut e, 0.1);
        e.stop();
        e.stop();
        let (l, _) = render(&mut e, 0.05);
        assert_eq!(peak(&l), 0.0);
    }

    /// A project whose sampler slot 0 holds a chromatic tone (keys 36..=96).
    fn tone_slot_project() -> Project {
        let mut p = Project::default();
        p.sampler_mut(MachineUid(6)).unwrap().slots[0] = Some(tone_slot(
            crate::sampler::slot::SlotMeta::chromatic(36, 96, 60, 44_100),
        ));
        p
    }

    #[test]
    fn note_patterns_play_on_their_own_channels() {
        use crate::project::Note;
        let mut p = tone_slot_project();
        p.fm_mut(MachineUid(5)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 32, 60, 120));
        p.sampler_mut(MachineUid(6)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 32, 72, 120));
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.5);
        let m = e.take_all_meters();
        assert!(m[4] > 0.01 && m[5] > 0.01, "{m:?}");
        assert!(m[..4].iter().all(|v| *v == 0.0), "{m:?}");
    }

    #[test]
    fn live_notes_audition_while_stopped() {
        let mut e = Engine::from_project(SR, &tone_slot_project());
        e.note_on(pos_uid(0), 60, 100);
        e.note_on(pos_uid(4), 200, 100);
        e.note_on(pos_uid(5), 60, 0);
        let (l, _) = render(&mut e, 0.05);
        assert_eq!(peak(&l), 0.0, "bad machine, pitch and velocity are ignored");
        e.note_on(pos_uid(5), 60, 100);
        let (l, _) = render(&mut e, 0.1);
        assert!(peak(&l) > 0.01);
        e.note_off(pos_uid(5), 60);
        render(&mut e, 0.3);
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-4);
    }

    #[test]
    fn stop_releases_note_machines() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineUid(5)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 64, 60, 120));
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
        p.set_param(MachineUid(5).param(MachineParam::FmOp(0, OpParam::L4)), 1.0);
        let mut e = Engine::from_project(SR, &p);
        e.play();
        e.note_on(pos_uid(4), 60, 100);
        render(&mut e, 0.1);
        e.note_off(pos_uid(4), 60);
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
        p.fm_mut(MachineUid(5)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 64, 60, 120));
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.2);
        e.play();
        let mut empty = PatternData::Notes(NotePattern::default());
        assert!(e.swap_pattern(pos_uid(4), 0, &mut empty));
        render(&mut e, 0.3);
        let (l, _) = render(&mut e, 0.05);
        assert!(peak(&l) < 1e-4, "no voice hangs after a restart");
    }

    #[test]
    fn deleting_a_sounding_note_while_playing_releases_it() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineUid(5)).unwrap().patterns[0].length = 64;
        p.fm_mut(MachineUid(5)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 8, 60, 120));
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.1);
        let mut empty = PatternData::Notes(NotePattern {
            length: 64,
            ..NotePattern::default()
        });
        assert!(e.swap_pattern(pos_uid(4), 0, &mut empty));
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
        p.fm_mut(MachineUid(5)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 32, 60, 120));
        p.set_param(MachineUid(5).param(FmParam::Volume), 0.0);
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
            e.set_param(MachineUid(c as u16 + 1).param(ChannelParam::DelaySend), 0.0);
            e.set_param(
                MachineUid(c as u16 + 1).param(ChannelParam::ReverbSend),
                0.0,
            );
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
        let PatternData::Synth(old) = set_pattern(
            &mut e,
            pos_uid(0),
            0,
            PatternData::Synth(SynthPattern::default()),
        ) else {
            panic!("a synth pattern comes back")
        };
        assert!(!old.roll.notes.is_empty());
        let PatternData::Drum(old_drums) = set_pattern(
            &mut e,
            pos_uid(2),
            0,
            PatternData::Drum(DrumPattern::default()),
        ) else {
            panic!("a drum pattern comes back")
        };
        assert!(!old_drums.roll.notes.is_empty());
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
        e.select_pattern(pos_uid(0), 200);
        e.select_pattern(MachineUid(99), 0);
        e.set_param(
            MachineUid(3).param(MachineParam::Drum(9, DrumParam::Tune)),
            0.5,
        );
        e.set_param(MachineUid(8).param(SynthParam::Cutoff), 0.5);
        e.set_param(MachineUid(7).param(ChannelParam::Level), 0.5);
        e.set_param(MachineUid(5).param(KitParam::Volume), 0.5);
        let (l, _) = render(&mut e, 0.5);
        assert!(all_finite(&l));
        assert!(e.positions().patterns.iter().all(|&p| p == 0));
    }

    #[test]
    fn malformed_or_misplaced_patterns_are_rejected() {
        let mut e = demo_engine();
        e.play();
        let mut short = SynthPattern::default();
        short.roll.length = 0;
        let short = PatternData::Synth(short);
        assert_eq!(set_pattern(&mut e, pos_uid(0), 0, short.clone()), short);
        let mut zero = DrumPattern::default();
        zero.roll.length = 0;
        let zero = PatternData::Drum(zero);
        assert_eq!(set_pattern(&mut e, pos_uid(2), 0, zero.clone()), zero);
        let mut ninth = DrumPattern::default();
        ninth
            .roll
            .notes
            .push(crate::project::Note::new(0, 4, 8, 100));
        let ninth = PatternData::Drum(ninth);
        assert_eq!(
            set_pattern(&mut e, pos_uid(2), 0, ninth.clone()),
            ninth,
            "no ninth drum"
        );
        let mut lane = SynthPattern::default();
        lane.param_lane = Some(crate::project::ParamLane {
            param: ParamId::Fx(crate::params::FxParam::DelayMs),
            values: vec![None; 2],
        });
        let lane = PatternData::Synth(lane);
        assert_eq!(set_pattern(&mut e, pos_uid(0), 0, lane.clone()), lane);
        let synth = PatternData::Synth(SynthPattern::default());
        assert_eq!(
            set_pattern(&mut e, MachineUid(99), 0, synth.clone()),
            synth,
            "unknown uid"
        );
        assert_eq!(
            set_pattern(&mut e, pos_uid(2), 0, synth.clone()),
            synth,
            "a synth pattern on a kit"
        );
        let drums = PatternData::Drum(DrumPattern::default());
        assert_eq!(
            set_pattern(&mut e, pos_uid(2), 200, drums.clone()),
            drums,
            "index past the bank"
        );
        let (l, _) = render(&mut e, 1.0);
        assert!(all_finite(&l) && rms(&l) > 0.02, "still playing the demo");
    }

    #[test]
    fn non_finite_param_is_dropped_and_values_clamped() {
        let mut e = demo_engine();
        e.set_param(MachineUid(1).param(SynthParam::Cutoff), f32::NAN);
        e.set_param(MachineUid(1).param(ChannelParam::Level), 50.0);
        e.play();
        let (l, _) = render(&mut e, 0.1);
        assert!(all_finite(&l));
        e.set_param(MachineUid(1).param(SynthParam::Cutoff), 0.5);
        let (l, _) = render(&mut e, 3.0);
        assert!(all_finite(&l) && peak(&l) > 0.01);
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn playhead_advances() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.3);
        assert!(e.positions().playhead[2] >= 2);
        assert!(e.positions().patterns.iter().all(|&p| p == 0));
    }

    #[test]
    fn lane_value_overrides_only_its_step() {
        use crate::project::ParamLane;
        let mut e = demo_engine();
        let mut p = crate::demo::demo_project()
            .synth(MachineUid(1))
            .unwrap()
            .patterns[0]
            .clone();
        let mut values = vec![None; 64];
        values[0] = Some(0.0);
        p.param_lane = Some(ParamLane {
            param: ParamId::Master(MasterParam::Level),
            values,
        });
        set_pattern(&mut e, pos_uid(0), 0, PatternData::Synth(p));
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
        let mut p = PatternData::Synth(SynthPattern::default());
        assert!(e.swap_pattern(pos_uid(0), 0, &mut p));
        let PatternData::Synth(p) = p else {
            panic!("a synth pattern comes back")
        };
        assert!(!p.roll.notes.is_empty(), "got the old pattern back");
        let mut bad = SynthPattern::default();
        bad.roll.length = 0;
        let mut bad = PatternData::Synth(bad);
        assert!(!e.swap_pattern(pos_uid(0), 1, &mut bad));
        assert!(
            matches!(&bad, PatternData::Synth(b) if b.roll.length == 0),
            "rejected pattern is untouched"
        );
        let mut d = PatternData::Drum(DrumPattern::default());
        assert!(e.swap_pattern(pos_uid(3), 0, &mut d));
        assert!(!e.swap_pattern(pos_uid(4), 0, &mut d), "not a kit");
    }

    #[test]
    fn meters_report_and_reset() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.5);
        let m = e.take_all_meters();
        assert!(
            m[..4].iter().all(|v| *v > 0.01),
            "both synths and kits: {m:?}"
        );
        let master = *m.last().unwrap();
        assert!(master > 0.01, "{m:?}");
        assert!(master <= 1.0 + 1e-6, "master is post-clip");
        e.stop();
        render(&mut e, 5.0);
        e.take_all_meters();
        render(&mut e, 0.05);
        assert!(e.take_all_meters().iter().all(|v| *v < 1e-3));
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
        e.select_pattern(pos_uid(3), 5);
        let mut want = vec![None; e.rack_len()];
        want[3] = Some(5);
        assert_eq!(e.positions().queued, want);
    }

    #[test]
    fn stop_restores_knobs_overridden_by_a_lane() {
        use crate::project::ParamLane;
        let mut e = demo_engine();
        let mut p = crate::demo::demo_project()
            .synth(MachineUid(1))
            .unwrap()
            .patterns[0]
            .clone();
        p.param_lane = Some(ParamLane {
            param: ParamId::Master(MasterParam::Level),
            values: vec![Some(0.0); 64],
        });
        set_pattern(&mut e, pos_uid(0), 0, PatternData::Synth(p));
        e.play();
        render(&mut e, 0.3);
        e.stop();
        set_pattern(
            &mut e,
            pos_uid(0),
            0,
            PatternData::Synth(
                crate::demo::demo_project()
                    .synth(MachineUid(1))
                    .unwrap()
                    .patterns[0]
                    .clone(),
            ),
        );
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert!(peak(&l) > 0.01, "master level was restored by stop()");
    }

    fn two_bar_demo() -> Project {
        let mut p = crate::demo::demo_project();
        let bar: Vec<_> = crate::machine::standard_rack()
            .iter()
            .map(|m| (m.0, 0))
            .collect();
        p.song = crate::testutil::bar_song(&p, &[bar.clone(), bar]);
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
        let mut good = compiled(&two_bar_demo().song);
        assert!(e.swap_song(&mut good));
        use crate::project::{Block, PATTERNS, PatternRef};
        let max = crate::song::MAX_SONG_STEPS as u32;
        let b = |start, slot| Block::new(start, PatternRef(slot));
        for (lanes, end) in [
            (vec![vec![b(max, 0)]], None),
            (vec![vec![b(0, PATTERNS as u8)]], None),
            (vec![vec![b(16, 0), b(0, 0)]], None),
            (vec![vec![b(0, 0), b(0, 1)]], None),
            (vec![vec![]; e.capacity() + 1], None),
            (Vec::new(), Some(max + 1)),
        ] {
            let mut bad = CompiledSong {
                lanes: lanes.clone(),
                end,
                ..Default::default()
            };
            assert!(!e.swap_song(&mut bad), "{lanes:?} {end:?}");
            assert_eq!(
                (bad.lanes, bad.end),
                (lanes, end),
                "rejected song is untouched"
            );
        }
    }

    #[test]
    fn looping_back_before_the_first_point_restores_the_knob() {
        let mut p = two_bar_demo();
        p.song.loop_range = Some((0, 32));
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
    fn swapping_the_song_mid_block_reseeks_the_tracks() {
        let mut e = Engine::from_project(SR, &two_bar_demo());
        let mut long = crate::demo::demo_project()
            .synth(MachineUid(1))
            .unwrap()
            .patterns[0]
            .clone();
        long.roll.length = 32;
        set_pattern(&mut e, pos_uid(0), 7, PatternData::Synth(long));
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
        assert_eq!(e.positions().patterns[0], 0);
        let mut song = two_bar_demo().song;
        song.lane_mut(MachineUid(1))[1].pattern = crate::project::PatternRef(7);
        assert!(e.swap_song(&mut compiled(&song)));
        for _ in 0..10_000 {
            if e.song_step() > 20 {
                break;
            }
            e.process(&mut l, &mut r);
        }
        assert_eq!(e.song_step(), 21, "the position stays");
        assert_eq!(e.song_bar(), 1);
        assert_eq!(e.positions().patterns[0], 7, "the new block's pattern");
        assert_eq!(e.positions().playhead[0], 4, "at its step 4");
    }

    #[test]
    fn song_start_and_position() {
        let mut e = Engine::from_project(SR, &two_bar_demo());
        e.set_mode(crate::sequencer::Mode::Song);
        e.set_song_start(16);
        e.play();
        render(&mut e, 0.01);
        assert_eq!(e.song_bar(), 1);
        assert_eq!(e.song_step(), 16);
        assert_eq!(e.mode(), crate::sequencer::Mode::Song);
    }

    #[test]
    fn note_patterns_swap_and_report() {
        use crate::project::Note;
        let mut e = demo_engine();
        let mut p = NotePattern::default();
        p.notes.push(Note::new(0, 4, 60, 100));
        let mut p = PatternData::Notes(p);
        assert!(e.swap_pattern(pos_uid(4), 2, &mut p));
        assert!(
            matches!(&p, PatternData::Notes(n) if n.notes.is_empty()),
            "got the old, empty pattern back"
        );
        assert!(!e.swap_pattern(pos_uid(0), 2, &mut p), "not a note machine");
        let mut bad = PatternData::Notes(NotePattern {
            length: 0,
            ..NotePattern::default()
        });
        assert!(!e.swap_pattern(pos_uid(5), 0, &mut bad));
        e.play();
        e.select_pattern(pos_uid(5), 9);
        assert_eq!(e.positions().queued[5], Some(9));
        assert_eq!(e.positions().patterns.len(), e.rack_len());
    }

    #[test]
    fn audition_survives_a_pattern_release_of_the_same_pitch() {
        use crate::project::Note;
        let mut p = Project::default();
        p.fm_mut(MachineUid(5)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 4, 60, 100));
        let mut held = Engine::from_project(SR, &p);
        held.play();
        held.note_on(pos_uid(4), 60, 100);
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
        p.fm_mut(MachineUid(5)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 32, 60, 120));
        p.set_param(
            MachineUid(5).param(MachineParam::FmOp(0, OpParam::Level)),
            0.0,
        );
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
        p.fm_mut(MachineUid(5)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 32, 57, 120));
        p.set_param(
            MachineUid(5).param(MachineParam::FmOp(1, OpParam::Level)),
            0.9,
        );
        let mut a = Engine::from_project(SR, &p);
        p.set_param(MachineUid(5).param(FmParam::Algorithm), 1.0);
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
            e.note_on(pos_uid(machine), 60, 100);
            render(&mut e, 0.05);
            e.play();
            let (l, _) = render(&mut e, 0.3);
            assert!(tail(&l) > 0.01, "machine {machine}: audition survives play");
            e.stop();
            let (l, _) = render(&mut e, 0.3);
            assert!(tail(&l) > 0.01, "machine {machine}: audition survives stop");
            e.note_off(pos_uid(machine), 60);
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
            e.note_on(pos_uid(machine), 72, 100);
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
        p.set_param(MachineUid(5).param(crate::params::FmParam::Mono), 1.0);
        let mut e = Engine::from_project(SR, &p);
        e.poly_on(4, 48, 100, false);
        e.note_on(pos_uid(4), 72, 100);
        e.play();
        assert_eq!(e.fm().sounding(), vec![72], "the audition keeps sounding");
        e.note_off(pos_uid(4), 72);
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
        p.set_param(MachineUid(5).param(MachineParam::FmOp(0, OpParam::L4)), 1.0);
        p.fm_mut(MachineUid(5)).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 8, 60, 120));
        p.song = crate::testutil::bar_song(&p, &[vec![(MachineUid(5), 0)]]);
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
            e.note_on(pos_uid(machine), 60, 100);
            e.note_off(pos_uid(machine), 60);
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

    #[test]
    fn a_mono_arp_plays_sixteen_sixteenths_in_a_bar() {
        use crate::params::{MachineParam, MonoParam};
        use crate::project::{MachineBody, Note};
        let mut p = Project::default();
        p.bpm = 120.0;
        let mut m = p.new_machine(MachineType::Mono).unwrap();
        m.params.insert(MachineParam::Mono(MonoParam::ArpOn), 1.0);
        if let MachineBody::Mono(mono) = &mut m.body {
            let pat = &mut mono.patterns[0];
            pat.length = 16;
            pat.notes = [48, 52, 55]
                .map(|pitch| Note::new(0, 64, pitch, 100))
                .to_vec();
        }
        let uid = m.uid;
        p.machines.push(m);
        let mut eng = Engine::from_project(SR, &p);
        eng.play();
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        // 120 BPM: one bar is 2 s, a 1/16 is SR / 8 samples.
        for _ in 0..(SR as usize * 2) / 64 {
            eng.process(&mut l, &mut r);
        }
        let pos = eng.position(uid).unwrap();
        let MachineDsp::Mono(mono) = &eng.slots[pos].dsp else {
            panic!("not a Mono slot")
        };
        assert_eq!(mono.arp_steps(), 16);
    }

    #[test]
    fn a_mono_arp_restarts_on_the_next_block_after_a_short_one() {
        use crate::params::{MachineParam, MonoParam};
        use crate::project::{Block, MachineBody, Note, PatternRef};
        let mut p = Project::default();
        p.bpm = 120.0;
        let mut m = p.new_machine(MachineType::Mono).unwrap();
        m.params.insert(MachineParam::Mono(MonoParam::ArpOn), 1.0);
        // 1/4: a step every 4 sixteenths.
        m.params.insert(MachineParam::Mono(MonoParam::ArpRate), 0.0);
        if let MachineBody::Mono(mono) = &mut m.body {
            for (slot, length) in [(0, 6u8), (1, 16)] {
                let pat = &mut mono.patterns[slot];
                pat.length = length;
                pat.notes = vec![Note::new(0, length as u16 * 4, 48, 100)];
            }
        }
        let uid = m.uid;
        p.machines.push(m);
        // A 6-step block, then a 16-step one.
        *p.song.lane_mut(uid) = vec![Block::new(0, PatternRef(0)), Block::new(6, PatternRef(1))];
        let mut eng = Engine::from_project(SR, &p);
        eng.set_mode(crate::sequencer::Mode::Song);
        eng.play();
        let pos = eng.position(uid).unwrap();
        let steps = |eng: &Engine| match &eng.slots[pos].dsp {
            MachineDsp::Mono(m) => m.arp_steps(),
            _ => panic!("not a Mono slot"),
        };
        let (mut l, mut r) = ([0.0; 1], [0.0; 1]);
        let mut third = None;
        for n in 0..SR as usize * 2 {
            eng.process(&mut l, &mut r);
            if steps(&eng) == 3 {
                third = Some(n);
                break;
            }
        }
        // Steps on beats 1 and 2 of the 6-step block, then the second one's
        // downbeat: 6 sixteenths in at 120 BPM, not a beat later.
        let bar_two = (6.0 * SR as f64 / 8.0) as usize;
        let third = third.expect("a third arp step");
        assert!(
            (bar_two..=bar_two + 2).contains(&third),
            "third step at {third}, bar two at {bar_two}"
        );
    }

    #[test]
    fn a_ratchet_hit_between_ticks_sounds_on_its_own_sample() {
        // A x2 one-tick kick: its second hit is half a tick in. Until then
        // it renders as the plain kick; from that sample on it doesn't.
        let kick = |ratchet: u8| {
            let mut p = Project::default();
            p.bpm = 120.0;
            p.swing = 0.0;
            let roll = p.roll_mut(MachineUid(3), 0).unwrap();
            roll.notes.clear();
            roll.notes.push(crate::project::Note {
                ratchet,
                ..crate::project::Note::new(0, 1, 0, 100)
            });
            let mut e = Engine::from_project(SR, &p);
            e.play();
            render(&mut e, 0.1).0
        };
        let (plain, twice) = (kick(1), kick(2));
        let half_tick = 60.0 / 120.0 / 16.0 * SR as f64 / 2.0;
        let first = plain.iter().zip(&twice).position(|(a, b)| a != b);
        assert_eq!(first, Some(half_tick.ceil() as usize));
    }

    #[test]
    fn held_ratchet_hits_cross_render_blocks_on_acid_and_poly() {
        // An x8 one-step note on the acid and on the sampler: rendered in
        // one go or in 37-sample blocks (held hits outlive a block), the
        // same; the second hit lands half a tick in. (The FM's envelopes
        // step per 32-sample block from each render call, so its output
        // depends on the blocks regardless of ratchets.)
        for (uid, pitch) in [(MachineUid(1), 48), (MachineUid(6), 37)] {
            let project = |ratchet: u8| {
                let mut p = sampler_project();
                p.bpm = 120.0;
                p.swing = 0.0;
                let roll = p.roll_mut(uid, 0).unwrap();
                roll.notes.clear();
                roll.notes.push(crate::project::Note {
                    ratchet,
                    ..crate::project::Note::new(0, 4, pitch, 100)
                });
                p
            };
            let whole = |p: &Project| {
                let mut e = Engine::from_project(SR, p);
                e.play();
                render(&mut e, 0.1).0
            };
            let chunked = |p: &Project| {
                let mut e = Engine::from_project(SR, p);
                e.play();
                let mut l = vec![0.0; (0.1 * SR) as usize];
                let mut r = l.clone();
                for (cl, cr) in l.chunks_mut(37).zip(r.chunks_mut(37)) {
                    e.process(cl, cr);
                }
                l
            };
            let x8 = project(8);
            let a = whole(&x8);
            assert_eq!(a, chunked(&x8), "{uid:?}");
            let half_tick = 60.0 / 120.0 / 16.0 * SR as f64 / 2.0;
            let first = whole(&project(1)).iter().zip(&a).position(|(x, y)| x != y);
            assert_eq!(first, Some(half_tick.ceil() as usize), "{uid:?}");
        }
    }

    #[test]
    fn a_pattern_edit_keeps_a_ringing_note_that_moved_index() {
        // FM: A 0-1 step, a skipped note, B 0-8 steps. Deleting A at
        // step 2 moves B to the skipped note's index; B keeps sounding.
        use crate::project::Note;
        let mut p = Project::default();
        p.bpm = 120.0;
        let pat = |notes: Vec<Note>| NotePattern {
            notes,
            ..NotePattern::default()
        };
        let skipped = Note {
            chance: 0,
            ..Note::new(0, 4, 50, 100)
        };
        let (a, b) = (Note::new(0, 4, 60, 100), Note::new(0, 32, 64, 100));
        *p.roll_mut(MachineUid(5), 0).unwrap() = pat(vec![a, skipped, b]);
        let mut e = Engine::from_project(SR, &p);
        e.play();
        render(&mut e, 0.3);
        assert_eq!(e.fm().sounding(), vec![64]);
        let mut edit = PatternData::Notes(pat(vec![skipped, b]));
        assert!(e.swap_pattern(MachineUid(5), 0, &mut edit));
        render(&mut e, 0.3);
        assert_eq!(e.fm().sounding(), vec![64], "B still rings");
    }

    #[test]
    fn chance_renders_the_same_every_time_and_follows_the_seed() {
        let plain = crate::demo::demo_project();
        let mut p = plain.clone();
        let uids: Vec<_> = p.machines.iter().map(|m| m.uid).collect();
        for uid in uids {
            if let Some(roll) = p.roll_mut(uid, 0) {
                for n in &mut roll.notes {
                    n.chance = 50;
                }
            }
        }
        let go = |p: &crate::project::Project| {
            let mut e = Engine::from_project(SR, p);
            assert_eq!(e.seed(), p.seed);
            e.play();
            render(&mut e, 2.0)
        };
        let a = go(&p);
        assert_eq!(a, go(&p), "same project, same render");
        assert_ne!(a, go(&plain), "chance 50 drops notes");
        p.seed = 7;
        assert_ne!(a, go(&p), "the seed picks which");
    }
}
