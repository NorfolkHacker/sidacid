const TARGET_RATE: f32 = 22_050.0;

pub struct LoFi {
    pub on: bool,
    step: f32,
    phase: f32,
    held: [f32; 2],
}

fn quantise(x: f32) -> f32 {
    (x.clamp(-1.0, 1.0) * 127.0).round() / 127.0
}

impl LoFi {
    pub fn new(sample_rate: f32) -> Self {
        Self { on: false, step: (TARGET_RATE / sample_rate).min(1.0), phase: 1.0, held: [0.0; 2] }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if !self.on {
            return;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            self.phase += self.step;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.held = [quantise(*a), quantise(*b)];
            }
            *a = self.held[0];
            *b = self.held[1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantises_to_8_bits_and_holds() {
        let mut f = LoFi::new(44_100.0);
        f.on = true;
        let mut l: Vec<f32> = (0..100).map(|i| (i as f32 * 0.37).sin() * 0.9).collect();
        let mut r = l.clone();
        f.process(&mut l, &mut r);
        for v in &l {
            let q = v * 127.0;
            assert!((q - q.round()).abs() < 1e-4, "{v} not on an 8-bit step");
        }
        let repeats = l.windows(2).filter(|w| w[0] == w[1]).count();
        assert!(repeats >= 45, "held at half rate, repeats = {repeats}");
    }

    #[test]
    fn off_is_passthrough() {
        let mut f = LoFi::new(44_100.0);
        let mut l = vec![0.123, 0.456];
        let mut r = l.clone();
        f.process(&mut l, &mut r);
        assert_eq!(l, vec![0.123, 0.456]);
    }
}
