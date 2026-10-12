# Third-party notices

`crates/sid-core` is derived from the Acid OS synthesiser (`acid-synth`),
https://github.com/NorfolkHacker/Acid-OS-V3, used under the MIT licence:

    MIT License — Copyright (c) 2026 NorfolkHacker
    (full text identical to LICENSE in this repository)

## DX7 envelope and scaling math

`crates/acid-engine/src/fm/env.rs` follows the DX7 envelope, output-level,
keyboard-scaling and velocity tables published in Google's
music-synthesizer-for-android (https://github.com/google/music-synthesizer-for-android),
used under the Apache License, Version 2.0
(https://www.apache.org/licenses/LICENSE-2.0). The code here is a new Rust
implementation; the tables and formulas are from that project.
