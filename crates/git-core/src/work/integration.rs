//! Explicit integration and raw, three-way conflict inspection.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

impl OperationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Merge => "Merge",
            Self::Rebase => "Rebase",
            Self::CherryPick => "Cherry-pick",
            Self::Revert => "Revert",
        }
    }
}

/// A worktree-local operation snapshot. Keep this with a Continue/Abort action so
/// an action for an old operation cannot affect a newly started operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationState {
    pub kind: OperationKind,
    pub head: Option<String>,
    pub branch: String,
    pub target_label: String,
    pub commit: Option<String>,
    /// The exact currently staged paths a Continue confirmation will commit.
    pub staged_paths: Vec<PathBuf>,
    /// An external interactive sequence needs its configured message editor.
    pub requires_message_editor: bool,
    state_token: String,
    index_token: String,
}

impl OperationState {
    /// Drafts belong to the same operation even when another conflict is staged.
    /// Continue/Abort still compare the complete state, including the index.
    pub fn same_operation(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.head == other.head
            && self.branch == other.branch
            && self.target_label == other.target_label
            && self.commit == other.commit
            && self.requires_message_editor == other.requires_message_editor
            && self.state_token == other.state_token
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrationPlan {
    pub head: String,
    pub branch: String,
    pub target_ref: String,
    pub target_oid: String,
    pub target_label: String,
    pub ahead: u64,
    pub behind: u64,
    /// Paths that differ between the displayed branch tips, in byte-safe form.
    pub affected_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictContent {
    pub mode: String,
    /// Raw content, bounded by MAX_BLOB_BYTES. Symlinks contain their target text.
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictSide {
    pub label: String,
    pub oid: String,
    pub content: ConflictContent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictPreview {
    pub path: PathBuf,
    pub head: Option<String>,
    pub operation: Option<OperationState>,
    pub base: Option<ConflictSide>,
    /// Index stage 2: the checked-out branch, or the new base during rebase.
    pub current: Option<ConflictSide>,
    /// Index stage 3: the integrated side, or the replayed commit during rebase.
    pub incoming: Option<ConflictSide>,
    pub working: Option<ConflictContent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Save exactly these UTF-8 bytes, then stage only this path using Git's
    /// configured filters. Only regular files support the inline editor.
    Manual {
        bytes: Vec<u8>,
    },
    Current,
    Incoming,
    /// Stage the working file already shown in the snapshot, including deletion.
    MarkResolved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegrationCommand {
    Merge {
        plan: IntegrationPlan,
    },
    Rebase {
        plan: IntegrationPlan,
    },
    Continue {
        expected: OperationState,
    },
    Abort {
        expected: OperationState,
    },
    /// End operation bookkeeping while retaining current HEAD/index/files.
    Quit {
        expected: OperationState,
    },
    Resolve {
        expected: Arc<ConflictPreview>,
        resolution: ConflictResolution,
    },
}

impl GitRepository {
    /// Detects operations started by any Git client, using this worktree's own
    /// Git directory. This never runs a hook, filter, or network operation.
    pub fn operation_state(&self) -> Result<Option<OperationState>> {
        ensure!(!self.bare, "A bare repository has no operation in progress");
        let directory = path_from_bytes(trim_line(&run_git(
            &self.path,
            &["rev-parse", "--absolute-git-dir"],
        )?));
        let mut files = Vec::new();
        for path in [
            "MERGE_HEAD",
            "MERGE_MSG",
            "AUTO_MERGE",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "ORIG_HEAD",
            "rebase-merge/head-name",
            "rebase-merge/onto",
            "rebase-merge/orig-head",
            "rebase-merge/stopped-sha",
            "rebase-merge/msgnum",
            "rebase-merge/end",
            "rebase-merge/git-rebase-todo",
            "rebase-merge/done",
            "rebase-merge/message",
            "rebase-merge/message-squash",
            "rebase-merge/message-fixup",
            "rebase-apply/head-name",
            "rebase-apply/onto",
            "rebase-apply/orig-head",
            "rebase-apply/original-commit",
            "rebase-apply/next",
            "rebase-apply/last",
            "sequencer/head",
            "sequencer/todo",
        ] {
            if let Some(bytes) = operation_file(&directory, Path::new(path))? {
                files.push((path, bytes));
            }
        }
        let value = |name: &str| {
            files
                .iter()
                .find(|(path, _)| *path == name)
                .map(|(_, bytes)| text(trim_line(bytes)))
        };
        let rebase = if operation_directory(&directory, "rebase-merge")? {
            Some("rebase-merge")
        } else if operation_directory(&directory, "rebase-apply")? {
            Some("rebase-apply")
        } else {
            None
        };
        let kind = if rebase.is_some() {
            OperationKind::Rebase
        } else if value("MERGE_HEAD").is_some() {
            OperationKind::Merge
        } else if value("CHERRY_PICK_HEAD").is_some()
            || value("sequencer/todo").is_some_and(|todo| todo.starts_with("pick "))
        {
            OperationKind::CherryPick
        } else if value("REVERT_HEAD").is_some()
            || value("sequencer/todo").is_some_and(|todo| todo.starts_with("revert "))
        {
            OperationKind::Revert
        } else {
            return Ok(None);
        };
        let status = self.status()?;
        let head = status.head;
        let branch = rebase
            .and_then(|prefix| value(&format!("{prefix}/head-name")))
            .map(|name| name.strip_prefix("refs/heads/").unwrap_or(&name).to_owned())
            .or(status.branch)
            .unwrap_or_else(|| "Detached HEAD".into());
        let commit = match kind {
            OperationKind::Merge => {
                value("MERGE_HEAD").and_then(|s| s.lines().next().map(str::to_owned))
            }
            OperationKind::Rebase => {
                value("rebase-merge/stopped-sha").or_else(|| value("rebase-apply/original-commit"))
            }
            OperationKind::CherryPick => value("CHERRY_PICK_HEAD"),
            OperationKind::Revert => value("REVERT_HEAD"),
        };
        let destination = if let Some(prefix) = rebase {
            value(&format!("{prefix}/onto"))
        } else {
            commit.clone()
        };
        let target_label = destination
            .as_deref()
            .map(|oid| self.integration_commit_label(oid))
            .transpose()?
            .unwrap_or_else(|| "Pending commits".into());
        let requires_message_editor = kind == OperationKind::Rebase
            && (value("rebase-merge/git-rebase-todo")
                .is_some_and(|todo| todo.lines().any(rebase_line_needs_editor))
                || value("rebase-merge/done").is_some_and(|done| {
                    done.lines()
                        .rfind(|line| {
                            !line.trim().is_empty() && !line.trim_start().starts_with('#')
                        })
                        .is_some_and(rebase_line_needs_editor)
                })
                || value("rebase-merge/message-squash").is_some());
        let mut digest = Sha256::new();
        for (path, bytes) in files {
            digest.update(path.as_bytes());
            digest.update([0]);
            digest.update(bytes);
            digest.update([0]);
        }
        let index = run_git(&self.path, &["ls-files", "--stage", "-z"])?;
        Ok(Some(OperationState {
            kind,
            head,
            branch,
            target_label,
            commit,
            staged_paths: status
                .entries
                .into_iter()
                .filter(|entry| entry.staged.is_some())
                .flat_map(|entry| entry.paths())
                .collect(),
            requires_message_editor,
            state_token: format!("{:x}", digest.finalize()),
            index_token: format!("{:x}", Sha256::digest(index)),
        }))
    }

    pub fn integration_plan(&self, target: &str) -> Result<IntegrationPlan> {
        if target == "@{upstream}" {
            return self.upstream_integration_plan();
        }
        let status = self.status()?;
        ensure!(
            status.operation.is_none(),
            "Finish the current Git operation first"
        );
        let head = status
            .head
            .context("Create the first commit before integrating branches")?;
        let branch = status
            .branch
            .context("Check out a branch before integrating")?;
        let target_ref = self.resolve_integration_target(target)?;
        let target_oid = self.resolve_start(&target_ref)?;
        let counts = run_git(
            &self.path,
            &[
                "rev-list",
                "--left-right",
                "--count",
                &format!("{head}...{target_oid}"),
                "--",
            ],
        )?;
        let counts = text(&counts);
        let mut counts = counts.split_whitespace();
        let ahead = counts
            .next()
            .context("Missing branch divergence count")?
            .parse()?;
        let behind = counts
            .next()
            .context("Missing target divergence count")?
            .parse()?;
        let paths = run_git(
            &self.path,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--name-only",
                "-z",
                &head,
                &target_oid,
                "--",
            ],
        )?;
        Ok(IntegrationPlan {
            head,
            branch,
            target_ref: target_ref.clone(),
            target_oid: target_oid.clone(),
            target_label: target_ref
                .strip_prefix("refs/heads/")
                .or_else(|| target_ref.strip_prefix("refs/remotes/"))
                .map(str::to_owned)
                .unwrap_or(self.integration_commit_label(&target_oid)?),
            ahead,
            behind,
            affected_paths: paths
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .map(path_from_bytes)
                .collect(),
        })
    }

    /// Resolve the actual configured upstream, which may be either a remote
    /// tracking branch or a local branch. Never infer it from display text.
    pub fn upstream_integration_plan(&self) -> Result<IntegrationPlan> {
        let target = run_git(
            &self.path,
            &[
                "rev-parse",
                "--symbolic-full-name",
                "--verify",
                "@{upstream}",
            ],
        )
        .context("Configure an upstream branch before integrating upstream changes")?;
        self.integration_plan(&text(trim_line(&target)))
    }

    fn resolve_integration_target(&self, target: &str) -> Result<String> {
        if validate_oid(target).is_ok()
            || target.starts_with("refs/heads/")
            || target.starts_with("refs/remotes/")
        {
            return Ok(target.into());
        }
        ensure!(
            !target.is_empty()
                && !target.starts_with('-')
                && target != "HEAD"
                && target.len() <= 1024,
            "Choose an existing local or remote branch"
        );
        let mut candidates = Vec::new();
        for prefix in ["refs/heads/", "refs/remotes/"] {
            let candidate = format!("{prefix}{target}");
            run_git(&self.path, &["check-ref-format", &candidate])
                .context("Choose a valid existing branch name")?;
            let output =
                run_git_output(&self.path, &["show-ref", "--verify", "--quiet", &candidate])?;
            ensure!(
                output.status.success() || output.status.code() == Some(1),
                "Unable to resolve integration target"
            );
            if output.status.success() {
                candidates.push(candidate);
            }
        }
        ensure!(
            candidates.len() <= 1,
            "This name matches both a local and remote branch. Choose its full refs/heads/ or refs/remotes/ name"
        );
        candidates
            .pop()
            .context("The branch no longer exists; refresh and choose an existing branch")
    }

    /// Read all present index stages and raw working bytes only when a conflict
    /// is activated. No conflict side is treated as a local filesystem path.
    pub fn conflict_preview(&self, path: &Path) -> Result<ConflictPreview> {
        validate_path(path)?;
        let operation = self.operation_state()?;
        let head = self.current_head()?;
        let branch = self
            .current_branch()?
            .unwrap_or_else(|| "Detached HEAD".into());
        let mut command = git_command(&self.path);
        command
            .args(["--literal-pathspecs", "ls-files", "--unmerged", "-z", "--"])
            .arg(path);
        let output = bounded_output(command, GIT_TIMEOUT)?;
        ensure!(
            output.status.success(),
            "Unable to inspect the conflicted index"
        );
        let mut stages = [None, None, None];
        for record in output.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let (metadata, recorded_path) = record.split_at(
                record
                    .iter()
                    .position(|b| *b == b'\t')
                    .context("Malformed conflict index entry")?,
            );
            ensure!(
                recorded_path[1..] == *path.as_os_str().as_encoded_bytes(),
                "Conflict path changed; refresh"
            );
            let metadata = std::str::from_utf8(metadata)?;
            let fields: Vec<_> = metadata.split_whitespace().collect();
            ensure!(fields.len() == 3, "Malformed conflict index metadata");
            let stage: usize = fields[2].parse()?;
            ensure!((1..=3).contains(&stage), "Unsupported conflict stage");
            let label = match stage {
                1 => "Common ancestor".into(),
                2 => match &operation {
                    Some(op) if op.kind == OperationKind::Rebase => {
                        format!("{} · updated base", op.target_label)
                    }
                    _ => format!("{branch} · current branch"),
                },
                _ => match &operation {
                    Some(op) if op.kind == OperationKind::Rebase => format!(
                        "{} · replayed commit {}",
                        op.branch,
                        op.commit.as_deref().map(short_oid).unwrap_or_default()
                    ),
                    Some(op) if op.kind == OperationKind::Revert => format!(
                        "Before reverted commit {}",
                        op.commit.as_deref().map(short_oid).unwrap_or_default()
                    ),
                    Some(op) => format!("{} · incoming changes", op.target_label),
                    None => "Incoming changes · index stage 3".into(),
                },
            };
            ensure!(stages[stage - 1].is_none(), "Duplicate conflict stage");
            stages[stage - 1] = Some(ConflictSide {
                label,
                oid: fields[1].into(),
                content: ConflictContent {
                    mode: fields[0].into(),
                    bytes: self.preview_side_bytes(fields[1], fields[0])?,
                },
            });
        }
        ensure!(
            stages.iter().any(Option::is_some),
            "This file is no longer conflicted; refresh the working copy"
        );
        let working = conflict_working_file(&self.path, path)?;
        let [base, current, incoming] = stages;
        Ok(ConflictPreview {
            path: path.into(),
            head,
            operation,
            base,
            current,
            incoming,
            working,
        })
    }

    /// App-level native editor handoff uses this path after explicit user action.
    /// Re-read the conflict after returning from the editor before marking it resolved.
    pub fn conflict_editor_path(&self, expected: &ConflictPreview) -> Result<PathBuf> {
        self.validate_conflict(expected)?;
        ensure!(
            expected
                .working
                .as_ref()
                .is_some_and(|content| matches!(content.mode.as_str(), "100644" | "100755")),
            "External editing requires a regular working file; choose a side or resolve the deletion first"
        );
        Ok(self.path.join(&expected.path))
    }

    /// Dispatch only through the serialized explicit-operation executor.
    pub fn execute_integration(&self, operation: &IntegrationCommand) -> Result<WriteOutcome> {
        ensure!(!self.bare, "Open a working copy to perform Git operations");
        let mut command = normal_command(&self.path);
        // GitTurtle presents the message and resolution itself. Continuing must
        // not launch a terminal editor, but normal hooks and signing still run.
        command.env("GIT_EDITOR", "true");
        match operation {
            IntegrationCommand::Merge { plan } | IntegrationCommand::Rebase { plan } => {
                let status = self.validate_integration_plan(plan)?;
                ensure!(
                    !status.entries.iter().any(|e| e.staged.is_some()),
                    "Commit or unstage staged changes before integrating branches"
                );
                if matches!(operation, IntegrationCommand::Rebase { .. }) {
                    ensure!(
                        !status.entries.iter().any(|e| !e.untracked),
                        "Commit or stash working changes before rebasing; no automatic stash is created"
                    );
                    self.protect_untracked_rebase_paths(plan)?;
                    command.args([
                        "rebase",
                        "--no-autostash",
                        "--no-update-refs",
                        "--no-autosquash",
                        "--",
                        &plan.target_oid,
                    ]);
                } else {
                    let message = format!(
                        "Merge {} '{}' into {}",
                        if plan.target_ref.starts_with("refs/remotes/") {
                            "remote-tracking branch"
                        } else {
                            "branch"
                        },
                        plan.target_label,
                        plan.branch
                    );
                    command.args([
                        "merge",
                        "--no-edit",
                        "--no-autostash",
                        "--no-squash",
                        "--ff",
                        "--no-overwrite-ignore",
                        "--message",
                        &message,
                        "--",
                        &plan.target_oid,
                    ]);
                }
            }
            IntegrationCommand::Continue { expected }
            | IntegrationCommand::Abort { expected }
            | IntegrationCommand::Quit { expected } => {
                ensure!(
                    self.operation_state()?.as_ref() == Some(expected),
                    "The operation changed; refresh before continuing or aborting"
                );
                let status = self.status()?;
                let abort = matches!(operation, IntegrationCommand::Abort { .. });
                let quit = matches!(operation, IntegrationCommand::Quit { .. });
                if !abort && !quit {
                    ensure!(
                        !status.entries.iter().any(|e| e.conflicted),
                        "Resolve and stage all conflicted files before continuing"
                    );
                    ensure!(
                        !expected.requires_message_editor,
                        "This interactive rebase includes a message-editing step. Continue it with `git rebase --continue` in your configured Git editor so reword, squash, and edited fixup messages are preserved. GitTurtle has left the sequence, index, and working files unchanged."
                    );
                } else if abort {
                    // AUTO_MERGE records the paths actually produced by Git's
                    // stopped merge/replay. Resolutions of those paths may be
                    // discarded by an explicit Abort; independent staged work
                    // must survive. Without this provenance, refuse uncertainty.
                    let resolution_paths = self.abort_resolution_paths()?;
                    ensure!(
                        !status.entries.iter().any(|entry| {
                            !entry.conflicted
                                && entry
                                    .paths()
                                    .iter()
                                    .any(|path| !resolution_paths.contains(path))
                                // Merge, cherry-pick and revert abort via
                                // reset --merge, which retains independent
                                // unstaged/untracked files. Rebase abort can
                                // hard-reset them, so retains the wider guard.
                                && (expected.kind == OperationKind::Rebase || entry.staged.is_some())
                        }),
                        "Save other changed files before aborting; Git may discard independent staged work or edits created during this operation. Stop and keep files preserves the current index and working files."
                    );
                }
                let verb = match expected.kind {
                    OperationKind::Merge => "merge",
                    OperationKind::Rebase => "rebase",
                    OperationKind::CherryPick => "cherry-pick",
                    OperationKind::Revert => "revert",
                };
                command.args([
                    verb,
                    if abort {
                        "--abort"
                    } else if quit {
                        "--quit"
                    } else {
                        "--continue"
                    },
                ]);
            }
            IntegrationCommand::Resolve {
                expected,
                resolution,
            } => {
                return self.resolve_conflict(expected, resolution);
            }
        }
        let output = checked_write_output(command, None, WRITE_TIMEOUT)?;
        integration_outcome(self, output)
    }

    fn abort_resolution_paths(&self) -> Result<Vec<PathBuf>> {
        let output = run_git_output(
            &self.path,
            &["rev-parse", "--verify", "--quiet", "AUTO_MERGE^{tree}"],
        )?;
        ensure!(
            output.status.success() || output.status.code() == Some(1),
            "Unable to inspect the operation's resolution paths"
        );
        if !output.status.success() {
            return Ok(Vec::new());
        }
        let tree = text(trim_line(&output.stdout));
        validate_oid(&tree)?;
        let paths = run_git(
            &self.path,
            &[
                "diff-tree",
                "--no-commit-id",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--name-only",
                "-r",
                "-z",
                "HEAD",
                &tree,
                "--",
            ],
        )?;
        Ok(paths
            .split(|b| *b == 0)
            .filter(|path| !path.is_empty())
            .map(path_from_bytes)
            .collect())
    }

    /// Git refuses ordinary untracked checkout collisions, but may overwrite
    /// ignored files while resetting to the rebase destination. Inspect only
    /// paths introduced by the destination or a replayed commit; unrelated
    /// ignored directories such as build outputs need no recursive traversal.
    fn protect_untracked_rebase_paths(&self, plan: &IntegrationPlan) -> Result<()> {
        use std::collections::HashSet;
        let tracked = run_git(&self.path, &["ls-files", "-z"])?;
        let tracked: HashSet<PathBuf> = tracked
            .split(|b| *b == 0)
            .filter(|path| !path.is_empty())
            .map(path_from_bytes)
            .collect();
        let mut paths = run_git(
            &self.path,
            &[
                "diff-tree",
                "--no-commit-id",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--diff-filter=AT",
                "--name-only",
                "-r",
                "-z",
                &plan.head,
                &plan.target_oid,
                "--",
            ],
        )?;
        let commits = run_git(
            &self.path,
            &[
                "rev-list",
                &format!("{}..{}", plan.target_oid, plan.head),
                "--",
            ],
        )?;
        ensure!(
            commits.len() <= MAX_WRITE_INPUT,
            "The rebase exceeds the local preflight limit; integrate fewer commits at a time"
        );
        if !commits.is_empty() {
            let mut command = git_command(&self.path);
            command.args([
                "diff-tree",
                "--stdin",
                "--root",
                "--no-commit-id",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--diff-filter=AT",
                "--name-only",
                "-m",
                "-r",
                "-z",
            ]);
            let replayed = checked_write_output(command, Some(commits), GIT_TIMEOUT)?.stdout;
            paths.extend(replayed);
        }
        ensure!(
            paths.len() <= MAX_WRITE_INPUT,
            "The rebase has too many changed paths for a safe local preflight"
        );
        let paths: HashSet<PathBuf> = paths
            .split(|b| *b == 0)
            .filter(|path| !path.is_empty())
            .map(path_from_bytes)
            .collect();
        ensure!(
            paths.len() <= 100_000,
            "The rebase exceeds the 100,000-path local preflight limit"
        );
        let started = Instant::now();
        for path in paths {
            ensure!(
                started.elapsed() < GIT_TIMEOUT,
                "The rebase file preflight exceeded its time limit; no rebase was started"
            );
            validate_path(&path)?;
            let mut prefix = PathBuf::new();
            let components: Vec<_> = path.components().collect();
            for (index, component) in components.iter().enumerate() {
                prefix.push(component.as_os_str());
                let metadata = match std::fs::symlink_metadata(self.path.join(&prefix)) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                    Err(error) => {
                        return Err(error).context("Unable to inspect a rebase destination path");
                    }
                };
                if !metadata.is_dir() {
                    ensure!(
                        tracked.contains(&prefix),
                        "Rebase would overwrite untracked or ignored work at {}. Move or save that path before rebasing; no rebase was started.",
                        prefix.display()
                    );
                    // A tracked symlink/file can deliberately become a directory.
                    // Do not follow it while inspecting the intended descendants.
                    if index + 1 < components.len() {
                        break;
                    }
                } else if index + 1 == components.len() {
                    ensure!(
                        !tracked.contains(&prefix),
                        "Rebase would replace the checked-out submodule at {}. Move or resolve that checkout explicitly first; no rebase was started.",
                        prefix.display()
                    );
                    let mut command = git_command(&self.path);
                    // Omitting exclude-standard intentionally includes ignored
                    // files. --directory avoids enumerating whole untracked trees.
                    command
                        .args([
                            "--literal-pathspecs",
                            "ls-files",
                            "--others",
                            "--directory",
                            "--no-empty-directory",
                            "-z",
                            "--",
                        ])
                        .arg(&prefix);
                    let output = bounded_output(command, GIT_TIMEOUT)?;
                    ensure!(
                        output.status.success(),
                        "Unable to inspect a directory replaced by rebase"
                    );
                    ensure!(
                        output.stdout.is_empty(),
                        "Rebase would replace a directory containing untracked or ignored work at {}. Move or save that directory before rebasing; no rebase was started.",
                        prefix.display()
                    );
                }
            }
        }
        Ok(())
    }

    fn current_head(&self) -> Result<Option<String>> {
        let output = run_git_output(&self.path, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
        ensure!(
            output.status.success() || output.status.code() == Some(1),
            "Unable to resolve HEAD"
        );
        Ok(output
            .status
            .success()
            .then(|| text(trim_line(&output.stdout))))
    }

    fn current_branch(&self) -> Result<Option<String>> {
        let output = run_git_output(&self.path, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        ensure!(
            output.status.success() || output.status.code() == Some(1),
            "Unable to resolve current branch"
        );
        Ok(output
            .status
            .success()
            .then(|| text(trim_line(&output.stdout))))
    }

    fn integration_commit_label(&self, oid: &str) -> Result<String> {
        validate_oid(oid)?;
        let bytes = run_git(
            &self.path,
            &[
                "for-each-ref",
                "--count=4",
                "--format=%(refname:short)",
                "--points-at",
                oid,
                "refs/heads",
                "refs/remotes",
            ],
        )?;
        let names = text(&bytes);
        Ok(names
            .lines()
            .next()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Commit {}", short_oid(oid))))
    }

    fn validate_integration_plan(&self, plan: &IntegrationPlan) -> Result<RepositoryStatus> {
        let status = self.status()?;
        ensure!(
            status.head.as_ref() == Some(&plan.head)
                && status.branch.as_ref() == Some(&plan.branch),
            "The current branch changed; review the integration again"
        );
        ensure!(
            self.resolve_start(&plan.target_ref)? == plan.target_oid,
            "The destination branch changed; review the integration again"
        );
        ensure!(
            self.operation_state()?.is_none() && !status.entries.iter().any(|e| e.conflicted),
            "Finish the current operation and resolve conflicts first"
        );
        Ok(status)
    }

    fn validate_conflict(&self, expected: &ConflictPreview) -> Result<()> {
        ensure!(
            self.conflict_preview(&expected.path)? == *expected,
            "The conflict or working file changed; reload it before resolving"
        );
        Ok(())
    }

    fn resolve_conflict(
        &self,
        expected: &ConflictPreview,
        resolution: &ConflictResolution,
    ) -> Result<WriteOutcome> {
        self.validate_conflict(expected)?;
        match resolution {
            ConflictResolution::Manual { bytes } => {
                ensure!(
                    bytes.len() <= MAX_DIFF_BYTES
                        && !bytes.contains(&0)
                        && std::str::from_utf8(bytes).is_ok(),
                    "Manual resolution requires UTF-8 text of at most 2 MiB without NUL bytes"
                );
                write_conflict_file(&self.path, &expected.path, expected.working.as_ref(), bytes)
                    .context(
                    "Unable to save the resolution; inspect the working file before retrying",
                )?;
            }
            ConflictResolution::Current | ConflictResolution::Incoming => {
                let side = if matches!(resolution, ConflictResolution::Current) {
                    &expected.current
                } else {
                    &expected.incoming
                };
                if let Some(side) = side {
                    ensure!(
                        side.content.mode != "160000",
                        "Resolve the submodule checkout explicitly, then mark it resolved"
                    );
                    let mut command = normal_command(&self.path);
                    command
                        .args([
                            "--literal-pathspecs",
                            "checkout",
                            if matches!(resolution, ConflictResolution::Current) {
                                "--ours"
                            } else {
                                "--theirs"
                            },
                            "--",
                        ])
                        .arg(&expected.path);
                    checked_write_output(command, None, WRITE_TIMEOUT).context(
                        "The chosen side may have been written; refresh before retrying",
                    )?;
                } else {
                    let mut command = normal_command(&self.path);
                    command
                        .args(["--literal-pathspecs", "rm", "--force", "--"])
                        .arg(&expected.path);
                    let output = checked_write_output(command, None, WRITE_TIMEOUT)?;
                    return integration_outcome(self, output);
                }
            }
            ConflictResolution::MarkResolved => {}
        }
        let mut command = normal_command(&self.path);
        command.args([
            "--literal-pathspecs",
            "add",
            "--all",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ]);
        let output = checked_write_output(
            command,
            Some(path_input(std::slice::from_ref(&expected.path))?),
            WRITE_TIMEOUT,
        )
        .context(
            "The resolution may be saved but not staged; refresh and inspect before retrying",
        )?;
        integration_outcome(self, output)
    }
}

fn integration_outcome(repo: &GitRepository, output: Output) -> Result<WriteOutcome> {
    let message = format!("{}{}", text(&output.stdout), text(&output.stderr))
        .trim()
        .to_owned();
    Ok(WriteOutcome {
        message: if message.is_empty() {
            "Git operation completed".into()
        } else {
            message
        },
        commit_oid: repo.current_head()?,
    })
}

fn short_oid(oid: &str) -> &str {
    &oid[..oid.len().min(10)]
}

fn rebase_line_needs_editor(line: &str) -> bool {
    let mut fields = line.split_whitespace();
    match fields.next() {
        Some("reword" | "r" | "squash" | "s") => true,
        Some("fixup" | "f" | "merge" | "m") => fields.next() == Some("-c"),
        _ => false,
    }
}

fn operation_directory(root: &Path, path: &str) -> Result<bool> {
    match std::fs::symlink_metadata(root.join(path)) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Git operation metadata is not a safe directory"
            );
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("Unable to inspect Git operation metadata"),
    }
}

fn operation_file(root: &Path, path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(root.join(path)) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= 1024 * 1024,
                "Git operation metadata is unsafe or exceeds 1 MiB"
            );
            let (mode, bytes) = read_worktree_file(root, path, "100644")?;
            ensure!(
                mode != "120000" && bytes.len() <= 1024 * 1024,
                "Git operation metadata changed or exceeds 1 MiB"
            );
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("Unable to inspect Git operation metadata"),
    }
}

fn conflict_working_file(root: &Path, path: &Path) -> Result<Option<ConflictContent>> {
    match std::fs::symlink_metadata(root.join(path)) {
        Ok(metadata) => {
            if metadata.is_dir() {
                return Ok(Some(ConflictContent {
                    mode: "160000".into(),
                    bytes: Vec::new(),
                }));
            }
            let (mode, bytes) = read_worktree_file(
                root,
                path,
                if metadata.file_type().is_symlink() {
                    "120000"
                } else {
                    "100644"
                },
            )?;
            Ok(Some(ConflictContent { mode, bytes }))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("Unable to inspect the conflicted working file"),
    }
}

#[cfg(unix)]
fn write_conflict_file(
    root: &Path,
    path: &Path,
    expected: Option<&ConflictContent>,
    bytes: &[u8],
) -> Result<()> {
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::io::{Seek, SeekFrom};
    use std::os::unix::fs::PermissionsExt;
    validate_path(path)?;
    ensure!(
        expected.is_none_or(|content| matches!(content.mode.as_str(), "100644" | "100755")),
        "Manual resolution edits regular files only; choose a side for symbolic links and submodules"
    );
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open(root, flags, Mode::empty())?;
    let components: Vec<_> = path.components().collect();
    for component in &components[..components.len() - 1] {
        directory = openat(&directory, component.as_os_str(), flags, Mode::empty())
            .context("Resolution cannot follow a symbolic-link directory")?;
    }
    let name = components.last().context("Missing filename")?.as_os_str();
    let flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    let fd = openat(
        &directory,
        name,
        if expected.is_none() {
            flags | OFlags::CREATE | OFlags::EXCL
        } else {
            flags
        },
        Mode::from_raw_mode(0o644),
    )?;
    let mut file = std::fs::File::from(fd);
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "Manual resolution requires a regular file"
    );
    if let Some(expected) = expected {
        let mode = if metadata.permissions().mode() & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        };
        ensure!(
            mode == expected.mode,
            "The working file mode changed; reload before resolving"
        );
        let mut actual = Vec::new();
        (&mut file)
            .take(MAX_BLOB_BYTES as u64 + 1)
            .read_to_end(&mut actual)?;
        ensure!(
            actual == expected.bytes,
            "The working file changed; reload before resolving"
        );
    }
    file.seek(SeekFrom::Start(0))?;
    file.write_all(bytes)?;
    file.set_len(bytes.len() as u64)?;
    Ok(())
}

#[cfg(not(unix))]
fn write_conflict_file(
    _root: &Path,
    _path: &Path,
    _expected: Option<&ConflictContent>,
    _bytes: &[u8],
) -> Result<()> {
    bail!("Safe manual resolution is currently supported on macOS and Linux")
}
