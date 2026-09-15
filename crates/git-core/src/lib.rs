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

#[derive(Clone)]
pub struct GitRepository {
    path: PathBuf,
    bare: bool,
    batch: Arc<Mutex<Option<BatchReader>>>,
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
        let bare = run_git(requested, &["rev-parse", "--is-bare-repository"])?;
        let bare = trim_line(&bare) == b"true";
        let root = run_git(
            requested,
            &[
                "rev-parse",
                if bare {
                    "--absolute-git-dir"
                } else {
                    "--show-toplevel"
                },
            ],
        )?;
        let path = path_from_bytes(trim_line(&root));
        ensure!(
            path.is_absolute(),
            "Git returned a non-absolute repository path"
        );
        Ok(Self {
            path,
            bare,
            batch: Arc::new(Mutex::new(None)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
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

    /// Resolve actual administration directories for local notifications.
    /// Linked worktrees have a private HEAD/index but share refs and objects.
    /// These passive reads never create or update Git metadata.
    pub fn git_directories(&self) -> Result<(PathBuf, PathBuf)> {
        let private = path_from_bytes(trim_line(&run_git(
            &self.path,
            &["rev-parse", "--absolute-git-dir"],
        )?));
        let common = path_from_bytes(trim_line(&run_git(
            &self.path,
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

    pub fn branches(&self) -> Result<Vec<Branch>> {
        let bytes = run_git(
            &self.path,
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
        let bytes = run_git(&self.path, &["worktree", "list", "--porcelain", "-z"])?;
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
            &self.path,
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
        parse_changes(&run_git(&self.path, &args)?)
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
            *guard = Some(BatchReader::spawn(&self.path)?);
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
            *guard = Some(BatchReader::spawn(&self.path)?);
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
            &self.path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let common = path_from_bytes(trim_line(&common));
        let configured = work::lfs_storage_config(&self.path)?;
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
                "Text preview is limited to 2 MiB and 100,000 lines per side.\nBefore: {old_bytes} bytes\nAfter: {new_bytes} bytes\n"
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

fn git_command(path: &Path) -> Command {
    let mut cmd = Command::new("git");
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
        .arg(path)
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

fn run_git(path: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = run_git_output(path, args)?;
    ensure!(
        output.status.success(),
        "Git {} failed: {}",
        args.first().unwrap_or(&"command"),
        text(&output.stderr).trim()
    );
    Ok(output.stdout)
}

fn run_git_output(path: &Path, args: &[&str]) -> Result<Output> {
    let mut command = git_command(path);
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
    fn spawn(path: &Path) -> Result<Self> {
        let mut command = git_command(path);
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
