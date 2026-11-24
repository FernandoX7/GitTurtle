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
    /// The working state the window held as this tab was last seen marked,
    /// the one read before the mark or behind it, kept until another lands
    /// with the mark cleared ([`GitTurtle::note_the_mark`]): until then what
    /// the tab holds of its repository, shown or retained while another tab
    /// is, its Git identity with it, is not read since the mark
    /// ([`GitTurtle::read_since_the_mark`]). Dangling where no state was
    /// held. Weak, so the status it names is not kept, while no later
    /// status can take its address.
    before_mark: Option<std::sync::Weak<gitturtle_core::RepositoryStatus>>,
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
    /// What held keyboard focus as the shown tab's latest Open began,
    /// before its return to History took focus to the list
    /// ([`GitTurtle::note_open_focus`]): where that read marks the tab or
    /// leaves it marked, a menu's focus goes to the mark's way back
    /// ([`GitTurtle::close_menus_for_the_mark`]), and so does the way back's
    /// own ([`GitTurtle::return_to_the_way_back`]).
    open_focus: OpenFocus,
    /// The presses of the navigation's Refresh under the mark's line that a
    /// running write refused as they began, each refused until it ends
    /// ([`GitTurtle::way_back_key_down`],
    /// [`GitTurtle::way_back_pointer_down`]).
    refused_press: RefusedPress,
    /// Return's and Space's presses in the window, each from its first
    /// key-down until its key-up ([`GitTurtle::note_key_press_down`]), with
    /// whether the mark's way back held focus as it began: the way back's
    /// Refresh takes no press that began on another control
    /// ([`GitTurtle::way_back_key_down`]).
    key_presses: [Option<KeyPress>; 2],
    /// The key-ups [`Self::key_presses`] has counted, so that a key-up's
    /// end of a press ([`GitTurtle::note_key_press_up`]) is its own.
    key_ups: u64,
    /// While a press is noted ([`Self::key_presses`]): ends every noted
    /// press as the window loses keyboard focus, to another app or to
    /// nothing in it focused, after which a key-up may never reach it
    /// (decided 2026-11-14, by the 2026-11-11 latch rule: a press latch ends
    /// on a lost key-up, focus leaving the view or the window). Kept, a
    /// press begun on another control whose key-up was lost so made the way
    /// back refuse the next press of that key there, once, with nothing to
    /// say why.
    key_presses_end: Option<Subscription>,
    #[cfg(test)]
    save_path: Option<PathBuf>,
}
/// A press of Return or Space in the window ([`State::key_presses`]).
#[derive(Clone, Copy)]
struct KeyPress {
    /// Whether the mark's way back held focus as the key went down.
    on_way_back: bool,
    /// The key-up that ends it as the window next runs a task, unless the
    /// key goes down again first, as the release and press of an X11
    /// repeat, or an input method's, arrive together. The window losing
    /// keyboard focus ends it too, its key-up perhaps lost outside the
    /// window ([`State::key_presses_end`]).
    released: Option<u64>,
}
/// The presses of the mark's Refresh a running write refused as they began,
/// and the presses of a key that began on another control
/// ([`State::refused_press`]): each key's and the pointer's, each its own,
/// so none ends another. While a key's is down no key's click presses the
/// Refresh, and while the pointer's is down no click does
/// ([`GitTurtle::press_way_back`]); a key's never refuses the pointer's
/// click (decided 2026-11-12, one press per input): a Return whose key-up
/// came outside the window refused every later click, with nothing to say
/// why, until a key went down or focus moved.
#[derive(Default)]
struct RefusedPress {
    /// Return's, then Space's ([`refused_key`]).
    keys: [Option<RefusedKey>; 2],
    /// While a key's is down: ends the keys' presses as focus moves to
    /// another control inside the window, whose key-ups then go there, and
    /// marks them as the window goes inactive, which keeps them
    /// ([`RefusedKey::left`]; FU45-A5's rule, decided 2026-11-11): under
    /// X11 a keyboard grab mid-hold ended a press, and the next repeat after
    /// the write pressed Refresh.
    blur: Option<Subscription>,
    /// The pointer's, until it comes up, over the Refresh or off it
    /// ([`GitTurtle::way_back_pointer_up`]).
    pointer: bool,
}
/// A refused press of Return or Space on the mark's Refresh
/// ([`RefusedPress::keys`]).
#[derive(Clone, Copy)]
struct RefusedKey {
    /// Whether the window went inactive while it was down, so that its
    /// key-up may have come outside the window, never to reach it: a fresh
    /// press of its key is refused as its repeats are and ends it with its
    /// own key-up, and a press of the other key, refused with it, ends it,
    /// so a key-up lost costs one press at most.
    left: bool,
}
/// Where [`RefusedPress::keys`] keeps the press of `key`: Return's, then
/// Space's.
fn refused_key(key: &str) -> Option<usize> {
    match key {
        "enter" => Some(0),
        "space" => Some(1),
        _ => None,
    }
}
/// What held keyboard focus as an Open of the shown tab began
/// ([`State::open_focus`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OpenFocus {
    /// Something else, or nothing.
    #[default]
    Elsewhere,
    /// A menu open on the tab.
    Menu,
    /// The mark's way back (`GitTurtle::mark_way_back`): the navigation's
    /// Refresh under its line or the rail's Open repository.
    WayBack,
    /// The mark's way back, on a marked tab read again in place, which
    /// keeps focus for the whole read ([`GitTurtle::open_keeps_the_way_back`]).
    KeptOnWayBack,
    /// Settings' Name or Email, which a marked tab refuses and takes out of
    /// the Tab order: where the read marks the tab, focus goes on to the way
    /// back as from the way back itself (decided 2026-11-14).
    IdentityField,
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
                    warm.workspace.file_history.rescale_code(code_ratio, cx);
                    warm.workspace
                        .revision_inspection
                        .rescale_code(code_ratio, cx);
                }
                if let Some(scales) = lists {
                    for (scroll, ratio) in [
                        (&warm.workspace.history_scroll, scales.history),
                        (&warm.workspace.file_scroll, scales.files),
                        (&warm.workspace.nav_scroll, scales.navigation),
                        (&warm.workspace.working_scroll, scales.files),
                    ] {
                        settings::rescale_list_scroll(scroll, ratio);
                    }
                    warm.workspace.blame.rescale_lists(scales);
                    warm.workspace.file_history.rescale_lists(scales);
                    warm.workspace.revision_inspection.rescale_lists(scales);
                    warm.workspace.history_list_layout = None;
                    warm.workspace.file_list_layout = None;
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
                                // Never saved (decided 2026-11-08).
                                git_dir_device: None,
                                common_device: None,
                            },
                            restored: true,
                        }),
                    saved: tab.bookmark,
                    warm: None,
                    error: None,
                    before_mark: None,
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
            input.set_selected_range_with_direction(
                anchor.min(caret)..anchor.max(caret),
                anchor > caret,
                cx,
            );
        });
    }
}

struct WarmTab {
    bytes: usize,
    workspace: RepositoryWorkspace,
    context: file_history::ReturnContext,
    inputs: TabInputs,
    retained_commit: Option<usize>,
    working_counts: working_selection::Counts,
    working_unprepared: Option<working_selection::Preparation>,
    local_refresh_failure: Option<crate::views::OperationFailure>,
    document_restore: Option<Bookmark>,
}

/// Left padding of the repository tab strip: reserves clear space for the
/// integrated macOS traffic lights when `TitlebarOptions::appears_transparent`
/// is active outside fullscreen, and collapses back to standard `0.5 rem`
/// (`px_2`) in fullscreen and on non-macOS platforms.
pub(crate) fn titlebar_left_inset(window: &Window) -> Pixels {
    if cfg!(target_os = "macos") && !window.is_fullscreen() {
        window.pixel_snap(px(78.))
    } else {
        window.pixel_snap(window.rem_size() * 0.5)
    }
}

/// Vertical traffic-light origin matching `tab_strip_height` on macOS.
#[cfg(target_os = "macos")]
pub(crate) fn macos_traffic_light_origin(strip_height: Pixels) -> Point<Pixels> {
    point(px(12.), ((strip_height - px(12.)) / 2.).max(px(8.)))
}

#[cfg(target_os = "macos")]
struct MacTitlebarDrag {
    armed: bool,
    double_click_armed: bool,
}

#[cfg(target_os = "macos")]
fn macos_titlebar_drag_backdrop(window: &mut Window, cx: &mut App) -> AnyElement {
    let state = window.use_state(cx, |_, _| MacTitlebarDrag {
        armed: false,
        double_click_armed: false,
    });
    div()
        .id("macos-titlebar-drag-backdrop")
        .absolute()
        .inset_0()
        .on_mouse_down_out(window.listener_for(&state, |state, _, _, _| {
            state.armed = false;
            state.double_click_armed = false;
        }))
        .on_mouse_down(
            MouseButton::Left,
            // Controls on the tab strip (tabs, close buttons, `+`, Workspaces)
            // prevent default; ignore their presses so clicking, double-clicking,
            // or drag-reordering tabs never triggers a window move or zoom.
            window.listener_for(&state, |state, event: &MouseDownEvent, window, _| {
                if window.default_prevented() {
                    state.armed = false;
                    state.double_click_armed = false;
                    return;
                }
                if event.click_count == 2 {
                    state.armed = false;
                    state.double_click_armed = true;
                } else {
                    state.armed = event.click_count == 1;
                    state.double_click_armed = false;
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            window.listener_for(&state, |state, _: &MouseUpEvent, window, _| {
                let zoom = state.double_click_armed && !window.default_prevented();
                state.armed = false;
                state.double_click_armed = false;
                if zoom {
                    window.titlebar_double_click();
                }
            }),
        )
        .on_mouse_move(
            window.listener_for(&state, |state, event: &MouseMoveEvent, window, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    state.armed = false;
                    state.double_click_armed = false;
                    return;
                }
                if state.armed && !cx.has_active_drag() {
                    state.armed = false;
                    state.double_click_armed = false;
                    gpui_kit::base::TextSelection::end(window, cx);
                    cx.stop_active_drag(window);
                    window.start_window_move();
                }
            }),
        )
        .into_any_element()
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
                .tags
                .iter()
                .map(|tag| {
                    std::mem::size_of::<gitturtle_core::Tag>()
                        + tag.name.capacity()
                        + tag.oid.capacity()
                        + tag.target_oid.capacity()
                        + tag.target_kind.capacity()
                        + tag.tagger.capacity()
                })
                .sum::<usize>()
            + self
                .stashes
                .iter()
                .map(|stash| {
                    std::mem::size_of::<gitturtle_core::StashEntry>()
                        + stash.selector.capacity()
                        + stash.oid.capacity()
                        + stash.name.capacity()
                })
                .sum::<usize>()
            + self
                .submodules
                .iter()
                .map(|sub| {
                    std::mem::size_of::<gitturtle_core::SubmoduleSummary>()
                        + sub.name.capacity()
                        + sub.path.as_os_str().len()
                        + sub.index_oid.capacity()
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
                TextMode::Markdown => "markdown",
                TextMode::Table => "table",
                TextMode::Formatted => "formatted",
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
        // Quick Open's source scope and History commit-range comparisons are
        // retained only in warm tabs. Cold restoration safely keeps the
        // underlying history selection, and must not mislabel a range or
        // tracked-source diff as a single commit's file change.
        if self.is_quick_source() || self.history_range.is_some() {
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
                before_mark: None,
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
        let mut workspace = std::mem::replace(&mut self.workspace, RepositoryWorkspace::empty(cx));
        workspace.repository = Some(repo);
        workspace.history_paging = workspace.history_paging.take_settled();
        let warm = WarmTab {
            bytes,
            workspace,
            context,
            inputs: TabInputs::capture(self.tab_inputs(), window, cx),
            retained_commit,
            working_counts,
            working_unprepared,
            local_refresh_failure,
            document_restore,
        };
        self.repository_tabs.tabs[index].saved = bookmark;
        self.repository_tabs.tabs[index].warm = (!cold).then_some(warm);
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
        self.path = warm
            .workspace
            .repository
            .as_ref()
            .map(|repo| repo.path().to_owned());
        // A wait for a preparation a tab dropped shows its own label in
        // that tab and says where it runs in another.
        self.hold_dropped_preparation();
        self.workspace = warm.workspace;
        self.seed_folders = false;
        self.automatic.retained_commit = warm.retained_commit;
        // What a read of another tab failed with is not this one's to say
        // (`GitTurtle::working_read_failure`).
        self.working_read_failure = None;
        self.working_paths.restore_counts(warm.working_counts);
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
/// deleted, and so must the device where both read one, which tells the
/// same inode number on another filesystem, btrfs subvolume or snapshot, as
/// a folder above re-pointed there can find. Where either read none, the
/// inode alone tells, so a birth time or device unknown on both sides never
/// makes two directories the same where an inode is unknown too. A found
/// open in the running session holds the directory's birth time as it is
/// now: the worker drops its retained repository once the birth time
/// changes (`worker::RepositoryIdentity`).
/// Reads what the opens captured, so it touches no filesystem.
fn same_git_directories(shown: &worker::GitDirectories, found: &worker::GitDirectories) -> bool {
    fn same<T: PartialEq>(shown: Option<T>, found: Option<T>) -> bool {
        shown.zip(found).is_none_or(|(a, b)| a == b)
    }
    let (shown_inodes, found_inodes) = (shown.inodes, found.inodes);
    let alike = same(shown_inodes.git_dir_birth, found_inodes.git_dir_birth)
        && same(shown_inodes.common_birth, found_inodes.common_birth)
        && same(shown_inodes.git_dir_device, found_inodes.git_dir_device)
        && same(shown_inodes.common_device, found_inodes.common_device);
    if shown.git_dir == found.git_dir && shown.common == found.common {
        alike
            && same(shown_inodes.git_dir, found_inodes.git_dir)
            && same(shown_inodes.common, found_inodes.common)
    } else {
        let known = |shown: Option<u64>, found: Option<u64>| shown.is_some() && shown == found;
        alike
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
                if let Some(WarmTab { workspace, .. }) = warm
                    && let Some(repository) = workspace.repository
                {
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
        // A new tab shows in its place, as a switch shows another: the menus
        // open over the tab left were built from it (an Open… panel that
        // does not block the window leaves them open while it asks).
        crate::native_accessibility::close_tab_menus(window, cx);
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
            before_mark: None,
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
        // What held focus as the Open began is for that Open alone.
        let open_focus = std::mem::take(&mut self.repository_tabs.open_focus);
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
            // The mark clearing takes its way back, which kept focus for
            // the read (`open_keeps_the_way_back`): focus goes to the list,
            // as Open's return to History gives it, while it is still there.
            if tab.error.take().is_some()
                && open_focus == OpenFocus::KeptOnWayBack
                && self.page == AppPage::Repository
                && self.mark_way_back.is_focused(window)
            {
                window.focus(&self.focus, cx);
            }
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
    ///
    /// Returns whether the shown tab's mark landed, the tab unmarked before.
    pub(super) fn tab_opening_failed(&mut self, could_not_check: bool) -> bool {
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
                let landed = tab.error.is_none() && self.error.is_some();
                tab.error = self.error.clone();
                // What the window holds is from before the mark from here.
                self.note_the_mark();
                return landed;
            }
        }
        false
    }
    /// Follows the shown tab's mark for [`Self::read_since_the_mark`], as
    /// the mark lands and each time the app notifies, on every page: while
    /// the tab is marked, the working state the window holds, read before
    /// the mark or landed behind it, is noted as the tab's
    /// ([`Tab::before_mark`]), and the note goes once another has landed
    /// with the mark cleared. Kept on the tab, it stays through a switch to
    /// another tab and back: kept with Settings' fields, it went as another
    /// repository was shown, while the tab kept the identity read before the
    /// mark, and Settings showed that identity again and saved it (the
    /// security re-check of FU44-D10, S1).
    pub(super) fn note_the_mark(&mut self) {
        let held = self.workspace.work_status.as_ref();
        let Some(tab) = self
            .repository_tabs
            .active
            .and_then(|index| self.repository_tabs.tabs.get_mut(index))
        else {
            return;
        };
        let same = |before: &std::sync::Weak<gitturtle_core::RepositoryStatus>| {
            held.is_none_or(|held| std::ptr::eq(before.as_ptr(), Arc::as_ptr(held)))
        };
        if tab.error.is_some() {
            if !tab.before_mark.as_ref().is_some_and(same) {
                tab.before_mark = Some(held.map(Arc::downgrade).unwrap_or_default());
            }
        } else if tab.before_mark.as_ref().is_some_and(|before| !same(before)) {
            tab.before_mark = None;
        }
    }
    /// Whether what the window holds of the shown tab's repository, its Git
    /// identity with it, was read since the tab's mark: the tab is not
    /// marked, and where it was, a working state has landed with the mark
    /// cleared ([`Self::note_the_mark`]). A Refresh's snapshot clears a mark
    /// in place before its status read lands, an Open in place keeping what
    /// it read (`tab_snapshot_accepted`), and that read may fail or be
    /// dropped as another tab is shown, so the mark's end alone says nothing
    /// of what the window holds.
    pub(super) fn read_since_the_mark(&self) -> bool {
        let Some(tab) = self
            .repository_tabs
            .active
            .and_then(|index| self.repository_tabs.tabs.get(index))
        else {
            return true;
        };
        tab.error.is_none()
            && tab.before_mark.as_ref().is_none_or(|before| {
                (self.work_status.as_ref())
                    .is_some_and(|held| !std::ptr::eq(before.as_ptr(), Arc::as_ptr(held)))
            })
    }
    /// Notes, as an Open of the shown tab begins, what holds keyboard focus
    /// ([`State::open_focus`]), a menu, the mark's way back or Settings' Name
    /// or Email: the Open returns to History, which takes focus to the list
    /// before its read can mark the tab, so a menu open on the tab as the
    /// watcher's quiet read found its repository gone no longer held focus
    /// as the mark landed, and a Refresh pressed under the mark's line left
    /// focus on the list the tab draws empty as its read left the tab
    /// marked.
    pub(super) fn note_open_focus(&mut self, window: &Window, cx: &App) {
        self.repository_tabs.open_focus = if crate::native_accessibility::focus_in_menu(window) {
            OpenFocus::Menu
        } else if self.mark_way_back.is_focused(window) {
            OpenFocus::WayBack
        } else if self.page == AppPage::Settings && self.identity_field_focused(window, cx) {
            OpenFocus::IdentityField
        } else {
            OpenFocus::Elsewhere
        };
    }
    /// Whether the Open of `path` beginning now keeps focus on the mark's
    /// way back, which held it as the Open began ([`State::open_focus`]),
    /// rather than give it to the commit list as Open's return to History
    /// does: the shown tab, marked, read again in place on History, its
    /// Refresh pressed or asked for there or on the rail's Open repository.
    /// The mark stays drawn until a snapshot is accepted, and with it its
    /// way back, so focus stays there for the whole read and a screen reader
    /// says "Refresh" once (decided 2026-11-11: the ring sat on the empty
    /// list until the read ended, and the list was heard, then Refresh).
    /// The snapshot that clears the mark gives focus to the list
    /// ([`Self::tab_snapshot_accepted`]); one that leaves it leaves focus
    /// where it is, [`Self::return_to_the_way_back`] the fallback.
    pub(super) fn open_keeps_the_way_back(&mut self, path: &Path) -> bool {
        let keeps = self.repository_tabs.open_focus == OpenFocus::WayBack
            && !self.repository_tabs.switching
            && self.page == AppPage::Repository
            && self.mode == WorkspaceMode::History
            && self.path.as_deref() == Some(path)
            && self.shown_tab_unavailable();
        if keeps {
            self.repository_tabs.open_focus = OpenFocus::KeptOnWayBack;
        }
        keeps
    }
    /// Gives focus back to the mark's way back where it held focus as the
    /// shown tab's latest Open began and that Open's read leaves the tab
    /// marked ([`State::open_focus`]): its Refresh pressed on a tab that
    /// stays unavailable, or Refresh asked for there or on the rail's Open
    /// repository. Open's return to History took focus to the list, which a
    /// marked tab draws empty, so Enter again did nothing (decided
    /// 2026-11-11: whole focus, feedback in place). An Open of a tab marked
    /// as it began kept focus there ([`Self::open_keeps_the_way_back`]); one
    /// the read marked gave it to the list. Only while focus is still on the
    /// list that return gave it: focus the person moved during a slow read
    /// stays where they took it.
    pub(super) fn return_to_the_way_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            std::mem::take(&mut self.repository_tabs.open_focus),
            OpenFocus::WayBack | OpenFocus::KeptOnWayBack | OpenFocus::IdentityField
        ) && self.page == AppPage::Repository
            && self.shown_tab_unavailable()
            && self.focus.is_focused(window)
        {
            window.focus(&self.mark_way_back, cx);
            cx.notify();
        }
    }
    /// Return or Space going down on the navigation's Refresh under the
    /// mark's line while a write runs, refused for its whole press
    /// ([`State::refused_press`]): its repeats, marked held or, as X11 sends
    /// them, fresh, and its key-up press nothing, even once the write has
    /// ended, and the busy spell's look shows at once, so the Refresh dims
    /// and says why in the frame of the refusal. GPUI arms a Button's press
    /// on any Enter or Space key-down and makes it as the key comes up, so a
    /// Return held through the write's end refreshed as it came up, and a
    /// repeat after the write armed it again. The other key pressed while
    /// it is down is refused with it, the write running or not, and its
    /// key-up leaves the first key's press refused (decided 2026-11-12):
    /// Space pressed after the write ended Return's press, and the press
    /// GPUI armed on Space's key-down was made at the next key-up, which
    /// read. A press ends at its own key-up or as focus moves to another
    /// control inside the window, never as the window goes inactive
    /// ([`RefusedPress::blur`]) or as another key goes down, though one
    /// whose key-up may have come outside the window ends as the other key
    /// is refused with it ([`RefusedKey::left`]), and a key going down never
    /// ends the pointer's: a letter typed while the pointer was held let its
    /// release refresh. A press that began on another control, its key held
    /// as focus came here ([`State::key_presses`]), is refused the same way,
    /// with no busy look, since nothing waits: its repeats and its key-up
    /// press nothing, and a fresh press after its key-up reads.
    pub(super) fn way_back_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refuse_way_back_key(event, true, window, cx);
    }
    /// Return or Space going down on the rail's Open repository, the mark's
    /// way back where the rail stands in the navigation's place: a press
    /// that began on another control is refused there for its whole press,
    /// and the other key's while it is down, as on the navigation's Refresh
    /// ([`Self::way_back_key_down`]), while a running write refuses
    /// neither, the folder chooser it opens being no write. It held the way
    /// back's focus with no such refusal, so a Return held in Settings' Name
    /// as the read that marks the tab gave it focus opened the chooser
    /// (FU44-D9's code and design re-checks).
    pub(super) fn rail_way_back_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refuse_way_back_key(event, false, window, cx);
    }
    /// [`Self::way_back_key_down`], a running write refusing the press, with
    /// its busy look, only where `write_refuses`.
    fn refuse_way_back_key(
        &mut self,
        event: &KeyDownEvent,
        write_refuses: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = refused_key(event.keystroke.key.as_str()) else {
            return;
        };
        // A press that began on another control, its key held as focus came
        // here, as the read that marks the tab gives focus on from Settings'
        // Name (decided 2026-11-14: a press acts only on the control it
        // began on): its repeats armed the Refresh, and its key-up pressed
        // it.
        let elsewhere =
            self.repository_tabs.key_presses[slot].is_some_and(|press| !press.on_way_back);
        let press = &mut self.repository_tabs.refused_press;
        if press.keys[slot].is_some() {
            window.prevent_default();
            return;
        }
        let other = &mut press.keys[1 - slot];
        let waits = other.is_some() || (write_refuses && self.operation_busy.is_some());
        if event.keystroke.modifiers.modified() || !(waits || elsewhere) {
            return;
        }
        if other.is_some_and(|other| other.left) {
            *other = None;
        }
        window.prevent_default();
        press.keys[slot] = Some(RefusedKey { left: false });
        if press.blur.is_none() {
            press.blur = Some(cx.on_blur(&self.mark_way_back, window, |this, window, _| {
                let press = &mut this.repository_tabs.refused_press;
                if window.is_window_active() {
                    press.keys = [None; 2];
                    press.blur = None;
                } else {
                    for key in press.keys.iter_mut().flatten() {
                        key.left = true;
                    }
                }
            }));
        }
        if waits && write_refuses {
            self.show_busy_look(cx);
        }
    }
    /// The key-up that ends a press [`Self::way_back_key_down`] refused
    /// presses nothing, and leaves the other key's, still down, refused.
    pub(super) fn way_back_key_up(
        &mut self,
        event: &KeyUpEvent,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        let Some(slot) = refused_key(event.keystroke.key.as_str()) else {
            return;
        };
        let press = &mut self.repository_tabs.refused_press;
        if press.keys[slot].take().is_some() {
            window.prevent_default();
            if press.keys.iter().all(Option::is_none) {
                press.blur = None;
            }
        }
    }
    /// Return or Space going down anywhere in the window, noted for the
    /// mark's way back ([`State::key_presses`]): a press as it begins, with
    /// whether the way back held focus then, and its repeats, marked held
    /// or, as X11 sends them, fresh or a release with its press, as part of
    /// it.
    pub(super) fn note_key_press_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = refused_key(event.keystroke.key.as_str()) else {
            return;
        };
        let on_way_back = self.mark_way_back.is_focused(window);
        let press = &mut self.repository_tabs.key_presses[slot];
        match press {
            Some(press) => press.released = None,
            None => {
                *press = Some(KeyPress {
                    on_way_back,
                    released: None,
                })
            }
        }
        if self.repository_tabs.key_presses_end.is_none() {
            self.repository_tabs.key_presses_end =
                Some(cx.on_focus_out(&self.app_focus, window, |this, _, _, _| {
                    let state = &mut this.repository_tabs;
                    state.key_presses = [None; 2];
                    state.key_presses_end = None;
                }));
        }
    }
    /// Return or Space coming up anywhere in the window ends its press
    /// ([`State::key_presses`]) as the window next runs a task, unless the
    /// key goes down again first, as an X11 repeat's or an input method's
    /// release and press arrive together.
    pub(super) fn note_key_press_up(
        &mut self,
        event: &KeyUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = refused_key(event.keystroke.key.as_str()) else {
            return;
        };
        let state = &mut self.repository_tabs;
        state.key_ups += 1;
        let key_up = state.key_ups;
        let Some(press) = &mut state.key_presses[slot] else {
            return;
        };
        press.released = Some(key_up);
        cx.spawn(async move |this, cx| {
            this.update(cx, |this, _| {
                let state = &mut this.repository_tabs;
                let press = &mut state.key_presses[slot];
                if press.is_some_and(|press| press.released == Some(key_up)) {
                    *press = None;
                }
                if state.key_presses.iter().all(Option::is_none) {
                    state.key_presses_end = None;
                }
            })
            .ok();
        })
        .detach();
    }
    /// The pointer going down on the navigation's Refresh under the mark's
    /// line while a write runs: its release presses nothing, even once the
    /// write has ended, and the busy spell's look shows at once
    /// ([`State::refused_press`]). GPUI makes a pointer's press as it comes
    /// up, so one that went down refused refreshed if the write ended first.
    pub(super) fn way_back_pointer_down(&mut self, cx: &mut Context<Self>) {
        let refused = self.operation_busy.is_some();
        self.repository_tabs.refused_press.pointer = refused;
        if refused {
            self.show_busy_look(cx);
        }
    }
    /// The pointer coming up, over the navigation's Refresh or off it, ends
    /// its refused press: over it, after the click it makes there, which
    /// the press refuses ([`Self::press_way_back`]), GPUI's mouse-up
    /// listeners on one element running in the bubble phase after its
    /// click. Released off it, or over it dimmed, which makes no click, the
    /// press stayed refused and refused every later Return after the write.
    pub(super) fn way_back_pointer_up(&mut self) {
        self.repository_tabs.refused_press.pointer = false;
    }
    /// A press of the navigation's Refresh under the mark's line, which
    /// reads the tab's repository again unless a write runs or a press that
    /// began refused is still down ([`State::refused_press`]): for a key's
    /// click, a key's or the pointer's; for the pointer's, its own, which
    /// ends with it, never a key's ([`RefusedPress`]). A refused press shows
    /// the busy spell's look at once, as one refused as it begins does.
    pub(super) fn press_way_back(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let press = &mut self.repository_tabs.refused_press;
        let began_refused = match event {
            ClickEvent::Keyboard(_) => press.pointer || press.keys.iter().any(Option::is_some),
            _ => std::mem::take(&mut press.pointer),
        };
        if began_refused || self.operation_busy.is_some() {
            self.show_busy_look(cx);
            return;
        }
        self.refresh(&Refresh, window, cx);
    }
    /// Closes every menu open on the shown tab as its mark lands
    /// ([`Self::tab_opening_failed`]), as a resize closes them: the
    /// navigation's, the commit list's and the file list's row menus and
    /// every button's and right-click menu, through the close a tab change
    /// uses (`native_accessibility::close_tab_menus`). A menu belongs to the
    /// state that drew it (decided 2026-11-11): a branch's menu open as the
    /// mark landed was hidden, never closed, so focus stayed on a menu no
    /// frame drew, Escape doing nothing, and the menu came back, still
    /// focused, with its actions on the branch from before as the mark
    /// cleared. Focus a closing menu held goes to the mark's way back, never
    /// to something undrawn: the navigation's Refresh on History, the rail's
    /// Open repository where the rail stands in its place
    /// (`mark_way_back`). A dialog or sheet keeps its own menus and focus,
    /// and another page than the repository's draws none of the tab's.
    pub(super) fn close_menus_for_the_mark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Repository
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        // Focus a menu held as the read that marks the tab began, which
        // Open's return to History took to the list, or holds now in the
        // frame last drawn, read before any menu closes.
        let held = self.repository_tabs.open_focus == OpenFocus::Menu
            || crate::native_accessibility::focus_in_menu(window);
        self.file_menu_row = None;
        let rows = [
            self.nav_menu.take(),
            self.commit_menu.take(),
            self.file_menu.take(),
        ];
        let closed = rows.iter().any(Option::is_some);
        drop(rows);
        crate::native_accessibility::close_tab_menus(window, cx);
        if held || (closed && window.focused(cx).is_none()) {
            window.focus(&self.mark_way_back, cx);
        }
        cx.notify();
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
        if self.refuse_project_at_tab_limit(cx) {
            return;
        }
        self.hub
            .update(cx, |hub, cx| hub.choose_folder(true, window, cx));
    }
    /// Refuse a Projects action (Open Folder, Clone, Create) before touching
    /// disk when the tab strip is already at [`MAX_TABS`].
    pub(super) fn refuse_project_at_tab_limit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.repository_tabs.tabs.len() < MAX_TABS {
            return false;
        }
        // One refusal line at a time: an earlier strip refusal goes.
        self.repository_tabs.refusal = None;
        self.hub.update(cx, |hub, cx| {
            hub.set_error(Some(TAB_LIMIT_REFUSAL.to_owned()), cx)
        });
        cx.notify();
        true
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
        // A menu open over the tab left was built from it: a choice made in
        // it once acted on the tab shown now.
        crate::native_accessibility::close_tab_menus(window, cx);
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
        if next != index {
            // A move closes them as a switch does, their tab now at another
            // place in the strip; focus stays in the tab shown.
            crate::native_accessibility::close_tab_menus(window, cx);
        }
        self.repository_tabs.reorder(index, next);
        self.save_repository_session(window, cx);
        cx.notify();
    }
    pub(super) fn reorder_repository_tab(
        &mut self,
        from: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
            || from == to
            || from >= self.repository_tabs.tabs.len()
            || to >= self.repository_tabs.tabs.len()
        {
            return;
        }
        crate::native_accessibility::close_tab_menus(window, cx);
        self.repository_tabs.reorder(from, to);
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
        let active = self.repository_tabs.active == Some(index);
        if active {
            // Its menus go with it, as a switch closes them, the last tab's
            // too, as Projects shows. A tab closed behind the one shown
            // leaves the shown tab's menus open: they act on it still.
            crate::native_accessibility::close_tab_menus(window, cx);
        }
        // Its cause gone with the closed tab, the page's refusal goes too,
        // as the strip's just did.
        self.hub.update(cx, |hub, cx| {
            hub.clear_error_matching(TAB_LIMIT_REFUSAL, cx);
            hub.clear_error_matching(RETAINED_ALLOWANCE_REFUSAL, cx);
        });
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
            self.selected_compare_commit = None;
            self.history_range = None;
            self.selected_file = None;
            self.tags.clear();
            self.tags_truncated = false;
            self.stashes.clear();
            self.submodules.clear();
            self.bisect_state = None;
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
        self.quick_open_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .evict_repo(&closed);
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
                && let Ok(completion) = owner.update(cx, |this, cx| {
                    // Never let closing a blank secondary window clobber the
                    // primary window's saved repository tabs.
                    if this.repository_tabs.tabs.is_empty() && cx.windows().len() > 1 {
                        return None;
                    }
                    Some(this.flush_session_for_shutdown(cx))
                })
                && let Some(completion) = completion
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
        for panels in [
            &self.workspace.content_panels,
            &self.workspace.history_panels,
        ] {
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
            "table" => TextMode::Table,
            "formatted" => TextMode::Formatted,
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
            &mut self.workspace.commits,
            &mut self.workspace.graph,
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
            &self.workspace.history_scroll,
            history_y,
            &mut self.workspace.history_list_layout,
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
        restore_list_viewport(
            &self.workspace.file_scroll,
            saved.file_y,
            &mut self.workspace.file_list_layout,
        );
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
        self.workspace.selected_commit = restore_selected_identity(
            &saved,
            &mut self.workspace.commits,
            &mut self.workspace.graph,
            &mut self.automatic.retained_commit,
        );
        self.filter_history_retaining_scroll(cx);
        self.parent = saved.parent;
        self.files = vec![file];
        self.selected_file = Some(0);
        self.mode = WorkspaceMode::Compare;
        self.pane = Pane::Files;
        self.sidebar = false;
        self.land_restore_focus(window, cx);
        self.refresh_file_filter(window, cx);
        restore_list_viewport(
            &self.workspace.history_scroll,
            saved.history_y,
            &mut self.workspace.history_list_layout,
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
                prepare_split: self.text_mode == TextMode::Split,
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
        if self.mode == WorkspaceMode::History
            && self.files.is_empty()
            && self.loading.is_none()
            && self.error.is_none()
            && !self.revision_inspection.is_active()
            && !self.file_history.is_active()
            && let Some(range) = self.history_range.clone()
        {
            self.request_history_range_changes(&range, window, cx);
            return;
        }
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
            .and_then(|warm| warm.workspace.operation_error.as_deref().map(str::to_owned))
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
                before_mark: None,
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
            warm.workspace.history_updates.committed = Some(oid);
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
            // Each failure the banner reports is another refusal, so the
            // same one again is announced again and its box opens at its
            // top (`GitTurtle::operation_error_line`).
            if error.is_some() {
                self.error_refusals = self.error_refusals.wrapping_add(1);
            }
            self.operation_notice = notice;
            self.operation_error = error;
        } else {
            if let Some(index) = self.repository_tabs.find(path)
                && let Some(warm) = self.repository_tabs.tabs[index].warm.as_mut()
            {
                warm.workspace.operation_notice = notice;
                warm.workspace.operation_error = error.clone();
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
                    before_mark: None,
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
        // The first of the rows whose controls the reason's popup at Pull requests stands
        // clear of, so the frame before's are forgotten here.
        self.begin_header_controls();
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
        let strip_height = tab_strip_height(window, cx);
        #[cfg(target_os = "macos")]
        {
            let traffic_light_cache = window.use_state(cx, |_, _| {
                std::cell::Cell::new(None::<(Point<Pixels>, bool)>)
            });
            let origin = macos_traffic_light_origin(strip_height);
            let key = (origin, window.is_fullscreen());
            if traffic_light_cache.read(cx).get() != Some(key) {
                traffic_light_cache.read(cx).set(Some(key));
                window.set_traffic_light_position(origin);
            }
        }
        let strip =
            div()
                .debug_selector(|| "repository-tab-strip".into())
                .relative()
                .flex()
                .items_center()
                // A ring's room between its controls, as between its tabs:
                // the 0.25 rem left Main Menu's ring reaching into a selected
                // first tab at 11 pt (`tab_gap`).
                .gap(tab_gap(window))
                .pl(titlebar_left_inset(window))
                .pr_2()
                .min_h(strip_height)
                .bg(rgb(colors.panel))
                .border_b(TAB_STRIP_BORDER)
                .border_color(rgb(colors.border))
                .when(cfg!(target_os = "macos"), |strip| {
                    #[cfg(target_os = "macos")]
                    let strip = strip.child(macos_titlebar_drag_backdrop(window, cx));
                    strip
                })
                .children(left_controls)
                .when(cfg!(target_os = "linux"), |strip| {
                    #[cfg(target_os = "linux")]
                    let strip = strip.child(self.framed_header_control(self.primary_menu.clone()));
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
                                let can_drag = self.repository_tabs.tabs.len() >= 2
                                    && !window.has_active_dialog(cx)
                                    && !window.has_active_sheet(cx);
                                let drag_label: SharedString = name.clone().into();
                                let drop_border = rgb(colors.accent);
                                self.header_control(div().id(("repository-tab-frame", index)))
                                    .debug_selector(move || format!("repository-tab-frame-{index}"))
                                    .flex()
                                    .items_center()
                                    .gap_0()
                                    .min_w(name_floor + appearance::ui_size(28.))
                                    .rounded(px(7.))
                                    .when(selected, |el| el.bg(rgb(colors.selected)))
                                    .when(can_drag, |el| {
                                        el.on_drag(
                                            DraggedRepositoryTab {
                                                index,
                                                label: drag_label.clone(),
                                            },
                                            |drag, _, _, cx| {
                                                cx.new(|_| DragTabPreview(drag.label.clone()))
                                            },
                                        )
                                        .drag_over::<DraggedRepositoryTab>(
                                            move |style, drag, _, _| {
                                                if drag.index == index {
                                                    style
                                                } else {
                                                    style.border_l_2().border_color(drop_border)
                                                }
                                            },
                                        )
                                        .on_drop(cx.listener(
                                            move |this, drag: &DraggedRepositoryTab, window, cx| {
                                                this.reorder_repository_tab(
                                                    drag.index, index, window, cx,
                                                );
                                            },
                                        ))
                                    })
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
                    self.framed_header_control(
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
                    ),
                )
                .child(self.framed_header_control(self.repository_tabs_menu(labels.workspaces, cx)))
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

#[derive(Clone)]
struct DraggedRepositoryTab {
    index: usize,
    label: SharedString,
}

struct DragTabPreview(SharedString);

impl Render for DragTabPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div()
            .px_3()
            .py_1()
            .rounded(px(7.))
            .bg(rgb(p.raised()))
            .border_1()
            .border_color(rgb(p.accent))
            .text_size(crate::appearance::ui_text(12.))
            .text_color(rgb(p.text))
            .child(self.0.clone())
    }
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
                    let label = gitturtle_core::strip_bidi_controls(&format!(
                        "{}{}{}",
                        if entry.pinned { "★ " } else { "" },
                        identity,
                        entry
                            .group
                            .as_ref()
                            .map_or(String::new(), |group| format!(" · {group}"))
                    ))
                    .into_owned();
                    let accessible = gitturtle_core::strip_bidi_controls(&format!(
                        "{}repository {}{}",
                        if entry.pinned { "Pinned " } else { "" },
                        identity,
                        entry
                            .group
                            .as_ref()
                            .map_or(String::new(), |group| format!(", workspace {group}"))
                    ))
                    .into_owned();
                    div().flex().items_center().gap_2().child(crate::native_accessibility::static_text(div().id(("local-workspace-repository", index)).debug_selector(move||format!("local-workspace-repository-{index}")).child(label), accessible.clone()).flex_1().min_w_0().truncate().tooltip(move|window,cx|Tooltip::new(accessible.clone()).build(window,cx)))
                        .child(button(("open-library-repo",index),"Open","",false).debug_selector(move||format!("open-library-repo-{index}")).accessibility_label(format!("Open repository {identity}")).on_click(cx.listener(move|this,_,window,cx|{window.close_dialog(cx);let _=this.owner.update(cx,|owner,cx|owner.open_repository_tab(path.clone(),window,cx));})))
                        .child(button(("remove-library-repo",index),"Remove","",false).accessibility_label(format!("Remove repository {identity} from local library; keep its folder and drafts")).tooltip("Remove from the local library; keep its folder and drafts").on_click(cx.listener(move|this,_,window,cx|{this.session.library.retain(|entry|entry.path.path()!=remove);let _=this.owner.update(cx,|owner,cx|{owner.repository_tabs.library.retain(|entry|entry.path.path()!=remove);owner.save_repository_session(window,cx);});cx.notify();})))
                }))))
    }
}

#[cfg(test)]
#[path = "repository_tabs_tests.rs"]
pub(crate) mod tests;
