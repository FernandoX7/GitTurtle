//! Local repository tabs: one active reader/watch session and bounded retained views.
use crate::*;
use anyhow::{Context as _, Result, ensure};
use futures::FutureExt;
use gpui_kit::{component::menu::PopupMenuItem, prelude::FluentBuilder};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MAX_TABS: usize = 8;
const MAX_SESSION_BYTES: usize = 16 * 1024 * 1024;
const MAX_RETAINED_BYTES: usize = 512 * 1024 * 1024;
const MAX_LIBRARY: usize = 128;
const MAX_GROUPS: usize = 16;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SavedPath {
    Text(String),
    #[cfg(unix)]
    Bytes {
        unix_bytes: Vec<u8>,
    },
}
impl SavedPath {
    fn new(path: &Path) -> Self {
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
        {
            Self::Text(path.to_string_lossy().into_owned())
        }
    }
    pub(super) fn path(&self) -> PathBuf {
        match self {
            Self::Text(value) => value.into(),
            #[cfg(unix)]
            Self::Bytes { unix_bytes } => {
                use std::os::unix::ffi::OsStringExt;
                std::ffi::OsString::from_vec(unix_bytes.clone()).into()
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SavedScope {
    Branch { name: String, remote: bool },
    Worktree(SavedPath),
}
impl SavedScope {
    fn from_scope(scope: &worker::Scope) -> Self {
        match scope {
            worker::Scope::Branch { name, remote } => Self::Branch {
                name: name.clone(),
                remote: *remote,
            },
            worker::Scope::Worktree { path } => Self::Worktree(SavedPath::new(path)),
        }
    }
    fn scope(&self) -> worker::Scope {
        match self {
            Self::Branch { name, remote } => worker::Scope::Branch {
                name: name.clone(),
                remote: *remote,
            },
            Self::Worktree(path) => worker::Scope::Worktree { path: path.path() },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SavedHistoryScope {
    Pinned(Vec<String>),
    Commit(String),
}
impl SavedHistoryScope {
    fn from_scope(scope: &gitturtle_core::HistoryScope) -> Option<Self> {
        match scope {
            gitturtle_core::HistoryScope::PinnedRefs(tips) => Some(Self::Pinned(tips.clone())),
            gitturtle_core::HistoryScope::FromCommit(oid) => Some(Self::Commit(oid.clone())),
            _ => None,
        }
    }
    fn scope(&self) -> gitturtle_core::HistoryScope {
        match self {
            Self::Pinned(tips) => gitturtle_core::HistoryScope::PinnedRefs(tips.clone()),
            Self::Commit(oid) => gitturtle_core::HistoryScope::FromCommit(oid.clone()),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Bookmark {
    pub scope: Option<(String, SavedScope)>,
    pub pinned: Option<SavedHistoryScope>,
    pub offset: usize,
    pub query: String,
    pub selected_oid: Option<String>,
    pub selection: Option<SavedCommit>,
    pub comparison: Option<SavedFile>,
    pub origins: [Option<String>; 2],
    pub pdf: [Option<pdf_view::Bookmark>; 2],
    pub model: [Option<model_view::Bookmark>; 2],
    pub markdown: Option<markdown_view::Bookmark>,
    pub selected_file: Option<SavedPath>,
    pub parent: usize,
    pub history_y: f32,
    pub horizontal_x: f32,
    pub file_y: f32,
    pub graph_offset: usize,
    pub navigation_width: Option<f32>,
    pub inspector_width: Option<f32>,
    pub zoom: f32,
    pub text_mode: String,
    pub compare: bool,
}
impl Bookmark {
    fn retain_history_only(&mut self) {
        self.comparison = None;
        self.compare = false;
        self.selected_file = None;
        self.origins = [None, None];
        self.pdf = [None, None];
        self.model = [None, None];
        self.markdown = None;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedCommit {
    oid: String,
    parents: Vec<String>,
    author: String,
    timestamp: i64,
    subject: String,
    body: String,
}
impl SavedCommit {
    fn new(commit: &Commit) -> Self {
        Self {
            oid: commit.oid.clone(),
            parents: commit.parents.clone(),
            author: commit.author.clone(),
            timestamp: commit.timestamp,
            subject: commit.subject.clone(),
            body: commit.body.clone(),
        }
    }
    fn commit(&self) -> Commit {
        Commit {
            oid: self.oid.clone(),
            parents: self.parents.clone(),
            author: self.author.clone(),
            timestamp: self.timestamp,
            subject: self.subject.clone(),
            body: self.body.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedFile {
    old_path: Option<SavedPath>,
    new_path: Option<SavedPath>,
    old_oid: Option<String>,
    new_oid: Option<String>,
    old_mode: String,
    new_mode: String,
    status: String,
}
impl SavedFile {
    fn new(file: &FileChange) -> Self {
        Self {
            old_path: file.old_path.as_deref().map(SavedPath::new),
            new_path: file.new_path.as_deref().map(SavedPath::new),
            old_oid: file.old_oid.clone(),
            new_oid: file.new_oid.clone(),
            old_mode: file.old_mode.clone(),
            new_mode: file.new_mode.clone(),
            status: file.status.letter().into(),
        }
    }
    fn file(&self) -> FileChange {
        FileChange {
            old_path: self.old_path.as_ref().map(SavedPath::path),
            new_path: self.new_path.as_ref().map(SavedPath::path),
            old_oid: self.old_oid.clone(),
            new_oid: self.new_oid.clone(),
            old_mode: self.old_mode.clone(),
            new_mode: self.new_mode.clone(),
            status: match self.status.as_str() {
                "A" => gitturtle_core::ChangeStatus::Added,
                "D" => gitturtle_core::ChangeStatus::Deleted,
                "R" => gitturtle_core::ChangeStatus::Renamed,
                "T" => gitturtle_core::ChangeStatus::TypeChanged,
                _ => gitturtle_core::ChangeStatus::Modified,
            },
        }
    }
    fn has_captured_sides(&self) -> bool {
        (self.old_path.is_some() || self.new_path.is_some())
            && self.old_path.is_some() == self.old_oid.is_some()
            && self.new_path.is_some() == self.new_oid.is_some()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedTab {
    pub path: SavedPath,
    #[serde(default)]
    pub bookmark: Bookmark,
    /// Whether `path` is still the folder the person asked to open, its
    /// explicit open never accepted (`Tab::resolved`). Written only then:
    /// a tab saved without it, as every older session's, is the root its
    /// open resolved, which a restore must find again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unresolved: bool,
    /// The Git directories the tab captured at its root (`Tab::git_dir`),
    /// which its first reopen after a restart names: a `.git` swapped for
    /// another repository's gitfile, or a `commondir` re-pointed, while the
    /// app was closed marks the tab. Written only once captured; a tab
    /// saved without them, as every older session's, captures them at its
    /// first reopen. A malformed value is none captured
    /// ([`lenient_git_directories`]).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient_git_directories"
    )]
    pub git_directories: Option<SavedGitDirectories>,
}
/// [`SavedTab::git_directories`] as saved, or none where a value that parses
/// has the wrong shape: it degrades alone, so the tab captures them at its
/// first reopen. Failing the whole session, it dropped every tab, the library
/// and its groups. A syntax error is the file's, reported as in any other
/// field: swallowed part way through a token, the rest of the file loaded as
/// if the field were absent.
fn lenient_git_directories<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<SavedGitDirectories>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(serde_json::from_value(serde_json::Value::deserialize(deserializer)?).ok())
}
/// [`worker::GitDirectories`] as a session saves them, byte-safe as a tab's
/// path is, each with its inode at capture and its birth time where the
/// filesystem records one, which the tab restored from the session compares
/// ([`worker::GitDirectories::restored`]). A session saved before they were
/// kept has none and compares none; one saved before birth times were kept
/// has the inodes alone and compares those.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedGitDirectories {
    pub git_dir: SavedPath,
    pub common: SavedPath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_dir_inode: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_inode: Option<u64>,
    /// Since the Unix epoch ([`gitturtle_core::GitRepository::git_dir_birth`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_dir_birth: Option<std::time::Duration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_birth: Option<std::time::Duration>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub path: SavedPath,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub group: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub version: u32,
    pub tabs: Vec<SavedTab>,
    pub active: usize,
    pub library: Vec<LibraryEntry>,
    pub groups: Vec<String>,
}
impl Session {
    pub fn load() -> Self {
        session_path()
            .and_then(|path| Self::load_at(&path))
            .unwrap_or_default()
    }
    fn load_at(path: &Path) -> Result<Self> {
        let bytes = preferences::read_store(path, MAX_SESSION_BYTES as u64)?;
        let mut session: Self = serde_json::from_slice(&bytes)?;
        ensure!(
            session.version == 1,
            "Unsupported repository session version"
        );
        session.normalize();
        Ok(session)
    }
    fn normalize(&mut self) {
        self.version = 1;
        let active = self.tabs.get(self.active).map(|tab| tab.path.path());
        let mut seen = HashSet::new();
        self.tabs.retain(|tab| {
            let path = tab.path.path();
            path.is_absolute() && seen.insert(path)
        });
        self.tabs.truncate(MAX_TABS);
        self.active = active
            .and_then(|path| self.tabs.iter().position(|tab| tab.path.path() == path))
            .unwrap_or(0);
        self.groups.retain(|group| {
            !group.trim().is_empty() && group.len() <= 64 && !group.contains(['\0', '\n', '\r'])
        });
        let mut seen = HashSet::new();
        self.groups.retain(|group| seen.insert(group.clone()));
        self.groups.truncate(MAX_GROUPS);
        let mut seen = HashSet::new();
        self.library.retain(|entry| {
            let path = entry.path.path();
            path.is_absolute() && seen.insert(path)
        });
        self.library.truncate(MAX_LIBRARY);
        for entry in &mut self.library {
            if entry
                .group
                .as_ref()
                .is_some_and(|group| !self.groups.contains(group))
            {
                entry.group = None;
            }
        }
        for tab in &mut self.tabs {
            let b = &mut tab.bookmark;
            // Older sessions could save a mutable Quick Source as an Added
            // comparison without a blob. It cannot be read as immutable history.
            if b.comparison
                .as_ref()
                .is_some_and(|file| !file.has_captured_sides())
            {
                b.retain_history_only();
            }
            while b.query.len() > 4096 {
                b.query.pop();
            }
            b.graph_offset = b.graph_offset.min(127);
            b.navigation_width = b
                .navigation_width
                .filter(|value| value.is_finite())
                .map(|value| value.clamp(180., 360.));
            b.inspector_width = b
                .inspector_width
                .filter(|value| value.is_finite())
                .map(|value| value.clamp(280., 480.));
            if !b.zoom.is_finite() || b.zoom <= 0. {
                b.zoom = 1.;
            }
            if !b.history_y.is_finite() {
                b.history_y = 0.;
            }
            if !b.file_y.is_finite() {
                b.file_y = 0.;
            }
            if !b.horizontal_x.is_finite() {
                b.horizontal_x = 0.;
            }
        }
    }
    pub fn save(self) -> Result<()> {
        self.save_at(&session_path()?)
    }
    fn save_at(mut self, path: &Path) -> Result<()> {
        match Self::load_at(path) {
            Ok(_) => {}
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            Err(error) => {
                return Err(error).context(
                    "The existing repository session could not be read and was preserved for recovery",
                );
            }
        }
        self.normalize();
        let bytes = serde_json::to_vec_pretty(&self)?;
        ensure!(
            bytes.len() <= MAX_SESSION_BYTES,
            "Repository session exceeds 16 MiB"
        );
        preferences::atomic_write(path, &bytes)
    }
}
fn session_path() -> Result<PathBuf> {
    Ok(preferences::settings_path()?.with_file_name("repository-session.json"))
}

pub struct Tab {
    pub path: PathBuf,
    /// Whether `path` is the work-tree root Git discovery found when the
    /// person opened this tab, which every later passive reopen (Refresh,
    /// quiet reads, a restore at launch) must find there again
    /// ([`State::reopen_root`]): `false` only while a tab's explicit open of
    /// the folder the person chose has not been accepted
    /// ([`GitTurtle::tab_snapshot_accepted`]), when it may still land on
    /// the work tree enclosing that folder.
    resolved: bool,
    /// The Git directories Git reported at the tab's root when its snapshot
    /// was last accepted or a read last installed its repository in place
    /// ([`State::follow_read_repository`]), which a passive reopen of that
    /// root must find there again ([`State::reopen_root`]); `None` until
    /// then. Saved with
    /// the session, so a restored tab names them, and compares their
    /// inodes, at its first reopen; an older session's tab, saved without
    /// them, captures them then.
    git_dir: Option<worker::GitDirectories>,
    saved: Bookmark,
    warm: Option<WarmTab>,
    pub error: Option<String>,
}
/// Why a session save did not finish: a full queue refused it, which only
/// asks again, or it really failed, which the window reports.
#[derive(Clone, Debug)]
enum SessionSaveFailure {
    Busy,
    Failed(String),
}
type SessionSaveCompletion = futures::future::Shared<
    futures::future::BoxFuture<'static, std::result::Result<(), SessionSaveFailure>>,
>;

/// Where a warm tab shown again puts focus: the control it was left with,
/// for as long as its repository is the one shown. A cold tab's restore
/// landing in Compare uses it to keep the control the person focused while
/// the tab was restored, where the frame that draws Compare still draws it
/// ([`GitTurtle::land_restore_focus`]), and a failed one that shows Projects
/// to keep a control that page draws too
/// ([`GitTurtle::show_projects_for_failed_open`]).
struct FocusReturn {
    /// The page the return is for: another shown since supersedes it.
    page: AppPage,
    path: Option<PathBuf>,
    /// Where focus waits for the return: the window's root after a warm
    /// switch, the person's control as a restore lands. Focus moved off it
    /// since supersedes the return.
    held: FocusHandle,
    focus: Option<FocusHandle>,
}

/// History's search asked for while the shown tab's snapshot is read
/// ([`GitTurtle::defer_search`]), for the tab it was asked in and the focus
/// the key found. It opens once the tab can show History's search, unless
/// focus has moved since ([`GitTurtle::run_pending_search`]).
struct PendingSearch {
    path: PathBuf,
    focus: Option<FocusHandle>,
}

#[derive(Default)]
pub struct State {
    pub tabs: Vec<Tab>,
    pub active: Option<usize>,
    pub library: Vec<LibraryEntry>,
    pub groups: Vec<String>,
    pub switching: bool,
    /// The bookmark the shown tab, cold until its restore ends, is being
    /// restored from. It belongs to that tab: a tab left mid-restore takes
    /// it back (`retain_active_tab`), and closing the tab drops it.
    pub restoring: Option<Bookmark>,
    pub document_restore: Option<Bookmark>,
    /// The focus a warm tab shown again gives back as the frame that draws
    /// it is built ([`GitTurtle::return_tab_focus`]).
    focus_return: Option<FocusReturn>,
    /// The watch on the control a cold restore landing in Compare left
    /// focus on, until focus leaves it ([`GitTurtle::land_focus_leaving`]).
    landed_focus: Option<Subscription>,
    /// History's search asked for while the shown tab's snapshot is read,
    /// until the tab can show it ([`GitTurtle::run_pending_search`]).
    pending_search: Option<PendingSearch>,
    panel_observers: Vec<Subscription>,
    pub save_pending: Option<Task<()>>,
    /// The wait before asking again after a full queue refused the save.
    save_retry: Option<Task<()>>,
    saver: commit_drafts::CoalescingSaver<(), Session>,
    save_completion: Option<SessionSaveCompletion>,
    closing: bool,
    /// Why the tab strip last refused what it was asked, drawn beneath the
    /// strip until dismissed or a tab opens, closes or is shown
    /// ([`GitTurtle::refuse_in_tab_strip`]).
    refusal: Option<String>,
    /// The strip's refusals so far, which gives each its own line node, so
    /// the same refusal asked for again is announced again.
    refusals: usize,
    /// The explicit Open of a marked tab's folder under way, which reads it
    /// afresh ([`GitTurtle::open_repository_tab`]).
    recapture: Option<Recapture>,
    #[cfg(test)]
    save_path: Option<PathBuf>,
}
/// An explicit Open of a marked tab's folder (Open…, the projects list, a
/// library group): its read names no root or Git directory, so it may find
/// another repository there, the work tree enclosing the folder among them.
/// The tab keeps the root, Git directory and mark it had until that read's
/// snapshot is accepted, which replaces them
/// ([`GitTurtle::tab_snapshot_accepted`]); a failed read leaves them as they
/// were ([`GitTurtle::tab_opening_failed`]). Cleared as the Open was asked,
/// a failed one once left the tab with nothing captured, saved as
/// unresolved, and its next Refresh, an explicit open too, could open the
/// enclosing repository in the tab, writes following it.
enum Recapture {
    /// Asked for, until the Open's read is: an Open of this folder then is
    /// that read ([`State::recapture_asked`]).
    Asked(PathBuf),
    /// That read, by its generation (`GitTurtle::generation`).
    Reading(u64),
    /// That read failed, or an Open could not check the folder, for the tab
    /// at `path`, with what the window and the Projects page say for it in
    /// place of the mark's own reason: it goes with that tab's mark, never
    /// another's ([`GitTurtle::tab_found_readable`]).
    Failed { path: PathBuf, error: String },
}
/// The tab strip's refusal of a ninth tab, in the same words wherever the
/// person asked for it: the strip's own line or the Projects page's.
/// The local library's refusal of a repository past its bound, said where
/// the person asked ([`GitTurtle::set_tab_library`]).
const LIBRARY_FULL_REFUSAL: &str =
    "Local library holds up to 128 repositories. Remove an entry before adding another.";
pub(super) const TAB_LIMIT_REFUSAL: &str =
    "Switch to an open tab, or close one to open another; its commit drafts remain saved.";
/// The refusal of a switch past the retained allowance, said where the
/// person asked, as the tab limit's is ([`GitTurtle::refuse_where_asked`]).
const RETAINED_ALLOWANCE_REFUSAL: &str = "Retained tabs would exceed the 512 MiB preview allowance. Close a tab to release its retained inspection before switching; drafts stay saved.";
impl State {
    /// Whether an Open of `path` is the read an explicit Open of its marked
    /// tab's folder asked for ([`Recapture`]), naming no root or Git
    /// directory.
    pub(super) fn recapture_asked(&self, path: &Path) -> bool {
        matches!(&self.recapture, Some(Recapture::Asked(asked)) if asked == path)
    }
    /// Notes the read [`Self::recapture_asked`] said an Open was, by the
    /// generation that Open's read has.
    pub(super) fn recapture_reading(&mut self, generation: u64) {
        self.recapture = Some(Recapture::Reading(generation));
    }
    /// Whether `path` is a tab marked " · unavailable": a row that opens it
    /// (the Projects page's, the project list's) is then an explicit Open of
    /// its folder ([`Recapture`]), where a row of a whole tab only shows it.
    pub(super) fn is_marked(&self, path: &Path) -> bool {
        self.find(path)
            .is_some_and(|index| self.tabs[index].error.is_some())
    }
    /// The repository a read of the shown tab just installed in place, the
    /// read's reopen having found it at the tab's root (a quiet read's, or
    /// Show latest's): the tab's directories and inodes follow it. Left as
    /// captured, a repository cloned again at that path during the run saved
    /// its old inodes and marked the tab at the next launch. Only the tab
    /// the repository is at takes them; an Open's accepted snapshot writes
    /// its own ([`GitTurtle::tab_snapshot_accepted`]), and a warm tab shown
    /// again holds what it held when it was left.
    pub(super) fn follow_read_repository(&mut self, repository: &gitturtle_core::GitRepository) {
        if let Some(tab) = self
            .active
            .and_then(|index| self.tabs.get_mut(index))
            .filter(|tab| tab.path == repository.path())
        {
            tab.git_dir = Some(worker::GitDirectories::of(repository));
        }
    }
    /// Drops History's search held while a snapshot was read, as another
    /// read of a repository starts or another tab shows
    /// ([`GitTurtle::run_pending_search`]).
    pub(super) fn drop_pending_search(&mut self) {
        self.pending_search = None;
    }

    /// Keeps History's search held for the path a tab was opened by once
    /// its snapshot names the repository's own path, which the window then
    /// shows ([`GitTurtle::run_pending_search`]).
    pub(super) fn resolve_pending_search(&mut self, opened: Option<&Path>, resolved: &Path) {
        if let Some(pending) = self.pending_search.as_mut()
            && opened == Some(pending.path.as_path())
        {
            pending.path = resolved.to_owned();
        }
    }

    /// Whether the restore under way lands in Compare, on a captured
    /// comparison or an older session's chosen file, where History's search
    /// held meanwhile opens ([`GitTurtle::land_restore_focus`]), or in
    /// History where it never gets there: its file gone or none chosen
    /// (`restore_tab_file`), or a read of its own that `GitTurtle::request`
    /// sends, or its saved query's search page
    /// (`GitTurtle::continue_history_search`), failed or dropped.
    pub(super) fn restore_lands_in_compare(&self) -> bool {
        self.restoring
            .as_ref()
            .is_some_and(|saved| saved.comparison.is_some() || saved.compare)
    }

    pub(super) fn rescale_text_viewports(
        &mut self,
        lists: Option<settings::ListScales>,
        code_ratio: f32,
        cx: &mut App,
    ) {
        for tab in &mut self.tabs {
            if let Some(scales) = lists {
                tab.saved.history_y *= scales.history;
                tab.saved.file_y *= scales.files;
            }
            if let Some(warm) = &mut tab.warm {
                if code_ratio != 1.0 {
                    warm.context.rescale_code(code_ratio, cx);
                    warm.file_history.rescale_code(code_ratio, cx);
                    warm.inspections.rescale_code(code_ratio, cx);
                }
                if let Some(scales) = lists {
                    for (scroll, ratio) in [
                        (&warm.history_scroll, scales.history),
                        (&warm.file_scroll, scales.files),
                        (&warm.nav_scroll, scales.navigation),
                        (&warm.working_scroll, scales.files),
                    ] {
                        settings::rescale_list_scroll(scroll, ratio);
                    }
                    warm.blame.rescale_lists(scales);
                    warm.file_history.rescale_lists(scales);
                    warm.inspections.rescale_lists(scales);
                    warm.history_list_layout = None;
                    warm.file_list_layout = None;
                }
            }
        }
        if let Some(scales) = lists {
            for saved in [&mut self.restoring, &mut self.document_restore]
                .into_iter()
                .flatten()
            {
                saved.history_y *= scales.history;
                saved.file_y *= scales.files;
            }
        }
    }

    pub fn from_session(session: Session) -> Self {
        let active = (!session.tabs.is_empty()).then_some(session.active);
        Self {
            tabs: session
                .tabs
                .into_iter()
                .map(|tab| Tab {
                    path: tab.path.path(),
                    resolved: !tab.unresolved,
                    git_dir: tab
                        .git_directories
                        .map(|directories| worker::GitDirectories {
                            git_dir: directories.git_dir.path(),
                            common: directories.common.path(),
                            inodes: worker::DirectoryInodes {
                                git_dir: directories.git_dir_inode,
                                common: directories.common_inode,
                                git_dir_birth: directories.git_dir_birth,
                                common_birth: directories.common_birth,
                            },
                            restored: true,
                        }),
                    saved: tab.bookmark,
                    warm: None,
                    error: None,
                })
                .collect(),
            active,
            library: session.library,
            groups: session.groups,
            ..Default::default()
        }
    }
    pub fn active_path(&self) -> Option<&Path> {
        self.active
            .and_then(|index| self.tabs.get(index))
            .map(|tab| tab.path.as_path())
    }
    fn find(&self, path: &Path) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.path == path)
    }
    /// The root an Open of `path` in the shown tab must find there again
    /// (`worker::Job::Open::root`): the tab's own path once its explicit
    /// open resolved it ([`Tab::resolved`]), or a worktree's own path, as
    /// Git lists it, opened from its row. `None` for a tab's first,
    /// explicit open of the folder the person chose (Open…, a dropped
    /// folder, the projects list, a library group), which may land on the
    /// work tree enclosing that folder. A guess that misses fails safe: the
    /// tab shows its repository unavailable rather than open another. With
    /// the tab's own path come the Git directories it captured there
    /// ([`Tab::git_dir`], `worker::Job::Open::git_dir`), which the reopen
    /// must find too; a worktree's row names none.
    pub(super) fn reopen_root(
        &self,
        path: &Path,
        scope: Option<&worker::Scope>,
    ) -> Option<(PathBuf, Option<worker::GitDirectories>)> {
        if let Some(tab) = self
            .active
            .and_then(|index| self.tabs.get(index))
            .filter(|tab| tab.resolved && tab.path == path)
        {
            return Some((path.to_owned(), tab.git_dir.clone()));
        }
        let worktree =
            matches!(scope, Some(worker::Scope::Worktree { path: tree }) if tree == path);
        worktree.then(|| (path.to_owned(), None))
    }
    fn saved(&self) -> Session {
        Session {
            version: 1,
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab {
                    path: SavedPath::new(&tab.path),
                    bookmark: tab.saved.clone(),
                    unresolved: !tab.resolved,
                    git_directories: tab.git_dir.as_ref().map(|directories| SavedGitDirectories {
                        git_dir: SavedPath::new(&directories.git_dir),
                        common: SavedPath::new(&directories.common),
                        git_dir_inode: directories.inodes.git_dir,
                        common_inode: directories.inodes.common,
                        git_dir_birth: directories.inodes.git_dir_birth,
                        common_birth: directories.inodes.common_birth,
                    }),
                })
                .collect(),
            active: self.active.unwrap_or(0),
            library: self.library.clone(),
            groups: self.groups.clone(),
        }
    }
    fn retained_bytes(&self) -> usize {
        self.tabs
            .iter()
            .filter_map(|tab| tab.warm.as_ref())
            .map(|warm| warm.bytes)
            .sum()
    }
    fn reorder(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let active = self.active_path().map(Path::to_owned);
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = active.and_then(|path| self.find(&path));
    }
}

/// A saved inspector may address a commit outside the bounded visible window.
/// Retain one immutable identity without adding it to the visible rows.
fn restore_selected_identity(
    saved: &Bookmark,
    commits: &mut Vec<Commit>,
    graph: &mut Vec<graph::GraphRow>,
    retained: &mut Option<usize>,
) -> Option<usize> {
    let oid = saved.selected_oid.as_ref()?;
    if let Some(index) = commits.iter().position(|commit| &commit.oid == oid) {
        return Some(index);
    }
    let commit = saved
        .selection
        .as_ref()
        .filter(|commit| &commit.oid == oid)?
        .commit();
    let index = commits.len();
    commits.push(commit);
    graph.push(graph::GraphRow::default());
    *retained = Some(index);
    Some(index)
}

fn restore_list_viewport(
    scroll: &UniformListScrollHandle,
    y: f32,
    layout: &mut Option<(Size<Pixels>, Pixels)>,
) {
    let mut state = scroll.0.borrow_mut();
    // Cold reads queue initial item navigation before their saved bookmark is
    // applied. A direct offset alone leaves that request active for prepaint.
    state.deferred_scroll_to_item = None;
    state.base_handle.set_offset(point(px(0.), px(y)));
    // A loading/empty layout is not the restored list's resize baseline.
    // Its first populated paint must preserve intentional manual scrolling.
    *layout = None;
}

pub(super) fn file_bytes(file: &FileChange) -> usize {
    std::mem::size_of::<FileChange>()
        + file.old_path.as_ref().map_or(0, |p| p.as_os_str().len())
        + file.new_path.as_ref().map_or(0, |p| p.as_os_str().len())
        + file.old_oid.as_ref().map_or(0, String::capacity)
        + file.new_oid.as_ref().map_or(0, String::capacity)
        + file.old_mode.capacity()
        + file.new_mode.capacity()
}
pub(super) fn history_bytes(commits: &[Commit], graph: &[graph::GraphRow]) -> usize {
    commits.iter().map(Commit::history_bytes).sum::<usize>()
        + graph
            .iter()
            .map(|row| {
                std::mem::size_of::<graph::GraphRow>()
                    + row.edges.len() * std::mem::size_of::<graph::Edge>()
            })
            .sum::<usize>()
}
// The widgets are shared by the window, but their values belong to one tab.
// Cold tabs always start with empty inputs; warm tabs restore the exact
// snapshot: each field's text, and its selection with the end its caret is
// at, so a tab shown again keeps what the person left selected. Restored
// from its text alone, a field came back with its caret at the end and
// nothing selected.
#[derive(Default)]
struct TabInputs([FieldSnapshot; 6]);
impl TabInputs {
    fn capture(inputs: [&Entity<InputState>; 6], window: &mut Window, cx: &mut App) -> Self {
        Self(inputs.map(|input| FieldSnapshot::capture(input, window, cx)))
    }
    fn restore(self, inputs: [&Entity<InputState>; 6], window: &mut Window, cx: &mut App) {
        for (input, field) in inputs.into_iter().zip(self.0) {
            field.restore(input, window, cx);
        }
    }
}

/// One field's text and selection as its tab was left.
#[derive(Default)]
pub(super) struct FieldSnapshot {
    value: String,
    /// The selection's byte offsets, the same offset twice where only the
    /// caret is.
    selected: std::ops::Range<usize>,
    /// Whether the caret is at the selection's start, as a selection made
    /// backwards leaves it.
    reversed: bool,
}
impl FieldSnapshot {
    pub(super) fn capture<M: gpui_kit::base::input::InputModeKind>(
        input: &Entity<gpui_kit::base::input::InputBaseState<M>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        use gpui::EntityInputHandler as _;
        input.update(cx, |input, cx| {
            let value = input.value().to_string();
            let caret = input.cursor();
            // Text an input method is still composing is kept as typed, with
            // the caret at its end and nothing selected: a selection within
            // it is the input method's, which ends with the composition.
            if input.marked_text_range(window, cx).is_some() {
                return Self {
                    value,
                    selected: caret..caret,
                    reversed: false,
                };
            }
            // The kit reports where the caret is rather than the selection's
            // direction; with no composition its caret is the selection's
            // start exactly where the selection was made backwards.
            let selected = input.selected_range();
            let reversed = !selected.is_empty() && caret == selected.start;
            Self {
                value,
                selected,
                reversed,
            }
        })
    }
    pub(super) fn restore<M: gpui_kit::base::input::InputModeKind>(
        self,
        input: &Entity<gpui_kit::base::input::InputBaseState<M>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        input.update(cx, |input, cx| {
            input.set_value(self.value, window, cx);
            // The text leaves a single-line field's caret at its end and a
            // multi-line field's at its start, nothing selected; the
            // selection is always set again, from where it began to the
            // caret, so one made backwards keeps its caret at its start.
            let std::ops::Range { start, end } = self.selected;
            let (anchor, caret) = if self.reversed {
                (end, start)
            } else {
                (start, end)
            };
            input.set_selected_range(anchor..caret, cx);
        });
    }
}

struct WarmTab {
    bytes: usize,
    repository: GitRepository,
    scope: Option<(String, worker::Scope)>,
    context: file_history::ReturnContext,
    file_history: file_history::State,
    inspections: revision_inspection::State,
    blame: blame::State,
    search: history_search::State,
    paging: history_paging::State,
    history_updates: history_updates::State,
    inputs: TabInputs,
    branches: Vec<Branch>,
    worktrees: Vec<Worktree>,
    nav_rows: Vec<NavRow>,
    nav_ids: Vec<ElementId>,
    nav_mode: NavMode,
    nav_cursor: Option<usize>,
    expanded: HashSet<String>,
    commits: Vec<Commit>,
    visible: Vec<usize>,
    graph: Vec<graph::GraphRow>,
    graph_lanes: usize,
    graph_offset: usize,
    graph_notice: Option<String>,
    refs: worker::HistoryReferences,
    selected_commit: Option<usize>,
    parent: usize,
    retained_commit: Option<usize>,
    history_scroll: UniformListScrollHandle,
    file_scroll: UniformListScrollHandle,
    // Resize detection must compare with this tab's measured viewport, never
    // with the preceding tab's independently retained panel geometry.
    history_list_layout: Option<(Size<Pixels>, Pixels)>,
    working_layout: Option<Entity<workspace::WorkingInspectorLayout>>,
    file_list_layout: Option<(Size<Pixels>, Pixels)>,
    nav_scroll: UniformListScrollHandle,
    horizontal: ScrollHandle,
    content_panels: Entity<ResizableState>,
    history_panels: Entity<ResizableState>,
    retained_history_files: Option<(Vec<FileChange>, Option<usize>)>,
    work_status: Option<Arc<gitturtle_core::RepositoryStatus>>,
    /// The Git directories `work_status` and `remotes` were read from
    /// (`GitTurtle::working_identity`).
    working_identity: Option<worker::GitDirectories>,
    integration_state: Option<gitturtle_core::OperationState>,
    profile: Option<gitturtle_core::GitProfile>,
    remotes: Vec<gitturtle_core::Remote>,
    working_rows: Vec<workspace::WorkingRow>,
    /// The counts that go with `working_rows`, shown with them on return
    /// until the filter's rows land.
    working_counts: working_selection::Counts,
    /// What the rows are prepared again for as the tab returns, where
    /// `working_rows` are not the status's own rows for its query and
    /// grouping: rows still being prepared as it was left, or a filter
    /// error. `None` brings the rows back as they were left.
    working_unprepared: Option<working_selection::Preparation>,
    working_selected: Option<(usize, gitturtle_core::ChangeArea)>,
    working_scroll: UniformListScrollHandle,
    working_selection: working_selection::Selection,
    operation_notice: Option<String>,
    operation_error: Option<crate::views::OperationFailure>,
    /// Which of [`Self::operation_error`] a quiet read wrote
    /// (`automatic_refresh::State::failure`).
    local_refresh_failure: Option<crate::views::OperationFailure>,
    /// The places a comparison restored with the tab waited to restore as
    /// the tab was left (`State::document_restore`), which its return
    /// restores ([`GitTurtle::resume_tab_documents`]).
    document_restore: Option<Bookmark>,
}

impl GitTurtle {
    fn tab_retained_bytes(&self) -> usize {
        history_bytes(&self.commits, &self.graph)
            + self.inspector_message.retained_bytes()
            + self
                .content
                .as_ref()
                .map_or(0, |content| content.bytes().saturating_mul(4))
            // Compare's unified patch keeps its decorations with its editor; the
            // presentation they were built from is counted in the content above.
            + self
                .patch_decoration
                .as_ref()
                .map_or(0, editor_find::PatchDecorations::retained_bytes)
            + self.files.iter().map(file_bytes).sum::<usize>()
            + self.file_history.retained_bytes()
            + self.revision_inspection.retained_bytes()
            + self.blame.retained_bytes()
            + self.history_search.retained_bytes()
            + self.history_updates.retained_bytes()
            + self
                .branches
                .iter()
                .map(|branch| {
                    std::mem::size_of::<Branch>() + branch.name.capacity() + branch.oid.capacity()
                })
                .sum::<usize>()
            + self
                .worktrees
                .iter()
                .map(|worktree| {
                    std::mem::size_of::<Worktree>()
                        + worktree.path.as_os_str().len()
                        + worktree.branch.as_ref().map_or(0, String::capacity)
                        + worktree.oid.capacity()
                })
                .sum::<usize>()
            + self
                .refs
                .retained_bytes()
            + self
                .expanded_folders
                .iter()
                .map(String::capacity)
                .sum::<usize>()
            + self
                .retained_history_files
                .as_ref()
                .map_or(0, |(files, _)| files.iter().map(file_bytes).sum::<usize>())
            + self.work_status.as_ref().map_or(0, |status| {
                status
                    .entries
                    .iter()
                    .map(|entry| {
                        std::mem::size_of_val(entry)
                            + entry.path.as_os_str().len()
                            + entry
                                .original_path
                                .as_ref()
                                .map_or(0, |path| path.as_os_str().len())
                    })
                    .sum()
            })
    }
    fn tab_bookmark(&self, cx: &App) -> Bookmark {
        let selected = self
            .selected_commit
            .and_then(|index| self.commits.get(index));
        let origins = self
            .file_history_preview_origins()
            .or_else(|| self.inspection_preview_origins())
            .unwrap_or_else(|| {
                markdown_view::Origins::revisions(
                    selected
                        .and_then(|commit| commit.parents.get(self.parent))
                        .cloned(),
                    selected.map(|commit| commit.oid.clone()),
                )
            });
        let revisions = [origins.old, origins.new].map(|scope| match scope {
            Some(gitturtle_core::PreviewAssetScope::Revision(oid)) => Some(oid),
            _ => None,
        });
        let (pdf, model) = if let Some(Content::Rich(content)) = self.content.as_deref() {
            (
                [
                    content.old.pdf.as_ref().map(|document| document.bookmark()),
                    content.new.pdf.as_ref().map(|document| document.bookmark()),
                ],
                [
                    content
                        .old
                        .model
                        .as_ref()
                        .map(|document| document.bookmark()),
                    content
                        .new
                        .model
                        .as_ref()
                        .map(|document| document.bookmark()),
                ],
            )
        } else {
            ([None, None], [None, None])
        };
        let mut bookmark = Bookmark {
            selection: selected.map(SavedCommit::new),
            comparison: (self.mode == WorkspaceMode::Compare)
                .then(|| {
                    self.selected_file
                        .and_then(|index| self.files.get(index))
                        .map(SavedFile::new)
                })
                .flatten(),
            origins: revisions,
            pdf,
            model,
            markdown: self
                .markdown_view
                .as_ref()
                .map(|view| view.read(cx).bookmark()),
            scope: self
                .scope
                .as_ref()
                .map(|(label, scope)| (label.clone(), SavedScope::from_scope(scope))),
            pinned: self
                .history_paging
                .scope
                .as_ref()
                .and_then(SavedHistoryScope::from_scope)
                .or_else(|| {
                    self.history_search
                        .pinned_scope()
                        .as_ref()
                        .and_then(SavedHistoryScope::from_scope)
                }),
            offset: self.history_paging.offset,
            query: self.search.read(cx).value().to_string(),
            selected_oid: self
                .selected_commit
                .and_then(|index| self.commits.get(index))
                .map(|commit| commit.oid.clone()),
            selected_file: self
                .selected_file
                .and_then(|index| self.files.get(index))
                .and_then(|file| file.new_path.as_deref().or(file.old_path.as_deref()))
                .map(SavedPath::new),
            parent: self.parent,
            history_y: f32::from(self.history_scroll.0.borrow().base_handle.offset().y),
            horizontal_x: f32::from(self.history_horizontal.offset().x),
            file_y: f32::from(self.file_scroll.0.borrow().base_handle.offset().y),
            graph_offset: self.graph_offset,
            navigation_width: Some(self.settings.navigation_width),
            inspector_width: Some(self.settings.inspector_width),
            zoom: self.zoom,
            text_mode: match self.text_mode {
                TextMode::Unified => "diff",
                TextMode::Split => "split",
                TextMode::Before => "before",
                TextMode::After => "after",
                TextMode::Diagrams => "diagrams",
                #[allow(unreachable_patterns)]
                _ => "markdown",
            }
            .into(),
            compare: self.mode == WorkspaceMode::Compare,
        };
        // A comparison restored with the tab whose documents wait for its
        // reply, or for the window's return to it, keeps the places it was
        // saved with, which its documents do not show yet. Taken from what
        // shows, a tab left mid-restore saved the window's zoom and no PDF
        // page or model camera, and came back without them.
        if let Some(saved) = self.pending_tab_documents() {
            bookmark.zoom = saved.zoom;
            bookmark.pdf = saved.pdf;
            bookmark.model = saved.model.clone();
            bookmark.markdown = saved.markdown;
        }
        // Quick Open's source scope/inspection stack is retained only in warm
        // tabs. Cold restoration can safely keep its underlying history
        // selection, but must not label the source as that commit's file change.
        if self.is_quick_source() {
            bookmark.retain_history_only();
        }
        bookmark
    }
    fn retain_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(repo) = self.repository.clone() else {
            self.leave_unread_tab();
            return true;
        };
        // A cold tab still being restored from its bookmark keeps nothing
        // warm (below), so it retains nothing.
        let cold = self.repository_tabs.restoring.is_some();
        let bytes = self.tab_retained_bytes();
        if !cold && self.repository_tabs.retained_bytes().saturating_add(bytes) > MAX_RETAINED_BYTES
        {
            self.refuse_where_asked(RETAINED_ALLOWANCE_REFUSAL, cx);
            return false;
        }
        self.persist_commit_draft(window, cx);
        let index = if let Some(index) = self.repository_tabs.find(repo.path()) {
            index
        } else {
            if self.repository_tabs.tabs.len() >= MAX_TABS {
                return false;
            }
            self.repository_tabs.tabs.push(Tab {
                path: repo.path().to_owned(),
                resolved: true,
                git_dir: Some(worker::GitDirectories::of(&repo)),
                saved: Bookmark::default(),
                warm: None,
                error: None,
            });
            self.repository_tabs.tabs.len() - 1
        };
        // The bookmark a cold tab is being restored from belongs to that tab:
        // left before its restore ends, the tab keeps it and stays cold, so
        // shown again it is restored from it again. Left in the window, the
        // next tab took it up: its Refresh restored this tab's zoom, widths
        // and comparison, read in its own repository, its selection was no
        // longer revealed, and this tab came back warm without the selection
        // and comparison it was saved with.
        let restoring = self.repository_tabs.restoring.take();
        let bookmark = restoring.unwrap_or_else(|| self.tab_bookmark(cx));
        // The places a restored comparison waits to restore belong to this
        // tab: they go with it, for its return. Left in the window, the next
        // tab shown took them up as it returned from Settings or Projects
        // (`resume_repository_page`).
        let document_restore = self.pending_tab_documents().cloned();
        self.repository_tabs.document_restore = None;
        // The comparison stays warm with the tab for its return, so a review
        // being prepared for it ends here with its read.
        self.release_text_review(cx);
        self.invalidate_read();
        self.worker.release_history();
        self.work_generation = self.work_generation.wrapping_add(1);
        self.status_task = None;
        // A working read the tab asked for while a write or preparation ran
        // is dropped with its rows' reads: restored, it reads quietly.
        self.working_read_deferred = None;
        // Every review the tab was preparing is dropped, the window busy
        // until its read ends on the executor every tab shares.
        self.end_preparations(crate::page_navigation::PageSwitch::Tab, window, cx);
        self.file_history.pause_for_tab();
        self.revision_inspection.pause_for_tab();
        self.blame.pause_for_tab();
        pdf_view::pause(self.content.as_deref());
        model_view::pause(self.content.as_deref());
        markdown_view::pause(self.content.as_deref());
        self.image_drag = None;
        let retained_commit = self.automatic.retained_commit;
        // Goes with the error slot the tab retains below.
        let local_refresh_failure = self.automatic.take_failure();
        self.automatic.reset();
        // The tab keeps the shown rows with their counts. Rows kept from an
        // earlier status while a newer one's were prepared index a status
        // the tab does not keep, so then it keeps none.
        let working_counts = if self.working_paths.retains_rows() {
            self.working_rows.clear();
            Default::default()
        } else {
            self.working_paths.counts()
        };
        let working_unprepared = self.working_paths.unprepared();
        self.working_paths.reset();
        // A row's travel belongs to the shown list: it neither waits in the
        // retained tab nor lands when the tab returns.
        self.working_travel = Default::default();
        self.file_paths.reset();
        let mut context = self.take_inspection_context(window, cx);
        context.pause_for_tab();
        let warm = WarmTab {
            bytes,
            repository: repo,
            scope: self.scope.take(),
            context,
            file_history: std::mem::take(&mut self.file_history),
            inspections: std::mem::take(&mut self.revision_inspection),
            blame: std::mem::take(&mut self.blame),
            search: std::mem::take(&mut self.history_search),
            paging: self.history_paging.take_settled(),
            history_updates: std::mem::take(&mut self.history_updates),
            inputs: TabInputs::capture(self.tab_inputs(), window, cx),
            branches: std::mem::take(&mut self.branches),
            worktrees: std::mem::take(&mut self.worktrees),
            nav_rows: std::mem::take(&mut self.nav_rows),
            nav_ids: std::mem::take(&mut self.nav_ids),
            nav_mode: self.nav_mode,
            nav_cursor: self.nav_cursor,
            expanded: std::mem::take(&mut self.expanded_folders),
            commits: std::mem::take(&mut self.commits),
            visible: std::mem::take(&mut self.visible),
            graph: std::mem::take(&mut self.graph),
            graph_lanes: self.graph_lanes,
            graph_offset: self.graph_offset,
            graph_notice: self.graph_notice.take(),
            refs: std::mem::take(&mut self.refs),
            selected_commit: self.selected_commit.take(),
            parent: self.parent,
            retained_commit,
            history_scroll: std::mem::take(&mut self.history_scroll),
            file_scroll: std::mem::take(&mut self.file_scroll),
            history_list_layout: self.history_list_layout.take(),
            working_layout: self.working_layout.take(),
            file_list_layout: self.file_list_layout.take(),
            nav_scroll: std::mem::take(&mut self.nav_scroll),
            horizontal: std::mem::take(&mut self.history_horizontal),
            content_panels: std::mem::replace(
                &mut self.content_panels,
                cx.new(|_| ResizableState::default()),
            ),
            history_panels: std::mem::replace(
                &mut self.history_panels,
                cx.new(|_| ResizableState::default()),
            ),
            retained_history_files: self.retained_history_files.take(),
            work_status: self.work_status.take(),
            working_identity: self.working_identity.take(),
            integration_state: self.integration_state.take(),
            profile: self.profile.take(),
            remotes: std::mem::take(&mut self.remotes),
            working_rows: std::mem::take(&mut self.working_rows),
            working_counts,
            working_unprepared,
            working_selected: self.working_selected.take(),
            working_scroll: std::mem::take(&mut self.working_scroll),
            working_selection: std::mem::take(&mut self.working_selection),
            operation_notice: self.operation_notice.take(),
            operation_error: self.operation_error.take(),
            local_refresh_failure,
            document_restore,
        };
        self.repository_tabs.tabs[index].saved = bookmark;
        self.repository_tabs.tabs[index].warm = (!cold).then_some(warm);
        self.repository = None;
        self.path = None;
        self.observe_tab_panels(cx);
        true
    }

    /// Leaves the shown tab before its repository is read, as a cold tab
    /// whose snapshot is still being read: the tab keeps the bookmark it is
    /// restored from, and its read is cancelled. Left to land, that
    /// snapshot reached the tab shown in its place, which took it for an
    /// alias of the left tab, closed itself and showed the left tab again.
    fn leave_unread_tab(&mut self) {
        let restoring = self.repository_tabs.restoring.take();
        let Some(index) = self.repository_tabs.active else {
            return;
        };
        if let (Some(saved), Some(tab)) = (restoring, self.repository_tabs.tabs.get_mut(index)) {
            tab.saved = saved;
        }
        self.invalidate_read();
    }
    /// Ends a cold tab's restore as a read the person asks for in the tab
    /// takes the place of the restore's own: every foreground read starts
    /// here (`pause_history_search_for_read`, and a search's next page),
    /// before it drops the read in flight, which while the tab is restored
    /// is the restore's. The restore then applies none of its remaining
    /// stages, and the tab is left warm like any other. Its steps ask for
    /// their reads once the previous one has landed, so none ends it, and
    /// its read of a saved merge parent's changes, which replaces its own
    /// first, sets the bookmark aside meanwhile; a read of its own that
    /// fails or is dropped ends it as it ends (`request`, the search's
    /// reply). Decision (2026-11-03): a tab is left cold only while its restore
    /// still waits for its own read. Left waiting for a read it would never
    /// get, the tab was left cold and dropped what the person did in it,
    /// such as a Quick Open source, and a reply in place of the restore's,
    /// such as another commit's changed files, took the saved selection.
    /// History's search asked for while the snapshot was read goes with it:
    /// the read is the person's later action, which wins (decision
    /// 2026-11-05). Held, the search waited for a Compare the restore no
    /// longer reached.
    pub(super) fn end_replaced_restore(&mut self) {
        if self.repository_tabs.restoring.is_some()
            && (self.task.is_some() || self.history_search_pending())
        {
            self.repository_tabs.restoring = None;
            self.repository_tabs.drop_pending_search();
        }
    }
    fn restore_warm_tab(&mut self, warm: WarmTab, window: &mut Window, cx: &mut Context<Self>) {
        self.repository_tabs.switching = true;
        self.path = Some(warm.repository.path().to_owned());
        // A wait for a preparation a tab dropped shows its own label in
        // that tab and says where it runs in another.
        self.hold_dropped_preparation();
        self.repository = Some(warm.repository);
        self.scope = warm.scope;
        self.file_history = warm.file_history;
        self.revision_inspection = warm.inspections;
        self.blame = warm.blame;
        self.history_search = warm.search;
        self.history_paging = warm.paging;
        self.history_updates = warm.history_updates;
        self.branches = warm.branches;
        self.worktrees = warm.worktrees;
        self.nav_rows = warm.nav_rows;
        self.nav_ids = warm.nav_ids;
        self.nav_mode = warm.nav_mode;
        self.nav_cursor = warm.nav_cursor;
        self.expanded_folders = warm.expanded;
        self.seed_folders = false;
        self.commits = warm.commits;
        self.visible = warm.visible;
        self.graph = warm.graph;
        self.graph_lanes = warm.graph_lanes;
        self.graph_offset = warm.graph_offset;
        self.graph_notice = warm.graph_notice;
        self.refs = warm.refs;
        self.selected_commit = warm.selected_commit;
        self.parent = warm.parent;
        self.automatic.retained_commit = warm.retained_commit;
        self.history_scroll = warm.history_scroll;
        self.file_scroll = warm.file_scroll;
        self.history_list_layout = warm.history_list_layout;
        self.working_layout.set(warm.working_layout);
        self.file_list_layout = warm.file_list_layout;
        self.nav_scroll = warm.nav_scroll;
        self.history_horizontal = warm.horizontal;
        self.content_panels = warm.content_panels;
        self.history_panels = warm.history_panels;
        self.retained_history_files = warm.retained_history_files;
        self.work_status = warm.work_status;
        self.working_identity = warm.working_identity;
        // What a read of another tab failed with is not this one's to say
        // (`GitTurtle::working_read_failure`).
        self.working_read_failure = None;
        self.integration_state = warm.integration_state;
        self.profile = warm.profile;
        self.remotes = warm.remotes;
        self.working_rows = warm.working_rows;
        self.working_paths.restore_counts(warm.working_counts);
        self.working_selected = warm.working_selected;
        self.working_scroll = warm.working_scroll;
        self.working_selection = warm.working_selection;
        self.operation_notice = warm.operation_notice;
        self.operation_error = warm.operation_error;
        self.automatic.restore_failure(warm.local_refresh_failure);
        warm.inputs.restore(self.tab_inputs(), window, cx);
        self.observe_tab_panels(cx);
        if let Some(index) = self.repository_tabs.active {
            let saved = self.repository_tabs.tabs[index].saved.clone();
            self.restore_tab_widths(&saved);
        }
        self.page = AppPage::Repository;
        self.forget_page_return();
        self.repository_tabs.drop_pending_search();
        self.restore_inspection_context(warm.context, window, cx);
        if let Some(saved) = warm.document_restore {
            self.resume_tab_documents(saved, window, cx);
        }
        // A preview that landed while Settings or Projects showed, before
        // the tab was left, draws its editor now, as Back builds it
        // (`resume_repository_page`): restored unbuilt, it read "Loading
        // text…" until its next read.
        if self.mode != WorkspaceMode::History {
            self.prepare_editor(false, window, cx);
        }
        // The tab's focus goes back to the control it was left with as the
        // frame that draws the tab is built (`return_tab_focus`), so that
        // frame draws it focused. Until then application shortcuts stay
        // attached through the window's root: the control may not be in the
        // tree the last tab drew, where a key would reach no shortcut.
        // Quick source's read that landed so owes nothing, and its text
        // takes the focus its pane held as the window left, as Back gives
        // it (`quick_source_return_focus`); where nothing is focused, the
        // tab's fallback is that pane's (`files_focus`). Settled with the
        // text's handle thrown away, the tab gave focus to the information
        // where Back gave it to the text.
        let focus = match window.focused(cx) {
            Some(focus) => Some(self.quick_source_return_focus(focus, cx)),
            None => self.settle_quick_source_landing(cx),
        };
        self.repository_tabs.focus_return = Some(FocusReturn {
            page: AppPage::Repository,
            path: self.path.clone(),
            held: self.app_focus.clone(),
            focus,
        });
        self.app_focus.focus(window, cx);
        self.repository_tabs.switching = false;
        if let Some(path) = self.path.clone() {
            self.branch_input_scope.opened(&path);
            self.restore_commit_draft(path, window, cx);
        }
        self.resume_file_history(window, cx);
        self.resume_tab_preview(window, cx);
        self.ensure_local_watcher(true, window, cx);
        self.queue_automatic_refresh(
            local_refresh::LocalChange {
                rescan: true,
                ..Default::default()
            },
            window,
            cx,
        );
        // The rows come back as the tab left them, scrolled where they
        // were with their selection and cursor; only rows it left being
        // prepared, or refused by the filter, are prepared again, the shown
        // ones kept in place until they land. Prepared again as a query
        // edit's are (`filter_working_paths`), which clears the selection
        // and scrolls to the top, a Changes tab once came back at its first
        // row with nothing selected.
        if let Some(preparation) = warm.working_unprepared {
            self.schedule_working_filter(None, false, None, preparation, window, cx);
        }
        self.hub.update(cx, |hub, cx| {
            hub.set_can_go_back(true, cx);
            hub.set_busy(false, cx);
        });
        // One whole frame draws the tab, its focus, rows and lanes settled
        // in it. `repaint_page` asked for one more at the next frame, which
        // drew the same tab again.
        cx.notify();
        window.refresh();
    }

    /// Gives a warm tab shown again its focus back, as the window builds the
    /// frame that draws the tab and before anything in it reads focus: the
    /// control the tab was left with, which that frame draws focused, so no
    /// frame follows to show it. Focus moved while a frame is drawn asks for
    /// no frame of its own. The window's render calls it after its other
    /// focus repairs. A newer focus change, another repository or page shown,
    /// or a modal open since the switch supersedes the return
    /// (`restore_warm_tab` left focus on the window's root).
    ///
    /// Whether the control is in the tab's tree is known only once the frame
    /// is built: one that has gone, as a tab button since closed, gives way
    /// as the frame ends to the shown mode's list, or the window's root, and
    /// that is drawn in a frame of its own. The switch once gave focus back
    /// only two frames after it drew the tab, from frame callbacks, and drew
    /// a frame to show it there too. A cold restore landing in Compare keeps
    /// the person's control the same way, focus held on it rather than on
    /// the root.
    pub(super) fn return_tab_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(FocusReturn {
            page,
            path,
            held,
            focus,
        }) = self.repository_tabs.focus_return.take()
        else {
            return;
        };
        if self.page != page
            || self.path != path
            || !held.is_focused(window)
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        // The shown mode's list, focused with the pane it is, as choosing a
        // commit or a file names it: focused alone, the window went on
        // naming the pane it named before, which a File history or Blame
        // opened from there keeps and returns focus to. Projects has its
        // first stop instead, and no pane to name.
        let (fallback, pane) = if page == AppPage::Projects {
            (self.hub.read(cx).selected_mode_focus().clone(), self.pane)
        } else if self.mode == WorkspaceMode::History {
            (self.focus.clone(), Pane::History)
        } else {
            (self.files_focus().clone(), Pane::Files)
        };
        let focus = focus.unwrap_or_else(|| {
            self.pane = pane;
            fallback.clone()
        });
        focus.focus(window, cx);
        // A modal opened before the next frame returns here, not to the root
        // this frame's `update_modal_focus` saw focused: one opened by its
        // keys after a tab shown by its keys, since only a key that follows
        // the pointer draws a frame before it acts.
        self.modal_return_focus = Some(focus.clone());
        cx.defer_in(window, move |this, window, cx| {
            if this.page != page
                || this.path != path
                || !focus.is_focused(window)
                || this.app_focus.contains(&focus, window)
            {
                return;
            }
            if this.app_focus.contains(&fallback, window) {
                this.pane = pane;
                fallback.focus(window, cx);
            } else {
                this.app_focus.focus(window, cx);
            }
        });
    }

    /// Gives the repository page its focus back as the window returns to it
    /// from Settings or Projects (`resume_repository_page`): `focus`, the
    /// control it was left with, held as a warm tab's is, so a control the
    /// frame that draws the page draws nowhere in the tab gives way as that
    /// frame ends to the shown mode's list (`return_tab_focus`). A cold
    /// restore landing in Compare under the other page left focus on a
    /// control Compare takes away, such as the navigation's filter focused
    /// while the snapshot was read, and Back gave focus back to it, where no
    /// key reached the window's shortcuts until a click.
    pub(super) fn give_page_focus_back(
        &mut self,
        focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        focus.focus(window, cx);
        self.repository_tabs.focus_return = Some(FocusReturn {
            page: AppPage::Repository,
            path: self.path.clone(),
            held: focus.clone(),
            focus: Some(focus),
        });
    }

    /// Shows Projects in place of a repository page left with nothing to
    /// show, an Open having failed with no repository in the window: a
    /// marked tab restored cold, at launch or as it is switched to. The page
    /// starts on its first stop, the selected action tab drawn with its
    /// ring, as every other way onto it does (`show_projects`). Focus stayed
    /// on History's list, which Projects does not draw, so the page showed
    /// none and a screen reader was left on the window itself.
    ///
    /// Focus the person moved while the tab was read is theirs, as at a
    /// restore that lands in Compare ([`Self::land_restore_focus`]): a dialog
    /// or sheet keeps it and its close lands on that first stop, and a
    /// control keeps it only where the frame that draws the page draws it
    /// too, as the tab strip's Menu; one it draws nowhere gives way to the
    /// first stop as that frame ends ([`Self::return_tab_focus`]).
    pub(super) fn show_projects_for_failed_open(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if std::mem::replace(&mut self.page, AppPage::Projects) == AppPage::Projects {
            return;
        }
        let first_stop = self.hub.read(cx).selected_mode_focus().clone();
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            self.return_modal_to(Some(first_stop));
            return;
        }
        match (window.focused(cx)).filter(|_| !self.restore_focus_stranded(window, cx)) {
            Some(focus) => {
                self.repository_tabs.focus_return = Some(FocusReturn {
                    page: AppPage::Projects,
                    path: self.path.clone(),
                    held: focus.clone(),
                    focus: Some(focus),
                });
            }
            None => {
                first_stop.focus(window, cx);
                self.hub.update(cx, |hub, _| hub.opened_on_first_stop());
            }
        }
    }
}

/// Whether two opens found the same Git and common directories. At the
/// same paths, where both captured a directory's inode, the inodes match: a
/// directory replaced at the same path, as a re-clone there leaves, is
/// another repository. At other paths, as a folder above the root
/// re-pointed leaves them (`worker::FoldersAbove`), only the inodes tell,
/// so both must have captured them and they must match: the folder is the
/// same repository only if its Git and common directories are, moved and
/// linked back. Either way, where both opens read a directory's birth time
/// (`GitRepository::git_dir_birth`), it must match too: ext4 and XFS can
/// give a directory created after a delete the inode number of the one
/// deleted. Where either read none, the inode alone tells, so a birth time
/// unknown on both sides never makes two directories the same where an
/// inode is unknown too. A found open in the running session holds the
/// directory's birth time as it is now: the worker drops its retained
/// repository once the birth time changes (`worker::RepositoryIdentity`).
/// Reads what the opens captured, so it touches no filesystem.
fn same_git_directories(shown: &worker::GitDirectories, found: &worker::GitDirectories) -> bool {
    let (shown_inodes, found_inodes) = (shown.inodes, found.inodes);
    let born_together =
        |shown: Option<std::time::Duration>, found| shown.zip(found).is_none_or(|(a, b)| a == b);
    let births = born_together(shown_inodes.git_dir_birth, found_inodes.git_dir_birth)
        && born_together(shown_inodes.common_birth, found_inodes.common_birth);
    if shown.git_dir == found.git_dir && shown.common == found.common {
        let same =
            |shown: Option<u64>, found: Option<u64>| shown.zip(found).is_none_or(|(a, b)| a == b);
        births
            && same(shown_inodes.git_dir, found_inodes.git_dir)
            && same(shown_inodes.common, found_inodes.common)
    } else {
        let known = |shown: Option<u64>, found: Option<u64>| shown.is_some() && shown == found;
        births
            && known(shown_inodes.git_dir, found_inodes.git_dir)
            && known(shown_inodes.common, found_inodes.common)
    }
}

impl GitTurtle {
    /// Whether `found`, the repository a read the window accepts found at
    /// the shown tab (an Open's or Refresh's snapshot, Show latest's, a
    /// quiet read's), is another than the one the shown working status and
    /// remotes were read from (`GitTurtle::working_identity`), or, with none
    /// read yet, than the window's handle ([`same_git_directories`]). Such a
    /// read is a fresh capture: what the window read of the other goes
    /// (`GitTurtle::forget_repository_state`) before the found one's status
    /// read starts. Compared with the handle alone, it missed a re-clone
    /// that Show latest or a quiet read had installed already, whose handle
    /// it compared with itself while the status and remotes shown were the
    /// replaced one's, and a folder above the root re-pointed, whose paths
    /// it took to be another tab's open (security probes S1 to S3, FU44-A).
    pub(super) fn shows_another_repository(&self, found: &GitRepository) -> bool {
        let shown = (self.working_identity.clone())
            .or_else(|| self.repository.as_ref().map(worker::GitDirectories::of));
        shown.is_some_and(|shown| !same_git_directories(&shown, &worker::GitDirectories::of(found)))
    }
    fn tab_inputs(&self) -> [&Entity<InputState>; 6] {
        [
            &self.search,
            &self.nav_search,
            &self.working_filter,
            &self.branch_name,
            &self.remote_name,
            &self.remote_branch,
        ]
    }
    fn clear_tab_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let switching = self.repository_tabs.switching;
        self.repository_tabs.switching = true;
        TabInputs::default().restore(self.tab_inputs(), window, cx);
        self.repository_tabs.switching = switching;
    }
    pub(super) fn open_repository_tab(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.repository_tabs.find(&path) {
            if self.repository_tabs.tabs[index].error.is_some() {
                // A fresh capture forgets what the window read, so it waits
                // for a running write in the words the Projects page and the
                // palette give it, as the Projects and pane rows refuse it
                // (`project_event`, `open_project_from_pane`). Open…, whose
                // folder is chosen after the write may have started, once
                // captured the tab, and the write, failing safe, named the
                // replaced repository on the fresh tab.
                if self.operation_busy.is_some() {
                    self.refuse_where_asked(self.busy_reason(), cx);
                    return;
                }
                self.recapture_marked_tab(index, path, window, cx);
            } else {
                self.switch_repository_tab(index, window, cx);
            }
        } else {
            self.limit = 500;
            self.open(path, None, window, cx);
        }
    }
    /// An explicit Open of a marked tab's folder reads it afresh
    /// ([`Recapture`]): once accepted the tab takes the repository found and
    /// its mark clears. It once reopened the tab by its old identity and was
    /// refused again, which only closing the tab recovered from. Shown
    /// already, the tab opens in place, as switching to it opens nothing.
    /// From another tab, what the tab retained goes, so its switch opens it
    /// cold: restored as retained, it once opened nothing. A refused switch
    /// leaves the tab as it was, what it retained too.
    fn recapture_marked_tab(
        &mut self,
        index: usize,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.repository_tabs.recapture = Some(Recapture::Asked(path.clone()));
        if self.repository_tabs.active == Some(index) {
            self.limit = 500;
            self.open(path, None, window, cx);
        } else {
            let warm = self.repository_tabs.tabs[index].warm.take();
            self.switch_repository_tab(index, window, cx);
            if self.repository_tabs.active == Some(index) {
                // Its repository handle may hold the last reference to an
                // object reader, which stops its process as it drops.
                if let Some(WarmTab { repository, .. }) = warm {
                    cx.background_spawn(async move { drop(repository) })
                        .detach();
                }
            } else if let Some(tab) = self.repository_tabs.tabs.get_mut(index) {
                // Refused where it was asked, which ends the wait of a row
                // pressed on the Projects page ([`Self::refuse_where_asked`]).
                tab.warm = warm;
            }
        }
        if matches!(self.repository_tabs.recapture, Some(Recapture::Asked(_))) {
            self.repository_tabs.recapture = None;
        }
    }
    pub(super) fn tab_open_requested(
        &mut self,
        path: &Path,
        scope: &Option<(String, worker::Scope)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.repository_tabs.switching {
            return false;
        }
        if let Some(index) = self.repository_tabs.find(path) {
            if self.repository_tabs.active != Some(index)
                || self
                    .repository
                    .as_ref()
                    .is_none_or(|repo| repo.path() != path)
            {
                self.switch_repository_tab(index, window, cx);
                return true;
            }
            return false;
        }
        if self
            .repository
            .as_ref()
            .is_some_and(|repo| repo.path() == path)
            && scope.is_some()
        {
            return false;
        }
        if self.repository_tabs.tabs.len() >= MAX_TABS {
            self.refuse_where_asked(TAB_LIMIT_REFUSAL, cx);
            return true;
        }
        if !self.retain_active_tab(window, cx) {
            return true;
        }
        self.repository_tabs.refusal = None;
        let index = self.repository_tabs.tabs.len();
        // The folder the person chose: its open may land on the work tree
        // enclosing it, whose root the tab keeps once it is accepted.
        self.repository_tabs.tabs.push(Tab {
            path: path.to_owned(),
            resolved: false,
            git_dir: None,
            saved: Bookmark::default(),
            warm: None,
            error: None,
        });
        self.repository_tabs.active = Some(index);
        self.clear_tab_inputs(window, cx);
        false
    }
    pub(super) fn tab_snapshot_accepted(
        &mut self,
        repo: &GitRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // A fresh capture's snapshot replaces what the tab had captured.
        let afresh = matches!(
            self.repository_tabs.recapture,
            Some(Recapture::Reading(generation)) if generation == self.generation
        );
        // So is one that finds another repository than the shown status and
        // remotes were read from, as a re-clone at the tab's path or a
        // folder above it re-pointed leaves: an Open or Refresh in place
        // kept what was read of the replaced one, so until the status read
        // landed Push pushed the found repository's branch to its own
        // remote while Targets named the other's (security probe N1,
        // FU39-A5; S2 and S3, FU44-A).
        let replaced = self.shows_another_repository(repo);
        self.repository_tabs.recapture = None;
        if let Some(existing) = self.repository_tabs.find(repo.path())
            && self.repository_tabs.active != Some(existing)
        {
            let mut existing = existing;
            if let Some(pending) = self.repository_tabs.active.take() {
                self.repository_tabs.tabs.remove(pending);
                if pending < existing {
                    existing -= 1;
                }
            }
            self.path = None;
            self.repository = None;
            self.switch_repository_tab(existing, window, cx);
            return true;
        }
        if let Some(index) = self.repository_tabs.active
            && let Some(tab) = self.repository_tabs.tabs.get_mut(index)
        {
            // The root discovery found is the tab's repository from now on:
            // a later passive reopen finds it again or shows it unavailable.
            tab.path = repo.path().to_owned();
            tab.resolved = true;
            tab.git_dir = Some(worker::GitDirectories::of(repo));
            tab.error = None;
        }
        // Capture after receive installs the accepted metadata. A newly opened
        // active tab may never be switched or otherwise trigger a session save.
        // Cold bookmarks finish restoration before taking their replacement.
        if self.repository_tabs.restoring.is_none() {
            self.save_repository_session(window, cx);
        }
        // What the window read of the Git directory the tab had goes before
        // the status read of the one found starts, as an Open of another
        // path forgets it: the same path opened again in place kept it, so
        // until that read landed the header named the other branch, Push
        // and Fetch went to its remote's name, and Commit committed an
        // index no read had shown under the title typed for the other one.
        if afresh || replaced {
            self.forget_repository_state(window, cx);
        }
        false
    }
    /// Marks the shown tab with the error its repository's own read, an
    /// Open, failed with, so its label reads " · unavailable" until a
    /// snapshot of it is accepted ([`Self::tab_snapshot_accepted`]), as the
    /// tab is opened again. The label speaks for the repository, so no other
    /// read marks it: a preview's or a review's failure belongs to the
    /// comparison and says so in its pane. Marked by every failed read, a
    /// review past its limits left the tab "unavailable" after Reset review
    /// brought the diff back, until the repository was opened again (QA-101
    /// O1).
    ///
    /// A fresh capture of a marked tab's folder that fails ([`Recapture`])
    /// leaves the tab its root, Git directory and mark as they were, the
    /// failure said as an Open's. So does an Open that `could_not_check` its
    /// folder ([`gitturtle_core::CouldNotCheck`]): a lost permission or a
    /// volume that stopped answering says nothing of the repository, so it
    /// marks nothing new and leaves a mark as it was, and the next read
    /// that finds the folder decides ([`Self::tab_found_readable`]).
    pub(super) fn tab_opening_failed(&mut self, could_not_check: bool) {
        let afresh = matches!(
            self.repository_tabs.recapture,
            Some(Recapture::Reading(generation)) if generation == self.generation
        );
        if let Some(index) = self.repository_tabs.active
            && let Some(tab) = self.repository_tabs.tabs.get_mut(index)
        {
            if (afresh && tab.error.is_some()) || could_not_check {
                self.repository_tabs.recapture =
                    self.error.clone().map(|error| Recapture::Failed {
                        path: tab.path.clone(),
                        error,
                    });
            } else {
                tab.error = self.error.clone();
            }
        }
    }
    /// Refuses what the tab strip was asked, a ninth tab, a switch past the
    /// retained allowance, closing a tab while its write runs or a library
    /// past its bound, saying why beneath the strip, where the person
    /// acted, as an alert heard at once and again for each refusal. A
    /// refusal never replaces what the window shows: set as the window's
    /// error, it took the place of the diff being read ("Preview
    /// unavailable") and of History's list. The line stays until
    /// dismissed or until a tab opens, closes or is shown.
    pub(super) fn refuse_in_tab_strip(
        &mut self,
        reason: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.repository_tabs.refusal = Some(reason.into());
        self.repository_tabs.refusals = self.repository_tabs.refusals.wrapping_add(1);
        cx.notify();
    }
    /// Refuses an Open or a switch where the person asked for it: an Open
    /// from the Projects page, a recent project's row among them, in that
    /// page's feedback line, under the row, which also ends the Open the hub
    /// waits for (its actions stayed refused, "Opening project…"), and any
    /// other in the tab strip's ([`Self::refuse_in_tab_strip`]), a Clone's
    /// or a Create's wait going on (`ProjectHub::refuse_open`). Neither
    /// replaces what the window shows, as the window's error did. A row's
    /// switch past the retained allowance was said in the tab strip, and a
    /// whole tab's left the page waiting.
    pub(super) fn refuse_where_asked(&mut self, reason: &str, cx: &mut Context<Self>) {
        let from_projects = self.page == AppPage::Projects
            && self.hub.update(cx, |hub, cx| hub.refuse_open(reason, cx));
        if from_projects {
            // One refusal line at a time: an earlier strip refusal goes.
            self.repository_tabs.refusal = None;
        } else {
            self.refuse_in_tab_strip(reason.to_owned(), cx);
        }
    }
    /// At the tab limit, refuses in the tab strip before a folder is asked
    /// for, in the words the limit refuses a chosen folder with, and returns
    /// true: every route into [`GitTurtle::choose_repository`] (the Open and
    /// New Tab shortcuts, the menu, the palette, the strip's + and the
    /// rail's folder). Asked first, a person chose a folder only to be told
    /// that no tab could hold it.
    pub(super) fn refuse_picker_at_tab_limit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.repository_tabs.tabs.len() < MAX_TABS {
            return false;
        }
        self.refuse_in_tab_strip(TAB_LIMIT_REFUSAL, cx);
        true
    }
    /// The Projects page's own folder picker, or, at the tab limit, the
    /// limit's refusal in that page's feedback line, where the person
    /// pressed it, as a recent project's Open is refused
    /// ([`Self::tab_open_requested`]).
    pub(super) fn choose_project_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.repository_tabs.tabs.len() < MAX_TABS {
            self.hub
                .update(cx, |hub, cx| hub.choose_folder(true, window, cx));
            return;
        }
        // One refusal line at a time: an earlier strip refusal goes.
        self.repository_tabs.refusal = None;
        self.hub.update(cx, |hub, cx| {
            hub.set_error(Some(TAB_LIMIT_REFUSAL.to_owned()), cx)
        });
    }
    /// The tab strip's refusal line ([`Self::refuse_in_tab_strip`]) with its
    /// Dismiss, drawn beneath the strip.
    fn render_tab_refusal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let refusal = self.repository_tabs.refusal.clone()?;
        let colors = palette(cx);
        Some(
            div()
                .debug_selector(|| "repository-tab-refusal-row".into())
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_1()
                .bg(rgb(colors.removed_background))
                .border_b_1()
                .border_color(rgb(colors.border))
                .child(crate::native_accessibility::refusal_line(
                    "repository-tab-refusal",
                    self.repository_tabs.refusals,
                    refusal,
                    |line| {
                        line.flex_1()
                            .min_w_0()
                            .text_size(appearance::ui_text(12.))
                            .text_color(rgb(colors.removed))
                    },
                ))
                .child(
                    button("dismiss-tab-refusal", "Dismiss", "", false)
                        .debug_selector(|| "dismiss-tab-refusal".into())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.repository_tabs.refusal = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
    /// Whether the shown tab is marked " · unavailable", its Open having
    /// failed ([`Self::tab_opening_failed`]).
    pub(super) fn shown_tab_unavailable(&self) -> bool {
        self.repository_tabs
            .active
            .and_then(|index| self.repository_tabs.tabs.get(index))
            .is_some_and(|tab| tab.error.is_some())
    }
    /// Whether the shown tab has no History for want of a read: its Open
    /// failed, marking it or, unable to check its folder, not
    /// ([`Self::tab_opening_failed`]).
    pub(super) fn shown_tab_unread(&self) -> bool {
        self.shown_tab_unavailable()
            || self
                .repository_tabs
                .active
                .and_then(|index| self.repository_tabs.tabs.get(index))
                .is_some_and(|tab| {
                    matches!(
                        &self.repository_tabs.recapture,
                        Some(Recapture::Failed { path, .. }) if *path == tab.path
                    )
                })
    }
    /// The working status the header and the Git action bar speak from: none
    /// where the shown tab is marked, whose count, branch, edge and sync
    /// state would be those of the last status read, or "Detached HEAD" where
    /// none was. Those bars say what the mark says instead. Read each frame
    /// rather than cleared at the mark, since a mark goes on and off without
    /// a status read, and a retained status read may still land behind it.
    pub(crate) fn header_status(&self) -> Option<&gitturtle_core::RepositoryStatus> {
        self.work_status
            .as_deref()
            .filter(|_| !self.shown_tab_unavailable())
    }
    /// Clears the shown tab's " · unavailable" as a quiet read finds its
    /// repository readable again, with the opening error the window shows
    /// for it, and returns whether there was a mark to clear. A tab's mark
    /// states the repository's current state, so the read the watcher asks
    /// for clears it as an explicit Refresh's accepted snapshot does
    /// ([`Self::tab_snapshot_accepted`]); one that still fails keeps it.
    /// Cleared only by Refresh, a repository moved back or repaired outside
    /// the app stayed "unavailable" while the window read it.
    pub(super) fn tab_found_readable(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self
            .repository_tabs
            .active
            .and_then(|index| self.repository_tabs.tabs.get_mut(index))
        else {
            return false;
        };
        let path = tab.path.clone();
        let mark = tab.error.take();
        // A failed fresh capture of its folder, or an Open that could not
        // check it, said its own failure in place of the mark's reason
        // ([`Recapture::Failed`]), which goes with it; another tab's stays
        // for that tab.
        let failed = match self.repository_tabs.recapture.take_if(|recapture| {
            matches!(recapture, Recapture::Failed { path: failed, .. } if *failed == path)
        }) {
            Some(Recapture::Failed { error, .. }) => Some(error),
            _ => None,
        };
        if mark.is_none() && failed.is_none() {
            return false;
        }
        for error in mark.into_iter().chain(failed) {
            if self.error.as_ref() == Some(&error) {
                self.error = None;
            }
            // The Projects page's error is the same opening error: left,
            // closing the last tab met the person with an alert that the
            // repository it had just read was not available.
            self.hub
                .update(cx, |hub, cx| hub.clear_error_matching(&error, cx));
        }
        true
    }
    /// Gives the shown tab's mark the reason its repository cannot be opened
    /// now, as a Changes read that failed found it
    /// (`worker::MarkedReadFailed`), the window with it and the Projects
    /// page where it says the old one, and returns whether there was a mark
    /// to restate. Only the words change: the tab keeps its root and Git
    /// directory, and no other repository is opened or accepted in it. An
    /// explicit Refresh there once kept the mark's old reason after Git
    /// started finding another repository.
    pub(super) fn tab_mark_restated(&mut self, reason: String, cx: &mut Context<Self>) -> bool {
        let Some(previous) = self
            .repository_tabs
            .active
            .and_then(|index| self.repository_tabs.tabs.get_mut(index))
            .and_then(|tab| {
                tab.error
                    .as_mut()
                    .map(|error| std::mem::replace(error, reason.clone()))
            })
        else {
            return false;
        };
        self.error = Some(reason.clone());
        self.hub.update(cx, |hub, cx| {
            hub.replace_error_matching(&previous, reason, cx)
        });
        true
    }
    /// A read of the shown tab's repository found it gone from its root
    /// (`worker::RepositoryGone`): Git finds another repository there, the
    /// work tree enclosing it once its `.git` went, or none. The tab is read
    /// again as Refresh reads it, a passive reopen that finds the repository
    /// gone too, so the failed Open marks it " · unavailable" with the
    /// reason, History says "Could not open repository", and the saved
    /// session keeps the tab's path ([`Self::tab_opening_failed`]); it never
    /// shows the other repository. A tab marked already keeps its mark, so
    /// the reads the watcher asks for meanwhile ask for no further Open.
    pub(super) fn tab_repository_gone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown_tab_unavailable() {
            return;
        }
        if let Some(path) = self.path.clone() {
            self.open(path, self.scope.clone(), window, cx);
        }
    }
    pub(super) fn switch_repository_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.repository_tabs.tabs.len()
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        if self.repository_tabs.active == Some(index)
            && self
                .repository
                .as_ref()
                .is_some_and(|repo| repo.path() == self.repository_tabs.tabs[index].path)
        {
            // Back from another page, it takes up the repository page as Back
            // does (`return_from_page`): File history opened as the window
            // left came back without its selected revision, and quiet reads
            // held meanwhile, a write's reload among them, waited for the
            // next local change, every menu that follows writes refused.
            if std::mem::replace(&mut self.page, AppPage::Repository) != AppPage::Repository {
                self.resume_repository_page(window, cx);
            } else {
                self.try_automatic_refresh(window, cx);
            }
            self.repaint_page(window, cx);
            return;
        }
        if !self.retain_active_tab(window, cx) {
            return;
        }
        self.repository_tabs.refusal = None;
        self.repository_tabs.active = Some(index);
        let path = self.repository_tabs.tabs[index].path.clone();
        if let Some(warm) = self.repository_tabs.tabs[index].warm.take() {
            self.restore_warm_tab(warm, window, cx);
        } else {
            self.clear_tab_inputs(window, cx);
            let saved = self.repository_tabs.tabs[index].saved.clone();
            self.restore_tab_widths(&saved);
            let scope = saved
                .scope
                .as_ref()
                .map(|(label, scope)| (label.clone(), scope.scope()));
            let restore = saved.selected_oid.is_some()
                || saved.comparison.is_some()
                || saved.offset > 0
                || !saved.query.is_empty();
            self.repository_tabs.restoring = restore.then_some(saved);
            self.repository_tabs.switching = true;
            self.open(path, scope, window, cx);
            self.repository_tabs.switching = false;
        }
        self.save_repository_session(window, cx);
    }
    pub(super) fn cycle_repository_tab(
        &mut self,
        direction: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.repository_tabs.tabs.len();
        if count == 0 {
            return;
        }
        let current = self.repository_tabs.active.unwrap_or(0);
        let next = (current as i32 + direction).rem_euclid(count as i32) as usize;
        self.switch_repository_tab(next, window, cx);
    }
    pub(super) fn move_repository_tab(
        &mut self,
        direction: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some(index) = self.repository_tabs.active else {
            return;
        };
        let next = (index as i32 + direction)
            .clamp(0, self.repository_tabs.tabs.len().saturating_sub(1) as i32)
            as usize;
        self.repository_tabs.reorder(index, next);
        self.save_repository_session(window, cx);
        cx.notify();
    }
    pub(super) fn close_repository_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some(tab) = self.repository_tabs.tabs.get(index) else {
            return;
        };
        if self.operation_busy.is_some()
            && self.operation_repository.as_deref() == Some(tab.path.as_path())
        {
            let reason = format!(
                "{} is running in {}. Wait for it to finish or use its Cancel action before closing this tab.",
                self.operation_busy.unwrap_or("A Git operation"),
                tab.path.display()
            );
            self.refuse_in_tab_strip(reason, cx);
            return;
        }
        self.repository_tabs.refusal = None;
        // Its cause gone with the closed tab, the page's refusal goes too,
        // as the strip's just did.
        self.hub.update(cx, |hub, cx| {
            hub.clear_error_matching(TAB_LIMIT_REFUSAL, cx);
            hub.clear_error_matching(RETAINED_ALLOWANCE_REFUSAL, cx);
        });
        let active = self.repository_tabs.active == Some(index);
        if active {
            // The closed tab's read is dropped, and what it was preparing:
            // as a tab switch drops it when another tab is shown, and as
            // Projects cancels it when the last tab closes.
            self.working_read_deferred = None;
            let switch = if self.repository_tabs.tabs.len() > 1 {
                crate::page_navigation::PageSwitch::Tab
            } else {
                crate::page_navigation::PageSwitch::Away
            };
            self.end_preparations(switch, window, cx);
            self.persist_commit_draft(window, cx);
            self.invalidate_read();
            self.worker.release_history();
            self.automatic.reset();
            self.work_generation = self.work_generation.wrapping_add(1);
            self.status_task = None;
            // The bookmark the closed tab was being restored from goes with
            // it, as the places its comparison waited to restore go with its
            // preview: left, the tab shown next took it up.
            self.repository_tabs.restoring = None;
            self.clear_preview();
            self.file_history = file_history::State::default();
            self.revision_inspection = revision_inspection::State::default();
            self.blame = blame::State::default();
            self.discard_history_search();
            self.commits.clear();
            self.inspector_message
                .replace(commit_message::State::default(), cx);
            self.graph.clear();
            self.visible.clear();
            self.files.clear();
            self.selected_commit = None;
            self.selected_file = None;
            self.history_list_layout = None;
            self.file_list_layout = None;
            self.repository = None;
            self.path = None;
            self.scope = None;
            self.work_status = None;
            self.working_identity = None;
            self.working_read_failure = None;
            self.working_rows.clear();
            self.working_paths.reset();
            self.file_paths.reset();
        }
        let closed = self.repository_tabs.tabs.remove(index).path;
        // A wait for a preparation the closed tab dropped says it finishes
        // a closed tab's work, in a tab that opens its repository again too.
        self.forget_dropped_preparation_tab(&closed);
        self.hold_dropped_preparation();
        if active {
            self.repository_tabs.active = None;
            if !self.repository_tabs.tabs.is_empty() {
                self.switch_repository_tab(
                    index.min(self.repository_tabs.tabs.len() - 1),
                    window,
                    cx,
                );
            } else {
                self.page = AppPage::Projects;
                self.page_return_focus = None;
                self.hub
                    .update(cx, |hub, cx| hub.set_can_go_back(false, cx));
                // The visible Projects page takes focus on its selected
                // action tab, drawn with its ring.
                self.hub
                    .update(cx, |hub, cx| hub.focus_selected_mode(window, cx));
            }
        } else if self
            .repository_tabs
            .active
            .is_some_and(|active| active > index)
        {
            self.repository_tabs.active = self.repository_tabs.active.map(|active| active - 1);
        }
        self.save_repository_session(window, cx);
        self.repaint_page(window, cx);
    }
    fn session_snapshot(&mut self, cx: &App) -> Session {
        if let Some(index) = self.repository_tabs.active
            && self.repository.is_some()
        {
            let bookmark = self.tab_bookmark(cx);
            if let Some(tab) = self.repository_tabs.tabs.get_mut(index) {
                tab.saved = bookmark;
            }
        }
        self.repository_tabs.saved()
    }
    fn queue_session_snapshot(&mut self, cx: &App) -> SessionSaveCompletion {
        let session = self.session_snapshot(cx);
        #[cfg(test)]
        let save_path = self.repository_tabs.save_path.clone();
        let response = self.repository_tabs.saver.queue_with(
            &self.preferences_writer,
            (),
            session,
            move |sessions| {
                let Some(session) = sessions.get(&()).cloned() else {
                    return Ok(());
                };
                #[cfg(test)]
                if let Some(path) = &save_path {
                    return session.save_at(path);
                }
                session.save()
            },
        );
        if let Some(response) = response {
            self.repository_tabs.save_completion = Some(
                response
                    .map(|result| match result {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(error)) if error.is::<operations::Busy>() => {
                            Err(SessionSaveFailure::Busy)
                        }
                        Ok(Err(error)) => Err(SessionSaveFailure::Failed(format!("{error:#}"))),
                        Err(_) => Err(SessionSaveFailure::Failed(
                            "Repository session saving was interrupted".into(),
                        )),
                    })
                    .boxed()
                    .shared(),
            );
        }
        // A coalesced close snapshot belongs to the already accepted save job;
        // waiting for an extra barrier could fail when the executor is full.
        self.repository_tabs
            .save_completion
            .as_ref()
            .expect("every pending session belongs to an accepted completion")
            .clone()
    }
    fn flush_session_for_shutdown(&mut self, cx: &App) -> SessionSaveCompletion {
        self.repository_tabs.save_pending = None;
        if self.repository_tabs.closing
            && let Some(completion) = &self.repository_tabs.save_completion
        {
            return completion.clone();
        }
        self.repository_tabs.closing = true;
        self.queue_session_snapshot(cx)
    }
    pub(super) fn install_tab_quit_observer(&mut self, window: &Window, cx: &mut Context<Self>) {
        let owner = cx.weak_entity();
        let owner_window = window.window_handle().window_id();
        self.subscriptions.push(cx.on_window_closed(move |cx, id| {
            if id == owner_window
                && let Ok(completion) =
                    owner.update(cx, |this, cx| this.flush_session_for_shutdown(cx))
            {
                // Window-close observers run before its entities are released.
                // Keep the actual save alive at app lifetime for delayed native
                // termination after the last window and this view disappear.
                let mut completion = Some(completion);
                cx.on_app_quit(move |_| {
                    let completion = completion.take();
                    async move {
                        if let Some(completion) = completion {
                            let _ = completion.await;
                        }
                    }
                })
                .detach();
            }
        }));
        self.subscriptions.push(cx.on_app_quit(|this, cx| {
            let completion = this.flush_session_for_shutdown(cx);
            async move {
                let _ = completion.await;
            }
        }));
    }
    /// Saves the session shortly after the tabs change. The person asked for
    /// none of these, so a writer that is merely behind is not a failure: a
    /// save a full queue refuses reports nothing and asks again with the
    /// newest session.
    pub(super) fn save_repository_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(250));
        self.repository_tabs.save_pending = Some(cx.spawn_in(window, async move |this, cx| {
            timer.await;
            let response = this.update(cx, |this, cx| this.queue_session_snapshot(cx));
            if let Ok(response) = response {
                let result = response.await;
                let _ = this.update_in(cx, |this, window, cx| match result {
                    Ok(()) => {}
                    Err(SessionSaveFailure::Busy) => this.ask_again_to_save_session(window, cx),
                    Err(SessionSaveFailure::Failed(error)) => {
                        this.draft_save_error = Some(format!("Repository session: {error}"));
                        cx.notify();
                    }
                });
            }
        }));
    }
    /// Asks again, after the wait, to save the session a full queue refused.
    fn ask_again_to_save_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.repository_tabs.save_retry.is_some() {
            return;
        }
        let retry = self.retry_refused_save(window, cx, |this, window, cx| {
            this.repository_tabs.save_retry = None;
            if this.repository_tabs.saver.is_pending() {
                this.save_repository_session(window, cx);
            }
        });
        self.repository_tabs.save_retry = Some(retry);
    }
    fn observe_tab_panels(&mut self, cx: &mut Context<Self>) {
        self.repository_tabs.panel_observers.clear();
        for panels in [&self.content_panels, &self.history_panels] {
            self.repository_tabs
                .panel_observers
                .push(cx.observe(panels, |_, _, cx| cx.notify()));
        }
    }
    /// Restores a tab's saved widths as they were chosen: the panels draw
    /// them within the window, which never writes them.
    pub(super) fn restore_tab_widths(&mut self, saved: &Bookmark) {
        if let Some(width) = saved.navigation_width {
            self.settings.navigation_width = width;
        }
        if let Some(width) = saved.inspector_width {
            self.settings.inspector_width = width;
        }
    }
    pub(super) fn finish_tab_snapshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = self.repository_tabs.restoring.clone() else {
            self.save_repository_session(window, cx);
            return;
        };
        self.restore_tab_widths(&saved);
        self.graph_offset = saved.graph_offset;
        self.zoom = saved.zoom;
        self.text_mode = match saved.text_mode.as_str() {
            "split" => TextMode::Split,
            "before" => TextMode::Before,
            "after" => TextMode::After,
            "diagrams" => TextMode::Diagrams,
            "markdown" => TextMode::Markdown,
            _ => TextMode::Unified,
        };
        let query = saved.query.clone();
        if let Some(pinned) = &saved.pinned
            && saved.offset > 0
        {
            // The saved scope becomes the window's only with its page's
            // rows. A page refused, as while a write runs, leaves nothing to
            // wait for: the restore ends in History, paging on the scope the
            // snapshot shows.
            if !self.request_history_page_in(pinned.scope(), saved.offset, window, cx) {
                self.end_restore_in_history(window, cx);
            }
            return;
        }
        if !query.is_empty() {
            let pinned = saved.pinned.as_ref().map(SavedHistoryScope::scope);
            self.repository_tabs.switching = true;
            self.search
                .update(cx, |input, cx| input.set_value(query, window, cx));
            self.repository_tabs.switching = false;
            self.history_query_changed(window, cx);
            self.history_search.restore_pinned_scope(pinned);
            if saved.comparison.is_none() {
                // A saved query the search refuses, past 4 KiB, starts no
                // read: the restore ends in History.
                if !self.history_search_pending() {
                    self.end_restore_in_history(window, cx);
                }
                return;
            }
        }
        self.restore_tab_selection(window, cx);
    }
    pub(super) fn restore_tab_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = self.repository_tabs.restoring.clone() else {
            return;
        };
        if saved.comparison.is_some() {
            self.restore_captured_tab_comparison(saved, window, cx);
            return;
        }
        let selected = restore_selected_identity(
            &saved,
            &mut self.commits,
            &mut self.graph,
            &mut self.automatic.retained_commit,
        );
        self.filter_history_retaining_scroll(cx);
        let history_y = saved.history_y;
        let horizontal_x = saved.horizontal_x;
        if let Some(index) = selected {
            self.select_commit(index, window, cx);
            let parent = saved
                .parent
                .min(self.commits[index].parents.len().saturating_sub(1));
            if parent != 0 {
                // The restore's own read of the saved parent's changes
                // replaces its read of the first parent's, so the bookmark
                // stands aside meanwhile (`end_replaced_restore`).
                let restoring = self.repository_tabs.restoring.take();
                self.change_parent(parent, window, cx);
                self.repository_tabs.restoring = restoring;
            }
        } else if saved.selected_oid.is_some() {
            self.status =
                "Saved selection is unavailable; choose a commit from the restored history".into();
        }
        restore_list_viewport(
            &self.history_scroll,
            history_y,
            &mut self.history_list_layout,
        );
        self.history_horizontal
            .set_offset(point(px(horizontal_x), px(0.)));
        // With no saved commit to select, the restore ends here in History,
        // where History's search asked for meanwhile opens, after a page or
        // a search reply as after the snapshot. One bound for Compare, as an
        // older bookmark with no captured comparison is, held it for a
        // Compare that never came, and it was lost.
        if selected.is_none() {
            self.end_restore_in_history(window, cx);
        }
    }
    /// Ends a cold tab's restore that stops in History short of the reads it
    /// had left, as one that finds no saved commit, or whose page it cannot
    /// read or finds empty past the end (`finish_tab_snapshot`,
    /// `receive_history_page`): History's search asked for meanwhile opens
    /// there (`run_pending_search`). Decision (2026-11-05): every way a
    /// restore ends runs that search or drops it, and a restore always ends;
    /// left set with no read under way, it held the search, and the tab's
    /// later replies took the saved selection.
    pub(super) fn end_restore_in_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.repository_tabs.restoring.take().is_some() {
            self.run_pending_search(window, cx);
        }
    }
    pub(super) fn restore_tab_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = self.repository_tabs.restoring.take() else {
            return;
        };
        if let Some(path) = saved.selected_file.as_ref().map(SavedPath::path)
            && let Some(index) = self.files.iter().position(|file| {
                file.new_path.as_ref() == Some(&path) || file.old_path.as_ref() == Some(&path)
            })
        {
            if saved.compare {
                // Focus lands as it does on a captured comparison: a dialog,
                // a sheet, Settings or Projects shown over the restore, or a
                // control the person moved to keeps it, and History's list,
                // where showing the tab put it, gives way to the list
                // Compare draws. Chosen as a click chooses it, the file took
                // focus from a dialog, whose keys then reached the page
                // under it, and from Settings or Projects, whose keys then
                // reached no drawn control.
                self.show_file(index, false, window, cx);
                self.land_restore_focus(window, cx);
            } else {
                self.selected_file = Some(index);
            }
        } else {
            // A bookmark in Compare whose file the saved commit no longer
            // changes, or that chose none, stays in History, where History's
            // search asked for meanwhile opens, as it does where a restore
            // stays in History; held for a Compare that never came, it was
            // lost (`run_pending_search`).
            self.run_pending_search(window, cx);
        }
        restore_list_viewport(&self.file_scroll, saved.file_y, &mut self.file_list_layout);
        // Seed the completed layout without reapplying the history offset:
        // a wheel scroll made during the selected commit's read must survive.
        self.history_list_layout = None;
        self.save_repository_session(window, cx);
    }

    fn restore_captured_tab_comparison(
        &mut self,
        saved: Bookmark,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(_), Some(file)) = (
            self.repository.as_ref(),
            saved.comparison.as_ref().map(SavedFile::file),
        ) else {
            return;
        };
        self.repository_tabs.restoring = None;
        self.selected_commit = restore_selected_identity(
            &saved,
            &mut self.commits,
            &mut self.graph,
            &mut self.automatic.retained_commit,
        );
        self.filter_history_retaining_scroll(cx);
        self.parent = saved.parent;
        self.files = vec![file.clone()];
        self.selected_file = Some(0);
        self.mode = WorkspaceMode::Compare;
        self.pane = Pane::Files;
        self.sidebar = false;
        self.land_restore_focus(window, cx);
        self.refresh_file_filter(window, cx);
        restore_list_viewport(
            &self.history_scroll,
            saved.history_y,
            &mut self.history_list_layout,
        );
        self.history_horizontal
            .set_offset(point(px(saved.horizontal_x), px(0.)));
        // History's search, asked for while the snapshot was read, left
        // Compare for History as the restore landed: nothing shows the
        // comparison to read.
        if self.mode == WorkspaceMode::Compare {
            self.request_restored_comparison(saved, window, cx);
        }
    }

    /// Holds History's search, asked for by its shortcut or command
    /// (`search`) while the shown tab's snapshot is read, for the tab and
    /// the focus the key found: History has nothing to show yet, and
    /// showing it then showed Projects, whose own control took focus, so
    /// the key acted on another page than the one it was pressed in.
    /// Decision (2026-11-05, owner-delegated): a shortcut acts on the
    /// context it was pressed in, so the search opens as the tab lands
    /// (`run_pending_search`), never Projects. Returns whether it held it:
    /// only a tab reading its repository shows none.
    pub(super) fn defer_search(&mut self, window: &Window, cx: &App) -> bool {
        let Some(path) = self.path.clone().filter(|_| self.repository.is_none()) else {
            return false;
        };
        self.repository_tabs.pending_search = Some(PendingSearch {
            path,
            focus: window.focused(cx),
        });
        true
    }

    /// Opens History's search held while the tab's snapshot was read
    /// (`defer_search`) once the tab can show it: as a fresh open's snapshot
    /// lands, as a cold tab's does where its restore stays in History, as
    /// that restore lands in Compare (`land_restore_focus`), which the
    /// search leaves for History as it does from Compare, or as it ends in
    /// History short of Compare (`restore_lands_in_compare`). It opens only in
    /// the tab it was asked in, on the repository page with no modal over
    /// it, and while focus rests where the key found it: a control the
    /// person focused meanwhile keeps focus, and the search is dropped, as
    /// it is once the window opens or shows another tab (`open`,
    /// `restore_warm_tab`). Returns whether it opened.
    pub(super) fn run_pending_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(PendingSearch { path, focus }) = self.repository_tabs.pending_search.take() else {
            return false;
        };
        if self.repository.is_none()
            || self.page != AppPage::Repository
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
            || self.path.as_ref() != Some(&path)
            || window.focused(cx) != focus
        {
            return false;
        }
        self.search(&Search, window, cx);
        true
    }

    /// Focus as a cold tab's restore lands in Compare, on its captured
    /// comparison or on the file an older session's bookmark chooses from
    /// the saved commit's changes (`restore_tab_file`). Showing the tab put
    /// focus on History's commit list (`open`, through `back_to_history`),
    /// which Compare does not draw, so it goes where choosing the file puts
    /// it, the changed-file list (`focus_compare_files`), and so does the
    /// focus Settings or Projects, opened over the restore, returns to,
    /// where it was History's list or the root; any other control it
    /// returns to is checked as the frame that draws the page ends
    /// (`give_page_focus_back`). Focus the person moved while the tab was
    /// restored is theirs: a dialog or sheet keeps it, and a control in the
    /// tab, or one no frame has drawn yet, keeps it while the frame that
    /// draws Compare draws it, while one Compare takes away, as the
    /// navigation's filter, or one that frame draws nowhere, gives way to
    /// that list as that frame ends (`return_tab_focus`), since a control
    /// no frame draws answers no key. Where focus stays, it is watched
    /// until it leaves (`land_focus_leaving`), as a menu's close hands it
    /// back to what the menu was opened from. Left on History's list, focus
    /// rested on nothing the frame drew, where keys reached none of the
    /// window's shortcuts, Escape and the tab shortcuts among them, until a
    /// click on a file row.
    fn land_restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // History's search asked for while the snapshot was read opens
        // here, leaving Compare as the search does.
        if self.run_pending_search(window, cx) {
            return;
        }
        if self.page != AppPage::Repository {
            if self
                .page_return_focus
                .as_ref()
                .is_some_and(|focus| *focus == self.focus || *focus == self.app_focus)
            {
                self.page_return_focus = Some(self.file_focus.clone());
            }
            return;
        }
        // A dialog or sheet keeps focus, one opened since the last frame
        // too, and gives it back as it closes through the window's own
        // repair (`update_modal_focus`), to a control the page draws.
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some(focus) = window
            .focused(cx)
            .filter(|focus| *focus != self.focus && *focus != self.app_focus)
        else {
            self.focus_compare_files(window, cx);
            return;
        };
        // Whether the frame that draws Compare draws the control is known
        // as that frame ends: one the last frame drew in the tab, or one no
        // frame has drawn yet, as a menu opened since, keeps focus there if
        // that frame draws it in the tab and otherwise gives way to the
        // changed-file list (`return_tab_focus`).
        if self.app_focus.contains(&focus, window) || window.context_stack().is_empty() {
            self.repository_tabs.focus_return = Some(FocusReturn {
                page: AppPage::Repository,
                path: self.path.clone(),
                held: focus.clone(),
                focus: Some(focus.clone()),
            });
        }
        self.land_focus_leaving(&focus, window, cx);
    }

    /// Whether focus rests where no key reaches Compare after a restore
    /// landed there: on nothing; on History's commit list, where showing the tab
    /// put it and which Compare does not draw; on the window's root, which
    /// draws no focus; or on a control the last frame drew nowhere, which
    /// has no key context.
    fn restore_focus_stranded(&self, window: &Window, cx: &App) -> bool {
        window.focused(cx).is_none_or(|focus| {
            focus == self.focus || focus == self.app_focus || window.context_stack().is_empty()
        })
    }

    /// Lands focus once more as it leaves `kept`, the control the person's
    /// focus rested on as the restore landed in Compare, if the frame it
    /// leaves in draws it nowhere a key reaches: a menu opened while the
    /// snapshot was read and closed after the landing gives focus back to
    /// what it was opened from, as History's commit list or the
    /// navigation's filter, which Compare does not draw, so focus goes to
    /// the changed-file list as the landing puts it. Without it the menu's
    /// Escape left focus on nothing drawn, and Escape, the palette and the
    /// tab shortcuts did nothing until a click on a file row. Focus leaving
    /// it for anything drawn, or once the tab or its page is left, ends the
    /// watch.
    fn land_focus_leaving(
        &mut self,
        kept: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.path.clone();
        self.repository_tabs.landed_focus =
            Some(cx.on_focus_out(kept, window, move |this, _, window, cx| {
                this.repository_tabs.landed_focus = None;
                if this.page == AppPage::Repository
                    && this.path == path
                    && this.mode == WorkspaceMode::Compare
                    && !window.has_active_dialog(cx)
                    && !window.has_active_sheet(cx)
                    && this.restore_focus_stranded(window, cx)
                {
                    this.focus_compare_files(window, cx);
                }
            }));
    }

    /// Reads the captured comparison `saved` keeps, under its own label,
    /// which Back cancels too, its documents' places waiting for the reply
    /// ([`Self::restore_tab_documents`]).
    fn request_restored_comparison(
        &mut self,
        saved: Bookmark,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(repo), Some(file)) = (
            self.repository.clone(),
            saved.comparison.as_ref().map(SavedFile::file),
        ) else {
            return;
        };
        let origins =
            markdown_view::Origins::revisions(saved.origins[0].clone(), saved.origins[1].clone());
        self.repository_tabs.document_restore = Some(saved);
        self.request(
            Job::Preview {
                repo,
                file,
                origins,
            },
            RESTORED_PREVIEW_LABEL,
            window,
            cx,
        );
    }

    /// Whether the window shows the comparison `saved` keeps: Compare, on
    /// the file it was saved with.
    fn shows_saved_comparison(&self, saved: &Bookmark) -> bool {
        self.mode == WorkspaceMode::Compare
            && saved.comparison.as_ref().is_some_and(|saved| {
                self.selected_file
                    .and_then(|index| self.files.get(index))
                    .is_some_and(|shown| SavedFile::new(shown).file() == saved.file())
            })
    }

    /// The places of the comparison restored with the shown tab that wait
    /// to be restored, where that comparison is the one shown.
    fn pending_tab_documents(&self) -> Option<&Bookmark> {
        self.repository_tabs
            .document_restore
            .as_ref()
            .filter(|saved| self.shows_saved_comparison(saved))
    }

    /// Takes up the restore a warm tab was left in as it shows again: a
    /// comparison whose reply had landed restores its documents with its
    /// editor, and one whose read the switch cancelled is read again, its
    /// places waiting for the reply. Without it, the tab came back without
    /// the places it was saved with.
    fn resume_tab_documents(
        &mut self,
        saved: Bookmark,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.shows_saved_comparison(&saved) {
            return;
        }
        if self.content.is_some() {
            self.repository_tabs.document_restore = Some(saved);
            self.prepare_editor(false, window, cx);
            self.restore_tab_documents(cx);
        } else {
            self.request_restored_comparison(saved, window, cx);
        }
    }

    /// Reads again the preview a warm tab shown again in Compare was left
    /// reading, which the switch cancelled (`retain_active_tab`), without
    /// activating its file, as File history reads its selected revision
    /// again (`resume_file_history`). A comparison restored with the tab is
    /// read again under its own label first (`resume_tab_documents`), and
    /// File history's revision by File history, so neither is read twice;
    /// attribution shown over the comparison reads it as it closes. A Quick
    /// Open source is read as it was opened, by its tracked read
    /// (`read_selected_preview`). The tab came back in Compare with its file
    /// selected, no content and nothing reading it.
    fn resume_tab_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != WorkspaceMode::Compare
            || self.content.is_some()
            || self.loading.is_some()
            || self.blame.is_visible()
        {
            return;
        }
        self.read_selected_preview(window, cx);
    }

    /// Restores the documents of the comparison restored with the tab to
    /// the places it was saved with, as its reply lands on the repository
    /// page or the window returns to it. Only that comparison takes them:
    /// with another shown, they belong to nothing shown and are dropped.
    pub(super) fn restore_tab_documents(&mut self, cx: &mut Context<Self>) {
        let Some(saved) = self.repository_tabs.document_restore.take() else {
            return;
        };
        if !self.shows_saved_comparison(&saved) {
            return;
        }
        self.zoom = saved.zoom;
        if let Some(Content::Rich(content)) = self.content.as_deref() {
            for (index, side) in [&content.old, &content.new].into_iter().enumerate() {
                if let (Some(document), Some(bookmark)) = (&side.pdf, saved.pdf[index]) {
                    document.restore(bookmark);
                }
                if let (Some(document), Some(bookmark)) = (&side.model, saved.model[index].clone())
                {
                    document.restore(bookmark);
                }
            }
        }
        if let (Some(view), Some(bookmark)) = (&self.markdown_view, saved.markdown) {
            view.update(cx, |view, cx| view.restore(bookmark, cx));
        }
    }
    pub(super) fn tab_operation_error(&self, path: &Path) -> Option<String> {
        if self.path.as_deref() == Some(path) {
            return self.operation_error.as_deref().map(str::to_owned);
        }
        self.repository_tabs
            .find(path)
            .and_then(|index| self.repository_tabs.tabs[index].warm.as_ref())
            .and_then(|warm| warm.operation_error.as_deref().map(str::to_owned))
    }
    pub(super) fn register_completed_repository(
        &mut self,
        path: PathBuf,
        done: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repository_tabs.find(&path).is_none() && self.repository_tabs.tabs.len() < MAX_TABS
        {
            // The root the clone or create returned, as discovery resolved it.
            self.repository_tabs.tabs.push(Tab {
                path: path.clone(),
                resolved: true,
                git_dir: None,
                saved: Bookmark::default(),
                warm: None,
                error: None,
            });
        }
        // The project's name and the path are the user's, either of which
        // can hold " · " itself: the line gives way only between the parts,
        // so a narrow one still reads "Repository cloned" whole.
        self.set_status_parts(&[
            done,
            name,
            &path.display().to_string(),
            "open its repository tab or folder",
        ]);
        self.save_repository_session(window, cx);
    }
    pub(super) fn tab_commit_receipt(&mut self, path: &Path, oid: String) {
        if self.path.as_deref() == Some(path) {
            self.history_updates.committed = Some(oid);
        } else if let Some(index) = self.repository_tabs.find(path)
            && let Some(warm) = self.repository_tabs.tabs[index].warm.as_mut()
        {
            warm.history_updates.committed = Some(oid);
        }
    }

    pub(super) fn tab_operation_feedback(
        &mut self,
        path: &Path,
        notice: Option<String>,
        error: Option<crate::views::OperationFailure>,
        cx: &mut Context<Self>,
    ) {
        if self.path.as_deref() == Some(path) {
            if let Some(notice) = &notice {
                self.record_outcome(path, notice.clone(), cx);
            }
            self.operation_notice = notice;
            self.operation_error = error;
        } else {
            if let Some(index) = self.repository_tabs.find(path)
                && let Some(warm) = self.repository_tabs.tabs[index].warm.as_mut()
            {
                warm.operation_notice = notice;
                warm.operation_error = error.clone();
            }
            let outcome = if error.is_some() {
                "failed"
            } else {
                "completed"
            };
            self.set_status_parts(&[
                &format!("Git operation {outcome} in {}", path.display()),
                "open that tab for details",
            ]);
        }
    }
}

impl GitTurtle {
    pub(super) fn tab_is_pinned(&self, path: &Path) -> bool {
        self.repository_tabs
            .library
            .iter()
            .find(|entry| entry.path.path() == path)
            .is_some_and(|entry| entry.pinned)
    }
    /// Records `path` in the local library with what changes for it, and
    /// returns whether the library took it: one past [`MAX_LIBRARY`] is
    /// refused, and the caller says so where the person acted
    /// ([`LIBRARY_FULL_REFUSAL`]): the pin beneath the tab strip, the Local
    /// workspaces dialog in its own error line, which drew the refusal on
    /// the strip under its backdrop and listed the entry as added.
    fn set_tab_library(
        &mut self,
        path: PathBuf,
        pinned: Option<bool>,
        group: Option<Option<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let index = self
            .repository_tabs
            .library
            .iter()
            .position(|entry| entry.path.path() == path)
            .unwrap_or_else(|| {
                self.repository_tabs.library.push(LibraryEntry {
                    path: SavedPath::new(&path),
                    pinned: false,
                    group: None,
                });
                self.repository_tabs.library.len() - 1
            });
        if self.repository_tabs.library.len() > MAX_LIBRARY {
            self.repository_tabs.library.pop();
            return false;
        }
        let entry = &mut self.repository_tabs.library[index];
        if let Some(pinned) = pinned {
            entry.pinned = pinned;
        }
        if let Some(group) = group {
            entry.group = group;
        }
        self.save_repository_session(window, cx);
        cx.notify();
        true
    }
    pub(super) fn toggle_repository_pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        if let Some(path) = self.path.clone() {
            let pinned = !self.tab_is_pinned(&path);
            if !self.set_tab_library(path, Some(pinned), None, window, cx) {
                self.refuse_in_tab_strip(LIBRARY_FULL_REFUSAL, cx);
            }
        }
    }
    pub(super) fn open_repository_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let owner = cx.entity().downgrade();
        let session = self.session_snapshot(cx);
        let active = self.path.clone();
        let names = self.project_names.clone();
        let view = cx.new(|cx| LibraryView::new(owner, session, active, names, window, cx));
        window.open_alert_dialog(cx, move |dialog, window, _| {
            dialog
                .title("Local workspaces")
                .width(appearance::dialog_width(px(680.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(view.clone())
                .footer(crate::tags::dialog_footer("Done", None))
        });
    }
    fn open_repository_group(&mut self, group: &str, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<_> = self
            .repository_tabs
            .library
            .iter()
            .filter(|entry| entry.group.as_deref() == Some(group))
            .map(|entry| entry.path.path())
            .collect();
        let new = paths
            .iter()
            .filter(|path| self.repository_tabs.find(path).is_none())
            .count();
        if self.repository_tabs.tabs.len() + new > MAX_TABS {
            self.refuse_in_tab_strip(
                format!(
                    "Opening {group} needs {new} more tabs. Close tabs to keep at most eight open."
                ),
                cx,
            );
            return;
        }
        for path in &paths {
            if self.repository_tabs.find(path).is_none() {
                // Opening a group is an explicit open of each folder.
                self.repository_tabs.tabs.push(Tab {
                    path: path.clone(),
                    resolved: false,
                    git_dir: None,
                    saved: Bookmark::default(),
                    warm: None,
                    error: None,
                });
            }
        }
        if let Some(index) = paths
            .first()
            .and_then(|path| self.repository_tabs.find(path))
        {
            self.switch_repository_tab(index, window, cx);
        }
        self.save_repository_session(window, cx);
    }
    fn can_rename_repository_tab(&self, path: &Path) -> bool {
        self.repository_tabs.active_path() == Some(path)
            && self.path.as_deref() == Some(path)
            && self
                .repository
                .as_ref()
                .is_some_and(|repository| repository.path() == path)
    }

    /// The Workspaces menu's trigger: labelled with its caret, or, where the
    /// tab strip gives its room to the tabs' names (`labelled` false), its
    /// worktree glyph alone in a 28 px control, as the compact Actions menus
    /// are, its name and the tooltip that leads with it unchanged.
    fn repository_tabs_menu(&self, labelled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        const NAME: &str = "Workspaces and tab actions";
        const DETAIL: &str = "Switch, pin or reorder tabs; manage saved workspaces";
        let owner = cx.entity().downgrade();
        let trigger = if labelled {
            button("repository-workspaces", "Workspaces", "worktree", false)
                .dropdown_caret(true)
                .accessibility_label(NAME)
                .tooltip(DETAIL)
        } else {
            crate::views::compact_control_name(
                button("repository-workspaces", "", "worktree", false),
                "repository-workspaces",
                NAME,
            )
            .tooltip(format!("Workspaces · {DETAIL}"))
        };
        trigger
            .debug_selector(|| "repository-workspaces".into())
            .app_menu(move |mut menu, _, cx| {
                let Some(view) = owner.upgrade() else {
                    return menu;
                };
                let this = view.read(cx);
                let active = this.repository_tabs.active;
                let active_path = active
                    .and_then(|index| this.repository_tabs.tabs.get(index))
                    .map(|tab| tab.path.clone());
                menu = menu
                    .max_h(appearance::ui_size(440.))
                    .scrollable(true)
                    .label("Open repositories");
                for (index, tab) in this.repository_tabs.tabs.iter().enumerate() {
                    let path = tab.path.clone();
                    let target = path.clone();
                    let owner = owner.clone();
                    let name = this.project_name(&path);
                    menu = menu.item(
                        PopupMenuItem::new(format!("{}  {}", index + 1, name))
                            .checked(active == Some(index))
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if let Some(index) = this.repository_tabs.find(&target) {
                                        this.switch_repository_tab(index, window, cx);
                                    }
                                });
                            }),
                    );
                }
                if let Some(path) = active_path {
                    let pinned = this.tab_is_pinned(&path);
                    let pin_owner = owner.clone();
                    let pin_path = path.clone();
                    menu = menu.separator().item(
                        PopupMenuItem::new(if pinned {
                            "Unpin current repository"
                        } else {
                            "Pin current repository"
                        })
                        .disabled(this.repository.is_none())
                        .on_click(move |_, window, cx| {
                            let _ = pin_owner.update(cx, |this, cx| {
                                if this.path.as_ref() == Some(&pin_path) {
                                    this.toggle_repository_pin(window, cx);
                                }
                            });
                        }),
                    );
                    let rename_owner = owner.clone();
                    let rename_path = path.clone();
                    menu = menu.item(
                        PopupMenuItem::new("Rename current project…")
                            .disabled(!this.can_rename_repository_tab(&path))
                            .on_click(move |_, window, cx| {
                                let _ = rename_owner.update(cx, |this, cx| {
                                    if this.can_rename_repository_tab(&rename_path) {
                                        this.open_rename_project(rename_path.clone(), window, cx);
                                    }
                                });
                            }),
                    );
                    for (direction, label, disabled) in [
                        (-1, "Move current tab left", active == Some(0)),
                        (
                            1,
                            "Move current tab right",
                            active.is_none_or(|index| index + 1 >= this.repository_tabs.tabs.len()),
                        ),
                    ] {
                        let owner = owner.clone();
                        let target = path.clone();
                        menu = menu.item(PopupMenuItem::new(label).disabled(disabled).on_click(
                            move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if this.path.as_ref() == Some(&target) {
                                        this.move_repository_tab(direction, window, cx);
                                    }
                                });
                            },
                        ));
                    }
                }
                let library_owner = owner.clone();
                menu.separator()
                    .item(PopupMenuItem::new("Manage saved workspaces…").on_click(
                        move |_, window, cx| {
                            let _ = library_owner
                                .update(cx, |this, cx| this.open_repository_library(window, cx));
                        },
                    ))
            })
    }
    pub(super) fn render_repository_tabs(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let (left_controls, right_controls, drag_region) = {
            #[cfg(target_os = "linux")]
            {
                let busy = self.operation_busy.is_some();
                let looks_busy = self.operation_shown().map(|_| self.busy_reason());
                (
                    window_chrome::controls(
                        window_chrome::Side::Left,
                        busy,
                        looks_busy,
                        window,
                        cx,
                    ),
                    window_chrome::controls(
                        window_chrome::Side::Right,
                        busy,
                        looks_busy,
                        window,
                        cx,
                    ),
                    window_chrome::drag_region(window, cx),
                )
            }
            #[cfg(not(target_os = "linux"))]
            {
                (None::<AnyElement>, None::<AnyElement>, None::<AnyElement>)
            }
        };
        // The scrolling tab list clips its children to its own bounds on both
        // axes, so it keeps room inside for the ring a focused tab or close
        // button draws outside its edge, and gives the room back through its
        // margin so the tabs do not move. The strip is the window's first row,
        // so the room above a tab can only come from the strip itself.
        let ring_room = tab_list_ring_room(window, cx);
        // Menu and Workspaces each keep their labels only where every tab's
        // name stays whole beside them, and give way one at a time; the menu
        // renders inside this strip, after this, so it reads the decision in
        // this frame.
        let labels = self.tab_strip_labels(window, cx);
        #[cfg(target_os = "linux")]
        self.primary_menu
            .update(cx, |menu, _| menu.set_labelled(labels.menu));
        let (line, insets) = (
            crate::views::LineKey::current(window),
            tab_label_insets(window),
        );
        let strip =
            div()
                .debug_selector(|| "repository-tab-strip".into())
                .flex()
                .items_center()
                // A ring's room between its controls, as between its tabs:
                // the 0.25 rem left Main Menu's ring reaching into a selected
                // first tab at 11 pt (`tab_gap`).
                .gap(tab_gap(window))
                .px_2()
                .min_h(tab_strip_height(window, cx))
                .bg(rgb(colors.panel))
                .border_b(TAB_STRIP_BORDER)
                .border_color(rgb(colors.border))
                .children(left_controls)
                .when(cfg!(target_os = "linux"), |strip| {
                    #[cfg(target_os = "linux")]
                    let strip = strip.child(self.primary_menu.clone());
                    strip
                })
                .child(crate::native_accessibility::clips_children(
                    div()
                        .id("repository-tabs")
                        .debug_selector(|| "repository-tabs".into())
                        .role(Role::TabList)
                        .aria_label("Repository tabs")
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(tab_gap(window))
                        .p(ring_room)
                        .m(-ring_room)
                        .overflow_x_scroll()
                        .children(self.repository_tabs.tabs.iter().enumerate().map(
                            |(index, tab)| {
                                let selected = self.repository_tabs.active == Some(index);
                                let pinned = self.tab_is_pinned(&tab.path);
                                let busy = self.operation_shown().is_some()
                                    && self.operation_repository.as_ref() == Some(&tab.path);
                                let name = self.project_name(&tab.path);
                                let (label, pin) = self.tab_label(tab);
                                // Content before decoration: where the list is
                                // too narrow for every tab whole, even with
                                // Menu and Workspaces as icons, each tab's
                                // name gives way, down to its first character
                                // and an ellipsis, before its close button
                                // (which keeps the kit's `flex_shrink_0`) is
                                // cut at the edge. Only past that floor does
                                // the list scroll. Both lengths lie on device
                                // pixels, so layout keeps the floor whole.
                                let name_floor =
                                    insets + tab_label_widths(&label, pin, &line, window).1;
                                div()
                                    .debug_selector(move || format!("repository-tab-frame-{index}"))
                                    .flex()
                                    .items_center()
                                    .gap_0()
                                    .min_w(name_floor + appearance::ui_size(28.))
                                    .rounded(px(7.))
                                    .when(selected, |el| el.bg(rgb(colors.selected)))
                                    .child(
                                        button(("repository-tab", index), label, "", selected)
                                            // The frame's fill and edge are
                                            // the selected tab's, so under More
                                            // contrast its Button keeps no
                                            // fill, nor edge, of its own.
                                            .with_variant(
                                                appearance::control_button_variant_on_fill(
                                                    selected, cx,
                                                ),
                                            )
                                            .flex_shrink(1.)
                                            .min_w(name_floor)
                                            .debug_selector(move || {
                                                format!("repository-tab-{index}")
                                            })
                                            .role(Role::Tab)
                                            .accessibility_label(format!(
                                                "Repository tab {}, {}, {}{}{}{}{}",
                                                index + 1,
                                                name,
                                                tab.path.display(),
                                                if selected { ", selected" } else { "" },
                                                if pinned { ", pinned" } else { "" },
                                                if busy { ", Git operation running" } else { "" },
                                                if tab.error.is_some() {
                                                    ", unavailable"
                                                } else {
                                                    ""
                                                }
                                            ))
                                            .max_w(appearance::ui_size(220.))
                                            .tooltip(format!(
                                                "{}\n{}\nSwitch tab {} · {}",
                                                name,
                                                tab.path.display(),
                                                index + 1,
                                                shortcuts::tab_label(index)
                                            ))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.switch_repository_tab(index, window, cx)
                                            })),
                                    )
                                    .child(
                                        button(("close-repository-tab", index), "", "close", false)
                                            .debug_selector(move || {
                                                format!("repository-tab-close-{index}")
                                            })
                                            .accessibility_label(format!(
                                                "Close repository tab {}, {}; drafts remain saved",
                                                name,
                                                tab.path.display()
                                            ))
                                            .tooltip(format!(
                                                "Close {} · drafts remain saved\n{}",
                                                name,
                                                tab.path.display()
                                            ))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.close_repository_tab(index, window, cx)
                                            })),
                                    )
                                    // Under More contrast the selected tab's
                                    // 1 px edge, over its frame and both of
                                    // its Buttons, moving nothing.
                                    .when(selected, |el| {
                                        el.children(appearance::resting_edge(px(7.), cx))
                                    })
                            },
                        ))
                        .children(drag_region),
                ))
                .child(
                    button("new-repository-tab", "", "plus", false)
                        .debug_selector(|| "new-repository-tab".into())
                        .accessibility_label("Open repository tab")
                        .tooltip(format!(
                            "Open a repository tab · {}",
                            shortcuts::label(shortcuts::ShortcutId::NewTab)
                        ))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.choose_repository(&OpenRepository, window, cx)
                        })),
                )
                .child(self.repository_tabs_menu(labels.workspaces, cx))
                .children(right_controls)
                .into_any_element();
        // A refusal is said beneath the strip it came from; at rest the
        // strip is the row alone.
        match self.render_tab_refusal(cx) {
            Some(refusal) => div()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .child(strip)
                .child(refusal)
                .into_any_element(),
            None => strip,
        }
    }

    /// A repository tab's label, its pin, project name and what it reports,
    /// and the pin's length in bytes.
    pub(super) fn tab_label(&self, tab: &Tab) -> (String, usize) {
        let mut label = String::new();
        let pin = self.write_tab_label(tab, &mut label);
        (label, pin)
    }

    /// Writes `tab`'s label ([`Self::tab_label`]) after what `label` holds,
    /// and returns the pin's length in bytes. The name is `project_name`'s,
    /// borrowed rather than copied, so a frame that measures every tab's
    /// label into one buffer allocates nothing (`tab_whole_width_each`).
    fn write_tab_label(&self, tab: &Tab, label: &mut String) -> usize {
        let pin = if self.tab_is_pinned(&tab.path) {
            "★ "
        } else {
            ""
        };
        let busy = self.operation_shown().is_some()
            && self.operation_repository.as_ref() == Some(&tab.path);
        label.push_str(pin);
        match self.project_names.get(&tab.path) {
            Some(name) => label.push_str(name),
            // `preferences::directory_name`, which `project_name` falls back
            // to, without its copy: the folder's name, or the whole path
            // where it has none.
            None => label.push_str(
                &tab.path
                    .file_name()
                    .unwrap_or(tab.path.as_os_str())
                    .to_string_lossy(),
            ),
        }
        label.push_str(if busy {
            " · working"
        } else if tab.error.is_some() {
            " · unavailable"
        } else {
            ""
        });
        pin.len()
    }

    /// Each tab's width with its label whole, to the tab's scaled 220 px
    /// cap, without its close button, passed to `each` in order: its label,
    /// as layout rounds it (`tab_label_widths`), between its insets
    /// (`tab_label_insets`), the cap snapped to device pixels as layout
    /// snaps it (at 18 pt and scale factor 1.5 the cap is drawn 304⅔ px
    /// wide, not 304.62). The strip's choice asks on every frame
    /// (`tabs_whole_width`), so each label is written into one buffer the
    /// thread keeps and read from the label cache, under the line key the
    /// window's root render read as it began (`views::LineKey::frame`): on
    /// a frame whose tabs have not changed it allocates nothing, whatever
    /// the tab count,
    /// where a label, the project's name and the font's key were allocated
    /// anew for each tab, and the list of widths for each frame.
    fn tab_whole_width_each(&self, window: &Window, mut each: impl FnMut(Pixels)) {
        thread_local! {
            /// The label being measured, kept for the next frame's.
            static LABEL: std::cell::RefCell<String> = const {
                std::cell::RefCell::new(String::new())
            };
        }
        let key = crate::views::LineKey::current(window);
        let insets = tab_label_insets(window);
        let cap = window.pixel_snap(appearance::ui_size(220.));
        LABEL.with(|label| {
            let mut label = label.borrow_mut();
            for tab in &self.repository_tabs.tabs {
                label.clear();
                let pin = self.write_tab_label(tab, &mut label);
                each((insets + tab_label_widths(&label, pin, &key, window).0).min(cap));
            }
        });
    }

    /// Each tab's whole width ([`Self::tab_whole_width_each`]), in order.
    #[cfg(test)]
    pub(super) fn tab_whole_widths(&self, window: &Window) -> Vec<Pixels> {
        let mut widths = Vec::new();
        self.tab_whole_width_each(window, |width| widths.push(width));
        widths
    }

    /// The width the tab list takes with every tab whole
    /// (`tab_whole_width_each`) beside its 28 px close button, [`tab_gap`]
    /// apart: what the strip's labelled actions give way to
    /// (`GitTurtle::tab_strip_labels`).
    pub(super) fn tabs_whole_width(&self, window: &Window) -> Pixels {
        let gap = tab_gap(window);
        let close = window.pixel_snap(appearance::ui_size(28.));
        let mut total = px(0.);
        let mut tabs = 0;
        self.tab_whole_width_each(window, |tab| {
            total += tab + close + if tabs > 0 { gap } else { px(0.) };
            tabs += 1;
        });
        total
    }
}

/// The room the tab list keeps inside its clip, on every side, for the ring
/// a focused tab or close button draws outside its edge
/// (`appearance::button_ring_room`), rounded up to a whole device pixel:
/// layout rounds a length to the nearest device pixel, so at scale factor
/// 1.5 the ring's 3 px laid out as 2⅔ px and the clip cut the ring's outer
/// device pixel; rounded up it is 3⅓ px there. At scale factors 1 and 2 it is
/// the ring's room exactly.
pub(super) fn tab_list_ring_room(window: &Window, cx: &App) -> Pixels {
    let scale = window.scale_factor();
    px((f32::from(appearance::button_ring_room(cx)) * scale).ceil() / scale)
}

/// The gap between repository tabs: the list's 0.25 rem, snapped to the
/// device pixel as layout snaps it, and never less than the room the app's
/// Button ring (`appearance::BUTTON_FOCUS_RING`, which the theme installs)
/// takes outside a focused tab or close button, its gap plus its width
/// snapped by `Window::pixel_snap`: to the nearest device pixel, ties toward
/// zero, which is how layout rounds the ring's absolute offset, so the gap is
/// never under the ring's drawn outset. Below 12 pt the 0.25 rem is narrower
/// than the ring's 3 px, so at 11
/// pt and scale factor 1.25 or 2 a focused close button's ring reached a
/// device pixel into the next tab, whose selected fill, painted after it,
/// covered that pixel, and a focused tab's ring reached over the tab before
/// it. From 12 pt up the 0.25 rem holds the ring, so nothing moves there.
///
/// The strip keeps its controls the same gap apart, the window controls
/// beside it and each other, Main Menu, the tab list, New tab and
/// Workspaces, and predicts the list's
/// room with it (`GitTurtle::tab_list_room`, `window_chrome::strip_width`):
/// with the strip's 0.25 rem, at 11 pt and scale factor 1.25 or 2 Main
/// Menu's ring reached a device pixel into a selected first tab, whose fill
/// covered it, the first tab's ring a device pixel over Menu, and New tab's
/// and Workspaces' rings, and each window control's and its neighbour's,
/// into each other.
pub(super) fn tab_gap(window: &Window) -> Pixels {
    let ring = appearance::BUTTON_FOCUS_RING;
    window
        .pixel_snap(window.rem_size() * 0.25)
        .max(window.pixel_snap(ring.gap + ring.width))
}

/// A tab's label insets, the `button` helper's `LABEL_INSET` beside its
/// label on either side, each snapped to device pixels as layout snaps it.
fn tab_label_insets(window: &Window) -> Pixels {
    window.pixel_snap(appearance::ui_size(crate::LABEL_INSET)) * 2.
}

/// The rule under the repository tab strip, inside its height.
const TAB_STRIP_BORDER: Pixels = px(1.);

/// Tab labels' whole widths, rounded up to a whole pixel, and floors by
/// label, with the font and sizes they were laid out with. The window's
/// scale factor is applied as each is read (`tab_label_widths`), so windows
/// at two scale factors share them: keyed by the scale factor too, the one
/// entry was laid out again on each window's frames in turn.
type LabelWidths =
    std::cell::RefCell<Option<(crate::views::LineKey, Vec<(String, (Pixels, Pixels))>)>>;

thread_local! {
    /// Each tab label's whole width and floor (`tab_label_widths`), so a
    /// steady render lays none out.
    static TAB_LABEL_WIDTHS: LabelWidths = const { std::cell::RefCell::new(None) };
}

/// A repository tab label's whole width, rounded up as layout rounds it, to
/// a whole pixel and then a whole device pixel
/// (`views::laid_out_text_width`), and its floor: the narrowest it is drawn
/// in without a lone ellipsis, in the interface font at the tab's text size:
/// the whole label, or, where that is narrower, its pin (the first `pin`
/// bytes) and the name's first character before the ellipsis. The toolkit's
/// truncation keeps the characters that fit strictly before its ellipsis and
/// drops trailing whitespace and ASCII punctuation, so the cut runs to the
/// first character it keeps, and takes the next whole pixel past its width,
/// rounded up to a whole device pixel too: as a tab's minimum width, a whole
/// pixel off the device grid lost a third of a pixel to layout at scale
/// factor 1.5, which a cut a hair narrower than the floor needs. `key` is
/// the window's [`crate::views::LineKey`], read once for every tab a frame
/// measures: reading it builds the window's text style, whose default font
/// features GPUI allocates.
fn tab_label_widths(
    label: &str,
    pin: usize,
    key: &crate::views::LineKey,
    window: &Window,
) -> (Pixels, Pixels) {
    let (whole, floor) = TAB_LABEL_WIDTHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref().is_none_or(|(cached, _)| cached != key) {
            *cache = Some((key.clone(), Vec::new()));
        }
        let floors = &mut cache.as_mut().expect("the cache for this key").1;
        if let Some((_, widths)) = floors.iter().find(|(cached, _)| cached == label) {
            return *widths;
        }
        let size = appearance::ui_text(12.);
        let weight = window.text_style().font_weight;
        let width = |text: &str| crate::views::shaped_width(text, size, weight, window);
        let whole = width(label).ceil();
        let floor = label[pin..]
            .char_indices()
            .find(|(_, c)| !(c.is_whitespace() || c.is_ascii_punctuation()))
            .map_or(whole, |(start, c)| {
                let cut = format!("{}…", &label[..pin + start + c.len_utf8()]);
                (width(&cut).floor() + px(1.)).min(whole)
            });
        // Labels change as tabs open, close, pin or report work; the tabs a
        // window holds bound what stays.
        if floors.len() >= MAX_TABS * 4 {
            floors.clear();
        }
        floors.push((label.to_owned(), (whole, floor)));
        (whole, floor)
    });
    // Layout rounds the whole label's pixel up to a whole device pixel; the
    // floor, no wider than the whole pixel, takes the same rounding, so it
    // stays no wider than the whole label.
    (
        crate::views::laid_out_text_width(whole, window),
        crate::views::laid_out_text_width(floor, window),
    )
}

/// The repository tab strip's minimum height: the larger of its 36 px at the
/// interface size and a tab or close button (the shared `button` helper's
/// 28 px, like every control in the strip) plus `ring_room` above and below it
/// and the strip's bottom rule, which lies inside that height. The controls
/// are centered above the rule, so the strip grows only by the few pixels the
/// smallest text scales lack (below 0.875 of the default, where the scaled
/// 8 px between 36 and 28 fall under the default ring's 3 px twice plus the
/// 1 px rule) and nothing moves where the room already fits. The strip is
/// drawn, and its height predicted, at [`tab_strip_height`].
pub(super) fn tab_strip_min_height(ring_room: Pixels) -> Pixels {
    let controls = appearance::ui_size(28.);
    appearance::ui_size(36.).max(controls + ring_room * 2. + TAB_STRIP_BORDER)
}

/// The tab strip's minimum height in `window`, where it is drawn
/// (`render_repository_tabs`) and where the chrome predicts it
/// (`views::GitTurtle::chrome`): [`tab_strip_min_height`] for the tab
/// list's room, rounded up to a device pixel ([`tab_list_ring_room`]), and
/// where that grows the strip past its 36 px, the room above and below a
/// control as layout snaps the control's 28 px, rounded up to a whole
/// device pixel. Laid out from the unrounded 3 px, at 11 pt and scale
/// factor 1.5 or 1.75 the strip left a focused tab's ring a fraction of a
/// device pixel short above the tab, and from the authored 28 px, rounded
/// to the nearest pixel, a whole one at desktop text scale 0.5 and 1.75:
/// the ring's outer pixel fell outside the window. Where the 36 px leave
/// the room it is those 36 px, so nothing moves there.
pub(super) fn tab_strip_height(window: &Window, cx: &App) -> Pixels {
    let room = tab_list_ring_room(window, cx);
    let least = tab_strip_min_height(room);
    if least <= appearance::ui_size(36.) {
        return least;
    }
    let scale = window.scale_factor();
    let grown = window.pixel_snap(appearance::ui_size(28.)) + room * 2. + TAB_STRIP_BORDER;
    px((f32::from(grown) * scale).ceil() / scale)
}

struct LibraryView {
    owner: WeakEntity<GitTurtle>,
    session: Session,
    active: Option<PathBuf>,
    project_names: HashMap<PathBuf, String>,
    name: Entity<InputState>,
    error: Option<String>,
    /// How many times the error line has said why something was refused,
    /// which keys its node: the same refusal again is a fresh node, which
    /// an assistive technology announces again.
    errors_shown: usize,
    _subscription: Subscription,
}
impl LibraryView {
    fn new(
        owner: WeakEntity<GitTurtle>,
        session: Session,
        active: Option<PathBuf>,
        project_names: HashMap<PathBuf, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Workspace name"));
        let subscription =
            cx.subscribe_in(&name, window, |_, _, _: &InputEvent, _, cx| cx.notify());
        Self {
            owner,
            session,
            active,
            project_names,
            name,
            error: None,
            errors_shown: 0,
            _subscription: subscription,
        }
    }

    /// Says why the dialog refused what was asked, heard again however
    /// often the same refusal is given.
    fn refuse(&mut self, error: impl Into<String>) {
        self.errors_shown = self.errors_shown.wrapping_add(1);
        self.error = Some(error.into());
    }

    fn project_identity(&self, path: &Path) -> String {
        let name = self
            .project_names
            .get(path)
            .cloned()
            .unwrap_or_else(|| preferences::directory_name(path));
        format!("{name} · {}", path.display())
    }
}
use crate::native_accessibility::text as workspace_text;

impl Render for LibraryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        let create_label = self.active.as_ref().map_or_else(
            || "Create named local workspace".to_owned(),
            |path| {
                format!(
                    "Create named local workspace and add repository {}",
                    self.project_identity(path)
                )
            },
        );
        div().flex().flex_col().gap_3().child(workspace_text("local-workspaces-description", "Pinned repositories stay available after their tab closes. Named workspaces are local groups; opening a group only reads its selected repository.").text_size(appearance::ui_text(12.)))
            .children(self.active.as_ref().map(|path| workspace_text("local-workspaces-current-repository", format!("Current repository: {}", self.project_identity(path))).text_size(appearance::ui_text(12.)).truncate().tooltip({let identity=self.project_identity(path);move|window,cx|Tooltip::new(identity.clone()).build(window,cx)})))
            .child(div().flex().items_center().gap_2().child(div().debug_selector(||"local-workspace-name".into()).flex_1().child(Input::new(&self.name).aria_label("New local workspace name"))).child(button("create-local-workspace","Create workspace","",false).accessibility_label(create_label).debug_selector(||"create-local-workspace".into()).on_click(cx.listener(|this,_,window,cx|{
                let name=this.name.read(cx).value().trim().to_owned();
                if name.is_empty()||name.len()>64||name.contains(['\n','\r','\0']){this.refuse("Use a workspace name of 1–64 bytes.");cx.notify();return;}
                if this.session.groups.len()>=MAX_GROUPS||this.session.groups.contains(&name){this.refuse("Choose a new name; up to 16 local workspaces are supported.");cx.notify();return;}
                // The group is made either way; a full library refuses only
                // the repository, said here, where the person asked.
                this.session.groups.push(name.clone());let active=this.active.clone();let has_path=active.is_some();let refused=this.owner.update(cx,|owner,cx|{owner.repository_tabs.groups.push(name.clone());let accepted=active.is_some_and(|path|owner.set_tab_library(path,None,Some(Some(name.clone())),window,cx));if !accepted{owner.save_repository_session(window,cx);}has_path&&!accepted}).unwrap_or(false);
                if !refused&&let Some(path)=this.active.clone(){if let Some(entry)=this.session.library.iter_mut().find(|entry|entry.path.path()==path){entry.group=Some(name);}else{this.session.library.push(LibraryEntry{path:SavedPath::new(&path),pinned:false,group:Some(name)});}}
                this.name.update(cx,|input,cx|input.set_value("",window,cx));if refused{this.refuse(LIBRARY_FULL_REFUSAL);}else{this.error=None;}cx.notify();
            }))))
            .children(self.error.as_ref().map(|error|crate::native_accessibility::refusal_line("local-workspaces-error", self.errors_shown, error.clone(), |line|line.text_color(rgb(colors.removed)))))
            .child(crate::native_accessibility::clips_children(div().id("local-workspace-library").max_h(px(360.)).overflow_y_scroll().flex().flex_col().gap_2()
                .children(self.session.groups.clone().into_iter().enumerate().map(|(index,group)|{
                    let count=self.session.library.iter().filter(|entry|entry.group.as_ref()==Some(&group)).count();let assign=group.clone();let open=group.clone();let remove=group.clone();
                    let description=format!("{group} · {}", crate::counted(count, "repository", "repositories"));
                    let add_label=self.active.as_ref().map_or_else(||format!("Add current repository to workspace {group}; no repository is open"), |path|format!("Add repository {} to workspace {group}", self.project_identity(path)));
                    div().flex().items_center().gap_2().child(workspace_text(("local-workspace-name", index), description.clone()).flex_1())
                        .child(button(("open-workspace",index),"Open","",false).accessibility_label(format!("Open workspace {description}")).disabled(count==0).on_click(cx.listener(move|this,_,window,cx|{window.close_dialog(cx);let _=this.owner.update(cx,|owner,cx|owner.open_repository_group(&open,window,cx));})))
                        .child(button(("assign-workspace",index),"Add current","",false).accessibility_label(add_label).debug_selector(move||format!("assign-workspace-{index}")).disabled(self.active.is_none()).on_click(cx.listener(move|this,_,window,cx|{if let Some(path)=this.active.clone(){
                            // A full library refuses the repository, said here, where the person asked;
                            // added, the line no longer says it was refused.
                            let refused=this.owner.update(cx,|owner,cx|!owner.set_tab_library(path.clone(),None,Some(Some(assign.clone())),window,cx)).unwrap_or(false);
                            if refused{this.refuse(LIBRARY_FULL_REFUSAL);}else{this.error=None;if let Some(entry)=this.session.library.iter_mut().find(|entry|entry.path.path()==path){entry.group=Some(assign.clone());}else{this.session.library.push(LibraryEntry{path:SavedPath::new(&path),pinned:false,group:Some(assign.clone())});}}cx.notify();}})))
                        .child(button(("delete-workspace",index),"Remove group","",false).accessibility_label(format!("Remove workspace group {group}; keep its repositories and drafts")).on_click(cx.listener(move|this,_,window,cx|{this.session.groups.retain(|group|group!=&remove);for entry in &mut this.session.library{if entry.group.as_ref()==Some(&remove){entry.group=None;}}let _=this.owner.update(cx,|owner,cx|{owner.repository_tabs.groups.retain(|group|group!=&remove);for entry in &mut owner.repository_tabs.library{if entry.group.as_ref()==Some(&remove){entry.group=None;}}owner.save_repository_session(window,cx);});cx.notify();})))
                }))
                .children(self.session.library.clone().into_iter().enumerate().map(|(index,entry)|{
                    let path=entry.path.path();let remove=path.clone();
                    let identity=self.project_identity(&path);
                    let label=format!("{}{}{}",if entry.pinned{"★ "}else{""},identity,entry.group.as_ref().map_or(String::new(),|group|format!(" · {group}")));
                    let accessible=format!("{}repository {}{}",if entry.pinned{"Pinned "}else{""},identity,entry.group.as_ref().map_or(String::new(),|group|format!(", workspace {group}")));
                    div().flex().items_center().gap_2().child(workspace_text(("local-workspace-repository", index),label).aria_label(accessible.clone()).flex_1().min_w_0().truncate().tooltip(move|window,cx|Tooltip::new(accessible.clone()).build(window,cx)))
                        .child(button(("open-library-repo",index),"Open","",false).debug_selector(move||format!("open-library-repo-{index}")).accessibility_label(format!("Open repository {identity}")).on_click(cx.listener(move|this,_,window,cx|{window.close_dialog(cx);let _=this.owner.update(cx,|owner,cx|owner.open_repository_tab(path.clone(),window,cx));})))
                        .child(button(("remove-library-repo",index),"Remove","",false).accessibility_label(format!("Remove repository {identity} from local library; keep its folder and drafts")).tooltip("Remove from the local library; keep its folder and drafts").on_click(cx.listener(move|this,_,window,cx|{this.session.library.retain(|entry|entry.path.path()!=remove);let _=this.owner.update(cx,|owner,cx|{owner.repository_tabs.library.retain(|entry|entry.path.path()!=remove);owner.save_repository_session(window,cx);});cx.notify();})))
                }))))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    use crate::test_support::allocations;

    async fn settle_tab_test(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        for _ in 0..8 {
            cx.executor().run_until_parked();
            let tasks = cx.update(|_, cx| {
                app.update(cx, |app, _| {
                    [
                        app.task.take(),
                        app.status_task.take(),
                        app.integration_task.take(),
                        app.repository_tabs.save_pending.take(),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                })
            });
            if tasks.is_empty() {
                break;
            }
            for task in tasks {
                task.await;
            }
        }
        let (operations, preferences) = app.read_with(cx, |app, _| {
            (
                app.operations.submit_read(|| Ok(())),
                app.preferences_writer.submit_read(|| Ok(())),
            )
        });
        operations.await.unwrap().unwrap();
        preferences.await.unwrap().unwrap();
        cx.executor().run_until_parked();
        cx.update(|_, cx| {
            app.update(cx, |app, _| {
                app.automatic.reset();
                app._display_preferences_task = None;
            })
        });
    }

    /// " · unavailable" marks a tab while its repository cannot be opened
    /// and clears once it opens: a tab opened on a folder that holds no
    /// repository is marked, and opened again once one is there, it reads
    /// as its name alone.
    #[gpui::test]
    async fn an_unavailable_tab_clears_once_its_repository_opens(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let arriving = fixture.path().join("arriving");
        let label = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                let tab = &app.repository_tabs.tabs[1];
                (app.tab_label(tab).0, tab.error.is_some())
            })
        };
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(arriving.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        assert_eq!(label(cx), ("arriving · unavailable".into(), true));
        crate::views::tests::fixture_repository(&arriving);
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        settle_tab_test(&app, cx).await;
        assert_eq!(label(cx), ("arriving".into(), false), "opened, it clears");
    }

    /// Asks for a quiet read as the local watcher does for a change made
    /// outside the app, and waits for it to land.
    fn quiet_read_landed(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        quiet_read_of(
            app,
            cx,
            crate::local_refresh::LocalChange {
                worktree: true,
                ..Default::default()
            },
        );
    }

    /// As [`quiet_read_landed`], for the watcher's `change`.
    fn quiet_read_of(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        change: crate::local_refresh::LocalChange,
    ) {
        // The subscription is made on the operation thread.
        app.read_with(cx, |app, _| app.operations.drain());
        cx.executor().run_until_parked();
        app.update(cx, |app, _| app.automatic.report_local_change(change));
        quiet_reads_landed(app, cx);
    }

    /// Waits for the quiet reads the window has asked for to land, the one
    /// running and any waiting behind it, on the reader's thread they read
    /// on, which neither `run_until_parked` nor a drain of the operation
    /// thread waits for.
    fn quiet_reads_landed(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        for _ in 0..16 {
            cx.executor().run_until_parked();
            if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
                crate::views::tests::draw(cx);
                return;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
        }
        panic!("the quiet read never landed");
    }

    /// Ends Git's search for a repository at `fixture`, the temporary
    /// directory that holds the History window's repository, so that the
    /// repository with its `.git` moved away is no repository, wherever the
    /// temporary directory lies. Git looks for `.git` in each folder above
    /// one that has none, and a folder inside a work tree opens that work
    /// tree (`RepositorySession::open`): under a temporary directory inside a
    /// checkout, as one under its `.local/` is, Refresh with the `.git` away
    /// opened the checkout, 500 commits with no mark. Git refuses an invalid
    /// `.git` file rather than look further up.
    fn stop_discovery_at(fixture: &tempfile::TempDir) {
        std::fs::write(fixture.path().join(".git"), "no repository here\n").unwrap();
    }

    /// A tab's mark states its repository's current state: Refresh that
    /// cannot open the repository, its `.git` moved away, marks the tab; a
    /// quiet read the watcher asks for while it is still away keeps the
    /// mark, and so does one that reads the working state but not History
    /// (a branch naming a missing commit); once it reads whole, the next
    /// quiet read clears the mark and the opening error the window and the
    /// Projects page showed, and brings History back, as an explicit
    /// Refresh does, so closing the last tab shows Projects without that
    /// error. Only Refresh cleared it.
    #[gpui::test]
    async fn a_quiet_read_that_finds_the_repository_clears_its_tab_mark(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        stop_discovery_at(&fixture);
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let (git, away) = (root.join(".git"), root.join(".git-away"));
        let shown = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                let tab = &app.repository_tabs.tabs[0];
                (
                    app.tab_label(tab).0.ends_with(" · unavailable"),
                    app.error.is_some(),
                    app.commits.len(),
                )
            })
        };
        std::fs::rename(&git, &away).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(shown(cx), (true, true, 0), "Refresh cannot open it");
        quiet_read_landed(&app, cx);
        assert_eq!(
            shown(cx),
            (true, true, 0),
            "a quiet read that fails keeps it"
        );
        // Back, but with a branch naming a missing commit: the working state
        // reads, History does not, so the repository is not readable whole.
        std::fs::rename(&away, &git).unwrap();
        let broken = git.join("refs/heads/broken");
        std::fs::write(&broken, "1111111111111111111111111111111111111111\n").unwrap();
        quiet_read_landed(&app, cx);
        assert_eq!(
            shown(cx),
            (true, true, 0),
            "a quiet read whose History fails keeps it"
        );
        std::fs::remove_file(&broken).unwrap();
        quiet_read_landed(&app, cx);
        assert_eq!(
            shown(cx),
            (false, false, 1),
            "one that finds the repository clears it, History back"
        );
        // The Projects page's opening error went with it: closing the last
        // tab meets the person with no alert about the repository.
        let close = cx
            .debug_bounds("repository-tab-close-0")
            .expect("the tab's close button");
        cx.simulate_click(close.center(), gpui::Modifiers::default());
        crate::native_accessibility::written::forget();
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(app.read_with(cx, |app, _| app.page), AppPage::Projects);
        assert_eq!(
            crate::native_accessibility::written::announced_from("project-hub-error"),
            Vec::new(),
            "no stale opening error on Projects"
        );
    }

    /// A tab scoped to a branch is readable whole only once History reads
    /// that branch: back, but with the branch gone, a quiet read finds the
    /// working state and the branches but not the History the failed Open
    /// cleared (`Retained`), so the mark and the opening error stay rather
    /// than leave "No commits yet" with nothing to say why. Refresh opens
    /// All history then.
    #[gpui::test]
    async fn a_quiet_read_whose_scope_vanished_keeps_the_tab_mark(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        stop_discovery_at(&fixture);
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::views::tests::fixture_git(&root, &["branch", "topic"]);
        let topic = worker::Scope::Branch {
            name: "topic".into(),
            remote: false,
        };
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open(
                    root.clone(),
                    Some(("topic".into(), topic.clone())),
                    window,
                    cx,
                )
            })
        });
        crate::views::tests::settle(&app, cx).await;
        let shown = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                let tab = &app.repository_tabs.tabs[0];
                (
                    app.tab_label(tab).0.ends_with(" · unavailable"),
                    app.error.is_some(),
                    app.scope.as_ref().map(|scope| scope.1.clone()),
                )
            })
        };
        assert_eq!(
            shown(cx),
            (false, false, Some(topic.clone())),
            "scoped to topic"
        );
        let (git, away) = (root.join(".git"), root.join(".git-away"));
        std::fs::rename(&git, &away).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            shown(cx),
            (true, true, Some(topic.clone())),
            "Refresh cannot open it"
        );
        std::fs::rename(&away, &git).unwrap();
        crate::views::tests::fixture_git(&root, &["branch", "-D", "topic"]);
        quiet_read_landed(&app, cx);
        assert_eq!(
            shown(cx),
            (true, true, Some(topic)),
            "a quiet read whose scope vanished keeps it"
        );
        assert_eq!(app.read_with(cx, |app, _| app.commits.len()), 0);
    }

    /// Makes `folder`, the test's own temporary directory, a work tree with
    /// one commit, "Enclosing commit", and returns its root, so a repository
    /// inside it is nested in another wherever that directory lies. Git's
    /// own `init`, since the app's refuses a folder that holds anything.
    fn enclosing_work_tree(folder: &Path) -> PathBuf {
        use crate::views::tests::{fixture_git, fixture_identity};
        fixture_git(folder, &["init", "-q", "-b", "main"]);
        fixture_identity(folder);
        std::fs::write(folder.join("enclosing.txt"), "enclosing\n").unwrap();
        fixture_git(folder, &["add", "enclosing.txt"]);
        fixture_git(folder, &["commit", "-qm", "Enclosing commit"]);
        gitturtle_core::GitRepository::open(folder)
            .unwrap()
            .path()
            .to_owned()
    }

    /// What the window shows of its first tab: how many tabs, the tab's
    /// path, whether it is marked " · unavailable", the window's error, its
    /// repository's path and History's subjects.
    type FirstTab = (
        usize,
        PathBuf,
        bool,
        Option<String>,
        Option<PathBuf>,
        Vec<String>,
    );
    fn first_tab(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> FirstTab {
        app.read_with(cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            (
                app.repository_tabs.tabs.len(),
                tab.path.clone(),
                app.tab_label(tab).0.ends_with(" · unavailable"),
                app.error.clone(),
                app.repository.as_ref().map(|repo| repo.path().to_owned()),
                app.commits
                    .iter()
                    .map(|commit| commit.subject.clone())
                    .collect(),
            )
        })
    }

    /// A tab stays on the repository the person opened. The History
    /// fixture lies inside another work tree, and with its `.git` away Git
    /// looks in the folder above and finds that one. A quiet read the
    /// watcher asks for finds the repository gone, and the tab is read as
    /// Refresh reads it: marked " · unavailable", History saying "Could not
    /// open repository" and why, the enclosing repository's commit never
    /// shown and its root never the tab's, and the saved session keeps the
    /// tab's path. Refresh keeps that. Both once opened the enclosing
    /// repository in the tab, with no mark and no error, and the next write
    /// would have gone there. With its `.git` back, the next quiet read
    /// finds the repository and clears the mark.
    #[gpui::test]
    async fn a_tab_never_moves_to_the_work_tree_enclosing_its_repository(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let enclosing = enclosing_work_tree(fixture.path());
        assert_eq!(
            first_tab(&app, cx),
            (
                1,
                root.clone(),
                false,
                None,
                Some(root.clone()),
                vec!["fixture notes".into()]
            ),
            "the History fixture"
        );
        let (git, away) = (root.join(".git"), fixture.path().join("narrow-git-away"));
        std::fs::rename(&git, &away).unwrap();
        let gone = |cx: &mut VisualTestContext, after: &str| {
            let (tabs, path, marked, error, repository, subjects) = first_tab(&app, cx);
            assert_eq!(
                (tabs, path, marked, repository, subjects),
                (
                    1,
                    root.clone(),
                    true,
                    Some(root.clone()),
                    Vec::<String>::new()
                ),
                "{after}: marked, on its own repository"
            );
            let error = error.expect("the window says why");
            assert!(
                error.starts_with(&format!("The repository at {} is gone.", root.display()))
                    && error.contains(&format!(
                        "finds the repository at {} there",
                        enclosing.display()
                    )),
                "{after}: {error}"
            );
            assert!(
                cx.debug_bounds("empty-state").is_some(),
                "{after}: History says it could not open the repository"
            );
            let saved = app.read_with(cx, |app, _| app.repository_tabs.saved());
            assert_eq!(saved.tabs[0].path.path(), root, "{after}: the saved path");
            assert!(!saved.tabs[0].unresolved);
        };
        quiet_read_landed(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        gone(cx, "a quiet read");
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        gone(cx, "Refresh");
        quiet_read_landed(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        gone(cx, "a quiet read of a marked tab");

        std::fs::rename(&away, &git).unwrap();
        quiet_read_landed(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            first_tab(&app, cx),
            (
                1,
                root.clone(),
                false,
                None,
                Some(root),
                vec!["fixture notes".into()]
            ),
            "back, it clears"
        );
    }

    /// A tab restored at launch is a passive reopen too: saved by an older
    /// session, with no word on how it was opened, its path is the root its
    /// open resolved, and with that repository's `.git` gone, inside another
    /// work tree, the restore marks it " · unavailable" and says why rather
    /// than open the enclosing repository in it, and the session saved
    /// after keeps its path. It once restored the enclosing repository.
    #[gpui::test]
    async fn a_restored_tab_never_opens_the_work_tree_enclosing_its_repository(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let enclosing = enclosing_work_tree(fixture.path());
        let inner = crate::views::tests::fixture_repository(enclosing.join("inner"));
        let root = inner.path().to_owned();
        let saved: Session = serde_json::from_value(serde_json::json!({
            "version": 1,
            "tabs": [{ "path": root.to_str().expect("a UTF-8 fixture path") }],
            "active": 0,
        }))
        .unwrap();
        std::fs::rename(root.join(".git"), fixture.path().join("inner-git-away")).unwrap();
        let destination = isolated_session("restored-gone.json");
        let (app, mut window_cx) = open_tab_window(
            cx,
            root.clone(),
            saved,
            destination.clone(),
            size(px(1000.), px(680.)),
        );
        let cx = &mut window_cx;
        settle_tab_test(&app, cx).await;
        let (tabs, path, marked, error, repository, subjects) = first_tab(&app, cx);
        assert_eq!(
            (tabs, path, marked, repository, subjects),
            (1, root.clone(), true, None, Vec::<String>::new()),
            "restored marked, on its own repository"
        );
        let error = error.expect("the window says why");
        // No Git directories saved: a `.git` gone and one never there are the
        // same on disk, so it says what works for both.
        assert!(
            error.starts_with(&format!(
                "This tab's folder {} has no repository of its own:",
                root.display()
            )) && error.contains(&enclosing.display().to_string()),
            "{error}"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.save_repository_session(window, cx)));
        settle_tab_test(&app, cx).await;
        let after = Session::load_at(&destination).unwrap();
        assert_eq!(after.tabs.len(), 1);
        assert_eq!(after.tabs[0].path.path(), root, "the saved path stays");
    }

    /// A tab an older session saved on a folder inside a work tree, with no
    /// Git directories captured, has no repository of its own to bring
    /// back: the tab is marked, and its message says what the person can do
    /// about it, which Open Repository on that folder then does. It read
    /// "The repository at … is gone … Restore its .git and refresh" for
    /// good, about a folder that never held a `.git`.
    #[gpui::test]
    async fn a_saved_tab_inside_a_work_tree_says_what_can_be_done(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let enclosing = enclosing_work_tree(fixture.path());
        let folder = enclosing.join("sub").join("folder");
        std::fs::create_dir_all(&folder).unwrap();
        let saved: Session = serde_json::from_value(serde_json::json!({
            "version": 1,
            "tabs": [{ "path": folder.to_str().expect("a UTF-8 fixture path") }],
            "active": 0,
        }))
        .unwrap();
        let (app, mut window_cx) = open_tab_window(
            cx,
            folder.clone(),
            saved,
            isolated_session("saved-inside-a-work-tree.json"),
            size(px(1000.), px(680.)),
        );
        let cx = &mut window_cx;
        settle_tab_test(&app, cx).await;
        let (tabs, path, marked, error, repository, subjects) = first_tab(&app, cx);
        assert_eq!(
            (tabs, path, marked, repository, subjects),
            (1, folder.clone(), true, None, Vec::<String>::new()),
            "marked, and opening nothing"
        );
        let error = error.expect("the window says why");
        assert_eq!(
            error,
            format!(
                "This tab's folder {} has no repository of its own: Git finds the repository at {} there, which this tab does not open. To open that repository here, choose this folder with Open Repository; otherwise close this tab.",
                folder.display(),
                enclosing.display()
            )
        );

        // What it says works.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(folder.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        let (tabs, path, marked, error, repository, subjects) = first_tab(&app, cx);
        assert_eq!(
            (tabs, path, marked, error, repository),
            (1, enclosing.clone(), false, None, Some(enclosing.clone())),
            "Open Repository on the folder opens the repository found there"
        );
        assert_eq!(subjects, vec!["Enclosing commit".to_owned()]);
    }

    /// The session is saved as tabs change, and the person asks for none of
    /// those saves, so a preference writer that is merely behind is not a
    /// failure: a queue other saves have filled refuses the save in silence
    /// and it asks again until the newest session is stored. A refusal once
    /// read "Repository session: Background executor is busy" beneath the
    /// commit message.
    #[gpui::test]
    async fn a_writer_that_is_behind_never_fails_the_session_save(cx: &mut TestAppContext) {
        use crate::workspace::REFUSED_SAVE_RETRY;
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let destination = fixture.path().join("session-behind-a-busy-writer.json");
        let saved = destination.clone();
        cx.update(|_, cx| app.update(cx, |app, _| app.repository_tabs.save_path = Some(saved)));
        // The wait before a session save, and then the ask's own turn.
        let wait = std::time::Duration::from_millis(250);
        let pass = |time: std::time::Duration, cx: &mut VisualTestContext| {
            cx.executor().advance_clock(time);
            cx.executor().run_until_parked();
        };
        let answer = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| app.preferences_writer.drain());
            cx.executor().run_until_parked();
        };
        let error =
            |cx: &mut VisualTestContext| app.read_with(cx, |app, _| app.draft_save_error.clone());
        let change = |group: &str, cx: &mut VisualTestContext| {
            let group = group.to_owned();
            cx.update(|_, cx| app.update(cx, |app, _| app.repository_tabs.groups.push(group)));
        };
        pass(wait, cx);
        answer(cx);

        let held = app.read_with(cx, |app, _| app.preferences_writer.hold_reached());
        let fillers: Vec<_> = (0..8)
            .map(|_| app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(()))))
            .collect();
        for mut filler in fillers {
            assert!(matches!(filler.try_recv(), Ok(None)), "the queue fills");
        }
        change("Refused", cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.save_repository_session(window, cx)));
        pass(wait, cx);
        assert_eq!(error(cx), None);
        // Still full: refused again, and still nothing to report.
        pass(REFUSED_SAVE_RETRY, cx);
        pass(wait, cx);
        assert_eq!(error(cx), None);

        drop(held);
        answer(cx);
        let stored = |cx: &mut VisualTestContext| {
            answer(cx);
            Session::load_at(&destination)
                .map(|session| session.groups)
                .unwrap_or_default()
        };
        assert_eq!(
            stored(cx),
            Vec::<String>::new(),
            "only asking again brings it"
        );

        change("Newest", cx);
        pass(REFUSED_SAVE_RETRY, cx);
        pass(wait, cx);
        assert_eq!(
            stored(cx),
            vec!["Refused".to_owned(), "Newest".to_owned()],
            "the newest session reaches the store"
        );
        assert_eq!(error(cx), None);
    }

    /// A writer whose thread has stopped can never take the save, so asking
    /// again could not help: the window says so.
    #[gpui::test]
    async fn a_stopped_writer_still_reports_the_session_save(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        app.read_with(cx, |app, _| app.preferences_writer.drain());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.preferences_writer = crate::operations::SerialExecutor::stopped("stopped");
                app.save_repository_session(window, cx);
            })
        });
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(250));
        cx.executor().run_until_parked();
        let error = app
            .read_with(cx, |app, _| app.draft_save_error.clone())
            .expect("the failure is reported");
        assert!(error.starts_with("Repository session:"), "{error}");
        assert!(
            app.read_with(cx, |app, _| app.repository_tabs.save_retry.is_none()),
            "nothing asks again"
        );
    }

    /// A worktree's row opens that worktree's own root, as Git lists it, and
    /// nothing else: the linked worktree lies inside the main work tree,
    /// which lies inside another, and with its `.git` file gone Git finds
    /// the main work tree from its folder. Its row opens a tab of its own,
    /// marked " · unavailable" with the reason, and leaves the main tab as
    /// it was; it once opened the main work tree, the new tab folded into
    /// the main one, with no word that the worktree had gone.
    #[gpui::test]
    async fn a_worktree_row_never_opens_the_work_tree_enclosing_its_worktree(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        enclosing_work_tree(fixture.path());
        let linked = root.join("linked");
        crate::views::tests::fixture_git(
            &root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                linked.to_str().expect("a UTF-8 fixture path"),
            ],
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        let linked = linked.canonicalize().unwrap();
        let row = app.update(cx, |app, cx| {
            app.nav_mode = crate::NavMode::Worktrees;
            app.rebuild_navigation(cx);
            app.nav_rows
                .iter()
                .position(|row| {
                    matches!(row, crate::NavRow::Worktree(index)
                        if app.worktrees[*index].path.canonicalize().ok().as_ref() == Some(&linked))
                })
                .expect("the linked worktree's row")
        });
        std::fs::remove_file(linked.join(".git")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.activate_navigation(row, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            let tabs = &app.repository_tabs.tabs;
            assert_eq!(tabs.len(), 2, "a tab of its own");
            assert_eq!(tabs[0].path, root, "the main tab as it was");
            assert!(tabs[0].error.is_none());
            assert_eq!(app.repository_tabs.active, Some(1));
            assert_eq!(tabs[1].path.canonicalize().unwrap(), linked);
            assert!(app.tab_label(&tabs[1]).0.ends_with(" · unavailable"));
            let error = app.error.clone().expect("the window says why");
            assert!(
                error.starts_with(&format!(
                    "The repository at {} is gone.",
                    tabs[1].path.display()
                )) && error.contains(&format!("finds the repository at {} there", root.display())),
                "{error}"
            );
            assert!(app.repository.is_none() && app.commits.is_empty());
        });
    }

    /// The History fixture inside another work tree, with its `.git` moved
    /// away and quiet reads held, so only the read a test asks for runs:
    /// the window, its root, the enclosing root and how the first tab reads
    /// once that read has landed: marked, on its own repository, with a
    /// reason that names the enclosing repository, nothing of it shown.
    async fn gone_inside_another(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut VisualTestContext,
        PathBuf,
        PathBuf,
    ) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let enclosing = enclosing_work_tree(fixture.path());
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        (fixture, app, cx, root, enclosing)
    }
    fn assert_gone(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        tab: usize,
        root: &Path,
        enclosing: &Path,
        after: &str,
    ) {
        app.read_with(cx, |app, _| {
            let shown = &app.repository_tabs.tabs[tab];
            assert_eq!(app.repository_tabs.active, Some(tab), "{after}");
            assert_eq!(shown.path, root, "{after}: on its own repository");
            assert!(
                app.tab_label(shown).0.ends_with(" · unavailable"),
                "{after}: marked"
            );
            let error = app.error.clone().expect("the window says why");
            assert!(
                error.starts_with(&format!("The repository at {} is gone.", root.display()))
                    && error.contains(&format!(
                        "finds the repository at {} there",
                        enclosing.display()
                    )),
                "{after}: {error}"
            );
            assert!(app.commits.is_empty(), "{after}: no enclosing history");
            // The enclosing repository's status names its own file and the
            // tab's folder; the tab's own earlier status may still be shown.
            let enclosing_entry =
                |path: &Path| path == Path::new("enclosing.txt") || path.starts_with("narrow");
            assert!(
                app.work_status.as_ref().is_none_or(|status| {
                    !status
                        .entries
                        .iter()
                        .any(|entry| enclosing_entry(&entry.path))
                }),
                "{after}: no enclosing working state"
            );
        });
    }

    /// Show latest reads the shown repository's latest history from its own
    /// root and installs it in place: with the `.git` away inside another
    /// work tree, it once installed the enclosing repository's history in
    /// the tab. It now marks the tab and says why, as Refresh does.
    #[gpui::test]
    async fn show_latest_never_installs_the_enclosing_repository(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root, enclosing) = gone_inside_another(cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_latest_history(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "Show latest");
    }

    /// Refresh in Changes reads the working state on the operation thread:
    /// with the `.git` away inside another work tree, it once showed the
    /// enclosing repository's status rows under the tab. It now marks the
    /// tab and says why, as History's Refresh does, and shows none of them.
    #[gpui::test]
    async fn refresh_in_changes_never_shows_the_enclosing_status(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let enclosing = enclosing_work_tree(fixture.path());
        std::fs::write(fixture.path().join("enclosing.txt"), "changed\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        assert!(
            app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working
                && app.repository_tabs.tabs[0].error.is_none()),
            "in Changes, readable"
        );
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "Refresh in Changes");
        assert!(
            app.read_with(cx, |app, _| app.working_rows.is_empty()),
            "no enclosing rows"
        );
    }

    /// The History fixture with its `.git` moved away and no repository
    /// enclosing it: how the first tab reads once a read has found nothing
    /// there. Marked, on its own repository, saying the reasons any folder
    /// that is not a repository gets (`repository_access::explain`) in its
    /// mark and the window, never that the repository is gone, which is
    /// only for another repository Git finds at its path.
    fn assert_not_a_repository(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        root: &Path,
        after: &str,
    ) {
        app.read_with(cx, |app, _| {
            let shown = &app.repository_tabs.tabs[0];
            assert_eq!(shown.path, root, "{after}: on its own repository");
            assert!(
                app.tab_label(shown).0.ends_with(" · unavailable"),
                "{after}: marked"
            );
            let error = app.error.clone().expect("the window says why");
            assert!(
                error.starts_with("This folder is not an available Git repository.")
                    && error.contains(&format!("Folder: {}", root.display()))
                    && !error.contains("is gone"),
                "{after}: {error}"
            );
            assert_eq!(
                shown.error.as_ref(),
                Some(&error),
                "{after}: the mark's reason"
            );
            assert!(app.commits.is_empty(), "{after}: no history");
        });
    }

    /// With its `.git` gone and no repository enclosing its folder, Refresh
    /// fails as any open of a folder that is not a repository does, and the
    /// tab is marked with those reasons; the saved session keeps its path.
    #[gpui::test]
    async fn refresh_with_no_repository_found_says_the_folder_is_not_one(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_not_a_repository(&app, cx, &root, "Refresh");
        let saved = app.read_with(cx, |app, _| app.repository_tabs.saved());
        assert_eq!(saved.tabs[0].path.path(), root, "the saved path");
    }

    /// Show latest, with the tab's `.git` gone and no repository enclosing
    /// its folder, marks the tab as Refresh does, with the reasons any
    /// folder that is not a repository gets. It once left the tab unmarked,
    /// its history still shown, under "Could not read local history: …".
    #[gpui::test]
    async fn show_latest_with_no_repository_found_marks_the_tab(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_latest_history(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_not_a_repository(&app, cx, &root, "Show latest");
    }

    /// Refresh in Changes, with its `.git` gone and no repository enclosing
    /// its folder, marks the tab through its Open, which says the reasons
    /// any folder that is not a repository gets; Refresh again in the
    /// marked tab adds nothing beside them. It once said "Working tree: The
    /// repository at … is gone. Git finds no repository there" above them.
    #[gpui::test]
    async fn refresh_in_changes_with_no_repository_found_says_only_why(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        assert!(
            app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working
                && app.work_status.is_some()
                && app.repository_tabs.tabs[0].error.is_none()),
            "in Changes, readable"
        );
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        assert_not_a_repository(&app, cx, &root, "Refresh in Changes");
        assert_eq!(app.read_with(cx, |app, _| app.operation_error_text()), None);
        // The failed Open shows History; Changes again, and Refresh there.
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        let after = "Refresh in the marked tab's Changes";
        assert_not_a_repository(&app, cx, &root, after);
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Working, "{after}: in Changes");
            assert_eq!(app.operation_error, None, "{after}: nothing beside it");
            assert!(app.work_status.is_none(), "{after}: no status");
        });
    }

    /// A marked tab's explicit Refresh in Changes states the reason that is
    /// true now: marked with no repository found, once a work tree comes to
    /// enclose its folder the mark names what Git now finds, in the words an
    /// Open would use, and the window and Projects with it. The tab stays
    /// on its own repository and in Changes, and opens nothing else. Where
    /// nothing is found again, the mark goes back to those reasons. It once
    /// kept the mark's old reason.
    #[gpui::test]
    async fn refresh_in_changes_states_the_marked_tab_s_current_reason(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        assert_not_a_repository(&app, cx, &root, "marked by Refresh");
        let projects = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, cx| {
                app.hub.read(cx).shown_error().map(str::to_owned)
            })
        };
        assert!(projects(cx).is_some(), "Projects says the same");
        let refresh_in_changes = async |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
            app.read_with(cx, |app, _| app.operations.drain());
            crate::views::tests::settle(&app, cx).await;
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
            app.read_with(cx, |app, _| app.operations.drain());
            crate::views::tests::settle(&app, cx).await;
        };
        // Git starts finding another repository at the tab's folder. The
        // window's error follows the mark, whatever it said by then.
        let enclosing = enclosing_work_tree(fixture.path());
        cx.update(|_, cx| app.update(cx, |app, _| app.error = None));
        refresh_in_changes(cx).await;
        let after = "Refresh once Git finds another repository";
        assert_gone(&app, cx, 0, &root, &enclosing, after);
        app.read_with(cx, |app, _| {
            let shown = &app.repository_tabs.tabs[0];
            assert_eq!(shown.error, app.error, "{after}: the mark's reason");
            assert!(app.mode == WorkspaceMode::Working, "{after}: in Changes");
            assert_eq!(app.operation_error, None, "{after}: nothing beside it");
            assert_eq!(
                app.repository.as_ref().map(|repo| repo.path().to_owned()),
                Some(root.clone()),
                "{after}: its own repository, no other"
            );
        });
        assert_eq!(
            projects(cx),
            app.read_with(cx, |app, _| app.error.clone()),
            "{after}: Projects says the same"
        );

        // And back: nothing found again, the reasons a folder gets.
        std::fs::remove_dir_all(fixture.path().join(".git")).unwrap();
        refresh_in_changes(cx).await;
        assert_not_a_repository(&app, cx, &root, "Refresh once nothing is found again");
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Working, "in Changes");
            assert_eq!(app.operation_error, None, "nothing beside it");
        });
    }

    /// A read that found a reason for a tab's mark lands to find the mark
    /// gone, a quiet read having cleared it meanwhile: the read's own
    /// failure still says what it did, and marks nothing. Lost with the
    /// mark, nothing said why Changes had emptied.
    #[gpui::test]
    async fn a_marked_read_s_failure_says_what_it_did_once_the_mark_has_gone(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, _root) = settled_history_window(cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.working_read_failed(
                    worker::MarkedReadFailed {
                        reason: "the reason an Open gives".into(),
                        read: anyhow::anyhow!("the status read failed"),
                    }
                    .into(),
                    window,
                    cx,
                )
            })
        });
        assert_eq!(
            operation_error(&app, cx),
            Some("Working tree: the status read failed".into())
        );
        app.read_with(cx, |app, _| {
            assert!(app.repository_tabs.tabs[0].error.is_none(), "no mark made");
            assert_eq!(app.error, None, "no reason shown");
        });
    }

    /// A quiet read the watcher asks for, with the tab's `.git` gone and no
    /// repository enclosing its folder, reads the tab as Refresh reads it:
    /// marked, with the reasons any folder that is not a repository gets,
    /// and nothing beside them; a quiet read of the marked tab keeps that.
    /// It once left the tab unmarked, its history still shown, under
    /// "Local refresh:" and Git's own "not a git repository".
    #[gpui::test]
    async fn a_quiet_read_with_no_repository_found_marks_the_tab(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        // The window's own first reads, the rescan the watcher's setup asks
        // for among them, land before the `.git` goes: one running across
        // the move reads the working state and then fails reading History.
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        assert_eq!(app.read_with(cx, |app, _| app.operation_error_text()), None);
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        cx.update(|_, cx| app.update(cx, |app, _| app.automatic.hold_quiet_reads(false)));
        for after in ["a quiet read", "a quiet read of the marked tab"] {
            quiet_read_landed(&app, cx);
            crate::views::tests::settle(&app, cx).await;
            assert_not_a_repository(&app, cx, &root, after);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_error_text()),
                None,
                "{after}: nothing beside it"
            );
        }
    }

    /// `operation_error` as the window holds it.
    fn operation_error(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Option<String> {
        app.read_with(cx, |app, _| app.operation_error_text())
    }

    /// Has the shown tab's quiet read fail with `error`, as its reply lands.
    fn quiet_read_fails(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        error: impl Into<anyhow::Error>,
    ) {
        let error = error.into();
        cx.update(|window, cx| app.update(cx, |app, cx| app.quiet_read_failed(error, window, cx)));
    }

    /// A History window whose first reads have landed, quiet reads free to
    /// start, and the root of its repository.
    async fn settled_history_window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut VisualTestContext,
        PathBuf,
    ) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        cx.update(|_, cx| app.update(cx, |app, _| app.automatic.hold_quiet_reads(false)));
        (fixture, app, cx, root)
    }

    /// A quiet read that finds the marked tab's repository changed as it
    /// reopened it, at its own path, writes no banner: the mark says it,
    /// and such a banner once outlived the mark.
    #[gpui::test]
    async fn a_quiet_read_that_finds_it_changed_while_opening_writes_no_banner_beside_the_mark(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, _root, _third_git) = marked_by_a_swapped_git(cx).await;
        let (path, generation) = app.read_with(cx, |app, _| {
            (app.path.clone().expect("a repository"), app.generation)
        });
        quiet_read_fails(
            &app,
            cx,
            gitturtle_core::ChangedWhileOpening {
                found: Some(path.clone()),
                path,
            },
        );
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(operation_error(&app, cx), None, "the mark says it");
        app.read_with(cx, |app, _| {
            assert!(app.repository_tabs.tabs[0].error.is_some(), "still marked");
            assert_eq!(app.generation, generation, "a marked tab asks for no Open");
        });
    }

    /// The same read of an unmarked tab has no mark to say it: it keeps the
    /// banner, which the next quiet read that succeeds takes away, and runs
    /// no Open, which would leave Changes and cancel an open form for a
    /// failure that may pass.
    #[gpui::test]
    async fn a_quiet_read_that_finds_it_changed_while_opening_keeps_the_banner_on_an_unmarked_tab(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, root) = settled_history_window(cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        let generation = app.read_with(cx, |app, _| app.generation);
        quiet_read_fails(
            &app,
            cx,
            gitturtle_core::ChangedWhileOpening {
                found: Some(root.clone()),
                path: root.clone(),
            },
        );
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(
            operation_error(&app, cx),
            Some(format!(
                "Local refresh: The repository at {} changed while it was opened. Open it again.",
                root.display()
            )),
            "nothing else says it"
        );
        app.read_with(cx, |app, _| {
            assert_eq!(app.generation, generation, "no Open");
            assert!(app.mode == WorkspaceMode::Working, "still in Changes");
            assert!(app.repository_tabs.tabs[0].error.is_none(), "not marked");
        });
        quiet_read_landed(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(operation_error(&app, cx), None, "the next good read");
    }

    /// The next quiet read that succeeds takes a "Local refresh:" banner
    /// away, and a banner a write put in that slot, however it is worded,
    /// stays: a passive read never clears a write's failure.
    #[gpui::test]
    async fn the_next_quiet_read_takes_only_its_own_banner(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root) = settled_history_window(cx).await;
        let settle_read = async |app: &Entity<GitTurtle>, cx: &mut VisualTestContext| {
            quiet_read_landed(app, cx);
            crate::views::tests::settle(app, cx).await;
        };

        quiet_read_fails(&app, cx, anyhow::anyhow!("the read stopped"));
        let banner = "Local refresh: the read stopped".to_owned();
        assert_eq!(operation_error(&app, cx), Some(banner), "the banner");
        settle_read(&app, cx).await;
        assert_eq!(
            operation_error(&app, cx),
            None,
            "the next good read clears it"
        );

        // A write's failure in the same words stays, whichever read lands.
        let written = "Local refresh: a hook refused the commit".to_owned();
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.tab_operation_feedback(&root, None, Some(written.clone().into()), cx)
            })
        });
        settle_read(&app, cx).await;
        assert_eq!(
            operation_error(&app, cx),
            Some(written.clone()),
            "a write's stays"
        );
        quiet_read_fails(&app, cx, anyhow::anyhow!("the read stopped"));
        assert_eq!(
            operation_error(&app, cx),
            Some(written.clone()),
            "a read does not replace a write's failure"
        );
        settle_read(&app, cx).await;
        assert_eq!(
            operation_error(&app, cx),
            Some(written.clone()),
            "nor clear it"
        );

        // A write's failure over the read's banner stays too.
        cx.update(|_, cx| app.update(cx, |app, _| app.operation_error = None));
        quiet_read_fails(&app, cx, anyhow::anyhow!("the read stopped"));
        cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.tab_operation_feedback(&root, None, Some(written.clone().into()), cx)
            })
        });
        settle_read(&app, cx).await;
        assert_eq!(operation_error(&app, cx), Some(written), "over the banner");
    }

    /// A quiet read's banner draws the line core typed for its failure, its
    /// report whole behind Details…, and the next good read takes that
    /// failure by the whole of it: another failure with the same report and
    /// a line of its own is not the banner this read wrote, and stays.
    #[gpui::test]
    async fn a_quiet_read_s_banner_keeps_its_summary_and_goes_by_its_whole_failure(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, _root) = settled_history_window(cx).await;
        let report = "Local refresh: the whole report\nof what Git said";
        quiet_read_fails(
            &app,
            cx,
            gitturtle_core::NotARepository(
                "the whole report\nof what Git said".into(),
                Some("the line that says it".into()),
            ),
        );
        app.read_with(cx, |app, _| {
            let shown = app.operation_error.as_ref().expect("the banner");
            assert_eq!(
                shown.banner().as_ref(),
                "Local refresh: the line that says it"
            );
            assert_eq!(&**shown, report, "its report whole");
        });
        cx.update(|_, cx| {
            app.update(cx, |app, _| {
                app.operation_error = Some(crate::views::OperationFailure::summarized(
                    report.to_owned(),
                    Some("a write's own line".to_owned()),
                ));
            })
        });
        quiet_read_landed(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            let shown = app.operation_error.as_ref().expect("not the read's banner");
            assert_eq!(shown.banner().as_ref(), "a write's own line");
        });
    }

    /// A banner left behind by a tab switch comes back with its tab and
    /// still goes with that tab's next good read, whenever it lands.
    #[gpui::test]
    async fn a_local_refresh_banner_goes_with_its_tab(cx: &mut TestAppContext) {
        let (fixture, app, cx, _root) = settled_history_window(cx).await;
        let banner = "Local refresh: the read stopped".to_owned();
        quiet_read_fails(&app, cx, anyhow::anyhow!("the read stopped"));
        assert_eq!(
            operation_error(&app, cx),
            Some(banner.clone()),
            "the banner"
        );
        open_another_tab(&app, cx, &fixture.path().join("other")).await;
        assert_eq!(operation_error(&app, cx), None, "another tab's slot");
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        assert_eq!(operation_error(&app, cx), Some(banner), "back with its tab");
        // The switch starts the tab's rescan on the reader's thread, and the
        // watch it sets up on the operation thread asks for another as it
        // lands: both are waited for. `settle_tab_test` forgets a tab's quiet
        // reads as it ends, the banner they wrote among them: on a slow host
        // the rescan still read then, and was dropped with the banner it
        // would have taken.
        app.read_with(cx, |app, _| app.operations.drain());
        quiet_reads_landed(&app, cx);
        assert_eq!(
            operation_error(&app, cx),
            None,
            "its next good read clears it"
        );
    }

    /// A folder's permissions taken away until this drops, so a test that
    /// takes them leaves a temporary directory that can be removed, however
    /// it ends.
    #[cfg(unix)]
    struct DeniedFolder {
        path: PathBuf,
        mode: u32,
    }

    #[cfg(unix)]
    impl DeniedFolder {
        fn new(path: &Path) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path).unwrap().permissions().mode();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
            Self {
                path: path.to_owned(),
                mode,
            }
        }
    }

    #[cfg(unix)]
    impl Drop for DeniedFolder {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(self.mode))
                .unwrap();
        }
    }

    /// Whether this process is denied a mode-000 folder: root enters one.
    #[cfg(unix)]
    fn denied_a_locked_folder() -> bool {
        let locked = tempfile::tempdir().unwrap();
        let denied = DeniedFolder::new(locked.path());
        let refused = std::fs::read_dir(locked.path()).is_err();
        drop(denied);
        refused
    }

    /// Which folder's permissions a test takes away.
    #[cfg(unix)]
    #[derive(Clone, Copy)]
    enum Denied {
        /// The folder above the repository, so it cannot be reached.
        Parent,
        /// The repository's own folder, its metadata still readable from a
        /// parent that stays searchable.
        Repository,
    }

    #[cfg(unix)]
    impl Denied {
        fn apply(self, fixture: &tempfile::TempDir, root: &Path) -> DeniedFolder {
            DeniedFolder::new(match self {
                Self::Parent => fixture.path(),
                Self::Repository => root,
            })
        }
    }

    /// A folder whose parent lost search permission, read by a quiet read,
    /// could not be checked: the tab is not marked and keeps its history,
    /// and the banner says the folder could not be read and why, never that
    /// it is gone. Giving the permission back, the next read decides: it
    /// once read as a folder gone, marked until that read.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_quiet_read_that_cannot_check_the_folder_marks_nothing(cx: &mut TestAppContext) {
        quiet_read_cannot_check(cx, Denied::Parent).await;
    }

    /// The repository's own folder denied says the same, its metadata still
    /// readable: past a check of that alone, the failure came raw from
    /// Git.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_quiet_read_of_a_denied_repository_folder_marks_nothing(cx: &mut TestAppContext) {
        quiet_read_cannot_check(cx, Denied::Repository).await;
    }

    #[cfg(unix)]
    async fn quiet_read_cannot_check(cx: &mut TestAppContext, which: Denied) {
        if !denied_a_locked_folder() {
            return;
        }
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        cx.update(|_, cx| app.update(cx, |app, _| app.automatic.hold_quiet_reads(false)));
        let shows = app.read_with(cx, |app, _| app.commits.len());
        assert!(shows > 0, "history shown");

        let denied = which.apply(&fixture, &root);
        quiet_read_landed(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            let shown = &app.repository_tabs.tabs[0];
            assert!(shown.error.is_none(), "nothing marked");
            assert!(!app.tab_label(shown).0.ends_with(" · unavailable"));
            assert_eq!(app.error, None, "no failed Open");
            assert_eq!(app.commits.len(), shows, "history stays");
            let banner = app.operation_error.clone().expect("the read says why");
            assert!(
                banner.starts_with(&format!("Local refresh: Could not read {}", root.display()))
                    && banner.contains("Permission denied")
                    && !banner.contains("gone"),
                "{banner}"
            );
        });

        drop(denied);
        quiet_read_landed(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            let shown = &app.repository_tabs.tabs[0];
            assert!(shown.error.is_none(), "unmarked");
            assert_eq!(app.commits.len(), shows, "history stays");
        });
    }

    /// An explicit Refresh that cannot check the folder fails as an Open:
    /// the window says it could not be read, the tab keeps its state, marked
    /// nothing new, and the next read that finds the folder decides, bringing
    /// History back and taking the words with it.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_refresh_that_cannot_check_the_folder_marks_nothing_new(cx: &mut TestAppContext) {
        refresh_cannot_check(cx, Denied::Parent).await;
    }

    /// History's Refresh of the repository's own denied folder agrees with
    /// the quiet read: it once failed raw past the metadata check and marked
    /// the tab.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_refresh_of_a_denied_repository_folder_marks_nothing_new(cx: &mut TestAppContext) {
        refresh_cannot_check(cx, Denied::Repository).await;
    }

    #[cfg(unix)]
    async fn refresh_cannot_check(cx: &mut TestAppContext, which: Denied) {
        if !denied_a_locked_folder() {
            return;
        }
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        cx.update(|_, cx| app.update(cx, |app, _| app.automatic.hold_quiet_reads(false)));
        let shows = app.read_with(cx, |app, _| app.commits.len());
        assert!(shows > 0, "history shown");

        let denied = which.apply(&fixture, &root);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            let shown = &app.repository_tabs.tabs[0];
            assert!(shown.error.is_none(), "nothing marked");
            assert!(!app.tab_label(shown).0.ends_with(" · unavailable"));
            let error = app.error.clone().expect("the window says why");
            assert!(
                error.contains(&format!("Could not read {}", root.display()))
                    && error.contains("Permission denied")
                    && !error.contains("gone"),
                "{error}"
            );
        });

        drop(denied);
        quiet_read_landed(&app, cx);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.repository_tabs.tabs[0].error.is_none(), "unmarked");
            assert_eq!(app.error, None, "the read found it");
            assert_eq!(app.commits.len(), shows, "History back");
        });
    }

    /// A Refresh in Changes that cannot check the folder, its parent's
    /// search permission lost, fails its working read as
    /// `gitturtle_core::CouldNotCheck` and leaves no status and no read
    /// running: nothing claims a branch or a detached HEAD, the picker
    /// saying the failure and the way back
    /// (`GitTurtle::working_status_not_read`). The picker, Fetch, Pull, Push
    /// and the composer fell to "Detached HEAD" while Changes said "Changes
    /// unavailable". With the permission back, Refresh reads the branch
    /// again.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_refresh_in_changes_that_cannot_check_the_folder_claims_no_branch(
        cx: &mut TestAppContext,
    ) {
        if !denied_a_locked_folder() {
            return;
        }
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        // Tall enough for the composer's full form, with its branch line.
        cx.simulate_resize(size(px(1280.), px(1200.)));
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let picker = || crate::branch_actions::PICKER_LABEL.with(|label| label.borrow().clone());
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(picker().as_deref(), Some("main"), "read");

        let denied = Denied::Parent.apply(&fixture, &root);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(!app.shown_tab_unavailable(), "nothing marked");
            assert!(app.working_status_not_read(), "no status, no read running");
        });
        crate::views::tests::assert_claims_no_branch(
            &app,
            cx,
            &format!("Working tree: Could not read {}", root.display()),
            "a folder that cannot be checked",
        );

        drop(denied);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_eq!(picker().as_deref(), Some("main"), "read again");
    }

    /// A marked tab's navigation lists no branch, worktree or count read
    /// before the mark: in each mode it says what the mark says, as the
    /// header does, with the mark's way back in its tooltip, and its keys
    /// open nothing on what it no longer lists. It listed "LOCAL BRANCHES 1"
    /// over "main", of a repository the tab can no longer open, and Shift
    /// F10 opened that branch's actions. A Refresh that finds the
    /// repository again lists them again.
    #[gpui::test]
    async fn a_marked_tab_s_navigation_lists_nothing_from_before(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        cx.update(|_, cx| {
            crate::shortcuts::bind_keys(cx);
            crate::native_accessibility::bind_keys(cx);
        });
        let local = "nav-section/LOCAL BRANCHES/1";
        // Shift F10 on the branch row, which opens its actions where it is
        // listed; the menu, if any, is put away again.
        let manage = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.nav_cursor =
                        (app.nav_rows.iter()).position(|row| matches!(row, NavRow::Branch(..)));
                    window.focus(&app.nav_focus, cx);
                    cx.notify();
                })
            });
            draw(cx);
            cx.simulate_keystrokes("shift-f10");
            draw(cx);
            app.update(cx, |app, _| app.nav_menu.take().is_some())
        };
        draw(cx);
        assert!(
            cx.debug_bounds(local).is_some(),
            "the count before the mark"
        );
        assert!(
            cx.debug_bounds("navigation-unavailable").is_none(),
            "nothing to say before the mark"
        );
        assert!(manage(cx), "Shift F10 opens the branch's actions");

        let away = fixture.path().join("git-away");
        std::fs::rename(root.join(".git"), &away).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        settle(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.shown_tab_unavailable()),
            "marked"
        );
        assert!(
            app.read_with(cx, |app, _| !app.branches.is_empty()),
            "the branches read before the mark are kept"
        );
        assert!(!manage(cx), "Shift F10 opens nothing from before");
        for (mode, line) in [
            (NavMode::Local, "Branches unavailable"),
            (NavMode::Remote, "Remote branches unavailable"),
            (NavMode::Worktrees, "Worktrees unavailable"),
        ] {
            app.update(cx, |app, cx| {
                app.nav_mode = mode;
                app.rebuild_navigation(cx);
                cx.notify();
            });
            draw(cx);
            assert!(
                cx.debug_bounds("navigation-unavailable").is_some(),
                "{line}: said"
            );
            assert_eq!(
                crate::empty_state::shown("navigation-unavailable").as_deref(),
                Some(line),
                "{line}: its words"
            );
            let way_back = crate::native_accessibility::written::get("navigation-unavailable-line")
                .and_then(|line| line.node.description().map(str::to_owned));
            assert_eq!(
                way_back.as_deref(),
                Some(crate::branch_actions::UNAVAILABLE_WAY_BACK),
                "{line}: the mark's way back"
            );
            for row in ["nav-row-0", "nav-row-1", "nav-row-2"] {
                assert!(cx.debug_bounds(row).is_none(), "{line}: {row} not listed");
            }
            for section in [
                local,
                "nav-section/REMOTE BRANCHES/0",
                "nav-section/WORKTREES/1",
            ] {
                assert!(cx.debug_bounds(section).is_none(), "{line}: {section}");
            }
        }
        app.update(cx, |app, cx| {
            app.nav_mode = NavMode::Local;
            app.rebuild_navigation(cx);
            cx.notify();
        });

        std::fs::rename(&away, root.join(".git")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        settle(&app, cx).await;
        assert!(
            !app.read_with(cx, |app, _| app.shown_tab_unavailable()),
            "unmarked"
        );
        assert!(cx.debug_bounds(local).is_some(), "listed again");
        assert!(cx.debug_bounds("navigation-unavailable").is_none());
    }

    /// An explicit Open of a marked tab's folder whose snapshot names a
    /// repository another tab already shows, as the work tree enclosing the
    /// folder can be once the folder's own `.git` has gone, merges into that
    /// tab as a new tab's Open does (`tab_snapshot_accepted`): the marked tab
    /// goes, and the other is shown, read once and unmarked, so no two tabs
    /// hold one repository.
    #[gpui::test]
    async fn a_recapture_onto_a_repository_another_tab_shows_merges_into_it(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let enclosing = enclosing_work_tree(fixture.path());
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(enclosing.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        std::fs::rename(root.join(".git"), fixture.path().join("root-git-away")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        settle_tab_test(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.tabs.len(), 2);
            assert_eq!(app.repository_tabs.active, Some(0));
            assert!(
                app.repository_tabs.tabs[0].error.is_some(),
                "Refresh marks it"
            );
        });

        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        app.read_with(cx, |app, _| {
            let paths: Vec<_> = (app.repository_tabs.tabs.iter())
                .map(|tab| tab.path.clone())
                .collect();
            assert_eq!(paths, vec![enclosing.clone()], "the marked tab goes");
            assert_eq!(app.repository_tabs.active, Some(0), "the other shown");
            assert_eq!(app.path.as_ref(), Some(&enclosing));
            assert!(!app.shown_tab_unavailable(), "unmarked");
            assert_eq!(app.error, None);
        });
    }

    /// A cold tab shown again is opened by its captured root: with its
    /// `.git` away inside another work tree, the switch once opened the
    /// enclosing repository in it. It now shows the tab marked, on its own
    /// repository, and leaves the other tab as it was.
    #[gpui::test]
    async fn a_cold_switch_never_opens_the_enclosing_repository(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let enclosing = enclosing_work_tree(fixture.path());
        let other = crate::views::tests::fixture_repository(fixture.path().join("other"));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(other.path().to_owned(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        app.update(cx, |app, _| app.repository_tabs.tabs[0].warm = None);
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "a cold switch");
        app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.tabs.len(), 2);
            assert_eq!(app.repository_tabs.tabs[1].path, other.path());
        });
    }

    /// A cold tab shown again names the Git directory it captured at its
    /// root, which the reopen must find there: with its `.git` swapped for
    /// a third repository's gitfile while another tab was shown, the switch
    /// once opened that third repository in it, the root being the same.
    #[gpui::test]
    async fn a_cold_switch_never_takes_a_swapped_git_entry(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let other = crate::views::tests::fixture_repository(fixture.path().join("other"));
        let third = crate::views::tests::fixture_repository(fixture.path().join("third"));
        let third_git = gitturtle_core::GitRepository::open(third.path())
            .unwrap()
            .git_dir()
            .to_owned();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(other.path().to_owned(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        app.update(cx, |app, _| app.repository_tabs.tabs[0].warm = None);
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", third_git.display()),
        )
        .unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_gone(
            &app,
            cx,
            0,
            &root,
            &third_git,
            "a cold switch to a swapped .git",
        );
    }

    /// Regaining focus with the tab's `.git` gone and no repository
    /// enclosing its folder sets up the watcher again and asks for a quiet
    /// read: the tab is marked with the reasons any folder that is not a
    /// repository gets, and nothing says "Some changes need Refresh" beside
    /// them. The watcher's setup once failed as Git's own "not a git
    /// repository" into that warning.
    #[gpui::test]
    async fn regained_focus_with_no_repository_found_says_only_why(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        // What the window's activation observer asks for (`main.rs`).
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.ensure_local_watcher(true, window, cx);
                app.queue_automatic_refresh(
                    crate::local_refresh::LocalChange {
                        rescan: true,
                        ..Default::default()
                    },
                    window,
                    cx,
                );
            })
        });
        for _ in 0..4 {
            app.read_with(cx, |app, _| app.operations.drain());
            crate::views::tests::settle(&app, cx).await;
        }
        assert_not_a_repository(&app, cx, &root, "regained focus");
        app.read_with(cx, |app, _| {
            assert_eq!(app.operation_error, None, "nothing beside the mark");
            assert_eq!(
                app.automatic.watch_warning(),
                None,
                "the watcher's setup marked the tab instead"
            );
        });
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("local-refresh-warning").is_none(),
            "no \"Some changes need Refresh\" beside the mark"
        );
    }

    /// The watcher's setup failing after Git reported the administration
    /// directories, its watch refused as when the host's inotify instances
    /// run out, warns only while the tab's repository is still the one at
    /// its path. With the tab's `.git` swapped for a third repository's
    /// gitfile, whose directories Git then reports, the tab is marked and
    /// names that repository, with no "Some changes need Refresh" beside
    /// the mark. The refused watch once warned there and left the tab
    /// unmarked, and with no watcher nothing marked it later.
    #[gpui::test]
    async fn a_refused_watch_warns_only_while_the_repository_is_still_there(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let third = crate::views::tests::fixture_repository(fixture.path().join("third"));
        let third_git = gitturtle_core::GitRepository::open(third.path())
            .unwrap()
            .git_dir()
            .to_owned();
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let refuse_watch = |app: &Entity<GitTurtle>, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.watch_through(crate::local_refresh::Backend::Unavailable, window, cx)
                })
            });
        };

        refuse_watch(&app, cx);
        for _ in 0..2 {
            app.read_with(cx, |app, _| app.operations.drain());
            crate::views::tests::settle(&app, cx).await;
        }
        app.read_with(cx, |app, _| {
            let shown = &app.repository_tabs.tabs[0];
            assert!(
                !app.tab_label(shown).0.ends_with(" · unavailable"),
                "still there: unmarked"
            );
            assert_eq!(app.error, None, "still there");
            let warning = app
                .automatic
                .watch_warning()
                .expect("still there: the watcher's warning");
            assert!(
                warning.starts_with("Automatic refresh could not start.")
                    && warning.contains("Create native local-change watcher"),
                "{warning}"
            );
        });
        assert!(
            cx.debug_bounds("local-refresh-warning").is_some(),
            "still there: \"Some changes need Refresh\""
        );

        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", third_git.display()),
        )
        .unwrap();
        refuse_watch(&app, cx);
        for _ in 0..2 {
            app.read_with(cx, |app, _| app.operations.drain());
            crate::views::tests::settle(&app, cx).await;
        }
        let after = "a refused watch of a swapped .git";
        assert_gone(&app, cx, 0, &root, &third_git, after);
        app.read_with(cx, |app, _| {
            assert_eq!(
                app.automatic.watch_warning(),
                None,
                "{after}: marked instead"
            );
            assert_eq!(
                app.operation_error, None,
                "{after}: nothing beside the mark"
            );
        });
        assert!(
            cx.debug_bounds("local-refresh-warning").is_none(),
            "{after}: no \"Some changes need Refresh\" beside the mark"
        );
    }

    /// A tab restored at launch is reopened by its saved root as it was
    /// saved, never resolved again: with that root turned into a link to
    /// another repository while the app was closed, the restore marks the
    /// tab " · unavailable" and says why, on its own path, rather than open
    /// the other repository in it.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_restored_tab_whose_root_became_a_link_is_marked_not_switched(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let repository = crate::views::tests::fixture_repository(fixture.path().join("repository"));
        let other = crate::views::tests::fixture_repository(fixture.path().join("other"));
        let root = repository.path().to_owned();
        let saved: Session = serde_json::from_value(serde_json::json!({
            "version": 1,
            "tabs": [{ "path": root.to_str().expect("a UTF-8 fixture path") }],
            "active": 0,
        }))
        .unwrap();
        std::fs::rename(&root, fixture.path().join("moved")).unwrap();
        std::os::unix::fs::symlink(other.path(), &root).unwrap();
        let destination = isolated_session("restored-linked.json");
        let (app, mut window_cx) = open_tab_window(
            cx,
            root.clone(),
            saved,
            destination,
            size(px(1000.), px(680.)),
        );
        let cx = &mut window_cx;
        settle_tab_test(&app, cx).await;
        let (tabs, path, marked, error, repository, subjects) = first_tab(&app, cx);
        assert_eq!(
            (tabs, path, marked, repository, subjects),
            (1, root.clone(), true, None, Vec::<String>::new()),
            "restored marked, on its own path"
        );
        let error = error.expect("the window says why");
        assert!(
            error.starts_with(&format!("The repository at {} is gone.", root.display()))
                && error.contains(&other.path().display().to_string()),
            "{error}"
        );
    }

    /// A tab restored at launch whose saved root reaches its repository
    /// through a folder that is a link, as macOS reaches every temporary
    /// folder through `/var` (a link to `/private/var`), opens that
    /// repository unmarked and takes its real root, and Refresh keeps it; a
    /// tab saved at its real root with its Git directories, whose folder
    /// was then moved and linked back from its old place, opens the same
    /// way when it is switched to cold. Compared as saved, both were marked
    /// " · unavailable", where they opened before.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_restored_tab_reached_through_a_linked_folder_opens(cx: &mut TestAppContext) {
        use crate::views::tests::{fixture_git, fixture_repository};
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        for folder in ["real", "kept"] {
            std::fs::create_dir(base.join(folder)).unwrap();
        }
        let first = fixture_repository(base.join("real/first"));
        let second = fixture_repository(base.join("kept/second"));
        for (repository, subject) in [(&first, "First fixture"), (&second, "Second fixture")] {
            fixture_git(
                repository.path(),
                &["commit", "--allow-empty", "-m", subject],
            );
        }
        std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();
        let through = base.join("link/first");
        let captured = worker::GitDirectories::of(&second);
        let tab = |path: &Path, git_directories| SavedTab {
            path: SavedPath::new(path),
            bookmark: Bookmark::default(),
            unresolved: false,
            git_directories,
        };
        let saved = Session {
            version: 1,
            active: 0,
            tabs: vec![
                tab(&through, None),
                tab(
                    second.path(),
                    Some(SavedGitDirectories {
                        git_dir: SavedPath::new(&captured.git_dir),
                        common: SavedPath::new(&captured.common),
                        git_dir_inode: captured.inodes.git_dir,
                        common_inode: captured.inodes.common,
                        git_dir_birth: captured.inodes.git_dir_birth,
                        common_birth: captured.inodes.common_birth,
                    }),
                ),
            ],
            ..Default::default()
        };
        // Moved on the one filesystem and linked back: its inodes are kept.
        std::fs::rename(base.join("kept"), base.join("moved")).unwrap();
        std::os::unix::fs::symlink(base.join("moved"), base.join("kept")).unwrap();
        let (app, mut window_cx) = open_tab_window(
            cx,
            through,
            saved,
            isolated_session("restored-through-a-link.json"),
            size(px(1000.), px(680.)),
        );
        let cx = &mut window_cx;
        let opened = |cx: &mut VisualTestContext, index: usize, root: &Path, subject: &str| {
            app.read_with(cx, |app, _| {
                assert_eq!(app.repository_tabs.active, Some(index), "{subject}");
                let tab = &app.repository_tabs.tabs[index];
                assert!(
                    !app.tab_label(tab).0.ends_with(" · unavailable"),
                    "{subject}: marked, {:?}",
                    app.error
                );
                assert_eq!(app.error, None, "{subject}");
                assert_eq!(tab.path, root, "{subject}: the tab takes its real root");
                assert_eq!(
                    app.repository.as_ref().map(|repository| repository.path()),
                    Some(root),
                    "{subject}"
                );
                assert_eq!(
                    app.commits
                        .iter()
                        .map(|commit| commit.subject.as_str())
                        .collect::<Vec<_>>(),
                    [subject]
                );
            });
        };
        settle_tab_test(&app, cx).await;
        opened(cx, 0, first.path(), "First fixture");
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        settle_tab_test(&app, cx).await;
        opened(cx, 0, first.path(), "First fixture");

        let moved = base.join("moved/second");
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        settle_tab_test(&app, cx).await;
        opened(cx, 1, &moved, "Second fixture");
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        settle_tab_test(&app, cx).await;
        opened(cx, 1, &moved, "Second fixture");
    }

    /// A folder that is a link to a repository's own folder opens that
    /// repository in a tab that takes its real root, as Git reports it;
    /// Refresh opens it again, and so does the restore of the session saved
    /// after it, unmarked.
    #[cfg(unix)]
    #[gpui::test]
    async fn an_opened_link_to_a_repository_folder_refreshes_and_restores(cx: &mut TestAppContext) {
        use crate::views::tests::{fixture_git, fixture_repository};
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let repository = fixture_repository(base.join("repository"));
        fixture_git(
            repository.path(),
            &["commit", "--allow-empty", "-m", "Linked fixture"],
        );
        let root = repository.path().to_owned();
        let link = base.join("link-to-repository");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        let destination = isolated_session("opened-through-a-link.json");
        let opened = |app: &Entity<GitTurtle>, cx: &mut VisualTestContext, after: &str| {
            assert_eq!(
                first_tab(app, cx),
                (
                    1,
                    root.clone(),
                    false,
                    None,
                    Some(root.clone()),
                    vec!["Linked fixture".to_owned()]
                ),
                "{after}"
            );
        };
        let (app, mut window_cx) = open_tab_window(
            cx,
            link,
            Session::default(),
            destination.clone(),
            size(px(1000.), px(680.)),
        );
        settle_tab_test(&app, &mut window_cx).await;
        opened(&app, &mut window_cx, "opened");
        window_cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        settle_tab_test(&app, &mut window_cx).await;
        opened(&app, &mut window_cx, "Refresh");
        window_cx
            .update(|window, cx| app.update(cx, |app, cx| app.save_repository_session(window, cx)));
        settle_tab_test(&app, &mut window_cx).await;

        let saved = Session::load_at(&destination).unwrap();
        let (restored, mut window_cx) = open_tab_window(
            cx,
            saved.tabs[0].path.path(),
            saved,
            isolated_session("restored-after-a-link.json"),
            size(px(1000.), px(680.)),
        );
        settle_tab_test(&restored, &mut window_cx).await;
        opened(&restored, &mut window_cx, "restored");
    }

    /// A malformed saved `git_directories` degrades alone to none captured:
    /// the session loads every tab and its library, a well-formed tab's
    /// directories with them. Failing the whole file, it once dropped every
    /// tab, the library and its groups.
    #[test]
    fn a_malformed_saved_git_directory_degrades_alone() {
        let file = isolated_session("malformed-directories.json");
        std::fs::create_dir_all(file.parent().expect("a folder")).unwrap();
        let session = serde_json::json!({
            "version": 1,
            "tabs": [
                { "path": "/a", "git_directories": 5 },
                { "path": "/b", "git_directories": { "git_dir": "/b/.git" } },
                { "path": "/c", "git_directories": { "git_dir": "/c/.git", "common": "/c/.git" } },
                { "path": "/d", "git_directories": { "git_dir": "/d/.git", "common": "/d/.git", "git_dir_inode": "x" } },
                { "path": "/e", "git_directories": { "git_dir": "/e/.git", "common": "/e/.git", "git_dir_inode": 7, "common_inode": 8 } },
            ],
            "active": 1,
            "library": [{ "path": "/b" }],
        });
        std::fs::write(&file, serde_json::to_vec(&session).unwrap()).unwrap();
        let loaded = Session::load_at(&file).expect("the session loads");
        assert_eq!(loaded.tabs.len(), 5);
        assert_eq!(loaded.active, 1);
        assert_eq!(loaded.library.len(), 1);
        for index in [0, 1, 3] {
            assert!(loaded.tabs[index].git_directories.is_none(), "{index}");
        }
        let state = State::from_session(loaded);
        assert_eq!(
            state.tabs[2].git_dir,
            Some(worker::GitDirectories {
                git_dir: "/c/.git".into(),
                common: "/c/.git".into(),
                inodes: worker::DirectoryInodes::default(),
                restored: true,
            }),
            "saved before inodes were kept: nothing to compare"
        );
        assert_eq!(
            state.tabs[4].git_dir.as_ref().map(|tab| tab.inodes),
            Some(worker::DirectoryInodes {
                git_dir: Some(7),
                common: Some(8),
                ..Default::default()
            })
        );
    }

    /// Each Git directory's identity, as the window compares a capture
    /// with the repository its status was read from: its inode and, where
    /// both opens read one, its birth time, so an inode number taken again
    /// after a delete is another repository and a filesystem that records
    /// no birth time compares the inode alone. At other paths only these
    /// tell, so both opens must have read the inodes: equal birth times
    /// without them are not the same repository.
    #[test]
    fn a_git_directory_s_identity_is_its_inode_and_birth_time() {
        let directories = |at: &str, inode: Option<u64>, birth: Option<u64>| {
            let birth = birth.map(std::time::Duration::from_secs);
            worker::GitDirectories {
                git_dir: PathBuf::from(at).join(".git"),
                common: PathBuf::from(at).join(".git"),
                inodes: worker::DirectoryInodes {
                    git_dir: inode,
                    common: inode,
                    git_dir_birth: birth,
                    common_birth: birth,
                },
                restored: false,
            }
        };
        let shown = directories("/x/repo", Some(7), Some(100));
        for (found, same, what) in [
            (
                directories("/x/repo", Some(7), Some(100)),
                true,
                "unchanged",
            ),
            (
                directories("/x/repo", Some(7), Some(200)),
                false,
                "an inode taken again",
            ),
            (
                directories("/x/repo", Some(7), None),
                true,
                "no birth time read",
            ),
            (
                directories("/x/repo", Some(8), Some(100)),
                false,
                "another inode",
            ),
            (
                directories("/x/repo", None, None),
                true,
                "nothing read to compare",
            ),
            (
                directories("/y/repo", Some(7), Some(100)),
                true,
                "moved and linked back",
            ),
            (
                directories("/y/repo", Some(7), Some(200)),
                false,
                "elsewhere, born later",
            ),
            (
                directories("/y/repo", Some(8), Some(100)),
                false,
                "another repository",
            ),
            (
                directories("/y/repo", Some(7), None),
                true,
                "elsewhere, no birth time read",
            ),
            (
                directories("/y/repo", None, None),
                false,
                "elsewhere, nothing read here",
            ),
        ] {
            assert_eq!(same_git_directories(&shown, &found), same, "{what}");
        }
        let unknown = directories("/x/repo", None, None);
        for (found, what) in [
            (
                directories("/y/repo", None, None),
                "elsewhere, nothing read either side",
            ),
            (
                directories("/y/repo", Some(7), Some(100)),
                "elsewhere, nothing read there",
            ),
        ] {
            assert!(!same_git_directories(&unknown, &found), "{what}");
        }
        let mut common_later = shown.clone();
        common_later.inodes.common_birth = Some(std::time::Duration::from_secs(200));
        assert!(
            !same_git_directories(&shown, &common_later),
            "the common directory's inode taken again"
        );
        assert!(
            !same_git_directories(
                &directories("/x/repo", None, Some(100)),
                &directories("/y/repo", None, Some(100)),
            ),
            "elsewhere, a birth time without an inode"
        );
    }

    /// A session saved before birth times were kept restores its tabs and
    /// compares their inodes, as it did: the tab opens where its repository
    /// is the one it saved, and is marked where a re-clone at its path took
    /// another inode. The birth times a session saves now are optional
    /// fields such a session lacks.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_session_saved_before_birth_times_compares_the_inodes(cx: &mut TestAppContext) {
        use crate::views::tests::{fixture_git, fixture_repository};
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let repository = fixture_repository(base.join("repository"));
        fixture_git(
            repository.path(),
            &["commit", "--allow-empty", "-m", "Before"],
        );
        let root = repository.path().to_owned();
        let saved = saved_with_inodes(&repository, "before-births-saved.json");
        let mut older = serde_json::to_value(&saved).unwrap();
        let directories = &mut older["tabs"][0]["git_directories"];
        let fields = directories.as_object_mut().expect("saved with the tab");
        if repository.git_dir_birth().is_some() {
            assert!(fields.contains_key("git_dir_birth"), "{fields:?}");
        }
        fields.remove("git_dir_birth");
        fields.remove("common_birth");
        let older: Session = serde_json::from_value(older).unwrap();
        let directories = older.tabs[0].git_directories.as_ref().expect("loaded");
        assert_eq!(
            (directories.git_dir_birth, directories.common_birth),
            (None, None),
            "none saved"
        );
        assert_eq!(directories.git_dir_inode, repository.git_dir_inode());
        let restore = |cx: &mut TestAppContext, name: &str| {
            open_tab_window(
                cx,
                root.clone(),
                older.clone(),
                isolated_session(name),
                size(px(1000.), px(680.)),
            )
        };

        let (app, mut window_cx) = restore(cx, "before-births-first.json");
        settle_tab_test(&app, &mut window_cx).await;
        app.read_with(&window_cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            assert!(tab.error.is_none(), "{:?}", tab.error);
            assert_eq!(app.commits[0].subject, "Before");
        });

        // Moved aside rather than deleted, which could free the inode for
        // the new `.git` to take.
        std::fs::rename(&root, base.join("before")).unwrap();
        let cloned = fixture_repository(&root);
        fixture_git(cloned.path(), &["commit", "--allow-empty", "-m", "After"]);
        assert_ne!(cloned.git_dir_inode(), repository.git_dir_inode());
        let (app, mut window_cx) = restore(cx, "before-births-second.json");
        settle_tab_test(&app, &mut window_cx).await;
        assert_marked_for(&app, &mut window_cx, &root, cloned.git_dir());
    }

    /// A syntax error inside `git_directories` is a malformed file, reported
    /// as a syntax error in any other field is; only a value that parses
    /// but has the wrong shape degrades alone. Swallowed, a token that failed
    /// part way let the rest of the file load as if the field were absent.
    #[test]
    fn a_syntax_error_in_saved_git_directories_is_a_malformed_file() {
        let file = isolated_session("malformed-syntax.json");
        std::fs::create_dir_all(file.parent().expect("a folder")).unwrap();
        for malformed in [
            br#"{"version":1,"tabs":[{"path":"/a","git_directories":tx}],"active":0}"#.as_slice(),
            br#"{"version":1,"tabs":[{"path":tx}],"active":0}"#,
        ] {
            std::fs::write(&file, malformed).unwrap();
            let error = Session::load_at(&file).expect_err("the file is malformed");
            assert!(
                error
                    .downcast_ref::<serde_json::Error>()
                    .is_some_and(serde_json::Error::is_syntax),
                "{error:#}"
            );
        }
    }

    /// A tab's captured Git directories are saved with the session and
    /// named by its first reopen after a restart: a linked worktree's
    /// `commondir` re-pointed at another repository, or a `.git` swapped for
    /// a third repository's gitfile, while the app was closed marks the tab
    /// and says what Git finds, rather than take the other repository's refs
    /// and objects with the root unchanged. A session saved without them, as
    /// every older one, loads as it did.
    #[gpui::test]
    async fn a_restored_tab_names_the_git_directories_it_saved(cx: &mut TestAppContext) {
        use crate::views::tests::{fixture_git, fixture_repository};
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let main = fixture_repository(fixture.path().join("main"));
        let linked = fixture.path().join("linked");
        fixture_git(
            main.path(),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().expect("a UTF-8 fixture path"),
            ],
        );
        let linked = gitturtle_core::GitRepository::open(&linked).unwrap();
        let swapped = fixture_repository(fixture.path().join("swapped"));
        let other = fixture_repository(fixture.path().join("other"));
        let third = fixture_repository(fixture.path().join("third"));
        let tab = |path: &Path| serde_json::json!({ "path": path.to_str().expect("UTF-8") });
        let older: Session = serde_json::from_value(serde_json::json!({
            "version": 1,
            "tabs": [tab(linked.path()), tab(swapped.path())],
            "active": 0,
        }))
        .unwrap();
        assert!(older.tabs.iter().all(|tab| tab.git_directories.is_none()));
        let mut state = State::from_session(older);
        assert!(
            state
                .tabs
                .iter()
                .all(|tab| tab.resolved && tab.git_dir.is_none()),
            "an older session loads as it did"
        );
        state.tabs[0].git_dir = Some(worker::GitDirectories::of(&linked));
        state.tabs[1].git_dir = Some(worker::GitDirectories::of(&swapped));
        let file = isolated_session("saved-directories.json");
        state.saved().save_at(&file).unwrap();
        let saved = Session::load_at(&file).unwrap();
        assert!(
            saved.tabs.iter().all(|tab| tab.git_directories.is_some()),
            "saved with each tab"
        );

        std::fs::write(
            linked.git_dir().join("commondir"),
            format!("{}\n", other.git_dir().display()),
        )
        .unwrap();
        std::fs::rename(
            swapped.path().join(".git"),
            fixture.path().join("swapped-git-away"),
        )
        .unwrap();
        std::fs::write(
            swapped.path().join(".git"),
            format!("gitdir: {}\n", third.git_dir().display()),
        )
        .unwrap();
        let (app, mut window_cx) = open_tab_window(
            cx,
            linked.path().to_owned(),
            saved,
            isolated_session("saved-directories-after.json"),
            size(px(1000.), px(680.)),
        );
        let cx = &mut window_cx;
        settle_tab_test(&app, cx).await;
        let marked = |cx: &mut VisualTestContext, index: usize, root: &Path, found: &Path| {
            app.read_with(cx, |app, _| {
                assert_eq!(app.repository_tabs.active, Some(index));
                let tab = &app.repository_tabs.tabs[index];
                assert_eq!(tab.path, root);
                assert!(app.tab_label(tab).0.ends_with(" · unavailable"), "marked");
                let error = app.error.clone().expect("the window says why");
                assert!(
                    error.starts_with(&format!("The repository at {} is gone.", root.display()))
                        && error.contains(&found.display().to_string()),
                    "{error}"
                );
                assert!(app.commits.is_empty(), "nothing of the other repository");
            });
        };
        marked(cx, 0, linked.path(), other.git_dir());
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        settle_tab_test(&app, cx).await;
        marked(cx, 1, swapped.path(), third.git_dir());
    }

    /// A session saved with `repository`'s tab as it holds it once its
    /// snapshot is accepted, read back from a file as a launch reads it.
    #[cfg(unix)]
    fn saved_with_inodes(repository: &gitturtle_core::GitRepository, name: &str) -> Session {
        let path = repository.path().to_str().expect("UTF-8");
        let older: Session = serde_json::from_value(serde_json::json!({
            "version": 1,
            "tabs": [{ "path": path }],
            "active": 0,
        }))
        .unwrap();
        let mut state = State::from_session(older);
        let captured = worker::GitDirectories::of(repository);
        state.tabs[0].git_dir = Some(captured.clone());
        let file = isolated_session(name);
        state.saved().save_at(&file).unwrap();
        let saved = Session::load_at(&file).unwrap();
        let directories = saved.tabs[0]
            .git_directories
            .as_ref()
            .expect("saved with the tab");
        assert_eq!(directories.git_dir_inode, captured.inodes.git_dir);
        assert_eq!(directories.common_inode, captured.inodes.common);
        assert_eq!(directories.git_dir_birth, captured.inodes.git_dir_birth);
        assert_eq!(directories.common_birth, captured.inodes.common_birth);
        saved
    }

    /// `app`'s only tab, marked for `found`, shows nothing of it.
    #[cfg(unix)]
    fn assert_marked_for(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        root: &Path,
        found: &Path,
    ) {
        app.read_with(cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            assert_eq!(tab.path, root);
            assert!(app.tab_label(tab).0.ends_with(" · unavailable"), "marked");
            let error = app.error.clone().expect("the window says why");
            assert!(
                error.starts_with(&format!("The repository at {} is gone.", root.display()))
                    && error.contains(&found.display().to_string()),
                "{error}"
            );
            assert!(app.commits.is_empty(), "nothing of the other repository");
        });
    }

    /// A folder above a restored tab's root, re-pointed while the app was
    /// closed to another of the person's repositories with the root's folder
    /// name, reaches the same paths and is told by the inodes the session
    /// saved: the tab is marked as for a swapped `.git`, with the same
    /// words, and shows none of that repository. Compared by path alone,
    /// it took the other repository, writes following it.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_restored_tab_whose_folder_above_was_re_pointed_is_marked(cx: &mut TestAppContext) {
        use crate::views::tests::{fixture_git, fixture_repository};
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        for folder in ["projects", "elsewhere"] {
            std::fs::create_dir(base.join(folder)).unwrap();
        }
        let repository = fixture_repository(base.join("projects/shared-name"));
        let other = fixture_repository(base.join("elsewhere/shared-name"));
        for (repository, subject) in [(&repository, "Own fixture"), (&other, "Other fixture")] {
            fixture_git(
                repository.path(),
                &["commit", "--allow-empty", "-m", subject],
            );
        }
        let root = repository.path().to_owned();
        let saved = saved_with_inodes(&repository, "folder-above-saved.json");
        std::fs::rename(base.join("projects"), base.join("projects-away")).unwrap();
        std::os::unix::fs::symlink(base.join("elsewhere"), base.join("projects")).unwrap();
        let (app, mut window_cx) = open_tab_window(
            cx,
            root.clone(),
            saved,
            isolated_session("folder-above-after.json"),
            size(px(1000.), px(680.)),
        );
        let cx = &mut window_cx;
        settle_tab_test(&app, cx).await;
        assert_marked_for(&app, cx, &root, other.git_dir());
    }

    /// A repository cloned again at a restored tab's path has new inodes and
    /// is refused as a re-pointed folder is, which it cannot be told from.
    /// Closing the tab and opening the folder adopts it: a fresh open has
    /// no saved fields, and the tab it makes holds the new repository's.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_restored_tab_refuses_a_re_clone_until_its_folder_is_opened_again(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{fixture_git, fixture_repository};
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let repository = fixture_repository(base.join("repository"));
        fixture_git(
            repository.path(),
            &["commit", "--allow-empty", "-m", "Before"],
        );
        let root = repository.path().to_owned();
        let saved = saved_with_inodes(&repository, "re-clone-saved.json");
        // Moved aside rather than deleted, which could free the inode for
        // the new `.git` to take.
        std::fs::rename(&root, base.join("before")).unwrap();
        let cloned = fixture_repository(&root);
        fixture_git(cloned.path(), &["commit", "--allow-empty", "-m", "After"]);
        assert_ne!(cloned.git_dir_inode(), repository.git_dir_inode());
        let (app, mut window_cx) = open_tab_window(
            cx,
            root.clone(),
            saved,
            isolated_session("re-clone-after.json"),
            size(px(1000.), px(680.)),
        );
        let cx = &mut window_cx;
        settle_tab_test(&app, cx).await;
        assert_marked_for(&app, cx, &root, cloned.git_dir());

        cx.update(|window, cx| app.update(cx, |app, cx| app.close_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.repository_tabs.tabs.is_empty()));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        app.read_with(cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            assert_eq!(tab.path, root);
            assert!(tab.error.is_none(), "unmarked");
            assert!(!app.tab_label(tab).0.ends_with(" · unavailable"));
            assert_eq!(app.error, None);
            assert_eq!(
                app.commits
                    .iter()
                    .map(|commit| commit.subject.as_str())
                    .collect::<Vec<_>>(),
                ["After"]
            );
            assert_eq!(
                tab.git_dir.as_ref(),
                Some(&worker::GitDirectories::of(&cloned)),
                "it holds the new repository's, as a tab of this run does"
            );
        });
    }

    /// QA-107: a tab marked as it is restored cold, from a saved session
    /// whose repository was cloned again while the app was closed, opens
    /// Projects on the selected action tab with its ring whole, at 13 pt in
    /// a full window and 18 pt in a 461 × 490 half tile, as every other way
    /// onto the page does. Focus stayed on History's list, which Projects
    /// does not draw, so the page showed no focus and a screen reader was
    /// left on the window itself.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_marked_tab_restored_cold_opens_projects_on_its_ringed_first_stop(
        cx: &mut TestAppContext,
    ) {
        use crate::appearance::{ThemeChoice, custom::ThemeSelection};
        use crate::views::tests::{fixture_git, fixture_repository};
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let repository = fixture_repository(base.join("repository"));
        fixture_git(
            repository.path(),
            &["commit", "--allow-empty", "-m", "Before"],
        );
        let root = repository.path().to_owned();
        let saved = saved_with_inodes(&repository, "first-stop-saved.json");
        // Moved aside rather than deleted, which could free the inode for
        // the new `.git` to take.
        std::fs::rename(&root, base.join("before")).unwrap();
        let cloned = fixture_repository(&root);
        for (text, width, height) in [(13, 1280., 800.), (18, 461., 490.)] {
            let at = format!("{text} pt in {width} × {height}");
            let (app, mut window_cx) = open_tab_window(
                cx,
                root.clone(),
                saved.clone(),
                isolated_session(&format!("first-stop-after-{text}.json")),
                size(px(width), px(height)),
            );
            let cx = &mut window_cx;
            cx.update(|window, cx| {
                cx.set_reduce_motion(true);
                app.update(cx, |app, cx| {
                    app.settings.theme = ThemeSelection::BuiltIn(ThemeChoice::Midnight);
                    app.settings.follow_system = false;
                    app.apply_appearance(window, cx);
                });
                crate::appearance::apply_text_sizes(text, 12, window, cx);
            });
            settle_tab_test(&app, cx).await;
            assert_marked_for(&app, cx, &root, cloned.git_dir());
            crate::tags::tests::draw(cx);
            cx.update(|window, cx| {
                let app = app.read(cx);
                assert!(app.page == AppPage::Projects, "{at}");
                assert!(
                    app.hub.read(cx).selected_mode_focused(window),
                    "{at}: the selected action tab holds focus"
                );
                assert!(
                    app.app_focus.contains_focused(window, cx),
                    "{at}: inside the window's handlers"
                );
            });
            crate::tags::tests::assert_ring_painted(cx, "project-mode-Open", &at);
            crate::tags::tests::assert_rings_whole(cx, &["project-mode-Open"]);
            // The reason begins in the view the ring leaves it.
            let reason = crate::tags::tests::rendered(cx, "project-hub-error");
            let view = app.read_with(cx, |app, cx| app.hub.read(cx).body_scroll().bounds());
            assert!(
                reason.top() < view.bottom() && reason.top() >= view.top(),
                "{at}: the reason {reason:?} begins in {view:?}"
            );
        }
    }

    /// A session saved on a repository whose `.git` was then swapped for a
    /// gitfile to a third repository while the app was closed, and a file
    /// added to its folder, restored cold in a window that opens Projects on
    /// its first stop with the mark's reason, the tab's folder among the
    /// recent projects. Returns the fixture, the window and the third
    /// repository's Git directory.
    #[cfg(unix)]
    async fn restored_marked_with_its_row(
        cx: &mut TestAppContext,
        name: &str,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        VisualTestContext,
        PathBuf,
    ) {
        use crate::views::tests::{fixture_git, fixture_repository};
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let repository = fixture_repository(base.join("repository"));
        fixture_git(
            repository.path(),
            &["commit", "--allow-empty", "-m", "Before"],
        );
        let root = repository.path().to_owned();
        let saved = saved_with_inodes(&repository, &format!("{name}-saved.json"));
        let third = fixture_repository(base.join("third"));
        fixture_git(third.path(), &["commit", "--allow-empty", "-m", "Third"]);
        let third_git = third.git_dir().to_owned();
        std::fs::rename(root.join(".git"), base.join("away-git")).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", third_git.display()),
        )
        .unwrap();
        // A change for the header to count once the tab is captured afresh
        // ([`assert_recaptured`]).
        std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
        let (app, mut window_cx) = open_tab_window(
            cx,
            root.clone(),
            saved,
            isolated_session(&format!("{name}-after.json")),
            size(px(1280.), px(800.)),
        );
        settle_tab_test(&app, &mut window_cx).await;
        let hub = app.read_with(&window_cx, |app, _| app.hub.clone());
        window_cx.update(|_, cx| hub.update(cx, |hub, cx| hub.set_recent(vec![root], cx)));
        crate::views::tests::draw(&mut window_cx);
        (fixture, app, window_cx, third_git)
    }

    /// Enter going down in the window, a held key's repeat where `is_held`.
    fn enter_down(cx: &mut VisualTestContext, is_held: bool) {
        cx.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
            is_held,
            prefer_character_input: false,
        });
        crate::views::tests::draw(cx);
    }

    fn enter_up(cx: &mut VisualTestContext) {
        cx.simulate_event(gpui_kit::KeyUpEvent {
            keystroke: gpui_kit::Keystroke::parse("enter").unwrap(),
        });
        crate::views::tests::draw(cx);
    }

    /// A restored marked tab's Projects page as it opened: focus on
    /// `focus`, the mark's reason said, the tab still marked and nothing
    /// opened or captured.
    #[cfg(unix)]
    fn assert_nothing_opened(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        focus: Option<&FocusHandle>,
        third_git: &Path,
        at: &str,
    ) {
        cx.update(|window, cx| {
            let app = app.read(cx);
            let hub = app.hub.read(cx);
            assert_eq!(app.page, AppPage::Projects, "{at}");
            match focus {
                Some(focus) => assert!(focus.is_focused(window), "{at}: focus stays"),
                None => assert!(hub.selected_mode_focused(window), "{at}: focus stays"),
            }
            assert!(hub.shown_error().is_some(), "{at}: the reason stays");
            assert!(!hub.shown_busy(), "{at}: nothing opens");
            let tab = &app.repository_tabs.tabs[0];
            assert!(tab.error.is_some(), "{at}: still marked");
            assert_ne!(
                tab.git_dir.as_ref().map(|dirs| dirs.git_dir.as_path()),
                Some(third_git),
                "{at}: nothing captured"
            );
        });
    }

    /// Enter or Space on a restored marked tab's first stop, the action tab
    /// already selected, does nothing: it cleared the mark's reason and moved
    /// focus to the recent search, where a held Enter's next repeat opened
    /// the first recent project, the marked tab, a fresh capture nobody
    /// asked for. Held as repeats marked held (macOS, Wayland) or as fresh
    /// presses (X11), with an unpaired release among them, the key presses
    /// nothing that moves focus.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_held_enter_on_a_restored_marked_tab_s_first_stop_opens_nothing(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, mut window_cx, third_git) =
            restored_marked_with_its_row(cx, "held-first-stop").await;
        let cx = &mut window_cx;
        assert_nothing_opened(&app, cx, None, &third_git, "at launch");
        cx.simulate_keystrokes("space");
        crate::views::tests::draw(cx);
        assert_nothing_opened(&app, cx, None, &third_git, "Space");
        enter_down(cx, false);
        for _ in 0..4 {
            enter_down(cx, true);
        }
        assert_nothing_opened(&app, cx, None, &third_git, "held, its repeats marked");
        enter_up(cx);
        settle_tab_test(&app, cx).await;
        assert_nothing_opened(&app, cx, None, &third_git, "released");
        for _ in 0..4 {
            enter_down(cx, false);
        }
        // X11's unpaired auto-release, its next press a fresh one.
        enter_up(cx);
        enter_down(cx, false);
        enter_down(cx, false);
        enter_up(cx);
        settle_tab_test(&app, cx).await;
        assert_nothing_opened(&app, cx, None, &third_git, "X11's repeats");
    }

    /// The recent search's Enter opens only for a press that began in the
    /// field and is not held: an Enter that began on another control, its
    /// X11 repeat arriving as a fresh press once focus moved to the search,
    /// and a held repeat open nothing; a fresh press there opens the first
    /// recent project.
    #[cfg(unix)]
    #[gpui::test]
    async fn the_recent_search_opens_only_for_an_enter_that_began_in_it(cx: &mut TestAppContext) {
        let (_fixture, app, mut window_cx, third_git) =
            restored_marked_with_its_row(cx, "search-enter").await;
        let cx = &mut window_cx;
        let search = app.read_with(cx, |app, cx| app.hub.read(cx).search_focus(cx));
        // Begun on the first stop, then focus moves to the search, as a
        // press that moves focus does.
        enter_down(cx, false);
        cx.update(|window, cx| search.focus(window, cx));
        crate::views::tests::draw(cx);
        enter_down(cx, false);
        assert_nothing_opened(&app, cx, Some(&search), &third_git, "an X11 repeat");
        enter_down(cx, true);
        assert_nothing_opened(&app, cx, Some(&search), &third_git, "a marked repeat");
        enter_up(cx);
        settle_tab_test(&app, cx).await;
        // Held from the field itself, its repeats open nothing either.
        enter_down(cx, true);
        assert_nothing_opened(&app, cx, Some(&search), &third_git, "held in the field");
        enter_up(cx);
        settle_tab_test(&app, cx).await;
        // A fresh press in the field opens the first recent project.
        enter_down(cx, false);
        enter_up(cx);
        settle_tab_test(&app, cx).await;
        let root = app.read_with(cx, |app, _| app.repository_tabs.tabs[0].path.clone());
        assert_recaptured(&app, cx, 0, &root, &third_git);
    }

    /// The recent search opens nothing for a modified Enter, which its
    /// field reports as another Enter or not at all, nor for an Enter that
    /// goes down while an input method composes there, which commits the
    /// composition; a plain press, its text committed, then opens the first
    /// recent project. Acted on as a raw key-down that checked neither, each
    /// opened it. A modified press leaves nothing noted for a plain Enter
    /// the field reports with no key-down: noted for it, and taken only by
    /// the plain Enter, it opened the project then.
    #[cfg(unix)]
    #[gpui::test]
    async fn the_recent_search_opens_nothing_for_a_modified_or_composing_enter(
        cx: &mut TestAppContext,
    ) {
        use gpui::EntityInputHandler as _;
        let (_fixture, app, mut window_cx, third_git) =
            restored_marked_with_its_row(cx, "search-modified-enter").await;
        let cx = &mut window_cx;
        let (search, field) = app.read_with(cx, |app, cx| {
            let hub = app.hub.read(cx);
            (hub.search_focus(cx), hub.search_field())
        });
        cx.update(|window, cx| search.focus(window, cx));
        crate::views::tests::draw(cx);
        let press = |cx: &mut VisualTestContext, keys: &str| {
            let keystroke = gpui_kit::Keystroke::parse(keys).unwrap();
            cx.simulate_event(gpui_kit::KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            crate::views::tests::draw(cx);
            cx.simulate_event(gpui_kit::KeyUpEvent { keystroke });
            crate::views::tests::draw(cx);
        };
        for keys in ["ctrl-enter", "cmd-enter", "alt-enter", "shift-enter"] {
            press(cx, keys);
            settle_tab_test(&app, cx).await;
            assert_nothing_opened(&app, cx, Some(&search), &third_git, keys);
            // A plain Enter the field reports with no key-down of its own.
            cx.update(|_, cx| {
                field.update(cx, |_, cx| {
                    cx.emit(gpui_kit::component::input::InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    })
                })
            });
            crate::views::tests::draw(cx);
            settle_tab_test(&app, cx).await;
            let after = format!("{keys}, then a reported Enter");
            assert_nothing_opened(&app, cx, Some(&search), &third_git, &after);
        }
        // "repo" composing, which the recent project's folder matches.
        cx.update(|window, cx| {
            field.update(cx, |field, cx| {
                field.replace_and_mark_text_in_range(None, "repo", None, window, cx)
            })
        });
        crate::views::tests::draw(cx);
        press(cx, "enter");
        settle_tab_test(&app, cx).await;
        assert_nothing_opened(&app, cx, Some(&search), &third_git, "composing");
        cx.update(|window, cx| field.update(cx, |field, cx| field.unmark_text(window, cx)));
        crate::views::tests::draw(cx);
        press(cx, "enter");
        settle_tab_test(&app, cx).await;
        let root = app.read_with(cx, |app, _| app.repository_tabs.tabs[0].path.clone());
        assert_recaptured(&app, cx, 0, &root, &third_git);
    }

    /// A window on a whole tab that has switched to `gone`, a cold tab whose
    /// repository is no longer there, its read held so the test can act
    /// before it fails and the window shows Projects.
    fn switch_to_gone_cold_tab(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        gone: PathBuf,
    ) -> crate::worker::ReaderHeld {
        let index = app.update(cx, |app, _| {
            app.repository_tabs.tabs.push(Tab {
                path: gone,
                resolved: true,
                git_dir: None,
                saved: Bookmark::default(),
                warm: None,
                error: None,
            });
            app.repository_tabs.tabs.len() - 1
        });
        let held = hold_reads(app, cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.switch_repository_tab(index, window, cx))
        });
        crate::views::tests::draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.repository.is_none()),
            "the cold tab is read with no repository in the window"
        );
        held
    }

    /// Lets the held read of [`switch_to_gone_cold_tab`] fail, and draws
    /// Projects, which the window shows in its place.
    async fn fail_held_read(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        held: crate::worker::ReaderHeld,
    ) {
        drop(held);
        settle_tab_test(app, cx).await;
        crate::views::tests::draw(cx);
        assert_eq!(app.read_with(cx, |app, _| app.page), AppPage::Projects);
    }

    /// Focus the person moved while the tab was read stays theirs only
    /// where Projects draws it: a tab strip control keeps focus, and the
    /// navigation's filter, which Projects does not draw, gives way to the
    /// selected action tab as the frame that draws the page ends. Left
    /// there, focus rested on a control no frame drew and no key reached.
    #[gpui::test]
    async fn a_failed_cold_restore_keeps_only_the_focus_projects_draws(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let held = switch_to_gone_cold_tab(&app, cx, fixture.path().join("gone-filter"));
        cx.update(|window, cx| {
            app.read(cx)
                .nav_search
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx)
        });
        crate::views::tests::draw(cx);
        fail_held_read(&app, cx, held).await;
        cx.update(|window, cx| {
            assert!(
                app.read(cx).hub.read(cx).selected_mode_focused(window),
                "the filter gives way to the page's first stop"
            );
        });

        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        crate::views::tests::draw(cx);
        let held = switch_to_gone_cold_tab(&app, cx, fixture.path().join("gone-menu"));
        let strip_control = cx.update(|window, cx| {
            let root = app.read(cx).app_focus.clone();
            root.focus(window, cx);
            window.focus_next(cx);
            window.focused(cx).expect("Tab reaches the tab strip")
        });
        crate::views::tests::draw(cx);
        fail_held_read(&app, cx, held).await;
        cx.update(|window, _| {
            assert!(
                strip_control.is_focused(window),
                "Projects draws the tab strip, whose control keeps focus"
            );
        });
    }

    /// A dialog open as the read fails keeps focus, and its close lands on
    /// the page's first stop rather than the list the tab was read with,
    /// which Projects does not draw.
    #[gpui::test]
    async fn a_failed_cold_restore_under_a_dialog_returns_to_the_first_stop(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let held = switch_to_gone_cold_tab(&app, cx, fixture.path().join("gone"));
        cx.update(|window, cx| window.open_dialog(cx, |dialog, _, _| dialog.title("Probe")));
        crate::views::tests::draw(cx);
        fail_held_read(&app, cx, held).await;
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx), "the dialog stays open");
            assert!(
                !app.read(cx).hub.read(cx).selected_mode_focused(window),
                "and keeps focus"
            );
            window.close_dialog(cx);
        });
        crate::views::tests::draw(cx);
        crate::views::tests::draw(cx);
        cx.update(|window, cx| {
            assert!(
                app.read(cx).hub.read(cx).selected_mode_focused(window),
                "its close lands on the page's first stop"
            );
        });
    }

    /// A repository cloned again at the shown tab's path during the run,
    /// brought in by `read`: the tab's directories and inodes follow, so the
    /// next session save holds the ones that are now true and a window
    /// restored from that save finds the tab whole. They once stayed as
    /// captured and the launch after marked the tab. The tab of another
    /// repository takes nothing.
    #[cfg(unix)]
    async fn a_tab_follows_a_repository_cloned_again(
        cx: &mut TestAppContext,
        name: &str,
        read: impl FnOnce(&Entity<GitTurtle>, &mut VisualTestContext),
    ) {
        use crate::views::tests::{fixture_git, fixture_repository};
        cx.executor().allow_parking();
        crate::views::tests::init_app(cx);
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().canonicalize().unwrap();
        let repository = fixture_repository(base.join("repository"));
        fixture_git(
            repository.path(),
            &["commit", "--allow-empty", "-m", "Before"],
        );
        let root = repository.path().to_owned();
        let window_size = size(px(1000.), px(680.));
        let (app, mut window_cx) = open_tab_window(
            cx,
            root.clone(),
            Session::default(),
            isolated_session(&format!("{name}-first.json")),
            window_size,
        );
        // Keeps the watcher, as `settle_tab_test` does not: a quiet read
        // comes through it.
        crate::views::tests::settle(&app, &mut window_cx).await;
        // The rescan that follows the watcher's setup lands first: running
        // as the repository moves, it finds no repository and marks the tab.
        quiet_read_landed(&app, &mut window_cx);
        // Moved aside rather than deleted, which could free the inode for
        // the new `.git` to take.
        std::fs::rename(&root, base.join("before")).unwrap();
        let cloned = fixture_repository(&root);
        fixture_git(cloned.path(), &["commit", "--allow-empty", "-m", "After"]);
        assert_ne!(cloned.git_dir_inode(), repository.git_dir_inode());

        read(&app, &mut window_cx);
        crate::views::tests::settle(&app, &mut window_cx).await;
        let other = fixture_repository(base.join("other"));
        let saved = app.update(&mut window_cx, |app, _| {
            app.repository_tabs.follow_read_repository(&other);
            app.repository_tabs.saved()
        });
        let directories = saved.tabs[0]
            .git_directories
            .as_ref()
            .expect("saved with the tab");
        assert_eq!(directories.git_dir.path(), cloned.git_dir());
        assert_eq!(directories.git_dir_inode, cloned.git_dir_inode());
        assert_eq!(directories.common_inode, cloned.common_dir_inode());

        let (restored, mut window_cx) = open_tab_window(
            cx,
            root.clone(),
            saved,
            isolated_session(&format!("{name}-second.json")),
            window_size,
        );
        settle_tab_test(&restored, &mut window_cx).await;
        restored.read_with(&window_cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            assert!(tab.error.is_none(), "{:?}", tab.error);
            assert!(!app.tab_label(tab).0.ends_with(" · unavailable"));
            assert_eq!(app.commits[0].subject, "After");
        });
    }

    /// A quiet read of a change in the Git directory reads History, which
    /// brings the repository cloned again in.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_quiet_read_takes_a_repository_cloned_again_into_the_tab(cx: &mut TestAppContext) {
        a_tab_follows_a_repository_cloned_again(cx, "quiet-read", |app, cx| {
            app.read_with(cx, |app, _| app.operations.drain());
            cx.executor().run_until_parked();
            app.update(cx, |app, _| {
                app.automatic
                    .report_local_change(crate::local_refresh::LocalChange {
                        git: true,
                        ..Default::default()
                    })
            });
            // Found cloned again, the repository's status is read on the
            // operation thread and its watch is made again, whose rescan
            // follows (`apply_quiet_refresh`).
            for _ in 0..16 {
                cx.executor().run_until_parked();
                let reading = app.read_with(cx, |app, _| {
                    app.automatic.quiet_read_requested() || app.status_task.is_some()
                });
                if !reading {
                    return;
                }
                app.read_with(cx, |app, _| {
                    app.worker.wait_idle();
                    app.operations.drain();
                });
            }
            panic!("the quiet read and the status read it asked for never landed");
        })
        .await;
    }

    /// Show latest installs the snapshot it reads in place, past the
    /// acceptance an Open's snapshot takes.
    #[cfg(unix)]
    #[gpui::test]
    async fn show_latest_takes_a_repository_cloned_again_into_the_tab(cx: &mut TestAppContext) {
        a_tab_follows_a_repository_cloned_again(cx, "show-latest", |app, cx| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_latest_history(window, cx)))
        })
        .await;
    }

    /// An explicit Open of the folder a marked tab shows is a fresh
    /// capture: it names no root or Git directory, and once accepted the
    /// tab takes the repository Git finds there and its mark clears. It
    /// once reopened the tab by its old identity and was refused again.
    #[gpui::test]
    async fn an_explicit_open_of_a_marked_tab_s_folder_captures_it_afresh(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root, third_git) = marked_by_a_swapped_git(cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            let shown = &app.repository_tabs.tabs[0];
            assert_eq!(shown.path, root, "the same tab");
            assert!(shown.error.is_none(), "its mark cleared");
            assert!(!app.tab_label(shown).0.ends_with(" · unavailable"));
            assert_eq!(
                shown.git_dir.as_ref().map(|dirs| dirs.git_dir.as_path()),
                Some(third_git.as_path())
            );
            assert_eq!(
                app.repository
                    .as_ref()
                    .map(|repository| repository.git_dir()),
                Some(third_git.as_path()),
                "the repository found there"
            );
        });
    }

    /// The History fixture with its `.git` swapped for a gitfile to a third
    /// repository's Git directory and quiet reads held, marked " ·
    /// unavailable" by Refresh, then a file added to its folder that no read
    /// has seen: the window, the tab's root and the third repository's Git
    /// directory. The fixture's folder held nothing else to count, so a
    /// header that read "Changes" after a fresh capture said nothing of
    /// whether it was read ([`assert_recaptured`]).
    async fn marked_by_a_swapped_git(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut VisualTestContext,
        PathBuf,
        PathBuf,
    ) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let third = crate::views::tests::fixture_repository(fixture.path().join("third"));
        let third_git = third.git_dir().to_owned();
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", third_git.display()),
        )
        .unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert!(
            app.read_with(cx, |app, _| app.repository_tabs.tabs[0].error.is_some()),
            "Refresh marks the tab"
        );
        std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
        (fixture, app, cx, root, third_git)
    }

    /// Opens a tab on a new repository at `path` and lands its read.
    async fn open_another_tab(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, path: &Path) {
        let repository = crate::views::tests::fixture_repository(path);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(repository.path().to_owned(), window, cx)
            })
        });
        settle_tab_test(app, cx).await;
    }

    /// An explicit Open of a marked tab's folder from another tab reads it
    /// afresh too: what the tab retained goes, so its switch opens it, and
    /// the tab takes the repository Git finds there, its mark cleared. It
    /// once showed the tab as retained and opened nothing, its root and Git
    /// directory already cleared, so a quiet read that found the repository
    /// left it unmarked with nothing captured, saved as unresolved.
    #[gpui::test]
    async fn an_explicit_open_of_a_marked_tab_s_folder_from_another_tab_captures_it_afresh(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx, root, third_git) = marked_by_a_swapped_git(cx).await;
        open_another_tab(&app, cx, &fixture.path().join("other")).await;
        assert!(
            app.read_with(cx, |app, _| app.repository_tabs.active == Some(1)
                && app.repository_tabs.tabs[0].warm.is_some()),
            "the marked tab retained, another shown"
        );
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.active, Some(0), "the marked tab shown");
            let shown = &app.repository_tabs.tabs[0];
            assert_eq!(shown.path, root, "the same tab");
            assert!(shown.error.is_none(), "its mark cleared");
            assert!(!app.tab_label(shown).0.ends_with(" · unavailable"));
            assert!(shown.resolved);
            assert_eq!(
                shown.git_dir.as_ref().map(|dirs| dirs.git_dir.as_path()),
                Some(third_git.as_path())
            );
            assert_eq!(
                app.repository
                    .as_ref()
                    .map(|repository| repository.git_dir()),
                Some(third_git.as_path()),
                "the repository found there"
            );
            assert!(!app.repository_tabs.saved().tabs[0].unresolved);
        });
    }

    /// QA-107: where the tab is marked the header says what the mark says,
    /// in the full bars and the compact ones. "Changes · 1" and the branch
    /// came from the last status read, kept beside the mark; with no status
    /// read the picker read "Detached HEAD", of a repository that cannot be
    /// opened. The picker refuses its menu, which listed the branches from
    /// before, and the edge's "No remote" goes with its action. The profile
    /// picker names no author and its tooltip no address from before, read
    /// from the configuration of the repository the tab cannot open.
    #[gpui::test]
    async fn a_marked_tab_s_header_shows_nothing_from_before(cx: &mut TestAppContext) {
        use crate::branch_actions::PICKER_LABEL;
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::draw(cx);
        let before = app.read_with(cx, |app, _| app.changes_label());
        assert!(before.starts_with("Changes · "), "a count: {before}");
        assert_eq!(
            PICKER_LABEL.with(|label| label.borrow().clone()).as_deref(),
            Some("main")
        );
        assert!(
            app.read_with(cx, |app, _| app.repository_edge().is_some()),
            "the edge before the mark: no remote"
        );
        let read_identity = |cx: &mut VisualTestContext| {
            app.update(cx, |app, cx| {
                app.profile = Some(gitturtle_core::GitProfile {
                    name: "Ada Lovelace".into(),
                    email: "ada@example.invalid".into(),
                    private_worktree: false,
                    ..app.profile.clone().unwrap_or_default()
                });
                cx.notify();
            });
            crate::views::tests::draw(cx);
        };
        let picker_drawn =
            || crate::profiles::PROFILE_PICKER_DRAWN.with(|drawn| drawn.borrow().clone());
        read_identity(cx);
        assert_eq!(
            picker_drawn(),
            Some((
                "Git profile: Ada Lovelace. Choose or manage profiles".into(),
                "Ada Lovelace <ada@example.invalid> · Repository configuration shared by linked worktrees"
                    .into()
            )),
            "the identity before the mark"
        );

        std::fs::rename(root.join(".git"), fixture.path().join("git-away")).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", fixture.path().join("nowhere").display()),
        )
        .unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::draw(cx);
        assert!(app.read_with(cx, |app, _| app.shown_tab_unavailable()));
        // The identity read before, kept beside the mark.
        read_identity(cx);
        let says_the_mark = |cx: &mut VisualTestContext, at: &str| {
            assert_eq!(
                app.read_with(cx, |app, _| app.changes_label()),
                "Changes",
                "{at}: no count from before"
            );
            assert_eq!(
                picker_drawn(),
                Some((
                    "Git profile: Profiles. Choose or manage profiles".into(),
                    "Choose or manage named Git author profiles".into()
                )),
                "{at}: no identity from before"
            );
            assert_eq!(
                PICKER_LABEL.with(|label| label.borrow().clone()).as_deref(),
                Some("Unavailable"),
                "{at}: no branch from before"
            );
            let refusal = crate::workspace::refusals::get("current-branch").expect("drawn");
            assert!(
                refusal.dimmed && refusal.announced,
                "{at}: refuses its menu"
            );
            assert_eq!(
                crate::views::CONTROL_TOOLTIPS
                    .with(|tooltips| tooltips.borrow().get("current-branch").cloned())
                    .as_deref(),
                Some(
                    "Repository unavailable · Refresh, or choose a repository with Open Repository"
                ),
                "{at}: the way back, in the mark's words"
            );
            assert!(
                app.read_with(cx, |app, _| app.repository_edge().is_none()),
                "{at}: no edge from before"
            );
            assert!(cx.debug_bounds("branch-edge").is_none(), "{at}: its line");
            assert!(cx.debug_bounds("branch-sync-state").is_none(), "{at}");
        };
        says_the_mark(cx, "the status kept beside the mark");
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        crate::views::tests::draw(cx);
        says_the_mark(cx, "the compact bars");
        assert_eq!(
            crate::views::COMPACT_LABELS
                .with(|names| names.borrow().get("changes-tab").cloned())
                .as_deref(),
            Some("Changes"),
            "the compact toggle's name"
        );
        assert!(
            cx.debug_bounds("changes-count-badge").is_none(),
            "the compact toggle's badge"
        );
        app.update(cx, |app, cx| {
            app.work_status = None;
            cx.notify();
        });
        crate::views::tests::draw(cx);
        says_the_mark(cx, "no status read");
    }

    /// Where the tab is marked, Fetch, Pull and Push refuse and say the mark
    /// as the branch picker does, with a remote configured and without one.
    /// With the status absent they read it as a detached HEAD, Pull and Push
    /// saying "Switch to a local branch before pulling or pushing", and
    /// Fetch stayed "Fetch from origin", a remote from before, and pressed.
    /// Targets' Switch and Create refuse there too, with a branch named,
    /// their tooltips saying the mark after what they do: they stayed
    /// available toward the repository the tab can no longer open.
    #[gpui::test]
    async fn a_marked_tab_s_git_actions_refuse_and_say_the_mark(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let tooltip = |id: &str| {
            crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow().get(id).cloned())
        };
        let with_a_remote = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.remotes.push(gitturtle_core::Remote {
                        name: "origin".into(),
                        url: "https://example.invalid/fixture.git".into(),
                        push_url: "https://example.invalid/fixture.git".into(),
                    });
                    app.remote_name
                        .update(cx, |input, cx| input.set_value("origin", window, cx));
                    app.remote_branch
                        .update(cx, |input, cx| input.set_value("main", window, cx));
                    cx.notify();
                })
            });
            crate::views::tests::draw(cx);
        };
        let destination =
            || crate::workspace::TARGETS_DESTINATION.with(|line| line.borrow().clone());
        let branch_forms = [
            ("checkout-branch", "Switch to the named local branch"),
            ("create-branch", "Create and switch to a branch from HEAD"),
        ];
        let says_the_mark = |at: &str, compact: bool| {
            for (id, label) in [("fetch", "Fetch"), ("pull", "Pull"), ("push", "Push")] {
                let refusal = crate::workspace::refusals::get(id).expect("drawn");
                assert!(refusal.announced && refusal.dimmed, "{at}: {id} refuses");
                let reason = crate::branch_actions::UNAVAILABLE_REASON;
                let said = if compact {
                    format!("{label} · {reason}")
                } else {
                    reason.to_owned()
                };
                assert_eq!(tooltip(id), Some(said), "{at}: {id}'s tooltip");
                if compact {
                    assert_eq!(
                        crate::views::COMPACT_LABELS
                            .with(|names| names.borrow().get(id).cloned())
                            .as_deref(),
                        Some(format!("{label}: {reason}").as_str()),
                        "{at}: {id}'s name"
                    );
                }
            }
            // Targets' line names no remote or branch from before either.
            assert_eq!(
                destination().as_deref(),
                Some(crate::branch_actions::UNAVAILABLE_REASON),
                "{at}: where Push goes"
            );
            if !compact {
                for (id, purpose) in branch_forms {
                    let refusal = crate::workspace::refusals::get(id).expect("drawn");
                    assert!(refusal.announced && refusal.dimmed, "{at}: {id} refuses");
                    assert_eq!(
                        tooltip(id),
                        Some(format!(
                            "{purpose} · {}",
                            crate::branch_actions::UNAVAILABLE_REASON
                        )),
                        "{at}: {id}'s tooltip"
                    );
                }
            }
        };
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.git_actions_open = true;
                app.branch_name
                    .update(cx, |input, cx| input.set_value("topic", window, cx));
                cx.notify();
            })
        });
        with_a_remote(cx);
        let before = destination().expect("Targets' line");
        assert!(
            before.starts_with("Push to origin/main · "),
            "before the mark: {before}"
        );
        for (id, purpose) in branch_forms {
            let refusal = crate::workspace::refusals::get(id).expect("drawn");
            assert!(
                !refusal.announced && !refusal.dimmed,
                "before the mark: {id} with a branch named"
            );
            assert_eq!(tooltip(id).as_deref(), Some(purpose), "before the mark");
        }
        app.update(cx, |app, cx| {
            app.remotes.clear();
            cx.notify();
        });
        crate::views::tests::draw(cx);
        std::fs::rename(root.join(".git"), fixture.path().join("git-away")).unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", fixture.path().join("nowhere").display()),
        )
        .unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::draw(cx);
        assert!(app.read_with(cx, |app, _| app.shown_tab_unavailable()));
        says_the_mark("no remote", false);
        with_a_remote(cx);
        says_the_mark("a remote", false);
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        crate::views::tests::draw(cx);
        says_the_mark("the compact bar, a remote", true);
        app.update(cx, |app, cx| {
            app.remotes.clear();
            cx.notify();
        });
        crate::views::tests::draw(cx);
        says_the_mark("the compact bar, no remote", true);

        // A click does nothing: no write starts.
        let fetch = crate::tags::tests::rendered(cx, "fetch");
        cx.simulate_click(fetch.center(), gpui::Modifiers::default());
        crate::views::tests::draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.operation_busy.is_none()),
            "nothing runs"
        );
    }

    /// Presses the Projects page's row for `path`, as a person would.
    fn press_projects_row(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, path: &Path) {
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        cx.update(|window, cx| {
            hub.update(cx, |hub, cx| hub.set_recent(vec![path.to_owned()], cx));
            app.update(cx, |app, cx| app.show_projects(window, cx));
        });
        crate::views::tests::draw(cx);
        let row = cx
            .debug_bounds("recent-project-0")
            .expect("the project's row");
        cx.simulate_click(row.center(), gpui::Modifiers::default());
    }

    /// The tab at `index` took the repository found at its folder from a
    /// Projects row: the same tab, shown on the repository page, its mark
    /// gone and that repository's Git directory captured. Once its status
    /// read has landed, the header says that repository's working state, as
    /// the mark had hidden it: the count of the one file added while the
    /// tab was marked, which only a status read since then finds, the branch
    /// and the edge. Until it lands the header shows none
    /// ([`assert_a_fresh_capture_shows_nothing_from_before`]).
    fn assert_recaptured(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        index: usize,
        root: &Path,
        git_dir: &Path,
    ) {
        crate::views::tests::draw(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.changes_label(), "Changes · 1", "the header's count");
            // Counted by a read since the mark: the status from before it,
            // of the Git directory the tab had, counted another change.
            let counted = app.header_status().map(|status| {
                (status.entries.iter())
                    .map(|entry| (entry.path.as_path(), entry.untracked))
                    .collect::<Vec<_>>()
            });
            assert_eq!(
                counted,
                Some(vec![(Path::new("untracked.txt"), true)]),
                "the file added while the tab was marked"
            );
            assert_eq!(
                app.repository_edge(),
                Some(crate::empty_state::RepositoryEdge::NoRemote),
                "its edge"
            );
        });
        assert_eq!(
            crate::branch_actions::PICKER_LABEL
                .with(|label| label.borrow().clone())
                .as_deref(),
            Some("main"),
            "its branch"
        );
        assert!(cx.debug_bounds("branch-edge").is_some(), "its edge's line");
        app.read_with(cx, |app, _| {
            assert_eq!(app.page, AppPage::Repository, "the tab is shown");
            assert_eq!(app.repository_tabs.active, Some(index));
            let tab = &app.repository_tabs.tabs[index];
            assert_eq!(tab.path, root, "the same tab");
            assert!(tab.error.is_none(), "its mark cleared");
            assert!(!app.tab_label(tab).0.ends_with(" · unavailable"));
            assert_eq!(app.error, None, "nothing left to say");
            assert_eq!(
                tab.git_dir.as_ref().map(|dirs| dirs.git_dir.as_path()),
                Some(git_dir),
                "the repository found there, by the accepted snapshot"
            );
            assert_eq!(
                app.repository.as_ref().map(|repo| repo.git_dir()),
                Some(git_dir)
            );
        });
    }

    /// QA-107 check 1.2: the Projects page's row for the shown marked tab
    /// reads its folder afresh, as the Local workspaces row and Open do. It
    /// went back to the tab through `open`, and the tab stayed unavailable
    /// with the reason it had.
    #[gpui::test]
    async fn a_projects_row_captures_the_shown_marked_tab_afresh(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root, third_git) = marked_by_a_swapped_git(cx).await;
        press_projects_row(&app, cx, &root);
        settle_tab_test(&app, cx).await;
        assert_recaptured(&app, cx, 0, &root, &third_git);
    }

    /// The same from another tab: pressed behind the tab shown, the row's
    /// `open` became a switch, which reads the tab's own identity again.
    #[gpui::test]
    async fn a_projects_row_captures_a_marked_tab_behind_another_afresh(cx: &mut TestAppContext) {
        let (fixture, app, cx, root, third_git) = marked_by_a_swapped_git(cx).await;
        open_another_tab(&app, cx, &fixture.path().join("other")).await;
        press_projects_row(&app, cx, &root);
        settle_tab_test(&app, cx).await;
        assert_recaptured(&app, cx, 0, &root, &third_git);
    }

    /// A fresh capture of the shown marked tab through `route`, which asks
    /// for it as a person would, shows and acts on nothing from the Git
    /// directory the tab had until the status read of the one found lands.
    /// The tab read on branch `before`, its upstream on `origin` (bare
    /// `a.git`), a file staged and a title typed; then its `.git` became a
    /// third repository's gitfile, with `origin` at bare `b.git`, a commit on
    /// `main` and a file of its own staged that no read has shown, and
    /// History's Refresh marked it, keeping the status read before. With the
    /// operation executor held, the accepted snapshot's status read waits:
    /// in the frame the mark clears in, and in Changes opened after it,
    /// there is no count, branch, remote, Targets value or row from before,
    /// and Fetch, Pull, Push and Commit refuse. Opened again in place, the
    /// tab kept the status read before, so until the read landed Push
    /// pushed the third repository's `main` to its own `origin`, and Commit
    /// committed its staged file, never shown, under the title typed for
    /// the other (security probes W1 and W3, FU39-A4).
    pub(crate) async fn assert_a_fresh_capture_shows_nothing_from_before(
        cx: &mut TestAppContext,
        route: impl FnOnce(&Entity<GitTurtle>, &mut VisualTestContext, &Path),
    ) {
        assert_a_capture_shows_nothing_from_before(cx, Replaced::Marked, route).await;
    }

    /// How [`assert_a_capture_shows_nothing_from_before`] puts the third
    /// repository where the tab's was.
    #[derive(Clone, Copy, PartialEq)]
    enum Replaced {
        /// The tab's `.git` becomes the third repository's gitfile, which
        /// History's Refresh marks: `route` reads the marked tab's folder.
        Marked,
        /// The third repository's Git directory moves to the tab's `.git`,
        /// as a re-clone at the path leaves it, which marks nothing.
        InPlace,
        /// The tab, opened at `x/repo`, sees `x` moved aside and linked to
        /// `y`, whose `repo` is the third repository: a folder above the
        /// root re-pointed, which a reopen follows (`worker::FoldersAbove`)
        /// and which marks nothing.
        Above,
    }

    /// [`assert_a_fresh_capture_shows_nothing_from_before`], with the third
    /// repository put where the tab's was as `replaced` says; `route` then
    /// reads the folder, marked or not.
    async fn assert_a_capture_shows_nothing_from_before(
        cx: &mut TestAppContext,
        replaced: Replaced,
        route: impl FnOnce(&Entity<GitTurtle>, &mut VisualTestContext, &Path),
    ) {
        use crate::views::tests::{fixture_git, fixture_git_output, fixture_repository};
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let base = fixture.path().to_owned();
        let root = if replaced == Replaced::Above {
            for folder in ["x", "y"] {
                std::fs::create_dir_all(base.join(folder)).unwrap();
            }
            let repository = fixture_repository(base.join("x").join("repo"));
            fixture_git(
                repository.path(),
                &["commit", "-q", "--allow-empty", "-m", "Before"],
            );
            let root = repository.path().to_owned();
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.open_repository_tab(root.clone(), window, cx)
                })
            });
            settle_tab_test(&app, cx).await;
            root
        } else {
            app.read_with(cx, |app, _| app.path.clone().expect("a repository"))
        };
        let read = |dir: &Path, args: &[&str]| {
            let output = fixture_git_output(dir, args);
            (output.status.success())
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        };
        let tip = |bare: &Path, branch: &str| read(bare, &["rev-parse", "--verify", "-q", branch]);
        let (bare_a, bare_b) = (base.join("a.git"), base.join("b.git"));
        for bare in [&bare_a, &bare_b] {
            fixture_git(
                &base,
                &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
            );
        }
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        fixture_git(&root, &["switch", "-c", "before"]);
        fixture_git(
            &root,
            &["remote", "add", "origin", bare_a.to_str().unwrap()],
        );
        fixture_git(&root, &["push", "-q", "-u", "origin", "before"]);
        std::fs::write(root.join("staged.txt"), "the person's\n").unwrap();
        fixture_git(&root, &["add", "staged.txt"]);
        let third = fixture_repository(if replaced == Replaced::Above {
            base.join("y").join("repo")
        } else {
            base.join("third")
        });
        fixture_git(
            third.path(),
            &["commit", "-q", "--allow-empty", "-m", "Third"],
        );
        fixture_git(
            third.path(),
            &["remote", "add", "origin", bare_b.to_str().unwrap()],
        );
        std::fs::write(third.path().join("planted.txt"), "not the person's\n").unwrap();
        fixture_git(third.path(), &["add", "planted.txt"]);
        let third_git = third.git_dir().to_owned();
        let pushed = tip(&bare_a, "refs/heads/before");
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.refresh(&Refresh, window, cx);
                app.show_working(window, cx);
            })
        });
        crate::views::tests::settle(&app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.commit_title.update(cx, |title, cx| {
                    title.set_value("The person's title", window, cx)
                });
                app.show_history(window, cx);
                app.git_actions_open = true;
            })
        });
        crate::views::tests::settle(&app, cx).await;
        let picker = || crate::branch_actions::PICKER_LABEL.with(|label| label.borrow().clone());
        assert_eq!(picker().as_deref(), Some("before"), "read before the mark");
        assert_eq!(
            app.read_with(cx, |app, _| app.changes_label()),
            if replaced == Replaced::Above {
                "Changes · 1"
            } else {
                "Changes · 2"
            }
        );
        assert!(
            !crate::workspace::refusals::get("push")
                .expect("drawn")
                .announced
        );

        let found_git = match replaced {
            Replaced::InPlace => {
                std::fs::rename(root.join(".git"), base.join("before-git-away")).unwrap();
                std::fs::rename(&third_git, root.join(".git")).unwrap();
                root.join(".git")
            }
            Replaced::Marked => {
                std::fs::rename(root.join(".git"), base.join("before-git-away")).unwrap();
                std::fs::write(
                    root.join(".git"),
                    format!("gitdir: {}\n", third_git.display()),
                )
                .unwrap();
                cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
                crate::views::tests::settle(&app, cx).await;
                assert!(
                    app.read_with(cx, |app, _| app.shown_tab_unavailable()),
                    "Refresh marks the tab"
                );
                third_git.clone()
            }
            Replaced::Above => {
                std::fs::rename(base.join("x"), base.join("x-away")).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(base.join("y"), base.join("x")).unwrap();
                #[cfg(not(unix))]
                unreachable!("a folder above is re-pointed through a link");
                third_git.clone()
            }
        };
        let identity = |app: &GitTurtle| {
            (app.repository.as_ref()).map(|repo| (repo.git_dir().to_owned(), repo.git_dir_inode()))
        };
        let shown = app.read_with(cx, |app, _| identity(app));

        let held = app.read_with(cx, |app, _| app.operations.hold());
        route(&app, cx, &root);
        for _ in 0..16 {
            let installed = app.read_with(cx, |app, _| {
                identity(app).is_some_and(|found| Some(found) != shown) && app.loading.is_none()
            });
            if installed {
                break;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
        }
        app.update(cx, |app, cx| {
            assert_eq!(app.page, AppPage::Repository, "the snapshot installed");
            assert!(!app.shown_tab_unavailable(), "its mark cleared");
            assert!(app.status_task.is_some(), "its status read waits");
            app.git_actions_open = true;
            cx.notify();
        });
        let frame = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            })
        };
        let refuses = |cx: &mut VisualTestContext, id: &'static str, at: &str| {
            let bounds = crate::tags::tests::rendered(cx, id);
            let refusal = crate::workspace::refusals::get(id).expect("drawn");
            assert!(refusal.announced && refusal.dimmed, "{at}: {id} refuses");
            bounds
        };
        let nothing_runs = |cx: &mut VisualTestContext, bounds: Bounds<Pixels>, at: &str| {
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
            assert_eq!(app.read_with(cx, |app, _| app.operation_busy), None, "{at}");
        };
        let destination =
            || crate::workspace::TARGETS_DESTINATION.with(|line| line.borrow().clone());
        let a = bare_a.display().to_string();
        frame(cx);
        // Refused at once, Fetch, Pull and Push look it once the read has
        // lasted a moment, as a write's look waits (`track_unread_look`).
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        frame(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.changes_label()),
            "Changes",
            "no count from before"
        );
        assert_ne!(picker().as_deref(), Some("before"), "no branch from before");
        assert!(
            destination().is_none_or(|line| !line.contains("before") && !line.contains(&a)),
            "Targets names no remote from before: {:?}",
            destination()
        );
        app.read_with(cx, |app, cx| {
            assert!(app.header_status().is_none(), "no status from before");
            assert!(app.remotes.is_empty(), "no remote from before");
            assert_eq!(
                app.remote_name.read(cx).value().as_ref(),
                "",
                "Targets' remote"
            );
            assert_eq!(
                app.remote_branch.read(cx).value().as_ref(),
                "",
                "Targets' branch"
            );
        });
        // History lists, selects and retains nothing of the replaced
        // repository, so its row menu offers no write on one of its
        // commits, and the navigation's folders are the found one's.
        let found_row = |subject: &str| subject == "Third";
        app.read_with(cx, |app, _| {
            let subjects = (app.commits.iter())
                .map(|commit| commit.subject.as_str())
                .collect::<Vec<_>>();
            assert!(
                subjects.iter().all(|subject| found_row(subject)),
                "no row from before: {subjects:?}"
            );
            assert_eq!(
                app.automatic.retained_commit, None,
                "no row retained from before"
            );
            assert_eq!(
                app.expanded_folders,
                crate::navigation::current_ancestors(&app.branches),
                "the navigation's folders"
            );
        });
        cx.update(|window, cx| {
            let focus = app.read(cx).focus.clone();
            window.focus(&focus, cx);
            window.dispatch_action(Box::new(crate::native_accessibility::RowActions), cx);
        });
        frame(cx);
        let offered = app.update(cx, |app, cx| {
            let offered = (app.commit_menu.take()).and_then(|_| {
                let commit = app.commits.get(app.selected_commit?)?;
                Some(commit.subject.clone())
            });
            cx.notify();
            offered
        });
        assert!(
            offered.as_deref().is_none_or(found_row),
            "the row menu offers a write on {offered:?}"
        );
        frame(cx);
        let actions = ["fetch", "pull", "push"].map(|id| (id, refuses(cx, id, "History")));
        for (id, bounds) in actions {
            nothing_runs(cx, bounds, id);
        }

        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        frame(cx);
        app.read_with(cx, |app, _| {
            assert!(app.working_rows.is_empty(), "no Changes row from before");
            assert!(app.actionable_working_rows().is_empty(), "nothing to stage");
        });
        assert!(
            cx.debug_bounds("working-header-count").is_none(),
            "Changes: no count"
        );
        if cx.debug_bounds("commit-staged").is_some() {
            let commit = refuses(cx, "commit-staged", "Changes");
            nothing_runs(cx, commit, "Commit");
        }
        let push = refuses(cx, "push", "Changes");
        nothing_runs(cx, push, "Push in Changes");

        drop(held);
        settle_tab_test(&app, cx).await;
        assert_eq!(picker().as_deref(), Some("main"), "the branch read since");
        app.read_with(cx, |app, cx| {
            let staged = app.header_status().map(|status| {
                (status.entries.iter())
                    .filter(|entry| entry.staged.is_some())
                    .map(|entry| entry.path.clone())
                    .collect::<Vec<_>>()
            });
            assert_eq!(
                staged,
                Some(vec![PathBuf::from("planted.txt")]),
                "its own index"
            );
            assert_eq!(
                (app.remotes.iter())
                    .map(|remote| remote.url.as_str())
                    .collect::<Vec<_>>(),
                [bare_b.to_str().unwrap()],
                "its own remote"
            );
            assert_eq!(app.remote_name.read(cx).value().as_ref(), "origin");
            assert_eq!(app.remote_branch.read(cx).value().as_ref(), "main");
            assert_eq!(app.operation_notice, None, "nothing ran");
        });
        frame(cx);
        assert_eq!(
            destination().as_deref(),
            Some(format!("Push to origin/main · {}", bare_b.display()).as_str()),
            "Targets name its own remote"
        );
        assert_eq!(tip(&bare_b, "refs/heads/main"), None, "Push pushed nothing");
        assert_eq!(tip(&bare_a, "refs/heads/before"), pushed);
        assert_eq!(
            read(
                &base,
                &[
                    "--git-dir",
                    found_git.to_str().unwrap(),
                    "log",
                    "-1",
                    "--format=%s"
                ]
            )
            .as_deref(),
            Some("Third"),
            "Commit committed nothing"
        );
        if replaced == Replaced::Above {
            // Back as it was, so the folder's removal stays inside it.
            std::fs::remove_file(base.join("x")).unwrap();
            std::fs::rename(base.join("x-away"), base.join("x")).unwrap();
        }
    }

    #[gpui::test]
    async fn a_projects_row_s_fresh_capture_shows_nothing_from_before(cx: &mut TestAppContext) {
        assert_a_fresh_capture_shows_nothing_from_before(cx, press_projects_row).await;
    }

    /// History's Refresh that finds another repository's Git directory at
    /// the tab's own `.git`, as a re-clone at the path leaves it, captures
    /// it afresh, unmarked as it is: until its status read lands, Push and
    /// Commit refuse and nothing read of the replaced one is shown, and then
    /// Targets name its own remote. Opened again in place, the tab kept the
    /// status read before, so Push pushed the found repository's branch to
    /// its own remote while Targets named the other's (security probe N1,
    /// FU39-A5).
    #[gpui::test]
    async fn a_refresh_that_finds_another_git_directory_in_place_captures_it_afresh(
        cx: &mut TestAppContext,
    ) {
        assert_a_capture_shows_nothing_from_before(cx, Replaced::InPlace, |app, cx, _| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        })
        .await;
    }

    /// Show latest over a re-clone at the tab's path installs the found
    /// repository's handle; a Refresh after it compares the found repository
    /// with the one the shown status and remotes were read from, not with
    /// that handle, and captures it afresh. Compared with the handle, it
    /// compared the found repository with itself, kept the status read
    /// before, and Push pushed the found repository's `main` to its own
    /// remote while Targets named the other's (security probe S2, FU44-A).
    #[gpui::test]
    async fn a_refresh_after_show_latest_over_a_re_clone_captures_it_afresh(
        cx: &mut TestAppContext,
    ) {
        assert_a_capture_shows_nothing_from_before(cx, Replaced::InPlace, |app, cx, _| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_latest_history(window, cx)));
            for _ in 0..16 {
                if app.read_with(cx, |app, _| app.loading.is_none()) {
                    break;
                }
                app.read_with(cx, |app, _| app.worker.wait_idle());
                cx.executor().run_until_parked();
            }
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        })
        .await;
    }

    /// A Refresh compares the repository it finds with the one the shown
    /// working status and remotes were read from, never with the window's
    /// handle: with the handle on a re-clone at the tab's path already, the
    /// status and remotes shown still the replaced repository's, a Refresh
    /// that finds the re-clone forgets them before the found one's status
    /// lands. Compared with the handle, it compared the re-clone with itself
    /// and kept them.
    #[gpui::test]
    async fn a_refresh_compares_the_status_s_repository_not_the_window_s_handle(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{fixture_git, fixture_repository};
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let base = fixture.path().to_owned();
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let remote = base.join("a.git");
        fixture_git(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let read = app.read_with(cx, |app, _| {
            assert_eq!(app.remotes.len(), 1, "the remote read");
            app.working_identity.clone().expect("the status read")
        });
        let third = fixture_repository(base.join("third"));
        fixture_git(
            third.path(),
            &["commit", "-q", "--allow-empty", "-m", "Third"],
        );
        std::fs::rename(root.join(".git"), base.join("before-git-away")).unwrap();
        std::fs::rename(third.git_dir(), root.join(".git")).unwrap();
        let found = gitturtle_core::GitRepository::open(&root).unwrap();
        assert!(!same_git_directories(
            &read,
            &worker::GitDirectories::of(&found)
        ));
        // The handle on the re-clone, the status shown the other's.
        app.update(cx, |app, _| app.repository = Some(found));
        let held = app.read_with(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        for _ in 0..16 {
            if app.read_with(cx, |app, _| app.loading.is_none()) {
                break;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
        }
        app.read_with(cx, |app, _| {
            let subjects = (app.commits.iter())
                .map(|commit| commit.subject.as_str())
                .collect::<Vec<_>>();
            assert_eq!(subjects, ["Third"], "the snapshot installed");
            assert!(app.remotes.is_empty(), "no remote from before");
            assert!(app.work_status.is_none(), "no status from before");
            assert!(app.status_task.is_some(), "its status read waits");
        });
        drop(held);
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.remotes.is_empty(), "the re-clone has none");
            assert!(app.work_status.is_some(), "its own status read");
        });
    }

    /// A working status is labelled with the repository whose handle read
    /// it, the one the window held as it asked for the read, never with the
    /// handle it holds as the read lands: a read of one repository that
    /// lands after the handle moved to another names the one it read, which
    /// every later capture is compared with. Labelled at its landing, it
    /// named the other.
    #[gpui::test]
    async fn a_status_read_names_the_repository_that_read_it(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let other = crate::views::tests::fixture_repository(fixture.path().join("other"));
        let read = app.read_with(cx, |app, _| {
            worker::GitDirectories::of(app.repository.as_ref().expect("opened"))
        });
        let held = app.read_with(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        // The handle moves while the read waits.
        app.update(cx, |app, _| app.repository = Some(other.clone()));
        drop(held);
        for _ in 0..16 {
            cx.executor().run_until_parked();
            if app.read_with(cx, |app, _| app.status_task.is_none()) {
                break;
            }
            app.read_with(cx, |app, _| app.operations.drain());
        }
        app.read_with(cx, |app, _| {
            assert!(app.status_task.is_none(), "the status read landed");
            assert_eq!(app.working_identity.as_ref(), Some(&read));
            assert_ne!(read, worker::GitDirectories::of(&other));
        });
    }

    /// A status read asked for again while the first still waits in the
    /// operation executor takes it over with the newer request's handle:
    /// with the handle moved meanwhile to a re-clone at the tab's path, the
    /// status that lands is the re-clone's, labelled with it. The waiting
    /// read once kept the handle it was built with and answered the newer
    /// request, so the status was labelled with the replaced repository
    /// (security re-check O1, FU44-C2).
    #[gpui::test]
    async fn a_status_read_taken_over_names_the_newer_request_s_repository(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{fixture_git, fixture_repository};
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let base = fixture.path().to_owned();
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let replaced = app.read_with(cx, |app, _| {
            worker::GitDirectories::of(app.repository.as_ref().expect("opened"))
        });
        let third = fixture_repository(base.join("third"));
        fixture_git(
            third.path(),
            &["commit", "-q", "--allow-empty", "-m", "Third"],
        );
        let held = app.read_with(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        std::fs::rename(root.join(".git"), base.join("before-git-away")).unwrap();
        std::fs::rename(third.git_dir(), root.join(".git")).unwrap();
        let found = gitturtle_core::GitRepository::open(&root).unwrap();
        let found_directories = worker::GitDirectories::of(&found);
        assert!(!same_git_directories(&replaced, &found_directories));
        // The handle moves to the re-clone and Changes is read again while
        // the first read still waits.
        app.update(cx, |app, _| app.repository = Some(found));
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        drop(held);
        for _ in 0..16 {
            cx.executor().run_until_parked();
            if app.read_with(cx, |app, _| app.status_task.is_none()) {
                break;
            }
            app.read_with(cx, |app, _| app.operations.drain());
        }
        app.read_with(cx, |app, _| {
            assert!(app.status_task.is_none(), "the status read landed");
            assert_eq!(app.working_identity.as_ref(), Some(&found_directories));
        });
    }

    /// Show latest that finds a re-clone at the tab's path installs it in
    /// place as a fresh capture, as an accepted Refresh does: what was read
    /// of the replaced repository goes and the found one's status is read,
    /// so Push and Commit refuse until it lands and Targets then name the
    /// found one's remote. Installed over the status and remotes read
    /// before, Push pushed the found repository's `main` to its own remote
    /// while Targets named the other's (security probe S1, FU44-A).
    #[gpui::test]
    async fn show_latest_over_a_re_clone_captures_it_afresh(cx: &mut TestAppContext) {
        assert_a_capture_shows_nothing_from_before(cx, Replaced::InPlace, |app, cx, _| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_latest_history(window, cx)));
        })
        .await;
    }

    /// A Refresh that finds the tab's repository through a folder above its
    /// root re-pointed at a folder holding another repository by the same
    /// name, which the reopen follows (`worker::FoldersAbove`), captures it
    /// afresh: at other paths only the inodes tell the repository, and
    /// they differ. Compared only at the same path, it kept the status read
    /// before, and Push pushed the found repository's `main` to its own
    /// remote while Targets named the other's (security probe S3, FU44-A).
    #[cfg(unix)]
    #[gpui::test]
    async fn a_refresh_through_a_re_pointed_folder_above_captures_another_repository_afresh(
        cx: &mut TestAppContext,
    ) {
        assert_a_capture_shows_nothing_from_before(cx, Replaced::Above, |app, cx, _| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        })
        .await;
    }

    /// A quiet read of a Git change that finds a re-clone at the tab's path
    /// captures it afresh, as an accepted Refresh does: what was read of the
    /// replaced repository goes and the found one's status is read
    /// explicitly, so Push and Commit refuse until it lands. It once took
    /// the working state its read had gone through the window's handle for,
    /// by path, which may be either repository's.
    #[gpui::test]
    async fn a_quiet_read_that_finds_a_re_clone_captures_it_afresh(cx: &mut TestAppContext) {
        assert_a_capture_shows_nothing_from_before(cx, Replaced::InPlace, |app, cx, _| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.automatic.hold_quiet_reads(false);
                    app.queue_automatic_refresh(
                        crate::local_refresh::LocalChange {
                            git: true,
                            worktree: true,
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                })
            });
            quiet_reads_landed(app, cx);
            app.update(cx, |app, _| app.automatic.hold_quiet_reads(true));
        })
        .await;
    }

    #[gpui::test]
    async fn open_s_fresh_capture_shows_nothing_from_before(cx: &mut TestAppContext) {
        assert_a_fresh_capture_shows_nothing_from_before(cx, |_, cx, root| {
            cx.update(|window, cx| window.dispatch_action(Box::new(crate::OpenRepository), cx));
            assert!(cx.did_prompt_for_paths(), "Open… asks for a folder");
            cx.simulate_path_prompt_response(|_| Some(vec![root.to_owned()]));
        })
        .await;
    }

    #[gpui::test]
    async fn a_local_workspaces_row_s_fresh_capture_shows_nothing_from_before(
        cx: &mut TestAppContext,
    ) {
        assert_a_fresh_capture_shows_nothing_from_before(cx, |app, cx, _| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.toggle_repository_pin(window, cx);
                    app.open_repository_library(window, cx);
                })
            });
            crate::views::tests::draw(cx);
            let open = crate::tags::tests::rendered(cx, "open-library-repo-0");
            cx.simulate_click(open.center(), gpui::Modifiers::default());
        })
        .await;
    }

    /// Until an Open's status read lands, the branch picker names the branch
    /// the accepted snapshot found checked out, or claims none where it
    /// found none, and Fetch, Pull and Push refuse saying the read, as
    /// Changes says it, as does Targets' destination line. The palette's
    /// Branch opens the snapshot's branch, as the picker's "Current branch
    /// settings…" does, and says the read where the snapshot found none.
    /// With no status they read a detached HEAD and no remote: "Detached
    /// HEAD", "Switch to a local branch before pulling or pushing", "No
    /// configured remote selected · Check Targets", "No remote configured"
    /// and "Check out a named local branch first"; then Branch refused the
    /// branch the picker named, saying the read.
    #[gpui::test]
    async fn an_open_says_its_status_is_being_read_until_it_lands(cx: &mut TestAppContext) {
        use crate::views::tests::{fixture_git, fixture_repository};
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let topic = fixture_repository(fixture.path().join("topic"));
        fixture_git(
            topic.path(),
            &["commit", "-q", "--allow-empty", "-m", "Topic"],
        );
        fixture_git(topic.path(), &["switch", "-q", "-c", "topic"]);
        let detached = fixture_repository(fixture.path().join("detached"));
        fixture_git(
            detached.path(),
            &["commit", "-q", "--allow-empty", "-m", "One"],
        );
        fixture_git(detached.path(), &["switch", "-q", "--detach"]);
        let picker = || crate::branch_actions::PICKER_LABEL.with(|label| label.borrow().clone());
        let tooltip = |id: &str| {
            crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow().get(id).cloned())
        };
        let reading = crate::workspace::READING_WORKING_TREE;
        let destination = || {
            crate::workspace::TARGETS_DESTINATION
                .with(|line| line.borrow().as_ref().map(ToString::to_string))
        };
        let branch_reason = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                (app.read(cx)).menu_command_reason(
                    crate::command_palette::CommandId::Branch,
                    window,
                    cx,
                )
            })
        };
        for (repository, while_read, branch_while_read, read, branch_once_read) in [
            (&topic, "topic", None, "topic", None),
            (
                &detached,
                crate::branch_actions::READING,
                Some(reading),
                "Detached HEAD",
                Some("Check out a named local branch first"),
            ),
        ] {
            let held = app.read_with(cx, |app, _| app.operations.hold());
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.open_repository_tab(repository.path().to_owned(), window, cx);
                    app.git_actions_open = true;
                })
            });
            for _ in 0..16 {
                let installed = app.read_with(cx, |app, _| {
                    app.repository.as_ref().map(|repo| repo.git_dir()) == Some(repository.git_dir())
                });
                if installed {
                    break;
                }
                app.read_with(cx, |app, _| app.worker.wait_idle());
                cx.executor().run_until_parked();
            }
            assert!(
                app.read_with(cx, |app, _| app.work_status.is_none()
                    && app.status_task.is_some()),
                "{while_read}: its status read waits"
            );
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            assert_eq!(
                picker().as_deref(),
                Some(while_read),
                "the picker while read"
            );
            for id in ["fetch", "pull", "push"] {
                assert!(cx.debug_bounds(id).is_some(), "{while_read}: {id} drawn");
                let refusal = crate::workspace::refusals::get(id).expect("drawn");
                assert!(
                    refusal.announced && !refusal.dimmed,
                    "{while_read}: {id} refuses, its look waiting"
                );
                let said = tooltip(id).unwrap_or_default();
                assert!(said.ends_with(reading), "{while_read}: {id} says {said}");
            }
            // The look comes once the read has lasted a moment, as a
            // write's does (`GitTurtle::track_unread_look`).
            cx.executor()
                .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            for id in ["fetch", "pull", "push"] {
                let refusal = crate::workspace::refusals::get(id).expect("drawn");
                assert!(
                    refusal.announced && refusal.dimmed,
                    "{while_read}: {id} looks refused"
                );
            }
            assert_eq!(
                destination().as_deref(),
                Some(reading),
                "{while_read}: Targets' destination"
            );
            assert_eq!(
                branch_reason(cx),
                branch_while_read,
                "{while_read}: the palette's Branch"
            );
            if branch_while_read.is_none() {
                // It opens the snapshot's branch, its settings read as the
                // picker's "Current branch settings…" reads them.
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        let path = app.path.clone();
                        app.run_menu_command(
                            crate::command_palette::CommandId::Branch,
                            &path,
                            window,
                            cx,
                        )
                    })
                });
                assert_eq!(
                    app.read_with(cx, |app, _| app.operation_busy),
                    Some(crate::branch_actions::PREPARING),
                    "{while_read}: Branch reads its settings"
                );
                app.update(cx, |app, _| app.cancel_branch_action());
            }
            drop(held);
            settle_tab_test(&app, cx).await;
            assert_eq!(picker().as_deref(), Some(read), "the picker once read");
            for id in ["fetch", "pull", "push"] {
                let said = tooltip(id).unwrap_or_default();
                assert!(!said.ends_with(reading), "{read}: {id} says {said}");
            }
            let line = destination().unwrap_or_default();
            assert!(
                line.starts_with("No remote configured"),
                "{read}: Targets once read says {line}"
            );
            assert_eq!(
                branch_reason(cx),
                branch_once_read,
                "{read}: Branch once read"
            );
        }
    }

    /// Before an Open's snapshot lands, with no repository installed and no
    /// status read started, the status is not read yet either: the branch
    /// picker reads "Reading…" and Fetch, Pull and Push refuse saying the
    /// read, for an Open of a new path from History and for a cold switch
    /// to a tab that is not marked. Until the snapshot landed they said
    /// "Detached HEAD" and "No configured remote selected · Check Targets"
    /// (FU39-A5 review probe).
    #[gpui::test]
    async fn an_open_says_its_status_is_being_read_before_its_snapshot_lands(
        cx: &mut TestAppContext,
    ) {
        use crate::views::tests::{fixture_git, fixture_repository};
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        crate::views::tests::hold_quiet_reads_for_test(&app, cx);
        let topic = fixture_repository(fixture.path().join("topic"));
        fixture_git(
            topic.path(),
            &["commit", "-q", "--allow-empty", "-m", "Topic"],
        );
        fixture_git(topic.path(), &["switch", "-q", "-c", "topic"]);
        let picker = || crate::branch_actions::PICKER_LABEL.with(|label| label.borrow().clone());
        let tooltip = |id: &str| {
            crate::views::CONTROL_TOOLTIPS.with(|tooltips| tooltips.borrow().get(id).cloned())
        };
        let reading = crate::workspace::READING_WORKING_TREE;
        let assert_reading = |cx: &mut VisualTestContext, route: &str| {
            assert!(
                app.read_with(cx, |app, _| app.page == AppPage::Repository
                    && app.repository.is_none()
                    && app.loading.is_some()
                    && app.status_task.is_none()),
                "{route}: its snapshot read waits"
            );
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            assert_eq!(
                picker().as_deref(),
                Some(crate::branch_actions::READING),
                "{route}: the picker"
            );
            for id in ["fetch", "pull", "push"] {
                assert!(cx.debug_bounds(id).is_some(), "{route}: {id} drawn");
                let refusal = crate::workspace::refusals::get(id).expect("drawn");
                assert!(refusal.announced, "{route}: {id} refuses");
                let said = tooltip(id).unwrap_or_default();
                assert!(said.ends_with(reading), "{route}: {id} says {said}");
            }
            // Looking it once the read has lasted a moment
            // (`GitTurtle::track_unread_look`).
            cx.executor()
                .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            for id in ["fetch", "pull", "push"] {
                let refusal = crate::workspace::refusals::get(id).expect("drawn");
                assert!(refusal.dimmed, "{route}: {id} looks refused");
            }
        };

        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(topic.path().to_owned(), window, cx);
                app.git_actions_open = true;
            })
        });
        assert_reading(cx, "an Open from History");
        drop(held);
        settle_tab_test(&app, cx).await;
        assert_eq!(picker().as_deref(), Some("topic"), "the Open's branch");

        app.update(cx, |app, _| app.repository_tabs.tabs[0].warm = None);
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.switch_repository_tab(0, window, cx);
                app.git_actions_open = true;
            })
        });
        assert_reading(cx, "a cold switch");
        drop(held);
        settle_tab_test(&app, cx).await;
        assert_eq!(picker().as_deref(), Some("main"), "the switch's branch");
        for id in ["fetch", "pull", "push"] {
            let said = tooltip(id).unwrap_or_default();
            assert!(!said.ends_with(reading), "read: {id} says {said}");
        }
    }

    /// A row whose fresh capture fails keeps the tab's root, Git directories
    /// and mark, and says what the Open found, as Open and the Local
    /// workspaces row do: the tab shown with that failure, which the Projects
    /// page says too when it is opened again.
    #[gpui::test]
    async fn a_projects_row_whose_capture_fails_keeps_the_marked_tab(cx: &mut TestAppContext) {
        let (fixture, app, cx, root, enclosing) = gone_inside_another(cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "Refresh");
        let (git_dir, mark) = app.read_with(cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            (tab.git_dir.clone(), tab.error.clone())
        });
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", fixture.path().join("nowhere").display()),
        )
        .unwrap();
        press_projects_row(&app, cx, &root);
        settle_tab_test(&app, cx).await;
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        hub.read_with(cx, |hub, _| assert!(!hub.shown_busy(), "the wait is over"));
        let said = hub.read_with(cx, |hub, _| hub.shown_error().map(str::to_owned));
        let said = said.expect("what the Open found");
        assert!(!said.contains("is gone"), "not the mark's reason: {said}");
        app.read_with(cx, |app, _| {
            assert_eq!(app.page, AppPage::Repository, "the tab shown");
            assert_eq!(app.error.as_deref(), Some(said.as_str()));
            let tab = &app.repository_tabs.tabs[0];
            assert_eq!(tab.path, root, "its root");
            assert_eq!(tab.git_dir, git_dir, "its Git directories");
            assert_eq!(tab.error, mark, "its mark as it was");
            assert!(!app.repository_tabs.saved().tabs[0].unresolved);
        });
    }

    /// A row's fresh capture refused as the switch to its tab is, past the
    /// retained allowance, ends the page's wait and says why under the row,
    /// as the tab limit's refusal is said, and the tab strip says nothing.
    /// Left waiting, every action on the page stayed refused under "Opening
    /// project…" with nothing read; then it ended the wait and said why in
    /// the strip. A whole tab's row refused so left the page waiting.
    #[gpui::test]
    async fn a_projects_row_refused_its_switch_ends_the_pages_wait(cx: &mut TestAppContext) {
        let (fixture, app, cx, root, third_git) = marked_by_a_swapped_git(cx).await;
        let other = fixture.path().join("other");
        open_another_tab(&app, cx, &other).await;
        open_another_tab(&app, cx, &fixture.path().join("another")).await;
        let other = app.read_with(cx, |app, _| app.repository_tabs.tabs[1].path.clone());
        let bytes = app.update(cx, |app, _| {
            let warm = app.repository_tabs.tabs[1]
                .warm
                .as_mut()
                .expect("the second tab retained");
            std::mem::replace(&mut warm.bytes, MAX_RETAINED_BYTES + 1)
        });
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        for (row, what) in [
            (&root, "the marked tab's row"),
            (&other, "a whole tab's row"),
        ] {
            press_projects_row(&app, cx, row);
            settle_tab_test(&app, cx).await;
            hub.read_with(cx, |hub, _| {
                assert!(!hub.shown_busy(), "{what}: the wait is over");
                assert!(
                    hub.shown_error()
                        .is_some_and(|said| said == RETAINED_ALLOWANCE_REFUSAL),
                    "{what}: said under the row"
                );
            });
            app.read_with(cx, |app, _| {
                assert_eq!(app.page, AppPage::Projects, "{what}");
                assert_eq!(app.repository_tabs.active, Some(2), "{what}: refused");
                assert_eq!(
                    app.repository_tabs.refusal, None,
                    "{what}: not in the strip"
                );
                let tab = &app.repository_tabs.tabs[0];
                assert!(tab.error.is_some(), "{what}: its mark as it was");
                assert!(tab.warm.is_some(), "{what}: what it retained kept");
            });
        }

        // A refused switch ends only the wait its own Open began: an Open…
        // of the marked tab's folder while the page waits for a Clone says
        // why in the tab strip, the Clone's wait going on.
        cx.update(|window, cx| {
            hub.update(cx, |hub, cx| hub.wait_for_clone(cx));
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            });
        });
        crate::views::tests::draw(cx);
        hub.read_with(cx, |hub, _| {
            assert!(hub.shown_busy(), "the Clone's wait goes on")
        });
        app.read_with(cx, |app, _| {
            assert_eq!(
                app.repository_tabs.refusal.as_deref(),
                Some(RETAINED_ALLOWANCE_REFUSAL),
                "the tab strip says why"
            );
        });
        hub.update(cx, |hub, cx| hub.set_busy(false, cx));
        app.update(cx, |app, _| {
            let warm = app.repository_tabs.tabs[1]
                .warm
                .as_mut()
                .expect("the second tab retained");
            warm.bytes = bytes;
        });
        // Asked again, with the allowance back, it reads.
        press_projects_row(&app, cx, &root);
        settle_tab_test(&app, cx).await;
        assert_recaptured(&app, cx, 0, &root, &third_git);
    }

    /// Open… of a marked tab's folder chosen while a write runs, which may
    /// start after the picker opened, captures nothing: it says in the tab
    /// strip that it waits for the write, in the words the palette and the
    /// Projects page give it, and the tab keeps its mark, root and Git
    /// directory, as the Projects and pane rows refuse it. It once captured
    /// the tab afresh, and the write, failing safe, then named the replaced
    /// repository on the fresh tab (FU39-A security review). Asked again
    /// once the write ends, it captures the tab.
    #[gpui::test]
    async fn open_refuses_to_capture_a_marked_tab_while_a_write_runs(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root, third_git) = marked_by_a_swapped_git(cx).await;
        let open = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window.dispatch_action(Box::new(crate::OpenRepository), cx));
            assert!(cx.did_prompt_for_paths(), "Open… asks for a folder");
        };
        let captured = app.read_with(cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            (
                tab.path.clone(),
                tab.git_dir.as_ref().map(|d| d.git_dir.clone()),
            )
        });
        open(cx);
        app.update(cx, |app, cx| {
            app.operation_busy = Some("Pushing branch…");
            cx.notify();
        });
        cx.simulate_path_prompt_response(|_| Some(vec![root.clone()]));
        settle_tab_test(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(
                app.repository_tabs.refusal.as_deref(),
                Some(crate::workspace::BUSY_REASON),
                "the tab strip says it waits for the write"
            );
            let tab = &app.repository_tabs.tabs[0];
            assert!(tab.error.is_some(), "its mark as it was");
            assert_eq!(
                (
                    tab.path.clone(),
                    tab.git_dir.as_ref().map(|d| d.git_dir.clone())
                ),
                captured,
                "its root and Git directory as they were"
            );
            assert!(app.repository_tabs.recapture.is_none(), "nothing read");
            assert_eq!(app.loading, None, "nothing read");
        });
        assert!(
            cx.debug_bounds("repository-tab-refusal").is_some(),
            "said beneath the strip"
        );

        app.update(cx, |app, cx| {
            app.operation_busy = None;
            cx.notify();
        });
        open(cx);
        cx.simulate_path_prompt_response(|_| Some(vec![root.clone()]));
        settle_tab_test(&app, cx).await;
        assert_recaptured(&app, cx, 0, &root, &third_git);
    }

    /// A refused switch changes nothing on a marked tab: an explicit Open of
    /// its folder from another tab, with the tabs retained past their
    /// allowance or with a dialog open, opens nothing and leaves the tab its
    /// root, Git directory, mark and what it retained, so shown again and
    /// refreshed it is a passive reopen still, which never opens the work
    /// tree enclosing it. It once cleared the root and Git directory first,
    /// leaving the tab with nothing captured, saved as unresolved, and that
    /// Refresh opened the enclosing repository in the tab.
    #[gpui::test]
    async fn a_refused_open_of_a_marked_tab_s_folder_changes_nothing_on_it(
        cx: &mut TestAppContext,
    ) {
        let (fixture, app, cx, root, enclosing) = gone_inside_another(cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "Refresh");
        let (git_dir, mark) = app.read_with(cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            (tab.git_dir.clone(), tab.error.clone())
        });
        open_another_tab(&app, cx, &fixture.path().join("other")).await;
        open_another_tab(&app, cx, &fixture.path().join("another")).await;
        let unchanged = |cx: &mut VisualTestContext, after: &str| {
            app.read_with(cx, |app, _| {
                assert_eq!(app.repository_tabs.active, Some(2), "{after}: refused");
                let tab = &app.repository_tabs.tabs[0];
                assert_eq!(tab.path, root, "{after}");
                assert!(tab.resolved, "{after}: its root as captured");
                assert_eq!(tab.git_dir, git_dir, "{after}: its Git directory");
                assert_eq!(tab.error, mark, "{after}: its mark as it was");
                assert!(tab.warm.is_some(), "{after}: what it retained kept");
                assert!(!app.repository_tabs.saved().tabs[0].unresolved, "{after}");
            });
        };
        let bytes = app.update(cx, |app, _| {
            let warm = app.repository_tabs.tabs[1]
                .warm
                .as_mut()
                .expect("the second tab retained");
            std::mem::replace(&mut warm.bytes, MAX_RETAINED_BYTES + 1)
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        unchanged(cx, "past the allowance");
        assert!(
            app.read_with(cx, |app, _| app
                .repository_tabs
                .refusal
                .as_deref()
                .is_some_and(|refusal| refusal.contains("512 MiB"))),
            "the tab strip says why"
        );
        app.update(cx, |app, _| {
            app.repository_tabs.tabs[1]
                .warm
                .as_mut()
                .expect("the second tab retained")
                .bytes = bytes;
        });

        cx.update(|window, cx| window.open_dialog(cx, |dialog, _, _| dialog.title("Probe")));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        unchanged(cx, "a dialog open");
        cx.update(|window, cx| window.close_dialog(cx));

        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "Refresh, shown again");
    }

    /// An explicit Open of a marked tab's folder superseded before its read
    /// lands leaves the tab as it was: Refresh pressed meanwhile is a passive
    /// reopen still, which finds the repository gone and never opens the
    /// work tree enclosing it. With the tab's root and Git directory cleared
    /// as the Open was asked, that Refresh once opened the enclosing
    /// repository in the tab.
    #[gpui::test]
    async fn a_superseded_open_of_a_marked_tab_s_folder_leaves_it_as_it_was(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx, root, enclosing) = gone_inside_another(cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "Refresh");
        let git_dir = app.read_with(cx, |app, _| app.repository_tabs.tabs[0].git_dir.clone());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx);
                app.refresh(&Refresh, window, cx);
            })
        });
        settle_tab_test(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "Refresh after the Open");
        app.read_with(cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            assert!(tab.resolved && tab.git_dir == git_dir, "as captured");
            assert!(!app.repository_tabs.saved().tabs[0].unresolved);
        });
    }

    /// The History fixture inside another work tree, marked " ·
    /// unavailable" by Refresh with its `.git` away, and an explicit Open of
    /// its folder that fails, the folder's `.git` a gitfile to nowhere: from
    /// the tab itself or, `from_another`, from a second tab shown meanwhile.
    /// The tab keeps the root, Git directory and mark it had, saved as
    /// resolved, and the window says why the Open failed. With that gitfile
    /// gone, Refresh, a passive reopen still, finds the repository gone, and
    /// Show latest with a gitfile to the enclosing repository's Git
    /// directory, the tab's captured one named, installs none of its
    /// history; neither ever puts the enclosing repository where a write
    /// would go. The failed Open once left the tab
    /// with nothing captured, saved as unresolved, and its next Refresh, an
    /// explicit open too, opened the enclosing repository in the tab.
    async fn a_failed_open_keeps_the_marked_tab(cx: &mut TestAppContext, from_another: bool) {
        let (fixture, app, cx, root, enclosing) = gone_inside_another(cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "Refresh");
        let (git_dir, mark) = app.read_with(cx, |app, _| {
            let tab = &app.repository_tabs.tabs[0];
            (
                tab.git_dir.clone().expect("captured as it opened"),
                tab.error.clone().expect("marked"),
            )
        });
        if from_another {
            open_another_tab(&app, cx, &fixture.path().join("other")).await;
            assert_eq!(
                app.read_with(cx, |app, _| app.repository_tabs.active),
                Some(1)
            );
        }
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", fixture.path().join("nowhere").display()),
        )
        .unwrap();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        let kept = |cx: &mut VisualTestContext, after: &str| {
            app.read_with(cx, |app, _| {
                assert_eq!(app.repository_tabs.active, Some(0), "{after}: shown");
                let tab = &app.repository_tabs.tabs[0];
                assert_eq!(tab.path, root, "{after}: its root");
                assert!(tab.resolved, "{after}: its root as captured");
                assert_eq!(tab.git_dir.as_ref(), Some(&git_dir), "{after}");
                assert!(
                    app.tab_label(tab).0.ends_with(" · unavailable"),
                    "{after}: marked"
                );
                assert!(
                    app.repository
                        .as_ref()
                        .is_none_or(|repo| repo.path() == root
                            && worker::GitDirectories::of(repo) == git_dir),
                    "{after}: writes would go to no other repository"
                );
                let saved = app.repository_tabs.saved();
                assert_eq!(saved.tabs[0].path.path(), root, "{after}");
                assert!(!saved.tabs[0].unresolved, "{after}: saved as resolved");
            });
        };
        kept(cx, "the failed Open");
        app.read_with(cx, |app, _| {
            assert_eq!(
                app.repository_tabs.tabs[0].error.as_ref(),
                Some(&mark),
                "its mark as it was"
            );
            let error = app
                .error
                .clone()
                .expect("the window says why the Open failed");
            assert!(error != mark && !error.contains("is gone"), "{error}");
        });

        std::fs::remove_file(root.join(".git")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        kept(cx, "Refresh");
        assert_gone(&app, cx, 0, &root, &enclosing, "Refresh");

        let enclosing_git = gitturtle_core::GitRepository::open(&enclosing)
            .unwrap()
            .git_dir()
            .to_owned();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", enclosing_git.display()),
        )
        .unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_latest_history(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        kept(cx, "Show latest");
        assert!(
            app.read_with(cx, |app, _| app.commits.is_empty()),
            "Show latest: no enclosing history"
        );
    }

    #[gpui::test]
    async fn a_failed_open_of_the_shown_marked_tab_s_folder_keeps_what_it_captured(
        cx: &mut TestAppContext,
    ) {
        a_failed_open_keeps_the_marked_tab(cx, false).await;
    }

    #[gpui::test]
    async fn a_failed_open_of_a_marked_tab_s_folder_from_another_tab_keeps_what_it_captured(
        cx: &mut TestAppContext,
    ) {
        a_failed_open_keeps_the_marked_tab(cx, true).await;
    }

    /// What the window says for a failed fresh capture of a marked tab's
    /// folder goes with the mark it left in place: with the repository's
    /// `.git` back, a quiet read that finds it readable clears both and
    /// brings History back, where the Open's failure would have stayed
    /// beside it, its text not the mark's.
    #[gpui::test]
    async fn a_failed_open_s_failure_goes_with_the_mark_it_kept(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let enclosing = enclosing_work_tree(fixture.path());
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
        crate::views::tests::settle(&app, cx).await;
        assert_gone(&app, cx, 0, &root, &enclosing, "Refresh");
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", fixture.path().join("nowhere").display()),
        )
        .unwrap();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        let error = app.read_with(cx, |app, _| app.error.clone());
        assert!(
            error.is_some_and(|error| !error.contains("is gone")),
            "the Open's failure"
        );
        std::fs::remove_file(root.join(".git")).unwrap();
        std::fs::rename(fixture.path().join("narrow-git-away"), root.join(".git")).unwrap();
        work_tree_changed(&app, cx).await;
        assert_eq!(
            first_tab(&app, cx),
            (
                1,
                root.clone(),
                false,
                None,
                Some(root),
                vec!["fixture notes".into()]
            ),
            "readable again"
        );
    }

    /// Lands the quiet read a change in the shown tab's work tree asks for,
    /// as the watcher would report it, with no watcher needed.
    async fn work_tree_changed(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let change = crate::local_refresh::LocalChange {
                    worktree: true,
                    ..Default::default()
                };
                app.queue_automatic_refresh(change, window, cx)
            })
        });
        for _ in 0..16 {
            cx.executor().run_until_parked();
            if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
                break;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
        }
        crate::views::tests::settle(app, cx).await;
    }

    /// A failed fresh capture's word belongs to its own tab: another marked
    /// tab found readable clears its own mark and leaves the first tab's
    /// failure on the Projects page, that tab still unavailable. Kept for
    /// whichever tab was found readable next, the failure once went with
    /// another tab's mark.
    #[gpui::test]
    async fn another_tab_found_readable_keeps_a_failed_open_s_failure(cx: &mut TestAppContext) {
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        enclosing_work_tree(fixture.path());
        open_another_tab(&app, cx, &fixture.path().join("other")).await;
        let other = app.read_with(cx, |app, _| app.path.clone().expect("the second tab"));
        let refresh = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)))
        };
        std::fs::rename(other.join(".git"), fixture.path().join("other-git-away")).unwrap();
        refresh(cx);
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        std::fs::rename(root.join(".git"), fixture.path().join("narrow-git-away")).unwrap();
        refresh(cx);
        settle_tab_test(&app, cx).await;
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", fixture.path().join("nowhere").display()),
        )
        .unwrap();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(root.clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        let failure = app.read_with(cx, |app, cx| {
            let failure = app.error.clone().expect("the Open's failure");
            assert_eq!(app.hub.read(cx).shown_error(), Some(failure.as_str()));
            assert!(
                app.repository_tabs
                    .tabs
                    .iter()
                    .all(|tab| tab.error.is_some())
            );
            failure
        });

        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        settle_tab_test(&app, cx).await;
        std::fs::rename(fixture.path().join("other-git-away"), other.join(".git")).unwrap();
        work_tree_changed(&app, cx).await;
        app.read_with(cx, |app, cx| {
            let tabs = &app.repository_tabs.tabs;
            assert_eq!(app.repository_tabs.active, Some(1));
            assert!(tabs[1].error.is_none(), "the second tab readable");
            assert!(tabs[0].error.is_some(), "the first still unavailable");
            assert_eq!(
                app.hub.read(cx).shown_error(),
                Some(failure.as_str()),
                "its failure kept"
            );
        });
    }

    /// A full local library refuses a repository where the person asked:
    /// Create workspace in the Local workspaces dialog says why in the
    /// dialog's own error line, announced at once, lists nothing as added
    /// and draws nothing beneath the tab strip, under the dialog's
    /// backdrop, where the refusal once went while the dialog's list
    /// showed the repository added. Add current on the group is refused
    /// the same way, and once the library has room it adds the repository
    /// and the line goes: it once still said the repository was refused
    /// beside the group that listed it. Each refusal is a fresh node,
    /// announced again: the line kept one node, so the second refusal,
    /// in the same words, went unheard.
    #[gpui::test]
    async fn a_full_library_refuses_in_the_local_workspaces_dialog(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        use gpui::Modifiers;
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        app.update(cx, |app, _| {
            app.repository_tabs.library = (0..MAX_LIBRARY)
                .map(|kept| LibraryEntry {
                    path: SavedPath::new(&fixture.path().join(format!("kept-{kept}"))),
                    pinned: false,
                    group: None,
                })
                .collect();
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_library(window, cx)));
        crate::views::tests::draw(cx);
        let name = cx
            .debug_bounds("local-workspace-name")
            .expect("the name field");
        cx.simulate_click(name.center(), Modifiers::default());
        cx.simulate_input("Team");
        let create = cx
            .debug_bounds("create-local-workspace")
            .expect("Create workspace");
        cx.simulate_click(create.center(), Modifiers::default());
        written::forget();
        crate::views::tests::draw(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.library.len(), MAX_LIBRARY, "refused");
            assert_eq!(app.repository_tabs.groups, vec!["Team".to_owned()]);
            assert_eq!(
                app.repository_tabs.refusal, None,
                "nothing beneath the strip"
            );
            assert_eq!(app.error, None);
        });
        let refusal = |what: &str| {
            let announced = written::announced_from("local-workspaces-error");
            assert!(
                announced.len() == 1 && announced[0].1 == written::Announced::ASSERTIVE,
                "{what}: the dialog says why, at once: {announced:?}"
            );
            announced[0].0.clone()
        };
        let created = refusal("Create workspace");
        let team = || {
            written::get("local-workspace-name-0")
                .and_then(|written| written.node.value().map(str::to_owned))
                .expect("the group's line")
        };
        assert_eq!(team(), "Team · 0 repositories");
        let add = cx
            .debug_bounds("assign-workspace-0")
            .expect("Team's Add current");
        cx.simulate_click(add.center(), Modifiers::default());
        written::forget();
        crate::views::tests::draw(cx);
        assert!(
            cx.debug_bounds("local-workspaces-error").is_some(),
            "Add current is refused in the dialog too"
        );
        assert_ne!(
            refusal("Add current"),
            created,
            "the same refusal again is a fresh node, announced again"
        );
        assert_eq!(team(), "Team · 0 repositories", "nothing added");
        app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.library.len(), MAX_LIBRARY, "refused");
        });
        app.update(cx, |app, _| {
            app.repository_tabs.library.pop();
        });
        cx.simulate_click(add.center(), Modifiers::default());
        written::forget();
        crate::views::tests::draw(cx);
        assert!(
            cx.debug_bounds("local-workspaces-error").is_none(),
            "added, the dialog no longer says it was refused"
        );
        assert_eq!(team(), "Team · 1 repository", "the group lists it");
        app.read_with(cx, |app, _| {
            let root = app.path.clone().expect("a repository");
            assert!(
                app.repository_tabs.library.iter().any(
                    |entry| entry.path.path() == root && entry.group.as_deref() == Some("Team")
                ),
                "added to the library, in Team"
            );
        });
    }

    /// `app` holding `count` tabs, those past its own in folders under `root`.
    fn hold_tabs(app: &mut GitTurtle, root: &Path, count: usize) {
        for held in app.repository_tabs.tabs.len()..count {
            app.repository_tabs.tabs.push(Tab {
                path: root.join(format!("held-{held}")),
                resolved: true,
                git_dir: None,
                saved: Bookmark::default(),
                warm: None,
                error: None,
            });
        }
    }

    /// At the tab limit no folder is asked for: every route that opens a new
    /// tab through the folder picker says the limit's refusal at once, where
    /// the person acted, and one tab short of the limit each still asks.
    /// Asked first, a person chose a folder only to be told no tab could
    /// hold it.
    #[gpui::test]
    async fn a_new_tab_at_the_limit_is_refused_before_the_picker(cx: &mut TestAppContext) {
        use crate::command_palette::CommandId;
        use crate::native_accessibility::written;
        use crate::views::tests::{draw, settle};
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = fixture.path().to_owned();
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        // Compare draws the rail in this width, its folder among its controls.
        cx.simulate_resize(size(px(1000.), px(980.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle(&app, cx).await;
        draw(cx);
        let key = |name: &str| crate::shortcuts::resolve_key(name, cfg!(target_os = "macos"));
        let (new_tab, open) = (key("primary-t"), key("primary-o"));
        let focus = app.read_with(cx, |app, _| app.app_focus.clone());
        cx.update(|window, cx| window.focus(&focus, cx));
        let click = |cx: &mut VisualTestContext, selector: &'static str| {
            let bounds = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is drawn"));
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
        };
        let press_palette = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.run_palette_command(CommandId::Open, window, cx)
                })
            })
        };
        type Press<'a> = Box<dyn Fn(&mut VisualTestContext) + 'a>;
        let routes: Vec<(&str, Press)> = vec![
            (
                "the New Tab shortcut",
                Box::new(|cx| cx.simulate_keystrokes(&new_tab)),
            ),
            (
                "the Open shortcut",
                Box::new(|cx| cx.simulate_keystrokes(&open)),
            ),
            (
                "the menu's action",
                Box::new(|cx| {
                    cx.update(|window, cx| window.dispatch_action(Box::new(OpenRepository), cx))
                }),
            ),
            ("the command palette", Box::new(press_palette)),
            (
                "the strip's +",
                Box::new(move |cx| click(cx, "new-repository-tab")),
            ),
            (
                "the rail's folder",
                Box::new(move |cx| click(cx, "rail-open")),
            ),
        ];
        app.update(cx, |app, _| hold_tabs(app, &root, MAX_TABS - 1));
        draw(cx);
        for (route, press) in &routes {
            press(cx);
            assert!(cx.did_prompt_for_paths(), "{route}: below the limit, asked");
            cx.simulate_path_prompt_response(|_| None);
            settle(&app, cx).await;
            assert_eq!(
                app.read_with(cx, |app, _| app.repository_tabs.refusal.clone()),
                None,
                "{route}: nothing refused below the limit"
            );
        }
        app.update(cx, |app, _| hold_tabs(app, &root, MAX_TABS));
        draw(cx);
        let mut nodes = Vec::new();
        for (route, press) in &routes {
            written::forget();
            press(cx);
            assert!(
                !cx.did_prompt_for_paths(),
                "{route}: no folder is asked for"
            );
            draw(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.repository_tabs.refusal.clone())
                    .as_deref(),
                Some(TAB_LIMIT_REFUSAL),
                "{route}: refused in the limit's own words"
            );
            assert!(
                cx.debug_bounds("repository-tab-refusal").is_some(),
                "{route}: the strip says so"
            );
            let announced = written::announced_from("repository-tab-refusal");
            assert!(
                announced.len() == 1 && announced[0].1 == written::Announced::ASSERTIVE,
                "{route}: announced at once: {announced:?}"
            );
            nodes.push(announced[0].0.clone());
            app.update(cx, |app, cx| {
                app.repository_tabs.refusal = None;
                cx.notify();
            });
            draw(cx);
        }
        nodes.dedup();
        assert_eq!(
            nodes.len(),
            routes.len(),
            "each refusal is a fresh node, announced again"
        );
        assert_eq!(
            app.read_with(cx, |app, _| app.repository_tabs.tabs.len()),
            MAX_TABS,
            "no tab was opened or marked"
        );
        // The Projects page's own picker is refused on the page, as a recent
        // project's Open is, and one tab short of the limit it still asks.
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.show_projects(window, cx);
                app.refuse_in_tab_strip("An earlier refusal.", cx);
            })
        });
        draw(cx);
        written::forget();
        click(cx, "hub-open-folder");
        cx.run_until_parked();
        draw(cx);
        assert!(!cx.did_prompt_for_paths(), "no folder is asked for");
        hub.read_with(cx, |hub, _| {
            assert_eq!(
                hub.shown_error(),
                Some(TAB_LIMIT_REFUSAL),
                "the page says why"
            );
            assert!(!hub.shown_busy(), "its actions stay available");
        });
        let announced = written::announced_from("project-hub-error");
        assert!(
            announced.len() == 1 && announced[0].1 == written::Announced::ASSERTIVE,
            "announced at once: {announced:?}"
        );
        assert!(
            cx.debug_bounds("repository-tab-refusal").is_none(),
            "said once, where the person acted"
        );
        app.update(cx, |app, _| {
            app.repository_tabs.tabs.truncate(MAX_TABS - 1);
        });
        click(cx, "hub-open-folder");
        cx.run_until_parked();
        assert!(cx.did_prompt_for_paths(), "below the limit, asked");
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();
    }

    /// Choose a repository… refused at the limit says so in the place the
    /// person is looking, directly under the button and inside the visible
    /// body, in a half tile at 13 and 18 pt and in the full window: drawn at
    /// the action panel's foot, the refusal lay about 100 px past the view
    /// at 18 pt in a 461 × 490 window, and the press looked dead.
    #[gpui::test]
    async fn a_refusal_under_choose_a_repository_is_in_view(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep;
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        app.update(cx, |app, _| hold_tabs(app, &root, MAX_TABS));
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
        for (window, interface) in [
            (size(px(461.), px(490.)), 13),
            (size(px(461.), px(490.)), 18),
            (size(px(1480.), px(973.)), 13),
        ] {
            sweep::prepare(&app, cx);
            cx.simulate_resize(window);
            sweep::interface_size(cx, interface);
            hub.update(cx, |hub, cx| hub.set_error(None, cx));
            crate::views::tests::draw(cx);
            // The worst place the person can have pressed it from: scrolled
            // only as far as it takes to show the button at the body's foot.
            let scroll = hub.read_with(cx, |hub, _| hub.body_scroll());
            scroll.set_offset(point(px(0.), px(0.)));
            crate::views::tests::draw(cx);
            let button = cx.debug_bounds("hub-open-folder").expect("the button");
            let body = cx.debug_bounds("project-hub-body").expect("the body");
            let past = button.bottom() - body.bottom();
            if past > Pixels::ZERO {
                scroll.set_offset(point(px(0.), -past));
                crate::views::tests::draw(cx);
            }
            let button = cx.debug_bounds("hub-open-folder").expect("the button");
            cx.simulate_click(button.center(), gpui::Modifiers::default());
            cx.run_until_parked();
            crate::views::tests::draw(cx);
            assert_eq!(
                hub.read_with(cx, |hub, _| hub.shown_error().map(str::to_owned))
                    .as_deref(),
                Some(TAB_LIMIT_REFUSAL),
                "{window:?} at {interface} pt: refused"
            );
            let button = cx.debug_bounds("hub-open-folder").expect("the button");
            let line = cx.debug_bounds("project-hub-error").expect("the refusal");
            let body = cx.debug_bounds("project-hub-body").expect("the body");
            assert!(
                line.top() >= button.bottom() && line.top() - button.bottom() <= px(24.),
                "{window:?} at {interface} pt: directly under the button: {button:?}, {line:?}"
            );
            assert!(
                line.top() >= body.top() && line.bottom() <= body.bottom() + px(0.5),
                "{window:?} at {interface} pt: inside the visible body: {line:?} in {body:?}"
            );
        }
    }

    /// A ninth tab refused from a recent row leaves that row in view: where
    /// the row and the feedback line can both be seen the page moves only so
    /// far as both show, and where they cannot it keeps its place, the row
    /// where it was, and the refusal is announced politely. Revealing the
    /// line by itself scrolled the pressed row out of view.
    #[gpui::test]
    async fn a_refusal_from_a_recent_row_leaves_that_row_in_view(cx: &mut TestAppContext) {
        use crate::copy_feedback::tests::announced;
        use crate::focus_ring::sweep;
        use crate::native_accessibility::written;
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        app.update(cx, |app, _| hold_tabs(app, &root, MAX_TABS));
        let recents: Vec<_> = (0..3)
            .map(|index| {
                crate::views::tests::fixture_repository(
                    fixture.path().join(format!("recent-{index}")),
                )
            })
            .collect();
        let paths: Vec<_> = recents.iter().map(|repo| repo.path().to_owned()).collect();
        let paths_kept = paths.clone();
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        cx.update(|window, cx| {
            hub.update(cx, |hub, cx| hub.set_recent(paths, cx));
            app.update(cx, |app, cx| app.show_projects(window, cx));
        });
        let scroll = hub.read_with(cx, |hub, _| hub.body_scroll());
        let body =
            |cx: &mut VisualTestContext| cx.debug_bounds("project-hub-body").expect("the body");
        // The row brought to the body's foot, where the line above it lies
        // furthest from it.
        let at_foot = |cx: &mut VisualTestContext, row: &'static str| {
            scroll.set_offset(point(px(0.), px(0.)));
            crate::views::tests::draw(cx);
            let past = cx.debug_bounds(row).expect("the row").bottom() - body(cx).bottom();
            if past > Pixels::ZERO {
                scroll.set_offset(point(px(0.), -past));
                crate::views::tests::draw(cx);
            }
        };
        let press = |cx: &mut VisualTestContext, row: &'static str| {
            let bounds = cx.debug_bounds(row).expect("the row");
            cx.simulate_click(bounds.center(), gpui::Modifiers::default());
            written::forget();
            cx.run_until_parked();
            crate::views::tests::draw(cx);
            bounds
        };
        // Too far from the line for both to be seen: the page keeps its place.
        sweep::prepare(&app, cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        sweep::interface_size(cx, 18);
        hub.update(cx, |hub, cx| hub.set_error(None, cx));
        at_foot(cx, "recent-project-1");
        let before = press(cx, "recent-project-1");
        assert_eq!(
            hub.read_with(cx, |hub, _| hub.shown_error().map(str::to_owned))
                .as_deref(),
            Some(TAB_LIMIT_REFUSAL),
            "refused"
        );
        let row = cx.debug_bounds("recent-project-1").expect("the row");
        let view = body(cx);
        assert!(
            (row.top() - before.top()).abs() <= px(0.5),
            "the row keeps its place: {row:?}, was {before:?}"
        );
        assert!(
            row.top() >= view.top() && row.bottom() <= view.bottom() + px(0.5),
            "and stays in view: {row:?} in {view:?}"
        );
        let line = cx.debug_bounds("project-hub-error").expect("the refusal");
        assert!(
            line.bottom() <= view.top() || line.top() >= view.bottom(),
            "the line stays out of view: {line:?} in {view:?}"
        );
        let (_, words) = announced(cx).expect("announced politely");
        assert!(words.contains("Switch to an open tab"), "{words}");
        assert!(
            written::announced_from("project-hub-error").is_empty(),
            "and not as an alert"
        );
        // The row says why where the person acted, in the removed colour and
        // its own bounds, the whole text in its tooltip.
        let (location, tooltip, name) = crate::projects::location_drawn(1).expect("the row");
        assert_eq!(location, TAB_LIMIT_REFUSAL, "the row reads the refusal");
        let place = paths_kept[1].display().to_string();
        assert_eq!(
            tooltip,
            format!("{TAB_LIMIT_REFUSAL}\n{place}"),
            "its tooltip holds it whole, and the location it replaces"
        );
        assert!(
            name.contains(TAB_LIMIT_REFUSAL) && name.contains(&place),
            "its name carries the refusal: {name}"
        );
        let removed = cx.read(|cx| crate::appearance::palette(cx).removed);
        assert_eq!(
            crate::appearance::drawn_color("recent-project-location-1"),
            Some(rgb(removed).into()),
            "in the removed colour"
        );
        assert_eq!(
            cx.debug_bounds("recent-project-1").expect("the row").size,
            before.size,
            "in its own bounds"
        );
        // The line brought into view, the row reads its place again.
        let offset = scroll.offset().y + (view.top() - line.top());
        scroll.set_offset(point(px(0.), offset.min(px(0.))));
        crate::views::tests::draw(cx);
        let (location, tooltip, name) = crate::projects::location_drawn(1).expect("the row");
        assert_ne!(location, TAB_LIMIT_REFUSAL, "the line in view, its place");
        assert_eq!(tooltip, place, "and its tooltip");
        assert!(!name.contains(TAB_LIMIT_REFUSAL), "and its name: {name}");
        // Close enough for both: the page moves only so far as both show,
        // and the line is the alert it always is.
        cx.simulate_resize(size(px(461.), px(860.)));
        hub.update(cx, |hub, cx| hub.set_error(None, cx));
        at_foot(cx, "recent-project-0");
        press(cx, "recent-project-0");
        let row = cx.debug_bounds("recent-project-0").expect("the row");
        let line = cx.debug_bounds("project-hub-error").expect("the refusal");
        let view = body(cx);
        for (name, part) in [("row", row), ("line", line)] {
            assert!(
                part.top() >= view.top() && part.bottom() <= view.bottom() + px(0.5),
                "the {name} shows: {part:?} in {view:?}"
            );
        }
        let alerts = written::announced_from("project-hub-error");
        assert!(
            alerts.len() == 1 && alerts[0].1 == written::Announced::ASSERTIVE,
            "the line it shows is the alert: {alerts:?}"
        );
    }

    /// The Projects page at the tab limit in a `window` at `interface` pt,
    /// with a recent project for each of `names`, the row `row` brought to
    /// the body's foot and pressed, which the limit refuses.
    async fn refused_from_a_row<'a>(
        cx: &'a mut TestAppContext,
        names: &[&str],
        window: Size<Pixels>,
        interface: u8,
        row: &'static str,
    ) -> (
        tempfile::TempDir,
        Vec<PathBuf>,
        Entity<crate::projects::ProjectHub>,
        &'a mut VisualTestContext,
    ) {
        use crate::focus_ring::sweep;
        use crate::views::tests::draw;
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        app.update(cx, |app, _| hold_tabs(app, &root, MAX_TABS));
        let paths: Vec<_> = names
            .iter()
            .map(|name| {
                crate::views::tests::fixture_repository(fixture.path().join(name))
                    .path()
                    .to_owned()
            })
            .collect();
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        let recents = paths.clone();
        cx.update(|window, cx| {
            hub.update(cx, |hub, cx| hub.set_recent(recents, cx));
            app.update(cx, |app, cx| app.show_projects(window, cx));
        });
        sweep::prepare(&app, cx);
        cx.simulate_resize(window);
        sweep::interface_size(cx, interface);
        hub.update(cx, |hub, cx| hub.set_error(None, cx));
        let scroll = hub.read_with(cx, |hub, _| hub.body_scroll());
        scroll.set_offset(point(px(0.), px(0.)));
        draw(cx);
        let body = cx.debug_bounds("project-hub-body").expect("the body");
        let past = cx.debug_bounds(row).expect("the row").bottom() - body.bottom();
        if past > Pixels::ZERO {
            scroll.set_offset(point(px(0.), -past));
            draw(cx);
        }
        let pressed = cx.debug_bounds(row).expect("the row");
        cx.simulate_click(pressed.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        assert_eq!(
            hub.read_with(cx, |hub, _| hub.shown_error().map(str::to_owned))
                .as_deref(),
            Some(TAB_LIMIT_REFUSAL),
            "refused"
        );
        (fixture, paths, hub, cx)
    }

    /// A refusal a recent row holds stays with its project: a search that
    /// keeps the project lets the refusal follow it to its new row, one that
    /// hides it shows the refusal on no row, and a new recent list follows
    /// the same rule. Held by list position, typing in the search put the
    /// refusal on whichever project came to stand there.
    #[gpui::test]
    async fn a_row_s_refusal_follows_its_project_through_a_search(cx: &mut TestAppContext) {
        let (_fixture, paths, hub, cx) = refused_from_a_row(
            cx,
            &["other-first", "pressed-row", "other-second"],
            size(px(461.), px(490.)),
            18,
            "recent-project-1",
        )
        .await;
        let refused = |index: usize| {
            crate::projects::location_drawn(index).expect("the row").0 == TAB_LIMIT_REFUSAL
        };
        assert!(refused(1), "the pressed row reads the refusal");
        let search = |cx: &mut VisualTestContext, query: &str| {
            cx.update(|window, cx| hub.update(cx, |hub, cx| hub.search_recent(query, window, cx)));
            crate::views::tests::draw(cx);
        };
        search(cx, "pressed-row");
        assert!(refused(0), "it follows its project to the first row");
        search(cx, "other-");
        assert!(
            !refused(0) && !refused(1),
            "hidden, no other project takes it"
        );
        search(cx, "");
        assert!(refused(1), "shown again, its row reads it again");
        let moved = vec![paths[1].clone(), paths[0].clone(), paths[2].clone()];
        cx.update(|_, cx| hub.update(cx, |hub, cx| hub.set_recent(moved, cx)));
        crate::views::tests::draw(cx);
        assert!(
            refused(0) && !refused(1),
            "a new recent list keeps it on the project"
        );
    }

    /// The refusal a recent row reads clears the small-label margin on the
    /// row's own fills, at rest, hovered and pressed, in its removed hue:
    /// Nord's token read 4.38:1 on the row's hover fill.
    #[gpui::test]
    async fn a_row_s_refusal_reads_on_the_row_s_own_fills(cx: &mut TestAppContext) {
        use crate::appearance::{ThemeChoice, custom::contrast};
        let (_fixture, _paths, _hub, cx) = refused_from_a_row(
            cx,
            &["one", "two", "three"],
            size(px(461.), px(490.)),
            18,
            "recent-project-1",
        )
        .await;
        cx.update(|window, cx| ThemeChoice::Nord.apply(Some(window), cx));
        crate::views::tests::draw(cx);
        let palette = cx.read(crate::appearance::palette);
        let fills = palette.control_fills(false, palette.canvas);
        let Some(drawn) = crate::appearance::drawn_color("recent-project-location-1") else {
            panic!("the row reads the refusal");
        };
        let drawn = u32::from(gpui_kit::Rgba::from(drawn)) >> 8;
        for fill in fills {
            assert!(
                contrast(drawn, fill) >= 4.75,
                "{drawn:06x} on {fill:06x}: {:.2}:1",
                contrast(drawn, fill)
            );
        }
        assert_eq!(
            drawn,
            palette.small_label(palette.removed, &fills),
            "the removed hue, moved only as far as it must"
        );
    }

    /// One resize that brings the feedback line into view ends the row's
    /// refusal in the next frame, with no hover or input: the row reads the
    /// line's place from the last frame, and in the wide layout it draws
    /// before the line, so nothing asked for the frame that corrects it.
    #[gpui::test]
    async fn one_resize_that_shows_the_line_ends_the_row_s_refusal(cx: &mut TestAppContext) {
        let (_fixture, _paths, hub, cx) = refused_from_a_row(
            cx,
            &["one", "two", "three"],
            size(px(461.), px(490.)),
            13,
            "recent-project-1",
        )
        .await;
        let scroll = hub.read_with(cx, |hub, _| hub.body_scroll());
        // The line a little above the view: the wide layout's roomier padding
        // brings it in.
        let line = cx.debug_bounds("project-hub-error").expect("the refusal");
        let view = cx.debug_bounds("project-hub-body").expect("the body");
        scroll.set_offset(point(
            px(0.),
            (scroll.offset().y + (view.top() - line.top()) - px(6.)).min(px(0.)),
        ));
        crate::views::tests::draw(cx);
        let line = cx.debug_bounds("project-hub-error").expect("the refusal");
        let view = cx.debug_bounds("project-hub-body").expect("the body");
        assert!(
            line.top() < view.top(),
            "above the view: {line:?} in {view:?}"
        );
        assert_eq!(
            crate::projects::location_drawn(1).expect("the row").0,
            TAB_LIMIT_REFUSAL,
            "out of view, the row reads it"
        );
        cx.simulate_resize(size(px(1000.), px(490.)));
        cx.run_until_parked();
        let line = cx.debug_bounds("project-hub-error").expect("the refusal");
        let view = cx.debug_bounds("project-hub-body").expect("the body");
        assert!(
            line.top() >= view.top() && line.bottom() <= view.bottom() + px(0.5),
            "the resize brings it into view: {line:?} in {view:?}"
        );
        assert_ne!(
            crate::projects::location_drawn(1).expect("the row").0,
            TAB_LIMIT_REFUSAL,
            "the row reads its place in the frame after the resize"
        );
    }

    /// A recent row's press that opened its tab is over: a later refusal at
    /// Choose a repository… answers Choose, revealed as an alert under that
    /// button. Taken for the old press's answer, the line stayed below the
    /// view with a polite announcement.
    #[gpui::test]
    async fn a_refusal_at_choose_after_a_row_opened_is_choose_s_own(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep;
        use crate::native_accessibility::written;
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        app.update(cx, |app, _| hold_tabs(app, &root, MAX_TABS - 1));
        let recents: Vec<_> = (0..2)
            .map(|index| {
                let repo = crate::views::tests::fixture_repository(
                    fixture.path().join(format!("recent-{index}")),
                );
                crate::tags::tests::git(repo.path(), &["commit", "--allow-empty", "-qm", "first"]);
                repo
            })
            .collect();
        let paths: Vec<_> = recents.iter().map(|repo| repo.path().to_owned()).collect();
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        cx.update(|window, cx| {
            hub.update(cx, |hub, cx| hub.set_recent(paths, cx));
            app.update(cx, |app, cx| app.show_projects(window, cx));
        });
        sweep::prepare(&app, cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        sweep::interface_size(cx, 18);
        crate::views::tests::draw(cx);
        let scroll = hub.read_with(cx, |hub, _| hub.body_scroll());
        let body =
            |cx: &mut VisualTestContext| cx.debug_bounds("project-hub-body").expect("the body");
        let to_foot = |cx: &mut VisualTestContext, selector: &'static str| {
            scroll.set_offset(point(px(0.), px(0.)));
            crate::views::tests::draw(cx);
            let past = cx.debug_bounds(selector).expect("drawn").bottom() - body(cx).bottom();
            if past > Pixels::ZERO {
                scroll.set_offset(point(px(0.), -past));
                crate::views::tests::draw(cx);
            }
        };
        // The eighth tab, opened from the second row.
        to_foot(cx, "recent-project-1");
        let row = cx.debug_bounds("recent-project-1").expect("the row");
        cx.simulate_click(row.center(), gpui::Modifiers::default());
        crate::views::tests::settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.tabs.len(), MAX_TABS, "the row opened");
            assert_eq!(app.page, AppPage::Repository, "and left the page");
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
        crate::views::tests::draw(cx);
        to_foot(cx, "hub-open-folder");
        let button = cx.debug_bounds("hub-open-folder").expect("the button");
        cx.simulate_click(button.center(), gpui::Modifiers::default());
        written::forget();
        cx.run_until_parked();
        crate::views::tests::draw(cx);
        assert_eq!(
            hub.read_with(cx, |hub, _| hub.shown_error().map(str::to_owned))
                .as_deref(),
            Some(TAB_LIMIT_REFUSAL),
            "refused at Choose"
        );
        let line = cx.debug_bounds("project-hub-error").expect("the refusal");
        let view = body(cx);
        assert!(
            line.top() >= view.top() && line.bottom() <= view.bottom() + px(0.5),
            "the line under Choose is in view: {line:?} in {view:?}"
        );
        let alerts = written::announced_from("project-hub-error");
        assert!(
            alerts.len() == 1 && alerts[0].1 == written::Announced::ASSERTIVE,
            "and is the alert it always is: {alerts:?}"
        );
    }

    /// Closing a tab ends the limit's refusal on the page as well as in the
    /// strip: its cause is gone with the tab, and the next Choose a
    /// repository… asks for a folder. Another refusal is left alone.
    #[gpui::test]
    async fn closing_a_tab_ends_the_limits_refusal_on_the_page(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let root = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        app.update(cx, |app, _| hold_tabs(app, &root, MAX_TABS));
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        let close = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    let index = app.repository_tabs.tabs.len() - 1;
                    app.close_repository_tab(index, window, cx);
                })
            });
            crate::views::tests::draw(cx);
        };
        hub.update(cx, |hub, cx| {
            hub.set_error(Some("Another error".into()), cx)
        });
        close(cx);
        assert_eq!(
            hub.read_with(cx, |hub, _| hub.shown_error().map(str::to_owned))
                .as_deref(),
            Some("Another error"),
            "another refusal stays"
        );
        hub.update(cx, |hub, cx| {
            hub.set_error(Some(TAB_LIMIT_REFUSAL.to_owned()), cx)
        });
        close(cx);
        assert_eq!(
            hub.read_with(cx, |hub, _| hub.shown_error().map(str::to_owned)),
            None,
            "the limit's refusal ends with the tab that caused it"
        );
    }

    /// A ninth tab asked for from the Projects page is refused there,
    /// beside the project the person chose, in the tab strip's words and
    /// announced at once as the page's other feedback is, and the page's
    /// actions are available again. Refused in the window's error alone,
    /// which the page does not show, the hub stayed busy, every action
    /// refused under "Opening project…", with nothing to say why.
    #[gpui::test]
    async fn a_ninth_tab_refused_from_projects_says_so_on_the_page(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let other = crate::views::tests::fixture_repository(fixture.path().join("other"));
        app.update(cx, |app, _| {
            for held in 0..MAX_TABS - 1 {
                app.repository_tabs.tabs.push(Tab {
                    path: fixture.path().join(format!("held-{held}")),
                    resolved: true,
                    git_dir: None,
                    saved: Bookmark::default(),
                    warm: None,
                    error: None,
                });
            }
        });
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        cx.update(|window, cx| {
            hub.update(cx, |hub, cx| {
                hub.set_recent(vec![other.path().to_owned()], cx)
            });
            app.update(cx, |app, cx| {
                app.show_projects(window, cx);
                // An earlier refusal of the strip's, which the page's own
                // replaces: one refusal line at a time.
                app.refuse_in_tab_strip("An earlier refusal.", cx);
            });
        });
        crate::views::tests::draw(cx);
        let mut nodes = Vec::new();
        for refusal in ["first", "second"] {
            let project = cx
                .debug_bounds("recent-project-0")
                .expect("the recent project");
            cx.simulate_click(project.center(), gpui::Modifiers::default());
            written::forget();
            crate::views::tests::settle(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert_eq!(
                    app.repository_tabs.tabs.len(),
                    MAX_TABS,
                    "{refusal}: refused"
                );
                assert_eq!(app.page, AppPage::Projects);
                assert_eq!(app.error, None, "{refusal}: not in the window's error");
            });
            hub.read_with(cx, |hub, _| {
                assert_eq!(
                    hub.shown_error(),
                    Some(TAB_LIMIT_REFUSAL),
                    "{refusal}: the page says why"
                );
                assert!(
                    !hub.shown_busy(),
                    "{refusal}: and its actions are available again"
                );
            });
            let announced = written::announced_from("project-hub-error");
            assert!(
                announced.len() == 1 && announced[0].1 == written::Announced::ASSERTIVE,
                "{refusal}: announced at once: {announced:?}"
            );
            nodes.push(announced[0].0.clone());
            assert!(
                cx.debug_bounds("repository-tab-refusal").is_none(),
                "{refusal}: said once, where the person acted"
            );
        }
        assert_ne!(
            nodes[0], nodes[1],
            "the same refusal again is a fresh node, announced again"
        );
    }

    #[gpui::test]
    async fn rename_tab_waits_for_discovery_and_rejects_stale_or_unavailable_targets(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repository = crate::views::tests::fixture_repository(fixture.path().join("repository"));
        let canonical = repository.path().to_owned();
        let nested = canonical.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let unavailable = fixture.path().join("missing");
        let destination = isolated_session("isolated-session.json");
        let mut preferences = Preferences::default();
        preferences
            .project_names
            .insert(canonical.clone(), "Turtle 客户端".into());
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(|window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    preferences,
                    Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(nested.clone(), window, cx);
                assert!(!app.can_rename_repository_tab(&nested));
                assert!(!app.can_rename_repository_tab(&canonical));
            });
        });
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert_eq!(app.repository_tabs.active_path(), Some(canonical.as_path()));
                assert!(app.can_rename_repository_tab(&canonical));
                assert_eq!(app.project_name(&canonical), "Turtle 客户端");
                assert!(!app.can_rename_repository_tab(&nested));
                app.nav_mode = NavMode::Worktrees;
                for (query, expected) in [
                    ("turtle 客户端", true),
                    ("repository", true),
                    ("main", true),
                    ("not this project", false),
                    ("", true),
                ] {
                    app.nav_search
                        .update(cx, |search, cx| search.set_value(query, window, cx));
                    app.rebuild_navigation(cx);
                    assert_eq!(
                        app.nav_rows
                            .iter()
                            .any(|row| matches!(row, NavRow::Worktree(_))),
                        expected,
                        "worktree search must match custom names, paths and branches: {query}"
                    );
                }
                // An accepted Git operation does not block app-only naming.
                app.operation_busy = Some("Fetching");
                assert!(app.can_rename_repository_tab(&canonical));
                app.operation_busy = None;
                app.open_repository_tab(unavailable.clone(), window, cx);
                assert!(!app.can_rename_repository_tab(&canonical));
                assert!(!app.can_rename_repository_tab(&unavailable));
            });
        });
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert!(!app.can_rename_repository_tab(&unavailable));
                app.switch_repository_tab(0, window, cx);
                assert!(app.can_rename_repository_tab(&canonical));
                assert!(!app.can_rename_repository_tab(&unavailable));
            });
        });
        settle_tab_test(&app, cx).await;
    }

    #[gpui::test]
    fn fractional_list_scaling_preserves_cold_and_restoring_bookmarks(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            let bookmark = Bookmark {
                history_y: -3400.,
                file_y: -4400.,
                ..Default::default()
            };
            let mut state = State {
                tabs: vec![Tab {
                    path: PathBuf::from("fixture"),
                    resolved: true,
                    git_dir: None,
                    saved: bookmark.clone(),
                    warm: None,
                    error: None,
                }],
                restoring: Some(bookmark.clone()),
                document_restore: Some(bookmark),
                ..Default::default()
            };
            let mut previous = 1.;
            for scale in [1.25, 1.15, 1.] {
                let scales = settings::ListScales::new(
                    previous,
                    scale,
                    appearance::Density::Comfortable,
                    window,
                );
                state.rescale_text_viewports(scales, 1., cx);
                for saved in [
                    &state.tabs[0].saved,
                    state.restoring.as_ref().unwrap(),
                    state.document_restore.as_ref().unwrap(),
                ] {
                    let history = -100. * f32::from(window.pixel_snap(px(34. * scale)));
                    let files = -100. * f32::from(window.pixel_snap(px(44. * scale)));
                    assert!((saved.history_y - history).abs() < 0.01);
                    assert!((saved.file_y - files).abs() < 0.01);
                }
                previous = scale;
            }
        });
    }

    #[gpui::test]
    async fn desktop_scale_updates_each_window_and_retained_tab_once(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        crate::views::tests::init_app(cx);
        let mut windows = Vec::new();
        let mut apps = Vec::new();
        let mut editors = Vec::new();
        for index in 0..2 {
            let repository = crate::views::tests::fixture_repository(
                fixture.path().join(format!("repo-{index}")),
            );
            let saved = isolated_session(&format!("session-{index}.json"));
            let mut captured = None;
            let window = cx.add_window(|window, cx| {
                let app = cx.new(|cx| {
                    GitTurtle::new(
                        None,
                        Preferences::default(),
                        Session::default(),
                        activity::State::default(),
                        recovery_drafts::State::default(),
                        window,
                        cx,
                    )
                });
                captured = Some(app.clone());
                gpui_kit::component::Root::new(app, window, cx)
            });
            let app = captured.unwrap();
            let pair = cx.update(|cx| {
                window
                    .update(cx, |_, window, cx| {
                        app.update(cx, |app, cx| {
                            app.repository_tabs.save_path = Some(saved);
                            app.repository = Some(repository);
                            let retained =
                                text::editor("retained source\n", "text", None, window, cx);
                            retained.update(cx, |editor, cx| {
                                editor.set_selected_range(1..5, cx);
                                editor.set_scroll_offset(point(px(-40.), px(-400.)), cx);
                            });
                            app.before_editor = Some(retained.clone());
                            app.history_scroll
                                .0
                                .borrow()
                                .base_handle
                                .set_offset(point(px(-10.), px(-200.)));
                            app.history_list_layout = Some((size(px(600.), px(400.)), px(34.)));
                            assert!(app.retain_active_tab(window, cx));
                            let active = text::editor("active source\n", "text", None, window, cx);
                            active.update(cx, |editor, cx| {
                                editor.set_selected_range(2..6, cx);
                                editor.set_scroll_offset(point(px(-20.), px(-300.)), cx);
                            });
                            app.before_editor = Some(active.clone());
                            app.history_scroll
                                .0
                                .borrow()
                                .base_handle
                                .set_offset(point(px(-15.), px(-100.)));
                            (active, retained)
                        })
                    })
                    .unwrap()
            });
            windows.push(window);
            apps.push(app);
            editors.push(pair);
        }
        cx.executor().run_until_parked();
        // Publish the same per-App notification used by the Linux bridge,
        // without changing process-wide font globals shared by parallel tests.
        cx.update(|cx| {
            cx.set_global(appearance::DesktopTextScale(1.25));
            cx.set_global(appearance::DesktopTextScale(1.5));
        });
        cx.update(|cx| cx.set_global(appearance::DesktopTextScale(1.5)));
        for (factor, scale) in [(1.5, 1.5), (1., 1.)] {
            cx.update(|cx| cx.set_global(appearance::DesktopTextScale(scale)));
            cx.update(|cx| {
                for (app, (active, retained)) in apps.iter().zip(&editors) {
                    let app = app.read(cx);
                    assert_eq!(app.before_editor.as_ref(), Some(active));
                    assert_eq!(
                        active.read(cx).scroll_offset(),
                        point(px(-20. * factor), px(-300. * factor))
                    );
                    assert_eq!(active.read(cx).selected_range(), 2..6);
                    assert_eq!(
                        retained.read(cx).scroll_offset(),
                        point(px(-40. * factor), px(-400. * factor))
                    );
                    assert_eq!(retained.read(cx).selected_range(), 1..5);
                    assert_eq!(
                        app.history_scroll.0.borrow().base_handle.offset(),
                        point(px(-15.), px(-100. * factor))
                    );
                    let tab = &app.repository_tabs.tabs[0];
                    let warm = tab.warm.as_ref().unwrap();
                    assert_eq!(
                        warm.history_scroll.0.borrow().base_handle.offset(),
                        point(px(-10.), px(-200. * factor))
                    );
                    assert_eq!(tab.saved.history_y, -200. * factor);
                    assert!(warm.history_list_layout.is_none());
                    assert_eq!(
                        app.settings.interface_text_size,
                        appearance::DEFAULT_INTERFACE_TEXT_SIZE
                    );
                    assert_eq!(
                        app.settings.code_text_size,
                        appearance::DEFAULT_CODE_TEXT_SIZE
                    );
                }
            });
        }
        for (window, app) in windows.into_iter().zip(apps) {
            cx.update(|cx| {
                window
                    .update(cx, |_, window, _| window.remove_window())
                    .unwrap()
            });
            let (saved, preferences) = cx.update(|cx| {
                (
                    app.read(cx)
                        .repository_tabs
                        .save_completion
                        .clone()
                        .unwrap(),
                    app.read(cx).preferences_writer.submit_read(|| Ok(())),
                )
            });
            saved.await.unwrap();
            preferences.await.unwrap().unwrap();
        }
        cx.executor().run_until_parked();
    }

    /// Three frames, each followed by the work it schedules.
    fn draw_tab_frames(cx: &mut VisualTestContext) {
        for _ in 0..3 {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            cx.executor().run_until_parked();
        }
    }

    /// `commits` linear commits on `main` without a checkout: History lists
    /// the oldest last, at index `commits - 1`.
    fn linear_history_fixture(path: PathBuf, commits: u32) -> GitRepository {
        use std::{
            io::Write,
            process::{Command, Stdio},
        };

        let repo = crate::views::tests::fixture_repository(path);
        let mut input = String::new();
        for index in 0..commits {
            let message = format!("history commit {index}");
            input.push_str(&format!(
                "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{}\n",
                1_700_000_000 + index,
                message.len(),
                message
            ));
        }
        input.push_str("done\n");
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(repo.path())
            .args(["fast-import", "--quiet"])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(name);
        }
        let mut import = command.spawn().unwrap();
        import
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let imported = import.wait_with_output().unwrap();
        assert!(
            imported.status.success(),
            "{}",
            String::from_utf8_lossy(&imported.stderr)
        );
        repo
    }

    /// Where a test window saves its tab session, `name` in a directory of the
    /// test process's root that goes with the root at exit. A window's queued
    /// save can run after its test, and a save makes its folder again, so a
    /// session in the test's fixture came back outside it once it was gone.
    fn isolated_session(name: &str) -> PathBuf {
        preferences::test_directories::until_exit("tab-session").join(name)
    }

    /// A window that is `window_size` from its first frame, opening `initial`
    /// with `saved` as its repository session and saving to `save_path`.
    /// `add_window_view` opens a maximized window and runs the deferred open
    /// before it returns: on a loaded host a cold tab's reads can finish
    /// there, and a resize afterwards is a real viewport change over the
    /// restored layout.
    fn open_tab_window(
        cx: &mut TestAppContext,
        initial: PathBuf,
        saved: Session,
        save_path: PathBuf,
        window_size: Size<Pixels>,
    ) -> (Entity<GitTurtle>, VisualTestContext) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let window = cx
            .update(|cx| {
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(0.), px(0.)),
                            window_size,
                        ))),
                        ..Default::default()
                    },
                    move |window, cx| {
                        let app = cx.new(|cx| {
                            let mut app = GitTurtle::new(
                                Some(initial),
                                Preferences::default(),
                                saved,
                                activity::State::default(),
                                recovery_drafts::State::default(),
                                window,
                                cx,
                            );
                            app.repository_tabs.save_path = Some(save_path);
                            app
                        });
                        *captured.borrow_mut() = Some(app.clone());
                        cx.new(|cx| Root::new(app, window, cx))
                    },
                )
            })
            .unwrap();
        let app = observed.borrow_mut().take().unwrap();
        (app, VisualTestContext::from_window(*window, cx))
    }

    #[gpui::test]
    async fn cold_tab_restores_rendered_history_offset_without_revealing_manual_selection(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repo = linear_history_fixture(fixture.path().join("history"), 500);
        let destination = isolated_session("isolated-session.json");
        crate::views::tests::init_app(cx);
        cx.update(|cx| {
            cx.bind_keys([KeyBinding::new("end", LastRow, Some("GitTurtleList"))]);
        });
        let mut expected_y = None;
        let mut expected_oid = None;
        // Save End at a resized viewport, restore it cold, then restore an
        // intentional manual scroll away from that same selected commit.
        for pass in 0..3 {
            let saved = if pass == 0 {
                Session::default()
            } else {
                Session::load_at(&destination).unwrap()
            };
            if pass > 0 {
                assert_eq!(Some(px(saved.tabs[0].bookmark.history_y)), expected_y);
            }
            // The cold tab restores into a window already at its own size.
            let (app, mut window_cx) = open_tab_window(
                cx,
                repo.path().to_owned(),
                saved,
                destination.clone(),
                size(px(1480.), px(981.)),
            );
            let window_cx = &mut window_cx;
            settle_tab_test(&app, window_cx).await;
            draw_tab_frames(window_cx);
            if pass == 0 {
                window_cx.simulate_resize(size(px(1000.), px(680.)));
                draw_tab_frames(window_cx);
                window_cx.update(|window, cx| {
                    let focus = app.read(cx).focus.clone();
                    window.focus(&focus, cx);
                });
                window_cx.simulate_keystrokes("end");
                settle_tab_test(&app, window_cx).await;
                window_cx.simulate_resize(size(px(1480.), px(981.)));
                draw_tab_frames(window_cx);
            }
            window_cx.read(|cx| {
                let app = app.read(cx);
                assert_eq!(app.visible.len(), 500);
                assert_eq!(app.selected_commit, Some(499));
                let oid = &app.commits[499].oid;
                if let Some(expected) = &expected_oid {
                    assert_eq!(oid, expected);
                }
                let scroll = app.history_scroll.0.borrow();
                let offset = scroll.base_handle.offset().y;
                if let Some(expected) = expected_y {
                    assert_eq!(
                        offset, expected,
                        "cold first layout must retain the saved viewport"
                    );
                }
                let measured = scroll
                    .last_item_size
                    .as_ref()
                    .expect("rendered list geometry");
                let row_height = measured.contents.height / app.visible.len() as f32;
                let selected_bottom = offset + row_height * app.visible.len();
                if pass < 2 {
                    assert!(selected_bottom <= measured.item.height + px(1.));
                    assert!(selected_bottom - row_height >= px(0.));
                } else {
                    assert!(
                        selected_bottom > measured.item.height,
                        "manual scroll stays away from selection"
                    );
                }
            });
            if pass == 1 {
                let manual_y = window_cx.update(|window, cx| {
                    let (position, delta) = app.update(cx, |app, cx| {
                        // A real file read is still pending when the wheel
                        // event arrives; its reply must not reset the viewport.
                        app.repository_tabs.restoring = Some(app.tab_bookmark(cx));
                        app.select_commit(499, window, cx);
                        let scroll = &app.history_scroll.0.borrow().base_handle;
                        (scroll.bounds().center(), -scroll.offset().y / 2.)
                    });
                    window.dispatch_event(
                        gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                            position,
                            delta: gpui::ScrollDelta::Pixels(point(px(0.), delta)),
                            modifiers: Modifiers::default(),
                            touch_phase: gpui::TouchPhase::Moved,
                        }),
                        cx,
                    );
                    let app = app.read(cx);
                    assert!(app.repository_tabs.restoring.is_some());
                    let moved = app.history_scroll.0.borrow().base_handle.offset().y;
                    assert_ne!(
                        Some(moved),
                        expected_y,
                        "the actual wheel event must scroll"
                    );
                    moved
                });
                settle_tab_test(&app, window_cx).await;
                draw_tab_frames(window_cx);
                window_cx.read(|cx| {
                    assert_eq!(
                        app.read(cx)
                            .history_scroll
                            .0
                            .borrow()
                            .base_handle
                            .offset()
                            .y,
                        manual_y,
                        "the completed file read must preserve the newer wheel scroll"
                    );
                });
            }
            let completion = window_cx.update(|_, cx| {
                app.update(cx, |app, cx| {
                    expected_y = Some(app.history_scroll.0.borrow().base_handle.offset().y);
                    expected_oid = Some(app.commits[499].oid.clone());
                    app.queue_session_snapshot(cx)
                })
            });
            completion.await.unwrap();
            window_cx.update(|window, _| window.remove_window());
            let closing = window_cx.cx.update(|cx| {
                app.read(cx)
                    .repository_tabs
                    .save_completion
                    .clone()
                    .unwrap()
            });
            closing.await.unwrap();
        }
    }

    /// A cold tab's changed-files read can land on the first frame after the
    /// window resizes, as it did on a loaded host. That frame still lays
    /// History's table out for the width its previous frame measured, so the
    /// frame after it widens the table without the window changing: the
    /// restored viewport, scrolled away from the selected commit, must stay.
    #[gpui::test]
    async fn restored_viewport_survives_its_last_read_landing_after_a_resize(
        cx: &mut TestAppContext,
    ) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repo = linear_history_fixture(fixture.path().join("history"), 500);
        crate::views::tests::init_app(cx);
        // A test window's maximized size, before the window manager tiles it.
        let (app, mut window_cx) = open_tab_window(
            cx,
            repo.path().to_owned(),
            Session::default(),
            isolated_session("isolated-session.json"),
            size(px(1920.), px(1080.)),
        );
        let window_cx = &mut window_cx;
        settle_tab_test(&app, window_cx).await;
        window_cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(499, window, cx)));
        settle_tab_test(&app, window_cx).await;
        window_cx.update(|_, cx| {
            app.read(cx)
                .history_scroll
                .0
                .borrow()
                .base_handle
                .set_offset(point(px(0.), px(-4000.)))
        });
        draw_tab_frames(window_cx);
        let saved = window_cx.update(|_, cx| app.read(cx).tab_bookmark(cx));
        assert_eq!(saved.history_y, -4000.);
        // Restore the bookmark as `finish_tab_snapshot` does once the cold
        // snapshot is in: the changed-files read is the only one pending
        // when the window takes its tiled size.
        window_cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.repository_tabs.restoring = Some(saved.clone());
                app.restore_tab_selection(window, cx);
            })
        });
        window_cx.simulate_resize(size(px(1480.), px(981.)));
        settle_tab_test(&app, window_cx).await;
        draw_tab_frames(window_cx);
        window_cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.repository_tabs.restoring.is_none(), "the read landed");
            assert_eq!(app.selected_commit, Some(499));
            let scroll = app.history_scroll.0.borrow();
            let offset = scroll.base_handle.offset().y;
            assert_eq!(
                offset,
                px(saved.history_y),
                "the restored viewport must not reveal the selection"
            );
            let measured = scroll
                .last_item_size
                .as_ref()
                .expect("rendered list geometry");
            let row_height = measured.contents.height / app.visible.len() as f32;
            assert!(
                offset + row_height * app.visible.len() > measured.item.height,
                "the selected commit stays below the restored viewport"
            );
        });
        window_cx.update(|window, _| window.remove_window());
        let closing = window_cx.cx.update(|cx| {
            app.read(cx)
                .repository_tabs
                .save_completion
                .clone()
                .unwrap()
        });
        closing.await.unwrap();
    }

    #[gpui::test]
    async fn warm_tabs_restore_measured_viewports_with_their_manual_scroll_positions(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let first = crate::views::tests::fixture_repository(fixture.path().join("first"));
        let second = crate::views::tests::fixture_repository(fixture.path().join("second"));
        let initial = first.path().to_owned();
        let next = second.path().to_owned();
        let destination = isolated_session("isolated-session.json");
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, window_cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(initial),
                    Preferences::default(),
                    Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        settle_tab_test(&app, window_cx).await;
        let first_history = Some((size(px(760.), px(440.)), px(34.)));
        let first_files = Some((size(px(320.), px(260.)), px(44.)));
        let second_history = Some((size(px(640.), px(380.)), px(34.)));
        let second_files = Some((size(px(440.), px(210.)), px(44.)));
        let history_offset = point(px(-30.), px(-680.));
        let files_offset = point(px(0.), px(-352.));
        window_cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                // These are the snapshots produced by each list's layout
                // probe. Exercise the real tab ownership transfer together
                // with its scroll handles before another frame can measure.
                app.history_list_layout = first_history;
                app.file_list_layout = first_files;
                app.history_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(history_offset);
                app.file_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(files_offset);
                app.open_repository_tab(next, window, cx);
                assert!(app.history_list_layout.is_none());
                assert!(app.file_list_layout.is_none());
            });
        });
        settle_tab_test(&app, window_cx).await;
        window_cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.history_list_layout = second_history;
                app.file_list_layout = second_files;
                app.switch_repository_tab(0, window, cx);
                assert_eq!(app.history_list_layout, first_history);
                assert_eq!(app.file_list_layout, first_files);
                assert_eq!(
                    app.history_scroll.0.borrow().base_handle.offset(),
                    history_offset
                );
                assert_eq!(
                    app.file_scroll.0.borrow().base_handle.offset(),
                    files_offset
                );
                app.close_repository_tab(0, window, cx);
                assert_eq!(app.history_list_layout, second_history);
                assert_eq!(app.file_list_layout, second_files);
                app.close_repository_tab(0, window, cx);
                assert!(app.history_list_layout.is_none());
                assert!(app.file_list_layout.is_none());
            });
        });
        settle_tab_test(&app, window_cx).await;
    }

    /// Compare's unified patch keeps its decorations with its editor. The active tab's figure
    /// counts them and the presentation they were built from, so a decorated patch reaches the
    /// retained-tabs bound that the same patch without them stays within.
    #[gpui::test]
    async fn compare_patch_decorations_count_toward_the_retained_tabs_bound(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let first = crate::views::tests::fixture_repository(fixture.path().join("first"));
        let second = crate::views::tests::fixture_repository(fixture.path().join("second"));
        let initial = first.path().to_owned();
        let next = second.path().to_owned();
        let destination = isolated_session("isolated-session.json");
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, window_cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(initial),
                    Preferences::default(),
                    Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        settle_tab_test(&app, window_cx).await;
        window_cx.update(|window, cx| {
            app.update(cx, |app, cx| app.open_repository_tab(next, window, cx))
        });
        settle_tab_test(&app, window_cx).await;

        let (decorated, without) = window_cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                assert_eq!(app.repository_tabs.active, Some(1));
                assert!(app.repository_tabs.tabs[0].warm.is_some());
                let content = Arc::new(text::decorated_patch_content());
                let Content::Text {
                    patch,
                    presentation,
                    ..
                } = content.as_ref()
                else {
                    unreachable!()
                };
                let (editor, decoration) = text::editor_with_decorations(
                    patch,
                    "diff",
                    Some(presentation.as_ref()),
                    window,
                    cx,
                );
                let presentation = presentation.retained_bytes();
                let decoration = decoration.expect("a patch editor keeps its decorations");
                let decorated = decoration.retained_bytes();
                assert!(decorated > 0, "a patch with changes has decorations");
                app.text_mode = TextMode::Unified;
                app.patch_editor = Some(editor);
                app.content = Some(content);
                app.patch_decoration = Some(decoration);

                let with = app.tab_retained_bytes();
                let decoration = app.patch_decoration.take();
                let without = app.tab_retained_bytes();
                assert_eq!(with - without, decorated);
                let content = app.content.take();
                assert!(without - app.tab_retained_bytes() >= presentation);
                app.content = content;
                app.patch_decoration = decoration;
                (decorated, without)
            })
        });

        // Fill the warm tab so the undecorated patch exactly reaches the bound.
        let at_bound = |app: &mut GitTurtle| {
            app.repository_tabs.tabs[0]
                .warm
                .as_mut()
                .expect("the first tab stays warm")
                .bytes = MAX_RETAINED_BYTES - without;
        };
        window_cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                at_bound(app);
                app.error = None;
                assert_eq!(app.tab_retained_bytes(), without + decorated);
                app.switch_repository_tab(0, window, cx);
                assert_eq!(
                    app.repository_tabs.active,
                    Some(1),
                    "the decorated patch is refused at the bound"
                );
                assert!(
                    app.repository_tabs
                        .refusal
                        .as_deref()
                        .is_some_and(|e| e.contains("512 MiB")),
                    "the tab strip says why"
                );
                assert_eq!(app.error, None, "not over the preview");
                assert!(
                    app.patch_decoration.is_some(),
                    "a refusal keeps the preview"
                );

                app.patch_decoration = None;
                app.error = None;
                app.switch_repository_tab(0, window, cx);
                assert_eq!(
                    app.repository_tabs.active,
                    Some(0),
                    "the same patch without decorations is retained"
                );
                assert!(app.error.is_none());
                assert_eq!(
                    app.repository_tabs.tabs[1]
                        .warm
                        .as_ref()
                        .map(|warm| warm.bytes),
                    Some(without)
                );
            })
        });
        settle_tab_test(&app, window_cx).await;
    }

    #[gpui::test]
    async fn closing_warm_tabs_keeps_open_repository_keyboard_shortcut_reachable(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let paths = (0..4)
            .map(|index| {
                crate::views::tests::fixture_repository(
                    fixture.path().join(format!("repository-{index}")),
                )
                .path()
                .to_owned()
            })
            .collect::<Vec<_>>();
        let initial = paths[0].clone();
        let destination = isolated_session("isolated-session.json");
        let open_key = if cfg!(target_os = "macos") {
            "cmd-o"
        } else {
            "ctrl-o"
        };
        crate::views::tests::init_app(cx);
        cx.update(|cx| {
            cx.bind_keys([KeyBinding::new(open_key, OpenRepository, Some("GitTurtle"))]);
        });
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, window_cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(initial),
                    Preferences::default(),
                    Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        settle_tab_test(&app, window_cx).await;
        for path in paths.iter().skip(1) {
            window_cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.open_repository_tab(path.clone(), window, cx)
                });
            });
            settle_tab_test(&app, window_cx).await;
        }
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..3 {
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
                cx.executor().run_until_parked();
            }
        }
        fn click(cx: &mut VisualTestContext, selector: &'static str) {
            draw(cx);
            let bounds = cx.debug_bounds(selector).expect("rendered tab control");
            cx.simulate_click(bounds.center(), Modifiers::default());
        }
        // The toolkit leaves mouse focus unchanged. Keyboard navigation can
        // focus the fourth tab before a mouse switch captures that handle in
        // the first workspace. The close sequence then removes its target.
        click(window_cx, "repository-tab-0");
        settle_tab_test(&app, window_cx).await;
        draw(window_cx);
        let retired_focus = window_cx.update(|window, cx| {
            window.blur(cx);
            for _ in 0..7 {
                window.focus_next(cx);
            }
            let focus = window.focused(cx).expect("keyboard-focused fourth tab");
            assert!(app.read(cx).app_focus.contains(&focus, window));
            focus
        });
        click(window_cx, "repository-tab-3");
        settle_tab_test(&app, window_cx).await;
        for selector in [
            "repository-tab-close-3",
            "repository-tab-close-2",
            "repository-tab-close-1",
        ] {
            click(window_cx, selector);
            settle_tab_test(&app, window_cx).await;
        }
        draw(window_cx);
        let attached = window_cx.update(|window, cx| {
            let app = app.read(cx);
            assert_eq!(app.repository_tabs.tabs.len(), 1);
            assert_eq!(app.path.as_deref(), Some(paths[0].as_path()));
            assert!(
                !app.app_focus.contains(&retired_focus, window),
                "the previously keyboard-focused tab has been removed"
            );
            window
                .focused(cx)
                .is_some_and(|focus| app.app_focus.contains(&focus, window))
        });
        assert!(!window_cx.did_prompt_for_paths());
        window_cx.simulate_keystrokes(open_key);
        assert!(
            window_cx.did_prompt_for_paths(),
            "Open shortcut must reach the native picker after repeated warm closes; attached focus={attached}"
        );
        assert!(
            attached,
            "the restored focus must belong to the visible workspace"
        );
        window_cx.simulate_path_prompt_response(|_| None);
        settle_tab_test(&app, window_cx).await;

        // A surviving editor target must still be restored exactly; fixing a
        // removed tab must not replace every warm return with generic focus.
        window_cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.search.read(cx).focus_handle(cx).focus(window, cx);
                app.open_repository_tab(paths[1].clone(), window, cx);
            });
        });
        settle_tab_test(&app, window_cx).await;
        click(window_cx, "repository-tab-close-1");
        settle_tab_test(&app, window_cx).await;
        draw(window_cx);
        window_cx.update(|window, cx| {
            assert!(
                app.read(cx)
                    .search
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window),
                "warm return must retain the surviving search input focus"
            );
        });
        settle_tab_test(&app, window_cx).await;
    }

    /// The tab list scrolls, so GPUI clips everything in it to the list's own
    /// bounds on both axes. The list keeps room inside that clip for the
    /// installed Button focus ring: each tab's and close button's ring, `gap +
    /// width` outside its edge, lies within the content mask the list paints
    /// its tabs in, at scale factors 2, 1.5 and 1: at 1.5 the 3 px room once
    /// laid out as 2⅔ px, as layout rounds a length to the nearest device
    /// pixel, and the clip cut each ring's outer device pixel. The list's
    /// margin gives the room back, so every tab sits where a list without the
    /// room puts it.
    #[gpui::test]
    async fn repository_tab_list_keeps_room_for_every_focus_ring(cx: &mut TestAppContext) {
        use gpui_kit::component::{FocusRing, Root, Theme};
        use std::{cell::RefCell, rc::Rc};

        /// Each tab with its close button.
        const TABS: [[&str; 2]; 2] = [
            ["repository-tab-0", "repository-tab-close-0"],
            ["repository-tab-1", "repository-tab-close-1"],
        ];
        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..3 {
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
                cx.executor().run_until_parked();
            }
        }
        fn rendered(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("rendered {selector}"))
        }
        /// Every tab control, then the header below the strip.
        fn layout(cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
            TABS.into_iter()
                .flatten()
                .chain(["repository-header"])
                .map(|selector| rendered(cx, selector))
                .collect()
        }
        /// The content mask the tab list paints its tabs in, read from the
        /// fill behind the selected tab and its close button: GPUI keeps a
        /// filled quad's mask whole, and nothing between the list and that
        /// fill clips.
        fn tab_list_mask(
            cx: &mut VisualTestContext,
            [tab, close]: [&'static str; 2],
        ) -> Bounds<Pixels> {
            let surface = rendered(cx, tab).union(&rendered(cx, close));
            cx.update(|window, _| {
                let scale = window.scale_factor();
                let logical = |bounds: Bounds<ScaledPixels>| {
                    let logical = |value: ScaledPixels| px(value.as_f32() / scale);
                    Bounds::from_corners(
                        point(logical(bounds.left()), logical(bounds.top())),
                        point(logical(bounds.right()), logical(bounds.bottom())),
                    )
                };
                let device = px(1. / scale);
                let masks = window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| {
                        let drawn = logical(quad.bounds);
                        !quad.background.is_transparent()
                            && [
                                (drawn.left(), surface.left()),
                                (drawn.top(), surface.top()),
                                (drawn.right(), surface.right()),
                                (drawn.bottom(), surface.bottom()),
                            ]
                            .into_iter()
                            .all(|(drawn, expected)| (drawn - expected).abs() < device)
                    })
                    .map(|quad| logical(quad.content_mask.bounds))
                    .collect::<Vec<_>>();
                match masks.as_slice() {
                    [mask, rest @ ..] if rest.iter().all(|other| other == mask) => *mask,
                    _ => panic!("one selected tab surface at {surface:?}, masked by {masks:?}"),
                }
            })
        }
        /// `mask` holds `ring`, within float error of the device pixels both
        /// lie on.
        fn holds(mask: Bounds<Pixels>, ring: Bounds<Pixels>) -> bool {
            let slack = px(0.01);
            mask.left() <= ring.left() + slack
                && mask.top() <= ring.top() + slack
                && mask.right() + slack >= ring.right()
                && mask.bottom() + slack >= ring.bottom()
        }

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let paths = (0..TABS.len())
            .map(|index| {
                crate::views::tests::fixture_repository(
                    fixture.path().join(format!("repository-{index}")),
                )
                .path()
                .to_owned()
            })
            .collect::<Vec<_>>();
        let initial = paths[0].clone();
        let destination = isolated_session("isolated-session.json");
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(initial),
                    Preferences::default(),
                    Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.apply_appearance(window, cx);
                app.open_repository_tab(paths[1].clone(), window, cx);
            })
        });
        settle_tab_test(&app, cx).await;
        let installed = cx.read(|cx| Theme::global(cx).button_focus_ring);
        assert_eq!(installed, appearance::BUTTON_FOCUS_RING);
        let footprint = installed.gap + installed.width;

        // The tab opened last is selected, then the first, at each scale.
        for scale in [2., 1.5, 1.] {
            cx.update(|window, _| window.set_scale_factor(scale));
            for selected in [1, 0] {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        app.switch_repository_tab(selected, window, cx)
                    })
                });
                settle_tab_test(&app, cx).await;
                draw(cx);
                let mask = tab_list_mask(cx, TABS[selected]);
                for selector in TABS.into_iter().flatten() {
                    let ring = rendered(cx, selector).dilate(footprint);
                    assert!(
                        holds(mask, ring),
                        "at scale {scale}, tab {selected} selected: the ring around \
                         {selector}, {ring:?}, lies outside the tab list's content mask {mask:?}"
                    );
                }
            }
        }
        cx.update(|window, _| window.set_scale_factor(2.));
        draw(cx);

        // Without the room the tabs lay out exactly as they do with it, and
        // the list clips every ring: the room alone keeps them whole.
        let with_room = layout(cx);
        cx.update(|window, cx| {
            Theme::global_mut(cx).button_focus_ring = FocusRing {
                width: px(0.),
                gap: px(0.),
                ..installed
            };
            app.update(cx, |_, cx| cx.notify());
            window.refresh();
        });
        draw(cx);
        assert_eq!(layout(cx), with_room, "the margin gives the room back");
        let mask = tab_list_mask(cx, TABS[0]);
        for selector in TABS.into_iter().flatten() {
            let ring = rendered(cx, selector).dilate(footprint);
            assert!(
                !holds(mask, ring),
                "without room the list clips the ring around {selector}, {ring:?}, to {mask:?}"
            );
        }
        cx.update(|window, cx| {
            Theme::global_mut(cx).button_focus_ring = installed;
            window.refresh();
        });
        settle_tab_test(&app, cx).await;
    }

    /// At interface text sizes 11, 12, 13 and 18 pt, at desktop text scale
    /// 1.0 and, on Linux (the only desktop that reports one), 0.5, the ring a
    /// focused first tab, a focused close button and a focused last tab draw,
    /// `gap + width` outside their edge, lies inside the window and inside the
    /// content mask the control paints in, the intersection of every
    /// ancestor's. The strip is the window's first row, so where its 36 px
    /// leave less than the ring's room above and below a tab it grows by
    /// exactly what it lacks. Where they already leave the room, at 12 pt and
    /// above at desktop scale 1.0, its minimum height is the 36 px it always
    /// had, so it lays out at that height with every tab and close button
    /// centered above its rule, where they always were. At 11 and 12 pt at
    /// scale factors 1.5 and 1.75 the ring stays inside out to its outer
    /// device pixel. A larger installed ring grows the strip in turn.
    #[gpui::test]
    async fn tab_strip_keeps_every_focus_ring_inside_the_window_at_every_text_size(
        cx: &mut TestAppContext,
    ) {
        use crate::tags::tests::{content_mask, draw, install_ring, rendered};
        use gpui_kit::component::{FocusRing, Root, Theme};
        use std::{cell::RefCell, rc::Rc};

        /// The first tab, its close button and the last tab.
        const CONTROLS: [&str; 3] = [
            "repository-tab-0",
            "repository-tab-close-0",
            "repository-tab-1",
        ];
        /// Moves keyboard focus through the window until `selector` draws a
        /// focus ring.
        fn focus(cx: &mut VisualTestContext, selector: &'static str) {
            cx.update(|window, cx| window.blur(cx));
            for _ in 0..64 {
                cx.update(|window, cx| window.focus_next(cx));
                draw(cx);
                let element = rendered(cx, selector);
                if !appearance::painted_button(cx, element).1.is_empty() {
                    return;
                }
            }
            panic!("keyboard focus never reaches {selector}");
        }
        /// `outer` holds `inner`, within float error of the device pixels
        /// both lie on.
        fn holds(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
            let slack = px(0.01);
            outer.left() <= inner.left() + slack
                && outer.top() <= inner.top() + slack
                && outer.right() + slack >= inner.right()
                && outer.bottom() + slack >= inner.bottom()
        }
        /// Focuses each control in turn and checks that its ring, out to the
        /// device pixel its outer edge is drawn on (`tab_list_ring_room`),
        /// lies inside the window and inside the content mask the control
        /// paints in.
        fn assert_rings_inside(cx: &mut VisualTestContext, at: &str) {
            let footprint = cx.update(|window, cx| tab_list_ring_room(window, cx));
            let viewport = cx.update(|window, _| {
                Bounds::new(point(Pixels::ZERO, Pixels::ZERO), window.viewport_size())
            });
            for selector in CONTROLS {
                // The ring is the focused Button's child, so it paints in the
                // mask the Button's own fill paints in.
                let mask = content_mask(cx, selector);
                focus(cx, selector);
                let ring = rendered(cx, selector).dilate(footprint);
                assert!(
                    holds(viewport, ring),
                    "at {at} the ring around focused {selector}, {ring:?}, leaves the window \
                     {viewport:?}"
                );
                assert!(
                    holds(mask, ring),
                    "at {at} the ring around focused {selector}, {ring:?}, lies outside its \
                     content mask {mask:?}"
                );
            }
            cx.update(|window, cx| window.blur(cx));
            draw(cx);
        }
        /// The strip leaves exactly the ring's room above and below the first
        /// tab, inside its rule.
        fn assert_strip_fits_ring(cx: &mut VisualTestContext, at: &str) {
            let (footprint, device) = cx.update(|window, cx| {
                (
                    appearance::button_ring_room(cx),
                    px(1. / window.scale_factor()),
                )
            });
            let strip = rendered(cx, "repository-tab-strip");
            let tab = rendered(cx, CONTROLS[0]);
            let above = tab.top() - strip.top();
            let below = strip.bottom() - TAB_STRIP_BORDER - tab.bottom();
            assert!(
                (above - footprint).abs() < device && (below - footprint).abs() < device,
                "at {at} the strip {strip:?} leaves {above:?} above and {below:?} below \
                 {tab:?}, not the ring's {footprint:?}"
            );
        }
        fn set_text_size(
            cx: &mut VisualTestContext,
            app: &Entity<GitTurtle>,
            interface: u8,
            desktop: f32,
        ) {
            cx.update(|window, cx| {
                #[cfg(target_os = "linux")]
                appearance::set_desktop_text_scale(desktop, cx);
                #[cfg(not(target_os = "linux"))]
                assert_eq!(desktop, 1.0, "only Linux reports a desktop text scale");
                app.update(cx, |app, cx| {
                    app.settings.interface_text_size = interface;
                    let code = app.settings.code_text_size;
                    appearance::apply_text_sizes(interface, code, window, cx);
                    cx.notify();
                })
            });
        }

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let paths = (0..2)
            .map(|index| {
                crate::views::tests::fixture_repository(
                    fixture.path().join(format!("repository-{index}")),
                )
                .path()
                .to_owned()
            })
            .collect::<Vec<_>>();
        let initial = paths[0].clone();
        let destination = isolated_session("isolated-session.json");
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(initial),
                    Preferences::default(),
                    Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1000.), px(680.)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.apply_appearance(window, cx);
                app.open_repository_tab(paths[1].clone(), window, cx);
            })
        });
        settle_tab_test(&app, cx).await;
        let installed = cx.read(|cx| Theme::global(cx).button_focus_ring);
        assert_eq!(installed, appearance::BUTTON_FOCUS_RING);
        let ring_room = installed.gap + installed.width;

        let desktop_scales: &[f32] = if cfg!(target_os = "linux") {
            &[1.0, 0.5]
        } else {
            &[1.0]
        };
        for &desktop in desktop_scales {
            for interface in [11, 12, 13, 18] {
                let at = format!("{interface} pt at desktop scale {desktop}");
                set_text_size(cx, &app, interface, desktop);
                settle_tab_test(&app, cx).await;
                draw(cx);
                assert_rings_inside(cx, &at);

                // The test window draws at scale factor 2 only. At scale
                // factor 1, as on the native host, GPUI rounds each authored
                // length to whole pixels before layout, ties toward zero, and
                // centers a tab in what the strip leaves above its rule: the
                // room there holds the ring too, as at 11 pt at desktop scale
                // 1.0, where 36 px alone leave 5 px around a 24 px tab.
                let whole = |length: Pixels| (f32::from(length) - 0.5).ceil();
                let around = whole(tab_strip_min_height(ring_room))
                    - f32::from(TAB_STRIP_BORDER)
                    - whole(appearance::ui_size(28.));
                assert!(
                    around >= 2. * f32::from(ring_room),
                    "at {at} at scale factor 1 the strip leaves {around} px around a tab"
                );

                let device = cx.update(|window, _| px(1. / window.scale_factor()));
                if desktop == 1.0 && interface >= 12 {
                    // The strip's 36 px already leave the room, so its minimum
                    // height is exactly what it was and nothing in it moves:
                    // it keeps that height, and every tab and close button
                    // stays centered above its rule.
                    assert_eq!(
                        tab_strip_min_height(ring_room),
                        appearance::ui_size(36.),
                        "at {at} the strip keeps its minimum height"
                    );
                    let strip = rendered(cx, "repository-tab-strip");
                    assert!(
                        (strip.size.height - appearance::ui_size(36.)).abs() < device,
                        "at {at} the strip keeps its height, not {strip:?}"
                    );
                    for selector in CONTROLS {
                        let control = rendered(cx, selector);
                        let above = control.top() - strip.top();
                        let below = strip.bottom() - TAB_STRIP_BORDER - control.bottom();
                        assert!(
                            (control.size.height - appearance::ui_size(28.)).abs() < device
                                && (above - below).abs() <= device,
                            "at {at} {selector}, {control:?}, stays centered in {strip:?}"
                        );
                    }
                } else {
                    assert!(
                        tab_strip_min_height(ring_room) > appearance::ui_size(36.),
                        "at {at} the strip grows"
                    );
                    assert_strip_fits_ring(cx, &at);
                }
            }
        }

        // At scale factors 1.5 and 1.75 the ring's 3 px end inside a device
        // pixel, which its outer edge is drawn on: below about 0.875 of the
        // default size the strip grows for the room rounded up to that pixel
        // (`tab_list_ring_room`), where it grew for the unrounded 3 px and
        // the ring's outer pixel fell outside the window above a tab.
        for scale in [1.5, 1.75] {
            cx.update(|window, _| window.set_scale_factor(scale));
            for &desktop in desktop_scales {
                for interface in [11, 12] {
                    let at = format!("{interface} pt at desktop scale {desktop}, scale {scale}");
                    set_text_size(cx, &app, interface, desktop);
                    settle_tab_test(&app, cx).await;
                    draw(cx);
                    assert_rings_inside(cx, &at);
                }
            }
        }
        cx.update(|window, _| window.set_scale_factor(2.));
        settle_tab_test(&app, cx).await;

        // The room follows the installed ring: a 5 px ring's room exceeds what
        // the strip's 36 px leave at the default size, so the strip grows.
        set_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE, 1.0);
        settle_tab_test(&app, cx).await;
        install_ring(
            cx,
            &app,
            FocusRing {
                width: px(3.),
                gap: px(2.),
                opacity: 1.,
            },
        );
        let at = "13 pt with a 5 px ring";
        assert_rings_inside(cx, at);
        assert_strip_fits_ring(cx, at);
        install_ring(cx, &app, installed);
        settle_tab_test(&app, cx).await;
    }

    /// A tab or close button focused from the keyboard draws its ring whole:
    /// one ring, its four sides on whole device pixels, as wide on every side
    /// and as far from its control on every side, inside the strip above its
    /// rule and inside the content mask it paints in, covered by nothing
    /// painted after it and clear of the tab beside it ([`tab_gap`]); and the
    /// focus moves nothing in the strip. Checked for the first tab, its close
    /// button and the selected second tab at scale factors 1.25, 1.5, 1.75
    /// and 2, at 11, 13 and 18 pt, in a 1480 × 800 and a 461 × 490 window,
    /// with the palette applied. At 11 pt and scale factor 1.25 or 2 the
    /// tabs' 0.25 rem gap once left the close button's ring reaching a device
    /// pixel into the selected tab, whose fill covered it.
    #[gpui::test]
    async fn a_keyboard_focused_tab_draws_its_ring_whole_on_device_pixels(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, rendered};
        use gpui_kit::{ScaledPixels, component::ActiveTheme as _};

        /// What the strip lays out, which focus must not move.
        const LAYOUT: [&str; 9] = [
            "repository-tab-strip",
            "repository-tabs",
            "repository-tab-frame-0",
            "repository-tab-frame-1",
            "repository-tab-0",
            "repository-tab-1",
            "repository-tab-close-0",
            "repository-tab-close-1",
            "repository-header",
        ];
        fn layout(cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
            LAYOUT
                .into_iter()
                .map(|selector| rendered(cx, selector))
                .collect()
        }
        /// Focuses the window's first control, then presses Tab until
        /// `selector` draws a ring: with nothing focused a key reaches no
        /// control's context, so Tab alone would focus nothing.
        fn tab_onto(cx: &mut VisualTestContext, selector: &'static str) {
            cx.update(|window, cx| {
                window.blur(cx);
                window.focus_next(cx);
            });
            draw(cx);
            for _ in 0..64 {
                let element = rendered(cx, selector);
                if !appearance::painted_button(cx, element).1.is_empty() {
                    return;
                }
                cx.simulate_keystrokes("tab");
                draw(cx);
            }
            panic!("Tab never reaches {selector}");
        }
        /// A ring the last frame painted, in device pixels.
        #[derive(Debug)]
        struct Ring {
            bounds: Bounds<f32>,
            /// Left, top, right and bottom.
            widths: [f32; 4],
            /// The union of the masks its sides are painted in: GPUI paints a
            /// border-only quad once per side.
            mask: Bounds<f32>,
            /// Every filled quad painted after it that reaches into its band.
            covered_by: Vec<Bounds<f32>>,
        }
        fn rings(cx: &mut VisualTestContext) -> Vec<Ring> {
            cx.update(|window, cx| {
                let installed = cx.theme().button_focus_ring;
                let color = cx.theme().ring.alpha(installed.opacity);
                let device = |b: Bounds<ScaledPixels>| {
                    Bounds::from_corners(
                        point(b.left().as_f32(), b.top().as_f32()),
                        point(b.right().as_f32(), b.bottom().as_f32()),
                    )
                };
                let quads = window.painted_quads();
                let mut rings: Vec<(usize, Ring)> = Vec::new();
                for (index, quad) in quads.iter().enumerate() {
                    let w = quad.border_widths;
                    let widths = [w.left, w.top, w.right, w.bottom].map(|w| w.as_f32());
                    if !quad.background.is_transparent()
                        || quad.border_color != color
                        || widths.iter().all(|w| *w == 0.)
                    {
                        continue;
                    }
                    let (bounds, mask) = (device(quad.bounds), device(quad.content_mask.bounds));
                    match rings.iter_mut().find(|(_, ring)| ring.bounds == bounds) {
                        Some((_, ring)) => ring.mask = ring.mask.union(&mask),
                        None => rings.push((
                            index,
                            Ring {
                                bounds,
                                widths,
                                mask,
                                covered_by: Vec::new(),
                            },
                        )),
                    }
                }
                for (first, ring) in &mut rings {
                    let (outer, [left, top, right, bottom]) = (ring.bounds, ring.widths);
                    let inner = Bounds::from_corners(
                        point(outer.left() + left, outer.top() + top),
                        point(outer.right() - right, outer.bottom() - bottom),
                    );
                    for quad in &quads[*first..] {
                        let drawn = device(quad.bounds);
                        let reaches = drawn.left() < outer.right()
                            && drawn.right() > outer.left()
                            && drawn.top() < outer.bottom()
                            && drawn.bottom() > outer.top();
                        let inside = drawn.left() >= inner.left()
                            && drawn.right() <= inner.right()
                            && drawn.top() >= inner.top()
                            && drawn.bottom() <= inner.bottom();
                        if !quad.background.is_transparent() && reaches && !inside {
                            ring.covered_by.push(drawn);
                        }
                    }
                }
                rings.into_iter().map(|(_, ring)| ring).collect()
            })
        }

        let (_fixture, app, cx, paths) = strip_window(cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(paths[1].clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;

        let mut failures = Vec::new();
        for (width, height) in [(1480., 800.), (461., 490.)] {
            for scale in [1.25, 1.5, 1.75, 2.] {
                for interface in [11, 13, 18] {
                    // A resize puts the test window back at scale 2.
                    resize_strip(cx, &app, (interface, width, height)).await;
                    cx.update(|window, cx| {
                        window.set_scale_factor(scale);
                        window.blur(cx);
                    });
                    settle_tab_test(&app, cx).await;
                    draw(cx);
                    assert_eq!(cx.update(|window, _| window.scale_factor()), scale);
                    let resting = layout(cx);
                    let strip = rendered(cx, "repository-tab-strip");
                    // The second tab is selected: the first close button's
                    // ring meets its frame's fill on the right, the second
                    // tab's meets the first tab's frame on the left.
                    for (control, beside) in [
                        ("repository-tab-0", None),
                        (
                            "repository-tab-close-0",
                            Some(("repository-tab-frame-1", true)),
                        ),
                        ("repository-tab-1", Some(("repository-tab-frame-0", false))),
                    ] {
                        let at = format!(
                            "{control} at scale {scale}, {interface} pt, {width} x {height}"
                        );
                        tab_onto(cx, control);
                        if layout(cx) != resting {
                            failures.push(format!(
                                "{at}: focus moved the strip from {resting:?} to {:?}",
                                layout(cx)
                            ));
                        }
                        let to_device = |b: Bounds<Pixels>| {
                            Bounds::from_corners(
                                point(f32::from(b.left()) * scale, f32::from(b.top()) * scale),
                                point(f32::from(b.right()) * scale, f32::from(b.bottom()) * scale),
                            )
                        };
                        let element = to_device(rendered(cx, control));
                        // The ring around the control; others in the ring's
                        // colour (a list's accent rule) lie elsewhere.
                        let rings = rings(cx)
                            .into_iter()
                            .filter(|ring| ring.bounds.contains(&element.center()))
                            .collect::<Vec<_>>();
                        let [ring] = rings.as_slice() else {
                            failures.push(format!("{at}: one ring, not {rings:?}"));
                            continue;
                        };
                        let (bounds, widths) = (ring.bounds, ring.widths);
                        let strip = to_device(strip);
                        let edges = [bounds.left(), bounds.top(), bounds.right(), bounds.bottom()];
                        let whole = |v: f32| (v - v.round()).abs() < 0.01;
                        let gaps = [
                            element.left() - bounds.left() - widths[0],
                            element.top() - bounds.top() - widths[1],
                            bounds.right() - widths[2] - element.right(),
                            bounds.bottom() - widths[3] - element.bottom(),
                        ];
                        let slack = 0.01;
                        let report = format!(
                            "{ring:?} with gaps {gaps:?} around {element:?} in strip {strip:?}"
                        );
                        if !edges.into_iter().chain(widths).all(whole) {
                            failures.push(format!("{at}: off the device pixel: {report}"));
                        }
                        if widths.iter().any(|w| (w - widths[0]).abs() > slack)
                            || gaps.iter().any(|g| (g - gaps[0]).abs() > slack)
                            || gaps[0] < 1. - slack
                        {
                            failures.push(format!("{at}: uneven ring: {report}"));
                        }
                        // The strip's 1 px rule, as layout rounds a stroke.
                        let rule = (scale - 0.5_f32).ceil().max(1.);
                        if bounds.top() < strip.top() - slack
                            || bounds.bottom() > strip.bottom() - rule + slack
                        {
                            failures.push(format!("{at}: outside the strip: {report}"));
                        }
                        let mask = ring.mask;
                        if mask.left() > bounds.left() + slack
                            || mask.top() > bounds.top() + slack
                            || mask.right() + slack < bounds.right()
                            || mask.bottom() + slack < bounds.bottom()
                        {
                            failures.push(format!("{at}: clipped: {report}"));
                        }
                        if !ring.covered_by.is_empty() {
                            failures.push(format!("{at}: painted over: {report}"));
                        }
                        if let Some((neighbour, right)) = beside {
                            let neighbour = to_device(rendered(cx, neighbour));
                            let reaches = if right {
                                bounds.right() > neighbour.left() + slack
                            } else {
                                bounds.left() < neighbour.right() - slack
                            };
                            if reaches {
                                failures.push(format!(
                                    "{at}: into the tab beside it, {neighbour:?}: {report}"
                                ));
                            }
                        }
                    }
                    cx.update(|window, cx| window.blur(cx));
                    draw(cx);
                }
            }
        }
        cx.update(|window, _| window.set_scale_factor(2.));
        reset_strip_text(cx, &app);
        settle_tab_test(&app, cx).await;
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The strip keeps a ring's room beside its tab list and between its
    /// controls ([`tab_gap`]): Main Menu focused from the keyboard beside a
    /// selected first tab draws its ring clear of that tab's frame and
    /// covered by nothing painted after it, the first tab's ring stays clear
    /// of Menu, and New tab's and Workspaces' rings stay clear of each other,
    /// as do the window controls' in a client-decorated window, at 11, 13
    /// and 18 pt at scale factors 1.25, 1.5, 1.75 and 2 in a
    /// 1480 × 800 and a 461 × 490 window. At 11 pt and scale factor 1.25 or
    /// 2 the strip's 0.25 rem gap left Menu's ring a device pixel into the
    /// first tab, whose selected fill, painted after it, covered that pixel,
    /// and the first tab's ring a device pixel over Menu; the window
    /// controls' `gap_1` let each control's ring reach a device pixel into
    /// the one beside it.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn the_strip_keeps_a_rings_room_beside_its_tabs(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, rendered};
        use gpui_kit::{ScaledPixels, component::ActiveTheme as _};

        /// Focuses the window's first control, then presses Tab until
        /// `selector` draws a ring.
        fn tab_onto(cx: &mut VisualTestContext, selector: &'static str) {
            cx.update(|window, cx| {
                window.blur(cx);
                window.focus_next(cx);
            });
            draw(cx);
            for _ in 0..64 {
                let element = rendered(cx, selector);
                if !appearance::painted_button(cx, element).1.is_empty() {
                    return;
                }
                cx.simulate_keystrokes("tab");
                draw(cx);
            }
            panic!("Tab never reaches {selector}");
        }
        /// Each ring the last frame painted around the centre of `element`,
        /// in device pixels, with every filled quad painted after it that
        /// reaches into its band.
        type Ring = (Bounds<f32>, Vec<Bounds<f32>>);
        fn rings_around(cx: &mut VisualTestContext, element: Bounds<f32>) -> Vec<Ring> {
            cx.update(|window, cx| {
                let installed = cx.theme().button_focus_ring;
                let color = cx.theme().ring.alpha(installed.opacity);
                let device = |b: Bounds<ScaledPixels>| {
                    Bounds::from_corners(
                        point(b.left().as_f32(), b.top().as_f32()),
                        point(b.right().as_f32(), b.bottom().as_f32()),
                    )
                };
                let quads = window.painted_quads();
                let mut rings: Vec<Ring> = Vec::new();
                for (index, quad) in quads.iter().enumerate() {
                    let w = quad.border_widths;
                    let [left, top, right, bottom] =
                        [w.left, w.top, w.right, w.bottom].map(|w| w.as_f32());
                    let outer = device(quad.bounds);
                    if !quad.background.is_transparent()
                        || quad.border_color != color
                        || [left, top, right, bottom].iter().all(|w| *w == 0.)
                        || !outer.contains(&element.center())
                        || rings.iter().any(|(ring, _)| *ring == outer)
                    {
                        continue;
                    }
                    let inner = Bounds::from_corners(
                        point(outer.left() + left, outer.top() + top),
                        point(outer.right() - right, outer.bottom() - bottom),
                    );
                    let covered = quads[index..]
                        .iter()
                        .filter(|quad| !quad.background.is_transparent())
                        .map(|quad| device(quad.bounds))
                        .filter(|drawn| {
                            let reaches = drawn.left() < outer.right()
                                && drawn.right() > outer.left()
                                && drawn.top() < outer.bottom()
                                && drawn.bottom() > outer.top();
                            let inside = drawn.left() >= inner.left()
                                && drawn.right() <= inner.right()
                                && drawn.top() >= inner.top()
                                && drawn.bottom() <= inner.bottom();
                            reaches && !inside
                        })
                        .collect();
                    rings.push((outer, covered));
                }
                rings
            })
        }

        let _decorated = Decorated::on();

        let (_fixture, app, cx, paths) = strip_window(cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(paths[1].clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;

        let mut failures = Vec::new();
        for (width, height) in [(1480., 800.), (461., 490.)] {
            for scale in [1.25, 1.5, 1.75, 2.] {
                for interface in [11, 13, 18] {
                    // A resize puts the test window back at scale 2.
                    resize_strip(cx, &app, (interface, width, height)).await;
                    cx.update(|window, cx| {
                        window.set_scale_factor(scale);
                        window.blur(cx);
                    });
                    settle_tab_test(&app, cx).await;
                    draw(cx);
                    assert_eq!(cx.update(|window, _| window.scale_factor()), scale);
                    assert_eq!(
                        cx.update(|_, cx| app.read(cx).repository_tabs.active),
                        Some(0),
                        "the first tab is selected"
                    );
                    // Each control and the one beside it, on its right or
                    // its left: the first tab's frame is selected, so its
                    // fill is painted after Menu's ring.
                    for (control, beside, right) in [
                        ("main-menu", "repository-tab-frame-0", true),
                        ("repository-tab-0", "main-menu", false),
                        ("new-repository-tab", "repository-workspaces", true),
                        ("repository-workspaces", "new-repository-tab", false),
                        ("minimize-window", "maximize-window", true),
                        ("maximize-window", "minimize-window", false),
                        ("maximize-window", "close-window", true),
                        ("close-window", "maximize-window", false),
                    ] {
                        let at = format!(
                            "{control} at scale {scale}, {interface} pt, {width} x {height}"
                        );
                        tab_onto(cx, control);
                        let to_device = |b: Bounds<Pixels>| {
                            Bounds::from_corners(
                                point(f32::from(b.left()) * scale, f32::from(b.top()) * scale),
                                point(f32::from(b.right()) * scale, f32::from(b.bottom()) * scale),
                            )
                        };
                        let element = to_device(rendered(cx, control));
                        let rings = rings_around(cx, element);
                        let [(ring, covered)] = rings.as_slice() else {
                            failures.push(format!("{at}: one ring, not {rings:?}"));
                            continue;
                        };
                        let neighbour = to_device(rendered(cx, beside));
                        let slack = 0.01;
                        let reaches = if right {
                            ring.right() > neighbour.left() + slack
                        } else {
                            ring.left() < neighbour.right() - slack
                        };
                        if reaches {
                            failures.push(format!(
                                "{at}: its ring {ring:?} reaches into {beside} {neighbour:?}"
                            ));
                        }
                        if !covered.is_empty() {
                            failures.push(format!(
                                "{at}: its ring {ring:?} is painted over by {covered:?}"
                            ));
                        }
                    }
                    cx.update(|window, cx| window.blur(cx));
                    draw(cx);
                }
            }
        }
        cx.update(|window, _| window.set_scale_factor(2.));
        reset_strip_text(cx, &app);
        settle_tab_test(&app, cx).await;
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Draws the test's window as client-decorated, with its controls,
    /// while it lives, and as GPUI's test window once it is dropped.
    #[cfg(target_os = "linux")]
    struct Decorated;

    #[cfg(target_os = "linux")]
    impl Decorated {
        fn on() -> Self {
            crate::window_chrome::CLIENT_DECORATED.with(|decorated| decorated.set(true));
            Self
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for Decorated {
        fn drop(&mut self) {
            crate::window_chrome::CLIENT_DECORATED.with(|decorated| decorated.set(false));
        }
    }

    /// In a client-decorated window, whose strip draws the window controls
    /// and, after the tabs inside the list, the drag region, the tabs take
    /// exactly the room `GitTurtle::tab_list_room` predicts for them, the
    /// room `window_chrome::strip_width` leaves: the list, between Menu and
    /// New tab, holds that room, one gap and the drag region's least width
    /// inside its ring's room on either side, and a tab whose name the strip
    /// cuts spans exactly that room. At 13 and 18 pt in 461 × 490 and
    /// 640 × 680 windows at scale factors 1.25, 1.5 and 2. The exact-fit
    /// tests above lay the strip out undecorated, where the decorated room
    /// goes unchecked.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_decorated_strip_gives_its_tabs_the_room_predicted(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, rendered};

        let _decorated = Decorated::on();
        let (_fixture, app, cx, paths) = strip_window(cx).await;
        app.update(cx, |app, cx| {
            app.project_names
                .insert(paths[0].clone(), "a much longer repository name".to_owned());
            cx.notify();
        });
        let mut failures = Vec::new();
        let mut cut = 0;
        for (interface, width, height) in [(13, 461., 490.), (18, 461., 490.), (18, 640., 680.)] {
            for scale in [1.25, 1.5, 2.] {
                // A resize puts the test window back at scale 2.
                resize_strip(cx, &app, (interface, width, height)).await;
                cx.update(|window, _| window.set_scale_factor(scale));
                settle_tab_test(&app, cx).await;
                draw(cx);
                let at = format!("{interface} pt in {width} × {height} at {scale}x");
                for control in ["minimize-window", "maximize-window", "close-window"] {
                    if cx.debug_bounds(control).is_none() {
                        failures.push(format!("{at}: {control} is not drawn"));
                    }
                }
                let (room, ring, gap, drag, whole) = cx.update(|window, cx| {
                    let app = app.read(cx);
                    let labels = app.tab_strip_labels(window, cx);
                    (
                        app.tab_list_room(
                            labels,
                            app.tab_strip_labelled_actions(window),
                            window,
                            cx,
                        ),
                        tab_list_ring_room(window, cx),
                        tab_gap(window),
                        window.pixel_snap(appearance::ui_size(32.)),
                        app.tabs_whole_width(window),
                    )
                });
                let list = rendered(cx, "repository-tabs").size.width - ring * 2. - gap - drag;
                if (list - room).abs() >= px(0.01) {
                    failures.push(format!(
                        "{at}: the list holds {list:?} for its tabs, predicted {room:?}"
                    ));
                }
                if whole > room {
                    cut += 1;
                    let tab = rendered(cx, "repository-tab-frame-0").size.width;
                    if (tab - room).abs() >= px(0.01) {
                        failures.push(format!(
                            "{at}: the cut tab is {tab:?} wide, not the {room:?} predicted"
                        ));
                    }
                }
            }
        }
        cx.update(|window, _| window.set_scale_factor(2.));
        reset_strip_text(cx, &app);
        settle_tab_test(&app, cx).await;
        assert!(cut > 0, "some window cuts the tab's name");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The text sizes and windows the narrow tab strip tests lay out.
    const STRIP_SIZES: [(u8, f32, f32); 6] = [
        (13, 1480., 980.),
        (13, 640., 680.),
        (13, 461., 490.),
        (18, 1480., 980.),
        (18, 640., 680.),
        (18, 461., 490.),
    ];

    /// A 1480 × 980 window on two fresh repositories, "compact" and
    /// "second", the first open in the only tab, and their paths. Keep the
    /// returned directory until the test ends.
    async fn strip_window(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut VisualTestContext,
        [PathBuf; 2],
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let paths = ["compact", "second"].map(|name| {
            crate::views::tests::fixture_repository(fixture.path().join(name))
                .path()
                .to_owned()
        });
        let initial = paths[0].clone();
        let destination = isolated_session("isolated-session.json");
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(initial),
                    Preferences::default(),
                    Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1480.), px(980.)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
        settle_tab_test(&app, cx).await;
        (fixture, app, cx, paths)
    }

    /// Lays the strip's window out at `interface` pt in `width` × `height`.
    async fn resize_strip(
        cx: &mut VisualTestContext,
        app: &Entity<GitTurtle>,
        (interface, width, height): (u8, f32, f32),
    ) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                let code = app.settings.code_text_size;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        cx.simulate_resize(size(px(width), px(height)));
        settle_tab_test(app, cx).await;
        crate::tags::tests::draw(cx);
    }

    /// Puts the strip's window back at the default interface size.
    fn reset_strip_text(cx: &mut VisualTestContext, app: &Entity<GitTurtle>) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let size = appearance::DEFAULT_INTERFACE_TEXT_SIZE;
                app.settings.interface_text_size = size;
                let code = app.settings.code_text_size;
                appearance::apply_text_sizes(size, code, window, cx);
            })
        });
    }

    /// A tab's width with `name` whole, and at its floor, the name's first
    /// character and ellipsis, both with the tab's insets.
    fn tab_widths(cx: &mut VisualTestContext, name: &str) -> (Pixels, Pixels) {
        cx.update(|window, _| {
            let weight = window.text_style().font_weight;
            let size = appearance::ui_text(12.);
            let insets = appearance::ui_size(crate::LABEL_INSET * 2.);
            let width = |text: &str| crate::views::shaped_width(text, size, weight, window);
            let cut = format!("{}…", &name[..1]);
            (
                insets + width(name).ceil(),
                insets + width(&cut).floor() + px(1.),
            )
        })
    }

    /// Content before decoration: a tab too wide for the strip's list cuts
    /// its name, never its close button. At 13 and 18 pt in 461 and 640 px
    /// windows the close button and the ring a focused one draws,
    /// `button_ring_room` outside its edge, lie whole inside the list's clip
    /// and the window, right of the tab, which is no wider than its whole
    /// label and no narrower than its first character and ellipsis, so it
    /// never shows a lone one. The native fixture's "compact" is whole in
    /// every window: at 18 pt in the 461 × 490 half tile, where the list's
    /// clip was about 116 px beside a labelled Menu and Workspaces and the
    /// button was once cut, then the name, both are icons
    /// (`tab_strip_actions_become_icons_before_a_tab_name_is_cut`). In a
    /// 1480 × 980 window the tab keeps the width it had, the long name its
    /// scaled 220 px. With two tabs, the list scrolls only once both names
    /// are down to their floors, and no tab overlaps the next.
    #[gpui::test]
    async fn a_narrow_tab_strip_cuts_the_name_before_the_close_button(cx: &mut TestAppContext) {
        use crate::tags::tests::{content_mask, draw, holds, rendered};

        /// The list's clip, read from the first tab, which the pointer can
        /// always reach, and the window's bounds.
        fn clips(cx: &mut VisualTestContext) -> (Bounds<Pixels>, Bounds<Pixels>) {
            let mask = content_mask(cx, "repository-tab-0");
            cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
            draw(cx);
            let viewport = cx.update(|window, _| {
                Bounds::new(point(Pixels::ZERO, Pixels::ZERO), window.viewport_size())
            });
            (mask, viewport)
        }

        let (_fixture, app, cx, paths) = strip_window(cx).await;

        let (ring_room, device) = cx.update(|window, cx| {
            (
                appearance::button_ring_room(cx),
                px(1. / window.scale_factor()),
            )
        });

        for name in ["compact", "a much longer repository name"] {
            app.update(cx, |app, cx| {
                app.project_names.insert(paths[0].clone(), name.to_owned());
                cx.notify();
            });
            for sizes in STRIP_SIZES {
                let (interface, width, height) = sizes;
                let at = format!("{name:?} at {interface} pt in {width} × {height}");
                resize_strip(cx, &app, sizes).await;
                let (whole, floor) = tab_widths(cx, name);
                let tab = rendered(cx, "repository-tab-0");
                let close = rendered(cx, "repository-tab-close-0");
                let (mask, viewport) = clips(cx);
                let ring = close.dilate(ring_room);
                assert!(
                    holds(mask, ring) && holds(viewport, ring),
                    "{at}: the close button {close:?} and its ring {ring:?} are cut by the \
                     list's clip {mask:?} or the window {viewport:?}"
                );
                assert!(
                    (close.size.width - appearance::ui_size(28.)).abs() < device
                        && tab.right() <= close.left() + device,
                    "{at}: the close button {close:?} keeps its size right of the tab {tab:?}"
                );
                // A tab was never wider than its scaled 220 px.
                let widest = whole.min(appearance::ui_size(220.));
                assert!(
                    tab.size.width >= floor - device && tab.size.width <= widest + device,
                    "{at}: the tab {tab:?} lies between its first character and ellipsis \
                     {floor:?} and its whole label {whole:?}"
                );
                let cut = tab.size.width < whole - device;
                if width > 1000. {
                    assert!(
                        (tab.size.width - widest).abs() < device,
                        "{at}: the tab {tab:?} keeps its width {widest:?}"
                    );
                } else if name == "compact" {
                    assert!(!cut, "{at}: the name is whole in {tab:?}");
                } else {
                    assert!(cut, "{at}: the long name is cut in {tab:?}");
                }
            }
        }

        // Two tabs: each keeps its floor and its close button beside it, and
        // where both close buttons cannot be whole, both names are at their
        // floors and the list scrolls.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(paths[1].clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        for sizes in STRIP_SIZES {
            let (interface, width, height) = sizes;
            let at = format!("two tabs at {interface} pt in {width} × {height}");
            resize_strip(cx, &app, sizes).await;
            let (mask, viewport) = clips(cx);
            let tabs = [
                (
                    "a much longer repository name",
                    "repository-tab-0",
                    "repository-tab-close-0",
                ),
                ("second", "repository-tab-1", "repository-tab-close-1"),
            ]
            .map(|(name, tab, close)| {
                let (_, floor) = tab_widths(cx, name);
                let (tab, close) = (rendered(cx, tab), rendered(cx, close));
                assert!(
                    tab.size.width >= floor - device && tab.right() <= close.left() + device,
                    "{at}: {name:?}, {tab:?}, keeps its floor {floor:?} left of {close:?}"
                );
                (tab, close, floor)
            });
            assert!(
                tabs[0].1.right() <= tabs[1].0.left() + device,
                "{at}: the first close button {:?} overlaps the second tab {:?}",
                tabs[0].1,
                tabs[1].0
            );
            let whole = tabs.iter().all(|(_, close, _)| {
                let ring = close.dilate(ring_room);
                holds(mask, ring) && holds(viewport, ring)
            });
            let floored = tabs
                .iter()
                .all(|(tab, _, floor)| tab.size.width < *floor + device);
            assert!(
                whole || floored,
                "{at}: a close button is cut in {mask:?} before both names reach their \
                 floors: {tabs:?}"
            );
        }
        reset_strip_text(cx, &app);
    }

    /// One responsive order in the tab strip: Menu (on Linux) and Workspaces
    /// keep their labels only where every tab's name is whole beside them,
    /// and become 28 px icons, keeping their names, "Main Menu" and
    /// "Workspaces and tab actions", before a tab's name is cut, one at a
    /// time, Workspaces' first, and only where the tabs need the room. At 13
    /// and 18 pt in 461 ×
    /// 490, 640 × 680 and 1480 × 980 windows, with one tab named "compact" or
    /// "a much longer repository name" and then with "second" beside it, the
    /// form drawn is the one `tab_strip_labels` decides before layout, and
    /// the list takes the room `tab_list_room` predicts for it; a labelled
    /// control is as wide as predicted, while one shows every tab is whole,
    /// and a control drawn as its icon would not fit its label: the room the
    /// whole tabs leave empty in the list is less than the label takes. At 18
    /// pt in the 461 × 490 half tile, where "compact" was cut to "com…"
    /// beside a 100 px Menu and a 173 px Workspaces and then both became
    /// icons with about 175 px of the list empty, Menu keeps its label beside
    /// Workspaces' icon and the name is whole.
    #[gpui::test]
    async fn tab_strip_actions_become_icons_before_a_tab_name_is_cut(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, rendered};
        use crate::views::{COMPACT_NAMES, TabStripLabels};

        let (_fixture, app, cx, paths) = strip_window(cx).await;
        let (ring_room, device) = cx.update(|window, cx| {
            (
                appearance::button_ring_room(cx),
                px(1. / window.scale_factor()),
            )
        });
        let controls: &[(&str, &str)] = if cfg!(target_os = "linux") {
            &[
                ("main-menu", "Main Menu"),
                ("repository-workspaces", "Workspaces and tab actions"),
            ]
        } else {
            &[("repository-workspaces", "Workspaces and tab actions")]
        };
        let mut forms = std::collections::BTreeSet::new();
        for tabs in [1, 2] {
            if tabs == 2 {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        app.open_repository_tab(paths[1].clone(), window, cx)
                    })
                });
                settle_tab_test(&app, cx).await;
            }
            for name in ["compact", "a much longer repository name"] {
                app.update(cx, |app, cx| {
                    app.project_names.insert(paths[0].clone(), name.to_owned());
                    cx.notify();
                });
                for sizes in STRIP_SIZES {
                    let (interface, width, height) = sizes;
                    let at =
                        format!("{name:?} of {tabs} tabs at {interface} pt in {width} × {height}");
                    resize_strip(cx, &app, sizes).await;
                    COMPACT_NAMES.with(|names| names.borrow_mut().clear());
                    draw(cx);
                    let names = COMPACT_NAMES.with(|names| names.borrow().clone());
                    let (labels, room, (menu, workspaces), whole, icon) =
                        cx.update(|window, cx| {
                            let app = app.read(cx);
                            let labels = app.tab_strip_labels(window, cx);
                            (
                                labels,
                                app.tab_list_room(
                                    labels,
                                    app.tab_strip_labelled_actions(window),
                                    window,
                                    cx,
                                ),
                                app.tab_strip_labelled_actions(window),
                                app.tabs_whole_width(window),
                                window.pixel_snap(appearance::ui_size(28.)),
                            )
                        });
                    forms.insert((interface, width as u32, tabs, name, labels));
                    // A label is measured as layout lays it out, so a
                    // labelled control is predicted as wide as it is drawn.
                    for &(control, accessible) in controls {
                        let (labelled, wide) = match control {
                            "main-menu" => (labels.menu, menu),
                            _ => (labels.workspaces, workspaces),
                        };
                        let drawn = rendered(cx, control).size.width;
                        let predicted = if labelled { wide } else { icon };
                        assert!(
                            (drawn - predicted).abs() < px(0.01),
                            "{at}: {control} is {drawn:?}, predicted {predicted:?} wide"
                        );
                        assert_eq!(
                            names.get(control).copied(),
                            (!labelled).then_some(accessible),
                            "{at}: {control}'s compact name"
                        );
                        // Its label gave way because the tabs need its room.
                        assert!(
                            labelled || room - whole < wide - icon,
                            "{at}: {control} is its icon beside {:?} of empty list, which \
                             holds the {:?} its label adds",
                            room - whole,
                            wide - icon
                        );
                    }
                    // The list's bounds hold the ring's room on either side.
                    let list = rendered(cx, "repository-tabs").size.width - ring_room * 2.;
                    assert!(
                        (list - room).abs() < px(0.01),
                        "{at}: the tab list takes {list:?}, predicted {room:?}"
                    );
                    let names = [name, "second"];
                    for index in 0..tabs {
                        let (whole, _) = tab_widths(cx, names[index]);
                        let whole = whole.min(appearance::ui_size(220.));
                        let tab = rendered(cx, ["repository-tab-0", "repository-tab-1"][index]);
                        assert!(
                            !labels.any() || tab.size.width >= whole - device,
                            "{at}: tab {index}, {tab:?}, is cut to less than {whole:?} \
                             beside labelled actions"
                        );
                    }
                }
            }
        }
        // Each form is reached where its room is: both labels where the tabs
        // leave room, Menu's alone beside "compact" at 18 pt in the 461 × 490
        // half tile, where Linux draws Menu, and both icons beside the long
        // name there.
        let labelled = |menu: bool, workspaces| TabStripLabels {
            menu: menu && cfg!(target_os = "linux"),
            workspaces,
        };
        assert!(
            forms.contains(&(13, 1480, 1, "compact", labelled(true, true))),
            "{forms:?}"
        );
        assert!(
            !cfg!(target_os = "linux")
                || forms.contains(&(18, 461, 1, "compact", labelled(true, false))),
            "{forms:?}"
        );
        assert!(
            forms.iter().any(|form| form.4 == TabStripLabels::default()),
            "{forms:?}"
        );
        reset_strip_text(cx, &app);
    }

    /// A tab label is laid out once for windows at two scale factors drawn
    /// in turn, as a monitor of each shows them: the cache keeps its whole
    /// pixel and each read rounds it to that window's device pixels, so at
    /// 1.5 it lands on a third of a pixel and at 2 on a half. Keyed by the
    /// scale factor, the cache's one entry was laid out again at each change.
    #[gpui::test]
    async fn tab_labels_are_laid_out_once_across_scale_factors(cx: &mut TestAppContext) {
        let (_fixture, _app, cx, _paths) = strip_window(cx).await;
        let label = "a much longer repository name";
        // The label's width at `scale`, and the lines its read laid out.
        let read = |cx: &mut VisualTestContext, scale: f32| {
            let layouts = || crate::views::LINE_LAYOUTS.with(|count| count.get());
            cx.update(|window, _| {
                window.set_scale_factor(scale);
                let before = layouts();
                let key = crate::views::LineKey::current(window);
                let width = tab_label_widths(label, 0, &key, window).0;
                (width, layouts() - before)
            })
        };
        let first = [read(cx, 1.5).0, read(cx, 2.).0];
        for turn in 0..3 {
            for (scale, width) in [1.5, 2.].into_iter().zip(first) {
                assert_eq!(
                    read(cx, scale),
                    (width, 0),
                    "turn {turn} at {scale}: its own width, with no label laid out again"
                );
            }
        }
        for (width, scale) in first.into_iter().zip([1.5, 2.]) {
            let device = f32::from(width) * scale;
            assert!(
                (device - device.round()).abs() < 0.001,
                "{width:?} on a device pixel at {scale}"
            );
        }
        cx.update(|window, _| window.set_scale_factor(2.));
    }

    /// A tab cut to its floor keeps the floor whole at scale factor 1.5:
    /// in a window too narrow for its name, at 13, 17 and 18 pt, the tab
    /// lays out at its floor, no narrower than its laid-out insets and the
    /// floor's whole pixel, the room its first character and ellipsis need.
    /// The floor in whole pixels lies off the device grid there for every
    /// other width, as at 13 pt, and layout once rounded such a tab a third
    /// of a pixel narrower; the floor is now rounded up to a device pixel, as
    /// the label is. The insets are each snapped as layout snaps the label's
    /// padding: at 17 pt their scaled sum rounded a device pixel short of
    /// the two paddings. The window is too narrow on every platform's strip:
    /// where its 160 px leave the drawn tab room past its floor, it gives
    /// that room up (macOS 15, without Linux's Menu, drew the tab 56 px wide
    /// there at 13 pt).
    #[gpui::test]
    async fn a_tab_at_its_floor_keeps_it_whole_at_a_fractional_scale(cx: &mut TestAppContext) {
        use crate::tags::tests::rendered;

        let (_fixture, app, cx, paths) = strip_window(cx).await;
        let name = "a much longer repository name";
        app.update(cx, |app, cx| {
            app.project_names.insert(paths[0].clone(), name.to_owned());
            cx.notify();
        });
        let mut off_grid = 0;
        for interface in [13, 17, 18] {
            let at = format!("{interface} pt");
            resize_strip(cx, &app, (interface, 160., 490.)).await;
            // A resize lays the window out at its own scale factor again.
            cx.update(|window, _| window.set_scale_factor(1.5));
            crate::tags::tests::draw(cx);
            let (insets, floor, device) = cx.update(|window, _| {
                assert_eq!(window.scale_factor(), 1.5, "{at}");
                let weight = window.text_style().font_weight;
                let size = appearance::ui_text(12.);
                let cut = crate::views::shaped_width("a…", size, weight, window);
                (
                    tab_label_insets(window),
                    cut.floor() + px(1.),
                    px(1. / window.scale_factor()),
                )
            });
            let device_floor = f32::from(floor) * 1.5;
            off_grid += usize::from((device_floor - device_floor.round()).abs() > 0.01);
            // Too narrow for the name on every platform: 160 px leaves the
            // tab at its floor beside Linux's Menu, but macOS's strip holds
            // no Menu (`TabStripLabels`), and there the list kept 84 px at
            // 13 pt and the tab 56. Where the drawn tab has room past its
            // floor, the window gives up that room and a pixel more.
            let past = rendered(cx, "repository-tab-0").size.width - (insets + floor);
            if past >= device {
                let narrower = 160. - (f32::from(past) * 1.5).ceil() / 1.5 - 1.;
                resize_strip(cx, &app, (interface, narrower, 490.)).await;
                cx.update(|window, _| window.set_scale_factor(1.5));
                crate::tags::tests::draw(cx);
            }
            let tab = rendered(cx, "repository-tab-0");
            assert!(
                tab.size.width >= insets + floor - px(0.01),
                "{at}: the tab {tab:?} is narrower than its insets {insets:?} and floor {floor:?}"
            );
            assert!(
                tab.size.width < insets + floor + device,
                "{at}: the tab {tab:?} stands at its floor {floor:?}, the window too narrow"
            );
        }
        assert!(off_grid > 0, "a floor lies off the device grid at 1.5");
        reset_strip_text(cx, &app);
        cx.update(|window, _| window.set_scale_factor(2.));
    }

    /// The strip's choice measures every tab's whole label on each frame
    /// (`tabs_whole_width`), and on a frame where no tab changed that
    /// allocates nothing: each label is written into one buffer the thread
    /// keeps and read from the label cache, under the line key the window's
    /// root render read as it began (`LineKey::frame`), which reading again
    /// within the render allocates nothing either. It once formatted each
    /// tab's label, copied its project's name and read the key, three
    /// allocations per tab, and collected the widths, one more per frame;
    /// then read the key on its own, which builds the window's text style,
    /// whose default font features GPUI allocates. In a frame the window
    /// draws, the root render reads the key once and the strip, measuring
    /// and drawing its tabs, reads none of its own: without the root
    /// render's key the strip read two each frame, which the measurements
    /// below, each under a key the test opens, would not see. The labels it
    /// measures are the tabs' own: a renamed project's name, and a folder's
    /// name with a running write's "· working".
    #[gpui::test]
    async fn the_tab_strip_measures_its_tabs_without_allocating(cx: &mut TestAppContext) {
        let (_fixture, app, cx, paths) = strip_window(cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(paths[1].clone(), window, cx)
            })
        });
        settle_tab_test(&app, cx).await;
        app.update(cx, |app, cx| {
            app.project_names
                .insert(paths[0].clone(), "a much longer repository name".into());
            cx.notify();
        });
        crate::tags::tests::draw(cx);
        // One frame the window draws, every view built: the strip's line
        // keys as the strip's own source reads them.
        crate::views::line_key_reads::watch("repository_tabs.rs");
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let reads = crate::views::line_key_reads::take();
        crate::views::line_key_reads::stop();
        assert_eq!(
            reads,
            crate::views::line_key_reads::Reads {
                frames: 1,
                shared: 2,
                read: 0
            },
            "the root render reads the line key once, and the strip's measurement and its \
             tabs share it"
        );
        // Reading the line key within the window's root render, which reads
        // it once as it begins.
        let key = cx.update(|window, _| {
            let _frame = crate::views::LineKey::frame(window);
            let before = allocations();
            let _ = crate::views::LineKey::current(window);
            allocations() - before
        });
        assert_eq!(key, 0, "reading the line key within the render allocates");
        // The tab list's whole width, and what measuring it allocates within
        // the window's root render.
        let measure = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let _frame = crate::views::LineKey::frame(window);
                let app = app.read(cx);
                let before = allocations();
                let whole = app.tabs_whole_width(window);
                (whole, allocations() - before)
            })
        };
        let labels = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.repository_tabs
                    .tabs
                    .iter()
                    .map(|tab| app.tab_label(tab).0)
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(
            labels(cx),
            ["a much longer repository name", "second"],
            "each tab's label is its project's name"
        );
        let (whole, _) = measure(cx);
        for frame in 0..3 {
            assert_eq!(
                measure(cx),
                (whole, 0),
                "frame {frame}: the same width, with nothing allocated"
            );
        }
        // A tab that changes is measured again, and then allocates nothing.
        app.update(cx, |app, cx| {
            app.operation_busy = Some("Fetching…");
            app.operation_repository = Some(paths[1].clone());
            cx.notify();
        });
        crate::tags::tests::draw(cx);
        cx.executor()
            .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
        crate::tags::tests::draw(cx);
        assert_eq!(
            labels(cx),
            ["a much longer repository name", "second · working"],
            "the busy tab's label reports its work"
        );
        let (working, _) = measure(cx);
        assert!(working > whole, "{working:?} for the longer label");
        assert_eq!(measure(cx), (working, 0), "nothing allocated");
        app.update(cx, |app, cx| {
            app.operation_busy = None;
            app.operation_repository = None;
            cx.notify();
        });
    }

    /// Every clip that keeps room for the ring a focused Button draws outside
    /// its edge keeps exactly the installed ring's room,
    /// `appearance::button_ring_room`, and follows a ring of another size:
    /// the repository tab list, the Tags list, the Reflog list and the Your
    /// themes list. The Your themes list's room is also the room its page
    /// reveal keeps around a revealed row, and the room its strips cover
    /// while the rows stand off a boundary (`rows_off_boundary`).
    #[gpui::test]
    async fn every_ring_room_follows_the_installed_ring(cx: &mut TestAppContext) {
        use crate::tags::tests::{content_mask, draw, install_ring, rendered, tagged_repository};
        use appearance::{ThemeChoice, custom::CustomTheme};
        use gpui_kit::component::{FocusRing, Root, Theme, WindowExt};
        use std::{cell::RefCell, rc::Rc};

        /// How far inside `mask` the nearest of `control`'s edges stands.
        fn room_kept(mask: Bounds<Pixels>, control: Bounds<Pixels>) -> Pixels {
            [
                control.left() - mask.left(),
                control.top() - mask.top(),
                mask.right() - control.right(),
                mask.bottom() - control.bottom(),
            ]
            .into_iter()
            .fold(Pixels::MAX, Pixels::min)
        }
        fn assert_room(what: &str, kept: Pixels, room: Pixels) {
            assert!(
                (kept - room).abs() < px(0.01),
                "{what} keeps {kept:?} of room, not the installed ring's {room:?}"
            );
        }
        fn shown(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
            settings::shown(cx, selector).unwrap_or_else(|| panic!("{selector} is rendered"))
        }
        fn seed(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, count: u32) {
            cx.update(|_, cx| {
                app.update(cx, |app, cx| {
                    let themes = (1..=count)
                        .map(|n| {
                            let base = ThemeChoice::ALL[(n as usize - 1) % ThemeChoice::ALL.len()];
                            CustomTheme::from_base(n, format!("Seed {n:02}"), base)
                        })
                        .collect();
                    app.set_custom_themes(themes, cx);
                    cx.notify();
                })
            });
            draw(cx);
        }
        fn scroll_rows(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, y: Pixels) {
            cx.update(|_, cx| {
                app.update(cx, |app, cx| {
                    app.theme_editor
                        .rows_scroll()
                        .0
                        .borrow()
                        .base_handle
                        .set_offset(point(Pixels::ZERO, y));
                    app.notify_settings_page(cx);
                })
            });
            draw(cx);
        }
        fn scroll_page(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, y: Pixels) {
            cx.update(|_, cx| {
                app.update(cx, |app, cx| {
                    app.theme_editor
                        .page_scroll()
                        .set_offset(point(Pixels::ZERO, y));
                    app.notify_settings_page(cx);
                })
            });
            draw(cx);
        }

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let initial = repo.path().to_owned();
        let destination = isolated_session("isolated-session.json");
        crate::views::tests::init_app(cx);
        cx.update(|cx| {
            // A dialog otherwise slides in on a wall-clock animation.
            cx.set_reduce_motion(true);
        });
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(initial),
                    Preferences::default(),
                    Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1000.), px(680.)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.apply_appearance(window, cx)));
        settle_tab_test(&app, cx).await;

        // A ring of another size than the palette's: 1 px wide, 1 px out.
        // Smaller, so the window's own edge, 3.5 px above the strip's tabs,
        // does not clip before the tab list does.
        let installed = cx.read(|cx| Theme::global(cx).button_focus_ring);
        assert_eq!(installed, appearance::BUTTON_FOCUS_RING);
        let ring = FocusRing {
            width: px(1.),
            gap: px(1.),
            ..installed
        };
        install_ring(cx, &app, ring);
        let room = cx.read(appearance::button_ring_room);
        assert_eq!(room, px(2.), "the helper follows the installed ring");

        // The repository tab list. The window's deferred open adds the tab
        // before any read, and the settles above ran it, so the frame
        // `install_ring` drew already holds the tab: nothing to wait for.
        let tab = rendered(cx, "repository-tab-0");
        assert_room(
            "the tab list",
            room_kept(content_mask(cx, "repository-tab-0"), tab),
            room,
        );

        // The Tags list.
        // Tags opens once its list, read on the operation executor's own
        // thread, lands: wait for that read rather than for wall-clock time.
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        crate::tags::tests::read_lands(&app, cx).await;
        draw(cx);
        let row = rendered(cx, "tag-row-0");
        assert_room(
            "the Tags list",
            room_kept(content_mask(cx, "tag-row-0"), row),
            room,
        );
        cx.update(|window, cx| window.close_all_dialogs(cx));
        draw(cx);

        // The Reflog list.
        // The Reflog reads on a thread of its own; its browser waits for that
        // read to land rather than for wall-clock time.
        let browser =
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_reflog_browser(window, cx)));
        crate::focus_ring::sweep::Opened::finish(browser, cx);
        draw(cx);
        let entry = rendered(cx, "reflog-entry-0");
        assert_room(
            "the Reflog list",
            room_kept(content_mask(cx, "reflog-entry-0"), entry),
            room,
        );
        cx.update(|window, cx| window.close_all_dialogs(cx));
        draw(cx);

        // The Your themes list: the room above its first row and below its
        // last, over the rows' box, which the list clips at its own bounds.
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        seed(cx, &app, 4);
        let list = shown(cx, "custom-themes-rows");
        let rows_box = shown(cx, "custom-themes-rows-box");
        let first = shown(cx, "custom-theme-1");
        let last = shown(cx, "custom-theme-4");
        assert_room("the Your themes list above", first.top() - list.top(), room);
        assert_room(
            "the Your themes list below",
            list.bottom() - last.bottom(),
            room,
        );
        assert_eq!(
            (first.top(), last.bottom()),
            (rows_box.top(), rows_box.bottom()),
            "the room moves no row: the rows fill their box"
        );

        // The page reveal keeps the same room around the revealed row: with
        // the rows' box flush against the page's bottom, focusing the last
        // row's Delete… scrolls the page by the room and no further.
        let viewport = shown(cx, "settings-scroll");
        let offset = cx.read(|cx| app.read(cx).theme_editor.page_scroll().offset().y);
        scroll_page(cx, &app, offset + viewport.bottom() - rows_box.bottom());
        assert_eq!(
            shown(cx, "custom-themes-rows-box").bottom(),
            viewport.bottom(),
            "the rows' box is flush against the page's bottom"
        );
        let handle = cx.update(|_, cx| app.read(cx).theme_editor.row_focus(4, cx));
        cx.update(|window, cx| {
            theme_editor::State::focus_row_action(
                &handle,
                theme_editor::RowFocus::Action(2),
                window,
                cx,
            )
        });
        draw(cx);
        let last = shown(cx, "custom-theme-4");
        let list = shown(cx, "custom-themes-rows");
        assert_room(
            "the page reveal below the last row",
            viewport.bottom() - last.bottom(),
            room,
        );
        assert_eq!(
            list.bottom(),
            viewport.bottom(),
            "the page reveals the list's own room, and no more"
        );
        cx.update(|window, cx| window.blur(cx));
        draw(cx);

        // The strips cover exactly the room while the rows stand off a row
        // boundary, and are absent on one.
        seed(cx, &app, 32);
        scroll_page(cx, &app, Pixels::ZERO);
        scroll_rows(cx, &app, px(-10.));
        let list = shown(cx, "custom-themes-rows");
        let rows_box = shown(cx, "custom-themes-rows-box");
        let top = shown(cx, "custom-themes-ring-room-top");
        let bottom = shown(cx, "custom-themes-ring-room-bottom");
        assert_eq!(
            top,
            Bounds::new(list.origin, size(list.size.width, room)),
            "the top strip is the room above the rows"
        );
        assert_eq!(
            bottom,
            Bounds::new(
                point(list.left(), rows_box.bottom()),
                size(list.size.width, room)
            ),
            "the bottom strip is the room below them"
        );
        assert_eq!(bottom.bottom(), list.bottom());
        scroll_rows(cx, &app, Pixels::ZERO);
        assert!(
            !settings::page_shows(cx, "custom-themes-ring-room-top"),
            "on a boundary the room is the ring's"
        );

        cx.update(|window, cx| {
            Theme::global_mut(cx).button_focus_ring = installed;
            window.refresh();
        });
        settle_tab_test(&app, cx).await;
    }

    #[gpui::test]
    async fn closing_the_last_repository_tab_keeps_application_actions_reachable(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repository = crate::views::tests::fixture_repository(fixture.path().join("repository"));
        let path = repository.path().to_owned();
        let destination = isolated_session("isolated-session.json");
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, window_cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(path),
                    Preferences::default(),
                    Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        settle_tab_test(&app, window_cx).await;
        window_cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            app.update(cx, |app, cx| {
                assert_eq!(app.repository_tabs.tabs.len(), 1);
                app.focus.focus(window, cx);
                app.page_return_focus = Some(app.focus.clone());
                app.close_repository_tab(0, window, cx);
                assert!(app.page == AppPage::Projects);
                assert!(app.page_return_focus.is_none());
            });
            window.draw(cx).clear(cx);
            // Focus is on the page's selected action tab, drawn with its
            // ring, inside the window's own handlers.
            app.update(cx, |app, cx| {
                assert!(app.hub.read(cx).selected_mode_focused(window));
                assert!(app.app_focus.contains_focused(window, cx));
            });
            window.dispatch_action(Box::new(ShowSettings), cx);
        });
        window_cx.executor().run_until_parked();
        assert!(app.read_with(window_cx, |app, _| app.page == AppPage::Settings));
        settle_tab_test(&app, window_cx).await;
    }

    #[gpui::test]
    async fn new_tab_and_latest_bookmark_survive_actual_window_close_with_full_save_queue(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{
            cell::RefCell,
            rc::Rc,
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
                mpsc,
            },
        };

        cx.executor().allow_parking();
        cx.executor().set_block_on_ticks(100..=100);
        let fixture = tempfile::tempdir().unwrap();
        let first = crate::views::tests::fixture_repository(fixture.path().join("first"));
        let second = crate::views::tests::fixture_repository(fixture.path().join("new-tab"));
        let first_path = first.path().canonicalize().unwrap();
        let second_path = second.path().canonicalize().unwrap();
        let save_path = isolated_session("isolated-session.json");
        let saved = Session {
            version: 1,
            active: 0,
            tabs: vec![SavedTab {
                path: SavedPath::new(&first_path),
                bookmark: Bookmark::default(),
                unresolved: false,
                git_directories: None,
            }],
            ..Default::default()
        };
        saved.clone().save_at(&save_path).unwrap();
        crate::views::tests::init_app(cx);
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let destination = save_path.clone();
        let (root_view, window_cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(first_path),
                    Preferences::default(),
                    saved,
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        settle_tab_test(&app, window_cx).await;
        window_cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(second_path.clone(), window, cx);
            })
        });
        settle_tab_test(&app, window_cx).await;
        // Opening and accepting metadata must save both tabs even if no switch,
        // reorder, group action or later close ever occurs.
        let persisted = Session::load_at(&save_path).unwrap();
        assert_eq!(persisted.tabs.len(), 2);
        assert_eq!(persisted.tabs[persisted.active].path.path(), second_path);
        assert!(persisted.tabs[persisted.active].bookmark.query.is_empty());

        let (release, gate) = mpsc::channel();
        let (started, running) = futures::channel::oneshot::channel();
        let blocker = app.read_with(window_cx, |app, _| {
            app.preferences_writer.submit(move || {
                let _ = started.send(());
                gate.recv()?;
                Ok(())
            })
        });
        running.await.unwrap();
        let queued = app.read_with(window_cx, |app, _| {
            (0..7)
                .map(|_| app.preferences_writer.submit(|| Ok(())))
                .collect::<Vec<_>>()
        });
        let accepted = window_cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                // The accepted session snapshot occupies the eighth queue slot.
                app.queue_session_snapshot(cx)
            })
        });
        let refused = app.read_with(window_cx, |app, _| app.preferences_writer.submit(|| Ok(())));
        assert!(refused.await.unwrap().is_err());
        let search = app.read_with(window_cx, |app, _| app.search.clone());
        let weak = app.downgrade();
        drop(app);
        drop(root_view);
        window_cx.update(|window, cx| {
            // The shared widget value changes immediately before the window is
            // removed. The earlier accepted snapshot does not contain it.
            search.update(cx, |input, cx| {
                input.set_value("last query before closing", window, cx)
            });
            window.open_dialog(cx, |dialog, _, _| {
                dialog.title("Open dialog during window close")
            });
            assert!(window.has_active_dialog(cx));
            window.remove_window();
        });
        window_cx.cx.update(|cx| assert!(cx.windows().is_empty()));
        assert!(
            weak.upgrade().is_none(),
            "the window and GitTurtle view must really be released"
        );
        assert!(
            Session::load_at(&save_path).unwrap().tabs[1]
                .bookmark
                .query
                .is_empty()
        );
        // Resolve the real executor from a background task during GPUI's actual
        // shutdown wait. The removed window cannot own the final completion.
        let released = Arc::new(AtomicBool::new(false));
        let during_shutdown = released.clone();
        window_cx
            .executor()
            .spawn(async move {
                release.send(()).unwrap();
                during_shutdown.store(true, Ordering::SeqCst);
            })
            .detach();
        cx.quit();
        // Shutdown steps this task only while a quit future is still pending.
        assert!(
            released.load(Ordering::SeqCst),
            "shutdown must wait on the accepted session save"
        );
        blocker.await.unwrap().unwrap();
        for response in queued {
            response.await.unwrap().unwrap();
        }
        // GPUI stops waiting after 200 ms of real time, so slow disk I/O can
        // still be writing the coalesced close snapshot when quit returns.
        accepted.await.unwrap();
        let restored = Session::load_at(&save_path).unwrap();
        assert_eq!(restored.tabs.len(), 2);
        assert_eq!(restored.tabs[restored.active].path.path(), second_path);
        assert_eq!(
            restored.tabs[restored.active].bookmark.query,
            "last query before closing"
        );
        let restored_state = State::from_session(restored);
        assert_eq!(restored_state.active_path(), Some(second_path.as_path()));
        assert_eq!(
            restored_state.tabs[restored_state.active.unwrap()]
                .saved
                .query,
            "last query before closing"
        );
    }

    #[gpui::test]
    async fn cold_legacy_quick_source_restores_history_without_inventing_a_file_change(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repo = crate::views::tests::fixture_repository(fixture.path().join("history"));
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args([
                "-c",
                "user.name=Session Fixture",
                "-c",
                "user.email=session@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "A commit with no changed files",
            ])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let selected = repo.history(1).unwrap().remove(0);
        let source_path = PathBuf::from("lfs-corrupt.glb");
        let source = FileChange {
            old_path: None,
            new_path: Some(source_path.clone()),
            old_oid: None,
            new_oid: None,
            old_mode: "000000".into(),
            new_mode: "100644".into(),
            status: gitturtle_core::ChangeStatus::Added,
        };
        let destination = isolated_session("isolated-session.json");
        let saved = Session {
            version: 1,
            tabs: vec![SavedTab {
                path: SavedPath::new(repo.path()),
                bookmark: Bookmark {
                    selected_oid: Some(selected.oid.clone()),
                    selection: Some(SavedCommit::new(&selected)),
                    comparison: Some(SavedFile::new(&source)),
                    selected_file: Some(SavedPath::new(&source_path)),
                    compare: true,
                    ..Default::default()
                },
                unresolved: false,
                git_directories: None,
            }],
            ..Default::default()
        };
        // Reproduce a session written by the older Quick Open path, then use
        // the production loader and cold startup rather than constructing a
        // pre-sanitized in-memory bookmark.
        std::fs::write(&destination, serde_json::to_vec(&saved).unwrap()).unwrap();
        let restored = Session::load_at(&destination).unwrap();
        crate::views::tests::init_app(cx);
        let initial = repo.path().to_owned();
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, window_cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    Some(initial),
                    Preferences::default(),
                    restored,
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                app.repository_tabs.save_path = Some(destination);
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        settle_tab_test(&app, window_cx).await;
        window_cx.read(|cx| {
            let app = app.read(cx);
            assert!(app.mode == WorkspaceMode::History);
            assert_eq!(app.commits[app.selected_commit.unwrap()].oid, selected.oid);
            assert!(app.files.is_empty(), "the selected commit has no changes");
            assert!(app.selected_file.is_none());
            assert!(app.content.is_none());
            assert!(app.repository_tabs.restoring.is_none());
            let bookmark = app.tab_bookmark(cx);
            assert!(!bookmark.compare);
            assert!(bookmark.comparison.is_none());
            assert!(bookmark.selected_file.is_none());
        });
    }

    #[gpui::test]
    async fn startup_installs_root_before_restoring_six_tabs_and_respects_initial_path(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let settings = preferences::settings_path().unwrap();
        // In a directory of this test process's root, never the user's settings.
        assert!(
            settings
                .parent()
                .and_then(Path::parent)
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("gitturtle-app-tests-")
        );
        let paths: Vec<_> = (0..7)
            .map(|index| fixture.path().join(format!("repository-{index}")))
            .collect();
        for path in [&paths[3], &paths[6]] {
            let repo = crate::views::tests::fixture_repository(path);
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(repo.path())
                .args([
                    "-c",
                    "user.name=Startup Fixture",
                    "-c",
                    "user.email=startup@example.invalid",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--allow-empty",
                    "-m",
                    "Initial fixture commit",
                ])
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        crate::views::tests::init_app(cx);
        // The first initial path is the saved active repository; the second
        // represents an explicit CLI path that takes precedence over that tab.
        for initial_index in [3, 6] {
            let mut saved = Session {
                version: 1,
                active: 3,
                tabs: paths[..6]
                    .iter()
                    .map(|path| SavedTab {
                        path: SavedPath::new(path),
                        bookmark: Bookmark::default(),
                        unresolved: false,
                        git_directories: None,
                    })
                    .collect(),
                ..Default::default()
            };
            saved.tabs[3].bookmark.scope = Some((
                "Saved main scope".into(),
                SavedScope::Branch {
                    name: "main".into(),
                    remote: false,
                },
            ));
            let initial = paths[initial_index].clone();
            let expected = initial.canonicalize().unwrap();
            let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
            let observed = captured.clone();
            let (_, window_cx) = cx.add_window_view(move |window, cx| {
                let app = cx.new(|cx| {
                    let app = GitTurtle::new(
                        Some(initial),
                        Preferences::default(),
                        saved,
                        activity::State::default(),
                        recovery_drafts::State::default(),
                        window,
                        cx,
                    );
                    assert!(app.path.is_none(), "opening must wait for the actual Root");
                    app
                });
                *captured.borrow_mut() = Some(app.clone());
                Root::new(app, window, cx)
            });
            let app = observed.borrow().as_ref().unwrap().clone();
            window_cx.executor().run_until_parked();
            window_cx.update(|window, cx| {
                assert!(!window.has_active_dialog(cx));
                app.update(cx, |app, _| {
                    assert_eq!(
                        app.path.as_ref().map(|path| path.canonicalize().unwrap()),
                        Some(expected.clone())
                    );
                    assert_eq!(
                        app.repository_tabs.tabs.len(),
                        if initial_index == 3 { 6 } else { 7 }
                    );
                    assert_eq!(
                        app.scope.as_ref().map(|scope| scope.0.as_str()),
                        if initial_index == 3 {
                            Some("Saved main scope")
                        } else {
                            None
                        }
                    );
                });
            });
            // Drain actual worker replies and persistence before destroying the
            // test platform; it must not receive wakes from an old executor.
            for _ in 0..8 {
                let tasks = window_cx.update(|_, cx| {
                    app.update(cx, |app, _| {
                        [
                            app.task.take(),
                            app.status_task.take(),
                            app.integration_task.take(),
                            app.repository_tabs.save_pending.take(),
                        ]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                    })
                });
                if tasks.is_empty() {
                    break;
                }
                for task in tasks {
                    task.await;
                }
                window_cx.executor().run_until_parked();
            }
            window_cx.update(|_, cx| {
                app.update(cx, |app, _| {
                    assert_eq!(
                        app.repository
                            .as_ref()
                            .map(|repo| repo.path().canonicalize().unwrap()),
                        Some(expected.clone())
                    );
                    app.automatic.reset();
                    app._display_preferences_task = None;
                })
            });
            let (operations, preferences, worker) = app.read_with(window_cx, |app, _| {
                (
                    app.operations.submit_read(|| Ok(())),
                    app.preferences_writer.submit_read(|| Ok(())),
                    app.worker.submit(Job::ReleaseHistory),
                )
            });
            operations.await.unwrap().unwrap();
            preferences.await.unwrap().unwrap();
            worker.await.unwrap().unwrap();
            window_cx.executor().run_until_parked();
        }
    }

    #[gpui::test]
    fn shared_inputs_start_empty_for_new_tabs_and_restore_independent_warm_queries(
        cx: &mut TestAppContext,
    ) {
        struct Probe([Entity<InputState>; 6]);
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                Input::new(&self.0[0])
            }
        }
        cx.update(gpui_kit::init);
        let (probe, cx) = cx.add_window_view(|window, cx| {
            Probe(std::array::from_fn(|_| {
                cx.new(|cx| InputState::new(window, cx))
            }))
        });
        cx.update(|window, cx| {
            let inputs = probe.read(cx).0.clone();
            for (input, value) in inputs.iter().zip([
                "change 11000",
                "feature/",
                "src/",
                "draft-branch",
                "origin",
                "main",
            ]) {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
            let first_tab = TabInputs::capture(inputs.each_ref(), window, cx);
            TabInputs::default().restore(inputs.each_ref(), window, cx);
            assert!(inputs.iter().all(|input| input.read(cx).value().is_empty()));

            // Editing the new repository must not change the retained first tab.
            inputs[0].update(cx, |input, cx| {
                input.set_value("new repository query", window, cx)
            });
            let second_tab = TabInputs::capture(inputs.each_ref(), window, cx);
            first_tab.restore(inputs.each_ref(), window, cx);
            assert_eq!(inputs[0].read(cx).value().as_ref(), "change 11000");
            assert_eq!(inputs[1].read(cx).value().as_ref(), "feature/");
            let first_tab = TabInputs::capture(inputs.each_ref(), window, cx);
            second_tab.restore(inputs.each_ref(), window, cx);
            assert_eq!(inputs[0].read(cx).value().as_ref(), "new repository query");
            assert!(
                inputs[1..]
                    .iter()
                    .all(|input| input.read(cx).value().is_empty())
            );
            first_tab.restore(inputs.each_ref(), window, cx);
            assert_eq!(inputs[0].read(cx).value().as_ref(), "change 11000");
        });
    }

    #[test]
    fn workspace_text_exposes_full_repository_and_group_description() {
        let description = "Pinned repository /projects/one/same-name, workspace Related";
        let text = workspace_text("repository-description", description).truncate();
        let mut node = gpui::accesskit::Node::new(text.a11y_role().expect("named text role"));
        text.write_a11y_info(&mut node);
        // A label is named by its value.
        assert_eq!(node.role(), Role::Label);
        assert_eq!(node.value(), Some(description));
        let other = workspace_text("other-repository", "/projects/two/same-name");
        let mut other_node = gpui::accesskit::Node::new(other.a11y_role().unwrap());
        other.write_a11y_info(&mut other_node);
        assert_ne!(node.value(), other_node.value());
    }

    /// A workspace name Local workspaces refuses is an alert announced at
    /// once, as a whole, as every form's refusal is: it was an alert with no
    /// live region, which an assistive technology is never asked to
    /// announce, so a screen reader heard nothing when Create workspace
    /// refused an empty name.
    #[gpui::test]
    async fn a_refused_workspace_name_is_announced_at_once(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = views::tests::history_window(cx).await;
        // The dialog otherwise slides in on the wall clock, and under load
        // its button moved between the press and the release: no click.
        cx.update(|_, cx| cx.set_reduce_motion(true));
        crate::focus_ring::sweep::open_dialog(&app, cx, "Local workspaces", |app, window, cx| {
            app.open_repository_library(window, cx)
        })
        .await;
        assert!(cx.debug_bounds("local-workspaces-error").is_none());
        native_accessibility::written::forget();
        crate::copy_feedback::tests::click(cx, "create-local-workspace");
        views::tests::settle(&app, cx).await;
        assert!(
            cx.debug_bounds("local-workspaces-error").is_some(),
            "an empty name is refused in the dialog"
        );
        assert_eq!(
            native_accessibility::written::announced_from("local-workspaces-error"),
            vec![(
                "local-workspaces-error-1".to_owned(),
                native_accessibility::written::Announced::ASSERTIVE
            )]
        );
        cx.update(|window, cx| window.close_dialog(cx));
        views::tests::draw(cx);
    }

    fn session(paths: &[&str], active: usize) -> Session {
        Session {
            version: 1,
            tabs: paths
                .iter()
                .map(|path| SavedTab {
                    path: SavedPath::Text((*path).into()),
                    bookmark: Bookmark::default(),
                    unresolved: false,
                    git_directories: None,
                })
                .collect(),
            active,
            ..Default::default()
        }
    }
    fn commit(oid: &str) -> Commit {
        Commit {
            oid: oid.into(),
            parents: vec!["first-parent".into(), "second-parent".into()],
            author: "Author\t名".into(),
            timestamp: -42,
            subject: "Subject\ncontinued".into(),
            body: "Body\nwith literal metadata".into(),
        }
    }
    #[test]
    fn reorder_and_roundtrip_keep_active_repository_and_independent_bookmarks() {
        let mut saved = session(&["/fixture/one", "/fixture/two", "/fixture/three"], 1);
        saved.tabs[0].bookmark.query = "one query".into();
        saved.tabs[1].bookmark.query = "two query".into();
        saved.tabs[1].bookmark.parent = 1;
        let mut tabs = State::from_session(saved);
        tabs.reorder(1, 0);
        tabs.reorder(2, 1);
        assert_eq!(tabs.active_path(), Some(Path::new("/fixture/two")));
        let output: Session =
            serde_json::from_slice(&serde_json::to_vec(&tabs.saved()).unwrap()).unwrap();
        assert_eq!(output.active, 0);
        assert_eq!(output.tabs[0].bookmark.query, "two query");
        assert_eq!(output.tabs[0].bookmark.parent, 1);
        assert_eq!(output.tabs[2].bookmark.query, "one query");
        assert!(tabs.tabs.iter().all(|tab| tab.warm.is_none()));
    }
    #[test]
    fn normalization_preserves_active_identity_when_invalid_and_duplicate_entries_are_removed() {
        let mut saved = session(
            &["relative", "/fixture/one", "/fixture/one", "/fixture/two"],
            3,
        );
        saved.tabs[3].bookmark.query = "界".repeat(2000);
        saved.tabs[3].bookmark.history_y = f32::NAN;
        saved.tabs[3].bookmark.zoom = -1.;
        saved.tabs[3].bookmark.inspector_width = Some(f32::INFINITY);
        saved.normalize();
        assert_eq!(saved.tabs.len(), 2);
        assert_eq!(saved.active, 1);
        let bookmark = &saved.tabs[1].bookmark;
        assert!(bookmark.query.len() <= 4096);
        assert!(bookmark.query.chars().all(|character| character == '界'));
        assert_eq!(bookmark.history_y, 0.);
        assert_eq!(bookmark.zoom, 1.);
        assert_eq!(bookmark.inspector_width, None);
    }
    #[test]
    fn loading_session_is_bounded_and_keeps_unavailable_repositories_for_recovery() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("session.json");
        let unavailable = fixture.path().join("removed-worktree");
        let mut saved = session(&[], 0);
        saved.tabs.push(SavedTab {
            path: SavedPath::new(&unavailable),
            bookmark: Bookmark::default(),
            unresolved: false,
            git_directories: None,
        });
        std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        let loaded = Session::load_at(&path).unwrap();
        assert_eq!(loaded.tabs[0].path.path(), unavailable);
        assert!(!unavailable.exists());
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_SESSION_BYTES as u64 + 1).unwrap();
        assert!(
            Session::load_at(&path)
                .unwrap_err()
                .to_string()
                .contains("exceeds")
        );
        std::fs::write(&path, br#"{"version":999}"#).unwrap();
        assert!(
            Session::load_at(&path)
                .unwrap_err()
                .to_string()
                .contains("version")
        );
    }
    #[test]
    fn saving_a_session_preserves_corrupt_and_unsupported_restart_data() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("session.json");
        for original in [b"{truncated".as_slice(), br#"{"version":999}"#] {
            std::fs::write(&path, original).unwrap();
            // Startup may show an empty/default view, but a later navigation
            // or quit save cannot replace the original recovery material.
            let recovered = Session::load_at(&path).unwrap_or_default();
            assert!(recovered.save_at(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
        std::fs::remove_file(&path).unwrap();
        session(&["/fixture/one"], 0).save_at(&path).unwrap();
        session(&["/fixture/two"], 0).save_at(&path).unwrap();
        assert_eq!(
            Session::load_at(&path).unwrap().tabs[0].path.path(),
            Path::new("/fixture/two")
        );
    }
    #[test]
    fn library_pins_and_groups_survive_closing_tabs_without_becoming_open_readers() {
        let mut saved = session(&["/fixture/one", "/fixture/two"], 0);
        saved.groups = vec!["Related projects".into()];
        saved.library = vec![LibraryEntry {
            path: SavedPath::Text("/fixture/one".into()),
            pinned: true,
            group: Some("Related projects".into()),
        }];
        let mut tabs = State::from_session(saved);
        tabs.tabs.clear();
        tabs.active = None;
        let mut restored: Session =
            serde_json::from_slice(&serde_json::to_vec(&tabs.saved()).unwrap()).unwrap();
        restored.normalize();
        assert!(restored.tabs.is_empty());
        assert_eq!(restored.groups, ["Related projects"]);
        assert!(restored.library[0].pinned);
        assert_eq!(
            restored.library[0].group.as_deref(),
            Some("Related projects")
        );
    }
    #[test]
    fn normalization_bounds_open_tabs_library_and_group_memberships() {
        let mut saved = session(&[], 0);
        saved.tabs = (0..12)
            .map(|index| SavedTab {
                path: SavedPath::Text(format!("/fixture/{index}")),
                bookmark: Bookmark::default(),
                unresolved: false,
                git_directories: None,
            })
            .collect();
        saved.active = 11;
        saved.groups = (0..20).map(|index| format!("group-{index}")).collect();
        saved.groups.push("invalid\nname".into());
        saved.library = (0..150)
            .map(|index| LibraryEntry {
                path: SavedPath::Text(format!("/fixture/{index}")),
                pinned: true,
                group: Some("group-19".into()),
            })
            .collect();
        saved.normalize();
        assert_eq!(saved.tabs.len(), MAX_TABS);
        assert_eq!(saved.active, 0);
        assert_eq!(saved.groups.len(), MAX_GROUPS);
        assert_eq!(saved.library.len(), MAX_LIBRARY);
        assert!(saved.library.iter().all(|entry| entry.group.is_none()));
    }
    #[test]
    fn selected_identity_outside_window_is_retained_once_without_replacing_visible_commits() {
        let target = commit("captured-selection");
        let saved = Bookmark {
            selected_oid: Some(target.oid.clone()),
            selection: Some(SavedCommit::new(&target)),
            parent: 1,
            ..Default::default()
        };
        let mut commits = vec![commit("visible-first"), commit("visible-second")];
        let mut graph = vec![graph::GraphRow::default(); 2];
        let mut retained = None;
        assert_eq!(
            restore_selected_identity(&saved, &mut commits, &mut graph, &mut retained),
            Some(2)
        );
        assert_eq!(retained, Some(2));
        assert_eq!(commits[0].oid, "visible-first");
        assert_eq!(commits[2].parents[1], "second-parent");
        assert_eq!(
            restore_selected_identity(&saved, &mut commits, &mut graph, &mut retained),
            Some(2)
        );
        assert_eq!(commits.len(), 3);
        assert_eq!(graph.len(), 3);
        let mismatched = Bookmark {
            selected_oid: Some("different".into()),
            ..saved
        };
        assert_eq!(
            restore_selected_identity(&mismatched, &mut commits, &mut graph, &mut retained),
            None
        );
        assert_eq!(commits.len(), 3);
    }
    #[test]
    fn captured_comparison_roundtrip_preserves_each_blob_mode_parent_and_document_choice() {
        let old = PathBuf::from("before name.pdf");
        let new = PathBuf::from("after name.pdf");
        let file = FileChange {
            old_path: Some(old),
            new_path: Some(new),
            old_oid: Some("before-blob".into()),
            new_oid: Some("after-blob".into()),
            old_mode: "100644".into(),
            new_mode: "100755".into(),
            status: gitturtle_core::ChangeStatus::Renamed,
        };
        let saved = Bookmark {
            selected_oid: Some("merge".into()),
            selection: Some(SavedCommit::new(&commit("merge"))),
            comparison: Some(SavedFile::new(&file)),
            origins: [Some("parent-2".into()), Some("merge".into())],
            model: [
                Some(model_view::Bookmark {
                    target: [1., 2., 3.],
                    yaw: -std::f64::consts::FRAC_PI_4,
                    pitch: 0.6,
                    span: 42.,
                    linked: false,
                    wireframe: true,
                }),
                Some(model_view::Bookmark {
                    target: [-1., -2., -3.],
                    yaw: 1.2,
                    pitch: -0.2,
                    span: 13.,
                    linked: false,
                    wireframe: false,
                }),
            ],
            parent: 1,
            zoom: 2.5,
            text_mode: "markdown".into(),
            compare: true,
            ..Default::default()
        };
        let mut session = session(&["/fixture"], 0);
        session.tabs[0].bookmark = saved.clone();
        let mut restored: Session =
            serde_json::from_slice(&serde_json::to_vec(&session).unwrap()).unwrap();
        restored.normalize();
        let restored = &restored.tabs[0].bookmark;
        assert_eq!(restored.comparison.as_ref().unwrap().file(), file);
        assert_eq!(
            restored.selection.as_ref().unwrap().commit(),
            commit("merge")
        );
        assert_eq!(restored.origins, saved.origins);
        assert_eq!(restored.parent, 1);
        assert_eq!(restored.zoom, 2.5);
        assert_eq!(restored.text_mode, "markdown");
        assert!(restored.compare);
        assert_eq!(
            serde_json::to_value(&restored.model).unwrap(),
            serde_json::to_value(&saved.model).unwrap()
        );
    }
    #[test]
    fn captured_added_and_deleted_comparisons_keep_their_absent_side_on_restart() {
        for added in [false, true] {
            let file = FileChange {
                old_path: (!added).then(|| PathBuf::from("model.glb")),
                new_path: added.then(|| PathBuf::from("model.glb")),
                old_oid: (!added).then(|| "deleted-blob".into()),
                new_oid: added.then(|| "added-blob".into()),
                old_mode: if added { "000000" } else { "100644" }.into(),
                new_mode: if added { "100644" } else { "000000" }.into(),
                status: if added {
                    gitturtle_core::ChangeStatus::Added
                } else {
                    gitturtle_core::ChangeStatus::Deleted
                },
            };
            let mut saved = session(&["/fixture"], 0);
            saved.tabs[0].bookmark = Bookmark {
                comparison: Some(SavedFile::new(&file)),
                compare: true,
                ..Default::default()
            };
            let mut restored: Session =
                serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
            restored.normalize();
            assert!(restored.tabs[0].bookmark.compare);
            assert_eq!(
                restored.tabs[0]
                    .bookmark
                    .comparison
                    .as_ref()
                    .unwrap()
                    .file(),
                file
            );
        }
    }
    #[test]
    #[cfg(unix)]
    fn session_preserves_non_utf8_repository_and_comparison_paths() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(
            b"/fixture/byte-\xff\n\t".to_vec(),
        ));
        let file = FileChange {
            old_path: Some(path.clone()),
            new_path: None,
            old_oid: Some("deleted".into()),
            new_oid: None,
            old_mode: "120000".into(),
            new_mode: "000000".into(),
            status: gitturtle_core::ChangeStatus::Deleted,
        };
        let mut saved = session(&[], 0);
        saved.tabs.push(SavedTab {
            path: SavedPath::new(&path),
            bookmark: Bookmark {
                comparison: Some(SavedFile::new(&file)),
                ..Default::default()
            },
            unresolved: false,
            git_directories: None,
        });
        let mut restored: Session =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        restored.normalize();
        assert_eq!(restored.tabs[0].path.path(), path);
        assert_eq!(
            restored.tabs[0]
                .bookmark
                .comparison
                .as_ref()
                .unwrap()
                .file(),
            file
        );
    }

    /// The places a comparison restored with a tab is saved with, here its
    /// zoom and a PDF page, belong to that tab. Left while the comparison's
    /// read runs, the tab keeps them in its bookmark, as a session saved
    /// then does, and in its warm state, and as it returns it reads the
    /// comparison again, whose reply restores them. The window keeps none:
    /// the tab shown in its place, in Compare at a zoom of its own, keeps
    /// that zoom as the window returns from Settings or Projects, and so
    /// does the tab shown when the restoring one is closed. The window kept
    /// them, so the next tab took up the left one's zoom on a return from
    /// Settings, and the left tab's bookmark saved the window's zoom and no
    /// PDF page, which it came back without.
    #[gpui::test]
    async fn a_tab_left_mid_restore_keeps_its_comparisons_places(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        let second = tempfile::tempdir().unwrap();
        let repo = crate::views::tests::fixture_repository(second.path().join("second"));
        std::fs::write(repo.path().join("notes.txt"), "second notes\n").unwrap();
        crate::tags::tests::git(repo.path(), &["add", "notes.txt"]);
        crate::tags::tests::git(repo.path(), &["commit", "--quiet", "-m", "second notes"]);
        let places = |saved: &Bookmark| {
            serde_json::to_value((saved.zoom, &saved.pdf, &saved.model, &saved.markdown)).unwrap()
        };
        let show_page = |page: AppPage,
                         app: &mut GitTurtle,
                         window: &mut Window,
                         cx: &mut Context<GitTurtle>| {
            match page {
                AppPage::Settings => app.show_settings(window, cx),
                _ => app.show_projects(window, cx),
            }
            assert!(app.page == page);
        };
        // The first tab shows its file in Compare, at a zoom of its own.
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle_tab_test(&app, cx).await;
        let zoom = 1.5;
        app.update(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Compare && app.content.is_some());
            app.zoom = zoom;
        });
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle_tab_test(&app, cx).await;
        let saved = app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.active, Some(1));
            let commit = &app.commits[app.selected_commit.expect("a selected commit")];
            let file = app.files.first().expect("the second tab's file");
            Bookmark {
                selected_oid: Some(commit.oid.clone()),
                selection: Some(SavedCommit::new(commit)),
                comparison: Some(SavedFile::new(file)),
                origins: [commit.parents.first().cloned(), Some(commit.oid.clone())],
                selected_file: Some(SavedPath::new(file.path())),
                pdf: [
                    None,
                    Some(
                        serde_json::from_value(
                            serde_json::json!({"page": 3, "zoom": 1.25, "linked": false}),
                        )
                        .unwrap(),
                    ),
                ],
                zoom: 2.5,
                compare: true,
                ..Default::default()
            }
        });
        // The second tab restores its comparison, whose read the reader
        // holds, idle first so nothing it ran before counts as running.
        let restore = |app: &mut GitTurtle, window: &mut Window, cx: &mut Context<GitTurtle>| {
            app.clear_preview();
            app.repository_tabs.restoring = Some(saved.clone());
            app.restore_tab_selection(window, cx);
            assert_eq!(app.loading, Some(RESTORED_PREVIEW_LABEL));
        };
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                restore(app, window, cx);
                assert_eq!(
                    places(&app.tab_bookmark(cx)),
                    places(&saved),
                    "a session saved mid-restore keeps the comparison's places"
                );
                app.switch_repository_tab(0, window, cx);
                assert!(
                    app.repository_tabs.document_restore.is_none(),
                    "the left tab's places leave the window with it"
                );
                assert_eq!(app.zoom, zoom, "the first tab shows at its own zoom");
                let left = &app.repository_tabs.tabs[1];
                assert_eq!(
                    places(&left.saved),
                    places(&saved),
                    "its bookmark keeps them"
                );
                assert!(
                    left.warm
                        .as_ref()
                        .is_some_and(|warm| warm.document_restore.is_some()),
                    "its warm state keeps them"
                );
            })
        });
        drop(held);
        settle_tab_test(&app, cx).await;
        for page in [AppPage::Settings, AppPage::Projects] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    show_page(page, app, window, cx);
                    app.return_from_page(window, cx);
                })
            });
            settle_tab_test(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert!(app.page == AppPage::Repository && app.repository_tabs.active == Some(0));
                assert_eq!(
                    app.zoom, zoom,
                    "the first tab keeps its zoom on the return from {page:?}"
                );
            });
        }
        // Shown again, the left tab reads its comparison again and restores
        // the places it was saved with.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.switch_repository_tab(1, window, cx);
                assert_eq!(
                    app.loading,
                    Some(RESTORED_PREVIEW_LABEL),
                    "the comparison is read again"
                );
            })
        });
        settle_tab_test(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Compare && app.content.is_some());
            assert_eq!(app.zoom, 2.5, "the tab comes back at its saved zoom");
            assert!(app.repository_tabs.document_restore.is_none());
        });
        // Closed while its read runs, the tab takes its places with it.
        let held = app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                restore(app, window, cx);
                app.close_repository_tab(1, window, cx);
                assert_eq!(app.repository_tabs.active, Some(0));
                assert!(app.repository_tabs.document_restore.is_none());
                assert_eq!(app.zoom, zoom);
            })
        });
        drop(held);
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                show_page(AppPage::Settings, app, window, cx);
                app.return_from_page(window, cx);
                assert_eq!(app.zoom, zoom, "the first tab keeps its zoom");
            })
        });
    }

    /// Where a cold tab's restore stands as it is left: its snapshot read,
    /// the changed files of its saved selection, or its saved comparison.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum ColdStage {
        History,
        Changes,
        Comparison,
    }

    /// The bookmark a cold tab is restored from belongs to that tab. Left
    /// at any stage of its restore for a warm tab, or closed, it leaves the
    /// window none: the tab shown reveals its selection as History's
    /// viewport changes and keeps its own zoom, widths, text mode and
    /// selection through its Refresh and a commit selection, reading
    /// nothing of the left tab's. The left tab, left before its comparison
    /// is read, stays cold with its bookmark, so it comes back with its
    /// selection, comparison, zoom and widths. Left in the window, the
    /// bookmark stopped the next tab's reveal, its Refresh took up the left
    /// tab's zoom, widths and comparison, read in the wrong repository, and
    /// the left tab came back warm without its selection; left before its
    /// snapshot landed, that snapshot closed the tab shown.
    #[gpui::test]
    async fn a_cold_tab_left_mid_restore_takes_its_bookmark_with_it(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window_from(
            cx,
            crate::views::tests::history_script(60, 1),
        )
        .await;
        let second = tempfile::tempdir().unwrap();
        let repo = crate::views::tests::fixture_repository(second.path().join("second"));
        std::fs::write(repo.path().join("second.txt"), "second notes\n").unwrap();
        crate::tags::tests::git(repo.path(), &["add", "second.txt"]);
        crate::tags::tests::git(repo.path(), &["commit", "--quiet", "-m", "second notes"]);
        let first_path = app.read_with(cx, |app, _| app.path.clone().unwrap());
        let json = |saved: &Bookmark| serde_json::to_value(saved).unwrap();
        // The second tab shows its file in Compare, at its own zoom, text
        // mode and widths, and is left for the first.
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle_tab_test(&app, cx).await;
        let second_oid = app.update(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Compare && app.content.is_some());
            app.zoom = 2.5;
            app.text_mode = TextMode::Split;
            app.settings.navigation_width = 310.;
            app.settings.inspector_width = 430.;
            app.commits[0].oid.clone()
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        // The first tab selects its oldest commit, below History's viewport
        // once scrolled to the top, at a zoom and widths of its own.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.zoom = 1.5;
                app.settings.navigation_width = 250.;
                app.settings.inspector_width = 380.;
                app.select_commit(59, window, cx);
            })
        });
        settle_tab_test(&app, cx).await;
        let (compared, first_oid) = app.read_with(cx, |app, _| {
            let mut saved = app.repository_tabs.tabs[1].saved.clone();
            saved.pdf[1] = Some(
                serde_json::from_value(
                    serde_json::json!({"page": 3, "zoom": 1.25, "linked": false}),
                )
                .unwrap(),
            );
            (saved, app.commits[59].oid.clone())
        });
        assert!(compared.comparison.is_some() && compared.zoom == 2.5);
        let mut selected = compared.clone();
        selected.comparison = None;
        selected.compare = false;
        let first_tab = |app: &GitTurtle, tabs: usize, when: &str| {
            assert_eq!(
                app.repository_tabs.tabs.len(),
                tabs,
                "{when}: no tab closes"
            );
            assert_eq!(app.repository_tabs.active, Some(0), "{when}");
            assert_eq!(app.path.as_ref(), Some(&first_path), "{when}");
            assert!(
                app.repository_tabs.restoring.is_none(),
                "{when}: no bookmark waits"
            );
            assert!(app.repository_tabs.document_restore.is_none(), "{when}");
            assert!(app.mode == WorkspaceMode::History, "{when}: History stays");
            assert_eq!(app.zoom, 1.5, "{when}: its own zoom");
            assert!(app.text_mode == TextMode::Unified, "{when}: its text mode");
            assert_eq!(
                (app.settings.navigation_width, app.settings.inspector_width),
                (250., 380.),
                "{when}: its own widths"
            );
            assert!(
                app.files
                    .iter()
                    .all(|file| file.path() != Path::new("second.txt")),
                "{when}: nothing of the left tab's is read"
            );
        };
        let selected_oid = |app: &GitTurtle| {
            app.selected_commit
                .and_then(|index| app.commits.get(index))
                .map(|commit| commit.oid.clone())
        };
        let shows_selection = |app: &GitTurtle| {
            let scroll = app.history_scroll.0.borrow();
            let offset = -scroll.base_handle.offset().y;
            let measured = scroll.last_item_size.as_ref().expect("History's geometry");
            let row = measured.contents.height / app.visible.len() as f32;
            let index = app.visible.iter().position(|&i| i == 59).unwrap() as f32;
            row * index >= offset && row * (index + 1.) <= offset + measured.item.height
        };
        let mut height = 680.;
        for (stage, saved, close) in [
            (ColdStage::History, &compared, false),
            (ColdStage::Changes, &selected, false),
            (ColdStage::Comparison, &compared, false),
            (ColdStage::Changes, &selected, true),
        ] {
            // The second tab is cold, as a launch leaves every tab but the
            // one shown, and is restored with its read held at `stage`.
            app.update(cx, |app, _| {
                let tab = &mut app.repository_tabs.tabs[1];
                tab.warm = None;
                tab.saved = saved.clone();
            });
            let mut held = app.read_with(cx, |app, _| {
                app.worker.wait_idle();
                app.worker.hold()
            });
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.switch_repository_tab(1, window, cx);
                    assert!(app.repository_tabs.restoring.is_some());
                    assert_eq!(app.loading, Some("Reading local history…"));
                })
            });
            if stage != ColdStage::History {
                drop(held);
                held = app.read_with(cx, |app, _| {
                    app.worker.wait_idle();
                    app.worker.hold()
                });
                cx.executor().run_until_parked();
                app.read_with(cx, |app, _| {
                    let label = match stage {
                        ColdStage::Changes => "Reading changed files…",
                        _ => RESTORED_PREVIEW_LABEL,
                    };
                    assert_eq!(app.loading, Some(label), "{stage:?}");
                    assert_eq!(
                        app.repository_tabs.restoring.is_some(),
                        stage == ColdStage::Changes
                    );
                });
            }
            let when = format!("{stage:?}{}", if close { ", closed" } else { "" });
            let tabs = if close { 1 } else { 2 };
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    if close {
                        app.close_repository_tab(1, window, cx);
                        first_tab(app, tabs, &when);
                        return;
                    }
                    app.switch_repository_tab(0, window, cx);
                    first_tab(app, tabs, &when);
                    let left = &app.repository_tabs.tabs[1];
                    if stage == ColdStage::Comparison {
                        assert!(
                            left.warm
                                .as_ref()
                                .is_some_and(|warm| warm.document_restore.is_some()),
                            "{when}: its comparison is read again on return"
                        );
                    } else {
                        assert!(left.warm.is_none(), "{when}: the left tab stays cold");
                        assert_eq!(json(&left.saved), json(saved), "{when}: its bookmark");
                    }
                })
            });
            drop(held);
            settle_tab_test(&app, cx).await;
            app.read_with(cx, |app, _| {
                first_tab(app, tabs, &format!("{when}, its read ended"));
                assert_eq!(selected_oid(app), Some(first_oid.clone()), "{when}");
            });
            // Scrolled away from its selection, History reveals it as its
            // viewport changes.
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                app.history_scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), px(0.)))
            });
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| assert!(!shows_selection(app), "{when}"));
            height = if height == 680. { 600. } else { 680. };
            cx.simulate_resize(size(px(1000.), px(height)));
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(shows_selection(app), "{when}: the selection is revealed")
            });
            // Refreshed, and with a commit selected, the first tab keeps
            // its own.
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh(&Refresh, window, cx)));
            settle_tab_test(&app, cx).await;
            app.read_with(cx, |app, _| {
                first_tab(app, tabs, &format!("{when}, refreshed"));
                assert_eq!(selected_oid(app), Some(first_oid.clone()), "{when}");
                assert!(app.content.is_none(), "{when}: no comparison is read");
            });
            for index in [58, 59] {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.select_commit(index, window, cx))
                });
                settle_tab_test(&app, cx).await;
            }
            app.read_with(cx, |app, _| {
                first_tab(app, tabs, &format!("{when}, a commit selected"));
                assert_eq!(selected_oid(app), Some(first_oid.clone()), "{when}");
                assert!(!app.files.is_empty(), "{when}: its own files");
            });
            if close {
                // The last tab closed mid-restore leaves no bookmark either.
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        app.repository_tabs.restoring = Some(app.tab_bookmark(cx));
                        app.close_repository_tab(0, window, cx);
                        assert!(app.repository_tabs.tabs.is_empty());
                        assert!(app.repository_tabs.restoring.is_none(), "{when}");
                    })
                });
                break;
            }
            // Shown again, the second tab comes back as it was saved. Left
            // warm while its comparison was read, it reads it once, under
            // its own label (`resume_tab_documents`), before a returning
            // tab's preview would be read again (`resume_tab_preview`).
            let previews = app.read_with(cx, |app, _| app.worker.previews());
            let comparison = stage == ColdStage::Comparison;
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.switch_repository_tab(1, window, cx);
                    if comparison {
                        assert_eq!(app.loading, Some(RESTORED_PREVIEW_LABEL), "{when}");
                        assert_eq!(app.worker.previews(), previews + 1, "{when}: one read");
                    }
                })
            });
            settle_tab_test(&app, cx).await;
            app.read_with(cx, |app, _| {
                if comparison {
                    assert_eq!(app.worker.previews(), previews + 1, "{when}: read once");
                }
                assert_eq!(app.repository_tabs.active, Some(1), "{when}");
                assert_eq!(selected_oid(app), Some(second_oid.clone()), "{when}");
                let file = app.selected_file.and_then(|index| app.files.get(index));
                assert_eq!(
                    file.map(FileChange::path),
                    Some(Path::new("second.txt")),
                    "{when}: its file"
                );
                assert_eq!(
                    app.mode == WorkspaceMode::Compare,
                    saved.compare,
                    "{when}: its comparison"
                );
                if saved.compare {
                    assert!(app.content.is_some(), "{when}: its comparison shows");
                    assert_eq!(app.zoom, 2.5, "{when}: at its zoom");
                    assert!(app.text_mode == TextMode::Split, "{when}");
                }
                assert_eq!(
                    (app.settings.navigation_width, app.settings.inspector_width),
                    (310., 430.),
                    "{when}: its widths"
                );
                assert!(app.repository_tabs.restoring.is_none(), "{when}: restored");
            });
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
            });
            settle_tab_test(&app, cx).await;
        }
    }

    /// A warm tab left while its comparison's preview is read reads it
    /// again as it is shown again: one read, of the same file, whose reply
    /// shows. Back still cancels such a read and stays in History when the
    /// reply would have landed. The switch cancelled the read, and the tab
    /// came back in Compare with its file selected, no content and nothing
    /// reading it.
    #[gpui::test]
    async fn a_tab_left_while_its_preview_is_read_reads_it_again(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        let second = tempfile::tempdir().unwrap();
        let repo = crate::views::tests::fixture_repository(second.path().join("second"));
        std::fs::write(repo.path().join("second.txt"), "second notes\n").unwrap();
        crate::tags::tests::git(repo.path(), &["add", "second.txt"]);
        crate::tags::tests::git(repo.path(), &["commit", "--quiet", "-m", "second notes"]);
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        let shown = |app: &GitTurtle| {
            app.selected_file
                .and_then(|index| app.files.get(index))
                .cloned()
        };
        let hold = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.worker.wait_idle();
                app.worker.hold()
            })
        };
        // The first tab's preview is read, held, as the tab is left.
        let held = hold(cx);
        let file = cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.select_file(0, window, cx);
                assert_eq!(app.loading, Some(PREVIEW_LABEL));
                let file = shown(app).expect("a selected file");
                app.switch_repository_tab(1, window, cx);
                file
            })
        });
        drop(held);
        settle_tab_test(&app, cx).await;
        for back in [false, true] {
            let held = hold(cx);
            let previews = app.read_with(cx, |app, _| app.worker.previews());
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.switch_repository_tab(0, window, cx);
                    assert!(app.mode == WorkspaceMode::Compare);
                    assert_eq!(shown(app).as_ref(), Some(&file), "its file stays selected");
                    assert_eq!(
                        app.loading,
                        Some(PREVIEW_LABEL),
                        "its preview is read again"
                    );
                    assert_eq!(app.worker.previews(), previews + 1, "once");
                    if back {
                        app.automatic.hold_quiet_reads(true);
                        app.back_to_history(window, cx);
                        assert_eq!(app.loading, None, "Back cancels the read");
                    }
                })
            });
            drop(held);
            settle_tab_test(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert_eq!(
                    app.worker.previews(),
                    previews + 1,
                    "nothing reads it twice"
                );
                if back {
                    assert!(
                        app.mode == WorkspaceMode::History,
                        "the reply opens nothing"
                    );
                    assert!(app.content.is_none());
                } else {
                    assert!(app.mode == WorkspaceMode::Compare);
                    assert_eq!(shown(app).as_ref(), Some(&file));
                    assert!(app.content.is_some(), "its reply shows");
                }
            });
            if !back {
                // Left again while it is read, for Back on the return.
                let held = hold(cx);
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        app.clear_preview();
                        app.load_file(0, window, cx);
                        assert_eq!(app.loading, Some(PREVIEW_LABEL));
                        app.switch_repository_tab(1, window, cx);
                    })
                });
                drop(held);
                settle_tab_test(&app, cx).await;
            }
        }
    }

    /// A warm tab left while its Quick Open working-tree source is read
    /// reads it again as it returns, as it was opened, by one tracked read
    /// and no comparison read: the file's working content shows. Read again
    /// as a comparison, the source asked for neither side's object, since
    /// its working side has none, and showed an empty file.
    #[gpui::test]
    async fn a_tab_left_while_its_source_is_read_reads_it_again(cx: &mut TestAppContext) {
        use gitturtle_core::{HistoryCancellation, PathScope};
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        // The fixture's history is imported: its file is checked out and
        // changed, for a working-tree source of its own.
        let first_path = app.read_with(cx, |app, _| app.path.clone().unwrap());
        crate::tags::tests::git(&first_path, &["reset", "--quiet", "--hard"]);
        std::fs::write(first_path.join("notes.txt"), "working notes\n").unwrap();
        let second = tempfile::tempdir().unwrap();
        let repo = crate::views::tests::fixture_repository(second.path().join("second"));
        std::fs::write(repo.path().join("second.txt"), "second notes\n").unwrap();
        crate::tags::tests::git(repo.path(), &["add", "second.txt"]);
        crate::tags::tests::git(repo.path(), &["commit", "--quiet", "-m", "second notes"]);
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        let hold = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.worker.wait_idle();
                app.worker.hold()
            })
        };
        let reads = |app: &GitTurtle| (app.worker.previews(), app.worker.tracked_previews());
        // The source is read, held, as the tab is left.
        let held = hold(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let repo = app.repository.clone().expect("a repository");
                let paths = repo
                    .search_tracked_paths(
                        &PathScope::Worktree,
                        "notes",
                        &HistoryCancellation::default(),
                    )
                    .expect("tracked paths");
                let entry = paths.entries.first().expect("the file").clone();
                app.show_tracked_file(paths.scope.clone(), entry, window, cx);
                assert_eq!(app.loading, Some("Reading tracked file…"));
                app.switch_repository_tab(1, window, cx);
            })
        });
        drop(held);
        settle_tab_test(&app, cx).await;
        let held = hold(cx);
        let (previews, tracked) = app.read_with(cx, |app, _| reads(app));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.switch_repository_tab(0, window, cx);
                assert!(app.is_quick_source(), "the source stays shown");
                assert_eq!(
                    app.loading,
                    Some("Reading tracked file…"),
                    "it is read again"
                );
                assert_eq!(reads(app), (previews, tracked + 1), "by its tracked read");
            })
        });
        drop(held);
        settle_tab_test(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(reads(app), (previews, tracked + 1), "once");
            assert!(app.mode == WorkspaceMode::Compare && app.is_quick_source());
            assert!(
                matches!(
                    app.content.as_deref(),
                    Some(Content::Text { new, .. }) if new == "working notes\n"
                ),
                "its working content shows"
            );
        });
    }

    /// A cold tab's restore ends as a read the person asks for in the tab
    /// takes the place of its own, here Quick Open while its saved
    /// selection's changed files are read, or as its own read fails: left,
    /// the tab stays warm with what it shows, its Quick Open source and its
    /// selection, and comes back with them. Left cold all the same, as while
    /// its restore still waits for its own read, it dropped its source and
    /// came back restored from its bookmark, the restore having waited for a
    /// reply that never came.
    #[gpui::test]
    async fn a_tab_whose_restore_is_replaced_or_fails_is_left_warm(cx: &mut TestAppContext) {
        use gitturtle_core::{HistoryCancellation, PathScope};
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        let second = tempfile::tempdir().unwrap();
        let repo = crate::views::tests::fixture_repository(second.path().join("second"));
        std::fs::write(repo.path().join("second.txt"), "second notes\n").unwrap();
        crate::tags::tests::git(repo.path(), &["add", "second.txt"]);
        crate::tags::tests::git(repo.path(), &["commit", "--quiet", "-m", "second notes"]);
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle_tab_test(&app, cx).await;
        let oid = app.read_with(cx, |app, _| app.commits[0].oid.clone());
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        let saved = app.read_with(cx, |app, _| app.repository_tabs.tabs[1].saved.clone());
        assert_eq!(saved.selected_oid.as_deref(), Some(oid.as_str()));
        let hold = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.worker.wait_idle();
                app.worker.hold()
            })
        };
        let selected_oid = |app: &GitTurtle| {
            app.selected_commit
                .and_then(|index| app.commits.get(index))
                .map(|commit| commit.oid.clone())
        };
        let shows_source = |app: &GitTurtle| {
            app.is_quick_source()
                && matches!(
                    app.content.as_deref(),
                    Some(Content::Text { new, .. }) if new == "second notes\n"
                )
        };
        for failed in [false, true] {
            let when = if failed { "failed" } else { "Quick Open" };
            // The second tab is cold, as a launch leaves it, and restored,
            // its saved selection's changed files read, held.
            app.update(cx, |app, _| {
                let tab = &mut app.repository_tabs.tabs[1];
                tab.warm = None;
                tab.saved = saved.clone();
            });
            let held = hold(cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            drop(held);
            let held = hold(cx);
            cx.executor().run_until_parked();
            app.read_with(cx, |app, _| {
                assert_eq!(app.loading, Some("Reading changed files…"), "{when}");
                assert!(app.repository_tabs.restoring.is_some(), "{when}");
            });
            if failed {
                // Its commit's tree is gone, so the read fails.
                let output = std::process::Command::new("git")
                    .args(["rev-parse", "HEAD^{tree}"])
                    .current_dir(repo.path())
                    .output()
                    .unwrap();
                let tree = String::from_utf8(output.stdout).unwrap();
                let tree = tree.trim();
                std::fs::remove_file(
                    repo.path()
                        .join(".git/objects")
                        .join(&tree[..2])
                        .join(&tree[2..]),
                )
                .unwrap();
                drop(held);
                app.read_with(cx, |app, _| app.worker.wait_idle());
                cx.executor().run_until_parked();
                app.read_with(cx, |app, _| assert!(app.error.is_some(), "{when}"));
            } else {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        let repo = app.repository.clone().expect("a repository");
                        let paths = repo
                            .search_tracked_paths(
                                &PathScope::Worktree,
                                "second",
                                &HistoryCancellation::default(),
                            )
                            .expect("tracked paths");
                        let entry = paths.entries.first().expect("the file").clone();
                        app.show_tracked_file(paths.scope.clone(), entry, window, cx);
                    })
                });
                drop(held);
                settle_tab_test(&app, cx).await;
                app.read_with(cx, |app, _| assert!(shows_source(app), "{when}"));
            }
            app.read_with(cx, |app, _| {
                assert!(
                    app.repository_tabs.restoring.is_none(),
                    "{when}: its restore ends"
                );
            });
            // Left, it stays warm, and comes back as it was left.
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.switch_repository_tab(0, window, cx);
                    assert!(
                        app.repository_tabs.tabs[1].warm.is_some(),
                        "{when}: left warm"
                    );
                })
            });
            settle_tab_test(&app, cx).await;
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            settle_tab_test(&app, cx).await;
            app.read_with(cx, |app, _| {
                assert!(app.repository_tabs.restoring.is_none(), "{when}");
                if failed {
                    assert!(app.mode == WorkspaceMode::History, "{when}");
                    assert_eq!(
                        selected_oid(app),
                        Some(oid.clone()),
                        "{when}: its selection"
                    );
                } else {
                    assert!(shows_source(app), "{when}: its source");
                }
            });
            if !failed {
                // Its selection waits under the source.
                cx.update(|window, cx| app.update(cx, |app, cx| app.back_to_history(window, cx)));
                settle_tab_test(&app, cx).await;
                app.read_with(cx, |app, _| {
                    assert!(app.mode == WorkspaceMode::History, "{when}");
                    assert_eq!(
                        selected_oid(app),
                        Some(oid.clone()),
                        "{when}: its selection"
                    );
                });
            }
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
            });
            settle_tab_test(&app, cx).await;
        }
    }

    /// A cold tab restored onto a merge's second parent reads the first
    /// parent's changes and then, in their place, the saved parent's: that
    /// read is the restore's own, so the restore goes on and lands its saved
    /// parent and file. Taken for a read the person asked for, it ended the
    /// restore.
    #[gpui::test]
    async fn a_restore_onto_a_merge_parent_keeps_its_bookmark(cx: &mut TestAppContext) {
        let script = "commit refs/heads/main\nmark :1\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 4\nbase\nM 100644 inline base.txt\ndata 5\nbase\n\ncommit refs/heads/side\nmark :2\ncommitter Fixture <fixture@example.invalid> 1700000001 +0000\ndata 4\nside\nfrom :1\nM 100644 inline side.txt\ndata 5\nside\n\ncommit refs/heads/main\nmark :3\ncommitter Fixture <fixture@example.invalid> 1700000002 +0000\ndata 5\nmerge\nfrom :1\nmerge :2\nM 100644 inline main.txt\ndata 5\nmain\n\ndone\n";
        let (_first, app, cx) = crate::views::tests::history_window_from(cx, script).await;
        let second = tempfile::tempdir().unwrap();
        let repo = crate::views::tests::fixture_repository(second.path().join("second"));
        std::fs::write(repo.path().join("second.txt"), "second notes\n").unwrap();
        crate::tags::tests::git(repo.path(), &["add", "second.txt"]);
        crate::tags::tests::git(repo.path(), &["commit", "--quiet", "-m", "second notes"]);
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        // The merge is selected against its second parent, its second file
        // selected.
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle_tab_test(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.change_parent(1, window, cx)));
        settle_tab_test(&app, cx).await;
        app.update(cx, |app, _| app.selected_file = Some(1));
        let shown = |app: &GitTurtle| {
            (
                app.parent,
                app.files
                    .iter()
                    .map(|file| file.path().to_owned())
                    .collect::<Vec<_>>(),
                app.selected_file,
            )
        };
        let left = app.read_with(cx, |app, _| {
            assert_eq!(app.commits[0].parents.len(), 2, "a merge");
            shown(app)
        });
        assert_eq!(
            left,
            (
                1,
                vec![PathBuf::from("main.txt"), PathBuf::from("side.txt")],
                Some(1)
            )
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        settle_tab_test(&app, cx).await;
        // Cold, it is restored, its reads held one at a time.
        app.update(cx, |app, _| app.repository_tabs.tabs[0].warm = None);
        let hold = |cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| {
                app.worker.wait_idle();
                app.worker.hold()
            })
        };
        let held = hold(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        drop(held);
        let held = hold(cx);
        cx.executor().run_until_parked();
        app.read_with(cx, |app, _| {
            assert_eq!(app.loading, Some("Reading comparison…"));
            assert!(
                app.repository_tabs.restoring.is_some(),
                "its own read goes on"
            );
        });
        drop(held);
        settle_tab_test(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.repository_tabs.restoring.is_none(), "restored");
            assert_eq!(shown(app), left, "its saved parent's changes and file");
        });
    }

    /// A second tab whose last commit changes `notes.txt` and `report.pdf`
    /// from the commit before, opened beside the first and shown.
    async fn open_compared_tab(
        app: &Entity<GitTurtle>,
        root: &Path,
        cx: &mut VisualTestContext,
    ) -> GitRepository {
        let repo = crate::views::tests::fixture_repository(root.join("second"));
        for version in ["first", "second"] {
            std::fs::write(repo.path().join("notes.txt"), format!("{version} notes\n")).unwrap();
            std::fs::write(
                repo.path().join("report.pdf"),
                format!("%PDF-1.4\n% {version}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n"),
            )
            .unwrap();
            crate::tags::tests::git(repo.path(), &["add", "notes.txt", "report.pdf"]);
            crate::tags::tests::git(repo.path(), &["commit", "--quiet", "-m", version]);
        }
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle_tab_test(app, cx).await;
        repo
    }

    /// Saves the shown second tab on `name` in its commit `commit`, in
    /// Compare or selected in History, shows the first tab and leaves the
    /// second cold with that bookmark, as a launch leaves it.
    async fn leave_tab_cold_on(
        app: &Entity<GitTurtle>,
        commit: usize,
        name: &str,
        compare: bool,
        cx: &mut VisualTestContext,
    ) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.back_to_history(window, cx);
                app.select_commit(commit, window, cx);
            })
        });
        settle_tab_test(app, cx).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                let index = app
                    .files
                    .iter()
                    .position(|file| file.path() == Path::new(name))
                    .expect("the commit changes it");
                if compare {
                    app.select_file(index, window, cx);
                } else {
                    app.selected_file = Some(index);
                }
            })
        });
        settle_tab_test(app, cx).await;
        let saved = app.read_with(cx, |app, cx| app.tab_bookmark(cx));
        assert_eq!(
            saved.comparison.is_some(),
            compare,
            "{name}: its comparison"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(app, cx).await;
        draw_tab_frames(cx);
        app.update(cx, |app, _| {
            let tab = &mut app.repository_tabs.tabs[1];
            tab.warm = None;
            tab.saved = saved;
        });
    }

    /// The control holding focus, where the last frame drew it inside the
    /// window's root, which itself draws no focus.
    fn drawn_focus(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Option<FocusHandle> {
        cx.update(|window, cx| {
            let app = app.read(cx);
            window
                .focused(cx)
                .filter(|focus| *focus != app.app_focus && app.app_focus.contains(focus, window))
        })
    }

    /// Holds the repository reader once it is idle, so its next read waits.
    fn hold_reads(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
    ) -> crate::worker::ReaderHeld {
        app.read_with(cx, |app, _| {
            app.worker.wait_idle();
            app.worker.hold()
        })
    }

    /// A cold tab whose restore lands in Compare, on a PDF or on a text
    /// diff, focuses its changed-file list as choosing the file does, and
    /// one that lands in History focuses History's commit list as History's
    /// entry does. Every frame of the restore draws the control holding
    /// focus, and a key then acts: Escape goes back to History, Up selects
    /// the commit above. Showing the tab put focus on History's list, which
    /// the restored comparison does not draw, so keys reached none of the
    /// window's shortcuts until a click on a file row.
    #[gpui::test]
    async fn a_cold_restore_focuses_what_its_page_draws(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, files) = app.read_with(cx, |app, _| (app.focus.clone(), app.file_focus.clone()));
        for (commit, name, compare) in [
            (0, "report.pdf", true),
            (0, "notes.txt", true),
            (1, "notes.txt", false),
        ] {
            let when = format!("{name} in {}", if compare { "Compare" } else { "History" });
            leave_tab_cold_on(&app, commit, name, compare, cx).await;
            // Shown, it reads its snapshot with History's list focused.
            let held = hold_reads(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert_eq!(app.loading, Some("Reading local history…"), "{when}");
            });
            assert_eq!(
                drawn_focus(&app, cx),
                Some(list.clone()),
                "{when}: its snapshot"
            );
            // Its next stage: the comparison, or the saved commit's changes.
            drop(held);
            let held = hold_reads(&app, cx);
            cx.executor().run_until_parked();
            draw_tab_frames(cx);
            let stage = if compare {
                RESTORED_PREVIEW_LABEL
            } else {
                "Reading changed files…"
            };
            let shown = if compare { &files } else { &list };
            app.read_with(cx, |app, _| assert_eq!(app.loading, Some(stage), "{when}"));
            assert_eq!(
                drawn_focus(&app, cx),
                Some(shown.clone()),
                "{when}: {stage}"
            );
            drop(held);
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository_tabs.restoring.is_none(), "{when}: restored");
                assert_eq!(app.mode == WorkspaceMode::Compare, compare, "{when}");
                let file = app.selected_file.and_then(|index| app.files.get(index));
                assert_eq!(file.map(FileChange::path), Some(Path::new(name)), "{when}");
                assert!(!compare || app.content.is_some(), "{when}: its comparison");
            });
            assert_eq!(
                drawn_focus(&app, cx),
                Some(shown.clone()),
                "{when}: restored"
            );
            if compare {
                cx.simulate_keystrokes("escape");
                draw_tab_frames(cx);
                app.read_with(cx, |app, _| {
                    assert!(
                        app.mode == WorkspaceMode::History,
                        "{when}: Escape goes back"
                    );
                });
                assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "{when}: back");
            } else {
                cx.simulate_keystrokes("up");
                settle_tab_test(&app, cx).await;
                app.read_with(cx, |app, _| {
                    assert_eq!(app.selected_commit, Some(0), "{when}: Up selects");
                });
            }
        }
    }

    /// Focus the person moves while a cold tab is restored is theirs as the
    /// restore lands in Compare: a control Compare draws too, the window's
    /// first control, in the tab strip every repository page draws, reached
    /// by the keyboard while the snapshot is read, keeps it; one Compare
    /// takes away, the navigation's filter, gives way to the changed-file
    /// list as the frame that draws Compare ends. Settings opened over the
    /// restore keeps its focus; Back returns to the changed-file list, not
    /// to History's list where showing the tab had put focus, nor to the
    /// navigation's filter, which Settings opened from it returned to.
    /// History's search shortcut pressed meanwhile opens History's search
    /// as the restore lands instead
    /// (`a_search_asked_for_while_a_tab_loads_opens_as_it_lands`).
    #[gpui::test]
    async fn a_cold_restore_keeps_focus_the_person_moved(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, files, navigation) = app.read_with(cx, |app, cx| {
            (
                app.focus.clone(),
                app.file_focus.clone(),
                app.nav_search.read(cx).focus_handle(cx),
            )
        });
        for moved in [
            "tab strip",
            "navigation",
            "settings",
            "navigation, then settings",
        ] {
            leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
            let held = hold_reads(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw_tab_frames(cx);
            // The person moves focus while the snapshot is read.
            match moved {
                // Next Control from nothing focused: the window's first.
                "tab strip" => cx.update(|window, cx| {
                    window.blur(cx);
                    window.focus_next(cx);
                }),
                "navigation" => cx.update(|window, cx| window.focus(&navigation, cx)),
                "settings" => {
                    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)))
                }
                // Settings opened from the navigation's filter returns to
                // that filter, which Compare takes away.
                _ => {
                    cx.update(|window, cx| window.focus(&navigation, cx));
                    draw_tab_frames(cx);
                    assert_eq!(drawn_focus(&app, cx), Some(navigation.clone()));
                    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
                    app.read_with(cx, |app, _| {
                        assert_eq!(app.page_return_focus, Some(navigation.clone()), "{moved}");
                    });
                }
            }
            draw_tab_frames(cx);
            let before = cx.update(|window, cx| window.focused(cx));
            match moved {
                "tab strip" => {
                    let kept = before.clone().expect("the window's first control");
                    assert!(kept != list && kept != files, "{moved}: neither list");
                    assert_eq!(drawn_focus(&app, cx), Some(kept), "{moved}: drawn");
                }
                // Drawn where History shows it, so only the frame that
                // draws Compare takes it away.
                "navigation" => assert_eq!(drawn_focus(&app, cx), Some(navigation.clone())),
                _ => assert!(before.is_some() && before != Some(files.clone()), "{moved}"),
            }
            drop(held);
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository_tabs.restoring.is_none(), "{moved}: restored");
                assert!(app.mode == WorkspaceMode::Compare && app.content.is_some());
            });
            let after = cx.update(|window, cx| window.focused(cx));
            match moved {
                "tab strip" => {
                    assert_eq!(after, before, "the person's control keeps focus");
                    assert_eq!(drawn_focus(&app, cx), before, "drawn in Compare");
                }
                "navigation" => {
                    assert_eq!(
                        drawn_focus(&app, cx),
                        Some(files.clone()),
                        "{moved}: not stranded"
                    );
                }
                _ => {
                    assert_eq!(after, before, "{moved}: the page keeps its focus");
                    cx.update(|window, cx| {
                        app.update(cx, |app, cx| app.return_from_page(window, cx))
                    });
                    draw_tab_frames(cx);
                    assert_eq!(
                        drawn_focus(&app, cx),
                        Some(files.clone()),
                        "{moved}: back from the page"
                    );
                }
            }
        }
    }

    /// History's search, asked for by its shortcut while a tab's snapshot
    /// is read, opens in that tab once it can show it, never Projects: as a
    /// cold tab's restore lands in Compare, which the search leaves for
    /// History, as one that stays in History lands its snapshot, and as a
    /// new tab's snapshot lands. Focus the person moves meanwhile keeps its
    /// place, and the search does not open. With no repository read yet,
    /// History, which the shortcut showed, was Projects, whose own control
    /// took focus, so the key acted on another page.
    #[gpui::test]
    async fn a_search_asked_for_while_a_tab_loads_opens_as_it_lands(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, files, search, navigation) = app.read_with(cx, |app, cx| {
            (
                app.focus.clone(),
                app.file_focus.clone(),
                app.search.read(cx).focus_handle(cx),
                app.nav_search.read(cx).focus_handle(cx),
            )
        });
        let find = if cfg!(target_os = "macos") {
            "cmd-f"
        } else {
            "ctrl-f"
        };
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        for (commit, compare, moved) in [(0, true, false), (1, false, false), (0, true, true)] {
            let when = format!(
                "{} restore{}",
                if compare { "Compare" } else { "History" },
                if moved { ", focus moved" } else { "" }
            );
            leave_tab_cold_on(&app, commit, "notes.txt", compare, cx).await;
            let held = hold_reads(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw_tab_frames(cx);
            assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "{when}");
            cx.simulate_keystrokes(find);
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.page == AppPage::Repository, "{when}: the tab stays");
                assert!(app.repository.is_none(), "{when}: still read");
            });
            assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "{when}: kept");
            if moved {
                cx.update(|window, cx| window.focus(&navigation, cx));
                draw_tab_frames(cx);
            }
            drop(held);
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            assert!(
                app.read_with(cx, |app, _| app.repository_tabs.restoring.is_none()),
                "{when}: restored"
            );
            if moved {
                assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Compare));
                assert_eq!(drawn_focus(&app, cx), Some(files.clone()), "{when}");
            } else {
                assert!(
                    app.read_with(cx, |app, _| app.page == AppPage::Repository
                        && app.mode == WorkspaceMode::History),
                    "{when}: History shows"
                );
                assert_eq!(drawn_focus(&app, cx), Some(search.clone()), "{when}");
            }
        }

        // A new tab, opened from the shown one.
        let third = crate::views::tests::fixture_repository(root.path().join("third"));
        std::fs::write(third.path().join("notes.txt"), "third\n").unwrap();
        crate::tags::tests::git(third.path(), &["add", "notes.txt"]);
        crate::tags::tests::git(third.path(), &["commit", "--quiet", "-m", "third"]);
        let held = hold_reads(&app, cx);
        let path = third.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        draw_tab_frames(cx);
        assert_eq!(
            app.read_with(cx, |app, _| app.loading),
            Some("Reading local history…"),
            "the new tab reads its snapshot"
        );
        let before = focused(cx);
        cx.simulate_keystrokes(find);
        draw_tab_frames(cx);
        assert!(app.read_with(cx, |app, _| app.page == AppPage::Repository));
        assert_eq!(focused(cx), before, "focus stays where the key found it");
        drop(held);
        settle_tab_test(&app, cx).await;
        draw_tab_frames(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.path.as_deref(), Some(third.path()), "the new tab");
            assert!(app.page == AppPage::Repository && app.mode == WorkspaceMode::History);
        });
        assert_eq!(drawn_focus(&app, cx), Some(search), "History's search");
    }

    /// History's search, asked for while a cold tab's snapshot is read,
    /// opens where a restore bound for Compare ends in History instead: a
    /// bookmark in Compare, as an older session saved it, whose file the
    /// saved commit no longer changes or that chose none
    /// (`restore_tab_file`), and one whose changed files' read fails, which
    /// ends the restore (`request`). Held for a Compare that never came,
    /// the search was lost, and focus stayed on History's list.
    #[gpui::test]
    async fn a_search_asked_for_while_a_compare_restore_ends_in_history_opens_there(
        cx: &mut TestAppContext,
    ) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, search) = app.read_with(cx, |app, cx| {
            (app.focus.clone(), app.search.read(cx).focus_handle(cx))
        });
        let find = if cfg!(target_os = "macos") {
            "cmd-f"
        } else {
            "ctrl-f"
        };
        // The failed read comes last: it takes the commit's tree away.
        for (when, file) in [
            ("its file gone", Some("gone.txt")),
            ("no file chosen", None),
            ("its read failed", Some("notes.txt")),
        ] {
            let failed = when == "its read failed";
            leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
            app.update(cx, |app, _| {
                let saved = &mut app.repository_tabs.tabs[1].saved;
                saved.comparison = None;
                saved.compare = true;
                saved.selected_file = file.map(|name| SavedPath::new(Path::new(name)));
            });
            let held = hold_reads(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw_tab_frames(cx);
            cx.simulate_keystrokes(find);
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository.is_none(), "{when}: still read");
                assert!(app.repository_tabs.restore_lands_in_compare(), "{when}");
            });
            assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "{when}: kept");
            drop(held);
            if failed {
                // The snapshot lands, and the saved commit's changed files
                // are read without its tree.
                let held = hold_reads(&app, cx);
                cx.executor().run_until_parked();
                app.read_with(cx, |app, _| {
                    assert_eq!(app.loading, Some("Reading changed files…"), "{when}");
                    assert!(app.repository_tabs.restoring.is_some(), "{when}");
                });
                let output = std::process::Command::new("git")
                    .args(["rev-parse", "HEAD^{tree}"])
                    .current_dir(repo.path())
                    .output()
                    .unwrap();
                let tree = String::from_utf8(output.stdout).unwrap();
                let tree = tree.trim();
                std::fs::remove_file(
                    repo.path()
                        .join(".git/objects")
                        .join(&tree[..2])
                        .join(&tree[2..]),
                )
                .unwrap();
                drop(held);
                app.read_with(cx, |app, _| app.worker.wait_idle());
                cx.executor().run_until_parked();
                app.read_with(cx, |app, _| assert!(app.error.is_some(), "{when}"));
            }
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository_tabs.restoring.is_none(), "{when}: ended");
                assert!(
                    app.page == AppPage::Repository && app.mode == WorkspaceMode::History,
                    "{when}: History shows"
                );
            });
            assert_eq!(drawn_focus(&app, cx), Some(search.clone()), "{when}");
        }
    }

    /// History's search, asked for while a cold tab's snapshot is read,
    /// opens where the restore's search for its saved query ends with no
    /// page, failed or dropped: that ends the restore in History, short of
    /// the Compare an older session's bookmark was bound for
    /// (`continue_history_search`). Held for a Compare that never came, the
    /// search was lost, and focus stayed on History's list.
    #[gpui::test]
    async fn a_search_asked_for_while_a_restores_search_fails_opens_in_history(
        cx: &mut TestAppContext,
    ) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, search) = app.read_with(cx, |app, cx| {
            (app.focus.clone(), app.search.read(cx).focus_handle(cx))
        });
        let find = if cfg!(target_os = "macos") {
            "cmd-f"
        } else {
            "ctrl-f"
        };
        leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
        let bookmark = app.read_with(cx, |app, _| app.repository_tabs.tabs[1].saved.clone());
        for (round, when) in ["its search failed", "its search dropped"]
            .into_iter()
            .enumerate()
        {
            let failed = when == "its search failed";
            if round > 0 {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
                });
                settle_tab_test(&app, cx).await;
                draw_tab_frames(cx);
            }
            app.update(cx, |app, _| {
                let tab = &mut app.repository_tabs.tabs[1];
                tab.warm = None;
                tab.saved = Bookmark {
                    comparison: None,
                    compare: true,
                    query: "second".into(),
                    // A commit the repository no longer has, the saved
                    // search's scope, fails its search.
                    pinned: failed.then(|| SavedHistoryScope::Commit("1".repeat(40))),
                    offset: 0,
                    ..bookmark.clone()
                };
            });
            let held = hold_reads(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw_tab_frames(cx);
            cx.simulate_keystrokes(find);
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository.is_none(), "{when}: still read");
                assert!(app.repository_tabs.restore_lands_in_compare(), "{when}");
            });
            assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "{when}: kept");
            // The snapshot lands, and the saved query is searched once its
            // typing pause is over, its page read while the reader is held.
            drop(held);
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
            app.read_with(cx, |app, _| {
                assert!(app.repository.is_some(), "{when}: landed");
                assert!(app.repository_tabs.restoring.is_some(), "{when}");
                assert!(app.history_search_pending(), "{when}: searched");
            });
            let held = hold_reads(&app, cx);
            cx.executor()
                .advance_clock(crate::history_search::SEARCH_DEBOUNCE);
            cx.executor().run_until_parked();
            app.read_with(cx, |app, _| {
                assert_eq!(app.loading, Some("Searching repository history…"), "{when}");
                assert!(app.repository_tabs.restoring.is_some(), "{when}");
            });
            if !failed {
                // Dropped unanswered, as a newer read drops one waiting.
                app.read_with(cx, |app, _| app.worker.cancel());
            }
            drop(held);
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
            app.read_with(cx, |app, _| {
                assert!(!app.history_search_pending(), "{when}: answered");
                assert_eq!(
                    app.history_search_empty().map(|(title, _)| title),
                    Some(if failed {
                        "Search could not complete"
                    } else {
                        "Search paused"
                    }),
                    "{when}"
                );
            });
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository_tabs.restoring.is_none(), "{when}: ended");
                assert!(
                    app.page == AppPage::Repository && app.mode == WorkspaceMode::History,
                    "{when}: History shows"
                );
            });
            assert_eq!(drawn_focus(&app, cx), Some(search.clone()), "{when}");
        }
    }

    /// History's search, asked for while a cold tab's snapshot is read,
    /// opens where a restore bound for Compare, as an older session's
    /// bookmark with no captured comparison is, finds no saved commit to
    /// select in its saved query's results or its saved page, and no saved
    /// selection: the restore ends there in History
    /// (`restore_tab_selection`). Held for a Compare that never came, the
    /// search was lost, and focus stayed on History's list.
    #[gpui::test]
    async fn a_search_asked_for_while_a_restore_finds_no_commit_opens_in_history(
        cx: &mut TestAppContext,
    ) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, search) = app.read_with(cx, |app, cx| {
            (app.focus.clone(), app.search.read(cx).focus_handle(cx))
        });
        let find = if cfg!(target_os = "macos") {
            "cmd-f"
        } else {
            "ctrl-f"
        };
        leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
        let bookmark = app.read_with(cx, |app, _| app.repository_tabs.tabs[1].saved.clone());
        let tip = bookmark.selected_oid.clone().expect("the tip is selected");
        for (round, when) in ["its query's results", "its saved page"]
            .into_iter()
            .enumerate()
        {
            let searched = round == 0;
            if round > 0 {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
                });
                settle_tab_test(&app, cx).await;
                draw_tab_frames(cx);
            }
            app.update(cx, |app, _| {
                let tab = &mut app.repository_tabs.tabs[1];
                tab.warm = None;
                tab.saved = Bookmark {
                    comparison: None,
                    compare: true,
                    selection: None,
                    // The tip, which neither the search for the older
                    // commit nor the page past the tip lists.
                    selected_oid: Some(tip.clone()),
                    query: if searched {
                        "first".into()
                    } else {
                        String::new()
                    },
                    pinned: (!searched).then(|| SavedHistoryScope::Commit(tip.clone())),
                    offset: usize::from(!searched),
                    ..bookmark.clone()
                };
            });
            let held = hold_reads(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw_tab_frames(cx);
            cx.simulate_keystrokes(find);
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository.is_none(), "{when}: still read");
                assert!(app.repository_tabs.restore_lands_in_compare(), "{when}");
            });
            assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "{when}: kept");
            drop(held);
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
            assert!(
                app.read_with(cx, |app, _| app.repository.is_some()),
                "{when}: landed"
            );
            if searched {
                // The saved query is searched once its typing pause is over.
                assert!(
                    app.read_with(cx, |app, _| app.repository_tabs.restoring.is_some()),
                    "{when}: restoring"
                );
                cx.executor()
                    .advance_clock(crate::history_search::SEARCH_DEBOUNCE);
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository_tabs.restoring.is_none(), "{when}: ended");
                assert!(!app.commits.is_empty(), "{when}: its rows are listed");
                assert_eq!(app.selected_commit, None, "{when}: no saved commit");
                assert!(
                    app.page == AppPage::Repository && app.mode == WorkspaceMode::History,
                    "{when}: History shows"
                );
            });
            assert_eq!(drawn_focus(&app, cx), Some(search.clone()), "{when}");
        }
    }

    /// A read the person starts while a cold tab's restore bound for
    /// Compare still reads, Down in History's list, ends the restore and
    /// drops History's search asked for while the snapshot was read
    /// (`end_replaced_restore`): the later action wins, and focus stays on
    /// the list, on the commit Down chose. Held, the search waited for a
    /// Compare the restore no longer reached, to open over the person's
    /// choice at some later landing.
    #[gpui::test]
    async fn a_read_the_person_starts_during_a_restore_drops_the_held_search(
        cx: &mut TestAppContext,
    ) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let list = app.read_with(cx, |app, _| app.focus.clone());
        let find = if cfg!(target_os = "macos") {
            "cmd-f"
        } else {
            "ctrl-f"
        };
        leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
        app.update(cx, |app, _| {
            let saved = &mut app.repository_tabs.tabs[1].saved;
            saved.comparison = None;
            assert!(saved.compare, "a bookmark in Compare");
        });
        let held = hold_reads(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        draw_tab_frames(cx);
        cx.simulate_keystrokes(find);
        draw_tab_frames(cx);
        assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "kept");
        drop(held);
        // The snapshot lands, and the saved commit's changed files are read
        // while the reader is held.
        let held = hold_reads(&app, cx);
        cx.executor().run_until_parked();
        draw_tab_frames(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.loading, Some("Reading changed files…"));
            assert!(app.repository_tabs.restore_lands_in_compare(), "restoring");
            assert!(app.repository_tabs.pending_search.is_some(), "held");
            assert_eq!(app.selected_commit, Some(0), "the saved commit");
        });
        assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "on the list");
        cx.simulate_keystrokes("down");
        app.read_with(cx, |app, _| {
            assert!(app.repository_tabs.restoring.is_none(), "Down ends it");
            assert!(app.repository_tabs.pending_search.is_none(), "and drops it");
        });
        drop(held);
        settle_tab_test(&app, cx).await;
        draw_tab_frames(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.selected_commit, Some(1), "the commit Down chose");
            assert!(app.page == AppPage::Repository && app.mode == WorkspaceMode::History);
        });
        assert_eq!(drawn_focus(&app, cx), Some(list), "focus stays on the list");
    }

    /// A cold tab's restore whose saved page lies past the end, as after a
    /// fetch at a shallower depth cut its saved scope's history to the
    /// snapshot's own rows, shows the newest page in its place
    /// (`receive_history_page`), and with no saved commit for that page to
    /// list ends in History there, where History's search asked for while
    /// the snapshot was read opens. So does one whose last page, asked for
    /// in that page's place, a write starting meanwhile refuses, one whose
    /// page is refused while a write runs, one whose saved query, past
    /// 4 KiB, the search refuses (`finish_tab_snapshot`),
    /// and one whose saved query a write running as its typing pause ends
    /// keeps from being searched (`continue_history_search`). Left set with
    /// no read under way, the restore never ended, and the search it held
    /// never opened.
    #[gpui::test]
    async fn a_restore_with_no_page_to_read_ends_and_opens_the_held_search(
        cx: &mut TestAppContext,
    ) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, search) = app.read_with(cx, |app, cx| {
            (app.focus.clone(), app.search.read(cx).focus_handle(cx))
        });
        let find = if cfg!(target_os = "macos") {
            "cmd-f"
        } else {
            "ctrl-f"
        };
        leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
        let bookmark = app.read_with(cx, |app, _| app.repository_tabs.tabs[1].saved.clone());
        let tip = bookmark.selected_oid.clone().expect("the tip is selected");
        for (round, when) in [
            "its page past the end",
            "its page refused",
            "its query past 4 KiB",
            "its search refused while a write runs",
            "its last page refused",
        ]
        .into_iter()
        .enumerate()
        {
            let refused = round == 1;
            let long_query = round == 2;
            let search_refused = round == 3;
            let last_refused = round == 4;
            if round > 0 {
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
                });
                settle_tab_test(&app, cx).await;
                draw_tab_frames(cx);
            }
            app.update(cx, |app, _| {
                // The snapshot reads the tab's two commits as a full page,
                // so the next one starts at the saved offset.
                app.limit = 2;
                let tab = &mut app.repository_tabs.tabs[1];
                tab.warm = None;
                tab.saved = if long_query || search_refused {
                    Bookmark {
                        comparison: None,
                        compare: true,
                        query: if long_query {
                            "x".repeat(4097)
                        } else {
                            "first".into()
                        },
                        pinned: None,
                        offset: 0,
                        ..bookmark.clone()
                    }
                } else {
                    Bookmark {
                        comparison: None,
                        compare: true,
                        // No saved commit for the page shown to list.
                        selection: None,
                        selected_oid: None,
                        pinned: Some(SavedHistoryScope::Commit(tip.clone())),
                        offset: 2,
                        ..bookmark.clone()
                    }
                };
            });
            let held = hold_reads(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw_tab_frames(cx);
            cx.simulate_keystrokes(find);
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository.is_none(), "{when}: still read");
                assert!(app.repository_tabs.restore_lands_in_compare(), "{when}");
            });
            assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "{when}: kept");
            if refused {
                // A write runs as the snapshot lands.
                app.update(cx, |app, _| app.operation_busy = Some("Committing…"));
            }
            drop(held);
            app.read_with(cx, |app, _| app.worker.wait_idle());
            // The snapshot lands, and its saved page waits for the reader.
            let page_held = last_refused.then(|| app.read_with(cx, |app, _| app.worker.hold()));
            cx.executor().run_until_parked();
            if let Some(page_held) = page_held {
                app.read_with(cx, |app, _| {
                    assert!(app.repository_tabs.restoring.is_some(), "{when}: restoring");
                    assert_eq!(app.loading, Some("Reading older history…"), "{when}");
                });
                // A write starts before the empty page at the end lands, and
                // refuses the last page the window asks for in its place:
                // the restore ends there with the snapshot's rows.
                app.update(cx, |app, _| app.operation_busy = Some("Committing…"));
                drop(page_held);
                app.read_with(cx, |app, _| app.worker.wait_idle());
                cx.executor().run_until_parked();
                app.read_with(cx, |app, _| {
                    assert!(app.repository_tabs.restoring.is_none(), "{when}: ended");
                    assert_eq!(app.loading, None, "{when}: nothing read");
                    assert_eq!(
                        (app.history_paging.offset, app.visible.len()),
                        (0, 2),
                        "{when}: the snapshot's rows"
                    );
                });
                app.update(cx, |app, _| app.operation_busy = None);
            }
            if refused {
                app.update(cx, |app, _| app.operation_busy = None);
            } else if long_query {
                assert_eq!(
                    app.read_with(cx, |app, _| app
                        .history_search_empty()
                        .map(|(title, _)| title)),
                    Some("Search could not complete"),
                    "{when}: refused"
                );
            } else if search_refused {
                // The saved query waits out its typing pause, and a write
                // runs as it ends.
                app.read_with(cx, |app, _| {
                    assert!(app.repository_tabs.restoring.is_some(), "{when}: restoring");
                    assert!(app.history_search_pending(), "{when}: its pause");
                });
                app.update(cx, |app, _| app.operation_busy = Some("Committing…"));
                cx.executor()
                    .advance_clock(crate::history_search::SEARCH_DEBOUNCE);
                cx.executor().run_until_parked();
                app.read_with(cx, |app, _| {
                    assert!(!app.history_search_pending(), "{when}: not searched");
                    assert_eq!(app.loading, None, "{when}: nothing read");
                });
                app.update(cx, |app, _| app.operation_busy = None);
            } else {
                app.read_with(cx, |app, _| app.worker.wait_idle());
                cx.executor().run_until_parked();
                // The newest page, which holds the whole history, shows in
                // the saved page's place.
                app.read_with(cx, |app, _| {
                    assert_eq!(app.history_paging.offset, 0, "{when}: the newest page");
                    assert_eq!(app.visible.len(), 2, "{when}: its rows");
                });
            }
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository.is_some(), "{when}: landed");
                assert!(app.repository_tabs.restoring.is_none(), "{when}: ended");
                assert!(
                    app.page == AppPage::Repository && app.mode == WorkspaceMode::History,
                    "{when}: History shows"
                );
            });
            assert_eq!(drawn_focus(&app, cx), Some(search.clone()), "{when}");
        }
    }

    /// A cold tab's restore whose saved page is not read, refused as a write
    /// runs when its snapshot lands or failed as its saved scope's commit is
    /// gone, leaves History's paging on the history the snapshot shows
    /// (`request_history_page_in`), and Older then continues that history.
    /// The saved scope, pinned as the page was asked for, stayed under the
    /// snapshot's rows: Older read that scope's next page there, skipping
    /// the commit between the two tips, or failed on every press where the
    /// saved scope's commit was gone. Read, the page replaces the snapshot's
    /// rows whatever its offset, since it lists another scope.
    #[gpui::test]
    async fn a_restores_unread_page_leaves_paging_on_the_shown_history(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        let root = tempfile::tempdir().unwrap();
        let repo = linear_history_fixture(root.path().join("history"), 3);
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle_tab_test(&app, cx).await;
        let listed = |app: &GitTurtle| {
            app.commits
                .iter()
                .map(|commit| commit.oid.clone())
                .collect::<Vec<_>>()
        };
        let (history, bookmark) = app.read_with(cx, |app, cx| (listed(app), app.tab_bookmark(cx)));
        assert_eq!(history.len(), 3, "the tab lists its history");
        for when in ["its page refused", "its page failed", "its page landed"] {
            let refused = when == "its page refused";
            let landed = when == "its page landed";
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
            });
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            if landed {
                // A quiet read after the landing would replace rows at the
                // top of History with the snapshot's own.
                crate::views::tests::hold_quiet_reads_for_test(&app, cx);
            }
            app.update(cx, |app, _| {
                // The snapshot reads one row, which leaves Older a page.
                app.limit = 1;
                let tab = &mut app.repository_tabs.tabs[1];
                tab.warm = None;
                tab.saved = Bookmark {
                    comparison: None,
                    compare: false,
                    selection: None,
                    selected_oid: None,
                    selected_file: None,
                    query: String::new(),
                    // A page into the history saved while the middle commit
                    // was the tip, or of a commit the repository lost since.
                    pinned: Some(SavedHistoryScope::Commit(if refused || landed {
                        history[1].clone()
                    } else {
                        "1".repeat(40)
                    })),
                    offset: 1,
                    ..bookmark.clone()
                };
            });
            let held = hold_reads(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw_tab_frames(cx);
            if refused {
                // A write runs as the snapshot lands.
                app.update(cx, |app, _| app.operation_busy = Some("Committing…"));
            }
            drop(held);
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
            if refused {
                app.update(cx, |app, _| app.operation_busy = None);
            }
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            if landed {
                // The saved scope's page, offset 1 of the middle commit's
                // history, replaces the snapshot's row, whose offset it
                // would continue: appended, it listed the tip and then the
                // oldest commit, the middle one skipped at the seam.
                app.read_with(cx, |app, _| {
                    assert!(app.repository_tabs.restoring.is_none(), "{when}: ended");
                    assert_eq!(app.error, None, "{when}: read");
                    assert_eq!(listed(app), history[2..], "{when}: its page alone");
                    assert_eq!(app.history_paging.offset, 1, "{when}: at its offset");
                    // The window takes the page's scope with its rows, and
                    // the page's end: it ends that history, so Older stops.
                    assert_eq!(
                        app.history_paging.scope,
                        Some(gitturtle_core::HistoryScope::FromCommit(history[1].clone())),
                        "{when}: in the saved scope"
                    );
                    assert_eq!(app.history_paging.next_offset, None, "{when}: at its end");
                });
                continue;
            }
            app.read_with(cx, |app, _| {
                assert!(app.repository_tabs.restoring.is_none(), "{when}: ended");
                assert_eq!(listed(app), history[..1], "{when}: the snapshot's row");
                assert_eq!(app.history_paging.next_offset, Some(1), "{when}");
            });
            let older = cx.debug_bounds("load-more").expect("Older is drawn");
            cx.simulate_click(older.center(), Modifiers::default());
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert_eq!(app.error, None, "{when}: Older read");
                assert_eq!(listed(app), history, "{when}: Older continues it");
            });
        }
    }

    /// A cold tab's restore whose saved page lies at or past the end of its
    /// saved scope's history, as after a fetch at a shallower depth, shows
    /// the last page's worth of rows up to that end, in that
    /// scope, and selects the saved commit where that page lists it
    /// (`receive_history_page`). The empty page at the end emptied History's
    /// window, and a page past it failed the restore's read, leaving the
    /// snapshot's rows under an error.
    #[gpui::test]
    async fn a_restores_page_past_the_end_shows_the_last_page(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        let root = tempfile::tempdir().unwrap();
        let page = history_paging::PAGE_SIZE;
        let total = page + 20;
        let repo = linear_history_fixture(root.path().join("history"), total as u32);
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle_tab_test(&app, cx).await;
        let listed = |app: &GitTurtle| {
            app.commits
                .iter()
                .map(|commit| commit.oid.clone())
                .collect::<Vec<_>>()
        };
        let (tip, bookmark) = app.read_with(cx, |app, cx| {
            assert_eq!(app.history_paging.next_offset, Some(page), "a page listed");
            (app.commits[0].oid.clone(), app.tab_bookmark(cx))
        });
        // The whole history, newest first.
        let history = repo
            .history(total + 1)
            .unwrap()
            .into_iter()
            .map(|commit| commit.oid)
            .collect::<Vec<_>>();
        assert_eq!(history.len(), total, "the fixture's history");
        let oldest = history[total - 1].clone();
        for (offset, when) in [(total, "at the end"), (total + page, "past the end")] {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx))
            });
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.update(cx, |app, _| {
                let tab = &mut app.repository_tabs.tabs[1];
                tab.warm = None;
                tab.saved = Bookmark {
                    comparison: None,
                    compare: false,
                    selection: None,
                    selected_oid: Some(oldest.clone()),
                    selected_file: None,
                    query: String::new(),
                    pinned: Some(SavedHistoryScope::Commit(tip.clone())),
                    offset,
                    ..bookmark.clone()
                };
            });
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository_tabs.restoring.is_none(), "{when}: ended");
                assert_eq!(app.error, None, "{when}: read");
                // A full page, whose end Older's empty read then confirms.
                assert_eq!(
                    (app.history_paging.offset, app.history_paging.next_offset),
                    (total - page, Some(total)),
                    "{when}: the last page"
                );
                assert_eq!(listed(app), history[total - page..], "{when}: its rows");
                assert_eq!(app.visible.len(), page, "{when}: listed");
                assert_eq!(
                    app.selected_commit
                        .map(|index| app.commits[index].oid.clone()),
                    Some(oldest.clone()),
                    "{when}: the saved commit"
                );
            });
        }
    }

    /// Another warm tab shown from Settings forgets the control Back would
    /// have returned focus to in the tab Settings was opened from, as every
    /// way out of Settings to a page that places focus itself does
    /// (`forget_page_return`): the tab gives focus back to its own control.
    /// The return once stayed set, the left tab's control, for no Back.
    #[gpui::test]
    async fn a_warm_tab_shown_from_settings_forgets_backs_return(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let list = app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.active, Some(1));
            assert!(
                app.repository_tabs.tabs[0].warm.is_some(),
                "the first is warm"
            );
            app.focus.clone()
        });
        cx.update(|window, cx| window.focus(&list, cx));
        draw_tab_frames(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw_tab_frames(cx);
        app.read_with(cx, |app, _| {
            assert!(app.page == AppPage::Settings, "Settings shows");
            assert_eq!(app.page_return_focus, Some(list.clone()), "Back's return");
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle_tab_test(&app, cx).await;
        draw_tab_frames(cx);
        app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.active, Some(0), "the first tab shows");
            assert!(app.page == AppPage::Repository, "on the repository page");
            assert!(app.page_return_focus.is_none(), "forgotten");
        });
    }

    /// A comparison whose preview landed while Settings showed, its tab then
    /// left from Settings and shown again, draws its text in the frame that
    /// draws the tab, as Back builds it (`resume_repository_page`): built only
    /// by Back, the tab came back reading "Loading text…" until the file's
    /// next read.
    #[gpui::test]
    async fn a_preview_landed_under_settings_draws_as_its_tab_returns(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, fixture_repository, history_window, settle};
        let (fixture, app, cx) = history_window(cx).await;
        let other = fixture_repository(fixture.path().join("other"));
        let held = app.read_with(cx, |app, _| app.worker.hold());
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        draw(cx);
        drop(held);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        draw(cx);
        app.read_with(cx, |app, _| {
            assert!(
                app.page == AppPage::Settings
                    && app.content.is_some()
                    && app.text_mode == crate::TextMode::Unified,
                "the preview lands while Settings shows"
            );
            assert!(
                app.patch_editor.is_none(),
                "with no editor on a hidden page"
            );
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_repository_tab(other.path().to_owned(), window, cx)
            })
        });
        settle(&app, cx).await;
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        // The frame that draws the tab.
        crate::width_walk::frame(cx);
        app.read_with(cx, |app, _| {
            assert!(
                app.page == AppPage::Repository && app.mode == WorkspaceMode::Compare,
                "the tab returns to its comparison"
            );
            assert!(
                app.patch_editor.is_some(),
                "its text is built in the frame that draws the tab"
            );
        });
    }

    /// A menu opened while a cold tab's snapshot is read, by a click on the
    /// tab strip's Workspaces button, which leaves focus where it was, keeps
    /// focus as the restore lands in Compare. Closed by Escape after that,
    /// it gives focus back to what it was opened from, History's commit
    /// list, which Compare does not draw, and focus goes on to the
    /// changed-file list as the landing puts it, so the next Escape goes
    /// back to History. Given back to History's list alone, focus rested on
    /// nothing drawn and keys reached none of the window's shortcuts.
    #[gpui::test]
    async fn a_menu_closed_after_a_cold_restore_lands_leaves_focus_drawn(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, files) = app.read_with(cx, |app, _| (app.focus.clone(), app.file_focus.clone()));
        let item = "menu-item: Manage saved workspaces… · MenuItem";
        // Focus leaving a control reaches its listeners only in the active
        // window, as the person's is.
        cx.update(|window, _| window.activate_window());
        leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
        let held = hold_reads(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        draw_tab_frames(cx);
        assert_eq!(drawn_focus(&app, cx), Some(list.clone()), "its snapshot");
        let trigger = cx
            .debug_bounds("repository-workspaces")
            .expect("the Workspaces button");
        cx.simulate_click(trigger.center(), Modifiers::default());
        draw_tab_frames(cx);
        assert!(cx.debug_bounds(item).is_some(), "the menu opened");
        let menu = drawn_focus(&app, cx).expect("the menu holds focus");
        assert!(menu != list && menu != files, "the menu holds focus");
        drop(held);
        settle_tab_test(&app, cx).await;
        draw_tab_frames(cx);
        app.read_with(cx, |app, _| {
            assert!(app.repository_tabs.restoring.is_none(), "restored");
            assert!(app.mode == WorkspaceMode::Compare && app.content.is_some());
        });
        assert!(cx.debug_bounds(item).is_some(), "the menu stays open");
        assert_eq!(drawn_focus(&app, cx), Some(menu), "the menu keeps focus");
        cx.simulate_keystrokes("escape");
        draw_tab_frames(cx);
        assert!(cx.debug_bounds(item).is_none(), "Escape closed the menu");
        assert_eq!(
            drawn_focus(&app, cx),
            Some(files),
            "on the changed-file list"
        );
        cx.simulate_keystrokes("escape");
        draw_tab_frames(cx);
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History, "Escape goes back");
        });
        assert_eq!(drawn_focus(&app, cx), Some(list), "History's list");
    }

    /// A dialog, Local workspaces, or the Workspaces menu, opened in the
    /// update a cold tab's restore lands in Compare, before any frame has
    /// drawn it, keeps focus through the frames that draw Compare: the
    /// landing finds focus on what opened, which no frame has drawn yet,
    /// rather than on History's list, where showing the tab put it. Closed,
    /// each gives focus back to History's list, which Compare does not
    /// draw, and focus goes on to the changed-file list. The landing once
    /// took focus no frame had drawn for stranded and moved it to that list
    /// at once: the dialog lost its focus to the page under it, and the
    /// menu, which takes focus again as the next frame builds it, gave it
    /// back on Escape to History's list with nothing watching it, where no
    /// key reached the window's shortcuts.
    #[gpui::test]
    async fn what_opens_in_the_update_a_cold_restore_lands_keeps_focus(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, files) = app.read_with(cx, |app, _| (app.focus.clone(), app.file_focus.clone()));
        let item = "menu-item: Manage saved workspaces… · MenuItem";
        // Focus leaving a control reaches its listeners only in the active
        // window, as the person's is.
        cx.update(|window, _| window.activate_window());
        for opened in ["dialog", "menu"] {
            leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(
                    app.repository_tabs.restoring.is_none(),
                    "{opened}: restored"
                );
                assert!(app.mode == WorkspaceMode::Compare && app.content.is_some());
            });
            let position = cx
                .debug_bounds("repository-workspaces")
                .expect("the Workspaces button")
                .center();
            // The landing's update, with focus where it finds it: on
            // History's list, where showing the tab put it, and then on
            // what opened since the last frame.
            let held = cx.update(|window, cx| {
                window.focus(&list, cx);
                if opened == "dialog" {
                    app.update(cx, |app, cx| app.open_repository_library(window, cx));
                } else {
                    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
                    window.dispatch_event(
                        PlatformInput::MouseDown(MouseDownEvent {
                            button,
                            position,
                            modifiers,
                            click_count: 1,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        PlatformInput::MouseUp(MouseUpEvent {
                            button,
                            position,
                            modifiers,
                            click_count: 1,
                        }),
                        cx,
                    );
                }
                let held = window.focused(cx).expect("what opened takes focus");
                assert!(
                    held != list && window.context_stack().is_empty(),
                    "{opened}: focus on what no frame has drawn yet"
                );
                app.update(cx, |app, cx| app.land_restore_focus(window, cx));
                held
            });
            draw_tab_frames(cx);
            let kept = cx.update(|window, cx| {
                held.contains_focused(window, cx) && !window.context_stack().is_empty()
            });
            assert!(
                kept,
                "{opened}: keeps focus through the frames that draw Compare"
            );
            if opened == "dialog" {
                assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
                cx.update(|window, cx| window.close_dialog(cx));
            } else {
                assert!(cx.debug_bounds(item).is_some(), "the menu opened");
                assert!(
                    drawn_focus(&app, cx).is_some_and(|focus| focus != list && focus != files),
                    "the menu holds focus"
                );
                cx.simulate_keystrokes("escape");
            }
            draw_tab_frames(cx);
            assert!(cx.debug_bounds(item).is_none(), "{opened}: closed");
            assert_eq!(
                drawn_focus(&app, cx),
                Some(files.clone()),
                "{opened}: closed, on the changed-file list"
            );
        }
    }

    /// A cold tab whose bookmark is in Compare without a captured
    /// comparison, as an older session saved it, restores its file in
    /// Compare from the saved commit's changes (`restore_tab_file`). A
    /// dialog opened over the restore, Local workspaces, keeps focus as the
    /// file is chosen: the file is still selected and shown, and closing
    /// the dialog gives focus to the changed-file list. The restore once
    /// chose the file as a click does and focused that list under the
    /// dialog, whose keys then reached the page behind it.
    #[gpui::test]
    async fn restoring_a_tabs_file_leaves_focus_in_an_open_dialog(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let files = app.read_with(cx, |app, _| app.file_focus.clone());
        leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
        app.update(cx, |app, _| {
            let saved = &mut app.repository_tabs.tabs[1].saved;
            saved.comparison = None;
            assert!(saved.compare, "a bookmark in Compare");
        });
        // Shown, it reads its snapshot while the dialog opens.
        let held = hold_reads(&app, cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        draw_tab_frames(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_library(window, cx)));
        draw_tab_frames(cx);
        let dialog = cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx), "the dialog opened");
            window.focused(cx).expect("the dialog holds focus")
        });
        assert!(dialog != files, "the dialog holds focus");
        drop(held);
        settle_tab_test(&app, cx).await;
        draw_tab_frames(cx);
        app.read_with(cx, |app, _| {
            assert!(app.repository_tabs.restoring.is_none(), "restored");
            assert!(app.mode == WorkspaceMode::Compare, "its file is chosen");
            let file = app.selected_file.and_then(|index| app.files.get(index));
            assert_eq!(file.map(FileChange::path), Some(Path::new("notes.txt")));
            assert!(app.content.is_some(), "and shown");
        });
        cx.update(|window, cx| {
            assert!(window.has_active_dialog(cx), "the dialog stays open");
            assert_eq!(window.focused(cx), Some(dialog), "the dialog keeps focus");
            window.close_dialog(cx);
        });
        draw_tab_frames(cx);
        assert_eq!(
            drawn_focus(&app, cx),
            Some(files),
            "closed, on the changed-file list"
        );
    }

    /// Settings or Projects shown over the restore of a cold tab whose
    /// bookmark is in Compare without a captured comparison keeps focus on
    /// its control as the file is chosen from the saved commit's changes
    /// (`restore_tab_file`), as it does over a captured comparison's
    /// restore: the file is still selected and shown, and Back from the
    /// page focuses the changed-file list, not History's list, where
    /// showing the tab put focus. The restore once chose the file as a
    /// click does and focused that list under the page, which draws none
    /// of it, so keys reached no drawn control.
    #[gpui::test]
    async fn restoring_a_tabs_file_leaves_focus_on_the_page_shown_over_it(cx: &mut TestAppContext) {
        let (_first, app, cx) = crate::views::tests::history_window(cx).await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let root = tempfile::tempdir().unwrap();
        let _repo = open_compared_tab(&app, root.path(), cx).await;
        let (list, files) = app.read_with(cx, |app, _| (app.focus.clone(), app.file_focus.clone()));
        for (page, name) in [
            (AppPage::Settings, "Settings"),
            (AppPage::Projects, "Projects"),
        ] {
            leave_tab_cold_on(&app, 0, "notes.txt", true, cx).await;
            app.update(cx, |app, _| {
                let saved = &mut app.repository_tabs.tabs[1].saved;
                saved.comparison = None;
                assert!(saved.compare, "{name}: a bookmark in Compare");
            });
            // Shown, it reads its snapshot while the page is shown over it.
            let held = hold_reads(&app, cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx))
            });
            draw_tab_frames(cx);
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    if page == AppPage::Settings {
                        app.show_settings(window, cx);
                    } else {
                        app.show_projects(window, cx);
                    }
                })
            });
            draw_tab_frames(cx);
            assert!(app.read_with(cx, |app, _| app.page == page), "{name} shown");
            let control = drawn_focus(&app, cx).expect("the page's control holds focus");
            assert!(
                control != files && control != list,
                "{name}: its own control"
            );
            drop(held);
            settle_tab_test(&app, cx).await;
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.repository_tabs.restoring.is_none(), "{name}: restored");
                assert!(app.page == page, "{name}: still shown");
                assert!(
                    app.mode == WorkspaceMode::Compare,
                    "{name}: its file chosen"
                );
                let file = app.selected_file.and_then(|index| app.files.get(index));
                assert_eq!(file.map(FileChange::path), Some(Path::new("notes.txt")));
                assert!(app.content.is_some(), "{name}: and shown");
            });
            assert_eq!(
                drawn_focus(&app, cx),
                Some(control),
                "{name}: its control keeps focus"
            );
            app.read_with(cx, |app, _| {
                assert_eq!(
                    app.page_return_focus,
                    Some(files.clone()),
                    "{name}: Back returns to the changed-file list itself"
                );
            });
            cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
            draw_tab_frames(cx);
            app.read_with(cx, |app, _| {
                assert!(app.page == AppPage::Repository && app.mode == WorkspaceMode::Compare);
            });
            assert_eq!(
                drawn_focus(&app, cx),
                Some(files.clone()),
                "{name}: back, on the changed-file list"
            );
        }
    }

    /// A focus return that gives way as the frame ends, to a control the
    /// frame drew nowhere in the tab, focuses Compare's changed-file list
    /// with the pane it is: the window names the Files pane, as choosing a
    /// file does, whatever pane it named before, and the next arrow key
    /// moves the file selection. The fallback once focused the list and
    /// left the pane named History, so the pane a File history or Blame
    /// opened from there kept, and returned focus to, was History's commit
    /// list, which Compare does not draw.
    #[gpui::test]
    async fn a_focus_return_giving_way_names_the_pane_it_focuses(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = crate::views::tests::history_window_from(
            cx,
            "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 13\nfixture notes\nM 100644 inline notes.txt\ndata 6\nnotes\nM 100644 inline other.txt\ndata 6\nother\n\ndone\n",
        )
        .await;
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
        settle_tab_test(&app, cx).await;
        draw_tab_frames(cx);
        let (files, navigation) = app.read_with(cx, |app, cx| {
            assert!(app.mode == WorkspaceMode::Compare && app.content.is_some());
            (
                app.file_focus.clone(),
                app.nav_search.read(cx).focus_handle(cx),
            )
        });
        // Focus returns, as a switch to this tab returns it, to the
        // navigation's filter, which Compare does not draw, with the window
        // naming History's pane.
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.pane = Pane::History;
                app.repository_tabs.focus_return = Some(FocusReturn {
                    page: AppPage::Repository,
                    path: app.path.clone(),
                    held: app.app_focus.clone(),
                    focus: Some(navigation.clone()),
                });
                app.app_focus.focus(window, cx);
            })
        });
        draw_tab_frames(cx);
        assert_eq!(drawn_focus(&app, cx), Some(files), "the changed-file list");
        app.read_with(cx, |app, _| {
            assert!(app.pane == Pane::Files, "the pane it focuses");
            assert_eq!(app.selected_file, Some(0));
        });
        cx.simulate_keystrokes("down");
        settle_tab_test(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.selected_file, Some(1), "Down moves the file selection");
            assert!(app.pane == Pane::Files);
        });
    }

    /// Under More contrast the selected repository tab, whose `selected`
    /// frame joins its Button to its close button, draws one 1 px edge in the
    /// kit's resting edge over that frame (`appearance::resting_edge`), at
    /// rest and with the pointer over its Button, and none over either
    /// Button inside it: its Button rests on the frame's fill with none of
    /// its own (`appearance::control_button_variant_on_fill`), so the kit
    /// draws no second edge. Off, the frame draws no edge and its Button
    /// keeps the shared helper's selected fill, in Midnight and Braden; the
    /// frame and both Buttons keep their bounds.
    #[gpui::test]
    async fn the_selected_tab_draws_one_resting_edge_over_its_frame(cx: &mut TestAppContext) {
        let (_fixture, app, cx) = views::tests::history_window(cx).await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        views::tests::settle(&app, cx).await;
        let selectors = [
            "repository-tab-frame-0",
            "repository-tab-0",
            "repository-tab-close-0",
        ];
        for theme in [
            appearance::ThemeChoice::Midnight,
            appearance::ThemeChoice::Daylight,
        ] {
            for hovered in [false, true] {
                let [off, on] = [false, true].map(|more| {
                    cx.update(|window, cx| {
                        appearance::set_more_contrast(more, cx);
                        theme.apply(Some(window), cx);
                    });
                    views::tests::draw(cx);
                    let bounds = selectors.map(|selector| {
                        cx.debug_bounds(selector)
                            .unwrap_or_else(|| panic!("{selector} drawn"))
                    });
                    let pointer = if hovered {
                        bounds[1].center()
                    } else {
                        point(px(1.), px(1.))
                    };
                    cx.simulate_mouse_move(pointer, None, Modifiers::default());
                    views::tests::draw(cx);
                    let edges = appearance::painted_edges(cx, bounds[0]);
                    let tab = appearance::painted_button(cx, bounds[1]).0;
                    (bounds, edges, tab)
                });
                let at = format!("{theme:?} hovered {hovered}");
                assert_eq!(on.0, off.0, "{at}: the frame and Buttons keep their bounds");
                assert!(off.1.is_empty(), "{at}: off, no edge: {:?}", off.1);
                let (edge, selected) = cx.update(|_, cx| {
                    let palette = appearance::palette(cx);
                    (palette.resting_edge(), palette.selected)
                });
                let [drawn] = on.1[..] else {
                    panic!("{at}: one edge in the frame: {:?}", on.1);
                };
                let over = |a: Bounds<Pixels>, b: Bounds<Pixels>| {
                    [
                        (a.left(), b.left()),
                        (a.top(), b.top()),
                        (a.right(), b.right()),
                        (a.bottom(), b.bottom()),
                    ]
                    .into_iter()
                    .all(|(a, b)| (a - b).abs() < px(0.5))
                };
                assert!(
                    drawn.one_px && drawn.color == edge && over(drawn.bounds, on.0[0]),
                    "{at}: #{edge:06x} over the frame {:?}: {drawn:?}",
                    on.0[0]
                );
                if !hovered {
                    let fill = gpui_kit::Background::from(gpui_kit::Hsla::from(rgb(selected)));
                    assert_eq!(off.2, [fill], "{at}: off, the Button fills as the helper");
                    assert!(on.2.is_empty(), "{at}: on, no fill of its own: {:?}", on.2);
                }
            }
        }
        cx.update(|window, cx| {
            appearance::set_more_contrast(false, cx);
            appearance::ThemeChoice::Midnight.apply(Some(window), cx);
        });
    }
}
