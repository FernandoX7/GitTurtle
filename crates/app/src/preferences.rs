//! User preferences live outside inspected repositories. Loading is read-only;
//! explicit successful opens update a small, atomically replaced recent list.

use anyhow::{Context, Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_RECENT: usize = 10;
const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default)]
pub struct Preferences {
    pub recent_repositories: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct StoredPreferences {
    version: u32,
    #[serde(default)]
    recent_repositories: Vec<StoredPath>,
}

// JSON strings keep ordinary settings readable. Unix filenames can contain
// arbitrary bytes, so use a lossless representation for those paths only.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum StoredPath {
    Text(String),
    #[cfg(unix)]
    Bytes {
        unix_bytes: Vec<u8>,
    },
}

impl StoredPath {
    fn from_path(path: &Path) -> Self {
        if let Some(text) = path.to_str() {
            return Self::Text(text.to_owned());
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self::Bytes {
                unix_bytes: path.as_os_str().as_bytes().to_vec(),
            }
        }
        #[cfg(not(unix))]
        Self::Text(path.to_string_lossy().into_owned())
    }

    fn into_path(self) -> PathBuf {
        match self {
            Self::Text(text) => PathBuf::from(text),
            #[cfg(unix)]
            Self::Bytes { unix_bytes } => {
                use std::{ffi::OsString, os::unix::ffi::OsStringExt};
                PathBuf::from(OsString::from_vec(unix_bytes))
            }
        }
    }
}

impl Preferences {
    /// Missing, invalid, or unavailable preferences do not prevent startup.
    /// Loading does not create a settings directory or modify a settings file.
    pub fn load() -> Self {
        settings_path()
            .and_then(|path| Self::load_from(&path))
            .unwrap_or_default()
    }

    pub fn last_repository(&self) -> Option<PathBuf> {
        self.recent_repositories.first().cloned()
    }

    /// Remember an existing path. Canonicalization deduplicates symlink aliases.
    /// A persistence failure leaves the previous in-memory list intact.
    pub fn remember_repository(&mut self, path: &Path) -> Result<()> {
        self.remember_at(path, &settings_path()?)
    }

    fn load_from(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_SETTINGS_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_SETTINGS_BYTES,
            "Settings are too large"
        );
        let stored: StoredPreferences = serde_json::from_slice(&bytes)?;
        ensure!(stored.version == 1, "Unsupported settings version");
        let mut seen = HashSet::new();
        let recent_repositories = stored
            .recent_repositories
            .into_iter()
            .map(StoredPath::into_path)
            .filter(|path| path.is_absolute() && seen.insert(path.clone()))
            .take(MAX_RECENT)
            .collect();
        Ok(Self {
            recent_repositories,
        })
    }

    fn remember_at(&mut self, path: &Path, settings: &Path) -> Result<()> {
        let path = path
            .canonicalize()
            .context("Resolve recent repository path")?;
        let mut recent_repositories = vec![path.clone()];
        let mut seen = HashSet::from([path]);
        recent_repositories.extend(
            self.recent_repositories
                .iter()
                .filter(|path| path.is_absolute() && seen.insert((*path).clone()))
                .take(MAX_RECENT - 1)
                .cloned(),
        );
        let next = Self {
            recent_repositories,
        };
        next.save_to(settings)?;
        *self = next;
        Ok(())
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        let stored = StoredPreferences {
            version: 1,
            recent_repositories: self
                .recent_repositories
                .iter()
                .map(|path| StoredPath::from_path(path))
                .collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&stored)?;
        bytes.push(b'\n');
        ensure!(
            bytes.len() as u64 <= MAX_SETTINGS_BYTES,
            "Settings are too large"
        );
        atomic_write(path, &bytes).context("Save recent repositories")
    }
}

fn absolute_environment_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn settings_path() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let directory = absolute_environment_path("HOME")
        .map(|home| home.join("Library/Application Support/GitTurtle"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let directory = absolute_environment_path("XDG_CONFIG_HOME")
        .or_else(|| absolute_environment_path("HOME").map(|home| home.join(".config")))
        .map(|config| config.join("gitturtle"));
    #[cfg(not(unix))]
    let directory = absolute_environment_path("APPDATA").map(|appdata| appdata.join("GitTurtle"));
    directory
        .map(|directory| directory.join("preferences.json"))
        .ok_or_else(|| anyhow!("The user settings directory is unavailable"))
}

struct PendingFile(PathBuf);

impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .context("Settings path has no parent directory")?;
    ensure!(
        directory.is_absolute(),
        "Settings directory must be absolute"
    );
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory)?;

    // create_new prevents following an existing temporary-file symlink. The
    // final rename occurs on the same filesystem, so readers see a whole file.
    let (pending, mut file) = (0..32)
        .find_map(|_| {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let temporary = directory.join(format!(
                ".preferences.{}.{}.tmp",
                std::process::id(),
                sequence
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&temporary) {
                Ok(file) => Some(Ok((PendingFile(temporary), file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error)),
            }
        })
        .context("Could not reserve an atomic settings file")??;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&pending.0, path)?;
    // Best-effort directory sync adds crash durability where supported. The
    // replacement has already succeeded; an unsupported directory sync should
    // not report a failed save to the user.
    if let Ok(directory_file) = File::open(directory) {
        let _ = directory_file.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "gitturtle-preferences-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn remembers_ten_unique_paths_and_atomically_replaces_settings() {
        let fixture = TestDirectory::new();
        let settings = fixture.0.join("settings/preferences.json");
        let mut preferences = Preferences::default();
        for index in 0..12 {
            let repository = fixture.0.join(format!("repo-{index}"));
            fs::create_dir(&repository).unwrap();
            preferences.remember_at(&repository, &settings).unwrap();
            assert_eq!(fs::read_dir(&repository).unwrap().count(), 0);
        }
        let last = fixture.0.join("repo-5").canonicalize().unwrap();
        preferences.remember_at(&last, &settings).unwrap();
        let loaded = Preferences::load_from(&settings).unwrap();
        assert_eq!(loaded.last_repository(), Some(last));
        assert_eq!(loaded.recent_repositories.len(), MAX_RECENT);
        assert_eq!(loaded.recent_repositories, preferences.recent_repositories);
        assert_eq!(fs::read_dir(settings.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn loading_invalid_settings_does_not_rewrite_them() {
        let fixture = TestDirectory::new();
        let settings = fixture.0.join("preferences.json");
        fs::write(&settings, b"incomplete json").unwrap();
        assert!(Preferences::load_from(&settings).is_err());
        assert_eq!(fs::read(&settings).unwrap(), b"incomplete json");
        let absent = fixture.0.join("absent/preferences.json");
        assert!(Preferences::load_from(&absent).is_err());
        assert!(!absent.parent().unwrap().exists());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_round_trip_without_loss() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let fixture = TestDirectory::new();
        let repository = fixture.0.join(OsString::from_vec(b"repo-\xff".to_vec()));
        // APFS rejects creating this filename, while Unix Git paths can still
        // contain arbitrary bytes. Verify persistence without filesystem I/O
        // against the synthetic repository path.
        let settings = fixture.0.join("settings/preferences.json");
        Preferences {
            recent_repositories: vec![repository.clone()],
        }
        .save_to(&settings)
        .unwrap();
        let loaded = Preferences::load_from(&settings).unwrap();
        assert_eq!(loaded.recent_repositories, vec![repository]);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aliases_are_deduplicated() {
        let fixture = TestDirectory::new();
        let repository = fixture.0.join("repository");
        fs::create_dir(&repository).unwrap();
        let alias = fixture.0.join("alias");
        std::os::unix::fs::symlink(&repository, &alias).unwrap();
        let settings = fixture.0.join("settings/preferences.json");
        let mut preferences = Preferences::default();
        preferences.remember_at(&repository, &settings).unwrap();
        preferences.remember_at(&alias, &settings).unwrap();
        let loaded = Preferences::load_from(&settings).unwrap();
        assert_eq!(
            loaded.recent_repositories,
            vec![repository.canonicalize().unwrap()]
        );
    }
}
