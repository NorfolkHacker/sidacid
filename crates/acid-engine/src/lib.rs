//! SidAcid sound engine: instruments, effects, sequencer and project data.
//! No GUI or audio-device dependencies; everything renders offline.

// DSP and sequencer loops index several parallel arrays; iterators would obscure them.
#![allow(clippy::needless_range_loop)]

pub mod demo;
pub mod drums;
pub mod engine;
pub mod fm;
pub mod fx;
pub mod host;
pub mod machine;
mod migrate;
pub mod mixer;
pub mod params;
pub mod project;
pub mod rack;
mod rack_edit;
pub mod random;
pub mod render;
pub mod sampler;
pub mod sequencer;
pub mod song;
pub mod steps;
pub mod synth;
pub mod va;

#[cfg(test)]
#[allow(dead_code)]
pub(crate) mod testutil {
    pub fn rms(x: &[f32]) -> f32 {
        if x.is_empty() {
            return 0.0;
        }
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    pub fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, v| m.max(v.abs()))
    }

    pub fn all_finite(x: &[f32]) -> bool {
        x.iter().all(|v| v.is_finite())
    }
}
