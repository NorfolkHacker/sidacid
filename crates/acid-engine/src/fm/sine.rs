//! Sine lookup: a 4096-point table read with linear interpolation, or
//! (Vintage) a coarse 1024-point read without interpolation.

use std::f32::consts::TAU;

const SIZE: usize = 4096;

pub struct Sine {
    table: Box<[f32]>,
}

impl Default for Sine {
    fn default() -> Self {
        Self::new()
    }
}

impl Sine {
    /// Allocates the table: call off the audio thread.
    pub fn new() -> Self {
        Self {
            table: (0..=SIZE)
                .map(|i| (i as f32 / SIZE as f32 * TAU).sin())
                .collect(),
        }
    }

    /// sin(2π · phase) for a phase in cycles (any value).
    pub fn at(&self, phase: f32, vintage: bool) -> f32 {
        let p = phase - phase.floor();
        let x = p * SIZE as f32;
        let i = (x as usize).min(SIZE - 1);
        if vintage {
            self.table[i & !3]
        } else {
            let a = self.table[i];
            a + (self.table[i + 1] - a) * (x - i as f32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_sine_is_accurate_and_vintage_is_coarse() {
        let s = Sine::new();
        for i in 0..1000 {
            let ph = i as f32 / 997.0 - 3.0;
            let exact = (ph * std::f32::consts::TAU).sin();
            assert!((s.at(ph, false) - exact).abs() < 2e-5, "phase {ph}");
            assert!((s.at(ph, true) - exact).abs() < 0.01);
        }
        assert_eq!(s.at(0.25, false), 1.0);
        assert_eq!(s.at(0.1, true), s.at(0.1002, true), "vintage steps");
    }
}
