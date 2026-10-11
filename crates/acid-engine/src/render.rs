//! Offline rendering to buffers and 16-bit stereo WAV files.

use crate::engine::Engine;
use crate::project::Project;
use std::path::Path;

pub const EXPORT_SAMPLE_RATE: u32 = 44_100;
const CHUNK: usize = 4410;
const MAX_TAIL_SECONDS: usize = 10;
const SILENCE: f32 = 1.0e-4;

/// Renders the selected patterns for `bars` bars of 16 steps, then lets
/// FX tails ring out (up to 10 s, stopping after 0.1 s of silence).
pub fn render_patterns(project: &Project, bars: u32) -> (Vec<f32>, Vec<f32>) {
    let sr = EXPORT_SAMPLE_RATE as f32;
    let mut engine = Engine::new(sr);
    engine.load_project(project);
    engine.play();
    let step_seconds = 60.0 / project.bpm.clamp(40.0, 300.0) as f64 / 4.0;
    let total = (bars as f64 * 16.0 * step_seconds * sr as f64).round() as usize;
    let mut l = vec![0.0; total];
    let mut r = vec![0.0; total];
    engine.process(&mut l, &mut r);
    engine.stop();
    let mut cl = vec![0.0; CHUNK];
    let mut cr = vec![0.0; CHUNK];
    for _ in 0..(MAX_TAIL_SECONDS * EXPORT_SAMPLE_RATE as usize / CHUNK) {
        engine.process(&mut cl, &mut cr);
        l.extend_from_slice(&cl);
        r.extend_from_slice(&cr);
        if cl.iter().chain(cr.iter()).all(|v| v.abs() < SILENCE) {
            break;
        }
    }
    (l, r)
}

pub fn write_wav(path: &Path, l: &[f32], r: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: EXPORT_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for (a, b) in l.iter().zip(r) {
        w.write_sample((a.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        w.write_sample((b.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    w.finalize()
}

pub fn render_patterns_wav(project: &Project, bars: u32, path: &Path) -> Result<(), hound::Error> {
    let (l, r) = render_patterns(project, bars);
    write_wav(path, &l, &r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::rms;

    #[test]
    fn render_length_covers_bars_plus_tail() {
        let mut p = crate::demo::demo_project();
        p.bpm = 120.0; // 1 bar = 2 s
        let (l, r) = render_patterns(&p, 1);
        assert_eq!(l.len(), r.len());
        assert!(l.len() >= 2 * EXPORT_SAMPLE_RATE as usize);
        assert!(l.len() <= 12 * EXPORT_SAMPLE_RATE as usize + 4410);
        assert!(rms(&l[..88_200]) > 0.02);
    }

    #[test]
    fn wav_file_is_written() {
        let path = std::env::temp_dir().join(format!("sidacid-render-{}.wav", std::process::id()));
        let p = crate::demo::demo_project();
        render_patterns_wav(&p, 1, &path).unwrap();
        let reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, EXPORT_SAMPLE_RATE);
        assert_eq!(spec.bits_per_sample, 16);
        assert!(reader.len() > 0);
        std::fs::remove_file(&path).unwrap();
    }
}
