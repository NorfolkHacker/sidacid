//! Transport modes and recording: Pattern/Song switch, Rec arm, play/stop
//! (stop twice returns to the start), and writing touched-knob automation
//! and pad changes into the song as one undoable record pass.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::host::Command;
use acid_engine::machine::MachineUid;
use acid_engine::params::ParamId;
use acid_engine::sequencer::Mode;
use acid_engine::song;

impl SidAcidApp {
    pub(crate) fn recording(&self) -> bool {
        self.view.record && self.view.mode == Mode::Song && self.status.playing
    }

    /// A fresh engine (after loading a document or restarting audio)
    /// starts stopped in Pattern mode: make the transport view match.
    pub(crate) fn reset_transport_view(&mut self) {
        self.doc.end_gesture();
        self.doc.end_pass();
        self.view.mode = Mode::Pattern;
        self.view.record = false;
        self.view.touched_knob = None;
        self.view.engine_touch = None;
        self.view.live.clear();
        self.view.rec_last = None;
        self.view.was_playing = false;
        self.view.stops_sent = 0;
        // Until its first status (none comes if audio is dropped).
        self.status.playing = false;
    }

    pub(crate) fn set_mode(&mut self, mode: Mode) {
        if self.view.mode == mode {
            return;
        }
        self.stop_recording();
        self.view.record = false;
        self.view.mode = mode;
        // A range or selection means ticks in one mode, steps in the other.
        self.view.dock.range = None;
        self.view.dock.range_from = None;
        for panel in self.view.panels.values_mut() {
            panel.selected.clear();
        }
        // A Stop while stopped would cut tails and auditions, and use up
        // the engine's "first stop after a song end only releases".
        if self.status.playing {
            self.send(vec![Command::Stop]);
            // Until a status that has taken the Stop says otherwise (a fast
            // second Stop must see it; see `on_status`).
            self.status.playing = false;
        }
        self.send(vec![Command::SetMode(mode)]);
    }

    pub(crate) fn set_record(&mut self, on: bool) {
        if !on {
            self.stop_recording();
        }
        self.view.record = on && self.view.mode == Mode::Song;
    }

    /// Ends the current record pass: one undo step, live values dropped.
    /// A knob still held stays held, but the engine no longer is told so
    /// (song automation plays it again until recording resumes).
    pub(crate) fn stop_recording(&mut self) {
        self.doc.end_gesture();
        self.doc.end_pass();
        self.release_engine_touch();
        self.view.live.clear();
        self.view.rec_last = None;
    }

    pub(crate) fn play(&mut self) {
        if self.view.mode == Mode::Song {
            self.view.song_cursor = self.view.song_cursor.min(self.last_song_bar_start());
            self.send(vec![Command::SetSongStart(self.view.song_cursor)]);
        }
        self.send(vec![Command::Play]);
    }

    /// The first step of the song's last 16-step bar (0 for an empty
    /// song): the latest step Play starts from, so a cursor left past a
    /// shortened song still plays its end.
    pub(crate) fn last_song_bar_start(&self) -> u32 {
        let lens = song::lens_of(&self.doc.project);
        let end = song::song_end(&self.doc.project.song, &lens);
        end.saturating_sub(1) / 16 * 16
    }

    /// The song has something to play: a block, or a length of its own.
    pub(crate) fn has_song(&self) -> bool {
        let lens = song::lens_of(&self.doc.project);
        song::song_end(&self.doc.project.song, &lens) > 0
    }

    /// Stop; pressed while already stopped, returns to the song start.
    pub(crate) fn stop(&mut self) {
        if !self.status.playing {
            self.view.song_cursor = self.doc.project.song.loop_range.map_or(0, |(a, _)| a);
            self.send(vec![Command::SetSongStart(self.view.song_cursor)]);
        }
        if self.view.record {
            self.stop_recording();
        }
        self.send(vec![Command::Stop]);
        // Until a status that has taken this Stop says otherwise (see
        // `on_status`): a second Stop before then returns to the start,
        // and nothing more is recorded.
        self.status.playing = false;
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
        if song::write_automation(&mut new_song, id, from, step, value)
            && self.view.message.is_none()
        {
            self.view.message = Some(
                "Part of the recorded automation was cut: it ran past the longest song.".into(),
            );
        }
        let cmds = self.doc.record(Edit::Song {
            song: Box::new(new_song),
        });
        self.send(cmds);
        self.view.rec_last = Some((id, step));
    }

    /// While recording, a pad click also writes the pattern into the song.
    pub(crate) fn record_pattern_change(&mut self, uid: MachineUid, slot: u8) {
        if !self.recording() || self.doc.project.machine(uid).is_none() {
            return;
        }
        let mut new_song = self.doc.project.song.clone();
        {
            let lens = song::lens_of(&self.doc.project);
            song::record_pattern(&mut new_song, uid, self.status.song_step, slot, &lens);
        }
        if new_song == self.doc.project.song {
            return;
        }
        let cmds = self.doc.record(Edit::Song {
            song: Box::new(new_song),
        });
        self.send(cmds);
    }

    /// The knob's gesture is over: release its touch, and outside a record
    /// pass close its undo gesture.
    pub(crate) fn release_knob(&mut self, id: ParamId) {
        if self.view.touched_knob != Some(id) {
            return;
        }
        self.view.touched_knob = None;
        self.release_engine_touch();
        // A wheel gesture's step closes once the wheel rests instead.
        if !self.recording() && !self.knob_scroll_open(id) {
            self.doc.end_gesture();
        }
    }

    /// Tells the engine the held knob is let go, once.
    pub(crate) fn release_engine_touch(&mut self) {
        if let Some(id) = self.view.engine_touch.take() {
            self.send(vec![Command::Touch(id, false)]);
        }
    }

    /// Tells the engine `id` is held (while recording), once per hold.
    pub(crate) fn engine_touch(&mut self, id: ParamId) {
        if self.view.engine_touch == Some(id) {
            return;
        }
        self.release_engine_touch();
        self.view.engine_touch = Some(id);
        self.send(vec![Command::Touch(id, true)]);
    }
}
