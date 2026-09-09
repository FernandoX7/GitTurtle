//! Reviewed linear history editing through Git's native sequencer.
use super::*;
use std::collections::HashSet;

pub const MAX_INTERACTIVE_REBASE_COMMITS: usize = 100;
const MAX_REBASE_MESSAGE: usize = 1024 * 1024;
const MESSAGE_MARKER: &str = "rebase-merge/gitturtle-message-edit";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebaseAction {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

impl RebaseAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pick => "Pick",
            Self::Reword => "Reword",
            Self::Squash => "Squash",
            Self::Fixup => "Fixup",
            Self::Drop => "Drop",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebaseStep {
    pub oid: String,
    pub action: RebaseAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractiveRebasePlan {
    pub root: PathBuf,
    pub branch: String,
    pub head: String,
    pub base_revision: String,
    pub base: String,
    /// Oldest first; all commits must appear exactly once in the edited plan.
    pub commits: Vec<RecoveryCommit>,
    /// Local remote-tracking names containing any affected commit. No fetch.
    pub known_published_refs: Vec<String>,
}

impl InteractiveRebasePlan {
    pub fn review_identity(&self) -> String {
        let mut digest = Sha256::new();
        for value in [&self.branch, &self.base, &self.head] {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
        format!("{:x}", digest.finalize())
    }
    pub fn steps(&self) -> Vec<RebaseStep> {
        self.commits
            .iter()
            .map(|commit| RebaseStep {
                oid: commit.oid.clone(),
                action: RebaseAction::Pick,
            })
            .collect()
    }

    pub fn validate_steps(&self, steps: &[RebaseStep]) -> Result<()> {
        ensure!(
            steps.len() == self.commits.len(),
            "Keep every reviewed commit in the plan; choose Drop to remove a commit explicitly"
        );
        let known: HashSet<_> = self
            .commits
            .iter()
            .map(|commit| commit.oid.as_str())
            .collect();
        let mut seen = HashSet::new();
        let mut preceding = false;
        for step in steps {
            ensure!(
                known.contains(step.oid.as_str()) && seen.insert(step.oid.as_str()),
                "The plan contains a duplicate or unreviewed commit"
            );
            match step.action {
                RebaseAction::Squash | RebaseAction::Fixup => {
                    ensure!(preceding, "Squash and Fixup need a preceding kept commit")
                }
                RebaseAction::Pick | RebaseAction::Reword => preceding = true,
                RebaseAction::Drop => (),
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractiveRebaseResume {
    pub root: PathBuf,
    pub operation: OperationState,
    /// Git's exact current message/template, including comments and newlines.
    pub message: Option<String>,
    /// Failed reword already applied its commit; Continue alone skips its edit.
    amend_reword: bool,
}

impl InteractiveRebaseResume {
    pub fn draft_identity(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.operation.draft_identity(true));
        digest.update([
            u8::from(self.amend_reword),
            u8::from(self.message.is_some()),
        ]);
        if let Some(message) = &self.message {
            digest.update(message.as_bytes());
        }
        format!("{:x}", digest.finalize())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractiveRebaseCommand {
    Start {
        plan: InteractiveRebasePlan,
        steps: Vec<RebaseStep>,
        acknowledge_published: bool,
    },
    Continue {
        expected: InteractiveRebaseResume,
        message: Option<String>,
    },
}

impl GitRepository {
    /// Bounded passive preparation. The base must precede a single linear chain.
    pub fn interactive_rebase_plan(&self, base: &str) -> Result<InteractiveRebasePlan> {
        ensure!(!self.bare, "Open a working copy before editing commits");
        ensure!(
            !base.is_empty()
                && base.len() <= 1024
                && !base.starts_with('-')
                && !base.contains(['\0', '\n', '\r']),
            "Choose a locally available base revision"
        );
        let status = self.status()?;
        ensure!(
            status.operation.is_none() && self.operation_state()?.is_none(),
            "Finish or abort the current operation before editing commits"
        );
        let branch = status
            .branch
            .context("Check out a branch before editing commits")?;
        let head = status
            .head
            .context("Create commits before editing history")?;
        let base_oid = text(trim_line(&run_git(
            &self.path,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{base}^{{commit}}"),
            ],
        )?));
        validate_oid(&base_oid)?;
        let output = run_git(
            &self.path,
            &[
                "rev-list",
                "--reverse",
                "--topo-order",
                "--max-count=101",
                &format!("{base_oid}..{head}"),
                "--",
            ],
        )?;
        let oids: Vec<_> = std::str::from_utf8(&output)?.lines().collect();
        ensure!(
            !oids.is_empty(),
            "Choose a base before at least one commit on the current branch"
        );
        ensure!(
            oids.len() <= MAX_INTERACTIVE_REBASE_COMMITS,
            "Review at most 100 commits at a time; choose a more recent base"
        );
        let mut commits = Vec::with_capacity(oids.len());
        let mut previous = base_oid.clone();
        let mut bytes = 0;
        let started = Instant::now();
        for oid in oids {
            ensure!(
                started.elapsed() < GIT_TIMEOUT,
                "The rebase review exceeded its time limit; choose a more recent base"
            );
            let commit = self.recovery_commit(oid)?;
            ensure!(
                commit.parents.len() == 1 && commit.parents[0] == previous,
                "This range is not a linear sequence descending directly from the base. Merge-preserving rewrites, root commits, and unrelated bases are not supported; choose a base after the last merge."
            );
            bytes += commit.message.len();
            ensure!(
                bytes <= 4 * MAX_REBASE_MESSAGE && commit.message.len() <= MAX_REBASE_MESSAGE,
                "Commit messages exceed the native rebase review limit"
            );
            previous = commit.oid.clone();
            commits.push(commit);
        }
        ensure!(
            previous == head,
            "The selected commits do not reach the current branch tip"
        );
        // In a linear chain, a remote containing any later commit necessarily
        // contains the oldest commit. One containment walk covers the range.
        let refs = run_git(
            &self.path,
            &[
                "for-each-ref",
                "--count=101",
                "--format=%(refname)",
                "--contains",
                &commits[0].oid,
                "refs/remotes/",
            ],
        )?;
        let published: Vec<_> = std::str::from_utf8(&refs)?
            .lines()
            .map(str::to_owned)
            .collect();
        ensure!(
            published.len() <= 100,
            "Too many remote-tracking references contain this history for a bounded review"
        );
        Ok(InteractiveRebasePlan {
            root: self.path.clone(),
            branch,
            head,
            base_revision: base.into(),
            base: base_oid,
            commits,
            known_published_refs: published,
        })
    }

    /// Reads worktree-local Git state, including the native editor checkpoint,
    /// so an interrupted operation can resume after restarting the application.
    pub fn interactive_rebase_resume(&self) -> Result<InteractiveRebaseResume> {
        let operation = self
            .operation_state()?
            .context("There is no rebase to continue")?;
        ensure!(
            operation.kind == OperationKind::Rebase,
            "The current operation is not a rebase"
        );
        let (private, _) = self.git_directories()?;
        ensure!(
            integration::operation_directory(&private, "rebase-merge")?,
            "This rebase uses Git's apply backend. Continue it in your configured Git editor, or Abort/Keep files here; native message editing supports the merge backend."
        );
        let read = |name: &str| integration::operation_file(&private, Path::new(name));
        let message = read("rebase-merge/message")?
            .map(String::from_utf8)
            .transpose()
            .context(
                "This rebase message is not UTF-8; continue in Git using its original encoding",
            )?;
        let done = read("rebase-merge/done")?.unwrap_or_default();
        let last = std::str::from_utf8(&done)?
            .lines()
            .rfind(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .unwrap_or_default();
        let reword = matches!(last.split_whitespace().next(), Some("reword" | "r"));
        let amend = read("rebase-merge/amend")?.map(|bytes| text(trim_line(&bytes)));
        let native_edit = read(MESSAGE_MARKER)?.map(|bytes| text(trim_line(&bytes)));
        let amend_reword = (native_edit.as_ref() == operation.head.as_ref()
            && native_edit.is_some()
            && operation.staged_paths.is_empty())
            || (reword && amend.is_some());
        ensure!(
            !(reword
                && message.is_some()
                && operation.staged_paths.is_empty()
                && native_edit.is_none()
                && amend.is_none()),
            "This interrupted external reword has no confirmed message checkpoint. Use your Git editor to amend its message and continue; native Continue will not silently skip the edit. Abort and Keep files remain available."
        );
        Ok(InteractiveRebaseResume {
            root: self.path.clone(),
            operation,
            message,
            amend_reword,
        })
    }

    pub fn execute_interactive_rebase(
        &self,
        action: &InteractiveRebaseCommand,
    ) -> Result<WriteOutcome> {
        let mut command = normal_command(&self.path);
        let mut editor = MessageEditor::new(None)?;
        command.env("GIT_EDITOR", editor.command()?);
        match action {
            InteractiveRebaseCommand::Start {
                plan,
                steps,
                acknowledge_published,
            } => {
                ensure!(
                    self.path == plan.root,
                    "The reviewed rebase belongs to another worktree"
                );
                plan.validate_steps(steps)?;
                ensure!(
                    &self.interactive_rebase_plan(&plan.base_revision)? == plan,
                    "The branch, base, or remote-tracking references changed; review the sequence again"
                );
                ensure!(
                    plan.known_published_refs.is_empty() || *acknowledge_published,
                    "Acknowledge that these commits are known to remote-tracking references before rewriting them"
                );
                let status = self.status()?;
                ensure!(
                    !status.entries.iter().any(|entry| !entry.untracked),
                    "Commit or stash staged and working changes first; interactive rebase never creates an automatic stash"
                );
                ensure!(
                    !self
                        .worktrees()?
                        .iter()
                        .any(|worktree| worktree.branch.as_ref() == Some(&plan.branch)
                            && worktree.path != self.path),
                    "This branch is also checked out in another worktree; move that worktree to another branch first"
                );
                self.protect_untracked_rebase_paths(&IntegrationPlan {
                    head: plan.head.clone(),
                    branch: plan.branch.clone(),
                    target_ref: plan.base_revision.clone(),
                    target_oid: plan.base.clone(),
                    target_label: plan.base_revision.clone(),
                    ahead: plan.commits.len() as u64,
                    behind: 0,
                    affected_paths: Vec::new(),
                })?;
                let todo = steps
                    .iter()
                    .map(|step| format!("{} {}\n", step.action.label().to_lowercase(), step.oid))
                    .collect::<String>();
                command.env("GIT_SEQUENCE_EDITOR", format!("printf '%s' '{}' >", todo));
                command.args([
                    "rebase",
                    "--interactive",
                    "--force-rebase",
                    "--keep-empty",
                    "--empty=keep",
                    "--no-autostash",
                    "--no-update-refs",
                    "--no-autosquash",
                    "--no-rebase-merges",
                    "--",
                    &plan.base,
                ]);
            }
            InteractiveRebaseCommand::Continue { expected, message } => {
                ensure!(
                    self.path == expected.root && &self.interactive_rebase_resume()? == expected,
                    "The rebase or staged content changed; refresh and review Continue again"
                );
                ensure!(
                    !self.status()?.entries.iter().any(|entry| entry.conflicted),
                    "Resolve and stage every conflicted file before continuing"
                );
                ensure!(
                    message.is_some() == expected.message.is_some(),
                    "Review the pending Git message before continuing"
                );
                if let Some(message) = message {
                    ensure!(
                        !message.trim().is_empty()
                            && !message.contains('\0')
                            && message.len() <= MAX_REBASE_MESSAGE,
                        "Enter a nonempty commit message of at most 1 MiB without NUL bytes"
                    );
                    editor = MessageEditor::new(Some(message))?;
                    command.env("GIT_EDITOR", editor.command()?);
                    if expected.amend_reword {
                        // Git's Continue deliberately skips a failed reword
                        // unless the user first amends it. Keep the real commit
                        // path, author, hooks, editor cleanup and signing.
                        let mut amend = normal_command(&self.path);
                        amend.env("GIT_EDITOR", editor.command()?);
                        amend.args(["commit", "--amend", "--edit", "--allow-empty"]);
                        checked_write_output(amend, None, WRITE_TIMEOUT)?;
                        let (private, _) = self.git_directories()?;
                        if integration::operation_file(&private, Path::new(MESSAGE_MARKER))?
                            .is_some()
                        {
                            std::fs::remove_file(private.join(MESSAGE_MARKER))?;
                        }
                    }
                }
                command.args(["rebase", "--continue"]);
            }
        }
        let result = bounded_write_output(command, None, WRITE_TIMEOUT)?;
        // Only Git's explicit editor refusal is a deliberate native pause.
        // Cancellation, timeouts, hooks and signing failures retain uncertainty.
        let stderr = authentication::redact_current(&text(&result.stderr));
        if !result.status.success()
            && editor.directory.join("message.requested").is_file()
            && self
                .operation_state()?
                .is_some_and(|state| state.kind == OperationKind::Rebase)
        {
            let (private, _) = self.git_directories()?;
            ensure!(
                integration::operation_directory(&private, "rebase-merge")?,
                "The rebase changed while preparing its message editor"
            );
            if integration::operation_file(&private, Path::new(MESSAGE_MARKER))?.is_some() {
                std::fs::remove_file(private.join(MESSAGE_MARKER))?;
            }
            let head = self
                .status()?
                .head
                .context("The rebase no longer has a current commit")?;
            let mut marker = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(private.join(MESSAGE_MARKER))?;
            marker.write_all(head.as_bytes())?;
            drop(editor);
            return Ok(WriteOutcome { message: "Rebase paused for commit-message review. Choose Continue to edit the message in GitTurtle. The operation can also be aborted or resumed after restarting.".into(), commit_oid: self.status()?.head });
        }
        ensure!(
            result.status.success(),
            "Interactive rebase stopped: {}\n{}\nReview Working Changes and the operation state before choosing Continue or Abort. No retry was attempted.",
            stderr.trim(),
            authentication::redact_current(&text(&result.stdout)).trim()
        );
        integration::integration_outcome(self, result)
    }
}

/// One editor response per explicit Continue. Any later message step pauses
/// again instead of silently accepting a squash/reword template. These private
/// files are transient application data, never committed repository content.
struct MessageEditor {
    directory: PathBuf,
}

impl MessageEditor {
    fn new(message: Option<&str>) -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "gitturtle-rebase-message-{}-{timestamp}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&directory)
            .context("Create private native message response")?;
        let editor = Self { directory };
        if let Some(message) = message {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(editor.directory.join("message"))?;
            file.write_all(message.as_bytes())?;
        }
        Ok(editor)
    }

    #[cfg(unix)]
    fn command(&self) -> Result<OsString> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let path = self.directory.join("message");
        let mut quoted = Vec::from(b"'".as_slice());
        for byte in path.as_os_str().as_bytes() {
            if *byte == b'\'' {
                quoted.extend_from_slice(b"'\\''");
            } else {
                quoted.push(*byte);
            }
        }
        quoted.push(b'\'');
        let mut command = Vec::from(
            b"sh -c 'if [ -f \"$1\" ]; then cat \"$1\" > \"$2\" && rm \"$1\"; else printf requested > \"$1.requested\"; exit 1; fi' - "
                .as_slice(),
        );
        command.extend(quoted);
        Ok(OsString::from_vec(command))
    }

    #[cfg(not(unix))]
    fn command(&self) -> Result<OsString> {
        bail!("Native rebase message editing requires a POSIX shell")
    }
}

impl Drop for MessageEditor {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
