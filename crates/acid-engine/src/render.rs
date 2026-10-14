//! Offline rendering to buffers and 16-bit stereo WAV files.

use crate::engine::Engine;
use crate::project::Project;
use crate::rack::CompiledSong;
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
    let _ = render_tail(&mut engine, &mut |cl: &[f32], cr: &[f32]| {
        l.extend_from_slice(cl);
        r.extend_from_slice(cr);
        Ok::<(), std::convert::Infallible>(())
    });
    (l, r)
}

/// Hands FX tail audio from a stopped engine to `sink`: up to
/// `MAX_TAIL_SECONDS`, stopping early once a chunk is silent.
fn render_tail<E>(
    engine: &mut Engine,
    sink: &mut impl FnMut(&[f32], &[f32]) -> Result<(), E>,
) -> Result<(), E> {
    let mut cl = vec![0.0; CHUNK];
    let mut cr = vec![0.0; CHUNK];
    for _ in 0..(MAX_TAIL_SECONDS * EXPORT_SAMPLE_RATE as usize / CHUNK) {
        engine.process(&mut cl, &mut cr);
        sink(&cl, &cr)?;
        if cl.iter().chain(cr.iter()).all(|v| v.abs() < SILENCE) {
            break;
        }
    }
    Ok(())
}

/// Renders the whole song from bar 1 (the loop range is ignored so the
/// export always ends), then lets FX tails ring out. `progress` gets
/// 0..=1 and finishes with 1.0.
pub fn render_song(project: &Project, progress: impl FnMut(f32)) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let cap = MAX_SONG_SECONDS * EXPORT_SAMPLE_RATE as usize;
    let _ = render_song_to(project, cap, progress, |cl: &[f32], cr: &[f32]| {
        l.extend_from_slice(cl);
        r.extend_from_slice(cr);
        Ok::<(), std::convert::Infallible>(())
    });
    (l, r)
}

/// The song render loop: hands each rendered chunk to `sink`, stopping
/// at the first sink error.
fn render_song_to<E>(
    project: &Project,
    cap: usize,
    mut progress: impl FnMut(f32),
    mut sink: impl FnMut(&[f32], &[f32]) -> Result<(), E>,
) -> Result<(), E> {
    let sr = EXPORT_SAMPLE_RATE as f32;
    let mut p = project.clone();
    p.song.loop_range = None;
    let mut engine = Engine::from_project(sr, &p);
    engine.set_mode(Mode::Song);
    engine.set_song_start(0);
    engine.play();
    let compiled = CompiledSong::compile(&p.song, &p.rack_uids());
    let steps = song::compiled_total_steps(&compiled, song::Banks::of(&p)).max(1) as f64;
    let step_samples = 60.0 / p.bpm.clamp(40.0, 300.0) as f64 / 4.0 * sr as f64;
    let expected = steps * step_samples;
    let mut rendered = 0;
    let mut cl = vec![0.0; CHUNK];
    let mut cr = vec![0.0; CHUNK];
    progress(0.0);
    while engine.is_playing() && rendered < cap {
        engine.process(&mut cl, &mut cr);
        sink(&cl, &cr)?;
        rendered += CHUNK;
        progress((rendered as f64 / expected).min(0.99) as f32);
    }
    // Usually the song has ended (and stopped) itself; a second stop
    // would hard-silence the FM tail.
    if engine.is_playing() {
        engine.stop();
    }
    render_tail(&mut engine, &mut sink)?;
    progress(1.0);
    Ok(())
}

/// Renders the song straight into a WAV file, chunk by chunk, so a long
/// song is never held in memory whole.
pub fn render_song_wav(
    project: &Project,
    path: &Path,
    progress: impl FnMut(f32),
) -> Result<(), hound::Error> {
    let mut w = hound::WavWriter::create(path, wav_spec())?;
    let cap = MAX_SONG_SECONDS * EXPORT_SAMPLE_RATE as usize;
    render_song_to(project, cap, progress, |l: &[f32], r: &[f32]| {
        write_frames(&mut w, l, r)
    })?;
    w.finalize()
}

fn wav_spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 2,
        sample_rate: EXPORT_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

fn write_frames<W: std::io::Write + std::io::Seek>(
    w: &mut hound::WavWriter<W>,
    l: &[f32],
    r: &[f32],
) -> Result<(), hound::Error> {
    for (a, b) in l.iter().zip(r) {
        w.write_sample((a.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        w.write_sample((b.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    Ok(())
}

pub fn write_wav(path: &Path, l: &[f32], r: &[f32]) -> Result<(), hound::Error> {
    let mut w = hound::WavWriter::create(path, wav_spec())?;
    write_frames(&mut w, l, r)?;
    w.finalize()
}

pub fn render_patterns_wav(project: &Project, bars: u32, path: &Path) -> Result<(), hound::Error> {
    let (l, r) = render_patterns(project, bars);
    write_wav(path, &l, &r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::MachineId;
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
            cells: crate::machine::MachineId::ALL
                .iter()
                .map(|m| {
                    let pattern = Some(crate::project::PatternRef(0));
                    (
                        m.uid(),
                        crate::project::Cell {
                            pattern,
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        };
        p.song.bars = vec![bar.clone(), bar];
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
    fn song_export_keeps_the_fm_release_tail() {
        use crate::params::{OpParam, fm_op_param};
        let mut p = Project {
            bpm: 120.0, // 1 bar = 2 s
            ..Project::default()
        };
        p.fm_mut(MachineId::Fm.uid()).unwrap().patterns[0]
            .notes
            .push(crate::project::Note {
                start: 0,
                len: 8,
                pitch: 60,
                vel: 120,
            });
        // OP1 releases slowly: the note rings past the song's end.
        p.set_param(fm_op_param(0, OpParam::R4), 0.3);
        let mut bar = crate::project::Bar::default();
        bar.cell_mut(MachineId::Fm.uid()).pattern = Some(crate::project::PatternRef(0));
        p.song.bars = vec![bar];
        let (l, _) = render_song(&p, |_| {});
        let sr = EXPORT_SAMPLE_RATE as usize;
        assert!(l.len() > 2 * sr + sr / 4, "a tail follows the song");
        assert!(
            rms(&l[2 * sr..2 * sr + sr / 4]) > 1e-3,
            "the FM release rings on after the song ends"
        );
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
        let mut seen = Vec::new();
        let (l, _) = render_song(&crate::demo::demo_project(), |f| seen.push(f));
        assert!(l.len() <= 2 * CHUNK);
        assert!(rms(&l) < 1e-3, "an empty song is silent");
        assert_eq!(seen.last().copied(), Some(1.0));
    }

    #[test]
    fn the_song_length_cap_stops_a_runaway_render() {
        let mut p = two_bar_song();
        let bar = p.song.bars[0].clone();
        p.song.bars = vec![bar; 999];
        let cap = 2 * CHUNK;
        let (mut n, mut seen) = (0, Vec::new());
        render_song_to(
            &p,
            cap,
            |f| seen.push(f),
            |l: &[f32], _: &[f32]| {
                n += l.len();
                Ok::<(), std::convert::Infallible>(())
            },
        )
        .unwrap();
        let tail = 10 * EXPORT_SAMPLE_RATE as usize;
        assert!(n >= cap, "rendered up to the cap ({n})");
        assert!(n <= cap + tail, "capped, then only the tail ({n})");
        assert_eq!(seen.last().copied(), Some(1.0));
    }

    #[test]
    fn streamed_song_wav_matches_the_buffered_render() {
        let path = std::env::temp_dir().join(format!("sidacid-song-{}.wav", std::process::id()));
        let p = two_bar_song();
        let mut seen = Vec::new();
        render_song_wav(&p, &path, |f| seen.push(f)).unwrap();
        let (l, r) = render_song(&p, |_| {});
        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec(), wav_spec());
        let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(samples.len(), 2 * l.len());
        let quantise = |v: f32| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        for (i, (a, b)) in l.iter().zip(&r).enumerate() {
            assert_eq!(samples[2 * i], quantise(*a));
            assert_eq!(samples[2 * i + 1], quantise(*b));
        }
        assert!(seen.windows(2).all(|w| w[1] >= w[0]));
        assert_eq!(seen.last().copied(), Some(1.0));
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
