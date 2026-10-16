# SidAcid: Editor Dock, Song Lanes and Note Tools — Design

Date: 2026-10-16
Status: approved in brainstorming, pending spec review

## Goal

Move every part's pattern editor into one dock at the bottom of the
window, where all parts are visible together as stacked piano-roll lanes.
Replace the bar-based song with per-part lanes of pattern blocks that can
never collide. Give the piano roll real selection editing, 1/64
resolution for every part, keyboard entry, scale tools, per-note chance,
ratchet and trig conditions, and a menu of note tools.

User-facing text (UI, README, docs) never names a commercial product or
manufacturer.

## Decisions

| Question | Decision |
|---|---|
| Dock layout | Stacked lanes, one per machine, all visible, sharing one time axis |
| Acid synth editor | The 303 step view and its switch are removed; the roll is the only editor |
| Step-write replacement | Keyboard entry in the roll (cursor + note keys) |
| Song model | Per-part lanes of pattern blocks, replacing bars |
| Pattern grows under a placed block | Later blocks on that lane are pushed right (ripple) |
| Block placement | Free on the lane, snapped to the bar, gaps play silence, never overlapping |
| Dock and song | One dock, two modes: Pattern mode edits notes, Song mode edits blocks |
| 1/64 scope | Acid synths and drums move to tick-based notes like the other parts |
| Extras this round | Scale highlight/snap/fold, note tools menu, trig conditions, chance + ratchet |

Out of scope (backlog): triplet grids (needs a 12-ticks-per-step base),
follow actions, editable ghost notes, per-note expression curves, note
echo, per-drum-row loop lengths, printing chance outcomes to fixed notes,
neighbour trig conditions.

## Phases

Each phase is its own branch and implementation plan, and leaves the app
working.

1. Unified note model; acid and drums convert; 303 view removed.
2. Bottom dock with stacked lanes; keyboard entry.
3. Selection editing.
4. Song lanes.
5. Scale highlight, snap and fold.
6. Chance, ratchet and trig conditions.
7. Note tools menu.

## 1. Unified note model

### Data

`Note` (in `acid-engine/src/project.rs`) gains fields, all with defaults
that are skipped when saving so FM, sampler and Mono files barely change:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `accent` | `bool` | false | Accent (acid, drums) |
| `slide` | `bool` | false | Glide into the next note, holding the envelope (acid) |
| `chance` | `u8` | 100 | Play chance in percent (phase 6) |
| `ratchet` | `u8` | 1 | Even re-triggers within the note (phase 6) |
| `cond` | `Cond` | `Always` | Trig condition (phase 6) |

Fields land in phase 1 so the file format changes once; phase 6 makes the
engine act on `chance`, `ratchet` and `cond`.

- **Acid synths:** `SynthPattern` keeps `length`, `transpose` and its
  per-step `param_lane`, but `steps: Vec<SynthStep>` is replaced by
  `notes: Vec<Note>` in ticks (`TICKS_PER_STEP` = 4, so 1 tick = 1/64).
  Playback stays monophonic. A note with `slide` glides into the next
  note without retriggering the envelope, which is today's slide/tie
  behaviour. Overlapping notes are not stored: edits trim the earlier
  note at the later note's start.
- **Drums:** `DrumPattern` becomes notes with `pitch` = instrument index
  0–7 (BD, SD, LT, HT, CP, RS, CH, OH), `accent` for accent, and `len`
  defaulting to one step (it matters for ratchet in phase 6). Closed hat
  still chokes open hat.
- `MAX_NOTES` (512, a full 64-step pattern on all eight drums) applies to every pattern.

### Migration (project version 4 → 5)

- Acid: an `On` step and the `Tie` steps after it become one note.
  Accent comes from the first step, slide from the last. Pitch comes from
  `SynthStep::midi` with transpose 0.
- Drums: each `On`/`Accent` hit becomes a note at `step × 4` ticks, one
  step long.
- A Tie at step 0 of a pattern whose last step plays continues that note
  over the loop (the last note slides into a same-pitch note at tick 0).
  A Tie after a song bar line or a queued pattern switch starts its own
  note.
- Malformed old step lists are refused with an error, not converted.
- A golden render test proves a converted fixture renders identical
  audio.

### Code that follows

- Sequencer synth and drum playback (`sequencer.rs`).
- Smart randomiser (`random.rs`): dice, Template and Evolve are rewritten
  to produce notes. Behaviour is kept: same scales, groove-aware placement
  and chained slides. The Rest die and the drum dice work on the 1/16
  grid; the other synth dice change the existing notes in place. A step
  the Rest die turns on repeats the nearest earlier note's pitch.
- Song record and pattern copy/paste.
- Removed: `step303.rs`, `step_edit.rs`, the 303 pitch-scroll use of
  `scroll.rs` (its knob wheel gestures stay) and the `SeqView` toggle (including `Edit::View` and the saved view).
  The acid roll (`pianoroll.rs`, `roll_edit.rs`) is folded into the shared
  roll editor.

## 2. Bottom dock

### Layout

- A resizable bottom panel (`egui::TopBottomPanel::bottom`). The rack
  scrolls above it and keeps knobs, pattern pads, length, transpose and
  dice. The rack panels no longer draw editors.
- The dock height is saved in settings and defaults to about 45% of the
  window (the layout targets 1280×800).
- **Dock header:** snap grid (1/4, 1/8, 1/16, 1/32, 1/64), horizontal
  zoom, follow-playhead toggle. Later phases add the scale picker, the
  note inspector and the Tools menu here.

### Lanes

- One lane per machine, in rack order. All lanes share one time ruler,
  zoom and horizontal scroll.
- In Pattern mode each lane shows its machine's selected pattern. Time
  past the pattern's length is shaded. Each lane has its own playhead,
  since patterns loop at different lengths.
- **Lane header:** machine name, pattern label (e.g. A01), collapse
  arrow, and a drag handle for the lane height. Lane height and collapse
  state are saved per machine.
- **Melodic lanes** (acid, FM, sampler, Mono): narrow key strip with
  click-to-audition, vertical scroll through pitch inside the lane,
  starting at the machine's usual range (the existing
  `roll_start_pitch`). Sampler rows keep their slot/slice labels.
- Accent shows as a bright top edge on the note; slide as a tie line to
  the next note.
- **Drum lanes:** eight labelled rows; hits drawn as blocks, accent as a
  brighter fill.
- **Expression strip:** a per-lane toggle opens a thin strip under the
  lane that edits velocity (later also chance and ratchet) for its notes.

### Focus

- Clicking in a lane focuses that machine: the lane is outlined and its
  rack panel header lights up. Keyboard entry, paste and note tools act
  on the focused lane.

### Keyboard entry

- The focused lane has a cursor at a step and pitch (a drum row on drum
  lanes).
- Arrow keys move the cursor by one grid step or one semitone/row.
- The note keys (the tracker layout of the removed step-writer,
  `crates/app/src/ui/step_edit.rs` at commit `32308c6`: Z S X D C V G B H N J M
  for the lower octave, Q 2 W 3 E R 5 T 6 Y 7 U I above) write a note one grid
  step long at the cursor and advance it. On drum lanes any note key writes a hit on
  the cursor's row.
- Backspace deletes the note under the cursor. Space still plays/stops.

## 3. Selection editing

### Mouse

- Click on empty space: add a note. Drag on empty space: marquee select.
- Click a note: select it. Shift-click: toggle it in the selection.
- Drag a selected note: move the whole selection, snapped to the grid,
  keeping relative positions. Alt while dragging: 1-tick resolution.
- Ctrl-drag: move copies, leaving the originals.
- Drag the right edge of a selected note: resize every selected note by
  the same amount.
- Right-click a note: delete it; inside the selection, delete the
  selection.
- Drag along the time ruler: select a time range across all lanes.

### Keys (only while the dock has keyboard focus)

| Key | With a selection | Without |
|---|---|---|
| ←/→ | move notes one grid step | move cursor |
| Alt+←/→ | move notes one tick | — |
| ↑/↓ | transpose a semitone | move cursor |
| Shift+↑/↓ | transpose an octave | — |
| Ctrl+C / Ctrl+X | copy / cut | — |
| Ctrl+V | paste at cursor | paste at cursor |
| Ctrl+D | duplicate right after the selection | — |
| Ctrl+A | select all in lane | select all in lane |
| Delete | delete selection | — |
| Esc | deselect | — |

### Rules

- Every operation is one undo step.
- The clipboard is internal. It stores notes relative to the earliest
  start, with pitch and all fields. A multi-lane clipboard pastes back
  into the same machines.
- Melodic↔melodic paste works; melodic↔drum paste is refused with a
  message.
- Acid lanes stay monophonic: the note just edited wins: other notes
  starting inside it are deleted, and a note sounding into its start is
  cut there.
- Notes past the pattern length are kept but not played.
- A paste that would exceed `MAX_NOTES` is refused with a message.
- Editing logic is pure functions with unit tests, in the style of
  `poly_edit.rs`.

## 4. Song lanes

### Data

- `Song.bars` is replaced by `lanes: Vec<(MachineUid, Vec<Block>)>`,
  blocks sorted by start and never overlapping.
- `Block { start: u32 (steps), pattern: PatternRef, len: Option<u16>,
  mute: bool, transpose: i8 }`.
- `len: None`: the block lasts its pattern's length and follows it.
  `len: Some(n)`: fixed by dragging the block's right edge. Longer than
  the pattern loops it; shorter plays only its start.
- Automation stays keyed by global step. `loop_range` becomes a step
  range. The song ends at its last block's end, or at a later end point a
  converted song carries (`end`). A loop reaching past that end extends
  the song to the loop's end, so it still loops.

### Ripple and overlap

- When a pattern grows, each `None` block of it grows. Later blocks on
  that lane move right only as far as needed to clear it (a gap absorbs
  growth first), cascading down the lane.
- When a pattern shrinks, later blocks stay put and a gap opens.
- `Some` blocks never ripple.
- A pattern-length edit whose ripple would pass the maximum song length
  is refused.
- No operation can create an overlap: a move or paste onto occupied
  space lands on the nearest free spot.

### Dock in Song mode

- Lanes show blocks with a mini preview of their notes; the ruler shows
  16-step bars; one playhead.
- Double-click empty space: place the machine's selected pattern there,
  snapped to the bar. Double-click a block: switch to Pattern mode with
  that pattern selected.
- Click, marquee, Shift-click, drag, Ctrl-drag copy, right-click delete,
  Ctrl+C/V/D, Delete and ←/→ (one bar) work on blocks as in section 3.
- Drag a block's right edge to fix its length.
- Ruler selection: **Loop**, **Insert time** (push everything after it
  right on every lane; a block the insert point falls inside stays put,
  unsplit), **Delete time** (remove it and close the gap; a
  block spanning the deleted range shrinks and keeps its tail). The loop
  triggers when playback crosses its end; a song start after the loop
  plays on.
- **Capture** places every machine's selected pattern at the song
  cursor, replacing "Add bar".
- With one block selected, the dock header shows its pattern, mute and
  transpose (transpose only for machine types that transpose).
- The song panel keeps the automation list; its bar timeline, bar editor
  and bar buttons are removed.

### Engine

- `CompiledSong` holds a sorted block list per rack position. At a
  block's start the track restarts its pattern. Gaps and muted blocks are
  silent. Pattern mode is unchanged.
- Each track's arpeggiator clock counts from its block's start.
- The current-bar machinery (`bar_len`, `compiled_bar_steps`) is
  replaced by block lookup by global step; the song position display
  shows bar = step / 16.

### Recording

- A pattern change while recording in Song mode writes a block at the
  play position, replacing what it covers on that lane only. Knob
  recording is unchanged. A record pass remains one undo step.

### Migration (project version 5 → 6)

- Each bar's cell becomes one block at the bar's start step. A pattern
  exactly as long as the bar gets `len: None`, otherwise
  `len: Some(bar length)`, reproducing today's looping exactly. Mutes and
  transposes move onto the blocks. Loop range converts from bars to
  steps.
- A golden render test proves a converted song sounds the same.

## 5. Scale

- The project gets one key: root and scale. Scales are the randomiser's
  existing set plus Chromatic (off). The picker is in the dock header.
  Drum lanes ignore it.
- Highlight: in-scale rows shaded, root row marked.
- Snap to scale (toggle): new notes, dragged notes and ↑/↓ transposes
  land on scale degrees.
- Fold (per-lane toggle): hides out-of-scale rows; rows holding
  off-scale notes stay visible.

## 6. Chance, ratchet and trig conditions

- **Chance:** 0–100%. Decided by a hash of the project seed, rack
  position, the track's loop count and the note index, so playback,
  re-render and WAV export are repeatable. No allocation on the audio
  thread (covered by `tests/no_alloc.rs`).
- **Ratchet:** 1–8 evenly spaced re-triggers inside the note's length.
  On acid each is a fresh gate.
- **Conditions** (`Cond`): `Always`, `Ratio(a, b)` (plays on pass a of
  every b, b ≤ 8), `First`, `NotFirst`, `Fill`, `NotFill`, `Pre`,
  `NotPre` (whether the lane's previous conditional note played). Each
  track counts its loops; the count resets on Play and at each song
  block's start.
- **Fill:** a transport button, on while held, Shift-click latches.
- **Inspector:** with notes selected, the dock header edits velocity,
  chance, ratchet, condition, accent and slide for all of them. The
  expression strip can show chance and ratchet.
- Notes with chance < 100 or a condition get a dashed outline and a
  badge ("50%", "1:4", "×3").

## 7. Note tools menu

A **Tools** menu in the dock header acts on the focused lane's selection,
or the whole pattern if nothing is selected. Each tool has a popup with
its settings and previews live; Apply commits one undo step, Cancel
reverts.

| Tool | Settings |
|---|---|
| Quantize | grid, strength %, quantize ends |
| Humanize | timing ± ticks, velocity ± |
| Legato | — (each note stretches to the next) |
| Reverse | — |
| Invert | — (mirror pitches within the selection's range) |
| Strum | ticks between notes, up/down (melodic) |
| Randomize in scale | density, pitch range, length range, seed, new seed |
| Arpeggiate | up / down / up-down / random, rate, gate (melodic) |
| Euclidean | hits, steps, rotation, pitch or drum row |

Each tool is a pure function over notes with unit tests.

## Testing

- TDD on the pure editing, ripple and tool functions.
- Sequencer tests: acid/drum note playback, slide, block playback and
  gaps, ripple, chance determinism, ratchet timing, conditions, Fill.
- Golden render tests for both migrations.
- Headless draw tests for the dock in both modes.
- Each phase checked in the running app with a screenshot.
