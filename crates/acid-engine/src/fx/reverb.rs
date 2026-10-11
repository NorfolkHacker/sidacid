use crate::params::lin;

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const INPUT_GAIN: f32 = 0.015;

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0, store: 0.0 }
    }

    fn process(&mut self, input: f32, feedback: f32, damp1: f32, damp2: f32) -> f32 {
        let out = self.buf[self.idx];
        self.store = out * damp2 + self.store * damp1;
        if self.store.abs() < 1.0e-20 {
            self.store = 0.0;
        }
        self.buf[self.idx] = input + self.store * feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], idx: 0 }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buf[self.idx];
        let out = buffered - input;
        self.buf[self.idx] = input + buffered * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    feedback: f32,
    damp1: f32,
    damp2: f32,
    wet1: f32,
    wet2: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let sized = |n: usize, spread: usize| ((n + spread) as f32 * scale) as usize;
        let side = |spread: usize| {
            (
                COMB_TUNING.iter().map(|&n| Comb::new(sized(n, spread))).collect::<Vec<_>>(),
                ALLPASS_TUNING.iter().map(|&n| Allpass::new(sized(n, spread))).collect::<Vec<_>>(),
            )
        };
        let (cl, al) = side(0);
        let (cr, ar) = side(STEREO_SPREAD);
        let mut r = Self { combs: [cl, cr], allpasses: [al, ar], feedback: 0.84, damp1: 0.2, damp2: 0.8, wet1: 1.0, wet2: 0.0 };
        r.set_size(0.6);
        r.set_damping(0.5);
        r.set_width(1.0);
        r
    }

    pub fn set_size(&mut self, v: f32) {
        self.feedback = lin(v, 0.7, 0.98);
    }

    pub fn set_damping(&mut self, v: f32) {
        self.damp1 = lin(v, 0.0, 0.4);
        self.damp2 = 1.0 - self.damp1;
    }

    pub fn set_width(&mut self, v: f32) {
        let w = v.clamp(0.0, 1.0);
        self.wet1 = w / 2.0 + 0.5;
        self.wet2 = (1.0 - w) / 2.0;
    }

    pub fn reset(&mut self) {
        for side in &mut self.combs {
            for c in side {
                c.buf.fill(0.0);
                c.store = 0.0;
            }
        }
        for side in &mut self.allpasses {
            for a in side {
                a.buf.fill(0.0);
            }
        }
    }

    pub fn process(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        for i in 0..in_l.len() {
            let input = (in_l[i] + in_r[i]) * INPUT_GAIN;
            let mut side_out = [0.0f32; 2];
            for s in 0..2 {
                let mut acc = 0.0;
                for c in &mut self.combs[s] {
                    acc += c.process(input, self.feedback, self.damp1, self.damp2);
                }
                for a in &mut self.allpasses[s] {
                    acc = a.process(acc);
                }
                side_out[s] = acc;
            }
            out_l[i] += side_out[0] * self.wet1 + side_out[1] * self.wet2;
            out_r[i] += side_out[1] * self.wet1 + side_out[0] * self.wet2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{all_finite, peak};

    const SR: f32 = 44_100.0;

    fn impulse_response(r: &mut Reverb, n: usize) -> Vec<f32> {
        let mut il = vec![0.0; n];
        il[0] = 1.0;
        let ir = il.clone();
        let mut ol = vec![0.0; n];
        let mut or = vec![0.0; n];
        r.process(&il, &ir, &mut ol, &mut or);
        ol
    }

    #[test]
    fn has_a_tail_that_decays() {
        let mut r = Reverb::new(SR);
        r.set_size(0.5);
        r.set_damping(0.5);
        let out = impulse_response(&mut r, 10 * SR as usize);
        assert!(peak(&out[2205..4410]) > 1e-3, "tail present at 50-100 ms");
        assert!(peak(&out[9 * SR as usize..]) < 1e-3, "tail gone by 9 s");
    }

    #[test]
    fn max_size_is_stable() {
        let mut r = Reverb::new(SR);
        r.set_size(1.0);
        r.set_damping(0.0);
        let out = impulse_response(&mut r, 5 * SR as usize);
        assert!(all_finite(&out));
        assert!(peak(&out) < 1.0);
    }

    #[test]
    fn works_at_48k() {
        let mut r = Reverb::new(48_000.0);
        let out = impulse_response(&mut r, 48_000);
        assert!(all_finite(&out) && peak(&out) > 1e-4);
    }
}
