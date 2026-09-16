//! Explicit single-file discard: revert one reviewed status entry to HEAD, or
//! delete one reviewed untracked file. Git owns every write.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscardPlan {
    /// The reviewed status row. Execution refuses any other row.
    pub entry: StatusEntry,
    /// HEAD when the plan was prepared. `None` only for an untracked file on
    /// an unborn branch.
    pub head: Option<String>,
    root: PathBuf,
    working: Vec<WorkingIdentity>,
}

/// Working-file identity for stale-review detection. Content is never read.
#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkingIdentity {
    Absent,
    File {
        len: u64,
        modified: Option<std::time::SystemTime>,
    },
    Symlink {
        len: u64,
        modified: Option<std::time::SystemTime>,
    },
    Directory,
    Other,
}

impl DiscardPlan {
    /// Both paths of a rename; one path otherwise.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.entry.paths()
    }
}

impl GitRepository {
    /// Capture the reviewed entry, HEAD, and working-file identities. The plan
    /// refuses conflicted, submodule, and untracked-directory targets.
    pub fn discard_plan(&self, expected: &StatusEntry) -> Result<DiscardPlan> {
        ensure!(!self.bare, "Open a working copy to discard changes.");
        for path in expected.paths() {
            validate_path(&path)?;
        }
        ensure!(
            !expected.conflicted,
            "Resolve this conflict with the conflict tools; discard does not apply to conflicted files."
        );
        let status = self.status()?;
        let entry = status
            .entries
            .iter()
            .find(|entry| {
                entry.path == expected.path && entry.original_path == expected.original_path
            })
            .context("This file no longer appears in Changes. Refresh and review it again.")?;
        ensure!(
            entry == expected,
            "This file's status changed. Refresh Changes and review it again."
        );
        ensure!(
            [&entry.head_mode, &entry.index_mode, &entry.worktree_mode]
                .iter()
                .all(|mode| mode.as_str() != "160000"),
            "Submodule changes are not discarded here. Use Git inside the submodule."
        );
        let head = if self.has_head()? {
            let oid = text(trim_line(&run_git(
                &self.path,
                &["rev-parse", "--verify", "HEAD^{commit}"],
            )?));
            validate_oid(&oid)?;
            Some(oid)
        } else {
            None
        };
        ensure!(
            entry.untracked || head.is_some(),
            "There is no commit to restore from yet. Unstage the file to keep it out of the first commit."
        );
        let mut working = Vec::new();
        for path in entry.paths() {
            let identity = working_identity(&self.path.join(&path))?;
            if entry.untracked {
                ensure!(
                    matches!(
                        identity,
                        WorkingIdentity::File { .. } | WorkingIdentity::Symlink { .. }
                    ),
                    "Untracked directories, including nested repositories, are not deleted here. Remove them outside GitTurtle."
                );
            }
            working.push(identity);
        }
        Ok(DiscardPlan {
            entry: entry.clone(),
            head,
            root: self.path.clone(),
            working,
        })
    }

    pub(super) fn execute_discard(&self, plan: &DiscardPlan) -> Result<WriteOutcome> {
        ensure!(
            plan.root == self.path,
            "The selected repository changed. Review the discard again."
        );
        let fresh = self.discard_plan(&plan.entry)?;
        ensure!(
            fresh == *plan,
            "The file, index, or HEAD changed after review. Review the discard again."
        );
        let paths = plan.paths();
        let mut command = normal_command(&self.path);
        let input = if plan.entry.untracked {
            // `git clean` has no pathspec-file option; the literal path is one
            // argument. Directories were refused during review, so no nested
            // repository or untracked directory can be swept up.
            command
                .args(["--literal-pathspecs", "clean", "--force", "--"])
                .arg(&plan.entry.path);
            None
        } else {
            // A tracked path missing from HEAD is removed from the index and
            // the working folder; a rename restores its source and removes
            // its destination. Configured filters apply to restored content.
            command.args([
                "--literal-pathspecs",
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ]);
            Some(path_input(&paths)?)
        };
        let output = checked_write_output(command, input, WRITE_TIMEOUT).map_err(|error| {
            error.context(if plan.entry.untracked {
                "Deleting the untracked file did not report success. Refresh Changes and inspect the file before another attempt; GitTurtle did not retry."
            } else {
                "Discarding changes did not report success. Git may have restored part of the file. Refresh Changes and inspect the file before another attempt; GitTurtle did not retry."
            })
        })?;
        let status = self.status()?;
        ensure!(
            !status.entries.iter().any(|entry| {
                paths.contains(&entry.path)
                    || entry
                        .original_path
                        .as_ref()
                        .is_some_and(|old| paths.contains(old))
            }),
            "Git reported success, but the file still appears in Changes. Refresh and inspect it before another attempt."
        );
        if plan.entry.untracked {
            ensure!(
                working_identity(&self.path.join(&plan.entry.path))? == WorkingIdentity::Absent,
                "Git reported success, but the untracked file still exists. Inspect it before another attempt."
            );
        }
        let message = if plan.entry.untracked {
            format!("Deleted untracked file {}.", plan.entry.path.display())
        } else if let Some(old) = &plan.entry.original_path {
            format!(
                "Discarded the rename {} → {}. {} matches HEAD again.",
                old.display(),
                plan.entry.path.display(),
                old.display()
            )
        } else {
            format!(
                "Discarded changes to {}. The file matches HEAD.",
                plan.entry.path.display()
            )
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

fn working_identity(path: &Path) -> Result<WorkingIdentity> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            let modified = metadata.modified().ok();
            Ok(if file_type.is_file() {
                WorkingIdentity::File {
                    len: metadata.len(),
                    modified,
                }
            } else if file_type.is_symlink() {
                WorkingIdentity::Symlink {
                    len: metadata.len(),
                    modified,
                }
            } else if file_type.is_dir() {
                WorkingIdentity::Directory
            } else {
                WorkingIdentity::Other
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(WorkingIdentity::Absent),
        Err(error) => Err(error).context("Unable to inspect the working file."),
    }
}
