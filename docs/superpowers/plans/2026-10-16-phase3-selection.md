# Phase 3: Selection Editing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The dock's lanes get real selection editing: click and box-select notes, move, copy-drag, resize and delete the selection, nudge and transpose it from the keyboard, copy/cut/paste/duplicate it (also across all lanes through a time range picked on the ruler), all as single undo steps.

**Architecture:** A new pure module `ui/note_ops.rs` holds every selection operation on a `NotePattern` (select by box or range, move/transpose as a group, resize, delete, copy, paste, duplicate, and "settle" which applies the acid one-note rule or the same-pitch merge after an edit). A selection is stored **by note value** (`Vec<Note>` per lane), so it survives index changes and undo without bookkeeping and is resolved to indices when used. `lane.rs`'s mouse handling is reworked around the selection (drags recompute from a snapshot each frame, as phase 1's roll did). A ruler drag sets a dock-wide time range that selects across lanes. Keyboard operations go through `dock.rs` while the dock has keyboard focus; Ctrl+C/X/V/D/A reach the dock instead of the pattern copy/paste when it does.

**Tech Stack:** Rust 2024, egui 0.31.

**Spec:** `docs/superpowers/specs/2026-10-16-dock-song-lanes-design.md` §3. Phases 1–2 are merged: every pattern is a `NotePattern` reachable through `Project::roll(uid, slot)`; the dock (`ui/dock.rs`), lanes (`ui/lane.rs`), geometry (`ui/lane_geom.rs`), keyboard entry (`ui/key_entry.rs`) and edits through `commit_poly(uid, slot, Some(pattern), dragging, finished)` exist.

## Global Constraints

- User-facing text never names a commercial product or manufacturer.
- Every operation is one undo step: single-lane ops commit one `Edit::NotePattern`; multi-lane ops commit one `Edit::Group` of `Edit::NotePattern`s.
- Acid (bass synth) lanes stay one note at a time: the note just edited wins (`poly_edit::make_mono` semantics).
- Notes moved past the pattern length are kept (not played), as everywhere else.
- `MAX_NOTES` (512) per pattern holds: a paste or duplicate that would exceed it is refused with a message (`view.message`), changing nothing.
- Melodic↔melodic paste works; melodic↔drum paste is refused with a message.
- Space always plays/stops; Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y / Ctrl+S / Ctrl+O / Ctrl+N keep working everywhere. Ctrl+C / Ctrl+V keep copying/pasting whole patterns when the dock does **not** have keyboard focus.
- No engine changes; golden tests untouched.
- Work on branch `phase3-selection`; commit with `git -c user.name=NorfolkHacker -c user.email=17689595+NorfolkHacker@users.noreply.github.com commit …`, messages ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; `cargo fmt --all` before each commit; app package `sidacid`. Each task ends with `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Decisions taken in this plan

| Question | Decision |
|---|---|
| How a selection is stored | `PanelState::selected: Vec<Note>` (by value) plus `PanelState::sel_slot: u8`; resolved to indices with `note_ops::resolve` (each selected value matches one not-yet-used index). A selection whose slot differs from the selected pattern is treated as empty |
| Click on a note | Selects it (Shift-click toggles it); no longer does nothing |
| Click on empty space | Adds a note one snap long and makes it the selection (Shift-click on empty space only clears nothing and adds nothing — it is ignored) |
| Drag on empty space | Box (marquee) select; Shift adds to the selection. The old "drag on empty space draws a note of the dragged length" goes away |
| Drag on a note body | Moves the selection (the grabbed note joins/replaces the selection first); Ctrl at drag start moves copies; Alt while dragging moves by single ticks |
| Drag a note's right edge | Resizes every selected note by the same tick delta (min length 1) |
| Right-click | On a selected note: deletes the selection; on another note: deletes that note |
| Group move limits | The whole group's delta is clamped so no note starts before tick 0 or at/after `STEPS × 4`, and no pitch leaves 0..=127 (drums 0..=7) |
| After a move/resize drop or paste | `note_ops::settle`: acid lanes run `make_mono` for each touched note in ascending start order (each touched note keeps itself and cuts or removes what it overlaps); other lanes merge same-pitch overlaps into the touched note (as `resolve_overlaps` did for one note) |
| Ruler | A drag on the ruler above the thumb strip selects a time range (snapped to the dock snap); it replaces every lane's selection with the notes starting in the range, and is drawn as a band over the ruler and lanes. A click on the ruler clears it |
| Clipboard | `ViewState::note_clip: Option<NoteClip>`; `NoteClip { lanes: Vec<(MachineUid, LaneKind, Vec<Note>)> }` with starts relative to the clip's origin (the earliest selected start, or the range start) |
| Copy with a ruler range | Copies every lane's notes in the range (multi-lane clip); without a range, copies the focused lane's selection (or the whole lane when nothing is selected) |
| Paste target | At the focused lane's cursor tick. A single-lane clip pastes into the focused lane; a multi-lane clip pastes back into the same machines (by uid; machines no longer in the rack are skipped). Pasted notes become the selection |
| Duplicate (Ctrl+D) | Pastes a copy of the selection immediately after it: offset = selection span (max end − min start) rounded up to the dock snap |
| Keys and the selection | With a selection in the focused lane: ←/→ move it one snap, Alt+←/→ one tick, ↑/↓ one semitone (drum row), Shift+↑/↓ an octave (melodic only), Delete/Backspace delete it, Esc clears it. Without a selection those keys keep their phase-2 keyboard-entry meaning |
| Selection after undo/redo | Kept by value: still-present notes stay selected; vanished ones drop out |

---

### Task 1: Pure selection operations

**Files:**
- Create: `crates/app/src/ui/note_ops.rs`
- Modify: `crates/app/src/ui/mod.rs` (`mod note_ops;` with a narrow `#[allow(dead_code)] // used by the lanes (phase 3, tasks 2–4)`)

**Interfaces** (all `pub`, operating on `acid_engine::project::{Note, NotePattern}`, with `key_entry::LaneKind` for kind-specific rules):
- `pub fn resolve(p: &NotePattern, selected: &[Note]) -> Vec<usize>` — sorted unique indices; each selected value claims the first unclaimed equal note.
- `pub fn values(p: &NotePattern, idx: &[usize]) -> Vec<Note>`.
- `pub fn in_box(p: &NotePattern, t0: u16, t1: u16, lo: u8, hi: u8) -> Vec<usize>` — notes with `start < ticks()` overlapping ticks `[min(t0,t1), max(t0,t1)]` (inclusive of the cells) and pitches `min..=max` of `lo, hi`.
- `pub fn in_range(p: &NotePattern, t0: u16, t1: u16) -> Vec<usize>` — notes starting in `[t0, t1)`.
- `pub fn shift(p: &mut NotePattern, idx: &[usize], dt: i32, dp: i32, kind: LaneKind) -> Vec<usize>` — moves the group by the clamped delta (see decisions; pitch range 0..=7 on drums), returns the same indices (positions don't change). Returns them unchanged if the clamped delta is (0, 0).
- `pub fn resize(p: &mut NotePattern, idx: &[usize], dlen: i32)` — each `len = (len + dlen).max(1)`, capped so `start + len ≤ u16::MAX`.
- `pub fn delete(p: &mut NotePattern, idx: &[usize])`.
- `pub fn copy(p: &NotePattern, idx: &[usize], origin: u16) -> Vec<Note>` — values with `start - origin` (origin ≤ every start; callers pass the min start or a range start).
- `pub enum PasteError { TooMany, WrongKind }` and `pub fn paste(p: &mut NotePattern, notes: &[Note], at: u16, kind: LaneKind) -> Result<Vec<usize>, PasteError>` — appends each note with `start + at` (notes whose start would be ≥ `STEPS × 4` are dropped), refuses `TooMany` if the result would exceed `MAX_NOTES`, refuses `WrongKind` if `kind == Drums` and any pitch ≥ 8 (the caller checks melodic-into-drum / drum-into-melodic by clip kind first; this is the safety net), then `settle`s the new notes and returns their indices.
- `pub fn duplicate(p: &mut NotePattern, idx: &[usize], snap_ticks: u16, kind: LaneKind) -> Result<Vec<usize>, PasteError>` — span = `max end − min start` rounded up to a multiple of `snap_ticks`; pastes `copy(.., min start)` at `min start + span`.
- `pub fn settle(p: &mut NotePattern, touched: &[usize], kind: LaneKind) -> Vec<usize>` — Acid: in ascending start order, `make_mono` on each touched note (tracking index changes); Poly/Drums: merge each touched note with same-pitch notes it overlaps (union span, the touched note keeps its other fields), touched notes absorbing untouched ones; returns the touched notes' new indices.

- [ ] **Step 1: Create the branch** — `git switch -c phase3-selection`.

- [ ] **Step 2: Write the failing tests** (in `note_ops.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::key_entry::LaneKind;
    use acid_engine::project::{MAX_NOTES, Note, NotePattern};

    fn pat(notes: &[(u16, u16, u8)]) -> NotePattern {
        NotePattern {
            notes: notes.iter().map(|&(s, l, p)| Note::new(s, l, p, 100)).collect(),
            length: 16,
        }
    }

    fn spans(p: &NotePattern) -> Vec<(u16, u16, u8)> {
        let mut v: Vec<_> = p.notes.iter().map(|n| (n.start, n.len, n.pitch)).collect();
        v.sort();
        v
    }

    #[test]
    fn selections_resolve_by_value() {
        let p = pat(&[(0, 4, 60), (4, 4, 62), (0, 4, 60)]);
        let sel = vec![Note::new(0, 4, 60, 100), Note::new(4, 4, 62, 100)];
        assert_eq!(resolve(&p, &sel), vec![0, 1]);
        let twice = vec![Note::new(0, 4, 60, 100); 2];
        assert_eq!(resolve(&p, &twice), vec![0, 2], "duplicates claim one each");
        assert_eq!(resolve(&p, &[Note::new(9, 1, 1, 1)]), Vec::<usize>::new());
    }

    #[test]
    fn box_and_range_selection() {
        let p = pat(&[(0, 4, 60), (8, 4, 64), (8, 4, 50), (70, 4, 60)]);
        assert_eq!(in_box(&p, 9, 2, 66, 58), vec![0, 1], "corners in any order");
        assert_eq!(in_range(&p, 4, 12), vec![1, 2]);
        assert_eq!(in_box(&p, 0, 63, 0, 127).len(), 3, "a note past the end isn't boxed");
    }

    #[test]
    fn a_group_moves_as_far_as_its_edges_allow() {
        let mut p = pat(&[(4, 4, 60), (12, 4, 126)]);
        shift(&mut p, &[0, 1], -8, 5, LaneKind::Poly);
        assert_eq!(spans(&p), vec![(0, 4, 61), (8, 4, 127)], "clamped to tick 0 and pitch 127");
        let mut d = pat(&[(0, 4, 6)]);
        shift(&mut d, &[0], 0, 3, LaneKind::Drums);
        assert_eq!(d.notes[0].pitch, 7, "drum rows stop at the open hat");
    }

    #[test]
    fn resize_delete_copy() {
        let mut p = pat(&[(0, 4, 60), (8, 2, 62), (20, 4, 64)]);
        resize(&mut p, &[0, 1], -3);
        assert_eq!(spans(&p)[..2], [(0, 1, 60), (8, 1, 62)]);
        let c = copy(&p, &[1, 2], 8);
        assert_eq!(c.iter().map(|n| (n.start, n.pitch)).collect::<Vec<_>>(), [(0, 62), (12, 64)]);
        delete(&mut p, &[0, 2]);
        assert_eq!(spans(&p), vec![(8, 1, 62)]);
    }

    #[test]
    fn paste_places_at_the_cursor_and_respects_the_cap() {
        let mut p = pat(&[]);
        let clip = vec![Note::new(0, 4, 60, 100), Note::new(4, 4, 62, 100)];
        let idx = paste(&mut p, &clip, 16, LaneKind::Poly).unwrap();
        assert_eq!(values(&p, &idx).iter().map(|n| n.start).collect::<Vec<_>>(), [16, 20]);
        let mut full = pat(&[]);
        full.notes = vec![Note::new(0, 1, 1, 1); MAX_NOTES - 1];
        assert_eq!(paste(&mut full, &clip, 0, LaneKind::Poly), Err(PasteError::TooMany));
        assert_eq!(full.notes.len(), MAX_NOTES - 1, "nothing changed");
        let mut kit = pat(&[]);
        assert_eq!(paste(&mut kit, &clip, 0, LaneKind::Drums), Err(PasteError::WrongKind));
    }

    #[test]
    fn duplicate_goes_right_after_the_selection() {
        let mut p = pat(&[(0, 4, 60), (4, 3, 62)]);
        let idx = duplicate(&mut p, &[0, 1], 4, LaneKind::Poly).unwrap();
        // span 7 rounds up to 8
        assert_eq!(values(&p, &idx).iter().map(|n| n.start).collect::<Vec<_>>(), [8, 12]);
    }

    #[test]
    fn settling_an_acid_edit_keeps_the_touched_notes() {
        let mut p = pat(&[(0, 16, 40), (8, 4, 48)]);
        let idx = settle(&mut p, &[1], LaneKind::Acid);
        assert_eq!(spans(&p), vec![(0, 8, 40), (8, 4, 48)]);
        assert_eq!(p.notes[idx[0]].pitch, 48);
    }

    #[test]
    fn settling_a_poly_edit_merges_same_pitch_overlaps() {
        let mut p = pat(&[(0, 4, 60), (2, 4, 60), (2, 4, 64)]);
        let idx = settle(&mut p, &[1], LaneKind::Poly);
        assert_eq!(spans(&p), vec![(0, 6, 60), (2, 4, 64)]);
        assert_eq!(p.notes[idx[0]].start, 0);
    }
}
```

- [ ] **Step 3: Run to verify they fail** — `cargo test -p sidacid note_ops` → compile errors.

- [ ] **Step 4: Implement** per the interfaces; reuse `poly_edit::make_mono`. Keep each function small and documented.

- [ ] **Step 5: Run** — `cargo test -p sidacid note_ops && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 6: Commit** — `feat(app): pure selection operations for the dock lanes`.

---

### Task 2: Selection state, drawing, and the mouse

**Files:** Modify `crates/app/src/ui/mod.rs` (`PanelState::selected: Vec<Note>`, `PanelState::sel_slot: u8`), `crates/app/src/ui/poly_edit.rs` (replace `PolyDrag` with `LaneDrag`), `crates/app/src/ui/lane.rs`.

**Interfaces:**
- `pub enum LaneDrag { Move { origin: Box<NotePattern>, idx: Vec<usize>, grab: (u16, u8) }, Resize { origin: Box<NotePattern>, idx: Vec<usize>, grab: u16 }, Marquee { from: (u16, u8), add: bool }, Velocity }` in `poly_edit.rs` (`PanelState::poly_drag` becomes `Option<LaneDrag>`; rename the field to `drag`).
- `pub(crate) fn selection(&self, uid) -> Vec<usize>` on `SidAcidApp` (resolves `selected` against the selected pattern, empty when `sel_slot` differs) and `pub(crate) fn set_selection(&mut self, uid, notes: Vec<Note>)` (stores values and the current slot).

**Behaviour** (replaces the grid's click/drag handling in `lane_body`; strips, wheel, focus and audition stay):
- Selected notes draw with a 1.5 pt `theme::ACID_LIGHT` outline (drum hits too); unselected notes as now.
- Press/click/drag rules exactly as in the decisions table. Implementation notes:
  - On drag start over a note body: if the note isn't selected, the selection becomes just that note (with Shift: it's added). If Ctrl is held, append copies of the selected notes to the pattern and make the copies the dragged set (originals stay). Store `LaneDrag::Move { origin: pattern snapshot after any copies, idx, grab: (tick, pitch) }`.
  - Each drag frame: `pattern = *origin.clone()`; `note_ops::shift(&mut pattern, &idx, dt, dp, kind)` where `dt` = pointer tick − grab tick snapped to the dock snap (Alt held: unsnapped), `dp` = pointer row pitch − grab pitch (drum rows likewise). Commit with `dragging = true`.
  - Drag start near a selected-or-grabbed note's right edge → `LaneDrag::Resize`; each frame `pattern = origin; note_ops::resize(&mut pattern, &idx, dlen)` with `dlen` = snapped pointer tick + snap − grab-note end.
  - On drop (move or resize): `note_ops::settle(&mut pattern, &idx, kind)`; the selection becomes the settled notes' values; the drop frame merges into the drag's undo step (keep `commit_poly_frame`'s "dropped frame counts as dragging" rule).
  - Drag on empty space → `LaneDrag::Marquee`; draw the box; on release the selection becomes `in_box(..)` (plus the old selection with Shift). No edit, no undo step.
  - Click on empty space → `poly_edit::add` one snap long, `settle` it, select it. Click on a note → select it / Shift toggles. Right-click per the table. Drum double-click accent stays.
- Every gesture is one undo step (or none for pure selection changes).
- `release_stale_poly_input` keeps clearing the drag.

**Tests** (frame tests with injected pointer events, like `clicking_a_scrolled_drum_lane_adds_one_hit_under_the_pointer`, on an FM lane unless stated):
- `clicking_a_note_selects_it_and_shift_click_toggles`
- `dragging_on_empty_space_box_selects`
- `dragging_the_selection_moves_every_selected_note_in_one_undo_step`
- `ctrl_dragging_leaves_the_originals_and_moves_copies`
- `resizing_one_selected_note_resizes_them_all`
- `right_clicking_a_selected_note_deletes_the_selection`
- `moving_onto_an_acid_note_keeps_the_moved_note` (acid lane; the unmoved note is cut)
- the existing drum/strip/audition/undo tests keep passing (port `PolyDrag` names).

Commit: `feat(app): select, box-select, move, copy-drag, resize and delete notes in the lanes`.

---

### Task 3: Ruler time range across lanes

**Files:** Modify `crates/app/src/ui/dock.rs`, `crates/app/src/ui/mod.rs` (`DockState::range: Option<(u16, u16)>`), `crates/app/src/ui/lane.rs` (draw the band).

**Behaviour:** A drag on the ruler above the thumb strip sets `dock.range` from the press tick to the current tick (both snapped to the dock snap, ordered, end exclusive, at least one snap wide) and, on every drag frame, replaces each lane's selection with `in_range` of its selected pattern. A click (press and release without moving) on the ruler clears `range` (selections stay). Any click in a lane grid clears `range` too. The range draws as a translucent `theme::ACID` band on the ruler and behind each lane's notes. The thumb strip keeps scrolling.

**Tests:** `a_ruler_drag_selects_the_range_in_every_lane`; `a_ruler_click_clears_the_range`; `the_thumb_strip_still_scrolls`.

Commit: `feat(app): pick a time range on the ruler to select across every lane`.

---

### Task 4: Keyboard and clipboard

**Files:** Modify `crates/app/src/ui/keys.rs` (new `Shortcut::{Cut, Duplicate, SelectAll}`; route clipboard shortcuts), `crates/app/src/ui/dock.rs` (`dock_key` handles selection keys first; clipboard methods), `crates/app/src/ui/mod.rs` (`ViewState::note_clip: Option<NoteClip>`, `NoteClip`).

**Interfaces:**
- `pub(crate) struct NoteClip { pub lanes: Vec<(MachineUid, LaneKind, Vec<Note>)> }`.
- `pub(crate) fn copy_notes(&mut self, cut: bool)`, `pub(crate) fn paste_notes(&mut self)`, `pub(crate) fn duplicate_notes(&mut self)`, `pub(crate) fn select_all_notes(&mut self)` on `SidAcidApp` in `dock.rs`.

**Behaviour:**
- `keys::shortcut`: Ctrl+X → `Cut`, Ctrl+D → `Duplicate`, Ctrl+A → `SelectAll`; `egui::Event::Cut` → `Cut`. In `handle_shortcuts`: when `view.dock.keyboard` is set, `Copy`/`Cut`/`Paste`/`Duplicate`/`SelectAll` call the note methods; otherwise `Copy`/`Paste` keep calling `copy_pattern`/`paste_pattern` and `Cut`/`Duplicate`/`SelectAll` do nothing. The "backends may send both a key and a clipboard event" de-duplication extends to `Cut`.
- `dock_key` (before key entry), for the focused machine's lane when it has a selection: ←/→ `shift` by ±snap (Alt: ±1), ↑/↓ by ±1 pitch/row, Shift+↑/↓ by ±12 (melodic only; ignored on drums), Delete/Backspace `delete`, Esc clears the selection (and `dock.range`). Moves and deletes commit one undo step and keep the moved notes selected. `entry_key` currently rejects Alt; check Alt+arrows **before** calling `entry_key`. Without a selection, keys fall through to keyboard entry as in phase 2. Esc with nothing selected does nothing.
- Copy: with `dock.range`, every lane's notes starting in the range, origin = range start (multi-lane clip); else the focused lane's selection (whole lane when empty), origin = its min start. Cut = copy then delete (one `Edit::Group` when multi-lane). Paste: at the focused lane's cursor tick; single-lane clip into the focused lane (refuse with `view.message = "Drum notes can't be pasted onto a melodic part"` / `"Melodic notes can't be pasted onto drums"` when kinds differ — Acid and Poly are both melodic); multi-lane clip into the same uids (skip missing machines), one `Edit::Group`; `TooMany` → `view.message = "That paste would put more than 512 notes in the pattern"`. Pasted notes become each lane's selection. Duplicate: per the decision, focused lane only (or every lane with a range, shifting each lane's copy by the range length).
- Ctrl+A selects every note of the focused lane's pattern (clears `range`).

**Tests** (direct calls plus at least two frame-level key tests like phase 2's): arrows/Alt/Shift move and transpose a selection with one undo each; Delete removes it; Esc clears; Ctrl+C then Ctrl+V pastes at the cursor; cut is one undo; melodic→drum refused with the message and no change; multi-lane copy/paste through a range lands in the same machines in one undo; paste over the cap refused; Ctrl+D duplicates after the selection; without dock focus Ctrl+C still copies the whole pattern (the existing pattern-clipboard tests keep passing).

Commit: `feat(app): copy, cut, paste, duplicate, nudge and transpose the selection`.

---

### Task 5: Docs and a look

- README "Editor dock" bullets gain: click/Shift-click/box select; drag to move (Ctrl-drag copies, Alt for fine moves); drag an edge to resize the selection; right-click deletes; ruler drag selects a time range across every lane; arrows nudge (Alt: one 1/64), ↑/↓ transpose (Shift: octave); Ctrl+C/X/V/D/A on notes while the dock has focus (Ctrl+C/V copy whole patterns otherwise); Delete; Esc. No product names.
- Run the app briefly (`cargo run --release -p sidacid`, no synthetic input), screenshot cropped to the SidAcid window into the git-ignored workspace, delete any full-desktop capture, close the app.
- Final gate: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
- Commit: `docs: selection editing in the dock`.
