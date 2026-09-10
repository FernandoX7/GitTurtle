//! User preferences live outside inspected repositories. Loading is read-only;
//! Writes merge the latest stored recents/settings before atomic replacement.
//! A single application executor serializes these operations off the UI thread.

use crate::{
    appearance::{Density, ThemeChoice},
    columns::ColumnSettings,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_RECENT: usize = 10;
const MAX_SETTINGS_BYTES: u64 = 8 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: ThemeChoice,
    pub follow_system: bool,
    pub external_editor: String,
    pub columns: ColumnSettings,
    pub density: Density,
    pub graph_spacing: u8,
    pub navigation_width: f32,
    pub inspector_width: f32,
    pub interface_text_size: u8,
    pub code_text_size: u8,
    pub reopen_last: bool,
    pub default_branch: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            follow_system: false,
            external_editor: String::new(),
            columns: ColumnSettings::default(),
            density: Density::default(),
            graph_spacing: 20,
            navigation_width: 220.,
            inspector_width: 320.,
            interface_text_size: crate::appearance::DEFAULT_INTERFACE_TEXT_SIZE,
            code_text_size: crate::appearance::DEFAULT_CODE_TEXT_SIZE,
            reopen_last: true,
            default_branch: "main".into(),
        }
    }
}

impl AppSettings {
    pub fn resolved_theme(&self, appearance: gpui_kit::WindowAppearance) -> ThemeChoice {
        if !self.follow_system {
            return self.theme;
        }
        match appearance {
            gpui_kit::WindowAppearance::Light | gpui_kit::WindowAppearance::VibrantLight => {
                ThemeChoice::Daylight
            }
            _ => {
                if self.theme.is_light() {
                    ThemeChoice::Midnight
                } else {
                    self.theme
                }
            }
        }
    }

    pub fn normalize(&mut self) {
        self.columns.normalize();
        self.graph_spacing = self.graph_spacing.clamp(12, 32);
        self.navigation_width = if self.navigation_width.is_finite() {
            self.navigation_width.clamp(180., 360.)
        } else {
            220.
        };
        self.inspector_width = if self.inspector_width.is_finite() {
            self.inspector_width.clamp(280., 480.)
        } else {
            320.
        };
        self.interface_text_size = self.interface_text_size.clamp(
            *crate::appearance::INTERFACE_TEXT_RANGE.start(),
            *crate::appearance::INTERFACE_TEXT_RANGE.end(),
        );
        self.code_text_size = self.code_text_size.clamp(
            *crate::appearance::CODE_TEXT_RANGE.start(),
            *crate::appearance::CODE_TEXT_RANGE.end(),
        );
    }

    /// Validate an explicit settings edit before persistence. This is only the
    /// preference boundary; Git operations still validate their actual targets.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            crate::appearance::INTERFACE_TEXT_RANGE.contains(&self.interface_text_size)
                && crate::appearance::CODE_TEXT_RANGE.contains(&self.code_text_size),
            "Choose interface text from 11 to 18 points and code text from 10 to 24 points"
        );
        ensure!(
            self.external_editor.len() <= 4096
                && !self.external_editor.contains(['\0', '\n', '\r']),
            "Editor must be an application name or executable path of at most 4,096 bytes"
        );
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitDraft {
    pub title: String,
    pub description: String,
}

impl CommitDraft {
    /// Only the separator is supplied by the application. Whitespace, comment
    /// lines, Markdown, and trailing newlines belong to the user's message.
    pub fn message(&self) -> String {
        if self.description.is_empty() {
            self.title.clone()
        } else {
            format!("{}\n\n{}", self.title, self.description)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.title.is_empty() && self.description.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Preferences {
    pub recent_repositories: Vec<PathBuf>,
    pub settings: AppSettings,
    pub commit_drafts: HashMap<PathBuf, CommitDraft>,
}

#[derive(Serialize, Deserialize)]
struct StoredPreferences {
    version: u32,
    #[serde(default)]
    recent_repositories: Vec<StoredPath>,
    #[serde(default)]
    settings: AppSettings,
    #[serde(default)]
    commit_drafts: Vec<StoredDraft>,
}

#[derive(Serialize, Deserialize)]
struct StoredDraft {
    worktree: StoredPath,
    draft: CommitDraft,
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

    /// Keys must be canonical worktree roots resolved during repository
    /// discovery. Do not key drafts by a branch, common Git directory, or the
    /// initially requested folder: linked worktrees need independent drafts.
    pub fn save_commit_drafts(drafts: &HashMap<PathBuf, CommitDraft>) -> Result<()> {
        Self::save_commit_drafts_at(drafts, &settings_path()?)
    }

    fn save_commit_drafts_at(drafts: &HashMap<PathBuf, CommitDraft>, path: &Path) -> Result<()> {
        let mut next = Self::load_for_write(path)?;
        for (worktree, draft) in drafts {
            ensure!(worktree.is_absolute(), "Draft worktree must be absolute");
            if draft.is_empty() {
                next.commit_drafts.remove(worktree);
            } else {
                next.commit_drafts.insert(worktree.clone(), draft.clone());
            }
        }
        next.save_to(path)
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
            matches!(stored.version, 1..=3),
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
            commit_drafts: stored
                .commit_drafts
                .into_iter()
                .filter_map(|stored| {
                    let path = stored.worktree.into_path();
                    (path.is_absolute() && !stored.draft.is_empty()).then_some((path, stored.draft))
                })
                .collect(),
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
            commit_drafts: current.commit_drafts,
        };
        next.save_to(settings)?;
        *self = next;
        Ok(())
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        let stored = StoredPreferences {
            version: 3,
            recent_repositories: self
                .recent_repositories
                .iter()
                .map(|path| StoredPath::from_path(path))
                .collect(),
            settings: self.settings.clone(),
            commit_drafts: {
                let mut entries: Vec<_> = self.commit_drafts.iter().collect();
                entries.sort_by_key(|(path, _)| *path);
                entries
                    .into_iter()
                    .filter(|(_, draft)| !draft.is_empty())
                    .map(|(path, draft)| StoredDraft {
                        worktree: StoredPath::from_path(path),
                        draft: draft.clone(),
                    })
                    .collect()
            },
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

#[cfg(not(test))]
fn absolute_environment_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(not(test))]
pub(super) fn settings_path() -> Result<PathBuf> {
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
        .ok_or_else(|| anyhow::anyhow!("The user settings directory is unavailable"))
}

// Full-app tests run real background and quit-time preference writes. Resolve
// their defaults here, on every thread, without changing process environment or
// allowing an integration fixture to replace a developer's saved session.
#[cfg(test)]
pub(super) fn settings_path() -> Result<PathBuf> {
    static DIRECTORY: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let directory = DIRECTORY.get_or_init(|| {
        tempfile::Builder::new()
            .prefix("gitturtle-app-tests-")
            .tempdir()
            .expect("create isolated test preferences")
    });
    Ok(directory.path().join("preferences.json"))
}

struct PendingFile(PathBuf);

impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
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

    #[test]
    fn text_sizes_migrate_independently_and_reject_unsupported_values() {
        let old: AppSettings =
            serde_json::from_str(r#"{"theme":"nord","density":"compact"}"#).unwrap();
        assert_eq!((old.interface_text_size, old.code_text_size), (13, 12));
        let mut changed = old.clone();
        changed.interface_text_size = 18;
        changed.code_text_size = 24;
        changed.validate().unwrap();
        let loaded: AppSettings =
            serde_json::from_str(&serde_json::to_string(&changed).unwrap()).unwrap();
        assert_eq!(loaded, changed);
        assert_eq!(loaded.theme, ThemeChoice::Nord);
        assert_eq!(loaded.density, Density::Compact);
        changed.interface_text_size = 0;
        changed.code_text_size = 255;
        assert!(changed.validate().is_err());
        changed.normalize();
        assert_eq!(
            (changed.interface_text_size, changed.code_text_size),
            (11, 24)
        );
        changed.validate().unwrap();
    }

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            // Concurrent tests can observe the same system-clock timestamp.
            // A process-local sequence keeps their exclusive roots distinct.
            let sequence = TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "gitturtle-preferences-{}-{nonce}-{sequence}",
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
    fn commit_message_preserves_user_formatting_and_only_adds_the_separator() {
        for (title, description, expected) in [
            ("Fix preview", "", "Fix preview"),
            ("  Keep title spacing  ", "", "  Keep title spacing  "),
            (
                "Fix preview",
                "Why this matters.\n\n- Preserve indentation\n  and spacing.  \n# Keep comments\n",
                "Fix preview\n\nWhy this matters.\n\n- Preserve indentation\n  and spacing.  \n# Keep comments\n",
            ),
            (
                "Subject",
                "\nLeading blank line\n",
                "Subject\n\n\nLeading blank line\n",
            ),
            ("Subject", "   ", "Subject\n\n   "),
        ] {
            assert_eq!(
                CommitDraft {
                    title: title.into(),
                    description: description.into(),
                }
                .message(),
                expected
            );
        }
    }

    #[test]
    fn drafts_survive_restart_settings_and_recents_with_separate_worktrees() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let main = fixture.0.join("main");
        let linked = fixture.0.join("linked");
        fs::create_dir(&main).unwrap();
        fs::create_dir(&linked).unwrap();
        let main = main.canonicalize().unwrap();
        let linked = linked.canonicalize().unwrap();
        // A version-two store is read without migration writes and gains a
        // drafts section only after an explicit save.
        let old = br#"{"version":2,"settings":{"theme":"daylight"}}"#;
        fs::write(&path, old).unwrap();
        assert!(
            Preferences::load_from(&path)
                .unwrap()
                .commit_drafts
                .is_empty()
        );
        assert_eq!(fs::read(&path).unwrap(), old);
        let first = CommitDraft {
            title: "Work on main".into(),
            description: "Details\n\n  * Keep whitespace.\n".into(),
        };
        let second = CommitDraft {
            title: "Independent linked work".into(),
            description: "Different branch, same common Git directory.".into(),
        };
        let drafts = HashMap::from([(main.clone(), first), (linked.clone(), second)]);
        Preferences::save_commit_drafts_at(&drafts, &path).unwrap();
        let edited = AppSettings {
            density: Density::Compact,
            interface_text_size: 16,
            code_text_size: 19,
            graph_spacing: 28,
            navigation_width: 275.,
            inspector_width: 410.,
            ..Default::default()
        };
        Preferences::save_settings_at(&edited, &path).unwrap();
        let mut stale = Preferences::default();
        stale.remember_at(&main, &path).unwrap();
        let restarted = Preferences::load_from(&path).unwrap();
        assert_eq!(restarted.commit_drafts, drafts);
        assert_eq!(restarted.settings, edited);
        assert_eq!(
            restarted.recent_repositories.as_slice(),
            std::slice::from_ref(&main)
        );

        Preferences::save_commit_drafts_at(
            &HashMap::from([(main.clone(), CommitDraft::default())]),
            &path,
        )
        .unwrap();
        let cleared = Preferences::load_from(&path).unwrap();
        assert!(!cleared.commit_drafts.contains_key(&main));
        assert_eq!(cleared.commit_drafts.get(&linked), drafts.get(&linked));
        assert_eq!(cleared.settings, edited);
        assert_eq!(fs::read_dir(&main).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&linked).unwrap().count(), 0);
    }

    #[test]
    fn invalid_draft_store_or_path_cannot_destroy_saved_text() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let draft = CommitDraft {
            title: "Retain after failure".into(),
            description: "Unsaved work".into(),
        };
        let valid = HashMap::from([(fixture.0.join("repository"), draft.clone())]);
        Preferences::save_commit_drafts_at(&valid, &path).unwrap();
        let saved = fs::read(&path).unwrap();
        let invalid = HashMap::from([(PathBuf::from("relative/worktree"), draft)]);
        assert!(Preferences::save_commit_drafts_at(&invalid, &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        for original in [br#"{"version":99}"#.as_slice(), b"broken"] {
            fs::write(&path, original).unwrap();
            assert!(Preferences::save_commit_drafts_at(&valid, &path).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
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
        assert_eq!(encoded["version"], 3);
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
            commit_drafts: HashMap::from([(
                repository.clone(),
                CommitDraft {
                    title: "Byte-safe worktree".into(),
                    description: String::new(),
                },
            )]),
            ..Default::default()
        }
        .save_to(&settings)
        .unwrap();
        let loaded = Preferences::load_from(&settings).unwrap();
        assert_eq!(loaded.recent_repositories, vec![repository]);
        assert_eq!(
            loaded.commit_drafts.values().next().unwrap().title,
            "Byte-safe worktree"
        );
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
