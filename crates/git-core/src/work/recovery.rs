//! Explicit recovery operations. Preparation is passive and pins the selected
//! worktree, branch, index, commit, and stash identities before any write.
use super::*;

const MAX_STASH_LOG_BYTES: usize = 16 * 1024 * 1024;
const MAX_STASH_ENTRIES: usize = 50_000;
const MAX_RECOVERY_WORK_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryHead {
    pub branch: String,
    pub head: String,
    root: PathBuf,
    branch_ref: Option<String>,
    index_token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryKind {
    Amend,
    Undo,
    Revert,
    CherryPick,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryCommit {
    pub oid: String,
    pub parents: Vec<String>,
    pub subject: String,
    /// Exact UTF-8 message from the raw commit, including final newlines.
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitRecoveryPlan {
    pub kind: RecoveryKind,
    pub current: RecoveryHead,
    pub target: RecoveryCommit,
    /// One-based parent choice, required explicitly for merge replay/revert.
    pub mainline: Option<usize>,
    pub affected_paths: Vec<PathBuf>,
    /// Current index changes that Amend will include in the replacement tip.
    pub staged_paths: Vec<PathBuf>,
    /// Containment in locally available remote-tracking refs only. This does
    /// not prove that a commit has never been published; no fetch is performed.
    pub known_published_refs: Vec<BranchReference>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashEntry {
    pub selector: String,
    pub oid: String,
    pub name: String,
    pub timestamp: i64,
    pub ordinal: usize,
    root: PathBuf,
    // The full bounded reflog snapshot disambiguates even duplicate stores of
    // the same object with identical names and second-resolution timestamps.
    reflog_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashPage {
    pub entries: Vec<StashEntry>,
    pub next_offset: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashSnapshot {
    pub stash: StashEntry,
    pub base_oid: String,
    pub index_oid: String,
    /// Base-to-saved-index changes, preserving their original stage boundary.
    pub staged: Vec<FileChange>,
    /// Saved-index-to-saved-working-tree changes.
    pub unstaged: Vec<FileChange>,
    /// Saved untracked files, absent on the old side.
    pub untracked: Vec<FileChange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashCreatePlan {
    pub current: RecoveryHead,
    pub include_untracked: bool,
    pub affected_paths: Vec<PathBuf>,
    worktree_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashApplyPlan {
    pub current: RecoveryHead,
    pub stash: StashEntry,
    pub restore_index: bool,
    pub affected_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryCommand {
    CreateStash {
        plan: StashCreatePlan,
        name: String,
    },
    ApplyStash {
        plan: StashApplyPlan,
    },
    DropStash {
        stash: StashEntry,
    },
    Amend {
        plan: CommitRecoveryPlan,
        message: String,
    },
    Undo {
        plan: CommitRecoveryPlan,
    },
    Revert {
        plan: CommitRecoveryPlan,
    },
    CherryPick {
        plan: CommitRecoveryPlan,
    },
}

impl GitRepository {
    pub fn recovery_plan(
        &self,
        kind: RecoveryKind,
        target_oid: Option<&str>,
        mainline: Option<usize>,
    ) -> Result<CommitRecoveryPlan> {
        let (current, status) = self.recovery_head()?;
        self.require_recovery_branch(&current)?;
        let target_oid = match kind {
            RecoveryKind::Amend | RecoveryKind::Undo => {
                ensure!(
                    target_oid.is_none_or(|oid| oid == current.head),
                    "Amend and Undo apply only to the displayed current tip"
                );
                ensure!(
                    mainline.is_none(),
                    "Amend and Undo do not take a parent choice"
                );
                current.head.as_str()
            }
            RecoveryKind::Revert | RecoveryKind::CherryPick => {
                target_oid.context("Choose a commit for this operation")?
            }
        };
        let target = self.recovery_commit(target_oid)?;
        let known_published_refs = self.recovery_published_refs(&target.oid)?;
        let staged_paths = sorted_paths(
            status
                .entries
                .iter()
                .filter(|entry| entry.staged.is_some())
                .flat_map(StatusEntry::paths)
                .collect(),
        );
        let affected_paths = match kind {
            RecoveryKind::Amend => {
                let mut paths = recovery_change_paths(&self.changes_with_renames(&target.oid, 0)?);
                paths.extend(
                    status
                        .entries
                        .iter()
                        .filter(|entry| entry.staged.is_some())
                        .flat_map(StatusEntry::paths),
                );
                sorted_paths(paths)
            }
            RecoveryKind::Undo => {
                ensure!(
                    target.parents.len() == 1,
                    "Undo requires a local commit with one parent. Root and merge commits need an explicit recovery strategy; use Revert when appropriate."
                );
                ensure!(
                    known_published_refs.is_empty(),
                    "This commit is contained in locally known remote-tracking branches ({}). Use Revert to preserve published history. No network check was performed.",
                    reference_names(&known_published_refs)
                );
                recovery_change_paths(&self.changes_with_renames(&target.oid, 0)?)
            }
            RecoveryKind::Revert | RecoveryKind::CherryPick => {
                let parent = recovery_parent(&target, mainline)?;
                let paths = recovery_change_paths(&self.changes_with_renames(&target.oid, parent)?);
                ensure!(
                    !status.entries.iter().any(|entry| entry.staged.is_some()),
                    "Commit or unstage staged changes before replaying a commit; this operation creates its own commit"
                );
                self.protect_recovery_paths(&status, &paths)?;
                paths
            }
        };
        Ok(CommitRecoveryPlan {
            kind,
            current,
            target,
            mainline,
            affected_paths,
            staged_paths,
            known_published_refs,
        })
    }

    /// Named stash metadata from local reflogs. Pages contain at most 500 rows;
    /// the identity snapshot has a 16 MiB/50,000-entry bound.
    pub fn stash_list(&self, offset: usize, limit: usize) -> Result<StashPage> {
        ensure!(
            (1..=500).contains(&limit),
            "Stash pages must contain 1–500 entries"
        );
        let entries = self.recovery_stash_entries()?;
        let end = offset.saturating_add(limit).min(entries.len());
        let next_offset = (end < entries.len()).then_some(end);
        Ok(StashPage {
            entries: entries.into_iter().skip(offset).take(limit).collect(),
            next_offset,
        })
    }

    pub fn stash_snapshot(&self, stash: &StashEntry) -> Result<StashSnapshot> {
        self.validate_recovery_stash(stash)?;
        let commit = self.recovery_commit(&stash.oid)?;
        ensure!(
            (2..=3).contains(&commit.parents.len()),
            "This stash does not have a supported base/index/working-tree structure"
        );
        let base_oid = commit.parents[0].clone();
        let index_oid = commit.parents[1].clone();
        let staged = self.recovery_tree_changes(&base_oid, &index_oid)?;
        let unstaged = self.recovery_tree_changes(&index_oid, &stash.oid)?;
        let untracked = if let Some(untracked) = commit.parents.get(2) {
            ensure!(
                self.recovery_commit(untracked)?.parents.is_empty(),
                "The stash's untracked snapshot has an unexpected parent"
            );
            self.changes_with_renames(untracked, 0)?
        } else {
            Vec::new()
        };
        Ok(StashSnapshot {
            stash: stash.clone(),
            base_oid,
            index_oid,
            staged,
            unstaged,
            untracked,
        })
    }

    pub fn stash_create_plan(&self, include_untracked: bool) -> Result<StashCreatePlan> {
        let (current, status) = self.recovery_head()?;
        let entries: Vec<_> = status
            .entries
            .iter()
            .filter(|entry| !entry.untracked || include_untracked)
            .collect();
        ensure!(
            !entries.is_empty(),
            "There are no changes to stash with this untracked-file choice"
        );
        ensure!(
            !entries.iter().any(|entry| entry.index_mode == "160000"
                || entry.head_mode == "160000"
                || entry.worktree_mode == "160000"),
            "A stash does not save nested submodule working changes. Commit or stash inside the submodule first, then review the parent repository."
        );
        let affected_paths = sorted_paths(entries.iter().flat_map(|entry| entry.paths()).collect());
        let worktree_token = self.recovery_worktree_token(&affected_paths)?;
        Ok(StashCreatePlan {
            current,
            include_untracked,
            affected_paths,
            worktree_token,
        })
    }

    pub fn stash_apply_plan(
        &self,
        stash: &StashEntry,
        restore_index: bool,
    ) -> Result<StashApplyPlan> {
        let (current, status) = self.recovery_head()?;
        let snapshot = self.stash_snapshot(stash)?;
        let affected_paths = sorted_paths(
            snapshot
                .staged
                .iter()
                .chain(&snapshot.unstaged)
                .chain(&snapshot.untracked)
                .flat_map(|change| change.old_path.iter().chain(&change.new_path).cloned())
                .collect(),
        );
        self.protect_recovery_paths(&status, &affected_paths)?;
        Ok(StashApplyPlan {
            current,
            stash: stash.clone(),
            restore_index,
            affected_paths,
        })
    }

    pub fn execute_recovery(&self, operation: &RecoveryCommand) -> Result<WriteOutcome> {
        ensure!(
            !self.bare,
            "Open a working copy to perform recovery operations"
        );
        let mut command = normal_command(&self.path);
        let mut input = None;
        let creates_commit;
        match operation {
            RecoveryCommand::CreateStash { plan, name } => {
                validate_recovery_message(name, "Enter a name for the stash")?;
                ensure!(
                    !name.contains(['\r', '\n']),
                    "Use one line for the stash name"
                );
                ensure!(
                    self.stash_create_plan(plan.include_untracked)? == *plan,
                    "The branch, index, or included working files changed. Review the stash contents again."
                );
                command.args(["stash", "push", "--message", name]);
                if plan.include_untracked {
                    command.arg("--include-untracked");
                }
                command.arg("--");
                creates_commit = false;
            }
            RecoveryCommand::ApplyStash { plan } => {
                ensure!(
                    self.stash_apply_plan(&plan.stash, plan.restore_index)? == *plan,
                    "The branch, index, stash, or affected paths changed. Review the stash application again."
                );
                command.args(["stash", "apply"]);
                if plan.restore_index {
                    command.arg("--index");
                }
                command.arg(&plan.stash.oid);
                creates_commit = false;
            }
            RecoveryCommand::DropStash { stash } => {
                self.validate_recovery_stash(stash)?;
                command.args(["stash", "drop", "--", &stash.selector]);
                creates_commit = false;
            }
            RecoveryCommand::Amend { plan, message } => {
                self.validate_commit_recovery(plan, RecoveryKind::Amend)?;
                validate_recovery_message(message, "Enter a commit message")?;
                command.args(["commit", "--amend", "--cleanup=verbatim", "--file=-"]);
                input = Some(message.as_bytes().to_vec());
                creates_commit = true;
            }
            RecoveryCommand::Undo { plan } => {
                self.validate_commit_recovery(plan, RecoveryKind::Undo)?;
                // Soft reset changes only HEAD/its branch and recovery reflogs.
                // Index bytes and all working files remain exactly as they are.
                command.args(["reset", "--soft", &plan.target.parents[0], "--"]);
                creates_commit = false;
            }
            RecoveryCommand::Revert { plan } | RecoveryCommand::CherryPick { plan } => {
                let kind = if matches!(operation, RecoveryCommand::Revert { .. }) {
                    RecoveryKind::Revert
                } else {
                    RecoveryKind::CherryPick
                };
                self.validate_commit_recovery(plan, kind)?;
                command.env("GIT_EDITOR", "true").args([
                    if kind == RecoveryKind::Revert {
                        "revert"
                    } else {
                        "cherry-pick"
                    },
                    "--no-edit",
                ]);
                if let Some(parent) = plan.mainline {
                    command.arg("--mainline").arg(parent.to_string());
                }
                command.arg("--").arg(&plan.target.oid);
                creates_commit = true;
            }
        }
        let output = if let RecoveryCommand::ApplyStash { plan } = operation {
            self.checked_stash_apply_output(command, &plan.stash)?
        } else {
            checked_write_output(command, input, WRITE_TIMEOUT)?
        };
        let message = format!("{}{}", text(&output.stdout), text(&output.stderr))
            .trim()
            .to_owned();
        let commit_oid = if creates_commit {
            self.status()?.head
        } else {
            None
        };
        Ok(WriteOutcome {
            message: if message.is_empty() {
                "Recovery operation completed".into()
            } else {
                message
            },
            commit_oid,
        })
    }

    fn checked_stash_apply_output(&self, command: Command, stash: &StashEntry) -> Result<Output> {
        // Only a completed nonzero command can be classified here. A timeout,
        // oversized output, or lost process result retains its existing error
        // and uncertainty; none of these paths retries or changes the repository.
        let output = bounded_write_output(command, None, WRITE_TIMEOUT)?;
        if output.status.success() {
            return Ok(output);
        }
        let conflicted = run_git(&self.path, &["ls-files", "--unmerged", "-z"])
            .map(|entries| !entries.is_empty());
        // Stash apply does not remove entries. Recheck the saved object rather
        // than its old ordinal, since another worktree can add a stash meanwhile.
        let retained = self
            .recovery_stash_entries()
            .map(|entries| entries.iter().any(|entry| entry.oid == stash.oid));
        let summary = match &conflicted {
            Ok(true) => "Stash restoration produced conflicts.",
            Ok(false) => "Stash restoration did not complete.",
            Err(_) => {
                "Stash restoration did not complete; the conflict state could not be verified."
            }
        };
        let saved = match retained {
            Ok(true) => "The stash remains saved.",
            Ok(false) => "The reviewed stash is no longer listed; review the saved stashes.",
            Err(_) => "The saved stash could not be rechecked; review the saved stashes.",
        };
        let guidance = if matches!(conflicted, Ok(true)) {
            "Resolve the conflicted files in Changes, then stage the resolved files. Stash restoration has no Continue or Abort operation."
        } else {
            "Review Git's diagnostics and the current files before taking another action."
        };
        // Some older Git releases return an empty stderr when stash apply
        // cannot acquire the index lock. Report observed local state without
        // claiming it proves the cause or removing another writer's lock.
        let index_locked = !matches!(conflicted, Ok(true))
            && run_git(
                &self.path,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "index.lock",
                ],
            )
            .ok()
            .is_some_and(|path| {
                std::fs::symlink_metadata(path_from_bytes(trim_line(&path))).is_ok()
            });
        let local_state = if index_locked {
            "\n\nAn index.lock is present in this working copy; another Git operation may own it. Let that operation finish, then refresh."
        } else {
            ""
        };
        // Keep both original streams, including stdout-only merge diagnostics.
        // The leading sentence is suitable for the compact operation error bar;
        // Details retains the full diagnostic content and actual exit status.
        bail!(
            "{summary} {saved}\n\n{guidance}{local_state}\n\nGit result: {}\n\nGit stdout:\n{}\n\nGit stderr:\n{}",
            output.status,
            text(&output.stdout),
            text(&output.stderr),
        )
    }

    fn recovery_head(&self) -> Result<(RecoveryHead, RepositoryStatus)> {
        ensure!(
            !self.bare,
            "Open a working copy to perform recovery operations"
        );
        let status = self.status()?;
        ensure!(
            status.operation.is_none()
                && !status.entries.iter().any(|entry| entry.conflicted)
                && self.operation_state()?.is_none(),
            "Finish the current operation and resolve conflicts before starting another recovery action"
        );
        let head = status
            .head
            .clone()
            .context("Create the first commit before using this recovery action")?;
        let index = run_git(&self.path, &["ls-files", "--stage", "--debug", "-z"])?;
        Ok((
            RecoveryHead {
                branch: status
                    .branch
                    .clone()
                    .unwrap_or_else(|| "Detached HEAD".into()),
                head,
                root: self.path.clone(),
                branch_ref: status.branch.clone(),
                index_token: format!("{:x}", Sha256::digest(index)),
            },
            status,
        ))
    }

    fn require_recovery_branch(&self, current: &RecoveryHead) -> Result<()> {
        let branch = current
            .branch_ref
            .as_ref()
            .context("Check out a branch before changing commit history")?;
        ensure!(
            !self
                .worktrees()?
                .iter()
                .any(|worktree| worktree.branch.as_ref() == Some(branch)
                    && worktree.path != self.path),
            "This branch is also checked out in another worktree. Move that worktree to another branch before changing shared history."
        );
        Ok(())
    }

    fn validate_commit_recovery(
        &self,
        plan: &CommitRecoveryPlan,
        kind: RecoveryKind,
    ) -> Result<()> {
        ensure!(
            plan.kind == kind && plan.current.root == self.path,
            "This recovery plan belongs to a different operation or worktree"
        );
        let current = self.recovery_plan(kind, Some(&plan.target.oid), plan.mainline)?;
        ensure!(
            current == *plan,
            "The branch tip, index, or locally known publication state changed. Review the recovery action again."
        );
        Ok(())
    }

    pub(super) fn recovery_commit(&self, oid: &str) -> Result<RecoveryCommit> {
        validate_oid(oid)?;
        let bytes = self.read_object(oid, "commit")?;
        let separator = bytes
            .windows(2)
            .position(|pair| pair == b"\n\n")
            .context("Malformed commit object")?;
        let headers = &bytes[..separator];
        let mut parents = Vec::new();
        for line in headers.split(|byte| *byte == b'\n') {
            if let Some(parent) = line.strip_prefix(b"parent ") {
                let parent = std::str::from_utf8(parent).context("Invalid commit parent")?;
                validate_oid(parent)?;
                parents.push(parent.into());
            }
        }
        let message = String::from_utf8(bytes[separator + 2..].to_vec()).context("This commit message is not UTF-8; use Git with its original encoding to preserve its text")?;
        let subject = message.lines().next().unwrap_or_default().to_owned();
        Ok(RecoveryCommit {
            oid: oid.into(),
            parents,
            subject,
            message,
        })
    }

    fn recovery_published_refs(&self, oid: &str) -> Result<Vec<BranchReference>> {
        validate_oid(oid)?;
        let bytes = run_git(
            &self.path,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)",
                "--contains",
                oid,
                "refs/remotes/",
            ],
        )?;
        let mut refs = Vec::new();
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let mut fields = line.split(|byte| *byte == 0);
            let name = std::str::from_utf8(fields.next().context("Missing reference name")?)?;
            let tip = std::str::from_utf8(fields.next().context("Missing reference tip")?)?;
            ensure!(
                fields.next().is_none(),
                "Malformed remote-tracking reference metadata"
            );
            validate_oid(tip)?;
            refs.push(BranchReference {
                name: name.into(),
                oid: tip.into(),
            });
        }
        Ok(refs)
    }

    fn recovery_tree_changes(&self, old: &str, new: &str) -> Result<Vec<FileChange>> {
        validate_oid(old)?;
        validate_oid(new)?;
        parse_changes(&run_git(
            &self.path,
            &[
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
                old,
                new,
                "--",
            ],
        )?)
    }

    fn recovery_stash_entries(&self) -> Result<Vec<StashEntry>> {
        let exists = run_git_output(
            &self.path,
            &["rev-parse", "--verify", "--quiet", "refs/stash"],
        )?;
        ensure!(
            exists.status.success() || exists.status.code() == Some(1),
            "Unable to inspect the stash reference"
        );
        if !exists.status.success() {
            return Ok(Vec::new());
        }
        let bytes = run_git(
            &self.path,
            &[
                "log",
                "--walk-reflogs",
                "--no-show-signature",
                "--no-decorate",
                "--no-notes",
                "--no-ext-diff",
                "--no-textconv",
                "-z",
                "--format=%H%x00%gs%x00%at",
                "refs/stash",
                "--",
            ],
        )?;
        ensure!(
            bytes.len() <= MAX_STASH_LOG_BYTES,
            "The stash reflog exceeds the 16 MiB inspection limit"
        );
        ensure!(
            bytes.is_empty() || bytes.last() == Some(&0),
            "Incomplete stash reflog output"
        );
        let token = format!("{:x}", Sha256::digest(&bytes));
        let fields: Vec<_> = bytes
            .strip_suffix(&[0])
            .unwrap_or(&bytes)
            .split(|byte| *byte == 0)
            .collect();
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        ensure!(
            fields.len().is_multiple_of(3) && fields.len() / 3 <= MAX_STASH_ENTRIES,
            "The stash reflog is malformed or exceeds 50,000 entries"
        );
        fields
            .as_chunks::<3>()
            .0
            .iter()
            .enumerate()
            .map(|(ordinal, record)| {
                let oid = std::str::from_utf8(record[0])?;
                validate_oid(oid)?;
                Ok(StashEntry {
                    selector: format!("stash@{{{ordinal}}}"),
                    oid: oid.into(),
                    name: text(record[1]),
                    timestamp: std::str::from_utf8(record[2])?
                        .parse()
                        .context("Invalid stash timestamp")?,
                    ordinal,
                    root: self.path.clone(),
                    reflog_token: token.clone(),
                })
            })
            .collect()
    }

    fn validate_recovery_stash(&self, stash: &StashEntry) -> Result<()> {
        ensure!(
            stash.root == self.path,
            "This stash action was prepared for a different worktree"
        );
        let entries = self.recovery_stash_entries()?;
        ensure!(
            entries.get(stash.ordinal) == Some(stash),
            "The stash list changed. Refresh and review the selected stash again; no stash was removed."
        );
        Ok(())
    }

    fn protect_recovery_paths(
        &self,
        status: &RepositoryStatus,
        affected: &[PathBuf],
    ) -> Result<()> {
        let overlaps = |path: &Path| {
            affected.iter().any(|affected| {
                path == affected || path.starts_with(affected) || affected.starts_with(path)
            })
        };
        ensure!(
            !status
                .entries
                .iter()
                .flat_map(StatusEntry::paths)
                .any(|path| overlaps(&path)),
            "Save the local changes that overlap this operation's affected paths first. Unrelated working files can remain in place."
        );
        // Ignored files do not appear in porcelain status. Protect any existing
        // affected path that is absent from the current index as well.
        let tracked = run_git(&self.path, &["ls-files", "-z"])?;
        let tracked: std::collections::HashSet<_> = tracked
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(path_from_bytes)
            .collect();
        for path in affected {
            validate_path(path)?;
            let mut prefix = PathBuf::new();
            for component in path.components() {
                prefix.push(component.as_os_str());
                match std::fs::symlink_metadata(self.path.join(&prefix)) {
                    Ok(metadata) if &prefix != path && metadata.is_dir() => {}
                    Ok(_) if tracked.contains(&prefix) => {
                        // A visible, clean tracked file may itself be replaced
                        // by a directory in this change. Do not traverse that
                        // file or symlink while inspecting its future children.
                        ensure!(
                            &prefix == path || affected.contains(&prefix),
                            "The tracked path '{}' obstructs an affected path. Review the operation's file changes.",
                            prefix.display()
                        );
                        break;
                    }
                    Ok(_) => bail!(
                        "The untracked or ignored path '{}' would be affected. Move or save it before proceeding.",
                        prefix.display()
                    ),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                    Err(error) => {
                        return Err(error).context("Unable to inspect an affected working path");
                    }
                }
            }
        }
        Ok(())
    }

    fn recovery_worktree_token(&self, paths: &[PathBuf]) -> Result<String> {
        let mut digest = Sha256::new();
        let mut total = 0usize;
        for path in paths {
            validate_path(path)?;
            digest.update(path.as_os_str().as_encoded_bytes());
            digest.update([0]);
            match std::fs::symlink_metadata(self.path.join(path)) {
                Ok(metadata) => {
                    ensure!(
                        !metadata.is_dir(),
                        "A changed directory or submodule cannot be captured as a regular stash file"
                    );
                    let (mode, bytes) = read_worktree_file(
                        &self.path,
                        path,
                        if metadata.file_type().is_symlink() {
                            "120000"
                        } else {
                            "100644"
                        },
                    )?;
                    total = total.saturating_add(bytes.len());
                    ensure!(
                        total <= MAX_RECOVERY_WORK_BYTES,
                        "The selected stash files exceed the 256 MiB preparation budget. Stash a smaller change using Git."
                    );
                    digest.update(mode.as_bytes());
                    digest.update([0]);
                    digest.update((bytes.len() as u64).to_le_bytes());
                    digest.update(&bytes);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    digest.update(b"absent")
                }
                Err(error) => return Err(error).context("Unable to inspect a stash working path"),
            }
            digest.update([0]);
        }
        Ok(format!("{:x}", digest.finalize()))
    }
}

fn recovery_parent(commit: &RecoveryCommit, mainline: Option<usize>) -> Result<usize> {
    if commit.parents.len() > 1 {
        let parent = mainline.context(
            "Choose the merge parent explicitly before reverting or cherry-picking a merge commit",
        )?;
        ensure!(
            (1..=commit.parents.len()).contains(&parent),
            "The selected merge parent is unavailable"
        );
        Ok(parent - 1)
    } else {
        ensure!(
            mainline.is_none(),
            "A parent choice is only valid for a merge commit"
        );
        Ok(0)
    }
}

fn validate_recovery_message(message: &str, empty: &str) -> Result<()> {
    ensure!(!message.trim().is_empty(), "{empty}");
    ensure!(
        message.len() <= 64 * 1024 && !message.contains('\0'),
        "Text must be at most 64 KiB and contain no NUL bytes"
    );
    Ok(())
}

fn sorted_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths.dedup();
    paths
}

fn recovery_change_paths(changes: &[FileChange]) -> Vec<PathBuf> {
    sorted_paths(
        changes
            .iter()
            .flat_map(|change| change.old_path.iter().chain(&change.new_path).cloned())
            .collect(),
    )
}

fn reference_names(references: &[BranchReference]) -> String {
    let mut names = references
        .iter()
        .take(8)
        .map(|reference| reference.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if references.len() > 8 {
        names.push_str(&format!(", and {} more", references.len() - 8));
    }
    names
}
