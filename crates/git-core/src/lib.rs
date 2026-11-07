//! Git inspection and explicitly requested working-copy operations. Every command uses arguments, and
//! object reads never invoke external diff drivers, textconv, hooks, or fetch.
//!
//! Methods are blocking and belong on a worker thread, never a UI render thread.
//! Clones share a persistent `cat-file` process; its lock only protects the wire
//! protocol. Repository operations themselves do not take that lock.

mod blame;
mod conflict_blocks;
mod history;
mod inspection;
mod local_watch;
mod preview_assets;
mod process_io;
mod work;
pub use blame::*;
pub use conflict_blocks::*;
pub use history::*;
pub use inspection::*;
pub use local_watch::*;
pub use preview_assets::*;
pub use work::*;

use anyhow::{Context, Result, bail, ensure};
use process_io::{ChildPipes, PipeControl, StopPipe};
use sha2::{Digest, Sha256};
use similar::{Algorithm, TextDiff};
use std::{
    ffi::{OsStr, OsString},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Encoded blob limit. Image decoders should additionally limit decoded pixels.
pub const MAX_BLOB_BYTES: usize = 64 * 1024 * 1024;
/// A line diff is optional for larger files; the file list is always available.
pub const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_DIFF_LINES: usize = 100_000;
const GIT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_COMMAND_OUTPUT: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub oid: String,
    pub parents: Vec<String>,
    pub author: String,
    pub timestamp: i64,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub oid: String,
    pub remote: bool,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub oid: String,
    pub detached: bool,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChanged,
}

impl ChangeStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Added => "Added",
            Self::Modified => "Modified",
            Self::Deleted => "Deleted",
            Self::Renamed => "Renamed",
            Self::TypeChanged => "Type changed",
        }
    }

    pub fn letter(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::TypeChanged => "T",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub old_path: Option<PathBuf>,
    pub new_path: Option<PathBuf>,
    pub old_oid: Option<String>,
    pub new_oid: Option<String>,
    pub status: ChangeStatus,
    pub old_mode: String,
    pub new_mode: String,
}

impl FileChange {
    pub fn path(&self) -> &Path {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or_else(|| Path::new(""))
    }

    pub fn is_submodule(&self) -> bool {
        self.old_mode == "160000" || self.new_mode == "160000"
    }
}

/// Useful preview states are separate from operational errors (missing objects,
/// permissions, corrupt objects). `diff` offers a string convenience interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextPreview {
    Patch(String),
    Binary,
    TooLarge {
        old_bytes: usize,
        new_bytes: usize,
    },
    Submodule {
        old_oid: Option<String>,
        new_oid: Option<String>,
    },
}

/// Source bytes are present only when they fit the text-preview budget. Reuse
/// these for split views instead of decompressing the same Git objects twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPreviewWithSources {
    pub preview: TextPreview,
    pub old: Vec<u8>,
    pub new: Vec<u8>,
}

/// A repository [`GitRepository::open`] refused: Git, asked again once the
/// files that can move its work tree or common directory were stamped,
/// found another repository at its path (by what differs), or none, so
/// what was captured is not what Git would act on.
#[derive(Debug)]
pub struct ChangedWhileOpening {
    /// The work-tree root (a bare repository's Git directory) Git reported.
    pub path: PathBuf,
    /// What Git finds there now, if anything.
    pub found: Option<PathBuf>,
}

impl std::fmt::Display for ChangedWhileOpening {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "The repository at {} changed while it was opened",
            self.path.display()
        )?;
        match &self.found {
            Some(found) => write!(f, ": Git now finds {} there.", found.display()),
            None => write!(f, ": Git now finds no repository there."),
        }?;
        write!(f, " Open it again.")
    }
}

impl std::error::Error for ChangedWhileOpening {}

#[cfg(test)]
thread_local! {
    /// Runs in [`GitRepository::open`] between the capture and Git's
    /// confirmation on this thread: a test re-points what the capture read.
    static OPEN_CHECKPOINT: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

/// What Git finds at an opened repository's path now
/// ([`GitRepository::found_at_path`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FoundAtPath {
    /// The repository as it was opened.
    Same,
    /// Another repository: the work tree enclosing the path, its own `.git`
    /// gone, by its work-tree root (a bare repository's Git directory); or,
    /// at the same root, another repository's Git directory, by that
    /// directory (its `.git` swapped for another repository's gitfile).
    Other(PathBuf),
    /// No repository, or no folder at the path.
    None,
}

#[derive(Clone)]
pub struct GitRepository {
    path: PathBuf,
    bare: bool,
    identity: Arc<Identity>,
    batch: Arc<Mutex<Option<BatchReader>>>,
}

/// What an opened repository is, captured once at open: the Git directory
/// Git reported and the `.git` entry Git discovery finds from the root. They
/// confine every later command ([`GitTarget`]) and let a check compare them
/// in-process, without starting Git ([`GitRepository::verify_at_path`]).
#[derive(Debug)]
struct Identity {
    /// The work-tree root (a bare repository's Git directory), canonical.
    root: PathBuf,
    /// The Git directory Git reported at open, canonical.
    git_dir: PathBuf,
    /// Its common directory (a linked worktree's main Git directory, whose
    /// refs and objects it uses), canonical; `git_dir` itself otherwise.
    common: PathBuf,
    /// The folder holding the `.git` entry (a directory or a gitfile) that
    /// leads Git from the root to `git_dir`: the root itself, or a folder
    /// above it whose repository sets `core.worktree` to a folder inside.
    /// `None` for a bare repository, and where no such entry leads there.
    holder: Option<PathBuf>,
    /// The folder Git discovery must never enter from the repository's
    /// folders: the parent of the folder holding `.git` (of a bare
    /// repository's Git directory). `None` where there is none or where it
    /// cannot be named ([`Identity::confined`]).
    ceiling: Option<PathBuf>,
    /// Whether every command is confined: a ceiling placed, or nothing above
    /// to escape to. `false` where no `.git` entry leads to the Git
    /// directory, and where the ceiling's path holds the platform's list
    /// separator, which `GIT_CEILING_DIRECTORIES` would split into two paths
    /// that confine nothing; there, every write and Changes' read ask Git
    /// first ([`GitRepository::verify_at_path`]).
    confined: bool,
    /// The files that can move the work tree (`core.worktree`) or the
    /// repository without touching the `.git` entry: the Git directory's
    /// `config`, `config.worktree` and `commondir` (a linked worktree's
    /// refs and objects), and a linked worktree's common `config`.
    configs: Vec<PathBuf>,
    /// Their file identities as last confirmed: unchanged, the work tree has
    /// not moved; changed, Git is asked once and, the repository the same,
    /// they are confirmed again.
    stamps: Mutex<Vec<Option<FileStamp>>>,
}

/// A file's identity, as the app's retained repository identity reads it:
/// device, inode and change time where the platform has them, size and
/// modification time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStamp {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed: (i64, i64),
    size: u64,
    modified: Option<std::time::SystemTime>,
}

impl FileStamp {
    fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            size: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

/// `GIT_CEILING_DIRECTORIES`' list separator.
const LIST_SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };

impl Identity {
    fn capture(root: &Path, git_dir: &Path, bare: bool) -> Result<Self> {
        let root = root
            .canonicalize()
            .context("Resolve the repository's folder")?;
        let git_dir = git_dir
            .canonicalize()
            .context("Resolve the repository's Git directory")?;
        let holder = if bare {
            None
        } else {
            git_entry_holder(&root)
                .filter(|holder| resolve_git_entry(holder).as_ref() == Some(&git_dir))
                .map(Path::to_path_buf)
        };
        let enclosing = if bare {
            Some(git_dir.as_path())
        } else {
            holder.as_deref()
        };
        let (ceiling, confined) = match enclosing.map(Path::parent) {
            None => (None, false),
            Some(None) => (None, true),
            // Skipped where the variable would split the path and silently
            // confine nothing: the per-operation check stays there.
            Some(Some(parent))
                if parent
                    .as_os_str()
                    .as_encoded_bytes()
                    .contains(&LIST_SEPARATOR.as_bytes()[0]) =>
            {
                (None, false)
            }
            Some(Some(parent)) => (Some(parent.to_path_buf()), true),
        };
        let mut configs = vec![
            git_dir.join("config"),
            git_dir.join("config.worktree"),
            git_dir.join("commondir"),
        ];
        let common = common_dir(&git_dir).unwrap_or_else(|| git_dir.clone());
        if common != git_dir {
            configs.push(common.join("config"));
        }
        let stamps = Mutex::new(configs.iter().map(|path| FileStamp::read(path)).collect());
        Ok(Self {
            root,
            git_dir,
            common,
            holder,
            ceiling,
            confined,
            configs,
            stamps,
        })
    }

    fn config_stamps(&self) -> Vec<Option<FileStamp>> {
        self.configs
            .iter()
            .map(|path| FileStamp::read(path))
            .collect()
    }

    /// Whether the repository is still at `path` as captured, by the
    /// filesystem alone: the root resolves to the captured root (it was not
    /// turned into a link to another folder), the `.git` entry Git finds
    /// first from it is the captured one, and that entry still leads to the
    /// captured Git directory (not swapped for another repository's). The
    /// captured values are compared as they are, never resolved again.
    fn holds(&self, path: &Path, bare: bool) -> bool {
        if !path.canonicalize().is_ok_and(|now| now == self.root) {
            return false;
        }
        if bare {
            return true;
        }
        self.holder.as_deref().is_some_and(|holder| {
            git_entry_holder(path) == Some(holder)
                && resolve_git_entry(holder).as_ref() == Some(&self.git_dir)
        })
    }
}

/// A linked worktree's common Git directory, from its `commondir` file
/// (relative to `git_dir`), canonical; `None` where there is none.
fn common_dir(git_dir: &Path) -> Option<PathBuf> {
    let mut bytes = Vec::new();
    std::fs::File::open(git_dir.join("commondir"))
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let end = bytes
        .iter()
        .rposition(|byte| *byte != b'\n' && *byte != b'\r')
        .map_or(0, |last| last + 1);
    git_dir
        .join(path_from_bytes(&bytes[..end]))
        .canonicalize()
        .ok()
}

/// The nearest folder at or above `root` with a `.git` entry, where Git
/// discovery from `root` stops.
fn git_entry_holder(root: &Path) -> Option<&Path> {
    root.ancestors()
        .find(|folder| std::fs::symlink_metadata(folder.join(".git")).is_ok())
}

/// The Git directory the `.git` entry in `holder` leads to, canonical: the
/// directory itself, or a gitfile's `gitdir:` path, relative to `holder`.
fn resolve_git_entry(holder: &Path) -> Option<PathBuf> {
    let entry = holder.join(".git");
    if !std::fs::symlink_metadata(&entry).ok()?.is_file() {
        return entry.canonicalize().ok();
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&entry)
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let target = bytes.strip_prefix(b"gitdir: ")?;
    // Git strips only trailing line ends: a path ending in a space is
    // another folder, and one holding a NUL is not compared at all.
    let end = target
        .iter()
        .rposition(|byte| *byte != b'\n' && *byte != b'\r')
        .map_or(0, |last| last + 1);
    let target = &target[..end];
    if target.contains(&0) {
        return None;
    }
    holder.join(path_from_bytes(target)).canonicalize().ok()
}

/// Whether a command failed because Git found no repository: how every
/// command of an opened repository fails once its `.git` is gone.
pub fn finds_no_repository(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("not a git repository")
}

/// Where one Git process runs, and the folder Git discovery must not enter
/// from there. Every command an opened repository starts takes its target
/// from it ([`GitRepository::target`]); only open, init, clone and
/// [`GitRepository::found_at_path`]'s diagnosis run unconfined.
#[derive(Clone, Copy)]
pub(crate) struct GitTarget<'a> {
    directory: &'a Path,
    ceiling: Option<&'a Path>,
}

impl<'a> GitTarget<'a> {
    /// Discovery as Git does it with no repository opened yet.
    pub(crate) fn unconfined(directory: &'a Path) -> Self {
        Self {
            directory,
            ceiling: None,
        }
    }

    pub(crate) fn directory(&self) -> &'a Path {
        self.directory
    }

    /// Sets `GIT_CEILING_DIRECTORIES` to the person's own value, if any,
    /// followed by this target's ceiling: with the repository's `.git` gone,
    /// Git then stops with "not a git repository" instead of running on the
    /// work tree enclosing it. Hooks, filters and helpers inherit it; a
    /// hook's `git -C ..` still runs there, since Git never treats the folder
    /// it starts in as above a ceiling.
    fn confine(&self, command: &mut Command) {
        if let Some(ceiling) = self.ceiling {
            command.env(
                "GIT_CEILING_DIRECTORIES",
                ceiling_directories(std::env::var_os("GIT_CEILING_DIRECTORIES"), ceiling),
            );
        }
    }
}

/// The person's `GIT_CEILING_DIRECTORIES` with `ceiling` appended, never
/// replacing it.
fn ceiling_directories(person: Option<OsString>, ceiling: &Path) -> OsString {
    match person.filter(|value| !value.is_empty()) {
        Some(mut value) => {
            value.push(LIST_SEPARATOR);
            value.push(ceiling);
            value
        }
        None => ceiling.as_os_str().to_owned(),
    }
}

impl std::fmt::Debug for GitRepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitRepository")
            .field("path", &self.path)
            .field("bare", &self.bare)
            .finish_non_exhaustive()
    }
}

impl GitRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let requested = path.as_ref();
        ensure!(
            requested.is_dir(),
            "Not a directory: {}",
            requested.display()
        );
        // Discovery from the requested folder, unconfined: a folder inside a
        // work tree opens that work tree.
        let reported = run_git(
            GitTarget::unconfined(requested),
            &["rev-parse", "--is-bare-repository", "--absolute-git-dir"],
        )?;
        let split = reported
            .iter()
            .position(|byte| *byte == b'\n')
            .context("Git did not report the repository's Git directory")?;
        let bare = &reported[..split] == b"true";
        let git_dir = path_from_bytes(trim_line(&reported[split + 1..]));
        let path = if bare {
            git_dir.clone()
        } else {
            path_from_bytes(trim_line(&run_git(
                GitTarget::unconfined(requested),
                &["rev-parse", "--show-toplevel"],
            )?))
        };
        ensure!(
            path.is_absolute() && git_dir.is_absolute(),
            "Git returned a non-absolute repository path"
        );
        let identity = Arc::new(Identity::capture(&path, &git_dir, bare)?);
        let repository = Self {
            path,
            bare,
            identity,
            batch: Arc::new(Mutex::new(None)),
        };
        // Stamp, then ask, as `verify_at_path` orders it: the files that can
        // move the work tree or the common directory were stamped after Git
        // reported the root and `commondir` was read, so a rename between
        // the two left a handle bound to another work tree or common
        // directory whose stamps verified as its own. Git is asked once
        // more, after the stamps (one `rev-parse` per fresh open).
        #[cfg(test)]
        OPEN_CHECKPOINT.with_borrow_mut(|hook| {
            if let Some(hook) = hook {
                hook();
            }
        });
        match repository.found_at_path()? {
            FoundAtPath::Same => Ok(repository),
            FoundAtPath::Other(found) => Err(ChangedWhileOpening {
                path: repository.path,
                found: Some(found),
            }
            .into()),
            FoundAtPath::None => Err(ChangedWhileOpening {
                path: repository.path,
                found: None,
            }
            .into()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The Git directory Git reported when the repository was opened,
    /// canonical.
    pub fn git_dir(&self) -> &Path {
        &self.identity.git_dir
    }

    /// The common directory of [`Self::git_dir`] when the repository was
    /// opened, canonical: a linked worktree's main Git directory, whose refs
    /// and objects it uses, and the Git directory itself otherwise.
    pub fn common_dir(&self) -> &Path {
        &self.identity.common
    }

    /// Where this repository's commands run: at its root, confined to it.
    pub(crate) fn target(&self) -> GitTarget<'_> {
        GitTarget {
            directory: &self.path,
            ceiling: self.identity.ceiling.as_deref(),
        }
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    pub fn is_bare(&self) -> bool {
        self.bare
    }

    /// What Git discovery finds at this repository's path now, unconfined:
    /// Git looks for a repository in each folder above one that has none, so
    /// a work tree whose `.git` is gone (deleted, moved, a failed clone
    /// cleaned up) is found as the work tree enclosing it, if any. Every
    /// command of the repository itself is confined to it
    /// ([`GitTarget::confine`]) and fails there instead; this diagnosis words
    /// that failure. A passive read, which never fetches or writes; it
    /// starts one Git process, one per path where a path holds a line
    /// break.
    pub fn found_at_path(&self) -> Result<FoundAtPath> {
        if !self.path.is_dir() {
            return Ok(FoundAtPath::None);
        }
        let mut args = vec![
            "rev-parse",
            "--absolute-git-dir",
            "--path-format=absolute",
            "--git-common-dir",
        ];
        if !self.bare {
            args.push("--show-toplevel");
        }
        let output = run_git_output(GitTarget::unconfined(&self.path), &args)?;
        if !output.status.success() {
            let diagnostic = text(&output.stderr);
            ensure!(
                diagnostic.contains("not a git repository"),
                "Git rev-parse failed: {}",
                diagnostic.trim()
            );
            return Ok(FoundAtPath::None);
        }
        // The Git directory's line, its common directory's, and a work
        // tree's root. A path holding a line break splits into more lines:
        // each is then asked for alone, its whole output one path, since an
        // open confirms what it captured here and such a repository once
        // could not be opened at all.
        let mut lines: Vec<_> = trim_line(&output.stdout)
            .split(|byte| *byte == b'\n')
            .map(path_from_bytes)
            .collect();
        let alone: &[&[&str]] = &[
            &["rev-parse", "--absolute-git-dir"],
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            &["rev-parse", "--show-toplevel"],
        ];
        let alone = &alone[..if self.bare { 2 } else { 3 }];
        if lines.len() != alone.len() {
            lines = alone
                .iter()
                .map(|args| {
                    run_git(GitTarget::unconfined(&self.path), args)
                        .map(|output| path_from_bytes(trim_line(&output)))
                })
                .collect::<Result<_>>()?;
        }
        let (git_dir, common, found) = match (lines.as_slice(), self.bare) {
            ([git_dir, common, root], false) => (git_dir, common, root.clone()),
            ([git_dir, common], true) => (git_dir, common, git_dir.clone()),
            _ => return Ok(FoundAtPath::Other(path_from_bytes(&output.stdout))),
        };
        let same_root = found == self.path
            || found
                .canonicalize()
                .is_ok_and(|found| found == self.identity.root);
        if !same_root {
            return Ok(FoundAtPath::Other(found));
        }
        if !git_dir
            .canonicalize()
            .is_ok_and(|git_dir| git_dir == self.identity.git_dir)
        {
            return Ok(FoundAtPath::Other(git_dir.clone()));
        }
        // A linked worktree's `commondir` re-pointed: another repository's
        // refs and objects.
        Ok(
            if common
                .canonicalize()
                .is_ok_and(|common| common == self.identity.common)
            {
                FoundAtPath::Same
            } else {
                FoundAtPath::Other(common.clone())
            },
        )
    }

    /// Whether this repository is still the one at its path, as Git would
    /// find it there. Where every command is confined and the `.git` entry
    /// and root still match what was captured at open, it answers from the
    /// filesystem alone, starting no process; otherwise (an entry changed,
    /// or a repository whose commands cannot be confined) it asks Git
    /// ([`Self::found_at_path`]).
    ///
    /// A configuration edit also asks Git, once (`core.worktree` re-pointed
    /// after open moves the work tree with no `.git` entry changed): the
    /// repository the same, the edited files are confirmed again.
    pub fn verify_at_path(&self) -> Result<FoundAtPath> {
        if !(self.identity.confined && self.identity.holds(&self.path, self.bare)) {
            return self.found_at_path();
        }
        let stamps = self.identity.config_stamps();
        let mut confirmed = self
            .identity
            .stamps
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *confirmed == stamps {
            return Ok(FoundAtPath::Same);
        }
        let found = self.found_at_path()?;
        if found == FoundAtPath::Same {
            *confirmed = stamps;
        }
        Ok(found)
    }

    /// Refuses, before Git starts, a write whose repository is no longer
    /// the one at its path ([`Self::verify_at_path`]): run there, it would
    /// act on the repository enclosing that path, or fail part way.
    fn ensure_found_at_path(&self) -> Result<()> {
        match self.verify_at_path()? {
            FoundAtPath::Same => Ok(()),
            // Its own `.git` gone, or its work tree redirected elsewhere.
            FoundAtPath::Other(other) => bail!(
                "The repository changed: at {} Git now finds the repository at {}. Git was not started.",
                self.path.display(),
                other.display()
            ),
            FoundAtPath::None => bail!(
                "The repository at {} is gone: Git finds no repository there. Git was not started.",
                self.path.display()
            ),
        }
    }

    /// A failed command's error, worded as the repository gone where Git
    /// stopped at its confinement: "not a git repository" from a confined
    /// command means the repository's `.git` went away while it ran or
    /// just before, so the diagnosis ([`Self::found_at_path`]) says what Git
    /// finds there now. The command's own error stays beneath it, uncertain
    /// outcome included. Any other failure is returned as it is, with no
    /// process started.
    pub fn word_gone(&self, error: anyhow::Error) -> anyhow::Error {
        if !finds_no_repository(&error) {
            return error;
        }
        match self.found_at_path() {
            Ok(FoundAtPath::Other(other)) => error.context(format!(
                "The repository changed: at {} Git now finds the repository at {}",
                self.path.display(),
                other.display()
            )),
            Ok(FoundAtPath::None) => error.context(format!(
                "The repository at {} is gone: Git finds no repository there",
                self.path.display()
            )),
            Ok(FoundAtPath::Same) | Err(_) => error,
        }
    }

    /// Resolve actual administration directories for local notifications.
    /// Linked worktrees have a private HEAD/index but share refs and objects.
    /// These passive reads never create or update Git metadata.
    pub fn git_directories(&self) -> Result<(PathBuf, PathBuf)> {
        let private = path_from_bytes(trim_line(&run_git(
            self.target(),
            &["rev-parse", "--absolute-git-dir"],
        )?));
        let common = path_from_bytes(trim_line(&run_git(
            self.target(),
            &["rev-parse", "--git-common-dir"],
        )?));
        let absolute = |path: PathBuf| -> Result<PathBuf> {
            let path = if path.is_absolute() {
                path
            } else {
                self.path.join(path)
            };
            path.canonicalize()
                .context("Resolve Git administration directory")
        };
        Ok((absolute(private)?, absolute(common)?))
    }

    /// Whether the repository is shallow, its history cut at grafted
    /// boundaries that a fetch can deepen without moving any reference.
    pub fn is_shallow(&self) -> Result<bool> {
        let bytes = run_git(self.target(), &["rev-parse", "--is-shallow-repository"])?;
        Ok(trim_line(&bytes) == b"true")
    }

    pub fn branches(&self) -> Result<Vec<Branch>> {
        let bytes = run_git(
            self.target(),
            &[
                "for-each-ref",
                "--sort=-committerdate",
                "--format=%(refname)%00%(objectname)%00%(HEAD)%00%(symref)",
                "refs/heads/",
                "refs/remotes/",
            ],
        )?;
        let mut branches = Vec::new();
        for record in bytes.split(|b| *b == b'\n').filter(|r| !r.is_empty()) {
            let fields: Vec<_> = record.split(|b| *b == 0).collect();
            ensure!(fields.len() == 4, "Malformed branch record from Git");
            if !fields[3].is_empty() {
                continue;
            } // remote HEAD is an alias, not a branch
            let refname = text(fields[0]);
            let (remote, name) = if let Some(name) = refname.strip_prefix("refs/heads/") {
                (false, name)
            } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
                (true, name)
            } else {
                continue;
            };
            branches.push(Branch {
                name: name.into(),
                oid: text(fields[1]),
                remote,
                current: fields[2] == b"*",
            });
        }
        Ok(branches)
    }

    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        let bytes = run_git(self.target(), &["worktree", "list", "--porcelain", "-z"])?;
        let mut trees = Vec::new();
        let mut current: Option<Worktree> = None;
        for line in bytes.split(|b| *b == 0) {
            if let Some(path) = line.strip_prefix(b"worktree ") {
                if let Some(tree) = current.take() {
                    trees.push(tree);
                }
                current = Some(Worktree {
                    path: path_from_bytes(path),
                    branch: None,
                    oid: String::new(),
                    detached: false,
                    locked: false,
                    prunable: false,
                });
            } else if let Some(tree) = current.as_mut() {
                if let Some(oid) = line.strip_prefix(b"HEAD ") {
                    tree.oid = text(oid);
                } else if let Some(branch) = line.strip_prefix(b"branch refs/heads/") {
                    tree.branch = Some(text(branch));
                } else if line == b"detached" {
                    tree.detached = true;
                } else if line == b"locked" || line.starts_with(b"locked ") {
                    tree.locked = true;
                } else if line == b"prunable" || line.starts_with(b"prunable ") {
                    tree.prunable = true;
                }
            }
        }
        if let Some(tree) = current {
            trees.push(tree);
        }
        Ok(trees)
    }

    pub fn history(&self, limit: usize) -> Result<Vec<Commit>> {
        self.history_impl(None, 0, limit)
    }

    /// Paging across moving refs may shift rows; refresh/reset paging when refs
    /// change. `history_from_page` uses a fixed commit anchor for stable paging.
    pub fn history_page(&self, offset: usize, limit: usize) -> Result<Vec<Commit>> {
        self.history_impl(None, offset, limit)
    }

    /// A full object ID from a branch/worktree/commit, never an untrusted revision
    /// expression. Keeps range parsing and option injection out of the API.
    pub fn history_from(&self, oid: &str, limit: usize) -> Result<Vec<Commit>> {
        validate_oid(oid)?;
        self.history_impl(Some(oid), 0, limit)
    }

    pub fn history_from_page(&self, oid: &str, offset: usize, limit: usize) -> Result<Vec<Commit>> {
        validate_oid(oid)?;
        self.history_impl(Some(oid), offset, limit)
    }

    fn history_impl(&self, oid: Option<&str>, offset: usize, limit: usize) -> Result<Vec<Commit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let count = format!("--max-count={limit}");
        let skip = format!("--skip={offset}");
        let bytes = run_git(
            self.target(),
            &[
                "log",
                "--topo-order",
                "--no-show-signature",
                "--no-decorate",
                "--encoding=UTF-8",
                "-z",
                "--format=%H%x00%P%x00%an%x00%at%x00%s%x00%b",
                &count,
                &skip,
                oid.unwrap_or("--all"),
                "--",
            ],
        )?;
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        let fields: Vec<_> = bytes
            .strip_suffix(&[0])
            .unwrap_or(&bytes)
            .split(|b| *b == 0)
            .collect();
        ensure!(fields.len() % 6 == 0, "Malformed commit metadata from Git");
        fields
            .as_chunks::<6>()
            .0
            .iter()
            .map(|f| {
                Ok(Commit {
                    oid: text(f[0]),
                    parents: text(f[1]).split_whitespace().map(str::to_owned).collect(),
                    author: text(f[2]),
                    timestamp: std::str::from_utf8(f[3])?
                        .parse()
                        .context("Invalid commit timestamp")?,
                    subject: text(f[4]),
                    body: text(f[5]).trim_end_matches('\n').to_owned(),
                })
            })
            .collect()
    }

    /// Changed files compared with one selected parent. For root commits the
    /// baseline is the empty tree. Rename detection is intentionally optional.
    pub fn changes(&self, commit_oid: &str, parent_index: usize) -> Result<Vec<FileChange>> {
        self.changes_impl(commit_oid, parent_index, false)
    }

    /// Run bounded rename detection only when requested, off the initial click.
    pub fn changes_with_renames(
        &self,
        commit_oid: &str,
        parent_index: usize,
    ) -> Result<Vec<FileChange>> {
        self.changes_impl(commit_oid, parent_index, true)
    }

    fn changes_impl(
        &self,
        oid: &str,
        parent_index: usize,
        renames: bool,
    ) -> Result<Vec<FileChange>> {
        validate_oid(oid)?;
        let commit = self.read_object(oid, "commit")?;
        let mut parents = Vec::new();
        for line in commit
            .split(|b| *b == b'\n')
            .take_while(|line| !line.is_empty())
        {
            if let Some(parent) = line.strip_prefix(b"parent ") {
                let parent = std::str::from_utf8(parent).context("Invalid commit parent")?;
                validate_oid(parent)?;
                parents.push(parent);
            }
        }
        ensure!(
            (parents.is_empty() && parent_index == 0) || parent_index < parents.len(),
            "Commit has no parent at index {parent_index}"
        );
        let mut args = vec![
            "diff-tree",
            "--no-commit-id",
            "--raw",
            "--no-abbrev",
            "-z",
            "-r",
            "--no-ext-diff",
            "--no-textconv",
            if renames {
                "--find-renames=50%"
            } else {
                "--no-renames"
            },
            "-l1000",
        ];
        if parents.is_empty() {
            args.push("--root");
            args.push(oid);
        } else {
            args.push(parents[parent_index]);
            args.push(oid);
        }
        args.push("--");
        parse_changes(&run_git(self.target(), &args)?)
    }

    pub fn blob(&self, oid: &str) -> Result<Vec<u8>> {
        self.read_object(oid, "blob")
    }

    fn read_object(&self, oid: &str, kind: &str) -> Result<Vec<u8>> {
        validate_oid(oid)?;
        let mut guard = self
            .batch
            .lock()
            .map_err(|_| anyhow::anyhow!("Object reader lock was poisoned"))?;
        if guard.is_none() {
            *guard = Some(BatchReader::spawn(self.target())?);
        }
        let result = guard
            .as_mut()
            .expect("initialized above")
            .read_object(oid, kind);
        // A failed/oversized read might leave unread protocol bytes. Discard the
        // process so the next request always starts at a known frame boundary.
        if result.is_err() {
            *guard = None;
        }
        result
    }

    /// Read the encoded object size without inflating or transferring its body.
    pub fn blob_size(&self, oid: &str) -> Result<usize> {
        validate_oid(oid)?;
        let mut guard = self
            .batch
            .lock()
            .map_err(|_| anyhow::anyhow!("Object reader lock was poisoned"))?;
        if guard.is_none() {
            *guard = Some(BatchReader::spawn(self.target())?);
        }
        let result = guard
            .as_mut()
            .expect("initialized above")
            .request_header("info", oid, "blob");
        if result.is_err() {
            *guard = None;
        }
        result
    }

    pub fn text_preview(&self, file: &FileChange) -> Result<TextPreview> {
        Ok(self.text_preview_with_sources(file)?.preview)
    }

    pub fn text_preview_with_sources(&self, file: &FileChange) -> Result<TextPreviewWithSources> {
        let only_submodule_sides = file.is_submodule()
            && (file.old_oid.is_none() || file.old_mode == "160000")
            && (file.new_oid.is_none() || file.new_mode == "160000");
        if only_submodule_sides {
            return Ok(TextPreviewWithSources {
                preview: TextPreview::Submodule {
                    old_oid: file.old_oid.clone(),
                    new_oid: file.new_oid.clone(),
                },
                old: Vec::new(),
                new: Vec::new(),
            });
        }
        let old_bytes = file
            .old_oid
            .as_deref()
            .map(|oid| self.preview_side_size(oid, &file.old_mode))
            .transpose()?
            .unwrap_or_default();
        let new_bytes = file
            .new_oid
            .as_deref()
            .map(|oid| self.preview_side_size(oid, &file.new_mode))
            .transpose()?
            .unwrap_or_default();
        if old_bytes > MAX_DIFF_BYTES || new_bytes > MAX_DIFF_BYTES {
            return Ok(TextPreviewWithSources {
                preview: TextPreview::TooLarge {
                    old_bytes,
                    new_bytes,
                },
                old: Vec::new(),
                new: Vec::new(),
            });
        }
        let old = file
            .old_oid
            .as_deref()
            .map(|oid| self.preview_side_bytes(oid, &file.old_mode))
            .transpose()?
            .unwrap_or_default();
        let new = file
            .new_oid
            .as_deref()
            .map(|oid| self.preview_side_bytes(oid, &file.new_mode))
            .transpose()?
            .unwrap_or_default();
        if old.contains(&0) || new.contains(&0) {
            return Ok(TextPreviewWithSources {
                preview: TextPreview::Binary,
                old,
                new,
            });
        }
        let (Ok(old_text), Ok(new_text)) = (std::str::from_utf8(&old), std::str::from_utf8(&new))
        else {
            return Ok(TextPreviewWithSources {
                preview: TextPreview::Binary,
                old,
                new,
            });
        };
        if old.iter().filter(|b| **b == b'\n').count() > MAX_DIFF_LINES
            || new.iter().filter(|b| **b == b'\n').count() > MAX_DIFF_LINES
        {
            return Ok(TextPreviewWithSources {
                preview: TextPreview::TooLarge {
                    old_bytes: old.len(),
                    new_bytes: new.len(),
                },
                old: Vec::new(),
                new: Vec::new(),
            });
        }
        let old_label = file
            .old_path
            .as_ref()
            .map(|p| patch_label("a", p))
            .unwrap_or_else(|| "/dev/null".into());
        let new_label = file
            .new_path
            .as_ref()
            .map(|p| patch_label("b", p))
            .unwrap_or_else(|| "/dev/null".into());
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Patience)
            .timeout(Duration::from_millis(250))
            .diff_lines(old_text, new_text);
        let mut patch = diff
            .unified_diff()
            .context_radius(3)
            .header(&old_label, &new_label)
            .to_string();
        if file.old_mode != file.new_mode {
            patch = if file.old_mode == "000000" {
                format!("new file mode {}\n{patch}", file.new_mode)
            } else if file.new_mode == "000000" {
                format!("deleted file mode {}\n{patch}", file.old_mode)
            } else {
                format!(
                    "old mode {}\nnew mode {}\n{patch}",
                    file.old_mode, file.new_mode
                )
            };
        }
        if patch.is_empty() {
            patch = "File contents are identical.\n".into();
        }
        Ok(TextPreviewWithSources {
            preview: TextPreview::Patch(patch),
            old,
            new,
        })
    }

    fn preview_side_size(&self, oid: &str, mode: &str) -> Result<usize> {
        if mode == "160000" {
            Ok(format!("Subproject commit {oid}\n").len())
        } else {
            self.blob_size(oid)
        }
    }

    fn preview_side_bytes(&self, oid: &str, mode: &str) -> Result<Vec<u8>> {
        if mode == "160000" {
            Ok(format!("Subproject commit {oid}\n").into_bytes())
        } else {
            self.blob(oid)
        }
    }

    /// Resolve only a verified object already present in the local LFS store.
    /// No Git LFS executable, smudge/extension helper, or network is invoked.
    /// Missing objects return `None`; corrupt/oversized/unsafe objects are errors.
    /// A local LFS store is mutable, so callers should not cache misses forever.
    pub fn local_lfs_object(
        &self,
        oid: &str,
        expected_size: u64,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>> {
        ensure!(
            oid.len() == 64 && oid.bytes().all(|b| b.is_ascii_hexdigit()),
            "Expected an LFS SHA-256 object ID"
        );
        let limit = max_bytes.min(MAX_BLOB_BYTES);
        ensure!(
            expected_size <= limit as u64,
            "LFS object is {expected_size} bytes; preview limit is {limit} bytes"
        );
        let oid = oid.to_ascii_lowercase();
        if expected_size == 0 && oid == format!("{:x}", Sha256::digest([])) {
            return Ok(Some(Vec::new()));
        }
        let storage = self.preview_lfs_storage()?;
        let Some(mut file) = open_local_lfs(&storage, &oid)? else {
            return Ok(None);
        };
        let metadata = file
            .metadata()
            .context("Unable to inspect local LFS object")?;
        ensure!(metadata.is_file(), "Local LFS object is not a regular file");
        ensure!(
            metadata.len() == expected_size,
            "Local LFS object size mismatch: expected {expected_size}, found {}",
            metadata.len()
        );
        let mut bytes = Vec::with_capacity(expected_size as usize);
        (&mut file)
            .take(expected_size.saturating_add(1))
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == expected_size,
            "Local LFS object changed while being read"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == oid,
            "Local LFS object failed SHA-256 verification"
        );
        Ok(Some(bytes))
    }

    fn preview_lfs_storage(&self) -> Result<PathBuf> {
        let common = run_git(
            self.target(),
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let common = path_from_bytes(trim_line(&common));
        let configured = work::lfs_storage_config(self.target())?;
        let storage = if configured.status.success() {
            let value = configured
                .stdout
                .strip_suffix(&[0])
                .unwrap_or(&configured.stdout);
            if value.is_empty() {
                common.join("lfs")
            } else {
                let path = path_from_bytes(value);
                if path.is_absolute() {
                    path
                } else {
                    common.join(path)
                }
            }
        } else if configured.status.code() == Some(1) {
            common.join("lfs")
        } else {
            bail!(
                "Unable to read local LFS storage configuration: {}",
                text(&configured.stderr).trim()
            );
        };
        Ok(storage)
    }

    pub fn diff(&self, file: &FileChange) -> Result<String> {
        Ok(match self.text_preview(file)? {
            TextPreview::Patch(patch) => patch,
            TextPreview::Binary => "Binary or non-UTF-8 file.\n".into(),
            TextPreview::TooLarge {
                old_bytes,
                new_bytes,
            } => format!(
                "Text preview is limited to 2 MiB and 100,000 lines per side.\nBefore: {}\nAfter: {}\n",
                counted(old_bytes, "byte", "bytes"),
                counted(new_bytes, "byte", "bytes")
            ),
            TextPreview::Submodule { old_oid, new_oid } => format!(
                "Submodule commit\n- {}\n+ {}\n",
                old_oid.as_deref().unwrap_or("(absent)"),
                new_oid.as_deref().unwrap_or("(absent)")
            ),
        })
    }
}

fn parse_changes(bytes: &[u8]) -> Result<Vec<FileChange>> {
    let mut fields = bytes.split(|b| *b == 0);
    let mut changes = Vec::new();
    while let Some(header) = fields.next() {
        if header.is_empty() {
            continue;
        }
        ensure!(header.starts_with(b":"), "Malformed raw diff header");
        let parts: Vec<_> = header[1..].split(|b| *b == b' ').collect();
        ensure!(parts.len() == 5, "Malformed raw diff fields");
        let status = match parts[4].first() {
            Some(b'A') => ChangeStatus::Added,
            Some(b'M') => ChangeStatus::Modified,
            Some(b'D') => ChangeStatus::Deleted,
            Some(b'R') => ChangeStatus::Renamed,
            Some(b'T') => ChangeStatus::TypeChanged,
            _ => bail!("Unsupported tree change status: {}", text(parts[4])),
        };
        let first_path = path_from_bytes(fields.next().context("Missing path in raw diff")?);
        let (old_path, new_path) = match status {
            ChangeStatus::Added => (None, Some(first_path)),
            ChangeStatus::Deleted => (Some(first_path), None),
            ChangeStatus::Renamed => (
                Some(first_path),
                Some(path_from_bytes(
                    fields.next().context("Missing renamed path")?,
                )),
            ),
            _ => (Some(first_path.clone()), Some(first_path)),
        };
        let object_id = |id: &[u8]| {
            if id.iter().all(|b| *b == b'0') {
                None
            } else {
                Some(text(id))
            }
        };
        changes.push(FileChange {
            old_path,
            new_path,
            old_oid: object_id(parts[2]),
            new_oid: object_id(parts[3]),
            status,
            old_mode: text(parts[0]),
            new_mode: text(parts[1]),
        });
    }
    Ok(changes)
}

fn git_command(target: GitTarget<'_>) -> Command {
    let mut cmd = Command::new("git");
    target.confine(&mut cmd);
    isolate_process_group(&mut cmd);
    cmd.arg("--no-pager")
        .arg("--no-optional-locks")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "-c",
            "protocol.allow=never",
            "-c",
            "credential.helper=",
        ])
        .arg("-C")
        .arg(target.directory)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_EXTERNAL_DIFF",
        "GIT_DIFF_OPTS",
        "GIT_NAMESPACE",
        "GIT_SHALLOW_FILE",
        "GIT_REPLACE_REF_BASE",
    ] {
        cmd.env_remove(name);
    }
    cmd
}

fn run_git(target: GitTarget<'_>, args: &[&str]) -> Result<Vec<u8>> {
    let output = run_git_output(target, args)?;
    ensure!(
        output.status.success(),
        "Git {} failed: {}",
        args.first().unwrap_or(&"command"),
        text(&output.stderr).trim()
    );
    Ok(output.stdout)
}

fn run_git_output(target: GitTarget<'_>, args: &[&str]) -> Result<Output> {
    let mut command = git_command(target);
    command.args(args);
    bounded_output(command, GIT_TIMEOUT)
}

fn bounded_output(mut command: Command, timeout: Duration) -> Result<Output> {
    ensure!(
        !work::inspection_cancelled(),
        "Repository inspection cancelled"
    );
    isolate_process_group(&mut command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Unable to start Git; install Git and ensure it is on PATH")?;
    let pipes = ChildPipes::take(&mut child)?;
    let read = |pipe: Box<dyn Read + Send>, limit: usize| {
        thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            pipe.take(limit as u64 + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        })
    };
    let stdout = read(Box::new(pipes.output), MAX_COMMAND_OUTPUT);
    let stderr = read(
        Box::new(pipes.error.expect("Requested Git error pipe")),
        128 * 1024,
    );
    let start = Instant::now();
    let mut status = None;
    let result = loop {
        if work::inspection_cancelled() {
            break Err(anyhow::anyhow!("Repository inspection cancelled"));
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(error) => break Err(error.into()),
            }
        }
        if let Some(status) = status
            && stdout.is_finished()
            && stderr.is_finished()
        {
            break Ok(status);
        }
        if start.elapsed() >= timeout {
            break Err(anyhow::anyhow!(
                "Git read exceeded its {} second time limit",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(1));
    };
    if result.is_err() {
        pipes.control.stop();
        terminate_process_group(&child);
        let _ = child.kill();
        let _ = child.wait();
    }
    // Join both before propagating an error from either pipe, and retain the
    // deadline/cancellation error instead of the resulting stopped-pipe error.
    let stdout = stdout.join();
    let stderr = stderr.join();
    let status = result?;
    let stdout = stdout.map_err(|_| anyhow::anyhow!("Git output reader stopped"))??;
    let stderr = stderr.map_err(|_| anyhow::anyhow!("Git error reader stopped"))??;
    ensure!(
        stdout.len() <= MAX_COMMAND_OUTPUT && stderr.len() <= 128 * 1024,
        "Git command output exceeded its memory limit"
    );
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

struct BatchReader {
    child: Child,
    requests: mpsc::Sender<BatchRequest>,
    worker: Option<JoinHandle<()>>,
    timeout: Duration,
    pipes: PipeControl,
}

enum BatchRequest {
    Read {
        oid: String,
        kind: String,
        contents: bool,
        response: mpsc::Sender<Result<BatchValue>>,
    },
    Stop,
}

enum BatchValue {
    Body(Vec<u8>),
    Size(usize),
}

struct BatchWire {
    input: StopPipe<ChildStdin>,
    output: BufReader<StopPipe<ChildStdout>>,
}

impl BatchReader {
    fn spawn(target: GitTarget<'_>) -> Result<Self> {
        let mut command = git_command(target);
        command.args(["cat-file", "--batch-command"]);
        Self::spawn_command(command, GIT_TIMEOUT)
    }

    fn spawn_command(mut command: Command, timeout: Duration) -> Result<Self> {
        isolate_process_group(&mut command);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Unable to start Git object reader")?;
        let pipes = ChildPipes::take(&mut child)?;
        let input = pipes.input.expect("Requested Git object input pipe");
        let output = BufReader::new(pipes.output);
        let (requests, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut wire = BatchWire { input, output };
            while let Ok(BatchRequest::Read {
                oid,
                kind,
                contents,
                response,
            }) = receiver.recv()
            {
                let result = if contents {
                    wire.read_object(&oid, &kind).map(BatchValue::Body)
                } else {
                    wire.request_header("info", &oid, &kind)
                        .map(BatchValue::Size)
                };
                let failed = result.is_err();
                let _ = response.send(result);
                if failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            requests,
            worker: Some(worker),
            timeout,
            pipes: pipes.control,
        })
    }

    fn request(&self, oid: &str, kind: &str, contents: bool) -> Result<BatchValue> {
        let (response, result) = mpsc::channel();
        self.requests
            .send(BatchRequest::Read {
                oid: oid.into(),
                kind: kind.into(),
                contents,
                response,
            })
            .context("Git object reader disconnected")?;
        result
            .recv_timeout(self.timeout)
            .context("Git object read exceeded its time limit or disconnected")?
    }

    fn read_object(&self, oid: &str, kind: &str) -> Result<Vec<u8>> {
        match self.request(oid, kind, true)? {
            BatchValue::Body(bytes) => Ok(bytes),
            _ => bail!("Unexpected object response"),
        }
    }

    fn request_header(&self, _request: &str, oid: &str, kind: &str) -> Result<usize> {
        match self.request(oid, kind, false)? {
            BatchValue::Size(size) => Ok(size),
            _ => bail!("Unexpected object size response"),
        }
    }
}

impl BatchWire {
    fn read_object(&mut self, oid: &str, kind: &str) -> Result<Vec<u8>> {
        let size = self.request_header("contents", oid, kind)?;
        ensure!(
            size <= MAX_BLOB_BYTES,
            "Object is {size} bytes; preview limit is {MAX_BLOB_BYTES} bytes"
        );
        let mut bytes = vec![0; size];
        self.output
            .read_exact(&mut bytes)
            .context("Truncated Git object")?;
        let mut delimiter = [0];
        self.output.read_exact(&mut delimiter)?;
        ensure!(delimiter == *b"\n", "Invalid Git object delimiter");
        Ok(bytes)
    }

    fn request_header(&mut self, request: &str, oid: &str, kind: &str) -> Result<usize> {
        writeln!(self.input, "{request} {oid}").context("Git object reader disconnected")?;
        self.input.flush()?;
        let mut header = String::new();
        let received = self.output.read_line(&mut header)?;
        // Older Git releases exit for a missing promisor object when lazy
        // fetching is disabled instead of returning a batch `missing` record.
        // EOF can also mean another reader failure, so retain that uncertainty.
        ensure!(
            received != 0,
            "Git object reader ended before responding for {oid}. The object is either not available locally or the reader failed (automatic fetching is disabled)."
        );
        let fields: Vec<_> = header.split_whitespace().collect();
        ensure!(
            fields.len() != 2 || fields[1] != "missing",
            "Object {oid} is not available locally (automatic fetching is disabled)"
        );
        ensure!(
            fields.len() == 3 && fields[0].eq_ignore_ascii_case(oid),
            "Invalid object response for {oid}: {}",
            header.trim()
        );
        ensure!(
            fields[1] == kind,
            "Object {oid} is a {}, not a {}",
            fields[1],
            if kind == "blob" { "file blob" } else { kind }
        );
        fields[2].parse().context("Invalid Git object size")
    }
}

impl Drop for BatchReader {
    fn drop(&mut self) {
        self.pipes.stop();
        let _ = self.requests.send(BatchRequest::Stop);
        terminate_process_group(&self.child);
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(unix)]
fn isolate_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(unix)]
fn terminate_process_group(child: &Child) {
    use rustix::process::{Pid, Signal, kill_process_group};
    if let Some(pid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process_group(pid, Signal::KILL);
    }
}

#[cfg(not(unix))]
fn isolate_process_group(_command: &mut Command) {}

#[cfg(not(unix))]
fn terminate_process_group(_child: &Child) {}

#[cfg(unix)]
fn open_local_lfs(storage: &Path, oid: &str) -> Result<Option<std::fs::File>> {
    use rustix::{
        fs::{Mode, OFlags, open, openat},
        io::Errno,
    };
    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let missing = |error: Errno| error == Errno::NOENT;
    let mut directory = match open(storage, directory_flags, Mode::empty()) {
        Ok(fd) => fd,
        Err(error) if missing(error) => return Ok(None),
        Err(error) => return Err(error).context("Unable to open local LFS storage safely"),
    };
    // Directory-relative opens prevent a concurrent symlink replacement from
    // redirecting a later component outside the chosen object store.
    for component in ["objects", &oid[..2], &oid[2..4]] {
        directory = match openat(&directory, component, directory_flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(error) if missing(error) => return Ok(None),
            Err(error) => return Err(error).context("Unsafe or unreadable local LFS directory"),
        };
    }
    let file = match openat(
        &directory,
        oid,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(error) if missing(error) => return Ok(None),
        Err(error) => return Err(error).context("Unsafe or unreadable local LFS object"),
    };
    Ok(Some(std::fs::File::from(file)))
}

#[cfg(not(unix))]
fn open_local_lfs(_storage: &Path, _oid: &str) -> Result<Option<std::fs::File>> {
    bail!("Safe local LFS resolution is currently supported on macOS and Linux")
}

fn validate_oid(oid: &str) -> Result<()> {
    ensure!(
        (oid.len() == 40 || oid.len() == 64) && oid.bytes().all(|b| b.is_ascii_hexdigit()),
        "Expected a full Git object ID"
    );
    Ok(())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `count` with the noun that agrees with it ("1 byte", "2 bytes"), as the
/// app's `counted` builds every count it shows; this crate cannot reach the
/// app's, so every count core words is built here.
pub(crate) fn counted<T>(count: T, one: &str, many: &str) -> String
where
    T: std::fmt::Display + PartialEq + From<u8>,
{
    format!("{count} {}", if count == T::from(1) { one } else { many })
}

// Remove exactly Git's line terminator, never whitespace in a directory name.
fn trim_line(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(text(bytes))
}

fn patch_label(prefix: &str, path: &Path) -> String {
    let label = format!("{prefix}/{}", path.to_string_lossy());
    if label.chars().any(char::is_control) {
        format!("{label:?}")
    } else {
        label
    }
}

#[cfg(unix)]
fn null_device() -> &'static OsStr {
    OsStr::new("/dev/null")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs Git in `folder` with an empty system and global configuration
    /// and a fixed identity.
    fn git(folder: &Path, args: &[&str]) {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(folder)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "GitTurtle Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "GitTurtle Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid");
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(name);
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{}", text(&output.stderr));
    }

    /// Opens `folder` with `change` run between the capture and Git's
    /// confirmation.
    fn open_changed_part_way(
        folder: &Path,
        change: impl FnMut() + 'static,
    ) -> Result<GitRepository> {
        OPEN_CHECKPOINT.with_borrow_mut(|hook| *hook = Some(Box::new(change)));
        let opened = GitRepository::open(folder);
        OPEN_CHECKPOINT.with_borrow_mut(|hook| *hook = None);
        opened
    }

    /// An open stamps the files that can move the work tree or the common
    /// directory after Git reported the root and `commondir` was read, then
    /// asks Git again: a linked worktree's `commondir` re-pointed at another
    /// repository, or `core.worktree` re-pointed, between the two is refused
    /// with what Git finds. It once returned a handle bound to what it had
    /// read, whose stamps verified the changed files as its own, so writes
    /// went to the other repository or work tree.
    #[test]
    fn an_open_refuses_what_changed_while_it_was_captured() {
        let fixture = tempfile::tempdir().unwrap();
        let main = fixture.path().join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "--quiet"]);
        git(&main, &["commit", "--quiet", "--allow-empty", "-m", "main"]);
        let linked = fixture.path().join("linked");
        git(
            &main,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        );
        let other = fixture.path().join("other");
        std::fs::create_dir(&other).unwrap();
        git(&other, &["init", "--quiet"]);
        let other_git = other.join(".git").canonicalize().unwrap();
        let commondir = GitRepository::open(&linked)
            .unwrap()
            .git_dir()
            .join("commondir");
        let pointed = other_git.clone();
        let changed = commondir.clone();
        let refused = open_changed_part_way(&linked, move || {
            std::fs::write(&changed, format!("{}\n", pointed.display())).unwrap();
        })
        .unwrap_err()
        .downcast::<ChangedWhileOpening>()
        .expect("refused as changed");
        assert_eq!(refused.found.as_deref(), Some(other_git.as_path()));

        let elsewhere = fixture.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let elsewhere = elsewhere.canonicalize().unwrap();
        let (folder, moved) = (main.clone(), elsewhere.clone());
        let refused = open_changed_part_way(&main, move || {
            git(
                &folder,
                &["config", "core.worktree", moved.to_str().unwrap()],
            );
        })
        .unwrap_err()
        .downcast::<ChangedWhileOpening>()
        .expect("refused as changed");
        assert_eq!(refused.found.as_deref(), Some(elsewhere.as_path()));
    }

    /// The confinement appends to the person's own ceilings, never
    /// replacing them.
    #[test]
    fn the_ceiling_follows_the_person_s_own_ceilings() {
        let ceiling = Path::new("/projects");
        assert_eq!(ceiling_directories(None, ceiling), "/projects");
        assert_eq!(ceiling_directories(Some("".into()), ceiling), "/projects");
        assert_eq!(
            ceiling_directories(Some(format!("/home{LIST_SEPARATOR}/srv").into()), ceiling),
            format!("/home{LIST_SEPARATOR}/srv{LIST_SEPARATOR}/projects").as_str()
        );
    }

    /// Where the parent of the folder holding `.git` holds the list
    /// separator, `GIT_CEILING_DIRECTORIES` would split it and confine
    /// nothing: no ceiling is set there, and the repository is unconfined,
    /// so its writes and Changes' read ask Git first.
    #[test]
    fn a_ceiling_holding_the_list_separator_is_never_set() {
        let temp = tempfile::TempDir::new().unwrap();
        let named = temp.path().join(format!("a{LIST_SEPARATOR}b"));
        let root = named.join("repository");
        std::fs::create_dir_all(&named).unwrap();
        GitRepository::init(&root, "main").unwrap();
        let repo = GitRepository::open(&root).unwrap();
        assert_eq!(repo.identity.ceiling, None);
        assert!(!repo.identity.confined);
        let command = git_command(repo.target());
        assert!(
            !command
                .get_envs()
                .any(|(name, _)| name == "GIT_CEILING_DIRECTORIES")
        );

        let root = temp.path().join("plain").join("repository");
        std::fs::create_dir_all(temp.path().join("plain")).unwrap();
        GitRepository::init(&root, "main").unwrap();
        let repo = GitRepository::open(&root).unwrap();
        let parent = temp.path().join("plain").canonicalize().unwrap();
        assert_eq!(repo.identity.ceiling.as_deref(), Some(parent.as_path()));
        assert!(repo.identity.confined);
        let command = work::normal_command(repo.target());
        let set = command
            .get_envs()
            .find(|(name, _)| *name == "GIT_CEILING_DIRECTORIES")
            .and_then(|(_, value)| value.map(OsStr::to_owned));
        assert_eq!(
            set,
            Some(ceiling_directories(
                std::env::var_os("GIT_CEILING_DIRECTORIES"),
                &parent
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn passive_deadline_includes_pipe_holders_after_parent_exit() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 0.3 & exit 0"]);
        let started = Instant::now();
        let error = bounded_output(command, Duration::from_millis(30)).unwrap_err();
        assert!(error.to_string().contains("time limit"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn passive_deadline_joins_readers_with_detached_pipe_holders() {
        let (fixture, command) = process_io::fixtures::DetachedPipeHolder::command(false);
        let started = Instant::now();
        let error = bounded_output(command, Duration::from_millis(300)).unwrap_err();
        fixture.wait_ready();
        assert!(error.to_string().contains("time limit"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn object_timeout_joins_its_reader_with_detached_pipe_holders() {
        let (fixture, command) = process_io::fixtures::DetachedPipeHolder::command(false);
        let reader = BatchReader::spawn_command(command, Duration::from_millis(30)).unwrap();
        fixture.wait_ready();
        let started = Instant::now();
        let error = reader.read_object(&"0".repeat(40), "blob").unwrap_err();
        assert!(error.to_string().contains("time limit"));
        drop(reader);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn command_deadline_terminates_a_child_and_its_pipe_holding_descendant() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 10 & wait"]);
        let started = Instant::now();
        let error = bounded_output(command, Duration::from_millis(30)).unwrap_err();
        assert!(error.to_string().contains("time limit"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn stalled_object_response_times_out_and_reaps_its_reader() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "read line; sleep 10"]);
        let started = Instant::now();
        let reader = BatchReader::spawn_command(command, Duration::from_millis(30)).unwrap();
        let error = reader.read_object(&"0".repeat(40), "blob").unwrap_err();
        assert!(error.to_string().contains("time limit"));
        drop(reader);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

#[cfg(not(unix))]
fn null_device() -> &'static OsStr {
    OsStr::new("NUL")
}
