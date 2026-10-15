//! Loading Mono patches: from the starter bank or Randomise. A load is
//! one undoable edit.

use super::SidAcidApp;
use crate::document::Edit;
use acid_engine::machine::MachineUid;
use acid_engine::params::MonoParam;
pub(crate) use acid_engine::va::starter::STARTER;

impl SidAcidApp {
    pub(crate) fn load_mono_patch(
        &mut self,
        uid: MachineUid,
        name: &str,
        values: Vec<(MonoParam, f32)>,
    ) {
        let values = values.into_iter().map(|(p, v)| (uid.param(p), v)).collect();
        let cmds = self.doc.apply(
            Edit::MonoPatch {
                uid,
                name: name.into(),
                values,
            },
            false,
        );
        self.send(cmds);
        self.view.focus = Some(uid);
    }

    /// The Mono panel's Randomise: a new sound that always plays, as one
    /// undo step. Volume, arp, drift and chorus are kept.
    pub(crate) fn randomise_mono_patch(&mut self, uid: MachineUid) {
        let seed = self.next_seed();
        self.randomise_mono_patch_seeded(uid, seed);
    }

    pub(crate) fn randomise_mono_patch_seeded(&mut self, uid: MachineUid, seed: u64) {
        let project = &self.doc.project;
        if project.mono(uid).is_none() {
            return;
        }
        let values =
            acid_engine::random::randomise_mono(|p| project.param(uid.param(p)), seed);
        self.load_mono_patch(uid, "Random", values);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::machine::MachineType;
    use acid_engine::params::MonoParam;

    fn app_with_mono() -> (SidAcidApp, MachineUid) {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Mono);
        let uid = app
            .doc
            .project
            .machines
            .iter()
            .find(|m| m.machine_type() == MachineType::Mono)
            .unwrap()
            .uid;
        (app, uid)
    }

    #[test]
    fn loading_a_starter_patch_is_one_undo_step() {
        let (mut app, uid) = app_with_mono();
        let before = app.doc.project.clone();
        let preset = &STARTER[1];
        app.load_mono_patch(uid, preset.name, preset.to_params());
        assert_eq!(app.doc.project.mono(uid).unwrap().patch_name, preset.name);
        for (p, v) in preset.to_params() {
            assert!((app.doc.project.param(uid.param(p)) - v).abs() < 1e-6, "{p:?}");
        }
        app.doc.undo();
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn randomising_keeps_volume_and_the_sequence_and_undoes_in_one_step() {
        let (mut app, uid) = app_with_mono();
        let before = app.doc.project.clone();
        app.randomise_mono_patch_seeded(uid, 7);
        let p = &app.doc.project;
        assert_eq!(p.mono(uid).unwrap().patch_name, "Random");
        assert_eq!(
            p.param(uid.param(MonoParam::Volume)),
            before.param(uid.param(MonoParam::Volume))
        );
        assert_eq!(p.mono(uid).unwrap().patterns, before.mono(uid).unwrap().patterns);
        app.doc.undo();
        assert_eq!(app.doc.project, before);
    }
}
