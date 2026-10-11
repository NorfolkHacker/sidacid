use crate::params::{exp, lin};
use std::f32::consts::TAU;

const MAX_SECONDS: f32 = 2.0;

pub struct Delay {
    pub ping_pong: bool,
    sample_rate: f32,
    buf: [Vec<f32>; 2],
    write: usize,
    delay: usize,
    feedback: f32,
    tone_coef: f32,
    lp: [f32; 2],
    sync: bool,
    division: usize,
    ms: f32,
    bpm: f32,
}

impl Delay {
    /// Delay lengths in 16th notes: 1/16, 1/8, 3/16, 1/4, 3/8, 1/2.
    pub const DIVISION_STEPS: [f32; 6] = [1.0, 2.0, 3.0, 4.0, 6.0, 8.0];

    pub fn new(sample_rate: f32) -> Self {
        let len = (sample_rate * MAX_SECONDS) as usize + 1;
        let mut d = Self {
            ping_pong: true,
            sample_rate,
            buf: [vec![0.0; len], vec![0.0; len]],
            write: 0,
            delay: 1,
            feedback: 0.4,
            tone_coef: 1.0,
            lp: [0.0; 2],
            sync: true,
            division: 2,
            ms: 300.0,
            bpm: 125.0,
        };
        d.set_tone(0.6);
        d.recompute();
        d
    }

    fn recompute(&mut self) {
        let seconds = if self.sync {
            Self::DIVISION_STEPS[self.division] * 60.0 / self.bpm / 4.0
        } else {
            self.ms / 1000.0
        };
        let len = self.buf[0].len();
        self.delay = ((seconds * self.sample_rate) as usize).clamp(1, len - 1);
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.bpm = bpm.max(1.0);
        self.recompute();
    }

    pub fn set_sync(&mut self, sync: bool) {
        self.sync = sync;
        self.recompute();
    }

    pub fn set_division(&mut self, idx: usize) {
        self.division = idx.min(Self::DIVISION_STEPS.len() - 1);
        self.recompute();
    }

    pub fn set_ms(&mut self, ms: f32) {
        self.ms = ms;
        self.recompute();
    }

    pub fn set_feedback(&mut self, v: f32) {
        self.feedback = lin(v, 0.0, 0.95);
    }

    pub fn set_tone(&mut self, v: f32) {
        let fc = exp(v, 500.0, 16_000.0);
        self.tone_coef = 1.0 - (-TAU * fc / self.sample_rate).exp();
    }

    pub fn delay_samples(&self) -> usize {
        self.delay
    }

    pub fn reset(&mut self) {
        for b in &mut self.buf {
            b.fill(0.0);
        }
        self.lp = [0.0; 2];
    }

    pub fn process(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let len = self.buf[0].len();
        for i in 0..in_l.len() {
            let read = (self.write + len - self.delay) % len;
            let dl = self.buf[0][read];
            let dr = self.buf[1][read];
            self.lp[0] += (dl - self.lp[0]) * self.tone_coef;
            self.lp[1] += (dr - self.lp[1]) * self.tone_coef;
            let fb_l = self.lp[0] * self.feedback;
            let fb_r = self.lp[1] * self.feedback;
            if self.ping_pong {
                self.buf[0][self.write] = 0.5 * (in_l[i] + in_r[i]) + fb_r;
                self.buf[1][self.write] = fb_l;
            } else {
                self.buf[0][self.write] = in_l[i] + fb_l;
                self.buf[1][self.write] = in_r[i] + fb_r;
            }
            out_l[i] += dl;
            out_r[i] += dr;
            self.write = (self.write + 1) % len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 44_100.0;

    fn impulse_response(d: &mut Delay, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut il = vec![0.0; n];
        let mut ir = vec![0.0; n];
        il[0] = 1.0;
        ir[0] = 1.0;
        let mut ol = vec![0.0; n];
        let mut or = vec![0.0; n];
        d.process(&il, &ir, &mut ol, &mut or);
        (ol, or)
    }

    #[test]
    fn synced_time_matches_tempo() {
        let mut d = Delay::new(SR);
        d.set_bpm(120.0);
        d.set_sync(true);
        d.set_division(0); // one 16th = 0.125 s
        assert_eq!(d.delay_samples(), 5512);
        d.set_division(2); // 3/16
        assert_eq!(d.delay_samples(), 16_537);
    }

    #[test]
    fn free_time_in_ms() {
        let mut d = Delay::new(SR);
        d.set_sync(false);
        d.set_ms(100.0);
        assert_eq!(d.delay_samples(), 4410);
    }

    #[test]
    fn echo_arrives_after_delay_time() {
        let mut d = Delay::new(SR);
        d.ping_pong = false;
        d.set_sync(false);
        d.set_ms(100.0);
        d.set_feedback(0.0);
        d.set_tone(1.0);
        let (l, _) = impulse_response(&mut d, 9000);
        assert!(peak(&l[..4400]) < 1e-6);
        assert!(peak(&l[4410..4420]) > 0.3);
    }

    #[test]
    fn ping_pong_alternates_sides() {
        let mut d = Delay::new(SR);
        d.ping_pong = true;
        d.set_sync(false);
        d.set_ms(100.0);
        d.set_feedback(0.5);
        d.set_tone(1.0);
        let (l, r) = impulse_response(&mut d, 13_300);
        assert!(peak(&l[4410..4430]) > 0.3 && peak(&r[4410..4430]) < 1e-3);
        assert!(peak(&r[8820..8850]) > 0.1 && peak(&l[8820..8850]) < 1e-3);
    }

    #[test]
    fn max_feedback_is_stable() {
        let mut d = Delay::new(SR);
        d.set_sync(false);
        d.set_ms(50.0);
        d.set_feedback(1.0);
        let (l, r) = impulse_response(&mut d, 10 * SR as usize);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(peak(&l) < 2.0);
        assert!(peak(&l[9 * SR as usize..]) < peak(&l[..SR as usize]));
    }
}
