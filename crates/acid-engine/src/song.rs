//! Pure song helpers shared by the sequencer, offline render and the UI:
//! block lanes (ends, lookup, placement without overlap, ripple, time
//! insert/delete, recorded pattern changes) and automation.

use crate::machine::{MAX_MACHINES, MachineType, MachineUid};
use crate::params::ParamId;
use crate::project::{
    AutoLane, Block, DrumPattern, MachineEntry, NotePattern, PATTERNS, PatternRef, Project, STEPS,
    Song, SynthPattern, steps_of,
};
use crate::rack::{CompiledSong, Slot, SlotPatterns};
use std::collections::BTreeMap;

/// Bars in the longest song (with `STEPS` steps each, `MAX_SONG_STEPS`).
pub const MAX_BARS: usize = 999;

/// One machine's pattern bank, borrowed, by what plays it.
#[derive(Clone, Copy)]
pub enum Bank<'a> {
    Synth(&'a [SynthPattern]),
    Drum(&'a [DrumPattern]),
    Notes(&'a [NotePattern]),
}

impl<'a> Bank<'a> {
    pub fn len(&self) -> usize {
        match self {
            Bank::Synth(b) => b.len(),
            Bank::Drum(b) => b.len(),
            Bank::Notes(b) => b.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Length in steps (1..=STEPS) of pattern `slot`.
    pub fn length(&self, slot: u8) -> Option<usize> {
        let s = slot as usize;
        let length = match self {
            Bank::Synth(b) => b.get(s)?.roll.length,
            Bank::Drum(b) => b.get(s)?.roll.length,
            Bank::Notes(b) => b.get(s)?.length,
        };
        Some(steps_of(length))
    }

    fn of_slot(p: &'a SlotPatterns) -> Self {
        match p {
            SlotPatterns::Synth(v) => Bank::Synth(v),
            SlotPatterns::Drum(v) => Bank::Drum(v),
            SlotPatterns::Notes(v) => Bank::Notes(v),
        }
    }
}

/// Every rack position's pattern bank, borrowed, with the machine there.
/// Cheap to copy. Positions past the rack hold `None`.
#[derive(Clone, Copy)]
pub struct Banks<'a> {
    pub slots: [Option<Bank<'a>>; MAX_MACHINES],
    /// The machine at each position: song lanes are keyed by its uid, and
    /// its type says whether the song transpose applies.
    pub machines: [Option<(MachineUid, MachineType)>; MAX_MACHINES],
}

impl<'a> Banks<'a> {
    /// No machines.
    pub fn empty() -> Self {
        Self {
            slots: [None; MAX_MACHINES],
            machines: [None; MAX_MACHINES],
        }
    }

    /// The engine's rack, in order.
    pub fn of_slots(slots: &'a [Slot]) -> Self {
        let mut b = Self::empty();
        for (i, s) in slots.iter().take(MAX_MACHINES).enumerate() {
            b.slots[i] = Some(Bank::of_slot(&s.patterns));
            b.machines[i] = Some((s.uid, s.ty));
        }
        b
    }

    /// The project's rack, in order.
    pub fn of(p: &'a Project) -> Self {
        Self::of_machines(&p.machines)
    }

    /// A rack of machine entries, in order.
    pub fn of_machines(machines: &'a [MachineEntry]) -> Self {
        use crate::project::MachineBody;
        let mut b = Self::empty();
        for (i, m) in machines.iter().take(MAX_MACHINES).enumerate() {
            b.slots[i] = Some(match &m.body {
                MachineBody::Synth(x) => Bank::Synth(&x.patterns),
                MachineBody::Kit(x) => Bank::Drum(&x.patterns),
                MachineBody::Fm(x) => Bank::Notes(&x.patterns),
                MachineBody::Sampler(x) => Bank::Notes(&x.patterns),
                MachineBody::Mono(x) => Bank::Notes(&x.patterns),
            });
            b.machines[i] = Some((m.uid, m.machine_type()));
        }
        b
    }

    /// Patterns in the bank at rack position `pos`; 0 for an empty position.
    pub fn bank_len(&self, pos: usize) -> usize {
        self.slots
            .get(pos)
            .copied()
            .flatten()
            .map_or(0, |b| b.len())
    }

    /// Length in steps (1..=STEPS) of pattern `slot` at rack position `pos`.
    pub fn length(&self, pos: usize, slot: u8) -> Option<usize> {
        self.slots.get(pos).copied().flatten()?.length(slot)
    }

    /// Whether the song transpose applies at rack position `pos`.
    pub fn transposes(&self, pos: usize) -> bool {
        self.machines
            .get(pos)
            .copied()
            .flatten()
            .is_some_and(|(_, ty)| ty.transposes())
    }
}

/// `block_end` of a block on the lane at rack position `pos` of a
/// compiled song, with lengths from the engine's own banks (real-time
/// safe).
pub fn compiled_block_end(b: &Block, pos: usize, banks: Banks<'_>) -> u32 {
    let len = b.len.map_or_else(
        || {
            banks
                .length(pos, b.pattern.0)
                .map_or(DEFAULT_PATTERN_STEPS, |n| n as u32)
        },
        u32::from,
    );
    b.start.saturating_add(len)
}

/// `song_end` of a compiled song (real-time safe).
pub fn compiled_song_end(song: &CompiledSong, banks: Banks<'_>) -> u32 {
    song.lanes
        .iter()
        .enumerate()
        .filter_map(|(pos, lane)| Some(compiled_block_end(lane.last()?, pos, banks)))
        .fold(song.end.unwrap_or(0), u32::max)
}

/// `block_at` on the lane at rack position `pos` of a compiled song
/// (real-time safe).
pub fn compiled_block_at(
    song: &CompiledSong,
    pos: usize,
    step: u32,
    banks: Banks<'_>,
) -> Option<usize> {
    let lane = song.lanes.get(pos)?;
    let i = lane.partition_point(|b| b.start <= step).checked_sub(1)?;
    (step < compiled_block_end(&lane[i], pos, banks)).then_some(i)
}

/// Steps of pattern `slot` on machine `uid`: what a block lasts when it
/// has no length of its own.
pub type Lens<'a> = &'a dyn Fn(MachineUid, u8) -> u32;

/// `Lens` over the project's machines: the steps of that machine's
/// pattern, 16 when the machine or pattern is missing.
pub fn lens_of(p: &Project) -> impl Fn(MachineUid, u8) -> u32 {
    let banks = Banks::of(p);
    move |uid, slot| {
        banks
            .machines
            .iter()
            .position(|m| m.is_some_and(|(u, _)| u == uid))
            .and_then(|pos| banks.length(pos, slot))
            .map_or(DEFAULT_PATTERN_STEPS, |n| n as u32)
    }
}

/// A missing pattern's length.
const DEFAULT_PATTERN_STEPS: u32 = 16;

/// The step after `b`'s last.
pub fn block_end(b: &Block, uid: MachineUid, lens: Lens<'_>) -> u32 {
    let len = b.len.map_or_else(|| lens(uid, b.pattern.0), u32::from);
    b.start.saturating_add(len)
}

/// Where the song ends: its last block's end, or `song.end` if later.
pub fn song_end(song: &Song, lens: Lens<'_>) -> u32 {
    song.lanes
        .iter()
        .flat_map(|(uid, blocks)| blocks.iter().map(|b| block_end(b, *uid, lens)))
        .fold(song.end.unwrap_or(0), u32::max)
}

/// The block of a sorted lane that covers `step`.
pub fn block_at(blocks: &[Block], uid: MachineUid, step: u32, lens: Lens<'_>) -> Option<usize> {
    let i = blocks.partition_point(|b| b.start <= step).checked_sub(1)?;
    (step < block_end(&blocks[i], uid, lens)).then_some(i)
}

/// Puts `block` on a sorted lane at its start if that's free, else at the
/// nearest free start on the step grid, searching both ways (later wins
/// a tie). Returns its index, or `None` (lane untouched) when it fits
/// nowhere before `MAX_SONG_STEPS`.
pub fn place(
    blocks: &mut Vec<Block>,
    uid: MachineUid,
    block: Block,
    lens: Lens<'_>,
) -> Option<usize> {
    let len = block_end(&Block { start: 0, ..block }, uid, lens);
    let max = MAX_SONG_STEPS as u32;
    if len == 0 || len > max {
        return None;
    }
    let want = block.start;
    // Each gap [from, to) between blocks, the last up to the limit.
    let mut best: Option<u32> = None;
    let mut from = 0;
    for i in 0..=blocks.len() {
        let to = blocks.get(i).map_or(max, |b| b.start.min(max));
        if to >= from && to - from >= len {
            let start = want.clamp(from, to - len);
            let better = best.is_none_or(|b| {
                let (d, e) = (start.abs_diff(want), b.abs_diff(want));
                d < e || (d == e && start > b)
            });
            if better {
                best = Some(start);
            }
        }
        if let Some(b) = blocks.get(i) {
            from = from.max(block_end(b, uid, lens));
        }
    }
    let start = best?;
    let i = blocks.partition_point(|b| b.start < start);
    blocks.insert(i, Block { start, ..block });
    Some(i)
}

/// Pattern `slot` grew from `old_len` to `new_len` steps: each of its
/// `len: None` blocks grows, and later blocks move right only as far as
/// needed to clear the block in front (a gap absorbs growth first),
/// cascading down the lane. Shrinking leaves gaps; `Some` blocks never
/// ripple. Other blocks' lengths come from `lens`.
pub fn ripple(
    blocks: &mut [Block],
    uid: MachineUid,
    slot: u8,
    old_len: u32,
    new_len: u32,
    lens: Lens<'_>,
) {
    if new_len <= old_len {
        return;
    }
    let mut front = 0;
    for b in blocks.iter_mut() {
        b.start = b.start.max(front);
        front = if b.len.is_none() && b.pattern.0 == slot {
            b.start.saturating_add(new_len)
        } else {
            block_end(b, uid, lens)
        };
    }
}

/// Inserts `n` steps at step `at` on every lane: blocks and automation
/// points at or after it move right, as do the song's end and the loop
/// (inserting at or before the loop start shifts it, inside it or right
/// at its end extends it). At most the room left before
/// `MAX_SONG_STEPS` is inserted.
pub fn insert_time(song: &mut Song, at: u32, n: u32, lens: Lens<'_>) {
    let max = MAX_SONG_STEPS as u32;
    let n = n.min(max.saturating_sub(song_end(song, lens)));
    if n == 0 {
        return;
    }
    for (_, blocks) in &mut song.lanes {
        for b in blocks.iter_mut().filter(|b| b.start >= at) {
            b.start += n;
        }
    }
    shift_automation(song, at, n as i64);
    if let Some(e) = &mut song.end
        && *e >= at
    {
        *e = e.saturating_add(n).min(max);
    }
    if let Some((a, b)) = song.loop_range {
        let shift = |i: u32| i.saturating_add(n).min(max);
        song.loop_range = Some(if at <= a {
            (shift(a), shift(b))
        } else if at <= b {
            (a, shift(b))
        } else {
            (a, b)
        });
    }
}

/// Removes steps `a..b` from every lane and closes the gap: blocks
/// starting inside go, a block running into `a` is cut to end there,
/// later blocks and automation move left, points inside go (and lanes
/// left empty), the song's end follows, and a loop whose start or last
/// step is inside is cleared.
pub fn delete_time(song: &mut Song, a: u32, b: u32, lens: Lens<'_>) {
    if a >= b {
        return;
    }
    let d = b - a;
    for (uid, blocks) in &mut song.lanes {
        blocks.retain(|x| x.start < a || x.start >= b);
        for x in blocks.iter_mut() {
            if x.start >= b {
                x.start -= d;
            } else if block_end(x, *uid, lens) > a {
                x.len = Some((a - x.start) as u16);
            }
        }
    }
    for lane in &mut song.automation {
        lane.points.retain(|k, _| *k < a || *k >= b);
    }
    shift_automation(song, b, -(d as i64));
    song.automation.retain(|l| !l.points.is_empty());
    if let Some(e) = &mut song.end {
        *e = if *e >= b { *e - d } else { (*e).min(a) };
    }
    let inside = |i: u32| (a..b).contains(&i);
    let map = |i: u32| if i >= b { i - d } else { i };
    song.loop_range = song
        .loop_range
        .filter(|&(la, lb)| !inside(la) && !inside(lb.saturating_sub(1)))
        .map(|(la, lb)| (map(la), map(lb)));
}

/// A pattern change recorded at `step` on `uid`'s lane, at the start of
/// its 16-step bar. The block covering that is split there (the left
/// part keeps its pattern, fixed to its length); the right part and each
/// following block that touches the one before with the same old
/// pattern switch to `slot`, keeping their spans. In a gap, a new block
/// of `slot` goes there, cut to end before the next block. Out-of-range
/// slots are ignored.
pub fn record_pattern(song: &mut Song, uid: MachineUid, step: u32, slot: u8, lens: Lens<'_>) {
    let max = MAX_SONG_STEPS as u32;
    let at = step / 16 * 16;
    if slot as usize >= PATTERNS || at >= max {
        return;
    }
    let new = PatternRef(slot);
    let new_len = lens(uid, slot);
    let blocks = song.lane_mut(uid);
    let Some(mut i) = block_at(blocks, uid, at, lens) else {
        let i = blocks.partition_point(|b| b.start < at);
        let room = blocks.get(i).map_or(max, |b| b.start) - at;
        blocks.insert(
            i,
            Block {
                len: (new_len > room).then_some(room as u16),
                ..Block::new(at, new)
            },
        );
        return;
    };
    let old = blocks[i].pattern;
    if blocks[i].start < at {
        let end = block_end(&blocks[i], uid, lens);
        let right = Block {
            start: at,
            len: Some((end - at) as u16),
            ..blocks[i]
        };
        blocks[i].len = Some((at - blocks[i].start) as u16);
        i += 1;
        blocks.insert(i, right);
    }
    let mut end = blocks[i].start;
    while let Some(b) = blocks.get_mut(i)
        && b.start == end
        && b.pattern == old
    {
        end = block_end(b, uid, lens);
        let span = end - b.start;
        b.pattern = new;
        b.len = (span != new_len).then_some(span as u16);
        i += 1;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{MasterParam, SynthParam};

    const U: MachineUid = MachineUid(1);

    /// Slot 1 is 32 steps, slot 2 is 8, the rest 16.
    fn lens(_: MachineUid, slot: u8) -> u32 {
        match slot {
            1 => 32,
            2 => 8,
            _ => 16,
        }
    }

    fn b(start: u32, slot: u8) -> Block {
        Block::new(start, PatternRef(slot))
    }

    fn fixed(start: u32, slot: u8, len: u16) -> Block {
        Block {
            len: Some(len),
            ..b(start, slot)
        }
    }

    /// (start, slot, len) of each block.
    fn shape(blocks: &[Block]) -> Vec<(u32, u8, Option<u16>)> {
        blocks
            .iter()
            .map(|b| (b.start, b.pattern.0, b.len))
            .collect()
    }

    #[test]
    fn a_block_ends_after_its_pattern_or_its_own_length() {
        assert_eq!(block_end(&b(4, 1), U, &lens), 36);
        assert_eq!(block_end(&b(4, 0), U, &lens), 20);
        assert_eq!(block_end(&fixed(4, 1, 10), U, &lens), 14, "shorter");
        assert_eq!(block_end(&fixed(4, 2, 40), U, &lens), 44, "longer loops");
    }

    #[test]
    fn the_song_ends_at_its_last_block_or_its_end_if_later() {
        let mut song = Song::default();
        assert_eq!(song_end(&song, &lens), 0, "empty");
        song.lane_mut(U).extend([b(0, 0), b(16, 1)]);
        song.lane_mut(MachineUid(3)).push(fixed(8, 0, 24));
        assert_eq!(song_end(&song, &lens), 48);
        song.end = Some(64);
        assert_eq!(song_end(&song, &lens), 64, "trailing empty bars");
        song.end = Some(20);
        assert_eq!(song_end(&song, &lens), 48);
    }

    #[test]
    fn block_at_finds_the_covering_block() {
        let lane = [b(0, 0), b(32, 2)];
        let at = |step| block_at(&lane, U, step, &lens);
        assert_eq!(at(0), Some(0));
        assert_eq!(at(15), Some(0));
        assert_eq!(at(16), None, "a gap");
        assert_eq!(at(32), Some(1));
        assert_eq!(at(39), Some(1));
        assert_eq!(at(40), None, "past the end");
        assert_eq!(block_at(&[], U, 0, &lens), None);
    }

    #[test]
    fn place_takes_the_asked_start_when_free() {
        let mut lane = vec![b(0, 0), b(32, 0)];
        assert_eq!(place(&mut lane, U, b(16, 0), &lens), Some(1));
        assert_eq!(shape(&lane), [(0, 0, None), (16, 0, None), (32, 0, None)]);
    }

    #[test]
    fn place_moves_an_overlapping_block_to_the_nearest_free_start() {
        let mut lane = vec![b(16, 0)];
        assert_eq!(place(&mut lane, U, b(8, 0), &lens), Some(0), "back to 0");
        assert_eq!(lane[0].start, 0);
        let mut lane = vec![b(16, 0)];
        assert_eq!(place(&mut lane, U, b(28, 0), &lens), Some(1), "on to 32");
        assert_eq!(lane[1].start, 32);
        // An 8-step block at 20: 8 (12 back) or 32 (12 on); later wins.
        let mut lane = vec![b(16, 0)];
        assert_eq!(place(&mut lane, U, b(20, 2), &lens), Some(1));
        assert_eq!(shape(&lane), [(16, 0, None), (32, 2, None)]);
        // A gap too short is skipped.
        let mut lane = vec![b(0, 0), b(24, 0)];
        assert_eq!(place(&mut lane, U, b(16, 0), &lens), Some(2));
        assert_eq!(lane[2].start, 40);
        // Mute, transpose and length are kept.
        let mut lane = vec![b(0, 0)];
        let block = Block {
            mute: true,
            transpose: -3,
            ..fixed(4, 0, 6)
        };
        place(&mut lane, U, block, &lens);
        assert_eq!(lane[1], Block { start: 16, ..block });
    }

    #[test]
    fn place_refuses_a_block_that_fits_nowhere() {
        let max = MAX_SONG_STEPS as u32;
        let mut lane = vec![fixed(0, 0, max as u16)];
        assert_eq!(place(&mut lane, U, b(40, 0), &lens), None);
        assert_eq!(lane.len(), 1, "lane untouched");
        let mut lane = Vec::new();
        assert_eq!(place(&mut lane, U, b(max - 8, 0), &lens), Some(0));
        assert_eq!(lane[0].start, max - 16, "kept inside the song limit");
        let mut lane = Vec::new();
        assert_eq!(
            place(&mut lane, U, fixed(0, 0, 0), &lens),
            None,
            "no length"
        );
    }

    #[test]
    fn a_growing_pattern_pushes_later_blocks_right_in_a_cascade() {
        // Slot 0 grows from 16 to 24; the lens still says 16.
        let mut lane = vec![b(0, 0), b(16, 3), b(32, 3), b(64, 3)];
        ripple(&mut lane, U, 0, 16, 24, &lens);
        assert_eq!(
            shape(&lane),
            [(0, 0, None), (24, 3, None), (40, 3, None), (64, 3, None)],
            "the gap before 64 absorbs the rest"
        );
        let mut lane = vec![b(0, 0), b(16, 0), b(32, 3)];
        ripple(&mut lane, U, 0, 16, 24, &lens);
        assert_eq!(
            shape(&lane),
            [(0, 0, None), (24, 0, None), (48, 3, None)],
            "every block of the pattern grows"
        );
    }

    #[test]
    fn a_gap_absorbs_growth_first() {
        let mut lane = vec![b(0, 0), b(20, 3)];
        ripple(&mut lane, U, 0, 16, 20, &lens);
        assert_eq!(shape(&lane), [(0, 0, None), (20, 3, None)]);
        ripple(&mut lane, U, 0, 20, 22, &lens);
        assert_eq!(shape(&lane), [(0, 0, None), (22, 3, None)]);
    }

    #[test]
    fn shrinking_leaves_a_gap_and_fixed_blocks_never_ripple() {
        let mut lane = vec![b(0, 0), b(16, 3)];
        ripple(&mut lane, U, 0, 16, 8, &lens);
        assert_eq!(shape(&lane), [(0, 0, None), (16, 3, None)]);
        let mut lane = vec![fixed(0, 0, 16), b(16, 3)];
        ripple(&mut lane, U, 0, 16, 32, &lens);
        assert_eq!(shape(&lane), [(0, 0, Some(16)), (16, 3, None)]);
        let mut lane = vec![b(0, 5), b(16, 3)];
        ripple(&mut lane, U, 0, 16, 32, &lens);
        assert_eq!(shape(&lane), [(0, 5, None), (16, 3, None)], "other pattern");
    }

    fn three_block_song() -> (Song, ParamId) {
        let mut song = Song::default();
        song.lane_mut(U).extend([b(0, 0), b(16, 0), b(32, 0)]);
        song.lane_mut(MachineUid(3)).push(fixed(8, 0, 16));
        (song, ParamId::Master(MasterParam::Level))
    }

    fn keys(song: &Song, id: ParamId) -> Vec<u32> {
        lane(song, id).unwrap().points.keys().copied().collect()
    }

    #[test]
    fn insert_time_moves_later_blocks_automation_and_the_loop() {
        let (mut song, id) = three_block_song();
        write_automation(&mut song, id, 5, 5, 0.1);
        write_automation(&mut song, id, 16, 16, 0.2);
        write_automation(&mut song, id, 20, 20, 0.3);
        song.loop_range = Some((16, 48));
        song.end = Some(48);
        insert_time(&mut song, 16, 8, &lens);
        assert_eq!(
            shape(song.lane(U)),
            [(0, 0, None), (24, 0, None), (40, 0, None)]
        );
        assert_eq!(shape(song.lane(MachineUid(3))), [(8, 0, Some(16))]);
        assert_eq!(keys(&song, id), [5, 24, 28]);
        assert_eq!(song.loop_range, Some((24, 56)), "at the loop start: shifts");
        assert_eq!(song.end, Some(56));
        let loop_after = |at: u32| {
            let (mut song, _) = three_block_song();
            song.loop_range = Some((16, 48));
            insert_time(&mut song, at, 8, &lens);
            song.loop_range
        };
        assert_eq!(loop_after(0), Some((24, 56)), "before: shifts");
        assert_eq!(loop_after(32), Some((16, 56)), "inside: extends");
        assert_eq!(loop_after(48), Some((16, 56)), "right after: extends");
        assert_eq!(loop_after(49), Some((16, 48)), "past: unchanged");
    }

    #[test]
    fn insert_time_stops_at_the_song_length_limit() {
        let max = MAX_SONG_STEPS as u32;
        let mut song = Song::default();
        song.lane_mut(U).push(b(max - 32, 0));
        insert_time(&mut song, 0, 100, &lens);
        assert_eq!(song.lane(U)[0].start, max - 16, "only the room left");
    }

    #[test]
    fn delete_time_cuts_removes_and_closes_the_gap() {
        let mut song = Song::default();
        song.lane_mut(U).extend([b(0, 0), b(16, 0), b(40, 0)]);
        song.lane_mut(MachineUid(3)).push(fixed(4, 1, 40));
        let id = ParamId::Master(MasterParam::Level);
        let other = MachineUid(1).param(SynthParam::Cutoff);
        write_automation(&mut song, id, 4, 4, 0.1);
        write_automation(&mut song, id, 10, 10, 0.2);
        write_automation(&mut song, id, 30, 30, 0.3);
        write_automation(&mut song, other, 12, 20, 0.5);
        song.loop_range = Some((32, 48));
        song.end = Some(64);
        delete_time(&mut song, 8, 24, &lens);
        assert_eq!(
            shape(song.lane(U)),
            [(0, 0, Some(8)), (24, 0, None)],
            "cut at 8, the block at 16 removed, 40 moved to 24"
        );
        assert_eq!(shape(song.lane(MachineUid(3))), [(4, 1, Some(4))]);
        assert_eq!(keys(&song, id), [4, 14]);
        assert!(lane(&song, other).is_none(), "an emptied lane is pruned");
        assert_eq!(
            song.loop_range,
            Some((16, 32)),
            "a loop after the cut moves"
        );
        assert_eq!(song.end, Some(48));
        let loop_after = |range: (u32, u32)| {
            let (mut song, _) = three_block_song();
            song.loop_range = Some(range);
            delete_time(&mut song, 16, 32, &lens);
            song.loop_range
        };
        assert_eq!(loop_after((0, 16)), Some((0, 16)), "before: kept");
        assert_eq!(loop_after((0, 48)), Some((0, 32)), "around: shrinks");
        assert_eq!(loop_after((16, 32)), None, "inside: cleared");
        assert_eq!(loop_after((8, 24)), None, "its end deleted: cleared");
        assert_eq!(loop_after((24, 48)), None, "its start deleted: cleared");
    }

    #[test]
    fn a_recorded_pattern_change_splits_the_block_it_lands_in() {
        let mut song = Song::default();
        song.lane_mut(U)
            .extend([fixed(0, 0, 64), b(64, 0), b(80, 0), b(96, 5), b(112, 0)]);
        record_pattern(&mut song, U, 37, 9, &lens);
        assert_eq!(
            shape(song.lane(U)),
            [
                (0, 0, Some(32)),
                (32, 9, Some(32)),
                (64, 9, None),
                (80, 9, None),
                (96, 5, None),
                (112, 0, None),
            ],
            "split at bar 32; the change runs until another pattern"
        );
        // A switched block keeps its span when the patterns differ in length.
        let mut song = Song::default();
        song.lane_mut(U).extend([b(0, 0), b(16, 0)]);
        record_pattern(&mut song, U, 16, 1, &lens);
        assert_eq!(shape(song.lane(U)), [(0, 0, None), (16, 1, Some(16))]);
    }

    #[test]
    fn a_recorded_pattern_change_runs_through_following_blocks() {
        let mut song = Song::default();
        song.lane_mut(U).extend(
            [0, 0, 0, 5, 0]
                .iter()
                .enumerate()
                .map(|(i, &s)| b(i as u32 * 16, s)),
        );
        record_pattern(&mut song, U, 16, 9, &lens);
        let slots: Vec<u8> = song.lane(U).iter().map(|b| b.pattern.0).collect();
        assert_eq!(slots, [0, 9, 9, 5, 0]);
        // A gap ends the run too.
        let mut song = Song::default();
        song.lane_mut(U).extend([b(0, 0), b(32, 0)]);
        record_pattern(&mut song, U, 0, 9, &lens);
        let slots: Vec<u8> = song.lane(U).iter().map(|b| b.pattern.0).collect();
        assert_eq!(slots, [9, 0]);
    }

    #[test]
    fn a_recorded_pattern_change_in_a_gap_places_a_block_cut_to_fit() {
        let mut song = Song::default();
        record_pattern(&mut song, MachineUid(6), 20, 3, &lens);
        assert_eq!(shape(song.lane(MachineUid(6))), [(16, 3, None)]);
        let mut song = Song::default();
        song.lane_mut(U).push(b(40, 0));
        record_pattern(&mut song, U, 33, 1, &lens);
        assert_eq!(shape(song.lane(U)), [(32, 1, Some(8)), (40, 0, None)]);
        let before = song.clone();
        record_pattern(&mut song, U, 0, PATTERNS as u8, &lens);
        assert_eq!(song, before, "out-of-range slot ignored");
    }

    #[test]
    fn lens_of_reads_the_project_patterns() {
        let mut p = Project::default();
        p.sampler_mut(MachineUid(6)).unwrap().patterns[3].length = 48;
        let l = lens_of(&p);
        assert_eq!(l(MachineUid(6), 3), 48);
        assert_eq!(l(MachineUid(6), 0), 16);
        assert_eq!(l(MachineUid(42), 3), 16, "missing machine");
        assert_eq!(l(MachineUid(6), 200), 16, "missing pattern");
    }

    #[test]
    fn banks_read_lengths_and_ignore_bad_slots() {
        let mut p = Project::default();
        p.synth_mut(MachineUid(1)).unwrap().patterns[1].roll.length = 0;
        p.kit_mut(MachineUid(3)).unwrap().patterns[2].roll.length = 255;
        p.sampler_mut(MachineUid(6)).unwrap().patterns[3].length = 48;
        let b = Banks::of(&p);
        assert_eq!(b.length(0, 200), None, "slot past the bank");
        assert_eq!(b.length(0, 1), Some(1), "length 0 counts as 1");
        assert_eq!(b.length(2, 2), Some(STEPS), "length past STEPS is clamped");
        assert_eq!(b.length(5, 3), Some(48));
        assert_eq!(b.length(6, 0), None);
        assert_eq!(b.bank_len(4), PATTERNS);
    }

    #[test]
    fn compiled_lookups_match_the_project_ones() {
        let mut p = Project::default();
        p.synth_mut(MachineUid(2)).unwrap().patterns[1].roll.length = 32;
        p.song.lane_mut(MachineUid(2)).extend([b(0, 1), b(48, 0)]);
        p.song.lane_mut(MachineUid(5)).push(fixed(16, 0, 40));
        let c = CompiledSong::compile(&p.song, &p.rack_uids());
        let banks = Banks::of(&p);
        let l = lens_of(&p);
        assert_eq!(compiled_song_end(&c, banks), song_end(&p.song, &l));
        assert_eq!(compiled_song_end(&c, banks), 64);
        for step in [0, 31, 32, 47, 48, 63, 64] {
            assert_eq!(
                compiled_block_at(&c, 1, step, banks),
                block_at(p.song.lane(MachineUid(2)), MachineUid(2), step, &l),
                "step {step}"
            );
        }
        assert_eq!(compiled_block_at(&c, 4, 55, banks), Some(0));
        assert_eq!(compiled_block_at(&c, 4, 56, banks), None);
        assert_eq!(compiled_block_at(&c, 15, 0, banks), None, "past the rack");
    }

    #[test]
    fn value_at_holds_the_last_point() {
        let mut song = Song::default();
        let id = MachineUid(1).param(SynthParam::Cutoff);
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
    fn non_finite_automation_is_ignored() {
        let mut song = Song::default();
        let id = ParamId::Master(MasterParam::Level);
        write_automation(&mut song, id, 0, 3, f32::NAN);
        write_automation(&mut song, id, 0, 3, f32::INFINITY);
        assert!(song.automation.is_empty());
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
}
