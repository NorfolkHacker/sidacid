//! The arpeggiator: which note plays next (Up, Up & Down, Down over 1–3
//! octaves of the held notes) and when. While the transport runs, steps
//! fall on the song's tick grid (the engine reports each tick); stopped,
//! it free-runs from the tempo. Step lengths are in thirds of a tick, so
//! triplets land exactly.

use super::patch::ArpMode;
use super::synth::MAX_HELD;

const MAX_NOTES: usize = MAX_HELD * 3;
/// Sequencer ticks per beat (4 per 1/16 step).
const TICKS_PER_BEAT: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpEvent {
    Idle,
    /// Play the next note now.
    Step,
    /// The current arp note's gate closes now.
    GateOff,
}

pub struct Arp {
    /// Held pitches, ascending, repeated up `octaves` octaves.
    notes: [u8; MAX_NOTES],
    len: usize,
    pos: usize,
    transport: bool,
    /// Samples until the step scheduled in the current tick.
    pub(super) next_in: Option<f64>,
    tick_len: f64,
    free_tick_len: f64,
    free_left: f64,
    gate_left: f64,
    steps: u64,
}

impl Default for Arp {
    fn default() -> Self {
        Self::new()
    }
}

impl Arp {
    pub fn new() -> Self {
        Self {
            notes: [0; MAX_NOTES],
            len: 0,
            pos: 0,
            transport: false,
            next_in: None,
            tick_len: 0.0,
            free_tick_len: 1500.0,
            free_left: 0.0,
            gate_left: 0.0,
            steps: 0,
        }
    }

    /// Rebuilds the sequence from the held pitches (any order, no
    /// duplicates), keeping the current place in it.
    pub fn set_notes(&mut self, held: &[u8], octaves: u8) {
        let mut base = [0u8; MAX_HELD];
        let n = held.len().min(MAX_HELD);
        base[..n].copy_from_slice(&held[..n]);
        base[..n].sort_unstable();
        self.len = 0;
        for oct in 0..octaves.max(1) {
            for &p in &base[..n] {
                let q = p as u16 + 12 * oct as u16;
                if q <= 127 && self.len < MAX_NOTES {
                    self.notes[self.len] = q as u8;
                    self.len += 1;
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Drops pending gate-off and free-run timing (the arp stopped or reset).
    pub fn clear_timing(&mut self) {
        self.gate_left = 0.0;
        self.free_left = 0.0;
    }

    /// Back to power-on: no notes' timing, no transport, no scheduled step.
    pub fn reset(&mut self) {
        self.clear_timing();
        self.transport = false;
        self.next_in = None;
        self.pos = 0;
    }

    /// One sample passes with no arp notes: a step scheduled in this tick
    /// counts down and is dropped once its sample is gone, so a note
    /// arriving later cannot fire off the grid.
    pub fn skip(&mut self) {
        if let Some(n) = self.next_in {
            self.next_in = (n > 0.5).then_some(n - 1.0);
        }
    }

    pub fn restart(&mut self) {
        self.pos = 0;
    }

    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// The next pitch in `mode`, advancing; `None` with no notes.
    pub fn next_pitch(&mut self, mode: ArpMode) -> Option<u8> {
        let len = self.len;
        if len == 0 {
            return None;
        }
        let period = match mode {
            ArpMode::UpDown if len >= 2 => 2 * len - 2,
            _ => len,
        };
        let i = self.pos.min(period - 1);
        self.pos = (i + 1) % period;
        Some(match mode {
            ArpMode::Up => self.notes[i],
            ArpMode::Down => self.notes[len - 1 - i],
            ArpMode::UpDown if i < len => self.notes[i],
            ArpMode::UpDown => self.notes[2 * len - 2 - i],
        })
    }

    /// Sequencer tick number `tick` (`tick_len` samples long) starts now:
    /// schedules the step boundary inside it, if there is one.
    pub fn clock_tick(&mut self, tick: u64, tick_len: f64, thirds: u32) {
        self.transport = true;
        self.tick_len = tick_len;
        let t3 = tick * 3;
        let d = thirds.max(1) as u64;
        let off = t3.div_ceil(d) * d - t3;
        self.next_in = (off < 3).then(|| off as f64 / 3.0 * tick_len);
    }

    pub fn stop_transport(&mut self) {
        self.transport = false;
        self.next_in = None;
    }

    pub fn set_bpm(&mut self, bpm: f32, sample_rate: f32) {
        self.free_tick_len = 60.0 / bpm.max(1.0) as f64 / TICKS_PER_BEAT * sample_rate as f64;
    }

    /// Transport stopped and the first note just arrived: step on the next
    /// sample, then every step length after it.
    pub fn start_free(&mut self) {
        self.free_left = 1.0;
    }

    /// One sample on.
    pub fn advance(&mut self, thirds: u32) -> ArpEvent {
        let tick = if self.transport { self.tick_len } else { self.free_tick_len };
        let step_len = thirds as f64 / 3.0 * tick;
        let step = if self.transport {
            match self.next_in {
                Some(n) if n <= 0.5 => {
                    self.next_in = None;
                    true
                }
                Some(n) => {
                    self.next_in = Some(n - 1.0);
                    false
                }
                None => false,
            }
        } else {
            self.free_left -= 1.0;
            let due = self.free_left <= 0.0;
            if due {
                self.free_left += step_len;
            }
            due
        };
        if step {
            self.steps += 1;
            self.gate_left = step_len * 0.5;
            return ArpEvent::Step;
        }
        if self.gate_left > 0.0 {
            self.gate_left -= 1.0;
            if self.gate_left <= 0.0 {
                return ArpEvent::GateOff;
            }
        }
        ArpEvent::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ArpMode::*;

    fn take(a: &mut Arp, mode: ArpMode, n: usize) -> Vec<u8> {
        (0..n).map(|_| a.next_pitch(mode).unwrap()).collect()
    }

    fn arp(notes: &[u8], octaves: u8) -> Arp {
        let mut a = Arp::new();
        a.set_notes(notes, octaves);
        a
    }

    #[test]
    fn up_down_and_up_down_orders() {
        assert_eq!(take(&mut arp(&[60, 48, 55], 1), Up, 4), [48, 55, 60, 48]);
        assert_eq!(take(&mut arp(&[60, 48, 55], 1), Down, 4), [60, 55, 48, 60]);
        assert_eq!(take(&mut arp(&[48, 55, 60], 1), UpDown, 6), [48, 55, 60, 55, 48, 55]);
        assert_eq!(take(&mut arp(&[48], 1), UpDown, 2), [48, 48]);
        assert_eq!(arp(&[], 1).next_pitch(Up), None);
    }

    #[test]
    fn range_adds_octaves() {
        assert_eq!(take(&mut arp(&[48, 55], 2), Up, 5), [48, 55, 60, 67, 48]);
        assert_eq!(take(&mut arp(&[120], 3), Up, 2), [120, 120], "past 127 is dropped");
    }

    #[test]
    fn a_shrinking_chord_clamps_the_place() {
        let mut a = arp(&[48, 52, 55, 60], 1);
        take(&mut a, Up, 3);
        a.set_notes(&[48, 52, 55], 1);
        assert_eq!(a.next_pitch(Up), Some(55));
    }

    #[test]
    fn cleared_timing_leaves_no_stale_gate_off() {
        let mut a = arp(&[48], 1);
        a.clock_tick(0, 100.0, 12);
        assert_eq!(a.advance(12), ArpEvent::Step);
        a.clear_timing();
        a.clock_tick(1, 100.0, 12);
        assert!((0..200).all(|_| a.advance(12) == ArpEvent::Idle));
    }

    #[test]
    fn a_chord_change_keeps_the_place() {
        let mut a = arp(&[48, 55, 60], 1);
        take(&mut a, Up, 2);
        a.set_notes(&[48, 55, 60, 64], 1);
        assert_eq!(a.next_pitch(Up), Some(60));
    }

    #[test]
    fn sixteenths_and_triplets_land_on_the_tick_grid() {
        let mut a = Arp::new();
        let at = |a: &mut Arp, tick: u64, thirds: u32| {
            a.clock_tick(tick, 300.0, thirds);
            a.next_in
        };
        assert_eq!(at(&mut a, 0, 12), Some(0.0));
        assert_eq!(at(&mut a, 1, 12), None);
        assert_eq!(at(&mut a, 4, 12), Some(0.0));
        // 1/16T: steps at 0, 8/3 and 16/3 ticks.
        assert_eq!(at(&mut a, 1, 8), None);
        assert_eq!(at(&mut a, 2, 8), Some(200.0));
        assert_eq!(at(&mut a, 5, 8), Some(100.0));
    }

    #[test]
    fn clocked_steps_fire_on_time_and_gate_for_half_a_step() {
        let mut a = arp(&[48], 1);
        a.clock_tick(0, 100.0, 12);
        assert_eq!(a.advance(12), ArpEvent::Step);
        let gate_off = (1..400).find(|_| a.advance(12) == ArpEvent::GateOff);
        assert_eq!(gate_off, Some(200));
        assert_eq!(a.steps(), 1);
    }

    #[test]
    fn stopped_it_free_runs_from_the_tempo() {
        let mut a = arp(&[48], 1);
        a.set_bpm(120.0, 48_000.0);
        a.start_free();
        assert_eq!(a.advance(12), ArpEvent::Step);
        // 1/16 at 120 BPM is 6000 samples.
        let next = (1..7000).find(|_| a.advance(12) == ArpEvent::Step);
        assert_eq!(next, Some(6000));
    }
}
