//! Pure step-grid logic: labels, row toggles, note nudging and the
//! step-write keyboard state machine (tracker layout).

use super::lane::NOTE_NAMES;
use acid_engine::steps::{Gate, SynthGrid, SynthStep};
use eframe::egui::Key;

pub fn cycle_gate(gate: Gate) -> Gate {
    match gate {
        Gate::Off => Gate::On,
        Gate::On => Gate::Tie,
        Gate::Tie => Gate::Off,
    }
}

pub fn gate_label(gate: Gate) -> &'static str {
    match gate {
        Gate::Off => "·",
        Gate::On => "■",
        Gate::Tie => "—",
    }
}

pub fn step_label(step: &SynthStep) -> &'static str {
    if step.gate == Gate::Off {
        "·"
    } else {
        NOTE_NAMES[step.note as usize % 12]
    }
}

/// Moves a step's pitch by `delta` semitones within the three-octave range.
pub fn nudge_note(step: SynthStep, delta: i32) -> SynthStep {
    let total =
        ((step.octave.clamp(-1, 1) as i32 + 1) * 12 + (step.note % 12) as i32 + delta).clamp(0, 35);
    SynthStep {
        note: (total % 12) as u8,
        octave: (total / 12) as i8 - 1,
        ..step
    }
}

/// Click behaviour of the grid rows: 1 Down, 2 Up, 3 Accent, 4 Slide, 5 Gate.
pub fn toggle_row(row: usize, step: SynthStep) -> SynthStep {
    match row {
        1 => SynthStep {
            octave: if step.octave < 0 { 0 } else { -1 },
            ..step
        },
        2 => SynthStep {
            octave: if step.octave > 0 { 0 } else { 1 },
            ..step
        },
        3 => SynthStep {
            accent: !step.accent,
            ..step
        },
        4 => SynthStep {
            slide: !step.slide,
            ..step
        },
        5 => SynthStep {
            gate: cycle_gate(step.gate),
            ..step
        },
        _ => step,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKey {
    /// Semitones above C of the writer's octave (0..=24).
    Note(u8),
    Rest,
    Tie,
    Accent,
    Slide,
    Left,
    Right,
    Back,
    OctaveDown,
    OctaveUp,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WriteAction {
    Set { step: usize, value: SynthStep },
    Moved,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepWriter {
    pub cursor: usize,
    pub octave: i8,
}

impl StepWriter {
    pub fn new(cursor: usize) -> Self {
        Self { cursor, octave: 0 }
    }

    pub fn handle(&mut self, key: WriteKey, grid: &SynthGrid) -> WriteAction {
        let len = (grid.length as usize).clamp(1, grid.steps.len());
        if self.cursor >= len {
            self.cursor = 0;
        }
        let here = self.cursor;
        let current = grid.steps[here];
        match key {
            WriteKey::Note(semis) => {
                let octave = (self.octave as i32 + semis as i32 / 12).clamp(-1, 1) as i8;
                let value = SynthStep {
                    note: semis % 12,
                    octave,
                    gate: Gate::On,
                    ..current
                };
                self.cursor = (here + 1) % len;
                WriteAction::Set { step: here, value }
            }
            WriteKey::Rest => {
                self.cursor = (here + 1) % len;
                WriteAction::Set {
                    step: here,
                    value: SynthStep {
                        gate: Gate::Off,
                        ..current
                    },
                }
            }
            WriteKey::Tie => {
                self.cursor = (here + 1) % len;
                WriteAction::Set {
                    step: here,
                    value: SynthStep {
                        gate: Gate::Tie,
                        ..current
                    },
                }
            }
            WriteKey::Accent => WriteAction::Set {
                step: here,
                value: SynthStep {
                    accent: !current.accent,
                    ..current
                },
            },
            WriteKey::Slide => WriteAction::Set {
                step: here,
                value: SynthStep {
                    slide: !current.slide,
                    ..current
                },
            },
            WriteKey::Left => {
                self.cursor = (here + len - 1) % len;
                WriteAction::Moved
            }
            WriteKey::Right => {
                self.cursor = (here + 1) % len;
                WriteAction::Moved
            }
            WriteKey::Back => {
                let prev = (here + len - 1) % len;
                self.cursor = prev;
                WriteAction::Set {
                    step: prev,
                    value: SynthStep {
                        gate: Gate::Off,
                        ..grid.steps[prev]
                    },
                }
            }
            WriteKey::OctaveDown => {
                self.octave = (self.octave - 1).max(-1);
                WriteAction::Moved
            }
            WriteKey::OctaveUp => {
                self.octave = (self.octave + 1).min(1);
                WriteAction::Moved
            }
            WriteKey::Exit => WriteAction::Exit,
        }
    }
}

/// Tracker piano layout: Z-row one octave, Q-row the octave above.
pub fn key_to_write(key: Key) -> Option<WriteKey> {
    use WriteKey::*;
    Some(match key {
        Key::Z => Note(0),
        Key::S => Note(1),
        Key::X => Note(2),
        Key::D => Note(3),
        Key::C => Note(4),
        Key::V => Note(5),
        Key::G => Note(6),
        Key::B => Note(7),
        Key::H => Note(8),
        Key::N => Note(9),
        Key::J => Note(10),
        Key::M => Note(11),
        Key::Comma | Key::Q => Note(12),
        Key::Num2 => Note(13),
        Key::W => Note(14),
        Key::Num3 => Note(15),
        Key::E => Note(16),
        Key::R => Note(17),
        Key::Num5 => Note(18),
        Key::T => Note(19),
        Key::Num6 => Note(20),
        Key::Y => Note(21),
        Key::Num7 => Note(22),
        Key::U => Note(23),
        Key::I => Note(24),
        Key::Space => Rest,
        Key::Minus => Tie,
        Key::A => Accent,
        Key::F => Slide,
        Key::ArrowLeft => Left,
        Key::ArrowRight => Right,
        Key::Backspace => Back,
        Key::OpenBracket => OctaveDown,
        Key::CloseBracket => OctaveUp,
        Key::Escape => Exit,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(note: u8, octave: i8) -> SynthStep {
        SynthStep {
            note,
            octave,
            gate: Gate::On,
            accent: false,
            slide: false,
        }
    }

    fn grid(length: u8) -> SynthGrid {
        SynthGrid {
            steps: vec![SynthStep::default(); acid_engine::project::STEPS],
            length,
        }
    }

    #[test]
    fn gate_cycles_off_on_tie() {
        assert_eq!(cycle_gate(Gate::Off), Gate::On);
        assert_eq!(cycle_gate(Gate::On), Gate::Tie);
        assert_eq!(cycle_gate(Gate::Tie), Gate::Off);
    }

    #[test]
    fn labels() {
        assert_eq!(step_label(&on(1, 0)), "C#");
        assert_eq!(step_label(&SynthStep::default()), "·");
        assert_eq!(gate_label(Gate::Tie), "—");
    }

    #[test]
    fn nudge_crosses_octaves_and_clamps() {
        assert_eq!(nudge_note(on(11, 0), 1), on(0, 1));
        assert_eq!(nudge_note(on(0, 0), -1), on(11, -1));
        assert_eq!(nudge_note(on(0, -1), -5), on(0, -1));
        assert_eq!(nudge_note(on(11, 1), 3), on(11, 1));
    }

    #[test]
    fn row_toggles() {
        let s = on(0, 0);
        assert_eq!(toggle_row(1, s).octave, -1);
        assert_eq!(toggle_row(1, toggle_row(1, s)).octave, 0);
        assert_eq!(toggle_row(2, s).octave, 1);
        assert!(toggle_row(3, s).accent);
        assert!(toggle_row(4, s).slide);
        assert_eq!(toggle_row(5, s).gate, Gate::Tie);
    }

    #[test]
    fn writer_enters_notes_and_advances() {
        let p = grid(4);
        let mut w = StepWriter::new(3);
        match w.handle(WriteKey::Note(4), &p) {
            WriteAction::Set { step: 3, value } => assert_eq!(value, on(4, 0)),
            other => panic!("{other:?}"),
        }
        assert_eq!(w.cursor, 0, "wraps at the pattern length");
        match w.handle(WriteKey::Note(14), &p) {
            WriteAction::Set { step: 0, value } => assert_eq!(value, on(2, 1)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn writer_rest_tie_and_marks() {
        let mut p = grid(16);
        p.steps[0] = on(5, 0);
        let mut w = StepWriter::new(0);
        match w.handle(WriteKey::Accent, &p) {
            WriteAction::Set { step: 0, value } => assert!(value.accent && value.note == 5),
            other => panic!("{other:?}"),
        }
        assert_eq!(w.cursor, 0, "marks do not advance");
        assert!(
            matches!(w.handle(WriteKey::Rest, &p), WriteAction::Set { step: 0, value } if value.gate == Gate::Off)
        );
        assert!(
            matches!(w.handle(WriteKey::Tie, &p), WriteAction::Set { step: 1, value } if value.gate == Gate::Tie)
        );
        assert_eq!(w.cursor, 2);
        assert!(
            matches!(w.handle(WriteKey::Back, &p), WriteAction::Set { step: 1, value } if value.gate == Gate::Off)
        );
        assert_eq!(w.cursor, 1);
        assert!(matches!(w.handle(WriteKey::Exit, &p), WriteAction::Exit));
    }

    #[test]
    fn writer_octave_is_bounded() {
        let p = grid(16);
        let mut w = StepWriter::new(0);
        w.handle(WriteKey::OctaveDown, &p);
        w.handle(WriteKey::OctaveDown, &p);
        assert_eq!(w.octave, -1);
        assert!(
            matches!(w.handle(WriteKey::Note(0), &p), WriteAction::Set { value, .. } if value.octave == -1)
        );
        w.handle(WriteKey::OctaveUp, &p);
        w.handle(WriteKey::OctaveUp, &p);
        w.handle(WriteKey::OctaveUp, &p);
        assert_eq!(w.octave, 1);
        assert!(
            matches!(w.handle(WriteKey::Note(12), &p), WriteAction::Set { value, .. } if value.octave == 1)
        );
    }

    #[test]
    fn cursor_is_clamped_after_the_pattern_shrinks() {
        let p = grid(2);
        let mut w = StepWriter::new(10);
        assert!(matches!(
            w.handle(WriteKey::Note(0), &p),
            WriteAction::Set { step: 0, .. }
        ));
    }

    #[test]
    fn tracker_key_layout() {
        use eframe::egui::Key;
        assert_eq!(key_to_write(Key::Z), Some(WriteKey::Note(0)));
        assert_eq!(key_to_write(Key::S), Some(WriteKey::Note(1)));
        assert_eq!(key_to_write(Key::M), Some(WriteKey::Note(11)));
        assert_eq!(key_to_write(Key::Q), Some(WriteKey::Note(12)));
        assert_eq!(key_to_write(Key::I), Some(WriteKey::Note(24)));
        assert_eq!(key_to_write(Key::Space), Some(WriteKey::Rest));
        assert_eq!(key_to_write(Key::A), Some(WriteKey::Accent));
        assert_eq!(key_to_write(Key::F), Some(WriteKey::Slide));
        assert_eq!(key_to_write(Key::OpenBracket), Some(WriteKey::OctaveDown));
        assert_eq!(key_to_write(Key::K), None);
    }
}
