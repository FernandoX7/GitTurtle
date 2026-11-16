//! Private copies of captured bytes for the desktop's default application.
//!
//! A Linux handler keeps running after the launcher returns, and some reopen
//! the path later, so a copy outlives the click (unlike Quick Look's, which
//! lives exactly as long as its viewer). Copies stay until this session's
//! bound evicts the oldest or the app quits; a later launch removes what an
//! ended session left behind.
//!
//! Layout, under the user's cache (`$XDG_CACHE_HOME`, else `~/.cache`):
//!
//! ```text
//! gitturtle/captured-copies/            0700, owned by the user, never a symlink
//!   session-<random>/                   0700, one per running GitTurtle process
//!     lock                              0600, held exclusively for the process's life
//!     copy-<random>/                    0500 once written, one per click
//!       Captured revision.<suffix>      0400, the exact captured bytes
//! ```
//!
//! Every directory below the root is created fresh with a random name, and the
//! copy itself with `O_CREAT | O_EXCL | O_NOFOLLOW`, so nothing that already
//! exists, a symlink included, is ever written through. The descriptor handed
//! over is reopened read-only from that path and checked to be the same file.
//!
//! A handler's Save or Save As starts in the copy's folder. That folder is
//! made read-only once the copy is in it, so the user's edits cannot land
//! where eviction or quitting would delete them; it is made writable again
//! only to be removed.
use super::passive::Kind;
use anyhow::{Context as _, Result, ensure};
use gitturtle_preview::MAX_INPUT_BYTES;
use std::{
    collections::VecDeque,
    fs::{self, DirBuilder, File, OpenOptions},
    io::Write as _,
    os::unix::fs::{
        DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
    },
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime},
};

/// At most this many copies per session; opening another removes the oldest.
pub(super) const MAX_COPIES: usize = 8;
/// At most this many bytes per session, four copies at the 32 MiB input bound.
pub(super) const MAX_BYTES: u64 = 128 * 1024 * 1024;
/// A session directory being prepared that is this old was abandoned.
const STAGING_AGE: Duration = Duration::from_secs(60 * 60);
/// The most entries a cleanup inspects, so a flooded folder stays bounded.
const MAX_SCANNED: usize = 1024;

/// The refusal of captured bytes larger than [`MAX_INPUT_BYTES`], typed so
/// the line beneath the control can say so in its own words
/// ([`super::Category::TooLarge`]); its sentence is the refusal's.
#[derive(Debug)]
pub(super) struct TooLarge;

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The captured file exceeds the 32 MiB limit for opening it in another app.")
    }
}

impl std::error::Error for TooLarge {}
const SESSION: &str = "session-";
const STAGING: &str = ".staging-";

/// One written copy and a read-only descriptor of exactly that file.
pub(super) struct Copy {
    pub(super) path: PathBuf,
    pub(super) directory: PathBuf,
    pub(super) file: File,
}

pub(super) struct Store {
    root: PathBuf,
    max_copies: usize,
    max_bytes: u64,
    session: Mutex<Option<Session>>,
}

struct Session {
    directory: PathBuf,
    /// Held exclusively until the process ends, which tells a later cleanup
    /// whether the session is still running.
    _lock: File,
    copies: VecDeque<(PathBuf, u64)>,
    bytes: u64,
}

/// The process's store under the user's cache, or `None` when the
/// environment names no absolute cache or home directory.
pub(super) fn shared() -> Option<&'static Store> {
    static STORE: OnceLock<Option<Store>> = OnceLock::new();
    STORE
        .get_or_init(|| default_root().map(Store::new))
        .as_ref()
}

#[cfg(not(test))]
fn default_root() -> Option<PathBuf> {
    let absolute = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    absolute("XDG_CACHE_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".cache")))
        .map(|cache| cache.join("gitturtle").join("captured-copies"))
}

/// Tests never write to the user's cache.
#[cfg(test)]
fn default_root() -> Option<PathBuf> {
    Some(
        crate::preferences::test_settings_directory()
            .path()
            .join("captured-copies"),
    )
}

impl Store {
    pub(super) fn new(root: PathBuf) -> Self {
        Self::with_limits(root, MAX_COPIES, MAX_BYTES)
    }

    fn with_limits(root: PathBuf, max_copies: usize, max_bytes: u64) -> Self {
        Self {
            root,
            max_copies,
            max_bytes,
            session: Mutex::new(None),
        }
    }

    /// Writes `bytes` as a new private copy typed by `kind`, first removing
    /// this session's oldest copies to stay within its bounds.
    pub(super) fn write(&self, bytes: &[u8], kind: Kind) -> Result<Copy> {
        ensure!(bytes.len() <= MAX_INPUT_BYTES, TooLarge);
        let length = bytes.len() as u64;
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if session.is_none() {
            *session = Some(self.open_session()?);
        }
        let session = session.as_mut().expect("opened above");
        while session.copies.len() >= self.max_copies || session.bytes + length > self.max_bytes {
            let Some((directory, size)) = session.copies.pop_front() else {
                break;
            };
            remove_tree(&directory);
            session.bytes -= size;
        }
        let directory = tempfile::Builder::new()
            .prefix("copy-")
            .rand_bytes(16)
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&session.directory)
            .context("Could not create a private folder for the copy")?
            .keep();
        let copy = write_copy(&directory, bytes, kind)
            .and_then(|copy| {
                set_mode(&directory, 0o500)
                    .context("Could not make the copy's folder read-only")?;
                Ok(copy)
            })
            .inspect_err(|_| {
                remove_tree(&directory);
            })?;
        session.copies.push_back((directory, length));
        session.bytes += length;
        Ok(copy)
    }

    /// Removes the sessions whose process has ended (their lock is free) and
    /// abandoned staging folders, and returns how many it removed. A missing
    /// store is empty; a root that is not a private directory of this user is
    /// left alone.
    pub(super) fn remove_stale(&self) -> Result<usize> {
        if fs::symlink_metadata(&self.root)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            return Ok(0);
        }
        open_private(&self.root)?;
        let entries = fs::read_dir(&self.root)?;
        let own = self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|session| session.directory.clone());
        let mut removed = 0;
        for entry in entries.take(MAX_SCANNED).flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.file_type().is_dir() || Some(&path) == own.as_ref() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Held until the removal below ends, so a second cleanup sees the
            // session as running rather than removing it at the same time.
            let mut held = None;
            let stale = if name.starts_with(SESSION) {
                match OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                    .open(path.join("lock"))
                {
                    // Free means the process that held it has ended.
                    Ok(lock) if lock.try_lock().is_ok() => {
                        held = Some(lock);
                        true
                    }
                    Ok(_) => false,
                    Err(error) => error.kind() == std::io::ErrorKind::NotFound,
                }
            } else if name.starts_with(STAGING) {
                metadata
                    .modified()
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age > STAGING_AGE)
            } else {
                false
            };
            if stale && remove_tree(&path) {
                removed += 1;
            }
            drop(held);
        }
        Ok(removed)
    }

    /// Removes this session's folder and its copies, when the app quits.
    pub(super) fn close(&self) {
        if let Some(session) = self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            remove_tree(&session.directory);
        }
    }

    /// Prepares the session under a staging name with its lock already held,
    /// then renames it into place, so a concurrent cleanup never sees a
    /// session whose lock is free because it has not been taken yet.
    fn open_session(&self) -> Result<Session> {
        private_directory(&self.root)?;
        let staging = tempfile::Builder::new()
            .prefix(STAGING)
            .rand_bytes(16)
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&self.root)
            .context("Could not create this session's private folder")?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(staging.path().join("lock"))?;
        lock.lock()?;
        let token = staging
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(STAGING))
            .context("Unexpected staging folder name")?
            .to_owned();
        let directory = self.root.join(format!("{SESSION}{token}"));
        ensure!(
            fs::symlink_metadata(&directory).is_err(),
            "A session folder with this name already exists"
        );
        fs::rename(staging.path(), &directory)?;
        // The staging guard would now remove nothing; release it.
        let _ = staging.keep();
        Ok(Session {
            directory,
            _lock: lock,
            copies: VecDeque::new(),
            bytes: 0,
        })
    }
}

fn write_copy(directory: &Path, bytes: &[u8], kind: Kind) -> Result<Copy> {
    let path = directory.join(format!("Captured revision.{}", kind.extension()));
    let mut written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .context("Could not create the private copy")?;
    written.write_all(bytes)?;
    // The portal refuses a writable descriptor for a read-only request, and
    // the handler gets no way to change the copy.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)?;
    let (created, reopened) = (written.metadata()?, file.metadata()?);
    ensure!(
        created.dev() == reopened.dev()
            && created.ino() == reopened.ino()
            && reopened.len() == bytes.len() as u64
            && reopened.mode() & 0o777 == 0o400,
        "The private copy changed before it could be opened"
    );
    Ok(Copy {
        path,
        directory: directory.to_owned(),
        file,
    })
}

/// Creates `path` (and missing parents) as this user's private directory, or
/// accepts it when it already is one, tightening its mode to 0700 through the
/// descriptor that was checked, so a path swapped in between is never changed.
fn private_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let directory = open_private(path)?;
    if directory.metadata()?.mode() & 0o077 != 0 {
        directory.set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Opens a real directory (never a symlink to one) that this user owns.
fn open_private(path: &Path) -> Result<File> {
    let refused = || {
        format!(
            "{} is not a private folder of this user, so no captured bytes are written there",
            path.display()
        )
    };
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .with_context(refused)?;
    let metadata = directory.metadata()?;
    // SAFETY: geteuid has no preconditions and cannot fail.
    let user = unsafe { libc::geteuid() };
    ensure!(metadata.is_dir() && metadata.uid() == user, refused());
    Ok(directory)
}

/// Sets a directory's mode through a descriptor opened without following
/// a symlink, so a path swapped for a link is never changed.
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?
        .set_permissions(fs::Permissions::from_mode(mode))
}

/// Removes a session or copy folder, and reports whether it went. Copy
/// folders are read-only, so the folder and the folders directly in it are
/// made writable first; `remove_dir_all` does not follow symlinks inside
/// the tree.
fn remove_tree(path: &Path) -> bool {
    let _ = set_mode(path, 0o700);
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.take(MAX_SCANNED).flatten() {
            let _ = set_mode(&entry.path(), 0o700);
        }
    }
    fs::remove_dir_all(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().mode() & 0o777
    }

    #[test]
    fn copies_are_exact_private_read_only_and_typed_by_content() {
        let home = tempfile::tempdir().unwrap();
        let store = Store::new(home.path().join("cache/gitturtle/captured-copies"));
        let bytes = b"%PDF-1.7\n%\xe2\xe3\n";
        let copy = store.write(bytes, Kind::Pdf).unwrap();
        // The name is fixed and the suffix is the kind's, never a stored name.
        assert_eq!(copy.path.file_name().unwrap(), "Captured revision.pdf");
        assert_eq!(fs::read(&copy.path).unwrap(), bytes);
        assert_eq!(mode(&copy.path), 0o400);
        let session = copy.directory.parent().unwrap();
        for directory in [&store.root, session] {
            assert_eq!(mode(directory), 0o700, "{}", directory.display());
        }
        // A handler's Save As starts here, and nothing saved here survives
        // eviction or quitting, so nothing can be saved here.
        assert_eq!(mode(&copy.directory), 0o500);
        // SAFETY: geteuid has no preconditions; root ignores the mode.
        if unsafe { libc::geteuid() } != 0 {
            let saved = fs::write(copy.directory.join("Edited.pdf"), b"edits");
            assert_eq!(
                saved.unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
        assert_eq!(mode(&session.join("lock")), 0o600);
        // The handed-over descriptor is read-only and is the written file.
        let flags =
            unsafe { libc::fcntl(std::os::fd::AsRawFd::as_raw_fd(&copy.file), libc::F_GETFL) };
        assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
        assert_eq!(
            copy.file.metadata().unwrap().ino(),
            fs::metadata(&copy.path).unwrap().ino()
        );
        // Two clicks never share a folder.
        let second = store.write(bytes, Kind::Pdf).unwrap();
        assert_ne!(second.directory, copy.directory);
        assert_eq!(second.directory.parent(), Some(session));
        store.close();
        assert!(!session.exists(), "read-only folders are still removed");
    }

    #[test]
    fn an_existing_name_or_symlink_is_never_written_through() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("victim");
        fs::write(&target, b"keep").unwrap();
        let directory = home.path().join("copy");
        fs::create_dir(&directory).unwrap();
        std::os::unix::fs::symlink(&target, directory.join("Captured revision.txt")).unwrap();
        assert!(write_copy(&directory, b"replaced", Kind::Text).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        fs::remove_file(directory.join("Captured revision.txt")).unwrap();
        fs::write(directory.join("Captured revision.txt"), b"existing").unwrap();
        assert!(write_copy(&directory, b"replaced", Kind::Text).is_err());
        assert_eq!(
            fs::read(directory.join("Captured revision.txt")).unwrap(),
            b"existing"
        );
        // A store root that is a symlink to elsewhere is refused.
        let elsewhere = home.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let root = home.path().join("captured-copies");
        std::os::unix::fs::symlink(&elsewhere, &root).unwrap();
        assert!(Store::new(root).write(b"text", Kind::Text).is_err());
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
    }

    #[test]
    fn a_broader_root_is_tightened_to_owner_only() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("captured-copies");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
        let store = Store::new(root.clone());
        store.write(b"text", Kind::Text).unwrap();
        assert_eq!(mode(&root), 0o700);
        store.close();
    }

    fn session_copies(session: &Path) -> (usize, u64) {
        fs::read_dir(session)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("copy-"))
            .flat_map(|entry| fs::read_dir(entry.path()).unwrap().flatten())
            .fold((0, 0), |(count, bytes), file| {
                (count + 1, bytes + file.metadata().unwrap().len())
            })
    }

    #[test]
    fn the_session_bounds_its_copies_by_count_and_bytes_removing_the_oldest() {
        // Four copies at the 32 MiB input bound fill a session.
        assert_eq!((MAX_COPIES, MAX_BYTES), (8, 4 * MAX_INPUT_BYTES as u64));
        let home = tempfile::tempdir().unwrap();
        let store = Store::with_limits(home.path().join("captured-copies"), 3, 100);
        let copies: Vec<Copy> = (0..5)
            .map(|index| {
                store
                    .write(format!("copy {index}").as_bytes(), Kind::Text)
                    .unwrap()
            })
            .collect();
        let present = |copy: &Copy| copy.path.exists();
        assert!(!present(&copies[0]) && !present(&copies[1]));
        assert!(copies[2..].iter().all(present));
        let session = copies[0].directory.parent().unwrap().to_owned();
        assert_eq!(session_copies(&session).0, 3);
        // A large copy removes copies from the oldest until it fits.
        let large = store.write(&[b'a'; 90], Kind::Text).unwrap();
        assert!(present(&large) && present(&copies[4]));
        assert!(!present(&copies[2]) && !present(&copies[3]));
        assert_eq!(session_copies(&session), (2, 96));
        let whole = store.write(&[b'b'; 100], Kind::Text).unwrap();
        assert!(!present(&large) && present(&whole));
        assert_eq!(session_copies(&session), (1, 100));
        store.close();
        // Larger than the limit, the refusal is typed and says why.
        let Err(refused) = store.write(&vec![b'a'; MAX_INPUT_BYTES + 1], Kind::Text) else {
            panic!("a copy over the limit is refused");
        };
        assert!(refused.is::<TooLarge>(), "{refused:#}");
        assert_eq!(
            format!("{refused:#}"),
            "The captured file exceeds the 32 MiB limit for opening it in another app."
        );
        assert_eq!(MAX_INPUT_BYTES, 32 << 20, "the sentence names the limit");
    }

    #[test]
    fn cleanup_removes_ended_sessions_and_keeps_running_ones() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("captured-copies");
        let running = Store::new(root.clone());
        let live = running.write(b"live", Kind::Text).unwrap();
        // An ended session: its lock file exists but nobody holds it.
        let ended = root.join("session-ended");
        fs::create_dir(&ended).unwrap();
        fs::write(ended.join("lock"), b"").unwrap();
        fs::create_dir(ended.join("copy-old")).unwrap();
        fs::write(ended.join("copy-old/Captured revision.txt"), b"old").unwrap();
        fs::set_permissions(ended.join("copy-old"), fs::Permissions::from_mode(0o500)).unwrap();
        // A fresh staging folder may belong to a session starting now.
        let staging = root.join(".staging-fresh");
        fs::create_dir(&staging).unwrap();
        // Unrelated entries and symlinks are never followed or removed.
        let outside = home.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("session-link")).unwrap();

        let later = Store::new(root.clone());
        assert_eq!(later.remove_stale().unwrap(), 1);
        assert!(!ended.exists());
        assert!(live.path.exists(), "a running session's copies stay");
        assert!(staging.exists());
        assert!(outside.join("keep").exists());

        // On exit the session removes its own folder.
        running.close();
        assert!(!live.directory.parent().unwrap().exists());
        assert!(
            Store::new(home.path().join("missing"))
                .remove_stale()
                .unwrap()
                == 0
        );
    }
}
