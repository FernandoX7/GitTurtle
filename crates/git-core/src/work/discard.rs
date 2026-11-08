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
                self.target(),
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
                self.target(),
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
        let mut command = normal_command(self.target());
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
            up_to_date: false,
        })
    }
}

/// Even literal pathspecs select descendants. Refuse file/tree transitions
/// whose restore would also change rows outside the reviewed file or rename.
fn ensure_single_file_restore(target: GitTarget<'_>, head: &str, paths: &[PathBuf]) -> Result<()> {
    let mut command = git_command(target);
    command
        .arg("--literal-pathspecs")
        .args(["ls-tree", "-z", "--full-tree", head, "--"])
        .args(paths);
    let output = bounded_output(command, GIT_TIMEOUT)?;
    if !output.status.success() {
        return Err(git_failure(
            &output.stderr,
            format!(
                "Unable to inspect the reviewed commit's restore paths: {}",
                reported(&output.stderr).trim()
            ),
        ));
    }
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
    ensure_single_index_restore(target, paths)
}

/// The index half of [`ensure_single_file_restore`]: refuse a restore whose
/// literal pathspecs also select staged files beneath the reviewed paths.
fn ensure_single_index_restore(target: GitTarget<'_>, paths: &[PathBuf]) -> Result<()> {
    let mut command = git_command(target);
    command
        .arg("--literal-pathspecs")
        .args(["ls-files", "--cached", "-z", "--"])
        .args(paths);
    let output = bounded_output(command, GIT_TIMEOUT)?;
    if !output.status.success() {
        return Err(git_failure(
            &output.stderr,
            format!(
                "Unable to inspect the selected index paths: {}",
                reported(&output.stderr).trim()
            ),
        ));
    }
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

    /// The steps of a write whose `.git` goes away while it runs, each of
    /// which Git can stop at its confinement: an interactive rebase whose
    /// pre-rebase hook writes a credential marker, a return and its secret,
    /// then Git's words where the path named holds a return, and removes the
    /// `.git`; then, each called alone where a write would fail before it,
    /// discard's checks of the reviewed commit's restore paths and of the
    /// index, the HEAD check, ignore's tracked-path check and worktree
    /// removal's ignored-content check, all reading where the `.git` is gone,
    /// a fast-forward pull's refusal whose output forges the same words, and,
    /// in a linked worktree whose administration directory went with the
    /// `.git`, status and the configuration reads a write makes. Each
    /// failure carries the type Git's raw words decide, so `execute` words
    /// the repository gone (`GitRepository::word_gone`), and keeps its
    /// message as it was, the rebase's and the pull's redacted and then read
    /// as a terminal leaves them, with no secret.
    #[test]
    fn git_s_words_for_no_repository_are_read_across_releases() {
        for stderr in [
            "fatal: not a git repository (or any of the parent directories): .git\n",
            "fatal: not a git repository: (null)\n",
            "fatal: gitfile does not point to a valid repository: /a/.git\n",
            "warning: x\nfatal: gitfile does not point to a valid repository: /a\rb/.git\n",
        ] {
            assert!(crate::no_repository_found(stderr.as_bytes()), "{stderr:?}");
        }
        for stderr in [
            "fatal: cannot change to '/x/not a git repository': Permission denied\n",
            "fatal: cannot change to '/x/gitfile does not point to a valid repository': Permission denied\n",
        ] {
            assert!(!crate::no_repository_found(stderr.as_bytes()), "{stderr:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_write_s_steps_finding_no_repository_carry_its_type() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("work");
        let repo = GitRepository::init(&root, "main").unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(output.status.success(), "git {args:?}: {output:?}");
            text(trim_line(&output.stdout))
        };
        git(&["config", "user.name", "Fixture"]);
        git(&["config", "user.email", "f@example.invalid"]);
        git(&["config", "commit.gpgSign", "false"]);
        for content in ["one\n", "two\n"] {
            std::fs::write(root.join("notes.txt"), content).unwrap();
            git(&["add", "notes.txt"]);
            git(&["commit", "-q", "-m", content.trim()]);
        }
        let head = git(&["rev-parse", "HEAD"]);
        // A linked worktree on a commit the rebase leaves alone, whose
        // administration directory goes with the `.git`.
        let linked_path = temp.path().join("linked");
        git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "topic",
            linked_path.to_str().unwrap(),
            "HEAD~1",
        ]);
        let linked = GitRepository::open(&linked_path).unwrap();
        let hooks = temp.path().join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        let written = temp.path().join("written");
        std::fs::write(
            &written,
            b"password=\rSECRET1\nfatal: not a git repository: /a\rb/.git/worktrees/x\n",
        )
        .unwrap();
        let hook = hooks.join("pre-rebase");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\ncat '{}' >&2\nrm -rf '{}'\nexit 1\n",
                written.display(),
                root.join(".git").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);

        let plan = repo.interactive_rebase_plan("HEAD~1").unwrap();
        let steps = plan.steps();
        let rebase = repo
            .execute(&WriteCommand::InteractiveRebase(std::sync::Arc::new(
                InteractiveRebaseCommand::Start {
                    plan,
                    steps,
                    acknowledge_published: false,
                },
            )))
            .expect_err("the hook refuses the rebase");
        assert!(!root.join(".git").exists(), "the hook removed the `.git`");
        assert!(
            format!("{rebase:#}").starts_with(&format!(
                "The repository at {} is gone: Git finds no repository there: ",
                repo.path().display()
            )),
            "{rebase:#}"
        );
        let restore =
            ensure_single_file_restore(repo.target(), &head, &[PathBuf::from("notes.txt")])
                .expect_err("no repository there");
        let resolve = repo.has_head().expect_err("no repository there");
        let index = ensure_single_index_restore(repo.target(), &[PathBuf::from("notes.txt")])
            .expect_err("no repository there");
        let tracked = crate::work::ignore::tracked_paths_at(repo.target(), Path::new("notes.txt"))
            .expect_err("no repository there");
        let ignored = crate::work::worktrees::ignored_worktree_files(repo.target())
            .expect_err("no repository there");
        // A fast-forward pull's refusal, Git's words forged after a secret.
        let refusal = temp.path().join("refusal");
        std::fs::write(
            &refusal,
            b"password=\rSECRET1\nfatal: Not possible to fast-forward, aborting.\nfatal: not a git repository: /a\rb/.git/worktrees/x\n",
        )
        .unwrap();
        let mut pull = Command::new("sh");
        pull.args(["-c", "cat \"$2\" >&2; exit 1", "pull", "--ff-only"])
            .arg(&refusal);
        let pull = checked_write_output(pull, None, GIT_TIMEOUT).expect_err("the pull is refused");
        let gone = "fatal: not a git repository (or any of the parent directories): .git";
        // The rebase's stop as it was: the hook's words, then Git's own for
        // the refusal, which Git words differently across releases ("error:
        // The pre-rebase hook …" in 2.55) and an older Git follows with more
        // once the `.git` is gone, then ours.
        let stopped = rebase.root_cause().to_string();
        let (hook, rest) = stopped
            .strip_prefix("Interactive rebase stopped: ")
            .and_then(|rest| rest.split_once('\n'))
            .expect("the rebase's stop");
        assert_eq!(hook, "password=[redacted]", "{stopped:?}");
        let (named, rest) = rest.split_once('\n').expect("Git's words");
        assert_eq!(named, "b/.git/worktrees/x", "{stopped:?}");
        assert!(
            rest.ends_with("\nReview Working Changes and the operation state before choosing Continue or Abort. No retry was attempted."),
            "{stopped:?}"
        );
        // The linked worktree's administration directory went with the
        // `.git`, so even its configuration reads find no repository, and
        // status does at its first, the configured filters.
        assert!(!root.join(".git/worktrees/linked").exists());
        let status = linked.status().expect_err("no repository there");
        let lfs_config = effective_lfs_config(linked.target()).expect_err("no repository there");
        for (error, shown) in [
            (rebase, stopped.clone()),
            (
                restore,
                format!("Unable to inspect the reviewed commit's restore paths: {gone}"),
            ),
            (resolve, format!("Unable to resolve HEAD: {gone}\n")),
            (
                index,
                format!("Unable to inspect the selected index paths: {gone}"),
            ),
            (tracked, format!("Unable to inspect tracked paths: {gone}\n")),
            (
                ignored,
                format!("Unable to inspect ignored worktree content: {gone}\n"),
            ),
            (
                pull,
                "Branches have diverged; choose Merge or Rebase to continue.\n\nGit stderr:\npassword=[redacted]\nfatal: Not possible to fast-forward, aborting.\nb/.git/worktrees/x\n\nGit stdout:\n".into(),
            ),
            (status, "Unable to inspect configured filters".into()),
            (
                lfs_config,
                "Cannot read the effective Git configuration for LFS.".into(),
            ),
        ] {
            assert_eq!(error.root_cause().to_string(), shown);
            assert!(
                error.chain().any(|cause| cause.is::<NotARepository>()),
                "{error:#}"
            );
            assert!(finds_no_repository(&error), "{error:#}");
            assert!(!format!("{error:#}").contains("SECRET1"), "{error:#}");
        }
        // Git words the missing administration directory differently across
        // releases ("not a git repository: (null)" in 2.55, "gitfile does not
        // point to a valid repository: <path>" in 2.56), so only ours and
        // Git's "fatal:" are asserted; the type says it was recognised.
        for (error, ours) in [
            (
                normal_config_at(linked.target(), "user.name").map(drop),
                "Unable to read Git configuration",
            ),
            (
                branch_upstream_configured(linked.target(), "topic").map(drop),
                "Unable to read the branch's upstream configuration",
            ),
            (
                linked.profile_config().map(drop),
                "Unable to read Git identity configuration",
            ),
            (
                linked.tag_signing(true).map(drop),
                "Unable to read tag signing configuration",
            ),
            (
                linked
                    .management_config_values("remote.origin.url", true)
                    .map(drop),
                "Unable to read Git configuration",
            ),
            (
                linked
                    .management_config_entries_in_scope("^remote\\.", false)
                    .map(drop),
                "Unable to inspect Git configuration",
            ),
            (
                GitRepository::lfs_storage_from(
                    PathBuf::from("unused"),
                    &lfs_storage_config(linked.target()).expect("Git ran"),
                )
                .map(drop),
                "Unable to read local LFS storage configuration",
            ),
        ] {
            let error = error.expect_err("no repository there");
            assert!(
                error
                    .root_cause()
                    .to_string()
                    .starts_with(&format!("{ours}: fatal: ")),
                "{error:#}"
            );
            assert!(
                error.chain().any(|cause| cause.is::<NotARepository>()),
                "{error:#}"
            );
        }
    }
}
