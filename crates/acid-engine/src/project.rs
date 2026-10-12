//! Project data: patterns for all six machines, knob values, song. Saved
//! as RON (`.sidacid`). Fixed-size arrays are Vecs of exact length, checked
//! by `validate` on load.

use crate::machine::{MACHINES, MachineKind, TRANSPOSED, kind_of};
use crate::params::{DRUM_INSTRUMENTS, ParamId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub const PROJECT_VERSION: u32 = 2;
pub const STEPS: usize = 64;
pub const PATTERNS: usize = 128;
pub const BANK_SIZE: usize = 16;
/// MIDI note of step note 0 at octave 0 (C2).
pub const BASE_MIDI: i32 = 36;
/// Note-pattern resolution: ticks per 1/16 step.
pub const TICKS_PER_STEP: u16 = 4;
/// Most notes one note pattern may hold.
pub const MAX_NOTES: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Gate {
    #[default]
    Off,
    On,
    /// Continue the previous note without retriggering.
    Tie,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthStep {
    pub note: u8,
    pub octave: i8,
    pub gate: Gate,
    pub accent: bool,
    pub slide: bool,
}

impl SynthStep {
    pub fn midi(&self, transpose: i8) -> u8 {
        (BASE_MIDI + 12 * self.octave as i32 + self.note as i32 + transpose as i32).clamp(0, 127)
            as u8
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamLane {
    pub param: ParamId,
    pub values: Vec<Option<f32>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthPattern {
    pub steps: Vec<SynthStep>,
    pub length: u8,
    pub transpose: i8,
    pub param_lane: Option<ParamLane>,
}

impl SynthPattern {
    /// Shape the engine relies on: 64 steps, length 1..=64, a full-size lane.
    pub fn is_well_formed(&self) -> bool {
        self.steps.len() == STEPS
            && (1..=STEPS as u8).contains(&self.length)
            && self
                .param_lane
                .as_ref()
                .is_none_or(|l| l.values.len() == STEPS)
    }
}

impl Default for SynthPattern {
    fn default() -> Self {
        Self {
            steps: vec![SynthStep::default(); STEPS],
            length: 16,
            transpose: 0,
            param_lane: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DrumHit {
    #[default]
    Off,
    On,
    Accent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrumPattern {
    pub steps: Vec<[DrumHit; DRUM_INSTRUMENTS]>,
    pub length: u8,
}

impl DrumPattern {
    pub fn is_well_formed(&self) -> bool {
        self.steps.len() == STEPS && (1..=STEPS as u8).contains(&self.length)
    }
}

impl Default for DrumPattern {
    fn default() -> Self {
        Self {
            steps: vec![[DrumHit::Off; DRUM_INSTRUMENTS]; STEPS],
            length: 16,
        }
    }
}

/// One note of a poly note pattern, in ticks (`TICKS_PER_STEP` per step).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub start: u16,
    pub len: u16,
    pub pitch: u8,
    pub vel: u8,
}

/// A polyphonic pattern for the FM synth and sampler. Notes are in no
/// particular order; notes starting past `length` are kept but not played.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NotePattern {
    pub notes: Vec<Note>,
    pub length: u8,
}

impl NotePattern {
    /// Pattern length in ticks.
    pub fn ticks(&self) -> u16 {
        (self.length as u16).clamp(1, STEPS as u16) * TICKS_PER_STEP
    }

    pub fn is_well_formed(&self) -> bool {
        let max_start = STEPS as u16 * TICKS_PER_STEP;
        (1..=STEPS as u8).contains(&self.length)
            && self.notes.len() <= MAX_NOTES
            && self.notes.iter().all(|n| {
                n.len >= 1 && n.start < max_start && n.pitch <= 127 && (1..=127).contains(&n.vel)
            })
    }
}

impl Default for NotePattern {
    fn default() -> Self {
        Self {
            notes: Vec::new(),
            length: 16,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FmMachine {
    pub patterns: Vec<NotePattern>,
}

impl Default for FmMachine {
    fn default() -> Self {
        Self {
            patterns: vec![NotePattern::default(); PATTERNS],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SamplerMachine {
    pub patterns: Vec<NotePattern>,
}

impl Default for SamplerMachine {
    fn default() -> Self {
        Self {
            patterns: vec![NotePattern::default(); PATTERNS],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeqView {
    #[default]
    Step303,
    PianoRoll,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthMachine {
    pub patterns: Vec<SynthPattern>,
    pub view: SeqView,
}

impl Default for SynthMachine {
    fn default() -> Self {
        Self {
            patterns: vec![SynthPattern::default(); PATTERNS],
            view: SeqView::Step303,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrumMachine {
    pub patterns: Vec<DrumPattern>,
}

impl Default for DrumMachine {
    fn default() -> Self {
        Self {
            patterns: vec![DrumPattern::default(); PATTERNS],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternRef(pub u8);

impl PatternRef {
    /// "A01" .. "H16".
    pub fn label(self) -> String {
        let bank = (b'A' + self.0 / BANK_SIZE as u8) as char;
        format!("{bank}{:02}", self.0 as usize % BANK_SIZE + 1)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bar {
    pub patterns: [Option<PatternRef>; MACHINES],
    pub mutes: [bool; MACHINES],
    /// Song transpose for Synth 1, Synth 2, FM (`MachineId::transpose_index`).
    pub transpose: [i8; TRANSPOSED],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoLane {
    pub param: ParamId,
    pub points: BTreeMap<u32, f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub bars: Vec<Bar>,
    pub loop_range: Option<(u16, u16)>,
    pub automation: Vec<AutoLane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    pub synths: [SynthMachine; 2],
    pub drums: [DrumMachine; 2],
    pub fm: FmMachine,
    pub sampler: SamplerMachine,
    pub params: BTreeMap<ParamId, f32>,
    /// Selected pattern per machine, by `MachineId::index`.
    pub selected: [u8; MACHINES],
    pub song: Song,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: PROJECT_VERSION,
            name: "Untitled".into(),
            bpm: 125.0,
            swing: 0.0,
            synths: Default::default(),
            drums: Default::default(),
            fm: FmMachine::default(),
            sampler: SamplerMachine::default(),
            params: ParamId::all()
                .into_iter()
                .map(|id| (id, id.info().default))
                .collect(),
            selected: [0; MACHINES],
            song: Song::default(),
        }
    }
}

#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Parse(String),
    TooNew(u32),
    Invalid(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "file error: {e}"),
            ProjectError::Parse(e) => write!(f, "not a valid SidAcid project: {e}"),
            ProjectError::TooNew(v) => write!(
                f,
                "project version {v} is newer than this SidAcid ({PROJECT_VERSION})"
            ),
            ProjectError::Invalid(e) => write!(f, "project is damaged: {e}"),
        }
    }
}

impl std::error::Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        ProjectError::Io(e)
    }
}

#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

impl Project {
    /// Note patterns of the FM synth (machine 4) or sampler (machine 5).
    pub fn note_patterns(&self, machine: usize) -> Option<&Vec<NotePattern>> {
        match kind_of(machine)? {
            MachineKind::Notes(0) => Some(&self.fm.patterns),
            MachineKind::Notes(_) => Some(&self.sampler.patterns),
            _ => None,
        }
    }

    pub fn note_patterns_mut(&mut self, machine: usize) -> Option<&mut Vec<NotePattern>> {
        match kind_of(machine)? {
            MachineKind::Notes(0) => Some(&mut self.fm.patterns),
            MachineKind::Notes(_) => Some(&mut self.sampler.patterns),
            _ => None,
        }
    }

    pub fn param(&self, id: ParamId) -> f32 {
        self.params.get(&id).copied().unwrap_or(id.info().default)
    }

    pub fn validate(&self) -> Result<(), ProjectError> {
        let bad = |m: String| Err(ProjectError::Invalid(m));
        if !self.bpm.is_finite() || !self.swing.is_finite() {
            return bad("bpm or swing is not a number".into());
        }
        if self
            .params
            .iter()
            .any(|(id, v)| !id.is_valid() || !v.is_finite())
        {
            return bad("invalid parameter value".into());
        }
        for (i, m) in self.synths.iter().enumerate() {
            if m.patterns.len() != PATTERNS {
                return bad(format!("synth {} has {} patterns", i + 1, m.patterns.len()));
            }
            for (j, p) in m.patterns.iter().enumerate() {
                if !p.is_well_formed() {
                    return bad(format!(
                        "synth {} pattern {}",
                        i + 1,
                        PatternRef(j as u8).label()
                    ));
                }
                if let Some(lane) = &p.param_lane
                    && (!lane.param.is_valid()
                        || lane.values.iter().flatten().any(|v| !v.is_finite()))
                {
                    return bad(format!(
                        "synth {} pattern {} param lane",
                        i + 1,
                        PatternRef(j as u8).label()
                    ));
                }
            }
        }
        for (i, m) in self.drums.iter().enumerate() {
            if m.patterns.len() != PATTERNS {
                return bad(format!(
                    "drum machine {} has {} patterns",
                    i + 1,
                    m.patterns.len()
                ));
            }
            for (j, p) in m.patterns.iter().enumerate() {
                if !p.is_well_formed() {
                    return bad(format!(
                        "drum machine {} pattern {}",
                        i + 1,
                        PatternRef(j as u8).label()
                    ));
                }
            }
        }
        for (name, pats) in [
            ("FM", &self.fm.patterns),
            ("Sampler", &self.sampler.patterns),
        ] {
            if pats.len() != PATTERNS {
                return bad(format!("{name} has {} patterns", pats.len()));
            }
            if let Some(j) = pats.iter().position(|p| !p.is_well_formed()) {
                return bad(format!("{name} pattern {}", PatternRef(j as u8).label()));
            }
        }
        if self.selected.iter().any(|&s| s as usize >= PATTERNS) {
            return bad("selected pattern out of range".into());
        }
        if self
            .song
            .bars
            .iter()
            .flat_map(|b| b.patterns)
            .flatten()
            .any(|r| r.0 as usize >= PATTERNS)
        {
            return bad("song references a pattern out of range".into());
        }
        if self.song.bars.len() > crate::song::MAX_BARS {
            return bad(format!("song has {} bars", self.song.bars.len()));
        }
        if let Some((a, b)) = self.song.loop_range
            && (a > b || b as usize >= self.song.bars.len())
        {
            return bad("song loop range is out of bounds".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for lane in &self.song.automation {
            if !lane.param.is_valid() || !seen.insert(lane.param) {
                return bad("song automation lane is invalid or repeated".into());
            }
            if lane
                .points
                .values()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return bad("song automation value is out of range".into());
            }
        }
        Ok(())
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("project serialises")
    }

    pub fn from_ron(text: &str) -> Result<Project, ProjectError> {
        let parse = |e: ron::error::SpannedError| ProjectError::Parse(e.to_string());
        let head: VersionOnly = ron::from_str(text).map_err(parse)?;
        if head.version > PROJECT_VERSION {
            return Err(ProjectError::TooNew(head.version));
        }
        let p = if head.version < PROJECT_VERSION {
            ron::from_str::<crate::migrate::ProjectV1>(text)
                .map_err(parse)?
                .upgrade()
        } else {
            ron::from_str::<Project>(text).map_err(parse)?
        };
        p.validate()?;
        Ok(p)
    }

    /// Writes `path.tmp` then renames over `path`, so a crash never leaves
    /// a half-written project.
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        let tmp = path.with_extension("sidacid.tmp");
        std::fs::write(&tmp, self.to_ron())?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Project, ProjectError> {
        Project::from_ron(&std::fs::read_to_string(path)?)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one field of a default project
mod tests {
    use super::*;
    use crate::params::SynthParam;

    #[test]
    fn default_project_shape() {
        let p = Project::default();
        assert_eq!(p.version, PROJECT_VERSION);
        assert_eq!(p.synths[0].patterns.len(), PATTERNS);
        assert_eq!(p.drums[1].patterns.len(), PATTERNS);
        assert_eq!(p.synths[0].patterns[0].steps.len(), STEPS);
        assert_eq!(p.drums[0].patterns[0].steps.len(), STEPS);
        assert_eq!(p.synths[0].patterns[0].length, 16);
        assert_eq!(p.params.len(), ParamId::all().len());
        p.validate().unwrap();
    }

    #[test]
    fn step_midi_uses_octave_note_and_transpose() {
        let s = SynthStep {
            note: 3,
            octave: 1,
            gate: Gate::On,
            accent: false,
            slide: false,
        };
        assert_eq!(s.midi(0), 36 + 12 + 3);
        assert_eq!(s.midi(-2), 36 + 12 + 1);
        let low = SynthStep {
            note: 0,
            octave: -1,
            ..s
        };
        assert_eq!(low.midi(0), 24);
    }

    #[test]
    fn pattern_ref_labels() {
        assert_eq!(PatternRef(0).label(), "A01");
        assert_eq!(PatternRef(37).label(), "C06");
        assert_eq!(PatternRef(127).label(), "H16");
    }

    #[test]
    fn param_lookup_falls_back_to_default() {
        let mut p = Project::default();
        let id = ParamId::Synth(1, SynthParam::Cutoff);
        p.params.remove(&id);
        assert_eq!(p.param(id), id.info().default);
        p.params.insert(id, 0.9);
        assert_eq!(p.param(id), 0.9);
    }

    #[test]
    fn ron_round_trip() {
        let mut p = Project::default();
        p.name = "Test".into();
        p.bpm = 133.0;
        p.synths[1].patterns[5].steps[3] = SynthStep {
            note: 7,
            octave: -1,
            gate: Gate::On,
            accent: true,
            slide: true,
        };
        p.drums[0].patterns[2].steps[4][0] = DrumHit::Accent;
        p.params
            .insert(ParamId::Synth(0, SynthParam::Resonance), 0.95);
        p.song.bars.push(Bar {
            patterns: [
                Some(PatternRef(5)),
                None,
                Some(PatternRef(2)),
                None,
                Some(PatternRef(9)),
                None,
            ],
            mutes: [false, true, false, false, false, true],
            transpose: [0, 3, -5],
        });
        p.fm.patterns[9].notes.push(Note {
            start: 3,
            len: 6,
            pitch: 64,
            vel: 90,
        });
        let text = p.to_ron();
        let back = Project::from_ron(&text).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn newer_version_is_rejected() {
        let mut p = Project::default();
        p.version = PROJECT_VERSION + 1;
        let text = p.to_ron();
        match Project::from_ron(&text) {
            Err(ProjectError::TooNew(v)) => assert_eq!(v, PROJECT_VERSION + 1),
            other => panic!("expected TooNew, got {other:?}"),
        }
    }

    #[test]
    fn invalid_shapes_are_rejected() {
        let mut p = Project::default();
        p.synths[0].patterns[0].length = 0;
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.drums[1].patterns.pop();
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.selected[2] = 200;
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn non_finite_and_bad_index_values_are_rejected() {
        let mut p = Project::default();
        p.bpm = f32::NAN;
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.swing = f32::INFINITY;
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.params
            .insert(ParamId::Synth(0, SynthParam::Cutoff), f32::NAN);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.params.insert(ParamId::Synth(5, SynthParam::Cutoff), 0.5);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        let mut lane = ParamLane {
            param: ParamId::Drum(0, 9, crate::params::DrumParam::Tune),
            values: vec![None; STEPS],
        };
        p.synths[0].patterns[0].param_lane = Some(lane.clone());
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        lane.param = ParamId::Synth(0, SynthParam::Cutoff);
        lane.values[3] = Some(f32::NAN);
        p.synths[0].patterns[0].param_lane = Some(lane);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn garbage_is_a_parse_error() {
        assert!(matches!(
            Project::from_ron("not ron"),
            Err(ProjectError::Parse(_))
        ));
    }

    #[test]
    fn save_and_load_file() {
        let dir = std::env::temp_dir().join(format!("sidacid-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.sidacid");
        let mut p = Project::default();
        p.name = "Saved".into();
        p.save(&path).unwrap();
        assert!(!path.with_extension("sidacid.tmp").exists());
        let back = Project::load(&path).unwrap();
        assert_eq!(back.name, "Saved");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn song_shapes_are_validated() {
        use crate::params::SynthParam;
        let mut p = Project::default();
        p.song.bars = vec![Bar::default(); crate::song::MAX_BARS + 1];
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));

        let mut p = Project::default();
        p.song.bars = vec![Bar::default(); 3];
        p.song.loop_range = Some((2, 1));
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        p.song.loop_range = Some((1, 3));
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        p.song.loop_range = Some((1, 2));
        p.validate().unwrap();

        let id = ParamId::Synth(0, SynthParam::Cutoff);
        let mut p = Project::default();
        let lane = AutoLane {
            param: id,
            points: [(0, 0.5)].into_iter().collect(),
        };
        p.song.automation = vec![lane.clone(), lane.clone()];
        assert!(
            matches!(p.validate(), Err(ProjectError::Invalid(_))),
            "duplicate lane"
        );
        p.song.automation = vec![AutoLane {
            param: ParamId::Synth(7, SynthParam::Cutoff),
            ..lane.clone()
        }];
        assert!(
            matches!(p.validate(), Err(ProjectError::Invalid(_))),
            "invalid param"
        );
        p.song.automation = vec![AutoLane {
            param: id,
            points: [(0, f32::NAN)].into_iter().collect(),
        }];
        assert!(
            matches!(p.validate(), Err(ProjectError::Invalid(_))),
            "NaN point"
        );
        p.song.automation = vec![AutoLane {
            param: id,
            points: [(0, 1.5)].into_iter().collect(),
        }];
        assert!(
            matches!(p.validate(), Err(ProjectError::Invalid(_))),
            "out-of-range point"
        );
    }

    #[test]
    fn note_patterns_validate() {
        let mut p = NotePattern::default();
        assert!(p.is_well_formed());
        assert_eq!(p.ticks(), 16 * TICKS_PER_STEP);
        p.notes.push(Note {
            start: 0,
            len: 4,
            pitch: 60,
            vel: 100,
        });
        assert!(p.is_well_formed());
        p.length = 4;
        p.notes.push(Note {
            start: 200,
            len: 1,
            pitch: 60,
            vel: 1,
        });
        assert!(p.is_well_formed(), "notes past a shortened end are kept");
        let mut bad = p.clone();
        bad.notes.push(Note {
            start: 0,
            len: 0,
            pitch: 60,
            vel: 100,
        });
        assert!(!bad.is_well_formed(), "zero length");
        let mut bad = p.clone();
        bad.notes.push(Note {
            start: 0,
            len: 1,
            pitch: 60,
            vel: 0,
        });
        assert!(!bad.is_well_formed(), "zero velocity");
        let mut bad = p.clone();
        bad.notes.push(Note {
            start: 0,
            len: 1,
            pitch: 128,
            vel: 1,
        });
        assert!(!bad.is_well_formed(), "pitch above 127");
        let mut bad = p.clone();
        bad.notes.push(Note {
            start: 256,
            len: 1,
            pitch: 1,
            vel: 1,
        });
        assert!(!bad.is_well_formed(), "start past 64 steps");
        let mut bad = NotePattern::default();
        bad.notes = vec![
            Note {
                start: 0,
                len: 1,
                pitch: 1,
                vel: 1
            };
            MAX_NOTES + 1
        ];
        assert!(!bad.is_well_formed(), "over the note cap");
    }

    #[test]
    fn note_machines_are_reachable_by_machine_index() {
        let mut p = Project::default();
        assert_eq!(p.fm.patterns.len(), PATTERNS);
        assert_eq!(p.sampler.patterns.len(), PATTERNS);
        assert!(p.note_patterns(0).is_none());
        p.note_patterns_mut(5).unwrap()[3].length = 8;
        assert_eq!(p.sampler.patterns[3].length, 8);
        assert_eq!(p.note_patterns(4).unwrap().len(), PATTERNS);
        p.fm.patterns[1].notes.push(Note {
            start: 0,
            len: 1,
            pitch: 60,
            vel: 0,
        });
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn v1_files_load_with_empty_new_machines() {
        use crate::migrate::{BarV1, ProjectV1, SongV1};
        let base = Project::default();
        let mut params = BTreeMap::new();
        params.insert(ParamId::Synth(0, SynthParam::Cutoff), 0.9);
        let mut synths = base.synths.clone();
        synths[1].patterns[2].length = 7;
        let v1 = ProjectV1 {
            version: 1,
            name: "Old".into(),
            bpm: 131.0,
            swing: 0.2,
            synths,
            drums: base.drums.clone(),
            params,
            selected: [1, 2, 3, 4],
            song: SongV1 {
                bars: vec![BarV1 {
                    patterns: [Some(PatternRef(1)), None, Some(PatternRef(3)), None],
                    mutes: [true, false, false, true],
                    transpose: [2, -3],
                }],
                loop_range: Some((0, 0)),
                automation: Vec::new(),
            },
        };
        let text = ron::ser::to_string_pretty(&v1, ron::ser::PrettyConfig::default()).unwrap();
        let p = Project::from_ron(&text).unwrap();
        assert_eq!(p.version, PROJECT_VERSION);
        assert_eq!((p.name.as_str(), p.bpm, p.swing), ("Old", 131.0, 0.2));
        assert_eq!(p.synths[1].patterns[2].length, 7);
        assert_eq!(p.selected, [1, 2, 3, 4, 0, 0]);
        assert_eq!(p.param(ParamId::Synth(0, SynthParam::Cutoff)), 0.9);
        let ch5 = ParamId::Channel(5, crate::params::ChannelParam::Level);
        assert_eq!(p.params.get(&ch5), Some(&ch5.info().default));
        let bar = p.song.bars[0];
        assert_eq!(
            bar.patterns,
            [
                Some(PatternRef(1)),
                None,
                Some(PatternRef(3)),
                None,
                None,
                None
            ]
        );
        assert_eq!(bar.mutes, [true, false, false, true, false, false]);
        assert_eq!(bar.transpose, [2, -3, 0]);
        assert_eq!(p.song.loop_range, Some((0, 0)));
        assert_eq!(p.fm.patterns.len(), PATTERNS);
        assert!(p.sampler.patterns.iter().all(|n| n.notes.is_empty()));
    }
}
