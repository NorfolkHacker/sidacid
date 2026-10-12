//! The sampler: 16 sample slots (chopped breaks or chromatic voices),
//! 16 voices, choke, tempo lock and Grit. Audio is embedded in the
//! project as 16-bit PCM and handed to the audio thread as `Arc`s.

// DSP loops index parallel arrays.
#![allow(clippy::needless_range_loop)]

pub mod sample;
pub mod settings;
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
/// Most decoded audio (f32 bytes) a project may hold.
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// New zones are placed upward from here (MIDI 24 = C1).
pub const FIRST_KEY: u8 = 24;
