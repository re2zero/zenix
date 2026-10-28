//! The prefix keymap: which keystroke arms the shell, and which command each
//! chord runs.
//!
//! The table is derived from [`super::commands::COMMANDS`] rather than kept
//! beside it, so a chord cannot reach a command the palette has never heard
//! of. The shape is herdr's own — a prefix key (Ctrl-B), then one chord — with
//! one exception: the palette also has a chord carrying a modifier of its own.
//!
//! ## Why the prefix
//!
//! The terminal owns the keyboard. A global `Ctrl-T` would steal a shell's own
//! job the moment focus landed in a pane, so every directory chord except the
//! three carrying their own modifier waits behind the prefix: the first press
//! arms the shell (and is consumed), the second reaches a command. Typing the
//! prefix twice sends it on to the pane instead of swallowing it, so an
//! application that binds Ctrl-B still sees it.
//!
//! Three chords carry their own modifier and are claimed outright: the
//! palette (`secondary-shift-p`: Ctrl on Linux and Windows, Command on the
//! Mac), the settings dialog (`secondary-,`) and the conversation panel
//! (`secondary-shift-a`). A terminal has no use for them, and each is promised
//! by a control that would otherwise lie.
//!
//! Both kinds of binding are matched in [`Root`]'s keystroke interceptor, not
//! through GPUI's keymap: the interceptor runs before focus dispatch, so an
//! armed prefix owns the next key wherever the keyboard is, and a claimed
//! chord is decided before a pane can type it. The cost is paid once per key:
//! the table has twenty entries.
//!
//! ## Spelling
//!
//! A chord is stored in GPUI's key syntax and parsed for the host once, when
//! the keymap is built. `secondary-` is resolved there rather than at match
//! time, and [`Keymap::label`] hands the raw spelling to the library's
//! `KbdCombo`, which resolves it again for the face it draws.

use gpui::{Keystroke, OperatingSystem};

use super::commands::{COMMANDS, Command};
use crate::plugins::actions::Catalog;
use crate::plugins::model::PluginAction;

/// The keystroke that arms the shell, as herdr's own default.
pub(crate) const PREFIX: &str = "ctrl-b";

/// What a chord runs: one of the directory's commands, or an action a plugin
/// exposed. The action is the catalog's entry, so the chord runs exactly what
/// the palette row offers and nothing the registry no longer declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Chord {
    Command(Command),
    Plugin(PluginAction),
}

/// One keystroke and the command it runs.
struct Binding {
    command: Command,
    keystroke: Keystroke,
    /// Whether the keystroke is reached after the prefix rather than claimed
    /// outright.
    prefixed: bool,
}

/// One keystroke a plugin declared, and the action behind it.
struct PluginBinding {
    action: PluginAction,
    keystroke: Keystroke,
    prefixed: bool,
}

/// Which keystrokes reach which command.
pub(crate) struct Keymap {
    /// The host whose modifier names the chords were resolved for, kept so a
    /// label spells the prefix the same way the key is matched.
    os: OperatingSystem,
    prefix: Keystroke,
    bindings: Vec<Binding>,
    /// The plugin half of the table. Plugins are read from the daemon, so this
    /// is rebuilt whenever the registry moves; the directory's half never is.
    plugins: Vec<PluginBinding>,
}

impl Keymap {
    /// Build the table for the host whose modifier names `secondary-` stands
    /// for. A chord the directory spells badly is dropped rather than taking
    /// the window down; the palette still offers its row.
    pub(crate) fn new(os: OperatingSystem) -> Self {
        let prefix = match parse(PREFIX, os) {
            Some(prefix) => prefix,
            // The built-in prefix is a static string; a parse failure is a
            // broken build, and an unparseable prefix would make every chord
            // unreachable. Keep the type honest without panicking in a paint.
            None => Keystroke {
                key: "b".into(),
                key_char: None,
                modifiers: gpui::Modifiers {
                    control: true,
                    ..Default::default()
                },
            },
        };
        let mut bindings = Vec::new();
        for info in COMMANDS {
            for key in info.prefixed {
                if let Some(keystroke) = parse(key, os) {
                    bindings.push(Binding {
                        command: info.command,
                        keystroke,
                        prefixed: true,
                    });
                } else {
                    tracing::warn!(
                        command = info.id,
                        chord = key,
                        "prefixed chord does not parse"
                    );
                }
            }
            for key in info.direct {
                if let Some(keystroke) = parse(key, os) {
                    bindings.push(Binding {
                        command: info.command,
                        keystroke,
                        prefixed: false,
                    });
                } else {
                    tracing::warn!(
                        command = info.id,
                        chord = key,
                        "direct chord does not parse"
                    );
                }
            }
        }
        Self {
            os,
            prefix,
            bindings,
            plugins: Vec::new(),
        }
    }

    /// Rebuild the plugin half of the table from the catalog. A declared chord
    /// the directory already owns is dropped, and so is one an earlier plugin
    /// declared — the same first-come order the catalog is built in, so the
    /// same registry read always produces the same table. A chord that does
    /// not parse is dropped rather than crashing the shell: the palette still
    /// offers the action.
    pub(crate) fn set_plugins(&mut self, catalog: &Catalog) {
        self.plugins.clear();
        for entry in catalog.entries() {
            let Some(key) = entry.chord() else { continue };
            let Some(keystroke) = parse(key, self.os) else {
                tracing::warn!(
                    plugin = %entry.action.plugin_id,
                    action = %entry.action.action_id,
                    chord = key,
                    "plugin chord does not parse"
                );
                continue;
            };
            let taken = self
                .bindings
                .iter()
                .any(|binding| same(&keystroke, &binding.keystroke))
                || self
                    .plugins
                    .iter()
                    .any(|binding| same(&keystroke, &binding.keystroke));
            if taken {
                tracing::info!(
                    plugin = %entry.action.plugin_id,
                    action = %entry.action.action_id,
                    chord = key,
                    "plugin chord is already taken; the palette row keeps the action"
                );
                continue;
            }
            let prefixed = is_bare(&keystroke);
            self.plugins.push(PluginBinding {
                action: entry.action.clone(),
                keystroke,
                prefixed,
            });
        }
    }

    /// Whether `typed` arms the shell.
    pub(crate) fn is_prefix(&self, typed: &Keystroke) -> bool {
        same(typed, &self.prefix)
    }

    /// The chord a keystroke runs when it follows the prefix.
    pub(crate) fn chord(&self, typed: &Keystroke) -> Option<Chord> {
        if let Some(command) = self
            .bindings
            .iter()
            .find(|binding| binding.prefixed && same(typed, &binding.keystroke))
            .map(|binding| binding.command)
        {
            return Some(Chord::Command(command));
        }
        self.plugins
            .iter()
            .find(|binding| binding.prefixed && same(typed, &binding.keystroke))
            .map(|binding| Chord::Plugin(binding.action.clone()))
    }

    /// The chord a keystroke runs on its own: only one carrying a modifier of
    /// its own is claimed this way, so nothing here shadows typing.
    pub(crate) fn claimed(&self, typed: &Keystroke) -> Option<Chord> {
        if let Some(command) = self
            .bindings
            .iter()
            .find(|binding| !binding.prefixed && same(typed, &binding.keystroke))
            .map(|binding| binding.command)
        {
            return Some(Chord::Command(command));
        }
        self.plugins
            .iter()
            .find(|binding| !binding.prefixed && same(typed, &binding.keystroke))
            .map(|binding| Chord::Plugin(binding.action.clone()))
    }

    /// The prefix as it is pressed, for the status bar's cap.
    pub(crate) fn prefix_label(&self) -> String {
        self.prefix.unparse(self.os)
    }

    /// A command's chord as the user presses it: the direct spelling when it
    /// has one, otherwise the prefix followed by the bare key. `None` for a
    /// command the directory leaves to the palette alone.
    pub(crate) fn label(&self, command: Command) -> Option<String> {
        let info = command.info()?;
        if let Some(direct) = info.direct.first() {
            return Some((*direct).to_owned());
        }
        info.prefixed
            .first()
            .map(|key| format!("{} {key}", self.prefix_label()))
    }

    /// A chord a plugin declared, as it is pressed: after the prefix when it
    /// is a bare key, alone when it carries a modifier of its own. The same
    /// rule that decides whether it is claimed outright or waits behind the
    /// prefix — see [`claimed`](Self::claimed).
    pub(crate) fn chord_label(&self, key: &str) -> String {
        match parse(key, self.os) {
            Some(keystroke) if is_bare(&keystroke) => {
                format!("{} {key}", self.prefix_label())
            }
            _ => key.to_owned(),
        }
    }

    /// The chord a plugin action actually won, as the manager draws it beside
    /// the action's row. `None` when the action declared no chord, or when the
    /// declaration's chord collided with one already bound — the palette still
    /// offers the action, and the row must not name a key that does nothing.
    pub(crate) fn plugin_chord_label(&self, plugin_id: &str, action_id: &str) -> Option<String> {
        let binding = self.plugins.iter().find(|binding| {
            binding.action.plugin_id == plugin_id && binding.action.action_id == action_id
        })?;
        let key = binding.keystroke.unparse(self.os);
        Some(if binding.prefixed {
            format!("{} {key}", self.prefix_label())
        } else {
            key
        })
    }
}

/// Whether a chord is a bare key, which is reached after the prefix. A chord
/// that carries a modifier of its own is claimed outright.
fn is_bare(keystroke: &Keystroke) -> bool {
    !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.platform
        && !keystroke.modifiers.function
}

/// One keystroke as the host spells it, or nothing when it does not parse.
fn parse(key: &str, os: OperatingSystem) -> Option<Keystroke> {
    Keystroke::parse_with_os(key, os).ok()
}

/// Whether a typed keystroke is the bound one. The chords here are literal
/// keys in a US layout, so modifier and key identity is the whole match; the
/// library's keyboard mapper is not in this path.
fn same(typed: &Keystroke, bound: &Keystroke) -> bool {
    typed.key == bound.key && typed.modifiers == bound.modifiers
}

#[cfg(test)]
mod tests {
    use gpui::OperatingSystem;

    use super::*;

    fn typed(key: &str) -> Keystroke {
        parse(key, OperatingSystem::Linux).expect("the test keystrokes parse")
    }

    #[test]
    fn the_built_in_prefix_parses_and_is_the_directories_own() {
        let keymap = Keymap::new(OperatingSystem::Linux);
        assert_eq!(keymap.prefix_label(), "ctrl-b");
        assert!(keymap.is_prefix(&typed("ctrl-b")));
        assert!(!keymap.is_prefix(&typed("ctrl-shift-b")));
    }

    #[test]
    fn every_chord_parses_and_is_unique() {
        let mut seen: Vec<(String, String)> = Vec::new();
        for info in COMMANDS {
            for key in info.direct.iter().chain(info.prefixed) {
                assert!(
                    parse(key, OperatingSystem::Linux).is_some(),
                    "{}: {key} does not parse",
                    info.id
                );
                let parsed = parse(key, OperatingSystem::Linux).expect("parsed above");
                let spelling = format!(
                    "{:?}+{:?}+{:?}+{:?}/{}",
                    parsed.modifiers.control,
                    parsed.modifiers.alt,
                    parsed.modifiers.shift,
                    parsed.modifiers.platform,
                    parsed.key
                );
                assert!(
                    !seen.iter().any(|(other, _)| other == &spelling),
                    "{}: {key} repeats {}",
                    info.id,
                    seen.iter()
                        .find(|(other, _)| other == &spelling)
                        .map(|(_, id)| id.as_str())
                        .unwrap_or_default()
                );
                seen.push((spelling, info.id.to_owned()));
            }
        }
    }

    #[test]
    fn a_chord_runs_the_command_the_directory_names() {
        let keymap = Keymap::new(OperatingSystem::Linux);
        assert_eq!(
            keymap.chord(&typed("t")),
            Some(Chord::Command(Command::NewTab))
        );
        assert_eq!(
            keymap.chord(&typed("shift-t")),
            Some(Chord::Command(Command::CloseTab))
        );
        assert_eq!(keymap.chord(&typed("q")), None);
        assert_eq!(keymap.chord(&typed("ctrl-b")), None);
    }

    /// The palette is the one chord claimed without the prefix; on Linux its
    /// `secondary-` is Control, on the Mac it is Command.
    #[test]
    fn the_palette_chord_is_claimed_outright() {
        let linux = Keymap::new(OperatingSystem::Linux);
        assert_eq!(
            linux.claimed(&typed("ctrl-shift-p")),
            Some(Chord::Command(Command::TogglePalette))
        );
        assert_eq!(linux.chord(&typed("ctrl-shift-p")), None);
        let mac = Keymap::new(OperatingSystem::Mac);
        let cmd = parse("cmd-shift-p", OperatingSystem::Mac).expect("cmd parses");
        assert_eq!(
            mac.claimed(&cmd),
            Some(Chord::Command(Command::TogglePalette))
        );
    }

    /// A row says the chord as it is pressed: the prefix is spelled out for a
    /// bare key, and a direct chord keeps its own spelling for the keycap
    /// renderer to resolve.
    #[test]
    fn a_label_is_the_chord_the_user_presses() {
        let keymap = Keymap::new(OperatingSystem::Linux);
        assert_eq!(keymap.label(Command::NewTab).as_deref(), Some("ctrl-b t"));
        assert_eq!(
            keymap.label(Command::TogglePalette).as_deref(),
            Some("secondary-shift-p")
        );
        assert_eq!(keymap.label(Command::NewWorktree), None);
    }

    fn action(key: &str) -> crate::plugins::model::PluginAction {
        crate::plugins::model::PluginAction {
            plugin_id: "board.app".into(),
            action_id: key.into(),
            title: key.into(),
            description: None,
            contexts: None,
        }
    }

    fn catalog_of(declarations: &str, actions: &[crate::plugins::model::PluginAction]) -> Catalog {
        let ui = crate::plugins::ui_manifest::parse(
            "board.app",
            std::path::Path::new("/tmp/board"),
            declarations,
        );
        Catalog::build(actions, &ui)
    }

    /// A plugin's chord follows herdr's rule: a bare key waits behind the
    /// prefix, a chord with its own modifier is claimed outright.
    #[test]
    fn plugin_chords_follow_the_prefix_rule() {
        let catalog = catalog_of(
            "zenix_ui_version = 1\n\
             [[command]]\nid = \"sync\"\naction = \"sync\"\ntitle = \"Sync\"\nkey = \"shift-y\"\n\
             [[command]]\nid = \"open\"\naction = \"open\"\ntitle = \"Open\"\nkey = \"ctrl-shift-l\"\n",
            &[action("sync"), action("open")],
        );
        let mut keymap = Keymap::new(OperatingSystem::Linux);
        keymap.set_plugins(&catalog);
        assert_eq!(
            keymap.chord_label("shift-y"),
            "ctrl-b shift-y",
            "a bare chord is shown after the prefix"
        );
        assert_eq!(keymap.chord_label("ctrl-shift-l"), "ctrl-shift-l");
        assert_eq!(
            keymap.chord(&typed("shift-y")),
            Some(Chord::Plugin(action("sync")))
        );
        assert_eq!(
            keymap.claimed(&typed("ctrl-shift-l")),
            Some(Chord::Plugin(action("open")))
        );
    }

    /// A plugin cannot take a chord the directory owns, and the first plugin
    /// to declare one keeps it: the table is built in catalog order.
    #[test]
    fn a_plugin_chord_never_displaces_the_directory_or_another_plugin() {
        let catalog = catalog_of(
            "zenix_ui_version = 1\n\
             [[command]]\nid = \"new-tab\"\naction = \"new-tab\"\ntitle = \"New tab\"\nkey = \"t\"\n\
             [[command]]\nid = \"sync\"\naction = \"sync\"\ntitle = \"Sync\"\nkey = \"shift-y\"\n\
             [[command]]\nid = \"sync-again\"\naction = \"sync-again\"\ntitle = \"Sync again\"\nkey = \"shift-y\"\n",
            &[action("new-tab"), action("sync"), action("sync-again")],
        );
        let mut keymap = Keymap::new(OperatingSystem::Linux);
        keymap.set_plugins(&catalog);
        // The directory's `t` still means New Tab.
        assert_eq!(
            keymap.chord(&typed("t")),
            Some(Chord::Command(Command::NewTab))
        );
        // The first declaration owns shift-y; the second keeps its row only.
        assert_eq!(
            keymap.chord(&typed("shift-y")),
            Some(Chord::Plugin(action("sync")))
        );
        let catalog = catalog_of("zenix_ui_version = 1\n", &[action("sync")]);
        keymap.set_plugins(&catalog);
        assert_eq!(keymap.chord(&typed("shift-y")), None, "a re-read drops it");
    }
}
