//! Bounded passive revision comparisons and tracked-path discovery.
use super::*;
use crate::case_fold::CaseInsensitiveMatcher;
use crate::history::{ReadEnd, stream_history};
use std::io::{Read, Seek, SeekFrom};

const MAX_INSPECTION_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRACKED_PATHS: usize = 100_000;
pub const MAX_PATH_MATCHES: usize = 500;

/// Zero-spawn fingerprint of a working copy's Git index (`.git/index`),
/// combining filesystem identity and the trailing Git index checksum so
/// cached Quick Open path tables invalidate immediately on any index change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexFingerprint {
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    ctime_sec: i64,
    #[cfg(unix)]
    ctime_nsec: i64,
    trailer: [u8; 32],
    trailer_len: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackedMode {
    Regular,
    Executable,
    Symlink,
    Submodule,
    Other(u32),
}

impl TrackedMode {
    fn parse(raw: &[u8]) -> Result<Self> {
        Ok(match raw {
            b"100644" => Self::Regular,
            b"100755" => Self::Executable,
            b"120000" => Self::Symlink,
            b"160000" => Self::Submodule,
            _ => {
                let text = std::str::from_utf8(raw)?;
                let octal = u32::from_str_radix(text, 8)
                    .with_context(|| format!("Invalid Git mode: {text}"))?;
                Self::Other(octal)
            }
        })
    }

    fn to_mode_string(self) -> String {
        match self {
            Self::Regular => "100644".to_owned(),
            Self::Executable => "100755".to_owned(),
            Self::Symlink => "120000".to_owned(),
            Self::Submodule => "160000".to_owned(),
            Self::Other(mode) => format!("{mode:06o}"),
        }
    }
}

#[derive(Clone, Debug)]
struct CompactTrackedPath {
    path_start: u32,
    path_len: u32,
    oid_start: u32,
    oid_len: u16,
    mode: TrackedMode,
    conflicted: bool,
}

/// Compact packed table of tracked repository paths for one pinned scope.
/// Stores all raw path bytes and hex OIDs in two contiguous arenas so repeated
/// Quick Open queries run in memory with zero subprocess spawns.
#[derive(Clone, Debug)]
pub struct TrackedPathIndex {
    scope: PathScope,
    paths_arena: Vec<u8>,
    oids_arena: Vec<u8>,
    entries: Vec<CompactTrackedPath>,
    total_scanned: usize,
    truncated: bool,
}

impl TrackedPathIndex {
    pub fn scope(&self) -> &PathScope {
        &self.scope
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn total_scanned(&self) -> usize {
        self.total_scanned
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Approximate heap footprint of this cached tracked-path table.
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.paths_arena.capacity()
            + self.oids_arena.capacity()
            + self.entries.capacity() * std::mem::size_of::<CompactTrackedPath>()
    }

    /// Filter the cached tracked-path table by literal case-insensitive
    /// substring, returning at most [`MAX_PATH_MATCHES`] matches.
    pub fn search(&self, query: &str, cancellation: &HistoryCancellation) -> Result<TrackedPaths> {
        ensure!(query.len() <= 4096, "Path query exceeds 4 KiB");
        cancellation.check()?;
        let matcher = CaseInsensitiveMatcher::new(query);
        let mut scratch = String::new();
        let mut entries = Vec::new();
        let mut hit_match_limit = false;
        for (idx, item) in self.entries.iter().enumerate() {
            if idx & 0x7FF == 0 {
                cancellation.check()?;
            }
            let p_start = item.path_start as usize;
            let p_end = p_start + item.path_len as usize;
            let raw_path = &self.paths_arena[p_start..p_end];
            if !matcher.matches_bytes(raw_path, &mut scratch) {
                continue;
            }
            if entries.len() == MAX_PATH_MATCHES {
                hit_match_limit = true;
                break;
            }
            let o_start = item.oid_start as usize;
            let o_end = o_start + usize::from(item.oid_len);
            let oid = std::str::from_utf8(&self.oids_arena[o_start..o_end])
                .context("Cached tracked OID is not UTF-8")?
                .to_owned();
            entries.push(TrackedPath {
                path: path_from_bytes(raw_path),
                oid,
                mode: item.mode.to_mode_string(),
                conflicted: item.conflicted,
            });
        }
        Ok(TrackedPaths {
            scope: self.scope.clone(),
            entries,
            total_scanned: self.total_scanned,
            truncated: self.truncated || hit_match_limit,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ComparisonMode {
    #[default]
    Endpoints,
    SinceBranching,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedRevision {
    pub expression: String,
    pub oid: String,
}

#[derive(Clone, Debug)]
pub struct RevisionComparison {
    pub before: ResolvedRevision,
    pub after: ResolvedRevision,
    pub base_oid: String,
    pub mode: ComparisonMode,
    pub files: Vec<FileChange>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PathScope {
    Worktree,
    Revision(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackedPath {
    pub path: PathBuf,
    pub oid: String,
    pub mode: String,
    pub conflicted: bool,
}

#[derive(Clone, Debug)]
pub struct TrackedPaths {
    pub scope: PathScope,
    pub entries: Vec<TrackedPath>,
    pub total_scanned: usize,
    pub truncated: bool,
}

impl GitRepository {
    /// Resolve one local commit expression. Git sees a single argument after
    /// --end-of-options; ranges, missing objects and noncommits are refused.
    pub fn resolve_inspection_revision(
        &self,
        expression: &str,
        cancellation: &HistoryCancellation,
    ) -> Result<ResolvedRevision> {
        let expression = expression.trim();
        ensure!(
            !expression.is_empty()
                && expression.len() <= 4096
                && !expression.contains(['\0', '\n', '\r']),
            "Enter one local commit revision (up to 4 KiB)"
        );
        // Git otherwise resolves a short branch/tag collision with only a
        // warning. Require the user to name the namespace in that case.
        if !expression.starts_with("refs/") && !expression.contains(['~', '^', ':', '@']) {
            let names = [
                format!("refs/heads/{expression}"),
                format!("refs/tags/{expression}"),
                format!("refs/remotes/{expression}"),
            ];
            let mut command = git_command(self.target());
            command.args(["for-each-ref", "--format=%(refname)", "--"]);
            command.args(&names);
            let refs = inspection_read(command, cancellation, MAX_INSPECTION_BYTES)?;
            let count = refs
                .split(|b| *b == b'\n')
                .filter(|line| names.iter().any(|name| name.as_bytes() == *line))
                .count();
            ensure!(
                count <= 1,
                "Ambiguous revision {expression}. Use refs/heads/… or refs/tags/… to select the intended target"
            );
        }
        let mut command = git_command(self.target());
        command
            .args(["rev-parse", "--verify", "--end-of-options"])
            .arg(format!("{expression}^{{commit}}"));
        let bytes = inspection_read(command, cancellation, 1024)
            .with_context(|| format!("Local commit revision {expression} is unavailable; browsing never downloads objects"))?;
        let oid = std::str::from_utf8(trim_line(&bytes))?.to_owned();
        validate_oid(&oid)?;
        Ok(ResolvedRevision {
            expression: expression.to_owned(),
            oid,
        })
    }

    pub fn compare_revisions(
        &self,
        before: &str,
        after: &str,
        mode: ComparisonMode,
        cancellation: &HistoryCancellation,
    ) -> Result<RevisionComparison> {
        let before = self.resolve_inspection_revision(before, cancellation)?;
        let after = self.resolve_inspection_revision(after, cancellation)?;
        let base_oid = if mode == ComparisonMode::SinceBranching {
            let mut command = git_command(self.target());
            command.args(["merge-base", "--all", &before.oid, &after.oid]);
            let bases = inspection_read(command, cancellation, 64 * 1024)
                .context("Cannot establish a common ancestor. Histories may be unrelated or required local objects unavailable; endpoint comparison remains available")?;
            let bases: Vec<_> = std::str::from_utf8(&bases)?.lines().collect();
            ensure!(
                bases.len() == 1,
                "These revisions have multiple equally valid merge bases. Changes since branching is ambiguous; use endpoint comparison"
            );
            validate_oid(bases[0])?;
            bases[0].to_owned()
        } else {
            before.oid.clone()
        };
        let mut command = git_command(self.target());
        command.args([
            "diff-tree",
            "--no-commit-id",
            "--raw",
            "--no-abbrev",
            "-z",
            "-r",
            "--no-ext-diff",
            "--no-textconv",
            "--find-renames=50%",
            "-l1000",
            &base_oid,
            &after.oid,
            "--",
        ]);
        let bytes = inspection_read(command, cancellation, MAX_INSPECTION_BYTES)?;
        let files = parse_changes(&bytes)?;
        Ok(RevisionComparison {
            before,
            after,
            base_oid,
            mode,
            files,
        })
    }

    /// Zero-spawn fingerprint of `.git/index` for validating cached worktree
    /// path tables. Returns `Ok(None)` for bare repositories or unborn
    /// repositories that do not yet have an index file.
    pub fn index_fingerprint(&self) -> Result<Option<IndexFingerprint>> {
        if self.bare {
            return Ok(None);
        }
        let git_dir = if matches!(self.verify_at_path()?, FoundAtPath::Same) {
            self.git_dir().to_path_buf()
        } else {
            self.git_directories()?.0
        };
        let index_path = git_dir.join("index");
        #[cfg(unix)]
        let mut file = {
            use rustix::fs::{Mode, OFlags, open};
            match open(
                &index_path,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                Mode::empty(),
            ) {
                Ok(fd) => std::fs::File::from(fd),
                Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR) => return Ok(None),
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("Unable to open Git index {}", index_path.display())
                    });
                }
            }
        };
        #[cfg(not(unix))]
        let mut file = match std::fs::File::open(&index_path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("Unable to open Git index {}", index_path.display()));
            }
        };
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file(),
            "Git index is not a regular file: {}",
            index_path.display()
        );
        let len = metadata.len();
        let trailer_len = usize::try_from(len.min(32)).unwrap_or(32);
        let mut trailer = [0u8; 32];
        if trailer_len > 0 {
            let offset = i64::try_from(trailer_len).unwrap_or(32);
            file.seek(SeekFrom::End(-offset))?;
            file.read_exact(&mut trailer[..trailer_len])?;
        }
        #[cfg(unix)]
        let (dev, ino, ctime_sec, ctime_nsec) = {
            use std::os::unix::fs::MetadataExt;
            (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            )
        };
        Ok(Some(IndexFingerprint {
            len,
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            dev,
            #[cfg(unix)]
            ino,
            #[cfg(unix)]
            ctime_sec,
            #[cfg(unix)]
            ctime_nsec,
            trailer,
            trailer_len: u8::try_from(trailer_len).unwrap_or(32),
        }))
    }

    /// Loads up to 100,000 tracked paths / 16 MiB into a compact packed index
    /// for repeated zero-spawn Quick Open filtering.
    pub fn load_tracked_paths(
        &self,
        scope: &PathScope,
        cancellation: &HistoryCancellation,
    ) -> Result<TrackedPathIndex> {
        let mut command = git_command(self.target());
        let pinned_scope = match scope {
            PathScope::Worktree => {
                ensure!(
                    !self.bare,
                    "A bare repository has no working files; choose a revision"
                );
                command.args(["ls-files", "--stage", "-z", "--"]);
                PathScope::Worktree
            }
            PathScope::Revision(expression) => {
                let revision = self.resolve_inspection_revision(expression, cancellation)?;
                command.args(["ls-tree", "-r", "-z", "--full-tree", &revision.oid, "--"]);
                PathScope::Revision(revision.oid)
            }
        };
        let mut pending = Vec::new();
        let mut bytes = 0usize;
        let mut paths_arena = Vec::new();
        let mut oids_arena = Vec::new();
        let mut entries: Vec<CompactTrackedPath> = Vec::new();
        let mut total_scanned = 0usize;
        let mut truncated = false;
        let is_worktree = matches!(pinned_scope, PathScope::Worktree);

        let end = stream_history(command, None, cancellation, GIT_TIMEOUT, |chunk| {
            bytes += chunk.len();
            if bytes > MAX_INSPECTION_BYTES {
                truncated = true;
                return Ok(false);
            }
            pending.extend_from_slice(chunk);
            let mut consumed = 0;
            while let Some(n) = pending[consumed..].iter().position(|b| *b == 0) {
                let end = consumed + n;
                let record = &pending[consumed..end];
                consumed = end + 1;
                if total_scanned >= MAX_TRACKED_PATHS {
                    truncated = true;
                    return Ok(false);
                }
                total_scanned += 1;
                let tab = record
                    .iter()
                    .position(|b| *b == b'\t')
                    .context("Malformed tracked path")?;
                let (f0, f1, f2) = split_three_fields(&record[..tab])
                    .context("Malformed tracked path metadata")?;
                let raw_path = &record[tab + 1..];
                if let Some(last) = entries.last() {
                    let prev = &paths_arena[last.path_start as usize
                        ..last.path_start as usize + last.path_len as usize];
                    if prev == raw_path {
                        continue;
                    }
                }
                let (oid_bytes, conflicted) = if is_worktree {
                    (f1, f2 != b"0")
                } else {
                    (f2, false)
                };
                let _ = std::str::from_utf8(oid_bytes)?;
                let mode = TrackedMode::parse(f0)?;
                let path_start =
                    u32::try_from(paths_arena.len()).context("Tracked path arena overflow")?;
                let path_len =
                    u32::try_from(raw_path.len()).context("Tracked path length overflow")?;
                let oid_start =
                    u32::try_from(oids_arena.len()).context("Tracked OID arena overflow")?;
                let oid_len =
                    u16::try_from(oid_bytes.len()).context("Tracked OID length overflow")?;
                paths_arena.extend_from_slice(raw_path);
                oids_arena.extend_from_slice(oid_bytes);
                entries.push(CompactTrackedPath {
                    path_start,
                    path_len,
                    oid_start,
                    oid_len,
                    mode,
                    conflicted,
                });
            }
            if consumed > 0 {
                pending.drain(..consumed);
            }
            Ok(true)
        })?;
        if end != ReadEnd::Complete {
            truncated = true;
        }
        paths_arena.shrink_to_fit();
        oids_arena.shrink_to_fit();
        entries.shrink_to_fit();
        Ok(TrackedPathIndex {
            scope: pinned_scope,
            paths_arena,
            oids_arena,
            entries,
            total_scanned,
            truncated,
        })
    }

    /// Search at most 100,000 tracked entries / 16 MiB. The returned revision
    /// scope is pinned. Worktree paths include tracked deletions and conflicts.
    pub fn search_tracked_paths(
        &self,
        scope: &PathScope,
        query: &str,
        cancellation: &HistoryCancellation,
    ) -> Result<TrackedPaths> {
        ensure!(query.len() <= 4096, "Path query exceeds 4 KiB");
        let mut command = git_command(self.target());
        let scope = match scope {
            PathScope::Worktree => {
                ensure!(
                    !self.bare,
                    "A bare repository has no working files; choose a revision"
                );
                command.args(["ls-files", "--stage", "-z", "--"]);
                PathScope::Worktree
            }
            PathScope::Revision(expression) => {
                let revision = self.resolve_inspection_revision(expression, cancellation)?;
                command.args(["ls-tree", "-r", "-z", "--full-tree", &revision.oid, "--"]);
                PathScope::Revision(revision.oid)
            }
        };
        let matcher = CaseInsensitiveMatcher::new(query);
        let mut scratch = String::new();
        let mut pending = Vec::new();
        let mut bytes = 0usize;
        let mut result = TrackedPaths {
            scope,
            entries: Vec::new(),
            total_scanned: 0,
            truncated: false,
        };
        let end = stream_history(command, None, cancellation, GIT_TIMEOUT, |chunk| {
            bytes += chunk.len();
            if bytes > MAX_INSPECTION_BYTES {
                result.truncated = true;
                return Ok(false);
            }
            pending.extend_from_slice(chunk);
            let mut consumed = 0;
            while let Some(n) = pending[consumed..].iter().position(|b| *b == 0) {
                let end = consumed + n;
                let record = &pending[consumed..end];
                consumed = end + 1;
                if result.total_scanned >= MAX_TRACKED_PATHS {
                    result.truncated = true;
                    return Ok(false);
                }
                result.total_scanned += 1;
                let tab = record
                    .iter()
                    .position(|b| *b == b'\t')
                    .context("Malformed tracked path")?;
                let (f0, f1, f2) = split_three_fields(&record[..tab])
                    .context("Malformed tracked path metadata")?;
                let raw_path = &record[tab + 1..];
                if !matcher.matches_bytes(raw_path, &mut scratch) {
                    continue;
                }
                let path = path_from_bytes(raw_path);
                let (oid, conflicted) = match result.scope {
                    PathScope::Worktree => (f1, f2 != b"0"),
                    PathScope::Revision(_) => (f2, false),
                };
                if result
                    .entries
                    .last()
                    .is_some_and(|entry| entry.path == path)
                {
                    continue;
                }
                if result.entries.len() == MAX_PATH_MATCHES {
                    result.truncated = true;
                    return Ok(false);
                }
                result.entries.push(TrackedPath {
                    path,
                    oid: std::str::from_utf8(oid)?.to_owned(),
                    mode: std::str::from_utf8(f0)?.to_owned(),
                    conflicted,
                });
            }
            if consumed > 0 {
                pending.drain(..consumed);
            }
            Ok(true)
        })?;
        if end != ReadEnd::Complete {
            result.truncated = true;
        }
        Ok(result)
    }

    pub fn tracked_path_change(&self, entry: &TrackedPath) -> FileChange {
        FileChange {
            old_path: None,
            new_path: Some(entry.path.clone()),
            old_oid: None,
            new_oid: Some(entry.oid.clone()),
            status: ChangeStatus::Added,
            old_mode: "000000".into(),
            new_mode: entry.mode.clone(),
        }
    }

    /// Raw descriptor-relative read; symbolic links expose their stored target.
    pub fn read_tracked_working_file(&self, entry: &TrackedPath) -> Result<(String, Vec<u8>)> {
        ensure!(
            !entry.conflicted,
            "This file has unresolved conflicts; open it in Working Changes"
        );
        crate::work::read_worktree_file(&self.path, &entry.path, &entry.mode)
    }
}

fn split_three_fields(meta: &[u8]) -> Option<(&[u8], &[u8], &[u8])> {
    let mut parts = meta.split(|&b| b == b' ');
    let f0 = parts.next()?;
    let f1 = parts.next()?;
    let f2 = parts.next()?;
    parts.next().is_none().then_some((f0, f1, f2))
}

fn inspection_read(
    command: Command,
    cancellation: &HistoryCancellation,
    limit: usize,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let end = stream_history(command, None, cancellation, GIT_TIMEOUT, |chunk| {
        ensure!(
            bytes.len().saturating_add(chunk.len()) <= limit,
            "Inspection exceeds its bounded output limit; narrow the revision or path scope"
        );
        bytes.extend_from_slice(chunk);
        Ok(true)
    })?;
    ensure!(
        end == ReadEnd::Complete,
        "Inspection exceeded its time limit"
    );
    Ok(bytes)
}
