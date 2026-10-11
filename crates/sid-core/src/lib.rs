//! SID-style sound chip for SidAcid.
//!
//! Derived from the Acid OS synthesiser (`acid-synth`), MIT licence,
//! Copyright (c) 2026 NorfolkHacker. See THIRD_PARTY_NOTICES.md.

mod env;
mod osc;

pub use env::{EnvStage, Envelope};
pub use osc::{Oscillator, Waveform};
