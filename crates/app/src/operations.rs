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
}

impl SerialExecutor {
    pub fn new(name: &str) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Operation>(8);
        let _ = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                for operation in receiver {
                    operation(true);
                }
            });
        Self { sender }
    }

    pub fn submit<T: Send + 'static>(
        &self,
        operation: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> oneshot::Receiver<Result<T>> {
        self.submit_impl(operation, false)
    }

    /// Superseded passive status reads may be skipped before they start. Explicit
    /// writes always use submit() and survive a dropped UI reply.
    pub fn submit_read<T: Send + 'static>(
        &self,
        operation: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> oneshot::Receiver<Result<T>> {
        self.submit_impl(operation, true)
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
}
