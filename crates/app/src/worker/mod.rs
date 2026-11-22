mod cache;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub(crate) mod helper_image_tests;
#[cfg(test)]
mod image_tests;
#[cfg(test)]
mod open_bench;
mod prepare;
mod repository_identity;
mod session;
#[cfg(test)]
mod snapshot_bench;
#[cfg(test)]
mod tests;
mod types;
#[cfg(test)]
mod working_bench;

use cache::PreviewCache;
pub(crate) use cache::{PreviewKey, TrackedPathCache};
#[cfg(test)]
use prepare::execute;
pub(crate) use prepare::render_image;
#[cfg(test)]
use prepare::{
    LfsSides, content_unchanged, image_change, layout_graph, prepared_text, read_snapshot,
    render_image_with_metadata, working_image_side,
};
use prepare::{execute_metadata, execute_preview};
use session::RepositorySession;
pub use session::{DirectoryInodes, GitDirectories, RepositoryGone};
pub(crate) use session::{MarkedReadFailed, gone_unless_found, with_current_mark_reason};
#[cfg(test)]
use session::{QUIET_CHECKPOINT, refused_open, worded_open_failure};
pub use types::*;

#[cfg(test)]
use crate::{graph, text::PatchPresentation};
use anyhow::{Result, anyhow, ensure};
use futures::channel::oneshot;
#[cfg(test)]
use gitturtle_core::{Commit, FileChange, GitRepository};
#[cfg(test)]
use gitturtle_preview::ImagePreview;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
};
#[cfg(test)]
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const PREVIEW_EDGE: u32 = 1600;
const CACHE_BYTE_LIMIT: usize = 128 * 1024 * 1024;
const CACHE_ENTRY_LIMIT: usize = 32;
const GRAPH_LANE_LIMIT: usize = 128;
const GRAPH_EDGE_LIMIT: usize = 200_000;

struct Request {
    job: Job,
    generation: u64,
    reply: oneshot::Sender<Result<Output>>,
    cancellation: gitturtle_core::HistoryCancellation,
}

#[derive(Default)]
struct LaneQueue {
    pending: Option<Request>,
    active: Option<Active>,
    /// Test-only: this lane's thread holds a request it has not finished,
    /// answered or skipped ([`Worker::wait_idle`]).
    #[cfg(test)]
    running: bool,
    /// Test-only: this lane's thread takes no request while held
    /// ([`Worker::hold`], [`Worker::hold_lane`]).
    #[cfg(test)]
    held: usize,
}

impl LaneQueue {
    /// Whether this lane's thread waits rather than taking its pending request.
    fn waits(&self) -> bool {
        #[cfg(test)]
        let held = self.held > 0;
        #[cfg(not(test))]
        let held = false;
        self.pending.is_none() || held
    }

    /// Supersedes the request this lane is running: its token is cancelled, so
    /// it stops at its next check and the Git processes it runs are terminated.
    /// A comparison preview's preparation inside a phase that checks nothing
    /// until it ends ([`Cancellation::unobserved`]) is only marked superseded
    /// and kept, with its token, so a request for the same preview can take it
    /// over ([`Worker::submit`]); a request that took it over earlier is no
    /// longer answered.
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

struct Queues {
    metadata: LaneQueue,
    preview: LaneQueue,
    /// Test-only single-lane mode used by [`Worker::with_executor`].
    single_lane: bool,
    preview_spawned: bool,
    preview_startup_error: Option<String>,
    release_search: bool,
    closed: bool,
    startup_error: Option<String>,
}

impl Queues {
    fn lane_mut(&mut self, lane: Lane) -> &mut LaneQueue {
        match lane {
            Lane::Metadata => &mut self.metadata,
            Lane::Preview => &mut self.preview,
        }
    }

    fn supersede_all(&mut self) {
        self.metadata.supersede_active();
        self.preview.supersede_active();
    }

    fn clear_pending_all(&mut self) {
        self.metadata.pending = None;
        self.preview.pending = None;
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

/// Keeps one or both of a [`Worker`]'s lanes from taking requests until it
/// drops ([`Worker::hold`], [`Worker::hold_lane`]).
#[cfg(test)]
#[must_use = "the reader is released as soon as the guard drops"]
pub struct ReaderHeld {
    queue: Arc<(Mutex<Queues>, Condvar)>,
    metadata: bool,
    preview: bool,
}

#[cfg(test)]
impl Drop for ReaderHeld {
    fn drop(&mut self) {
        let (lock, ready) = &*self.queue;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        if self.metadata {
            state.metadata.held = state.metadata.held.saturating_sub(1);
        }
        if self.preview {
            state.preview.held = state.preview.held.saturating_sub(1);
        }
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
    pub(super) fn check(&self) -> Result<()> {
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

type PreviewSpawner = Box<dyn FnOnce() -> std::io::Result<()> + Send>;

/// Bounded two-lane repository worker:
/// - **Metadata lane (`gitturtle-reader`)**: owns `RepositorySession` exclusively
///   (history, snapshots, commit changes, search, Quick Open paths, blame).
/// - **Preview lane (`gitturtle-preview`, lazy-spawned on first preview job)**:
///   owns `PreviewCache` exclusively (diffs, image/PDF/mesh/Markdown decoding,
///   text review, split-diff preparation).
///
/// Submitting any newer request supersedes active and pending work across both
/// lanes so slow in-process decoding never head-of-line blocks metadata reads,
/// while preserving in-flight preview takeover when returning to the same file.
/// Dropping the worker never waits for an active filesystem or decoder call.
pub struct Worker {
    queue: Arc<(Mutex<Queues>, Condvar)>,
    latest: Arc<AtomicU64>,
    preview_spawner: Mutex<Option<PreviewSpawner>>,
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
        Self::with_lanes(execute_metadata, execute_preview)
    }

    fn with_lanes(
        mut metadata_executor: impl FnMut(Job, &Cancellation, &mut RepositorySession) -> Result<Output>
        + Send
        + 'static,
        mut preview_executor: impl FnMut(Job, &mut PreviewCache, &Cancellation) -> Result<Output>
        + Send
        + 'static,
    ) -> Self {
        let queue = Arc::new((
            Mutex::new(Queues {
                metadata: LaneQueue::default(),
                preview: LaneQueue::default(),
                single_lane: false,
                preview_spawned: false,
                preview_startup_error: None,
                release_search: false,
                closed: false,
                startup_error: None,
            }),
            Condvar::new(),
        ));
        let latest = Arc::new(AtomicU64::new(0));
        #[cfg(test)]
        let preparations = Arc::new(AtomicU64::new(0));

        // Eagerly spawn the metadata lane (`gitturtle-reader`).
        let meta_queue = Arc::clone(&queue);
        let meta_latest = Arc::clone(&latest);
        let spawn = std::thread::Builder::new()
            .name("gitturtle-reader".into())
            .spawn(move || {
                let mut session = RepositorySession::default();
                loop {
                    let request = {
                        let (lock, ready) = &*meta_queue;
                        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                        #[cfg(test)]
                        {
                            state.metadata.running = false;
                            ready.notify_all();
                        }
                        while state.metadata.waits() && !state.release_search && !state.closed {
                            state = ready.wait(state).unwrap_or_else(|error| error.into_inner());
                        }
                        if state.closed {
                            break;
                        }
                        if state.release_search {
                            state.release_search = false;
                            #[cfg(test)]
                            {
                                state.metadata.running = true;
                            }
                            drop(state);
                            session.search = None;
                            continue;
                        }
                        let request = state
                            .metadata
                            .pending
                            .take()
                            .expect("pending metadata request checked above");
                        if request.reply.is_canceled()
                            && !matches!(request.job, Job::ReleaseHistory)
                        {
                            continue;
                        }
                        state.metadata.active = Some(Active::Read(request.cancellation.clone()));
                        #[cfg(test)]
                        {
                            state.metadata.running = true;
                        }
                        request
                    };
                    let cancellation = Cancellation {
                        generation: request.generation,
                        latest: Arc::clone(&meta_latest),
                        history: request.cancellation,
                        preparation: None,
                    };
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        cancellation.check()?;
                        metadata_executor(request.job, &cancellation, &mut session)
                    }));
                    let result = match result {
                        Ok(result) => result,
                        Err(_) => {
                            session = RepositorySession::default();
                            Err(anyhow!(
                                "Repository metadata read failed unexpectedly. Refresh or reopen the repository."
                            ))
                        }
                    };
                    {
                        let mut state = meta_queue
                            .0
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        state.metadata.active = None;
                    }
                    let _ = request.reply.send(result);
                }
            });
        if let Err(error) = spawn {
            let mut state = queue.0.lock().unwrap_or_else(|error| error.into_inner());
            state.closed = true;
            state.startup_error = Some(format!("Cannot start repository worker: {error}"));
        }

        // Lazy spawner for the preview/decoding lane (`gitturtle-preview`).
        let prev_queue = Arc::clone(&queue);
        let prev_latest = Arc::clone(&latest);
        #[cfg(test)]
        let prev_preparations = Arc::clone(&preparations);
        let preview_spawner: PreviewSpawner = Box::new(move || {
            std::thread::Builder::new()
                .name("gitturtle-preview".into())
                .spawn(move || {
                    let mut cache = PreviewCache::default();
                    loop {
                        let (request, preparation) = {
                            let (lock, ready) = &*prev_queue;
                            let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                            #[cfg(test)]
                            {
                                state.preview.running = false;
                                ready.notify_all();
                            }
                            while state.preview.waits() && !state.closed {
                                state =
                                    ready.wait(state).unwrap_or_else(|error| error.into_inner());
                            }
                            if state.closed {
                                break;
                            }
                            let request = state
                                .preview
                                .pending
                                .take()
                                .expect("pending preview request checked above");
                            if request.reply.is_canceled() {
                                continue;
                            }
                            let preparation = match PreviewKey::of(&request.job) {
                                Some(key) => {
                                    let preparation = Arc::new(Preparation::default());
                                    state.preview.active = Some(Active::Preview {
                                        key: Box::new(key),
                                        preparation: Arc::clone(&preparation),
                                        history: request.cancellation.clone(),
                                        adopted: None,
                                    });
                                    #[cfg(test)]
                                    prev_preparations.fetch_add(1, Ordering::AcqRel);
                                    Some(preparation)
                                }
                                None => {
                                    state.preview.active =
                                        Some(Active::Read(request.cancellation.clone()));
                                    None
                                }
                            };
                            #[cfg(test)]
                            {
                                state.preview.running = true;
                            }
                            (request, preparation)
                        };
                        let previewed = preparation.is_some();
                        let cancellation = Cancellation {
                            generation: request.generation,
                            latest: Arc::clone(&prev_latest),
                            history: request.cancellation,
                            preparation,
                        };
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            cancellation.check()?;
                            preview_executor(request.job, &mut cache, &cancellation)
                        }));
                        let result = match result {
                            Ok(result) => result,
                            Err(_) => {
                                // A failed decoder must not silently kill the
                                // preview lane or leave partial cache state.
                                cache = PreviewCache::default();
                                Err(anyhow!(
                                    "Preview processing failed unexpectedly. Select another file or reopen the repository."
                                ))
                            }
                        };
                        let reply = {
                            let mut state = prev_queue
                                .0
                                .lock()
                                .unwrap_or_else(|error| error.into_inner());
                            let active = state.preview.active.take();
                            if previewed {
                                match active {
                                    Some(Active::Preview {
                                        adopted: Some(adopted),
                                        ..
                                    }) => adopted,
                                    _ => request.reply,
                                }
                            } else {
                                request.reply
                            }
                        };
                        let _ = reply.send(result);
                    }
                })
                .map(drop)
        });

        Self {
            queue,
            latest,
            preview_spawner: Mutex::new(Some(preview_spawner)),
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

    #[cfg(test)]
    fn with_executor(
        mut executor: impl FnMut(Job, &mut PreviewCache, &Cancellation) -> Result<Output>
        + Send
        + 'static,
    ) -> Self {
        let queue = Arc::new((
            Mutex::new(Queues {
                metadata: LaneQueue::default(),
                preview: LaneQueue::default(),
                single_lane: true,
                preview_spawned: true,
                preview_startup_error: None,
                release_search: false,
                closed: false,
                startup_error: None,
            }),
            Condvar::new(),
        ));
        let latest = Arc::new(AtomicU64::new(0));
        let worker_queue = Arc::clone(&queue);
        let worker_latest = Arc::clone(&latest);
        let preparations = Arc::new(AtomicU64::new(0));
        let worker_preparations = Arc::clone(&preparations);
        let spawn = std::thread::Builder::new()
            .name("gitturtle-reader".into())
            .spawn(move || {
                let mut cache = PreviewCache::default();
                loop {
                    let (request, preparation) = {
                        let (lock, ready) = &*worker_queue;
                        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                        state.metadata.running = false;
                        ready.notify_all();
                        while state.metadata.waits() && !state.closed {
                            state = ready.wait(state).unwrap_or_else(|error| error.into_inner());
                        }
                        if state.closed {
                            break;
                        }
                        let request = state
                            .metadata
                            .pending
                            .take()
                            .expect("pending request checked above");
                        if request.reply.is_canceled()
                            && !matches!(request.job, Job::ReleaseHistory)
                        {
                            continue;
                        }
                        let preparation = match PreviewKey::of(&request.job) {
                            Some(key) => {
                                let preparation = Arc::new(Preparation::default());
                                state.metadata.active = Some(Active::Preview {
                                    key: Box::new(key),
                                    preparation: Arc::clone(&preparation),
                                    history: request.cancellation.clone(),
                                    adopted: None,
                                });
                                worker_preparations.fetch_add(1, Ordering::AcqRel);
                                Some(preparation)
                            }
                            None => {
                                state.metadata.active =
                                    Some(Active::Read(request.cancellation.clone()));
                                None
                            }
                        };
                        state.metadata.running = true;
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
                            cache = PreviewCache::default();
                            Err(anyhow!(
                                "Preview processing failed unexpectedly. Select another file or reopen the repository."
                            ))
                        }
                    };
                    let reply = {
                        let mut state = worker_queue
                            .0
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        let active = state.metadata.active.take();
                        if previewed {
                            match active {
                                Some(Active::Preview {
                                    adopted: Some(adopted),
                                    ..
                                }) => adopted,
                                _ => request.reply,
                            }
                        } else {
                            request.reply
                        }
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
            preview_spawner: Mutex::new(None),
            submissions: AtomicU64::new(0),
            working_previews: AtomicU64::new(0),
            previews: AtomicU64::new(0),
            tracked_previews: AtomicU64::new(0),
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

    /// Test-only: keeps both worker lanes from taking requests until the
    /// returned guard drops, as a slow read ahead of them would, so a request
    /// submitted meanwhile stays pending, where a newer one or
    /// [`Self::cancel`] drops it unanswered.
    #[cfg(test)]
    pub fn hold(&self) -> ReaderHeld {
        let mut state = self
            .queue
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.metadata.held += 1;
        state.preview.held += 1;
        ReaderHeld {
            queue: Arc::clone(&self.queue),
            metadata: true,
            preview: true,
        }
    }

    /// Test-only: holds only one specific lane so concurrency tests can keep
    /// one lane paused while verifying the other executes freely.
    #[cfg(test)]
    pub(crate) fn hold_lane(&self, lane: Lane) -> ReaderHeld {
        let mut state = self
            .queue
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.lane_mut(lane).held += 1;
        ReaderHeld {
            queue: Arc::clone(&self.queue),
            metadata: lane == Lane::Metadata,
            preview: lane == Lane::Preview,
        }
    }

    /// Test-only: blocks until both worker lanes have answered or skipped
    /// every request submitted so far, holding the test clock: it waits on
    /// the reader threads, which `run_until_parked` cannot wait for,
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
                (state.metadata.pending.is_some()
                    || state.metadata.running
                    || state.preview.pending.is_some()
                    || state.preview.running
                    || state.release_search)
                    && !state.closed
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
        state.supersede_all();
        state.clear_pending_all();
    }

    /// Closing an inactive history stream must not join Git on the UI thread.
    /// A subsequent Open also replaces it if this queued release is superseded.
    pub fn release_history(&self) {
        drop(self.submit(Job::ReleaseHistory));
    }

    /// Signals the metadata lane to drop any live `HistorySearch` stream off
    /// the UI thread without perturbing active generations or pending jobs.
    pub fn release_search(&self) {
        let (lock, ready) = &*self.queue;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        if !state.closed && !state.single_lane {
            state.release_search = true;
            ready.notify_all();
        }
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
        let spawn_preview = {
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
            let single_lane = state.single_lane;
            let target_lane = if single_lane {
                Lane::Metadata
            } else {
                job.lane()
            };
            if target_lane == Lane::Preview
                && let Some(error) = state.preview_startup_error.as_deref()
            {
                let _ = reply.send(Err(anyhow!("{error}")));
                return receiver;
            }
            // Allocate the sequence while holding the queue lock, so concurrent
            // submitters cannot replace a newer request with an older sequence.
            let generation = self.latest.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
            // The same comparison preview, still decoding its image, whatever
            // was asked for since: that preparation answers this request, so a
            // file chosen again has its bytes decoded once. Nothing waits behind
            // it any more.
            let preview_lane = if single_lane {
                &mut state.metadata
            } else {
                &mut state.preview
            };
            if let Some(Active::Preview {
                key,
                preparation,
                adopted,
                ..
            }) = &mut preview_lane.active
                && PreviewKey::of(&job).is_some_and(|wanted| wanted == **key)
                && preparation.take_over()
            {
                *adopted = Some(reply);
                if !single_lane {
                    state.metadata.supersede_active();
                }
                state.clear_pending_all();
                ready.notify_all();
                return receiver;
            }
            state.supersede_all();
            state.clear_pending_all();
            state.lane_mut(target_lane).pending = Some(Request {
                job,
                generation,
                reply,
                cancellation: gitturtle_core::HistoryCancellation::default(),
            });
            let need_spawn = target_lane == Lane::Preview && !state.preview_spawned;
            if need_spawn {
                state.preview_spawned = true;
            }
            // All: a test waiting for the reader to settle (`wait_idle`) waits
            // on this condition too, and must not take a lane's wake-up.
            ready.notify_all();
            need_spawn
        };
        if spawn_preview {
            let spawner = self
                .preview_spawner
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
            if let Some(spawn) = spawner
                && let Err(error) = spawn()
            {
                let message = format!("Cannot start preview worker: {error}");
                let (lock, ready) = &*self.queue;
                let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
                state.preview_startup_error = Some(message.clone());
                if let Some(pending) = state.preview.pending.take() {
                    let _ = pending.reply.send(Err(anyhow!("{message}")));
                }
                ready.notify_all();
            }
        }
        receiver
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, ready) = &*self.queue;
        let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
        self.latest.fetch_add(1, Ordering::AcqRel);
        state.supersede_all();
        state.closed = true;
        state.release_search = false;
        state.clear_pending_all();
        ready.notify_all();
    }
}
