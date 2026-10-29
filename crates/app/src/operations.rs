//! Explicit operations survive UI selection changes. They are serialized and
//! never enter the replaceable history/preview queue.
use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::mpsc,
};

type Operation = Box<dyn FnOnce(bool) + Send>;

/// How long a test waits on a thread of the app's own, an executor answering
/// ([`SerialExecutor::drain`]) or the repository reader going idle
/// (`Worker::wait_idle`), before it fails rather than hangs: a job that never
/// answers, or one the test itself holds, once stopped the test for good.
/// [`TEST_WAIT_FLOOR`], or longer where `GITTURTLE_TEST_WAIT_SECS` asks for
/// more, as a slow or heavily loaded host may; never shorter.
#[cfg(test)]
pub(crate) fn test_wait() -> std::time::Duration {
    test_wait_from(std::env::var("GITTURTLE_TEST_WAIT_SECS").ok().as_deref())
}

/// The shortest [`test_wait`]: the variable can only raise it, so a run
/// cannot turn a slow answer into a failure.
#[cfg(test)]
const TEST_WAIT_FLOOR: std::time::Duration = std::time::Duration::from_secs(30);

/// [`test_wait`] for the variable's value, if set: whole seconds above the
/// floor raise it; anything else leaves the floor.
#[cfg(test)]
fn test_wait_from(seconds: Option<&str>) -> std::time::Duration {
    seconds
        .and_then(|seconds| seconds.trim().parse::<u64>().ok())
        .map(std::time::Duration::from_secs)
        .map_or(TEST_WAIT_FLOOR, |asked| asked.max(TEST_WAIT_FLOOR))
}

/// Keep outcome ownership tied to the last successfully resolved worktree,
/// rather than the requested path (which may be an alias or fail to open).
#[derive(Default)]
pub(super) struct RepositoryOutcomes {
    repository: Option<PathBuf>,
}

pub(super) struct OpeningOutcomes {
    repository: Option<PathBuf>,
    error: Option<String>,
    notice: Option<String>,
}

impl RepositoryOutcomes {
    pub(super) fn begin_open(
        &self,
        error: &Option<String>,
        notice: &Option<String>,
    ) -> OpeningOutcomes {
        OpeningOutcomes {
            repository: self.repository.clone(),
            error: error.clone(),
            notice: notice.clone(),
        }
    }

    /// Apply only with a successful, generation-accepted repository snapshot.
    /// Create/Clone can publish their new notice after dispatching Open, so an
    /// outcome that replaced the captured one must survive this completion.
    pub(super) fn opened(
        &mut self,
        repository: &Path,
        opening: OpeningOutcomes,
        error: &mut Option<String>,
        notice: &mut Option<String>,
    ) {
        if opening.repository.as_deref() != Some(repository) {
            if *error == opening.error {
                *error = None;
            }
            if *notice == opening.notice {
                *notice = None;
            }
        }
        self.repository = Some(repository.to_owned());
    }
}

pub struct SerialExecutor {
    sender: mpsc::SyncSender<Operation>,
    /// The thread's name, which a test's failed wait reports.
    #[cfg(test)]
    name: String,
}

/// Keeps a [`SerialExecutor`] held until it drops ([`SerialExecutor::hold`]).
#[cfg(test)]
#[must_use = "the executor is released as soon as the guard drops"]
pub struct Held {
    _release: mpsc::Sender<()>,
}

impl SerialExecutor {
    pub fn new(name: &str) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Operation>(8);
        #[cfg(test)]
        let test_settings = crate::preferences::test_settings_directory();
        let _ = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                // The worker retains this test's storage until accepted jobs
                // drain, even if the owner test thread has already exited.
                #[cfg(test)]
                crate::preferences::inherit_test_settings_directory(test_settings);
                for operation in receiver {
                    operation(true);
                }
            });
        Self {
            sender,
            #[cfg(test)]
            name: name.into(),
        }
    }

    pub fn submit<T: Send + 'static>(
        &self,
        operation: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> oneshot::Receiver<Result<T>> {
        self.submit_impl(operation, false)
    }

    /// Attach explicit cancellation and in-memory askpass to one accepted write.
    pub fn submit_controlled<T: Send + 'static>(
        &self,
        control: gitturtle_core::OperationControl,
        operation: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> oneshot::Receiver<Result<T>> {
        self.submit(move || gitturtle_core::run_controlled(control, operation))
    }

    /// Superseded passive status reads may be skipped before they start. Explicit
    /// writes always use submit() and survive a dropped UI reply.
    pub fn submit_read<T: Send + 'static>(
        &self,
        operation: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> oneshot::Receiver<Result<T>> {
        self.submit_impl(operation, true)
    }

    /// Block until every job submitted before this call has answered. The
    /// queue is FIFO, so a marker queued now runs after all of them.
    /// `run_until_parked` cannot wait for this thread, so a GPUI test drains
    /// first and then settles to handle the replies on its own thread. A job
    /// that has not answered within [`test_wait`], such as one the test
    /// itself holds, fails the test at the caller's line.
    #[cfg(test)]
    #[track_caller]
    pub fn drain(&self) {
        self.drain_within(test_wait());
    }

    /// [`drain`](Self::drain), failing after `limit` rather than
    /// [`test_wait`]. The queue is bounded, so a queue that a held job keeps
    /// full takes no marker either: the wait for room counts too.
    #[cfg(test)]
    #[track_caller]
    pub fn drain_within(&self, limit: std::time::Duration) {
        let deadline = std::time::Instant::now() + limit;
        let (done, drained) = mpsc::channel();
        let mut marker: Operation = Box::new(move |_| {
            let _ = done.send(());
        });
        loop {
            match self.sender.try_send(marker) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Full(refused)) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "the {} executor's queue stayed full for {limit:?}: a job ahead of the drain never answered or is still held",
                        self.name
                    );
                    marker = refused;
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    panic!("the {} executor stopped before the drain", self.name)
                }
            }
        }
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match drained.recv_timeout(left) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => panic!(
                "the {} executor did not reach the drain within {limit:?}: a job ahead of it never answered or is still held",
                self.name
            ),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("the {} executor stopped before the drain", self.name)
            }
        }
    }

    /// Holds this executor's thread until the returned guard drops, as a
    /// slow job would: whatever is submitted meanwhile waits behind it. A
    /// GPUI test holds a dialog's read this way to act on its loading row
    /// before the reply lands.
    #[cfg(test)]
    pub fn hold(&self) -> Held {
        let (release, released) = mpsc::channel::<()>();
        self.sender
            .send(Box::new(move |_| {
                // Dropping the guard closes the channel, which ends the wait.
                let _ = released.recv();
            }))
            .expect("the executor accepts the hold");
        Held { _release: release }
    }

    fn submit_impl<T: Send + 'static>(
        &self,
        operation: impl FnOnce() -> Result<T> + Send + 'static,
        skip_cancelled: bool,
    ) -> oneshot::Receiver<Result<T>> {
        let (reply, response) = oneshot::channel();
        let job: Operation = Box::new(move |accepted| {
            if skip_cancelled && reply.is_canceled() {
                return;
            }
            let result = if accepted {
                catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| {
                    Err(anyhow!("Operation interrupted unexpectedly. Refresh the repository before retrying; it may have changed."))
                })
            } else {
                Err(anyhow!(
                    "Background executor is busy or unavailable. Try again after the current operation finishes."
                ))
            };
            let _ = reply.send(result);
        });
        if let Err(error) = self.sender.try_send(job) {
            match error {
                mpsc::TrySendError::Full(job) | mpsc::TrySendError::Disconnected(job) => job(false),
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_settings_are_inherited_by_workers_and_isolated_between_owners() {
        use crate::github::{NewPull, Repository, drafts};

        let (saved, ready) = mpsc::channel();
        let paths = std::thread::scope(|scope| {
            let mut releases = Vec::new();
            let owners: Vec<_> = ["first owner's draft", "second owner's draft"]
                .into_iter()
                .map(|body| {
                    let saved = saved.clone();
                    let (release, gate) = mpsc::channel();
                    releases.push(release);
                    scope.spawn(move || {
                        let settings = crate::preferences::settings_path().unwrap();
                        let writer = SerialExecutor::new("test-settings-writer");
                        let reader = SerialExecutor::new("test-settings-reader");
                        let expected = settings.clone();
                        let result = futures::executor::block_on(writer.submit(move || {
                            assert_eq!(crate::preferences::settings_path()?, expected);
                            drafts::save(drafts::Draft::Pull(NewPull {
                                repository: Repository::parse("fixture/shared-key")?,
                                title: "Same draft identity".into(),
                                body: body.into(),
                                head: "feature".into(),
                                base: "main".into(),
                                draft: true,
                                expected_head: None,
                                expected_base: None,
                            }))
                        }));
                        // Both stores have finished replacing their contents.
                        // A shared default directory loses one owner's text.
                        saved.send(()).unwrap();
                        gate.recv_timeout(std::time::Duration::from_secs(5))
                            .unwrap();
                        result.unwrap().unwrap();
                        let loaded = futures::executor::block_on(reader.submit(drafts::load))
                            .unwrap()
                            .unwrap();
                        assert_eq!(loaded.len(), 1);
                        assert_eq!(loaded[0].text(), body);
                        settings
                    })
                })
                .collect();
            for _ in &owners {
                ready
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            }
            for release in releases {
                release.send(()).unwrap();
            }
            owners
                .into_iter()
                .map(|owner| owner.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_ne!(paths[0], paths[1]);
    }

    #[test]
    fn test_settings_outlive_owner_until_accepted_jobs_drain() {
        let (settings, directory, release, held, queued) = std::thread::spawn(|| {
            let settings = crate::preferences::settings_path().unwrap();
            let directory = Arc::downgrade(&crate::preferences::test_settings_directory());
            let executor = SerialExecutor::new("test-settings-owner-exit");
            let (release, gate) = mpsc::channel();
            let (started, running) = mpsc::channel();
            let held = executor.submit(move || {
                started.send(())?;
                gate.recv_timeout(std::time::Duration::from_secs(5))?;
                Ok(())
            });
            running
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            let expected = settings.clone();
            let queued = executor.submit(move || {
                let settings = crate::preferences::settings_path()?;
                assert_eq!(settings, expected);
                crate::preferences::atomic_write(&settings, b"queued after owner exit")?;
                Ok(std::fs::read(settings)?)
            });
            (settings, directory, release, held, queued)
        })
        .join()
        .unwrap();
        // Both the owner's thread-local context and its executor are dropped;
        // the blocked worker must still retain storage for the accepted write.
        assert!(directory.upgrade().is_some());
        assert!(settings.parent().unwrap().is_dir());
        release.send(()).unwrap();
        futures::executor::block_on(held).unwrap().unwrap();
        assert_eq!(
            futures::executor::block_on(queued).unwrap().unwrap(),
            b"queued after owner exit"
        );
    }

    #[test]
    fn successful_repository_switch_clears_previous_outcomes_without_resurrecting_them() {
        let first = Path::new("fixture/main-worktree");
        let second = Path::new("fixture/linked-worktree");
        let mut ownership = RepositoryOutcomes::default();
        let (mut error, mut notice) = (None, None);
        let opening = ownership.begin_open(&error, &notice);
        ownership.opened(first, opening, &mut error, &mut notice);
        error = Some("Stash restoration produced conflicts. The stash remains saved.".into());
        notice = Some("Earlier operation in the first worktree".into());

        let opening = ownership.begin_open(&error, &notice);
        ownership.opened(second, opening, &mut error, &mut notice);
        assert_eq!((error.as_deref(), notice.as_deref()), (None, None));

        let opening = ownership.begin_open(&error, &notice);
        ownership.opened(first, opening, &mut error, &mut notice);
        assert_eq!((error, notice), (None, None));
    }

    #[test]
    fn failed_open_and_same_resolved_worktree_retain_existing_outcomes() {
        let repository = Path::new("fixture/canonical-worktree");
        let mut ownership = RepositoryOutcomes::default();
        let (mut error, mut notice) = (None, None);
        let opening = ownership.begin_open(&error, &notice);
        ownership.opened(repository, opening, &mut error, &mut notice);
        error = Some("Hook refused the commit".into());
        notice = Some("Resolution draft retained".into());

        // A failed or cancelled read never applies its pending outcome reset.
        drop(ownership.begin_open(&error, &notice));
        let reopening = ownership.begin_open(&error, &notice);
        // Discovery can resolve a nested or aliased requested path to this same root.
        ownership.opened(repository, reopening, &mut error, &mut notice);
        assert_eq!(error.as_deref(), Some("Hook refused the commit"));
        assert_eq!(notice.as_deref(), Some("Resolution draft retained"));
    }

    #[test]
    fn opening_preserves_new_completion_notice_and_errors_then_scopes_them() {
        let mut ownership = RepositoryOutcomes::default();
        let mut error = Some("Previous project failed".into());
        let mut notice = None;
        let opening = ownership.begin_open(&error, &notice);
        notice = Some("Repository cloned · new-project".into());
        error = Some("Could not save settings: disk unavailable".into());
        ownership.opened(
            Path::new("fixture/new-project"),
            opening,
            &mut error,
            &mut notice,
        );
        assert_eq!(notice.as_deref(), Some("Repository cloned · new-project"));
        assert_eq!(
            error.as_deref(),
            Some("Could not save settings: disk unavailable")
        );

        let opening = ownership.begin_open(&error, &notice);
        ownership.opened(
            Path::new("fixture/another-project"),
            opening,
            &mut error,
            &mut notice,
        );
        assert_eq!((error, notice), (None, None));
    }

    #[test]
    fn accepted_operations_keep_order_after_a_reply_is_dropped() {
        let executor = SerialExecutor::new("operation-test");
        let values = Arc::new(Mutex::new(Vec::new()));
        let (release, gate) = mpsc::channel();
        let first_values = Arc::clone(&values);
        let first = executor.submit(move || {
            gate.recv()?;
            first_values.lock().unwrap().push(1);
            Ok(())
        });
        drop(first);
        let last_values = Arc::clone(&values);
        let last = executor.submit(move || {
            last_values.lock().unwrap().push(2);
            Ok(())
        });
        release.send(()).unwrap();
        futures::executor::block_on(last).unwrap().unwrap();
        assert_eq!(*values.lock().unwrap(), [1, 2]);
    }

    #[test]
    fn obsolete_status_reads_are_skipped_without_discarding_a_queued_write() {
        let executor = SerialExecutor::new("operation-status-test");
        let (release, gate) = mpsc::channel();
        let first = executor.submit(move || {
            gate.recv()?;
            Ok(())
        });
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let obsolete_ran = Arc::clone(&ran);
        let obsolete = executor.submit_read(move || {
            obsolete_ran.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        drop(obsolete);
        let write = executor.submit(|| Ok("write executed"));
        release.send(()).unwrap();
        futures::executor::block_on(first).unwrap().unwrap();
        assert_eq!(
            futures::executor::block_on(write).unwrap().unwrap(),
            "write executed"
        );
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn a_panicking_operation_reports_uncertainty_and_executor_recovers() {
        let executor = SerialExecutor::new("operation-panic-test");
        let failed = executor.submit(|| -> Result<()> { panic!("fixture failure") });
        assert!(
            futures::executor::block_on(failed)
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("may have changed")
        );
        assert_eq!(
            futures::executor::block_on(executor.submit(|| Ok(42)))
                .unwrap()
                .unwrap(),
            42
        );
    }

    /// A drain behind a job that never answers fails, naming the executor,
    /// rather than waiting for good: a theme editor test once drained the
    /// preference writer while a job of its own still held it.
    #[test]
    #[should_panic(expected = "the drain-held-test executor did not reach the drain")]
    fn a_drain_behind_a_held_job_fails_rather_than_hangs() {
        let executor = SerialExecutor::new("drain-held-test");
        let _held = executor.hold();
        executor.drain_within(std::time::Duration::from_millis(50));
    }

    /// The queue is bounded, so a drain behind a held job and a full queue
    /// finds no room for its marker; that wait fails too, as it did when the
    /// theme editor's full-executor import stayed pending.
    #[test]
    #[should_panic(expected = "the drain-full-test executor's queue stayed full")]
    fn a_drain_behind_a_full_held_queue_fails_rather_than_hangs() {
        let executor = SerialExecutor::new("drain-full-test");
        // The held job is running, off the queue, before its eight slots
        // are taken.
        let (started, running) = mpsc::channel();
        let (_release, held) = mpsc::channel::<()>();
        let _running = executor.submit(move || {
            started.send(()).unwrap();
            let _ = held.recv();
            Ok(())
        });
        running.recv().unwrap();
        let _queued: Vec<_> = (0..8).map(|_| executor.submit(|| Ok(()))).collect();
        executor.drain_within(std::time::Duration::from_millis(50));
    }

    /// `GITTURTLE_TEST_WAIT_SECS` raises the waits' bound on a slow host
    /// and never lowers it; a value that is not whole seconds leaves it.
    #[test]
    fn the_test_wait_variable_only_raises_the_bound() {
        let floor = std::time::Duration::from_secs(30);
        assert_eq!(test_wait_from(None), floor);
        assert_eq!(
            test_wait_from(Some("120")),
            std::time::Duration::from_secs(120)
        );
        assert_eq!(
            test_wait_from(Some(" 45 ")),
            std::time::Duration::from_secs(45)
        );
        assert_eq!(test_wait_from(Some("5")), floor, "never lower");
        assert_eq!(test_wait_from(Some("0")), floor);
        assert_eq!(test_wait_from(Some("1.5")), floor);
        assert_eq!(test_wait_from(Some("-1")), floor);
        assert_eq!(test_wait_from(Some("")), floor);
    }
}
