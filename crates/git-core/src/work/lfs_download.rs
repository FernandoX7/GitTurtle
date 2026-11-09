//! Explicit, one-pointer LFS fetches. Neither preparation nor downloading
//! checks out, smudges, stages, or rewrites working files.
use super::*;

pub const MAX_LFS_PREVIEW_DOWNLOAD: u64 = 32 * 1024 * 1024;

/// Earlier Git LFS lets a repository's `.lfsconfig` or submodule configuration
/// choose a command that runs while fetching (CVE-2026-105802, CVE-2026-105803),
/// with no workaround short of upgrading, so older tooling is refused before it
/// reads the repository's configuration.
const MIN_LFS_VERSION: (u64, u64, u64) = (3, 8, 1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LfsDownloadTarget {
    pub path: PathBuf,
    /// Committed/index pointer blob, or None for raw working-copy content.
    pub blob_oid: Option<String>,
    pub pointer: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LfsDownloadPlan {
    pub target: LfsDownloadTarget,
    pub oid: String,
    pub size: u64,
    pub remote: String,
    /// Redacted effective endpoint reported by the configured Git LFS tooling.
    pub source: String,
    root: PathBuf,
    directories: (PathBuf, PathBuf),
    storage: PathBuf,
    config_token: String,
}

/// What is wrong with a Git LFS object a preview cannot use, judged of that
/// object alone: a failure of the repository it is read through is an error
/// instead. A download cannot help either, so neither is offered one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfsObjectFailure {
    /// Its pointer declares more than a preview reads, so the store is not
    /// read at all.
    TooLarge,
    /// The store holds something that is not the object its pointer names,
    /// or that cannot be read safely.
    Corrupt,
}

/// What the local LFS store held for one pointer
/// ([`GitRepository::local_lfs_read`]).
#[derive(Debug)]
pub(crate) enum LocalLfsObject {
    /// The object, matching the pointer's size and digest.
    Verified(Vec<u8>),
    /// None there: a download could still bring it.
    Absent,
    /// There or declared, and unusable: why, and in what words.
    Failed(LfsObjectFailure, anyhow::Error),
}

/// What the local LFS store held for the two sides of a text comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedLfsText {
    /// The comparison with every object that could be shown, or `None` to
    /// keep the two stored texts.
    pub text: Option<TextPreviewWithSources>,
    /// Which sides, old then new, are pointers whose object was found absent
    /// from the local store. `false` means not found absent, not present: a
    /// side can be no pointer, or its object there but binary or too large
    /// to set as text, or one of those [`Self::failed`] names. A pointer's
    /// text cannot say which.
    pub missing: [bool; 2],
    /// Which sides, old then new, are pointers whose object could not be
    /// used, and why. Such a side stays its pointer, as a missing one does,
    /// and the other side keeps its preview; none of them is also `missing`.
    pub failed: [Option<LfsObjectFailure>; 2],
}

impl GitRepository {
    /// Resolve each LFS text side on its own, from the local store alone: a
    /// side whose object is there shows it, and a side whose object is
    /// missing or unusable stays its pointer. `text` is `None`, keeping the
    /// original pointer comparison visible, with no side resolved or with a
    /// resolved side that has no text to set beside the other's pointer;
    /// `missing` and `failed` say which sides are which either way. Resolved
    /// content has no partial staging semantics because Git's underlying
    /// blobs remain pointers.
    ///
    /// The read stays passive: it opens the local store and runs no process
    /// that writes, whatever the pointers name.
    pub fn resolved_lfs_text(
        &self,
        file: &FileChange,
        old: &[u8],
        new: &[u8],
    ) -> Result<ResolvedLfsText> {
        let mut resolved = [None, None];
        let mut missing = [false; 2];
        let mut failed = [None; 2];
        for (index, bytes) in [old, new].into_iter().enumerate() {
            let Ok((oid, size)) = parse_download_pointer(bytes) else {
                continue;
            };
            match self.local_lfs_read(&oid, size, MAX_LFS_PREVIEW_DOWNLOAD as usize) {
                Ok(LocalLfsObject::Verified(local)) => resolved[index] = Some(local),
                Ok(LocalLfsObject::Absent) => missing[index] = true,
                Ok(LocalLfsObject::Failed(failure, _)) => failed[index] = Some(failure),
                // A failure of the repository itself, after a side was
                // already found out, still leaves the two pointers.
                Err(_) if missing.contains(&true) || failed.iter().any(Option::is_some) => {
                    return Ok(ResolvedLfsText {
                        text: None,
                        missing,
                        failed,
                    });
                }
                Err(error) => return Err(error),
            }
        }
        let stored = ResolvedLfsText {
            text: None,
            missing,
            failed,
        };
        if resolved.iter().all(Option::is_none) {
            return Ok(stored);
        }
        let [before, after] = resolved;
        let old = before.unwrap_or_else(|| old.to_vec());
        let new = after.unwrap_or_else(|| new.to_vec());
        let preview = preview_bytes(file, &old, &new);
        let a_pointer_stays = missing.contains(&true) || failed.iter().any(Option::is_some);
        if a_pointer_stays && !matches!(preview, TextPreview::Patch(_)) {
            return Ok(stored);
        }
        Ok(ResolvedLfsText {
            text: Some(TextPreviewWithSources { preview, old, new }),
            missing,
            failed,
        })
    }

    pub fn lfs_download_plan(
        &self,
        target: &LfsDownloadTarget,
        remote: &str,
    ) -> Result<LfsDownloadPlan> {
        validate_path(&target.path)?;
        let (oid, size) = parse_download_pointer(&target.pointer)?;
        ensure!(
            size <= MAX_LFS_PREVIEW_DOWNLOAD,
            "This LFS object is {size} bytes, above the 32 MiB preview input limit. Downloading it here would not enable a preview."
        );
        self.validate_remote(remote)?;
        let bytes = if let Some(blob_oid) = &target.blob_oid {
            validate_oid(blob_oid)?;
            ensure!(
                self.blob_size(blob_oid)? <= 1024,
                "The selected object is not a bounded Git LFS pointer."
            );
            self.blob(blob_oid)?
        } else {
            let (mode, bytes) = read_worktree_file(&self.path, &target.path, "100644")?;
            ensure!(
                mode != "120000",
                "Git LFS downloads do not follow a working-file symlink."
            );
            bytes
        };
        ensure!(
            bytes == target.pointer,
            "The selected LFS pointer changed. Refresh the preview and review the download again."
        );
        ensure!(
            self.local_lfs_object(&oid, size, MAX_LFS_PREVIEW_DOWNLOAD as usize)?
                .is_none(),
            "This verified LFS object is already available locally. Refresh the preview to display it."
        );
        let directories = self.git_directories()?;
        let storage = self.preview_lfs_storage()?;
        let mut version = normal_command(self.target());
        version.args(["lfs", "version"]);
        let output = bounded_output(version, GIT_TIMEOUT)?;
        if !output.status.success() {
            return Err(git_failure(
                &output.stderr,
                format!(
                    "Git LFS tooling is unavailable. Install Git LFS 3.8.1 or newer, then review this download again. {}",
                    reported_redacted(&output.stderr, redact_diagnostic)
                ),
            ));
        }
        require_patched_lfs(&text(&output.stdout))?;
        // `git lfs env` creates storage directories, even though it is a config
        // query. Redirect that side effect into our private temporary directory.
        let temporary = LfsTemporary::new()?;
        let mut command = normal_command(self.target());
        command
            .arg("-c")
            .arg(format!("remote.lfsdefault={remote}"))
            .arg("-c")
            .arg(config_path("lfs.storage", &temporary.path))
            .args(["lfs", "env"]);
        let output = bounded_output(command, GIT_TIMEOUT)?;
        if !output.status.success() {
            return Err(git_failure(
                &output.stderr,
                format!(
                    "Git LFS tooling is unavailable or its configuration cannot be read. Install Git LFS, then review this download again. {}",
                    reported_redacted(&output.stderr, redact_diagnostic)
                ),
            ));
        }
        let environment = text(&output.stdout);
        let endpoints = environment
            .lines()
            .filter(|line| {
                line.starts_with("Endpoint=")
                    || line.starts_with(&format!("Endpoint ({remote})="))
                    || line.starts_with(&format!("Endpoint({remote})="))
            })
            .collect::<Vec<_>>()
            .join("\n");
        ensure!(
            !endpoints.is_empty(),
            "Git LFS did not report a download endpoint for this remote. Check its LFS URL configuration."
        );
        let config = effective_lfs_config(self.target())?;
        let mut digest = Sha256::new();
        digest.update(config);
        digest.update(endpoints.as_bytes());
        Ok(LfsDownloadPlan {
            target: target.clone(),
            oid,
            size,
            remote: remote.into(),
            source: redact_diagnostic(&endpoints),
            root: self.path.clone(),
            directories,
            storage,
            config_token: format!("{:x}", digest.finalize()),
        })
    }

    pub(super) fn execute_lfs_download(&self, plan: &LfsDownloadPlan) -> Result<WriteOutcome> {
        ensure!(
            self.path == plan.root && self.git_directories()? == plan.directories,
            "The selected repository changed. Review the LFS download again."
        );
        ensure!(
            self.lfs_download_plan(&plan.target, &plan.remote)? == *plan,
            "The LFS endpoint, configuration, or pointer changed after review. Review the download again."
        );
        let temporary = LfsTemporary::new()?;
        let objects = temporary.path.join("objects");
        let pointer_oid = if let Some(oid) = &plan.target.blob_oid {
            oid.clone()
        } else {
            std::fs::create_dir(&objects)?;
            let mut hash = normal_command(self.target());
            hash.env("GIT_OBJECT_DIRECTORY", &objects)
                .args(["hash-object", "-w", "--stdin"]);
            let oid = text(trim_line(
                &checked_write_output(hash, Some(plan.target.pointer.clone()), GIT_TIMEOUT)?.stdout,
            ));
            validate_oid(&oid)?;
            oid
        };
        let mut command = normal_command(self.target());
        configure_network(&mut command, self)?;
        // A blob has no reachable commits or sibling files. Passing exactly one
        // pointer blob to --all scans and transfers exactly that object's OID.
        // This also avoids path-glob ambiguity and implicit recent-ref fetches.
        command.args([
            "-c",
            "lfs.concurrenttransfers=1",
            "-c",
            "lfs.transfer.maxretries=0",
            "-c",
            "lfs.fetchrecentalways=false",
            "lfs",
            "fetch",
            "--all",
            &plan.remote,
            &pointer_oid,
        ]);
        if plan.target.blob_oid.is_none() {
            command.env("GIT_OBJECT_DIRECTORY", &objects);
        }
        checked_write_output(command, None, NETWORK_TIMEOUT)?;
        ensure!(
            self.local_lfs_object(&plan.oid, plan.size, MAX_LFS_PREVIEW_DOWNLOAD as usize)?
                .is_some(),
            "Git LFS finished without making the selected object available. Check the configured source and server object; no working files were changed."
        );
        Ok(WriteOutcome {
            message: downloaded(&plan.target.path, plan.size),
            commit_oid: None,
            up_to_date: false,
        })
    }
}

/// The effective configuration a download plan's digest covers. A failure is
/// typed as any configuration read's is ([`git_failure`]): in a linked
/// worktree whose administration directory is gone even `git config` finds
/// no repository.
pub(super) fn effective_lfs_config(target: GitTarget<'_>) -> Result<Vec<u8>> {
    let mut config = normal_command(target);
    config.args(["config", "--null", "--list"]);
    let output = bounded_output(config, GIT_TIMEOUT)?;
    if !output.status.success() {
        return Err(git_failure(
            &output.stderr,
            "Cannot read the effective Git configuration for LFS.".into(),
        ));
    }
    Ok(output.stdout)
}

pub(crate) fn lfs_storage_config(target: GitTarget<'_>) -> Result<Output> {
    let mut command = normal_command(target);
    command.env("GIT_OPTIONAL_LOCKS", "0").args([
        "config",
        "--null",
        "--path",
        "--get",
        "lfs.storage",
    ]);
    bounded_output(command, GIT_TIMEOUT)
}

/// Accepts `git lfs version` output naming an exact release at or above
/// [`MIN_LFS_VERSION`]; anything unrecognized is refused rather than assumed.
fn require_patched_lfs(output: &str) -> Result<()> {
    let found = output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("git-lfs/"))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default();
    let parts = found
        .split('.')
        .map(|part| {
            part.bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| part.parse::<u64>().ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>();
    let Some(&[major, minor, patch]) = parts.as_deref() else {
        bail!(
            "Cannot determine the installed Git LFS version. Install Git LFS 3.8.1 or newer, then review this download again."
        );
    };
    ensure!(
        (major, minor, patch) >= MIN_LFS_VERSION,
        "Git LFS {major}.{minor}.{patch} can run a command chosen by a repository's configuration while fetching (CVE-2026-105802, CVE-2026-105803). Update Git LFS to 3.8.1 or newer, then review this download again."
    );
    Ok(())
}

fn config_path(key: &str, path: &Path) -> OsString {
    let mut value = OsString::from(key);
    value.push("=");
    value.push(path);
    value
}

fn parse_download_pointer(bytes: &[u8]) -> Result<(String, u64)> {
    ensure!(
        bytes.len() <= 1024,
        "The selected content exceeds the LFS pointer limit."
    );
    let value = std::str::from_utf8(bytes)?;
    let mut lines = value.lines();
    ensure!(
        lines.next() == Some("version https://git-lfs.github.com/spec/v1"),
        "The selected content is not a Git LFS v1 pointer."
    );
    let (mut oid, mut size) = (None, None);
    for line in lines {
        let (key, value) = line.split_once(' ').context("Malformed Git LFS pointer")?;
        match key {
            "oid" => {
                ensure!(oid.is_none(), "Duplicate LFS object ID");
                let hex = value
                    .strip_prefix("sha256:")
                    .context("Unsupported LFS object hash")?;
                ensure!(
                    hex.len() == 64
                        && hex
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                    "Invalid LFS SHA-256 object ID"
                );
                oid = Some(hex.to_owned());
            }
            "size" => {
                ensure!(
                    size.is_none()
                        && !value.is_empty()
                        && value.bytes().all(|b| b.is_ascii_digit()),
                    "Invalid LFS object size"
                );
                size = Some(value.parse::<u64>()?);
            }
            _ if key.starts_with("ext-") && !value.is_empty() => {}
            _ => bail!("Unsupported LFS pointer field"),
        }
    }
    Ok((
        oid.context("Missing LFS object ID")?,
        size.context("Missing LFS object size")?,
    ))
}

struct LfsTemporary {
    path: PathBuf,
}
impl LfsTemporary {
    fn new() -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "gitturtle-lfs-{}-{time}-{}",
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
            .create(&path)
            .context("Create private temporary LFS inspection storage")?;
        Ok(Self { path })
    }
}
impl Drop for LfsTemporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// What a verified download of `path`'s object, `size` bytes, says it did,
/// its size agreeing with its noun: a one-byte object read "1 bytes".
fn downloaded(path: &Path, size: u64) -> String {
    format!(
        "Downloaded and verified LFS preview for {} · {}. Working files and the index are preserved.",
        path.display(),
        crate::counted(size, "byte", "bytes")
    )
}

#[cfg(test)]
mod tests {
    use super::require_patched_lfs;

    #[test]
    fn a_download_counts_its_bytes_with_their_noun() {
        assert_eq!(
            super::downloaded(std::path::Path::new("tiny.bin"), 1),
            "Downloaded and verified LFS preview for tiny.bin · 1 byte. Working files and the index are preserved."
        );
        assert!(super::downloaded(std::path::Path::new("image.png"), 2).contains(" · 2 bytes."));
    }

    #[test]
    fn lfs_downloads_require_a_release_with_the_configuration_command_fixes() {
        for patched in [
            "git-lfs/3.8.1 (GitHub; darwin arm64; go 1.25.3)\n",
            "git-lfs/3.9.0 (GitHub; linux amd64; go 1.26.1)",
            "git-lfs/3.10.0 (Debian; linux arm64; go 1.26.2)",
            "git-lfs/4.0.0",
        ] {
            assert!(require_patched_lfs(patched).is_ok(), "{patched}");
        }
        for vulnerable in [
            "git-lfs/3.8.0 (GitHub; linux amd64; go 1.25.1)",
            "git-lfs/3.7.12 (GitHub; darwin arm64; go 1.24.6)",
            "git-lfs/2.13.3 (GitHub; linux amd64; go 1.16.2)",
        ] {
            let error = require_patched_lfs(vulnerable).unwrap_err().to_string();
            assert!(error.contains("CVE-2026-105803"), "{error}");
            assert!(error.contains("3.8.1 or newer"), "{error}");
        }
        for unknown in [
            "",
            "git version 2.55.0",
            "git-lfs/3.8.1-pre (GitHub; linux amd64)",
            "git-lfs/3.8 (GitHub)",
            "git-lfs/3.8.1.1",
            "git-lfs/+3.8.1",
            "xgit-lfs/3.8.1",
        ] {
            let error = require_patched_lfs(unknown).unwrap_err().to_string();
            assert!(error.starts_with("Cannot determine"), "{unknown}: {error}");
        }
    }
}
