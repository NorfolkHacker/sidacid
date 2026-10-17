//! A pad and its velocity layers as the project stores them.

use crate::sampler::sample::SampleData;
use crate::sampler::slot::{pcm_codec, pcm_problem};
use serde::{Deserialize, Serialize};

use super::MAX_LAYERS;

/// One sample on a pad, played for velocities `vel_lo..=vel_hi`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// Inclusive, 1..=127.
    pub vel_lo: u8,
    pub vel_hi: u8,
    /// File stem, at most 24 characters.
    pub name: String,
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u8,
    /// Interleaved 16-bit samples.
    #[serde(with = "pcm_codec")]
    pub pcm: Vec<i16>,
}

impl Layer {
    /// A layer from decoded audio (interleaved, -1..1), covering every velocity.
    pub fn new(name: &str, sample_rate: u32, channels: u8, samples: &[f32]) -> Layer {
        Layer {
            vel_lo: 1,
            vel_hi: 127,
            name: name.to_string(),
            sample_rate,
            channels,
            pcm: samples
                .iter()
                .map(|x| (x.clamp(-1.0, 1.0) * 32_767.0).round() as i16)
                .collect(),
        }
    }

    pub fn frames(&self) -> u32 {
        (self.pcm.len() / self.channels.max(1) as usize) as u32
    }

    /// Size of the decoded f32 audio the engine holds.
    pub fn memory_bytes(&self) -> usize {
        self.pcm.len() * 4
    }

    /// Builds the engine's copy. Allocates: call off the audio thread.
    pub fn to_sample(&self) -> SampleData {
        SampleData::from_pcm(&self.pcm, self.channels, self.sample_rate)
    }

    /// Why this layer could not play, if it couldn't (no audio, bad
    /// channels or rate, too long).
    pub fn problem(&self) -> Option<&'static str> {
        pcm_problem(&self.pcm, self.channels, self.sample_rate)
    }

    fn is_well_formed(&self) -> bool {
        self.problem().is_none()
            && 1 <= self.vel_lo
            && self.vel_lo <= self.vel_hi
            && self.vel_hi <= 127
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pad {
    /// "" = shown as the first layer's name, or "P<n>".
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub layers: Vec<Layer>,
}

impl Pad {
    /// Sets every layer's range to the even split for the layer count.
    pub fn split_evenly(&mut self) {
        let n = self.layers.len().min(MAX_LAYERS);
        // Velocities 1..=127, cut at these boundaries.
        let cuts: &[u8] = match n {
            0 => return,
            1 => &[127],
            2 => &[63, 127],
            3 => &[42, 85, 127],
            _ => &[31, 63, 95, 127],
        };
        let mut lo = 1;
        for (l, &hi) in self.layers.iter_mut().zip(cuts) {
            l.vel_lo = lo;
            l.vel_hi = hi;
            lo = hi + 1;
        }
    }

    /// Index of the layer `vel` plays (first match), if any.
    pub fn layer_for(&self, vel: u8) -> Option<usize> {
        self.layers
            .iter()
            .position(|l| (l.vel_lo..=l.vel_hi).contains(&vel))
    }

    pub fn is_well_formed(&self) -> bool {
        self.layers.len() <= MAX_LAYERS && self.layers.iter().all(Layer::is_well_formed)
    }

    pub fn memory_bytes(&self) -> usize {
        self.layers.iter().map(Layer::memory_bytes).sum()
    }

    /// What the pad lane and the pad grid show.
    pub fn label(&self, pad: usize) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else if let Some(l) = self.layers.first() {
            l.name.clone()
        } else {
            format!("P{}", pad + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer() -> Layer {
        Layer::new("kick", 44_100, 1, &[0.5; 100])
    }

    #[test]
    fn layers_split_evenly_by_count() {
        let mut p = Pad::default();
        let want: [&[(u8, u8)]; 4] = [
            &[(1, 127)],
            &[(1, 63), (64, 127)],
            &[(1, 42), (43, 85), (86, 127)],
            &[(1, 31), (32, 63), (64, 95), (96, 127)],
        ];
        for w in want {
            p.layers.push(layer());
            p.split_evenly();
            let got: Vec<(u8, u8)> = p.layers.iter().map(|l| (l.vel_lo, l.vel_hi)).collect();
            assert_eq!(got, w);
        }
    }

    #[test]
    fn the_first_matching_layer_plays() {
        let mut p = Pad {
            layers: vec![layer(), layer()],
            ..Pad::default()
        };
        p.split_evenly();
        assert_eq!(p.layer_for(10), Some(0));
        assert_eq!(p.layer_for(64), Some(1));
        p.layers[1].vel_lo = 100; // a gap: 64..=99 plays nothing
        assert_eq!(p.layer_for(80), None);
        p.layers[0].vel_hi = 120; // an overlap: the first wins
        assert_eq!(p.layer_for(110), Some(0));
        assert_eq!(Pad::default().layer_for(100), None);
    }

    #[test]
    fn a_pad_with_five_layers_or_an_inverted_range_is_malformed() {
        let mut p = Pad {
            layers: vec![layer(); 5],
            ..Pad::default()
        };
        assert!(!p.is_well_formed());
        p.layers.truncate(1);
        assert!(p.is_well_formed());
        p.layers[0].vel_lo = 90;
        p.layers[0].vel_hi = 20;
        assert!(!p.is_well_formed());
        p.layers[0].vel_lo = 0;
        p.layers[0].vel_hi = 127;
        assert!(!p.is_well_formed(), "velocity 0 is not a note");
    }

    #[test]
    fn labels_fall_back_to_the_layer_then_the_number() {
        let mut p = Pad::default();
        assert_eq!(p.label(0), "P1");
        p.layers.push(layer());
        assert_eq!(p.label(0), "kick");
        p.name = "Big".into();
        assert_eq!(p.label(15), "Big");
    }
}
