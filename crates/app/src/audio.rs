//! Live output through cpal. The engine runs inside the device callback
//! via `EngineHost`; the UI talks to it only through the command and
//! event rings held by `AudioLink`.

use acid_engine::engine::Engine;
use acid_engine::host::{Command, EngineHost, Event, channel};
use acid_engine::project::Project;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use rtrb::{Consumer, Producer, PushError};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

/// Frames rendered per engine call inside the device callback.
pub const RENDER_CHUNK: usize = 4096;

pub struct AudioLink {
    _stream: cpal::Stream,
    commands: Producer<Command>,
    events: Consumer<Event>,
    failed: Arc<AtomicBool>,
    /// Frames in the device's latest callback (0 before the first).
    buffer_frames: Arc<AtomicU32>,
    pub sample_rate: f32,
    pub device_name: String,
}

impl AudioLink {
    /// Opens the default output device and starts the engine on it with
    /// `project` loaded.
    pub fn start(project: &Project) -> Result<AudioLink, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no audio output device found")?;
        let device_name = device.name().unwrap_or_else(|_| "audio device".into());
        let supported = device
            .default_output_config()
            .map_err(|e| format!("audio device: {e}"))?;
        let sample_rate = supported.sample_rate().0 as f32;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let (engine_host, commands, events) = channel(Engine::from_project(sample_rate, project));
        let failed = Arc::new(AtomicBool::new(false));
        let buffer_frames = Arc::new(AtomicU32::new(0));
        let shared = (failed.clone(), buffer_frames.clone());
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, engine_host, shared),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, engine_host, shared),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, engine_host, shared),
            other => Err(format!("unsupported sample format {other:?}")),
        }?;
        stream
            .play()
            .map_err(|e| format!("could not start audio: {e}"))?;
        Ok(AudioLink {
            _stream: stream,
            commands,
            events,
            failed,
            buffer_frames,
            sample_rate,
            device_name,
        })
    }

    /// Queues a command; hands it back if the ring is full.
    pub fn send(&mut self, cmd: Command) -> Result<(), Command> {
        self.commands.push(cmd).map_err(|PushError::Full(c)| c)
    }

    pub fn poll(&mut self) -> Option<Event> {
        self.events.pop().ok()
    }

    pub fn buffer_frames(&self) -> u32 {
        self.buffer_frames.load(Ordering::Relaxed)
    }

    pub fn has_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    /// Builds a fresh engine for `project` here on the UI thread and
    /// swaps it in whole on the audio thread.
    pub fn load_project(&mut self, project: &Project) -> Result<(), Command> {
        self.send(Command::ReplaceEngine(Box::new(Engine::from_project(
            self.sample_rate,
            project,
        ))))
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut host: EngineHost,
    (failed, buffer_frames): (Arc<AtomicBool>, Arc<AtomicU32>),
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate.0 as f32;
    let mut l = vec![0.0f32; RENDER_CHUNK];
    let mut r = vec![0.0f32; RENDER_CHUNK];
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                let started = Instant::now();
                let frames = data.len() / channels.max(1);
                let mut render = |a: &mut [f32], b: &mut [f32]| host.process(a, b);
                fill_interleaved(&mut render, &mut l, &mut r, data, channels);
                buffer_frames.store(frames as u32, Ordering::Relaxed);
                if frames > 0 {
                    let budget = frames as f32 / sample_rate;
                    host.set_cpu_load(started.elapsed().as_secs_f32() / budget);
                }
            },
            move |_err| {
                failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| format!("could not open audio stream: {e}"))
}

/// Renders into `l`/`r` in chunks and writes interleaved device frames:
/// left/right on channels 0/1, their average on a mono device, silence on
/// any further channels.
pub fn fill_interleaved<T>(
    render: &mut impl FnMut(&mut [f32], &mut [f32]),
    l: &mut [f32],
    r: &mut [f32],
    data: &mut [T],
    channels: usize,
) where
    T: Sample + FromSample<f32>,
{
    let channels = channels.max(1);
    let frames = data.len() / channels;
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(l.len()).min(r.len());
        let (bl, br) = (&mut l[..n], &mut r[..n]);
        render(bl, br);
        for i in 0..n {
            let frame = &mut data[(done + i) * channels..(done + i + 1) * channels];
            if channels == 1 {
                frame[0] = T::from_sample(0.5 * (bl[i] + br[i]));
            } else {
                frame[0] = T::from_sample(bl[i]);
                frame[1] = T::from_sample(br[i]);
                for s in &mut frame[2..] {
                    *s = T::EQUILIBRIUM;
                }
            }
        }
        done += n;
    }
    // A partial frame at the end (data.len() not a multiple of channels).
    for s in &mut data[frames * channels..] {
        *s = T::EQUILIBRIUM;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(l: &mut [f32], r: &mut [f32]) {
        l.fill(0.5);
        r.fill(-0.25);
    }

    #[test]
    fn stereo_interleaves_across_chunks() {
        let mut l = vec![0.0; RENDER_CHUNK];
        let mut r = vec![0.0; RENDER_CHUNK];
        let frames = RENDER_CHUNK * 3 + 17;
        let mut data = vec![0.0f32; 2 * frames];
        let mut n = 0.0f32;
        let mut ramp = |l: &mut [f32], r: &mut [f32]| {
            for i in 0..l.len() {
                l[i] = n;
                r[i] = -n;
                n += 1.0;
            }
        };
        fill_interleaved(&mut ramp, &mut l, &mut r, &mut data, 2);
        for (k, frame) in data.chunks(2).enumerate() {
            assert_eq!(frame, &[k as f32, -(k as f32)], "frame {k}");
        }
    }

    #[test]
    fn mono_device_gets_the_average() {
        let mut l = vec![0.0; 64];
        let mut r = vec![0.0; 64];
        let mut data = vec![0.0f32; 100];
        fill_interleaved(&mut constant, &mut l, &mut r, &mut data, 1);
        assert!(data.iter().all(|v| (*v - 0.125).abs() < 1e-6));
    }

    #[test]
    fn extra_channels_are_silent() {
        let mut l = vec![0.0; 64];
        let mut r = vec![0.0; 64];
        let mut data = vec![1.0f32; 4 * 10];
        fill_interleaved(&mut constant, &mut l, &mut r, &mut data, 4);
        for frame in data.chunks(4) {
            assert_eq!(frame, &[0.5, -0.25, 0.0, 0.0]);
        }
    }

    #[test]
    fn integer_formats_convert() {
        let mut l = vec![0.0; 64];
        let mut r = vec![0.0; 64];
        let mut data = vec![0i16; 2 * 8];
        fill_interleaved(&mut constant, &mut l, &mut r, &mut data, 2);
        assert!(data[0] > 16_000 && data[1] < -8_000);
    }

    #[test]
    fn a_partial_trailing_frame_is_silenced() {
        let mut l = vec![0.0; 64];
        let mut r = vec![0.0; 64];
        let mut data = vec![1.0f32; 2 * 10 + 1];
        fill_interleaved(&mut constant, &mut l, &mut r, &mut data, 2);
        assert_eq!(data[19], -0.25);
        assert_eq!(data[20], 0.0);
    }
}
