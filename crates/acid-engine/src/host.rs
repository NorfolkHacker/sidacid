//! Real-time bridge between the UI thread and the audio thread. The UI
//! pushes `Command`s; the audio callback calls `EngineHost::process`,
//! which applies them, renders, and pushes back `Event`s: a ~60 Hz
//! `Status`, plus every replaced pattern or engine so that memory is
//! freed on the UI thread, never on the audio thread.

use crate::engine::Engine;
use crate::machine::MACHINES;
use crate::params::{CHANNELS, ParamId};
use crate::project::{DrumPattern, NotePattern, Song, SynthPattern};
use crate::sequencer::Mode;
use rtrb::{Consumer, Producer, PushError, RingBuffer};

pub const COMMAND_CAPACITY: usize = 4096;
pub const EVENT_CAPACITY: usize = 1024;
const STATUS_RATE_HZ: f32 = 60.0;
/// Event slots kept free for disposals: statuses never use the last ones,
/// so a stalled UI cannot make replaced boxes leak.
const DISPOSAL_RESERVE: usize = 256;
/// Weight of the newest CPU-load sample in the smoothed value.
const CPU_SMOOTHING: f32 = 0.1;

pub enum Command {
    SetParam(ParamId, f32),
    SetBpm(f32),
    SetSwing(f32),
    Play,
    Stop,
    SelectPattern {
        machine: u8,
        slot: u8,
    },
    SynthPattern {
        synth: u8,
        slot: u8,
        pattern: Box<SynthPattern>,
    },
    DrumPattern {
        kit: u8,
        slot: u8,
        pattern: Box<DrumPattern>,
    },
    NotePattern {
        machine: u8,
        slot: u8,
        pattern: Box<NotePattern>,
    },
    ReplaceEngine(Box<Engine>),
    SetMode(Mode),
    Song(Box<Song>),
    SetSongStart(u16),
    /// The user is holding (true) or released (false) a knob while recording.
    Touch(ParamId, bool),
    /// Sound follows the knob without changing its saved value (record mode).
    SetParamLive(ParamId, f32),
    /// Audition: play a note on a note machine now.
    NoteOn {
        machine: u8,
        pitch: u8,
        vel: u8,
    },
    NoteOff {
        machine: u8,
        pitch: u8,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Status {
    pub playing: bool,
    pub playhead: [usize; MACHINES],
    pub patterns: [u8; MACHINES],
    pub queued: [Option<u8>; MACHINES],
    /// Peaks since the previous status: channels 0..CHANNELS, then master.
    pub meters: [f32; CHANNELS + 1],
    /// Smoothed fraction of the callback's time budget spent rendering.
    pub cpu: f32,
    pub guard_fired: bool,
    pub mode: Mode,
    pub song_bar: u16,
    /// Global song step (song mode).
    pub song_step: u32,
}

pub enum Event {
    Status(Status),
    DisposeSynthPattern(Box<SynthPattern>),
    DisposeDrumPattern(Box<DrumPattern>),
    DisposeNotePattern(Box<NotePattern>),
    DisposeEngine(Box<Engine>),
    DisposeSong(Box<Song>),
}

pub struct EngineHost {
    engine: Box<Engine>,
    commands: Consumer<Command>,
    events: Producer<Event>,
    status_every: usize,
    since_status: usize,
    meters: [f32; CHANNELS + 1],
    guard_fired: bool,
    cpu: f32,
}

/// Wires an engine to a fresh pair of rings. Call on the UI thread; move
/// the `EngineHost` into the audio callback.
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
        meters: [0.0; CHANNELS + 1],
        guard_fired: false,
        cpu: 0.0,
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
        }
    }

    /// Applies pending commands, renders, and reports. Real-time safe.
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        self.drain_commands();
        self.engine.process(out_l, out_r);
        for (acc, m) in self.meters.iter_mut().zip(self.engine.take_meters()) {
            *acc = acc.max(m);
        }
        self.guard_fired |= self.engine.take_guard_fired();
        self.since_status += out_l.len();
        if self.since_status >= self.status_every {
            self.since_status = 0;
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
                Command::Stop => self.engine.stop(),
                Command::SelectPattern { machine, slot } => {
                    self.engine.select_pattern(machine as usize, slot)
                }
                Command::SynthPattern {
                    synth,
                    slot,
                    mut pattern,
                } => {
                    self.engine
                        .swap_synth_pattern(synth as usize, slot, &mut pattern);
                    self.dispose(Event::DisposeSynthPattern(pattern));
                }
                Command::DrumPattern {
                    kit,
                    slot,
                    mut pattern,
                } => {
                    self.engine
                        .swap_drum_pattern(kit as usize, slot, &mut pattern);
                    self.dispose(Event::DisposeDrumPattern(pattern));
                }
                Command::NotePattern {
                    machine,
                    slot,
                    mut pattern,
                } => {
                    self.engine
                        .swap_note_pattern(machine as usize, slot, &mut pattern);
                    self.dispose(Event::DisposeNotePattern(pattern));
                }
                Command::ReplaceEngine(mut engine) => {
                    std::mem::swap(&mut self.engine, &mut engine);
                    self.dispose(Event::DisposeEngine(engine));
                }
                Command::SetMode(mode) => self.engine.set_mode(mode),
                Command::Song(mut song) => {
                    self.engine.swap_song(&mut song);
                    self.dispose(Event::DisposeSong(song));
                }
                Command::SetSongStart(bar) => self.engine.set_song_start(bar as usize),
                Command::Touch(id, on) => self.engine.touch(id, on),
                Command::SetParamLive(id, v) => self.engine.set_param_live(id, v),
                Command::NoteOn {
                    machine,
                    pitch,
                    vel,
                } => self.engine.note_on(machine as usize, pitch, vel),
                Command::NoteOff { machine, pitch } => {
                    self.engine.note_off(machine as usize, pitch)
                }
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
            guard_fired: self.guard_fired,
            mode: self.engine.mode(),
            song_bar: self.engine.song_bar().min(u16::MAX as usize) as u16,
            song_step: self.engine.song_step(),
        };
        if self.events.slots() > DISPOSAL_RESERVE && self.events.push(Event::Status(status)).is_ok()
        {
            self.meters = [0.0; CHANNELS + 1];
            self.guard_fired = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        tx.push(Command::SynthPattern {
            synth: 0,
            slot: 0,
            pattern: Box::new(SynthPattern::default()),
        })
        .unwrap();
        run(&mut host, 512);
        let mut found = false;
        while let Ok(ev) = rx.pop() {
            if let Event::DisposeSynthPattern(old) = ev {
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
        assert!((40..=61).contains(&statuses.len()), "{}", statuses.len());
        let last = statuses.last().unwrap();
        assert!(last.playing);
        assert!(last.meters[CHANNELS] > 0.0);
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
    fn full_event_ring_does_not_panic() {
        let (mut host, mut tx, _rx) = host_with_demo();
        for i in 0..2000 {
            tx.push(Command::DrumPattern {
                kit: 0,
                slot: (i % 128) as u8,
                pattern: Box::new(DrumPattern::default()),
            })
            .unwrap();
        }
        run(&mut host, 512);
    }

    #[test]
    fn disposals_land_even_when_the_ui_never_reads_events() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        run(&mut host, SR as usize * 60);
        tx.push(Command::SynthPattern {
            synth: 0,
            slot: 0,
            pattern: Box::new(SynthPattern::default()),
        })
        .unwrap();
        run(&mut host, 512);
        let disposed = std::iter::from_fn(|| rx.pop().ok())
            .any(|e| matches!(e, Event::DisposeSynthPattern(_)));
        assert!(disposed);
    }

    #[test]
    fn song_commands_and_status() {
        let (mut host, mut tx, mut rx) = host_with_demo();
        let song = crate::project::Song {
            bars: vec![crate::project::Bar::default(); 4],
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
}
