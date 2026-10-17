//! The Pad Kit: 16 pads, each up to four velocity layers of sample.

pub mod pad;

/// Pads in a Pad Kit.
pub const PADS: usize = 16;
/// Velocity layers on one pad.
pub const MAX_LAYERS: usize = 4;
