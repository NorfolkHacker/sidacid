//! Offline rendering to buffers and 16-bit stereo WAV files.

use crate::engine::Engine;
use crate::project::Project;
use crate::sequencer::Mode;
use crate::song;
use std::path::Path;

pub const EXPORT_SAMPLE_RATE: u32 = 44_100;
const CHUNK: usize = 4410;
const MAX_TAIL_SECONDS: usize = 10;
const SILENCE: f32 = 1.0e-4;
pub const MAX_SONG_SECONDS: usize = 3600;

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
    render_tail(&mut engine, &mut l, &mut r);
    (l, r)
}

/// Appends FX tail audio after a stopped engine: up to `MAX_TAIL_SECONDS`,
/// stopping early once a chunk is silent.
fn render_tail(engine: &mut Engine, l: &mut Vec<f32>, r: &mut Vec<f32>) {
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
}

/// Renders the whole song from bar 1 (the loop range is ignored so the
/// export always ends), then lets FX tails ring out. `progress` gets
/// 0..=1 and finishes with 1.0.
pub fn render_song(project: &Project, mut progress: impl FnMut(f32)) -> (Vec<f32>, Vec<f32>) {
    let sr = EXPORT_SAMPLE_RATE as f32;
    let mut p = project.clone();
    p.song.loop_range = None;
    let mut engine = Engine::from_project(sr, &p);
    engine.set_mode(Mode::Song);
    engine.set_song_start(0);
    engine.play();
    let (synths, drums) = song::banks(&p);
    let steps = song::total_steps(&p.song, synths, drums).max(1) as f64;
    let step_samples = 60.0 / p.bpm.clamp(40.0, 300.0) as f64 / 4.0 * sr as f64;
    let expected = steps * step_samples;
    let cap = MAX_SONG_SECONDS * EXPORT_SAMPLE_RATE as usize;
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let mut cl = vec![0.0; CHUNK];
    let mut cr = vec![0.0; CHUNK];
    progress(0.0);
    while engine.is_playing() && l.len() < cap {
        engine.process(&mut cl, &mut cr);
        l.extend_from_slice(&cl);
        r.extend_from_slice(&cr);
        progress((l.len() as f64 / expected).min(0.99) as f32);
    }
    engine.stop();
    render_tail(&mut engine, &mut l, &mut r);
    progress(1.0);
    (l, r)
}

pub fn render_song_wav(
    project: &Project,
    path: &Path,
    progress: impl FnMut(f32),
) -> Result<(), hound::Error> {
    let (l, r) = render_song(project, progress);
    write_wav(path, &l, &r)
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

    fn two_bar_song() -> Project {
        let mut p = crate::demo::demo_project();
        p.bpm = 120.0;
        let bar = crate::project::Bar {
            patterns: [Some(crate::project::PatternRef(0)); 4],
            ..Default::default()
        };
        p.song.bars = vec![bar, bar];
        p
    }

    #[test]
    fn song_render_covers_the_song_plus_tail() {
        let mut seen = Vec::new();
        let (l, r) = render_song(&two_bar_song(), |f| seen.push(f));
        assert_eq!(l.len(), r.len());
        let sr = EXPORT_SAMPLE_RATE as usize;
        assert!(l.len() >= 4 * sr, "two bars at 120 BPM = 4 s");
        assert!(l.len() <= 15 * sr);
        assert!(rms(&l[..4 * sr]) > 0.02);
        assert!(
            seen.windows(2).all(|w| w[1] >= w[0]),
            "progress is monotonic"
        );
        assert_eq!(seen.last().copied(), Some(1.0));
    }

    #[test]
    fn looped_song_still_ends_when_exported() {
        let mut p = two_bar_song();
        p.song.loop_range = Some((0, 1));
        let (l, _) = render_song(&p, |_| {});
        assert!(l.len() <= 15 * EXPORT_SAMPLE_RATE as usize);
    }

    #[test]
    fn empty_song_renders_almost_nothing() {
        let (l, _) = render_song(&crate::demo::demo_project(), |_| {});
        assert!(l.len() <= 2 * EXPORT_SAMPLE_RATE as usize);
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
