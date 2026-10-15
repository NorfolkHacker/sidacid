//! Machine identity and types. A project's rack is an ordered list of
//! machines, each named by a stable `MachineUid` and built from a
//! `MachineType`.

use serde::{Deserialize, Serialize};

/// Largest number of machines a rack may hold.
pub const MAX_MACHINES: usize = 16;

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
    Mono,
}

impl MachineType {
    pub const ALL: [MachineType; 6] = [
        MachineType::Synth,
        MachineType::Kit808,
        MachineType::Kit909,
        MachineType::Fm,
        MachineType::Sampler,
        MachineType::Mono,
    ];

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            MachineType::Synth => "303 Synth",
            MachineType::Kit808 => "808 Kit",
            MachineType::Kit909 => "909 Kit",
            MachineType::Fm => "FM",
            MachineType::Sampler => "Sampler",
            MachineType::Mono => "Mono",
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
            MachineType::Mono => "Mono",
        }
    }

    /// Whether the song transpose applies to this type.
    pub fn transposes(self) -> bool {
        matches!(
            self,
            MachineType::Synth | MachineType::Fm | MachineType::Mono
        )
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let types: Vec<_> = r.iter().map(|m| m.1).collect();
        use MachineType as T;
        assert_eq!(
            types,
            [T::Synth, T::Synth, T::Kit808, T::Kit909, T::Fm, T::Sampler]
        );
    }

    #[test]
    fn only_synths_fm_and_mono_transpose() {
        let t: Vec<_> = MachineType::ALL.iter().map(|t| t.transposes()).collect();
        assert_eq!(t, [true, false, false, true, false, true]);
    }
}
