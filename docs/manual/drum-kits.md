# Drum kits

SidAcid has two drum machines, **Kit A** and **Kit B**. Both play the same eight instruments, in the style of classic analog drum machines, but synthesised on a sound engine modelled on a classic 8-bit home-computer sound chip. Kit A has long, boomy drums; Kit B is punchier and more metallic. This chapter covers the instruments and how each kit voices them, the knobs, accents, the closed hat choking the open hat, and the drum panel's pads and buttons.

A new project's rack has one of each, named **Kit A** and **Kit B**. You can add more from the rack's **+ Add machine** menu, where the types are also called **Kit A** and **Kit B** (see [Rack and patterns](rack-and-patterns.md)).

## The instruments

| Short name | Instrument |
|---|---|
| BD | Bass drum (kick) |
| SD | Snare drum |
| LT | Low tom |
| HT | High tom |
| CP | Hand clap |
| RS | Rim shot |
| CH | Closed hi-hat |
| OH | Open hi-hat |

Each instrument is built from a pitched oscillator whose pitch drops quickly after the hit, filtered noise, or both. The sections below describe what each kit does with them.

## The panel

A drum kit's panel shows:

- the header, with the machine's name, "DRUMS" and the selected pattern (for example "KIT A DRUMS   A01");
- a grid of knobs: one column per instrument (BD to OH) and one row each for **Tune**, **Decay**, **Tone** and **Level**;
- the kit's **Volume** and **Accent** knobs, to the right of the grid;
- the pattern pads (banks A–H, patterns 1–16), the **Len** field and the **Template** and **Evolve** buttons.

The panel has no step grid. You place and edit drum hits in the kit's lane in the [editor dock](editor-dock.md).

Drag a knob up or down to turn it (hold Shift for fine control), or use the mouse wheel over it. Double-click a knob to reset it. The knobs in the instrument grid have no labels of their own: hover over one to see its name and value as a percentage.

## Instrument knobs

Every instrument has the same four knobs:

| Control | What it does | Range | Default |
|---|---|---|---|
| Tune | The instrument's pitch. On most instruments this is one octave down to one octave up; see the kit tables for the exceptions. | 0–100 % (50 % is the instrument's normal pitch) | 50 % |
| Decay | How long the instrument rings. | 0–100 % | 50 % |
| Tone | The instrument's colour. What it changes depends on the instrument and the kit. | 0–100 % | 50 % |
| Level | The instrument's volume in the kit. At 0 the instrument is silent. | 0–100 % | 80 % |

Level works on a curve: half the knob's travel gives a quarter of the full volume, so the top of the knob has the most effect.

### Kit A

| Instrument | Sound | Tune | Decay | Tone |
|---|---|---|---|---|
| BD | A long, soft triangle-wave boom that drops to about 50 Hz. | Pitch | 0.15 to 1.35 s | How far the pitch drops at the start (from 1.5 to 4.5 times the final pitch): more Tone, more punch |
| SD | Two triangle tones dropping in pitch, plus high-passed noise. | Pitch | Body 0.12 to 0.22 s, noise 0.1 to 0.35 s | Balance: low Tone is more body, high Tone is more noise |
| LT, HT | Triangle-wave toms (about 110 Hz and 175 Hz) with a pitch drop and a touch of noise. | Pitch | 0.2 to 0.8 s | How far the pitch drops at the start |
| CP | Three quick bursts of band-passed noise, then a tail. | No effect | Tail 0.1 to 0.5 s | Pitch of the noise (band centre 1000 to 2500 Hz) |
| RS | A short, high pulse-wave click. | Pitch | 25 to 45 ms | Loudness of the click |
| CH, OH | High-passed noise. | Moves the noise filter, up to half an octave down or up | CH 30 to 110 ms, OH 0.25 to 1.05 s | Brightness (filter cutoff 6 to 11 kHz) |

### Kit B

| Instrument | Sound | Tune | Decay | Tone |
|---|---|---|---|---|
| BD | A punchy kick: a short pulse-wave click, then a triangle wave with a fast pitch drop to about 50 Hz. | Pitch | 0.15 to 0.75 s | How far the pitch drops at the start (from 3 to 7 times the final pitch) |
| SD | A triangle tone dropping in pitch, plus high-passed noise. | Pitch | Body 0.08 to 0.14 s, noise 0.08 to 0.43 s | Amount of noise: higher is snappier |
| LT, HT | Triangle-wave toms (about 100 Hz and 160 Hz) with a short pulse-wave click and a pitch drop. | Pitch | 0.15 to 0.65 s | How far the pitch drops at the start |
| CP | Four quick, tightly spaced bursts of band-passed noise, then a tail. | No effect | Tail 0.08 to 0.38 s | Pitch of the noise (band centre 1200 to 2200 Hz) |
| RS | A short, ring-modulated triangle tone. | Pitch | 20 to 40 ms | The ring modulation's colour (more or less metallic) |
| CH, OH | A metallic ring-modulated tone plus high-passed noise. | Pitch of the metallic tone | CH 40 to 140 ms, OH 0.2 to 0.9 s | Brightness of the noise (filter cutoff 7 to 11 kHz) |

## Kit knobs

| Control | What it does | Range | Default |
|---|---|---|---|
| Volume | The whole kit's output level, before its mixer channel. Like Level, it works on a curve. | 0–100 % | 70 % |
| Accent | How much louder accented hits are: from no change at 0 % to double the level at 100 %. | 0–100 % | 50 % |

The kit plays in mono: every instrument is centred. Use the kit's mixer channel to pan it (see [Mixer and effects](mixer-and-effects.md)).

## Accents

Any hit can be accented. An accented hit plays louder by the amount the kit's **Accent** knob sets; its sound is otherwise the same. You accent hits in the dock: double-click a hit, use the lane's accent strip, or use the **Accent** control in the dock's note inspector (see [Editor dock](editor-dock.md)).

## Closed hat chokes open hat

When a closed hat (CH) plays while an open hat (OH) is still ringing, the open hat is cut off with a very fast fade, as on classic drum machines. Use this for the familiar open-hat-then-closed-hat figure: a long OH Decay rings until the next closed hat stops it.

## Voices

A kit plays up to six sounds at once. Each instrument plays one sound at a time: a new hit cuts off that instrument's previous one. If six instruments are already sounding, a new hit takes over the sound that started longest ago.

## Pattern length

**Len** sets the selected pattern's length in 1/16 steps, from 1 to 64. Drag the field, or click it and type a value. A drag is one undo step; each arrow-key press or typed value is its own. A new pattern is 16 steps long, and patterns of different lengths on different machines loop independently.

## Template and Evolve

Both buttons replace the selected pattern, and each click is one undo step: press Ctrl+Z to go back. Each click uses fresh random numbers, so clicking again gives a different result.

**Template** replaces the pattern with a fresh basic groove that fills the pattern's current length, repeating every 16 steps:

- **Kick:** half the time four on the floor (steps 1, 5, 9 and 13), otherwise one of three syncopated patterns. The kick on the first step of every 16 is accented.
- **Backbeat:** a snare or a clap on steps 5 and 13.
- **Hats:** one of four patterns: closed hats on the beat with open hats on the off-beats, closed eighths, closed sixteenths, or a broken closed-hat figure with an open hat on step 15.

**Evolve** makes one to three small changes to the pattern. Each change is one of:

- adding a hit, most often a closed hat, kick or open hat, and most often on a beat;
- moving a hit one step earlier or later;
- accenting a plain hit, or un-accenting an accented one.

Both buttons work on the 1/16 grid. After either one, every hit sits on a step: Evolve snaps off-grid hits to the step they start in, and it resets the hits' chance, ratchet and conditions. On any step where an open hat plays, both buttons remove the closed hat.

The acid synths' **Dice** buttons also read a kit's selected pattern, so the bass line grooves with the drums: the first acid synth pairs with the first kit in the rack, the second with the second (see [Acid synth](acid-synth.md#dice)).

## Editing hits

You place, move and delete drum hits in the kit's lane in the [editor dock](editor-dock.md). A drum lane has one row per instrument, labelled BD to OH. Click empty space to add a hit, right-click a hit to delete it, and drag hits to move them. Hits can sit at any snap position, not only on 1/16 steps. Double-click a hit to toggle its accent. Every hit can also have a chance, ratchet and condition (see [Editor dock](editor-dock.md)).

## See also

[Pad Kit](pad-kit.md) · [Editor dock](editor-dock.md) · [Rack and patterns](rack-and-patterns.md) · [Acid synth](acid-synth.md) · [Mixer and effects](mixer-and-effects.md) · [Song mode](song-mode.md)
