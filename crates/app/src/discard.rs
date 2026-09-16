//! Contextual single-file discard: review the exact entry, then revert it to
//! HEAD or delete the reviewed untracked file through the operation executor.
use crate::*;
use gitturtle_core::{ChangeStatus, DiscardPlan, StatusEntry, WriteCommand};

mod review;

const PREPARING: &str = "Reviewing file changes…";

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
}

impl GitTurtle {
    pub(super) fn cancel_discard_action(&mut self) {
        self.discard_actions.generation = self.discard_actions.generation.wrapping_add(1);
        self.discard_actions.task = None;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }

    /// Prepare a fresh review of this exact status row, then confirm it. The
    /// reviewed row, HEAD, and working identity are rechecked before Git runs.
    pub(super) fn open_discard(
        &mut self,
        entry: StatusEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || self.page != AppPage::Repository || entry.conflicted {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let repository = repo.path().to_owned();
        self.cancel_discard_action();
        let generation = self.discard_actions.generation;
        self.operation_busy = Some(PREPARING);
        self.operation_error = None;
        let response = self
            .operations
            .submit_read(move || repo.discard_plan(&entry));
        self.discard_actions.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Discard preparation was interrupted. Review the file again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.discard_actions.generation != generation {
                    return;
                }
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                if this.path.as_ref() != Some(&repository)
                    || this.repository.as_ref().map(|repo| repo.path())
                        != Some(repository.as_path())
                    || this.page != AppPage::Repository
                {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(plan) => {
                        this.confirm_discard(repository, plan, generation, window, cx);
                    }
                    Err(error) => this.operation_error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

/// Describe the whole-file effect, including staged additions that are deleted.
fn consequence(plan: &DiscardPlan) -> &'static str {
    let entry = &plan.entry;
    if entry.untracked {
        "This untracked file will be deleted from the working folder."
    } else if entry.original_path.is_some() {
        "The original path will be restored from the last commit. The renamed path will be removed from the index and deleted from the working folder."
    } else if entry.staged == Some(ChangeStatus::Added)
        || entry.unstaged == Some(ChangeStatus::Added)
    {
        "This file is not in the last commit. It will be removed from the index and deleted from the working folder."
    } else {
        "This file will be restored from the last commit in both the index and the working folder."
    }
}

fn scope(entry: &StatusEntry) -> &'static str {
    if entry.untracked {
        return "Untracked file";
    }
    match (entry.staged.is_some(), entry.unstaged.is_some()) {
        (true, true) => "Staged and unstaged changes",
        (true, false) => "Staged changes",
        _ => "Unstaged changes",
    }
}

#[cfg(test)]
mod tests;
