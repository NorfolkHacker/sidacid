//! Project data: patterns for all four machines, knob values, song. Saved
//! as RON (`.sidacid`). Fixed-size arrays are Vecs of exact length, checked
//! by `validate` on load.

use crate::params::{ParamId, DRUM_INSTRUMENTS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub const PROJECT_VERSION: u32 = 1;
pub const STEPS: usize = 64;
pub const PATTERNS: usize = 128;
pub const BANK_SIZE: usize = 16;
/// MIDI note of step note 0 at octave 0 (C2).
pub const BASE_MIDI: i32 = 36;

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
        (BASE_MIDI + 12 * self.octave as i32 + self.note as i32 + transpose as i32).clamp(0, 127) as u8
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

impl Default for SynthPattern {
    fn default() -> Self {
        Self { steps: vec![SynthStep::default(); STEPS], length: 16, transpose: 0, param_lane: None }
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

impl Default for DrumPattern {
    fn default() -> Self {
        Self { steps: vec![[DrumHit::Off; DRUM_INSTRUMENTS]; STEPS], length: 16 }
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
        Self { patterns: vec![SynthPattern::default(); PATTERNS], view: SeqView::Step303 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrumMachine {
    pub patterns: Vec<DrumPattern>,
}

impl Default for DrumMachine {
    fn default() -> Self {
        Self { patterns: vec![DrumPattern::default(); PATTERNS] }
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
    pub patterns: [Option<PatternRef>; 4],
    pub mutes: [bool; 4],
    pub transpose: [i8; 2],
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
    pub params: BTreeMap<ParamId, f32>,
    /// Selected pattern per machine: synth 1, synth 2, 808, 909.
    pub selected: [u8; 4],
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
            params: ParamId::all().into_iter().map(|id| (id, id.info().default)).collect(),
            selected: [0; 4],
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
            ProjectError::TooNew(v) => write!(f, "project version {v} is newer than this SidAcid ({PROJECT_VERSION})"),
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
    pub fn param(&self, id: ParamId) -> f32 {
        self.params.get(&id).copied().unwrap_or(id.info().default)
    }

    pub fn validate(&self) -> Result<(), ProjectError> {
        let bad = |m: String| Err(ProjectError::Invalid(m));
        for (i, m) in self.synths.iter().enumerate() {
            if m.patterns.len() != PATTERNS {
                return bad(format!("synth {} has {} patterns", i + 1, m.patterns.len()));
            }
            for (j, p) in m.patterns.iter().enumerate() {
                if p.steps.len() != STEPS || !(1..=STEPS as u8).contains(&p.length) {
                    return bad(format!("synth {} pattern {}", i + 1, PatternRef(j as u8).label()));
                }
                if let Some(lane) = &p.param_lane {
                    if lane.values.len() != STEPS {
                        return bad(format!("synth {} pattern {} param lane", i + 1, PatternRef(j as u8).label()));
                    }
                }
            }
        }
        for (i, m) in self.drums.iter().enumerate() {
            if m.patterns.len() != PATTERNS {
                return bad(format!("drum machine {} has {} patterns", i + 1, m.patterns.len()));
            }
            for (j, p) in m.patterns.iter().enumerate() {
                if p.steps.len() != STEPS || !(1..=STEPS as u8).contains(&p.length) {
                    return bad(format!("drum machine {} pattern {}", i + 1, PatternRef(j as u8).label()));
                }
            }
        }
        if self.selected.iter().any(|&s| s as usize >= PATTERNS) {
            return bad("selected pattern out of range".into());
        }
        if self.song.bars.iter().flat_map(|b| b.patterns).flatten().any(|r| r.0 as usize >= PATTERNS) {
            return bad("song references a pattern out of range".into());
        }
        Ok(())
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("project serialises")
    }

    pub fn from_ron(text: &str) -> Result<Project, ProjectError> {
        let head: VersionOnly = ron::from_str(text).map_err(|e| ProjectError::Parse(e.to_string()))?;
        if head.version > PROJECT_VERSION {
            return Err(ProjectError::TooNew(head.version));
        }
        let p: Project = ron::from_str(text).map_err(|e| ProjectError::Parse(e.to_string()))?;
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
        let s = SynthStep { note: 3, octave: 1, gate: Gate::On, accent: false, slide: false };
        assert_eq!(s.midi(0), 36 + 12 + 3);
        assert_eq!(s.midi(-2), 36 + 12 + 1);
        let low = SynthStep { note: 0, octave: -1, ..s };
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
        p.synths[1].patterns[5].steps[3] = SynthStep { note: 7, octave: -1, gate: Gate::On, accent: true, slide: true };
        p.drums[0].patterns[2].steps[4][0] = DrumHit::Accent;
        p.params.insert(ParamId::Synth(0, SynthParam::Resonance), 0.95);
        p.song.bars.push(Bar { patterns: [Some(PatternRef(5)), None, Some(PatternRef(2)), None], mutes: [false, true, false, false], transpose: [0, 3] });
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
    fn garbage_is_a_parse_error() {
        assert!(matches!(Project::from_ron("not ron"), Err(ProjectError::Parse(_))));
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
}
