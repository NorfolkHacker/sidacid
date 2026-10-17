//! The warning before an action that would leave the system short of
//! free memory: adding or duplicating a machine, loading a sample or
//! opening a project. Nothing is refused; the user may go ahead.

use super::SidAcidApp;
use crate::sysmem::{MemLevel, MemSnapshot, format_bytes, level};
use acid_engine::machine::{MachineType, MachineUid};
use acid_engine::project::Project;
use acid_engine::sampler::decode::Decoded;
use eframe::egui;

/// An action that needs memory, held while the warning asks.
pub(crate) enum MemAction {
    Add(MachineType),
    Duplicate(MachineUid),
    /// A decoded file for slot `slot` of sampler `uid`.
    LoadSample {
        uid: MachineUid,
        slot: usize,
        name: String,
        decoded: Box<Decoded>,
    },
    /// A decoded file for pad `pad` of Pad Kit `uid`.
    LoadPad {
        uid: MachineUid,
        pad: usize,
        name: String,
        decoded: Box<Decoded>,
    },
    /// Several decoded files for consecutive pads from `first`, held as one
    /// action; `skipped` names the files that failed, of `total` dropped.
    LoadPads {
        uid: MachineUid,
        first: usize,
        files: Vec<(String, Box<Decoded>)>,
        skipped: Vec<String>,
        total: usize,
    },
    /// A parsed kit file for Pad Kit `uid`.
    LoadKit {
        uid: MachineUid,
        kit: Box<acid_engine::padkit::file::PadKitFile>,
    },
    /// A parsed project that passed `check_fits`, with where it came from.
    Open {
        project: Box<Project>,
        path: std::path::PathBuf,
    },
}

pub(crate) struct MemWarn {
    pub action: MemAction,
    /// Free memory the action would leave, in bytes.
    pub free_after: u64,
    /// The window has drawn once (and given Cancel the focus).
    shown: bool,
}

/// Whether an action needing `extra` bytes would leave the system in the
/// red: the free memory it would leave if so. None without figures.
pub(crate) fn warn_for(extra: u64, mem: Option<MemSnapshot>) -> Option<u64> {
    let m = mem?;
    let after = m.available.saturating_sub(extra);
    (level(m.total, after) == MemLevel::Critical).then_some(after)
}

/// The engine memory `p`'s machines take, by their types' estimates
/// (samples aside).
pub(crate) fn machines_bytes(p: &Project) -> u64 {
    p.machines
        .iter()
        .map(|m| m.machine_type().memory_estimate() as u64)
        .sum()
}

/// The warning's text.
fn warning_text(free: u64) -> String {
    format!(
        "Adding this leaves about {} of free memory. The system may slow down or the app may be stopped.",
        format_bytes(free)
    )
}

impl SidAcidApp {
    /// Runs `action` now, or holds it behind the warning (replacing one
    /// already held). The machine an add or duplicate made now, if any.
    pub(crate) fn with_memory_check(
        &mut self,
        extra: u64,
        action: MemAction,
    ) -> Option<MachineUid> {
        if self.mem.is_none() {
            self.mem = self.sysmem.snapshot();
        }
        match warn_for(extra, self.mem) {
            Some(free_after) => {
                self.pending_warn = Some(MemWarn {
                    action,
                    free_after,
                    shown: false,
                });
                None
            }
            None => self.run_mem_action(action),
        }
    }

    /// Runs the held action (`go`) or drops it.
    pub(crate) fn resolve_memory_warning(&mut self, go: bool) {
        if let Some(warn) = self.pending_warn.take()
            && go
        {
            self.run_mem_action(warn.action);
        }
    }

    fn run_mem_action(&mut self, action: MemAction) -> Option<MachineUid> {
        match action {
            MemAction::Add(ty) => self.add_machine_now(ty),
            MemAction::Duplicate(uid) => self.duplicate_machine_now(uid),
            MemAction::LoadSample {
                uid,
                slot,
                name,
                decoded,
            } => {
                self.load_decoded_now(uid, slot, &name, *decoded);
                None
            }
            MemAction::LoadPad {
                uid,
                pad,
                name,
                decoded,
            } => {
                self.load_pad_decoded_now(uid, pad, &name, *decoded);
                None
            }
            MemAction::LoadPads {
                uid,
                first,
                files,
                skipped,
                total,
            } => {
                self.load_pads_now(uid, first, files, skipped, total);
                None
            }
            MemAction::LoadKit { uid, kit } => {
                self.load_kit_now(uid, *kit);
                None
            }
            MemAction::Open { project, path } => {
                self.open_project_now(*project, path);
                None
            }
        }
    }

    pub(crate) fn memory_warning_ui(&mut self, ctx: &egui::Context) {
        let Some(warn) = &mut self.pending_warn else {
            return;
        };
        let first = !std::mem::replace(&mut warn.shown, true);
        let text = warning_text(warn.free_after);
        let mut answer = None;
        // A modal: the rest of the UI gets no input while the warning is
        // up, so the document cannot change under the held action.
        let modal = egui::Modal::new(egui::Id::new("low_memory")).show(ctx, |ui| {
            ui.heading("Low memory");
            ui.label(text);
            ui.horizontal(|ui| {
                if ui.button("Go ahead").clicked() {
                    answer = Some(true);
                }
                let cancel = ui.button("Cancel");
                if first {
                    cancel.request_focus();
                }
                if cancel.clicked() {
                    answer = Some(false);
                }
            });
        });
        // Esc, or a click outside the dialog, cancels.
        if modal.should_close() {
            answer = Some(false);
        }
        if let Some(go) = answer {
            self.resolve_memory_warning(go);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::sysmem::{GIB, MIB};

    #[test]
    fn a_warning_only_when_the_action_would_reach_red() {
        let m = Some(MemSnapshot {
            total: 4 * GIB,
            available: 600 * MIB,
        });
        assert_eq!(warn_for(50 * MIB, m), None, "550 MB is still above 512 MB");
        assert_eq!(warn_for(100 * MIB, m), Some(500 * MIB));
        assert_eq!(warn_for(100 * MIB, None), None);
    }

    #[test]
    fn cancel_leaves_the_rack_alone_and_go_ahead_adds() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.sysmem = Box::new(crate::sysmem::FakeMemory(Some(MemSnapshot {
            total: 4 * GIB,
            available: 512 * MIB, // already red: any add warns
        })));
        app.refresh_memory(0.0);
        app.with_memory_check(1, MemAction::Add(MachineType::Fm));
        assert!(app.pending_warn.is_some());
        assert_eq!(app.doc.project.machines.len(), 6);
        app.resolve_memory_warning(false);
        assert!(app.pending_warn.is_none());
        assert_eq!(app.doc.project.machines.len(), 6);
        app.with_memory_check(1, MemAction::Add(MachineType::Fm));
        app.resolve_memory_warning(true);
        assert_eq!(app.doc.project.machines.len(), 7);
    }

    fn short_app() -> SidAcidApp {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        app.sysmem = Box::new(crate::sysmem::FakeMemory(Some(MemSnapshot {
            total: 4 * GIB,
            available: 512 * MIB,
        })));
        app
    }

    #[test]
    fn a_sample_load_waits_for_go_ahead() {
        use crate::test_rack::SAMPLER;
        let mut app = short_app();
        let wav = super::super::tests::test_wav(0.5);
        app.load_sample_bytes(SAMPLER, 0, "Stab", Some("wav"), &wav);
        assert!(app.pending_warn.is_some(), "decoded, then held");
        let filled =
            |app: &SidAcidApp| app.doc.project.sampler(SAMPLER).unwrap().slots[0].is_some();
        assert!(!filled(&app));
        app.resolve_memory_warning(true);
        assert!(filled(&app));
    }

    #[test]
    fn a_pad_load_waits_for_go_ahead() {
        let mut app = short_app();
        let uid = app.add_machine_now(MachineType::PadKit).unwrap();
        let wav = super::super::tests::test_wav(0.5);
        app.load_pad_bytes(uid, 3, "Hat", Some("wav"), &wav);
        assert!(app.pending_warn.is_some(), "decoded, then held");
        let n = |app: &SidAcidApp| app.doc.project.padkit(uid).unwrap().pads[3].layers.len();
        assert_eq!(n(&app), 0);
        app.resolve_memory_warning(true);
        assert_eq!(n(&app), 1);
    }

    #[test]
    fn a_duplicate_counts_the_samplers_audio() {
        use crate::test_rack::SAMPLER;
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let wav = super::super::tests::test_wav(2.0);
        app.load_sample_bytes(SAMPLER, 0, "Break", Some("wav"), &wav);
        let audio = app.doc.project.sample_bytes() as u64;
        assert!(audio > 0);
        let est = MachineType::Sampler.memory_estimate() as u64;
        // Just enough free that the estimate alone passes, the audio not.
        app.sysmem = Box::new(crate::sysmem::FakeMemory(Some(MemSnapshot {
            total: 4 * GIB,
            available: 512 * MIB + est + audio / 2,
        })));
        assert!(app.duplicate_machine(SAMPLER).is_none());
        assert!(app.pending_warn.is_some());
        app.resolve_memory_warning(false);
        assert_eq!(app.doc.project.machines.len(), 6);
    }

    #[test]
    fn a_duplicate_counts_the_pad_kits_audio() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let uid = app.add_machine(MachineType::PadKit).unwrap();
        let wav = super::super::tests::test_wav(2.0);
        app.load_pad_bytes(uid, 0, "Kick", Some("wav"), &wav);
        let audio = app.doc.project.sample_bytes() as u64;
        assert!(audio > 0);
        let est = MachineType::PadKit.memory_estimate() as u64;
        // Just enough free that the estimate alone passes, the audio not.
        app.sysmem = Box::new(crate::sysmem::FakeMemory(Some(MemSnapshot {
            total: 4 * GIB,
            available: 512 * MIB + est + audio / 2,
        })));
        assert!(app.duplicate_machine(uid).is_none());
        assert!(app.pending_warn.is_some());
        app.resolve_memory_warning(false);
        assert_eq!(app.doc.project.machines.len(), 7);
    }

    #[test]
    fn the_window_focuses_cancel_and_esc_cancels() {
        let mut app = short_app();
        let ctx = egui::Context::default();
        app.with_memory_check(1, MemAction::Add(MachineType::Fm));
        let run = |app: &mut SidAcidApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.ui(ctx));
        };
        run(&mut app, vec![]);
        assert!(
            ctx.memory(|m| m.focused()).is_some(),
            "Cancel has the focus"
        );
        // Space presses the focused Cancel rather than starting playback.
        let key = |key, pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        run(
            &mut app,
            vec![key(egui::Key::Space, true), key(egui::Key::Space, false)],
        );
        run(&mut app, vec![]);
        assert!(app.pending_warn.is_none(), "Cancel was pressed");
        assert!(
            !app.pending
                .iter()
                .chain(&app.flushed)
                .any(|c| matches!(c, acid_engine::host::Command::Play))
        );
        assert_eq!(app.doc.project.machines.len(), 6);
        app.with_memory_check(1, MemAction::Add(MachineType::Fm));
        run(&mut app, vec![]);
        run(&mut app, vec![key(egui::Key::Escape, true)]);
        assert!(app.pending_warn.is_none());
        assert_eq!(app.doc.project.machines.len(), 6);
    }

    #[test]
    fn while_the_warning_is_held_the_rest_of_the_ui_gets_no_clicks() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let ctx = egui::Context::default();
        let run = |app: &mut SidAcidApp, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| app.ui(ctx));
        };
        run(&mut app, vec![]);
        // A point on the app's own UI: wherever a widget reports hover.
        app.sysmem = Box::new(crate::sysmem::FakeMemory(Some(MemSnapshot {
            total: 4 * GIB,
            available: 512 * MIB,
        })));
        app.with_memory_check(1, MemAction::Add(MachineType::Fm));
        run(&mut app, vec![]);
        let top = ctx.memory(|m| m.top_modal_layer());
        assert!(top.is_some(), "the warning is a modal layer");
        // Every layer below the modal is blocked from input.
        let below = ctx.layer_id_at(egui::pos2(5.0, 5.0));
        assert_eq!(below, top, "the modal backdrop covers the whole screen");
    }

    #[test]
    fn a_new_document_drops_a_held_action() {
        let mut app = short_app();
        app.with_memory_check(1, MemAction::Add(MachineType::Fm));
        assert!(app.pending_warn.is_some());
        app.load_document(Document::new(Project::default()));
        assert!(
            app.pending_warn.is_none(),
            "it belonged to the old document"
        );
        app.resolve_memory_warning(true);
        assert_eq!(app.doc.project.machines.len(), 6);
    }
}
