//! Older project files. v1 had four machines: no FM or sampler, 4-wide
//! bars and two synth transposes. v1 and v2 named params by machine
//! position (`LegacyParamId`); v3 names them by machine uid. v3 and v4
//! stored acid and drum patterns as 1/16 steps; v5 stores them as notes.

use crate::drums::KitKind;
use crate::machine::{MachineType, MachineUid, standard_rack};
use crate::params::{
    ChannelParam, DRUM_INSTRUMENTS, DrumParam, FmParam, FxParam, GlobalParam, KitParam,
    MachineParam, MasterParam, OpParam, ParamId, SamplerParam, SlotParam, SynthParam,
};
use crate::project::{
    AutoLane, Bar, Cell, DrumMachine, DrumPattern, FmMachine, MachineBody, MachineEntry,
    MonoMachine, PATTERNS, PROJECT_VERSION, ParamLane, PatternRef, Project, STEPS, SamplerMachine,
    Song, SynthMachine, SynthPattern,
};
use crate::steps::{DrumGrid, DrumHit, SynthGrid, SynthStep};
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

/// The v3/v4 synth sequencer view, frozen (v5 dropped it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SeqView {
    #[default]
    Step303,
    PianoRoll,
}

/// The v3/v4 acid pattern: 1/16 steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SynthPatternV4 {
    pub steps: Vec<SynthStep>,
    pub length: u8,
    pub transpose: i8,
    pub param_lane: Option<ParamLane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SynthMachineV4 {
    pub patterns: Vec<SynthPatternV4>,
    pub view: SeqView,
}

/// The v3/v4 drum pattern: one hit per instrument per 1/16 step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DrumPatternV4 {
    pub steps: Vec<[DrumHit; DRUM_INSTRUMENTS]>,
    pub length: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DrumMachineV4 {
    pub kind: KitKind,
    pub patterns: Vec<DrumPatternV4>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum MachineBodyV4 {
    Synth(SynthMachineV4),
    Kit(DrumMachineV4),
    Fm(FmMachine),
    Sampler(SamplerMachine),
    Mono(MonoMachine),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct MachineEntryV4 {
    pub uid: MachineUid,
    pub name: String,
    pub selected: u8,
    pub params: BTreeMap<MachineParam, f32>,
    pub body: MachineBodyV4,
}

/// The v3/v4 project: the rack, with step-based acid and drum patterns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProjectV4 {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub machines: Vec<MachineEntryV4>,
    pub next_uid: u16,
    pub globals: BTreeMap<GlobalParam, f32>,
    pub song: Song,
}

impl Default for SynthPatternV4 {
    fn default() -> Self {
        Self {
            steps: vec![SynthStep::default(); STEPS],
            length: 16,
            transpose: 0,
            param_lane: None,
        }
    }
}

impl Default for DrumPatternV4 {
    fn default() -> Self {
        Self {
            steps: vec![[DrumHit::Off; DRUM_INSTRUMENTS]; STEPS],
            length: 16,
        }
    }
}

impl SynthPatternV4 {
    fn upgrade(self) -> SynthPattern {
        SynthPattern {
            roll: SynthGrid {
                steps: self.steps,
                length: self.length,
            }
            .to_roll(),
            transpose: self.transpose,
            param_lane: self.param_lane,
        }
    }
}

impl DrumPatternV4 {
    fn upgrade(self) -> DrumPattern {
        DrumPattern {
            roll: DrumGrid {
                steps: self.steps,
                length: self.length,
            }
            .to_roll(),
        }
    }
}

impl MachineBodyV4 {
    /// An empty v4 machine of `ty`.
    fn new(ty: MachineType) -> Self {
        let kit = |kind| {
            MachineBodyV4::Kit(DrumMachineV4 {
                kind,
                patterns: vec![DrumPatternV4::default(); PATTERNS],
            })
        };
        match ty {
            MachineType::Synth => MachineBodyV4::Synth(SynthMachineV4 {
                patterns: vec![SynthPatternV4::default(); PATTERNS],
                view: SeqView::Step303,
            }),
            MachineType::Kit808 => kit(KitKind::K808),
            MachineType::Kit909 => kit(KitKind::K909),
            MachineType::Fm => MachineBodyV4::Fm(FmMachine::default()),
            MachineType::Sampler => MachineBodyV4::Sampler(SamplerMachine::default()),
            MachineType::Mono => MachineBodyV4::Mono(MonoMachine::default()),
        }
    }

    fn upgrade(self) -> MachineBody {
        match self {
            MachineBodyV4::Synth(s) => MachineBody::Synth(SynthMachine {
                patterns: s
                    .patterns
                    .into_iter()
                    .map(SynthPatternV4::upgrade)
                    .collect(),
            }),
            MachineBodyV4::Kit(k) => MachineBody::Kit(DrumMachine {
                kind: k.kind,
                patterns: k.patterns.into_iter().map(DrumPatternV4::upgrade).collect(),
            }),
            MachineBodyV4::Fm(f) => MachineBody::Fm(f),
            MachineBodyV4::Sampler(s) => MachineBody::Sampler(s),
            MachineBodyV4::Mono(m) => MachineBody::Mono(m),
        }
    }
}

impl ProjectV4 {
    /// The v4 default project (as `Project::default()` was): the
    /// standard six machines, every param at its default.
    pub(crate) fn standard() -> Self {
        Self {
            version: 4,
            name: "Untitled".into(),
            bpm: 125.0,
            swing: 0.0,
            machines: standard_rack()
                .into_iter()
                .map(|(uid, ty, name)| MachineEntryV4 {
                    uid,
                    name: name.into(),
                    selected: 0,
                    params: MachineParam::all_for(ty)
                        .into_iter()
                        .map(|p| (p, p.info().default))
                        .collect(),
                    body: MachineBodyV4::new(ty),
                })
                .collect(),
            next_uid: 7,
            globals: GlobalParam::all()
                .into_iter()
                .map(|g| (g, g.info().default))
                .collect(),
            song: Song::default(),
        }
    }

    /// A v5 project: acid and drum steps become notes.
    pub(crate) fn upgrade(self) -> Project {
        Project {
            version: PROJECT_VERSION,
            name: self.name,
            bpm: self.bpm,
            swing: self.swing,
            machines: self
                .machines
                .into_iter()
                .map(|m| MachineEntry {
                    uid: m.uid,
                    name: m.name,
                    selected: m.selected,
                    params: m.params,
                    body: m.body.upgrade(),
                })
                .collect(),
            next_uid: self.next_uid,
            globals: self.globals,
            song: self.song,
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
    fn upgrade(self) -> SynthMachineV4 {
        SynthMachineV4 {
            patterns: self
                .patterns
                .into_iter()
                .map(|p| SynthPatternV4 {
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
            steps: vec![SynthStep::default(); STEPS],
            length: 16,
            transpose: 0,
            param_lane: None,
        };
        Self {
            patterns: vec![pattern; PATTERNS],
            view: SeqView::Step303,
        }
    }
}

/// The v1/v2 drum machine: no kit kind (the position chose the kit).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DrumMachineV2 {
    pub patterns: Vec<DrumPatternV4>,
}

#[cfg(test)]
impl Default for DrumMachineV2 {
    fn default() -> Self {
        Self {
            patterns: vec![DrumPatternV4::default(); PATTERNS],
        }
    }
}

/// The v2 bar: one slot per fixed machine, by standard-rack position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct BarV2 {
    pub patterns: [Option<PatternRef>; 6],
    pub mutes: [bool; 6],
    /// Song transpose for Synth 1, Synth 2, FM.
    pub transpose: [i8; 3],
}

impl BarV2 {
    /// One cell per fixed machine, on the standard rack's uids.
    fn upgrade(self) -> Bar {
        Bar {
            cells: standard_rack()
                .iter()
                .enumerate()
                .map(|(i, &(uid, _, _))| {
                    // Synth 1, Synth 2 and FM (positions 0, 1, 4) transpose.
                    let transpose = match i {
                        0 | 1 => self.transpose[i],
                        4 => self.transpose[2],
                        _ => 0,
                    };
                    let cell = Cell {
                        pattern: self.patterns[i],
                        mute: self.mutes[i],
                        transpose,
                    };
                    (uid, cell)
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SongV2 {
    pub bars: Vec<BarV2>,
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
    pub drums: [DrumMachineV2; 2],
    pub fm: FmMachine,
    pub sampler: SamplerMachine,
    pub params: BTreeMap<LegacyParamId, f32>,
    pub selected: [u8; 6],
    pub song: SongV2,
}

impl ProjectV2 {
    /// A v4 project: the six machines as rack entries on uids 1..=6,
    /// params keyed by uid, bars as per-uid cells.
    pub(crate) fn upgrade(self) -> ProjectV4 {
        let [s0, s1] = self.synths;
        let [d0, d1] = self.drums;
        let kit = |kind, d: DrumMachineV2| {
            MachineBodyV4::Kit(DrumMachineV4 {
                kind,
                patterns: d.patterns,
            })
        };
        let mut bodies = [
            MachineBodyV4::Synth(s0.upgrade()),
            MachineBodyV4::Synth(s1.upgrade()),
            kit(KitKind::K808, d0),
            kit(KitKind::K909, d1),
            MachineBodyV4::Fm(self.fm),
            MachineBodyV4::Sampler(self.sampler),
        ]
        .into_iter();
        let mut p = ProjectV4 {
            name: self.name,
            bpm: self.bpm,
            swing: self.swing,
            ..ProjectV4::standard()
        };
        for (m, entry) in p.machines.iter_mut().enumerate() {
            entry.selected = self.selected[m];
            if let Some(body) = bodies.next() {
                entry.body = body;
            }
        }
        for (id, v) in self.params {
            match id.upgrade() {
                // Stored even when it doesn't fit, so validation rejects
                // the damaged file. A position past the six has no entry
                // and is dropped.
                ParamId::Machine(uid, mp) => {
                    if let Some(m) = p.machines.iter_mut().find(|m| m.uid == uid) {
                        m.params.insert(mp, v);
                    }
                }
                global => {
                    if let Some(g) = global.global() {
                        p.globals.insert(g, v);
                    }
                }
            }
        }
        p.song = Song {
            bars: self.song.bars.into_iter().map(BarV2::upgrade).collect(),
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
    pub drums: [DrumMachineV2; 2],
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
                        let mut bar = BarV2::default();
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
    fn v4_steps_upgrade_to_notes() {
        use crate::project::{Note, PROJECT_VERSION, Project};
        use crate::steps::{ACID_VEL, DrumHit, Gate, SynthStep};
        let mut v4 = ProjectV4::standard();
        let MachineBodyV4::Synth(s) = &mut v4.machines[0].body else {
            panic!("position 0 is Synth 1")
        };
        let first = SynthStep {
            note: 0,
            octave: 0,
            gate: Gate::On,
            accent: true,
            slide: false,
        };
        s.patterns[0].steps[0] = first;
        s.patterns[0].steps[1] = SynthStep {
            gate: Gate::Tie,
            slide: true,
            ..first
        };
        let MachineBodyV4::Kit(k) = &mut v4.machines[2].body else {
            panic!("position 2 is the 808")
        };
        k.patterns[0].steps[4][1] = DrumHit::Accent;
        let up = Project::from_ron(&ron::ser::to_string(&v4).unwrap()).unwrap();
        assert_eq!(up.version, PROJECT_VERSION);
        let synth = up.synth(MachineUid(1)).unwrap();
        assert_eq!(
            synth.patterns[0].roll.notes,
            vec![Note {
                accent: true,
                slide: true,
                ..Note::new(0, 8, 36, ACID_VEL)
            }]
        );
        let kit = up.kit(MachineUid(3)).unwrap();
        assert_eq!(
            kit.patterns[0].roll.notes,
            vec![Note {
                accent: true,
                ..Note::new(16, 4, 1, ACID_VEL)
            }]
        );
    }

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
                bars: vec![BarV2::default()],
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
        assert_eq!(
            p.param(MachineUid(4).param(MachineParam::Drum(2, DrumParam::Decay))),
            0.1
        );
        assert_eq!(p.param(ParamId::Master(MasterParam::Level)), 0.6);
        let params: usize = p.machines.iter().map(|m| m.params.len()).sum();
        assert_eq!(
            params + p.globals.len(),
            589,
            "missing params filled with defaults"
        );
        let lane = p.synth(MachineUid(2)).unwrap().patterns[3]
            .param_lane
            .as_ref()
            .unwrap();
        assert_eq!(lane.param, MachineUid(2).param(SynthParam::Resonance));
        assert_eq!(
            p.song.automation[0].param,
            MachineUid(5).param(MachineParam::FmOp(4, OpParam::R2))
        );
        let selected: Vec<_> = p.machines.iter().map(|m| m.selected).collect();
        assert_eq!(selected, [0, 1, 2, 3, 4, 5]);
        // Saved again, it is a v3 file that loads as itself.
        assert_eq!(Project::from_ron(&p.to_ron()).unwrap(), p);
    }
}
