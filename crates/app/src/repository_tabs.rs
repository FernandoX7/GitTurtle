//! Local repository tabs: one active reader/watch session and bounded retained views.
use crate::*;
use anyhow::{Result, ensure};
use gpui_kit::prelude::FluentBuilder;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};

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
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_SESSION_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= MAX_SESSION_BYTES,
            "Repository session exceeds 16 MiB"
        );
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
    pub fn save(mut self) -> Result<()> {
        self.normalize();
        let bytes = serde_json::to_vec_pretty(&self)?;
        ensure!(
            bytes.len() <= MAX_SESSION_BYTES,
            "Repository session exceeds 16 MiB"
        );
        preferences::atomic_write(&session_path()?, &bytes)
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
}
impl State {
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
    query: String,
    nav_query: String,
    working_query: String,
    branch_input: String,
    remote_input: String,
    remote_branch_input: String,
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
            + self
                .content
                .as_ref()
                .map_or(0, |content| content.bytes().saturating_mul(4))
            + self.files.iter().map(file_bytes).sum::<usize>()
            + self.file_history.retained_bytes()
            + self.revision_inspection.retained_bytes()
            + self.blame.retained_bytes()
            + self.history_search.retained_bytes()
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
        Bookmark {
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
        }
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
            query: self.search.read(cx).value().to_string(),
            nav_query: self.nav_search.read(cx).value().to_string(),
            working_query: self.working_filter.read(cx).value().to_string(),
            branch_input: self.branch_name.read(cx).value().to_string(),
            remote_input: self.remote_name.read(cx).value().to_string(),
            remote_branch_input: self.remote_branch.read(cx).value().to_string(),
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
        for (input, value) in [
            (&self.search, warm.query),
            (&self.nav_search, warm.nav_query),
            (&self.working_filter, warm.working_query),
            (&self.branch_name, warm.branch_input),
            (&self.remote_name, warm.remote_input),
            (&self.remote_branch, warm.remote_branch_input),
        ] {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
        self.observe_tab_panels(cx);
        if let Some(index) = self.repository_tabs.active {
            let saved = self.repository_tabs.tabs[index].saved.clone();
            self.restore_tab_widths(&saved);
        }
        self.page = AppPage::Repository;
        self.restore_inspection_context(warm.context, window, cx);
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
            self.graph.clear();
            self.visible.clear();
            self.files.clear();
            self.selected_commit = None;
            self.selected_file = None;
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
    pub(super) fn install_tab_quit_observer(&mut self, cx: &mut Context<Self>) {
        self.subscriptions.push(cx.on_app_quit(|this, cx| {
            let session = this.session_snapshot(cx);
            let reply = this.preferences_writer.submit(move || session.save());
            async move {
                let _ = reply.await;
            }
        }));
    }
    pub(super) fn save_repository_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let timer = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(250));
        self.repository_tabs.save_pending = Some(cx.spawn_in(window, async move |this, cx| {
            timer.await;
            let response = this.update(cx, |this, cx| {
                let session = this.session_snapshot(cx);
                this.preferences_writer.submit(move || session.save())
            });
            if let Ok(response) = response {
                let result = response.await;
                let _ = this.update(cx, |this, cx| {
                    if let Ok(Err(error)) = result {
                        this.draft_save_error = Some(format!("Repository session: {error:#}"));
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
        self.history_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(history_y)));
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
        self.file_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(saved.file_y)));
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
        self.history_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(px(0.), px(saved.history_y)));
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
    pub(super) fn render_repository_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = palette(cx);
        div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .min_h(appearance::ui_size(36.))
            .bg(rgb(colors.panel))
            .border_b_1()
            .border_color(rgb(colors.border))
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
                                let name = tab
                                    .path
                                    .file_name()
                                    .unwrap_or(tab.path.as_os_str())
                                    .to_string_lossy();
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
                                            .role(Role::Tab)
                                            .accessibility_label(format!(
                                                "Repository tab {}, {}{}{}",
                                                index + 1,
                                                tab.path.display(),
                                                if selected { ", selected" } else { "" },
                                                if busy { ", Git operation running" } else { "" }
                                            ))
                                            .max_w(px(220.))
                                            .tooltip(format!(
                                                "{}\nSwitch tab {} · {}⌥{}",
                                                tab.path.display(),
                                                index + 1,
                                                if cfg!(target_os = "macos") {
                                                    "⌘"
                                                } else {
                                                    "Ctrl+"
                                                },
                                                index + 1
                                            ))
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.switch_repository_tab(index, window, cx)
                                            })),
                                    )
                                    .child(
                                        button(("close-repository-tab", index), "×", "", false)
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
                    ),
            )
            .child(
                button("new-repository-tab", "+", "", false)
                    .accessibility_label("Open repository tab")
                    .tooltip("Open a repository tab")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.choose_repository(&OpenRepository, window, cx)
                    })),
            )
            .child(
                button(
                    "pin-repository-tab",
                    if self
                        .path
                        .as_deref()
                        .is_some_and(|path| self.tab_is_pinned(path))
                    {
                        "Unpin"
                    } else {
                        "Pin"
                    },
                    "",
                    false,
                )
                .disabled(self.repository.is_none())
                .on_click(
                    cx.listener(|this, _, window, cx| this.toggle_repository_pin(window, cx)),
                ),
            )
            .child(
                button("repository-tab-move-left", "←", "", false)
                    .accessibility_label("Move repository tab left")
                    .tooltip("Move active repository tab left")
                    .disabled(self.repository_tabs.active.is_none_or(|index| index == 0))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.move_repository_tab(-1, window, cx)),
                    ),
            )
            .child(
                button("repository-tab-move-right", "→", "", false)
                    .accessibility_label("Move repository tab right")
                    .tooltip("Move active repository tab right")
                    .disabled(
                        self.repository_tabs
                            .active
                            .is_none_or(|index| index + 1 >= self.repository_tabs.tabs.len()),
                    )
                    .on_click(
                        cx.listener(|this, _, window, cx| this.move_repository_tab(1, window, cx)),
                    ),
            )
            .child(
                button("repository-workspaces", "Workspaces", "", false).on_click(
                    cx.listener(|this, _, window, cx| this.open_repository_library(window, cx)),
                ),
            )
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
impl Render for LibraryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        div().flex().flex_col().gap_3().child(div().text_size(appearance::ui_text(12.)).child("Pinned repositories stay available after their tab closes. Named workspaces are local groups; opening a group only reads its selected repository."))
            .child(div().flex().items_center().gap_2().child(div().flex_1().child(Input::new(&self.name))).child(button("create-local-workspace","Create workspace","",false).on_click(cx.listener(|this,_,window,cx|{
                let name=this.name.read(cx).value().trim().to_owned();
                if name.is_empty()||name.len()>64||name.contains(['\n','\r','\0']){this.error=Some("Use a workspace name of 1–64 bytes.".into());cx.notify();return;}
                if this.session.groups.len()>=MAX_GROUPS||this.session.groups.contains(&name){this.error=Some("Choose a new name; up to 16 local workspaces are supported.".into());cx.notify();return;}
                this.session.groups.push(name.clone());let active=this.active.clone();let _=this.owner.update(cx,|owner,cx|{owner.repository_tabs.groups.push(name.clone());if let Some(path)=active{owner.set_tab_library(path,None,Some(Some(name.clone())),window,cx);}else{owner.save_repository_session(window,cx);}});
                if let Some(path)=this.active.clone(){if let Some(entry)=this.session.library.iter_mut().find(|entry|entry.path.path()==path){entry.group=Some(name);}else{this.session.library.push(LibraryEntry{path:SavedPath::new(&path),pinned:false,group:Some(name)});}}
                this.name.update(cx,|input,cx|input.set_value("",window,cx));this.error=None;cx.notify();
            }))))
            .children(self.error.as_ref().map(|error|div().text_color(rgb(colors.removed)).child(error.clone())))
            .child(div().id("local-workspace-library").max_h(px(360.)).overflow_y_scroll().flex().flex_col().gap_2()
                .children(self.session.groups.clone().into_iter().enumerate().map(|(index,group)|{
                    let count=self.session.library.iter().filter(|entry|entry.group.as_ref()==Some(&group)).count();let assign=group.clone();let open=group.clone();let remove=group.clone();
                    div().flex().items_center().gap_2().child(div().flex_1().child(format!("{group} · {count} repositories")))
                        .child(button(("open-workspace",index),"Open","",false).disabled(count==0).on_click(cx.listener(move|this,_,window,cx|{window.close_dialog(cx);let _=this.owner.update(cx,|owner,cx|owner.open_repository_group(&open,window,cx));})))
                        .child(button(("assign-workspace",index),"Add current","",false).disabled(self.active.is_none()).on_click(cx.listener(move|this,_,window,cx|{if let Some(path)=this.active.clone(){let _=this.owner.update(cx,|owner,cx|owner.set_tab_library(path.clone(),None,Some(Some(assign.clone())),window,cx));if let Some(entry)=this.session.library.iter_mut().find(|entry|entry.path.path()==path){entry.group=Some(assign.clone());}else{this.session.library.push(LibraryEntry{path:SavedPath::new(&path),pinned:false,group:Some(assign.clone())});}cx.notify();}})))
                        .child(button(("delete-workspace",index),"Remove group","",false).on_click(cx.listener(move|this,_,window,cx|{this.session.groups.retain(|group|group!=&remove);for entry in &mut this.session.library{if entry.group.as_ref()==Some(&remove){entry.group=None;}}let _=this.owner.update(cx,|owner,cx|{owner.repository_tabs.groups.retain(|group|group!=&remove);for entry in &mut owner.repository_tabs.library{if entry.group.as_ref()==Some(&remove){entry.group=None;}}owner.save_repository_session(window,cx);});cx.notify();})))
                }))
                .children(self.session.library.clone().into_iter().enumerate().map(|(index,entry)|{
                    let path=entry.path.path();let remove=path.clone();let label=format!("{}{}{}",if entry.pinned{"★ "}else{""},path.display(),entry.group.map_or(String::new(),|group|format!(" · {group}")));
                    div().flex().items_center().gap_2().child(div().flex_1().min_w_0().truncate().child(label))
                        .child(button(("open-library-repo",index),"Open","",false).on_click(cx.listener(move|this,_,window,cx|{window.close_dialog(cx);let _=this.owner.update(cx,|owner,cx|owner.open_repository_tab(path.clone(),window,cx));})))
                        .child(button(("remove-library-repo",index),"Remove","",false).tooltip("Remove from the local library; keep its folder and drafts").on_click(cx.listener(move|this,_,window,cx|{this.session.library.retain(|entry|entry.path.path()!=remove);let _=this.owner.update(cx,|owner,cx|{owner.repository_tabs.library.retain(|entry|entry.path.path()!=remove);owner.save_repository_session(window,cx);});cx.notify();})))
                })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

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
        let file = File::create(&path).unwrap();
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
            parent: 1,
            zoom: 2.5,
            text_mode: "markdown".into(),
            compare: true,
            ..Default::default()
        };
        let restored: Bookmark =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(restored.comparison.unwrap().file(), file);
        assert_eq!(restored.selection.unwrap().commit(), commit("merge"));
        assert_eq!(restored.origins, saved.origins);
        assert_eq!(restored.parent, 1);
        assert_eq!(restored.zoom, 2.5);
        assert_eq!(restored.text_mode, "markdown");
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
