//! SID-style sound chip for SidAcid.
//!
//! Derived from the Acid OS synthesiser (`acid-synth`), MIT licence,
//! Copyright (c) 2026 NorfolkHacker. See THIRD_PARTY_NOTICES.md.

// DSP loops index several parallel per-voice arrays at once; iterators would obscure them.
#![allow(clippy::needless_range_loop)]

mod chip;
mod env;
mod osc;
mod svf;

pub use chip::{SidChip, SyncMode, Voice};
pub use env::{EnvStage, Envelope};
pub use osc::{Oscillator, Waveform};
pub use svf::{FILTER_BP, FILTER_HP, FILTER_LP, Svf};
