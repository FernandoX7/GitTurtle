//! Bounded passive revision comparisons and tracked-path discovery.
use super::*;
use crate::history::{ReadEnd, stream_history};

const MAX_INSPECTION_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRACKED_PATHS: usize = 100_000;
pub const MAX_PATH_MATCHES: usize = 500;

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

#[derive(Clone, Debug, PartialEq, Eq)]
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
            let mut command = git_command(&self.path);
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
        let mut command = git_command(&self.path);
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
            let mut command = git_command(&self.path);
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
        let mut command = git_command(&self.path);
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

    /// Search at most 100,000 tracked entries / 16 MiB. The returned revision
    /// scope is pinned. Worktree paths include tracked deletions and conflicts.
    pub fn search_tracked_paths(
        &self,
        scope: &PathScope,
        query: &str,
        cancellation: &HistoryCancellation,
    ) -> Result<TrackedPaths> {
        ensure!(query.len() <= 4096, "Path query exceeds 4 KiB");
        let mut command = git_command(&self.path);
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
        let query = query.to_lowercase();
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
                let fields: Vec<_> = record[..tab].split(|b| *b == b' ').collect();
                ensure!(fields.len() == 3, "Malformed tracked path metadata");
                let path = path_from_bytes(&record[tab + 1..]);
                if !path.to_string_lossy().to_lowercase().contains(&query) {
                    continue;
                }
                let (oid, conflicted) = match result.scope {
                    PathScope::Worktree => (fields[1], fields[2] != b"0"),
                    PathScope::Revision(_) => (fields[2], false),
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
                    mode: std::str::from_utf8(fields[0])?.to_owned(),
                    conflicted,
                });
            }
            pending.drain(..consumed);
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
