//! The prefix keymap: which keystroke arms the shell, and which command each
//! chord runs.
//!
//! Ported in shape from herdr-gpui (Apache-2.0) `crates/herdr-gpui/src/keymap.rs`.
//! The daemon's `[keys]` table, custom commands, pane keys and the config's
//! `[keybindings]` overrides stay upstream: M5 wires configuration. What stays
//! here is the shape that matters — a command's shortcuts live in the
//! directory, so the palette, a menu and the prefix state machine cannot
//! disagree about what a chord runs.
//!
//! A chord may also run a plugin action. Those are not in the directory: they
//! are read from the daemon, so the set changes while the shell runs and
//! [`Keymap::set_plugins`] rebuilds that half of the table whenever the plugin
//! registry is read. The rule for what a plugin chord claims is herdr's own
//! rule for a bare key: a chord that carries a modifier of its own
//! (`ctrl-shift-l`) is claimed outright, and a chord that is a bare key
//! (`y`, `shift-y`) is reached after the prefix — otherwise a plugin could
//! swallow the typing meant for the pane.

use gpui_kit::{Action, App, KeyBinding, Keystroke};

use super::commands::{COMMANDS, Command};
use crate::plugins::actions::Catalog;
use crate::plugins::model::PluginAction;

/// The keystroke that arms the shell, as herdr's own default.
const PREFIX: &str = "ctrl-b";

/// Runs a directory command. One binding carries its own command, so the
/// keymap never has to look one up at dispatch time.
#[derive(Clone, PartialEq, serde::Deserialize, Action)]
#[action(no_json)]
pub(crate) struct RunCommand {
    pub(crate) command: Command,
}

/// What a chord runs: one of the directory's commands, or an action a plugin
/// exposed. The action is the entry the catalog built, so the chord runs exactly
/// what the palette row offers and nothing that the registry no longer declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Chord {
    Command(Command),
    Plugin(PluginAction),
}

impl Chord {
    /// The plugin action this chord runs, when it runs one.
    pub(crate) fn plugin(&self) -> Option<&PluginAction> {
        match self {
            Self::Command(_) => None,
            Self::Plugin(action) => Some(action),
        }
    }

    /// The directory command this chord runs, when it runs one.
    pub(crate) fn command(&self) -> Option<Command> {
        match self {
            Self::Command(command) => Some(*command),
            Self::Plugin(_) => None,
        }
    }
}

/// One keystroke and what it runs.
struct Binding {
    chord: Chord,
    keystroke: Keystroke,
    /// Whether the keystroke is reached after the prefix rather than claimed
    /// outright. The directory's chords are all prefixed; a plugin's are
    /// prefixed when they are bare keys.
    prefixed: bool,
}

/// Which keystrokes reach which command.
pub(crate) struct Keymap {
    prefix: Keystroke,
    bindings: Vec<Binding>,
}

impl Default for Keymap {
    fn default() -> Self {
        let prefix = Keystroke::parse(PREFIX).expect("the built-in prefix parses; see the tests");
        let mut bindings = Vec::new();
        for info in COMMANDS {
            for key in info.prefixed {
                if let Ok(keystroke) = Keystroke::parse(key) {
                    bindings.push(Binding {
                        chord: Chord::Command(info.command),
                        keystroke,
                        prefixed: true,
                    });
                }
            }
        }
        // Workspace numbers are generated rather than listed: nine entries in
        // the directory would be nine copies of the same command.
        for number in 1..=9u8 {
            if let Ok(keystroke) = Keystroke::parse(&number.to_string()) {
                bindings.push(Binding {
                    chord: Chord::Command(Command::WorkspaceNumber(number)),
                    keystroke,
                    prefixed: true,
                });
            }
        }
        Self { prefix, bindings }
    }
}

impl Keymap {
    /// Whether `typed` arms the shell. Any other bare key would swallow
    /// ordinary typing, so only a modified key or a function key qualifies.
    pub(crate) fn is_prefix(&self, typed: &Keystroke) -> bool {
        matches(typed, &self.prefix)
    }

    /// The chord a keystroke runs when it follows the prefix: a directory
    /// command, or a plugin action declared on a bare key.
    pub(crate) fn chord(&self, typed: &Keystroke) -> Option<Chord> {
        self.bindings
            .iter()
            .find(|binding| binding.prefixed && matches(typed, &binding.keystroke))
            .map(|binding| binding.chord.clone())
    }

    /// The chord a keystroke runs on its own: only a plugin declares these, and
    /// only with a modifier of its own, so nothing here shadows typing.
    pub(crate) fn claimed(&self, typed: &Keystroke) -> Option<Chord> {
        self.bindings
            .iter()
            .find(|binding| !binding.prefixed && matches(typed, &binding.keystroke))
            .map(|binding| binding.chord.clone())
    }

    /// Rebuild the plugin half of the table from the catalog. A declared chord
    /// that a directory command already owns is dropped, and so is one an
    /// earlier plugin declared — the same first-come order the catalog is built
    /// in, so the same registry read always produces the same table.
    pub(crate) fn set_plugins(&mut self, catalog: &Catalog) {
        self.bindings
            .retain(|binding| binding.chord.plugin().is_none());
        for entry in catalog.entries() {
            let Some(key) = entry.chord() else { continue };
            let Some(keystroke) = parse(key) else {
                continue;
            };
            if self
                .bindings
                .iter()
                .any(|binding| matches(&keystroke, &binding.keystroke))
            {
                continue;
            }
            let prefixed = is_bare(&keystroke);
            self.bindings.push(Binding {
                chord: Chord::Plugin(entry.action.clone()),
                keystroke,
                prefixed,
            });
        }
    }

    /// The prefix as shown to the user while it is armed.
    pub(crate) fn prefix_label(&self) -> String {
        self.prefix.unparse()
    }

    /// A declared chord as it is pressed, which is what the palette shows beside
    /// the row: after the prefix when it is a bare key, alone otherwise.
    pub(crate) fn chord_label(&self, key: &str) -> String {
        match parse(key) {
            Some(keystroke) if is_bare(&keystroke) => format!("{} {key}", self.prefix_label()),
            _ => key.to_owned(),
        }
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

/// The label a directory command's first direct chord wears, the way the
/// chrome's key caps show it. A direct chord carries a modifier of its own —
/// that is what makes it direct rather than a chord behind the prefix — so the
/// label is the chord as the directory spells it.
pub(crate) fn direct_label(command: Command) -> Option<String> {
    let info = COMMANDS.iter().find(|info| info.command == command)?;
    Some((*info.direct.first()?).to_owned())
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

/// A plugin's chord as a keystroke, or nothing when it is not one. A
/// declaration is a plugin's text, not ours, so a chord that does not parse is
/// dropped rather than crashing the shell — the palette still shows the action,
/// and the trust review is where a bad declaration is reported.
fn parse(key: &str) -> Option<Keystroke> {
    Keystroke::parse(key).ok()
}

/// Whether a chord is a bare key, which is reached after the prefix. A chord
/// that carries a modifier of its own is claimed outright.
fn is_bare(keystroke: &Keystroke) -> bool {
    !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
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
    use crate::plugins::actions::Catalog;
    use crate::plugins::model::PluginAction;
    use crate::plugins::ui_manifest::UiContributions;

    fn typed(key: &str) -> Keystroke {
        Keystroke::parse(key).expect("the test keystrokes parse")
    }

    fn action(plugin_id: &str, action_id: &str) -> PluginAction {
        PluginAction {
            plugin_id: plugin_id.into(),
            action_id: action_id.into(),
            title: format!("{action_id} title"),
            description: None,
            contexts: None,
        }
    }

    fn catalog(text: &str) -> Catalog {
        let ui = crate::plugins::ui_manifest::parse(
            "board.app",
            std::path::Path::new("/tmp/board"),
            text,
        );
        Catalog::build(&[action("board.app", "sync")], &ui)
    }

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
        assert_eq!(
            keymap.chord(&typed("n")),
            Some(Chord::Command(Command::NextWorkspace))
        );
        assert_eq!(
            keymap.chord(&typed("3")),
            Some(Chord::Command(Command::WorkspaceNumber(3)))
        );
        assert_eq!(keymap.chord(&typed("q")), None);
    }

    #[test]
    fn the_prefix_arms_on_its_own_keystroke_only() {
        let keymap = Keymap::default();
        assert!(keymap.is_prefix(&typed(PREFIX)));
        assert!(!keymap.is_prefix(&typed("b")));
    }

    #[test]
    fn a_capital_letter_chord_needs_the_shift_it_declares() {
        let keymap = Keymap::default();
        assert_eq!(
            keymap.chord(&typed("shift-h")),
            Some(Chord::Command(Command::ResizeLeft))
        );
        assert_eq!(
            keymap.chord(&typed("h")),
            Some(Chord::Command(Command::FocusLeft))
        );
    }

    /// A declared chord with a modifier of its own is claimed outright, and one
    /// that is a bare key is reached after the prefix: a plugin may not swallow
    /// a keystroke the pane would have been typed into.
    #[test]
    fn a_plugin_chord_is_claimed_outright_when_it_carries_a_modifier() {
        let mut keymap = Keymap::default();
        keymap.set_plugins(&catalog(
            r#"
[[command]]
id = "sync"
title = "Sync"
action = "sync"
key = "ctrl-shift-l"
"#,
        ));
        assert_eq!(
            keymap.claimed(&typed("ctrl-shift-l")),
            Some(Chord::Plugin(action("board.app", "sync")))
        );
        assert_eq!(keymap.chord(&typed("ctrl-shift-l")), None);

        let mut keymap = Keymap::default();
        keymap.set_plugins(&catalog(
            r#"
[[command]]
id = "sync"
title = "Sync"
action = "sync"
key = "shift-y"
"#,
        ));
        assert_eq!(keymap.claimed(&typed("shift-y")), None);
        assert_eq!(
            keymap.chord(&typed("shift-y")),
            Some(Chord::Plugin(action("board.app", "sync")))
        );
    }

    /// The directory is read first, so a plugin cannot take a chord the shell
    /// already uses; and a second plugin cannot take one from the first.
    #[test]
    fn a_declared_chord_the_directory_already_owns_is_dropped() {
        let mut keymap = Keymap::default();
        keymap.set_plugins(&catalog(
            r#"
[[command]]
id = "sync"
title = "Sync"
action = "sync"
key = "n"
"#,
        ));
        assert_eq!(
            keymap.chord(&typed("n")),
            Some(Chord::Command(Command::NextWorkspace))
        );
    }

    /// The half of the table a plugin owns is rebuilt, not appended to: reading
    /// the registry again after an unlink leaves no chord behind.
    #[test]
    fn rebuilding_from_a_catalog_without_the_plugin_removes_its_chord() {
        let mut keymap = Keymap::default();
        let declared = catalog(
            r#"
[[command]]
id = "sync"
title = "Sync"
action = "sync"
key = "ctrl-shift-l"
"#,
        );
        keymap.set_plugins(&declared);
        assert!(keymap.claimed(&typed("ctrl-shift-l")).is_some());

        keymap.set_plugins(&Catalog::build(&[], &UiContributions::default()));
        assert!(keymap.claimed(&typed("ctrl-shift-l")).is_none());
        assert_eq!(
            keymap.chord(&typed("n")),
            Some(Chord::Command(Command::NextWorkspace)),
            "the directory's own chords survive the rebuild"
        );
    }

    /// A chord that does not parse is the plugin's mistake: the shell drops it
    /// rather than refusing to build a keymap, and the action still reaches the
    /// palette.
    #[test]
    fn a_declared_chord_that_does_not_parse_is_dropped_alone() {
        let mut keymap = Keymap::default();
        keymap.set_plugins(&catalog(
            r#"
[[command]]
id = "sync"
title = "Sync"
action = "sync"
key = "not a key"
"#,
        ));
        assert!(keymap.claimed(&typed("ctrl-shift-l")).is_none());
        assert_eq!(
            keymap.chord(&typed("n")),
            Some(Chord::Command(Command::NextWorkspace))
        );
    }
}
