//! Real-time bridge between the UI thread and the audio thread. The UI
//! pushes `Command`s; the audio callback calls `EngineHost::process`,
//! which applies them, renders, and pushes back `Event`s: a ~60 Hz
//! `Status`, plus every replaced pattern or engine so that memory is
//! freed on the UI thread, never on the audio thread.

use crate::engine::Engine;
use crate::machine::{MAX_MACHINES, MachineUid};
use crate::mixer::remap_meters;
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
const CPU_SMOOTHING: f32 = 0.1;

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
    SetSongStart(u16),
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    pub playing: bool,
    /// By rack position.
    pub playhead: [usize; MAX_MACHINES],
    /// By rack position.
    pub patterns: [u8; MAX_MACHINES],
    /// By rack position.
    pub queued: [Option<u8>; MAX_MACHINES],
    /// Peaks since the previous status: rack positions 0..MAX_MACHINES,
    /// then master. Muted (or soloed-out) machines read 0.
    pub meters: [f32; MAX_MACHINES + 1],
    /// Smoothed fraction of the callback's time budget spent rendering.
    pub cpu: f32,
    /// Worst single callback load since the previous status.
    pub cpu_peak: f32,
    /// Callbacks whose render took longer than their time budget, counted
    /// for the life of the stream (wrapping); engine swaps do not reset it.
    pub overruns: u32,
    pub guard_fired: bool,
    pub mode: Mode,
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
    /// never reset). Per-position arrays describe that rack.
    pub rack_gen: u32,
}

// Events travel by value through a preallocated ring: boxing the Status
// would allocate on the audio thread.
#[allow(clippy::large_enum_variant)]
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
}

pub struct EngineHost {
    engine: Box<Engine>,
    commands: Consumer<Command>,
    events: Producer<Event>,
    status_every: usize,
    since_status: usize,
    meters: [f32; MAX_MACHINES + 1],
    guard_fired: bool,
    song_rejected: bool,
    rack_rejected: bool,
    /// Racks taken: accepted `SetRack`s plus `ReplaceEngine`s (wrapping).
    rack_gen: u32,
    cpu: f32,
    cpu_peak: f32,
    overruns: u32,
    stops: u32,
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
        meters: [0.0; MAX_MACHINES + 1],
        guard_fired: false,
        song_rejected: false,
        rack_rejected: false,
        rack_gen: 0,
        cpu: 0.0,
        cpu_peak: 0.0,
        overruns: 0,
        stops: 0,
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
        let meters = self.engine.take_meters();
        for (acc, m) in self.meters.iter_mut().zip(&meters) {
            *acc = acc.max(*m);
        }
        self.guard_fired |= self.engine.take_guard_fired();
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
                    self.meters = [0.0; MAX_MACHINES + 1];
                    self.guard_fired = false;
                    self.song_rejected = false;
                    self.rack_rejected = false;
                    self.rack_gen = self.rack_gen.wrapping_add(1);
                    self.since_status = 0;
                    self.stops = 0;
                }
                Command::SetMode(mode) => self.engine.set_mode(mode),
                Command::Song(mut song) => {
                    self.song_rejected |= !self.engine.swap_song(&mut song);
                    self.dispose(Event::DisposeSong(song));
                }
                Command::SetSongStart(bar) => self.engine.set_song_start(bar as usize),
                Command::Touch(id, on) => self.engine.touch(id, on),
                Command::SetParamLive(id, v) => self.engine.set_param_live(id, v),
                Command::SetRack(mut swap) => {
                    match self.engine.set_rack(&mut swap) {
                        Some(map) => {
                            remap_meters(&mut self.meters, &map);
                            self.rack_gen = self.rack_gen.wrapping_add(1);
                        }
                        None => self.rack_rejected = true,
                    }
                    self.dispose(Event::DisposeRack(swap));
                }
                Command::NoteOn { uid, pitch, vel } => self.engine.note_on(uid, pitch, vel),
                Command::NoteOff { uid, pitch } => self.engine.note_off(uid, pitch),
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
        let status = Status {
            playing: self.engine.is_playing(),
            playhead: self.engine.playhead(),
            patterns: self.engine.current_patterns(),
            queued: self.engine.queued_patterns(),
            meters: self.meters,
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
        };
        if self.events.slots() > DISPOSAL_RESERVE && self.events.push(Event::Status(status)).is_ok()
        {
            self.meters = [0.0; MAX_MACHINES + 1];
            self.cpu_peak = 0.0;
            self.guard_fired = false;
            self.song_rejected = false;
            self.rack_rejected = false;
        }
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
                assert!(
                    old.steps
                        .iter()
                        .any(|s| s.gate != crate::project::Gate::Off)
                );
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
        assert!(last.meters[MAX_MACHINES] > 0.0);
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
        let song = CompiledSong {
            bars: vec![Default::default(); 4],
            ..Default::default()
        };
        tx.push(Command::Song(Box::new(song))).unwrap();
        tx.push(Command::SetMode(crate::sequencer::Mode::Song))
            .unwrap();
        tx.push(Command::SetSongStart(2)).unwrap();
        tx.push(Command::Play).unwrap();
        run(&mut host, 2048);
        let events: Vec<Event> = std::iter::from_fn(|| rx.pop().ok()).collect();
        assert!(events.iter().any(|e| matches!(e, Event::DisposeSong(_))));
        let status = events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Status(s) => Some(*s),
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
        host.meters = [1.0; MAX_MACHINES + 1];
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
        assert!(first.meters.iter().all(|m| *m == 0.0), "{first:?}");
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
        let e = host.engine();
        assert_eq!(e.knob(cutoff), Some(0.125), "SetParam moves the knob");
        assert_eq!(e.live_param(cutoff), 0.125);
        assert_eq!(e.swing(), 0.4, "SetSwing");
        assert_eq!(e.current_patterns()[3], 7, "SelectPattern while stopped");
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
        let bar = crate::project::Bar {
            cells: crate::machine::standard_rack()
                .iter()
                .map(|m| {
                    let pattern = Some(crate::project::PatternRef(0));
                    (
                        m.0,
                        crate::project::Cell {
                            pattern,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        };
        p.song.bars = vec![bar.clone(), bar];
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
            bars: vec![Default::default(); crate::song::MAX_BARS + 1],
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
        let first = *statuses(&mut rx).last().unwrap();
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
        let s = *statuses(&mut rx).last().unwrap();
        assert_eq!(s.overruns, 2);
        assert_eq!(s.cpu_peak, 2.0);
        // The count runs across engine swaps; the UI keeps its own baseline.
        tx.push(Command::ReplaceEngine(Box::new(Engine::new(SR))))
            .unwrap();
        host.set_cpu_load(1.2);
        run(&mut host, 1024);
        assert_eq!(statuses(&mut rx).last().unwrap().overruns, 3);
    }
}
