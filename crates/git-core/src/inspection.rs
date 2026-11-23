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
    pub after_working: bool,
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
            after_working: false,
            files,
        })
    }

    /// Passive read comparing one local commit revision against the current working tree.
    pub fn compare_revision_with_working(
        &self,
        before: &str,
        cancellation: &HistoryCancellation,
    ) -> Result<RevisionComparison> {
        ensure!(
            !self.bare,
            "Bare repositories do not have a working tree to compare against."
        );
        let before = self.resolve_inspection_revision(before, cancellation)?;
        // Passive diff against the working tree must disable repo-local
        // clean/process filters and avoid descending into dirty submodules.
        let mut command = crate::work::passive_status_command(self.target())?;
        command.args([
            "diff",
            "--raw",
            "--no-abbrev",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=dirty",
            "--find-renames=50%",
            "-l1000",
            &before.oid,
            "--",
        ]);
        let bytes = inspection_read(command, cancellation, MAX_INSPECTION_BYTES)?;
        let files = parse_changes(&bytes)?;
        let base_oid = before.oid.clone();
        Ok(RevisionComparison {
            before,
            after: ResolvedRevision {
                expression: "Working tree".into(),
                oid: String::new(),
            },
            base_oid,
            mode: ComparisonMode::Endpoints,
            after_working: true,
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

    /// Resolve a user-entered commit hash, branch, tag, or revision expression
    /// (for example `HEAD~3`, `v1.0`, or a short SHA) to a full commit OID.
    /// Passive read only; rejects flags and control characters.
    pub fn resolve_revision(&self, spec: &str) -> Result<String> {
        let trimmed = spec.trim();
        ensure!(
            !trimmed.is_empty()
                && trimmed.len() <= 4096
                && !trimmed.starts_with('-')
                && !trimmed.contains(['\0', '\r', '\n']),
            "Enter a commit hash, branch name, tag, or revision expression."
        );
        let bytes = run_git(
            self.target(),
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{trimmed}^{{commit}}"),
            ],
        )
        .with_context(|| format!("No commit matches '{trimmed}'"))?;
        let oid = text(trim_line(&bytes));
        validate_oid(&oid)?;
        Ok(oid)
    }

    /// Format one commit as a bounded plain-text patch suitable for clipboard
    /// copy. Passive read only; never invokes external diff drivers or textconv.
    pub fn commit_patch(&self, oid: &str, cancellation: &HistoryCancellation) -> Result<String> {
        const MAX_COPY_PATCH_BYTES: usize = 4 * 1024 * 1024;
        validate_oid(oid)?;
        cancellation.check()?;
        let commit = self.recovery_commit(oid)?;
        let mut command = git_command(self.target());
        if commit.parents.len() > 1 {
            command.args([
                "diff-tree",
                "-m",
                "--first-parent",
                "--patch",
                "--no-notes",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                oid,
            ]);
        } else {
            command.args([
                "format-patch",
                "--stdout",
                "-1",
                "--no-stat",
                "--no-signature",
                "--no-notes",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                oid,
            ]);
        }
        let bytes = inspection_read(command, cancellation, MAX_COPY_PATCH_BYTES)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Format a bounded commit range (`base..head`) as plain-text patches.
    /// Passive read only; disables external diff drivers, textconv, and notes.
    pub fn format_patch_range(
        &self,
        base_oid: &str,
        head_oid: &str,
        cancellation: &HistoryCancellation,
    ) -> Result<String> {
        const MAX_RANGE_PATCH_BYTES: usize = 8 * 1024 * 1024;
        validate_oid(base_oid)?;
        validate_oid(head_oid)?;
        cancellation.check()?;
        let mut command = git_command(self.target());
        command.args([
            "format-patch",
            "--stdout",
            "--no-stat",
            "--no-signature",
            "--no-notes",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            &format!("{base_oid}..{head_oid}"),
        ]);
        let bytes = inspection_read(command, cancellation, MAX_RANGE_PATCH_BYTES)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Passively inspect and locally verify a commit's OpenPGP / SSH / X.509
    /// signature strictly against the local keyring or global/system SSH
    /// `allowedSignersFile`.
    ///
    /// Security invariants:
    /// - Returns `Ok(None)` immediately with zero subprocesses for unsigned commits.
    /// - Never runs `git verify-commit` against repository-local `.git/config`
    ///   (untrusted repos cannot override `gpg.program` or `allowedSignersFile`).
    /// - Disables OpenPGP network key retrieval (`--auto-key-locate clear,local`,
    ///   `--no-auto-key-import`, `--keyserver-options no-auto-key-retrieve`).
    /// - Strips Unicode bidi override controls from untrusted signer metadata.
    pub fn commit_signature(
        &self,
        oid: &str,
        cancellation: &HistoryCancellation,
    ) -> Result<Option<CommitSignature>> {
        validate_oid(oid)?;
        cancellation.check()?;
        let raw = self.read_object(oid, "commit")?;
        let (payload, signature) = match extract_commit_signature(&raw) {
            ExtractedSignature::None => return Ok(None),
            ExtractedSignature::Oversized => {
                return Ok(Some(CommitSignature {
                    format: SignatureFormat::OpenPgp,
                    trust: SignatureTrust::Unverified,
                    signer: None,
                    key_id: None,
                    detail: "Commit signature exceeds the 64 KiB offline verification limit."
                        .into(),
                }));
            }
            ExtractedSignature::Multiple => {
                return Ok(Some(CommitSignature {
                    format: SignatureFormat::OpenPgp,
                    trust: SignatureTrust::Unverified,
                    signer: None,
                    key_id: None,
                    detail:
                        "Commit contains multiple signature headers; offline verification skipped."
                            .into(),
                }));
            }
            ExtractedSignature::InvalidUtf8 => {
                return Ok(Some(CommitSignature {
                    format: SignatureFormat::OpenPgp,
                    trust: SignatureTrust::Unverified,
                    signer: None,
                    key_id: None,
                    detail: "Commit signature header is not valid UTF-8.".into(),
                }));
            }
            ExtractedSignature::Present { payload, signature } => (payload, signature),
        };
        cancellation.check()?;
        let has_header = |marker: &str| signature.lines().any(|line| line.starts_with(marker));
        let format = if has_header("-----BEGIN PGP SIGNATURE-----") {
            SignatureFormat::OpenPgp
        } else if has_header("-----BEGIN SSH SIGNATURE-----") {
            SignatureFormat::Ssh
        } else if has_header("-----BEGIN SIGNED MESSAGE-----")
            || has_header("-----BEGIN PKCS7-----")
        {
            SignatureFormat::X509
        } else {
            return Ok(Some(CommitSignature {
                format: SignatureFormat::OpenPgp,
                trust: SignatureTrust::Unverified,
                signer: None,
                key_id: None,
                detail: "Unrecognized commit signature header.".into(),
            }));
        };
        let verified = match format {
            SignatureFormat::OpenPgp => verify_openpgp_local(&payload, &signature, cancellation),
            SignatureFormat::Ssh => verify_ssh_local(&payload, &signature, cancellation),
            SignatureFormat::X509 => Ok(CommitSignature {
                format: SignatureFormat::X509,
                trust: SignatureTrust::Unverified,
                signer: None,
                key_id: None,
                detail: "Signed with X.509 (offline local verification not enabled).".into(),
            }),
        }?;
        Ok(Some(CommitSignature {
            format: verified.format,
            trust: verified.trust,
            signer: verified
                .signer
                .map(|s| strip_bidi_controls(&s).into_owned()),
            key_id: verified
                .key_id
                .map(|s| strip_bidi_controls(&s).into_owned()),
            detail: strip_bidi_controls(&verified.detail).into_owned(),
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureFormat {
    OpenPgp,
    Ssh,
    X509,
}

impl SignatureFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenPgp => "GPG",
            Self::Ssh => "SSH",
            Self::X509 => "X.509",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureTrust {
    Good,
    GoodUntrusted,
    ExpiredSignature,
    ExpiredKey,
    RevokedKey,
    Bad,
    MissingKey,
    Unverified,
}

impl SignatureTrust {
    pub const fn badge_label(self) -> &'static str {
        match self {
            Self::Good => "Verified",
            Self::GoodUntrusted => "Signed (untrusted key)",
            Self::ExpiredSignature => "Expired signature",
            Self::ExpiredKey => "Expired key",
            Self::RevokedKey => "Revoked key",
            Self::Bad => "Invalid signature",
            Self::MissingKey => "Signed (key not in local keyring)",
            Self::Unverified => "Signed (unverified)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitSignature {
    pub format: SignatureFormat,
    pub trust: SignatureTrust,
    pub signer: Option<String>,
    pub key_id: Option<String>,
    pub detail: String,
}

enum ExtractedSignature {
    None,
    Oversized,
    Multiple,
    InvalidUtf8,
    Present { payload: Vec<u8>, signature: String },
}

/// Split raw commit object bytes into `(signed_payload, signature_text)` by
/// removing the `gpgsig` or `gpgsig-sha256` multiline header block.
fn extract_commit_signature(raw: &[u8]) -> ExtractedSignature {
    const MAX_SIG_BYTES: usize = 64 * 1024;
    let header_end = raw
        .windows(2)
        .position(|w| w == b"\n\n")
        .map(|i| i + 1)
        .unwrap_or(raw.len());
    let headers = &raw[..header_end];
    let body = &raw[header_end..];

    let mut payload = Vec::with_capacity(raw.len());
    let mut sig_lines: Vec<u8> = Vec::new();
    let mut in_sig = false;
    let mut found = false;

    for line in headers.split_inclusive(|b| *b == b'\n') {
        if in_sig {
            if let Some(rest) = line.strip_prefix(b" ") {
                if sig_lines.len().saturating_add(rest.len()) > MAX_SIG_BYTES {
                    return ExtractedSignature::Oversized;
                }
                sig_lines.extend_from_slice(rest);
                continue;
            }
            in_sig = false;
        }
        if line.starts_with(b"gpgsig ") || line.starts_with(b"gpgsig-sha256 ") {
            if found {
                return ExtractedSignature::Multiple;
            }
            if let Some(space) = line.iter().position(|b| *b == b' ') {
                found = true;
                in_sig = true;
                let first = &line[space + 1..];
                if first.len() > MAX_SIG_BYTES {
                    return ExtractedSignature::Oversized;
                }
                sig_lines.extend_from_slice(first);
                continue;
            }
        }
        payload.extend_from_slice(line);
    }
    if !found || sig_lines.is_empty() {
        return ExtractedSignature::None;
    }
    payload.extend_from_slice(body);
    match String::from_utf8(sig_lines) {
        Ok(signature) => ExtractedSignature::Present { payload, signature },
        Err(_) => ExtractedSignature::InvalidUtf8,
    }
}

fn resolve_gpg_binary() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        for candidate in [
            "/opt/homebrew/bin/gpg",
            "/usr/local/bin/gpg",
            "/usr/local/MacGPG2/bin/gpg",
        ] {
            let p = Path::new(candidate);
            if p.is_file() {
                return p.to_path_buf();
            }
        }
    }
    PathBuf::from("gpg")
}

fn verify_openpgp_local(
    payload: &[u8],
    signature: &str,
    cancellation: &HistoryCancellation,
) -> Result<CommitSignature> {
    cancellation.check()?;
    let temp = create_private_sig_dir()?;
    let sig_path = temp.path().join("commit.sig");
    std::fs::write(&sig_path, signature.as_bytes())?;
    let mut cmd = Command::new(resolve_gpg_binary());
    cmd.args([
        "--batch",
        "--no-tty",
        "--no-auto-check-trustdb",
        "--status-fd=1",
        "--auto-key-locate",
        "clear,local",
        "--no-auto-key-import",
        "--keyserver-options",
        "no-auto-key-retrieve",
        "--verify",
    ])
    .arg(&sig_path)
    .arg("-")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let Ok(output) = work::bounded_write_output_cancellable(
        cmd,
        Some(payload.to_vec()),
        Duration::from_secs(5),
        cancellation,
    ) else {
        cancellation.check()?;
        return Ok(CommitSignature {
            format: SignatureFormat::OpenPgp,
            trust: SignatureTrust::Unverified,
            signer: None,
            key_id: None,
            detail: "Signed with GPG (local `gpg` tool unavailable or timed out).".into(),
        });
    };
    let status_text = String::from_utf8_lossy(&output.stdout);
    let mut trust = SignatureTrust::Unverified;
    let mut signer: Option<String> = None;
    let mut key_id: Option<String> = None;
    let mut has_good_sig = false;
    let mut has_valid_sig = false;
    let mut has_trusted_key = false;

    for line in status_text.lines() {
        let Some(rest) = line.strip_prefix("[GNUPG:] ") else {
            continue;
        };
        let mut parts = rest.splitn(3, ' ');
        let tag = parts.next().unwrap_or("");
        let arg1 = parts.next().unwrap_or("");
        let arg2 = parts.next().unwrap_or("");
        match tag {
            "GOODSIG" => {
                has_good_sig = true;
                if !arg1.is_empty() {
                    key_id = Some(arg1.to_owned());
                }
                if !arg2.trim().is_empty() {
                    signer = Some(arg2.trim().to_owned());
                }
            }
            "VALIDSIG" => {
                let fields: Vec<&str> = rest.split_whitespace().collect();
                // GnuPG DETAILS: VALIDSIG <fpr> <date> <ts> <exp> <ver> <rsv> <pk> <hash> <sig-class> [primary-fpr]
                // Detached document signatures have sig-class 00 (binary) or 01 (canonical text).
                if fields
                    .get(9)
                    .is_some_and(|sig_class| matches!(*sig_class, "00" | "01"))
                {
                    has_valid_sig = true;
                }
                if !arg1.is_empty() {
                    key_id = Some(arg1.to_owned());
                }
            }
            "TRUST_ULTIMATE" | "TRUST_FULLY" => {
                has_trusted_key = true;
            }
            "EXPSIG" => {
                trust = SignatureTrust::ExpiredSignature;
                if !arg1.is_empty() {
                    key_id = Some(arg1.to_owned());
                }
                if !arg2.trim().is_empty() {
                    signer = Some(arg2.trim().to_owned());
                }
            }
            "EXPKEYSIG" => {
                trust = SignatureTrust::ExpiredKey;
                if !arg1.is_empty() {
                    key_id = Some(arg1.to_owned());
                }
                if !arg2.trim().is_empty() {
                    signer = Some(arg2.trim().to_owned());
                }
            }
            "REVKEYSIG" => {
                trust = SignatureTrust::RevokedKey;
                if !arg1.is_empty() {
                    key_id = Some(arg1.to_owned());
                }
                if !arg2.trim().is_empty() {
                    signer = Some(arg2.trim().to_owned());
                }
            }
            "BADSIG" => {
                trust = SignatureTrust::Bad;
                if !arg1.is_empty() {
                    key_id = Some(arg1.to_owned());
                }
            }
            "NO_PUBKEY" => {
                trust = SignatureTrust::MissingKey;
                if !arg1.is_empty() {
                    key_id = Some(arg1.to_owned());
                }
            }
            _ => {}
        }
    }
    if output.status.success()
        && has_good_sig
        && has_valid_sig
        && matches!(trust, SignatureTrust::Unverified)
    {
        trust = if has_trusted_key {
            SignatureTrust::Good
        } else {
            SignatureTrust::GoodUntrusted
        };
    }
    let detail = match trust {
        SignatureTrust::Good => "Good GPG signature verified against your local keyring.".into(),
        SignatureTrust::GoodUntrusted => {
            "Good GPG signature, but the key is not marked trusted in your local keyring.".into()
        }
        SignatureTrust::MissingKey => {
            "Commit is GPG-signed, but the public key is not in your local keyring (offline check)."
                .into()
        }
        SignatureTrust::Bad => "GPG signature failed verification.".into(),
        SignatureTrust::ExpiredSignature => "GPG signature has expired.".into(),
        SignatureTrust::ExpiredKey => "Signing GPG key has expired.".into(),
        SignatureTrust::RevokedKey => "Signing GPG key has been revoked.".into(),
        SignatureTrust::Unverified => {
            "Commit is GPG-signed; local verification did not complete.".into()
        }
    };
    Ok(CommitSignature {
        format: SignatureFormat::OpenPgp,
        trust,
        signer,
        key_id,
        detail,
    })
}

fn verify_ssh_local(
    payload: &[u8],
    signature: &str,
    cancellation: &HistoryCancellation,
) -> Result<CommitSignature> {
    cancellation.check()?;
    let temp = create_private_sig_dir()?;
    // Read `gpg.ssh.allowedSignersFile` strictly from global/system Git config,
    // never repository-local `.git/config`.
    let allowed_path = read_global_ssh_allowed_signers(temp.path(), cancellation)?;
    let Some(allowed_path) = allowed_path else {
        return Ok(CommitSignature {
            format: SignatureFormat::Ssh,
            trust: SignatureTrust::Unverified,
            signer: None,
            key_id: None,
            detail: "Signed with SSH (no global gpg.ssh.allowedSignersFile configured).".into(),
        });
    };
    // Follow symlinks for the user's own global allowedSignersFile (supporting
    // dotfile managers), while still requiring a regular file (refusing FIFOs/dirs).
    let Ok(meta) = std::fs::metadata(&allowed_path) else {
        return Ok(CommitSignature {
            format: SignatureFormat::Ssh,
            trust: SignatureTrust::MissingKey,
            signer: None,
            key_id: None,
            detail: "Signed with SSH (configured allowedSignersFile was not found).".into(),
        });
    };
    if !meta.file_type().is_file() {
        return Ok(CommitSignature {
            format: SignatureFormat::Ssh,
            trust: SignatureTrust::Unverified,
            signer: None,
            key_id: None,
            detail: "Signed with SSH (allowedSignersFile must be a regular file).".into(),
        });
    }
    let sig_path = temp.path().join("commit.sig");
    std::fs::write(&sig_path, signature.as_bytes())?;

    let mut find = Command::new("ssh-keygen");
    find.args(["-Y", "find-principals", "-f"])
        .arg(&allowed_path)
        .arg("-s")
        .arg(&sig_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(find_out) =
        work::bounded_write_output_cancellable(find, None, Duration::from_secs(5), cancellation)
    else {
        cancellation.check()?;
        return Ok(CommitSignature {
            format: SignatureFormat::Ssh,
            trust: SignatureTrust::Unverified,
            signer: None,
            key_id: None,
            detail: "Signed with SSH (local `ssh-keygen` unavailable or timed out).".into(),
        });
    };
    let principal = if find_out.status.success() {
        String::from_utf8_lossy(&find_out.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned)
    } else {
        None
    };
    let Some(principal) = principal else {
        let stderr_lower = String::from_utf8_lossy(&find_out.stderr).to_ascii_lowercase();
        let is_missing_key = stderr_lower.contains("no principal matched")
            || stderr_lower.contains("none of the keys");
        return Ok(CommitSignature {
            format: SignatureFormat::Ssh,
            trust: if is_missing_key {
                SignatureTrust::MissingKey
            } else {
                SignatureTrust::Unverified
            },
            signer: None,
            key_id: None,
            detail: if is_missing_key {
                "Signed with SSH (signing key is not listed in your global allowedSignersFile)."
                    .into()
            } else {
                "Signed with SSH (could not parse signature or match principal locally).".into()
            },
        });
    };
    let mut verify = Command::new("ssh-keygen");
    verify
        .args(["-Y", "verify", "-f"])
        .arg(&allowed_path)
        .args(["-I", &principal, "-n", "git", "-s"])
        .arg(&sig_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(verify_out) = work::bounded_write_output_cancellable(
        verify,
        Some(payload.to_vec()),
        Duration::from_secs(5),
        cancellation,
    ) else {
        cancellation.check()?;
        return Ok(CommitSignature {
            format: SignatureFormat::Ssh,
            trust: SignatureTrust::Unverified,
            signer: Some(principal),
            key_id: None,
            detail: "Signed with SSH (`ssh-keygen -Y verify` did not complete).".into(),
        });
    };
    if verify_out.status.success() {
        Ok(CommitSignature {
            format: SignatureFormat::Ssh,
            trust: SignatureTrust::Good,
            signer: Some(principal),
            key_id: None,
            detail: "Good SSH signature verified against your global allowedSignersFile.".into(),
        })
    } else {
        Ok(CommitSignature {
            format: SignatureFormat::Ssh,
            trust: SignatureTrust::Bad,
            signer: Some(principal),
            key_id: None,
            detail: "SSH signature failed verification.".into(),
        })
    }
}

fn read_global_ssh_allowed_signers(
    workdir: &Path,
    cancellation: &HistoryCancellation,
) -> Result<Option<PathBuf>> {
    for scope in ["--global", "--system"] {
        cancellation.check()?;
        let mut cmd = work::passive_user_config_command(workdir);
        cmd.args(["config", scope, "--get", "gpg.ssh.allowedSignersFile"]);
        if let Ok(out) =
            work::bounded_write_output_cancellable(cmd, None, Duration::from_secs(3), cancellation)
            && out.status.success()
        {
            let raw = text(trim_line(&out.stdout));
            if raw.is_empty() {
                continue;
            }
            if let Some(rest) = raw.strip_prefix("~/") {
                let Some(home) = std::env::var_os("HOME") else {
                    return Ok(None);
                };
                return Ok(Some(PathBuf::from(home).join(rest)));
            }
            return Ok(Some(PathBuf::from(raw)));
        }
    }
    Ok(None)
}

struct PrivateSigDir(PathBuf);

impl PrivateSigDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for PrivateSigDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn create_private_sig_dir() -> Result<PrivateSigDir> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let pid = std::process::id();
    let base = std::env::temp_dir();
    for attempt in 0u64..64 {
        let mut h1 = RandomState::new().build_hasher();
        h1.write_u64(attempt);
        let r1 = h1.finish();
        let mut h2 = RandomState::new().build_hasher();
        h2.write_u32(pid);
        let r2 = h2.finish();
        let candidate = base.join(format!("gitturtle-sig-{pid}-{r1:016x}{r2:016x}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            if builder.create(&candidate).is_ok() {
                return Ok(PrivateSigDir(candidate));
            }
        }
        #[cfg(not(unix))]
        {
            if std::fs::create_dir(&candidate).is_ok() {
                return Ok(PrivateSigDir(candidate));
            }
        }
    }
    bail!("Unable to create a private temporary directory for local signature verification")
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
