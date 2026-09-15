//! Explicit working-copy operations, kept separate from passive object inspection.
use super::*;
use std::path::Component;

mod integration;
pub use integration::*;
mod branches;
pub use branches::*;
mod authentication;
mod diagnostics;
pub use authentication::{
    AuthenticationPrompt, OperationControl, redact_diagnostic, run_askpass_if_requested,
    run_controlled,
};
mod tags;
pub use tags::*;
mod ignore;
pub use ignore::*;
mod recovery;
pub use recovery::*;
mod worktrees;
pub use worktrees::*;
mod reflog;
pub use reflog::*;
mod lfs_download;
pub use lfs_download::*;
mod interactive_rebase;
pub use interactive_rebase::*;
mod rewrite_review;
pub use rewrite_review::*;
mod profiles;
pub use profiles::*;

const WRITE_TIMEOUT: Duration = Duration::from_secs(90);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(180);
const WRITE_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const MAX_WRITE_INPUT: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeArea {
    Staged,
    Unstaged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: PathBuf,
    pub original_path: Option<PathBuf>,
    pub staged: Option<ChangeStatus>,
    pub unstaged: Option<ChangeStatus>,
    pub untracked: bool,
    pub conflicted: bool,
    head_oid: Option<String>,
    index_oid: Option<String>,
    head_mode: String,
    index_mode: String,
    worktree_mode: String,
}
impl StatusEntry {
    /// Both paths belong to a rename. Always pass both when staging/unstaging it.
    pub fn paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![self.path.clone()];
        if let Some(old) = &self.original_path {
            paths.push(old.clone());
        }
        paths
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepositoryStatus {
    pub entries: Vec<StatusEntry>,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub operation: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitProfile {
    pub name: String,
    pub email: String,
    pub signing: bool,
    pub tag_signing: bool,
    pub annotated_tag_signing: bool,
    pub signing_key: Option<String>,
    pub signing_format: Option<String>,
    pub private_worktree: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
    pub push_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreePreview {
    pub file: FileChange,
    /// Bounded raw bytes (up to MAX_BLOB_BYTES per side), also suitable for images.
    /// Absence is represented by the corresponding file path being None.
    pub old: Vec<u8>,
    pub new: Vec<u8>,
    pub preview: TextPreview,
    /// A bounded, owned selection model. Working snapshots must not be cached.
    pub partial: Option<PartialDiff>,
    pub partial_unavailable: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartialLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartialLine {
    pub kind: PartialLineKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    /// Exact source text, including the terminating newline when present.
    pub text: String,
    /// Stable within this snapshot. Context lines cannot be selected.
    pub change_id: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartialHunk {
    pub header: String,
    pub lines: Vec<PartialLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartialSelection {
    /// Zero-based hunk positions from this snapshot.
    Hunks(Vec<usize>),
    /// Changed-line IDs, including both removed and added lines for a replacement.
    Lines(Vec<usize>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartialDiff {
    pub path: PathBuf,
    pub area: ChangeArea,
    pub hunks: Vec<PartialHunk>,
    snapshot: PartialSnapshot,
    edits: Vec<PartialEdit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PartialSnapshot {
    root: PathBuf,
    file: FileChange,
    old: Vec<u8>,
    new: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PartialEdit {
    kind: PartialLineKind,
    old_index: Option<usize>,
    new_index: Option<usize>,
    change_id: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteCommand {
    DownloadLfs(Arc<LfsDownloadPlan>),
    Worktree(Arc<WorktreeCommand>),
    RecoverReflog(Arc<ReflogRecoveryPlan>),
    InteractiveRebase(Arc<InteractiveRebaseCommand>),
    PublishRewrite(Arc<LeasedPublishPlan>),
    Tag(Arc<TagCommand>),
    Ignore(Arc<IgnorePlan>),
    Recovery(Arc<RecoveryCommand>),
    Branch(Arc<BranchCommand>),
    Integration(IntegrationCommand),
    ApplyPartial {
        diff: Arc<PartialDiff>,
        selection: PartialSelection,
    },
    Stage {
        paths: Vec<PathBuf>,
    },
    StageAll,
    Unstage {
        paths: Vec<PathBuf>,
    },
    UnstageAll,
    Commit {
        message: String,
    },
    Checkout {
        branch: String,
    },
    CreateBranch {
        name: String,
        start_point: Option<String>,
    },
    Fetch {
        remote: String,
    },
    Pull {
        remote: String,
        branch: String,
    },
    Push {
        remote: String,
        local_branch: String,
        remote_branch: String,
    },
    SetIdentity {
        name: String,
        email: String,
    },
    ApplyProfile(Arc<ProfilePlan>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteOutcome {
    pub message: String,
    pub commit_oid: Option<String>,
}

impl GitRepository {
    /// No index refresh, filesystem-monitor invocation, or automatic network access.
    pub fn status(&self) -> Result<RepositoryStatus> {
        ensure!(!self.bare, "A bare repository has no working copy");
        let mut command = passive_status_command(&self.path)?;
        command.args([
            "-c",
            "diff.renameLimit=1000",
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=all",
            "--renames",
            "--ignore-submodules=dirty",
            "-z",
        ]);
        let output = bounded_output(command, GIT_TIMEOUT)?;
        ensure!(
            output.status.success(),
            "Unable to read working-copy status: {}",
            text(&output.stderr).trim()
        );
        let bytes = output.stdout;
        let mut status = parse_status(&bytes)?;
        let git_dir = path_from_bytes(trim_line(&run_git(
            &self.path,
            &["rev-parse", "--absolute-git-dir"],
        )?));
        for (file, name) in [
            ("rebase-merge", "Rebase"),
            ("rebase-apply", "Rebase"),
            ("MERGE_HEAD", "Merge"),
            ("CHERRY_PICK_HEAD", "Cherry-pick"),
            ("REVERT_HEAD", "Revert"),
        ] {
            if std::fs::symlink_metadata(git_dir.join(file)).is_ok() {
                status.operation = Some(name.into());
                break;
            }
        }
        Ok(status)
    }

    /// Effective identity, including normal Git global/include/worktree configuration.
    /// Unlike object inspection, this command intentionally reads normal Git config.
    pub fn profile(&self) -> Result<GitProfile> {
        let mut profile = self.profile_config()?;
        let mut command = normal_command(&self.path);
        command.args(["var", "GIT_AUTHOR_IDENT"]);
        let output = bounded_write_output(command, None, GIT_TIMEOUT)?;
        if output.status.success() {
            let identity = text(trim_line(&output.stdout));
            if let Some((identity, _)) = identity.rsplit_once("> ")
                && let Some((name, email)) = identity.rsplit_once(" <")
            {
                profile.name = name.into();
                profile.email = email.into();
            }
        }
        Ok(profile)
    }

    pub fn remotes(&self) -> Result<Vec<Remote>> {
        let mut command = normal_command(&self.path);
        command.args(["remote"]);
        let bytes = checked_write_output(command, None, GIT_TIMEOUT)?.stdout;
        text(&bytes)
            .lines()
            .map(|name| {
                validate_remote_name(name)?;
                let read = |push: bool| -> Result<String> {
                    let mut command = normal_command(&self.path);
                    command.args(["remote", "get-url"]);
                    if push {
                        command.arg("--push");
                    }
                    command.args(["--", name]);
                    Ok(text(trim_line(
                        &checked_write_output(command, None, GIT_TIMEOUT)?.stdout,
                    )))
                };
                Ok(Remote {
                    name: name.into(),
                    url: read(false)?,
                    push_url: read(true)?,
                })
            })
            .collect()
    }

    fn normal_config(&self, key: &str) -> Result<Option<String>> {
        normal_config_at(&self.path, key)
    }

    /// Worktree reads never run clean/smudge filters and never follow stored symlinks.
    /// Index/HEAD sides use immutable object IDs from the status snapshot. A changed
    /// working file is reread on activation; callers should refresh after writes.
    pub fn worktree_preview(
        &self,
        entry: &StatusEntry,
        area: ChangeArea,
    ) -> Result<WorktreePreview> {
        ensure!(
            !entry.conflicted,
            "This path has unresolved conflicts. Resolve it in your editor, then stage the resolved file."
        );
        validate_path(&entry.path)?;
        let (old_oid, new_oid, old_mode, new_mode, status, old_path, new_path) = match area {
            ChangeArea::Staged => {
                let status = entry.staged.context("This file has no staged change")?;
                (
                    entry.head_oid.clone(),
                    entry.index_oid.clone(),
                    entry.head_mode.clone(),
                    entry.index_mode.clone(),
                    status,
                    (status != ChangeStatus::Added).then(|| {
                        entry
                            .original_path
                            .clone()
                            .unwrap_or_else(|| entry.path.clone())
                    }),
                    (status != ChangeStatus::Deleted).then(|| entry.path.clone()),
                )
            }
            ChangeArea::Unstaged => {
                let status = entry.unstaged.context("This file has no unstaged change")?;
                (
                    entry.index_oid.clone(),
                    None,
                    entry.index_mode.clone(),
                    entry.worktree_mode.clone(),
                    status,
                    (status != ChangeStatus::Added).then(|| entry.path.clone()),
                    (status != ChangeStatus::Deleted).then(|| entry.path.clone()),
                )
            }
        };
        let mut file = FileChange {
            old_oid,
            new_oid,
            old_mode,
            new_mode,
            status,
            old_path,
            new_path,
        };
        if file.old_path.is_none() {
            file.old_mode = "000000".into();
        }
        if file.new_path.is_none() {
            file.new_mode = "000000".into();
        }
        let old = file
            .old_oid
            .as_deref()
            .map(|oid| self.preview_side_bytes(oid, &file.old_mode))
            .transpose()?
            .unwrap_or_default();
        let new = match area {
            ChangeArea::Staged => file.new_oid.as_deref().map(|oid| self.preview_side_bytes(oid, &file.new_mode)).transpose()?.unwrap_or_default(),
            ChangeArea::Unstaged if file.new_path.is_none() => Vec::new(),
            ChangeArea::Unstaged if file.new_mode == "160000" => {
                b"Submodule working copy changed. Open the submodule to inspect its working files.\n".to_vec()
            },
            ChangeArea::Unstaged => {
                let (mode, bytes) = read_worktree_file(&self.path, &entry.path, &file.new_mode)?;
                file.new_mode = mode;
                bytes
            },
        };
        let preview = if area == ChangeArea::Unstaged && file.new_mode == "160000" {
            TextPreview::Patch(format!(
                "Submodule working copy changed.\nIndex commit: {}\nOpen the submodule repository to inspect its checked-out commit and working files.\n",
                file.old_oid.as_deref().unwrap_or("(not present in index)")
            ))
        } else {
            preview_bytes(&file, &old, &new)
        };
        let mut result = WorktreePreview {
            file,
            old,
            new,
            preview,
            partial: None,
            partial_unavailable: None,
        };
        match self.partial_diff(entry, area, &result) {
            Ok(diff) => result.partial = Some(diff),
            Err(error) => result.partial_unavailable = Some(error.to_string()),
        }
        Ok(result)
    }

    fn partial_diff(
        &self,
        entry: &StatusEntry,
        area: ChangeArea,
        preview: &WorktreePreview,
    ) -> Result<PartialDiff> {
        let file = &preview.file;
        ensure!(
            entry.original_path.is_none() && file.status != ChangeStatus::Renamed,
            "Stage or unstage this rename as a whole file."
        );
        ensure!(
            [&file.old_mode, &file.new_mode]
                .iter()
                .all(|mode| matches!(mode.as_str(), "100644" | "100755" | "000000")),
            "Use the whole-file action for symbolic links, submodules, or type changes."
        );
        ensure!(
            file.old_mode == file.new_mode || file.old_path.is_none() || file.new_path.is_none(),
            "Use the whole-file action to preserve this file's permission change."
        );
        ensure!(
            matches!(&preview.preview, TextPreview::Patch(_)),
            "Partial staging is available for text within the diff size limits."
        );
        // check-attr is a passive read of normal attributes; it never invokes a
        // filter or a diff driver. Keep display bytes raw, and disclose cases in
        // which combining those bytes with index contents would be misleading.
        let mut command = normal_command(&self.path);
        command.env("GIT_OPTIONAL_LOCKS", "0").args([
            "check-attr",
            "-z",
            "--stdin",
            "filter",
            "working-tree-encoding",
            "ident",
            "text",
            "eol",
            "diff",
        ]);
        let attributes = checked_write_output(
            command,
            Some(path_input(std::slice::from_ref(&entry.path))?),
            GIT_TIMEOUT,
        )?
        .stdout;
        let fields: Vec<_> = attributes.split(|byte| *byte == 0).collect();
        let attribute = |name: &[u8]| -> &[u8] {
            fields
                .as_chunks::<3>()
                .0
                .iter()
                .find_map(|item| (item[1] == name).then_some(item[2]))
                .unwrap_or(b"unspecified")
        };
        let enabled = |value: &[u8]| !matches!(value, b"unset" | b"unspecified");
        ensure!(
            attribute(b"diff") != b"unset",
            "Git marks this file as binary; use the whole-file action."
        );
        if area == ChangeArea::Unstaged {
            ensure!(
                ![b"filter".as_slice(), b"working-tree-encoding", b"ident"]
                    .iter()
                    .any(|name| enabled(attribute(name))),
                "This file uses Git content conversion; stage it as a whole file to respect that configuration."
            );
            if preview.new.windows(2).any(|pair| pair == b"\r\n") && attribute(b"text") != b"unset"
            {
                let autocrlf = self.normal_config("core.autocrlf")?.unwrap_or_default();
                ensure!(
                    !enabled(attribute(b"text"))
                        && !enabled(attribute(b"eol"))
                        && !matches!(autocrlf.to_ascii_lowercase().as_str(), "true" | "input"),
                    "Git normalizes this file's line endings; stage it as a whole file."
                );
            }
        }
        PartialDiff::new(self.path.clone(), entry.path.clone(), area, preview)
    }

    fn apply_partial(
        &self,
        diff: &PartialDiff,
        selection: &PartialSelection,
    ) -> Result<WriteOutcome> {
        ensure!(
            diff.snapshot.root == self.path,
            "This selection belongs to another working copy; refresh first."
        );
        validate_path(&diff.path)?;
        let (bytes, remove) = diff.selected_contents(selection)?;
        // Holding the real index.lock prevents other cooperating Git writers
        // from changing the target between validation and atomic publication.
        let index = path_from_bytes(trim_line(&run_git(
            &self.path,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        )?));
        let transaction = PartialIndex::acquire(index)?;
        let current = self.status()?;
        let entry = current
            .entries
            .iter()
            .find(|entry| entry.path == diff.path)
            .context("This change is no longer present; refresh before selecting changes again.")?;
        ensure!(
            !entry.conflicted,
            "This file has become conflicted; refresh and resolve it first."
        );
        let fresh = self.worktree_preview(entry, diff.area)?;
        ensure!(
            fresh
                .partial
                .as_ref()
                .is_some_and(|fresh| fresh.snapshot == diff.snapshot),
            "This file, its staged contents, or its Git attributes changed; refresh and select changes again."
        );
        let mut input = if remove {
            // The object-ID width is repository-dependent (SHA-1 or SHA-256).
            let oid = diff
                .snapshot
                .file
                .old_oid
                .as_ref()
                .or(diff.snapshot.file.new_oid.as_ref())
                .context("Missing object identity for file removal")?;
            format!("0 {}\t", "0".repeat(oid.len())).into_bytes()
        } else {
            let mut hash = normal_command(&self.path);
            hash.args(["hash-object", "-w", "--stdin"]);
            if diff.area == ChangeArea::Unstaged {
                hash.arg("--path").arg(&diff.path);
            } else {
                hash.arg("--no-filters");
            }
            let oid = text(trim_line(
                &checked_write_output(hash, Some(bytes.clone()), WRITE_TIMEOUT)?.stdout,
            ));
            validate_oid(&oid)?;
            if diff.area == ChangeArea::Unstaged {
                let mut raw_hash = normal_command(&self.path);
                raw_hash.args(["hash-object", "--stdin", "--no-filters"]);
                let raw_oid = checked_write_output(raw_hash, Some(bytes), WRITE_TIMEOUT)?.stdout;
                ensure!(
                    trim_line(&raw_oid) == oid.as_bytes(),
                    "Git content conversion changed this selection; use whole-file staging to preserve Git's configured behavior."
                );
            }
            let mode = if diff.snapshot.file.old_mode != "000000" {
                &diff.snapshot.file.old_mode
            } else {
                &diff.snapshot.file.new_mode
            };
            format!("{mode} {oid}\t").into_bytes()
        };
        input.extend_from_slice(diff.path.as_os_str().as_encoded_bytes());
        input.push(0);
        transaction.prepare(self)?;
        let mut update = normal_command(&self.path);
        update.env("GIT_INDEX_FILE", &transaction.lock).args([
            "update-index",
            "-z",
            "--index-info",
        ]);
        checked_write_output(update, Some(input), WRITE_TIMEOUT)?;
        transaction.publish()?;
        Ok(WriteOutcome {
            message: if diff.area == ChangeArea::Unstaged {
                "Selected changes staged"
            } else {
                "Selected changes unstaged"
            }
            .into(),
            commit_oid: None,
        })
    }

    /// Must be called from the application's serialized explicit-operation worker.
    /// Never place this in a replaceable preview queue or automatically retry it.
    pub fn execute(&self, operation: &WriteCommand) -> Result<WriteOutcome> {
        let mut outcome = self.execute_inner(operation)?;
        outcome.message = authentication::redact_current(&outcome.message);
        Ok(outcome)
    }

    fn execute_inner(&self, operation: &WriteCommand) -> Result<WriteOutcome> {
        ensure!(!self.bare, "Open a working copy to perform Git operations");
        let mut command = normal_command(&self.path);
        let mut input = None;
        let mut timeout = WRITE_TIMEOUT;
        match operation {
            WriteCommand::ApplyProfile(plan) => return self.execute_profile(plan),
            WriteCommand::DownloadLfs(plan) => return self.execute_lfs_download(plan),
            WriteCommand::Worktree(command) => return self.execute_worktree(command),
            WriteCommand::RecoverReflog(plan) => return self.execute_reflog_recovery(plan),
            WriteCommand::InteractiveRebase(command) => {
                return self.execute_interactive_rebase(command);
            }
            WriteCommand::PublishRewrite(plan) => return self.execute_leased_publish(plan),
            WriteCommand::Tag(command) => return self.execute_tag(command),
            WriteCommand::Ignore(plan) => return self.execute_ignore(plan),
            WriteCommand::Recovery(command) => return self.execute_recovery(command),
            WriteCommand::Branch(command) => return self.execute_branch(command),
            WriteCommand::Integration(command) => return self.execute_integration(command),
            WriteCommand::ApplyPartial { diff, selection } => {
                return self.apply_partial(diff, selection);
            }
            WriteCommand::Stage { paths } => {
                input = Some(path_input(paths)?);
                command.args([
                    "--literal-pathspecs",
                    "add",
                    "--all",
                    "--pathspec-from-file=-",
                    "--pathspec-file-nul",
                ]);
            }
            WriteCommand::StageAll => {
                command.args(["--literal-pathspecs", "add", "--all", "--", "."]);
            }
            WriteCommand::Unstage { paths } => {
                input = Some(path_input(paths)?);
                if self.has_head()? {
                    command.args([
                        "--literal-pathspecs",
                        "restore",
                        "--staged",
                        "--source=HEAD",
                        "--pathspec-from-file=-",
                        "--pathspec-file-nul",
                    ]);
                } else {
                    command.args([
                        "--literal-pathspecs",
                        "rm",
                        "--cached",
                        "-r",
                        "--force",
                        "--ignore-unmatch",
                        "--pathspec-from-file=-",
                        "--pathspec-file-nul",
                    ]);
                }
            }
            WriteCommand::UnstageAll => {
                if self.has_head()? {
                    command.args([
                        "--literal-pathspecs",
                        "restore",
                        "--staged",
                        "--source=HEAD",
                        "--",
                        ".",
                    ]);
                } else {
                    command.args([
                        "--literal-pathspecs",
                        "rm",
                        "--cached",
                        "-r",
                        "--force",
                        "--ignore-unmatch",
                        "--",
                        ".",
                    ]);
                }
            }
            WriteCommand::Commit { message } => {
                ensure!(!message.trim().is_empty(), "Enter a commit message");
                ensure!(
                    message.len() <= 64 * 1024 && !message.contains('\0'),
                    "Commit message must be at most 64 KiB and contain no NUL bytes"
                );
                command.args(["commit", "--cleanup=verbatim", "--file=-"]);
                input = Some(message.as_bytes().to_vec());
            }
            WriteCommand::Checkout { branch } => {
                self.validate_branch(branch)?;
                run_git(
                    &self.path,
                    &["show-ref", "--verify", &format!("refs/heads/{branch}")],
                )?;
                command.args(["switch", "--no-guess", "--", branch]);
            }
            WriteCommand::CreateBranch { name, start_point } => {
                self.validate_branch(name)?;
                command.args(["switch", "--no-guess", "--create", name]);
                if let Some(start) = start_point {
                    let oid = self.resolve_start(start)?;
                    command.arg(oid);
                }
            }
            WriteCommand::Fetch { remote } => {
                self.validate_remote(remote)?;
                configure_network(&mut command, self)?;
                command.args([
                    "fetch",
                    "--progress",
                    "--no-recurse-submodules",
                    "--no-prune",
                    "--",
                    remote,
                ]);
                timeout = NETWORK_TIMEOUT;
            }
            WriteCommand::Pull { remote, branch } => {
                self.validate_remote(remote)?;
                self.validate_branch(branch)?;
                let status = self.status()?;
                ensure!(status.branch.is_some(), "Check out a branch before pulling");
                ensure!(
                    status.operation.is_none() && !status.entries.iter().any(|e| e.conflicted),
                    "Finish the current Git operation and resolve conflicts before pulling"
                );
                configure_network(&mut command, self)?;
                command.args([
                    "pull",
                    "--progress",
                    "--ff-only",
                    "--no-rebase",
                    "--no-autostash",
                    "--no-recurse-submodules",
                    "--",
                    remote,
                    &format!("refs/heads/{branch}"),
                ]);
                timeout = NETWORK_TIMEOUT;
            }
            WriteCommand::Push {
                remote,
                local_branch,
                remote_branch,
            } => {
                self.validate_remote(remote)?;
                self.validate_branch(local_branch)?;
                self.validate_branch(remote_branch)?;
                let mut urls = normal_command(&self.path);
                urls.args(["remote", "get-url", "--push", "--all", "--", remote]);
                ensure!(
                    text(&checked_write_output(urls, None, GIT_TIMEOUT)?.stdout)
                        .lines()
                        .count()
                        == 1,
                    "This remote has multiple push destinations. Configure a remote with one push URL so the visible destination is unambiguous."
                );
                run_git(
                    &self.path,
                    &[
                        "show-ref",
                        "--verify",
                        &format!("refs/heads/{local_branch}"),
                    ],
                )?;
                configure_network(&mut command, self)?;
                // Explicit refspec and disabled mirror/follow-tags prevent normal
                // push configuration from expanding the visible target.
                command.args([
                    "-c",
                    &format!("remote.{remote}.mirror=false"),
                    "-c",
                    "push.followTags=false",
                    "push",
                    "--progress",
                    "--porcelain",
                    "--no-force",
                    "--no-force-with-lease",
                    "--no-follow-tags",
                    "--recurse-submodules=no",
                    "--set-upstream",
                    "--",
                    remote,
                    &format!("refs/heads/{local_branch}:refs/heads/{remote_branch}"),
                ]);
                timeout = NETWORK_TIMEOUT;
            }
            WriteCommand::SetIdentity { name, email } => {
                let plan = self.profile_plan(ProfileIdentity {
                    name: name.clone(),
                    email: email.clone(),
                    signing: None,
                })?;
                return self.execute_profile(&plan);
            }
        }
        let output = checked_write_output(command, input, timeout)?;
        let message = format!("{}{}", text(&output.stdout), text(&output.stderr))
            .trim()
            .to_owned();
        let commit_oid = if matches!(operation, WriteCommand::Commit { .. }) {
            Some(text(trim_line(&run_git(
                &self.path,
                &["rev-parse", "--verify", "HEAD"],
            )?)))
        } else {
            None
        };
        Ok(WriteOutcome {
            message: if message.is_empty() {
                "Git operation completed".into()
            } else {
                message
            },
            commit_oid,
        })
    }

    pub fn init(destination: impl AsRef<Path>, initial_branch: &str) -> Result<Self> {
        let destination = fresh_destination(destination.as_ref())?;
        let parent = destination
            .parent()
            .context("Choose a destination inside an existing folder")?;
        validate_branch_at(parent, initial_branch)?;
        let mut command = normal_command(parent);
        command
            .args(["init", "--initial-branch", initial_branch, "--"])
            .arg(&destination);
        checked_write_output(command, None, WRITE_TIMEOUT)?;
        Self::open(destination)
    }

    pub fn clone_repository(source: &str, destination: impl AsRef<Path>) -> Result<Self> {
        ensure!(
            !source.trim().is_empty()
                && !source.starts_with('-')
                && !source.contains(['\0', '\n', '\r']),
            "Enter a Git URL or local repository path"
        );
        ensure!(source.len() <= 16 * 1024, "Repository address is too long");
        authentication::validate_clone_address(source)?;
        // ext transport runs arbitrary commands. Native Git's usual SSH/HTTPS,
        // file, and git transports remain available only on this explicit action.
        ensure!(
            !source.starts_with("ext::"),
            "The executable ext transport is not supported"
        );
        let destination = fresh_destination(destination.as_ref())?;
        let parent = destination
            .parent()
            .context("Choose a destination inside an existing folder")?;
        let mut command = normal_command(parent);
        authentication::configure_askpass(
            &mut command,
            normal_config_at(parent, "core.askPass")?.is_some(),
        )?;
        if normal_config_at(parent, "core.sshCommand")?.is_none() {
            configure_default_network(&mut command);
        }
        command
            .args([
                "clone",
                "--progress",
                "--no-recurse-submodules",
                "--",
                source,
            ])
            .arg(&destination);
        checked_write_output(command, None, NETWORK_TIMEOUT)?;
        Self::open(destination)
    }

    fn has_head(&self) -> Result<bool> {
        let result = run_git_output(&self.path, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
        ensure!(
            result.status.success() || result.status.code() == Some(1),
            "Unable to resolve HEAD: {}",
            text(&result.stderr)
        );
        Ok(result.status.success())
    }
    fn validate_branch(&self, name: &str) -> Result<()> {
        validate_branch_at(&self.path, name)
    }
    fn validate_remote(&self, name: &str) -> Result<()> {
        validate_remote_name(name)?;
        ensure!(
            self.remotes()?.iter().any(|r| r.name == name),
            "Remote '{name}' no longer exists; refresh and choose a remote"
        );
        Ok(())
    }
    fn resolve_start(&self, name: &str) -> Result<String> {
        if validate_oid(name).is_err() {
            ensure!(
                name.starts_with("refs/heads/") || name.starts_with("refs/remotes/"),
                "Choose an existing branch or commit as the starting point"
            );
            run_git(&self.path, &["check-ref-format", name])?;
        }
        Ok(text(trim_line(&run_git(
            &self.path,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{name}^{{commit}}"),
            ],
        )?)))
    }
}

impl PartialDiff {
    fn new(
        root: PathBuf,
        path: PathBuf,
        area: ChangeArea,
        preview: &WorktreePreview,
    ) -> Result<Self> {
        let old = std::str::from_utf8(&preview.old)?;
        let new = std::str::from_utf8(&preview.new)?;
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Patience)
            .timeout(Duration::from_millis(250))
            .diff_lines(old, new);
        let mut next_id = 0;
        let edits: Vec<_> = diff
            .iter_all_changes()
            .map(|change| {
                let kind = match change.tag() {
                    similar::ChangeTag::Equal => PartialLineKind::Context,
                    similar::ChangeTag::Insert => PartialLineKind::Addition,
                    similar::ChangeTag::Delete => PartialLineKind::Deletion,
                };
                let change_id = (kind != PartialLineKind::Context).then(|| {
                    let id = next_id;
                    next_id += 1;
                    id
                });
                PartialEdit {
                    kind,
                    old_index: change.old_index(),
                    new_index: change.new_index(),
                    change_id,
                }
            })
            .collect();
        ensure!(next_id > 0, "There are no text changes to select.");
        // Merge changed ranges whenever their three lines of context overlap.
        let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
        for (index, _) in edits
            .iter()
            .enumerate()
            .filter(|(_, edit)| edit.change_id.is_some())
        {
            let range = index.saturating_sub(3)..(index + 4).min(edits.len());
            if let Some(previous) = ranges
                .last_mut()
                .filter(|previous| previous.end >= range.start)
            {
                previous.end = range.end;
            } else {
                ranges.push(range);
            }
        }
        let old_lines: Vec<_> = old.split_inclusive('\n').collect();
        let new_lines: Vec<_> = new.split_inclusive('\n').collect();
        let mut old_before = 0;
        let mut new_before = 0;
        let mut cursor = 0;
        let mut hunks = Vec::with_capacity(ranges.len());
        for range in ranges {
            for edit in &edits[cursor..range.start] {
                old_before += usize::from(edit.old_index.is_some());
                new_before += usize::from(edit.new_index.is_some());
            }
            let old_count = edits[range.clone()]
                .iter()
                .filter(|edit| edit.old_index.is_some())
                .count();
            let new_count = edits[range.clone()]
                .iter()
                .filter(|edit| edit.new_index.is_some())
                .count();
            let header = format!(
                "@@ -{},{} +{},{} @@",
                old_before + usize::from(old_count > 0),
                old_count,
                new_before + usize::from(new_count > 0),
                new_count
            );
            let lines = edits[range.clone()]
                .iter()
                .map(|edit| PartialLine {
                    kind: edit.kind,
                    old_line: edit.old_index.map(|index| index + 1),
                    new_line: edit.new_index.map(|index| index + 1),
                    text: edit
                        .old_index
                        .map(|index| old_lines[index])
                        .unwrap_or_else(|| new_lines[edit.new_index.expect("added line index")])
                        .into(),
                    change_id: edit.change_id,
                })
                .collect();
            hunks.push(PartialHunk { header, lines });
            old_before += old_count;
            new_before += new_count;
            cursor = range.end;
        }
        Ok(Self {
            path,
            area,
            hunks,
            snapshot: PartialSnapshot {
                root,
                file: preview.file.clone(),
                old: preview.old.clone(),
                new: preview.new.clone(),
            },
            edits,
        })
    }

    /// Retained allocation estimate for worker/UI memory accounting.
    pub fn bytes(&self) -> usize {
        self.path.capacity()
            + self.snapshot.root.capacity()
            + self
                .snapshot
                .file
                .old_path
                .as_ref()
                .map_or(0, PathBuf::capacity)
            + self
                .snapshot
                .file
                .new_path
                .as_ref()
                .map_or(0, PathBuf::capacity)
            + self
                .snapshot
                .file
                .old_oid
                .as_ref()
                .map_or(0, String::capacity)
            + self
                .snapshot
                .file
                .new_oid
                .as_ref()
                .map_or(0, String::capacity)
            + self.snapshot.file.old_mode.capacity()
            + self.snapshot.file.new_mode.capacity()
            + self.snapshot.old.capacity()
            + self.snapshot.new.capacity()
            + self.edits.capacity() * std::mem::size_of::<PartialEdit>()
            + self.hunks.capacity() * std::mem::size_of::<PartialHunk>()
            + self
                .hunks
                .iter()
                .map(|hunk| {
                    hunk.header.capacity()
                        + hunk.lines.capacity() * std::mem::size_of::<PartialLine>()
                        + hunk
                            .lines
                            .iter()
                            .map(|line| line.text.capacity())
                            .sum::<usize>()
                })
                .sum::<usize>()
    }

    fn selected_contents(&self, selection: &PartialSelection) -> Result<(Vec<u8>, bool)> {
        let count = self
            .edits
            .iter()
            .filter(|edit| edit.change_id.is_some())
            .count();
        let mut chosen = vec![false; count];
        let mut select = |id: usize| -> Result<()> {
            *chosen
                .get_mut(id)
                .context("This selection is invalid; refresh and select changes again.")? = true;
            Ok(())
        };
        match selection {
            PartialSelection::Hunks(indices) => {
                for index in indices {
                    let hunk = self
                        .hunks
                        .get(*index)
                        .context("This hunk no longer exists; refresh the preview.")?;
                    for id in hunk.lines.iter().filter_map(|line| line.change_id) {
                        select(id)?;
                    }
                }
            }
            PartialSelection::Lines(ids) => {
                for id in ids {
                    select(*id)?;
                }
            }
        }
        ensure!(
            chosen.iter().any(|selected| *selected),
            "Select at least one changed line or hunk."
        );
        let old_lines: Vec<_> = self
            .snapshot
            .old
            .split_inclusive(|byte| *byte == b'\n')
            .collect();
        let new_lines: Vec<_> = self
            .snapshot
            .new
            .split_inclusive(|byte| *byte == b'\n')
            .collect();
        let mut result = Vec::with_capacity(self.snapshot.old.len().max(self.snapshot.new.len()));
        for edit in &self.edits {
            let selected = edit.change_id.is_some_and(|id| chosen[id]);
            let use_new = if self.area == ChangeArea::Unstaged {
                selected
            } else {
                !selected
            };
            let line = match edit.kind {
                PartialLineKind::Context => Some(old_lines[edit.old_index.expect("context index")]),
                PartialLineKind::Deletion if !use_new => {
                    Some(old_lines[edit.old_index.expect("deleted index")])
                }
                PartialLineKind::Addition if use_new => {
                    Some(new_lines[edit.new_index.expect("added index")])
                }
                _ => None,
            };
            if let Some(line) = line {
                // A retained EOF fragment followed by an addition would silently
                // join two lines. Require both sides of that newline replacement.
                ensure!(
                    result.is_empty() || result.ends_with(b"\n"),
                    "This selection changes the final newline; select both replacement lines or the whole hunk."
                );
                result.extend_from_slice(line);
            }
        }
        let all = chosen.iter().all(|selected| *selected);
        let remove = all
            && match self.area {
                ChangeArea::Unstaged => self.snapshot.file.new_path.is_none(),
                ChangeArea::Staged => self.snapshot.file.old_path.is_none(),
            };
        Ok((result, remove))
    }
}

struct PartialIndex {
    index: PathBuf,
    lock: PathBuf,
    nested_lock: PathBuf,
    published: bool,
}

impl PartialIndex {
    fn acquire(index: PathBuf) -> Result<Self> {
        let mut lock = index.as_os_str().to_os_string();
        lock.push(".lock");
        let lock = PathBuf::from(lock);
        let mut nested_lock = lock.as_os_str().to_os_string();
        nested_lock.push(".lock");
        let nested_lock = PathBuf::from(nested_lock);
        match std::fs::symlink_metadata(&nested_lock) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => bail!(
                "A partial-staging lock already exists; inspect the prior operation before retrying."
            ),
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .context("Git's index is locked by another operation; let it finish, then refresh.")?;
        Ok(Self {
            index,
            lock,
            nested_lock,
            published: false,
        })
    }

    fn prepare(&self, repository: &GitRepository) -> Result<()> {
        match std::fs::symlink_metadata(&self.index) {
            Ok(metadata) => {
                ensure!(
                    metadata.file_type().is_file(),
                    "Partial staging requires a regular Git index."
                );
                ensure!(
                    metadata.len() <= MAX_COMMAND_OUTPUT as u64,
                    "This index exceeds the partial-staging size limit; use whole-file staging."
                );
                std::fs::copy(&self.index, &self.lock)
                    .context("Unable to prepare the staged selection")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut command = normal_command(&repository.path);
                command
                    .env("GIT_INDEX_FILE", &self.lock)
                    .args(["read-tree", "--empty"]);
                checked_write_output(command, None, WRITE_TIMEOUT)?;
            }
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    fn publish(mut self) -> Result<()> {
        std::fs::File::open(&self.lock)?.sync_all()?;
        std::fs::rename(&self.lock, &self.index)
            .context("Unable to publish selected changes; the previous index was preserved")?;
        self.published = true;
        Ok(())
    }
}

impl Drop for PartialIndex {
    fn drop(&mut self) {
        if !self.published {
            // Clean our nested lock while the real lock is still held. The Git
            // process has exited or been reaped; unpublished bytes can be dropped.
            let _ = std::fs::remove_file(&self.nested_lock);
            let _ = std::fs::remove_file(&self.lock);
        }
    }
}

fn parse_status(bytes: &[u8]) -> Result<RepositoryStatus> {
    let mut result = RepositoryStatus::default();
    let mut records = bytes.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            if value != b"(initial)" {
                result.head = Some(text(value));
            }
        } else if let Some(value) = record.strip_prefix(b"# branch.head ") {
            if value != b"(detached)" {
                result.branch = Some(text(value));
            }
        } else if let Some(value) = record.strip_prefix(b"# branch.upstream ") {
            result.upstream = Some(text(value));
        } else if let Some(value) = record.strip_prefix(b"# branch.ab ") {
            let value = text(value);
            if let Some((ahead, behind)) = value.split_once(' ') {
                result.ahead = ahead.trim_start_matches('+').parse()?;
                result.behind = behind.trim_start_matches('-').parse()?;
            }
        } else if record[0] == b'1' || record[0] == b'2' {
            let rename = record[0] == b'2';
            let fields: Vec<_> = record
                .splitn(if rename { 10 } else { 9 }, |b| *b == b' ')
                .collect();
            ensure!(
                fields.len() == if rename { 10 } else { 9 },
                "Malformed Git status entry"
            );
            ensure!(fields[1].len() == 2, "Malformed status code");
            let path = path_from_bytes(fields[if rename { 9 } else { 8 }]);
            let original_path = if rename {
                Some(path_from_bytes(
                    records.next().context("Missing rename source")?,
                ))
            } else {
                None
            };
            result.entries.push(StatusEntry {
                path,
                original_path,
                staged: status_code(fields[1][0])?,
                unstaged: status_code(fields[1][1])?,
                untracked: false,
                conflicted: false,
                head_oid: status_oid(fields[6]),
                index_oid: status_oid(fields[7]),
                head_mode: text(fields[3]),
                index_mode: text(fields[4]),
                worktree_mode: text(fields[5]),
            });
        } else if let Some(path) = record.strip_prefix(b"? ") {
            result.entries.push(StatusEntry {
                path: path_from_bytes(path),
                original_path: None,
                staged: None,
                unstaged: Some(ChangeStatus::Added),
                untracked: true,
                conflicted: false,
                head_oid: None,
                index_oid: None,
                head_mode: "000000".into(),
                index_mode: "000000".into(),
                worktree_mode: "100644".into(),
            });
        } else if record[0] == b'u' {
            let fields: Vec<_> = record.splitn(11, |b| *b == b' ').collect();
            ensure!(fields.len() == 11, "Malformed conflicted status entry");
            result.entries.push(StatusEntry {
                path: path_from_bytes(fields[10]),
                original_path: None,
                staged: None,
                unstaged: None,
                untracked: false,
                conflicted: true,
                head_oid: status_oid(fields[7]),
                index_oid: None,
                head_mode: text(fields[3]),
                index_mode: "000000".into(),
                worktree_mode: text(fields[6]),
            });
        } else if !record.starts_with(b"# ") {
            bail!("Unsupported Git status entry");
        }
    }
    Ok(result)
}
fn status_code(value: u8) -> Result<Option<ChangeStatus>> {
    Ok(match value {
        b'.' => None,
        b'A' => Some(ChangeStatus::Added),
        b'M' => Some(ChangeStatus::Modified),
        b'D' => Some(ChangeStatus::Deleted),
        b'R' | b'C' => Some(ChangeStatus::Renamed),
        b'T' => Some(ChangeStatus::TypeChanged),
        _ => bail!("Unsupported Git status code: {}", value as char),
    })
}
fn status_oid(value: &[u8]) -> Option<String> {
    (!value.iter().all(|b| *b == b'0')).then(|| text(value))
}

fn validate_path(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty()
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "Choose a repository-relative file path"
    );
    ensure!(
        !path.as_os_str().as_encoded_bytes().contains(&0),
        "File path contains NUL"
    );
    Ok(())
}
fn path_input(paths: &[PathBuf]) -> Result<Vec<u8>> {
    ensure!(!paths.is_empty(), "Select files first");
    let mut input = Vec::new();
    for path in paths {
        validate_path(path)?;
        ensure!(
            input
                .len()
                .saturating_add(path.as_os_str().len())
                .saturating_add(1)
                <= MAX_WRITE_INPUT,
            "Too many selected paths; select fewer files or use the all-files action"
        );
        input.extend_from_slice(path.as_os_str().as_encoded_bytes());
        input.push(0);
    }
    Ok(input)
}
fn validate_branch_at(path: &Path, name: &str) -> Result<()> {
    const INVALID_NAME: &str = "Invalid branch name. Use a name such as feature/my-change; avoid spaces, a leading '-' and Git-special characters.";
    ensure!(
        !name.is_empty()
            && name.len() <= 1024
            && !name.starts_with('-')
            && name != "HEAD"
            && !name.contains('\0'),
        INVALID_NAME
    );
    let output = run_git_output(path, &["check-ref-format", &format!("refs/heads/{name}")])
        .context("Unable to validate the branch name")?;
    if !output.status.success() {
        let stderr = text(&output.stderr);
        let stdout = text(&output.stdout);
        let diagnostics = [stderr.trim(), stdout.trim()]
            .into_iter()
            .filter(|message| !message.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        ensure!(!diagnostics.is_empty(), INVALID_NAME);
        bail!("{INVALID_NAME}\nGit: {diagnostics}");
    }
    Ok(())
}
fn validate_remote_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 1024
            && !name.starts_with('-')
            && !name.contains(['\0', '\n', '\r', ':'])
            && !name.chars().any(char::is_whitespace),
        "Choose a configured remote name"
    );
    Ok(())
}
fn config_bool(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "true" | "yes" | "on" | "1" | ""
        )
    })
}
fn validate_identity(name: &str, email: &str) -> Result<()> {
    ensure!(
        !name.trim().is_empty()
            && name.len() <= 512
            && !name.contains(['\n', '\r', '\0', '<', '>']),
        "Enter a valid commit author name"
    );
    ensure!(
        !email.trim().is_empty()
            && email.len() <= 512
            && !email.chars().any(char::is_whitespace)
            && !email.contains(['\0', '<', '>']),
        "Enter a valid commit email address"
    );
    Ok(())
}
fn fresh_destination(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "Choose an absolute destination path");
    let name = path.file_name().context("Choose a new repository folder")?;
    let parent = path
        .parent()
        .context("Choose a parent folder")?
        .canonicalize()
        .context("The destination's parent folder must already exist")?;
    let destination = parent.join(name);
    match std::fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Destination must be a new or empty directory"
            );
            // Enumeration only validates occupancy; its result must remain
            // separate from the destination path returned to the caller.
            let mut entries = match std::fs::read_dir(&destination) {
                Ok(entries) => entries,
                Err(error) => return Err(error.into()),
            };
            ensure!(
                entries.next().is_none(),
                "Destination is not empty; choose a new folder"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("Unable to inspect destination"),
    }
    Ok(destination)
}

/// Status may otherwise execute clean/process filters while verifying a dirty
/// stat entry. Disable every configured local filter driver without touching the
/// repository's config. Raw worktree bytes are also what our preview displays.
fn passive_status_command(path: &Path) -> Result<Command> {
    let result = run_git_output(
        path,
        &[
            "config",
            "--includes",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\..*\\.(clean|process|required)$",
        ],
    )?;
    ensure!(
        result.status.success() || result.status.code() == Some(1),
        "Unable to inspect configured filters"
    );
    let mut command = git_command(path);
    for key in result
        .stdout
        .split(|b| *b == 0)
        .filter(|key| !key.is_empty())
    {
        let key = std::str::from_utf8(key).context("Filter configuration key is not UTF-8")?;
        ensure!(
            !key.contains(['\n', '\r', '=']),
            "Unsupported filter configuration key"
        );
        command.arg("-c").arg(format!(
            "{key}={}",
            if key.ends_with(".required") {
                "false"
            } else {
                ""
            }
        ));
    }
    Ok(command)
}
fn normal_config_at(path: &Path, key: &str) -> Result<Option<String>> {
    let mut command = normal_command(path);
    command.args(["config", "--null", "--get", key]);
    let output = bounded_write_output(command, None, GIT_TIMEOUT)?;
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    ensure!(
        output.status.success(),
        "Unable to read Git configuration: {}",
        text(&output.stderr).trim()
    );
    Ok(Some(text(
        output.stdout.strip_suffix(&[0]).unwrap_or(&output.stdout),
    )))
}

pub(super) fn normal_command(path: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .args([
            "--no-pager",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "protocol.ext.allow=never",
        ])
        .arg("-C")
        .arg(path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    authentication::configure_environment(&mut command);
    // Preserve identity, hooks, signing, filters, credential helpers, SSH settings,
    // includes, and normal system/global config, but never an inherited target.
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_NAMESPACE",
        "GIT_SHALLOW_FILE",
        "GIT_REPLACE_REF_BASE",
        "GIT_OPTIONAL_LOCKS",
        "GIT_EXTERNAL_DIFF",
        "GIT_DIFF_OPTS",
    ] {
        command.env_remove(key);
    }
    command
}
fn configure_default_network(command: &mut Command) {
    if std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none() {
        // Configured core.sshCommand is checked by repository-aware caller.
        command.env(
            "GIT_SSH_COMMAND",
            if authentication::is_controlled() {
                "ssh -o ConnectTimeout=15"
            } else {
                "ssh -o BatchMode=yes -o ConnectTimeout=15"
            },
        );
    }
}
fn configure_network(command: &mut Command, repo: &GitRepository) -> Result<()> {
    authentication::configure_askpass(command, repo.normal_config("core.askPass")?.is_some())?;
    if repo.normal_config("core.sshCommand")?.is_none() {
        configure_default_network(command);
    }
    Ok(())
}

fn checked_write_output(
    command: Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Output> {
    let creates_commit = command.get_args().any(|arg| {
        matches!(
            arg.to_str(),
            Some("commit" | "merge" | "rebase" | "cherry-pick" | "revert" | "tag")
        )
    });
    let fast_forward_pull = command.get_args().any(|arg| arg == "pull")
        && command.get_args().any(|arg| arg == "--ff-only");
    let output = bounded_write_output(command, input, timeout)?;
    let stderr = authentication::redact_current(&text(&output.stderr));
    let stdout = authentication::redact_current(&text(&output.stdout));
    if !output.status.success()
        && let Some(headline) =
            diagnostics::pull_refusal_headline(&output.stderr, &output.stdout, fast_forward_pull)
    {
        bail!("{headline}\n\nGit stderr:\n{stderr}\n\nGit stdout:\n{stdout}");
    }
    ensure!(
        output.status.success(),
        "Git operation failed: {}{}{}",
        if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        },
        if stdout.trim().is_empty() || stderr.trim().is_empty() {
            String::new()
        } else {
            format!("\n{}", stdout.trim())
        },
        diagnostics::guidance(&output.stderr, &output.stdout, creates_commit)
            .map_or(String::new(), |help| format!("\n\n{help}"))
    );
    Ok(output)
}

#[cfg(unix)]
fn make_pipe_nonblocking(pipe: &impl std::os::fd::AsFd) -> Result<()> {
    let flags = rustix::fs::fcntl_getfl(pipe)?;
    rustix::fs::fcntl_setfl(pipe, flags | rustix::fs::OFlags::NONBLOCK)?;
    Ok(())
}

/// Drain pipes concurrently, retaining bounded output. Observe pipe completion as
/// well as child exit so hooks/SSH descendants cannot hold the UI operation open.
fn bounded_write_output(
    mut command: Command,
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Output> {
    let control = authentication::current_control();
    if control.as_ref().is_some_and(OperationControl::is_cancelled) {
        bail!("Git operation cancelled before starting. It was not retried.");
    }
    isolate_process_group(&mut command);
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Unable to start Git; install Git and ensure it is on PATH")?;
    let stdout_pipe = child.stdout.take().expect("piped Git stdout");
    let stderr_pipe = child.stderr.take().expect("piped Git stderr");
    let stdin_pipe = child.stdin.take();
    // A helper can create a new session and retain our pipe descriptors. Killing
    // Git's process group then cannot close those descriptors. Nonblocking I/O
    // lets every owned reader/writer observe shutdown and be joined promptly.
    #[cfg(unix)]
    if let Err(error) = (|| -> Result<()> {
        make_pipe_nonblocking(&stdout_pipe)?;
        make_pipe_nonblocking(&stderr_pipe)?;
        if let Some(pipe) = &stdin_pipe {
            make_pipe_nonblocking(pipe)?;
        }
        Ok(())
    })() {
        terminate_process_group(&child);
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.context("Unable to configure bounded Git pipe I/O"));
    }
    let io_stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let read = |mut pipe: Box<dyn Read + Send>, progress: Option<OperationControl>| {
        let stopped = Arc::clone(&io_stopped);
        thread::spawn(move || -> std::io::Result<(Vec<u8>, bool)> {
            let mut bytes = Vec::new();
            let mut overflow = false;
            let mut chunk = [0u8; 8192];
            while !stopped.load(std::sync::atomic::Ordering::Acquire) {
                let count = match pipe.read(&mut chunk) {
                    Ok(count) => count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                };
                if count == 0 {
                    break;
                }
                if let Some(control) = &progress {
                    control.update_progress(&chunk[..count]);
                }
                let keep = count.min(WRITE_OUTPUT_LIMIT.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&chunk[..keep]);
                overflow |= keep < count;
            }
            Ok((bytes, overflow))
        })
    };
    let stdout = read(Box::new(stdout_pipe), None);
    let stderr = read(Box::new(stderr_pipe), control.clone());
    let stdin = input.map(|bytes| {
        let mut pipe = stdin_pipe.expect("piped Git stdin");
        let stopped = Arc::clone(&io_stopped);
        thread::spawn(move || -> std::io::Result<()> {
            let mut written = 0;
            while written < bytes.len() && !stopped.load(std::sync::atomic::Ordering::Acquire) {
                let end = (written + 8192).min(bytes.len());
                match pipe.write(&bytes[written..end]) {
                    Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                    Ok(count) => written += count,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        })
    });
    let start = Instant::now();
    let mut status = None;
    let mut failure = None;
    loop {
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(error) => {
                    failure = Some(anyhow::Error::from(error));
                    break;
                }
            }
        }
        if status.is_some()
            && stdout.is_finished()
            && stderr.is_finished()
            && stdin.as_ref().is_none_or(|t| t.is_finished())
        {
            break;
        }
        if control.as_ref().is_some_and(OperationControl::is_cancelled) {
            failure = Some(anyhow::anyhow!(
                "Git operation cancelled. Its result may have applied partially or remotely; inspect local state and explicitly check the remote before retrying. It was not retried."
            ));
            break;
        }
        if start.elapsed() >= timeout {
            failure = Some(anyhow::anyhow!(
                "Git operation exceeded its {} second time limit. Its result may have applied partially or remotely; refresh and inspect before retrying. It was not retried.",
                timeout.as_secs()
            ));
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    if failure.is_some() {
        io_stopped.store(true, std::sync::atomic::Ordering::Release);
        terminate_process_group(&child);
        let _ = child.kill();
        let _ = child.wait();
    }
    let stdout = stdout
        .join()
        .map_err(|_| anyhow::anyhow!("Git output reader stopped"))??;
    let stderr = stderr
        .join()
        .map_err(|_| anyhow::anyhow!("Git error reader stopped"))??;
    if let Some(stdin) = stdin {
        let _ = stdin.join();
    } // Rejected Git may close stdin early; stderr is authoritative.
    if let Some(error) = failure {
        return Err(error);
    }
    ensure!(
        !stdout.1 && !stderr.1,
        "Git output exceeded its limit. The operation may have completed; refresh and inspect before retrying."
    );
    Ok(Output {
        status: status.context("Missing Git exit status")?,
        stdout: stdout.0,
        stderr: stderr.0,
    })
}

#[cfg(unix)]
pub(super) fn read_worktree_file(
    root: &Path,
    path: &Path,
    expected_mode: &str,
) -> Result<(String, Vec<u8>)> {
    use rustix::fs::{Mode, OFlags, open, openat, readlinkat};
    use std::os::unix::fs::PermissionsExt;
    validate_path(path)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    // GitRepository::open returns the actual absolute worktree root. Traversal
    // beneath it stays descriptor-relative even if another tool renames folders.
    let mut directory =
        open(root, flags, Mode::empty()).context("Unable to safely open working copy")?;
    let components: Vec<_> = path.components().collect();
    for component in &components[..components.len() - 1] {
        directory = openat(&directory, component.as_os_str(), flags, Mode::empty())
            .context("Preview cannot follow a symbolic-link directory")?;
    }
    let name = components.last().context("Missing filename")?.as_os_str();
    // readlinkat reads the stored target, not the destination. It also handles an
    // untracked symlink whose porcelain record does not carry a mode.
    if let Ok(target) = readlinkat(&directory, name, Vec::new()) {
        return Ok(("120000".into(), target.into_bytes()));
    }
    ensure!(
        expected_mode != "120000",
        "Symbolic link changed; refresh the working copy"
    );
    let fd = openat(
        &directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .context("Unable to read working file safely; refresh if it changed")?;
    let mut file = std::fs::File::from(fd);
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "Preview is available only for regular files or stored symlinks"
    );
    ensure!(
        metadata.len() <= MAX_BLOB_BYTES as u64,
        "Working file exceeds the {} MiB preview limit",
        MAX_BLOB_BYTES / 1024 / 1024
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    (&mut file)
        .take(MAX_BLOB_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_BLOB_BYTES,
        "Working file grew beyond the preview limit"
    );
    Ok((
        if metadata.permissions().mode() & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        }
        .into(),
        bytes,
    ))
}
#[cfg(not(unix))]
pub(super) fn read_worktree_file(
    _root: &Path,
    _path: &Path,
    _expected_mode: &str,
) -> Result<(String, Vec<u8>)> {
    bail!("Safe working-copy preview is currently supported on macOS and Linux")
}

fn preview_bytes(file: &FileChange, old: &[u8], new: &[u8]) -> TextPreview {
    if file.is_submodule()
        && (file.old_path.is_none() || file.old_mode == "160000")
        && (file.new_path.is_none() || file.new_mode == "160000")
    {
        return TextPreview::Submodule {
            old_oid: file.old_oid.clone(),
            new_oid: file.new_oid.clone(),
        };
    }
    if old.len() > MAX_DIFF_BYTES
        || new.len() > MAX_DIFF_BYTES
        || old.iter().filter(|b| **b == b'\n').count() > MAX_DIFF_LINES
        || new.iter().filter(|b| **b == b'\n').count() > MAX_DIFF_LINES
    {
        return TextPreview::TooLarge {
            old_bytes: old.len(),
            new_bytes: new.len(),
        };
    }
    let (Ok(old_text), Ok(new_text)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return TextPreview::Binary;
    };
    if old.contains(&0) || new.contains(&0) {
        return TextPreview::Binary;
    }
    let old_label = file
        .old_path
        .as_ref()
        .map(|p| patch_label("a", p))
        .unwrap_or_else(|| "/dev/null".into());
    let new_label = file
        .new_path
        .as_ref()
        .map(|p| patch_label("b", p))
        .unwrap_or_else(|| "/dev/null".into());
    let mut patch = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_millis(250))
        .diff_lines(old_text, new_text)
        .unified_diff()
        .context_radius(3)
        .header(&old_label, &new_label)
        .to_string();
    if file.old_mode != file.new_mode {
        patch = if file.old_mode == "000000" {
            format!("new file mode {}\n{patch}", file.new_mode)
        } else if file.new_mode == "000000" {
            format!("deleted file mode {}\n{patch}", file.old_mode)
        } else {
            format!(
                "old mode {}\nnew mode {}\n{patch}",
                file.old_mode, file.new_mode
            )
        };
    }
    if patch.is_empty() {
        patch = "File contents are identical.\n".into();
    }
    TextPreview::Patch(patch)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutation_deadline_includes_pipe_holding_hook_descendant_after_parent_exit() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 10 & exit 0"]);
        let start = Instant::now();
        let error = bounded_write_output(command, None, Duration::from_millis(35)).unwrap_err();
        assert!(error.to_string().contains("may have applied"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    #[cfg(unix)]
    #[test]
    fn status_and_write_input_preserve_non_utf8_paths_even_on_utf8_only_filesystems() {
        let state = parse_status(b"? non-\xff\npath\0").unwrap();
        assert_eq!(
            state.entries[0].path.as_os_str().as_encoded_bytes(),
            b"non-\xff\npath"
        );
        assert_eq!(
            path_input(&state.entries[0].paths()).unwrap(),
            b"non-\xff\npath\0"
        );
    }
    #[test]
    fn literal_path_validation_preserves_weird_bytes_but_rejects_escaping_paths() {
        assert!(path_input(&[PathBuf::from("-odd[abc]\nfile")]).is_ok());
        for path in ["../escape", "/absolute", "", "a/../b"] {
            assert!(path_input(&[PathBuf::from(path)]).is_err());
        }
    }
}
