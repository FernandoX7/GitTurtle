//! Reviewed worktree creation/removal. Git alone owns checkout and cleanup.
use super::*;

thread_local! {
    static INSPECTION_CANCELLATION: std::cell::RefCell<Option<HistoryCancellation>> = const { std::cell::RefCell::new(None) };
}

/// A scoped cancellation boundary for management dialogs. Passive command
/// policy remains unchanged; cancellation kills and reaps an active Git read.
pub fn run_cancellable_inspection<T>(
    cancellation: HistoryCancellation,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    struct Restore(Option<HistoryCancellation>);
    impl Drop for Restore {
        fn drop(&mut self) {
            INSPECTION_CANCELLATION.set(self.0.take());
        }
    }
    let _restore = Restore(INSPECTION_CANCELLATION.replace(Some(cancellation)));
    ensure!(!inspection_cancelled(), "Repository inspection cancelled");
    operation()
}

pub(crate) fn inspection_cancelled() -> bool {
    INSPECTION_CANCELLATION.with_borrow(|value| {
        value
            .as_ref()
            .is_some_and(HistoryCancellation::is_cancelled)
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeDetails {
    pub tree: Worktree,
    pub main: bool,
    pub current: bool,
    pub missing: bool,
    pub changed_files: usize,
    /// Ignored files also contain user data and prevent removal.
    pub ignored_files: usize,
    pub removal_blocked: Option<String>,
    root: PathBuf,
    directories: Option<(PathBuf, PathBuf)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateWorktreePlan {
    pub destination: PathBuf,
    pub branch: String,
    pub new_branch: bool,
    pub target_oid: String,
    root: PathBuf,
    directories: (PathBuf, PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeCommand {
    Create(CreateWorktreePlan),
    Remove(WorktreeDetails),
}

impl GitRepository {
    /// Read detailed status only for the selected worktree, never fan out a
    /// repository-wide status scan just to render the navigator.
    pub fn worktree_details(&self, expected: &Worktree) -> Result<WorktreeDetails> {
        let trees = self.worktrees()?;
        let tree = trees
            .iter()
            .find(|tree| tree.path == expected.path)
            .context("This worktree is no longer registered. Refresh the list.")?;
        ensure!(
            tree == expected,
            "The worktree branch, commit, or lock changed. Refresh the list and review it again."
        );
        let main = trees.first().is_some_and(|first| first.path == tree.path);
        let current = tree.path == self.path;
        let missing = !tree.path.is_dir();
        let mut details = WorktreeDetails {
            tree: tree.clone(),
            main,
            current,
            missing,
            changed_files: 0,
            ignored_files: 0,
            removal_blocked: None,
            root: self.path.clone(),
            directories: None,
        };
        if missing {
            details.removal_blocked = Some("The folder is missing or unavailable. Restore or reconnect it before managing it; GitTurtle does not delete directories or prune metadata to repair missing worktrees.".into());
        } else {
            let repo = GitRepository::open(&tree.path)?;
            ensure!(
                repo.path == tree.path,
                "The worktree folder now resolves to a different repository."
            );
            let directories = repo.git_directories()?;
            ensure!(
                directories.1 == self.git_directories()?.1,
                "The worktree folder belongs to a different repository."
            );
            let status = repo.status()?;
            details.changed_files = status.entries.len();
            // git worktree remove permits ignored files. Refuse those as well;
            // users must explicitly move/remove their content outside this UI.
            let mut command = passive_status_command(repo.path())?;
            command.args([
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
            ]);
            let output = bounded_output(command, GIT_TIMEOUT)?;
            ensure!(
                output.status.success(),
                "Unable to inspect ignored worktree content: {}",
                text(&output.stderr)
            );
            details.ignored_files = output
                .stdout
                .split(|b| *b == 0)
                .filter(|p| !p.is_empty())
                .count();
            details.directories = Some(directories);
            if status.operation.is_some() || status.entries.iter().any(|entry| entry.conflicted) {
                details.removal_blocked = Some("Finish the active Git operation and resolve conflicts before removing this worktree.".into());
            } else if details.changed_files > 0 || details.ignored_files > 0 {
                details.removal_blocked = Some("This worktree contains changed, untracked, or ignored files. Commit, stash, or move that content before removal.".into());
            }
        }
        if main {
            details.removal_blocked = Some(
                "The main worktree cannot be removed. Only linked worktrees can be removed here."
                    .into(),
            );
        } else if current {
            details.removal_blocked = Some("This is the worktree currently open in GitTurtle. Open another worktree before removing it.".into());
        } else if tree.locked {
            details.removal_blocked = Some("This worktree is locked. Unlock it with Git after reviewing why it was locked; GitTurtle never forces removal.".into());
        }
        Ok(details)
    }

    pub fn create_worktree_plan(
        &self,
        destination: &Path,
        branch: &str,
        new_branch: bool,
        start_point: &str,
    ) -> Result<CreateWorktreePlan> {
        ensure!(
            !self.bare,
            "Open a working repository before creating a linked worktree."
        );
        self.validate_branch(branch)?;
        let destination = worktree_destination(destination)?;
        let trees = self.worktrees()?;
        ensure!(
            !trees.iter().any(|tree| tree.path == destination),
            "Git already has a worktree registered at this destination."
        );
        ensure!(
            !trees
                .iter()
                .any(|tree| tree.branch.as_deref() == Some(branch)),
            "Branch '{branch}' is already checked out in another worktree. Choose another branch or create a new one."
        );
        let reference = format!("refs/heads/{branch}");
        let branches = self.branches()?;
        let existing = branches
            .iter()
            .find(|item| !item.remote && item.name == branch);
        let revision = if new_branch {
            ensure!(
                existing.is_none(),
                "Branch '{branch}' already exists. Choose Existing branch or use another name."
            );
            ensure!(
                !start_point.is_empty()
                    && start_point.len() <= 4096
                    && !start_point.starts_with('-')
                    && !start_point.contains('\0'),
                "Choose a local commit revision for the new branch."
            );
            start_point
        } else {
            ensure!(
                existing.is_some(),
                "Local branch '{branch}' no longer exists. Refresh and choose another branch."
            );
            &reference
        };
        let target_oid = text(trim_line(&run_git(
            &self.path,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{revision}^{{commit}}"),
            ],
        )?));
        validate_oid(&target_oid)?;
        Ok(CreateWorktreePlan {
            destination,
            branch: branch.into(),
            new_branch,
            target_oid,
            root: self.path.clone(),
            directories: self.git_directories()?,
        })
    }

    pub(super) fn execute_worktree(&self, operation: &WorktreeCommand) -> Result<WriteOutcome> {
        let mut command = normal_command(&self.path);
        let message = match operation {
            WorktreeCommand::Create(plan) => {
                ensure!(
                    plan.root == self.path && self.git_directories()? == plan.directories,
                    "The repository identity changed. Review the worktree again."
                );
                let fresh = self.create_worktree_plan(
                    &plan.destination,
                    &plan.branch,
                    plan.new_branch,
                    &plan.target_oid,
                )?;
                ensure!(
                    fresh == *plan,
                    "The destination or branch changed after review. Review the worktree again."
                );
                command.args(["worktree", "add"]);
                if plan.new_branch {
                    command.args(["--no-track", "-b", &plan.branch]);
                }
                command.arg("--").arg(&plan.destination);
                if plan.new_branch {
                    command.arg(&plan.target_oid);
                } else {
                    command.arg(&plan.branch);
                }
                format!(
                    "Created worktree for {} at {}",
                    plan.branch,
                    plan.destination.display()
                )
            }
            WorktreeCommand::Remove(plan) => {
                ensure!(
                    plan.root == self.path,
                    "The selected repository changed. Review removal again."
                );
                let fresh = self.worktree_details(&plan.tree)?;
                ensure!(
                    fresh == *plan,
                    "The worktree identity or content changed after review. Review removal again."
                );
                ensure!(
                    fresh.removal_blocked.is_none(),
                    "{}",
                    fresh.removal_blocked.as_deref().unwrap_or_default()
                );
                // No --force, directory deletion, metadata pruning, or branch deletion.
                command
                    .args(["worktree", "remove", "--"])
                    .arg(&plan.tree.path);
                format!(
                    "Removed worktree at {}. Its branch remains available.",
                    plan.tree.path.display()
                )
            }
        };
        let output = match checked_write_output(command, None, WRITE_TIMEOUT) {
            Ok(output) => output,
            Err(error) => {
                if let WorktreeCommand::Create(plan) = operation {
                    let retained_branch = self.branches().ok().and_then(|branches| {
                        branches
                            .into_iter()
                            .find(|branch| !branch.remote && branch.name == plan.branch)
                    });
                    let summary = if let Some(branch) = retained_branch {
                        format!(
                            "Worktree creation did not report success. Branch '{}' remains at {}. Inspect the destination and refresh worktrees before another attempt; GitTurtle did not retry or delete the branch.",
                            branch.name, branch.oid
                        )
                    } else {
                        "Worktree creation did not report success. Git may have created a branch or part of the destination. Refresh and inspect before another attempt; GitTurtle did not retry or clean up user content.".into()
                    };
                    return Err(error.context(summary));
                }
                return Err(error);
            }
        };
        let diagnostic = format!("{}{}", text(&output.stdout), text(&output.stderr));
        Ok(WriteOutcome {
            message: if diagnostic.trim().is_empty() {
                message
            } else {
                format!("{message}\n{}", diagnostic.trim())
            },
            commit_oid: None,
        })
    }
}

fn worktree_destination(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "Choose an absolute destination folder.");
    ensure!(
        path.components().all(|component| matches!(
            component,
            Component::RootDir | Component::Prefix(_) | Component::Normal(_)
        )),
        "The destination cannot contain '.' or '..' components."
    );
    let name = path
        .file_name()
        .context("Choose a new folder inside an existing parent folder.")?;
    let parent = path
        .parent()
        .context("Choose a destination parent folder.")?
        .canonicalize()
        .context("The destination parent folder does not exist or is unavailable.")?;
    let destination = parent.join(name);
    ensure!(
        std::fs::symlink_metadata(&destination)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
        "The destination already exists or cannot be inspected. Choose a new, unused folder; existing folders are never replaced."
    );
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelling_inspection_terminates_the_active_process_and_clears_scope() {
        let cancellation = HistoryCancellation::default();
        let signal = cancellation.clone();
        let trigger = thread::spawn(move || {
            thread::sleep(Duration::from_millis(60));
            signal.cancel();
        });
        let started = Instant::now();
        let error = run_cancellable_inspection(cancellation, || {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 10"]);
            bounded_output(command, Duration::from_secs(15))
        })
        .unwrap_err();
        trigger.join().unwrap();
        assert!(error.to_string().contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!inspection_cancelled());
    }
}
