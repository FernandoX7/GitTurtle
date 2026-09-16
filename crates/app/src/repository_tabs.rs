//! Local repository tabs: one active reader/watch session and bounded retained views.
use crate::*;
use anyhow::{Context as _, Result, ensure};
use futures::FutureExt;
use gpui_kit::{
    component::menu::{DropdownMenu, PopupMenuItem},
    prelude::FluentBuilder,
};
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
    saved: Bookmark,
    warm: Option<WarmTab>,
    pub error: Option<String>,
}
type SessionSaveCompletion =
    futures::future::Shared<futures::future::BoxFuture<'static, std::result::Result<(), String>>>;

#[derive(Default)]
pub struct State {
    pub tabs: Vec<Tab>,
    pub active: Option<usize>,
    pub library: Vec<LibraryEntry>,
    pub groups: Vec<String>,
    pub switching: bool,
    pub restoring: Option<Bookmark>,
    pub document_restore: Option<Bookmark>,
    panel_observers: Vec<Subscription>,
    pub save_pending: Option<Task<()>>,
    saver: commit_drafts::CoalescingSaver<(), Session>,
    save_completion: Option<SessionSaveCompletion>,
    closing: bool,
    #[cfg(test)]
    save_path: Option<PathBuf>,
}
impl State {
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
    fn saved(&self) -> Session {
        Session {
            version: 1,
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab {
                    path: SavedPath::new(&tab.path),
                    bookmark: tab.saved.clone(),
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
// Cold tabs always start with empty inputs; warm tabs restore the exact snapshot.
#[derive(Default)]
struct TabInputs {
    values: [String; 6],
}
impl TabInputs {
    fn capture(inputs: [&Entity<InputState>; 6], cx: &App) -> Self {
        Self {
            values: inputs.map(|input| input.read(cx).value().to_string()),
        }
    }
    fn restore(self, inputs: [&Entity<InputState>; 6], window: &mut Window, cx: &mut App) {
        for (input, value) in inputs.into_iter().zip(self.values) {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
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
    nav_mode: NavMode,
    nav_cursor: Option<usize>,
    expanded: HashSet<String>,
    commits: Vec<Commit>,
    visible: Vec<usize>,
    graph: Vec<graph::GraphRow>,
    graph_lanes: usize,
    graph_offset: usize,
    graph_notice: Option<String>,
    refs: HashMap<String, Vec<String>>,
    selected_commit: Option<usize>,
    parent: usize,
    retained_commit: Option<usize>,
    history_scroll: UniformListScrollHandle,
    file_scroll: UniformListScrollHandle,
    // Resize detection must compare with this tab's measured viewport, never
    // with the preceding tab's independently retained panel geometry.
    history_list_layout: Option<(Size<Pixels>, Pixels)>,
    file_list_layout: Option<(Size<Pixels>, Pixels)>,
    nav_scroll: UniformListScrollHandle,
    horizontal: ScrollHandle,
    content_panels: Entity<ResizableState>,
    history_panels: Entity<ResizableState>,
    retained_history_files: Option<(Vec<FileChange>, Option<usize>)>,
    work_status: Option<Arc<gitturtle_core::RepositoryStatus>>,
    integration_state: Option<gitturtle_core::OperationState>,
    profile: Option<gitturtle_core::GitProfile>,
    remotes: Vec<gitturtle_core::Remote>,
    working_rows: Vec<workspace::WorkingRow>,
    working_selected: Option<(usize, gitturtle_core::ChangeArea)>,
    working_scroll: UniformListScrollHandle,
    working_selection: working_selection::Selection,
    operation_notice: Option<String>,
    operation_error: Option<String>,
}

impl GitTurtle {
    fn tab_retained_bytes(&self) -> usize {
        history_bytes(&self.commits, &self.graph)
            + self.inspector_message.retained_bytes()
            + self
                .content
                .as_ref()
                .map_or(0, |content| content.bytes().saturating_mul(4))
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
                .iter()
                .map(|(oid, names)| {
                    oid.capacity() + names.iter().map(String::capacity).sum::<usize>()
                })
                .sum::<usize>()
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
            return true;
        };
        let bytes = self.tab_retained_bytes();
        if self.repository_tabs.retained_bytes().saturating_add(bytes) > MAX_RETAINED_BYTES {
            self.error=Some("Retained tabs would exceed the 512 MiB preview allowance. Close a tab to release its retained inspection before switching; drafts stay saved.".into());
            cx.notify();
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
                saved: Bookmark::default(),
                warm: None,
                error: None,
            });
            self.repository_tabs.tabs.len() - 1
        };
        let bookmark = self.tab_bookmark(cx);
        self.invalidate_read();
        self.worker.release_history();
        self.work_generation = self.work_generation.wrapping_add(1);
        self.status_task = None;
        self.integration_task = None;
        self.file_history.pause_for_tab();
        self.revision_inspection.pause_for_tab();
        self.blame.pause_for_tab();
        pdf_view::pause(self.content.as_deref());
        model_view::pause(self.content.as_deref());
        markdown_view::pause(self.content.as_deref());
        self.image_drag = None;
        let retained_commit = self.automatic.retained_commit;
        self.automatic.reset();
        self.working_paths.reset();
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
            paging: std::mem::take(&mut self.history_paging),
            history_updates: std::mem::take(&mut self.history_updates),
            inputs: TabInputs::capture(self.tab_inputs(), cx),
            branches: std::mem::take(&mut self.branches),
            worktrees: std::mem::take(&mut self.worktrees),
            nav_rows: std::mem::take(&mut self.nav_rows),
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
            integration_state: self.integration_state.take(),
            profile: self.profile.take(),
            remotes: std::mem::take(&mut self.remotes),
            working_rows: std::mem::take(&mut self.working_rows),
            working_selected: self.working_selected.take(),
            working_scroll: std::mem::take(&mut self.working_scroll),
            working_selection: std::mem::take(&mut self.working_selection),
            operation_notice: self.operation_notice.take(),
            operation_error: self.operation_error.take(),
        };
        self.repository_tabs.tabs[index].saved = bookmark;
        self.repository_tabs.tabs[index].warm = Some(warm);
        self.repository = None;
        self.path = None;
        self.observe_tab_panels(cx);
        true
    }
    fn restore_warm_tab(&mut self, warm: WarmTab, window: &mut Window, cx: &mut Context<Self>) {
        self.repository_tabs.switching = true;
        self.path = Some(warm.repository.path().to_owned());
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
        self.file_list_layout = warm.file_list_layout;
        self.nav_scroll = warm.nav_scroll;
        self.history_horizontal = warm.horizontal;
        self.content_panels = warm.content_panels;
        self.history_panels = warm.history_panels;
        self.retained_history_files = warm.retained_history_files;
        self.work_status = warm.work_status;
        self.integration_state = warm.integration_state;
        self.profile = warm.profile;
        self.remotes = warm.remotes;
        self.working_rows = warm.working_rows;
        self.working_selected = warm.working_selected;
        self.working_scroll = warm.working_scroll;
        self.working_selection = warm.working_selection;
        self.operation_notice = warm.operation_notice;
        self.operation_error = warm.operation_error;
        warm.inputs.restore(self.tab_inputs(), window, cx);
        self.observe_tab_panels(cx);
        if let Some(index) = self.repository_tabs.active {
            let saved = self.repository_tabs.tabs[index].saved.clone();
            self.restore_tab_widths(&saved);
        }
        self.page = AppPage::Repository;
        self.restore_inspection_context(warm.context, window, cx);
        // A retained keyboard target can be a tab button that has since been
        // removed. Keep application shortcuts attached during the transition,
        // then recover the exact editor/control only after this tree is drawn.
        let retained_focus = window.focused(cx);
        self.app_focus.focus(window, cx);
        let path = self.path.clone();
        let owner = cx.entity().downgrade();
        window.on_next_frame(move |window, _| {
            // Frame callbacks run before drawing. The second callback sees
            // the restored workspace rather than the previous tab's tree.
            window.on_next_frame(move |window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if this.page != AppPage::Repository
                        || this.path != path
                        || !this.app_focus.is_focused(window)
                        || window.has_active_dialog(cx)
                        || window.has_active_sheet(cx)
                    {
                        return;
                    }
                    let fallback = if this.mode == WorkspaceMode::History {
                        &this.focus
                    } else {
                        &this.file_focus
                    };
                    let focus = retained_focus
                        .as_ref()
                        .filter(|focus| this.app_focus.contains(focus, window))
                        .or_else(|| {
                            this.app_focus
                                .contains(fallback, window)
                                .then_some(fallback)
                        })
                        .unwrap_or(&this.app_focus);
                    focus.focus(window, cx);
                    window.refresh();
                });
            });
        });
        self.repository_tabs.switching = false;
        if let Some(path) = self.path.clone() {
            self.branch_input_scope.opened(&path);
            self.restore_commit_draft(path, window, cx);
        }
        self.resume_file_history(window, cx);
        self.ensure_local_watcher(true, window, cx);
        self.queue_automatic_refresh(
            local_refresh::LocalChange {
                rescan: true,
                ..Default::default()
            },
            window,
            cx,
        );
        self.filter_working_paths(window, cx);
        self.hub.update(cx, |hub, cx| {
            hub.set_can_go_back(true, cx);
            hub.set_busy(false, cx);
        });
        self.repaint_page(window, cx);
    }
}

impl GitTurtle {
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
            self.switch_repository_tab(index, window, cx);
        } else {
            self.limit = 500;
            self.open(path, None, window, cx);
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
            self.error=Some("Eight repository tabs are open. Close a tab before opening another; its commit drafts remain saved.".into());
            cx.notify();
            return true;
        }
        if !self.retain_active_tab(window, cx) {
            return true;
        }
        let index = self.repository_tabs.tabs.len();
        self.repository_tabs.tabs.push(Tab {
            path: path.to_owned(),
            saved: Bookmark::default(),
            warm: None,
            error: None,
        });
        self.repository_tabs.active = Some(index);
        self.repository_tabs.restoring = None;
        self.clear_tab_inputs(window, cx);
        false
    }
    pub(super) fn tab_snapshot_accepted(
        &mut self,
        repo: &GitRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
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
            tab.path = repo.path().to_owned();
            tab.error = None;
        }
        // Capture after receive installs the accepted metadata. A newly opened
        // active tab may never be switched or otherwise trigger a session save.
        // Cold bookmarks finish restoration before taking their replacement.
        if self.repository_tabs.restoring.is_none() {
            self.save_repository_session(window, cx);
        }
        false
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
            self.page = AppPage::Repository;
            self.repaint_page(window, cx);
            return;
        }
        if !self.retain_active_tab(window, cx) {
            return;
        }
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
            self.error = Some(format!(
                "{} is running in {}. Wait for it to finish or use its Cancel action before closing this tab.",
                self.operation_busy.unwrap_or("A Git operation"),
                tab.path.display()
            ));
            cx.notify();
            return;
        }
        let active = self.repository_tabs.active == Some(index);
        if active {
            self.persist_commit_draft(window, cx);
            self.invalidate_read();
            self.worker.release_history();
            self.automatic.reset();
            self.work_generation = self.work_generation.wrapping_add(1);
            self.status_task = None;
            self.clear_preview();
            self.file_history = file_history::State::default();
            self.revision_inspection = revision_inspection::State::default();
            self.blame = blame::State::default();
            self.discard_history_search();
            self.commits.clear();
            self.inspector_message = commit_message::State::default();
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
            self.working_rows.clear();
            self.working_paths.reset();
            self.file_paths.reset();
        }
        self.repository_tabs.tabs.remove(index);
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
                self.app_focus.focus(window, cx);
                self.hub
                    .update(cx, |hub, cx| hub.set_can_go_back(false, cx));
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
                        Ok(Err(error)) => Err(format!("{error:#}")),
                        Err(_) => Err("Repository session saving was interrupted".into()),
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
    pub(super) fn save_repository_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(250));
        self.repository_tabs.save_pending = Some(cx.spawn_in(window, async move |this, cx| {
            timer.await;
            let response = this.update(cx, |this, cx| this.queue_session_snapshot(cx));
            if let Ok(response) = response {
                let result = response.await;
                let _ = this.update(cx, |this, cx| {
                    if let Err(error) = result {
                        this.draft_save_error = Some(format!("Repository session: {error}"));
                        cx.notify();
                    }
                });
            }
        }));
    }
    fn observe_tab_panels(&mut self, cx: &mut Context<Self>) {
        self.repository_tabs.panel_observers.clear();
        for panels in [&self.content_panels, &self.history_panels] {
            self.repository_tabs
                .panel_observers
                .push(cx.observe(panels, |_, _, cx| cx.notify()));
        }
    }
    fn restore_tab_widths(&mut self, saved: &Bookmark) {
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
            self.history_paging.scope = Some(pinned.scope());
            let offset = saved.offset;
            self.request_history_page(offset, window, cx);
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
                self.change_parent(parent, window, cx);
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
        if selected.is_none() {
            self.repository_tabs.restoring = None;
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
                self.select_file(index, window, cx);
            } else {
                self.selected_file = Some(index);
            }
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
        let (Some(repo), Some(file)) = (
            self.repository.clone(),
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
        self.refresh_file_filter(cx);
        restore_list_viewport(
            &self.history_scroll,
            saved.history_y,
            &mut self.history_list_layout,
        );
        self.history_horizontal
            .set_offset(point(px(saved.horizontal_x), px(0.)));
        self.repository_tabs.document_restore = Some(saved.clone());
        self.request(
            Job::Preview {
                repo,
                file,
                origins: markdown_view::Origins::revisions(
                    saved.origins[0].clone(),
                    saved.origins[1].clone(),
                ),
            },
            "Restoring captured comparison…",
            window,
            cx,
        );
    }

    pub(super) fn restore_tab_documents(&mut self, cx: &mut Context<Self>) {
        let Some(saved) = self.repository_tabs.document_restore.take() else {
            return;
        };
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
            return self.operation_error.clone();
        }
        self.repository_tabs
            .find(path)
            .and_then(|index| self.repository_tabs.tabs[index].warm.as_ref())
            .and_then(|warm| warm.operation_error.clone())
    }
    pub(super) fn register_completed_repository(
        &mut self,
        path: PathBuf,
        notice: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repository_tabs.find(&path).is_none() && self.repository_tabs.tabs.len() < MAX_TABS
        {
            self.repository_tabs.tabs.push(Tab {
                path: path.clone(),
                saved: Bookmark::default(),
                warm: None,
                error: None,
            });
        }
        self.status = format!(
            "{notice} · {} · open its repository tab or folder",
            path.display()
        );
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
        error: Option<String>,
    ) {
        if self.path.as_deref() == Some(path) {
            if let Some(notice) = &notice {
                self.status = notice.clone();
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
            self.status = format!(
                "Git operation {} in {} · open that tab for details",
                if error.is_some() {
                    "failed"
                } else {
                    "completed"
                },
                path.display()
            );
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
    fn set_tab_library(
        &mut self,
        path: PathBuf,
        pinned: Option<bool>,
        group: Option<Option<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            self.error=Some("Local library holds up to 128 repositories. Remove an entry before adding another.".into());
            return;
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
    }
    pub(super) fn toggle_repository_pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        if let Some(path) = self.path.clone() {
            let pinned = !self.tab_is_pinned(&path);
            self.set_tab_library(path, Some(pinned), None, window, cx);
        }
    }
    pub(super) fn open_repository_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let owner = cx.entity().downgrade();
        let session = self.session_snapshot(cx);
        let active = self.path.clone();
        let view = cx.new(|cx| LibraryView::new(owner, session, active, window, cx));
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Local workspaces")
                .width(px(680.))
                .child(view.clone())
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default().ok_text("Done"),
                )
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
            self.error = Some(format!(
                "Opening {group} needs {new} more tabs. Close tabs to keep at most eight open."
            ));
            cx.notify();
            return;
        }
        for path in &paths {
            if self.repository_tabs.find(path).is_none() {
                self.repository_tabs.tabs.push(Tab {
                    path: path.clone(),
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
    fn repository_tabs_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        button("repository-workspaces", "Workspaces", "worktree", false)
            .dropdown_caret(true)
            .accessibility_label("Workspaces and tab actions")
            .tooltip("Switch, pin or reorder tabs; manage saved workspaces")
            .dropdown_menu(move |mut menu, _, cx| {
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
                    menu = menu.item(PopupMenuItem::new("Rename current project…").on_click(
                        move |_, window, cx| {
                            let _ = rename_owner.update(cx, |this, cx| {
                                this.open_rename_project(rename_path.clone(), window, cx)
                            });
                        },
                    ));
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = palette(cx);
        let (left_controls, right_controls, drag_region) = {
            #[cfg(target_os = "linux")]
            {
                let busy = self.operation_busy.is_some();
                (
                    window_chrome::controls(window_chrome::Side::Left, busy, _window, cx),
                    window_chrome::controls(window_chrome::Side::Right, busy, _window, cx),
                    window_chrome::drag_region(_window, cx),
                )
            }
            #[cfg(not(target_os = "linux"))]
            {
                (None::<AnyElement>, None::<AnyElement>, None::<AnyElement>)
            }
        };
        div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .min_h(appearance::ui_size(36.))
            .bg(rgb(colors.panel))
            .border_b_1()
            .border_color(rgb(colors.border))
            .children(left_controls)
            .when(cfg!(target_os = "linux"), |strip| {
                #[cfg(target_os = "linux")]
                let strip = strip.child(self.primary_menu.clone());
                strip
            })
            .child(
                div()
                    .id("repository-tabs")
                    .role(Role::TabList)
                    .aria_label("Repository tabs")
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .overflow_x_scroll()
                    .children(
                        self.repository_tabs
                            .tabs
                            .iter()
                            .enumerate()
                            .map(|(index, tab)| {
                                let selected = self.repository_tabs.active == Some(index);
                                let pinned = self.tab_is_pinned(&tab.path);
                                let busy = self.operation_busy.is_some()
                                    && self.operation_repository.as_ref() == Some(&tab.path);
                                let name = self.project_name(&tab.path);
                                let label = format!(
                                    "{}{}{}",
                                    if pinned { "★ " } else { "" },
                                    name,
                                    if busy {
                                        " · working"
                                    } else if tab.error.is_some() {
                                        " · unavailable"
                                    } else {
                                        ""
                                    }
                                );
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_0()
                                    .rounded(px(7.))
                                    .when(selected, |el| el.bg(rgb(colors.selected)))
                                    .child(
                                        button(("repository-tab", index), label, "", selected)
                                            .debug_selector(move || {
                                                format!("repository-tab-{index}")
                                            })
                                            .role(Role::Tab)
                                            .accessibility_label(format!(
                                                "Repository tab {}, {}{}{}",
                                                index + 1,
                                                tab.path.display(),
                                                if selected { ", selected" } else { "" },
                                                if busy { ", Git operation running" } else { "" }
                                            ))
                                            .max_w(appearance::ui_size(220.))
                                            .tooltip(format!(
                                                "{}\nSwitch tab {} · {}",
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
                                                "Close repository tab {}; drafts remain saved",
                                                tab.path.display()
                                            ))
                                            .tooltip(format!(
                                                "Close {} · drafts remain saved",
                                                tab.path.display()
                                            ))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.close_repository_tab(index, window, cx)
                                            })),
                                    )
                            }),
                    )
                    .children(drag_region),
            )
            .child(
                button("new-repository-tab", "", "plus", false)
                    .accessibility_label("Open repository tab")
                    .tooltip(format!(
                        "Open a repository tab · {}",
                        shortcuts::label(shortcuts::ShortcutId::NewTab)
                    ))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.choose_repository(&OpenRepository, window, cx)
                    })),
            )
            .child(self.repository_tabs_menu(cx))
            .children(right_controls)
            .into_any_element()
    }
}

struct LibraryView {
    owner: WeakEntity<GitTurtle>,
    session: Session,
    active: Option<PathBuf>,
    name: Entity<InputState>,
    error: Option<String>,
    _subscription: Subscription,
}
impl LibraryView {
    fn new(
        owner: WeakEntity<GitTurtle>,
        session: Session,
        active: Option<PathBuf>,
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
            name,
            error: None,
            _subscription: subscription,
        }
    }
}
fn workspace_text(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(text.clone())
        .child(text)
}

impl Render for LibraryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        let create_label = self.active.as_ref().map_or_else(
            || "Create named local workspace".to_owned(),
            |path| {
                format!(
                    "Create named local workspace and add repository {}",
                    path.display()
                )
            },
        );
        div().flex().flex_col().gap_3().child(workspace_text("local-workspaces-description", "Pinned repositories stay available after their tab closes. Named workspaces are local groups; opening a group only reads its selected repository.").text_size(appearance::ui_text(12.)))
            .children(self.active.as_ref().map(|path| workspace_text("local-workspaces-current-repository", format!("Current repository: {}", path.display())).text_size(appearance::ui_text(12.))))
            .child(div().flex().items_center().gap_2().child(div().flex_1().child(Input::new(&self.name).aria_label("New local workspace name"))).child(button("create-local-workspace","Create workspace","",false).accessibility_label(create_label).on_click(cx.listener(|this,_,window,cx|{
                let name=this.name.read(cx).value().trim().to_owned();
                if name.is_empty()||name.len()>64||name.contains(['\n','\r','\0']){this.error=Some("Use a workspace name of 1–64 bytes.".into());cx.notify();return;}
                if this.session.groups.len()>=MAX_GROUPS||this.session.groups.contains(&name){this.error=Some("Choose a new name; up to 16 local workspaces are supported.".into());cx.notify();return;}
                this.session.groups.push(name.clone());let active=this.active.clone();let _=this.owner.update(cx,|owner,cx|{owner.repository_tabs.groups.push(name.clone());if let Some(path)=active{owner.set_tab_library(path,None,Some(Some(name.clone())),window,cx);}else{owner.save_repository_session(window,cx);}});
                if let Some(path)=this.active.clone(){if let Some(entry)=this.session.library.iter_mut().find(|entry|entry.path.path()==path){entry.group=Some(name);}else{this.session.library.push(LibraryEntry{path:SavedPath::new(&path),pinned:false,group:Some(name)});}}
                this.name.update(cx,|input,cx|input.set_value("",window,cx));this.error=None;cx.notify();
            }))))
            .children(self.error.as_ref().map(|error|workspace_text("local-workspaces-error", error.clone()).role(Role::Alert).text_color(rgb(colors.removed))))
            .child(div().id("local-workspace-library").max_h(px(360.)).overflow_y_scroll().flex().flex_col().gap_2()
                .children(self.session.groups.clone().into_iter().enumerate().map(|(index,group)|{
                    let count=self.session.library.iter().filter(|entry|entry.group.as_ref()==Some(&group)).count();let assign=group.clone();let open=group.clone();let remove=group.clone();
                    let description=format!("{group} · {count} {}", if count == 1 { "repository" } else { "repositories" });
                    let add_label=self.active.as_ref().map_or_else(||format!("Add current repository to workspace {group}; no repository is open"), |path|format!("Add repository {} to workspace {group}", path.display()));
                    div().flex().items_center().gap_2().child(workspace_text(("local-workspace-name", index), description.clone()).flex_1())
                        .child(button(("open-workspace",index),"Open","",false).accessibility_label(format!("Open workspace {description}")).disabled(count==0).on_click(cx.listener(move|this,_,window,cx|{window.close_dialog(cx);let _=this.owner.update(cx,|owner,cx|owner.open_repository_group(&open,window,cx));})))
                        .child(button(("assign-workspace",index),"Add current","",false).accessibility_label(add_label).disabled(self.active.is_none()).on_click(cx.listener(move|this,_,window,cx|{if let Some(path)=this.active.clone(){let _=this.owner.update(cx,|owner,cx|owner.set_tab_library(path.clone(),None,Some(Some(assign.clone())),window,cx));if let Some(entry)=this.session.library.iter_mut().find(|entry|entry.path.path()==path){entry.group=Some(assign.clone());}else{this.session.library.push(LibraryEntry{path:SavedPath::new(&path),pinned:false,group:Some(assign.clone())});}cx.notify();}})))
                        .child(button(("delete-workspace",index),"Remove group","",false).accessibility_label(format!("Remove workspace group {group}; keep its repositories and drafts")).on_click(cx.listener(move|this,_,window,cx|{this.session.groups.retain(|group|group!=&remove);for entry in &mut this.session.library{if entry.group.as_ref()==Some(&remove){entry.group=None;}}let _=this.owner.update(cx,|owner,cx|{owner.repository_tabs.groups.retain(|group|group!=&remove);for entry in &mut owner.repository_tabs.library{if entry.group.as_ref()==Some(&remove){entry.group=None;}}owner.save_repository_session(window,cx);});cx.notify();})))
                }))
                .children(self.session.library.clone().into_iter().enumerate().map(|(index,entry)|{
                    let path=entry.path.path();let remove=path.clone();
                    let name=self.owner.read_with(cx,|owner,_|owner.project_name(&path)).unwrap_or_else(|_|preferences::directory_name(&path));
                    let label=format!("{}{} · {}{}",if entry.pinned{"★ "}else{""},name,path.display(),entry.group.as_ref().map_or(String::new(),|group|format!(" · {group}")));
                    let accessible=format!("{}repository {}{}",if entry.pinned{"Pinned "}else{""},path.display(),entry.group.as_ref().map_or(String::new(),|group|format!(", workspace {group}")));
                    div().flex().items_center().gap_2().child(workspace_text(("local-workspace-repository", index),label).aria_label(accessible).flex_1().min_w_0().truncate())
                        .child(button(("open-library-repo",index),"Open","",false).accessibility_label(format!("Open repository {}", path.display())).on_click(cx.listener(move|this,_,window,cx|{window.close_dialog(cx);let _=this.owner.update(cx,|owner,cx|owner.open_repository_tab(path.clone(),window,cx));})))
                        .child(button(("remove-library-repo",index),"Remove","",false).accessibility_label(format!("Remove repository {} from local library; keep its folder and drafts", remove.display())).tooltip("Remove from the local library; keep its folder and drafts").on_click(cx.listener(move|this,_,window,cx|{this.session.library.retain(|entry|entry.path.path()!=remove);let _=this.owner.update(cx,|owner,cx|{owner.repository_tabs.library.retain(|entry|entry.path.path()!=remove);owner.save_repository_session(window,cx);});cx.notify();})))
                })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

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
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
        let mut windows = Vec::new();
        let mut apps = Vec::new();
        let mut editors = Vec::new();
        for index in 0..2 {
            let repository =
                GitRepository::init(fixture.path().join(format!("repo-{index}")), "main").unwrap();
            let saved = fixture.path().join(format!("session-{index}.json"));
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

    #[gpui::test]
    async fn cold_tab_restores_rendered_history_offset_without_revealing_manual_selection(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{
            cell::RefCell,
            io::Write,
            process::{Command, Stdio},
            rc::Rc,
        };

        fn draw(cx: &mut VisualTestContext) {
            for _ in 0..3 {
                cx.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.draw(cx).clear(cx);
                });
                cx.executor().run_until_parked();
            }
        }

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repo = GitRepository::init(fixture.path().join("history"), "main").unwrap();
        let mut input = String::new();
        for index in 0..500 {
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
        let destination = fixture.path().join("isolated-session.json");
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
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
            let initial = repo.path().to_owned();
            let save_path = destination.clone();
            let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
            let observed = captured.clone();
            let (_, window_cx) = cx.add_window_view(move |window, cx| {
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
                Root::new(app, window, cx)
            });
            let app = observed.borrow_mut().take().unwrap();
            window_cx.simulate_resize(size(px(1480.), px(981.)));
            settle_tab_test(&app, window_cx).await;
            draw(window_cx);
            if pass == 0 {
                window_cx.simulate_resize(size(px(1000.), px(680.)));
                draw(window_cx);
                window_cx.update(|window, cx| {
                    let focus = app.read(cx).focus.clone();
                    window.focus(&focus, cx);
                });
                window_cx.simulate_keystrokes("end");
                settle_tab_test(&app, window_cx).await;
                window_cx.simulate_resize(size(px(1480.), px(981.)));
                draw(window_cx);
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
                draw(window_cx);
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

    #[gpui::test]
    async fn warm_tabs_restore_measured_viewports_with_their_manual_scroll_positions(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let first = GitRepository::init(fixture.path().join("first"), "main").unwrap();
        let second = GitRepository::init(fixture.path().join("second"), "main").unwrap();
        let initial = first.path().to_owned();
        let next = second.path().to_owned();
        let destination = fixture.path().join("isolated-session.json");
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
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
                GitRepository::init(fixture.path().join(format!("repository-{index}")), "main")
                    .unwrap()
                    .path()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        let initial = paths[0].clone();
        let destination = fixture.path().join("isolated-session.json");
        let open_key = if cfg!(target_os = "macos") {
            "cmd-o"
        } else {
            "ctrl-o"
        };
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
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

    #[gpui::test]
    async fn closing_the_last_repository_tab_keeps_application_actions_reachable(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use std::{cell::RefCell, rc::Rc};

        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repository = GitRepository::init(fixture.path().join("repository"), "main").unwrap();
        let path = repository.path().to_owned();
        let destination = fixture.path().join("isolated-session.json");
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
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
                assert!(app.app_focus.is_focused(window));
            });
            window.draw(cx).clear(cx);
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
        use std::{cell::RefCell, rc::Rc, sync::mpsc};

        cx.executor().allow_parking();
        cx.executor().set_block_on_ticks(100..=100);
        let fixture = tempfile::tempdir().unwrap();
        let first = GitRepository::init(fixture.path().join("first"), "main").unwrap();
        let second = GitRepository::init(fixture.path().join("new-tab"), "main").unwrap();
        let first_path = first.path().canonicalize().unwrap();
        let second_path = second.path().canonicalize().unwrap();
        let save_path = fixture.path().join("isolated-session.json");
        let saved = Session {
            version: 1,
            active: 0,
            tabs: vec![SavedTab {
                path: SavedPath::new(&first_path),
                bookmark: Bookmark::default(),
            }],
            ..Default::default()
        };
        saved.clone().save_at(&save_path).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
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
        window_cx.update(|_, cx| {
            app.update(cx, |app, cx| {
                // The accepted session snapshot occupies the eighth queue slot.
                drop(app.queue_session_snapshot(cx));
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
        window_cx
            .executor()
            .spawn(async move {
                release.send(()).unwrap();
            })
            .detach();
        cx.quit();
        blocker.await.unwrap().unwrap();
        for response in queued {
            response.await.unwrap().unwrap();
        }
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
        let repo = GitRepository::init(fixture.path().join("history"), "main").unwrap();
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
        let destination = fixture.path().join("isolated-session.json");
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
            }],
            ..Default::default()
        };
        // Reproduce a session written by the older Quick Open path, then use
        // the production loader and cold startup rather than constructing a
        // pre-sanitized in-memory bookmark.
        std::fs::write(&destination, serde_json::to_vec(&saved).unwrap()).unwrap();
        let restored = Session::load_at(&destination).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
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
        assert!(
            settings
                .parent()
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
            let repo = GitRepository::init(path, "main").unwrap();
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
        cx.update(|cx| {
            gpui_kit::init(cx);
            image_lifetime::init(cx);
        });
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
            let first_tab = TabInputs::capture(inputs.each_ref(), cx);
            TabInputs::default().restore(inputs.each_ref(), window, cx);
            assert!(inputs.iter().all(|input| input.read(cx).value().is_empty()));

            // Editing the new repository must not change the retained first tab.
            inputs[0].update(cx, |input, cx| {
                input.set_value("new repository query", window, cx)
            });
            let second_tab = TabInputs::capture(inputs.each_ref(), cx);
            first_tab.restore(inputs.each_ref(), window, cx);
            assert_eq!(inputs[0].read(cx).value().as_ref(), "change 11000");
            assert_eq!(inputs[1].read(cx).value().as_ref(), "feature/");
            let first_tab = TabInputs::capture(inputs.each_ref(), cx);
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
        assert_eq!(node.role(), Role::Label);
        assert_eq!(node.label(), Some(description));
        let other = workspace_text("other-repository", "/projects/two/same-name");
        let mut other_node = gpui::accesskit::Node::new(other.a11y_role().unwrap());
        other.write_a11y_info(&mut other_node);
        assert_ne!(node.label(), other_node.label());
    }

    fn session(paths: &[&str], active: usize) -> Session {
        Session {
            version: 1,
            tabs: paths
                .iter()
                .map(|path| SavedTab {
                    path: SavedPath::Text((*path).into()),
                    bookmark: Bookmark::default(),
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
}
