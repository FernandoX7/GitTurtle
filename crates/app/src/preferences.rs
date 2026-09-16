//! User preferences live outside inspected repositories. Loading is read-only;
//! Writes merge the latest stored recents/settings before atomic replacement.
//! A single application executor serializes these operations off the UI thread.

use crate::{
    appearance::{Density, ThemeChoice},
    columns::ColumnSettings,
    project_library::{ProjectGroup, ProjectLibrary, ProjectNode},
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
/// Custom project names outlive the ten recent entries, so they are bounded
/// separately. Reaching the limit refuses a new name instead of evicting one.
const MAX_PROJECT_NAMES: usize = 128;
pub const MAX_PROJECT_NAME_BYTES: usize = 128;
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
    /// Show the project list on the far left of the window.
    pub project_pane: bool,
    pub project_pane_width: f32,
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
            project_pane: false,
            project_pane_width: 240.,
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
        self.project_pane_width = if self.project_pane_width.is_finite() {
            self.project_pane_width.clamp(180., 360.)
        } else {
            240.
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
    /// Display names chosen by the user, keyed by canonical worktree root.
    /// These rename a project in the client only; no folder is ever moved.
    pub project_names: HashMap<PathBuf, String>,
    /// Known projects and the user's groups for the project list pane.
    pub project_library: ProjectLibrary,
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
    #[serde(default)]
    project_names: Vec<StoredProjectName>,
    #[serde(default)]
    project_library: Vec<StoredNode>,
}

/// The saved project list. An externally tagged enum keeps each line of the
/// settings file readable while the byte-safe path form stays available.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredNode {
    Group(StoredGroup),
    Project { path: StoredPath },
}

#[derive(Serialize, Deserialize)]
struct StoredGroup {
    id: u32,
    name: String,
    #[serde(default)]
    collapsed: bool,
    #[serde(default)]
    nodes: Vec<StoredNode>,
}

impl StoredNode {
    fn from_node(node: &ProjectNode) -> Self {
        match node {
            ProjectNode::Group(group) => Self::Group(StoredGroup {
                id: group.id,
                name: group.name.clone(),
                collapsed: group.collapsed,
                nodes: group.nodes.iter().map(Self::from_node).collect(),
            }),
            ProjectNode::Project(path) => Self::Project {
                path: StoredPath::from_path(path),
            },
        }
    }

    fn into_node(self) -> ProjectNode {
        match self {
            Self::Group(group) => ProjectNode::Group(ProjectGroup {
                id: group.id,
                name: group.name,
                collapsed: group.collapsed,
                nodes: group.nodes.into_iter().map(Self::into_node).collect(),
            }),
            Self::Project { path } => ProjectNode::Project(path.into_path()),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredProjectName {
    path: StoredPath,
    name: String,
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

    /// Save or clear one project's display name. `None` restores the folder
    /// name. This runs on the serialized preference executor beside recents,
    /// and merges into the latest stored file rather than a UI snapshot.
    pub fn save_project_name(path: &Path, name: Option<&str>) -> Result<Self> {
        Self::save_project_name_at(path, name, &settings_path()?)
    }

    fn save_project_name_at(path: &Path, name: Option<&str>, settings: &Path) -> Result<Self> {
        ensure!(
            path.is_absolute(),
            "A project path must be absolute to carry a name"
        );
        let mut next = Self::load_for_write(settings)?;
        match name {
            Some(name) => {
                let name = name.trim();
                validate_project_name(name).map_err(anyhow::Error::msg)?;
                ensure!(
                    next.project_names.contains_key(path)
                        || next.project_names.len() < MAX_PROJECT_NAMES,
                    "Up to {MAX_PROJECT_NAMES} projects can carry a custom name. Restore a folder name before adding another."
                );
                next.project_names.insert(path.to_owned(), name.to_owned());
            }
            None => {
                next.project_names.remove(path);
            }
        }
        next.save_to(settings)?;
        Ok(next)
    }

    /// Replace the saved project list. The caller edits a loaded copy, and
    /// this writer validates the result before it reaches the settings file.
    pub fn save_project_library(library: &ProjectLibrary) -> Result<Self> {
        Self::save_project_library_at(library, &settings_path()?)
    }

    fn save_project_library_at(library: &ProjectLibrary, settings: &Path) -> Result<Self> {
        library.validate().map_err(anyhow::Error::msg)?;
        let mut next = Self::load_for_write(settings)?;
        next.project_library = library.clone();
        next.save_to(settings)?;
        Ok(next)
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
        let bytes = read_store(path, MAX_SETTINGS_BYTES)?;
        let stored: StoredPreferences = serde_json::from_slice(&bytes)?;
        ensure!(
            matches!(stored.version, 1..=5),
            "Unsupported settings version"
        );
        let mut seen = HashSet::new();
        let recent_repositories: Vec<PathBuf> = stored
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
        // Unlike recents, names are user-authored data. Refuse a malformed or
        // oversized section instead of silently dropping entries when an
        // unrelated settings, recent-repository, or draft save rewrites it.
        ensure!(
            stored.project_names.len() <= MAX_PROJECT_NAMES,
            "Too many saved project names"
        );
        let mut project_names = HashMap::new();
        for stored in stored.project_names {
            let path = stored.path.into_path();
            ensure!(path.is_absolute(), "Saved project path must be absolute");
            let name = stored.name.trim();
            validate_project_name(name)
                .map_err(anyhow::Error::msg)
                .context("Invalid saved project name")?;
            ensure!(
                project_names.insert(path, name.to_owned()).is_none(),
                "Duplicate saved project name path"
            );
        }
        // Groups are user-authored too. Refuse a malformed section rather than
        // letting an unrelated write replace the project list with an empty one.
        let mut project_library = ProjectLibrary {
            nodes: stored
                .project_library
                .into_iter()
                .map(StoredNode::into_node)
                .collect(),
        };
        project_library
            .validate()
            .map_err(anyhow::Error::msg)
            .context("Invalid saved project list")?;
        // A store written before the project list starts from the recents, so
        // the pane is useful at once. An emptied list stays empty afterwards.
        if stored.version < 5 {
            for path in &recent_repositories {
                project_library.remember(path);
            }
        }
        Ok(Self {
            recent_repositories,
            settings,
            project_library,
            commit_drafts: stored
                .commit_drafts
                .into_iter()
                .filter_map(|stored| {
                    let path = stored.worktree.into_path();
                    (path.is_absolute() && !stored.draft.is_empty()).then_some((path, stored.draft))
                })
                .collect(),
            project_names,
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
        let mut seen = HashSet::from([path.clone()]);
        recent_repositories.extend(
            current
                .recent_repositories
                .iter()
                .filter(|path| path.is_absolute() && seen.insert((*path).clone()))
                .take(MAX_RECENT - 1)
                .cloned(),
        );
        let mut project_library = current.project_library;
        // Opening a project is how the pane learns about it. The recent list
        // is bounded at ten; the pane keeps the rest of the user's projects.
        project_library.remember(&path);
        let next = Self {
            recent_repositories,
            settings: current.settings,
            commit_drafts: current.commit_drafts,
            project_names: current.project_names,
            project_library,
        };
        next.save_to(settings)?;
        *self = next;
        Ok(())
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        let stored = StoredPreferences {
            version: 5,
            recent_repositories: self
                .recent_repositories
                .iter()
                .map(|path| StoredPath::from_path(path))
                .collect(),
            settings: self.settings.clone(),
            project_names: {
                let mut entries: Vec<_> = self.project_names.iter().collect();
                entries.sort_by_key(|(path, _)| *path);
                entries
                    .into_iter()
                    .map(|(path, name)| StoredProjectName {
                        path: StoredPath::from_path(path),
                        name: name.clone(),
                    })
                    .collect()
            },
            project_library: self
                .project_library
                .nodes
                .iter()
                .map(StoredNode::from_node)
                .collect(),
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

/// The name a project shows when the user has not chosen one. Filenames can
/// hold arbitrary bytes, so fall back to the whole path rather than dropping it.
pub fn directory_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// A display name is presentation only: it needs to fit on one line and stay
/// distinguishable from an empty field. It is never used as a filesystem path.
pub fn validate_project_name(name: &str) -> std::result::Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name for this project, or restore its folder name.".into());
    }
    if name.len() > MAX_PROJECT_NAME_BYTES {
        return Err(format!(
            "Use a project name of at most {MAX_PROJECT_NAME_BYTES} bytes."
        ));
    }
    if name
        .chars()
        .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
    {
        return Err("Use a project name on a single line, without control characters.".into());
    }
    Ok(())
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

// Full-app tests run real background and quit-time preference writes. Each test
// thread owns an isolated directory, inherited by its serial executors, so
// concurrent fixtures cannot overwrite each other's read/merge/write stores.
#[cfg(test)]
thread_local! {
    static TEST_SETTINGS_DIRECTORY: std::cell::RefCell<Option<std::sync::Arc<tempfile::TempDir>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn test_settings_directory() -> std::sync::Arc<tempfile::TempDir> {
    TEST_SETTINGS_DIRECTORY.with(|directory| {
        directory
            .borrow_mut()
            .get_or_insert_with(|| {
                std::sync::Arc::new(
                    tempfile::Builder::new()
                        .prefix("gitturtle-app-tests-")
                        .tempdir()
                        .expect("create isolated test preferences"),
                )
            })
            .clone()
    })
}

#[cfg(test)]
pub(super) fn inherit_test_settings_directory(directory: std::sync::Arc<tempfile::TempDir>) {
    TEST_SETTINGS_DIRECTORY.with(|current| *current.borrow_mut() = Some(directory));
}

#[cfg(test)]
pub(super) fn settings_path() -> Result<PathBuf> {
    Ok(test_settings_directory().path().join("preferences.json"))
}

struct PendingFile(PathBuf);

impl Drop for PendingFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Open app data once, reject links and special files, and bound the bytes from
/// that descriptor. A path metadata check alone cannot constrain a later open
/// or a file that grows while it is being read. Nonblocking open also prevents a
/// FIFO from stalling startup or the serialized persistence executor.
pub(super) fn read_store(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(not(unix))]
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(std::io::Error::other(
            "Saved application data must be a regular file",
        ));
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(std::io::Error::other(
            "Saved application data is not a regular file or exceeds its size limit",
        ));
    }
    read_store_contents(file, limit)
}

fn read_store_contents(reader: impl Read, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other(
            "Saved application data exceeds its size limit",
        ));
    }
    Ok(bytes)
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
    fn store_reads_bound_the_descriptor_even_after_observed_file_growth() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("store.json");
        fs::write(&path, b"{}").unwrap();
        let file = File::open(&path).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 2);
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[b' '; 4096])
            .unwrap();
        assert!(read_store_contents(file, 32).is_err());
        assert!(read_store(&path, 32).is_err());
        fs::write(&path, [b'x'; 32]).unwrap();
        assert_eq!(read_store(&path, 32).unwrap(), [b'x'; 32]);
        assert!(read_store(fixture.path(), 32).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn store_reads_refuse_links_and_fifos_and_saves_preserve_them() {
        use std::{
            ffi::CString,
            os::unix::{ffi::OsStrExt, fs::symlink},
        };

        let fixture = tempfile::tempdir().unwrap();
        let target = fixture.path().join("original.json");
        let original = br#"{"version":3,"settings":{"theme":"daylight"}}"#;
        fs::write(&target, original).unwrap();
        let link = fixture.path().join("linked.json");
        symlink(&target, &link).unwrap();
        let dangling = fixture.path().join("dangling.json");
        symlink(fixture.path().join("absent.json"), &dangling).unwrap();
        let fifo = fixture.path().join("fifo.json");
        let fifo_name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: the C string is valid for this call and the disposable path
        // does not exist. No process writes to this FIFO.
        assert_eq!(unsafe { libc::mkfifo(fifo_name.as_ptr(), 0o600) }, 0);
        let start = std::time::Instant::now();
        for path in [&link, &dangling, &fifo] {
            assert!(read_store(path, MAX_SETTINGS_BYTES).is_err());
            assert!(Preferences::save_settings_at(&AppSettings::default(), path).is_err());
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::symlink_metadata(&dangling)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(target).unwrap(), original);
    }

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
    fn project_names_survive_restart_and_restore_the_folder_name_when_cleared() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let project = fixture.0.join("turtle-client");
        let other = fixture.0.join("turtle-core");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&other).unwrap();
        let project = project.canonicalize().unwrap();
        let other = other.canonicalize().unwrap();

        // A version-three store predates project names and reads without one.
        let old = br#"{"version":3,"settings":{"theme":"daylight"}}"#;
        fs::write(&path, old).unwrap();
        assert!(
            Preferences::load_from(&path)
                .unwrap()
                .project_names
                .is_empty()
        );
        assert_eq!(fs::read(&path).unwrap(), old);

        let saved = Preferences::save_project_name_at(&project, Some("  Client · 客户端  "), &path)
            .unwrap();
        assert_eq!(
            saved.project_names.get(&project).map(String::as_str),
            Some("Client · 客户端")
        );
        Preferences::save_project_name_at(&other, Some("Core"), &path).unwrap();

        // Unrelated preference writes merge rather than dropping the names.
        Preferences::save_settings_at(
            &AppSettings {
                density: Density::Compact,
                ..Default::default()
            },
            &path,
        )
        .unwrap();
        let mut stale = Preferences::default();
        stale.remember_at(&project, &path).unwrap();
        let drafts = HashMap::from([(
            project.clone(),
            CommitDraft {
                title: "Keep this draft while renaming".into(),
                description: "Uncommitted work".into(),
            },
        )]);
        Preferences::save_commit_drafts_at(&drafts, &path).unwrap();
        let restarted = Preferences::load_from(&path).unwrap();
        assert_eq!(
            restarted.project_names.get(&project).map(String::as_str),
            Some("Client · 客户端")
        );
        assert_eq!(
            restarted.project_names.get(&other).map(String::as_str),
            Some("Core")
        );
        assert_eq!(restarted.settings.density, Density::Compact);
        assert_eq!(restarted.commit_drafts, drafts);

        // Clearing one name leaves the folder and every other name intact.
        Preferences::save_project_name_at(&project, None, &path).unwrap();
        let cleared = Preferences::load_from(&path).unwrap();
        assert!(!cleared.project_names.contains_key(&project));
        assert_eq!(
            cleared.project_names.get(&other).map(String::as_str),
            Some("Core")
        );
        assert_eq!(cleared.commit_drafts, drafts);
        assert_eq!(directory_name(&project), "turtle-client");
        assert_eq!(fs::read_dir(&project).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&other).unwrap().count(), 0);
    }

    #[test]
    fn the_project_list_survives_restart_and_unrelated_preference_writes() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let project = fixture.0.join("turtle-client");
        let other = fixture.0.join("turtle-core");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&other).unwrap();
        let project = project.canonicalize().unwrap();
        let other = other.canonicalize().unwrap();

        // A version-four store predates the project list and reads without one.
        let old = br#"{"version":4,"settings":{"theme":"daylight"}}"#;
        fs::write(&path, old).unwrap();
        assert!(
            Preferences::load_from(&path)
                .unwrap()
                .project_library
                .nodes
                .is_empty()
        );
        assert_eq!(fs::read(&path).unwrap(), old);

        // Opening a project is what teaches the list about it.
        let mut opened = Preferences::default();
        opened.remember_at(&project, &path).unwrap();
        opened.remember_at(&other, &path).unwrap();
        assert!(opened.project_library.contains_project(&project));
        assert!(opened.project_library.contains_project(&other));

        let mut library = opened.project_library.clone();
        let work = library.create_group(None, "Work").unwrap();
        let clients = library.create_group(Some(work), "Clients").unwrap();
        library.move_project(&project, Some(clients)).unwrap();
        library.set_collapsed(work, true);
        let saved = Preferences::save_project_library_at(&library, &path).unwrap();
        assert_eq!(saved.project_library, library);

        // An unrelated write merges the list rather than replacing it.
        Preferences::save_settings_at(
            &AppSettings {
                density: Density::Compact,
                project_pane: true,
                ..Default::default()
            },
            &path,
        )
        .unwrap();
        Preferences::save_project_name_at(&project, Some("Client"), &path).unwrap();
        let restarted = Preferences::load_from(&path).unwrap();
        assert_eq!(restarted.project_library, library);
        assert!(restarted.settings.project_pane);
        assert_eq!(
            restarted
                .project_library
                .group(work)
                .map(|group| group.collapsed),
            Some(true)
        );

        // Reopening a project already in a group leaves it where the user put it.
        let mut reopened = Preferences::default();
        reopened.remember_at(&project, &path).unwrap();
        assert_eq!(reopened.project_library, library);
    }

    #[test]
    fn an_older_store_seeds_the_project_list_from_its_recent_projects_once() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let first = fixture.0.join("first");
        let second = fixture.0.join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let first = first.canonicalize().unwrap();
        let second = second.canonicalize().unwrap();
        let older = serde_json::json!({
            "version": 4,
            "recent_repositories": [first.to_str().unwrap(), second.to_str().unwrap()],
        });
        fs::write(&path, serde_json::to_vec(&older).unwrap()).unwrap();

        let seeded = Preferences::load_from(&path).unwrap();
        assert_eq!(
            seeded.project_library.rows(),
            vec![
                crate::project_library::LibraryRow::Project {
                    path: first.clone(),
                    depth: 0,
                    parent: None,
                },
                crate::project_library::LibraryRow::Project {
                    path: second.clone(),
                    depth: 0,
                    parent: None,
                },
            ]
        );
        // Reading does not rewrite the file; the next ordinary write does.
        assert_eq!(
            fs::read(&path).unwrap(),
            serde_json::to_vec(&older).unwrap()
        );
        Preferences::save_settings_at(&AppSettings::default(), &path).unwrap();

        // A version-five store keeps an emptied list instead of seeding again.
        Preferences::save_project_library_at(&ProjectLibrary::default(), &path).unwrap();
        let reloaded = Preferences::load_from(&path).unwrap();
        assert!(reloaded.project_library.rows().is_empty());
        assert_eq!(reloaded.recent_repositories, vec![first, second]);
    }

    #[test]
    fn an_unusable_saved_project_list_is_refused_rather_than_dropped() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let project = fixture.0.join("turtle-client");
        fs::create_dir(&project).unwrap();
        let project = project.canonicalize().unwrap();
        let stored = serde_json::json!({
            "version": 5,
            "project_library": [
                {"group": {"id": 1, "name": "Work", "nodes": [
                    {"project": {"path": project.to_str().unwrap()}}
                ]}},
                {"group": {"id": 1, "name": "Also one", "nodes": []}}
            ]
        });
        fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
        assert!(Preferences::load_from(&path).is_err());
        // Every writer reads before it writes, so none of them rewrites the file.
        assert!(Preferences::save_settings_at(&AppSettings::default(), &path).is_err());
        assert!(Preferences::save_project_name_at(&project, Some("Client"), &path).is_err());
        assert!(Preferences::save_project_library_at(&ProjectLibrary::default(), &path).is_err());
        assert!(Preferences::default().remember_at(&project, &path).is_err());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
            stored
        );
    }

    #[test]
    fn project_names_reject_unusable_text_and_bound_their_count() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let project = fixture.0.join("repository");
        for name in [
            "",
            "   ",
            "two\nlines",
            "two\u{2028}lines",
            "two\u{2029}paragraphs",
            "bell\u{7}",
            &"x".repeat(129),
            &"客".repeat(43),
        ] {
            assert!(validate_project_name(name).is_err(), "{name:?}");
            assert!(Preferences::save_project_name_at(&project, Some(name), &path).is_err());
        }
        for name in ["Client", "客户端", &"x".repeat(128), &"客".repeat(42)] {
            validate_project_name(name).unwrap();
        }
        assert!(
            Preferences::save_project_name_at(Path::new("relative/path"), Some("Name"), &path)
                .is_err()
        );
        assert!(!path.exists(), "a refused name must not create a store");

        for index in 0..MAX_PROJECT_NAMES {
            Preferences::save_project_name_at(
                &fixture.0.join(format!("repo-{index}")),
                Some(&format!("Project {index}")),
                &path,
            )
            .unwrap();
        }
        let saved = fs::read(&path).unwrap();
        assert!(Preferences::save_project_name_at(&project, Some("One too many"), &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        // Renaming an already named project stays possible at the limit.
        Preferences::save_project_name_at(&fixture.0.join("repo-0"), Some("Renamed"), &path)
            .unwrap();
        let loaded = Preferences::load_from(&path).unwrap();
        assert_eq!(loaded.project_names.len(), MAX_PROJECT_NAMES);
        assert_eq!(
            loaded
                .project_names
                .get(&fixture.0.join("repo-0"))
                .map(String::as_str),
            Some("Renamed")
        );
    }

    #[test]
    fn invalid_saved_project_names_are_preserved_by_every_preference_writer() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let project = fixture.0.join("repository");
        fs::create_dir(&project).unwrap();
        let draft = CommitDraft {
            title: "Preserve this draft".into(),
            description: "and the original names".into(),
        };
        let valid_entry = serde_json::json!({"path": project, "name": "Client"});
        let excessive: Vec<_> = (0..=MAX_PROJECT_NAMES)
            .map(|index| {
                serde_json::json!({
                    "path": fixture.0.join(format!("repository-{index}")),
                    "name": format!("Project {index}"),
                })
            })
            .collect();
        for entries in [
            vec![serde_json::json!({"path": project, "name": "two\u{2028}lines"})],
            vec![serde_json::json!({"path": "relative/path", "name": "Client"})],
            vec![valid_entry.clone(), valid_entry],
            excessive,
        ] {
            let original = serde_json::to_vec(&serde_json::json!({
                "version": 4,
                "project_names": entries,
                "commit_drafts": [{"worktree": project, "draft": draft}],
            }))
            .unwrap();
            fs::write(&path, &original).unwrap();
            assert!(Preferences::load_from(&path).is_err());
            assert!(Preferences::save_settings_at(&AppSettings::default(), &path).is_err());
            assert!(Preferences::default().remember_at(&project, &path).is_err());
            assert!(Preferences::save_commit_drafts_at(&HashMap::new(), &path).is_err());
            assert!(Preferences::save_project_name_at(&project, Some("Renamed"), &path).is_err());
            assert!(Preferences::save_project_name_at(&project, None, &path).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
        }
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
        assert_eq!(encoded["version"], 5);
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
            project_names: HashMap::from([(repository.clone(), "Byte-safe project".into())]),
            ..Default::default()
        }
        .save_to(&settings)
        .unwrap();
        let loaded = Preferences::load_from(&settings).unwrap();
        assert_eq!(loaded.recent_repositories, vec![repository.clone()]);
        assert_eq!(
            loaded.project_names.get(&repository).map(String::as_str),
            Some("Byte-safe project")
        );
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
