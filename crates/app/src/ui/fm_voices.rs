//! Loading FM voices: from the starter bank, an imported DX7 bank, or a
//! .syx file. A load is one undoable edit.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::fm::starter::starter_bank;
use acid_engine::fm::sysex::{self, Dx7Voice};
use acid_engine::machine::MachineId;
use std::sync::OnceLock;

/// Which list the voice browser shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FmBrowser {
    #[default]
    Starter,
    Imported,
}

/// The starter bank, built on first use.
pub(crate) fn starter() -> &'static [Dx7Voice] {
    static BANK: OnceLock<Vec<Dx7Voice>> = OnceLock::new();
    BANK.get_or_init(starter_bank)
}

impl SidAcidApp {
    /// Loads a voice that did not come from the browser grid (so none of
    /// its rows is highlighted); the grid sets `fm_voice_src` after this.
    pub(crate) fn load_fm_voice(&mut self, voice: &Dx7Voice) {
        self.view.fm_voice_src = None;
        let cmds = self.doc.apply(
            Edit::FmVoice {
                name: voice.name.clone(),
                values: voice.to_params(),
            },
            false,
        );
        self.send(cmds);
        self.view.focus = MachineId::Fm.index();
    }

    pub(crate) fn import_syx(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DX7 SysEx", &["syx", "SYX"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) => self.import_syx_bytes(&bytes),
            Err(e) => self.view.message = Some(format!("Could not read {}:\n{e}", path.display())),
        }
    }

    /// A single voice loads straight into the FM; a bank goes to the browser.
    pub(crate) fn import_syx_bytes(&mut self, bytes: &[u8]) {
        match sysex::parse(bytes) {
            Ok(voices) if voices.len() == 1 => self.load_fm_voice(&voices[0]),
            Ok(voices) => {
                let cmds = self.doc.apply(Edit::FmBank { bank: Some(voices) }, false);
                self.send(cmds);
                self.view.fm_browser = FmBrowser::Imported;
            }
            Err(e) => {
                self.view.message = Some(format!(
                    "Could not import the file: {e}. Use a DX7 .syx file."
                ))
            }
        }
    }
}
