//! The Mono panel: the synth's knobs grouped in front-panel order (VCO,
//! Source, VCF, Env; then LFO, Porta, Arp, Drift, Chorus), an envelope
//! sketch, the patch browser, and the pattern row with the poly piano roll.

use super::knob::CELL;
use super::mono_patches::STARTER;
use super::{SidAcidApp, theme};
use acid_engine::machine::MachineUid;
use acid_engine::params::MonoParam as M;
use acid_engine::project::{MonoMachine, PatternRef};
use eframe::egui::{self, Sense, Stroke, pos2, vec2};

type Group = (&'static str, &'static [(M, &'static str)]);

const FRONT: [Group; 4] = [
    (
        "VCO",
        &[
            (M::Range, "Range"),
            (M::Tune, "Tune"),
            (M::VcoMod, "Mod"),
            (M::PulseWidth, "PW"),
            (M::PwmSource, "PWM"),
        ],
    ),
    (
        "SOURCE",
        &[
            (M::PulseLevel, "Pulse"),
            (M::SawLevel, "Saw"),
            (M::SubLevel, "Sub"),
            (M::SubWave, "Sub Wave"),
            (M::NoiseLevel, "Noise"),
        ],
    ),
    (
        "VCF",
        &[
            (M::Cutoff, "Cutoff"),
            (M::Resonance, "Reso"),
            (M::EnvAmount, "Env"),
            (M::VcfMod, "Mod"),
            (M::KeyFollow, "Kybd"),
            (M::VelToVcf, "Vel"),
        ],
    ),
    (
        "ENV",
        &[
            (M::Attack, "A"),
            (M::Decay, "D"),
            (M::Sustain, "S"),
            (M::Release, "R"),
            (M::Trigger, "Trig"),
            (M::VcaMode, "VCA"),
            (M::VelToVca, "Vel"),
        ],
    ),
];
const SECOND: [Group; 6] = [
    ("LFO", &[(M::LfoWave, "Wave"), (M::LfoRate, "Rate")]),
    ("PORTA", &[(M::Portamento, "Mode"), (M::PortaTime, "Time")]),
    (
        "ARP",
        &[
            (M::ArpOn, "On"),
            (M::ArpMode, "Mode"),
            (M::ArpRange, "Range"),
            (M::ArpRate, "Rate"),
        ],
    ),
    ("DRIFT", &[(M::Drift, "Drift")]),
    (
        "CHORUS",
        &[
            (M::ChorusRate, "Rate"),
            (M::ChorusDepth, "Depth"),
            (M::ChorusMix, "Mix"),
        ],
    ),
    ("OUT", &[(M::Volume, "Volume")]),
];

impl SidAcidApp {
    /// Mono machine `uid`; it must be a Mono machine in the rack.
    fn mono_of(&self, uid: MachineUid) -> &MonoMachine {
        self.doc
            .project
            .mono(uid)
            .expect("a Mono machine in the rack")
    }

    pub(crate) fn mono_panel(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let slot = self.selected(uid);
        let title = format!(
            "{}   {}   {}",
            self.panel_title(uid).to_uppercase(),
            PatternRef(slot).label(),
            self.mono_of(uid).patch_name
        );
        self.machine_panel(ui, uid, "mono", title, |app, ui| {
            ui.horizontal_wrapped(|ui| {
                for group in FRONT {
                    app.mono_group(ui, uid, group);
                }
            });
            ui.horizontal_wrapped(|ui| {
                for group in SECOND {
                    app.mono_group(ui, uid, group);
                }
                app.mono_env_sketch(ui, uid);
            });
            app.mono_patch_browser(ui, uid);
            app.note_pattern_row(ui, uid);
            app.poly_roll(ui, uid);
        });
    }

    /// One framed knob group. Its width is worked out before drawing (the
    /// wrapping row cannot see a group's width until it is drawn), so a
    /// group that would run past the right edge starts the next row.
    fn mono_group(&mut self, ui: &mut egui::Ui, uid: MachineUid, (label, knobs): Group) {
        let text = egui::RichText::new(label).small().color(theme::ACID);
        let label_w = egui::WidgetText::from(text.clone())
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Small,
            )
            .size()
            .x;
        let frame = egui::Frame::group(ui.style());
        let n = knobs.len() as f32;
        let knobs_w = n * CELL.x + (n - 1.0) * ui.spacing().item_spacing.x;
        // A switch whose text is wider than its cell grows a little, so the
        // width drawn last frame wins when it is larger.
        let id = ui.id().with(("mono_group", label));
        let drawn = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
        let width = (knobs_w.max(label_w) + frame.total_margin().sum().x).max(drawn);
        wrap_unless_fits(ui, width);
        let r = frame.show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(text);
                ui.horizontal(|ui| {
                    for &(p, name) in knobs {
                        self.param_knob(ui, uid.param(p), name);
                    }
                });
            });
        });
        let w = r.response.rect.width();
        ui.data_mut(|d| d.insert_temp(id, w));
    }

    /// Display-only ADSR shape from the current knobs (times drawn on a
    /// log-ish scale so short and long envelopes both read).
    fn mono_env_sketch(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let v = |p: M| self.doc.project.param(uid.param(p));
        let (a, d, s, r) = (v(M::Attack), v(M::Decay), v(M::Sustain), v(M::Release));
        let size = vec2(96.0, 44.0);
        wrap_unless_fits(ui, size.x);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, theme::GRID_DARK);
        let inner = rect.shrink(3.0);
        // Segment widths: attack, decay, a fixed sustain hold, release.
        let w = [0.1 + a, 0.1 + d, 0.6, 0.1 + r];
        let total: f32 = w.iter().sum();
        let mut x = 0.0;
        let mut points = vec![(0.0, 0.0)];
        for (seg, level) in w.iter().zip([1.0, s, s, 0.0]) {
            x += seg / total;
            points.push((x, level));
        }
        let points: Vec<egui::Pos2> = points
            .iter()
            .map(|(x, y)| {
                pos2(
                    inner.left() + x * inner.width(),
                    inner.bottom() - y * inner.height(),
                )
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5_f32, theme::ACID)));
    }

    fn mono_patch_browser(&mut self, ui: &mut egui::Ui, uid: MachineUid) {
        let current = self.mono_of(uid).patch_name.clone();
        ui.horizontal(|ui| {
            ui.label("Patches");
            if ui
                .button("Randomise")
                .on_hover_text("A new random sound that always plays (undoable)")
                .clicked()
            {
                self.randomise_mono_patch(uid);
            }
        });
        let mut picked = None;
        egui::Grid::new(("mono_patches", uid))
            .spacing([4.0, 2.0])
            .show(ui, |ui| {
                for (i, preset) in STARTER.iter().enumerate() {
                    if ui
                        .selectable_label(preset.name == current, preset.name)
                        .clicked()
                    {
                        picked = Some(i);
                    }
                    if i % 7 == 6 {
                        ui.end_row();
                    }
                }
            });
        if let Some(i) = picked {
            let preset = &STARTER[i];
            self.load_mono_patch(uid, preset.name, preset.to_params());
        }
    }
}

/// Starts a new row of a wrapping layout when an item `width` wide would
/// run past the visible right edge. The edge is the clip rect as well as
/// the ui's own: an earlier over-wide row in the rack widens every later
/// ui's max rect past the window.
fn wrap_unless_fits(ui: &mut egui::Ui, width: f32) {
    let x = ui.cursor().min.x;
    let at_row_start = x <= ui.max_rect().left() + 0.5;
    let right = ui.max_rect().right().min(ui.clip_rect().right());
    if !at_row_start && x + width > right {
        ui.end_row();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use acid_engine::machine::MachineType;

    /// Every text shape drawn in a frame: its string and screen rect.
    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match s {
                egui::Shape::Text(t) => {
                    out.push((t.galley.job.text.clone(), t.visual_bounding_rect()))
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in shapes {
            walk(&c.shape, &mut out);
        }
        out
    }

    /// The demo project plus a Mono, drawn in a `w`×`h` window with the
    /// rack scrolled until the Mono panel (header to patch list) is on
    /// screen. Returns the texts drawn between the header and "Patches".
    fn mono_panel_texts(w: f32, h: f32) -> Vec<(String, egui::Rect)> {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let uid = app.add_machine(MachineType::Mono).expect("room for a Mono");
        let title = app.panel_title(uid).to_uppercase();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(w, h));
        let mut time = 0.0;
        let mut frame = |app: &mut SidAcidApp, events: Vec<egui::Event>| {
            time += 1.0 / 60.0;
            let input = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            };
            texts(&ctx.run(input, |ctx| app.ui(ctx)).shapes)
        };
        let find = |drawn: &[(String, egui::Rect)], f: &dyn Fn(&str) -> bool| {
            drawn.iter().find(|(t, _)| f(t)).map(|(_, r)| *r)
        };
        // Scroll with the pointer in the rack's left gutter, clear of knobs,
        // step grids and rolls (which take the wheel themselves).
        let pointer = pos2(12.0, h * 0.6);
        for _ in 0..400 {
            let drawn = frame(&mut app, vec![]);
            let head = find(&drawn, &|t| t.starts_with(&title));
            let patches = find(&drawn, &|t| t == "Patches");
            if let (Some(head), Some(patches)) = (head, patches)
                && head.top() >= 0.0
                && patches.bottom() <= h
            {
                return drawn
                    .into_iter()
                    .filter(|(_, r)| r.top() > head.bottom() && r.bottom() < patches.top())
                    .collect();
            }
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(pointer),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(0.0, -20.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        panic!("the Mono panel never came fully on screen at {w}x{h}");
    }

    #[test]
    fn every_group_and_knob_is_drawn_inside_the_window() {
        for (w, h) in [(1280.0, 800.0), (636.0, 1044.0)] {
            let drawn = mono_panel_texts(w, h);
            let drawn_as = |want: &str| {
                // Knobs paint their name; switches draw "name\nvalue".
                drawn
                    .iter()
                    .any(|(t, _)| t == want || t.starts_with(&format!("{want}\n")))
            };
            for (label, knobs) in FRONT.iter().chain(SECOND.iter()) {
                assert!(drawn_as(label), "{label} not drawn at {w}x{h}");
                for (_, name) in knobs.iter() {
                    assert!(drawn_as(name), "{label} {name} not drawn at {w}x{h}");
                }
            }
            for (t, r) in &drawn {
                assert!(r.right() <= w, "{t:?} ends at {} at {w}x{h}", r.right());
            }
        }
    }

    #[test]
    fn drawing_and_hovering_never_edit_the_project() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        let uid = app.add_machine(MachineType::Mono).expect("room for a Mono");
        let title = app.panel_title(uid).to_uppercase();
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut SidAcidApp, size: egui::Vec2, events: Vec<egui::Event>| {
            time += 1.0 / 60.0;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(time),
                events,
                ..Default::default()
            };
            texts(&ctx.run(input, |ctx| app.ui(ctx)).shapes)
        };
        frame(&mut app, vec2(1280.0, 800.0), vec![]);
        let project = app.doc.project.clone();
        let depth = app.doc.undo_depth();
        // Down the rack at the first size, back up at the second.
        for (size, wheel) in [(vec2(1280.0, 800.0), -300.0), (vec2(636.0, 1044.0), 300.0)] {
            let mut mono_seen = false;
            for i in 0..300 {
                let t = i as f32 / 300.0;
                let events = if i % 20 == 19 {
                    // Scroll the rack a little, with the pointer in the left
                    // gutter where nothing takes the wheel.
                    vec![
                        egui::Event::PointerMoved(pos2(12.0, size.y * 0.6)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: vec2(0.0, wheel),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                } else if i % 2 == 0 {
                    vec![]
                } else {
                    // Sweep the pointer across the window, row by row.
                    let x = (t * 17.0).fract() * size.x;
                    vec![egui::Event::PointerMoved(pos2(x, t * size.y))]
                };
                let drawn = frame(&mut app, size, events);
                mono_seen |= drawn
                    .iter()
                    .any(|(s, r)| s.starts_with(&title) && r.top() >= 0.0 && r.bottom() <= size.y);
            }
            assert!(mono_seen, "the Mono panel was never on screen at {size:?}");
        }
        assert_eq!(app.doc.undo_depth(), depth, "no undo step without a click");
        assert!(
            app.doc.project == project,
            "the project changed without a click"
        );
    }

    #[test]
    fn the_panel_draws_its_sections_and_the_patch_list() {
        let mut app = SidAcidApp::headless(Document::new(acid_engine::demo::demo_project()));
        app.add_machine(MachineType::Mono);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| app.ui(ctx));
        let mut texts = Vec::new();
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        // The groups and knobs are checked in
        // `every_group_and_knob_is_drawn_inside_the_window`.
        for want in ["Squelch", "Randomise"] {
            assert!(texts.iter().any(|t| t == want), "{want} not drawn");
        }
    }
}
