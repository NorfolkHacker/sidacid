//! Bit-exact audio pins: the rack refactor must not change one sample.
//! The fixture is a v2 project; every later model loads it via migration.

use acid_engine::project::Project;
use acid_engine::render::{render_patterns, render_song};

const PATTERN_HASH: u64 = 0x37e7_dc98_f0d6_23f1;
const SONG_HASH: u64 = 0xb18d_e278_6f54_1314;

fn fnv(l: &[f32], r: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (a, b) in l.iter().zip(r) {
        for bits in [a.to_bits(), b.to_bits()] {
            for byte in bits.to_le_bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}

fn fixture() -> Project {
    let text = include_str!("fixtures/golden_v2.sidacid");
    Project::from_ron(text).expect("golden fixture loads")
}

#[test]
fn pattern_mode_render_is_bit_identical() {
    let (l, r) = render_patterns(&fixture(), 4);
    let h = fnv(&l, &r);
    assert_eq!(
        h,
        PATTERN_HASH,
        "pattern render changed: {h:#x} ({} frames)",
        l.len()
    );
}

#[test]
fn song_mode_render_is_bit_identical() {
    let (l, r) = render_song(&fixture(), |_| {});
    let h = fnv(&l, &r);
    assert_eq!(
        h,
        SONG_HASH,
        "song render changed: {h:#x} ({} frames)",
        l.len()
    );
}

#[test]
fn the_golden_render_is_not_silence() {
    let (l, _) = render_song(&fixture(), |_| {});
    assert!(l.iter().any(|s| s.abs() > 0.01));
}
