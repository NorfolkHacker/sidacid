//! Older project files. v1 had four machines: no FM or sampler, 4-wide
//! bars and two synth transposes.

use crate::params::ParamId;
use crate::project::{AutoLane, Bar, DrumMachine, PatternRef, Project, Song, SynthMachine};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct BarV1 {
    pub patterns: [Option<PatternRef>; 4],
    pub mutes: [bool; 4],
    pub transpose: [i8; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SongV1 {
    pub bars: Vec<BarV1>,
    pub loop_range: Option<(u16, u16)>,
    pub automation: Vec<AutoLane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProjectV1 {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub synths: [SynthMachine; 2],
    pub drums: [DrumMachine; 2],
    pub params: BTreeMap<ParamId, f32>,
    pub selected: [u8; 4],
    pub song: SongV1,
}

impl ProjectV1 {
    /// A v2 project: the four old machines as they were, FM and sampler
    /// empty, new params at their defaults.
    pub(crate) fn upgrade(self) -> Project {
        let mut p = Project {
            name: self.name,
            bpm: self.bpm,
            swing: self.swing,
            synths: self.synths,
            drums: self.drums,
            ..Project::default()
        };
        p.params.extend(self.params);
        p.selected[..4].copy_from_slice(&self.selected);
        p.song = Song {
            bars: self
                .song
                .bars
                .into_iter()
                .map(|b| {
                    let mut bar = Bar::default();
                    bar.patterns[..4].copy_from_slice(&b.patterns);
                    bar.mutes[..4].copy_from_slice(&b.mutes);
                    bar.transpose[..2].copy_from_slice(&b.transpose);
                    bar
                })
                .collect(),
            loop_range: self.song.loop_range,
            automation: self.song.automation,
        };
        p
    }
}
