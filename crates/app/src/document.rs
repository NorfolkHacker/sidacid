//! The editable project plus undo/redo. Every change is an `Edit`;
//! applying one returns the engine `Command`s that bring the audio
//! thread in line. Undo stores the inverse edit.

use acid_engine::fm::sysex::Dx7Voice;
use acid_engine::host::Command;
use acid_engine::params::ParamId;
use acid_engine::project::{
    DrumHit, DrumPattern, NotePattern, Project, STEPS, SeqView, Song, SynthPattern, SynthStep,
};
use acid_engine::sampler::slot::{SlotData, SlotMeta, SlotPlay};
use std::path::PathBuf;
use std::sync::Arc;

pub const UNDO_LIMIT: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Param {
        id: ParamId,
        value: f32,
    },
    SynthStep {
        synth: usize,
        slot: u8,
        step: usize,
        value: SynthStep,
    },
    SynthLength {
        synth: usize,
        slot: u8,
        length: u8,
    },
    SynthTranspose {
        synth: usize,
        slot: u8,
        transpose: i8,
    },
    SynthPattern {
        synth: usize,
        slot: u8,
        pattern: Box<SynthPattern>,
    },
    DrumHit {
        kit: usize,
        slot: u8,
        step: usize,
        inst: usize,
        hit: DrumHit,
    },
    DrumLength {
        kit: usize,
        slot: u8,
        length: u8,
    },
    DrumPattern {
        kit: usize,
        slot: u8,
        pattern: Box<DrumPattern>,
    },
    NotePattern {
        machine: usize,
        slot: u8,
        pattern: Box<NotePattern>,
    },
    NoteLength {
        machine: usize,
        slot: u8,
        length: u8,
    },
    Bpm(f32),
    Swing(f32),
    View {
        synth: usize,
        view: SeqView,
    },
    Song {
        song: Box<Song>,
    },
    /// Loading an FM voice: many params and the voice name, one undo step.
    FmVoice {
        name: String,
        values: Vec<(ParamId, f32)>,
    },
    FmBank {
        bank: Option<Vec<Dx7Voice>>,
    },
    /// Loading or clearing a sampler slot, with its knob values: one undo step.
    SamplerSlot {
        slot: usize,
        data: Option<Box<SlotData>>,
        values: Vec<(ParamId, f32)>,
    },
    /// Changing a loaded slot's zone, slices, mode or markers.
    SlotMeta {
        slot: usize,
        meta: Box<SlotMeta>,
    },
}

/// Which edits may fold into one undo step during a gesture.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MergeKey {
    Param(ParamId),
    SynthStep(usize, u8, usize),
    SynthLength(usize, u8),
    SynthTranspose(usize, u8),
    SynthPattern(usize, u8),
    DrumHit(usize, u8, usize, usize),
    DrumLength(usize, u8),
    DrumPattern(usize, u8),
    NotePattern(usize, u8),
    NoteLength(usize, u8),
    Bpm,
    Swing,
    View(usize),
    Song,
    FmVoice,
    FmBank,
    SamplerSlot(usize),
    SlotMeta(usize),
}

fn merge_key(edit: &Edit) -> MergeKey {
    match edit {
        Edit::Param { id, .. } => MergeKey::Param(*id),
        Edit::SynthStep {
            synth, slot, step, ..
        } => MergeKey::SynthStep(*synth, *slot, *step),
        Edit::SynthLength { synth, slot, .. } => MergeKey::SynthLength(*synth, *slot),
        Edit::SynthTranspose { synth, slot, .. } => MergeKey::SynthTranspose(*synth, *slot),
        Edit::SynthPattern { synth, slot, .. } => MergeKey::SynthPattern(*synth, *slot),
        Edit::DrumHit {
            kit,
            slot,
            step,
            inst,
            ..
        } => MergeKey::DrumHit(*kit, *slot, *step, *inst),
        Edit::DrumLength { kit, slot, .. } => MergeKey::DrumLength(*kit, *slot),
        Edit::DrumPattern { kit, slot, .. } => MergeKey::DrumPattern(*kit, *slot),
        Edit::NotePattern { machine, slot, .. } => MergeKey::NotePattern(*machine, *slot),
        Edit::NoteLength { machine, slot, .. } => MergeKey::NoteLength(*machine, *slot),
        Edit::Bpm(_) => MergeKey::Bpm,
        Edit::Swing(_) => MergeKey::Swing,
        Edit::View { synth, .. } => MergeKey::View(*synth),
        Edit::Song { .. } => MergeKey::Song,
        Edit::FmVoice { .. } => MergeKey::FmVoice,
        Edit::FmBank { .. } => MergeKey::FmBank,
        Edit::SamplerSlot { slot, .. } => MergeKey::SamplerSlot(*slot),
        Edit::SlotMeta { slot, .. } => MergeKey::SlotMeta(*slot),
    }
}

pub struct Document {
    pub project: Project,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Merge key of the open gesture, if the last edit was a merging one.
    gesture: Option<MergeKey>,
}

impl Document {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            path: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
        }
    }

    /// Applies `edit` and returns the engine commands to send. With
    /// `merge`, consecutive edits to the same target form one undo step
    /// until `end_gesture`.
    pub fn apply(&mut self, edit: Edit, merge: bool) -> Vec<Command> {
        let key = merge_key(&edit);
        let continuing = merge && self.gesture == Some(key) && !self.undo.is_empty();
        let inverse = self.apply_raw(edit);
        let cmds = self.commands_for(&inverse);
        if !continuing {
            self.undo.push(inverse);
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.gesture = merge.then_some(key);
        self.redo.clear();
        self.dirty = true;
        cmds
    }

    pub fn end_gesture(&mut self) {
        self.gesture = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> Vec<Command> {
        self.gesture = None;
        let Some(edit) = self.undo.pop() else {
            return Vec::new();
        };
        let inverse = self.apply_raw(edit);
        let cmds = self.commands_for(&inverse);
        self.redo.push(inverse);
        self.dirty = true;
        cmds
    }

    pub fn redo(&mut self) -> Vec<Command> {
        self.gesture = None;
        let Some(edit) = self.redo.pop() else {
            return Vec::new();
        };
        let inverse = self.apply_raw(edit);
        let cmds = self.commands_for(&inverse);
        self.undo.push(inverse);
        self.dirty = true;
        cmds
    }

    /// Selecting a pattern is a transport action: saved, not undoable.
    pub fn select_pattern(&mut self, machine: usize, slot: u8) -> Vec<Command> {
        self.project.selected[machine] = slot;
        self.dirty = true;
        vec![Command::SelectPattern {
            machine: machine as u8,
            slot,
        }]
    }

    /// Applies an edit and returns its inverse.
    fn apply_raw(&mut self, edit: Edit) -> Edit {
        let p = &mut self.project;
        match edit {
            Edit::Param { id, value } => {
                let old = p.param(id);
                p.params.insert(id, value.clamp(0.0, 1.0));
                Edit::Param { id, value: old }
            }
            Edit::SynthStep {
                synth,
                slot,
                step,
                value,
            } => {
                let old = std::mem::replace(
                    &mut p.synths[synth].patterns[slot as usize].steps[step],
                    value,
                );
                Edit::SynthStep {
                    synth,
                    slot,
                    step,
                    value: old,
                }
            }
            Edit::SynthLength {
                synth,
                slot,
                length,
            } => {
                let pat = &mut p.synths[synth].patterns[slot as usize];
                let old = std::mem::replace(&mut pat.length, length.clamp(1, STEPS as u8));
                Edit::SynthLength {
                    synth,
                    slot,
                    length: old,
                }
            }
            Edit::SynthTranspose {
                synth,
                slot,
                transpose,
            } => {
                let pat = &mut p.synths[synth].patterns[slot as usize];
                let old = std::mem::replace(&mut pat.transpose, transpose.clamp(-12, 12));
                Edit::SynthTranspose {
                    synth,
                    slot,
                    transpose: old,
                }
            }
            Edit::SynthPattern {
                synth,
                slot,
                pattern,
            } => {
                let old = std::mem::replace(&mut p.synths[synth].patterns[slot as usize], *pattern);
                Edit::SynthPattern {
                    synth,
                    slot,
                    pattern: Box::new(old),
                }
            }
            Edit::DrumHit {
                kit,
                slot,
                step,
                inst,
                hit,
            } => {
                let old = std::mem::replace(
                    &mut p.drums[kit].patterns[slot as usize].steps[step][inst],
                    hit,
                );
                Edit::DrumHit {
                    kit,
                    slot,
                    step,
                    inst,
                    hit: old,
                }
            }
            Edit::DrumLength { kit, slot, length } => {
                let pat = &mut p.drums[kit].patterns[slot as usize];
                let old = std::mem::replace(&mut pat.length, length.clamp(1, STEPS as u8));
                Edit::DrumLength {
                    kit,
                    slot,
                    length: old,
                }
            }
            Edit::DrumPattern { kit, slot, pattern } => {
                let old = std::mem::replace(&mut p.drums[kit].patterns[slot as usize], *pattern);
                Edit::DrumPattern {
                    kit,
                    slot,
                    pattern: Box::new(old),
                }
            }
            Edit::NotePattern {
                machine,
                slot,
                pattern,
            } => {
                let pats = p.note_patterns_mut(machine).expect("note machine");
                let old = std::mem::replace(&mut pats[slot as usize], *pattern);
                Edit::NotePattern {
                    machine,
                    slot,
                    pattern: Box::new(old),
                }
            }
            Edit::NoteLength {
                machine,
                slot,
                length,
            } => {
                let pat = &mut p.note_patterns_mut(machine).expect("note machine")[slot as usize];
                let old = std::mem::replace(&mut pat.length, length.clamp(1, STEPS as u8));
                Edit::NoteLength {
                    machine,
                    slot,
                    length: old,
                }
            }
            Edit::Bpm(bpm) => Edit::Bpm(std::mem::replace(&mut p.bpm, bpm.clamp(40.0, 300.0))),
            Edit::Swing(swing) => {
                Edit::Swing(std::mem::replace(&mut p.swing, swing.clamp(0.0, 0.75)))
            }
            Edit::View { synth, view } => Edit::View {
                synth,
                view: std::mem::replace(&mut p.synths[synth].view, view),
            },
            Edit::Song { song } => Edit::Song {
                song: Box::new(std::mem::replace(&mut p.song, *song)),
            },
            Edit::FmVoice { name, values } => {
                let old_name = std::mem::replace(&mut p.fm.voice_name, name);
                let old_values = values
                    .into_iter()
                    .map(|(id, v)| {
                        let old = p.param(id);
                        p.params.insert(id, v.clamp(0.0, 1.0));
                        (id, old)
                    })
                    .collect();
                Edit::FmVoice {
                    name: old_name,
                    values: old_values,
                }
            }
            Edit::SamplerSlot { slot, data, values } => {
                let old = std::mem::replace(&mut p.sampler.slots[slot], data.map(|d| *d));
                let old_values = values
                    .into_iter()
                    .map(|(id, v)| {
                        let before = p.param(id);
                        p.params.insert(id, v.clamp(0.0, 1.0));
                        (id, before)
                    })
                    .collect();
                Edit::SamplerSlot {
                    slot,
                    data: old.map(Box::new),
                    values: old_values,
                }
            }
            Edit::SlotMeta { slot, meta } => match p.sampler.slots[slot].as_mut() {
                Some(d) => Edit::SlotMeta {
                    slot,
                    meta: Box::new(std::mem::replace(&mut d.meta, *meta)),
                },
                None => Edit::SlotMeta { slot, meta },
            },
            Edit::FmBank { bank } => Edit::FmBank {
                bank: std::mem::replace(&mut p.fm.bank, bank),
            },
        }
    }

    /// Commands that sync the engine with the project at `edit`'s target.
    fn commands_for(&self, edit: &Edit) -> Vec<Command> {
        let p = &self.project;
        match edit {
            Edit::Param { id, .. } => vec![Command::SetParam(*id, p.param(*id))],
            Edit::SynthStep { synth, slot, .. }
            | Edit::SynthLength { synth, slot, .. }
            | Edit::SynthTranspose { synth, slot, .. }
            | Edit::SynthPattern { synth, slot, .. } => vec![Command::SynthPattern {
                synth: *synth as u8,
                slot: *slot,
                pattern: Box::new(p.synths[*synth].patterns[*slot as usize].clone()),
            }],
            Edit::DrumHit { kit, slot, .. }
            | Edit::DrumLength { kit, slot, .. }
            | Edit::DrumPattern { kit, slot, .. } => {
                vec![Command::DrumPattern {
                    kit: *kit as u8,
                    slot: *slot,
                    pattern: Box::new(p.drums[*kit].patterns[*slot as usize].clone()),
                }]
            }
            Edit::NotePattern { machine, slot, .. } | Edit::NoteLength { machine, slot, .. } => {
                vec![Command::NotePattern {
                    machine: *machine as u8,
                    slot: *slot,
                    pattern: Box::new(
                        p.note_patterns(*machine).expect("note machine")[*slot as usize].clone(),
                    ),
                }]
            }
            Edit::Bpm(_) => vec![Command::SetBpm(p.bpm)],
            Edit::Swing(_) => vec![Command::SetSwing(p.swing)],
            Edit::View { .. } => Vec::new(),
            Edit::Song { .. } => vec![Command::Song(Box::new(p.song.clone()))],
            Edit::FmVoice { values, .. } => values
                .iter()
                .map(|(id, _)| Command::SetParam(*id, p.param(*id)))
                .collect(),
            Edit::FmBank { .. } => Vec::new(),
            Edit::SamplerSlot { slot, values, .. } => {
                let data = p.sampler.slots[*slot].as_ref();
                let mut cmds = vec![
                    Command::SamplerSample {
                        slot: *slot as u8,
                        sample: data.map(|d| Arc::new(d.to_sample())),
                    },
                    Command::SamplerPlay {
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
            Edit::SlotMeta { slot, .. } => vec![Command::SamplerPlay {
                slot: *slot as u8,
                play: Box::new(
                    p.sampler.slots[*slot]
                        .as_ref()
                        .map_or(SlotPlay::EMPTY, |d| SlotPlay::from_meta(&d.meta)),
                ),
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::params::{ParamId, SynthParam};
    use acid_engine::project::{DrumHit, Gate, SeqView, SynthStep};

    fn doc() -> Document {
        Document::new(Project::default())
    }

    const CUTOFF: ParamId = ParamId::Synth(0, SynthParam::Cutoff);

    #[test]
    fn loading_an_fm_voice_is_one_undo_step() {
        use acid_engine::params::{FmParam, OpParam};
        let mut d = doc();
        let alg = ParamId::Fm(FmParam::Algorithm);
        let lvl = ParamId::FmOp(2, OpParam::Level);
        let cmds = d.apply(
            Edit::FmVoice {
                name: "BRASS".into(),
                values: vec![(alg, 1.0), (lvl, 0.5)],
            },
            false,
        );
        assert_eq!(d.project.fm.voice_name, "BRASS");
        assert_eq!(d.project.param(alg), 1.0);
        assert_eq!(cmds.len(), 2);
        assert!(cmds.iter().all(|c| matches!(c, Command::SetParam(..))));
        d.undo();
        assert_eq!(d.project.fm.voice_name, "INIT VOICE");
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
        assert!(d.dirty);
    }

    #[test]
    fn undo_and_redo_round_trip() {
        let mut d = doc();
        let before = d.project.clone();
        let step = SynthStep {
            note: 3,
            octave: 1,
            gate: Gate::On,
            accent: true,
            slide: false,
        };
        d.apply(
            Edit::SynthStep {
                synth: 1,
                slot: 5,
                step: 2,
                value: step,
            },
            false,
        );
        d.apply(
            Edit::DrumHit {
                kit: 0,
                slot: 0,
                step: 4,
                inst: 1,
                hit: DrumHit::Accent,
            },
            false,
        );
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
                id: ParamId::Synth(0, SynthParam::Resonance),
                value: 0.9,
            },
            true,
        );
        d.undo();
        assert_eq!(d.project.param(CUTOFF), 0.9);
    }

    #[test]
    fn merging_never_loses_edits_to_other_cells() {
        use acid_engine::project::{Gate, SynthStep};
        let mut d = doc();
        let before = d.project.clone();
        let on = SynthStep {
            note: 1,
            octave: 0,
            gate: Gate::On,
            accent: false,
            slide: false,
        };
        d.apply(
            Edit::SynthStep {
                synth: 0,
                slot: 0,
                step: 2,
                value: on,
            },
            true,
        );
        d.apply(
            Edit::SynthStep {
                synth: 0,
                slot: 0,
                step: 5,
                value: on,
            },
            true,
        );
        d.apply(
            Edit::SynthLength {
                synth: 0,
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
        let before = d.project.synths[0].patterns[0].clone();
        let mut a = before.clone();
        a.length = 3;
        let mut b = a.clone();
        b.transpose = 5;
        d.apply(
            Edit::SynthPattern {
                synth: 0,
                slot: 0,
                pattern: Box::new(a),
            },
            true,
        );
        d.apply(
            Edit::SynthPattern {
                synth: 0,
                slot: 0,
                pattern: Box::new(b),
            },
            true,
        );
        d.end_gesture();
        d.undo();
        assert_eq!(d.project.synths[0].patterns[0], before);
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
    fn step_edits_send_the_whole_pattern() {
        let mut d = doc();
        let cmds = d.apply(
            Edit::SynthLength {
                synth: 0,
                slot: 3,
                length: 7,
            },
            false,
        );
        match cmds.as_slice() {
            [
                Command::SynthPattern {
                    synth: 0,
                    slot: 3,
                    pattern,
                },
            ] => assert_eq!(pattern.length, 7),
            _ => panic!("expected one SynthPattern command"),
        }
        let cmds = d.apply(
            Edit::DrumLength {
                kit: 1,
                slot: 2,
                length: 12,
            },
            false,
        );
        assert!(matches!(
            cmds.as_slice(),
            [Command::DrumPattern {
                kit: 1,
                slot: 2,
                ..
            }]
        ));
    }

    #[test]
    fn note_pattern_edits_undo_and_sync_the_engine() {
        use acid_engine::project::{Note, NotePattern};
        let mut d = doc();
        let mut p = NotePattern::default();
        p.notes.push(Note {
            start: 2,
            len: 3,
            pitch: 64,
            vel: 99,
        });
        let cmds = d.apply(
            Edit::NotePattern {
                machine: 5,
                slot: 7,
                pattern: Box::new(p.clone()),
            },
            false,
        );
        assert_eq!(d.project.sampler.patterns[7], p);
        assert!(matches!(
            cmds.as_slice(),
            [Command::NotePattern { machine: 5, slot: 7, pattern }] if **pattern == p
        ));
        d.apply(
            Edit::NoteLength {
                machine: 5,
                slot: 7,
                length: 99,
            },
            false,
        );
        assert_eq!(d.project.sampler.patterns[7].length, 64, "clamped");
        d.undo();
        assert_eq!(d.project.sampler.patterns[7].length, 16);
        d.undo();
        assert!(d.project.sampler.patterns[7].notes.is_empty());
        assert!(d.project.fm.patterns[7].notes.is_empty());
    }

    #[test]
    fn values_are_clamped() {
        let mut d = doc();
        d.apply(Edit::Bpm(1000.0), false);
        assert_eq!(d.project.bpm, 300.0);
        d.apply(Edit::Swing(-1.0), false);
        assert_eq!(d.project.swing, 0.0);
        d.apply(
            Edit::SynthLength {
                synth: 0,
                slot: 0,
                length: 0,
            },
            false,
        );
        assert_eq!(d.project.synths[0].patterns[0].length, 1);
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
    fn view_edit_is_undoable_and_silent() {
        let mut d = doc();
        let cmds = d.apply(
            Edit::View {
                synth: 1,
                view: SeqView::PianoRoll,
            },
            false,
        );
        assert!(cmds.is_empty());
        assert_eq!(d.project.synths[1].view, SeqView::PianoRoll);
        d.undo();
        assert_eq!(d.project.synths[1].view, SeqView::Step303);
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

    #[test]
    fn song_edits_merge_into_one_record_pass() {
        use acid_engine::project::Bar;
        let mut d = doc();
        for n in 1..=5 {
            let mut song = d.project.song.clone();
            song.bars = vec![Bar::default(); n];
            let cmds = d.apply(
                Edit::Song {
                    song: Box::new(song),
                },
                true,
            );
            assert!(matches!(cmds.as_slice(), [Command::Song(s)] if s.bars.len() == n));
        }
        d.end_gesture();
        d.undo();
        assert!(
            d.project.song.bars.is_empty(),
            "the whole pass undoes at once"
        );
        d.redo();
        assert_eq!(d.project.song.bars.len(), 5);
    }

    #[test]
    fn select_pattern_marks_dirty_and_commands_engine() {
        let mut d = doc();
        let cmds = d.select_pattern(2, 17);
        assert_eq!(d.project.selected[2], 17);
        assert!(matches!(
            cmds.as_slice(),
            [Command::SelectPattern {
                machine: 2,
                slot: 17
            }]
        ));
        assert!(d.dirty);
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
        let choke = ParamId::SamplerSlot(2, SlotParam::Choke);
        let cmds = d.apply(
            Edit::SamplerSlot {
                slot: 2,
                data: Some(Box::new(data.clone())),
                values: vec![(choke, 1.0)],
            },
            false,
        );
        assert_eq!(d.project.sampler.slots[2], Some(data));
        assert!(matches!(
            cmds[0],
            Command::SamplerSample {
                slot: 2,
                sample: Some(_)
            }
        ));
        assert!(matches!(cmds[1], Command::SamplerPlay { slot: 2, .. }));
        assert!(matches!(cmds[2], Command::SetParam(id, v) if id == choke && v == 1.0));
        let mut meta = d.project.sampler.slots[2].as_ref().unwrap().meta.clone();
        meta.slices = vec![0, 300];
        let cmds = d.apply(
            Edit::SlotMeta {
                slot: 2,
                meta: Box::new(meta),
            },
            false,
        );
        assert_eq!(cmds.len(), 1);
        assert!(matches!(cmds[0], Command::SamplerPlay { slot: 2, .. }));
        d.undo();
        assert_eq!(
            d.project.sampler.slots[2].as_ref().unwrap().meta.slices,
            vec![0, 500]
        );
        d.undo();
        assert_eq!(d.project.sampler.slots[2], None);
        assert_eq!(d.project.param(choke), 0.0);
    }
}
