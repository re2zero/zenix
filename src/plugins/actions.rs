//! One plugin action, derived once for every surface this client offers it on.
//!
//! A plugin action can be reached three ways: a row in the command palette, the
//! chord its `zenix-ui.toml` asks for, and an MCP tool an agent calls. Those are
//! one action seen three times, so they come from one place — this catalog —
//! and a plugin that is unlinked, disabled or missing from the daemon's answer
//! disappears from all three at once. Nothing here draws or invokes: it pairs
//! the daemon's action with the declaration the plugin made for it, and names
//! the three projections.
//!
//! Two decisions belong here rather than to any one surface:
//!
//! * **The MCP name is a pure function of the ids.** MCP clients accept
//!   `[A-Za-z0-9_-]` (SEP-986 allows dots too, clients in the field do not), so
//!   the name is `plugin_` plus both ids folded to that set, shortened with a
//!   digest when it would still be too long. Building the catalog in one order
//!   (sorted by qualified id, first claim wins, a shared name gets a numbered
//!   suffix) is what makes the tool list and the tool call agree without either
//!   side remembering anything.
//! * **A declaration may be absent.** The daemon knows the action; only this
//!   client reads `zenix-ui.toml`. A plugin that declares no row still offers
//!   its action in the palette and as a tool, under the daemon's own title — the
//!   declaration is what adds a title of its own and a chord, never what makes
//!   the action exist.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::model::{Availability, InstalledPlugin, PluginAction};
use super::ui_manifest::{CommandSpec, UiContributions};

/// What every bridged tool's name starts with, so the MCP bridge cannot shadow
/// a built-in tool (`spawn_worker`, `list_workers`, …) and a name in a log is
/// recognisable as a plugin's.
const NAME_PREFIX: &str = "plugin_";

/// The longest tool name this catalog will produce. SEP-986 says 1–64; a client
/// that enforces something narrower is a client-side problem, and staying inside
/// the specification's own bound is the part that is ours.
const NAME_LIMIT: usize = 64;

/// One action, as all three surfaces see it.
pub(crate) struct Exposed {
    /// The action the daemon offers, which is what can actually run.
    pub(crate) action: PluginAction,
    /// The declaration behind it, when the plugin made one: where a row's own
    /// title and the chord come from.
    declared: Option<Arc<CommandSpec>>,
    /// The MCP tool name, assigned once so the list and the call agree.
    name: String,
}

impl Exposed {
    /// What a row is called: the declaration's title, else the daemon's.
    pub(crate) fn label(&self) -> &str {
        match &self.declared {
            Some(declared) => declared.title.as_str(),
            None => self.action.title.as_str(),
        }
    }

    /// The chord the plugin asked for, when it asked for one.
    pub(crate) fn chord(&self) -> Option<&str> {
        self.declared
            .as_ref()
            .and_then(|declared| declared.key.as_deref())
            .filter(|key| !key.trim().is_empty())
    }

    /// Whether this action may be offered in the current focus: the daemon's own
    /// contexts, and the declaration's `when` if there is one.
    pub(crate) fn is_offered(&self, availability: Availability) -> bool {
        if !self.action.is_available(availability) {
            return false;
        }
        match &self.declared {
            Some(declared) => availability.satisfies(&declared.when),
            None => true,
        }
    }

    /// The name this action is exposed to an MCP client under.
    pub(crate) fn tool_name(&self) -> &str {
        &self.name
    }
}

/// Every action the daemon reports, paired with what the plugins declare for
/// them, in one order.
pub(crate) struct Catalog {
    entries: Vec<Arc<Exposed>>,
}

impl Catalog {
    /// Pair the daemon's actions with the declarations. An action the daemon
    /// reports without a matching `[[command]]` is still here; a declaration
    /// naming no action the daemon reports is not, because nothing could run it.
    pub(crate) fn build(actions: &[PluginAction], ui: &UiContributions) -> Self {
        let mut sorted: Vec<&PluginAction> = actions.iter().collect();
        sorted.sort_by_key(|action| action.qualified_id());
        let mut taken: HashSet<String> = HashSet::with_capacity(sorted.len());
        let mut seen: HashSet<String> = HashSet::with_capacity(sorted.len());
        let mut entries = Vec::with_capacity(sorted.len());
        for action in sorted {
            let qualified = action.qualified_id();
            // Two actions cannot share an id; a daemon that says otherwise gets
            // the first one, so the catalog stays one entry per name.
            if !seen.insert(qualified) {
                continue;
            }
            let declared = ui
                .commands
                .iter()
                .find(|command| {
                    command.plugin_id == action.plugin_id && command.action_id == action.action_id
                })
                .cloned();
            let base = tool_name(&action.plugin_id, &action.action_id);
            let mut name = base.clone();
            let mut n = 2u32;
            while !taken.insert(name.clone()) {
                name = suffixed(&base, n);
                n += 1;
            }
            entries.push(Arc::new(Exposed {
                action: (*action).clone(),
                declared,
                name,
            }));
        }
        Self { entries }
    }

    /// Keep only the actions a plugin has enabled. The daemon lists a disabled
    /// plugin's actions anyway — measured on 0.9.5: `plugin.action.list` answers
    /// for one, and `plugin.action.invoke` refuses it with `plugin_disabled` — so
    /// offering them is this client's to refuse. The names stay as assigned, so
    /// enabling a plugin does not rename its tools.
    pub(crate) fn runnable(self, plugins: &[InstalledPlugin]) -> Self {
        let entries = self
            .entries
            .into_iter()
            .filter(|entry| {
                plugins
                    .iter()
                    .any(|plugin| plugin.enabled && plugin.plugin_id == entry.action.plugin_id)
            })
            .collect();
        Self { entries }
    }

    /// Every entry, in tool-name order.
    pub(crate) fn entries(&self) -> &[Arc<Exposed>] {
        &self.entries
    }

    /// The entries offered in this focus, in the same order.
    pub(crate) fn offered(
        &self,
        availability: Availability,
    ) -> impl Iterator<Item = &Arc<Exposed>> {
        self.entries
            .iter()
            .filter(move |entry| entry.is_offered(availability))
    }

    /// The entry exposed under a tool name, which is what a tool call names.
    pub(crate) fn named(&self, name: &str) -> Option<&Arc<Exposed>> {
        self.entries.iter().find(|entry| entry.tool_name() == name)
    }
}

/// The name one action is exposed under: `plugin_` plus its ids with every
/// character outside `[A-Za-z0-9_]` folded to `_`, shortened if that is still
/// longer than a tool name may be.
pub(crate) fn tool_name(plugin_id: &str, action_id: &str) -> String {
    shorten(&format!(
        "{NAME_PREFIX}{}_{}",
        escape(plugin_id),
        escape(action_id)
    ))
}

/// Fold a daemon-side id into the character set a tool name may use. Two ids
/// that fold together are told apart by [`Catalog::build`], not here.
fn escape(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Keep a long name inside the limit, with a digest of the whole name as its
/// tail so two names sharing a long prefix stay apart.
fn shorten(name: &str) -> String {
    if name.len() <= NAME_LIMIT {
        return name.to_owned();
    }
    let digest = digest(name);
    let keep = NAME_LIMIT - digest.len() - 1;
    let mut shortened: String = name.chars().take(keep).collect();
    shortened.push('_');
    shortened.push_str(&digest);
    shortened
}

fn suffixed(base: &str, n: u32) -> String {
    let suffix = format!("_{n}");
    let keep = NAME_LIMIT.saturating_sub(suffix.len());
    let mut name: String = base.chars().take(keep).collect();
    name.push_str(&suffix);
    name
}

fn digest(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::plugins::model::Context;
    use std::prelude::v1::test;

    fn action(plugin_id: &str, action_id: &str, title: &str) -> PluginAction {
        PluginAction {
            plugin_id: plugin_id.into(),
            action_id: action_id.into(),
            title: title.into(),
            description: None,
            contexts: None,
        }
    }

    fn plugin(id: &str) -> InstalledPlugin {
        InstalledPlugin {
            plugin_id: id.into(),
            name: id.into(),
            version: "0.1.0".into(),
            plugin_root: "/tmp/plugin".into(),
            description: None,
            enabled: true,
            panes: Vec::new(),
            events: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn ui(text: &str) -> UiContributions {
        crate::plugins::ui_manifest::parse("board.app", std::path::Path::new("/tmp/board"), text)
    }

    /// The three surfaces are three projections of one entry, so a declaration's
    /// title and chord reach the row and the chord map, and the tool name is the
    /// same string in the list and in a call.
    #[test]
    fn one_entry_names_its_row_chord_and_tool() {
        let catalog = Catalog::build(
            &[action("board.app", "sync", "Sync board")],
            &ui(r#"
[[command]]
id = "sync"
title = "Sync the board"
action = "sync"
key = "shift-y"
"#),
        );
        let entry = &catalog.entries()[0];
        assert_eq!(entry.label(), "Sync the board");
        assert_eq!(entry.chord(), Some("shift-y"));
        assert_eq!(entry.tool_name(), "plugin_board_app_sync");
        assert!(catalog.named("plugin_board_app_sync").is_some());
    }

    /// A plugin that declares nothing still offers its action: the daemon's
    /// title, no chord, and a tool name that is a function of the ids alone.
    #[test]
    fn an_undeclared_action_is_still_offered() {
        let catalog = Catalog::build(
            &[action("a.b", "sync", "Sync")],
            &UiContributions::default(),
        );
        let entry = &catalog.entries()[0];
        assert_eq!(entry.label(), "Sync");
        assert_eq!(entry.chord(), None);
        assert_eq!(entry.tool_name(), "plugin_a_b_sync");
    }

    /// The exit condition of §M11d, as a test: with the plugin gone, none of the
    /// three surfaces has anything left — they are built from this one list.
    #[test]
    fn removing_the_plugin_removes_the_row_the_chord_and_the_tool() {
        let declaration = ui(r#"
[[command]]
id = "sync"
title = "Sync the board"
action = "sync"
key = "shift-y"
"#);
        let present = Catalog::build(&[action("board.app", "sync", "Sync")], &declaration);
        assert_eq!(present.entries().len(), 1);
        assert!(present.entries()[0].chord().is_some());

        // The daemon no longer reports the action, which is what unlinking a
        // plugin looks like from here.
        let gone = Catalog::build(&[], &declaration);
        assert!(gone.entries().is_empty());
        assert!(gone.offered(Availability::of(None)).next().is_none());
        assert!(gone.named("plugin_board_app_sync").is_none());
    }

    /// A disabled plugin still has its actions listed by the daemon; the client
    /// offers none of them, and the ones it does offer keep their names.
    #[test]
    fn a_disabled_plugins_actions_are_not_offered() {
        let actions = [action("a.b", "one", "One"), action("c.d", "two", "Two")];
        let enabled = InstalledPlugin {
            plugin_id: "c.d".into(),
            enabled: true,
            ..plugin("c.d")
        };
        let disabled = InstalledPlugin {
            plugin_id: "a.b".into(),
            enabled: false,
            ..plugin("a.b")
        };
        let catalog = Catalog::build(&actions, &UiContributions::default())
            .runnable(&[disabled.clone(), enabled.clone()]);
        assert_eq!(catalog.entries().len(), 1);
        assert_eq!(catalog.entries()[0].tool_name(), "plugin_c_d_two");
        assert!(catalog.named("plugin_a_b_one").is_none());

        // Both enabled: both are offered, and the disabled one's name was never
        // given away.
        let both = Catalog::build(&actions, &UiContributions::default()).runnable(&[
            InstalledPlugin {
                enabled: true,
                ..disabled
            },
            enabled,
        ]);
        assert_eq!(both.entries().len(), 2);
        assert!(both.named("plugin_a_b_one").is_some());
    }

    /// A declaration narrows where its action is offered; the daemon's contexts
    /// still apply underneath it.
    #[test]
    fn a_declaration_can_narrow_where_its_action_is_offered() {
        let catalog = Catalog::build(
            &[action("board.app", "sync", "Sync")],
            &ui(r#"
[[command]]
id = "sync"
title = "Sync"
action = "sync"
when = ["selection"]
"#),
        );
        let entry = &catalog.entries()[0];
        assert!(!entry.is_offered(Availability::of(None)));
        let _ = Context::Selection;
    }

    /// Two ids that fold to the same characters still get distinct tool names,
    /// and the second one's name is stable.
    #[test]
    fn names_that_fold_together_are_told_apart() {
        let catalog = Catalog::build(
            &[action("a.b", "x-y", "one"), action("a:b", "x.y", "two")],
            &UiContributions::default(),
        );
        let names: Vec<&str> = catalog
            .entries()
            .iter()
            .map(|entry| entry.tool_name())
            .collect();
        assert_eq!(names.len(), 2);
        assert_ne!(names[0], names[1]);
        assert!(names.iter().all(|name| name.len() <= NAME_LIMIT));
        assert_eq!(names[0], "plugin_a_b_x_y");
    }
}
