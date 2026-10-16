//! Pure operations on a block selection (indices into one machine's sorted
//! song lane), shared by the dock's Song mode: pick, move, resize,
//! delete, copy, paste and duplicate. None of them leaves two blocks of
//! the lane overlapping: blocks that move or are pasted together keep
//! their spacing and go by one offset, the nearest to the one asked for
//! at which all of them fit (`group_offset`); an edit with no such offset
//! is refused whole.

use acid_engine::machine::MachineUid;
use acid_engine::project::Block;
use acid_engine::song::{self, Lens, MAX_SONG_STEPS};

/// Steps in a bar: what Song-mode moves and lengths snap to.
pub const BAR: u32 = 16;

/// Indices of the blocks equal to the selected values; each selected
/// value claims the first unclaimed equal block. Sorted, unique.
pub fn resolve(blocks: &[Block], selected: &[Block]) -> Vec<usize> {
    let mut used = vec![false; blocks.len()];
    for s in selected {
        if let Some(i) = (0..blocks.len()).find(|&i| !used[i] && blocks[i] == *s) {
            used[i] = true;
        }
    }
    (0..used.len()).filter(|&i| used[i]).collect()
}

/// The blocks at `idx` (out-of-range indices are skipped).
pub fn values(blocks: &[Block], idx: &[usize]) -> Vec<Block> {
    idx.iter().filter_map(|&i| blocks.get(i).copied()).collect()
}

/// Blocks overlapping the steps from `s0` to `s1` (both included, in
/// either order).
pub fn in_box(blocks: &[Block], uid: MachineUid, s0: u32, s1: u32, lens: Lens<'_>) -> Vec<usize> {
    let (a, b) = (s0.min(s1), s0.max(s1));
    (0..blocks.len())
        .filter(|&i| blocks[i].start <= b && song::block_end(&blocks[i], uid, lens) > a)
        .collect()
}

/// Blocks starting in `[a, b)`.
pub fn in_range(blocks: &[Block], a: u32, b: u32) -> Vec<usize> {
    (0..blocks.len())
        .filter(|&i| (a..b).contains(&blocks[i].start))
        .collect()
}

/// One lane's part of a group edit: the blocks that stay, its machine,
/// and the blocks that move together (at their starts before the move).
#[derive(Clone, Copy)]
pub struct GroupLane<'a> {
    pub rest: &'a [Block],
    pub uid: MachineUid,
    pub moved: &'a [Block],
}

/// Whether every moved block of `lanes`, moved by `d` steps, starts at or
/// after 0, ends by `MAX_SONG_STEPS` and overlaps no block that stays.
fn group_fits(lanes: &[GroupLane<'_>], d: i64, lens: Lens<'_>) -> bool {
    let max = MAX_SONG_STEPS as i64;
    lanes.iter().all(|l| {
        l.moved.iter().all(|m| {
            let s = m.start as i64 + d;
            let e = s + (song::block_end(m, l.uid, lens) - m.start) as i64;
            if s < 0 || e > max {
                return false;
            }
            // Ends of a sorted lane without overlaps are sorted too.
            let i = l.rest.partition_point(|x| (x.start as i64) < e);
            i == 0 || (song::block_end(&l.rest[i - 1], l.uid, lens) as i64) <= s
        })
    })
}

/// The one offset that moves every lane's group: the nearest to `want`
/// on its grid (`want` plus whole `unit`s, both ways; the later wins a
/// tie) at which every moved block fits against the blocks that stay on
/// its lane. `None` when there is none before `MAX_SONG_STEPS`.
pub fn group_offset(lanes: &[GroupLane<'_>], want: i64, unit: u32, lens: Lens<'_>) -> Option<i64> {
    let unit = unit.max(1) as i64;
    let max = MAX_SONG_STEPS as i64;
    if group_fits(lanes, want, lens) {
        return Some(want);
    }
    for k in 1..=max / unit + 1 {
        let (later, earlier) = (want + k * unit, want - k * unit);
        if later > max && earlier < -max {
            break;
        }
        for d in [later, earlier] {
            if group_fits(lanes, d, lens) {
                return Some(d);
            }
        }
    }
    None
}

/// `rest` with `moved` added `d` steps later (they must fit there): the
/// lane, sorted, and the moved blocks' indices in it.
pub fn with_moved(rest: &[Block], moved: &[Block], d: i64) -> (Vec<Block>, Vec<usize>) {
    let placed: Vec<Block> = moved
        .iter()
        .map(|b| Block {
            start: (b.start as i64 + d) as u32,
            ..*b
        })
        .collect();
    let mut lane: Vec<Block> = rest.iter().chain(&placed).copied().collect();
    lane.sort_by_key(|b| b.start);
    // Starts are unique on a lane without overlaps.
    let idx = (0..lane.len())
        .filter(|&i| placed.iter().any(|p| p.start == lane[i].start))
        .collect();
    (lane, idx)
}

/// Moves the blocks at `idx` together by the nearest offset to `dt` on
/// the `unit` grid at which they all fit (`group_offset`), keeping their
/// spacing. Returns their new indices; `None`, the lane untouched, when
/// there is no such offset.
pub fn shift(
    blocks: &mut Vec<Block>,
    uid: MachineUid,
    idx: &[usize],
    dt: i64,
    unit: u32,
    lens: Lens<'_>,
) -> Option<Vec<usize>> {
    let moved = values(blocks, idx);
    if moved.is_empty() {
        return Some(Vec::new());
    }
    let rest: Vec<Block> = (0..blocks.len())
        .filter(|i| !idx.contains(i))
        .map(|i| blocks[i])
        .collect();
    let lane = GroupLane {
        rest: &rest,
        uid,
        moved: &moved,
    };
    let d = group_offset(&[lane], dt, unit, lens)?;
    if d == 0 {
        return Some(idx.to_vec());
    }
    let (lane, new) = with_moved(&rest, &moved, d);
    *blocks = lane;
    Some(new)
}

/// Copies of the blocks at `idx`, moved together like `shift` moves
/// (the originals stay and count as taken). Returns the copies' indices;
/// `None`, the lane untouched, when they fit nowhere.
pub fn shift_copies(
    blocks: &mut Vec<Block>,
    uid: MachineUid,
    idx: &[usize],
    dt: i64,
    unit: u32,
    lens: Lens<'_>,
) -> Option<Vec<usize>> {
    let moved = values(blocks, idx);
    if moved.is_empty() {
        return Some(Vec::new());
    }
    let lane = GroupLane {
        rest: blocks,
        uid,
        moved: &moved,
    };
    let d = group_offset(&[lane], dt, unit, lens)?;
    let (lane, new) = with_moved(blocks, &moved, d);
    *blocks = lane;
    Some(new)
}

/// Removes the blocks at `idx`.
pub fn delete(blocks: &mut Vec<Block>, idx: &[usize]) {
    let mut i = 0;
    blocks.retain(|_| {
        let keep = !idx.contains(&i);
        i += 1;
        keep
    });
}

/// The blocks at `idx` with starts relative to `origin` (<= every start).
pub fn copy(blocks: &[Block], idx: &[usize], origin: u32) -> Vec<Block> {
    values(blocks, idx)
        .into_iter()
        .map(|b| Block {
            start: b.start.saturating_sub(origin),
            ..b
        })
        .collect()
}

/// Places `clip` (starts relative to its origin) at step `at`, or as
/// near it as the whole clip fits on the `unit` grid (`group_offset`).
/// Returns the pasted blocks' indices; `None`, the lane untouched, when
/// it fits nowhere.
pub fn paste(
    blocks: &mut Vec<Block>,
    uid: MachineUid,
    clip: &[Block],
    at: u32,
    unit: u32,
    lens: Lens<'_>,
) -> Option<Vec<usize>> {
    if clip.is_empty() {
        return Some(Vec::new());
    }
    let lane = GroupLane {
        rest: blocks,
        uid,
        moved: clip,
    };
    let d = group_offset(&[lane], at as i64, unit, lens)?;
    let (lane, new) = with_moved(blocks, clip, d);
    *blocks = lane;
    Some(new)
}

/// Pastes a copy of the blocks at `idx` right after them: one span (last
/// end - first start) later, rounded up to whole bars (or the nearest
/// whole-bar offset where it fits).
pub fn duplicate(
    blocks: &mut Vec<Block>,
    uid: MachineUid,
    idx: &[usize],
    lens: Lens<'_>,
) -> Option<Vec<usize>> {
    let sel = values(blocks, idx);
    let lo = sel.iter().map(|b| b.start).min()?;
    let hi = sel.iter().map(|b| song::block_end(b, uid, lens)).max()?;
    let span = (hi - lo).div_ceil(BAR) * BAR;
    let clip = copy(blocks, idx, lo);
    paste(blocks, uid, &clip, lo.saturating_add(span), BAR, lens)
}

/// Lengthens (or shortens) each block at `idx` by `dlen` steps, fixing
/// its length (`len: Some`): at least 1 step, and never past the next
/// block's start or the song limit. `dlen == 0` changes nothing.
pub fn resize(blocks: &mut [Block], uid: MachineUid, idx: &[usize], dlen: i64, lens: Lens<'_>) {
    if dlen == 0 {
        return;
    }
    let max = MAX_SONG_STEPS as u32;
    for &i in idx {
        let Some(b) = blocks.get(i) else {
            continue;
        };
        let next = blocks.get(i + 1).map_or(max, |n| n.start.min(max));
        let cap = (next.saturating_sub(b.start)).min(u16::MAX as u32).max(1) as i64;
        let len = song::block_end(b, uid, lens) - b.start;
        let new = (len as i64 + dlen).clamp(1, cap) as u16;
        blocks[i].len = Some(new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::project::PatternRef;

    const UID: MachineUid = MachineUid(1);

    /// Pattern `n` lasts 16 steps, except pattern 1 (8) and 2 (32).
    fn lens(_: MachineUid, slot: u8) -> u32 {
        match slot {
            1 => 8,
            2 => 32,
            _ => 16,
        }
    }

    fn b(start: u32, pattern: u8) -> Block {
        Block::new(start, PatternRef(pattern))
    }

    fn starts(blocks: &[Block]) -> Vec<u32> {
        blocks.iter().map(|b| b.start).collect()
    }

    #[test]
    fn resolve_finds_selected_values() {
        let lane = vec![b(0, 0), b(16, 0), b(32, 1)];
        assert_eq!(resolve(&lane, &[b(32, 1), b(0, 0)]), [0, 2]);
        assert!(resolve(&lane, &[b(8, 0)]).is_empty());
    }

    #[test]
    fn box_and_range_pick_blocks() {
        let lane = vec![b(0, 0), b(16, 1), b(32, 2)];
        assert_eq!(in_box(&lane, UID, 20, 10, &lens), [0, 1]);
        assert_eq!(in_box(&lane, UID, 24, 31, &lens), Vec::<usize>::new());
        assert_eq!(in_box(&lane, UID, 40, 40, &lens), [2]);
        assert_eq!(in_range(&lane, 16, 33), [1, 2]);
        assert_eq!(in_range(&lane, 1, 16), Vec::<usize>::new());
    }

    #[test]
    fn shift_moves_into_free_space() {
        let mut lane = vec![b(0, 0), b(16, 0), b(64, 0)];
        assert_eq!(
            shift(&mut lane, UID, &[0, 1], 16, BAR, &lens),
            Some(vec![0, 1])
        );
        assert_eq!(starts(&lane), [16, 32, 64]);
    }

    #[test]
    fn shift_is_held_at_the_song_start() {
        let mut lane = vec![b(16, 0), b(48, 0)];
        assert_eq!(shift(&mut lane, UID, &[1], -64, 1, &lens), Some(vec![0]));
        assert_eq!(starts(&lane), [0, 16], "held at step 0, past the block");
        let mut lane = vec![b(16, 0)];
        shift(&mut lane, UID, &[0], -64, 1, &lens);
        assert_eq!(starts(&lane), [0]);
    }

    #[test]
    fn a_move_onto_a_block_lands_on_the_nearest_free_spot() {
        let mut lane = vec![b(0, 0), b(32, 0)];
        // 0 → 32 is taken: the nearest free start is 48 (16 is as near,
        // later wins the tie).
        assert_eq!(shift(&mut lane, UID, &[0], 32, 1, &lens), Some(vec![1]));
        assert_eq!(starts(&lane), [32, 48]);
    }

    #[test]
    fn a_move_that_fits_nowhere_is_refused() {
        let max = MAX_SONG_STEPS as u32;
        // Bars end to end up to the limit: a copy fits nowhere.
        let mut lane: Vec<Block> = (0..max / 16).map(|i| b(i * 16, 0)).collect();
        let before = lane.clone();
        assert_eq!(shift_copies(&mut lane, UID, &[0], 16, BAR, &lens), None);
        assert_eq!(lane, before, "untouched");
        // (A plain move always fits: its own steps are free once it goes.)
    }

    #[test]
    fn a_group_move_keeps_its_spacing_and_never_takes_in_other_blocks() {
        // A and B selected, C not; all 16 steps.
        let lane = vec![b(0, 0), b(16, 3), b(32, 4)];
        // One bar on: B would sit on C. Offsets +32 (A on C) and +0 are
        // next, equally near; +32 does not fit, so it stays put.
        let mut l = lane.clone();
        assert_eq!(
            shift(&mut l, UID, &[0, 1], 16, BAR, &lens),
            Some(vec![0, 1])
        );
        assert_eq!(l, lane);
        // Two bars on: +32 does not fit, +48 is next (later beats +16,
        // which does not fit either).
        let mut l = lane.clone();
        assert_eq!(
            shift(&mut l, UID, &[0, 1], 32, BAR, &lens),
            Some(vec![1, 2])
        );
        assert_eq!(l, [b(32, 4), b(48, 0), b(64, 3)]);
        // In steps the group fits at +0 or from +48 on: +0 is nearer.
        let mut l = lane.clone();
        assert_eq!(shift(&mut l, UID, &[0, 1], 16, 1, &lens), Some(vec![0, 1]));
        assert_eq!(l, lane);
    }

    #[test]
    fn a_group_offset_fits_every_lane_at_once() {
        let (r1, m1) = (vec![b(16, 0)], vec![b(0, 0)]);
        let (r2, m2) = (vec![b(48, 0)], vec![b(0, 0)]);
        let lanes = [
            GroupLane {
                rest: &r1,
                uid: UID,
                moved: &m1,
            },
            GroupLane {
                rest: &r2,
                uid: MachineUid(2),
                moved: &m2,
            },
        ];
        // Lane 1 frees up from 32, lane 2 is taken at 48..64.
        assert_eq!(group_offset(&lanes, 32, BAR, &lens), Some(32));
        assert_eq!(group_offset(&lanes, 48, BAR, &lens), Some(64));
        assert_eq!(group_offset(&lanes[..1], 16, BAR, &lens), Some(32));
    }

    #[test]
    fn copies_move_and_originals_stay() {
        let mut lane = vec![b(0, 0), b(16, 1)];
        assert_eq!(
            shift_copies(&mut lane, UID, &[0, 1], 32, BAR, &lens),
            Some(vec![2, 3])
        );
        assert_eq!(starts(&lane), [0, 16, 32, 48]);
    }

    #[test]
    fn delete_and_copy() {
        let mut lane = vec![b(0, 0), b(16, 1), b(32, 2)];
        assert_eq!(copy(&lane, &[1, 2], 16), [b(0, 1), b(16, 2)]);
        delete(&mut lane, &[0, 2]);
        assert_eq!(lane, [b(16, 1)]);
    }

    #[test]
    fn a_paste_lands_whole_on_the_nearest_free_offset() {
        let mut lane = vec![b(16, 0)];
        let clip = [b(0, 1), b(8, 1)];
        // 12..28 overlaps 16..32: the whole clip goes to the nearest
        // offset where it fits, 0 (32 is further).
        assert_eq!(paste(&mut lane, UID, &clip, 12, 1, &lens), Some(vec![0, 1]));
        assert_eq!(starts(&lane), [0, 8, 16]);
    }

    #[test]
    fn duplicate_repeats_after_the_span_in_bars() {
        let mut lane = vec![b(0, 1), b(64, 0)];
        assert_eq!(duplicate(&mut lane, UID, &[0], &lens), Some(vec![1]));
        assert_eq!(starts(&lane), [0, 16, 64], "8 steps round up to a bar");
    }

    #[test]
    fn resize_fixes_the_length_and_stops_at_the_next_block() {
        let mut lane = vec![b(0, 0), b(20, 0), b(64, 0)];
        resize(&mut lane, UID, &[0, 1], 16, &lens);
        assert_eq!(lane[0].len, Some(20), "up to the next block");
        assert_eq!(lane[1].len, Some(32));
        resize(&mut lane, UID, &[1], -40, &lens);
        assert_eq!(lane[1].len, Some(1), "at least a step");
        let mut lane = vec![b(0, 0)];
        resize(&mut lane, UID, &[0], 0, &lens);
        assert_eq!(lane[0].len, None, "no change, still follows its pattern");
    }
}
