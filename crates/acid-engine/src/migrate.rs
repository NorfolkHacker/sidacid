//! Older project files. v1 had four machines: no FM or sampler, 4-wide
//! bars and two synth transposes. v1 and v2 named params by machine
//! position (`LegacyParamId`); v3 names them by machine uid.

use crate::machine::MachineUid;
use crate::params::{
    ChannelParam, DrumParam, FmParam, FxParam, KitParam, MachineParam, MasterParam, OpParam,
    ParamId, SamplerParam, SlotParam, SynthParam,
};
use crate::project::{
    AutoLane, Bar, DrumMachine, FmMachine, ParamLane, PatternRef, Project, SamplerMachine, SeqView,
    Song, SynthMachine, SynthPattern, SynthStep,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The v1/v2 `ParamId`, frozen: same variant names, order and fields, so
/// old RON parses unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) enum LegacyParamId {
    Synth(u8, SynthParam),
    Drum(u8, u8, DrumParam),
    Kit(u8, KitParam),
    Channel(u8, ChannelParam),
    Fx(FxParam),
    Master(MasterParam),
    Fm(FmParam),
    Sampler(SamplerParam),
    FmOp(u8, OpParam),
    SamplerSlot(u8, SlotParam),
}

impl LegacyParamId {
    /// The same control on the standard rack's uids (Synth 1..2 = 1..2,
    /// 808 = 3, 909 = 4, FM = 5, Sampler = 6). Out-of-range positions map
    /// to uids outside the rack, which validation then rejects.
    pub(crate) fn upgrade(self) -> ParamId {
        let m = |base: u16, i: u8, p| ParamId::Machine(MachineUid(base + u16::from(i)), p);
        match self {
            LegacyParamId::Synth(s, p) => m(1, s, MachineParam::Synth(p)),
            LegacyParamId::Drum(k, i, p) => m(3, k, MachineParam::Drum(i, p)),
            LegacyParamId::Kit(k, p) => m(3, k, MachineParam::Kit(p)),
            LegacyParamId::Channel(c, p) => m(1, c, MachineParam::Channel(p)),
            LegacyParamId::Fm(p) => m(5, 0, MachineParam::Fm(p)),
            LegacyParamId::FmOp(o, p) => m(5, 0, MachineParam::FmOp(o, p)),
            LegacyParamId::Sampler(p) => m(6, 0, MachineParam::Sampler(p)),
            LegacyParamId::SamplerSlot(s, p) => m(6, 0, MachineParam::SamplerSlot(s, p)),
            LegacyParamId::Fx(p) => ParamId::Fx(p),
            LegacyParamId::Master(p) => ParamId::Master(p),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct LegacyParamLane {
    pub param: LegacyParamId,
    pub values: Vec<Option<f32>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct LegacyAutoLane {
    pub param: LegacyParamId,
    pub points: BTreeMap<u32, f32>,
}

impl LegacyAutoLane {
    fn upgrade(self) -> AutoLane {
        AutoLane {
            param: self.param.upgrade(),
            points: self.points,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SynthPatternV2 {
    pub steps: Vec<SynthStep>,
    pub length: u8,
    pub transpose: i8,
    pub param_lane: Option<LegacyParamLane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SynthMachineV2 {
    pub patterns: Vec<SynthPatternV2>,
    pub view: SeqView,
}

impl SynthMachineV2 {
    fn upgrade(self) -> SynthMachine {
        SynthMachine {
            patterns: self
                .patterns
                .into_iter()
                .map(|p| SynthPattern {
                    steps: p.steps,
                    length: p.length,
                    transpose: p.transpose,
                    param_lane: p.param_lane.map(|l| ParamLane {
                        param: l.param.upgrade(),
                        values: l.values,
                    }),
                })
                .collect(),
            view: self.view,
        }
    }
}

#[cfg(test)]
impl Default for SynthMachineV2 {
    fn default() -> Self {
        let pattern = SynthPatternV2 {
            steps: vec![SynthStep::default(); crate::project::STEPS],
            length: 16,
            transpose: 0,
            param_lane: None,
        };
        Self {
            patterns: vec![pattern; crate::project::PATTERNS],
            view: SeqView::Step303,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SongV2 {
    pub bars: Vec<Bar>,
    pub loop_range: Option<(u16, u16)>,
    pub automation: Vec<LegacyAutoLane>,
}

/// The v2 project: six fixed machines, params named by position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProjectV2 {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub synths: [SynthMachineV2; 2],
    pub drums: [DrumMachine; 2],
    pub fm: FmMachine,
    pub sampler: SamplerMachine,
    pub params: BTreeMap<LegacyParamId, f32>,
    pub selected: [u8; 6],
    pub song: SongV2,
}

impl ProjectV2 {
    /// A v3 project: the same machines and values, params keyed by uid.
    pub(crate) fn upgrade(self) -> Project {
        let [s0, s1] = self.synths;
        let mut p = Project {
            name: self.name,
            bpm: self.bpm,
            swing: self.swing,
            synths: [s0.upgrade(), s1.upgrade()],
            drums: self.drums,
            fm: self.fm,
            sampler: self.sampler,
            selected: self.selected,
            ..Project::default()
        };
        p.params
            .extend(self.params.into_iter().map(|(id, v)| (id.upgrade(), v)));
        p.song = Song {
            bars: self.song.bars,
            loop_range: self.song.loop_range,
            automation: self
                .song
                .automation
                .into_iter()
                .map(LegacyAutoLane::upgrade)
                .collect(),
        };
        p
    }
}

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
    pub automation: Vec<LegacyAutoLane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProjectV1 {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub synths: [SynthMachineV2; 2],
    pub drums: [DrumMachine; 2],
    pub params: BTreeMap<LegacyParamId, f32>,
    pub selected: [u8; 4],
    pub song: SongV1,
}

impl ProjectV1 {
    /// A v2 project: the four old machines as they were, FM and sampler
    /// empty, new params at their defaults.
    pub(crate) fn upgrade(self) -> ProjectV2 {
        let mut selected = [0; 6];
        selected[..4].copy_from_slice(&self.selected);
        ProjectV2 {
            version: 2,
            name: self.name,
            bpm: self.bpm,
            swing: self.swing,
            synths: self.synths,
            drums: self.drums,
            fm: FmMachine::default(),
            sampler: SamplerMachine::default(),
            params: self.params,
            selected,
            song: SongV2 {
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
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_params_map_to_the_standard_uids() {
        use crate::machine::MachineUid as U;
        use crate::params::*;
        let m = |u, p| ParamId::Machine(U(u), p);
        assert_eq!(
            LegacyParamId::Synth(1, SynthParam::Cutoff).upgrade(),
            m(2, MachineParam::Synth(SynthParam::Cutoff))
        );
        assert_eq!(
            LegacyParamId::Drum(1, 3, DrumParam::Tone).upgrade(),
            m(4, MachineParam::Drum(3, DrumParam::Tone))
        );
        assert_eq!(
            LegacyParamId::Kit(0, KitParam::Accent).upgrade(),
            m(3, MachineParam::Kit(KitParam::Accent))
        );
        assert_eq!(
            LegacyParamId::Channel(5, ChannelParam::Pan).upgrade(),
            m(6, MachineParam::Channel(ChannelParam::Pan))
        );
        assert_eq!(
            LegacyParamId::FmOp(2, OpParam::Level).upgrade(),
            m(5, MachineParam::FmOp(2, OpParam::Level))
        );
        assert_eq!(
            LegacyParamId::SamplerSlot(9, SlotParam::Tune).upgrade(),
            m(6, MachineParam::SamplerSlot(9, SlotParam::Tune))
        );
        assert_eq!(
            LegacyParamId::Fx(FxParam::DelayMs).upgrade(),
            ParamId::Fx(FxParam::DelayMs)
        );
    }

    #[test]
    fn v2_projects_load_with_uid_keyed_params_lanes_and_automation() {
        use crate::params::*;
        let mut synths = [SynthMachineV2::default(), SynthMachineV2::default()];
        synths[1].patterns[3].param_lane = Some(LegacyParamLane {
            param: LegacyParamId::Synth(1, SynthParam::Resonance),
            values: vec![Some(0.25); crate::project::STEPS],
        });
        let mut params = BTreeMap::new();
        params.insert(LegacyParamId::Drum(1, 2, DrumParam::Decay), 0.1);
        params.insert(LegacyParamId::Master(MasterParam::Level), 0.6);
        let v2 = ProjectV2 {
            version: 2,
            name: "Two".into(),
            bpm: 120.0,
            swing: 0.0,
            synths,
            drums: Default::default(),
            fm: FmMachine::default(),
            sampler: SamplerMachine::default(),
            params,
            selected: [0, 1, 2, 3, 4, 5],
            song: SongV2 {
                bars: vec![Bar::default()],
                loop_range: None,
                automation: vec![LegacyAutoLane {
                    param: LegacyParamId::FmOp(4, OpParam::R2),
                    points: [(0, 0.5)].into_iter().collect(),
                }],
            },
        };
        let text = ron::ser::to_string_pretty(&v2, ron::ser::PrettyConfig::default()).unwrap();
        let p = Project::from_ron(&text).unwrap();
        assert_eq!(p.version, crate::project::PROJECT_VERSION);
        assert_eq!(p.param(drum_param(1, 2, DrumParam::Decay)), 0.1);
        assert_eq!(p.param(ParamId::Master(MasterParam::Level)), 0.6);
        assert_eq!(p.params.len(), 589, "missing params filled with defaults");
        let lane = p.synths[1].patterns[3].param_lane.as_ref().unwrap();
        assert_eq!(lane.param, synth_param(1, SynthParam::Resonance));
        assert_eq!(p.song.automation[0].param, fm_op_param(4, OpParam::R2));
        assert_eq!(p.selected, [0, 1, 2, 3, 4, 5]);
        // Saved again, it is a v3 file that loads as itself.
        assert_eq!(Project::from_ron(&p.to_ron()).unwrap(), p);
    }
}
