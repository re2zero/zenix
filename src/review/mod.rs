//! The focused workspace's changes: `git status` and `git diff`, read off the
//! UI thread.
//!
//! The daemon says which repository the focused workspace sits in
//! (`worktree.list`'s `repo_root`) and never reports file-level changes, so the
//! client runs git itself: read-only commands, no shell, one worker thread, one
//! pulse per answer. The surface is `ui::review`; this module only turns git's
//! machine formats into rows.
//!
//! Command shapes checked against git 2.51 on 2026-10-29:
//!
//! * `status --porcelain=v1 -z --branch --untracked-files=all` — the first
//!   record is `## <branch>...<upstream> [ahead N, behind M]`, then one
//!   `XY <path>` per file; a rename or copy adds the old path as the next
//!   record.
//! * `diff --numstat -z` — `added \t deleted \t path` per file; a rename has
//!   an empty path and the old/new names in the next two records. A binary
//!   file reports `-`.
//! * `diff --no-index -- /dev/null <path>` — an untracked file as a new-file
//!   diff. It exits 1 because the files differ, which is the answer, not a
//!   failure.
//!
//! Every read is bounded: the diff body is cut at [`MAX_DIFF_LINES`] and the
//! cut is reported, so a generated file cannot turn one paint into a million
//! rows.

use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use crossbeam_channel::{Receiver, Sender};

/// The most diff lines one file is read into. A minified or generated file can
/// be millions of lines; the sheet says it was cut rather than freezing.
const MAX_DIFF_LINES: usize = 4000;

/// One changed file with its two index sides, as porcelain reports them.
///
/// `index` is the staged letter (`' '` when only the worktree moved) and
/// `worktree` the unstaged one; `?` on both means untracked. The counts are
/// per side because a partially staged file has two different diffs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ChangedFile {
    pub(crate) path: String,
    pub(crate) index: char,
    pub(crate) worktree: char,
    pub(crate) staged_additions: u64,
    pub(crate) staged_deletions: u64,
    pub(crate) worktree_additions: u64,
    pub(crate) worktree_deletions: u64,
    pub(crate) binary: bool,
    pub(crate) untracked: bool,
}

impl ChangedFile {
    /// Whether the index side holds a change for this file.
    pub(crate) fn staged(&self) -> bool {
        !self.untracked && self.index != ' '
    }

    /// Whether the worktree side holds a change (untracked counts).
    pub(crate) fn unstaged(&self) -> bool {
        self.untracked || self.worktree != ' '
    }

    /// The letter a row shows for one side.
    pub(crate) fn letter(&self, staged: bool) -> char {
        if self.untracked {
            '?'
        } else if staged {
            self.index
        } else {
            self.worktree
        }
    }
}

/// The repository as one read found it.
#[derive(Clone, Debug, Default)]
pub(crate) struct ReviewState {
    pub(crate) root: String,
    pub(crate) branch: Option<String>,
    pub(crate) ahead: usize,
    pub(crate) behind: usize,
    pub(crate) files: Vec<ChangedFile>,
    /// Why the last read failed, verbatim from git.
    pub(crate) error: Option<String>,
}

impl ReviewState {
    pub(crate) fn staged(&self) -> impl Iterator<Item = &ChangedFile> {
        self.files.iter().filter(|file| file.staged())
    }

    pub(crate) fn unstaged(&self) -> impl Iterator<Item = &ChangedFile> {
        self.files.iter().filter(|file| file.unstaged())
    }
}

/// The diff one file was last read as.
#[derive(Clone, Debug, Default)]
pub(crate) struct DiffState {
    pub(crate) path: Option<String>,
    pub(crate) staged: bool,
    pub(crate) text: String,
    pub(crate) truncated: bool,
    pub(crate) error: Option<String>,
    /// The read this answer belongs to. A slower, older read loses to a newer
    /// one instead of overwriting the file the reader just picked.
    pub(crate) generation: u64,
}

enum Request {
    Refresh {
        root: String,
    },
    Diff {
        root: String,
        path: String,
        staged: bool,
        untracked: bool,
        generation: u64,
    },
}

/// The review host: one worker thread, two caches, one pulse channel.
pub(crate) struct ReviewHost {
    review: Arc<Mutex<ReviewState>>,
    diff: Arc<Mutex<DiffState>>,
    commands: Sender<Request>,
}

impl ReviewHost {
    pub(crate) fn start() -> (Self, Receiver<()>) {
        let review = Arc::new(Mutex::new(ReviewState::default()));
        let diff = Arc::new(Mutex::new(DiffState::default()));
        let (commands, requests) = crossbeam_channel::unbounded();
        let (pulses_out, pulses) = crossbeam_channel::bounded(1);
        let worker_review = review.clone();
        let worker_diff = diff.clone();
        thread::Builder::new()
            .name("zenix-review".into())
            .spawn(move || work(worker_review, worker_diff, requests, pulses_out))
            .expect("failed to spawn the review worker thread");
        (
            Self {
                review,
                diff,
                commands,
            },
            pulses,
        )
    }

    /// A copy of the last status read.
    pub(crate) fn state(&self) -> ReviewState {
        lock(&self.review).clone()
    }

    /// A copy of the last diff read.
    pub(crate) fn diff(&self) -> DiffState {
        lock(&self.diff).clone()
    }

    pub(crate) fn refresh(&self, root: String) {
        let _ = self.commands.send(Request::Refresh { root });
    }

    pub(crate) fn read_diff(
        &self,
        root: String,
        path: String,
        staged: bool,
        untracked: bool,
        generation: u64,
    ) {
        let _ = self.commands.send(Request::Diff {
            root,
            path,
            staged,
            untracked,
            generation,
        });
    }
}

fn work(
    review: Arc<Mutex<ReviewState>>,
    diff: Arc<Mutex<DiffState>>,
    requests: Receiver<Request>,
    pulses: Sender<()>,
) {
    while let Ok(request) = requests.recv() {
        match request {
            Request::Refresh { root } => {
                *lock(&review) = read_review(&root);
            }
            Request::Diff {
                root,
                path,
                staged,
                untracked,
                generation,
            } => {
                let answer = read_diff(&root, &path, staged, untracked);
                let mut cache = lock(&diff);
                if cache.generation <= generation {
                    cache.path = Some(path);
                    cache.staged = staged;
                    cache.text = answer.text;
                    cache.truncated = answer.truncated;
                    cache.error = answer.error;
                    cache.generation = generation;
                }
            }
        }
        // Bounded at one: a burst of answers coalesces into one repaint.
        let _ = pulses.try_send(());
    }
}

fn read_review(root: &str) -> ReviewState {
    let mut state = ReviewState {
        root: root.to_owned(),
        ..Default::default()
    };
    let status = match run_git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
        &[0],
    ) {
        Ok(output) => output,
        Err(reason) => {
            state.error = Some(reason);
            return state;
        }
    };
    let (branch, ahead, behind) = parse_branch(&status);
    let mut files = parse_status(&status);
    if let Ok(raw) = run_git(root, &["diff", "--numstat", "-z", "--cached"], &[0]) {
        apply_numstat(&mut files, &parse_numstat(&raw), true);
    }
    if let Ok(raw) = run_git(root, &["diff", "--numstat", "-z"], &[0]) {
        apply_numstat(&mut files, &parse_numstat(&raw), false);
    }
    state.branch = branch;
    state.ahead = ahead;
    state.behind = behind;
    state.files = files;
    state
}

struct DiffAnswer {
    text: String,
    truncated: bool,
    error: Option<String>,
}

fn read_diff(root: &str, path: &str, staged: bool, untracked: bool) -> DiffAnswer {
    // An untracked file has no side to ask: `--no-index` against /dev/null is
    // the one command that spells it as a new file. It exits 1 on purpose.
    let (args, ok): (Vec<&str>, &[i32]) = if untracked {
        (
            vec!["diff", "--no-index", "-U3", "--", "/dev/null", path],
            &[0, 1],
        )
    } else if staged {
        (vec!["diff", "--cached", "-U3", "--", path], &[0])
    } else {
        (vec!["diff", "-U3", "--", path], &[0])
    };
    match run_git(root, &args, ok) {
        Ok(text) => {
            let (text, truncated) = cut_diff(&text);
            DiffAnswer {
                text,
                truncated,
                error: None,
            }
        }
        Err(reason) => DiffAnswer {
            text: String::new(),
            truncated: false,
            error: Some(reason),
        },
    }
}

/// Keep at most [`MAX_DIFF_LINES`] and say whether anything was dropped.
fn cut_diff(text: &str) -> (String, bool) {
    let mut lines: Vec<&str> = text.lines().collect();
    let truncated = lines.len() > MAX_DIFF_LINES;
    if truncated {
        lines.truncate(MAX_DIFF_LINES);
    }
    (lines.join("\n"), truncated)
}

fn run_git(root: &str, args: &[&str], ok: &[i32]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    let code = output.status.code().unwrap_or(-1);
    if !ok.contains(&code) {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!("git exited {code}")
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The porcelain status records, minus the branch line. A rename or copy
/// record carries the old path as the record after it; that record is consumed
/// here so it cannot be read as a file of its own.
fn parse_status(raw: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut records = raw.split('\0').filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        if record.starts_with("## ") {
            continue;
        }
        let mut chars = record.chars();
        let index = chars.next().unwrap_or(' ');
        let worktree = chars.next().unwrap_or(' ');
        let path = record.get(3..).unwrap_or_default();
        if path.is_empty() {
            continue;
        }
        let mut file = ChangedFile {
            path: path.to_owned(),
            index,
            worktree,
            untracked: index == '?' && worktree == '?',
            ..Default::default()
        };
        if matches!(index, 'R' | 'C') {
            let _old = records.next();
        }
        if file.untracked {
            file.index = '?';
        }
        files.push(file);
    }
    files
}

/// The branch line: name, and how far it is from its upstream when the status
/// says so.
fn parse_branch(raw: &str) -> (Option<String>, usize, usize) {
    let line = raw.split('\0').next().unwrap_or_default();
    let line = line.strip_prefix("## ").unwrap_or_default();
    let (name, rest) = line.split_once(" [").unwrap_or((line, ""));
    let name = name.split("...").next().unwrap_or(name).trim();
    let branch = if name == "HEAD (no branch)" || name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    };
    let mut ahead = 0;
    let mut behind = 0;
    if let Some(rest) = rest.strip_suffix(']') {
        for part in rest.split(',') {
            let part = part.trim();
            if let Some(value) = part.strip_prefix("ahead ") {
                ahead = value.trim().parse().unwrap_or(0);
            }
            if let Some(value) = part.strip_prefix("behind ") {
                behind = value.trim().parse().unwrap_or(0);
            }
        }
    }
    (branch, ahead, behind)
}

struct Numstat {
    path: String,
    additions: u64,
    deletions: u64,
    binary: bool,
}

/// The numstat records. A rename has an empty path and its old and new names
/// in the next two records; the new one is the path the row shows.
fn parse_numstat(raw: &str) -> Vec<Numstat> {
    let mut stats = Vec::new();
    let mut records = raw.split('\0').filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        let mut fields = record.split('\t');
        let added = fields.next().unwrap_or_default();
        let deleted = fields.next().unwrap_or_default();
        let path = fields.next().unwrap_or_default();
        let binary = added == "-" && deleted == "-";
        let (additions, deletions) = match (added.parse(), deleted.parse()) {
            (Ok(additions), Ok(deletions)) => (additions, deletions),
            _ => (0, 0),
        };
        let path = if path.is_empty() {
            let _old = records.next();
            records.next().unwrap_or_default().to_owned()
        } else {
            path.to_owned()
        };
        if path.is_empty() {
            continue;
        }
        stats.push(Numstat {
            path,
            additions,
            deletions,
            binary,
        });
    }
    stats
}

fn apply_numstat(files: &mut [ChangedFile], stats: &[Numstat], staged: bool) {
    for stat in stats {
        if let Some(file) = files.iter_mut().find(|file| file.path == stat.path) {
            if staged {
                file.staged_additions = stat.additions;
                file.staged_deletions = stat.deletions;
            } else {
                file.worktree_additions = stat.additions;
                file.worktree_deletions = stat.deletions;
            }
            file.binary |= stat.binary;
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_parser_keeps_both_sides_and_the_untracked() {
        let raw = "## main...origin/main [ahead 2, behind 1]\u{0}M  staged.txt\u{0} M worktree.txt\u{0}?? new.txt\u{0}";
        let mut files = parse_status(raw);
        files.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(files.len(), 3);
        let staged = files.iter().find(|f| f.path == "staged.txt").unwrap();
        assert!(staged.staged());
        assert!(!staged.unstaged());
        let moved = files.iter().find(|f| f.path == "worktree.txt").unwrap();
        assert!(!moved.staged());
        assert!(moved.unstaged());
        let new = files.iter().find(|f| f.path == "new.txt").unwrap();
        assert!(new.untracked);
        assert!(new.unstaged());
        assert_eq!(new.letter(true), '?');
        assert_eq!(moved.letter(false), 'M');
    }

    #[test]
    fn the_branch_line_says_the_name_and_the_gap() {
        assert_eq!(
            parse_branch("## main...origin/main [ahead 2, behind 1]\u{0}"),
            (Some("main".to_owned()), 2, 1)
        );
        assert_eq!(
            parse_branch("## main\u{0}"),
            (Some("main".to_owned()), 0, 0)
        );
        assert_eq!(parse_branch("## HEAD (no branch)\u{0}"), (None, 0, 0));
    }

    #[test]
    fn a_rename_record_does_not_become_a_second_file() {
        let raw = "## main\u{0}RM renamed.txt\u{0}old.txt\u{0} M other.txt\u{0}";
        let files = parse_status(raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "renamed.txt");
        assert_eq!(files[1].path, "other.txt");
    }

    #[test]
    fn numstat_counts_land_on_the_side_that_reported_them() {
        let mut files = vec![ChangedFile {
            path: "a.txt".into(),
            index: 'M',
            worktree: 'M',
            ..Default::default()
        }];
        apply_numstat(&mut files, &parse_numstat("2\t0\ta.txt\u{0}"), true);
        apply_numstat(&mut files, &parse_numstat("1\t3\ta.txt\u{0}"), false);
        assert_eq!(files[0].staged_additions, 2);
        assert_eq!(files[0].staged_deletions, 0);
        assert_eq!(files[0].worktree_additions, 1);
        assert_eq!(files[0].worktree_deletions, 3);
    }

    #[test]
    fn a_rename_numstat_uses_the_new_name_and_a_binary_file_is_marked() {
        let stats = parse_numstat("0\t0\t\u{0}old.txt\u{0}new.txt\u{0}-\t-\tpic.png\u{0}");
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].path, "new.txt");
        assert!(!stats[0].binary);
        assert_eq!(stats[1].path, "pic.png");
        assert!(stats[1].binary);
    }

    #[test]
    fn a_diff_is_cut_at_the_cap_and_says_so() {
        let lines: Vec<String> = (0..MAX_DIFF_LINES + 5).map(|i| format!("+{i}")).collect();
        let text = lines.join("\n");
        let (kept, truncated) = cut_diff(&text);
        assert!(truncated);
        assert_eq!(kept.lines().count(), MAX_DIFF_LINES);
        // A diff under the cap is returned whole.
        let (kept, truncated) = cut_diff("+one\n+two");
        assert!(!truncated);
        assert_eq!(kept, "+one\n+two");
    }
}
