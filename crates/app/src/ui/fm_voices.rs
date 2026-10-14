//! Loading FM voices: from the starter bank, an imported DX7 bank, or a
//! .syx file. A load is one undoable edit.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::fm::starter::starter_bank;
use acid_engine::fm::sysex::{self, Dx7Voice};
use acid_engine::machine::MachineUid;
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
    pub(crate) fn load_fm_voice(&mut self, uid: MachineUid, voice: &Dx7Voice) {
        self.view.panel(uid).fm_voice_src = None;
        let values = voice
            .to_params()
            .into_iter()
            .map(|(p, v)| (uid.param(p), v))
            .collect();
        let cmds = self.doc.apply(
            Edit::FmVoice {
                uid,
                name: voice.name.clone(),
                values,
            },
            false,
        );
        self.send(cmds);
        self.view.focus = Some(uid);
    }

    pub(crate) fn import_syx(&mut self, uid: MachineUid) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DX7 SysEx", &["syx", "SYX"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) => self.import_syx_bytes(uid, &bytes),
            Err(e) => self.view.message = Some(format!("Could not read {}:\n{e}", path.display())),
        }
    }

    /// A single voice loads straight into FM machine `uid`; a bank goes
    /// to its browser.
    pub(crate) fn import_syx_bytes(&mut self, uid: MachineUid, bytes: &[u8]) {
        match sysex::parse(bytes) {
            Ok(voices) if voices.len() == 1 => self.load_fm_voice(uid, &voices[0]),
            Ok(voices) => {
                let edit = Edit::FmBank {
                    uid,
                    bank: Some(voices),
                };
                let panel = self.view.panel(uid);
                panel.fm_voice_src = voice_source_after(panel.fm_voice_src, Some(&edit));
                let cmds = self.doc.apply(edit, false);
                self.send(cmds);
                self.view.panel(uid).fm_browser = FmBrowser::Imported;
            }
            Err(e) => {
                self.view.message = Some(format!(
                    "Could not import the file: {e}. Use a DX7 .syx file."
                ))
            }
        }
    }
}

impl super::ViewState {
    /// Before `edit` is applied (an undo or redo): drops the voice source
    /// of the FM machine it loads a voice or bank into, as
    /// `voice_source_after` says.
    pub(crate) fn voice_source_before(&mut self, edit: Option<&Edit>) {
        if let Some(&(Edit::FmVoice { uid, .. } | Edit::FmBank { uid, .. })) = edit {
            let panel = self.panel(uid);
            panel.fm_voice_src = voice_source_after(panel.fm_voice_src, edit);
        }
    }
}

/// The browser row still to highlight once `edit` is applied. A voice load
/// brings in a voice that came from no row; a bank change puts other voices
/// under an imported row's index.
pub(crate) fn voice_source_after(
    src: Option<(FmBrowser, usize)>,
    edit: Option<&Edit>,
) -> Option<(FmBrowser, usize)> {
    match (edit, src) {
        (Some(Edit::FmVoice { .. }), _) => None,
        (Some(Edit::FmBank { .. }), Some((FmBrowser::Imported, _))) => None,
        _ => src,
    }
}
