//! Explicit recovery operations. Preparation is passive and pins the selected
//! worktree, branch, index, commit, and stash identities before any write.
use super::*;
use crate::history::{ReadEnd, stream_history};

const MAX_STASH_LOG_BYTES: usize = 16 * 1024 * 1024;
const MAX_STASH_ENTRIES: usize = 50_000;
const MAX_RECOVERY_WORK_BYTES: usize = 256 * 1024 * 1024;
/// The most a recovery write accepts, a commit message or a stash's name,
/// and so the furthest a recovery plan reads a commit's message: to one byte
/// past this, which tells a message at the limit from one over it.
pub const MAX_RECOVERY_MESSAGE_BYTES: usize = 64 * 1024;
const NOT_UTF8_MESSAGE: &str =
    "This commit message is not UTF-8; use Git with its original encoding to preserve its text";

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
    /// Empty on an Amend plan's target whose message is over the limit
    /// ([`CommitRecoveryPlan::message_too_long`]).
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitRecoveryPlan {
    pub kind: RecoveryKind,
    pub current: RecoveryHead,
    pub target: RecoveryCommit,
    /// Whether an Amend plan's target message is over
    /// [`MAX_RECOVERY_MESSAGE_BYTES`], the most Amend can submit, so executing
    /// the plan is refused. Then only `target.subject` keeps any of it, from
    /// a read that stopped one byte past the limit. Always false for the
    /// other kinds, which read the whole commit.
    pub message_too_long: bool,
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
        // Only Amend loads the message into a form, so only its plan stops
        // reading at what it can submit; the others keep the whole-object
        // read and each of its refusals.
        let (target, message_too_long) = if kind == RecoveryKind::Amend {
            self.amend_target(target_oid)?
        } else {
            (self.recovery_commit(target_oid)?, false)
        };
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
                let mut paths = recovery_change_paths(&self.amend_target_changes(&target)?);
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
            message_too_long,
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
        let mut command = normal_command(self.target());
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
                ensure!(
                    !plan.message_too_long,
                    "This commit's message is too long to amend here"
                );
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
            up_to_date: false,
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
        let conflicted = run_git(self.target(), &["ls-files", "--unmerged", "-z"])
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
                self.target(),
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
        // Keep both streams, including stdout-only merge diagnostics,
        // redacted and then as a terminal leaves them (`reported_redacted`),
        // as every other write's failure reads them: stash apply runs
        // configured filters, whose output can hold a credential.
        // The leading sentence is the compact operation error bar's own line;
        // Details retains the full diagnostic content and actual exit status.
        let headline = format!("{summary} {saved}");
        Err(git_failure_summarized(
            &output.stderr,
            Some(headline.clone()),
            format!(
                "{headline}\n\n{guidance}{local_state}\n\nGit result: {}\n\nGit stdout:\n{}\n\nGit stderr:\n{}",
                output.status,
                reported_redacted(&output.stdout, authentication::redact_current),
                reported_redacted(&output.stderr, authentication::redact_current),
            ),
        ))
    }

    fn recovery_head(&self) -> Result<(RecoveryHead, RepositoryStatus)> {
        ensure!(
            !self.bare,
            "Open a working copy to perform recovery operations"
        );
        let status = self.status()?;
        let paused =
            if status.operation.is_some() || status.entries.iter().any(|entry| entry.conflicted) {
                // Named from the status's own marker, which a merge, rebase,
                // cherry-pick or revert leaves, and otherwise from the full
                // operation read (a sequencer step without its HEAD file);
                // conflicts with neither, as a stash restoration leaves,
                // refuse in their own words. The full read, a second status
                // read among others, runs only where the marker is missing.
                let named = status.operation.as_deref().and_then(|label| {
                    OperationKind::ALL
                        .into_iter()
                        .find(|kind| kind.label() == label)
                });
                Some(named.or_else(|| {
                    self.operation_state()
                        .ok()
                        .flatten()
                        .map(|operation| operation.kind)
                }))
            } else {
                self.operation_state()?
                    .map(|operation| Some(operation.kind))
            };
        if let Some(operation) = paused {
            bail!(paused_recovery_refusal(operation));
        }
        let head = status
            .head
            .clone()
            .context("Create the first commit before using this recovery action")?;
        let index = run_git(self.target(), &["ls-files", "--stage", "--debug", "-z"])?;
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
        let (parents, start) = recovery_commit_parts(&bytes)?;
        let message = String::from_utf8(bytes[start..].to_vec()).context(NOT_UTF8_MESSAGE)?;
        let subject = message.lines().next().unwrap_or_default().to_owned();
        Ok(RecoveryCommit {
            oid: oid.into(),
            parents,
            subject,
            message,
        })
    }

    /// The commit `oid` for an Amend plan, its message read no further than
    /// one byte past [`MAX_RECOVERY_MESSAGE_BYTES`], and whether that byte
    /// is there: a message over the limit is left out of the commit, whose
    /// subject is then from what the read reached.
    fn amend_target(&self, oid: &str) -> Result<(RecoveryCommit, bool)> {
        validate_oid(oid)?;
        self.ensure_commit_object(oid)?;
        let bytes = self.commit_through_message_limit(oid)?;
        let (parents, start) = recovery_commit_parts(&bytes)?;
        let read = &bytes[start..];
        let too_long = read.len() > MAX_RECOVERY_MESSAGE_BYTES;
        let text = &read[..read.len().min(MAX_RECOVERY_MESSAGE_BYTES)];
        let text = match std::str::from_utf8(text) {
            Ok(text) => text,
            // The limit can fall inside a character; only what comes before
            // it is used, and no more of the message was read to complete it.
            Err(cut) if too_long && cut.error_len().is_none() => {
                std::str::from_utf8(&text[..cut.valid_up_to()]).expect("checked up to the cut")
            }
            Err(error) => return Err(error).context(NOT_UTF8_MESSAGE),
        };
        Ok((
            RecoveryCommit {
                oid: oid.into(),
                parents,
                subject: text.lines().next().unwrap_or_default().to_owned(),
                message: if too_long {
                    String::new()
                } else {
                    text.to_owned()
                },
            },
            too_long,
        ))
    }

    /// Refuses `oid` unless the object reader has it as a commit, in the
    /// reader's own words for a missing object or another type, asking for
    /// its header alone. `git cat-file commit` would read through a tag to
    /// the commit it names, and say only that it failed for a missing one.
    fn ensure_commit_object(&self, oid: &str) -> Result<()> {
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
            .request_header("info", oid, "commit");
        // As every request of the shared reader: a failed one may leave
        // unread protocol bytes, so the next starts a new process.
        if result.is_err() {
            *guard = None;
        }
        result.map(drop)
    }

    /// The raw commit `oid` through one byte past
    /// [`MAX_RECOVERY_MESSAGE_BYTES`] of its message, where the read stops
    /// however long the message is: `read_object` would take the whole
    /// object first. Streamed, so its deadline and pipe cleanup are the ones
    /// every streamed read has.
    fn commit_through_message_limit(&self, oid: &str) -> Result<Vec<u8>> {
        let mut command = git_command(self.target());
        command.args(["cat-file", "commit", oid]);
        let mut commit = CommitThroughMessageLimit::default();
        let end = stream_history(
            command,
            None,
            &HistoryCancellation::default(),
            GIT_TIMEOUT,
            |chunk| commit.take(chunk),
        )?;
        ensure!(
            end != ReadEnd::TimedOut,
            "Reading this commit exceeded its time limit"
        );
        Ok(commit.bytes)
    }

    fn recovery_published_refs(&self, oid: &str) -> Result<Vec<BranchReference>> {
        validate_oid(oid)?;
        let bytes = run_git(
            self.target(),
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
        self.recovery_diff_tree([old, new])
    }

    /// What Amend's `target` changed against its first parent, or added
    /// where it is a root commit, from the parents it already holds:
    /// `changes_with_renames` would read the whole commit again to find
    /// them, however long its message is.
    fn amend_target_changes(&self, target: &RecoveryCommit) -> Result<Vec<FileChange>> {
        match target.parents.first() {
            Some(parent) => self.recovery_tree_changes(parent, &target.oid),
            None => self.recovery_diff_tree(["--root", &target.oid]),
        }
    }

    fn recovery_diff_tree(&self, sides: [&str; 2]) -> Result<Vec<FileChange>> {
        parse_changes(&run_git(
            self.target(),
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
                sides[0],
                sides[1],
                "--",
            ],
        )?)
    }

    fn recovery_stash_entries(&self) -> Result<Vec<StashEntry>> {
        let exists = run_git_output(
            self.target(),
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
            self.target(),
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
        let tracked = run_git(self.target(), &["ls-files", "-z"])?;
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

/// Why every recovery plan refuses while `operation` is paused, or while
/// conflicts remain with no operation to continue (`None`, as a conflicted
/// stash restoration leaves them): it names the paused operation and the
/// two ways its banner ends it, Continue (Complete merge for a merge) and
/// Abort. A client that refuses before asking, from the pause it read
/// last, uses the same words, and [`is_paused_recovery_refusal`] tells
/// them apart from any other reason.
pub const fn paused_recovery_refusal(operation: Option<OperationKind>) -> &'static str {
    match operation {
        Some(OperationKind::Merge) => {
            "A merge is paused. Complete or abort it before another recovery action."
        }
        Some(OperationKind::Rebase) => {
            "A rebase is paused. Continue or abort it before another recovery action."
        }
        Some(OperationKind::CherryPick) => {
            "A cherry-pick is paused. Continue or abort it before another recovery action."
        }
        Some(OperationKind::Revert) => {
            "A revert is paused. Continue or abort it before another recovery action."
        }
        None => {
            "Conflicts remain in this working copy. Resolve them before another recovery action."
        }
    }
}

/// The pause `reason` refuses for, where it is one of the
/// [`paused_recovery_refusal`]s: the operation it names, or `None` for
/// conflicts alone.
pub fn paused_recovery_operation(reason: &str) -> Option<Option<OperationKind>> {
    std::iter::once(None)
        .chain(OperationKind::ALL.map(Some))
        .find(|operation| paused_recovery_refusal(*operation) == reason)
}

/// Whether `reason` is one of the [`paused_recovery_refusal`]s.
pub fn is_paused_recovery_refusal(reason: &str) -> bool {
    paused_recovery_operation(reason).is_some()
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
        message.len() <= MAX_RECOVERY_MESSAGE_BYTES && !message.contains('\0'),
        "Text must be at most 64 KiB and contain no NUL bytes"
    );
    Ok(())
}

/// A raw commit as Git streams it: its headers, through the blank line that
/// ends them and within the object limit a whole-object read keeps, and its
/// message's first [`MAX_RECOVERY_MESSAGE_BYTES`] and one byte.
#[derive(Default)]
struct CommitThroughMessageLimit {
    bytes: Vec<u8>,
    /// Where the headers end, once their blank line has arrived.
    separator: Option<usize>,
}

impl CommitThroughMessageLimit {
    /// Takes the next `chunk` from Git and says whether to read another.
    fn take(&mut self, chunk: &[u8]) -> Result<bool> {
        // The blank line can straddle two chunks.
        let from = self.bytes.len().saturating_sub(1);
        self.bytes.extend_from_slice(chunk);
        if self.separator.is_none() {
            self.separator = self.bytes[from..]
                .windows(2)
                .position(|pair| pair == b"\n\n")
                .map(|at| from + at);
            ensure!(
                self.separator.is_some() || self.bytes.len() <= MAX_BLOB_BYTES,
                "This commit's headers are over the object limit"
            );
        }
        let Some(separator) = self.separator else {
            return Ok(true);
        };
        let through_limit = separator + 2 + MAX_RECOVERY_MESSAGE_BYTES + 1;
        self.bytes.truncate(through_limit);
        Ok(self.bytes.len() < through_limit)
    }
}

/// A raw commit's parents and the offset of its message.
fn recovery_commit_parts(bytes: &[u8]) -> Result<(Vec<String>, usize)> {
    let separator = bytes
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .context("Malformed commit object")?;
    let mut parents = Vec::new();
    for line in bytes[..separator].split(|byte| *byte == b'\n') {
        if let Some(parent) = line.strip_prefix(b"parent ") {
            let parent = std::str::from_utf8(parent).context("Invalid commit parent")?;
            validate_oid(parent)?;
            parents.push(parent.into());
        }
    }
    Ok((parents, separator + 2))
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

#[cfg(test)]
mod tests {
    use super::*;

    const HEADERS: &[u8] = b"tree x\n\n";

    #[test]
    fn a_commit_read_stops_one_byte_past_the_message_limit() {
        let mut commit = CommitThroughMessageLimit::default();
        assert!(commit.take(HEADERS).unwrap(), "headers alone go on");
        assert!(
            commit
                .take(&vec![b'x'; MAX_RECOVERY_MESSAGE_BYTES])
                .unwrap(),
            "a message at the limit may yet end there"
        );
        assert!(
            !commit.take(&[b'x'; 8192]).unwrap(),
            "past the limit the read stops"
        );
        assert_eq!(
            commit.bytes.len(),
            HEADERS.len() + MAX_RECOVERY_MESSAGE_BYTES + 1,
            "keeping one byte of the chunk that took it past"
        );
    }

    #[test]
    fn a_blank_line_across_two_chunks_ends_a_commits_headers() {
        let mut commit = CommitThroughMessageLimit::default();
        assert!(commit.take(b"tree x\n").unwrap());
        assert_eq!(commit.separator, None);
        assert!(commit.take(b"\nmessage").unwrap());
        assert_eq!(commit.separator, Some(6));
    }

    /// Status refuses a HEAD whose commit is gone, so through a plan only a
    /// commit removed after that read reaches Amend's; this drives that read.
    #[test]
    fn amend_says_when_its_target_is_not_available() {
        let temp = tempfile::TempDir::new().unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(["init", "-q"])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .status()
            .unwrap();
        assert!(status.success());
        let repository = GitRepository::open(temp.path()).unwrap();
        let missing = "1".repeat(40);
        let error = repository.amend_target(&missing).unwrap_err().to_string();
        assert!(
            error.contains("is not available locally (automatic fetching is disabled)"),
            "{error}"
        );
    }

    #[test]
    fn a_commits_headers_stay_within_the_object_limit() {
        let mut commit = CommitThroughMessageLimit::default();
        let error = commit.take(&vec![b'a'; MAX_BLOB_BYTES + 1]).unwrap_err();
        assert!(
            error.to_string().contains("over the object limit"),
            "{error}"
        );
    }
}
