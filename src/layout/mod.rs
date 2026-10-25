//! The project layout template: what a project's tab looks like, kept as a file
//! this client can read and write.
//!
//! herdr has no template concept of its own. It persists whole sessions
//! (`session.json`, `session-snapshots/`) in a different internal shape — u32 ids
//! and `Horizontal`/`Vertical` — and nothing in it says what a project should
//! open with. The template is zenix's, and it is deliberately the daemon's own
//! shape rather than a new one: the file is `LayoutApplyParams` without the target
//! ids, which is the shape `layout.export` answers with and `layout.apply` accepts.
//!
//! Both methods live only on the JSON API lane. The endpoint lane's 40-method
//! whitelist carries `layout.set_split_ratio` and answers these two with
//! `unsupported_method` (`herdr/src/server/client_commands.rs:15-56`), so the calls
//! here go through `crate::herdr::api` — the lane the herdr CLI itself uses.
//!
//! `layout.apply` always creates a new tab and is not idempotent: every call makes
//! another one, and a `pane_id` inside a template is read and then ignored. That is
//! why restoring is a command a person runs, never something the client does on its
//! own when a project is entered. Focus inside a restored tab cannot be asked for
//! at all: `apply` brings the tab forward and leaves its root pane active.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::herdr::api::{ApiClient, required_field};

/// The directory a project's template lives in, and its file name.
pub(crate) const DIR: &str = ".zenix";
pub(crate) const FILE_NAME: &str = "layout.toml";

/// How far up from a pane's cwd the walk to the project goes. A pane's cwd is the
/// only starting point the daemon carries, and a walk with no bound would reach the
/// filesystem root.
pub(crate) const MAX_UPWARD: usize = 12;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("the template {path} is not a layout: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("the template cannot be written as a layout: {source}")]
    Format { source: toml::ser::Error },
    #[error("the template cannot be sent as an apply request: {source}")]
    Encode { source: serde_json::Error },
    #[error(transparent)]
    Api(#[from] crate::herdr::api::ApiError),
}

/// One node of the tree, field for field the daemon's `LayoutNode`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Node {
    Pane {
        #[serde(flatten)]
        pane: Pane,
    },
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SplitDirection {
    Right,
    Down,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Pane {
    /// The daemon's id for the pane. It arrives with an export and is cleared
    /// before the file is written: a template names no ids, because the panes it
    /// creates are new ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) command: Option<Vec<String>>,
    /// The daemon's `HashMap`, mirrored as a `BTreeMap`. The file is written to be
    /// read, diffed and committed, so the same template has to print the same text
    /// every time.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) env: BTreeMap<String, String>,
}

/// A project's template: `LayoutApplyParams` with the target ids taken out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Template {
    /// The name the restored tab gets. An export cannot carry it — the daemon's
    /// export answers with the tree and its ids, not with the tab's label — so this
    /// is a field a person writes into the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) tab_label: Option<String>,
    /// Whether the restored tab is brought to the front. The daemon's own field,
    /// and it defaults to false there; a template written by this client says true,
    /// because a restore that lands somewhere unseen is not what a restore is for.
    #[serde(default)]
    pub(crate) focus: bool,
    pub(crate) root: Node,
}

/// The daemon's `LayoutDescription`, as an export or an apply answers it: the same
/// tree, with the ids the daemon owns. `zoomed` and `focused_pane_id` are not
/// mirrored, because a template cannot carry either.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Description {
    pub(crate) workspace_id: String,
    pub(crate) tab_id: String,
    pub(crate) root: Node,
}

impl Template {
    /// The same template with the daemon's pane ids cleared, which is what a file
    /// holds.
    pub(crate) fn stripped(&self) -> Self {
        Self {
            tab_label: self.tab_label.clone(),
            focus: self.focus,
            root: self.root.stripped(),
        }
    }

    /// The request `layout.apply` takes for one workspace. The file's shape is the
    /// request's shape, so this is the target id added to it and nothing else.
    pub(crate) fn params(&self, workspace_id: &str) -> Result<Value, Error> {
        let mut params =
            serde_json::to_value(self.stripped()).map_err(|source| Error::Encode { source })?;
        params["workspace_id"] = json!(workspace_id);
        Ok(params)
    }
}

impl Node {
    /// How many panes the tree holds: the leaves.
    pub(crate) fn panes(&self) -> usize {
        match self {
            Node::Pane { .. } => 1,
            Node::Split { first, second, .. } => first.panes() + second.panes(),
        }
    }

    fn stripped(&self) -> Self {
        match self {
            Node::Pane { pane } => Node::Pane {
                pane: Pane {
                    pane_id: None,
                    label: pane.label.clone(),
                    cwd: pane.cwd.clone(),
                    command: pane.command.clone(),
                    env: pane.env.clone(),
                },
            },
            Node::Split {
                direction,
                ratio,
                first,
                second,
            } => Node::Split {
                direction: *direction,
                ratio: *ratio,
                first: Box::new(first.stripped()),
                second: Box::new(second.stripped()),
            },
        }
    }
}

impl Description {
    /// The tree as a template: the ids the daemon owns are dropped, because a
    /// template creates new panes rather than naming existing ones.
    pub(crate) fn template(&self) -> Template {
        Template {
            tab_label: None,
            focus: true,
            root: self.root.stripped(),
        }
    }
}

/// The project a template belongs to: the first directory up from `cwd` that holds
/// a `.git`, or `cwd` itself when nothing above it is a repository. A workspace can
/// be a subdirectory of the project, so the walk is what makes the project's file
/// findable from it.
pub(crate) fn project_root(cwd: &Path) -> PathBuf {
    let mut path = cwd;
    for _ in 0..MAX_UPWARD {
        if path.join(".git").exists() {
            return path.to_path_buf();
        }
        let Some(parent) = path.parent() else { break };
        path = parent;
    }
    cwd.to_path_buf()
}

/// The template file for the project `cwd` belongs to, walking up from it. Nothing
/// found is not an error: it means the project has no template yet.
pub(crate) fn find(cwd: &Path) -> Option<PathBuf> {
    let mut path = cwd;
    for _ in 0..MAX_UPWARD {
        let candidate = path.join(DIR).join(FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        let Some(parent) = path.parent() else { break };
        path = parent;
    }
    None
}

pub(crate) fn read(path: &Path) -> Result<Template, Error> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| Error::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// The file's text: the same bytes [`write`] puts on disk, for the block's
/// preview of what the project carries.
pub(crate) fn to_text(template: &Template) -> String {
    toml::to_string(&template.stripped()).unwrap_or_default()
}

/// Write the template, creating the project's `.zenix` directory on the way. The
/// ids are cleared here rather than by the caller, so a template read from a running
/// daemon cannot carry its ids into the file.
pub(crate) fn write(path: &Path, template: &Template) -> Result<(), Error> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })?;
    let text = toml::to_string(&template.stripped()).map_err(|source| Error::Format { source })?;
    std::fs::write(path, text).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Read one tab's layout off the daemon and return it as a template.
pub(crate) fn export(client: &ApiClient, tab_id: &str) -> Result<Template, Error> {
    let result = client.call("layout.export", json!({ "tab_id": tab_id }))?;
    let layout: Description = required_field(&result, "layout")?;
    Ok(layout.template())
}

/// Restore a template into one workspace. The daemon always creates a new tab, so
/// this is something a person asked for.
pub(crate) fn apply(
    client: &ApiClient,
    workspace_id: &str,
    template: &Template,
) -> Result<Description, Error> {
    let result = client.call("layout.apply", template.params(workspace_id)?)?;
    let layout: Description = required_field(&result, "layout")?;
    Ok(layout)
}

/// What the layout block knows about the project it speaks for.
#[derive(Clone, Debug, Default)]
pub(crate) struct LayoutState {
    /// The pane cwd the last scan ran for. A scan is a handful of directory checks
    /// and one small file, so it runs when the cwd moves and not every frame.
    pub(crate) cwd: Option<String>,
    pub(crate) root: Option<PathBuf>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) template: Option<Template>,
    /// What the last save or restore did, until the next one.
    pub(crate) last: Option<String>,
    /// Why the last read, save or restore failed, until one succeeds.
    pub(crate) error: Option<String>,
}

impl LayoutState {
    /// Walk up from `cwd` to the project and read its template. A read that fails
    /// is reported here; one that succeeds does not erase what a save or a restore
    /// reported, because the block shows the last thing that happened.
    pub(crate) fn scan(&mut self, cwd: &Path) {
        self.cwd = Some(cwd.to_string_lossy().into_owned());
        self.root = Some(project_root(cwd));
        self.path = find(cwd);
        self.template = None;
        let Some(path) = self.path.clone() else {
            return;
        };
        match read(&path) {
            Ok(template) => self.template = Some(template),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    /// What a save or a restore did, as the block shows it.
    pub(crate) fn record(&mut self, outcome: Result<String, Error>) {
        match outcome {
            Ok(what) => {
                self.last = Some(what);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    fn pane(label: &str) -> Node {
        Node::Pane {
            pane: Pane {
                label: Some(label.to_owned()),
                ..Pane::default()
            },
        }
    }

    fn template() -> Template {
        Template {
            tab_label: Some("zenix".to_owned()),
            focus: true,
            root: Node::Split {
                direction: SplitDirection::Right,
                ratio: 0.5,
                first: Box::new(pane("code")),
                second: Box::new(Node::Split {
                    direction: SplitDirection::Down,
                    ratio: 0.25,
                    first: Box::new(pane("notes")),
                    second: Box::new(Node::Pane {
                        pane: Pane {
                            command: Some(vec!["cargo".to_owned(), "test".to_owned()]),
                            env: BTreeMap::from([("HERDR_ENV".to_owned(), "1".to_owned())]),
                            ..Pane::default()
                        },
                    }),
                }),
            },
        }
    }

    #[test]
    fn a_template_round_trips_through_the_file() {
        let text = toml::to_string(&template()).expect("the template is writable");
        let back: Template = toml::from_str(&text).expect("the same text reads back");
        assert_eq!(back, template());
    }

    /// The file is not a zenix schema: it is the daemon's apply request, so the
    /// JSON this client sends has the field names the daemon already declares.
    #[test]
    fn the_file_shape_is_the_apply_request_shape() {
        let params = template().params("w1").expect("a request");
        assert_eq!(params["workspace_id"], "w1");
        assert_eq!(params["tab_label"], "zenix");
        assert_eq!(params["focus"], true);
        assert_eq!(params["root"]["type"], "split");
        assert_eq!(params["root"]["first"]["type"], "pane");
        assert_eq!(params["root"]["second"]["first"]["type"], "pane");
    }

    /// A template read from a running daemon carries the daemon's pane ids; the file
    /// it is written to must not.
    #[test]
    fn a_template_written_from_an_export_carries_no_ids() {
        let description = Description {
            workspace_id: "w1".to_owned(),
            tab_id: "t1".to_owned(),
            root: Node::Pane {
                pane: Pane {
                    pane_id: Some("w1:p2".to_owned()),
                    label: Some("code".to_owned()),
                    ..Pane::default()
                },
            },
        };
        let template = description.template();
        assert_eq!(template.root, pane("code"));
        let params = template.params("w1").expect("a request");
        assert!(
            params["root"].get("pane_id").is_none(),
            "the request carries no pane id"
        );
    }

    #[test]
    fn the_pane_count_counts_leaves() {
        assert_eq!(template().root.panes(), 3);
        assert_eq!(pane("only").panes(), 1);
    }

    /// The file is meant to be read and diffed, so the same template has to print
    /// the same text however its environment arrived.
    #[test]
    fn env_is_written_in_its_own_order() {
        // The keys are named in one order and written in another: the file is meant
        // to be diffed, so the same template prints the same text every time.
        let pane = Pane {
            env: BTreeMap::from([
                ("ZZ".to_owned(), "2".to_owned()),
                ("AA".to_owned(), "1".to_owned()),
            ]),
            ..Pane::default()
        };
        let text = toml::to_string(&Node::Pane { pane }).expect("a pane is writable");
        let env = text
            .split_once("[env]")
            .expect("the env table is written")
            .1;
        let lines: Vec<&str> = env.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines, vec!["AA = \"1\"", "ZZ = \"2\""], "{text}");
    }

    #[test]
    fn the_project_root_is_the_first_directory_up_that_is_a_repository() {
        let root = std::env::temp_dir().join("zenix_layout_root");
        let nested = root.join("apps").join("worker");
        // A run that panicked before its cleanup leaves a fixture behind.
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).expect("the fixture is created");
        assert_eq!(project_root(&nested), root);
        // Nothing above it is a repository, so the cwd is its own root.
        let loose = std::env::temp_dir().join("zenix_layout_loose");
        std::fs::create_dir_all(&loose).expect("the fixture is created");
        assert_eq!(project_root(&loose), loose);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(loose);
    }

    #[test]
    fn a_template_is_found_from_a_subdirectory_of_the_project() {
        let root = std::env::temp_dir().join("zenix_layout_find");
        let nested = root.join("apps").join("worker");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&nested).expect("the fixture is created");
        std::fs::create_dir_all(root.join(DIR)).expect("the fixture is created");
        std::fs::write(
            root.join(DIR).join(FILE_NAME),
            "[root]\ntype = \"pane\"\nlabel = \"code\"\n",
        )
        .expect("the fixture template is written");
        assert_eq!(find(&nested), Some(root.join(DIR).join(FILE_NAME)));
        let missing = std::env::temp_dir().join("zenix_layout_missing");
        std::fs::create_dir_all(&missing).expect("the fixture is created");
        assert_eq!(find(&missing), None);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(missing);
    }

    #[test]
    fn a_template_that_cannot_be_read_leaves_the_reason_behind() {
        let root = std::env::temp_dir().join("zenix_layout_bad");
        std::fs::create_dir_all(root.join(DIR)).expect("the fixture is created");
        std::fs::write(root.join(DIR).join(FILE_NAME), "not a layout")
            .expect("the fixture is written");
        let mut state = LayoutState::default();
        state.scan(&root);
        assert_eq!(state.path, Some(root.join(DIR).join(FILE_NAME)));
        assert!(state.template.is_none());
        assert!(state.error.is_some(), "{state:?}");
        let _ = std::fs::remove_dir_all(root);
    }
}
