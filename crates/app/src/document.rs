//! The editable project plus undo/redo. Every change is an `Edit`;
//! applying one returns the engine `Command`s that bring the audio
//! thread in line. Undo stores the inverse edit.

use acid_engine::fm::sysex::Dx7Voice;
use acid_engine::host::{Command, PatternData};
use acid_engine::machine::{MAX_MACHINES, MachineType, MachineUid, default_name};
use acid_engine::params::{DRUM_INSTRUMENTS, ParamId};
use acid_engine::project::{
    DrumPattern, MachineBody, MachineEntry, NotePattern, Project, RemovedMachine, STEPS, Song,
    SynthPattern,
};
use acid_engine::rack::CompiledSong;
use acid_engine::sampler::slot::{SlotData, SlotMeta, SlotPlay};
use acid_engine::song;
use std::path::PathBuf;
use std::sync::Arc;

pub const UNDO_LIMIT: usize = 200;

// Panic messages for an edit whose uid is not a machine of its type.
const NO_MACHINE: &str = "the machine is in the rack";
const NO_SYNTH: &str = "the edit names a synth in the rack";
const NO_KIT: &str = "the edit names a drum kit in the rack";
const NO_NOTES: &str = "the edit names a machine with patterns in the rack";
const NO_FM: &str = "the edit names an FM machine in the rack";
const NO_MONO: &str = "the edit names a Mono machine in the rack";
const NO_SAMPLER: &str = "the edit names a sampler in the rack";
const NO_ROOM: &str = "the rack has room for the machine and its uid is free";

/// One change to the project. Each edit names its machine by `uid`, which
/// must be a machine of the edit's type in the rack (a note edit's: any
/// machine), and indices must be in range, or `Document::apply` panics:
/// `slot` < `PATTERNS` (a sampler `slot` < `SLOTS`). Values are clamped: params to 0..=1,
/// lengths to 1..=`STEPS`, transpose to -12..=12, BPM and swing to their
/// ranges.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Param {
        id: ParamId,
        value: f32,
    },
    SynthTranspose {
        uid: MachineUid,
        slot: u8,
        transpose: i8,
    },
    SynthPattern {
        uid: MachineUid,
        slot: u8,
        pattern: Box<SynthPattern>,
    },
    DrumPattern {
        uid: MachineUid,
        slot: u8,
        pattern: Box<DrumPattern>,
    },
    NotePattern {
        uid: MachineUid,
        slot: u8,
        pattern: Box<NotePattern>,
    },
    NoteLength {
        uid: MachineUid,
        slot: u8,
        length: u8,
    },
    Bpm(f32),
    Swing(f32),
    Song {
        song: Box<Song>,
    },
    /// Loading an FM voice: many params and the voice name, one undo step.
    /// Values apply in order; a repeated id ends on its last value.
    FmVoice {
        uid: MachineUid,
        name: String,
        values: Vec<(ParamId, f32)>,
    },
    FmBank {
        uid: MachineUid,
        bank: Option<Vec<Dx7Voice>>,
    },
    /// Loading a Mono patch: its name and every knob, one undo step.
    MonoPatch {
        uid: MachineUid,
        name: String,
        values: Vec<(ParamId, f32)>,
    },
    /// Loading or clearing a sampler slot, with its knob values: one undo step.
    SamplerSlot {
        uid: MachineUid,
        slot: usize,
        data: Option<Box<SlotData>>,
        values: Vec<(ParamId, f32)>,
    },
    /// Changing a loaded slot's zone, slices, mode or markers.
    SlotMeta {
        uid: MachineUid,
        slot: usize,
        meta: Box<SlotMeta>,
    },
    /// Putting a machine into the rack with everything it took out when
    /// removed: undo of a removal, and adding or duplicating (nothing else
    /// to restore). The rack must have room and the uid must be free.
    InsertMachine {
        removed: Box<RemovedMachine>,
    },
    /// Taking a machine out with its song lane and automation lanes.
    RemoveMachine {
        uid: MachineUid,
    },
    /// Moving a machine to rack position `to` (clamped).
    MoveMachine {
        uid: MachineUid,
        to: usize,
    },
    /// Setting a machine's name exactly (`Document::rename_machine`
    /// resolves an empty name to the type's default).
    RenameMachine {
        uid: MachineUid,
        name: String,
    },
    /// Several edits as one undo step (a record pass), applied in order.
    Group(Vec<Edit>),
}

fn is_song(edit: &Edit) -> bool {
    matches!(edit, Edit::Song { .. })
}

/// Which edits may fold into one undo step during a gesture.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MergeKey {
    Param(ParamId),
    SynthTranspose(MachineUid, u8),
    SynthPattern(MachineUid, u8),
    DrumPattern(MachineUid, u8),
    NotePattern(MachineUid, u8),
    NoteLength(MachineUid, u8),
    Bpm,
    Swing,
    Song,
    FmVoice(MachineUid),
    MonoPatch(MachineUid),
    FmBank(MachineUid),
    SamplerSlot(MachineUid, usize),
    SlotMeta(MachineUid, usize),
    Group,
    /// Rack edits: never merge, each is its own undo step.
    Rack,
}

fn merge_key(edit: &Edit) -> MergeKey {
    match edit {
        Edit::Param { id, .. } => MergeKey::Param(*id),
        Edit::SynthTranspose { uid, slot, .. } => MergeKey::SynthTranspose(*uid, *slot),
        Edit::SynthPattern { uid, slot, .. } => MergeKey::SynthPattern(*uid, *slot),
        Edit::DrumPattern { uid, slot, .. } => MergeKey::DrumPattern(*uid, *slot),
        Edit::NotePattern { uid, slot, .. } => MergeKey::NotePattern(*uid, *slot),
        Edit::NoteLength { uid, slot, .. } => MergeKey::NoteLength(*uid, *slot),
        Edit::Bpm(_) => MergeKey::Bpm,
        Edit::Swing(_) => MergeKey::Swing,
        Edit::Song { .. } => MergeKey::Song,
        Edit::FmVoice { uid, .. } => MergeKey::FmVoice(*uid),
        Edit::MonoPatch { uid, .. } => MergeKey::MonoPatch(*uid),
        Edit::FmBank { uid, .. } => MergeKey::FmBank(*uid),
        Edit::SamplerSlot { uid, slot, .. } => MergeKey::SamplerSlot(*uid, *slot),
        Edit::SlotMeta { uid, slot, .. } => MergeKey::SlotMeta(*uid, *slot),
        Edit::Group(_) => MergeKey::Group,
        Edit::InsertMachine { .. }
        | Edit::RemoveMachine { .. }
        | Edit::MoveMachine { .. }
        | Edit::RenameMachine { .. } => MergeKey::Rack,
    }
}

/// One undo or redo step: the edit that reverses it, and the generation
/// of the document state the step leads to when redone.
struct Step {
    edit: Edit,
    generation: u64,
}

/// Approximate size of the sample data an edit holds.
fn edit_bytes(edit: &Edit) -> usize {
    match edit {
        Edit::SamplerSlot { data: Some(d), .. } => d.pcm.len() * std::mem::size_of::<i16>(),
        Edit::Group(edits) => edits.iter().map(edit_bytes).sum(),
        Edit::InsertMachine { removed } => match &removed.entry.body {
            MachineBody::Sampler(s) => s
                .slots
                .iter()
                .flatten()
                .map(|d| d.pcm.len() * std::mem::size_of::<i16>())
                .sum(),
            _ => 0,
        },
        _ => 0,
    }
}

pub struct Document {
    pub project: Project,
    pub path: Option<PathBuf>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Merge key of the open gesture, if the last edit was a merging one.
    gesture: Option<MergeKey>,
    /// Generation of the state at the bottom of the undo history.
    base_generation: u64,
    /// Last generation handed out; every edit gets a fresh one.
    last_generation: u64,
    /// Generation of the state last saved (or loaded).
    saved_generation: u64,
    /// A change outside undo (a pattern selection) since the last save.
    unsaved_selection: bool,
    /// Sample bytes the undo history may hold; see `UNDO_SAMPLE_BYTES`.
    undo_byte_budget: usize,
    /// The newest undo step is the open record pass's `Edit::Group`.
    pass_open: bool,
    /// Why the last edit was refused (it changed nothing), until taken.
    refusal: Option<&'static str>,
}

/// A pattern edit refused because its ripple would not fit the song.
pub const SONG_TOO_LONG: &str = "That would push the song past its maximum length";

/// Sample data (PCM in `SlotData`) the undo history may hold: as much
/// 16-bit PCM as the whole 256 MB (f32) sample pool decodes from, so a
/// full pool's worth of reloads stays undoable. Past it the oldest steps are dropped, the newest always kept.
pub const UNDO_SAMPLE_BYTES: usize = 128 * 1024 * 1024;

impl Document {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            path: None,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            base_generation: 0,
            last_generation: 0,
            saved_generation: 0,
            unsaved_selection: false,
            undo_byte_budget: UNDO_SAMPLE_BYTES,
            pass_open: false,
            refusal: None,
        }
    }

    /// Applies `edit` and returns the engine commands to send. With
    /// `merge`, consecutive edits to the same target form one undo step
    /// until `end_gesture`. An edit that changes nothing adds no step and
    /// leaves the dirty state and any open gesture alone.
    ///
    /// Uids and indices must be valid (see `Edit`), or this panics. Values are clamped to their valid ranges.
    pub fn apply(&mut self, edit: Edit, merge: bool) -> Vec<Command> {
        let key = merge_key(&edit);
        let continuing = merge && self.gesture == Some(key) && !self.undo.is_empty();
        self.refusal = None;
        let inverse = self.apply_raw(edit);
        if self.refusal.is_some() {
            return Vec::new();
        }
        let cmds = self.commands_for(&inverse);
        // A stray no-op in the middle of a drag must not split its step.
        if self.is_no_op(&inverse) {
            return cmds;
        }
        self.last_generation += 1;
        let generation = self.last_generation;
        if continuing {
            if let Some(top) = self.undo.last_mut() {
                top.generation = generation;
                // The gesture's first ripple: undo must restore the song
                // from before it too (earlier merged edits left it alone).
                if let Edit::Group(mut inv) = inverse
                    && !matches!(&top.edit, Edit::Group(e) if e.iter().any(is_song))
                    && let Some(i) = inv.iter().position(is_song)
                {
                    let old = std::mem::replace(&mut top.edit, Edit::Group(Vec::new()));
                    top.edit = Edit::Group(vec![old, inv.swap_remove(i)]);
                }
            }
        } else {
            self.undo.push(Step {
                edit: inverse,
                generation,
            });
            self.pass_open = false;
        }
        self.gesture = merge.then_some(key);
        self.redo.clear();
        self.trim_undo();
        cmds
    }

    /// Applies an edit made during a record pass (recorded song changes,
    /// switches, tempo and swing). All of a pass's edits form one undo
    /// step until `end_pass`. An ordinary `apply` in between closes that
    /// step, so the rest of the pass becomes a step of its own: undo never
    /// reorders history. An edit that changes nothing is not recorded.
    pub fn record(&mut self, edit: Edit) -> Vec<Command> {
        self.refusal = None;
        let inverse = self.apply_raw(edit);
        if self.refusal.is_some() {
            return Vec::new();
        }
        let cmds = self.commands_for(&inverse);
        if self.is_no_op(&inverse) {
            return cmds;
        }
        self.gesture = None;
        self.last_generation += 1;
        let generation = self.last_generation;
        match self.undo.last_mut() {
            Some(Step {
                edit: Edit::Group(edits),
                generation: top,
            }) if self.pass_open => {
                // Undo needs only each target's value from before the pass.
                let key = merge_key(&inverse);
                if !edits.iter().any(|e| merge_key(e) == key) {
                    edits.push(inverse);
                }
                *top = generation;
            }
            _ => {
                self.undo.push(Step {
                    edit: Edit::Group(vec![inverse]),
                    generation,
                });
                self.pass_open = true;
            }
        }
        self.redo.clear();
        self.trim_undo();
        cmds
    }

    /// Closes the record pass: later edits start new undo steps.
    pub fn end_pass(&mut self) {
        self.pass_open = false;
        self.gesture = None;
    }

    /// Drops the oldest steps past `UNDO_LIMIT` or the sample byte budget.
    fn trim_undo(&mut self) {
        let mut bytes: usize = self.undo_sample_bytes();
        let mut drop = 0;
        while self.undo.len() - drop > 1
            && (self.undo.len() - drop > UNDO_LIMIT || bytes > self.undo_byte_budget)
        {
            bytes -= edit_bytes(&self.undo[drop].edit);
            drop += 1;
        }
        if drop > 0 {
            self.base_generation = self.undo[drop - 1].generation;
            self.undo.drain(..drop);
        }
    }

    /// Sample bytes held by the undo and redo history.
    pub(crate) fn undo_sample_bytes(&self) -> usize {
        self.undo
            .iter()
            .chain(&self.redo)
            .map(|s| edit_bytes(&s.edit))
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn set_undo_byte_budget(&mut self, bytes: usize) {
        self.undo_byte_budget = bytes;
    }

    /// Whether `inverse` (just returned by `apply_raw`) restores what is
    /// already there: the edit changed nothing. Only cheap targets are
    /// compared; the rest always count as a change.
    fn is_no_op(&self, inverse: &Edit) -> bool {
        let p = &self.project;
        match inverse {
            Edit::Param { id, value } => p.param(*id) == *value,
            Edit::Bpm(bpm) => p.bpm == *bpm,
            Edit::Swing(swing) => p.swing == *swing,
            Edit::SynthTranspose {
                uid,
                slot,
                transpose,
            } => p.synth(*uid).expect(NO_SYNTH).patterns[*slot as usize].transpose == *transpose,
            Edit::NoteLength { uid, slot, length } => {
                p.roll(*uid, *slot).is_some_and(|r| r.length == *length)
            }
            Edit::MoveMachine { uid, to } => p.position(*uid).expect(NO_MACHINE) == *to,
            Edit::RenameMachine { uid, name } => p.machine(*uid).expect(NO_MACHINE).name == *name,
            _ => false,
        }
    }

    /// The state the document is in: the generation of the newest undo step.
    fn generation(&self) -> u64 {
        self.undo
            .last()
            .map_or(self.base_generation, |s| s.generation)
    }

    /// Unsaved changes: the state differs from the one last saved (undoing
    /// or redoing back to it counts as clean), or a pattern was selected.
    pub fn is_dirty(&self) -> bool {
        self.unsaved_selection || self.generation() != self.saved_generation
    }

    /// The current state is now the saved one.
    pub fn mark_saved(&mut self) {
        self.saved_generation = self.generation();
        self.unsaved_selection = false;
    }

    /// Marks unsaved changes without an edit (tests).
    #[cfg(test)]
    pub(crate) fn mark_dirty(&mut self) {
        self.unsaved_selection = true;
    }

    pub fn end_gesture(&mut self) {
        self.gesture = None;
    }

    /// Changes with each edit that changes the project (not undo/redo):
    /// equal serials mean no edit came in between.
    pub fn edit_serial(&self) -> u64 {
        self.last_generation
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The edit the next `undo` applies (the inverse of the last change).
    pub fn next_undo(&self) -> Option<&Edit> {
        self.undo.last().map(|step| &step.edit)
    }

    /// The edit the next `redo` applies.
    pub fn next_redo(&self) -> Option<&Edit> {
        self.redo.last().map(|step| &step.edit)
    }

    pub fn undo(&mut self) -> Vec<Command> {
        self.gesture = None;
        self.pass_open = false;
        let Some(step) = self.undo.pop() else {
            return Vec::new();
        };
        let untaken = self.refusal.take();
        let inverse = self.apply_raw(step.edit);
        debug_assert!(self.refusal.is_none(), "history steps always fit");
        self.refusal = untaken;
        let cmds = self.commands_for(&inverse);
        self.redo.push(Step {
            edit: inverse,
            generation: step.generation,
        });
        cmds
    }

    pub fn redo(&mut self) -> Vec<Command> {
        self.gesture = None;
        self.pass_open = false;
        let Some(step) = self.redo.pop() else {
            return Vec::new();
        };
        let untaken = self.refusal.take();
        let inverse = self.apply_raw(step.edit);
        debug_assert!(self.refusal.is_none(), "history steps always fit");
        self.refusal = untaken;
        let cmds = self.commands_for(&inverse);
        self.undo.push(Step {
            edit: inverse,
            generation: step.generation,
        });
        cmds
    }

    /// Selecting a pattern is a transport action: saved, not undoable.
    /// `uid` must be in the rack.
    pub fn select_pattern(&mut self, uid: MachineUid, slot: u8) -> Vec<Command> {
        let entry = self.project.machine_mut(uid).expect(NO_MACHINE);
        if entry.selected != slot {
            entry.selected = slot;
            self.unsaved_selection = true;
        }
        vec![Command::SelectPattern { uid, slot }]
    }

    /// Undo steps available.
    #[cfg(test)]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Adds a fresh machine of `ty` at the end of the rack as one undo
    /// step. Returns its uid, or None when the rack is full.
    pub fn add_machine(&mut self, ty: MachineType) -> Option<(MachineUid, Vec<Command>)> {
        if self.project.machines.len() >= MAX_MACHINES {
            return None;
        }
        let index = self.project.machines.len();
        let entry = self.project.new_machine(ty);
        Some(self.insert_new(index, entry))
    }

    /// Adds a deep copy of `uid` directly below it as one undo step.
    /// Returns the copy's uid, or None when the rack is full or `uid` absent.
    pub fn duplicate_machine(&mut self, uid: MachineUid) -> Option<(MachineUid, Vec<Command>)> {
        if self.project.machines.len() >= MAX_MACHINES {
            return None;
        }
        let index = self.project.position(uid)? + 1;
        let entry = self.project.duplicate_machine(uid)?;
        Some(self.insert_new(index, entry))
    }

    fn insert_new(&mut self, index: usize, entry: MachineEntry) -> (MachineUid, Vec<Command>) {
        let uid = entry.uid;
        let removed = Box::new(RemovedMachine {
            index,
            entry,
            song_lane: None,
            lanes: Vec::new(),
            param_lanes: Vec::new(),
        });
        (uid, self.apply(Edit::InsertMachine { removed }, false))
    }

    /// Removes `uid` with its song lane and automation lanes as one undo step.
    /// Returns its name, or None when it is not in the rack.
    pub fn remove_machine(&mut self, uid: MachineUid) -> Option<(String, Vec<Command>)> {
        let name = self.project.machine(uid)?.name.clone();
        Some((name, self.apply(Edit::RemoveMachine { uid }, false)))
    }

    /// Moves `uid` (in the rack) to position `to` (clamped).
    pub fn move_machine(&mut self, uid: MachineUid, to: usize) -> Vec<Command> {
        self.apply(Edit::MoveMachine { uid, to }, false)
    }

    /// Renames `uid` (in the rack); an empty or all-whitespace name becomes
    /// the type's default name.
    pub fn rename_machine(&mut self, uid: MachineUid, name: &str) -> Vec<Command> {
        let name = if name.trim().is_empty() {
            let ty = self.project.machine(uid).expect(NO_MACHINE).machine_type();
            let others: Vec<&str> = self
                .project
                .machines
                .iter()
                .filter(|m| m.uid != uid)
                .map(|m| m.name.as_str())
                .collect();
            default_name(ty, &others)
        } else {
            name.to_string()
        };
        self.apply(Edit::RenameMachine { uid, name }, false)
    }

    /// Why the last `apply` or `record` changed nothing, once.
    pub fn take_refusal(&mut self) -> Option<&'static str> {
        self.refusal.take()
    }

    /// Applies an edit and returns its inverse. A pattern edit that
    /// lengthens the pattern ripples its machine's song lane in the same
    /// step (the inverse then also restores the song); when the ripple
    /// would not fit, nothing changes, `refusal` is set and the inverse
    /// is an empty group. Undo and redo along the history always fit:
    /// growing back after a shrink finds the gaps it left, and a ripple
    /// already applied is its own fixed point.
    fn apply_raw(&mut self, edit: Edit) -> Edit {
        let target = match &edit {
            Edit::SynthPattern { uid, slot, .. }
            | Edit::DrumPattern { uid, slot, .. }
            | Edit::NotePattern { uid, slot, .. }
            | Edit::NoteLength { uid, slot, .. } => Some((*uid, *slot)),
            _ => None,
        };
        let Some((uid, slot)) = target else {
            return self.apply_one(edit);
        };
        let old_len = song::lens_of(&self.project)(uid, slot);
        let inverse = self.apply_one(edit);
        let p = &self.project;
        let mut lane = p.song.lane(uid).to_vec();
        let fits = {
            let lens = song::lens_of(p);
            let new_len = lens(uid, slot);
            if new_len <= old_len || lane.is_empty() {
                return inverse;
            }
            song::ripple(&mut lane, uid, slot, old_len, new_len, &lens)
        };
        if !fits {
            self.apply_one(inverse);
            self.refusal = Some(SONG_TOO_LONG);
            return Edit::Group(Vec::new());
        }
        if lane == p.song.lane(uid) {
            return inverse;
        }
        let old = self.project.song.clone();
        *self.project.song.lane_mut(uid) = lane;
        Edit::Group(vec![
            inverse,
            Edit::Song {
                song: Box::new(old),
            },
        ])
    }

    /// `apply_raw` without the ripple.
    fn apply_one(&mut self, edit: Edit) -> Edit {
        let p = &mut self.project;
        match edit {
            Edit::Param { id, value } => {
                let old = p.param(id);
                p.set_param(id, value.clamp(0.0, 1.0));
                Edit::Param { id, value: old }
            }
            Edit::SynthTranspose {
                uid,
                slot,
                transpose,
            } => {
                let pat = &mut p.synth_mut(uid).expect(NO_SYNTH).patterns[slot as usize];
                let old = std::mem::replace(&mut pat.transpose, transpose.clamp(-12, 12));
                Edit::SynthTranspose {
                    uid,
                    slot,
                    transpose: old,
                }
            }
            Edit::SynthPattern { uid, slot, pattern } => {
                let mut pattern = *pattern;
                pattern.roll.length = pattern.roll.length.clamp(1, STEPS as u8);
                pattern.transpose = pattern.transpose.clamp(-12, 12);
                debug_assert!(pattern.is_well_formed(), "malformed synth pattern");
                let old = std::mem::replace(
                    &mut p.synth_mut(uid).expect(NO_SYNTH).patterns[slot as usize],
                    pattern,
                );
                Edit::SynthPattern {
                    uid,
                    slot,
                    pattern: Box::new(old),
                }
            }
            Edit::DrumPattern { uid, slot, pattern } => {
                let mut pattern = *pattern;
                pattern.roll.length = pattern.roll.length.clamp(1, STEPS as u8);
                debug_assert!(pattern.is_well_formed(), "malformed drum pattern");
                let old = std::mem::replace(
                    &mut p.kit_mut(uid).expect(NO_KIT).patterns[slot as usize],
                    pattern,
                );
                Edit::DrumPattern {
                    uid,
                    slot,
                    pattern: Box::new(old),
                }
            }
            Edit::NotePattern { uid, slot, pattern } => {
                let mut pattern = *pattern;
                pattern.length = pattern.length.clamp(1, STEPS as u8);
                debug_assert!(pattern.is_well_formed(), "malformed note pattern");
                debug_assert!(
                    !matches!(p.machine(uid).map(|m| &m.body), Some(MachineBody::Kit(_)))
                        || pattern
                            .notes
                            .iter()
                            .all(|n| (n.pitch as usize) < DRUM_INSTRUMENTS),
                    "a kit note's pitch must be a drum instrument"
                );
                let pat = p.roll_mut(uid, slot).expect(NO_NOTES);
                let old = std::mem::replace(pat, pattern);
                Edit::NotePattern {
                    uid,
                    slot,
                    pattern: Box::new(old),
                }
            }
            Edit::NoteLength { uid, slot, length } => {
                let pat = p.roll_mut(uid, slot).expect(NO_NOTES);
                let old = std::mem::replace(&mut pat.length, length.clamp(1, STEPS as u8));
                Edit::NoteLength {
                    uid,
                    slot,
                    length: old,
                }
            }
            Edit::Bpm(bpm) => Edit::Bpm(std::mem::replace(&mut p.bpm, bpm.clamp(40.0, 300.0))),
            Edit::Swing(swing) => {
                Edit::Swing(std::mem::replace(&mut p.swing, swing.clamp(0.0, 0.75)))
            }
            Edit::Song { song } => Edit::Song {
                song: Box::new(std::mem::replace(&mut p.song, *song)),
            },
            Edit::FmVoice { uid, name, values } => {
                let old_name = std::mem::replace(&mut p.fm_mut(uid).expect(NO_FM).voice_name, name);
                let mut old_values: Vec<_> = values
                    .into_iter()
                    .map(|(id, v)| {
                        let old = p.param(id);
                        p.set_param(id, v.clamp(0.0, 1.0));
                        (id, old)
                    })
                    .collect();
                // Undo in reverse order, so a repeated id ends on its first old value.
                old_values.reverse();
                Edit::FmVoice {
                    uid,
                    name: old_name,
                    values: old_values,
                }
            }
            Edit::MonoPatch { uid, name, values } => {
                let old_name =
                    std::mem::replace(&mut p.mono_mut(uid).expect(NO_MONO).patch_name, name);
                let mut old_values: Vec<_> = values
                    .into_iter()
                    .map(|(id, v)| {
                        let old = p.param(id);
                        p.set_param(id, v.clamp(0.0, 1.0));
                        (id, old)
                    })
                    .collect();
                old_values.reverse();
                Edit::MonoPatch {
                    uid,
                    name: old_name,
                    values: old_values,
                }
            }
            Edit::SamplerSlot {
                uid,
                slot,
                data,
                values,
            } => {
                let old = std::mem::replace(
                    &mut p.sampler_mut(uid).expect(NO_SAMPLER).slots[slot],
                    data.map(|d| *d),
                );
                let mut old_values: Vec<_> = values
                    .into_iter()
                    .map(|(id, v)| {
                        let before = p.param(id);
                        p.set_param(id, v.clamp(0.0, 1.0));
                        (id, before)
                    })
                    .collect();
                old_values.reverse();
                Edit::SamplerSlot {
                    uid,
                    slot,
                    data: old.map(Box::new),
                    values: old_values,
                }
            }
            Edit::SlotMeta { uid, slot, meta } => {
                match p.sampler_mut(uid).expect(NO_SAMPLER).slots[slot].as_mut() {
                    Some(d) => Edit::SlotMeta {
                        uid,
                        slot,
                        meta: Box::new(std::mem::replace(&mut d.meta, *meta)),
                    },
                    None => Edit::SlotMeta { uid, slot, meta },
                }
            }
            Edit::FmBank { uid, bank } => Edit::FmBank {
                uid,
                bank: std::mem::replace(&mut p.fm_mut(uid).expect(NO_FM).bank, bank),
            },
            Edit::InsertMachine { removed } => {
                let uid = removed.entry.uid;
                assert!(
                    p.machines.len() < MAX_MACHINES && p.machine(uid).is_none(),
                    "{NO_ROOM}"
                );
                p.restore_machine(*removed);
                Edit::RemoveMachine { uid }
            }
            Edit::RemoveMachine { uid } => Edit::InsertMachine {
                removed: Box::new(p.remove_machine(uid).expect(NO_MACHINE)),
            },
            Edit::MoveMachine { uid, to } => Edit::MoveMachine {
                uid,
                to: p.move_machine(uid, to).expect(NO_MACHINE),
            },
            Edit::RenameMachine { uid, name } => Edit::RenameMachine {
                uid,
                name: std::mem::replace(&mut p.machine_mut(uid).expect(NO_MACHINE).name, name),
            },
            // All or nothing: a refused child undoes the ones before it.
            Edit::Group(edits) => {
                let mut inverses = Vec::with_capacity(edits.len());
                for e in edits {
                    let inverse = self.apply_raw(e);
                    if self.refusal.is_some() {
                        for done in inverses.into_iter().rev() {
                            self.apply_raw(done);
                        }
                        return Edit::Group(Vec::new());
                    }
                    inverses.push(inverse);
                }
                inverses.reverse();
                Edit::Group(inverses)
            }
        }
    }

    /// Commands that sync the engine with the project at `edit`'s target.
    fn commands_for(&self, edit: &Edit) -> Vec<Command> {
        let p = &self.project;
        match edit {
            Edit::Param { id, .. } => vec![Command::SetParam(*id, p.param(*id))],
            Edit::SynthTranspose { uid, slot, .. }
            | Edit::SynthPattern { uid, slot, .. }
            | Edit::DrumPattern { uid, slot, .. }
            | Edit::NotePattern { uid, slot, .. }
            | Edit::NoteLength { uid, slot, .. } => vec![pattern_command(p, *uid, *slot)],
            Edit::Bpm(_) => vec![Command::SetBpm(p.bpm)],
            Edit::Swing(_) => vec![Command::SetSwing(p.swing)],
            Edit::Song { .. } => vec![Command::Song(Box::new(CompiledSong::compile(
                &p.song,
                &p.rack_uids(),
            )))],
            Edit::FmVoice { values, .. } | Edit::MonoPatch { values, .. } => values
                .iter()
                .map(|(id, _)| Command::SetParam(*id, p.param(*id)))
                .collect(),
            Edit::FmBank { .. } => Vec::new(),
            Edit::SamplerSlot {
                uid, slot, values, ..
            } => {
                let data = p.sampler(*uid).expect(NO_SAMPLER).slots[*slot].as_ref();
                let mut cmds = vec![
                    Command::SamplerSample {
                        uid: *uid,
                        slot: *slot as u8,
                        sample: data.map(|d| Arc::new(d.to_sample())),
                    },
                    Command::SamplerPlay {
                        uid: *uid,
                        slot: *slot as u8,
                        play: Box::new(
                            data.map_or(SlotPlay::EMPTY, |d| SlotPlay::from_meta(&d.meta)),
                        ),
                    },
                ];
                cmds.extend(
                    values
                        .iter()
                        .map(|(id, _)| Command::SetParam(*id, p.param(*id))),
                );
                cmds
            }
            Edit::SlotMeta { uid, slot, .. } => vec![Command::SamplerPlay {
                uid: *uid,
                slot: *slot as u8,
                play: Box::new(
                    p.sampler(*uid).expect(NO_SAMPLER).slots[*slot]
                        .as_ref()
                        .map_or(SlotPlay::EMPTY, |d| SlotPlay::from_meta(&d.meta)),
                ),
            }],
            // The app resends the whole rack after any rack edit.
            Edit::InsertMachine { .. }
            | Edit::RemoveMachine { .. }
            | Edit::MoveMachine { .. }
            | Edit::RenameMachine { .. } => Vec::new(),
            Edit::Group(edits) => edits.iter().flat_map(|e| self.commands_for(e)).collect(),
        }
    }
}

/// The command that hands the engine pattern `slot` of `uid` as it is now.
fn pattern_command(p: &Project, uid: MachineUid, slot: u8) -> Command {
    let s = slot as usize;
    let data = match &p.machine(uid).expect(NO_MACHINE).body {
        MachineBody::Synth(m) => PatternData::Synth(m.patterns[s].clone()),
        MachineBody::Kit(m) => PatternData::Drum(m.patterns[s].clone()),
        MachineBody::Fm(m) => PatternData::Notes(m.patterns[s].clone()),
        MachineBody::Mono(m) => PatternData::Notes(m.patterns[s].clone()),
        MachineBody::Sampler(m) => PatternData::Notes(m.patterns[s].clone()),
    };
    Command::Pattern {
        uid,
        index: slot,
        pattern: Box::new(data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_rack::*;
    use acid_engine::machine::{MachineType, MachineUid};
    use acid_engine::params::MachineParam;
    use acid_engine::params::{ParamId, SynthParam};
    use acid_engine::project::Note;
    use acid_engine::steps::{DrumHit, set_drum_hit};

    fn doc() -> Document {
        Document::new(Project::default())
    }

    /// A `NotePattern` edit: pattern `slot` of `uid` as it is now with `note` added.
    fn add_note(d: &Document, uid: MachineUid, slot: u8, note: Note) -> Edit {
        let mut roll = d.project.roll(uid, slot).unwrap().clone();
        roll.notes.push(note);
        Edit::NotePattern {
            uid,
            slot,
            pattern: Box::new(roll),
        }
    }

    /// A `NotePattern` edit: drum pattern `slot` of `uid` with one hit set.
    fn drum_edit(d: &Document, uid: MachineUid, slot: u8, step: usize, inst: usize) -> Edit {
        let mut roll = d.project.roll(uid, slot).unwrap().clone();
        assert!(set_drum_hit(&mut roll, step, inst, DrumHit::Accent));
        Edit::NotePattern {
            uid,
            slot,
            pattern: Box::new(roll),
        }
    }

    const CUTOFF: ParamId = ParamId::Machine(SYNTH1, MachineParam::Synth(SynthParam::Cutoff));

    #[test]
    fn loading_an_fm_voice_is_one_undo_step() {
        use acid_engine::params::{FmParam, OpParam};
        let mut d = doc();
        let alg = FM.param(FmParam::Algorithm);
        let lvl = FM.param(MachineParam::FmOp(2, OpParam::Level));
        let cmds = d.apply(
            Edit::FmVoice {
                uid: FM,
                name: "BRASS".into(),
                values: vec![(alg, 1.0), (lvl, 0.5)],
            },
            false,
        );
        assert_eq!(d.project.fm(FM).unwrap().voice_name, "BRASS");
        assert_eq!(d.project.param(alg), 1.0);
        assert_eq!(cmds.len(), 2);
        assert!(cmds.iter().all(|c| matches!(c, Command::SetParam(..))));
        d.undo();
        assert_eq!(d.project.fm(FM).unwrap().voice_name, "INIT VOICE");
        assert_eq!(d.project.param(alg), alg.info().default);
        assert_eq!(d.project.param(lvl), lvl.info().default);
    }

    #[test]
    fn param_edit_updates_project_and_emits_command() {
        let mut d = doc();
        let cmds = d.apply(
            Edit::Param {
                id: CUTOFF,
                value: 0.9,
            },
            false,
        );
        assert_eq!(d.project.param(CUTOFF), 0.9);
        assert!(
            matches!(cmds.as_slice(), [Command::SetParam(id, v)] if *id == CUTOFF && *v == 0.9)
        );
        assert!(d.is_dirty());
    }

    #[test]
    fn undo_and_redo_round_trip() {
        let mut d = doc();
        let before = d.project.clone();
        d.apply(add_note(&d, SYNTH2, 5, Note::new(8, 4, 51, 100)), false);
        d.apply(drum_edit(&d, KIT808, 0, 4, 1), false);
        let after = d.project.clone();
        d.undo();
        d.undo();
        assert_eq!(d.project, before);
        assert!(!d.can_undo());
        d.redo();
        d.redo();
        assert_eq!(d.project, after);
        assert!(!d.can_redo());
    }

    /// The project's song with one automation point written at `step`.
    fn song_with_point(d: &Document, step: u32, value: f32) -> Edit {
        let mut song = d.project.song.clone();
        acid_engine::song::write_automation(&mut song, CUTOFF, step, step, value);
        Edit::Song {
            song: Box::new(song),
        }
    }

    const WAVE: ParamId = ParamId::Machine(SYNTH1, MachineParam::Synth(SynthParam::Wave));

    #[test]
    fn a_record_pass_is_one_undo_step_across_switch_and_bpm_edits() {
        let mut d = doc();
        let before = d.project.clone();
        let cmds = d.record(song_with_point(&d, 0, 0.5));
        assert!(matches!(cmds.as_slice(), [Command::Song(_)]));
        let cmds = d.record(Edit::Param {
            id: WAVE,
            value: 1.0,
        });
        assert!(matches!(cmds.as_slice(), [Command::SetParam(id, v)] if id == &WAVE && v == &1.0));
        d.record(Edit::Bpm(150.0));
        d.record(song_with_point(&d, 1, 0.75));
        d.record(Edit::Bpm(151.0));
        d.record(Edit::Swing(0.25));
        d.end_pass();
        let after = d.project.clone();
        assert_eq!(after.bpm, 151.0);
        let cmds = d.undo();
        assert_eq!(d.project, before, "one undo step reverts the whole pass");
        assert!(!d.can_undo());
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Command::SetBpm(b) if *b == before.bpm))
        );
        assert!(cmds.iter().any(|c| matches!(c, Command::Song(_))));
        d.redo();
        assert_eq!(d.project, after, "redo brings the whole pass back");
    }

    #[test]
    fn record_pass_steps_keep_dirty_tracking() {
        let mut d = doc();
        d.record(song_with_point(&d, 0, 0.5));
        d.mark_saved();
        d.record(Edit::Param {
            id: WAVE,
            value: 1.0,
        });
        assert!(d.is_dirty(), "an edit merged into the pass after a save");
        d.end_pass();
        d.mark_saved();
        d.undo();
        assert!(d.is_dirty());
        d.redo();
        assert!(!d.is_dirty(), "redo returns to the saved state");
        let same = d.project.bpm;
        d.record(Edit::Bpm(same));
        assert!(
            !d.is_dirty(),
            "a no-op edit leaves the pass and dirty state alone"
        );
        d.record(Edit::Bpm(99.0));
        d.end_pass();
        d.undo();
        assert_eq!(d.project.bpm, same, "a later pass is its own step");
        assert!(!d.is_dirty());
        assert!(d.can_undo());
    }

    #[test]
    fn an_ordinary_edit_closes_the_pass_step() {
        let mut d = doc();
        d.record(song_with_point(&d, 0, 0.5));
        let mid = d.project.song.clone();
        d.apply(
            Edit::NoteLength {
                uid: SYNTH1,
                slot: 0,
                length: 3,
            },
            false,
        );
        d.record(song_with_point(&d, 1, 0.5));
        d.undo();
        assert_eq!(
            d.project.song, mid,
            "the pass splits around the pattern edit"
        );
        d.undo();
        d.undo();
        assert!(d.project.song.automation.is_empty());
        assert!(!d.can_undo());
    }

    #[test]
    fn a_gesture_is_one_undo_step() {
        let mut d = doc();
        let start = d.project.param(CUTOFF);
        for v in [0.4, 0.5, 0.6, 0.7] {
            d.apply(
                Edit::Param {
                    id: CUTOFF,
                    value: v,
                },
                true,
            );
        }
        d.end_gesture();
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: 0.1,
            },
            true,
        );
        d.end_gesture();
        d.undo();
        assert_eq!(d.project.param(CUTOFF), 0.7);
        d.undo();
        assert_eq!(d.project.param(CUTOFF), start);
    }

    #[test]
    fn different_params_never_merge() {
        let mut d = doc();
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: 0.9,
            },
            true,
        );
        d.apply(
            Edit::Param {
                id: SYNTH1.param(SynthParam::Resonance),
                value: 0.9,
            },
            true,
        );
        d.undo();
        assert_eq!(d.project.param(CUTOFF), 0.9);
    }

    #[test]
    fn merging_never_loses_edits_to_other_cells() {
        let mut d = doc();
        let before = d.project.clone();
        d.apply(add_note(&d, SYNTH1, 0, Note::new(8, 4, 37, 100)), true);
        d.apply(add_note(&d, SYNTH1, 0, Note::new(20, 4, 37, 100)), true);
        d.apply(
            Edit::NoteLength {
                uid: SYNTH1,
                slot: 0,
                length: 8,
            },
            true,
        );
        d.end_gesture();
        while d.can_undo() {
            d.undo();
        }
        assert_eq!(d.project, before);
    }

    #[test]
    fn consecutive_merged_pattern_edits_undo_in_one_step() {
        let mut d = doc();
        let before = d.project.synth(SYNTH1).unwrap().patterns[0].clone();
        let mut a = before.clone();
        a.roll.length = 3;
        let mut b = a.clone();
        b.transpose = 5;
        d.apply(
            Edit::SynthPattern {
                uid: SYNTH1,
                slot: 0,
                pattern: Box::new(a),
            },
            true,
        );
        d.apply(
            Edit::SynthPattern {
                uid: SYNTH1,
                slot: 0,
                pattern: Box::new(b),
            },
            true,
        );
        d.end_gesture();
        d.undo();
        assert_eq!(d.project.synth(SYNTH1).unwrap().patterns[0], before);
        assert!(!d.can_undo());
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut d = doc();
        d.apply(Edit::Bpm(140.0), false);
        d.undo();
        assert!(d.can_redo());
        d.apply(Edit::Bpm(150.0), false);
        assert!(!d.can_redo());
    }

    #[test]
    fn length_edits_send_the_whole_pattern() {
        let mut d = doc();
        let cmds = d.apply(
            Edit::NoteLength {
                uid: SYNTH1,
                slot: 3,
                length: 7,
            },
            false,
        );
        match cmds.as_slice() {
            [
                Command::Pattern {
                    uid: MachineUid(1),
                    index: 3,
                    pattern,
                },
            ] => assert!(matches!(&**pattern, PatternData::Synth(p) if p.roll.length == 7)),
            _ => panic!("expected one Pattern command for Synth 1"),
        }
        let cmds = d.apply(
            Edit::NoteLength {
                uid: KIT909,
                slot: 2,
                length: 12,
            },
            false,
        );
        assert!(matches!(
            cmds.as_slice(),
            [Command::Pattern {
                uid: MachineUid(4),
                index: 2,
                pattern,
            }] if matches!(**pattern, PatternData::Drum(_))
        ));
    }

    #[test]
    fn note_edits_reach_acid_and_drum_patterns() {
        use acid_engine::project::{Note, NotePattern};
        let mut d = Document::new(Project::default());
        // A drum pattern's pitch is its instrument (0..8).
        for (uid, pitch) in [(SYNTH1, 40), (KIT808, 3)] {
            let roll = NotePattern {
                notes: vec![Note::new(3, 5, pitch, 100)],
                length: 12,
            };
            let cmds = d.apply(
                Edit::NotePattern {
                    uid,
                    slot: 2,
                    pattern: Box::new(roll.clone()),
                },
                false,
            );
            assert_eq!(d.project.roll(uid, 2), Some(&roll));
            // The engine drops a pattern of the wrong variant silently.
            let [Command::Pattern { pattern, .. }] = &cmds[..] else {
                panic!("{uid:?}: one Pattern command");
            };
            let variant_ok = match **pattern {
                PatternData::Synth(_) => uid == SYNTH1,
                PatternData::Drum(_) => uid == KIT808,
                PatternData::Notes(_) => false,
            };
            assert!(variant_ok, "{uid:?}: wrong pattern variant");
            d.apply(
                Edit::NoteLength {
                    uid,
                    slot: 2,
                    length: 20,
                },
                false,
            );
            assert_eq!(d.project.roll(uid, 2).unwrap().length, 20);
        }
        d.undo();
        assert_eq!(d.project.roll(KIT808, 2).unwrap().length, 12);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "drum instrument")]
    fn a_kit_note_must_be_a_drum_instrument() {
        use acid_engine::project::{Note, NotePattern};
        let mut d = Document::new(Project::default());
        let roll = NotePattern {
            notes: vec![Note::new(0, 4, 40, 100)],
            length: 16,
        };
        d.apply(
            Edit::NotePattern {
                uid: KIT808,
                slot: 0,
                pattern: Box::new(roll),
            },
            false,
        );
    }

    #[test]
    fn note_pattern_edits_undo_and_sync_the_engine() {
        use acid_engine::project::{Note, NotePattern};
        let mut d = doc();
        let mut p = NotePattern::default();
        p.notes.push(Note::new(2, 3, 64, 99));
        let cmds = d.apply(
            Edit::NotePattern {
                uid: SAMPLER,
                slot: 7,
                pattern: Box::new(p.clone()),
            },
            false,
        );
        assert_eq!(d.project.sampler(SAMPLER).unwrap().patterns[7], p);
        assert!(matches!(
            cmds.as_slice(),
            [Command::Pattern { uid: MachineUid(6), index: 7, pattern }]
                if **pattern == PatternData::Notes(p.clone())
        ));
        d.apply(
            Edit::NoteLength {
                uid: SAMPLER,
                slot: 7,
                length: 99,
            },
            false,
        );
        assert_eq!(
            d.project.sampler(SAMPLER).unwrap().patterns[7].length,
            64,
            "clamped"
        );
        d.undo();
        assert_eq!(d.project.sampler(SAMPLER).unwrap().patterns[7].length, 16);
        d.undo();
        assert!(
            d.project.sampler(SAMPLER).unwrap().patterns[7]
                .notes
                .is_empty()
        );
        assert!(d.project.fm(FM).unwrap().patterns[7].notes.is_empty());
    }

    #[test]
    fn values_are_clamped() {
        let mut d = doc();
        d.apply(Edit::Bpm(1000.0), false);
        assert_eq!(d.project.bpm, 300.0);
        d.apply(Edit::Swing(-1.0), false);
        assert_eq!(d.project.swing, 0.0);
        d.apply(
            Edit::NoteLength {
                uid: SYNTH1,
                slot: 0,
                length: 0,
            },
            false,
        );
        assert_eq!(d.project.synth(SYNTH1).unwrap().patterns[0].roll.length, 1);
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: 2.0,
            },
            false,
        );
        assert_eq!(d.project.param(CUTOFF), 1.0);
    }

    #[test]
    fn undo_history_is_bounded() {
        let mut d = doc();
        for i in 0..(UNDO_LIMIT + 50) {
            d.apply(Edit::Bpm(60.0 + i as f32 * 0.1), false);
        }
        let mut n = 0;
        while d.can_undo() {
            d.undo();
            n += 1;
        }
        assert_eq!(n, UNDO_LIMIT);
    }

    /// SYNTH1's lane: pattern 0 at 0 and 16, pattern 3 at 32 (all 16 long).
    fn rippling_doc() -> Document {
        use acid_engine::project::{Block, PatternRef};
        let mut d = doc();
        d.project.song.lane_mut(SYNTH1).extend([
            Block::new(0, PatternRef(0)),
            Block::new(16, PatternRef(0)),
            Block::new(32, PatternRef(3)),
        ]);
        d
    }

    fn starts(d: &Document, uid: MachineUid) -> Vec<u32> {
        d.project.song.lane(uid).iter().map(|b| b.start).collect()
    }

    fn length(uid: MachineUid, slot: u8, length: u8) -> Edit {
        Edit::NoteLength { uid, slot, length }
    }

    #[test]
    fn a_longer_pattern_ripples_its_lane_in_the_same_undo_step() {
        let mut d = rippling_doc();
        let cmds = d.apply(length(SYNTH1, 0, 24), false);
        assert_eq!(starts(&d, SYNTH1), [0, 24, 48]);
        assert!(matches!(
            cmds.as_slice(),
            [Command::Pattern { .. }, Command::Song(s)] if s.lanes[0][2].start == 48
        ));
        assert_eq!(d.undo_depth(), 1);
        let cmds = d.undo();
        assert_eq!(starts(&d, SYNTH1), [0, 16, 32], "undo restores the song");
        assert_eq!(d.project.roll(SYNTH1, 0).unwrap().length, 16);
        assert!(cmds.iter().any(|c| matches!(c, Command::Song(_))));
        d.redo();
        assert_eq!(starts(&d, SYNTH1), [0, 24, 48]);
        assert_eq!(d.project.roll(SYNTH1, 0).unwrap().length, 24);
        d.undo();
        assert_eq!(starts(&d, SYNTH1), [0, 16, 32]);
    }

    #[test]
    fn whole_pattern_edits_ripple_when_their_length_changes() {
        let mut d = rippling_doc();
        let mut pattern = d.project.synth(SYNTH1).unwrap().patterns[0].clone();
        pattern.roll.length = 20;
        d.apply(
            Edit::SynthPattern {
                uid: SYNTH1,
                slot: 0,
                pattern: Box::new(pattern.clone()),
            },
            false,
        );
        assert_eq!(starts(&d, SYNTH1), [0, 20, 40]);
        pattern.roll.length = 8;
        let cmds = d.apply(
            Edit::SynthPattern {
                uid: SYNTH1,
                slot: 0,
                pattern: Box::new(pattern),
            },
            false,
        );
        assert_eq!(starts(&d, SYNTH1), [0, 20, 40], "shrinking leaves gaps");
        assert!(!cmds.iter().any(|c| matches!(c, Command::Song(_))));
        d.undo();
        d.undo();
        assert_eq!(starts(&d, SYNTH1), [0, 16, 32]);
        // Another machine's lane and pattern edits elsewhere are untouched.
        let mut d = rippling_doc();
        d.apply(length(SYNTH2, 0, 32), false);
        assert_eq!(starts(&d, SYNTH1), [0, 16, 32]);
    }

    #[test]
    fn a_length_drag_undoes_its_ripple_in_one_step() {
        use acid_engine::project::{Block, PatternRef};
        let mut d = doc();
        // A gap before the block at 24: the drag's first steps fill it and
        // only later ones move that block (and change the song).
        d.project
            .song
            .lane_mut(SYNTH1)
            .extend([Block::new(0, PatternRef(0)), Block::new(24, PatternRef(3))]);
        for n in 17..=28 {
            d.apply(length(SYNTH1, 0, n), true);
        }
        d.end_gesture();
        assert_eq!(starts(&d, SYNTH1), [0, 28]);
        assert_eq!(d.undo_depth(), 1);
        d.undo();
        assert_eq!(starts(&d, SYNTH1), [0, 24], "the whole drag undoes");
        assert_eq!(d.project.roll(SYNTH1, 0).unwrap().length, 16);
    }

    #[test]
    fn a_ripple_past_the_song_limit_is_refused() {
        use acid_engine::project::{Block, PatternRef};
        let max = acid_engine::song::MAX_SONG_STEPS as u32;
        let mut d = doc();
        d.project.song.lane_mut(SYNTH1).extend([
            Block::new(max - 32, PatternRef(0)),
            Block::new(max - 16, PatternRef(3)),
        ]);
        let before = d.project.clone();
        assert!(d.apply(length(SYNTH1, 0, 24), false).is_empty());
        assert_eq!(d.take_refusal(), Some(SONG_TOO_LONG));
        assert_eq!(d.take_refusal(), None, "once");
        assert_eq!(d.project, before);
        assert_eq!(d.undo_depth(), 0);
        assert!(!d.is_dirty());
        d.apply(length(SYNTH1, 0, 8), false);
        assert_eq!(d.take_refusal(), None, "shrinking always fits");
        assert_eq!(d.project.roll(SYNTH1, 0).unwrap().length, 8);
    }

    #[test]
    fn a_group_with_a_refused_child_changes_nothing() {
        use acid_engine::project::{Block, PatternRef};
        let max = acid_engine::song::MAX_SONG_STEPS as u32;
        let mut d = doc();
        d.project.song.lane_mut(SYNTH1).extend([
            Block::new(max - 32, PatternRef(0)),
            Block::new(max - 16, PatternRef(3)),
        ]);
        let before = d.project.clone();
        let cmds = d.record(Edit::Group(vec![
            Edit::Bpm(150.0),
            length(SYNTH1, 0, 24),
            Edit::Swing(0.3),
        ]));
        assert!(cmds.is_empty());
        assert_eq!(d.take_refusal(), Some(SONG_TOO_LONG));
        assert_eq!(d.project, before, "the BPM edit before it is undone");
        assert_eq!(d.undo_depth(), 0);
        let cmds = d.apply(
            Edit::Group(vec![Edit::Bpm(150.0), length(SYNTH1, 0, 24)]),
            false,
        );
        assert!(cmds.is_empty());
        assert_eq!(d.take_refusal(), Some(SONG_TOO_LONG));
        assert_eq!(d.project, before);
        assert_eq!(d.undo_depth(), 0);
    }

    #[test]
    fn removing_a_machine_takes_its_song_lane_and_undo_restores_it() {
        let mut d = rippling_doc();
        let song = d.project.song.clone();
        d.remove_machine(SYNTH1).unwrap();
        assert!(d.project.song.lane(SYNTH1).is_empty());
        d.undo();
        assert_eq!(d.project.song, song);
        d.redo();
        assert!(d.project.song.lane(SYNTH1).is_empty());
    }

    #[test]
    fn song_edits_merge_into_one_record_pass() {
        let mut d = doc();
        d.project.song = Song::default();
        for n in 1..=5 {
            let mut song = d.project.song.clone();
            song.end = Some(n * 16);
            let cmds = d.apply(
                Edit::Song {
                    song: Box::new(song),
                },
                true,
            );
            assert!(matches!(cmds.as_slice(), [Command::Song(s)] if s.end == Some(n * 16)));
        }
        d.end_gesture();
        d.undo();
        assert_eq!(d.project.song.end, None, "the whole pass undoes at once");
        d.redo();
        assert_eq!(d.project.song.end, Some(80));
    }

    #[test]
    fn select_pattern_marks_dirty_and_commands_engine() {
        let mut d = doc();
        let cmds = d.select_pattern(KIT808, 17);
        assert_eq!(d.project.machine(KIT808).unwrap().selected, 17);
        assert!(matches!(
            cmds.as_slice(),
            [Command::SelectPattern {
                uid: MachineUid(3),
                slot: 17
            }]
        ));
        assert!(d.is_dirty());
    }

    #[test]
    fn sampler_slot_edits_undo_and_sync_the_engine() {
        use acid_engine::params::SlotParam;
        use acid_engine::sampler::slot::{SlotData, SlotMeta};
        let mut d = doc();
        let tone: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.1).sin() * 0.5).collect();
        let data = SlotData::new(
            "Hit",
            22_050,
            1,
            &tone,
            SlotMeta::sliced(24, vec![0, 500], 120.0, 1000),
        );
        let choke = SAMPLER.param(MachineParam::SamplerSlot(2, SlotParam::Choke));
        let cmds = d.apply(
            Edit::SamplerSlot {
                uid: SAMPLER,
                slot: 2,
                data: Some(Box::new(data.clone())),
                values: vec![(choke, 1.0)],
            },
            false,
        );
        assert_eq!(d.project.sampler(SAMPLER).unwrap().slots[2], Some(data));
        assert!(matches!(
            cmds[0],
            Command::SamplerSample {
                uid: MachineUid(6),
                slot: 2,
                sample: Some(_)
            }
        ));
        assert!(matches!(
            cmds[1],
            Command::SamplerPlay {
                uid: MachineUid(6),
                slot: 2,
                ..
            }
        ));
        assert!(matches!(cmds[2], Command::SetParam(id, v) if id == choke && v == 1.0));
        let mut meta = d.project.sampler(SAMPLER).unwrap().slots[2]
            .as_ref()
            .unwrap()
            .meta
            .clone();
        meta.slices = vec![0, 300];
        let cmds = d.apply(
            Edit::SlotMeta {
                uid: SAMPLER,
                slot: 2,
                meta: Box::new(meta),
            },
            false,
        );
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], Command::SamplerPlay { slot: 2, .. }));
        d.undo();
        assert_eq!(
            d.project.sampler(SAMPLER).unwrap().slots[2]
                .as_ref()
                .unwrap()
                .meta
                .slices,
            vec![0, 500]
        );
        d.undo();
        assert_eq!(d.project.sampler(SAMPLER).unwrap().slots[2], None);
        assert_eq!(d.project.param(choke), 0.0);
    }

    #[test]
    fn a_no_op_edit_is_not_an_undo_step_and_leaves_the_document_clean() {
        let mut d = doc();
        let start = d.project.param(CUTOFF);
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: start,
            },
            false,
        );
        assert!(!d.can_undo() && !d.is_dirty());
        // A clamped value equal to the stored one is also no change.
        d.apply(Edit::Bpm(300.0), false);
        d.apply(Edit::Bpm(1000.0), false);
        d.undo();
        assert!(!d.can_undo(), "the clamped repeat added no step");
        let length = d.project.synth(SYNTH1).unwrap().patterns[0].roll.length;
        d.apply(
            Edit::NoteLength {
                uid: SYNTH1,
                slot: 0,
                length,
            },
            false,
        );
        assert!(!d.can_undo() && !d.is_dirty());
    }

    #[test]
    fn a_no_op_never_merges_into_an_unrelated_step() {
        let mut d = doc();
        d.apply(Edit::Bpm(150.0), false);
        let start = d.project.param(CUTOFF);
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: start,
            },
            true,
        );
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: 0.9,
            },
            true,
        );
        d.end_gesture();
        d.undo();
        assert_eq!(d.project.param(CUTOFF), start);
        assert_eq!(d.project.bpm, 150.0, "the bpm step is separate");
    }

    #[test]
    fn a_no_op_edit_leaves_an_open_gesture_open() {
        let mut d = doc();
        let start = d.project.param(CUTOFF);
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: 0.3,
            },
            true,
        );
        // e.g. a redraw's rounding that sets the tempo it already has
        d.apply(Edit::Bpm(d.project.bpm), false);
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: 0.6,
            },
            true,
        );
        d.end_gesture();
        d.undo();
        assert_eq!(d.project.param(CUTOFF), start, "one step for the drag");
        assert!(!d.can_undo());
    }

    #[test]
    fn undoing_back_to_the_saved_state_clears_dirty() {
        let mut d = doc();
        d.apply(Edit::Bpm(150.0), false);
        d.mark_saved();
        assert!(!d.is_dirty());
        d.apply(Edit::Bpm(160.0), false);
        assert!(d.is_dirty());
        d.undo();
        assert!(!d.is_dirty(), "back at the saved state");
        d.undo();
        assert!(d.is_dirty(), "before the saved state");
        d.redo();
        assert!(!d.is_dirty(), "redo back to the saved state");
        d.redo();
        assert!(d.is_dirty());
    }

    #[test]
    fn a_fresh_document_undone_back_to_its_start_is_clean() {
        let mut d = doc();
        d.apply(Edit::Bpm(150.0), false);
        assert!(d.is_dirty());
        d.undo();
        assert!(!d.is_dirty());
    }

    #[test]
    fn a_new_edit_after_undoing_past_the_save_keeps_the_document_dirty() {
        let mut d = doc();
        d.apply(Edit::Bpm(150.0), false);
        d.mark_saved();
        d.undo();
        d.apply(Edit::Swing(0.3), false);
        d.undo();
        assert!(d.is_dirty(), "the saved state is unreachable now");
        d.apply(Edit::Bpm(150.0), false);
        assert!(d.is_dirty(), "same content, but a different history step");
    }

    #[test]
    fn continuing_a_gesture_after_a_save_is_dirty() {
        let mut d = doc();
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: 0.3,
            },
            true,
        );
        d.mark_saved();
        d.apply(
            Edit::Param {
                id: CUTOFF,
                value: 0.4,
            },
            true,
        );
        assert!(d.is_dirty());
    }

    #[test]
    fn selecting_a_pattern_stays_dirty_through_undo() {
        let mut d = doc();
        d.apply(Edit::Bpm(150.0), false);
        d.mark_saved();
        d.select_pattern(SYNTH1, 3);
        assert!(d.is_dirty());
        d.apply(Edit::Bpm(160.0), false);
        d.undo();
        assert!(d.is_dirty(), "the selection change is not undoable");
        d.mark_saved();
        assert!(!d.is_dirty());
    }

    #[test]
    fn the_saved_state_trimmed_off_the_history_is_unreachable() {
        let mut d = doc();
        d.mark_saved();
        for i in 0..(UNDO_LIMIT + 5) {
            d.apply(Edit::Bpm(60.0 + i as f32), false);
        }
        while d.can_undo() {
            d.undo();
        }
        assert!(d.is_dirty(), "the oldest steps were dropped");
    }

    #[test]
    fn whole_pattern_edits_are_clamped() {
        use acid_engine::project::{DrumPattern, NotePattern};
        let mut d = doc();
        let mut p = SynthPattern {
            roll: NotePattern {
                length: 0,
                ..Default::default()
            },
            transpose: 40,
            ..Default::default()
        };
        d.apply(
            Edit::SynthPattern {
                uid: SYNTH1,
                slot: 0,
                pattern: Box::new(p.clone()),
            },
            false,
        );
        assert_eq!(d.project.synth(SYNTH1).unwrap().patterns[0].roll.length, 1);
        assert_eq!(d.project.synth(SYNTH1).unwrap().patterns[0].transpose, 12);
        p.roll.length = 200;
        p.transpose = -40;
        d.apply(
            Edit::SynthPattern {
                uid: SYNTH1,
                slot: 0,
                pattern: Box::new(p),
            },
            false,
        );
        assert_eq!(
            d.project.synth(SYNTH1).unwrap().patterns[0].roll.length,
            STEPS as u8
        );
        assert_eq!(d.project.synth(SYNTH1).unwrap().patterns[0].transpose, -12);
        let drums = DrumPattern {
            roll: NotePattern {
                length: 0,
                ..Default::default()
            },
        };
        d.apply(
            Edit::DrumPattern {
                uid: KIT909,
                slot: 2,
                pattern: Box::new(drums),
            },
            false,
        );
        assert_eq!(d.project.kit(KIT909).unwrap().patterns[2].roll.length, 1);
        let notes = NotePattern {
            length: 99,
            ..Default::default()
        };
        d.apply(
            Edit::NotePattern {
                uid: FM,
                slot: 0,
                pattern: Box::new(notes),
            },
            false,
        );
        assert_eq!(d.project.fm(FM).unwrap().patterns[0].length, STEPS as u8);
    }

    #[test]
    fn repeated_ids_in_a_voice_load_undo_to_the_original_value() {
        use acid_engine::params::FmParam;
        let mut d = doc();
        let alg = FM.param(FmParam::Algorithm);
        d.apply(
            Edit::FmVoice {
                uid: FM,
                name: "TWICE".into(),
                values: vec![(alg, 0.2), (alg, 0.9)],
            },
            false,
        );
        assert_eq!(d.project.param(alg), 0.9);
        d.undo();
        assert_eq!(d.project.param(alg), alg.info().default);
        d.redo();
        assert_eq!(d.project.param(alg), 0.9);
    }

    #[test]
    fn repeated_ids_in_a_slot_load_undo_to_the_original_value() {
        use acid_engine::params::SlotParam;
        let mut d = doc();
        let choke = SAMPLER.param(MachineParam::SamplerSlot(0, SlotParam::Choke));
        d.apply(
            Edit::SamplerSlot {
                uid: SAMPLER,
                slot: 0,
                data: None,
                values: vec![(choke, 0.5), (choke, 1.0)],
            },
            false,
        );
        d.undo();
        assert_eq!(d.project.param(choke), choke.info().default);
    }

    fn slot_load(samples: usize) -> Edit {
        use acid_engine::sampler::slot::SlotMeta;
        let data = SlotData {
            name: "Big".into(),
            meta: SlotMeta::sliced(36, vec![0], 120.0, samples as u32),
            sample_rate: 44_100,
            channels: 1,
            pcm: vec![1; samples],
        };
        Edit::SamplerSlot {
            uid: SAMPLER,
            slot: 0,
            data: Some(Box::new(data)),
            values: Vec::new(),
        }
    }

    #[test]
    fn sample_data_in_the_undo_history_stays_within_the_byte_budget() {
        let mut d = doc();
        // Each load is 1000 bytes of PCM; the budget fits three of them.
        d.set_undo_byte_budget(3_500);
        for _ in 0..10 {
            d.apply(slot_load(500), false);
        }
        assert!(d.undo_sample_bytes() <= 3_500);
        let mut n = 0;
        while d.can_undo() {
            d.undo();
            n += 1;
        }
        assert_eq!(n, 3, "only the newest three loads fit");
        assert!(
            d.project.sampler(SAMPLER).unwrap().slots[0].is_some(),
            "oldest loads dropped"
        );
    }

    #[test]
    fn the_newest_step_is_kept_even_when_over_budget() {
        let mut d = doc();
        d.set_undo_byte_budget(100);
        d.apply(slot_load(500), false);
        d.apply(slot_load(600), false);
        assert!(d.can_undo());
        d.undo();
        assert_eq!(
            d.project.sampler(SAMPLER).unwrap().slots[0]
                .as_ref()
                .unwrap()
                .pcm
                .len(),
            500
        );
    }

    #[test]
    fn rack_edits_undo_and_redo() {
        let mut d = Document::new(acid_engine::demo::demo_project());
        let start = d.project.clone();
        let (uid, _) = d.add_machine(MachineType::Sampler).unwrap();
        d.rename_machine(uid, "Breaks");
        d.move_machine(uid, 0);
        let (copy, _) = d.duplicate_machine(MachineUid(1)).unwrap();
        let (name, _) = d.remove_machine(MachineUid(4)).unwrap();
        assert_eq!(name, "909");
        let end = d.project.clone();
        for _ in 0..5 {
            d.undo();
        }
        assert_eq!(d.project.machines, start.machines);
        assert_eq!(d.project.song, start.song);
        for _ in 0..5 {
            d.redo();
        }
        assert_eq!(d.project, end);
        assert!(d.project.machine(copy).is_some());
        assert_eq!(d.project.machines[0].name, "Breaks");
    }

    #[test]
    fn edits_to_a_machine_survive_its_removal_and_return() {
        let mut d = Document::new(acid_engine::demo::demo_project());
        let id = ParamId::Machine(MachineUid(2), MachineParam::Synth(SynthParam::Cutoff));
        d.apply(Edit::Param { id, value: 0.1 }, false);
        d.remove_machine(MachineUid(2));
        d.undo(); // machine back
        d.undo(); // knob back — must not panic
        assert_eq!(
            d.project.param(id),
            acid_engine::demo::demo_project().param(id)
        );
        d.redo();
        d.redo();
        assert!(d.project.machine(MachineUid(2)).is_none());
        d.undo();
        assert_eq!(d.project.param(id), 0.1);
    }

    #[test]
    fn the_rack_cap_and_no_op_edits() {
        let mut d = Document::new(acid_engine::project::Project::default());
        while d.project.machines.len() < 16 {
            d.add_machine(MachineType::Kit808).unwrap();
        }
        assert!(d.add_machine(MachineType::Fm).is_none());
        assert!(d.duplicate_machine(MachineUid(1)).is_none());
        let depth = d.undo_depth();
        d.move_machine(MachineUid(1), 0);
        d.rename_machine(MachineUid(1), "Synth 1");
        assert_eq!(d.undo_depth(), depth, "no-ops leave no undo step");
    }

    #[test]
    fn a_removed_samplers_audio_counts_against_the_undo_budget() {
        let mut d = doc();
        d.apply(slot_load(500), false);
        d.apply(slot_load(500), false); // the undo step holds the first load's 1000 bytes
        assert_eq!(d.undo_sample_bytes(), 1_000);
        d.remove_machine(SAMPLER);
        assert_eq!(
            d.undo_sample_bytes(),
            2_000,
            "the removed sampler's audio is counted"
        );
        d.set_undo_byte_budget(1_500);
        d.move_machine(SYNTH1, 1); // trims while over budget
        assert!(d.undo_sample_bytes() <= 1_500);
        d.undo();
        d.undo();
        assert!(d.project.sampler(SAMPLER).is_some());
    }
}
