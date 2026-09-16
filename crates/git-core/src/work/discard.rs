//! Explicit single-file discard: revert one reviewed status entry to HEAD, or
//! delete one reviewed untracked file. Git owns every write.
use super::*;

#[cfg(unix)]
use rustix::fs::{AtFlags, FileType, Mode, OFlags, Stat, fstat, open, openat, readlinkat, statat};

const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscardPlan {
    /// The reviewed status row. Execution refuses any other row.
    pub entry: StatusEntry,
    /// HEAD when the plan was prepared. `None` only for an untracked file on
    /// an unborn branch.
    pub head: Option<String>,
    root: PathBuf,
    directories: [DirectoryIdentity; 3],
    working: Vec<WorkingIdentity>,
}

/// Bounded raw content and filesystem identity for stale-review detection.
/// Symlinks capture their stored target text, never the linked destination.
#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkingIdentity {
    Absent,
    File { metadata: String, digest: [u8; 32] },
    Symlink { metadata: String, digest: [u8; 32] },
    Directory,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryIdentity {
    path: PathBuf,
    device: u64,
    inode: u64,
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
        // A cached repository path does not pin Git's effective worktree:
        // core.worktree or a replaced .git file can redirect later commands.
        let selected = GitRepository::open(&self.path)?;
        ensure!(
            selected.path == self.path && !selected.bare,
            "The selected repository changed. Reopen it and review the discard again."
        );
        let git_directories = selected.git_directories()?;
        let directories = [
            directory_identity(&self.path)?,
            directory_identity(&git_directories.0)?,
            directory_identity(&git_directories.1)?,
        ];
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
                entry.path == expected.path
                    && entry.original_path == expected.original_path
                    && entry.untracked == expected.untracked
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
        if !entry.untracked {
            ensure_single_file_restore(
                &self.path,
                head.as_deref().context("Missing reviewed HEAD")?,
                &entry.paths(),
            )?;
        }
        let mut working = Vec::new();
        let started = Instant::now();
        for path in entry.paths() {
            let identity = working_identity(&self.path, &path, started)?;
            let present = matches!(
                identity,
                WorkingIdentity::File { .. } | WorkingIdentity::Symlink { .. }
            );
            if entry.untracked {
                ensure!(
                    present,
                    "Untracked directories, including nested repositories, are not deleted here. Remove them outside GitTurtle."
                );
            } else {
                // Git would replace a directory or special file at the path
                // and delete everything under it, including untracked work.
                ensure!(
                    present || identity == WorkingIdentity::Absent,
                    "The path {} is now a directory or a special file. Git would delete everything under it. Move or remove it with Git before discarding.",
                    path.display()
                );
                // Git records the path as deleted or renamed away, so a file
                // there is untracked content that restore would overwrite.
                let git_expects_absent = entry.original_path.as_deref() == Some(path.as_path())
                    || entry.worktree_mode == "000000";
                ensure!(
                    !(present && git_expects_absent),
                    "An untracked file occupies {} while Git records it as deleted or renamed away. Discard would overwrite that file with the last commit. Delete its untracked row first, or unstage the change to keep the file.",
                    path.display()
                );
            }
            working.push(identity);
        }
        Ok(DiscardPlan {
            entry: entry.clone(),
            head,
            root: self.path.clone(),
            directories,
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
        // Keep normal write configuration (including filters), but bind its
        // target and raw objects to the same repository and commit reviewed by
        // passive reads. Replacement refs must not substitute another tree.
        command
            .arg("--git-dir")
            .arg(&plan.directories[1].path)
            .arg("--work-tree")
            .arg(&plan.root)
            .env("GIT_NO_REPLACE_OBJECTS", "1");
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
            command
                .args(["--literal-pathspecs", "restore", "--source"])
                .arg(plan.head.as_deref().context("Missing reviewed HEAD")?)
                .args([
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
                entry.untracked == plan.entry.untracked
                    && (paths.contains(&entry.path)
                        || entry
                            .original_path
                            .as_ref()
                            .is_some_and(|old| paths.contains(old)))
            }),
            "Git reported success, but the file still appears in Changes. Refresh and inspect it before another attempt."
        );
        if plan.entry.untracked {
            ensure!(
                working_identity(&self.path, &plan.entry.path, Instant::now())?
                    == WorkingIdentity::Absent,
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

/// Even literal pathspecs select descendants. Refuse file/tree transitions
/// whose restore would also change rows outside the reviewed file or rename.
fn ensure_single_file_restore(root: &Path, head: &str, paths: &[PathBuf]) -> Result<()> {
    let mut command = git_command(root);
    command
        .arg("--literal-pathspecs")
        .args(["ls-tree", "-z", "--full-tree", head, "--"])
        .args(paths);
    let output = bounded_output(command, GIT_TIMEOUT)?;
    ensure!(
        output.status.success(),
        "Unable to inspect the reviewed commit's restore paths: {}",
        text(&output.stderr).trim()
    );
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .context("Malformed restore tree entry")?;
        let path = path_from_bytes(&record[tab + 1..]);
        let fields: Vec<_> = record[..tab].split(|byte| *byte == b' ').collect();
        ensure!(fields.len() == 3, "Malformed restore tree metadata");
        ensure!(
            fields[1] != b"tree" && paths.contains(&path),
            "The path {} is a directory in the reviewed commit. Discard would also restore other files beneath it. Review this file/directory replacement with Git.",
            path.display()
        );
    }
    let mut command = git_command(root);
    command
        .arg("--literal-pathspecs")
        .args(["ls-files", "--cached", "-z", "--"])
        .args(paths);
    let output = bounded_output(command, GIT_TIMEOUT)?;
    ensure!(
        output.status.success(),
        "Unable to inspect the selected index paths: {}",
        text(&output.stderr).trim()
    );
    for record in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let path = path_from_bytes(record);
        ensure!(
            paths.contains(&path),
            "There are staged files beneath the selected path ({}). Discard would also remove those files from the index. Review this file/directory replacement with Git.",
            path.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
fn directory_identity(path: &Path) -> Result<DirectoryIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path)
        .context("Unable to inspect the selected repository directory")?;
    ensure!(
        metadata.is_dir(),
        "The selected repository changed. Its working and administration folders must be real directories."
    );
    Ok(DirectoryIdentity {
        path: path.to_owned(),
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn directory_identity(_path: &Path) -> Result<DirectoryIdentity> {
    bail!("Safe discard review currently requires macOS or Linux")
}

fn check_snapshot(started: Instant) -> Result<()> {
    ensure!(
        !inspection_cancelled()
            && !authentication::current_control().is_some_and(|control| control.is_cancelled()),
        "Discard review cancelled"
    );
    ensure!(
        started.elapsed() <= SNAPSHOT_TIMEOUT,
        "Discard review exceeded its 5-second inspection limit. Review the file with Git before discarding."
    );
    Ok(())
}

/// Open every component relative to a no-follow directory descriptor. An
/// absent ancestor implies an absent file; any other unsafe parent is refused.
#[cfg(unix)]
fn working_identity(root: &Path, path: &Path, started: Instant) -> Result<WorkingIdentity> {
    check_snapshot(started)?;
    validate_path(path)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open(root, flags, Mode::empty())
        .context("Unable to safely open the working folder for discard review")?;
    let mut current = PathBuf::new();
    let components: Vec<_> = path.components().collect();
    for component in &components[..components.len() - 1] {
        check_snapshot(started)?;
        current.push(component);
        match openat(&directory, component.as_os_str(), flags, Mode::empty()) {
            Ok(parent) => directory = parent,
            Err(rustix::io::Errno::NOENT) => return Ok(WorkingIdentity::Absent),
            Err(error) => return Err(error).with_context(|| format!(
                "Unable to safely inspect the parent path {}. It may be unreadable, a file, or a symbolic link. Inspect it before discarding {}.",
                current.display(),
                path.display()
            )),
        }
    }
    let name = components.last().context("Missing filename")?.as_os_str();
    let before = match statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => return Ok(WorkingIdentity::Absent),
        Err(error) => return Err(error).context("Unable to inspect the working file"),
    };
    let metadata = stat_identity(&before);
    let identity = match FileType::from_raw_mode(before.st_mode) {
        FileType::RegularFile => {
            let length = u64::try_from(before.st_size).context("Invalid working file size")?;
            ensure!(
                length <= MAX_BLOB_BYTES as u64,
                "The file exceeds the 64 MiB discard review limit. Review and discard it with Git."
            );
            // NONBLOCK prevents a replacement FIFO from hanging open. Verify
            // type and identity before reading through this descriptor.
            let fd = openat(
                &directory,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .context("Unable to safely read the working file for discard review")?;
            let opened = fstat(&fd)?;
            ensure!(
                FileType::from_raw_mode(opened.st_mode) == FileType::RegularFile
                    && metadata == stat_identity(&opened),
                "The file changed during discard review. Refresh and review it again."
            );
            let mut file = std::fs::File::from(fd);
            let mut contents = Sha256::new();
            let mut bytes = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                check_snapshot(started)?;
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                bytes += count as u64;
                ensure!(
                    bytes <= MAX_BLOB_BYTES as u64,
                    "The file grew beyond the 64 MiB discard review limit. Review and discard it with Git."
                );
                contents.update(&buffer[..count]);
            }
            ensure!(
                bytes == length && metadata == stat_identity(&fstat(&file)?),
                "The file changed during discard review. Refresh and review it again."
            );
            WorkingIdentity::File {
                metadata: metadata.clone(),
                digest: contents.finalize().into(),
            }
        }
        FileType::Symlink => {
            let target = readlinkat(&directory, name, Vec::new())
                .context("Unable to read the stored symbolic-link target for discard review")?;
            WorkingIdentity::Symlink {
                metadata: metadata.clone(),
                digest: Sha256::digest(target.to_bytes()).into(),
            }
        }
        FileType::Directory => WorkingIdentity::Directory,
        _ => WorkingIdentity::Other,
    };
    ensure!(
        metadata == stat_identity(&statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW)?),
        "The file changed during discard review. Refresh and review it again."
    );
    check_snapshot(started)?;
    Ok(identity)
}

/// Access time is excluded because the passive snapshot can update it. Mode
/// is included even when core.filemode or index flags hide executable changes.
#[cfg(unix)]
fn stat_identity(stat: &Stat) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        stat.st_dev,
        stat.st_ino,
        stat.st_mode,
        stat.st_size,
        stat.st_mtime,
        stat.st_mtime_nsec,
        stat.st_ctime,
        stat.st_ctime_nsec
    )
}

#[cfg(not(unix))]
fn working_identity(_root: &Path, _path: &Path, _started: Instant) -> Result<WorkingIdentity> {
    bail!("Safe discard review currently requires macOS or Linux")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn discard_snapshot_observes_cancellation_and_deadline() {
        let cancellation = HistoryCancellation::default();
        let error = run_cancellable_inspection(cancellation.clone(), || {
            check_snapshot(Instant::now())?;
            cancellation.cancel();
            check_snapshot(Instant::now())
        })
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"));

        let control = OperationControl::default();
        let error = run_controlled(control.clone(), || {
            control.cancel();
            check_snapshot(Instant::now())
        })
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        assert!(check_snapshot(Instant::now()).is_ok());

        let expired = Instant::now() - SNAPSHOT_TIMEOUT - Duration::from_secs(1);
        let error = check_snapshot(expired).unwrap_err();
        assert!(error.to_string().contains("5-second inspection limit"));
    }

    #[test]
    fn discard_snapshot_refuses_special_files_without_opening_them() {
        let temp = tempfile::TempDir::new().unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(temp.path().join("fifo"))
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            working_identity(temp.path(), Path::new("fifo"), Instant::now()).unwrap(),
            WorkingIdentity::Other
        );
    }
}
