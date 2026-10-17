//! The per-position half of the audio thread's status: what the panels
//! show by rack position (`Shown`), and the buffers it arrives in
//! (`BufferPool`). The host fills only buffers the UI hands it, so the
//! audio thread never allocates one.

use acid_engine::host::{PositionStatus, STATUS_BUFFERS};
use acid_engine::machine::MachineUid;

/// The per-position status the panels show, by rack position.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub p: PositionStatus,
}

impl Shown {
    /// 0 past the end.
    pub fn playhead(&self, pos: usize) -> usize {
        self.p.playhead.get(pos).copied().unwrap_or(0)
    }

    /// 0 past the end.
    pub fn pattern(&self, pos: usize) -> u8 {
        self.p.patterns.get(pos).copied().unwrap_or(0)
    }

    /// None past the end.
    pub fn queued(&self, pos: usize) -> Option<u8> {
        self.p.queued.get(pos).copied().flatten()
    }

    /// 0.0 past the end.
    pub fn meter(&self, pos: usize) -> f32 {
        self.p.meters.get(pos).copied().unwrap_or(0.0)
    }

    /// Takes `from`'s values (copying, keeping our own allocation).
    pub fn take_from(&mut self, from: &PositionStatus) {
        let p = &mut self.p;
        p.playhead.clone_from(&from.playhead);
        p.patterns.clone_from(&from.patterns);
        p.queued.clone_from(&from.queued);
        p.meters.clone_from(&from.meters);
        p.slot_cpu.clone_from(&from.slot_cpu);
    }

    /// Moves values from `old_rack` order to `new_rack` order; new machines
    /// read defaults.
    pub fn follow(&mut self, old_rack: &[MachineUid], new_rack: &[MachineUid]) {
        let old = std::mem::take(&mut self.p);
        let p = &mut self.p;
        for uid in new_rack {
            let j = old_rack.iter().position(|u| u == uid);
            p.playhead.push(at(&old.playhead, j));
            p.patterns.push(at(&old.patterns, j));
            p.queued.push(at(&old.queued, j));
            p.meters.push(at(&old.meters, j));
            p.slot_cpu.push(at(&old.slot_cpu, j));
        }
    }
}

/// `v[j]`, or the default when there is no `j` or `v` is shorter.
fn at<T: Copy + Default>(v: &[T], j: Option<usize>) -> T {
    j.and_then(|j| v.get(j).copied()).unwrap_or_default()
}

/// Buffers to send so the host holds `STATUS_BUFFERS`; tracks how many are out.
pub struct BufferPool {
    /// Buffers sent and not yet back.
    out: usize,
    /// Rack positions each new buffer has room for.
    capacity: usize,
}

impl BufferPool {
    pub fn new(capacity: usize) -> Self {
        Self { out: 0, capacity }
    }

    /// Fresh buffers to send now: `STATUS_BUFFERS - out` of `capacity`.
    // Boxed: each goes on as it is, in a `Command::StatusBuffer`.
    #[allow(clippy::vec_box)]
    pub fn top_up(&mut self) -> Vec<Box<PositionStatus>> {
        let fresh = STATUS_BUFFERS.saturating_sub(self.out);
        self.out += fresh;
        (0..fresh)
            .map(|_| PositionStatus::with_capacity(self.capacity))
            .collect()
    }

    /// A buffer came back (filled or disposed): reuse it if it fits, else drop.
    pub fn returned(&mut self, mut b: Box<PositionStatus>) -> Option<Box<PositionStatus>> {
        self.out = self.out.saturating_sub(1);
        debug_assert!(self.out < STATUS_BUFFERS);
        if !b.fits(self.capacity) {
            return None;
        }
        b.clear();
        self.out += 1;
        Some(b)
    }

    /// New buffers have room for `capacity` positions from now on;
    /// smaller ones that come back are dropped (and replaced).
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
    }

    /// Rack positions each new buffer has room for.
    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// A new host holds none of ours.
    pub fn reset(&mut self) {
        self.out = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::machine::MachineUid as U;

    #[test]
    fn shown_reads_defaults_past_the_rack_and_follows_moves() {
        let mut s = Shown::default();
        let p = PositionStatus {
            playhead: vec![3, 5],
            patterns: vec![1, 2],
            queued: vec![None, Some(4)],
            meters: vec![0.5, 0.25],
            slot_cpu: vec![0.1, 0.2],
        };
        s.take_from(&p);
        assert_eq!((s.playhead(1), s.pattern(1), s.queued(1)), (5, 2, Some(4)));
        assert_eq!(
            (s.playhead(9), s.pattern(9), s.queued(9), s.meter(9)),
            (0, 0, None, 0.0)
        );
        s.follow(&[U(1), U(2)], &[U(2), U(7), U(1)]);
        assert_eq!((s.playhead(0), s.playhead(1), s.playhead(2)), (5, 0, 3));
        assert_eq!(s.meter(0), 0.25);
    }

    #[test]
    fn the_pool_keeps_four_out_and_drops_small_buffers() {
        let mut pool = BufferPool::new(16);
        assert_eq!(pool.top_up().len(), STATUS_BUFFERS);
        assert!(pool.top_up().is_empty());
        let back = pool.returned(PositionStatus::with_capacity(16));
        assert!(back.is_some(), "a fitting buffer is reused");
        pool.set_capacity(32);
        assert!(pool.returned(PositionStatus::with_capacity(16)).is_none());
        assert_eq!(pool.top_up().len(), 1, "one replaced at the new size");
        pool.reset();
        assert_eq!(pool.top_up().len(), STATUS_BUFFERS);
    }
}
