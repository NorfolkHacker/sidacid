//! The Mono synth's knobs in real units. `MonoSynth::set_param` decodes
//! each knob into this struct, so render reads ready-to-use values.

use super::lfo::LfoWave;
use super::osc::SubWave;
use crate::params::{MonoParam, exp, mono_steps, on, step_index};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwmSource {
    Manual,
    Lfo,
    Env,
}

/// What restarts the envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// Every new note, legato or not.
    GateTrig,
    /// Only a note after a gap.
    Gate,
    /// Each LFO cycle while a note is held (and a note after a gap).
    Lfo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Porta {
    Off,
    On,
    /// Glides only between overlapping notes.
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArpMode {
    Up,
    UpDown,
    Down,
}

/// Arp step length per `ArpRate` position, in thirds of a sequencer tick
/// (a tick is a 1/64 note): 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32.
pub const ARP_THIRDS: [u32; mono_steps::ARP_RATE] = [48, 24, 16, 12, 8, 6];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonoPatch {
    pub octave_semis: f32,
    pub tune_semis: f32,
    /// LFO → pitch depth at full LFO swing.
    pub vco_mod_semis: f32,
    /// 0 = square; 1 = the thinnest pulse (or full PWM depth).
    pub pw: f32,
    pub pwm_source: PwmSource,
    pub pulse: f32,
    pub saw: f32,
    pub sub: f32,
    pub sub_wave: SubWave,
    pub noise: f32,
    pub cutoff_hz: f32,
    /// Ladder feedback, 0..=4.2.
    pub resonance_k: f32,
    pub env_oct: f32,
    pub vcf_mod_oct: f32,
    /// Cutoff tracking of the note: 1 = full (an octave per octave).
    pub key_follow: f32,
    pub vel_vcf_oct: f32,
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,
    pub trigger: Trigger,
    /// VCA follows the gate instead of the envelope.
    pub vca_gate: bool,
    pub vel_vca: f32,
    pub lfo_wave: LfoWave,
    pub lfo_hz: f32,
    pub porta: Porta,
    /// Glide time constant.
    pub porta_s: f32,
    pub arp_on: bool,
    pub arp_mode: ArpMode,
    pub arp_octaves: u8,
    pub arp_thirds: u32,
    pub drift: f32,
    pub chorus_rate: f32,
    pub chorus_depth: f32,
    pub chorus_mix: f32,
    /// Output gain (the knob squared).
    pub volume: f32,
}

impl Default for MonoPatch {
    fn default() -> Self {
        let mut p = Self {
            octave_semis: 0.0,
            tune_semis: 0.0,
            vco_mod_semis: 0.0,
            pw: 0.0,
            pwm_source: PwmSource::Manual,
            pulse: 0.0,
            saw: 0.0,
            sub: 0.0,
            sub_wave: SubWave::Oct1Square,
            noise: 0.0,
            cutoff_hz: 1000.0,
            resonance_k: 0.0,
            env_oct: 0.0,
            vcf_mod_oct: 0.0,
            key_follow: 0.0,
            vel_vcf_oct: 0.0,
            attack_s: 0.0,
            decay_s: 0.0,
            sustain: 0.0,
            release_s: 0.0,
            trigger: Trigger::GateTrig,
            vca_gate: false,
            vel_vca: 0.0,
            lfo_wave: LfoWave::Triangle,
            lfo_hz: 1.0,
            porta: Porta::Off,
            porta_s: 0.0,
            arp_on: false,
            arp_mode: ArpMode::Up,
            arp_octaves: 1,
            arp_thirds: 12,
            drift: 0.0,
            chorus_rate: 0.0,
            chorus_depth: 0.0,
            chorus_mix: 0.0,
            volume: 0.0,
        };
        for m in MonoParam::ALL {
            p.set(m, m.info().default);
        }
        p
    }
}

impl MonoPatch {
    /// Decodes knob `p` at normalised `v` (clamped to 0..=1).
    pub fn set(&mut self, p: MonoParam, v: f32) {
        use MonoParam as M;
        use mono_steps as s;
        let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let pick = |n: usize| step_index(v, n as u8);
        match p {
            M::Range => self.octave_semis = [-12.0, 0.0, 12.0, 24.0][pick(s::RANGE)],
            M::Tune => self.tune_semis = v - 0.5,
            M::VcoMod => self.vco_mod_semis = v * v * 12.0,
            M::PulseWidth => self.pw = v,
            M::PwmSource => {
                self.pwm_source =
                    [PwmSource::Manual, PwmSource::Lfo, PwmSource::Env][pick(s::PWM_SOURCE)]
            }
            M::PulseLevel => self.pulse = v,
            M::SawLevel => self.saw = v,
            M::SubLevel => self.sub = v,
            M::SubWave => {
                self.sub_wave =
                    [SubWave::Oct1Square, SubWave::Oct2Square, SubWave::Oct2Pulse][pick(s::SUB_WAVE)]
            }
            M::NoiseLevel => self.noise = v,
            M::Cutoff => self.cutoff_hz = exp(v, 20.0, 18_000.0),
            M::Resonance => self.resonance_k = v * 4.2,
            M::EnvAmount => self.env_oct = v * 8.0,
            M::VcfMod => self.vcf_mod_oct = v * 4.0,
            M::KeyFollow => self.key_follow = v,
            M::VelToVcf => self.vel_vcf_oct = v * 3.0,
            M::Attack => self.attack_s = exp(v, 0.0015, 4.0),
            M::Decay => self.decay_s = exp(v, 0.002, 8.0),
            M::Sustain => self.sustain = v,
            M::Release => self.release_s = exp(v, 0.002, 8.0),
            M::Trigger => {
                self.trigger = [Trigger::GateTrig, Trigger::Gate, Trigger::Lfo][pick(s::TRIGGER)]
            }
            M::VcaMode => self.vca_gate = on(v),
            M::VelToVca => self.vel_vca = v,
            M::LfoWave => {
                self.lfo_wave = [
                    LfoWave::Triangle,
                    LfoWave::Square,
                    LfoWave::Random,
                    LfoWave::Noise,
                ][pick(s::LFO_WAVES)]
            }
            M::LfoRate => self.lfo_hz = exp(v, 0.1, 30.0),
            M::Portamento => self.porta = [Porta::Off, Porta::On, Porta::Auto][pick(s::PORTAMENTO)],
            M::PortaTime => self.porta_s = exp(v, 0.001, 1.5),
            M::ArpOn => self.arp_on = on(v),
            M::ArpMode => {
                self.arp_mode = [ArpMode::Up, ArpMode::UpDown, ArpMode::Down][pick(s::ARP_MODE)]
            }
            M::ArpRange => self.arp_octaves = 1 + pick(s::ARP_RANGE) as u8,
            M::ArpRate => self.arp_thirds = ARP_THIRDS[pick(s::ARP_RATE)],
            M::Drift => self.drift = v,
            M::ChorusRate => self.chorus_rate = v,
            M::ChorusDepth => self.chorus_depth = v,
            M::ChorusMix => self.chorus_mix = v,
            M::Volume => self.volume = v * v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MonoParam as M;

    #[test]
    fn defaults_are_a_playable_eight_foot_saw() {
        let p = MonoPatch::default();
        assert_eq!(p.octave_semis, 0.0);
        assert_eq!(p.saw, 0.8);
        assert_eq!(p.arp_thirds, 12, "1/16");
        assert!(!p.arp_on);
        assert_eq!(p.porta, Porta::Off);
        assert_eq!(p.trigger, Trigger::GateTrig);
        assert_eq!(p.sub_wave, SubWave::Oct1Square);
    }

    #[test]
    fn switches_pick_every_position() {
        let mut p = MonoPatch::default();
        p.set(M::Range, 0.0);
        assert_eq!(p.octave_semis, -12.0);
        p.set(M::Range, 1.0);
        assert_eq!(p.octave_semis, 24.0);
        p.set(M::ArpRate, 0.0);
        assert_eq!(p.arp_thirds, 48);
        p.set(M::ArpRate, 1.0);
        assert_eq!(p.arp_thirds, 6);
        p.set(M::ArpRange, 1.0);
        assert_eq!(p.arp_octaves, 3);
        p.set(M::Portamento, 1.0);
        assert_eq!(p.porta, Porta::Auto);
        p.set(M::PwmSource, 0.5);
        assert_eq!(p.pwm_source, PwmSource::Lfo);
        p.set(M::LfoWave, 1.0);
        assert_eq!(p.lfo_wave, LfoWave::Noise);
    }

    #[test]
    fn every_param_has_a_default_in_range() {
        for m in M::ALL {
            let i = m.info();
            assert!((0.0..=1.0).contains(&i.default), "{m:?}");
        }
        assert_eq!(M::ALL.len(), 36);
    }

    #[test]
    fn out_of_range_values_clamp() {
        let mut p = MonoPatch::default();
        p.set(M::Cutoff, 5.0);
        assert!((p.cutoff_hz - 18_000.0).abs() < 1.0);
        p.set(M::Resonance, -1.0);
        assert_eq!(p.resonance_k, 0.0);
    }
}
