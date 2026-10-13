//! The prefix keymap: which keystroke arms the shell, and which command each
//! chord runs.
//!
//! Ported in shape from herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/keymap.rs`.
//! The daemon's `[keys]` table, custom commands, pane keys and the config's
//! `[keybindings]` overrides stay upstream: M5 wires configuration. What stays
//! here is the shape that matters — a command's shortcuts live in the
//! directory, so the palette, a menu and the prefix state machine cannot
//! disagree about what a chord runs.

use gpui_kit::{Action, App, KeyBinding, Keystroke};

use super::commands::{COMMANDS, Command};

/// The keystroke that arms the shell, as herdr's own default.
const PREFIX: &str = "ctrl-b";

/// Runs a directory command. One binding carries its own command, so the
/// keymap never has to look one up at dispatch time.
#[derive(Clone, PartialEq, serde::Deserialize, Action)]
#[action(no_json)]
pub(crate) struct RunCommand {
    pub(crate) command: Command,
}

/// Which keystrokes reach which command.
pub(crate) struct Keymap {
    prefix: Keystroke,
    /// Every prefixed chord, with the command it runs.
    chords: Vec<(Command, Keystroke)>,
}

impl Default for Keymap {
    fn default() -> Self {
        let prefix = Keystroke::parse(PREFIX).expect("the built-in prefix parses; see the tests");
        let mut chords = Vec::new();
        for info in COMMANDS {
            for key in info.prefixed {
                if let Ok(chord) = Keystroke::parse(key) {
                    chords.push((info.command, chord));
                }
            }
        }
        // Workspace numbers are generated rather than listed: nine entries in
        // the directory would be nine copies of the same command.
        for number in 1..=9u8 {
            if let Ok(chord) = Keystroke::parse(&number.to_string()) {
                chords.push((Command::WorkspaceNumber(number), chord));
            }
        }
        Self { prefix, chords }
    }
}

impl Keymap {
    /// Whether `typed` arms the shell. Any other bare key would swallow
    /// ordinary typing, so only a modified key or a function key qualifies.
    pub(crate) fn is_prefix(&self, typed: &Keystroke) -> bool {
        matches(typed, &self.prefix)
    }

    /// The command a chord runs when `typed` follows the prefix.
    pub(crate) fn chord(&self, typed: &Keystroke) -> Option<Command> {
        self.chords
            .iter()
            .find(|(_, chord)| matches(typed, chord))
            .map(|(command, _)| *command)
    }

    /// The prefix as shown to the user while it is armed.
    pub(crate) fn prefix_label(&self) -> String {
        self.prefix.unparse()
    }

    /// Every keystroke GPUI binds directly, with the command it runs. Prefix
    /// chords are not among them; the interceptor claims those.
    pub(crate) fn bindings(&self) -> Vec<(Command, String)> {
        let mut bindings = Vec::new();
        for info in COMMANDS {
            for keystroke in info.direct {
                bindings.push((info.command, (*keystroke).to_owned()));
            }
        }
        for number in 1..=9u8 {
            bindings.push((Command::TabNumber(number), format!("cmd-{number}")));
        }
        bindings
    }
}

/// Bind every direct keystroke to its command.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys(
        Keymap::default()
            .bindings()
            .into_iter()
            .map(|(command, keystroke)| KeyBinding::new(&keystroke, RunCommand { command }, None)),
    );
}

/// Whether a typed keystroke is the bound one. Upstream compares through
/// GPUI's own `should_match`, which also folds a layout's remapped character
/// in; the chords here are all literal keys in a US layout, so the modifier
/// and key identity is the whole match.
fn matches(typed: &Keystroke, bound: &Keystroke) -> bool {
    typed.key == bound.key && typed.modifiers == bound.modifiers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_prefix_parses() {
        assert!(Keystroke::parse(PREFIX).is_ok());
        let keymap = Keymap::default();
        assert_eq!(keymap.prefix_label(), "ctrl-b");
    }

    #[test]
    fn every_prefixed_chord_parses_and_is_unique() {
        let mut seen: Vec<String> = Vec::new();
        for info in COMMANDS {
            for key in info.prefixed {
                assert!(
                    Keystroke::parse(key).is_ok(),
                    "{}: {key} does not parse",
                    info.id
                );
                assert!(!seen.contains(&(*key).to_owned()), "{key}");
                seen.push((*key).to_owned());
            }
        }
        for number in 1..=9u8 {
            let key = number.to_string();
            assert!(!seen.contains(&key), "{key}");
        }
    }

    #[test]
    fn a_chord_runs_the_command_the_directory_names() {
        let keymap = Keymap::default();
        let n = Keystroke::parse("n").expect("n parses");
        assert_eq!(keymap.chord(&n), Some(Command::NextWorkspace));
        let three = Keystroke::parse("3").expect("3 parses");
        assert_eq!(keymap.chord(&three), Some(Command::WorkspaceNumber(3)));
        let unbound = Keystroke::parse("q").expect("q parses");
        assert_eq!(keymap.chord(&unbound), None);
    }

    #[test]
    fn the_prefix_arms_on_its_own_keystroke_only() {
        let keymap = Keymap::default();
        let prefix = Keystroke::parse(PREFIX).expect("the prefix parses");
        assert!(keymap.is_prefix(&prefix));
        let typed = Keystroke::parse("b").expect("b parses");
        assert!(!keymap.is_prefix(&typed));
    }

    #[test]
    fn a_capital_letter_chord_needs_the_shift_it_declares() {
        let keymap = Keymap::default();
        let shifted = Keystroke::parse("shift-h").expect("shift-h parses");
        assert_eq!(keymap.chord(&shifted), Some(Command::ResizeLeft));
        let plain = Keystroke::parse("h").expect("h parses");
        assert_eq!(keymap.chord(&plain), Some(Command::FocusLeft));
    }
}
