//! A rack slot: one machine's sound engine, knob storage, patterns,
//! channel strip and render buffer, built from its `MachineEntry`.

use crate::drums::DrumKit;
use crate::engine::{Engine, event_reserve};
use crate::fm::FmSynth;
use crate::machine::{MachineType, MachineUid, rack_capacity};
use crate::mixer::{ChannelStrip, StereoBuf};
use crate::params::MachineParam;
use crate::project::{
    AutoLane, Block, DrumPattern, MachineBody, MachineEntry, NotePattern, Project, Song,
    SynthPattern,
};
use crate::sampler::slot::SlotPlay;
use crate::sampler::{SLOTS, Sampler};
use crate::sequencer::{SeqBuffers, SeqEvent};
use crate::synth::BassSynth;
use crate::va::MonoSynth;
use std::ops::Range;
use std::sync::Arc;

/// The sound engine behind a slot.
// Slots are built once off the audio thread and never moved by value in
// `process`, so the size gap between variants costs nothing.
#[allow(clippy::large_enum_variant)]
pub enum MachineDsp {
    Synth(BassSynth),
    Kit(DrumKit),
    Fm(Box<FmSynth>),
    Mono(Box<MonoSynth>),
    Sampler(Box<Sampler>),
}

/// A slot's pattern bank, by what plays it.
pub enum SlotPatterns {
    Synth(Vec<SynthPattern>),
    Drum(Vec<DrumPattern>),
    Notes(Vec<NotePattern>),
}

pub struct Slot {
    pub uid: MachineUid,
    pub ty: MachineType,
    pub dsp: MachineDsp,
    /// `MachineParam::all_for(ty)`, so knobs re-apply without allocating.
    pub params: Box<[MachineParam]>,
    /// Knob values set through `set_param`, indexed by `MachineParam::index_for`.
    pub knobs: Box<[f32]>,
    /// Effective value per param: the knob, or song automation / live
    /// record value. Lane restores return here.
    pub base: Box<[f32]>,
    /// Params held by the user while recording: automation skips them.
    pub touched: Box<[bool]>,
    pub patterns: SlotPatterns,
    pub strip: ChannelStrip,
    pub buf: StereoBuf,
    /// Post-fader peak since the last take; 0 when muted.
    pub meter: f32,
    /// Render + strip time (ns) since the last take.
    pub cpu_ns: u64,
}

impl Slot {
    /// Builds a slot for `entry` with its params applied and its samples
    /// loaded (allocates: UI thread / construction only).
    pub fn from_entry(entry: &MachineEntry, sample_rate: f32) -> Slot {
        let ty = entry.machine_type();
        let (dsp, patterns) = match &entry.body {
            MachineBody::Synth(m) => (
                MachineDsp::Synth(BassSynth::new(sample_rate)),
                SlotPatterns::Synth(m.patterns.clone()),
            ),
            MachineBody::Kit(m) => (
                MachineDsp::Kit(DrumKit::new(m.kind, sample_rate)),
                SlotPatterns::Drum(m.patterns.clone()),
            ),
            MachineBody::Fm(m) => (
                MachineDsp::Fm(Box::new(FmSynth::new(sample_rate))),
                SlotPatterns::Notes(m.patterns.clone()),
            ),
            MachineBody::Sampler(m) => (
                MachineDsp::Sampler(Box::new(Sampler::new(sample_rate))),
                SlotPatterns::Notes(m.patterns.clone()),
            ),
            MachineBody::Mono(m) => (
                MachineDsp::Mono(Box::new(MonoSynth::new(sample_rate))),
                SlotPatterns::Notes(m.patterns.clone()),
            ),
        };
        let params: Box<[MachineParam]> = MachineParam::all_for(ty).into();
        let knobs: Box<[f32]> = params
            .iter()
            .map(|p| {
                entry
                    .params
                    .get(p)
                    .copied()
                    .filter(|v| v.is_finite())
                    .map_or(p.info().default, |v| v.clamp(0.0, 1.0))
            })
            .collect();
        let mut slot = Slot {
            uid: entry.uid,
            ty,
            dsp,
            base: knobs.clone(),
            touched: vec![false; params.len()].into(),
            knobs,
            params,
            patterns,
            strip: ChannelStrip::new(sample_rate),
            buf: StereoBuf::new(Engine::MAX_BLOCK),
            meter: 0.0,
            cpu_ns: 0,
        };
        for i in 0..slot.params.len() {
            slot.apply(slot.params[i], slot.knobs[i]);
        }
        if let (MachineDsp::Sampler(sampler), MachineBody::Sampler(m)) =
            (&mut slot.dsp, &entry.body)
        {
            for i in 0..SLOTS {
                let data = m.slots.get(i).and_then(Option::as_ref);
                let mut sample = data.map(|d| Arc::new(d.to_sample()));
                sampler.swap_sample(i, &mut sample);
                let mut play = data.map_or(SlotPlay::EMPTY, |d| SlotPlay::from_meta(&d.meta));
                sampler.swap_play(i, &mut play);
            }
        }
        slot
    }

    /// Position of `p` in this slot's knob arrays; `None` if it doesn't fit.
    pub fn index_of(&self, p: MachineParam) -> Option<usize> {
        p.index_for(self.ty)
    }

    /// Sends a value (already clamped) to the DSP or the strip.
    pub fn apply(&mut self, p: MachineParam, v: f32) {
        match (p, &mut self.dsp) {
            (MachineParam::Channel(p), _) => self.strip.set(p, v),
            (MachineParam::Synth(p), MachineDsp::Synth(s)) => s.set_param(p, v),
            (MachineParam::Drum(i, p), MachineDsp::Kit(k)) => k.set_param(i as usize, p, v),
            (MachineParam::Kit(p), MachineDsp::Kit(k)) => k.set_kit_param(p, v),
            (MachineParam::Fm(p), MachineDsp::Fm(f)) => f.set_param(p, v),
            (MachineParam::FmOp(op, p), MachineDsp::Fm(f)) => f.set_op_param(op as usize, p, v),
            (MachineParam::Sampler(p), MachineDsp::Sampler(s)) => s.set_param(p, v),
            (MachineParam::SamplerSlot(i, p), MachineDsp::Sampler(s)) => {
                s.set_slot_param(i as usize, p, v)
            }
            (MachineParam::Mono(p), MachineDsp::Mono(m)) => m.set_param(p, v),
            _ => {}
        }
    }

    /// Renders the DSP into `buf[span]`.
    pub fn render(&mut self, span: Range<usize>) {
        let (l, r) = (&mut self.buf.l[span.clone()], &mut self.buf.r[span]);
        match &mut self.dsp {
            MachineDsp::Synth(s) => s.render(l, r),
            MachineDsp::Kit(k) => k.render(l, r),
            MachineDsp::Fm(f) => f.render(l, r),
            MachineDsp::Sampler(s) => s.render(l, r),
            MachineDsp::Mono(m) => m.render(l, r),
        }
    }

    /// Clears the DSP's voices and filter state (NaN guard).
    pub fn reset(&mut self) {
        match &mut self.dsp {
            MachineDsp::Synth(s) => s.reset(),
            MachineDsp::Kit(k) => k.reset(),
            MachineDsp::Fm(f) => f.reset(),
            MachineDsp::Sampler(s) => s.reset(),
            MachineDsp::Mono(m) => m.reset(),
        }
    }
}

/// A song whose lanes are resolved to rack positions: what the sequencer
/// plays. Built on the UI thread for one rack order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompiledSong {
    /// Per rack position: that machine's blocks, sorted by start.
    pub lanes: Vec<Vec<Block>>,
    pub end: Option<u32>,
    pub loop_range: Option<(u32, u32)>,
    pub automation: Vec<AutoLane>,
}

impl CompiledSong {
    /// Resolves uid lanes to rack positions (UI thread; allocates). Lanes
    /// of machines not in `rack` are dropped.
    pub fn compile(song: &Song, rack: &[MachineUid]) -> CompiledSong {
        CompiledSong {
            lanes: rack.iter().map(|&uid| song.lane(uid).to_vec()).collect(),
            end: song.end,
            loop_range: song.loop_range,
            automation: song.automation.clone(),
        }
    }

    /// The blocks at rack position `pos`; empty past the rack.
    pub fn lane(&self, pos: usize) -> &[Block] {
        self.lanes.get(pos).map_or(&[], Vec::as_slice)
    }
}

/// A bigger rack capacity for the engine, built on the UI thread: the
/// sequencer's per-position storage and the event buffer. After the swap
/// it holds the engine's old ones, for disposal.
pub struct Growth {
    pub capacity: usize,
    pub(crate) seq: SeqBuffers,
    /// Empty, with room for `event_reserve(capacity)` events.
    pub(crate) events: Vec<SeqEvent>,
}

impl Growth {
    /// Allocates.
    pub fn new(capacity: usize) -> Box<Growth> {
        Box::new(Growth {
            capacity,
            seq: SeqBuffers::new(capacity),
            events: Vec::with_capacity(event_reserve(capacity)),
        })
    }
}

/// A rack edit for the audio thread (`Command::SetRack`): everything the
/// new rack needs that the engine doesn't already hold, built on the UI
/// thread. After `Engine::set_rack` it carries what left the engine (the
/// old slot `Vec` in `into`, unused `fresh` slots, the old song) back for
/// disposal.
pub struct RackSwap {
    /// New rack order, at most `capacity`.
    pub order: Vec<MachineUid>,
    /// Same length as `order`: a new slot, or `None` to move the slot the
    /// engine already has for that uid.
    pub fresh: Vec<Option<Slot>>,
    /// Empty, with room for `capacity` slots: the new rack is moved into it.
    pub into: Vec<Slot>,
    /// The song compiled for `order`.
    pub song: Box<CompiledSong>,
    /// Pattern selection per new position; taken (at once, not queued)
    /// by fresh slots only. Surviving slots keep their track state.
    /// Same length as `order`.
    pub selected: Vec<u8>,
    /// Same length as `order`; filled by the engine: where each new
    /// position's slot came from (its old position), `None` for a fresh slot.
    pub map: Vec<Option<usize>>,
    /// Same length as `order`; engine scratch: each new position's type.
    pub tys: Vec<Option<MachineType>>,
    /// The engine's rack capacity after this swap.
    pub capacity: usize,
    /// The storage for `capacity`, when that is more than the engine
    /// holds now; afterwards the engine's old storage.
    pub growth: Option<Box<Growth>>,
}

impl RackSwap {
    /// UI-thread helper: builds a swap from the project (after a rack
    /// edit) and the uids the engine has now, for an engine holding
    /// `engine_capacity` positions (grown if the new rack needs more).
    /// Only machines not in `current` get new slots. Allocates.
    pub fn build(
        project: &Project,
        current: &[MachineUid],
        sample_rate: f32,
        engine_capacity: usize,
    ) -> Box<RackSwap> {
        let machines = &project.machines;
        let order: Vec<MachineUid> = machines.iter().map(|m| m.uid).collect();
        let fresh = machines
            .iter()
            .map(|m| (!current.contains(&m.uid)).then(|| Slot::from_entry(m, sample_rate)))
            .collect();
        let selected = machines.iter().map(|m| m.selected).collect();
        let n = order.len();
        let capacity = rack_capacity(n).max(engine_capacity);
        let growth = (capacity > engine_capacity).then(|| Growth::new(capacity));
        let song = Box::new(CompiledSong::compile(&project.song, &order));
        Box::new(RackSwap {
            order,
            fresh,
            into: Vec::with_capacity(capacity),
            song,
            selected,
            map: vec![None; n],
            tys: vec![None; n],
            capacity,
            growth,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::PatternRef;

    #[test]
    fn songs_compile_to_rack_positions() {
        let mut song = Song::default();
        let block = Block {
            mute: true,
            transpose: 2,
            ..Block::new(16, PatternRef(3))
        };
        song.lane_mut(MachineUid(5)).push(block);
        song.lane_mut(MachineUid(9))
            .push(Block::new(0, PatternRef(1)));
        song.end = Some(64);
        song.loop_range = Some((16, 32));
        let c = CompiledSong::compile(&song, &[MachineUid(5), MachineUid(1)]);
        assert_eq!(
            c.lanes,
            vec![vec![block], vec![]],
            "uid 9 isn't in the rack"
        );
        assert_eq!(c.lane(0), [block]);
        assert!(c.lane(7).is_empty());
        assert_eq!((c.end, c.loop_range), (Some(64), Some((16, 32))));
    }
}
