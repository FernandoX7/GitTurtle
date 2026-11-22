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
pub(crate) use cache::PreviewKey;
use prepare::execute;
pub(crate) use prepare::render_image;
#[cfg(test)]
use prepare::{
    LfsSides, content_unchanged, image_change, layout_graph, prepared_text, read_snapshot,
    render_image_with_metadata, working_image_side,
};
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
