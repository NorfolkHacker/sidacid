# Manual style guide (for writers, not readers)

This file is for whoever writes or updates the manual. It is not linked from the manual.

## Facts

- Every statement must match the code in `crates/` as it is now. Check ranges, defaults, switch positions, key bindings and labels in the source (`crates/acid-engine/src/params.rs` for knob names and defaults, `crates/app/src/ui/` for labels, buttons and keys). When the README and the code disagree, the code wins. Note the disagreement in your report.
- Use the labels exactly as the UI shows them (for example "Env Mod", "Kit A", "Snap to scale", "Len 16").
- Don't describe features that don't exist. If you're unsure how something behaves, say so in your report rather than guessing in the text.

## Names

- No commercial product, maker or trademark names anywhere: no names of real synths, drum machines, sound chips, computers or companies. Describe the sound or behaviour instead ("a classic 8-bit home-computer sound chip", "classic analog drum machines", "classic 6-operator FM synths").
- The file formats `.syx`, `.wav`, `.flac`, `.mp3`, `.ogg` and the audio systems ALSA, JACK, PipeWire, PulseAudio, WASAPI and ASIO are fine to name: they are formats and drivers the user has to pick.

## Voice

- Second person, present tense, plain English: "Drag a note's right edge to make it longer."
- Short paragraphs. Use tables for knob and parameter lists (Control | What it does | Range or positions). Use numbered steps for procedures.
- Define a term the first time it appears in a chapter, or link to where it's defined.
- No marketing language.

## Structure

- One chapter per file, starting with a `# Title` and a one-paragraph summary of what the chapter covers.
- `##` for sections, `###` for subsections. No deeper.
- Link to other chapters with relative links (`[Song mode](song-mode.md)`), to sections with anchors (`[conditions](editor-dock.md#conditions)`).
- Images live in `docs/images/` and are linked as `../images/name.png`. Only use images that exist.
- End each chapter with a "See also" line linking related chapters.
