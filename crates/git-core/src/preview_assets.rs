//! Bounded local resources for captured document previews. No URL is opened;
//! metadata resolves only literal repository paths and content is raw blob/file bytes.
use super::*;
use crate::history::{ReadEnd, stream_history};
use std::{collections::HashMap, path::Component};

pub const MAX_PREVIEW_ASSETS: usize = 32;
const MAX_ASSET_PATH: usize = 4096;
const MAX_ASSET_METADATA: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PreviewAssetScope {
    /// A full, immutable commit OID. Mutable revision expressions are refused.
    Revision(String),
    /// A single captured stage-zero metadata listing; bytes come from its OIDs.
    Index,
    /// Tracked regular files captured now, without filters or symlink traversal.
    /// This does not claim an atomic snapshot of multiple working files.
    Worktree,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewAsset {
    pub path: PathBuf,
    pub blob_oid: Option<String>,
    pub mode: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewAssetResult {
    pub destination: String,
    pub asset: Option<PreviewAsset>,
    pub error: Option<String>,
}

#[derive(Clone)]
struct Entry {
    oid: String,
    mode: String,
    conflicted: bool,
}

impl GitRepository {
    /// Capture a finite batch of local image resources relative to the document.
    /// Each image retains its own error; unsafe source scope/document identity is
    /// a batch error. Decoding and aggregate decoded-pixel limits belong to callers.
    ///
    /// For Index/Worktree, `expected_document_oid` checks the selected document's
    /// stage-zero identity before resources are used. Revision checks the document
    /// blob in the captured commit. Worktree assets are raw bytes captured at this
    /// call, not historical substitutes or an atomic multi-file snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn capture_preview_assets(
        &self,
        scope: &PreviewAssetScope,
        document_path: &Path,
        destinations: &[String],
        expected_document_oid: Option<&str>,
        per_asset_limit: usize,
        aggregate_limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<Vec<PreviewAssetResult>> {
        check(cancellation)?;
        ensure!(
            destinations.len() <= MAX_PREVIEW_ASSETS,
            "Document exceeds the 32 local-image limit"
        );
        ensure!(
            (1..=MAX_BLOB_BYTES).contains(&per_asset_limit)
                && (1..=MAX_BLOB_BYTES).contains(&aggregate_limit),
            "Invalid local-image byte budget"
        );
        valid_path(document_path)?;
        if let Some(oid) = expected_document_oid {
            validate_oid(oid)?;
        }
        let paths: Vec<_> = destinations
            .iter()
            .map(|destination| preview_asset_path(document_path, destination))
            .collect();
        let mut requested = vec![document_path.to_path_buf()];
        requested.extend(paths.iter().filter_map(|path| path.as_ref().ok()).cloned());
        requested.sort();
        requested.dedup();
        let metadata = self.asset_entries(scope, &requested, cancellation)?;
        let document = metadata
            .get(document_path)
            .context("The preview document is absent from its captured revision/index scope")?;
        regular(document)?;
        if let Some(expected) = expected_document_oid {
            ensure!(
                document.oid.eq_ignore_ascii_case(expected),
                "Document identity changed before local images could be captured; refresh the preview"
            );
        }
        if *scope == PreviewAssetScope::Worktree {
            open_working_regular(&self.path, document_path)?;
        }
        let mut remaining = aggregate_limit;
        let mut results = Vec::with_capacity(destinations.len());
        for (destination, path) in destinations.iter().zip(paths) {
            check(cancellation)?;
            let asset = (|| {
                let path = path?;
                let entry=metadata.get(&path).context("Local image is absent from the captured revision/index; only tracked regular files are supported")?;
                regular(entry)?;
                ensure!(
                    remaining > 0,
                    "Document local images exceed the aggregate byte budget"
                );
                let limit = per_asset_limit.min(remaining);
                let (bytes, mode, oid) = if *scope == PreviewAssetScope::Worktree {
                    let (bytes, mode) = read_working(&self.path, &path, limit, cancellation)?;
                    (bytes, mode, None)
                } else {
                    (
                        self.asset_blob(&entry.oid, limit, cancellation)?,
                        entry.mode.clone(),
                        Some(entry.oid.clone()),
                    )
                };
                remaining = remaining.saturating_sub(bytes.len());
                Ok(PreviewAsset {
                    path,
                    blob_oid: oid,
                    mode,
                    bytes,
                })
            })();
            // Cancellation stops the batch; it is not presented as dozens of
            // unrelated per-image failures after the selection was dismissed.
            check(cancellation)?;
            results.push(match asset {
                Ok(asset) => PreviewAssetResult {
                    destination: destination.clone(),
                    asset: Some(asset),
                    error: None,
                },
                Err(error) => PreviewAssetResult {
                    destination: destination.clone(),
                    asset: None,
                    error: Some(format!("{error:#}")),
                },
            });
        }
        Ok(results)
    }

    fn asset_entries(
        &self,
        scope: &PreviewAssetScope,
        paths: &[PathBuf],
        cancellation: &HistoryCancellation,
    ) -> Result<HashMap<PathBuf, Entry>> {
        let mut command = asset_command(&self.path);
        match scope {
            PreviewAssetScope::Revision(oid) => {
                validate_oid(oid)?;
                let mut kind = asset_command(&self.path);
                kind.args(["cat-file", "-t", oid]);
                ensure!(
                    trim_line(&read(kind, cancellation, 1024)?) == b"commit",
                    "Local image revision must be a full commit OID"
                );
                command.args(["ls-tree", "-z", "--full-tree", oid, "--"]);
            }
            PreviewAssetScope::Index | PreviewAssetScope::Worktree => {
                ensure!(
                    !self.bare,
                    "A bare repository has no index or working image scope"
                );
                command.args(["ls-files", "--stage", "-z", "--"]);
            }
        }
        command.args(paths);
        let bytes = read(command, cancellation, MAX_ASSET_METADATA)?;
        let mut entries: HashMap<PathBuf, Entry> = HashMap::new();
        for record in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            check(cancellation)?;
            let tab = record
                .iter()
                .position(|b| *b == b'\t')
                .context("Malformed local-image path metadata")?;
            let path = path_from_bytes(&record[tab + 1..]);
            // Literal lookup can still include descendants when the requested
            // target is a directory. They never become an implicit image source.
            if !paths.contains(&path) {
                continue;
            }
            let fields: Vec<_> = record[..tab].split(|b| *b == b' ').collect();
            ensure!(fields.len() == 3, "Malformed local-image entry metadata");
            let (oid, conflicted) = match scope {
                PreviewAssetScope::Revision(_) => {
                    ensure!(
                        matches!(fields[1], b"blob" | b"tree" | b"commit"),
                        "Invalid tree entry type"
                    );
                    (fields[2], false)
                }
                _ => (fields[1], fields[2] != b"0"),
            };
            let oid = std::str::from_utf8(oid)?.to_owned();
            validate_oid(&oid)?;
            let entry = Entry {
                oid,
                mode: std::str::from_utf8(fields[0])?.into(),
                conflicted,
            };
            if let Some(previous) = entries.get_mut(&path) {
                previous.conflicted = true;
            } else {
                entries.insert(path, entry);
            }
        }
        Ok(entries)
    }

    fn asset_blob(
        &self,
        oid: &str,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<Vec<u8>> {
        let mut size = asset_command(&self.path);
        size.args(["cat-file", "-s", oid]);
        let size: usize = std::str::from_utf8(trim_line(&read(size, cancellation, 1024)?))?
            .parse()
            .context("Invalid local-image object size")?;
        ensure!(
            size <= limit,
            "Local image exceeds its remaining encoded-byte budget ({limit} bytes)"
        );
        let mut command = asset_command(&self.path);
        command.args(["cat-file", "blob", oid]);
        let bytes = read(command, cancellation, limit)?;
        ensure!(
            bytes.len() == size,
            "Local-image object length is inconsistent"
        );
        Ok(bytes)
    }
}

/// Percent-decode a relative destination into a byte-safe repository path. URL
/// schemes, absolute paths, query/fragment features and repository escapes are
/// explicitly unsupported. Encoded spaces/non-UTF-8 filename bytes are retained.
pub fn preview_asset_path(document_path: &Path, destination: &str) -> Result<PathBuf> {
    valid_path(document_path)?;
    ensure!(
        !destination.is_empty() && destination.len() <= MAX_ASSET_PATH,
        "Local-image destination is empty or exceeds 4 KiB"
    );
    ensure!(
        !destination.contains(['?', '#']),
        "Local-image query parameters and fragments are unsupported"
    );
    let mut bytes = Vec::with_capacity(destination.len());
    let mut input = destination.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = input
                .next()
                .and_then(hex)
                .context("Invalid percent escape in local-image path")?;
            let low = input
                .next()
                .and_then(hex)
                .context("Invalid percent escape in local-image path")?;
            bytes.push(high * 16 + low);
        } else {
            bytes.push(byte);
        }
    }
    ensure!(
        !bytes.starts_with(b"/")
            && !bytes
                .iter()
                .any(|b| matches!(b, b'\\' | b':' | 0 | b'\r' | b'\n')),
        "Local images must use repository-relative paths; absolute paths, network URLs and schemes are not loaded"
    );
    let mut path = document_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    for component in bytes.split(|b| *b == b'/') {
        match component {
            b"" | b"." => {}
            b".." => {
                ensure!(path.pop(), "Local-image path escapes the repository");
            }
            _ => {
                ensure!(
                    !component.eq_ignore_ascii_case(b".git"),
                    "Repository administration paths are not preview resources"
                );
                path.push(path_from_bytes(component));
            }
        }
    }
    valid_path(&path)?;
    Ok(path)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn valid_path(path: &Path) -> Result<()> {
    ensure!(!path.as_os_str().is_empty() && path.as_os_str().len()<=MAX_ASSET_PATH && path.components().count()<=128
        && path.components().all(|c|matches!(c,Component::Normal(name) if !name.as_encoded_bytes().eq_ignore_ascii_case(b".git")))
        && !path.as_os_str().as_encoded_bytes().contains(&0),"Preview resources require a bounded repository-relative path outside .git");
    Ok(())
}

fn regular(entry: &Entry) -> Result<()> {
    ensure!(
        !entry.conflicted,
        "Local image/document has unresolved index conflicts"
    );
    ensure!(
        matches!(entry.mode.as_str(), "100644" | "100755"),
        "Local previews do not follow stored symlinks, directories or submodules"
    );
    Ok(())
}

fn check(cancellation: &HistoryCancellation) -> Result<()> {
    ensure!(
        !cancellation.is_cancelled(),
        "Local-image capture cancelled"
    );
    Ok(())
}

fn asset_command(root: &Path) -> Command {
    let mut command = git_command(root);
    command
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env_remove("GIT_GLOB_PATHSPECS")
        .env_remove("GIT_NOGLOB_PATHSPECS")
        .env_remove("GIT_ICASE_PATHSPECS");
    command
}

fn read(command: Command, cancellation: &HistoryCancellation, limit: usize) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let end = stream_history(command, None, cancellation, GIT_TIMEOUT, |chunk| {
        ensure!(
            output.len().saturating_add(chunk.len()) <= limit,
            "Local-image read exceeds its byte budget"
        );
        output.extend_from_slice(chunk);
        Ok(true)
    })?;
    ensure!(
        end == ReadEnd::Complete,
        "Local-image read did not complete within its deadline"
    );
    Ok(output)
}

#[cfg(unix)]
fn open_working_regular(root: &Path, path: &Path) -> Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags, open, openat};
    valid_path(path)?;
    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open(root, directory_flags, Mode::empty())
        .context("Cannot safely open the document worktree")?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        if components.peek().is_some() {
            directory = openat(
                &directory,
                component.as_os_str(),
                directory_flags,
                Mode::empty(),
            )
            .context("Local preview cannot follow a symbolic-link directory")?;
        } else {
            let file = std::fs::File::from(
                openat(
                    &directory,
                    component.as_os_str(),
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                    Mode::empty(),
                )
                .context("Local preview cannot open an absent or symbolic-link file")?,
            );
            ensure!(
                file.metadata()?.is_file(),
                "Local preview supports regular files only"
            );
            return Ok(file);
        }
    }
    bail!("Local preview has no filename")
}

#[cfg(not(unix))]
fn open_working_regular(_root: &Path, _path: &Path) -> Result<std::fs::File> {
    bail!("Safe local-image working reads currently require macOS or Linux")
}

#[cfg(unix)]
fn read_working(
    root: &Path,
    path: &Path,
    limit: usize,
    cancellation: &HistoryCancellation,
) -> Result<(Vec<u8>, String)> {
    use std::os::unix::fs::MetadataExt;
    let mut file = open_working_regular(root, path)?;
    let before = file.metadata()?;
    ensure!(
        before.len() <= limit as u64,
        "Working image exceeds its remaining encoded-byte budget ({limit} bytes)"
    );
    let mut bytes = Vec::with_capacity(before.len() as usize);
    let mut chunk = [0; 16 * 1024];
    loop {
        check(cancellation)?;
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        ensure!(
            bytes.len().saturating_add(count) <= limit,
            "Working image grew beyond its byte budget"
        );
        bytes.extend_from_slice(&chunk[..count]);
    }
    let after = file.metadata()?;
    ensure!(
        before.len() == after.len()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "Working image changed while it was being captured; refresh the preview"
    );
    Ok((
        bytes,
        if before.mode() & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        }
        .into(),
    ))
}

#[cfg(not(unix))]
fn read_working(
    _root: &Path,
    _path: &Path,
    _limit: usize,
    _cancellation: &HistoryCancellation,
) -> Result<(Vec<u8>, String)> {
    bail!("Safe local-image working reads currently require macOS or Linux")
}
