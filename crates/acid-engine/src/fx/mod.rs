//! Effects. Every process() works on stereo slices in place (inserts) or
//! adds a wet signal into output slices (sends).

pub mod compressor;
pub mod delay;
pub mod distortion;
pub mod lofi;
pub mod reverb;

pub use compressor::Compressor;
pub use delay::Delay;
pub use distortion::Distortion;
pub use lofi::LoFi;
pub use reverb::Reverb;
