//! Global keyboard shortcuts.

use super::SidAcidApp;
use super::files::Pending;
use acid_engine::host::Command;
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

impl SidAcidApp {
    pub(crate) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let step_writing = self.view.writers.iter().any(Option::is_some);
        let pressed: Vec<Shortcut> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => shortcut(*key, *modifiers, step_writing),
                    // Some platforms deliver Ctrl+C / Ctrl+V as clipboard events.
                    egui::Event::Copy => Some(Shortcut::Copy),
                    egui::Event::Paste(_) => Some(Shortcut::Paste),
                    _ => None,
                })
                .collect()
        });
        for s in pressed {
            match s {
                Shortcut::PlayStop => {
                    let cmd = if self.status.playing {
                        Command::Stop
                    } else {
                        Command::Play
                    };
                    self.send(vec![cmd]);
                }
                Shortcut::Save => {
                    self.save(false);
                }
                Shortcut::Open => self.request(Pending::Open),
                Shortcut::New => self.request(Pending::New),
                Shortcut::Undo => {
                    let cmds = self.doc.undo();
                    self.send(cmds);
                }
                Shortcut::Redo => {
                    let cmds = self.doc.redo();
                    self.send(cmds);
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
}
