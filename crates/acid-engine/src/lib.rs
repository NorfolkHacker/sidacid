//! SidAcid sound engine: instruments, effects, sequencer and project data.
//! No GUI or audio-device dependencies; everything renders offline.

pub mod drums;
pub mod fx;
pub mod mixer;
pub mod params;
pub mod project;
pub mod sequencer;
pub mod synth;

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
