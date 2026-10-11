//! Oscillator: 32-bit phase accumulator, the four SID waveforms and the
//! noise LFSR. Waveform shapes and LFSR taps come from Acid OS acid-synth.

const PHASE_SCALE: f64 = 4_294_967_296.0;
/// Maps a 24-bit value to 0..2.
const TO_UNIT: f32 = 2.0 / 16_777_216.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Waveform {
    #[default]
    Pulse,
    Saw,
    Triangle,
    Noise,
}

#[derive(Clone, Debug)]
pub struct Oscillator {
    pub waveform: Waveform,
    pub phase: u32,
    pub increment: u32,
    pub pulse_threshold: u32,
    pub lfsr: u32,
    noise_out: f32,
}

impl Oscillator {
    pub fn new(seed: u32) -> Self {
        Self {
            waveform: Waveform::Pulse,
            phase: 0,
            increment: 0,
            pulse_threshold: 1 << 31,
            lfsr: 0xACE1 + seed,
            noise_out: 0.0,
        }
    }

    pub fn set_freq(&mut self, hz: f32, sample_rate: f32) {
        let inc = hz.max(0.0) as f64 / sample_rate as f64 * PHASE_SCALE;
        self.increment = inc.min(PHASE_SCALE / 2.0 - 1.0) as u32;
    }

    pub fn freq(&self, sample_rate: f32) -> f32 {
        (self.increment as f64 * sample_rate as f64 / PHASE_SCALE) as f32
    }

    /// Duty cycle 0.01..0.99 (0.5 = square).
    pub fn set_pulse_width(&mut self, width: f32) {
        self.pulse_threshold = (width.clamp(0.01, 0.99) as f64 * PHASE_SCALE) as u32;
    }

    pub fn msb(&self) -> bool {
        self.phase >> 31 != 0
    }

    /// Current output, -1..1. `ring_msb` is the ring partner's msb when
    /// ring modulation is on (triangle only, as on the SID).
    pub fn sample(&self, ring_msb: bool) -> f32 {
        match self.waveform {
            Waveform::Saw => (self.phase >> 8) as f32 * TO_UNIT - 1.0,
            Waveform::Triangle => {
                let msb = self.msb() ^ ring_msb;
                let lower = self.phase << 1;
                let tri = if msb { !lower } else { lower };
                (tri >> 8) as f32 * TO_UNIT - 1.0
            }
            Waveform::Pulse => {
                if self.phase < self.pulse_threshold {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Noise => self.noise_out,
        }
    }

    /// Advances one sample; returns true when the phase wrapped. Noise
    /// clocks its LFSR every 1/16 cycle (each time bit 28 carries).
    pub fn advance(&mut self) -> bool {
        let old = self.phase;
        let sum = old as u64 + self.increment as u64;
        self.phase = sum as u32;
        if self.waveform == Waveform::Noise {
            let clocks = (sum >> 28) - (old as u64 >> 28);
            for _ in 0..clocks {
                self.clock_noise();
            }
        }
        sum > u32::MAX as u64
    }

    pub fn reset_phase(&mut self) {
        self.phase = 0;
    }

    fn clock_noise(&mut self) {
        noise_advance(&mut self.lfsr);
        self.noise_out = (self.lfsr & 0xFF) as f32 / 127.5 - 1.0;
    }
}

fn noise_advance(lfsr: &mut u32) {
    let mut v = *lfsr & 0xFFFF;
    let lsb = v & 1;
    v >>= 1;
    if lsb != 0 {
        v ^= 0xB400;
    }
    *lfsr = v;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn frequency_is_accurate() {
        let mut o = Oscillator::new(0);
        o.set_freq(441.0, 44_100.0);
        let wraps = (0..44_100).filter(|_| o.advance()).count();
        assert!((440..=441).contains(&wraps), "wraps = {wraps}");
        assert!((o.freq(44_100.0) - 441.0).abs() < 0.01);
    }

    #[test]
    fn frequency_is_clamped_below_nyquist() {
        let mut o = Oscillator::new(0);
        o.set_freq(1.0e9, 44_100.0);
        assert!(o.increment < 1 << 31);
    }

    #[test]
    fn saw_shape() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Saw;
        o.phase = 0;
        assert!(approx(o.sample(false), -1.0));
        o.phase = 0x8000_0000;
        assert!(approx(o.sample(false), 0.0));
    }

    #[test]
    fn triangle_shape_and_ring_mod() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Triangle;
        o.phase = 0x2000_0000;
        assert!(approx(o.sample(false), -0.5));
        // Ring mod flips the half of the cycle when the partner's msb is set.
        assert!(approx(o.sample(true), 0.5));
        o.phase = 0x4000_0000;
        assert!(approx(o.sample(false), 0.0));
    }

    #[test]
    fn pulse_width() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Pulse;
        o.set_pulse_width(0.25);
        o.phase = 0x1000_0000;
        assert_eq!(o.sample(false), 1.0);
        o.phase = 0x5000_0000;
        assert_eq!(o.sample(false), -1.0);
    }

    #[test]
    fn noise_lfsr_taps_match_acid_synth() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Noise;
        o.increment = 1 << 28; // exactly one LFSR clock per sample
        o.advance();
        // 0xACE1 >> 1 = 0x5670, lsb was 1, xor 0xB400 -> 0xE270.
        assert_eq!(o.lfsr, 0xE270);
        assert!(approx(o.sample(false), 0x70 as f32 / 127.5 - 1.0));
    }

    #[test]
    fn noise_clocks_sixteen_times_per_cycle() {
        let mut o = Oscillator::new(0);
        o.waveform = Waveform::Noise;
        o.set_freq(1000.0, 48_000.0);
        let mut changes = 0;
        let mut prev = o.lfsr;
        for _ in 0..48_000 {
            o.advance();
            if o.lfsr != prev {
                changes += 1;
                prev = o.lfsr;
            }
        }
        assert!((15_990..=16_010).contains(&changes), "changes = {changes}");
    }

    #[test]
    fn reset_phase_zeroes() {
        let mut o = Oscillator::new(0);
        o.phase = 1234;
        o.reset_phase();
        assert_eq!(o.phase, 0);
    }
}
