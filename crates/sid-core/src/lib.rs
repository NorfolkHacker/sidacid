//! SID-style sound chip for SidAcid.
//!
//! Derived from the Acid OS synthesiser (`acid-synth`), MIT licence,
//! Copyright (c) 2026 NorfolkHacker. See THIRD_PARTY_NOTICES.md.

mod env;
mod osc;
mod svf;

pub use env::{EnvStage, Envelope};
pub use osc::{Oscillator, Waveform};
pub use svf::{Svf, FILTER_BP, FILTER_HP, FILTER_LP};
