//! Global keyboard shortcuts.

use super::SidAcidApp;
use super::files::Pending;
use eframe::egui::{self, Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    PlayStop,
    Save,
    Open,
    New,
    Undo,
    Redo,
    Copy,
    Paste,
}

pub fn shortcut(key: Key, modifiers: Modifiers, step_writing: bool) -> Option<Shortcut> {
    if modifiers.command {
        return match key {
            Key::S => Some(Shortcut::Save),
            Key::O => Some(Shortcut::Open),
            Key::N => Some(Shortcut::New),
            Key::Z if modifiers.shift => Some(Shortcut::Redo),
            Key::Z => Some(Shortcut::Undo),
            Key::Y => Some(Shortcut::Redo),
            Key::C => Some(Shortcut::Copy),
            Key::V => Some(Shortcut::Paste),
            _ => None,
        };
    }
    (key == Key::Space && !step_writing).then_some(Shortcut::PlayStop)
}

/// The shortcut an input event triggers, if any. Key auto-repeat is ignored.
fn event_shortcut(e: &egui::Event, step_writing: bool) -> Option<Shortcut> {
    match e {
        egui::Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } => shortcut(*key, *modifiers, step_writing),
        // Some platforms deliver Ctrl+C / Ctrl+V as clipboard events.
        egui::Event::Copy => Some(Shortcut::Copy),
        egui::Event::Paste(_) => Some(Shortcut::Paste),
        _ => None,
    }
}

/// A text field (a rename, a number being typed into a drag value) has the
/// keyboard. `Context::wants_keyboard_input` is true for any focused widget,
/// so a focused button would swallow Space as a click instead of Play/Stop.
fn typing(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.focused())
        .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some())
}

impl SidAcidApp {
    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if typing(ctx) || self.view.confirm.is_some() || self.view.message.is_some() {
            return;
        }
        let step_writing = self.view.panels.values().any(|p| p.writer.is_some());
        let pressed: Vec<Shortcut> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| event_shortcut(e, step_writing))
                .collect()
        });
        // Space is Play/Stop here: take every Space event (presses, repeats,
        // releases) out of the frame, or the widget with keyboard focus
        // would also see it as a click, once per key repeat.
        if !step_writing {
            ctx.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(e, egui::Event::Key { key: Key::Space, modifiers, .. }
                        if !modifiers.command)
                })
            });
        }
        let (mut copied, mut pasted) = (false, false);
        for s in pressed {
            // Backends may send both a key and a clipboard event for one press.
            match s {
                Shortcut::Copy if std::mem::replace(&mut copied, true) => continue,
                Shortcut::Paste if std::mem::replace(&mut pasted, true) => continue,
                _ => {}
            }
            match s {
                Shortcut::PlayStop => {
                    if self.status.playing {
                        self.stop()
                    } else {
                        self.play()
                    }
                }
                Shortcut::Save => {
                    self.save(false);
                }
                Shortcut::Open => self.request(Pending::Open),
                Shortcut::New => self.request(Pending::New),
                Shortcut::Undo => {
                    self.undo();
                }
                Shortcut::Redo => {
                    self.redo();
                }
                Shortcut::Copy => self.copy_pattern(),
                Shortcut::Paste => self.paste_pattern(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Key, Modifiers};

    #[test]
    fn command_shortcuts() {
        let cmd = Modifiers::COMMAND;
        assert_eq!(shortcut(Key::S, cmd, false), Some(Shortcut::Save));
        assert_eq!(shortcut(Key::O, cmd, false), Some(Shortcut::Open));
        assert_eq!(shortcut(Key::N, cmd, false), Some(Shortcut::New));
        assert_eq!(shortcut(Key::Z, cmd, false), Some(Shortcut::Undo));
        assert_eq!(
            shortcut(Key::Z, cmd | Modifiers::SHIFT, false),
            Some(Shortcut::Redo)
        );
        assert_eq!(shortcut(Key::Y, cmd, false), Some(Shortcut::Redo));
        assert_eq!(shortcut(Key::C, cmd, true), Some(Shortcut::Copy));
        assert_eq!(shortcut(Key::V, cmd, true), Some(Shortcut::Paste));
    }

    #[test]
    fn space_plays_unless_step_writing() {
        assert_eq!(
            shortcut(Key::Space, Modifiers::NONE, false),
            Some(Shortcut::PlayStop)
        );
        assert_eq!(shortcut(Key::Space, Modifiers::NONE, true), None);
        assert_eq!(shortcut(Key::Z, Modifiers::NONE, false), None);
    }

    #[test]
    fn key_repeat_is_ignored() {
        let space = |repeat| egui::Event::Key {
            key: Key::Space,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: Modifiers::NONE,
        };
        assert_eq!(
            event_shortcut(&space(false), false),
            Some(Shortcut::PlayStop)
        );
        assert_eq!(event_shortcut(&space(true), false), None);
    }

    fn space(pressed: bool, repeat: bool) -> egui::Event {
        egui::Event::Key {
            key: Key::Space,
            physical_key: None,
            pressed,
            repeat,
            modifiers: Modifiers::NONE,
        }
    }

    /// One frame: the app's shortcuts, then a focused button. Returns
    /// whether the button saw a click.
    fn frame_with_focused_button(
        app: &mut SidAcidApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> bool {
        let id = egui::Id::new("focused button");
        ctx.memory_mut(|m| m.request_focus(id));
        let mut clicked = false;
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                app.handle_shortcuts(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    clicked = ui.add(egui::Button::new("Randomise")).clicked();
                    // The button's auto id differs; give the focus to it.
                    let r = ui.interact(ui.min_rect(), id, egui::Sense::click());
                    clicked |= r.clicked();
                });
            },
        );
        clicked
    }

    fn app() -> SidAcidApp {
        SidAcidApp::headless(crate::document::Document::new(
            acid_engine::demo::demo_project(),
        ))
    }

    fn plays(app: &SidAcidApp) -> usize {
        use acid_engine::host::Command;
        app.pending
            .iter()
            .filter(|c| matches!(c, Command::Play))
            .count()
    }

    #[test]
    fn space_plays_without_clicking_the_focused_widget() {
        let (mut app, ctx) = (app(), egui::Context::default());
        // A warm-up frame so the widget exists and holds focus.
        frame_with_focused_button(&mut app, &ctx, vec![]);
        let clicked = frame_with_focused_button(
            &mut app,
            &ctx,
            vec![space(true, false), space(false, false)],
        );
        assert!(!clicked, "Space also clicked the focused widget");
        assert_eq!(plays(&app), 1, "Space still starts playback");
    }

    #[test]
    fn holding_space_never_clicks_the_focused_widget() {
        let (mut app, ctx) = (app(), egui::Context::default());
        frame_with_focused_button(&mut app, &ctx, vec![]);
        let mut clicks = 0;
        let mut events = vec![space(true, false)];
        for _ in 0..5 {
            clicks += frame_with_focused_button(&mut app, &ctx, events) as usize;
            events = vec![space(true, true)];
        }
        clicks += frame_with_focused_button(&mut app, &ctx, vec![space(false, false)]) as usize;
        assert_eq!(clicks, 0);
        assert_eq!(plays(&app), 1, "auto-repeat doesn't toggle the transport");
    }

    #[test]
    fn space_types_into_a_focused_text_field() {
        let (mut app, ctx) = (app(), egui::Context::default());
        let id = egui::Id::new("rename");
        let mut text = String::from("Mono");
        let mut frame = |app: &mut SidAcidApp, events: Vec<egui::Event>| {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.handle_shortcuts(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut text).id(id));
                        r.request_focus();
                    });
                },
            );
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        frame(
            &mut app,
            vec![
                space(true, false),
                egui::Event::Text(" ".into()),
                space(false, false),
            ],
        );
        assert_eq!(text, "Mono ");
        assert_eq!(plays(&app), 0, "typing a space doesn't start playback");
    }
}
