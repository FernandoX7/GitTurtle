//! Contextual single-file discard: review the exact entry, then revert it to
//! HEAD or delete the reviewed untracked file through the operation executor.
use crate::*;
use gitturtle_core::{ChangeStatus, DiscardPlan, StatusEntry, WriteCommand};

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
                if this.path.as_ref() != Some(&repository) || this.page != AppPage::Repository {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(plan) => {
                        let (title, explanation, action) = review(&repository, &plan);
                        this.confirm_git_write(
                            title,
                            explanation,
                            action,
                            WriteCommand::Discard(Arc::new(plan)),
                            window,
                            cx,
                        );
                    }
                    Err(error) => this.operation_error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

/// The confirmation names the exact effect for this row. Deleted content is
/// not recoverable from Git, so the text says which changes disappear.
fn review(repository: &std::path::Path, plan: &DiscardPlan) -> (String, String, &'static str) {
    let entry = &plan.entry;
    let name = entry
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry.path.display().to_string());
    if entry.untracked {
        return (
            format!("Delete untracked file {name}"),
            format!(
                "Repository: {}\nFile: {}\n\nThis file is not in the index or the last commit. Git deletes it from the working folder. Its content cannot be recovered from Git.\n\nOther files, the index, and the branch stay as they are. If the file changes before confirmation, this action requires a fresh review.",
                repository.display(),
                entry.path.display()
            ),
            "Delete file",
        );
    }
    let target = entry.original_path.as_ref().map_or_else(
        || format!("File: {}", entry.path.display()),
        |old| format!("Rename: {} → {}", old.display(), entry.path.display()),
    );
    let effect = if let Some(old) = &entry.original_path {
        format!(
            "Git restores {} from the last commit and removes {} from the index and the working folder.",
            old.display(),
            entry.path.display()
        )
    } else if entry.staged == Some(ChangeStatus::Added) {
        "This file is not in the last commit. Git removes it from the index and deletes it from the working folder.".to_owned()
    } else if entry.staged == Some(ChangeStatus::Deleted)
        || entry.unstaged == Some(ChangeStatus::Deleted)
    {
        "Git restores this file from the last commit into the index and the working folder."
            .to_owned()
    } else {
        "Git restores this file's index entry and working content from the last commit.".to_owned()
    };
    let scope = match (entry.staged.is_some(), entry.unstaged.is_some()) {
        (true, true) => "Staged and unstaged changes",
        (true, false) => "Staged changes",
        _ => "Unstaged changes",
    };
    (
        format!("Discard changes to {name}"),
        format!(
            "Repository: {}\n{target}\nLast commit: {}\n\n{effect} {scope} to this file are discarded and cannot be recovered from Git.\n\nOther files, their index entries, and the branch stay as they are. If the file, index, or HEAD changes before confirmation, this action requires a fresh review.",
            repository.display(),
            plan.head.as_deref().map(short_oid).unwrap_or_default()
        ),
        "Discard changes",
    )
}

#[cfg(test)]
mod tests;
