//! Coalesce typing into one accepted preference job. The job owns pending
//! snapshots, so navigation or a dropped UI reply cannot discard a draft save.

use crate::{
    operations::SerialExecutor,
    preferences::{CommitDraft, Preferences},
};
use anyhow::Result;
use futures::channel::oneshot;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

type Drafts = HashMap<PathBuf, CommitDraft>;

#[derive(Default)]
struct Pending {
    drafts: Drafts,
    scheduled: bool,
}

#[derive(Default)]
pub struct DraftSaver {
    pending: Arc<Mutex<Pending>>,
}

impl DraftSaver {
    pub fn queue(
        &self,
        executor: &SerialExecutor,
        worktree: PathBuf,
        draft: CommitDraft,
    ) -> Option<oneshot::Receiver<Result<()>>> {
        self.queue_with(executor, worktree, draft, Preferences::save_commit_drafts)
    }

    fn queue_with(
        &self,
        executor: &SerialExecutor,
        worktree: PathBuf,
        draft: CommitDraft,
        save: impl FnMut(&Drafts) -> Result<()> + Send + 'static,
    ) -> Option<oneshot::Receiver<Result<()>>> {
        {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            pending.drafts.insert(worktree, draft);
            if pending.scheduled {
                return None;
            }
            pending.scheduled = true;
        }
        let scheduled = Scheduled {
            pending: Arc::clone(&self.pending),
            active: true,
        };
        // Calling a method captures the complete guard. Disjoint field capture
        // would otherwise leave its Drop on the submitting thread.
        Some(executor.submit(move || scheduled.save(save)))
    }
}

/// Also runs if the bounded executor rejects the job before executing it.
/// Retain unsaved snapshots and allow the next explicit retry/edit to save.
struct Scheduled {
    pending: Arc<Mutex<Pending>>,
    active: bool,
}

impl Scheduled {
    fn save(mut self, mut save: impl FnMut(&Drafts) -> Result<()>) -> Result<()> {
        loop {
            let batch = {
                let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
                if pending.drafts.is_empty() {
                    // Reset under the same lock that observed an empty queue.
                    // A concurrent edit will start another job.
                    pending.scheduled = false;
                    self.active = false;
                    return Ok(());
                }
                std::mem::take(&mut pending.drafts)
            };
            if let Err(error) = save(&batch) {
                let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
                for (path, draft) in batch {
                    // An edit made during the failed save takes priority.
                    pending.drafts.entry(path).or_insert(draft);
                }
                pending.scheduled = false;
                self.active = false;
                return Err(error);
            }
        }
    }
}

impl Drop for Scheduled {
    fn drop(&mut self) {
        if self.active {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .scheduled = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn draft(title: &str) -> CommitDraft {
        CommitDraft {
            title: title.into(),
            description: "A preserved description.\n".into(),
        }
    }

    #[test]
    fn coalesces_typing_and_keeps_distinct_worktrees_after_dropped_reply() {
        let executor = SerialExecutor::new("draft-coalescing-test");
        let saver = DraftSaver::default();
        let (release, gate) = mpsc::channel();
        let first = executor.submit(move || {
            gate.recv()?;
            Ok(())
        });
        let saved = Arc::new(Mutex::new(Vec::new()));
        let output = Arc::clone(&saved);
        let response = saver
            .queue_with(&executor, "/repo".into(), draft("First"), move |drafts| {
                output.lock().unwrap().push(drafts.clone());
                Ok(())
            })
            .unwrap();
        drop(response);
        for title in ["Second", "Final title"] {
            assert!(
                saver
                    .queue_with(&executor, "/repo".into(), draft(title), |_| {
                        panic!("coalesced edits must not schedule another job")
                    })
                    .is_none()
            );
        }
        assert!(
            saver
                .queue_with(&executor, "/linked-worktree".into(), draft("Other"), |_| {
                    unreachable!()
                })
                .is_none()
        );
        release.send(()).unwrap();
        futures::executor::block_on(first).unwrap().unwrap();
        futures::executor::block_on(executor.submit(|| Ok(())))
            .unwrap()
            .unwrap();
        assert_eq!(
            *saved.lock().unwrap(),
            vec![HashMap::from([
                (PathBuf::from("/repo"), draft("Final title")),
                (PathBuf::from("/linked-worktree"), draft("Other")),
            ])]
        );
    }

    #[test]
    fn save_failure_preserves_all_drafts_and_retry_saves_latest_text() {
        let executor = SerialExecutor::new("draft-failure-test");
        let saver = DraftSaver::default();
        let (started, running) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let failed = saver
            .queue_with(&executor, "/repo".into(), draft("Submitted"), move |_| {
                started.send(()).unwrap();
                gate.recv()?;
                anyhow::bail!("disk unavailable")
            })
            .unwrap();
        running.recv().unwrap();
        assert!(
            saver
                .queue_with(&executor, "/repo".into(), draft("Edited"), |_| Ok(()))
                .is_none()
        );
        assert!(
            saver
                .queue_with(&executor, "/linked-worktree".into(), draft("Other"), |_| {
                    Ok(())
                })
                .is_none()
        );
        release.send(()).unwrap();
        assert!(futures::executor::block_on(failed).unwrap().is_err());
        let (saved, output) = mpsc::channel();
        let retry = saver
            .queue_with(
                &executor,
                "/repo".into(),
                draft("Retry latest"),
                move |drafts| {
                    saved.send(drafts.clone()).unwrap();
                    Ok(())
                },
            )
            .unwrap();
        futures::executor::block_on(retry).unwrap().unwrap();
        assert_eq!(
            output.recv().unwrap(),
            HashMap::from([
                (PathBuf::from("/repo"), draft("Retry latest")),
                (PathBuf::from("/linked-worktree"), draft("Other")),
            ])
        );
    }

    #[test]
    fn rejected_job_retains_pending_worktrees_for_the_next_retry() {
        let executor = SerialExecutor::new("draft-full-executor-test");
        let saver = DraftSaver::default();
        let (started, running) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let first = executor.submit(move || {
            started.send(()).unwrap();
            gate.recv()?;
            Ok(())
        });
        running.recv().unwrap();
        let queued: Vec<_> = (0..8).map(|_| executor.submit(|| Ok(()))).collect();
        let rejected = saver
            .queue_with(&executor, "/repo".into(), draft("Keep me"), |_| {
                panic!("a rejected operation must not write")
            })
            .unwrap();
        assert!(futures::executor::block_on(rejected).unwrap().is_err());
        release.send(()).unwrap();
        futures::executor::block_on(first).unwrap().unwrap();
        for response in queued {
            futures::executor::block_on(response).unwrap().unwrap();
        }
        let (saved, output) = mpsc::channel();
        let retry = saver
            .queue_with(
                &executor,
                "/other".into(),
                draft("Other work"),
                move |drafts| {
                    saved.send(drafts.clone()).unwrap();
                    Ok(())
                },
            )
            .unwrap();
        futures::executor::block_on(retry).unwrap().unwrap();
        assert_eq!(
            output.recv().unwrap(),
            HashMap::from([
                (PathBuf::from("/repo"), draft("Keep me")),
                (PathBuf::from("/other"), draft("Other work")),
            ])
        );
    }
}
