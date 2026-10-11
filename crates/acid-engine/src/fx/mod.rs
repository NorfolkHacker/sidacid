//! Effects. Every process() works on stereo slices in place (inserts) or
//! adds a wet signal into output slices (sends).

pub mod compressor;
pub mod distortion;

pub use compressor::Compressor;
pub use distortion::Distortion;
