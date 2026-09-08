//! Explicit operations survive UI selection changes. They are serialized and
//! never enter the replaceable history/preview queue.
use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc,
};

type Operation = Box<dyn FnOnce(bool) + Send>;

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
