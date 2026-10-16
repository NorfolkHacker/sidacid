//! Song mode's dock tools: the header's buttons (capture, loop, insert
//! and delete time, and the one selected block's pattern, mute and
//! transpose), the block keys and the block clipboard. Every edit is one
//! `Edit::Song` undo step; a selection change alone is none. Blocks
//! never overlap: a paste or a move onto taken steps goes, as a group, to
//! the nearest offset on the bar grid where it fits, and one that fits
//! nowhere is refused with a message.

use super::song_ops::{self, BAR, GroupLane};
use super::{SidAcidApp, theme};
use acid_engine::machine::MachineUid;
use acid_engine::project::{BANK_SIZE, Block, PATTERNS, PatternRef, Song};
use acid_engine::song::{self, MAX_SONG_STEPS};
use eframe::egui::{self, Key, Modifiers, RichText};

/// Blocks copied from the dock's song lanes, starts relative to the
/// clip's origin: one lane (pastes into the focused lane) or every lane
/// of a ruler range (pastes back into the same machines).
#[derive(Clone, Debug)]
pub(crate) struct BlockClip {
    pub lanes: LaneBlocks,
}

/// Blocks per machine: a clip's lanes, or what a paste put on each lane.
type LaneBlocks = Vec<(MachineUid, Vec<Block>)>;

/// A range tool: its label, hover text and action.
type RangeTool = (&'static str, &'static str, fn(&mut SidAcidApp));

/// Why a paste or duplicate found nowhere to go.
const NO_ROOM: &str = "There is no room on the song for those blocks";

/// The furthest a song transpose goes, in semitones either way.
const MAX_TRANSPOSE: i8 = 12;

impl SidAcidApp {
    /// Makes `song` the song as one undo step (nothing when unchanged).
    fn commit_song(&mut self, song: Song) {
        if song != self.doc.project.song {
            self.apply_song(song, false);
        }
        self.doc.end_gesture();
    }

    /// Clears every lane's block selection.
    fn clear_block_selections(&mut self) {
        for panel in self.view.panels.values_mut() {
            panel.song_sel.clear();
        }
    }

    /// The one selected block, when exactly one is selected over all
    /// lanes: its machine, index and value.
    pub(crate) fn single_block(&self) -> Option<(MachineUid, usize, Block)> {
        let mut found = None;
        for uid in self.doc.project.rack_uids() {
            for i in self.block_selection(uid) {
                if found.is_some() {
                    return None;
                }
                found = Some((uid, i, self.doc.project.song.lane(uid)[i]));
            }
        }
        found
    }

    /// The Song-mode part of the dock header.
    pub(crate) fn song_tools(&mut self, ui: &mut egui::Ui) {
        let has_rack = !self.doc.project.machines.is_empty();
        let range = self.view.dock.range.is_some();
        let r = ui
            .add_enabled(has_rack, egui::Button::new("Capture"))
            .on_hover_text("Place every machine's selected pattern at the song cursor");
        self.tool_rect("Capture", &r);
        if r.clicked() {
            self.capture();
        }
        ui.separator();
        let tools: [RangeTool; 3] = [
            ("Loop", "Loop the picked bars", Self::loop_range),
            (
                "Insert time",
                "Push everything from the picked bars' start right by their length",
                Self::insert_range_time,
            ),
            (
                "Delete time",
                "Remove the picked bars and close the gap",
                Self::delete_range_time,
            ),
        ];
        for (name, hint, act) in tools {
            let r = ui
                .add_enabled(range, egui::Button::new(name))
                .on_hover_text(hint)
                .on_disabled_hover_text("Drag along the ruler to pick bars first");
            self.tool_rect(name, &r);
            if r.clicked() {
                act(self);
            }
        }
        if self.doc.project.song.loop_range.is_some() {
            let r = ui.button("Clear loop");
            self.tool_rect("Clear loop", &r);
            if r.clicked() {
                let mut s = self.doc.project.song.clone();
                s.loop_range = None;
                self.commit_song(s);
            }
        }
        if let Some((uid, _, b)) = self.single_block() {
            ui.separator();
            self.block_tools(ui, uid, b);
        }
    }

    /// The selected block's pattern pickers, mute and transpose.
    fn block_tools(&mut self, ui: &mut egui::Ui, uid: MachineUid, b: Block) {
        let banks = (PATTERNS / BANK_SIZE) as u8;
        let (bank, num) = (b.pattern.0 / BANK_SIZE as u8, b.pattern.0 % BANK_SIZE as u8);
        let mut pick = None;
        let mut rects = Vec::new();
        ui.label(RichText::new("Pattern").small().color(theme::ACID));
        let r = egui::ComboBox::from_id_salt("song_block_bank")
            .width(28.0)
            .selected_text(bank_letter(bank).to_string())
            .show_ui(ui, |ui| {
                for k in 0..banks {
                    let r = ui.selectable_label(k == bank, bank_letter(k).to_string());
                    rects.push((format!("bank {}", bank_letter(k)), r.rect));
                    if r.clicked() {
                        pick = Some(k * BANK_SIZE as u8 + num);
                    }
                }
            });
        self.tool_rect("bank", &r.response);
        let r = egui::ComboBox::from_id_salt("song_block_number")
            .width(28.0)
            .selected_text(format!("{:02}", num + 1))
            .show_ui(ui, |ui| {
                for n in 0..BANK_SIZE as u8 {
                    let r = ui.selectable_label(n == num, format!("{:02}", n + 1));
                    rects.push((format!("number {}", n + 1), r.rect));
                    if r.clicked() {
                        pick = Some(bank * BANK_SIZE as u8 + n);
                    }
                }
            });
        self.tool_rect("number", &r.response);
        #[cfg(test)]
        self.view.dock.tool_rects.extend(rects);
        #[cfg(not(test))]
        drop(rects);
        if let Some(slot) = pick {
            self.set_block_pattern(slot);
        }
        let r = ui
            .selectable_label(b.mute, "Mute")
            .on_hover_text("Silence this block");
        self.tool_rect("Mute", &r);
        if r.clicked() {
            self.edit_single_block(|b| b.mute = !b.mute);
        }
        let transposes = self
            .doc
            .project
            .machine(uid)
            .is_some_and(|m| m.machine_type().transposes());
        if transposes {
            ui.label(RichText::new("Trans").small().color(theme::ACID));
            let r = ui.add_enabled(b.transpose > -MAX_TRANSPOSE, egui::Button::new("−"));
            self.tool_rect("Trans -", &r);
            if r.on_hover_text("Down a semitone").clicked() {
                self.transpose_block(-1);
            }
            ui.label(RichText::new(format!("{:+}", b.transpose)).monospace());
            let r = ui.add_enabled(b.transpose < MAX_TRANSPOSE, egui::Button::new("+"));
            self.tool_rect("Trans +", &r);
            if r.on_hover_text("Up a semitone").clicked() {
                self.transpose_block(1);
            }
        }
    }

    /// Tests only: where a header tool was drawn this frame, by name.
    #[cfg_attr(not(test), allow(unused_variables))]
    fn tool_rect(&mut self, name: &str, r: &egui::Response) {
        #[cfg(test)]
        self.view.dock.tool_rects.push((name.to_string(), r.rect));
    }

    /// Places every machine's selected pattern at the song cursor (each
    /// on the nearest free start of its lane), as one undo step; the
    /// placed blocks become the selection. Refused whole, with a message,
    /// when one fits nowhere.
    pub(crate) fn capture(&mut self) {
        if self.mouse_editing() || self.doc.project.machines.is_empty() {
            return;
        }
        let at = self.view.song_cursor;
        let mut s = self.doc.project.song.clone();
        let placed: Option<Vec<(MachineUid, Block)>> = {
            let p = &self.doc.project;
            let lens = song::lens_of(p);
            p.machines
                .iter()
                .map(|m| {
                    let lane = s.lane_mut(m.uid);
                    let block = Block::new(at, PatternRef(m.selected));
                    song::place(lane, m.uid, block, &lens).map(|i| (m.uid, lane[i]))
                })
                .collect()
        };
        let Some(placed) = placed else {
            self.view.message = Some("There is no room on the song for that pattern".into());
            return;
        };
        self.view.dock.range = None;
        self.clear_block_selections();
        for (uid, b) in placed {
            self.view.panel(uid).song_sel = vec![b];
        }
        self.commit_song(s);
    }

    /// Loops the ruler range.
    pub(crate) fn loop_range(&mut self) {
        let Some((a, b)) = self.view.dock.range else {
            return;
        };
        let mut s = self.doc.project.song.clone();
        s.loop_range = Some((a as u32, b as u32));
        self.commit_song(s);
    }

    /// Inserts the ruler range's length of empty steps at its start on
    /// every lane. The range stays, now over the inserted steps; the
    /// moved blocks drop out of the selection.
    pub(crate) fn insert_range_time(&mut self) {
        let Some((a, b)) = self.view.dock.range else {
            return;
        };
        if self.mouse_editing() {
            return;
        }
        let mut s = self.doc.project.song.clone();
        let room =
            self.with_lens(|lens| (MAX_SONG_STEPS as u32).saturating_sub(song::song_end(&s, lens)));
        self.with_lens(|lens| song::insert_time(&mut s, a as u32, (b - a) as u32, lens));
        if s != self.doc.project.song && room < (b - a) as u32 {
            *self.status_line_mut() =
                Some("Only part of the time fit before the song's limit".into());
        }
        if s == self.doc.project.song {
            self.view.message = Some("There is no room on the song for more time".into());
            return;
        }
        self.clear_block_selections();
        self.commit_song(s);
    }

    /// Removes the ruler range's steps from every lane and closes the
    /// gap; the range and the selections go.
    pub(crate) fn delete_range_time(&mut self) {
        let Some((a, b)) = self.view.dock.range else {
            return;
        };
        if self.mouse_editing() {
            return;
        }
        let mut s = self.doc.project.song.clone();
        self.with_lens(|lens| song::delete_time(&mut s, a as u32, b as u32, lens));
        self.view.dock.range = None;
        self.clear_block_selections();
        self.commit_song(s);
    }

    /// Edits the one selected block with `f` (it stays selected), as one
    /// undo step.
    fn edit_single_block(&mut self, f: impl FnOnce(&mut Block)) {
        let Some((uid, i, b)) = self.single_block() else {
            return;
        };
        if self.mouse_editing() {
            return;
        }
        let mut new = b;
        f(&mut new);
        let mut lane = self.doc.project.song.lane(uid).to_vec();
        lane[i] = new;
        self.view.panel(uid).song_sel = vec![new];
        self.commit_song_lane(uid, lane, false);
    }

    /// Plays pattern `slot` in the one selected block, from the same
    /// start. A block that follows its pattern's length and would now run
    /// into the next block (or past the song's limit) gets a fixed length
    /// that ends there.
    pub(crate) fn set_block_pattern(&mut self, slot: u8) {
        let Some((uid, i, b)) = self.single_block() else {
            return;
        };
        let limit = self
            .doc
            .project
            .song
            .lane(uid)
            .get(i + 1)
            .map_or(MAX_SONG_STEPS as u32, |n| n.start);
        let end = self.with_lens(|lens| {
            song::block_end(
                &Block {
                    pattern: PatternRef(slot),
                    ..b
                },
                uid,
                lens,
            )
        });
        if end > limit && b.len.is_none_or(|l| b.start + l as u32 != limit) {
            *self.status_line_mut() =
                Some("The block was shortened to fit before the next one".into());
        }
        self.edit_single_block(|b| {
            b.pattern = PatternRef(slot);
            if end > limit {
                b.len = Some((limit - b.start).min(u16::MAX as u32) as u16);
            }
        });
    }

    /// Moves the one selected block's song transpose by `d` semitones,
    /// held to ±12.
    pub(crate) fn transpose_block(&mut self, d: i8) {
        self.edit_single_block(|b| {
            b.transpose = (b.transpose + d).clamp(-MAX_TRANSPOSE, MAX_TRANSPOSE);
        });
    }

    /// A key on `uid`'s song lane with the dock focused: ←/→ move its
    /// block selection a bar (Alt: a step), Delete/Backspace remove it,
    /// Esc clears the ruler range and every selection (or the lane's
    /// selection). True when the key was used.
    pub(crate) fn song_key(&mut self, uid: MachineUid, key: Key, mods: Modifiers) -> bool {
        if mods.command || mods.ctrl {
            return false;
        }
        let sel = self.block_selection(uid);
        if key == Key::Escape {
            if self.view.dock.range.take().is_some() {
                self.clear_block_selections();
                return true;
            }
            if sel.is_empty() {
                return false;
            }
            self.view.panel(uid).song_sel.clear();
            return true;
        }
        if sel.is_empty() {
            return false;
        }
        let unit = if mods.alt { 1 } else { BAR };
        let mut lane = self.doc.project.song.lane(uid).to_vec();
        match key {
            Key::ArrowLeft | Key::ArrowRight => {
                let dt = if key == Key::ArrowLeft {
                    -(unit as i64)
                } else {
                    unit as i64
                };
                let new =
                    self.with_lens(|lens| song_ops::shift(&mut lane, uid, &sel, dt, unit, lens));
                let Some(new) = new else {
                    return true;
                };
                if lane == self.doc.project.song.lane(uid) {
                    // Held where it is: no edit, and the range still holds.
                    return true;
                }
                self.set_block_selection(uid, &lane, &new);
            }
            Key::Delete | Key::Backspace => {
                song_ops::delete(&mut lane, &sel);
                self.view.panel(uid).song_sel.clear();
            }
            _ => return false,
        }
        // The range no longer describes what is selected.
        self.drop_range();
        self.commit_song_lane(uid, lane, false);
        true
    }

    /// Copies to the block clipboard: with a ruler range, every lane's
    /// blocks starting in it (relative to its start); else the focused
    /// lane's selection (relative to its earliest start). With `cut` the
    /// copied blocks are then removed, in one undo step. Nothing to copy
    /// leaves the clipboard as it was.
    pub(crate) fn copy_blocks(&mut self, cut: bool) {
        if self.mouse_editing() {
            return;
        }
        let mut s = self.doc.project.song.clone();
        let mut lanes = Vec::new();
        let mut take = |uid: MachineUid, idx: &[usize], origin: u32| {
            let lane = s.lane(uid).to_vec();
            lanes.push((uid, song_ops::copy(&lane, idx, origin)));
            if cut && !idx.is_empty() {
                song_ops::delete(s.lane_mut(uid), idx);
            }
        };
        let range = self.view.dock.range;
        if let Some((lo, hi)) = range {
            for uid in self.doc.project.rack_uids() {
                let idx = song_ops::in_range(self.doc.project.song.lane(uid), lo as u32, hi as u32);
                take(uid, &idx, lo as u32);
            }
        } else {
            let Some(uid) = self.focused() else {
                return;
            };
            let idx = self.block_selection(uid);
            let lane = self.doc.project.song.lane(uid);
            let Some(origin) = idx.iter().map(|&i| lane[i].start).min() else {
                return;
            };
            take(uid, &idx, origin);
        }
        if lanes.iter().all(|(_, blocks)| blocks.is_empty()) {
            return;
        }
        self.view.block_clip = Some(BlockClip { lanes });
        if cut {
            if range.is_some() {
                self.view.dock.range = None;
                self.clear_block_selections();
            } else if let Some(uid) = self.focused() {
                self.view.panel(uid).song_sel.clear();
            }
            self.commit_song(s);
        }
    }

    /// Puts each `(uid, clip)` onto its lane of `s`, all by the one offset
    /// nearest `want` on the bar grid at which every clip fits. Returns
    /// that offset and the pasted blocks of each lane; `None`, `s`
    /// untouched, with a message, when there is no such offset.
    fn paste_group(
        &mut self,
        s: &mut Song,
        targets: &LaneBlocks,
        want: u32,
    ) -> Option<(i64, LaneBlocks)> {
        let d = {
            let groups: Vec<GroupLane<'_>> = targets
                .iter()
                .map(|(uid, clip)| GroupLane {
                    rest: s.lane(*uid),
                    uid: *uid,
                    moved: clip,
                })
                .collect();
            self.with_lens(|lens| song_ops::group_offset(&groups, want as i64, BAR, lens))
        };
        let Some(d) = d else {
            self.view.message = Some(NO_ROOM.into());
            return None;
        };
        let mut pasted = Vec::new();
        for (uid, clip) in targets {
            let (lane, idx) = song_ops::with_moved(s.lane(*uid), clip, d);
            pasted.push((*uid, song_ops::values(&lane, &idx)));
            *s.lane_mut(*uid) = lane;
        }
        Some((d, pasted))
    }

    /// Pastes the block clipboard at the song cursor: a one-lane clip into
    /// the focused lane, a range clip back into the machines it came from
    /// (those no longer in the rack are skipped). All lanes go by one
    /// offset, the nearest bar-grid one where everything fits. One undo
    /// step; a paste that fits nowhere is refused with a message.
    pub(crate) fn paste_blocks(&mut self) {
        if self.mouse_editing() {
            return;
        }
        let (Some(clip), Some(focus)) = (self.view.block_clip.clone(), self.focused()) else {
            return;
        };
        let targets: LaneBlocks = match &clip.lanes[..] {
            [(_, blocks)] => vec![(focus, blocks.clone())],
            lanes => lanes
                .iter()
                .filter(|(uid, blocks)| {
                    !blocks.is_empty() && self.doc.project.machine(*uid).is_some()
                })
                .cloned()
                .collect(),
        };
        if targets.is_empty() {
            return;
        }
        let mut s = self.doc.project.song.clone();
        let at = self.view.song_cursor;
        let Some((_, pasted)) = self.paste_group(&mut s, &targets, at) else {
            return;
        };
        if clip.lanes.len() == 1 {
            self.drop_range();
        } else {
            self.view.dock.range = None;
            self.clear_block_selections();
        }
        self.select_pasted(pasted);
        self.commit_song(s);
    }

    /// Makes each lane's pasted blocks its selection.
    fn select_pasted(&mut self, pasted: LaneBlocks) {
        for (uid, sel) in pasted {
            self.view.panel(uid).song_sel = sel;
        }
    }

    /// Pastes a copy of the focused lane's selection right after it (its
    /// span rounded up to whole bars). With a ruler range, every lane's
    /// blocks in the range are repeated one range length later instead,
    /// and the range moves onto the copies. One undo step.
    pub(crate) fn duplicate_blocks(&mut self) {
        if self.mouse_editing() {
            return;
        }
        let mut s = self.doc.project.song.clone();
        if let Some((lo, hi)) = self.view.dock.range {
            let targets: LaneBlocks = self
                .doc
                .project
                .rack_uids()
                .into_iter()
                .filter_map(|uid| {
                    let lane = s.lane(uid);
                    let idx = song_ops::in_range(lane, lo as u32, hi as u32);
                    (!idx.is_empty()).then(|| (uid, song_ops::copy(lane, &idx, lo as u32)))
                })
                .collect();
            if targets.is_empty() {
                return;
            }
            let Some((d, pasted)) = self.paste_group(&mut s, &targets, hi as u32) else {
                return;
            };
            self.clear_block_selections();
            self.select_pasted(pasted);
            let len = (hi - lo) as i64;
            // The copies may sit before the range's start; the new range
            // stays on the song.
            let start = d.clamp(0, MAX_SONG_STEPS as i64 - len);
            self.view.dock.range = Some((start as u16, (start + len) as u16));
        } else {
            let Some(uid) = self.focused() else {
                return;
            };
            let idx = self.block_selection(uid);
            if idx.is_empty() {
                return;
            }
            let lane = s.lane_mut(uid);
            let lens = song::lens_of(&self.doc.project);
            let Some(new) = song_ops::duplicate(lane, uid, &idx, &lens) else {
                self.view.message = Some(NO_ROOM.into());
                return;
            };
            let sel = song_ops::values(lane, &new);
            self.view.panel(uid).song_sel = sel;
        }
        self.commit_song(s);
    }

    /// Selects every block of the focused song lane (and drops the ruler
    /// range).
    pub(crate) fn select_all_blocks(&mut self) {
        if self.mouse_editing() {
            return;
        }
        let Some(uid) = self.focused() else {
            return;
        };
        let lane = self.doc.project.song.lane(uid).to_vec();
        self.drop_range();
        self.view.panel(uid).song_sel = lane;
    }
}

/// A pattern bank's letter: 0 is A.
fn bank_letter(bank: u8) -> char {
    (b'A' + bank) as char
}
