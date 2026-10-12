//! Decoded sample audio as the audio thread reads it. Immutable once
//! built; shared with the engine through an `Arc`.

#[derive(Debug, PartialEq)]
pub struct SampleData {
    /// Rate the audio was recorded at (playback converts on the fly).
    pub sample_rate: u32,
    pub left: Box<[f32]>,
    /// `None` for a mono sample (the left side plays on both).
    pub right: Option<Box<[f32]>>,
}

fn hermite(xm1: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c = (x1 - xm1) * 0.5;
    let v = x0 - x1;
    let w = c + v;
    let a = w + v + (x2 - x0) * 0.5;
    let b = w + a;
    ((a * t - b) * t + c) * t + x0
}

impl SampleData {
    pub fn frames(&self) -> usize {
        self.left.len()
    }

    fn at(data: &[f32], i: isize) -> f32 {
        let last = data.len() as isize - 1;
        data[i.clamp(0, last.max(0)) as usize]
    }

    fn cubic(data: &[f32], pos: f64) -> f32 {
        if data.is_empty() {
            return 0.0;
        }
        let i = pos.floor();
        let t = (pos - i) as f32;
        let i = i as isize;
        hermite(
            Self::at(data, i - 1),
            Self::at(data, i),
            Self::at(data, i + 1),
            Self::at(data, i + 2),
            t,
        )
    }

    /// Cubic-interpolated (left, right) at fractional frame `pos`,
    /// clamped to the sample.
    pub fn read(&self, pos: f64) -> (f32, f32) {
        let l = Self::cubic(&self.left, pos);
        let r = match &self.right {
            Some(r) => Self::cubic(r, pos),
            None => l,
        };
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp() -> SampleData {
        SampleData {
            sample_rate: 44_100,
            left: (0..100).map(|i| i as f32 / 100.0).collect(),
            right: None,
        }
    }

    #[test]
    fn cubic_read_is_exact_on_frames_and_on_a_ramp() {
        let s = ramp();
        assert_eq!(s.frames(), 100);
        assert_eq!(
            s.read(10.0),
            (0.1, 0.1),
            "mono reads the same on both sides"
        );
        let (l, _) = s.read(10.5);
        assert!((l - 0.105).abs() < 1e-6);
        assert_eq!(s.read(-5.0).0, 0.0, "clamped at the start");
        assert!((s.read(500.0).0 - 0.99).abs() < 1e-6, "clamped at the end");
    }

    #[test]
    fn stereo_reads_both_sides() {
        let s = SampleData {
            sample_rate: 48_000,
            left: vec![1.0; 8].into(),
            right: Some(vec![-1.0; 8].into()),
        };
        assert_eq!(s.read(3.3), (1.0, -1.0));
    }
}
