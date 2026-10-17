//! A whole Pad Kit as one `.padkit` file: every pad with its layers, and
//! the kit's and pads' knob values. Patterns and channel params are not
//! part of a kit.

use super::pad::Pad;
use super::{MAX_LAYERS, PADS};
use crate::params::MachineParam;
use crate::project::{MachineBody, MachineEntry};
use serde::{Deserialize, Serialize};

pub const KIT_VERSION: u32 = 1;
pub const KIT_EXTENSION: &str = "padkit";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PadKitFile {
    pub version: u32,
    /// Exactly `PADS`.
    pub pads: Vec<Pad>,
    /// `PadKit` and `Pad` params only.
    pub params: Vec<(MachineParam, f32)>,
}

fn is_kit_param(p: MachineParam) -> bool {
    match p {
        MachineParam::PadKit(_) => true,
        MachineParam::Pad(i, _) => (i as usize) < PADS,
        _ => false,
    }
}

/// Why `pad` could not be used, if it couldn't.
fn pad_problem(pad: &Pad) -> Option<String> {
    if pad.layers.len() > MAX_LAYERS {
        return Some(format!("more than {MAX_LAYERS} layers"));
    }
    for (j, l) in pad.layers.iter().enumerate() {
        if let Some(p) = l.problem() {
            return Some(format!("layer {}: {p}", j + 1));
        }
    }
    (!pad.is_well_formed()).then(|| "the velocity ranges are not valid".to_string())
}

impl PadKitFile {
    /// From a Pad Kit machine entry (its pads and its PadKit/Pad param values).
    pub fn from_entry(entry: &MachineEntry) -> Option<PadKitFile> {
        let MachineBody::PadKit(k) = &entry.body else {
            return None;
        };
        Some(PadKitFile {
            version: KIT_VERSION,
            pads: k.pads.clone(),
            params: entry
                .params
                .iter()
                .filter(|(p, _)| is_kit_param(**p))
                .map(|(p, v)| (*p, *v))
                .collect(),
        })
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).expect("kit serialises")
    }

    /// Parses with the project's decompression budget (`Some(bytes)` of
    /// 16-bit PCM; None = unbounded). A malformed pad refuses the whole
    /// file: "pad 3: layer 2: the sample is empty".
    pub fn from_ron_within(text: &str, budget: Option<u64>) -> Result<PadKitFile, String> {
        let f: PadKitFile = crate::sampler::slot::pcm_codec::with_budget(budget, || {
            ron::from_str(text).map_err(|e| e.to_string())
        })?;
        if f.version != KIT_VERSION {
            return Err(format!("version {} is not supported", f.version));
        }
        if f.pads.len() != PADS {
            return Err(format!("it has {} pads, not {PADS}", f.pads.len()));
        }
        for (i, pad) in f.pads.iter().enumerate() {
            if let Some(p) = pad_problem(pad) {
                return Err(format!("pad {}: {p}", i + 1));
            }
        }
        if let Some((p, v)) = f
            .params
            .iter()
            .find(|(p, v)| !is_kit_param(*p) || !(0.0..=1.0).contains(v))
        {
            return Err(match p {
                MachineParam::PadKit(k) => format!("{} is out of range ({v})", k.name()),
                MachineParam::Pad(i, k) if (*i as usize) < PADS => {
                    format!(
                        "pad {}: {} is out of range ({v})",
                        *i as usize + 1,
                        k.name()
                    )
                }
                _ => "a setting that isn't a Pad Kit's".to_string(),
            });
        }
        Ok(f)
    }

    /// Size of the decoded audio of every layer.
    pub fn memory_bytes(&self) -> usize {
        self.pads.iter().map(Pad::memory_bytes).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::MachineType;
    use crate::padkit::pad::Layer;
    use crate::params::PadParam;
    use crate::project::Project;

    fn entry() -> MachineEntry {
        let mut p = Project::default();
        let mut m = p.new_machine(MachineType::PadKit).unwrap();
        if let crate::project::MachineBody::PadKit(k) = &mut m.body {
            k.pads[0]
                .layers
                .push(Layer::new("kick", 44_100, 1, &[0.5; 64]));
            k.pads[0].split_evenly();
        }
        m.params.insert(MachineParam::Pad(0, PadParam::Tune), 0.75);
        m
    }

    #[test]
    fn a_kit_round_trips() {
        let f = PadKitFile::from_entry(&entry()).unwrap();
        let back = PadKitFile::from_ron_within(&f.to_ron(), None).unwrap();
        assert_eq!(back, f);
        assert!(
            back.params
                .contains(&(MachineParam::Pad(0, PadParam::Tune), 0.75))
        );
    }

    #[test]
    fn a_broken_pad_refuses_the_whole_file() {
        let mut f = PadKitFile::from_entry(&entry()).unwrap();
        f.pads[2].layers.push(Layer::new("x", 44_100, 1, &[]));
        let err = PadKitFile::from_ron_within(&f.to_ron(), None).unwrap_err();
        assert!(err.starts_with("pad 3: "), "{err}");
        let mut g = PadKitFile::from_entry(&entry()).unwrap();
        g.pads.pop();
        assert!(PadKitFile::from_ron_within(&g.to_ron(), None).is_err());
    }

    #[test]
    fn a_kit_over_the_budget_is_refused_while_parsing() {
        let f = PadKitFile::from_entry(&entry()).unwrap();
        assert!(PadKitFile::from_ron_within(&f.to_ron(), Some(10)).is_err());
    }

    #[test]
    fn a_wrong_version_or_foreign_param_is_refused() {
        let mut f = PadKitFile::from_entry(&entry()).unwrap();
        f.version = 2;
        assert!(PadKitFile::from_ron_within(&f.to_ron(), None).is_err());
        let mut g = PadKitFile::from_entry(&entry()).unwrap();
        g.params.push((
            MachineParam::Channel(crate::params::ChannelParam::Level),
            0.5,
        ));
        let err = PadKitFile::from_ron_within(&g.to_ron(), None).unwrap_err();
        assert_eq!(err, "a setting that isn't a Pad Kit's");
        let mut h = PadKitFile::from_entry(&entry()).unwrap();
        h.params.push((MachineParam::Pad(0, PadParam::Tune), 1.5));
        let err = PadKitFile::from_ron_within(&h.to_ron(), None).unwrap_err();
        assert_eq!(err, "pad 1: Tune is out of range (1.5)");
        let mut k = PadKitFile::from_entry(&entry()).unwrap();
        k.params.push((MachineParam::Pad(16, PadParam::Tune), 0.5));
        assert!(PadKitFile::from_ron_within(&k.to_ron(), None).is_err());
    }
}
