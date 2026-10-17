//! The Pad Kit: 16 pads, each up to four velocity layers of sample.

mod machine;
pub mod pad;
pub mod play;
pub mod settings;

pub use machine::PadKit;

/// Pads in a Pad Kit.
pub const PADS: usize = 16;
/// Velocity layers on one pad.
pub const MAX_LAYERS: usize = 4;
