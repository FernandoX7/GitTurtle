//! Filesystem identity for worker-owned retained repository processes.
use anyhow::{Result, ensure};
use gitturtle_core::GitRepository;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Cheap worker-side checks retain object readers across normal ref/index edits,
/// but never across replacement of the worktree or its administration directory.
pub(super) struct RepositoryIdentity {
    directories: Vec<(PathBuf, DirectoryIdentity)>,
    links: Vec<(PathBuf, Option<std::fs::Metadata>)>,
}

struct DirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    /// When the directory was created, where the filesystem records it
    /// (`statx` on Linux, `st_birthtime` on macOS), read by the same
    /// `metadata` call: a directory created after another was deleted can
    /// take its inode number (ext4 and XFS reuse them), and an overlayfs
    /// copy-up keeps the inode while the directory is the upper layer's
    /// new one. Compared only where both reads have one, as
    /// `repository_tabs::same_git_directories` compares the open's.
    #[cfg(unix)]
    created: Option<SystemTime>,
    #[cfg(not(unix))]
    created: SystemTime,
}

#[cfg(all(test, unix))]
thread_local! {
    /// A birth time a test gives the directory at a path on this thread in
    /// place of the one its filesystem records, as a copy-up or a reused
    /// inode number leaves a directory: the same inode, born again.
    static BIRTH: std::cell::RefCell<Option<(PathBuf, SystemTime)>> =
        const { std::cell::RefCell::new(None) };
}

/// Gives the directory at `path` the birth time `born` in every
/// [`DirectoryIdentity`] read on this thread, or none of them with `None`.
#[cfg(all(test, unix))]
pub(super) fn born_for_test(born: Option<(PathBuf, SystemTime)>) {
    BIRTH.with_borrow_mut(|birth| *birth = born);
}

impl DirectoryIdentity {
    fn read(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        ensure!(metadata.is_dir(), "Repository directory was replaced");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let created = metadata.created().ok();
            #[cfg(test)]
            let created = BIRTH
                .with_borrow(|birth| {
                    (birth.as_ref())
                        .filter(|(born, _)| born == path)
                        .map(|(_, at)| *at)
                })
                .or(created);
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
                created,
            })
        }
        #[cfg(not(unix))]
        Ok(Self {
            created: metadata.created()?,
        })
    }

    /// Whether `current` is still this directory: the same device and
    /// inode and, where both reads have a birth time, the same one.
    fn is(&self, current: &Self) -> bool {
        #[cfg(unix)]
        {
            self.device == current.device
                && self.inode == current.inode
                && (self.created)
                    .zip(current.created)
                    .is_none_or(|(before, now)| before == now)
        }
        #[cfg(not(unix))]
        {
            self.created == current.created
        }
    }
}

impl RepositoryIdentity {
    pub(super) fn capture(repository: &GitRepository) -> Result<Self> {
        let (private, common) = repository.git_directories()?;
        let directories = [repository.path(), &private, &common]
            .into_iter()
            .map(|path| Ok((path.to_owned(), DirectoryIdentity::read(path)?)))
            .collect::<Result<_>>()?;
        let links = [repository.path().join(".git"), private.join("commondir")]
            .into_iter()
            .map(|path| {
                let metadata = std::fs::symlink_metadata(&path)
                    .ok()
                    .filter(|metadata| !metadata.is_dir());
                (path, metadata)
            })
            .collect();
        Ok(Self { directories, links })
    }

    pub(super) fn is_current(&self) -> bool {
        self.directories.iter().all(|(path, identity)| {
            DirectoryIdentity::read(path).is_ok_and(|current| identity.is(&current))
        }) && self.links.iter().all(|(path, previous)| {
            let current = std::fs::symlink_metadata(path)
                .ok()
                .filter(|metadata| !metadata.is_dir());
            match (previous, current) {
                (None, None) => true,
                (Some(before), Some(after)) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;
                        // Restoring mtime after an in-place, same-length gitdir
                        // edit must not preserve the old repository reader.
                        if before.dev() != after.dev()
                            || before.ino() != after.ino()
                            || before.ctime() != after.ctime()
                            || before.ctime_nsec() != after.ctime_nsec()
                        {
                            return false;
                        }
                    }
                    before.file_type() == after.file_type()
                        && before.len() == after.len()
                        && before.modified().ok() == after.modified().ok()
                }
                _ => false,
            }
        })
    }
}
