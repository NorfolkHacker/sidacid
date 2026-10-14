//! Pure song helpers shared by the sequencer, offline render and the UI:
//! bar lengths, global step positions, automation lookup and editing.

use crate::machine::{MACHINES, MachineId, MachineKind, NOTE_MACHINES, kind_of};
use crate::params::ParamId;
use crate::project::{
    AutoLane, Bar, Cell, DrumPattern, NotePattern, PATTERNS, PatternRef, Project, STEPS, Song,
    SynthPattern, steps_of,
};
use std::collections::BTreeMap;

pub const MAX_BARS: usize = 999;
pub const DEFAULT_BAR_STEPS: usize = 16;

/// Every machine's pattern bank, borrowed. Cheap to copy.
#[derive(Clone, Copy)]
pub struct Banks<'a> {
    pub synths: [&'a [SynthPattern]; 2],
    pub drums: [&'a [DrumPattern]; 2],
    pub notes: [&'a [NotePattern]; NOTE_MACHINES],
}

impl<'a> Banks<'a> {
    pub fn new(
        synths: &'a [Vec<SynthPattern>; 2],
        drums: &'a [Vec<DrumPattern>; 2],
        notes: &'a [Vec<NotePattern>; NOTE_MACHINES],
    ) -> Self {
        Self {
            synths: [&synths[0], &synths[1]],
            drums: [&drums[0], &drums[1]],
            notes: [&notes[0], &notes[1]],
        }
    }

    /// The standard six machines' banks; a missing machine has an empty bank.
    pub fn of(p: &'a Project) -> Self {
        let synth = |m: MachineId| p.synth(m.uid()).map_or(&[][..], |s| &s.patterns[..]);
        let kit = |m: MachineId| p.kit(m.uid()).map_or(&[][..], |k| &k.patterns[..]);
        let notes = |m: MachineId| p.note_patterns(m.uid()).map_or(&[][..], |n| &n[..]);
        Self {
            synths: [synth(MachineId::Synth1), synth(MachineId::Synth2)],
            drums: [kit(MachineId::Kit808), kit(MachineId::Kit909)],
            notes: [notes(MachineId::Fm), notes(MachineId::Sampler)],
        }
    }

    /// Patterns in `machine`'s bank; 0 for an invalid machine.
    pub fn bank_len(&self, machine: usize) -> usize {
        match kind_of(machine) {
            Some(MachineKind::Synth(s)) => self.synths[s].len(),
            Some(MachineKind::Drum(k)) => self.drums[k].len(),
            Some(MachineKind::Notes(n)) => self.notes[n].len(),
            None => 0,
        }
    }

    /// Length in steps (1..=STEPS) of `machine`'s pattern `slot`.
    pub fn length(&self, machine: usize, slot: u8) -> Option<usize> {
        let s = slot as usize;
        let length = match kind_of(machine)? {
            MachineKind::Synth(i) => self.synths[i].get(s)?.length,
            MachineKind::Drum(i) => self.drums[i].get(s)?.length,
            MachineKind::Notes(i) => self.notes[i].get(s)?.length,
        };
        Some(steps_of(length))
    }
}

/// Machine `m`'s (by `MachineId::index`) cell in `bar`.
fn cell_of(bar: &Bar, m: usize) -> Cell {
    MachineId::from_index(m).map_or_else(Cell::default, |id| bar.cell(id.uid()))
}

/// Steps in a bar: its longest assigned pattern, or 16 when none is assigned.
pub fn bar_steps(bar: &Bar, banks: Banks<'_>) -> usize {
    let longest = (0..MACHINES)
        .filter_map(|m| banks.length(m, cell_of(bar, m).pattern?.0))
        .max()
        .unwrap_or(0);
    if longest == 0 {
        DEFAULT_BAR_STEPS
    } else {
        longest
    }
}

/// Global step at which `bar` starts (`bar == bars.len()` gives the song length).
pub fn bar_start_step(song: &Song, bar: usize, banks: Banks<'_>) -> u32 {
    song.bars
        .iter()
        .take(bar)
        .map(|b| bar_steps(b, banks) as u32)
        .sum()
}

pub fn total_steps(song: &Song, banks: Banks<'_>) -> u32 {
    bar_start_step(song, song.bars.len(), banks)
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

/// Steps in the longest possible song: `MAX_BARS` bars of `STEPS` steps.
pub const MAX_SONG_STEPS: usize = MAX_BARS * STEPS;

/// Writes `value` at every step from `from` to `to` (inclusive, either order)
/// in `param`'s lane, creating the lane if needed. A non-finite value is
/// ignored. At most `MAX_SONG_STEPS` points are written: a wilder range
/// stops after that many steps from its low end, and true is returned so
/// the caller can say the write was cut short.
pub fn write_automation(song: &mut Song, param: ParamId, from: u32, to: u32, value: f32) -> bool {
    if !value.is_finite() {
        return false;
    }
    let value = value.clamp(0.0, 1.0);
    let a = from.min(to);
    let last = a.saturating_add(MAX_SONG_STEPS as u32 - 1);
    let cut = from.max(to) > last;
    let b = from.max(to).min(last);
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
    cut
}

pub fn clear_automation(song: &mut Song, param: ParamId) {
    song.automation.retain(|l| l.param != param);
}

/// A pattern change made while recording: sets `machine` to `slot` in `bar`
/// and in the following bars that still held the bar's old reference.
/// Out-of-range machines and slots are ignored.
pub fn write_pattern_ref(song: &mut Song, bar: usize, machine: usize, slot: u8) {
    if machine >= MACHINES || slot as usize >= PATTERNS {
        return;
    }
    let Some(uid) = MachineId::from_index(machine).map(MachineId::uid) else {
        return;
    };
    let Some(old) = song.bars.get(bar).map(|b| b.cell(uid).pattern) else {
        return;
    };
    for b in bar..song.bars.len() {
        if b > bar && song.bars[b].cell(uid).pattern != old {
            break;
        }
        song.bars[b].cell_mut(uid).pattern = Some(PatternRef(slot));
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

/// Inserts `bars` before bar `at` (clamped to the song end), moving later
/// automation along. The loop range follows: inserting before or at the
/// loop's first bar shifts the loop; inserting inside it, or right after
/// its last bar, extends it to include the new bars; inserting further on
/// leaves it alone. Paste and Duplicate go through here too.
pub fn insert_bars(song: &mut Song, at: usize, bars: &[Bar], banks: Banks<'_>) {
    let at = at.min(song.bars.len());
    let room = MAX_BARS.saturating_sub(song.bars.len());
    let bars = &bars[..bars.len().min(room)];
    if bars.is_empty() {
        return;
    }
    let start = bar_start_step(song, at, banks);
    let added: u32 = bars.iter().map(|b| bar_steps(b, banks) as u32).sum();
    shift_automation(song, start, added as i64);
    song.bars.splice(at..at, bars.iter().cloned());
    if let Some((a, b)) = song.loop_range {
        let n = bars.len() as u16;
        // a stray range past the bars can't overflow or leave the bar cap
        let shift = |i: u16| i.saturating_add(n).min((MAX_BARS - 1) as u16);
        let (a_at, b_at) = (a as usize, b as usize);
        song.loop_range = Some(if at <= a_at {
            (shift(a), shift(b))
        } else if at <= b_at + 1 {
            (a, shift(b))
        } else {
            (a, b)
        });
    }
}

pub fn delete_bars(song: &mut Song, first: usize, last: usize, banks: Banks<'_>) {
    if song.bars.is_empty() {
        return;
    }
    let last = last.min(song.bars.len() - 1);
    if first > last {
        return;
    }
    let start = bar_start_step(song, first, banks);
    let end = bar_start_step(song, last + 1, banks);
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

pub fn duplicate_bars(song: &mut Song, first: usize, last: usize, banks: Banks<'_>) {
    if first > last || last >= song.bars.len() {
        return;
    }
    let copy: Vec<Bar> = song.bars[first..=last].to_vec();
    let src_start = bar_start_step(song, first, banks);
    let src_end = bar_start_step(song, last + 1, banks);
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
    insert_bars(song, last + 1, &copy, banks);
    let inserted = song.bars.len() - before;
    if inserted == 0 {
        return;
    }
    // At the bar cap only the first `inserted` bars were copied: their
    // automation ends where the truncated copy ends.
    let span = bar_start_step(song, first + inserted, banks) - src_start;
    let dst = bar_start_step(song, last + 1, banks);
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
    use crate::params::{MasterParam, SynthParam, synth_param};
    use crate::project::{DrumPattern, Project, SynthPattern};

    fn project_with_lengths(synth0: u8, drum0: u8) -> Project {
        let mut p = Project::default();
        p.synth_mut(MachineId::Synth1.uid()).unwrap().patterns[1] = SynthPattern {
            length: synth0,
            ..SynthPattern::default()
        };
        p.kit_mut(MachineId::Kit808.uid()).unwrap().patterns[2] = DrumPattern {
            length: drum0,
            ..DrumPattern::default()
        };
        p
    }

    fn bar(refs: [Option<u8>; 4]) -> Bar {
        let mut bar = Bar::default();
        for (m, r) in refs.into_iter().enumerate() {
            bar.cell_mut(MachineId::ALL[m].uid()).pattern = r.map(PatternRef);
        }
        bar
    }

    #[test]
    fn bar_length_is_the_longest_assigned_pattern() {
        let p = project_with_lengths(32, 12);
        let b = Banks::of(&p);
        assert_eq!(bar_steps(&bar([None; 4]), b), DEFAULT_BAR_STEPS);
        assert_eq!(bar_steps(&bar([Some(1), None, Some(2), None]), b), 32);
        assert_eq!(bar_steps(&bar([None, None, Some(2), None]), b), 12);
    }

    #[test]
    fn start_steps_accumulate_bar_lengths() {
        let mut p = project_with_lengths(32, 16);
        p.song.bars = vec![
            bar([None; 4]),
            bar([Some(1), None, None, None]),
            bar([None; 4]),
        ];
        let b = Banks::of(&p);
        assert_eq!(bar_start_step(&p.song, 0, b), 0);
        assert_eq!(bar_start_step(&p.song, 1, b), 16);
        assert_eq!(bar_start_step(&p.song, 2, b), 48);
        assert_eq!(total_steps(&p.song, b), 64);
    }

    #[test]
    fn value_at_holds_the_last_point() {
        let mut song = Song::default();
        let id = synth_param(0, SynthParam::Cutoff);
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
        let refs: Vec<_> = song
            .bars
            .iter()
            .map(|b| b.cell(MachineId::Synth1.uid()).pattern.unwrap().0)
            .collect();
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
        let b = Banks::of(&snapshot);
        insert_bars(&mut p.song, 1, &[Bar::default(), Bar::default()], b);
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
        let b = Banks::of(&snapshot);
        delete_bars(&mut p.song, 1, 1, b);
        assert_eq!(p.song.bars.len(), 3);
        let keys: Vec<u32> = lane(&p.song, id).unwrap().points.keys().copied().collect();
        assert_eq!(keys, vec![5, 24]);
        assert_eq!(
            p.song.loop_range,
            Some((1, 2)),
            "loop after the cut moves back"
        );
        delete_bars(&mut p.song, 2, 2, b);
        assert_eq!(p.song.loop_range, None, "loop endpoint deleted");
    }

    #[test]
    fn duplicate_copies_bars_and_their_automation() {
        let (mut p, id) = sixteen_step_song(2);
        p.song.bars[1].cell_mut(MachineId::Kit808.uid()).mute = true;
        write_automation(&mut p.song, id, 18, 18, 0.6);
        let snapshot = p.clone();
        let b = Banks::of(&snapshot);
        duplicate_bars(&mut p.song, 1, 1, b);
        assert_eq!(p.song.bars.len(), 3);
        assert!(p.song.bars[2].cell(MachineId::Kit808.uid()).mute);
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
        let b = Banks::of(&snapshot);
        duplicate_bars(&mut p.song, 0, 1, b);
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
        write_pattern_ref(&mut song, 0, MACHINES, 1);
        write_pattern_ref(&mut song, 0, 0, PATTERNS as u8);
        assert_eq!(song, before);
    }

    #[test]
    fn deleting_all_of_a_lanes_points_removes_the_lane() {
        let (mut p, id) = sixteen_step_song(3);
        let other = synth_param(0, SynthParam::Cutoff);
        write_automation(&mut p.song, id, 18, 20, 0.5);
        write_automation(&mut p.song, other, 40, 40, 0.5);
        let snapshot = p.clone();
        let b = Banks::of(&snapshot);
        delete_bars(&mut p.song, 1, 1, b);
        assert!(lane(&p.song, id).is_none(), "empty lane pruned");
        assert_eq!(p.song.automation.len(), 1);
        assert!(is_automated(&p.song, other));
    }

    #[test]
    fn note_patterns_count_toward_bar_length() {
        let mut p = Project::default();
        p.sampler_mut(MachineId::Sampler.uid()).unwrap().patterns[3].length = 48;
        let mut b = Bar::default();
        b.cell_mut(MachineId::Sampler.uid()).pattern = Some(PatternRef(3));
        assert_eq!(bar_steps(&b, Banks::of(&p)), 48);
        assert_eq!(Banks::of(&p).length(5, 3), Some(48));
        assert_eq!(Banks::of(&p).length(6, 0), None);
        assert_eq!(Banks::of(&p).bank_len(4), PATTERNS);
    }

    #[test]
    fn recorded_pattern_change_reaches_the_sampler() {
        let mut song = Song {
            bars: vec![Bar::default(); 3],
            ..Default::default()
        };
        write_pattern_ref(&mut song, 1, 5, 7);
        assert_eq!(
            song.bars[1].cell(MachineId::Sampler.uid()).pattern,
            Some(PatternRef(7))
        );
        assert_eq!(
            song.bars[2].cell(MachineId::Sampler.uid()).pattern,
            Some(PatternRef(7))
        );
        write_pattern_ref(&mut song, 0, 6, 7);
        assert!(song.bars[0].cells.iter().all(|(_, c)| c.pattern.is_none()));
    }

    #[test]
    fn write_automation_caps_a_runaway_range() {
        let mut song = Song::default();
        let id = ParamId::Master(MasterParam::Level);
        let cap = MAX_BARS * STEPS;
        write_automation(&mut song, id, 0, cap as u32 + 1000, 0.5);
        assert_eq!(lane(&song, id).unwrap().points.len(), cap);
        clear_automation(&mut song, id);
        write_automation(&mut song, id, 0, u32::MAX, 0.5);
        assert_eq!(lane(&song, id).unwrap().points.len(), cap, "no 4G fill");
        clear_automation(&mut song, id);
        write_automation(&mut song, id, u32::MAX, u32::MAX - 3, 0.5);
        assert_eq!(
            lane(&song, id).unwrap().points.len(),
            4,
            "no overflow at the top"
        );
    }

    #[test]
    fn write_automation_reports_a_cut_range() {
        let mut song = Song::default();
        let id = ParamId::Master(MasterParam::Level);
        let last = MAX_SONG_STEPS as u32 - 1;
        assert!(!write_automation(&mut song, id, 0, last, 0.5), "fits");
        assert!(
            !write_automation(&mut song, id, last, 0, 0.5),
            "either order"
        );
        assert!(write_automation(&mut song, id, 0, last + 1, 0.5));
        assert!(write_automation(&mut song, id, u32::MAX, 0, 0.5));
        assert!(
            !write_automation(&mut song, id, 0, u32::MAX, f32::NAN),
            "ignored, not cut"
        );
    }

    #[test]
    fn bar_steps_ignores_bad_slots_and_clamps_lengths() {
        let mut p = Project::default();
        p.synth_mut(MachineId::Synth1.uid()).unwrap().patterns[1].length = 0;
        p.kit_mut(MachineId::Kit808.uid()).unwrap().patterns[2].length = 255;
        let b = Banks::of(&p);
        assert_eq!(b.length(0, 200), None, "slot past the bank");
        assert_eq!(b.length(0, 1), Some(1), "length 0 counts as 1");
        assert_eq!(b.length(2, 2), Some(STEPS), "length past STEPS is clamped");
        assert_eq!(
            bar_steps(&bar([Some(200), None, None, None]), b),
            DEFAULT_BAR_STEPS
        );
        assert_eq!(bar_steps(&bar([Some(1), None, None, None]), b), 1);
        assert_eq!(bar_steps(&bar([Some(1), None, Some(2), None]), b), STEPS);
    }

    #[test]
    fn inserting_inside_or_at_the_end_of_the_loop_extends_it() {
        let (p, _) = sixteen_step_song(5);
        let b = Banks::of(&p);
        let insert_at = |at: usize| {
            let mut song = p.song.clone();
            song.loop_range = Some((1, 3));
            insert_bars(&mut song, at, &[Bar::default()], b);
            song.loop_range
        };
        assert_eq!(insert_at(0), Some((2, 4)), "before the loop: shifts");
        assert_eq!(insert_at(1), Some((2, 4)), "at the loop start: shifts");
        assert_eq!(insert_at(2), Some((1, 4)), "inside: extends");
        assert_eq!(insert_at(3), Some((1, 4)), "before the last bar: extends");
        assert_eq!(insert_at(4), Some((1, 4)), "right after the end: extends");
        assert_eq!(insert_at(5), Some((1, 3)), "past the end: unchanged");
    }

    #[test]
    fn bar_count_is_capped() {
        let (mut p, _) = sixteen_step_song(MAX_BARS - 1);
        let snapshot = p.clone();
        let b = Banks::of(&snapshot);
        insert_bars(&mut p.song, 0, &vec![Bar::default(); 5], b);
        assert_eq!(p.song.bars.len(), MAX_BARS);
    }

    #[test]
    fn insert_before_an_extreme_loop_range_saturates() {
        let (p, _) = sixteen_step_song(3);
        let b = Banks::of(&p);
        let insert = |range: (u16, u16), at: usize| {
            let mut song = p.song.clone();
            song.loop_range = Some(range);
            insert_bars(&mut song, at, &vec![Bar::default(); 2], b);
            song.loop_range
        };
        let top = (MAX_BARS - 1) as u16;
        assert_eq!(insert((u16::MAX - 1, u16::MAX), 0), Some((top, top)));
        assert_eq!(insert((0, u16::MAX), 1), Some((0, top)), "extends");
        assert_eq!(
            insert((1, 2), 0),
            Some((3, 4)),
            "a normal loop still shifts"
        );
    }
}
