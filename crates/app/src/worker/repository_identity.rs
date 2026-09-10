//! Filesystem identity for worker-owned retained repository processes.
use anyhow::{Result, ensure};
use gitturtle_core::GitRepository;
use std::path::{Path, PathBuf};

/// Cheap worker-side checks retain object readers across normal ref/index edits,
/// but never across replacement of the worktree or its administration directory.
pub(super) struct RepositoryIdentity {
    directories: Vec<(PathBuf, DirectoryIdentity)>,
    links: Vec<(PathBuf, Option<std::fs::Metadata>)>,
}

#[derive(PartialEq)]
struct DirectoryIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    created: std::time::SystemTime,
}

impl DirectoryIdentity {
    fn read(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        ensure!(metadata.is_dir(), "Repository directory was replaced");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        Ok(Self {
            created: metadata.created()?,
        })
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
            DirectoryIdentity::read(path).is_ok_and(|current| current == *identity)
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
