//! The sampler: 16 sample slots (chopped breaks or chromatic voices),
//! 16 voices, choke, tempo lock and Grit. Audio is embedded in the
//! project as 16-bit PCM and handed to the audio thread as `Arc`s.

// DSP loops index parallel arrays.
#![allow(clippy::needless_range_loop)]

pub mod decode;
pub mod machine;
pub mod sample;
pub mod settings;
pub mod slicing;
pub mod slot;
pub mod voice;

/// Sample slots.
pub const SLOTS: usize = crate::params::SAMPLER_SLOTS;
/// Voices shared by all slots.
pub const VOICES: usize = 16;
/// Most slices one slot may have.
pub const MAX_SLICES: usize = 64;
/// Longest sample a slot accepts.
pub const MAX_SECONDS: f32 = 30.0;
/// Most decoded audio (f32 bytes) a project may hold, shared by every
/// sampler in the rack.
pub const SAMPLE_POOL: usize = 256 * 1024 * 1024;
/// New zones are placed upward from here (MIDI 24 = C1).
pub const FIRST_KEY: u8 = 24;
pub use machine::Sampler;
