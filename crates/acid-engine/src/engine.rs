//! The whole groovebox: two bass synths, two drum kits, the mixer and the
//! sequencer, rendered sample-accurately. process() is real-time safe:
//! no allocation, locking or I/O.

use crate::drums::{DrumKit, KitKind};
use crate::mixer::{Mixer, StereoBuf};
use crate::params::ParamId;
use crate::project::{DrumPattern, Project, SynthPattern, PATTERNS};
use crate::sequencer::{SeqEvent, Sequencer};
use crate::synth::BassSynth;

/// Upper bound on sequencer ticks fired without time advancing; guards
/// against a sequencer that never moves forward.
const MAX_TICKS_PER_POSITION: usize = 64;

pub struct Engine {
    synths: [BassSynth; 2],
    kits: [DrumKit; 2],
    mixer: Mixer,
    seq: Sequencer,
    synth_patterns: [Vec<SynthPattern>; 2],
    drum_patterns: [Vec<DrumPattern>; 2],
    chans: [StereoBuf; 4],
    events: Vec<SeqEvent>,
}

impl Engine {
    pub const MAX_BLOCK: usize = 512;

    pub fn new(sample_rate: f32) -> Self {
        Self {
            synths: [BassSynth::new(sample_rate), BassSynth::new(sample_rate)],
            kits: [DrumKit::new(KitKind::K808, sample_rate), DrumKit::new(KitKind::K909, sample_rate)],
            mixer: Mixer::new(sample_rate, Self::MAX_BLOCK),
            seq: Sequencer::new(sample_rate),
            synth_patterns: [vec![SynthPattern::default(); PATTERNS], vec![SynthPattern::default(); PATTERNS]],
            drum_patterns: [vec![DrumPattern::default(); PATTERNS], vec![DrumPattern::default(); PATTERNS]],
            chans: std::array::from_fn(|_| StereoBuf::new(Self::MAX_BLOCK)),
            events: Vec::with_capacity(64),
        }
    }

    /// Not real-time safe (clones patterns); call while stopped or from
    /// a non-audio thread before handing the engine to the audio thread.
    pub fn load_project(&mut self, p: &Project) {
        for s in 0..2 {
            self.synth_patterns[s] = p.synths[s].patterns.clone();
            self.drum_patterns[s] = p.drums[s].patterns.clone();
        }
        for id in ParamId::all() {
            self.set_param(id, p.param(id));
        }
        self.set_bpm(p.bpm);
        self.set_swing(p.swing);
        for (m, &slot) in p.selected.iter().enumerate() {
            self.seq.select(m, slot);
        }
    }

    /// Out-of-range ids and non-finite values are ignored; values are
    /// clamped to 0..=1.
    pub fn set_param(&mut self, id: ParamId, v: f32) {
        if !id.is_valid() || !v.is_finite() {
            return;
        }
        let v = v.clamp(0.0, 1.0);
        match id {
            ParamId::Synth(s, p) => self.synths[s as usize].set_param(p, v),
            ParamId::Drum(k, i, p) => self.kits[k as usize].set_param(i as usize, p, v),
            ParamId::Kit(k, p) => self.kits[k as usize].set_kit_param(p, v),
            _ => self.mixer.set_param(id, v),
        }
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.seq.set_bpm(bpm);
        self.mixer.set_bpm(self.seq.bpm());
    }

    pub fn set_swing(&mut self, swing: f32) {
        self.seq.set_swing(swing);
    }

    pub fn play(&mut self) {
        self.seq.play();
    }

    pub fn stop(&mut self) {
        self.seq.stop();
        for s in &mut self.synths {
            s.note_off();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.seq.is_playing()
    }

    /// Ignores `machine >= 4` and `slot >= 128`.
    pub fn select_pattern(&mut self, machine: usize, slot: u8) {
        if machine < 4 && (slot as usize) < PATTERNS {
            self.seq.select(machine, slot);
        }
    }

    /// Stores `p` and returns the old pattern. An invalid target or a
    /// malformed pattern is not stored: `p` is handed back unchanged.
    pub fn set_synth_pattern(&mut self, synth: usize, slot: u8, p: SynthPattern) -> SynthPattern {
        if synth >= 2 || slot as usize >= PATTERNS || !p.is_well_formed() {
            return p;
        }
        std::mem::replace(&mut self.synth_patterns[synth][slot as usize], p)
    }

    pub fn set_drum_pattern(&mut self, kit: usize, slot: u8, p: DrumPattern) -> DrumPattern {
        if kit >= 2 || slot as usize >= PATTERNS || !p.is_well_formed() {
            return p;
        }
        std::mem::replace(&mut self.drum_patterns[kit][slot as usize], p)
    }

    pub fn playhead(&self) -> [usize; 4] {
        std::array::from_fn(|m| self.seq.playhead(m))
    }

    pub fn current_patterns(&self) -> [u8; 4] {
        std::array::from_fn(|m| self.seq.current_pattern(m))
    }

    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        debug_assert_eq!(out_l.len(), out_r.len());
        let total = out_l.len().min(out_r.len());
        let mut offset = 0;
        while offset < total {
            let n = (total - offset).min(Self::MAX_BLOCK);
            self.process_block(&mut out_l[offset..offset + n], &mut out_r[offset..offset + n]);
            offset += n;
        }
    }

    fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len();
        for c in &mut self.chans {
            c.l[..n].fill(0.0);
            c.r[..n].fill(0.0);
        }
        let mut pos = 0;
        let mut ticks_here = 0;
        while pos < n {
            let until = self.seq.samples_to_next_tick();
            if until == 0 && ticks_here < MAX_TICKS_PER_POSITION {
                ticks_here += 1;
                self.events.clear();
                self.seq.tick(
                    [&self.synth_patterns[0], &self.synth_patterns[1]],
                    [&self.drum_patterns[0], &self.drum_patterns[1]],
                    &mut self.events,
                );
                for i in 0..self.events.len() {
                    match self.events[i] {
                        SeqEvent::Note { synth, midi, accent, legato } => self.synths[synth].note_on(midi, accent, legato),
                        SeqEvent::NoteOff { synth } => self.synths[synth].note_off(),
                        SeqEvent::Drum { kit, inst, accent } => self.kits[kit].trigger(inst, accent),
                        SeqEvent::Param { id, value } => self.set_param(id, value),
                    }
                }
                continue;
            }
            // Past the tick bound (a broken tempo) just render on.
            ticks_here = 0;
            let run = if until == 0 { n - pos } else { until.min(n - pos) };
            let span = pos..pos + run;
            for s in 0..2 {
                let c = &mut self.chans[s];
                self.synths[s].render(&mut c.l[span.clone()], &mut c.r[span.clone()]);
            }
            for k in 0..2 {
                let c = &mut self.chans[2 + k];
                self.kits[k].render(&mut c.l[span.clone()], &mut c.r[span.clone()]);
            }
            self.seq.advance(run);
            pos += run;
        }
        if self.mixer.process(&mut self.chans, n, out_l, out_r) {
            for s in &mut self.synths {
                s.reset();
            }
            for k in &mut self.kits {
                k.reset();
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests tweak one field of a default pattern
mod tests {
    use super::*;
    use crate::params::{ChannelParam, DrumParam, KitParam, MasterParam, SynthParam};
    use crate::testutil::{all_finite, peak, rms};

    const SR: f32 = 44_100.0;

    fn render(e: &mut Engine, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * SR) as usize;
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        e.process(&mut l, &mut r);
        (l, r)
    }

    fn demo_engine() -> Engine {
        let mut e = Engine::new(SR);
        e.load_project(&crate::demo::demo_project());
        e
    }

    #[test]
    fn stopped_engine_is_silent() {
        let mut e = demo_engine();
        let (l, _) = render(&mut e, 0.5);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn playing_demo_makes_bounded_sound() {
        let mut e = demo_engine();
        e.play();
        let (l, r) = render(&mut e, 4.0);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(rms(&l) > 0.02, "rms = {}", rms(&l));
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn odd_buffer_sizes_render_identically() {
        let mut a = demo_engine();
        let mut b = demo_engine();
        a.play();
        b.play();
        let (la, _) = render(&mut a, 1.0);
        let mut lb = Vec::new();
        let mut remaining = la.len();
        while remaining > 0 {
            let n = remaining.min(37);
            let mut l = vec![0.0; n];
            let mut r = vec![0.0; n];
            b.process(&mut l, &mut r);
            lb.extend(l);
            remaining -= n;
        }
        for (x, y) in la.iter().zip(&lb) {
            assert!((x - y).abs() < 1e-5);
        }
    }

    #[test]
    fn master_level_zero_is_silent() {
        let mut e = demo_engine();
        e.set_param(ParamId::Master(MasterParam::Level), 0.0);
        e.play();
        let (l, _) = render(&mut e, 1.0);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn stop_closes_synth_gates_and_tails_die() {
        let mut e = demo_engine();
        for c in 0..4 {
            e.set_param(ParamId::Channel(c, ChannelParam::DelaySend), 0.0);
            e.set_param(ParamId::Channel(c, ChannelParam::ReverbSend), 0.0);
        }
        e.play();
        render(&mut e, 1.0);
        e.stop();
        render(&mut e, 3.0);
        let (l, _) = render(&mut e, 0.2);
        assert!(peak(&l) < 1e-3, "peak = {}", peak(&l));
    }

    #[test]
    fn pattern_replacement_returns_old_pattern() {
        let mut e = demo_engine();
        let old = e.set_synth_pattern(0, 0, SynthPattern::default());
        assert!(old.steps.iter().any(|s| s.gate != crate::project::Gate::Off));
        let old_drums = e.set_drum_pattern(0, 0, DrumPattern::default());
        assert!(old_drums.steps.iter().flatten().any(|h| *h != crate::project::DrumHit::Off));
    }

    #[test]
    fn nan_tempo_cannot_hang_or_poison() {
        let mut e = demo_engine();
        e.set_bpm(f32::NAN);
        e.set_swing(f32::NAN);
        e.play();
        let (l, r) = render(&mut e, 0.5);
        assert!(all_finite(&l) && all_finite(&r));
    }

    #[test]
    fn broken_clock_cannot_hang_process() {
        let mut e = demo_engine();
        e.play();
        e.seq.poison_clock();
        let (l, r) = render(&mut e, 0.2);
        assert_eq!(l.len(), r.len());
        assert!(all_finite(&l) && all_finite(&r));
    }

    #[test]
    fn out_of_range_indices_are_ignored() {
        let mut e = demo_engine();
        e.play();
        e.select_pattern(0, 200);
        e.select_pattern(9, 0);
        e.set_param(ParamId::Drum(0, 9, DrumParam::Tune), 0.5);
        e.set_param(ParamId::Synth(7, SynthParam::Cutoff), 0.5);
        e.set_param(ParamId::Channel(4, ChannelParam::Level), 0.5);
        e.set_param(ParamId::Kit(2, KitParam::Volume), 0.5);
        let (l, _) = render(&mut e, 0.5);
        assert!(all_finite(&l));
        assert_eq!(e.current_patterns(), [0, 0, 0, 0]);
    }

    #[test]
    fn malformed_or_misplaced_patterns_are_rejected() {
        let mut e = demo_engine();
        e.play();
        let mut short = SynthPattern::default();
        short.steps.truncate(3);
        let back = e.set_synth_pattern(0, 0, short.clone());
        assert_eq!(back, short);
        let mut zero = DrumPattern::default();
        zero.length = 0;
        assert_eq!(e.set_drum_pattern(0, 0, zero.clone()), zero);
        let mut lane = SynthPattern::default();
        lane.param_lane = Some(crate::project::ParamLane { param: ParamId::Fx(crate::params::FxParam::DelayMs), values: vec![None; 2] });
        assert_eq!(e.set_synth_pattern(0, 0, lane.clone()), lane);
        assert_eq!(e.set_synth_pattern(2, 0, SynthPattern::default()), SynthPattern::default());
        assert_eq!(e.set_drum_pattern(0, 200, DrumPattern::default()), DrumPattern::default());
        let (l, _) = render(&mut e, 1.0);
        assert!(all_finite(&l) && rms(&l) > 0.02, "still playing the demo");
    }

    #[test]
    fn non_finite_param_is_dropped_and_values_clamped() {
        let mut e = demo_engine();
        e.set_param(ParamId::Synth(0, SynthParam::Cutoff), f32::NAN);
        e.set_param(ParamId::Channel(0, ChannelParam::Level), 50.0);
        e.play();
        let (l, _) = render(&mut e, 0.1);
        assert!(all_finite(&l));
        e.set_param(ParamId::Synth(0, SynthParam::Cutoff), 0.5);
        let (l, _) = render(&mut e, 3.0);
        assert!(all_finite(&l) && peak(&l) > 0.01);
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn playhead_advances() {
        let mut e = demo_engine();
        e.play();
        render(&mut e, 0.3);
        assert!(e.playhead()[2] >= 2);
        assert_eq!(e.current_patterns(), [0, 0, 0, 0]);
    }
}
