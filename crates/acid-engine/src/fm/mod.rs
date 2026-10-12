//! Six-operator FM synth in the style of the DX7 / M-VAVE FM-1:
//! 32 algorithms, DX7 envelopes and scaling, LFO, pitch EG, 12-voice
//! poly or mono+glide, filter/chorus/phaser, and DX7 SysEx voices.

// DSP loops index several parallel per-operator arrays at once.
#![allow(clippy::needless_range_loop)]

pub mod algorithms;
pub mod env;
pub mod modulation;
pub mod patch;
pub mod sine;

/// Operators per voice.
pub const OPS: usize = crate::params::FM_OPS;
/// Samples per control block: envelopes, LFO and pitch update once per block.
pub const BLOCK: usize = 32;
