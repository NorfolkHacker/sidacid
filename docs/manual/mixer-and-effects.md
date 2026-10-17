# Mixer and effects

Every machine in the rack has its own mixer channel, with distortion, a compressor, level, pan, mute, solo and sends to a shared delay and reverb. The channels, the two effect returns and the master compressor sit in the **MIXER & FX** section below the rack; the master level and the Lo-Fi switch are in the transport bar. This chapter covers each of them, the level meters, and the CPU and memory readout.

## Signal flow

Each machine's sound goes through its channel in this order:

1. **Distortion** (if **Dist** is on)
2. **Compressor** (if **Comp** is on)
3. **Level** and **Pan**
4. **Mute** and **Solo**
5. The **Delay** and **Reverb** sends, taken after Level and Pan (post-fader)

All channels are added together, with the delay and reverb returns, and then go through the master bus:

1. **Master compressor** (if its **Comp** is on)
2. **Master** level
3. **Lo-Fi** (if on)
4. A final clip at full scale (0 dBFS), so the output never goes past it

If something in the mix ever produces an invalid value, SidAcid silences that moment of audio, clears the effects, and shows "Audio fault recovered" under the master section for a few seconds.

## Using the controls

The labels on the knobs are short ("Thresh", "Fdbk"); hovering over a knob shows its full name and value. In the mixer the value is shown as a percentage of the knob's travel; the tables below give what that means in real units. Switches show their state ("Off" or "On") and change when you click them: a left click moves forward, a right click moves back. See [Getting started](getting-started.md) for dragging, fine adjustment and resetting knobs.

## Channel strips

There is one strip per machine, in rack order, headed by the machine's name and a level meter. Adding, removing or moving a machine in the rack changes the strips to match (see [The rack and patterns](rack-and-patterns.md)). The row of strips scrolls sideways when it is wider than the window.

### Level, pan, mute and solo

| Control | What it does | Range or positions | Default |
|---|---|---|---|
| **Level** | The channel's volume. | Silent to +6 dB | 70% (about 0 dB) |
| **Pan** | Places the channel in the stereo field (equal-power). | Left to right | Centre |
| **Mute** | Silences the channel, including its sends. | Off, On | Off |
| **Solo** | While any channel is soloed, only soloed channels are heard. Channels that are not soloed lose their sends too. | Off, On | Off |

### Distortion

A waveshaping distortion. It runs at twice the sample rate (2× oversampling) to keep harsh aliasing down at high Drive.

| Control | What it does | Range or positions | Default |
|---|---|---|---|
| **Dist** | Turns the distortion on. | Off, On | Off |
| **Drive** | How hard the sound is pushed into the distortion. | 0 to +32 dB of gain | 30% (about +10 dB) |
| **Tone** | Low-pass filter on the distorted sound. Lower is darker. | 800 Hz to 12 kHz | 60% (about 4 kHz) |
| **Mix** | Balance between the clean and the distorted sound. | 0% (clean) to 100% (distorted) | 100% |

### Compressor

A compressor evens out the level: when the sound goes above the threshold, it is turned down by the ratio. It follows the peaks of both sides together and has a soft knee (it starts working gradually, 3 dB below the threshold).

| Control | What it does | Range or positions | Default |
|---|---|---|---|
| **Comp** | Turns the compressor on. | Off, On | Off |
| **Thresh** | Level above which compression starts. | −40 dB to 0 dB | 70% (−12 dB) |
| **Ratio** | How strongly the level above the threshold is reduced. The knob is not linear: its middle is about 4.5:1. | 1:1 to 20:1 | 25% (about 2.1:1) |
| **Attack** | How fast the compressor turns the sound down. | 0.1 ms to 100 ms | 20% (about 0.4 ms) |
| **Release** | How fast it lets the level come back up. | 10 ms to 1000 ms | 40% (about 63 ms) |
| **Makeup** | Gain added after compression. | 0 dB to +24 dB | 0 dB |

### Sends

| Control | What it does | Range | Default |
|---|---|---|---|
| **Delay** | How much of the channel goes to the delay ("Delay Send" in tooltips). | 0–100% | 0% |
| **Reverb** | How much of the channel goes to the reverb ("Reverb Send" in tooltips). | 0–100% | 0% |

The sends are taken after Level and Pan, so turning a channel down also turns down what it sends.

## Delay

One delay is shared by every channel. Its controls are under **Delay**, to the right of the strips.

| Control | What it does | Range or positions | Default |
|---|---|---|---|
| **Sync** | On: the delay time follows the project tempo, set by **Time**. Off: the delay time is set in milliseconds by **ms**. | Off, On | On |
| **Time** | Delay time in note lengths, when Sync is on. | 1/16, 1/8, 3/16, 1/4, 3/8, 1/2 | 3/16 |
| **ms** | Delay time in milliseconds, when Sync is off. | 10 ms to 2000 ms | 30% (about 49 ms) |
| **Fdbk** | Feedback: how much of each echo is fed back to make the next. | 0% to 95% of the echo | 40% (38%) |
| **Tone** | Low-pass filter in the feedback path. Each repeat is darker than the last; lower is darker. | 500 Hz to 16 kHz | 60% (4 kHz) |
| **Ping** | Ping-pong. On: the send is mixed to mono and the echoes bounce between left and right. Off: each side echoes on its own side. | Off, On | On |
| **Return** | How loud the delay is in the mix. | 0–100% | 70% |

The longest delay is 2 seconds. A synced time longer than that (a 1/2 note below 60 BPM) is held at 2 seconds.

## Reverb

One reverb is shared by every channel. Its controls are under **Reverb**, below the delay. The reverb mixes its send to mono on the way in and spreads it back out to stereo.

| Control | What it does | Range | Default |
|---|---|---|---|
| **Size** | Length of the reverb tail. | Short to very long | 60% |
| **Damp** | Damping: how quickly high frequencies fade in the tail. Higher is darker. | 0–100% | 50% |
| **Width** | Stereo width of the reverb. 0% is mono. | 0–100% | 100% |
| **Return** | How loud the reverb is in the mix. | 0–100% | 60% |

The delay and the reverb are separate: the delay's echoes do not go into the reverb.

## Master bus

### Master compressor

The master compressor works on the whole mix and has the same controls as a channel compressor, under **Master**, with different defaults. It is on by default.

| Control | Range or positions | Default |
|---|---|---|
| **Comp** | Off, On | On |
| **Thresh** | −40 dB to 0 dB | 80% (−8 dB) |
| **Ratio** | 1:1 to 20:1 | 15% (about 1.6:1) |
| **Attack** | 0.1 ms to 100 ms | 30% (about 0.8 ms) |
| **Release** | 10 ms to 1000 ms | 40% (about 63 ms) |
| **Makeup** | 0 dB to +24 dB | 0 dB |

### Master level and Lo-Fi

These two are in the transport bar at the top of the window.

| Control | What it does | Range or positions | Default |
|---|---|---|---|
| **Master** | The output level, after the master compressor. | Silent to about +3.5 dB | 80% (about −0.4 dB) |
| **Lo-Fi** | Turns the whole mix into 8-bit audio held at a 22.05 kHz sample rate, for a crunchy, low-resolution sound. | Off, On | Off |

Lo-Fi comes after the Master level, so turning Master down also lowers the resolution Lo-Fi leaves for your sound.

## Meters

Each strip has a meter next to its name, and the master section has one next to "Master".

- A meter shows the peak level since its last reading, on a scale from −48 dB (empty) to 0 dB (full).
- It turns from green to red when the peak reaches full scale (0 dB, within about 0.1 dB).
- A channel meter reads after Level and Pan. A muted channel, or one silenced by another channel's Solo, reads empty.
- The master meter reads the final output, after Lo-Fi and the clip.

A red master meter means the output is clipping: turn down **Master**, or the channels feeding it.

## The performance readout

The transport bar shows how hard the computer is working, after the **Master** knob.

| Readout | What it shows |
|---|---|
| **CPU n%** | How much of each audio callback's time is spent making sound, smoothed. It turns amber above 80% and red at 100%. Click it to open Audio Settings. |
| **pk n%** | The worst callback in the last second. |
| **MEM n MB** | Memory the app is using, read once a second. |
| **Smp n/256 MB** | Decoded sampler audio in use, against the [sample memory limit](sampler.md#limits). It turns amber above 80%. |
| **⚠ n** | Appears after an audible dropout: the count of renders that ran out of time plus dropouts the audio driver reports (JACK only). Hover to see the split and how long ago the last one was; click to clear it. |

Hover over **CPU** or **pk** to see the audio driver, device, sample rate and buffer size, and a "Top:" line naming the three busiest machines and the share spent on the effects and master bus ("FX/master").

Each machine's panel header also shows its own share as **CPU n%**. It turns amber above 25% and red above 50%.

See also: [Getting started](getting-started.md) · [The rack and patterns](rack-and-patterns.md) · [Sampler](sampler.md) · [Song mode](song-mode.md) · [Files and export](files-and-export.md)
