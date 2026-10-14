//! Rack edits on a [`Project`]: add, duplicate, remove/restore, move, rename.
//! Pure data; re-exported from `project`.

use crate::machine::{MAX_MACHINES, MachineType, MachineUid, default_name};
use crate::params::ParamId;
use crate::project::{AutoLane, Cell, MachineBody, MachineEntry, ParamLane, Project};

/// Everything removing a machine takes out of the project, so undo can put it back exactly.
#[derive(Clone, Debug, PartialEq)]
pub struct RemovedMachine {
    /// Its rack position.
    pub index: usize,
    pub entry: MachineEntry,
    /// (bar index, index within `bar.cells`, its cell).
    pub cells: Vec<(usize, usize, Cell)>,
    /// (index in `song.automation`, lane).
    pub lanes: Vec<(usize, AutoLane)>,
    /// (synth uid, pattern index, lane) that targeted it.
    pub param_lanes: Vec<(MachineUid, u8, ParamLane)>,
}

impl Project {
    /// A fresh machine of `ty` with the next uid and a default name; does not insert it.
    pub fn new_machine(&mut self, ty: MachineType) -> MachineEntry {
        let names: Vec<&str> = self.machines.iter().map(|m| m.name.as_str()).collect();
        let name = default_name(ty, &names);
        MachineEntry::new(self.take_uid(), ty, name)
    }

    /// Deep copy of `uid` with a new uid and "<name> copy"; does not insert it. None if absent.
    pub fn duplicate_machine(&mut self, uid: MachineUid) -> Option<MachineEntry> {
        let mut copy = self.machine(uid)?.clone();
        copy.uid = self.take_uid();
        copy.name = format!("{} copy", copy.name);
        Some(copy)
    }

    /// Inserts at `index` (clamped to len); None if the rack is full or the uid is taken.
    pub fn insert_machine(&mut self, index: usize, entry: MachineEntry) -> Option<()> {
        if self.machines.len() >= MAX_MACHINES || self.machine(entry.uid).is_some() {
            return None;
        }
        // Keep `next_uid` above every uid in the rack, whatever the entry's origin.
        self.next_uid = self.next_uid.max(entry.uid.0.saturating_add(1));
        let index = index.min(self.machines.len());
        self.machines.insert(index, entry);
        Some(())
    }

    /// Takes `uid` out of the rack with its song cells, its song automation lanes and
    /// every synth pattern param lane that targets it.
    pub fn remove_machine(&mut self, uid: MachineUid) -> Option<RemovedMachine> {
        let index = self.position(uid)?;
        let entry = self.machines.remove(index);
        let mut cells = Vec::new();
        for (b, bar) in self.song.bars.iter_mut().enumerate() {
            if let Some(i) = bar.cells.iter().position(|(u, _)| *u == uid) {
                let (_, cell) = bar.cells.remove(i);
                cells.push((b, i, cell));
            }
        }
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
            cells,
            lanes,
            param_lanes,
        })
    }

    /// Puts a removed machine back exactly (entry at its index, cells, lanes at their indices, param lanes).
    pub fn restore_machine(&mut self, removed: RemovedMachine) {
        let RemovedMachine {
            index,
            entry,
            cells,
            lanes,
            param_lanes,
        } = removed;
        let uid = entry.uid;
        if self.insert_machine(index, entry).is_none() {
            return;
        }
        for (b, i, cell) in cells {
            if let Some(bar) = self.song.bars.get_mut(b)
                && !bar.cells.iter().any(|(u, _)| *u == uid)
            {
                let i = i.min(bar.cells.len());
                bar.cells.insert(i, (uid, cell));
            }
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

    /// The next uid; uids are never reused.
    fn take_uid(&mut self) -> MachineUid {
        let uid = MachineUid(self.next_uid);
        self.next_uid += 1;
        uid
    }
}

/// Whether `param` belongs to machine `uid`.
fn targets(param: ParamId, uid: MachineUid) -> bool {
    matches!(param, ParamId::Machine(u, _) if u == uid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{KitParam, MachineParam};
    use crate::project::{Bar, PatternRef};

    #[test]
    fn new_machines_take_fresh_uids_and_default_names() {
        let mut p = Project::default();
        let a = p.new_machine(MachineType::Kit909);
        assert_eq!((a.uid, a.name.as_str()), (MachineUid(7), "909 2"));
        p.insert_machine(p.machines.len(), a).unwrap();
        let b = p.new_machine(MachineType::Synth);
        assert_eq!((b.uid, b.name.as_str()), (MachineUid(8), "Synth 3"));
        assert_eq!(p.next_uid, 9);
        p.validate().unwrap();
    }

    #[test]
    fn the_rack_refuses_a_seventeenth_machine_and_a_taken_uid() {
        let mut p = Project::default();
        while p.machines.len() < MAX_MACHINES {
            let m = p.new_machine(MachineType::Fm);
            p.insert_machine(p.machines.len(), m).unwrap();
        }
        let extra = p.new_machine(MachineType::Fm);
        assert!(p.insert_machine(0, extra).is_none());
        let mut q = Project::default();
        let dup = q.machines[0].clone();
        assert!(q.insert_machine(0, dup).is_none());
    }

    #[test]
    fn removing_a_machine_takes_its_cells_lanes_and_foreign_param_lanes_and_restore_undoes_it() {
        let mut p = crate::demo::demo_project();
        // Synth 1's pattern 0 has a param lane on the 909's Level; the song names the 909; automation too.
        let target = ParamId::Machine(MachineUid(4), MachineParam::Kit(KitParam::Volume));
        p.synth_mut(MachineUid(1)).unwrap().patterns[0].param_lane = Some(ParamLane {
            param: target,
            values: vec![Some(0.5); crate::project::STEPS],
        });
        let mut bar = Bar::default();
        *bar.cell_mut(MachineUid(4)) = Cell {
            pattern: Some(PatternRef(1)),
            mute: false,
            transpose: 0,
        };
        *bar.cell_mut(MachineUid(1)) = Cell {
            pattern: Some(PatternRef(0)),
            mute: false,
            transpose: 0,
        };
        p.song.bars.push(bar);
        p.song.automation.push(AutoLane {
            param: target,
            points: [(0u32, 0.3f32)].into_iter().collect(),
        });
        p.validate().unwrap();
        let before = p.clone();
        let removed = p.remove_machine(MachineUid(4)).unwrap();
        assert_eq!(removed.index, 3);
        assert!(p.machine(MachineUid(4)).is_none());
        assert_eq!(p.song.bars[0].cells.len(), 1);
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
    fn duplicates_copy_sound_and_patterns_but_not_song_cells() {
        let mut p = crate::demo::demo_project();
        let d = p.duplicate_machine(MachineUid(1)).unwrap();
        assert_eq!(d.name, "Synth 1 copy");
        assert_ne!(d.uid, MachineUid(1));
        assert_eq!(d.body, p.machine(MachineUid(1)).unwrap().body);
        assert_eq!(d.params, p.machine(MachineUid(1)).unwrap().params);
        p.insert_machine(1, d.clone()).unwrap();
        assert!(
            p.song
                .bars
                .iter()
                .all(|b| b.cells.iter().all(|(u, _)| *u != d.uid))
        );
        p.validate().unwrap();
    }

    #[test]
    fn moving_and_renaming() {
        let mut p = Project::default();
        assert_eq!(p.move_machine(MachineUid(1), 99), Some(0));
        assert_eq!(p.rack_uids().last(), Some(&MachineUid(1)));
        assert_eq!(
            p.rename_machine(MachineUid(3), "Boom"),
            Some("808".to_string())
        );
        assert_eq!(p.machine(MachineUid(3)).unwrap().name, "Boom");
        p.rename_machine(MachineUid(3), "  ");
        assert_eq!(p.machine(MachineUid(3)).unwrap().name, "808");
    }

    #[test]
    fn restore_puts_interleaved_cells_and_lanes_back_in_their_places() {
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
        for order in [[3u16, 4, 1], [4, 1, 3], [1, 3, 4]] {
            let mut bar = Bar::default();
            for u in order {
                bar.cell_mut(MachineUid(u)).mute = true;
            }
            p.song.bars.push(bar);
        }
        p.validate().unwrap();
        let before = p.clone();
        let removed = p.remove_machine(MachineUid(4)).unwrap();
        assert_eq!(p.song.automation.len(), 2);
        assert_eq!(
            p.song.bars[0]
                .cells
                .iter()
                .map(|c| c.0.0)
                .collect::<Vec<_>>(),
            [3, 1]
        );
        p.validate().unwrap();
        p.restore_machine(removed);
        assert_eq!(p, before);
        assert!(p.remove_machine(MachineUid(99)).is_none());
    }
}
