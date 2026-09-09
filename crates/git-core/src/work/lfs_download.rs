//! Explicit, one-pointer LFS fetches. Neither preparation nor downloading
//! checks out, smudges, stages, or rewrites working files.
use super::*;

pub const MAX_LFS_PREVIEW_DOWNLOAD: u64 = 32 * 1024 * 1024;

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

impl GitRepository {
    /// Resolve both available LFS text sides together. Missing objects keep the
    /// original pointer comparison visible; resolved content has no partial
    /// staging semantics because Git's underlying blobs remain pointers.
    pub fn resolved_lfs_text(
        &self,
        file: &FileChange,
        old: &[u8],
        new: &[u8],
    ) -> Result<Option<TextPreviewWithSources>> {
        let mut resolved = [None, None];
        let mut found = false;
        for (index, bytes) in [old, new].into_iter().enumerate() {
            if let Ok((oid, size)) = parse_download_pointer(bytes) {
                found = true;
                let Some(local) =
                    self.local_lfs_object(&oid, size, MAX_LFS_PREVIEW_DOWNLOAD as usize)?
                else {
                    return Ok(None);
                };
                resolved[index] = Some(local);
            }
        }
        if !found {
            return Ok(None);
        }
        let [before, after] = resolved;
        let old = before.unwrap_or_else(|| old.to_vec());
        let new = after.unwrap_or_else(|| new.to_vec());
        Ok(Some(TextPreviewWithSources {
            preview: preview_bytes(file, &old, &new),
            old,
            new,
        }))
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
        // `git lfs env` creates storage directories, even though it is a config
        // query. Redirect that side effect into our private temporary directory.
        let temporary = LfsTemporary::new()?;
        let mut command = normal_command(&self.path);
        command
            .arg("-c")
            .arg(format!("remote.lfsdefault={remote}"))
            .arg("-c")
            .arg(config_path("lfs.storage", &temporary.path))
            .args(["lfs", "env"]);
        let output = bounded_output(command, GIT_TIMEOUT)?;
        ensure!(
            output.status.success(),
            "Git LFS tooling is unavailable or its configuration cannot be read. Install Git LFS, then review this download again. {}",
            redact_diagnostic(&text(&output.stderr))
        );
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
        let mut config = normal_command(&self.path);
        config.args(["config", "--null", "--list"]);
        let output = bounded_output(config, GIT_TIMEOUT)?;
        ensure!(
            output.status.success(),
            "Cannot read the effective Git configuration for LFS."
        );
        let mut digest = Sha256::new();
        digest.update(output.stdout);
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
            let mut hash = normal_command(&self.path);
            hash.env("GIT_OBJECT_DIRECTORY", &objects)
                .args(["hash-object", "-w", "--stdin"]);
            let oid = text(trim_line(
                &checked_write_output(hash, Some(plan.target.pointer.clone()), GIT_TIMEOUT)?.stdout,
            ));
            validate_oid(&oid)?;
            oid
        };
        let mut command = normal_command(&self.path);
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
            message: format!(
                "Downloaded and verified LFS preview for {} · {} bytes. Working files and the index are preserved.",
                plan.target.path.display(),
                plan.size
            ),
            commit_oid: None,
        })
    }
}

pub(crate) fn lfs_storage_config(path: &Path) -> Result<Output> {
    let mut command = normal_command(path);
    command.env("GIT_OPTIONAL_LOCKS", "0").args([
        "config",
        "--null",
        "--path",
        "--get",
        "lfs.storage",
    ]);
    bounded_output(command, GIT_TIMEOUT)
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
