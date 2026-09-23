mod repository_identity;
use repository_identity::RepositoryIdentity;

use crate::{graph, text::PatchPresentation};
use anyhow::{Result, anyhow, ensure};
use futures::channel::oneshot;
use gitturtle_core::{Branch, Commit, FileChange, GitRepository, TextPreview, Worktree};
use gitturtle_preview::{
    ImagePreview, MAX_INPUT_BYTES, decode_image, detect_lfs_pointer, is_image_path, metadata,
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
    pub history_scope: gitturtle_core::HistoryScope,
    pub history_offset: usize,
    pub history_next_offset: Option<usize>,
    pub graph: Vec<graph::GraphRow>,
    pub graph_notice: Option<String>,
    pub refs: HashMap<String, Vec<String>>,
    pub elapsed: Duration,
}

pub struct ImageSide {
    pub animation: Option<Arc<crate::gif_playback::Timeline>>,
    pub captured: Option<Arc<[u8]>>,
    pub literal_source: Option<Arc<str>>,
    pub image: Option<ImagePreview>,
    /// BGRA conversion happens on the worker; the UI only shares this allocation.
    pub render: Option<Arc<RenderImage>>,
    pub message: Option<String>,
    pub bytes: usize,
    /// Retain only a bounded missing pointer so an explicit download can review
    /// the exact object. Normal preview construction remains entirely passive.
    pub lfs_pointer: Option<Vec<u8>>,
}

pub enum Content {
    Rich(crate::rich_preview::Comparison),
    Conflict(Arc<crate::conflicts::Presentation>),
    Text {
        diagrams: Option<Arc<crate::rich_preview::Comparison>>,
        markdown: Option<Arc<crate::markdown_view::Comparison>>,
        patch: String,
        old: String,
        new: String,
        presentation: Arc<PatchPresentation>,
        split: Arc<crate::split_diff::SplitPresentation>,
        partial: Option<Arc<crate::partial_view::PartialActions>>,
        partial_unavailable: Option<String>,
    },
    Images {
        old: Box<ImageSide>,
        new: Box<ImageSide>,
    },
    Notice(String),
}

impl Content {
    /// CPU allocations retained by a cache entry. UI-held Arcs and uploaded GPU
    /// textures have independent lifetimes and need their own presentation budget.
    pub(super) fn bytes(&self) -> usize {
        match self {
            Self::Rich(preview) => preview.retained_bytes(),
            Self::Text {
                diagrams,
                markdown,
                patch,
                old,
                new,
                presentation,
                split,
                partial,
                partial_unavailable,
            } => {
                diagrams
                    .as_ref()
                    .map_or(0, |preview| preview.retained_bytes())
                    + markdown
                        .as_ref()
                        .map_or(0, |preview| preview.retained_bytes())
                    + patch.capacity()
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
                    std::mem::size_of::<ImageSide>()
                        + side
                            .image
                            .as_ref()
                            .map_or(0, |image| image.rgba.capacity() + image.format.capacity())
                        + side.render.as_ref().map_or(0, |render| {
                            (0..render.frame_count())
                                .map(|frame| render.as_bytes(frame).map_or(0, <[u8]>::len))
                                .sum::<usize>()
                        })
                        + side
                            .animation
                            .as_ref()
                            .map_or(0, |timeline| timeline.retained_bytes())
                        + side.message.as_ref().map_or(0, String::capacity)
                        + side.lfs_pointer.as_ref().map_or(0, Vec::capacity)
                        + side.captured.as_ref().map_or(0, |bytes| bytes.len())
                        + side
                            .literal_source
                            .as_ref()
                            .map_or(0, |source| source.len())
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
    /// Drop the idle history process on the worker when leaving its tab.
    ReleaseHistory,
    HistoryPage {
        repo: GitRepository,
        scope: gitturtle_core::HistoryScope,
        offset: usize,
        limit: usize,
    },
    ReviewText {
        content: Arc<Content>,
        options: crate::text_review::Options,
    },
    CompareRevisions {
        repo: GitRepository,
        before: String,
        after: String,
        mode: gitturtle_core::ComparisonMode,
    },
    RevisionTargets {
        repo: GitRepository,
    },
    TrackedPaths {
        repo: GitRepository,
        scope: gitturtle_core::PathScope,
        query: String,
    },
    TrackedPreview {
        repo: GitRepository,
        entry: gitturtle_core::TrackedPath,
        scope: gitturtle_core::PathScope,
    },
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
        head: Option<String>,
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
        origins: crate::markdown_view::Origins,
    },
}

pub enum Output {
    HistoryReleased,
    HistoryPage(HistoryPageResult),
    ReviewText(Arc<Content>, Duration),
    RevisionComparison(gitturtle_core::RevisionComparison),
    RevisionTargets(Vec<String>),
    TrackedPaths(gitturtle_core::TrackedPaths),
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

pub struct HistoryPageResult {
    pub page: gitturtle_core::HistoryTraversalPage,
    pub graph: Vec<graph::GraphRow>,
    pub graph_notice: Option<String>,
    pub elapsed: Duration,
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
    /// Test-only: every job submitted to this worker's queue, so a test can
    /// assert that a path submits none.
    #[cfg(test)]
    submissions: AtomicU64,
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
                    if request.reply.is_canceled() && !matches!(request.job, Job::ReleaseHistory) {
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
        Self {
            queue,
            latest,
            #[cfg(test)]
            submissions: AtomicU64::new(0),
        }
    }

    /// Test-only: the jobs submitted so far, including any a closed worker
    /// refused.
    #[cfg(test)]
    pub fn submissions(&self) -> u64 {
        self.submissions.load(Ordering::Acquire)
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

    /// Closing an inactive history stream must not join Git on the UI thread.
    /// A subsequent Open also replaces it if this queued release is superseded.
    pub fn release_history(&self) {
        drop(self.submit(Job::ReleaseHistory));
    }

    pub fn submit(&self, job: Job) -> oneshot::Receiver<Result<Output>> {
        #[cfg(test)]
        self.submissions.fetch_add(1, Ordering::AcqRel);
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
    identity: RepositoryIdentity,
}

/// Retain one current worktree session, independently of preview eviction and
/// UI selection. Replacing it and dropping its process-owning Git handle happen
/// on the worker. Linked worktrees never share a session merely because their
/// common object directory is the same: HEAD and local configuration differ.
#[derive(Default)]
struct RepositorySession {
    current: Option<RetainedRepository>,
    history: Option<OrdinaryHistory>,
}

struct OrdinaryHistory {
    path: PathBuf,
    traversal: gitturtle_core::HistoryTraversal,
    graph: graph::GraphCursor,
}

impl RepositorySession {
    fn open(&mut self, requested: &Path) -> Result<GitRepository> {
        let canonical = requested.canonicalize()?;
        if self
            .current
            .as_ref()
            .is_some_and(|current| !current.identity.is_current())
        {
            self.current = None;
            self.history = None;
        }
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
        let identity = RepositoryIdentity::capture(&repository)?;
        self.history = None;
        self.current = Some(RetainedRepository {
            canonical_root,
            repository: Arc::clone(&repository),
            identity,
        });
        Ok(repository.as_ref().clone())
    }

    fn history_page(
        &mut self,
        repo: &GitRepository,
        scope: &gitturtle_core::HistoryScope,
        offset: usize,
        limit: usize,
        cancellation: &Cancellation,
    ) -> Result<HistoryPageResult> {
        let start = Instant::now();
        let matches = self.history.as_ref().is_some_and(|history| {
            history.path == repo.path()
                && history.traversal.scope() == scope
                && history.traversal.offset() <= offset
        });
        if !matches {
            self.history = None;
            self.history = Some(OrdinaryHistory {
                path: repo.path().to_owned(),
                traversal: repo.history_traversal(scope, &cancellation.history)?,
                graph: graph::GraphCursor::default(),
            });
        }
        let result = (|| {
            let history = self.history.as_mut().expect("History cursor initialized");
            // Back/restore replays a captured stream once, discarding each
            // bounded page and retaining only the ancestry frontier. Sequential
            // browsing does not enter this loop or replay any prior records.
            while history.traversal.offset() < offset {
                cancellation.check()?;
                let count =
                    (offset - history.traversal.offset()).min(gitturtle_core::MAX_HISTORY_PAGE);
                let page = history.traversal.next_page(count, &cancellation.history)?;
                history
                    .graph
                    .append(&page.commits, GRAPH_LANE_LIMIT, GRAPH_EDGE_LIMIT, || {
                        cancellation.check()
                    })?;
                ensure!(
                    page.next_offset.is_some() || history.traversal.offset() == offset,
                    "Saved history position is past the captured history"
                );
            }
            let page = history.traversal.next_page(
                limit.clamp(1, gitturtle_core::MAX_HISTORY_PAGE),
                &cancellation.history,
            )?;
            let (graph, hidden) =
                history
                    .graph
                    .append(&page.commits, GRAPH_LANE_LIMIT, GRAPH_EDGE_LIMIT, || {
                        cancellation.check()
                    })?;
            cancellation.check()?;
            Ok(HistoryPageResult {
                page,
                graph,
                graph_notice: hidden.then(graph_notice),
                elapsed: start.elapsed(),
            })
        })();
        if result.is_err() {
            self.history = None;
        }
        result
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
    origins: crate::markdown_view::Origins,
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
            origins: Default::default(),
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
        Job::ReleaseHistory => {
            session.history = None;
            Ok(Output::HistoryReleased)
        }
        Job::HistoryPage {
            repo,
            scope,
            offset,
            limit,
        } => session
            .history_page(&repo, &scope, offset, limit, cancellation)
            .map(Output::HistoryPage),
        Job::ReviewText { content, options } => {
            let content = crate::text_review::prepare(&content, options, || cancellation.check())?;
            Ok(Output::ReviewText(content, start.elapsed()))
        }
        Job::CompareRevisions {
            repo,
            before,
            after,
            mode,
        } => {
            let result = repo.compare_revisions(&before, &after, mode, &cancellation.history)?;
            cancellation.check()?;
            Ok(Output::RevisionComparison(result))
        }
        Job::RevisionTargets { repo } => {
            gitturtle_core::run_cancellable_inspection(cancellation.history.clone(), || {
                let mut targets = vec!["HEAD".into()];
                targets.extend(repo.branches()?.into_iter().take(10_000).map(|b| {
                    format!(
                        "refs/{}/{}",
                        if b.remote { "remotes" } else { "heads" },
                        b.name
                    )
                }));
                cancellation.check()?;
                targets.extend(
                    repo.tags()?
                        .tags
                        .into_iter()
                        .map(|tag| format!("refs/tags/{}", tag.name)),
                );
                cancellation.check()?;
                Ok(Output::RevisionTargets(targets))
            })
        }
        Job::TrackedPaths { repo, scope, query } => Ok(Output::TrackedPaths(
            repo.search_tracked_paths(&scope, &query, &cancellation.history)?,
        )),
        Job::TrackedPreview { repo, entry, scope } => {
            let mut file = repo.tracked_path_change(&entry);
            if matches!(scope, gitturtle_core::PathScope::Revision(_)) {
                return execute(
                    Job::Preview {
                        repo,
                        file,
                        origins: crate::markdown_view::Origins::revisions(
                            None,
                            match scope {
                                gitturtle_core::PathScope::Revision(oid) => Some(oid),
                                _ => None,
                            },
                        ),
                    },
                    cache,
                    cancellation,
                    session,
                );
            }
            let (mode, bytes) = repo.read_tracked_working_file(&entry)?;
            cancellation.check()?;
            file.new_mode = mode;
            let mut content = if let Some(content) =
                supplied_nontext_content(&repo, &file, &[], &bytes, cancellation)?
            {
                content
            } else if let Some(content) =
                resolved_lfs_text_content(&repo, &file, &[], &bytes, cancellation)?
            {
                content
            } else if bytes.len() > gitturtle_core::MAX_DIFF_BYTES
                || bytes.iter().filter(|b| **b == b'\n').count() > gitturtle_core::MAX_DIFF_LINES
            {
                Content::Notice("File exceeds the 2 MiB / 100,000-line text preview limit. File history remains available.".into())
            } else if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
                Content::Notice(
                    "Binary or non-UTF-8 working file. File history remains available.".into(),
                )
            } else {
                let source = String::from_utf8(bytes)?;
                // Quick Open is a source inspection, so no synthetic staged
                // patch or selected-edit actions are manufactured.
                prepared_text(String::new(), String::new(), source)
            };
            attach_mermaid(&mut content, &file, cancellation)?;
            attach_markdown_assets(
                &mut content,
                &repo,
                &file,
                &crate::markdown_view::Origins {
                    old: None,
                    new: Some(gitturtle_core::PreviewAssetScope::Worktree),
                },
                cancellation,
            )?;
            cancellation.check()?;
            Ok(Output::Preview(Arc::new(content), start.elapsed()))
        }
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
                            head: working.status.head.clone(),
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
        Job::WorkingPreview {
            repo,
            entry,
            area,
            head,
        } => {
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
            let mut partial_diff = preview.partial;
            let mut unavailable = preview.partial_unavailable;
            let mut content = if let Some(content) =
                supplied_nontext_content(&repo, &file, &preview.old, &preview.new, cancellation)?
            {
                content
            } else if let Some(content) =
                resolved_lfs_text_content(&repo, &file, &preview.old, &preview.new, cancellation)?
            {
                partial_diff = None;
                unavailable = Some("This view shows verified LFS object content. Partial staging is unavailable because Git stages the underlying pointer; use whole-file staging.".into());
                content
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
            attach_mermaid(&mut content, &file, cancellation)?;
            let origins = match area {
                gitturtle_core::ChangeArea::Staged => crate::markdown_view::Origins {
                    old: head.map(gitturtle_core::PreviewAssetScope::Revision),
                    new: Some(gitturtle_core::PreviewAssetScope::Index),
                },
                gitturtle_core::ChangeArea::Unstaged => crate::markdown_view::Origins {
                    old: Some(gitturtle_core::PreviewAssetScope::Index),
                    new: Some(gitturtle_core::PreviewAssetScope::Worktree),
                },
            };
            attach_markdown_assets(&mut content, &repo, &file, &origins, cancellation)?;
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
            let mut snapshot =
                read_snapshot(path.clone(), scope.as_ref(), limit, cancellation, session)
                    .map_err(|error| crate::repository_access::explain(&path, error))?;
            cancellation.check()?;
            snapshot.elapsed = start.elapsed();
            Ok(Output::Snapshot(snapshot))
        }
        Job::Changes { repo, oid, parent } => {
            let changes = repo.changes_with_renames(&oid, parent)?;
            cancellation.check()?;
            Ok(Output::Changes(changes, start.elapsed()))
        }
        Job::Preview {
            repo,
            file,
            origins,
        } => {
            let mut key = PreviewKey::new(repo.path(), &file);
            key.origins = origins.clone();
            if let Some(content) = cache.get(&key) {
                return Ok(Output::Preview(content, start.elapsed()));
            }
            let mut content = text_content(&repo, &file, cancellation)?;
            attach_markdown_assets(&mut content, &repo, &file, &origins, cancellation)?;
            let cacheable = match &content {
                Content::Images { old, new } => old.message.is_none() && new.message.is_none(),
                Content::Rich(preview) => {
                    preview.old.error.is_none() && preview.new.error.is_none()
                }
                Content::Text {
                    old, new, markdown, ..
                } => {
                    markdown
                        .as_ref()
                        .is_none_or(|markdown| markdown.cacheable())
                        && ![old, new]
                            .iter()
                            .any(|source| detect_lfs_pointer(source.as_bytes()).is_some())
                }
                _ => true,
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
    commit.history_bytes()
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
    let scope = match oid {
        Some(oid) if oid.bytes().all(|byte| byte == b'0') => {
            gitturtle_core::HistoryScope::PinnedRefs(Vec::new())
        }
        Some(oid) => gitturtle_core::HistoryScope::FromCommit(oid.to_owned()),
        None => gitturtle_core::HistoryScope::AllRefs,
    };
    let (commits, graph, graph_notice, history_scope, history_next_offset) = if limit == 0 {
        (Vec::new(), Vec::new(), None, scope, None)
    } else {
        // An explicit or quiet refresh captures new tips and starts a new
        // traversal. Ordinary subsequent pages use the immutable returned scope.
        session.history = None;
        let result = session.history_page(&repository, &scope, 0, limit, cancellation)?;
        let pinned = session
            .history
            .as_ref()
            .expect("Live history traversal")
            .traversal
            .scope()
            .clone();
        (
            result.page.commits,
            result.graph,
            result.graph_notice,
            pinned,
            result.page.next_offset,
        )
    };
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
        history_scope,
        history_offset: 0,
        history_next_offset,
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
    Ok((rows, Some(graph_notice())))
}

fn graph_notice() -> String {
    "Graph connections hidden for this history. Select a branch to view a smaller graph.".into()
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

fn regular_preview(file: &FileChange) -> bool {
    !file.is_submodule() && file.old_mode != "120000" && file.new_mode != "120000"
}

/// Detection operates on captured bytes, never a filesystem filename. Extension
/// hints still give corrupt/empty images an honest decoder error; plain source
/// named .png remains literal text. Stored symlinks and gitlinks bypass rendering.
fn supplied_nontext_content(
    repo: &GitRepository,
    file: &FileChange,
    old: &[u8],
    new: &[u8],
    cancellation: &Cancellation,
) -> Result<Option<Content>> {
    if !regular_preview(file) {
        return Ok(None);
    }
    cancellation.check()?;
    let image = [old, new].iter().any(|bytes| metadata::is_image(bytes))
        || (image_change(file)
            && [old, new].iter().any(|bytes| {
                !metadata::is_literal_text(bytes) || detect_lfs_pointer(bytes).is_some()
            }))
        || (image_change(file)
            && ((file.old_path.is_some() && old.is_empty())
                || (file.new_path.is_some() && new.is_empty())));
    if image {
        let old = working_image_side(
            repo,
            old.to_vec(),
            file.old_path.is_some(),
            &file
                .old_path
                .as_deref()
                .unwrap_or(file.path())
                .to_string_lossy(),
            cancellation,
        );
        cancellation.check()?;
        let new = working_image_side(
            repo,
            new.to_vec(),
            file.new_path.is_some(),
            &file
                .new_path
                .as_deref()
                .unwrap_or(file.path())
                .to_string_lossy(),
            cancellation,
        );
        cancellation.check()?;
        return Ok(Some(Content::Images {
            old: Box::new(old),
            new: Box::new(new),
        }));
    }
    let model = [file.old_path.as_deref(), file.new_path.as_deref()]
        .into_iter()
        .flatten()
        .any(gitturtle_preview::model3d::is_model_path);
    // Models keep independently usable sides when only one LFS object is local.
    // Other documents retain their existing literal-pointer comparison path.
    if !model
        && [old, new]
            .iter()
            .any(|bytes| detect_lfs_pointer(bytes).is_some())
    {
        return Ok(None);
    }
    if !model
        && ![old, new]
            .iter()
            .any(|bytes| !metadata::is_literal_text(bytes) || bytes.starts_with(b"%PDF-"))
    {
        return Ok(None);
    }
    let side = |bytes: &[u8], path: Option<&Path>| -> Result<Arc<crate::rich_preview::Side>> {
        let Some(path) = path else {
            return Ok(Arc::new(crate::rich_preview::Side::unavailable(
                file.path(),
                false,
                None,
            )));
        };
        if bytes.len() > MAX_INPUT_BYTES {
            return Ok(Arc::new(crate::rich_preview::Side::unavailable(
                path,
                true,
                Some(format!(
                    "File contains {} bytes, exceeding the 32 MiB captured preview limit.",
                    bytes.len()
                )),
            )));
        }
        Ok(Arc::new(if model {
            captured_model_side(repo, bytes.to_vec(), path, cancellation)?
        } else {
            crate::rich_preview::Side::prepare(bytes.to_vec(), path, || cancellation.check())?
        }))
    };
    Ok(Some(Content::Rich(crate::rich_preview::Comparison {
        old: side(old, file.old_path.as_deref())?,
        new: side(new, file.new_path.as_deref())?,
    })))
}

fn captured_model_side(
    repo: &GitRepository,
    bytes: Vec<u8>,
    path: &Path,
    cancellation: &Cancellation,
) -> Result<crate::rich_preview::Side> {
    cancellation.check()?;
    let Some(pointer) = detect_lfs_pointer(&bytes) else {
        return crate::rich_preview::Side::prepare(bytes, path, || cancellation.check());
    };
    let local = repo.local_lfs_object(&pointer.oid, pointer.size, MAX_INPUT_BYTES);
    cancellation.check()?;
    let error = match local {
        Ok(Some(local)) => {
            return crate::rich_preview::Side::prepare(local, path, || cancellation.check());
        }
        Ok(None) => format!(
            "This model’s LFS object is unavailable in the local store ({} bytes). No download was attempted.",
            pointer.size
        ),
        Err(error) => format!(
            "This model’s local LFS object could not be verified: {error:#}. No download was attempted."
        ),
    };
    let mut side = crate::rich_preview::Side::unavailable(path, true, Some(error));
    side.metadata = metadata::inspect(&bytes, path);
    side.metadata.format = "Git LFS pointer".into();
    // Keep the captured identity available to Copy and explicit download actions.
    // Its error prevents immutable caching while the local store can change.
    side.metadata.source = std::str::from_utf8(&bytes).ok().map(Arc::from);
    side.captured = Some(bytes.into());
    Ok(side)
}

fn nontext_history_content(
    repo: &GitRepository,
    file: &FileChange,
    cancellation: &Cancellation,
) -> Result<Option<(Content, bool)>> {
    if !regular_preview(file) {
        return Ok(None);
    }
    let read = |oid: Option<&str>| -> Result<Vec<u8>> {
        cancellation.check()?;
        let Some(oid) = oid else {
            return Ok(Vec::new());
        };
        let size = repo.blob_size(oid)?;
        ensure!(
            size <= MAX_INPUT_BYTES,
            "File has {size} bytes, exceeding the 32 MiB captured preview limit"
        );
        cancellation.check()?;
        repo.blob(oid)
    };
    let old = read(file.old_oid.as_deref());
    cancellation.check()?;
    let new = read(file.new_oid.as_deref());
    cancellation.check()?;
    if let (Ok(old), Ok(new)) = (&old, &new) {
        return Ok(
            supplied_nontext_content(repo, file, old, new, cancellation)?.map(|content| {
                let cacheable = match &content {
                    Content::Images { old, new } => old.message.is_none() && new.message.is_none(),
                    Content::Rich(preview) => {
                        preview.old.error.is_none() && preview.new.error.is_none()
                    }
                    _ => true,
                };
                (content, cacheable)
            }),
        );
    }
    if image_change(file)
        || [old.as_ref().ok(), new.as_ref().ok()]
            .into_iter()
            .flatten()
            .any(|bytes| metadata::is_image(bytes))
    {
        let side = |bytes: Result<Vec<u8>>, path: Option<&Path>| {
            let name = path.unwrap_or(file.path()).to_string_lossy();
            match bytes {
                Ok(bytes) => working_image_side(repo, bytes, path.is_some(), &name, cancellation),
                Err(error) => ImageSide {
                    animation: None,
                    image: None,
                    render: None,
                    message: Some(format!("{error:#}")),
                    bytes: 0,
                    lfs_pointer: None,
                    captured: None,
                    literal_source: None,
                },
            }
        };
        let old = side(old, file.old_path.as_deref());
        cancellation.check()?;
        let new = side(new, file.new_path.as_deref());
        cancellation.check()?;
        return Ok(Some((
            Content::Images {
                old: Box::new(old),
                new: Box::new(new),
            },
            false,
        )));
    }
    let model = [file.old_path.as_deref(), file.new_path.as_deref()]
        .into_iter()
        .flatten()
        .any(gitturtle_preview::model3d::is_model_path);
    // Keep readable and missing sides independent; a missing historical object
    // never substitutes a working file and is never cached as permanently absent.
    let side = |bytes: Result<Vec<u8>>,
                path: Option<&Path>|
     -> Result<Arc<crate::rich_preview::Side>> {
        let Some(path) = path else {
            return Ok(Arc::new(crate::rich_preview::Side::unavailable(
                file.path(),
                false,
                None,
            )));
        };
        Ok(Arc::new(match bytes {
            Ok(bytes) if model => captured_model_side(repo, bytes, path, cancellation)?,
            Ok(bytes) => crate::rich_preview::Side::prepare(bytes, path, || cancellation.check())?,
            Err(error) => {
                crate::rich_preview::Side::unavailable(path, true, Some(format!("{error:#}")))
            }
        }))
    };
    Ok(Some((
        Content::Rich(crate::rich_preview::Comparison {
            old: side(old, file.old_path.as_deref())?,
            new: side(new, file.new_path.as_deref())?,
        }),
        false,
    )))
}

fn prepared_text(patch: String, old: String, new: String) -> Content {
    let presentation = Arc::new(PatchPresentation::prepare(&patch));
    let split = Arc::new(crate::split_diff::SplitPresentation::prepare(
        &old,
        &new,
        &presentation,
    ));
    Content::Text {
        diagrams: None,
        markdown: None,
        patch,
        old,
        new,
        presentation,
        split,
        partial: None,
        partial_unavailable: None,
    }
}

fn attach_mermaid(
    content: &mut Content,
    file: &FileChange,
    cancellation: &Cancellation,
) -> Result<()> {
    if let Content::Text {
        old, new, markdown, ..
    } = content
        && markdown.is_none()
    {
        *markdown =
            crate::markdown_view::prepare(old, new, file, || cancellation.check())?.map(Arc::new);
    }
    if let Content::Text {
        old,
        new,
        diagrams,
        markdown,
        ..
    } = content
        && diagrams.is_none()
    {
        *diagrams = if let Some(markdown) = markdown {
            markdown.diagrams().map(Arc::new)
        } else {
            crate::rich_preview::prepare_mermaid(old, new, file, || cancellation.check())?
                .map(Arc::new)
        };
    }
    Ok(())
}

fn attach_markdown_assets(
    content: &mut Content,
    repo: &GitRepository,
    file: &FileChange,
    origins: &crate::markdown_view::Origins,
    cancellation: &Cancellation,
) -> Result<()> {
    if let Content::Text {
        markdown: Some(markdown),
        ..
    } = content
        && let Some(markdown) = Arc::get_mut(markdown)
    {
        crate::markdown_view::capture_assets(markdown, repo, file, origins, &cancellation.history)?;
    }
    cancellation.check()
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
                diagrams: ad,
                markdown: am,
                ..
            },
            Content::Text {
                patch: b,
                old: bo,
                new: bn,
                partial_unavailable: bu,
                partial: bp,
                diagrams: bd,
                markdown: bm,
                ..
            },
        ) => {
            a == b
                && match (am, bm) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.same_source(b),
                    _ => false,
                }
                && match (ad, bd) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.old.same_source(&b.old) && a.new.same_source(&b.new),
                    _ => false,
                }
                && ao == bo
                && an == bn
                && au == bu
                && ap.as_ref().map(|partial| &partial.diff)
                    == bp.as_ref().map(|partial| &partial.diff)
        }
        (Content::Notice(a), Content::Notice(b)) => a == b,
        (Content::Rich(a), Content::Rich(b)) => {
            a.old.same_source(&b.old) && a.new.same_source(&b.new)
        }
        (Content::Conflict(a), Content::Conflict(b)) => a.snapshot == b.snapshot,
        (Content::Images { old: ao, new: an }, Content::Images { old: bo, new: bn }) => {
            [ao, an].into_iter().zip([bo, bn]).all(|(a, b)| {
                a.bytes == b.bytes
                    && a.captured == b.captured
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
    let sources = match repo.text_preview_with_sources(file) {
        Ok(sources) => sources,
        Err(error) => {
            if let Some((content, _)) = nontext_history_content(repo, file, cancellation)? {
                return Ok(content);
            }
            return Err(error);
        }
    };
    cancellation.check()?;
    if matches!(sources.preview, TextPreview::TooLarge { .. }) {
        if let Some((content, _)) = nontext_history_content(repo, file, cancellation)? {
            return Ok(content);
        }
    } else if let Some(content) =
        supplied_nontext_content(repo, file, &sources.old, &sources.new, cancellation)?
    {
        return Ok(content);
    }
    if let Some(content) =
        resolved_lfs_text_content(repo, file, &sources.old, &sources.new, cancellation)?
    {
        return Ok(content);
    }
    let mut content = match sources.preview {
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
    };
    attach_mermaid(&mut content, file, cancellation)?;
    Ok(content)
}

fn resolved_lfs_text_content(
    repo: &GitRepository,
    file: &FileChange,
    old: &[u8],
    new: &[u8],
    cancellation: &Cancellation,
) -> Result<Option<Content>> {
    let Some(sources) = repo.resolved_lfs_text(file, old, new)? else {
        return Ok(None);
    };
    if ([file.old_path.as_deref(), file.new_path.as_deref()]
        .into_iter()
        .flatten()
        .any(gitturtle_preview::model3d::is_model_path)
        || [sources.old.as_slice(), sources.new.as_slice()]
            .iter()
            .any(|bytes| !metadata::is_literal_text(bytes) || bytes.starts_with(b"%PDF-")))
        && let Some(content) =
            supplied_nontext_content(repo, file, &sources.old, &sources.new, cancellation)?
    {
        return Ok(Some(content));
    }
    let mut content = match sources.preview {
        TextPreview::Patch(patch) => prepared_text(patch, String::from_utf8(sources.old)?, String::from_utf8(sources.new)?),
        TextPreview::Binary => Content::Notice("Verified LFS object downloaded. Its content is binary or not UTF-8 and has no text preview.".into()),
        TextPreview::TooLarge { old_bytes, new_bytes } => Content::Notice(format!("Verified LFS content exceeds the text display limit · {old_bytes} → {new_bytes} bytes.")),
        TextPreview::Submodule { .. } => Content::Notice("Submodules do not have an LFS text preview.".into()),
    };
    attach_mermaid(&mut content, file, cancellation)?;
    Ok(Some(content))
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
            animation: None,
            captured: None,
            literal_source: None,
            image: None,
            render: None,
            message: None,
            bytes: 0,
            lfs_pointer: None,
        };
    }
    let mut missing_pointer = None;
    let mut animation = None;
    let mut preview_notice = None;
    let result = (|| -> Result<(ImagePreview, Arc<RenderImage>, usize)> {
        cancellation.check()?;
        ensure!(
            bytes.len() <= MAX_INPUT_BYTES,
            "Image exceeds the 32 MiB input limit"
        );
        if let Some(pointer) = detect_lfs_pointer(&bytes) {
            let local = repo.local_lfs_object(&pointer.oid, pointer.size, MAX_INPUT_BYTES)?;
            if local.is_none() {
                missing_pointer = Some(bytes.clone());
            }
            bytes = local.ok_or_else(|| {
                anyhow!(
                    "Git LFS object is unavailable in the local store · {} bytes. No download was attempted.",
                    pointer.size
                )
            })?;
        }
        cancellation.check()?;
        if gitturtle_preview::animation::is_gif(&bytes) {
            match crate::gif_playback::prepare(&bytes, || cancellation.check()) {
                Ok(prepared) => {
                    cancellation.check()?;
                    animation = Some(prepared.timeline);
                    return Ok((prepared.image, prepared.render, bytes.len()));
                }
                Err(error) => {
                    cancellation.check()?;
                    preview_notice = Some(format!(
                        "Static first frame only; animation unavailable: {error:#}"
                    ));
                }
            }
        }
        let decoded = decode_image(&bytes, name, PREVIEW_EDGE)?;
        cancellation.check()?;
        let render = render_image(&decoded)?;
        Ok((decoded, render, bytes.len()))
    })();
    let literal_source = (bytes.len() <= gitturtle_preview::MAX_SVG_BYTES)
        .then(|| std::str::from_utf8(&bytes).ok().map(Arc::<str>::from))
        .flatten();
    let captured = (bytes.len() <= MAX_INPUT_BYTES && missing_pointer.is_none())
        .then(|| Arc::<[u8]>::from(bytes));
    match result {
        Ok((image, render, bytes)) => ImageSide {
            animation,
            captured,
            literal_source,
            image: Some(image),
            render: Some(render),
            message: preview_notice,
            bytes,
            lfs_pointer: None,
        },
        Err(error) => ImageSide {
            animation: None,
            captured,
            literal_source,
            image: None,
            render: None,
            message: Some(format!("{error:#}")),
            bytes: 0,
            lfs_pointer: missing_pointer,
        },
    }
}

pub(super) fn render_image(preview: &ImagePreview) -> Result<Arc<RenderImage>> {
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
            diagrams: None,
            markdown: None,
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

    #[test]
    fn ordinary_history_worker_continues_and_restores_the_same_graph_snapshot() {
        let fixture = Fixture::new();
        let mut input = Vec::new();
        for index in 0..1203 {
            let message = format!("commit {index}");
            input.extend_from_slice(format!("commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> {} +0000\ndata {}\n{}\n", 1_700_000_000 + index, message.len(), message).as_bytes());
        }
        input.extend_from_slice(b"done\n");
        fixture.git(&["fast-import", "--quiet"], &input);
        let repo = GitRepository::open(&fixture.0).unwrap();
        let expected = repo.history(2000).unwrap();
        let expected_graph = graph::layout(&expected, || Ok::<_, ()>(())).unwrap();
        let mut session = RepositorySession::default();
        let mut cache = PreviewCache::default();
        let Output::Snapshot(snapshot) = execute(
            Job::Open {
                path: fixture.0.clone(),
                scope: None,
                limit: 500,
            },
            &mut cache,
            &active(),
            &mut session,
        )
        .unwrap() else {
            panic!("snapshot")
        };
        assert_eq!(snapshot.commits, expected[..500]);
        assert_eq!(snapshot.graph, expected_graph[..500]);
        let scope = snapshot.history_scope;
        fixture.git(&["update-ref", "refs/heads/main", &expected[700].oid], b"");
        let mut read = |offset, session: &mut RepositorySession| {
            let Output::HistoryPage(result) = execute(
                Job::HistoryPage {
                    repo: repo.clone(),
                    scope: scope.clone(),
                    offset,
                    limit: 500,
                },
                &mut cache,
                &active(),
                session,
            )
            .unwrap() else {
                panic!("history page")
            };
            result
        };
        let page = read(500, &mut session);
        assert_eq!(page.page.commits, expected[500..1000]);
        assert_eq!(page.graph, expected_graph[500..1000]);
        let page = read(1000, &mut session);
        assert_eq!(page.page.commits, expected[1000..]);
        assert_eq!(page.graph, expected_graph[1000..]);
        assert_eq!(page.page.next_offset, None);
        let previous = read(500, &mut session);
        assert_eq!(previous.page.commits, expected[500..1000]);
        assert_eq!(previous.graph, expected_graph[500..1000]);
        execute(Job::ReleaseHistory, &mut cache, &active(), &mut session).unwrap();
        assert!(session.history.is_none());
    }

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
                head: None,
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
        let expected_bytes = preview.rgba.capacity()
            + preview.format.capacity()
            + 8
            + 2 * std::mem::size_of::<ImageSide>();
        let content = Content::Images {
            old: ImageSide {
                animation: None,
                captured: None,
                literal_source: None,
                image: Some(preview),
                render: Some(rendered),
                message: None,
                bytes: 8,
                lfs_pointer: None,
            }
            .into(),
            new: ImageSide {
                animation: None,
                captured: None,
                literal_source: None,
                image: None,
                render: None,
                message: None,
                bytes: 0,
                lfs_pointer: None,
            }
            .into(),
        };
        assert_eq!(content.bytes(), expected_bytes);
    }

    fn added_file(fixture: &Fixture, name: &str, bytes: &[u8]) -> FileChange {
        FileChange {
            old_path: None,
            new_path: Some(name.into()),
            old_oid: None,
            new_oid: Some(fixture.blob(bytes)),
            status: ChangeStatus::Added,
            old_mode: "000000".into(),
            new_mode: "100644".into(),
        }
    }

    fn captured_preview(repo: &GitRepository, file: FileChange) -> Arc<Content> {
        let Output::Preview(content, _) = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected captured preview")
        };
        content
    }

    #[test]
    fn gif_worker_retains_composed_frames_and_accounts_for_every_render_buffer() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                encoder
                    .encode_frame(image::Frame::from_parts(
                        image::RgbaImage::from_pixel(3, 2, image::Rgba(color)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(100, 1),
                    ))
                    .unwrap();
            }
        }
        let content = captured_preview(&repo, added_file(&fixture, "animation.data", &bytes));
        let Content::Images { new, .. } = &*content else {
            panic!("GIF magic should route to images");
        };
        let render = new.render.as_ref().unwrap();
        assert_eq!(render.frame_count(), 2);
        assert_eq!(new.animation.as_ref().unwrap().end_ms.len(), 2);
        assert_ne!(render.as_bytes(0), render.as_bytes(1));
        assert_eq!(new.captured.as_deref(), Some(bytes.as_slice()));
        let rendered_bytes: usize = (0..render.frame_count())
            .map(|frame| render.as_bytes(frame).unwrap().len())
            .sum();
        assert!(
            content.bytes()
                >= bytes.len() + rendered_bytes + new.image.as_ref().unwrap().rgba.len()
        );
    }

    #[test]
    fn model_worker_retains_obj_geometry_without_losing_captured_source_identity() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let source = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        fs::write(fixture.0.join("triangle.obj"), "different current file").unwrap();
        let content = captured_preview(&repo, added_file(&fixture, "triangle.obj", source));
        let Content::Rich(preview) = &*content else {
            panic!("ASCII model uses geometric preview");
        };
        assert_eq!(preview.new.metadata.format, "OBJ");
        let model = preview.new.model.as_ref().expect("retained model geometry");
        assert!(preview.new.pages.is_empty());
        assert_eq!(model.bookmark().target, [0.5, 0.5, 0.]);
        assert!(model.retained_bytes() > 0);
        assert!(content.bytes() >= source.len() + model.retained_bytes());
        assert_eq!(preview.new.captured.as_deref(), Some(source.as_slice()));
        assert_eq!(
            preview.new.metadata.source.as_deref().unwrap().as_bytes(),
            source
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join("triangle.obj")).unwrap(),
            "different current file"
        );
    }

    const GLB_BEFORE: &[u8] =
        include_bytes!("../../preview/tests/fixtures/models/glb/assembly-before.glb");
    const GLB_AFTER: &[u8] =
        include_bytes!("../../preview/tests/fixtures/models/glb/assembly-after.glb");
    const MESHOPT_GLB_BEFORE: &[u8] =
        include_bytes!("../../preview/tests/fixtures/models/glb/meshopt-arch-before.glb");
    const MESHOPT_GLB_AFTER: &[u8] =
        include_bytes!("../../preview/tests/fixtures/models/glb/meshopt-arch-after.glb");

    #[test]
    fn glb_history_preserves_captured_revisions_absence_and_independent_errors() {
        check_glb_history(GLB_BEFORE, GLB_AFTER);
    }

    #[test]
    fn meshopt_glb_history_preserves_captured_revisions_absence_and_independent_errors() {
        check_glb_history(MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER);
    }

    fn check_glb_history(before: &[u8], after: &[u8]) {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        fs::write(fixture.0.join("assembly.GLB"), b"unrelated worktree").unwrap();
        let mut file = added_file(&fixture, "assembly.GLB", after);
        file.old_path = Some("assembly.glb".into());
        file.old_oid = Some(fixture.blob(before));
        file.old_mode = "100644".into();
        file.status = ChangeStatus::Renamed;
        let content = captured_preview(&repo, file.clone());
        let Content::Rich(sides) = &*content else {
            panic!("native GLB comparison")
        };
        for (side, expected) in [(&sides.old, before), (&sides.new, after)] {
            assert_eq!(side.captured.as_deref(), Some(expected));
            assert!(
                side.metadata.source.is_none(),
                "binary bytes are not staging text"
            );
            assert!(side.model.is_some(), "{:?}", side.error);
            assert!(side.pages.is_empty(), "no eager fixed frames");
        }
        assert!(
            content.bytes()
                >= before.len()
                    + after.len()
                    + sides.old.model.as_ref().unwrap().retained_bytes()
                    + sides.new.model.as_ref().unwrap().retained_bytes()
        );

        file.old_oid = Some(fixture.blob(b"glTF truncated"));
        let invalid = captured_preview(&repo, file.clone());
        let Content::Rich(sides) = &*invalid else {
            panic!("side-specific GLB error")
        };
        assert!(sides.old.error.is_some());
        assert!(sides.old.present);
        assert_eq!(
            sides.old.captured.as_deref(),
            Some(b"glTF truncated".as_slice())
        );
        assert!(sides.new.model.is_some());

        file.old_oid = Some(fixture.blob(b""));
        let empty = captured_preview(&repo, file.clone());
        let Content::Rich(sides) = &*empty else {
            panic!("present empty GLB side")
        };
        assert!(sides.old.present && sides.old.error.is_some());
        assert_eq!(sides.old.captured.as_deref(), Some(b"".as_slice()));
        assert!(sides.new.model.is_some());

        for deleted in [false, true] {
            let mut absent = added_file(&fixture, "assembly.GLB", after);
            if deleted {
                std::mem::swap(&mut absent.old_path, &mut absent.new_path);
                std::mem::swap(&mut absent.old_oid, &mut absent.new_oid);
                std::mem::swap(&mut absent.old_mode, &mut absent.new_mode);
                absent.status = ChangeStatus::Deleted;
            }
            let content = captured_preview(&repo, absent);
            let Content::Rich(sides) = &*content else {
                panic!("absent GLB side")
            };
            let (missing, present) = if deleted {
                (&sides.new, &sides.old)
            } else {
                (&sides.old, &sides.new)
            };
            assert!(!missing.present);
            assert!(missing.captured.is_none() && missing.error.is_none());
            assert!(present.model.is_some());
        }
        assert_eq!(
            fs::read(fixture.0.join("assembly.GLB")).unwrap(),
            b"unrelated worktree"
        );
    }

    #[test]
    fn glb_working_preview_uses_current_bytes_without_caching_or_index_writes() {
        check_glb_working_preview(GLB_BEFORE, GLB_AFTER);
    }

    #[test]
    fn meshopt_glb_working_preview_uses_current_bytes_without_caching_or_index_writes() {
        check_glb_working_preview(MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER);
    }

    fn check_glb_working_preview(before: &[u8], after: &[u8]) {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("assembly.glb"), before).unwrap();
        fixture.git(&["add", "assembly.glb"], b"");
        fixture.git(&["-c", "commit.gpgsign=false", "commit", "-qm", "GLB"], b"");
        let repo = GitRepository::open(&fixture.0).unwrap();
        let index = fs::read(fixture.0.join(".git/index")).unwrap();
        let mut cache = PreviewCache::default();
        let mut session = RepositorySession::default();
        for bytes in [after, b"invalid GLB".as_slice(), after] {
            fs::write(fixture.0.join("assembly.glb"), bytes).unwrap();
            let entry = repo.status().unwrap().entries.remove(0);
            let Output::WorkingPreview(_, content, _) = execute(
                Job::WorkingPreview {
                    head: None,
                    repo: repo.clone(),
                    entry,
                    area: gitturtle_core::ChangeArea::Unstaged,
                },
                &mut cache,
                &active(),
                &mut session,
            )
            .unwrap() else {
                panic!("working GLB")
            };
            let Content::Rich(sides) = &*content else {
                panic!("rich GLB")
            };
            assert_eq!(sides.old.captured.as_deref(), Some(before));
            assert_eq!(sides.new.captured.as_deref(), Some(bytes));
            assert!(sides.old.model.is_some());
            assert_eq!(sides.new.model.is_some(), bytes == after);
            assert!(cache.entries.is_empty());
        }
        assert_eq!(fs::read(fixture.0.join(".git/index")).unwrap(), index);
    }

    #[test]
    fn glb_lfs_miss_is_retryable_and_verified_local_bytes_enter_the_model_pipeline() {
        check_glb_lfs_retry(GLB_AFTER);
    }

    #[test]
    fn meshopt_glb_lfs_miss_is_retryable_and_verified_local_bytes_enter_the_model_pipeline() {
        check_glb_lfs_retry(MESHOPT_GLB_AFTER);
    }

    fn check_glb_lfs_retry(bytes: &[u8]) {
        use sha2::{Digest, Sha256};
        let fixture = Fixture::new();
        let oid = format!("{:x}", Sha256::digest(bytes));
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
            bytes.len()
        );
        let file = added_file(&fixture, "assembly.glb", pointer.as_bytes());
        let repo = GitRepository::open(&fixture.0).unwrap();
        let mut cache = PreviewCache::default();
        let mut session = RepositorySession::default();
        let mut preview = |expected_entries| {
            let Output::Preview(content, _) = execute(
                Job::Preview {
                    origins: Default::default(),
                    repo: repo.clone(),
                    file: file.clone(),
                },
                &mut cache,
                &active(),
                &mut session,
            )
            .unwrap() else {
                panic!("LFS GLB")
            };
            assert_eq!(cache.entries.len(), expected_entries);
            content
        };
        let missing = preview(0);
        let Content::Rich(sides) = &*missing else {
            panic!("unavailable LFS model retains its captured pointer")
        };
        assert!(!sides.old.present && sides.old.error.is_none());
        assert!(sides.new.present && sides.new.error.is_some());
        assert_eq!(sides.new.captured.as_deref(), Some(pointer.as_bytes()));
        let targets = crate::lfs_download::preview_targets(&missing, &file);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].0, 1);
        assert_eq!(targets[0].1.pointer, pointer.as_bytes());
        assert_eq!(targets[0].1.blob_oid, file.new_oid);
        let directory = fixture
            .0
            .join(".git/lfs/objects")
            .join(&oid[..2])
            .join(&oid[2..4]);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(&oid), bytes).unwrap();
        let available = preview(1);
        let Content::Rich(sides) = &*available else {
            panic!("resolved GLB geometry")
        };
        assert_eq!(sides.new.captured.as_deref(), Some(bytes));
        assert!(sides.new.model.is_some(), "{:?}", sides.new.error);
        assert!(!sides.old.present);
        assert!(crate::lfs_download::preview_targets(&available, &file).is_empty());
        assert!(Arc::ptr_eq(&available, &preview(1)));
    }

    #[test]
    fn meshopt_glb_lfs_comparison_keeps_verified_side_and_retries_missing_or_corrupt_side() {
        use sha2::{Digest, Sha256};
        for unavailable_side in [0, 1] {
            let fixture = Fixture::new();
            let bytes = [MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER];
            let oids = bytes.map(|bytes| format!("{:x}", Sha256::digest(bytes)));
            let pointers: [String; 2] = std::array::from_fn(|i| {
                format!(
                    "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
                    oids[i],
                    bytes[i].len()
                )
            });
            let objects: [PathBuf; 2] = std::array::from_fn(|i| {
                let directory = fixture
                    .0
                    .join(".git/lfs/objects")
                    .join(&oids[i][..2])
                    .join(&oids[i][2..4]);
                fs::create_dir_all(&directory).unwrap();
                directory.join(&oids[i])
            });
            let available_side = 1 - unavailable_side;
            fs::write(&objects[available_side], bytes[available_side]).unwrap();
            let mut file = added_file(&fixture, "after.GLB", pointers[1].as_bytes());
            file.old_path = Some("before.glb".into());
            file.old_oid = Some(fixture.blob(pointers[0].as_bytes()));
            file.old_mode = "100644".into();
            file.status = ChangeStatus::Renamed;
            let repo = GitRepository::open(&fixture.0).unwrap();
            let mut cache = PreviewCache::default();
            let mut session = RepositorySession::default();
            let mut preview = |expected_cached| {
                let Output::Preview(content, _) = execute(
                    Job::Preview {
                        repo: repo.clone(),
                        file: file.clone(),
                        origins: Default::default(),
                    },
                    &mut cache,
                    &active(),
                    &mut session,
                )
                .unwrap() else {
                    panic!("LFS model comparison")
                };
                assert_eq!(cache.entries.len(), expected_cached);
                content
            };
            for corrupt in [false, true] {
                if corrupt {
                    // Preserve size so verification must check the SHA-256 too.
                    let mut damaged = bytes[unavailable_side].to_vec();
                    damaged[0] ^= 1;
                    fs::write(&objects[unavailable_side], damaged).unwrap();
                }
                let content = preview(0);
                let Content::Rich(comparison) = &*content else {
                    panic!("independent LFS model sides")
                };
                let sides = [&comparison.old, &comparison.new];
                let usable = sides[available_side];
                assert!(usable.model.is_some(), "{:?}", usable.error);
                assert_eq!(usable.captured.as_deref(), Some(bytes[available_side]));
                let unavailable = sides[unavailable_side];
                assert!(unavailable.present && unavailable.model.is_none());
                let error = unavailable.error.as_deref().unwrap();
                assert!(error.contains(if corrupt {
                    "SHA-256 verification"
                } else {
                    "unavailable in the local store"
                }));
                assert_eq!(
                    unavailable.captured.as_deref(),
                    Some(pointers[unavailable_side].as_bytes())
                );
                assert_eq!(
                    unavailable.metadata.source.as_deref(),
                    Some(pointers[unavailable_side].as_str())
                );
                let targets = crate::lfs_download::preview_targets(&content, &file);
                assert_eq!(targets.len(), 1);
                assert_eq!(targets[0].0, unavailable_side);
                assert_eq!(targets[0].1.pointer, pointers[unavailable_side].as_bytes());
                assert_eq!(
                    Some(&targets[0].1.path),
                    [file.old_path.as_ref(), file.new_path.as_ref()][unavailable_side]
                );
                assert_eq!(
                    targets[0].1.blob_oid.as_ref(),
                    [file.old_oid.as_ref(), file.new_oid.as_ref()][unavailable_side]
                );
            }
            fs::write(&objects[unavailable_side], bytes[unavailable_side]).unwrap();
            let resolved = preview(1);
            let Content::Rich(comparison) = &*resolved else {
                panic!("both verified LFS models")
            };
            for (side, bytes) in [&comparison.old, &comparison.new].into_iter().zip(bytes) {
                assert!(side.model.is_some() && side.error.is_none());
                assert_eq!(side.captured.as_deref(), Some(bytes));
            }
            assert!(crate::lfs_download::preview_targets(&resolved, &file).is_empty());
            assert!(Arc::ptr_eq(&resolved, &preview(1)));
        }
    }

    #[test]
    fn meshopt_glb_cache_reserves_geometry_and_frames_before_any_raster_is_requested() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let file = added_file(&fixture, "arch.glb", MESHOPT_GLB_AFTER);
        let key = PreviewKey::new(repo.path(), &file);
        let content = captured_preview(&repo, file);
        let weak = {
            let Content::Rich(sides) = &*content else {
                panic!("retained compressed GLB")
            };
            let model = sides.new.model.as_ref().unwrap();
            assert!(sides.new.pages.is_empty());
            assert!(model.retained_bytes() >= 36 * 3 * 3 * 8 + 2 * 720 * 720 * 4);
            assert!(content.bytes() >= MESHOPT_GLB_AFTER.len() + model.retained_bytes());
            Arc::downgrade(model)
        };
        let weight = content.bytes();
        let mut insufficient = PreviewCache::with_limits(weight - 1, 1);
        insufficient.insert(key.clone(), content.clone());
        assert!(insufficient.entries.is_empty());
        assert_eq!(insufficient.bytes, 0);
        let mut cache = PreviewCache::with_limits(weight, 1);
        cache.insert(key.clone(), content.clone());
        assert_eq!(cache.bytes, weight);
        assert!(Arc::ptr_eq(&cache.get(&key).unwrap(), &content));
        cache.remove(&key);
        assert_eq!(cache.bytes, 0);
        assert!(
            weak.upgrade().is_some(),
            "the active view still owns its model"
        );
        drop(content);
        assert!(
            weak.upgrade().is_none(),
            "the last content owner releases the model"
        );
    }

    #[test]
    fn meshopt_glb_working_lfs_keeps_pointer_targets_and_retries_without_index_or_worktree_writes()
    {
        use sha2::{Digest, Sha256};
        let fixture = Fixture::new();
        let oids = [MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER]
            .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
        let pointers: [String; 2] = std::array::from_fn(|i| {
            format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
                oids[i],
                [MESHOPT_GLB_BEFORE, MESHOPT_GLB_AFTER][i].len()
            )
        });
        let path = fixture.0.join("assembly.glb");
        fs::write(&path, &pointers[0]).unwrap();
        fixture.git(&["add", "assembly.glb"], b"");
        fixture.git(&["-c", "commit.gpgsign=false", "commit", "-qm", "LFS"], b"");
        let index = fs::read(fixture.0.join(".git/index")).unwrap();
        fs::write(&path, &pointers[1]).unwrap();
        let objects: [PathBuf; 2] = std::array::from_fn(|i| {
            let directory = fixture
                .0
                .join(".git/lfs/objects")
                .join(&oids[i][..2])
                .join(&oids[i][2..4]);
            fs::create_dir_all(&directory).unwrap();
            directory.join(&oids[i])
        });
        fs::write(&objects[0], MESHOPT_GLB_BEFORE).unwrap();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let mut cache = PreviewCache::default();
        for available in [false, true] {
            if available {
                fs::write(&objects[1], MESHOPT_GLB_AFTER).unwrap();
            }
            let entry = repo.status().unwrap().entries.remove(0);
            let Output::WorkingPreview(file, content, _) = execute(
                Job::WorkingPreview {
                    repo: repo.clone(),
                    entry,
                    area: gitturtle_core::ChangeArea::Unstaged,
                    head: None,
                },
                &mut cache,
                &active(),
                &mut RepositorySession::default(),
            )
            .unwrap() else {
                panic!("working LFS comparison")
            };
            let Content::Rich(sides) = &*content else {
                panic!("working LFS model sides")
            };
            assert!(sides.old.model.is_some() && sides.old.error.is_none());
            assert_eq!(sides.old.captured.as_deref(), Some(MESHOPT_GLB_BEFORE));
            assert_eq!(sides.new.model.is_some(), available);
            let targets = crate::lfs_download::preview_targets(&content, &file);
            if available {
                assert!(targets.is_empty());
                assert_eq!(sides.new.captured.as_deref(), Some(MESHOPT_GLB_AFTER));
            } else {
                assert_eq!(targets.len(), 1);
                assert_eq!(targets[0].0, 1);
                assert_eq!(targets[0].1.path, Path::new("assembly.glb"));
                assert!(targets[0].1.blob_oid.is_none());
                assert_eq!(targets[0].1.pointer, pointers[1].as_bytes());
            }
            assert!(cache.entries.is_empty());
            assert_eq!(fs::read(fixture.0.join(".git/index")).unwrap(), index);
            assert_eq!(fs::read(&path).unwrap(), pointers[1].as_bytes());
        }
    }

    #[test]
    fn meshopt_glb_cancellation_is_propagated_instead_of_retaining_a_failed_side() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let prepared = crate::rich_preview::Side::prepare(
            MESHOPT_GLB_AFTER.to_vec(),
            Path::new("arch.glb"),
            || {
                calls.set(calls.get() + 1);
                Ok(())
            },
        )
        .unwrap();
        assert!(prepared.model.is_some(), "{:?}", prepared.error);
        let checkpoints = calls.get();
        drop(prepared);
        for stop_at in [1, checkpoints / 2, checkpoints] {
            let cancellation = active();
            calls.set(0);
            let result = crate::rich_preview::Side::prepare(
                MESHOPT_GLB_AFTER.to_vec(),
                Path::new("arch.glb"),
                || {
                    calls.set(calls.get() + 1);
                    if calls.get() >= stop_at {
                        cancellation.history.cancel();
                    }
                    cancellation.check()
                },
            );
            let error = result
                .err()
                .expect("cancelled preparation must not retain content");
            assert!(error.to_string().contains("superseded"));
        }
    }

    #[test]
    fn mermaid_history_keeps_literal_renamed_sources_and_failed_diagrams() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let old = "flowchart LR\r\nA[Before]-->B\r\n";
        let new = "flowchart LR\r\nA[After]-->B\r\n";
        fs::write(fixture.0.join("new.mmd"), "different current source").unwrap();
        let mut file = added_file(&fixture, "new.mmd", new.as_bytes());
        file.old_path = Some("old.mermaid".into());
        file.old_oid = Some(fixture.blob(old.as_bytes()));
        file.old_mode = "100644".into();
        file.status = ChangeStatus::Renamed;
        let content = captured_preview(&repo, file);
        let Content::Text {
            old: actual_old,
            new: actual_new,
            diagrams: Some(diagrams),
            ..
        } = &*content
        else {
            panic!("Mermaid must retain text");
        };
        assert_eq!(actual_old, old);
        assert_eq!(actual_new, new);
        assert_eq!(diagrams.old.captured.as_deref(), Some(old.as_bytes()));
        assert_eq!(diagrams.new.captured.as_deref(), Some(new.as_bytes()));
        assert!(diagrams.old.pages[0].render.is_some() && diagrams.new.pages[0].render.is_some());
        assert!(content.bytes() >= diagrams.retained_bytes() + old.len() + new.len());
        let broken = "sequenceDiagram\n%%{init: {}}%%\nAlice->>Bob: literal";
        let content =
            captured_preview(&repo, added_file(&fixture, "broken.mmd", broken.as_bytes()));
        assert!(
            matches!(&*content, Content::Text { new, diagrams: Some(diagrams), .. } if new == broken && diagrams.new.pages[0].error.is_some())
        );
        let mut symlink = added_file(&fixture, "link.mmd", old.as_bytes());
        symlink.new_mode = "120000".into();
        assert!(matches!(
            &*captured_preview(&repo, symlink),
            Content::Text { diagrams: None, .. }
        ));
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{LFS_OID}\nsize {}\n",
            SVG.len()
        );
        let unavailable = captured_preview(
            &repo,
            added_file(&fixture, "missing.mmd", pointer.as_bytes()),
        );
        assert!(
            matches!(&*unavailable, Content::Text {diagrams: None, new, ..} if new == &pointer)
        );
    }

    #[test]
    fn mermaid_working_and_quick_open_keep_exact_staging_and_review_sources() {
        let fixture = Fixture::new();
        let old = "# Workflow\n```mermaid\nflowchart LR; A-->B\n```\n";
        let new = "# Workflow\n```mermaid\nflowchart LR; A-->C\n```\n";
        fs::write(fixture.0.join("README.md"), old).unwrap();
        fixture.git(&["add", "README.md"], b"");
        fixture.git(
            &["-c", "commit.gpgsign=false", "commit", "-qm", "Chart"],
            b"",
        );
        fs::write(fixture.0.join("README.md"), new).unwrap();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let index = fs::read(fixture.0.join(".git/index")).unwrap();
        let Output::WorkingPreview(_, content, _) = execute(
            Job::WorkingPreview {
                head: None,
                repo: repo.clone(),
                entry: repo.status().unwrap().entries.remove(0),
                area: gitturtle_core::ChangeArea::Unstaged,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("working preview");
        };
        let Content::Text {
            diagrams: Some(diagrams),
            partial: Some(_),
            new: actual,
            ..
        } = &*content
        else {
            panic!("Chart retains exact partial staging");
        };
        assert_eq!(actual, new);
        let reviewed = crate::text_review::prepare(
            &content,
            crate::text_review::Options {
                hide_whitespace: true,
                context: 3,
            },
            || Ok(()),
        )
        .unwrap();
        assert!(
            matches!(&*reviewed, Content::Text { diagrams: Some(retained), partial: None, .. } if Arc::ptr_eq(retained, diagrams))
        );
        let paths = repo
            .search_tracked_paths(
                &gitturtle_core::PathScope::Worktree,
                "README",
                &gitturtle_core::HistoryCancellation::default(),
            )
            .unwrap();
        let Output::Preview(quick, _) = execute(
            Job::TrackedPreview {
                repo,
                entry: paths.entries[0].clone(),
                scope: gitturtle_core::PathScope::Worktree,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("quick preview");
        };
        assert!(
            matches!(&*quick, Content::Text { diagrams: Some(diagrams), partial: None, new: actual, .. } if actual == new && !diagrams.old.present && diagrams.new.pages[0].render.is_some())
        );
        assert_eq!(fs::read(fixture.0.join(".git/index")).unwrap(), index);
        assert_eq!(
            fs::read_to_string(fixture.0.join("README.md")).unwrap(),
            new
        );
    }

    #[test]
    fn content_detection_preserves_mislabeled_source_and_image_bytes() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let source = b"const literal = '<svg is just text>';\n";
        let preview = captured_preview(&repo, added_file(&fixture, "misleading.png", source));
        assert!(matches!(&*preview,Content::Text {new,..} if new.as_bytes()==source));
        let image = captured_preview(&repo, added_file(&fixture, "no-extension", SVG));
        let Content::Images { old, new } = &*image else {
            panic!("content should identify SVG")
        };
        assert!(old.image.is_none());
        assert_eq!(new.captured.as_deref(), Some(SVG));
        assert_eq!(new.literal_source.as_deref().unwrap().as_bytes(), SVG);
        assert!(image.bytes() >= SVG.len() * 2 + 8);
    }

    #[test]
    fn rich_history_keeps_exact_old_new_and_missing_sides_without_worktree_substitution() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let old = b"RIFF\0\0\0\0WAVE";
        let new = b"RIFF\x01\0\0\0WAVE";
        fs::write(fixture.0.join("sound.wav"), b"unrelated current worktree").unwrap();
        let mut file = added_file(&fixture, "sound.wav", new);
        file.old_path = Some("old-name.wav".into());
        file.old_oid = Some(fixture.blob(old));
        file.old_mode = "100644".into();
        file.status = ChangeStatus::Renamed;
        let preview = captured_preview(&repo, file.clone());
        let Content::Rich(sides) = &*preview else {
            panic!("expected media metadata")
        };
        assert_eq!(sides.old.captured.as_deref(), Some(old.as_slice()));
        assert_eq!(sides.new.captured.as_deref(), Some(new.as_slice()));
        assert_eq!(sides.old.metadata.format, "WAV audio");
        file.old_oid = Some("f".repeat(40));
        let missing = captured_preview(&repo, file);
        let Content::Rich(sides) = &*missing else {
            panic!("expected independent unavailable side")
        };
        assert!(sides.old.error.is_some());
        assert_eq!(sides.new.captured.as_deref(), Some(new.as_slice()));
        assert_eq!(
            fs::read(fixture.0.join("sound.wav")).unwrap(),
            b"unrelated current worktree"
        );
    }

    #[test]
    fn quick_open_and_working_changes_share_bounded_media_routing() {
        let fixture = Fixture::new();
        let original = b"RIFF\0\0\0\0WAVE";
        let changed = b"RIFF\x01\0\0\0WAVE";
        fs::write(fixture.0.join("sound.wav"), original).unwrap();
        fixture.git(&["add", "sound.wav"], b"");
        fixture.git(
            &["-c", "commit.gpgsign=false", "commit", "-qm", "Audio"],
            b"",
        );
        fs::write(fixture.0.join("sound.wav"), changed).unwrap();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let entry = repo.status().unwrap().entries.remove(0);
        let Output::WorkingPreview(_, content, _) = execute(
            Job::WorkingPreview {
                head: None,
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
        let Content::Rich(sides) = &*content else {
            panic!("expected media preview")
        };
        assert_eq!(sides.old.captured.as_deref(), Some(original.as_slice()));
        assert_eq!(sides.new.captured.as_deref(), Some(changed.as_slice()));
        let paths = repo
            .search_tracked_paths(
                &gitturtle_core::PathScope::Worktree,
                "sound",
                &gitturtle_core::HistoryCancellation::default(),
            )
            .unwrap();
        let Output::Preview(content, _) = execute(
            Job::TrackedPreview {
                repo,
                entry: paths.entries[0].clone(),
                scope: gitturtle_core::PathScope::Worktree,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected quick source")
        };
        let Content::Rich(sides) = &*content else {
            panic!("expected quick source media preview")
        };
        assert!(!sides.old.present);
        assert_eq!(sides.new.captured.as_deref(), Some(changed.as_slice()));
    }

    #[test]
    fn stored_symlink_named_as_image_is_literal_and_never_followed() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let target = b"../private-file.png";
        let mut file = added_file(&fixture, "link.png", target);
        file.new_mode = "120000".into();
        let preview = captured_preview(&repo, file);
        assert!(matches!(&*preview,Content::Text {new,..} if new.as_bytes()==target));
    }

    #[test]
    fn missing_image_object_keeps_the_readable_side_rendered() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let mut file = added_file(&fixture, "image.svg", SVG);
        file.old_path = Some("image.svg".into());
        file.old_mode = "100644".into();
        file.old_oid = Some("f".repeat(40));
        let preview = captured_preview(&repo, file);
        let Content::Images { old, new } = &*preview else {
            panic!("expected independent image sides")
        };
        assert!(old.message.is_some());
        assert!(new.render.is_some());
        assert_eq!(new.captured.as_deref(), Some(SVG));
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
                    origins: Default::default(),
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
    fn lfs_text_preview_retries_missing_pointer_and_displays_verified_content() {
        let fixture = Fixture::new();
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{LFS_OID}\nsize {}\n",
            SVG.len()
        );
        let repo = GitRepository::open(&fixture.0).unwrap();
        let file = FileChange {
            old_path: None,
            new_path: Some("source.txt".into()),
            old_oid: None,
            new_oid: Some(fixture.blob(pointer.as_bytes())),
            status: ChangeStatus::Added,
            old_mode: "000000".into(),
            new_mode: "100644".into(),
        };
        let mut cache = PreviewCache::default();
        let mut session = RepositorySession::default();
        let missing = execute(
            Job::Preview {
                origins: Default::default(),
                repo: repo.clone(),
                file: file.clone(),
            },
            &mut cache,
            &active(),
            &mut session,
        )
        .unwrap();
        assert!(
            matches!(missing, Output::Preview(content, _) if matches!(content.as_ref(), Content::Text { new, .. } if new == &pointer))
        );
        assert!(cache.entries.is_empty());
        let directory = fixture
            .0
            .join(".git/lfs/objects")
            .join(&LFS_OID[..2])
            .join(&LFS_OID[2..4]);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(LFS_OID), SVG).unwrap();
        let available = execute(
            Job::Preview {
                repo,
                file,
                origins: Default::default(),
            },
            &mut cache,
            &active(),
            &mut session,
        )
        .unwrap();
        assert!(
            matches!(available, Output::Preview(content, _) if matches!(content.as_ref(), Content::Text { new, partial, .. } if new.as_bytes() == SVG && partial.is_none()))
        );
        assert_eq!(cache.entries.len(), 1);
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
    fn retained_session_reopens_replaced_repository_at_the_same_path() {
        for replace_root in [true, false] {
            let fixture = Fixture::new();
            let replacement = Fixture::new();
            let old_blob = fixture.blob(b"old repository object\n");
            let new_blob = replacement.blob(b"replacement repository object\n");
            let mut session = RepositorySession::default();
            assert_eq!(
                session.open(&fixture.0).unwrap().blob(&old_blob).unwrap(),
                b"old repository object\n"
            );
            let retained = Arc::downgrade(&session.current.as_ref().unwrap().repository);
            let preserved = tempfile::tempdir().unwrap();
            let (original, incoming) = if replace_root {
                (fixture.0.clone(), replacement.0.clone())
            } else {
                (fixture.0.join(".git"), replacement.0.join(".git"))
            };
            fs::rename(&original, preserved.path().join("original")).unwrap();
            fs::rename(&incoming, &original).unwrap();
            let reopened = session.open(&fixture.0).unwrap();
            assert_eq!(
                reopened.blob(&new_blob).unwrap(),
                b"replacement repository object\n"
            );
            assert!(
                retained.upgrade().is_none(),
                "replacement must release the previous object reader owner"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn retained_session_reopens_in_place_gitdir_redirection_with_preserved_mtime() {
        use std::os::unix::fs::MetadataExt;
        let fixture = Fixture::new();
        let replacement = Fixture::new();
        let old_blob = fixture.blob(b"old linked repository object\n");
        let new_blob = replacement.blob(b"new linked repository object\n");
        let directories = tempfile::tempdir().unwrap();
        let original = directories.path().join("admin-a");
        let incoming = directories.path().join("admin-b");
        fs::rename(fixture.0.join(".git"), &original).unwrap();
        fs::rename(replacement.0.join(".git"), &incoming).unwrap();
        let git_link = fixture.0.join(".git");
        let old_pointer = format!("gitdir: {}\n", original.display());
        let new_pointer = format!("gitdir: {}\n", incoming.display());
        assert_eq!(old_pointer.len(), new_pointer.len());
        fs::write(&git_link, old_pointer).unwrap();
        let mut session = RepositorySession::default();
        assert_eq!(
            session.open(&fixture.0).unwrap().blob(&old_blob).unwrap(),
            b"old linked repository object\n"
        );
        let before = fs::metadata(&git_link).unwrap();
        std::thread::sleep(Duration::from_millis(2));
        let mut file = fs::OpenOptions::new().write(true).open(&git_link).unwrap();
        file.write_all(new_pointer.as_bytes()).unwrap();
        file.set_modified(before.modified().unwrap()).unwrap();
        drop(file);
        let after = fs::metadata(&git_link).unwrap();
        assert_eq!(before.ino(), after.ino());
        assert_eq!(before.len(), after.len());
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        assert!(original.is_dir() && incoming.is_dir());
        assert_eq!(
            session.open(&fixture.0).unwrap().blob(&new_blob).unwrap(),
            b"new linked repository object\n"
        );
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
