//! Pure song helpers shared by the sequencer, offline render and the UI:
//! bar lengths, global step positions, automation lookup and editing.

use crate::params::ParamId;
use crate::project::{AutoLane, Bar, DrumPattern, PatternRef, Project, STEPS, Song, SynthPattern};
use std::collections::BTreeMap;

pub const MAX_BARS: usize = 999;
pub const DEFAULT_BAR_STEPS: usize = 16;

pub type SynthBanks<'a> = [&'a [SynthPattern]; 2];
pub type DrumBanks<'a> = [&'a [DrumPattern]; 2];

pub fn banks(p: &Project) -> (SynthBanks<'_>, DrumBanks<'_>) {
    (
        [&p.synths[0].patterns[..], &p.synths[1].patterns[..]],
        [&p.drums[0].patterns[..], &p.drums[1].patterns[..]],
    )
}

/// Steps in a bar: its longest assigned pattern, or 16 when none is assigned.
pub fn bar_steps(bar: &Bar, synths: SynthBanks<'_>, drums: DrumBanks<'_>) -> usize {
    let mut longest = 0;
    for (m, r) in bar.patterns.iter().enumerate() {
        let Some(PatternRef(slot)) = r else { continue };
        let length = if m < 2 {
            synths[m].get(*slot as usize).map(|p| p.length)
        } else {
            drums[m - 2].get(*slot as usize).map(|p| p.length)
        };
        if let Some(l) = length {
            longest = longest.max((l as usize).clamp(1, STEPS));
        }
    }
    if longest == 0 {
        DEFAULT_BAR_STEPS
    } else {
        longest
    }
}

/// Global step at which `bar` starts (`bar == bars.len()` gives the song length).
pub fn bar_start_step(
    song: &Song,
    bar: usize,
    synths: SynthBanks<'_>,
    drums: DrumBanks<'_>,
) -> u32 {
    song.bars
        .iter()
        .take(bar)
        .map(|b| bar_steps(b, synths, drums) as u32)
        .sum()
}

pub fn total_steps(song: &Song, synths: SynthBanks<'_>, drums: DrumBanks<'_>) -> u32 {
    bar_start_step(song, song.bars.len(), synths, drums)
}

/// The lane's value at `step`: its last point at or before it.
pub fn value_at(lane: &AutoLane, step: u32) -> Option<f32> {
    lane.points.range(..=step).next_back().map(|(_, v)| *v)
}

pub fn lane(song: &Song, param: ParamId) -> Option<&AutoLane> {
    song.automation.iter().find(|l| l.param == param)
}

pub fn is_automated(song: &Song, param: ParamId) -> bool {
    lane(song, param).is_some_and(|l| !l.points.is_empty())
}

/// Writes `value` at every step from `from` to `to` (inclusive, either order)
/// in `param`'s lane, creating the lane if needed.
pub fn write_automation(song: &mut Song, param: ParamId, from: u32, to: u32, value: f32) {
    let value = value.clamp(0.0, 1.0);
    let (a, b) = (from.min(to), from.max(to));
    let index = match song.automation.iter().position(|l| l.param == param) {
        Some(i) => i,
        None => {
            song.automation.push(AutoLane {
                param,
                points: BTreeMap::new(),
            });
            song.automation.len() - 1
        }
    };
    for step in a..=b {
        song.automation[index].points.insert(step, value);
    }
}

pub fn clear_automation(song: &mut Song, param: ParamId) {
    song.automation.retain(|l| l.param != param);
}

/// A pattern change made while recording: sets `machine` to `slot` in `bar`
/// and in the following bars that still held the bar's old reference.
pub fn write_pattern_ref(song: &mut Song, bar: usize, machine: usize, slot: u8) {
    let Some(old) = song.bars.get(bar).map(|b| b.patterns[machine]) else {
        return;
    };
    for b in bar..song.bars.len() {
        if b > bar && song.bars[b].patterns[machine] != old {
            break;
        }
        song.bars[b].patterns[machine] = Some(PatternRef(slot));
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests set one field of a default song
mod tests {
    use super::*;
    use crate::params::{MasterParam, SynthParam};
    use crate::project::{DrumPattern, Project, SynthPattern};

    fn project_with_lengths(synth0: u8, drum0: u8) -> Project {
        let mut p = Project::default();
        p.synths[0].patterns[1] = SynthPattern {
            length: synth0,
            ..SynthPattern::default()
        };
        p.drums[0].patterns[2] = DrumPattern {
            length: drum0,
            ..DrumPattern::default()
        };
        p
    }

    fn bar(refs: [Option<u8>; 4]) -> Bar {
        Bar {
            patterns: refs.map(|r| r.map(PatternRef)),
            ..Bar::default()
        }
    }

    #[test]
    fn bar_length_is_the_longest_assigned_pattern() {
        let p = project_with_lengths(32, 12);
        let (s, d) = banks(&p);
        assert_eq!(bar_steps(&bar([None; 4]), s, d), DEFAULT_BAR_STEPS);
        assert_eq!(bar_steps(&bar([Some(1), None, Some(2), None]), s, d), 32);
        assert_eq!(bar_steps(&bar([None, None, Some(2), None]), s, d), 12);
    }

    #[test]
    fn start_steps_accumulate_bar_lengths() {
        let mut p = project_with_lengths(32, 16);
        p.song.bars = vec![
            bar([None; 4]),
            bar([Some(1), None, None, None]),
            bar([None; 4]),
        ];
        let (s, d) = banks(&p);
        assert_eq!(bar_start_step(&p.song, 0, s, d), 0);
        assert_eq!(bar_start_step(&p.song, 1, s, d), 16);
        assert_eq!(bar_start_step(&p.song, 2, s, d), 48);
        assert_eq!(total_steps(&p.song, s, d), 64);
    }

    #[test]
    fn value_at_holds_the_last_point() {
        let mut song = Song::default();
        let id = ParamId::Synth(0, SynthParam::Cutoff);
        write_automation(&mut song, id, 4, 4, 0.2);
        write_automation(&mut song, id, 10, 10, 0.8);
        let l = lane(&song, id).unwrap();
        assert_eq!(value_at(l, 3), None);
        assert_eq!(value_at(l, 4), Some(0.2));
        assert_eq!(value_at(l, 9), Some(0.2));
        assert_eq!(value_at(l, 10), Some(0.8));
        assert_eq!(value_at(l, 99), Some(0.8));
    }

    #[test]
    fn write_automation_fills_an_inclusive_range_and_clamps() {
        let mut song = Song::default();
        let id = ParamId::Master(MasterParam::Level);
        assert!(!is_automated(&song, id));
        write_automation(&mut song, id, 7, 5, 1.5);
        let l = lane(&song, id).unwrap();
        assert_eq!(l.points.len(), 3);
        assert!(l.points.values().all(|v| *v == 1.0));
        write_automation(&mut song, id, 6, 6, 0.25);
        assert_eq!(lane(&song, id).unwrap().points[&6], 0.25);
        assert_eq!(song.automation.len(), 1, "one lane per param");
        assert!(is_automated(&song, id));
        clear_automation(&mut song, id);
        assert!(!is_automated(&song, id));
        assert!(song.automation.is_empty());
    }

    #[test]
    fn recorded_pattern_change_runs_until_the_next_change() {
        let mut song = Song::default();
        song.bars = [0, 0, 0, 5, 0]
            .iter()
            .map(|&r| bar([Some(r), None, None, None]))
            .collect();
        write_pattern_ref(&mut song, 1, 0, 9);
        let refs: Vec<_> = song.bars.iter().map(|b| b.patterns[0].unwrap().0).collect();
        assert_eq!(refs, vec![0, 9, 9, 5, 0]);
        write_pattern_ref(&mut song, 99, 0, 1);
        assert_eq!(song.bars.len(), 5, "out-of-range bar is ignored");
    }
}
