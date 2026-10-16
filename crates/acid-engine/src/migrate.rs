//! Older project files. v1 had four machines: no FM or sampler, 4-wide
//! bars and two synth transposes. v1 and v2 named params by machine
//! position (`LegacyParamId`); v3 names them by machine uid. v3 and v4
//! stored acid and drum patterns as 1/16 steps; v5 stores them as notes.
//! v1 to v5 songs are rows of bars; v6 songs are lanes of blocks.

use crate::drums::KitKind;
use crate::machine::{MAX_MACHINES, MachineType, MachineUid, standard_rack};
use crate::params::{
    ChannelParam, DRUM_INSTRUMENTS, DrumParam, FmParam, FxParam, GlobalParam, KitParam,
    MachineParam, MasterParam, OpParam, ParamId, SamplerParam, SlotParam, SynthParam,
};
use crate::project::{
    AutoLane, Block, DrumMachine, DrumPattern, FmMachine, MachineBody, MachineEntry, MonoMachine,
    PATTERNS, PROJECT_VERSION, ParamLane, PatternRef, Project, ProjectError, STEPS, SamplerMachine,
    Song, SynthMachine, SynthPattern,
};
use crate::song::{Banks, MAX_BARS};
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
    pub song: SongV5,
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
    /// Rejects step lists the grid conversion can't read: every acid and
    /// drum pattern holds exactly `STEPS` steps (the v4 `validate` rule;
    /// lengths, lanes and pattern counts are checked after conversion).
    pub(crate) fn check_shapes(&self) -> Result<(), ProjectError> {
        for m in &self.machines {
            let bad = match &m.body {
                MachineBodyV4::Synth(s) => s.patterns.iter().position(|p| p.steps.len() != STEPS),
                MachineBodyV4::Kit(k) => k.patterns.iter().position(|p| p.steps.len() != STEPS),
                _ => None,
            };
            if let Some(j) = bad {
                let label = match u8::try_from(j) {
                    Ok(j) if (j as usize) < PATTERNS => PatternRef(j).label(),
                    _ => (j + 1).to_string(),
                };
                return Err(ProjectError::Invalid(format!(
                    "{}: pattern {label}",
                    m.name
                )));
            }
        }
        Ok(())
    }

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
            song: SongV5::default(),
        }
    }

    /// A v5 project: acid and drum steps become notes.
    pub(crate) fn upgrade(self) -> ProjectV5 {
        ProjectV5 {
            version: 5,
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
                    lane: Default::default(),
                })
                .collect(),
            next_uid: self.next_uid,
            globals: self.globals,
            song: self.song,
        }
    }
}

/// One machine's slot in a v1..v5 song bar, frozen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CellV5 {
    pub pattern: Option<PatternRef>,
    pub mute: bool,
    pub transpose: i8,
}

/// A v3..v5 song bar: one cell per machine, by uid. It lasts as long as
/// its longest pattern; shorter ones loop inside it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BarV5 {
    pub cells: Vec<(MachineUid, CellV5)>,
}

impl BarV5 {
    fn cell(&self, uid: MachineUid) -> CellV5 {
        self.cells
            .iter()
            .find(|(u, _)| *u == uid)
            .map_or_else(CellV5::default, |(_, c)| *c)
    }
}

/// The v3..v5 song: a row of bars, the loop as bar indices (inclusive).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct SongV5 {
    pub bars: Vec<BarV5>,
    pub loop_range: Option<(u16, u16)>,
    pub automation: Vec<AutoLane>,
}

/// A bar with no pattern lasts this long.
const DEFAULT_BAR_STEPS: usize = 16;

/// Steps in a bar: its longest assigned pattern, or 16 when none is assigned.
fn bar_steps(bar: &BarV5, banks: Banks<'_>) -> usize {
    (0..MAX_MACHINES)
        .filter_map(|pos| {
            let (uid, _) = banks.machines[pos]?;
            banks.length(pos, bar.cell(uid).pattern?.0)
        })
        .max()
        .unwrap_or(DEFAULT_BAR_STEPS)
}

/// Global step at which `bar` starts (`bar == bars.len()` gives the song length).
fn bar_start_step(song: &SongV5, bar: usize, banks: Banks<'_>) -> u32 {
    song.bars
        .iter()
        .take(bar)
        .map(|b| bar_steps(b, banks) as u32)
        .sum()
}

/// The v5 project: notes everywhere, the song still in bars.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProjectV5 {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub machines: Vec<MachineEntry>,
    pub next_uid: u16,
    pub globals: BTreeMap<GlobalParam, f32>,
    pub song: SongV5,
}

impl ProjectV5 {
    /// The v5 song rules the conversion relies on: cells on rack
    /// machines, once per bar, patterns in range, at most `MAX_BARS` bars
    /// and a loop inside them.
    pub(crate) fn check_song(&self) -> Result<(), ProjectError> {
        let bad = |m: &str| Err(ProjectError::Invalid(m.into()));
        for bar in &self.song.bars {
            for (i, (uid, cell)) in bar.cells.iter().enumerate() {
                if !self.machines.iter().any(|m| m.uid == *uid) {
                    return bad("song bar names a machine not in the rack");
                }
                if bar.cells[..i].iter().any(|(u, _)| u == uid) {
                    return bad("song bar names a machine twice");
                }
                if cell.pattern.is_some_and(|r| r.0 as usize >= PATTERNS) {
                    return bad("song references a pattern out of range");
                }
            }
        }
        if self.song.bars.len() > MAX_BARS {
            return Err(ProjectError::Invalid(format!(
                "song has {} bars",
                self.song.bars.len()
            )));
        }
        if let Some((a, b)) = self.song.loop_range
            && (a > b || b as usize >= self.song.bars.len())
        {
            return bad("song loop range is out of bounds");
        }
        Ok(())
    }

    /// A v6 project: each bar's cells become blocks at the bar's start,
    /// as long as the bar (`len: None` when the pattern is exactly that
    /// long), so the song plays, ends and loops exactly as before.
    pub(crate) fn upgrade(self) -> Project {
        let song = convert_song(&self.song, Banks::of_machines(&self.machines));
        Project {
            version: PROJECT_VERSION,
            name: self.name,
            bpm: self.bpm,
            swing: self.swing,
            machines: self.machines,
            next_uid: self.next_uid,
            globals: self.globals,
            song,
            key: Default::default(),
        }
    }
}

/// Bars to lanes (see `ProjectV5::upgrade`). Lanes come in rack order,
/// one per machine with a pattern somewhere; cells of machines outside
/// `banks` are dropped (`check_song` rejects them first).
fn convert_song(old: &SongV5, banks: Banks<'_>) -> Song {
    let starts: Vec<u32> = (0..=old.bars.len())
        .map(|i| bar_start_step(old, i, banks))
        .collect();
    let total = starts[old.bars.len()];
    let mut lanes = Vec::new();
    for pos in 0..MAX_MACHINES {
        let Some((uid, _)) = banks.machines[pos] else {
            continue;
        };
        let blocks: Vec<Block> = old
            .bars
            .iter()
            .enumerate()
            .filter_map(|(i, bar)| {
                let cell = bar.cell(uid);
                let pattern = cell.pattern?;
                let bar_len = starts[i + 1] - starts[i];
                let own = banks.length(pos, pattern.0).map(|n| n as u32);
                Some(Block {
                    start: starts[i],
                    pattern,
                    len: (own != Some(bar_len)).then_some(bar_len as u16),
                    mute: cell.mute,
                    transpose: cell.transpose,
                })
            })
            .collect();
        if !blocks.is_empty() {
            lanes.push((uid, blocks));
        }
    }
    Song {
        lanes,
        end: (total > 0).then_some(total),
        loop_range: old
            .loop_range
            .map(|(a, b)| (starts[a as usize], starts[b as usize + 1])),
        automation: old.automation.clone(),
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
    fn upgrade(self) -> BarV5 {
        BarV5 {
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
                    let cell = CellV5 {
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
        p.song = SongV5 {
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

    fn load(v4: &ProjectV4) -> Result<Project, ProjectError> {
        Project::from_ron(&ron::ser::to_string(v4).unwrap())
    }

    /// A v5 project with Synth 1's A02 32 steps long and the sampler's A03
    /// 8 steps, and a song of five bars: 32 (a short pattern looping in
    /// a longer bar, mutes, a transpose), empty, 8, 16, empty.
    fn v5_with_song() -> ProjectV5 {
        let mut v5 = ProjectV4::standard().upgrade();
        let MachineBody::Synth(s) = &mut v5.machines[0].body else {
            panic!("position 0 is Synth 1")
        };
        s.patterns[1].roll.length = 32;
        let MachineBody::Sampler(x) = &mut v5.machines[5].body else {
            panic!("position 5 is the sampler")
        };
        x.patterns[2].length = 8;
        let cell = |slot: Option<u8>, mute, transpose| CellV5 {
            pattern: slot.map(PatternRef),
            mute,
            transpose,
        };
        let bar = |cells: Vec<(u16, CellV5)>| BarV5 {
            cells: cells.into_iter().map(|(u, c)| (MachineUid(u), c)).collect(),
        };
        v5.song = SongV5 {
            bars: vec![
                bar(vec![
                    (1, cell(Some(1), false, 0)),
                    (3, cell(Some(0), true, 0)),
                    (5, cell(Some(0), false, 3)),
                ]),
                bar(vec![(4, cell(None, true, 0))]),
                bar(vec![(6, cell(Some(2), false, 0)), (1, cell(None, true, 0))]),
                bar(vec![
                    (3, cell(Some(0), false, 0)),
                    (2, cell(Some(0), false, -1)),
                ]),
                bar(Vec::new()),
            ],
            loop_range: Some((1, 3)),
            automation: vec![AutoLane {
                param: ParamId::Master(MasterParam::Level),
                points: [(40, 0.5)].into_iter().collect(),
            }],
        };
        v5
    }

    #[test]
    fn v5_bars_convert_to_blocks_exactly() {
        let p = v5_with_song().upgrade();
        let b = |start, slot| Block::new(start, PatternRef(slot));
        assert_eq!(
            p.song.lanes,
            [
                (MachineUid(1), vec![b(0, 1)]),
                (
                    MachineUid(2),
                    vec![Block {
                        transpose: -1,
                        ..b(56, 0)
                    }]
                ),
                (
                    MachineUid(3),
                    vec![
                        Block {
                            len: Some(32),
                            mute: true,
                            ..b(0, 0)
                        },
                        b(56, 0)
                    ]
                ),
                (
                    MachineUid(5),
                    vec![Block {
                        len: Some(32),
                        transpose: 3,
                        ..b(0, 0)
                    }]
                ),
                (MachineUid(6), vec![b(48, 2)]),
            ]
        );
        assert_eq!(p.song.end, Some(32 + 16 + 8 + 16 + 16), "empty bars kept");
        assert_eq!(p.song.loop_range, Some((32, 72)), "bars 1..=3 as steps");
        assert_eq!(p.song.automation, v5_with_song().song.automation);
        p.validate().unwrap();
    }

    #[test]
    fn an_empty_v5_song_converts_to_the_empty_song() {
        assert_eq!(
            ProjectV4::standard().upgrade().upgrade(),
            Project::default()
        );
    }

    #[test]
    fn a_v5_file_loads_converted_and_saves_as_v6() {
        let v5 = v5_with_song();
        let text = ron::ser::to_string_pretty(&v5, ron::ser::PrettyConfig::default()).unwrap();
        assert!(text.contains("version: 5,"));
        let p = Project::from_ron(&text).unwrap();
        assert_eq!(p, v5_with_song().upgrade());
        assert_eq!(p.version, PROJECT_VERSION);
        assert_eq!(Project::from_ron(&p.to_ron()).unwrap(), p);
    }

    #[test]
    fn a_damaged_v5_song_is_invalid_not_a_panic() {
        let load = |v5: &ProjectV5| Project::from_ron(&ron::ser::to_string(v5).unwrap());
        let mut v5 = v5_with_song();
        v5.song.loop_range = Some((2, 5));
        assert!(matches!(load(&v5), Err(ProjectError::Invalid(_))), "loop");
        let mut v5 = v5_with_song();
        v5.song.loop_range = Some((3, 2));
        assert!(matches!(load(&v5), Err(ProjectError::Invalid(_))), "loop");
        let mut v5 = v5_with_song();
        v5.song.bars[0].cells[0].1.pattern = Some(PatternRef(PATTERNS as u8));
        assert!(matches!(load(&v5), Err(ProjectError::Invalid(_))), "ref");
        let mut v5 = v5_with_song();
        v5.song.bars[0]
            .cells
            .push((MachineUid(42), CellV5::default()));
        assert!(matches!(load(&v5), Err(ProjectError::Invalid(_))), "uid");
        let mut v5 = v5_with_song();
        v5.song.bars = vec![BarV5::default(); MAX_BARS + 1];
        v5.song.loop_range = None;
        assert!(matches!(load(&v5), Err(ProjectError::Invalid(_))), "bars");
    }

    #[test]
    fn damaged_v4_step_lists_are_invalid_not_a_panic() {
        for steps in [3, STEPS + 1] {
            let mut v4 = ProjectV4::standard();
            let MachineBodyV4::Synth(s) = &mut v4.machines[1].body else {
                panic!("position 1 is Synth 2")
            };
            s.patterns[5].steps.resize(steps, SynthStep::default());
            match load(&v4) {
                Err(ProjectError::Invalid(e)) => assert_eq!(e, "Synth 2: pattern A06"),
                other => panic!("{steps} synth steps: {other:?}"),
            }
            let mut v4 = ProjectV4::standard();
            let MachineBodyV4::Kit(k) = &mut v4.machines[3].body else {
                panic!("position 3 is the 909")
            };
            k.patterns[0]
                .steps
                .resize(steps, [DrumHit::Off; DRUM_INSTRUMENTS]);
            match load(&v4) {
                Err(ProjectError::Invalid(e)) => assert_eq!(e, "909: pattern A01"),
                other => panic!("{steps} drum steps: {other:?}"),
            }
        }
    }

    #[test]
    fn damaged_v2_step_lists_are_invalid_not_a_panic() {
        let mut v2 = ProjectV2 {
            version: 2,
            name: "Two".into(),
            bpm: 120.0,
            swing: 0.0,
            synths: Default::default(),
            drums: Default::default(),
            fm: FmMachine::default(),
            sampler: SamplerMachine::default(),
            params: BTreeMap::new(),
            selected: [0; 6],
            song: SongV2 {
                bars: Vec::new(),
                loop_range: None,
                automation: Vec::new(),
            },
        };
        v2.synths[0].patterns[0].steps.truncate(3);
        let text = ron::ser::to_string(&v2).unwrap();
        assert!(matches!(
            Project::from_ron(&text),
            Err(ProjectError::Invalid(_))
        ));
    }

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
