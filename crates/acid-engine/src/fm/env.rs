//! DX7 operator envelope plus output-level, keyboard and velocity
//! scaling. The math follows the published reverse-engineering of the
//! DX7 in music-synthesizer-for-android (Apache-2.0, used by Dexed); see
//! THIRD_PARTY_NOTICES.md. Envelope levels live in a log2 domain: one
//! unit doubles the amplitude, and `tick` returns 0.0 at full scale.

use super::BLOCK;

const LEVEL_LUT: [f32; 20] = [
    0.0, 5.0, 9.0, 13.0, 17.0, 20.0, 23.0, 25.0, 27.0, 29.0, 31.0, 33.0, 35.0, 37.0, 39.0, 41.0,
    42.0, 43.0, 45.0, 46.0,
];
const VELOCITY_DATA: [u8; 64] = [
    0, 70, 86, 97, 106, 114, 121, 126, 132, 138, 142, 148, 152, 156, 160, 163, 166, 170, 173, 174,
    178, 181, 184, 186, 189, 190, 194, 196, 198, 200, 202, 205, 206, 209, 211, 214, 216, 218, 220,
    222, 224, 225, 227, 229, 230, 232, 233, 235, 237, 238, 240, 241, 242, 243, 244, 246, 246, 248,
    249, 250, 251, 252, 253, 254,
];
const EXP_SCALE: [u8; 33] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 14, 16, 19, 23, 27, 33, 39, 47, 56, 66, 80, 94, 110, 126,
    142, 158, 174, 190, 206, 222, 238, 250,
];
/// Envelope units at full amplitude: L=99 at output level 99 gives
/// (63 * 64 + 127 * 32 - 4256) / 256 = 15.0.
const FULL: f32 = 15.0;
/// Where a rising stage jumps to first (the DX7's fast attack onset).
const ATTACK_JUMP: f32 = 1716.0 / 256.0;

/// A DX7 0..99 level on the internal 0..127 scale (continuous).
pub fn scale_out_level(x: f32) -> f32 {
    let x = x.clamp(0.0, 99.0);
    if x >= 20.0 {
        return 28.0 + x;
    }
    let i = x.floor() as usize;
    let next = if i + 1 < LEVEL_LUT.len() {
        LEVEL_LUT[i + 1]
    } else {
        48.0
    };
    LEVEL_LUT[i] + (next - LEVEL_LUT[i]) * (x - i as f32)
}

fn scale_curve(group: i32, depth: i32, curve: i32) -> i32 {
    let scale = if curve == 0 || curve == 3 {
        (group * depth * 329) >> 12
    } else {
        let raw = EXP_SCALE[(group.max(0) as usize).min(EXP_SCALE.len() - 1)] as i32;
        (raw * depth * 329) >> 15
    };
    if curve < 2 { -scale } else { scale }
}

/// Keyboard level scaling for MIDI `note`. Curves: 0 -LIN, 1 -EXP, 2 +EXP, 3 +LIN.
pub fn scale_level(
    note: i32,
    break_point: i32,
    left_depth: i32,
    right_depth: i32,
    left_curve: i32,
    right_curve: i32,
) -> i32 {
    let offset = note - break_point - 17;
    if offset >= 0 {
        scale_curve((offset + 1) / 3, right_depth, right_curve)
    } else {
        scale_curve(-(offset - 1) / 3, left_depth, left_curve)
    }
}

/// Keyboard rate scaling: added to the envelope's rate index.
pub fn scale_rate(note: i32, sensitivity: i32) -> i32 {
    let x = (note / 3 - 7).clamp(0, 31);
    (sensitivity * x) >> 3
}

/// Velocity sensitivity, in output-level units.
pub fn scale_velocity(velocity: i32, sensitivity: i32) -> i32 {
    let v = VELOCITY_DATA[(velocity.clamp(0, 127) >> 1) as usize] as i32 - 239;
    ((sensitivity * v + 7) >> 3) << 4
}

/// An operator's output level in envelope units, from its 0..99 level
/// and its keyboard and velocity scaling.
pub fn operator_out_level(level: f32, level_scaling: i32, velocity_scaling: i32) -> f32 {
    let ol = (scale_out_level(level) + level_scaling as f32).min(127.0) * 32.0;
    (ol + velocity_scaling as f32).max(0.0)
}

/// A DX7 four-rate, four-level envelope, advanced once per (sub-)block.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    rates: [f32; 4],
    levels: [f32; 4],
    out_level: f32,
    rate_scaling: i32,
    lvl: f32,
    target: f32,
    stage: usize,
    rising: bool,
    inc: f32,
    down: bool,
    /// Envelope units per raw DX7 increment, for one full block at this rate.
    inc_scale: f32,
}

impl Env {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            rates: [99.0; 4],
            levels: [99.0, 99.0, 99.0, 0.0],
            out_level: 0.0,
            rate_scaling: 0,
            lvl: 0.0,
            target: 0.0,
            stage: 4,
            rising: false,
            inc: 0.0,
            down: false,
            inc_scale: BLOCK as f32 / 64.0 * 44_100.0 / sample_rate / (1u32 << 24) as f32,
        }
    }

    /// Key down: starts the attack from silence.
    pub fn start(&mut self, rates: [f32; 4], levels: [f32; 4], out_level: f32, rate_scaling: i32) {
        self.rates = rates;
        self.levels = levels;
        self.out_level = out_level;
        self.rate_scaling = rate_scaling;
        self.lvl = 0.0;
        self.down = true;
        self.advance(0);
    }

    /// Key up: moves to the release stage.
    pub fn release(&mut self) {
        if self.down {
            self.down = false;
            self.advance(3);
        }
    }

    /// A new output level (a knob moved while the note plays).
    pub fn set_out_level(&mut self, out_level: f32) {
        self.out_level = out_level;
        if self.down && self.stage == 3 {
            self.lvl = self.actual(self.levels[2]);
        } else if self.stage < 4 {
            self.target = self.actual(self.levels[self.stage]);
            self.rising = self.target > self.lvl;
        } else {
            // Through the release: hold L4 at the new output level, so a
            // drone (L4 above silence) follows the Level knob.
            self.lvl = self.actual(self.levels[3]);
        }
    }

    /// New rates, levels or rate scaling (a knob moved while the note
    /// plays): the current stage heads for its new level at its new rate.
    /// A new sustain level is approached at R3, not jumped to.
    pub fn set_shape(&mut self, rates: [f32; 4], levels: [f32; 4], rate_scaling: i32) {
        if rates == self.rates && levels == self.levels && rate_scaling == self.rate_scaling {
            return;
        }
        self.rates = rates;
        self.levels = levels;
        self.rate_scaling = rate_scaling;
        if self.down && self.stage == 3 {
            if self.actual(levels[2]) != self.lvl {
                self.advance(2);
            }
        } else if self.stage < 4 {
            self.advance(self.stage);
        } else {
            // Through the release: hold the new L4, as `set_out_level` does.
            self.lvl = self.actual(levels[3]);
        }
    }

    pub fn is_down(&self) -> bool {
        self.down
    }

    /// Released and through its release stage.
    pub fn is_finished(&self) -> bool {
        !self.down && self.stage >= 4
    }

    fn actual(&self, level: f32) -> f32 {
        let a = (scale_out_level(level) * 0.5).floor() * 64.0 + self.out_level - 4256.0;
        a.max(16.0) / 256.0
    }

    fn advance(&mut self, stage: usize) {
        self.stage = stage;
        if stage < 4 {
            self.target = self.actual(self.levels[stage]);
            self.rising = self.target > self.lvl;
            let q =
                ((self.rates[stage] * 41.0 / 64.0).floor() as i32 + self.rate_scaling).clamp(0, 63);
            let raw = (4 + (q & 3)) as f32 * 2f32.powi(2 + 6 + (q >> 2));
            self.inc = raw * self.inc_scale;
        }
    }

    /// Advances `n` samples (at most one `BLOCK`). Returns the level as
    /// log2 amplitude (0.0 = full scale).
    pub fn tick(&mut self, n: usize) -> f32 {
        let inc = self.inc * n.min(BLOCK) as f32 / BLOCK as f32;
        if self.stage < 3 || (self.stage < 4 && !self.down) {
            if self.rising {
                if self.lvl < ATTACK_JUMP {
                    self.lvl = ATTACK_JUMP;
                }
                self.lvl += (17.0 - self.lvl.floor()) * inc;
                if self.lvl >= self.target {
                    self.lvl = self.target;
                    self.advance(self.stage + 1);
                }
            } else {
                self.lvl -= inc;
                if self.lvl <= self.target {
                    self.lvl = self.target;
                    self.advance(self.stage + 1);
                }
            }
        }
        self.lvl - FULL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn blocks(seconds: f32) -> usize {
        (seconds * SR / BLOCK as f32).ceil() as usize
    }

    fn full_env(rates: [f32; 4], levels: [f32; 4]) -> Env {
        let mut e = Env::new(SR);
        e.start(rates, levels, operator_out_level(99.0, 0, 0), 0);
        e
    }

    #[test]
    fn output_level_scale() {
        assert_eq!(scale_out_level(99.0), 127.0);
        assert_eq!(scale_out_level(20.0), 48.0);
        assert_eq!(scale_out_level(0.0), 0.0);
        let mut last = -1.0;
        for i in 0..=990 {
            let s = scale_out_level(i as f32 / 10.0);
            assert!(s >= last);
            last = s;
        }
    }

    #[test]
    fn fast_attack_reaches_full_level_and_sustains() {
        let mut e = full_env([99.0; 4], [99.0, 99.0, 99.0, 0.0]);
        let mut level = f32::MIN;
        for _ in 0..blocks(0.02) {
            level = e.tick(BLOCK);
        }
        assert!(level.abs() < 0.05, "level {level}");
        for _ in 0..blocks(1.0) {
            level = e.tick(BLOCK);
        }
        assert!(level.abs() < 0.05, "sustains at L3");
        assert!(e.is_down() && !e.is_finished());
    }

    #[test]
    fn release_falls_silent_and_finishes() {
        let mut e = full_env([99.0; 4], [99.0, 99.0, 99.0, 0.0]);
        for _ in 0..blocks(0.05) {
            e.tick(BLOCK);
        }
        e.release();
        let mut level = 0.0;
        for _ in 0..blocks(0.05) {
            level = e.tick(BLOCK);
        }
        assert!(level < -12.0, "level {level}");
        assert!(e.is_finished());
    }

    #[test]
    fn slower_rates_take_longer() {
        let attack_blocks = |rate: f32| {
            let mut e = full_env([rate, 99.0, 99.0, 99.0], [99.0, 99.0, 99.0, 0.0]);
            (0..blocks(30.0))
                .position(|_| e.tick(BLOCK) > -0.1)
                .unwrap()
        };
        assert!(attack_blocks(50.0) > attack_blocks(70.0));
        assert!(attack_blocks(70.0) > attack_blocks(99.0));
    }

    #[test]
    fn rate_timing_matches_msfa_reference() {
        // msfa (env.cc): qrate = rate * 41 >> 6 and each 64-sample block at
        // 44.1 kHz moves the level by (4 + (qrate & 3)) << (2 + 6 +
        // (qrate >> 2)) in Q24 units of 1/256 of an envelope unit.
        // R=50: qrate 32, so 4 << 16 per block = 1/64 unit per 64 samples,
        // 44100 / 64 / 64 = 10.766 units/s. L1=99 to L2=0 at full output
        // level spans 15.0 - 16/256 = 14.9375 units: 1.387 s.
        let reference = 14.9375 / (44_100.0 / 64.0 / 64.0);
        let mut e = full_env([99.0, 50.0, 99.0, 99.0], [99.0, 0.0, 0.0, 0.0]);
        let mut attack = None;
        let mut decay_end = None;
        for b in 1..=blocks(5.0) {
            e.tick(BLOCK);
            if attack.is_none() && e.stage >= 1 {
                attack = Some(b);
            }
            if e.stage >= 2 {
                decay_end = Some(b);
                break;
            }
        }
        let secs = |b: usize| (b * BLOCK) as f32 / SR;
        // R1=99: qrate 63, a jump to the onset then one 64-sample step at
        // 44.1 kHz covers the rest, so the attack is done in under 2 ms.
        let attack = secs(attack.unwrap());
        assert!(attack < 0.002, "attack took {attack} s");
        let decay = secs(decay_end.unwrap()) - attack;
        assert!(
            (decay - reference).abs() < reference * 0.1,
            "decay {decay} s, msfa reference {reference} s"
        );
    }

    #[test]
    fn a_zero_level_is_silent() {
        let mut e = Env::new(SR);
        e.start(
            [99.0; 4],
            [99.0, 99.0, 99.0, 0.0],
            operator_out_level(0.0, 0, 0),
            0,
        );
        let level = (0..blocks(0.05)).map(|_| e.tick(BLOCK)).last().unwrap();
        assert!(level < -13.0, "level {level}");
    }

    #[test]
    fn raising_the_output_level_while_sustaining_takes_effect() {
        let mut e = Env::new(SR);
        e.start(
            [99.0; 4],
            [99.0, 99.0, 99.0, 0.0],
            operator_out_level(50.0, 0, 0),
            0,
        );
        let quiet = (0..blocks(0.1)).map(|_| e.tick(BLOCK)).last().unwrap();
        e.set_out_level(operator_out_level(99.0, 0, 0));
        let loud = e.tick(BLOCK);
        assert!(loud > quiet + 1.0, "{quiet} -> {loud}");
    }

    #[test]
    fn envelope_speed_does_not_depend_on_block_size() {
        let rates = [60.0, 55.0, 50.0, 45.0];
        let levels = [99.0, 70.0, 80.0, 0.0];
        let mut a = full_env(rates, levels);
        let mut b = full_env(rates, levels);
        // 224 samples = 7 blocks of 32 = 32 blocks of 7.
        let mut worst = 0.0f32;
        for step in 0..400 {
            if step == 250 {
                a.release();
                b.release();
            }
            let la = (0..7).map(|_| a.tick(32)).last().unwrap();
            let lb = (0..32).map(|_| b.tick(7)).last().unwrap();
            worst = worst.max((la - lb).abs());
        }
        assert!(worst < 0.1, "levels differ by {worst} log2 units");
    }

    #[test]
    fn a_finished_drone_follows_the_output_level() {
        // L4 above silence: the released note holds L4 for ever.
        let mut e = full_env([99.0; 4], [99.0, 99.0, 99.0, 99.0]);
        e.tick(BLOCK);
        e.release();
        let drone = (0..blocks(0.05)).map(|_| e.tick(BLOCK)).last().unwrap();
        assert!(e.is_finished() && drone > -0.1, "drone at {drone}");
        e.set_out_level(operator_out_level(0.0, 0, 0));
        let level = e.tick(BLOCK);
        assert!(level < -13.0, "level 0 silences the drone: {level}");
    }

    #[test]
    fn a_new_sustain_level_is_approached_at_rate_3_not_jumped_to() {
        let rates = [99.0, 99.0, 30.0, 99.0];
        let mut e = full_env(rates, [99.0, 99.0, 99.0, 0.0]);
        let full = (0..blocks(0.05)).map(|_| e.tick(BLOCK)).last().unwrap();
        e.set_shape(rates, [99.0, 99.0, 50.0, 0.0], 0);
        let next = e.tick(BLOCK);
        assert!(full - next < 0.1, "no jump: {full} -> {next}");
        let settled = (0..blocks(10.0)).map(|_| e.tick(BLOCK)).last().unwrap();
        let want = full_env(rates, [99.0, 99.0, 50.0, 0.0]).actual(50.0) - FULL;
        assert!((settled - want).abs() < 0.01, "{settled} vs {want}");
        assert!(e.is_down() && e.stage == 3, "back in the sustain");
    }

    #[test]
    fn scaling_helpers() {
        assert_eq!(scale_rate(60, 0), 0);
        assert_eq!(scale_rate(127, 7), 27);
        assert_eq!(scale_velocity(127, 0), 0);
        assert_eq!(scale_velocity(127, 7), 224);
        assert!(scale_velocity(40, 7) < scale_velocity(100, 7));
        assert_eq!(scale_level(100, 39, 0, 0, 0, 0), 0);
        assert!(
            scale_level(100, 39, 0, 99, 0, 3) > 0,
            "+LIN raises above the break point"
        );
        assert!(
            scale_level(100, 39, 0, 99, 0, 0) < 0,
            "-LIN lowers above the break point"
        );
        assert!(
            scale_level(20, 39, 99, 0, 1, 0) < 0,
            "-EXP lowers below the break point"
        );
        assert_eq!(
            scale_level(20, 39, 99, 0, 1, 0),
            scale_level(20, 39, 99, 99, 1, 3),
            "right side unused below"
        );
    }
}
