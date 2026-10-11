//! Transport modes and recording: Pattern/Song switch, Rec arm, play/stop
//! (stop twice returns to the start), and writing touched-knob automation
//! and pad changes into the song as one undoable record pass.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::params::ParamId;
use acid_engine::sequencer::Mode;
use acid_engine::song;

impl SidAcidApp {
    pub(crate) fn recording(&self) -> bool {
        self.view.record && self.view.mode == Mode::Song && self.status.playing
    }

    pub(crate) fn set_mode(&mut self, mode: Mode) {
        if self.view.mode == mode {
            return;
        }
        self.stop_recording();
        self.view.record = false;
        self.view.mode = mode;
        self.send(vec![Command::Stop, Command::SetMode(mode)]);
    }

    pub(crate) fn set_record(&mut self, on: bool) {
        if !on {
            self.stop_recording();
        }
        self.view.record = on && self.view.mode == Mode::Song;
    }

    /// Ends the current record pass: one undo step, live values dropped.
    pub(crate) fn stop_recording(&mut self) {
        self.doc.end_gesture();
        if let Some(id) = self.view.touched_knob {
            self.send(vec![Command::Touch(id, false)]);
        }
        self.view.live.clear();
        self.view.rec_last = None;
    }

    pub(crate) fn play(&mut self) {
        if self.view.mode == Mode::Song {
            self.send(vec![Command::SetSongStart(self.view.song_cursor as u16)]);
        }
        self.send(vec![Command::Play]);
    }

    /// Stop; pressed while already stopped, returns to the song start.
    pub(crate) fn stop(&mut self) {
        if !self.status.playing {
            self.view.song_cursor = self
                .doc
                .project
                .song
                .loop_range
                .map_or(0, |(a, _)| a as usize);
            self.send(vec![Command::SetSongStart(self.view.song_cursor as u16)]);
        }
        if self.view.record {
            self.stop_recording();
        }
        self.send(vec![Command::Stop]);
    }

    /// Writes the held knob's live value into the song up to the current step.
    pub(crate) fn record_automation(&mut self) {
        if !self.recording() {
            self.view.rec_last = None;
            return;
        }
        let Some(id) = self.view.touched_knob else {
            self.view.rec_last = None;
            return;
        };
        let Some(&value) = self.view.live.get(&id) else {
            return;
        };
        let step = self.status.song_step;
        if song::lane(&self.doc.project.song, id).and_then(|l| l.points.get(&step)) == Some(&value)
        {
            return;
        }
        let from = match self.view.rec_last {
            Some((p, s)) if p == id && s < step => s + 1,
            _ => step,
        };
        let mut new_song = self.doc.project.song.clone();
        song::write_automation(&mut new_song, id, from, step, value);
        let cmds = self.doc.apply(
            Edit::Song {
                song: Box::new(new_song),
            },
            true,
        );
        self.send(cmds);
        self.view.rec_last = Some((id, step));
    }

    /// While recording, a pad click also writes the pattern into the song.
    pub(crate) fn record_pattern_change(&mut self, machine: usize, slot: u8) {
        if !self.recording() {
            return;
        }
        let bar = self.status.song_bar as usize;
        if bar >= self.doc.project.song.bars.len() {
            return;
        }
        let mut new_song = self.doc.project.song.clone();
        song::write_pattern_ref(&mut new_song, bar, machine, slot);
        let cmds = self.doc.apply(
            Edit::Song {
                song: Box::new(new_song),
            },
            true,
        );
        self.send(cmds);
    }

    /// The knob's gesture is over: release its touch, and outside a record
    /// pass close its undo gesture.
    pub(crate) fn release_knob(&mut self, id: ParamId) {
        if self.view.touched_knob != Some(id) {
            return;
        }
        self.view.touched_knob = None;
        self.send(vec![Command::Touch(id, false)]);
        if !self.recording() {
            self.doc.end_gesture();
        }
    }
}
