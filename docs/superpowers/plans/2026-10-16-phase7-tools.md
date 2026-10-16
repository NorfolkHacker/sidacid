# Phase 7: Note Tools Menu Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tools menu in the dock header runs a note tool on the focused lane's selection (or its whole visible pattern when nothing is selected): Quantize, Humanize, Legato, Reverse, Invert, Strum, Randomize in scale, Arpeggiate, Euclidean. Each tool opens a small popup with its settings, previews live, and Apply commits one undo step while Cancel restores the pattern.

**Architecture:** Pure tool functions in `crates/app/src/ui/note_tools.rs` take `(&NotePattern, &[usize], &Params, kind, key) -> (NotePattern, Vec<usize>)` and always start from the same snapshot, so previews never drift. The dock keeps an open-tool state (`ViewState::tool: Option<ToolSession { uid, slot, origin, idx, tool, params }>`); while a session is open other edits on that lane are blocked (the same guard as a live drag). Apply ends the merged gesture; Cancel undoes it (or re-applies the origin when nothing was committed).

**Tech Stack:** Rust 2024, egui 0.31.

**Spec:** `docs/superpowers/specs/2026-10-16-dock-song-lanes-design.md` §7. Phases 1–6 are merged (`note_ops::settle`, selection by value, `Key` from phase 5, `Note` fields from phase 6).

## Global Constraints

- User-facing text never names a commercial product or manufacturer.
- No engine changes; golden hashes unchanged.
- Scope = selection, or every note starting inside the pattern length when nothing is selected (never notes hidden past the end).
- Tools that change timing or pitch run `note_ops::settle` on the touched notes (acid stays one note at a time; poly merges same-pitch overlaps), except where a tool states otherwise.
- Apply = one undo step; Cancel leaves no undo step and the pattern as it was; while open, no other edit on that lane (keys, mouse, clipboard, undo/redo) is applied.
- Randomness is seeded: the popup shows a seed and a "New seed" button; the same seed gives the same result.
- Branch `phase7-tools`; usual commit identity and trailer; `cargo fmt --all`; each task ends with the workspace tests and clippy `-D warnings` clean.

## Tools and their settings

| Tool | Settings | Rule |
|---|---|---|
| Quantize | grid (the five snaps), strength 0–100 %, ends on/off | start moves `strength` of the way to the nearest grid line; with ends, the end too (length ≥ 1) |
| Humanize | timing ± ticks (0–8), velocity ± (0–40), seed | uniform random offsets per note, clamped (start ≥ 0, vel 1–127) |
| Legato | — | each note's length runs to the next note's start (the next start strictly after it, any pitch on acid/drums, same lane); the last runs to the pattern end |
| Reverse | — | `start' = span_start + span_end − end` for each note (the selection's span) |
| Invert | — | `pitch' = lo + hi − pitch` over the selection's pitch range (melodic only; disabled on drums) |
| Strum | ticks between notes (1–8), up/down | notes sharing a start are staggered in pitch order (up = low first) |
| Randomize in scale | density 10–100 %, pitch range (low, high), length range (min, max in snap units), seed | replaces the scope's notes with new ones on the dock snap grid across the scope's time span, pitches in the project key (any pitch when Chromatic), drums: random instruments; acid: one note at a time |
| Arpeggiate | style (up, down, up-down, random), rate (the five snaps), gate 10–100 % | notes sharing a start (a chord) become a sequence through the chord's pitches at `rate` across the chord's length; melodic only |
| Euclidean | hits, steps (1–64), rotation, pitch (melodic) or drum row | over the scope's time span (or the pattern) in dock-snap cells: places `hits` notes spread by the Bresenham/Euclid rule, rotated; replaces existing notes on that pitch/row within the span |

---

### Task 1: Pure tools (timing and pitch)

`note_tools.rs` with Quantize, Humanize, Legato, Reverse, Invert, Strum, each a pure function with unit tests (incl. acid settle behaviour and drum restrictions).

Commit: `feat(app): note tools for timing and pitch`.

### Task 2: Pure tools (generators)

Randomize in scale, Arpeggiate, Euclidean with unit tests (determinism by seed, key respect, Euclid patterns E(3,8) = x..x..x., rotation, drum rows).

Commit: `feat(app): generator note tools`.

### Task 3: The Tools menu, popups, preview, Apply/Cancel

Dock header "Tools" menu (Pattern mode; disabled tools greyed per lane kind), a popup window per tool with its settings, live preview from the snapshot, Apply/Cancel, the edit guard while open, seed controls. Frame tests: preview changes the pattern, Cancel restores with no undo step, Apply is one undo step, other keys are blocked while open, scope rules.

Commit: `feat(app): the Tools menu with live preview`.

### Task 4: Docs and a look

README Tools bullets (no product names); brief cropped screenshot (delete any full capture); final gate. Commit `docs: note tools`.
