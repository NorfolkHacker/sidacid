//! Project data: the rack of machines (each with its patterns, sound and
//! knob values), the global knobs and the song. Saved as RON
//! (`.sidacid`). Fixed-size arrays are Vecs of exact length, checked by
//! `validate` on load.

use crate::drums::KitKind;
use crate::machine::{MAX_MACHINES, MachineType, MachineUid, standard_rack};
use crate::params::{DRUM_INSTRUMENTS, GlobalParam, MachineParam, ParamId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub const PROJECT_VERSION: u32 = 3;
pub const STEPS: usize = 64;

/// A pattern's `length` field as a step count in 1..=STEPS. Patterns are
/// validated on load and swap, but the fields are pub: this stays defensive.
pub fn steps_of(length: u8) -> usize {
    (length as usize).clamp(1, STEPS)
}
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
    /// Pattern length in ticks. The clamp is defensive (the fields are
    /// pub, so a value may skip `is_well_formed`); never a panic here: this
    /// runs on the audio thread.
    pub fn ticks(&self) -> u16 {
        steps_of(self.length) as u16 * TICKS_PER_STEP
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
    /// Name of the voice the FM params were last loaded from.
    #[serde(default = "init_voice_name")]
    pub voice_name: String,
    /// The last imported DX7 bank, shown in the voice browser.
    #[serde(default)]
    pub bank: Option<Vec<crate::fm::sysex::Dx7Voice>>,
}

fn init_voice_name() -> String {
    "INIT VOICE".into()
}

impl Default for FmMachine {
    fn default() -> Self {
        Self {
            patterns: vec![NotePattern::default(); PATTERNS],
            voice_name: init_voice_name(),
            bank: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SamplerMachine {
    pub patterns: Vec<NotePattern>,
    /// One entry per slot; `None` = empty.
    #[serde(default = "empty_slots", deserialize_with = "at_most_slots")]
    pub slots: Vec<Option<crate::sampler::slot::SlotData>>,
}

/// Reads the slot list, refusing more than `SLOTS` entries before any
/// extra entry's audio is decoded.
fn at_most_slots<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<Option<crate::sampler::slot::SlotData>>, D::Error> {
    use crate::sampler::SLOTS;
    use serde::de::{Error, IgnoredAny, SeqAccess, Visitor};
    struct Slots;
    impl<'de> Visitor<'de> for Slots {
        type Value = Vec<Option<crate::sampler::slot::SlotData>>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "at most {SLOTS} sample slots")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut slots = Vec::with_capacity(SLOTS);
            while slots.len() < SLOTS {
                match seq.next_element()? {
                    Some(slot) => slots.push(slot),
                    None => return Ok(slots),
                }
            }
            if seq.next_element::<IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(SLOTS + 1, &self));
            }
            Ok(slots)
        }
    }
    d.deserialize_seq(Slots)
}

fn empty_slots() -> Vec<Option<crate::sampler::slot::SlotData>> {
    vec![None; crate::sampler::SLOTS]
}

impl Default for SamplerMachine {
    fn default() -> Self {
        Self {
            patterns: vec![NotePattern::default(); PATTERNS],
            slots: empty_slots(),
        }
    }
}

impl SamplerMachine {
    fn range(&self, slot: usize) -> Option<(u8, u8)> {
        self.slots.get(slot)?.as_ref().map(|d| d.meta.key_range())
    }

    /// The first loaded slot whose zone contains `key`.
    pub fn slot_for_key(&self, key: u8) -> Option<usize> {
        (0..self.slots.len()).find(|&i| {
            self.range(i)
                .is_some_and(|(lo, hi)| (lo..=hi).contains(&key))
        })
    }

    /// True when `meta`'s zone would overlap another loaded slot's zone.
    pub fn zone_clash(&self, slot: usize, meta: &crate::sampler::slot::SlotMeta) -> bool {
        let (lo, hi) = meta.key_range();
        (0..self.slots.len())
            .filter(|&i| i != slot)
            .filter_map(|i| self.range(i))
            .any(|(a, b)| lo <= b && a <= hi)
    }

    /// True when the decoded audio fits in `limit` bytes.
    pub fn memory_ok(&self, limit: usize) -> bool {
        self.memory_bytes() <= limit
    }

    /// Decoded audio the engine will hold, in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.slots.iter().flatten().map(|d| d.memory_bytes()).sum()
    }

    /// Lowest key from C1 up where a `span`-key zone fits without
    /// overlapping another slot's zone (`slot`'s own zone is ignored).
    pub fn free_base_key(&self, slot: usize, span: u8) -> Option<u8> {
        let span = span.max(1) as u16;
        (crate::sampler::FIRST_KEY as u16..=128u16.checked_sub(span)?)
            .map(|k| k as u8)
            .find(|&k| {
                let hi = (k as u16 + span - 1) as u8;
                !(0..self.slots.len())
                    .filter(|&i| i != slot)
                    .filter_map(|i| self.range(i))
                    .any(|(a, b)| k <= b && a <= hi)
            })
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
    /// Which drum kit plays the patterns.
    pub kind: KitKind,
    pub patterns: Vec<DrumPattern>,
}

impl DrumMachine {
    /// A kit of `kind` with empty patterns.
    pub fn new(kind: KitKind) -> Self {
        Self {
            kind,
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

/// One machine's slot in a song bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub pattern: Option<PatternRef>,
    pub mute: bool,
    /// Song transpose; only machine types that transpose use it.
    pub transpose: i8,
}

/// A song bar: one cell per machine, by uid. A machine with no cell
/// plays nothing (a default cell).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bar {
    pub cells: Vec<(MachineUid, Cell)>,
}

impl Bar {
    /// `uid`'s cell; the default (empty) cell if the bar has none.
    pub fn cell(&self, uid: MachineUid) -> Cell {
        self.cells
            .iter()
            .find(|(u, _)| *u == uid)
            .map_or_else(Cell::default, |(_, c)| *c)
    }

    /// `uid`'s cell, added as a default cell if the bar has none.
    pub fn cell_mut(&mut self, uid: MachineUid) -> &mut Cell {
        let i = match self.cells.iter().position(|(u, _)| *u == uid) {
            Some(i) => i,
            None => {
                self.cells.push((uid, Cell::default()));
                self.cells.len() - 1
            }
        };
        &mut self.cells[i].1
    }
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

/// A machine's sound data and patterns, by type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MachineBody {
    Synth(SynthMachine),
    Kit(DrumMachine),
    Fm(FmMachine),
    Sampler(SamplerMachine),
}

impl MachineBody {
    /// An empty machine of `ty`.
    pub fn new(ty: MachineType) -> Self {
        match ty {
            MachineType::Synth => MachineBody::Synth(SynthMachine::default()),
            MachineType::Kit808 => MachineBody::Kit(DrumMachine::new(KitKind::K808)),
            MachineType::Kit909 => MachineBody::Kit(DrumMachine::new(KitKind::K909)),
            MachineType::Fm => MachineBody::Fm(FmMachine::default()),
            MachineType::Sampler => MachineBody::Sampler(SamplerMachine::default()),
        }
    }

    pub fn machine_type(&self) -> MachineType {
        match self {
            MachineBody::Synth(_) => MachineType::Synth,
            MachineBody::Kit(k) => match k.kind {
                KitKind::K808 => MachineType::Kit808,
                KitKind::K909 => MachineType::Kit909,
            },
            MachineBody::Fm(_) => MachineType::Fm,
            MachineBody::Sampler(_) => MachineType::Sampler,
        }
    }
}

/// One machine in the rack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineEntry {
    pub uid: MachineUid,
    pub name: String,
    /// Selected pattern.
    pub selected: u8,
    /// Channel strip + type params.
    pub params: BTreeMap<MachineParam, f32>,
    pub body: MachineBody,
}

impl MachineEntry {
    /// An empty machine of `ty` with every param at its default.
    pub fn new(uid: MachineUid, ty: MachineType, name: String) -> Self {
        Self {
            uid,
            name,
            selected: 0,
            params: MachineParam::all_for(ty)
                .into_iter()
                .map(|p| (p, p.info().default))
                .collect(),
            body: MachineBody::new(ty),
        }
    }

    pub fn machine_type(&self) -> MachineType {
        self.body.machine_type()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub name: String,
    pub bpm: f32,
    pub swing: f32,
    /// The rack, in order.
    pub machines: Vec<MachineEntry>,
    /// Uid for the next machine added; above every uid ever used.
    pub next_uid: u16,
    /// Send effects and master bus.
    pub globals: BTreeMap<GlobalParam, f32>,
    pub song: Song,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: PROJECT_VERSION,
            name: "Untitled".into(),
            bpm: 125.0,
            swing: 0.0,
            machines: standard_rack()
                .into_iter()
                .map(|(uid, ty, name)| MachineEntry::new(uid, ty, name.into()))
                .collect(),
            next_uid: 7,
            globals: GlobalParam::all()
                .into_iter()
                .map(|g| (g, g.info().default))
                .collect(),
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

/// Parses with embedded 16-bit PCM limited to half the f32 memory limit.
fn with_pcm_budget<T>(f: impl FnOnce() -> T) -> T {
    let budget = crate::sampler::MEMORY_LIMIT as u64 / 2;
    crate::sampler::slot::pcm_codec::with_budget(budget, f)
}

#[derive(Deserialize)]
struct VersionOnly {
    version: u32,
}

macro_rules! body_accessors {
    ($get:ident, $get_mut:ident, $variant:ident, $ty:ty) => {
        pub fn $get(&self, uid: MachineUid) -> Option<&$ty> {
            match &self.machine(uid)?.body {
                MachineBody::$variant(m) => Some(m),
                _ => None,
            }
        }

        pub fn $get_mut(&mut self, uid: MachineUid) -> Option<&mut $ty> {
            match &mut self.machine_mut(uid)?.body {
                MachineBody::$variant(m) => Some(m),
                _ => None,
            }
        }
    };
}

impl Project {
    pub fn machine(&self, uid: MachineUid) -> Option<&MachineEntry> {
        self.machines.iter().find(|m| m.uid == uid)
    }

    pub fn machine_mut(&mut self, uid: MachineUid) -> Option<&mut MachineEntry> {
        self.machines.iter_mut().find(|m| m.uid == uid)
    }

    /// The machines' uids in rack order: what a song compiles against.
    pub fn rack_uids(&self) -> Vec<MachineUid> {
        self.machines.iter().map(|m| m.uid).collect()
    }

    /// `uid`'s position in the rack.
    pub fn position(&self, uid: MachineUid) -> Option<usize> {
        self.machines.iter().position(|m| m.uid == uid)
    }

    body_accessors!(synth, synth_mut, Synth, SynthMachine);
    body_accessors!(kit, kit_mut, Kit, DrumMachine);
    body_accessors!(fm, fm_mut, Fm, FmMachine);
    body_accessors!(sampler, sampler_mut, Sampler, SamplerMachine);

    /// Note patterns of an FM or Sampler machine.
    pub fn note_patterns(&self, uid: MachineUid) -> Option<&Vec<NotePattern>> {
        match &self.machine(uid)?.body {
            MachineBody::Fm(m) => Some(&m.patterns),
            MachineBody::Sampler(m) => Some(&m.patterns),
            _ => None,
        }
    }

    pub fn note_patterns_mut(&mut self, uid: MachineUid) -> Option<&mut Vec<NotePattern>> {
        match &mut self.machine_mut(uid)?.body {
            MachineBody::Fm(m) => Some(&mut m.patterns),
            MachineBody::Sampler(m) => Some(&mut m.patterns),
            _ => None,
        }
    }

    /// The stored value, or the param's default when none is stored.
    pub fn param(&self, id: ParamId) -> f32 {
        let stored = match id {
            ParamId::Machine(uid, p) => self.machine(uid).and_then(|m| m.params.get(&p)),
            ParamId::Fx(_) | ParamId::Master(_) => id.global().and_then(|g| self.globals.get(&g)),
        };
        stored.copied().unwrap_or(id.info().default)
    }

    /// Stores `v`; ignored for a uid not in the rack or a param that
    /// doesn't fit the machine's type.
    pub fn set_param(&mut self, id: ParamId, v: f32) {
        match id {
            ParamId::Machine(uid, p) => {
                if let Some(m) = self.machine_mut(uid)
                    && p.fits(m.machine_type())
                {
                    m.params.insert(p, v);
                }
            }
            ParamId::Fx(_) | ParamId::Master(_) => {
                if let Some(g) = id.global() {
                    self.globals.insert(g, v);
                }
            }
        }
    }

    /// True when `id` names a global or a machine in the rack whose type
    /// it fits.
    fn param_in_rack(&self, id: ParamId) -> bool {
        match id {
            ParamId::Machine(uid, p) => self.machine(uid).is_some_and(|m| p.fits(m.machine_type())),
            ParamId::Fx(_) | ParamId::Master(_) => true,
        }
    }

    pub fn validate(&self) -> Result<(), ProjectError> {
        let bad = |m: String| Err(ProjectError::Invalid(m));
        if !self.bpm.is_finite() || !self.swing.is_finite() {
            return bad("bpm or swing is not a number".into());
        }
        if self.machines.len() > MAX_MACHINES {
            return bad(format!("rack has {} machines", self.machines.len()));
        }
        let mut uids = std::collections::BTreeSet::new();
        if !self.machines.iter().all(|m| uids.insert(m.uid)) {
            return bad("two machines share a uid".into());
        }
        if uids.last().is_some_and(|u| self.next_uid <= u.0) {
            return bad("next machine uid is already in use".into());
        }
        if self.globals.values().any(|v| !v.is_finite()) {
            return bad("invalid parameter value".into());
        }
        let mut sampler_bytes = 0usize;
        for m in &self.machines {
            self.validate_machine(m)?;
            if let MachineBody::Sampler(s) = &m.body {
                sampler_bytes = sampler_bytes.saturating_add(s.memory_bytes());
            }
        }
        if sampler_bytes > crate::sampler::MEMORY_LIMIT {
            return bad("sampler audio exceeds 64 MB".into());
        }
        for bar in &self.song.bars {
            for (i, (uid, cell)) in bar.cells.iter().enumerate() {
                if self.machine(*uid).is_none() {
                    return bad("song bar names a machine not in the rack".into());
                }
                if bar.cells[..i].iter().any(|(u, _)| u == uid) {
                    return bad("song bar names a machine twice".into());
                }
                if cell.pattern.is_some_and(|r| r.0 as usize >= PATTERNS) {
                    return bad("song references a pattern out of range".into());
                }
            }
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
            if !self.param_in_rack(lane.param) || !seen.insert(lane.param) {
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

    /// Checks one machine's params, selection and body.
    fn validate_machine(&self, m: &MachineEntry) -> Result<(), ProjectError> {
        let bad = |msg: String| Err(ProjectError::Invalid(format!("{}: {msg}", m.name)));
        let ty = m.machine_type();
        if m.params.iter().any(|(p, v)| !p.fits(ty) || !v.is_finite()) {
            return bad("invalid parameter value".into());
        }
        if m.selected as usize >= PATTERNS {
            return bad("selected pattern out of range".into());
        }
        let count = match &m.body {
            MachineBody::Synth(s) => s.patterns.len(),
            MachineBody::Kit(k) => k.patterns.len(),
            MachineBody::Fm(f) => f.patterns.len(),
            MachineBody::Sampler(s) => s.patterns.len(),
        };
        if count != PATTERNS {
            return bad(format!("{count} patterns"));
        }
        let label = |j: usize| PatternRef(j as u8).label();
        match &m.body {
            MachineBody::Synth(s) => {
                for (j, p) in s.patterns.iter().enumerate() {
                    if !p.is_well_formed() {
                        return bad(format!("pattern {}", label(j)));
                    }
                    if let Some(lane) = &p.param_lane
                        && (!self.param_in_rack(lane.param)
                            || lane.values.iter().flatten().any(|v| !v.is_finite()))
                    {
                        return bad(format!("pattern {} param lane", label(j)));
                    }
                }
            }
            MachineBody::Kit(k) => {
                if let Some(j) = k.patterns.iter().position(|p| !p.is_well_formed()) {
                    return bad(format!("pattern {}", label(j)));
                }
            }
            MachineBody::Fm(f) => {
                if let Some(j) = f.patterns.iter().position(|p| !p.is_well_formed()) {
                    return bad(format!("pattern {}", label(j)));
                }
                if let Some(bank) = &f.bank
                    && (bank.len() > crate::fm::sysex::BANK_VOICES
                        || bank.iter().any(|v| !v.is_valid()))
                {
                    return bad("FM voice bank".into());
                }
            }
            MachineBody::Sampler(s) => {
                if let Some(j) = s.patterns.iter().position(|p| !p.is_well_formed()) {
                    return bad(format!("pattern {}", label(j)));
                }
                if s.slots.len() != crate::sampler::SLOTS {
                    return bad(format!("{} slots", s.slots.len()));
                }
                for (i, slot) in s.slots.iter().enumerate() {
                    if let Some(d) = slot {
                        if !d.is_valid() {
                            return bad(format!("slot {}", i + 1));
                        }
                        if s.zone_clash(i, &d.meta) {
                            return bad(format!("slot {} overlaps another zone", i + 1));
                        }
                    }
                }
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
        // Each known version is matched explicitly, so bumping
        // PROJECT_VERSION without an upgrade path can't misread old files.
        let p = match head.version {
            0 | 1 => ron::from_str::<crate::migrate::ProjectV1>(text)
                .map_err(parse)?
                .upgrade()
                .upgrade(),
            2 => with_pcm_budget(|| ron::from_str::<crate::migrate::ProjectV2>(text))
                .map_err(parse)?
                .upgrade(),
            PROJECT_VERSION => with_pcm_budget(|| ron::from_str::<Project>(text)).map_err(parse)?,
            v => return Err(ProjectError::TooNew(v)),
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
    use crate::machine::MachineUid;
    use crate::params::{FmParam, KitParam, MasterParam, SynthParam};

    const SAMPLER: MachineUid = MachineUid(6);
    const FM: MachineUid = MachineUid(5);

    fn sampler(p: &Project) -> &SamplerMachine {
        p.sampler(SAMPLER).unwrap()
    }

    fn sampler_mut(p: &mut Project) -> &mut SamplerMachine {
        p.sampler_mut(SAMPLER).unwrap()
    }

    fn fm_mut(p: &mut Project) -> &mut FmMachine {
        p.fm_mut(FM).unwrap()
    }

    fn synth_mut(p: &mut Project, s: usize) -> &mut SynthMachine {
        p.synth_mut(MachineUid(1 + s as u16)).unwrap()
    }

    fn kit_mut(p: &mut Project, k: usize) -> &mut DrumMachine {
        p.kit_mut(MachineUid(3 + k as u16)).unwrap()
    }

    #[test]
    fn a_v2_project_upgrades_to_the_standard_six() {
        let v2 = include_str!("../tests/fixtures/golden_v2.sidacid");
        let p = Project::from_ron(v2).unwrap();
        assert_eq!(p.version, PROJECT_VERSION);
        let names: Vec<_> = p.machines.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Synth 1", "Synth 2", "808", "909", "FM", "Sampler"]);
        let uids: Vec<_> = p.machines.iter().map(|m| m.uid.0).collect();
        assert_eq!(uids, [1, 2, 3, 4, 5, 6]);
        assert_eq!(p.next_uid, 7);
        assert!(matches!(&p.machines[2].body, MachineBody::Kit(k) if k.kind == KitKind::K808));
        assert!(matches!(&p.machines[3].body, MachineBody::Kit(k) if k.kind == KitKind::K909));
        // The fixture's bar 2 mutes the 909 and transposes Synth 2 by -2, FM by 4.
        let bar = &p.song.bars[2];
        assert!(bar.cell(MachineUid(4)).mute);
        assert_eq!(bar.cell(MachineUid(2)).transpose, -2);
        assert_eq!(bar.cell(MachineUid(5)).transpose, 4);
        assert_eq!(bar.cell(MachineUid(3)).transpose, 0);
        p.validate().unwrap();
    }

    #[test]
    fn v3_round_trips() {
        let p = crate::demo::demo_project();
        assert_eq!(Project::from_ron(&p.to_ron()).unwrap(), p);
    }

    #[test]
    fn a_bar_may_not_name_a_machine_twice() {
        let mut p = Project::default();
        p.song.bars.push(Bar {
            cells: vec![
                (MachineUid(1), Cell::default()),
                (MachineUid(1), Cell::default()),
            ],
        });
        assert!(p.validate().unwrap_err().to_string().contains("twice"));
    }

    #[test]
    fn validate_rejects_broken_racks() {
        let ok = Project::default();
        let mut p = ok.clone();
        p.machines[1].uid = p.machines[0].uid;
        assert!(p.validate().is_err(), "duplicate uid");
        let mut p = ok.clone();
        p.next_uid = 6;
        assert!(p.validate().is_err(), "next_uid not above every uid");
        let mut p = ok.clone();
        p.song.bars.push(Bar {
            cells: vec![(MachineUid(42), Cell::default())],
        });
        assert!(p.validate().is_err(), "cell for unknown uid");
        let mut p = ok.clone();
        p.song.automation.push(AutoLane {
            param: ParamId::Machine(MachineUid(1), MachineParam::Fm(FmParam::Volume)),
            points: Default::default(),
        });
        assert!(p.validate().is_err(), "FM param on a synth");
        let mut p = ok.clone();
        p.machines[0]
            .params
            .insert(MachineParam::Kit(KitParam::Volume), 0.5);
        assert!(p.validate().is_err(), "kit param on a synth");
    }

    #[test]
    fn any_rack_layout_of_up_to_sixteen_validates() {
        let mut p = Project::default();
        p.machines.swap(0, 2);
        p.validate().unwrap();
        p.machines.pop();
        p.validate().unwrap();
        p.machines.clear();
        p.validate().unwrap();
        let mut p = Project::default();
        let extra: Vec<_> = (0..MAX_MACHINES as u16 - 5)
            .map(|i| MachineEntry::new(MachineUid(7 + i), MachineType::Fm, format!("FM {i}")))
            .collect();
        p.machines.extend(extra);
        p.next_uid = 100;
        assert!(p.validate().is_err(), "more than MAX_MACHINES");
        p.machines.pop();
        p.validate().unwrap();
    }

    #[test]
    fn project_params_resolve_through_entries_and_globals() {
        let mut p = Project::default();
        let id = ParamId::Machine(MachineUid(3), MachineParam::Kit(KitParam::Accent));
        p.set_param(id, 0.25);
        assert_eq!(p.param(id), 0.25);
        p.set_param(ParamId::Master(MasterParam::Level), 0.1);
        assert_eq!(p.param(ParamId::Master(MasterParam::Level)), 0.1);
        let unknown = ParamId::Machine(MachineUid(99), MachineParam::Synth(SynthParam::Tune));
        p.set_param(unknown, 0.9);
        assert_eq!(
            p.param(unknown),
            MachineParam::Synth(SynthParam::Tune).info().default
        );
        let misfit = ParamId::Machine(MachineUid(1), MachineParam::Kit(KitParam::Accent));
        p.set_param(misfit, 0.9);
        p.validate().unwrap();
    }

    #[test]
    fn machine_accessors_find_entries_by_uid() {
        let mut p = Project::default();
        assert_eq!(p.position(MachineUid(4)), Some(3));
        assert_eq!(p.position(MachineUid(7)), None);
        assert!(p.synth(MachineUid(2)).is_some());
        assert!(p.synth(MachineUid(3)).is_none());
        assert_eq!(p.kit(MachineUid(4)).unwrap().kind, KitKind::K909);
        assert!(p.fm(FM).is_some() && p.sampler(SAMPLER).is_some());
        p.kit_mut(MachineUid(3)).unwrap().patterns[1].length = 9;
        assert_eq!(p.kit(MachineUid(3)).unwrap().patterns[1].length, 9);
        assert_eq!(
            p.machine(MachineUid(5)).unwrap().machine_type(),
            MachineType::Fm
        );
        for (m, (uid, ty, name)) in p.machines.iter().zip(standard_rack()) {
            assert_eq!((m.uid, m.machine_type(), m.name.as_str()), (uid, ty, name));
            assert_eq!(m.params.len(), MachineParam::count_for(ty));
        }
        assert_eq!(p.globals.len(), GlobalParam::COUNT);
    }

    #[test]
    fn bar_cells_default_when_absent() {
        let mut bar = Bar::default();
        assert_eq!(bar.cell(MachineUid(3)), Cell::default());
        bar.cell_mut(MachineUid(3)).mute = true;
        bar.cell_mut(MachineUid(3)).transpose = 2;
        assert_eq!(bar.cells.len(), 1);
        assert_eq!(
            bar.cell(MachineUid(3)),
            Cell {
                pattern: None,
                mute: true,
                transpose: 2
            }
        );
    }

    #[test]
    fn sampler_slots_round_trip_validate_and_place_zones() {
        use crate::sampler::slot::{SlotData, SlotMeta};
        let mut p = Project::default();
        assert_eq!(sampler(&p).slots.len(), crate::sampler::SLOTS);
        let tone: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.02).sin() * 0.5).collect();
        sampler_mut(&mut p).slots[0] = Some(SlotData::new(
            "Amen",
            22_050,
            1,
            &tone,
            SlotMeta::sliced(24, vec![0, 500, 1000, 1500], 120.0, 2000),
        ));
        sampler_mut(&mut p).slots[3] = Some(SlotData::new(
            "Vox",
            22_050,
            1,
            &tone,
            SlotMeta::chromatic(40, 63, 40, 2000),
        ));
        let back = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(back, p);
        let s = sampler(&p);
        assert_eq!(s.slot_for_key(25), Some(0));
        assert_eq!(s.slot_for_key(50), Some(3));
        assert_eq!(s.slot_for_key(30), None);
        assert_eq!(
            s.free_base_key(1, 4),
            Some(28),
            "first gap above C1 that fits"
        );
        assert_eq!(s.free_base_key(1, 20), Some(64));
        assert_eq!(
            s.free_base_key(0, 4),
            Some(24),
            "a slot ignores its own zone"
        );
        assert!(s.zone_clash(1, &SlotMeta::chromatic(26, 30, 26, 2000)));
        assert!(!s.zone_clash(0, &SlotMeta::chromatic(26, 30, 26, 2000)));
        assert_eq!(s.memory_bytes(), 2 * 2000 * 4);
        assert_eq!(s.free_base_key(0, 200), None);
        assert!(s.memory_ok(16_000));
        assert!(!s.memory_ok(15_999));

        let mut clash = p.clone();
        sampler_mut(&mut clash).slots[5] = Some(SlotData::new(
            "X",
            22_050,
            1,
            &tone,
            SlotMeta::chromatic(26, 30, 26, 2000),
        ));
        assert!(
            matches!(clash.validate(), Err(ProjectError::Invalid(_))),
            "overlapping zones"
        );
        let mut broken = p.clone();
        sampler_mut(&mut broken).slots.pop();
        assert!(matches!(broken.validate(), Err(ProjectError::Invalid(_))));
        let mut bad_meta = p.clone();
        sampler_mut(&mut bad_meta).slots[0]
            .as_mut()
            .unwrap()
            .meta
            .slices = vec![0, 5000];
        assert!(matches!(bad_meta.validate(), Err(ProjectError::Invalid(_))));
    }

    /// A default project's RON with the sampler's slot list replaced.
    fn ron_with_slot_entries(entries: &[String]) -> String {
        let text = Project::default().to_ron();
        let start = text.find("slots:").expect("slots field");
        let open = start + text[start..].find('[').expect("slots list");
        let close = open + text[open..].find(']').expect("end of slots list");
        format!("{}{}{}", &text[..=open], entries.join(", "), &text[close..])
    }

    fn slot_entry(base_key: u8, frames: u32) -> (String, String) {
        use crate::sampler::slot::{SlotData, SlotMeta, pcm_codec};
        let mut d = SlotData::new(
            "x",
            44_100,
            1,
            &[],
            SlotMeta::chromatic(base_key, base_key + 7, base_key, frames),
        );
        d.pcm = vec![0; frames as usize];
        (
            ron::ser::to_string(&Some(d)).unwrap(),
            pcm_codec::encode(&vec![0; frames as usize]),
        )
    }

    #[test]
    fn more_than_sixteen_slots_are_refused_without_decoding_the_extras() {
        let nones = vec!["None".to_string(); crate::sampler::SLOTS + 1];
        assert!(matches!(
            Project::from_ron(&ron_with_slot_entries(&nones)),
            Err(ProjectError::Parse(_))
        ));
        // The 17th entry's audio is not valid base64: were it decoded, the
        // error would be about that instead of the slot count.
        let (entry, pcm) = slot_entry(0, 100);
        let mut entries = vec!["None".to_string(); crate::sampler::SLOTS];
        entries.push(entry.replace(&pcm, "!!not base64!!"));
        match Project::from_ron(&ron_with_slot_entries(&entries)) {
            Err(ProjectError::Parse(e)) => assert!(e.contains("at most 16"), "{e}"),
            other => panic!("expected a parse error, got {other:?}"),
        }
        let exact = vec!["None".to_string(); crate::sampler::SLOTS];
        assert!(Project::from_ron(&ron_with_slot_entries(&exact)).is_ok());
    }

    #[test]
    fn embedded_audio_over_the_memory_limit_is_refused_while_parsing() {
        // 16 slots of 2.2 MB of 16-bit audio: 35 MB, over the 32 MB of PCM
        // that MEMORY_LIMIT (64 MB of f32) allows.
        let entries: Vec<String> = (0..crate::sampler::SLOTS as u8)
            .map(|i| slot_entry(i * 8, 1_100_000).0)
            .collect();
        match Project::from_ron(&ron_with_slot_entries(&entries)) {
            Err(ProjectError::Parse(e)) => assert!(e.contains("memory"), "{e}"),
            other => panic!("expected a parse error, got {other:?}"),
        }
        // The budget is per parse: a project within it still loads afterwards.
        let few: Vec<String> = (0..2u8).map(|i| slot_entry(i * 8, 1_100_000).0).collect();
        let mut few = few;
        few.resize(crate::sampler::SLOTS, "None".into());
        assert!(Project::from_ron(&ron_with_slot_entries(&few)).is_ok());
        assert!(Project::from_ron(&ron_with_slot_entries(&few)).is_ok());
    }

    /// A project saved by Plan 4, before the FM voice name and bank and
    /// the sampler slots existed, with its folded runs unfolded. A run is
    /// `<<N|item>>`, N copies of `item` joined by commas. Runs may nest
    /// (unfolded innermost first, from the last `<<`), and an item must not
    /// itself contain `|` or `>>` outside a nested run.
    fn plan4_project_text() -> String {
        let mut s = include_str!("../tests/fixtures/plan4.ron").to_string();
        while let Some(open) = s.rfind("<<") {
            let bar = open + s[open..].find('|').expect("run count");
            let close = bar + s[bar..].find(">>").expect("run end");
            let n: usize = s[open + 2..bar].parse().expect("run count");
            let items = vec![&s[bar + 1..close]; n].join(",");
            s.replace_range(open..close + 2, &items);
        }
        s
    }

    #[test]
    fn a_plan_4_project_still_loads() {
        let p = Project::from_ron(&plan4_project_text()).unwrap();
        assert_eq!((p.name.as_str(), p.bpm), ("Plan 4", 130.0));
        assert_eq!(
            p.synth(MachineUid(1)).unwrap().patterns[0].steps[0].gate,
            Gate::On
        );
        assert_eq!(p.fm(FM).unwrap().patterns[0].notes.len(), 1);
        assert_eq!(sampler(&p).patterns[1].notes[0].pitch, 36);
        p.validate().unwrap();
    }

    #[test]
    fn projects_without_sampler_slots_still_load() {
        let p = Project::from_ron(&plan4_project_text()).unwrap();
        assert_eq!(sampler(&p).slots.len(), crate::sampler::SLOTS);
        assert!(sampler(&p).slots.iter().all(Option::is_none));
    }

    #[test]
    fn fm_voice_name_and_bank_round_trip_and_default() {
        let mut p = Project::default();
        assert_eq!(p.fm(FM).unwrap().voice_name, "INIT VOICE");
        fm_mut(&mut p).voice_name = "E.PIANO 1".into();
        fm_mut(&mut p).bank = Some(crate::fm::starter::starter_bank());
        let back = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(back, p);
        let loaded = Project::from_ron(&plan4_project_text()).unwrap();
        assert_eq!(loaded.fm(FM).unwrap().voice_name, "INIT VOICE");
        assert_eq!(loaded.fm(FM).unwrap().bank, None);
    }

    #[test]
    fn a_damaged_fm_bank_is_rejected() {
        let mut p = Project::default();
        let mut bad = crate::fm::sysex::Dx7Voice::init();
        bad.algorithm = 40;
        fm_mut(&mut p).bank = Some(vec![bad]);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        fm_mut(&mut p).bank = Some(vec![crate::fm::sysex::Dx7Voice::init(); 33]);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn default_project_shape() {
        let mut p = Project::default();
        assert_eq!(p.version, PROJECT_VERSION);
        assert_eq!(synth_mut(&mut p, 0).patterns.len(), PATTERNS);
        assert_eq!(kit_mut(&mut p, 1).patterns.len(), PATTERNS);
        assert_eq!(synth_mut(&mut p, 0).patterns[0].steps.len(), STEPS);
        assert_eq!(kit_mut(&mut p, 0).patterns[0].steps.len(), STEPS);
        assert_eq!(synth_mut(&mut p, 0).patterns[0].length, 16);
        let params: usize = p.machines.iter().map(|m| m.params.len()).sum();
        assert_eq!(params + p.globals.len(), 589);
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
        let id = MachineUid(2).param(SynthParam::Cutoff);
        p.machines[1]
            .params
            .remove(&MachineParam::Synth(SynthParam::Cutoff));
        assert_eq!(p.param(id), id.info().default);
        p.set_param(id, 0.9);
        assert_eq!(p.param(id), 0.9);
    }

    #[test]
    fn ron_round_trip() {
        let mut p = Project::default();
        p.name = "Test".into();
        p.bpm = 133.0;
        synth_mut(&mut p, 1).patterns[5].steps[3] = SynthStep {
            note: 7,
            octave: -1,
            gate: Gate::On,
            accent: true,
            slide: true,
        };
        kit_mut(&mut p, 0).patterns[2].steps[4][0] = DrumHit::Accent;
        p.set_param(MachineUid(1).param(SynthParam::Resonance), 0.95);
        let mut bar = Bar::default();
        bar.cell_mut(MachineUid(1)).pattern = Some(PatternRef(5));
        bar.cell_mut(MachineUid(3)).pattern = Some(PatternRef(2));
        bar.cell_mut(MachineUid(5)).pattern = Some(PatternRef(9));
        bar.cell_mut(MachineUid(5)).transpose = -5;
        bar.cell_mut(MachineUid(2)).mute = true;
        bar.cell_mut(MachineUid(2)).transpose = 3;
        bar.cell_mut(MachineUid(6)).mute = true;
        p.song.bars.push(bar);
        fm_mut(&mut p).patterns[9].notes.push(Note {
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
        synth_mut(&mut p, 0).patterns[0].length = 0;
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        kit_mut(&mut p, 1).patterns.pop();
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.machines[2].selected = 200;
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
        p.machines[0]
            .params
            .insert(MachineParam::Synth(SynthParam::Cutoff), f32::NAN);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.machines[5]
            .params
            .insert(MachineParam::Synth(SynthParam::Cutoff), 0.5);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        p.globals
            .insert(GlobalParam::Master(MasterParam::Level), f32::NAN);
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        let mut lane = ParamLane {
            param: MachineUid(3).param(MachineParam::Drum(9, crate::params::DrumParam::Tune)),
            values: vec![None; STEPS],
        };
        synth_mut(&mut p, 0).patterns[0].param_lane = Some(lane.clone());
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        lane.param = MachineUid(1).param(SynthParam::Cutoff);
        lane.values[3] = Some(f32::NAN);
        synth_mut(&mut p, 0).patterns[0].param_lane = Some(lane);
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

        let id = MachineUid(1).param(SynthParam::Cutoff);
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
            param: MachineUid(8).param(SynthParam::Cutoff),
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
    fn note_machines_are_reachable_by_uid() {
        let mut p = Project::default();
        assert_eq!(fm_mut(&mut p).patterns.len(), PATTERNS);
        assert_eq!(sampler(&p).patterns.len(), PATTERNS);
        assert!(p.note_patterns(MachineUid(1)).is_none());
        assert!(p.note_patterns(MachineUid(9)).is_none());
        p.note_patterns_mut(SAMPLER).unwrap()[3].length = 8;
        assert_eq!(sampler(&p).patterns[3].length, 8);
        assert_eq!(p.note_patterns(FM).unwrap().len(), PATTERNS);
        fm_mut(&mut p).patterns[1].notes.push(Note {
            start: 0,
            len: 1,
            pitch: 60,
            vel: 0,
        });
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn v1_files_load_with_empty_new_machines() {
        use crate::migrate::{BarV1, LegacyParamId, ProjectV1, SongV1, SynthMachineV2};
        let mut params = BTreeMap::new();
        params.insert(LegacyParamId::Synth(0, SynthParam::Cutoff), 0.9);
        let mut synths = [SynthMachineV2::default(), SynthMachineV2::default()];
        synths[1].patterns[2].length = 7;
        let v1 = ProjectV1 {
            version: 1,
            name: "Old".into(),
            bpm: 131.0,
            swing: 0.2,
            synths,
            drums: Default::default(),
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
        assert_eq!(p.synth(MachineUid(2)).unwrap().patterns[2].length, 7);
        let selected: Vec<_> = p.machines.iter().map(|m| m.selected).collect();
        assert_eq!(selected, [1, 2, 3, 4, 0, 0]);
        assert_eq!(p.param(MachineUid(1).param(SynthParam::Cutoff)), 0.9);
        let ch = MachineParam::Channel(crate::params::ChannelParam::Level);
        assert_eq!(p.machines[5].params.get(&ch), Some(&ch.info().default));
        let bar = &p.song.bars[0];
        let patterns: Vec<_> = (1..=6).map(|u| bar.cell(MachineUid(u)).pattern).collect();
        assert_eq!(
            patterns,
            [
                Some(PatternRef(1)),
                None,
                Some(PatternRef(3)),
                None,
                None,
                None
            ]
        );
        let mutes: Vec<_> = (1..=6).map(|u| bar.cell(MachineUid(u)).mute).collect();
        assert_eq!(mutes, [true, false, false, true, false, false]);
        let transposes = [1, 2, 5].map(|u| bar.cell(MachineUid(u)).transpose);
        assert_eq!(transposes, [2, -3, 0]);
        assert_eq!(p.song.loop_range, Some((0, 0)));
        assert_eq!(p.fm(FM).unwrap().patterns.len(), PATTERNS);
        assert!(sampler(&p).patterns.iter().all(|n| n.notes.is_empty()));
        assert_eq!(p.kit(MachineUid(4)).unwrap().kind, KitKind::K909);
    }
}
