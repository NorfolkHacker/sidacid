//! `Engine::process` and `EngineHost::process` must never allocate (or
//! free): they run on the audio thread. A counting global allocator records allocations made by this
//! thread while armed.

use acid_engine::engine::Engine;
use acid_engine::host::{self, Command, Event};
use acid_engine::params::{MasterParam, ParamId};
use acid_engine::project::{Bar, Note, NotePattern, PatternRef, Project};
use acid_engine::sequencer::Mode;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn note_alloc() {
    let _ = ARMED.try_with(|armed| {
        if armed.get() {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_alloc();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note_alloc();
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_alloc();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Runs `f` and returns how many allocations this thread made in it.
fn count_allocs(f: impl FnOnce()) -> usize {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    COUNT.with(|c| c.get())
}

const SR: f32 = 48_000.0;

/// The demo groove plus chords on the FM and a riff on the sampler, a
/// 4-bar looped song using every machine, and master-level automation.
fn busy_project() -> Project {
    let mut p = acid_engine::demo::demo_project();
    for (i, root) in [48u8, 53, 55, 50].into_iter().enumerate() {
        for third in [0, 4, 7] {
            p.fm.patterns[0].notes.push(Note {
                start: i as u16 * 16,
                len: 14,
                pitch: root + third,
                vel: 100,
            });
        }
    }
    for k in 0..32u16 {
        p.sampler.patterns[0].notes.push(Note {
            start: k * 2,
            len: 1,
            pitch: 36 + (k % 12) as u8,
            vel: 90,
        });
    }
    let bar = Bar {
        patterns: [Some(PatternRef(0)); acid_engine::machine::MACHINES],
        ..Bar::default()
    };
    p.song.bars = vec![bar; 4];
    p.song.loop_range = Some((0, 3));
    acid_engine::song::write_automation(
        &mut p.song,
        ParamId::Master(MasterParam::Level),
        0,
        63,
        0.6,
    );
    p
}

fn render_seconds(e: &mut Engine, l: &mut [f32], r: &mut [f32], seconds: f32) {
    let blocks = (seconds * SR / l.len() as f32) as usize;
    for _ in 0..blocks {
        e.process(l, r);
    }
}

#[test]
fn pattern_mode_does_not_allocate() {
    let mut e = Engine::from_project(SR, &busy_project());
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    e.play();
    let n = count_allocs(|| render_seconds(&mut e, &mut l, &mut r, 6.0));
    assert_eq!(n, 0);
}

#[test]
fn song_mode_with_automation_does_not_allocate() {
    let mut e = Engine::from_project(SR, &busy_project());
    let (mut l, mut r) = (vec![0.0; 333], vec![0.0; 333]);
    e.set_mode(Mode::Song);
    e.play();
    let n = count_allocs(|| render_seconds(&mut e, &mut l, &mut r, 10.0));
    assert_eq!(n, 0);
}

#[test]
fn audition_and_pattern_swaps_do_not_allocate() {
    let mut e = Engine::from_project(SR, &busy_project());
    let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
    let mut replacement = NotePattern::default();
    replacement.notes.push(Note {
        start: 0,
        len: 8,
        pitch: 60,
        vel: 100,
    });
    e.play();
    let n = count_allocs(|| {
        e.note_on(4, 72, 110);
        e.note_on(5, 40, 110);
        render_seconds(&mut e, &mut l, &mut r, 0.5);
        assert!(e.swap_note_pattern(4, 0, &mut replacement));
        e.note_off(4, 72);
        e.note_off(5, 40);
        render_seconds(&mut e, &mut l, &mut r, 1.0);
        e.stop();
        render_seconds(&mut e, &mut l, &mut r, 0.5);
    });
    assert_eq!(n, 0);
    drop(replacement);
}

#[test]
fn the_counter_sees_allocations() {
    let n = count_allocs(|| {
        drop(std::hint::black_box(Vec::<u8>::with_capacity(16)));
    });
    assert!(n >= 1, "positive control: {n}");
}

#[test]
fn host_commands_and_rendering_do_not_allocate() {
    let (mut host, mut tx, mut rx) = host::channel(Engine::from_project(SR, &busy_project()));
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let mut replacement = NotePattern::default();
    replacement.notes.push(Note {
        start: 4,
        len: 4,
        pitch: 62,
        vel: 90,
    });
    let commands = [
        Command::Play,
        Command::NoteOn {
            machine: 5,
            pitch: 60,
            vel: 100,
        },
        Command::NotePattern {
            machine: 4,
            slot: 0,
            pattern: Box::new(replacement),
        },
        Command::NoteOff {
            machine: 5,
            pitch: 60,
        },
    ];
    for c in commands {
        assert!(tx.push(c).is_ok());
    }
    let n = count_allocs(|| {
        for _ in 0..400 {
            host.process(&mut l, &mut r);
        }
    });
    assert_eq!(n, 0);
    let mut disposed = 0;
    while let Ok(e) = rx.pop() {
        if matches!(e, Event::DisposeNotePattern(_)) {
            disposed += 1;
        }
    }
    assert_eq!(
        disposed, 1,
        "the swapped-out pattern comes back to be freed"
    );
}
