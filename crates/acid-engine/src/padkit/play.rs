//! Each pad's velocity layer ranges, in a fixed-size form for the audio thread.

use super::pad::Pad;
use super::{MAX_LAYERS, PADS};

/// Each pad's layer ranges, fixed size for the audio thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadPlay {
    /// Inclusive velocity range of each layer.
    pub ranges: [[(u8, u8); MAX_LAYERS]; PADS],
    /// Layers in use on each pad.
    pub counts: [u8; PADS],
}

impl PadPlay {
    pub const EMPTY: PadPlay = PadPlay {
        ranges: [[(0, 0); MAX_LAYERS]; PADS],
        counts: [0; PADS],
    };

    /// Built on the UI thread from the project's pads.
    pub fn from_pads(pads: &[Pad]) -> PadPlay {
        let mut play = PadPlay::EMPTY;
        for (p, pad) in pads.iter().take(PADS).enumerate() {
            let layers = &pad.layers[..pad.layers.len().min(MAX_LAYERS)];
            for (l, layer) in layers.iter().enumerate() {
                play.ranges[p][l] = (layer.vel_lo, layer.vel_hi);
            }
            play.counts[p] = layers.len() as u8;
        }
        play
    }

    /// The layer of `pad` that `vel` plays (first match), if any.
    pub fn layer_for(&self, pad: usize, vel: u8) -> Option<usize> {
        let n = *self.counts.get(pad)? as usize;
        self.ranges[pad][..n.min(MAX_LAYERS)]
            .iter()
            .position(|&(lo, hi)| (lo..=hi).contains(&vel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::padkit::pad::Layer;

    fn pad(n: usize) -> Pad {
        let mut p = Pad {
            layers: vec![Layer::new("x", 48_000, 1, &[0.5; 10]); n],
            ..Pad::default()
        };
        p.split_evenly();
        p
    }

    #[test]
    fn from_pads_copies_ranges_and_counts() {
        let mut pads = vec![Pad::default(); PADS];
        pads[0] = pad(2);
        pads[3] = pad(4);
        let play = PadPlay::from_pads(&pads);
        assert_eq!(play.counts[0], 2);
        assert_eq!(play.counts[1], 0);
        assert_eq!(play.counts[3], 4);
        assert_eq!(play.ranges[0][..2], [(1, 63), (64, 127)]);
        assert_eq!(play.ranges[3][3], (96, 127));
    }

    #[test]
    fn layer_for_matches_the_pad() {
        let mut pads = vec![Pad::default(); PADS];
        pads[0] = pad(3);
        pads[1] = pad(1);
        pads[1].layers[0].vel_lo = 100;
        let play = PadPlay::from_pads(&pads);
        for vel in 1..=127 {
            assert_eq!(play.layer_for(0, vel), pads[0].layer_for(vel), "vel {vel}");
        }
        assert_eq!(play.layer_for(1, 50), None);
        assert_eq!(play.layer_for(1, 100), Some(0));
        assert_eq!(play.layer_for(2, 100), None, "an empty pad");
        assert_eq!(play.layer_for(PADS, 100), None, "no such pad");
        assert_eq!(PadPlay::EMPTY.layer_for(0, 100), None);
    }

    #[test]
    fn extra_layers_and_short_pad_lists_are_safe() {
        let play = PadPlay::from_pads(&[pad(4), pad(4), pad(4)]);
        assert_eq!(play.counts[2], 4);
        assert_eq!(play.counts[3], 0);
        let mut big = pad(4);
        big.layers.push(Layer::new("x", 48_000, 1, &[0.5; 10]));
        assert_eq!(PadPlay::from_pads(&[big]).counts[0], MAX_LAYERS as u8);
    }
}
