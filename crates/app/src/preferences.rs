//! User preferences live outside inspected repositories. Loading is read-only;
//! Writes merge the latest stored recents/settings before atomic replacement.
//! A single application executor serializes these operations off the UI thread.

use crate::{
    appearance::{
        Density, Palette, ThemeChoice,
        custom::{
            CustomTheme, ResolvedTheme, ThemeSelection, TokenKind, built_in_from_key,
            fallback_base, format_hex, parse_hex, validate_theme_name,
        },
    },
    columns::ColumnSettings,
    project_library::{ProjectGroup, ProjectLibrary, ProjectNode},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::IgnoredAny};
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
/// Custom themes are user-authored, so reaching the limit refuses a new theme
/// instead of evicting one, and a stored section above it refuses later writes.
pub const MAX_CUSTOM_THEMES: usize = 32;
/// The store version this build writes. Every earlier version still loads.
const STORE_VERSION: u32 = 6;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// A built-in selection keeps the version-5 bare string (`"nord"`); a custom
    /// theme is stored as `{"custom": id}`.
    pub theme: ThemeSelection,
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
            theme: ThemeSelection::default(),
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
    /// The theme to apply. Without following the system this is the selection's
    /// palette; a custom id missing from `custom_themes` resolves to the default
    /// theme. Following the system, a light appearance selects Braden and a dark
    /// one keeps a dark selection (built-in or custom) and otherwise Midnight.
    pub fn resolved_theme(
        &self,
        appearance: gpui_kit::WindowAppearance,
        custom_themes: &[CustomTheme],
    ) -> ResolvedTheme {
        let chosen = self.theme.resolve(custom_themes);
        if !self.follow_system {
            return chosen;
        }
        match appearance {
            gpui_kit::WindowAppearance::Light | gpui_kit::WindowAppearance::VibrantLight => {
                ResolvedTheme::built_in(ThemeChoice::Daylight)
            }
            _ if chosen.is_light => ResolvedTheme::built_in(ThemeChoice::Midnight),
            _ => chosen,
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
    /// User-authored themes, in saved order. Empty for stores before version 6.
    pub custom_themes: Vec<CustomTheme>,
}

/// `Themes` is the saved section's type. Only a failed load reads the store
/// again with `IgnoredAny` there, to tell a refused `custom_themes` section
/// from a failure elsewhere in the file.
#[derive(Serialize, Deserialize)]
struct StoredPreferences<Themes = StoredCustomThemes> {
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
    #[serde(default)]
    custom_themes: Themes,
}

/// The saved `custom_themes` section. Reading keeps at most one entry past
/// [`MAX_CUSTOM_THEMES`] and skips the rest without parsing them as themes, so
/// an oversized section costs no more memory than a full one and
/// `validate_custom_themes` refuses it with the bound, exactly as it refuses a
/// section one entry over it.
#[derive(Default, Serialize)]
#[serde(transparent)]
struct StoredCustomThemes(Vec<StoredCustomTheme>);

impl<'de> Deserialize<'de> for StoredCustomThemes {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StoredCustomThemes;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a list of custom themes")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<Self::Value, A::Error> {
                let mut themes = Vec::new();
                while themes.len() <= MAX_CUSTOM_THEMES {
                    let Some(theme) = access.next_element()? else {
                        return Ok(StoredCustomThemes(themes));
                    };
                    themes.push(theme);
                }
                while access.next_element::<IgnoredAny>()?.is_some() {}
                Ok(StoredCustomThemes(themes))
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}

/// One saved custom theme. The 21 tokens are lowercase `#rrggbb` strings keyed
/// like the export document, so the section stays readable and hand-repairable.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredCustomTheme {
    id: u32,
    name: String,
    /// `None` when the store names a base this build does not know, such as a
    /// built-in added by a newer release that still writes version 6.
    #[serde(deserialize_with = "known_base")]
    base: Option<ThemeChoice>,
    tokens: StoredTokens,
}

/// `ThemeChoice` reads any unknown string as Midnight, which would make a light
/// theme's reset target dark. Keep the unknown case visible so loading can use
/// the same lightness fallback as import; a non-string base is still refused.
fn known_base<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ThemeChoice>, D::Error> {
    Ok(built_in_from_key(&String::deserialize(deserializer)?))
}

impl StoredCustomTheme {
    fn from_theme(theme: &CustomTheme) -> Self {
        Self {
            id: theme.id,
            name: theme.name.clone(),
            base: Some(theme.base),
            tokens: StoredTokens(theme.palette),
        }
    }

    fn into_theme(self) -> CustomTheme {
        let palette = self.tokens.0;
        CustomTheme {
            id: self.id,
            name: self.name,
            base: self.base.unwrap_or_else(|| fallback_base(palette)),
            palette,
        }
    }
}

/// Every token exactly once, as `#rrggbb`. A repeated, unknown, missing or
/// malformed token fails the whole store rather than being dropped or defaulted.
struct StoredTokens(Palette);

impl Serialize for StoredTokens {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(TokenKind::ALL.len()))?;
        for kind in TokenKind::ALL {
            map.serialize_entry(kind.key(), &format_hex(self.0.get(kind)))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for StoredTokens {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StoredTokens;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an object of #rrggbb theme tokens")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<Self::Value, A::Error> {
                use serde::de::Error;
                let mut palette = ThemeChoice::default().palette();
                let mut seen = HashSet::new();
                while let Some((key, value)) = access.next_entry::<String, String>()? {
                    let kind = TokenKind::from_key(&key)
                        .ok_or_else(|| A::Error::custom(format!("unknown theme token {key:?}")))?;
                    if !seen.insert(kind) {
                        return Err(A::Error::custom(format!("repeated theme token {key:?}")));
                    }
                    let color = value
                        .starts_with('#')
                        .then(|| parse_hex(&value))
                        .flatten()
                        .ok_or_else(|| {
                            A::Error::custom(format!("theme token {key:?} is not #rrggbb"))
                        })?;
                    palette.set(kind, color);
                }
                if let Some(missing) = TokenKind::ALL.into_iter().find(|kind| !seen.contains(kind))
                {
                    return Err(A::Error::custom(format!(
                        "missing theme token {:?}",
                        missing.key()
                    )));
                }
                Ok(StoredTokens(palette))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

/// The bounds a custom-theme section must meet both when it is saved and when it
/// is loaded: at most [`MAX_CUSTOM_THEMES`] entries, unique ids, and names that
/// pass `validate_theme_name` (one line, 1-128 bytes, not a built-in label,
/// unique ignoring case).
pub fn validate_custom_themes(themes: &[CustomTheme]) -> std::result::Result<(), String> {
    if themes.len() > MAX_CUSTOM_THEMES {
        return Err(format!(
            "Up to {MAX_CUSTOM_THEMES} custom themes can be saved. Delete one before adding another."
        ));
    }
    let mut ids = HashSet::new();
    for (index, theme) in themes.iter().enumerate() {
        if !ids.insert(theme.id) {
            return Err(format!("Two custom themes share the id {}.", theme.id));
        }
        validate_theme_name(&theme.name, &themes[..index], None)?;
    }
    Ok(())
}

/// The context of a load refused by the saved `custom_themes` section. Every
/// save rereads the store first, so each one reports it; the section can only
/// be repaired outside the app, so the message names the file and the section.
fn custom_themes_refusal(path: &Path) -> String {
    format!(
        "Invalid saved custom themes. Repair or remove the custom_themes section of {}",
        path.display()
    )
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

    /// Replace the saved custom themes. Run this on the serialized preference
    /// executor: it rereads the disk store and replaces only this section, so
    /// settings, recents, drafts, names and the project list saved since the
    /// caller loaded its copy survive. A section outside the bounds is refused.
    // The theme editor submits this; until it lands, tests are the only callers.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn save_custom_themes(themes: &[CustomTheme]) -> Result<Self> {
        Self::save_custom_themes_at(themes, &settings_path()?)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn save_custom_themes_at(themes: &[CustomTheme], settings: &Path) -> Result<Self> {
        let themes: Vec<CustomTheme> = themes
            .iter()
            .map(|theme| CustomTheme {
                name: theme.name.trim().to_owned(),
                ..theme.clone()
            })
            .collect();
        validate_custom_themes(&themes).map_err(anyhow::Error::msg)?;
        let mut next = Self::load_for_write(settings)?;
        next.custom_themes = themes;
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
        let stored: StoredPreferences = match serde_json::from_slice(&bytes) {
            Ok(stored) => stored,
            // The rest of the store parses, so the custom themes refused it.
            Err(error)
                if serde_json::from_slice::<StoredPreferences<IgnoredAny>>(&bytes).is_ok() =>
            {
                return Err(error).with_context(|| custom_themes_refusal(path));
            }
            Err(error) => return Err(error.into()),
        };
        ensure!(
            (1..=STORE_VERSION).contains(&stored.version),
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
        // Custom themes are user-authored as well: a malformed token, repeated id
        // or name, or more than the bound fails the load so later saves refuse
        // instead of rewriting the file without them, each with the same
        // `custom_themes_refusal`. Stores before version 6 have no section and
        // load with none.
        let custom_themes: Vec<CustomTheme> = stored
            .custom_themes
            .0
            .into_iter()
            .map(|stored| {
                let mut theme = stored.into_theme();
                theme.name = theme.name.trim().to_owned();
                theme
            })
            .collect();
        validate_custom_themes(&custom_themes)
            .map_err(anyhow::Error::msg)
            .with_context(|| custom_themes_refusal(path))?;
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
            custom_themes,
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
            custom_themes: current.custom_themes,
        };
        next.save_to(settings)?;
        *self = next;
        Ok(())
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        let stored: StoredPreferences = StoredPreferences {
            version: STORE_VERSION,
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
            custom_themes: StoredCustomThemes(
                self.custom_themes
                    .iter()
                    .map(StoredCustomTheme::from_theme)
                    .collect(),
            ),
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

fn absolute_environment_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// Where a save dialog starts before the user navigates: their home folder,
/// or the working directory when the environment names none. An exported
/// document is the user's file, so this is only the dialog's starting point.
pub(super) fn home_directory() -> PathBuf {
    #[cfg(unix)]
    let home = absolute_environment_path("HOME");
    #[cfg(not(unix))]
    let home = absolute_environment_path("USERPROFILE");
    home.filter(|home| home.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
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
    write_through_temporary(directory, path, bytes, ".preferences", Some(0o600))
}

/// Write a document the user chose a location for, such as an exported theme,
/// through the same temporary-file-and-rename path as application data. Unlike
/// [`atomic_write`] this never creates the folder — the save dialog returns one
/// that exists — and leaves the platform's default permissions, because the
/// user may share what they exported. A refused destination fails before any
/// bytes are written, and a failure after that removes the temporary file, so
/// no partial or truncated document is left behind.
pub(super) fn write_exported_document(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
        .context("The chosen location has no parent folder")?;
    ensure!(
        directory.is_absolute(),
        "Choose a destination folder with an absolute path"
    );
    write_through_temporary(directory, path, bytes, ".gitturtle-export", None)
        .with_context(|| format!("Could not write to {}", directory.display()))
}

/// Write `bytes` into `directory` under a temporary name and rename it onto
/// `path`, so a reader sees either the previous file or the whole new one.
/// `mode` sets the temporary file's Unix permissions where the data is private.
fn write_through_temporary(
    directory: &Path,
    path: &Path,
    bytes: &[u8],
    prefix: &str,
    mode: Option<u32>,
) -> Result<()> {
    #[cfg(not(unix))]
    let _ = mode;
    // create_new prevents following an existing temporary-file symlink. The
    // final rename occurs on the same filesystem, so readers see a whole file.
    let (pending, mut file) = (0..32)
        .find_map(|_| {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let temporary =
                directory.join(format!("{prefix}.{}.{}.tmp", std::process::id(), sequence));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            if let Some(mode) = mode {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(mode);
            }
            match options.open(&temporary) {
                Ok(file) => Some(Ok((PendingFile(temporary), file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error)),
            }
        })
        .context("Could not reserve a temporary file beside the destination")??;
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
    fn exported_documents_replace_whole_files_and_leave_nothing_behind_on_failure() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("sunset.gitturtle-theme.json");
        write_exported_document(&path, b"first\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first\n");
        // A second export replaces the whole file rather than truncating it.
        write_exported_document(&path, b"second document\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second document\n");
        assert_eq!(fs::read_dir(fixture.path()).unwrap().count(), 1);

        // A relative or parentless destination is refused before any write.
        assert!(write_exported_document(Path::new("sunset.json"), b"x").is_err());
        // A destination folder that does not exist is refused; the export
        // dialog returns an existing one, and nothing is created here.
        let missing = fixture.path().join("absent").join("theme.json");
        assert!(write_exported_document(&missing, b"x").is_err());
        assert!(!missing.parent().unwrap().exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = fixture.path().join("locked");
            fs::create_dir(&locked).unwrap();
            let destination = locked.join("theme.json");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
            let refused = write_exported_document(&destination, b"x").unwrap_err();
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(
                format!("{refused:#}")
                    .contains(&format!("Could not write to {}", locked.display())),
                "an unwritable folder reports the refused write: {refused:#}"
            );
            // Temp-and-rename: no partial or temporary file is left behind.
            assert_eq!(fs::read_dir(&locked).unwrap().count(), 0);
        }
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
        assert_eq!(loaded.theme, ThemeSelection::BuiltIn(ThemeChoice::Nord));
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
        assert_eq!(encoded["version"], 6);
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
            theme: ThemeSelection::BuiltIn(ThemeChoice::Daylight),
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
        assert_eq!(
            loaded.settings.theme,
            ThemeSelection::BuiltIn(ThemeChoice::Midnight)
        );
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

    fn custom_theme(id: u32, name: &str, base: ThemeChoice, accent: u32) -> CustomTheme {
        let mut theme = CustomTheme::from_base(id, name, base);
        theme.palette.set(TokenKind::Accent, accent);
        theme
    }

    /// A light custom theme and a dark one, with accents no built-in uses.
    fn two_custom_themes() -> Vec<CustomTheme> {
        vec![
            custom_theme(1, "Sunrise", ThemeChoice::Daylight, 0x8a2be2),
            custom_theme(2, "Harbor", ThemeChoice::Nord, 0x12ab34),
        ]
    }

    #[test]
    fn a_complete_version_five_store_loads_unchanged_without_custom_themes_or_a_write() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let project = fixture.0.join("turtle");
        let other = fixture.0.join("shell");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&other).unwrap();
        let project = project.canonicalize().unwrap();
        let other = other.canonicalize().unwrap();
        let (project_text, other_text) = (project.to_str().unwrap(), other.to_str().unwrap());
        let original = serde_json::to_vec_pretty(&serde_json::json!({
            "version": 5,
            "recent_repositories": [project_text, other_text],
            "settings": {"theme": "nord", "density": "compact", "code_text_size": 15},
            "commit_drafts": [
                {"worktree": project_text, "draft": {"title": "Fix", "description": "Why\n"}}
            ],
            "project_names": [{"path": other_text, "name": "Shell tools"}],
            "project_library": [
                {"group": {"id": 3, "name": "Work", "collapsed": true, "nodes": [
                    {"project": {"path": project_text}}
                ]}},
                {"project": {"path": other_text}}
            ]
        }))
        .unwrap();
        fs::write(&path, &original).unwrap();

        let loaded = Preferences::load_from(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original, "loading never writes");
        assert_eq!(
            loaded.recent_repositories,
            vec![project.clone(), other.clone()]
        );
        assert_eq!(
            loaded.settings,
            AppSettings {
                theme: ThemeSelection::BuiltIn(ThemeChoice::Nord),
                density: Density::Compact,
                code_text_size: 15,
                ..AppSettings::default()
            }
        );
        assert_eq!(
            loaded.commit_drafts,
            HashMap::from([(
                project.clone(),
                CommitDraft {
                    title: "Fix".into(),
                    description: "Why\n".into(),
                }
            )])
        );
        assert_eq!(
            loaded.project_names,
            HashMap::from([(other.clone(), "Shell tools".to_owned())])
        );
        assert_eq!(
            loaded.project_library.nodes,
            vec![
                ProjectNode::Group(ProjectGroup {
                    id: 3,
                    name: "Work".into(),
                    collapsed: true,
                    nodes: vec![ProjectNode::Project(project.clone())],
                }),
                ProjectNode::Project(other.clone()),
            ]
        );
        assert!(loaded.custom_themes.is_empty());

        // The first explicit save migrates to version 6 and keeps every section.
        let saved = Preferences::save_settings_at(&loaded.settings, &path).unwrap();
        let encoded: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(encoded["version"], 6);
        assert_eq!(encoded["settings"]["theme"], "nord");
        assert_eq!(encoded["custom_themes"], serde_json::json!([]));
        let reloaded = Preferences::load_from(&path).unwrap();
        for preferences in [&saved, &reloaded] {
            assert_eq!(preferences.recent_repositories, loaded.recent_repositories);
            assert_eq!(preferences.settings, loaded.settings);
            assert_eq!(preferences.commit_drafts, loaded.commit_drafts);
            assert_eq!(preferences.project_names, loaded.project_names);
            assert_eq!(
                preferences.project_library.nodes,
                loaded.project_library.nodes
            );
            assert!(preferences.custom_themes.is_empty());
        }
    }

    #[test]
    fn custom_themes_and_a_custom_selection_round_trip_byte_for_byte_at_version_six() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let themes = two_custom_themes();
        Preferences::save_custom_themes_at(&themes, &path).unwrap();
        let settings = AppSettings {
            theme: ThemeSelection::Custom(2),
            ..AppSettings::default()
        };
        Preferences::save_settings_at(&settings, &path).unwrap();
        let written = fs::read(&path).unwrap();
        let text = std::str::from_utf8(&written).unwrap();
        assert!(text.contains("\"version\": 6,"), "{text}");
        assert!(
            text.contains("\"theme\": {\n      \"custom\": 2\n    },"),
            "{text}"
        );
        assert!(text.contains("\"accent\": \"#12ab34\""), "{text}");

        let loaded = Preferences::load_from(&path).unwrap();
        assert_eq!(loaded.custom_themes, themes);
        assert_eq!(loaded.settings.theme, ThemeSelection::Custom(2));
        for appearance in [
            gpui_kit::WindowAppearance::Dark,
            gpui_kit::WindowAppearance::Light,
        ] {
            let resolved = loaded
                .settings
                .resolved_theme(appearance, &loaded.custom_themes);
            assert_eq!(resolved, ResolvedTheme::custom(&themes[1]));
            assert_eq!(resolved.palette.get(TokenKind::Accent), 0x12ab34);
        }
        assert_eq!(fs::read(&path).unwrap(), written, "loading never writes");

        // Unrelated saves carry the section and the selection through unchanged.
        Preferences::save_settings_at(&loaded.settings, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), written);
        Preferences::save_commit_drafts_at(&HashMap::new(), &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), written);
        Preferences::save_custom_themes_at(&loaded.custom_themes, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), written);
    }

    #[test]
    fn unusable_custom_themes_refuse_every_later_write_and_keep_the_original_bytes() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let project = fixture.0.join("turtle");
        fs::create_dir(&project).unwrap();
        let project = project.canonicalize().unwrap();
        Preferences::save_custom_themes_at(&two_custom_themes(), &path).unwrap();
        let valid: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let themes = |edit: &dyn Fn(&mut Vec<serde_json::Value>)| {
            let mut store = valid.clone();
            edit(store["custom_themes"].as_array_mut().unwrap());
            serde_json::to_vec_pretty(&store).unwrap()
        };
        let cases = [
            ("duplicate id", themes(&|themes| themes[1]["id"] = 1.into())),
            (
                "duplicate name ignoring case",
                themes(&|themes| themes[1]["name"] = "sUNRISE".into()),
            ),
            (
                "33 themes",
                themes(&|themes| {
                    let template = themes[0].clone();
                    themes.clear();
                    for id in 1..=33 {
                        let mut theme = template.clone();
                        theme["id"] = id.into();
                        theme["name"] = format!("Theme {id}").into();
                        themes.push(theme);
                    }
                }),
            ),
            (
                "invalid hex token",
                themes(&|themes| themes[0]["tokens"]["accent"] = "#12ab3g".into()),
            ),
            (
                "token without #",
                themes(&|themes| themes[0]["tokens"]["accent"] = "12ab34".into()),
            ),
            (
                "missing token",
                themes(&|themes| {
                    themes[0]["tokens"].as_object_mut().unwrap().remove("hunk");
                }),
            ),
            (
                "built-in name",
                themes(&|themes| themes[0]["name"] = "Nord".into()),
            ),
            (
                "multi-line name",
                themes(&|themes| themes[0]["name"] = "Sun\nrise".into()),
            ),
            (
                "non-string base",
                themes(&|themes| themes[0]["base"] = 7.into()),
            ),
            (
                "unknown theme field",
                themes(&|themes| themes[0]["favorite"] = true.into()),
            ),
        ];
        // The 33-theme store is otherwise valid: only the bound refuses it.
        let bounded: Vec<CustomTheme> = (1..=32)
            .map(|id| custom_theme(id, &format!("Theme {id}"), ThemeChoice::Nord, 0x123456))
            .collect();
        validate_custom_themes(&bounded).unwrap();
        // Every refusal names the file and the section to repair, and a save
        // adds only its own context in front of it.
        let refusal = format!(
            "Invalid saved custom themes. Repair or remove the custom_themes section of {}: ",
            path.display()
        );
        let refused = |case: &str| {
            let error = format!(
                "{:#}",
                Preferences::load_from(&path).map(drop).expect_err(case)
            );
            assert!(error.starts_with(&refusal), "{case}: {error}");
            let saves = [
                Preferences::save_settings_at(&AppSettings::default(), &path).map(drop),
                Preferences::save_custom_themes_at(&two_custom_themes(), &path).map(drop),
                Preferences::save_commit_drafts_at(&HashMap::new(), &path),
                Preferences::save_project_name_at(&project, Some("Turtle"), &path).map(drop),
                Preferences::save_project_library_at(&ProjectLibrary::default(), &path).map(drop),
                Preferences::default().remember_at(&project, &path),
            ];
            for save in saves {
                let error = format!("{:#}", save.expect_err(case));
                assert!(
                    error.starts_with(&format!(
                        "Read current preferences before saving: {refusal}"
                    )),
                    "{case}: {error}"
                );
            }
        };
        for (case, original) in cases {
            fs::write(&path, &original).unwrap();
            refused(case);
            assert_eq!(fs::read(&path).unwrap(), original, "{case}");
        }

        // A repeated token key cannot be expressed through a JSON value, so write it literally.
        fs::write(&path, serde_json::to_vec_pretty(&valid).unwrap()).unwrap();
        let text = String::from_utf8(fs::read(&path).unwrap()).unwrap();
        let original = text
            .replacen(
                "\"accent\": \"#8a2be2\",",
                "\"accent\": \"#8a2be2\",\n        \"accent\": \"#000000\",",
                1,
            )
            .into_bytes();
        assert_ne!(original, text.as_bytes());
        fs::write(&path, &original).unwrap();
        refused("repeated token");
        assert_eq!(fs::read(&path).unwrap(), original);

        // A failure elsewhere in the file keeps its own message.
        let mut elsewhere = valid.clone();
        elsewhere["project_names"] = 7.into();
        fs::write(&path, serde_json::to_vec_pretty(&elsewhere).unwrap()).unwrap();
        let error = format!("{:#}", Preferences::load_from(&path).map(drop).unwrap_err());
        assert!(!error.contains("custom_themes"), "{error}");
    }

    #[test]
    fn an_oversized_custom_themes_section_is_read_one_entry_past_the_bound_and_refused_by_it() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        Preferences::save_custom_themes_at(&two_custom_themes(), &path).unwrap();
        let valid: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        // One theme over the bound, then `extra` entries that are not themes at
        // all: nothing past the bound is read as a theme, so they cannot change
        // the refusal.
        let store = |extra: usize| {
            let mut store = valid.clone();
            let themes = store["custom_themes"].as_array_mut().unwrap();
            let template = themes[0].clone();
            themes.clear();
            for id in 1..=MAX_CUSTOM_THEMES as u32 + 1 {
                let mut theme = template.clone();
                theme["id"] = id.into();
                theme["name"] = format!("Theme {id}").into();
                themes.push(theme);
            }
            themes.extend((0..extra).map(|_| serde_json::Value::from(0)));
            serde_json::to_vec(&store).unwrap()
        };
        let refusal = |original: &[u8]| {
            fs::write(&path, original).unwrap();
            let error = format!("{:#}", Preferences::load_from(&path).map(drop).unwrap_err());
            assert!(Preferences::save_settings_at(&AppSettings::default(), &path).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
            error
        };
        let over_bound = refusal(&store(0));
        assert!(
            over_bound.ends_with(
                "Up to 32 custom themes can be saved. Delete one before adding another."
            ),
            "{over_bound}"
        );
        let oversized = store(100_000);
        assert_eq!(refusal(&oversized), over_bound);
        let stored: StoredPreferences = serde_json::from_slice(&oversized).unwrap();
        assert_eq!(stored.custom_themes.0.len(), MAX_CUSTOM_THEMES + 1);
    }

    #[test]
    fn saving_custom_themes_refuses_an_unbounded_or_ambiguous_section_without_writing() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        Preferences::save_custom_themes_at(&two_custom_themes(), &path).unwrap();
        let original = fs::read(&path).unwrap();
        let too_many: Vec<CustomTheme> = (1..=33)
            .map(|id| custom_theme(id, &format!("Theme {id}"), ThemeChoice::Nord, 0x123456))
            .collect();
        let mut same_id = two_custom_themes();
        same_id[1].id = 1;
        let mut same_name = two_custom_themes();
        same_name[1].name = " SUNRISE ".into();
        let mut built_in_name = two_custom_themes();
        built_in_name[0].name = "braden".into();
        for themes in [too_many, same_id, same_name, built_in_name] {
            assert!(Preferences::save_custom_themes_at(&themes, &path).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
        }
        // Names are stored trimmed, like project names.
        let mut padded = two_custom_themes();
        padded[0].name = "  Sunrise  ".into();
        let saved = Preferences::save_custom_themes_at(&padded, &path).unwrap();
        assert_eq!(saved.custom_themes, two_custom_themes());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn an_unknown_saved_base_falls_back_by_lightness_without_a_write() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        let mut themes = two_custom_themes();
        themes[0].base = ThemeChoice::Porcelain;
        Preferences::save_custom_themes_at(&themes, &path).unwrap();
        let mut store: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        // A newer release that adds built-ins still writes version 6.
        store["custom_themes"][0]["base"] = "future_light".into();
        store["custom_themes"][1]["base"] = "future_dark".into();
        let original = serde_json::to_vec_pretty(&store).unwrap();
        fs::write(&path, &original).unwrap();

        let loaded = Preferences::load_from(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original, "loading never writes");
        // The palettes and names are kept; only the reset target is substituted,
        // Braden for the light theme rather than `ThemeChoice`'s Midnight.
        themes[0].base = ThemeChoice::Daylight;
        themes[1].base = ThemeChoice::Midnight;
        assert_eq!(loaded.custom_themes, themes);
    }

    #[test]
    fn a_missing_custom_selection_resolves_to_the_default_theme_without_a_write() {
        let fixture = TestDirectory::new();
        let path = fixture.0.join("preferences.json");
        Preferences::save_custom_themes_at(&two_custom_themes(), &path).unwrap();
        let mut store: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        store["settings"]["theme"] = serde_json::json!({"custom": 99});
        let original = serde_json::to_vec_pretty(&store).unwrap();
        fs::write(&path, &original).unwrap();

        let loaded = Preferences::load_from(&path).unwrap();
        assert_eq!(loaded.settings.theme, ThemeSelection::Custom(99));
        for appearance in [
            gpui_kit::WindowAppearance::Dark,
            gpui_kit::WindowAppearance::Light,
        ] {
            assert_eq!(
                loaded
                    .settings
                    .resolved_theme(appearance, &loaded.custom_themes),
                ResolvedTheme::built_in(ThemeChoice::default())
            );
        }
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn saving_custom_themes_on_the_preference_executor_merges_the_latest_disk_store() {
        let path = settings_path().unwrap();
        let project = test_settings_directory().path().join("turtle");
        fs::create_dir_all(&project).unwrap();
        let project = project.canonicalize().unwrap();
        Preferences::save_settings_at(&AppSettings::default(), &path).unwrap();
        // The editor edits a copy loaded earlier; settings, recents and names
        // change on disk before its save reaches the executor.
        let snapshot = Preferences::load_from(&path).unwrap();
        let changed = AppSettings {
            density: Density::Compact,
            follow_system: true,
            ..snapshot.settings.clone()
        };
        Preferences::save_settings_at(&changed, &path).unwrap();
        Preferences::default().remember_at(&project, &path).unwrap();
        Preferences::save_project_name_at(&project, Some("Turtle"), &path).unwrap();

        let executor = crate::operations::SerialExecutor::new("gitturtle-preferences-test");
        let themes = two_custom_themes();
        let submitted = themes.clone();
        let saved = futures::executor::block_on(
            executor.submit(move || Preferences::save_custom_themes(&submitted)),
        )
        .unwrap()
        .unwrap();
        let loaded = Preferences::load_from(&path).unwrap();
        for preferences in [&saved, &loaded] {
            assert_eq!(preferences.settings, changed);
            assert_eq!(preferences.recent_repositories, vec![project.clone()]);
            assert_eq!(
                preferences.project_names,
                HashMap::from([(project.clone(), "Turtle".to_owned())])
            );
            assert_eq!(preferences.custom_themes, themes);
        }

        // A later settings save carries the disk's custom section.
        let with_selection = AppSettings {
            theme: ThemeSelection::Custom(1),
            ..changed
        };
        let saved = Preferences::save_settings_at(&with_selection, &path).unwrap();
        assert_eq!(saved.custom_themes, themes);
        assert_eq!(Preferences::load_from(&path).unwrap().custom_themes, themes);
    }

    #[test]
    fn custom_selections_follow_the_system_by_their_own_lightness() {
        let themes = two_custom_themes();
        let (light, dark) = (&themes[0], &themes[1]);
        assert!(light.is_light() && !dark.is_light());
        let mut settings = AppSettings::default();
        for (appearance, follow, selected, expected) in [
            // Not following the system: the selection's palette under either appearance.
            (
                gpui_kit::WindowAppearance::Light,
                false,
                light,
                ResolvedTheme::custom(light),
            ),
            (
                gpui_kit::WindowAppearance::Dark,
                false,
                light,
                ResolvedTheme::custom(light),
            ),
            (
                gpui_kit::WindowAppearance::Light,
                false,
                dark,
                ResolvedTheme::custom(dark),
            ),
            (
                gpui_kit::WindowAppearance::Dark,
                false,
                dark,
                ResolvedTheme::custom(dark),
            ),
            // Following: light selects Braden; dark keeps a dark palette, else Midnight.
            (
                gpui_kit::WindowAppearance::Light,
                true,
                light,
                ResolvedTheme::built_in(ThemeChoice::Daylight),
            ),
            (
                gpui_kit::WindowAppearance::VibrantLight,
                true,
                dark,
                ResolvedTheme::built_in(ThemeChoice::Daylight),
            ),
            (
                gpui_kit::WindowAppearance::Dark,
                true,
                light,
                ResolvedTheme::built_in(ThemeChoice::Midnight),
            ),
            (
                gpui_kit::WindowAppearance::VibrantDark,
                true,
                dark,
                ResolvedTheme::custom(dark),
            ),
        ] {
            settings.theme = ThemeSelection::Custom(selected.id);
            settings.follow_system = follow;
            assert_eq!(
                settings.resolved_theme(appearance, &themes),
                expected,
                "{appearance:?} follow={follow} {}",
                selected.name
            );
        }
        // Built-ins keep the existing rule when custom themes are present.
        settings.follow_system = true;
        settings.theme = ThemeSelection::BuiltIn(ThemeChoice::Nord);
        assert_eq!(
            settings.resolved_theme(gpui_kit::WindowAppearance::Dark, &themes),
            ResolvedTheme::built_in(ThemeChoice::Nord)
        );
        settings.theme = ThemeSelection::BuiltIn(ThemeChoice::Porcelain);
        assert_eq!(
            settings.resolved_theme(gpui_kit::WindowAppearance::Dark, &themes),
            ResolvedTheme::built_in(ThemeChoice::Midnight)
        );
    }
}
