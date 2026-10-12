//! The six machines in the rack. Every per-machine array (selected
//! pattern, playhead, song bar slots, mixer channel) is indexed by
//! `MachineId::index`.

pub const MACHINES: usize = 6;
/// Machines with a song transpose: Synth 1, Synth 2, FM.
pub const TRANSPOSED: usize = 3;
/// Machines sequenced from note patterns: FM, Sampler.
pub const NOTE_MACHINES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MachineId {
    Synth1,
    Synth2,
    Kit808,
    Kit909,
    Fm,
    Sampler,
}

/// What a machine plays, with its index among machines of that kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineKind {
    Synth(usize),
    Drum(usize),
    Notes(usize),
}

impl MachineId {
    pub const ALL: [MachineId; MACHINES] = [
        MachineId::Synth1,
        MachineId::Synth2,
        MachineId::Kit808,
        MachineId::Kit909,
        MachineId::Fm,
        MachineId::Sampler,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<MachineId> {
        Self::ALL.get(i).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            MachineId::Synth1 => "Synth 1",
            MachineId::Synth2 => "Synth 2",
            MachineId::Kit808 => "808",
            MachineId::Kit909 => "909",
            MachineId::Fm => "FM",
            MachineId::Sampler => "Sampler",
        }
    }

    pub fn kind(self) -> MachineKind {
        match self {
            MachineId::Synth1 => MachineKind::Synth(0),
            MachineId::Synth2 => MachineKind::Synth(1),
            MachineId::Kit808 => MachineKind::Drum(0),
            MachineId::Kit909 => MachineKind::Drum(1),
            MachineId::Fm => MachineKind::Notes(0),
            MachineId::Sampler => MachineKind::Notes(1),
        }
    }

    /// Index into `Bar::transpose`; `None` for machines that never transpose.
    pub fn transpose_index(self) -> Option<usize> {
        match self {
            MachineId::Synth1 => Some(0),
            MachineId::Synth2 => Some(1),
            MachineId::Fm => Some(2),
            _ => None,
        }
    }
}

pub fn kind_of(machine: usize) -> Option<MachineKind> {
    MachineId::from_index(machine).map(MachineId::kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_round_trip_and_kinds_are_dense() {
        for (i, m) in MachineId::ALL.iter().enumerate() {
            assert_eq!(m.index(), i);
            assert_eq!(MachineId::from_index(i), Some(*m));
        }
        assert_eq!(MachineId::from_index(MACHINES), None);
        assert_eq!(kind_of(4), Some(MachineKind::Notes(0)));
        assert_eq!(kind_of(5), Some(MachineKind::Notes(1)));
        assert_eq!(kind_of(3), Some(MachineKind::Drum(1)));
        let transposed: Vec<_> = MachineId::ALL
            .iter()
            .filter_map(|m| m.transpose_index())
            .collect();
        assert_eq!(transposed, (0..TRANSPOSED).collect::<Vec<_>>());
    }
}
