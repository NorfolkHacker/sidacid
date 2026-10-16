//! `Engine::process` and `EngineHost::process` must never allocate (or
//! free): they run on the audio thread. A counting global allocator records allocations made by this
//! thread while armed.

use acid_engine::engine::Engine;
use acid_engine::host::{self, Command, Event, PatternData};
use acid_engine::machine::MachineUid;
use acid_engine::params::{MachineParam, MasterParam, ParamId};
use acid_engine::project::{
    Block, DrumPattern, Note, NotePattern, PatternRef, Project, SynthPattern,
};
use acid_engine::rack::CompiledSong;
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
const FM: MachineUid = MachineUid(5);
const SAMPLER: MachineUid = MachineUid(6);

/// The demo groove plus chords on the FM and a riff on the sampler, a
/// 4-bar looped song using every machine, and master-level automation.
fn busy_project() -> Project {
    let mut p = acid_engine::demo::demo_project();
    // Every drum instrument, on and off the 1/16 grid, on both kits (the
    // demo's acid lines already slide).
    for uid in [MachineUid(3), MachineUid(4)] {
        for inst in 0..8u8 {
            p.kit_mut(uid).unwrap().patterns[0].roll.notes.push(Note {
                accent: inst % 2 == 0,
                ..Note::new(inst as u16 * 8 + 1, 4, inst, 100)
            });
        }
    }
    for (i, root) in [48u8, 53, 55, 50].into_iter().enumerate() {
        for third in [0, 4, 7] {
            p.fm_mut(MachineUid(5)).unwrap().patterns[0]
                .notes
                .push(Note::new(i as u16 * 16, 14, root + third, 100));
        }
    }
    for k in 0..32u16 {
        p.sampler_mut(MachineUid(6)).unwrap().patterns[0]
            .notes
            .push(Note::new(k * 2, 1, 36 + (k % 12) as u8, 90));
    }
    let tone: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.03).sin() * 0.6).collect();
    let slices: Vec<u32> = (0..12).map(|i| i * 3675).collect();
    p.sampler_mut(MachineUid(6)).unwrap().slots[0] =
        Some(acid_engine::sampler::slot::SlotData::new(
            "Busy",
            44_100,
            1,
            &tone,
            acid_engine::sampler::slot::SlotMeta::sliced(36, slices, 120.0, 44_100),
        ));
    p.set_param(
        MachineUid(6).param(MachineParam::SamplerSlot(
            0,
            acid_engine::params::SlotParam::Choke,
        )),
        1.0,
    );
    p.set_param(
        MachineUid(6).param(acid_engine::params::SamplerParam::Grit),
        1.0,
    );
    // Four 16-step bars: a block per machine and bar.
    p.song.lanes = p
        .machines
        .iter()
        .map(|m| {
            let blocks = (0..4).map(|i| Block::new(i * 16, PatternRef(0))).collect();
            (m.uid, blocks)
        })
        .collect();
    p.song.end = Some(64);
    p.song.loop_range = Some((0, 64));
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

/// `busy_project` grown to `MAX_MACHINES` slots of every type, reordered,
/// each new machine playing its type's busy pattern, all in a looped song
/// with automation on new machines' params.
fn busy_sixteen_slot_project() -> Project {
    use acid_engine::machine::{MAX_MACHINES, MachineUid};
    use acid_engine::params::{ChannelParam, MachineParam};
    use acid_engine::project::{MachineBody, MachineEntry};
    let mut p = busy_project();
    p.machines.swap(0, 3);
    for u in 7..=MAX_MACHINES as u16 {
        // Clone a busy machine of the cycling type under a new uid.
        let from = p.machines[(u as usize - 7) % 6].clone();
        p.machines.push(MachineEntry {
            uid: MachineUid(u),
            name: format!("M{u}"),
            ..from
        });
    }
    p.next_uid = MAX_MACHINES as u16 + 1;
    assert!(
        p.machines
            .iter()
            .any(|m| m.uid.0 > 6 && matches!(m.body, MachineBody::Sampler(_)))
    );
    p.song.lanes = p
        .machines
        .iter()
        .map(|m| {
            let blocks = (0..4)
                .map(|i| Block {
                    transpose: 3,
                    ..Block::new(i * 16, PatternRef(0))
                })
                .collect();
            (m.uid, blocks)
        })
        .collect();
    for u in [8u16, 12, 16] {
        let id = ParamId::Machine(MachineUid(u), MachineParam::Channel(ChannelParam::Level));
        acid_engine::song::write_automation(&mut p.song, id, 0, 40, 0.4);
    }
    p.validate().unwrap();
    p
}

#[test]
fn a_sixteen_slot_song_with_automation_does_not_allocate() {
    let p = busy_sixteen_slot_project();
    let mut e = Engine::from_project(SR, &p);
    let (mut l, mut r) = (vec![0.0; 300], vec![0.0; 300]);
    e.set_mode(Mode::Song);
    e.play();
    let mut song = CompiledSong::compile(&p.song, &p.rack_uids());
    let n = count_allocs(|| {
        render_seconds(&mut e, &mut l, &mut r, 4.0);
        assert!(e.swap_song(&mut song));
        render_seconds(&mut e, &mut l, &mut r, 4.0);
        e.stop();
    });
    assert_eq!(n, 0);
    assert!(l.iter().any(|v| *v != 0.0) || r.iter().any(|v| *v != 0.0));
}

/// The busy project with chance, ratchets and every kind of condition
/// on the acid lines, both kits and the poly machines.
fn chance_project() -> Project {
    use acid_engine::project::Cond;
    let mut p = busy_project();
    p.seed = 1234;
    let conds = [
        Cond::Always,
        Cond::Ratio(1, 2),
        Cond::Ratio(3, 4),
        Cond::First,
        Cond::NotFirst,
        Cond::Fill,
        Cond::NotFill,
        Cond::Pre,
        Cond::NotPre,
    ];
    for uid in 1..=6 {
        let roll = p.roll_mut(MachineUid(uid), 0).unwrap();
        for (i, n) in roll.notes.iter_mut().enumerate() {
            n.chance = [100, 50, 75, 25][i % 4];
            n.ratchet = (i % 8) as u8 + 1;
            n.cond = conds[i % conds.len()];
        }
    }
    p
}

#[test]
fn chance_ratchets_conditions_and_fill_do_not_allocate() {
    for mode in [Mode::Pattern, Mode::Song] {
        let mut e = Engine::from_project(SR, &chance_project());
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        e.set_mode(mode);
        e.play();
        let n = count_allocs(|| {
            render_seconds(&mut e, &mut l, &mut r, 3.0);
            e.set_fill(true);
            e.set_seed(99);
            render_seconds(&mut e, &mut l, &mut r, 3.0);
            e.set_fill(false);
            render_seconds(&mut e, &mut l, &mut r, 2.0);
        });
        assert_eq!(n, 0, "{mode:?}");
    }
}

#[test]
fn sub_tick_ratchets_do_not_allocate() {
    // The densest case: one-tick x8 ratchets (hits an eighth of a tick
    // apart), more of them stacked on a tick than a track follows, on
    // every kind of machine, at the fastest tempo.
    let mut p = busy_project();
    p.bpm = 300.0;
    for uid in 1..=6 {
        let roll = p.roll_mut(MachineUid(uid), 0).unwrap();
        roll.notes.clear();
        for start in [0u16, 1, 5, 9] {
            for k in 0..20u8 {
                roll.notes.push(Note {
                    ratchet: 8 - k % 2,
                    ..Note::new(start, 1 + (k % 3) as u16, 40 + k, 100)
                });
            }
        }
    }
    let mut e = Engine::from_project(SR, &p);
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    e.play();
    let n = count_allocs(|| render_seconds(&mut e, &mut l, &mut r, 3.0));
    assert_eq!(n, 0);
}

#[test]
fn edits_that_move_ringing_notes_do_not_allocate() {
    // Reordered copies of the playing FM and kit patterns, swapped in
    // mid-pass: the played notes and pending ratchets follow them.
    let p = chance_project();
    let mut e = Engine::from_project(SR, &p);
    let mut fm = p.roll(FM, 0).unwrap().clone();
    fm.notes.reverse();
    let mut fm = PatternData::Notes(fm);
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    e.play();
    render_seconds(&mut e, &mut l, &mut r, 0.3);
    let n = count_allocs(|| {
        for _ in 0..4 {
            assert!(e.swap_pattern(FM, 0, &mut fm));
            render_seconds(&mut e, &mut l, &mut r, 0.2);
        }
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
    replacement.notes.push(Note::new(0, 8, 60, 100));
    let mut replacement = PatternData::Notes(replacement);
    e.play();
    let n = count_allocs(|| {
        e.note_on(FM, 72, 110);
        e.note_on(SAMPLER, 40, 110);
        render_seconds(&mut e, &mut l, &mut r, 0.5);
        assert!(e.swap_pattern(FM, 0, &mut replacement));
        e.note_off(FM, 72);
        e.note_off(SAMPLER, 40);
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
    p.sampler_mut(MachineUid(6)).unwrap().slots[0] =
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
            e.note_on(SAMPLER, k, 100);
            render_seconds(&mut e, &mut l, &mut r, 0.01);
        }
        assert!(e.swap_sampler_play(SAMPLER, 0, &mut markers));
        render_seconds(&mut e, &mut l, &mut r, 0.2);
        assert!(e.swap_sampler_play(SAMPLER, 0, &mut zone));
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
    p.sampler_mut(MachineUid(6)).unwrap().slots[0] =
        Some(SlotData::new("Pad", 44_100, 1, &tone, pad));
    // Key 60 plays a 20-frame slice, well inside the 2 ms steal fade.
    let tick = SlotMeta::sliced(60, vec![0, 20], 120.0, 44_100);
    p.sampler_mut(MachineUid(6)).unwrap().slots[1] =
        Some(SlotData::new("Tick", 44_100, 1, &tone, tick));
    let mut e = Engine::from_project(SR, &p);
    let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
    let n = count_allocs(|| {
        // 16 held pad notes fill the voices; each tick steals one.
        for k in 36..52 {
            e.note_on(SAMPLER, k, 100);
        }
        render_seconds(&mut e, &mut l, &mut r, 0.05);
        for _ in 0..4 {
            e.note_on(SAMPLER, 60, 100);
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
            e.set_param(MachineUid(5).param(FmParam::FilterOn), on);
            e.set_param(MachineUid(5).param(FmParam::PhaserMix), on);
            render_seconds(&mut e, &mut l, &mut r, 0.2);
        }
    });
    assert_eq!(n, 0);
}

#[test]
fn a_stepping_mono_arp_does_not_allocate() {
    use acid_engine::machine::MachineType;
    use acid_engine::params::MonoParam as M;
    use acid_engine::project::MachineBody;
    let mut p = busy_project();
    let mut m = p.new_machine(MachineType::Mono);
    for (param, v) in [
        (M::ArpOn, 1.0),
        (M::ArpRate, 0.8), // 1/16T
        (M::ChorusMix, 0.7),
        (M::Drift, 1.0),
        (M::Portamento, 0.5), // On
    ] {
        m.params.insert(MachineParam::Mono(param), v);
    }
    if let MachineBody::Mono(mono) = &mut m.body {
        // Overlapping held notes, so the arp has a chord to step through.
        mono.patterns[0].notes = [(0, 36), (4, 40), (8, 43), (24, 47), (40, 38)]
            .map(|(start, pitch)| Note::new(start, 24, pitch, 100))
            .to_vec();
    }
    let uid = m.uid;
    p.machines.push(m);
    p.validate().unwrap();
    let mut e = Engine::from_project(SR, &p);
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let n = count_allocs(|| {
        e.play();
        render_seconds(&mut e, &mut l, &mut r, 1.0);
        e.set_bpm(97.0);
        render_seconds(&mut e, &mut l, &mut r, 0.5);
        for (on, range) in [(0.0, 0.5), (1.0, 1.0), (0.0, 0.0), (1.0, 0.5)] {
            e.set_param(uid.param(M::ArpOn), on);
            e.set_param(uid.param(M::ArpRange), range);
            render_seconds(&mut e, &mut l, &mut r, 0.3);
        }
        e.set_bpm(160.0);
        render_seconds(&mut e, &mut l, &mut r, 0.5);
        e.stop();
        render_seconds(&mut e, &mut l, &mut r, 0.1);
        e.stop();
        render_seconds(&mut e, &mut l, &mut r, 0.5);
    });
    assert_eq!(n, 0);
}

#[test]
fn cpu_timing_does_not_allocate() {
    let mut e = Engine::from_project(SR, &busy_project());
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    e.play();
    let mut total = 0u64;
    let n = count_allocs(|| {
        for _ in 0..200 {
            e.process(&mut l, &mut r);
            let (slots, shared) = e.take_cpu_ns();
            total += slots.iter().sum::<u64>() + shared;
        }
    });
    assert_eq!(n, 0);
    assert!(total > 0, "the blocks were timed");
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
    replacement.notes.push(Note::new(4, 4, 62, 90));
    let sample = Some(std::sync::Arc::new(
        acid_engine::sampler::sample::SampleData {
            sample_rate: 44_100,
            left: vec![0.25; 256].into(),
            right: None,
        },
    ));
    let commands = [
        Command::Play,
        Command::NoteOn {
            uid: SAMPLER,
            pitch: 60,
            vel: 100,
        },
        Command::Pattern {
            uid: FM,
            index: 0,
            pattern: Box::new(PatternData::Notes(replacement)),
        },
        Command::Pattern {
            uid: MachineUid(1),
            index: 0,
            pattern: Box::new(PatternData::Synth(SynthPattern::default())),
        },
        Command::Pattern {
            uid: MachineUid(3),
            index: 0,
            pattern: Box::new(PatternData::Drum(DrumPattern::default())),
        },
        // Refused (wrong type): the box still goes back to be freed.
        Command::Pattern {
            uid: MachineUid(3),
            index: 1,
            pattern: Box::new(PatternData::Notes(NotePattern::default())),
        },
        Command::NoteOff {
            uid: SAMPLER,
            pitch: 60,
        },
        Command::SamplerSample {
            uid: SAMPLER,
            slot: 1,
            sample,
        },
        Command::SamplerPlay {
            uid: SAMPLER,
            slot: 1,
            play: Box::new(acid_engine::sampler::slot::SlotPlay::EMPTY),
        },
        Command::SetMode(Mode::Song),
        Command::Song(Box::new(CompiledSong::compile(
            &busy_project().song,
            &busy_project().rack_uids(),
        ))),
        Command::Play,
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
    let mut songs = 0;
    let mut sampler_boxes = 0;
    while let Ok(e) = rx.pop() {
        if matches!(e, Event::DisposeSample(_) | Event::DisposeSlotPlay(_)) {
            sampler_boxes += 1;
        }
        if matches!(e, Event::DisposePattern(_)) {
            disposed += 1;
        }
        if matches!(e, Event::DisposeSong(_)) {
            songs += 1;
        }
    }
    assert_eq!(songs, 1, "the replaced song comes back to be freed");
    assert_eq!(
        sampler_boxes, 2,
        "the replaced slot data comes back to be freed"
    );
    assert_eq!(
        disposed, 4,
        "every swapped-out or refused pattern comes back to be freed"
    );
}

#[test]
fn a_rack_swap_through_the_host_does_not_allocate() {
    use acid_engine::machine::MachineType;
    use acid_engine::project::MachineEntry;
    use acid_engine::rack::RackSwap;
    let p = busy_project();
    let (mut host, mut tx, mut rx) = host::channel(Engine::from_project(SR, &p));
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    // Remove the 808, move the FM first, add a Sampler.
    let mut q = p.clone();
    q.machines.retain(|m| m.uid != MachineUid(3));
    let fm = q.machines.remove(3);
    q.machines.insert(0, fm);
    q.machines.push(MachineEntry::new(
        MachineUid(7),
        MachineType::Sampler,
        "S2".into(),
    ));
    q.next_uid = 8;
    q.song.lanes.retain(|(u, _)| *u != MachineUid(3));
    q.validate().unwrap();
    let swap = RackSwap::build(&q, &host.engine().slot_uids(), SR);
    // And a malformed one (a repeated uid), refused without allocating.
    let mut bad = RackSwap::build(&q, &host.engine().slot_uids(), SR);
    bad.order[1] = bad.order[0];
    tx.push(Command::SetMode(Mode::Song)).ok().unwrap();
    tx.push(Command::Play).ok().unwrap();
    let n = count_allocs(|| {
        for _ in 0..50 {
            host.process(&mut l, &mut r);
        }
    });
    assert_eq!(n, 0);
    assert!(tx.push(Command::SetRack(bad)).is_ok());
    assert!(tx.push(Command::SetRack(swap)).is_ok());
    let n = count_allocs(|| {
        for _ in 0..400 {
            host.process(&mut l, &mut r);
        }
    });
    assert_eq!(n, 0);
    assert_eq!(
        host.engine().slot_uids(),
        [5, 1, 2, 4, 6, 7].map(MachineUid).to_vec()
    );
    let mut racks = 0;
    while let Ok(e) = rx.pop() {
        if let Event::DisposeRack(b) = e {
            racks += 1;
            drop(b);
        }
    }
    assert_eq!(racks, 2, "both swaps come back to be freed");
    assert!(l.iter().any(|v| *v != 0.0) || r.iter().any(|v| *v != 0.0));
}

#[test]
fn machines_joining_mid_pattern_do_not_allocate() {
    use acid_engine::machine::MachineType;
    use acid_engine::project::MachineEntry;
    use acid_engine::rack::RackSwap;
    let p = busy_project();
    let (mut host, mut tx, _rx) = host::channel(Engine::from_project(SR, &p));
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let mut q = p.clone();
    for (uid, ty) in [
        (7, MachineType::Synth),
        (8, MachineType::Fm),
        (9, MachineType::Mono),
    ] {
        q.machines
            .push(MachineEntry::new(MachineUid(uid), ty, format!("M{uid}")));
    }
    q.next_uid = 10;
    q.validate().unwrap();
    let swap = RackSwap::build(&q, &host.engine().slot_uids(), SR);
    tx.push(Command::Play).ok().unwrap();
    for _ in 0..77 {
        host.process(&mut l, &mut r);
    }
    assert!(tx.push(Command::SetRack(swap)).is_ok());
    let n = count_allocs(|| {
        for _ in 0..200 {
            host.process(&mut l, &mut r);
        }
    });
    assert_eq!(n, 0);
    assert_eq!(host.engine().slot_uids().len(), q.machines.len());
}
