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
    /// What Git finds there now, if anything: [`Self::path`] itself where
    /// Git still reports the same repository, only the capture having
    /// failed.
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
            Some(found) if *found == self.path => write!(f, "."),
            Some(found) => write!(f, ": Git now finds {} there.", found.display()),
            None => write!(f, ": Git now finds no repository there."),
        }?;
        write!(f, " Open it again.")
    }
}

impl std::error::Error for ChangedWhileOpening {}

/// A repository [`GitRepository::open`] refused as it stands, nothing having
/// changed: Git found it in one folder and reports a work tree outside that
/// folder (`core.worktree`), which no `.git` entry leads back from, so no
/// command could be confined to it.
#[derive(Debug)]
pub struct WorkTreeOutside {
    /// Where Git found the repository: the folder holding its `.git` entry,
    /// or its Git directory where no entry leads there.
    pub path: PathBuf,
    /// The work tree Git reports for it.
    pub work_tree: PathBuf,
}

impl std::fmt::Display for WorkTreeOutside {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GitTurtle opens a repository only inside the folder Git found it in: the work tree of the repository at {} is {} (core.worktree).",
            self.path.display(),
            self.work_tree.display()
        )
    }
}

impl std::error::Error for WorkTreeOutside {}

#[cfg(test)]
thread_local! {
    /// Runs in [`GitRepository::open`] between the capture and Git's
    /// confirmation on this thread: a test re-points what the capture read.
    static OPEN_CHECKPOINT: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
    /// Runs in [`GitRepository::verify_at_path`] as it is about to ask Git
    /// on this thread: a test moves a link back under the check.
    static VERIFY_CHECKPOINT: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
    /// Runs in [`resolve_git_entry`] as it is about to open a gitfile, on
    /// this thread: a test swaps a FIFO in for it.
    static GITFILE_CHECKPOINT: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
    /// Fails every [`open_regular_file`] on this thread as a process out of
    /// file descriptors does: a test reaches the error only this process
    /// meets, Git's own open succeeding.
    static OPEN_REFUSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
    /// The inode of `git_dir` at capture, where the platform has one
    /// ([`GitRepository::git_dir_inode`]).
    git_dir_inode: Option<u64>,
    /// The inode of `common` at capture ([`GitRepository::common_dir_inode`]).
    common_inode: Option<u64>,
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
    /// What those files say, as last confirmed: unchanged, the work tree
    /// and common directory have not moved; changed, Git is asked once
    /// and, the repository the same, they are confirmed again.
    confirmed: Mutex<Confirmed>,
}

/// What [`GitRepository::verify_at_path`] compares against, as last
/// confirmed.
#[derive(Debug)]
struct Confirmed {
    /// The file identities of [`Identity::configs`].
    stamps: Vec<Option<FileStamp>>,
    /// The paths those files store, which their stamps cannot speak for: a
    /// link on the way re-pointed touches none of the files, so each is
    /// resolved again at every check. Which paths they are is the files'
    /// to say, so a check takes them again only where a stamp changed
    /// ([`Identity::stored_paths`]).
    stored: Vec<StoredPath>,
}

/// A path Git keeps in a file and resolves itself ([`Confirmed::stored`]):
/// a `commondir` file's, the `core.worktree` Git reads.
#[derive(Debug)]
struct StoredPath {
    /// The stored path, joined to the Git directory as Git joins it.
    path: PathBuf,
    /// The captured folder it must lead to: the common directory or the
    /// root, never where it happened to lead when last confirmed, a value
    /// a link flipped back and forth could restore.
    leads_to: PathBuf,
}

impl StoredPath {
    /// Whether the stored path still leads where it must, by the filesystem
    /// alone: one path resolution, starting no process.
    fn holds(&self) -> bool {
        self.path
            .canonicalize()
            .is_ok_and(|now| now == self.leads_to)
    }
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
    /// What the repository is, and whether the `.git` entry at its root
    /// could not be read ([`GitEntry::Unread`]): the open then refuses a
    /// handle that nothing could confine.
    fn capture(root: &Path, git_dir: &Path, bare: bool) -> Result<(Self, bool)> {
        let root = root
            .canonicalize()
            .context("Resolve the repository's folder")?;
        let git_dir = git_dir
            .canonicalize()
            .context("Resolve the repository's Git directory")?;
        let entry = (!bare).then(|| git_entry_holder(&root)).flatten();
        let said = entry.map(read_git_entry);
        let unread = matches!(said, Some(GitEntry::Unread));
        let holder = match said {
            Some(GitEntry::Leads(found)) if found == git_dir => entry.map(Path::to_path_buf),
            _ => None,
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
        let common = common_dir(&git_dir)
            .context("Read the repository's common directory")?
            .unwrap_or_else(|| git_dir.clone());
        if common != git_dir {
            configs.push(common.join("config"));
        }
        let git_dir_inode = directory_inode(&git_dir);
        let common_inode = if common == git_dir {
            git_dir_inode
        } else {
            directory_inode(&common)
        };
        // The stored paths after the stamps, as every check takes them: a
        // file edited after the stamps is a changed stamp at the next check.
        let stamps = configs.iter().map(|path| FileStamp::read(path)).collect();
        let stored = Self::stored_paths(&git_dir, &common, &root, bare)?;
        let identity = Self {
            root,
            git_dir,
            common,
            git_dir_inode,
            common_inode,
            holder,
            ceiling,
            confined,
            configs,
            confirmed: Mutex::new(Confirmed { stamps, stored }),
        };
        Ok((identity, unread))
    }

    /// The paths Git reads out of this repository's files and resolves
    /// itself, with the folder each must lead to: a `commondir` file's and,
    /// for a work tree, the one `core.worktree` Git reads here
    /// ([`work_tree_git_reads`]). The files choose them, never where a path
    /// leads as it is read: a link flipped during a check once dropped its
    /// `core.worktree` for the handle's life, and a `commondir` leading
    /// through a link to the repository's own Git directory kept no entry,
    /// so re-pointing either link went unseen and a write reached the other
    /// repository. A process is started only for a file that can name a
    /// work tree.
    fn stored_paths(
        git_dir: &Path,
        common: &Path,
        root: &Path,
        bare: bool,
    ) -> Result<Vec<StoredPath>> {
        let mut stored = Vec::new();
        let commondir = stored_common_dir(git_dir)
            .with_context(|| format!("Read {}", git_dir.join("commondir").display()))?;
        if let Some(path) = commondir {
            stored.push(StoredPath {
                path,
                leads_to: common.to_owned(),
            });
        }
        if !bare && let Some(value) = work_tree_git_reads(git_dir, common)? {
            stored.push(StoredPath {
                path: git_dir.join(value),
                leads_to: root.to_owned(),
            });
        }
        Ok(stored)
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

/// `directory`'s inode, where the platform has one and it can be read.
fn directory_inode(directory: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(directory)
            .ok()
            .map(|metadata| metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        None
    }
}

/// A linked worktree's common Git directory, from its `commondir` file
/// (relative to `git_dir`), canonical; `None` where there is none.
fn common_dir(git_dir: &Path) -> std::io::Result<Option<PathBuf>> {
    Ok(stored_common_dir(git_dir)?.and_then(|path| path.canonicalize().ok()))
}

/// `path` opened for reading where it is a regular file; `None` where nothing
/// is there or it is anything else. Opening a FIFO waits for a writer with no
/// deadline, on the reader thread and at a check that holds the identity's
/// lock, so the open never waits and the file it opened is the one whose type
/// decides: a type read before the open left a moment for a swap to a FIFO
/// to block both. Anything but a regular file is left to Git, whose read has
/// a deadline. Nothing there includes a path through something that is no
/// folder, as Git takes one. Any other failure is an error, never "no such
/// file": a process out of file descriptors once took a stored path for one
/// that was not there, and the check it ran for kept no guard on it.
#[cfg(unix)]
fn open_regular_file(path: &Path) -> std::io::Result<Option<std::fs::File>> {
    use rustix::fs::{FileType, Mode, OFlags, fstat, open};
    #[cfg(test)]
    if OPEN_REFUSED.get() {
        return Err(rustix::io::Errno::MFILE.into());
    }
    let flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::CLOEXEC;
    let file = match rustix::io::retry_on_intr(|| open(path, flags, Mode::empty())) {
        Ok(file) => file,
        Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let regular = FileType::from_raw_mode(fstat(&file)?.st_mode) == FileType::RegularFile;
    Ok(regular.then(|| file.into()))
}

/// `path` opened for reading where it is a regular file, as on Unix.
#[cfg(not(unix))]
fn open_regular_file(path: &Path) -> std::io::Result<Option<std::fs::File>> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => std::fs::File::open(path).map(Some),
        Ok(_) => Ok(None),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// The path a linked worktree's `commondir` file stores, joined to `git_dir`
/// as Git joins it; `None` where there is none, or it is no regular file
/// ([`open_regular_file`]).
fn stored_common_dir(git_dir: &Path) -> std::io::Result<Option<PathBuf>> {
    let Some(file) = open_regular_file(&git_dir.join("commondir"))? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(64 * 1024).read_to_end(&mut bytes)?;
    let end = bytes
        .iter()
        .rposition(|byte| *byte != b'\n' && *byte != b'\r')
        .map_or(0, |last| last + 1);
    Ok(Some(git_dir.join(path_from_bytes(&bytes[..end]))))
}

/// Whether the configuration `file` can hold a `core.worktree`: it names the
/// word in any case (a key is matched case-insensitively), or it is too
/// large to say. A missing file holds none, one that is no regular file
/// ([`open_regular_file`]) is left to Git, and one that cannot be opened is
/// an error. A key name never spans a line continuation, so the word is
/// whole.
fn may_name_a_work_tree(file: &Path) -> std::io::Result<bool> {
    const LIMIT: u64 = 1024 * 1024;
    let Some(file) = open_regular_file(file)? else {
        return Ok(false);
    };
    let mut bytes = Vec::new();
    Ok(match file.take(LIMIT + 1).read_to_end(&mut bytes) {
        Ok(_) => {
            bytes.len() as u64 > LIMIT
                || bytes
                    .windows(b"worktree".len())
                    .any(|word| word.eq_ignore_ascii_case(b"worktree"))
        }
        Err(_) => true,
    })
}

/// The `core.worktree` Git reads for the Git directory `git_dir`, as it reads
/// one when it sets up a repository (`check_repository_format_gently`). Under
/// `extensions.worktreeConfig`, in the common `config`, the Git directory's
/// own `config.worktree` is read and its value wins over the common
/// `config`'s. Otherwise the common `config`'s is read, unless a `commondir`
/// file exists: a linked worktree does not take its main worktree's. A
/// common `config` with no `core.repositoryformatversion`, or one Git reads
/// as -1, defines nothing, as Git clears what it read from one; a guard on a
/// value Git ignores once asked Git at every check.
fn work_tree_git_reads(git_dir: &Path, common: &Path) -> Result<Option<PathBuf>> {
    let file = common.join("config");
    let shared = WorkTreeSettings::read(git_dir, &file)?;
    if !shared.has_format_version(git_dir, &file)? {
        return Ok(None);
    }
    if shared.reads_per_worktree(git_dir, &file)? {
        let own = WorkTreeSettings::read(git_dir, &git_dir.join("config.worktree"))?;
        return Ok(own.value.or(shared.value));
    }
    // By the entry, as Git tests for the file: reading it can wait.
    let has_common = std::fs::symlink_metadata(git_dir.join("commondir")).is_ok();
    Ok(shared.value.filter(|_| !has_common))
}

/// What a configuration file says of the work tree, by Git's own parser and
/// with no includes, as Git reads it when it sets up a repository.
#[derive(Default)]
struct WorkTreeSettings {
    /// Its `core.worktree`, the last where it sets several, as Git takes it.
    value: Option<PathBuf>,
    /// Its `extensions.worktreeConfig` as it spells it, the last where it
    /// sets several, with no value at all as `Some(None)`.
    per_worktree: Option<Option<Vec<u8>>>,
    /// Its `core.repositoryformatversion` as it spells it, the last where
    /// it sets several, with no value at all as `Some(None)`.
    version: Option<Option<Vec<u8>>>,
}

impl WorkTreeSettings {
    /// What `file` says: nothing where it cannot name either setting
    /// ([`may_name_a_work_tree`]), starting no process, or is not one Git
    /// can read, so it is not one Git read them from either. One process
    /// reads a file's settings.
    fn read(git_dir: &Path, file: &Path) -> Result<Self> {
        let mut settings = Self::default();
        if !may_name_a_work_tree(file).with_context(|| format!("Read {}", file.display()))? {
            return Ok(settings);
        }
        let mut command = git_command(GitTarget::unconfined(git_dir));
        command
            .args(["config", "--null", "--file"])
            .arg(file)
            .args([
                "--get-regexp",
                r"^(core\.repositoryformatversion|core\.worktree|extensions\.worktreeconfig)$",
            ]);
        let output = bounded_output(command, GIT_TIMEOUT)?;
        if !output.status.success() {
            return Ok(settings);
        }
        // Each entry is its key, then a line feed and its value if it has
        // one. The last of several is the one Git goes by.
        for entry in output.stdout.split(|byte| *byte == 0) {
            let (key, value) = match entry.iter().position(|byte| *byte == b'\n') {
                Some(end) => (&entry[..end], Some(&entry[end + 1..])),
                None => (entry, None),
            };
            match key {
                b"core.worktree" => {
                    settings.value = value.filter(|value| !value.is_empty()).map(path_from_bytes);
                }
                b"core.repositoryformatversion" => {
                    settings.version = Some(value.map(<[u8]>::to_vec));
                }
                b"extensions.worktreeconfig" => {
                    settings.per_worktree = Some(value.map(<[u8]>::to_vec));
                }
                _ => {}
            }
        }
        Ok(settings)
    }

    /// Whether its `extensions.worktreeConfig` is on, read from `file` by
    /// Git where only Git can say ([`git_boolean`]).
    fn reads_per_worktree(&self, git_dir: &Path, file: &Path) -> Result<bool> {
        match &self.per_worktree {
            None => Ok(false),
            Some(value) => match git_boolean(value.as_deref()) {
                Some(on) => Ok(on),
                None => {
                    Ok(
                        config_by_git(git_dir, file, "--type=bool", "extensions.worktreeconfig")?
                            == b"true",
                    )
                }
            },
        }
    }

    /// Whether Git goes by this `config`'s settings: it clears them where
    /// the format version is missing or reads as -1. An unsigned string of
    /// digits stands; any other spelling, a sign, a prefix or a unit, Git
    /// reads ([`config_by_git`]), as `-0x1` is -1 and `-1k` is not.
    fn has_format_version(&self, git_dir: &Path, file: &Path) -> Result<bool> {
        match &self.version {
            None => Ok(false),
            Some(Some(value)) if !value.is_empty() && value.iter().all(u8::is_ascii_digit) => {
                Ok(true)
            }
            Some(_) => {
                Ok(
                    config_by_git(git_dir, file, "--type=int", "core.repositoryformatversion")?
                        != b"-1",
                )
            }
        }
    }
}

/// A configuration boolean Git spells plainly, as Git reads it: no value at
/// all is true and an empty one false, `true`, `yes`, `on`, `false`, `no` and
/// `off` go by their word in any case, and a string of decimal digits is true
/// unless every digit is zero (one Git refuses to read, as `08`, it refuses
/// the repository for too, so that answer is never used). `None` for anything
/// else, a sign, a prefix or a unit, which only Git can say
/// ([`config_by_git`]): which of those it takes, and as what, is its C
/// library's, and `0b0`, zero to a C23 `strtoimax`, once read here as true
/// put the guard on a file Git did not read.
fn git_boolean(value: Option<&[u8]>) -> Option<bool> {
    let Some(value) = value else {
        return Some(true);
    };
    let is = |word: &str| value.eq_ignore_ascii_case(word.as_bytes());
    if is("true") || is("yes") || is("on") {
        return Some(true);
    }
    if value.is_empty() || is("false") || is("no") || is("off") {
        return Some(false);
    }
    value
        .iter()
        .all(u8::is_ascii_digit)
        .then(|| value.iter().any(|digit| *digit != b'0'))
}

/// The last `key` in `file` as the parser Git itself reads it with gives it
/// (`kind`, `--type=bool` or `--type=int`), for a spelling only Git can say.
/// An error where Git refuses the value: the paths are then not taken again,
/// what was confirmed stays, and the next check asks once more.
fn config_by_git(git_dir: &Path, file: &Path, kind: &str, key: &str) -> Result<Vec<u8>> {
    let mut command = git_command(GitTarget::unconfined(git_dir));
    command
        .args(["config", "--file"])
        .arg(file)
        .args([kind, "--get", key]);
    let output = bounded_output(command, GIT_TIMEOUT)?;
    ensure!(
        output.status.success(),
        "Git could not read {key} in {}.",
        file.display()
    );
    Ok(trim_line(&output.stdout).to_vec())
}

/// The nearest folder at or above `root` with a `.git` entry, where Git
/// discovery from `root` stops.
fn git_entry_holder(root: &Path) -> Option<&Path> {
    root.ancestors()
        .find(|folder| std::fs::symlink_metadata(folder.join(".git")).is_ok())
}

/// What a folder's `.git` entry says ([`read_git_entry`]).
enum GitEntry {
    /// It leads to this Git directory, canonical.
    Leads(PathBuf),
    /// A gitfile that could not be read or followed, or something no `.git`
    /// can be, as a FIFO swapped in for one.
    Unread,
    /// Nothing to say: it is gone, or a link that leads nowhere.
    Unknown,
}

/// What the `.git` entry in `holder` says: the directory itself, or a
/// gitfile's `gitdir:` path, relative to `holder`. A gitfile that is no
/// regular file as it is opened, swapped for a FIFO, is unread
/// ([`open_regular_file`]): every check opens it.
fn read_git_entry(holder: &Path) -> GitEntry {
    let entry = holder.join(".git");
    let Ok(metadata) = std::fs::symlink_metadata(&entry) else {
        return GitEntry::Unknown;
    };
    let kind = metadata.file_type();
    if kind.is_dir() || kind.is_symlink() {
        return entry
            .canonicalize()
            .map_or(GitEntry::Unknown, GitEntry::Leads);
    }
    if !kind.is_file() {
        return GitEntry::Unread;
    }
    #[cfg(test)]
    GITFILE_CHECKPOINT.with_borrow_mut(|hook| {
        if let Some(hook) = hook {
            hook();
        }
    });
    read_gitfile(&entry, holder).map_or(GitEntry::Unread, GitEntry::Leads)
}

/// The Git directory the `.git` entry in `holder` leads to, canonical, where
/// it can be said ([`read_git_entry`]).
fn resolve_git_entry(holder: &Path) -> Option<PathBuf> {
    match read_git_entry(holder) {
        GitEntry::Leads(found) => Some(found),
        GitEntry::Unread | GitEntry::Unknown => None,
    }
}

/// The Git directory the gitfile `entry` in `holder` names, canonical.
fn read_gitfile(entry: &Path, holder: &Path) -> Option<PathBuf> {
    let mut bytes = Vec::new();
    open_regular_file(entry)
        .ok()??
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let target = bytes.strip_prefix(b"gitdir: ")?;
    // Git strips only trailing line ends, so a path ending in a space is
    // another folder, then reads the rest as a C string: what follows a NUL
    // is not part of the path.
    let end = target
        .iter()
        .rposition(|byte| *byte != b'\n' && *byte != b'\r')
        .map_or(0, |last| last + 1);
    let target = target[..end].split(|byte| *byte == 0).next()?;
    holder.join(path_from_bytes(target)).canonicalize().ok()
}

/// Where Git found the repository at `git_dir` from `requested`: the folder
/// holding the `.git` entry that leads to it, or the Git directory itself
/// where no entry does, as for a Git directory opened as it is.
fn folder_found_in(requested: &Path, git_dir: &Path) -> PathBuf {
    let requested = requested.canonicalize().ok();
    requested
        .as_deref()
        .and_then(git_entry_holder)
        .filter(|holder| resolve_git_entry(holder).as_deref() == Some(git_dir))
        .unwrap_or(git_dir)
        .to_path_buf()
}

/// What Git reports a folder's repository to be ([`GitRepository::open`]).
#[derive(PartialEq, Eq)]
struct Reported {
    bare: bool,
    git_dir: PathBuf,
    /// The work-tree root (a bare repository's Git directory).
    root: PathBuf,
}

impl Reported {
    /// Git's answer from `requested`, unconfined; a folder inside a work
    /// tree reports that work tree.
    fn by_git(requested: &Path) -> Result<Self> {
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
        let root = if bare {
            git_dir.clone()
        } else {
            path_from_bytes(trim_line(&run_git(
                GitTarget::unconfined(requested),
                &["rev-parse", "--show-toplevel"],
            )?))
        };
        Ok(Self {
            bare,
            git_dir,
            root,
        })
    }
}

/// Whether a command failed because Git found no repository: how every
/// command of an opened repository fails once its `.git` is gone. Decided by
/// the type its failure carries ([`NotARepository`]), from Git's own words,
/// before the message, which shows them as a terminal leaves them; the
/// message is read only for a failure that carries none.
pub fn finds_no_repository(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<NotARepository>())
        || says_no_repository(&format!("{error:#}"))
}

/// Git's words for a folder it finds no repository at: its usual "not a git
/// repository", and since Git 2.56 what it says of a `.git` file whose target
/// is no repository, as a linked worktree's is once its administration
/// directory is gone (2.55 printed "not a git repository: (null)" there).
const NO_REPOSITORY_WORDS: [&str; 2] = [
    "not a git repository",
    "gitfile does not point to a valid repository",
];

/// Whether `message` holds Git's words for finding no repository, for a
/// failure that carries no [`NotARepository`] type.
pub fn says_no_repository(message: &str) -> bool {
    NO_REPOSITORY_WORDS
        .iter()
        .any(|words| message.contains(words))
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
        let reported = Reported::by_git(requested)?;
        ensure!(
            reported.root.is_absolute() && reported.git_dir.is_absolute(),
            "Git returned a non-absolute repository path"
        );
        let (identity, gitfile_unread) =
            Identity::capture(&reported.root, &reported.git_dir, reported.bare)?;
        let identity = Arc::new(identity);
        let repository = Self {
            path: reported.root.clone(),
            bare: reported.bare,
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
        let found = match repository.found_at_path()? {
            // Git read the gitfile the capture could not, swapped under it:
            // captured so, the handle had no ceiling and asked Git at every
            // check for its whole life.
            FoundAtPath::Same if gitfile_unread => {
                return Err(ChangedWhileOpening {
                    found: Some(repository.path.clone()),
                    path: repository.path,
                }
                .into());
            }
            // A decoy `.git` directory that Git skips, nearer the root than the real
            // one, leaves no holder either, so that handle asks Git at each
            // check. Accepted: it costs one process, lets no write through,
            // and refusing would risk a healthy layout.
            FoundAtPath::Same => return Ok(repository),
            FoundAtPath::Other(found) => Some(found),
            FoundAtPath::None => None,
        };
        // No `.git` entry leads from the work tree back to the Git directory
        // and Git, asked again, reports both as it did: the layout as it
        // stands, which the confinement refuses, with nothing to open again.
        if !reported.bare
            && repository.identity.holder.is_none()
            && Reported::by_git(requested).is_ok_and(|now| now == reported)
        {
            return Err(WorkTreeOutside {
                path: folder_found_in(requested, &repository.identity.git_dir),
                work_tree: repository.path,
            }
            .into());
        }
        Err(ChangedWhileOpening {
            path: repository.path,
            found,
        }
        .into())
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

    /// The inode of [`Self::git_dir`] when the repository was opened, where
    /// the platform has one: read once by the open, so asking starts no
    /// filesystem access, and a session can save it beside the path.
    pub fn git_dir_inode(&self) -> Option<u64> {
        self.identity.git_dir_inode
    }

    /// The inode of [`Self::common_dir`] when the repository was opened
    /// ([`Self::git_dir_inode`]).
    pub fn common_dir_inode(&self) -> Option<u64> {
        self.identity.common_inode
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
        if !output.status.success() && no_repository_found(&output.stderr) {
            return Ok(FoundAtPath::None);
        }
        let output = checked_git_output(&args, output)?;
        // The Git directory's line, its common directory's, and a work
        // tree's root. A path holding a line break splits into more lines:
        // each is then asked for alone, its whole output one path, since an
        // open confirms what it captured here and such a repository once
        // could not be opened at all.
        let mut lines: Vec<_> = trim_line(&output)
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
            _ => return Ok(FoundAtPath::Other(path_from_bytes(&output))),
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
    /// repository the same, the edited files are confirmed again. So does a
    /// path those files store no longer leading where it must, a link on
    /// the way to the common directory or the work tree re-pointed; the
    /// paths stay as they were while no file changed, whatever Git answers.
    /// Paths that cannot be read after a file changed are an error, what
    /// was confirmed stays, and the next check asks again.
    pub fn verify_at_path(&self) -> Result<FoundAtPath> {
        if !(self.identity.confined && self.identity.holds(&self.path, self.bare)) {
            return self.found_at_path();
        }
        let stamps = self.identity.config_stamps();
        let mut confirmed = self
            .identity
            .confirmed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let changed = confirmed.stamps != stamps;
        if !changed && confirmed.stored.iter().all(StoredPath::holds) {
            return Ok(FoundAtPath::Same);
        }
        let identity = &self.identity;
        let stored = changed
            .then(|| {
                Identity::stored_paths(
                    &identity.git_dir,
                    &identity.common,
                    &identity.root,
                    self.bare,
                )
            })
            .transpose()?;
        #[cfg(test)]
        VERIFY_CHECKPOINT.with_borrow_mut(|hook| {
            if let Some(hook) = hook {
                hook();
            }
        });
        let found = self.found_at_path()?;
        if found == FoundAtPath::Same
            && let Some(stored) = stored
        {
            *confirmed = Confirmed { stamps, stored };
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
    /// stopped at its confinement: a confined command that finds no
    /// repository, as the type its failure carries says, or for a failure
    /// that carries none its message ([`finds_no_repository`]), means the
    /// repository's `.git` went away while it ran or just before, so the
    /// diagnosis ([`Self::found_at_path`]) says what Git finds there now.
    /// The command's own error stays beneath it, uncertain
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
        Self::lfs_storage_from(common, &configured)
    }

    /// Where `lfs.storage`, as `git config` answered, puts local LFS objects.
    /// A failure is typed as any configuration read's is ([`git_failure`]).
    pub(crate) fn lfs_storage_from(common: PathBuf, configured: &Output) -> Result<PathBuf> {
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
            return Err(git_failure(
                &configured.stderr,
                format!(
                    "Unable to read local LFS storage configuration: {}",
                    reported(&configured.stderr).trim()
                ),
            ));
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
    checked_git_output(args, run_git_output(target, args)?)
}

/// The stdout of `git <args>`, which wrote `output`, once it succeeded;
/// otherwise its failure, "Git <command> failed: …" with Git's stderr as a
/// terminal leaves it, typed where Git found no repository
/// ([`git_failure`]).
fn checked_git_output(args: &[&str], output: Output) -> Result<Vec<u8>> {
    if !output.status.success() {
        return Err(git_failure(
            &output.stderr,
            format!(
                "Git {} failed: {}",
                args.first().unwrap_or(&"command"),
                reported(&output.stderr).trim()
            ),
        ));
    }
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

/// Whether a failed command's `stderr` says no repository holds the path it
/// ran at: one of its lines in Git's own words ([`text`], split on line
/// feeds alone) starts with `fatal: ` and [`NO_REPOSITORY_WORDS`]. Git names
/// paths in its other failures, so the words anywhere else, a folder's name
/// in "fatal: cannot change to '/x/not a git repository': Permission
/// denied", are that failure. A carriage return in a path Git names, a mount
/// point's or a linked worktree's main repository's, stays inside its line,
/// where [`reported`] would leave only what follows it and turn a path with
/// no repository into a failure refusing every write.
fn no_repository_found(stderr: &[u8]) -> bool {
    text(stderr).split('\n').any(|line| {
        line.strip_prefix("fatal: ").is_some_and(|rest| {
            NO_REPOSITORY_WORDS
                .iter()
                .any(|words| rest.starts_with(words))
        })
    })
}

/// A Git command failed because, in its own words, no repository holds the
/// path it ran at (`no_repository_found`): told apart by this type rather
/// than by its message, which shows those words as a terminal leaves them,
/// so a carriage return in a path Git names leaves the message only what
/// follows it. It is the failure itself, its message the command's own
/// (`git_failure`), so what every surface shows is unchanged. The app tells
/// a folder with no repository apart, and a tab whose repository is gone, by
/// this type before any text, as [`finds_no_repository`] does.
#[derive(Debug)]
pub struct NotARepository(
    /// The failure's message as it is shown, as "Git rev-parse failed: …".
    pub String,
);

impl std::fmt::Display for NotARepository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NotARepository {}

/// A failed Git command's error, `message` as it is shown: a
/// [`NotARepository`] where Git's raw `stderr` says no repository holds the
/// path ([`no_repository_found`]), otherwise `message` alone. [`run_git`],
/// status and the history streams (`history_failure`) report through it,
/// the reads whose failure says that a repository's `.git` is gone, and so
/// does a write's command (`checked_write_output`) and every step of a
/// write that reports Git's stderr, which [`GitRepository::word_gone`] then
/// words as the repository gone: its configuration reads and edits too,
/// since in a linked worktree whose administration directory is gone even
/// `git config` fails so. Only `check-ref-format`, which needs no
/// repository, never does.
/// Decided 2026-11-07: telling them apart read only the message, Git's
/// words as a terminal leaves them, so where a Git named a path holding a
/// carriage return such a read left the tab unmarked and showed a cut
/// message.
fn git_failure(stderr: &[u8], message: String) -> anyhow::Error {
    if no_repository_found(stderr) {
        NotARepository(message).into()
    } else {
        anyhow::Error::msg(message)
    }
}

/// Git's captured output in `bytes` as a message shows it: [`text`] as a
/// terminal leaves it ([`as_terminal_leaves`]). Every failure that embeds
/// Git's stdout or stderr reads it through this, before its own words join
/// it, so a step's progress never runs into the error after it; the
/// diagnostics that match Git's words read the raw bytes.
fn reported(bytes: &[u8]) -> String {
    let text = text(bytes);
    match as_terminal_leaves(&text) {
        std::borrow::Cow::Borrowed(_) => text,
        std::borrow::Cow::Owned(shown) => shown,
    }
}

/// [`reported`] for a failure that redacts Git's output: `redact` reads
/// Git's own text first, and only what it leaves is read as a terminal
/// leaves it, as a write's outcome is (`GitRepository::execute`). Read the
/// other way round, a carriage return between a redaction's marker and its
/// secret, as in "password=\rsecret", left only the secret, which no marker
/// then names. Decided 2026-11-07: every failure that redacts reads Git's
/// streams through this.
fn reported_redacted(bytes: &[u8], redact: impl FnOnce(&str) -> String) -> String {
    as_terminal_leaves(&redact(&text(bytes))).into_owned()
}

/// `output`, Git's reported output, as a terminal leaves it: Git writes a
/// step's progress on one line, each step ended by a carriage return so the
/// next overwrites it, so within each line only the text after its last
/// carriage return is kept, a trailing one (as in a `\r\n` line end)
/// keeping the text before it, and a line that ends up blank is dropped. A
/// line without a carriage return is kept as it is, so output without one
/// comes back unchanged. Decided 2026-11-07: after a rebase, the status read
/// "Rebasing (1/3)Rebasing (2/3)Rebasing (3/3)Successfully rebased and
/// updated refs/heads/main.", the returns dropped as it was drawn, where a
/// terminal leaves only the outcome. A write's outcome reads it so
/// (`GitRepository::execute`), and a failure reads Git's own streams so
/// ([`reported`]), never the text it composes around them, where our words
/// share a line with Git's. Where a carriage return was found, the blank
/// lines before the first kept one are dropped too, so the text never starts
/// with a line break. Lines are cut only at the ASCII CR and LF characters,
/// which never occur inside a multi-byte UTF-8 sequence, so every character
/// kept is one of the decoded text's own.
fn as_terminal_leaves(output: &str) -> std::borrow::Cow<'_, str> {
    if !output.contains('\r') {
        return output.into();
    }
    let lines: Vec<&str> = output
        .split('\n')
        .filter_map(|line| {
            if !line.contains('\r') {
                return Some(line);
            }
            let line = line.trim_end_matches('\r');
            let shown = line.rsplit_once('\r').map_or(line, |(_, after)| after);
            (!shown.trim().is_empty()).then_some(shown)
        })
        .skip_while(|line| line.trim().is_empty())
        .collect();
    lines.join("\n").into()
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

    /// The check of `repository`, with `change` run as it is about to ask
    /// Git.
    #[cfg(unix)]
    fn verify_changed_part_way(
        repository: &GitRepository,
        change: impl FnMut() + 'static,
    ) -> Result<FoundAtPath> {
        VERIFY_CHECKPOINT.with_borrow_mut(|hook| *hook = Some(Box::new(change)));
        let found = repository.verify_at_path();
        VERIFY_CHECKPOINT.with_borrow_mut(|hook| *hook = None);
        found
    }

    /// Points the existing link `link` at `target`.
    #[cfg(unix)]
    fn re_point(link: &Path, target: &Path) {
        std::fs::remove_file(link).unwrap();
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    /// A folder `name` in `base` with a repository and one commit.
    #[cfg(unix)]
    fn committed_repository(base: &Path, name: &str) -> PathBuf {
        let root = base.join(name);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet"]);
        git(&root, &["commit", "--quiet", "--allow-empty", "-m", name]);
        root
    }

    /// A worktree of `main` at `path`, on its own branch `name`.
    #[cfg(unix)]
    fn add_worktree(main: &Path, name: &str, path: &Path) {
        git(
            main,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                name,
                path.to_str().unwrap(),
            ],
        );
    }

    /// Where the entries the check keeps must lead, and the stored paths.
    #[cfg(unix)]
    fn guards(repository: &GitRepository) -> Vec<(PathBuf, PathBuf)> {
        repository
            .identity
            .confirmed
            .lock()
            .unwrap()
            .stored
            .iter()
            .map(|stored| (stored.leads_to.clone(), stored.path.clone()))
            .collect()
    }

    /// The guard on a `core.worktree` reached through a link is chosen by
    /// the file Git reads, never by where the link leads as it is chosen,
    /// and left alone while no stamped file changed: a flip during a check
    /// that went to Git once dropped it for the handle's life, and the next
    /// re-point staged the other tree. Whether or not the configuration
    /// changed during the flip, the write is refused before Git starts.
    #[cfg(unix)]
    #[test]
    fn a_link_flipped_during_a_check_keeps_the_work_tree_guard() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let root = committed_repository(&base, "root");
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("foreign.txt"), "foreign\n").unwrap();
        let link = base.join("tree-link");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        git(&root, &["config", "core.worktree", link.to_str().unwrap()]);
        let repository = GitRepository::open(&root).unwrap();
        assert_eq!(repository.verify_at_path().unwrap(), FoundAtPath::Same);

        for stamp_changed in [false, true] {
            re_point(&link, &elsewhere);
            if stamp_changed {
                git(&root, &["config", "gitturtle.marker", "1"]);
            }
            let (back, target) = (link.clone(), root.clone());
            let found = verify_changed_part_way(&repository, move || re_point(&back, &target));
            assert_eq!(found.unwrap(), FoundAtPath::Same, "stamp {stamp_changed}");

            re_point(&link, &elsewhere);
            let error = repository.execute(&WriteCommand::StageAll).unwrap_err();
            assert!(
                format!("{error:#}").contains("Git was not started"),
                "stamp {stamp_changed}: {error:#}"
            );
            re_point(&link, &root);
            assert_eq!(repository.verify_at_path().unwrap(), FoundAtPath::Same);
        }
    }

    /// The check keeps the paths Git reads for this repository and no
    /// others, as Git 2.56 reads them: `config.worktree` over the common
    /// `config` under `extensions.worktreeConfig`; otherwise the common
    /// `config`, but never where a `commondir` file exists, which itself
    /// always gets an entry. A value Git does not read is no guard, wherever
    /// it leads; one it reads is, wherever it leads now.
    #[cfg(unix)]
    #[test]
    fn the_check_guards_the_paths_git_reads() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let elsewhere = elsewhere.to_str().unwrap();
        let worktree_file = |value: &str| format!("[core]\n\tworktree = {value}\n");

        let plain = committed_repository(&base, "plain");
        let opened = GitRepository::open(&plain).unwrap();
        assert_eq!(guards(&opened), [], "nothing stored");

        let by_link = committed_repository(&base, "by-link");
        let link = base.join("by-link-tree");
        std::os::unix::fs::symlink(&by_link, &link).unwrap();
        git(
            &by_link,
            &["config", "core.worktree", link.to_str().unwrap()],
        );
        let opened = GitRepository::open(&by_link).unwrap();
        assert_eq!(guards(&opened), [(by_link.clone(), link.clone())]);

        let unread = committed_repository(&base, "unread");
        let file = unread.join(".git/config.worktree");
        std::fs::write(&file, worktree_file(elsewhere)).unwrap();
        let opened = GitRepository::open(&unread).unwrap();
        assert_eq!(opened.path(), unread, "no extension: Git ignores the file");
        assert_eq!(guards(&opened), []);

        let main = committed_repository(&base, "main");
        let linked = base.join("linked");
        add_worktree(&main, "linked", &linked);
        git(
            &main,
            &["config", "core.worktree", linked.to_str().unwrap()],
        );
        let opened = GitRepository::open(&linked).unwrap();
        let private = opened.git_dir().to_owned();
        let common = main.join(".git");
        let common_entry = (common.clone(), private.join("../.."));
        assert_eq!(
            guards(&opened),
            std::slice::from_ref(&common_entry),
            "the main worktree's value is not a linked worktree's"
        );

        let per_worktree = committed_repository(&base, "per-worktree");
        let second = base.join("per-worktree-linked");
        add_worktree(&per_worktree, "second", &second);
        git(
            &per_worktree,
            &["config", "extensions.worktreeConfig", "true"],
        );
        let opened = GitRepository::open(&second).unwrap();
        let private = opened.git_dir().to_owned();
        let tree_link = base.join("second-tree");
        std::os::unix::fs::symlink(&second, &tree_link).unwrap();
        std::fs::write(
            private.join("config.worktree"),
            worktree_file(tree_link.to_str().unwrap()),
        )
        .unwrap();
        let opened = GitRepository::open(&second).unwrap();
        assert_eq!(
            guards(&opened),
            [
                (per_worktree.join(".git"), private.join("../..")),
                (second.clone(), tree_link.clone()),
            ]
        );

        let own = committed_repository(&base, "own");
        let own_git = own.join(".git");
        let own_link = base.join("own-link");
        std::os::unix::fs::symlink(&own_git, &own_link).unwrap();
        std::fs::write(
            own_git.join("commondir"),
            format!("{}\n", own_link.display()),
        )
        .unwrap();
        git(&own, &["config", "core.worktree", elsewhere]);
        let opened = GitRepository::open(&own).unwrap();
        assert_eq!(
            opened.path(),
            own,
            "a commondir file: Git ignores the value"
        );
        assert_eq!(guards(&opened), [(own_git.clone(), own_link.clone())]);

        let both = committed_repository(&base, "both");
        git(&both, &["config", "extensions.worktreeConfig", "true"]);
        git(&both, &["config", "core.worktree", elsewhere]);
        let both_link = base.join("both-tree");
        std::os::unix::fs::symlink(&both, &both_link).unwrap();
        std::fs::write(
            both.join(".git/config.worktree"),
            worktree_file(both_link.to_str().unwrap()),
        )
        .unwrap();
        let opened = GitRepository::open(&both).unwrap();
        assert_eq!(
            guards(&opened),
            [(both.clone(), both_link.clone())],
            "config.worktree wins"
        );
    }

    /// A common `config` with no `core.repositoryformatversion` defines
    /// nothing: Git clears what it read from one, so it goes by neither its
    /// `core.worktree` nor the `config.worktree` its extension would name.
    /// No guard is kept on a value Git ignores, which asked Git at every
    /// check. The version in place, Git goes by the value, and a re-point of
    /// the link it reads is refused before Git starts.
    #[cfg(unix)]
    #[test]
    fn a_config_with_no_format_version_names_no_work_tree() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let common_settings = "\tfilemode = true\n\tbare = false\n";
        let layout = |name: &str, settings: String, own: Option<&str>| {
            let root = committed_repository(&base, name);
            for folder in ["sub1", "sub2"] {
                std::fs::create_dir(root.join(folder)).unwrap();
            }
            std::fs::write(
                root.join(".git/config"),
                format!("[core]\n{common_settings}{settings}"),
            )
            .unwrap();
            if let Some(own) = own {
                std::fs::write(root.join(".git/config.worktree"), own).unwrap();
            }
            root
        };
        let layouts = [
            ("relative", "\tworktree = ../sub1\n".to_owned(), None),
            (
                "absolute",
                format!("\tworktree = {}\n", elsewhere.display()),
                None,
            ),
            (
                "extension",
                "\tworktree = ../sub1\n[extensions]\n\tworktreeConfig = true\n".to_owned(),
                Some("[core]\n\tworktree = ../sub2\n"),
            ),
        ];
        for (name, settings, own) in layouts {
            let root = layout(name, settings, own);
            let opened = GitRepository::open(&root).unwrap();
            assert_eq!(opened.path(), root, "{name}");
            assert_eq!(guards(&opened), [], "{name}");
            for _ in 0..3 {
                let found = verify_changed_part_way(&opened, || panic!("the check asked Git"));
                assert_eq!(found.unwrap(), FoundAtPath::Same, "{name}");
            }
        }

        let root = committed_repository(&base, "versioned");
        let link = base.join("versioned-tree");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        let settings = format!(
            "\trepositoryformatversion = 0\n\tworktree = {}\n",
            link.display()
        );
        std::fs::write(
            root.join(".git/config"),
            format!("[core]\n{common_settings}{settings}"),
        )
        .unwrap();
        let opened = GitRepository::open(&root).unwrap();
        assert_eq!(guards(&opened), [(root.clone(), link.clone())]);
        re_point(&link, &elsewhere);
        let error = opened.execute(&WriteCommand::StageAll).unwrap_err();
        assert!(
            format!("{error:#}").contains("Git was not started"),
            "{error:#}"
        );
    }

    /// `core.repositoryformatversion` as Git goes by it, real Git's answer
    /// for each spelling on whatever C library it was built with: a version
    /// of -1 is none, so Git ignores `core.worktree`, and a guard on that
    /// value once asked Git at every check. The last of several counts.
    #[cfg(unix)]
    #[test]
    fn a_format_version_reads_as_git_goes_by_it() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let line = |value: &str| format!("\trepositoryformatversion = {value}\n");
        let cases = [
            ("-1", line("-1")),
            ("-01", line("-01")),
            ("-0x1", line("-0x1")),
            ("-0b1", line("-0b1")),
            ("-2", line("-2")),
            ("-1k", line("-1k")),
            ("-0", line("-0")),
            ("0x0", line("0x0")),
            ("0", line("0")),
            ("1", line("1")),
            ("absent", String::new()),
            ("0 then -1", format!("{}{}", line("0"), line("-1"))),
            ("-1 then 0", format!("{}{}", line("-1"), line("0"))),
        ];
        for (index, (name, version)) in cases.into_iter().enumerate() {
            let root = committed_repository(&base, &format!("repository-{index}"));
            let tree = root.join("sub1");
            std::fs::create_dir(&tree).unwrap();
            let git_dir = root.join(".git");
            std::fs::write(
                git_dir.join("config"),
                format!("[core]\n{version}\tbare = false\n\tworktree = ../sub1\n"),
            )
            .unwrap();
            let by_git = Command::new("git")
                .arg("-C")
                .arg(&tree)
                .args(["rev-parse", "--show-toplevel"])
                .output()
                .unwrap();
            let by_git = by_git
                .status
                .success()
                .then(|| trim_line(&by_git.stdout) == tree.as_os_str().as_encoded_bytes());
            let ours = work_tree_git_reads(&git_dir, &git_dir)
                .map(|value| value.is_some())
                .ok();
            assert_eq!(ours, by_git, "{name}");
        }
    }

    /// `extensions.worktreeConfig` reads as real Git reads it, one value of
    /// each form, on whatever C library this Git was built with: refused
    /// where Git refuses it. A spelling only Git can read goes to Git, and
    /// the last of several values is the one that counts.
    #[test]
    fn a_configuration_boolean_reads_as_git_reads_it() {
        let fixture = tempfile::tempdir().unwrap();
        let file = fixture.path().join("config");
        let values = [
            None,
            Some(""),
            Some("true"),
            Some("TRUE"),
            Some("yes"),
            Some("on"),
            Some("false"),
            Some("FALSE"),
            Some("no"),
            Some("off"),
            Some("0"),
            Some("00"),
            Some("1"),
            Some("2"),
            Some("-1"),
            Some("0x0"),
            Some("0x1"),
            Some("0x10"),
            Some("0xa"),
            Some("0k"),
            Some("1k"),
            Some("0b0"),
            Some("0B0"),
            Some("0b00"),
            Some("-0b0"),
            Some("0b0k"),
            Some("maybe"),
        ];
        let line = |value: Option<&str>| match value {
            Some(value) => format!("\tworktreeConfig = {value}\n"),
            None => "\tworktreeConfig\n".to_owned(),
        };
        for value in values {
            for lines in [line(value), format!("{}{}", line(Some("0x1")), line(value))] {
                std::fs::write(&file, format!("[extensions]\n{lines}")).unwrap();
                let by_git = Command::new("git")
                    .args(["config", "--file"])
                    .arg(&file)
                    .args(["--type=bool", "--get", "extensions.worktreeconfig"])
                    .output()
                    .unwrap();
                let by_git = by_git
                    .status
                    .success()
                    .then(|| String::from_utf8_lossy(&by_git.stdout).trim() == "true");
                let ours = WorkTreeSettings::read(fixture.path(), &file)
                    .and_then(|settings| settings.reads_per_worktree(fixture.path(), &file))
                    .ok();
                assert_eq!(ours, by_git, "{lines:?}");
            }
        }
    }

    /// `work` on a thread of its own, its answer arriving on the channel:
    /// a read blocked opening a FIFO never answers, so a test can fail
    /// past a limit rather than hang.
    #[cfg(unix)]
    fn answering(work: impl FnOnce() -> bool + Send + 'static) -> std::sync::mpsc::Receiver<bool> {
        let (answer, answered) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = answer.send(work());
        });
        answered
    }

    #[cfg(unix)]
    fn make_fifo(path: &Path) {
        let _ = std::fs::remove_file(path);
        assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
    }

    /// Lets go a reader waiting in `open` on the FIFO at `path`: a writer's
    /// open, which never waits itself.
    #[cfg(unix)]
    fn release_fifo(path: &Path) {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
            .open(path);
    }

    /// A FIFO at a file the identity reads never blocks it: its open waits
    /// for a writer with no deadline, on the one reader thread and at a
    /// check holding the identity's lock, where one at `config.worktree`
    /// hung an open and one at `commondir` a check for as long as it stayed.
    /// Only a regular file is opened; anything else is Git's to read, under
    /// its deadline (`commondir`, and `config.worktree` under its extension,
    /// are ones Git reads, so those checks take it; with no extension
    /// neither reads that file).
    #[cfg(unix)]
    #[test]
    fn a_fifo_in_a_git_directory_never_blocks_the_identity() {
        const LIMIT: Duration = Duration::from_secs(40);
        let fixture = tempfile::tempdir().unwrap();
        let init = |name: &str| {
            let root = fixture.path().join(name);
            std::fs::create_dir(&root).unwrap();
            git(&root, &["init", "--quiet"]);
            git(&root, &["commit", "--quiet", "--allow-empty", "-m", name]);
            root.canonicalize().unwrap()
        };
        let linked = |main: &Path, name: &str| {
            let path = fixture.path().join(name);
            git(
                main,
                &[
                    "worktree",
                    "add",
                    "--quiet",
                    "-b",
                    name,
                    path.to_str().unwrap(),
                ],
            );
            path.canonicalize().unwrap()
        };

        // Every Git command and open before the first FIFO: this test's
        // own `git` has no deadline, and `worktree add` reads the
        // `commondir` of the worktrees already there.
        let config_at_open = init("config-at-open");
        let config_after_open = init("config-after-open");
        let per_worktree = init("per-worktree");
        git(
            &per_worktree,
            &["config", "extensions.worktreeConfig", "true"],
        );
        let main = init("main");
        let common_at_open = linked(&main, "common-at-open");
        let common_after_open = linked(&main, "common-after-open");
        let config_repository = GitRepository::open(&config_after_open).unwrap();
        let per_worktree_repository = GitRepository::open(&per_worktree).unwrap();
        let common_repository = GitRepository::open(&common_after_open).unwrap();
        let private = GitRepository::open(&common_at_open)
            .unwrap()
            .git_dir()
            .to_owned();
        let fifos = [
            config_at_open.join(".git/config.worktree"),
            config_after_open.join(".git/config.worktree"),
            per_worktree.join(".git/config.worktree"),
            private.join("commondir"),
            common_repository.git_dir().join("commondir"),
        ];
        for fifo in &fifos {
            make_fifo(fifo);
        }

        let fifo = fifos[0].clone();
        let answers = [
            (
                "reading a FIFO at config.worktree",
                answering(move || matches!(may_name_a_work_tree(&fifo), Ok(false))),
            ),
            (
                "reading a FIFO at commondir",
                answering(move || matches!(stored_common_dir(&private), Ok(None))),
            ),
            (
                "an open with a FIFO at config.worktree",
                answering(move || GitRepository::open(&config_at_open).is_ok()),
            ),
            (
                "a check with a FIFO at config.worktree",
                answering(move || {
                    config_repository
                        .verify_at_path()
                        .is_ok_and(|found| found == FoundAtPath::Same)
                }),
            ),
            (
                "a check with a FIFO at a config.worktree Git reads",
                answering(move || per_worktree_repository.verify_at_path().is_err()),
            ),
            (
                "an open with a FIFO at commondir",
                answering(move || GitRepository::open(&common_at_open).is_err()),
            ),
            (
                "a check with a FIFO at commondir",
                answering(move || common_repository.verify_at_path().is_err()),
            ),
        ];

        let deadline = std::time::Instant::now() + LIMIT;
        let (mut blocked, mut wrong) = (Vec::new(), Vec::new());
        for (what, answer) in &answers {
            match answer.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(true) => {}
                Ok(false) => wrong.push(*what),
                Err(_) => blocked.push(*what),
            }
        }
        // Past the limit, whatever still waits is let go before the test
        // fails, so a regression ends here and holds no thread or process.
        if !blocked.is_empty() {
            for _ in 0..20 {
                for fifo in &fifos {
                    release_fifo(fifo);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        assert!(blocked.is_empty(), "blocked past {LIMIT:?}: {blocked:?}");
        assert!(wrong.is_empty(), "the wrong answer: {wrong:?}");
    }

    /// A file the identity reads that cannot be opened is no file that is
    /// not there: only nothing at the path, or no regular file, reads as no
    /// entry. Failing as a process out of file descriptors does, the check
    /// that follows a configuration edit errs and keeps what it confirmed,
    /// where it once stored no guard on the `commondir` and Git, whose own
    /// open succeeded, confirmed that. The next check asks again.
    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_opened_is_no_file_that_is_not_there() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        assert!(matches!(open_regular_file(&base.join("gone")), Ok(None)));
        let plain = base.join("plain");
        std::fs::write(&plain, "").unwrap();
        assert!(matches!(open_regular_file(&plain.join("under")), Ok(None)));
        let looped = base.join("looped");
        std::os::unix::fs::symlink(&looped, &looped).unwrap();
        assert!(open_regular_file(&looped).is_err());

        // A `.git` directory, so the failing open is the stored path's:
        // a gitfile's would send the check to Git before it got there.
        let own = committed_repository(&base, "own");
        let link = base.join("own-link");
        std::os::unix::fs::symlink(own.join(".git"), &link).unwrap();
        std::fs::write(own.join(".git/commondir"), format!("{}\n", link.display())).unwrap();
        let opened = GitRepository::open(&own).unwrap();
        let confirmed = guards(&opened);
        assert_eq!(confirmed.len(), 1, "the commondir");

        git(&own, &["config", "gitturtle.marker", "1"]);
        OPEN_REFUSED.set(true);
        let found = opened.verify_at_path();
        OPEN_REFUSED.set(false);
        let error = format!("{:#}", found.unwrap_err());
        assert!(error.contains("commondir"), "names the file: {error}");
        assert_eq!(guards(&opened), confirmed);

        assert_eq!(opened.verify_at_path().unwrap(), FoundAtPath::Same);
        assert_eq!(guards(&opened), confirmed);
    }

    /// A gitfile Git accepts is read as Git reads it, as a C string that
    /// ends at its first NUL: read whole it was no gitfile, and the handle
    /// had no ceiling and asked Git at every check for its whole life.
    #[cfg(unix)]
    #[test]
    fn a_gitfile_with_a_nul_opens_confined() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let main = committed_repository(&base, "main");
        let linked = base.join("linked");
        add_worktree(&main, "linked", &linked);
        let private = GitRepository::open(&linked).unwrap().git_dir().to_owned();
        let mut gitfile = format!("gitdir: {}", private.display()).into_bytes();
        gitfile.extend_from_slice(b"\0trailing junk");
        std::fs::write(linked.join(".git"), gitfile).unwrap();

        let opened = GitRepository::open(&linked).unwrap();
        assert_eq!(opened.git_dir(), private);
        assert!(opened.identity.confined);
        assert!(opened.identity.holds(opened.path(), false));
        for _ in 0..3 {
            let found = verify_changed_part_way(&opened, || panic!("the check asked Git"));
            assert_eq!(found.unwrap(), FoundAtPath::Same);
        }
    }

    /// A gitfile that cannot be read as the repository is captured refuses
    /// the open, though Git, asked a moment later, finds the repository:
    /// captured so, the handle could not be confined for its whole life.
    /// Opened again with the gitfile in place, it is confined. A FIFO at
    /// `.git` is as unread as one met at the open. Past the limit, whatever
    /// still waits is let go before the test fails.
    #[cfg(unix)]
    #[test]
    fn a_gitfile_unreadable_at_capture_refuses_the_open() {
        const LIMIT: Duration = Duration::from_secs(40);
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let main = committed_repository(&base, "main");
        let linked = base.join("linked");
        add_worktree(&main, "linked", &linked);
        let entry = linked.join(".git");
        let gitfile = std::fs::read(&entry).unwrap();
        let fifo = base.join("fifo");
        make_fifo(&fifo);

        let (root, swapped, to, aside, back) = (
            linked.clone(),
            fifo.clone(),
            entry.clone(),
            fifo.clone(),
            entry.clone(),
        );
        let answer = answering(move || {
            GITFILE_CHECKPOINT.with_borrow_mut(|hook| {
                *hook = Some(Box::new(move || {
                    let _ = std::fs::rename(&swapped, &to);
                }));
            });
            let opened = open_changed_part_way(&root, move || {
                std::fs::rename(&back, &aside).unwrap();
                std::fs::write(&back, &gitfile).unwrap();
            });
            GITFILE_CHECKPOINT.with_borrow_mut(|hook| *hook = None);
            let said = format!(
                "The repository at {} changed while it was opened. Open it again.",
                root.display()
            );
            opened.is_err_and(|error| {
                error.is::<ChangedWhileOpening>() && format!("{error:#}") == said
            })
        });
        let answered = answer.recv_timeout(LIMIT);
        if answered.is_err() {
            for _ in 0..20 {
                release_fifo(&entry);
                release_fifo(&fifo);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        assert_eq!(answered, Ok(true), "blocked past {LIMIT:?}, or opened it");

        let opened = GitRepository::open(&linked).unwrap();
        assert!(opened.identity.confined);

        std::fs::rename(&fifo, base.join("at-git-next")).unwrap();
        let holder = base.join("holder");
        std::fs::create_dir(&holder).unwrap();
        std::fs::rename(base.join("at-git-next"), holder.join(".git")).unwrap();
        assert!(matches!(read_git_entry(&holder), GitEntry::Unread));
    }

    /// A gitfile swapped for a FIFO between the look at its type and its
    /// open is no gitfile: the check opens it every time, on the one reader
    /// thread, and that exchange once held a check for as long as the FIFO
    /// stayed. Past the limit, whatever still waits is let go before the
    /// test fails.
    #[cfg(unix)]
    #[test]
    fn a_gitfile_swapped_for_a_fifo_never_blocks_its_reading() {
        const LIMIT: Duration = Duration::from_secs(40);
        let fixture = tempfile::tempdir().unwrap();
        let holder = fixture.path().join("holder");
        std::fs::create_dir(&holder).unwrap();
        let entry = holder.join(".git");
        std::fs::write(&entry, "gitdir: elsewhere\n").unwrap();
        let fifo = fixture.path().join("fifo");
        make_fifo(&fifo);

        let (from, to) = (fifo, entry.clone());
        let answer = answering(move || {
            GITFILE_CHECKPOINT.with_borrow_mut(|hook| {
                *hook = Some(Box::new(move || std::fs::rename(&from, &to).unwrap()));
            });
            resolve_git_entry(&holder).is_none()
        });
        let answered = answer.recv_timeout(LIMIT);
        if answered.is_err() {
            for _ in 0..20 {
                release_fifo(&entry);
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        assert_eq!(answered, Ok(true), "blocked past {LIMIT:?}, or read it");
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

    /// A repository in `folder` whose `core.worktree` is `work_tree`.
    fn repository_working_elsewhere(folder: &Path, work_tree: &Path) {
        std::fs::create_dir_all(folder).unwrap();
        git(folder, &["init", "--quiet"]);
        git(
            folder,
            &["config", "core.worktree", work_tree.to_str().unwrap()],
        );
    }

    fn outside_message(path: &Path, work_tree: &Path) -> String {
        format!(
            "GitTurtle opens a repository only inside the folder Git found it in: the work tree of the repository at {} is {} (core.worktree).",
            path.display(),
            work_tree.display()
        )
    }

    /// A layout Git reports the same both times, its work tree outside the
    /// folder Git found the repository in, is refused as what it is, naming
    /// both folders, with nothing to retry; it once said the repository
    /// "changed while it was opened" and "Open it again". Opened from a
    /// folder inside the repository, or with its work tree inside another
    /// repository, it is still the same refusal.
    #[test]
    fn a_work_tree_outside_its_folder_is_refused_in_its_own_words() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let repository = base.join("repository");
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        repository_working_elsewhere(&repository, &elsewhere);
        let inside = repository.join("inside");
        std::fs::create_dir(&inside).unwrap();
        for opened in [&repository, &inside] {
            let refused = GitRepository::open(opened).unwrap_err();
            assert!(!refused.is::<ChangedWhileOpening>(), "{refused:#}");
            let message = refused.to_string();
            assert_eq!(message, outside_message(&repository, &elsewhere));
            assert!(!message.contains("Open it again"), "{message}");
            let outside = refused.downcast::<WorkTreeOutside>().expect("its own");
            assert_eq!(outside.path, repository);
            assert_eq!(outside.work_tree, elsewhere);
        }

        // Inside another repository, whose own work tree Git finds there.
        let enclosing = base.join("enclosing");
        std::fs::create_dir(&enclosing).unwrap();
        git(&enclosing, &["init", "--quiet"]);
        let within = enclosing.join("within");
        std::fs::create_dir(&within).unwrap();
        let second = base.join("second");
        repository_working_elsewhere(&second, &within);
        let refused = GitRepository::open(&second).unwrap_err();
        assert_eq!(refused.to_string(), outside_message(&second, &within));

        // A Git directory opened as it is, with no `.git` entry: named itself.
        let plain = base.join("plain.git");
        std::fs::create_dir(&plain).unwrap();
        git(&plain, &["init", "--quiet", "--bare"]);
        git(&plain, &["config", "core.bare", "false"]);
        git(
            &plain,
            &["config", "core.worktree", elsewhere.to_str().unwrap()],
        );
        let refused = GitRepository::open(&plain).unwrap_err();
        assert_eq!(refused.to_string(), outside_message(&plain, &elsewhere));
    }

    /// A work tree outside its folder that Git reports differently the
    /// second time was re-pointed while it was opened: changed, as is a
    /// repository re-pointed there after an open. The handle already open
    /// is refused as changed, and only a new open meets the layout as it
    /// now stands.
    #[test]
    fn a_work_tree_re_pointed_outside_is_still_a_change() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let first = base.join("first");
        let second = base.join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let repository = base.join("repository");
        repository_working_elsewhere(&repository, &first);
        let (folder, moved) = (repository.clone(), second.clone());
        let refused = open_changed_part_way(&repository, move || {
            git(
                &folder,
                &["config", "core.worktree", moved.to_str().unwrap()],
            );
        })
        .unwrap_err();
        assert!(!refused.is::<WorkTreeOutside>(), "{refused:#}");
        let changed = refused
            .downcast::<ChangedWhileOpening>()
            .expect("refused as changed");
        assert_eq!(changed.path, first);

        let inside = base.join("inside");
        std::fs::create_dir(&inside).unwrap();
        git(&inside, &["init", "--quiet"]);
        let opened = GitRepository::open(&inside).unwrap();
        git(
            &inside,
            &["config", "core.worktree", second.to_str().unwrap()],
        );
        assert_eq!(
            opened.verify_at_path().unwrap(),
            FoundAtPath::Other(second.clone())
        );
        let refused = opened.ensure_found_at_path().unwrap_err().to_string();
        assert!(refused.contains("The repository changed"), "{refused}");
        assert!(refused.contains("Git was not started"), "{refused}");
        let reopened = GitRepository::open(&inside).unwrap_err();
        assert_eq!(reopened.to_string(), outside_message(&inside, &second));
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

    /// A repository opened at a path that reaches it through a link, a
    /// linked folder above it (as macOS reaches every temporary folder
    /// through `/var`) or a link to its own folder, is the repository at its
    /// real root, as Git reports it, confined there: its root, Git
    /// directory and ceiling are the real folders, its check from the
    /// filesystem and Git's both find it the same, starting no write, and
    /// its reads run.
    #[cfg(unix)]
    #[test]
    fn an_open_through_a_link_is_the_repository_at_its_real_root() {
        let temp = tempfile::TempDir::new().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let real = base.join("real");
        let root = real.join("repository");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet"]);
        git(
            &root,
            &["commit", "--quiet", "--allow-empty", "-m", "linked"],
        );
        std::os::unix::fs::symlink(&real, base.join("link")).unwrap();
        std::os::unix::fs::symlink(&root, base.join("repository-link")).unwrap();
        for at in [
            base.join("link").join("repository"),
            base.join("repository-link"),
        ] {
            let repo = GitRepository::open(&at).unwrap();
            let opened = at.display();
            assert_eq!(repo.path(), root, "{opened}");
            assert_eq!(repo.git_dir(), root.join(".git"), "{opened}");
            assert_eq!(repo.identity.root, root, "{opened}");
            assert_eq!(repo.identity.ceiling.as_deref(), Some(real.as_path()));
            assert!(repo.identity.confined, "{opened}");
            assert_eq!(repo.verify_at_path().unwrap(), FoundAtPath::Same);
            assert_eq!(repo.found_at_path().unwrap(), FoundAtPath::Same);
            assert!(repo.ensure_found_at_path().is_ok(), "{opened}");
            assert!(repo.status().unwrap().entries.is_empty(), "{opened}");
            assert_eq!(repo.branches().unwrap().len(), 1, "{opened}");
        }
    }

    /// `git <args>` failing with `stderr`, as [`run_git`] reports it.
    #[cfg(unix)]
    fn failed(args: &[&str], stderr: &[u8]) -> anyhow::Error {
        use std::os::unix::process::ExitStatusExt;
        let output = Output {
            status: std::process::ExitStatus::from_raw(128 << 8),
            stdout: Vec::new(),
            stderr: stderr.to_vec(),
        };
        checked_git_output(args, output).expect_err("a failed command")
    }

    /// A path with no repository is told apart in Git's own words, even
    /// where the mount point Git names holds a carriage return, which leaves
    /// only "b)" once read as a terminal leaves it; any other failure reports
    /// Git's output as a terminal leaves it.
    #[cfg(unix)]
    #[test]
    fn no_repository_is_found_in_git_s_own_words() {
        let boundary = b"fatal: not a git repository (or any parent up to mount point /mnt/a\rb)\nStopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).\n";
        assert!(no_repository_found(boundary));
        assert!(no_repository_found(
            b"fatal: not a git repository: '.git'\n"
        ));
        let failure = failed(
            &["rev-parse", "--absolute-git-dir"],
            b"Checking (1/2)\rChecking (2/2)\rfatal: bad object HEAD\n",
        );
        assert!(!failure.is::<NotARepository>(), "{failure}");
        assert_eq!(
            failure.to_string(),
            "Git rev-parse failed: fatal: bad object HEAD"
        );
        // As an open reports them: the folder with no repository by its type,
        // whatever its message keeps.
        let open = failed(&["rev-parse", "--is-bare-repository"], boundary);
        assert!(open.is::<NotARepository>(), "{open}");
        assert_eq!(
            open.to_string(),
            "Git rev-parse failed: b)\nStopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set)."
        );
        // Git names the path it could not enter, so a folder called "not a
        // git repository", or one inside it, whose access was denied is that
        // failure, never a folder with no repository.
        for denied in [
            b"fatal: cannot change to '/x/not a git repository': Permission denied\n".as_slice(),
            b"fatal: cannot change to '/x/not a git repository/inner': Permission denied\n",
        ] {
            assert!(!no_repository_found(denied));
            let error = failed(&["rev-parse"], denied);
            assert!(!error.is::<NotARepository>(), "{error}");
            assert!(error.to_string().ends_with("Permission denied"), "{error}");
        }
    }

    /// A read whose `.git` is gone, as every command of an opened repository
    /// fails once it is: where the path Git names holds a carriage return,
    /// its message keeps only what follows it, as it did, with none of Git's
    /// "not a git repository", and the read is still told apart as finding no
    /// repository, by the type its failure carries, through any context.
    /// Telling it apart by the message, the tab stayed unmarked.
    #[cfg(unix)]
    #[test]
    fn a_read_finding_no_repository_is_told_apart_whatever_its_message_keeps() {
        let stderr = b"fatal: not a git repository: /a\rb/.git/worktrees/x\n";
        let error = failed(&["rev-parse", "--absolute-git-dir"], stderr);
        // Shown as it was before the type: Git's words as a terminal leaves
        // them, after ours.
        let shown = "Git rev-parse failed: b/.git/worktrees/x";
        assert_eq!(error.to_string(), shown);
        assert_eq!(format!("{error:#}"), shown);
        assert!(error.is::<NotARepository>(), "{error}");
        assert!(finds_no_repository(&error), "{error:#}");
        let resolved = error.context("Resolve Git administration directory");
        assert_eq!(
            format!("{resolved:#}"),
            format!("Resolve Git administration directory: {shown}")
        );
        assert!(finds_no_repository(&resolved), "{resolved:#}");
        // Another failure carries no type, and its message alone decides.
        let other = failed(&["status"], b"fatal: index file corrupt\n");
        assert!(!finds_no_repository(&other), "{other:#}");
        assert!(finds_no_repository(&anyhow::anyhow!(
            "fatal: not a git repository (or any of the parent directories): .git"
        )));
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
