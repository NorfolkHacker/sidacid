//! Decoding audio files (WAV, FLAC, MP3, OGG Vorbis) with symphonia.
//! Not real-time: called on the UI thread when a sample is loaded.

use super::MAX_SECONDS;
use std::fmt;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    pub sample_rate: u32,
    /// 1 or 2 (more channels are cut to the first two).
    pub channels: u8,
    /// Interleaved, -1..1.
    pub samples: Vec<f32>,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.sample_rate.max(1) as f32
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Unsupported(String),
    Empty,
    TooLong,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Unsupported(e) => write!(f, "the file could not be read as audio ({e})"),
            DecodeError::Empty => f.write_str("the file has no audio"),
            DecodeError::TooLong => write!(f, "the sample is longer than {MAX_SECONDS:.0} seconds"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub fn decode(bytes: &[u8], extension: Option<&str>) -> Result<Decoded, DecodeError> {
    let unsupported = |e: Error| DecodeError::Unsupported(e.to_string());
    let mss = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(unsupported)?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| DecodeError::Unsupported("no audio track".into()))?;
    let track_id = track.id;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(unsupported)?;
    let mut channels = 0usize;
    let mut samples: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(unsupported(e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                let spec = *audio.spec();
                sample_rate = spec.rate;
                let ch = spec.channels.count();
                let mut buf = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
                buf.copy_interleaved_ref(audio);
                let keep = ch.min(2);
                channels = keep;
                for frame in buf.samples().chunks(ch.max(1)) {
                    samples.extend_from_slice(&frame[..keep.min(frame.len())]);
                }
                if sample_rate > 0
                    && samples.len() / keep.max(1) > (MAX_SECONDS * sample_rate as f32) as usize
                {
                    return Err(DecodeError::TooLong);
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(unsupported(e)),
        }
    }
    if channels == 0 || samples.is_empty() || sample_rate == 0 {
        return Err(DecodeError::Empty);
    }
    Ok(Decoded {
        sample_rate,
        channels: channels as u8,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(channels: u16, rate: u32, frames: usize) -> Vec<u8> {
        let mut cur = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::new(&mut cur, spec).unwrap();
        for i in 0..frames {
            for c in 0..channels {
                let v = ((i as f32 * 0.05).sin() * 16_000.0) as i16;
                w.write_sample(if c == 0 { v } else { -v }).unwrap();
            }
        }
        w.finalize().unwrap();
        cur.into_inner()
    }

    #[test]
    fn decodes_mono_and_stereo_wav() {
        let d = decode(&wav(1, 22_050, 11_025), Some("wav")).unwrap();
        assert_eq!((d.sample_rate, d.channels, d.frames()), (22_050, 1, 11_025));
        assert!((d.seconds() - 0.5).abs() < 1e-6);
        assert!((d.samples[20] - (20.0f32 * 0.05).sin() * 16_000.0 / 32_768.0).abs() < 1e-3);
        let s = decode(&wav(2, 44_100, 1000), None).unwrap();
        assert_eq!((s.channels, s.frames()), (2, 1000));
        assert!(
            (s.samples[41] + s.samples[40]).abs() < 1e-4,
            "right is the negated left"
        );
    }

    #[test]
    fn refuses_garbage_empty_and_long_files() {
        assert!(matches!(
            decode(b"definitely not audio", Some("wav")),
            Err(DecodeError::Unsupported(_))
        ));
        assert_eq!(
            decode(&wav(1, 8_000, 0), Some("wav")),
            Err(DecodeError::Empty)
        );
        assert_eq!(
            decode(&wav(1, 8_000, 8_000 * 31), Some("wav")),
            Err(DecodeError::TooLong)
        );
        assert!(!DecodeError::TooLong.to_string().is_empty());
    }
}
