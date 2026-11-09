mod repository_identity;
#[cfg(test)]
mod snapshot_bench;
#[cfg(test)]
mod working_bench;
use repository_identity::RepositoryIdentity;

use crate::{graph, text::PatchPresentation};
use anyhow::{Result, anyhow, ensure};
use futures::channel::oneshot;
use gitturtle_core::{Branch, Commit, FileChange, GitRepository, TextPreview, Worktree};
use gitturtle_preview::{
    ImagePreview, MAX_INPUT_BYTES, detect_lfs_pointer, is_image_path, metadata,
};
use gpui_kit::{RenderImage, SharedString};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
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
    /// Each commit's local and remote branches, HEAD's first, then its tags.
    pub refs: HistoryReferences,
    pub elapsed: Duration,
    /// An explicit Open whose branch or worktree scope no longer resolves
    /// reads All history instead and names the vanished scope here once.
    pub vanished_scope: Option<VanishedScope>,
    /// Whether the repository may be shallow, which a quiet read finds out:
    /// deepening it outside the app gives commits new parents, and can bring
    /// new rows, while every reference and the page stay as they were. Only
    /// a quiet read's snapshot reads it, and one that cannot says `true`.
    pub shallow: bool,
}

/// Every commit's references as History shows them, prepared with the
/// snapshot on the worker: a row reads its commit's and shares their
/// strings, and a frame reads the most any commit has, so neither joins,
/// formats or counts anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryReferences {
    by_commit: HashMap<String, CommitReferences>,
    /// The most references any commit has, which narrow History's compact
    /// pill is sized for.
    widest: usize,
}

impl HistoryReferences {
    /// Each commit's `branches`, HEAD's first and the others in Git's order,
    /// then its `tags` in Git's order, by name. A tag names the commit it
    /// peels to; one of a tree, a blob or another tag names no row.
    fn new(branches: &[Branch], tags: &[gitturtle_core::Tag]) -> Self {
        let mut names: HashMap<&str, (Vec<&str>, Vec<&str>)> = HashMap::new();
        for branch in branches {
            let (branches, _) = names.entry(&branch.oid).or_default();
            if branch.current {
                branches.insert(0, &branch.name);
            } else {
                branches.push(&branch.name);
            }
        }
        for tag in tags.iter().filter(|tag| tag.target_kind == "commit") {
            names.entry(&tag.target_oid).or_default().1.push(&tag.name);
        }
        let by_commit: HashMap<_, _> = names
            .into_iter()
            .map(|(oid, (branches, tags))| {
                (oid.to_owned(), CommitReferences::new(&branches, &tags))
            })
            .collect();
        Self {
            widest: by_commit
                .values()
                .map(|refs| refs.names.len())
                .max()
                .unwrap_or(0),
            by_commit,
        }
    }

    /// The references of the commit `oid`, if it has any.
    pub fn get(&self, oid: &str) -> Option<&CommitReferences> {
        self.by_commit.get(oid)
    }

    /// The most references any commit has.
    pub fn widest(&self) -> usize {
        self.widest
    }

    /// Every commit's references.
    #[cfg(test)]
    pub fn commits(&self) -> impl Iterator<Item = &CommitReferences> {
        self.by_commit.values()
    }

    /// The bytes these references keep, for a retained tab's accounting.
    pub fn retained_bytes(&self) -> usize {
        self.by_commit
            .iter()
            .map(|(oid, refs)| oid.capacity() + refs.retained_bytes())
            .sum()
    }
}

/// One commit's references as History shows them, prepared with the
/// snapshot on the worker, so a row's badge, pill, tooltip and accessible
/// name share these strings rather than joining them per row and frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitReferences {
    /// Its branches, HEAD's first and the others in Git's order, then its
    /// tags.
    pub names: Vec<SharedString>,
    /// How many of `names`, from the first, are branches.
    pub branches: usize,
    /// "+N" for the names after the first, which the badge counts.
    pub more: Option<SharedString>,
    /// Every name on its own line, a tag's as "tag ‹name›": the badge's and
    /// the pill's tooltip.
    pub lines: SharedString,
    /// Every name, comma-separated, a tag's as "tag ‹name›", as the row's
    /// accessible name says them.
    pub listed: SharedString,
}

impl CommitReferences {
    fn new(branches: &[&str], tags: &[&str]) -> Self {
        let count = branches.len() + tags.len();
        // A tag is said as one, so it reads apart from a branch of its name.
        let said: Vec<String> = branches
            .iter()
            .map(|name| (*name).to_owned())
            .chain(tags.iter().map(|name| format!("tag {name}")))
            .collect();
        Self {
            more: (count > 1).then(|| format!("+{}", count - 1).into()),
            lines: said.join("\n").into(),
            listed: said.join(", ").into(),
            branches: branches.len(),
            names: branches
                .iter()
                .chain(tags)
                .map(|name| SharedString::from((*name).to_owned()))
                .collect(),
        }
    }

    /// Whether the first name, which the badge and the pill show, is a tag.
    pub fn leads_with_tag(&self) -> bool {
        self.branches == 0
    }

    /// The bytes these strings keep, for a retained tab's accounting.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.names.capacity() * std::mem::size_of::<SharedString>()
            + self.names.iter().map(|name| name.len()).sum::<usize>()
            + self.more.as_ref().map_or(0, |more| more.len())
            + self.lines.len()
            + self.listed.len()
    }
}

pub struct ImageSide {
    pub animation: Option<Arc<crate::gif_playback::Timeline>>,
    pub captured: Option<Arc<[u8]>>,
    /// Whether the captured bytes offer the open action, decided with them.
    pub system_preview: Option<crate::system_preview::Offer>,
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
        /// Which sides, old then new, are Git LFS pointers whose object the
        /// local store does not hold, as the reading found them: a pointer's
        /// text cannot say, since its object can be local and unreadable as
        /// text, and only a missing one is called missing or offered.
        lfs_missing: [bool; 2],
        /// Which sides, old then new, are Git LFS pointers whose object could
        /// not be used, and why: each says so in its own words and offers no
        /// download, which could not help.
        lfs_failed: [Option<gitturtle_core::LfsObjectFailure>; 2],
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
                lfs_missing: _,
                lfs_failed: _,
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

/// A branch or worktree scope that no longer resolves in the current local
/// snapshot. Explicit Open replaces it with All history; quiet refresh and an
/// unpinned search report it while keeping the displayed history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VanishedScope(pub Scope);

impl std::fmt::Display for VanishedScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Scope::Branch { name, remote } => write!(
                f,
                "{} branch '{name}' no longer exists in the local repository snapshot",
                if *remote { "Remote-tracking" } else { "Local" }
            ),
            Scope::Worktree { path } => write!(
                f,
                "Worktree '{}' is no longer registered in the local repository snapshot",
                path.display()
            ),
        }
    }
}

impl std::error::Error for VanishedScope {}

/// What a snapshot read does when its branch or worktree scope is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MissingScope {
    /// Fail the read; quiet refresh then retains the displayed history.
    Fail,
    /// Read All history and record the vanished scope on the snapshot.
    AllHistory,
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
        /// The work-tree root a tab's passive reopen (Refresh, Show latest,
        /// a scope chosen, a restored tab, a worktree's row) must find at
        /// `path` again, captured when the person opened the tab; `None`
        /// for an explicit open, which may land on the work tree enclosing
        /// `path` (`RepositorySession::open`).
        root: Option<PathBuf>,
        /// The Git directories the tab captured at `root` when the person
        /// opened it, which a passive reopen must find there again: a `.git`
        /// swapped for another repository's gitfile, or a `commondir`
        /// re-pointed, is another repository. `None` for an explicit open
        /// and a worktree's row.
        git_dir: Option<GitDirectories>,
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
    /// The working state of the shown tab's repository, read on the
    /// operation thread, as Changes' Refresh and a write's reload read it.
    /// A repository no longer at its path fails with [`RepositoryGone`]:
    /// checked from the filesystem before the read, starting no process
    /// (`GitRepository::verify_at_path`), and again from the read's own
    /// failure, since every command is confined to the repository and, its
    /// `.git` gone, fails as "not a git repository" instead of reading the
    /// work tree enclosing it.
    pub fn read(repo: &GitRepository) -> Result<Self> {
        ensure_found_at_path(repo)?;
        let state = Self::read_with_checkpoint(repo, || Ok(()))
            .map_err(|error| confined_failure(repo, error))?;
        state.confirmed(repo)
    }

    /// This state, unless a part of it failed because its repository went
    /// away while it was read: its `.git` gone after the status, a later
    /// part fails as "not a git repository", and the repository gone is the
    /// answer ([`RepositoryGone`]), not an "Operation state: …" banner
    /// beside the tab's mark. Checked from the filesystem, and only when a
    /// part failed.
    fn confirmed(self, repo: &GitRepository) -> Result<Self> {
        if self.operation.is_err() || self.profile.is_err() || self.remotes.is_err() {
            ensure_found_at_path(repo)?;
        }
        Ok(self)
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
        /// The scope that vanished, when that is why the read failed; the
        /// view reports one vanished scope once.
        vanished: Option<VanishedScope>,
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
    active: Option<Active>,
    /// Test-only: the reader thread holds a request it has not finished,
    /// answered or skipped ([`Worker::wait_idle`]).
    #[cfg(test)]
    running: bool,
    /// Test-only: the reader takes no request while a test holds it
    /// ([`Worker::hold`]).
    #[cfg(test)]
    held: bool,
}

impl Queue {
    /// Whether the reader waits rather than taking the pending request.
    fn waits(&self) -> bool {
        #[cfg(test)]
        let held = self.held;
        #[cfg(not(test))]
        let held = false;
        self.pending.is_none() || held
    }

    /// Supersedes the request the reader is running: its token is
    /// cancelled, so it stops at its next check and the Git processes it runs
    /// are terminated. A comparison preview's preparation inside a phase that
    /// checks nothing until it ends ([`Cancellation::unobserved`]) is only
    /// marked superseded and kept, with its token, so a request for the same
    /// preview can take it over ([`Worker::submit`]); a request that took it
    /// over earlier is no longer answered.
    fn supersede_active(&mut self) {
        match self.active.take() {
            Some(Active::Read(history)) => history.cancel(),
            Some(Active::Preview {
                key,
                preparation,
                history,
                ..
            }) => {
                if preparation.supersede() {
                    self.active = Some(Active::Preview {
                        key,
                        preparation,
                        history,
                        adopted: None,
                    });
                } else {
                    history.cancel();
                }
            }
            None => {}
        }
    }
}

/// The request the reader is running, which a newer request supersedes.
enum Active {
    /// Stops at its next check; the Git processes it runs are terminated.
    Read(gitturtle_core::HistoryCancellation),
    /// A comparison preview's preparation (`Job::Preview`), with its token.
    /// It stays here until the reader finishes it unless a newer request
    /// stops it, so that a request for the same preview can take it over
    /// inside a phase that checks nothing: its reply then goes to `adopted`.
    Preview {
        key: Box<PreviewKey>,
        preparation: Arc<Preparation>,
        history: gitturtle_core::HistoryCancellation,
        adopted: Option<oneshot::Sender<Result<Output>>>,
    },
}

/// Where a comparison preview's preparation stands. Its work checks the
/// request's cancellation as it goes, as any read's does, and a newer
/// request stops it at once, its token cancelled
/// ([`Queue::supersede_active`]), except inside a phase that checks nothing
/// until it ends, an image decoded in this process
/// ([`Cancellation::unobserved`]). There a newer request only marks it
/// superseded, and a request for the same preview made before the phase
/// ends takes it over ([`Worker::submit`]), so a file chosen again while its
/// image is decoded, even after another file was chosen, has its bytes
/// decoded once. A phase that ends superseded stops the preparation and
/// cancels its token.
#[derive(Default)]
struct Preparation(AtomicU8);

impl Preparation {
    /// Checking as it goes: a newer request stops it.
    const OBSERVING: u8 = 0;
    /// Inside a phase that checks nothing until it ends.
    const UNOBSERVED: u8 = 1;
    /// Superseded inside such a phase.
    const SUPERSEDED: u8 = 2;
    const STOPPED: u8 = 3;

    /// Moves to the state `next` gives for the current one, atomically, and
    /// returns the state it left.
    fn update(&self, next: impl Fn(u8) -> u8) -> u8 {
        let mut state = self.0.load(Ordering::Acquire);
        loop {
            match self.0.compare_exchange_weak(
                state,
                next(state),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(left) => return left,
                Err(actual) => state = actual,
            }
        }
    }

    /// A newer request for other work supersedes the preparation: true where
    /// it is only marked superseded, inside a phase that checks nothing, and
    /// false where it stops.
    fn supersede(&self) -> bool {
        let left = self.update(|state| match state {
            Self::UNOBSERVED | Self::SUPERSEDED => Self::SUPERSEDED,
            _ => Self::STOPPED,
        });
        matches!(left, Self::UNOBSERVED | Self::SUPERSEDED)
    }

    /// A request for the same preview takes the preparation over: true only
    /// inside a phase that checks nothing, superseded there or not.
    fn take_over(&self) -> bool {
        let left = self.update(|state| match state {
            Self::UNOBSERVED | Self::SUPERSEDED => Self::UNOBSERVED,
            other => other,
        });
        matches!(left, Self::UNOBSERVED | Self::SUPERSEDED)
    }

    /// Enters a phase that checks nothing: false once stopped.
    fn enter(&self) -> bool {
        let left = self.update(|state| match state {
            Self::OBSERVING => Self::UNOBSERVED,
            other => other,
        });
        left != Self::STOPPED
    }

    /// Leaves that phase: false where it ends superseded, which stops the
    /// preparation.
    fn leave(&self) -> bool {
        let left = self.update(|state| match state {
            Self::OBSERVING | Self::UNOBSERVED => Self::OBSERVING,
            _ => Self::STOPPED,
        });
        matches!(left, Self::OBSERVING | Self::UNOBSERVED)
    }

    /// The preparation's own check: whether it still answers a request. A
    /// check that finds it superseded stops it.
    fn check(&self) -> bool {
        let left = self.update(|state| match state {
            Self::SUPERSEDED => Self::STOPPED,
            other => other,
        });
        matches!(left, Self::OBSERVING | Self::UNOBSERVED)
    }
}

/// Keeps a [`Worker`]'s reader from taking requests until it drops
/// ([`Worker::hold`]).
#[cfg(test)]
#[must_use = "the reader is released as soon as the guard drops"]
pub struct ReaderHeld {
    queue: Arc<(Mutex<Queue>, Condvar)>,
}

#[cfg(test)]
impl Drop for ReaderHeld {
    fn drop(&mut self) {
        let (lock, ready) = &*self.queue;
        lock.lock().unwrap_or_else(|error| error.into_inner()).held = false;
        ready.notify_all();
    }
}

struct Cancellation {
    generation: u64,
    latest: Arc<AtomicU64>,
    history: gitturtle_core::HistoryCancellation,
    /// A comparison preview's preparation, which a newer request stops
    /// through its [`Preparation`] and its token rather than the generation,
    /// so that a request for the same preview can take over a phase that
    /// checks nothing ([`Self::unobserved`]).
    preparation: Option<Arc<Preparation>>,
}

impl Cancellation {
    fn check(&self) -> Result<()> {
        let current = match &self.preparation {
            Some(preparation) => {
                if !preparation.check() {
                    self.history.cancel();
                }
                !self.history.is_cancelled()
            }
            None => {
                self.latest.load(Ordering::Acquire) == self.generation
                    && !self.history.is_cancelled()
            }
        };
        ensure!(
            current,
            "Repository request superseded by a newer selection"
        );
        Ok(())
    }

    /// Runs `work`, which checks nothing until it ends, such as an image
    /// decoded in this process. Meanwhile a newer request only marks a
    /// comparison preview's preparation superseded ([`Preparation`]), so a
    /// request for the same preview can still take it over; one still
    /// superseded as `work` ends stops then, its token cancelled, and this
    /// answers that. Work that checks as it goes runs outside this, where a
    /// newer request stops it at once.
    fn unobserved<T>(&self, work: impl FnOnce() -> T) -> Result<T> {
        let Some(preparation) = &self.preparation else {
            return Ok(work());
        };
        ensure!(
            preparation.enter(),
            "Repository request superseded by a newer selection"
        );
        let done = work();
        if !preparation.leave() {
            self.history.cancel();
            return Err(anyhow!(
                "Repository request superseded by a newer selection"
            ));
        }
        Ok(done)
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
    /// Test-only: the Working file previews among them
    /// ([`Self::working_previews`]).
    #[cfg(test)]
    working_previews: AtomicU64,
    /// Test-only: the comparison previews among them (`Job::Preview`)
    /// ([`Self::previews`]).
    #[cfg(test)]
    previews: AtomicU64,
    /// Test-only: Quick Open's source reads among them
    /// (`Job::TrackedPreview`) ([`Self::tracked_previews`]).
    #[cfg(test)]
    tracked_previews: AtomicU64,
    /// Test-only: the comparison previews the reader started preparing
    /// ([`Self::preparations`]).
    #[cfg(test)]
    preparations: Arc<AtomicU64>,
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
                #[cfg(test)]
                running: false,
                #[cfg(test)]
                held: false,
            }),
            Condvar::new(),
        ));
        let latest = Arc::new(AtomicU64::new(0));
        let worker_queue = Arc::clone(&queue);
        let worker_latest = Arc::clone(&latest);
        #[cfg(test)]
        let preparations = Arc::new(AtomicU64::new(0));
        #[cfg(test)]
        let worker_preparations = Arc::clone(&preparations);
        let spawn = std::thread::Builder::new()
            .name("gitturtle-reader".into())
            .spawn(move || {
                let mut cache = PreviewCache::default();
                loop {
                    let (request, preparation) = {
                        let (lock, ready) = &*worker_queue;
                        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                        // The request taken last has answered or was skipped.
                        #[cfg(test)]
                        {
                            state.running = false;
                            ready.notify_all();
                        }
                        while state.waits() && !state.closed {
                            state = ready.wait(state).unwrap_or_else(|error| error.into_inner());
                        }
                        if state.closed {
                            break;
                        }
                        let request = state.pending.take().expect("pending request checked above");
                        if request.reply.is_canceled()
                            && !matches!(request.job, Job::ReleaseHistory)
                        {
                            continue;
                        }
                        let preparation = match PreviewKey::of(&request.job) {
                            Some(key) => {
                                let preparation = Arc::new(Preparation::default());
                                state.active = Some(Active::Preview {
                                    key: Box::new(key),
                                    preparation: Arc::clone(&preparation),
                                    history: request.cancellation.clone(),
                                    adopted: None,
                                });
                                #[cfg(test)]
                                worker_preparations.fetch_add(1, Ordering::AcqRel);
                                Some(preparation)
                            }
                            None => {
                                state.active = Some(Active::Read(request.cancellation.clone()));
                                None
                            }
                        };
                        #[cfg(test)]
                        {
                            state.running = true;
                        }
                        (request, preparation)
                    };
                    let previewed = preparation.is_some();
                    let cancellation = Cancellation {
                        generation: request.generation,
                        latest: Arc::clone(&worker_latest),
                        history: request.cancellation,
                        preparation,
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
                    // A preview's preparation answers the request that took
                    // it over, if one did, in place of the one that started it.
                    let reply = if previewed {
                        let mut state = worker_queue
                            .0
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        match state.active.take() {
                            Some(Active::Preview {
                                adopted: Some(adopted),
                                ..
                            }) => adopted,
                            _ => request.reply,
                        }
                    } else {
                        request.reply
                    };
                    let _ = reply.send(result);
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
            #[cfg(test)]
            working_previews: AtomicU64::new(0),
            #[cfg(test)]
            previews: AtomicU64::new(0),
            #[cfg(test)]
            tracked_previews: AtomicU64::new(0),
            #[cfg(test)]
            preparations,
        }
    }

    /// Test-only: the jobs submitted so far, including any a closed worker
    /// refused.
    #[cfg(test)]
    pub fn submissions(&self) -> u64 {
        self.submissions.load(Ordering::Acquire)
    }

    /// Test-only: the Working file previews (`Job::WorkingPreview`)
    /// submitted so far, so a test can assert that a path reads none while
    /// other reads run around it.
    #[cfg(test)]
    pub fn working_previews(&self) -> u64 {
        self.working_previews.load(Ordering::Acquire)
    }

    /// Test-only: the comparison previews (`Job::Preview`) submitted so
    /// far, a restored comparison's included.
    #[cfg(test)]
    pub fn previews(&self) -> u64 {
        self.previews.load(Ordering::Acquire)
    }

    /// Test-only: Quick Open's source reads (`Job::TrackedPreview`)
    /// submitted so far.
    #[cfg(test)]
    pub fn tracked_previews(&self) -> u64 {
        self.tracked_previews.load(Ordering::Acquire)
    }

    /// Test-only: the comparison previews (`Job::Preview`) the reader has
    /// started preparing so far: requests it skipped, or that took over a
    /// preparation under way, start none.
    #[cfg(test)]
    pub fn preparations(&self) -> u64 {
        self.preparations.load(Ordering::Acquire)
    }

    /// Test-only: keeps the reader from taking a request until the
    /// returned guard drops, as a slow read ahead of it would, so a request
    /// submitted meanwhile stays pending, where a newer one or
    /// [`Self::cancel`] drops it unanswered.
    #[cfg(test)]
    pub fn hold(&self) -> ReaderHeld {
        self.queue
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .held = true;
        ReaderHeld {
            queue: Arc::clone(&self.queue),
        }
    }

    /// Test-only: blocks until the reader thread has answered or skipped
    /// every request submitted so far, holding the test clock: it waits on
    /// the reader's own thread, which `run_until_parked` cannot wait for,
    /// and a GPUI test then parks to handle the reply on its own thread. A
    /// read that has not answered within `operations::test_wait` fails the
    /// test at the caller's line.
    #[cfg(test)]
    #[track_caller]
    pub fn wait_idle(&self) {
        self.wait_idle_within(crate::operations::test_wait());
    }

    /// [`wait_idle`](Self::wait_idle), failing after `limit`.
    #[cfg(test)]
    #[track_caller]
    pub fn wait_idle_within(&self, limit: Duration) {
        let (lock, ready) = &*self.queue;
        let state = lock.lock().unwrap_or_else(|error| error.into_inner());
        let (state, waited) = ready
            .wait_timeout_while(state, limit, |state| {
                (state.pending.is_some() || state.running) && !state.closed
            })
            .unwrap_or_else(|error| error.into_inner());
        drop(state);
        assert!(
            !waited.timed_out(),
            "the repository reader did not answer or skip its requests within {limit:?}: a read never answered or is still held"
        );
    }

    /// Invalidate mutable reads even when there is no replacement file to load.
    /// Active work stops at its next checkpoint; queued work is dropped now.
    pub fn cancel(&self) {
        let (lock, _) = &*self.queue;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        self.latest.fetch_add(1, Ordering::AcqRel);
        state.supersede_active();
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
        #[cfg(test)]
        if matches!(job, Job::WorkingPreview { .. }) {
            self.working_previews.fetch_add(1, Ordering::AcqRel);
        }
        #[cfg(test)]
        if matches!(job, Job::Preview { .. }) {
            self.previews.fetch_add(1, Ordering::AcqRel);
        }
        #[cfg(test)]
        if matches!(job, Job::TrackedPreview { .. }) {
            self.tracked_previews.fetch_add(1, Ordering::AcqRel);
        }
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
        let queue = &mut *state;
        // The same comparison preview, still decoding its image, whatever
        // was asked for since: that preparation answers this request, so a
        // file chosen again has its bytes decoded once. Nothing waits behind
        // it any more.
        if let Some(Active::Preview {
            key,
            preparation,
            adopted,
            ..
        }) = &mut queue.active
            && PreviewKey::of(&job).is_some_and(|wanted| wanted == **key)
            && preparation.take_over()
        {
            *adopted = Some(reply);
            queue.pending = None;
            ready.notify_all();
            return receiver;
        }
        queue.supersede_active();
        state.pending = Some(Request {
            job,
            generation,
            reply,
            cancellation: gitturtle_core::HistoryCancellation::default(),
        });
        // All: a test waiting for the reader to settle (`wait_idle`) waits
        // on this condition too, and must not take the reader's wake-up.
        ready.notify_all();
        receiver
    }
}

/// A tab's repository that a passive read no longer finds at its path: Git
/// finds another repository there, the work tree enclosing it once its
/// `.git` is gone (deleted, moved, a failed clone cleaned up), or none.
/// The tab stays on the repository the person opened, marked unavailable
/// with this reason (`GitTurtle::tab_repository_gone`), and never shows
/// the other one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryGone {
    /// The tab's repository, its work-tree root.
    pub path: PathBuf,
    /// The repository Git finds there now, by its root, if any.
    pub found: Option<PathBuf>,
}

impl std::fmt::Display for RepositoryGone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "The repository at {} is gone. ", self.path.display())?;
        match &self.found {
            Some(found) => write!(
                f,
                "Git now finds the repository at {} there, which this tab does not open.",
                found.display()
            )?,
            None => write!(f, "Git finds no repository there.")?,
        }
        write!(
            f,
            " Restore its .git and refresh, or choose a repository with Open Repository."
        )
    }
}

impl std::error::Error for RepositoryGone {}

/// The Git directories a tab captured at its root as its snapshot was
/// accepted, which a passive reopen of that root must find there again
/// ([`RepositorySession::open`]): the Git directory Git reported and its
/// common directory ([`GitRepository::git_dir`],
/// [`GitRepository::common_dir`]). A `.git` swapped for another
/// repository's gitfile changes the first; a linked worktree's `commondir`
/// re-pointed at another repository keeps the root and the first, yet its
/// refs and objects, and every write's, are that other repository's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitDirectories {
    pub git_dir: PathBuf,
    pub common: PathBuf,
    /// Their inodes as captured at the repository's open, which a session
    /// saves beside these paths.
    pub inodes: DirectoryInodes,
    /// Whether these came from a saved session. Only such a tab compares
    /// [`Self::inodes`] with the directories it finds: a tab opened in this
    /// run, or one whose reopen was accepted, holds [`Self::of`]'s.
    pub restored: bool,
}

/// The inodes of a tab's [`GitDirectories`]. `None` where a session saved
/// none, as every older session, and where the platform has none: there is
/// then nothing to compare.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirectoryInodes {
    pub git_dir: Option<u64>,
    pub common: Option<u64>,
}

impl GitDirectories {
    pub fn of(repository: &GitRepository) -> Self {
        Self {
            git_dir: repository.git_dir().to_owned(),
            common: repository.common_dir().to_owned(),
            inodes: DirectoryInodes {
                git_dir: repository.git_dir_inode(),
                common: repository.common_dir_inode(),
            },
            restored: false,
        }
    }

    /// The directory of `repository` that differs from these, by its path
    /// or, for a restored tab, by the inode it saved, the Git directory
    /// before the common one; `None` where both match. Reads what the
    /// repository's open captured, so it touches no filesystem.
    fn mismatch(&self, repository: &GitRepository) -> Option<PathBuf> {
        let differs = |saved: Option<u64>, found: Option<u64>| {
            self.restored && saved.is_some_and(|saved| found != Some(saved))
        };
        if repository.git_dir() != self.git_dir
            || differs(self.inodes.git_dir, repository.git_dir_inode())
        {
            Some(repository.git_dir().to_owned())
        } else if repository.common_dir() != self.common
            || differs(self.inodes.common, repository.common_dir_inode())
        {
            Some(repository.common_dir().to_owned())
        } else {
            None
        }
    }
}

/// The folders above a tab's captured root as they resolve now, where they
/// resolve to other folders than the ones captured: a root saved through a
/// linked folder (macOS reaches every temporary folder through `/var`, a
/// link to `/private/var`; a person's projects folder can be a link in
/// their home), or one whose folder was moved and linked back from its old
/// place. A passive reopen compares the tab's root and Git directories
/// through them, on the worker as it reads the repository, never at launch;
/// the root's own name and everything below those folders are compared as
/// captured, so a root turned into a link to another repository, a `.git`
/// swapped for another repository's, or a re-pointed `commondir` is still
/// another repository ([`RepositorySession::open`]). Compared as captured,
/// every such root was another repository, its tab marked " ·
/// unavailable" where it opened before.
struct FoldersAbove {
    captured: PathBuf,
    now: PathBuf,
}

impl FoldersAbove {
    /// `None` where `root` is reached as captured, as every root Git
    /// reported is (it is `canonical`, the folder the reopen asked for
    /// resolved), and where its folders cannot be resolved.
    fn of(root: &Path, canonical: &Path) -> Option<Self> {
        if root == canonical {
            return None;
        }
        root.file_name()?;
        let captured = root.parent()?;
        let now = captured.canonicalize().ok()?;
        (now != captured).then(|| Self {
            captured: captured.to_owned(),
            now,
        })
    }

    /// `path` reached through these folders as they resolve now; as
    /// captured where it lies outside them, as a linked worktree's Git
    /// directories in another folder do.
    fn through(&self, path: &Path) -> PathBuf {
        match path.strip_prefix(&self.captured) {
            Ok(rest) if rest.as_os_str().is_empty() => self.now.clone(),
            Ok(rest) => self.now.join(rest),
            Err(_) => path.to_owned(),
        }
    }
}

/// Fails with [`RepositoryGone`] where `repo` is no longer the repository
/// at its path: its `.git` gone, swapped for another repository's, or its
/// root turned into a link to another folder. From the filesystem alone
/// while it holds; Git is asked only once it does not, to name what it
/// finds, or for a repository whose commands cannot be confined.
fn ensure_found_at_path(repo: &GitRepository) -> Result<()> {
    let found = match repo.verify_at_path()? {
        gitturtle_core::FoundAtPath::Same => return Ok(()),
        gitturtle_core::FoundAtPath::Other(found) => Some(found),
        gitturtle_core::FoundAtPath::None => None,
    };
    Err(RepositoryGone {
        path: repo.path().to_owned(),
        found,
    }
    .into())
}

/// An open of `requested` that core refused, as the reader reports it: a
/// repository that changed while it was opened is [`RepositoryGone`], and so
/// is a tab's root whose work tree now lies outside its folder, where an
/// explicit open of such a layout says so in its own words. One that Git
/// still reports at its path, only the capture having failed, stays
/// [`gitturtle_core::ChangedWhileOpening`]: it once read "is gone. Git now
/// finds the repository at P there", the one path twice, of a healthy
/// repository.
fn refused_open(requested: &Path, passive: bool, error: anyhow::Error) -> anyhow::Error {
    let error = match error.downcast::<gitturtle_core::ChangedWhileOpening>() {
        Ok(changed) if changed.found.as_ref() == Some(&changed.path) => return changed.into(),
        Ok(changed) => {
            return RepositoryGone {
                path: requested.to_owned(),
                found: changed.found,
            }
            .into();
        }
        Err(error) => error,
    };
    match error.downcast::<gitturtle_core::WorkTreeOutside>() {
        Ok(outside) if passive => RepositoryGone {
            path: requested.to_owned(),
            found: Some(outside.work_tree),
        }
        .into(),
        Ok(outside) => outside.into(),
        Err(error) => error,
    }
}

/// What a failed open says: `repository_access::explain`'s reasons, but for
/// a refusal that says what happened in its own words.
fn worded_open_failure(path: &Path, error: anyhow::Error) -> anyhow::Error {
    if error.is::<RepositoryGone>()
        || error.is::<gitturtle_core::WorkTreeOutside>()
        || error.is::<gitturtle_core::ChangedWhileOpening>()
    {
        error
    } else {
        crate::repository_access::explain(path, error)
    }
}

/// A read of `repo` that failed as "not a git repository", as
/// [`RepositoryGone`]: its commands are confined to it, so that is how a
/// read fails once its `.git` went away, and the diagnosis names what Git
/// finds there now. Any other failure stays its own, with no process.
pub(crate) fn confined_failure(repo: &GitRepository, error: anyhow::Error) -> anyhow::Error {
    if error.is::<RepositoryGone>() || !gitturtle_core::finds_no_repository(&error) {
        return error;
    }
    let found = match repo.found_at_path() {
        Ok(gitturtle_core::FoundAtPath::Other(found)) => Some(found),
        Ok(gitturtle_core::FoundAtPath::None) => None,
        Ok(gitturtle_core::FoundAtPath::Same) | Err(_) => return error,
    };
    RepositoryGone {
        path: repo.path().to_owned(),
        found,
    }
    .into()
}

/// Any failure of a read of `repo` as [`RepositoryGone`] where `repo` is
/// gone or no longer the repository at its path, for reads whose steps can
/// fail in other words as its `.git` moves while they run, as the watcher's
/// setup resolving the directories Git reported. A "not a git repository"
/// failure is Git's to decide ([`confined_failure`]): a `.git` half removed
/// (its HEAD deleted, the folder and `config` left) still holds for the
/// filesystem check, which kept the failure its own and the tab unmarked.
/// Any other failure is checked from the filesystem first
/// ([`GitRepository::verify_at_path`]). A repository still there keeps the
/// failure its own. Unlike [`confined_failure`], it checks whatever the
/// failure, so it is for reads that rarely fail, never a hot path.
pub(crate) fn gone_unless_found(repo: &GitRepository, error: anyhow::Error) -> anyhow::Error {
    if error.is::<RepositoryGone>() {
        return error;
    }
    if gitturtle_core::finds_no_repository(&error) {
        return confined_failure(repo, error);
    }
    let found = match repo.verify_at_path() {
        Ok(gitturtle_core::FoundAtPath::Other(found)) => Some(found),
        Ok(gitturtle_core::FoundAtPath::None) => None,
        Ok(gitturtle_core::FoundAtPath::Same) | Err(_) => return error,
    };
    RepositoryGone {
        path: repo.path().to_owned(),
        found,
    }
    .into()
}

/// A quiet read's failed reopen of `repo`, as [`RepositoryGone`] with
/// nothing found where Git now finds no repository at its path, so the tab
/// is read as Refresh reads it, whose Open says why
/// (`repository_access::explain`). Any other failure, as a deadline or an
/// access error, stays its own. Git is asked only once the reopen failed,
/// and not at all when the reopen already says the repository is gone.
fn nothing_found_is_gone(repo: &GitRepository, error: anyhow::Error) -> anyhow::Error {
    if error.is::<RepositoryGone>() {
        return error;
    }
    match repo.found_at_path() {
        Ok(gitturtle_core::FoundAtPath::None) => RepositoryGone {
            path: repo.path().to_owned(),
            found: None,
        }
        .into(),
        _ => error,
    }
}

#[cfg(test)]
thread_local! {
    /// Runs at each checkpoint of a quiet read's working read on this
    /// thread: a test moves a repository's `.git` part way through.
    static QUIET_CHECKPOINT: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn quiet_checkpoint() {
    QUIET_CHECKPOINT.with_borrow_mut(|hook| {
        if let Some(hook) = hook {
            hook();
        }
    });
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
    /// The repository at `requested`. An explicit open (`captured` `None`)
    /// takes what Git discovery finds, the work tree enclosing a folder
    /// inside one included. A tab's passive reopen names the root `captured`
    /// when the person opened it, and finds that root or nothing: where Git
    /// now finds another repository at `requested`, its `.git` gone and a
    /// work tree enclosing it, the open fails with [`RepositoryGone`] rather
    /// than move the tab to another repository; where it finds none, it
    /// fails as any open of a folder that is not a repository does.
    ///
    /// The captured root is compared as it was captured, never resolved
    /// again, but for the folders above it ([`FoldersAbove`]): a root since
    /// turned into a link to another repository resolves elsewhere and is
    /// that other repository, gone from the tab, while one reached through
    /// a linked folder above it, as macOS reaches every temporary folder
    /// through `/var`, is the repository it was. At the same root, the Git
    /// directories found must be `git_dir`, the ones the tab captured
    /// there, where it names them (through the same folders): not another
    /// repository's that a swapped `.git` leads to, nor another
    /// repository's refs and objects that a linked worktree's re-pointed
    /// `commondir` leads to. The retained repository answers only where its
    /// own directories are the captured ones; otherwise Git is asked again,
    /// and a mismatch found fails as `RepositoryGone`, naming the directory
    /// that differs. A tab restored from a session also compares the inodes
    /// it saved ([`GitDirectories::restored`]), which the same paths through
    /// a folder above re-pointed at another repository cannot match; it is
    /// the open here that reads them, never the launch.
    fn open(
        &mut self,
        requested: &Path,
        captured: Option<&Path>,
        git_dir: Option<&GitDirectories>,
    ) -> Result<GitRepository> {
        let canonical = requested.canonicalize()?;
        let folders = captured.and_then(|root| FoldersAbove::of(root, &canonical));
        let through = |path: &Path| match &folders {
            Some(folders) => folders.through(path),
            None => path.to_owned(),
        };
        let captured = captured.map(through);
        let git_dir = git_dir.map(|directories| GitDirectories {
            git_dir: through(&directories.git_dir),
            common: through(&directories.common),
            ..*directories
        });
        let git_dir = git_dir.as_ref();
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
            && captured.as_ref().is_none_or(|root| *root == canonical)
            && git_dir.is_none_or(|git_dir| git_dir.mismatch(&current.repository).is_none())
        {
            return Ok(current.repository.as_ref().clone());
        }
        // A nested directory needs Git discovery; it may still resolve to the
        // existing root, in which case keep the original persistent reader.
        // Git finding no repository there fails as it always has, with the
        // reasons `repository_access::explain` gives a folder that is not
        // one; only another repository found is `RepositoryGone` (see
        // `refused_open`).
        let passive = captured.is_some();
        let discovered = GitRepository::open(&canonical)
            .map_err(|error| refused_open(requested, passive, error))?;
        let canonical_root = discovered.path().canonicalize()?;
        if captured
            .as_ref()
            .is_some_and(|root| *root != canonical_root)
        {
            return Err(RepositoryGone {
                path: requested.to_owned(),
                found: Some(discovered.path().to_owned()),
            }
            .into());
        }
        if let Some(found) = git_dir.and_then(|git_dir| git_dir.mismatch(&discovered)) {
            return Err(RepositoryGone {
                path: requested.to_owned(),
                found: Some(found),
            }
            .into());
        }
        if let Some(current) = &self.current
            && current.canonical_root == canonical_root
            && GitDirectories::of(&discovered)
                .mismatch(&current.repository)
                .is_none()
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
                // A position past the history's end, as a restored tab's
                // saved page is where a shallower fetch cut it, reads as
                // the empty page at the end, which tells the window where
                // the history ends (`receive_history_page`).
                if page.next_offset.is_none() {
                    break;
                }
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
        state.supersede_active();
        state.closed = true;
        state.pending = None;
        ready.notify_all();
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

    /// The key of a comparison preview (`Job::Preview`), the whole of what
    /// it reads, as its cache entry is keyed.
    fn of(job: &Job) -> Option<Self> {
        let Job::Preview {
            repo,
            file,
            origins,
        } = job
        else {
            return None;
        };
        Some(Self {
            origins: origins.clone(),
            ..Self::new(repo.path(), file)
        })
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
            let mut lfs = LfsSides::default();
            let mut content = if let Some(content) =
                supplied_nontext_content(&repo, &file, &[], &bytes, cancellation)?
            {
                content
            } else if let Some(content) =
                resolved_lfs_text_content(&repo, &file, &[], &bytes, &mut lfs, cancellation)?
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
            lfs.mark(&mut content);
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
            // A quiet read reopens the tab's repository passively: where Git
            // no longer finds it at its path, every read there would read the
            // work tree enclosing it, so it reads nothing ([`RepositoryGone`]).
            // The retained session answers from its filesystem identities
            // without starting Git while the repository is unchanged. Where
            // Git finds no repository there at all, the read fails as gone
            // with nothing found, so the tab is read as Refresh reads it and
            // its Open gives the reasons any folder that is not a repository
            // gets; it once said Git's own "not a git repository" as a local
            // refresh error and left the tab unmarked.
            let root = Some(repo.path());
            let captured = GitDirectories::of(&repo);
            // The handle's own check, before the session's reopen: the
            // session answers from its directories' and links' identities,
            // which a `core.worktree` re-pointed in the repository's
            // configuration leaves as they were while it moves the work
            // tree; the handle's check compares those configuration files
            // and asks Git once one changed (`GitRepository::verify_at_path`).
            ensure_found_at_path(&repo)?;
            session
                .open(repo.path(), root, Some(&captured))
                .map_err(|error| nothing_found_is_gone(&repo, error))?;
            let working = WorkingState::read_with_checkpoint(&repo, || {
                #[cfg(test)]
                quiet_checkpoint();
                cancellation.check()
            })
            .map_err(|error| confined_failure(&repo, error))?;
            cancellation.check()?;
            let snapshot = history.then(|| {
                match read_snapshot(
                    repo.path().to_owned(),
                    (root, Some(&captured)),
                    scope.as_ref(),
                    MissingScope::Fail,
                    limit,
                    cancellation,
                    session,
                ) {
                    Ok(snapshot) => Ok(QuietHistory::Refreshed(worker_shallow(snapshot, &repo))),
                    Err(error) => {
                        cancellation.check()?;
                        // A zero-row unscoped snapshot reads current navigation
                        // metadata without loading a replacement history page.
                        let metadata = read_snapshot(
                            repo.path().to_owned(),
                            (root, Some(&captured)),
                            None,
                            MissingScope::Fail,
                            0,
                            cancellation,
                            session,
                        )?;
                        Ok(QuietHistory::Retained {
                            metadata,
                            vanished: error.downcast_ref::<VanishedScope>().cloned(),
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
            // A read already running when the repository's `.git` went away
            // fails part way, its commands confined to it: its operation
            // state, its History or its comparison fails as "not a git
            // repository", which once showed as an "Operation state: …" or
            // "History scope changed: …" banner beside the tab's mark. Any
            // part failed, the repository is checked again (from the
            // filesystem while it holds), and a repository gone is the whole
            // read's answer, so the tab is marked and says only that.
            let working = if matches!(snapshot, Some(Err(_) | Ok(QuietHistory::Retained { .. })))
                || matches!(preview, Some(Err(_)))
            {
                ensure_found_at_path(&repo)?;
                working
            } else {
                working.confirmed(&repo)?
            };
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
            let mut lfs = LfsSides::default();
            let mut content = if let Some(content) =
                supplied_nontext_content(&repo, &file, &preview.old, &preview.new, cancellation)?
            {
                content
            } else if let Some(content) = resolved_lfs_text_content(
                &repo,
                &file,
                &preview.old,
                &preview.new,
                &mut lfs,
                cancellation,
            )? {
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
                        "Text preview exceeds the display budget · before {} · after {}.",
                        crate::counted(old_bytes, "byte", "bytes"),
                        crate::counted(new_bytes, "byte", "bytes")
                    )),
                    TextPreview::Submodule { old_oid, new_oid } => Content::Notice(format!(
                        "Submodule · {} → {}",
                        old_oid.as_deref().unwrap_or("absent"),
                        new_oid.as_deref().unwrap_or("absent")
                    )),
                }
            };
            lfs.mark(&mut content);
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
        Job::Open {
            path,
            scope,
            limit,
            root,
            git_dir,
        } => {
            // An explicit Open (Refresh, Show latest, a restored tab) resolves
            // the scope again; a vanished one opens All history, never an
            // unavailable repository. A tab's repository gone from its root
            // says so in its own words.
            let mut snapshot = read_snapshot(
                path.clone(),
                (root.as_deref(), git_dir.as_ref()),
                scope.as_ref(),
                MissingScope::AllHistory,
                limit,
                cancellation,
                session,
            )
            .map_err(|error| worded_open_failure(&path, error))?;
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

/// The snapshot of the repository at `path`: the one Git discovery finds,
/// or, for a tab's passive reopen, the one at `root` or none
/// ([`RepositorySession::open`]).
fn read_snapshot(
    path: PathBuf,
    (root, git_dir): (Option<&Path>, Option<&GitDirectories>),
    scope: Option<&Scope>,
    missing: MissingScope,
    limit: usize,
    cancellation: &Cancellation,
    session: &mut RepositorySession,
) -> Result<Snapshot> {
    let start = Instant::now();
    cancellation.check()?;
    let repository = session.open(&path, root, git_dir)?;
    cancellation.check()?;
    let branches = repository.branches()?;
    cancellation.check()?;
    let worktrees = repository.worktrees()?;
    cancellation.check()?;
    let (oid, vanished_scope) = match scope_oid(scope, &branches, &worktrees) {
        Ok(oid) => (oid, None),
        Err(vanished) if missing == MissingScope::AllHistory => (None, Some(vanished)),
        Err(vanished) => return Err(vanished.into()),
    };
    let scope = match oid {
        Some(oid) if oid.bytes().all(|byte| byte == b'0') => {
            gitturtle_core::HistoryScope::PinnedRefs(Vec::new())
        }
        Some(oid) => gitturtle_core::HistoryScope::FromCommit(oid.to_owned()),
        None => gitturtle_core::HistoryScope::AllRefs,
    };
    // The tag read needs nothing from the history page, so one scoped thread
    // reads the tags and prepares every commit's references while this one
    // reads the page: a snapshot runs at most one Git read beside its page,
    // and the scope joins it, whatever the page's outcome, before returning.
    let ((commits, graph, graph_notice, history_scope, history_next_offset), refs) =
        std::thread::scope(|threads| -> Result<_> {
            let references = || history_references(&repository, &branches, cancellation);
            let beside = std::thread::Builder::new()
                .name("gitturtle-tags".into())
                .spawn_scoped(threads, references)
                .ok();
            let page = if limit == 0 {
                (Vec::new(), Vec::new(), None, scope, None)
            } else {
                // An explicit or quiet refresh captures new tips and starts a
                // new traversal. Ordinary subsequent pages use the immutable
                // returned scope.
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
            let refs = match beside {
                Some(read) => read
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                // A thread the system cannot start reads them after the page.
                None => references(),
            };
            Ok((page, refs?))
        })?;
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
        vanished_scope,
        shallow: false,
    })
}

/// `snapshot` with whether `repo` is shallow, `true` where that cannot be
/// read, as the landing then draws whatever it installs.
fn worker_shallow(mut snapshot: Snapshot, repo: &GitRepository) -> Snapshot {
    snapshot.shallow = repo.is_shallow().unwrap_or(true);
    snapshot
}

/// Each commit's references in Git's order, HEAD's branch first, then its
/// tags: History shows the first one in its badge and in narrow History's
/// pill. A tag list Git cannot give leaves History without tags rather than
/// without history; the Tags page reports why. The read is the Tags page's
/// own passive one, within its deadline, and a newer request stops and reaps
/// its Git process.
fn history_references(
    repository: &GitRepository,
    branches: &[Branch],
    cancellation: &Cancellation,
) -> Result<HistoryReferences> {
    let tags = gitturtle_core::run_cancellable_inspection(cancellation.history.clone(), || {
        repository.tags()
    })
    .map(|list| list.tags)
    .unwrap_or_default();
    cancellation.check()?;
    Ok(HistoryReferences::new(branches, &tags))
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
) -> std::result::Result<Option<&'a str>, VanishedScope> {
    let Some(scope) = scope else {
        return Ok(None);
    };
    let resolved = match scope {
        Scope::Branch { name, remote } => branches
            .iter()
            .find(|branch| branch.name == *name && branch.remote == *remote)
            .map(|branch| branch.oid.as_str()),
        Scope::Worktree { path } => worktrees
            .iter()
            .find(|worktree| worktree.path == *path)
            .map(|worktree| worktree.oid.as_str()),
    };
    resolved
        .map(Some)
        .ok_or_else(|| VanishedScope(scope.clone()))
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
    let absent = matches!(local, Ok(None));
    let error = match local {
        Ok(Some(local)) => {
            return crate::rich_preview::Side::prepare(local, path, || cancellation.check());
        }
        Ok(None) => format!(
            "This model’s LFS object is unavailable in the local store ({}). No download was attempted.",
            crate::counted(pointer.size, "byte", "bytes")
        ),
        Err(error) => format!(
            "This model’s local LFS object could not be verified: {error:#}. No download was attempted."
        ),
    };
    let mut side = crate::rich_preview::Side::unavailable(path, true, Some(error));
    side.lfs_missing = absent;
    side.metadata = metadata::inspect(&bytes, path);
    side.metadata.format = "Git LFS pointer".into();
    // Keep the captured identity available to Copy and explicit download actions.
    // Its error prevents immutable caching while the local store can change.
    side.metadata.source = std::str::from_utf8(&bytes).ok().map(Arc::from);
    side.capture(bytes.into(), &|| cancellation.check())?;
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
                    system_preview: None,
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
        lfs_missing: [false; 2],
        lfs_failed: [None; 2],
    }
}

/// What the local store said of each side's Git LFS object
/// ([`GitRepository::resolved_lfs_text`]), old then new.
#[derive(Clone, Copy, Default)]
struct LfsSides {
    missing: [bool; 2],
    failed: [Option<gitturtle_core::LfsObjectFailure>; 2],
}

impl LfsSides {
    /// Records them on `content`. Text shows each side beside these marks;
    /// a notice, which shows neither side, says what is wrong with any side
    /// that failed, so a binary comparison does not hide it.
    fn mark(self, content: &mut Content) {
        match content {
            Content::Text {
                lfs_missing,
                lfs_failed,
                ..
            } => {
                *lfs_missing = self.missing;
                *lfs_failed = self.failed;
            }
            Content::Notice(message) => {
                for (side, failure) in ["Before", "After"].into_iter().zip(self.failed) {
                    if let Some(failure) = failure {
                        let words = crate::lfs_download::failure_words(failure);
                        message.push_str(&format!("\n\n{side}: this {words}."));
                    }
                }
            }
            _ => {}
        }
    }
}

fn attach_mermaid(
    content: &mut Content,
    file: &FileChange,
    cancellation: &Cancellation,
) -> Result<()> {
    if let Content::Text {
        old,
        new,
        markdown,
        lfs_failed,
        ..
    } = content
        && markdown.is_none()
    {
        *markdown =
            crate::markdown_view::prepare(old, new, file, *lfs_failed, || cancellation.check())?
                .map(Arc::new);
    }
    if let Content::Text {
        old,
        new,
        diagrams,
        markdown,
        lfs_failed,
        ..
    } = content
        && diagrams.is_none()
    {
        *diagrams = if let Some(markdown) = markdown {
            markdown.diagrams().map(Arc::new)
        } else {
            crate::rich_preview::prepare_mermaid(old, new, file, *lfs_failed, || {
                cancellation.check()
            })?
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
                lfs_missing: aq,
                lfs_failed: ar,
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
                lfs_missing: bq,
                lfs_failed: br,
                ..
            },
        ) => {
            a == b
                && aq == bq
                && ar == br
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
    let mut lfs = LfsSides::default();
    if let Some(mut content) = resolved_lfs_text_content(
        repo,
        file,
        &sources.old,
        &sources.new,
        &mut lfs,
        cancellation,
    )? {
        lfs.mark(&mut content);
        attach_mermaid(&mut content, file, cancellation)?;
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
    lfs.mark(&mut content);
    attach_mermaid(&mut content, file, cancellation)?;
    Ok(content)
}

/// The comparison with the Git LFS objects the local store could show, or
/// `None` to keep the stored texts. `lfs` receives which sides' objects are
/// truly absent or unusable either way, for the caller to mark on what it
/// shows before it attaches Mermaid, which reads those marks.
fn resolved_lfs_text_content(
    repo: &GitRepository,
    file: &FileChange,
    old: &[u8],
    new: &[u8],
    lfs: &mut LfsSides,
    cancellation: &Cancellation,
) -> Result<Option<Content>> {
    let resolved = repo.resolved_lfs_text(file, old, new)?;
    *lfs = LfsSides {
        missing: resolved.missing,
        failed: resolved.failed,
    };
    let Some(sources) = resolved.text else {
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
    Ok(Some(match sources.preview {
        TextPreview::Patch(patch) => prepared_text(patch, String::from_utf8(sources.old)?, String::from_utf8(sources.new)?),
        TextPreview::Binary => Content::Notice("Verified LFS object downloaded. Its content is binary or not UTF-8 and has no text preview.".into()),
        TextPreview::TooLarge { old_bytes, new_bytes } => Content::Notice(format!("Verified LFS content exceeds the text display limit · {old_bytes} → {new_bytes} bytes.")),
        TextPreview::Submodule { .. } => Content::Notice("Submodules do not have an LFS text preview.".into()),
    }))
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
            system_preview: None,
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
                    "Git LFS object is unavailable in the local store · {}. No download was attempted.",
                    crate::counted(pointer.size, "byte", "bytes")
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
        // AVIF and JPEG 2000 decode in the confined helper on Linux, which
        // the reader's check kills as soon as a newer request supersedes
        // this one. An image decoded in this process checks nothing until it
        // ends, so the same preview asked for again meanwhile takes the
        // decode over (`Cancellation::unobserved`).
        let decode =
            || crate::image_helper::decode(&bytes, name, PREVIEW_EDGE, &|| cancellation.check());
        let decoded = if decoded_in_process(&bytes) {
            cancellation.unobserved(decode)??
        } else {
            decode()?
        };
        cancellation.check()?;
        let render = render_image(&decoded)?;
        Ok((decoded, render, bytes.len()))
    })();
    let literal_source = (bytes.len() <= gitturtle_preview::MAX_SVG_BYTES)
        .then(|| std::str::from_utf8(&bytes).ok().map(Arc::<str>::from))
        .flatten();
    let captured = (bytes.len() <= MAX_INPUT_BYTES && missing_pointer.is_none())
        .then(|| Arc::<[u8]>::from(bytes));
    // A cancelled decision offers nothing; the caller's next check ends the job.
    let system_preview = captured.as_deref().and_then(|bytes| {
        crate::system_preview::offer(bytes, &|| cancellation.check())
            .ok()
            .flatten()
    });
    match result {
        Ok((image, render, bytes)) => ImageSide {
            animation,
            captured,
            system_preview,
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
            system_preview,
            literal_source,
            image: None,
            render: None,
            message: Some(format!("{error:#}")),
            bytes: 0,
            lfs_pointer: missing_pointer,
        },
    }
}

/// Whether [`crate::image_helper::decode`] decodes `bytes` in this process,
/// where it checks nothing until it ends, rather than in the confined helper,
/// which runs the reader's check at least every 25 ms.
fn decoded_in_process(bytes: &[u8]) -> bool {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        gitturtle_preview::confined_image_format(bytes).is_none()
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = bytes;
        true
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
            root: None,
            git_dir: None,
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

    /// Waiting for the reader to go idle while a read never answers fails
    /// rather than waiting for good.
    #[test]
    #[should_panic(expected = "the repository reader did not answer or skip its requests")]
    fn waiting_for_a_read_that_never_answers_fails_rather_than_hangs() {
        let (started_tx, started_rx) = mpsc::channel();
        let (_release, held) = mpsc::channel::<()>();
        let worker = Worker::with_executor(move |_, _, _| {
            started_tx.send(())?;
            let _ = held.recv();
            Ok(notice("late"))
        });
        let _reply = worker.submit(open_job("held"));
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.wait_idle_within(Duration::from_millis(50));
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
            lfs_missing: [false; 2],
            lfs_failed: [None; 2],
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
            preparation: None,
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
                root: None,
                git_dir: None,
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
        // A position past the end reads as the empty page at the end, and
        // the last page's worth of rows up to it keeps its ancestry.
        let past = read(5000, &mut session);
        assert!(past.page.commits.is_empty(), "past the end");
        assert_eq!((past.page.offset, past.page.next_offset), (1203, None));
        let last = read(703, &mut session);
        assert_eq!(last.page.commits, expected[703..]);
        assert_eq!(last.graph, expected_graph[703..]);
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
            preparation: None,
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
        assert_eq!(
            error.downcast_ref::<VanishedScope>(),
            Some(&VanishedScope(Scope::Branch {
                name: "deleted".into(),
                remote: false,
            })),
            "search reports the scope change, not an unavailable repository"
        );
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
        let QuietHistory::Retained {
            metadata,
            error,
            vanished,
        } = refresh.snapshot.unwrap().unwrap()
        else {
            panic!("missing scopes must retain displayed history")
        };
        assert!(error.contains("departing"));
        assert_eq!(
            vanished,
            Some(VanishedScope(Scope::Branch {
                name: "departing".into(),
                remote: false,
            })),
            "the view reports one vanished scope once by its identity"
        );
        assert!(
            metadata
                .branches
                .iter()
                .all(|branch| branch.name != "departing")
        );
        assert!(
            metadata
                .refs
                .commits()
                .flat_map(|refs| &refs.names)
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
                system_preview: None,
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
                system_preview: None,
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

    /// A comparison preview chosen again while its image is still decoded
    /// in this process, a phase that checks nothing until it ends, is
    /// answered by that preparation, even after another file was chosen in
    /// between and superseded it: the reader prepares it once and never
    /// prepares the file chosen in between. A preparation whose decode ended
    /// superseded has stopped, so choosing its file again prepares it anew.
    /// Chosen again, a preview was prepared from the start each time: an
    /// image's bytes read and decoded twice.
    #[test]
    fn a_preview_chosen_again_while_it_is_prepared_is_prepared_once() {
        use futures::executor::block_on;
        use std::sync::{atomic::AtomicBool, mpsc};

        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let preview = |name: &str| Job::Preview {
            repo: repo.clone(),
            file: FileChange {
                old_path: None,
                new_path: Some(name.into()),
                old_oid: None,
                new_oid: Some("2".repeat(40)),
                status: gitturtle_core::ChangeStatus::Added,
                old_mode: "000000".into(),
                new_mode: "100644".into(),
            },
            origins: Default::default(),
        };
        let shown = |output: Output| match output {
            Output::Preview(content, _) => match &*content {
                Content::Notice(name) => name.clone(),
                _ => panic!("expected the test's notice"),
            },
            _ => panic!("expected a preview"),
        };
        let (started_tx, started) = mpsc::channel();
        let (checked_tx, checked) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        // Holds the next preparation of "slow.png" in a phase that checks
        // nothing until it ends, as an image decoded in this process does,
        // until released; with `pause` set it then reports whether that
        // phase ended answering and waits once more before it ends.
        let hold = Arc::new(AtomicBool::new(false));
        let pause = Arc::new(AtomicBool::new(false));
        let (held, paused) = (Arc::clone(&hold), Arc::clone(&pause));
        let worker = Worker::with_executor(move |job, _, cancellation| {
            let Job::Preview { file, .. } = job else {
                unreachable!("the test asks for previews only")
            };
            let name = file.path().to_string_lossy().into_owned();
            if name == "slow.png" && held.swap(false, Ordering::AcqRel) {
                let decoded = cancellation.unobserved(|| {
                    let _ = started_tx.send(name.clone());
                    released.recv_timeout(Duration::from_secs(10))
                });
                if paused.swap(false, Ordering::AcqRel) {
                    checked_tx.send(decoded.is_ok())?;
                    released.recv_timeout(Duration::from_secs(10))?;
                }
                decoded??;
            } else {
                started_tx.send(name.clone())?;
            }
            cancellation.check()?;
            Ok(Output::Preview(
                Arc::new(Content::Notice(name)),
                Duration::ZERO,
            ))
        });
        let next_started = || started.recv_timeout(Duration::from_secs(10)).unwrap();

        // Chosen, another chosen, then chosen again while it is prepared.
        hold.store(true, Ordering::Release);
        let first = worker.submit(preview("slow.png"));
        assert_eq!(next_started(), "slow.png");
        let mut between = worker.submit(preview("other.png"));
        let again = worker.submit(preview("slow.png"));
        assert!(
            between.try_recv().is_err(),
            "the file chosen in between is never read"
        );
        release.send(()).unwrap();
        assert_eq!(shown(block_on(again).unwrap().unwrap()), "slow.png");
        worker.wait_idle();
        assert_eq!(
            started.try_iter().collect::<Vec<_>>(),
            Vec::<String>::new(),
            "slow.png is prepared once, other.png never"
        );
        assert!(
            !matches!(block_on(first), Ok(Ok(_))),
            "the request the preparation began for is answered no more"
        );
        assert_eq!(worker.preparations(), 1);

        // Stopped as its decode ends superseded, a preparation is not taken
        // over.
        hold.store(true, Ordering::Release);
        pause.store(true, Ordering::Release);
        let first = worker.submit(preview("slow.png"));
        assert_eq!(next_started(), "slow.png");
        let _between = worker.submit(preview("other.png"));
        release.send(()).unwrap();
        assert!(
            !checked.recv_timeout(Duration::from_secs(10)).unwrap(),
            "its decode ends superseded"
        );
        let again = worker.submit(preview("slow.png"));
        release.send(()).unwrap();
        assert!(
            block_on(first).unwrap().is_err(),
            "the stopped preparation ends superseded"
        );
        assert_eq!(
            next_started(),
            "slow.png",
            "chosen again, it is prepared anew"
        );
        assert_eq!(shown(block_on(again).unwrap().unwrap()), "slow.png");
        worker.wait_idle();
        assert_eq!(worker.preparations(), 3);
    }

    /// A comparison preview's work that watches the request's cancellation
    /// token, as a Markdown document's local images do as they are captured
    /// (`markdown_view::capture_assets`: its Git reads, which the token's
    /// cancellation terminates, and each image's check), stops as soon as
    /// another file is chosen, as Back cancels the read, and as the window's
    /// worker goes with its tab: the token is cancelled then, as any read's
    /// is. Such work is never taken over: choosing its file again prepares it
    /// anew. Kept as a preparation another request could take over, the
    /// preview once left the token alone, so the capture ran to its end and
    /// the next file's preview waited behind it.
    #[test]
    fn superseding_back_and_closing_stop_a_previews_token_watching_work() {
        use futures::executor::block_on;
        use std::sync::mpsc;

        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let preview = |name: &str| Job::Preview {
            repo: repo.clone(),
            file: FileChange {
                old_path: None,
                new_path: Some(name.into()),
                old_oid: None,
                new_oid: Some("2".repeat(40)),
                status: gitturtle_core::ChangeStatus::Added,
                old_mode: "000000".into(),
                new_mode: "100644".into(),
            },
            origins: Default::default(),
        };
        // "capture.md" stands for a document whose images are captured: it
        // watches only the token, for up to ten seconds, and reports whether
        // the token was cancelled.
        let (started_tx, started) = mpsc::channel();
        let (stopped_tx, stopped) = mpsc::channel();
        let worker = Worker::with_executor(move |job, _, cancellation| {
            let Job::Preview { file, .. } = job else {
                unreachable!("the test asks for previews only")
            };
            let name = file.path().to_string_lossy().into_owned();
            started_tx.send(name.clone())?;
            if name == "capture.md" {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !cancellation.history.is_cancelled() && Instant::now() < deadline {
                    std::thread::yield_now();
                }
                stopped_tx.send(cancellation.history.is_cancelled())?;
            }
            cancellation.check()?;
            Ok(Output::Preview(
                Arc::new(Content::Notice(name)),
                Duration::ZERO,
            ))
        });
        fn next<T>(receiver: &mpsc::Receiver<T>) -> T {
            receiver.recv_timeout(Duration::from_secs(20)).unwrap()
        }

        // Another file chosen, then the first again: the capture stops at
        // once and its file is prepared anew. Once the capture has stopped
        // the reader may start the file chosen in between before the first is
        // chosen again, as it does whenever the test is slow to choose it;
        // that file is then superseded in turn, and answers, if at all, only
        // for itself.
        let first = worker.submit(preview("capture.md"));
        assert_eq!(next(&started), "capture.md");
        let between = worker.submit(preview("other.md"));
        assert!(next(&stopped), "choosing another file stops the capture");
        let again = worker.submit(preview("capture.md"));
        assert!(block_on(first).unwrap().is_err());
        let mut restarted = next(&started);
        let between_started = restarted == "other.md";
        if between_started {
            restarted = next(&started);
        }
        assert_eq!(restarted, "capture.md", "chosen again, prepared anew");
        if let Ok(Ok(Output::Preview(content, _))) = block_on(between) {
            assert!(
                matches!(&*content, Content::Notice(name) if name == "other.md"),
                "the file chosen in between answers only for itself"
            );
        }
        // Back cancels the read.
        worker.cancel();
        assert!(next(&stopped), "Back stops the capture");
        assert!(block_on(again).unwrap().is_err());
        worker.wait_idle();
        // The reader counts a preparation as it takes the request and checks
        // the request before the work starts, so the file chosen in between
        // may be counted, stopped before it reports starting.
        let preparations = worker.preparations();
        assert!(
            (2 + u64::from(between_started)..=3).contains(&preparations),
            "{preparations} preparations, the file chosen in between started: {between_started}"
        );

        // The tab closes, and its worker goes.
        let _closing = worker.submit(preview("capture.md"));
        assert_eq!(next(&started), "capture.md");
        drop(worker);
        assert!(next(&stopped), "closing the tab stops the capture");
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
                if corrupt {
                    // A download is refused for a local object that fails
                    // verification, so none is offered.
                    assert!(targets.is_empty());
                    continue;
                }
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

    /// Whether a side offers Open in default app is decided here, with its
    /// captured bytes and from them alone: a GIF named like a launcher and a
    /// PDF named like a script offer their own types, and SVG and audio,
    /// whatever their names, offer nothing.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_default_app_offer_is_decided_with_the_captured_bytes() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let gif = include_bytes!("../../preview/tests/fixtures/disposal-transparency.gif");
        let content = captured_preview(&repo, added_file(&fixture, "launcher.desktop", gif));
        let Content::Images { old, new } = &*content else {
            panic!("GIF magic routes to images")
        };
        assert_eq!(old.system_preview, None, "an absent side offers nothing");
        let offer = new.system_preview.expect("a GIF offers the action");
        assert!(offer.tooltip().contains(" as a GIF image "));
        let svg = captured_preview(&repo, added_file(&fixture, "picture.png", SVG));
        let Content::Images { new, .. } = &*svg else {
            panic!("content should identify SVG")
        };
        assert!(new.captured.is_some());
        assert_eq!(new.system_preview, None);
        let pdf = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n";
        let rich = captured_preview(&repo, added_file(&fixture, "install.sh", pdf));
        let Content::Rich(preview) = &*rich else {
            panic!("PDF is a rich side")
        };
        let offer = preview.new.system_preview.expect("PDF offers the action");
        assert!(offer.tooltip().contains(" as a PDF document "));
        let wav = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x44\xac\0\0\x88\x58\x01\0\x02\0\x10\0data\0\0\0\0";
        let audio = captured_preview(&repo, added_file(&fixture, "sound.wav", wav));
        let Content::Rich(preview) = &*audio else {
            panic!("WAV is captured metadata")
        };
        assert!(preview.new.captured.is_some());
        assert_eq!(preview.new.system_preview, None, "audio offers nothing");
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

    /// Each side's Git LFS object is judged on its own. One over the preview
    /// limit or failing verification stays its pointer and says which, and
    /// the other side keeps its preview, where either once failed the whole
    /// file. Neither offers a download, which could not help; a side whose
    /// object is only missing still does. Nothing is cached: the store can
    /// change under the pointer.
    #[test]
    fn a_failing_lfs_side_leaves_the_other_sides_preview_and_offers_no_download() {
        use gitturtle_core::LfsObjectFailure::{Corrupt, TooLarge};
        use sha2::{Digest, Sha256};
        let fixture = Fixture::new();
        // The pointer to `body`, with `stored` as its object in the local store.
        let pointer = |body: &[u8], stored: Option<&[u8]>| {
            let oid = format!("{:x}", Sha256::digest(body));
            if let Some(stored) = stored {
                let directory = fixture
                    .0
                    .join(".git/lfs/objects")
                    .join(&oid[..2])
                    .join(&oid[2..4]);
                fs::create_dir_all(&directory).unwrap();
                fs::write(directory.join(&oid), stored).unwrap();
            }
            format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
                body.len()
            )
        };
        let oversize = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
            "e".repeat(64),
            33 * 1024 * 1024
        );
        // The right size and the wrong bytes.
        let corrupt = pointer(b"the true text\n", Some(b"a wrong objec\n"));
        let local_body = "the local object\n";
        let local = pointer(local_body.as_bytes(), Some(local_body.as_bytes()));
        let missing = pointer(b"never stored\n", None);
        let plain = "plain text\n";
        let (oversize, corrupt, local, missing) = (
            oversize.as_str(),
            corrupt.as_str(),
            local.as_str(),
            missing.as_str(),
        );
        let repo = GitRepository::open(&fixture.0).unwrap();
        let modified = |old: &[u8], new: &[u8]| FileChange {
            old_path: Some("notes.txt".into()),
            new_path: Some("notes.txt".into()),
            old_oid: Some(fixture.blob(old)),
            new_oid: Some(fixture.blob(new)),
            status: ChangeStatus::Modified,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
        };
        type Row<'a> = (
            &'a str,
            &'a str,
            &'a str,
            &'a str,
            &'a str,
            [bool; 2],
            [Option<gitturtle_core::LfsObjectFailure>; 2],
            &'a [usize],
        );
        let rows: [Row<'_>; 7] = [
            (
                "over the limit first beside plain text",
                oversize,
                plain,
                oversize,
                plain,
                [false, false],
                [Some(TooLarge), None],
                &[],
            ),
            (
                "corrupt first beside plain text",
                corrupt,
                plain,
                corrupt,
                plain,
                [false, false],
                [Some(Corrupt), None],
                &[],
            ),
            (
                "plain text first beside a corrupt object",
                plain,
                corrupt,
                plain,
                corrupt,
                [false, false],
                [None, Some(Corrupt)],
                &[],
            ),
            (
                "corrupt first beside a local object",
                corrupt,
                local,
                corrupt,
                local_body,
                [false, false],
                [Some(Corrupt), None],
                &[],
            ),
            (
                "a local object first beside one over the limit",
                local,
                oversize,
                local_body,
                oversize,
                [false, false],
                [None, Some(TooLarge)],
                &[],
            ),
            (
                "over the limit first beside a missing object",
                oversize,
                missing,
                oversize,
                missing,
                [false, true],
                [Some(TooLarge), None],
                &[1],
            ),
            (
                "both unusable",
                corrupt,
                oversize,
                corrupt,
                oversize,
                [false, false],
                [Some(Corrupt), Some(TooLarge)],
                &[],
            ),
        ];
        for (name, old, new, shown_old, shown_new, absent, failed, offered) in rows {
            let file = modified(old.as_bytes(), new.as_bytes());
            let mut cache = PreviewCache::default();
            let output = execute(
                Job::Preview {
                    origins: Default::default(),
                    repo: repo.clone(),
                    file: file.clone(),
                },
                &mut cache,
                &active(),
                &mut RepositorySession::default(),
            )
            .unwrap_or_else(|error| panic!("{name}: the whole preview failed: {error:#}"));
            let Output::Preview(content, _) = output else {
                panic!("{name}: expected a preview")
            };
            let Content::Text {
                old: drawn_old,
                new: drawn_new,
                lfs_missing,
                lfs_failed,
                ..
            } = &*content
            else {
                panic!("{name}: expected text")
            };
            assert_eq!(drawn_old, shown_old, "{name}");
            assert_eq!(drawn_new, shown_new, "{name}");
            assert_eq!(*lfs_missing, absent, "{name}");
            assert_eq!(*lfs_failed, failed, "{name}");
            assert_eq!(
                crate::lfs_download::preview_targets(&content, &file)
                    .iter()
                    .map(|(side, _)| *side)
                    .collect::<Vec<_>>(),
                offered,
                "{name}: only a missing object offers a download"
            );
            assert!(cache.entries.is_empty(), "{name}: a pointer stays uncached");
        }
        // With no text to show beside the pointer the file is a notice, which
        // still says what is wrong with the side that failed.
        let file = modified(oversize.as_bytes(), b"\0binary\n");
        let Output::Preview(content, _) = execute(
            Job::Preview {
                origins: Default::default(),
                repo,
                file,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected a preview")
        };
        let Content::Notice(message) = &*content else {
            panic!("expected the binary comparison's notice")
        };
        assert!(
            message.ends_with("\n\nBefore: this Git LFS object is over the 32 MiB preview limit."),
            "{message}"
        );
    }

    /// Mermaid is attached after the sides are marked, on both the route that
    /// resolves a side and the one that keeps the stored texts, so Diagrams
    /// says what is wrong with a failing side rather than that it is absent.
    #[test]
    fn a_failing_pointer_beside_mermaid_says_what_is_wrong_in_diagrams() {
        use sha2::{Digest, Sha256};
        let fixture = Fixture::new();
        let oversize = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\n",
            "e".repeat(64),
            33 * 1024 * 1024
        );
        let diagram = "flowchart LR\n  A --> B\n";
        // A local pointer to the diagram, so that side resolves.
        let oid = format!("{:x}", Sha256::digest(diagram));
        let directory = fixture
            .0
            .join(".git/lfs/objects")
            .join(&oid[..2])
            .join(&oid[2..4]);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(&oid), diagram).unwrap();
        let local = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
            diagram.len()
        );
        let repo = GitRepository::open(&fixture.0).unwrap();
        for (name, new) in [
            ("regular text", diagram),
            ("a resolved object", local.as_str()),
        ] {
            let file = FileChange {
                old_path: Some("flow.mmd".into()),
                new_path: Some("flow.mmd".into()),
                old_oid: Some(fixture.blob(oversize.as_bytes())),
                new_oid: Some(fixture.blob(new.as_bytes())),
                status: ChangeStatus::Modified,
                old_mode: "100644".into(),
                new_mode: "100644".into(),
            };
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
                panic!("{name}: expected a preview")
            };
            let Content::Text {
                diagrams: Some(diagrams),
                ..
            } = &*content
            else {
                panic!("{name}: the regular side draws")
            };
            assert_eq!(
                diagrams.old.metadata.details,
                [
                    "This side's Git LFS object is over the 32 MiB preview limit; its stored text \
                  is below."
                ],
                "{name}"
            );
            assert!(diagrams.new.pages[0].render.is_some(), "{name}");
        }
    }

    /// The same stored texts with a side's object failing differently are not
    /// the same content: what the side says depends on it, and it changes
    /// without the pointer's text changing.
    #[test]
    fn a_change_in_how_an_lfs_side_failed_is_a_content_change() {
        use gitturtle_core::LfsObjectFailure::{Corrupt, TooLarge};
        let text = |failed: [Option<gitturtle_core::LfsObjectFailure>; 2]| {
            let mut content = prepared_text("patch".into(), "old".into(), "new".into());
            LfsSides {
                missing: [false; 2],
                failed,
            }
            .mark(&mut content);
            content
        };
        assert!(content_unchanged(
            &text([Some(Corrupt), None]),
            &text([Some(Corrupt), None])
        ));
        assert!(!content_unchanged(
            &text([Some(Corrupt), None]),
            &text([Some(TooLarge), None])
        ));
        assert!(!content_unchanged(
            &text([None, None]),
            &text([Some(Corrupt), None])
        ));
    }

    /// A Markdown file with one pointer side keeps its Rendered view: the
    /// regular side renders, and the pointer side stays literal, saying what
    /// its own object is: absent, or why it cannot be used.
    #[test]
    fn a_mixed_markdown_preview_renders_the_regular_side() {
        let fixture = Fixture::new();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let pointer = |size: u64| {
            format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {size}\n",
                "b".repeat(64)
            )
        };
        for (name, stored, says, absent) in [
            (
                "missing",
                pointer(42),
                "This side's Git LFS object is not in the local store, and no download was \
                 attempted; its stored text is below.",
                true,
            ),
            (
                "over the limit",
                pointer(33 * 1024 * 1024),
                "This side's Git LFS object is over the 32 MiB preview limit; its stored text \
                 is below.",
                false,
            ),
        ] {
            let file = FileChange {
                old_path: Some("README.md".into()),
                new_path: Some("README.md".into()),
                old_oid: Some(fixture.blob(b"# Before\n")),
                new_oid: Some(fixture.blob(stored.as_bytes())),
                status: ChangeStatus::Modified,
                old_mode: "100644".into(),
                new_mode: "100644".into(),
            };
            let Output::Preview(content, _) = execute(
                Job::Preview {
                    origins: Default::default(),
                    repo: repo.clone(),
                    file: file.clone(),
                },
                &mut PreviewCache::default(),
                &active(),
                &mut RepositorySession::default(),
            )
            .unwrap() else {
                panic!("{name}: expected a preview")
            };
            let Content::Text {
                markdown: Some(markdown),
                lfs_missing,
                ..
            } = &*content
            else {
                panic!("{name}: a mixed file has a Rendered view")
            };
            assert!(
                markdown.old.literal.is_none(),
                "{name}: the regular side renders"
            );
            assert_eq!(markdown.new.literal.as_deref(), Some(says), "{name}");
            assert_eq!(*lfs_missing, [false, absent], "{name}");
            assert_eq!(
                crate::lfs_download::preview_targets(&content, &file)
                    .iter()
                    .map(|(side, _)| *side)
                    .collect::<Vec<_>>(),
                if absent { vec![1] } else { Vec::new() },
                "{name}"
            );
        }
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

    /// With one object in the local store and the other missing, the text
    /// shows the first and keeps the other's pointer, and nothing is cached:
    /// the missing object can still arrive.
    #[test]
    fn lfs_text_preview_resolves_the_side_whose_object_is_local() {
        let fixture = Fixture::new();
        let local = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{LFS_OID}\nsize {}\n",
            SVG.len()
        );
        let missing = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
            "b".repeat(64)
        );
        let directory = fixture
            .0
            .join(".git/lfs/objects")
            .join(&LFS_OID[..2])
            .join(&LFS_OID[2..4]);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(LFS_OID), SVG).unwrap();
        let repo = GitRepository::open(&fixture.0).unwrap();
        let file = FileChange {
            old_path: Some("source.txt".into()),
            new_path: Some("source.txt".into()),
            old_oid: Some(fixture.blob(local.as_bytes())),
            new_oid: Some(fixture.blob(missing.as_bytes())),
            status: ChangeStatus::Modified,
            old_mode: "100644".into(),
            new_mode: "100644".into(),
        };
        let mut cache = PreviewCache::default();
        let mut session = RepositorySession::default();
        let shown = execute(
            Job::Preview {
                origins: Default::default(),
                repo,
                file,
            },
            &mut cache,
            &active(),
            &mut session,
        )
        .unwrap();
        assert!(
            matches!(shown, Output::Preview(content, _) if matches!(content.as_ref(), Content::Text { old, new, partial, lfs_missing, .. } if old.as_bytes() == SVG && new == &missing && partial.is_none() && *lfs_missing == [false, true]))
        );
        assert!(cache.entries.is_empty());
    }

    /// A committed text file whose working copy is now a Git LFS pointer to
    /// an object the local store lacks, and that pointer's text.
    fn working_missing_pointer(fixture: &Fixture) -> String {
        fs::write(fixture.0.join("notes.txt"), "before\n").unwrap();
        fixture.git(&["add", "notes.txt"], b"");
        fixture.git(
            &["-c", "commit.gpgsign=false", "commit", "-qm", "Notes"],
            b"",
        );
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 42\n",
            "b".repeat(64)
        );
        fs::write(fixture.0.join("notes.txt"), &pointer).unwrap();
        pointer
    }

    /// The side a Working preview found absent is marked, so it offers its
    /// download: unmarked, the action silently goes.
    #[test]
    fn a_working_preview_marks_a_missing_lfs_object_and_offers_its_download() {
        let fixture = Fixture::new();
        let pointer = working_missing_pointer(&fixture);
        let repo = GitRepository::open(&fixture.0).unwrap();
        let entry = repo.status().unwrap().entries.remove(0);
        let Output::WorkingPreview(file, content, _) = execute(
            Job::WorkingPreview {
                head: None,
                repo,
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
        let Content::Text {
            new, lfs_missing, ..
        } = &*content
        else {
            panic!("expected the pointer's text")
        };
        assert_eq!(new, &pointer);
        assert_eq!(*lfs_missing, [false, true]);
        let targets = crate::lfs_download::preview_targets(&content, &file);
        assert_eq!(
            targets.iter().map(|(side, _)| *side).collect::<Vec<_>>(),
            [1]
        );
    }

    /// So is the side Quick Open's working source found absent.
    #[test]
    fn quick_open_marks_a_missing_lfs_object_and_offers_its_download() {
        let fixture = Fixture::new();
        let pointer = working_missing_pointer(&fixture);
        let repo = GitRepository::open(&fixture.0).unwrap();
        let paths = repo
            .search_tracked_paths(
                &gitturtle_core::PathScope::Worktree,
                "notes",
                &gitturtle_core::HistoryCancellation::default(),
            )
            .unwrap();
        let entry = paths.entries[0].clone();
        let file = repo.tracked_path_change(&entry);
        let Output::Preview(content, _) = execute(
            Job::TrackedPreview {
                repo,
                entry,
                scope: gitturtle_core::PathScope::Worktree,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap() else {
            panic!("expected quick source")
        };
        let Content::Text {
            new, lfs_missing, ..
        } = &*content
        else {
            panic!("expected the pointer's text")
        };
        assert_eq!(new, &pointer);
        assert_eq!(*lfs_missing, [false, true]);
        let targets = crate::lfs_download::preview_targets(&content, &file);
        assert_eq!(
            targets.iter().map(|(side, _)| *side).collect::<Vec<_>>(),
            [1]
        );
    }

    /// The same stored texts with another side's object missing are not the
    /// same content: what the file says and offers depends on it, and it
    /// changes without the pointer's text changing.
    #[test]
    fn a_change_in_which_lfs_object_is_missing_is_a_content_change() {
        let text = |missing: [bool; 2]| {
            let mut content = prepared_text("patch".into(), "old".into(), "new".into());
            LfsSides {
                missing,
                failed: [None; 2],
            }
            .mark(&mut content);
            content
        };
        assert!(content_unchanged(
            &text([true, false]),
            &text([true, false])
        ));
        assert!(!content_unchanged(
            &text([true, false]),
            &text([true, true])
        ));
    }

    /// A snapshot names each commit's tags after its branches, by name, an
    /// annotated tag at the commit it peels to; a tag of a blob or a tree
    /// names no commit. Each commit's strings are prepared once: the
    /// tooltip's lines and the accessible name's list say "tag ‹name›",
    /// "+N" counts the tags, and the most references any commit has is kept.
    #[test]
    fn snapshots_name_each_commits_tags_after_its_branches() {
        let fixture = Fixture::new();
        fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], &[]);
        let tree = fixture.git(&["mktree"], &[]);
        let first = fixture.git(&["commit-tree", &tree, "-m", "first"], &[]);
        let second = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "second"], &[]);
        fixture.git(&["update-ref", "refs/heads/main", &second], &[]);
        fixture.git(&["update-ref", "refs/heads/side", &first], &[]);
        fixture.git(&["tag", "v0.1", &first], &[]);
        fixture.git(&["tag", "-a", "-m", "Release", "v1.0", &second], &[]);
        fixture.git(&["tag", "a-first", &first], &[]);
        let blob = fixture.blob(b"notes\n");
        fixture.git(&["tag", "blob", &blob], &[]);
        fixture.git(&["tag", "-a", "-m", "Tree", "tree", &tree], &[]);
        let snapshot = read_snapshot(
            fixture.0.clone(),
            (None, None),
            None,
            MissingScope::Fail,
            10,
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap();
        let names = |oid: &str| {
            snapshot.refs.get(oid).map(|refs| {
                refs.names
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(names(&first).unwrap(), ["side", "a-first", "v0.1"]);
        assert_eq!(names(&second).unwrap(), ["main", "v1.0"]);
        assert_eq!(names(&blob), None);
        assert_eq!(names(&tree), None);
        let first = snapshot.refs.get(&first).unwrap();
        assert_eq!(first.branches, 1);
        assert!(!first.leads_with_tag());
        assert_eq!(first.more.as_deref(), Some("+2"));
        assert_eq!(first.lines.as_ref(), "side\ntag a-first\ntag v0.1");
        assert_eq!(first.listed.as_ref(), "side, tag a-first, tag v0.1");
        assert_eq!(snapshot.refs.widest(), 3);

        // A commit with tags alone leads with a tag.
        fixture.git(&["update-ref", "-d", "refs/heads/side"], &[]);
        let snapshot = read_snapshot(
            fixture.0.clone(),
            (None, None),
            None,
            MissingScope::Fail,
            10,
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap();
        let first = snapshot.refs.get(&snapshot.commits[1].oid).unwrap();
        assert!(first.leads_with_tag());
        assert_eq!(first.listed.as_ref(), "tag a-first, tag v0.1");
        assert_eq!(snapshot.refs.widest(), 2);
    }

    /// The tag read runs beside the history page; a tag list Git cannot give,
    /// here because of a symbolic tag reference, leaves the snapshot with its
    /// history and branches and without any tags.
    #[test]
    fn a_tag_list_git_cannot_give_leaves_history_without_tags() {
        let fixture = Fixture::new();
        fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], &[]);
        let tree = fixture.git(&["mktree"], &[]);
        let first = fixture.git(&["commit-tree", &tree, "-m", "first"], &[]);
        let second = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "second"], &[]);
        fixture.git(&["update-ref", "refs/heads/main", &second], &[]);
        fixture.git(&["tag", "v1.0", &second], &[]);
        fixture.git(&["tag", "v0.1", &first], &[]);
        fixture.git(
            &["symbolic-ref", "refs/tags/latest", "refs/heads/main"],
            &[],
        );
        let repository = GitRepository::open(&fixture.0).unwrap();
        assert!(repository.tags().is_err(), "Git cannot give this tag list");
        let snapshot = read_snapshot(
            fixture.0.clone(),
            (None, None),
            None,
            MissingScope::Fail,
            10,
            &active(),
            &mut RepositorySession::default(),
        )
        .unwrap();
        assert_eq!(
            snapshot
                .commits
                .iter()
                .map(|commit| commit.oid.as_str())
                .collect::<Vec<_>>(),
            [second.as_str(), first.as_str()]
        );
        let names = snapshot.refs.get(&second).unwrap();
        assert_eq!(names.names, ["main"]);
        assert_eq!(names.listed.as_ref(), "main");
        assert_eq!(snapshot.refs.get(&first), None);
        assert_eq!(snapshot.refs.widest(), 1);
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
        let before = read_snapshot(
            fixture.0.clone(),
            (None, None),
            Some(&scope),
            MissingScope::Fail,
            10,
            &active(),
            &mut session,
        )
        .unwrap();
        assert_eq!(before.commits[0].oid, first);
        let worktree_scope = Scope::Worktree {
            path: before.repository.path().to_owned(),
        };
        let second = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "second"], &[]);
        fixture.git(&["update-ref", "refs/heads/main", &second], &[]);
        for scope in [&scope, &worktree_scope] {
            let refreshed = read_snapshot(
                fixture.0.clone(),
                (None, None),
                Some(scope),
                MissingScope::Fail,
                10,
                &active(),
                &mut session,
            )
            .unwrap();
            assert_eq!(refreshed.commits[0].oid, second);
            assert_eq!(refreshed.commits[1].oid, first);
        }
        // A missing selection must never silently fall back to unrelated refs.
        fixture.git(&["update-ref", "-d", "refs/heads/main"], &[]);
        let error = read_snapshot(
            fixture.0.clone(),
            (None, None),
            Some(&scope),
            MissingScope::Fail,
            10,
            &active(),
            &mut session,
        )
        .err()
        .expect("deleted branch must fail");
        assert!(error.to_string().contains("no longer exists"));
    }

    #[test]
    fn explicit_open_with_a_vanished_scope_reads_all_history_and_names_the_scope() {
        let fixture = Fixture::new();
        fixture.git(&["symbolic-ref", "HEAD", "refs/heads/main"], b"");
        let tree = fixture.git(&["mktree"], b"");
        let first = fixture.git(&["commit-tree", &tree, "-m", "first"], b"");
        let side = fixture.git(&["commit-tree", &tree, "-p", &first, "-m", "side"], b"");
        fixture.git(&["update-ref", "refs/heads/main", &first], b"");
        fixture.git(&["update-ref", "refs/heads/departing", &side], b"");
        fixture.git(&["update-ref", "refs/heads/kept", &side], b"");
        let linked = fixture.0.with_file_name(format!(
            "{}-linked",
            fixture.0.file_name().unwrap().to_string_lossy()
        ));
        fixture.git(
            &["worktree", "add", "-q", linked.to_str().unwrap(), "kept"],
            b"",
        );
        let mut session = RepositorySession::default();
        let open = |scope: Scope, session: &mut RepositorySession| {
            let Output::Snapshot(snapshot) = execute(
                Job::Open {
                    path: fixture.0.clone(),
                    scope: Some(scope),
                    limit: 10,
                    root: None,
                    git_dir: None,
                },
                &mut PreviewCache::default(),
                &active(),
                session,
            )
            .unwrap() else {
                panic!("expected snapshot")
            };
            snapshot
        };
        let branch = Scope::Branch {
            name: "departing".into(),
            remote: false,
        };
        let scoped = open(branch.clone(), &mut session);
        assert_eq!(scoped.vanished_scope, None);
        let worktree = Scope::Worktree {
            path: scoped
                .worktrees
                .iter()
                .find(|worktree| worktree.branch.as_deref() == Some("kept"))
                .expect("linked worktree")
                .path
                .clone(),
        };
        assert_eq!(open(worktree.clone(), &mut session).vanished_scope, None);

        fixture.git(&["update-ref", "-d", "refs/heads/departing"], b"");
        fixture.git(
            &["worktree", "remove", "--force", linked.to_str().unwrap()],
            b"",
        );
        for scope in [branch, worktree] {
            let snapshot = open(scope.clone(), &mut session);
            assert_eq!(
                snapshot.vanished_scope,
                Some(VanishedScope(scope.clone())),
                "{scope:?}"
            );
            assert!(
                matches!(
                    snapshot.history_scope,
                    gitturtle_core::HistoryScope::PinnedRefs(_)
                ),
                "All history pins every current ref"
            );
            assert_eq!(
                snapshot
                    .commits
                    .iter()
                    .map(|commit| commit.oid.as_str())
                    .collect::<Vec<_>>(),
                vec![side.as_str(), first.as_str()],
                "{scope:?} reads the repository's commits from current refs"
            );
        }
        // Quiet refresh keeps failing so the displayed history is retained.
        let error = read_snapshot(
            fixture.0.clone(),
            (None, None),
            Some(&Scope::Branch {
                name: "departing".into(),
                remote: false,
            }),
            MissingScope::Fail,
            10,
            &active(),
            &mut session,
        )
        .err()
        .expect("a strict read fails");
        assert!(error.downcast_ref::<VanishedScope>().is_some());
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
                session
                    .open(&fixture.0, None, None)
                    .unwrap()
                    .blob(&old_blob)
                    .unwrap(),
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
            let reopened = session.open(&fixture.0, None, None).unwrap();
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
            session
                .open(&fixture.0, None, None)
                .unwrap()
                .blob(&old_blob)
                .unwrap(),
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
            session
                .open(&fixture.0, None, None)
                .unwrap()
                .blob(&new_blob)
                .unwrap(),
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
        let ui_repository = session.open(&fixture.0, None, None).unwrap();
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
            let reopened = session.open(path, None, None).unwrap();
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
        let linked_repo = session.open(&linked, None, None).unwrap();
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
        let main_repo = session.open(&fixture.0, None, None).unwrap();
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

    /// A tab's passive reopen finds the repository it opened or none. With
    /// the inner repository's `.git` gone, Git discovery from its root finds
    /// the work tree enclosing it: an explicit open takes that work tree, as
    /// a folder inside one opens it, while a quiet read from a session that
    /// retained the inner repository, Changes' working read and a reopen by
    /// the captured root each fail as `RepositoryGone`, naming it, and read
    /// nothing of it, the enclosing repository's working state included.
    #[test]
    fn a_reopen_finds_the_repository_it_opened_or_none() {
        let outer = Fixture::new();
        let inner = outer.0.join("inner");
        fs::create_dir(&inner).unwrap();
        let init = Fixture::command(&inner)
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        assert!(init.status.success());
        let mut session = RepositorySession::default();
        let opened = session.open(&inner, None, None).unwrap();
        let root = opened.path().to_owned();
        assert_eq!(session.open(&root, Some(&root), None).unwrap().path(), root);
        assert!(WorkingState::read(&opened).is_ok());

        fs::rename(inner.join(".git"), outer.0.join("inner-git-away")).unwrap();
        fs::write(outer.0.join("outer-change.txt"), "outer\n").unwrap();
        let outer_root = outer.0.canonicalize().unwrap();
        let gone = |error: anyhow::Error| {
            let gone = error
                .downcast::<RepositoryGone>()
                .expect("the repository is gone");
            assert_eq!(gone.path, root);
            assert_eq!(
                gone.found.map(|found| found.canonicalize().unwrap()),
                Some(outer_root.clone()),
                "it names the repository Git finds there"
            );
        };
        let quiet = execute(
            Job::QuietRefresh {
                repo: opened.clone(),
                scope: None,
                limit: 10,
                history: false,
                selected: None,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut session,
        );
        match quiet {
            Ok(_) => panic!("a quiet read read the enclosing repository"),
            Err(error) => gone(error),
        }
        match WorkingState::read(&opened) {
            Ok(_) => panic!("Changes read the enclosing repository"),
            Err(error) => gone(error),
        }
        gone(session.open(&root, Some(&root), None).unwrap_err());
        let error = execute(
            Job::Open {
                path: root.clone(),
                scope: None,
                limit: 10,
                root: Some(root.clone()),
                git_dir: None,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut session,
        )
        .err()
        .expect("Refresh fails");
        assert!(
            format!("{error:#}").starts_with(&format!(
                "The repository at {} is gone. Git now finds the repository at ",
                root.display()
            )),
            "said in its own words: {error:#}"
        );
        assert_eq!(
            session
                .open(&root, None, None)
                .unwrap()
                .path()
                .canonicalize()
                .unwrap(),
            outer_root,
            "an explicit open takes the enclosing work tree"
        );

        // With the enclosing repository's `.git` gone too, Git finds no
        // repository at the root: Refresh fails with the reasons any folder
        // that is not a repository gets, never "is gone", and the working
        // read still tells Changes the tab's repository is gone, with
        // nothing found, so it marks the tab through that Open.
        fs::rename(outer.0.join(".git"), outer.0.join("outer-git-away")).unwrap();
        let error = execute(
            Job::Open {
                path: root.clone(),
                scope: None,
                limit: 10,
                root: Some(root.clone()),
                git_dir: None,
            },
            &mut PreviewCache::default(),
            &active(),
            &mut session,
        )
        .err()
        .expect("Refresh fails");
        assert!(!error.is::<RepositoryGone>(), "not gone: {error:#}");
        let said = format!("{error:#}");
        assert!(
            said.starts_with("This folder is not an available Git repository.")
                && said.contains(&format!("Folder: {}", root.display())),
            "repository_access::explain's reasons: {said}"
        );
        match WorkingState::read(&opened) {
            Ok(_) => panic!("Changes read a folder that is not a repository"),
            Err(error) => assert_eq!(
                error.downcast::<RepositoryGone>().expect("gone"),
                RepositoryGone {
                    path: root.clone(),
                    found: None
                }
            ),
        }
    }

    /// A tab's repository is its root and the Git directory Git reported
    /// when it opened, compared as captured: with its `.git` swapped for
    /// another repository's gitfile, or its root turned into a link to
    /// another repository, Changes' working read, a quiet read and a
    /// reopen by the captured root each fail as `RepositoryGone`, naming
    /// what Git finds, and read nothing of the other repository; the
    /// session never adopts it. Restored, the tab reads again.
    #[cfg(unix)]
    #[test]
    fn a_reopen_never_takes_a_swapped_git_entry_or_a_linked_root() {
        let fixture = Fixture::new();
        let other = Fixture::new();
        let mut session = RepositorySession::default();
        let opened = session.open(&fixture.0, None, None).unwrap();
        let root = opened.path().to_owned();
        let other_repository = GitRepository::open(&other.0).unwrap();
        let gone = |error: anyhow::Error, found: &Path| {
            let gone = error
                .downcast::<RepositoryGone>()
                .expect("the repository is gone");
            assert_eq!(gone.path, root);
            assert_eq!(
                gone.found.map(|found| found.canonicalize().unwrap()),
                Some(found.to_owned()),
                "it names what Git finds there"
            );
        };
        let quiet = |session: &mut RepositorySession| {
            execute(
                Job::QuietRefresh {
                    repo: opened.clone(),
                    scope: None,
                    limit: 10,
                    history: false,
                    selected: None,
                },
                &mut PreviewCache::default(),
                &active(),
                session,
            )
            .err()
            .expect("a quiet read reads nothing")
        };

        let away = fixture.0.with_extension("git-away");
        fs::rename(fixture.0.join(".git"), &away).unwrap();
        fs::write(
            fixture.0.join(".git"),
            format!("gitdir: {}\n", other_repository.git_dir().display()),
        )
        .unwrap();
        let other_git = other_repository.git_dir();
        let captured = GitDirectories::of(&opened);
        gone(WorkingState::read(&opened).err().expect("gone"), other_git);
        gone(quiet(&mut session), other_git);
        // A reopen by the tab's captured root and Git directory, warm, and
        // cold after the session opened another repository: the session
        // keeps no memory of the tab, which names its own Git directory.
        gone(
            session
                .open(&root, Some(&root), Some(&captured))
                .unwrap_err(),
            other_git,
        );
        let third = Fixture::new();
        session.open(&third.0, None, None).unwrap();
        gone(
            session
                .open(&root, Some(&root), Some(&captured))
                .unwrap_err(),
            other_git,
        );
        fs::remove_file(fixture.0.join(".git")).unwrap();
        fs::rename(&away, fixture.0.join(".git")).unwrap();
        assert_eq!(
            session
                .open(&root, Some(&root), Some(&captured))
                .unwrap()
                .path(),
            root
        );
        assert!(WorkingState::read(&opened).is_ok());

        let moved = fixture.0.with_extension("moved");
        fs::rename(&fixture.0, &moved).unwrap();
        std::os::unix::fs::symlink(&other.0, &fixture.0).unwrap();
        let other_root = other_repository.path().canonicalize().unwrap();
        gone(
            WorkingState::read(&opened).err().expect("gone"),
            &other_root,
        );
        gone(quiet(&mut session), &other_root);
        gone(
            session.open(&root, Some(&root), None).unwrap_err(),
            &other_root,
        );
        fs::remove_file(&fixture.0).unwrap();
        fs::rename(&moved, &fixture.0).unwrap();
    }

    /// A tab's root reached through a folder that is a link, as macOS
    /// reaches every temporary folder through `/var` (a link to
    /// `/private/var`), is the repository there, warm and cold: saved
    /// through the link with no Git directories captured yet, and captured
    /// at its real root with its Git directories, its folder then moved and
    /// linked back from its old place. Compared as captured, each reopen
    /// found the repository gone and the tab was marked " · unavailable",
    /// where it opened before. The root itself turned into a link to
    /// another repository, reached through the linked folder, is still
    /// another repository.
    #[cfg(unix)]
    #[test]
    fn a_reopen_through_a_linked_folder_finds_the_repository_it_captured() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let real = base.join("real");
        let root = real.join("repository");
        fs::create_dir_all(&root).unwrap();
        let init = Fixture::command(&root)
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        assert!(init.status.success());
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let through = link.join("repository");
        let other = Fixture::new();
        let mut session = RepositorySession::default();
        let reopen = |session: &mut RepositorySession,
                      at: &Path,
                      git_dir: Option<&GitDirectories>,
                      cold: bool| {
            if cold {
                session.open(&other.0, None, None).unwrap();
            }
            session.open(at, Some(at), git_dir)
        };
        let opened = reopen(&mut session, &through, None, false)
            .unwrap_or_else(|error| panic!("saved through the link: {error:#}"));
        assert_eq!(opened.path(), root, "saved through the link");
        let captured = GitDirectories::of(&opened);
        for cold in [false, true] {
            let reopened = reopen(&mut session, &through, Some(&captured), cold)
                .unwrap_or_else(|error| panic!("through the link, cold {cold}: {error:#}"));
            assert_eq!(reopened.path(), root, "through the link, cold {cold}");
        }

        let moved = base.join("moved");
        fs::rename(&real, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &real).unwrap();
        let now = moved.join("repository");
        for cold in [false, true] {
            let reopened = reopen(&mut session, &root, Some(&captured), cold)
                .unwrap_or_else(|error| panic!("moved, cold {cold}: {error:#}"));
            assert_eq!(reopened.path(), now, "moved, cold {cold}");
        }

        fs::rename(&now, moved.join("repository-away")).unwrap();
        std::os::unix::fs::symlink(&other.0, &now).unwrap();
        let other_root = other.0.canonicalize().unwrap();
        for git_dir in [None, Some(&captured)] {
            let gone = reopen(&mut session, &through, git_dir, false)
                .expect_err("a root turned into a link is another repository")
                .downcast::<RepositoryGone>()
                .expect("gone");
            assert_eq!(gone.found.as_deref(), Some(other_root.as_path()));
        }
    }

    /// A folder made to hold one repository, `git init`ed.
    #[cfg(unix)]
    fn init_at(path: &Path) {
        fs::create_dir_all(path).unwrap();
        let init = Fixture::command(path)
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        assert!(init.status.success());
    }

    /// A restored tab compares the inodes it saved, warm and cold: a folder
    /// above its root re-pointed to another repository with the root's
    /// folder name resolves to the same paths through that folder, and is
    /// refused naming that repository's Git directory, the mark a swapped
    /// `.git` gets. Moved on the one filesystem and linked back, the
    /// repository keeps its inodes and reopens. A tab opened in this run
    /// compares none, and neither does an older session that saved none.
    #[cfg(unix)]
    #[test]
    fn a_restored_tab_tells_its_repository_by_the_inodes_it_saved() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let projects = base.join("projects");
        let root = projects.join("repository");
        init_at(&root);
        let other = Fixture::new();
        let mut session = RepositorySession::default();
        let opened = session.open(&root, None, None).unwrap();
        let captured = GitDirectories::of(&opened);
        assert!(!captured.restored, "of compares nothing");
        assert!(captured.inodes.git_dir.is_some());
        let restored = GitDirectories {
            restored: true,
            ..captured.clone()
        };
        let reopen = |session: &mut RepositorySession, named: &GitDirectories, cold: bool| {
            if cold {
                session.open(&other.0, None, None).unwrap();
            }
            session.open(&root, Some(&root), Some(named))
        };

        let moved = base.join("moved");
        fs::rename(&projects, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &projects).unwrap();
        for cold in [false, true] {
            let reopened = reopen(&mut session, &restored, cold)
                .unwrap_or_else(|error| panic!("moved, cold {cold}: {error:#}"));
            assert_eq!(reopened.path(), moved.join("repository"), "cold {cold}");
        }
        fs::remove_file(&projects).unwrap();
        fs::rename(&moved, &projects).unwrap();

        // The repository kept aside, so its inodes stay taken.
        let away = base.join("away");
        fs::rename(&projects, &away).unwrap();
        let elsewhere = base.join("elsewhere");
        let found = elsewhere.join("repository");
        init_at(&found);
        std::os::unix::fs::symlink(&elsewhere, &projects).unwrap();
        for cold in [false, true] {
            let gone = reopen(&mut session, &restored, cold)
                .expect_err("another repository by the root's folder name")
                .downcast::<RepositoryGone>()
                .expect("gone");
            assert_eq!(gone.path, root, "cold {cold}");
            assert_eq!(
                gone.found.as_deref(),
                Some(found.join(".git").as_path()),
                "cold {cold}"
            );
        }
        let older = GitDirectories {
            inodes: DirectoryInodes::default(),
            ..restored.clone()
        };
        for named in [&captured, &older] {
            assert_eq!(
                reopen(&mut session, named, true)
                    .unwrap_or_else(|error| panic!("{named:?}: {error:#}"))
                    .path(),
                found,
                "{named:?}"
            );
        }
    }

    /// Each saved inode is compared on its own, the Git directory's before
    /// the common directory's, and the retained repository answers only
    /// where they match: named with a different one, the reopen asks Git
    /// and fails as gone, naming that directory.
    #[cfg(unix)]
    #[test]
    fn a_restored_tab_compares_each_directory_s_inode() {
        let fixture = Fixture::new();
        let mut session = RepositorySession::default();
        let opened = session.open(&fixture.0, None, None).unwrap();
        let root = opened.path().to_owned();
        let captured = GitDirectories::of(&opened);
        let inode = |inode: Option<u64>| Some(inode.expect("an inode") + 1);
        let git_dir = GitDirectories {
            inodes: DirectoryInodes {
                git_dir: inode(captured.inodes.git_dir),
                common: captured.inodes.common,
            },
            restored: true,
            ..captured.clone()
        };
        let common = GitDirectories {
            inodes: DirectoryInodes {
                git_dir: captured.inodes.git_dir,
                common: inode(captured.inodes.common),
            },
            restored: true,
            ..captured.clone()
        };
        for (named, found) in [(&git_dir, &captured.git_dir), (&common, &captured.common)] {
            let gone = session
                .open(&root, Some(&root), Some(named))
                .unwrap_err()
                .downcast::<RepositoryGone>()
                .expect("gone");
            assert_eq!(gone.found.as_deref(), Some(found.as_path()));
        }
        let matching = GitDirectories {
            restored: true,
            ..captured
        };
        assert_eq!(
            session
                .open(&root, Some(&root), Some(&matching))
                .unwrap()
                .path(),
            root
        );
    }

    /// A repository cloned again at the tab's path has new inodes, which
    /// cannot be told from another repository re-pointed there, so a
    /// restored tab refuses it the same way; an explicit open has no saved
    /// fields and adopts it, and a tab that then holds its own passes.
    #[cfg(unix)]
    #[test]
    fn a_re_clone_is_refused_to_a_restored_tab_and_adopted_by_an_open() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let root = base.join("repository");
        init_at(&root);
        let mut session = RepositorySession::default();
        let captured = GitDirectories::of(&session.open(&root, None, None).unwrap());
        let restored = GitDirectories {
            restored: true,
            ..captured.clone()
        };
        // Moved aside rather than deleted, which could free the inode for
        // the new `.git` to take.
        fs::rename(&root, base.join("before")).unwrap();
        init_at(&root);
        let gone = session
            .open(&root, Some(&root), Some(&restored))
            .expect_err("a new repository at the path")
            .downcast::<RepositoryGone>()
            .expect("gone");
        assert_eq!(gone.found.as_deref(), Some(root.join(".git").as_path()));

        let adopted = GitDirectories::of(&session.open(&root, None, None).unwrap());
        assert_ne!(adopted.inodes.git_dir, captured.inodes.git_dir);
        assert_eq!(
            session
                .open(&root, Some(&root), Some(&adopted))
                .unwrap()
                .path(),
            root
        );
    }

    /// An open refused because only the capture failed, Git still
    /// reporting the repository at its path, says what is true in core's
    /// words: not gone, and the one path not named twice. One that Git now
    /// finds elsewhere is the repository gone, as before.
    #[test]
    fn an_open_refused_at_its_own_path_is_not_the_repository_gone() {
        let root = PathBuf::from("/repository");
        let refused = |found: Option<PathBuf>| {
            let error = anyhow::Error::from(gitturtle_core::ChangedWhileOpening {
                path: root.clone(),
                found,
            });
            worded_open_failure(&root, refused_open(&root, false, error))
        };
        let same = refused(Some(root.clone()));
        assert!(same.is::<gitturtle_core::ChangedWhileOpening>(), "{same:#}");
        assert_eq!(
            format!("{same:#}"),
            "The repository at /repository changed while it was opened. Open it again."
        );
        let elsewhere = refused(Some(PathBuf::from("/other")));
        assert!(elsewhere.is::<RepositoryGone>(), "{elsewhere:#}");
        assert!(
            format!("{elsewhere:#}").contains("Git now finds the repository at /other there"),
            "{elsewhere:#}"
        );
    }

    /// A repository whose work tree lies outside the folder Git found it
    /// in: an explicit open says so in core's own words, with no reasons
    /// around it and nothing to open again, and a tab's passive reopen,
    /// its work tree re-pointed there, is the repository gone, naming the
    /// work tree, so the tab is marked as it is for any re-point.
    #[test]
    fn a_work_tree_outside_its_folder_is_refused_in_core_s_words() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let root = base.join("repository");
        let elsewhere = base.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        fs::create_dir(&root).unwrap();
        let init = Fixture::command(&root)
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        assert!(init.status.success());
        let configured = Fixture::command(&root)
            .args(["config", "core.worktree"])
            .arg(&elsewhere)
            .output()
            .unwrap();
        assert!(configured.status.success());
        let said = format!(
            "GitTurtle opens a repository only inside the folder Git found it in: the work tree of the repository at {} is {} (core.worktree).",
            root.display(),
            elsewhere.display()
        );
        let mut session = RepositorySession::default();
        let explicit = session.open(&root, None, None).unwrap_err();
        assert!(
            explicit.is::<gitturtle_core::WorkTreeOutside>(),
            "{explicit:#}"
        );
        let open = |path_root: Option<PathBuf>, session: &mut RepositorySession| {
            execute(
                Job::Open {
                    path: root.clone(),
                    scope: None,
                    limit: 10,
                    root: path_root,
                    git_dir: None,
                },
                &mut PreviewCache::default(),
                &active(),
                session,
            )
            .err()
            .expect("the open is refused")
        };
        let error = open(None, &mut session);
        assert_eq!(format!("{error:#}"), said);
        assert!(!error.is::<RepositoryGone>(), "{error:#}");

        let reopened = session.open(&root, Some(&root), None).unwrap_err();
        assert_eq!(
            reopened.downcast::<RepositoryGone>().expect("gone"),
            RepositoryGone {
                path: root.clone(),
                found: Some(elsewhere.clone()),
            }
        );
        let error = open(Some(root.clone()), &mut session);
        assert!(error.is::<RepositoryGone>(), "marks the tab: {error:#}");
    }

    /// A linked worktree's `commondir` re-pointed at another repository
    /// keeps its root and its own Git directory, yet its refs and objects,
    /// and every write's, are that other repository's: a reopen by the
    /// tab's captured identity, warm or cold, finds the repository gone,
    /// naming that repository's Git directory, and with `commondir` back it
    /// opens again. It once compared the root and Git directory alone, so
    /// the tab took a handle bound to the other repository, and its next
    /// Stage all and Commit wrote that repository's objects and branch.
    #[test]
    fn a_reopen_never_takes_a_re_pointed_commondir() {
        let fixture = Fixture::new();
        fixture.git(&["commit", "--quiet", "--allow-empty", "-m", "main"], &[]);
        let linked = fixture.0.join("linked");
        fixture.git(
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
            &[],
        );
        let mut session = RepositorySession::default();
        let opened = session.open(&linked, None, None).unwrap();
        let root = opened.path().to_owned();
        let captured = GitDirectories::of(&opened);
        assert_ne!(captured.git_dir, captured.common, "a linked worktree");
        let other = Fixture::new();
        let other_git = GitRepository::open(&other.0).unwrap().git_dir().to_owned();
        let commondir = opened.git_dir().join("commondir");
        let kept = fs::read(&commondir).unwrap();
        fs::write(&commondir, format!("{}\n", other_git.display())).unwrap();
        let gone = |error: anyhow::Error| {
            let gone = error.downcast::<RepositoryGone>().expect("gone");
            assert_eq!(gone.path, root);
            assert_eq!(gone.found.as_deref(), Some(other_git.as_path()));
        };
        gone(
            session
                .open(&root, Some(&root), Some(&captured))
                .unwrap_err(),
        );
        let third = Fixture::new();
        session.open(&third.0, None, None).unwrap();
        gone(
            session
                .open(&root, Some(&root), Some(&captured))
                .unwrap_err(),
        );
        fs::write(&commondir, kept).unwrap();
        assert_eq!(
            session
                .open(&root, Some(&root), Some(&captured))
                .unwrap()
                .path(),
            root
        );
    }

    /// The retained repository answers a reopen only where its own Git
    /// directories are the ones the tab names: named with another common
    /// directory, the reopen asks Git, which finds the repository's own,
    /// and fails as gone rather than hand the retained repository over.
    #[test]
    fn a_retained_repository_answers_only_the_directories_it_has() {
        let fixture = Fixture::new();
        let mut session = RepositorySession::default();
        let opened = session.open(&fixture.0, None, None).unwrap();
        let root = opened.path().to_owned();
        let captured = GitDirectories::of(&opened);
        let named = GitDirectories {
            common: fixture.0.join("elsewhere"),
            ..captured.clone()
        };
        let gone = session
            .open(&root, Some(&root), Some(&named))
            .unwrap_err()
            .downcast::<RepositoryGone>()
            .expect("gone");
        assert_eq!(gone.found.as_deref(), Some(captured.common.as_path()));
        assert_eq!(
            session
                .open(&root, Some(&root), Some(&captured))
                .unwrap()
                .path(),
            root
        );
    }

    /// A quiet read with the `.git` swapped for another repository's
    /// gitfile reads nothing of that repository. And a quiet read already
    /// running when the `.git`
    /// moves away (the watcher's setup rescan racing the move) fails part
    /// way, its working read after the status and its History, confined to
    /// the repository: the whole read is the repository gone, which marks
    /// the tab, never an "Operation state: …" or "History scope changed: …"
    /// banner beside the mark.
    #[test]
    fn a_quiet_read_whose_git_moves_part_way_is_the_repository_gone() {
        let outer = Fixture::new();
        let inner = outer.0.join("inner");
        fs::create_dir(&inner).unwrap();
        for args in [
            &["init", "--quiet"][..],
            &[
                "-c",
                "user.name=GitTurtle Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "inner",
            ],
        ] {
            let output = Fixture::command(&inner).args(args).output().unwrap();
            assert!(output.status.success());
        }
        let mut session = RepositorySession::default();
        let opened = session.open(&inner, None, None).unwrap();
        let quiet = |session: &mut RepositorySession, history: bool| {
            execute(
                Job::QuietRefresh {
                    repo: opened.clone(),
                    scope: None,
                    limit: 10,
                    history,
                    selected: None,
                },
                &mut PreviewCache::default(),
                &active(),
                session,
            )
        };
        let gone = |result: Result<Output>| match result {
            Ok(_) => panic!("the read answered with another repository or a banner"),
            Err(error) => {
                let gone = error.downcast::<RepositoryGone>().expect("gone");
                assert_eq!(gone.path, opened.path());
            }
        };

        let other = Fixture::new();
        let other_git = GitRepository::open(&other.0).unwrap().git_dir().to_owned();
        let away = outer.0.join("inner-git-away");
        fs::rename(inner.join(".git"), &away).unwrap();
        fs::write(
            inner.join(".git"),
            format!("gitdir: {}\n", other_git.display()),
        )
        .unwrap();
        gone(quiet(&mut session, false));
        fs::remove_file(inner.join(".git")).unwrap();
        fs::rename(&away, inner.join(".git")).unwrap();
        assert!(quiet(&mut session, true).is_ok());

        let checkpoints = std::rc::Rc::new(std::cell::Cell::new(0));
        let (git, moved) = (inner.join(".git"), away.clone());
        let counted = std::rc::Rc::clone(&checkpoints);
        QUIET_CHECKPOINT.with_borrow_mut(|hook| {
            *hook = Some(Box::new(move || {
                counted.set(counted.get() + 1);
                // After the status, before the rest and History.
                if counted.get() == 2 {
                    fs::rename(&git, &moved).unwrap();
                }
            }))
        });
        let result = quiet(&mut session, true);
        QUIET_CHECKPOINT.with_borrow_mut(|hook| *hook = None);
        assert!(checkpoints.get() >= 2, "the move ran part way");
        gone(result);
    }

    /// A quiet read checks its own handle before the session's reopen:
    /// with `core.worktree` re-pointed in the repository's configuration
    /// after a quiet read, which changes no `.git` entry or directory the
    /// session's identity holds while the work tree moves elsewhere, the
    /// next quiet read is the repository gone, never the other folder's
    /// status. The session alone answered with the moved work tree.
    #[test]
    fn a_quiet_read_after_core_worktree_is_re_pointed_is_the_repository_gone() {
        let outer = Fixture::new();
        let inner = outer.0.join("inner");
        fs::create_dir(&inner).unwrap();
        let elsewhere = outer.0.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        fs::write(elsewhere.join("stray.txt"), "stray\n").unwrap();
        for args in [
            &["init", "--quiet"][..],
            &[
                "-c",
                "user.name=GitTurtle Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "inner",
            ],
        ] {
            let output = Fixture::command(&inner).args(args).output().unwrap();
            assert!(output.status.success());
        }
        let mut session = RepositorySession::default();
        let opened = session.open(&inner, None, None).unwrap();
        let quiet = |session: &mut RepositorySession| {
            execute(
                Job::QuietRefresh {
                    repo: opened.clone(),
                    scope: None,
                    limit: 10,
                    history: true,
                    selected: None,
                },
                &mut PreviewCache::default(),
                &active(),
                session,
            )
        };
        assert!(quiet(&mut session).is_ok());
        let output = Fixture::command(&inner)
            .args(["config", "core.worktree", elsewhere.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());
        let error = quiet(&mut session)
            .err()
            .expect("the quiet read answered with the moved work tree");
        let gone = error
            .downcast::<RepositoryGone>()
            .unwrap_or_else(|error| panic!("not gone: {error:#}"));
        assert_eq!(gone.path, opened.path());
    }

    /// A working read already running when the repository's `.git` moves
    /// away: its status read, the parts after it fail as "not a git
    /// repository", its commands confined to the repository. The read is
    /// then the repository gone, which marks the tab, never an "Operation
    /// state: …" banner beside the mark.
    #[test]
    fn a_working_read_whose_git_moves_part_way_is_the_repository_gone() {
        let outer = Fixture::new();
        let inner = outer.0.join("inner");
        fs::create_dir(&inner).unwrap();
        let init = Fixture::command(&inner)
            .args(["init", "--quiet"])
            .output()
            .unwrap();
        assert!(init.status.success());
        let opened = GitRepository::open(&inner).unwrap();
        let checkpoints = std::cell::Cell::new(0);
        let state = WorkingState::read_with_checkpoint(&opened, || {
            checkpoints.set(checkpoints.get() + 1);
            // After the status, before the rest.
            if checkpoints.get() == 2 {
                fs::rename(inner.join(".git"), outer.0.join("inner-git-away")).unwrap();
            }
            Ok(())
        })
        .unwrap();
        let operation = format!("{:#}", state.operation.as_ref().expect_err("failed"));
        assert!(operation.contains("not a git repository"), "{operation}");
        let gone = state
            .confirmed(&opened)
            .err()
            .expect("the repository is gone")
            .downcast::<RepositoryGone>()
            .expect("gone");
        assert_eq!(gone.path, opened.path());
        assert_eq!(
            gone.found.map(|found| found.canonicalize().unwrap()),
            Some(outer.0.canonicalize().unwrap())
        );
    }

    /// The watcher's setup failure is the repository gone whenever the
    /// repository is gone or no longer one at its path, whatever the words:
    /// a resolution failing as its `.git` moves is checked from the
    /// filesystem, and kept its own while the repository is still there; a
    /// confined "not a git repository" is Git's to decide, so a `.git` half
    /// removed (HEAD deleted, the folder and `config` left), which the
    /// filesystem check still takes for the same repository, is gone too.
    /// That check once kept it the watcher's warning, the tab unmarked.
    #[test]
    fn a_watch_setup_failure_is_the_repository_gone_unless_it_is_still_there() {
        let outer = Fixture::new();
        let outer_root = outer.0.canonicalize().unwrap();
        let repository = |name: &str| {
            let root = outer.0.join(name);
            fs::create_dir(&root).unwrap();
            let init = Fixture::command(&root)
                .args(["init", "--quiet"])
                .output()
                .unwrap();
            assert!(init.status.success());
            GitRepository::open(&root).unwrap()
        };
        let gone = |repo: &GitRepository, error: anyhow::Error, after: &str| {
            let gone = gone_unless_found(repo, error)
                .downcast::<RepositoryGone>()
                .unwrap_or_else(|error| panic!("{after}: not gone: {error:#}"));
            assert_eq!(gone.path, repo.path(), "{after}");
            assert_eq!(
                gone.found.map(|found| found.canonicalize().unwrap()),
                Some(outer_root.clone()),
                "{after}: it names the repository Git finds there"
            );
        };
        // As `git_directories` fails resolving a directory moved away.
        let resolution = || {
            use anyhow::Context as _;
            outer
                .0
                .join("nowhere")
                .canonicalize()
                .context("Resolve Git administration directory")
                .expect_err("nothing there")
        };

        let moved = repository("moved");
        let kept = gone_unless_found(&moved, resolution());
        assert!(!kept.is::<RepositoryGone>(), "still there: {kept:#}");
        assert_eq!(format!("{kept:#}"), format!("{:#}", resolution()));
        fs::rename(moved.path().join(".git"), outer.0.join("moved-git-away")).unwrap();
        gone(&moved, resolution(), "its .git moved away");

        let half = repository("half");
        fs::remove_file(half.git_dir().join("HEAD")).unwrap();
        assert!(half.git_dir().join("config").is_file());
        let confined = half
            .git_directories()
            .expect_err("Git finds no repository there");
        assert!(
            gitturtle_core::finds_no_repository(&confined),
            "{confined:#}"
        );
        assert_eq!(
            half.verify_at_path().unwrap(),
            gitturtle_core::FoundAtPath::Same,
            "the filesystem check still holds"
        );
        gone(&half, confined, "its HEAD deleted");
    }
}

/// Linux AVIF and JPEG 2000 sides through the confined helper's client, with
/// shell fakes named for each image's bytes (`image_helper::named`).
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) mod helper_image_tests {
    use crate::pdf_helper::HelperCommand;
    use crate::views::tests::{draw, history_window_from, settle};
    use crate::*;
    use core::prelude::v1::test;
    use gitturtle_preview::pdf_helper::PROTOCOL_VERSION;
    use std::path::Path;
    use std::time::{Duration, Instant};

    /// Longer than a test's drains wait: a helper still running when it
    /// should have been stopped fails the test rather than meet its deadline.
    const LONG: Duration = Duration::from_secs(60);

    /// A fake that never answers and ends with the test process.
    pub(crate) const BLOCKS: &str = "while kill -0 $PPID 2>/dev/null; do sleep 0.05; done";

    /// How soon a helper whose check failed is gone, counted once the call
    /// that failed it has returned: the client checks at least every 25 ms,
    /// and the bound stays far from a fake's deadline while it leaves a
    /// loaded host's scheduling room.
    pub(crate) const PROMPTLY: Duration = Duration::from_secs(1);

    const AVIF: &[u8] = include_bytes!("../../preview/tests/fixtures/half-red-blue.avif");
    const JP2: &[u8] = include_bytes!("../../preview/tests/fixtures/half-red-blue.jp2");
    const J2K: &[u8] = include_bytes!("../../preview/tests/fixtures/half-red-blue.j2k");
    const HEIC: &[u8] = include_bytes!("../../preview/tests/fixtures/half-red-blue.heic");

    /// `base` with `name` after it: bytes of the image's own, so a helper of
    /// its own, that still sniff as `base`'s format.
    fn named_bytes(base: &[u8], name: &str) -> Vec<u8> {
        [base, name.as_bytes()].concat()
    }

    /// From now on, `bytes` decode through a shell fake that writes its pid
    /// to `pid` and then runs `script`, stopped at `deadline`.
    fn name_helper(bytes: &[u8], pid: &Path, script: &str, deadline: Duration) {
        let script = format!(
            "PATH=/usr/bin:/bin; printf %s $$ > '{}'; {script}",
            pid.display()
        );
        crate::image_helper::named::name(
            bytes,
            HelperCommand::new("/bin/sh", ["-c", script.as_str()]),
            deadline,
        );
    }

    /// A script printing a 4 × 2 image reply that echoes format `code` with
    /// `flags`, kept as `name` in `directory`.
    fn image_reply(directory: &Path, name: &str, code: u8, flags: u16) -> String {
        let mut frame = b"GTIr".to_vec();
        frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        frame.extend_from_slice(&[0, code]);
        frame.extend_from_slice(&flags.to_le_bytes());
        for value in [4u32, 2, 4, 2, 1, 2, 3] {
            frame.extend_from_slice(&value.to_le_bytes());
        }
        frame.extend_from_slice(&[200; 4 * 2 * 4]);
        printing(directory, name, &frame)
    }

    /// A script printing an image error reply of wire `code`.
    fn error_reply(directory: &Path, name: &str, code: u8) -> String {
        let mut frame = b"GTIr".to_vec();
        frame.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        frame.extend_from_slice(&[1, code]);
        frame.extend_from_slice(&0u32.to_le_bytes());
        printing(directory, name, &frame)
    }

    /// A script printing `bytes`, kept as `name` in `directory`.
    fn printing(directory: &Path, name: &str, bytes: &[u8]) -> String {
        let path = directory.join(name);
        std::fs::write(&path, bytes).unwrap();
        format!("exec cat '{}'", path.display())
    }

    /// The pid a fake wrote, once it has.
    pub(crate) fn pid_in(path: &Path) -> libc::pid_t {
        let started = Instant::now();
        loop {
            if let Some(pid) = std::fs::read_to_string(path)
                .ok()
                .and_then(|pid| pid.parse().ok())
            {
                return pid;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the fake never started"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// The process is gone and reaped: not even a zombie is left.
    fn reaped(pid: libc::pid_t) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        let result = unsafe { libc::kill(pid, 0) };
        result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    /// Whether `pid` is reaped [`PROMPTLY`] after `since`, when the call
    /// that failed its check returned.
    pub(crate) fn reaped_within(pid: libc::pid_t, since: Instant) -> bool {
        while since.elapsed() < PROMPTLY {
            if reaped(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        reaped(pid)
    }

    /// A `git fast-import` stream of one commit adding `files`.
    fn added(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut input = b"commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 6\nimages\n".to_vec();
        for (path, bytes) in files {
            input.extend_from_slice(
                format!("M 100644 inline {path}\ndata {}\n", bytes.len()).as_bytes(),
            );
            input.extend_from_slice(bytes);
            input.push(b'\n');
        }
        input.extend_from_slice(b"done\n");
        input
    }

    /// Selects the commit's `path`, which enters Compare.
    fn select(app: &Entity<GitTurtle>, path: &str, cx: &mut VisualTestContext) {
        let index = app.read_with(cx, |app, _| {
            app.files
                .iter()
                .position(|file| file.path() == Path::new(path))
                .expect("the commit adds it")
        });
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(index, window, cx)));
    }

    /// Compares the commit's `path` once its preview has arrived.
    async fn compare(app: &Entity<GitTurtle>, path: &str, cx: &mut VisualTestContext) {
        select(app, path, cx);
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(app, cx).await;
        draw(cx);
    }

    /// Whether After's caption draws `details` beneath its name.
    fn after_says(details: &str, cx: &mut VisualTestContext) -> bool {
        let selector: &'static str = format!("image-side-details-1: {details}").leak();
        cx.debug_bounds(selector).is_some()
    }

    /// What After says in its image's place, read as static text, if it
    /// draws `reason` there whole.
    fn after_reason(reason: &str, cx: &mut VisualTestContext) -> Option<String> {
        crate::native_accessibility::written::forget();
        draw(cx);
        let selector: &'static str = format!("image-side-empty-1: {reason}").leak();
        cx.debug_bounds(selector)?;
        crate::native_accessibility::written::labels()
            .into_iter()
            .find(|(written, _)| written.starts_with("image-side-empty") && written.ends_with('1'))
            .and_then(|(_, name)| name)
    }

    /// Captions on Linux are macOS's without "macOS codec", built from the
    /// reply's format echo and flags: each flag adds its words, the first
    /// frame of an animation in place of the first image. Each side offers
    /// Open in default app.
    #[gpui::test]
    async fn helper_images_are_captioned_from_their_echo_and_flags(cx: &mut TestAppContext) {
        let replies = tempfile::tempdir().unwrap();
        let cases: [(&str, &[u8], u8, u16, &str); 7] = [
            ("still.avif", AVIF, 1, 0, "AVIF · first image"),
            (
                "animation.avif",
                AVIF,
                1,
                1,
                "AVIF · first frame of an animation",
            ),
            (
                "cropped.avif",
                AVIF,
                1,
                2,
                "AVIF · first image · crop not applied",
            ),
            (
                "hdr.avif",
                AVIF,
                1,
                4,
                "AVIF · first image · HDR not tone-mapped",
            ),
            (
                "profile.avif",
                AVIF,
                1,
                8,
                "AVIF · first image · color profile not applied",
            ),
            ("picture.jp2", JP2, 2, 0, "JPEG 2000 / JP2 · first image"),
            (
                "codestream.j2k",
                J2K,
                3,
                0,
                "JPEG 2000 / J2K codestream · first image",
            ),
        ];
        let images: Vec<Vec<u8>> = cases
            .iter()
            .map(|(path, base, ..)| named_bytes(base, path))
            .collect();
        for ((path, _, code, flags, _), bytes) in cases.iter().zip(&images) {
            let reply = image_reply(replies.path(), path, *code, *flags);
            let pid = replies.path().join(format!("{path}.pid"));
            name_helper(bytes, &pid, &reply, LONG);
        }
        let files: Vec<(&str, &[u8])> = cases
            .iter()
            .zip(&images)
            .map(|((path, ..), bytes)| (*path, bytes.as_slice()))
            .collect();
        let (_fixture, app, cx) = history_window_from(cx, added(&files)).await;
        cx.simulate_resize(size(px(1480.), px(973.)));
        for (path, _, _, _, caption) in cases {
            compare(&app, path, cx).await;
            let details = format!("4 × 2 · {caption}");
            assert!(after_says(&details, cx), "{path} draws {details:?}");
            assert!(
                cx.debug_bounds("image-system-preview-1").is_some(),
                "{path} offers Open in default app"
            );
        }
    }

    /// An image that fails: its path, the bytes it is made from, its fake's
    /// script and deadline (none for an image no helper is named for) and
    /// the sentence it says.
    type Failing<'a> = (&'a str, &'a [u8], Option<(&'a str, Duration)>, &'a str);

    /// Each helper failure and a document error say why where After's image
    /// would be, drawn and read as static text in the client's fixed words;
    /// HEIF/HEIC, which Linux never decodes, says so. Each side still offers
    /// Open in default app, and each helper is reaped.
    #[gpui::test]
    async fn helper_failures_say_why_and_keep_open_in_default_app(cx: &mut TestAppContext) {
        let fakes = tempfile::tempdir().unwrap();
        let short = Duration::from_millis(200);
        let grid = error_reply(fakes.path(), "grid", 10);
        let cases: [Failing<'_>; 6] = [
            (
                "slow.avif",
                AVIF,
                Some(("exec sleep 30", short)),
                "The image took longer than 10 seconds to decode and was stopped.",
            ),
            (
                "stopped.jp2",
                JP2,
                Some(("exit 3", LONG)),
                "The image decoder stopped on this image.",
            ),
            (
                "unreadable.j2k",
                J2K,
                Some(("exit 0", LONG)),
                "The image decoder returned an unreadable result.",
            ),
            // No helper is named for it, so its helper cannot start.
            (
                "unstartable.avif",
                AVIF,
                None,
                "The image decoder could not start.",
            ),
            (
                "grid.avif",
                AVIF,
                Some((grid.as_str(), LONG)),
                "AVIF image grids are not decoded on Linux yet",
            ),
            (
                "photo.heic",
                HEIC,
                None,
                "HEIF / HEIC images are not decoded on Linux",
            ),
        ];
        let images: Vec<Vec<u8>> = cases
            .iter()
            .map(|(path, base, ..)| named_bytes(base, path))
            .collect();
        for ((path, _, helper, _), bytes) in cases.iter().zip(&images) {
            if let Some((script, deadline)) = helper {
                let pid = fakes.path().join(format!("{path}.pid"));
                name_helper(bytes, &pid, script, *deadline);
            }
        }
        let files: Vec<(&str, &[u8])> = cases
            .iter()
            .zip(&images)
            .map(|((path, ..), bytes)| (*path, bytes.as_slice()))
            .collect();
        let (_fixture, app, cx) = history_window_from(cx, added(&files)).await;
        cx.simulate_resize(size(px(1480.), px(973.)));
        for (path, _, helper, sentence) in cases {
            compare(&app, path, cx).await;
            assert_eq!(
                after_reason(sentence, cx).as_deref(),
                Some(sentence),
                "{path} draws and reads its reason in its image's place"
            );
            assert!(
                cx.debug_bounds("image-system-preview-1").is_some(),
                "{path} offers Open in default app"
            );
            if helper.is_some() {
                let pid = pid_in(&fakes.path().join(format!("{path}.pid")));
                assert!(reaped(pid), "{path}'s helper is reaped");
            }
        }
    }

    /// A slow AVIF superseded by another file's selection: its helper is
    /// killed and reaped promptly, counted from the selection's return so
    /// the view's own work never counts, its late result never reaches the
    /// view, and Compare shows the file selected next.
    #[gpui::test]
    async fn another_selection_drops_a_slow_image_and_kills_its_helper(cx: &mut TestAppContext) {
        let fakes = tempfile::tempdir().unwrap();
        let slow = named_bytes(AVIF, "another selection");
        let started = fakes.path().join("slow");
        name_helper(&slow, &started, BLOCKS, LONG);
        let png = crate::system_preview::tests::png(None);
        let input = added(&[("figure.png", &png), ("slow.avif", &slow)]);
        let (_fixture, app, cx) = history_window_from(cx, &input).await;
        select(&app, "slow.avif", cx);
        let helper = pid_in(&started);
        select(&app, "figure.png", cx);
        assert!(
            reaped_within(helper, Instant::now()),
            "the slow image's helper is killed and reaped as the selection changes"
        );
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::Compare);
            let Some(Content::Images { new, .. }) = app.content.as_deref() else {
                panic!("the PNG's comparison is shown");
            };
            assert_eq!(
                new.image.as_ref().map(|image| image.format.as_str()),
                Some("PNG")
            );
        });
    }

    /// Back while a JP2 decodes: its helper is killed and reaped promptly
    /// after Back returns, nothing is left loading, and its late result
    /// never reopens Compare.
    #[gpui::test]
    async fn back_drops_a_slow_image_and_never_reopens_compare(cx: &mut TestAppContext) {
        let fakes = tempfile::tempdir().unwrap();
        let slow = named_bytes(JP2, "Back");
        let started = fakes.path().join("slow");
        name_helper(&slow, &started, BLOCKS, LONG);
        let (_fixture, app, cx) = history_window_from(cx, added(&[("slow.jp2", &slow)])).await;
        select(&app, "slow.jp2", cx);
        let helper = pid_in(&started);
        cx.update(|window, cx| app.update(cx, |app, cx| app.back_to_history(window, cx)));
        assert!(
            reaped_within(helper, Instant::now()),
            "the slow image's helper is killed and reaped on Back"
        );
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert!(app.mode == WorkspaceMode::History, "Compare stays closed");
            assert_eq!(app.loading, None);
            assert!(app.content.is_none(), "no comparison arrives");
        });
    }

    /// A switch to another repository tab while an AVIF decodes: its helper
    /// is killed and reaped promptly after the switch returns, and its late
    /// result reaches neither tab, so the tab shown stays in History.
    #[gpui::test]
    async fn a_tab_switch_drops_a_slow_image_and_kills_its_helper(cx: &mut TestAppContext) {
        let fakes = tempfile::tempdir().unwrap();
        let slow = named_bytes(AVIF, "a tab switch");
        let started = fakes.path().join("slow");
        name_helper(&slow, &started, BLOCKS, LONG);
        let (_fixture, app, cx) = history_window_from(cx, added(&[("slow.avif", &slow)])).await;
        let second = tempfile::tempdir().unwrap();
        let repo = crate::views::tests::fixture_repository(second.path().join("second"));
        std::fs::write(repo.path().join("notes.txt"), "second notes\n").unwrap();
        crate::tags::tests::git(repo.path(), &["add", "notes.txt"]);
        crate::tags::tests::git(repo.path(), &["commit", "--quiet", "-m", "second notes"]);
        let path = repo.path().to_owned();
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_repository_tab(path, window, cx)));
        settle(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(0, window, cx)));
        settle(&app, cx).await;
        select(&app, "slow.avif", cx);
        let helper = pid_in(&started);
        cx.update(|window, cx| app.update(cx, |app, cx| app.switch_repository_tab(1, window, cx)));
        assert!(
            reaped_within(helper, Instant::now()),
            "the slow image's helper is killed and reaped as the tab changes"
        );
        app.read_with(cx, |app, _| app.worker.wait_idle());
        settle(&app, cx).await;
        app.read_with(cx, |app, _| {
            assert_eq!(app.repository_tabs.active, Some(1));
            assert!(app.mode == WorkspaceMode::History, "Compare stays closed");
            assert!(app.content.is_none(), "no comparison arrives");
        });
    }
}
