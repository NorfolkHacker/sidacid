//! Pure helpers for the sampler panel and the roll: waveform peaks,
//! frame/pixel mapping, marker editing, key names and row labels.

use super::step_edit::NOTE_NAMES;
use acid_engine::sampler::slot::{SlotData, SlotMeta, SlotMode};
use eframe::egui::Color32;

/// Row tints for sampler zones in the roll, cycled by slot.
pub const SLOT_TINTS: [Color32; 4] = [
    Color32::from_rgb(0xB8, 0xF5, 0x3A),
    Color32::from_rgb(0x4C, 0xB8, 0xFF),
    Color32::from_rgb(0xFF, 0xC8, 0x3C),
    Color32::from_rgb(0xFF, 0x50, 0x3C),
];

/// (min, max) of the samples in each of `columns` equal spans, -1..1.
pub fn peaks(pcm: &[i16], channels: usize, columns: usize) -> Vec<(f32, f32)> {
    let ch = channels.max(1);
    let frames = pcm.len() / ch;
    (0..columns)
        .map(|c| {
            let a = c * frames / columns.max(1);
            let b = ((c + 1) * frames / columns.max(1)).max(a + 1).min(frames);
            if a >= frames {
                return (0.0, 0.0);
            }
            let span = &pcm[a * ch..b * ch];
            let lo = span.iter().copied().min().unwrap_or(0) as f32 / 32_768.0;
            let hi = span.iter().copied().max().unwrap_or(0) as f32 / 32_768.0;
            (lo, hi)
        })
        .collect()
}

pub fn frame_to_x(frame: u32, frames: u32, left: f32, width: f32) -> f32 {
    left + frame as f32 / frames.max(1) as f32 * width
}

pub fn x_to_frame(x: f32, frames: u32, left: f32, width: f32) -> u32 {
    (((x - left) / width.max(1.0)).clamp(0.0, 1.0) * frames as f32).round() as u32
}

/// The marker nearest `x` within `tolerance` pixels.
pub fn nearest_marker(xs: &[f32], x: f32, tolerance: f32) -> Option<usize> {
    xs.iter()
        .enumerate()
        .map(|(i, m)| (i, (m - x).abs()))
        .filter(|(_, d)| *d <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Slice(usize),
    Start,
    End,
    LoopStart,
    LoopEnd,
}

/// The draggable markers of a slot and their frames.
pub fn markers(meta: &SlotMeta) -> Vec<(Marker, u32)> {
    match meta.mode {
        SlotMode::Slice => meta
            .slices
            .iter()
            .enumerate()
            .map(|(i, &f)| (Marker::Slice(i), f))
            .collect(),
        SlotMode::Chromatic => {
            let mut v = vec![(Marker::Start, meta.start), (Marker::End, meta.end)];
            if meta.loop_on {
                v.push((Marker::LoopStart, meta.loop_start));
                v.push((Marker::LoopEnd, meta.loop_end));
            }
            v
        }
    }
}

/// Moves a marker, clamping it so the meta stays well-formed. Returns
/// whether anything changed. Slice 0 never moves.
pub fn set_marker(meta: &mut SlotMeta, marker: Marker, frame: u32, frames: u32) -> bool {
    if !meta.is_well_formed(frames) {
        return false;
    }
    let before = meta.clone();
    match marker {
        Marker::Slice(0) => return false,
        Marker::Slice(i) => {
            if i >= meta.slices.len() {
                return false;
            }
            let lo = meta.slices[i - 1] + 1;
            let hi = meta
                .slices
                .get(i + 1)
                .map_or(frames.saturating_sub(1), |&n| n - 1);
            if lo > hi {
                return false;
            }
            meta.slices[i] = frame.clamp(lo, hi);
        }
        Marker::Start => {
            meta.start = frame.min(meta.end - 1);
            meta.loop_start = meta.loop_start.max(meta.start);
            meta.loop_end = meta.loop_end.max(meta.loop_start + 1);
        }
        Marker::End => {
            meta.end = frame.clamp(meta.start + 1, frames);
            meta.loop_end = meta.loop_end.min(meta.end);
            meta.loop_start = meta.loop_start.min(meta.loop_end - 1);
        }
        Marker::LoopStart => meta.loop_start = frame.clamp(meta.start, meta.loop_end - 1),
        Marker::LoopEnd => meta.loop_end = frame.clamp(meta.loop_start + 1, meta.end),
    }
    *meta != before
}

/// "C1" for MIDI 24.
pub fn key_name(key: u8) -> String {
    format!("{}{}", NOTE_NAMES[key as usize % 12], key as i32 / 12 - 1)
}

/// "Keys C1–D#2".
pub fn zone_text(meta: &SlotMeta) -> String {
    let (lo, hi) = meta.key_range();
    format!("Keys {}–{}", key_name(lo), key_name(hi.min(127)))
}

/// The roll's label for a sampler row: slot index and "Name ·05"
/// (slice 5) or "Name C3" (chromatic key).
pub fn row_label(slots: &[Option<SlotData>], key: u8) -> Option<(usize, String)> {
    slots.iter().enumerate().find_map(|(i, s)| {
        let d = s.as_ref()?;
        let (lo, hi) = d.meta.key_range();
        if key < lo || key > hi {
            return None;
        }
        let short: String = d
            .name
            .chars()
            .take(4)
            .collect::<String>()
            .trim_end()
            .to_string();
        Some((
            i,
            match d.meta.mode {
                SlotMode::Slice => format!("{short} ·{:02}", key - lo + 1),
                SlotMode::Chromatic => format!("{short} {}", key_name(key)),
            },
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use acid_engine::sampler::slot::{SlotData, SlotMeta};

    #[test]
    fn peaks_cover_each_column() {
        let pcm: Vec<i16> = (0..1000)
            .map(|i| if i < 500 { 16_384 } else { -32_768 })
            .collect();
        let p = peaks(&pcm, 1, 10);
        assert_eq!(p.len(), 10);
        assert_eq!(p[0], (0.5, 0.5));
        assert_eq!(p[9], (-1.0, -1.0));
        assert!(peaks(&[], 1, 10).iter().all(|c| *c == (0.0, 0.0)));
    }

    #[test]
    fn frames_and_pixels_round_trip() {
        assert_eq!(frame_to_x(500, 1000, 10.0, 200.0), 110.0);
        assert_eq!(x_to_frame(110.0, 1000, 10.0, 200.0), 500);
        assert_eq!(x_to_frame(-50.0, 1000, 10.0, 200.0), 0);
        assert_eq!(x_to_frame(9999.0, 1000, 10.0, 200.0), 1000);
        assert_eq!(nearest_marker(&[10.0, 50.0, 52.0], 51.5, 4.0), Some(2));
        assert_eq!(nearest_marker(&[10.0], 30.0, 4.0), None);
    }

    #[test]
    fn markers_move_within_their_neighbours() {
        let mut m = SlotMeta::sliced(36, vec![0, 100, 200, 300], 120.0, 1000);
        assert!(set_marker(&mut m, Marker::Slice(2), 150, 1000));
        assert_eq!(m.slices, vec![0, 100, 150, 300]);
        assert!(set_marker(&mut m, Marker::Slice(2), 50, 1000));
        assert_eq!(m.slices[2], 101, "clamped above the previous slice");
        assert!(
            !set_marker(&mut m, Marker::Slice(0), 10, 1000),
            "slice 0 stays at the start"
        );
        let mut c = SlotMeta::chromatic(48, 71, 60, 1000);
        c.loop_on = true;
        assert!(set_marker(&mut c, Marker::Start, 200, 1000));
        assert!(
            !set_marker(&mut c, Marker::LoopStart, 100, 1000),
            "already clamped to the start"
        );
        assert_eq!(c.loop_start, 200, "the loop stays inside the play range");
        assert!(set_marker(&mut c, Marker::End, 900, 1000));
        assert_eq!(c.loop_end, 900, "loop end follows a shorter end");
        assert!(c.is_well_formed(1000));
        assert_eq!(markers(&c).len(), 4);
        assert_eq!(markers(&m).len(), 4);
    }

    #[test]
    fn set_marker_never_panics_and_stays_well_formed() {
        for frames in [1u32, 2, 3, 1000] {
            let mut metas = vec![SlotMeta::sliced(36, vec![0], 120.0, frames)];
            if frames > 3 {
                metas.push(SlotMeta::sliced(36, vec![0, 1, frames / 2], 120.0, frames));
            }
            for on in [false, true] {
                let mut c = SlotMeta::chromatic(48, 71, 60, frames);
                c.loop_on = on;
                metas.push(c);
            }
            let kinds = [
                Marker::Slice(0),
                Marker::Slice(1),
                Marker::Slice(2),
                Marker::Slice(9),
                Marker::Start,
                Marker::End,
                Marker::LoopStart,
                Marker::LoopEnd,
            ];
            for meta in &metas {
                let was_ok = meta.is_well_formed(frames);
                for kind in kinds {
                    for f in [0, 1, 2, frames.saturating_sub(1), frames, u32::MAX] {
                        let mut m = meta.clone();
                        set_marker(&mut m, kind, f, frames);
                        if was_ok {
                            assert!(m.is_well_formed(frames), "{kind:?} {f} frames {frames}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn names_zones_and_row_labels() {
        assert_eq!(key_name(24), "C1");
        assert_eq!(key_name(61), "C#4");
        let tone = vec![0.1f32; 1000];
        let mut slots: Vec<Option<SlotData>> = vec![None; 16];
        slots[0] = Some(SlotData::new(
            "Amen Brother",
            22_050,
            1,
            &tone,
            SlotMeta::sliced(24, vec![0, 100, 200, 300, 400], 120.0, 1000),
        ));
        slots[2] = Some(SlotData::new(
            "Vox",
            22_050,
            1,
            &tone,
            SlotMeta::chromatic(48, 59, 48, 1000),
        ));
        assert_eq!(zone_text(&slots[0].as_ref().unwrap().meta), "Keys C1–E1");
        assert_eq!(row_label(&slots, 28), Some((0, "Amen ·05".to_string())));
        assert_eq!(row_label(&slots, 52), Some((2, "Vox E3".to_string())));
        assert_eq!(row_label(&slots, 40), None);
    }
}
