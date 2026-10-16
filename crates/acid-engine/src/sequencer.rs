//! Pattern-mode sequencer. Sample-accurate: the engine asks how many
//! samples until the next tick, renders that many, then calls tick().
//! Each 16th step has four ticks, and every track (acid, drum and the
//! poly note tracks) runs on every tick: its notes start on the tick
//! they are written at. An acid gate closes 2 ticks before its note's
//! written end (at least 1 tick after it starts), unless the note slides
//! into one starting at its end, which then plays legato: the 303's
//! half-step gate on a one-step note. The param lane sends on each
//! step's first tick. Tracks are by rack position; a track's kind comes
//! from the bank at its position. In Song mode each track follows its own
//! machine's lane of blocks: a block's start restarts its pattern, and
//! gaps and muted blocks are silent. A note's chance and condition are
//! decided once, at its start tick (see `trig`), against the track's loop
//! count; a note that plays with a ratchet then sounds each sub-hit.

use crate::machine::MAX_MACHINES;
use crate::params::{DRUM_INSTRUMENTS, ParamId};
use crate::project::{Block, DrumPattern, Note, NotePattern, SynthPattern, TICKS_PER_STEP};
use crate::rack::{CompiledSong, RackMap};
use crate::song::{
    Bank, Banks, compiled_block_at, compiled_block_end, compiled_song_end, value_at,
};
use crate::trig::{self, TrackTrig};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Pattern,
    Song,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SeqEvent {
    Note {
        slot: usize,
        midi: u8,
        accent: bool,
        legato: bool,
    },
    NoteOff {
        slot: usize,
    },
    Drum {
        slot: usize,
        inst: usize,
        accent: bool,
    },
    Param {
        id: ParamId,
        value: f32,
    },
    ParamRestore {
        id: ParamId,
    },
    /// A recorded song-automation value for this step.
    Automation {
        id: ParamId,
        value: f32,
    },
    /// The song position jumped to before this lane's first point: the
    /// param goes back to its knob value.
    AutomationReset {
        id: ParamId,
    },
    /// A note machine (FM or Sampler) starts a note.
    PolyOn {
        slot: usize,
        pitch: u8,
        vel: u8,
    },
    PolyOff {
        slot: usize,
        pitch: u8,
    },
}

/// Sequencer ticks per step: the step's quarters.
const SUBS: u8 = 4;
// Note tracks advance one tick per sequencer tick.
const _: () = assert!(TICKS_PER_STEP == SUBS as u16);

/// Ratcheted notes a track follows at once (more play their first hit only).
const RATCHETS: usize = 16;

#[derive(Clone, Copy, Debug, Default)]
struct Track {
    step: usize,
    pattern: u8,
    queued: Option<u8>,
    /// Acid: the gate is open (a note sounds).
    gate_open: bool,
    /// Acid: the sounding note slides into a note starting at its end.
    prev_slide: bool,
    /// Acid: ticks until the sounding note's gate-off check (0 = none).
    gate_left: u16,
    /// Acid: ticks from that check to the note's end, where a held gate
    /// must find the next note.
    hold_dist: u16,
    /// Acid: ticks until a held gate must find a note (0 = none).
    hold_left: u16,
    /// Acid: the sounding note's end tick, clipped to the pattern and
    /// wrapped (0 when it ends at the pattern's end).
    cur_end: u16,
    /// Param whose lane value is overriding the knob on the current step.
    /// Each synth track keeps its own: if both lane the same shared param
    /// (only possible from a hand-made file), the later track's write wins
    /// within a step, and its restore can undo the earlier track's value.
    lane_override: Option<ParamId>,
    /// The machine plays nothing now (a gap, a muted block, or a block
    /// on a missing pattern).
    silent: bool,
    /// The block's song transpose.
    transpose_add: i8,
    /// Song mode: the block playing, by index into the track's lane.
    block: Option<usize>,
    /// Song mode: that block's first step (kept through a gap after it).
    block_start: u32,
    /// Song mode: that block's end step, as last read.
    block_end: u32,
    /// Tick position in the pattern.
    pos: u16,
    /// Note tracks: pitches sounding now, one bit per MIDI note.
    held: u128,
    /// Chance and conditions: the loop count and the last conditional result.
    trig: TrackTrig,
    /// Notes that played with a ratchet and have sub-hits to come, by
    /// index in the pattern's notes (the first `rat_n`).
    rat: [u16; RATCHETS],
    rat_n: u8,
}

impl Track {
    /// Back to the first pass: loop count 0, no previous result, no
    /// pending sub-hits.
    fn reset_trig(&mut self) {
        self.trig = TrackTrig::default();
        self.rat_n = 0;
    }

    /// Follows the sub-hits of note `i` (it just played with a ratchet).
    fn push_ratchet(&mut self, i: usize) {
        if let Ok(i) = u16::try_from(i)
            && (self.rat_n as usize) < RATCHETS
        {
            self.rat[self.rat_n as usize] = i;
            self.rat_n += 1;
        }
    }

    fn ratcheting(&self, i: usize) -> bool {
        u16::try_from(i).is_ok_and(|i| self.rat[..self.rat_n as usize].contains(&i))
    }

    /// Drops the notes with no sub-hit after tick `p`.
    fn retire_ratchets(&mut self, notes: &[Note], p: u16) {
        let mut k = 0;
        for j in 0..self.rat_n as usize {
            let i = self.rat[j];
            if notes.get(i as usize).is_some_and(|n| !ratchet_at(n, p).1) {
                self.rat[k] = i;
                k += 1;
            }
        }
        self.rat_n = k as u8;
    }
}

/// Where tick `p` falls among ratcheted note `n`'s sub-hits: whether one
/// after its first starts at `p` (sub-hits collapsed onto one tick count
/// once), and whether none starts after `p`.
fn ratchet_at(n: &Note, p: u16) -> (bool, bool) {
    let (mut fires, mut last) = (false, n.start);
    for (s, _) in trig::sub_hits(n) {
        fires |= s == p && s != n.start;
        last = s;
    }
    (fires, p >= last)
}

pub struct Sequencer {
    sample_rate: f32,
    bpm: f32,
    swing: f32,
    playing: bool,
    global_step: u64,
    /// The quarter of the current step (0 = step start, 2 = mid-step).
    sub: u8,
    samples_to_next: f64,
    /// By rack position.
    tracks: [Track; MAX_MACHINES],
    mode: Mode,
    /// Song mode: the step Play starts from.
    song_start: u32,
    /// Song mode: the global step playing (it advances on quarter 2).
    song_step: u32,
    /// Play: the next step start finds every track's block.
    seek: bool,
    /// The next step emits every lane's held value, not just its points.
    auto_sync: bool,
    /// The song changed while playing: the next step start finds every
    /// track's block afresh.
    resync_pending: bool,
    /// Song mode: the song step was reached by playing on from the one
    /// before (not by Play), so reaching the loop end crosses it.
    advanced: bool,
    /// The arp clock of the tick last run, per rack position.
    clocks: [(u64, f64); MAX_MACHINES],
    /// Fill is held: `Fill` notes play, `NotFill` ones don't.
    fill: bool,
    /// The project's chance seed.
    seed: u32,
    /// The pattern last selected per machine; restored when song
    /// playback (which overrides the tracks' patterns) is left.
    chosen: [u8; MAX_MACHINES],
    ended: bool,
}

fn transposed(pitch: u8, add: i8) -> u8 {
    (pitch as i16 + add as i16).clamp(0, 127) as u8
}

/// The note of `roll` that starts at tick `pos`, with its index (the
/// first stored: acid patterns are kept monophonic by the editor).
fn note_starting(roll: &NotePattern, pos: u16) -> Option<(usize, &Note)> {
    roll.notes
        .iter()
        .enumerate()
        .find(|(_, n)| n.start == pos && pos < roll.ticks())
}

/// Advances a step-free track one tick; at the pattern's end it wraps,
/// counting a loop, or takes a queued pattern, starting its count afresh.
fn advance(t: &mut Track, ticks: u16) {
    t.step = (t.pos / TICKS_PER_STEP) as usize;
    t.pos += 1;
    if t.pos >= ticks {
        t.pos = 0;
        t.rat_n = 0;
        if let Some(q) = t.queued.take() {
            t.pattern = q;
            t.reset_trig();
        } else {
            t.trig.loops = t.trig.loops.wrapping_add(1);
        }
    }
}

/// A note cut by a block line: the hold check closes the gate on the
/// line's first tick unless a note starts there (the bar-line cut of v4).
fn cut_gate(t: &mut Track) {
    if t.gate_open && (t.gate_left > 0 || t.hold_left > 0) {
        if t.gate_left > 0 {
            t.prev_slide = false;
        }
        t.gate_left = 0;
        t.hold_left = 1;
    }
}

/// Takes block `i` of the lane at `pos` as the track's: its pattern (a
/// pattern past the bank is silent), mute, transpose and span.
fn take_block(t: &mut Track, pos: usize, i: usize, b: &Block, banks: Banks<'_>) {
    if (b.pattern.0 as usize) < banks.bank_len(pos) {
        t.pattern = b.pattern.0;
        t.silent = b.mute;
    } else {
        t.silent = true;
    }
    t.transpose_add = if banks.transposes(pos) {
        b.transpose
    } else {
        0
    };
    t.block = Some(i);
    t.block_start = b.start;
    t.block_end = compiled_block_end(b, pos, banks);
    t.reset_trig();
}

/// Note-offs for every pitch the track holds.
fn release_all(t: &mut Track, slot: usize, out: &mut Vec<SeqEvent>) {
    while t.held != 0 {
        let pitch = t.held.trailing_zeros() as u8;
        t.held &= !(1u128 << pitch);
        out.push(SeqEvent::PolyOff { slot, pitch });
    }
}

impl Sequencer {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            bpm: 125.0,
            swing: 0.0,
            playing: false,
            global_step: 0,
            sub: 0,
            samples_to_next: 0.0,
            tracks: [Track::default(); MAX_MACHINES],
            mode: Mode::Pattern,
            song_start: 0,
            song_step: 0,
            seek: false,
            auto_sync: false,
            resync_pending: false,
            advanced: false,
            clocks: [(0, 0.0); MAX_MACHINES],
            fill: false,
            seed: 0,
            chosen: [0; MAX_MACHINES],
            ended: false,
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        if bpm.is_finite() {
            self.bpm = bpm.clamp(40.0, 300.0);
        }
    }

    /// Test hook: makes the tick clock NaN, as a broken tempo would.
    #[cfg(test)]
    pub(crate) fn poison_clock(&mut self) {
        self.samples_to_next = f64::NAN;
    }

    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    /// Fill on (held or latched) or off. Real-time safe.
    pub fn set_fill(&mut self, fill: bool) {
        self.fill = fill;
    }

    pub fn fill(&self) -> bool {
        self.fill
    }

    /// The seed the chance rolls hash (the project's).
    pub fn set_seed(&mut self, seed: u32) {
        self.seed = seed;
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    #[cfg(test)]
    pub(crate) fn swing(&self) -> f32 {
        self.swing
    }

    pub fn set_swing(&mut self, swing: f32) {
        if swing.is_finite() {
            self.swing = swing.clamp(0.0, 0.75);
        }
    }

    fn step_len(&self, step: u64) -> f64 {
        let base = 60.0 / self.bpm as f64 / 4.0 * self.sample_rate as f64;
        let s = self.swing as f64 * 0.5;
        if step.is_multiple_of(2) {
            base * (1.0 + s)
        } else {
            base * (1.0 - s)
        }
    }

    pub fn play(&mut self) {
        self.playing = true;
        self.global_step = 0;
        self.sub = 0;
        self.samples_to_next = 0.0;
        self.ended = false;
        self.advanced = false;
        if self.mode == Mode::Song {
            self.song_step = self.song_start;
            self.seek = true;
        }
        for t in &mut self.tracks {
            t.step = 0;
            t.gate_open = false;
            t.prev_slide = false;
            t.gate_left = 0;
            t.hold_left = 0;
            t.lane_override = None;
            t.silent = false;
            t.transpose_add = 0;
            t.block = None;
            t.block_start = 0;
            t.block_end = 0;
            t.pos = 0;
            t.held = 0;
            t.reset_trig();
            if let Some(q) = t.queued.take() {
                t.pattern = q;
            }
        }
        self.resync_pending = false;
    }

    pub fn stop(&mut self) {
        self.playing = false;
        self.resync_pending = false;
        for t in &mut self.tracks {
            t.step = 0;
            t.gate_open = false;
            t.prev_slide = false;
            t.gate_left = 0;
            t.hold_left = 0;
            t.lane_override = None;
            t.silent = false;
            t.transpose_add = 0;
            t.block = None;
            t.block_start = 0;
            t.block_end = 0;
            t.pos = 0;
            t.held = 0;
            t.reset_trig();
        }
        if self.mode == Mode::Song {
            self.restore_chosen();
        }
    }

    /// Puts each machine back on its last selected pattern.
    fn restore_chosen(&mut self) {
        for (t, &slot) in self.tracks.iter_mut().zip(&self.chosen) {
            t.pattern = slot;
            t.queued = None;
        }
    }

    /// The song was edited while playing: every track finds its block
    /// afresh at the next step start. Real-time safe.
    pub fn resync(&mut self) {
        if self.playing && self.mode == Mode::Song {
            self.resync_pending = true;
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Ignored while playing (the UI stops before switching).
    pub fn set_mode(&mut self, mode: Mode) {
        if !self.playing && self.mode != mode {
            self.mode = mode;
            self.restore_chosen();
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The step Song-mode Play starts from.
    pub fn set_song_start(&mut self, step: u32) {
        self.song_start = step;
    }

    /// The 16-step bar of the song position (display only).
    pub fn song_bar(&self) -> usize {
        (self.song_step / 16) as usize
    }

    pub fn song_step(&self) -> u32 {
        self.song_step
    }

    /// True once after a song played past its end.
    pub fn take_ended(&mut self) -> bool {
        std::mem::take(&mut self.ended)
    }

    /// Song mode, each step's first tick: a pending seek or resync finds
    /// every track's block, the loop jumps back (when playback reaches its
    /// end, so a start past it plays on), the song ends (releasing
    /// what sounds; returns false), then each track starts the block
    /// starting now or goes silent at its block's end, then automation.
    fn song_step_start(
        &mut self,
        song: &CompiledSong,
        banks: Banks<'_>,
        out: &mut Vec<SeqEvent>,
    ) -> bool {
        if self.seek || self.resync_pending {
            self.seek = false;
            self.resync_pending = false;
            self.seek_all(song, banks, false);
            self.auto_sync = true;
        }
        // Only crossing the (exclusive) loop end loops: a start at or
        // past it plays on.
        let advanced = std::mem::take(&mut self.advanced);
        if let Some((a, b)) = song.loop_range
            && a < b
            && advanced
            && self.song_step == b
        {
            self.song_step = a;
            self.seek_all(song, banks, true);
            self.auto_sync = true;
        }
        if self.song_step >= compiled_song_end(song, banks) {
            for pos in 0..MAX_MACHINES {
                if matches!(banks.slots[pos], Some(Bank::Synth(_))) && self.tracks[pos].gate_open {
                    out.push(SeqEvent::NoteOff { slot: pos });
                }
            }
            for pos in 0..MAX_MACHINES {
                if matches!(banks.slots[pos], Some(Bank::Notes(_))) {
                    release_all(&mut self.tracks[pos], pos, out);
                }
            }
            self.stop();
            self.ended = true;
            return false;
        }
        for pos in 0..MAX_MACHINES {
            self.follow_blocks(pos, song, banks);
        }
        // Before the pattern lanes, so a lane value wins on its step.
        self.emit_automation(song, out);
        true
    }

    /// The track at `pos` on a step start: a block starting now restarts
    /// it on the block's pattern; else the end of its block (read live,
    /// so a pattern edited while playing moves it) silences it. Both cut
    /// an open acid gate as a bar line did.
    fn follow_blocks(&mut self, pos: usize, song: &CompiledSong, banks: Banks<'_>) {
        let lane = song.lane(pos);
        let step = self.song_step;
        let t = &mut self.tracks[pos];
        if let Ok(i) = lane.binary_search_by_key(&step, |b| b.start) {
            t.step = 0;
            t.pos = 0;
            cut_gate(t);
            take_block(t, pos, i, &lane[i], banks);
        } else if let Some(i) = t.block {
            t.block_end = lane.get(i).map_or(0, |b| compiled_block_end(b, pos, banks));
            if step >= t.block_end {
                t.block = None;
                t.silent = true;
                t.step = 0;
                t.pos = 0;
                t.reset_trig();
                cut_gate(t);
            }
        }
    }

    /// Every track onto its block at the song position (a step start);
    /// `cut` closes open gates as a block line does (the loop jump).
    fn seek_all(&mut self, song: &CompiledSong, banks: Banks<'_>, cut: bool) {
        let next = self.song_step as u64 * SUBS as u64;
        for pos in 0..MAX_MACHINES {
            if cut {
                cut_gate(&mut self.tracks[pos]);
            }
            self.seek_track(pos, next, song, banks);
        }
    }

    /// Puts the track at `pos` where it would be at song tick `next` (the
    /// next tick to run, 4 per step): on the block covering it, at that
    /// tick's place in its pattern (looping inside a longer block), or
    /// silent in a gap.
    fn seek_track(&mut self, pos: usize, next: u64, song: &CompiledSong, banks: Banks<'_>) {
        let step = (next / SUBS as u64).min(u32::MAX as u64) as u32;
        let t = &mut self.tracks[pos];
        let Some(i) = compiled_block_at(song, pos, step, banks) else {
            t.block = None;
            t.silent = true;
            t.step = 0;
            t.pos = 0;
            t.reset_trig();
            return;
        };
        let b = song.lane(pos)[i];
        take_block(t, pos, i, &b, banks);
        let len = banks.length(pos, t.pattern).unwrap_or(1) as u64;
        let ticks = len * TICKS_PER_STEP as u64;
        let rel = next - b.start as u64 * SUBS as u64;
        t.pos = (rel % ticks) as u16;
        // The passes straight playback from the block start would count.
        t.trig.loops = (rel / ticks).min(u32::MAX as u64) as u32;
        // As `advance` leaves it: the step of the last tick run.
        let last = rel.checked_sub(1).map_or(0, |k| k % ticks);
        t.step = (last / TICKS_PER_STEP as u64) as usize;
    }

    /// The song tick about to run. The step counter advances on quarter
    /// 2, so on quarter 3 it is one step ahead of the ticks.
    fn next_song_tick(&self) -> Option<u64> {
        let at = self.song_step as u64 * SUBS as u64;
        if self.sub == 3 {
            at.checked_sub(1)
        } else {
            Some(at + self.sub as u64)
        }
    }

    /// Song automation for the current step. On a sync step every lane
    /// reports its held value, and lanes with no point yet reset.
    fn emit_automation(&mut self, song: &CompiledSong, out: &mut Vec<SeqEvent>) {
        for lane in &song.automation {
            if self.auto_sync {
                out.push(match value_at(lane, self.song_step) {
                    Some(value) => SeqEvent::Automation {
                        id: lane.param,
                        value,
                    },
                    None => SeqEvent::AutomationReset { id: lane.param },
                });
            } else if let Some(&value) = lane.points.get(&self.song_step) {
                out.push(SeqEvent::Automation {
                    id: lane.param,
                    value,
                });
            }
        }
        self.auto_sync = false;
    }

    /// Rack edit (real-time safe): track `p` takes the state of old track
    /// `map[p]`, so surviving machines keep their position, pattern, queue
    /// and held notes. New machines (positions `banks` fills that `map`
    /// doesn't) get a fresh track on pattern `selected[p]`, in phase with
    /// the others (see `join`); positions past the rack are cleared.
    /// `song` is the song for the new rack.
    pub fn rearrange(
        &mut self,
        map: &RackMap,
        banks: Banks<'_>,
        song: &CompiledSong,
        selected: &[u8; MAX_MACHINES],
    ) {
        let (old_tracks, old_chosen) = (self.tracks, self.chosen);
        for pos in 0..MAX_MACHINES {
            let in_rack = banks.slots[pos].is_some();
            let old = map[pos].filter(|&o| o < MAX_MACHINES && in_rack);
            (self.tracks[pos], self.chosen[pos]) = match old {
                Some(o) => (old_tracks[o], old_chosen[o]),
                None if in_rack => {
                    let t = Track {
                        pattern: selected[pos],
                        ..Track::default()
                    };
                    (t, selected[pos])
                }
                None => (Track::default(), 0),
            };
            if old.is_none() && in_rack {
                self.join(pos, banks, song);
            }
        }
    }

    /// Puts the fresh track at `pos` where it would be had it played from
    /// the start. Pattern mode: at the global step (tick, for note tracks)
    /// modulo its pattern's length, as the tracks that never switched are.
    /// Song mode: on its block at the song position (see `seek_track`).
    /// Stopped: nothing to do. A step whose start tick has passed sounds
    /// from the next step on.
    fn join(&mut self, pos: usize, banks: Banks<'_>, song: &CompiledSong) {
        if !self.playing {
            return;
        }
        if self.mode == Mode::Song {
            match self.next_song_tick() {
                Some(next) => self.seek_track(pos, next, song, banks),
                None => self.tracks[pos].silent = true,
            }
            return;
        }
        let next_tick = self.global_step * SUBS as u64 + self.sub as u64;
        let t = &mut self.tracks[pos];
        let len = banks.length(pos, t.pattern).unwrap_or(1) as u64;
        let ticks = len * TICKS_PER_STEP as u64;
        t.pos = (next_tick % ticks) as u16;
        t.reset_trig();
        t.trig.loops = (next_tick / ticks).min(u32::MAX as u64) as u32;
        // As `advance` leaves it: the step of the last tick run.
        let last = next_tick.checked_sub(1).map_or(0, |k| k % ticks);
        t.step = (last / TICKS_PER_STEP as u64) as usize;
    }

    /// Selects pattern `slot` for the track at rack position `pos`.
    pub fn select(&mut self, pos: usize, slot: u8) {
        self.chosen[pos] = slot;
        if self.mode == Mode::Song && self.playing {
            return;
        }
        let t = &mut self.tracks[pos];
        if self.playing {
            t.queued = Some(slot);
        } else {
            t.pattern = slot;
            t.queued = None;
        }
    }

    pub fn current_pattern(&self, pos: usize) -> u8 {
        self.tracks[pos].pattern
    }

    pub fn queued_pattern(&self, pos: usize) -> Option<u8> {
        self.tracks[pos].queued
    }

    pub fn playhead(&self, pos: usize) -> usize {
        self.tracks[pos].step
    }

    pub fn samples_to_next_tick(&self) -> usize {
        if !self.playing {
            return usize::MAX;
        }
        self.samples_to_next.max(0.0).ceil() as usize
    }

    /// The arp clock of the tick last run, per rack position: the tick at
    /// 4 per step (counted from Play in Pattern mode; in Song mode from
    /// the start of that position's block, so the arp's grid restarts on
    /// every block) and its length in samples, swing included. Read after
    /// `tick`: a block starting on that tick is already counted.
    pub fn tick_clocks(&self) -> [(u64, f64); MAX_MACHINES] {
        self.clocks
    }

    fn set_clocks(&mut self, quarter: f64) {
        if self.mode == Mode::Song {
            let next = self.next_song_tick().unwrap_or(0);
            for (c, t) in self.clocks.iter_mut().zip(&self.tracks) {
                *c = (
                    next.saturating_sub(t.block_start as u64 * SUBS as u64),
                    quarter,
                );
            }
        } else {
            let tick = self.global_step * SUBS as u64 + self.sub as u64;
            self.clocks = [(tick, quarter); MAX_MACHINES];
        }
    }

    pub fn advance(&mut self, n: usize) {
        if self.playing {
            self.samples_to_next -= n as f64;
        }
    }

    pub fn tick(&mut self, banks: Banks<'_>, song: &CompiledSong, out: &mut Vec<SeqEvent>) {
        if !self.playing {
            return;
        }
        let quarter = self.step_len(self.global_step) * 0.25;
        if self.sub == 0 && self.mode == Mode::Song && !self.song_step_start(song, banks, out) {
            // The song ended on this tick: its clock is the first of none.
            self.clocks = [(0, quarter); MAX_MACHINES];
            return;
        }
        self.set_clocks(quarter);
        for pos in 0..MAX_MACHINES {
            if let Some(Bank::Synth(bank)) = banks.slots[pos] {
                self.synth_tick(pos, bank, out);
            }
        }
        for pos in 0..MAX_MACHINES {
            if let Some(Bank::Drum(bank)) = banks.slots[pos] {
                self.drum_tick(pos, bank, out);
            }
        }
        if self.sub == 2 && self.mode == Mode::Song {
            self.song_step = self.song_step.saturating_add(1);
            self.advanced = true;
        }
        self.note_ticks(banks, out);
        self.sub = (self.sub + 1) % SUBS;
        if self.sub == 0 {
            self.global_step += 1;
        }
        self.samples_to_next += quarter;
    }

    /// One tick of the acid track at rack position `pos`: the gate-off
    /// check two ticks before a note's end (held when the note slides into
    /// one starting at its end that will play), a held gate's end, a note
    /// start (if it plays) or a ratchet sub-hit, and on a step's first
    /// tick the param lane.
    fn synth_tick(&mut self, pos: usize, bank: &[SynthPattern], out: &mut Vec<SeqEvent>) {
        let (seed, fill) = (self.seed, self.fill);
        let t = &mut self.tracks[pos];
        let Some(pat) = bank.get(t.pattern as usize) else {
            return;
        };
        let roll = &pat.roll;
        let ticks = roll.ticks();
        if t.pos >= ticks {
            t.pos = 0;
            t.rat_n = 0;
        }
        let p = t.pos;
        if t.silent {
            if t.gate_open {
                out.push(SeqEvent::NoteOff { slot: pos });
                t.gate_open = false;
            }
            t.prev_slide = false;
            t.gate_left = 0;
            t.hold_left = 0;
            t.rat_n = 0;
            if let Some(id) = t.lane_override.take() {
                out.push(SeqEvent::ParamRestore { id });
            }
            advance(t, ticks);
            return;
        }
        let mut check_hold = false;
        if t.gate_open {
            if t.gate_left > 0 {
                t.gate_left -= 1;
                if t.gate_left == 0 {
                    // Looks at this pattern's own note at the end (the 303
                    // way): a held gate then runs into whatever plays next.
                    if t.prev_slide && next_plays(roll, t, p, pos, seed, fill) {
                        t.hold_left = t.hold_dist;
                        check_hold = t.hold_left == 0;
                    } else {
                        out.push(SeqEvent::NoteOff { slot: pos });
                        t.gate_open = false;
                    }
                }
            } else if t.hold_left > 0 {
                t.hold_left -= 1;
                check_hold = t.hold_left == 0;
            }
        }
        // What sounds now: a note starting here that plays (decided once,
        // at its start; it may be legato), else a ratchet sub-hit of the
        // note playing (never legato). `last`: no sub-hit follows.
        let mut hit = None;
        if let Some((i, n)) = note_starting(roll, p)
            && trig::plays(n, i, pos, seed, fill, &mut t.trig)
        {
            t.rat_n = 0;
            if n.ratchet > 1 {
                t.push_ratchet(i);
            }
            hit = Some((n, true, ratchet_at(n, p).1));
        } else if t.rat_n > 0
            && let Some(n) = roll.notes.get(t.rat[0] as usize)
            && let (true, last) = ratchet_at(n, p)
        {
            hit = Some((n, false, last));
        }
        t.retire_ratchets(&roll.notes, p);
        if check_hold && hit.is_none() {
            out.push(SeqEvent::NoteOff { slot: pos });
            t.gate_open = false;
        }
        if let Some((n, may_legato, last)) = hit {
            let legato = may_legato && t.prev_slide && t.gate_open;
            out.push(SeqEvent::Note {
                slot: pos,
                midi: transposed(n.pitch, pat.transpose.saturating_add(t.transpose_add)),
                accent: n.accent,
                legato,
            });
            // Each hit closes 2 ticks before its own end; only the last
            // may slide, holding to the note's end.
            let sub = trig::sub_hits(n).next().map_or(1, |h| h.1);
            let end = n.start.saturating_add(n.len.max(1)).min(ticks);
            let span = p.saturating_add(sub).min(ticks) - p;
            let gate = span.saturating_sub(2).max(1);
            t.gate_open = true;
            t.prev_slide = last && n.slide;
            t.gate_left = gate;
            t.hold_dist = (end - p).saturating_sub(gate);
            t.hold_left = 0;
            // Wrapped: a note ending at the pattern's end slides into tick 0.
            t.cur_end = if end >= ticks { 0 } else { end };
        }
        if p.is_multiple_of(TICKS_PER_STEP) {
            let step = (p / TICKS_PER_STEP) as usize;
            let lane_value = pat.param_lane.as_ref().and_then(|lane| {
                lane.values
                    .get(step)
                    .copied()
                    .flatten()
                    .map(|v| (lane.param, v))
            });
            match lane_value {
                Some((id, value)) => {
                    if let Some(old) = t.lane_override
                        && old != id
                    {
                        out.push(SeqEvent::ParamRestore { id: old });
                    }
                    out.push(SeqEvent::Param { id, value });
                    t.lane_override = Some(id);
                }
                None => {
                    if let Some(id) = t.lane_override.take() {
                        out.push(SeqEvent::ParamRestore { id });
                    }
                }
            }
        }
        advance(t, ticks);
    }

    /// One tick of the drum track at rack position `pos`: every hit
    /// starting now, in instrument order (closed hat before open hat, so
    /// the choke works as before), each instrument at most once.
    fn drum_tick(&mut self, pos: usize, bank: &[DrumPattern], out: &mut Vec<SeqEvent>) {
        let (seed, fill) = (self.seed, self.fill);
        let t = &mut self.tracks[pos];
        let Some(pat) = bank.get(t.pattern as usize) else {
            return;
        };
        let roll = &pat.roll;
        let ticks = roll.ticks();
        if t.pos >= ticks {
            t.pos = 0;
            t.rat_n = 0;
        }
        if t.silent {
            t.rat_n = 0;
        } else {
            let p = t.pos;
            let (mut on, mut accent) = (0u8, 0u8);
            // Hits starting now that play, then the ratchet sub-hits due.
            for (i, n) in roll.notes.iter().enumerate() {
                let now = if n.start == p {
                    let ok = trig::plays(n, i, pos, seed, fill, &mut t.trig);
                    if ok && n.ratchet > 1 {
                        t.push_ratchet(i);
                    }
                    ok
                } else {
                    t.ratcheting(i) && ratchet_at(n, p).0
                };
                if now && (n.pitch as usize) < DRUM_INSTRUMENTS {
                    on |= 1 << n.pitch;
                    if n.accent {
                        accent |= 1 << n.pitch;
                    }
                }
            }
            t.retire_ratchets(&roll.notes, p);
            for inst in 0..DRUM_INSTRUMENTS {
                if on & (1 << inst) != 0 {
                    out.push(SeqEvent::Drum {
                        slot: pos,
                        inst,
                        accent: accent & (1 << inst) != 0,
                    });
                }
            }
        }
        advance(t, ticks);
    }

    /// One tick of each note track: releases, then starts, then advance.
    fn note_ticks(&mut self, banks: Banks<'_>, out: &mut Vec<SeqEvent>) {
        for m in 0..MAX_MACHINES {
            let Some(Bank::Notes(bank)) = banks.slots[m] else {
                continue;
            };
            let (seed, fill) = (self.seed, self.fill);
            let t = &mut self.tracks[m];
            if t.silent {
                release_all(t, m, out);
                t.rat_n = 0;
                continue;
            }
            let Some(pat) = bank.get(t.pattern as usize) else {
                continue;
            };
            let ticks = pat.ticks();
            if t.pos >= ticks {
                t.pos = 0;
                t.rat_n = 0;
            }
            let p = t.pos;
            if p == 0 {
                release_all(t, m, out);
            } else {
                // Release every held pitch no note sounds through this tick:
                // notes that end here, and notes deleted, moved or re-pitched
                // by an edit while they rang. A note starting here doesn't
                // count, so back-to-back notes retrigger (off, then on); nor
                // does one with a ratchet sub-hit here.
                let mut want = 0u128;
                for (i, n) in pat.notes.iter().enumerate() {
                    let end = n.start.saturating_add(n.len).min(ticks);
                    if n.start < p && p < end && !(t.ratcheting(i) && ratchet_at(n, p).0) {
                        want |= 1u128 << transposed(n.pitch, t.transpose_add);
                    }
                }
                let mut stale = t.held & !want;
                t.held &= want;
                while stale != 0 {
                    let pitch = stale.trailing_zeros() as u8;
                    stale &= !(1u128 << pitch);
                    out.push(SeqEvent::PolyOff { slot: m, pitch });
                }
            }
            for (i, n) in pat.notes.iter().enumerate() {
                let now = if n.start == p {
                    let ok = trig::plays(n, i, m, seed, fill, &mut t.trig);
                    if ok && n.ratchet > 1 {
                        t.push_ratchet(i);
                    }
                    ok
                } else {
                    t.ratcheting(i) && ratchet_at(n, p).0
                };
                if now {
                    let pitch = transposed(n.pitch, t.transpose_add);
                    t.held |= 1u128 << pitch;
                    out.push(SeqEvent::PolyOn {
                        slot: m,
                        pitch,
                        vel: n.vel,
                    });
                }
            }
            t.retire_ratchets(&pat.notes, p);
            advance(t, ticks);
        }
    }
}

/// Whether the note starting where the acid track's sounding note ends
/// will play, so a slide holds into it: decided on a copy of the track's
/// state as it will be then (a pass on, if that end wraps). The note's
/// own start decides again; should that differ, the gate closes there.
fn next_plays(roll: &NotePattern, t: &Track, p: u16, pos: usize, seed: u32, fill: bool) -> bool {
    note_starting(roll, t.cur_end).is_some_and(|(i, n)| {
        let mut tt = t.trig;
        if t.cur_end < p {
            if t.queued.is_some() {
                tt = TrackTrig::default();
            } else {
                tt.loops = tt.loops.wrapping_add(1);
            }
        }
        trig::plays(n, i, pos, seed, fill, &mut tt)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::{MachineUid, standard_rack};
    use crate::project::{DrumPattern, NotePattern, PatternRef, SynthPattern};
    use crate::steps::{ACID_VEL, DrumGrid, DrumHit, Gate, SynthGrid, SynthStep};

    /// Sets synth pattern steps the old way, through the grid view.
    fn set_steps(p: &mut SynthPattern, length: u8, steps: &[(usize, SynthStep)]) {
        let mut g = SynthGrid::from_roll(&p.roll);
        g.length = length;
        for &(i, s) in steps {
            g.steps[i] = s;
        }
        p.roll = g.to_roll();
    }

    fn set_hits(p: &mut DrumPattern, length: u8, hits: &[(usize, usize, DrumHit)]) {
        let mut g = DrumGrid::from_roll(&p.roll);
        g.length = length;
        for &(step, inst, h) in hits {
            g.steps[step][inst] = h;
        }
        p.roll = g.to_roll();
    }

    const SR: f32 = 44_100.0;

    #[test]
    fn non_finite_tempo_is_ignored() {
        let mut seq = Sequencer::new(SR);
        seq.set_bpm(140.0);
        seq.set_swing(0.3);
        seq.set_bpm(f32::NAN);
        seq.set_bpm(f32::INFINITY);
        seq.set_swing(f32::NAN);
        assert_eq!(seq.bpm(), 140.0);
        assert_eq!(seq.swing, 0.3);
    }

    struct Fixture {
        synths: [Vec<SynthPattern>; 2],
        drums: [Vec<DrumPattern>; 2],
        notes: [Vec<NotePattern>; 2],
        song: crate::project::Song,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                synths: [
                    vec![SynthPattern::default(); 4],
                    vec![SynthPattern::default(); 4],
                ],
                drums: [
                    vec![DrumPattern::default(); 4],
                    vec![DrumPattern::default(); 4],
                ],
                notes: [
                    vec![NotePattern::default(); 4],
                    vec![NotePattern::default(); 4],
                ],
                song: crate::project::Song::default(),
            }
        }

        /// A song of bars as v5 had them, as the v6 conversion gives it:
        /// `rows[i]` names the pattern of each of the first four machines
        /// in bar i, which lasts its longest pattern (16 steps if none).
        /// Each named pattern is one block at the bar's start, as long as
        /// the bar; the song lasts every bar, empty ones too.
        fn set_bars(&mut self, rows: &[[Option<u8>; 4]]) {
            let mut song = crate::project::Song::default();
            let banks = self.banks();
            let mut start = 0u32;
            for row in rows {
                let bar = (0..4)
                    .filter_map(|pos| banks.length(pos, row[pos]?))
                    .max()
                    .unwrap_or(16) as u32;
                for (pos, slot) in row.iter().enumerate() {
                    let Some(slot) = *slot else { continue };
                    let own = banks.length(pos, slot).map(|n| n as u32);
                    let block = Block {
                        len: (own != Some(bar)).then_some(bar as u16),
                        ..Block::new(start, PatternRef(slot))
                    };
                    song.lane_mut(MachineUid(pos as u16 + 1)).push(block);
                }
                start += bar;
            }
            song.end = (start > 0).then_some(start);
            self.song = song;
        }

        /// The block `i` of machine `uid`'s lane.
        fn block(&mut self, uid: u16, i: usize) -> &mut Block {
            &mut self.song.lane_mut(MachineUid(uid))[i]
        }

        /// The standard rack's six tracks at positions 0..6.
        fn banks(&self) -> Banks<'_> {
            let mut b = Banks::empty();
            for (pos, (uid, ty, _)) in standard_rack().into_iter().enumerate() {
                b.machines[pos] = Some((uid, ty));
                b.slots[pos] = Some(match pos {
                    0 | 1 => Bank::Synth(&self.synths[pos]),
                    2 | 3 => Bank::Drum(&self.drums[pos - 2]),
                    _ => Bank::Notes(&self.notes[pos - 4]),
                });
            }
            b
        }

        fn run(&self, seq: &mut Sequencer, total: usize) -> Vec<(usize, SeqEvent)> {
            let uids: Vec<_> = standard_rack().iter().map(|m| m.0).collect();
            let song = CompiledSong::compile(&self.song, &uids);
            let mut pos = 0;
            let mut buf = Vec::with_capacity(64);
            let mut out = Vec::new();
            while pos < total {
                let n = seq.samples_to_next_tick().min(total - pos);
                if n == 0 {
                    buf.clear();
                    seq.tick(self.banks(), &song, &mut buf);
                    out.extend(buf.iter().map(|e| (pos, *e)));
                    continue;
                }
                seq.advance(n);
                pos += n;
            }
            out
        }
    }

    fn on(note: u8) -> SynthStep {
        SynthStep {
            note,
            octave: 0,
            gate: Gate::On,
            accent: false,
            slide: false,
        }
    }

    fn new_seq() -> Sequencer {
        let mut s = Sequencer::new(SR);
        s.set_bpm(120.0); // 16th = 5512.5 samples
        s
    }

    #[test]
    fn stopped_sequencer_never_ticks() {
        let s = new_seq();
        assert_eq!(s.samples_to_next_tick(), usize::MAX);
    }

    #[test]
    fn four_on_the_floor_is_sample_accurate() {
        let mut f = Fixture::new();
        for st in [0, 4, 8, 12] {
            set_hits(&mut f.drums[0][0], 16, &[(st, 0, DrumHit::On)]);
        }
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 88_200);
        let kicks: Vec<usize> = ev
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e,
                    SeqEvent::Drum {
                        slot: 2,
                        inst: 0,
                        ..
                    }
                )
            })
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(kicks, vec![0, 22_050, 44_100, 66_150]);
    }

    #[test]
    fn odd_steps_land_on_the_next_whole_sample() {
        let mut f = Fixture::new();
        set_hits(&mut f.drums[1][0], 16, &[(1, 6, DrumHit::Accent)]);
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 11_000);
        assert_eq!(
            ev,
            vec![(
                5513,
                SeqEvent::Drum {
                    slot: 3,
                    inst: 6,
                    accent: true
                }
            )]
        );
    }

    #[test]
    fn swing_delays_odd_steps() {
        let mut f = Fixture::new();
        set_hits(&mut f.drums[0][0], 16, &[(1, 0, DrumHit::On)]);
        set_hits(&mut f.drums[0][0], 16, &[(2, 0, DrumHit::On)]);
        let mut s = new_seq();
        s.set_swing(0.5);
        s.play();
        let ev = f.run(&mut s, 12_000);
        let pos: Vec<usize> = ev.iter().map(|(p, _)| *p).collect();
        assert_eq!(pos, vec![6891, 11_025]); // 5512.5 * 1.25 = 6890.6
    }

    #[test]
    fn plain_note_releases_at_half_step() {
        let mut f = Fixture::new();
        set_steps(&mut f.synths[0][0], 16, &[(0, on(0))]);
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 5000);
        assert_eq!(
            ev,
            vec![
                (
                    0,
                    SeqEvent::Note {
                        slot: 0,
                        midi: 36,
                        accent: false,
                        legato: false
                    }
                ),
                (2757, SeqEvent::NoteOff { slot: 0 }),
            ]
        );
    }

    #[test]
    fn slide_holds_gate_and_marks_next_note_legato() {
        let mut f = Fixture::new();
        set_steps(
            &mut f.synths[1][0],
            16,
            &[(
                0,
                SynthStep {
                    slide: true,
                    ..on(0)
                },
            )],
        );
        set_steps(&mut f.synths[1][0], 16, &[(1, on(7))]);
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 11_000);
        assert_eq!(
            ev,
            vec![
                (
                    0,
                    SeqEvent::Note {
                        slot: 1,
                        midi: 36,
                        accent: false,
                        legato: false
                    }
                ),
                (
                    5513,
                    SeqEvent::Note {
                        slot: 1,
                        midi: 43,
                        accent: false,
                        legato: true
                    }
                ),
                (8269, SeqEvent::NoteOff { slot: 1 }),
            ]
        );
    }

    #[test]
    fn tie_extends_without_retrigger() {
        let mut f = Fixture::new();
        set_steps(&mut f.synths[0][0], 16, &[(0, on(5))]);
        set_steps(
            &mut f.synths[0][0],
            16,
            &[(
                1,
                SynthStep {
                    gate: Gate::Tie,
                    ..on(9)
                },
            )],
        );
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 11_000);
        assert_eq!(
            ev,
            vec![
                (
                    0,
                    SeqEvent::Note {
                        slot: 0,
                        midi: 41,
                        accent: false,
                        legato: false
                    }
                ),
                (8269, SeqEvent::NoteOff { slot: 0 }),
            ]
        );
    }

    #[test]
    fn slide_into_rest_releases_at_half_step() {
        let mut f = Fixture::new();
        set_steps(
            &mut f.synths[0][0],
            16,
            &[(
                0,
                SynthStep {
                    slide: true,
                    ..on(0)
                },
            )],
        );
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 5000);
        assert_eq!(ev[1], (2757, SeqEvent::NoteOff { slot: 0 }));
    }

    #[test]
    fn accent_and_transpose_pass_through() {
        let mut f = Fixture::new();
        set_steps(
            &mut f.synths[0][0],
            16,
            &[(
                0,
                SynthStep {
                    accent: true,
                    octave: 1,
                    ..on(2)
                },
            )],
        );
        f.synths[0][0].transpose = -3;
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 10);
        assert_eq!(
            ev[0].1,
            SeqEvent::Note {
                slot: 0,
                midi: 36 + 12 + 2 - 3,
                accent: true,
                legato: false
            }
        );
    }

    #[test]
    fn pattern_length_wraps_and_polymeter() {
        let mut f = Fixture::new();
        set_steps(&mut f.synths[0][0], 3, &[(0, on(0))]);
        set_hits(&mut f.drums[0][0], 4, &[(0, 0, DrumHit::On)]);
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 12 * 5512); // steps 0..=11 (step 12 starts at 66 150)
        let notes = ev
            .iter()
            .filter(|(_, e)| matches!(e, SeqEvent::Note { .. }))
            .count();
        let kicks = ev
            .iter()
            .filter(|(_, e)| matches!(e, SeqEvent::Drum { .. }))
            .count();
        assert_eq!(notes, 4); // steps 0, 3, 6, 9
        assert_eq!(kicks, 3); // steps 0, 4, 8
    }

    #[test]
    fn selecting_while_playing_queues_until_wrap() {
        let mut f = Fixture::new();
        set_hits(&mut f.drums[0][1], 16, &[(0, 1, DrumHit::On)]); // pattern 1 has a snare on step 0
        let mut s = new_seq();
        s.play();
        f.run(&mut s, 3 * 5513);
        s.select(2, 1);
        assert_eq!(s.current_pattern(2), 0);
        assert_eq!(s.queued_pattern(2), Some(1));
        let ev = f.run(&mut s, 14 * 5513);
        let snare = ev
            .iter()
            .find(|(_, e)| matches!(e, SeqEvent::Drum { inst: 1, .. }));
        assert!(snare.is_some());
        assert_eq!(s.current_pattern(2), 1);
    }

    #[test]
    fn an_off_grid_acid_note_plays_at_its_tick_with_a_short_gate() {
        let mut f = Fixture::new();
        f.synths[0][0].roll.notes = vec![Note::new(5, 1, 48, ACID_VEL)];
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 4 * 5512);
        let quarter = 5512.5_f64 / 4.0;
        let on = ev
            .iter()
            .find(|(_, e)| matches!(e, SeqEvent::Note { .. }))
            .unwrap();
        let off = ev
            .iter()
            .find(|(_, e)| matches!(e, SeqEvent::NoteOff { .. }))
            .unwrap();
        assert_eq!(on.0, (5.0 * quarter).ceil() as usize);
        assert_eq!(off.0, (6.0 * quarter).ceil() as usize, "one tick later");
    }

    #[test]
    fn a_drum_hit_between_sixteenths_plays_on_its_tick() {
        let mut f = Fixture::new();
        f.drums[0][0].roll.notes = vec![Note::new(2, 4, 0, ACID_VEL)];
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 5512);
        let hit = ev
            .iter()
            .find(|(_, e)| matches!(e, SeqEvent::Drum { .. }))
            .unwrap();
        assert_eq!(hit.0, (2.0 * 5512.5 / 4.0_f64).ceil() as usize);
    }

    #[test]
    fn selecting_while_stopped_is_immediate() {
        let mut s = new_seq();
        s.select(0, 9);
        assert_eq!(s.current_pattern(0), 9);
        assert_eq!(s.queued_pattern(0), None);
    }

    #[test]
    fn param_lane_emits_param_events() {
        use crate::params::SynthParam;
        use crate::project::ParamLane;
        let mut f = Fixture::new();
        let id = MachineUid(1).param(SynthParam::Cutoff);
        let mut values = vec![None; 64];
        values[0] = Some(0.8);
        f.synths[0][0].param_lane = Some(ParamLane { param: id, values });
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 10);
        assert_eq!(ev, vec![(0, SeqEvent::Param { id, value: 0.8 })]);
    }

    #[test]
    fn stop_resets_playheads() {
        let f = Fixture::new();
        let mut s = new_seq();
        s.play();
        f.run(&mut s, 5 * 5513);
        assert_eq!(s.playhead(0), 5);
        s.stop();
        assert!(!s.is_playing());
        assert_eq!(s.playhead(0), 0);
    }

    #[test]
    fn lane_override_is_restored_on_the_next_step_without_a_value() {
        use crate::params::SynthParam;
        use crate::project::ParamLane;
        let mut f = Fixture::new();
        let id = MachineUid(1).param(SynthParam::Cutoff);
        let mut values = vec![None; 64];
        values[0] = Some(0.8);
        f.synths[0][0].param_lane = Some(ParamLane { param: id, values });
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 6000);
        assert_eq!(
            ev,
            vec![
                (0, SeqEvent::Param { id, value: 0.8 }),
                (5513, SeqEvent::ParamRestore { id })
            ]
        );
    }

    #[test]
    fn lane_param_change_restores_the_old_param() {
        use crate::params::SynthParam;
        use crate::project::ParamLane;
        let mut f = Fixture::new();
        let cutoff = MachineUid(1).param(SynthParam::Cutoff);
        let reso = MachineUid(1).param(SynthParam::Resonance);
        f.synths[0][0].param_lane = Some(ParamLane {
            param: cutoff,
            values: vec![Some(0.8); 64],
        });
        let mut values = vec![None; 64];
        values[0] = Some(0.3);
        f.synths[0][1].param_lane = Some(ParamLane {
            param: reso,
            values,
        });
        let mut s = new_seq();
        s.play();
        f.run(&mut s, 10);
        s.select(0, 1);
        let ev = f.run(&mut s, 16 * 5513);
        let restore = ev
            .iter()
            .position(|(_, e)| *e == SeqEvent::ParamRestore { id: cutoff })
            .expect("A's lane param is restored");
        let a_lane = SeqEvent::Param {
            id: cutoff,
            value: 0.8,
        };
        assert!(
            ev[..restore].iter().any(|(_, e)| *e == a_lane),
            "A's lane played before the switch"
        );
        // The restore comes at the switch to B: the start of the next bar
        // (positions restart at this run, 10 samples into the bar).
        let at = ev[restore].0 as f32;
        let step = SR * 60.0 / 120.0 / 4.0;
        assert!((at - (16.0 * step - 10.0)).abs() < step / 2.0, "at {at}");
        let boundary: Vec<SeqEvent> = ev
            .iter()
            .filter(|(pos, _)| *pos == ev[restore].0)
            .map(|(_, e)| *e)
            .collect();
        assert_eq!(
            boundary,
            vec![
                SeqEvent::ParamRestore { id: cutoff },
                SeqEvent::Param {
                    id: reso,
                    value: 0.3
                },
            ],
            "pattern B's lane replaces A's: the old param comes back first"
        );
        let restores = ev
            .iter()
            .filter(|(_, e)| matches!(e, SeqEvent::ParamRestore { .. }))
            .count();
        assert_eq!(restores, 1, "only A's param is restored, and only once");
        assert!(
            ev[restore..]
                .iter()
                .all(|(_, e)| !matches!(e, SeqEvent::Param { id, .. } if *id == cutoff)),
            "A's lane writes nothing after the switch"
        );
    }

    #[test]
    fn both_synths_laning_one_param_is_last_writer_wins() {
        use crate::params::{FxParam, ParamId};
        use crate::project::ParamLane;
        let mut f = Fixture::new();
        let id = ParamId::Fx(FxParam::DelayFeedback);
        f.synths[0][0].param_lane = Some(ParamLane {
            param: id,
            values: vec![Some(0.2); 64],
        });
        let mut values = vec![None; 64];
        values[0] = Some(0.9);
        f.synths[1][0].param_lane = Some(ParamLane { param: id, values });
        let mut s = new_seq();
        s.play();
        let ev = f.run(&mut s, 6000);
        let at =
            |p: usize| -> Vec<SeqEvent> { ev.iter().filter(|e| e.0 == p).map(|e| e.1).collect() };
        assert_eq!(
            at(0),
            vec![
                SeqEvent::Param { id, value: 0.2 },
                SeqEvent::Param { id, value: 0.9 },
            ],
            "step 0: synth 2's value is written last"
        );
        assert_eq!(
            at(5513),
            vec![
                SeqEvent::Param { id, value: 0.2 },
                SeqEvent::ParamRestore { id },
            ],
            "step 1: synth 2's restore comes after synth 1's value and wins"
        );
    }

    fn notes(ev: &[(usize, SeqEvent)]) -> Vec<(usize, u8)> {
        ev.iter()
            .filter_map(|(p, e)| match e {
                SeqEvent::Note { midi, .. } => Some((*p, *midi)),
                _ => None,
            })
            .collect()
    }

    fn song_seq() -> Sequencer {
        let mut s = new_seq();
        s.set_mode(Mode::Song);
        s
    }

    #[test]
    fn song_plays_bars_in_order() {
        let mut f = Fixture::new();
        set_steps(&mut f.synths[0][0], 16, &[(0, on(0))]);
        set_steps(&mut f.synths[0][1], 16, &[(0, on(7))]);
        f.set_bars(&[[Some(0), None, None, None], [Some(1), None, None, None]]);
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 33 * 5513);
        assert_eq!(notes(&ev), vec![(0, 36), (88_200, 43)]);
        assert!(!s.is_playing(), "stops after the last bar");
        assert!(s.take_ended());
        assert!(!s.take_ended(), "reported once");
    }

    #[test]
    fn a_block_longer_than_its_pattern_loops_it() {
        let mut f = Fixture::new();
        set_steps(&mut f.synths[0][0], 8, &[(0, on(0))]);
        f.drums[0][0].roll.length = 16;
        f.set_bars(&[[Some(0), None, Some(0), None]]);
        assert_eq!(f.block(1, 0).len, Some(16));
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 17 * 5513);
        assert_eq!(
            notes(&ev).len(),
            2,
            "8-step synth pattern plays twice in the 16-step bar"
        );
    }

    #[test]
    fn the_tick_clock_restarts_at_every_block() {
        let mut f = Fixture::new();
        f.synths[0][0].roll.length = 6;
        f.synths[0][1].roll.length = 16;
        f.set_bars(&[[Some(0), None, None, None], [Some(1), None, None, None]]);
        // The 808 has its own block from step 2: its own clock.
        f.song.lane_mut(MachineUid(3)).push(Block {
            len: Some(4),
            ..Block::new(2, PatternRef(0))
        });
        let uids: Vec<_> = standard_rack().iter().map(|m| m.0).collect();
        let song = CompiledSong::compile(&f.song, &uids);
        let mut s = song_seq();
        s.play();
        let (mut ticks, mut kit) = (Vec::new(), Vec::new());
        let mut buf = Vec::with_capacity(64);
        while s.is_playing() {
            if s.samples_to_next_tick() == 0 {
                buf.clear();
                s.tick(f.banks(), &song, &mut buf);
                ticks.push(s.tick_clocks()[0].0);
                kit.push(s.tick_clocks()[2].0);
            } else {
                s.advance(s.samples_to_next_tick());
            }
        }
        let bar = |steps: u64| (0..steps * SUBS as u64).collect::<Vec<_>>();
        let expected: Vec<u64> = [bar(6), bar(16)].concat();
        // The song-ending tick (the third bar's start) is clocked too.
        assert_eq!(ticks[..expected.len()], expected[..]);
        assert_eq!(kit[8..24], bar(4)[..], "from its block's start at step 2");
    }

    #[test]
    fn none_and_mute_silence_a_machine_and_transpose_adds() {
        let mut f = Fixture::new();
        set_steps(&mut f.synths[0][0], 16, &[(0, on(0))]);
        set_hits(&mut f.drums[0][0], 16, &[(0, 0, DrumHit::On)]);
        f.set_bars(&[[None, None, Some(0), None], [Some(0), None, Some(0), None]]);
        f.block(3, 0).mute = true;
        f.block(1, 0).transpose = 5;
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 32 * 5513);
        let drums = ev
            .iter()
            .filter(|(_, e)| matches!(e, SeqEvent::Drum { .. }))
            .count();
        assert_eq!(drums, 1, "muted in bar 1, playing in bar 2");
        assert_eq!(
            notes(&ev),
            vec![(88_200, 41)],
            "no synth in bar 1; bar 2 transposed +5"
        );
    }

    #[test]
    fn loop_range_repeats_and_resets_the_song_step() {
        let mut f = Fixture::new();
        f.set_bars(&[[None; 4]; 3]);
        f.song.loop_range = Some((16, 48)); // bars 1 and 2
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 48 * 5513 + 10);
        assert!(s.is_playing());
        assert_eq!(s.song_bar(), 1, "jumped back to the loop start");
        assert_eq!(s.song_step(), 16);
    }

    #[test]
    fn starting_past_the_loop_end_plays_on_to_the_song_end() {
        let mut f = Fixture::new();
        f.set_bars(&[[None; 4]; 3]);
        f.song.loop_range = Some((0, 16));
        let mut s = song_seq();
        s.set_song_start(32);
        s.play();
        f.run(&mut s, 8 * 5513);
        assert!(s.is_playing());
        assert_eq!(s.song_step(), 40, "no jump to the loop start");
        f.run(&mut s, 9 * 5513);
        assert!(!s.is_playing(), "it ends at the song end");
        assert!(s.take_ended());
    }

    #[test]
    fn a_loop_past_the_song_end_still_loops() {
        let mut f = Fixture::new();
        f.synths[0][0].roll.length = 12;
        f.song
            .lane_mut(MachineUid(1))
            .push(Block::new(0, PatternRef(0)));
        f.song.loop_range = Some((0, 16));
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 20 * 5513);
        assert!(s.is_playing(), "the loop extends the 12-step song");
        assert!(!s.take_ended());
        assert_eq!(s.song_step(), 4, "looped back at step 16");
    }

    #[test]
    fn a_two_bar_song_with_a_loop_over_bars_1_to_4_loops() {
        let mut f = Fixture::new();
        f.set_bars(&[[Some(0), None, None, None]; 2]);
        f.song.loop_range = Some((16, 80));
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 84 * 5513);
        assert!(s.is_playing());
        assert!(!s.take_ended());
        assert_eq!(s.song_step(), 20, "back to bar 1 after bar 4");
    }

    #[test]
    fn starting_exactly_at_the_loop_end_plays_on() {
        let mut f = Fixture::new();
        f.set_bars(&[[None; 4]; 3]);
        f.song.loop_range = Some((0, 16));
        let mut s = song_seq();
        s.set_song_start(16);
        s.play();
        f.run(&mut s, 4 * 5513);
        assert_eq!(s.song_step(), 20, "no jump back on the first step");
        // A resync on a later step start does not loop either.
        s.resync();
        f.run(&mut s, 5513);
        assert_eq!(s.song_step(), 21);
        f.run(&mut s, 28 * 5513);
        assert!(!s.is_playing(), "it plays on to the song end");
    }

    #[test]
    fn crossing_the_loop_end_with_a_resync_pending_still_loops() {
        let mut f = Fixture::new();
        f.set_bars(&[[None; 4]; 3]);
        f.song.loop_range = Some((0, 16));
        let mut s = song_seq();
        s.play();
        // Past the mid tick of step 15: song step 16, its start not yet run.
        f.run(&mut s, 15 * 5513 + 3000);
        assert_eq!(s.song_step(), 16);
        s.resync();
        f.run(&mut s, 3000);
        assert!(s.is_playing());
        assert_eq!(s.song_step(), 0, "looped back to the start");
    }

    #[test]
    fn a_track_seeked_into_a_gap_restarts_its_playhead() {
        let mut f = Fixture::new();
        f.set_bars(&[[Some(0), None, None, None]; 2]);
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 5 * 5513);
        assert_eq!(s.playhead(0), 5);
        f.song.lane_mut(MachineUid(1)).remove(0);
        s.resync();
        f.run(&mut s, 5513);
        assert_eq!(s.playhead(0), 0, "silent in the gap, at its start");
    }

    #[test]
    fn automation_emits_points_and_syncs_on_start() {
        use crate::params::{MasterParam, ParamId};
        let id = ParamId::Master(MasterParam::Level);
        let mut f = Fixture::new();
        f.set_bars(&[[None; 4]; 2]);
        crate::song::write_automation(&mut f.song, id, 3, 3, 0.25);
        crate::song::write_automation(&mut f.song, id, 16, 16, 0.75);
        let autos = |ev: &[(usize, SeqEvent)]| -> Vec<(usize, f32)> {
            ev.iter()
                .filter_map(|(p, e)| match e {
                    SeqEvent::Automation { value, .. } => Some((*p, *value)),
                    _ => None,
                })
                .collect()
        };
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 17 * 5513);
        assert_eq!(autos(&ev), vec![(16_538, 0.25), (88_200, 0.75)]);

        let mut s = song_seq();
        s.set_song_start(16);
        s.play();
        assert_eq!(s.song_bar(), 1);
        let ev = f.run(&mut s, 10);
        assert_eq!(
            autos(&ev),
            vec![(0, 0.75)],
            "starting mid-song applies the held value"
        );
        assert_eq!(s.song_step(), 16);
    }

    #[test]
    fn pattern_mode_ignores_the_song_and_mode_is_fixed_while_playing() {
        let mut f = Fixture::new();
        set_steps(&mut f.synths[0][0], 16, &[(0, on(0))]);
        f.set_bars(&[[None; 4]]);
        let mut s = new_seq();
        s.play();
        s.set_mode(Mode::Song);
        assert_eq!(s.mode(), Mode::Pattern);
        let ev = f.run(&mut s, 100);
        assert_eq!(notes(&ev), vec![(0, 36)]);
    }

    #[test]
    fn selection_during_song_playback_is_kept_for_pattern_mode() {
        let mut f = Fixture::new();
        for b in f.synths.iter_mut() {
            b.resize(8, SynthPattern::default());
        }
        for b in f.drums.iter_mut() {
            b.resize(8, DrumPattern::default());
        }
        set_steps(&mut f.synths[0][2], 16, &[(0, on(4))]);
        set_steps(&mut f.synths[0][5], 16, &[(0, on(9))]);
        f.set_bars(&[[Some(5); 4]; 2]);
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 3 * 5513);
        for m in 0..4 {
            s.select(m, 2);
            assert_eq!(s.current_pattern(m), 5, "ignored while the song plays");
            assert_eq!(s.queued_pattern(m), None);
        }
        s.stop();
        s.set_mode(Mode::Pattern);
        s.play();
        for m in 0..4 {
            assert_eq!(s.current_pattern(m), 2, "machine {m}");
        }
        let ev = f.run(&mut s, 100);
        assert_eq!(notes(&ev), vec![(0, 40)], "pattern 2 plays, not 5");
    }

    fn resets(ev: &[(usize, SeqEvent)]) -> Vec<usize> {
        ev.iter()
            .filter_map(|(p, e)| matches!(e, SeqEvent::AutomationReset { .. }).then_some(*p))
            .collect()
    }

    #[test]
    fn loop_jump_resets_lanes_without_a_point_yet() {
        use crate::params::{MasterParam, ParamId};
        let id = ParamId::Master(MasterParam::Level);
        let mut f = Fixture::new();
        f.set_bars(&[[None; 4]; 2]);
        f.song.loop_range = Some((0, 32));
        crate::song::write_automation(&mut f.song, id, 20, 20, 0.0);
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 33 * 5513);
        assert!(s.is_playing(), "looping");
        assert_eq!((s.song_bar(), s.song_step()), (0, 1), "back in bar 0");
        // Step 32 (the loop jump) starts at 32 × 5512.5 = 176 400.
        assert_eq!(
            resets(&ev),
            vec![0, 176_400],
            "at the start and the loop jump"
        );
        let reset_then_point: Vec<SeqEvent> = ev
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e,
                    SeqEvent::Automation { .. } | SeqEvent::AutomationReset { .. }
                )
            })
            .map(|(_, e)| *e)
            .collect();
        assert_eq!(
            reset_then_point,
            vec![
                SeqEvent::AutomationReset { id },
                SeqEvent::Automation { id, value: 0.0 },
                SeqEvent::AutomationReset { id },
            ]
        );
    }

    #[test]
    fn song_automation_comes_before_pattern_lanes_in_a_step() {
        use crate::params::{MasterParam, ParamId};
        use crate::project::ParamLane;
        let id = ParamId::Master(MasterParam::Level);
        let mut f = Fixture::new();
        let mut values = vec![None; 64];
        values[2] = Some(0.9);
        f.synths[0][0].param_lane = Some(ParamLane { param: id, values });
        f.set_bars(&[[Some(0), None, None, None]]);
        crate::song::write_automation(&mut f.song, id, 2, 2, 0.1);
        let mut s = song_seq();
        s.play();
        let ev = f.run(&mut s, 3 * 5513);
        let at_step_2: Vec<SeqEvent> = ev
            .iter()
            .filter(|(p, _)| *p == 11_025)
            .map(|(_, e)| *e)
            .collect();
        assert_eq!(
            at_step_2,
            vec![
                SeqEvent::Automation { id, value: 0.1 },
                SeqEvent::Param { id, value: 0.9 },
            ],
            "the pattern lane wins on its step"
        );
    }

    #[test]
    fn resync_mid_block_reseeks_the_tracks() {
        let mut f = Fixture::new();
        set_steps(&mut f.synths[0][1], 32, &[(20, on(5))]);
        f.set_bars(&[[Some(0), None, None, None]]);
        f.block(1, 0).len = Some(32);
        let mut s = song_seq();
        s.play();
        // Past the mid tick of step 18: song step 19.
        f.run(&mut s, 103_000);
        assert_eq!((s.song_bar(), s.song_step()), (1, 19));
        assert_eq!(s.current_pattern(0), 0);
        // The block now plays pattern 1, 32 steps long.
        f.block(1, 0).pattern = PatternRef(1);
        f.block(1, 0).len = None;
        s.resync();
        let ev = f.run(&mut s, 13_000); // through the step-21 start tick
        assert_eq!(s.song_step(), 21, "the position stays");
        assert_eq!(s.current_pattern(0), 1, "on the new pattern at once");
        assert_eq!(notes(&ev), vec![(110_250 - 103_000, 41)], "its step 20");
    }

    #[test]
    fn resync_is_ignored_unless_a_song_plays() {
        let mut s = new_seq();
        s.play();
        s.resync();
        assert!(!s.resync_pending);
        let mut s = song_seq();
        s.resync();
        assert!(!s.resync_pending);
    }

    #[test]
    fn an_empty_song_ends_immediately() {
        let f = Fixture::new();
        let mut s = song_seq();
        s.play();
        f.run(&mut s, 10);
        assert!(!s.is_playing());
        assert!(s.take_ended());
    }

    #[test]
    fn note_tracks_select_queue_and_report() {
        let mut f = Fixture::new();
        f.notes[0][1].length = 4;
        let mut s = new_seq();
        s.select(4, 1);
        s.select(5, 2);
        assert_eq!((s.current_pattern(4), s.current_pattern(5)), (1, 2));
        s.play();
        s.select(4, 3);
        assert_eq!(s.queued_pattern(4), Some(3));
        f.run(&mut s, 4 * 5513);
        assert_eq!(s.current_pattern(4), 3, "switched after the 4-step pattern");
        assert_eq!(s.queued_pattern(4), None);
    }

    fn synth_events(ev: &[(usize, SeqEvent)]) -> Vec<(usize, SeqEvent)> {
        ev.iter()
            .filter(|(_, e)| {
                matches!(
                    e,
                    SeqEvent::Note { slot: 0, .. } | SeqEvent::NoteOff { slot: 0 }
                )
            })
            .copied()
            .collect()
    }

    #[test]
    fn a_song_ending_with_the_gate_open_sends_a_note_off() {
        let mut f = Fixture::new();
        // Step 0's Tie continues step 15 over the loop (see
        // `SynthGrid::to_roll`), as the old step sequencer did.
        set_steps(
            &mut f.synths[0][0],
            16,
            &[
                (
                    0,
                    SynthStep {
                        gate: Gate::Tie,
                        ..on(0)
                    },
                ),
                (15, on(3)),
            ],
        );
        f.set_bars(&[[Some(0), None, None, None]]);
        let mut s = song_seq();
        s.play();
        let ev = synth_events(&f.run(&mut s, 17 * 5513));
        assert_eq!(
            ev.last(),
            Some(&(88_200, SeqEvent::NoteOff { slot: 0 })),
            "step 15 ties into the wrap, so only the song end closes it: {ev:?}"
        );
        assert!(!s.is_playing());
    }

    // A v5 bar lasted as long as its longest pattern, so a shorter acid
    // pattern looping in it could be cut mid-note by the bar line, which
    // is now the next block's start: the note ends there, as in v4
    // (sample 88200 at 120 BPM), unless one starts on it.
    #[test]
    fn a_note_cut_by_a_block_line_ends_on_it() {
        let mut f = Fixture::new();
        let tie = SynthStep {
            gate: Gate::Tie,
            ..on(0)
        };
        set_steps(&mut f.synths[0][0], 6, &[(3, on(3)), (4, tie), (5, tie)]);
        f.drums[0][0].roll.length = 16;
        f.set_bars(&[[Some(0), None, Some(0), None]; 2]);
        let mut s = song_seq();
        s.play();
        // Up to the next bar's first note (step 19 = sample 104738).
        let ev: Vec<_> = synth_events(&f.run(&mut s, 104_000))
            .into_iter()
            .filter(|(p, _)| *p >= 82_000)
            .collect();
        assert_eq!(
            ev,
            vec![
                (
                    82_688,
                    SeqEvent::Note {
                        slot: 0,
                        midi: 39,
                        accent: false,
                        legato: false
                    }
                ),
                (88_200, SeqEvent::NoteOff { slot: 0 }),
            ],
            "{ev:?}"
        );
    }

    #[test]
    fn a_track_turning_silent_restores_its_lane_param() {
        use crate::params::SynthParam;
        use crate::project::ParamLane;
        let mut f = Fixture::new();
        let id = MachineUid(1).param(SynthParam::Cutoff);
        let mut values = vec![None; 64];
        values[15] = Some(0.8);
        f.synths[0][0].param_lane = Some(ParamLane { param: id, values });
        f.set_bars(&[[Some(0), None, None, None], [None; 4]]);
        let mut s = song_seq();
        s.play();
        let ev: Vec<_> = f
            .run(&mut s, 17 * 5513)
            .into_iter()
            .filter(|(_, e)| matches!(e, SeqEvent::Param { .. } | SeqEvent::ParamRestore { .. }))
            .collect();
        assert_eq!(
            ev,
            vec![
                (82_688, SeqEvent::Param { id, value: 0.8 }),
                (88_200, SeqEvent::ParamRestore { id }),
            ]
        );
    }

    /// One song bar playing synth pattern 0, whose last step slides into
    /// its own step 0, followed by `next`.
    fn slide_across_the_bar_line(next: [Option<u8>; 4]) -> Vec<(usize, SeqEvent)> {
        let mut f = Fixture::new();
        set_steps(&mut f.synths[0][0], 16, &[(0, on(0))]);
        set_steps(
            &mut f.synths[0][0],
            16,
            &[(
                15,
                SynthStep {
                    slide: true,
                    ..on(3)
                },
            )],
        );
        set_steps(&mut f.synths[0][1], 16, &[(0, on(7))]);
        set_steps(
            &mut f.synths[0][2],
            16,
            &[(
                0,
                SynthStep {
                    gate: Gate::Tie,
                    ..on(9)
                },
            )],
        );
        f.set_bars(&[[Some(0), None, None, None], next]);
        let mut s = song_seq();
        s.play();
        synth_events(&f.run(&mut s, 89_000))
            .into_iter()
            .filter(|(p, _)| *p >= 82_688)
            .collect()
    }

    // Decision #5 (303-like): the hold on a bar's last step looks at the
    // current pattern's own wrap, and the held gate runs into whatever the
    // next bar plays. Behaviour change in v5: a Tie after a bar line now
    // starts its own note (the next bar's pattern isn't known when a v4
    // pattern converts), so it slides in like any note instead of keeping
    // the old pitch.
    #[test]
    fn a_slide_on_a_bars_last_step_runs_into_the_next_bar() {
        let note = |midi, legato| SeqEvent::Note {
            slot: 0,
            midi,
            accent: false,
            legato,
        };
        assert_eq!(
            slide_across_the_bar_line([Some(1), None, None, None]),
            vec![(82_688, note(39, false)), (88_200, note(43, true))],
            "the next bar's first note slides in (legato)"
        );
        assert_eq!(
            slide_across_the_bar_line([Some(2), None, None, None]),
            vec![(82_688, note(39, false)), (88_200, note(45, true))],
            "a Tie after the bar line starts its own note: it slides in too"
        );
        assert_eq!(
            slide_across_the_bar_line([None; 4]),
            vec![
                (82_688, note(39, false)),
                (88_200, SeqEvent::NoteOff { slot: 0 })
            ],
            "a silent next bar cuts the note at its start, not mid-step"
        );
    }

    #[test]
    fn a_tie_on_step_0_continues_the_last_note_over_the_loop() {
        let mut f = Fixture::new();
        let tie = SynthStep {
            gate: Gate::Tie,
            ..on(0)
        };
        set_steps(
            &mut f.synths[0][0],
            16,
            &[
                (
                    0,
                    SynthStep {
                        gate: Gate::Tie,
                        ..on(9)
                    },
                ),
                (12, on(3)),
                (13, tie),
                (14, tie),
                (15, tie),
            ],
        );
        let mut s = new_seq();
        s.play();
        let ev: Vec<_> = synth_events(&f.run(&mut s, 92_000))
            .into_iter()
            .filter(|(p, _)| *p >= 66_150)
            .collect();
        let note = |legato| SeqEvent::Note {
            slot: 0,
            midi: 39,
            accent: false,
            legato,
        };
        assert_eq!(
            ev,
            vec![
                (66_150, note(false)),
                (88_200, note(true)),
                (90_957, SeqEvent::NoteOff { slot: 0 }),
            ],
            "held over the wrap: one legato note at the same pitch"
        );
    }

    use crate::project::Note;

    fn note(start: u16, len: u16, pitch: u8) -> Note {
        Note::new(start, len, pitch, 100)
    }

    fn poly(ev: &[(usize, SeqEvent)]) -> Vec<(usize, SeqEvent)> {
        ev.iter()
            .filter(|(_, e)| matches!(e, SeqEvent::PolyOn { .. } | SeqEvent::PolyOff { .. }))
            .copied()
            .collect()
    }

    #[test]
    fn poly_notes_land_on_their_ticks() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(1, 4, 60));
        let mut s = new_seq();
        s.play();
        let ev = poly(&f.run(&mut s, 8000));
        assert_eq!(
            ev,
            vec![
                (
                    1379,
                    SeqEvent::PolyOn {
                        slot: 4,
                        pitch: 60,
                        vel: 100
                    }
                ),
                (6891, SeqEvent::PolyOff { slot: 4, pitch: 60 }),
            ]
        );
    }

    #[test]
    fn chords_start_together_on_the_sampler() {
        let mut f = Fixture::new();
        for p in [60, 64, 67] {
            f.notes[1][0].notes.push(note(0, 2, p));
        }
        let mut s = new_seq();
        s.play();
        let ons: Vec<_> = poly(&f.run(&mut s, 100))
            .into_iter()
            .filter(|(pos, e)| *pos == 0 && matches!(e, SeqEvent::PolyOn { slot: 5, .. }))
            .collect();
        assert_eq!(ons.len(), 3);
    }

    #[test]
    fn notes_are_cut_at_the_loop_end() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(60, 40, 50));
        let mut s = new_seq();
        s.play();
        let ev = poly(&f.run(&mut s, 88_300));
        assert_eq!(
            ev.last(),
            Some(&(88_200, SeqEvent::PolyOff { slot: 4, pitch: 50 }))
        );
    }

    #[test]
    fn song_mutes_and_transposes_note_tracks() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(0, 4, 60));
        f.notes[1][0].notes.push(note(0, 4, 60));
        f.song.lane_mut(MachineUid(5)).push(Block {
            transpose: 3,
            ..Block::new(0, PatternRef(0))
        });
        f.song.lane_mut(MachineUid(6)).push(Block {
            mute: true,
            ..Block::new(0, PatternRef(0))
        });
        let mut s = new_seq();
        s.set_mode(Mode::Song);
        s.play();
        let ev = poly(&f.run(&mut s, 2000));
        assert_eq!(
            ev,
            vec![(
                0,
                SeqEvent::PolyOn {
                    slot: 4,
                    pitch: 63,
                    vel: 100
                }
            )]
        );
    }

    #[test]
    fn a_song_ending_releases_held_notes() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(0, 64, 60));
        f.song
            .lane_mut(MachineUid(5))
            .push(Block::new(0, PatternRef(0)));
        let mut s = new_seq();
        s.set_mode(Mode::Song);
        s.play();
        let ev = poly(&f.run(&mut s, 90_000));
        assert_eq!(
            ev.last(),
            Some(&(88_200, SeqEvent::PolyOff { slot: 4, pitch: 60 }))
        );
        assert!(!s.is_playing());
    }

    /// Plays a 4-step FM note at pitch 60 up to mid-pattern, applies
    /// `edit` to the pattern (as a live edit or undo swap would), then
    /// returns the poly events of the next tick.
    fn after_live_edit(edit: impl FnOnce(&mut NotePattern)) -> Vec<SeqEvent> {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(0, 16, 60));
        let mut s = new_seq();
        s.play();
        let ev = poly(&f.run(&mut s, 5600)); // pos is 5 (step 1, first quarter)
        assert_eq!(ev.len(), 1, "only the note-on so far: {ev:?}");
        edit(&mut f.notes[0][0]);
        poly(&f.run(&mut s, 1400))
            .into_iter()
            .map(|(_, e)| e)
            .collect()
    }

    const OFF_60: SeqEvent = SeqEvent::PolyOff { slot: 4, pitch: 60 };

    #[test]
    fn deleting_a_sounding_note_releases_it_on_the_next_tick() {
        assert_eq!(after_live_edit(|p| p.notes.clear()), vec![OFF_60]);
    }

    #[test]
    fn repitching_a_sounding_note_releases_the_old_pitch() {
        assert_eq!(after_live_edit(|p| p.notes[0].pitch = 72), vec![OFF_60]);
    }

    #[test]
    fn shortening_a_note_behind_the_playhead_releases_it() {
        assert_eq!(after_live_edit(|p| p.notes[0].len = 2), vec![OFF_60]);
    }

    #[test]
    fn moving_a_note_off_the_playhead_releases_it() {
        assert_eq!(after_live_edit(|p| p.notes[0].start = 32), vec![OFF_60]);
    }

    #[test]
    fn an_untouched_sounding_note_keeps_ringing() {
        assert_eq!(after_live_edit(|_| {}), vec![]);
    }

    #[test]
    fn back_to_back_notes_of_one_pitch_retrigger() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(0, 4, 60));
        f.notes[0][0].notes.push(note(4, 4, 60));
        let mut s = new_seq();
        s.play();
        let ev: Vec<_> = poly(&f.run(&mut s, 5600))
            .into_iter()
            .map(|(_, e)| e)
            .collect();
        let on = SeqEvent::PolyOn {
            slot: 4,
            pitch: 60,
            vel: 100,
        };
        assert_eq!(ev, vec![on, OFF_60, on]);
    }

    #[test]
    fn a_queued_switch_releases_the_old_patterns_notes() {
        let mut f = Fixture::new();
        f.notes[0][0].length = 1;
        f.notes[0][0].notes.push(note(0, 4, 60));
        f.notes[0][1].length = 1;
        f.notes[0][1].notes.push(note(0, 4, 62));
        let mut s = new_seq();
        s.play();
        s.select(4, 1);
        let ev = poly(&f.run(&mut s, 5600));
        assert_eq!(
            ev,
            vec![
                (
                    0,
                    SeqEvent::PolyOn {
                        slot: 4,
                        pitch: 60,
                        vel: 100
                    }
                ),
                (5513, SeqEvent::PolyOff { slot: 4, pitch: 60 }),
                (
                    5513,
                    SeqEvent::PolyOn {
                        slot: 4,
                        pitch: 62,
                        vel: 100
                    }
                ),
            ]
        );
    }

    #[test]
    fn poly_notes_follow_swing() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(TICKS_PER_STEP, 2, 60));
        set_hits(&mut f.drums[0][0], 16, &[(1, 0, DrumHit::On)]);
        let mut s = new_seq();
        s.set_swing(0.5);
        s.play();
        let ev = f.run(&mut s, 8000);
        let at = |want: fn(&SeqEvent) -> bool| ev.iter().find(|(_, e)| want(e)).map(|(p, _)| *p);
        let drum = at(|e| matches!(e, SeqEvent::Drum { .. }));
        let poly_on = at(|e| matches!(e, SeqEvent::PolyOn { .. }));
        assert_eq!(drum, Some(6891), "the swung odd step");
        assert_eq!(poly_on, drum, "the note on step 1 lands with the drum");
    }

    #[test]
    fn shrinking_the_pattern_past_the_playhead_wraps_and_releases() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(note(0, 64, 60));
        let mut s = new_seq();
        s.play();
        let ev = poly(&f.run(&mut s, 10 * 5513)); // into step 10
        assert_eq!(ev.len(), 1, "only the note-on so far: {ev:?}");
        f.notes[0][0].length = 4;
        let ev: Vec<_> = poly(&f.run(&mut s, 1400))
            .into_iter()
            .map(|(_, e)| e)
            .collect();
        let on = SeqEvent::PolyOn {
            slot: 4,
            pitch: 60,
            vel: 100,
        };
        assert_eq!(ev, vec![OFF_60, on], "wraps to tick 0 and retriggers");
    }

    #[test]
    fn tracks_follow_the_bank_at_their_position_and_its_type() {
        use crate::machine::{MachineType, MachineUid};
        let mut drums = vec![DrumPattern::default(); 2];
        set_hits(&mut drums[0], 16, &[(0, 3, DrumHit::On)]);
        let mut notes = vec![NotePattern::default(); 2];
        notes[0].notes.push(note(0, 2, 60));
        let mut b = Banks::empty();
        let rack = [
            (MachineUid(9), MachineType::Kit909, Bank::Drum(&drums)),
            (MachineUid(3), MachineType::Sampler, Bank::Notes(&notes)),
            (MachineUid(4), MachineType::Fm, Bank::Notes(&notes)),
        ];
        for (pos, (uid, ty, bank)) in rack.into_iter().enumerate() {
            b.machines[pos] = Some((uid, ty));
            b.slots[pos] = Some(bank);
        }
        let mut song = crate::project::Song::default();
        for uid in [9, 3, 4].map(MachineUid) {
            song.lane_mut(uid).push(Block {
                transpose: 5,
                ..Block::new(0, PatternRef(0))
            });
        }
        let song = CompiledSong::compile(&song, &[MachineUid(9), MachineUid(3), MachineUid(4)]);
        let mut s = song_seq();
        s.play();
        let mut out = Vec::new();
        s.tick(b, &song, &mut out);
        assert_eq!(
            out,
            vec![
                SeqEvent::Drum {
                    slot: 0,
                    inst: 3,
                    accent: false
                },
                SeqEvent::PolyOn {
                    slot: 1,
                    pitch: 60,
                    vel: 100
                },
                SeqEvent::PolyOn {
                    slot: 2,
                    pitch: 65,
                    vel: 100
                },
            ],
            "the Sampler ignores the transpose; the FM takes it"
        );
        assert_eq!(s.playhead(3), 0, "an empty position has no track");
    }

    // Phase 6: chance, ratchet, conditions and fill.

    use crate::project::Cond;

    /// Runs the next `n` ticks; events by tick, counted from the first.
    fn run_ticks(f: &Fixture, s: &mut Sequencer, n: u64) -> Vec<(u64, SeqEvent)> {
        let uids: Vec<_> = standard_rack().iter().map(|m| m.0).collect();
        let song = CompiledSong::compile(&f.song, &uids);
        let mut out = Vec::new();
        let mut buf = Vec::new();
        for k in 0..n {
            s.advance(s.samples_to_next_tick());
            buf.clear();
            s.tick(f.banks(), &song, &mut buf);
            out.extend(buf.iter().map(|e| (k, *e)));
        }
        out
    }

    /// The acid track at 0: 'N' a note, 'L' a legato note, 'O' a note-off.
    fn acid(ev: &[(u64, SeqEvent)]) -> Vec<(u64, char)> {
        ev.iter()
            .filter_map(|&(t, e)| match e {
                SeqEvent::Note {
                    slot: 0, legato, ..
                } => Some((t, if legato { 'L' } else { 'N' })),
                SeqEvent::NoteOff { slot: 0 } => Some((t, 'O')),
                _ => None,
            })
            .collect()
    }

    /// The first kit's hits as (tick, instrument).
    fn kit(ev: &[(u64, SeqEvent)]) -> Vec<(u64, usize)> {
        ev.iter()
            .filter_map(|&(t, e)| match e {
                SeqEvent::Drum { slot: 2, inst, .. } => Some((t, inst)),
                _ => None,
            })
            .collect()
    }

    /// The FM's events as (tick, on, pitch).
    fn fm(ev: &[(u64, SeqEvent)]) -> Vec<(u64, bool, u8)> {
        ev.iter()
            .filter_map(|&(t, e)| match e {
                SeqEvent::PolyOn { slot: 4, pitch, .. } => Some((t, true, pitch)),
                SeqEvent::PolyOff { slot: 4, pitch } => Some((t, false, pitch)),
                _ => None,
            })
            .collect()
    }

    fn ratcheted(start: u16, len: u16, pitch: u8, ratchet: u8) -> Note {
        Note {
            ratchet,
            ..note(start, len, pitch)
        }
    }

    fn cond(start: u16, pitch: u8, cond: Cond) -> Note {
        Note {
            cond,
            ..note(start, 1, pitch)
        }
    }

    /// The first kit on a one-step pattern (a loop every 4 ticks).
    fn one_step_kit(hits: &[Note]) -> Fixture {
        let mut f = Fixture::new();
        f.drums[0][0].roll.length = 1;
        f.drums[0][0].roll.notes.extend_from_slice(hits);
        f
    }

    #[test]
    fn an_acid_ratchet_plays_fresh_gates() {
        let mut f = Fixture::new();
        f.synths[0][0].roll.notes.push(ratcheted(0, 12, 48, 3));
        let mut s = new_seq();
        s.play();
        assert_eq!(
            acid(&run_ticks(&f, &mut s, 16)),
            [(0, 'N'), (2, 'O'), (4, 'N'), (6, 'O'), (8, 'N'), (10, 'O')]
        );
    }

    #[test]
    fn an_acid_ratchet_slides_only_after_its_last_hit() {
        let mut f = Fixture::new();
        let roll = &mut f.synths[0][0].roll;
        roll.notes.push(Note {
            slide: true,
            ..ratcheted(0, 12, 48, 3)
        });
        roll.notes.push(note(12, 4, 50));
        let mut s = new_seq();
        s.play();
        assert_eq!(
            acid(&run_ticks(&f, &mut s, 16)),
            [
                (0, 'N'),
                (2, 'O'),
                (4, 'N'),
                (6, 'O'),
                (8, 'N'),
                (12, 'L'),
                (14, 'O')
            ]
        );
    }

    #[test]
    fn a_skipped_acid_note_is_a_gap() {
        let slide_into = |chance: u8| {
            let mut f = Fixture::new();
            let roll = &mut f.synths[0][0].roll;
            roll.notes.push(Note {
                slide: true,
                ..note(0, 4, 48)
            });
            roll.notes.push(Note {
                chance,
                ..note(4, 4, 50)
            });
            let mut s = new_seq();
            s.play();
            acid(&run_ticks(&f, &mut s, 12))
        };
        assert_eq!(slide_into(100), [(0, 'N'), (4, 'L'), (6, 'O')]);
        assert_eq!(slide_into(0), [(0, 'N'), (2, 'O')]);
    }

    #[test]
    fn a_drum_ratchet_hits_on_each_sub_hit() {
        let mut f = Fixture::new();
        f.drums[0][0].roll.notes.push(ratcheted(0, 12, 1, 3));
        f.drums[0][0].roll.notes.push(note(4, 1, 0));
        let mut s = new_seq();
        s.play();
        assert_eq!(
            kit(&run_ticks(&f, &mut s, 16)),
            [(0, 1), (4, 0), (4, 1), (8, 1)],
            "in instrument order on a shared tick"
        );
    }

    #[test]
    fn a_poly_ratchet_releases_and_retriggers() {
        let mut f = Fixture::new();
        f.notes[0][0].notes.push(ratcheted(0, 12, 60, 3));
        let mut s = new_seq();
        s.play();
        assert_eq!(
            fm(&run_ticks(&f, &mut s, 16)),
            [
                (0, true, 60),
                (4, false, 60),
                (4, true, 60),
                (8, false, 60),
                (8, true, 60),
                (12, false, 60)
            ]
        );
    }

    #[test]
    fn a_ratchet_does_not_run_past_the_loop() {
        // Pattern of 2 steps (8 ticks); the note's later sub-hits fall
        // past its end and are dropped, also on the next pass.
        let mut f = Fixture::new();
        f.drums[0][0].roll.length = 2;
        f.drums[0][0].roll.notes.push(ratcheted(4, 12, 1, 3));
        let mut s = new_seq();
        s.play();
        assert_eq!(kit(&run_ticks(&f, &mut s, 16)), [(4, 1), (12, 1)]);
    }

    #[test]
    fn a_ratio_plays_every_other_loop() {
        let f = one_step_kit(&[cond(0, 0, Cond::Ratio(1, 2)), cond(0, 1, Cond::Ratio(2, 2))]);
        let mut s = new_seq();
        s.play();
        assert_eq!(
            kit(&run_ticks(&f, &mut s, 24)),
            [(0, 0), (4, 1), (8, 0), (12, 1), (16, 0), (20, 1)]
        );
        // Play starts the count again.
        s.play();
        assert_eq!(kit(&run_ticks(&f, &mut s, 4)), [(0, 0)]);
    }

    #[test]
    fn a_queued_switch_starts_the_loop_count_again() {
        let mut f = one_step_kit(&[note(0, 1, 0)]);
        f.drums[0][1].roll.length = 1;
        f.drums[0][1].roll.notes.push(cond(0, 1, Cond::First));
        let mut s = new_seq();
        s.play();
        let _ = run_ticks(&f, &mut s, 8);
        s.select(2, 1);
        // The switch lands at the next wrap, on its first pass.
        assert_eq!(kit(&run_ticks(&f, &mut s, 12)), [(0, 0), (4, 1)]);
    }

    #[test]
    fn the_loop_count_restarts_at_each_song_block() {
        let mut f = one_step_kit(&[cond(0, 0, Cond::First)]);
        for start in [0, 3] {
            f.song.lane_mut(MachineUid(3)).push(Block {
                len: Some(3),
                ..Block::new(start, PatternRef(0))
            });
        }
        f.song.end = Some(6);
        let mut s = song_seq();
        s.play();
        assert_eq!(kit(&run_ticks(&f, &mut s, 24)), [(0, 0), (12, 0)]);
    }

    #[test]
    fn a_seek_counts_the_passes_straight_playback_would() {
        // One 8-step block of a one-step pattern: 1:2 plays on steps 0, 2,
        // 4, 6. Played from step 3 it plays on 4 and 6.
        let mut f = one_step_kit(&[cond(0, 0, Cond::Ratio(1, 2))]);
        f.song.lane_mut(MachineUid(3)).push(Block {
            len: Some(8),
            ..Block::new(0, PatternRef(0))
        });
        f.song.end = Some(8);
        let mut s = song_seq();
        s.set_song_start(3);
        s.play();
        assert_eq!(kit(&run_ticks(&f, &mut s, 20)), [(4, 0), (12, 0)]);
    }

    #[test]
    fn a_joining_track_counts_the_passes_since_play() {
        // Pattern mode: a machine joining at tick 8 of one-step patterns
        // is on pass 2, where 1:2 plays.
        let f = one_step_kit(&[cond(0, 0, Cond::Ratio(1, 2))]);
        let mut s = new_seq();
        s.play();
        let _ = run_ticks(&f, &mut s, 8);
        let mut map: RackMap = std::array::from_fn(Some);
        map[2] = None;
        let song = CompiledSong::default();
        s.rearrange(&map, f.banks(), &song, &[0; MAX_MACHINES]);
        assert_eq!(kit(&run_ticks(&f, &mut s, 8)), [(0, 0)]);
    }

    #[test]
    fn fill_toggles_fill_notes() {
        let f = one_step_kit(&[cond(0, 0, Cond::Fill), cond(0, 1, Cond::NotFill)]);
        let mut s = new_seq();
        s.play();
        assert_eq!(kit(&run_ticks(&f, &mut s, 4)), [(0, 1)]);
        s.set_fill(true);
        assert!(s.fill());
        assert_eq!(kit(&run_ticks(&f, &mut s, 4)), [(0, 0)]);
        s.set_fill(false);
        assert_eq!(kit(&run_ticks(&f, &mut s, 4)), [(0, 1)]);
    }

    #[test]
    fn pre_follows_the_previous_conditional_note() {
        // A 0 % hit, then one on its result and one on its opposite.
        let f = one_step_kit(&[
            Note {
                chance: 0,
                ..note(0, 1, 0)
            },
            cond(1, 1, Cond::Pre),
            cond(2, 2, Cond::NotPre),
        ]);
        let mut s = new_seq();
        s.play();
        assert_eq!(kit(&run_ticks(&f, &mut s, 8)), [(2, 2), (6, 2)]);
    }

    #[test]
    fn chance_is_deterministic_and_follows_the_seed() {
        let f = one_step_kit(&[Note {
            chance: 50,
            ..note(0, 1, 0)
        }]);
        let go = |seed: u32| {
            let mut s = new_seq();
            s.set_seed(seed);
            s.play();
            kit(&run_ticks(&f, &mut s, 256))
        };
        let a = go(0);
        assert!((16..=48).contains(&a.len()), "{}", a.len());
        assert_eq!(a, go(0));
        assert_ne!(a, go(1));
    }
}
