//! Real-time bridge between the UI thread and the audio thread. The UI
//! pushes `Command`s; the audio callback calls `EngineHost::process`,
//! which applies them, renders, and pushes back `Event`s: a ~60 Hz
//! `Status`, plus every replaced pattern or engine so that memory is
//! freed on the UI thread, never on the audio thread.

use crate::engine::Engine;
use crate::machine::MachineUid;
use crate::params::ParamId;
use crate::project::{DrumPattern, NotePattern, SynthPattern};
use crate::rack::{CompiledSong, RackSwap};
use crate::sampler::sample::SampleData;
use crate::sampler::slot::SlotPlay;
use crate::sequencer::Mode;
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::Arc;

pub const COMMAND_CAPACITY: usize = 4096;
pub const EVENT_CAPACITY: usize = 1024;
const STATUS_RATE_HZ: f32 = 60.0;
/// Event slots kept free for disposals: statuses never use the last ones,
/// so a stalled UI cannot make replaced boxes leak.
const DISPOSAL_RESERVE: usize = 256;
/// Weight of the newest CPU-load sample in the smoothed value.
pub const CPU_SMOOTHING: f32 = 0.1;
/// Spare buffers the host holds at once.
pub const STATUS_BUFFERS: usize = 4;

/// One pattern of any machine type, for `Command::Pattern`.
#[derive(Clone, Debug, PartialEq)]
pub enum PatternData {
    Synth(SynthPattern),
    Drum(DrumPattern),
    Notes(NotePattern),
}

pub enum Command {
    SetParam(ParamId, f32),
    SetBpm(f32),
    SetSwing(f32),
    Play,
    Stop,
    /// Selects (or, while playing, queues) pattern `slot` on machine `uid`.
    SelectPattern {
        uid: MachineUid,
        slot: u8,
    },
    /// Stores `pattern` at `index` of machine `uid`'s bank. The old
    /// pattern (or this one, if refused) comes back as `DisposePattern`.
    Pattern {
        uid: MachineUid,
        index: u8,
        pattern: Box<PatternData>,
    },
    SamplerSample {
        uid: MachineUid,
        slot: u8,
        sample: Option<Arc<SampleData>>,
    },
    SamplerPlay {
        uid: MachineUid,
        slot: u8,
        play: Box<SlotPlay>,
    },
    ReplaceEngine(Box<Engine>),
    SetMode(Mode),
    /// The song compiled for the engine's rack order.
    Song(Box<CompiledSong>),
    /// The step Song-mode Play starts from.
    SetSongStart(u32),
    /// The user is holding (true) or released (false) a knob while recording.
    Touch(ParamId, bool),
    /// Sound follows the knob without changing its saved value (record mode).
    SetParamLive(ParamId, f32),
    /// A rack edit: the new order plus the slots the engine lacks. Comes
    /// back as `DisposeRack` with what left the engine (or untouched, if
    /// malformed).
    SetRack(Box<RackSwap>),
    /// Audition: play a note on a note machine now.
    NoteOn {
        uid: MachineUid,
        pitch: u8,
        vel: u8,
    },
    NoteOff {
        uid: MachineUid,
        pitch: u8,
    },
    /// Fill held or latched (true) or let go (false).
    SetFill(bool),
    /// The project's chance seed changed.
    SetSeed(u32),
    /// A spare buffer for a status's per-position values. Comes back in a
    /// `Status`, or as `DisposePositions` if it does not fit the rack or
    /// the host holds `STATUS_BUFFERS` already.
    StatusBuffer(Box<PositionStatus>),
}

/// By rack position, for one status. Allocated on the UI thread
/// (`with_capacity`), filled on the audio thread without growing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PositionStatus {
    pub playhead: Vec<usize>,
    pub patterns: Vec<u8>,
    pub queued: Vec<Option<u8>>,
    /// Post-fader peaks since the previous status; muted machines read 0.
    pub meters: Vec<f32>,
    /// Share of the audio time spent on each machine.
    pub slot_cpu: Vec<f32>,
}

impl PositionStatus {
    pub fn with_capacity(n: usize) -> Box<Self> {
        Box::new(Self {
            playhead: Vec::with_capacity(n),
            patterns: Vec::with_capacity(n),
            queued: Vec::with_capacity(n),
            meters: Vec::with_capacity(n),
            slot_cpu: Vec::with_capacity(n),
        })
    }

    /// Every Vec has room for `n` entries.
    pub fn fits(&self, n: usize) -> bool {
        [
            self.playhead.capacity(),
            self.patterns.capacity(),
            self.queued.capacity(),
            self.meters.capacity(),
            self.slot_cpu.capacity(),
        ]
        .iter()
        .all(|&c| c >= n)
    }

    pub fn clear(&mut self) {
        self.playhead.clear();
        self.patterns.clear();
        self.queued.clear();
        self.meters.clear();
        self.slot_cpu.clear();
    }

    /// Rack positions filled.
    pub fn len(&self) -> usize {
        self.playhead.len()
    }

    pub fn is_empty(&self) -> bool {
        self.playhead.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub playing: bool,
    /// The master peak since the previous status.
    pub master_meter: f32,
    /// The per-position values, when the host held a buffer that fits the
    /// rack (see `Command::StatusBuffer`). Without one the slots keep their
    /// meters and CPU time for the next status that has one.
    pub positions: Option<Box<PositionStatus>>,
    /// Smoothed fraction of the callback's time budget spent rendering.
    pub cpu: f32,
    /// Worst single callback load since the previous status.
    pub cpu_peak: f32,
    /// Callbacks whose render took longer than their time budget, counted
    /// for the life of the stream (wrapping); engine swaps do not reset it.
    pub overruns: u32,
    pub guard_fired: bool,
    pub mode: Mode,
    /// The 16-step bar of the song position (song mode; display only).
    pub song_bar: u16,
    /// Global song step (song mode).
    pub song_step: u32,
    /// A `Command::Song` was refused (an invalid song) since the previous
    /// status; the engine kept its old song.
    pub song_rejected: bool,
    /// A `Command::SetRack` was refused (malformed) since the previous
    /// status; the engine kept its rack.
    pub rack_rejected: bool,
    /// `Command::Stop`s taken since this engine was loaded (wrapping). The
    /// UI counts the Stops it sends the same way: a status that has not
    /// caught up predates its latest Stop.
    pub stops: u32,
    /// Rack generation this status reports on: the host's count of racks
    /// taken, i.e. accepted `SetRack`s plus `ReplaceEngine`s (wrapping;
    /// never reset). `positions` describes that rack.
    pub rack_gen: u32,
    /// Share spent on everything else: sequencing, sends and master.
    pub shared_cpu: f32,
}

// Events travel by value through a preallocated ring: boxing the Status
// would allocate on the audio thread.
pub enum Event {
    Status(Status),
    DisposePattern(Box<PatternData>),
    DisposeEngine(Box<Engine>),
    DisposeSong(Box<CompiledSong>),
    DisposeSample(Option<Arc<SampleData>>),
    DisposeSlotPlay(Box<SlotPlay>),
    /// After `SetRack`: the old slot `Vec` (in `into`) with the removed
    /// machines, unused fresh slots and the old song.
    DisposeRack(Box<RackSwap>),
    /// A status buffer too small for the rack, handed back unfilled.
    DisposePositions(Box<PositionStatus>),
}

pub struct EngineHost {
    engine: Box<Engine>,
    commands: Consumer<Command>,
    events: Producer<Event>,
    status_every: usize,
    since_status: usize,
    /// The master peak since the previous status.
    master_meter: f32,
    guard_fired: bool,
    song_rejected: bool,
    rack_rejected: bool,
    /// Racks taken: accepted `SetRack`s plus `ReplaceEngine`s (wrapping).
    rack_gen: u32,
    cpu: f32,
    cpu_peak: f32,
    overruns: u32,
    stops: u32,
    /// Shared engine work (sequencing, sends, master) since the previous
    /// status with positions; each slot keeps its own until one takes it.
    shared_ns: u64,
    /// Frames rendered since the previous status with positions (the
    /// audio time the work above covers).
    cpu_frames: usize,
    /// Buffers from the UI for the next statuses' per-position values.
    spare: [Option<Box<PositionStatus>>; STATUS_BUFFERS],
}

/// Wires an engine to a fresh pair of rings. Call on the UI thread; move
/// the `EngineHost` into the audio callback.
///
/// Drain the `Consumer<Event>` regularly (every UI frame): statuses and
/// disposals share it. While it is stalled, statuses stop once only the
/// last `DISPOSAL_RESERVE` slots are free, keeping those for disposals;
/// boxes beyond that are leaked rather than freed on the audio thread.
pub fn channel(engine: Engine) -> (EngineHost, Producer<Command>, Consumer<Event>) {
    let (command_tx, command_rx) = RingBuffer::new(COMMAND_CAPACITY);
    let (event_tx, event_rx) = RingBuffer::new(EVENT_CAPACITY);
    let status_every = (engine.sample_rate() / STATUS_RATE_HZ).max(1.0) as usize;
    let host = EngineHost {
        engine: Box::new(engine),
        commands: command_rx,
        events: event_tx,
        status_every,
        since_status: 0,
        master_meter: 0.0,
        guard_fired: false,
        song_rejected: false,
        rack_rejected: false,
        rack_gen: 0,
        cpu: 0.0,
        cpu_peak: 0.0,
        overruns: 0,
        stops: 0,
        shared_ns: 0,
        cpu_frames: 0,
        spare: Default::default(),
    };
    (host, command_tx, event_rx)
}

impl EngineHost {
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Records how much of the callback's time budget the last render
    /// used (1.0 = all of it).
    pub fn set_cpu_load(&mut self, load: f32) {
        if load.is_finite() {
            self.cpu += (load - self.cpu) * CPU_SMOOTHING;
            self.cpu_peak = self.cpu_peak.max(load);
            if load > 1.0 {
                self.overruns = self.overruns.wrapping_add(1);
            }
        }
    }

    /// Applies pending commands, renders, and reports. Real-time safe.
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        self.drain_commands();
        self.engine.process(out_l, out_r);
        self.master_meter = self.master_meter.max(self.engine.take_master_meter());
        self.guard_fired |= self.engine.take_guard_fired();
        self.shared_ns = self.shared_ns.saturating_add(self.engine.take_shared_ns());
        self.cpu_frames = self.cpu_frames.saturating_add(out_l.len());
        self.since_status += out_l.len();
        if self.since_status >= self.status_every {
            // Keep the remainder so the rate averages STATUS_RATE_HZ for
            // blocks up to `status_every` samples. A larger block still sends
            // one status (the cap stops a burst), so the rate drops to one
            // per block.
            self.since_status = (self.since_status - self.status_every).min(self.status_every);
            self.send_status();
        }
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                Command::SetParam(id, v) => self.engine.set_param(id, v),
                Command::SetBpm(bpm) => self.engine.set_bpm(bpm),
                Command::SetSwing(swing) => self.engine.set_swing(swing),
                Command::Play => self.engine.play(),
                Command::Stop => {
                    self.engine.stop();
                    self.stops = self.stops.wrapping_add(1);
                }
                Command::SelectPattern { uid, slot } => self.engine.select_pattern(uid, slot),
                Command::Pattern {
                    uid,
                    index,
                    mut pattern,
                } => {
                    self.engine.swap_pattern(uid, index, &mut pattern);
                    self.dispose(Event::DisposePattern(pattern));
                }
                Command::SamplerSample {
                    uid,
                    slot,
                    mut sample,
                } => {
                    self.engine
                        .swap_sampler_sample(uid, slot as usize, &mut sample);
                    self.dispose(Event::DisposeSample(sample));
                }
                Command::SamplerPlay {
                    uid,
                    slot,
                    mut play,
                } => {
                    self.engine.swap_sampler_play(uid, slot as usize, &mut play);
                    self.dispose(Event::DisposeSlotPlay(play));
                }
                Command::ReplaceEngine(mut engine) => {
                    std::mem::swap(&mut self.engine, &mut engine);
                    self.dispose(Event::DisposeEngine(engine));
                    // Nothing from the old engine carries over to the new one.
                    self.master_meter = 0.0;
                    self.guard_fired = false;
                    self.song_rejected = false;
                    self.rack_rejected = false;
                    self.rack_gen = self.rack_gen.wrapping_add(1);
                    self.since_status = 0;
                    self.stops = 0;
                    self.shared_ns = 0;
                    self.cpu_frames = 0;
                }
                Command::SetMode(mode) => self.engine.set_mode(mode),
                Command::Song(mut song) => {
                    self.song_rejected |= !self.engine.swap_song(&mut song);
                    self.dispose(Event::DisposeSong(song));
                }
                Command::SetSongStart(step) => self.engine.set_song_start(step),
                Command::Touch(id, on) => self.engine.touch(id, on),
                Command::SetParamLive(id, v) => self.engine.set_param_live(id, v),
                Command::SetRack(mut swap) => {
                    match self.engine.set_rack(&mut swap) {
                        Some(_) => self.rack_gen = self.rack_gen.wrapping_add(1),
                        None => self.rack_rejected = true,
                    }
                    self.dispose(Event::DisposeRack(swap));
                }
                Command::NoteOn { uid, pitch, vel } => self.engine.note_on(uid, pitch, vel),
                Command::NoteOff { uid, pitch } => self.engine.note_off(uid, pitch),
                Command::SetFill(fill) => self.engine.set_fill(fill),
                Command::SetSeed(seed) => self.engine.set_seed(seed),
                Command::StatusBuffer(b) => match self.spare.iter_mut().find(|s| s.is_none()) {
                    Some(s) => *s = Some(b),
                    None => self.dispose(Event::DisposePositions(b)),
                },
            }
        }
    }

    /// Hands a box back to the UI thread. If the ring is full the box is
    /// leaked rather than freed here: freeing would break real-time safety.
    fn dispose(&mut self, event: Event) {
        if let Err(PushError::Full(event)) = self.events.push(event) {
            std::mem::forget(event);
        }
    }

    fn send_status(&mut self) {
        // Statuses stop once only the disposal reserve is free; the slots
        // keep their stats until one with positions goes out.
        if self.events.slots() <= DISPOSAL_RESERVE {
            return;
        }
        let secs = self.cpu_frames as f32 / self.engine.sample_rate();
        let share = |ns: u64| {
            let s = ns as f32 / 1e9 / secs;
            if secs > 0.0 && s.is_finite() { s } else { 0.0 }
        };
        let positions = self.take_spare().map(|mut b| {
            b.clear();
            self.engine.fill_positions(&mut b, share);
            b
        });
        let status = Status {
            playing: self.engine.is_playing(),
            master_meter: self.master_meter,
            positions,
            cpu: self.cpu,
            cpu_peak: self.cpu_peak,
            overruns: self.overruns,
            guard_fired: self.guard_fired,
            mode: self.engine.mode(),
            song_bar: self.engine.song_bar().min(u16::MAX as usize) as u16,
            song_step: self.engine.song_step(),
            song_rejected: self.song_rejected,
            rack_rejected: self.rack_rejected,
            stops: self.stops,
            rack_gen: self.rack_gen,
            shared_cpu: share(self.shared_ns),
        };
        // The slots' CPU time waits for a status with positions, so the
        // shared time and the frames it covers wait with it.
        let took_slots = status.positions.is_some();
        if self.events.push(Event::Status(status)).is_ok() {
            self.master_meter = 0.0;
            self.cpu_peak = 0.0;
            self.guard_fired = false;
            self.song_rejected = false;
            self.rack_rejected = false;
            if took_slots {
                self.shared_ns = 0;
                self.cpu_frames = 0;
            }
        }
    }

    /// The first spare that fits the rack; spares that do not fit go back
    /// to the UI on the way.
    fn take_spare(&mut self) -> Option<Box<PositionStatus>> {
        let n = self.engine.rack_len();
        for i in 0..STATUS_BUFFERS {
            let Some(b) = self.spare[i].take() else {
                continue;
            };
            if b.fits(n) {
                return Some(b);
            }
            self.dispose(Event::DisposePositions(b));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::MachineUid;

    const SR: f32 = 44_100.0;

    fn host_with_demo() -> (EngineHost, rtrb::Producer<Command>, rtrb::Consumer<Event>) {
        channel(Engine::from_project(SR, &crate::demo::demo_project()))
    }

    fn run(host: &mut EngineHost, samples: usize) {
        let mut l = vec![0.0; 512];
        let mut r = vec![0.0; 512];
        let mut done = 0;
        while done < samples {
            host.process(&mut l, &mut r);
            done += 512;
        }
    }

    #[test]
    fn fill_and_seed_commands_reach_the_engine() {
        let (mut host, mut tx, _rx) = host_with_demo();
        tx.push(Command::SetFill(true)).unwrap();
        tx.push(Command::SetSeed(42)).unwrap();
        run(&mut host, 512);
        assert!(host.engine.fill());
        assert_eq!(host.engine.seed(), 42);
        tx.push(Command::SetFill(false)).unwrap();
        run(&mut host, 512);
        assert!(!host.engine.fill());
    }

    #[test]
    fn statuses_carry_shares_that_fit_in_the_budget() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::Play).unwrap();
        tx.push(Command::StatusBuffer(PositionStatus::with_capacity(16)))
            .ok()
            .unwrap();
        // Most statuses go without a buffer: the next one that has one
        // covers all the time since the previous one.
        run(&mut host, 48000);
        tx.push(Command::StatusBuffer(PositionStatus::with_capacity(16)))
            .ok()
            .unwrap();
        run(&mut host, 2048);
        let s = std::iter::from_fn(|| rx.pop().ok())
            .filter_map(|e| match e {
                Event::Status(s) => Some(s),
                _ => None,
            })
            .filter(|s| s.positions.is_some())
            .last()
            .unwrap();
        let slot_cpu = &s.positions.as_ref().unwrap().slot_cpu;
        assert_eq!(slot_cpu.len(), 6, "one per machine");
        let sum: f32 = slot_cpu.iter().sum::<f32>() + s.shared_cpu;
        assert!(slot_cpu.iter().all(|c| c.is_finite() && *c >= 0.0));
        assert!(s.shared_cpu.is_finite() && s.shared_cpu >= 0.0);
        assert!(sum > 0.0, "something was timed");
        assert!(
            sum < 1.0,
            "a test render on this machine is far below real time: {sum}"
        );
    }

    #[test]
    fn statuses_without_positions_keep_the_cpu_window_open() {
        let (mut host, _tx, _rx) = host_with_demo();
        let every = host.status_every;
        run(&mut host, 4 * every);
        assert!(host.cpu_frames >= 3 * every);
    }

    #[test]
    fn commands_for_an_unknown_uid_are_ignored_and_disposed() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::Pattern {
            uid: MachineUid(99),
            index: 0,
            pattern: Box::new(PatternData::Synth(Default::default())),
        })
        .unwrap();
        tx.push(Command::NoteOn {
            uid: MachineUid(99),
            pitch: 60,
            vel: 100,
        })
        .unwrap();
        run(&mut host, 512);
        assert!(
            std::iter::from_fn(|| rx.pop().ok()).any(|e| matches!(e, Event::DisposePattern(_)))
        );
    }

    #[test]
    fn a_pattern_of_the_wrong_type_is_refused() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let before = host.engine().pattern_len(MachineUid(3), 0); // 808 drum pattern length
        tx.push(Command::Pattern {
            uid: MachineUid(3),
            index: 0,
            pattern: Box::new(PatternData::Synth(Default::default())),
        })
        .unwrap();
        run(&mut host, 512);
        assert_eq!(host.engine().pattern_len(MachineUid(3), 0), before);
        assert!(
            std::iter::from_fn(|| rx.pop().ok()).any(|e| matches!(e, Event::DisposePattern(_)))
        );
    }

    #[test]
    fn a_rack_swap_reports_its_generation_and_comes_back_for_disposal() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let mut q = crate::demo::demo_project();
        q.machines.retain(|m| m.uid != MachineUid(3));
        let swap = RackSwap::build(&q, &host.engine().slot_uids(), SR);
        tx.push(Command::SetRack(swap)).unwrap();
        run(&mut host, SR as usize / 30);
        let (mut rack, mut gens) = (None, Vec::new());
        while let Ok(e) = rx.pop() {
            match e {
                Event::DisposeRack(b) => rack = Some(b),
                Event::Status(s) => gens.push(s.rack_gen),
                _ => {}
            }
        }
        let rack = rack.expect("the swap comes back");
        assert_eq!(rack.into.len(), 1);
        assert_eq!(rack.into[0].uid, MachineUid(3));
        assert!(!gens.is_empty() && gens.iter().all(|&g| g == 1), "{gens:?}");
        assert_eq!(host.engine().slot_uids().len(), 5);
    }

    #[test]
    fn rack_generation_counts_swaps_and_engine_replacements() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let last_gen = |rx: &mut rtrb::Consumer<Event>| {
            std::iter::from_fn(|| rx.pop().ok())
                .filter_map(|e| match e {
                    Event::Status(s) => Some(s.rack_gen),
                    _ => None,
                })
                .last()
        };
        run(&mut host, 2048);
        assert_eq!(last_gen(&mut rx), Some(0));
        let p = crate::demo::demo_project();
        tx.push(Command::ReplaceEngine(Box::new(Engine::from_project(
            SR, &p,
        ))))
        .unwrap();
        run(&mut host, 2048);
        assert_eq!(last_gen(&mut rx), Some(1), "a new engine is a new rack");
        let mut q = p.clone();
        q.machines.pop();
        let swap = crate::rack::RackSwap::build(&q, &host.engine().slot_uids(), SR);
        tx.push(Command::SetRack(swap)).unwrap();
        run(&mut host, 2048);
        assert_eq!(last_gen(&mut rx), Some(2));
    }

    #[test]
    fn a_refused_rack_swap_is_reported_once() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let mut p = crate::demo::demo_project();
        let extra = crate::project::MachineEntry::new(
            crate::machine::MachineUid(7),
            crate::machine::MachineType::Fm,
            "FM 2".into(),
        );
        p.machines.push(extra);
        p.next_uid = 8;
        let mut swap = crate::rack::RackSwap::build(&p, &host.engine().slot_uids(), SR);
        // Malformed: uid 7 is neither a fresh slot nor in the engine.
        swap.fresh[6] = None;
        tx.push(Command::SetRack(swap)).unwrap();
        run(&mut host, 2048);
        let flags: Vec<bool> = std::iter::from_fn(|| rx.pop().ok())
            .filter_map(|e| match e {
                Event::Status(s) => Some(s.rack_rejected),
                _ => None,
            })
            .collect();
        assert_eq!(flags.iter().filter(|f| **f).count(), 1);
    }

    #[test]
    fn commands_reach_the_engine() {
        let (mut host, mut tx, _rx) = host_with_demo();
        tx.push(Command::SetBpm(140.0)).unwrap();
        tx.push(Command::Play).unwrap();
        run(&mut host, 512);
        assert!(host.engine().is_playing());
        tx.push(Command::Stop).unwrap();
        run(&mut host, 512);
        assert!(!host.engine().is_playing());
    }

    #[test]
    fn replaced_pattern_comes_back_for_disposal() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::Pattern {
            uid: MachineUid(1),
            index: 0,
            pattern: Box::new(PatternData::Synth(SynthPattern::default())),
        })
        .unwrap();
        run(&mut host, 512);
        let mut found = false;
        while let Ok(ev) = rx.pop() {
            if let Event::DisposePattern(old) = ev {
                let PatternData::Synth(old) = *old else {
                    panic!("the synth pattern comes back")
                };
                assert!(!old.roll.notes.is_empty());
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn replaced_samples_and_play_data_come_back_for_disposal() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let mk = || {
            Some(Arc::new(SampleData {
                sample_rate: 44_100,
                left: vec![0.5; 64].into(),
                right: None,
            }))
        };
        tx.push(Command::SamplerSample {
            uid: MachineUid(6),
            slot: 0,
            sample: mk(),
        })
        .unwrap();
        run(&mut host, 512);
        tx.push(Command::SamplerSample {
            uid: MachineUid(6),
            slot: 0,
            sample: mk(),
        })
        .unwrap();
        tx.push(Command::SamplerPlay {
            uid: MachineUid(6),
            slot: 0,
            play: Box::new(SlotPlay::EMPTY),
        })
        .unwrap();
        run(&mut host, 512);
        let evs: Vec<Event> = std::iter::from_fn(|| rx.pop().ok()).collect();
        let samples = evs
            .iter()
            .filter(|e| matches!(e, Event::DisposeSample(Some(_))))
            .count();
        assert_eq!(samples, 1);
        assert!(evs.iter().any(|e| matches!(e, Event::DisposeSlotPlay(_))));
    }

    #[test]
    fn status_arrives_about_sixty_times_a_second() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::Play).unwrap();
        run(&mut host, SR as usize);
        let mut statuses = Vec::new();
        while let Ok(ev) = rx.pop() {
            if let Event::Status(s) = ev {
                statuses.push(s);
            }
        }
        // 512-sample blocks do not divide the 735-sample cadence: the
        // remainder carries over, so the rate averages 60 Hz.
        assert!((58..=61).contains(&statuses.len()), "{}", statuses.len());
        let last = statuses.last().unwrap();
        assert!(last.playing);
        assert!(last.master_meter > 0.0);
    }

    #[test]
    fn replace_engine_swaps_and_returns_the_old_one() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        tx.push(Command::Play).unwrap();
        run(&mut host, 512);
        tx.push(Command::ReplaceEngine(Box::new(Engine::new(SR))))
            .unwrap();
        run(&mut host, 512);
        assert!(!host.engine().is_playing());
        let disposed =
            std::iter::from_fn(|| rx.pop().ok()).any(|e| matches!(e, Event::DisposeEngine(_)));
        assert!(disposed);
    }

    #[test]
    fn statuses_count_the_stops_taken_and_a_new_engine_starts_at_zero() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let last_status = |rx: &mut rtrb::Consumer<Event>| {
            std::iter::from_fn(|| rx.pop().ok())
                .filter_map(|e| match e {
                    Event::Status(s) => Some(s),
                    _ => None,
                })
                .last()
                .unwrap()
        };
        tx.push(Command::Play).unwrap();
        tx.push(Command::Stop).unwrap();
        tx.push(Command::Stop).unwrap();
        run(&mut host, SR as usize / 10);
        assert_eq!(last_status(&mut rx).stops, 2);
        tx.push(Command::ReplaceEngine(Box::new(Engine::new(SR))))
            .unwrap();
        run(&mut host, SR as usize / 10);
        assert_eq!(last_status(&mut rx).stops, 0);
    }

    #[test]
    fn full_event_ring_does_not_panic() {
        let (mut host, mut tx, _rx) = host_with_demo();
        for i in 0..2000 {
            tx.push(Command::Pattern {
                uid: MachineUid(3),
                index: (i % 128) as u8,
                pattern: Box::new(PatternData::Drum(DrumPattern::default())),
            })
            .unwrap();
        }
        run(&mut host, 512);
    }

    #[test]
    fn disposals_land_even_when_the_ui_never_reads_events() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        run(&mut host, SR as usize * 60);
        tx.push(Command::Pattern {
            uid: MachineUid(1),
            index: 0,
            pattern: Box::new(PatternData::Synth(SynthPattern::default())),
        })
        .unwrap();
        run(&mut host, 512);
        let disposed =
            std::iter::from_fn(|| rx.pop().ok()).any(|e| matches!(e, Event::DisposePattern(_)));
        assert!(disposed);
    }

    #[test]
    fn song_commands_and_status() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        // Four empty 16-step bars.
        let song = CompiledSong {
            end: Some(64),
            ..Default::default()
        };
        tx.push(Command::Song(Box::new(song))).unwrap();
        tx.push(Command::SetMode(crate::sequencer::Mode::Song))
            .unwrap();
        tx.push(Command::SetSongStart(32)).unwrap();
        tx.push(Command::Play).unwrap();
        run(&mut host, 2048);
        let events: Vec<Event> = std::iter::from_fn(|| rx.pop().ok()).collect();
        assert!(events.iter().any(|e| matches!(e, Event::DisposeSong(_))));
        let status = events
            .into_iter()
            .rev()
            .find_map(|e| match e {
                Event::Status(s) => Some(s),
                _ => None,
            })
            .unwrap();
        assert_eq!(status.mode, crate::sequencer::Mode::Song);
        assert_eq!(status.song_bar, 2);
        assert_eq!(status.song_step, 32);
    }

    fn last_status(rx: &mut rtrb::Consumer<Event>) -> Option<Status> {
        std::iter::from_fn(|| rx.pop().ok())
            .filter_map(|e| match e {
                Event::Status(s) => Some(s),
                _ => None,
            })
            .last()
    }

    #[test]
    fn replace_engine_clears_host_state() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        host.guard_fired = true;
        host.master_meter = 1.0;
        host.since_status = host.status_every - 1;
        tx.push(Command::ReplaceEngine(Box::new(Engine::new(SR))))
            .unwrap();
        run(&mut host, 512);
        assert!(
            last_status(&mut rx).is_none(),
            "the status clock restarts with the new engine"
        );
        run(&mut host, SR as usize / 30);
        let first = std::iter::from_fn(|| rx.pop().ok())
            .find_map(|e| match e {
                Event::Status(s) => Some(s),
                _ => None,
            })
            .expect("a status");
        assert!(!first.guard_fired, "no stale fault for the new project");
        assert_eq!(first.master_meter, 0.0, "{first:?}");
    }

    /// Peak of `samples` of host output.
    fn run_peak(host: &mut EngineHost, samples: usize) -> f32 {
        let mut l = vec![0.0; 512];
        let mut r = vec![0.0; 512];
        let mut peak = 0.0f32;
        for _ in 0..samples.div_ceil(512) {
            host.process(&mut l, &mut r);
            peak = l.iter().chain(&r).fold(peak, |p, v| p.max(v.abs()));
        }
        peak
    }

    #[test]
    fn each_command_reaches_the_engine() {
        use crate::params::{MasterParam, SynthParam};
        let (mut host, mut tx, mut rx) = host_with_demo();
        let cutoff = MachineUid(1).param(SynthParam::Cutoff);
        let level = ParamId::Master(MasterParam::Level);

        tx.push(Command::SetParam(cutoff, 0.125)).unwrap();
        tx.push(Command::SetSwing(0.4)).unwrap();
        tx.push(Command::SelectPattern {
            uid: MachineUid(4),
            slot: 7,
        })
        .unwrap();
        tx.push(Command::Touch(level, true)).unwrap();
        tx.push(Command::SetParamLive(level, 0.3)).unwrap();
        run(&mut host, 512);
        assert_eq!(
            host.engine.positions().patterns[3],
            7,
            "SelectPattern while stopped"
        );
        let e = host.engine();
        assert_eq!(e.knob(cutoff), Some(0.125), "SetParam moves the knob");
        assert_eq!(e.live_param(cutoff), 0.125);
        assert_eq!(e.swing(), 0.4, "SetSwing");
        assert!(e.is_touched(level), "Touch(true)");
        assert_eq!(e.live_param(level), 0.3, "SetParamLive plays the value");
        assert_ne!(e.knob(level), Some(0.3), "SetParamLive keeps the knob");
        tx.push(Command::Touch(level, false)).unwrap();
        run(&mut host, 512);
        assert!(!host.engine().is_touched(level), "Touch(false)");

        tx.push(Command::Pattern {
            uid: MachineUid(4),
            index: 0,
            pattern: Box::new(PatternData::Drum(DrumPattern::default())),
        })
        .unwrap();
        tx.push(Command::Pattern {
            uid: MachineUid(5),
            index: 2,
            pattern: Box::new(PatternData::Notes(NotePattern::default())),
        })
        .unwrap();
        run(&mut host, 512);
        let events: Vec<Event> = std::iter::from_fn(|| rx.pop().ok()).collect();
        let old_drums = events.iter().find_map(|e| match e {
            Event::DisposePattern(p) => match &**p {
                PatternData::Drum(p) => Some(p),
                _ => None,
            },
            _ => None,
        });
        assert_eq!(
            old_drums.map(|p| *p
                == crate::demo::demo_project()
                    .kit(MachineUid(4))
                    .unwrap()
                    .patterns[0]),
            Some(true),
            "the replaced drum pattern comes back"
        );
        assert!(
            events.iter().any(
                |e| matches!(e, Event::DisposePattern(p) if matches!(**p, PatternData::Notes(_)))
            ),
            "the replaced note pattern comes back"
        );

        run_peak(&mut host, SR as usize);
        assert!(run_peak(&mut host, 2048) < 1e-4, "stopped and quiet");
        tx.push(Command::NoteOn {
            uid: MachineUid(5),
            pitch: 60,
            vel: 110,
        })
        .unwrap();
        assert!(run_peak(&mut host, 4096) > 0.01, "NoteOn auditions");
        tx.push(Command::NoteOff {
            uid: MachineUid(5),
            pitch: 60,
        })
        .unwrap();
        run_peak(&mut host, 2 * SR as usize);
        assert!(run_peak(&mut host, 2048) < 1e-4, "NoteOff releases");
    }

    #[test]
    fn set_param_live_reaches_the_engine() {
        let (mut host, mut tx, _rx) = host_with_demo();
        let id = ParamId::Master(crate::params::MasterParam::Level);
        tx.push(Command::SetParamLive(id, 0.3)).unwrap();
        run(&mut host, 512);
        assert_eq!(host.engine().live_param(id), 0.3);
    }

    #[test]
    fn touch_holds_a_param_against_automation_until_released() {
        let id = ParamId::Master(crate::params::MasterParam::Level);
        let mut p = crate::demo::demo_project();
        let bar: Vec<_> = crate::machine::standard_rack()
            .iter()
            .map(|m| (m.0, 0))
            .collect();
        p.song = crate::testutil::bar_song(&p, &[bar.clone(), bar]);
        crate::song::write_automation(&mut p.song, id, 0, 31, 0.8);
        let (mut host, mut tx, _rx) = channel(Engine::from_project(SR, &p));
        tx.push(Command::SetMode(Mode::Song)).unwrap();
        tx.push(Command::Touch(id, true)).unwrap();
        tx.push(Command::SetParamLive(id, 0.3)).unwrap();
        tx.push(Command::Play).unwrap();
        run(&mut host, SR as usize / 4);
        assert_eq!(
            host.engine().live_param(id),
            0.3,
            "held: automation skipped"
        );
        tx.push(Command::Touch(id, false)).unwrap();
        run(&mut host, SR as usize / 4);
        assert_eq!(
            host.engine().live_param(id),
            0.8,
            "released: follows automation"
        );
    }

    #[test]
    fn a_rejected_song_is_reported_once() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let song = CompiledSong {
            end: Some(crate::song::MAX_SONG_STEPS as u32 + 1),
            ..Default::default()
        };
        tx.push(Command::Song(Box::new(song))).unwrap();
        run(&mut host, SR as usize / 10);
        let rejected: Vec<bool> = std::iter::from_fn(|| rx.pop().ok())
            .filter_map(|e| match e {
                Event::Status(s) => Some(s.song_rejected),
                _ => None,
            })
            .collect();
        assert!(rejected.len() > 1, "{rejected:?}");
        assert!(rejected[0], "the next status reports the refusal");
        assert!(!rejected[1..].iter().any(|r| *r), "only once: {rejected:?}");
    }

    fn statuses(rx: &mut rtrb::Consumer<Event>) -> Vec<Status> {
        std::iter::from_fn(|| rx.pop().ok())
            .filter_map(|e| match e {
                Event::Status(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn cpu_peak_is_the_worst_load_since_the_previous_status() {
        let (mut host, _tx, mut rx) = host_with_demo();
        host.set_cpu_load(0.2);
        host.set_cpu_load(0.7);
        host.set_cpu_load(0.3);
        run(&mut host, 1024);
        let first = statuses(&mut rx).pop().unwrap();
        assert_eq!(first.cpu_peak, 0.7);
        host.set_cpu_load(0.4);
        run(&mut host, 1024);
        assert_eq!(statuses(&mut rx).last().unwrap().cpu_peak, 0.4);
    }

    #[test]
    fn a_load_over_the_budget_counts_an_overrun_and_nan_is_ignored() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        host.set_cpu_load(0.9);
        host.set_cpu_load(1.0);
        host.set_cpu_load(1.5);
        host.set_cpu_load(f32::NAN);
        host.set_cpu_load(f32::INFINITY);
        host.set_cpu_load(2.0);
        run(&mut host, 1024);
        let s = statuses(&mut rx).pop().unwrap();
        assert_eq!(s.overruns, 2);
        assert_eq!(s.cpu_peak, 2.0);
        // The count runs across engine swaps; the UI keeps its own baseline.
        tx.push(Command::ReplaceEngine(Box::new(Engine::new(SR))))
            .unwrap();
        host.set_cpu_load(1.2);
        run(&mut host, 1024);
        assert_eq!(statuses(&mut rx).last().unwrap().overruns, 3);
    }

    #[test]
    fn statuses_carry_positions_only_with_a_buffer() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        let statuses = |rx: &mut rtrb::Consumer<Event>| {
            let mut v = Vec::new();
            while let Ok(e) = rx.pop() {
                if let Event::Status(s) = e {
                    v.push(s);
                }
            }
            v
        };
        for _ in 0..10 {
            host.process(&mut l, &mut r);
        }
        assert!(statuses(&mut rx).iter().all(|s| s.positions.is_none()));
        tx.push(Command::StatusBuffer(PositionStatus::with_capacity(16)))
            .ok()
            .unwrap();
        for _ in 0..10 {
            host.process(&mut l, &mut r);
        }
        let got = statuses(&mut rx);
        let filled: Vec<_> = got.iter().filter_map(|s| s.positions.as_ref()).collect();
        assert_eq!(filled.len(), 1, "one buffer, one filled status");
        assert_eq!(filled[0].len(), host.engine().slot_uids().len());
    }

    #[test]
    fn a_buffer_too_small_for_the_rack_comes_back_unfilled() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        tx.push(Command::StatusBuffer(PositionStatus::with_capacity(2)))
            .ok()
            .unwrap();
        for _ in 0..10 {
            host.process(&mut l, &mut r);
        }
        let mut disposed = 0;
        while let Ok(e) = rx.pop() {
            match e {
                Event::DisposePositions(_) => disposed += 1,
                Event::Status(s) => assert!(s.positions.is_none()),
                _ => {}
            }
        }
        assert_eq!(disposed, 1);
    }
}
