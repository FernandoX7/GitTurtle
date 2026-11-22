//! Explicit, prepared recovery actions and a read-only stash inspector.
use crate::native_accessibility::LiveRegion as _;
use crate::*;
use gitturtle_core::{
    CheckoutDetachedPlan, CommitRecoveryPlan, ComparisonMode, HistoryCancellation,
    MAX_RECOVERY_MESSAGE_BYTES, OperationKind, RecoveryCommand, RecoveryKind, ResetMode, ResetPlan,
    StashEntry, StashPage, StashSnapshot, WriteCommand,
};
use gpui_kit::component::{
    WindowExt,
    checkbox::Checkbox,
    dialog::{Cancel, Confirm, DialogKind},
    menu::{PopupMenu, PopupMenuItem},
    scroll::{Scrollbar, ScrollbarMode},
};
use gpui_kit::prelude::FluentBuilder;

pub(crate) const PREPARING: &str = "Preparing recovery action…";
/// Why a recovery action refuses while the merge the tests pause is paused
/// (`gitturtle_core::paused_recovery_refusal`).
#[cfg(test)]
pub(super) const PAUSED_REFUSAL: &str =
    gitturtle_core::paused_recovery_refusal(Some(gitturtle_core::OperationKind::Merge));
/// What Stashes says once the merge the tests pause has ended
/// ([`pause_ended_line`]).
#[cfg(test)]
const PAUSE_ENDED: &str = "The merge has ended. Choose Review restore… to continue.";
/// What a refusal in the core's words for a pause is for
/// (`gitturtle_core::paused_recovery_operation`): the operation paused, or
/// `None` for conflicts left with none paused.
type RefusedFor = Option<OperationKind>;
/// What a recovery form says in place of a refusal for `cause`
/// (`gitturtle_core::paused_recovery_refusal`) once a status read finds it
/// gone, naming the form's own button, `review`, that tries again: a paused
/// operation that has ended (`GitTurtle::integration_ended`), "The merge has
/// ended. Choose Review stash to continue.", or conflicts alone, none of
/// which remain (`GitTurtle::conflicts_ended`), "No conflicts remain. Choose
/// Review stash to continue." It once said "The paused operation has ended.
/// Review again to continue." whatever had paused, naming no button.
fn pause_ended_line(cause: RefusedFor, review: &str) -> SharedString {
    match cause {
        Some(operation) => format!(
            "The {} has ended. Choose {review} to continue.",
            operation.label().to_lowercase()
        ),
        None => format!("No conflicts remain. Choose {review} to continue."),
    }
    .into()
}
/// The buttons that review again: the Amend and Save changes to stash…
/// forms' own, and Stashes' Review restore….
const REVIEW_AMEND: &str = "Review amend";
const REVIEW_STASH: &str = "Review stash";
const REVIEW_RESTORE: &str = "Review restore…";
/// Review restore…'s label while its own preparation looks busy
/// ([`StashBrowser::review_restore`]).
const RESTORE_PREPARING: &str = "Preparing…";
/// Review restore…'s name while its own preparation runs, heard at once.
const RESTORE_PREPARING_NAME: &str = "Review restore…, preparing the restoration";
/// What Stashes' footer note says for a Review restore… answer whose report
/// has no line that says anything ([`StashBrowser::restore_note`]).
const RESTORE_UNEXPLAINED: &str = "Git could not prepare this restoration.";

/// The first line of a Review restore… answer that says anything; `None`
/// where no line does.
pub(crate) fn first_line(answer: &str) -> Option<&str> {
    answer.lines().map(str::trim).find(|line| !line.is_empty())
}

/// What Stashes' footer note shows and the window's alert says for the
/// Review restore… answer `answer`: its [`first_line`], bounded as every
/// long accessible text is (`native_accessibility::bounded_accessible_text`),
/// or [`RESTORE_UNEXPLAINED`] where no line says anything. Core keeps up to
/// 4 MiB of each Git stream, so one line of stderr reached the alert, and
/// the note's name, layout and tooltip, at that length.
/// [`StashBrowser::refuse`] and [`StashBrowser::restore_note`] both take it
/// from here, so what is said and what is drawn stay in step.
fn restore_said(answer: &str) -> SharedString {
    first_line(answer).map_or(
        SharedString::new_static(RESTORE_UNEXPLAINED),
        native_accessibility::bounded_accessible_text,
    )
}

const STASH_PAGE_SIZE: usize = 40;
/// How many saved stashes one read of the list looks through for a stash
/// that has left the page shown ([`stash_page`]), the most the core gives.
const STASH_SEEK_SIZE: usize = 500;
/// What the Stashes detail says while the list is read.
const READING_LIST: &str = "Reading stashes…";
/// What it says while the chosen stash's saved changes are read.
const READING_STASH: &str = "Reading saved changes…";
/// What Stashes says where nothing is saved: its list's empty state, and the
/// words its Refresh control's name carries, which focus takes as the read
/// that opened it lands ([`StashBrowser::opening`]).
const NO_SAVED_STASHES: &str = "No saved stashes · Save one from the Actions menu in Changes";
/// The least height a saved file's comparison keeps in the Stashes detail,
/// at the default interface size, where the window leaves the detail less
/// than its inspection and it scrolls: about six lines of a patch.
const STASH_CONTENT_FLOOR: f32 = 120.;
/// How readily the stacked list panel's spare height and the gap above the
/// detail give way, next to the detail's floor, which shrinks at the usual
/// rate: a thousand times as readily, so they give their room first, down to
/// their least, and only a window too short for even that takes the detail
/// below its floor, where it scrolls and fades as before rather than spill
/// past the browser's foot, as a `min_h` floor would. (A detail that shrank
/// at a tiny rate instead kept its floor and spilled: a flex line whose
/// shrinking items' factors sum below one gives up only that share.)
const STACKED_SPARE_SHRINK: f32 = 1000.;
type Failure = Box<dyn FnOnce(String, &mut Window, &mut Context<GitTurtle>)>;
/// What runs as a prepared recovery action's review would open, before it
/// does: whether it still answers what was asked, false dropping it.
/// Stashes closes for Restore stash's.
type Reviewing = Box<dyn FnOnce(&mut Window, &mut Context<GitTurtle>) -> bool>;

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
    /// Whether the running preparation's plan is no longer wanted
    /// ([`GitTurtle::dismiss_recovery_read`]).
    dismissed: bool,
    /// Whether Changes asked for the running preparation, which History
    /// shown again drops rather than wants again
    /// ([`GitTurtle::recovery_asked_in_changes`]).
    asked_in_changes: bool,
    amend: Option<Entity<AmendForm>>,
    stash: Option<Entity<StashForm>>,
    /// The Stashes dialog opened last, which keeps its own Review restore…
    /// refusal and hears when the pause it was refused for ends
    /// ([`GitTurtle::integration_ended`]). The dialog holds it; closed, it
    /// goes.
    browser: Option<WeakEntity<StashBrowser>>,
    /// The working-state reads asked for before the latest refusal of a
    /// recovery review landed (`GitTurtle::request_status_read`): only a
    /// later read can find its cause gone ([`GitTurtle::conflicts_ended`]).
    /// One asked for before it may have read the working state before the
    /// conflict the core refused for came, as an outside `git stash apply`
    /// leaves one.
    refused_after: u64,
}

pub(super) struct CommitTarget {
    oid: String,
    subject: String,
    parents: Vec<String>,
}

/// The commit's object ID, which a commit's menu reads to know whether the
/// commit is HEAD (`native_accessibility::MenuCommit`).
impl AsRef<str> for CommitTarget {
    fn as_ref(&self) -> &str {
        &self.oid
    }
}

pub(super) fn commit_target(commit: &Commit) -> Arc<CommitTarget> {
    Arc::new(CommitTarget {
        oid: commit.oid.clone(),
        subject: commit.subject.clone(),
        parents: commit.parents.clone(),
    })
}

enum Prepared {
    Amend(Arc<CommitRecoveryPlan>),
    Reset(Arc<ResetPlan>),
    CheckoutDetached(Arc<CheckoutDetachedPlan>),
    Confirmation {
        title: String,
        explanation: String,
        action: &'static str,
        command: Arc<RecoveryCommand>,
    },
}

impl GitTurtle {
    pub(super) fn cancel_recovery_read(&mut self) {
        self.recovery.generation = self.recovery.generation.wrapping_add(1);
        self.recovery.task = None;
        self.recovery.dismissed = false;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }

    /// Cancels the recovery read `read` while it is still the one
    /// preparing ([`Self::cancel_recovery_read`]), never a later one: Stashes'
    /// Review restore… once what it asked for is no longer shown. Its job is
    /// skipped if it has not started, its answer dropped, and the window is
    /// free for another action at once.
    ///
    /// Its end, which reads the working state asked for meanwhile, never
    /// runs, so the cancel reads it, as a form's Cancel does
    /// ([`Self::read_deferred_working`]): Refresh pressed while the
    /// restoration was prepared once stayed noted until something else
    /// ended, and Changes kept the rows read before it. Stashes cancels from
    /// its listeners and as it is released, with no window at hand, so the
    /// read starts in the window this view draws in once the update ends.
    fn cancel_recovery_preparation(&mut self, read: u64, cx: &mut Context<Self>) {
        if self.recovery.generation == read && self.recovery.task.is_some() {
            self.cancel_recovery_read();
            let owner = cx.entity().downgrade();
            let window = self.window_handle;
            cx.defer(move |cx| {
                let _ = window.update(cx, |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| this.read_deferred_working(window, cx));
                });
            });
            cx.notify();
        }
    }

    /// Drops the plan the running preparation brings, as Changes opens over
    /// the view that asked for it, which [`Self::cancel_recovery_read`]
    /// does on leaving the page. Its read still ends as it would: the
    /// operation executor cannot stop it, so the preparation keeps its busy
    /// state until then and, ending, reads the working state Changes asked
    /// for meanwhile (`read_deferred_working`) rather than leave that read
    /// queued behind it. Opened during a preparation History started, Changes
    /// once showed its plan's form over the working list. `dismissed` false,
    /// as History shows again before the preparation ends, wants the plan
    /// again ([`Self::dismiss_history_plans`]).
    pub(super) fn dismiss_recovery_read(&mut self, dismissed: bool) {
        self.recovery.dismissed = dismissed && self.recovery.task.is_some();
    }

    /// Whether the running preparation is one Changes asked for, as its
    /// Amend last commit… asks: History shown again drops it rather than
    /// want its plan, which would open over History
    /// (`GitTurtle::end_preparations`).
    pub(super) fn recovery_asked_in_changes(&self) -> bool {
        self.recovery.task.is_some() && self.recovery.asked_in_changes
    }

    /// Reports that a recovery action refuses while an integration is
    /// paused, in the core's words and the way its refusal arrives, once the
    /// caller's update has ended: a form that asked, `failure`, closes and
    /// reopens with it, and otherwise the window's error shows it.
    ///
    /// The pause is the one the window read last, which may be stale: a
    /// merge finished in a terminal before the local watcher's read lands,
    /// or a retained tab restored its own. So the refusal also asks for a
    /// quiet read of the working state, which reads and writes nothing else,
    /// and the next try reads that pause, while a read that finds it ended
    /// replaces the refusal ([`Self::integration_ended`]). The action itself
    /// is never tried again.
    fn refuse_while_paused(
        &mut self,
        failure: Option<Failure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // In the core's words for the pause the window read, naming the
        // operation and how to end it.
        let message = gitturtle_core::paused_recovery_refusal(
            self.integration_state
                .as_ref()
                .map(|operation| operation.kind),
        )
        .to_owned();
        cx.defer_in(window, move |this, window, cx| {
            match failure {
                Some(failure) => failure(message, window, cx),
                None => {
                    this.operation_error = Some(message.into());
                    // A fresh node, heard again however often it refuses.
                    this.error_refusals = this.error_refusals.wrapping_add(1);
                }
            }
            cx.notify();
        });
        self.read_pause_again(window, cx);
    }

    /// Asks for a quiet read of the working state, which reads and writes
    /// nothing else, so the window's pause is read again.
    fn read_pause_again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.queue_automatic_refresh(
            local_refresh::LocalChange {
                worktree: true,
                ..Default::default()
            },
            window,
            cx,
        );
    }

    /// A status read found that the integration paused when the window last
    /// read it has ended: a recovery form of this repository, or its open
    /// Stashes dialog, whose line refuses because one was paused says it has
    /// ended instead, and the window's error stops refusing so. The refusal
    /// may be the window's or the core's, which share their words; any later
    /// reason stays. Stashes' note is said from the window's announcement
    /// node, once, politely: its detail can have scrolled the note out of
    /// view, where AccessKit's filter leaves out a line of its own
    /// ([`StashBrowser::refuse`]).
    pub(super) fn integration_ended(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.recovery_refusals_ended(false, window, cx);
        // As the forms do, a refusal for conflicts alone stays: the end of
        // an integration does not resolve them.
        if self
            .operation_error
            .as_deref()
            .and_then(gitturtle_core::paused_recovery_operation)
            .flatten()
            .is_some()
        {
            self.operation_error = None;
        }
    }

    /// A status read found no conflicted entry: a refusal for conflicts
    /// alone, which the end of an integration keeps, goes in that frame,
    /// the window's error and the line of an open Amend or Save changes to
    /// stash… form, or the open Stashes' note, of this repository alike,
    /// each of those giving way to the line that says none remain, naming
    /// its own button, as at the end of a pause ([`pause_ended_line`]).
    /// Only the core's exact refusal for conflicts alone goes
    /// (`gitturtle_core::paused_recovery_refusal(None)`); one that names a
    /// paused operation stays until that ends. The forms' and Stashes'
    /// copies once stayed after the window's error had gone, saying
    /// conflicts remained after they were resolved. Only a read asked for
    /// after the latest refusal landed, the `requested`th
    /// (`request_status_read`), withdraws one, for the forms and the
    /// window's error alike (`State::refused_after`): a read asked for
    /// before it, landing after it, may have read the working state before
    /// an outside `git stash apply` left the conflict, and once said "No
    /// conflicts remain" beside it. The refusal asks for a read of its own,
    /// which decides (`read_pause_again`).
    pub(super) fn conflicts_ended(
        &mut self,
        requested: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if requested <= self.recovery.refused_after {
            return;
        }
        self.recovery_refusals_ended(true, window, cx);
        if self.operation_error.as_deref() == Some(gitturtle_core::paused_recovery_refusal(None)) {
            self.operation_error = None;
        }
    }

    /// Replaces each refusal of this repository's open recovery forms and
    /// Stashes whose cause a status read has found gone, a paused
    /// integration or, where `conflicts`, conflicts alone, with the line
    /// that says so ([`refusal_ended`]). Stashes' note, which its detail can
    /// have scrolled out of view, is said by the window's announcement,
    /// once, politely ([`StashBrowser::refuse`]).
    fn recovery_refusals_ended(
        &mut self,
        conflicts: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.path.clone();
        if let Some(browser) = self.recovery.browser.as_ref().and_then(WeakEntity::upgrade)
            && path.as_deref() == Some(browser.read(cx).repository.path())
        {
            browser.update(cx, |browser, cx| {
                if let Some(cause) = refusal_ended(
                    &mut browser.restore_line,
                    &mut browser.pause_ended,
                    conflicts,
                ) {
                    browser.reveal_note_for(NoteReveal::PauseEnded);
                    crate::copy_feedback::announce(
                        window,
                        cx,
                        pause_ended_line(cause, REVIEW_RESTORE),
                    );
                    cx.notify();
                }
            });
        }
        if let Some(form) = &self.recovery.amend
            && form.read(cx).repository == path
        {
            form.update(cx, |form, cx| {
                if refusal_ended(&mut form.error, &mut form.pause_ended, conflicts).is_some() {
                    cx.notify();
                }
            });
        }
        if let Some(form) = &self.recovery.stash
            && form.read(cx).repository == path
        {
            form.update(cx, |form, cx| {
                if refusal_ended(&mut form.error, &mut form.pause_ended, conflicts).is_some() {
                    cx.notify();
                }
            });
        }
    }

    /// Prepares a recovery action with `prepare` on the operation executor,
    /// then asks to confirm it, or reports why it cannot run through
    /// `failure`, the form that asked, or else the window's error. Returns
    /// whether that outcome follows; false, with nothing reported, while a
    /// write runs, off the repository page or without a repository.
    fn read_recovery(
        &mut self,
        prepare: impl FnOnce(GitRepository) -> anyhow::Result<Prepared> + Send + 'static,
        failure: Option<Failure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.recovery_repository().is_none() {
            return false;
        }
        if self.integration_state.is_some() {
            // Every recovery plan refuses while a merge, rebase, cherry-pick
            // or revert is paused (the core's `recovery_head`), as the menus
            // show. Refused here without a read.
            self.refuse_while_paused(failure, window, cx);
            return true;
        }
        self.prepare_recovery(prepare, failure, None, window, cx)
    }

    /// The repository a recovery action reads; none while a write runs, off
    /// the repository page or without a repository.
    fn recovery_repository(&self) -> Option<GitRepository> {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return None;
        }
        self.repository.clone()
    }

    /// Prepares a recovery action, as [`Self::read_recovery`] does, without
    /// refusing first from the pause the window read last: the core's plan
    /// reads the pause itself. `reviewing` runs as the review would open,
    /// before it does, and drops it if it no longer answers what was asked;
    /// `failure` likewise decides where a refusal goes, if anywhere. Where
    /// the core's answer and the window's pause disagree, a
    /// refusal for a pause the window has not read or a plan where it reads
    /// one, the window reads its working state again, quietly
    /// ([`Self::read_pause_again`]).
    fn prepare_recovery(
        &mut self,
        prepare: impl FnOnce(GitRepository) -> anyhow::Result<Prepared> + Send + 'static,
        failure: Option<Failure>,
        reviewing: Option<Reviewing>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(repo) = self.recovery_repository() else {
            return false;
        };
        let path = repo.path().to_owned();
        self.cancel_recovery_read();
        let generation = self.recovery.generation;
        self.recovery.asked_in_changes = self.mode == WorkspaceMode::Working;
        self.operation_busy = Some(PREPARING);
        self.operation_error = None;
        let response = self.operations.submit_read(move || prepare(repo));
        self.recovery.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Recovery preparation stopped. Open the action again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.recovery.generation != generation {
                    return;
                }
                // The reads asked for before the core's answer landed: any
                // asked for from here on reads after what it read.
                let asked = this.status_requests;
                this.recovery.task = None;
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                this.read_deferred_working(window, cx);
                if this.path.as_ref() != Some(&path)
                    || this.page != AppPage::Repository
                    || this.recovery.dismissed
                {
                    cx.notify();
                    return;
                }
                let refused_for_pause =
                    matches!(&result, Err(error) if gitturtle_core::is_paused_recovery_refusal(&format!("{error:#}")));
                if refused_for_pause != this.integration_state.is_some() {
                    this.read_pause_again(window, cx);
                }
                match result {
                    Ok(Prepared::Amend(plan)) => this.show_amend_form(plan, window, cx),
                    Ok(Prepared::Reset(plan)) => this.show_reset_dialog(plan, window, cx),
                    Ok(Prepared::CheckoutDetached(plan)) => {
                        let explanation = format!(
                            "Check out {} ({}) in detached HEAD mode.\n\nCurrent HEAD: {}{}\n\nNo branch will be moved; existing commits remain reachable by their branches and reflog.",
                            short_oid(&plan.target_oid),
                            plan.target_subject,
                            plan.current_branch.as_deref().unwrap_or("detached HEAD"),
                            plan.current_head
                                .as_deref()
                                .map(|oid| format!(" at {}", short_oid(oid)))
                                .unwrap_or_default(),
                        );
                        this.confirm_git_write(
                            "Checkout commit (detached HEAD)".into(),
                            explanation,
                            "Checkout detached",
                            WriteCommand::CheckoutDetached(plan),
                            window,
                            cx,
                        );
                    }
                    Ok(Prepared::Confirmation {
                        title,
                        explanation,
                        action,
                        command,
                    }) => {
                        if reviewing.is_none_or(|reviewing| reviewing(window, cx)) {
                            this.confirm_git_write(
                                title,
                                explanation,
                                action,
                                WriteCommand::Recovery(command),
                                window,
                                cx,
                            );
                        }
                    }
                    Err(error) => {
                        // Only a read asked for after the refusal can find
                        // its cause gone (`conflicts_ended`).
                        this.recovery.refused_after = asked;
                        if let Some(failure) = failure {
                            failure(format!("{error:#}"), window, cx);
                        } else {
                            this.operation_error = Some(crate::views::OperationFailure::of(&error));
                        }
                    }
                }
                this.try_automatic_refresh(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
        true
    }

    /// The Changes header's Actions menu, opened by `working_actions_trigger`
    /// (`compact` in an inspector too narrow for its label beside Refresh,
    /// `workspace::WorkingForms`). `tight`, in an inspector too
    /// short for the selection controls (`workspace::WorkingFit`), the menu
    /// leads with Directories and the selection's Stage and Unstage, and
    /// with Refresh where it holds `Some(true)`, as Refresh then has no
    /// control of its own.
    pub(super) fn render_working_recovery_menu(
        &self,
        compact: bool,
        tight: Option<bool>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (staged, unstaged) = self.selection_counts();
        let grouped = self.working_selection.grouped;
        // A running write refuses them too, and they say why then
        // (`native_accessibility::waiting_item`).
        let selection_unavailable =
            self.working_paths.pending || self.working_paths.error.is_some();
        let refreshing = self.status_task.is_some();
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let has_changes = self
            .work_status
            .as_ref()
            .is_some_and(|status| !status.entries.is_empty());
        let ignore_entry = self
            .working_selected
            .and_then(|(index, area)| {
                (area == gitturtle_core::ChangeArea::Unstaged).then_some(index)
            })
            .and_then(|index| self.work_status.as_ref()?.entries.get(index))
            .filter(|entry| entry.untracked)
            .cloned();
        working_actions_trigger(compact, tight.is_some())
            .disabled(self.operation_shown().is_some())
            .app_menu(move |mut menu, _, cx| {
                // Every item refuses while a write runs (`working_item`), so
                // each is disabled then from the moment the write starts,
                // though the trigger opens the menu until the write looks
                // busy: an assistive technology heard each one available.
                // Each says why, the write by its label, unless a reason of
                // its own refuses it as well (`GitTurtle::menu_wait`).
                // A paused integration, read here too, disables Save changes
                // to stash…, Amend and Undo, as the menu is built again when
                // one pauses or ends (`native_accessibility::MenuInputs`).
                // Amend and Undo act on HEAD as the menu is built, which it
                // is again as HEAD moves (`AppDropdown::follows_head`): taken
                // as the menu opened, they reviewed the commit a `git reset`
                // outside the app had moved HEAD off, which the review then
                // refused.
                let (gone, wait, paused, head) =
                    owner.upgrade().map_or((true, None, true, None), |owner| {
                        let owner = owner.read(cx);
                        (
                            false,
                            owner.menu_wait(),
                            owner.integration_state.is_some(),
                            owner
                                .work_status
                                .as_ref()
                                .and_then(|status| status.head.clone()),
                        )
                    });
                if let Some(refresh) = tight {
                    menu = menu
                        .item(
                            working_item(
                                "Directories",
                                gone,
                                wait,
                                &owner,
                                &repository,
                                |this, window, cx| {
                                    this.working_selection.grouped =
                                        !this.working_selection.grouped;
                                    this.filter_working_paths(window, cx);
                                },
                            )
                            .checked(grouped),
                        )
                        .item(working_item(
                            format!("Stage {unstaged} selected"),
                            gone || unstaged == 0 || selection_unavailable,
                            wait,
                            &owner,
                            &repository,
                            |this, window, cx| this.apply_selected_files(false, window, cx),
                        ))
                        .item(working_item(
                            format!("Unstage {staged} selected"),
                            gone || staged == 0 || selection_unavailable,
                            wait,
                            &owner,
                            &repository,
                            |this, window, cx| this.apply_selected_files(true, window, cx),
                        ));
                    if refresh {
                        menu = menu.item(working_item(
                            "Refresh working changes",
                            gone || refreshing,
                            wait,
                            &owner,
                            &repository,
                            |this, window, cx| this.refresh_worktree(window, cx),
                        ));
                    }
                    menu = menu.separator();
                }
                let ignore_entry = ignore_entry.clone();
                let menu = working_menu_item(
                    menu,
                    "Ignore selected untracked file…",
                    gone || ignore_entry.is_none(),
                    wait,
                    &owner,
                    &repository,
                    move |this, window, cx| {
                        if let Some(entry) = &ignore_entry {
                            this.open_ignore(entry.clone(), window, cx);
                        }
                    },
                );
                let menu = working_menu_item(
                    menu,
                    "Save changes to stash…",
                    gone || !has_changes || paused,
                    wait,
                    &owner,
                    &repository,
                    |this, window, cx| this.open_stash_form(window, cx),
                );
                let menu = working_menu_item(
                    menu,
                    "Browse stashes…",
                    gone,
                    wait,
                    &owner,
                    &repository,
                    |this, window, cx| this.open_stashes(window, cx),
                );
                let Some(head) = &head else {
                    return menu;
                };
                let amend = head.clone();
                let undo = head.clone();
                let menu = working_menu_item(
                    menu.separator(),
                    "Amend last commit…",
                    gone || paused,
                    wait,
                    &owner,
                    &repository,
                    move |this, window, cx| {
                        this.prepare_commit_recovery(
                            RecoveryKind::Amend,
                            amend.clone(),
                            None,
                            window,
                            cx,
                        )
                    },
                );
                working_menu_item(
                    menu,
                    "Undo last local commit…",
                    gone || paused,
                    wait,
                    &owner,
                    &repository,
                    move |this, window, cx| {
                        this.prepare_commit_recovery(
                            RecoveryKind::Undo,
                            undo.clone(),
                            None,
                            window,
                            cx,
                        )
                    },
                )
            })
            // A write's start or end builds the open menu again, as does
            // HEAD moving to another commit.
            .follows_writes(cx.entity().downgrade())
            .follows_head()
            .into_any_element()
    }

    /// The selected commit's Actions menu, at the trailing edge of the
    /// message toolbar's copy controls (`commit_message::Toolbar`), labelled
    /// where the toolbar fits its label and caret there.
    pub(super) fn render_commit_recovery_menu(
        &self,
        commit: &Commit,
        labelled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let commit = commit_target(commit);
        let head_of = commit.clone();
        commit_actions_trigger(labelled)
            .ml_auto()
            .disabled(self.operation_shown().is_some())
            .app_menu(move |menu, window, cx| {
                Self::recovery_context_menu(menu, &owner, &repository, &commit, window, cx)
            })
            // A write's start or end builds the open menu again, as does the
            // commit becoming or no longer being HEAD.
            .follows_writes(cx.entity().downgrade())
            .follows_head_of(head_of)
            .into_any_element()
    }

    pub(super) fn recovery_context_menu(
        menu: PopupMenu,
        owner: &WeakEntity<Self>,
        repository: &Option<PathBuf>,
        commit: &Arc<CommitTarget>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let Some(this) = owner.upgrade() else {
            return menu;
        };
        let (paused, wait, current, compare_with) = {
            let this = this.read(cx);
            let (paused, wait, current) = this.commit_menu_state(commit);
            let compare_with = this.compare_partner_for(&commit.oid);
            (paused, wait, current, compare_with)
        };
        commit_menu(
            menu,
            owner,
            repository,
            commit,
            (paused, wait),
            current,
            compare_with,
            window,
            cx,
        )
    }

    fn compare_partner_for(&self, commit_oid: &str) -> Option<String> {
        let marked = self.selected_compare_commit.as_deref()?;
        if marked != commit_oid {
            Some(marked.to_owned())
        } else {
            self.selected_commit
                .and_then(|i| self.commits.get(i))
                .map(|c| c.oid.as_str())
                .filter(|selected| *selected != commit_oid)
                .map(str::to_owned)
        }
    }

    /// The commit's menu as a right-click builds it, for the keyboard to
    /// open below the selected row: built from the window's state it is
    /// given, as the menu opens while the window is being updated and again
    /// as a write or a pause starts or ends or the commit becomes or stops
    /// being HEAD, which adds or takes away Amend and Undo (`KeyboardMenu`).
    pub(super) fn keyboard_commit_menu(
        owner: WeakEntity<Self>,
        repository: Option<PathBuf>,
        commit: Arc<CommitTarget>,
    ) -> impl Fn(PopupMenu, &GitTurtle, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static
    {
        move |menu, app, window, cx| {
            let (paused, wait, current) = app.commit_menu_state(&commit);
            let compare_with = app.compare_partner_for(&commit.oid);
            commit_menu(
                menu,
                &owner,
                &repository,
                &commit,
                (paused, wait),
                current,
                compare_with,
                window,
                cx,
            )
        }
    }

    /// Enter in a keyboard's commit menu while an integration is paused,
    /// which disables every item it holds: says why in the window's error,
    /// as a press on one of them would have, and leaves the menu open
    /// (`KeyboardMenu::refuses_while_paused`).
    pub(super) fn refuse_commit_menu_while_paused(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refuse_while_paused(None, window, cx);
    }

    /// Whether a commit's menu offers nothing while an integration is
    /// paused, why its items wait while a write runs or its reload has yet
    /// to land, which they state (`native_accessibility::MenuWait`), and
    /// whether the commit is HEAD, which adds Amend and Undo.
    fn commit_menu_state(
        &self,
        commit: &CommitTarget,
    ) -> (bool, Option<native_accessibility::MenuWait>, bool) {
        let paused = self.integration_state.is_some();
        let current = self
            .work_status
            .as_ref()
            .and_then(|status| status.head.as_ref())
            == Some(&commit.oid);
        (paused, self.menu_wait(), current)
    }

    /// A commit menu's Revert, Cherry-pick, Amend or Undo. Each refuses while
    /// an integration is paused, as the menu shows it disabled
    /// ([`Self::commit_menu_state`]), before a merge's mainline is asked for,
    /// and says why in the window's error, as Review restore… does, should a
    /// press reach one, as one could in a keyboard's menu when it was built
    /// only as it opened: Enter there closed the menu and did nothing
    /// visible.
    pub(super) fn choose_commit_recovery(
        &mut self,
        kind: RecoveryKind,
        commit: Arc<CommitTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.integration_state.is_some() {
            self.refuse_while_paused(None, window, cx);
            return;
        }
        if matches!(kind, RecoveryKind::Revert | RecoveryKind::CherryPick)
            && commit.parents.len() > 1
        {
            let owner = cx.entity().downgrade();
            let repository = self.path.clone();
            window.open_alert_dialog(cx, move |dialog, window, _| {
                dialog.title("Choose the mainline parent").width(appearance::dialog_width(px(560.), window)).max_h(appearance::dialog_max_height(window))
                    .child(div().text_size(crate::appearance::ui_text(12.)).child(format!("{} is a merge commit. Choose the parent against which its change should be calculated.", short_oid(&commit.oid))))
                    .child(crate::native_accessibility::clips_children(div().id("recovery-mainline-parents").max_h(px(320.)).overflow_y_scroll().flex().flex_col().gap_2()
                        .children(commit.parents.iter().enumerate().take(128).map(|(index, parent)| {
                            let owner = owner.clone(); let repository = repository.clone(); let oid = commit.oid.clone();
                            button(("recovery-mainline", index), format!("Parent {} · {}", index + 1, short_oid(parent)), "", false)
                                .debug_selector(move || format!("recovery-mainline-{index}"))
                                .w_full().on_click(move |_, window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        if this.path == repository && this.page == AppPage::Repository {
                                            window.close_dialog(cx);
                                            this.prepare_commit_recovery(kind, oid.clone(), Some(index + 1), window, cx);
                                        }
                                    });
                                })
                        }))))
                    // The parents are equal choices, so none is filled, and
                    // Cancel is bordered rather than the dialog's default.
                    .footer(crate::tags::dialog_dismiss_footer("Cancel"))
            });
        } else {
            self.prepare_commit_recovery(kind, commit.oid.clone(), None, window, cx);
        }
    }

    fn prepare_commit_recovery(
        &mut self,
        kind: RecoveryKind,
        oid: String,
        mainline: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_recovery(move |repo| {
            let plan = repo.recovery_plan(kind, Some(&oid), mainline)?;
            anyhow::ensure!(plan.target.oid == oid, "The selected commit changed. Open its actions again.");
            if kind == RecoveryKind::Amend { return Ok(Prepared::Amend(Arc::new(plan))); }
            let (title, action, command, consequence) = match kind {
                RecoveryKind::Undo => (
                    "Undo last local commit", "Undo local commit", RecoveryCommand::Undo { plan: plan.clone() },
                    "Move the current branch back one commit while preserving the exact index and working files. The undone commit remains staged relative to the previous commit; your existing unstaged edits remain available.",
                ),
                RecoveryKind::Revert => (
                    "Revert commit", "Create revert commit", RecoveryCommand::Revert { plan: plan.clone() },
                    "Create a new commit that reverses this commit's change. Existing commits remain in history. Conflicts pause the operation for review in Changes.",
                ),
                RecoveryKind::CherryPick => (
                    "Cherry-pick commit", "Cherry-pick commit", RecoveryCommand::CherryPick { plan: plan.clone() },
                    "Apply this commit's change to the current branch as a new commit. Conflicts pause the operation for review in Changes.",
                ),
                RecoveryKind::Amend => unreachable!(),
            };
            let mut explanation = format!("{} · {}\n\nTarget branch: {} at {}\n\n{consequence}", short_oid(&plan.target.oid), plan.target.subject, plan.current.branch, short_oid(&plan.current.head));
            if let Some(parent) = plan.mainline { explanation.push_str(&format!("\n\nMainline: parent {parent} · {}", short_oid(&plan.target.parents[parent - 1]))); }
            append_paths(&mut explanation, &plan.affected_paths);
            if kind != RecoveryKind::Undo { explanation.push_str("\n\nGit uses its configured hooks and signing. This action does not contact a remote."); }
            Ok(Prepared::Confirmation { title: title.into(), explanation, action, command: command.into() })
        }, None, window, cx);
    }

    fn show_amend_form(
        &mut self,
        plan: Arc<CommitRecoveryPlan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.path.clone();
        let reuse = self
            .recovery
            .amend
            .as_ref()
            .filter(|form| {
                let form = form.read(cx);
                form.repository == path && form.plan.target.oid == plan.target.oid
            })
            .cloned();
        let form = if let Some(form) = reuse {
            form.update(cx, |form, _| form.plan = Arc::clone(&plan));
            form
        } else {
            let owner = cx.entity().downgrade();
            cx.new(|cx| AmendForm::new(owner, path, plan, window, cx))
        };
        self.recovery.amend = Some(form.clone());
        show_amend(form, window, cx);
    }

    pub(super) fn open_reset_commit(
        &mut self,
        oid: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_recovery(
            move |repo| Ok(Prepared::Reset(Arc::new(repo.reset_plan(&oid)?))),
            None,
            window,
            cx,
        );
    }

    pub(super) fn open_checkout_detached(
        &mut self,
        oid: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_recovery(
            move |repo| {
                Ok(Prepared::CheckoutDetached(Arc::new(
                    repo.checkout_detached_plan(&oid)?,
                )))
            },
            None,
            window,
            cx,
        );
    }

    pub(super) fn copy_commit_patch(
        &mut self,
        oid: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let repository = repo.path().to_owned();
        let short = short_oid(&oid);
        let response = self
            .operations
            .submit_read(move || repo.commit_patch(&oid, &HistoryCancellation::default()));
        cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Patch export was interrupted.")));
            let _ = this.update_in(cx, |this, window, cx| {
                if this.path.as_ref() != Some(&repository) {
                    return;
                }
                match result {
                    Ok(patch) => {
                        copy_feedback::copy(window, cx, None, "commit patch", patch);
                        this.status = format!("Copied patch for {short}");
                    }
                    Err(error) => {
                        this.operation_error = Some(crate::views::OperationFailure::of(&error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn show_reset_dialog(
        &mut self,
        plan: Arc<ResetPlan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let form = cx.new(|_| ResetForm {
            plan: Arc::clone(&plan),
            mode: ResetMode::Mixed,
        });
        let submitted = std::rc::Rc::new(std::cell::Cell::new(false));
        let hard_requested = std::rc::Rc::new(std::cell::Cell::new(false));
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let mode = form.read(cx).mode;
            let is_hard = mode == ResetMode::Hard;
            let requires_explicit = is_hard || !plan.known_published_refs.is_empty();
            let form_view = form.clone();
            let form_ok = form.clone();
            let owner = owner.clone();
            let repository = repository.clone();
            let plan = Arc::clone(&plan);
            let submitted = submitted.clone();
            let req = hard_requested.clone();
            let req_key = hard_requested.clone();
            let req_ok = hard_requested.clone();
            let action_label = match mode {
                ResetMode::Soft => "Reset --soft",
                ResetMode::Mixed => "Reset --mixed",
                ResetMode::Hard => "Reset --hard",
            };
            dialog
                .kind(DialogKind::Alert)
                .title(native_accessibility::dialog_title(
                    "reset-branch-title",
                    format!(
                        "Reset {} to {}",
                        plan.current.branch,
                        short_oid(&plan.target.oid)
                    ),
                ))
                .width(appearance::dialog_width(px(580.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(form_view)
                .footer(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .child(
                            crate::tags::dialog_cancel("cancel-reset-branch", "Cancel")
                                .debug_selector(|| "cancel-reset-branch".into())
                                .on_action(|_: &Confirm, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                })
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                }),
                        )
                        .child(
                            button("confirm-reset-branch", action_label, "", !is_hard)
                                .debug_selector(|| "confirm-reset-branch".into())
                                .when(is_hard, |btn| btn.danger())
                                .on_action(move |_: &Confirm, _, cx| {
                                    req_key.set(true);
                                    cx.propagate();
                                })
                                .on_click(move |_, window, cx| {
                                    req.set(true);
                                    window.dispatch_action(
                                        Box::new(Confirm { secondary: false }),
                                        cx,
                                    );
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let mode = form_ok.read(cx).mode;
                    if requires_explicit && !req_ok.replace(false) {
                        return false;
                    }
                    req_ok.set(false);
                    if submitted.replace(true) {
                        return true;
                    }
                    let _ = owner.update(cx, |this, cx| {
                        if this.path == repository
                            && this.page == AppPage::Repository
                            && this.operation_busy.is_none()
                        {
                            this.write(
                                WriteCommand::Recovery(
                                    RecoveryCommand::Reset {
                                        plan: (*plan).clone(),
                                        mode,
                                    }
                                    .into(),
                                ),
                                "Resetting branch…",
                                window,
                                cx,
                            );
                        }
                    });
                    true
                })
        });
    }

    pub(super) fn open_stash_selected(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || paths.is_empty() {
            return;
        }
        let path = self.path.clone();
        let owner = cx.entity().downgrade();
        let form = cx.new(|cx| StashForm::new_with_paths(owner, path, Some(paths), window, cx));
        self.recovery.stash = Some(form.clone());
        show_stash_form(form, window, cx);
    }

    fn open_stash_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() {
            return;
        }
        let path = self.path.clone();
        let form = self
            .recovery
            .stash
            .as_ref()
            .filter(|form| {
                let form = form.read(cx);
                form.repository == path && form.selected_paths.is_none()
            })
            .cloned()
            .unwrap_or_else(|| {
                let owner = cx.entity().downgrade();
                cx.new(|cx| StashForm::new(owner, path, window, cx))
            });
        self.recovery.stash = Some(form.clone());
        show_stash_form(form, window, cx);
    }

    pub(super) fn open_stashes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_stash_browser(window, cx);
    }

    /// Opens Stashes, as [`Self::open_stashes`] does, and returns its
    /// browser, which a test waits on for its reads (`StashBrowser::lands`).
    pub(super) fn open_stash_browser(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<StashBrowser>> {
        if self.operation_busy.is_some() {
            return None;
        }
        let repo = self.repository.clone()?;
        let owner = cx.entity().downgrade();
        let browser = cx.new(|cx| StashBrowser::new(owner, repo, window, cx));
        // Stashes opens on its saved stashes, the first real stop.
        let list = browser.read(cx).stash_focus.clone();
        self.show_stashes(browser.clone(), &list, false, window, cx);
        Some(browser)
    }

    /// Opens the Stashes dialog on `browser`, with focus on `focus`, one of
    /// the browser's controls: its saved stashes as it opens, or, opened
    /// again as Drop…'s confirmation is cancelled, the control the press
    /// found focus on ([`Self::reopen_stashes`]). Opened again it is
    /// `returning`, drawn where its entrance ends from its first frame
    /// (the kit's `at_rest`): a return to the dialog the person left, which
    /// slid in again as a new one would.
    fn show_stashes(
        &mut self,
        browser: Entity<StashBrowser>,
        focus: &FocusHandle,
        returning: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let opened = browser.clone();
        self.recovery.browser = Some(browser.downgrade());
        // The dialog holds the browser's only handle, so however it closes,
        // the browser's release stops a Review restore… nobody is waiting
        // for (`StashBrowser::new`).
        window.open_alert_dialog(cx, move |dialog, window, _| {
            dialog
                .when(returning, |dialog| {
                    dialog.entrance(gpui_kit::component::dialog::DialogEntrance::None)
                })
                .title(native_accessibility::dialog_title(
                    "stashes-title",
                    "Stashes",
                ))
                .width(appearance::dialog_width(px(1060.), window))
                .max_h(appearance::dialog_max_height(window))
                // Bordered beside the body's filled Review restore…, the
                // dialog's one default action.
                .footer(crate::tags::dialog_dismiss_footer("Close"))
                // In the dialog's content rather than its scrolling body,
                // whose scroller lays the browser out at the height it asks
                // for and cut its lines mid-line where the cap left less:
                // here the browser gives up what the cap takes.
                .content({
                    let browser = browser.clone();
                    move |content, _, _| content.min_h_0().child(browser.clone())
                })
        });
        // The dialog took focus on its own handle as it opened, which draws
        // nothing: focus goes to `focus`, set now so the first frame draws
        // its ring. The dialog keeps the control focus was on to return to
        // as it closes. Marked, as Settings marks the control it opens on,
        // so the ring shows even where a click opened it.
        let dialog_focus = window.focused(cx);
        opened.update(cx, |browser, _| browser.dialog_focus = dialog_focus);
        focus.focus(window, cx);
        self.focus_ring.mark(window);
        window.refresh();
    }

    /// Drop…'s confirmation cancelled, by Cancel or Escape (an alert never
    /// closes from a press on its backdrop): Stashes, which closed as the
    /// confirmation opened, opens again at rest on the browser it had, its
    /// page, selected stash and inspection as they were, with focus where
    /// the press found it (`acted`), Drop… itself for a key, else the saved
    /// stashes, and reads its page again as Refresh does, saying nothing
    /// unless the stash has gone (`StashBrowser::read_again`). Escape
    /// landed on History, the stash selection gone (QA-109). Only where
    /// Stashes could open: the same repository's page, no write running and
    /// no other dialog open.
    fn reopen_stashes(
        &mut self,
        browser: Entity<StashBrowser>,
        acted: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repository = browser.read(cx).repository.path().to_owned();
        if self.operation_busy.is_some()
            || self.page != AppPage::Repository
            || self.path.as_ref() != Some(&repository)
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        let focus = acted.unwrap_or_else(|| browser.read(cx).stash_focus.clone());
        self.show_stashes(browser.clone(), &focus, true, window, cx);
        browser.update(cx, |browser, cx| browser.read_again(window, cx));
    }

    /// The window's Refresh under a dialog: the open Stashes reads its saved
    /// stashes again ([`StashBrowser::refresh_saved_stashes`]), and any other
    /// modal keeps the key from the page beneath it. Returns whether Stashes
    /// took the key. Answered here, at the window, so it works from the
    /// dialog's footer and shell as from the browser's own controls. Drop…
    /// and Review restore… close Stashes before their confirmation opens,
    /// and the dialog held the browser's only handle, so under either the
    /// browser is gone, the key finds nothing to read, and no list renumbers
    /// behind an alert holding a `stash@{n}`. A dialog opened over a live
    /// Stashes, as a rich preview's Go to page is, holds focus itself and
    /// keeps the key: it acts only while focus lies inside the Stashes
    /// dialog ([`StashBrowser::dialog_focus`]).
    pub(super) fn refresh_open_stashes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if window.has_active_sheet(cx) || !window.has_active_dialog(cx) {
            return false;
        }
        let Some(browser) = self.recovery.browser.as_ref().and_then(WeakEntity::upgrade) else {
            return false;
        };
        let dialog = browser.read(cx).dialog_focus.clone();
        if !dialog.is_some_and(|focus| focus.contains_focused(window, cx)) {
            return false;
        }
        browser.update(cx, |browser, cx| browser.refresh_saved_stashes(window, cx));
        true
    }

    pub(super) fn finish_recovery_write(
        &mut self,
        repository: &PathBuf,
        command: &RecoveryCommand,
        succeeded: bool,
        cx: &mut Context<Self>,
    ) {
        match command {
            RecoveryCommand::Amend { message, .. } => {
                if let Some(form) = &self.recovery.amend
                    && form.read(cx).repository.as_ref() == Some(repository)
                {
                    if succeeded && form.read(cx).message(cx).as_ref() == Some(message) {
                        self.recovery.amend = None;
                    } else if !succeeded {
                        form.update(cx, |form, cx| {
                            form.error = self.tab_operation_error(repository);
                            cx.notify();
                        });
                    }
                }
            }
            RecoveryCommand::CreateStash { name, .. } => {
                if let Some(form) = &self.recovery.stash
                    && form.read(cx).repository.as_ref() == Some(repository)
                {
                    if succeeded && form.read(cx).name.read(cx).value().as_ref() == name {
                        self.recovery.stash = None;
                    } else if !succeeded {
                        form.update(cx, |form, cx| {
                            form.error = self.tab_operation_error(repository);
                            cx.notify();
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// The Working Changes Actions menu's trigger: "Actions" with its caret, or,
/// `compact`, where its label does not fit beside Refresh, a 28 px control
/// that shows the more-actions dots in the shared helper's frame, as
/// Refresh beside it shows its arrows, rather than a bare caret that read
/// as a disclosure arrow. Either is named "Working changes and recovery
/// actions"; the compact one's tooltip leads with "Actions" and, `tight`,
/// names the selection controls its menu then leads with.
pub(super) fn working_actions_trigger(compact: bool, tight: bool) -> Button {
    const NAME: &str = "Working changes and recovery actions";
    const DETAIL: &str = "Ignore selected untracked content, stashes, and last-commit actions";
    const TIGHT_DETAIL: &str = "Directories, Stage and Unstage for the selection, ignore, stashes, and last-commit actions";
    let control = if compact {
        crate::views::compact_control_name(
            button("working-recovery-menu", "", "", false).child(
                div()
                    .debug_selector(|| "working-recovery-more".into())
                    .flex()
                    .child(control_icon("more")),
            ),
            "working-recovery-menu",
            NAME,
        )
        .tooltip(format!(
            "Actions · {}",
            if tight { TIGHT_DETAIL } else { DETAIL }
        ))
    } else {
        // The shared 28 px compact control, as Refresh beside it is, with its
        // caret: the kit's small Button is 1.5 rem, shorter than Refresh.
        button("working-recovery-menu", "Actions", "", false)
            .accessibility_label(NAME)
            .tooltip(DETAIL)
            .dropdown_caret(true)
    };
    control.debug_selector(|| "working-recovery-menu".into())
}

/// The selected commit's Actions menu trigger: "Actions" with its caret in
/// the shared 28 px compact control, as tall as the copy controls beside it
/// and as wide as its label and caret, with the palette's hover and pressed
/// fills (the kit's small Button is 1.5 rem); or, not `labelled`, a 28 px
/// control showing the more-actions dots in the helper's frame, as Working's
/// compact Actions does. Either is named "Selected commit actions"; the
/// compact one's tooltip leads with "Actions".
pub(super) fn commit_actions_trigger(labelled: bool) -> Button {
    const NAME: &str = "Selected commit actions";
    const DETAIL: &str = "Revert or cherry-pick this commit, or amend or undo the last commit";
    let control = if labelled {
        button("commit-recovery-menu", "Actions", "", false)
            .accessibility_label(NAME)
            .tooltip(DETAIL)
            .dropdown_caret(true)
    } else {
        crate::views::compact_control_name(
            button("commit-recovery-menu", "", "", false).child(
                div()
                    .debug_selector(|| "commit-recovery-more".into())
                    .flex()
                    .child(control_icon("more")),
            ),
            "commit-recovery-menu",
            NAME,
        )
        .tooltip(format!("Actions · {DETAIL}"))
    };
    control.debug_selector(|| "commit-recovery-menu".into())
}

fn working_menu_item(
    menu: PopupMenu,
    label: &'static str,
    refused: bool,
    wait: Option<native_accessibility::MenuWait>,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    action: impl Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) + 'static,
) -> PopupMenu {
    menu.item(working_item(
        label, refused, wait, owner, repository, action,
    ))
}

/// A Working Changes or commit menu item that acts only on the repository
/// it was built for, on the repository page, while no operation runs:
/// unavailable while `refused` for a reason of its own, or while `wait`
/// says a write refuses it, which it then states
/// (`native_accessibility::waiting_item`).
fn working_item(
    label: impl Into<SharedString>,
    refused: bool,
    wait: Option<native_accessibility::MenuWait>,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    action: impl Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) + 'static,
) -> PopupMenuItem {
    let owner = owner.clone();
    let repository = repository.clone();
    native_accessibility::waiting_item(label, refused, wait).on_click(move |_, window, cx| {
        let _ = owner.update(cx, |this, cx| {
            if this.path == repository
                && this.page == AppPage::Repository
                && this.operation_busy.is_none()
            {
                action(this, window, cx);
            }
        });
    })
}

#[expect(clippy::too_many_arguments)]
fn commit_menu(
    mut menu: PopupMenu,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    commit: &Arc<CommitTarget>,
    (paused, wait): (bool, Option<native_accessibility::MenuWait>),
    current: bool,
    compare_with: Option<String>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    for (label, kind) in [
        ("Revert this commit…", RecoveryKind::Revert),
        ("Cherry-pick this commit…", RecoveryKind::CherryPick),
    ] {
        let commit = Arc::clone(commit);
        menu = working_menu_item(
            menu,
            label,
            paused,
            wait,
            owner,
            repository,
            move |this, window, cx| {
                this.choose_commit_recovery(kind, Arc::clone(&commit), window, cx)
            },
        );
    }
    if current {
        menu = menu.separator();
        for (label, kind) in [
            ("Amend last commit…", RecoveryKind::Amend),
            ("Undo last local commit…", RecoveryKind::Undo),
        ] {
            let commit = Arc::clone(commit);
            menu = working_menu_item(
                menu,
                label,
                paused,
                wait,
                owner,
                repository,
                move |this, window, cx| {
                    this.choose_commit_recovery(kind, Arc::clone(&commit), window, cx)
                },
            );
        }
    }
    let owner_more = owner.clone();
    let repo_more = repository.clone();
    let commit_more = Arc::clone(commit);
    menu.separator()
        .submenu("More actions", window, cx, move |mut sub, _, _| {
            {
                let oid = commit_more.oid.clone();
                sub = working_menu_item(
                    sub,
                    "Reset branch here…",
                    paused,
                    wait,
                    &owner_more,
                    &repo_more,
                    move |this, window, cx| this.open_reset_commit(oid.clone(), window, cx),
                );
            }
            {
                let oid = commit_more.oid.clone();
                sub = working_menu_item(
                    sub,
                    "Create branch here…",
                    paused,
                    wait,
                    &owner_more,
                    &repo_more,
                    move |this, window, cx| this.open_create_branch_at(oid.clone(), window, cx),
                );
            }
            {
                let oid = commit_more.oid.clone();
                sub = working_menu_item(
                    sub,
                    "Create tag here…",
                    paused,
                    wait,
                    &owner_more,
                    &repo_more,
                    move |this, window, cx| this.open_create_tag_at(Some(oid.clone()), window, cx),
                );
            }
            {
                let oid = commit_more.oid.clone();
                sub = working_menu_item(
                    sub,
                    "Checkout detached…",
                    paused,
                    wait,
                    &owner_more,
                    &repo_more,
                    move |this, window, cx| this.open_checkout_detached(oid.clone(), window, cx),
                );
            }
            {
                let oid = commit_more.oid.clone();
                sub = working_menu_item(
                    sub,
                    "Interactive rebase…",
                    paused,
                    wait,
                    &owner_more,
                    &repo_more,
                    move |this, window, cx| {
                        this.open_interactive_rebase(Some(oid.clone()), window, cx)
                    },
                );
            }
            sub = sub.separator();
            if !current {
                let oid = commit_more.oid.clone();
                sub = working_menu_item(
                    sub,
                    "Compare with HEAD",
                    false,
                    wait,
                    &owner_more,
                    &repo_more,
                    move |this, window, cx| {
                        this.compare_revisions_direct(
                            oid.clone(),
                            "HEAD".into(),
                            ComparisonMode::Endpoints,
                            window,
                            cx,
                        )
                    },
                );
            }
            {
                let oid = commit_more.oid.clone();
                sub = working_menu_item(
                    sub,
                    "Compare with working tree",
                    false,
                    wait,
                    &owner_more,
                    &repo_more,
                    move |this, window, cx| {
                        this.compare_revision_with_working(oid.clone(), window, cx)
                    },
                );
            }
            if let Some(other) = &compare_with {
                let oid = commit_more.oid.clone();
                let other = other.clone();
                sub = working_menu_item(
                    sub,
                    "Compare selected",
                    false,
                    wait,
                    &owner_more,
                    &repo_more,
                    move |this, window, cx| {
                        this.compare_revisions_direct(
                            other.clone(),
                            oid.clone(),
                            ComparisonMode::Endpoints,
                            window,
                            cx,
                        )
                    },
                );
            }
            sub = sub.separator();
            {
                let oid = commit_more.oid.clone();
                sub = sub.item(
                    PopupMenuItem::new("Copy hash").on_click(move |_, window, cx| {
                        copy_feedback::copy(window, cx, None, "commit hash", oid.clone());
                    }),
                );
            }
            {
                let subject = commit_more.subject.clone();
                sub = sub.item(PopupMenuItem::new("Copy subject").on_click(
                    move |_, window, cx| {
                        copy_feedback::copy(window, cx, None, "commit subject", subject.clone());
                    },
                ));
            }
            {
                let owner = owner_more.clone();
                let repository = repo_more.clone();
                let oid = commit_more.oid.clone();
                sub = sub.item(
                    PopupMenuItem::new("Copy patch").on_click(move |_, window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if this.path == repository && this.page == AppPage::Repository {
                                this.copy_commit_patch(oid.clone(), window, cx);
                            }
                        });
                    }),
                );
            }
            sub
        })
}

fn append_paths(explanation: &mut String, paths: &[PathBuf]) {
    explanation.push_str(&format!(
        "\n\n{}",
        crate::counted(paths.len(), "affected path", "affected paths")
    ));
    for path in paths.iter().take(24) {
        explanation.push_str(&format!("\n{}", path.display()));
    }
    if paths.len() > 24 {
        explanation.push_str(&format!("\n… and {} more", paths.len() - 24));
    }
}

fn split_message(message: &str) -> CommitDraft {
    let (title, description) = message.split_once('\n').unwrap_or((message, ""));
    CommitDraft {
        title: title.to_owned(),
        description: description
            .strip_prefix('\n')
            .unwrap_or(description)
            .to_owned(),
    }
}

fn amended_message(original: &str, initial: &CommitDraft, draft: &CommitDraft) -> String {
    if draft == initial {
        original.to_owned()
    } else {
        draft.message()
    }
}

/// What a press of Refresh waits to say ([`StashBrowser::say_refreshed`]).
#[derive(Clone, Copy, PartialEq)]
enum SpokenRefresh {
    /// What the landed page shows.
    Pending,
    /// Nothing, unless the inspected stash is no longer listed: the read
    /// Stashes opened again makes as Drop…'s confirmation is cancelled
    /// ([`StashBrowser::read_again`]), which no press asked for but which
    /// can clear the selection the person returned to.
    Quiet,
    /// The inspected stash is no longer listed.
    Gone,
}

/// The words a press of Refresh says for a page of the saved stashes: the
/// count where the page is the whole list, and otherwise which of them it
/// shows, as the heading does, one stash read as itself: a page of one once
/// said "Saved stashes 41 to 41".
fn spoken_page(offset: usize, next_offset: Option<usize>, count: usize) -> String {
    match (offset, next_offset, count) {
        (_, _, 0) => "No saved stashes".to_owned(),
        (0, None, 1) => "1 saved stash".to_owned(),
        (0, None, count) => format!("{count} saved stashes"),
        (offset, _, 1) => format!("Saved stash {}", offset + 1),
        (offset, _, count) => format!("Saved stashes {} to {}", offset + 1, offset + count),
    }
}

/// A saved stash's row ID: its stash commit, rather than its `stash@{n}`,
/// which renumbers as a stash is saved above it or dropped below it, so the
/// window's accessibility tree keeps one node for each stash as the list is
/// read again. The commit is hashed, so the ID costs no allocation as each
/// frame lays the rows out; its debug selector is still its place.
fn stash_row_id(stash: &StashEntry) -> ElementId {
    use std::hash::{Hash, Hasher};
    let mut commit = std::hash::DefaultHasher::new();
    stash.oid.hash(&mut commit);
    ElementId::NamedInteger("saved-stash".into(), commit.finish())
}

/// A saved stash's row's accessible name: its message, its date and its
/// short commit, none of which renumbers, never its `stash@{n}`: two stashes
/// saved with one message are told apart by when, then by what. Refresh after a
/// stash saved elsewhere keeps the selected stash selected where the list
/// renumbered it, its row the same node, and the refresh's one sentence says
/// where it now stands ("4 saved stashes, stash@{2} selected"): Orca speaks
/// any rename of the item it presents, so a name holding the `stash@{n}` was
/// heard a second time 12 ms later ("On main: Stash 2: notes edit ·
/// stash@{2}", QA-109). The row still shows its `stash@{n}`, and the
/// selected stash's detail line says it, static text Orca does not present,
/// so a renumber is still said once, by the refresh's sentence.
fn stash_row_name(stash: &StashEntry) -> String {
    format!(
        "{} · {} · {}",
        stash.name,
        short_date(stash.timestamp),
        short_oid(&stash.oid)
    )
}

fn list_key_selection(key: &str, selected: Option<usize>, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    match key {
        "up" => Some(selected.unwrap_or(0).saturating_sub(1).min(count - 1)),
        "down" => Some(selected.map_or(0, |index| index.saturating_add(1).min(count - 1))),
        "home" => Some(0),
        "end" => Some(count - 1),
        _ => None,
    }
}

/// What Amend edits, beside what the fields opened with.
struct AmendFields {
    title: Entity<InputState>,
    description: Entity<TextareaState>,
    initial: CommitDraft,
}
impl AmendFields {
    /// The message these fields amend `original` with: `original` byte for
    /// byte unless they were edited.
    fn message(&self, original: &str, cx: &App) -> String {
        let draft = CommitDraft {
            title: self.title.read(cx).value().to_string(),
            description: self.description.read(cx).value().to_string(),
        };
        amended_message(original, &self.initial, &draft)
    }
}
struct AmendForm {
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    plan: Arc<CommitRecoveryPlan>,
    /// `None` for a message over the most Amend can submit, which is not
    /// loaded ([`CommitRecoveryPlan::message_too_long`]): the form says so
    /// in their place and offers no review.
    fields: Option<AmendFields>,
    error: Option<String>,
    /// Whether the line beneath the fields says that what its last review
    /// was refused for has gone, the integration paused or the conflicts
    /// left ([`pause_ended_line`]).
    pause_ended: Option<RefusedFor>,
    /// How often the form has been shown, which gives its refusal a fresh
    /// node each time it opens again (`native_accessibility::refusal_line`).
    opened: usize,
}
impl AmendForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repository: Option<PathBuf>,
        plan: Arc<CommitRecoveryPlan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fields = (!plan.message_too_long).then(|| {
            let initial = split_message(&plan.target.message);
            let title =
                cx.new(|cx| InputState::new(window, cx).default_value(initial.title.clone()));
            let description = cx.new(|cx| {
                TextareaState::new(window, cx).default_value(initial.description.clone())
            });
            AmendFields {
                title,
                description,
                initial,
            }
        });
        Self {
            owner,
            repository,
            plan,
            fields,
            error: None,
            pause_ended: None,
            opened: 0,
        }
    }
    /// The message this form would amend with; `None` where it has no fields.
    fn message(&self, cx: &App) -> Option<String> {
        (self.fields.as_ref()).map(|fields| fields.message(&self.plan.target.message, cx))
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.pause_ended = None;
        let Some(fields) = &self.fields else {
            return true;
        };
        if fields.title.read(cx).value().trim().is_empty() {
            self.error = Some("Enter a commit title.".into());
            cx.notify();
            return false;
        }
        let message = fields.message(&self.plan.target.message, cx);
        let expected = Arc::clone(&self.plan);
        let form = cx.entity();
        let failure: Failure = Box::new(move |error, window, cx| {
            form.update(cx, |form, cx| {
                form.error = Some(error);
                cx.notify();
            });
            show_amend(form, window, cx);
        });
        self.owner.update(cx, |this, cx| {
            if this.path != self.repository { return false; }
            this.read_recovery(move |repo| {
                let plan = repo.recovery_plan(RecoveryKind::Amend, Some(&expected.target.oid), None)?;
                anyhow::ensure!(plan == *expected, "The branch or staged contents changed. Close this form and choose Amend again; your message is retained.");
                let mut explanation = format!("Replace {} on {} with a new commit using the staged contents and this message. {} currently staged paths will be included in the amendment. Unstaged working files remain available.\n\n{}", short_oid(&plan.target.oid), plan.current.branch, plan.staged_paths.len(), message);
                if !plan.known_published_refs.is_empty() { explanation.push_str(&format!("\n\nThis commit is reachable from locally available remote-tracking refs: {}. Amending rewrites its identity; coordinate with anyone using this history.", plan.known_published_refs.iter().take(12).map(|reference| reference.name.as_str()).collect::<Vec<_>>().join(", "))); }
                append_paths(&mut explanation, &plan.affected_paths);
                explanation.push_str("\n\nGit uses its configured hooks and signing. No remote is changed.");
                Ok(Prepared::Confirmation { title: "Amend last commit".into(), explanation, action: "Amend commit", command: RecoveryCommand::Amend { plan, message }.into() })
            }, Some(failure), window, cx)
        }).unwrap_or(false)
    }
}
impl Render for AmendForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(p.muted))
                    .child(format!(
                        "{} · {} · {} staged paths",
                        self.plan.current.branch,
                        short_oid(&self.plan.target.oid),
                        self.plan.staged_paths.len()
                    )),
            )
            .map(|form| match &self.fields {
                Some(fields) => form
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child("Title")
                            .child(Input::new(&fields.title).aria_label("Amended commit title")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child("Description · optional")
                            .child(shortcuts::tab_leaves(
                                Textarea::new(&fields.description)
                                    .h(px(170.))
                                    .aria_label("Amended commit description"),
                            )),
                    )
                    .child(
                        div()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(p.muted))
                            .child(
                                "The next step reviews the staged snapshot and the effect on history.",
                            ),
                    ),
                None => form.child(native_accessibility::text(
                    "amend-message-too-long",
                    format!(
                        "This commit's message is over {} KiB, too long to edit here",
                        MAX_RECOVERY_MESSAGE_BYTES / 1024
                    ),
                )),
            })
            .children(form_line(
                self.error.as_deref(),
                self.pause_ended,
                REVIEW_AMEND,
                self.opened,
                cx,
            ))
    }
}
fn show_amend(form: Entity<AmendForm>, window: &mut Window, cx: &mut App) {
    form.update(cx, |form, _| form.opened += 1);
    let editable = form.read(cx).fields.is_some();
    window.open_alert_dialog(cx, move |dialog, window, _| {
        let submit = form.clone();
        let dialog = dialog
            .title("Amend last commit")
            .width(appearance::dialog_width(px(580.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(form.clone());
        if editable {
            dialog
                .footer(crate::tags::dialog_footer(REVIEW_AMEND, Some("Cancel")))
                .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
        } else {
            // Nothing to review, so Close alone, the default a lone footer
            // action is: Enter closes the form, as Escape does.
            dialog.footer(crate::tags::dialog_footer("Close", None))
        }
    });
}

struct ResetForm {
    plan: Arc<ResetPlan>,
    mode: ResetMode,
}

impl Render for ResetForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let mode_detail = match self.mode {
            ResetMode::Soft => {
                "Moves HEAD only. Keeps all staged index changes and working tree files intact."
            }
            ResetMode::Mixed => {
                "Moves HEAD and resets the index. Keeps your working tree changes as unstaged edits."
            }
            ResetMode::Hard => {
                "Moves HEAD, resets the index, and permanently discards uncommitted changes in tracked working files."
            }
        };
        let dirty_summary = if self.plan.dirty_staged == 0 && self.plan.dirty_unstaged == 0 {
            "Working tree has no tracked staged or unstaged edits.".to_owned()
        } else {
            format!(
                "Current working tree: {} staged, {} unstaged.",
                crate::counted(self.plan.dirty_staged, "file", "files"),
                crate::counted(self.plan.dirty_unstaged, "file", "files"),
            )
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(crate::appearance::ui_text(12.))
            .text_color(rgb(p.text))
            .child(
                div()
                    .p_2p5()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(p.border))
                    .bg(rgb(p.panel))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!(
                        "Branch {} · current HEAD {}",
                        gitturtle_core::strip_bidi_controls(&self.plan.current.branch),
                        short_oid(&self.plan.current.head)
                    ))
                    .child(format!(
                        "Target {} · {}",
                        short_oid(&self.plan.target.oid),
                        gitturtle_core::strip_bidi_controls(&self.plan.target.subject)
                    ))
                    .child(
                        div()
                            .text_size(crate::appearance::ui_text(11.))
                            .text_color(rgb(p.muted))
                            .child(dirty_summary),
                    ),
            )
            .when(!self.plan.known_published_refs.is_empty(), |el| {
                el.child(
                    div()
                        .p_2p5()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(p.warning))
                        .bg(rgb(p.panel))
                        .text_size(crate::appearance::ui_text(11.))
                        .child(format!(
                            "Locally known remote references containing current HEAD: {}. Resetting will rewrite history relative to them.",
                            self.plan
                                .known_published_refs
                                .iter()
                                .map(|reference| {
                                    gitturtle_core::strip_bidi_controls(&reference.name)
                                        .into_owned()
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1p5()
                    .children([
                        (ResetMode::Soft, "reset-mode-soft", "Soft", "Reset mode: Soft"),
                        (ResetMode::Mixed, "reset-mode-mixed", "Mixed", "Reset mode: Mixed"),
                        (ResetMode::Hard, "reset-mode-hard", "Hard", "Reset mode: Hard"),
                    ].into_iter().map(|(mode, id, label, a11y)| {
                        button(id, label, "", self.mode == mode)
                            .debug_selector(move || id.into())
                            .accessibility_label(a11y)
                            .toggled(self.mode == mode)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.mode = mode;
                                cx.notify();
                            }))
                    })),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(if self.mode == ResetMode::Hard {
                        rgb(p.warning)
                    } else {
                        rgb(p.muted)
                    })
                    .child(mode_detail),
            )
    }
}

struct StashForm {
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    selected_paths: Option<Vec<PathBuf>>,
    name: Entity<InputState>,
    include_untracked: bool,
    error: Option<String>,
    /// Whether the line beneath the fields says that what its last review
    /// was refused for has gone, the integration paused or the conflicts
    /// left ([`pause_ended_line`]).
    pause_ended: Option<RefusedFor>,
    /// How often the form has been shown, which gives its refusal a fresh
    /// node each time it opens again (`native_accessibility::refusal_line`).
    opened: usize,
}
impl StashForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repository: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_paths(owner, repository, None, window, cx)
    }
    fn new_with_paths(
        owner: WeakEntity<GitTurtle>,
        repository: Option<PathBuf>,
        selected_paths: Option<Vec<PathBuf>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx
            .new(|cx| InputState::new(window, cx).placeholder("Describe the work you are pausing"));
        Self {
            owner,
            repository,
            selected_paths,
            name,
            include_untracked: false,
            error: None,
            pause_ended: None,
            opened: 0,
        }
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.pause_ended = None;
        let name = self.name.read(cx).value().to_string();
        if name.trim().is_empty() {
            self.error = Some("Give this stash a name so you can find it later.".into());
            cx.notify();
            return false;
        }
        let include_untracked = self.include_untracked;
        let selected_paths = self.selected_paths.clone();
        let form = cx.entity();
        let failure: Failure = Box::new(move |error, window, cx| {
            form.update(cx, |form, cx| {
                form.error = Some(error);
                cx.notify();
            });
            show_stash_form(form, window, cx);
        });
        self.owner.update(cx, |this, cx| {
            if this.path != self.repository { return false; }
            this.read_recovery(move |repo| {
                let plan = repo.stash_create_plan_for_paths(include_untracked, selected_paths.as_deref())?;
                let scope_note = if let Some(paths) = &plan.selected_paths {
                    format!(
                        "Only the {} chosen will be stashed and cleared from this working copy; other files stay intact.",
                        crate::counted(paths.len(), "selected file", "selected files")
                    )
                } else if include_untracked {
                    "Tracked staged/unstaged changes and untracked files are saved and removed from this working copy. Ignored files remain.".to_owned()
                } else {
                    "Tracked staged and unstaged changes will be saved separately, then removed from this working copy. Untracked and ignored files remain.".to_owned()
                };
                let mut explanation = format!("Save “{name}” from {}. {scope_note}", plan.current.branch);
                append_paths(&mut explanation, &plan.affected_paths);
                Ok(Prepared::Confirmation { title: "Save changes to stash".into(), explanation, action: "Save stash", command: RecoveryCommand::CreateStash { plan, name }.into() })
            }, Some(failure), window, cx)
        }).unwrap_or(false)
    }
}
impl Render for StashForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div().flex().flex_col().gap_3()
            .child(div().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child(
                match &self.selected_paths {
                    Some(paths) => format!(
                        "Pause {} in a named snapshot while keeping other working files untouched.",
                        crate::counted(paths.len(), "selected file", "selected files")
                    ),
                    None => "Pause the current work and keep a named snapshot you can inspect before restoring.".to_owned(),
                }
            ))
            .child(div().flex().flex_col().gap_1().child("Stash name").child(Input::new(&self.name).aria_label("Stash name")))
            .child(Checkbox::new("stash-include-untracked").label("Include untracked files").checked(self.include_untracked)
                .on_click(cx.listener(|this, checked, _, cx| { this.include_untracked = *checked; cx.notify(); })))
            .children(form_line(
                self.error.as_deref(),
                self.pause_ended,
                REVIEW_STASH,
                self.opened,
                cx,
            ))
    }
}
fn show_stash_form(form: Entity<StashForm>, window: &mut Window, cx: &mut App) {
    form.update(cx, |form, _| form.opened += 1);
    window.open_alert_dialog(cx, move |dialog, window, _| {
        let submit = form.clone();
        dialog
            .title("Save changes to stash")
            .width(appearance::dialog_width(px(520.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(form.clone())
            .footer(crate::tags::dialog_footer("Review stash", Some("Cancel")))
            .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
    });
}
/// Replaces a recovery form's `error`, or Stashes' refusal, given for a
/// cause a status read has found gone with the line that says so, `ended`,
/// which keeps what it was for; returns that where it did. Where
/// `conflicts`, only a refusal for conflicts alone goes, once none remain;
/// otherwise only one for a paused operation, once it has ended, a refusal
/// for conflicts alone staying since that end resolves none of them. Any
/// other reason stays.
fn refusal_ended(
    error: &mut Option<String>,
    ended: &mut Option<RefusedFor>,
    conflicts: bool,
) -> Option<RefusedFor> {
    let cause = error
        .as_deref()
        .and_then(gitturtle_core::paused_recovery_operation)
        .filter(|cause| cause.is_none() == conflicts)?;
    *error = None;
    *ended = Some(cause);
    Some(cause)
}

/// The line beneath a recovery form's fields: why its last review was
/// refused, an alert announced at once, again by a fresh node each time the
/// form opens again with it (`opened`, `native_accessibility::refusal_line`),
/// or, `pause_ended`, that the integration it was refused for has ended,
/// naming the form's `review` button, a status heard once assistive
/// technology finishes speaking. The refusal was no live region, so it went
/// unannounced as it first appeared and as the form opened again with it.
fn form_line(
    error: Option<&str>,
    pause_ended: Option<RefusedFor>,
    review: &str,
    opened: usize,
    cx: &App,
) -> Option<AnyElement> {
    match error {
        Some(error) => Some(native_accessibility::refusal_line(
            "recovery-form-error",
            opened,
            error.to_owned(),
            |line| {
                line.max_h(px(130.))
                    .overflow_y_scroll()
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(palette(cx).warning))
            },
        )),
        None => pause_ended.map(|operation| {
            let line = pause_ended_line(operation, review);
            native_accessibility::static_text(
                div()
                    .id("recovery-form-note")
                    .debug_selector(|| "recovery-form-note".into())
                    .role(Role::Status)
                    .aria_label(line.clone())
                    .polite()
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(palette(cx).muted))
                    .child(line.clone()),
                line,
            )
            .into_any_element()
        }),
    }
}

fn form_error(error: &str, cx: &App) -> AnyElement {
    div()
        .id("recovery-form-error")
        .debug_selector(|| "recovery-form-error".into())
        .max_h(px(130.))
        .overflow_y_scroll()
        .text_size(crate::appearance::ui_text(12.))
        .text_color(rgb(palette(cx).warning))
        .child(error.to_owned())
        .into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StashArea {
    Staged,
    Unstaged,
    Untracked,
}
impl StashArea {
    fn label(self) -> &'static str {
        match self {
            Self::Staged => "Staged",
            Self::Unstaged => "Unstaged",
            Self::Untracked => "Untracked",
        }
    }
    fn files(self, snapshot: &StashSnapshot) -> &[FileChange] {
        match self {
            Self::Staged => &snapshot.staged,
            Self::Unstaged => &snapshot.unstaged,
            Self::Untracked => &snapshot.untracked,
        }
    }
}

/// This transient inspector owns one bounded metadata executor and one
/// replaceable preview worker. Closing it releases both and its preview cache;
/// its replies never replace the main workspace's current comparison.
pub(crate) struct StashBrowser {
    owner: WeakEntity<GitTurtle>,
    repository: GitRepository,
    metadata: SerialExecutor,
    preview_worker: Worker,
    generation: u64,
    preview_generation: u64,
    /// This showing of the file preview, which a line beneath an open
    /// control belongs to; [`Self::clear_preview`] starts a new one.
    showing: crate::system_preview::Showing,
    /// The chosen stash's read of its saved changes.
    metadata_task: Option<Task<()>>,
    /// The list's read, apart from the chosen stash's: a stash chosen while
    /// the list is read, which the list still draws, starts its own read
    /// and leaves the list's to land ([`Self::load_page`]), which only
    /// another read of the list replaces, by `list_generation`. Sharing one
    /// task and generation, the click's read replaced the list's, and
    /// Refresh's answer never landed.
    list_task: Option<Task<()>>,
    list_generation: u64,
    /// Whether the list's running read is the Refresh key's
    /// ([`Self::refresh_saved_stashes`]), which leaves the browser as it is and
    /// so does not count as reading ([`Self::reading`]).
    list_quiet: bool,
    /// Whether Stashes, opened again while a read of its list or of the
    /// selected stash still ran ([`Self::read_again`]), owes the read it
    /// could not start: made once that read ends, so a change made under
    /// Drop…'s confirmation shows. The running read began before it, and
    /// its answer was all Stashes showed.
    read_owed: bool,
    /// What a press of Refresh waits to say once its read has landed
    /// ([`Self::say_refreshed`]), once per press; any other read of the list
    /// says nothing.
    spoken_refresh: Option<SpokenRefresh>,
    preview_task: Option<Task<()>>,
    page: Option<StashPage>,
    offset: usize,
    selected: Option<usize>,
    snapshot: Option<Arc<StashSnapshot>>,
    area: StashArea,
    selected_file: Option<usize>,
    content: Option<Arc<Content>>,
    text_mode: usize,
    editors: [Option<Entity<EditorState>>; 3],
    /// The patch editor's decorations, kept for its life like Compare's.
    decoration: Option<crate::editor_find::PatchDecorations>,
    patch_view: Option<Entity<diff_view::DiffView>>,
    stash_scroll: UniformListScrollHandle,
    /// The detail's scroll, back at its top for each stash it inspects,
    /// which reveals the control the keyboard moves focus onto.
    detail: crate::focus_reveal::FocusReveal,
    file_scroll: UniformListScrollHandle,
    stash_focus: FocusHandle,
    /// The Stashes dialog's own focus, which holds everything drawn in it,
    /// its body, footer and shell, and none of a dialog opened over it
    /// ([`GitTurtle::refresh_open_stashes`]).
    dialog_focus: Option<FocusHandle>,
    file_focus: FocusHandle,
    /// Refresh's focus, which takes focus where the read that opened Stashes
    /// finds nothing saved ([`Self::opening`]).
    refresh_focus: FocusHandle,
    /// Whether the list's first read has yet to land. With nothing saved it
    /// moves focus, while it still lies on the list where the dialog put
    /// it, to Refresh, the first control that can take it, the list holding
    /// no stash to reach; every later read leaves focus where it is.
    opening: bool,
    /// Tracked, never a Tab stop, by the column that holds the chosen
    /// stash's inspection, the detail with its footer: whether focus lies
    /// anywhere inside it ([`Self::release_inspection_focus`]), on a control
    /// the detail lists for its reveal or on one a comparison draws with a
    /// handle of its own, as a model's canvas and timeline do, or any a kind
    /// of content adds later. A press in the column never focuses it: its
    /// controls take focus as they did, and a press on anything else there
    /// leaves focus where it was.
    inspection_focus: FocusHandle,
    loading: Option<&'static str>,
    error: Option<String>,
    preview_error: Option<String>,
    /// The line a failing Git LFS side says above the saved file's preview.
    lfs_failure_line: crate::lfs_download::FailureLine,
    restore_index: bool,
    /// What Review restore… asked the core to prepare and the window's
    /// recovery read preparing it, until it answers. Choosing another stash
    /// or page, the other Restore staged state, or closing Stashes cancels
    /// it ([`Self::cancel_restore`]), and an answer to anything else is
    /// dropped ([`Self::answered`]): a review or refusal once landed for the
    /// stash or choice shown before.
    restoring: Option<(RestoreRequest, u64)>,
    /// Why Review restore… was refused, which the footer note beside it
    /// says in place of its explanation until another stash is chosen:
    /// the core's refusal for a paused integration, or any other answer,
    /// such as Git's report, which Details… opens whole.
    restore_line: Option<String>,
    /// The window's alert that says that refusal ([`Self::refuse`]) until a
    /// frame draws the note: a stash chosen before then, as a key or click
    /// handled ahead of the display's next frame chooses one, drops the note
    /// unshown and takes the alert back unheard
    /// ([`Self::forget_restore_line`]). Once a frame has drawn the note, the
    /// alert has been said with it and stays, however far the detail has
    /// scrolled the note.
    restore_alert: Option<crate::copy_feedback::Alerted>,
    /// The window's alert that says a failed read of the list or of the
    /// stash chosen (`tags::alert_error_line`) until a frame draws its
    /// line: another stash or page chosen before then takes it back
    /// unheard ([`Self::forget_read_alert`]), as Review restore…'s refusal
    /// is, so a failure for a stash the person has just moved off is never
    /// said.
    read_alert: Option<crate::copy_feedback::Alerted>,
    /// Whether that note says that what Review restore… was last refused
    /// for has gone, the integration paused or the conflicts left
    /// ([`pause_ended_line`]).
    pause_ended: Option<RefusedFor>,
    /// The next frame scrolls the stacked detail to the footer note, for
    /// why it changed: an answer to Review restore…'s press, a refusal or
    /// failure, has just replaced it, which shows while focus stays where
    /// the press found it, or the end of the pause it was refused for;
    /// either scrolls nothing while focus has moved onto another of the
    /// detail's controls ([`reveal_note`]). The next frame that draws
    /// Stashes takes it as it renders, so it never outlives that frame.
    reveal_note: std::rc::Rc<std::cell::Cell<Option<NoteReveal>>>,
    /// The lines a refusal keeps in the stacked footer note: two, or one
    /// where the detail cannot hold two beneath Review restore… whole with
    /// its ring, as at 18 pt in a 461 × 490 window ([`reveal_note`]).
    note_lines: std::rc::Rc<std::cell::Cell<usize>>,
    /// Where Review restore… lies in the stacked detail in the frame being
    /// prepainted, for [`reveal_note`].
    restore_at: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// How each drawn stash row's message, and the detail's title, was laid
    /// out last ([`MessageLaidOut`]), which its tooltip reads as the pointer
    /// rests on it: at most one entry for each row of a page, and the title's.
    messages_laid_out: LaidOutMessages,
}

/// Where a saved stash's message is drawn cut to its room: a row of the list,
/// by its index on the page, or the detail's title.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MessageLine {
    Row(usize),
    Title,
}

type LaidOutMessages =
    std::rc::Rc<std::cell::RefCell<std::collections::HashMap<MessageLine, MessageLaidOut>>>;

/// A saved stash's message as its line last laid it out: the room it had,
/// the size, font and display scale it was drawn at, and whether that room
/// cut it, measured only as one of them changes ([`measure_message`]). Keyed
/// by its size alone, a line kept the answer a font family or a display
/// scale since changed had given.
struct MessageLaidOut {
    message: String,
    width: Pixels,
    size: Pixels,
    font: gpui::Font,
    scale: f32,
    cut: bool,
}

/// Measures `message` as `line` lays it out at `size`, in the text style the
/// element that calls it prepaints in, so its font and weight measure it,
/// and records whether its room cuts it ([`message_tooltip`]). It answers for
/// the line as laid out last, so a hover after a resize, a text size or font
/// change, or a move to a display of another scale answers for the line
/// drawn.
fn measure_message(
    laid_out: &LaidOutMessages,
    line: MessageLine,
    message: String,
    size: Pixels,
) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    let recorded = laid_out.clone();
    move |room, window, _| {
        let width = room.size.width;
        let style = window.text_style();
        let font = style.font();
        let scale = window.scale_factor();
        let mut laid = recorded.borrow_mut();
        if laid.get(&line).is_some_and(|laid| {
            laid.width == width
                && laid.size == size
                && laid.font == font
                && laid.scale == scale
                && laid.message == message
        }) {
            return;
        }
        let run = style.to_run(message.len());
        let cut = window
            .text_system()
            .shape_line(message.clone().into(), size, &[run], None)
            .width()
            > width;
        laid.insert(
            line,
            MessageLaidOut {
                message,
                width,
                size,
                font,
                scale,
                cut,
            },
        );
    }
}

/// The tooltip of a stash's message drawn on `line`: the whole message
/// where its room cut it as last laid out ([`measure_message`]), and
/// otherwise nothing, as no line drawn whole has one (`DESIGN.md`, Image
/// comparison's rule for a line's tooltip).
fn message_tooltip(
    laid_out: &LaidOutMessages,
    line: MessageLine,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let laid_out = laid_out.clone();
    move |window, cx| {
        let cut = laid_out
            .borrow()
            .get(&line)
            .filter(|laid| laid.cut)
            .map(|laid| laid.message.clone());
        match cut {
            Some(message) => gpui_kit::component::tooltip::Tooltip::new(message).build(window, cx),
            // Nothing to add to a whole message: an empty view, drawn as
            // nothing.
            None => cx.new(|_| gpui::EmptyView).into(),
        }
    }
}

impl Drop for StashBrowser {
    fn drop(&mut self) {
        crate::model_view::pause(self.content.as_deref());
    }
}

/// What Stashes' Review restore… asked the core to prepare: the stash, by
/// its object, whether to restore its staged state, and the browser's
/// generation, which choosing a stash or reading the list again moves on.
#[derive(Clone, Debug, PartialEq)]
struct RestoreRequest {
    stash: String,
    restore_index: bool,
    generation: u64,
}

/// Why the stacked Stashes detail reveals its footer note ([`reveal_note`]).
#[derive(Clone, Debug, PartialEq, Eq)]
enum NoteReveal {
    /// Review restore… was pressed and answered, refused or failed
    /// ([`StashBrowser::refuse`]), with the focus the press found, if any:
    /// the note is the answer to that press and shows while focus stays
    /// there, on Review restore… or outside the detail. A pointer's press
    /// focuses no Button, so a click left focus on the saved files or a
    /// document's open control, and the answer below the fold showed
    /// nothing. Once the keyboard has moved focus onto another of the
    /// detail's controls while Git prepared, as on a large stash, the
    /// answer is heard from the window's alert, as the end of a pause is
    /// from its announcement, and the control the keyboard reached stays in
    /// view: the late answer scrolled it away.
    Pressed(Option<WeakFocusHandle>),
    /// A status read found the pause it was refused for ended
    /// ([`GitTurtle::integration_ended`]), which no press asked for: the
    /// note shows only while Review restore… or nothing in the detail holds
    /// focus, so a merge finished in a terminal never scrolls the control
    /// the keyboard left focused out of view.
    PauseEnded,
}

impl NoteReveal {
    /// Has the next frame that draws Stashes reveal the note for `cause`,
    /// beside any reveal already due in it, `next`. A press's answer due in
    /// that frame keeps its reveal over the end of its pause, which shows in
    /// fewer places. Neither outlives that frame: Stashes takes it as it
    /// renders ([`reveal_note`]), so a press's answer that frame did not
    /// scroll to, beside the list, never wins over an end of its pause that
    /// comes later.
    fn due_in(next: &std::cell::Cell<Option<Self>>, cause: Self) {
        let cause = match next.take() {
            Some(pressed @ Self::Pressed(_)) if cause == Self::PauseEnded => pressed,
            _ => cause,
        };
        next.set(Some(cause));
    }
}

/// Opens `app`'s Stashes, reads its list, selects its newest stash and reads
/// it, for tests in other modules. The reads run on the browser's own
/// executor, a thread `run_until_parked` cannot wait for, so each is awaited
/// rather than waited for in wall-clock time.
#[cfg(test)]
pub(crate) async fn open_selected_stash(app: &Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
    let browser = app
        .read_with(cx, |app, _| app.recovery.browser.clone())
        .and_then(|browser| browser.upgrade())
        .expect("the Stashes dialog opens");
    let read = browser.update(cx, |this, _| this.list_task.take());
    read.expect("the list is read").await;
    cx.update(|window, cx| browser.update(cx, |this, cx| this.select_stash(0, window, cx)));
    let read = browser.update(cx, |this, _| this.metadata_task.take());
    read.expect("the stash is read").await;
    cx.executor().run_until_parked();
    assert!(
        browser.read_with(cx, |this, _| this.snapshot.is_some()),
        "the stash was inspected"
    );
}

/// The page at `offset` of the saved stashes; or, where `sought` is the stash
/// the inspection shows and that page no longer holds it, the page that does,
/// looked for on the same bounded reader, so a stash a save elsewhere pushed
/// onto the next page is followed there and only one truly gone reads as
/// gone. A stash no page holds has gone, and `offset`'s page answers for it.
/// Answers the offset it read with the page, which is where the list now
/// stands. Reads at most `3 + total / STASH_SEEK_SIZE` lists, each bounded by
/// the core's own read of the stash reflog.
fn stash_page(
    repo: &GitRepository,
    offset: usize,
    sought: Option<&str>,
) -> anyhow::Result<(usize, StashPage)> {
    let page = repo.stash_list(offset, STASH_PAGE_SIZE)?;
    let Some(sought) = sought else {
        return Ok((offset, page));
    };
    if page.entries.iter().any(|stash| stash.oid == sought) {
        return Ok((offset, page));
    }
    let mut from = 0;
    loop {
        let wide = repo.stash_list(from, STASH_SEEK_SIZE)?;
        if let Some(row) = wide.entries.iter().position(|stash| stash.oid == sought) {
            let holding = (from + row) / STASH_PAGE_SIZE * STASH_PAGE_SIZE;
            return Ok((holding, repo.stash_list(holding, STASH_PAGE_SIZE)?));
        }
        match wide.next_offset {
            Some(next) => from = next,
            None => return Ok((offset, page)),
        }
    }
}

/// Why the open Stashes last refused Review restore…, the line its footer
/// note says, for tests in other modules.
#[cfg(test)]
pub(crate) fn stash_restore_line(app: &GitTurtle, cx: &App) -> Option<String> {
    let browser = app.recovery.browser.as_ref()?.upgrade()?;
    browser.read(cx).restore_line.clone()
}

/// The open Stashes and the focus of its saved stashes' list, for tests in
/// other modules.
#[cfg(test)]
pub(crate) fn open_stash_list(
    app: &GitTurtle,
    cx: &App,
) -> Option<(Entity<StashBrowser>, FocusHandle)> {
    let browser = app.recovery.browser.as_ref()?.upgrade()?;
    let list = browser.read(cx).stash_focus.clone();
    Some((browser, list))
}

/// The width walk (`width_walk`) reads Stashes, as the focus sweep
/// (`focus_ring::sweep`) walks a dialog, once its saved stashes have landed.
/// They are read on the browser's own thread, which neither the drain of the
/// app's executor nor `settle` waits for: a walk that began while the read
/// ran laid out no rows, as a slow read did on a hosted macOS runner.
#[cfg(test)]
impl crate::focus_ring::sweep::Opened for Option<Entity<StashBrowser>> {
    fn finish(self, cx: &mut VisualTestContext) {
        if let Some(browser) = self {
            StashBrowser::lands(&browser, cx);
        }
    }
}

impl StashBrowser {
    /// Returns once every read the browser has started, the saved stashes
    /// or a selected stash's inspection, has landed and been drawn. They run
    /// on the browser's own thread, which `run_until_parked` cannot wait for:
    /// once it drains, each has answered, which holds the test clock, and the
    /// replies land as the test parks.
    #[cfg(test)]
    pub(crate) fn lands(browser: &Entity<Self>, cx: &mut VisualTestContext) {
        // The list's read, and the read of the stash it keeps selected
        // (`Self::load_page`), which starts as the list lands.
        for _ in 0..2 {
            browser.read_with(cx, |browser, _| browser.metadata.drain());
            crate::views::tests::draw(cx);
        }
        assert!(
            browser.read_with(cx, |browser, _| browser.reading().is_none()
                && browser.loading.is_none()),
            "the Stashes read lands"
        );
    }

    fn new(
        owner: WeakEntity<GitTurtle>,
        repository: GitRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            owner,
            repository,
            metadata: SerialExecutor::new("stash-metadata"),
            preview_worker: Worker::new(),
            generation: 0,
            preview_generation: 0,
            showing: crate::system_preview::Showing::next(),
            metadata_task: None,
            list_task: None,
            list_generation: 0,
            list_quiet: false,
            read_owed: false,
            spoken_refresh: None,
            preview_task: None,
            page: None,
            offset: 0,
            selected: None,
            snapshot: None,
            area: StashArea::Staged,
            selected_file: None,
            content: None,
            text_mode: 0,
            editors: [None, None, None],
            decoration: None,
            patch_view: None,
            stash_scroll: UniformListScrollHandle::new(),
            detail: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
            file_scroll: UniformListScrollHandle::new(),
            // One Tab stop, with the arrow keys inside: a handle that is not
            // one left the list out of the Tab order, whatever the element
            // asked.
            stash_focus: cx.focus_handle().tab_stop(true),
            dialog_focus: None,
            // The saved files likewise: Tab passed them by.
            file_focus: cx.focus_handle().tab_stop(true),
            refresh_focus: cx.focus_handle(),
            opening: true,
            inspection_focus: cx.focus_handle(),
            loading: None,
            error: None,
            preview_error: None,
            lfs_failure_line: crate::lfs_download::FailureLine::new("stash-lfs-failure", cx),
            restore_index: false,
            restoring: None,
            restore_line: None,
            restore_alert: None,
            read_alert: None,
            pause_ended: None,
            reveal_note: Default::default(),
            note_lines: std::rc::Rc::new(std::cell::Cell::new(2)),
            restore_at: Default::default(),
            messages_laid_out: Default::default(),
        };
        // Released however its dialog closes, Stashes stops preparing a
        // Review restore… nobody is waiting for. A close callback of the
        // dialog's would never run: the alert dialog drops an `on_close`
        // given to it as it builds its surface (as `authentication.rs`
        // notes), so one cancelled nothing even while the browser was kept.
        cx.on_release(|this, cx| this.cancel_restore(cx)).detach();
        this.load_page(0, window, cx);
        this
    }

    fn is_current(&self, cx: &App) -> bool {
        self.owner.upgrade().is_some_and(|owner| {
            let owner = owner.read(cx);
            owner.path.as_deref() == Some(self.repository.path())
                && owner.page == AppPage::Repository
        })
    }

    /// Opens captured bytes for this showing's open controls through the
    /// window's `GitTurtle`. The controls are the detail's own, revealed
    /// with their rings as the keyboard reaches them, so the note that a
    /// pause has ended leaves one that holds focus where it is
    /// ([`reveal_note`]).
    fn opener(&self, cx: &App) -> crate::system_preview::Opener {
        let ring = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).focus_ring.clone())
            .unwrap_or_default();
        crate::system_preview::Opener::new(self.owner.clone(), self.showing)
            .revealed_by(self.detail.clone(), appearance::button_ring_room(cx))
            .ringed_by(ring)
    }

    fn clear_preview(&mut self) {
        crate::model_view::pause(self.content.as_deref());
        self.preview_generation = self.preview_generation.wrapping_add(1);
        self.showing = crate::system_preview::Showing::next();
        self.preview_worker.cancel();
        self.preview_task = None;
        self.content = None;
        self.editors = [None, None, None];
        self.decoration = None;
        self.patch_view = None;
        self.preview_error = None;
    }

    /// CPU bytes the open preview retains: its prepared content, which includes the patch
    /// presentation, with the same reserve for native editors as a repository tab, plus the
    /// patch editor's decorations. This transient inspector has no retained-bytes bound of its
    /// own, and a modal stays out of the repository tabs' bound because tabs cannot switch
    /// while it is open; its preview worker's cache bounds the content separately.
    #[cfg_attr(not(test), expect(dead_code))]
    fn preview_retained_bytes(&self) -> usize {
        self.content
            .as_ref()
            .map_or(0, |content| content.bytes().saturating_mul(4))
            + self
                .decoration
                .as_ref()
                .map_or(0, crate::editor_find::PatchDecorations::retained_bytes)
    }

    fn load_page(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_restore(cx);
        // The selection goes until the list lands, and with it the read of
        // the stash it held.
        self.generation = self.generation.wrapping_add(1);
        self.metadata_task = None;
        self.clear_preview();
        self.release_inspection_focus(window, cx);
        self.snapshot = None;
        self.detail.scroll().set_offset(Point::default());
        // The selected stash, by its stash commit, which the list read
        // again keeps selected where it then stands.
        let kept = self
            .selected
            .and_then(|index| self.page.as_ref()?.entries.get(index))
            .map(|stash| stash.oid.clone());
        self.selected = None;
        self.selected_file = None;
        self.loading = Some(READING_LIST);
        self.error = None;
        self.forget_restore_line(cx);
        self.forget_read_alert(cx);
        self.list_quiet = false;
        self.read_list(offset, kept, window, cx);
    }

    /// Refresh in Stashes, its control and the window's key under the dialog
    /// alike ([`GitTurtle::refresh_open_stashes`]): reads the page shown again by
    /// the list's bounded read and leaves everything else as it is until it
    /// answers, so the selected stash keeps its selection, its inspection
    /// and focus inside that inspection, wherever a stash saved or dropped
    /// elsewhere has renumbered it, and nothing dims for the moment it
    /// takes. The list scrolls only to keep in view a row that was in view
    /// before the read. A stash that has gone is no longer selected
    /// ([`Self::select_none`]). With no inspection drawn, or one that failed,
    /// there is nothing to keep and the list is read as a page turn reads
    /// it. A read already running brings the list, as the control is
    /// disabled for it.
    pub(super) fn refresh_saved_stashes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.metadata_task.is_some() || self.list_task.is_some() {
            return;
        }
        self.spoken_refresh = Some(SpokenRefresh::Pending);
        if self.snapshot.is_none() || self.error.is_some() {
            self.load_page(self.offset, window, cx);
            return;
        }
        self.list_quiet = true;
        self.read_list(self.offset, None, window, cx);
    }

    /// Stashes opened again on this browser as Drop…'s confirmation is
    /// cancelled ([`GitTurtle::reopen_stashes`]): reads the page shown
    /// again, as Refresh does, keeping the selected stash, its inspection
    /// and focus wherever a stash saved or dropped elsewhere meanwhile has
    /// renumbered it. No press asked for it, so it says nothing, unless the
    /// stash the person returned to has gone, dropped from a terminal while
    /// the confirmation was open: that clears the selection, which is said
    /// once, "Saved stash no longer listed", as Refresh says it. Cleared with
    /// nothing said, the person heard only the list's focus. A read still
    /// running is let land first, and this read made after it
    /// ([`Self::read_owed`]).
    fn read_again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.metadata_task.is_some() || self.list_task.is_some() {
            self.read_owed = true;
            return;
        }
        self.read_owed = false;
        self.refresh_saved_stashes(window, cx);
        if self.spoken_refresh.is_some() {
            self.spoken_refresh = Some(SpokenRefresh::Quiet);
        }
    }

    /// A read of the list or of the selected stash has ended: the read
    /// Stashes owes ([`Self::read_owed`]) is made once no other still runs,
    /// after the landing has been shown.
    fn pay_owed_read(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_owed {
            cx.defer_in(window, |this, window, cx| {
                if this.read_owed {
                    this.read_again(window, cx);
                }
            });
        }
    }

    /// Reads the page at `offset` of the saved stashes, replacing any read
    /// of the list still running, and shows it as it lands, keeping
    /// selected the stash chosen meanwhile, else `kept`.
    fn read_list(
        &mut self,
        offset: usize,
        kept: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.list_generation = self.list_generation.wrapping_add(1);
        let generation = self.list_generation;
        let repo = self.repository.clone();
        // A quiet read looks for the inspected stash wherever it stands now:
        // a stash saved elsewhere can push it onto the next page, which is
        // not gone (`stash_page`).
        let sought = self
            .list_quiet
            .then(|| {
                self.snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.stash.oid.clone())
            })
            .flatten();
        let response = self
            .metadata
            .submit_read(move || stash_page(&repo, offset, sought.as_deref()));
        self.list_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Stash listing was interrupted. Refresh to try again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.list_generation != generation {
                    return;
                }
                this.list_task = None;
                this.pay_owed_read(window, cx);
                let quiet = std::mem::replace(&mut this.list_quiet, false);
                let opening = std::mem::take(&mut this.opening);
                this.loading = this.reading();
                if !this.is_current(cx) {
                    this.spoken_refresh = None;
                    this.read_alert = Some(crate::tags::alert_error_line(
                        &mut this.error,
                        "The repository changed. Close this inspector and open Stashes again.",
                        window,
                        cx,
                    ));
                    cx.notify();
                    return;
                }
                let asked = offset;
                match result {
                    // Stashes dropped elsewhere can leave the page read again
                    // past the end of the list, the sought stash gone with
                    // them (`stash_page`): the first page stands in.
                    Ok((offset, page)) if page.entries.is_empty() && offset > 0 => {
                        if quiet {
                            this.spoken_refresh = this.spoken_refresh.map(|_| SpokenRefresh::Gone);
                        }
                        this.load_page(0, window, cx);
                    }
                    Ok((offset, page)) => {
                        // A stash saved since, from a terminal or the
                        // Actions menu, renumbers the list above the one
                        // selected, and a dropped one below it: the stash
                        // stays selected by its commit, never by its
                        // `stash@{n}`, and its saved changes are read
                        // again, focus where it was. Gone, nothing is
                        // selected. A stash chosen while the list was read,
                        // from the rows it still drew, stays selected the
                        // same way: its read is left to land where the
                        // list still names it as it did, and made again
                        // where the list renumbered it, which the core
                        // refuses to read by its old `stash@{n}`.
                        let chosen = this
                            .selected
                            .and_then(|index| this.page.as_ref()?.entries.get(index))
                            .cloned();
                        // A stash chosen from the old rows while this looked
                        // for the inspected one on another page: the page
                        // found is not the one being browsed, so the one
                        // asked for is read again, keeping what was chosen.
                        if offset != asked
                            && chosen.as_ref().is_some_and(|stash| {
                                !page.entries.iter().any(|entry| entry.oid == stash.oid)
                            })
                        {
                            let chosen = chosen.map(|stash| stash.oid);
                            this.read_list(asked, chosen, window, cx);
                            this.loading = this.reading();
                            cx.notify();
                            return;
                        }
                        let index = chosen
                            .as_ref()
                            .map(|stash| &stash.oid)
                            .or(kept.as_ref())
                            .and_then(|oid| {
                                page.entries.iter().position(|stash| stash.oid == *oid)
                            });
                        let unchanged = chosen.as_ref().is_some_and(|stash| {
                            index.is_some_and(|index| page.entries[index] == *stash)
                        });
                        // The Refresh key's read found the stash shown, on
                        // this page or, pushed there by a stash saved
                        // elsewhere, on the next (`stash_page`). What a stash
                        // holds is read by its objects, which never change,
                        // so its inspection, focus inside it and a Review
                        // restore… being prepared stay however the list
                        // renumbered it. It takes the entry the list names it
                        // by now, which Drop… and Review restore… hand the
                        // core, which refuses one from an older list.
                        if quiet
                            && let Some(index) = index
                            && let Some(snapshot) = this.snapshot.as_mut()
                            && snapshot.stash.oid == page.entries[index].oid
                        {
                            Arc::make_mut(snapshot).stash = page.entries[index].clone();
                            let in_view =
                                this.selected.is_some_and(|row| this.stash_row_in_view(row));
                            this.selected = Some(index);
                            this.page = Some(page);
                            this.offset = offset;
                            // The row keeps in view as it moves, where it was
                            // there; a person who scrolled away from it is
                            // not pulled back.
                            if in_view {
                                this.stash_scroll
                                    .scroll_to_item(index, ScrollStrategy::Nearest);
                            }
                            if this
                                .content
                                .as_deref()
                                .is_some_and(Content::has_changeable_lfs)
                                && let Some(file_index) = this.selected_file
                            {
                                this.select_file(file_index, window, cx);
                            }
                            this.say_refreshed(window, cx);
                            cx.notify();
                            return;
                        }
                        this.page = Some(page);
                        this.offset = offset;
                        match index {
                            Some(index) => {
                                this.stash_scroll
                                    .scroll_to_item(index, ScrollStrategy::Center);
                                if unchanged {
                                    this.selected = Some(index);
                                } else {
                                    this.selected = None;
                                    this.read_stash(index, window, cx);
                                }
                            }
                            None => {
                                if chosen.is_some() {
                                    this.spoken_refresh =
                                        this.spoken_refresh.map(|_| SpokenRefresh::Gone);
                                    this.select_none(window, cx);
                                }
                                this.stash_scroll.scroll_to_item(0, ScrollStrategy::Top);
                            }
                        }
                    }
                    Err(error) => {
                        this.spoken_refresh = None;
                        this.read_alert = Some(crate::tags::alert_error_line(
                            &mut this.error,
                            format!("{error:#}"),
                            window,
                            cx,
                        ));
                    }
                }
                // Once its own read has ended: a page past the end reads the
                // first page before it answers.
                if this.list_task.is_none() {
                    this.say_refreshed(window, cx);
                }
                // The read that opened Stashes found nothing saved: focus
                // goes to Refresh, enabled now, and the empty state's words
                // are read in its name, one screen reader sentence for one
                // opening, with no announcement of its own.
                if opening
                    && this
                        .page
                        .as_ref()
                        .is_some_and(|page| page.entries.is_empty())
                    && this.stash_focus.is_focused(window)
                {
                    window.focus(&this.refresh_focus, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Says what one press of Refresh found, once, when its read has landed:
    /// the saved stashes and the one selected, or that it is no longer
    /// listed. A read of the list no press asked for says nothing.
    fn say_refreshed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(spoken) = self
            .spoken_refresh
            .take()
            .filter(|spoken| *spoken != SpokenRefresh::Quiet)
        else {
            return;
        };
        let Some(page) = self.page.as_ref() else {
            return;
        };
        let text = if spoken == SpokenRefresh::Gone {
            "Saved stash no longer listed".to_owned()
        } else {
            let stashes = spoken_page(self.offset, page.next_offset, page.entries.len());
            match self.selected.and_then(|index| page.entries.get(index)) {
                Some(stash) => format!("{stashes}, {} selected", stash.selector),
                None => stashes,
            }
        };
        crate::copy_feedback::announce(window, cx, text);
    }

    /// Refresh's accessible name. With nothing saved it carries the empty
    /// state's words: focus lands on it as the read that opened Stashes
    /// finishes ([`Self::opening`]), so that landing reads them.
    fn refresh_name(&self) -> SharedString {
        if self
            .page
            .as_ref()
            .is_some_and(|page| page.entries.is_empty())
        {
            format!("Refresh saved stashes. {NO_SAVED_STASHES}").into()
        } else {
            "Refresh saved stashes".into()
        }
    }

    /// Whether row `row` of the saved stashes lay at all in the list's
    /// viewport as it was last drawn.
    fn stash_row_in_view(&self, row: usize) -> bool {
        let rows = self.page.as_ref().map_or(0, |page| page.entries.len());
        let state = self.stash_scroll.0.borrow();
        let Some(size) = state.last_item_size.filter(|_| rows > 0) else {
            return false;
        };
        let height = size.contents.height / rows as f32;
        let top = height * row as f32 + state.base_handle.offset().y;
        top < size.item.height && top + height > px(0.)
    }

    /// What the detail says is being read: the chosen stash's saved changes,
    /// else the list, whichever of the two reads still runs.
    fn reading(&self) -> Option<&'static str> {
        if self.metadata_task.is_some() {
            Some(READING_STASH)
        } else if self.list_task.is_some() && !self.list_quiet {
            Some(READING_LIST)
        } else {
            None
        }
    }

    /// Selects nothing, dropping the chosen stash's read and what it showed,
    /// as the list read again without the stash chosen meanwhile does.
    fn select_none(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_restore(cx);
        self.generation = self.generation.wrapping_add(1);
        self.metadata_task = None;
        self.clear_preview();
        // As in `read_stash`, a guard: the list read again that calls this
        // cleared the inspection as it started, and a stash chosen during
        // that read is inspected only after the list lands, its read behind
        // the list's on the one serial reader, so nothing is drawn here
        // today. Another reader or order would leave focus on the
        // inspection this clears.
        self.release_inspection_focus(window, cx);
        self.snapshot = None;
        self.selected = None;
        self.selected_file = None;
        self.error = None;
        self.loading = self.reading();
        self.forget_restore_line(cx);
        self.forget_read_alert(cx);
    }

    /// Before the chosen stash's inspection stops being drawn, moves focus
    /// held anywhere inside it ([`Self::inspection_focus`]: the saved files,
    /// a group, a text mode, the comparison and whatever it draws, such as
    /// a model's canvas, or the footer's actions) to the stash list, the
    /// nearest control of the same list that stays drawn: on the stash's
    /// row where the list read again keeps it selected, which the list
    /// scrolls into view as it lands ([`Self::load_page`]), else on the
    /// list alone. Focus left there stayed on a control no frame drew while
    /// the list was read, and once the stash had gone, on one no frame
    /// would draw again, where no key acted. Focus elsewhere, on the list,
    /// Refresh or the pager, stays where it is. Asked of the frame that
    /// last drew the inspection, before the next one leaves it out.
    fn release_inspection_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.snapshot.is_some() && self.inspection_focus.contains_focused(window, cx) {
            window.focus(&self.stash_focus, cx);
        }
    }

    fn select_stash(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .page
            .as_ref()
            .is_some_and(|page| index < page.entries.len())
        {
            window.focus(&self.stash_focus, cx);
            self.read_stash(index, window, cx);
        }
    }

    /// Selects stash `index` and reads its saved changes, as the list read
    /// again keeps its stash selected ([`Self::load_page`]), leaving focus
    /// where it is unless it lay on the inspection this clears, which hands
    /// it to the stash list ([`Self::release_inspection_focus`]).
    fn read_stash(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stash) = self
            .page
            .as_ref()
            .and_then(|page| page.entries.get(index))
            .cloned()
        else {
            return;
        };
        if self.selected == Some(index) && self.error.is_none() {
            // Chosen again, as a click on its row, Up on the first or Home
            // does: the stash shown, or being read, stays, and so does
            // Review restore…'s preparation, which the read again cancelled
            // with nothing said. Reading it again would check nothing that
            // acting on it does not: what it holds was read by its objects,
            // which never change, and whether it is still saved where the
            // list put it is checked again in core by Review restore…'s
            // plan (`stash_apply_plan`) and Drop…'s write, each refusing in
            // its own place once a terminal has changed the list.
            return;
        }
        self.cancel_restore(cx);
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.clear_preview();
        // Every way that clears the inspection releases its focus first.
        // Here it guards an order the reads keep today: a click or key
        // focuses the list before choosing (`select_stash`), and the list
        // read again, which calls this for a renumbered stash, cleared the
        // inspection as it started, while a stash chosen during that read
        // lands after it on the one serial reader.
        self.release_inspection_focus(window, cx);
        self.snapshot = None;
        self.detail.scroll().set_offset(Point::default());
        self.selected = Some(index);
        self.selected_file = None;
        self.loading = Some(READING_STASH);
        self.error = None;
        self.forget_restore_line(cx);
        self.forget_read_alert(cx);
        let repo = self.repository.clone();
        let response = self
            .metadata
            .submit_read(move || repo.stash_snapshot(&stash));
        self.metadata_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Stash inspection was interrupted. Select the stash again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                this.metadata_task = None;
                this.pay_owed_read(window, cx);
                this.loading = this.reading();
                if !this.is_current(cx) {
                    this.read_alert = Some(crate::tags::alert_error_line(
                        &mut this.error,
                        "The repository changed. Open Stashes again.",
                        window,
                        cx,
                    ));
                    cx.notify();
                    return;
                }
                match result {
                    Ok(snapshot) => {
                        this.area = [StashArea::Staged, StashArea::Unstaged, StashArea::Untracked]
                            .into_iter()
                            .find(|area| !area.files(&snapshot).is_empty())
                            .unwrap_or(StashArea::Staged);
                        this.snapshot = Some(Arc::new(snapshot));
                        this.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
                    }
                    Err(error) => {
                        this.read_alert = Some(crate::tags::alert_error_line(
                            &mut this.error,
                            format!("{error:#}"),
                            window,
                            cx,
                        ));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn choose_area(&mut self, area: StashArea, cx: &mut Context<Self>) {
        if self.area == area {
            return;
        }
        self.area = area;
        self.selected_file = None;
        self.clear_preview();
        self.file_scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn files(&self) -> &[FileChange] {
        self.snapshot
            .as_ref()
            .map_or(&[], |snapshot| self.area.files(snapshot))
    }

    fn select_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.files().get(index).cloned() else {
            return;
        };
        window.focus(&self.file_focus, cx);
        self.clear_preview();
        self.selected_file = Some(index);
        let generation = self.preview_generation;
        let response = self.preview_worker.submit(Job::Preview {
            origins: Default::default(),
            repo: self.repository.clone(),
            file,
            prepare_split: false,
        });
        self.preview_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.preview_generation != generation {
                    return;
                }
                this.preview_task = None;
                if !this.is_current(cx) {
                    this.preview_error = Some("The repository changed. Open Stashes again.".into());
                    cx.notify();
                    return;
                }
                match result {
                    Ok(Ok(Output::Preview(content, _))) => {
                        this.content = Some(content);
                        this.ensure_editor(window, cx);
                    }
                    Ok(Err(error)) => this.preview_error = Some(format!("{error:#}")),
                    _ => {
                        this.preview_error =
                            Some("Preview was interrupted. Select the file again.".into())
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn ensure_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.text_mode == 3 {
            return;
        }
        if self.editors[self.text_mode].is_some() {
            return;
        }
        let Some(Content::Text {
            patch,
            old,
            new,
            presentation,
            ..
        }) = self.content.as_deref()
        else {
            return;
        };
        let value = [patch, old, new][self.text_mode];
        let (editor, decoration) = text::editor_with_decorations(
            value,
            if self.text_mode == 0 { "diff" } else { "text" },
            (self.text_mode == 0).then_some(presentation.as_ref()),
            window,
            cx,
        );
        if self.text_mode == 0 {
            self.patch_view = Some(diff_view::new(editor.clone(), presentation, window, cx));
            self.decoration = decoration;
        }
        self.editors[self.text_mode] = Some(editor);
    }

    /// Review restore…: Stashes stays open, with its selection, while the
    /// core prepares the restoration and reads the pause itself, never the
    /// one the window read last, which a merge finished in a terminal can
    /// have ended unseen. Its review opens in place of Stashes; a refusal
    /// stays in Stashes, in the footer note beside Review restore…. Either
    /// answers only what was asked while Stashes still shows it: choosing
    /// another stash or page, the other Restore staged state, or closing
    /// Stashes cancels the preparation, and Review restore… can be pressed
    /// again at once.
    fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let stash = snapshot.stash.clone();
        let restore_index = self.restore_index;
        let repository = self.repository.path().to_owned();
        let request = RestoreRequest {
            stash: stash.oid.clone(),
            restore_index,
            generation: self.generation,
        };
        let browser = cx.entity().downgrade();
        let shown = browser.clone();
        let refused = request.clone();
        // Where the press left focus, which its answer is shown beside
        // unless the keyboard has moved on meanwhile.
        let held = window.focused(cx).map(|focus| focus.downgrade());
        let failure: Failure = Box::new(move |message, window, cx| {
            if let Some(browser) = browser.upgrade().filter(|_| window.has_active_dialog(cx)) {
                browser.update(cx, |browser, cx| {
                    if browser.answered(&refused) {
                        browser.refuse(message, held, window, cx);
                    }
                });
            }
        });
        let asked = request.clone();
        let reviewing: Reviewing = Box::new(move |window, cx| {
            let answers = shown
                .upgrade()
                .filter(|_| window.has_active_dialog(cx))
                .is_some_and(|browser| browser.update(cx, |browser, _| browser.answered(&asked)));
            if answers {
                window.close_dialog(cx);
            }
            answers
        });
        let started = self.owner.update(cx, |this, cx| {
            if this.path.as_ref() != Some(&repository) { return None; }
            this.prepare_recovery(move |repo| {
                let plan = repo.stash_apply_plan(&stash, restore_index)?;
                let mut explanation = format!("Restore “{}” ({}) onto {} at {}.\n\n{}\n\nThe stash remains saved after restoration. Conflicts leave the applied work in place for review in Changes.", stash.name, short_oid(&stash.oid), plan.current.branch, short_oid(&plan.current.head),
                    if restore_index { "Restore the original staged state as well as working files. Git may refuse if that staged state cannot be recreated safely." } else { "Restore tracked changes into the working files without recreating the stash's staged state. Saved untracked files are restored when present." });
                append_paths(&mut explanation, &plan.affected_paths);
                Ok(Prepared::Confirmation { title: "Restore stash".into(), explanation, action: "Restore stash", command: RecoveryCommand::ApplyStash { plan }.into() })
            }, Some(failure), Some(reviewing), window, cx).then_some(this.recovery.generation)
        });
        if let Ok(Some(read)) = started {
            self.restoring = Some((request, read));
            self.forget_restore_line(cx);
            cx.notify();
        }
    }

    /// Whether an answer to `request` still answers what Stashes shows: it
    /// is what Review restore… is preparing, for the stash, Restore staged
    /// state and selection shown. If so, Review restore… has its answer and
    /// prepares nothing more; if not, the answer is dropped.
    fn answered(&mut self, request: &RestoreRequest) -> bool {
        let answers = self
            .restoring
            .as_ref()
            .is_some_and(|(asked, _)| asked == request)
            && self.generation == request.generation
            && self.restore_index == request.restore_index
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.stash.oid == request.stash);
        if answers {
            self.restoring = None;
        }
        answers
    }

    /// Whether Review restore…'s own preparation still runs: the window's
    /// recovery read it started, which ends as it answers or is cancelled,
    /// from Stashes or from the window. A flag only its answer cleared would
    /// stay set where the window dropped that answer.
    fn preparing(&self, cx: &App) -> bool {
        self.restoring.as_ref().is_some_and(|(_, read)| {
            self.owner.upgrade().is_some_and(|owner| {
                let owner = owner.read(cx);
                owner.recovery.generation == *read && owner.recovery.task.is_some()
            })
        })
    }

    /// Whether a press of Drop… or Review restore… is refused, and whether
    /// they look it: the window's busy spell, its own Review restore…'s
    /// preparation included, or the browser's own read.
    fn refusal(&self, cx: &App) -> (bool, bool) {
        let busy = self.reading().is_some();
        let (refused, dimmed) =
            crate::workspace::dialog_refusal(&self.owner, Some(self.repository.path()), cx);
        (busy || refused, busy || dimmed)
    }

    /// The stacked detail's floor: Review restore…'s compact frame whole with
    /// its ring's `room` above it, the note's `gap_2` and one `line` of the
    /// note beneath it, so the detail's view, `room` taller than the column
    /// it fills, holds both with the ring's room at either edge
    /// ([`reveal_note`]).
    fn stacked_floor(room: Pixels, line: Pixels, window: &Window) -> Pixels {
        room + appearance::ui_size(28.) + window.rem_size() * 0.5 + line
    }

    /// The least the stacked list panel gives up its spare height to: its
    /// 38 px header, one 58 px row and its 36 px pager at the interface
    /// size, with the header's and pager's rules and the panel's border
    /// (1 px each), so its one row stays whole.
    fn stacked_list_minimum() -> Pixels {
        appearance::ui_size(38. + 58. + 36.) + px(4.)
    }

    /// Review restore…, tracking `focus`. While Git prepares its
    /// restoration it says so in place, where the window's progress label
    /// that says it lies beneath the dialog: its name at once, and its label
    /// reads "Preparing…" once the preparation looks busy, as it dims
    /// ([`crate::workspace::BusyLook`]). Both labels share one cell, so the
    /// control keeps the wider one's width, its own, and Drop… and the
    /// footer beside it never move; focus stays on it.
    fn review_restore(&self, focus: &FocusHandle, cx: &App) -> Button {
        let (refused, dimmed) = self.refusal(cx);
        let preparing = self.preparing(cx);
        let shown = preparing && dimmed;
        // Each label as the kit lays its own out, centred in the cell.
        let cell = |text: &'static str, state: &'static str, visible: bool| {
            div()
                .row_start(1)
                .col_start(1)
                .flex()
                .justify_center()
                .when(!visible, |cell| cell.invisible())
                // Named only while shown, so tests find
                // `restore-saved-stash/rest` or `…/preparing`, never both.
                .when(visible, |cell| {
                    cell.debug_selector(move || format!("restore-saved-stash/{state}"))
                })
                .child(
                    div()
                        .min_w_0()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .line_height(relative(1.))
                        .child(text)
                        // In tests, the size, weight and colour it is drawn in.
                        .children(appearance::text_probe(move || {
                            format!("restore-saved-stash/{state}/text")
                        })),
                )
        };
        // On the compact frame, as Drop… beside it is, with its height,
        // padding, label size and ring: the kit's small Button stood 19.5 px
        // beside Drop…'s 28 at 13 pt, and its label, `text_sm`, drew
        // 11.375 px beside Drop…'s 12 px. The frame draws no content of its
        // own; the cell holds both labels.
        let restore = crate::button_frame("restore-saved-stash", "Review restore…", "", false)
            .primary()
            .child(
                div()
                    .grid()
                    .grid_cols(1)
                    .items_center()
                    .child(cell("Review restore…", "rest", !shown))
                    .child(cell(RESTORE_PREPARING, "preparing", shown)),
            )
            // The content is drawn text, which names nothing.
            .accessibility_label(if preparing {
                RESTORE_PREPARING_NAME
            } else {
                "Review restore…"
            })
            .track_focus(focus)
            .debug_selector(|| "restore-saved-stash".into());
        crate::workspace::refusing(restore, || "restore-saved-stash".into(), refused, dimmed)
    }

    /// Stops preparing Review restore…'s restoration, which no longer
    /// answers what Stashes shows, or Stashes has closed: the window's read
    /// is cancelled ([`GitTurtle::cancel_recovery_preparation`]), so its
    /// answer never lands and Review restore… can be pressed again at once.
    fn cancel_restore(&mut self, cx: &mut App) {
        if let Some((_, read)) = self.restoring.take() {
            let _ = self
                .owner
                .update(cx, |owner, cx| owner.cancel_recovery_preparation(read, cx));
        }
    }

    /// Says why Review restore… was refused in the footer note beside it,
    /// for the press that found focus on `held` ([`NoteReveal::Pressed`]),
    /// and has the window's alert say the note's line at once, a fresh node
    /// for each answer, so one in the words of the one before is heard
    /// again (`copy_feedback::alert`). The note is no live region of its
    /// own: AccessKit's filter leaves out a node wholly outside the detail
    /// that clips it with nothing beside it in view, as the note is once
    /// focus has moved onto the saved files while Git prepared, at 18 pt in
    /// a 461 × 490 window, so its alert was never heard there; and the
    /// vendored AT-SPI adapter reports a line's `showing` change as the
    /// detail scrolls it in and out, which Orca presents for an alert, so an
    /// alert line could be said again. Another stash chosen before a frame
    /// draws the note takes the alert back unheard
    /// ([`Self::forget_restore_line`]).
    fn refuse(
        &mut self,
        message: String,
        held: Option<WeakFocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let said = restore_said(&message);
        self.restore_alert = Some(crate::copy_feedback::alert(window, cx, said));
        self.restore_line = Some(message);
        self.pause_ended = None;
        self.reveal_note_for(NoteReveal::Pressed(held));
        cx.notify();
    }

    /// Has the next frame that draws Stashes reveal the footer note for
    /// `cause` ([`NoteReveal::due_in`]).
    fn reveal_note_for(&self, cause: NoteReveal) {
        NoteReveal::due_in(&self.reveal_note, cause);
    }

    /// Takes back the alert of a failed read no frame has drawn yet
    /// ([`Self::read_alert`]), as another stash or page is chosen.
    fn forget_read_alert(&mut self, cx: &mut App) {
        if let Some(alerted) = self.read_alert.take() {
            crate::copy_feedback::withdraw(alerted, cx);
        }
    }

    /// The footer note says again what restoring does, and no longer
    /// scrolls the detail to a refusal that has gone: a reveal still due
    /// scrolled the next stash's detail to its foot. A refusal no frame has
    /// drawn yet goes unheard too: the window's alert that says it is taken
    /// back (`copy_feedback::withdraw`), which went on for 1.5 s, the note
    /// never drawn, and could cut into what the next stash says.
    fn forget_restore_line(&mut self, cx: &mut App) {
        if let Some(alerted) = self.restore_alert.take() {
            crate::copy_feedback::withdraw(alerted, cx);
        }
        self.restore_line = None;
        self.pause_ended = None;
        self.reveal_note.set(None);
    }

    /// The footer note beside Review restore…: why it was refused, which the
    /// window's alert says at once as it lands ([`Self::refuse`]); or, once the
    /// integration it was refused for has ended, that it has, which the
    /// window's announcement says once speech pauses
    /// ([`GitTurtle::integration_ended`]); or else what restoring does. Either
    /// line is a status, no live region of its own, read where it lies by
    /// whoever moves there, as the GitHub panel's lines are. The reason keeps
    /// `lines`, two beside the list and two or one stacked beneath it, where
    /// the detail can be about 60 px tall (18 pt in a 461 × 490 window)
    /// ([`reveal_note`]), the last ending in an ellipsis where it needs more,
    /// with its whole text in its tooltip as in its name: unclamped it took
    /// three lines there, more than the detail shows, and at two it left Review
    /// restore… cut. Any answer but the core's refusal for a paused
    /// integration, such as Git itself failing, gives the first line of its
    /// report that says anything, or [`RESTORE_UNEXPLAINED`] alone where none
    /// does, and Details… beside it opens the whole report above Stashes, with
    /// Copy details, revealed in the `stacked` detail with its ring, `room`, as
    /// the keyboard reaches it: the whole report, its stderr line after line,
    /// filled the footer unclamped, and a report of one line, as Git's stderr
    /// often is, had no Details…, its rest in a tooltip the keyboard never
    /// opens. The paused refusal's words are the core's, whole in its tooltip
    /// and name, and its end replaces it, so a Details… beside it would go from
    /// under the keyboard.
    fn restore_note(
        &self,
        lines: usize,
        stacked: bool,
        room: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        if let Some(reason) = &self.restore_line {
            // The first line that says anything: a report that opened with an
            // empty line, or said nothing at all, showed an empty note. With
            // no such line the note says that Git could not prepare the
            // restoration, and no Details… opens an empty report.
            let report =
                !gitturtle_core::is_paused_recovery_refusal(reason) && first_line(reason).is_some();
            let shown = restore_said(reason);
            let tooltip = shown.clone();
            // Static text, whose node reports the part of it the stacked
            // detail draws, as the note that its pause has ended does.
            let note = native_accessibility::static_text(
                div()
                    .id("stash-restore-refusal")
                    .debug_selector(|| "stash-restore-refusal".into())
                    .role(Role::Status)
                    .aria_label(shown.clone())
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.warning))
                    .line_clamp(lines)
                    .text_ellipsis()
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tooltip.clone())
                            .build(window, cx)
                    })
                    .child(shown.clone()),
                shown,
            );
            if !report {
                return note.into_any_element();
            }
            let details = self.detail.focus("stash-restore-details", cx);
            let whole = reason.clone();
            return div()
                .flex()
                .items_start()
                .gap_2()
                .child(note.flex_1().min_w_0())
                .child(
                    div()
                        .flex_shrink_0()
                        .when(stacked, |control| {
                            control
                                .on_children_prepainted(self.detail.control(details.clone(), room))
                        })
                        .child(
                            button("stash-restore-details", "Details…", "", false)
                                .track_focus(&details)
                                .debug_selector(|| "stash-restore-details".into())
                                .on_click(move |_, window, cx| {
                                    crate::integration::show_error_details(&whole, window, cx)
                                }),
                        ),
                )
                .into_any_element();
        }
        if let Some(operation) = self.pause_ended {
            let line = pause_ended_line(operation, REVIEW_RESTORE);
            let tooltip = line.clone();
            return native_accessibility::static_text(
                div()
                    .id("stash-restore-ended")
                    .debug_selector(|| "stash-restore-ended".into())
                    .role(Role::Status)
                    .aria_label(line.clone())
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    // As the refusal it replaces keeps its lines.
                    .line_clamp(lines)
                    .text_ellipsis()
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tooltip.clone())
                            .build(window, cx)
                    })
                    .child(line.clone()),
                line,
            )
            .into_any_element();
        }
        native_accessibility::text(
            "stash-restore-note",
            "Restoring keeps this stash saved. Drop it separately after checking the restored work.",
        )
        .text_size(crate::appearance::ui_text(10.))
        .text_color(rgb(p.muted))
        .into_any_element()
    }

    /// Drop…: Stashes closes as its confirmation opens, so no list
    /// renumbers behind an alert holding a `stash@{n}` and the window's
    /// Refresh key finds no Stashes under it
    /// ([`GitTurtle::refresh_open_stashes`]). The confirmation keeps this
    /// browser while it is open: cancelled, Stashes opens again on it
    /// ([`GitTurtle::reopen_stashes`]); confirmed, the write goes as before
    /// and the browser with the confirmation.
    fn drop_stash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let stash = snapshot.stash.clone();
        let repository = self.repository.path().to_owned();
        let accepted = self.owner.upgrade().is_some_and(|owner| {
            let owner = owner.read(cx);
            owner.path.as_ref() == Some(&repository) && owner.operation_busy.is_none()
        });
        if !accepted {
            return;
        }
        // Where the press found focus, among the browser's own controls,
        // which it keeps while its confirmation is open: Drop… for a key, or
        // the saved files or stashes a pointer's press, which focuses no
        // Button, left it on. A control of the dialog's own, as its Close,
        // goes with the dialog: Stashes then opens again on its stashes.
        let acted = window.focused(cx).filter(|_| {
            self.inspection_focus.contains_focused(window, cx)
                || self.stash_focus.is_focused(window)
                || self.refresh_focus.is_focused(window)
        });
        // Nothing it prepares, says or owes outlives the close, as its
        // release ensured when the dialog held its only handle: a read a
        // cancel owed ([`Self::read_owed`]) ran under this alert.
        self.cancel_restore(cx);
        self.spoken_refresh = None;
        self.read_owed = false;
        let browser = cx.entity();
        let owner = self.owner.clone();
        let reopen: crate::integration::Cancelled = std::rc::Rc::new(move |window, cx| {
            let (browser, acted) = (browser.clone(), acted.clone());
            let _ = owner.update(cx, |this, cx| {
                this.reopen_stashes(browser, acted, window, cx)
            });
        });
        let _ = self.owner.update(cx, |this, cx| {
            window.close_dialog(cx);
            this.recovery.browser = None;
            this.confirm_git_write_or_return("Drop saved stash".into(), format!("Remove “{}” ({}) from the saved stash list. Its saved changes will no longer be available here. Current index and working files stay as they are.\n\nInspect or restore any work you need before dropping this stash.", stash.name, short_oid(&stash.oid)), "Drop stash", WriteCommand::Recovery(RecoveryCommand::DropStash { stash }.into()), Some(reopen), window, cx);
        });
    }

    fn render_stash_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(stash) = self.page.as_ref().and_then(|page| page.entries.get(index)) else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let selected = self.selected == Some(index);
        // Its node, which keeps its stash's ID, reports the part of the row
        // the list draws, as History's changed files do. It is named by what
        // never renumbers, its message, date and commit
        // ([`stash_row_name`]).
        native_accessibility::clips_children(
            div()
                .id(stash_row_id(stash))
                .debug_selector(move || format!("saved-stash-row-{index}"))
                .role(Role::ListBoxOption)
                .aria_selected(selected)
                .aria_label(stash_row_name(stash))
                .h(crate::appearance::ui_size(58.))
                .w_full()
                .px_3()
                .py_2()
                .flex()
                .flex_col()
                .gap_1()
                .overflow_hidden()
                .bg(rgb(if selected { p.selected } else { p.panel }))
                .hover(|style| style.bg(rgb(p.row_hover(selected))))
                .cursor_pointer()
                // Where the row's width cuts the message, it is whole in its
                // tooltip; a message drawn whole has none
                // ([`message_tooltip`]).
                .child({
                    use gpui_kit::base::ElementExt as _;
                    let size = crate::appearance::ui_text(12.);
                    let line = MessageLine::Row(index);
                    div()
                        .id(("saved-stash-message", index))
                        .debug_selector(move || format!("saved-stash-message-{index}"))
                        .truncate()
                        .text_size(size)
                        .font_weight(FontWeight::MEDIUM)
                        .on_prepaint(measure_message(
                            &self.messages_laid_out,
                            line,
                            stash.name.clone(),
                            size,
                        ))
                        .tooltip(message_tooltip(&self.messages_laid_out, line))
                        .child(stash.name.clone())
                })
                .child({
                    let line = format!(
                        "{} · {} · {}",
                        stash.selector,
                        short_oid(&stash.oid),
                        short_date(stash.timestamp)
                    );
                    let drawn = line.clone();
                    div()
                        .debug_selector(move || format!("saved-stash-line-{index}: {drawn}"))
                        .truncate()
                        .text_size(crate::appearance::ui_text(10.))
                        .text_color(rgb(p.muted))
                        .child(line)
                })
                .on_click(
                    cx.listener(move |this, _, window, cx| this.select_stash(index, window, cx)),
                ),
        )
        .into_any_element()
    }

    fn render_file_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(file) = self.files().get(index) else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let selected = self.selected_file == Some(index);
        div()
            .id(("stash-file", index))
            .debug_selector(move || format!("stash-file-{index}"))
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .aria_label(format!("{} · {}", self.area.label(), file.path().display()))
            .h(crate::appearance::ui_size(28.))
            .w_full()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .bg(rgb(if selected { p.selected } else { p.canvas }))
            .hover(|style| style.bg(rgb(p.row_hover(selected))))
            .cursor_pointer()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(crate::appearance::ui_text(11.))
                    .child(file.path().display().to_string()),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child(format!("{:?}", file.status)),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select_file(index, window, cx)))
            .into_any_element()
    }

    /// A stashed image's two sides, `width` wide together. Each side's
    /// caption offers the open control Compare's image captions do, by the
    /// same rule: only where the side's captured bytes establish what it is
    /// (`ImageSide::system_preview`, decided on the worker with them, a
    /// raster image on Linux), so a side Compare offers none shows none.
    /// The control is one of the detail's (`system_preview::Opener::
    /// revealed_by`), so the keyboard reveals it there with its ring, and
    /// beneath its caption the short line says why its last click opened
    /// nothing, its whole answer in its tooltip and name. Both controls take
    /// one form and one place by Compare's rule
    /// ([`crate::image_compare::open_place`]), from the narrower room either
    /// side's name leaves, whether or not that side offers one, so an added
    /// or deleted image's one control stands where a changed image's two
    /// do: labelled where the label fits beside the name, otherwise the
    /// icon, on the name's line where the icon fits there, otherwise both on
    /// a line beneath the names; a side without one keeps that line, and
    /// each side's line saying why takes the taller answer's lines, at most
    /// two, so the sides' names and images stay level.
    fn render_images(
        &self,
        sides: [&crate::worker::ImageSide; 2],
        width: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette(cx);
        let opener = self.opener(cx);
        let file = self.selected_file.and_then(|index| self.files().get(index));
        let names = ["Before", "After"];
        let captions = [0, 1].map(|index| {
            if sides[index].animation.is_some() {
                format!("{} · first frame", names[index])
            } else {
                names[index].to_owned()
            }
        });
        let targets = [0, 1].map(|index| {
            let side = sides[index];
            let (Some(bytes), Some(offer)) = (&side.captured, side.system_preview) else {
                return None;
            };
            let path = file
                .and_then(|file| {
                    if index == 0 {
                        file.old_path.as_deref()
                    } else {
                        file.new_path.as_deref()
                    }
                })
                .unwrap_or(std::path::Path::new("image.bin"));
            let target = opener.target(
                ("image-system-preview", index),
                names[index],
                bytes,
                path,
                "image",
                offer,
            );
            Some((target, offer))
        });
        let rem = window.rem_size();
        let text = appearance::ui_text(11.);
        // A side's half, less its padding either side.
        let inner = ((width - rem * 0.5) / 2. - rem).max(Pixels::ZERO);
        // Every caption's room counts, as in Compare: chosen from the
        // offering sides' alone, an added image's After control could stay
        // beside its shorter name where a changed image's two went beneath
        // the names.
        let open = crate::image_compare::open_place(
            targets.iter().any(Option::is_some),
            (0..2).map(|index| {
                inner
                    - rem * 0.5
                    - crate::views::line_widths(&[(captions[index].as_str(), text)], window)
            }),
            window,
        );
        let square = appearance::ui_size(28.);
        let inline = open.is_none_or(|open| open.inline);
        let offered = open.is_some();
        let answers = targets
            .each_ref()
            .map(|target| target.as_ref().and_then(|(target, _)| target.answered(cx)));
        let lines = answers
            .iter()
            .flatten()
            .map(|answer| {
                crate::image_compare::wrapped_lines(answer.brief(), text, inner, window).clamp(1, 2)
            })
            .max();
        let line = crate::image_compare::text_line(text, window);
        let mut controls = targets.map(|target| {
            target.map(|(target, offer)| {
                target.control(
                    offer.form(open.map_or(Pixels::ZERO, |open| open.room), window),
                    cx,
                )
            })
        });
        let mut answers = answers;
        div()
            .size_full()
            .flex()
            .gap_2()
            .children((0..2).map(|index| {
                let side = sides[index];
                let control = controls[index].take();
                let answer = answers[index].take();
                let (inline_control, beneath) = if inline {
                    (control, None)
                } else {
                    (None, Some(control))
                };
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .when(offered && inline, |caption| caption.min_h(square))
                            .child(
                                div()
                                    .debug_selector(move || format!("stash-image-name-{index}"))
                                    .min_w_0()
                                    .truncate()
                                    .text_size(text)
                                    .text_color(rgb(p.muted))
                                    .child(captions[index].clone()),
                            )
                            .children(inline_control),
                    )
                    .children(beneath.map(|control| {
                        div()
                            .flex()
                            .items_center()
                            .h(square)
                            .flex_shrink_0()
                            .children(control)
                    }))
                    .children(lines.map(|lines| {
                        div()
                            .h(line * lines as f32)
                            .flex_shrink_0()
                            .min_w_0()
                            .children(answer.map(|answer| answer.short_line(Some(lines), cx)))
                    }))
                    .child(
                        div()
                            .debug_selector(move || format!("stash-image-{index}"))
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(side.render.clone(), |element, image| {
                                element.child(crate::gif_playback::static_image(image))
                            })
                            .when(side.render.is_none(), |element| {
                                element.child(
                                    div()
                                        .text_size(text)
                                        .text_color(rgb(p.muted))
                                        .child(side.message.clone().unwrap_or("Absent".into())),
                                )
                            }),
                    )
            }))
            .into_any_element()
    }

    /// The selected file, `width` wide as its box is in the frame it is
    /// drawn.
    fn render_content(&self, width: Pixels, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        if let Some(error) = &self.preview_error {
            return div()
                .p_3()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(p.warning))
                .child(error.clone())
                .into_any_element();
        }
        if self.preview_task.is_some() {
            return native_accessibility::text("stash-file-reading", "Reading saved file…")
                .p_4()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(p.muted))
                .into_any_element();
        }
        // Diagrams says it in the failing side's own place.
        let selected_file = self.selected_file.and_then(|index| self.files().get(index));
        let missing_lfs =
            self.content
                .as_deref()
                .zip(selected_file)
                .is_some_and(|(content, file)| {
                    !crate::lfs_download::preview_targets(content, file).is_empty()
                });
        let failure = match self.content.as_deref() {
            Some(Content::Text { lfs_failed, .. }) if self.text_mode != 3 => {
                crate::lfs_download::side_failures_line(*lfs_failed, ["Before", "After"]).or_else(
                    || {
                        missing_lfs.then(|| {
                            "Git LFS object is unavailable in the local store; its pointer is shown."
                                .to_owned()
                        })
                    },
                )
            }
            Some(Content::Text {
                diagrams,
                lfs_failed,
                ..
            }) if diagrams.is_none() => {
                crate::lfs_download::side_failures_line(*lfs_failed, ["Before", "After"])
            }
            _ => None,
        };
        let body = match self.content.as_deref() {
            Some(Content::Text {
                diagrams,
                old,
                new,
                lfs_missing,
                ..
            }) if self.text_mode == 3 => {
                let empty_diagrams = || {
                    let message = selected_file
                        .and_then(|file| {
                            let (path, mode, text, missing) = if file.new_path.is_some() {
                                (file.new_path.as_deref(), file.new_mode.as_str(), new.as_str(), lfs_missing[1])
                            } else {
                                (file.old_path.as_deref(), file.old_mode.as_str(), old.as_str(), lfs_missing[0])
                            };
                            let mermaid = path
                                .and_then(|p| p.extension())
                                .and_then(|ext| ext.to_str())
                                .is_some_and(|ext| {
                                    ext.eq_ignore_ascii_case("mmd")
                                        || ext.eq_ignore_ascii_case("mermaid")
                                });
                            if !mermaid {
                                return None;
                            }
                            match crate::rich_preview::Literal::of(mode, text) {
                                Some(crate::rich_preview::Literal::Link) => {
                                    Some("Stored as a symbolic link; inspect it in Source.")
                                }
                                Some(crate::rich_preview::Literal::LfsPointer) if missing => Some(
                                    "Git LFS object is unavailable locally; fetch it or inspect the pointer in Source.",
                                ),
                                Some(crate::rich_preview::Literal::LfsPointer) => {
                                    Some("Git LFS object is not readable as diagram text; use Source.")
                                }
                                None => None,
                            }
                        })
                        .unwrap_or("No Mermaid diagrams on this file; use Source.");
                    div().child(message).into_any_element()
                };
                diagrams.as_ref().map_or_else(empty_diagrams, |preview| {
                    crate::rich_preview::render_comparison(
                        preview,
                        false,
                        self.opener(cx),
                        width,
                        window,
                        cx,
                    )
                })
            }
            Some(Content::Rich(preview)) => crate::rich_preview::render_comparison(
                preview,
                false,
                self.opener(cx),
                width,
                window,
                cx,
            ),
            Some(Content::Text { .. }) if self.text_mode == 0 => {
                self.patch_view.as_ref().map_or_else(
                    || div().into_any_element(),
                    |view| view.clone().into_any_element(),
                )
            }
            Some(Content::Text { .. }) => self.editors[self.text_mode].as_ref().map_or_else(
                || div().into_any_element(),
                |editor| {
                    crate::editor_find::Editor::new(editor)
                        .readonly(true)
                        .size_full()
                        .aria_label("Saved stash file text")
                        .into_any_element()
                },
            ),
            Some(Content::Images { old, new }) => self.render_images([old, new], width, window, cx),
            Some(Content::Notice(message)) => div()
                .p_4()
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(p.muted))
                .child(message.clone())
                .into_any_element(),
            _ => native_accessibility::text(
                "stash-file-prompt",
                "Select a saved file to inspect its exact comparison.",
            )
            .p_4()
            .text_size(crate::appearance::ui_text(12.))
            .text_color(rgb(p.muted))
            .into_any_element(),
        };
        let ring = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).focus_ring.clone());
        crate::lfs_download::above_comparison(
            &self.lfs_failure_line,
            crate::lfs_download::comparison_key(
                self.selected_file.and_then(|index| self.files().get(index)),
            ),
            failure,
            body,
            ring.as_ref(),
            window,
            cx,
        )
    }
}

/// For the stacked Stashes detail's actions, the row with Review restore…
/// and the footer note beneath it, as each frame of `browser` prepaints
/// them. It keeps its `note_lines`, the lines a refusal keeps in the note,
/// at two, or one where its `detail` cannot hold two beneath Review
/// restore…, at its `restore_at`, whole with its ring, `margin`, each `line`
/// tall: at 18 pt in a 461 × 490 window the detail is about 60 px tall. A
/// change lays the detail out again. With a reveal `due` in this frame,
/// taken as Stashes rendered it, when a refusal or its end has just replaced
/// the note, it scrolls the detail by the least that shows Review restore…
/// and the note `margin` clear of its edges, or, where both still do not
/// fit, Review restore… whole at the top with what of the note the rest
/// shows: the Review restore… just pressed stays whole. Either note scrolls
/// while Review restore…, `pressed`, holds focus or nothing in the detail
/// does; the note due for a press ([`NoteReveal::Pressed`]) also while focus
/// stays where the press found it: a click focuses no Button, so focus stays
/// where it was, on the saved files or a document's open control, and the
/// answer to the click would otherwise lie below the fold. With focus moved
/// onto another of the detail's controls, such as the saved files, a file's
/// text or a document's open control, whether by the keyboard while Git
/// prepared the press's answer or before the end of its pause
/// ([`NoteReveal::PauseEnded`]), which no press asked for, the detail stays
/// where it is, and the note is heard, not shown: the window's alert or
/// announcement says it, outside the detail, which can have scrolled it out
/// of reach. It scrolls after the frame the note is laid out in, as a focus
/// reveal does for a control it has not drawn ([`crate::focus_reveal`]). A
/// note the detail had scrolled below its fold, beneath the Review restore…
/// just pressed, showed nothing, one revealed whole at two lines cut Review
/// restore…'s ring, a merge finished in a terminal scrolled the focused saved
/// files out of view, to Review restore… and the note that it had ended, a
/// click on Review restore… with focus in the detail showed its answer
/// nowhere, and its answer landing after Tab had reached the saved files
/// scrolled them out of view. The reveal is spent in this frame, or, where
/// the note is laid out again with the lines that fit, carried to the next
/// once, through the browser's `reveal_note`; otherwise it expires with this
/// frame, as beside the list, where the footer is always in view. Only a
/// frame beside the list dropped one before, so a reveal any other frame
/// left unspent would have stayed due, to scroll the stacked detail whenever
/// a later frame drew it, whatever focus had done since, and win over any
/// end of its pause meanwhile.
fn reveal_note(
    browser: &StashBrowser,
    due: Option<NoteReveal>,
    pressed: FocusHandle,
    margin: Pixels,
    line: Pixels,
) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
    let due = std::cell::Cell::new(due);
    let next = browser.reveal_note.clone();
    let lines = browser.note_lines.clone();
    let restore = browser.restore_at.clone();
    let detail = browser.detail.clone();
    move |bounds, window, cx| {
        let [_, note] = bounds[..] else {
            return;
        };
        let Some(control) = restore.get() else {
            return;
        };
        let scroll = detail.scroll();
        let viewport = scroll.bounds();
        let top = control.top() - margin;
        // The note's lines that fit beneath Review restore… and its ring.
        let room = viewport.size.height - (note.top() - top) - margin;
        let fit = (f32::from(room) / f32::from(line.max(px(1.))))
            .floor()
            .clamp(1., 2.) as usize;
        if lines.replace(fit) != fit {
            // Laid out again with the lines that fit; revealed then.
            if let Some(cause) = due.take() {
                NoteReveal::due_in(&next, cause);
            }
            window.defer(cx, |window, _| window.refresh());
            return;
        }
        let Some(cause) = due.take() else {
            return;
        };
        // Focus still where the press left it, for a press's answer.
        let kept = match &cause {
            NoteReveal::Pressed(held) => held
                .as_ref()
                .and_then(WeakFocusHandle::upgrade)
                .is_some_and(|held| held.is_focused(window)),
            NoteReveal::PauseEnded => false,
        };
        if !kept && !pressed.is_focused(window) && detail.holds_focus(window, cx) {
            // The control focused elsewhere in the detail stays in view.
            return;
        }
        let bottom = note.bottom() + margin;
        let delta = if bottom - top > viewport.size.height || top < viewport.top() {
            viewport.top() - top
        } else if bottom > viewport.bottom() {
            viewport.bottom() - bottom
        } else {
            return;
        };
        let offset = scroll.offset();
        let target = point(
            offset.x,
            (offset.y + delta).clamp(-scroll.max_offset().y, px(0.)),
        );
        if target != offset {
            let scroll = scroll.clone();
            window.defer(cx, move |window, _| {
                scroll.set_offset(target);
                window.refresh();
            });
        }
    }
}

impl Render for StashBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let browser = cx.entity().downgrade();
        let narrow = window.viewport_size().width < px(800.);
        let height = (f32::from(window.viewport_size().height) - 230.).clamp(300., 620.);
        let busy = self.reading().is_some();
        let stashes = self.page.as_ref().map_or(0, |page| page.entries.len());
        let list = uniform_list(
            "saved-stashes",
            stashes,
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.render_stash_row(index, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .size_full()
        .track_scroll(&self.stash_scroll);
        let files = uniform_list(
            "stash-files",
            self.files().len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.render_file_row(index, cx))
                    .collect::<Vec<_>>()
            }),
        );
        // While the saved files overflow their box, a scrollbar runs in a
        // gutter at their right, as in the detail around them and the
        // dialogs' lists (`tags::ScrollCue`), where the box is tall enough
        // for a thumb that moves, and a fade at each edge the rows continue
        // past where it is not; rows that fit keep the box's whole width.
        // The list reaches into the gutter the cue keeps beside its rows
        // (`ScrollCue::gutter`, the one `ScrollCue::list` keeps), so its
        // scroll, along whose right edge the track runs, spans it, and keeps
        // it clear for the track. Eleven saved files once had
        // only a row cut at the box's foot to say more lay below.
        let file_cue =
            crate::tags::ScrollCue::new(&self.file_scroll.0.borrow().base_handle, Pixels::ZERO);
        let gutter = crate::tags::ScrollCue::gutter();
        let files = file_cue
            .list(
                div()
                    .on_children_prepainted(file_cue.observe(|_, _, _| {}))
                    .id("stash-file-rows")
                    .debug_selector(|| "stash-file-rows".into())
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        files
                            .flex_1()
                            .min_h_0()
                            .mr(-gutter)
                            .pr(gutter)
                            .track_scroll(&self.file_scroll),
                    ),
                "stash-files-scrollbar",
            )
            .flex_1()
            .min_h_0();
        // The heading counts what the list holds: the page's stashes, or
        // which of them it shows where Previous or More pages through more.
        let heading = match &self.page {
            Some(page) if page.entries.is_empty() => "Saved work".to_owned(),
            Some(page) if self.offset == 0 && page.next_offset.is_none() => {
                format!("Saved work · {stashes}")
            }
            // A page of one reads as that stash, as Refresh says it
            // (`spoken_page`), with its word: a bare "Saved work · 41" reads
            // as a count, as "Saved work · 12" does, and the heading once
            // read "Saved work · 41–41".
            Some(_) if stashes == 1 => format!("Saved work · stash {}", self.offset + 1),
            Some(_) => format!("Saved work · {}–{}", self.offset + 1, self.offset + stashes),
            None => "Saved work".to_owned(),
        };
        // A Tab stop while it holds stashes, or may: GPUI reads the stop from
        // the handle, not the element. With nothing saved it holds nothing
        // to reach, focus having moved on to Refresh (`Self::opening`), and
        // Tab once drew a ring around the empty list. Focus already on it
        // stays and moves on from its place.
        let list_focus = self
            .stash_focus
            .clone()
            .tab_stop(self.page.is_none() || stashes > 0);
        // The list draws the window's keyboard ring on its own edge, between
        // the heading and the pager.
        let list_ring = self.owner.upgrade().and_then(|owner| {
            owner
                .read(cx)
                .focus_ring
                .edge(&self.stash_focus, rgb(p.accent))
        });
        // The saved files draw it as their own 1 px edge, as the rebase
        // sequence does.
        let files_ringed = self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).focus_ring.shows(&self.file_focus));
        // While its rows overflow it, as the stacked list's one row does when
        // two stashes are saved, the list shows its scrollbar, decided for
        // its height in the frame it is drawn (`in_frame`): it once showed
        // one stash with nothing saying that a second lay below it, under
        // the pager.
        let row_height = crate::appearance::ui_size(58.);
        let stash_scroll = self.stash_scroll.clone();
        let rows = crate::in_frame::in_frame(move |bounds, _, _| {
            let cued = row_height * stashes as f32 > bounds.size.height + px(0.5);
            div()
                .size_full()
                .relative()
                .when(cued, |rows| rows.pr(Scrollbar::width()))
                .child(list)
                .when(cued, |rows| {
                    rows.child(
                        div()
                            .debug_selector(|| "saved-stashes-scrollbar".into())
                            .absolute()
                            .inset_0()
                            .child(
                                Scrollbar::vertical(&stash_scroll)
                                    .id("saved-stashes-scrollbar")
                                    .mode(ScrollbarMode::Always),
                            ),
                    )
                })
                .into_any_element()
        })
        .named("saved-stashes");
        let snapshot = self.snapshot.clone();
        // Stacked in a narrow window, the saved-work panel is as tall as its
        // header, one 58 px row and its footer at the default size, scaled
        // with them: unscaled, its header and footer left the list 35.6 px
        // at 18 pt in a 461 × 490 window, short of a row and of the empty
        // line, which ran past the list's visible bottom. That line takes
        // the rows' own inset, where its 1 rem padding started it past
        // their text and outgrew the list. With no stash saved there is
        // nothing to inspect: no detail is drawn and the list takes the
        // browser's room. The browser asks for `height` and gives up what
        // the dialog's height cap takes.
        let detail = self.error.is_some()
            || !self
                .page
                .as_ref()
                .is_some_and(|page| page.entries.is_empty());
        let room = appearance::button_ring_room(cx);
        // A control the keyboard moves focus onto scrolls into the detail's
        // view in this frame with its ring; each reports where it lies.
        self.detail.reveal(window, cx);
        // Where the detail is too short for its scrollbar's thumb to move,
        // as stacked at 18 pt in a 461 × 490 window, it fades the edges its
        // content continues past, and a fade stops short of the ring of the
        // control focus is on (`ScrollCue::revealing`): a control the
        // keyboard reveals lies its ring's room from an edge, inside the
        // fade's height, and the fade once lay over its ring. The saved
        // files' own cue needs no reveal: the box, one Tab stop, draws its
        // ring on its own edge, outside the rows it scrolls.
        let cue = crate::tags::ScrollCue::revealing(&self.detail, room);
        let areas: Vec<_> = (0..3_usize)
            .map(|area| self.detail.focus(("stash-area", area), cx))
            .collect();
        let modes: Vec<_> = ["Diff", "Before", "After", "Diagrams"]
            .into_iter()
            .enumerate()
            .filter(|(mode, _)| {
                *mode != 3
                    || matches!(
                        self.content.as_deref(),
                        Some(Content::Text {
                            diagrams: Some(_),
                            ..
                        })
                    )
            })
            .map(|(mode, label)| {
                (
                    mode,
                    label,
                    self.detail.focus(("stash-text-mode", mode), cx),
                )
            })
            .collect();
        let mode_focus: Vec<_> = modes
            .iter()
            .map(|(_, _, focus)| Some(focus.clone()))
            .collect();
        // The comparison's text, where the keyboard reaches it.
        let text_focus: Vec<_> = self
            .editors
            .iter()
            .flatten()
            .map(|editor| editor.read(cx).focus_handle(cx))
            .chain(
                self.patch_view
                    .as_ref()
                    .map(|view| view.read(cx).focus_handle(cx)),
            )
            .collect();
        let restore_index = self.detail.focus("stash-restore-index", cx);
        let drop = self.detail.focus("drop-saved-stash", cx);
        let restore = self.detail.focus("restore-saved-stash", cx);
        // Beside the list, the restore actions and their note are the
        // detail's footer, whole on selection whatever the comparison takes:
        // at 18 pt in a 1000 × 680 window they lay below the fold of the
        // scrolling detail until the user scrolled. Stacked below the list in
        // a narrow window, where the detail is a few lines tall (about 67 px
        // at 18 pt in 461 × 490), they scroll with the rest, revealed with
        // their rings as the keyboard reaches them.
        let pinned = !narrow;
        // A write refuses Drop… and Review restore… at once, and assistive
        // technology hears them unavailable at once, while they dim only
        // once it looks busy (`workspace::BusyLook`); each refused a press
        // silently, announced and drawn available. The browser's own read
        // leaves them unavailable at once.
        let (refused, dimmed) = self.refusal(cx);
        // One line of the footer note, which a refusal keeps two or one of
        // in the stacked detail.
        let note_line = crate::image_compare::text_line(crate::appearance::ui_text(10.), window);
        // The note's reveal is due in this frame alone: taken now, it is
        // spent by the stacked footer this frame prepaints, or carried once
        // to the next where the note is laid out again, and otherwise it
        // expires, as beside the list, where the footer is always in view
        // ([`reveal_note`]).
        let due = self.reveal_note.take();
        if self.snapshot.is_some() {
            // This frame draws the footer note, as the window draws its
            // alert: a refusal it says has been heard and is no longer taken
            // back as another stash is chosen ([`Self::restore_alert`]).
            self.restore_alert = None;
        }
        if self.error.is_some() {
            // This frame draws a failed read's line: its alert has been
            // heard with it ([`Self::read_alert`]).
            self.read_alert = None;
        }
        let actions = self.snapshot.is_some().then(|| {
            let row = div()
                .debug_selector(|| "stash-actions".into())
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .pt_1()
                .when(!pinned, |row| {
                    let report = self.detail.controls(
                        vec![
                            Some(restore_index.clone()),
                            None,
                            Some(drop.clone()),
                            Some(restore.clone()),
                        ],
                        room,
                    );
                    let restore_at = self.restore_at.clone();
                    row.on_children_prepainted(move |bounds, window, cx| {
                        restore_at.set(bounds.get(3).copied());
                        report(bounds, window, cx);
                    })
                })
                .child(
                    Checkbox::new("stash-restore-index")
                        .track_focus(&restore_index)
                        .label("Restore staged state")
                        .checked(self.restore_index)
                        .on_click(cx.listener(|this, checked, _, cx| {
                            this.restore_index = *checked;
                            // A restoration prepared for the other choice
                            // no longer answers.
                            this.cancel_restore(cx);
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(
                    button("drop-saved-stash", "Drop…", "", false)
                        // In tests, the size, weight and colour its label is
                        // drawn in, which Review restore…'s matches.
                        .children(appearance::text_probe(|| "drop-saved-stash/text".into()))
                        .track_focus(&drop)
                        .debug_selector(|| "drop-saved-stash".into())
                        .map(|drop| {
                            crate::workspace::refusing(
                                drop,
                                || "drop-saved-stash".into(),
                                refused,
                                dimmed,
                            )
                        })
                        .on_click(cx.listener(|this, _, window, cx| this.drop_stash(window, cx))),
                )
                .child(
                    self.review_restore(&restore, cx)
                        .on_click(cx.listener(|this, _, window, cx| this.restore(window, cx))),
                );
            div()
                .flex()
                .flex_col()
                // The note lies `gap_2` beneath the buttons, beside the list
                // and stacked beneath it alike, at every size: the stacked
                // detail's floor holds that gap ([`StashBrowser::stacked_floor`]).
                // Stacked, it once lay `gap_1` beneath them, 3.5 px higher at
                // 13 pt than beside the list and than before the floor; at
                // 18 pt in a 461 × 490 window the list gives the floor's
                // 4.5 px more down to its one whole row.
                .gap_2()
                .flex_shrink_0()
                // Beside the list the footer ends at the browser's foot,
                // where the dialog's content clips: it keeps the room of the
                // ring of Details…, on its last line, inside, which the clip
                // cut at 13 pt in a 1480 × 980 window.
                .when(pinned, |actions| actions.pb(room))
                .when(!pinned, |actions| {
                    // Stacked, the note scrolls with the detail: revealed
                    // while Review restore… or nothing in the detail holds
                    // focus, and for a press's answer while focus stays
                    // where the press found it.
                    actions.on_children_prepainted(reveal_note(
                        self,
                        due,
                        restore.clone(),
                        room,
                        note_line,
                    ))
                })
                .child(row)
                .child(self.restore_note(
                    if pinned { 2 } else { self.note_lines.get() },
                    !pinned,
                    room,
                    cx,
                ))
        });
        let (footer, inline) = if pinned {
            (actions, None)
        } else {
            (None, actions)
        };
        // Until a file is chosen the comparison holds only its prompt and
        // keeps that height, which takes no room from the actions; a chosen
        // file's comparison keeps a floor of its own.
        let file_chosen = self.selected_file.is_some();
        // Stacked beneath the list, the detail keeps a floor that holds
        // Review restore… whole with its ring and a line of its note beneath
        // it, so the note's reveal needs no scroll of anything else. Where the
        // dialog's height cap leaves the detail less, as at 18 pt in a 461 ×
        // 490 window, the room comes from empty space: the list panel's spare
        // height beneath its one row, down to its header, that row and its
        // pager, and the gap between the list and the detail, down to
        // `gap_1`. A taller window or a smaller size leaves both as they
        // were, and a window too short even for that takes the detail below
        // its floor ([`STACKED_SPARE_SHRINK`]). The detail once kept 57.5 px
        // there, too short for the 39 px Review restore… and its 22.5 px line.
        let stacked = narrow && detail;
        let floor = (!pinned && self.snapshot.is_some())
            .then(|| Self::stacked_floor(room, note_line, window));
        div().debug_selector(|| "stash-browser".into()).h(px(height)).min_h_0().flex().when(!stacked, |element| element.gap_3()).when(narrow, |element| element.flex_col())
            .child(div().debug_selector(|| "stash-list-panel".into()).w(px(255.)).min_w_0().flex_shrink_0().h_full().flex().flex_col().border_1().border_color(rgb(p.border)).rounded(px(7.)).overflow_hidden()
                .when(stacked, |element| {
                    element
                        .w_full()
                        .h(crate::appearance::ui_size(140.))
                        .flex_shrink(STACKED_SPARE_SHRINK)
                        .min_h(Self::stacked_list_minimum())
                })
                .when(!detail, |element| element.w_full().flex_1())
                .child(div().h(crate::appearance::ui_size(38.)).flex().items_center().px_2().gap_1().border_b_1().border_color(rgb(p.border))
                    .child(native_accessibility::text("stash-list-heading", heading).flex_1().text_size(crate::appearance::ui_text(11.)).font_weight(FontWeight::SEMIBOLD))
                    .child(button("refresh-stashes", "", "refresh", false).track_focus(&self.refresh_focus).debug_selector(|| "refresh-stashes".into()).accessibility_label(self.refresh_name()).disabled(busy).on_click(cx.listener(|this, _, window, cx| this.refresh_saved_stashes(window, cx)))))
                .child(crate::native_accessibility::clips_children(div().id("stash-list-focus").debug_selector(|| "stash-list".into()).role(Role::ListBox).aria_label("Saved stashes").track_focus(&list_focus).relative().flex_1().min_h_0().flex().flex_col()
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if let Some(index) = list_key_selection(event.keystroke.key.as_str(), this.selected, this.page.as_ref().map_or(0, |page| page.entries.len())) {
                            cx.stop_propagation();
                            this.select_stash(index, window, cx);
                            this.stash_scroll.scroll_to_item(index, ScrollStrategy::Center);
                        }
                    }))
                    .when(stashes > 0, |element| element.child(rows))
                    .when(self.page.as_ref().is_some_and(|page| page.entries.is_empty()), |element| element.child(div().px_3().py_2().child(EmptyState::new("stash-list-empty", NO_SAVED_STASHES).block(cx))))
                    .children(list_ring)))
                .child(div().debug_selector(|| "stash-pager".into()).h(crate::appearance::ui_size(36.)).flex().items_center().justify_between().px_2().border_t_1().border_color(rgb(p.border))
                    .child(button("previous-stashes", "Previous", "", false).disabled(busy || self.offset == 0).on_click(cx.listener(|this, _, window, cx| this.load_page(this.offset.saturating_sub(STASH_PAGE_SIZE), window, cx))))
                    .child(button("next-stashes", "More", "", false).disabled(busy || self.page.as_ref().and_then(|page| page.next_offset).is_none()).on_click(cx.listener(|this, _, window, cx| { if let Some(offset) = this.page.as_ref().and_then(|page| page.next_offset) { this.load_page(offset, window, cx); } })))))
            // The detail scrolls where the window leaves it less than its
            // inspection, as below the stacked list in a short window, and
            // cues the rest with its scrollbar, as the dialogs' lists do. It
            // keeps the room of its controls' rings inside its clip and
            // gives it back, so nothing moves, except below its last line:
            // there the dialog's content clips at the browser's foot, which
            // cut the ring of a control revealed at the detail's bottom. A
            // control the keyboard moves onto scrolls into view with its
            // ring, as the Reflog's do. A chosen file's comparison keeps a
            // floor of its own rather than give its height up to the
            // controls around it, as at 18 pt in a 1000 × 680 window, where
            // it kept 30 px without one. Beside the list the actions are the
            // footer below it (`pinned`).
            // Stacked, the gap between the list and the detail, 0.75 rem,
            // gives way down to 0.25 rem where the detail needs its floor.
            .when(stacked, |element| element.child(div().debug_selector(|| "stash-stack-gap".into()).flex_shrink(STACKED_SPARE_SHRINK).h(rems(0.75)).min_h(rems(0.25))))
            // The column tracks whether focus lies in the inspection
            // ([`Self::inspection_focus`]); a press in it has GPUI focus
            // only the control pressed, whose own listener runs first, and
            // never the column, which would hold a focus no key acts on.
            .when(detail, |element| element.child(div().debug_selector(|| "stash-detail-column".into()).track_focus(&self.inspection_focus).on_any_mouse_down(|_, window, _| window.prevent_default()).flex_1().min_w_0().min_h_0().when_some(floor, |column, floor| column.flex_basis(floor)).flex().flex_col().gap_2().child(cue.list(div().on_children_prepainted(cue.observe(self.detail.observe())).id("stash-detail").debug_selector(|| "stash-detail".into()).flex_1().min_w_0().min_h_0().p(room).mx(-room).mt(-room).overflow_y_scroll().track_scroll(self.detail.scroll()).flex().flex_col().gap_2()
                .children(self.loading.map(|loading| native_accessibility::text("stash-detail-loading", loading).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted))))
                .children(self.error.as_ref().map(|error| form_error(error, cx)))
                .when_some(snapshot, |element, snapshot| element
                    .child(div().flex().flex_col().gap_1()
                        // The title, as a row's message, is whole in its
                        // tooltip only where its room cuts it, measured in
                        // its own style by the box it fills.
                        .child({ use gpui_kit::base::ElementExt as _; let size = crate::appearance::ui_text(14.); div().min_w_0().text_size(size).font_weight(FontWeight::SEMIBOLD).on_prepaint(measure_message(&self.messages_laid_out, MessageLine::Title, snapshot.stash.name.clone(), size)).child(native_accessibility::text("stash-detail-name", snapshot.stash.name.clone()).truncate().tooltip(message_tooltip(&self.messages_laid_out, MessageLine::Title))) })
                        .child(native_accessibility::text("stash-detail-identity", format!("{} · {} · base {} · saved index {}", snapshot.stash.selector, short_oid(&snapshot.stash.oid), short_oid(&snapshot.base_oid), short_oid(&snapshot.index_oid))).text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted))))
                    .child(div().flex().flex_wrap().items_center().gap_1().on_children_prepainted(self.detail.controls(areas.iter().cloned().map(Some).collect(), room)).children([StashArea::Staged, StashArea::Unstaged, StashArea::Untracked].into_iter().zip(areas).map(|(area, focus)| {
                        button(("stash-area", area as usize), format!("{} ({})", area.label(), area.files(&snapshot).len()), "", self.area == area).track_focus(&focus).debug_selector(move || format!("stash-area-{}", area as usize)).on_click(cx.listener(move |this, _, _, cx| this.choose_area(area, cx)))
                    })))
                    // The saved files, one Tab stop with the arrow keys
                    // inside. They are no taller than the detail's view holds
                    // with their ring, its content box inside the ring's room
                    // (`max_h` of it), so the keyboard reveals them whole:
                    // stacked beneath the list at 18 pt in a 461 × 490 window
                    // they took 85 px of a 63.5 px detail, which no reveal
                    // could show whole, and the detail's clip cut their ring.
                    // Their rows scroll inside them.
                    .child(div().flex().flex_col().flex_shrink_0().h(px(if narrow { 85. } else { 130. })).max_h(relative(1.)).on_children_prepainted(self.detail.control(self.file_focus.clone(), room)).child(crate::native_accessibility::clips_children(div().id("stash-files-focus").debug_selector(|| "stash-files".into()).role(Role::ListBox).aria_label("Saved files").tab_stop(true).track_focus(&self.file_focus).flex_1().min_h_0().flex().flex_col().border_1().border_color(rgb(if files_ringed { p.accent } else { p.border })).rounded(px(5.)).overflow_hidden()
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if let Some(index) = list_key_selection(event.keystroke.key.as_str(), this.selected_file, this.files().len()) {
                                cx.stop_propagation();
                                this.select_file(index, window, cx);
                                this.file_scroll.scroll_to_item(index, ScrollStrategy::Center);
                            }
                        }))
                        .when(!self.files().is_empty(), |element| element.child(files))
                        .when(self.files().is_empty(), |element| element.child(native_accessibility::text("stash-files-empty", "No saved files in this group.").p_3().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))))))
                    .child(div().flex().items_center().gap_1().on_children_prepainted(self.detail.controls(mode_focus, room)).children(modes.into_iter().map(|(mode, label, focus)| {
                        button(("stash-text-mode", mode), label, "", self.text_mode == mode).track_focus(&focus).disabled(!matches!(self.content.as_deref(), Some(Content::Text { .. }))).on_click(cx.listener(move |this, _, window, cx| { this.text_mode = mode; this.ensure_editor(window, cx); cx.notify(); }))
                    })))
                    .child(div().debug_selector(|| "stash-comparison".into()).when(file_chosen, |element| element.flex_1().min_h(crate::appearance::ui_size(STASH_CONTENT_FLOOR))).when(!file_chosen, |element| element.flex_grow(1.).flex_shrink_0()).border_1().border_color(rgb(p.border)).rounded(px(5.)).overflow_hidden().on_children_prepainted(self.detail.group(text_focus, room))
                        // A chosen file's comparison is built for the box's
                        // width in the frame it is drawn (`in_frame`), so its
                        // open controls take the form that width leaves them
                        // in the first frame after a resize; until then the
                        // box holds its prompt, whose height it keeps.
                        .when(file_chosen, |element| element.flex().flex_col().child(crate::in_frame::in_frame(move |bounds, window, cx| browser.update(cx, |this, cx| this.render_content(bounds.size.width, window, cx)).unwrap_or_else(|_| div().into_any_element())).named("stash-comparison")))
                        .when(!file_chosen, |element| element.child(self.render_content(px(0.), window, cx))))
                    .children(inline))
                // Where its loading line stood, at the detail's edge, and
                // short enough for one line beside the stacked list at 18 pt
                // in a 461 × 490 window: the centred prompt that named every
                // group wrapped there, past the detail's room, and left
                // "files." alone on its last line at 13 pt.
                .when(self.snapshot.is_none() && self.loading.is_none() && self.error.is_none(), |element| element.child(native_accessibility::text("stash-detail-placeholder", "Choose a stash to inspect its files.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))), "stash-detail-scrollbar").flex_1().min_w_0().min_h_0()).children(footer)))
    }
}

/// Prepares Amend last commit for `head`, whose form opens once the read
/// answers, for the prose fields' Tab regression in `views::tests`.
#[cfg(test)]
pub(crate) fn prepare_amend(
    app: &mut GitTurtle,
    head: String,
    window: &mut Window,
    cx: &mut Context<GitTurtle>,
) {
    app.prepare_commit_recovery(RecoveryKind::Amend, head, None, window, cx);
}

/// The open Amend form's description field.
#[cfg(test)]
pub(crate) fn amend_description(app: &GitTurtle, cx: &App) -> Option<Entity<TextareaState>> {
    let form = app.recovery.amend.as_ref()?;
    Some(form.read(cx).fields.as_ref()?.description.clone())
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
