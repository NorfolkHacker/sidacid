//! Project data: the rack of machines (each with its patterns, sound and
//! knob values), the global knobs and the song. Saved as RON
//! (`.sidacid`). Fixed-size arrays are Vecs of exact length, checked by
//! `validate` on load.

use crate::drums::KitKind;
use crate::machine::{MACHINE_COLOURS, MachineType, MachineUid, standard_rack};
use crate::params::{DRUM_INSTRUMENTS, GlobalParam, MachineParam, ParamId};
use crate::scale::Key;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub use crate::rack_edit::RemovedMachine;

pub const PROJECT_VERSION: u32 = 7;
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
/// Most notes one note pattern may hold: a full 64-step pattern on all
/// eight drums.
pub const MAX_NOTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamLane {
    pub param: ParamId,
    pub values: Vec<Option<f32>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SynthPattern {
    /// Monophonic notes; the roll editor keeps them from overlapping.
    pub roll: NotePattern,
    pub transpose: i8,
    /// One optional value per 1/16 step.
    pub param_lane: Option<ParamLane>,
}

impl SynthPattern {
    /// Shape the engine relies on: a well-formed roll and a full-size lane.
    pub fn is_well_formed(&self) -> bool {
        self.roll.is_well_formed()
            && self
                .param_lane
                .as_ref()
                .is_none_or(|l| l.values.len() == STEPS)
    }
}

/// A drum pattern: one-step notes whose pitch is the instrument (0..8).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DrumPattern {
    pub roll: NotePattern,
}

impl DrumPattern {
    pub fn is_well_formed(&self) -> bool {
        self.roll.is_well_formed()
            && self
                .roll
                .notes
                .iter()
                .all(|n| (n.pitch as usize) < DRUM_INSTRUMENTS)
    }
}

/// When a note plays, beyond its chance (spec phase 6; stored from v5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cond {
    #[default]
    Always,
    /// Plays on pass `a` of every `b` loops (1 <= a <= b <= 8).
    Ratio(u8, u8),
    First,
    NotFirst,
    Fill,
    NotFill,
    /// The lane's previous conditional note played.
    Pre,
    NotPre,
}

impl Cond {
    pub fn is_valid(self) -> bool {
        match self {
            Cond::Ratio(a, b) => (1..=8).contains(&b) && (1..=b).contains(&a),
            _ => true,
        }
    }

    fn is_always(&self) -> bool {
        *self == Cond::Always
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn full_chance() -> u8 {
    100
}

fn is_full_chance(c: &u8) -> bool {
    *c == 100
}

fn one(n: &u8) -> bool {
    *n == 1
}

fn single() -> u8 {
    1
}

/// One note of a note pattern, in ticks (`TICKS_PER_STEP` per step).
/// Fields past `vel` are skipped when at their defaults, so a plain
/// FM or sampler note saves as before.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub start: u16,
    pub len: u16,
    pub pitch: u8,
    pub vel: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub accent: bool,
    /// Acid: glide into a note starting where this one ends.
    #[serde(default, skip_serializing_if = "is_false")]
    pub slide: bool,
    /// Play chance in percent, 0..=100.
    #[serde(default = "full_chance", skip_serializing_if = "is_full_chance")]
    pub chance: u8,
    /// Even re-triggers within the note, 1..=8.
    #[serde(default = "single", skip_serializing_if = "one")]
    pub ratchet: u8,
    #[serde(default, skip_serializing_if = "Cond::is_always")]
    pub cond: Cond,
}

impl Note {
    pub const fn new(start: u16, len: u16, pitch: u8, vel: u8) -> Self {
        Self {
            start,
            len,
            pitch,
            vel,
            accent: false,
            slide: false,
            chance: 100,
            ratchet: 1,
            cond: Cond::Always,
        }
    }
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
                n.len >= 1
                    && n.start < max_start
                    && n.pitch <= 127
                    && (1..=127).contains(&n.vel)
                    && n.chance <= 100
                    && (1..=8).contains(&n.ratchet)
                    && n.cond.is_valid()
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
pub struct MonoMachine {
    pub patterns: Vec<NotePattern>,
    /// Name of the patch the Mono params were last loaded from.
    #[serde(default = "init_patch_name")]
    pub patch_name: String,
}

fn init_patch_name() -> String {
    "Init".into()
}

impl Default for MonoMachine {
    fn default() -> Self {
        Self {
            patterns: vec![NotePattern::default(); PATTERNS],
            patch_name: init_patch_name(),
        }
    }
}

/// A Pad Kit: note patterns whose pitch is the pad, and the pads' samples.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PadKitMachine {
    pub patterns: Vec<NotePattern>,
    /// Exactly `PADS`.
    #[serde(default = "empty_pads", deserialize_with = "exactly_pads")]
    pub pads: Vec<crate::padkit::pad::Pad>,
}

/// Reads the pad list, refusing any length but `PADS` before an extra
/// pad's audio is decoded.
pub(crate) fn exactly_pads<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<crate::padkit::pad::Pad>, D::Error> {
    use crate::padkit::PADS;
    use serde::de::{Error, IgnoredAny, SeqAccess, Visitor};
    struct Pads;
    impl<'de> Visitor<'de> for Pads {
        type Value = Vec<crate::padkit::pad::Pad>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "exactly {PADS} pads")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut pads = Vec::with_capacity(PADS);
            while pads.len() < PADS {
                match seq.next_element()? {
                    Some(pad) => pads.push(pad),
                    None => return Err(A::Error::invalid_length(pads.len(), &self)),
                }
            }
            if seq.next_element::<IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(PADS + 1, &self));
            }
            Ok(pads)
        }
    }
    d.deserialize_seq(Pads)
}

fn empty_pads() -> Vec<crate::padkit::pad::Pad> {
    vec![crate::padkit::pad::Pad::default(); crate::padkit::PADS]
}

impl Default for PadKitMachine {
    fn default() -> Self {
        Self {
            patterns: vec![NotePattern::default(); PATTERNS],
            pads: empty_pads(),
        }
    }
}

impl PadKitMachine {
    /// Size of the decoded f32 audio of every layer.
    pub fn memory_bytes(&self) -> usize {
        self.pads.iter().map(|p| p.memory_bytes()).sum()
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SynthMachine {
    pub patterns: Vec<SynthPattern>,
}

impl Default for SynthMachine {
    fn default() -> Self {
        Self {
            patterns: vec![SynthPattern::default(); PATTERNS],
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoLane {
    pub param: ParamId,
    pub points: BTreeMap<u32, f32>,
}

fn is_zero(n: &i8) -> bool {
    *n == 0
}

/// One pattern placed on a machine's song lane, in steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    /// First step, from the song start.
    pub start: u32,
    pub pattern: PatternRef,
    /// `None`: as long as the pattern (and follows it). `Some(n)`: `n`
    /// steps; longer than the pattern loops it, shorter plays its start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len: Option<u16>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mute: bool,
    /// Song transpose; only machine types that transpose use it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transpose: i8,
}

impl Block {
    /// An unmuted, untransposed block as long as its pattern.
    pub fn new(start: u32, pattern: PatternRef) -> Self {
        Self {
            start,
            pattern,
            len: None,
            mute: false,
            transpose: 0,
        }
    }
}

/// The song: one lane of blocks per machine (sorted by start, never
/// overlapping), global-step automation and a loop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub lanes: Vec<(MachineUid, Vec<Block>)>,
    /// The song lasts at least this many steps (it ends at the later of
    /// this and its last block's end). Converted songs keep their length.
    #[serde(default)]
    pub end: Option<u32>,
    /// Steps `(start, end)`, end exclusive.
    pub loop_range: Option<(u32, u32)>,
    pub automation: Vec<AutoLane>,
}

impl Song {
    /// `uid`'s blocks; empty if it has no lane.
    pub fn lane(&self, uid: MachineUid) -> &[Block] {
        self.lanes
            .iter()
            .find(|(u, _)| *u == uid)
            .map_or(&[], |(_, b)| b.as_slice())
    }

    /// `uid`'s blocks, its lane added (empty) if it has none.
    pub fn lane_mut(&mut self, uid: MachineUid) -> &mut Vec<Block> {
        let i = match self.lanes.iter().position(|(u, _)| *u == uid) {
            Some(i) => i,
            None => {
                self.lanes.push((uid, Vec::new()));
                self.lanes.len() - 1
            }
        };
        &mut self.lanes[i].1
    }
}

/// A machine's sound data and patterns, by type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MachineBody {
    Synth(SynthMachine),
    Kit(DrumMachine),
    Fm(FmMachine),
    Sampler(SamplerMachine),
    Mono(MonoMachine),
    PadKit(PadKitMachine),
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
            MachineType::Mono => MachineBody::Mono(MonoMachine::default()),
            MachineType::PadKit => MachineBody::PadKit(PadKitMachine::default()),
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
            MachineBody::Mono(_) => MachineType::Mono,
            MachineBody::PadKit(_) => MachineType::PadKit,
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
    /// How this machine's lane looks in the editor dock.
    #[serde(default, skip_serializing_if = "LaneView::is_default")]
    pub lane: LaneView,
    /// Its colour, an index into the palette (below
    /// `machine::MACHINE_COLOURS`). Absent in older files: loading gives
    /// such a machine its rack position's colour.
    #[serde(default = "MachineEntry::no_colour")]
    pub colour: u8,
}

/// A machine's lane in the editor dock: its height and what it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneView {
    /// Lane height in points; read it through `clamped_height`.
    pub height: u16,
    /// The lane is folded down to its header.
    pub collapsed: bool,
    /// The lane shows its velocity/parameter strip.
    pub strip: bool,
    /// A melodic lane shows only the rows on the project key's scale (and
    /// rows holding notes). Absent in older files.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fold: bool,
}

impl LaneView {
    pub const DEFAULT_HEIGHT: u16 = 120;
    pub const MIN_HEIGHT: u16 = 48;
    pub const MAX_HEIGHT: u16 = 480;
    /// A Pad Kit's lane: its 16 rows at 12 points each.
    pub const PADS_HEIGHT: u16 = 16 * 12;

    /// A fresh lane for a `ty` machine: a Pad Kit's is tall enough to read
    /// every row.
    pub fn for_type(ty: MachineType) -> Self {
        match ty {
            MachineType::PadKit => Self {
                height: Self::PADS_HEIGHT,
                ..Self::default()
            },
            _ => Self::default(),
        }
    }

    /// True when nothing differs from a fresh lane (so it is not saved).
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// `height` held to `MIN_HEIGHT..=MAX_HEIGHT`.
    pub fn clamped_height(&self) -> u16 {
        self.height.clamp(Self::MIN_HEIGHT, Self::MAX_HEIGHT)
    }
}

impl Default for LaneView {
    fn default() -> Self {
        Self {
            height: Self::DEFAULT_HEIGHT,
            collapsed: false,
            strip: true,
            fold: false,
        }
    }
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
            lane: LaneView::for_type(ty),
            colour: 0,
        }
    }

    /// A machine read from a file without a colour, until loading gives
    /// it one: 255 means "not in the file" and is resolved on load
    /// (`Project::colour_uncoloured`), so a file that says 255 loads with
    /// its rack position's colour rather than being refused.
    pub(crate) const NO_COLOUR: u8 = u8::MAX;

    fn no_colour() -> u8 {
        Self::NO_COLOUR
    }

    pub fn machine_type(&self) -> MachineType {
        self.body.machine_type()
    }

    /// Decoded bytes (f32, as `memory_bytes` counts) of this machine's
    /// samples: a Sampler's slots or a Pad Kit's layers; 0 for the rest.
    pub fn sample_bytes(&self) -> usize {
        match &self.body {
            MachineBody::Sampler(s) => s.memory_bytes(),
            MachineBody::PadKit(k) => k.memory_bytes(),
            _ => 0,
        }
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
    /// The project's key: root, scale and snap.
    #[serde(default, skip_serializing_if = "is_default_key")]
    pub key: Key,
    /// Seed of the chance rolls (see `trig`); "Reroll" changes it.
    #[serde(default, skip_serializing_if = "is_zero_seed")]
    pub seed: u32,
}

fn is_zero_seed(s: &u32) -> bool {
    *s == 0
}

fn is_default_key(k: &Key) -> bool {
    *k == Key::default()
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
                .enumerate()
                .map(|(i, (uid, ty, name))| MachineEntry {
                    colour: (i % MACHINE_COLOURS) as u8,
                    ..MachineEntry::new(uid, ty, name.into())
                })
                .collect(),
            next_uid: 7,
            globals: GlobalParam::all()
                .into_iter()
                .map(|g| (g, g.info().default))
                .collect(),
            song: Song::default(),
            key: Key::default(),
            seed: 0,
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

    /// Decoded bytes (f32, as `memory_bytes` counts) of every sample in the project.
    pub fn sample_bytes(&self) -> usize {
        self.machines.iter().map(MachineEntry::sample_bytes).sum()
    }
    body_accessors!(mono, mono_mut, Mono, MonoMachine);
    body_accessors!(padkit, padkit_mut, PadKit, PadKitMachine);

    /// Note patterns of an FM, Mono or Sampler machine.
    pub fn note_patterns(&self, uid: MachineUid) -> Option<&Vec<NotePattern>> {
        match &self.machine(uid)?.body {
            MachineBody::Fm(m) => Some(&m.patterns),
            MachineBody::Mono(m) => Some(&m.patterns),
            MachineBody::PadKit(m) => Some(&m.patterns),
            MachineBody::Sampler(m) => Some(&m.patterns),
            _ => None,
        }
    }

    pub fn note_patterns_mut(&mut self, uid: MachineUid) -> Option<&mut Vec<NotePattern>> {
        match &mut self.machine_mut(uid)?.body {
            MachineBody::Fm(m) => Some(&mut m.patterns),
            MachineBody::Mono(m) => Some(&mut m.patterns),
            MachineBody::PadKit(m) => Some(&mut m.patterns),
            MachineBody::Sampler(m) => Some(&mut m.patterns),
            _ => None,
        }
    }

    /// Pattern `slot`'s notes, for any machine.
    pub fn roll(&self, uid: MachineUid, slot: u8) -> Option<&NotePattern> {
        let s = slot as usize;
        match &self.machine(uid)?.body {
            MachineBody::Synth(m) => Some(&m.patterns.get(s)?.roll),
            MachineBody::Kit(m) => Some(&m.patterns.get(s)?.roll),
            MachineBody::Fm(m) => m.patterns.get(s),
            MachineBody::Mono(m) => m.patterns.get(s),
            MachineBody::PadKit(m) => m.patterns.get(s),
            MachineBody::Sampler(m) => m.patterns.get(s),
        }
    }

    pub fn roll_mut(&mut self, uid: MachineUid, slot: u8) -> Option<&mut NotePattern> {
        let s = slot as usize;
        match &mut self.machine_mut(uid)?.body {
            MachineBody::Synth(m) => Some(&mut m.patterns.get_mut(s)?.roll),
            MachineBody::Kit(m) => Some(&mut m.patterns.get_mut(s)?.roll),
            MachineBody::Fm(m) => m.patterns.get_mut(s),
            MachineBody::Mono(m) => m.patterns.get_mut(s),
            MachineBody::PadKit(m) => m.patterns.get_mut(s),
            MachineBody::Sampler(m) => m.patterns.get_mut(s),
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
    pub fn param_in_rack(&self, id: ParamId) -> bool {
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
        if self.key.root >= 12 {
            return bad(format!("key root {} is out of range", self.key.root));
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
        for m in &self.machines {
            self.validate_machine(m)?;
        }
        self.validate_lanes()?;
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

    /// Checks the song's lanes, length and loop: lanes on rack machines,
    /// one each; blocks sorted, never overlapping, on patterns in range
    /// and within `MAX_SONG_STEPS`.
    pub fn validate_lanes(&self) -> Result<(), ProjectError> {
        let bad = |m: &str| Err(ProjectError::Invalid(m.into()));
        let max = crate::song::MAX_SONG_STEPS as u32;
        let lens = crate::song::lens_of(self);
        for (i, (uid, blocks)) in self.song.lanes.iter().enumerate() {
            if self.machine(*uid).is_none() {
                return bad("song lane names a machine not in the rack");
            }
            if self.song.lanes[..i].iter().any(|(u, _)| u == uid) {
                return bad("song names a machine's lane twice");
            }
            let mut prev_end = 0;
            for b in blocks {
                if b.pattern.0 as usize >= PATTERNS {
                    return bad("song references a pattern out of range");
                }
                if b.len == Some(0) {
                    return bad("song block has no length");
                }
                let end = crate::song::block_end(b, *uid, &lens);
                if b.start < prev_end {
                    return bad("song blocks overlap or are out of order");
                }
                if end > max {
                    return bad("song block is past the song length limit");
                }
                prev_end = end;
            }
        }
        if self.song.end.is_some_and(|e| e > max) {
            return bad("song end is past the song length limit");
        }
        if let Some((a, b)) = self.song.loop_range
            && (a >= b || b > max)
        {
            return bad("song loop range is out of bounds");
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
        if m.colour as usize >= MACHINE_COLOURS {
            return bad(format!("colour {} is out of range", m.colour));
        }
        let count = match &m.body {
            MachineBody::Synth(s) => s.patterns.len(),
            MachineBody::Kit(k) => k.patterns.len(),
            MachineBody::Fm(f) => f.patterns.len(),
            MachineBody::Sampler(s) => s.patterns.len(),
            MachineBody::Mono(m) => m.patterns.len(),
            MachineBody::PadKit(k) => k.patterns.len(),
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
            MachineBody::Mono(m) => {
                if let Some(j) = m.patterns.iter().position(|p| !p.is_well_formed()) {
                    return bad(format!("pattern {}", label(j)));
                }
            }
            MachineBody::PadKit(k) => {
                for (j, p) in k.patterns.iter().enumerate() {
                    if !p.is_well_formed()
                        || p.notes
                            .iter()
                            .any(|n| n.pitch as usize >= crate::padkit::PADS)
                    {
                        return bad(format!("pattern {}", label(j)));
                    }
                }
                if k.pads.len() != crate::padkit::PADS {
                    return bad(format!("{} pads", k.pads.len()));
                }
                if let Some(i) = k.pads.iter().position(|p| !p.is_well_formed()) {
                    return bad(format!("pad {}", i + 1));
                }
            }
        }
        Ok(())
    }

    /// A machine read without a colour (saved before machines kept one)
    /// takes the one its rack position gave it then. `NO_COLOUR` (255)
    /// marks it: a literal 255 in a file is treated the same way.
    pub(crate) fn colour_uncoloured(&mut self) {
        for (i, m) in self.machines.iter_mut().enumerate() {
            if m.colour == MachineEntry::NO_COLOUR {
                m.colour = (i % MACHINE_COLOURS) as u8;
            }
        }
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .expect("project serialises")
    }

    pub fn from_ron(text: &str) -> Result<Project, ProjectError> {
        Project::from_ron_within(text, None)
    }

    /// Like `from_ron`, but refuses a file whose embedded audio inflates
    /// past `budget` bytes of 16-bit PCM in all (`None`: no bound), before
    /// it is all allocated.
    pub fn from_ron_within(text: &str, budget: Option<u64>) -> Result<Project, ProjectError> {
        crate::sampler::slot::pcm_codec::with_budget(budget, || Project::parse_ron(text))
    }

    fn parse_ron(text: &str) -> Result<Project, ProjectError> {
        let parse = |e: ron::error::SpannedError| ProjectError::Parse(e.to_string());
        let head: VersionOnly = ron::from_str(text).map_err(parse)?;
        // Each known version is matched explicitly, so bumping
        // PROJECT_VERSION without an upgrade path can't misread old files.
        let v5 = |p: crate::migrate::ProjectV5| -> Result<Project, ProjectError> {
            p.check_song()?;
            Ok(p.upgrade())
        };
        let v4 = |p: crate::migrate::ProjectV4| -> Result<Project, ProjectError> {
            p.check_shapes()?;
            v5(p.upgrade())
        };
        let mut p = match head.version {
            0 | 1 => v4(ron::from_str::<crate::migrate::ProjectV1>(text)
                .map_err(parse)?
                .upgrade()
                .upgrade())?,
            2 => v4(ron::from_str::<crate::migrate::ProjectV2>(text)
                .map_err(parse)?
                .upgrade())?,
            // v4 only added the Mono machine: a v3 file reads as v4.
            3 | 4 => v4(ron::from_str::<crate::migrate::ProjectV4>(text).map_err(parse)?)?,
            5 => v5(ron::from_str::<crate::migrate::ProjectV5>(text).map_err(parse)?)?,
            // v7 only added the Pad Kit: a v6 file reads as v7.
            6 | PROJECT_VERSION => ron::from_str::<Project>(text).map_err(parse)?,
            v => return Err(ProjectError::TooNew(v)),
        };
        p.version = PROJECT_VERSION;
        p.colour_uncoloured();
        // Kits older builds named "808"/"909" (plus a number) take today's
        // default names; a name the user typed is kept.
        for m in &mut p.machines {
            if let Some(name) = crate::machine::renamed_legacy_default(m.machine_type(), &m.name) {
                m.name = name;
            }
        }
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
        Project::load_within(path, None)
    }

    /// `load` with the audio bound of `from_ron_within`.
    pub fn load_within(path: &Path, budget: Option<u64>) -> Result<Project, ProjectError> {
        Project::from_ron_within(&std::fs::read_to_string(path)?, budget)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one field of a default project
mod tests {
    use super::*;
    use crate::machine::MachineUid;
    use crate::params::{FmParam, KitParam, MasterParam, SynthParam};

    #[test]
    fn key_saves_only_when_changed_and_round_trips() {
        use crate::scale::Scale;
        let mut p = Project::default();
        assert!(!p.to_ron().contains("key:"), "default key is skipped");
        p.key = Key {
            root: 9,
            scale: Scale::Minor,
            snap: true,
        };
        let text = p.to_ron();
        assert!(text.contains("key:"));
        assert_eq!(Project::from_ron(&text).unwrap().key, p.key);
        // a file without the field loads as Chromatic
        let old = Project::default().to_ron();
        assert_eq!(Project::from_ron(&old).unwrap().key, Key::default());
    }

    fn colours(p: &Project) -> Vec<u8> {
        p.machines.iter().map(|m| m.colour).collect()
    }

    #[test]
    fn the_standard_rack_and_the_demo_take_the_colours_in_rack_order() {
        assert_eq!(colours(&Project::default()), [0, 1, 2, 3, 4, 0]);
        assert_eq!(colours(&crate::demo::demo_project()), [0, 1, 2, 3, 4, 0]);
    }

    #[test]
    fn machine_colours_round_trip_and_one_past_the_palette_is_refused() {
        let mut p = Project::default();
        for (m, c) in p.machines.iter_mut().zip([4, 4, 0, 2, 1, 3]) {
            m.colour = c;
        }
        let ron = p.to_ron();
        assert!(ron.contains("colour: 4"));
        assert_eq!(Project::from_ron(&ron).unwrap(), p);
        p.machines[2].colour = MACHINE_COLOURS as u8 - 1;
        assert!(p.validate().is_ok());
        p.machines[2].colour = MACHINE_COLOURS as u8;
        match p.validate() {
            Err(ProjectError::Invalid(e)) => assert_eq!(e, "Kit A: colour 5 is out of range"),
            other => panic!("{other:?}"),
        }
        let ron = p.to_ron();
        assert!(Project::from_ron(&ron).is_err(), "nor is it loaded");
    }

    #[test]
    fn a_file_without_colours_loads_with_rack_order_colours() {
        // Today's version, saved before machines kept a colour: seven
        // machines, so the sixth and seventh start the palette again.
        let mut p = Project::default();
        let m = p.new_machine(MachineType::Mono).unwrap();
        p.insert_machine(2, m).unwrap();
        for m in &mut p.machines {
            m.colour = 4;
        }
        let text: String = p
            .to_ron()
            .lines()
            .filter(|l| !l.trim_start().starts_with("colour:"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(!text.contains("colour"));
        let back = Project::from_ron(&text).unwrap();
        assert_eq!(colours(&back), [0, 1, 2, 3, 4, 0, 1]);
        // An older version's file.
        let v4 = crate::migrate::ProjectV4::standard();
        let back = Project::from_ron(&ron::ser::to_string(&v4).unwrap()).unwrap();
        assert_eq!(colours(&back), [0, 1, 2, 3, 4, 0]);
    }

    #[test]
    fn key_root_must_be_a_pitch_class() {
        let mut p = Project::default();
        p.key.root = 11;
        assert!(p.validate().is_ok());
        p.key.root = 12;
        assert!(p.validate().is_err());
    }

    #[test]
    fn a_folded_lane_saves_its_fold_and_old_lanes_load_unfolded() {
        let mut p = Project::default();
        p.machines[0].lane.fold = true;
        assert!(!p.machines[0].lane.is_default());
        let ron = p.to_ron();
        assert!(ron.contains("fold: true"));
        let back = Project::from_ron(&ron).unwrap();
        assert!(back.machines[0].lane.fold);
        // A lane saved before fold existed loads unfolded.
        p.machines[0].lane = LaneView {
            height: 200,
            ..LaneView::default()
        };
        let ron = p.to_ron();
        assert!(!ron.contains("fold"), "unfolded is not written");
        let back = Project::from_ron(&ron).unwrap();
        assert!(!back.machines[0].lane.fold);
        assert_eq!(back.machines[0].lane.height, 200);
    }

    #[test]
    fn a_new_pad_kit_lane_is_tall_enough_for_its_rows_and_round_trips() {
        let mut p = Project::default();
        let m = p.new_machine(MachineType::PadKit).unwrap();
        assert_eq!(m.lane.height, 192);
        assert_eq!(m.lane.clamped_height(), 192);
        let uid = m.uid;
        p.insert_machine(p.machines.len(), m).unwrap();
        let back = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.machine(uid).unwrap().lane.height, 192);
        // Other machines keep the usual height.
        assert_eq!(
            p.new_machine(MachineType::Fm).unwrap().lane,
            LaneView::default()
        );
    }

    #[test]
    fn lane_views_save_only_when_changed() {
        let mut p = Project::default();
        assert!(!p.to_ron().contains(" lane:"), "defaults are skipped");
        p.machines[0].lane = LaneView {
            height: 200,
            collapsed: true,
            strip: false,
            fold: true,
        };
        let back = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(back.machines[0].lane, p.machines[0].lane);
        assert_eq!(back.machines[1].lane, LaneView::default());
        assert_eq!(
            LaneView {
                height: 5,
                ..LaneView::default()
            }
            .clamped_height(),
            48
        );
        assert_eq!(
            LaneView {
                height: 9000,
                ..LaneView::default()
            }
            .clamped_height(),
            480
        );
    }

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
        assert_eq!(
            names,
            ["Synth 1", "Synth 2", "Kit A", "Kit B", "FM", "Sampler"]
        );
        let uids: Vec<_> = p.machines.iter().map(|m| m.uid.0).collect();
        assert_eq!(uids, [1, 2, 3, 4, 5, 6]);
        assert_eq!(p.next_uid, 7);
        assert!(matches!(&p.machines[2].body, MachineBody::Kit(k) if k.kind == KitKind::K808));
        assert!(matches!(&p.machines[3].body, MachineBody::Kit(k) if k.kind == KitKind::K909));
        // The fixture's bar 2 mutes the 909 and transposes Synth 2 by -2,
        // FM by 4: its blocks carry that. Every machine plays every bar.
        assert_eq!(p.song.lanes.len(), 6);
        assert!(p.song.lanes.iter().all(|(_, blocks)| blocks.len() == 4));
        let block = |u: u16| p.song.lane(MachineUid(u))[2];
        assert!(block(4).mute);
        assert!(!block(3).mute);
        assert_eq!(block(2).transpose, -2);
        assert_eq!(block(5).transpose, 4);
        assert_eq!(block(3).transpose, 0);
        p.validate().unwrap();
    }

    /// A real v4 save (written by the v4 code): the demo project plus a
    /// Mono machine with one note, and Synth 2 in the old piano roll view.
    #[test]
    fn a_v4_save_loads_as_the_demo_on_notes() {
        let text = include_str!("../tests/fixtures/v4_demo.sidacid");
        assert!(text.contains("version: 4,") && text.contains("view: PianoRoll"));
        assert!(text.contains("name: \"808\"") && text.contains("name: \"909\""));
        let p = Project::from_ron(text).unwrap();
        assert_eq!(p.version, PROJECT_VERSION);
        assert_eq!(PROJECT_VERSION, 7);
        assert_eq!(
            (p.machines[2].name.as_str(), p.machines[3].name.as_str()),
            ("Kit A", "Kit B")
        );
        let demo = crate::demo::demo_project();
        assert_eq!(p.machines[..6], demo.machines[..6]);
        assert_eq!(p.song, demo.song);
        let mono = &p.machines[6];
        assert_eq!(
            (mono.name.as_str(), mono.machine_type()),
            ("Mono", MachineType::Mono)
        );
        let MachineBody::Mono(m) = &mono.body else {
            unreachable!()
        };
        assert_eq!(m.patterns[0].notes, [Note::new(0, 8, 48, 100)]);
        p.validate().unwrap();
    }

    /// A save from before the generic kit names: default names of the old
    /// stems take today's; names the user typed stay.
    #[test]
    fn old_default_kit_names_are_renamed_on_load() {
        let mut old = Project::default();
        old.machines[2].name = "808".into();
        old.machines[3].name = "909".into();
        for (uid, ty, name) in [
            (7, MachineType::Kit808, "808 2"),
            (8, MachineType::Kit909, "909 3"),
            (9, MachineType::Kit808, "808 Boom"),
            (10, MachineType::Kit909, "Big 909"),
            (11, MachineType::Synth, "808"),
        ] {
            old.insert_machine(99, MachineEntry::new(MachineUid(uid), ty, name.into()))
                .unwrap();
        }
        let text = old.to_ron();
        assert!(text.contains("K808") && text.contains("name: \"808 2\""));
        let p = Project::from_ron(&text).unwrap();
        let names: Vec<_> = p.machines.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Synth 1", "Synth 2", "Kit A", "Kit B", "FM", "Sampler", "Kit A 2", "Kit B 3",
                "808 Boom", "Big 909", "808",
            ]
        );
        // Everything but the names survives the trip.
        let mut renamed = old.clone();
        for (m, n) in renamed.machines.iter_mut().zip(&names) {
            m.name = (*n).to_string();
        }
        assert_eq!(p, renamed);
        // Today's names save and load unchanged.
        assert_eq!(Project::from_ron(&p.to_ron()).unwrap(), p);
    }

    #[test]
    fn v6_round_trips() {
        let mut with_song = crate::demo::demo_project();
        with_song.song.lane_mut(MachineUid(1)).push(Block {
            len: Some(24),
            mute: true,
            transpose: -2,
            ..Block::new(16, PatternRef(3))
        });
        with_song.song.end = Some(64);
        with_song.song.loop_range = Some((16, 40));
        for p in [Project::default(), crate::demo::demo_project(), with_song] {
            assert_eq!(Project::from_ron(&p.to_ron()).unwrap(), p);
        }
    }

    #[test]
    fn a_song_may_not_name_a_machine_lane_twice() {
        let mut p = Project::default();
        p.song.lanes = vec![(MachineUid(1), Vec::new()), (MachineUid(1), Vec::new())];
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
        p.song
            .lane_mut(MachineUid(42))
            .push(Block::new(0, PatternRef(0)));
        assert!(p.validate().is_err(), "lane for unknown uid");
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
    fn any_rack_layout_validates() {
        let mut p = Project::default();
        p.machines.swap(0, 2);
        p.validate().unwrap();
        p.machines.pop();
        p.validate().unwrap();
        p.machines.clear();
        p.validate().unwrap();
    }

    #[test]
    fn a_project_may_hold_forty_machines() {
        let mut p = Project::default();
        while p.machines.len() < 40 {
            let m = p.new_machine(MachineType::Fm).unwrap();
            p.insert_machine(p.machines.len(), m).unwrap();
        }
        p.validate().unwrap();
        let text = p.to_ron();
        assert_eq!(Project::from_ron(&text).unwrap().machines.len(), 40);
    }

    #[test]
    fn uids_run_out_at_the_top_of_the_range() {
        let mut p = Project::default();
        p.next_uid = u16::MAX;
        assert!(p.new_machine(MachineType::Fm).is_none());
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
        p.kit_mut(MachineUid(3)).unwrap().patterns[1].roll.length = 9;
        assert_eq!(p.kit(MachineUid(3)).unwrap().patterns[1].roll.length, 9);
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
    fn song_lanes_are_empty_when_absent() {
        let mut song = Song::default();
        assert!(song.lane(MachineUid(3)).is_empty());
        song.lane_mut(MachineUid(3))
            .push(Block::new(0, PatternRef(2)));
        song.lane_mut(MachineUid(3))
            .push(Block::new(16, PatternRef(2)));
        assert_eq!(song.lanes.len(), 1);
        assert_eq!(song.lane(MachineUid(3)).len(), 2);
        assert!(song.lane(MachineUid(4)).is_empty());
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

    /// Fills `uid`'s slots with silent mono 192 kHz audio whose decoded
    /// size (`memory_bytes`) totals `bytes` (a multiple of 4).
    fn fill_sampler(p: &mut Project, uid: MachineUid, bytes: usize) {
        use crate::sampler::slot::{SlotData, SlotMeta};
        let mut left = bytes / 4;
        let s = p.sampler_mut(uid).unwrap();
        for (slot, entry) in s.slots.iter_mut().enumerate() {
            let n = left.min(5_700_000);
            left -= n;
            if n == 0 {
                break;
            }
            let key = 24 + 2 * slot as u8;
            let d = SlotData {
                name: "big".into(),
                meta: SlotMeta::chromatic(key, key + 1, key, n as u32),
                sample_rate: 192_000,
                channels: 1,
                pcm: vec![0; n],
            };
            assert!(d.is_valid(), "{:?}", d.problem());
            *entry = Some(d);
        }
        assert_eq!(left, 0, "more audio than one sampler holds");
        assert_eq!(p.sampler(uid).unwrap().memory_bytes(), bytes);
    }

    #[test]
    fn a_project_may_hold_more_than_256_mb_of_audio() {
        let mut p = Project::default();
        fill_sampler(&mut p, MachineUid(6), 300 * 1024 * 1024);
        p.validate().unwrap();
        assert_eq!(p.sample_bytes(), 300 * 1024 * 1024);
    }

    #[test]
    fn a_project_over_the_old_limit_round_trips() {
        let mut p = Project::default();
        fill_sampler(&mut p, MachineUid(6), 90 * 1024 * 1024); // > 64 MB, < 256 MB
        let q = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(q, p);
    }

    #[test]
    fn embedded_audio_over_the_budget_is_refused_while_parsing() {
        // Two slots of 1 MB of 16-bit audio against a 1.5 MB budget.
        let mut entries: Vec<String> = (0..2u8).map(|i| slot_entry(i * 8, 500_000).0).collect();
        entries.resize(crate::sampler::SLOTS, "None".into());
        let text = ron_with_slot_entries(&entries);
        match Project::from_ron_within(&text, Some(1_500_000)) {
            Err(ProjectError::Parse(e)) => assert!(e.contains("free memory"), "{e}"),
            other => panic!("expected a parse error, got {other:?}"),
        }
        // The budget is per parse: the same text loads within a larger one,
        // and without any.
        assert!(Project::from_ron_within(&text, Some(2_000_000)).is_ok());
        assert!(Project::from_ron_within(&text, Some(2_000_000)).is_ok());
        assert!(Project::from_ron_within(&text, None).is_ok());
        assert!(Project::from_ron(&text).is_ok());
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
        // The file's one acid step: note 5, octave 0, On.
        assert_eq!(
            p.synth(MachineUid(1)).unwrap().patterns[0].roll.notes,
            vec![Note::new(0, 4, 41, crate::steps::ACID_VEL)]
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
        assert!(synth_mut(&mut p, 0).patterns[0].roll.notes.is_empty());
        assert!(kit_mut(&mut p, 0).patterns[0].roll.notes.is_empty());
        assert_eq!(synth_mut(&mut p, 0).patterns[0].roll.length, 16);
        let params: usize = p.machines.iter().map(|m| m.params.len()).sum();
        assert_eq!(params + p.globals.len(), 589);
        p.validate().unwrap();
    }

    #[test]
    fn step_midi_uses_octave_note_and_transpose() {
        use crate::steps::{Gate, SynthStep};
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
        synth_mut(&mut p, 1).patterns[5].roll.notes.push(Note {
            accent: true,
            slide: true,
            ..Note::new(12, 4, 31, 100)
        });
        kit_mut(&mut p, 0).patterns[2].roll.notes.push(Note {
            accent: true,
            ..Note::new(16, 4, 0, 100)
        });
        p.set_param(MachineUid(1).param(SynthParam::Resonance), 0.95);
        p.song
            .lane_mut(MachineUid(1))
            .push(Block::new(0, PatternRef(5)));
        p.song
            .lane_mut(MachineUid(3))
            .push(Block::new(0, PatternRef(2)));
        p.song.lane_mut(MachineUid(5)).push(Block {
            transpose: -5,
            ..Block::new(0, PatternRef(9))
        });
        p.song.lane_mut(MachineUid(2)).push(Block {
            mute: true,
            transpose: 3,
            len: Some(7),
            ..Block::new(16, PatternRef(0))
        });
        fm_mut(&mut p).patterns[9]
            .notes
            .push(Note::new(3, 6, 64, 90));
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
        synth_mut(&mut p, 0).patterns[0].roll.length = 0;
        assert!(matches!(p.validate(), Err(ProjectError::Invalid(_))));
        let mut p = Project::default();
        kit_mut(&mut p, 0).patterns[0].roll.notes.push(Note::new(
            0,
            4,
            DRUM_INSTRUMENTS as u8,
            100,
        ));
        assert!(
            matches!(p.validate(), Err(ProjectError::Invalid(_))),
            "a drum note past the last instrument"
        );
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
        let invalid = |p: &Project| matches!(p.validate(), Err(ProjectError::Invalid(_)));
        let max = crate::song::MAX_SONG_STEPS as u32;
        let with_lane = |blocks: Vec<Block>| {
            let mut p = Project::default();
            p.synth_mut(MachineUid(1)).unwrap().patterns[1].roll.length = 32;
            *p.song.lane_mut(MachineUid(1)) = blocks;
            p
        };
        let b = |start, slot| Block::new(start, PatternRef(slot));
        assert!(with_lane(vec![b(0, 1), b(32, 0)]).validate().is_ok());
        assert!(invalid(&with_lane(vec![b(0, 1), b(31, 0)])), "overlap");
        assert!(invalid(&with_lane(vec![b(32, 0), b(0, 0)])), "out of order");
        assert!(invalid(&with_lane(vec![b(0, PATTERNS as u8)])), "bad ref");
        let no_len = Block {
            len: Some(0),
            ..b(0, 0)
        };
        assert!(invalid(&with_lane(vec![no_len])), "zero length");
        let fixed = Block {
            len: Some(8),
            ..b(0, 1)
        };
        assert!(
            with_lane(vec![fixed, b(8, 0)]).validate().is_ok(),
            "own length"
        );
        assert!(with_lane(vec![b(max - 16, 0)]).validate().is_ok());
        assert!(invalid(&with_lane(vec![b(max - 15, 0)])), "past the limit");

        let mut p = Project::default();
        p.song.end = Some(max);
        p.validate().unwrap();
        p.song.end = Some(max + 1);
        assert!(invalid(&p), "end past the limit");
        let mut p = Project::default();
        p.song.loop_range = Some((32, 16));
        assert!(invalid(&p));
        p.song.loop_range = Some((16, 16));
        assert!(invalid(&p), "empty loop");
        p.song.loop_range = Some((16, max + 1));
        assert!(invalid(&p));
        p.song.loop_range = Some((16, 32));
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
        p.notes.push(Note::new(0, 4, 60, 100));
        assert!(p.is_well_formed());
        p.length = 4;
        p.notes.push(Note::new(200, 1, 60, 1));
        assert!(p.is_well_formed(), "notes past a shortened end are kept");
        let mut bad = p.clone();
        bad.notes.push(Note::new(0, 0, 60, 100));
        assert!(!bad.is_well_formed(), "zero length");
        let mut bad = p.clone();
        bad.notes.push(Note::new(0, 1, 60, 0));
        assert!(!bad.is_well_formed(), "zero velocity");
        let mut bad = p.clone();
        bad.notes.push(Note::new(0, 1, 128, 1));
        assert!(!bad.is_well_formed(), "pitch above 127");
        let mut bad = p.clone();
        bad.notes.push(Note::new(256, 1, 1, 1));
        assert!(!bad.is_well_formed(), "start past 64 steps");
        let mut bad = NotePattern::default();
        bad.notes = vec![Note::new(0, 1, 1, 1); MAX_NOTES + 1];
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
        fm_mut(&mut p).patterns[1]
            .notes
            .push(Note::new(0, 1, 60, 0));
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
        assert_eq!(p.synth(MachineUid(2)).unwrap().patterns[2].roll.length, 7);
        let selected: Vec<_> = p.machines.iter().map(|m| m.selected).collect();
        assert_eq!(selected, [1, 2, 3, 4, 0, 0]);
        assert_eq!(p.param(MachineUid(1).param(SynthParam::Cutoff)), 0.9);
        let ch = MachineParam::Channel(crate::params::ChannelParam::Level);
        assert_eq!(p.machines[5].params.get(&ch), Some(&ch.info().default));
        // The one bar's cells with a pattern become blocks at step 0; the
        // 909's mute and Synth 2's transpose had no pattern to act on.
        let synth = Block {
            mute: true,
            transpose: 2,
            ..Block::new(0, PatternRef(1))
        };
        assert_eq!(
            p.song.lanes,
            [
                (MachineUid(1), vec![synth]),
                (MachineUid(3), vec![Block::new(0, PatternRef(3))])
            ]
        );
        assert_eq!(p.song.end, Some(16));
        assert_eq!(p.song.loop_range, Some((0, 16)), "bar 0 as steps");
        assert_eq!(p.fm(FM).unwrap().patterns.len(), PATTERNS);
        assert!(sampler(&p).patterns.iter().all(|n| n.notes.is_empty()));
        assert_eq!(p.kit(MachineUid(4)).unwrap().kind, KitKind::K909);
    }

    #[test]
    fn a_project_with_a_mono_machine_round_trips() {
        let mut p = Project::default();
        let m = p.new_machine(MachineType::Mono).unwrap();
        let uid = m.uid;
        p.machines.push(m);
        p.mono_mut(uid).unwrap().patch_name = "Squelch".into();
        p.validate().unwrap();
        let back = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.mono(uid).unwrap().patterns.len(), PATTERNS);
    }

    #[test]
    fn a_project_with_a_pad_kit_round_trips() {
        use crate::padkit::pad::Layer;
        let mut p = Project::default();
        let m = p.new_machine(MachineType::PadKit).unwrap();
        let uid = m.uid;
        p.insert_machine(p.machines.len(), m).unwrap();
        let kit = p.padkit_mut(uid).unwrap();
        kit.pads[3]
            .layers
            .push(Layer::new("snare", 48_000, 2, &[0.25; 64]));
        kit.pads[3].split_evenly();
        kit.patterns[0].notes.push(Note::new(0, 4, 3, 100));
        p.validate().unwrap();
        let back = Project::from_ron(&p.to_ron()).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.machine(uid).unwrap().name, "Pads");
    }

    #[test]
    fn a_pad_kit_note_past_pad_sixteen_is_invalid() {
        let mut p = Project::default();
        let m = p.new_machine(MachineType::PadKit).unwrap();
        let uid = m.uid;
        p.insert_machine(p.machines.len(), m).unwrap();
        p.padkit_mut(uid).unwrap().patterns[0]
            .notes
            .push(Note::new(0, 4, 16, 100));
        assert!(p.validate().is_err());
    }

    #[test]
    fn version_6_files_still_load() {
        let text = Project::default()
            .to_ron()
            .replace("version: 7", "version: 6");
        assert_eq!(Project::from_ron(&text).unwrap().version, PROJECT_VERSION);
    }

    #[test]
    fn pad_audio_counts_toward_sample_bytes() {
        use crate::padkit::pad::Layer;
        let mut p = Project::default();
        let m = p.new_machine(MachineType::PadKit).unwrap();
        let uid = m.uid;
        p.insert_machine(p.machines.len(), m).unwrap();
        let before = p.sample_bytes();
        p.padkit_mut(uid).unwrap().pads[0]
            .layers
            .push(Layer::new("x", 44_100, 1, &[0.0; 1000]));
        assert_eq!(p.sample_bytes(), before + 1000 * 4);
    }

    #[test]
    fn version_3_files_still_load() {
        for version in [3, 4] {
            let v4 = crate::migrate::ProjectV4 {
                version,
                ..crate::migrate::ProjectV4::standard()
            };
            let p = Project::from_ron(&ron::ser::to_string(&v4).unwrap()).unwrap();
            assert_eq!(p.version, PROJECT_VERSION);
            assert_eq!(p, Project::default(), "v{version}");
        }
    }

    #[test]
    fn notes_save_without_default_fields_and_load_them_back() {
        let n = Note::new(4, 4, 60, 100);
        let text = ron::ser::to_string(&n).unwrap();
        assert_eq!(text, "(start:4,len:4,pitch:60,vel:100)");
        let back: Note = ron::from_str(&text).unwrap();
        assert_eq!(back, n);
        assert_eq!(
            (back.chance, back.ratchet, back.cond),
            (100, 1, Cond::Always)
        );
        let flagged = Note {
            accent: true,
            slide: true,
            chance: 50,
            ratchet: 3,
            cond: Cond::Ratio(1, 4),
            ..n
        };
        let back: Note = ron::from_str(&ron::ser::to_string(&flagged).unwrap()).unwrap();
        assert_eq!(back, flagged);
    }

    #[test]
    fn note_fields_are_validated() {
        let ok = |n: Note| {
            NotePattern {
                notes: vec![n],
                length: 16,
            }
            .is_well_formed()
        };
        let n = Note::new(0, 4, 60, 100);
        assert!(ok(n));
        assert!(!ok(Note { chance: 101, ..n }));
        assert!(!ok(Note { ratchet: 0, ..n }));
        assert!(!ok(Note { ratchet: 9, ..n }));
        assert!(ok(Note { ratchet: 8, ..n }));
        assert!(!ok(Note {
            cond: Cond::Ratio(0, 4),
            ..n
        }));
        assert!(!ok(Note {
            cond: Cond::Ratio(5, 4),
            ..n
        }));
        assert!(!ok(Note {
            cond: Cond::Ratio(1, 9),
            ..n
        }));
        assert!(ok(Note {
            cond: Cond::Ratio(4, 4),
            ..n
        }));
    }
}
