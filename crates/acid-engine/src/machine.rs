//! Machine identity and types. A project's rack is an ordered list of
//! machines, each named by a stable `MachineUid` and built from a
//! `MachineType`.

use serde::{Deserialize, Serialize};

/// Rack capacity grows in steps of this many machines.
pub const RACK_CHUNK: usize = 16;

/// The capacity an engine holds for a rack of `machines`.
pub fn rack_capacity(machines: usize) -> usize {
    machines.div_ceil(RACK_CHUNK).max(1) * RACK_CHUNK
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
    Mono,
    PadKit,
}

impl MachineType {
    /// Every type a rack can hold.
    pub const ALL: [MachineType; 7] = [
        MachineType::Synth,
        MachineType::Kit808,
        MachineType::Kit909,
        MachineType::Fm,
        MachineType::Sampler,
        MachineType::Mono,
        MachineType::PadKit,
    ];

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            MachineType::Synth => "Acid Synth",
            MachineType::Kit808 => "Kit A",
            MachineType::Kit909 => "Kit B",
            MachineType::Fm => "FM",
            MachineType::Sampler => "Sampler",
            MachineType::Mono => "Mono",
            MachineType::PadKit => "Pad Kit",
        }
    }

    /// Name stem for default names.
    pub fn stem(self) -> &'static str {
        match self {
            MachineType::Synth => "Synth",
            MachineType::Kit808 => "Kit A",
            MachineType::Kit909 => "Kit B",
            MachineType::Fm => "FM",
            MachineType::Sampler => "Sampler",
            MachineType::Mono => "Mono",
            MachineType::PadKit => "Pads",
        }
    }

    /// The name stem older projects gave this type, where it differs
    /// from today's `stem`.
    fn legacy_stem(self) -> Option<&'static str> {
        match self {
            MachineType::Kit808 => Some("808"),
            MachineType::Kit909 => Some("909"),
            _ => None,
        }
    }

    /// Rough heap + inline bytes one machine of this type takes in the
    /// engine (DSP state, buffers, channel strip), excluding samples.
    /// What `Slot::from_entry` allocated, rounded up to 4 KiB (measured
    /// 2026-10-17 by `tests/slot_memory.rs`).
    pub const fn memory_estimate(self) -> usize {
        match self {
            MachineType::Synth => 16 << 10,
            MachineType::Kit808 | MachineType::Kit909 => 12 << 10,
            MachineType::Fm => 44 << 10,
            MachineType::Sampler => 24 << 10,
            MachineType::Mono => 16 << 10,
            // Measured 18 152 bytes on 2026-10-17, empty pads.
            MachineType::PadKit => 20 << 10,
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

/// Today's name for a machine an older project saved under a default name
/// of the old stem: "808" → "Kit A", "808 3" → "Kit A 3" (likewise
/// "909…" → "Kit B…"). `None` for any other name, which the user chose.
pub fn renamed_legacy_default(ty: MachineType, name: &str) -> Option<String> {
    let rest = name.strip_prefix(ty.legacy_stem()?)?;
    let stem = ty.stem();
    if rest.is_empty() {
        return Some(stem.to_string());
    }
    let n = rest.strip_prefix(' ')?;
    (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| format!("{stem} {n}"))
}

/// Default name for a new machine of `ty`, given the names already in the
/// rack: the stem alone if unused ("Kit A"), else stem + lowest free number
/// ≥ 2 ("Kit A 2"); Synth always numbers ("Synth 1", "Synth 2", …).
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
        (MachineUid(3), MachineType::Kit808, "Kit A"),
        (MachineUid(4), MachineType::Kit909, "Kit B"),
        (MachineUid(5), MachineType::Fm, "FM"),
        (MachineUid(6), MachineType::Sampler, "Sampler"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rack_capacity_grows_in_chunks_of_sixteen() {
        assert_eq!(
            [0, 1, 16, 17, 32, 33].map(rack_capacity),
            [16, 16, 16, 32, 32, 48]
        );
    }

    #[test]
    fn default_names_number_from_two_except_synths() {
        assert_eq!(default_name(MachineType::Kit808, &[]), "Kit A");
        assert_eq!(default_name(MachineType::Kit808, &["Kit A"]), "Kit A 2");
        assert_eq!(
            default_name(MachineType::Kit808, &["Kit A", "Kit A 2"]),
            "Kit A 3"
        );
        assert_eq!(default_name(MachineType::Kit808, &["Kit A 2"]), "Kit A");
        assert_eq!(default_name(MachineType::Kit909, &["Kit B"]), "Kit B 2");
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
    fn only_old_default_kit_names_are_renamed() {
        use MachineType as T;
        let r = |ty, name| renamed_legacy_default(ty, name);
        assert_eq!(r(T::Kit808, "808").as_deref(), Some("Kit A"));
        assert_eq!(r(T::Kit808, "808 2").as_deref(), Some("Kit A 2"));
        assert_eq!(r(T::Kit909, "909").as_deref(), Some("Kit B"));
        assert_eq!(r(T::Kit909, "909 12").as_deref(), Some("Kit B 12"));
        for (ty, name) in [
            (T::Kit808, "808 Boom"),
            (T::Kit808, "8080"),
            (T::Kit808, "808 "),
            (T::Kit808, "808 2a"),
            (T::Kit808, "Kit A"),
            (T::Kit808, "909"),
            (T::Kit909, "808"),
            (T::Synth, "808"),
            (T::Sampler, "909 2"),
        ] {
            assert_eq!(r(ty, name), None, "{ty:?} {name:?} is kept");
        }
    }

    #[test]
    fn the_standard_rack_is_todays_six() {
        let r = standard_rack();
        let names: Vec<_> = r.iter().map(|m| m.2).collect();
        assert_eq!(
            names,
            ["Synth 1", "Synth 2", "Kit A", "Kit B", "FM", "Sampler"]
        );
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
        assert_eq!(t, [true, false, false, true, false, true, false]);
    }
}
