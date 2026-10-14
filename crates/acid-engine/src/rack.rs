//! A rack slot: one machine's sound engine, knob storage, patterns,
//! channel strip and render buffer, built from its `MachineEntry`.

use crate::drums::DrumKit;
use crate::engine::Engine;
use crate::fm::FmSynth;
use crate::machine::{MachineType, MachineUid};
use crate::mixer::{ChannelStrip, StereoBuf};
use crate::params::MachineParam;
use crate::project::{DrumPattern, MachineBody, MachineEntry, NotePattern, SynthPattern};
use crate::sampler::slot::SlotPlay;
use crate::sampler::{SLOTS, Sampler};
use crate::synth::BassSynth;
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
        }
    }

    /// Clears the DSP's voices and filter state (NaN guard).
    pub fn reset(&mut self) {
        match &mut self.dsp {
            MachineDsp::Synth(s) => s.reset(),
            MachineDsp::Kit(k) => k.reset(),
            MachineDsp::Fm(f) => f.reset(),
            MachineDsp::Sampler(s) => s.reset(),
        }
    }
}
