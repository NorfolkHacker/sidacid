//! Channel strips (distortion -> compressor -> level/pan/mute/solo,
//! post-fader delay and reverb sends; one per rack slot), send returns,
//! master compressor, master level, optional lo-fi, final clip and NaN guard.

use crate::fx::{Compressor, Delay, Distortion, LoFi, Reverb};
use crate::machine::MAX_MACHINES;
use crate::params::{ChannelParam, FxParam, GlobalParam, MasterParam, exp, on, step_index};
use crate::rack::Slot;
use std::f32::consts::{FRAC_PI_4, SQRT_2};

pub struct StereoBuf {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl StereoBuf {
    pub fn new(len: usize) -> Self {
        Self {
            l: vec![0.0; len],
            r: vec![0.0; len],
        }
    }
}

/// One slot's mixer channel.
pub struct ChannelStrip {
    dist: Distortion,
    comp: Compressor,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    mute: bool,
    solo: bool,
    delay_send: f32,
    reverb_send: f32,
}

impl ChannelStrip {
    /// A strip with every channel param at its default.
    pub fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            dist: Distortion::new(sample_rate),
            comp: Compressor::new(sample_rate),
            gain: 1.0,
            pan_l: 1.0,
            pan_r: 1.0,
            mute: false,
            solo: false,
            delay_send: 0.0,
            reverb_send: 0.0,
        };
        for p in ChannelParam::ALL {
            s.set(p, crate::params::MachineParam::Channel(p).info().default);
        }
        s
    }

    pub fn set(&mut self, p: ChannelParam, v: f32) {
        match p {
            ChannelParam::Level => self.gain = v * v * 2.0,
            ChannelParam::Pan => {
                let angle = v.clamp(0.0, 1.0) * 2.0 * FRAC_PI_4;
                self.pan_l = angle.cos() * SQRT_2;
                self.pan_r = angle.sin() * SQRT_2;
            }
            ChannelParam::Mute => self.mute = on(v),
            ChannelParam::Solo => self.solo = on(v),
            ChannelParam::DistOn => self.dist.on = on(v),
            ChannelParam::DistDrive => self.dist.set_drive(v),
            ChannelParam::DistTone => self.dist.set_tone(v),
            ChannelParam::DistMix => self.dist.set_mix(v),
            ChannelParam::CompOn => self.comp.on = on(v),
            ChannelParam::CompThreshold => self.comp.set_threshold(v),
            ChannelParam::CompRatio => self.comp.set_ratio(v),
            ChannelParam::CompAttack => self.comp.set_attack(v),
            ChannelParam::CompRelease => self.comp.set_release(v),
            ChannelParam::CompMakeup => self.comp.set_makeup(v),
            ChannelParam::DelaySend => self.delay_send = v,
            ChannelParam::ReverbSend => self.reverb_send = v,
        }
    }
}

/// The shared part of the mix: send effects and the master bus. The
/// channel strips live in the rack slots.
pub struct Mixer {
    delay: Delay,
    reverb: Reverb,
    master_comp: Compressor,
    lofi: LoFi,
    master_gain: f32,
    delay_return: f32,
    reverb_return: f32,
    delay_send: StereoBuf,
    reverb_send: StereoBuf,
    wet: StereoBuf,
    meters: [f32; MAX_MACHINES + 1],
}

impl Mixer {
    pub fn new(sample_rate: f32, max_block: usize) -> Self {
        let mut m = Self {
            delay: Delay::new(sample_rate),
            reverb: Reverb::new(sample_rate),
            master_comp: Compressor::new(sample_rate),
            lofi: LoFi::new(sample_rate),
            master_gain: 1.0,
            delay_return: 0.7,
            reverb_return: 0.6,
            delay_send: StereoBuf::new(max_block),
            reverb_send: StereoBuf::new(max_block),
            wet: StereoBuf::new(max_block),
            meters: [0.0; MAX_MACHINES + 1],
        };
        for g in GlobalParam::all() {
            m.set_global(g, g.info().default);
        }
        m
    }

    pub fn set_bpm(&mut self, bpm: f32) {
        self.delay.set_bpm(bpm);
    }

    /// Peak levels since the last call: slots by rack position post-fader
    /// (unused positions 0), then the master after the final clip at
    /// `MAX_MACHINES`. Resets them. Muted (or soloed-out) slots read 0.
    pub fn take_meters(&mut self) -> [f32; MAX_MACHINES + 1] {
        std::mem::take(&mut self.meters)
    }

    pub fn set_global(&mut self, g: GlobalParam, v: f32) {
        match g {
            GlobalParam::Fx(p) => match p {
                FxParam::DelaySync => self.delay.set_sync(on(v)),
                FxParam::DelayDivision => self.delay.set_division(step_index(v, 6)),
                FxParam::DelayMs => self.delay.set_ms(exp(v, 10.0, 2000.0)),
                FxParam::DelayFeedback => self.delay.set_feedback(v),
                FxParam::DelayTone => self.delay.set_tone(v),
                FxParam::DelayPingPong => self.delay.ping_pong = on(v),
                FxParam::DelayReturn => self.delay_return = v,
                FxParam::ReverbSize => self.reverb.set_size(v),
                FxParam::ReverbDamping => self.reverb.set_damping(v),
                FxParam::ReverbWidth => self.reverb.set_width(v),
                FxParam::ReverbReturn => self.reverb_return = v,
            },
            GlobalParam::Master(p) => match p {
                MasterParam::CompOn => self.master_comp.on = on(v),
                MasterParam::CompThreshold => self.master_comp.set_threshold(v),
                MasterParam::CompRatio => self.master_comp.set_ratio(v),
                MasterParam::CompAttack => self.master_comp.set_attack(v),
                MasterParam::CompRelease => self.master_comp.set_release(v),
                MasterParam::CompMakeup => self.master_comp.set_makeup(v),
                MasterParam::Level => self.master_gain = v * v * 1.5,
                MasterParam::LoFi => self.lofi.on = on(v),
            },
        }
    }

    /// Mixes `n` samples of the slots' buffers, through their strips in
    /// rack order, into `out_l`/`out_r`. Precondition: `n` is at most the
    /// `max_block` given to `Mixer::new` and no longer than any buffer
    /// passed in; at most `MAX_MACHINES` slots. Returns true when the NaN
    /// guard fired (output was silenced and effect state reset).
    pub fn process(
        &mut self,
        slots: &mut [Slot],
        n: usize,
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) -> bool {
        let (out_l, out_r) = (&mut out_l[..n], &mut out_r[..n]);
        out_l.fill(0.0);
        out_r.fill(0.0);
        for b in [&mut self.delay_send, &mut self.reverb_send] {
            b.l[..n].fill(0.0);
            b.r[..n].fill(0.0);
        }
        let count = slots.len().min(MAX_MACHINES);
        let slots = &mut slots[..count];
        let any_solo = slots.iter().any(|s| s.strip.solo);
        for (c, slot) in slots.iter_mut().enumerate() {
            let (strip, buf) = (&mut slot.strip, &mut slot.buf);
            let (l, r) = (&mut buf.l[..n], &mut buf.r[..n]);
            strip.dist.process(l, r);
            strip.comp.process(l, r);
            if strip.mute || (any_solo && !strip.solo) {
                continue;
            }
            let (gl, gr) = (strip.gain * strip.pan_l, strip.gain * strip.pan_r);
            for i in 0..n {
                let sl = l[i] * gl;
                let sr = r[i] * gr;
                self.meters[c] = self.meters[c].max(sl.abs()).max(sr.abs());
                out_l[i] += sl;
                out_r[i] += sr;
                self.delay_send.l[i] += sl * strip.delay_send;
                self.delay_send.r[i] += sr * strip.delay_send;
                self.reverb_send.l[i] += sl * strip.reverb_send;
                self.reverb_send.r[i] += sr * strip.reverb_send;
            }
        }
        self.wet.l[..n].fill(0.0);
        self.wet.r[..n].fill(0.0);
        self.delay.process(
            &self.delay_send.l[..n],
            &self.delay_send.r[..n],
            &mut self.wet.l[..n],
            &mut self.wet.r[..n],
        );
        for i in 0..n {
            out_l[i] += self.wet.l[i] * self.delay_return;
            out_r[i] += self.wet.r[i] * self.delay_return;
        }
        self.wet.l[..n].fill(0.0);
        self.wet.r[..n].fill(0.0);
        self.reverb.process(
            &self.reverb_send.l[..n],
            &self.reverb_send.r[..n],
            &mut self.wet.l[..n],
            &mut self.wet.r[..n],
        );
        for i in 0..n {
            out_l[i] += self.wet.l[i] * self.reverb_return;
            out_r[i] += self.wet.r[i] * self.reverb_return;
        }
        self.master_comp.process(out_l, out_r);
        for i in 0..n {
            out_l[i] *= self.master_gain;
            out_r[i] *= self.master_gain;
        }
        self.lofi.process(out_l, out_r);
        if out_l.iter().chain(out_r.iter()).any(|v| !v.is_finite()) {
            out_l.fill(0.0);
            out_r.fill(0.0);
            self.delay.reset();
            self.reverb.reset();
            self.master_comp.reset();
            for s in slots.iter_mut() {
                s.strip.comp.reset();
                s.strip.dist.reset();
            }
            return true;
        }
        for v in out_l.iter_mut().chain(out_r.iter_mut()) {
            *v = v.clamp(-1.0, 1.0);
            self.meters[MAX_MACHINES] = self.meters[MAX_MACHINES].max(v.abs());
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use crate::testutil::{all_finite, peak, rms};

    const SR: f32 = 44_100.0;
    const N: usize = 512;

    /// The standard six slots, all silent.
    fn slots() -> Vec<Slot> {
        Project::default()
            .machines
            .iter()
            .map(|e| Slot::from_entry(e, SR))
            .collect()
    }

    fn fill(slots: &mut [Slot], ch: usize, value: f32) {
        for s in slots.iter_mut() {
            s.buf.l.fill(0.0);
            s.buf.r.fill(0.0);
        }
        slots[ch].buf.l.fill(value);
        slots[ch].buf.r.fill(value);
    }

    fn slots_with(ch: usize, value: f32) -> Vec<Slot> {
        let mut s = slots();
        fill(&mut s, ch, value);
        s
    }

    fn run(m: &mut Mixer, slots: &mut [Slot]) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; N];
        let mut r = vec![0.0; N];
        m.process(slots, N, &mut l, &mut r);
        (l, r)
    }

    fn guard_fired(m: &mut Mixer, slots: &mut [Slot]) -> bool {
        let mut l = vec![0.0; N];
        let mut r = vec![0.0; N];
        m.process(slots, N, &mut l, &mut r)
    }

    fn quiet_mixer() -> Mixer {
        let mut m = Mixer::new(SR, N);
        m.set_global(GlobalParam::Master(MasterParam::CompOn), 0.0);
        m
    }

    #[test]
    fn channel_reaches_output() {
        let mut m = quiet_mixer();
        let (l, r) = run(&mut m, &mut slots_with(0, 0.25));
        assert!(rms(&l) > 0.1 && rms(&r) > 0.1);
    }

    #[test]
    fn mute_silences_channel() {
        let mut m = quiet_mixer();
        let mut s = slots_with(1, 0.25);
        s[1].strip.set(ChannelParam::Mute, 1.0);
        let (l, _) = run(&mut m, &mut s);
        assert_eq!(peak(&l), 0.0);
    }

    #[test]
    fn solo_isolates_channel() {
        let mut m = quiet_mixer();
        let mut s = slots_with(0, 0.25);
        s[2].strip.set(ChannelParam::Solo, 1.0);
        let (l, _) = run(&mut m, &mut s);
        assert_eq!(peak(&l), 0.0);
        fill(&mut s, 2, 0.25);
        let (l, _) = run(&mut m, &mut s);
        assert!(peak(&l) > 0.1);
    }

    #[test]
    fn hard_left_pan() {
        let mut m = quiet_mixer();
        let mut s = slots_with(0, 0.25);
        s[0].strip.set(ChannelParam::Pan, 0.0);
        let (l, r) = run(&mut m, &mut s);
        assert!(peak(&l) > 0.1);
        assert!(peak(&r) < 1e-6);
    }

    #[test]
    fn delay_send_produces_echo() {
        let mut m = quiet_mixer();
        m.set_global(GlobalParam::Fx(FxParam::DelaySync), 0.0);
        m.set_global(GlobalParam::Fx(FxParam::DelayMs), 0.0); // 10 ms = 441 samples
        let mut s = slots();
        s[0].strip.set(ChannelParam::DelaySend, 1.0);
        s[0].buf.l[0] = 0.5;
        s[0].buf.r[0] = 0.5;
        let (l, _) = run(&mut m, &mut s);
        assert!(peak(&l[430..460]) > 0.05);
    }

    #[test]
    fn output_is_clipped_and_finite() {
        let mut m = quiet_mixer();
        let mut s = slots();
        for slot in &mut s {
            slot.strip.set(ChannelParam::Level, 1.0);
            slot.buf.l.fill(5.0);
            slot.buf.r.fill(5.0);
        }
        let (l, r) = run(&mut m, &mut s);
        assert!(all_finite(&l) && all_finite(&r));
        assert!(peak(&l) <= 1.0);
    }

    #[test]
    fn meters_are_by_position_with_master_last() {
        let mut m = quiet_mixer();
        let mut s = slots_with(3, 0.25);
        run(&mut m, &mut s);
        let meters = m.take_meters();
        assert!(meters[3] > 0.1, "{meters:?}");
        assert!(meters[MAX_MACHINES] > 0.1, "{meters:?}");
        let others = (0..MAX_MACHINES).filter(|&i| i != 3);
        assert!(others.into_iter().all(|i| meters[i] == 0.0), "{meters:?}");
    }

    #[test]
    fn guard_reports_nan_only() {
        let mut m = quiet_mixer();
        assert!(!guard_fired(&mut m, &mut slots_with(0, 0.25)));
        assert!(guard_fired(&mut m, &mut slots_with(0, f32::NAN)));
        assert!(!guard_fired(&mut m, &mut slots_with(0, 0.25)));
    }

    #[test]
    fn nan_input_is_contained() {
        let mut m = quiet_mixer();
        let mut s = slots_with(0, f32::NAN);
        s[0].strip.set(ChannelParam::ReverbSend, 1.0);
        let (l, _) = run(&mut m, &mut s);
        assert!(all_finite(&l));
        fill(&mut s, 0, 0.0);
        let (l, _) = run(&mut m, &mut s);
        assert!(all_finite(&l), "reverb state was reset");
    }
}
