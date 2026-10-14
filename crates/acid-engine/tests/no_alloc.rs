//! `Engine::process` and `EngineHost::process` must never allocate (or
//! free): they run on the audio thread. A counting global allocator records allocations made by this
//! thread while armed.

use acid_engine::engine::Engine;
use acid_engine::host::{self, Command, Event};
use acid_engine::machine::MachineId;
use acid_engine::params::{MasterParam, ParamId, fm_param, sampler_param, slot_param};
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
            p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
                .notes
                .push(Note {
                    start: i as u16 * 16,
                    len: 14,
                    pitch: root + third,
                    vel: 100,
                });
        }
    }
    for k in 0..32u16 {
        p.sampler_mut(MachineId::Sampler.uid()).unwrap().patterns[0]
            .notes
            .push(Note {
                start: k * 2,
                len: 1,
                pitch: 36 + (k % 12) as u8,
                vel: 90,
            });
    }
    let tone: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.03).sin() * 0.6).collect();
    let slices: Vec<u32> = (0..12).map(|i| i * 3675).collect();
    p.sampler_mut(MachineId::Sampler.uid()).unwrap().slots[0] =
        Some(acid_engine::sampler::slot::SlotData::new(
            "Busy",
            44_100,
            1,
            &tone,
            acid_engine::sampler::slot::SlotMeta::sliced(36, slices, 120.0, 44_100),
        ));
    p.set_param(slot_param(0, acid_engine::params::SlotParam::Choke), 1.0);
    p.set_param(sampler_param(acid_engine::params::SamplerParam::Grit), 1.0);
    let bar = Bar {
        cells: MachineId::ALL
            .iter()
            .map(|m| {
                let pattern = Some(PatternRef(0));
                (
                    m.uid(),
                    acid_engine::project::Cell {
                        pattern,
                        ..Default::default()
                    },
                )
            })
            .collect(),
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
fn a_sixteen_slot_rack_does_not_allocate() {
    use acid_engine::machine::{MAX_MACHINES, MachineType, MachineUid};
    use acid_engine::project::MachineEntry;
    let mut p = busy_project();
    p.machines.swap(0, 3);
    let types = MachineType::ALL.iter().cycle();
    for (u, &ty) in (7..=MAX_MACHINES as u16).zip(types) {
        p.machines
            .push(MachineEntry::new(MachineUid(u), ty, format!("M{u}")));
    }
    p.next_uid = MAX_MACHINES as u16 + 1;
    p.validate().unwrap();
    let mut e = Engine::from_project(SR, &p);
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    e.play();
    let n = count_allocs(|| {
        e.set_param(ParamId::Master(MasterParam::Level), 0.5);
        render_seconds(&mut e, &mut l, &mut r, 2.0);
        e.stop();
    });
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
fn sampler_steals_and_marker_edits_do_not_allocate() {
    use acid_engine::sampler::slot::{SlotData, SlotMeta, SlotPlay};
    let mut p = Project::default();
    let tone: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.03).sin() * 0.6).collect();
    let mut meta = SlotMeta::chromatic(36, 96, 60, 44_100);
    meta.loop_on = true;
    meta.loop_start = 10_000;
    meta.loop_end = 30_000;
    p.sampler_mut(MachineId::Sampler.uid()).unwrap().slots[0] =
        Some(SlotData::new("Pad", 44_100, 1, &tone, meta.clone()));
    let mut e = Engine::from_project(SR, &p);
    let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
    meta.loop_end = 20_000;
    let mut markers = SlotPlay::from_meta(&meta);
    meta.base_key = 40;
    let mut zone = SlotPlay::from_meta(&meta);
    let n = count_allocs(|| {
        // 20 held notes on 16 voices: the last four steal
        for k in 40..60 {
            e.note_on(5, k, 100);
            render_seconds(&mut e, &mut l, &mut r, 0.01);
        }
        assert!(e.swap_sampler_play(0, &mut markers));
        render_seconds(&mut e, &mut l, &mut r, 0.2);
        assert!(e.swap_sampler_play(0, &mut zone));
        render_seconds(&mut e, &mut l, &mut r, 0.1);
    });
    assert_eq!(n, 0);
}

#[test]
fn a_steal_by_a_note_shorter_than_its_fade_does_not_allocate() {
    use acid_engine::sampler::slot::{SlotData, SlotMeta};
    let mut p = Project::default();
    let tone: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.03).sin() * 0.6).collect();
    let pad = SlotMeta::chromatic(36, 59, 48, 44_100);
    p.sampler_mut(MachineId::Sampler.uid()).unwrap().slots[0] =
        Some(SlotData::new("Pad", 44_100, 1, &tone, pad));
    // Key 60 plays a 20-frame slice, well inside the 2 ms steal fade.
    let tick = SlotMeta::sliced(60, vec![0, 20], 120.0, 44_100);
    p.sampler_mut(MachineId::Sampler.uid()).unwrap().slots[1] =
        Some(SlotData::new("Tick", 44_100, 1, &tone, tick));
    let mut e = Engine::from_project(SR, &p);
    let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
    let n = count_allocs(|| {
        // 16 held pad notes fill the voices; each tick steals one.
        for k in 36..52 {
            e.note_on(5, k, 100);
        }
        render_seconds(&mut e, &mut l, &mut r, 0.05);
        for _ in 0..4 {
            e.note_on(5, 60, 100);
            render_seconds(&mut e, &mut l, &mut r, 0.01);
        }
    });
    assert_eq!(n, 0);
}

#[test]
fn toggling_fm_effects_does_not_allocate() {
    use acid_engine::params::FmParam;
    let mut e = Engine::from_project(SR, &busy_project());
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    e.play();
    let n = count_allocs(|| {
        for on in [1.0, 0.0, 1.0, 0.0, 1.0] {
            e.set_param(fm_param(FmParam::FilterOn), on);
            e.set_param(fm_param(FmParam::PhaserMix), on);
            render_seconds(&mut e, &mut l, &mut r, 0.2);
        }
    });
    assert_eq!(n, 0);
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
