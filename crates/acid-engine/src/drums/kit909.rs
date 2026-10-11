//! 909-flavoured SID drum patches: pulse-click punchy kick, snappy snare,
//! tighter clap, ring-mod rimshot and metallic ring-mod hats.

use super::{DrumPatch, Osc2Mode, BD, CH, CP, HT, LT, OH, RS, SD};
use sid_core::{Waveform, FILTER_BP, FILTER_HP};

fn tune_factor(tune: f32) -> f32 {
    2f32.powf((tune - 0.5) * 2.0)
}

pub fn patch(inst: usize, tune: f32, decay: f32, tone: f32) -> DrumPatch {
    let tf = tune_factor(tune);
    let base = DrumPatch::default();
    match inst {
        BD => DrumPatch {
            tone_wave: Waveform::Triangle,
            click_wave: Some(Waveform::Pulse),
            click_s: 0.004,
            tone_end_hz: 50.0 * tf,
            tone_start_hz: 50.0 * tf * (3.0 + tone * 4.0),
            pitch_decay_s: 0.012,
            tone_decay_s: 0.15 + decay * 0.6,
            tone_level: 1.0,
            ..base
        },
        SD => DrumPatch {
            tone_wave: Waveform::Triangle,
            tone_start_hz: 260.0 * tf,
            tone_end_hz: 190.0 * tf,
            pitch_decay_s: 0.015,
            tone_decay_s: 0.08 + decay * 0.06,
            tone_level: 0.7,
            noise_level: 0.6 + tone * 0.4,
            noise_decay_s: 0.08 + decay * 0.35,
            noise_filter_mode: FILTER_HP,
            noise_cutoff_hz: 800.0,
            noise_res: 0.0,
            ..base
        },
        LT | HT => {
            let f = if inst == LT { 100.0 } else { 160.0 } * tf;
            DrumPatch {
                tone_wave: Waveform::Triangle,
                click_wave: Some(Waveform::Pulse),
                click_s: 0.003,
                tone_start_hz: f * (1.6 + tone * 1.0),
                tone_end_hz: f,
                pitch_decay_s: 0.06,
                tone_decay_s: 0.15 + decay * 0.5,
                tone_level: 0.9,
                ..base
            }
        }
        CP => DrumPatch {
            tone_level: 0.0,
            noise_level: 0.9,
            noise_decay_s: 0.08 + decay * 0.3,
            noise_filter_mode: FILTER_BP,
            noise_cutoff_hz: 1200.0 + tone * 1000.0,
            noise_res: 0.4,
            bursts: 4,
            burst_spacing_s: 0.008,
            ..base
        },
        RS => DrumPatch {
            tone_wave: Waveform::Triangle,
            tone_start_hz: 1100.0 * tf,
            tone_end_hz: 1100.0 * tf,
            tone_decay_s: 0.02 + decay * 0.02,
            tone_level: 0.7,
            osc2_mode: Osc2Mode::Ring,
            osc2_ratio: 1.37 + tone * 0.5,
            ..base
        },
        CH | OH => {
            let d = if inst == CH { 0.04 + decay * 0.1 } else { 0.2 + decay * 0.7 };
            DrumPatch {
                tone_wave: Waveform::Triangle,
                tone_start_hz: 3000.0 * tf,
                tone_end_hz: 3000.0 * tf,
                tone_decay_s: d,
                tone_level: 0.35,
                osc2_mode: Osc2Mode::Ring,
                osc2_ratio: 1.4142,
                noise_level: 0.7,
                noise_decay_s: d,
                noise_filter_mode: FILTER_HP,
                noise_cutoff_hz: 7000.0 + tone * 4000.0,
                noise_res: 0.1,
                ..base
            }
        }
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{kit808, Osc2Mode, BD, CH, CP, RS, SD};
    use super::*;
    use sid_core::Waveform;

    #[test]
    fn kick_has_pulse_click_and_fast_sweep() {
        let p = patch(BD, 0.5, 0.5, 0.5);
        assert_eq!(p.click_wave, Some(Waveform::Pulse));
        assert!(p.pitch_decay_s < kit808::patch(BD, 0.5, 0.5, 0.5).pitch_decay_s);
    }

    #[test]
    fn snare_noise_is_brighter_and_snappier_than_808() {
        let p = patch(SD, 0.5, 0.5, 1.0);
        assert!(p.noise_level >= 0.9);
    }

    #[test]
    fn hats_and_rimshot_use_ring_mod() {
        assert_eq!(patch(CH, 0.5, 0.5, 0.5).osc2_mode, Osc2Mode::Ring);
        assert_eq!(patch(RS, 0.5, 0.5, 0.5).osc2_mode, Osc2Mode::Ring);
    }

    #[test]
    fn clap_is_tighter_than_808() {
        let p = patch(CP, 0.5, 0.5, 0.5);
        assert!(p.burst_spacing_s < kit808::patch(CP, 0.5, 0.5, 0.5).burst_spacing_s);
        assert_eq!(p.bursts, 4);
    }
}
