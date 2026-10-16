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

    /// A song of bars as v5 had them, as the v6 conversion gives it: bar
    /// i plays `bars[i]` (machine, pattern) and lasts its longest pattern
    /// (16 steps if empty); each is one block at the bar's start, as long
    /// as the bar. The song lasts every bar, empty ones too.
    pub fn bar_song(
        p: &crate::project::Project,
        bars: &[Vec<(crate::machine::MachineUid, u8)>],
    ) -> crate::project::Song {
        use crate::project::{Block, PatternRef, Song};
        let lens = crate::song::lens_of(p);
        let mut song = Song::default();
        let mut start = 0;
        for bar in bars {
            let len = bar.iter().map(|&(u, s)| lens(u, s)).max().unwrap_or(16);
            for &(uid, slot) in bar {
                song.lane_mut(uid).push(Block {
                    len: (lens(uid, slot) != len).then_some(len as u16),
                    ..Block::new(start, PatternRef(slot))
                });
            }
            start += len;
        }
        song.end = (start > 0).then_some(start);
        song
    }
}
