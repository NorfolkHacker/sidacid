//! DX7 voice data: the 32-voice bulk dump (4104 bytes of packed
//! 128-byte voices) and the single-voice dump (163 bytes, one unpacked
//! 155-byte voice), plus conversion to and from FM params.

use crate::params::{FmParam, OpParam, ParamId, fm_op_param, fm_param, fm_steps, step_index};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const BANK_VOICES: usize = 32;
const PACKED: usize = 128;
const UNPACKED: usize = 155;
const BANK_HEADER: [u8; 3] = [0x09, 0x20, 0x00];
const SINGLE_HEADER: [u8; 3] = [0x00, 0x01, 0x1B];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dx7Op {
    pub rates: [u8; 4],
    pub levels: [u8; 4],
    pub break_point: u8,
    pub left_depth: u8,
    pub right_depth: u8,
    pub left_curve: u8,
    pub right_curve: u8,
    pub rate_scaling: u8,
    pub amp_mod_sens: u8,
    pub vel_sens: u8,
    pub level: u8,
    pub fixed: bool,
    pub coarse: u8,
    pub fine: u8,
    /// 0..=14, 7 = centre.
    pub detune: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dx7Voice {
    /// `ops[0]` is OP1.
    pub ops: [Dx7Op; 6],
    pub pitch_rates: [u8; 4],
    pub pitch_levels: [u8; 4],
    /// 0..=31 (algorithm 1 is 0).
    pub algorithm: u8,
    pub feedback: u8,
    pub osc_sync: bool,
    pub lfo_speed: u8,
    pub lfo_delay: u8,
    pub lfo_pmd: u8,
    pub lfo_amd: u8,
    pub lfo_sync: bool,
    pub lfo_wave: u8,
    pub pms: u8,
    /// 0..=48, 24 = no transpose.
    pub transpose: u8,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysexError {
    NotDx7,
    Truncated,
    Checksum,
}

impl fmt::Display for SysexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SysexError::NotDx7 => "it is not a DX7 voice or 32-voice bank",
            SysexError::Truncated => "the file is cut short",
            SysexError::Checksum => "its checksum does not match (the file is damaged)",
        })
    }
}

impl std::error::Error for SysexError {}

fn init_op(level: u8) -> Dx7Op {
    Dx7Op {
        rates: [99; 4],
        levels: [99, 99, 99, 0],
        break_point: 39,
        left_depth: 0,
        right_depth: 0,
        left_curve: 0,
        right_curve: 0,
        rate_scaling: 0,
        amp_mod_sens: 0,
        vel_sens: 0,
        level,
        fixed: false,
        coarse: 1,
        fine: 0,
        detune: 7,
    }
}

impl Dx7Voice {
    pub fn init() -> Self {
        Self {
            ops: [
                init_op(99),
                init_op(0),
                init_op(0),
                init_op(0),
                init_op(0),
                init_op(0),
            ],
            pitch_rates: [99; 4],
            pitch_levels: [50; 4],
            algorithm: 0,
            feedback: 0,
            osc_sync: true,
            lfo_speed: 35,
            lfo_delay: 0,
            lfo_pmd: 0,
            lfo_amd: 0,
            lfo_sync: true,
            lfo_wave: 0,
            pms: 3,
            transpose: 24,
            name: "INIT VOICE".into(),
        }
    }

    /// Every field forced into its DX7 range.
    pub fn clamped(mut self) -> Self {
        for o in &mut self.ops {
            for x in o.rates.iter_mut().chain(o.levels.iter_mut()) {
                *x = (*x).min(99);
            }
            o.break_point = o.break_point.min(99);
            o.left_depth = o.left_depth.min(99);
            o.right_depth = o.right_depth.min(99);
            o.left_curve = o.left_curve.min(3);
            o.right_curve = o.right_curve.min(3);
            o.rate_scaling = o.rate_scaling.min(7);
            o.amp_mod_sens = o.amp_mod_sens.min(3);
            o.vel_sens = o.vel_sens.min(7);
            o.level = o.level.min(99);
            o.coarse = o.coarse.min(31);
            o.fine = o.fine.min(99);
            o.detune = o.detune.min(14);
        }
        for x in self
            .pitch_rates
            .iter_mut()
            .chain(self.pitch_levels.iter_mut())
        {
            *x = (*x).min(99);
        }
        self.algorithm = self.algorithm.min(31);
        self.feedback = self.feedback.min(7);
        self.lfo_speed = self.lfo_speed.min(99);
        self.lfo_delay = self.lfo_delay.min(99);
        self.lfo_pmd = self.lfo_pmd.min(99);
        self.lfo_amd = self.lfo_amd.min(99);
        self.lfo_wave = self.lfo_wave.min(5);
        self.pms = self.pms.min(7);
        self.transpose = self.transpose.min(48);
        self.name = self
            .name
            .chars()
            .filter(|c| (' '..='~').contains(c))
            .take(10)
            .collect::<String>()
            .trim_end()
            .to_string();
        self
    }

    pub fn is_valid(&self) -> bool {
        self.clone().clamped() == *self
    }

    /// The voice as FM params (0..1 values). Loading a voice also turns
    /// every operator on.
    pub fn to_params(&self) -> Vec<(ParamId, f32)> {
        use FmParam as F;
        use OpParam as O;
        let n99 = |x: u8| x as f32 / 99.0;
        let step = |x: u8, n: usize| x as f32 / (n - 1) as f32;
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        let mut out = vec![
            (
                fm_param(F::Algorithm),
                step(self.algorithm, fm_steps::ALGORITHM),
            ),
            (
                fm_param(F::Feedback),
                step(self.feedback, fm_steps::FEEDBACK),
            ),
            (fm_param(F::OscSync), flag(self.osc_sync)),
            (fm_param(F::PitchR1), n99(self.pitch_rates[0])),
            (fm_param(F::PitchR2), n99(self.pitch_rates[1])),
            (fm_param(F::PitchR3), n99(self.pitch_rates[2])),
            (fm_param(F::PitchR4), n99(self.pitch_rates[3])),
            (fm_param(F::PitchL1), n99(self.pitch_levels[0])),
            (fm_param(F::PitchL2), n99(self.pitch_levels[1])),
            (fm_param(F::PitchL3), n99(self.pitch_levels[2])),
            (fm_param(F::PitchL4), n99(self.pitch_levels[3])),
            (fm_param(F::LfoSpeed), n99(self.lfo_speed)),
            (fm_param(F::LfoDelay), n99(self.lfo_delay)),
            (fm_param(F::LfoPmd), n99(self.lfo_pmd)),
            (fm_param(F::LfoAmd), n99(self.lfo_amd)),
            (fm_param(F::LfoSync), flag(self.lfo_sync)),
            (
                fm_param(F::LfoWave),
                step(self.lfo_wave, fm_steps::LFO_WAVES),
            ),
            (
                fm_param(F::PitchModSens),
                step(self.pms, fm_steps::PITCH_MOD_SENS),
            ),
            (
                fm_param(F::Transpose),
                step(self.transpose, fm_steps::TRANSPOSE),
            ),
        ];
        for (op, o) in self.ops.iter().enumerate() {
            let id = |p| fm_op_param(op as u8, p);
            out.extend([
                (id(O::R1), n99(o.rates[0])),
                (id(O::R2), n99(o.rates[1])),
                (id(O::R3), n99(o.rates[2])),
                (id(O::R4), n99(o.rates[3])),
                (id(O::L1), n99(o.levels[0])),
                (id(O::L2), n99(o.levels[1])),
                (id(O::L3), n99(o.levels[2])),
                (id(O::L4), n99(o.levels[3])),
                (id(O::BreakPoint), n99(o.break_point)),
                (id(O::LeftDepth), n99(o.left_depth)),
                (id(O::RightDepth), n99(o.right_depth)),
                (id(O::LeftCurve), step(o.left_curve, fm_steps::CURVES)),
                (id(O::RightCurve), step(o.right_curve, fm_steps::CURVES)),
                (
                    id(O::RateScaling),
                    step(o.rate_scaling, fm_steps::RATE_SCALING),
                ),
                (
                    id(O::AmpModSens),
                    step(o.amp_mod_sens, fm_steps::AMP_MOD_SENS),
                ),
                (id(O::VelSens), step(o.vel_sens, fm_steps::VEL_SENS)),
                (id(O::Level), n99(o.level)),
                (id(O::FixedFreq), flag(o.fixed)),
                (id(O::Coarse), step(o.coarse, fm_steps::COARSE)),
                (id(O::Fine), n99(o.fine)),
                (id(O::Detune), step(o.detune, fm_steps::DETUNE)),
                (id(O::On), 1.0),
            ]);
        }
        out
    }

    /// The voice the current FM params describe.
    pub fn from_params(name: &str, value: impl Fn(ParamId) -> f32) -> Self {
        use FmParam as F;
        use OpParam as O;
        let d = |id| (value(id).clamp(0.0, 1.0) * 99.0).round() as u8;
        let s = |id, n: usize| step_index(value(id), n as u8) as u8;
        let b = |id| value(id) >= 0.5;
        let g = |p| fm_param(p);
        let mut v = Dx7Voice::init();
        v.name = name.to_string();
        v.algorithm = s(g(F::Algorithm), fm_steps::ALGORITHM);
        v.feedback = s(g(F::Feedback), fm_steps::FEEDBACK);
        v.osc_sync = b(g(F::OscSync));
        v.pitch_rates = [
            d(g(F::PitchR1)),
            d(g(F::PitchR2)),
            d(g(F::PitchR3)),
            d(g(F::PitchR4)),
        ];
        v.pitch_levels = [
            d(g(F::PitchL1)),
            d(g(F::PitchL2)),
            d(g(F::PitchL3)),
            d(g(F::PitchL4)),
        ];
        v.lfo_speed = d(g(F::LfoSpeed));
        v.lfo_delay = d(g(F::LfoDelay));
        v.lfo_pmd = d(g(F::LfoPmd));
        v.lfo_amd = d(g(F::LfoAmd));
        v.lfo_sync = b(g(F::LfoSync));
        v.lfo_wave = s(g(F::LfoWave), fm_steps::LFO_WAVES);
        v.pms = s(g(F::PitchModSens), fm_steps::PITCH_MOD_SENS);
        v.transpose = s(g(F::Transpose), fm_steps::TRANSPOSE);
        for (op, o) in v.ops.iter_mut().enumerate() {
            let id = |p| fm_op_param(op as u8, p);
            o.rates = [d(id(O::R1)), d(id(O::R2)), d(id(O::R3)), d(id(O::R4))];
            o.levels = [d(id(O::L1)), d(id(O::L2)), d(id(O::L3)), d(id(O::L4))];
            o.break_point = d(id(O::BreakPoint));
            o.left_depth = d(id(O::LeftDepth));
            o.right_depth = d(id(O::RightDepth));
            o.left_curve = s(id(O::LeftCurve), fm_steps::CURVES);
            o.right_curve = s(id(O::RightCurve), fm_steps::CURVES);
            o.rate_scaling = s(id(O::RateScaling), fm_steps::RATE_SCALING);
            o.amp_mod_sens = s(id(O::AmpModSens), fm_steps::AMP_MOD_SENS);
            o.vel_sens = s(id(O::VelSens), fm_steps::VEL_SENS);
            o.level = d(id(O::Level));
            o.fixed = b(id(O::FixedFreq));
            o.coarse = s(id(O::Coarse), fm_steps::COARSE);
            o.fine = d(id(O::Fine));
            o.detune = s(id(O::Detune), fm_steps::DETUNE);
        }
        v.clamped()
    }
}

fn checksum(data: &[u8]) -> u8 {
    let sum = data.iter().fold(0u32, |a, &b| a + b as u32);
    ((128 - sum % 128) % 128) as u8
}

fn name_from(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if (32..127).contains(&b) {
                b as char
            } else {
                ' '
            }
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn name_bytes(name: &str) -> [u8; 10] {
    let mut out = [b' '; 10];
    for (o, c) in out
        .iter_mut()
        .zip(name.bytes().filter(|b| (32..127).contains(b)))
    {
        *o = c;
    }
    out
}

pub fn pack_voice(v: &Dx7Voice) -> [u8; PACKED] {
    let mut b = [0u8; PACKED];
    for (op, o) in v.ops.iter().enumerate() {
        let k = (5 - op) * 17;
        b[k..k + 4].copy_from_slice(&o.rates);
        b[k + 4..k + 8].copy_from_slice(&o.levels);
        b[k + 8] = o.break_point;
        b[k + 9] = o.left_depth;
        b[k + 10] = o.right_depth;
        b[k + 11] = (o.left_curve & 3) | (o.right_curve & 3) << 2;
        b[k + 12] = (o.rate_scaling & 7) | (o.detune & 15) << 3;
        b[k + 13] = (o.amp_mod_sens & 3) | (o.vel_sens & 7) << 2;
        b[k + 14] = o.level;
        b[k + 15] = o.fixed as u8 | (o.coarse & 31) << 1;
        b[k + 16] = o.fine;
    }
    b[102..106].copy_from_slice(&v.pitch_rates);
    b[106..110].copy_from_slice(&v.pitch_levels);
    b[110] = v.algorithm & 31;
    b[111] = (v.feedback & 7) | (v.osc_sync as u8) << 3;
    b[112] = v.lfo_speed;
    b[113] = v.lfo_delay;
    b[114] = v.lfo_pmd;
    b[115] = v.lfo_amd;
    b[116] = v.lfo_sync as u8 | (v.lfo_wave & 7) << 1 | (v.pms & 7) << 4;
    b[117] = v.transpose;
    b[118..128].copy_from_slice(&name_bytes(&v.name));
    b
}

pub fn unpack_voice(b: &[u8; PACKED]) -> Dx7Voice {
    let mut v = Dx7Voice::init();
    for (op, o) in v.ops.iter_mut().enumerate() {
        let k = (5 - op) * 17;
        o.rates.copy_from_slice(&b[k..k + 4]);
        o.levels.copy_from_slice(&b[k + 4..k + 8]);
        o.break_point = b[k + 8];
        o.left_depth = b[k + 9];
        o.right_depth = b[k + 10];
        o.left_curve = b[k + 11] & 3;
        o.right_curve = (b[k + 11] >> 2) & 3;
        o.rate_scaling = b[k + 12] & 7;
        o.detune = (b[k + 12] >> 3) & 15;
        o.amp_mod_sens = b[k + 13] & 3;
        o.vel_sens = (b[k + 13] >> 2) & 7;
        o.level = b[k + 14];
        o.fixed = b[k + 15] & 1 != 0;
        o.coarse = (b[k + 15] >> 1) & 31;
        o.fine = b[k + 16];
    }
    v.pitch_rates.copy_from_slice(&b[102..106]);
    v.pitch_levels.copy_from_slice(&b[106..110]);
    v.algorithm = b[110] & 31;
    v.feedback = b[111] & 7;
    v.osc_sync = (b[111] >> 3) & 1 != 0;
    v.lfo_speed = b[112];
    v.lfo_delay = b[113];
    v.lfo_pmd = b[114];
    v.lfo_amd = b[115];
    v.lfo_sync = b[116] & 1 != 0;
    v.lfo_wave = (b[116] >> 1) & 7;
    v.pms = (b[116] >> 4) & 7;
    v.transpose = b[117];
    v.name = name_from(&b[118..128]);
    v.clamped()
}

fn to_unpacked(v: &Dx7Voice) -> [u8; UNPACKED] {
    let mut b = [0u8; UNPACKED];
    for (op, o) in v.ops.iter().enumerate() {
        let k = (5 - op) * 21;
        b[k..k + 4].copy_from_slice(&o.rates);
        b[k + 4..k + 8].copy_from_slice(&o.levels);
        b[k + 8..k + 21].copy_from_slice(&[
            o.break_point,
            o.left_depth,
            o.right_depth,
            o.left_curve,
            o.right_curve,
            o.rate_scaling,
            o.amp_mod_sens,
            o.vel_sens,
            o.level,
            o.fixed as u8,
            o.coarse,
            o.fine,
            o.detune,
        ]);
    }
    b[126..130].copy_from_slice(&v.pitch_rates);
    b[130..134].copy_from_slice(&v.pitch_levels);
    b[134..145].copy_from_slice(&[
        v.algorithm,
        v.feedback,
        v.osc_sync as u8,
        v.lfo_speed,
        v.lfo_delay,
        v.lfo_pmd,
        v.lfo_amd,
        v.lfo_sync as u8,
        v.lfo_wave,
        v.pms,
        v.transpose,
    ]);
    b[145..155].copy_from_slice(&name_bytes(&v.name));
    b
}

fn from_unpacked(b: &[u8; UNPACKED]) -> Dx7Voice {
    let mut v = Dx7Voice::init();
    for (op, o) in v.ops.iter_mut().enumerate() {
        let k = (5 - op) * 21;
        o.rates.copy_from_slice(&b[k..k + 4]);
        o.levels.copy_from_slice(&b[k + 4..k + 8]);
        o.break_point = b[k + 8];
        o.left_depth = b[k + 9];
        o.right_depth = b[k + 10];
        o.left_curve = b[k + 11];
        o.right_curve = b[k + 12];
        o.rate_scaling = b[k + 13];
        o.amp_mod_sens = b[k + 14];
        o.vel_sens = b[k + 15];
        o.level = b[k + 16];
        o.fixed = b[k + 17] & 1 != 0;
        o.coarse = b[k + 18];
        o.fine = b[k + 19];
        o.detune = b[k + 20];
    }
    v.pitch_rates.copy_from_slice(&b[126..130]);
    v.pitch_levels.copy_from_slice(&b[130..134]);
    v.algorithm = b[134];
    v.feedback = b[135];
    v.osc_sync = b[136] & 1 != 0;
    v.lfo_speed = b[137];
    v.lfo_delay = b[138];
    v.lfo_pmd = b[139];
    v.lfo_amd = b[140];
    v.lfo_sync = b[141] & 1 != 0;
    v.lfo_wave = b[142];
    v.pms = b[143];
    v.transpose = b[144];
    v.name = name_from(&b[145..155]);
    v.clamped()
}

fn message(header: [u8; 3], data: &[u8]) -> Vec<u8> {
    let mut out = vec![0xF0, 0x43, 0x00, header[0], header[1], header[2]];
    out.extend_from_slice(data);
    out.push(checksum(data));
    out.push(0xF7);
    out
}

/// A 32-voice bank dump; fewer voices are padded with INIT voices.
pub fn bank_sysex(voices: &[Dx7Voice]) -> Vec<u8> {
    let init = Dx7Voice::init();
    let mut data = Vec::with_capacity(BANK_VOICES * PACKED);
    for i in 0..BANK_VOICES {
        data.extend_from_slice(&pack_voice(voices.get(i).unwrap_or(&init)));
    }
    message(BANK_HEADER, &data)
}

pub fn single_sysex(voice: &Dx7Voice) -> Vec<u8> {
    message(SINGLE_HEADER, &to_unpacked(voice))
}

/// The voices in a DX7 bank (32) or single-voice (1) dump. Bytes after
/// the first message are ignored.
pub fn parse(bytes: &[u8]) -> Result<Vec<Dx7Voice>, SysexError> {
    if bytes.len() < 6 || bytes[0] != 0xF0 || bytes[1] != 0x43 || bytes[2] & 0xF0 != 0 {
        return Err(SysexError::NotDx7);
    }
    let header = [bytes[3], bytes[4], bytes[5]];
    let len = match header {
        BANK_HEADER => BANK_VOICES * PACKED,
        SINGLE_HEADER => UNPACKED,
        _ => return Err(SysexError::NotDx7),
    };
    if bytes.len() < 6 + len + 1 {
        return Err(SysexError::Truncated);
    }
    let data = &bytes[6..6 + len];
    if checksum(data) != bytes[6 + len] {
        return Err(SysexError::Checksum);
    }
    Ok(if header == BANK_HEADER {
        data.as_chunks::<PACKED>()
            .0
            .iter()
            .map(unpack_voice)
            .collect()
    } else {
        vec![from_unpacked(data.try_into().expect("155 bytes"))]
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    fn odd_voice(seed: u8) -> Dx7Voice {
        let mut v = Dx7Voice::init();
        v.name = format!("VOICE {seed}");
        v.algorithm = seed % 32;
        v.feedback = seed % 8;
        v.osc_sync = seed.is_multiple_of(2);
        v.lfo_wave = seed % 6;
        v.pms = seed % 8;
        v.transpose = seed % 49;
        v.pitch_levels = [seed % 100, 50, 60, 70];
        for (i, o) in v.ops.iter_mut().enumerate() {
            let k = seed.wrapping_add(i as u8 * 7);
            o.rates = [k % 100, 99 - k % 100, 40, 60];
            o.levels = [99, k % 100, 50, 0];
            o.left_curve = k % 4;
            o.right_curve = (k + 1) % 4;
            o.rate_scaling = k % 8;
            o.detune = k % 15;
            o.amp_mod_sens = k % 4;
            o.vel_sens = k % 8;
            o.fixed = k.is_multiple_of(3);
            o.coarse = k % 32;
            o.fine = k % 100;
            o.level = 99 - k % 50;
            o.break_point = k % 100;
            o.left_depth = k % 100;
            o.right_depth = (k + 3) % 100;
        }
        v
    }

    #[test]
    fn packed_voices_round_trip() {
        for seed in 0..40 {
            let v = odd_voice(seed);
            assert_eq!(unpack_voice(&pack_voice(&v)), v, "seed {seed}");
        }
        let init = Dx7Voice::init();
        assert_eq!(unpack_voice(&pack_voice(&init)), init);
    }

    #[test]
    fn banks_and_single_voices_round_trip() {
        let voices: Vec<Dx7Voice> = (0..32).map(odd_voice).collect();
        let bytes = bank_sysex(&voices);
        assert_eq!(bytes.len(), 4104);
        assert_eq!(&bytes[..6], &[0xF0, 0x43, 0x00, 0x09, 0x20, 0x00]);
        assert_eq!(*bytes.last().unwrap(), 0xF7);
        assert_eq!(parse(&bytes).unwrap(), voices);
        let one = single_sysex(&voices[5]);
        assert_eq!(one.len(), 163);
        assert_eq!(parse(&one).unwrap(), vec![voices[5].clone()]);
        let short_bank = bank_sysex(&voices[..3]);
        let parsed = parse(&short_bank).unwrap();
        assert_eq!(parsed.len(), 32);
        assert_eq!(parsed[3], Dx7Voice::init(), "padded with INIT voices");
    }

    #[test]
    fn bad_files_are_refused() {
        let mut bytes = bank_sysex(&[Dx7Voice::init()]);
        bytes[100] ^= 1;
        assert_eq!(parse(&bytes), Err(SysexError::Checksum));
        assert_eq!(parse(&bank_sysex(&[])[..2000]), Err(SysexError::Truncated));
        assert_eq!(parse(b"hello"), Err(SysexError::NotDx7));
        assert_eq!(parse(&[0xF0, 0x41, 0, 0, 0, 0, 0]), Err(SysexError::NotDx7));
        assert!(!SysexError::Checksum.to_string().is_empty());
    }

    #[test]
    fn out_of_range_bytes_are_clamped() {
        let mut b = pack_voice(&Dx7Voice::init());
        b[0] = 127; // OP6 rate 1
        b[116] = 0x7F; // wave 7, pms 7, sync
        b[110] = 0x7F; // algorithm
        let v = unpack_voice(&b);
        assert!(v.is_valid());
        assert_eq!(v.ops[5].rates[0], 99);
        assert_eq!(v.lfo_wave, 5);
        assert_eq!(v.algorithm, 31);
    }

    #[test]
    fn params_round_trip_and_cover_every_voice_control() {
        let v = odd_voice(11);
        let params = v.to_params();
        assert_eq!(params.len(), 19 + 6 * 22);
        let map: std::collections::HashMap<ParamId, f32> = params.into_iter().collect();
        assert_eq!(map[&fm_op_param(3, crate::params::OpParam::On)], 1.0);
        let back = Dx7Voice::from_params(&v.name, |id| map[&id]);
        assert_eq!(back, v);
    }

    #[test]
    fn names_are_ascii_and_trimmed() {
        let mut v = Dx7Voice::init();
        v.name = "PIANO   ".into();
        let back = unpack_voice(&pack_voice(&v));
        assert_eq!(back.name, "PIANO");
        let printable = |n: &str| n.len() <= 10 && n.chars().all(|c| (' '..='~').contains(&c));
        v.name = "ÉLAN PIANO EXTRA".into();
        let back = unpack_voice(&pack_voice(&v));
        assert_eq!(back.name, "LAN PIANO");
        assert!(printable(&back.name) && back.is_valid());
        let clamped = v.clone().clamped();
        assert_eq!(clamped.name, "LAN PIANO");
        assert!(printable(&clamped.name));
        // DEL and NUL in the packed name read as spaces, trailing ones trimmed.
        let mut b = pack_voice(&Dx7Voice::init());
        b[118..128].copy_from_slice(b"AB\x7FC\0\0\0\0\0\0");
        assert_eq!(unpack_voice(&b).name, "AB C");
    }

    #[test]
    fn a_name_from_params_is_clamped() {
        let init: std::collections::HashMap<ParamId, f32> =
            Dx7Voice::init().to_params().into_iter().collect();
        let v = Dx7Voice::from_params("ÄBCDEFGHIJKLMNOP  ", |id| init[&id]);
        assert_eq!(v.name, "BCDEFGHIJK");
        assert!(v.is_valid());
    }

    #[test]
    fn packed_layout_is_the_dx7s() {
        // Byte layout from the DX7 VMEM (packed 32-voice) format: OP6
        // first, 17 bytes per operator, then the voice parameters.
        let mut v = Dx7Voice::init();
        let op6 = &mut v.ops[5];
        op6.rates = [10, 20, 30, 40];
        op6.levels = [91, 81, 71, 61];
        op6.break_point = 45;
        op6.left_depth = 12;
        op6.right_depth = 34;
        op6.left_curve = 1;
        op6.right_curve = 2;
        op6.rate_scaling = 5;
        op6.detune = 9;
        op6.amp_mod_sens = 2;
        op6.vel_sens = 6;
        op6.level = 88;
        op6.fixed = true;
        op6.coarse = 17;
        op6.fine = 42;
        v.ops[0].coarse = 3;
        v.ops[0].level = 77;
        v.pitch_rates = [1, 2, 3, 4];
        v.pitch_levels = [5, 6, 7, 8];
        v.algorithm = 21;
        v.feedback = 5;
        v.osc_sync = true;
        v.lfo_speed = 11;
        v.lfo_delay = 22;
        v.lfo_pmd = 33;
        v.lfo_amd = 44;
        v.lfo_sync = true;
        v.lfo_wave = 4;
        v.pms = 6;
        v.transpose = 30;
        v.name = "ABC".into();
        let b = pack_voice(&v);
        assert_eq!(
            &b[0..8],
            &[10, 20, 30, 40, 91, 81, 71, 61],
            "OP6 rates, levels"
        );
        assert_eq!(&b[8..11], &[45, 12, 34], "break point, depths");
        assert_eq!(b[11], (2 << 2) | 1, "RC << 2 | LC");
        assert_eq!(b[12], (9 << 3) | 5, "DET << 3 | RS");
        assert_eq!(b[13], (6 << 2) | 2, "KVS << 2 | AMS");
        assert_eq!(b[14], 88, "output level");
        assert_eq!(b[15], (17 << 1) | 1, "FC << 1 | mode");
        assert_eq!(b[16], 42, "fine");
        assert_eq!(b[85 + 14], 77, "OP1 is last");
        assert_eq!(b[85 + 15], 3 << 1, "OP1 coarse, ratio mode");
        assert_eq!(&b[102..110], &[1, 2, 3, 4, 5, 6, 7, 8], "pitch EG");
        assert_eq!(b[110], 21, "algorithm");
        assert_eq!(b[111], (1 << 3) | 5, "OKS << 3 | FB");
        assert_eq!(
            &b[112..116],
            &[11, 22, 33, 44],
            "LFO speed, delay, PMD, AMD"
        );
        assert_eq!(
            b[116],
            (6 << 4) | (4 << 1) | 1,
            "LPMS << 4 | LFW << 1 | LKS"
        );
        assert_eq!(b[117], 30, "transpose");
        assert_eq!(&b[118..128], b"ABC       ", "name, space padded");

        // Two's complement of the 7-bit sum: 1 + 2 + 3 = 6, 128 - 6 = 122.
        assert_eq!(checksum(&[1, 2, 3]), 122);
        // A bank of INIT voices, summed by hand from the layout above.
        let bank = bank_sysex(&[]);
        assert_eq!(bank[6 + 4095], b'E', "the last name byte of voice 32");
        assert_eq!(bank[6 + 4096], 96, "the bank checksum");
    }
}
