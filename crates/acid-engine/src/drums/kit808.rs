//! 808-flavoured SID drum patches: long triangle booms, two-tone snare,
//! burst clap, noise hats. Knobs are normalised 0..1.

use super::{BD, CH, CP, DrumPatch, HT, LT, OH, Osc2Mode, RS, SD};
use sid_core::{FILTER_BP, FILTER_HP, Waveform};

/// Tune knob: one octave down to one octave up.
fn tune_factor(tune: f32) -> f32 {
    2f32.powf((tune - 0.5) * 2.0)
}

pub fn patch(inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch {
    let tf = tune_factor(tune);
    let base = DrumPatch::default();
    match inst {
        BD => DrumPatch {
            tone_wave: Waveform::Triangle,
            tone_end_hz: 50.0 * tf,
            tone_start_hz: 50.0 * tf * (1.5 + tone * 3.0),
            pitch_decay_s: 0.03,
            tone_decay_s: 0.15 + decay * 1.2,
            tone_level: 1.0,
            ..base
        },
        SD => DrumPatch {
            tone_wave: Waveform::Triangle,
            tone_start_hz: 330.0 * tf,
            tone_end_hz: 180.0 * tf,
            pitch_decay_s: 0.01,
            tone_decay_s: 0.12 + decay * 0.1,
            tone_level: 0.8 * (1.0 - tone * 0.6),
            osc2_mode: Osc2Mode::Add,
            osc2_ratio: 1.48,
            noise_level: 0.4 + tone * 0.6,
            noise_decay_s: 0.1 + decay * 0.25,
            noise_filter_mode: FILTER_HP,
            noise_cutoff_hz: 1500.0,
            noise_res: 0.2,
            ..base
        },
        LT | HT => {
            let f = if inst == LT { 110.0 } else { 175.0 } * tf;
            DrumPatch {
                tone_wave: Waveform::Triangle,
                tone_start_hz: f * (1.3 + tone * 0.7),
                tone_end_hz: f,
                pitch_decay_s: 0.05,
                tone_decay_s: 0.2 + decay * 0.6,
                tone_level: 0.9,
                noise_level: 0.05,
                noise_decay_s: 0.05,
                noise_cutoff_hz: 3000.0,
                ..base
            }
        }
        CP => DrumPatch {
            tone_level: 0.0,
            noise_level: 0.9,
            noise_decay_s: 0.1 + decay * 0.4,
            noise_filter_mode: FILTER_BP,
            noise_cutoff_hz: 1000.0 + tone * 1500.0,
            noise_res: 0.3,
            bursts: 3,
            burst_spacing_s: 0.011,
            ..base
        },
        RS => DrumPatch {
            tone_wave: Waveform::Pulse,
            tone_start_hz: 1700.0 * tf,
            tone_end_hz: 1700.0 * tf,
            tone_decay_s: 0.025 + decay * 0.02,
            tone_level: 0.5 + tone * 0.3,
            ..base
        },
        CH | OH => DrumPatch {
            tone_level: 0.0,
            noise_level: 0.7,
            noise_decay_s: if inst == CH {
                0.03 + decay * 0.08
            } else {
                0.25 + decay * 0.8
            },
            noise_filter_mode: FILTER_HP,
            noise_cutoff_hz: (6000.0 + tone * 5000.0) * tf.sqrt(),
            noise_res: 0.1,
            ..base
        },
        _ => base,
    }
}
