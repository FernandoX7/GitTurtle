//! Coalesce typing into one accepted preference job. The job owns pending
//! snapshots, so navigation or a dropped UI reply cannot discard a draft save.

use crate::{
    operations::SerialExecutor,
    preferences::{CommitDraft, Preferences},
};
use anyhow::Result;
use futures::channel::oneshot;
use futures::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    hash::Hash,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

struct Pending<K, V> {
    drafts: HashMap<K, V>,
    scheduled: bool,
}

pub struct CoalescingSaver<K, V> {
    pending: Arc<Mutex<Pending<K, V>>>,
}

impl<K, V> Default for CoalescingSaver<K, V> {
    fn default() -> Self {
        Self {
            pending: Arc::new(Mutex::new(Pending {
                drafts: HashMap::new(),
                scheduled: false,
            })),
        }
    }
}

type SaveCompletion = Shared<BoxFuture<'static, std::result::Result<(), String>>>;

#[derive(Default)]
pub struct DraftSaver {
    saver: CoalescingSaver<PathBuf, CommitDraft>,
    completion: Rc<RefCell<Option<SaveCompletion>>>,
}

impl DraftSaver {
    /// The app owns this observer even after its last window and composer have
    /// disappeared. Await the accepted save itself; an unrelated earlier session
    /// save or an extra barrier in a full executor cannot cover this completion.
    pub fn install_quit_observer(&self, cx: &mut gpui_kit::App) {
        let completion = self.completion.clone();
        cx.on_app_quit(move |_| {
            let completion = completion.borrow().clone();
            async move {
                if let Some(completion) = completion {
                    let _ = completion.await;
                }
            }
        })
        .detach();
    }

    pub fn queue(
        &self,
        executor: &SerialExecutor,
        worktree: PathBuf,
        draft: CommitDraft,
    ) -> Option<SaveCompletion> {
        self.queue_with(executor, worktree, draft, Preferences::save_commit_drafts)
    }

    fn queue_with(
        &self,
        executor: &SerialExecutor,
        worktree: PathBuf,
        draft: CommitDraft,
        save: impl FnMut(&HashMap<PathBuf, CommitDraft>) -> Result<()> + Send + 'static,
    ) -> Option<SaveCompletion> {
        let response = self.saver.queue_with(executor, worktree, draft, save)?;
        let completion = response
            .map(|result| match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(format!("{error:#}")),
                Err(_) => Err("Draft save ended without a result".into()),
            })
            .boxed()
            .shared();
        *self.completion.borrow_mut() = Some(completion.clone());
        Some(completion)
    }
}

impl<K: Clone + Eq + Hash + Send + 'static, V: Clone + Send + 'static> CoalescingSaver<K, V> {
    pub(super) fn is_pending(&self) -> bool {
        let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.scheduled || !pending.drafts.is_empty()
    }
    pub(super) fn queue_with(
        &self,
        executor: &SerialExecutor,
        worktree: K,
        draft: V,
        save: impl FnMut(&HashMap<K, V>) -> Result<()> + Send + 'static,
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
struct Scheduled<K, V> {
    pending: Arc<Mutex<Pending<K, V>>>,
    active: bool,
}

impl<K: Eq + Hash, V> Scheduled<K, V> {
    fn save(mut self, mut save: impl FnMut(&HashMap<K, V>) -> Result<()>) -> Result<()> {
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

impl<K, V> Drop for Scheduled<K, V> {
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
    use gpui_kit::gpui;
    use std::sync::mpsc;

    fn draft(title: &str) -> CommitDraft {
        CommitDraft {
            title: title.into(),
            description: "A preserved description.\n".into(),
        }
    }

    #[gpui::test]
    async fn final_commit_draft_survives_window_removal_with_a_full_save_queue(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::*;

        struct Composer {
            saver: DraftSaver,
        }
        impl Render for Composer {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
            }
        }

        cx.executor().allow_parking();
        cx.executor().set_block_on_ticks(100..=100);
        let fixture = tempfile::tempdir().unwrap();
        let destination = fixture.path().join("saved-draft.json");
        let executor = SerialExecutor::new("draft-window-close-test");
        let (release, gate) = mpsc::channel();
        let (started, running) = oneshot::channel();
        let blocker = executor.submit(move || {
            let _ = started.send(());
            gate.recv()?;
            Ok(())
        });
        running.await.unwrap();
        let queued: Vec<_> = (0..7).map(|_| executor.submit(|| Ok(()))).collect();
        let (composer, window_cx) = cx.add_window_view(|_, cx| {
            let saver = DraftSaver::default();
            saver.install_quit_observer(cx);
            Composer { saver }
        });
        let output = destination.clone();
        composer.update(window_cx, |composer, _| {
            drop(composer.saver.queue_with(
                &executor,
                "/fixture/worktree".into(),
                draft("Earlier title"),
                move |drafts| {
                    std::fs::write(&output, serde_json::to_vec(drafts)?)?;
                    Ok(())
                },
            ));
            assert!(
                composer
                    .saver
                    .queue_with(
                        &executor,
                        "/fixture/worktree".into(),
                        draft("Final title\nwith exact spacing  "),
                        |_| unreachable!("final edit coalesces into the accepted save"),
                    )
                    .is_none()
            );
        });
        assert!(executor.submit(|| Ok(())).await.unwrap().is_err());
        let weak = composer.downgrade();
        drop(composer);
        window_cx.update(|window, _| window.remove_window());
        assert!(weak.upgrade().is_none());
        assert!(!destination.exists());
        window_cx
            .executor()
            .spawn(async move {
                release.send(()).unwrap();
            })
            .detach();
        cx.quit();
        // Inspect before awaiting or draining any external replies: GPUI's real
        // app shutdown must own and finish the accepted composer save itself.
        let restored: HashMap<PathBuf, CommitDraft> =
            serde_json::from_slice(&std::fs::read(destination).unwrap()).unwrap();
        assert_eq!(
            restored[&PathBuf::from("/fixture/worktree")],
            draft("Final title\nwith exact spacing  ")
        );
        blocker.await.unwrap().unwrap();
        for response in queued {
            response.await.unwrap().unwrap();
        }
    }

    #[test]
    fn coalesces_typing_and_keeps_distinct_worktrees_after_dropped_reply() {
        let executor = SerialExecutor::new("draft-coalescing-test");
        let saver = CoalescingSaver::<PathBuf, CommitDraft>::default();
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
        let saver = CoalescingSaver::<PathBuf, CommitDraft>::default();
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
        let saver = CoalescingSaver::<PathBuf, CommitDraft>::default();
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
