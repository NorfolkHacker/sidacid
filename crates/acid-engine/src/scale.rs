//! Musical scales and the project key (root + scale), shared by the
//! randomiser and the editor's highlight / snap / fold.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scale {
    #[default]
    Chromatic,
    Minor,
    MelodicMinor,
    Phrygian,
    Major,
    HungarianMinor,
}

impl Scale {
    pub const ALL: [Scale; 6] = [
        Scale::Chromatic,
        Scale::Minor,
        Scale::MelodicMinor,
        Scale::Phrygian,
        Scale::Major,
        Scale::HungarianMinor,
    ];

    /// Semitones above the root.
    pub fn degrees(self) -> &'static [u8] {
        match self {
            Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            Scale::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
            Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::HungarianMinor => &[0, 2, 3, 6, 7, 8, 11],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scale::Chromatic => "Chromatic",
            Scale::Minor => "Minor",
            Scale::MelodicMinor => "Melodic minor",
            Scale::Phrygian => "Phrygian",
            Scale::Major => "Major",
            Scale::HungarianMinor => "Hungarian minor",
        }
    }
}

pub const ROOT_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    /// Pitch class of the root, 0..12 with C = 0.
    pub root: u8,
    pub scale: Scale,
    #[serde(default)]
    pub snap: bool,
}

impl Key {
    pub fn contains(self, pitch: u8) -> bool {
        let rel = (pitch % 12 + 12 - self.root % 12) % 12;
        self.scale.degrees().contains(&rel)
    }

    pub fn is_root(self, pitch: u8) -> bool {
        pitch % 12 == self.root % 12
    }

    /// Nearest in-scale pitch, ties going up, within 0..=127.
    pub fn snap(self, pitch: u8) -> u8 {
        let pitch = pitch.min(127);
        for d in 0..12u8 {
            let up = pitch.saturating_add(d);
            if up <= 127 && self.contains(up) {
                return up;
            }
            if d <= pitch && self.contains(pitch - d) {
                return pitch - d;
            }
        }
        pitch
    }

    /// The next (or previous) in-scale pitch; from an off-scale pitch, the
    /// nearest in-scale pitch in that direction. Stays put at the ends.
    pub fn step(self, pitch: u8, up: bool) -> u8 {
        let mut p = pitch.min(127);
        loop {
            if up {
                if p >= 127 {
                    return pitch;
                }
                p += 1;
            } else {
                if p == 0 {
                    return pitch;
                }
                p -= 1;
            }
            if self.contains(p) {
                return p;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrees_of_each_scale() {
        assert_eq!(Scale::Chromatic.degrees(), (0..12).collect::<Vec<u8>>());
        assert_eq!(Scale::Minor.degrees(), [0, 2, 3, 5, 7, 8, 10]);
        assert_eq!(Scale::MelodicMinor.degrees(), [0, 2, 3, 5, 7, 9, 11]);
        assert_eq!(Scale::Phrygian.degrees(), [0, 1, 3, 5, 7, 8, 10]);
        assert_eq!(Scale::Major.degrees(), [0, 2, 4, 5, 7, 9, 11]);
        assert_eq!(Scale::HungarianMinor.degrees(), [0, 2, 3, 6, 7, 8, 11]);
        assert_eq!(Scale::ALL.len(), 6);
        assert_eq!(Scale::MelodicMinor.label(), "Melodic minor");
    }

    fn c_minor() -> Key {
        Key {
            root: 0,
            scale: Scale::Minor,
            snap: false,
        }
    }

    #[test]
    fn contains_c_minor() {
        let k = c_minor();
        let inside: Vec<u8> = (60..72).filter(|&p| k.contains(p)).collect();
        assert_eq!(inside, [60, 62, 63, 65, 67, 68, 70]);
        assert!(k.is_root(48) && k.is_root(60) && !k.is_root(61));
        let a = Key {
            root: 9,
            scale: Scale::Major,
            snap: false,
        };
        assert!(a.contains(69) && a.contains(73) && !a.contains(70));
    }

    #[test]
    fn snap_ties_go_up() {
        let major = Key {
            root: 0,
            scale: Scale::Major,
            snap: true,
        };
        assert_eq!(major.snap(61), 62); // C# is equidistant from C and D
        assert_eq!(major.snap(60), 60);
        assert_eq!(major.snap(66), 67); // F# is equidistant from F and G
    }

    #[test]
    fn snap_clamps_to_range() {
        // C# major: G (127) is off-scale and nothing above exists.
        let k = Key {
            root: 1,
            scale: Scale::Major,
            snap: true,
        };
        assert_eq!(k.snap(127), 126);
        // D major: C (0) is off-scale; C# above is nearest.
        let k = Key {
            root: 2,
            scale: Scale::Major,
            snap: true,
        };
        assert_eq!(k.snap(0), 1);
        let k = Key {
            root: 11,
            scale: Scale::HungarianMinor,
            snap: true,
        };
        for p in 0..=127u8 {
            let s = k.snap(p);
            assert!(s <= 127 && k.contains(s));
        }
    }

    #[test]
    fn step_within_and_across_octaves() {
        let k = c_minor();
        assert_eq!(k.step(60, true), 62);
        assert_eq!(k.step(70, true), 72);
        assert_eq!(k.step(60, false), 58);
        assert_eq!(k.step(62, false), 60);
        // from an off-scale pitch: nearest in that direction
        assert_eq!(k.step(61, true), 62);
        assert_eq!(k.step(61, false), 60);
        // clamped
        assert_eq!(k.step(127, true), 127);
        assert_eq!(k.step(0, false), 0);
        assert_eq!(k.step(126, true), 127); // G is in C minor
        let cs = Key {
            root: 1,
            scale: Scale::Major,
            snap: false,
        };
        assert_eq!(cs.step(126, true), 126); // nothing further up
    }

    #[test]
    fn chromatic_is_everything() {
        let k = Key::default();
        for p in 0..=127u8 {
            assert!(k.contains(p));
            assert_eq!(k.snap(p), p);
        }
        assert_eq!(k.step(60, true), 61);
        assert_eq!(k.step(60, false), 59);
        assert_eq!(k.step(127, true), 127);
    }
}
