//! The dock's Tools menu: runs a note tool (`note_tools`) on the focused
//! lane's selection, or on its whole visible pattern when nothing is
//! selected. Opening a tool snapshots the pattern and the scope; its popup
//! holds the tool's settings, and every frame the tool runs on that
//! snapshot (never on the last result) and the result is applied as a
//! preview outside the undo history (`Document::preview`), so the lane
//! shows it live. Apply keeps it as one undo step and selects the result;
//! Cancel (or closing the popup) puts the pattern back as if nothing
//! happened. While a tool is open nothing else edits that lane (see
//! `SidAcidApp::mouse_editing`).

use super::SidAcidApp;
use super::key_entry::LaneKind;
use super::lane::INST_NAMES;
use super::note_ops;
use super::note_tools::{self, Arp, ArpStyle, Euclid, Humanize, Quantize, Randomize, Strum};
use super::poly_edit::Snap;
use crate::document::Edit;
use acid_engine::machine::MachineUid;
use acid_engine::project::{Note, NotePattern};
use acid_engine::scale::{Key, ROOT_NAMES};
use acid_engine::sequencer::Mode;
use eframe::egui::{self, Align2, Rect, RichText};

/// The tools, in menu order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolKind {
    Quantize,
    Humanize,
    Legato,
    Reverse,
    Invert,
    Strum,
    Randomize,
    Arpeggiate,
    Euclid,
}

impl ToolKind {
    pub const ALL: [ToolKind; 9] = [
        ToolKind::Quantize,
        ToolKind::Humanize,
        ToolKind::Legato,
        ToolKind::Reverse,
        ToolKind::Invert,
        ToolKind::Strum,
        ToolKind::Randomize,
        ToolKind::Arpeggiate,
        ToolKind::Euclid,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToolKind::Quantize => "Quantize",
            ToolKind::Humanize => "Humanize",
            ToolKind::Legato => "Legato",
            ToolKind::Reverse => "Reverse",
            ToolKind::Invert => "Invert",
            ToolKind::Strum => "Strum",
            ToolKind::Randomize => "Randomize in scale",
            ToolKind::Arpeggiate => "Arpeggiate",
            ToolKind::Euclid => "Euclidean",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            ToolKind::Quantize => "Move notes toward the grid",
            ToolKind::Humanize => "Small random shifts in timing and velocity",
            ToolKind::Legato => "Each note runs to the next one",
            ToolKind::Reverse => "Play the notes backwards",
            ToolKind::Invert => "Mirror the pitches within their range",
            ToolKind::Strum => "Stagger the notes of each chord",
            ToolKind::Randomize => "Replace the notes with random ones in the key",
            ToolKind::Arpeggiate => "Turn each chord into a run of its notes",
            ToolKind::Euclid => "Spread hits as evenly as they go",
        }
    }

    /// Why the tool is greyed on a lane of `kind`, or None when it works
    /// there: Invert needs pitches (not drums), Strum and Arpeggiate need
    /// chords (a lane playing several notes at once).
    pub fn unavailable(self, kind: LaneKind) -> Option<&'static str> {
        match self {
            ToolKind::Invert if kind == LaneKind::Drums => Some("Melodic lanes only"),
            ToolKind::Strum | ToolKind::Arpeggiate if kind != LaneKind::Poly => {
                Some("Only on lanes that play chords")
            }
            _ => None,
        }
    }
}

/// Every tool's settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolParams {
    pub quantize: Quantize,
    pub humanize: Humanize,
    pub strum: Strum,
    pub randomize: Randomize,
    pub arp: Arp,
    pub euclid: Euclid,
}

impl ToolParams {
    /// `tool` with these settings on `p`'s `scope` (indices): the new
    /// pattern and the scope notes' (or new notes') indices in it. With
    /// `whole` (nothing was selected) the generators fill the whole
    /// pattern rather than the scope's span.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &self,
        tool: ToolKind,
        p: &NotePattern,
        scope: &[usize],
        whole: bool,
        kind: LaneKind,
        key: Key,
        snap: Snap,
    ) -> (NotePattern, Vec<usize>) {
        use note_tools::*;
        match tool {
            ToolKind::Quantize => quantize(p, scope, &self.quantize, kind),
            ToolKind::Humanize => humanize(p, scope, &self.humanize, kind),
            ToolKind::Legato => legato(p, scope, kind),
            ToolKind::Reverse => reverse(p, scope, kind),
            ToolKind::Invert => invert(p, scope, kind, key),
            ToolKind::Strum => strum(p, scope, &self.strum, kind),
            ToolKind::Randomize => randomize(p, scope, whole, &self.randomize, kind, key, snap),
            ToolKind::Arpeggiate => arpeggiate(p, scope, &self.arp, kind),
            ToolKind::Euclid => euclid(p, scope, whole, &self.euclid, kind, snap),
        }
    }
}

/// An open tool on one lane's pattern.
pub(crate) struct ToolSession {
    pub uid: MachineUid,
    pub slot: u8,
    /// The pattern when the tool opened: every preview starts from it.
    pub origin: NotePattern,
    /// The notes the tool acts on (indices into `origin`).
    pub scope: Vec<usize>,
    /// Nothing was selected: the scope is every visible note and the
    /// generators fill the whole pattern.
    pub whole: bool,
    pub tool: ToolKind,
    pub params: ToolParams,
    /// The pattern as the session last left it (`origin` until a preview
    /// is applied): anything else there means another edit changed it.
    last: NotePattern,
    /// The result's selection (indices into `last`).
    result_sel: Vec<usize>,
    /// The lane's selection when the tool opened, given back by Cancel.
    sel_before: (Vec<Note>, u8),
}

/// The tool popup's window (and so its layer) id.
pub(crate) fn tool_window_id() -> egui::Id {
    egui::Id::new("note_tool")
}

/// Seeds the popup hands out: short enough to read and type back.
const SEEDS: u64 = 100_000;

/// A MIDI pitch's name, C2 = 36.
fn pitch_name(p: u8) -> String {
    format!("{}{}", ROOT_NAMES[p as usize % 12], p as i32 / 12 - 1)
}

impl SidAcidApp {
    /// The Tools menu button (first in the Pattern-mode inspector row,
    /// with or without a selection) and the open tool's popup.
    pub(crate) fn tools_menu(&mut self, ui: &mut egui::Ui) {
        let Some(uid) = self.focused() else {
            ui.add_enabled(false, egui::Button::new("Tools"));
            return;
        };
        let kind = self.lane_kind(uid);
        let mut rects: Vec<(String, Rect)> = Vec::new();
        let mut picked = None;
        let menu = ui.menu_button("Tools", |ui| {
            for t in ToolKind::ALL {
                let off = t.unavailable(kind);
                let item = ui
                    .add_enabled(off.is_none(), egui::Button::new(t.label()))
                    .on_hover_text(t.hint())
                    .on_disabled_hover_text(off.unwrap_or_default());
                rects.push((format!("Tool {}", t.label()), item.rect));
                if item.clicked() {
                    picked = Some(t);
                    ui.close_menu();
                }
            }
        });
        let anchor = menu
            .response
            .on_hover_text("Run a tool on the selection, or the whole pattern")
            .rect;
        rects.push(("Tools".into(), anchor));
        if let Some(t) = picked {
            self.open_tool(t);
        }
        self.tool_window(ui.ctx(), anchor, &mut rects);
        #[cfg(test)]
        self.view.dock.tool_rects.extend(rects);
        #[cfg(not(test))]
        drop(rects);
    }

    /// Opens `tool` on the focused lane: a snapshot of its pattern and of
    /// the scope (the selection, else every visible note). Any open tool
    /// is cancelled first.
    pub(crate) fn open_tool(&mut self, tool: ToolKind) {
        self.cancel_tool();
        let Some(uid) = self.focused() else {
            return;
        };
        let slot = self.selected(uid);
        let Some(origin) = self.doc.project.roll(uid, slot).cloned() else {
            return;
        };
        if self.view.mode != Mode::Pattern {
            return;
        }
        let sel = self.selection(uid);
        let whole = sel.is_empty();
        let scope = if sel.is_empty() {
            note_tools::default_scope(&origin)
        } else {
            sel
        };
        let kind = self.lane_kind(uid);
        let mut params = self.view.tool_params;
        // Euclidean starts on the cursor's pitch or drum row.
        let pitch = self.view.panel(uid).cursor.pitch;
        params.euclid.pitch = if kind == LaneKind::Drums {
            pitch.min(INST_NAMES.len() as u8 - 1)
        } else {
            pitch.min(127)
        };
        let panel = self.view.panel(uid);
        let sel_before = (panel.selected.clone(), panel.sel_slot);
        // The preview's edits must not merge into an earlier gesture.
        self.doc.end_gesture();
        self.view.focus = Some(uid);
        self.view.tool = Some(ToolSession {
            uid,
            slot,
            last: origin.clone(),
            result_sel: scope.clone(),
            origin,
            scope,
            whole,
            tool,
            params,
            sel_before,
        });
        self.preview_tool();
    }

    /// Once a frame, ahead of the dock: a tool whose pattern was changed
    /// by something that didn't cancel it first (rack edits do, see
    /// `tool_yield`) is closed, leaving the pattern as it now is (its
    /// preview becomes an undo step under that edit); one whose lane,
    /// pattern slot or mode is no longer the one it opened on is cancelled.
    pub(crate) fn check_tool(&mut self) {
        let Some(t) = &self.view.tool else {
            return;
        };
        let (uid, slot) = (t.uid, t.slot);
        if self.doc.project.roll(uid, slot) != Some(&t.last) {
            self.view.tool = None;
            self.doc.settle_preview();
            self.doc.end_gesture();
            return;
        }
        if self.view.mode != Mode::Pattern
            || self.focused() != Some(uid)
            || self.selected(uid) != slot
        {
            self.cancel_tool();
        }
    }

    /// Ahead of a rack edit to `uid`'s pattern `slot` (dice, a template,
    /// its length or transpose, a pattern paste): a tool open on it is
    /// cancelled first, so the edit lands on the pattern as it was.
    pub(crate) fn tool_yield(&mut self, uid: MachineUid, slot: u8) {
        if self.tool_lane() == Some((uid, slot)) {
            self.cancel_tool();
        }
    }

    /// The open tool's lane: edits to it are held off.
    pub(crate) fn tool_lane(&self) -> Option<(MachineUid, u8)> {
        self.view.tool.as_ref().map(|t| (t.uid, t.slot))
    }

    /// Runs the open tool on its snapshot with its current settings and
    /// previews the result (outside the undo history) when it differs
    /// from what the lane holds; the result is selected.
    fn preview_tool(&mut self) {
        let key = self.doc.project.key;
        let snap = self.view.dock.snap;
        let Some(t) = &self.view.tool else {
            return;
        };
        let kind = self.lane_kind(t.uid);
        let (result, sel) = t
            .params
            .run(t.tool, &t.origin, &t.scope, t.whole, kind, key, snap);
        let (uid, slot) = (t.uid, t.slot);
        if result != t.last {
            let cmds = self.doc.preview(Edit::NotePattern {
                uid,
                slot,
                pattern: Box::new(result.clone()),
            });
            self.send(cmds);
            self.view.focus = Some(uid);
            if let Some(t) = &mut self.view.tool {
                t.last = result.clone();
            }
        }
        if let Some(t) = &mut self.view.tool {
            t.result_sel = sel.clone();
        }
        self.set_selection(uid, note_ops::values(&result, &sel));
    }

    /// Apply: the result stays as exactly one undo step, from the
    /// pattern as the tool found it to the result, on top of any other
    /// edit made while it was open (none when the result is the pattern
    /// as it was), and is selected.
    pub(crate) fn apply_tool(&mut self) {
        let Some(t) = self.view.tool.take() else {
            return;
        };
        self.view.tool_params = t.params;
        if t.last == t.origin {
            // Previews that came back to where they started.
            let cmds = self.doc.discard_preview();
            self.send(cmds);
        } else {
            self.doc.commit_preview();
        }
        self.doc.end_gesture();
        if self.view.mode == Mode::Pattern && self.selected(t.uid) == t.slot {
            self.set_selection(t.uid, note_ops::values(&t.last, &t.result_sel));
        }
    }

    /// Cancel: the pattern as it was and the old selection, with the undo
    /// history exactly as before the tool opened (plus any other edit made
    /// meanwhile).
    pub(crate) fn cancel_tool(&mut self) {
        let Some(t) = self.view.tool.take() else {
            return;
        };
        self.view.tool_params = t.params;
        let cmds = self.doc.discard_preview();
        self.send(cmds);
        self.doc.end_gesture();
        if self.view.mode == Mode::Pattern {
            let panel = self.view.panel(t.uid);
            (panel.selected, panel.sel_slot) = t.sel_before;
        }
    }

    /// The open tool's popup: its settings, Apply and Cancel. Closing it
    /// cancels. The preview follows the settings in the same frame.
    fn tool_window(&mut self, ctx: &egui::Context, anchor: Rect, rects: &mut Vec<(String, Rect)>) {
        let Some(t) = &self.view.tool else {
            return;
        };
        let (tool, mut params) = (t.tool, t.params);
        let kind = self.lane_kind(t.uid);
        let snap = self.view.dock.snap;
        let mut open = true;
        let (mut apply, mut cancel, mut new_seed) = (false, false, false);
        egui::Window::new(tool.label())
            .id(tool_window_id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(Align2::LEFT_BOTTOM)
            .default_pos(anchor.left_top())
            .show(ctx, |ui| {
                settings(ui, tool, &mut params, kind, snap, &mut new_seed, rects);
                ui.separator();
                ui.horizontal(|ui| {
                    let a = ui.button("Apply");
                    let c = ui.button("Cancel");
                    rects.push(("Apply".into(), a.rect));
                    rects.push(("Cancel".into(), c.rect));
                    apply = a.clicked();
                    cancel = c.clicked();
                });
            });
        if new_seed {
            let seed = self.next_seed() % SEEDS;
            match tool {
                ToolKind::Humanize => params.humanize.seed = seed,
                ToolKind::Randomize => params.randomize.seed = seed,
                ToolKind::Arpeggiate => params.arp.seed = seed,
                _ => {}
            }
        }
        if let Some(t) = &mut self.view.tool {
            t.params = params;
        }
        if cancel || !open {
            return self.cancel_tool();
        }
        self.preview_tool();
        if apply {
            self.apply_tool();
        }
    }
}

/// A labelled number in `range` with `suffix`; returns where its value
/// sits.
fn number(
    ui: &mut egui::Ui,
    name: &str,
    v: &mut u8,
    range: std::ops::RangeInclusive<u8>,
    suffix: &str,
) -> Rect {
    ui.horizontal(|ui| {
        ui.label(name);
        ui.add(
            egui::DragValue::new(v)
                .range(range)
                .speed(0.2)
                .suffix(suffix),
        )
        .rect
    })
    .inner
}

/// A labelled pitch, shown by name.
fn pitch(ui: &mut egui::Ui, name: &str, v: &mut u8) {
    ui.horizontal(|ui| {
        ui.label(name);
        ui.add(
            egui::DragValue::new(v)
                .range(0..=127)
                .speed(0.3)
                .custom_formatter(|p, _| pitch_name(p as u8)),
        );
    });
}

/// One choice per snap.
fn snaps(ui: &mut egui::Ui, name: &str, v: &mut Snap) {
    ui.horizontal(|ui| {
        ui.label(name);
        for s in Snap::ALL {
            if ui.selectable_label(*v == s, s.label()).clicked() {
                *v = s;
            }
        }
    });
}

/// The seed (typed or dragged) and a "New seed" button.
fn seed(ui: &mut egui::Ui, v: &mut u64, new_seed: &mut bool, rects: &mut Vec<(String, Rect)>) {
    ui.horizontal(|ui| {
        ui.label("Seed");
        ui.add(egui::DragValue::new(v).range(0..=SEEDS - 1).speed(1.0));
        let b = ui
            .button("New seed")
            .on_hover_text("Roll again: the same seed gives the same notes");
        rects.push(("New seed".into(), b.rect));
        *new_seed |= b.clicked();
    });
}

/// `tool`'s settings (the plan's table).
fn settings(
    ui: &mut egui::Ui,
    tool: ToolKind,
    p: &mut ToolParams,
    kind: LaneKind,
    snap: Snap,
    new_seed: &mut bool,
    rects: &mut Vec<(String, Rect)>,
) {
    let drums = kind == LaneKind::Drums;
    let note = |ui: &mut egui::Ui, text: &str| ui.label(RichText::new(text).weak());
    match tool {
        ToolKind::Quantize => {
            let q = &mut p.quantize;
            snaps(ui, "Grid", &mut q.grid);
            number(ui, "Strength", &mut q.strength, 0..=100, "%");
            ui.checkbox(&mut q.ends, "Quantize ends");
        }
        ToolKind::Humanize => {
            let h = &mut p.humanize;
            let timing = number(ui, "Timing ±", &mut h.timing, 0..=8, " ticks");
            rects.push(("Timing".into(), timing));
            number(ui, "Velocity ±", &mut h.vel, 0..=40, "");
            seed(ui, &mut h.seed, new_seed, rects);
        }
        ToolKind::Legato => {
            note(
                ui,
                "Each note runs to the next note's start; the last to the pattern end.",
            );
        }
        ToolKind::Reverse => {
            note(ui, "The notes play backwards over their span.");
        }
        ToolKind::Invert => {
            note(ui, "Pitches mirror within the notes' range.");
        }
        ToolKind::Strum => {
            let s = &mut p.strum;
            number(ui, "Ticks apart", &mut s.ticks, 1..=8, "");
            ui.horizontal(|ui| {
                ui.label("Order");
                if ui.selectable_label(s.up, "Up").clicked() {
                    s.up = true;
                }
                if ui.selectable_label(!s.up, "Down").clicked() {
                    s.up = false;
                }
            });
        }
        ToolKind::Randomize => {
            let r = &mut p.randomize;
            number(ui, "Density", &mut r.density, 10..=100, "%");
            // Drum hits are one cell each: no pitch range, no lengths.
            if !drums {
                pitch(ui, "Lowest", &mut r.pitch_lo);
                pitch(ui, "Highest", &mut r.pitch_hi);
                let unit = format!(" × {}", snap.label());
                rects.push((
                    "Shortest".into(),
                    number(ui, "Shortest", &mut r.len_min, 1..=16, &unit),
                ));
                rects.push((
                    "Longest".into(),
                    number(ui, "Longest", &mut r.len_max, 1..=16, &unit),
                ));
            }
            seed(ui, &mut r.seed, new_seed, rects);
        }
        ToolKind::Arpeggiate => {
            let a = &mut p.arp;
            ui.horizontal(|ui| {
                ui.label("Style");
                for (style, name) in [
                    (ArpStyle::Up, "Up"),
                    (ArpStyle::Down, "Down"),
                    (ArpStyle::UpDown, "Up-down"),
                    (ArpStyle::Random, "Random"),
                ] {
                    if ui.selectable_label(a.style == style, name).clicked() {
                        a.style = style;
                    }
                }
            });
            snaps(ui, "Rate", &mut a.rate);
            number(ui, "Gate", &mut a.gate, 10..=100, "%");
            if a.style == ArpStyle::Random {
                seed(ui, &mut a.seed, new_seed, rects);
            }
        }
        ToolKind::Euclid => {
            let e = &mut p.euclid;
            number(ui, "Steps", &mut e.steps, 1..=64, "");
            let steps = e.steps.clamp(1, 64);
            number(ui, "Hits", &mut e.hits, 0..=steps, "");
            number(ui, "Rotation", &mut e.rotation, 0..=steps - 1, "");
            if drums {
                ui.horizontal(|ui| {
                    ui.label("Row");
                    let row = (e.pitch as usize).min(INST_NAMES.len() - 1);
                    let combo = egui::ComboBox::from_id_salt("euclid_row")
                        .width(50.0)
                        .selected_text(INST_NAMES[row])
                        .show_ui(ui, |ui| {
                            for (i, name) in INST_NAMES.iter().enumerate() {
                                ui.selectable_value(&mut e.pitch, i as u8, *name);
                            }
                        });
                    rects.push(("Row".into(), combo.response.rect));
                });
            } else {
                pitch(ui, "Pitch", &mut e.pitch);
            }
            note(
                ui,
                &format!("One cell is {} (the dock's snap).", snap.label()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::test_rack::{FM, KIT808, SYNTH1};
    use acid_engine::project::Project;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};

    /// The test rack with `notes` in `uid`'s first pattern (64 ticks), the
    /// dock focused on `uid`, and `sel` (indices) selected.
    fn app_with(uid: MachineUid, notes: &[(u16, u16, u8)], sel: &[usize]) -> SidAcidApp {
        let mut app = SidAcidApp::headless(Document::new(Project::default()));
        let p = app.doc.project.roll_mut(uid, 0).unwrap();
        p.length = 16;
        p.notes = notes
            .iter()
            .map(|&(s, l, pi)| Note::new(s, l, pi, 100))
            .collect();
        let chosen = sel.iter().map(|&i| p.notes[i]).collect();
        app.view.focus = Some(uid);
        app.view.dock.keyboard = true;
        app.set_selection(uid, chosen);
        app
    }

    fn frame(app: &mut SidAcidApp, ctx: &egui::Context, time: f64, events: Vec<Event>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0))),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.ui(ctx));
    }

    fn press(app: &mut SidAcidApp, ctx: &egui::Context, at: Pos2, time: f64) {
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(app, ctx, time, vec![Event::PointerMoved(at)]);
        frame(app, ctx, time + 0.05, vec![button(true)]);
        frame(app, ctx, time + 0.1, vec![button(false)]);
    }

    fn rect(app: &SidAcidApp, name: &str) -> Option<Rect> {
        let rects = &app.view.dock.tool_rects;
        rects.iter().find(|(n, _)| n == name).map(|(_, r)| *r)
    }

    /// Draws a frame, then clicks `name` (which must be there).
    fn click(app: &mut SidAcidApp, ctx: &egui::Context, name: &str, time: f64) {
        frame(app, ctx, time, vec![]);
        let r = rect(app, name).unwrap_or_else(|| panic!("no {name}"));
        press(app, ctx, r.center(), time + 0.1);
    }

    /// Opens `tool` from the Tools menu, from egui time `time`.
    fn open(app: &mut SidAcidApp, ctx: &egui::Context, tool: ToolKind, time: f64) {
        click(app, ctx, "Tools", time);
        click(app, ctx, &format!("Tool {}", tool.label()), time + 1.0);
    }

    fn starts(app: &SidAcidApp, uid: MachineUid) -> Vec<u16> {
        let roll = app.doc.project.roll(uid, 0).unwrap();
        roll.notes.iter().map(|n| n.start).collect()
    }

    fn set_params(app: &mut SidAcidApp, f: impl FnOnce(&mut ToolParams)) {
        f(&mut app.view.tool.as_mut().expect("a tool is open").params);
    }

    fn key(key: egui::Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn quantize_previews_live_and_cancel_leaves_no_trace() {
        let mut app = app_with(FM, &[(2, 4, 60), (10, 4, 62)], &[]);
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, 0.0, vec![]);
        let before = app.doc.project.clone();
        assert!(!app.doc.is_dirty());
        open(&mut app, &ctx, ToolKind::Quantize, 1.0);
        assert!(app.view.tool.is_some(), "the popup is open");
        assert_eq!(starts(&app, FM), [4, 12], "full strength, live");
        set_params(&mut app, |p| p.quantize.strength = 50);
        frame(&mut app, &ctx, 3.0, vec![]);
        assert_eq!(starts(&app, FM), [3, 11], "half way, from the snapshot");
        assert_eq!(app.doc.undo_depth(), 0, "the preview is no undo step");
        assert_eq!(app.selection(FM), [0, 1], "the result is selected");
        click(&mut app, &ctx, "Cancel", 4.0);
        assert!(app.view.tool.is_none());
        assert_eq!(app.doc.project, before, "the pattern as it was");
        assert_eq!(app.doc.undo_depth(), 0, "no undo step");
        assert!(!app.doc.is_dirty(), "and clean");
        assert!(app.selection(FM).is_empty(), "the old (empty) selection");
    }

    #[test]
    fn apply_is_one_undo_step_and_selects_the_result() {
        let mut app = app_with(FM, &[(2, 4, 60), (10, 4, 62)], &[1]);
        let ctx = egui::Context::default();
        let before = app.doc.project.clone();
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        for (i, s) in [25, 75, 100].into_iter().enumerate() {
            set_params(&mut app, |p| p.quantize.strength = s);
            frame(&mut app, &ctx, 2.0 + i as f64, vec![]);
        }
        assert_eq!(starts(&app, FM), [2, 12], "only the selected note");
        click(&mut app, &ctx, "Apply", 6.0);
        assert!(app.view.tool.is_none());
        assert_eq!(app.doc.undo_depth(), 1, "one undo step");
        assert_eq!(app.selection(FM), [1], "the moved note is selected");
        assert_eq!(app.view.tool_params.quantize.strength, 100, "remembered");
        app.undo();
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn a_tool_that_changes_nothing_applies_no_step() {
        let mut app = app_with(FM, &[(0, 4, 60), (8, 4, 62)], &[]);
        let ctx = egui::Context::default();
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        assert_eq!(app.doc.undo_depth(), 0, "on the grid already");
        click(&mut app, &ctx, "Apply", 2.0);
        assert_eq!(app.doc.undo_depth(), 0);
        // A preview that came back to where it started.
        let mut app = app_with(FM, &[(2, 4, 60)], &[]);
        open(&mut app, &ctx, ToolKind::Quantize, 3.0);
        set_params(&mut app, |p| p.quantize.strength = 0);
        frame(&mut app, &ctx, 5.0, vec![]);
        click(&mut app, &ctx, "Apply", 6.0);
        assert_eq!(app.doc.undo_depth(), 0);
        assert!(!app.doc.is_dirty());
    }

    /// One full frame pressing and releasing each of `keys` in turn (a
    /// press with no release in between would count as a key repeat),
    /// with the dock holding the keyboard.
    fn keys(app: &mut SidAcidApp, ctx: &egui::Context, time: f64, keys: &[(egui::Key, Modifiers)]) {
        app.view.dock.keyboard = true;
        let mut events = Vec::new();
        for &(k, modifiers) in keys {
            events.push(key(k, modifiers));
            events.push(Event::Key {
                key: k,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            });
        }
        frame(app, ctx, time, events);
    }

    #[test]
    fn keys_clipboard_and_undo_wait_while_a_tool_is_open() {
        use egui::Key;
        let none = Modifiers::NONE;
        let cmd = Modifiers::COMMAND;
        // Without a tool these keys edit: the control.
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60), (10, 4, 62)], &[0]);
        keys(&mut app, &ctx, 0.0, &[(Key::ArrowRight, none)]);
        assert_eq!(starts(&app, FM), [6, 10], "an arrow moves the selection");
        keys(&mut app, &ctx, 1.0, &[(Key::Z, cmd)]);
        assert_eq!(starts(&app, FM), [2, 10], "Ctrl+Z undoes it");

        let mut app = app_with(FM, &[(2, 4, 60), (10, 4, 62)], &[0]);
        let ctx = egui::Context::default();
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        let preview = app.doc.project.clone();
        let pressed = [
            (Key::ArrowRight, none),
            (Key::Delete, none),
            (Key::A, none),
            (Key::Z, cmd),
            (Key::Y, cmd),
            (Key::X, cmd),
            (Key::D, cmd),
            (Key::V, cmd),
        ];
        keys(&mut app, &ctx, 3.0, &pressed);
        assert_eq!(app.doc.project, preview, "nothing but the preview");
        assert_eq!(app.doc.undo_depth(), 0);
        app.undo();
        assert_eq!(app.doc.project, preview, "the menu's undo waits too");
        assert!(app.view.tool.is_some(), "still open");
        // Once applied, undo works again.
        click(&mut app, &ctx, "Apply", 4.0);
        keys(&mut app, &ctx, 6.0, &[(Key::Z, cmd)]);
        assert_eq!(starts(&app, FM), [2, 10]);
    }

    #[test]
    fn esc_cancels_an_open_tool() {
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60), (10, 4, 62)], &[0]);
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        assert_eq!(starts(&app, FM), [4, 10]);
        keys(&mut app, &ctx, 3.0, &[(egui::Key::Escape, Modifiers::NONE)]);
        assert!(app.view.tool.is_none());
        assert_eq!(starts(&app, FM), [2, 10]);
        assert_eq!(app.doc.undo_depth(), 0);
        assert_eq!(app.selection(FM), [0], "the selection it had");
    }

    #[test]
    fn esc_closes_an_open_combo_before_it_cancels_the_tool() {
        let ctx = egui::Context::default();
        let mut app = app_with(KIT808, &[(0, 4, 0)], &[]);
        open(&mut app, &ctx, ToolKind::Euclid, 0.0);
        let popup = |ctx: &egui::Context| ctx.memory(|m| m.any_popup_open());
        for (time, keyboard) in [(2.0, false), (6.0, true)] {
            click(&mut app, &ctx, "Row", time);
            frame(&mut app, &ctx, time + 1.0, vec![]);
            assert!(popup(&ctx), "the row list is open");
            app.view.dock.keyboard = keyboard;
            tap(&mut app, &ctx, time + 2.0, egui::Key::Escape);
            frame(&mut app, &ctx, time + 2.5, vec![]);
            assert!(app.view.tool.is_some(), "only the list closes");
            assert!(!popup(&ctx), "the list is shut");
        }
        tap(&mut app, &ctx, 10.0, egui::Key::Escape);
        assert!(app.view.tool.is_none(), "a second Esc cancels the tool");
        assert_eq!(app.doc.undo_depth(), 0);
    }

    /// One frame pressing and releasing `k`, leaving the dock's keyboard
    /// focus as it is.
    fn tap(app: &mut SidAcidApp, ctx: &egui::Context, time: f64, k: egui::Key) {
        let up = Event::Key {
            key: k,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        frame(app, ctx, time, vec![key(k, Modifiers::NONE), up]);
    }

    #[test]
    fn esc_cancels_after_a_click_in_the_popup() {
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60), (10, 4, 62)], &[]);
        let before = app.doc.project.clone();
        open(&mut app, &ctx, ToolKind::Humanize, 0.0);
        click(&mut app, &ctx, "New seed", 2.0);
        assert!(
            !app.view.dock.keyboard,
            "the popup click took the dock's keyboard"
        );
        assert_ne!(app.doc.project, before, "previewing");
        tap(&mut app, &ctx, 4.0, egui::Key::Escape);
        assert!(app.view.tool.is_none(), "cancelled");
        assert_eq!(app.doc.project, before, "restored");
        assert_eq!(app.doc.undo_depth(), 0);
    }

    #[test]
    fn a_focused_popup_widget_keeps_its_keys() {
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60), (10, 4, 62)], &[]);
        open(&mut app, &ctx, ToolKind::Humanize, 0.0);
        // Focus Timing's value, then Tab on to the Apply button.
        click(&mut app, &ctx, "Timing", 2.0);
        let apply = rect(&app, "Apply").unwrap();
        let focused_rect = |ctx: &egui::Context| {
            ctx.memory(|m| m.focused())
                .and_then(|id| ctx.read_response(id))
                .map(|r| r.rect)
        };
        let mut time = 3.0;
        while focused_rect(&ctx) != Some(apply) {
            assert!(time < 20.0, "Tab never reached Apply");
            tap(&mut app, &ctx, time, egui::Key::Tab);
            time += 1.0;
        }
        // The popup may overlap the dock, whose keyboard focus then stays:
        // Enter still goes to the focused button, not the dock.
        app.view.dock.keyboard = true;
        tap(&mut app, &ctx, time, egui::Key::Enter);
        assert!(app.view.tool.is_none(), "Enter pressed Apply");
        assert_eq!(app.doc.undo_depth(), 1, "applied, not cancelled");
    }

    #[test]
    fn cancel_restores_the_origin_after_another_edit_came_in() {
        use acid_engine::params::{MachineParam, ParamId, SynthParam};
        let cutoff = ParamId::Machine(SYNTH1, MachineParam::Synth(SynthParam::Cutoff));
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60), (10, 4, 62)], &[]);
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        assert_eq!((starts(&app, FM), app.doc.undo_depth()), (vec![4, 12], 0));
        let cmds = app.doc.apply(
            Edit::Param {
                id: cutoff,
                value: 0.9,
            },
            false,
        );
        app.send(cmds);
        assert_eq!(app.doc.undo_depth(), 1);
        set_params(&mut app, |p| p.quantize.strength = 50);
        frame(&mut app, &ctx, 3.0, vec![]);
        assert_eq!((starts(&app, FM), app.doc.undo_depth()), (vec![3, 11], 1));
        click(&mut app, &ctx, "Cancel", 4.0);
        assert!(app.view.tool.is_none());
        assert_eq!(starts(&app, FM), [2, 10], "the origin, always");
        assert_eq!(app.doc.project.param(cutoff), 0.9, "the other edit stays");
        assert_eq!(app.doc.undo_depth(), 1, "only the other edit");
        app.undo();
        assert_eq!(starts(&app, FM), [2, 10], "which undoes alone");
        assert_ne!(app.doc.project.param(cutoff), 0.9);

        // An edit before the first preview.
        let mut app = app_with(FM, &[(2, 4, 60)], &[]);
        app.view.tool_params.quantize.strength = 0;
        open(&mut app, &ctx, ToolKind::Quantize, 10.0);
        assert_eq!(app.doc.undo_depth(), 0, "at 0 %: nothing to preview");
        let cmds = app.doc.apply(
            Edit::Param {
                id: cutoff,
                value: 0.9,
            },
            false,
        );
        app.send(cmds);
        set_params(&mut app, |p| p.quantize.strength = 100);
        frame(&mut app, &ctx, 12.0, vec![]);
        assert_eq!(starts(&app, FM), [4]);
        click(&mut app, &ctx, "Cancel", 13.0);
        assert_eq!(starts(&app, FM), [2]);
        assert_eq!(app.doc.project.param(cutoff), 0.9, "not dropped");
        assert_eq!(app.doc.undo_depth(), 1, "only the other edit");
    }

    #[test]
    fn rack_edits_on_the_tools_pattern_cancel_it_first() {
        use acid_engine::random::Dice;
        let ctx = egui::Context::default();
        let notes = [(2, 4, 36), (10, 4, 38)];
        // Dice: the same as dicing the pattern with no tool open.
        let mut plain = app_with(SYNTH1, &notes, &[]);
        plain.randomise_synth_seeded(SYNTH1, Dice::ALL, 5);
        let mut app = app_with(SYNTH1, &notes, &[]);
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        assert_eq!(starts(&app, SYNTH1), [4, 12]);
        app.randomise_synth_seeded(SYNTH1, Dice::ALL, 5);
        assert!(app.view.tool.is_none(), "cancelled");
        assert_eq!(app.doc.project, plain.doc.project, "diced from the origin");
        assert_eq!(app.doc.undo_depth(), 1, "only the dice");
        // Length: the origin's notes at the new length.
        let mut app = app_with(SYNTH1, &notes, &[]);
        open(&mut app, &ctx, ToolKind::Quantize, 3.0);
        app.set_synth_length(SYNTH1, 0, 8, false);
        assert!(app.view.tool.is_none());
        assert_eq!(starts(&app, SYNTH1), [2, 10]);
        assert_eq!(app.doc.project.roll(SYNTH1, 0).unwrap().length, 8);
        assert_eq!(app.doc.undo_depth(), 1);
        // A pattern paste from the Edit menu.
        let mut app = app_with(FM, &[(2, 4, 60)], &[]);
        let mut clip = app.doc.project.roll(FM, 0).unwrap().clone();
        clip.notes[0].start = 20;
        app.view.clipboard = Some(crate::ui::files::Clip::Notes(clip.clone()));
        open(&mut app, &ctx, ToolKind::Quantize, 6.0);
        app.paste_pattern();
        assert!(app.view.tool.is_none());
        assert_eq!(app.doc.project.roll(FM, 0).unwrap(), &clip);
        assert_eq!(app.doc.undo_depth(), 1);
    }

    #[test]
    fn an_apply_back_at_the_origin_after_another_edit_adds_no_step() {
        use acid_engine::params::{MachineParam, ParamId, SynthParam};
        let cutoff = ParamId::Machine(SYNTH1, MachineParam::Synth(SynthParam::Cutoff));
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60)], &[]);
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        let cmds = app.doc.apply(
            Edit::Param {
                id: cutoff,
                value: 0.9,
            },
            false,
        );
        app.send(cmds);
        set_params(&mut app, |p| p.quantize.strength = 0);
        frame(&mut app, &ctx, 3.0, vec![]);
        click(&mut app, &ctx, "Apply", 4.0);
        assert_eq!(starts(&app, FM), [2]);
        assert_eq!(app.doc.project.param(cutoff), 0.9);
        assert_eq!(app.doc.undo_depth(), 1, "only the knob");
    }

    #[test]
    fn apply_after_another_edit_is_one_step_from_the_origin() {
        use acid_engine::params::{MachineParam, ParamId, SynthParam};
        let cutoff = ParamId::Machine(SYNTH1, MachineParam::Synth(SynthParam::Cutoff));
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60), (10, 4, 62)], &[]);
        let (before, knob) = (app.doc.project.clone(), app.doc.project.param(cutoff));
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        set_params(&mut app, |p| p.quantize.strength = 50);
        frame(&mut app, &ctx, 2.0, vec![]);
        let cmds = app.doc.apply(
            Edit::Param {
                id: cutoff,
                value: 0.9,
            },
            false,
        );
        app.send(cmds);
        set_params(&mut app, |p| p.quantize.strength = 100);
        frame(&mut app, &ctx, 3.0, vec![]);
        click(&mut app, &ctx, "Apply", 4.0);
        assert_eq!(starts(&app, FM), [4, 12]);
        assert_eq!(
            app.doc.undo_depth(),
            2,
            "the knob and the tool, nothing else"
        );
        app.undo();
        assert_eq!(
            starts(&app, FM),
            [2, 10],
            "one undo goes back to the origin"
        );
        assert_eq!(app.doc.project.param(cutoff), 0.9, "under the other edit");
        app.undo();
        assert_eq!(app.doc.project.param(cutoff), knob);
        assert_eq!(app.doc.project, before);
        assert!(!app.doc.can_undo());
    }

    #[test]
    fn cancel_at_the_undo_limit_keeps_the_whole_history() {
        use crate::document::UNDO_LIMIT;
        use acid_engine::params::{MachineParam, ParamId, SynthParam};
        let cutoff = ParamId::Machine(SYNTH1, MachineParam::Synth(SynthParam::Cutoff));
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60)], &[]);
        let before = app.doc.project.clone();
        for i in 0..UNDO_LIMIT {
            let cmds = app.doc.apply(
                Edit::Param {
                    id: cutoff,
                    value: (i + 1) as f32 / 1000.0,
                },
                false,
            );
            app.send(cmds);
        }
        assert_eq!(app.doc.undo_depth(), UNDO_LIMIT);
        let full = app.doc.project.clone();
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        assert_eq!(starts(&app, FM), [4], "previewing");
        assert_eq!(app.doc.undo_depth(), UNDO_LIMIT);
        click(&mut app, &ctx, "Cancel", 2.0);
        assert_eq!(app.doc.project, full);
        assert_eq!(app.doc.undo_depth(), UNDO_LIMIT, "no step evicted");
        while app.doc.can_undo() {
            app.undo();
        }
        assert_eq!(app.doc.project, before, "the oldest step is still there");
    }

    #[test]
    fn an_edit_under_the_tool_closes_it_with_the_preview_as_a_step() {
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60)], &[]);
        let before = app.doc.project.clone();
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        let previewed = app.doc.project.clone();
        // Something that doesn't yield to the tool edits its pattern.
        let mut p = app.doc.project.roll(FM, 0).unwrap().clone();
        p.notes[0].pitch = 61;
        let cmds = app.doc.apply(
            Edit::NotePattern {
                uid: FM,
                slot: 0,
                pattern: Box::new(p),
            },
            false,
        );
        app.send(cmds);
        frame(&mut app, &ctx, 2.0, vec![]);
        assert!(app.view.tool.is_none(), "closed");
        assert_eq!(app.doc.undo_depth(), 2, "the preview, then that edit");
        app.undo();
        assert_eq!(app.doc.project, previewed);
        app.undo();
        assert_eq!(app.doc.project, before);
    }

    #[test]
    fn mouse_edits_on_the_tools_lane_wait() {
        let ctx = egui::Context::default();
        // A click on Synth 1's (top) grid adds a note...
        let click_grid = |app: &mut SidAcidApp, time: f64| {
            frame(app, &ctx, time, vec![]);
            let lanes = app.view.dock.lanes.unwrap();
            let at = pos2(
                lanes.left() + app.lane_key_w() + 41.0,
                lanes.top() + super::super::lane::HEADER_H + 30.0,
            );
            press(app, &ctx, at, time + 0.1);
        };
        let mut app = app_with(SYNTH1, &[(0, 4, 48)], &[]);
        click_grid(&mut app, 0.0);
        assert_eq!(starts(&app, SYNTH1).len(), 2, "a click adds a note");
        // ...but not under an open tool.
        let mut app = app_with(SYNTH1, &[(2, 4, 48)], &[]);
        open(&mut app, &ctx, ToolKind::Quantize, 1.0);
        let preview = app.doc.project.clone();
        click_grid(&mut app, 3.0);
        assert_eq!(app.doc.project, preview);
        assert!(app.view.tool.is_some());
    }

    #[test]
    fn the_scope_is_the_selection_or_every_visible_note() {
        // Note 2 starts past the 16-step end.
        let notes = [(0, 4, 60), (8, 4, 62), (70, 4, 64)];
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &notes, &[]);
        open(&mut app, &ctx, ToolKind::Reverse, 0.0);
        assert_eq!(app.view.tool.as_ref().unwrap().scope, [0, 1]);
        assert_eq!(starts(&app, FM), [8, 0, 70], "the hidden note stays");
        let mut app = app_with(FM, &notes, &[1, 2]);
        open(&mut app, &ctx, ToolKind::Reverse, 2.0);
        assert_eq!(app.view.tool.as_ref().unwrap().scope, [1, 2]);
    }

    #[test]
    fn melodic_tools_are_greyed_on_drums() {
        let ctx = egui::Context::default();
        let mut app = app_with(KIT808, &[(0, 4, 0), (0, 4, 2)], &[]);
        let mut time = 0.0;
        for t in [ToolKind::Invert, ToolKind::Strum, ToolKind::Arpeggiate] {
            open(&mut app, &ctx, t, time);
            assert!(app.view.tool.is_none(), "{t:?} is off on drums");
            // Close the menu.
            press(&mut app, &ctx, pos2(640.0, 100.0), time + 3.0);
            time += 4.0;
        }
        open(&mut app, &ctx, ToolKind::Euclid, time);
        let t = app.view.tool.as_ref().expect("Euclidean works on drums");
        assert!(t.params.euclid.pitch < 8, "on a drum row");
    }

    #[test]
    fn chord_tools_are_greyed_on_an_acid_lane() {
        let ctx = egui::Context::default();
        let mut app = app_with(SYNTH1, &[(0, 4, 36), (4, 4, 40)], &[]);
        assert_eq!(app.lane_kind(SYNTH1), LaneKind::Acid);
        let mut time = 0.0;
        for t in [ToolKind::Strum, ToolKind::Arpeggiate] {
            open(&mut app, &ctx, t, time);
            assert!(app.view.tool.is_none(), "{t:?} is off on a mono lane");
            press(&mut app, &ctx, pos2(640.0, 100.0), time + 3.0);
            time += 4.0;
        }
        open(&mut app, &ctx, ToolKind::Invert, time);
        assert!(app.view.tool.is_some(), "Invert works on a mono lane");
    }

    #[test]
    fn drum_randomize_has_no_length_settings() {
        let ctx = egui::Context::default();
        let mut app = app_with(KIT808, &[(0, 4, 0)], &[]);
        open(&mut app, &ctx, ToolKind::Randomize, 0.0);
        frame(&mut app, &ctx, 2.0, vec![]);
        assert!(rect(&app, "Shortest").is_none() && rect(&app, "Longest").is_none());
        let mut app = app_with(FM, &[(0, 4, 60)], &[]);
        open(&mut app, &ctx, ToolKind::Randomize, 3.0);
        frame(&mut app, &ctx, 5.0, vec![]);
        assert!(rect(&app, "Shortest").is_some() && rect(&app, "Longest").is_some());
    }

    #[test]
    fn removing_the_tools_machine_cancels_it_first() {
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60)], &[]);
        let before = app.doc.project.clone();
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        assert_eq!(starts(&app, FM), [4]);
        app.remove_machine(FM);
        assert!(app.view.tool.is_none(), "cancelled");
        assert_eq!(app.doc.undo_depth(), 1, "only the removal");
        app.undo();
        assert_eq!(app.doc.project, before, "back as it was before the tool");
    }

    #[test]
    fn song_mode_has_no_tools_button() {
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(0, 4, 60)], &[]);
        frame(&mut app, &ctx, 0.0, vec![]);
        assert!(rect(&app, "Tools").is_some(), "first in the inspector row");
        app.view.mode = Mode::Song;
        frame(&mut app, &ctx, 1.0, vec![]);
        assert!(rect(&app, "Tools").is_none());
    }

    #[test]
    fn the_tools_button_sits_first_in_the_inspector_row() {
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(0, 4, 60)], &[0]);
        frame(&mut app, &ctx, 0.0, vec![]);
        let (tools, vel) = (rect(&app, "Tools").unwrap(), rect(&app, "Vel").unwrap());
        assert!(tools.right() <= vel.left(), "left of the inspector");
        let snap = rect(&app, "Snap to scale").unwrap();
        assert!(
            tools.top() >= snap.bottom(),
            "in the row under the snap and key"
        );
        assert!(tools.left() < snap.left(), "at the row's start");
    }

    #[test]
    fn leaving_the_lane_its_pattern_or_pattern_mode_cancels() {
        let ctx = egui::Context::default();
        let leave: [fn(&mut SidAcidApp); 3] = [
            |app| app.view.focus = Some(SYNTH1),
            |app| app.select_pad(FM, 1),
            |app| app.view.mode = Mode::Song,
        ];
        for (i, leave) in leave.into_iter().enumerate() {
            let mut app = app_with(FM, &[(2, 4, 60)], &[]);
            let before = app.doc.project.roll(FM, 0).unwrap().clone();
            let time = i as f64 * 10.0;
            open(&mut app, &ctx, ToolKind::Quantize, time);
            assert_eq!(starts(&app, FM), [4]);
            leave(&mut app);
            frame(&mut app, &ctx, time + 3.0, vec![]);
            assert!(app.view.tool.is_none(), "case {i}");
            assert_eq!(app.doc.project.roll(FM, 0).unwrap(), &before, "case {i}");
            assert_eq!(app.doc.undo_depth(), 0, "case {i}");
        }
    }

    #[test]
    fn a_press_on_another_lane_cancels_before_its_edit() {
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(2, 4, 60)], &[]);
        open(&mut app, &ctx, ToolKind::Quantize, 0.0);
        frame(&mut app, &ctx, 2.0, vec![]);
        let lanes = app.view.dock.lanes.unwrap();
        // Synth 1's grid, on top.
        let at = pos2(
            lanes.left() + app.lane_key_w() + 41.0,
            lanes.top() + super::super::lane::HEADER_H + 30.0,
        );
        press(&mut app, &ctx, at, 3.0);
        assert!(app.view.tool.is_none());
        assert_eq!(starts(&app, FM), [2], "FM as it was");
        assert_eq!(app.doc.undo_depth(), 1, "only Synth 1's new note");
        app.undo();
        assert_eq!(app.doc.undo_depth(), 0);
    }

    #[test]
    fn a_new_seed_rolls_again_and_a_seed_repeats() {
        let ctx = egui::Context::default();
        let mut app = app_with(FM, &[(0, 4, 60), (60, 4, 62)], &[]);
        app.view.tool_params.randomize.density = 100;
        open(&mut app, &ctx, ToolKind::Randomize, 0.0);
        let first = app.doc.project.roll(FM, 0).unwrap().clone();
        let seed = app.view.tool.as_ref().unwrap().params.randomize.seed;
        click(&mut app, &ctx, "New seed", 2.0);
        let new = app.view.tool.as_ref().unwrap().params.randomize.seed;
        assert_ne!(new, seed);
        assert!(new < SEEDS);
        assert_ne!(app.doc.project.roll(FM, 0).unwrap(), &first);
        set_params(&mut app, |p| p.randomize.seed = seed);
        frame(&mut app, &ctx, 4.0, vec![]);
        assert_eq!(app.doc.project.roll(FM, 0).unwrap(), &first, "same seed");
        assert_eq!(app.doc.undo_depth(), 0, "still no step");
    }

    #[test]
    fn generators_with_nothing_selected_fill_the_whole_pattern() {
        let ctx = egui::Context::default();
        // One note at 0..4 and nothing selected: E(4,16) spreads its hits
        // over the whole 16-step pattern, not the note's one step.
        let mut app = app_with(FM, &[(0, 4, 50)], &[]);
        app.view.tool_params.euclid = Euclid {
            hits: 4,
            steps: 16,
            rotation: 0,
            pitch: 60,
        };
        app.view.panel(FM).cursor.pitch = 60;
        open(&mut app, &ctx, ToolKind::Euclid, 0.0);
        let roll = app.doc.project.roll(FM, 0).unwrap();
        let hits: Vec<u16> = roll
            .notes
            .iter()
            .filter(|n| n.pitch == 60)
            .map(|n| n.start)
            .collect();
        assert_eq!(hits, [0, 16, 32, 48]);
        // Randomize at 100 % fills every step of the pattern.
        let mut app = app_with(FM, &[(0, 4, 50)], &[]);
        app.view.tool_params.randomize = Randomize {
            density: 100,
            len_min: 1,
            len_max: 1,
            ..Randomize::default()
        };
        open(&mut app, &ctx, ToolKind::Randomize, 10.0);
        let mut got = starts(&app, FM);
        got.sort_unstable();
        assert_eq!(got, (0..16).map(|k| k * 4).collect::<Vec<u16>>());
        // With a selection they keep to its span.
        let mut app = app_with(FM, &[(0, 4, 50)], &[0]);
        app.view.tool_params.randomize.density = 100;
        open(&mut app, &ctx, ToolKind::Randomize, 20.0);
        assert_eq!(starts(&app, FM), [0]);
    }

    #[test]
    fn every_tool_opens_and_draws_its_popup() {
        let ctx = egui::Context::default();
        let mut time = 0.0;
        for (uid, notes) in [
            (FM, vec![(0, 8, 60), (0, 8, 64), (8, 4, 67)]),
            (KIT808, vec![(0, 4, 0), (4, 4, 1)]),
            (SYNTH1, vec![(0, 4, 36), (6, 4, 38)]),
        ] {
            for t in ToolKind::ALL {
                let mut app = app_with(uid, &notes, &[]);
                app.view.tool_params = ToolParams::default();
                app.view.tool_params.arp.style = ArpStyle::Random;
                if t.unavailable(app.lane_kind(uid)).is_some() {
                    continue;
                }
                open(&mut app, &ctx, t, time);
                frame(&mut app, &ctx, time + 3.0, vec![]);
                assert!(rect(&app, "Apply").is_some(), "{uid:?} {t:?}");
                click(&mut app, &ctx, "Apply", time + 4.0);
                time += 10.0;
                assert!(app.view.tool.is_none(), "{uid:?} {t:?}");
                assert!(app.doc.undo_depth() <= 1, "{uid:?} {t:?}");
            }
        }
    }
}
