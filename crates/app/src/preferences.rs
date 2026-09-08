//! User preferences live outside inspected repositories. Loading is read-only;
//! Writes merge the latest stored recents/settings before atomic replacement.
//! A single application executor serializes these operations off the UI thread.

use crate::{
    appearance::{Density, ThemeChoice},
    columns::ColumnSettings,
};
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: ThemeChoice,
    pub columns: ColumnSettings,
    pub density: Density,
    pub reopen_last: bool,
    pub default_branch: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            columns: ColumnSettings::default(),
            density: Density::default(),
            reopen_last: true,
            default_branch: "main".into(),
        }
    }
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.columns.normalize();
    }

    /// Validate an explicit settings edit before persistence. This is only the
    /// preference boundary; Git operations still validate their actual targets.
    pub fn validate(&self) -> Result<()> {
        let branch = &self.default_branch;
        ensure!(
            !branch.is_empty() && branch.len() <= 255,
            "Default branch must contain between 1 and 255 bytes"
        );
        ensure!(
            !branch.starts_with('-')
                && branch != "@"
                && branch != "HEAD"
                && !branch.ends_with('.')
                && !branch.contains("..")
                && !branch.contains("@{")
                && !branch
                    .bytes()
                    .any(|byte| byte <= b' ' || byte == 0x7f || b"~^:?*[\\".contains(&byte))
                && branch.split('/').all(|part| !part.is_empty()
                    && !part.starts_with('.')
                    && !part.ends_with(".lock")),
            "Default branch must be a valid Git branch name, such as main or team/main"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Preferences {
    pub recent_repositories: Vec<PathBuf>,
    pub settings: AppSettings,
}

#[derive(Serialize, Deserialize)]
struct StoredPreferences {
    version: u32,
    #[serde(default)]
    recent_repositories: Vec<StoredPath>,
    #[serde(default)]
    settings: AppSettings,
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

    /// Run on the same serialized executor as recent-repository updates. Read
    /// the latest recents now, not from a possibly stale UI preferences snapshot.
    pub fn save_settings(settings: &AppSettings) -> Result<Self> {
        Self::save_settings_at(settings, &settings_path()?)
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
        ensure!(
            matches!(stored.version, 1 | 2),
            "Unsupported settings version"
        );
        let mut seen = HashSet::new();
        let recent_repositories = stored
            .recent_repositories
            .into_iter()
            .map(StoredPath::into_path)
            .filter(|path| path.is_absolute() && seen.insert(path.clone()))
            .take(MAX_RECENT)
            .collect();
        let mut settings = stored.settings;
        settings.normalize();
        if settings.validate().is_err() {
            settings.default_branch = AppSettings::default().default_branch;
        }
        Ok(Self {
            recent_repositories,
            settings,
        })
    }

    fn load_for_write(path: &Path) -> Result<Self> {
        match Self::load_from(path) {
            Ok(preferences) => Ok(preferences),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(Self::default())
            }
            Err(error) => Err(error).context("Read current preferences before saving"),
        }
    }

    fn save_settings_at(settings: &AppSettings, path: &Path) -> Result<Self> {
        settings.validate()?;
        let mut next = Self::load_for_write(path)?;
        next.settings = settings.clone();
        next.settings.normalize();
        next.save_to(path)?;
        Ok(next)
    }

    fn remember_at(&mut self, path: &Path, settings: &Path) -> Result<()> {
        let path = path
            .canonicalize()
            .context("Resolve recent repository path")?;
        let current = Self::load_for_write(settings)?;
        let mut recent_repositories = vec![path.clone()];
        let mut seen = HashSet::from([path]);
        recent_repositories.extend(
            current
                .recent_repositories
                .iter()
                .filter(|path| path.is_absolute() && seen.insert((*path).clone()))
                .take(MAX_RECENT - 1)
                .cloned(),
        );
        let next = Self {
            recent_repositories,
            settings: current.settings,
        };
        next.save_to(settings)?;
        *self = next;
        Ok(())
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        let stored = StoredPreferences {
            version: 2,
            recent_repositories: self
                .recent_repositories
                .iter()
                .map(|path| StoredPath::from_path(path))
                .collect(),
            settings: self.settings.clone(),
        };
        let mut bytes = serde_json::to_vec_pretty(&stored)?;
        bytes.push(b'\n');
        ensure!(
            bytes.len() as u64 <= MAX_SETTINGS_BYTES,
            "Settings are too large"
        );
        atomic_write(path, &bytes).context("Save preferences")
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

    #[test]
    fn version_one_recents_migrate_without_writes_and_keep_default_settings() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let repository = fixture.0.join("old-project");
        let original = serde_json::to_vec(&serde_json::json!({
            "version": 1, "recent_repositories": [repository, "relative/path", repository]
        }))
        .unwrap();
        fs::write(&path, &original).unwrap();
        let loaded = Preferences::load_from(&path).unwrap();
        assert_eq!(loaded.recent_repositories, vec![repository]);
        assert_eq!(loaded.settings, AppSettings::default());
        assert_eq!(fs::read(&path).unwrap(), original);
        let saved = Preferences::save_settings_at(&loaded.settings, &path).unwrap();
        assert_eq!(saved.recent_repositories, loaded.recent_repositories);
        let encoded: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(encoded["version"], 2);
    }

    #[test]
    fn settings_save_keeps_new_recents_and_stale_recent_update_keeps_new_settings() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let first = fixture.0.join("first");
        let second = fixture.0.join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let mut stale = Preferences::default();
        let mut latest = Preferences::default();
        latest.remember_at(&first, &path).unwrap();
        let mut edited = AppSettings {
            theme: ThemeChoice::Daylight,
            density: Density::Compact,
            reopen_last: false,
            default_branch: "team/main".into(),
            ..Default::default()
        };
        edited
            .columns
            .set_visible(crate::columns::ColumnId::Author, false);
        edited
            .columns
            .set_width(crate::columns::ColumnId::Graph, 250.);
        let saved = Preferences::save_settings_at(&edited, &path).unwrap();
        assert_eq!(saved.last_repository(), Some(first.canonicalize().unwrap()));
        stale.remember_at(&second, &path).unwrap();
        let loaded = Preferences::load_from(&path).unwrap();
        assert_eq!(loaded.settings, edited);
        assert_eq!(
            loaded.recent_repositories,
            vec![
                second.canonicalize().unwrap(),
                first.canonicalize().unwrap()
            ]
        );
        assert_eq!(stale.settings, edited);
    }

    #[test]
    fn loaded_settings_repair_bounds_and_invalid_branch_without_changing_file() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let original = br#"{"version":2,"settings":{"theme":"unknown","density":"unknown","default_branch":"--upload-pack=evil","columns":{"subject":{"visible":false,"width":1},"graph":{"width":999999}}}}"#;
        fs::write(&path, original).unwrap();
        let loaded = Preferences::load_from(&path).unwrap();
        assert_eq!(loaded.settings.theme, ThemeChoice::Midnight);
        assert_eq!(loaded.settings.density, Density::Comfortable);
        assert_eq!(loaded.settings.default_branch, "main");
        assert!(loaded.settings.columns.subject.visible);
        assert_eq!(loaded.settings.columns.subject.width, 180.);
        assert_eq!(loaded.settings.columns.graph.width, 480.);
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn invalid_branch_edits_and_unsupported_store_do_not_destroy_preferences() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        Preferences::save_settings_at(&AppSettings::default(), &path).unwrap();
        let original = fs::read(&path).unwrap();
        for branch in [
            "",
            "-main",
            "HEAD",
            "HEAD.lock",
            "a b",
            "a..b",
            "a@{b",
            ".hidden",
            "a/.hidden",
            "a/b.lock",
            "a/",
            "a//b",
            "a\\b",
            "a?b",
            "a\nb",
            "a.",
            "@",
        ] {
            let edited = AppSettings {
                default_branch: branch.into(),
                ..Default::default()
            };
            assert!(
                Preferences::save_settings_at(&edited, &path).is_err(),
                "{branch:?}"
            );
            assert_eq!(fs::read(&path).unwrap(), original);
        }
        for branch in ["main", "team/main", "release-v1.2", "développement"] {
            AppSettings {
                default_branch: branch.into(),
                ..Default::default()
            }
            .validate()
            .unwrap();
        }
        for unsupported in [br#"{"version":99}"#.as_slice(), b"broken"] {
            fs::write(&path, unsupported).unwrap();
            assert!(Preferences::save_settings_at(&AppSettings::default(), &path).is_err());
            assert_eq!(fs::read(&path).unwrap(), unsupported);
        }
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
            ..Default::default()
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
