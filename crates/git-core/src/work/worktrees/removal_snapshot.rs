//! Bounded, no-follow inspection of the folder that Git will delete.
use super::*;

#[cfg(unix)]
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, Stat, fstat, open, openat, readlinkat, statat,
};
#[cfg(unix)]
use std::{collections::BTreeSet, ffi::CStr, os::unix::ffi::OsStrExt};

const ENTRY_LIMIT: usize = 100_000;
const DEPTH_LIMIT: usize = 128;
const PATH_BYTES_LIMIT: usize = 16 * 1024 * 1024;
const FILE_BYTES_LIMIT: u64 = 64 * 1024 * 1024;
const TOTAL_BYTES_LIMIT: u64 = 256 * 1024 * 1024;
const SCAN_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn cancelled() -> bool {
    inspection_cancelled()
        || authentication::current_control().is_some_and(|control| control.is_cancelled())
}

/// Git status omits repositories inside tracked directories and does not
/// identify bare repositories. Inspect every directory, including ignored
/// trees, independently of status. Hash the actual bytes that the review
/// proposes discarding; metadata alone cannot establish a stable snapshot.
#[cfg(unix)]
pub(super) fn snapshot(
    root: &Path,
    status: &[StatusEntry],
    ignored: &[PathBuf],
    digest: &mut sha2::Sha256,
) -> Result<()> {
    let mut paths = BTreeSet::new();
    for path in status
        .iter()
        .flat_map(StatusEntry::paths)
        .chain(ignored.iter().cloned())
    {
        validate_path(&path)?;
        paths.insert(path);
    }
    let mut scan = Scan {
        paths,
        digest,
        started: Instant::now(),
        entries: 0,
        path_bytes: 0,
        content_bytes: 0,
    };
    let directory = open(root, directory_flags(), Mode::empty())
        .context("Unable to safely open the worktree folder")?;
    scan.directory(&directory, Path::new(""), 0)
}

#[cfg(not(unix))]
pub(super) fn snapshot(
    _root: &Path,
    _status: &[StatusEntry],
    _ignored: &[PathBuf],
    _digest: &mut sha2::Sha256,
) -> Result<()> {
    bail!("Safe worktree removal inspection currently requires macOS or Linux")
}

#[cfg(unix)]
fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

#[cfg(unix)]
struct Scan<'a> {
    paths: BTreeSet<PathBuf>,
    digest: &'a mut sha2::Sha256,
    started: Instant,
    entries: usize,
    path_bytes: usize,
    content_bytes: u64,
}

#[cfg(unix)]
impl Scan<'_> {
    fn check(&self) -> Result<()> {
        ensure!(!cancelled(), "Repository inspection cancelled");
        ensure!(
            self.started.elapsed() <= SCAN_TIMEOUT,
            "Worktree safety inspection exceeded its 5-second limit"
        );
        Ok(())
    }

    fn directory(
        &mut self,
        directory: &rustix::fd::OwnedFd,
        path: &Path,
        depth: usize,
    ) -> Result<()> {
        self.check()?;
        ensure!(
            depth <= DEPTH_LIMIT,
            "Worktree safety inspection exceeded its directory depth limit"
        );
        let before = fstat(directory)?;
        let mut entries = Vec::new();
        let mut reader = Dir::read_from(directory)
            .with_context(|| format!("Unable to inspect folder {}", path.display()))?;
        while let Some(entry) = reader.read() {
            self.check()?;
            let entry = entry.context("Unable to enumerate every worktree entry")?;
            let name = entry.file_name();
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            // Only the selected worktree's root .git belongs to its captured
            // Git administration. A marker at any deeper level is protected.
            if name.to_bytes().eq_ignore_ascii_case(b".git") {
                ensure!(
                    depth == 0,
                    "This worktree contains a nested Git repository ({}). Move it outside this worktree before removal; its local commits would be deleted.",
                    path.display()
                );
                continue;
            }
            self.entries += 1;
            self.path_bytes = self
                .path_bytes
                .saturating_add(path.as_os_str().len() + name.to_bytes().len() + 1);
            ensure!(
                self.entries <= ENTRY_LIMIT,
                "Worktree safety inspection exceeded its 100,000-entry limit"
            );
            ensure!(
                self.path_bytes <= PATH_BYTES_LIMIT,
                "Worktree safety inspection exceeded its 16 MiB path limit"
            );
            entries.push(name.to_owned());
        }
        drop(reader);
        // HEAD plus an object store can hold unique commits even if refs are
        // packed or use reftable. Conservatively protect incomplete stores as
        // well, without opening repository configuration or following links.
        ensure!(
            !(depth > 0
                && entries
                    .iter()
                    .any(|name| name.to_bytes().eq_ignore_ascii_case(b"HEAD"))
                && entries
                    .iter()
                    .any(|name| name.to_bytes().eq_ignore_ascii_case(b"objects"))),
            "This worktree contains a nested Git repository or retained Git object store ({}). Move it outside this worktree before removal; its local commits would be deleted.",
            path.display()
        );
        entries.sort_by(|left, right| left.to_bytes().cmp(right.to_bytes()));
        self.hash(path.as_os_str().as_bytes());
        self.hash(stat_identity(&before).as_bytes());
        for name in entries {
            self.check()?;
            let child = path.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
            self.entry(directory, &name, &child, depth)
                .with_context(|| format!("Unable to inspect {}", child.display()))?;
        }
        ensure!(
            stat_identity(&before) == stat_identity(&fstat(directory)?),
            "Worktree folder changed during inspection; refresh and review removal again"
        );
        self.check()
    }

    fn entry(
        &mut self,
        directory: &rustix::fd::OwnedFd,
        name: &CStr,
        path: &Path,
        depth: usize,
    ) -> Result<()> {
        let before = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)?;
        self.hash(path.as_os_str().as_bytes());
        self.hash(stat_identity(&before).as_bytes());
        match FileType::from_raw_mode(before.st_mode) {
            FileType::Directory => {
                let child = openat(directory, name, directory_flags(), Mode::empty())?;
                ensure!(
                    stat_identity(&before) == stat_identity(&fstat(&child)?),
                    "Worktree folder changed during inspection"
                );
                self.directory(&child, path, depth + 1)?;
            }
            FileType::Symlink => {
                // Hash the link text, never the destination, even if it names
                // an external directory or special file.
                let target = readlinkat(directory, name, Vec::new())?;
                self.hash(target.to_bytes());
            }
            FileType::RegularFile => {
                if path
                    .ancestors()
                    .any(|ancestor| self.paths.contains(ancestor))
                {
                    self.regular_file(directory, name, &before)?;
                }
            }
            _ => bail!(
                "A special filesystem entry cannot be safely reviewed for removal; move it outside this worktree first"
            ),
        }
        ensure!(
            stat_identity(&before)
                == stat_identity(&statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)?),
            "Worktree content changed during inspection; refresh and review removal again"
        );
        Ok(())
    }

    fn regular_file(
        &mut self,
        directory: &rustix::fd::OwnedFd,
        name: &CStr,
        expected: &Stat,
    ) -> Result<()> {
        let length = u64::try_from(expected.st_size).context("Invalid working file size")?;
        ensure!(
            length <= FILE_BYTES_LIMIT,
            "A file exceeds the 64 MiB removal review limit; move or remove it before reviewing again"
        );
        ensure!(
            self.content_bytes.saturating_add(length) <= TOTAL_BYTES_LIMIT,
            "Affected files exceed the 256 MiB removal review limit; move or remove them before reviewing again"
        );
        // NONBLOCK prevents a file replaced by a FIFO from hanging in open.
        // Verify regular-file type and identity before attempting any read.
        let fd = openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )?;
        let opened = fstat(&fd)?;
        ensure!(
            FileType::from_raw_mode(opened.st_mode) == FileType::RegularFile
                && stat_identity(expected) == stat_identity(&opened),
            "Worktree file changed during inspection"
        );
        let mut file = std::fs::File::from(fd);
        let mut bytes = 0_u64;
        let mut contents = sha2::Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            self.check()?;
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            self.content_bytes += count as u64;
            ensure!(
                bytes <= FILE_BYTES_LIMIT && self.content_bytes <= TOTAL_BYTES_LIMIT,
                "Affected files grew beyond the removal review byte limit"
            );
            contents.update(&buffer[..count]);
        }
        ensure!(
            bytes == length && stat_identity(expected) == stat_identity(&fstat(&file)?),
            "Worktree file changed during inspection; refresh and review removal again"
        );
        self.hash(&contents.finalize());
        Ok(())
    }

    fn hash(&mut self, bytes: &[u8]) {
        self.digest.update((bytes.len() as u64).to_le_bytes());
        self.digest.update(bytes);
    }
}

/// Access time is intentionally excluded: passive reads can update it.
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn scan(digest: &mut sha2::Sha256) -> Scan<'_> {
        Scan {
            paths: BTreeSet::new(),
            digest,
            started: Instant::now(),
            entries: 0,
            path_bytes: 0,
            content_bytes: 0,
        }
    }

    #[test]
    fn active_filesystem_inspection_observes_cancellation() {
        let cancellation = HistoryCancellation::default();
        let mut digest = sha2::Sha256::new();
        let scan = scan(&mut digest);
        let error = run_cancellable_inspection(cancellation.clone(), || {
            scan.check()?;
            cancellation.cancel();
            scan.check()
        })
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        // The scoped signal must not poison the next worktree inspection.
        assert!(scan.check().is_ok());
    }

    #[test]
    fn filesystem_inspection_refuses_entry_path_depth_and_time_limits() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("file"), b"content").unwrap();
        let directory = open(root.path(), directory_flags(), Mode::empty()).unwrap();
        let mut digest = sha2::Sha256::new();

        let mut entries = scan(&mut digest);
        entries.entries = ENTRY_LIMIT;
        assert!(
            entries
                .directory(&directory, Path::new(""), 0)
                .unwrap_err()
                .to_string()
                .contains("entry limit")
        );

        let mut paths = scan(&mut digest);
        paths.path_bytes = PATH_BYTES_LIMIT;
        assert!(
            paths
                .directory(&directory, Path::new(""), 0)
                .unwrap_err()
                .to_string()
                .contains("path limit")
        );

        assert!(
            scan(&mut digest)
                .directory(&directory, Path::new(""), DEPTH_LIMIT + 1)
                .unwrap_err()
                .to_string()
                .contains("depth limit")
        );

        let mut elapsed = scan(&mut digest);
        elapsed.started = Instant::now() - SCAN_TIMEOUT - Duration::from_secs(1);
        assert!(
            elapsed
                .directory(&directory, Path::new(""), 0)
                .unwrap_err()
                .to_string()
                .contains("5-second limit")
        );
    }
}
