//! Bounded, passive local reflog inspection and explicit branch-only recovery.
use super::*;
use std::io::{Seek, SeekFrom};

const REFLOG_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_REFLOG_ENTRIES: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflogEntry {
    pub selector: String,
    pub reference: String,
    pub oid: String,
    pub previous_oid: String,
    pub timestamp: i64,
    pub message: String,
    pub ordinal: usize,
    root: PathBuf,
    snapshot: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflogPage {
    pub reference: String,
    pub entries: Vec<ReflogEntry>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflogRecoveryPlan {
    pub entry: ReflogEntry,
    pub branch: String,
    pub commit: Commit,
    directories: (PathBuf, PathBuf),
}

impl GitRepository {
    /// HEAD is private to this worktree; branch reflogs belong to shared refs.
    /// Read raw records so an expired object does not hide otherwise useful log
    /// entries. No object lookup or subprocess is performed per displayed row.
    pub fn reflog(&self, reference: &str) -> Result<ReflogPage> {
        ensure!(
            reference == "HEAD" || reference.starts_with("refs/heads/"),
            "Choose this worktree's HEAD or an exact local branch reference."
        );
        if reference != "HEAD" {
            self.validate_branch(reference.strip_prefix("refs/heads/").unwrap_or_default())?;
        }
        let (private, common) = self.git_directories()?;
        let directory = if reference == "HEAD" { private } else { common };
        let (bytes, tail_only) =
            read_reflog_tail(&directory, &PathBuf::from("logs").join(reference))?;
        let snapshot = format!("{:x}", Sha256::digest(&bytes));
        let records: Vec<_> = bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .collect();
        let truncated = tail_only || records.len() > MAX_REFLOG_ENTRIES;
        let mut entries = Vec::new();
        for (ordinal, record) in records
            .into_iter()
            .rev()
            .take(MAX_REFLOG_ENTRIES)
            .enumerate()
        {
            // Git can write a valid entry without a reason (notably the first
            // entry in a newly created linked worktree's HEAD log).
            let (metadata, message) = record
                .iter()
                .position(|byte| *byte == b'\t')
                .map(|separator| (&record[..separator], &record[separator + 1..]))
                .unwrap_or((record, &[]));
            let mut fields = metadata.split(|byte| *byte == b' ');
            let previous_oid = text(fields.next().context("Malformed previous reflog object")?);
            let oid = text(fields.next().context("Malformed reflog object")?);
            validate_oid(&previous_oid)?;
            validate_oid(&oid)?;
            let mut ending = metadata.rsplit(|byte| *byte == b' ');
            let _zone = ending.next().context("Malformed reflog timezone")?;
            let timestamp =
                std::str::from_utf8(ending.next().context("Malformed reflog timestamp")?)?
                    .parse::<i64>()
                    .context("Invalid reflog timestamp")?;
            entries.push(ReflogEntry {
                selector: format!("{reference}@{{{ordinal}}}"),
                reference: reference.into(),
                oid,
                previous_oid,
                timestamp,
                message: redact_diagnostic(&text(message))
                    .chars()
                    .take(2048)
                    .collect(),
                ordinal,
                root: self.path.clone(),
                snapshot: snapshot.clone(),
            });
        }
        Ok(ReflogPage {
            reference: reference.into(),
            entries,
            truncated,
        })
    }

    pub fn inspect_reflog_commit(&self, entry: &ReflogEntry) -> Result<Commit> {
        ensure!(
            entry.root == self.path,
            "This reflog entry belongs to a different worktree."
        );
        validate_oid(&entry.oid)?;
        self.history_from(&entry.oid, 1).with_context(|| "This commit object is unavailable locally. Reflog entries can outlive their objects after expiration or garbage collection. Recovery cannot recreate missing objects and does not fetch them.")?
            .into_iter().next().context("This reflog entry has no available commit object; it may have expired or been pruned.")
    }

    pub fn reflog_recovery_plan(
        &self,
        entry: &ReflogEntry,
        branch: &str,
    ) -> Result<ReflogRecoveryPlan> {
        self.revalidate_reflog_entry(entry)?;
        self.validate_branch(branch)?;
        ensure!(
            !self
                .branches()?
                .iter()
                .any(|item| !item.remote && item.name == branch),
            "Branch '{branch}' already exists. Choose a new recovery branch name."
        );
        let commit = self.inspect_reflog_commit(entry)?;
        Ok(ReflogRecoveryPlan {
            entry: entry.clone(),
            branch: branch.into(),
            commit,
            directories: self.git_directories()?,
        })
    }

    fn revalidate_reflog_entry(&self, entry: &ReflogEntry) -> Result<()> {
        ensure!(
            entry.root == self.path,
            "The selected worktree changed. Reopen its reflog."
        );
        let fresh = self.reflog(&entry.reference)?;
        ensure!(
            fresh.entries.get(entry.ordinal) == Some(entry),
            "This reflog changed or expired after selection. Refresh the reflog and review the target again."
        );
        Ok(())
    }

    pub(super) fn execute_reflog_recovery(
        &self,
        plan: &ReflogRecoveryPlan,
    ) -> Result<WriteOutcome> {
        ensure!(
            self.git_directories()? == plan.directories,
            "The repository identity changed. Review recovery again."
        );
        ensure!(
            self.reflog_recovery_plan(&plan.entry, &plan.branch)? == *plan,
            "The recovery target changed. Refresh the reflog and review it again."
        );
        let mut command = normal_command(&self.path);
        command.args(["branch", "--no-track", "--", &plan.branch, &plan.entry.oid]);
        checked_write_output(command, None, WRITE_TIMEOUT)?;
        Ok(WriteOutcome {
            message: format!(
                "Created recovery branch '{}' at {}. Your current branch, index, and working files are preserved.",
                plan.branch, plan.entry.oid
            ),
            commit_oid: None,
        })
    }
}

#[cfg(unix)]
fn read_reflog_tail(root: &Path, path: &Path) -> Result<(Vec<u8>, bool)> {
    use rustix::fs::{Mode, OFlags, open, openat};
    validate_path(path)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open(root, flags, Mode::empty())?;
    let components: Vec<_> = path.components().collect();
    for component in &components[..components.len() - 1] {
        match openat(&directory, component.as_os_str(), flags, Mode::empty()) {
            Ok(next) => directory = next,
            Err(rustix::io::Errno::NOENT) => return Ok((Vec::new(), false)),
            Err(error) => {
                return Err(error)
                    .context("Cannot safely read a symbolic-link or unavailable reflog directory");
            }
        }
    }
    let name = components
        .last()
        .context("Missing reflog filename")?
        .as_os_str();
    let fd = match openat(
        &directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => return Ok((Vec::new(), false)),
        Err(error) => {
            return Err(error).context("Cannot safely read a symbolic-link or unavailable reflog");
        }
    };
    let mut file = std::fs::File::from(fd);
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "A reflog must be a regular file.");
    let offset = metadata.len().saturating_sub(REFLOG_BYTES as u64);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(REFLOG_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= REFLOG_BYTES,
        "The reflog changed while reading. Refresh and try again."
    );
    if offset > 0 {
        let start = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(bytes.len());
        bytes.drain(..start);
    }
    ensure!(
        bytes.is_empty() || bytes.ends_with(b"\n"),
        "The reflog is being written or has an incomplete record. Refresh after the current operation finishes."
    );
    Ok((bytes, offset > 0))
}

#[cfg(not(unix))]
fn read_reflog_tail(_root: &Path, _path: &Path) -> Result<(Vec<u8>, bool)> {
    bail!("Safe local reflog inspection is currently supported on macOS and Linux")
}
