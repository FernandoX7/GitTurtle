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
    /// Force removal deletes changed, untracked and ignored content and any
    /// unfinished operation state. It still refuses main, current, locked or
    /// missing worktrees and worktrees that hold a Git lock file.
    pub force_removal_blocked: Option<String>,
    root: PathBuf,
    directories: Option<(PathBuf, PathBuf)>,
    directory_identities: Option<[WorktreeDirectoryIdentity; 4]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorktreeDirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    created: std::time::SystemTime,
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
    /// One explicit `--force`: Git deletes dirty content. GitTurtle never
    /// passes the second force that overrides a worktree lock.
    ForceRemove(WorktreeDetails),
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
        ensure!(
            trees.iter().filter(|item| item.path == tree.path).count() == 1,
            "This worktree has ambiguous Git registration paths. Inspect its administration directories before removal."
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
            force_removal_blocked: None,
            root: self.path.clone(),
            directories: None,
            directory_identities: None,
        };
        if missing {
            let reason = "The folder is missing or unavailable. Restore or reconnect it before managing it; GitTurtle does not delete directories or prune metadata to repair missing worktrees.";
            details.removal_blocked = Some(reason.into());
            details.force_removal_blocked = Some(reason.into());
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
            if !main {
                // Git's cleanup follows a symlink at the administration root
                // and can empty a directory outside the registered worktrees.
                // Canonicalization alone loses that redirection information.
                let admin_root = directories.1.join("worktrees");
                ensure!(
                    directories.0.parent() == Some(admin_root.as_path()),
                    "The worktree administration directory is redirected outside this repository's worktrees directory. Inspect it with Git before removal."
                );
                worktree_directory_identity(&admin_root)?;
                // Keep filesystem metadata separate from the returned plan:
                // only the regular-file check contributes to its validation.
                let git_file = match std::fs::symlink_metadata(tree.path.join(".git")) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        return Err(error)
                            .context("Unable to inspect the linked worktree .git file");
                    }
                };
                ensure!(
                    git_file.is_file(),
                    "The linked worktree .git path must be a regular file, not a symbolic link or directory."
                );
            }
            details.directory_identities = Some([
                worktree_directory_identity(&self.path)?,
                worktree_directory_identity(&tree.path)?,
                worktree_directory_identity(&directories.0)?,
                worktree_directory_identity(&directories.1)?,
            ]);
            let status = repo.status()?;
            details.changed_files = status.entries.len();
            // Both status and ordinary `git worktree remove` can overlook
            // modified files marked assume-unchanged or skip-worktree. Never
            // treat an index that skips these checks as proof of a clean tree.
            let index = run_git(&repo.path, &["ls-files", "-v", "-z"])?;
            let unchecked_index = index
                .split(|byte| *byte == 0)
                .filter_map(|entry| entry.first())
                .any(|tag| *tag == b'S' || tag.is_ascii_lowercase());
            // A held lock file can belong to a running Git process, so it
            // blocks force removal as well. Bisect and sequencer state is
            // unfinished work that force removal deliberately discards.
            let mut lock_files = false;
            let mut operation_state = false;
            for (file, lock) in [
                ("index.lock", true),
                ("HEAD.lock", true),
                ("config.worktree.lock", true),
                ("BISECT_START", false),
                ("sequencer", false),
            ] {
                match std::fs::symlink_metadata(directories.0.join(file)) {
                    Ok(_) if lock => lock_files = true,
                    Ok(_) => operation_state = true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).context("Unable to inspect worktree Git state");
                    }
                }
            }
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
            if lock_files {
                let reason = "This worktree holds a Git lock file (index.lock, HEAD.lock or config.worktree.lock). Another Git process may be active. Inspect and clear the lock with Git before removal; GitTurtle never removes locks, even with force removal.";
                details.removal_blocked = Some(reason.into());
                details.force_removal_blocked = Some(reason.into());
            } else if operation_state
                || status.operation.is_some()
                || status.entries.iter().any(|entry| entry.conflicted)
            {
                details.removal_blocked = Some("Finish the active Git operation, bisect, or sequencer state and resolve conflicts before removing this worktree. Force removal discards that unfinished state.".into());
            } else if details.changed_files > 0 || details.ignored_files > 0 {
                details.removal_blocked = Some("This worktree contains changed, untracked, or ignored files. Commit, stash, or move that content before removal. Force removal deletes it.".into());
            } else if unchecked_index {
                details.removal_blocked = Some("This worktree has assume-unchanged or skip-worktree entries, which can hide local changes. Review those files and sparse-checkout settings with Git before removal. Force removal deletes any hidden changes.".into());
            }
        }
        let protected = if main {
            Some("The main worktree cannot be removed. Only linked worktrees can be removed here.")
        } else if current {
            Some(
                "This is the worktree currently open in GitTurtle. Open another worktree before removing it.",
            )
        } else if tree.locked {
            Some(
                "This worktree is locked. Unlock it with Git after reviewing why it was locked; GitTurtle never overrides a worktree lock, even with force removal.",
            )
        } else {
            None
        };
        if let Some(reason) = protected {
            details.removal_blocked = Some(reason.into());
            details.force_removal_blocked = Some(reason.into());
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
            WorktreeCommand::Remove(plan) | WorktreeCommand::ForceRemove(plan) => {
                let force = matches!(operation, WorktreeCommand::ForceRemove(_));
                ensure!(
                    plan.root == self.path,
                    "The selected repository changed. Review removal again."
                );
                let fresh = self.worktree_details(&plan.tree)?;
                ensure!(
                    fresh == *plan,
                    "The worktree identity or content changed after review. Review removal again."
                );
                let blocked = if force {
                    &fresh.force_removal_blocked
                } else {
                    &fresh.removal_blocked
                };
                ensure!(
                    blocked.is_none(),
                    "{}",
                    blocked.as_deref().unwrap_or_default()
                );
                // Ordinary removal passes no --force. Force removal passes one
                // --force so Git deletes dirty content; a locked worktree needs
                // Git's second force and is refused above. Neither variant
                // deletes directories itself, prunes metadata, or deletes branches.
                command.args(["worktree", "remove"]);
                if force {
                    command.arg("--force");
                }
                command.arg("--").arg(&plan.tree.path);
                let kind = if plan.tree.branch.is_some() {
                    "worktree"
                } else {
                    "detached worktree"
                };
                let retained = if plan.tree.branch.is_some() {
                    " Its branch remains available."
                } else {
                    ""
                };
                if force {
                    format!(
                        "Force removed {kind} at {} and deleted {} changed or untracked and {} ignored files.{retained}",
                        plan.tree.path.display(),
                        plan.changed_files,
                        plan.ignored_files
                    )
                } else {
                    format!("Removed {kind} at {}.{retained}", plan.tree.path.display())
                }
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
                let context = if matches!(operation, WorktreeCommand::ForceRemove(_)) {
                    "Worktree force removal did not report success. Git may already have deleted some files or registration metadata. Refresh and inspect the worktree before another attempt; GitTurtle did not retry or delete remaining content."
                } else {
                    "Worktree removal did not report success. Git may already have removed some files or registration metadata. Refresh and inspect the worktree before another attempt; GitTurtle did not retry, force removal, or delete remaining content."
                };
                return Err(error.context(context));
            }
        };
        if let WorktreeCommand::Remove(plan) | WorktreeCommand::ForceRemove(plan) = operation {
            self.verify_worktree_removal(plan).context("Git reported worktree removal success, but cleanup could not be verified. Refresh and inspect the worktree and its registration before another attempt; GitTurtle did not retry or delete remaining content.")?;
        }
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

    fn verify_worktree_removal(&self, plan: &WorktreeDetails) -> Result<()> {
        ensure!(
            !self
                .worktrees()?
                .iter()
                .any(|tree| tree.path == plan.tree.path),
            "The worktree is still registered with Git."
        );
        let (private, _) = plan
            .directories
            .as_ref()
            .context("The reviewed worktree administration directory is unavailable.")?;
        for (path, label) in [
            (&plan.tree.path, "worktree folder"),
            (private, "private Git administration directory"),
        ] {
            match std::fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("Unable to inspect the {label}."));
                }
                Ok(_) => bail!("The {label} still exists."),
            }
        }
        Ok(())
    }
}

fn worktree_directory_identity(path: &Path) -> Result<WorktreeDirectoryIdentity> {
    let metadata =
        std::fs::symlink_metadata(path).context("Unable to inspect worktree directory identity")?;
    ensure!(
        metadata.is_dir(),
        "Worktree and administration directories must be directories, not symbolic links."
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(WorktreeDirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        Ok(WorktreeDirectoryIdentity {
            created: metadata
                .created()
                .context("Unable to establish worktree directory identity")?,
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
    fn removal_verification_requires_registration_folder_and_private_metadata_to_be_gone() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("main");
        GitRepository::init(&root, "main").unwrap();
        let repo = GitRepository::open(&root).unwrap();
        let main = repo.worktrees().unwrap().remove(0);
        let mut plan = repo.worktree_details(&main).unwrap();
        assert!(
            repo.verify_worktree_removal(&plan)
                .unwrap_err()
                .to_string()
                .contains("still registered")
        );

        let removed_worktree = temp.path().join("removed-worktree");
        plan.tree.path = removed_worktree.clone();
        let private = temp.path().join("private-metadata");
        plan.directories.as_mut().unwrap().0 = private.clone();
        std::fs::create_dir(&removed_worktree).unwrap();
        assert!(
            repo.verify_worktree_removal(&plan)
                .unwrap_err()
                .to_string()
                .contains("worktree folder still exists")
        );
        std::fs::remove_dir(&removed_worktree).unwrap();
        std::fs::create_dir(&private).unwrap();
        assert!(
            repo.verify_worktree_removal(&plan)
                .unwrap_err()
                .to_string()
                .contains("administration directory still exists")
        );
        std::fs::remove_dir(&private).unwrap();
        repo.verify_worktree_removal(&plan).unwrap();

        #[cfg(unix)]
        {
            // A replaced path can be a dangling symlink: exists()/is_dir()
            // would wrongly report successful filesystem cleanup here.
            std::os::unix::fs::symlink(temp.path().join("missing"), &removed_worktree).unwrap();
            assert!(repo.verify_worktree_removal(&plan).is_err());
        }
    }

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
