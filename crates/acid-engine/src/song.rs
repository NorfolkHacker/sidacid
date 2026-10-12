//! Pure song helpers shared by the sequencer, offline render and the UI:
//! bar lengths, global step positions, automation lookup and editing.

use crate::params::ParamId;
use crate::project::{
    AutoLane, Bar, DrumPattern, PATTERNS, PatternRef, Project, STEPS, Song, SynthPattern,
};
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
    // Machines 4 and 5 join bar lengths in the next change.
    for (m, r) in bar.patterns.iter().enumerate().take(4) {
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
/// in `param`'s lane, creating the lane if needed. A non-finite value is
/// ignored.
pub fn write_automation(song: &mut Song, param: ParamId, from: u32, to: u32, value: f32) {
    if !value.is_finite() {
        return;
    }
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
/// Out-of-range machines and slots are ignored.
pub fn write_pattern_ref(song: &mut Song, bar: usize, machine: usize, slot: u8) {
    if machine >= 4 || slot as usize >= PATTERNS {
        return;
    }
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

/// Moves every automation point at or after `from` by `delta` steps.
fn shift_automation(song: &mut Song, from: u32, delta: i64) {
    for lane in &mut song.automation {
        let moved: Vec<(u32, f32)> = lane.points.range(from..).map(|(k, v)| (*k, *v)).collect();
        for (k, _) in &moved {
            lane.points.remove(k);
        }
        for (k, v) in moved {
            let to = k as i64 + delta;
            if to >= 0 {
                lane.points.insert(to as u32, v);
            }
        }
    }
}

pub fn insert_bars(
    song: &mut Song,
    at: usize,
    bars: &[Bar],
    synths: SynthBanks<'_>,
    drums: DrumBanks<'_>,
) {
    let at = at.min(song.bars.len());
    let room = MAX_BARS.saturating_sub(song.bars.len());
    let bars = &bars[..bars.len().min(room)];
    if bars.is_empty() {
        return;
    }
    let start = bar_start_step(song, at, synths, drums);
    let added: u32 = bars
        .iter()
        .map(|b| bar_steps(b, synths, drums) as u32)
        .sum();
    shift_automation(song, start, added as i64);
    song.bars.splice(at..at, bars.iter().copied());
    if let Some((a, b)) = song.loop_range {
        let n = bars.len() as u16;
        let bump = |i: u16| if i as usize >= at { i + n } else { i };
        song.loop_range = Some((bump(a), bump(b)));
    }
}

pub fn delete_bars(
    song: &mut Song,
    first: usize,
    last: usize,
    synths: SynthBanks<'_>,
    drums: DrumBanks<'_>,
) {
    if song.bars.is_empty() {
        return;
    }
    let last = last.min(song.bars.len() - 1);
    if first > last {
        return;
    }
    let start = bar_start_step(song, first, synths, drums);
    let end = bar_start_step(song, last + 1, synths, drums);
    for lane in &mut song.automation {
        lane.points.retain(|k, _| *k < start || *k >= end);
    }
    shift_automation(song, end, -((end - start) as i64));
    song.automation.retain(|l| !l.points.is_empty());
    song.bars.drain(first..=last);
    let n = (last - first + 1) as u16;
    song.loop_range = song.loop_range.and_then(|(a, b)| {
        let map = |i: u16| -> Option<u16> {
            if (i as usize) < first {
                Some(i)
            } else if i as usize > last {
                Some(i - n)
            } else {
                None
            }
        };
        Some((map(a)?, map(b)?))
    });
}

pub fn duplicate_bars(
    song: &mut Song,
    first: usize,
    last: usize,
    synths: SynthBanks<'_>,
    drums: DrumBanks<'_>,
) {
    if first > last || last >= song.bars.len() {
        return;
    }
    let copy: Vec<Bar> = song.bars[first..=last].to_vec();
    let src_start = bar_start_step(song, first, synths, drums);
    let src_end = bar_start_step(song, last + 1, synths, drums);
    let points: Vec<(ParamId, Vec<(u32, f32)>)> = song
        .automation
        .iter()
        .map(|l| {
            (
                l.param,
                l.points
                    .range(src_start..src_end)
                    .map(|(k, v)| (k - src_start, *v))
                    .collect(),
            )
        })
        .collect();
    let before = song.bars.len();
    insert_bars(song, last + 1, &copy, synths, drums);
    let inserted = song.bars.len() - before;
    if inserted == 0 {
        return;
    }
    // At the bar cap only the first `inserted` bars were copied: their
    // automation ends where the truncated copy ends.
    let span = bar_start_step(song, first + inserted, synths, drums) - src_start;
    let dst = bar_start_step(song, last + 1, synths, drums);
    for (param, pts) in points {
        for (offset, v) in pts.into_iter().filter(|(o, _)| *o < span) {
            write_automation(song, param, dst + offset, dst + offset, v);
        }
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
        let mut patterns = [None; crate::machine::MACHINES];
        for (m, r) in refs.into_iter().enumerate() {
            patterns[m] = r.map(PatternRef);
        }
        Bar {
            patterns,
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

    fn sixteen_step_song(n: usize) -> (Project, ParamId) {
        let mut p = Project::default();
        p.song.bars = vec![Bar::default(); n];
        (p, ParamId::Master(MasterParam::Level))
    }

    #[test]
    fn insert_shifts_automation_and_loop() {
        let (mut p, id) = sixteen_step_song(3);
        write_automation(&mut p.song, id, 5, 5, 0.1);
        write_automation(&mut p.song, id, 20, 20, 0.2);
        p.song.loop_range = Some((1, 2));
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        insert_bars(&mut p.song, 1, &[Bar::default(), Bar::default()], s, d);
        assert_eq!(p.song.bars.len(), 5);
        let keys: Vec<u32> = lane(&p.song, id).unwrap().points.keys().copied().collect();
        assert_eq!(keys, vec![5, 52], "points from bar 1 on move by 32 steps");
        assert_eq!(p.song.loop_range, Some((3, 4)));
    }

    #[test]
    fn delete_removes_and_shifts_automation_and_clears_a_broken_loop() {
        let (mut p, id) = sixteen_step_song(4);
        write_automation(&mut p.song, id, 5, 5, 0.1);
        write_automation(&mut p.song, id, 20, 20, 0.2);
        write_automation(&mut p.song, id, 40, 40, 0.3);
        p.song.loop_range = Some((2, 3));
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        delete_bars(&mut p.song, 1, 1, s, d);
        assert_eq!(p.song.bars.len(), 3);
        let keys: Vec<u32> = lane(&p.song, id).unwrap().points.keys().copied().collect();
        assert_eq!(keys, vec![5, 24]);
        assert_eq!(
            p.song.loop_range,
            Some((1, 2)),
            "loop after the cut moves back"
        );
        delete_bars(&mut p.song, 2, 2, s, d);
        assert_eq!(p.song.loop_range, None, "loop endpoint deleted");
    }

    #[test]
    fn duplicate_copies_bars_and_their_automation() {
        let (mut p, id) = sixteen_step_song(2);
        p.song.bars[1].mutes[2] = true;
        write_automation(&mut p.song, id, 18, 18, 0.6);
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        duplicate_bars(&mut p.song, 1, 1, s, d);
        assert_eq!(p.song.bars.len(), 3);
        assert!(p.song.bars[2].mutes[2]);
        let keys: Vec<u32> = lane(&p.song, id).unwrap().points.keys().copied().collect();
        assert_eq!(keys, vec![18, 34]);
    }

    #[test]
    fn duplicate_at_the_cap_copies_only_the_inserted_bars_automation() {
        let (mut p, id) = sixteen_step_song(MAX_BARS - 1);
        write_automation(&mut p.song, id, 5, 5, 0.1);
        write_automation(&mut p.song, id, 20, 20, 0.2);
        write_automation(&mut p.song, id, 36, 36, 0.3);
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        duplicate_bars(&mut p.song, 0, 1, s, d);
        assert_eq!(p.song.bars.len(), MAX_BARS, "only bar 0 fit");
        let points: Vec<(u32, f32)> = lane(&p.song, id)
            .unwrap()
            .points
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        assert_eq!(
            points,
            vec![(5, 0.1), (20, 0.2), (37, 0.1), (52, 0.3)],
            "bar 1's point is not copied over the shifted bar 2"
        );
    }

    #[test]
    fn non_finite_automation_is_ignored() {
        let mut song = Song::default();
        let id = ParamId::Master(MasterParam::Level);
        write_automation(&mut song, id, 0, 3, f32::NAN);
        write_automation(&mut song, id, 0, 3, f32::INFINITY);
        assert!(song.automation.is_empty());
    }

    #[test]
    fn out_of_range_pattern_refs_are_ignored() {
        let mut song = Song::default();
        song.bars = vec![bar([Some(0); 4]); 2];
        let before = song.clone();
        write_pattern_ref(&mut song, 0, 4, 1);
        write_pattern_ref(&mut song, 0, 0, PATTERNS as u8);
        assert_eq!(song, before);
    }

    #[test]
    fn deleting_all_of_a_lanes_points_removes_the_lane() {
        let (mut p, id) = sixteen_step_song(3);
        let other = ParamId::Synth(0, SynthParam::Cutoff);
        write_automation(&mut p.song, id, 18, 20, 0.5);
        write_automation(&mut p.song, other, 40, 40, 0.5);
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        delete_bars(&mut p.song, 1, 1, s, d);
        assert!(lane(&p.song, id).is_none(), "empty lane pruned");
        assert_eq!(p.song.automation.len(), 1);
        assert!(is_automated(&p.song, other));
    }

    #[test]
    fn bar_count_is_capped() {
        let (mut p, _) = sixteen_step_song(MAX_BARS - 1);
        let snapshot = p.clone();
        let (s, d) = banks(&snapshot);
        insert_bars(&mut p.song, 0, &[Bar::default(); 5], s, d);
        assert_eq!(p.song.bars.len(), MAX_BARS);
    }
}
