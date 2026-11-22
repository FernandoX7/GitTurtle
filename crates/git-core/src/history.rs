//! Bounded, cancellable history reads. These APIs belong on a worker thread.
use super::*;
use crate::case_fold::CaseInsensitiveMatcher;
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::{
    Condvar, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub const MAX_HISTORY_PAGE: usize = 500;
const MAX_SEARCH_SCAN: usize = 50_000;
const MAX_SEARCH_BYTES: usize = 64 * 1024 * 1024;
const MAX_HISTORY_RECORD: usize = 2 * 1024 * 1024;
const MAX_FILE_HISTORY_BYTES: usize = 32 * 1024 * 1024;
const MAX_HISTORY_REFS: usize = 16_384;
const MAX_HISTORY_REF_BYTES: usize = MAX_HISTORY_REFS * 65;
/// First-commit timeout for `--topo-order` when a repository has no Git
/// commit-graph. Exceeding this restarts the walk in reverse-chronological
/// order so large repositories stream immediately instead of timing out.
const TOPO_FALLBACK_TIMEOUT: Duration = Duration::from_secs(4);
/// Metadata returned by one ordinary-history page, separate from the bounded
/// process pipe queue and the caller's retained history window.
pub const MAX_HISTORY_PAGE_BYTES: usize = 32 * 1024 * 1024;

/// Commit ordering used by a history traversal or search stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HistoryOrder {
    /// Git `--topo-order`: keeps branch lanes coherent for graph connections.
    #[default]
    Topological,
    /// Git's default reverse-chronological priority-queue walk: streams
    /// immediately without waiting for a full topological sort when no
    /// commit-graph is present.
    Chronological,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryTraversalStop {
    Exhausted,
    PageFull,
    ByteLimit,
}

#[derive(Clone, Debug)]
pub struct HistoryTraversalPage {
    pub commits: Vec<Commit>,
    /// Absolute row of the first commit in the captured traversal.
    pub offset: usize,
    /// A full page can require one final, empty read to establish exhaustion.
    pub next_offset: Option<usize>,
    pub stop: HistoryTraversalStop,
    /// Commit ordering actually produced for this traversal.
    pub order: HistoryOrder,
}

struct ChronologicalFallback {
    repo: GitRepository,
    skip: usize,
    timeout: Duration,
}

/// One immutable history traversal, intended to be owned by a worker.
/// Git walks the captured tips once; pages neither replay a growing prefix nor
/// use `--skip`. Only a partial record, one deferred commit and eight 8 KiB pipe
/// chunks can wait ahead of the current page. Git's own revision-walk memory is
/// independent of this metadata bound. Drop closes and reaps the process.
pub struct HistoryTraversal {
    scope: HistoryScope,
    order: HistoryOrder,
    offset: usize,
    stream: Option<HistoryStream>,
    deferred: Option<Commit>,
    fallback: Option<ChronologicalFallback>,
    failed: bool,
}

impl HistoryTraversal {
    pub fn scope(&self) -> &HistoryScope {
        &self.scope
    }

    pub fn order(&self) -> HistoryOrder {
        self.order
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Continue the same captured ordering. Cancellation/deadline/parse errors
    /// close the process and invalidate the cursor: callers must visibly
    /// restart, never silently accept a page after a partially consumed read.
    pub fn next_page(
        &mut self,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<HistoryTraversalPage> {
        ensure!(
            (1..=MAX_HISTORY_PAGE).contains(&limit),
            "History page must contain 1–500 results"
        );
        ensure!(
            !self.failed,
            "History traversal stopped; restart history to continue"
        );
        let result = self.read_page(limit, cancellation);
        if result.is_err() {
            self.stream = None;
            self.deferred = None;
            self.fallback = None;
            self.failed = true;
        }
        result
    }

    fn read_page(
        &mut self,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<HistoryTraversalPage> {
        cancellation.check()?;
        let mut start = Instant::now();
        let offset = self.offset;
        let mut commits = Vec::with_capacity(limit);
        let mut bytes = 0usize;
        let mut _cancel = self.stream.as_ref().map(|stream| {
            let wake_ch = Arc::clone(&stream.channel);
            cancellation.on_cancel(move || wake_ch.wake_reader())
        });
        let stop = loop {
            cancellation.check()?;
            if commits.len() == limit {
                break HistoryTraversalStop::PageFull;
            }
            let next = if let Some(commit) = self.deferred.take() {
                Some(commit)
            } else if let Some(stream) = self.stream.as_mut() {
                let first_with_fallback =
                    offset == 0 && commits.is_empty() && self.fallback.is_some();
                let wait_limit = if first_with_fallback {
                    self.fallback
                        .as_ref()
                        .map_or(GIT_TIMEOUT, |fb| fb.timeout.min(GIT_TIMEOUT))
                } else {
                    GIT_TIMEOUT
                };
                match stream.next_raw_record(cancellation, start, wait_limit)? {
                    RawRecordOutcome::Record(_) => {
                        self.fallback = None;
                        Some(parse_commit_fields(&stream.current_fields())?)
                    }
                    RawRecordOutcome::Eof => {
                        self.fallback = None;
                        None
                    }
                    RawRecordOutcome::TimedOut if first_with_fallback => {
                        let fallback = self.fallback.take().expect("checked above");
                        // Drop stalled `--topo-order` stream before spawning chronological fallback.
                        drop(_cancel);
                        self.stream = None;
                        cancellation.check()?;
                        let (cmd, input) = build_history_log_command(
                            fallback.repo.target(),
                            &self.scope,
                            HistoryOrder::Chronological,
                            fallback.skip,
                        )?;
                        let new_stream = HistoryStream::start(cmd, input)?;
                        let wake_ch = Arc::clone(&new_stream.channel);
                        _cancel = Some(cancellation.on_cancel(move || wake_ch.wake_reader()));
                        self.stream = Some(new_stream);
                        self.order = HistoryOrder::Chronological;
                        start = Instant::now();
                        continue;
                    }
                    RawRecordOutcome::TimedOut => {
                        bail!(
                            "History page exceeded its 15 second time limit; restart history to continue"
                        );
                    }
                }
            } else {
                None
            };
            let Some(commit) = next else {
                self.stream = None;
                break HistoryTraversalStop::Exhausted;
            };
            let commit_bytes = commit.history_bytes();
            if bytes.saturating_add(commit_bytes) > MAX_HISTORY_PAGE_BYTES && !commits.is_empty() {
                self.deferred = Some(commit);
                break HistoryTraversalStop::ByteLimit;
            }
            bytes = bytes.saturating_add(commit_bytes);
            commits.push(commit);
        };
        cancellation.check()?;
        self.offset = offset
            .checked_add(commits.len())
            .context("History offset overflow")?;
        Ok(HistoryTraversalPage {
            commits,
            offset,
            next_offset: (stop != HistoryTraversalStop::Exhausted).then_some(self.offset),
            stop,
            order: self.order,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickaxeMode {
    /// Commits where occurrences of the literal string changed (`git log -S`).
    String,
    /// Commits whose diff contains added or removed lines matching a regex (`git log -G`).
    Regex,
}

/// Optional structured filters combined with a history search query.
/// All text fields are bounded and passed to Git as attached single-argument
/// options or literal `--` pathspecs so repository or user input can never
/// inject command flags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistorySearchFilter {
    pub pickaxe: Option<(PickaxeMode, String)>,
    pub author: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub paths: Vec<PathBuf>,
    pub regex: bool,
    pub first_parent: bool,
    pub no_merges: bool,
}

impl HistorySearchFilter {
    pub fn is_empty(&self) -> bool {
        !self.filters_without_query() && !self.regex
    }

    /// True when at least one filter narrows history even with an empty message query.
    pub fn filters_without_query(&self) -> bool {
        self.pickaxe
            .as_ref()
            .is_some_and(|(_, pattern)| !pattern.is_empty())
            || self.author.as_deref().is_some_and(|v| !v.is_empty())
            || self.since.as_deref().is_some_and(|v| !v.is_empty())
            || self.until.as_deref().is_some_and(|v| !v.is_empty())
            || !self.paths.is_empty()
            || self.first_parent
            || self.no_merges
    }

    pub fn active_count(&self) -> usize {
        usize::from(
            self.pickaxe
                .as_ref()
                .is_some_and(|(_, pattern)| !pattern.is_empty()),
        ) + usize::from(self.author.as_deref().is_some_and(|v| !v.is_empty()))
            + usize::from(self.since.as_deref().is_some_and(|v| !v.is_empty()))
            + usize::from(self.until.as_deref().is_some_and(|v| !v.is_empty()))
            + usize::from(!self.paths.is_empty())
            + usize::from(self.regex)
            + usize::from(self.first_parent)
            + usize::from(self.no_merges)
    }

    pub fn validate(&self) -> Result<()> {
        let check_field = |value: &str, label: &str| -> Result<()> {
            ensure!(value.len() <= 4096, "{label} exceeds 4 KiB");
            ensure!(
                !value.contains(['\0', '\r', '\n']),
                "{label} must be a single line without NUL bytes"
            );
            Ok(())
        };
        if let Some((_, pattern)) = &self.pickaxe {
            check_field(pattern, "Diff content search pattern")?;
        }
        if let Some(author) = &self.author {
            check_field(author, "Author filter")?;
        }
        if let Some(since) = &self.since {
            check_field(since, "Since date filter")?;
        }
        if let Some(until) = &self.until {
            check_field(until, "Until date filter")?;
        }
        ensure!(
            self.paths.len() <= 64,
            "At most 64 path filters are supported per history search"
        );
        for path in &self.paths {
            let raw = path.as_os_str().as_encoded_bytes();
            ensure!(
                !raw.is_empty()
                    && raw.len() <= 16 * 1024
                    && !raw.contains(&0)
                    && path
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                "Path filters must be literal repository-relative paths"
            );
        }
        Ok(())
    }
}

/// Stateful streaming history search cursor owned by a worker.
/// Keeps the underlying `git log` stream open between Continue pages so
/// subsequent pages resume directly at the next commit without `--skip` replay
/// and without allocating `Commit` strings for non-matching commits.
pub struct HistorySearch {
    scope: HistoryScope,
    query: String,
    filter: HistorySearchFilter,
    matcher: CaseInsensitiveMatcher,
    order: HistoryOrder,
    offset: usize,
    stream: Option<HistoryStream>,
    fallback: Option<ChronologicalFallback>,
    scratch: String,
    failed: bool,
}

impl HistorySearch {
    pub fn scope(&self) -> &HistoryScope {
        &self.scope
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn filter(&self) -> &HistorySearchFilter {
        &self.filter
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn order(&self) -> HistoryOrder {
        self.order
    }

    /// Reads the next bounded search page from the active stream.
    /// Cancellation or stream errors close and invalidate the cursor.
    pub fn next_page(
        &mut self,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearchPage> {
        ensure!(
            (1..=MAX_HISTORY_PAGE).contains(&limit),
            "History page must contain 1–500 results"
        );
        ensure!(
            !self.failed,
            "History search stopped; restart search to continue"
        );
        let result = self.read_page(limit, cancellation);
        if result.is_err() {
            self.stream = None;
            self.fallback = None;
            self.failed = true;
        }
        result
    }

    fn read_page(
        &mut self,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearchPage> {
        cancellation.check()?;
        let mut start = Instant::now();
        let mut commits = Vec::new();
        let mut scanned = 0usize;
        let mut page_bytes = 0usize;
        let mut _cancel = self.stream.as_ref().map(|stream| {
            let wake_ch = Arc::clone(&stream.channel);
            cancellation.on_cancel(move || wake_ch.wake_reader())
        });
        let stop = loop {
            cancellation.check()?;
            if commits.len() == limit {
                break HistorySearchStop::PageFull;
            }
            if scanned == MAX_SEARCH_SCAN {
                break HistorySearchStop::ScanLimit;
            }
            if page_bytes > MAX_SEARCH_BYTES {
                break HistorySearchStop::ByteLimit;
            }
            let Some(stream) = self.stream.as_mut() else {
                break HistorySearchStop::Exhausted;
            };
            let first_with_fallback = self.offset == 0 && scanned == 0 && self.fallback.is_some();
            let wait_limit = if first_with_fallback {
                self.fallback
                    .as_ref()
                    .map_or(GIT_TIMEOUT, |fb| fb.timeout.min(GIT_TIMEOUT))
            } else {
                GIT_TIMEOUT
            };
            match stream.next_raw_record(cancellation, start, wait_limit)? {
                RawRecordOutcome::Eof => {
                    self.fallback = None;
                    self.stream = None;
                    break HistorySearchStop::Exhausted;
                }
                RawRecordOutcome::TimedOut if first_with_fallback => {
                    let fallback = self.fallback.take().expect("checked above");
                    drop(_cancel);
                    self.stream = None;
                    cancellation.check()?;
                    let (cmd, input) = build_history_log_command_filtered(
                        fallback.repo.target(),
                        &self.scope,
                        HistoryOrder::Chronological,
                        fallback.skip,
                        &self.query,
                        &self.filter,
                    )?;
                    let new_stream = HistoryStream::start(cmd, input)?;
                    let wake_ch = Arc::clone(&new_stream.channel);
                    _cancel = Some(cancellation.on_cancel(move || wake_ch.wake_reader()));
                    self.stream = Some(new_stream);
                    self.order = HistoryOrder::Chronological;
                    start = Instant::now();
                    continue;
                }
                RawRecordOutcome::TimedOut => {
                    break HistorySearchStop::TimeLimit;
                }
                RawRecordOutcome::Record(rec_bytes) => {
                    self.fallback = None;
                    page_bytes = page_bytes.saturating_add(rec_bytes);
                    let fields = stream.current_fields();
                    let body = trim_trailing_newlines(fields[5]);
                    scanned += 1;
                    let matched = self.filter.regex
                        || self.matcher.is_empty()
                        || [fields[0], fields[2], fields[4], body]
                            .iter()
                            .any(|field| self.matcher.matches_bytes(field, &mut self.scratch));
                    if matched {
                        commits.push(parse_commit_fields(&fields)?);
                    }
                    if commits.len() == limit {
                        break HistorySearchStop::PageFull;
                    }
                    if scanned == MAX_SEARCH_SCAN {
                        break HistorySearchStop::ScanLimit;
                    }
                    if page_bytes > MAX_SEARCH_BYTES {
                        break HistorySearchStop::ByteLimit;
                    }
                }
            }
        };
        cancellation.check()?;
        self.offset = self
            .offset
            .checked_add(scanned)
            .context("History offset overflow")?;
        if self.scratch.capacity() > 64 * 1024 {
            self.scratch = String::new();
        }
        if let Some(stream) = self.stream.as_mut() {
            stream.shrink_idle_buffer();
        }
        Ok(HistorySearchPage {
            scope: self.scope.clone(),
            commits,
            scanned,
            next_offset: (stop != HistorySearchStop::Exhausted).then_some(self.offset),
            stop,
        })
    }
}

fn trim_trailing_newlines(mut bytes: &[u8]) -> &[u8] {
    while let Some(stripped) = bytes.strip_suffix(b"\n") {
        bytes = stripped;
    }
    bytes
}

impl Commit {
    /// Retained metadata accounting for ordinary history windows.
    pub fn history_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.oid.capacity()
            + self.parents.capacity() * std::mem::size_of::<String>()
            + self.parents.iter().map(String::capacity).sum::<usize>()
            + self.author.capacity()
            + self.subject.capacity()
            + self.body.capacity()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryScope {
    /// Begin a new snapshot of local refs, including remote-tracking refs and
    /// detached HEAD. Continue with the returned page's pinned scope. Refresh
    /// by starting again with AllRefs and offset zero. No network access.
    AllRefs,
    /// Immutable tips returned for an AllRefs search session.
    PinnedRefs(Vec<String>),
    /// Ancestors of one immutable, full commit OID.
    FromCommit(String),
}

type CancelCallback = Box<dyn Fn() + Send + Sync>;

/// Unregisters a cancellation wake callback when dropped so long-lived
/// cancellation tokens never accumulate stale closures.
pub struct CancelGuard {
    waiters: Weak<Mutex<Vec<(u64, CancelCallback)>>>,
    id: u64,
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if let Some(waiters) = self.waiters.upgrade() {
            let mut list = waiters.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(pos) = list.iter().position(|(id, _)| *id == self.id) {
                let _ = list.swap_remove(pos);
            }
        }
    }
}

pub(crate) struct CancelWaiters {
    next_id: AtomicU64,
    waiters: Arc<Mutex<Vec<(u64, CancelCallback)>>>,
}

impl Default for CancelWaiters {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            waiters: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl CancelWaiters {
    pub(crate) fn register(
        &self,
        cancelled: &AtomicBool,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> CancelGuard {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut list = self.waiters.lock().unwrap_or_else(|e| e.into_inner());
        if cancelled.load(Ordering::Acquire) {
            drop(list);
            wake();
            return CancelGuard {
                waiters: Weak::new(),
                id: 0,
            };
        }
        list.push((id, Box::new(wake)));
        CancelGuard {
            waiters: Arc::downgrade(&self.waiters),
            id,
        }
    }

    pub(crate) fn wake_all(&self) {
        let callbacks = {
            let mut list = self.waiters.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *list)
        };
        for (_, wake) in callbacks {
            wake();
        }
    }
}

#[derive(Default)]
struct CancelState {
    cancelled: AtomicBool,
    waiters: CancelWaiters,
}

#[derive(Clone, Default)]
pub struct HistoryCancellation(Arc<CancelState>);

impl std::fmt::Debug for HistoryCancellation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HistoryCancellation")
            .field(&self.is_cancelled())
            .finish()
    }
}

impl HistoryCancellation {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.waiters.wake_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    /// Registers a wake callback invoked immediately if already cancelled or
    /// when [`Self::cancel`] is called. Dropped [`CancelGuard`] unregisters it.
    pub fn on_cancel(&self, wake: impl Fn() + Send + Sync + 'static) -> CancelGuard {
        self.0.waiters.register(&self.0.cancelled, wake)
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            return Err(CoreError::ReadCancelled.into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistorySearchStop {
    Exhausted,
    PageFull,
    ScanLimit,
    ByteLimit,
    TimeLimit,
}

impl HistorySearchStop {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exhausted => "Search complete",
            Self::PageFull => "More history is available",
            Self::ScanLimit => "Scanned 50,000 commits; continue searching",
            Self::ByteLimit => "Read 64 MiB of history; continue searching",
            Self::TimeLimit => "Search paused at its time limit; continue searching",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HistorySearchPage {
    pub scope: HistoryScope,
    pub commits: Vec<Commit>,
    /// Number of commits inspected in this request, including nonmatches.
    pub scanned: usize,
    /// Resume the same query/scope here. `None` alone means exhaustive.
    pub next_offset: Option<usize>,
    pub stop: HistorySearchStop,
}

#[derive(Clone, Debug)]
pub struct FileHistoryEntry {
    pub commit: Commit,
    /// Exact revision paths and blob IDs, including the absent deletion side.
    pub change: FileChange,
    /// This history follows the first-parent lineage through merges. The
    /// commit retains all actual parent OIDs for other comparison choices.
    pub parent_oid: Option<String>,
}

#[derive(Clone, Debug)]
pub struct FileHistoryPage {
    pub anchor: String,
    pub path: PathBuf,
    pub entries: Vec<FileHistoryEntry>,
    pub next_offset: Option<usize>,
}

impl GitRepository {
    /// Capture local refs/HEAD, then create one cancellable ordinary-history
    /// stream. A FromCommit/PinnedRefs scope can also restore a captured tab's
    /// snapshot. Ref changes become visible only in a new AllRefs traversal.
    pub fn history_traversal(
        &self,
        scope: &HistoryScope,
        cancellation: &HistoryCancellation,
    ) -> Result<HistoryTraversal> {
        self.history_traversal_in_order(scope, HistoryOrder::Topological, cancellation)
    }

    /// Create a cancellable ordinary-history stream with an explicit initial
    /// ordering. When `order` is `Topological` and the repository has no
    /// commit-graph, waiting longer than 4 seconds for the first commit
    /// switches automatically to streaming chronological order.
    pub fn history_traversal_in_order(
        &self,
        scope: &HistoryScope,
        order: HistoryOrder,
        cancellation: &HistoryCancellation,
    ) -> Result<HistoryTraversal> {
        self.history_traversal_with_timeout(scope, order, TOPO_FALLBACK_TIMEOUT, cancellation)
    }

    fn history_traversal_with_timeout(
        &self,
        scope: &HistoryScope,
        order: HistoryOrder,
        topo_fallback_timeout: Duration,
        cancellation: &HistoryCancellation,
    ) -> Result<HistoryTraversal> {
        cancellation.check()?;
        let scope = match scope {
            HistoryScope::AllRefs => self.pin_history_refs(cancellation)?,
            other => other.clone(),
        };
        let (command, input) = build_history_log_command(self.target(), &scope, order, 0)?;
        let empty = matches!(&scope, HistoryScope::PinnedRefs(refs) if refs.is_empty());
        let fallback = (!empty && order == HistoryOrder::Topological && !self.has_commit_graph())
            .then(|| ChronologicalFallback {
                repo: self.clone(),
                skip: 0,
                timeout: topo_fallback_timeout,
            });
        Ok(HistoryTraversal {
            scope,
            order,
            offset: 0,
            stream: if empty {
                None
            } else {
                Some(HistoryStream::start(command, input)?)
            },
            deferred: None,
            fallback,
            failed: false,
        })
    }

    /// Create a stateful streaming history search cursor starting at offset 0.
    /// Continue pages resume directly from the live `git log` stream without
    /// `--skip` prefix replay.
    pub fn history_search(
        &self,
        scope: &HistoryScope,
        query: &str,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearch> {
        self.history_search_in_order(scope, query, 0, HistoryOrder::Topological, cancellation)
    }

    /// Create a stateful streaming history search cursor starting at `offset`
    /// (passing `--skip={offset}` only once to resume a detached search).
    pub fn history_search_from_offset(
        &self,
        scope: &HistoryScope,
        query: &str,
        offset: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearch> {
        self.history_search_in_order(
            scope,
            query,
            offset,
            HistoryOrder::Topological,
            cancellation,
        )
    }

    /// Create a stateful streaming history search cursor with explicit initial
    /// ordering.
    pub fn history_search_in_order(
        &self,
        scope: &HistoryScope,
        query: &str,
        offset: usize,
        order: HistoryOrder,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearch> {
        self.history_search_filtered_in_order(
            scope,
            query,
            &HistorySearchFilter::default(),
            offset,
            order,
            cancellation,
        )
    }

    /// Create a stateful streaming history search cursor with structured
    /// filters (diff content `-S`/`-G`, author, date range, path, regex,
    /// first-parent, no-merges).
    pub fn history_search_filtered(
        &self,
        scope: &HistoryScope,
        query: &str,
        filter: &HistorySearchFilter,
        offset: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearch> {
        self.history_search_filtered_in_order(
            scope,
            query,
            filter,
            offset,
            HistoryOrder::Topological,
            cancellation,
        )
    }

    /// Create a stateful streaming history search cursor with structured
    /// filters and explicit initial ordering.
    pub fn history_search_filtered_in_order(
        &self,
        scope: &HistoryScope,
        query: &str,
        filter: &HistorySearchFilter,
        offset: usize,
        order: HistoryOrder,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearch> {
        ensure!(query.len() <= 4096, "History query exceeds 4 KiB");
        ensure!(
            !query.contains('\0'),
            "History query must not contain NUL bytes"
        );
        filter.validate()?;
        cancellation.check()?;
        let scope = match scope {
            HistoryScope::AllRefs => {
                ensure!(
                    offset == 0,
                    "Continue all-ref search with the pinned scope returned by its first page"
                );
                self.pin_history_refs(cancellation)?
            }
            other => other.clone(),
        };
        let (command, input) = build_history_log_command_filtered(
            self.target(),
            &scope,
            order,
            offset,
            query,
            filter,
        )?;
        let empty = matches!(&scope, HistoryScope::PinnedRefs(refs) if refs.is_empty());
        let fallback = (offset == 0
            && !empty
            && order == HistoryOrder::Topological
            && !self.has_commit_graph())
        .then(|| ChronologicalFallback {
            repo: self.clone(),
            skip: 0,
            timeout: TOPO_FALLBACK_TIMEOUT,
        });
        Ok(HistorySearch {
            scope,
            query: query.to_owned(),
            filter: filter.clone(),
            matcher: if filter.regex {
                CaseInsensitiveMatcher::new("")
            } else {
                CaseInsensitiveMatcher::new(query)
            },
            order,
            offset,
            stream: if empty {
                None
            } else {
                Some(HistoryStream::start(command, input)?)
            },
            fallback,
            scratch: String::new(),
            failed: false,
        })
    }

    /// Structured history search combining query and [`HistorySearchFilter`].
    pub fn search_history_filtered(
        &self,
        scope: &HistoryScope,
        query: &str,
        filter: &HistorySearchFilter,
        offset: usize,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearchPage> {
        ensure!(
            (1..=MAX_HISTORY_PAGE).contains(&limit),
            "History page must contain 1–500 results"
        );
        let mut cursor =
            self.history_search_filtered(scope, query, filter, offset, cancellation)?;
        cursor.next_page(limit, cancellation)
    }

    /// Literal, case-insensitive substring search of subject, description,
    /// author name and full hash. An empty query lists history. Offsets count
    /// scanned commits, not matches. Each call inspects at most 50,000 commits,
    /// 64 MiB and 15 seconds; a budget stop always returns a continuation.
    pub fn search_history(
        &self,
        scope: &HistoryScope,
        query: &str,
        offset: usize,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<HistorySearchPage> {
        ensure!(
            (1..=MAX_HISTORY_PAGE).contains(&limit),
            "History page must contain 1–500 results"
        );
        let mut cursor = self.history_search_from_offset(scope, query, offset, cancellation)?;
        cursor.next_page(limit, cancellation)
    }

    fn pin_history_refs(&self, cancellation: &HistoryCancellation) -> Result<HistoryScope> {
        let mut command = git_command(self.target());
        command.args([
            "rev-parse",
            "--revs-only",
            "--exclude=refs/stash",
            "--all",
            "HEAD",
        ]);
        let mut bytes = Vec::new();
        let end = stream_history(command, None, cancellation, GIT_TIMEOUT, |chunk| {
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= MAX_HISTORY_REF_BYTES,
                "History scope exceeds its reference snapshot limit"
            );
            bytes.extend_from_slice(chunk);
            Ok(true)
        })?;
        ensure!(
            end != ReadEnd::TimedOut,
            "Reference snapshot exceeded its 15 second time limit"
        );
        let mut refs = Vec::new();
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let oid = text(line);
            validate_oid(&oid)?;
            refs.push(oid);
        }
        refs.sort_unstable();
        refs.dedup();
        ensure!(
            refs.len() <= MAX_HISTORY_REFS,
            "History scope exceeds 16,384 reference tips"
        );
        Ok(HistoryScope::PinnedRefs(refs))
    }

    /// One file's rename-following first-parent history from an immutable
    /// commit. Merge rows compare against their real first parent; side-branch
    /// commits are represented by their merge. This explicit lineage avoids
    /// Git --follow's ambiguous path tracking across non-linear history.
    /// Paths are literal repository-relative bytes, including deleted paths.
    /// Paging replays the prefix to track skipped renames, within the same
    /// 32 MiB/15 second limits; deep histories can require an older anchor.
    pub fn file_history(
        &self,
        oid: &str,
        path: &Path,
        offset: usize,
        limit: usize,
        cancellation: &HistoryCancellation,
    ) -> Result<FileHistoryPage> {
        validate_oid(oid)?;
        ensure!(
            (1..=MAX_HISTORY_PAGE).contains(&limit),
            "File-history page must contain 1–500 results"
        );
        ensure!(
            !path.as_os_str().is_empty()
                && path.as_os_str().len() <= 16 * 1024
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
            "Expected a literal repository-relative file path"
        );
        cancellation.check()?;
        let prefix_count = offset
            .checked_add(limit)
            .and_then(|count| count.checked_add(1))
            .context("File-history offset overflow")?;
        let mut command = history_command(self.target());
        command
            .args([
                "--follow",
                "--first-parent",
                "--diff-merges=first-parent",
                "--raw",
                "--no-abbrev",
                "--root",
                "--find-renames=50%",
                "-l1000",
            ])
            // --skip suppresses a rename before --follow can update its path,
            // losing all revisions under the older name. Read the bounded
            // prefix and remove earlier rows only after following the lineage.
            .arg(format!("--max-count={prefix_count}"))
            .arg(oid)
            .arg("--")
            .arg(path);
        let mut bytes = Vec::new();
        let end = stream_history(command, None, cancellation, GIT_TIMEOUT, |chunk| {
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= MAX_FILE_HISTORY_BYTES,
                "File-history prefix exceeds its 32 MiB limit. Open an older revision as the history anchor, or request a smaller page"
            );
            bytes.extend_from_slice(chunk);
            Ok(true)
        })?;
        ensure!(
            end != ReadEnd::TimedOut,
            "File-history prefix exceeded its 15 second time limit. Retry, or open an older revision as the history anchor"
        );
        let mut entries = parse_file_history(&bytes, cancellation)?;
        entries.drain(..offset.min(entries.len()));
        let next_offset = if entries.len() > limit {
            entries.truncate(limit);
            Some(
                offset
                    .checked_add(limit)
                    .context("File-history offset overflow")?,
            )
        } else {
            None
        };
        Ok(FileHistoryPage {
            anchor: oid.to_owned(),
            path: path.to_owned(),
            entries,
            next_offset,
        })
    }
}

pub(super) fn history_command(target: GitTarget<'_>) -> Command {
    history_command_ordered(target, HistoryOrder::Topological)
}

pub(super) fn history_command_ordered(target: GitTarget<'_>, order: HistoryOrder) -> Command {
    let mut command = git_command(target);
    command
        .arg("--literal-pathspecs")
        .args(["-c", "log.follow=false", "log"]);
    if order == HistoryOrder::Topological {
        command.arg("--topo-order");
    }
    command.args([
        "--no-show-signature",
        "--no-decorate",
        "--no-notes",
        "--no-ext-diff",
        "--no-textconv",
        "--encoding=UTF-8",
        "-z",
        "--format=%H%x00%P%x00%an%x00%at%x00%s%x00%b",
    ]);
    command
}

fn build_history_log_command(
    target: GitTarget<'_>,
    scope: &HistoryScope,
    order: HistoryOrder,
    skip: usize,
) -> Result<(Command, Option<Vec<u8>>)> {
    build_history_log_command_filtered(
        target,
        scope,
        order,
        skip,
        "",
        &HistorySearchFilter::default(),
    )
}

fn build_history_log_command_filtered(
    target: GitTarget<'_>,
    scope: &HistoryScope,
    order: HistoryOrder,
    skip: usize,
    query: &str,
    filter: &HistorySearchFilter,
) -> Result<(Command, Option<Vec<u8>>)> {
    let mut command = history_command_ordered(target, order);
    if skip > 0 {
        command.arg(format!("--skip={skip}"));
    }
    if filter.first_parent {
        command.arg("--first-parent");
    }
    if filter.no_merges {
        command.arg("--no-merges");
    }
    if filter.regex {
        command.args(["--regexp-ignore-case", "--extended-regexp"]);
        if !query.is_empty() {
            command.arg(format!("--grep={query}"));
        }
    } else if filter.author.as_deref().is_some_and(|v| !v.is_empty()) {
        command.args(["--regexp-ignore-case", "--fixed-strings"]);
    }
    if let Some(author) = filter.author.as_deref().filter(|v| !v.is_empty()) {
        command.arg(format!("--author={author}"));
    }
    if let Some(since) = filter.since.as_deref().filter(|v| !v.is_empty()) {
        command.arg(format!("--since={since}"));
    }
    if let Some(until) = filter.until.as_deref().filter(|v| !v.is_empty()) {
        command.arg(format!("--until={until}"));
    }
    if let Some((mode, pattern)) = &filter.pickaxe
        && !pattern.is_empty()
    {
        // Always suppress patch output so `-S`/`-G` never injects diff text
        // into the 6-NUL-field stream parser.
        command.arg("--no-patch");
        match mode {
            PickaxeMode::String => {
                command.arg(format!("-S{pattern}"));
            }
            PickaxeMode::Regex => {
                command.arg(format!("-G{pattern}"));
            }
        }
    }
    let input = match scope {
        HistoryScope::PinnedRefs(refs) => {
            ensure!(
                refs.len() <= MAX_HISTORY_REFS,
                "History scope exceeds 16,384 reference tips"
            );
            let mut input = Vec::new();
            for oid in refs {
                validate_oid(oid)?;
                input.extend_from_slice(oid.as_bytes());
                input.push(b'\n');
            }
            command.arg("--stdin");
            Some(input)
        }
        HistoryScope::FromCommit(oid) => {
            validate_oid(oid)?;
            command.arg(oid);
            None
        }
        HistoryScope::AllRefs => unreachable!(),
    };
    command.arg("--");
    for path in &filter.paths {
        command.arg(path);
    }
    Ok((command, input))
}

pub(super) fn parse_commit_fields(fields: &[impl AsRef<[u8]>]) -> Result<Commit> {
    ensure!(fields.len() == 6, "Malformed commit metadata from Git");
    let oid = text(fields[0].as_ref());
    validate_oid(&oid)?;
    let parents = text(fields[1].as_ref())
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for parent in &parents {
        validate_oid(parent)?;
    }
    Ok(Commit {
        oid,
        parents,
        author: text(fields[2].as_ref()),
        timestamp: std::str::from_utf8(fields[3].as_ref())?
            .parse()
            .context("Invalid commit timestamp")?,
        subject: text(fields[4].as_ref()),
        body: text(fields[5].as_ref()).trim_end_matches('\n').to_owned(),
    })
}

fn parse_file_history(
    bytes: &[u8],
    cancellation: &HistoryCancellation,
) -> Result<Vec<FileHistoryEntry>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    ensure!(bytes.last() == Some(&0), "Incomplete file history from Git");
    let fields = bytes[..bytes.len() - 1]
        .split(|b| *b == 0)
        .collect::<Vec<_>>();
    let mut cursor = 0;
    let mut entries = Vec::new();
    while cursor < fields.len() {
        cancellation.check()?;
        ensure!(
            fields.len() - cursor >= 6,
            "Incomplete file-history metadata"
        );
        ensure!(
            fields[cursor..cursor + 6]
                .iter()
                .map(|f| f.len())
                .sum::<usize>()
                <= MAX_HISTORY_RECORD,
            "One commit's metadata exceeds the 2 MiB history limit"
        );
        let commit = parse_commit_fields(&fields[cursor..cursor + 6])?;
        cursor += 6;
        let mut changes = Vec::new();
        while cursor < fields.len() && fields[cursor].starts_with(b"\n:") {
            let header = &fields[cursor][1..];
            let renamed = header
                .rsplit(|b| *b == b' ')
                .next()
                .is_some_and(|status| status.starts_with(b"R"));
            let count = if renamed { 3 } else { 2 };
            ensure!(
                fields.len() - cursor >= count,
                "Incomplete file-history paths"
            );
            let mut raw = header.to_vec();
            raw.push(0);
            for field in &fields[cursor + 1..cursor + count] {
                raw.extend_from_slice(field);
                raw.push(0);
            }
            changes.extend(parse_changes(&raw)?);
            cursor += count;
        }
        ensure!(
            changes.len() == 1,
            "Git returned an ambiguous file-history comparison"
        );
        entries.push(FileHistoryEntry {
            parent_oid: commit.parents.first().cloned(),
            commit,
            change: changes.remove(0),
        });
    }
    Ok(entries)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReadEnd {
    Complete,
    Stopped,
    TimedOut,
}

struct StreamState {
    stdout: VecDeque<Vec<u8>>,
    stdout_done: bool,
    stderr: Option<Vec<u8>>,
    error: Option<std::io::Error>,
    writer_done: bool,
    exit_ready: bool,
    closed: bool,
}

/// Bounded event-driven pipe channel for history streaming. Backpressure caps
/// queued stdout chunks at [`READ_QUEUE`] without 5 ms polling or post-EOF spin.
struct StreamChannel {
    state: Mutex<StreamState>,
    readable: Condvar,
    writable: Condvar,
}

impl StreamChannel {
    fn new(has_writer: bool) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(StreamState {
                stdout: VecDeque::with_capacity(READ_QUEUE),
                stdout_done: false,
                stderr: None,
                error: None,
                writer_done: !has_writer,
                exit_ready: false,
                closed: false,
            }),
            readable: Condvar::new(),
            writable: Condvar::new(),
        })
    }

    fn push_stdout(&self, chunk: Vec<u8>) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while state.stdout.len() >= READ_QUEUE && !state.closed {
            state = self.writable.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        if state.closed {
            return false;
        }
        state.stdout.push_back(chunk);
        self.readable.notify_all();
        true
    }

    fn finish_stdout(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stdout_done = true;
        self.readable.notify_all();
    }

    fn finish_stderr(&self, bytes: Vec<u8>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.stderr = Some(bytes);
        self.readable.notify_all();
    }

    fn set_error(&self, error: std::io::Error) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.error.is_none() {
            state.error = Some(error);
        }
        self.readable.notify_all();
    }

    fn finish_writer(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.writer_done = true;
        self.readable.notify_all();
    }

    fn mark_exit(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.exit_ready = true;
        self.readable.notify_all();
    }

    fn wake_reader(&self) {
        let _guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.readable.notify_all();
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        self.writable.notify_all();
        self.readable.notify_all();
    }
}

enum RawRecordOutcome {
    Record(usize),
    Eof,
    TimedOut,
}

/// Reusable pipe reader. The worker consumes only as far as the requested page;
/// backpressure pauses Git naturally while the application browses that page.
struct HistoryStream {
    child: Child,
    pipes: PipeControl,
    channel: Arc<StreamChannel>,
    exit_watcher: Option<JoinHandle<()>>,
    readers: Vec<JoinHandle<()>>,
    writer: Option<JoinHandle<std::io::Result<()>>>,
    chunk: Vec<u8>,
    position: usize,
    record_buf: Vec<u8>,
    field_ranges: [Range<usize>; 6],
    field_count: usize,
    field_start: usize,
    record_bytes: usize,
    reaped: bool,
}

impl HistoryStream {
    fn start(mut command: Command, input: Option<Vec<u8>>) -> Result<Self> {
        isolate_process_group(&mut command);
        command.stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Unable to start Git history traversal")?;
        let pipes = ChildPipes::take(&mut child)?;
        let channel = StreamChannel::new(input.is_some());
        let writer = input.map(|bytes| {
            let mut stdin = pipes.input.expect("Requested Git input pipe");
            let done = Arc::clone(&channel);
            thread::spawn(move || {
                struct Done(Arc<StreamChannel>);
                impl Drop for Done {
                    fn drop(&mut self) {
                        self.0.finish_writer();
                    }
                }
                let _done = Done(done);
                stdin.write_all(&bytes)
            })
        });
        let mut stdout = pipes.output;
        let stderr = pipes.error.expect("Requested Git error pipe");
        let out_ch = Arc::clone(&channel);
        let output_reader = thread::spawn(move || {
            struct Done(Arc<StreamChannel>);
            impl Drop for Done {
                fn drop(&mut self) {
                    self.0.finish_stdout();
                }
            }
            let _done = Done(Arc::clone(&out_ch));
            let mut chunk = [0u8; 8192];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        if !out_ch.push_stdout(chunk[..n].to_vec()) {
                            break;
                        }
                    }
                    Err(error) => {
                        out_ch.set_error(error);
                        break;
                    }
                }
            }
        });
        let err_ch = Arc::clone(&channel);
        let error_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            match stderr.take(128 * 1024 + 1).read_to_end(&mut bytes) {
                Ok(_) => err_ch.finish_stderr(bytes),
                Err(error) => {
                    err_ch.set_error(error);
                    err_ch.finish_stderr(Vec::new());
                }
            }
        });
        Ok(Self {
            child,
            pipes: pipes.control,
            channel,
            exit_watcher: None,
            readers: vec![output_reader, error_reader],
            writer,
            chunk: Vec::new(),
            position: 0,
            record_buf: Vec::with_capacity(512),
            field_ranges: std::array::from_fn(|_| 0..0),
            field_count: 0,
            field_start: 0,
            record_bytes: 0,
            reaped: false,
        })
    }

    fn shrink_idle_buffer(&mut self) {
        if self.field_count == 6 && self.record_buf.capacity() > 64 * 1024 {
            self.record_buf.clear();
            self.record_buf.shrink_to(4096);
            self.field_count = 0;
            self.field_start = 0;
            self.record_bytes = 0;
        }
    }

    fn current_fields(&self) -> [&[u8]; 6] {
        [
            &self.record_buf[self.field_ranges[0].clone()],
            &self.record_buf[self.field_ranges[1].clone()],
            &self.record_buf[self.field_ranges[2].clone()],
            &self.record_buf[self.field_ranges[3].clone()],
            &self.record_buf[self.field_ranges[4].clone()],
            &self.record_buf[self.field_ranges[5].clone()],
        ]
    }

    fn next_raw_record(
        &mut self,
        cancellation: &HistoryCancellation,
        start: Instant,
        timeout: Duration,
    ) -> Result<RawRecordOutcome> {
        if self.field_count == 6 {
            self.record_buf.clear();
            self.field_count = 0;
            self.field_start = 0;
            self.record_bytes = 0;
        }
        let mut poll_exit = false;
        loop {
            cancellation.check()?;
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Ok(RawRecordOutcome::TimedOut);
            }
            while self.position < self.chunk.len() {
                let rest = &self.chunk[self.position..];
                if let Some(rel) = rest.iter().position(|&b| b == 0) {
                    let consumed = rel + 1;
                    self.record_bytes = self.record_bytes.saturating_add(consumed);
                    ensure!(
                        self.record_bytes <= MAX_HISTORY_RECORD,
                        "One commit's metadata exceeds the 2 MiB history limit"
                    );
                    self.record_buf.extend_from_slice(&rest[..rel]);
                    let end = self.record_buf.len();
                    self.field_ranges[self.field_count] = self.field_start..end;
                    self.field_count += 1;
                    self.field_start = end;
                    self.position += consumed;
                    if self.field_count == 6 {
                        return Ok(RawRecordOutcome::Record(self.record_bytes));
                    }
                } else {
                    self.record_bytes = self.record_bytes.saturating_add(rest.len());
                    ensure!(
                        self.record_bytes <= MAX_HISTORY_RECORD,
                        "One commit's metadata exceeds the 2 MiB history limit"
                    );
                    self.record_buf.extend_from_slice(rest);
                    self.position = self.chunk.len();
                }
            }

            let mut state = self.channel.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(chunk) = state.stdout.pop_front() {
                self.channel.writable.notify_one();
                drop(state);
                self.chunk = chunk;
                self.position = 0;
                continue;
            }
            if let Some(error) = state.error.take() {
                return Err(error.into());
            }
            let streams_done = state.stdout_done && state.stderr.is_some() && state.writer_done;
            if streams_done && (self.exit_watcher.is_none() || state.exit_ready || poll_exit) {
                let stderr = state.stderr.clone().unwrap_or_default();
                drop(state);
                ensure!(
                    stderr.len() <= 128 * 1024,
                    "Git history error output exceeded its limit"
                );
                if let Some(watcher) = self.exit_watcher.take() {
                    let _ = watcher.join();
                    poll_exit = true;
                }
                if let Some(status) = self.child.try_wait()? {
                    self.reaped = true;
                    if !status.success() {
                        return Err(history_failure(&stderr));
                    }
                    ensure!(
                        self.record_bytes == 0 && self.field_count == 0,
                        "Incomplete commit metadata from Git"
                    );
                    if let Some(writer) = self.writer.take() {
                        writer
                            .join()
                            .map_err(|_| anyhow::anyhow!("Git history input writer stopped"))??;
                    }
                    return Ok(RawRecordOutcome::Eof);
                }
                // Pipes closed just before the child finished exiting: spawn
                // the exit watcher lazily so normal exits pay zero extra thread.
                if !poll_exit {
                    let exit_ch = Arc::clone(&self.channel);
                    self.exit_watcher =
                        process_io::spawn_exit_watcher(self.child.id(), move || {
                            exit_ch.mark_exit()
                        });
                    if self.exit_watcher.is_some() {
                        continue;
                    }
                    poll_exit = true;
                }
                let state = self.channel.state.lock().unwrap_or_else(|e| e.into_inner());
                if cancellation.is_cancelled() {
                    continue;
                }
                let wait = remaining.min(Duration::from_millis(5));
                let _ = self
                    .channel
                    .readable
                    .wait_timeout(state, wait)
                    .unwrap_or_else(|e| e.into_inner());
                continue;
            }
            if cancellation.is_cancelled() {
                continue;
            }
            let _ = self
                .channel
                .readable
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Drop for HistoryStream {
    fn drop(&mut self) {
        self.channel.close();
        self.pipes.stop();
        if !self.reaped {
            terminate_process_group(&self.child);
            let _ = self.child.kill();
            if let Some(watcher) = self.exit_watcher.take() {
                let _ = watcher.join();
            }
            let _ = self.child.wait();
        } else if let Some(watcher) = self.exit_watcher.take() {
            let _ = watcher.join();
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

/// How many messages [`stream_history`]'s pipe readers queue ahead of it.
const READ_QUEUE: usize = 8;

/// A history read Git failed, "Git history read failed: …" with Git's
/// `stderr` as a terminal leaves it, typed where Git found no repository
/// ([`git_failure`]): every history, search, attribution and inspection
/// stream ends so, so a tab whose `.git` went away is told apart whatever
/// the message keeps of Git's words.
fn history_failure(stderr: &[u8]) -> anyhow::Error {
    git_failure(
        stderr,
        format!("Git history read failed: {}", reported(stderr).trim()),
    )
}

/// What [`stream_history`] ends with once its cancellation stopped it: [`ReadCancelled`],
/// unless Git had already failed. A failure already queued, Git's exit with its error output
/// or a failed pipe read, ends the read as that failure, as it would have a moment before the
/// cancel: the cancel once masked it, so a caller said "Cancelled" for a read Git had already
/// refused. It reads only what is already queued, so a cancel never waits on Git.
fn cancelled_end(
    channel: &StreamChannel,
    status: impl FnOnce() -> std::io::Result<Option<std::process::ExitStatus>>,
) -> anyhow::Error {
    let (error, stderr) = {
        let mut state = channel.state.lock().unwrap_or_else(|e| e.into_inner());
        (state.error.take(), state.stderr.take())
    };
    if let Some(error) = error {
        return error.into();
    }
    let Some(stderr) = stderr else {
        return CoreError::ReadCancelled.into();
    };
    match status() {
        Ok(Some(status)) if !status.success() => {
            if stderr.len() > 128 * 1024 {
                anyhow::anyhow!("Git history error output exceeded its limit")
            } else {
                history_failure(&stderr)
            }
        }
        _ => CoreError::ReadCancelled.into(),
    }
}

/// Drains both pipes through a bounded queue. Cancellation and the deadline
/// cover the process and descendants which retain its pipes after it exits.
pub(super) fn stream_history(
    mut command: Command,
    input: Option<Vec<u8>>,
    cancellation: &HistoryCancellation,
    timeout: Duration,
    mut consume: impl FnMut(&[u8]) -> Result<bool>,
) -> Result<ReadEnd> {
    cancellation.check()?;
    isolate_process_group(&mut command);
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Unable to start Git history read")?;
    let pipes = ChildPipes::take(&mut child)?;
    let channel = StreamChannel::new(input.is_some());
    let input_writer = input.map(|bytes| {
        let mut stdin = pipes.input.expect("Requested Git input pipe");
        let done = Arc::clone(&channel);
        thread::spawn(move || {
            struct Done(Arc<StreamChannel>);
            impl Drop for Done {
                fn drop(&mut self) {
                    self.0.finish_writer();
                }
            }
            let _done = Done(done);
            stdin.write_all(&bytes)
        })
    });
    let mut stdout = pipes.output;
    let stderr = pipes.error.expect("Requested Git error pipe");
    let out_ch = Arc::clone(&channel);
    let output_reader = thread::spawn(move || {
        struct Done(Arc<StreamChannel>);
        impl Drop for Done {
            fn drop(&mut self) {
                self.0.finish_stdout();
            }
        }
        let _done = Done(Arc::clone(&out_ch));
        let mut chunk = [0u8; 8192];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if !out_ch.push_stdout(chunk[..n].to_vec()) {
                        break;
                    }
                }
                Err(error) => {
                    out_ch.set_error(error);
                    break;
                }
            }
        }
    });
    let err_ch = Arc::clone(&channel);
    let error_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        match stderr.take(128 * 1024 + 1).read_to_end(&mut bytes) {
            Ok(_) => err_ch.finish_stderr(bytes),
            Err(error) => {
                err_ch.set_error(error);
                err_ch.finish_stderr(Vec::new());
            }
        }
    });
    let mut exit_watcher: Option<JoinHandle<()>> = None;
    let wake_ch = Arc::clone(&channel);
    let _cancel = cancellation.on_cancel(move || wake_ch.wake_reader());
    let start = Instant::now();
    let mut reaped = false;
    let result = (|| {
        let mut status = None;
        let mut poll_exit = false;
        loop {
            if cancellation.is_cancelled() {
                return Err(cancelled_end(&channel, || match status {
                    Some(status) => Ok(Some(status)),
                    None => {
                        let exit_ready = channel
                            .state
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .exit_ready;
                        // Only reap an already-exited child here; sending SIGKILL
                        // to a still-running child before `try_wait` would turn a
                        // clean cancellation into a signal-killed Git failure.
                        if exit_ready || process_io::child_has_exited(child.id()) {
                            terminate_process_group(&child);
                            if let Some(watcher) = exit_watcher.take() {
                                let _ = watcher.join();
                            }
                            let found = child.try_wait()?;
                            reaped |= found.is_some();
                            Ok(found)
                        } else {
                            Ok(None)
                        }
                    }
                }));
            }
            let remaining = timeout.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Ok(ReadEnd::TimedOut);
            }
            let mut state = channel.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(chunk) = state.stdout.pop_front() {
                channel.writable.notify_one();
                drop(state);
                if !consume(&chunk)? {
                    return Ok(ReadEnd::Stopped);
                }
                continue;
            }
            if let Some(error) = state.error.take() {
                return Err(error.into());
            }
            let streams_done = state.stdout_done && state.stderr.is_some() && state.writer_done;
            if streams_done
                && (status.is_some() || exit_watcher.is_none() || state.exit_ready || poll_exit)
            {
                let stderr_bytes = state.stderr.clone().unwrap_or_default();
                drop(state);
                ensure!(
                    stderr_bytes.len() <= 128 * 1024,
                    "Git history error output exceeded its limit"
                );
                if status.is_none() {
                    if let Some(watcher) = exit_watcher.take() {
                        let _ = watcher.join();
                        poll_exit = true;
                    }
                    status = child.try_wait()?;
                    if status.is_some() {
                        reaped = true;
                    }
                }
                if let Some(status) = status {
                    if !status.success() {
                        return Err(history_failure(&stderr_bytes));
                    }
                    return Ok(ReadEnd::Complete);
                }
                if !poll_exit {
                    let exit_ch = Arc::clone(&channel);
                    exit_watcher =
                        process_io::spawn_exit_watcher(child.id(), move || exit_ch.mark_exit());
                    if exit_watcher.is_some() {
                        continue;
                    }
                    poll_exit = true;
                }
                let state = channel.state.lock().unwrap_or_else(|e| e.into_inner());
                if cancellation.is_cancelled() {
                    continue;
                }
                let wait = remaining.min(Duration::from_millis(5));
                let _ = channel
                    .readable
                    .wait_timeout(state, wait)
                    .unwrap_or_else(|e| e.into_inner());
                continue;
            }
            if cancellation.is_cancelled() {
                continue;
            }
            let _ = channel
                .readable
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner());
        }
    })();
    drop(_cancel);
    // Always close the channel before joining: a bounded writer may otherwise
    // remain blocked when a page fills while Git is still producing output.
    channel.close();
    pipes.control.stop();
    if !reaped {
        terminate_process_group(&child);
        let _ = child.kill();
        if let Some(watcher) = exit_watcher.take() {
            let _ = watcher.join();
        }
        let _ = child.wait();
    } else if let Some(watcher) = exit_watcher.take() {
        let _ = watcher.join();
    }
    let _ = output_reader.join();
    let _ = error_reader.join();
    if let Some(writer) = input_writer {
        let written = writer
            .join()
            .map_err(|_| anyhow::anyhow!("Git history input writer stopped"))?;
        if matches!(result, Ok(ReadEnd::Complete)) {
            written.context("Send history reference snapshot")?;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn history_deadline_joins_detached_output_and_input_holders() {
        for input_held in [false, true] {
            let (fixture, command) = process_io::fixtures::DetachedPipeHolder::command(input_held);
            let started = Instant::now();
            let end = stream_history(
                command,
                input_held.then(|| vec![b'x'; 1024 * 1024]),
                &HistoryCancellation::default(),
                Duration::from_millis(300),
                |_| Ok(true),
            )
            .unwrap();
            fixture.wait_ready();
            assert_eq!(end, ReadEnd::TimedOut);
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }

    #[cfg(unix)]
    #[test]
    fn traversal_cancellation_joins_detached_output_and_input_holders() {
        for input_held in [false, true] {
            let (fixture, command) = process_io::fixtures::DetachedPipeHolder::command(input_held);
            let mut traversal = HistoryTraversal {
                scope: HistoryScope::PinnedRefs(Vec::new()),
                order: HistoryOrder::Topological,
                offset: 0,
                stream: Some(
                    HistoryStream::start(command, input_held.then(|| vec![b'x'; 1024 * 1024]))
                        .unwrap(),
                ),
                deferred: None,
                fallback: None,
                failed: false,
            };
            fixture.wait_ready();
            let cancel = HistoryCancellation::default();
            let cancelling = {
                let cancel = cancel.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(30));
                    cancel.cancel();
                })
            };
            let started = Instant::now();
            let error = traversal.next_page(10, &cancel).unwrap_err();
            assert!(error.to_string().contains("cancelled"));
            assert!(started.elapsed() < Duration::from_secs(2));
            assert!(traversal.stream.is_none());
            assert!(traversal.failed);
            cancelling.join().unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn ordinary_traversal_cancellation_reaps_an_active_process_and_pipe_holders() {
        let temp = tempfile::TempDir::new().unwrap();
        let marker = temp.path().join("survived");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "(sleep 0.3; touch \"$1\") & wait", "fixture"])
            .arg(&marker);
        let mut traversal = HistoryTraversal {
            scope: HistoryScope::PinnedRefs(Vec::new()),
            order: HistoryOrder::Topological,
            offset: 0,
            stream: Some(HistoryStream::start(command, None).unwrap()),
            deferred: None,
            fallback: None,
            failed: false,
        };
        let cancel = HistoryCancellation::default();
        let from_thread = cancel.clone();
        let cancelling = thread::spawn(move || {
            thread::sleep(Duration::from_millis(25));
            from_thread.cancel();
        });
        let started = Instant::now();
        assert!(
            traversal
                .next_page(500, &cancel)
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        cancelling.join().unwrap();
        assert!(traversal.stream.is_none());
        thread::sleep(Duration::from_millis(350));
        assert!(!marker.exists());
    }

    #[cfg(unix)]
    #[test]
    fn dropping_an_idle_history_page_closes_its_backpressured_producer() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "while :; do printf 'unparsed record'; done"]);
        let stream = HistoryStream::start(command, None).unwrap();
        thread::sleep(Duration::from_millis(25));
        let started = Instant::now();
        drop(stream);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_terminates_active_read_and_its_descendants() {
        let temp = tempfile::TempDir::new().unwrap();
        let marker = temp.path().join("survived");
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "(sleep 0.3; touch \"$1\") & echo ready; wait",
            "fixture",
        ]);
        command.arg(&marker);
        let cancel = HistoryCancellation::default();
        let cancel_on_output = cancel.clone();
        let started = Instant::now();
        let error = stream_history(command, None, &cancel, Duration::from_secs(5), |_| {
            cancel_on_output.cancel();
            Ok(true)
        })
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(400));
        assert!(!marker.exists(), "Cancelled Git descendant remained active");
    }

    #[cfg(unix)]
    #[test]
    fn deadline_includes_pipe_holders_after_the_parent_exits() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 10 & exit 0"]);
        let started = Instant::now();
        let end = stream_history(
            command,
            None,
            &HistoryCancellation::default(),
            Duration::from_millis(30),
            |_| Ok(true),
        )
        .unwrap();
        assert_eq!(end, ReadEnd::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    /// A read Git has already failed ends with Git's failure though the cancel comes before
    /// the read handles it: the cancel once masked the failure as [`ReadCancelled`]. Git has
    /// exited with its error while a descendant still holds its output, whose first bytes,
    /// a second later, bring the cancel.
    #[cfg(unix)]
    #[test]
    fn a_failure_git_already_reported_outlasts_a_later_cancel() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "( exec 2>&-; sleep 1; printf x ) & echo 'fatal: bad revision' >&2; exit 128",
        ]);
        let cancellation = HistoryCancellation::default();
        let error = stream_history(
            command,
            None,
            &cancellation,
            Duration::from_secs(20),
            |_| {
                cancellation.cancel();
                Ok(true)
            },
        )
        .unwrap_err();
        assert!(!is_read_cancelled(&error), "{error:#}");
        assert_eq!(
            error.to_string(),
            "Git history read failed: fatal: bad revision"
        );
    }

    /// A history read whose `.git` went away, where the path Git names holds
    /// a carriage return: a traversal's page, a stream, and a stream whose
    /// cancel came after Git failed each end as finding no repository, by
    /// the type their failure carries ([`CoreError::NotARepository`]), through any
    /// context, and each shows the message it showed before the type, Git's
    /// words as a terminal leaves them, with none of "not a git repository".
    /// Told apart by that message, History's read of a gone tab left it
    /// unmarked, showing "Could not read local history: … b/.git/worktrees/x".
    #[cfg(unix)]
    #[test]
    fn a_history_read_finding_no_repository_is_told_apart_whatever_its_message_keeps() {
        use std::os::unix::process::ExitStatusExt;
        let git = || {
            let mut command = Command::new("/bin/sh");
            command.args([
                "-c",
                r"printf 'fatal: not a git repository: /a\rb/.git/worktrees/x\n' >&2; exit 128",
            ]);
            command
        };
        let cancellation = HistoryCancellation::default();
        let mut traversal = HistoryTraversal {
            scope: HistoryScope::PinnedRefs(Vec::new()),
            order: HistoryOrder::Topological,
            offset: 0,
            stream: Some(HistoryStream::start(git(), None).unwrap()),
            deferred: None,
            fallback: None,
            failed: false,
        };
        let page = traversal.next_page(10, &cancellation).unwrap_err();
        let stream = stream_history(git(), None, &cancellation, Duration::from_secs(20), |_| {
            Ok(true)
        })
        .unwrap_err();
        let queued = StreamChannel::new(false);
        queued.finish_stderr(b"fatal: not a git repository: /a\rb/.git/worktrees/x\n".to_vec());
        let cancelled = cancelled_end(&queued, || {
            Ok(Some(std::process::ExitStatus::from_raw(128 << 8)))
        });
        let shown = "Git history read failed: b/.git/worktrees/x";
        for error in [page, stream, cancelled] {
            assert_eq!(error.to_string(), shown);
            assert_eq!(format!("{error:#}"), shown);
            assert!(
                matches!(
                    CoreError::from_anyhow(&error),
                    Some(CoreError::NotARepository { .. })
                ),
                "{error:#}"
            );
            assert!(finds_no_repository(&error), "{error:#}");
            let read = error.context("Read local history");
            assert_eq!(format!("{read:#}"), format!("Read local history: {shown}"));
            assert!(finds_no_repository(&read), "{read:#}");
        }
        // Git's other failures carry no type.
        let mut other = Command::new("/bin/sh");
        other.args(["-c", "echo 'fatal: bad revision' >&2; exit 128"]);
        let other = stream_history(other, None, &cancellation, Duration::from_secs(20), |_| {
            Ok(true)
        })
        .unwrap_err();
        assert!(
            !matches!(
                CoreError::from_anyhow(&other),
                Some(CoreError::NotARepository { .. })
            ),
            "{other:#}"
        );
        assert!(!finds_no_repository(&other), "{other:#}");
    }

    /// A read its cancellation stopped while Git still runs ends with [`CoreError::ReadCancelled`],
    /// which a caller finds through the context added on the way out, as the app does to
    /// say "Cancelled" rather than show a failure.
    #[cfg(unix)]
    #[test]
    fn a_cancelled_read_is_found_through_added_context() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf x; sleep 20"]);
        let cancellation = HistoryCancellation::default();
        let started = Instant::now();
        let error = stream_history(
            command,
            None,
            &cancellation,
            Duration::from_secs(20),
            |_| {
                cancellation.cancel();
                Ok(true)
            },
        )
        .context("Compare the captured revisions")
        .context("PR changes")
        .unwrap_err();
        assert!(is_read_cancelled(&error), "{error:#}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn a_full_page_stops_a_producer_without_blocking_on_the_bounded_queue() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "while :; do printf 'history record\\n'; done"]);
        let started = Instant::now();
        let end = stream_history(
            command,
            None,
            &HistoryCancellation::default(),
            Duration::from_secs(2),
            |_| Ok(false),
        )
        .unwrap();
        assert_eq!(end, ReadEnd::Stopped);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig {
            cases: 64,
            failure_persistence: None,
            rng_seed: proptest::test_runner::RngSeed::Fixed(0x5A17_C0DE_0002),
            ..proptest::prelude::ProptestConfig::default()
        })]

        #[test]
        fn proptest_commit_field_parser_never_panics(
            fields in proptest::collection::vec(
                proptest::collection::vec(proptest::prelude::any::<u8>(), 0..64),
                0..12,
            ),
        ) {
            let _ = parse_commit_fields(&fields);
        }
    }

    #[cfg(unix)]
    #[test]
    fn stalled_topological_first_commit_falls_back_to_chronological_without_commit_graph() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("repo");
        let repo = GitRepository::init(&root, "main").unwrap();
        for i in 0..3 {
            std::fs::write(root.join("f.txt"), format!("{i}\n")).unwrap();
            for args in [
                vec!["add", "."],
                vec![
                    "-c",
                    "user.name=Hist",
                    "-c",
                    "user.email=hist@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "commit",
                    "-q",
                    "-m",
                    &format!("commit {i}"),
                ],
            ] {
                let status = Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .args(&args)
                    .status()
                    .unwrap();
                assert!(status.success());
            }
        }
        assert!(!repo.has_commit_graph());
        let cancel = HistoryCancellation::default();
        let pinned = repo.pin_history_refs(&cancel).unwrap();
        let mut stall = Command::new("/bin/sh");
        stall.args(["-c", "sleep 10"]);
        let mut traversal = HistoryTraversal {
            scope: pinned,
            order: HistoryOrder::Topological,
            offset: 0,
            stream: Some(HistoryStream::start(stall, None).unwrap()),
            deferred: None,
            fallback: Some(ChronologicalFallback {
                repo,
                skip: 0,
                timeout: Duration::from_millis(80),
            }),
            failed: false,
        };
        let started = Instant::now();
        let page = traversal.next_page(10, &cancel).unwrap();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(page.order, HistoryOrder::Chronological);
        assert_eq!(traversal.order(), HistoryOrder::Chronological);
        assert_eq!(page.commits.len(), 3);
        assert_eq!(page.stop, HistoryTraversalStop::Exhausted);
    }

    #[test]
    fn streaming_history_search_pages_match_stateless_search_without_replay() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path().join("repo");
        let repo = GitRepository::init(&root, "main").unwrap();
        for i in 0..12 {
            std::fs::write(root.join("f.txt"), format!("{i}\n")).unwrap();
            let msg = if i % 3 == 0 {
                format!("Résumé match #{i}\n\nDetails for item {i}")
            } else {
                format!("Unrelated commit #{i}")
            };
            for args in [
                vec!["add", "."],
                vec![
                    "-c",
                    "user.name=Search",
                    "-c",
                    "user.email=search@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "commit",
                    "-q",
                    "-m",
                    &msg,
                ],
            ] {
                let status = Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .args(&args)
                    .status()
                    .unwrap();
                assert!(status.success());
            }
        }
        let cancel = HistoryCancellation::default();
        let baseline = repo
            .search_history(&HistoryScope::AllRefs, "RÉSUMÉ", 0, 20, &cancel)
            .unwrap();
        assert_eq!(baseline.commits.len(), 4);
        assert_eq!(baseline.scanned, 12);

        let mut cursor = repo
            .history_search(&HistoryScope::AllRefs, "RÉSUMÉ", &cancel)
            .unwrap();
        let mut streamed_oids = Vec::new();
        let mut total_scanned = 0;
        loop {
            let page = cursor.next_page(2, &cancel).unwrap();
            streamed_oids.extend(page.commits.into_iter().map(|c| c.oid));
            total_scanned += page.scanned;
            if page.stop == HistorySearchStop::Exhausted {
                assert!(page.next_offset.is_none());
                break;
            }
            assert_eq!(page.next_offset, Some(cursor.offset()));
        }
        assert_eq!(total_scanned, 12);
        assert_eq!(
            streamed_oids,
            baseline
                .commits
                .iter()
                .map(|c| c.oid.clone())
                .collect::<Vec<_>>()
        );
    }
}
