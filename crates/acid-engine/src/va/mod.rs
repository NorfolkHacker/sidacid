//! The Mono machine: a monophonic virtual-analog synth (VCO with sub
//! osc and noise, 24 dB ladder, one ADSR, LFO, portamento, drift,
//! chorus) with a tempo-synced arpeggiator.

pub mod adsr;
pub mod ladder;
pub mod lfo;
pub mod osc;
pub mod patch;

mod synth;

pub use synth::MonoSynth;
