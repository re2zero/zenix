//! The pre-link trust review: what a plugin's manifest would actually run, and
//! where that steps outside the plugin's own directory.
//!
//! Linking a plugin hands herdr a directory whose build, startup, action, event,
//! pane and link-handler commands run as the user, with the user's environment
//! and the full herdr CLI. Herdr reviews none of that code, so the client shows
//! the commands themselves before a plugin is linked: this module turns
//! `herdr-plugin.toml` into one flat, ordered list of argv lines plus the
//! hazards worth stopping on, and the review surface draws it. Nothing here
//! executes a plugin or writes anything — it reads one file and reports.
//!
//! Verified against the daemon 0.9.5 (`vendor/herdr-pins.tsv`):
//!
//! * `src/app/api/plugins/manifest.rs:12` (`RawPluginManifest`) is the parser.
//!   The TOML section names are its own field names — `[[build]]`,
//!   `[[startup]]`, `[[actions]]`, `[[events]]`, `[[panes]]`,
//!   `[[link_handlers]]` — and a field that is neither `Option` nor
//!   `#[serde(default)]` is required. There is no `deny_unknown_fields`, so an
//!   unknown key is ignored; this report ignores it too rather than calling a
//!   plugin broken that the daemon would run.
//! * `src/api/schema/plugins.rs:230` (`PluginManifestBuild/Action/EventHook/`…)
//!   are those shapes after normalisation, which is what `plugin.list` reports.
//! * `src/plugin_command.rs:5` (`command_for_argv_in_dir`) and
//!   `src/app/api/plugins/runtime.rs:103` are where a command is spawned: the
//!   plugin root is the child's working directory, and
//!   `src/plugin_command.rs:12` (`program_for_cwd`) sends a bare name to `PATH`
//!   while joining a relative name that contains a separator to that root. The
//!   author-facing promise is in `docs/next/website/src/content/docs/plugins.mdx`
//!   ("Runtime commands run with the plugin directory as their working
//!   directory"). So `sh mark.sh` is the plugin's own script and no hazard,
//!   while `sh -c …` hides code the manifest never shows.
//!
//! Two places where this report is deliberately softer than the daemon, both so
//! one review names every problem at once instead of stopping at the first:
//!
//! * A section entry with no `command` is a warning, not an error. The daemon
//!   refuses such a manifest (`manifest.rs:559`, `normalize_command`, plus the
//!   missing field itself), so a manifest that previews without complaint is not
//!   a promise that herdr will link it.
//! * A `[[link_handlers]]` entry naming an action the manifest does not declare
//!   is a warning too, where the daemon refuses the whole manifest
//!   (`manifest.rs:363`, `validate_link_handler_actions`). Nothing else here is
//!   softer than the daemon: a missing file, a malformed file and a missing `id`
//!   are all errors.
//!
//! Requiredness of the fields this report does not read — an action's `title`
//! and `description`, a pane's `placement` and size, item-level `platforms` —
//! stays the daemon's business at link time.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::model::InstalledPlugin;
use super::ui_manifest::UiContributions;

/// The manifest file, inside the plugin directory.
pub(crate) const FILE_NAME: &str = "herdr-plugin.toml";

/// One command the manifest declares, ready to be shown to a human.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommandLine {
    pub(crate) kind: CommandKind,
    /// The action id, pane id, event name or link-handler id it belongs to;
    /// empty for a build or startup command, which has no id of its own.
    pub(crate) id: String,
    /// The label a human reads: `"build"`, `"action mark"`, `"pane board"`.
    pub(crate) title: String,
    pub(crate) argv: Vec<String>,
}

/// Which part of the manifest a command came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CommandKind {
    Build,
    Startup,
    Action,
    Event,
    Pane,
    LinkHandler,
}

impl CommandKind {
    /// The word this kind is called in the manifest and in the report.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Startup => "startup",
            Self::Action => "action",
            Self::Event => "event",
            Self::Pane => "pane",
            Self::LinkHandler => "link handler",
        }
    }
}

/// One command worth looking at twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hazard {
    /// Index into `Preview::commands`.
    pub(crate) index: usize,
    /// The rule that matched, as a sentence. A command gets at most one hazard:
    /// the first rule in [`HAZARD_RULES`] order that matches it.
    pub(crate) reason: &'static str,
    /// How much the rule asks for, which is what the surfaces color it by.
    pub(crate) level: HazardLevel,
}

/// How much a hazard asks for — the prototype's `.hazard.hi` / `.hazard.mid`
/// pair, with everything unflagged drawing as the third grade. A *high* is a
/// command whose argv hides what will run or that steps around the user's
/// hands: inline code the manifest never shows, another user's privileges. A
/// *medium* leaves the plugin's directory or the machine's boundary — an
/// absolute program, the network, a package install, a system path — while
/// still naming what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HazardLevel {
    High,
    Medium,
}

impl HazardLevel {
    /// The i18n key tail the surfaces spell the grade with: `hazard_high` /
    /// `hazard_mid`, beside the unflagged `stays`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::High => "hazard_high",
            Self::Medium => "hazard_mid",
        }
    }
}

/// Everything the manifest says about what it will run, in the order the parts
/// run or the user meets them: build, startup, actions, events, panes, link
/// handlers, each section in the manifest's own order. Manifest order rather
/// than the daemon's listing order (it sorts actions, panes and events by id for
/// `plugin.list`) because a review reads next to the file it is reviewing.
#[derive(Clone, Debug)]
pub(crate) struct Preview {
    pub(crate) plugin_id: String,
    /// Falls back to the plugin id when the manifest declares none, together with
    /// a warning.
    pub(crate) name: String,
    /// Empty when the manifest declares none, together with a warning.
    pub(crate) version: String,
    pub(crate) commands: Vec<CommandLine>,
    /// Sorted by index.
    pub(crate) hazards: Vec<Hazard>,
    /// Manifest problems that are not fatal here, in the daemon's own wording
    /// where the daemon has words for them.
    pub(crate) warnings: Vec<String>,
    /// The UI this directory would declare for this client, read from its
    /// `zenix-ui.toml` beside the daemon's manifest. The daemon neither reads
    /// nor validates that file, so what it fills is only ever shown here and in
    /// the manager.
    pub(crate) ui: UiContributions,
}

/// Read `<dir>/herdr-plugin.toml` and summarise it. The only function here that
/// touches the filesystem, and it reads no other file: what a plugin's own
/// scripts do is what the report shows, not what it inspects.
///
/// A missing manifest, a file that is not TOML, and a manifest without a plugin
/// id are errors carrying a sentence a user can read, each naming the path that
/// was looked at.
pub(crate) fn preview(dir: &Path) -> Result<Preview, String> {
    let path = dir.join(FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("no plugin manifest at {}", path.display()));
        }
        Err(error) => {
            return Err(format!(
                "plugin manifest at {} could not be read: {error}",
                path.display()
            ));
        }
    };
    let raw: RawManifest = toml::from_str(&text).map_err(|error| {
        format!(
            "plugin manifest at {} could not be parsed: {error}",
            path.display()
        )
    })?;
    summarise(raw, &path)
}

/// The commands an *installed* plugin's contract carries, in the same
/// vocabulary as [`preview`] — the manager's page and the drawer draw the
/// daemon's own report, not a re-read of the file. `plugin.list` reports every
/// manifest section as linked, so this is what the daemon will run, without a
/// filesystem read and without asking a disabled plugin's directory to still
/// exist.
///
/// A link handler names an action, and the daemon refuses a manifest whose
/// handler names an unknown one — but this reads what the daemon reports, so a
/// handler that names nothing runnable is skipped rather than invented.
pub(crate) fn contract_commands(plugin: &InstalledPlugin) -> Vec<CommandLine> {
    let mut commands: Vec<CommandLine> = Vec::new();
    let mut push = |kind: CommandKind, id: &str, argv: &[String]| {
        if !argv.is_empty() {
            commands.push(CommandLine {
                kind,
                id: id.to_string(),
                title: title_for(kind, id),
                argv: argv.to_vec(),
            });
        }
    };
    for entry in &plugin.build {
        push(CommandKind::Build, "", &entry.command);
    }
    for entry in &plugin.startup {
        push(CommandKind::Startup, "", &entry.command);
    }
    let mut action_argv: HashMap<&str, &[String]> = HashMap::new();
    for action in &plugin.actions {
        action_argv.insert(action.id.as_str(), &action.command);
        push(CommandKind::Action, &action.id, &action.command);
    }
    for hook in &plugin.events {
        push(CommandKind::Event, &hook.on, &hook.command);
    }
    for pane in &plugin.panes {
        push(CommandKind::Pane, &pane.id, &pane.command);
    }
    for handler in &plugin.link_handlers {
        if let Some(argv) = action_argv.get(handler.action.as_str()) {
            push(CommandKind::LinkHandler, &handler.id, argv);
        }
    }
    commands
}

/// One command as one display line: `action slow: sh -c "sleep 1; echo done"`.
/// Display only — tokens are quoted for a human to read, not for a shell to
/// re-parse, so this string is never the thing that runs.
pub(crate) fn render_command(command: &CommandLine) -> String {
    let argv: Vec<String> = command.argv.iter().map(|token| quoted(token)).collect();
    format!("{}: {}", command.title, argv.join(" "))
}

/// One hazard rule: the sentence it reports, the grade it asks for, and the
/// test that decides it.
pub(crate) type HazardRule = (&'static str, HazardLevel, fn(&[String]) -> bool);

/// The hazard rules, in the order a command is tested against them — a command
/// gets the first one that matches and nothing else. Every rule is a question
/// about argv alone, so the list is what the tests drive and what the review
/// surface can show a user.
pub(crate) const HAZARD_RULES: &[HazardRule] = &[
    (
        "runs a program at an absolute path outside the plugin",
        HazardLevel::Medium,
        runs_absolute_program,
    ),
    (
        "asks for elevated privileges",
        HazardLevel::High,
        asks_for_elevation,
    ),
    (
        "runs inline shell code that is not in the manifest",
        HazardLevel::High,
        runs_inline_shell_code,
    ),
    (
        "runs inline interpreter code that is not in the manifest",
        HazardLevel::High,
        runs_inline_interpreter_code,
    ),
    (
        "reaches the network or installs packages",
        HazardLevel::Medium,
        reaches_network_or_installs,
    ),
    (
        "touches a system path outside the plugin",
        HazardLevel::Medium,
        touches_system_path,
    ),
];

/// The first rule that matches, which is the one hazard a command carries —
/// the sentence and the grade, without a place in any list.
pub(crate) fn hazard_for(argv: &[String]) -> Option<(&'static str, HazardLevel)> {
    HAZARD_RULES
        .iter()
        .find(|(_, _, matches)| matches(argv))
        .map(|(reason, level, _)| (*reason, *level))
}

/// One hazard at most per command, in command order.
fn hazards_for(commands: &[CommandLine]) -> Vec<Hazard> {
    let mut hazards: Vec<Hazard> = commands
        .iter()
        .enumerate()
        .filter_map(|(index, command)| {
            hazard_for(&command.argv).map(|(reason, level)| Hazard {
                index,
                reason,
                level,
            })
        })
        .collect();
    hazards.sort_by_key(|hazard| hazard.index);
    hazards
}

/// Elevated-privilege programs, matched as a token anywhere in argv: `sh x.sh`
/// escalated by a `sudo` argument is the same request as a leading `sudo`.
const ELEVATION_TOKENS: &[&str] = &["sudo", "doas", "pkexec", "su"];

/// Shells whose `-c` argument is the script, rather than a file in the plugin.
const SHELL_PROGRAMS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh"];

/// Interpreters whose `-e`/`-c`/`-p` argument is the program text.
const INTERPRETER_PROGRAMS: &[&str] = &["node", "deno", "bun", "python", "python3", "ruby", "perl"];

/// Programs that fetch or push something over the network.
const NETWORK_PROGRAMS: &[&str] = &["curl", "wget", "nc", "ncat", "socat", "ssh", "scp", "rsync"];

/// `git` subcommands that reach a remote.
const GIT_REMOTE_SUBCOMMANDS: &[&str] = &["clone", "fetch", "pull", "push", "remote"];

/// Package managers, and the subcommands that bring code onto the machine.
const PACKAGE_MANAGERS: &[&str] = &["cargo", "npm", "pnpm", "yarn", "pip", "pip3", "go", "gem"];
const INSTALL_SUBCOMMANDS: &[&str] = &["install", "add", "get"];

/// System locations a plugin has no business in, whether reached by absolute
/// path or by name.
const SYSTEM_PATHS: &[&str] = &["/usr", "/etc", "/var"];
const SYSTEM_PATH_PREFIXES: &[&str] = &["/etc/", "/usr/", "/var/", "/boot/", "/opt/"];

/// The user's SSH directory, as a plugin would write it.
const HOME_SSH: &str = "~/.ssh";

fn runs_absolute_program(argv: &[String]) -> bool {
    argv.first().is_some_and(|program| program.starts_with('/'))
}

fn asks_for_elevation(argv: &[String]) -> bool {
    argv.iter()
        .any(|token| ELEVATION_TOKENS.contains(&token.as_str()))
}

fn runs_inline_shell_code(argv: &[String]) -> bool {
    program_is_one_of(argv, SHELL_PROGRAMS) && argument_is_one_of(argv, &["-c"])
}

fn runs_inline_interpreter_code(argv: &[String]) -> bool {
    program_is_one_of(argv, INTERPRETER_PROGRAMS) && argument_is_one_of(argv, &["-e", "-c", "-p"])
}

fn reaches_network_or_installs(argv: &[String]) -> bool {
    let Some(program) = program_name(argv) else {
        return false;
    };
    NETWORK_PROGRAMS.contains(&program)
        || (program == "git" && argument_is_one_of(argv, GIT_REMOTE_SUBCOMMANDS))
        || (PACKAGE_MANAGERS.contains(&program) && argument_is_one_of(argv, INSTALL_SUBCOMMANDS))
}

fn touches_system_path(argv: &[String]) -> bool {
    argv.iter().any(|token| {
        SYSTEM_PATHS.contains(&token.as_str())
            || SYSTEM_PATH_PREFIXES
                .iter()
                .any(|prefix| token.starts_with(prefix))
            || token == HOME_SSH
            || token.starts_with("~/.ssh/")
    })
}

/// The program name as the daemon resolves it: everything after the last `/`.
/// `program_for_cwd` (herdr v0.9.3 `src/plugin_command.rs:12`) sends a name
/// without a separator to `PATH` and joins a relative one containing a
/// separator to the plugin root, which
/// `src/app/api/plugins/runtime.rs:103` makes the command's working directory.
fn program_name(argv: &[String]) -> Option<&str> {
    let program = argv.first()?;
    Some(program.rsplit('/').next().unwrap_or(program))
}

fn program_is_one_of(argv: &[String], programs: &[&str]) -> bool {
    program_name(argv).is_some_and(|program| programs.contains(&program))
}

/// Whether any token after the program is exactly one of `tokens` — a flag, not
/// a file a later argument happened to mention.
fn argument_is_one_of(argv: &[String], tokens: &[&str]) -> bool {
    argv.iter()
        .skip(1)
        .any(|token| tokens.contains(&token.as_str()))
}

/// The label a human reads for one command: the kind, then the id it belongs to.
fn title_for(kind: CommandKind, id: &str) -> String {
    if id.is_empty() {
        kind.label().to_string()
    } else {
        format!("{} {id}", kind.label())
    }
}

/// How a warning names the entry it is about: `build`, `action 'mark'`.
fn described(kind: CommandKind, id: &str) -> String {
    if id.is_empty() {
        kind.label().to_string()
    } else {
        format!("{} '{id}'", kind.label())
    }
}

/// A token as a human would type it: bare unless it holds a space or a quote, in
/// which case it is double-quoted with its own quotes and backslashes escaped.
fn quoted(token: &str) -> String {
    if !token.is_empty() && !token.contains([' ', '\t', '"', '\'', '\\']) {
        return token.to_string();
    }
    let mut text = String::with_capacity(token.len() + 2);
    text.push('"');
    for character in token.chars() {
        if matches!(character, '"' | '\\') {
            text.push('\\');
        }
        text.push(character);
    }
    text.push('"');
    text
}

/// The value if it is declared and not blank, trimmed — the daemon trims and
/// rejects blanks the same way (`non_empty_trimmed`).
fn declared(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Every command in the manifest, plus what the manifest got wrong. The section
/// order is the report's order; see [`Preview`].
fn summarise(raw: RawManifest, path: &Path) -> Result<Preview, String> {
    let Some(plugin_id) = declared(raw.id.as_deref()) else {
        return Err(format!(
            "plugin manifest at {} declares no plugin id",
            path.display()
        ));
    };
    let plugin_id = plugin_id.to_string();

    let mut warnings: Vec<String> = Vec::new();
    let name = match declared(raw.name.as_deref()) {
        Some(name) => name.to_string(),
        None => {
            warnings.push("plugin name is required".to_string());
            plugin_id.clone()
        }
    };
    let version = match declared(raw.version.as_deref()) {
        Some(version) => version.to_string(),
        None => {
            warnings.push("plugin version is required".to_string());
            String::new()
        }
    };
    if declared(raw.min_herdr_version.as_deref()).is_none() {
        warnings.push("plugin min_herdr_version is required".to_string());
    }
    if raw.platforms.is_none() {
        warnings.push("manifest does not declare platforms; platform support unknown".to_string());
    }

    let mut commands: Vec<CommandLine> = Vec::new();
    for entry in raw.build {
        push_command(
            &mut commands,
            CommandKind::Build,
            "",
            entry.command,
            &mut warnings,
        );
    }
    for entry in raw.startup {
        push_command(
            &mut commands,
            CommandKind::Startup,
            "",
            entry.command,
            &mut warnings,
        );
    }

    // A link handler runs an action's argv, so the actions are kept by id. The
    // daemon refuses a manifest whose handler names an unknown action
    // (`validate_link_handler_actions`, herdr v0.9.3
    // `src/app/api/plugins/manifest.rs:363`); warning here instead keeps the rest
    // of the manifest reviewable.
    let mut action_argv: HashMap<String, Vec<String>> = HashMap::new();
    for entry in raw.actions {
        if let Some(action) =
            command_line(CommandKind::Action, &entry.id, entry.command, &mut warnings)
        {
            action_argv.insert(action.id.clone(), action.argv.clone());
            commands.push(action);
        }
    }
    for entry in raw.events {
        push_command(
            &mut commands,
            CommandKind::Event,
            &entry.on,
            entry.command,
            &mut warnings,
        );
    }
    for entry in raw.panes {
        push_command(
            &mut commands,
            CommandKind::Pane,
            &entry.id,
            entry.command,
            &mut warnings,
        );
    }
    for handler in raw.link_handlers {
        let Some(argv) = action_argv.get(&handler.action) else {
            warnings.push(format!(
                "link handler '{}' references unknown action '{}'",
                handler.id, handler.action
            ));
            continue;
        };
        // A resolved handler always has argv: only commands with one are kept.
        commands.push(CommandLine {
            kind: CommandKind::LinkHandler,
            title: title_for(CommandKind::LinkHandler, &handler.id),
            id: handler.id,
            argv: argv.clone(),
        });
    }

    let hazards = hazards_for(&commands);
    let ui = path
        .parent()
        .map(|dir| UiContributions::read(&plugin_id, dir))
        .unwrap_or_default();
    Ok(Preview {
        plugin_id,
        name,
        version,
        commands,
        hazards,
        warnings,
        ui,
    })
}

fn push_command(
    commands: &mut Vec<CommandLine>,
    kind: CommandKind,
    id: &str,
    argv: Vec<String>,
    warnings: &mut Vec<String>,
) {
    if let Some(command) = command_line(kind, id, argv, warnings) {
        commands.push(command);
    }
}

/// The command, or a warning when the entry declares none: an entry with no
/// `command` runs nothing, which the daemon treats as a broken manifest and this
/// report treats as something to say out loud.
fn command_line(
    kind: CommandKind,
    id: &str,
    argv: Vec<String>,
    warnings: &mut Vec<String>,
) -> Option<CommandLine> {
    if argv.is_empty() {
        warnings.push(format!("{} has no command", described(kind, id)));
        return None;
    }
    Some(CommandLine {
        kind,
        id: id.to_string(),
        title: title_for(kind, id),
        argv,
    })
}

/// The manifest as herdr v0.9.3 parses it
/// (`src/app/api/plugins/manifest.rs:12`), narrowed to the fields this report
/// reads. Everything else is ignored, exactly as the daemon ignores unknown
/// keys; see the module docs for which required fields that leaves to the daemon.
#[derive(Deserialize)]
struct RawManifest {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    min_herdr_version: Option<String>,
    #[serde(default)]
    platforms: Option<Vec<String>>,
    #[serde(default)]
    build: Vec<RawBuild>,
    #[serde(default)]
    startup: Vec<RawStartup>,
    #[serde(default)]
    actions: Vec<RawAction>,
    #[serde(default)]
    events: Vec<RawEvent>,
    #[serde(default)]
    panes: Vec<RawPane>,
    #[serde(default)]
    link_handlers: Vec<RawLinkHandler>,
}

#[derive(Deserialize)]
struct RawBuild {
    #[serde(default)]
    command: Vec<String>,
}

#[derive(Deserialize)]
struct RawStartup {
    #[serde(default)]
    command: Vec<String>,
}

#[derive(Deserialize)]
struct RawAction {
    id: String,
    #[serde(default)]
    command: Vec<String>,
}

#[derive(Deserialize)]
struct RawEvent {
    on: String,
    #[serde(default)]
    command: Vec<String>,
}

#[derive(Deserialize)]
struct RawPane {
    id: String,
    #[serde(default)]
    command: Vec<String>,
}

#[derive(Deserialize)]
struct RawLinkHandler {
    id: String,
    action: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header of a manifest that is complete except for the section under
    /// test.
    const HEADER: &str = "id = \"test.plugin\"\nname = \"Test Plugin\"\nversion = \"0.1.0\"\nmin_herdr_version = \"0.9.0\"\nplatforms = [\"linux\"]\n";

    /// The plugin this repo links into an isolated session. A real manifest, read
    /// from disk, so the shape this parser expects and the shape an author writes
    /// cannot drift apart. The fixture lives under the local, gitignored
    /// `.relay/` tree, so a checkout that does not have it skips the one test
    /// that reads it rather than failing: every other test here builds its own
    /// manifest and still covers the parser.
    fn fixture_dir() -> Option<std::path::PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".relay/notes/plugin-smoke");
        dir.join(FILE_NAME).is_file().then_some(dir)
    }

    /// A plugin directory on disk, because `preview` reads a file. Dropped with
    /// the test.
    struct PluginDir(std::path::PathBuf);

    impl PluginDir {
        fn new(label: &str, manifest: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("zenix-trust-{}-{label}", std::process::id()));
            std::fs::create_dir_all(&path).expect("the fixture directory is created");
            std::fs::write(path.join(FILE_NAME), manifest)
                .expect("the fixture manifest is written");
            Self(path)
        }

        /// The manifest path a message about this directory has to name.
        fn manifest_path(&self) -> String {
            self.0.join(FILE_NAME).to_string_lossy().into_owned()
        }
    }

    impl Drop for PluginDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn argv(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|token| token.to_string()).collect()
    }

    /// The sentence of the rule that mentions `needle`, read out of the table so a
    /// test cannot pin a rule the table no longer holds.
    fn reason_for(needle: &str) -> &'static str {
        HAZARD_RULES
            .iter()
            .find(|(reason, _, _)| reason.contains(needle))
            .map(|(reason, _, _)| *reason)
            .unwrap_or_else(|| panic!("a hazard rule mentions {needle:?}"))
    }

    /// The sentence `hazard_for` answers, without the grade — what most of these
    /// tests pin is which rule fired, not how hard.
    fn matched(argv: &[String]) -> Option<&'static str> {
        hazard_for(argv).map(|(reason, _)| reason)
    }

    /// The review reads the UI the plugin would declare as well as the commands
    /// it would run: the daemon never reads `zenix-ui.toml`, so this screen is
    /// the only place its slots can be seen before a link.
    #[test]
    fn a_preview_carries_the_ui_the_plugin_would_declare() {
        let dir = PluginDir::new("ui", HEADER);
        // No `zenix-ui.toml` yet: the plugin contributes functions only.
        let ui = &preview(&dir.0).expect("a readable manifest").ui;
        // No `zenix-ui.toml` means nothing mounted: no commands, slots, or items.
        assert!(
            ui.commands.is_empty()
                && ui.sidebar_items.is_empty()
                && ui.panels.is_empty()
                && ui.status_items.is_empty()
        );
        std::fs::write(
            dir.0.join("zenix-ui.toml"),
            r#"
[[sidebar_item]]
id = "board"
title = "Project board"
panel = "board"
"#,
        )
        .expect("a UI declaration is written");
        let preview = preview(&dir.0).expect("a readable manifest");
        assert_eq!(preview.ui.sidebar_items.len(), 1);
        assert_eq!(preview.ui.sidebar_items[0].plugin_id, "test.plugin");
    }

    #[test]
    fn each_kind_has_its_own_label() {
        let labels = [
            CommandKind::Build,
            CommandKind::Startup,
            CommandKind::Action,
            CommandKind::Event,
            CommandKind::Pane,
            CommandKind::LinkHandler,
        ]
        .map(CommandKind::label);
        assert_eq!(
            labels,
            [
                "build",
                "startup",
                "action",
                "event",
                "pane",
                "link handler"
            ]
        );
    }

    /// The fixture declares three actions and two panes; both panes and two of the
    /// actions hand a script to a shell, which is the one thing in it worth
    /// flagging.
    #[test]
    fn the_fixture_manifest_becomes_the_commands_it_declares() {
        let Some(dir) = fixture_dir() else {
            eprintln!("skipped: this checkout has no .relay/notes/plugin-smoke fixture");
            return;
        };
        let preview = preview(&dir).expect("the fixture manifest previews");

        assert_eq!(preview.plugin_id, "zenix.smoke");
        assert_eq!(preview.name, "Zenix Smoke");
        assert_eq!(preview.version, "0.1.0");
        assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);

        let expected = vec![
            CommandLine {
                kind: CommandKind::Action,
                id: "mark".to_string(),
                title: "action mark".to_string(),
                argv: argv(&["sh", "mark.sh"]),
            },
            CommandLine {
                kind: CommandKind::Action,
                id: "slow".to_string(),
                title: "action slow".to_string(),
                argv: argv(&["sh", "-c", "sleep 1; echo slow done"]),
            },
            CommandLine {
                kind: CommandKind::Action,
                id: "fail".to_string(),
                title: "action fail".to_string(),
                argv: argv(&["sh", "-c", "echo deliberate failure 1>&2; exit 3"]),
            },
            CommandLine {
                kind: CommandKind::Pane,
                id: "board".to_string(),
                title: "pane board".to_string(),
                argv: argv(&["sh", "-c", "echo zenix smoke board; sleep 600"]),
            },
            CommandLine {
                kind: CommandKind::Pane,
                id: "note".to_string(),
                title: "pane note".to_string(),
                argv: argv(&["sh", "-c", "echo zenix smoke popup; sleep 600"]),
            },
        ];
        assert_eq!(preview.commands, expected);

        // The clean action `sh mark.sh` is index 0 and stays unflagged: the script
        // is the plugin's own file, run from the plugin root.
        assert_eq!(preview.hazards.len(), 4, "{:?}", preview.hazards);
        let indices: Vec<usize> = preview.hazards.iter().map(|hazard| hazard.index).collect();
        assert_eq!(indices, vec![1, 2, 3, 4]);
        for hazard in &preview.hazards {
            assert_eq!(hazard.reason, reason_for("inline shell"), "{hazard:?}");
        }
    }

    #[test]
    fn build_startup_and_event_commands_are_reported_with_their_kind() {
        let manifest = format!(
            "{HEADER}[[build]]\ncommand = [\"npm\", \"ci\"]\n\n[[startup]]\ncommand = [\"node\", \"dist/restore.js\"]\n\n[[events]]\non = \"worktree.created\"\ncommand = [\"herdr\", \"workspace\", \"list\"]\n"
        );
        let dir = PluginDir::new("sections", &manifest);
        let preview = preview(&dir.0).expect("the manifest previews");

        let expected = vec![
            CommandLine {
                kind: CommandKind::Build,
                id: String::new(),
                title: "build".to_string(),
                argv: argv(&["npm", "ci"]),
            },
            CommandLine {
                kind: CommandKind::Startup,
                id: String::new(),
                title: "startup".to_string(),
                argv: argv(&["node", "dist/restore.js"]),
            },
            CommandLine {
                kind: CommandKind::Event,
                id: "worktree.created".to_string(),
                title: "event worktree.created".to_string(),
                argv: argv(&["herdr", "workspace", "list"]),
            },
        ];
        assert_eq!(preview.commands, expected);
        assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
        assert!(preview.hazards.is_empty(), "{:?}", preview.hazards);
    }

    /// One sample per rule, in `HAZARD_RULES` order, so a rule added or reordered
    /// without a sample fails here rather than going untested.
    #[test]
    fn every_hazard_rule_fires_on_a_sample_of_its_own() {
        let samples: Vec<Vec<&str>> = vec![
            // Absolute first: this one is under /usr and trips the system-path rule too.
            vec!["/usr/local/bin/deploy", "--now"],
            vec!["sudo", "systemctl", "restart", "nginx"],
            vec!["bash", "-c", "echo hi"],
            vec!["python3", "-c", "print(1)"],
            vec!["curl", "https://example.com/install.sh"],
            vec!["cat", "/etc/hosts"],
        ];
        assert_eq!(
            samples.len(),
            HAZARD_RULES.len(),
            "one sample per hazard rule, in order"
        );

        for (sample, (reason, _, matches)) in samples.iter().zip(HAZARD_RULES) {
            let tokens = argv(sample);
            assert_eq!(matched(&tokens), Some(*reason), "{sample:?}");
            assert!(matches(&tokens), "{sample:?} should match {reason}");
        }
    }

    /// The grades the table assigns, in one place: code the manifest does not
    /// show or another user's privileges is high; leaving the plugin's
    /// directory or the machine's boundary is medium. The surfaces color the
    /// two grades differently and leave the unflagged ones muted.
    #[test]
    fn the_grades_split_hiding_from_leaving() {
        let grade = |sample: &[&str]| hazard_for(&argv(sample)).map(|(_, level)| level);
        assert_eq!(grade(&["sudo", "true"]), Some(HazardLevel::High));
        assert_eq!(grade(&["bash", "-c", "echo hi"]), Some(HazardLevel::High));
        assert_eq!(
            grade(&["python3", "-c", "print(1)"]),
            Some(HazardLevel::High)
        );
        assert_eq!(
            grade(&["/usr/local/bin/deploy", "--now"]),
            Some(HazardLevel::Medium)
        );
        assert_eq!(
            grade(&["curl", "https://example.com"]),
            Some(HazardLevel::Medium)
        );
        assert_eq!(grade(&["cat", "/etc/hosts"]), Some(HazardLevel::Medium));
        assert_eq!(grade(&["sh", "mark.sh"]), None);
    }

    /// The daemon's own report becomes the same command list the pre-link
    /// preview would have made of the manifest: sections in manifest order, a
    /// link handler resolved to the argv of the action it names, and an entry
    /// with no command simply absent.
    #[test]
    fn an_installed_plugins_report_becomes_its_contract_commands() {
        let plugin: InstalledPlugin = serde_json::from_value(serde_json::json!({
            "plugin_id": "zenix.smoke",
            "name": "Zenix Smoke",
            "version": "0.1.0",
            "manifest_path": "/tmp/plugin/herdr-plugin.toml",
            "plugin_root": "/tmp/plugin",
            "enabled": true,
            "build": [{"command": ["npm", "ci"]}],
            "startup": [{"command": ["node", "restore.js"]}],
            "actions": [
                {"id": "mark", "command": ["sh", "mark.sh"]},
                {"id": "slow", "command": ["sh", "-c", "sleep 1; echo slow done"]}
            ],
            "events": [{"on": "worktree.created", "command": ["herdr", "workspace", "list"]}],
            "panes": [{"id": "board", "title": "Board", "placement": "tab",
                        "command": ["./bin/board"]}],
            "link_handlers": [
                {"id": "issue", "action": "slow"},
                {"id": "gone", "action": "missing"}
            ],
        }))
        .expect("decodes an installed plugin");
        let commands = contract_commands(&plugin);
        let titles: Vec<&str> = commands
            .iter()
            .map(|command| command.title.as_str())
            .collect();
        assert_eq!(
            titles,
            vec![
                "build",
                "startup",
                "action mark",
                "action slow",
                "event worktree.created",
                "pane board",
                "link handler issue",
            ]
        );
        // The handler runs the action's argv — the same list the trust review
        // shows before the link. The handler naming a missing action is absent:
        // the daemon refuses such a manifest at link, and this reads what the
        // daemon reports rather than inventing a run.
        assert_eq!(
            commands[6].argv,
            argv(&["sh", "-c", "sleep 1; echo slow done"])
        );
    }

    #[test]
    fn only_the_first_matching_rule_is_reported() {
        assert_eq!(
            matched(&argv(&["/usr/bin/env", "sh"])),
            Some(reason_for("absolute path"))
        );
        assert_eq!(
            matched(&argv(&["sh", "-c", "sudo true"])),
            Some(reason_for("inline shell"))
        );
        assert_eq!(
            matched(&argv(&["sudo", "sh", "-c", "true"])),
            Some(reason_for("elevated"))
        );
    }

    #[test]
    fn privileged_programs_are_hazards_wherever_they_appear() {
        let elevation = reason_for("elevated");
        for sample in [
            vec!["sudo", "true"],
            vec!["doas", "true"],
            vec!["pkexec", "true"],
            vec!["su", "-c", "whoami"],
            vec!["sh", "deploy.sh", "sudo"],
        ] {
            assert_eq!(matched(&argv(&sample)), Some(elevation), "{sample:?}");
        }
    }

    #[test]
    fn inline_code_rules_need_their_own_flag() {
        // `-c` on a shell and `-e`/`-c`/`-p` on an interpreter are the flags that
        // put the program text in the manifest's argument list.
        for sample in [
            vec!["sh", "-c", "true"],
            vec!["bash", "-c", "true"],
            vec!["zsh", "-c", "true"],
            vec!["dash", "-c", "true"],
        ] {
            assert_eq!(
                matched(&argv(&sample)),
                Some(reason_for("inline shell")),
                "{sample:?}"
            );
        }
        for sample in [
            vec!["node", "-e", "console.log(1)"],
            vec!["python3", "-c", "print(1)"],
            vec!["ruby", "-e", "puts 1"],
            vec!["perl", "-p", "s/a/b/"],
        ] {
            assert_eq!(
                matched(&argv(&sample)),
                Some(reason_for("inline interpreter")),
                "{sample:?}"
            );
        }
        // A shell program with any other argument is not inline code.
        assert_eq!(matched(&argv(&["sh", "-x", "mark.sh"])), None);
    }

    #[test]
    fn network_and_install_programs_are_hazards() {
        let network = reason_for("network");
        for sample in [
            vec!["curl", "https://example.com/install.sh"],
            vec!["wget", "-q", "https://example.com/x.tar.gz"],
            vec!["nc", "-l", "9000"],
            vec!["socat", "TCP:example.com:80", "-"],
            vec!["ssh", "host", "uptime"],
            vec!["scp", "a", "host:/tmp"],
            vec!["rsync", "-a", "./dist/", "host:/srv/"],
            vec!["git", "clone", "https://example.com/plugin"],
            vec!["git", "push"],
            vec!["cargo", "install", "ripgrep"],
            vec!["npm", "add", "left-pad"],
            vec!["pnpm", "install"],
            vec!["yarn", "add", "x"],
            vec!["pip3", "get", "x"],
            vec!["go", "get", "example.com/x"],
            vec!["gem", "install", "x"],
        ] {
            assert_eq!(matched(&argv(&sample)), Some(network), "{sample:?}");
        }
    }

    #[test]
    fn system_paths_and_the_ssh_directory_are_hazards() {
        let system = reason_for("system path");
        for sample in [
            vec!["rm", "-rf", "/var/tmp/x"],
            vec!["cat", "/etc/shadow"],
            vec!["cp", "key", "/opt/keys"],
            vec!["install", "-m", "600", "k", "/boot/x"],
            vec!["ls", "/usr"],
            vec!["ls", "/var"],
            vec!["cat", "~/.ssh/id_ed25519"],
            vec!["cp", "~/.ssh/config", "backup"],
        ] {
            assert_eq!(matched(&argv(&sample)), Some(system), "{sample:?}");
        }
    }

    /// The daemon runs a plugin's commands in the plugin root and resolves a bare
    /// name against `PATH`, so a plugin's own scripts and ordinary CLI use are not
    /// hazards — only leaving that directory is.
    #[test]
    fn relative_programs_inside_the_plugin_are_not_hazards() {
        for sample in [
            vec!["sh", "mark.sh"],
            vec!["node", "dist/apply.js"],
            vec!["./bin/tool", "--json"],
            vec!["bash", "scripts/build.sh", "--release"],
            vec!["herdr", "workspace", "list"],
            vec!["git", "status"],
            vec!["npm", "run", "build"],
            vec!["cargo", "build"],
        ] {
            assert_eq!(matched(&argv(&sample)), None, "{sample:?}");
        }
    }

    #[test]
    fn a_command_with_nothing_to_run_is_not_a_hazard() {
        assert_eq!(matched(&[]), None);
        assert_eq!(matched(&argv(&[""])), None);
    }

    #[test]
    fn a_missing_directory_is_an_err_that_names_the_path_it_looked_at() {
        let dir = std::env::temp_dir().join(format!("zenix-trust-missing-{}", std::process::id()));
        let error = preview(&dir).expect_err("a directory with no manifest is an error");
        let manifest_path = dir.join(FILE_NAME);
        assert!(error.contains("no plugin manifest at"), "{error}");
        assert!(
            error.contains(&manifest_path.to_string_lossy().into_owned()),
            "{error}"
        );
    }

    #[test]
    fn a_malformed_manifest_is_an_err_that_names_the_path_it_looked_at() {
        let dir = PluginDir::new("malformed", "id = \"test.plugin\"\n[[actions]\n");
        let error = preview(&dir.0).expect_err("a file that is not TOML is an error");
        assert!(error.contains("could not be parsed"), "{error}");
        assert!(error.contains(&dir.manifest_path()), "{error}");
    }

    #[test]
    fn a_manifest_without_a_plugin_id_is_an_err() {
        for manifest in ["name = \"Nameless\"\n", "id = \"  \"\nname = \"Blank\"\n"] {
            let dir = PluginDir::new("no-id", manifest);
            let error = preview(&dir.0).expect_err("a plugin id is required");
            assert!(error.contains("declares no plugin id"), "{error}");
            assert!(error.contains(&dir.manifest_path()), "{error}");
        }
    }

    /// The daemon refuses a manifest like this, but a review is more useful when
    /// it says which entry is empty than when it stops at the first one.
    #[test]
    fn an_action_without_a_command_is_a_warning_not_a_failure() {
        let manifest = format!("{HEADER}[[actions]]\nid = \"mark\"\ntitle = \"Write a marker\"\n");
        let dir = PluginDir::new("no-command", &manifest);
        let preview = preview(&dir.0).expect("an entry with no command still previews");

        assert!(preview.commands.is_empty());
        assert!(preview.hazards.is_empty());
        assert_eq!(preview.warnings, vec!["action 'mark' has no command"]);
    }

    #[test]
    fn missing_top_level_metadata_becomes_warnings() {
        let dir = PluginDir::new("metadata", "id = \"test.plugin\"\n");
        let preview = preview(&dir.0).expect("only the plugin id is required to preview");

        assert_eq!(preview.name, "test.plugin", "the name falls back to the id");
        assert_eq!(preview.version, "");
        assert_eq!(
            preview.warnings,
            vec![
                "plugin name is required",
                "plugin version is required",
                "plugin min_herdr_version is required",
                "manifest does not declare platforms; platform support unknown",
            ]
        );
    }

    #[test]
    fn a_link_handler_runs_the_argv_of_the_action_it_names() {
        let manifest = format!(
            "{HEADER}[[actions]]\nid = \"open\"\ntitle = \"Open\"\ncommand = [\"node\", \"open.js\"]\n\n[[link_handlers]]\nid = \"issue\"\ntitle = \"Issues\"\npattern = \"^https://\"\naction = \"open\"\n"
        );
        let dir = PluginDir::new("link-handler", &manifest);
        let preview = preview(&dir.0).expect("the manifest previews");

        let expected = vec![
            CommandLine {
                kind: CommandKind::Action,
                id: "open".to_string(),
                title: "action open".to_string(),
                argv: argv(&["node", "open.js"]),
            },
            CommandLine {
                kind: CommandKind::LinkHandler,
                id: "issue".to_string(),
                title: "link handler issue".to_string(),
                argv: argv(&["node", "open.js"]),
            },
        ];
        assert_eq!(preview.commands, expected);
        assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);
        assert!(preview.hazards.is_empty(), "{:?}", preview.hazards);
    }

    #[test]
    fn a_link_handler_carries_the_hazard_of_the_action_it_runs() {
        let manifest = format!(
            "{HEADER}[[actions]]\nid = \"open\"\ntitle = \"Open\"\ncommand = [\"sh\", \"-c\", \"echo clicked\"]\n\n[[link_handlers]]\nid = \"issue\"\ntitle = \"Issues\"\npattern = \"^https://\"\naction = \"open\"\n"
        );
        let dir = PluginDir::new("link-handler-hazard", &manifest);
        let preview = preview(&dir.0).expect("the manifest previews");

        let seen: Vec<(usize, &str)> = preview
            .hazards
            .iter()
            .map(|hazard| (hazard.index, hazard.reason))
            .collect();
        let inline_shell = reason_for("inline shell");
        assert_eq!(seen, vec![(0, inline_shell), (1, inline_shell)]);
    }

    #[test]
    fn a_link_handler_with_an_unknown_action_warns_and_runs_nothing() {
        let manifest = format!(
            "{HEADER}[[link_handlers]]\nid = \"issue\"\ntitle = \"Issues\"\npattern = \"^https://\"\naction = \"missing\"\n"
        );
        let dir = PluginDir::new("unknown-action", &manifest);
        let preview = preview(&dir.0).expect("the rest of the manifest still previews");

        assert!(preview.commands.is_empty());
        assert_eq!(
            preview.warnings,
            vec!["link handler 'issue' references unknown action 'missing'"]
        );
    }

    #[test]
    fn rendering_a_command_quotes_only_what_a_reader_needs() {
        let slow = CommandLine {
            kind: CommandKind::Action,
            id: "slow".to_string(),
            title: "action slow".to_string(),
            argv: argv(&["sh", "-c", "sleep 1; echo slow done"]),
        };
        assert_eq!(
            render_command(&slow),
            "action slow: sh -c \"sleep 1; echo slow done\""
        );

        let build = CommandLine {
            kind: CommandKind::Build,
            id: String::new(),
            title: "build".to_string(),
            argv: argv(&["npm", "run", "build"]),
        };
        assert_eq!(render_command(&build), "build: npm run build");

        let escaped = CommandLine {
            kind: CommandKind::Event,
            id: "worktree.created".to_string(),
            title: "event worktree.created".to_string(),
            argv: argv(&["sh", "-c", "echo \"$HERDR_SOCKET_PATH\"", ""]),
        };
        assert_eq!(
            render_command(&escaped),
            "event worktree.created: sh -c \"echo \\\"$HERDR_SOCKET_PATH\\\"\" \"\""
        );
    }
}
