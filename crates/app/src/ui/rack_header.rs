//! Machine panel headers with the rack controls (move, duplicate, remove,
//! rename by double-click), and the Add bar below the last machine.
//!
//! Clicks are queued in `ViewState::rack_action` and applied once the
//! whole rack has been drawn (`rack_action_after_draw`): the rack must not
//! change while its panels are being drawn from it.

use super::{SidAcidApp, theme};
use acid_engine::machine::{MachineType, MachineUid};
use eframe::egui::{self, collapsing_header::CollapsingState};

/// Tooltip of Add and Duplicate when every machine id has been used.
const NO_FREE_IDS: &str = "the rack has no free machine ids";

// Button glyphs. The spec's ↑ ↓ ⧉ ✕ ▾ are not in egui's built-in fonts
// (they draw as empty boxes), so the nearest glyphs that are stand in.
const UP: &str = "⬆";
const DOWN: &str = "⬇";
const DUPLICATE: &str = "🗐";
const REMOVE: &str = "🗙";
const ADD: &str = "+ Add machine ⏷";

/// A rack control clicked this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RackAction {
    Up(MachineUid),
    Down(MachineUid),
    Duplicate(MachineUid),
    Remove(MachineUid),
    Add(MachineType),
}

/// The id of panel `(salt, uid)`'s open/closed state, made in the panel's
/// own vertical `Ui`. It is the id `CollapsingHeader::id_salt((salt, uid))`
/// gave the same panel (that header also wraps itself in `ui.vertical`), so
/// panels keep the open state they had before the rack controls.
fn header_id(ui: &egui::Ui, salt: &'static str, uid: MachineUid) -> egui::Id {
    ui.make_persistent_id(egui::Id::new((salt, uid)))
}

impl SidAcidApp {
    /// A fresh machine id is left.
    pub(crate) fn can_add(&self) -> bool {
        self.doc.project.next_uid < u16::MAX
    }

    /// `uid` is in the rack and not first.
    pub(crate) fn can_move_up(&self, uid: MachineUid) -> bool {
        self.doc.project.position(uid).is_some_and(|i| i > 0)
    }

    /// `uid` is in the rack and not last.
    pub(crate) fn can_move_down(&self, uid: MachineUid) -> bool {
        let n = self.doc.project.machines.len();
        self.doc.project.position(uid).is_some_and(|i| i + 1 < n)
    }

    /// The Add bar's count, "1 machine" or "n machines".
    pub(crate) fn rack_count_text(&self) -> String {
        match self.doc.project.machines.len() {
            1 => "1 machine".into(),
            n => format!("{n} machines"),
        }
    }

    /// Starts editing `uid`'s name, from its current name.
    pub(crate) fn begin_rename(&mut self, uid: MachineUid) {
        self.view.renaming = self.doc.project.machine(uid).map(|m| (uid, m.name.clone()));
    }

    /// Ends the rename with the typed name (trimmed; empty gives the
    /// type's default name). An unchanged name is no edit.
    pub(crate) fn commit_rename(&mut self) {
        let Some((uid, name)) = self.view.renaming.take() else {
            return;
        };
        let name = name.trim();
        if self
            .doc
            .project
            .machine(uid)
            .is_some_and(|m| m.name != name)
        {
            self.rename_machine(uid, name);
        }
    }

    /// Ends the rename, keeping the name.
    pub(crate) fn cancel_rename(&mut self) {
        self.view.renaming = None;
    }

    /// Applies the rack control clicked while the rack was drawn.
    pub(crate) fn rack_action_after_draw(&mut self) {
        let Some(action) = self.view.rack_action.take() else {
            return;
        };
        let to = |app: &Self, uid, step: isize| {
            app.doc
                .project
                .position(uid)
                .map(|i| i.saturating_add_signed(step))
        };
        match action {
            RackAction::Up(uid) => {
                if let Some(i) = to(self, uid, -1).filter(|_| self.can_move_up(uid)) {
                    self.move_machine(uid, i);
                }
            }
            RackAction::Down(uid) => {
                if let Some(i) = to(self, uid, 1).filter(|_| self.can_move_down(uid)) {
                    self.move_machine(uid, i);
                }
            }
            RackAction::Duplicate(uid) => {
                self.duplicate_machine(uid);
            }
            RackAction::Remove(uid) => self.remove_machine(uid),
            RackAction::Add(ty) => {
                self.add_machine(ty);
            }
        }
    }

    /// A machine's panel: a collapsing header showing `title`, the rack
    /// buttons and double-click rename, over `body`. The open state is
    /// kept under the id the panel's `CollapsingHeader` used (`header_id`).
    pub(crate) fn machine_panel(
        &mut self,
        ui: &mut egui::Ui,
        uid: MachineUid,
        salt: &'static str,
        title: String,
        body: impl FnOnce(&mut SidAcidApp, &mut egui::Ui),
    ) {
        ui.vertical(|ui| {
            let id = header_id(ui, salt, uid);
            let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, true);
            if let Some(open) = self.view.forced_open(salt) {
                state.set_open(open);
            }
            state
                .show_header(ui, |ui| self.panel_header(ui, uid, &title))
                .body(|ui| body(self, ui));
        });
    }

    /// The header row after the collapse arrow: the title (or the rename
    /// box), then the rack buttons at the right.
    fn panel_header(&mut self, ui: &mut egui::Ui, uid: MachineUid, title: &str) {
        let renaming = self.view.renaming.as_ref().is_some_and(|(u, _)| *u == uid);
        if renaming {
            self.rename_box(ui, uid, title);
        } else {
            // Only the arrow toggles the panel: a double-click here renames
            // and must leave the panel as it was. The focused machine's
            // title is lit, like its lane's outline in the dock.
            let mut text = egui::RichText::new(title).strong();
            if self.focused() == Some(uid) {
                text = text.color(theme::ACID);
            }
            let r = ui
                .add(
                    egui::Label::new(text)
                        .sense(egui::Sense::click())
                        .selectable(false),
                )
                .on_hover_text("Double-click to rename");
            if r.double_clicked() {
                self.begin_rename(uid);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut clicked = None;
            let can_add = self.can_add();
            if ui.button(REMOVE).on_hover_text("Remove").clicked() {
                clicked = Some(RackAction::Remove(uid));
            }
            let dup = ui.add_enabled(can_add, egui::Button::new(DUPLICATE));
            if dup
                .on_hover_text("Duplicate")
                .on_disabled_hover_text(NO_FREE_IDS)
                .clicked()
            {
                clicked = Some(RackAction::Duplicate(uid));
            }
            let down = ui.add_enabled(self.can_move_down(uid), egui::Button::new(DOWN));
            if down
                .on_hover_text("Move down")
                .on_disabled_hover_text("Move down")
                .clicked()
            {
                clicked = Some(RackAction::Down(uid));
            }
            let up = ui.add_enabled(self.can_move_up(uid), egui::Button::new(UP));
            if up
                .on_hover_text("Move up")
                .on_disabled_hover_text("Move up")
                .clicked()
            {
                clicked = Some(RackAction::Up(uid));
            }
            if clicked.is_some() {
                self.view.rack_action = clicked;
            }
            if let Some(share) = self.view.perf.machine_cpu(uid) {
                let normal = ui.visuals().text_color();
                ui.label(
                    egui::RichText::new(super::perf::cpu_label(share))
                        .color(super::perf::cpu_colour(share, normal)),
                )
                .on_hover_text("Share of the audio time this machine takes");
            }
        });
    }

    /// The inline name editor: Enter commits; Escape or clicking elsewhere
    /// cancels. The rest of the title stays beside it.
    fn rename_box(&mut self, ui: &mut egui::Ui, uid: MachineUid, title: &str) {
        let shown_name = self.panel_title(uid).to_uppercase();
        let rest = title.strip_prefix(&shown_name).unwrap_or_default();
        let Some((_, text)) = self.view.renaming.as_mut() else {
            return;
        };
        let r = ui.add(
            egui::TextEdit::singleline(text)
                .id_salt(("rename", uid))
                .desired_width(140.0),
        );
        if r.lost_focus() {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.commit_rename();
            } else {
                self.cancel_rename();
            }
        } else if !r.has_focus() {
            // The first frame of the rename.
            r.request_focus();
        }
        ui.label(egui::RichText::new(rest).strong());
    }

    /// Below the last machine: the Add menu and the machine count.
    pub(crate) fn add_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let can_add = self.can_add();
            let menu = ui.add_enabled_ui(can_add, |ui| {
                ui.menu_button(ADD, |ui| {
                    for ty in MachineType::ALL {
                        if ui.button(ty.label()).clicked() {
                            self.view.rack_action = Some(RackAction::Add(ty));
                            ui.close_menu();
                        }
                    }
                })
                .response
            });
            menu.inner.on_disabled_hover_text(NO_FREE_IDS);
            ui.label(self.rack_count_text());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::test_rack::*;
    use crate::ui::run_frames;
    use acid_engine::machine::{MachineType, MachineUid};
    use acid_engine::project::Project;
    use eframe::egui;

    fn demo() -> SidAcidApp {
        SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()))
    }

    #[test]
    fn rack_buttons_follow_the_rules() {
        let app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let first = app.doc.project.machines[0].uid;
        let last = app.doc.project.machines.last().unwrap().uid;
        assert!(!app.can_move_up(first) && app.can_move_down(first));
        assert!(app.can_move_up(last) && !app.can_move_down(last));
        assert!(app.can_add());
    }

    #[test]
    fn renaming_commits_on_enter_and_restores_the_default_when_empty() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.begin_rename(MachineUid(3));
        app.view.renaming.as_mut().unwrap().1 = "Boom".into();
        app.commit_rename();
        assert_eq!(app.panel_title(MachineUid(3)), "Boom");
        app.begin_rename(MachineUid(3));
        app.view.renaming.as_mut().unwrap().1 = String::new();
        app.commit_rename();
        assert_eq!(app.panel_title(MachineUid(3)), "Kit A");
        app.begin_rename(MachineUid(3));
        app.cancel_rename();
        assert!(app.view.renaming.is_none());
    }

    #[test]
    fn headers_keep_todays_text_and_the_rack_draws_with_controls() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        run_frames(&mut app, 2); // draws every panel through machine_panel without panicking
        app.add_machine(MachineType::Kit909);
        run_frames(&mut app, 2);
        assert_eq!(
            app.panel_title(*app.doc.project.rack_uids().last().unwrap()),
            "Kit B 2"
        );
    }

    #[test]
    fn a_rename_starts_from_the_name_and_a_cancel_changes_nothing() {
        let mut app = demo();
        app.begin_rename(KIT808);
        assert_eq!(app.view.renaming, Some((KIT808, "Kit A".to_string())));
        app.view.renaming.as_mut().unwrap().1 = "Boom".into();
        app.cancel_rename();
        assert_eq!(app.panel_title(KIT808), "Kit A");
        assert!(!app.doc.can_undo(), "a cancelled rename is no edit");
        app.begin_rename(MachineUid(99));
        assert!(
            app.view.renaming.is_none(),
            "no rename of a missing machine"
        );
    }

    #[test]
    fn the_rename_box_keeps_focus_then_enter_commits_and_escape_cancels() {
        let key = |key| {
            let mut input = egui::RawInput::default();
            input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            input
        };
        let ctx = egui::Context::default();
        let mut app = demo();
        app.begin_rename(KIT808);
        crate::ui::run_frames_in(&ctx, &mut app, 3);
        assert!(app.view.renaming.is_some(), "the box holds its focus");
        app.view.renaming.as_mut().unwrap().1 = "Boom".into();
        let _ = ctx.run(key(egui::Key::Enter), |ctx| app.ui(ctx));
        assert_eq!(app.panel_title(KIT808), "Boom");
        assert!(app.view.renaming.is_none());
        app.begin_rename(KIT808);
        crate::ui::run_frames_in(&ctx, &mut app, 2);
        app.view.renaming.as_mut().unwrap().1 = "Bang".into();
        let _ = ctx.run(key(egui::Key::Escape), |ctx| app.ui(ctx));
        assert!(app.view.renaming.is_none());
        assert_eq!(app.panel_title(KIT808), "Boom");
    }

    #[test]
    fn a_mono_machine_can_be_added_and_drawn() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        assert!(app.add_machine(MachineType::Mono).is_some());
        let m = app
            .doc
            .project
            .machines
            .iter()
            .find(|m| m.machine_type() == MachineType::Mono)
            .unwrap();
        assert_eq!(m.name, "Mono");
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.ui(ctx));
    }

    #[test]
    fn the_add_menu_stays_open_past_sixteen() {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        for _ in 0..20 {
            assert!(app.can_add());
            app.add_machine(MachineType::Fm).unwrap();
        }
        assert_eq!(app.doc.project.machines.len(), 26);
        assert_eq!(app.rack_count_text(), "26 machines");
        assert_eq!(app.engine_capacity, 32);
    }

    #[test]
    fn rack_actions_apply_after_the_panels_are_drawn() {
        let mut app = demo();
        app.view.rack_action = Some(RackAction::Down(SYNTH1));
        run_frames(&mut app, 1);
        assert_eq!(app.doc.project.machines[1].uid, SYNTH1);
        assert!(app.view.rack_action.is_none());
        app.view.rack_action = Some(RackAction::Up(SYNTH1));
        run_frames(&mut app, 1);
        assert_eq!(app.doc.project.machines[0].uid, SYNTH1);
        app.view.rack_action = Some(RackAction::Remove(KIT808));
        run_frames(&mut app, 1);
        assert!(app.doc.project.machine(KIT808).is_none());
        assert_eq!(
            app.view.status_line.as_deref(),
            Some("Removed 'Kit A' — Ctrl+Z to undo")
        );
        app.view.rack_action = Some(RackAction::Duplicate(FM));
        run_frames(&mut app, 1);
        let i = app.doc.project.position(FM).unwrap();
        let copy = app.doc.project.machines[i + 1].uid;
        assert_eq!(app.panel_title(copy), "FM copy");
        assert_eq!(app.view.focus, Some(copy), "focus moves to the copy");
        app.view.rack_action = Some(RackAction::Add(MachineType::Sampler));
        run_frames(&mut app, 2);
        let added = *app.doc.project.rack_uids().last().unwrap();
        assert_eq!(
            app.view.focus,
            Some(added),
            "focus moves to the new machine"
        );
    }

    #[test]
    fn the_rack_button_glyphs_are_in_the_font() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let font = egui::TextStyle::Button.resolve(&ctx.style());
        for c in [UP, DOWN, DUPLICATE, REMOVE, ADD].concat().chars() {
            assert!(ctx.fonts(|f| f.has_glyph(&font, c)), "{c} is missing");
        }
    }

    #[test]
    fn the_add_menu_lists_every_type_in_order() {
        let labels: Vec<&str> = MachineType::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(
            labels,
            [
                "Acid Synth",
                "Kit A",
                "Kit B",
                "FM",
                "Sampler",
                "Mono",
                "Pad Kit"
            ]
        );
    }

    #[test]
    fn adding_a_pad_kit_from_the_menu_draws_the_rack() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.view.rack_action = Some(RackAction::Add(MachineType::PadKit));
        run_frames(&mut app, 2);
        let added = *app.doc.project.rack_uids().last().unwrap();
        assert_eq!(
            app.doc.project.machine(added).unwrap().machine_type(),
            MachineType::PadKit
        );
        run_frames(&mut app, 1);
    }

    /// The panel's open state lives where its `CollapsingHeader` kept it.
    #[test]
    fn a_panel_header_keeps_the_collapsing_header_id() {
        let ctx = egui::Context::default();
        let mut ids = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for (salt, uid) in [("synth", SYNTH1), ("drums", KIT909), ("fm", FM)] {
                    let old = egui::CollapsingHeader::new("x")
                        .id_salt((salt, uid))
                        .show(ui, |_| {})
                        .header_response
                        .id;
                    let new = ui.vertical(|ui| header_id(ui, salt, uid)).inner;
                    ids.push((old, new));
                }
            });
        });
        for (old, new) in ids {
            assert_eq!(old, new);
        }
    }

    #[test]
    fn a_shut_panel_stays_shut_once_nothing_forces_it() {
        let ctx = egui::Context::default();
        let mut app = demo();
        let open = crate::ui::run_frames_in(&ctx, &mut app, 3);
        app.view.force_open.push(("drums", false));
        let shut = crate::ui::run_frames_in(&ctx, &mut app, 3);
        assert!(shut < open, "the kits' bodies are hidden");
        app.view.force_open.clear();
        let after = crate::ui::run_frames_in(&ctx, &mut app, 2);
        assert!(after <= shut, "the closed state is kept");
    }

    #[test]
    fn a_machine_back_from_undo_shows_the_bank_of_its_pattern() {
        let mut app = demo();
        app.doc.project.machines[2].selected = 20; // the 808, bank C
        app.remove_machine(KIT808);
        app.undo();
        assert_eq!(
            app.view.panel(KIT808).bank,
            20 / acid_engine::project::BANK_SIZE as u8
        );
        let copy = app.duplicate_machine(KIT808).unwrap();
        assert_eq!(
            app.view.panel(copy).bank,
            20 / acid_engine::project::BANK_SIZE as u8
        );
    }

    #[test]
    fn a_pasted_synth_pattern_drops_a_lane_for_a_machine_not_in_the_rack() {
        use acid_engine::params::{ParamId, SynthParam};
        use acid_engine::project::{ParamLane, STEPS};
        let mut app = demo();
        let lane = |param| {
            Some(ParamLane {
                param,
                values: vec![Some(0.5); STEPS],
            })
        };
        let pattern_with = |param| {
            let mut p = app.doc.project.synth(SYNTH1).unwrap().patterns[0].clone();
            p.param_lane = lane(param);
            p
        };
        let to_gone = pattern_with(SYNTH2.param(SynthParam::Cutoff));
        let misfit = pattern_with(ParamId::Machine(
            KIT808,
            acid_engine::params::MachineParam::Synth(SynthParam::Cutoff),
        ));
        let kept = pattern_with(SYNTH1.param(SynthParam::Cutoff));
        app.remove_machine(SYNTH2);
        app.view.focus = Some(SYNTH1);
        for (clip, keeps) in [(to_gone, false), (misfit, false), (kept, true)] {
            app.view.clipboard = Some(crate::ui::files::Clip::Synth(clip));
            app.paste_pattern();
            let slot = app.selected(SYNTH1) as usize;
            let lane = &app.doc.project.synth(SYNTH1).unwrap().patterns[slot].param_lane;
            assert_eq!(lane.is_some(), keeps);
        }
        assert!(app.doc.project.validate().is_ok());
    }

    #[test]
    fn a_pad_kit_is_named_pads_and_a_paste_onto_it_drops_notes_past_pad_16() {
        let mut app = demo();
        app.view.rack_action = Some(RackAction::Add(MachineType::PadKit));
        run_frames(&mut app, 2);
        let uid = *app.doc.project.rack_uids().last().unwrap();
        assert_eq!(app.doc.project.machine(uid).unwrap().name, "Pads");
        let mut roll = app.doc.project.padkit(uid).unwrap().patterns[0].clone();
        let note = |pitch| acid_engine::project::Note::new(0, 6, pitch, 100);
        roll.notes = vec![note(3), note(40)];
        app.view.focus = Some(uid);
        app.view.clipboard = Some(crate::ui::files::Clip::Notes(roll.clone()));
        app.paste_pattern();
        let slot = app.selected(uid) as usize;
        let pitches: Vec<u8> = app.doc.project.padkit(uid).unwrap().patterns[slot]
            .notes
            .iter()
            .map(|n| n.pitch)
            .collect();
        assert_eq!(pitches, [3]);
        assert!(app.doc.project.validate().is_ok());
        assert_eq!(
            app.view.message.as_deref(),
            Some("Left out 1 note above pad 16: a Pad Kit has 16 pads.")
        );
        // A paste that fits says nothing.
        roll.notes = vec![note(3), note(15)];
        app.view.clipboard = Some(crate::ui::files::Clip::Notes(roll));
        app.view.message = None;
        app.paste_pattern();
        assert!(app.view.message.is_none());
    }
}
