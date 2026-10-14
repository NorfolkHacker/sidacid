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

impl SidAcidApp {
    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() || self.view.confirm.is_some() || self.view.message.is_some()
        {
            return;
        }
        let step_writing = self.view.panels.values().any(|p| p.writer.is_some());
        let pressed: Vec<Shortcut> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| event_shortcut(e, step_writing))
                .collect()
        });
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
}
