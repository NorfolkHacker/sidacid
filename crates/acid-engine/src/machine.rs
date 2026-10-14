//! The six machines in the rack. Every per-machine array (selected
//! pattern, playhead, song bar slots, mixer channel) is indexed by
//! `MachineId::index`.

use serde::{Deserialize, Serialize};

/// Largest number of machines a rack may hold.
pub const MAX_MACHINES: usize = 16;

pub const MACHINES: usize = 6;
/// Machines with a song transpose: Synth 1, Synth 2, FM.
pub const TRANSPOSED: usize = 3;
/// Machines sequenced from note patterns: FM, Sampler.
pub const NOTE_MACHINES: usize = 2;
/// Index of the first note machine; note machine `n` is machine
/// `FIRST_NOTE_MACHINE + n`.
pub const FIRST_NOTE_MACHINE: usize = MachineId::Fm as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MachineId {
    Synth1,
    Synth2,
    Kit808,
    Kit909,
    Fm,
    Sampler,
}

/// Stable identity of a machine within a project. Allocated from
/// `Project::next_uid`, never reused, independent of rack position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MachineUid(pub u16);

/// The kind of sound engine a machine is built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MachineType {
    Synth,
    Kit808,
    Kit909,
    Fm,
    Sampler,
}

impl MachineType {
    pub const ALL: [MachineType; 5] = [
        MachineType::Synth,
        MachineType::Kit808,
        MachineType::Kit909,
        MachineType::Fm,
        MachineType::Sampler,
    ];

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            MachineType::Synth => "303 Synth",
            MachineType::Kit808 => "808 Kit",
            MachineType::Kit909 => "909 Kit",
            MachineType::Fm => "FM",
            MachineType::Sampler => "Sampler",
        }
    }

    /// Name stem for default names.
    pub fn stem(self) -> &'static str {
        match self {
            MachineType::Synth => "Synth",
            MachineType::Kit808 => "808",
            MachineType::Kit909 => "909",
            MachineType::Fm => "FM",
            MachineType::Sampler => "Sampler",
        }
    }

    /// Whether the song transpose applies to this type.
    pub fn transposes(self) -> bool {
        matches!(self, MachineType::Synth | MachineType::Fm)
    }
}

/// Default name for a new machine of `ty`, given the names already in the
/// rack: the stem alone if unused ("808"), else stem + lowest free number
/// ≥ 2 ("808 2"); Synth always numbers ("Synth 1", "Synth 2", …).
pub fn default_name(ty: MachineType, existing: &[&str]) -> String {
    let stem = ty.stem();
    let taken = |name: &str| existing.contains(&name);
    let numbered = |from: u32| {
        let n = (from..)
            .find(|n| !taken(&format!("{stem} {n}")))
            .unwrap_or(from);
        format!("{stem} {n}")
    };
    if ty == MachineType::Synth {
        numbered(1)
    } else if taken(stem) {
        numbered(2)
    } else {
        stem.to_string()
    }
}

/// Today's rack: (uid, type, name) for uids 1..=6.
pub fn standard_rack() -> [(MachineUid, MachineType, &'static str); 6] {
    [
        (MachineUid(1), MachineType::Synth, "Synth 1"),
        (MachineUid(2), MachineType::Synth, "Synth 2"),
        (MachineUid(3), MachineType::Kit808, "808"),
        (MachineUid(4), MachineType::Kit909, "909"),
        (MachineUid(5), MachineType::Fm, "FM"),
        (MachineUid(6), MachineType::Sampler, "Sampler"),
    ]
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

    /// Temporary bridge to the fixed-enum rack; removed when `MachineId`
    /// is replaced by uids. Synth1 → 1 … Sampler → 6.
    pub fn uid(self) -> MachineUid {
        MachineUid(self.index() as u16 + 1)
    }

    /// Inverse of `uid`; `None` for uids outside the six fixed machines.
    pub fn from_uid(uid: MachineUid) -> Option<MachineId> {
        let i = usize::from(uid.0).checked_sub(1)?;
        Self::from_index(i)
    }

    /// The sound-engine type of this fixed machine.
    pub fn machine_type(self) -> MachineType {
        match self {
            MachineId::Synth1 | MachineId::Synth2 => MachineType::Synth,
            MachineId::Kit808 => MachineType::Kit808,
            MachineId::Kit909 => MachineType::Kit909,
            MachineId::Fm => MachineType::Fm,
            MachineId::Sampler => MachineType::Sampler,
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
    fn transpose_index_covers_the_pitched_pattern_machines() {
        assert_eq!(MachineId::Synth1.transpose_index(), Some(0));
        assert_eq!(MachineId::Synth2.transpose_index(), Some(1));
        assert_eq!(MachineId::Fm.transpose_index(), Some(2));
        for m in [MachineId::Kit808, MachineId::Kit909, MachineId::Sampler] {
            assert_eq!(m.transpose_index(), None, "{m:?}");
        }
        assert_eq!(TRANSPOSED, 3);
    }

    #[test]
    fn indices_round_trip_and_kinds_are_dense() {
        for (i, m) in MachineId::ALL.iter().enumerate() {
            assert_eq!(m.index(), i);
            assert_eq!(MachineId::from_index(i), Some(*m));
        }
        assert_eq!(MachineId::from_index(MACHINES), None);
        for n in 0..NOTE_MACHINES {
            assert_eq!(kind_of(FIRST_NOTE_MACHINE + n), Some(MachineKind::Notes(n)));
        }
        assert_eq!(kind_of(4), Some(MachineKind::Notes(0)));
        assert_eq!(kind_of(5), Some(MachineKind::Notes(1)));
        assert_eq!(kind_of(3), Some(MachineKind::Drum(1)));
        let transposed: Vec<_> = MachineId::ALL
            .iter()
            .filter_map(|m| m.transpose_index())
            .collect();
        assert_eq!(transposed, (0..TRANSPOSED).collect::<Vec<_>>());
    }

    #[test]
    fn default_names_number_from_two_except_synths() {
        assert_eq!(default_name(MachineType::Kit808, &[]), "808");
        assert_eq!(default_name(MachineType::Kit808, &["808"]), "808 2");
        assert_eq!(
            default_name(MachineType::Kit808, &["808", "808 2"]),
            "808 3"
        );
        assert_eq!(default_name(MachineType::Kit808, &["808 2"]), "808");
        assert_eq!(default_name(MachineType::Synth, &[]), "Synth 1");
        assert_eq!(
            default_name(MachineType::Synth, &["Synth 1", "Synth 2"]),
            "Synth 3"
        );
        assert_eq!(
            default_name(MachineType::Sampler, &["Sampler", "Bass"]),
            "Sampler 2"
        );
    }

    #[test]
    fn the_standard_rack_is_todays_six() {
        let r = standard_rack();
        let names: Vec<_> = r.iter().map(|m| m.2).collect();
        assert_eq!(names, ["Synth 1", "Synth 2", "808", "909", "FM", "Sampler"]);
        let uids: Vec<_> = r.iter().map(|m| m.0.0).collect();
        assert_eq!(uids, [1, 2, 3, 4, 5, 6]);
        for (m, (uid, ty, _)) in MachineId::ALL.iter().zip(r) {
            assert_eq!(m.uid(), uid);
            assert_eq!(m.machine_type(), ty);
            assert_eq!(MachineId::from_uid(uid), Some(*m));
        }
        assert_eq!(MachineId::from_uid(MachineUid(7)), None);
    }

    #[test]
    fn only_synths_and_fm_transpose() {
        let t: Vec<_> = MachineType::ALL.iter().map(|t| t.transposes()).collect();
        assert_eq!(t, [true, false, false, true, false]);
    }
}
