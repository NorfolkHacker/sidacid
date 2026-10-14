//! Interim bridge (until the UI shows any rack): the six standard
//! machines by their old indices, through the project's uid accessors.
//! A loaded or new project always holds the standard six (validation
//! enforces it for now), so a missing machine is a bug.

use acid_engine::machine::{MachineId, MachineUid};
use acid_engine::project::{
    DrumMachine, FmMachine, MachineEntry, NotePattern, Project, SamplerMachine, SynthMachine,
};

const SYNTHS: [MachineId; 2] = [MachineId::Synth1, MachineId::Synth2];
const KITS: [MachineId; 2] = [MachineId::Kit808, MachineId::Kit909];
const MISSING: &str = "the standard rack holds this machine";

/// Uid of synth `s` (0 = Synth 1, 1 = Synth 2).
pub fn synth_uid(s: usize) -> MachineUid {
    SYNTHS[s].uid()
}

/// Uid of kit `k` (0 = 808, 1 = 909).
pub fn kit_uid(k: usize) -> MachineUid {
    KITS[k].uid()
}

/// Uid of machine `m` (by `MachineId::index`).
pub fn machine_uid(m: usize) -> MachineUid {
    MachineId::ALL[m].uid()
}

pub trait SixMachines {
    fn synth_at(&self, s: usize) -> &SynthMachine;
    fn synth_at_mut(&mut self, s: usize) -> &mut SynthMachine;
    fn kit_at(&self, k: usize) -> &DrumMachine;
    fn kit_at_mut(&mut self, k: usize) -> &mut DrumMachine;
    fn fm_machine(&self) -> &FmMachine;
    fn fm_machine_mut(&mut self) -> &mut FmMachine;
    fn sampler_machine(&self) -> &SamplerMachine;
    fn sampler_machine_mut(&mut self) -> &mut SamplerMachine;
    /// Note patterns of machine `m` (by `MachineId::index`), if it has any.
    fn notes_at(&self, m: usize) -> Option<&Vec<NotePattern>>;
    fn notes_at_mut(&mut self, m: usize) -> Option<&mut Vec<NotePattern>>;
    /// Machine `m` (by `MachineId::index`).
    fn entry_at(&self, m: usize) -> &MachineEntry;
    fn entry_at_mut(&mut self, m: usize) -> &mut MachineEntry;
}

impl SixMachines for Project {
    fn synth_at(&self, s: usize) -> &SynthMachine {
        self.synth(SYNTHS[s].uid()).expect(MISSING)
    }

    fn synth_at_mut(&mut self, s: usize) -> &mut SynthMachine {
        self.synth_mut(SYNTHS[s].uid()).expect(MISSING)
    }

    fn kit_at(&self, k: usize) -> &DrumMachine {
        self.kit(KITS[k].uid()).expect(MISSING)
    }

    fn kit_at_mut(&mut self, k: usize) -> &mut DrumMachine {
        self.kit_mut(KITS[k].uid()).expect(MISSING)
    }

    fn fm_machine(&self) -> &FmMachine {
        self.fm(MachineId::Fm.uid()).expect(MISSING)
    }

    fn fm_machine_mut(&mut self) -> &mut FmMachine {
        self.fm_mut(MachineId::Fm.uid()).expect(MISSING)
    }

    fn sampler_machine(&self) -> &SamplerMachine {
        self.sampler(MachineId::Sampler.uid()).expect(MISSING)
    }

    fn sampler_machine_mut(&mut self) -> &mut SamplerMachine {
        self.sampler_mut(MachineId::Sampler.uid()).expect(MISSING)
    }

    fn notes_at(&self, m: usize) -> Option<&Vec<NotePattern>> {
        self.note_patterns(MachineId::from_index(m)?.uid())
    }

    fn notes_at_mut(&mut self, m: usize) -> Option<&mut Vec<NotePattern>> {
        self.note_patterns_mut(MachineId::from_index(m)?.uid())
    }

    fn entry_at(&self, m: usize) -> &MachineEntry {
        self.machine(MachineId::ALL[m].uid()).expect(MISSING)
    }

    fn entry_at_mut(&mut self, m: usize) -> &mut MachineEntry {
        self.machine_mut(MachineId::ALL[m].uid()).expect(MISSING)
    }
}
