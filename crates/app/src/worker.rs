use crate::{graph, text::PatchPresentation};
use anyhow::{Result, anyhow, ensure};
use futures::channel::oneshot;
use gitturtle_core::{Branch, Commit, FileChange, GitRepository, TextPreview, Worktree};
use gitturtle_preview::{
    ImagePreview, MAX_INPUT_BYTES, decode_image, detect_lfs_pointer, is_image_path,
};
use gpui_kit::RenderImage;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const PREVIEW_EDGE: u32 = 1600;
const CACHE_BYTE_LIMIT: usize = 128 * 1024 * 1024;
const CACHE_ENTRY_LIMIT: usize = 32;
const GRAPH_LANE_LIMIT: usize = 128;
const GRAPH_EDGE_LIMIT: usize = 200_000;

pub struct Snapshot {
    pub repository: GitRepository,
    pub branches: Vec<Branch>,
    pub worktrees: Vec<Worktree>,
    pub commits: Vec<Commit>,
    pub graph: Vec<graph::GraphRow>,
    pub graph_notice: Option<String>,
    pub refs: HashMap<String, Vec<String>>,
    pub elapsed: Duration,
}

pub struct ImageSide {
    pub image: Option<ImagePreview>,
    /// BGRA conversion happens on the worker; the UI only shares this allocation.
    pub render: Option<Arc<RenderImage>>,
    pub message: Option<String>,
    pub bytes: usize,
}

pub enum Content {
    Conflict(Arc<crate::conflicts::Presentation>),
    Text {
        patch: String,
        old: String,
        new: String,
        presentation: Arc<PatchPresentation>,
        split: Arc<crate::split_diff::SplitPresentation>,
        partial: Option<Arc<crate::partial_view::PartialActions>>,
        partial_unavailable: Option<String>,
    },
    Images {
        old: ImageSide,
        new: ImageSide,
    },
    Notice(String),
}

impl Content {
    /// CPU allocations retained by a cache entry. UI-held Arcs and uploaded GPU
    /// textures have independent lifetimes and need their own presentation budget.
    fn bytes(&self) -> usize {
        match self {
            Self::Text {
                patch,
                old,
                new,
                presentation,
                split,
                partial,
                partial_unavailable,
            } => {
                patch.capacity()
                    + old.capacity()
                    + new.capacity()
                    + presentation.retained_bytes()
                    + split.retained_bytes()
                    + partial.as_ref().map_or(0, |p| p.bytes())
                    + partial_unavailable.as_ref().map_or(0, String::capacity)
            }
            Self::Images { old, new } => [old, new]
                .iter()
                .map(|side| {
                    side.image
                        .as_ref()
                        .map_or(0, |image| image.rgba.capacity() + image.format.capacity())
                        + side
                            .render
                            .as_ref()
                            .and_then(|render| render.as_bytes(0))
                            .map_or(0, <[u8]>::len)
                        + side.message.as_ref().map_or(0, String::capacity)
                })
                .sum(),
            Self::Conflict(presentation) => presentation.bytes(),
            Self::Notice(message) => message.capacity(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Branch { name: String, remote: bool },
    Worktree { path: PathBuf },
}

pub enum Job {
    Blame {
        repo: GitRepository,
        target: gitturtle_core::BlameTarget,
    },
    LineHistory {
        repo: GitRepository,
        oid: String,
        path: PathBuf,
        line: usize,
    },
    SearchHistory {
        repo: GitRepository,
        scope: Option<Scope>,
        pinned: Option<gitturtle_core::HistoryScope>,
        query: String,
        offset: usize,
        limit: usize,
        previous: Vec<(String, Vec<String>)>,
        remaining_bytes: usize,
    },
    FileHistory {
        repo: GitRepository,
        anchor: String,
        path: PathBuf,
        offset: usize,
        limit: usize,
    },
    QuietRefresh {
        repo: GitRepository,
        scope: Option<Scope>,
        limit: usize,
        history: bool,
        selected: Option<WorkingSelection>,
    },
    WorkingPreview {
        repo: GitRepository,
        entry: gitturtle_core::StatusEntry,
        area: gitturtle_core::ChangeArea,
    },
    Open {
        path: PathBuf,
        scope: Option<Scope>,
        limit: usize,
    },
    Changes {
        repo: GitRepository,
        oid: String,
        parent: usize,
    },
    Preview {
        repo: GitRepository,
        file: FileChange,
    },
}

pub enum Output {
    Blame(gitturtle_core::Blame),
    LineHistory(gitturtle_core::LineHistory),
    SearchHistory(SearchResult),
    FileHistory(gitturtle_core::FileHistoryPage),
    QuietRefresh(Box<QuietRefresh>),
    WorkingPreview(FileChange, Arc<Content>, Duration),
    Snapshot(Snapshot),
    Changes(Vec<FileChange>, Duration),
    Preview(Arc<Content>, Duration),
}

pub struct SearchResult {
    pub page: gitturtle_core::HistorySearchPage,
    pub graph: Vec<graph::GraphRow>,
    pub graph_notice: Option<String>,
    pub retained_bytes: usize,
}

pub struct WorkingState {
    pub status: gitturtle_core::RepositoryStatus,
    pub profile: Result<gitturtle_core::GitProfile>,
    pub remotes: Result<Vec<gitturtle_core::Remote>>,
    pub operation: Result<Option<gitturtle_core::OperationState>>,
}

impl WorkingState {
    pub fn read(repo: &GitRepository) -> Result<Self> {
        Self::read_with_checkpoint(repo, || Ok(()))
    }

    fn read_with_checkpoint(
        repo: &GitRepository,
        checkpoint: impl Fn() -> Result<()>,
    ) -> Result<Self> {
        checkpoint()?;
        let status = repo.status()?;
        checkpoint()?;
        let profile = repo.profile();
        checkpoint()?;
        let remotes = repo.remotes();
        checkpoint()?;
        let operation = repo.operation_state();
        checkpoint()?;
        Ok(Self {
            status,
            profile,
            remotes,
            operation,
        })
    }
}

pub struct WorkingSelection {
    pub path: PathBuf,
    pub area: gitturtle_core::ChangeArea,
    pub file: Option<FileChange>,
    pub content: Option<Arc<Content>>,
}

pub struct QuietRefresh {
    pub working: WorkingState,
    pub snapshot: Option<Result<QuietHistory>>,
    pub preview: Option<Result<QuietPreview>>,
}

pub enum QuietHistory {
    Refreshed(Snapshot),
    /// A branch/worktree scope can disappear externally. Its existing immutable
    /// history remains usable while fresh branch and worktree metadata is shown.
    Retained {
        metadata: Snapshot,
        error: String,
    },
}

pub enum QuietPreview {
    Unchanged,
    Absent,
    Changed {
        file: FileChange,
        content: Arc<Content>,
    },
}

struct Request {
    job: Job,
    generation: u64,
    reply: oneshot::Sender<Result<Output>>,
    cancellation: gitturtle_core::HistoryCancellation,
}

struct Queue {
    pending: Option<Request>,
    closed: bool,
    startup_error: Option<String>,
    active: Option<gitturtle_core::HistoryCancellation>,
}

struct Cancellation {
    generation: u64,
    latest: Arc<AtomicU64>,
    history: gitturtle_core::HistoryCancellation,
}

impl Cancellation {
    fn check(&self) -> Result<()> {
        ensure!(
            self.latest.load(Ordering::Acquire) == self.generation && !self.history.is_cancelled(),
            "Repository request superseded by a newer selection"
        );
        Ok(())
    }
}

/// One active read and one replaceable pending request. Replacement cancels a
/// pending reply immediately; active work stops at the next bounded checkpoint.
/// History search/file-history Git processes are actively terminated; existing
/// object reads and decoders retain their individual deadlines/checkpoints.
/// The UI must retain its own generation checks before presenting responses.
/// Dropping the worker never waits for an active filesystem or decoder call.
pub struct Worker {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    latest: Arc<AtomicU64>,
}

impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}

impl Worker {
    pub fn new() -> Self {
        // The session is owned by the executor on its reader thread. UI clones
        // can be cleared without closing/waiting for the shared cat-file child.
        let mut session = RepositorySession::default();
        Self::with_executor(move |job, cache, cancellation| {
            execute(job, cache, cancellation, &mut session)
        })
    }

    fn with_executor(
        mut executor: impl FnMut(Job, &mut PreviewCache, &Cancellation) -> Result<Output>
        + Send
        + 'static,
    ) -> Self {
        let queue = Arc::new((
            Mutex::new(Queue {
                pending: None,
                closed: false,
                startup_error: None,
                active: None,
            }),
            Condvar::new(),
        ));
        let latest = Arc::new(AtomicU64::new(0));
        let worker_queue = Arc::clone(&queue);
        let worker_latest = Arc::clone(&latest);
        let spawn = std::thread::Builder::new()
            .name("gitturtle-reader".into())
            .spawn(move || {
                let mut cache = PreviewCache::default();
                loop {
                    let request = {
                        let (lock, ready) = &*worker_queue;
                        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                        while state.pending.is_none() && !state.closed {
                            state = ready.wait(state).unwrap_or_else(|error| error.into_inner());
                        }
                        if state.closed {
                            break;
                        }
                        let request = state.pending.take().expect("pending request checked above");
                        state.active = Some(request.cancellation.clone());
                        request
                    };
                    if request.reply.is_canceled() {
                        continue;
                    }
                    let cancellation = Cancellation {
                        generation: request.generation,
                        latest: Arc::clone(&worker_latest),
                        history: request.cancellation,
                    };
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        cancellation.check()?;
                        executor(request.job, &mut cache, &cancellation)
                    }));
                    let result = match result {
                        Ok(result) => result,
                        Err(_) => {
                            // A failed decoder must not silently kill the sole
                            // worker or leave partially mutated cache accounting.
                            cache = PreviewCache::default();
                            Err(anyhow!(
                                "Preview processing failed unexpectedly. Select another file or reopen the repository."
                            ))
                        }
                    };
                    let _ = request.reply.send(result);
                }
            });
        if let Err(error) = spawn {
            let mut state = queue.0.lock().unwrap_or_else(|error| error.into_inner());
            state.closed = true;
            state.startup_error = Some(format!("Cannot start repository worker: {error}"));
        }
        Self { queue, latest }
    }

    /// Invalidate mutable reads even when there is no replacement file to load.
    /// Active work stops at its next checkpoint; queued work is dropped now.
    pub fn cancel(&self) {
        let (lock, _) = &*self.queue;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        self.latest.fetch_add(1, Ordering::AcqRel);
        if let Some(active) = state.active.take() {
            active.cancel();
        }
        state.pending = None;
    }

    pub fn submit(&self, job: Job) -> oneshot::Receiver<Result<Output>> {
        let (reply, receiver) = oneshot::channel();
        let (lock, ready) = &*self.queue;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            let _ = reply.send(Err(anyhow!(
                "{}",
                state
                    .startup_error
                    .as_deref()
                    .unwrap_or("Repository worker is closed")
            )));
            return receiver;
        }
        // Allocate the sequence while holding the queue lock, so concurrent
        // submitters cannot replace a newer request with an older sequence.
        let generation = self.latest.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        if let Some(active) = state.active.take() {
            active.cancel();
        }
        state.pending = Some(Request {
            job,
            generation,
            reply,
            cancellation: gitturtle_core::HistoryCancellation::default(),
        });
        ready.notify_one();
        receiver
    }
}

struct RetainedRepository {
    canonical_root: PathBuf,
    repository: Arc<GitRepository>,
}

/// Retain one current worktree session, independently of preview eviction and
/// UI selection. Replacing it and dropping its process-owning Git handle happen
/// on the worker. Linked worktrees never share a session merely because their
/// common object directory is the same: HEAD and local configuration differ.
#[derive(Default)]
struct RepositorySession {
    current: Option<RetainedRepository>,
}

impl RepositorySession {
    fn open(&mut self, requested: &Path) -> Result<GitRepository> {
        let canonical = requested.canonicalize()?;
        if let Some(current) = &self.current
            && current.canonical_root == canonical
        {
            return Ok(current.repository.as_ref().clone());
        }
        // A nested directory needs Git discovery; it may still resolve to the
        // existing root, in which case keep the original persistent reader.
        let discovered = GitRepository::open(&canonical)?;
        let canonical_root = discovered.path().canonicalize()?;
        if let Some(current) = &self.current
            && current.canonical_root == canonical_root
        {
            return Ok(current.repository.as_ref().clone());
        }
        let repository = Arc::new(discovered);
        self.current = Some(RetainedRepository {
            canonical_root,
            repository: Arc::clone(&repository),
        });
        Ok(repository.as_ref().clone())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, ready) = &*self.queue;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        self.latest.fetch_add(1, Ordering::AcqRel);
        if let Some(active) = state.active.take() {
            active.cancel();
        }
        state.closed = true;
        state.pending = None;
        ready.notify_one();
    }
}

/// Both filenames affect text patch headers and the image decoder hint. Store
/// byte-safe paths as structured fields instead of concatenating display strings.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct PreviewKey {
    repository: PathBuf,
    old_oid: Option<String>,
    new_oid: Option<String>,
    old_path: Option<PathBuf>,
    new_path: Option<PathBuf>,
    old_mode: String,
    new_mode: String,
    preview_edge: u32,
}

impl PreviewKey {
    fn new(repository: &Path, file: &FileChange) -> Self {
        Self {
            repository: repository.into(),
            old_oid: file.old_oid.clone(),
            new_oid: file.new_oid.clone(),
            old_path: file.old_path.clone(),
            new_path: file.new_path.clone(),
            old_mode: file.old_mode.clone(),
            new_mode: file.new_mode.clone(),
            preview_edge: PREVIEW_EDGE,
        }
    }
}

struct PreviewCache {
    entries: HashMap<PreviewKey, Arc<Content>>,
    lru: VecDeque<PreviewKey>,
    bytes: usize,
    byte_limit: usize,
    entry_limit: usize,
}

impl Default for PreviewCache {
    fn default() -> Self {
        Self::with_limits(CACHE_BYTE_LIMIT, CACHE_ENTRY_LIMIT)
    }
}

impl PreviewCache {
    fn with_limits(byte_limit: usize, entry_limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            lru: VecDeque::new(),
            bytes: 0,
            byte_limit,
            entry_limit,
        }
    }

    fn get(&mut self, key: &PreviewKey) -> Option<Arc<Content>> {
        let value = Arc::clone(self.entries.get(key)?);
        self.lru.retain(|existing| existing != key);
        self.lru.push_back(key.clone());
        Some(value)
    }

    fn remove(&mut self, key: &PreviewKey) {
        if let Some(value) = self.entries.remove(key) {
            self.bytes -= value.bytes();
        }
        self.lru.retain(|existing| existing != key);
    }

    fn insert(&mut self, key: PreviewKey, content: Arc<Content>) {
        self.remove(&key);
        let bytes = content.bytes();
        if bytes > self.byte_limit || self.entry_limit == 0 {
            return;
        }
        while self.bytes + bytes > self.byte_limit || self.entries.len() >= self.entry_limit {
            let Some(old) = self.lru.front().cloned() else {
                break;
            };
            self.remove(&old);
        }
        self.bytes += bytes;
        self.lru.push_back(key.clone());
        self.entries.insert(key, content);
    }
}

fn execute(
    job: Job,
    cache: &mut PreviewCache,
    cancellation: &Cancellation,
    session: &mut RepositorySession,
) -> Result<Output> {
    let start = Instant::now();
    cancellation.check()?;
    match job {
        Job::SearchHistory {
            repo,
            scope,
            pinned,
            query,
            offset,
            limit,
            previous,
            remaining_bytes,
        } => {
            let pinned = if let Some(pinned) = pinned {
                pinned
            } else {
                let branches = repo.branches()?;
                cancellation.check()?;
                let worktrees = repo.worktrees()?;
                cancellation.check()?;
                match scope_oid(scope.as_ref(), &branches, &worktrees)? {
                    Some(oid) if oid.bytes().all(|byte| byte == b'0') => {
                        gitturtle_core::HistoryScope::PinnedRefs(Vec::new())
                    }
                    Some(oid) => gitturtle_core::HistoryScope::FromCommit(oid.to_owned()),
                    None => gitturtle_core::HistoryScope::AllRefs,
                }
            };
            let page =
                repo.search_history(&pinned, &query, offset, limit, &cancellation.history)?;
            cancellation.check()?;
            let retained_bytes = page
                .commits
                .iter()
                .map(commit_metadata_bytes)
                .sum::<usize>();
            ensure!(
                retained_bytes <= remaining_bytes,
                "Search results exceed the 64 MiB display budget. Narrow the query to inspect these matches."
            );
            let mut topology = previous
                .into_iter()
                .map(|(oid, parents)| Commit {
                    oid,
                    parents,
                    author: String::new(),
                    timestamp: 0,
                    subject: String::new(),
                    body: String::new(),
                })
                .collect::<Vec<_>>();
            topology.extend(page.commits.iter().map(|commit| Commit {
                oid: commit.oid.clone(),
                parents: commit.parents.clone(),
                author: String::new(),
                timestamp: 0,
                subject: String::new(),
                body: String::new(),
            }));
            let (graph, graph_notice) = layout_graph(&topology, cancellation)?;
            Ok(Output::SearchHistory(SearchResult {
                page,
                graph,
                graph_notice,
                retained_bytes,
            }))
        }
        Job::Blame { repo, target } => {
            let blame = repo.blame(&target, &cancellation.history)?;
            cancellation.check()?;
            Ok(Output::Blame(blame))
        }
        Job::LineHistory {
            repo,
            oid,
            path,
            line,
        } => {
            let history = repo.line_history(&oid, &path, line, &cancellation.history)?;
            cancellation.check()?;
            Ok(Output::LineHistory(history))
        }
        Job::FileHistory {
            repo,
            anchor,
            path,
            offset,
            limit,
        } => {
            let page = repo.file_history(&anchor, &path, offset, limit, &cancellation.history)?;
            cancellation.check()?;
            Ok(Output::FileHistory(page))
        }
        Job::QuietRefresh {
            repo,
            scope,
            limit,
            history,
            selected,
        } => {
            let working = WorkingState::read_with_checkpoint(&repo, || cancellation.check())?;
            cancellation.check()?;
            let snapshot = history.then(|| {
                match read_snapshot(
                    repo.path().to_owned(),
                    scope.as_ref(),
                    limit,
                    cancellation,
                    session,
                ) {
                    Ok(snapshot) => Ok(QuietHistory::Refreshed(snapshot)),
                    Err(error) => {
                        cancellation.check()?;
                        // A zero-row unscoped snapshot reads current navigation
                        // metadata without loading a replacement history page.
                        let metadata =
                            read_snapshot(repo.path().to_owned(), None, 0, cancellation, session)?;
                        Ok(QuietHistory::Retained {
                            metadata,
                            error: format!("{error:#}"),
                        })
                    }
                }
            });
            cancellation.check()?;
            let preview = selected.map(
                |WorkingSelection {
                     path,
                     area: previous_area,
                     file: previous_file,
                     content: previous_content,
                 }| {
                    let Some(entry) = working
                        .status
                        .entries
                        .iter()
                        .find(|entry| entry.path == path)
                    else {
                        return Ok(QuietPreview::Absent);
                    };
                    let area = crate::workspace::retained_area(
                        previous_area,
                        entry.staged.is_some(),
                        entry.unstaged.is_some() || entry.untracked || entry.conflicted,
                    );
                    let Some(area) = area else {
                        return Ok(QuietPreview::Absent);
                    };
                    let Output::WorkingPreview(file, content, _) = execute(
                        Job::WorkingPreview {
                            repo: repo.clone(),
                            entry: entry.clone(),
                            area,
                        },
                        cache,
                        cancellation,
                        session,
                    )?
                    else {
                        unreachable!()
                    };
                    if area == previous_area
                        && previous_file.as_ref() == Some(&file)
                        && previous_content
                            .as_ref()
                            .is_some_and(|previous| content_unchanged(previous, &content))
                    {
                        Ok(QuietPreview::Unchanged)
                    } else {
                        Ok(QuietPreview::Changed { file, content })
                    }
                },
            );
            cancellation.check()?;
            Ok(Output::QuietRefresh(Box::new(QuietRefresh {
                working,
                snapshot,
                preview,
            })))
        }
        Job::WorkingPreview { repo, entry, area } => {
            if entry.conflicted {
                let presentation =
                    crate::conflicts::Presentation::prepare(repo.conflict_preview(&entry.path)?);
                cancellation.check()?;
                return Ok(Output::WorkingPreview(
                    presentation.file(),
                    Arc::new(Content::Conflict(Arc::new(presentation))),
                    start.elapsed(),
                ));
            }
            let preview = repo.worktree_preview(&entry, area)?;
            cancellation.check()?;
            let file = preview.file;
            let partial_diff = preview.partial;
            let mut unavailable = preview.partial_unavailable;
            let mut content = if image_change(&file) {
                let old_name = file
                    .old_path
                    .as_deref()
                    .unwrap_or(file.path())
                    .to_string_lossy();
                let new_name = file
                    .new_path
                    .as_deref()
                    .unwrap_or(file.path())
                    .to_string_lossy();
                let old = working_image_side(
                    &repo,
                    preview.old,
                    file.old_path.is_some(),
                    &old_name,
                    cancellation,
                );
                cancellation.check()?;
                let new = working_image_side(
                    &repo,
                    preview.new,
                    file.new_path.is_some(),
                    &new_name,
                    cancellation,
                );
                Content::Images { old, new }
            } else {
                match preview.preview {
                    TextPreview::Patch(patch) => prepared_text(
                        patch,
                        String::from_utf8(preview.old)?,
                        String::from_utf8(preview.new)?,
                    ),
                    TextPreview::Binary => {
                        Content::Notice("Binary file · no text comparison is available.".into())
                    }
                    TextPreview::TooLarge {
                        old_bytes,
                        new_bytes,
                    } => Content::Notice(format!(
                        "Text preview exceeds the display budget · before {old_bytes} bytes · after {new_bytes} bytes."
                    )),
                    TextPreview::Submodule { old_oid, new_oid } => Content::Notice(format!(
                        "Submodule · {} → {}",
                        old_oid.as_deref().unwrap_or("absent"),
                        new_oid.as_deref().unwrap_or("absent")
                    )),
                }
            };
            cancellation.check()?;
            if let Content::Text {
                patch,
                presentation,
                partial,
                partial_unavailable,
                ..
            } = &mut content
            {
                if let Some(diff) = partial_diff {
                    match crate::partial_view::prepare(patch, presentation, Arc::new(diff)) {
                        Ok(actions) => *partial = Some(Arc::new(actions)),
                        Err(error) => unavailable = Some(error.to_string()),
                    }
                }
                *partial_unavailable = unavailable;
            }
            cancellation.check()?;
            // Mutable worktree/index comparisons are deliberately not cached.
            Ok(Output::WorkingPreview(
                file,
                Arc::new(content),
                start.elapsed(),
            ))
        }
        Job::Open { path, scope, limit } => {
            let mut snapshot = read_snapshot(path, scope.as_ref(), limit, cancellation, session)?;
            cancellation.check()?;
            snapshot.elapsed = start.elapsed();
            Ok(Output::Snapshot(snapshot))
        }
        Job::Changes { repo, oid, parent } => {
            let changes = repo.changes_with_renames(&oid, parent)?;
            cancellation.check()?;
            Ok(Output::Changes(changes, start.elapsed()))
        }
        Job::Preview { repo, file } => {
            let key = PreviewKey::new(repo.path(), &file);
            if let Some(content) = cache.get(&key) {
                return Ok(Output::Preview(content, start.elapsed()));
            }
            let (content, cacheable) = if image_change(&file) {
                let old_name = file
                    .old_path
                    .as_deref()
                    .unwrap_or(file.path())
                    .to_string_lossy();
                let new_name = file
                    .new_path
                    .as_deref()
                    .unwrap_or(file.path())
                    .to_string_lossy();
                let (old, old_cacheable) =
                    image_side(&repo, file.old_oid.as_deref(), &old_name, cancellation);
                cancellation.check()?;
                let (new, new_cacheable) =
                    image_side(&repo, file.new_oid.as_deref(), &new_name, cancellation);
                (Content::Images { old, new }, old_cacheable && new_cacheable)
            } else {
                (text_content(&repo, &file, cancellation)?, true)
            };
            cancellation.check()?;
            let content = Arc::new(content);
            if cacheable {
                cache.insert(key, Arc::clone(&content));
            }
            Ok(Output::Preview(content, start.elapsed()))
        }
    }
}

fn commit_metadata_bytes(commit: &Commit) -> usize {
    std::mem::size_of::<Commit>()
        + commit.oid.capacity()
        + commit.parents.iter().map(String::capacity).sum::<usize>()
        + commit.author.capacity()
        + commit.subject.capacity()
        + commit.body.capacity()
}

fn read_snapshot(
    path: PathBuf,
    scope: Option<&Scope>,
    limit: usize,
    cancellation: &Cancellation,
    session: &mut RepositorySession,
) -> Result<Snapshot> {
    let start = Instant::now();
    cancellation.check()?;
    let repository = session.open(&path)?;
    cancellation.check()?;
    let branches = repository.branches()?;
    cancellation.check()?;
    let worktrees = repository.worktrees()?;
    cancellation.check()?;
    let oid = scope_oid(scope, &branches, &worktrees)?;
    let commits = if let Some(oid) = oid {
        // Git reports an all-zero HEAD for a worktree on an unborn branch.
        if oid.bytes().all(|byte| byte == b'0') {
            Vec::new()
        } else {
            repository.history_from(oid, limit)?
        }
    } else {
        repository.history(limit)?
    };
    cancellation.check()?;
    let (graph, graph_notice) = layout_graph(&commits, cancellation)?;
    cancellation.check()?;
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    for branch in &branches {
        refs.entry(branch.oid.clone())
            .or_default()
            .push(branch.name.clone());
    }
    cancellation.check()?;
    Ok(Snapshot {
        repository,
        branches,
        worktrees,
        commits,
        graph,
        graph_notice,
        refs,
        elapsed: start.elapsed(),
    })
}

fn layout_graph(
    commits: &[Commit],
    cancellation: &Cancellation,
) -> Result<(Vec<graph::GraphRow>, Option<String>)> {
    if graph_within_budget(commits, cancellation)? {
        return Ok((graph::layout(commits, || cancellation.check())?, None));
    }
    // Never truncate individual edges: that would imply incorrect ancestry.
    // A node-only fallback keeps the full history list and inspectors usable.
    let node = graph::GraphRow {
        width: 1,
        ..Default::default()
    };
    let rows = commits
        .iter()
        .map(|_| {
            cancellation.check()?;
            Ok(node.clone())
        })
        .collect::<Result<_>>()?;
    Ok((
        rows,
        Some(
            "Graph connections hidden for this history. Select a branch to view a smaller graph."
                .into(),
        ),
    ))
}

/// Preflight the graph's frontier and exact edge count without building lane
/// geometry. This bounds both the quadratic wide-history case and unusual
/// commits containing enormous parent lists before calling the layout engine.
fn graph_within_budget(commits: &[Commit], cancellation: &Cancellation) -> Result<bool> {
    let mut frontier = HashSet::<&str>::new();
    let mut parents = HashSet::new();
    let mut edges = 0usize;
    let mut parent_entries = 0usize;
    for commit in commits {
        cancellation.check()?;
        let before_width = frontier.len() + usize::from(!frontier.contains(commit.oid.as_str()));
        if before_width > GRAPH_LANE_LIMIT {
            return Ok(false);
        }
        frontier.remove(commit.oid.as_str());
        parents.clear();
        for parent in &commit.parents {
            parent_entries += 1;
            if parent_entries > GRAPH_EDGE_LIMIT {
                return Ok(false);
            }
            parents.insert(parent.as_str());
            frontier.insert(parent.as_str());
            if frontier.len() > GRAPH_LANE_LIMIT {
                return Ok(false);
            }
        }
        edges += before_width - 1 + parents.len();
        if edges > GRAPH_EDGE_LIMIT {
            return Ok(false);
        }
    }
    Ok(true)
}

fn scope_oid<'a>(
    scope: Option<&Scope>,
    branches: &'a [Branch],
    worktrees: &'a [Worktree],
) -> Result<Option<&'a str>> {
    match scope {
        None => Ok(None),
        Some(Scope::Branch { name, remote }) => branches
            .iter()
            .find(|branch| branch.name == *name && branch.remote == *remote)
            .map(|branch| Some(branch.oid.as_str()))
            .ok_or_else(|| {
                anyhow!(
                    "{} branch '{name}' no longer exists in the local repository snapshot",
                    if *remote { "Remote-tracking" } else { "Local" }
                )
            }),
        Some(Scope::Worktree { path }) => worktrees
            .iter()
            .find(|worktree| worktree.path == *path)
            .map(|worktree| Some(worktree.oid.as_str()))
            .ok_or_else(|| {
                anyhow!(
                    "Worktree '{}' is no longer registered in the local repository snapshot",
                    path.display()
                )
            }),
    }
}

fn image_change(file: &FileChange) -> bool {
    !file.is_submodule()
        && file.old_mode != "120000"
        && file.new_mode != "120000"
        && [file.old_path.as_deref(), file.new_path.as_deref()]
            .into_iter()
            .flatten()
            .any(is_image_path)
}

fn prepared_text(patch: String, old: String, new: String) -> Content {
    let presentation = Arc::new(PatchPresentation::prepare(&patch));
    let split = Arc::new(crate::split_diff::SplitPresentation::prepare(
        &old,
        &new,
        &presentation,
    ));
    Content::Text {
        patch,
        old,
        new,
        presentation,
        split,
        partial: None,
        partial_unavailable: None,
    }
}

/// Compare prepared mutable content off UI before deciding whether native
/// editors, image views, or partial selection snapshots need replacement.
fn content_unchanged(previous: &Content, next: &Content) -> bool {
    match (previous, next) {
        (
            Content::Text {
                patch: a,
                old: ao,
                new: an,
                partial_unavailable: au,
                partial: ap,
                ..
            },
            Content::Text {
                patch: b,
                old: bo,
                new: bn,
                partial_unavailable: bu,
                partial: bp,
                ..
            },
        ) => {
            a == b
                && ao == bo
                && an == bn
                && au == bu
                && ap.as_ref().map(|partial| &partial.diff)
                    == bp.as_ref().map(|partial| &partial.diff)
        }
        (Content::Notice(a), Content::Notice(b)) => a == b,
        (Content::Conflict(a), Content::Conflict(b)) => a.snapshot == b.snapshot,
        (Content::Images { old: ao, new: an }, Content::Images { old: bo, new: bn }) => {
            [ao, an].into_iter().zip([bo, bn]).all(|(a, b)| {
                a.bytes == b.bytes
                    && a.message == b.message
                    && match (&a.image, &b.image) {
                        (None, None) => true,
                        (Some(a), Some(b)) => {
                            a.width == b.width
                                && a.height == b.height
                                && a.original_width == b.original_width
                                && a.original_height == b.original_height
                                && a.format == b.format
                                && a.rgba == b.rgba
                        }
                        _ => false,
                    }
            })
        }
        _ => false,
    }
}

fn text_content(
    repo: &GitRepository,
    file: &FileChange,
    cancellation: &Cancellation,
) -> Result<Content> {
    let sources = repo.text_preview_with_sources(file)?;
    cancellation.check()?;
    Ok(match sources.preview {
        TextPreview::Patch(patch) => prepared_text(patch, String::from_utf8(sources.old)?, String::from_utf8(sources.new)?),
        TextPreview::Binary => Content::Notice(
            "Binary or non-UTF-8 file changed. Raw object IDs and file metadata are available above.".into(),
        ),
        TextPreview::TooLarge { old_bytes, new_bytes } => Content::Notice(format!(
            "Large file · {old_bytes} → {new_bytes} bytes\nThis change exceeds the automatic text preview budget. File metadata remains available."
        )),
        TextPreview::Submodule { old_oid, new_oid } => Content::Notice(format!(
            "Submodule reference changed\n\nBefore: {}\nAfter: {}",
            old_oid.as_deref().unwrap_or("Absent"), new_oid.as_deref().unwrap_or("Absent")
        )),
    })
}

/// Never cache an unavailable preview: the local object may appear before
/// another click. Successful LFS reads are size/hash verified by the core, so
/// their decoded content is immutable and safe to retain under the pointer OID.
fn image_side(
    repo: &GitRepository,
    oid: Option<&str>,
    file_name: &str,
    cancellation: &Cancellation,
) -> (ImageSide, bool) {
    let Some(oid) = oid else {
        return (
            ImageSide {
                image: None,
                render: None,
                message: None,
                bytes: 0,
            },
            true,
        );
    };
    let result = (|| -> Result<(ImagePreview, Arc<RenderImage>, usize)> {
        cancellation.check()?;
        let size = repo.blob_size(oid)?;
        ensure!(
            size <= MAX_INPUT_BYTES,
            "Image exceeds the 32 MiB input limit ({size} bytes)"
        );
        cancellation.check()?;
        let mut bytes = repo.blob(oid)?;
        cancellation.check()?;
        if let Some(pointer) = detect_lfs_pointer(&bytes) {
            bytes = repo
                .local_lfs_object(&pointer.oid, pointer.size, MAX_INPUT_BYTES)?
                .ok_or_else(|| anyhow!(
                    "Git LFS object is unavailable in the local store · {} bytes\nNo download was attempted.\n{}",
                    pointer.size,
                    pointer.oid
                ))?;
        }
        cancellation.check()?;
        let decoded = decode_image(&bytes, file_name, PREVIEW_EDGE)?;
        cancellation.check()?;
        let render = render_image(&decoded)?;
        Ok((decoded, render, bytes.len()))
    })();
    match result {
        Ok((image, render, bytes)) => (
            ImageSide {
                image: Some(image),
                render: Some(render),
                message: None,
                bytes,
            },
            true,
        ),
        Err(error) => (
            ImageSide {
                image: None,
                render: None,
                message: Some(format!("{error:#}")),
                bytes: 0,
            },
            false,
        ),
    }
}

fn working_image_side(
    repo: &GitRepository,
    mut bytes: Vec<u8>,
    present: bool,
    name: &str,
    cancellation: &Cancellation,
) -> ImageSide {
    if !present {
        return ImageSide {
            image: None,
            render: None,
            message: None,
            bytes: 0,
        };
    }
    let result = (|| -> Result<(ImagePreview, Arc<RenderImage>, usize)> {
        cancellation.check()?;
        ensure!(
            bytes.len() <= MAX_INPUT_BYTES,
            "Image exceeds the 32 MiB input limit"
        );
        if let Some(pointer) = detect_lfs_pointer(&bytes) {
            bytes = repo.local_lfs_object(&pointer.oid, pointer.size, MAX_INPUT_BYTES)?.ok_or_else(|| anyhow!("Git LFS object is unavailable locally · {} bytes. No download was attempted.", pointer.size))?;
        }
        cancellation.check()?;
        let decoded = decode_image(&bytes, name, PREVIEW_EDGE)?;
        cancellation.check()?;
        let render = render_image(&decoded)?;
        Ok((decoded, render, bytes.len()))
    })();
    match result {
        Ok((image, render, bytes)) => ImageSide {
            image: Some(image),
            render: Some(render),
            message: None,
            bytes,
        },
        Err(error) => ImageSide {
            image: None,
            render: None,
            message: Some(format!("{error:#}")),
            bytes: 0,
        },
    }
}

fn render_image(preview: &ImagePreview) -> Result<Arc<RenderImage>> {
    // GPUI RenderImage expects BGRA although image::Frame names the buffer RGBA.
    let mut bgra = preview.rgba.clone();
    let (pixels, remainder) = bgra.as_chunks_mut::<4>();
    ensure!(remainder.is_empty(), "Invalid preview pixel buffer");
    for pixel in pixels {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(preview.width, preview.height, bgra)
        .ok_or_else(|| anyhow!("Invalid preview dimensions"))?;
    Ok(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use gitturtle_core::ChangeStatus;
    use std::sync::mpsc;

    fn open_job(name: &str) -> Job {
        Job::Open {
            path: name.into(),
            scope: None,
            limit: 10,
        }
    }

    fn job_name(job: Job) -> String {
        match job {
            Job::Open { path, .. } => path.to_string_lossy().into_owned(),
            _ => panic!("test executor expects an Open job"),
        }
    }

    fn notice(message: &str) -> Output {
        Output::Preview(Arc::new(Content::Notice(message.into())), Duration::ZERO)
    }

    fn message(output: Output) -> String {
        match output {
            Output::Preview(content, _) => match &*content {
                Content::Notice(message) => message.clone(),
                _ => panic!("expected notice"),
            },
            _ => panic!("expected preview output"),
        }
    }

    fn file() -> FileChange {
        FileChange {
            old_path: Some("before.txt".into()),
            new_path: Some("after.txt".into()),
            old_oid: Some("1".repeat(40)),
            new_oid: Some("2".repeat(40)),
            status: ChangeStatus::Renamed,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
        }
    }

    fn key(name: &str) -> PreviewKey {
        let mut file = file();
        file.new_path = Some(name.into());
        PreviewKey::new(Path::new("/repository"), &file)
    }

    fn content(bytes: usize) -> Arc<Content> {
        Arc::new(Content::Notice("x".repeat(bytes)))
    }

    #[test]
    fn queue_replaces_pending_and_cancels_active_at_checkpoint() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = Worker::with_executor(move |job, _, cancellation| {
            let name = job_name(job);
            started_tx.send(name.clone())?;
            if name == "first" {
                release_rx.recv_timeout(Duration::from_secs(2))?;
            }
            cancellation.check()?;
            Ok(notice(&name))
        });
        let first = worker.submit(open_job("first"));
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "first"
        );
        let mut second = worker.submit(open_job("second"));
        let third = worker.submit(open_job("third"));
        // Replacement drops the pending reply synchronously and never executes it.
        assert!(second.try_recv().is_err());
        release_tx.send(()).unwrap();
        let first_error = block_on(first).unwrap().err().expect("superseded request");
        assert!(first_error.to_string().contains("superseded"));
        assert_eq!(message(block_on(third).unwrap().unwrap()), "third");
        assert_eq!(
            started_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "third"
        );
        assert!(started_rx.try_recv().is_err());
    }

    #[test]
    fn explicit_cancel_invalidates_active_and_pending_without_replacement() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = Worker::with_executor(move |_, _, cancellation| {
            started_tx.send(())?;
            release_rx.recv_timeout(Duration::from_secs(2))?;
            cancellation.check()?;
            Ok(notice("stale preview"))
        });
        let active = worker.submit(open_job("active"));
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut pending = worker.submit(open_job("pending"));
        worker.cancel();
        assert!(pending.try_recv().is_err());
        release_tx.send(()).unwrap();
        assert!(block_on(active).unwrap().is_err());
        assert!(started_rx.try_recv().is_err());
    }

    #[test]
    fn dropping_worker_releases_pending_without_joining_active_work() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = Worker::with_executor(move |_, _, cancellation| {
            started_tx.send(())?;
            release_rx.recv_timeout(Duration::from_secs(2))?;
            cancellation.check()?;
            Ok(notice("finished"))
        });
        let active = worker.submit(open_job("active"));
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut pending = worker.submit(open_job("pending"));
        // This must return while the active job is still waiting on release_rx.
        drop(worker);
        assert!(pending.try_recv().is_err());
        release_tx.send(()).unwrap();
        assert!(block_on(active).unwrap().is_err());
        assert!(started_rx.try_recv().is_err());
    }

    #[test]
    fn decoder_panic_is_reported_and_worker_recovers_with_empty_cache() {
        let worker = Worker::with_executor(|job, cache, _| {
            let name = job_name(job);
            if name == "panic" {
                cache.insert(key("partial"), content(12));
                panic!("simulated decoder failure");
            }
            ensure!(cache.entries.is_empty(), "cache survives panic");
            ensure!(cache.bytes == 0, "cache accounting survives panic");
            Ok(notice("recovered"))
        });
        let error = block_on(worker.submit(open_job("panic")))
            .unwrap()
            .err()
            .expect("panic converted into error");
        assert!(error.to_string().contains("unexpectedly"));
        assert_eq!(
            message(block_on(worker.submit(open_job("next"))).unwrap().unwrap()),
            "recovered"
        );
    }

    #[test]
    fn text_cache_budget_includes_prepared_patch_metadata() {
        let patch = "@@ -1 +1 @@\n-old\n+new\n".to_owned();
        let old = "old\n".to_owned();
        let new = "new\n".to_owned();
        let source_bytes = patch.capacity() + old.capacity() + new.capacity();
        let presentation = Arc::new(PatchPresentation::prepare(&patch));
        let split = Arc::new(crate::split_diff::SplitPresentation::prepare(
            &old,
            &new,
            &presentation,
        ));
        let metadata_bytes = presentation.retained_bytes() + split.retained_bytes();
        let content = Arc::new(Content::Text {
            patch,
            old,
            new,
            presentation,
            split,
            partial: None,
            partial_unavailable: None,
        });
        assert_eq!(content.bytes(), source_bytes + metadata_bytes);

        let mut cache = PreviewCache::with_limits(source_bytes + metadata_bytes - 1, 1);
        cache.insert(key("text"), Arc::clone(&content));
        assert!(cache.get(&key("text")).is_none());
        assert_eq!(cache.bytes, 0);

        let mut cache = PreviewCache::with_limits(source_bytes + metadata_bytes, 1);
        cache.insert(key("text"), content);
        assert!(cache.get(&key("text")).is_some());
        assert_eq!(cache.bytes, source_bytes + metadata_bytes);
    }

    #[test]
    fn cache_eviction_respects_access_recency_and_content_bytes() {
        let value = content(8);
        let weight = value.bytes();
        let mut cache = PreviewCache::with_limits(weight * 2, 3);
        cache.insert(key("a"), Arc::clone(&value));
        cache.insert(key("b"), Arc::clone(&value));
        assert!(cache.get(&key("a")).is_some());
        cache.insert(key("c"), Arc::clone(&value));
        assert!(cache.get(&key("b")).is_none());
        assert!(cache.get(&key("a")).is_some());
        assert!(cache.get(&key("c")).is_some());
        assert_eq!(cache.bytes, weight * 2);
        assert_eq!(cache.lru.len(), 2);
    }

    #[test]
    fn duplicate_cache_insert_replaces_weight_and_has_one_lru_entry() {
        let mut cache = PreviewCache::with_limits(100, 3);
        cache.insert(key("a"), content(8));
        let replacement = content(12);
        cache.insert(key("a"), Arc::clone(&replacement));
        assert_eq!(cache.bytes, replacement.bytes());
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.lru.len(), 1);
        assert!(Arc::ptr_eq(&cache.get(&key("a")).unwrap(), &replacement));
    }

    #[test]
    fn cache_bounds_entries_and_rejects_oversized_replacements() {
        let mut cache = PreviewCache::with_limits(8, 2);
        cache.insert(key("a"), content(1));
        cache.insert(key("b"), content(1));
        cache.insert(key("c"), content(1));
        assert!(cache.get(&key("a")).is_none());
        assert_eq!(cache.entries.len(), 2);
        cache.insert(key("b"), content(9));
        assert!(cache.get(&key("b")).is_none());
        assert_eq!(cache.bytes, 1);
        let mut disabled = PreviewCache::with_limits(100, 0);
        disabled.insert(key("empty"), content(0));
        assert!(disabled.entries.is_empty());
    }

    #[test]
    fn cache_keys_include_both_paths_modes_objects_repository_and_options() {
        let original = key("after.txt");
        let mut alternatives = Vec::new();
        let mut changed = original.clone();
        changed.old_path = Some("renamed-from.txt".into());
        alternatives.push(changed);
        let mut changed = original.clone();
        changed.new_path = Some("renamed-to.txt".into());
        alternatives.push(changed);
        let mut changed = original.clone();
        changed.old_oid = None;
        alternatives.push(changed);
        let mut changed = original.clone();
        changed.new_oid = None;
        alternatives.push(changed);
        let mut changed = original.clone();
        changed.old_mode = "120000".into();
        alternatives.push(changed);
        let mut changed = original.clone();
        changed.new_mode = "100755".into();
        alternatives.push(changed);
        let mut changed = original.clone();
        changed.repository = "/another-repository".into();
        alternatives.push(changed);
        let mut changed = original.clone();
        changed.preview_edge = 800;
        alternatives.push(changed);
        for alternative in alternatives {
            assert_ne!(original, alternative);
        }
    }

    #[cfg(unix)]
    #[test]
    fn cache_keys_preserve_non_utf8_path_bytes() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let mut first = file();
        first.old_path = Some(OsString::from_vec(b"before-\xff.txt".to_vec()).into());
        let mut second = first.clone();
        second.old_path = Some(OsString::from_vec(b"before-\xfe.txt".to_vec()).into());
        assert_eq!(
            first.old_path.as_ref().unwrap().to_string_lossy(),
            second.old_path.as_ref().unwrap().to_string_lossy()
        );
        assert_ne!(
            PreviewKey::new(Path::new("/repository"), &first),
            PreviewKey::new(Path::new("/repository"), &second)
        );
    }

    #[test]
    fn image_detection_checks_both_names_and_excludes_gitlinks_and_symlinks() {
        let mut file = file();
        file.old_path = Some("before.PNG".into());
        file.new_path = Some("after.txt".into());
        assert!(image_change(&file));
        file.new_mode = "160000".into();
        assert!(!image_change(&file));
        file.new_mode = "120000".into();
        assert!(!image_change(&file));
        file.new_mode = "100644".into();
        file.old_mode = "120000".into();
        assert!(!image_change(&file));
        file.old_mode = "100644".into();
        file.old_path = None;
        assert!(!image_change(&file));
    }

    fn graph_commit(oid: String, parents: Vec<String>) -> Commit {
        Commit {
            oid,
            parents,
            author: String::new(),
            timestamp: 0,
            subject: String::new(),
            body: String::new(),
        }
    }

    fn active() -> Cancellation {
        Cancellation {
            generation: 1,
            latest: Arc::new(AtomicU64::new(1)),
            history: gitturtle_core::HistoryCancellation::default(),
        }
    }

    #[test]
    fn replacement_and_explicit_cancel_signal_an_active_core_history_token() {
        for replace in [true, false] {
            let (started, start) = mpsc::channel();
            let worker = Worker::with_executor(move |job, _, cancellation| {
                if job_name(job) == "active" {
                    started.send(()).unwrap();
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while !cancellation.history.is_cancelled() && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    ensure!(
                        cancellation.history.is_cancelled(),
                        "Active core read was never cancelled"
                    );
                    Ok(notice("cancelled"))
                } else {
                    Ok(notice("replacement"))
                }
            });
            let active = worker.submit(open_job("active"));
            start.recv_timeout(Duration::from_secs(1)).unwrap();
            let replacement = if replace {
                Some(worker.submit(open_job("replacement")))
            } else {
                worker.cancel();
                None
            };
            assert_eq!(message(block_on(active).unwrap().unwrap()), "cancelled");
            if let Some(replacement) = replacement {
                assert_eq!(
                    message(block_on(replacement).unwrap().unwrap()),
                    "replacement"
                );
            }
        }
    }

    #[test]
    fn graph_budget_preserves_full_linear_history_and_rejects_excessive_width() {
        let linear: Vec<_> = (0..10_000)
            .map(|i| graph_commit(format!("commit-{i}"), vec![format!("commit-{}", i + 1)]))
            .collect();
        let (rows, notice) = layout_graph(&linear, &active()).unwrap();
        assert!(notice.is_none());
        assert_eq!(rows.len(), 10_000);
        assert!(
            rows.iter()
                .all(|row| row.width == 1 && row.edges.len() == 1)
        );

        let wide = [graph_commit(
            "merge".into(),
            (0..129).map(|i| format!("parent-{i}")).collect(),
        )];
        let (rows, notice) = layout_graph(&wide, &active()).unwrap();
        assert!(notice.is_some());
        assert_eq!(rows.len(), wide.len());
        assert!(
            rows.iter()
                .all(|row| row.edges.is_empty() && !row.incoming && row.width == 1)
        );
    }

    #[test]
    fn graph_budget_bounds_continuation_edges_even_below_lane_limit() {
        let mut commits = vec![graph_commit(
            "merge".into(),
            (0..64).map(|i| format!("outside-page-{i}")).collect(),
        )];
        commits.extend(
            (0..4_000)
                .map(|i| graph_commit(format!("commit-{i}"), vec![format!("commit-{}", i + 1)])),
        );
        let (rows, notice) = layout_graph(&commits, &active()).unwrap();
        assert!(
            notice.is_some(),
            "65 lanes across 4000 rows exceed the edge budget"
        );
        assert_eq!(rows.len(), commits.len());
        assert!(rows.iter().all(|row| row.edges.is_empty()));
    }

    #[test]
    fn graph_preflight_stops_when_selection_is_superseded() {
        let cancellation = active();
        cancellation.latest.store(2, Ordering::Release);
        let commits = [graph_commit("head".into(), vec!["parent".into()])];
        assert!(layout_graph(&commits, &cancellation).is_err());
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use gitturtle_core::ChangeStatus;
    use std::{
        fs,
        io::Write,
        process::{Command, Stdio},
    };

    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><rect width="1" height="1" fill="#ff0000"/></svg>"##;
    const LFS_OID: &str = "193f55b4f0be3159c3d9d19ba6d042895a869e1d9405d1bedf5a09d1f56ee6b7";

    struct Fixture(PathBuf);

    impl Fixture {
        fn command(path: &Path) -> Command {
            let mut command = Command::new("git");
            command
                .arg("-C")
                .arg(path)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null");
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
            command
        }

        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "gitturtle-worker-test-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            let output = Self::command(&path)
                .args(["init", "--quiet"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            Self(path)
        }

        fn blob(&self, bytes: &[u8]) -> String {
            self.git(&["hash-object", "-w", "--stdin"], bytes)
        }

        fn git(&self, args: &[&str], bytes: &[u8]) -> String {
            let mut child = Self::command(&self.0)
                .args(args)
                .env("GIT_AUTHOR_NAME", "GitTurtle Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
                .env("GIT_COMMITTER_NAME", "GitTurtle Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(bytes).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            String::from_utf8(output.stdout).unwrap().trim().into()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn active() -> Cancellation {
        Cancellation {
            generation: 1,
            latest: Arc::new(AtomicU64::new(1)),
            history: gitturtle_core::HistoryCancellation::default(),
        }
    }

    fn working_fixture() -> (Fixture, GitRepository, FileChange, Arc<Content>) {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("file.txt"), "base\n").unwrap();
        fixture.git(&["add", "file.txt"], b"");
        fixture.git(
            &["-c", "commit.gpgsign=false", "commit", "-qm", "Initial"],
            b"",
        );
        fs::write(fixture.0.join("file.txt"), "ours\n").unwrap();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let entry = repo.status().unwrap().entries.remove(0);
        let Output::WorkingPreview(file, content, _) = execute(
            Job::WorkingPreview {
                repo: repo.clone(),
                entry,
                area: gitturtle_core::ChangeArea::Unstaged,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected working preview")
        };
        (fixture, repo, file, content)
    }

    #[test]
    fn history_search_resolves_live_branch_and_worktree_scopes_and_prepares_graph() {
        let fixture = Fixture::new();
        let tree = fixture.git(&["mktree"], b"");
        let first = fixture.git(&["commit-tree", &tree, "-m", "first target"], b"");
        let next = fixture.git(
            &["commit-tree", &tree, "-p", &first, "-m", "new target"],
            b"",
        );
        fixture.git(&["update-ref", "refs/heads/main", &first], b"");
        fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], b"");
        let repo = GitRepository::open(&fixture.0).unwrap();
        let stale = repo.branches().unwrap();
        fixture.git(&["update-ref", "refs/heads/main", &next], b"");
        assert_eq!(
            stale
                .iter()
                .find(|branch| branch.name == "main")
                .unwrap()
                .oid,
            first
        );
        for scope in [
            Scope::Branch {
                name: "main".into(),
                remote: false,
            },
            Scope::Worktree {
                path: repo.path().to_owned(),
            },
        ] {
            let Output::SearchHistory(result) = execute(
                Job::SearchHistory {
                    repo: repo.clone(),
                    scope: Some(scope),
                    pinned: None,
                    query: "target".into(),
                    offset: 0,
                    limit: 20,
                    previous: Vec::new(),
                    remaining_bytes: 1024 * 1024,
                },
                &mut PreviewCache::default(),
                &active(),
                &mut RepositorySession::default(),
            )
            .unwrap() else {
                panic!("expected search output");
            };
            assert_eq!(
                result
                    .page
                    .commits
                    .iter()
                    .map(|commit| &commit.oid)
                    .collect::<Vec<_>>(),
                vec![&next, &first]
            );
            assert_eq!(
                result.page.scope,
                gitturtle_core::HistoryScope::FromCommit(next.clone())
            );
            assert_eq!(result.graph.len(), 2);
            assert!(result.retained_bytes > 0);
        }
        let error = execute(
            Job::SearchHistory {
                repo,
                scope: Some(Scope::Branch {
                    name: "deleted".into(),
                    remote: false,
                }),
                pinned: None,
                query: "target".into(),
                offset: 0,
                limit: 20,
                previous: Vec::new(),
                remaining_bytes: 1024,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .err()
        .expect("missing branch must fail");
        assert!(error.to_string().contains("branch"));
    }

    #[test]
    fn changed_files_preserve_detected_rename_paths_for_preview_identity() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("before.txt"), b"one\ntwo\nthree\n").unwrap();
        fixture.git(&["add", "before.txt"], b"");
        fixture.git(
            &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
            b"",
        );
        fixture.git(&["mv", "before.txt", "after.txt"], b"");
        fixture.git(
            &["-c", "commit.gpgsign=false", "commit", "-qm", "rename"],
            b"",
        );
        let oid = fixture.git(&["rev-parse", "HEAD"], b"");
        let repo = GitRepository::open(&fixture.0).unwrap();
        let Output::Changes(files, _) = execute(
            Job::Changes {
                repo: repo.clone(),
                oid,
                parent: 0,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected changed files");
        };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, ChangeStatus::Renamed);
        assert_eq!(files[0].old_path.as_deref(), Some(Path::new("before.txt")));
        assert_eq!(files[0].new_path.as_deref(), Some(Path::new("after.txt")));
        assert_eq!(
            PreviewKey::new(repo.path(), &files[0]).old_path.as_deref(),
            Some(Path::new("before.txt"))
        );
    }

    #[test]
    fn search_continuation_keeps_pinned_scope_after_ref_moves_and_bounds_retained_metadata() {
        let fixture = Fixture::new();
        let tree = fixture.git(&["mktree"], b"");
        let first = fixture.git(&["commit-tree", &tree, "-m", "first"], b"");
        let next = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "next"], b"");
        fixture.git(&["update-ref", "refs/heads/main", &next], b"");
        let repo = GitRepository::open(&fixture.0).unwrap();
        let run = |pinned, offset, previous, remaining_bytes| {
            execute(
                Job::SearchHistory {
                    repo: repo.clone(),
                    scope: None,
                    pinned,
                    query: "".into(),
                    offset,
                    limit: 1,
                    previous,
                    remaining_bytes,
                },
                &mut PreviewCache::default(),
                &active(),
                &mut RepositorySession::default(),
            )
        };
        let Output::SearchHistory(page) = run(None, 0, Vec::new(), 1024 * 1024).unwrap() else {
            panic!("expected search");
        };
        fixture.git(&["update-ref", "refs/heads/main", &first], b"");
        let previous = page
            .page
            .commits
            .iter()
            .map(|commit| (commit.oid.clone(), commit.parents.clone()))
            .collect();
        let Output::SearchHistory(rest) = run(
            Some(page.page.scope),
            page.page.next_offset.unwrap(),
            previous,
            1024 * 1024,
        )
        .unwrap() else {
            panic!("expected continuation");
        };
        assert_eq!(rest.page.commits[0].oid, first);
        assert_eq!(rest.graph.len(), 2);
        assert!(
            run(None, 0, Vec::new(), 0)
                .err()
                .unwrap()
                .to_string()
                .contains("display budget")
        );
    }

    fn quiet_preview(
        repo: &GitRepository,
        file: &FileChange,
        content: &Arc<Content>,
        history: bool,
    ) -> QuietRefresh {
        let mut cache = PreviewCache::default();
        let Output::QuietRefresh(result) = execute(
            Job::QuietRefresh {
                repo: repo.clone(),
                scope: None,
                limit: 100,
                history,
                selected: Some(WorkingSelection {
                    path: "file.txt".into(),
                    area: gitturtle_core::ChangeArea::Unstaged,
                    file: Some(file.clone()),
                    content: Some(Arc::clone(content)),
                }),
            },
            &mut cache,
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected quiet refresh")
        };
        assert!(
            cache.entries.is_empty(),
            "mutable previews must bypass the immutable cache"
        );
        *result
    }

    #[test]
    fn quiet_status_changes_keep_identical_preview_and_read_only_metadata() {
        let (fixture, repo, file, content) = working_fixture();
        let index = fixture.0.join(".git/index");
        let before = fs::read(&index).unwrap();
        let modified = fs::metadata(&index).unwrap().modified().unwrap();
        fs::write(fixture.0.join("note.txt"), "unrelated local edit\n").unwrap();
        let refresh = quiet_preview(&repo, &file, &content, false);
        assert_eq!(refresh.working.status.entries.len(), 2);
        assert!(refresh.snapshot.is_none());
        assert!(matches!(refresh.preview, Some(Ok(QuietPreview::Unchanged))));
        assert_eq!(fs::read(&index).unwrap(), before);
        assert_eq!(fs::metadata(index).unwrap().modified().unwrap(), modified);
        assert_eq!(fs::read(fixture.0.join("file.txt")).unwrap(), b"ours\n");
    }

    #[test]
    fn quiet_refresh_replaces_same_length_edits_and_removes_vanished_selection() {
        let (fixture, repo, file, content) = working_fixture();
        fs::write(fixture.0.join("file.txt"), "next\n").unwrap();
        let refresh = quiet_preview(&repo, &file, &content, false);
        let Some(Ok(QuietPreview::Changed { file, content })) = refresh.preview else {
            panic!("expected changed text")
        };
        let Content::Text { new, .. } = content.as_ref() else {
            panic!("expected text")
        };
        assert_eq!(new, "next\n");
        fs::write(fixture.0.join("file.txt"), "base\n").unwrap();
        let refresh = quiet_preview(&repo, &file, &content, false);
        assert!(matches!(refresh.preview, Some(Ok(QuietPreview::Absent))));
        assert!(refresh.working.status.entries.is_empty());
    }

    #[test]
    fn quiet_git_refresh_follows_external_staging_and_new_head() {
        let (fixture, repo, file, content) = working_fixture();
        fixture.git(&["add", "file.txt"], b"");
        let refresh = quiet_preview(&repo, &file, &content, true);
        let Some(Ok(QuietPreview::Changed {
            content: staged, ..
        })) = refresh.preview
        else {
            panic!("expected staged preview")
        };
        let Content::Text {
            partial: Some(partial),
            ..
        } = staged.as_ref()
        else {
            panic!("expected staged partial actions")
        };
        assert_eq!(partial.diff.area, gitturtle_core::ChangeArea::Staged);
        let QuietHistory::Refreshed(snapshot) = refresh.snapshot.unwrap().unwrap() else {
            panic!("expected refreshed history")
        };
        assert_eq!(snapshot.commits[0].subject, "Initial");
        fixture.git(
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "External commit",
            ],
            b"",
        );
        let refresh = quiet_preview(&repo, &file, &content, true);
        assert!(matches!(refresh.preview, Some(Ok(QuietPreview::Absent))));
        let QuietHistory::Refreshed(snapshot) = refresh.snapshot.unwrap().unwrap() else {
            panic!("expected refreshed history")
        };
        assert_eq!(snapshot.commits[0].subject, "External commit");
        assert!(refresh.working.status.entries.is_empty());
    }

    #[test]
    fn quiet_deleted_scope_keeps_history_context_but_updates_navigation_metadata() {
        let (fixture, repo, _, _) = working_fixture();
        fixture.git(&["branch", "departing"], b"");
        assert!(
            repo.branches()
                .unwrap()
                .iter()
                .any(|branch| branch.name == "departing")
        );
        fixture.git(&["branch", "-d", "departing"], b"");
        let Output::QuietRefresh(refresh) = execute(
            Job::QuietRefresh {
                repo,
                scope: Some(Scope::Branch {
                    name: "departing".into(),
                    remote: false,
                }),
                limit: 100,
                history: true,
                selected: None,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected quiet refresh")
        };
        let QuietHistory::Retained { metadata, error } = refresh.snapshot.unwrap().unwrap() else {
            panic!("missing scopes must retain displayed history")
        };
        assert!(error.contains("departing"));
        assert!(
            metadata
                .branches
                .iter()
                .all(|branch| branch.name != "departing")
        );
        assert!(
            metadata
                .refs
                .values()
                .flatten()
                .all(|name| name != "departing")
        );
        assert_eq!(metadata.worktrees.len(), 1);
        assert!(
            metadata.commits.is_empty(),
            "metadata fallback must not load another history page"
        );
    }

    #[test]
    fn scoped_recovery_refresh_updates_tip_without_broadening_pinned_search() {
        let fixture = Fixture::new();
        fixture.git(&["config", "user.name", "GitTurtle Test"], b"");
        fixture.git(&["config", "user.email", "test@example.invalid"], b"");
        fixture.git(&["config", "commit.gpgsign", "false"], b"");
        fs::write(fixture.0.join("file.txt"), "base\n").unwrap();
        fixture.git(&["add", "file.txt"], b"");
        fixture.git(&["commit", "-qm", "Initial"], b"");
        let base = fixture.git(&["rev-parse", "HEAD"], b"");
        fixture.git(&["checkout", "-qb", "tracking/native-release"], b"");
        fs::write(fixture.0.join("file.txt"), "published\n").unwrap();
        fixture.git(&["commit", "-am", "Revision 125: published change"], b"");
        let published = fixture.git(&["rev-parse", "HEAD"], b"");
        fixture.git(
            &["update-ref", "refs/remotes/origin/release", &published],
            b"",
        );
        let tree = fixture.git(&["rev-parse", "HEAD^{tree}"], b"");
        let unrelated = fixture.git(
            &[
                "commit-tree",
                &tree,
                "-p",
                &base,
                "-m",
                "Revision 125: unrelated branch",
            ],
            b"",
        );
        fixture.git(&["update-ref", "refs/heads/unrelated", &unrelated], b"");
        let repo = GitRepository::open(&fixture.0).unwrap();
        let scope = Scope::Branch {
            name: "tracking/native-release".into(),
            remote: false,
        };
        let pinned = gitturtle_core::HistoryScope::FromCommit(published.clone());
        let mut cache = PreviewCache::default();
        let mut session = RepositorySession::default();
        let search = |pinned, offset| Job::SearchHistory {
            repo: repo.clone(),
            scope: Some(scope.clone()),
            pinned,
            query: "Revision 125:".into(),
            offset,
            limit: 10,
            previous: Vec::new(),
            remaining_bytes: 1024 * 1024,
        };
        let Output::SearchHistory(before) =
            execute(search(None, 0), &mut cache, &active(), &mut session).unwrap()
        else {
            panic!("expected search results")
        };
        assert_eq!(before.page.scope, pinned);
        assert_eq!(before.page.commits.len(), 1);
        assert_eq!(before.page.commits[0].oid, published);

        let plan = repo
            .recovery_plan(gitturtle_core::RecoveryKind::Revert, Some(&published), None)
            .unwrap();
        repo.execute(&gitturtle_core::WriteCommand::Recovery(
            gitturtle_core::RecoveryCommand::Revert { plan }.into(),
        ))
        .unwrap();
        let reverted = fixture.git(&["rev-parse", "HEAD"], b"");
        let index = fs::read(fixture.0.join(".git/index")).unwrap();
        for refreshed_scope in [
            scope.clone(),
            Scope::Worktree {
                path: repo.path().to_owned(),
            },
        ] {
            let Output::QuietRefresh(refresh) = execute(
                Job::QuietRefresh {
                    repo: repo.clone(),
                    scope: Some(refreshed_scope),
                    limit: 100,
                    history: true,
                    selected: None,
                },
                &mut cache,
                &active(),
                &mut session,
            )
            .unwrap() else {
                panic!("expected quiet refresh")
            };
            assert_eq!(
                refresh.working.status.head.as_deref(),
                Some(reverted.as_str())
            );
            let QuietHistory::Refreshed(snapshot) = refresh.snapshot.unwrap().unwrap() else {
                panic!("valid scope must refresh")
            };
            assert_eq!(snapshot.commits[0].oid, reverted);
            assert!(
                snapshot
                    .commits
                    .iter()
                    .any(|commit| commit.oid == published)
            );
            assert!(
                snapshot
                    .commits
                    .iter()
                    .all(|commit| commit.oid != unrelated)
            );
        }
        let Output::SearchHistory(continued) = execute(
            search(Some(pinned.clone()), 1),
            &mut cache,
            &active(),
            &mut session,
        )
        .unwrap() else {
            panic!("expected pinned continuation")
        };
        assert_eq!(continued.page.scope, pinned);
        assert!(continued.page.commits.is_empty());
        let Output::SearchHistory(restarted) =
            execute(search(None, 0), &mut cache, &active(), &mut session).unwrap()
        else {
            panic!("expected restarted search")
        };
        assert_eq!(
            restarted.page.scope,
            gitturtle_core::HistoryScope::FromCommit(reverted.clone())
        );
        assert_eq!(
            restarted
                .page
                .commits
                .iter()
                .map(|commit| commit.oid.as_str())
                .collect::<Vec<_>>(),
            [reverted.as_str(), published.as_str()]
        );
        assert_eq!(fs::read(fixture.0.join(".git/index")).unwrap(), index);
        assert_eq!(fs::read(fixture.0.join("file.txt")).unwrap(), b"base\n");
    }

    #[test]
    fn render_pixels_are_bgra_without_changing_source_alpha_or_rgba() {
        let preview = ImagePreview {
            width: 2,
            height: 1,
            original_width: 2,
            original_height: 1,
            rgba: vec![255, 30, 10, 128, 5, 70, 200, 0],
            format: "test".into(),
        };
        let rendered = render_image(&preview).unwrap();
        assert_eq!(
            rendered.as_bytes(0).unwrap(),
            &[10, 30, 255, 128, 200, 70, 5, 0]
        );
        assert_eq!(preview.rgba, [255, 30, 10, 128, 5, 70, 200, 0]);
        let expected_bytes = preview.rgba.capacity() + preview.format.capacity() + 8;
        let content = Content::Images {
            old: ImageSide {
                image: Some(preview),
                render: Some(rendered),
                message: None,
                bytes: 8,
            },
            new: ImageSide {
                image: None,
                render: None,
                message: None,
                bytes: 0,
            },
        };
        assert_eq!(content.bytes(), expected_bytes);
    }

    #[test]
    fn lfs_preview_retries_misses_and_caches_verified_content() {
        let fixture = Fixture::new();
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{LFS_OID}\nsize {}\n",
            SVG.len()
        );
        let blob_oid = fixture.blob(pointer.as_bytes());
        let repository = GitRepository::open(&fixture.0).unwrap();
        let file = FileChange {
            old_path: None,
            new_path: Some("image.svg".into()),
            old_oid: None,
            new_oid: Some(blob_oid),
            status: ChangeStatus::Added,
            old_mode: "000000".into(),
            new_mode: "100644".into(),
        };
        let mut cache = PreviewCache::default();
        let mut session = RepositorySession::default();
        let mut preview = |expected_cached_entries| {
            let output = execute(
                Job::Preview {
                    repo: repository.clone(),
                    file: file.clone(),
                },
                &mut cache,
                &active(),
                &mut session,
            )
            .unwrap();
            assert_eq!(cache.entries.len(), expected_cached_entries);
            match output {
                Output::Preview(content, _) => content,
                _ => panic!("expected preview"),
            }
        };
        let missing = preview(0);
        match &*missing {
            Content::Images { old, new } => {
                assert!(old.image.is_none());
                assert!(old.message.is_none());
                assert!(new.image.is_none());
                assert!(
                    new.message
                        .as_ref()
                        .unwrap()
                        .contains("unavailable in the local store")
                );
            }
            _ => panic!("expected image sides"),
        }
        let store = fixture
            .0
            .join(".git/lfs/objects")
            .join(&LFS_OID[..2])
            .join(&LFS_OID[2..4]);
        fs::create_dir_all(&store).unwrap();
        let object = store.join(LFS_OID);
        fs::write(&object, SVG).unwrap();
        let available = preview(1);
        match &*available {
            Content::Images { new, .. } => {
                assert_eq!(new.bytes, SVG.len());
                assert_eq!(
                    new.render.as_ref().unwrap().as_bytes(0).unwrap(),
                    &[0, 0, 255, 255]
                );
                assert!(new.message.is_none());
            }
            _ => panic!("expected image sides"),
        }
        fs::remove_file(object).unwrap();
        // Cached decoded bytes remain the exact verified SHA-256 content even
        // if a separate client prunes its LFS store after our successful read.
        let cached = preview(1);
        assert!(Arc::ptr_eq(&available, &cached));
    }

    #[test]
    fn refreshing_branch_and_worktree_scopes_resolves_external_tip_movement() {
        let fixture = Fixture::new();
        fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], &[]);
        let tree = fixture.git(&["mktree"], &[]);
        let first = fixture.git(&["commit-tree", &tree, "-m", "first"], &[]);
        fixture.git(&["update-ref", "refs/heads/main", &first], &[]);
        let scope = Scope::Branch {
            name: "main".into(),
            remote: false,
        };
        let mut session = RepositorySession::default();
        let before =
            read_snapshot(fixture.0.clone(), Some(&scope), 10, &active(), &mut session).unwrap();
        assert_eq!(before.commits[0].oid, first);
        let worktree_scope = Scope::Worktree {
            path: before.repository.path().to_owned(),
        };
        let second = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "second"], &[]);
        fixture.git(&["update-ref", "refs/heads/main", &second], &[]);
        for scope in [&scope, &worktree_scope] {
            let refreshed =
                read_snapshot(fixture.0.clone(), Some(scope), 10, &active(), &mut session).unwrap();
            assert_eq!(refreshed.commits[0].oid, second);
            assert_eq!(refreshed.commits[1].oid, first);
        }
        // A missing selection must never silently fall back to unrelated refs.
        fixture.git(&["update-ref", "-d", "refs/heads/main"], &[]);
        let error = read_snapshot(fixture.0.clone(), Some(&scope), 10, &active(), &mut session)
            .err()
            .expect("deleted branch must fail");
        assert!(error.to_string().contains("no longer exists"));
    }

    #[test]
    fn retained_session_survives_ui_clear_and_nested_opens_but_separates_worktrees() {
        let fixture = Fixture::new();
        fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], &[]);
        let tree = fixture.git(&["mktree"], &[]);
        let oid = fixture.git(&["commit-tree", &tree, "-m", "initial"], &[]);
        fixture.git(&["update-ref", "refs/heads/main", &oid], &[]);
        let blob = fixture.blob(b"retained object reader\n");
        let mut session = RepositorySession::default();
        let ui_repository = session.open(&fixture.0).unwrap();
        assert_eq!(
            ui_repository.blob(&blob).unwrap(),
            b"retained object reader\n"
        );
        let retained = Arc::downgrade(&session.current.as_ref().unwrap().repository);
        drop(ui_repository);
        assert!(
            retained.upgrade().is_some(),
            "clearing UI must retain the process owner"
        );

        let nested = fixture.0.join("nested/directory");
        fs::create_dir_all(&nested).unwrap();
        for path in [&fixture.0, &nested] {
            let reopened = session.open(path).unwrap();
            assert!(Arc::ptr_eq(
                &retained.upgrade().unwrap(),
                &session.current.as_ref().unwrap().repository,
            ));
            assert_eq!(reopened.blob(&blob).unwrap(), b"retained object reader\n");
        }

        let linked = fixture.0.join("linked-worktree");
        fixture.git(
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "feature",
                linked.to_str().unwrap(),
            ],
            &[],
        );
        let linked_repo = session.open(&linked).unwrap();
        assert!(
            retained.upgrade().is_none(),
            "old retained owner is released on session replacement"
        );
        assert_eq!(linked_repo.path(), linked.canonicalize().unwrap());
        assert_eq!(
            linked_repo
                .branches()
                .unwrap()
                .iter()
                .find(|b| b.current)
                .unwrap()
                .name,
            "feature"
        );
        let main_repo = session.open(&fixture.0).unwrap();
        assert_eq!(
            main_repo
                .branches()
                .unwrap()
                .iter()
                .find(|b| b.current)
                .unwrap()
                .name,
            "main"
        );
    }
}
