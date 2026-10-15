//! The Mono machine's voice: one VCO + sub + noise through the ladder
//! and VCA, with last-note priority over a stack of held notes, the
//! 101-style trigger and portamento modes, drift and a chorus.

use super::adsr::Adsr;
use super::ladder::Ladder;
use super::lfo::{Drift, Lfo, Noise};
use super::osc::Vco;
use super::patch::{MonoPatch, Porta, PwmSource, Trigger};
use crate::fx::Chorus;
use crate::params::MonoParam;

/// Held notes remembered for fallback (and the arp).
pub(super) const MAX_HELD: usize = 16;
/// Mixer sum → filter input scale, leaving headroom before the tanh.
const MIX_LEVEL: f32 = 0.5;
const OUT_LEVEL: f32 = 0.6;
/// Drift at full: slow pitch wander, per-note offset, cutoff wander.
const DRIFT_SEMIS: f32 = 0.08;
const DRIFT_NOTE_SEMIS: f32 = 0.03;
const DRIFT_CUTOFF_OCT: f32 = 0.15;
/// Gate-mode VCA ramp, so on/off doesn't click.
const VCA_RAMP_S: f32 = 0.002;

fn midi_hz(semis: f32) -> f32 {
    440.0 * ((semis - 69.0) / 12.0).exp2()
}

#[derive(Clone, Copy, Default)]
struct Held {
    pitch: u8,
    vel: u8,
    /// An audition (piano-roll key), not a sequencer note.
    live: bool,
}

pub struct MonoSynth {
    sample_rate: f32,
    patch: MonoPatch,
    vco: Vco,
    filter: Ladder,
    pub(super) env: Adsr,
    lfo: Lfo,
    drift: Drift,
    noise: Noise,
    chorus: Chorus,
    held: [Held; MAX_HELD],
    held_len: usize,
    /// The gated pitch.
    sounding: Option<u8>,
    gate: bool,
    /// Current (gliding) pitch and where it is going, in MIDI semitones.
    pub(super) pitch: f32,
    target: f32,
    /// A pitch has been played since the last reset (portamento On glides from it).
    has_pitch: bool,
    note_offset: f32,
    /// Velocity of the sounding note, 0..=1.
    vel: f32,
    vca: f32,
    vca_coef: f32,
    glide_coef: f32,
}

impl MonoSynth {
    /// Allocates the chorus line: call off the audio thread.
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            sample_rate,
            patch: MonoPatch::default(),
            vco: Vco::new(),
            filter: Ladder::new(sample_rate),
            env: Adsr::new(sample_rate),
            lfo: Lfo::new(sample_rate),
            drift: Drift::new(sample_rate),
            noise: Noise::new(0x2545_f491),
            chorus: Chorus::new(sample_rate),
            held: [Held::default(); MAX_HELD],
            held_len: 0,
            sounding: None,
            gate: false,
            pitch: 48.0,
            target: 48.0,
            has_pitch: false,
            note_offset: 0.0,
            vel: 1.0,
            vca: 0.0,
            vca_coef: 1.0 - (-1.0 / (VCA_RAMP_S * sample_rate)).exp(),
            glide_coef: 1.0,
        };
        s.apply_patch();
        s
    }

    pub fn set_param(&mut self, p: MonoParam, v: f32) {
        self.patch.set(p, v);
        self.apply_patch();
    }

    fn apply_patch(&mut self) {
        let p = &self.patch;
        self.env.set(p.attack_s, p.decay_s, p.sustain, p.release_s);
        self.glide_coef = 1.0 - (-1.0 / (p.porta_s * self.sample_rate)).exp();
    }

    pub fn sounding(&self) -> Option<u8> {
        self.sounding
    }

    fn top(&self) -> Option<Held> {
        self.held[..self.held_len].last().copied()
    }

    fn remove_at(&mut self, i: usize) {
        self.held.copy_within(i + 1..self.held_len, i);
        self.held_len -= 1;
    }

    pub fn note_on(&mut self, pitch: u8, vel: u8, live: bool) {
        let mut live = live;
        if let Some(i) = self.held[..self.held_len].iter().position(|h| h.pitch == pitch) {
            // A pattern note on a held audition must not un-hold the key.
            live |= self.held[i].live;
            self.remove_at(i);
        }
        if self.held_len == MAX_HELD {
            self.remove_at(0);
        }
        self.held[self.held_len] = Held { pitch, vel, live };
        self.held_len += 1;
        self.sound(pitch, vel);
    }

    /// Releases `pitch` if it was started the same way (`live` or not).
    pub fn note_off(&mut self, pitch: u8, live: bool) {
        let found = self.held[..self.held_len]
            .iter()
            .position(|h| h.pitch == pitch && h.live == live);
        if let Some(i) = found {
            self.remove_at(i);
            self.follow_held();
        }
    }

    /// Sequencer notes off; auditions keep sounding.
    pub fn pattern_off(&mut self) {
        let mut kept = 0;
        for i in 0..self.held_len {
            if self.held[i].live {
                self.held[kept] = self.held[i];
                kept += 1;
            }
        }
        self.held_len = kept;
        self.follow_held();
    }

    pub fn all_off(&mut self) {
        self.held_len = 0;
        self.follow_held();
    }

    /// Clears notes, filter and modulation state (NaN guard, hard stop).
    pub fn reset(&mut self) {
        self.held_len = 0;
        self.sounding = None;
        self.gate = false;
        self.has_pitch = false;
        self.vca = 0.0;
        self.vco.reset();
        self.filter.reset();
        self.env.reset();
        self.lfo.reset();
        self.drift.reset();
        self.chorus.reset();
    }

    /// After the held set changed: sound the newest held note (legato) or
    /// close the gate.
    fn follow_held(&mut self) {
        match self.top() {
            Some(h) if self.sounding != Some(h.pitch) => self.sound(h.pitch, h.vel),
            Some(_) => {}
            None => self.close_gate(),
        }
    }

    fn sound(&mut self, pitch: u8, vel: u8) {
        let legato = self.gate;
        let glide = match self.patch.porta {
            Porta::Off => false,
            Porta::On => self.has_pitch,
            Porta::Auto => legato,
        };
        self.target = pitch as f32;
        if !glide {
            self.pitch = self.target;
        }
        self.has_pitch = true;
        self.sounding = Some(pitch);
        self.vel = vel as f32 / 127.0;
        self.note_offset = self.patch.drift * DRIFT_NOTE_SEMIS * self.drift.note_offset();
        if !legato || self.patch.trigger == Trigger::GateTrig {
            self.env.gate_on();
        }
        self.gate = true;
    }

    fn close_gate(&mut self) {
        self.gate = false;
        self.sounding = None;
        self.env.gate_off();
    }

    /// Adds the machine's output into `left`/`right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        for i in 0..n {
            let (l, r) = self.sample();
            left[i] += l;
            right[i] += r;
        }
    }

    fn sample(&mut self) -> (f32, f32) {
        let p = self.patch;
        let (lfo, wrapped) = self.lfo.next(p.lfo_wave, p.lfo_hz);
        if wrapped && self.gate && p.trigger == Trigger::Lfo {
            self.env.gate_on();
        }
        let env = self.env.next();
        let [drift_pitch, drift_cutoff] = self.drift.next();
        self.pitch += (self.target - self.pitch) * self.glide_coef;
        let semis = self.pitch
            + p.octave_semis
            + p.tune_semis
            + p.vco_mod_semis * lfo
            + p.drift * DRIFT_SEMIS * drift_pitch
            + self.note_offset;
        let pwm = match p.pwm_source {
            PwmSource::Manual => p.pw,
            PwmSource::Lfo => p.pw * 0.5 * (1.0 + lfo),
            PwmSource::Env => p.pw * env,
        };
        let o = self
            .vco
            .next(midi_hz(semis) / self.sample_rate, 0.5 - 0.45 * pwm, p.sub_wave);
        let mix = (o.pulse * p.pulse
            + o.saw * p.saw
            + o.sub * p.sub
            + self.noise.next() * p.noise)
            * MIX_LEVEL;
        let octaves = p.env_oct * env
            + p.vcf_mod_oct * lfo
            + p.key_follow * (self.pitch - 48.0) / 12.0
            + p.vel_vcf_oct * (self.vel * 2.0 - 1.0)
            + p.drift * DRIFT_CUTOFF_OCT * drift_cutoff;
        self.filter.set(p.cutoff_hz * octaves.exp2(), p.resonance_k);
        let y = self.filter.process(mix);
        if p.vca_gate {
            let to = if self.gate { 1.0 } else { 0.0 };
            self.vca += (to - self.vca) * self.vca_coef;
            if !self.gate && self.vca < 1e-5 {
                self.vca = 0.0;
            }
        } else {
            self.vca = env;
        }
        let vel_gain = 1.0 - p.vel_vca * (1.0 - self.vel);
        let x = y * self.vca * vel_gain * p.volume * OUT_LEVEL;
        if p.chorus_mix > 0.0 {
            let (wl, wr) = self.chorus.process(x, p.chorus_rate, p.chorus_depth);
            (x + (wl - x) * p.chorus_mix, x + (wr - x) * p.chorus_mix)
        } else {
            (x, x)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MonoParam as M;
    use crate::testutil::{all_finite, peak, rms};
    use crate::va::adsr::Stage;

    const SR: f32 = 48_000.0;

    fn render(s: &mut MonoSynth, n: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        s.render(&mut l, &mut r);
        l
    }

    #[test]
    fn silent_until_a_note_then_sounding() {
        let mut s = MonoSynth::new(SR);
        assert!(render(&mut s, 1000).iter().all(|v| *v == 0.0));
        s.note_on(48, 100, false);
        let y = render(&mut s, 4800);
        assert!(all_finite(&y));
        assert!(rms(&y) > 0.01 && peak(&y) < 1.5, "rms {} peak {}", rms(&y), peak(&y));
    }

    #[test]
    fn the_last_note_wins_and_releasing_falls_back() {
        let mut s = MonoSynth::new(SR);
        s.note_on(48, 100, false);
        s.note_on(60, 100, false);
        assert_eq!(s.sounding(), Some(60));
        s.note_off(48, false);
        assert_eq!(s.sounding(), Some(60), "a held-under note leaves the sound alone");
        s.note_on(48, 100, false);
        s.note_off(48, false);
        assert_eq!(s.sounding(), Some(60));
        s.note_off(60, false);
        assert_eq!(s.sounding(), None);
        assert_eq!(s.env.stage(), Stage::Release);
    }

    #[test]
    fn auto_portamento_glides_only_between_overlapping_notes() {
        let mut s = MonoSynth::new(SR);
        s.set_param(M::Portamento, 1.0);
        s.set_param(M::PortaTime, 1.0);
        s.note_on(48, 100, false);
        render(&mut s, 100);
        s.note_on(60, 100, false);
        render(&mut s, 100);
        assert!(s.pitch < 59.0, "gliding: {}", s.pitch);
        s.all_off();
        s.note_on(48, 100, false);
        assert_eq!(s.pitch, 48.0, "after a gap: no glide");
    }

    #[test]
    fn gate_mode_does_not_retrigger_legato_notes() {
        for (trigger, legato_stage) in [(0.0, Stage::Attack), (0.5, Stage::Sustain)] {
            let mut s = MonoSynth::new(SR);
            s.set_param(M::Trigger, trigger);
            s.set_param(M::Attack, 0.0);
            s.set_param(M::Decay, 0.0);
            s.note_on(48, 100, false);
            render(&mut s, 4800);
            assert_eq!(s.env.stage(), Stage::Sustain);
            s.note_on(50, 100, false);
            assert_eq!(s.env.stage(), legato_stage, "trigger {trigger}");
        }
    }

    #[test]
    fn drift_zero_ignores_drift_state_and_drift_changes_the_sound() {
        let play = |drift: f32, perturb: bool| {
            let mut s = MonoSynth::new(SR);
            s.set_param(M::Drift, drift);
            if perturb {
                for _ in 0..5000 {
                    s.drift.next();
                }
                s.drift.note_offset();
            }
            s.note_on(48, 100, false);
            render(&mut s, 9600)
        };
        assert_eq!(play(0.0, false), play(0.0, true));
        assert_ne!(play(1.0, false), play(1.0, true));
    }

    #[test]
    fn the_held_stack_caps_at_sixteen_and_drops_the_oldest() {
        let mut s = MonoSynth::new(SR);
        for p in 40..57 {
            s.note_on(p, 100, false);
        }
        for p in (41..57).rev() {
            assert_eq!(s.sounding(), Some(p));
            s.note_off(p, false);
        }
        assert_eq!(s.sounding(), None, "note 40 was dropped and must not come back");
    }

    #[test]
    fn releasing_a_middle_note_leaves_the_sound_and_repress_moves_to_top() {
        let mut s = MonoSynth::new(SR);
        s.note_on(40, 100, false);
        s.note_on(50, 100, false);
        s.note_on(60, 100, false);
        s.note_off(50, false);
        assert_eq!(s.sounding(), Some(60));
        s.note_on(40, 100, false);
        assert_eq!(s.sounding(), Some(40));
        s.note_off(40, false);
        assert_eq!(s.sounding(), Some(60));
    }

    #[test]
    fn portamento_on_glides_after_a_gap_and_off_never_glides() {
        let mut s = MonoSynth::new(SR);
        s.set_param(M::Portamento, 0.5);
        s.set_param(M::PortaTime, 1.0);
        s.note_on(48, 100, false);
        s.all_off();
        s.note_on(60, 100, false);
        render(&mut s, 100);
        assert!(s.pitch < 59.0 && s.pitch >= 48.0, "{}", s.pitch);
        let mut s = MonoSynth::new(SR);
        s.set_param(M::Portamento, 0.0);
        s.set_param(M::PortaTime, 1.0);
        s.note_on(48, 100, false);
        s.note_on(60, 100, false);
        assert_eq!(s.pitch, 60.0);
    }

    #[test]
    fn trigger_lfo_retriggers_each_cycle_while_held() {
        let mut s = MonoSynth::new(SR);
        s.set_param(M::Trigger, 1.0);
        s.set_param(M::LfoRate, 1.0);
        s.set_param(M::Attack, 0.0);
        s.set_param(M::Decay, 0.0);
        s.note_on(48, 100, false);
        let mut saw_attack_again = false;
        let mut reached_sustain = false;
        for _ in 0..48_000 {
            render(&mut s, 1);
            match s.env.stage() {
                Stage::Sustain => reached_sustain = true,
                Stage::Attack if reached_sustain => saw_attack_again = true,
                _ => {}
            }
        }
        assert!(reached_sustain && saw_attack_again);
    }

    #[test]
    fn render_adds_into_the_buffers() {
        let mut s = MonoSynth::new(SR);
        let (mut l, mut r) = (vec![1.0; 256], vec![1.0; 256]);
        s.render(&mut l, &mut r);
        assert!(l.iter().chain(r.iter()).all(|v| *v == 1.0));
    }

    #[test]
    fn gate_mode_fallback_does_not_retrigger() {
        let mut s = MonoSynth::new(SR);
        s.set_param(M::Trigger, 0.5);
        s.set_param(M::Attack, 0.0);
        s.set_param(M::Decay, 0.0);
        s.note_on(48, 100, false);
        s.note_on(60, 100, false);
        render(&mut s, 4800);
        assert_eq!(s.env.stage(), Stage::Sustain);
        s.note_off(60, false);
        assert_eq!(s.sounding(), Some(48));
        assert_eq!(s.env.stage(), Stage::Sustain);
    }

    #[test]
    fn reset_silences() {
        let mut s = MonoSynth::new(SR);
        s.note_on(48, 100, false);
        render(&mut s, 4800);
        s.reset();
        assert!(render(&mut s, 4800).iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_pattern_note_on_a_held_audition_does_not_silence_it() {
        let mut s = MonoSynth::new(SR);
        s.note_on(48, 100, true);
        s.note_on(48, 100, false);
        s.note_off(48, false);
        assert_eq!(s.sounding(), Some(48));
        s.note_off(48, true);
        assert_eq!(s.sounding(), None);
    }

    #[test]
    fn pattern_off_keeps_auditions() {
        let mut s = MonoSynth::new(SR);
        s.note_on(48, 100, true);
        s.note_on(60, 100, false);
        s.pattern_off();
        assert_eq!(s.sounding(), Some(48));
        s.note_off(48, false);
        assert_eq!(s.sounding(), Some(48), "a pattern NoteOff never releases an audition");
    }

    #[test]
    fn every_knob_at_both_ends_stays_finite_and_bounded() {
        for m in M::ALL {
            for v in [0.0, 1.0] {
                let mut s = MonoSynth::new(SR);
                s.set_param(M::SawLevel, 1.0);
                s.set_param(M::PulseLevel, 1.0);
                s.set_param(M::SubLevel, 1.0);
                s.set_param(M::NoiseLevel, 1.0);
                s.set_param(m, v);
                s.note_on(48, 127, false);
                let y = render(&mut s, 9600);
                assert!(all_finite(&y), "{m:?}={v}");
                assert!(peak(&y) < 4.0, "{m:?}={v}: {}", peak(&y));
            }
        }
    }
}
