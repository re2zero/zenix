//! `zenix-ui.toml`: what a plugin declares for this host to draw, and the one
//! rule that governs reading it — anything the host does not understand is
//! downgraded or skipped, never fatal.
//!
//! The daemon neither reads nor validates this file (design §2.2 of
//! `.trellis/tasks/10-12-ui-plugin-architecture/`); it is this host's own
//! contract with plugin authors, versioned apart from the plugin manifest so a
//! plugin can ship UI for a client it knows about and lose it cleanly on a
//! client that has moved on. Nothing here executes a plugin: a declaration
//! names a herdr action to invoke, a command whose stdout is read, or a view to
//! draw a command's JSON in.
//!
//! Reading is where the compatibility promises live, so they are all in this
//! file: an unknown `view` falls back to the key/value view, an unknown
//! `placement` to the drawer, an unknown context to nothing, an unknown field
//! to being ignored, and a file that speaks a newer schema or asks for a newer
//! client is skipped whole. Every downgrade that changes what the author asked
//! for leaves a warning behind, which is how a plugin author finds it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use super::model::{Context, InstalledPlugin};

/// The file a plugin puts beside its `herdr-plugin.toml`. Absent means the
/// plugin declares no UI, which is not an error.
pub(crate) const FILE_NAME: &str = "zenix-ui.toml";

/// The schema this client speaks. A file declaring a newer version is skipped
/// whole: a newer file may mean anything by the fields it keeps.
pub(crate) const SCHEMA_VERSION: u32 = 1;

/// This client's version, for the `min_zenix_version` gate.
pub(crate) const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Everything one plugin's UI file contributes, in the order it was declared.
/// The specs are shared rather than copied because a contribution mounted in
/// the shell outlives the frame that built it.
#[derive(Clone, Debug, Default)]
pub(crate) struct UiContributions {
    pub(crate) commands: Vec<Arc<CommandSpec>>,
    pub(crate) sidebar_items: Vec<Arc<SidebarItemSpec>>,
    pub(crate) panels: Vec<Arc<PanelSpec>>,
    pub(crate) status_items: Vec<Arc<StatusItemSpec>>,
    /// What reading the file had to downgrade or skip, and why. An unknown
    /// field is not in here: being ignored is what "unknown field" means.
    pub(crate) warnings: Vec<String>,
}

impl UiContributions {
    /// Read `zenix-ui.toml` out of one plugin's root. A missing file is an
    /// empty contribution set; anything else the host cannot use is a warning
    /// on an otherwise empty set, so one bad file never takes the plugin's
    /// actions or panes down with it.
    pub(crate) fn read(plugin_id: &str, plugin_root: &Path) -> Self {
        let path = plugin_root.join(FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => parse(plugin_id, plugin_root, &text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => skipped(plugin_id, format!("{FILE_NAME} could not be read: {error}")),
        }
    }

    /// The same, over text, for a caller that already has it.
    pub(crate) fn parse(plugin_id: &str, plugin_root: &Path, text: &str) -> Self {
        parse(plugin_id, plugin_root, text)
    }

    /// Whether anything at all would be mounted from this file.
    pub(crate) fn is_empty(&self) -> bool {
        self.commands.is_empty()
            && self.sidebar_items.is_empty()
            && self.panels.is_empty()
            && self.status_items.is_empty()
    }

    /// The panel a `[[sidebar_item]]` names, if this file declares it.
    pub(crate) fn panel(&self, local_id: &str) -> Option<&Arc<PanelSpec>> {
        self.panels.iter().find(|panel| panel.local_id == local_id)
    }
}

/// What every spec has in common: the plugin it belongs to, which kind of
/// contribution it is, and the local id the plugin gave it. The three together
/// make the one name the shell knows it by.
pub(crate) trait Declared {
    fn plugin_id(&self) -> &str;
    fn kind(&self) -> &'static str;
    fn local_id(&self) -> &str;

    /// The idempotent key of a mounted contribution: two plugins can both
    /// declare a `board` panel, and a reload of the same file produces the same
    /// key (design §2.2).
    fn qualified_id(&self) -> String {
        format!("{}:{}:{}", self.plugin_id(), self.kind(), self.local_id())
    }
}

impl Declared for CommandSpec {
    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    fn kind(&self) -> &'static str {
        "command"
    }

    fn local_id(&self) -> &str {
        &self.local_id
    }
}

impl Declared for SidebarItemSpec {
    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    fn kind(&self) -> &'static str {
        "sidebar"
    }

    fn local_id(&self) -> &str {
        &self.local_id
    }
}

impl Declared for PanelSpec {
    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    fn kind(&self) -> &'static str {
        "panel"
    }

    fn local_id(&self) -> &str {
        &self.local_id
    }
}

impl Declared for StatusItemSpec {
    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    fn kind(&self) -> &'static str {
        "status"
    }

    fn local_id(&self) -> &str {
        &self.local_id
    }
}

/// Fold every installed plugin's declared UI into the one set the shell mounts:
/// plugins in the daemon's order, sidebar items in the order they asked for, and
/// a duplicated id dropped in favour of the first declaration. A disabled plugin
/// contributes nothing, exactly as its actions are not offered.
pub(crate) fn mounted_ui(plugins: &[InstalledPlugin]) -> UiContributions {
    let mut mounted = UiContributions::default();
    for plugin in plugins.iter().filter(|plugin| plugin.enabled) {
        let declared = UiContributions::read(&plugin.plugin_id, Path::new(&plugin.plugin_root));
        mounted.warnings.extend(declared.warnings);
        mounted.commands.extend(declared.commands);
        mounted.sidebar_items.extend(declared.sidebar_items);
        mounted.panels.extend(declared.panels);
        mounted.status_items.extend(declared.status_items);
    }
    let mut warnings = std::mem::take(&mut mounted.warnings);
    mounted.commands = dedupe(mounted.commands, &mut warnings);
    mounted.sidebar_items = dedupe(mounted.sidebar_items, &mut warnings);
    mounted.panels = dedupe(mounted.panels, &mut warnings);
    mounted.status_items = dedupe(mounted.status_items, &mut warnings);
    // A stable sort, so two items asking for the same order keep the order their
    // plugins were read in.
    mounted.sidebar_items.sort_by_key(|item| item.order);
    mounted.warnings = warnings;
    mounted
}

fn dedupe<T: Declared>(items: Vec<Arc<T>>, warnings: &mut Vec<String>) -> Vec<Arc<T>> {
    let mut seen = HashSet::new();
    let mut kept = Vec::with_capacity(items.len());
    for item in items {
        let id = item.qualified_id();
        if seen.insert(id.clone()) {
            kept.push(item);
        } else {
            warnings.push(format!(
                "duplicate contribution \"{id}\"; the first declaration wins"
            ));
        }
    }
    kept
}

/// A row in the command palette, bound to an action the plugin's manifest
/// declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandSpec {
    pub(crate) plugin_id: String,
    pub(crate) local_id: String,
    pub(crate) title: String,
    /// The `[[actions]]` id in `herdr-plugin.toml` this row invokes.
    pub(crate) action_id: String,
    /// The states the row is offered in, in the daemon's own vocabulary. Empty
    /// means what it means for an action: always.
    pub(crate) when: Vec<Context>,
    /// The chord the author would like the row reachable by. The host binds it
    /// as declared when it carries a modifier of its own (`ctrl-shift-l`), and
    /// after the shell's prefix when it is a bare key (`y`) — a bare key bound
    /// outright would swallow the pane's typing. A chord a shell command already
    /// owns is dropped.
    pub(crate) key: Option<String>,
}

/// A line in the sidebar that opens one of the plugin's panels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SidebarItemSpec {
    pub(crate) plugin_id: String,
    pub(crate) local_id: String,
    pub(crate) title: String,
    /// Where among the sidebar's own blocks the item asks to sit. Ordering is
    /// the host's, so this is a hint the host sorts by, not a promise.
    pub(crate) order: i64,
    /// The `[[panel]]` local id this item opens.
    pub(crate) panel_id: Option<String>,
}

/// A panel of one of the built-in view kinds, fed by one data source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PanelSpec {
    pub(crate) plugin_id: String,
    pub(crate) local_id: String,
    pub(crate) title: String,
    /// The plugin's own directory. It is where a source command runs, exactly as
    /// it is for the daemon's own plugin commands, so a declaration can name a
    /// script beside its manifest.
    pub(crate) plugin_root: PathBuf,
    pub(crate) view: ViewKind,
    pub(crate) source: Option<Source>,
}

/// A segment of the status bar, formatted from one data source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StatusItemSpec {
    pub(crate) plugin_id: String,
    pub(crate) local_id: String,
    /// The plugin's own directory, as for a panel's source.
    pub(crate) plugin_root: PathBuf,
    /// The text to show, with `{field}` holes filled from the source's JSON
    /// object. A hole with no field behind it stays a hole.
    pub(crate) format: String,
    pub(crate) source: Option<Source>,
}

/// How a panel's JSON is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewKind {
    /// An array, one row per element.
    List,
    /// `{columns: [...], rows: [[...]]}`, or an array of objects.
    Table,
    /// An object, one row per key.
    Kv,
    /// Text, or an array of lines.
    Log,
}

impl ViewKind {
    /// The declared name, or the downgrade. `form` is accepted because schema
    /// v1 lists it, and drawn as a key/value view until a form has somewhere to
    /// submit to.
    fn of(declared: Option<&str>) -> (Self, Option<String>) {
        match declared {
            Some("list") => (Self::List, None),
            Some("table") => (Self::Table, None),
            Some("kv") => (Self::Kv, None),
            Some("log") => (Self::Log, None),
            Some("form") => (
                Self::Kv,
                Some("view \"form\" has nowhere to submit yet; drawn as kv".to_owned()),
            ),
            Some(other) => (
                Self::Kv,
                Some(format!("unknown view \"{other}\"; drawn as kv")),
            ),
            None => (Self::Kv, None),
        }
    }
}

/// Where a panel's data comes from: one command's stdout, parsed as JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Source {
    pub(crate) command: Vec<String>,
}

/// The raw file, as TOML hands it over. Every collection is optional and every
/// field a plugin's file may carry beyond these is ignored, which is the
/// forward-compatibility promise in practice.
#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    zenix_ui_version: Option<u32>,
    #[serde(default)]
    min_zenix_version: Option<String>,
    #[serde(default, rename = "command")]
    commands: Vec<RawCommand>,
    #[serde(default, rename = "sidebar_item")]
    sidebar_items: Vec<RawSidebarItem>,
    #[serde(default, rename = "panel")]
    panels: Vec<RawPanel>,
    #[serde(default, rename = "status_item")]
    status_items: Vec<RawStatusItem>,
    /// Only counted: a plugin settings section has no shipped consumer yet, so
    /// the declared shape is not interpreted here at all.
    #[serde(default, rename = "settings_section")]
    settings_sections: Vec<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawCommand {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    when: Vec<String>,
    #[serde(default)]
    key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawSidebarItem {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    order: Option<i64>,
    #[serde(default)]
    panel: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPanel {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    placement: Option<String>,
    #[serde(default)]
    view: Option<String>,
    #[serde(default)]
    source: Option<RawSource>,
}

#[derive(Debug, Deserialize)]
struct RawStatusItem {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    source: Option<RawSource>,
}

#[derive(Debug, Deserialize)]
struct RawSource {
    #[serde(default)]
    command: Option<Vec<String>>,
    #[serde(default)]
    event: Option<String>,
}

/// The placements this client draws. Any other — including the `overlay` and
/// `bottom` schema v1 lists — mounts as a drawer, because a drawer is the one
/// container the shell has (the plugin's own *panes* are where overlay drawing
/// lives, and the daemon owns those).
const DRAWN_PLACEMENT: &str = "drawer";

fn parse(plugin_id: &str, plugin_root: &Path, text: &str) -> UiContributions {
    let raw: RawManifest = match toml::from_str(text) {
        Ok(raw) => raw,
        Err(error) => return skipped(plugin_id, format!("{FILE_NAME} was not read: {error}")),
    };
    let mut warnings = Vec::new();

    if let Some(declared) = raw.zenix_ui_version.filter(|v| *v > SCHEMA_VERSION) {
        return skipped(
            plugin_id,
            format!(
                "{FILE_NAME} declares zenix_ui_version {declared}; this client speaks \
                 {SCHEMA_VERSION}, so its UI was skipped"
            ),
        );
    }
    // A version the author asked for that this client cannot compare against is
    // not a reason to refuse the file, so an unparsable value only warns.
    match raw.min_zenix_version.as_deref() {
        Some(required) if !required.trim().is_empty() => match parse_version(required) {
            Some(minimum) => {
                let core = parse_version(CORE_VERSION).expect("this crate's version parses");
                if minimum > core {
                    return skipped(
                        plugin_id,
                        format!(
                            "{FILE_NAME} asks for zenix {required}; this client is \
                             {CORE_VERSION}, so its UI was skipped"
                        ),
                    );
                }
            }
            None => warnings.push(format!(
                "min_zenix_version \"{required}\" is not a version; ignored"
            )),
        },
        _ => {}
    }

    // Read before the sidebar items, because a sidebar item may name a panel
    // declared anywhere in the file.
    let declared_panels: Vec<String> = raw
        .panels
        .iter()
        .filter_map(|panel| panel.id.clone())
        .collect();
    let mut ui = UiContributions::default();
    for (index, item) in raw.commands.into_iter().enumerate() {
        let Some(local_id) = identified("command", index, item.id, &mut warnings) else {
            continue;
        };
        let Some(action_id) = item.action.filter(|action| !action.is_empty()) else {
            warnings.push(format!("command \"{local_id}\" names no action; skipped"));
            continue;
        };
        let title = item
            .title
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| local_id.clone());
        let key = item.key.filter(|key| !key.trim().is_empty());
        ui.commands.push(Arc::new(CommandSpec {
            plugin_id: plugin_id.to_owned(),
            local_id,
            title,
            action_id,
            when: contexts(&item.when, &mut warnings),
            key,
        }));
    }

    for (index, item) in raw.sidebar_items.into_iter().enumerate() {
        let Some(local_id) = identified("sidebar_item", index, item.id, &mut warnings) else {
            continue;
        };
        let title = item
            .title
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| local_id.clone());
        let panel_id = item.panel.filter(|panel| !panel.is_empty());
        if let Some(panel_id) = panel_id
            .as_deref()
            .filter(|panel| !declared_panels.iter().any(|declared| declared == panel))
        {
            warnings.push(format!(
                "sidebar_item \"{local_id}\" opens \"{panel_id}\", which this file does not declare"
            ));
        }
        ui.sidebar_items.push(Arc::new(SidebarItemSpec {
            plugin_id: plugin_id.to_owned(),
            local_id,
            title,
            order: item.order.unwrap_or(0),
            panel_id,
        }));
    }

    for (index, item) in raw.panels.into_iter().enumerate() {
        let Some(local_id) = identified("panel", index, item.id, &mut warnings) else {
            continue;
        };
        let title = item
            .title
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| local_id.clone());
        match item.placement.as_deref() {
            Some(DRAWN_PLACEMENT) | None => {}
            Some(other) => warnings.push(format!(
                "panel \"{local_id}\" asks for placement \"{other}\", which this client does \
                 not draw; mounted as a drawer"
            )),
        }
        let (view, downgrade) = ViewKind::of(item.view.as_deref());
        if let Some(downgrade) = downgrade {
            warnings.push(format!("panel \"{local_id}\": {downgrade}"));
        }
        ui.panels.push(Arc::new(PanelSpec {
            plugin_id: plugin_id.to_owned(),
            local_id,
            title,
            plugin_root: plugin_root.to_path_buf(),
            view,
            source: source(item.source, "panel", &mut warnings),
        }));
    }

    for (index, item) in raw.status_items.into_iter().enumerate() {
        let Some(local_id) = identified("status_item", index, item.id, &mut warnings) else {
            continue;
        };
        let Some(format) = item.format.filter(|format| !format.trim().is_empty()) else {
            warnings.push(format!("status_item \"{local_id}\" has no format; skipped"));
            continue;
        };
        ui.status_items.push(Arc::new(StatusItemSpec {
            plugin_id: plugin_id.to_owned(),
            local_id,
            plugin_root: plugin_root.to_path_buf(),
            format,
            source: source(item.source, "status_item", &mut warnings),
        }));
    }

    if !raw.settings_sections.is_empty() {
        warnings.push(format!(
            "{} settings section(s) declared; plugin settings are not mounted yet",
            raw.settings_sections.len()
        ));
    }

    ui.warnings = warnings
        .into_iter()
        .map(|warning| format!("{plugin_id}: {warning}"))
        .collect();
    ui
}

/// One declaration's local id, or the reason it has none worth mounting.
fn identified(
    kind: &str,
    index: usize,
    id: Option<String>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let Some(id) = id.filter(|id| !id.is_empty()) else {
        warnings.push(format!("{kind} #{index} has no id; skipped"));
        return None;
    };
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        warnings.push(format!(
            "{kind} \"{id}\" is not ascii letters, digits, '_' or '-'; skipped"
        ));
        return None;
    }
    Some(id)
}

/// The contexts a declaration scopes itself to, or nothing for the names this
/// client does not know — the daemon's vocabulary is the only one there is.
fn contexts(declared: &[String], warnings: &mut Vec<String>) -> Vec<Context> {
    let mut contexts = Vec::new();
    for name in declared {
        match name.as_str() {
            "global" => contexts.push(Context::Global),
            "workspace" => contexts.push(Context::Workspace),
            "tab" => contexts.push(Context::Tab),
            "pane" => contexts.push(Context::Pane),
            "selection" => contexts.push(Context::Selection),
            other => warnings.push(format!("unknown context \"{other}\"; ignored")),
        }
    }
    contexts
}

/// A declaration's data source. Only a command is read in schema v1: an event
/// subscription needs the daemon's event lane, which the host does not carry
/// yet, and a source that would never fire is worse than no source.
fn source(raw: Option<RawSource>, kind: &str, warnings: &mut Vec<String>) -> Option<Source> {
    let raw = raw?;
    let command = raw.command.filter(|command| !command.is_empty());
    if let Some(event) = raw
        .event
        .as_deref()
        .filter(|event| !event.trim().is_empty())
    {
        if command.is_some() {
            warnings.push(format!(
                "{kind} declares both a command and the event \"{event}\"; the command is used"
            ));
        } else {
            warnings.push(format!(
                "{kind} subscribes to \"{event}\", which this client cannot subscribe to yet; \
                 no data will be read"
            ));
            return None;
        }
    }
    command.map(|command| Source { command })
}

/// A file that was read and then skipped whole, for the one reason given.
fn skipped(plugin_id: &str, reason: String) -> UiContributions {
    UiContributions {
        warnings: vec![format!("{plugin_id}: {reason}")],
        ..Default::default()
    }
}

/// `major.minor.patch`, with any suffix ignored. Enough to compare against the
/// declared minimum, and deliberately not a semver crate: the gate is a gate,
/// not a dependency.
fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let core = text.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin root a declaration's source command would run in. Nothing
    /// here reads it: the specs only carry it.
    fn root() -> &'static Path {
        Path::new("/tmp/zenix-ui-root")
    }

    const FIXTURE: &str = r#"
zenix_ui_version = 1
min_zenix_version = "0.1.0"
future_field = "ignored"

[[command]]
id = "apply"
title = "Apply project layout"
action = "apply"
when = ["workspace"]
key = "ctrl-shift-l"

[[sidebar_item]]
id = "board"
title = "Project board"
order = 50
panel = "board"

[[panel]]
id = "board"
title = "Project board"
placement = "drawer"
view = "table"
[panel.source]
command = ["sh", "board.sh"]

[[status_item]]
id = "wip"
source = { command = ["sh", "wip.sh"] }
format = "{count} WIP"
"#;

    #[test]
    fn a_missing_file_contributes_nothing_and_warns_about_nothing() {
        let ui = UiContributions::read("a.b", Path::new("/nonexistent/plugin-root"));
        assert!(ui.is_empty());
        assert!(ui.warnings.is_empty());
    }

    #[test]
    fn a_full_file_reads_as_declared() {
        let ui = UiContributions::parse("a.b", root(), FIXTURE);
        assert!(ui.warnings.is_empty(), "{:?}", ui.warnings);
        assert_eq!(ui.commands.len(), 1);
        let command = &ui.commands[0];
        assert_eq!(command.local_id, "apply");
        assert_eq!(command.action_id, "apply");
        assert_eq!(command.when, vec![Context::Workspace]);
        assert_eq!(command.key.as_deref(), Some("ctrl-shift-l"));
        assert_eq!(ui.sidebar_items.len(), 1);
        assert_eq!(ui.sidebar_items[0].order, 50);
        assert_eq!(ui.sidebar_items[0].panel_id.as_deref(), Some("board"));
        assert_eq!(ui.panels.len(), 1);
        assert_eq!(ui.panels[0].view, ViewKind::Table);
        assert_eq!(
            ui.panels[0].source.as_ref().map(|s| s.command.len()),
            Some(2)
        );
        assert_eq!(ui.status_items.len(), 1);
        assert_eq!(ui.status_items[0].format, "{count} WIP");
        assert_eq!(
            ui.panel("board").map(|panel| panel.title.as_str()),
            Some("Project board")
        );
    }

    /// A file from a future client is skipped whole, and says so, rather than
    /// being half-read under today's rules.
    #[test]
    fn a_newer_schema_version_skips_the_whole_file() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            "zenix_ui_version = 2\n\n[[command]]\nid = \"x\"\naction = \"x\"\n",
        );
        assert!(ui.is_empty());
        assert_eq!(ui.warnings.len(), 1);
        assert!(
            ui.warnings[0].contains("zenix_ui_version 2"),
            "{:?}",
            ui.warnings
        );
    }

    #[test]
    fn a_file_asking_for_a_newer_client_is_skipped_whole() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            "min_zenix_version = \"99.0.0\"\n\n[[panel]]\nid = \"x\"\n",
        );
        assert!(ui.is_empty());
        assert!(ui.warnings[0].contains("99.0.0"), "{:?}", ui.warnings);
        // The current client's own version is the gate, and it passes.
        let current = UiContributions::parse(
            "a.b",
            root(),
            "min_zenix_version = \"0.1.0\"\n\n[[panel]]\nid = \"x\"\n",
        );
        assert_eq!(current.panels.len(), 1, "{:?}", current.warnings);
        assert!(current.warnings.is_empty());
    }

    #[test]
    fn an_unparsable_minimum_version_only_warns() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            "min_zenix_version = \"soon\"\n\n[[panel]]\nid = \"x\"\n",
        );
        assert_eq!(ui.panels.len(), 1);
        assert_eq!(ui.warnings.len(), 1);
        assert!(
            ui.warnings[0].contains("not a version"),
            "{:?}",
            ui.warnings
        );
    }

    /// The downgrade rule, in the three shapes it takes: an unknown view, a
    /// view this client lists but cannot draw yet, and an unknown placement.
    #[test]
    fn unknown_views_and_placements_are_downgraded_with_a_warning() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            r#"
[[panel]]
id = "sparkline"
view = "sparkline"
placement = "floating"

[[panel]]
id = "form"
view = "form"
"#,
        );
        assert_eq!(ui.panels.len(), 2);
        assert_eq!(ui.panels[0].view, ViewKind::Kv);
        assert_eq!(ui.panels[1].view, ViewKind::Kv);
        assert_eq!(ui.warnings.len(), 3, "{:?}", ui.warnings);
        assert!(ui.warnings[0].contains("floating"));
        assert!(ui.warnings[1].contains("sparkline"));
        assert!(ui.warnings[2].contains("form"));
    }

    #[test]
    fn a_panel_without_a_view_is_drawn_as_key_value_without_a_warning() {
        let ui = UiContributions::parse("a.b", root(), "[[panel]]\nid = \"x\"\n");
        assert_eq!(ui.panels[0].view, ViewKind::Kv);
        assert!(ui.warnings.is_empty(), "{:?}", ui.warnings);
    }

    #[test]
    fn unknown_contexts_are_dropped_and_unknown_top_level_fields_ignored() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            r#"
unknown_table = { hello = "world" }

[[command]]
id = "x"
action = "x"
when = ["workspace", "galaxy"]
also_unknown = 3
"#,
        );
        assert_eq!(ui.commands[0].when, vec![Context::Workspace]);
        assert_eq!(ui.warnings.len(), 1);
        assert!(ui.warnings[0].contains("galaxy"), "{:?}", ui.warnings);
    }

    #[test]
    fn a_declaration_without_an_id_or_action_is_skipped_not_fatal() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            r#"
[[command]]
title = "no id"

[[command]]
id = "no action"
title = "bad id"

[[command]]
id = "ok"
"#,
        );
        assert!(ui.commands.is_empty());
        assert_eq!(ui.warnings.len(), 3, "{:?}", ui.warnings);
        assert!(ui.warnings[0].contains("has no id"));
        assert!(ui.warnings[1].contains("not ascii"));
        assert!(ui.warnings[2].contains("names no action"));
    }

    #[test]
    fn a_status_item_without_a_format_is_skipped() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            "[[status_item]]\nid = \"wip\"\nsource = { command = [\"true\"] }\n",
        );
        assert!(ui.status_items.is_empty());
        assert!(ui.warnings[0].contains("no format"), "{:?}", ui.warnings);
    }

    /// An event source cannot be subscribed to yet, so a declaration that asks
    /// for one has no data rather than a source that never fires.
    #[test]
    fn an_event_source_is_reported_and_leaves_the_declaration_without_data() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            "[[panel]]\nid = \"x\"\n[panel.source]\nevent = \"worktree.created\"\n",
        );
        assert_eq!(ui.panels.len(), 1);
        assert!(ui.panels[0].source.is_none());
        assert!(
            ui.warnings[0].contains("worktree.created"),
            "{:?}",
            ui.warnings
        );
    }

    #[test]
    fn a_sidebar_item_naming_an_undeclared_panel_warns_and_keeps_its_row() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            "[[sidebar_item]]\nid = \"board\"\ntitle = \"Board\"\npanel = \"nope\"\n",
        );
        assert_eq!(ui.sidebar_items.len(), 1);
        assert_eq!(ui.sidebar_items[0].panel_id.as_deref(), Some("nope"));
        assert!(
            ui.warnings[0].contains("does not declare"),
            "{:?}",
            ui.warnings
        );
    }

    #[test]
    fn plugin_settings_sections_are_counted_as_unmounted() {
        let ui = UiContributions::parse(
            "a.b",
            root(),
            "[[settings_section]]\nid = \"general\"\ntitle = \"Layout\"\n",
        );
        assert!(ui.is_empty());
        assert!(ui.warnings[0].contains("settings"), "{:?}", ui.warnings);
    }

    #[test]
    fn a_file_that_is_not_toml_warns_instead_of_failing() {
        let ui = UiContributions::parse("a.b", root(), "this is not = = toml");
        assert!(ui.is_empty());
        assert!(ui.warnings[0].contains("was not read"), "{:?}", ui.warnings);
    }

    #[test]
    fn versions_compare_by_their_three_numbers() {
        assert_eq!(parse_version("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("v1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("2.0.0-beta.1"), Some((2, 0, 0)));
        assert_eq!(parse_version("0.1.0+sha"), Some((0, 1, 0)));
        assert_eq!(parse_version("soon"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!(parse_version("0.10.0") > parse_version("0.9.9"));
    }

    /// A plugin root on disk, because mounting reads the file the way the host
    /// does. Dropped with the test.
    struct PluginRoot(std::path::PathBuf);

    impl PluginRoot {
        fn new(label: &str, ui: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("zenix-ui-manifest-{}-{label}", std::process::id()));
            std::fs::create_dir_all(&path).expect("the fixture directory is created");
            std::fs::write(path.join(FILE_NAME), ui).expect("the fixture file is written");
            Self(path)
        }
    }

    impl Drop for PluginRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn plugin(plugin_id: &str, root: &PluginRoot, enabled: bool) -> InstalledPlugin {
        InstalledPlugin {
            plugin_id: plugin_id.into(),
            name: plugin_id.into(),
            version: "0.1.0".into(),
            plugin_root: root.0.to_string_lossy().into_owned(),
            description: None,
            enabled,
            panes: Vec::new(),
            events: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn mounting_merges_plugins_and_orders_sidebar_items_as_declared() {
        let first = PluginRoot::new(
            "merge-first",
            "[[panel]]\nid = \"board\"\ntitle = \"Board\"\n\n[[sidebar_item]]\nid = \"late\"\ntitle = \"Late\"\norder = 90\npanel = \"board\"\n",
        );
        let second = PluginRoot::new(
            "merge-second",
            "[[status_item]]\nid = \"wip\"\nformat = \"{count} WIP\"\n\n[[sidebar_item]]\nid = \"early\"\ntitle = \"Early\"\norder = 10\n",
        );
        let mounted = mounted_ui(&[
            plugin("a.one", &first, true),
            plugin("b.two", &second, true),
        ]);
        assert!(mounted.warnings.is_empty(), "{:?}", mounted.warnings);
        assert_eq!(mounted.panels.len(), 1);
        assert_eq!(mounted.status_items.len(), 1);
        assert_eq!(mounted.panels[0].qualified_id(), "a.one:panel:board");
        assert_eq!(mounted.status_items[0].qualified_id(), "b.two:status:wip");
        // Order across plugins, not declaration order.
        let order: Vec<&str> = mounted
            .sidebar_items
            .iter()
            .map(|item| item.local_id.as_str())
            .collect();
        assert_eq!(order, vec!["early", "late"]);
    }

    #[test]
    fn mounting_drops_a_duplicate_and_skips_disabled_plugins() {
        let ui = "[[panel]]\nid = \"board\"\ntitle = \"First\"\n\n[[panel]]\nid = \"board\"\ntitle = \"Second\"\n";
        let root = PluginRoot::new("duplicate", ui);
        let mounted = mounted_ui(&[plugin("a.one", &root, true)]);
        assert_eq!(mounted.panels.len(), 1);
        assert_eq!(mounted.panels[0].title, "First");
        assert_eq!(mounted.warnings.len(), 1);
        assert!(
            mounted.warnings[0].contains("duplicate"),
            "{:?}",
            mounted.warnings
        );

        let mounted = mounted_ui(&[plugin("a.one", &root, false)]);
        assert!(mounted.is_empty());
        assert!(mounted.warnings.is_empty());
    }
}
