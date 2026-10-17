//! Rack edits on a [`Project`]: add, duplicate, remove/restore, move, rename.
//! Pure data; re-exported from `project`.

use crate::machine::{MACHINE_COLOURS, MachineType, MachineUid, default_name};
use crate::params::ParamId;
use crate::project::{AutoLane, Block, MachineBody, MachineEntry, ParamLane, Project};

/// Everything removing a machine takes out of the project, so undo can put it back exactly.
#[derive(Clone, Debug, PartialEq)]
pub struct RemovedMachine {
    /// Its rack position.
    pub index: usize,
    pub entry: MachineEntry,
    /// (index in `song.lanes`, its blocks).
    pub song_lane: Option<(usize, Vec<Block>)>,
    /// (index in `song.automation`, lane).
    pub lanes: Vec<(usize, AutoLane)>,
    /// (synth uid, pattern index, lane) that targeted it.
    pub param_lanes: Vec<(MachineUid, u8, ParamLane)>,
}

impl Project {
    /// The palette colour fewest machines in the rack have; the first such
    /// in palette order on a tie. What a new machine or a copy takes.
    pub fn least_used_colour(&self) -> u8 {
        let mut used = [0usize; MACHINE_COLOURS];
        for m in &self.machines {
            if let Some(n) = used.get_mut(m.colour as usize) {
                *n += 1;
            }
        }
        // `min_by_key` keeps the first of equal counts.
        (0..MACHINE_COLOURS).min_by_key(|&c| used[c]).unwrap_or(0) as u8
    }

    /// A fresh machine of `ty` with the next uid, a default name and the least-used colour;
    /// does not insert it. None once every uid has been used.
    pub fn new_machine(&mut self, ty: MachineType) -> Option<MachineEntry> {
        let names: Vec<&str> = self.machines.iter().map(|m| m.name.as_str()).collect();
        let name = default_name(ty, &names);
        let colour = self.least_used_colour();
        Some(MachineEntry {
            colour,
            ..MachineEntry::new(self.take_uid()?, ty, name)
        })
    }

    /// Deep copy of `uid` with a new uid, "<name> copy" and the least-used colour; synth
    /// param lanes on the source itself are retargeted to the copy. Does not insert it. None
    /// if absent or the uids have run out.
    pub fn duplicate_machine(&mut self, uid: MachineUid) -> Option<MachineEntry> {
        let mut copy = self.machine(uid)?.clone();
        copy.uid = self.take_uid()?;
        copy.name = format!("{} copy", copy.name);
        copy.colour = self.least_used_colour();
        // The copy's own lanes drive the copy, not the source; other targets stay.
        if let MachineBody::Synth(s) = &mut copy.body {
            for lane in s.patterns.iter_mut().filter_map(|p| p.param_lane.as_mut()) {
                if let ParamId::Machine(u, param) = lane.param
                    && u == uid
                {
                    lane.param = ParamId::Machine(copy.uid, param);
                }
            }
        }
        Some(copy)
    }

    /// Inserts at `index` (clamped to len); None if the uid is taken.
    pub fn insert_machine(&mut self, index: usize, entry: MachineEntry) -> Option<()> {
        if self.machine(entry.uid).is_some() {
            return None;
        }
        // Keep `next_uid` above every uid in the rack, whatever the entry's origin.
        self.next_uid = self.next_uid.max(entry.uid.0.saturating_add(1));
        let index = index.min(self.machines.len());
        self.machines.insert(index, entry);
        Some(())
    }

    /// Takes `uid` out of the rack with its song lane, its song automation lanes and
    /// every synth pattern param lane that targets it.
    pub fn remove_machine(&mut self, uid: MachineUid) -> Option<RemovedMachine> {
        let index = self.position(uid)?;
        let entry = self.machines.remove(index);
        let song_lane = self
            .song
            .lanes
            .iter()
            .position(|(u, _)| *u == uid)
            .map(|i| (i, self.song.lanes.remove(i).1));
        let mut lanes = Vec::new();
        let mut kept = Vec::with_capacity(self.song.automation.len());
        for (i, lane) in self.song.automation.drain(..).enumerate() {
            if targets(lane.param, uid) {
                lanes.push((i, lane));
            } else {
                kept.push(lane);
            }
        }
        self.song.automation = kept;
        let mut param_lanes = Vec::new();
        for m in &mut self.machines {
            if let MachineBody::Synth(s) = &mut m.body {
                for (j, pat) in s.patterns.iter_mut().enumerate() {
                    if pat
                        .param_lane
                        .as_ref()
                        .is_some_and(|l| targets(l.param, uid))
                        && let Some(lane) = pat.param_lane.take()
                    {
                        param_lanes.push((m.uid, j as u8, lane));
                    }
                }
            }
        }
        Some(RemovedMachine {
            index,
            entry,
            song_lane,
            lanes,
            param_lanes,
        })
    }

    /// Puts a removed machine back exactly (entry at its index, song lane and automation lanes at their indices, param lanes).
    pub fn restore_machine(&mut self, removed: RemovedMachine) {
        let RemovedMachine {
            index,
            entry,
            song_lane,
            lanes,
            param_lanes,
        } = removed;
        let uid = entry.uid;
        if self.insert_machine(index, entry).is_none() {
            return;
        }
        if let Some((i, blocks)) = song_lane
            && !self.song.lanes.iter().any(|(u, _)| *u == uid)
        {
            let i = i.min(self.song.lanes.len());
            self.song.lanes.insert(i, (uid, blocks));
        }
        // Ascending indices: each lane lands where it was once its predecessors are back.
        for (i, lane) in lanes {
            let i = i.min(self.song.automation.len());
            self.song.automation.insert(i, lane);
        }
        for (synth, j, lane) in param_lanes {
            if let Some(pat) = self
                .synth_mut(synth)
                .and_then(|s| s.patterns.get_mut(j as usize))
            {
                pat.param_lane = Some(lane);
            }
        }
    }

    /// Moves to `to` (clamped); returns the old index.
    pub fn move_machine(&mut self, uid: MachineUid, to: usize) -> Option<usize> {
        let from = self.position(uid)?;
        let entry = self.machines.remove(from);
        let to = to.min(self.machines.len());
        self.machines.insert(to, entry);
        Some(from)
    }

    /// Sets the name; an empty or all-whitespace name becomes the type's default name. Returns the old name.
    pub fn rename_machine(&mut self, uid: MachineUid, name: &str) -> Option<String> {
        let ty = self.machine(uid)?.machine_type();
        let name = if name.trim().is_empty() {
            let others: Vec<&str> = self
                .machines
                .iter()
                .filter(|m| m.uid != uid)
                .map(|m| m.name.as_str())
                .collect();
            default_name(ty, &others)
        } else {
            name.to_string()
        };
        let m = self.machine_mut(uid)?;
        Some(std::mem::replace(&mut m.name, name))
    }

    /// The next uid, or None once every u16 uid has been used; uids are
    /// never reused. `u16::MAX` itself is never handed out, so `next_uid`
    /// stays above every uid in the rack.
    fn take_uid(&mut self) -> Option<MachineUid> {
        let uid = MachineUid(self.next_uid);
        self.next_uid = self.next_uid.checked_add(1)?;
        Some(uid)
    }
}

/// Whether `param` belongs to machine `uid`.
fn targets(param: ParamId, uid: MachineUid) -> bool {
    matches!(param, ParamId::Machine(u, _) if u == uid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{KitParam, MachineParam, SynthParam};
    use crate::project::PatternRef;

    #[test]
    fn new_machines_take_fresh_uids_and_default_names() {
        let mut p = Project::default();
        let a = p.new_machine(MachineType::Kit909).unwrap();
        assert_eq!((a.uid, a.name.as_str()), (MachineUid(7), "Kit B 2"));
        p.insert_machine(p.machines.len(), a).unwrap();
        let b = p.new_machine(MachineType::Synth).unwrap();
        assert_eq!((b.uid, b.name.as_str()), (MachineUid(8), "Synth 3"));
        assert_eq!(p.next_uid, 9);
        p.validate().unwrap();
    }

    fn colours(p: &Project) -> Vec<u8> {
        p.machines.iter().map(|m| m.colour).collect()
    }

    #[test]
    fn a_machine_keeps_its_colour_when_moved() {
        let mut p = Project::default();
        p.machines[5].colour = 3;
        p.move_machine(MachineUid(6), 1).unwrap();
        assert_eq!(p.machine(MachineUid(6)).unwrap().colour, 3);
        assert_eq!(colours(&p), [0, 3, 1, 2, 3, 4], "the others keep theirs");
    }

    #[test]
    fn new_machines_and_copies_take_the_least_used_colour() {
        let mut p = Project::default();
        assert_eq!(colours(&p), [0, 1, 2, 3, 4, 0]);
        assert_eq!(p.least_used_colour(), 1, "a tie goes to palette order");
        for (set, want) in [
            ([0, 0, 1, 1, 2, 3], 4),
            ([2, 2, 2, 2, 2, 2], 0),
            ([4, 3, 0, 1, 0, 1], 2),
            ([1, 1, 2, 2, 3, 4], 0),
        ] {
            for (m, c) in p.machines.iter_mut().zip(set) {
                m.colour = c;
            }
            assert_eq!(p.least_used_colour(), want, "{set:?}");
            assert_eq!(p.new_machine(MachineType::Fm).unwrap().colour, want);
            assert_eq!(p.duplicate_machine(MachineUid(1)).unwrap().colour, want);
        }
        // A copy beside its source differs, unless every colour ties.
        let mut p = Project::default();
        assert_eq!(p.duplicate_machine(MachineUid(2)).unwrap().colour, 1);
        p.remove_machine(MachineUid(6)).unwrap();
        assert_eq!(p.duplicate_machine(MachineUid(1)).unwrap().colour, 0);
        p.machines.clear();
        assert_eq!(p.least_used_colour(), 0, "an empty rack");
    }

    #[test]
    fn the_rack_refuses_a_taken_uid() {
        let mut q = Project::default();
        let dup = q.machines[0].clone();
        assert!(q.insert_machine(0, dup).is_none());
    }

    #[test]
    fn removing_a_machine_takes_its_song_lane_lanes_and_foreign_param_lanes_and_restore_undoes_it()
    {
        let mut p = crate::demo::demo_project();
        // Synth 1's pattern 0 has a param lane on the 909's Level; the song names the 909; automation too.
        let target = ParamId::Machine(MachineUid(4), MachineParam::Kit(KitParam::Volume));
        p.synth_mut(MachineUid(1)).unwrap().patterns[0].param_lane = Some(ParamLane {
            param: target,
            values: vec![Some(0.5); crate::project::STEPS],
        });
        p.song
            .lane_mut(MachineUid(4))
            .push(Block::new(0, PatternRef(1)));
        p.song
            .lane_mut(MachineUid(1))
            .push(Block::new(0, PatternRef(0)));
        p.song.automation.push(AutoLane {
            param: target,
            points: [(0u32, 0.3f32)].into_iter().collect(),
        });
        p.validate().unwrap();
        let before = p.clone();
        let removed = p.remove_machine(MachineUid(4)).unwrap();
        assert_eq!(removed.index, 3);
        assert!(p.machine(MachineUid(4)).is_none());
        assert_eq!(p.song.lanes.len(), 1);
        assert!(p.song.lane(MachineUid(4)).is_empty());
        assert!(p.song.automation.is_empty());
        assert!(
            p.synth(MachineUid(1)).unwrap().patterns[0]
                .param_lane
                .is_none()
        );
        p.validate().unwrap();
        p.restore_machine(removed);
        assert_eq!(p, before);
    }

    #[test]
    fn duplicates_copy_sound_and_patterns_but_not_song_lanes() {
        let mut p = crate::demo::demo_project();
        let d = p.duplicate_machine(MachineUid(1)).unwrap();
        assert_eq!(d.name, "Synth 1 copy");
        assert_ne!(d.uid, MachineUid(1));
        assert_eq!(d.body, p.machine(MachineUid(1)).unwrap().body);
        assert_eq!(d.params, p.machine(MachineUid(1)).unwrap().params);
        p.insert_machine(1, d.clone()).unwrap();
        assert!(p.song.lanes.iter().all(|(u, _)| *u != d.uid));
        p.validate().unwrap();
    }

    #[test]
    fn moving_and_renaming() {
        let mut p = Project::default();
        assert_eq!(p.move_machine(MachineUid(1), 99), Some(0));
        assert_eq!(p.rack_uids().last(), Some(&MachineUid(1)));
        assert_eq!(
            p.rename_machine(MachineUid(3), "Boom"),
            Some("Kit A".to_string())
        );
        assert_eq!(p.machine(MachineUid(3)).unwrap().name, "Boom");
        p.rename_machine(MachineUid(3), "  ");
        assert_eq!(p.machine(MachineUid(3)).unwrap().name, "Kit A");
    }

    #[test]
    fn restore_puts_interleaved_song_lanes_and_automation_lanes_back_in_their_places() {
        let mut p = Project::default();
        let lane = |u: u16| AutoLane {
            param: ParamId::Machine(MachineUid(u), MachineParam::Kit(KitParam::Accent)),
            points: [(0u32, 0.5f32)].into_iter().collect(),
        };
        p.song.automation = vec![lane(3), lane(4), lane(3), lane(4)];
        p.song.automation[2].param =
            ParamId::Machine(MachineUid(3), MachineParam::Kit(KitParam::Volume));
        p.song.automation[3].param =
            ParamId::Machine(MachineUid(4), MachineParam::Kit(KitParam::Volume));
        for (start, u) in [3u16, 4, 1].into_iter().enumerate() {
            p.song.lane_mut(MachineUid(u)).push(Block {
                mute: true,
                ..Block::new(start as u32 * 16, PatternRef(0))
            });
        }
        p.validate().unwrap();
        let before = p.clone();
        let removed = p.remove_machine(MachineUid(4)).unwrap();
        assert_eq!(p.song.automation.len(), 2);
        assert_eq!(
            p.song.lanes.iter().map(|l| l.0.0).collect::<Vec<_>>(),
            [3, 1]
        );
        p.validate().unwrap();
        p.restore_machine(removed);
        assert_eq!(p, before);
        assert!(p.remove_machine(MachineUid(99)).is_none());
    }

    #[test]
    fn a_duplicate_synths_self_lanes_follow_the_copy_and_other_lanes_stay() {
        let mut p = Project::default();
        let own = ParamId::Machine(MachineUid(1), MachineParam::Synth(SynthParam::Cutoff));
        let other = ParamId::Machine(MachineUid(4), MachineParam::Kit(KitParam::Volume));
        let lane = |param| {
            Some(ParamLane {
                param,
                values: vec![Some(0.25); crate::project::STEPS],
            })
        };
        let s = p.synth_mut(MachineUid(1)).unwrap();
        s.patterns[0].param_lane = lane(own);
        s.patterns[5].param_lane = lane(other);
        let d = p.duplicate_machine(MachineUid(1)).unwrap();
        let MachineBody::Synth(copy) = &d.body else {
            panic!("not a synth")
        };
        assert_eq!(
            copy.patterns[0].param_lane,
            lane(ParamId::Machine(
                d.uid,
                MachineParam::Synth(SynthParam::Cutoff)
            ))
        );
        assert_eq!(copy.patterns[5].param_lane, lane(other));
        // The source keeps its own lane.
        assert_eq!(
            p.synth(MachineUid(1)).unwrap().patterns[0].param_lane,
            lane(own)
        );
        p.insert_machine(1, d).unwrap();
        p.validate().unwrap();
    }
}
