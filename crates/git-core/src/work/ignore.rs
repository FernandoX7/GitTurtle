//! Explicit literal ignore rules, prepared from untracked working paths.
use super::*;

const IGNORE_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgnoreDestination {
    Shared,
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnorePlan {
    pub path: PathBuf,
    pub directory: bool,
    pub destination: IgnoreDestination,
    pub destination_path: PathBuf,
    /// Exact bytes of the rule, excluding its line ending.
    pub rule: Vec<u8>,
    /// Existing tracked paths remain tracked even for a directory rule.
    pub tracked_paths: usize,
    root: PathBuf,
    selected_path: PathBuf,
    snapshot: IgnoreSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IgnoreSnapshot {
    bytes: Option<Vec<u8>>,
    identity: Option<(u64, u64, u32)>,
    directory_identity: (u64, u64),
}

impl IgnorePlan {
    pub fn rule_display(&self) -> String {
        match std::str::from_utf8(&self.rule) {
            Ok(rule) => rule.to_owned(),
            Err(_) => self
                .rule
                .iter()
                .map(|byte| {
                    if byte.is_ascii_graphic() || *byte == b' ' {
                        (*byte as char).to_string()
                    } else {
                        format!("\\x{byte:02x}")
                    }
                })
                .collect(),
        }
    }
    pub fn rule_is_utf8(&self) -> bool {
        std::str::from_utf8(&self.rule).is_ok()
    }
}

impl GitRepository {
    /// Directory selects this untracked file's immediate containing directory.
    pub fn ignore_plan(
        &self,
        selected_path: &Path,
        directory: bool,
        destination: IgnoreDestination,
    ) -> Result<IgnorePlan> {
        ensure!(!self.bare, "Open a working copy to ignore files.");
        validate_path(selected_path)?;
        let status = self.status()?;
        ensure!(
            status
                .entries
                .iter()
                .any(|entry| entry.path == selected_path && entry.untracked),
            "This file is no longer untracked. Refresh Changes; ignore rules never untrack files."
        );
        let path = if directory {
            selected_path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .context("This file is at the repository root; choose the file rule.")?
                .to_owned()
        } else {
            selected_path.to_owned()
        };
        validate_path(&path)?;
        let rule = literal_ignore_rule(&path, directory)?;
        let destination_path = match destination {
            IgnoreDestination::Shared => self.path.join(".gitignore"),
            IgnoreDestination::Local => self.git_directories()?.1.join("info/exclude"),
        };
        ensure!(
            self.path.join(&path) != destination_path,
            "The ignore file cannot ignore itself; choose repository-local excludes."
        );
        let snapshot = read_ignore_snapshot(&destination_path)?;
        let mut command = git_command(&self.path);
        command
            .args(["--literal-pathspecs", "ls-files", "-z", "--"])
            .arg(&path);
        let output = bounded_output(command, GIT_TIMEOUT)?;
        ensure!(
            output.status.success(),
            "Unable to inspect tracked paths: {}",
            text(&output.stderr)
        );
        let tracked_paths = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .count();
        Ok(IgnorePlan {
            path,
            directory,
            destination,
            destination_path,
            rule,
            tracked_paths,
            root: self.path.clone(),
            selected_path: selected_path.to_owned(),
            snapshot,
        })
    }

    pub(super) fn execute_ignore(&self, plan: &IgnorePlan) -> Result<WriteOutcome> {
        ensure!(
            plan.root == self.path
                && self.ignore_plan(&plan.selected_path, plan.directory, plan.destination)?
                    == *plan,
            "The file, ignore destination, or tracked paths changed; review the ignore rule again."
        );
        let existing = plan.snapshot.bytes.as_deref().unwrap_or_default();
        let newline: &[u8] = if existing.windows(2).any(|pair| pair == b"\r\n") {
            b"\r\n"
        } else {
            b"\n"
        };
        let mut replacement = existing.to_vec();
        if !replacement.is_empty() && !replacement.ends_with(b"\n") {
            replacement.extend_from_slice(newline);
        }
        replacement.extend_from_slice(&plan.rule);
        replacement.extend_from_slice(newline);
        ensure!(
            replacement.len() <= IGNORE_LIMIT,
            "The resulting ignore file would exceed 1 MiB."
        );
        publish_ignore(&plan.destination_path, &plan.snapshot, &replacement)?;
        Ok(WriteOutcome {
            message: format!(
                "Added an ignore rule for '{}'. No files were staged or untracked.",
                plan.path.display()
            ),
            commit_oid: None,
        })
    }
}

fn literal_ignore_rule(path: &Path, directory: bool) -> Result<Vec<u8>> {
    let bytes = path.as_os_str().as_encoded_bytes();
    ensure!(
        !bytes.iter().any(|byte| matches!(byte, 0 | b'\n' | b'\r')),
        "Git ignore rules cannot represent a filename containing a line break or NUL byte."
    );
    let mut rule = vec![b'/'];
    for &byte in bytes {
        if matches!(byte, b'\\' | b'*' | b'?' | b'[' | b']' | b'!' | b'#' | b' ') {
            rule.push(b'\\');
        }
        rule.push(byte);
    }
    if directory {
        rule.push(b'/');
    }
    Ok(rule)
}

#[cfg(unix)]
fn ignore_directory(path: &Path) -> Result<std::os::fd::OwnedFd> {
    use rustix::fs::{Mode, OFlags, open, openat};
    ensure!(path.is_absolute(), "Ignore destination must be absolute.");
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open("/", flags, Mode::empty())?;
    for component in path
        .parent()
        .context("Missing ignore directory")?
        .components()
    {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => {
                directory = openat(&directory, part, flags, Mode::empty())
                    .context("Ignore files cannot follow symbolic-link directories")?
            }
            _ => bail!("Invalid ignore destination"),
        }
    }
    Ok(directory)
}

#[cfg(unix)]
fn snapshot_at(directory: &std::os::fd::OwnedFd, name: &std::ffi::OsStr) -> Result<IgnoreSnapshot> {
    use rustix::fs::{Mode, OFlags, openat};
    use std::os::unix::fs::MetadataExt;
    let directory_meta = std::fs::File::from(directory.try_clone()?).metadata()?;
    let directory_identity = (directory_meta.dev(), directory_meta.ino());
    let fd = match openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => {
            return Ok(IgnoreSnapshot {
                bytes: None,
                identity: None,
                directory_identity,
            });
        }
        Err(error) => {
            return Err(error)
                .context("Ignore destination must be a regular file, never a symbolic link");
        }
    };
    let mut file = std::fs::File::from(fd);
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= IGNORE_LIMIT as u64,
        "Ignore destination must be a regular file no larger than 1 MiB."
    );
    let mut bytes = Vec::new();
    (&mut file)
        .take(IGNORE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= IGNORE_LIMIT && !bytes.contains(&0),
        "Ignore destination is oversized or contains NUL bytes."
    );
    Ok(IgnoreSnapshot {
        bytes: Some(bytes),
        identity: Some((metadata.dev(), metadata.ino(), metadata.mode())),
        directory_identity,
    })
}

#[cfg(unix)]
fn read_ignore_snapshot(path: &Path) -> Result<IgnoreSnapshot> {
    snapshot_at(
        &ignore_directory(path)?,
        path.file_name().context("Missing ignore filename")?,
    )
}

#[cfg(unix)]
fn publish_ignore(path: &Path, expected: &IgnoreSnapshot, bytes: &[u8]) -> Result<()> {
    use rustix::fs::{AtFlags, Mode, OFlags, openat, renameat, unlinkat};
    use std::os::unix::fs::PermissionsExt;
    let directory = ignore_directory(path)?;
    let name = path.file_name().context("Missing ignore filename")?;
    let mut lock_name = name.to_os_string();
    lock_name.push(".gitturtle-lock");
    let fd = openat(&directory, &lock_name, OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::from_raw_mode(0o600)).context("The ignore file is locked by another GitTurtle action. Inspect the prior action before retrying.")?;
    let result = (|| -> Result<()> {
        ensure!(
            snapshot_at(&directory, name)? == *expected,
            "The ignore file changed; review the exact rule and destination again."
        );
        let mut file = std::fs::File::from(fd);
        let mode = expected.identity.map_or(0o644, |(_, _, mode)| mode & 0o777);
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        ensure!(
            snapshot_at(&directory, name)? == *expected,
            "The ignore file changed while preparing the rule; no replacement was made."
        );
        renameat(&directory, &lock_name, &directory, name)
            .context("Unable to publish the ignore rule")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = unlinkat(&directory, &lock_name, AtFlags::empty());
    }
    result
}

#[cfg(not(unix))]
fn read_ignore_snapshot(_: &Path) -> Result<IgnoreSnapshot> {
    bail!("Safe ignore-file editing is supported on macOS and Linux.")
}
#[cfg(not(unix))]
fn publish_ignore(_: &Path, _: &IgnoreSnapshot, _: &[u8]) -> Result<()> {
    bail!("Safe ignore-file editing is supported on macOS and Linux.")
}
