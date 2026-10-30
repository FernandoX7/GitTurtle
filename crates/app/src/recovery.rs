//! Explicit, prepared recovery actions and a read-only stash inspector.
use crate::native_accessibility::LiveRegion as _;
use crate::*;
use gitturtle_core::{
    CommitRecoveryPlan, RecoveryCommand, RecoveryKind, StashPage, StashSnapshot, WriteCommand,
};
use gpui_kit::component::{
    WindowExt,
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    menu::{PopupMenu, PopupMenuItem},
    scroll::{Scrollbar, ScrollbarMode},
};
use gpui_kit::prelude::FluentBuilder;

const PREPARING: &str = "Preparing recovery action…";
/// Why a recovery action refuses while an integration is paused, in the
/// words of the core's plans (`GitTurtle::read_recovery`).
pub(super) const PAUSED_REFUSAL: &str =
    "Finish the current operation and resolve conflicts before starting another recovery action";
/// What a recovery form says in place of [`PAUSED_REFUSAL`] once a status
/// read finds the integration it refused for ended
/// (`GitTurtle::integration_ended`).
const PAUSE_ENDED: &str = "The paused operation has ended. Review again to continue.";
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
fn first_line(answer: &str) -> Option<&str> {
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
/// The least height a saved file's comparison keeps in the Stashes detail,
/// at the default interface size, where the window leaves the detail less
/// than its inspection and it scrolls: about six lines of a patch.
const STASH_CONTENT_FLOOR: f32 = 120.;
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
}

pub(super) struct CommitTarget {
    oid: String,
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
        parents: commit.parents.clone(),
    })
}

enum Prepared {
    Amend(Arc<CommitRecoveryPlan>),
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
        let message = PAUSED_REFUSAL.to_owned();
        cx.defer_in(window, move |this, window, cx| {
            match failure {
                Some(failure) => failure(message, window, cx),
                None => {
                    this.operation_error = Some(message);
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
        let path = self.path.clone();
        if let Some(browser) = self.recovery.browser.as_ref().and_then(WeakEntity::upgrade)
            && path.as_deref() == Some(browser.read(cx).repository.path())
        {
            browser.update(cx, |browser, cx| {
                if pause_ended(&mut browser.restore_line, &mut browser.pause_ended) {
                    browser.reveal_note_for(NoteReveal::PauseEnded);
                    crate::copy_feedback::announce(window, cx, PAUSE_ENDED);
                    cx.notify();
                }
            });
        }
        if let Some(form) = &self.recovery.amend
            && form.read(cx).repository == path
        {
            form.update(cx, |form, cx| {
                if pause_ended(&mut form.error, &mut form.pause_ended) {
                    cx.notify();
                }
            });
        }
        if let Some(form) = &self.recovery.stash
            && form.read(cx).repository == path
        {
            form.update(cx, |form, cx| {
                if pause_ended(&mut form.error, &mut form.pause_ended) {
                    cx.notify();
                }
            });
        }
        if self.operation_error.as_deref() == Some(PAUSED_REFUSAL) {
            self.operation_error = None;
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
                    matches!(&result, Err(error) if format!("{error:#}") == PAUSED_REFUSAL);
                if refused_for_pause != this.integration_state.is_some() {
                    this.read_pause_again(window, cx);
                }
                match result {
                    Ok(Prepared::Amend(plan)) => this.show_amend_form(plan, window, cx),
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
                        let message = format!("{error:#}");
                        if let Some(failure) = failure {
                            failure(message, window, cx);
                        } else {
                            this.operation_error = Some(message);
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
        let selection_unavailable = self.operation_busy.is_some()
            || self.working_paths.pending
            || self.working_paths.error.is_some();
        let refreshing = self.status_task.is_some();
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let head = self
            .work_status
            .as_ref()
            .and_then(|status| status.head.clone());
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
                // A paused integration, read here too, disables Save changes
                // to stash…, Amend and Undo, as the menu is built again when
                // one pauses or ends (`native_accessibility::MenuInputs`).
                let (busy, paused) = owner.upgrade().map_or((true, true), |owner| {
                    let owner = owner.read(cx);
                    (owner.write_unsettled(), owner.integration_state.is_some())
                });
                if let Some(refresh) = tight {
                    menu = menu
                        .item(
                            working_item(
                                "Directories",
                                busy,
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
                            unstaged == 0 || selection_unavailable || busy,
                            &owner,
                            &repository,
                            |this, window, cx| this.apply_selected_files(false, window, cx),
                        ))
                        .item(working_item(
                            format!("Unstage {staged} selected"),
                            staged == 0 || selection_unavailable || busy,
                            &owner,
                            &repository,
                            |this, window, cx| this.apply_selected_files(true, window, cx),
                        ));
                    if refresh {
                        menu = menu.item(working_item(
                            "Refresh working changes",
                            refreshing || busy,
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
                    ignore_entry.is_none() || busy,
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
                    !has_changes || paused || busy,
                    &owner,
                    &repository,
                    |this, window, cx| this.open_stash_form(window, cx),
                );
                let menu = working_menu_item(
                    menu,
                    "Browse stashes…",
                    busy,
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
                    paused || busy,
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
                    paused || busy,
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
            // A write's start or end builds the open menu again.
            .follows_writes(cx.entity().downgrade())
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
            .app_menu(move |menu, _, cx| {
                Self::recovery_context_menu(menu, &owner, &repository, &commit, cx)
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
        cx: &App,
    ) -> PopupMenu {
        let Some(this) = owner.upgrade() else {
            return menu;
        };
        let (disabled, current) = this.read(cx).commit_menu_state(commit);
        commit_menu(menu, owner, repository, commit, disabled, current)
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
    ) -> impl Fn(PopupMenu, &GitTurtle) -> PopupMenu + 'static {
        move |menu, app| {
            let (disabled, current) = app.commit_menu_state(&commit);
            commit_menu(menu, &owner, &repository, &commit, disabled, current)
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

    /// Whether a commit's menu offers nothing while a write or an
    /// integration runs, and whether the commit is HEAD, which adds Amend
    /// and Undo.
    fn commit_menu_state(&self, commit: &CommitTarget) -> (bool, bool) {
        let disabled = self.write_unsettled() || self.integration_state.is_some();
        let current = self
            .work_status
            .as_ref()
            .and_then(|status| status.head.as_ref())
            == Some(&commit.oid);
        (disabled, current)
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
                                .w_full().on_click(move |_, window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        if this.path == repository && this.page == AppPage::Repository {
                                            window.close_dialog(cx);
                                            this.prepare_commit_recovery(kind, oid.clone(), Some(index + 1), window, cx);
                                        }
                                    });
                                })
                        }))))
                    .button_props(DialogButtonProps::default().ok_text("Cancel"))
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

    fn open_stash_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() {
            return;
        }
        let path = self.path.clone();
        let form = self
            .recovery
            .stash
            .as_ref()
            .filter(|form| form.read(cx).repository == path)
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
        let opened = browser.clone();
        self.recovery.browser = Some(browser.downgrade());
        // The dialog holds the browser's only handle, so however it closes,
        // the browser's release stops a Review restore… nobody is waiting
        // for (`StashBrowser::new`).
        window.open_alert_dialog(cx, move |dialog, window, _| {
            dialog
                .title(native_accessibility::dialog_title(
                    "stashes-title",
                    "Stashes",
                ))
                .width(appearance::dialog_width(px(1060.), window))
                .max_h(appearance::dialog_max_height(window))
                .button_props(DialogButtonProps::default().ok_text("Close"))
                // In the dialog's content rather than its scrolling body,
                // whose scroller lays the browser out at the height it asks
                // for and cut its lines mid-line where the cap left less:
                // here the browser gives up what the cap takes.
                .content({
                    let browser = browser.clone();
                    move |content, _, _| content.min_h_0().child(browser.clone())
                })
        });
        Some(opened)
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
                    if succeeded && form.read(cx).message(cx) == *message {
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
    disabled: bool,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    action: impl Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) + 'static,
) -> PopupMenu {
    menu.item(working_item(label, disabled, owner, repository, action))
}

/// A Working Changes menu item that acts only on the repository it was
/// built for, on the repository page, while no operation runs.
fn working_item(
    label: impl Into<SharedString>,
    disabled: bool,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    action: impl Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) + 'static,
) -> PopupMenuItem {
    let owner = owner.clone();
    let repository = repository.clone();
    PopupMenuItem::new(label)
        .disabled(disabled)
        .on_click(move |_, window, cx| {
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

fn commit_menu(
    mut menu: PopupMenu,
    owner: &WeakEntity<GitTurtle>,
    repository: &Option<PathBuf>,
    commit: &Arc<CommitTarget>,
    disabled: bool,
    current: bool,
) -> PopupMenu {
    for (label, kind) in [
        ("Revert this commit…", RecoveryKind::Revert),
        ("Cherry-pick this commit…", RecoveryKind::CherryPick),
    ] {
        let commit = Arc::clone(commit);
        menu = working_menu_item(
            menu,
            label,
            disabled,
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
                disabled,
                owner,
                repository,
                move |this, window, cx| {
                    this.choose_commit_recovery(kind, Arc::clone(&commit), window, cx)
                },
            );
        }
    }
    menu
}

fn append_paths(explanation: &mut String, paths: &[PathBuf]) {
    explanation.push_str(&format!(
        "\n\n{} affected {}",
        paths.len(),
        if paths.len() == 1 { "path" } else { "paths" }
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

struct AmendForm {
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    plan: Arc<CommitRecoveryPlan>,
    title: Entity<InputState>,
    description: Entity<TextareaState>,
    initial: CommitDraft,
    error: Option<String>,
    /// Whether the line beneath the fields says that the integration its
    /// last review was refused for has ended ([`PAUSE_ENDED`]).
    pause_ended: bool,
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
        let initial = split_message(&plan.target.message);
        let title = cx.new(|cx| InputState::new(window, cx).default_value(initial.title.clone()));
        let description =
            cx.new(|cx| TextareaState::new(window, cx).default_value(initial.description.clone()));
        Self {
            owner,
            repository,
            plan,
            title,
            description,
            initial,
            error: None,
            pause_ended: false,
            opened: 0,
        }
    }
    fn draft(&self, cx: &App) -> CommitDraft {
        CommitDraft {
            title: self.title.read(cx).value().to_string(),
            description: self.description.read(cx).value().to_string(),
        }
    }
    fn message(&self, cx: &App) -> String {
        amended_message(&self.plan.target.message, &self.initial, &self.draft(cx))
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.pause_ended = false;
        if self.title.read(cx).value().trim().is_empty() {
            self.error = Some("Enter a commit title.".into());
            cx.notify();
            return false;
        }
        let message = self.message(cx);
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
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child("Title")
                    .child(Input::new(&self.title).aria_label("Amended commit title")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child("Description · optional")
                    .child(shortcuts::tab_leaves(
                        Textarea::new(&self.description)
                            .h(px(170.))
                            .aria_label("Amended commit description"),
                    )),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child("The next step reviews the staged snapshot and the effect on history."),
            )
            .children(form_line(
                self.error.as_deref(),
                self.pause_ended,
                self.opened,
                cx,
            ))
    }
}
fn show_amend(form: Entity<AmendForm>, window: &mut Window, cx: &mut App) {
    form.update(cx, |form, _| form.opened += 1);
    window.open_alert_dialog(cx, move |dialog, window, _| {
        let submit = form.clone();
        dialog
            .title("Amend last commit")
            .width(appearance::dialog_width(px(580.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(form.clone())
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Review amend")
                    .cancel_text("Cancel")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
    });
}

struct StashForm {
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    name: Entity<InputState>,
    include_untracked: bool,
    error: Option<String>,
    /// Whether the line beneath the fields says that the integration its
    /// last review was refused for has ended ([`PAUSE_ENDED`]).
    pause_ended: bool,
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
        let name = cx
            .new(|cx| InputState::new(window, cx).placeholder("Describe the work you are pausing"));
        Self {
            owner,
            repository,
            name,
            include_untracked: false,
            error: None,
            pause_ended: false,
            opened: 0,
        }
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.pause_ended = false;
        let name = self.name.read(cx).value().to_string();
        if name.trim().is_empty() {
            self.error = Some("Give this stash a name so you can find it later.".into());
            cx.notify();
            return false;
        }
        let include_untracked = self.include_untracked;
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
                let plan = repo.stash_create_plan(include_untracked)?;
                let mut explanation = format!("Save “{name}” from {}. Tracked staged and unstaged changes will be saved separately, then removed from this working copy. {}", plan.current.branch,
                    if include_untracked { "Untracked files are included and removed from the working copy. Ignored files remain." } else { "Untracked and ignored files remain in the working copy." });
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
            .child(div().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child("Pause the current work and keep a named snapshot you can inspect before restoring."))
            .child(div().flex().flex_col().gap_1().child("Stash name").child(Input::new(&self.name).aria_label("Stash name")))
            .child(Checkbox::new("stash-include-untracked").label("Include untracked files").checked(self.include_untracked)
                .on_click(cx.listener(|this, checked, _, cx| { this.include_untracked = *checked; cx.notify(); })))
            .children(form_line(self.error.as_deref(), self.pause_ended, self.opened, cx))
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
            .button_props(
                DialogButtonProps::default()
                    .ok_text("Review stash")
                    .cancel_text("Cancel")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
    });
}
/// Replaces a recovery form's `error` that refuses because an integration
/// was paused with the line that says it has ended, `ended`; returns whether
/// it did. Any other reason stays.
fn pause_ended(error: &mut Option<String>, ended: &mut bool) -> bool {
    if error.as_deref() != Some(PAUSED_REFUSAL) {
        return false;
    }
    *error = None;
    *ended = true;
    true
}

/// The line beneath a recovery form's fields: why its last review was
/// refused, an alert announced at once, again by a fresh node each time the
/// form opens again with it (`opened`, `native_accessibility::refusal_line`),
/// or, `pause_ended`, that the integration it was refused for has ended, a
/// status heard once assistive technology finishes speaking. The refusal was
/// no live region, so it went unannounced as it first appeared and as the
/// form opened again with it.
fn form_line(
    error: Option<&str>,
    pause_ended: bool,
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
        None => pause_ended.then(|| {
            native_accessibility::static_text(
                div()
                    .id("recovery-form-note")
                    .debug_selector(|| "recovery-form-note".into())
                    .role(Role::Status)
                    .aria_label(PAUSE_ENDED)
                    .polite()
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(palette(cx).muted))
                    .child(PAUSE_ENDED),
                PAUSE_ENDED,
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
    metadata_task: Option<Task<()>>,
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
    file_focus: FocusHandle,
    loading: Option<&'static str>,
    error: Option<String>,
    preview_error: Option<String>,
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
    /// Whether that note says the integration Review restore… was last
    /// refused for has ended ([`PAUSE_ENDED`]).
    pause_ended: bool,
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
    let read = browser.update(cx, |this, _| this.metadata_task.take());
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

/// Why the open Stashes last refused Review restore…, the line its footer
/// note says, for tests in other modules.
#[cfg(test)]
pub(crate) fn stash_restore_line(app: &GitTurtle, cx: &App) -> Option<String> {
    let browser = app.recovery.browser.as_ref()?.upgrade()?;
    browser.read(cx).restore_line.clone()
}

impl StashBrowser {
    /// Returns once every read the browser has started, the saved stashes
    /// or a selected stash's inspection, has landed and been drawn. They run
    /// on the browser's own thread, which `run_until_parked` cannot wait for:
    /// once it drains, each has answered, which holds the test clock, and the
    /// replies land as the test parks.
    #[cfg(test)]
    pub(crate) fn lands(browser: &Entity<Self>, cx: &mut VisualTestContext) {
        browser.read_with(cx, |browser, _| browser.metadata.drain());
        crate::views::tests::draw(cx);
        assert!(
            browser.read_with(cx, |browser, _| browser.metadata_task.is_none()
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
            // The saved files likewise: Tab passed them by.
            file_focus: cx.focus_handle().tab_stop(true),
            loading: None,
            error: None,
            preview_error: None,
            restore_index: false,
            restoring: None,
            restore_line: None,
            restore_alert: None,
            pause_ended: false,
            reveal_note: Default::default(),
            note_lines: std::rc::Rc::new(std::cell::Cell::new(2)),
            restore_at: Default::default(),
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
        crate::system_preview::Opener::new(self.owner.clone(), self.showing)
            .revealed_by(self.detail.clone(), appearance::button_ring_room(cx))
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
    #[cfg_attr(not(test), allow(dead_code))]
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
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.clear_preview();
        self.snapshot = None;
        self.detail.scroll().set_offset(Point::default());
        self.selected = None;
        self.selected_file = None;
        self.loading = Some("Reading stashes…");
        self.error = None;
        self.forget_restore_line(cx);
        let repo = self.repository.clone();
        let response = self
            .metadata
            .submit_read(move || repo.stash_list(offset, STASH_PAGE_SIZE));
        self.metadata_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Stash listing was interrupted. Refresh to try again."
                ))
            });
            let _ = this.update_in(cx, |this, _, cx| {
                if this.generation != generation {
                    return;
                }
                this.metadata_task = None;
                this.loading = None;
                if !this.is_current(cx) {
                    this.error = Some(
                        "The repository changed. Close this inspector and open Stashes again."
                            .into(),
                    );
                    cx.notify();
                    return;
                }
                match result {
                    Ok(page) => {
                        this.page = Some(page);
                        this.offset = offset;
                        this.stash_scroll.scroll_to_item(0, ScrollStrategy::Top);
                    }
                    Err(error) => this.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_stash(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stash) = self
            .page
            .as_ref()
            .and_then(|page| page.entries.get(index))
            .cloned()
        else {
            return;
        };
        window.focus(&self.stash_focus, cx);
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
        self.snapshot = None;
        self.detail.scroll().set_offset(Point::default());
        self.selected = Some(index);
        self.selected_file = None;
        self.loading = Some("Reading saved changes…");
        self.error = None;
        self.forget_restore_line(cx);
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
            let _ = this.update_in(cx, |this, _, cx| {
                if this.generation != generation {
                    return;
                }
                this.metadata_task = None;
                this.loading = None;
                if !this.is_current(cx) {
                    this.error = Some("The repository changed. Open Stashes again.".into());
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
                    Err(error) => this.error = Some(format!("{error:#}")),
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
        let busy = self.metadata_task.is_some();
        let (refused, dimmed) =
            crate::workspace::dialog_refusal(&self.owner, Some(self.repository.path()), cx);
        (busy || refused, busy || dimmed)
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
        // Its label is the compact helper's, as Drop…'s beside it is: the
        // kit's small label, `text_sm`, drew 11.375 px beside Drop…'s 12 px
        // at 13 pt.
        let restore = Button::new("restore-saved-stash")
            .primary()
            .small()
            .text_size(crate::appearance::ui_text(crate::LABEL_TEXT))
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
        self.pause_ended = false;
        self.reveal_note_for(NoteReveal::Pressed(held));
        cx.notify();
    }

    /// Has the next frame that draws Stashes reveal the footer note for
    /// `cause` ([`NoteReveal::due_in`]).
    fn reveal_note_for(&self, cause: NoteReveal) {
        NoteReveal::due_in(&self.reveal_note, cause);
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
        self.pause_ended = false;
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
            let report = reason != PAUSED_REFUSAL && first_line(reason).is_some();
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
        if self.pause_ended {
            return native_accessibility::static_text(
                div()
                    .id("stash-restore-ended")
                    .debug_selector(|| "stash-restore-ended".into())
                    .role(Role::Status)
                    .aria_label(PAUSE_ENDED)
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    // As the refusal it replaces keeps its lines.
                    .line_clamp(lines)
                    .text_ellipsis()
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(PAUSE_ENDED).build(window, cx)
                    })
                    .child(PAUSE_ENDED),
                PAUSE_ENDED,
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

    fn drop_stash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let stash = snapshot.stash.clone();
        let repository = self.repository.path().to_owned();
        let _ = self.owner.update(cx, |this, cx| {
            if this.path.as_ref() != Some(&repository) || this.operation_busy.is_some() { return; }
            window.close_dialog(cx);
            this.confirm_git_write("Drop saved stash".into(), format!("Remove “{}” ({}) from the saved stash list. Its saved changes will no longer be available here. Current index and working files stay as they are.\n\nInspect or restore any work you need before dropping this stash.", stash.name, short_oid(&stash.oid)), "Drop stash", WriteCommand::Recovery(RecoveryCommand::DropStash { stash }.into()), window, cx);
        });
    }

    fn render_stash_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(stash) = self.page.as_ref().and_then(|page| page.entries.get(index)) else {
            return div().into_any_element();
        };
        let p = palette(cx);
        let selected = self.selected == Some(index);
        div()
            .id(("saved-stash", stash.ordinal))
            .debug_selector(move || format!("saved-stash-row-{index}"))
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .aria_label(format!("{} · {}", stash.name, stash.selector))
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
            .child(
                div()
                    .truncate()
                    .text_size(crate::appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(stash.name.clone()),
            )
            .child(
                div()
                    .truncate()
                    .text_size(crate::appearance::ui_text(10.))
                    .text_color(rgb(p.muted))
                    .child(format!(
                        "{} · {} · {}",
                        stash.selector,
                        short_oid(&stash.oid),
                        short_date(stash.timestamp)
                    )),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select_stash(index, window, cx)))
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
    /// one form, from the narrower room a side's name leaves: labelled
    /// where the label fits beside the name, otherwise the icon, on the
    /// name's line where the icon fits there, otherwise both on a line
    /// beneath the names; a side without one keeps that line, and each
    /// side's line saying why takes the taller answer's lines, at most two,
    /// so the sides' names and images stay level.
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
        let room = [0, 1]
            .into_iter()
            .filter(|index| targets[*index].is_some())
            .map(|index| {
                inner
                    - rem * 0.5
                    - crate::views::line_widths(&[(captions[index].as_str(), text)], window)
            })
            .reduce(|narrower, room| narrower.min(room));
        let square = appearance::ui_size(28.);
        let inline = room.is_none_or(|room| room >= square);
        let offered = room.is_some();
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
                target.control(offer.form(room.unwrap_or_default(), window), cx)
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
        match self.content.as_deref() {
            Some(Content::Text { diagrams, .. }) if self.text_mode == 3 => {
                diagrams.as_ref().map_or_else(
                    || {
                        div()
                            .child("No Mermaid diagrams on this file; use Source.")
                            .into_any_element()
                    },
                    |preview| {
                        crate::rich_preview::render_comparison(
                            preview,
                            false,
                            self.opener(cx),
                            width,
                            window,
                            cx,
                        )
                    },
                )
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
        }
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
        let busy = self.metadata_task.is_some();
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
            Some(_) => format!("Saved work · {}–{}", self.offset + 1, self.offset + stashes),
            None => "Saved work".to_owned(),
        };
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
        div().debug_selector(|| "stash-browser".into()).h(px(height)).min_h_0().flex().gap_3().when(narrow, |element| element.flex_col())
            .child(div().debug_selector(|| "stash-list-panel".into()).w(px(255.)).min_w_0().flex_shrink_0().h_full().flex().flex_col().border_1().border_color(rgb(p.border)).rounded(px(7.)).overflow_hidden()
                .when(narrow && detail, |element| element.w_full().h(crate::appearance::ui_size(140.)))
                .when(!detail, |element| element.w_full().flex_1())
                .child(div().h(crate::appearance::ui_size(38.)).flex().items_center().px_2().gap_1().border_b_1().border_color(rgb(p.border))
                    .child(native_accessibility::text("stash-list-heading", heading).flex_1().text_size(crate::appearance::ui_text(11.)).font_weight(FontWeight::SEMIBOLD))
                    .child(button("refresh-stashes", "", "refresh", false).accessibility_label("Refresh saved stashes").disabled(busy).on_click(cx.listener(|this, _, window, cx| this.load_page(0, window, cx)))))
                .child(crate::native_accessibility::clips_children(div().id("stash-list-focus").debug_selector(|| "stash-list".into()).role(Role::ListBox).aria_label("Saved stashes").tab_stop(true).track_focus(&self.stash_focus).relative().flex_1().min_h_0().flex().flex_col()
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if let Some(index) = list_key_selection(event.keystroke.key.as_str(), this.selected, this.page.as_ref().map_or(0, |page| page.entries.len())) {
                            cx.stop_propagation();
                            this.select_stash(index, window, cx);
                            this.stash_scroll.scroll_to_item(index, ScrollStrategy::Center);
                        }
                    }))
                    .when(stashes > 0, |element| element.child(rows))
                    .when(self.page.as_ref().is_some_and(|page| page.entries.is_empty()), |element| element.child(div().px_3().py_2().child(EmptyState::new("stash-list-empty", "No saved stashes · Save one from the Actions menu in Changes").block(cx))))
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
            .when(detail, |element| element.child(div().debug_selector(|| "stash-detail-column".into()).flex_1().min_w_0().min_h_0().flex().flex_col().gap_2().child(cue.list(div().on_children_prepainted(cue.observe(self.detail.observe())).id("stash-detail").debug_selector(|| "stash-detail".into()).flex_1().min_w_0().min_h_0().p(room).mx(-room).mt(-room).overflow_y_scroll().track_scroll(self.detail.scroll()).flex().flex_col().gap_2()
                .children(self.loading.map(|loading| native_accessibility::text("stash-detail-loading", loading).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted))))
                .children(self.error.as_ref().map(|error| form_error(error, cx)))
                .when_some(snapshot, |element, snapshot| element
                    .child(div().flex().flex_col().gap_1()
                        .child(native_accessibility::text("stash-detail-name", snapshot.stash.name.clone()).truncate().text_size(crate::appearance::ui_text(14.)).font_weight(FontWeight::SEMIBOLD))
                        .child(native_accessibility::text("stash-detail-identity", format!("{} · base {} · saved index {}", short_oid(&snapshot.stash.oid), short_oid(&snapshot.base_oid), short_oid(&snapshot.index_oid))).text_size(crate::appearance::ui_text(10.)).text_color(rgb(p.muted))))
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
    Some(form.read(cx).description.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn amend_fields_preserve_description_whitespace_and_comment_lines() {
        let message = "Title\n\n  indented\n# literal description\n\n";
        let draft = split_message(message);
        assert_eq!(draft.title, "Title");
        assert_eq!(draft.description, "  indented\n# literal description\n\n");
        assert_eq!(draft.message(), message);
    }
    #[test]
    fn legacy_single_newline_messages_split_without_losing_body_text() {
        assert_eq!(
            split_message("Title\nbody\n"),
            CommitDraft {
                title: "Title".into(),
                description: "body\n".into()
            }
        );
        assert_eq!(
            split_message("Only title"),
            CommitDraft {
                title: "Only title".into(),
                description: String::new()
            }
        );
    }

    #[test]
    fn unchanged_amend_preserves_exact_legacy_message_and_edits_use_blank_separator() {
        let original = "Title\n  body\n# literal\n";
        let initial = split_message(original);
        assert_eq!(amended_message(original, &initial, &initial), original);
        let edited = CommitDraft {
            title: "Revised title".into(),
            description: initial.description.clone(),
        };
        assert_eq!(
            amended_message(original, &initial, &edited),
            "Revised title\n\n  body\n# literal\n"
        );
    }

    /// The stash inspector's unified patch keeps its decorations for the editor's life, and
    /// its preview figure counts them along with the presentation they were built from.
    #[gpui::test]
    async fn stash_patch_preview_counts_its_decorations_and_presentation(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.executor().allow_parking();
        let fixture = tempfile::tempdir().unwrap();
        let repository = GitRepository::init(fixture.path().join("repository"), "main").unwrap();
        cx.update(gpui_kit::init);
        let captured: std::rc::Rc<std::cell::RefCell<Option<Entity<StashBrowser>>>> =
            Default::default();
        let output = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let browser =
                cx.new(|cx| StashBrowser::new(WeakEntity::new_invalid(), repository, window, cx));
            *output.borrow_mut() = Some(browser.clone());
            gpui_kit::component::Root::new(browser, window, cx)
        });
        cx.run_until_parked();
        let browser = captured.borrow().as_ref().unwrap().clone();
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| {
                browser.content = Some(Arc::new(text::decorated_patch_content()));
                browser.text_mode = 0;
                browser.ensure_editor(window, cx);
                let Some(Content::Text { presentation, .. }) = browser.content.as_deref() else {
                    unreachable!()
                };
                let presentation = presentation.retained_bytes();
                let decoration = browser.decoration.clone().expect("kept with the editor");
                let decorated = decoration.retained_bytes();
                assert!(decorated > 0, "a patch with changes has decorations");

                let with = browser.preview_retained_bytes();
                browser.decoration = None;
                let without = browser.preview_retained_bytes();
                assert_eq!(with - without, decorated);
                browser.content = None;
                assert!(without - browser.preview_retained_bytes() >= presentation);

                browser.decoration = Some(decoration);
                browser.clear_preview();
                assert!(browser.decoration.is_none());
                assert_eq!(browser.preview_retained_bytes(), 0);
            })
        });
    }

    #[test]
    fn stash_list_navigation_bounds_empty_and_changed_pages() {
        assert_eq!(list_key_selection("down", None, 0), None);
        assert_eq!(list_key_selection("down", None, 4), Some(0));
        assert_eq!(list_key_selection("up", Some(0), 4), Some(0));
        assert_eq!(list_key_selection("down", Some(3), 4), Some(3));
        assert_eq!(list_key_selection("up", Some(39), 4), Some(3));
        assert_eq!(list_key_selection("end", None, 4), Some(3));
        assert_eq!(list_key_selection("home", Some(3), 4), Some(0));
        assert_eq!(list_key_selection("escape", Some(3), 4), None);
    }

    /// In a 461 × 490 half tile, at 13 and 18 pt, with a stash selected,
    /// every Tab stop of the Stashes dialog draws its ring whole and in view:
    /// the detail scrolls the control the keyboard moves onto into view with
    /// its ring, as the dialogs' lists do. Its group, mode and restore
    /// controls once stayed below the detail's fold, drawing no ring.
    #[gpui::test]
    async fn a_narrow_stash_detail_shows_every_stop_whole(cx: &mut gpui_kit::TestAppContext) {
        use crate::focus_ring::sweep;
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
        git(
            repo.path(),
            &[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                "kept for later",
            ],
        );
        let (app, cx) = window(cx, &repo);
        sweep::prepare(&app, cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        let mut failures = Vec::new();
        for interface in sweep::SIZES {
            sweep::interface_size(cx, interface);
            sweep::open_dialog(&app, cx, "Stashes", |app, window, cx| {
                app.open_stashes(window, cx)
            })
            .await;
            // The saved stashes, then the selected stash's inspection, are
            // read on the browser's own threads.
            let landed = |cx: &mut VisualTestContext, selector: &'static str| {
                for _ in 0..400 {
                    cx.executor()
                        .advance_clock(std::time::Duration::from_millis(50));
                    draw(cx);
                    if cx.debug_bounds(selector).is_some() {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                panic!("{selector} never drawn");
            };
            landed(cx, "saved-stash-row-0");
            // A click focuses the list; Home selects the stash from the
            // keyboard, as a person walking it with keys would.
            let row = cx.debug_bounds("saved-stash-row-0").unwrap();
            cx.simulate_click(row.center(), Modifiers::none());
            cx.simulate_keystrokes("home");
            landed(cx, "stash-area-0");
            sweep::walk(&app, cx, "Stashes", interface, &mut failures);
            sweep::close_dialogs(cx, "Stashes", interface, &mut failures);
        }
        sweep::assert_whole(failures);
    }

    /// At 18 pt in a 1000 × 680 window the Stashes detail shows Restore
    /// staged state, Drop… and Review restore… whole as a stash is selected,
    /// without scrolling, and keeps them there, its footer, as a chosen
    /// file's comparison takes its floor and the detail scrolls: they lay
    /// below the scrolling detail's fold, as the empty comparison kept the
    /// floor a file's takes. Until a file is chosen the comparison keeps its
    /// prompt's height. Stacked in a 461 × 490 window, where the detail is a
    /// few lines tall, the actions scroll with the rest.
    #[gpui::test]
    async fn the_stash_actions_stay_in_view_beside_the_list(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, scroll_down, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
        git(
            repo.path(),
            &[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                "kept for later",
            ],
        );
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        let landed = |cx: &mut VisualTestContext, selector: &'static str| {
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        let bounds = |cx: &mut VisualTestContext, selector: &'static str| {
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("no {selector}"))
        };
        let slack = px(0.5);
        for (width, height, pinned) in [(1000., 680., true), (461., 490., false)] {
            let at = format!("18 pt in {width} × {height}");
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
            landed(cx, "saved-stash-row-0");
            let row = bounds(cx, "saved-stash-row-0");
            cx.simulate_click(row.center(), Modifiers::none());
            landed(cx, "stash-actions");
            // Until a file is chosen, the comparison is its prompt's height.
            let comparison = bounds(cx, "stash-comparison");
            let floor = appearance::ui_size(STASH_CONTENT_FLOOR);
            // Layout snaps the floor to whole pixels.
            assert!(
                comparison.size.height < floor - px(1.),
                "{at}: the empty comparison {comparison:?} keeps a file's floor"
            );
            let browser = bounds(cx, "stash-browser");
            let detail = bounds(cx, "stash-detail");
            let viewport = cx.update(|window, _| window.viewport_size());
            let whole = |actions: Bounds<Pixels>| {
                actions.top() >= detail.top() - slack
                    && actions.bottom() <= browser.bottom().min(viewport.height) + slack
            };
            let actions = bounds(cx, "stash-actions");
            assert_eq!(whole(actions), pinned, "{at}: the actions {actions:?}");
            if pinned {
                // The footer, below the detail's scrolling part.
                assert!(
                    actions.top() >= detail.bottom(),
                    "{at}: {actions:?} in {detail:?}"
                );
                let file = bounds(cx, "stash-file-0");
                cx.simulate_click(file.center(), Modifiers::none());
                landed(cx, "stash-comparison");
                draw(cx);
                let comparison = bounds(cx, "stash-comparison");
                assert!(
                    comparison.size.height >= floor - slack,
                    "{at}: the chosen file's comparison {comparison:?}"
                );
            }
            // Scrolled to its end, the detail moves its actions only where
            // they are not its footer.
            let before = bounds(cx, "stash-actions");
            let delta = ScrollDelta::Pixels(point(px(0.), px(-10_000.)));
            let middle = bounds(cx, "stash-detail").center();
            scroll_down(cx, middle, delta);
            draw(cx);
            let after = bounds(cx, "stash-actions");
            assert_eq!(after == before, pinned, "{at}: {before:?}, then {after:?}");
            if pinned {
                assert!(whole(after), "{at}: the scrolled actions {after:?}");
            }
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Stacked in a 461 × 490 window, the Stashes dialog's saved-work list
    /// shows its first stash whole at 13 and 18 pt: its unscaled height left
    /// the list 35.6 px at 18 pt, short of an 80 px row. Every line the
    /// dialog draws lies whole inside its clip, and inside its pane, the
    /// list's or the detail's, where the browser holds it, and the detail's
    /// prompt is one whole line: at 18 pt the dialog's scrolling body cut
    /// "Choose a stash… / unstaged and untracked files." mid-line, as the
    /// browser asked for more height than the dialog's cap left it, and at
    /// 13 pt the prompt left "files." alone on its last line.
    #[gpui::test]
    async fn a_narrow_stash_list_keeps_its_first_row_whole(cx: &mut gpui_kit::TestAppContext) {
        use crate::empty_state::tests::{marked_runs, text_mark};
        use crate::views::tests::{draw, fixture_git, history_script, styled_history_window};
        let (_fixture, app, cx) =
            styled_history_window(cx, history_script(1, 1).as_bytes(), |root| {
                root.text_bg(text_mark())
            })
            .await;
        let root = app.read_with(cx, |app, _| app.path.clone().unwrap());
        fixture_git(&root, &["config", "user.name", "Stash Fixture"]);
        fixture_git(&root, &["config", "user.email", "stash@example.invalid"]);
        std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
        fixture_git(&root, &["stash", "push", "-m", "kept for later"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if cx.debug_bounds("saved-stash-row-0").is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for interface in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            let at = format!("{interface} pt in 461 × 490");
            let list = cx.debug_bounds("stash-list").expect("the saved-work list");
            let row = cx
                .debug_bounds("saved-stash-row-0")
                .expect("the saved stash's row");
            let slack = px(0.5);
            let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
                inner.left() >= outer.left() - slack
                    && inner.right() <= outer.right() + slack
                    && inner.top() >= outer.top() - slack
                    && inner.bottom() <= outer.bottom() + slack
            };
            assert!(
                row.top() >= list.top() - slack && row.bottom() <= list.bottom() + slack,
                "{at}: the row {row:?} is cut by the list {list:?}"
            );
            let [browser, panel, detail, prompt] = [
                "stash-browser",
                "stash-list-panel",
                "stash-detail",
                "stash-detail-placeholder",
            ]
            .map(|name| {
                cx.debug_bounds(name)
                    .unwrap_or_else(|| panic!("{at}: no {name}"))
            });
            let viewport = cx.update(|window, _| window.viewport_size());
            let runs = marked_runs(cx, Bounds::new(Point::default(), viewport));
            for run in &runs {
                assert!(
                    !run.cut,
                    "{at}: the line {:?} crosses its clip {:?}",
                    run.drawn, run.clip
                );
                assert!(
                    !browser.contains(&run.drawn.center())
                        || inside(run.drawn, panel)
                        || inside(run.drawn, detail),
                    "{at}: the line {:?} leaves its pane, the list {panel:?} or the detail {detail:?}",
                    run.drawn
                );
            }
            // The browser ends above the dialog's last line, Close in its
            // footer, rather than past the fold of a body that scrolls it.
            let footer = runs
                .iter()
                .map(|run| run.drawn)
                .max_by(|a, b| a.top().partial_cmp(&b.top()).unwrap())
                .unwrap();
            assert!(
                browser.bottom() <= footer.top(),
                "{at}: the browser {browser:?} ends past the footer's line {footer:?}"
            );
            let lines = runs
                .iter()
                .filter(|run| prompt.contains(&run.drawn.center()))
                .count();
            assert!(
                lines == 1 && inside(prompt, detail),
                "{at}: the prompt {prompt:?} is {lines} drawn lines in the detail {detail:?}"
            );
        }
    }

    /// A stashed document's comparison, at the floor the Stashes dialog keeps
    /// it (a scaled 120 px), says why a click on Open in default app opened
    /// nothing on one whole line beneath its header's controls, its whole
    /// answer in the line's tooltip, and the page beneath gives way below its
    /// minimum for it: at 18 pt in a 1000 × 680 window, the detail scrolled
    /// to the comparison, the room beneath the header held no line above that
    /// minimum, so the line drew none and the click changed nothing on screen.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_stashed_document_says_why_on_one_whole_line(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, scroll_down, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        std::fs::write(&document, pdf("first")).unwrap();
        git(repo.path(), &["add", "document.pdf"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add a document"]);
        std::fs::write(&document, pdf("second")).unwrap();
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let landed = |cx: &mut VisualTestContext, selector: &'static str| {
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-0");
        let file = cx.debug_bounds("stash-file-0").unwrap();
        cx.simulate_click(file.center(), Modifiers::none());
        landed(cx, "system-preview-1");
        // The detail scrolls; at its end the comparison is in view.
        let middle = cx.debug_bounds("stash-detail").unwrap().center();
        scroll_down(cx, middle, ScrollDelta::Pixels(point(px(0.), px(-10_000.))));
        draw(cx);
        crate::system_preview::tests::a_failed_open_answers_on_one_line(
            cx,
            "the Stashes dialog at 18 pt in 1000 × 680",
            [
                "system-preview-1",
                "system-preview-1/note",
                "rich-sides/body",
            ],
            &["stash-comparison", "stash-detail"],
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A stashed image's sides offer the open control Compare's image
    /// captions offer, by the same rule, and it is one of the Stashes
    /// detail's controls, as a stashed document's is: stacked beneath the
    /// list in a 461 × 490 window at 13 and 18 pt, with focus on Before's
    /// control and the detail scrolled to its top, away from the
    /// comparison, Tab onto After's control shows it whole with its ring in
    /// the detail's view in every frame drawn after the key, the first
    /// included, its ring unclipped, and the detail holds focus. A click
    /// that opens nothing says why on the line beneath After's caption.
    /// Stashes once drew a stashed image's sides with no open control.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_stashed_image_side_offers_its_open_control_in_the_detail(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::system_preview::tests::{latest_note, png, queue, seen_as_svg};
        use crate::tags::tests::{content_mask, draw, holds, rendered};
        let fixture = tempfile::tempdir().unwrap();
        let picture = ("picture.png", png(Some(b"first")), png(Some(b"second")));
        let (browser, cx) = stashed(
            cx,
            fixture.path(),
            &[picture],
            (461., 490., 13),
            (0, "image-system-preview-1"),
        );
        let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
        let after = "image-system-preview-1";
        let slack = px(0.01);
        for size in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(size, 12, window, cx));
            draw(cx);
            let room = cx.read(appearance::button_ring_room);
            let whole = |control: Bounds<Pixels>, view: Bounds<Pixels>| {
                let ring = control.dilate(room);
                ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
            };
            cx.update(|window, cx| {
                detail
                    .focus(("image-system-preview", 0_usize), cx)
                    .focus(window, cx)
            });
            draw(cx);
            cx.update(|window, _| {
                detail.scroll().set_offset(point(px(0.), px(0.)));
                window.refresh();
            });
            draw(cx);
            assert!(
                !whole(rendered(cx, after), rendered(cx, "stash-detail")),
                "{size} pt: at its top the detail does not show After's open control whole"
            );
            detail.take_painted();
            cx.simulate_keystrokes("tab");
            draw(cx);
            assert_eq!(
                cx.update(|window, _| detail.focused(window)),
                Some(("image-system-preview", 1_usize).into()),
                "{size} pt: Tab moves from Before's open control onto After's"
            );
            assert!(
                cx.update(|window, cx| detail.holds_focus(window, cx)),
                "{size} pt: the detail holds focus"
            );
            // The key's change of input modality draws a frame before focus
            // moves, with Before's control focused in the comparison's left
            // half; the frames after it hold After's.
            let middle = rendered(cx, "stash-comparison").center().x;
            let frames: Vec<_> = detail
                .take_painted()
                .into_iter()
                .skip_while(|frame| frame.control.is_some_and(|control| control.left() < middle))
                .collect();
            assert!(!frames.is_empty(), "{size} pt: Tab paints the detail");
            for (index, frame) in frames.iter().enumerate() {
                let control = frame
                    .control
                    .unwrap_or_else(|| panic!("{size} pt: frame {index} draws After's control"));
                assert!(
                    whole(control, frame.viewport),
                    "{size} pt: frame {index} of {} paints After's control {control:?} with its \
                     ring outside the detail {:?}",
                    frames.len(),
                    frame.viewport
                );
            }
            let (ring, mask) = (rendered(cx, after).dilate(room), content_mask(cx, after));
            assert!(
                holds(mask, ring),
                "{size} pt: After's ring {ring:?} is clipped to {mask:?}"
            );
        }
        // A click that opens nothing says why beneath After's caption.
        queue(Err(seen_as_svg("a PNG image")));
        let center = rendered(cx, after).center();
        cx.simulate_click(center, Modifiers::none());
        cx.run_until_parked();
        draw(cx);
        let (answer, failed) = cx.read(latest_note).expect("a line says why");
        assert!(
            failed && answer.starts_with("Could not open After."),
            "the answer {answer:?}"
        );
        let line = rendered(cx, "image-system-preview-1/note");
        let (control, comparison) = (rendered(cx, after), rendered(cx, "stash-comparison"));
        assert!(
            line.top() >= control.bottom() - slack
                && line.left() >= comparison.center().x - slack
                && line.right() <= comparison.right() + slack,
            "the line {line:?} lies beneath After's control {control:?}, in After's half of \
             {comparison:?}"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A stashed document's comparison takes its open controls' form from
    /// its box's width in the frame it is drawn ([`crate::in_frame`]): every
    /// frame drawn as the window narrows and widens between 461 and 1,481 px
    /// at 13 and 18 pt lays the comparison out as the frame it settles in,
    /// and the walk meets After's control labelled and as its icon. Built
    /// from the box's width a frame before, the first frame after a resize
    /// drew the form of the width before.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_stashed_comparison_takes_its_form_in_the_frame_it_is_drawn(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        std::fs::write(&document, pdf("first")).unwrap();
        git(repo.path(), &["add", "document.pdf"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add a document"]);
        std::fs::write(&document, pdf("second")).unwrap();
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1481.), px(980.)));
        draw(cx);
        let landed = |cx: &mut VisualTestContext, selector: &'static str| {
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-0");
        let file = cx.debug_bounds("stash-file-0").unwrap();
        cx.simulate_click(file.center(), Modifiers::none());
        landed(cx, "system-preview-1");
        let met = crate::system_preview::tests::open_controls_take_their_form_in_the_frame(
            cx,
            "the Stashes dialog",
            "stash-comparison",
        );
        assert_eq!(
            met.into_iter().collect::<Vec<_>>(),
            ["icon", "labelled"],
            "the walk meets both forms"
        );
    }

    /// The control `open` of a stashed side, its open control or another of
    /// its header's, lies in the side's own header, `list` drawn as
    /// `region`, and in the Stashes detail around it, `detail`
    /// (`system_preview::SideHeader::within`). With the
    /// detail scrolled to its top, away from the comparison, focus moves
    /// onto the control: through the header's list with the region showing
    /// the control, or, where `next` names the control after it, by
    /// Shift+Tab from that control with the region scrolled to its end,
    /// away from the open control, and the detail away from the region, so
    /// both containers scroll. The detail then holds focus, and in every
    /// frame painted after focus moves, the first included, the control lies
    /// whole with its ring in the detail's view and in the region's.
    #[cfg(target_os = "linux")]
    fn assert_the_detail_reveals_an_open_control(
        cx: &mut VisualTestContext,
        at: &str,
        (detail, list): (
            &crate::focus_reveal::FocusReveal,
            &crate::focus_reveal::FocusReveal,
        ),
        region: &'static str,
        (id, open): &crate::focus_reveal::tests::Control,
        next: Option<&crate::focus_reveal::tests::Control>,
    ) {
        use crate::tags::tests::{draw, rendered};
        let room = cx.read(appearance::button_ring_room);
        let slack = px(0.01);
        let within = |ring: Bounds<Pixels>, view: Bounds<Pixels>| {
            ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
        };
        let whole = |cx: &mut VisualTestContext, container: &'static str| {
            within(rendered(cx, open).dilate(room), rendered(cx, container))
        };
        list.reveal_next_focus();
        let first = next.map_or(id, |(next, _)| next);
        cx.update(|window, cx| list.focus(first.clone(), cx).focus(window, cx));
        draw(cx);
        cx.update(|window, cx| {
            if next.is_none() {
                window.blur(cx);
            }
            detail.scroll().set_offset(point(px(0.), px(0.)));
            window.refresh();
        });
        draw(cx);
        if next.is_some() {
            // The region scrolls to its end, away from the control. The
            // detail's scroll above moved the region whole and revealed
            // nothing in it, so the order of the two scrolls does not
            // matter (`focus_reveal`'s `Observation`).
            cx.update(|window, _| {
                let end = list.scroll().max_offset().y;
                list.scroll().set_offset(point(px(0.), -end));
                window.refresh();
            });
            draw(cx);
            // The region's scroll moves the control, which it clips, so the
            // detail is away from the region itself.
            let away = !within(rendered(cx, region), rendered(cx, "stash-detail"));
            assert!(
                !whole(cx, region) && away,
                "{at}: {region} does not show {open} whole, and at its top the detail does not \
                 show {region} whole"
            );
        } else {
            assert!(
                !whole(cx, "stash-detail") && whole(cx, region),
                "{at}: at its top, the detail does not show {open} whole, and {region} does"
            );
        }
        detail.take_painted();
        list.take_painted();
        if next.is_some() {
            cx.simulate_keystrokes("shift-tab");
        } else {
            detail.reveal_next_focus();
            list.reveal_next_focus();
            cx.update(|window, cx| list.focus(id.clone(), cx).focus(window, cx));
        }
        draw(cx);
        assert_eq!(
            cx.update(|window, _| list.focused(window)).as_ref(),
            Some(id),
            "{at}: focus is on {open}"
        );
        assert!(
            cx.update(|window, cx| detail.holds_focus(window, cx)),
            "{at}: the detail holds focus on {open}"
        );
        for (container, name) in [(detail, "stash-detail"), (list, region)] {
            let frames = container.take_painted();
            assert!(!frames.is_empty(), "{at}: {name} paints");
            for (index, frame) in frames.iter().enumerate() {
                let ring = frame
                    .control
                    .unwrap_or_else(|| panic!("{at}: frame {index} of {name} draws {open}"))
                    .dilate(room);
                assert!(
                    within(ring, frame.viewport),
                    "{at}: frame {index} of {} paints {open} with its ring, {ring:?}, \
                     outside {name}, {:?}",
                    frames.len(),
                    frame.viewport
                );
            }
            assert!(whole(cx, name), "{at}: {name} shows {open} whole");
        }
    }

    /// Stashes, open on a stash of `files`, each committed with its first
    /// bytes and stashed with its second, in a `width` × `height` window at
    /// `text` pt, with the stash chosen and its file `chosen`, in path
    /// order, once `drawn` is drawn.
    #[cfg(target_os = "linux")]
    fn stashed<'a>(
        cx: &'a mut gpui_kit::TestAppContext,
        fixture: &std::path::Path,
        files: &[(&str, Vec<u8>, Vec<u8>)],
        (width, height, text): (f32, f32, u8),
        (chosen, drawn): (usize, &'static str),
    ) -> (Entity<StashBrowser>, &'a mut VisualTestContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let repo = tagged_repository(fixture);
        for (name, first, _) in files {
            std::fs::write(repo.path().join(name), first).unwrap();
            git(repo.path(), &["add", name]);
        }
        git(repo.path(), &["commit", "--quiet", "-m", "Add them"]);
        for (name, _, second) in files {
            std::fs::write(repo.path().join(name), second).unwrap();
        }
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        cx.simulate_resize(size(px(width), px(height)));
        draw(cx);
        let browser = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
            .expect("Stashes opens");
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-0");
        cx.update(|window, cx| {
            browser.update(cx, |browser, cx| browser.select_file(chosen, window, cx))
        });
        landed(cx, drawn);
        (browser, cx)
    }

    /// Draws until `selector` is drawn, as the worker's reads land.
    fn landed(cx: &mut VisualTestContext, selector: &'static str) {
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            crate::tags::tests::draw(cx);
            if cx.debug_bounds(selector).is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("{selector} never drawn");
    }

    /// A stashed 3D model's header controls, the camera's, the views' and
    /// the link, are the Stashes detail's controls too, as its open control
    /// is: stacked beneath the list at 18 pt in a 461 × 490 window, with
    /// the detail scrolled to its top, away from the comparison, focus moved
    /// onto each shows it whole with its ring in the detail and in the
    /// header's region from the first frame, the detail holding focus
    /// ([`assert_the_detail_reveals_an_open_control`]). Only the open
    /// control was the detail's, so focus on the others left the detail
    /// where it was, the control below its fold.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_stashed_models_controls_are_the_details(cx: &mut gpui_kit::TestAppContext) {
        use crate::focus_reveal::tests::control;
        let fixture = tempfile::tempdir().unwrap();
        let model = |scale: &str| format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n");
        let shape = (
            "shape.obj",
            model("2").into_bytes(),
            model("1").into_bytes(),
        );
        let (browser, cx) = stashed(
            cx,
            fixture.path(),
            &[shape],
            (461., 490., 18),
            (0, "model-side-1/header"),
        );
        let view = browser.entity_id();
        let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
        let list = cx
            .update(|_, cx| crate::system_preview::side_header(view, "model-side-1", cx))
            .expect("After's header");
        for camera in [
            control("model-camera", 5),
            control("model-camera", 9),
            control("model-views", 1),
            control("model-link", 1),
        ] {
            assert_the_detail_reveals_an_open_control(
                cx,
                &format!("{} at 18 pt in 461 × 490", camera.1),
                (&detail, &list),
                "model-side-1/header",
                &camera,
                None,
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A stashed PDF's page arrows and page number are the Stashes detail's
    /// controls too, as its open control is: stacked beneath the list at
    /// 18 pt in a 461 × 490 window, with the detail scrolled to its top,
    /// away from the comparison, focus moved onto each of either side's
    /// shows it whole with its ring in the detail and in the header's region
    /// from the first frame, the detail holding focus
    /// ([`assert_the_detail_reveals_an_open_control`]). Only the open
    /// control was the detail's, so focus on the others left the detail
    /// where it was, the control below its fold.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_stashed_pdfs_page_controls_are_the_details(cx: &mut gpui_kit::TestAppContext) {
        use crate::focus_reveal::tests::control;
        use crate::tags::tests::draw;
        let fixture = tempfile::tempdir().unwrap();
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
                .into_bytes()
        };
        let document = ("document.pdf", pdf("first"), pdf("second"));
        let (browser, cx) = stashed(
            cx,
            fixture.path(),
            &[document],
            (461., 490., 18),
            (0, "system-preview-1"),
        );
        // The second of three pages, so both arrows step.
        browser.update(cx, |browser, cx| {
            let Some(Content::Rich(preview)) = browser.content.as_deref() else {
                panic!("a changed PDF is a document comparison");
            };
            browser.content = Some(Arc::new(crate::pdf_view::tests::paged_at(preview, 3, 1)));
            cx.notify();
        });
        draw(cx);
        crate::pdf_view::tests::drain_pages();
        draw(cx);
        let view = browser.entity_id();
        let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
        for (side, name) in [(0, "pdf-side-0"), (1, "pdf-side-1")] {
            let list = cx
                .update(|_, cx| crate::system_preview::side_header(view, name, cx))
                .expect("the side's header");
            let region: &'static str = format!("{name}/header").leak();
            for page in ["pdf-back", "pdf-forward", "pdf-page-number"] {
                let page = control(page, side);
                assert_the_detail_reveals_an_open_control(
                    cx,
                    &format!("{} at 18 pt in 461 × 490", page.1),
                    (&detail, &list),
                    region,
                    &page,
                    None,
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A side header's region keeps where the person scrolled it when the
    /// Stashes detail around it scrolls after a key input: stacked beneath
    /// the list at 18 pt in a 461 × 490 window, with Tab on one of a
    /// stashed model's header controls and the region scrolled away from
    /// it, scrolling the detail, which moves the region but changes neither
    /// focus nor the region's size, leaves the region where it was: whether
    /// the region shows the control cut or whole with its ring's room cut,
    /// which a frame counts as shown and a resize would reveal. Judged by
    /// where the region lay in the window, which the detail's scroll moves,
    /// the region scrolled back to its focused control, since GPUI keeps
    /// the keyboard as the last input through a trackpad's scroll.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_side_header_keeps_its_scroll_as_the_detail_scrolls(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::focus_reveal::tests::control;
        use crate::tags::tests::{draw, rendered};
        let fixture = tempfile::tempdir().unwrap();
        let model = |scale: &str| format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n");
        let shape = (
            "shape.obj",
            model("2").into_bytes(),
            model("1").into_bytes(),
        );
        let (browser, cx) = stashed(
            cx,
            fixture.path(),
            &[shape],
            (461., 490., 18),
            (0, "model-side-1/header"),
        );
        let view = browser.entity_id();
        let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
        let list = cx
            .update(|_, cx| crate::system_preview::side_header(view, "model-side-1", cx))
            .expect("After's header");
        let region = "model-side-1/header";
        let room = cx.read(appearance::button_ring_room);
        let (before, (id, camera)) = (control("model-camera", 5), control("model-camera", 6));
        // Tab onto the camera's second control, from its first.
        list.reveal_next_focus();
        cx.update(|window, cx| list.focus(before.0.clone(), cx).focus(window, cx));
        draw(cx);
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_eq!(
            cx.update(|window, _| list.focused(window)),
            Some(id),
            "Tab moves onto {camera}"
        );
        let slack = px(0.01);
        let whole = |cx: &mut VisualTestContext| {
            let (ring, view) = (rendered(cx, camera).dilate(room), rendered(cx, region));
            ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
        };
        assert!(whole(cx), "Tab reveals {camera} in {region}");
        // The person scrolls the region away from the focused control.
        let end = list.scroll().max_offset().y;
        assert!(end > px(0.), "{region} scrolls at 18 pt in 461 × 490");
        for offset in [px(0.), -end] {
            cx.update(|window, _| {
                list.scroll().set_offset(point(px(0.), offset));
                window.refresh();
            });
            draw(cx);
            if !whole(cx) {
                break;
            }
        }
        assert!(!whole(cx), "{region} is scrolled away from {camera}");
        let away = list.scroll().offset();
        // The detail scrolls without a key or the pointer moving.
        let scroll_detail = |cx: &mut VisualTestContext| {
            let at = rendered(cx, region).origin;
            let shown = detail.scroll().offset().y;
            let reach = detail.scroll().max_offset().y;
            let step = if shown < -reach / 2. {
                px(20.)
            } else {
                px(-20.)
            };
            cx.update(|window, _| {
                detail.scroll().set_offset(point(px(0.), shown + step));
                window.refresh();
            });
            draw(cx);
            assert_ne!(
                rendered(cx, region).origin,
                at,
                "the detail's scroll moves {region}"
            );
            assert!(
                cx.update(|window, _| window.last_input_was_keyboard()),
                "the last input is still Tab"
            );
        };
        scroll_detail(cx);
        assert_eq!(
            list.scroll().offset(),
            away,
            "{region} stays where it was scrolled"
        );

        // Scrolled by less than its ring's room, the control still lies
        // whole in the region, so the frame counts it as shown, and only
        // the region's unchanged size keeps the detail's scroll from
        // revealing it: judged by where the region lay, that scroll moved
        // the region back by the rest of the ring's room.
        let (control, view) = (rendered(cx, camera), rendered(cx, region));
        let offset = list.scroll().offset().y;
        let near = [
            offset + (view.top() + room / 2. - control.top()),
            offset + (view.bottom() - room / 2. - control.bottom()),
        ]
        .into_iter()
        .find(|near| *near <= px(0.) && *near >= -end)
        .unwrap_or_else(|| panic!("{region} scrolls {camera} within its ring's room of an edge"));
        cx.update(|window, _| {
            list.scroll().set_offset(point(px(0.), near));
            window.refresh();
        });
        draw(cx);
        let (control, view) = (rendered(cx, camera), rendered(cx, region));
        assert!(
            control.top() + slack >= view.top()
                && control.bottom() <= view.bottom() + slack
                && !whole(cx),
            "{camera} {control:?} lies whole in {region} {view:?}, its ring's room cut"
        );
        let near = list.scroll().offset();
        scroll_detail(cx);
        assert_eq!(
            list.scroll().offset(),
            near,
            "{region} stays where it was scrolled, {camera} whole in it"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A window shrunk until the Stashes detail cuts the control the
    /// keyboard left focus on shows it whole again in the frames after the
    /// resize: stacked beneath the list at 18 pt, with Tab on Review
    /// restore… at the detail's foot in a 461 × 640 window and the pointer
    /// since moved, as it is to drag the window's edge, the window shrunk
    /// to 461 × 490 shows it whole with its ring. A control the person had
    /// scrolled out of view is left there: only a cut the resize makes is
    /// revealed. Judged only on a key being the last input, a resize after
    /// the pointer moved left the focused control cut.
    #[gpui::test]
    async fn a_resize_that_cuts_the_focused_control_reveals_it(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "first\n").unwrap();
        git(repo.path(), &["add", "notes.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
        std::fs::write(repo.path().join("notes.txt"), "second\n").unwrap();
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(640.)));
        draw(cx);
        let browser = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
            .expect("Stashes opens");
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-0");
        let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
        let room = cx.read(appearance::button_ring_room);
        let slack = px(0.01);
        let whole = |cx: &mut VisualTestContext| {
            let ring = rendered(cx, "restore-saved-stash").dilate(room);
            let view = rendered(cx, "stash-detail");
            ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
        };
        let pointer_moves = |cx: &mut VisualTestContext| {
            let list = rendered(cx, "stash-list").center();
            cx.simulate_mouse_move(list, None, Modifiers::default());
            draw(cx);
        };
        tab_to(&browser, "restore-saved-stash", cx);
        draw(cx);
        assert!(whole(cx), "Tab reveals Review restore…");

        // Scrolled out of view by the person, it stays out of view.
        pointer_moves(cx);
        cx.update(|window, _| {
            detail.scroll().set_offset(point(px(0.), px(0.)));
            window.refresh();
        });
        draw(cx);
        assert!(
            !whole(cx),
            "at its top the detail does not show Review restore… whole"
        );
        cx.simulate_resize(size(px(461.), px(600.)));
        draw(cx);
        assert_eq!(
            detail.scroll().offset().y,
            px(0.),
            "a resize leaves a control the person scrolled away from where it is"
        );

        // Shown whole, it is shown whole again once a resize cuts it.
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_eq!(
            cx.update(|window, _| detail.focused(window)),
            Some("restore-saved-stash".into()),
            "Tab returns to Review restore…"
        );
        assert!(whole(cx), "Tab reveals Review restore… again");
        pointer_moves(cx);
        let foot = rendered(cx, "restore-saved-stash").bottom();
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        assert!(
            rendered(cx, "stash-detail").bottom() < foot,
            "the smaller window's detail ends above where Review restore… lay"
        );
        assert_eq!(
            cx.update(|window, _| detail.focused(window)),
            Some("restore-saved-stash".into()),
            "focus stays on Review restore…"
        );
        assert!(
            whole(cx),
            "the resize reveals Review restore… whole with its ring"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A window whose edge is dragged keeps the focused control whole
    /// through every step of the drag, not only its first: stacked beneath
    /// the list at 18 pt, with Tab on Review restore… at the foot of the
    /// Stashes detail and the pointer since moved, the window shrinks from
    /// 461 × 640 to 461 × 490 in 15 px steps, each step's next resize laid
    /// out before the frame that shows that step's reveal is drawn. Judged
    /// at the offset its frame was painted at, before the reveal it
    /// requested had applied, the step counted Review restore… as cut, so
    /// the next resize revealed nothing and left it cut for the rest of the
    /// drag.
    ///
    /// GPUI's test build draws a dirty window as soon as its effects are
    /// flushed, so a resize's reveal is always drawn before the next resize
    /// arrives. Each step is sequenced by hand instead: one update resizes
    /// the window and draws that frame, which requests its reveal, and
    /// queues the next resize behind the reveal, so the next frame is laid
    /// out at the smaller size with the reveal applied but never drawn at
    /// the size it was made for. The test shows that the reveal holds
    /// across such frames; it cannot show how a platform paces a real
    /// drag's resizes against its frames, which a native drag settles.
    #[gpui::test]
    async fn a_dragged_window_keeps_the_focused_control_whole_at_every_step(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::focus_reveal::tests::Painted;
        use crate::tags::tests::{draw, git, rendered, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "first\n").unwrap();
        git(repo.path(), &["add", "notes.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
        std::fs::write(repo.path().join("notes.txt"), "second\n").unwrap();
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(640.)));
        draw(cx);
        let browser = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
            .expect("Stashes opens");
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-0");
        let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
        let room = cx.read(appearance::button_ring_room);
        let slack = px(0.01);
        let whole = move |control: Bounds<Pixels>, view: Bounds<Pixels>| {
            let ring = control.dilate(room);
            ring.top() + slack >= view.top() && ring.bottom() <= view.bottom() + slack
        };
        let cut = |frame: &Painted| {
            !frame
                .control
                .is_some_and(|control| whole(control, frame.viewport))
        };
        tab_to(&browser, "restore-saved-stash", cx);
        draw(cx);
        assert!(
            whole(
                rendered(cx, "restore-saved-stash"),
                rendered(cx, "stash-detail")
            ),
            "Tab reveals Review restore…"
        );
        // The pointer moves, as it does to drag the window's edge.
        let list = rendered(cx, "stash-list").center();
        cx.simulate_mouse_move(list, None, Modifiers::default());
        draw(cx);
        let mut height: f32 = 640.;
        let mut held = 0;
        while height > 490. {
            let (step, next) = (height - 15., (height - 30.).max(490.));
            detail.take_painted();
            cx.update(|window, cx| {
                window.resize(size(px(461.), px(step)));
                window.bounds_changed(cx);
                window.draw(cx).clear(cx);
                window.defer(cx, move |window, cx| {
                    window.resize(size(px(461.), px(next)));
                    window.bounds_changed(cx);
                });
            });
            draw(cx);
            let frames = detail.take_painted();
            // The step's frame cut Review restore…, and the next resize cut
            // it again before the step's reveal was drawn.
            if let [stepped, resized, ..] = frames.as_slice()
                && cut(stepped)
                && cut(resized)
                && resized.viewport.size.height < stepped.viewport.size.height
            {
                held += 1;
            }
            assert_eq!(
                cx.update(|window, _| detail.focused(window)),
                Some("restore-saved-stash".into()),
                "at 461 × {next} focus stays on Review restore…"
            );
            assert!(
                whole(
                    rendered(cx, "restore-saved-stash"),
                    rendered(cx, "stash-detail")
                ),
                "dragged to 461 × {next}, the window shows Review restore… whole with its \
                 ring: {frames:?}"
            );
            height = next;
        }
        assert!(
            held > 0,
            "a step's next resize cut Review restore… before the step's reveal was drawn"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The Stashes detail's fades never lie over the ring of the control
    /// focus is on, as every cue whose controls a reveal scrolls into view
    /// (`tags::ScrollCue::revealing`): stacked beneath the list at 13 and
    /// 18 pt, in a 461 px wide window at the height where the detail is too
    /// short for a thumb that moves (461 × 490 at 18 pt), so it fades the
    /// edges its content continues past, Tab through the detail's controls
    /// reveals each, some at an edge where a whole fade would lie, and no
    /// frame paints a fade band over the focused control's ring. The
    /// detail's cue once took no notice of focus, and its fade lay over the
    /// ring of a control the keyboard had just revealed at its edge.
    #[gpui::test]
    async fn the_stashes_detail_fades_stop_short_of_the_focused_ring(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, painted_fades, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "first\n").unwrap();
        git(repo.path(), &["add", "notes.txt"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add notes"]);
        std::fs::write(repo.path().join("notes.txt"), "second\n").unwrap();
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let browser = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
            .expect("Stashes opens");
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-0");
        let file = cx.debug_bounds("stash-file-0").unwrap();
        cx.simulate_click(file.center(), Modifiers::none());
        landed(cx, "stash-comparison");
        let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
        let slack = px(0.01);
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            let room = cx.read(appearance::button_ring_room);
            // The tallest window, from 461 × 490 down, whose detail fades.
            let floor = (0..20)
                .map(|step| 490. - 10. * step as f32)
                .find(|height| {
                    cx.simulate_resize(size(px(461.), px(*height)));
                    draw(cx);
                    let list = detail.scroll().bounds();
                    list.size.height - room * 2. < crate::tags::SHORTEST_SCROLL_TRACK
                        && detail.scroll().max_offset().y > px(0.)
                })
                .unwrap_or_else(|| panic!("{text} pt: the detail fades in a window 461 px wide"));
            let at = format!("{text} pt in 461 × {floor}");
            let fade = cx
                .update(|_, _| appearance::ui_size(8.))
                .min(detail.scroll().bounds().size.height / 4.);
            cx.update(|window, cx| {
                let list = browser.read(cx).stash_focus.clone();
                window.focus(&list, cx);
            });
            draw(cx);
            let mut at_edge = 0;
            for step in 0..16 {
                cx.simulate_keystrokes("tab");
                draw(cx);
                let Some(control) = cx.update(|window, cx| detail.focused_bounds(window, cx))
                else {
                    continue;
                };
                let ring = control.dilate(room);
                let list = detail.scroll().bounds();
                let scrolled = -detail.scroll().offset().y;
                let reach = detail.scroll().max_offset().y;
                if (scrolled > px(0.5) && ring.top() < list.top() + fade)
                    || (scrolled < reach - px(0.5) && ring.bottom() > list.bottom() - fade)
                {
                    at_edge += 1;
                }
                let (top, bottom) = painted_fades(cx, list);
                assert!(
                    top.is_none_or(|top| {
                        list.top() + top <= ring.top() + slack || ring.bottom() <= list.top()
                    }) && bottom.is_none_or(|bottom| {
                        list.bottom() - bottom >= ring.bottom() - slack
                            || ring.top() >= list.bottom()
                    }),
                    "{at}, Tab {step}: the fades {top:?} and {bottom:?} of the detail {list:?} \
                     lie over the focused control's ring {ring:?}"
                );
            }
            assert!(
                at_edge > 0,
                "{at}: Tab reveals a control within a fade's height of an edge"
            );
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The saved files cue their overflow as the detail around them does
    /// (`tags::ScrollCue`): at 13 and 18 pt in a 1000 × 680 window, each
    /// size applied as Settings applies it (`assert_scroll_cue`,
    /// `assert_no_scroll_cue` and the gutter's check below each call
    /// `tags::tests::set_interface_text_size` for both), eleven
    /// unstaged files overflow their box, which shows a scrollbar in a
    /// gutter at the rows' right, its thumb following their scroll and
    /// covering no row, and the one staged file fits, drawing none, its row
    /// keeping the box's whole width. Eleven files once had only a row cut
    /// at the box's foot to say more lay below. The files' scroll reaches
    /// exactly into the cue's gutter (`ScrollCue::gutter`), so the track
    /// runs along its right edge and each row ends where the gutter begins:
    /// the list once copied the gutter's sum, which could drift from the
    /// cue's.
    #[gpui::test]
    async fn the_saved_files_cue_their_overflow(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{
            assert_no_scroll_cue, assert_scroll_cue, draw, git, rendered, set_interface_text_size,
            tagged_repository, window,
        };
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let names: Vec<_> = (0..12)
            .map(|index| format!("file-{index:02}.txt"))
            .collect();
        for name in &names {
            std::fs::write(repo.path().join(name), "first\n").unwrap();
            git(repo.path(), &["add", name]);
        }
        git(repo.path(), &["commit", "--quiet", "-m", "Add them"]);
        for name in &names {
            std::fs::write(repo.path().join(name), "second\n").unwrap();
        }
        git(repo.path(), &["add", &names[0]]);
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let browser = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
            .expect("Stashes opens");
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-0");
        let scroll = browser.read_with(cx, |browser, _| {
            browser.file_scroll.0.borrow().base_handle.clone()
        });
        browser.update(cx, |browser, cx| {
            browser.choose_area(StashArea::Unstaged, cx)
        });
        draw(cx);
        assert_eq!(
            browser.read_with(cx, |browser, _| browser.files().len()),
            11,
            "eleven unstaged files"
        );
        assert_scroll_cue(
            cx,
            &app,
            &scroll,
            "stash-file-rows",
            px(0.),
            &["stash-file-0", "stash-file-1", "stash-file-2"],
        );
        let gutter = crate::tags::ScrollCue::gutter();
        for size in [13, 18] {
            set_interface_text_size(cx, &app, size);
            assert!(
                scroll.max_offset().y > px(0.),
                "at {size} pt the eleven files overflow their box"
            );
            let (list, rows) = (scroll.bounds(), rendered(cx, "stash-file-rows"));
            let row = rendered(cx, "stash-file-0");
            assert!(
                (list.right() - rows.right()).abs() < px(0.01)
                    && (rows.right() - gutter - row.right()).abs() < px(0.01),
                "at {size} pt the files' scroll {list:?} spans the cue's {gutter:?} gutter \
                 in {rows:?}, beside the row {row:?}"
            );
        }
        set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
        browser.update(cx, |browser, cx| browser.choose_area(StashArea::Staged, cx));
        draw(cx);
        assert_no_scroll_cue(cx, &app, "stash-file-rows", px(0.), &["stash-file-0"]);
    }

    /// A stashed 3D model and PDF, at the floor the Stashes dialog keeps
    /// its comparison (a scaled 120 px), keep each side's own header at its
    /// floor, one control whole with the larger of its ring's room and the
    /// header's padding above and below, beneath which the model or page
    /// gives way, and Tab reveals each of the header's controls whole with
    /// its ring there, at 13 and 18 pt in a 1000 × 560 window with the
    /// detail scrolled to the comparison. The floor once counted the ring's
    /// room and the padding together, about 6 px more than either needed.
    /// Each side's open control is the detail's too: with the detail
    /// scrolled away from the comparison, stacked in a 461 × 490 window and,
    /// for the model, in the wide one, focus moved onto it through its
    /// header, and at 18 pt Shift+Tab onto it with the header's region
    /// scrolled away from it, shows it whole in both from the first frame,
    /// the detail holding focus
    /// ([`assert_the_detail_reveals_an_open_control`]). The detail once
    /// revealed it where the last frame drew it, before the region scrolled,
    /// so that first frame painted it below the detail's edge.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_stashed_side_header_keeps_its_floor(cx: &mut gpui_kit::TestAppContext) {
        use crate::focus_reveal::tests::{Control, control};
        use crate::system_preview::tests::assert_a_header_keeps_its_floor;
        use crate::tags::tests::{draw, git, scroll_down, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repo.path().join("document.pdf");
        let model = repo.path().join("shape.obj");
        for (text, scale) in [("first", "2"), ("second", "1")] {
            std::fs::write(&document, pdf(text)).unwrap();
            std::fs::write(
                &model,
                format!("v 0 0 0\nv {scale} 0 0\nv 0 {scale} 0\nf 1 2 3\n"),
            )
            .unwrap();
            if text == "first" {
                git(repo.path(), &["add", "document.pdf", "shape.obj"]);
                git(repo.path(), &["commit", "--quiet", "-m", "Add them"]);
            }
        }
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.update(|_, cx| appearance::ThemeChoice::Midnight.apply(None, cx));
        cx.simulate_resize(size(px(1000.), px(560.)));
        draw(cx);
        let landed = |cx: &mut VisualTestContext, selector: &'static str| {
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        let browser = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
            .expect("Stashes opens");
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-1");
        // The detail scrolls; at its end the comparison is in view.
        let to_the_comparison = |cx: &mut VisualTestContext| {
            let middle = cx.debug_bounds("stash-detail").unwrap().center();
            scroll_down(cx, middle, ScrollDelta::Pixels(point(px(0.), px(-10_000.))));
            draw(cx);
        };
        let view = browser.entity_id();
        let detail = browser.read_with(cx, |browser, _| browser.detail.clone());
        // Each side's open control with the detail scrolled away from the
        // comparison, in each of `windows` where the detail's top leaves it
        // out of view: as the window is, and stacked in a narrow one. At
        // 18 pt, where the header's region scrolls, also by Shift+Tab from
        // `next` with the region scrolled away from it.
        let in_the_detail = |cx: &mut VisualTestContext,
                             text: u8,
                             windows: &[(f32, f32)],
                             name: &'static str,
                             (open, next): (&Control, &Control)| {
            let list = cx
                .update(|_, cx| crate::system_preview::side_header(view, name, cx))
                .expect("the side's header");
            let region: &'static str = format!("{name}/header").leak();
            for &(width, height) in windows {
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                let at = format!("{} in {width} × {height} at {text} pt", open.1);
                assert_the_detail_reveals_an_open_control(
                    cx,
                    &at,
                    (&detail, &list),
                    region,
                    open,
                    None,
                );
                if text == 18 {
                    assert_the_detail_reveals_an_open_control(
                        cx,
                        &format!("Shift+Tab onto {at}"),
                        (&detail, &list),
                        region,
                        open,
                        Some(next),
                    );
                }
            }
            cx.simulate_resize(size(px(1000.), px(560.)));
            draw(cx);
        };
        let mut cameras = vec![control("model-external", 1)];
        cameras.extend((5..10).map(|index| control("model-camera", index)));
        cameras.extend([control("model-views", 1), control("model-link", 1)]);
        // The stash's files in path order: the document, then the model.
        let file = cx.debug_bounds("stash-file-1").unwrap();
        cx.simulate_click(file.center(), Modifiers::none());
        landed(cx, "model-side-1/header");
        for (text, floor) in [(13, 34.), (18, 48.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            draw(cx);
            to_the_comparison(cx);
            assert_a_header_keeps_its_floor(
                cx,
                &format!("the stashed model's After at {text} pt"),
                (view, "model-side-1"),
                px(floor),
                &control("model-source", 1),
                &cameras,
            );
            let windows = [(1000., 560.), (461., 490.)];
            let open = (&cameras[0], &cameras[1]);
            in_the_detail(cx, text, &windows, "model-side-1", open);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        draw(cx);
        let middle = cx.debug_bounds("stash-detail").unwrap().center();
        scroll_down(cx, middle, ScrollDelta::Pixels(point(px(0.), px(10_000.))));
        draw(cx);
        let file = cx.debug_bounds("stash-file-0").unwrap();
        cx.simulate_click(file.center(), Modifiers::none());
        landed(cx, "system-preview-1");
        browser.update(cx, |browser, cx| {
            let Some(Content::Rich(preview)) = browser.content.as_deref() else {
                panic!("a changed PDF is a document comparison");
            };
            browser.content = Some(Arc::new(crate::pdf_view::tests::paged(preview)));
            cx.notify();
        });
        draw(cx);
        crate::pdf_view::tests::drain_pages();
        draw(cx);
        for (text, floor) in [(13, 41.), (18, 57.)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            draw(cx);
            to_the_comparison(cx);
            for (side, page) in [(0, "pdf-read-text"), (1, "pdf-retry")] {
                let mut controls = [page, "pdf-forward", "pdf-page-number", "pdf-link"]
                    .map(|name| control(name, side))
                    .to_vec();
                controls.extend([1, 0, 2].map(|zoom| control("pdf-zoom", side * 3 + zoom)));
                let name = if side == 0 {
                    "pdf-side-0"
                } else {
                    "pdf-side-1"
                };
                assert_a_header_keeps_its_floor(
                    cx,
                    &format!("the stashed {name} at {text} pt"),
                    (view, name),
                    px(floor),
                    &control("pdf-system", side),
                    &controls,
                );
            }
            // The wide window's detail shows a PDF's open controls at its top.
            for (side, name, page) in [
                (0, "pdf-side-0", "pdf-read-text"),
                (1, "pdf-side-1", "pdf-retry"),
            ] {
                let windows = [(461., 490.)];
                let open = (&control("pdf-system", side), &control(page, side));
                in_the_detail(cx, text, &windows, name, open);
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The side headers the Stashes dialog draws for a stashed 3D model go
    /// with the dialog: their scroll and focus state, held by the view that
    /// draws them, once stayed in an application-wide table after the
    /// dialog closed, a model's two sides for every dialog opened.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_closed_stash_dialog_takes_its_side_headers(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let model = repo.path().join("shape.obj");
        std::fs::write(&model, b"v 0 0 0\nv 2 0 0\nv 0 2 0\nf 1 2 3\n").unwrap();
        git(repo.path(), &["add", "shape.obj"]);
        git(repo.path(), &["commit", "--quiet", "-m", "Add a model"]);
        std::fs::write(&model, b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        git(repo.path(), &["stash", "push", "-m", "kept for later"]);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let landed = |cx: &mut VisualTestContext, selector: &'static str| {
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        let browser = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_stash_browser(window, cx)))
            .expect("Stashes opens");
        landed(cx, "saved-stash-row-0");
        let row = cx.debug_bounds("saved-stash-row-0").unwrap();
        cx.simulate_click(row.center(), Modifiers::none());
        landed(cx, "stash-file-0");
        let file = cx.debug_bounds("stash-file-0").unwrap();
        cx.simulate_click(file.center(), Modifiers::none());
        landed(cx, "model-side-1/header");
        let view = browser.entity_id();
        let held = |cx: &mut VisualTestContext| {
            ["model-side-0", "model-side-1"]
                .into_iter()
                .filter(|name| {
                    cx.update(|_, cx| crate::system_preview::side_header(view, name, cx))
                        .is_some()
                })
                .count()
        };
        assert_eq!(held(cx), 2, "the dialog draws the model's two side headers");
        let weak = browser.downgrade();
        drop(browser);
        cx.update(|window, cx| window.close_dialog(cx));
        draw(cx);
        assert!(
            weak.upgrade().is_none(),
            "the closed dialog's view is released"
        );
        assert_eq!(held(cx), 0, "the closed dialog's side headers go with it");
    }

    /// Lets every quiet read the window has asked for land: each runs on
    /// the window's reader, a thread `run_until_parked` cannot wait for.
    fn quiet_reads_land(app: &Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) {
        for _ in 0..8 {
            if !app.read_with(cx, |app, _| app.automatic.quiet_read_requested()) {
                return;
            }
            app.read_with(cx, |app, _| app.worker.wait_idle());
            cx.executor().run_until_parked();
        }
        panic!("a quiet read asked for never ran");
    }

    /// Reads the working state, as Refresh does, and lets it land: a merge
    /// paused outside the app is the window's pause from then on.
    async fn read_pause(app: &Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(app, cx).await;
        quiet_reads_land(app, cx);
        assert!(
            app.read_with(cx, |app, _| app.integration_state.is_some()),
            "the merge paused"
        );
    }

    /// Draws a frame without running the window's tasks, so a reply
    /// waiting on the reader stays unread.
    fn frame(cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
    }

    /// The saved-work list is one Tab stop, in 461 × 490 and 1480 × 980
    /// windows at 13 and 18 pt: Tab reaches it, it draws its keyboard ring
    /// whole on its own edge, and Down moves the selection to the second of
    /// two stashes, which the list scrolls whole into its view, clear of the
    /// Previous/More pager beneath it. Its heading counts the stashes
    /// ("Saved work · 2"), and while its rows overflow it, as the stacked
    /// list's one row does in a half tile, it shows its scrollbar. The list
    /// was never a Tab stop, its focus handle not one, and the stacked list
    /// showed one stash with nothing saying a second lay below it, under the
    /// pager.
    #[gpui::test]
    async fn the_stash_list_is_one_stop_that_counts_and_cues_its_stashes(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for (name, text) in [("Index only", "first\n"), ("Layout experiment", "second\n")] {
            std::fs::write(repo.path().join("notes.txt"), text).unwrap();
            git(
                repo.path(),
                &["stash", "push", "--include-untracked", "-m", name],
            );
        }
        let (app, cx) = window(cx, &repo);
        crate::focus_ring::sweep::prepare(&app, cx);
        let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
        let landed = |cx: &mut VisualTestContext, selector: &'static str| {
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };
        let slack = px(0.5);
        for (width, height) in [(461., 490.), (1480., 980.)] {
            for interface in [13, 18] {
                let at = format!("{interface} pt in {width} × {height}");
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
                landed(cx, "saved-stash-row-0");
                let browser = app
                    .read_with(cx, |app, _| app.recovery.browser.clone())
                    .and_then(|browser| browser.upgrade())
                    .expect("Stashes is open");
                assert_eq!(
                    crate::native_accessibility::written::get("stash-list-heading")
                        .and_then(|heading| heading.node.value().map(str::to_owned)),
                    Some("Saved work · 2".to_owned()),
                    "{at}: the heading counts the stashes"
                );
                let list = cx.debug_bounds("stash-list").expect("the list");
                let pager = cx.debug_bounds("stash-pager").expect("the pager");
                let row = appearance::ui_size(58.);
                let overflows = row * 2. > list.size.height + slack;
                assert_eq!(
                    overflows,
                    width < 800.,
                    "{at}: the stacked list holds one row, the list beside the detail both"
                );
                assert_eq!(
                    cx.debug_bounds("saved-stashes-scrollbar").is_some(),
                    overflows,
                    "{at}: the scrollbar shows while the rows overflow {list:?}"
                );
                // Tab reaches the list.
                let stop = browser.read_with(cx, |browser, _| browser.stash_focus.clone());
                let mut reached = false;
                for _ in 0..12 {
                    cx.simulate_keystrokes("tab");
                    draw(cx);
                    if cx.update(|window, _| stop.is_focused(window)) {
                        reached = true;
                        break;
                    }
                }
                assert!(reached, "{at}: Tab reaches the saved-work list");
                let ring = crate::focus_ring::painted_outlines(cx, accent)
                    .into_iter()
                    .find(|outline| outline.bounds == list)
                    .unwrap_or_else(|| panic!("{at}: the list {list:?} draws no ring"));
                assert!(ring.whole, "{at}: the list's ring {ring:?} is whole");
                // Down twice selects the second stash and shows it whole.
                cx.simulate_keystrokes("down");
                draw(cx);
                cx.simulate_keystrokes("down");
                draw(cx);
                assert_eq!(
                    browser.read_with(cx, |browser, _| browser.selected),
                    Some(1),
                    "{at}: Down reaches the second stash"
                );
                let second = cx
                    .debug_bounds("saved-stash-row-1")
                    .expect("the second stash's row");
                assert!(
                    second.top() >= list.top() - slack
                        && second.bottom() <= list.bottom() + slack
                        && second.bottom() <= pager.top() + slack,
                    "{at}: the second stash {second:?} lies whole in the list {list:?}, above the pager {pager:?}"
                );
                assert!(
                    cx.update(|window, _| stop.is_focused(window)),
                    "{at}: the list keeps focus"
                );
                drop(browser);
                cx.simulate_keystrokes("escape");
                draw(cx);
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The saved files are one Tab stop, with the arrow keys inside, at 13
    /// pt in a 1480 × 980 window and at 18 pt in a 461 × 490 one: Tab
    /// reaches them, and they draw the keyboard ring whole on their own edge
    /// in the detail's view; Down moves the selection to the first and then
    /// the second of four files and End to the last, each whole in the list,
    /// which keeps focus. Stacked beneath the saved-work list at 18 pt, where
    /// the detail is about 60 px tall, they are no taller than it holds with
    /// their ring. Their focus handle was no Tab stop, so Tab passed them by,
    /// and at 85 px in that detail no reveal showed them whole: its clip cut
    /// their ring.
    #[gpui::test]
    async fn the_saved_files_are_one_stop_whole_in_the_detail(cx: &mut gpui_kit::TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        for name in ["alpha.txt", "beta.txt", "delta.txt", "gamma.txt"] {
            std::fs::write(repo.path().join(name), format!("{name}\n")).unwrap();
        }
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["stash", "push", "-m", "four files"]);
        let (app, cx) = window(cx, &repo);
        crate::focus_ring::sweep::prepare(&app, cx);
        let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - slack
                && inner.right() <= outer.right() + slack
                && inner.top() >= outer.top() - slack
                && inner.bottom() <= outer.bottom() + slack
        };
        let bounds = |cx: &mut VisualTestContext, name: &'static str| {
            cx.debug_bounds(name).unwrap_or_else(|| panic!("no {name}"))
        };
        for (width, height, interface) in [(1480., 980., 13), (461., 490., 18)] {
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            open_selected_stash(&app, cx).await;
            let stashes = app
                .read_with(cx, |app, _| app.recovery.browser.clone())
                .and_then(|browser| browser.upgrade())
                .expect("Stashes is open");
            assert_eq!(
                stashes.read_with(cx, |stashes, _| (stashes.area, stashes.files().len())),
                (StashArea::Staged, 4),
                "{at}: four saved files"
            );
            // Tab reaches the saved files, from the stash list.
            let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
            let focused =
                |cx: &mut VisualTestContext| cx.update(|window, _| files.is_focused(window));
            let mut reached = false;
            for _ in 0..12 {
                cx.simulate_keystrokes("tab");
                draw(cx);
                if focused(cx) {
                    reached = true;
                    break;
                }
            }
            assert!(reached, "{at}: Tab reaches the saved files");
            draw(cx);
            let [list, view] = [bounds(cx, "stash-files"), bounds(cx, "stash-detail")];
            assert!(
                inside(list, view),
                "{at}: the saved files {list:?} lie whole in the detail's view {view:?}"
            );
            let ring = crate::focus_ring::painted_outlines(cx, accent)
                .into_iter()
                .find(|outline| outline.bounds == list)
                .unwrap_or_else(|| panic!("{at}: the saved files {list:?} draw no ring"));
            assert!(ring.whole, "{at}: their ring {ring:?} is whole");
            // The arrow keys move the selection inside them.
            for (key, index, row) in [
                ("down", 0, "stash-file-0"),
                ("down", 1, "stash-file-1"),
                ("end", 3, "stash-file-3"),
            ] {
                cx.simulate_keystrokes(key);
                draw(cx);
                assert_eq!(
                    stashes.read_with(cx, |stashes, _| stashes.selected_file),
                    Some(index),
                    "{at}: {key} selects file {index}"
                );
                let [row, list] = [bounds(cx, row), bounds(cx, "stash-files")];
                assert!(
                    inside(row, list),
                    "{at}: {key} shows file {index} {row:?} whole in the list {list:?}"
                );
                assert!(focused(cx), "{at}: the saved files keep focus after {key}");
            }
            drop(stashes);
            cx.simulate_keystrokes("escape");
            draw(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The fixture of Stashes' Review restore… regressions: a repository
    /// with two stashes on `main`, "Index only" and the newest, "Layout
    /// experiment", and a branch `side` whose merge pauses cleanly, as the
    /// native review's `dlg` fixture has. Returns its fixture Git.
    fn two_stashes(repository: &std::path::Path) -> impl Fn(&[&str]) + '_ {
        let git = move |arguments: &[&str]| {
            let identity = [
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ];
            crate::views::tests::fixture_git(repository, &[&identity[..], arguments].concat())
        };
        git(&["reset", "-q", "--hard"]);
        git(&["checkout", "-q", "-b", "side"]);
        std::fs::write(repository.join("side.txt"), "side\n").unwrap();
        git(&["add", "side.txt"]);
        git(&["commit", "-q", "-m", "side"]);
        git(&["checkout", "-q", "main"]);
        for (name, text) in [("Index only", "first\n"), ("Layout experiment", "second\n")] {
            std::fs::write(repository.join("notes.txt"), text).unwrap();
            git(&["stash", "push", "-q", "-m", name]);
        }
        git
    }

    /// Lets a recovery action's preparation, which runs on the window's
    /// operation executor, answer and draws its outcome.
    fn recovery_lands(app: &Entity<GitTurtle>, cx: &mut gpui::VisualTestContext) {
        app.read_with(cx, |app, _| app.operations.drain());
        cx.executor().run_until_parked();
        crate::views::tests::draw(cx);
    }

    /// Moves focus with Tab onto the open Stashes' control `id`, which the
    /// stacked detail scrolls into view with its ring.
    fn tab_to(browser: &Entity<StashBrowser>, id: &'static str, cx: &mut gpui::VisualTestContext) {
        for _ in 0..40 {
            cx.simulate_keystrokes("tab");
            crate::views::tests::draw(cx);
            let focused = cx.update(|window, cx| browser.read(cx).detail.focused(window));
            if focused == Some(id.into()) {
                return;
            }
        }
        panic!("Tab never reached {id}");
    }

    /// A whole press of Space on the focused control: GPUI's keyboard click
    /// waits for the key up.
    fn space(cx: &mut gpui::VisualTestContext) {
        let keystroke = gpui::Keystroke::parse("space").expect("a keystroke");
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
    }

    /// Moves focus with Tab onto the open Stashes' Review restore…, which
    /// the stacked detail scrolls into view, and presses it with Space, as
    /// the keyboard does: Enter would confirm the dialog, closing it.
    fn press_review_restore(browser: &Entity<StashBrowser>, cx: &mut gpui::VisualTestContext) {
        tab_to(browser, "restore-saved-stash", cx);
        space(cx);
    }

    /// Every node an assistive technology reaches in the window's tree as
    /// the next frame writes it, through AccessKit's filter, under the
    /// dialog's own node: Stashes' footer note in the detail that clips it,
    /// and the window's alert and announcement beside the dialog layer.
    fn reached(cx: &mut gpui::VisualTestContext) -> Vec<crate::native_accessibility::Reachable> {
        use crate::native_accessibility::{reachable, written};
        written::forget();
        crate::views::tests::draw(cx);
        let (size, scale) =
            cx.update(|window, _| (window.viewport_size(), f64::from(window.scale_factor())));
        let edge = |value: Pixels| f64::from(f32::from(value)) * scale;
        let mut dialog = gpui::accesskit::Node::new(Role::Dialog);
        dialog.set_modal();
        let window = gpui::accesskit::Rect {
            x0: 0.,
            y0: 0.,
            x1: edge(size.width),
            y1: edge(size.height),
        };
        let tree = written::Assembly::default()
            .node("dialog", dialog, None)
            .everything(Some("dialog"))
            .tree(window, None);
        reachable(tree)
    }

    /// How many live regions in `found` say `text`, announced as `live`,
    /// outside every node that clips its children, as the window's alert
    /// and announcement are, wherever Stashes' detail has scrolled.
    fn said_from_the_window(
        found: &[crate::native_accessibility::Reachable],
        text: &str,
        live: gpui::accesskit::Live,
    ) -> usize {
        found
            .iter()
            .filter(|node| {
                node.name.as_deref() == Some(text) && node.live == live && node.clip.is_none()
            })
            .count()
    }

    /// What the window's AT-SPI adapter announces as it is fed the
    /// window's frames, as GPUI's Linux backend feeds it; no adapter runs
    /// elsewhere.
    struct Heard {
        #[cfg(target_os = "linux")]
        bus: crate::native_accessibility::atspi_state_tests::Bus,
    }

    impl Heard {
        /// The adapter as it starts on the window's next frame.
        fn start(cx: &mut gpui::VisualTestContext) -> Self {
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::{Bus, next_frame};
                Self {
                    bus: Bus::new(next_frame(cx)),
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                crate::views::tests::draw(cx);
                Self {}
            }
        }

        /// Feeds the adapter the window's next frame and returns what it
        /// announced since the last on Linux, `None` elsewhere.
        fn next(&mut self, cx: &mut gpui::VisualTestContext) -> Option<Vec<String>> {
            #[cfg(target_os = "linux")]
            {
                use crate::native_accessibility::atspi_state_tests::next_frame;
                self.bus.update(next_frame(cx));
                Some(self.bus.announced())
            }
            #[cfg(not(target_os = "linux"))]
            {
                crate::views::tests::draw(cx);
                None
            }
        }
    }

    /// Review restore… answers only what Stashes still shows, with the
    /// window's operations held so Git has not answered. Pressed on "Layout
    /// experiment", then "Index only" chosen: the preparation is cancelled
    /// at once, Review restore… can be pressed again, and once Git would
    /// have answered no review of "Layout experiment" opens; pressed again,
    /// it reviews "Index only". "Layout experiment" chosen again meanwhile,
    /// by a click on its row, Up on the first row and Home: nothing is read
    /// again and its review opens, where each read it again and cancelled
    /// the preparation with nothing said. The list read again meanwhile, as
    /// Refresh reads it: cancelled at once, and no review opens. Restore staged
    /// state chosen meanwhile: no review of the restoration without it
    /// opens, and pressed again the review restores it. Stashes closed with
    /// Escape: its browser's release cancels, and nothing opens or is said.
    /// An answer that outruns the
    /// cancellations, here for a choice changed behind their back, is
    /// dropped by the answer check: no review for the other Restore staged
    /// state, no refusal under a selection moved on. Stacked at 18 pt in a
    /// 461 × 490 window, with a merge paused: no refusal lands under the
    /// stash chosen meanwhile, and a refusal that lands with another stash
    /// chosen before a frame draws it is never heard and does not scroll
    /// that stash's detail to its foot once its inspection lands, while one
    /// a frame drew keeps its alert as another stash is chosen. Each
    /// answered what had been asked once it landed: the old stash's review
    /// opened, the refusal lay under the other stash, the review restored
    /// without the staged state, and the review popped up after Stashes had
    /// closed; the refusal's reveal, still due, scrolled the next stash's
    /// detail to its foot; and the window's alert said the refusal no frame
    /// had shown, for 1.5 s beside the stash chosen.
    #[gpui::test]
    async fn review_restore_answers_only_what_stashes_still_shows(cx: &mut gpui::TestAppContext) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = two_stashes(&repository);
        let browser = |cx: &mut gpui::VisualTestContext| {
            app.read_with(cx, |app, _| app.recovery.browser.clone())
                .and_then(|browser| browser.upgrade())
                .expect("Stashes is open")
        };
        let preparing = |cx: &mut gpui::VisualTestContext| {
            app.read_with(cx, |app, _| app.operation_busy == Some(PREPARING))
        };
        let pressable = || {
            crate::workspace::refusals::get("restore-saved-stash")
                .is_some_and(|refusal| !refusal.announced && !refusal.dimmed)
        };
        // The review open, by its explanation.
        let reviewed = |cx: &mut gpui::VisualTestContext| {
            cx.debug_bounds("operation-consequences").map(|_| {
                crate::native_accessibility::written::get("operation-consequences")
                    .and_then(|text| text.node.value().map(str::to_owned))
                    .unwrap_or_default()
            })
        };
        let close = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| window.close_all_dialogs(cx));
            crate::views::tests::draw(cx);
        };
        cx.simulate_resize(size(px(1480.), px(980.)));

        // Another stash chosen while Git prepares.
        open_selected_stash(&app, cx).await;
        let stashes = browser(cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        assert!(
            preparing(cx),
            "Git prepares Layout experiment's restoration"
        );
        let row = cx
            .debug_bounds("saved-stash-row-1")
            .expect("Index only's row");
        cx.simulate_click(row.center(), Modifiers::none());
        StashBrowser::lands(&stashes, cx);
        assert!(!preparing(cx), "choosing Index only cancels it");
        assert!(pressable(), "Review restore… can be pressed again at once");
        drop(held);
        recovery_lands(&app, cx);
        assert_eq!(reviewed(cx), None, "no review of Layout experiment opens");
        assert_eq!(
            stashes.read_with(cx, |stashes, _| stashes.selected),
            Some(1)
        );
        press_review_restore(&stashes, cx);
        recovery_lands(&app, cx);
        let review = reviewed(cx).expect("Index only's review opens");
        assert!(
            review.starts_with("Restore “On main: Index only”"),
            "{review}"
        );
        drop(stashes);
        close(cx);

        // The stash shown chosen again while Git prepares, by a click on its
        // row, then Up on the first row and Home: nothing is read again, the
        // preparation goes on and its review opens.
        open_selected_stash(&app, cx).await;
        let stashes = browser(cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        let generation = stashes.read_with(cx, |stashes, _| stashes.generation);
        let row = cx
            .debug_bounds("saved-stash-row-0")
            .expect("Layout experiment's row");
        cx.simulate_click(row.center(), Modifiers::none());
        crate::views::tests::draw(cx);
        // The click chose the row, which focuses the list: one that missed
        // it, leaving focus on Review restore…, would change nothing too.
        let list = stashes.read_with(cx, |stashes, _| stashes.stash_focus.clone());
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "the click on Layout experiment's row focuses the list"
        );
        for key in ["up", "home"] {
            cx.simulate_keystrokes(key);
            crate::views::tests::draw(cx);
        }
        assert!(
            preparing(cx),
            "choosing Layout experiment again cancels nothing"
        );
        assert_eq!(
            stashes.read_with(cx, |stashes, _| (
                stashes.generation,
                stashes.metadata_task.is_none(),
                stashes.selected
            )),
            (generation, true, Some(0)),
            "nothing is read again"
        );
        drop(held);
        recovery_lands(&app, cx);
        let review = reviewed(cx).expect("Layout experiment's review opens");
        assert!(
            review.starts_with("Restore “On main: Layout experiment”"),
            "{review}"
        );
        drop(stashes);
        close(cx);

        // The list read again while Git prepares, as Refresh, Previous and
        // More read it.
        open_selected_stash(&app, cx).await;
        let stashes = browser(cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        assert!(preparing(cx), "Git prepares the restoration");
        cx.update(|window, cx| stashes.update(cx, |stashes, cx| stashes.load_page(0, window, cx)));
        StashBrowser::lands(&stashes, cx);
        assert!(!preparing(cx), "reading the list again cancels it");
        drop(held);
        recovery_lands(&app, cx);
        assert_eq!(
            reviewed(cx),
            None,
            "no review opens once the list is read again"
        );
        drop(stashes);
        close(cx);

        // The other Restore staged state chosen while Git prepares.
        open_selected_stash(&app, cx).await;
        let stashes = browser(cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        assert!(preparing(cx), "Git prepares the restoration");
        tab_to(&stashes, "stash-restore-index", cx);
        space(cx);
        crate::views::tests::draw(cx);
        assert!(stashes.read_with(cx, |stashes, _| stashes.restore_index));
        assert!(!preparing(cx), "choosing Restore staged state cancels it");
        assert!(pressable(), "Review restore… can be pressed again at once");
        drop(held);
        recovery_lands(&app, cx);
        assert_eq!(
            reviewed(cx),
            None,
            "no review without the staged state opens"
        );
        press_review_restore(&stashes, cx);
        recovery_lands(&app, cx);
        let review = reviewed(cx).expect("the review opens");
        assert!(
            review.contains("Restore the original staged state"),
            "{review}"
        );
        drop(stashes);
        close(cx);

        // Stashes closed while Git prepares, its dialog holding the
        // browser's only handle: the browser's release cancels.
        open_selected_stash(&app, cx).await;
        let stashes = browser(cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        drop(stashes);
        cx.simulate_keystrokes("escape");
        crate::views::tests::draw(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "Escape closes Stashes"
        );
        assert!(!preparing(cx), "closing Stashes cancels it");
        drop(held);
        recovery_lands(&app, cx);
        assert_eq!(
            reviewed(cx),
            None,
            "no review opens once Stashes has closed"
        );
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error.clone()),
            None,
            "and nothing is said"
        );

        // An answer that outruns the cancellations, for the other Restore
        // staged state chosen behind their back: the answer check drops it.
        open_selected_stash(&app, cx).await;
        let stashes = browser(cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        stashes.update(cx, |stashes, _| {
            stashes.restore_index = !stashes.restore_index
        });
        assert!(preparing(cx), "nothing cancels the preparation");
        drop(held);
        recovery_lands(&app, cx);
        assert_eq!(
            reviewed(cx),
            None,
            "no review for the choice no longer shown"
        );
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Stashes stays open"
        );
        drop(stashes);
        close(cx);

        // Stacked, with a merge paused: another stash chosen while Git
        // prepares, a refusal that outruns the cancellations, and one that
        // lands as another stash is chosen.
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        open_selected_stash(&app, cx).await;
        let stashes = browser(cx);
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        cx.update(|window, cx| {
            stashes.update(cx, |stashes, cx| stashes.select_stash(1, window, cx))
        });
        StashBrowser::lands(&stashes, cx);
        assert!(!preparing(cx), "choosing Index only cancels it");
        drop(held);
        recovery_lands(&app, cx);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| (
                stashes.selected,
                stashes.restore_line.clone()
            )),
            (Some(1), None),
            "no refusal lands under Index only"
        );
        assert!(cx.debug_bounds("stash-restore-refusal").is_none());
        // The selection moved on behind the cancellations' back: the answer
        // check drops the refusal.
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        stashes.update(cx, |stashes, _| {
            stashes.generation = stashes.generation.wrapping_add(1)
        });
        assert!(preparing(cx), "nothing cancels the preparation");
        drop(held);
        recovery_lands(&app, cx);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
            None,
            "no refusal for the selection no longer shown"
        );
        // "Layout experiment" chosen as Index only's refusal lands, before a
        // frame draws it, as a click handled before the display's next
        // frame is: the test window draws as soon as the refusal's update
        // ends, so the choice is made from an observer, which runs first.
        let held = app.update(cx, |app, _| app.operations.hold());
        press_review_restore(&stashes, cx);
        let alerted = crate::copy_feedback::tests::alerted(cx).map(|(id, _)| id);
        let chooses = cx.update(|window, cx| {
            window.observe(&stashes, cx, |stashes, window, cx| {
                let refused = stashes.read(cx);
                if refused.restore_line.is_some() && refused.selected == Some(1) {
                    stashes.update(cx, |stashes, cx| stashes.select_stash(0, window, cx));
                }
            })
        });
        drop(held);
        recovery_lands(&app, cx);
        drop(chooses);
        let refused = crate::copy_feedback::tests::alerted(cx)
            .filter(|(id, text)| Some(id) != alerted.as_ref() && text == PAUSED_REFUSAL);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| (
                stashes.selected,
                stashes.restore_line.clone()
            )),
            (Some(0), None),
            "Layout experiment chosen"
        );
        assert_eq!(
            refused, None,
            "Index only's refusal, which no frame drew, is never said"
        );
        StashBrowser::lands(&stashes, cx);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| stashes.detail.scroll().offset().y),
            px(0.),
            "Layout experiment's detail opens at its top"
        );
        // A refusal a frame has drawn has been said with it: choosing
        // another stash then leaves its alert to end in its own time.
        press_review_restore(&stashes, cx);
        recovery_lands(&app, cx);
        let said =
            crate::copy_feedback::tests::alerted(cx).filter(|(_, text)| text == PAUSED_REFUSAL);
        assert!(said.is_some(), "Layout experiment is refused");
        cx.update(|window, cx| {
            stashes.update(cx, |stashes, cx| stashes.select_stash(1, window, cx))
        });
        StashBrowser::lands(&stashes, cx);
        assert_eq!(
            crate::copy_feedback::tests::alerted(cx),
            said,
            "the drawn refusal's alert stays as Index only is chosen"
        );
        git(&["merge", "--abort"]);
        drop(stashes);
        close(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        drop(fixture);
    }

    /// Review restore…'s preparation cancelled, as another stash is chosen
    /// or Stashes closes, runs the working read Refresh asked for while the
    /// restoration was prepared, as a form's Cancel does
    /// (`GitTurtle::cancel_recovery_preparation`): the preparation's end,
    /// which would have read it, never runs. The read stayed noted, and the
    /// working state stayed as read before it until something else ended.
    #[gpui::test]
    async fn a_refresh_asked_for_while_review_restore_prepares_runs_once_it_is_cancelled(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let _git = two_stashes(&repository);
        cx.simulate_resize(size(px(1480.), px(980.)));
        let browser = |cx: &mut gpui::VisualTestContext| {
            app.read_with(cx, |app, _| app.recovery.browser.clone())
                .and_then(|browser| browser.upgrade())
                .expect("Stashes is open")
        };
        let listed = |cx: &mut gpui::VisualTestContext, path: &str| {
            app.read_with(cx, |app, _| {
                app.work_status.as_ref().is_some_and(|status| {
                    status
                        .entries
                        .iter()
                        .any(|entry| entry.path == std::path::Path::new(path))
                })
            })
        };
        for (index, cancel) in ["Index only chosen", "Stashes closed"]
            .into_iter()
            .enumerate()
        {
            open_selected_stash(&app, cx).await;
            let stashes = browser(cx);
            // Review restore…, held as a slow disk holds it, and Refresh.
            let held = app.update(cx, |app, _| app.operations.hold());
            press_review_restore(&stashes, cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                Some(PREPARING),
                "{cancel}: the restoration is prepared"
            );
            let later = format!("later-{index}.txt");
            std::fs::write(repository.join(&later), "later\n").unwrap();
            cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
            assert!(
                app.read_with(cx, |app, _| app.working_read_deferred.is_some()),
                "{cancel}: Refresh waits for the preparation"
            );
            if index == 0 {
                let row = cx
                    .debug_bounds("saved-stash-row-1")
                    .expect("Index only's row");
                cx.simulate_click(row.center(), Modifiers::none());
                StashBrowser::lands(&stashes, cx);
                drop(stashes);
            } else {
                // The dialog holds the browser's only handle, whose release
                // cancels.
                drop(stashes);
                cx.simulate_keystrokes("escape");
                crate::views::tests::draw(cx);
            }
            app.read_with(cx, |app, _| {
                assert_eq!(app.operation_busy, None, "{cancel}: cancelled");
                assert_eq!(
                    app.working_read_deferred, None,
                    "{cancel}: the read Refresh asked for is taken up"
                );
                assert!(app.status_task.is_some(), "{cancel}: and runs");
            });
            drop(held);
            recovery_lands(&app, cx);
            crate::views::tests::settle(&app, cx).await;
            assert!(
                listed(cx, &later),
                "{cancel}: Refresh read the working state"
            );
            cx.update(|window, cx| window.close_all_dialogs(cx));
            crate::views::tests::draw(cx);
        }
        drop(fixture);
    }

    /// The keyboard ring the last frame painted around `control`: the
    /// tightest outline in the accent, at any opacity, that encloses it.
    fn ring_around(
        cx: &mut gpui::VisualTestContext,
        control: Bounds<Pixels>,
    ) -> Option<crate::focus_ring::PaintedOutline> {
        let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
        crate::focus_ring::painted_outlines_where(cx, move |color| {
            color.a > 0.
                && Hsla {
                    a: accent.a,
                    ..color
                } == accent
        })
        .into_iter()
        .filter(|ring| {
            ring.bounds.left() <= control.left()
                && ring.bounds.top() <= control.top()
                && ring.bounds.right() >= control.right()
                && ring.bounds.bottom() >= control.bottom()
        })
        .min_by(|a, b| {
            let area = |ring: &crate::focus_ring::PaintedOutline| {
                f32::from(ring.bounds.size.width) * f32::from(ring.bounds.size.height)
            };
            area(a).total_cmp(&area(b))
        })
    }

    /// Review restore… in Stashes reads the pause in the core rather than
    /// refuse from the one the window read last. A merge aborted in a
    /// terminal, unseen, no longer refuses it: Stashes gives way to the
    /// Restore stash review, as before the window refused from its cache,
    /// and the window reads its working state again. A merge that is paused
    /// refuses it in place, in 461 × 490 and 1480 × 980 windows at 13 and
    /// 18 pt: Stashes stays open with its stash selected, the footer note
    /// beside Review restore… says why, whole in the detail's view and
    /// beneath the control, a status that the window's alert says at once
    /// on each refusal, and nowhere else, and once a read finds the merge
    /// ended the note says so, which the window's announcement says once
    /// speech pauses. Refused from the
    /// window's cache, Stashes closed and the window's error that said why
    /// went with the quiet read that found the merge ended, within 0.3 s
    /// natively, so nothing showed and the selection was lost. Pressed from
    /// the keyboard, Review restore… keeps focus and its ring whole as the
    /// refusal lands, so Space presses it again; at 18 pt in the 461 × 490
    /// window, where the detail is about 60 px tall, the refusal keeps one
    /// line beneath it. The detail scrolled to show the whole two-line
    /// refusal there and cut Review restore…'s ring. The refusal's node is
    /// static text that reports the part of it the window draws, cut at the
    /// detail's foot once the detail scrolls it half past there, where it
    /// reported its whole layout.
    #[gpui::test]
    async fn review_restore_reads_the_pause_and_answers_in_stashes(cx: &mut gpui::TestAppContext) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        // The app's palette, whose accent draws the keyboard ring.
        crate::focus_ring::sweep::prepare(&app, cx);
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = two_stashes(&repository);
        let browser = |cx: &mut gpui::VisualTestContext| {
            app.read_with(cx, |app, _| app.recovery.browser.clone())
                .and_then(|browser| browser.upgrade())
        };
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - slack
                && inner.right() <= outer.right() + slack
                && inner.top() >= outer.top() - slack
                && inner.bottom() <= outer.bottom() + slack
        };

        // Aborted unseen: the core accepts and the review opens.
        cx.simulate_resize(size(px(1480.), px(980.)));
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        read_pause(&app, cx).await;
        open_selected_stash(&app, cx).await;
        git(&["merge", "--abort"]);
        let stashes = browser(cx).expect("Stashes is open");
        press_review_restore(&stashes, cx);
        drop(stashes);
        recovery_lands(&app, cx);
        assert!(
            cx.debug_bounds("operation-consequences").is_some()
                && cx.debug_bounds("stash-browser").is_none(),
            "the Restore stash review opens in place of Stashes"
        );
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error.clone()),
            None
        );
        quiet_reads_land(&app, cx);
        assert!(
            app.read_with(cx, |app, _| app.integration_state.is_none()),
            "the window reads the merge ended"
        );
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);

        // Paused: refused in place, and the pause's end said there.
        let mut last_alert = None;
        for (width, height) in [(461., 490.), (1480., 980.)] {
            for interface in [13, 18] {
                let at = format!("{interface} pt in {width} × {height}");
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                // Unread: the core's refusal is the first the window hears
                // of the pause.
                git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
                open_selected_stash(&app, cx).await;
                let stashes = browser(cx).expect("Stashes is open");
                press_review_restore(&stashes, cx);
                recovery_lands(&app, cx);
                assert!(
                    cx.update(|window, cx| window.has_active_dialog(cx))
                        && cx.debug_bounds("operation-consequences").is_none(),
                    "{at}: Stashes stays open"
                );
                assert_eq!(
                    stashes.read_with(cx, |stashes, _| (
                        stashes.selected,
                        stashes.snapshot.is_some(),
                        stashes.restore_line.clone()
                    )),
                    (Some(0), true, Some(PAUSED_REFUSAL.to_owned())),
                    "{at}: the stash stays selected and refused"
                );
                let [line, restore, detail, browser_bounds] = [
                    "stash-restore-refusal",
                    "restore-saved-stash",
                    "stash-detail-column",
                    "stash-browser",
                ]
                .map(|name| {
                    cx.debug_bounds(name)
                        .unwrap_or_else(|| panic!("{at}: no {name}"))
                });
                assert!(
                    inside(line, detail) && inside(line, browser_bounds),
                    "{at}: the refusal {line:?} lies whole in the detail {detail:?}"
                );
                assert!(
                    line.top() >= restore.bottom() && line.top() - restore.bottom() < px(24.),
                    "{at}: the refusal {line:?} lies beneath Review restore… {restore:?}"
                );
                // Review restore… keeps focus, whole with its ring.
                assert_eq!(
                    cx.update(|window, cx| stashes.read(cx).detail.focused(window)),
                    Some("restore-saved-stash".into()),
                    "{at}: Review restore… keeps focus"
                );
                let ring = ring_around(cx, restore)
                    .unwrap_or_else(|| panic!("{at}: Review restore… {restore:?} draws no ring"));
                assert!(ring.whole, "{at}: Review restore…'s ring {ring:?} is whole");
                let one = cx.update(|window, _| {
                    crate::image_compare::text_line(appearance::ui_text(10.), window)
                });
                let lines = (f32::from(line.size.height) / f32::from(one)).round() as usize;
                if width < 800. && interface == 18 {
                    assert_eq!(lines, 1, "{at}: the refusal {line:?} keeps one line");
                }
                let (alert, said) = crate::copy_feedback::tests::alerted(cx)
                    .unwrap_or_else(|| panic!("{at}: the window's alert says the refusal"));
                assert!(
                    said == PAUSED_REFUSAL && last_alert.as_ref() != Some(&alert),
                    "{at}: the window's alert says the refusal at once, not {said:?}"
                );
                assert_eq!(
                    crate::native_accessibility::written::announced_from("stash-restore-"),
                    Vec::new(),
                    "{at}: the note is no live region of its own"
                );
                assert_eq!(
                    app.read_with(cx, |app, _| app.operation_error.clone()),
                    None,
                    "{at}: and nowhere else"
                );
                // Its node is static text that reports the part of it the
                // detail draws: all of it, revealed, and only what the
                // detail's foot leaves of it once the detail scrolls it half
                // past there, at 18 pt in the 461 × 490 window. It reported
                // its whole layout wherever the detail scrolled it.
                let scale = f64::from(cx.update(|window, _| window.scale_factor()));
                let physical = |edge: Pixels| f64::from(f32::from(edge)) * scale;
                let node = || {
                    let written =
                        crate::native_accessibility::written::get("stash-restore-refusal")
                            .unwrap_or_else(|| {
                                panic!("{at}: the refusal writes its node as static text")
                            });
                    assert_eq!(written.node.role(), Role::Status, "{at}");
                    assert_eq!(written.node.label(), Some(PAUSED_REFUSAL), "{at}");
                    written.node.bounds().expect("bounds")
                };
                let whole = node();
                assert!(
                    (whole.y0 - physical(line.top())).abs() < 0.5
                        && (whole.y1 - physical(line.bottom())).abs() < 0.5,
                    "{at}: the whole refusal {line:?} reports {whole:?}"
                );
                if width < 800. && interface == 18 {
                    let scroll =
                        stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
                    let kept = scroll.offset();
                    let foot = cx
                        .debug_bounds("stash-detail")
                        .expect("the detail")
                        .bottom();
                    let middle = line.top() + line.size.height / 2.;
                    scroll.set_offset(point(kept.x, kept.y + (foot - middle)));
                    crate::views::tests::draw(cx);
                    let cut = cx
                        .debug_bounds("stash-restore-refusal")
                        .expect("the refusal");
                    let foot = cx
                        .debug_bounds("stash-detail")
                        .expect("the detail")
                        .bottom();
                    assert!(
                        cut.top() < foot && cut.bottom() > foot,
                        "{at}: the detail's foot {foot:?} cuts the refusal {cut:?}"
                    );
                    let drawn = node();
                    assert!(
                        (drawn.y0 - physical(cut.top())).abs() < 0.5
                            && (drawn.y1 - physical(foot)).abs() < 0.5,
                        "{at}: the refusal {cut:?}, cut at {foot:?}, reports {drawn:?}"
                    );
                    scroll.set_offset(kept);
                    crate::views::tests::draw(cx);
                }
                quiet_reads_land(&app, cx);
                assert!(
                    app.read_with(cx, |app, _| app.integration_state.is_some()),
                    "{at}: the refusal has the window read the pause"
                );
                // Pressed again where it is, the window's alert says it once
                // more, a fresh node in the same words.
                space(cx);
                recovery_lands(&app, cx);
                let again = crate::copy_feedback::tests::alerted(cx);
                assert!(
                    again
                        .as_ref()
                        .is_some_and(|(id, said)| *id != alert && said == PAUSED_REFUSAL),
                    "{at}: each refusal is said again, not {again:?} after {alert:?}"
                );
                last_alert = again.map(|(id, _)| id);
                // The merge ends; the next read says so in the note.
                git(&["merge", "--abort"]);
                cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
                crate::views::tests::settle(&app, cx).await;
                quiet_reads_land(&app, cx);
                crate::views::tests::draw(cx);
                assert!(
                    cx.debug_bounds("stash-restore-refusal").is_none(),
                    "{at}: the refusal goes"
                );
                let ended = cx
                    .debug_bounds("stash-restore-ended")
                    .unwrap_or_else(|| panic!("{at}: the note says the merge ended"));
                let detail = cx.debug_bounds("stash-detail-column").unwrap();
                assert!(inside(ended, detail), "{at}: {ended:?} in {detail:?}");
                let restore = cx.debug_bounds("restore-saved-stash").unwrap();
                assert!(
                    ring_around(cx, restore).is_some_and(|ring| ring.whole),
                    "{at}: Review restore… {restore:?} keeps its ring whole"
                );
                assert_eq!(
                    crate::copy_feedback::tests::announced(cx)
                        .map(|(_, said)| said)
                        .as_deref(),
                    Some(PAUSE_ENDED),
                    "{at}: the window's announcement says so, heard once speech pauses"
                );
                assert_eq!(
                    crate::native_accessibility::written::announced_from("stash-restore-"),
                    Vec::new(),
                    "{at}: the note is no live region of its own"
                );
                drop(stashes);
                cx.update(|window, cx| window.close_all_dialogs(cx));
                crate::views::tests::draw(cx);
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        drop(fixture);
    }

    /// When Git itself fails to prepare Review restore…'s restoration, as a
    /// status read of a damaged index does, Stashes stays open and its note
    /// gives the first line of Git's report, in 1480 × 980 and 461 × 490
    /// windows at 13 and 18 pt, whether the report runs past that line, as
    /// a corrupt index's does, or is that one line, as a truncated index's
    /// is: one or two lines, one where the stacked detail is about 60 px
    /// tall (18 pt in 461 × 490), whole in what the window shows of the
    /// detail, its scrolling view stacked and its column beside the list,
    /// and not in the window's error. Review restore…, which the press left
    /// focused, keeps focus, whole in that view with its ring. Details…
    /// beside the note, whole there with its ring as Tab reaches it, opens
    /// the whole report with Copy details above Stashes, and Escape closes
    /// it back to Stashes with focus on Details… again, whole with its ring.
    /// A report that opens with an empty line gives its first line that
    /// says anything, and one that says nothing gives that Git could not
    /// prepare the restoration, with no Details…. The stash shown, chosen
    /// again after a terminal saved another stash, is not read again, and
    /// Review restore…'s own plan finds the list changed. The note took
    /// Git's whole report, its stderr line after line, unclamped beside the
    /// list and with no Details… to read or copy it; a report of one line
    /// had no Details…, so stacked at 18 pt its rest lay only in a tooltip
    /// the keyboard never opens; a report that opened with an empty line, or
    /// said nothing, showed an empty note; and a first line longer than an
    /// accessible text carries reached the window's alert and the note's
    /// name whole.
    #[gpui::test]
    async fn a_failed_restore_preparation_says_its_first_line_with_details(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = two_stashes(&repository);
        // The app's palette, whose accent draws the keyboard ring.
        crate::focus_ring::sweep::prepare(&app, cx);
        let index = repository.join(".git").join("index");
        let kept = std::fs::read(&index).expect("the index");
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - slack
                && inner.right() <= outer.right() + slack
                && inner.top() >= outer.top() - slack
                && inner.bottom() <= outer.bottom() + slack
        };
        let note_label = || {
            crate::native_accessibility::written::get("stash-restore-refusal")
                .and_then(|note| note.node.label().map(str::to_owned))
        };
        // Git cannot read the status of a corrupt index, and says so on two
        // lines, or of a truncated one, and says so on one.
        let damaged: [(&str, &[u8], bool); 2] = [
            ("a corrupt index", &[b'x'; 128], true),
            ("a truncated index", b"DIRC", false),
        ];
        for (width, height) in [(1480., 980.), (461., 490.)] {
            for interface in [13, 18] {
                for (damage, bytes, more) in damaged {
                    let at = format!("{damage} at {interface} pt in {width} × {height}");
                    cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                    cx.simulate_resize(size(px(width), px(height)));
                    open_selected_stash(&app, cx).await;
                    let stashes = app
                        .read_with(cx, |app, _| app.recovery.browser.clone())
                        .and_then(|browser| browser.upgrade())
                        .expect("Stashes is open");
                    std::fs::write(&index, bytes).unwrap();
                    press_review_restore(&stashes, cx);
                    recovery_lands(&app, cx);
                    std::fs::write(&index, &kept).unwrap();
                    let report = stashes
                        .read_with(cx, |stashes, _| stashes.restore_line.clone())
                        .unwrap_or_else(|| panic!("{at}: Review restore… is refused"));
                    assert_eq!(
                        report.trim_end().lines().count() > 1,
                        more,
                        "{at}: Git's report: {report}"
                    );
                    assert_eq!(
                        note_label().as_deref(),
                        report.lines().next(),
                        "{at}: the note gives the report's first line"
                    );
                    assert_eq!(
                        crate::copy_feedback::tests::alerted(cx)
                            .map(|(_, said)| said)
                            .as_deref(),
                        report.lines().next(),
                        "{at}: the window's alert says that line, not the whole report"
                    );
                    assert_eq!(
                        app.read_with(cx, |app, _| app.operation_error.clone()),
                        None,
                        "{at}: Stashes says it, not the window"
                    );
                    // What the window shows of the detail: stacked, the
                    // detail's scrolling view, which the footer scrolls in;
                    // beside the list, its column, whose footer lies below
                    // the view.
                    let stacked = width < 800.;
                    let shown = if stacked {
                        "stash-detail"
                    } else {
                        "stash-detail-column"
                    };
                    let [note, restore, view] =
                        ["stash-restore-refusal", "restore-saved-stash", shown].map(|name| {
                            cx.debug_bounds(name)
                                .unwrap_or_else(|| panic!("{at}: no {name}"))
                        });
                    let one = cx.update(|window, _| {
                        crate::image_compare::text_line(appearance::ui_text(10.), window)
                    });
                    let lines = (f32::from(note.size.height) / f32::from(one)).round() as usize;
                    // One line where the stacked detail is about 60 px tall,
                    // and two at most elsewhere.
                    let keeps = if stacked && interface == 18 { 1 } else { 2 };
                    assert!(
                        (1..=keeps).contains(&lines) && inside(note, view),
                        "{at}: the note {note:?} keeps {lines} lines, at most {keeps}, whole in {view:?}"
                    );
                    // Review restore…, which the press left focused, keeps
                    // focus, whole in view with its ring, as the failure
                    // lands: its row is taller than the paused refusal's,
                    // with Details… beside the note.
                    assert_eq!(
                        cx.update(|window, cx| stashes.read(cx).detail.focused(window)),
                        Some("restore-saved-stash".into()),
                        "{at}: Review restore… keeps focus"
                    );
                    let ring = ring_around(cx, restore).unwrap_or_else(|| {
                        panic!("{at}: Review restore… {restore:?} draws no ring")
                    });
                    assert!(
                        ring.whole && inside(restore, view),
                        "{at}: Review restore… {restore:?} and its ring {ring:?} are whole in {view:?}"
                    );
                    // Details… opens the whole report above Stashes, and
                    // Escape closes it back to Stashes, focus on Details…
                    // again, whole in view with its ring.
                    let details_whole = |cx: &mut gpui::VisualTestContext, when: &str| {
                        assert_eq!(
                            cx.update(|window, cx| stashes.read(cx).detail.focused(window)),
                            Some("stash-restore-details".into()),
                            "{at}: Details… holds focus {when}"
                        );
                        let [details, view] = ["stash-restore-details", shown].map(|name| {
                            cx.debug_bounds(name)
                                .unwrap_or_else(|| panic!("{at}: no {name} {when}"))
                        });
                        let ring = ring_around(cx, details)
                            .unwrap_or_else(|| panic!("{at}: Details… draws no ring {when}"));
                        assert!(
                            ring.whole && inside(details, view),
                            "{at}: Details… {details:?} and its ring {ring:?} are whole in {view:?} {when}"
                        );
                    };
                    tab_to(&stashes, "stash-restore-details", cx);
                    details_whole(cx, "as Tab reaches it");
                    space(cx);
                    crate::views::tests::draw(cx);
                    assert!(
                        cx.debug_bounds("copy-operation-details").is_some(),
                        "{at}: Details… opens the report, with Copy details"
                    );
                    cx.simulate_keystrokes("escape");
                    cx.executor()
                        .advance_clock(std::time::Duration::from_millis(300));
                    crate::views::tests::draw(cx);
                    assert!(
                        cx.debug_bounds("copy-operation-details").is_none()
                            && cx.update(|window, cx| window.has_active_dialog(cx))
                            && cx.debug_bounds("stash-browser").is_some(),
                        "{at}: Escape closes the report back to Stashes"
                    );
                    details_whole(cx, "once the report closes");
                    drop(stashes);
                    cx.update(|window, cx| window.close_all_dialogs(cx));
                    crate::views::tests::draw(cx);
                }
            }
        }
        // A report that opens with an empty line gives the first line that
        // says anything, with Details… beside it.
        open_selected_stash(&app, cx).await;
        let stashes = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");
        let refuse = |message: &str, cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| {
                stashes.update(cx, |stashes, cx| {
                    stashes.refuse(message.into(), None, window, cx)
                })
            });
            crate::views::tests::draw(cx);
            crate::copy_feedback::tests::alerted(cx).map(|(_, said)| said)
        };
        let said = refuse(
            "\n  \nUnable to read working-copy status: fatal: index file corrupt\n",
            cx,
        );
        let first = "Unable to read working-copy status: fatal: index file corrupt";
        assert_eq!(note_label().as_deref(), Some(first));
        assert_eq!(said.as_deref(), Some(first), "the window's alert says it");
        assert!(cx.debug_bounds("stash-restore-details").is_some());
        // A report that says nothing still says that Git could not prepare
        // the restoration, with no Details… to open nothing.
        let said = refuse("\n  \n", cx);
        assert_eq!(
            note_label().as_deref(),
            Some(RESTORE_UNEXPLAINED),
            "a blank report gives the fallback line"
        );
        assert_eq!(
            said.as_deref(),
            Some(RESTORE_UNEXPLAINED),
            "the window's alert says it"
        );
        assert!(cx.debug_bounds("stash-restore-details").is_none());
        // A first line past what an accessible text carries, as one line of
        // Git's stderr can be, is said and named by at most 16 KiB of it,
        // ending in an ellipsis, alike: whole, the alert and the note carried
        // what core keeps of the stream, up to 4 MiB.
        let long = "Unable to read working-copy status: ".to_owned() + &"x".repeat(64 * 1024);
        let said = refuse(&format!("{long}\nmore\n"), cx).expect("the window's alert says it");
        assert!(
            said.len() <= 16 * 1024
                && said.ends_with('…')
                && long.starts_with(said.trim_end_matches('…')),
            "the window's alert says {} bytes of the {}-byte line",
            said.len(),
            long.len()
        );
        assert!(
            note_label().is_some_and(|named| named == said),
            "the note is named by the line the alert says"
        );
        assert!(cx.debug_bounds("stash-restore-details").is_some());
        drop(stashes);
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
        // Chosen again after a terminal saved another stash, the stash shown
        // is not read again: Review restore…'s own plan finds the list
        // changed, and the note says so.
        open_selected_stash(&app, cx).await;
        let stashes = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");
        std::fs::write(repository.join("notes.txt"), "third\n").unwrap();
        git(&["stash", "push", "-q", "-m", "Saved in a terminal"]);
        let row = cx
            .debug_bounds("saved-stash-row-0")
            .expect("Layout experiment's row");
        cx.simulate_click(row.center(), Modifiers::none());
        crate::views::tests::draw(cx);
        assert!(
            stashes.read_with(cx, |stashes, _| stashes.metadata_task.is_none()),
            "chosen again, the stash shown is not read again"
        );
        press_review_restore(&stashes, cx);
        recovery_lands(&app, cx);
        assert_eq!(
            note_label().as_deref(),
            Some(
                "The stash list changed. Refresh and review the selected stash again; no stash was removed."
            ),
            "Review restore…'s plan finds the list changed"
        );
        drop(stashes);
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        drop(fixture);
    }

    /// Stacked at 18 pt in a 461 × 490 window, where the detail is about
    /// 60 px tall, the note that a merge has ended scrolls the detail only
    /// while nothing else in it holds focus. Review restore… refused while
    /// the merge pauses, focus moves to the saved files, as a click on one
    /// moves it, and the merge is aborted in a terminal: once a read finds
    /// it ended, the note says so, and the detail stays where it is, with
    /// the saved files focused and whole in its view; the note below its
    /// fold, out of AccessKit's reach, is said once by the window's
    /// announcement, through AccessKit's filter and, on Linux, the AT-SPI
    /// adapter, and scrolled to, it is a status in reach that no one hears
    /// again. A polite note of its own there was left out by the filter
    /// and heard by no one. Refused again with
    /// focus on the saved-work list instead, outside the detail, and the
    /// detail scrolled to its top: the end reveals Review restore… and the
    /// note whole beneath it. The detail jumped to Review restore… and the note
    /// on any read that found the merge ended, scrolling the focused saved
    /// files out of view.
    #[gpui::test]
    async fn the_end_of_a_pause_leaves_focus_in_view_in_stashes(cx: &mut gpui::TestAppContext) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = two_stashes(&repository);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - slack
                && inner.right() <= outer.right() + slack
                && inner.top() >= outer.top() - slack
                && inner.bottom() <= outer.bottom() + slack
        };
        let ended = |stashes: &Entity<StashBrowser>, cx: &mut gpui::VisualTestContext| {
            stashes.read_with(cx, |stashes, _| {
                stashes.restore_line.is_none() && stashes.pause_ended
            }) && cx.debug_bounds("stash-restore-ended").is_some()
        };

        // Refused while the merge pauses, unread, which the refusal has the
        // window read.
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        open_selected_stash(&app, cx).await;
        let stashes = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");
        press_review_restore(&stashes, cx);
        recovery_lands(&app, cx);
        quiet_reads_land(&app, cx);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
            Some(PAUSED_REFUSAL.to_owned())
        );
        assert!(app.read_with(cx, |app, _| app.integration_state.is_some()));
        // Focus moves to the saved files, as a click on one moves it, and
        // they scroll into the detail's view.
        let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
        cx.update(|window, cx| window.focus(&files, cx));
        crate::views::tests::draw(cx);
        let scroll = stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
        let kept = scroll.offset();
        let in_view = |cx: &mut gpui::VisualTestContext| {
            let [files, view] =
                ["stash-files", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
            // Whole in view, no taller than the detail shows.
            (inside(files, view), files, view)
        };
        let (shown, files_at, view) = in_view(cx);
        assert!(
            shown,
            "the saved files {files_at:?} are whole in view {view:?} at {kept:?}"
        );
        let mut heard = Heard::start(cx);
        // The merge ends in a terminal; the next read says so in the note.
        git(&["merge", "--abort"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        crate::views::tests::draw(cx);
        assert!(ended(&stashes, cx), "the note says the merge ended");
        assert_eq!(
            scroll.offset(),
            kept,
            "the detail stays where the keyboard left it"
        );
        assert!(
            cx.update(|window, _| files.is_focused(window)) && in_view(cx).0,
            "the saved files keep focus, whole in view"
        );
        // Below the fold, the note is out of reach; the window's
        // announcement says it, once, as the read lands.
        let found = reached(cx);
        assert!(
            !found
                .iter()
                .any(|node| node.clip.is_some() && node.name.as_deref() == Some(PAUSE_ENDED)),
            "the note below the fold is out of reach: {found:?}"
        );
        assert_eq!(
            said_from_the_window(&found, PAUSE_ENDED, gpui::accesskit::Live::Polite),
            1,
            "the window's announcement says the merge ended, in reach: {found:?}"
        );
        assert_eq!(
            crate::native_accessibility::written::announced_from("stash-restore-"),
            Vec::new(),
            "the note is no live region of its own"
        );
        if let Some(heard) = heard.next(cx) {
            assert_eq!(
                heard.iter().filter(|said| *said == PAUSE_ENDED).count(),
                1,
                "AT-SPI announces the end once as the read lands: {heard:?}"
            );
        }
        // Scrolled to the note, it is in reach, read where it lies, and
        // nothing says it again.
        scroll.set_offset(point(kept.x, -scroll.max_offset().y));
        let found = reached(cx);
        assert!(
            found.iter().any(|node| node.role == Role::Status
                && node.clip.is_some()
                && node.live == gpui::accesskit::Live::Off
                && node.name.as_deref() == Some(PAUSE_ENDED)),
            "the note scrolled into view is a status in reach: {found:?}"
        );
        if let Some(heard) = heard.next(cx) {
            assert!(
                !heard.iter().any(|said| said == PAUSE_ENDED),
                "AT-SPI says the end once, not again on scroll: {heard:?}"
            );
        }

        // Refused again, the keyboard on the saved-work list, outside the
        // detail, and the detail at its top.
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        read_pause(&app, cx).await;
        press_review_restore(&stashes, cx);
        recovery_lands(&app, cx);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
            Some(PAUSED_REFUSAL.to_owned())
        );
        let list = stashes.read_with(cx, |stashes, _| stashes.stash_focus.clone());
        cx.update(|window, cx| window.focus(&list, cx));
        scroll.set_offset(Point::default());
        crate::views::tests::draw(cx);
        let restore = cx
            .debug_bounds("restore-saved-stash")
            .expect("Review restore…");
        let view = cx.debug_bounds("stash-detail").expect("the detail");
        assert!(
            !inside(restore, view),
            "Review restore… {restore:?} lies below the detail's view {view:?}"
        );
        git(&["merge", "--abort"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        crate::views::tests::draw(cx);
        assert!(ended(&stashes, cx), "the note says the merge ended");
        let [note, restore, view] = ["stash-restore-ended", "restore-saved-stash", "stash-detail"]
            .map(|name| cx.debug_bounds(name).expect(name));
        assert!(
            inside(restore, view) && inside(note, view),
            "Review restore… {restore:?} and the note {note:?} are revealed in {view:?}"
        );
        assert!(
            cx.update(|window, _| list.is_focused(window)),
            "the saved-work list keeps focus"
        );
        drop(stashes);
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        drop(fixture);
    }

    /// Stacked at 18 pt in a 461 × 490 window, where the detail is about
    /// 60 px tall, a click on Review restore… shows its answer wherever
    /// focus lies. Tab leaves focus on the saved files, the detail is
    /// scrolled to show Review restore… at its foot with the note below its
    /// fold, and a click on Review restore… while a merge pauses, which
    /// focuses no Button and so leaves focus on the saved files, is
    /// refused: the detail scrolls to Review restore… and the refusal
    /// beneath it, whole in its view. The note's reveal scrolled only while
    /// Review restore… or nothing in the detail held focus, as only the end
    /// of a pause should, so the click's refusal lay below the fold with
    /// nothing in view saying anything.
    #[gpui::test]
    async fn a_clicked_review_restore_shows_its_answer_wherever_focus_lies(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = two_stashes(&repository);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - slack
                && inner.right() <= outer.right() + slack
                && inner.top() >= outer.top() - slack
                && inner.bottom() <= outer.bottom() + slack
        };
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        open_selected_stash(&app, cx).await;
        let stashes = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");

        // Tab leaves focus on the saved files.
        let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
        let on_files =
            |cx: &mut gpui::VisualTestContext| cx.update(|window, _| files.is_focused(window));
        for _ in 0..12 {
            cx.simulate_keystrokes("tab");
            crate::views::tests::draw(cx);
            if on_files(cx) {
                break;
            }
        }
        assert!(on_files(cx), "Tab reaches the saved files");
        // The detail scrolled to show Review restore… at its foot, with the
        // note below its fold.
        let scroll = stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
        let room = cx.read(appearance::button_ring_room);
        let [restore, view] =
            ["restore-saved-stash", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
        let offset = scroll.offset();
        scroll.set_offset(point(
            offset.x,
            (offset.y + view.bottom() - room - restore.bottom())
                .clamp(-scroll.max_offset().y, px(0.)),
        ));
        crate::views::tests::draw(cx);
        let [restore, note, view] = ["restore-saved-stash", "stash-restore-note", "stash-detail"]
            .map(|name| cx.debug_bounds(name).expect(name));
        assert!(
            inside(restore, view) && note.top() >= view.bottom() - slack,
            "Review restore… {restore:?} lies in the detail's view {view:?}, the note {note:?} below it"
        );
        let kept = scroll.offset();

        // A click on Review restore… leaves focus on the saved files.
        cx.simulate_click(restore.center(), Modifiers::none());
        assert!(on_files(cx), "the click leaves focus on the saved files");
        recovery_lands(&app, cx);
        quiet_reads_land(&app, cx);
        crate::views::tests::draw(cx);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
            Some(PAUSED_REFUSAL.to_owned()),
            "the click is refused"
        );
        let [note, restore, view] = [
            "stash-restore-refusal",
            "restore-saved-stash",
            "stash-detail",
        ]
        .map(|name| cx.debug_bounds(name).expect(name));
        assert!(
            inside(note, view) && inside(restore, view),
            "Review restore… {restore:?} and the refusal {note:?} are revealed in {view:?}, \
             scrolled from {kept:?} to {:?}",
            scroll.offset()
        );
        assert!(on_files(cx), "the saved files keep focus");
        git(&["merge", "--abort"]);
        drop(stashes);
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        drop(fixture);
    }

    /// Stacked at 18 pt in a 461 × 490 window, where the detail is about
    /// 60 px tall, the answer to a press of Review restore… leaves the
    /// control the keyboard reached while Git prepared it in view. With a
    /// merge paused and the window's operations held, so Git has not
    /// answered, a click on Review restore…, focus on the saved-work list,
    /// then Tab onto the saved files, which the detail reveals whole: once
    /// the refusal lands, the detail stays where it is, the saved files
    /// focused and whole in its view, and the note below its fold, out of
    /// AccessKit's reach, is said once by the window's alert, through
    /// AccessKit's filter and, on Linux, the AT-SPI adapter; scrolled to
    /// the note, the detail brings it into reach, a status no one hears
    /// again. Pressed with Space instead, then Shift+Tab back onto the
    /// saved files: the same. The press's answer scrolled the detail to
    /// Review restore… and the note wherever focus lay, taking the saved
    /// files out of view; then, left where it was, the note was an alert
    /// that AccessKit's filter left out below the fold, heard by no one.
    #[gpui::test]
    async fn a_late_restore_answer_leaves_the_control_tabbed_to_in_view(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = two_stashes(&repository);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - slack
                && inner.right() <= outer.right() + slack
                && inner.top() >= outer.top() - slack
                && inner.bottom() <= outer.bottom() + slack
        };
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        open_selected_stash(&app, cx).await;
        let stashes = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");
        let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
        let list = stashes.read_with(cx, |stashes, _| stashes.stash_focus.clone());
        let on_files =
            |cx: &mut gpui::VisualTestContext| cx.update(|window, _| files.is_focused(window));
        let scroll = stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
        let files_in_view = |cx: &mut gpui::VisualTestContext| {
            let [files, view] =
                ["stash-files", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
            (inside(files, view), files, view)
        };
        // Moves focus onto the saved files with `key`, which the detail
        // reveals whole.
        let move_to_files = |key: &str, cx: &mut gpui::VisualTestContext| {
            for _ in 0..12 {
                cx.simulate_keystrokes(key);
                crate::views::tests::draw(cx);
                if on_files(cx) {
                    break;
                }
            }
            assert!(on_files(cx), "{key} reaches the saved files");
            let (shown, files_at, view) = files_in_view(cx);
            assert!(
                shown,
                "the saved files {files_at:?} are revealed whole in {view:?}"
            );
        };

        for press in ["click", "space"] {
            let held = if press == "click" {
                // Focus on the saved-work list, outside the detail, and
                // Review restore… scrolled into the detail's view.
                assert!(
                    cx.update(|window, _| list.is_focused(window)),
                    "the saved-work list holds focus"
                );
                let [restore, view] = ["restore-saved-stash", "stash-detail"]
                    .map(|name| cx.debug_bounds(name).expect(name));
                let room = cx.read(appearance::button_ring_room);
                let offset = scroll.offset();
                scroll.set_offset(point(
                    offset.x,
                    (offset.y + view.bottom() - room - restore.bottom())
                        .clamp(-scroll.max_offset().y, px(0.)),
                ));
                crate::views::tests::draw(cx);
                let restore = cx
                    .debug_bounds("restore-saved-stash")
                    .expect("Review restore…");
                let held = app.update(cx, |app, _| app.operations.hold());
                cx.simulate_click(restore.center(), Modifiers::none());
                held
            } else {
                tab_to(&stashes, "restore-saved-stash", cx);
                let held = app.update(cx, |app, _| app.operations.hold());
                space(cx);
                held
            };
            crate::views::tests::draw(cx);
            assert!(
                stashes.read_with(cx, |stashes, cx| stashes.preparing(cx)),
                "{press}: Git prepares the restoration"
            );
            // The keyboard moves on to the saved files while Git prepares.
            move_to_files(if press == "click" { "tab" } else { "shift-tab" }, cx);
            let kept = scroll.offset();
            let mut heard = Heard::start(cx);

            drop(held);
            recovery_lands(&app, cx);
            quiet_reads_land(&app, cx);
            crate::views::tests::draw(cx);
            assert_eq!(
                stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
                Some(PAUSED_REFUSAL.to_owned()),
                "{press}: the press is refused"
            );
            let (shown, files_at, view) = files_in_view(cx);
            assert!(
                on_files(cx) && shown && scroll.offset() == kept,
                "{press}: the saved files {files_at:?} keep focus, whole in {view:?}, \
                 the detail kept at {kept:?}, not {:?}",
                scroll.offset()
            );
            // Below the fold, the note is out of reach; the window's alert
            // says it, once, as it lands.
            let found = reached(cx);
            assert!(
                !found
                    .iter()
                    .any(|node| node.clip.is_some() && node.name.as_deref() == Some(PAUSED_REFUSAL)),
                "{press}: the note below the fold is out of reach: {found:?}"
            );
            assert_eq!(
                said_from_the_window(&found, PAUSED_REFUSAL, gpui::accesskit::Live::Assertive),
                1,
                "{press}: the window's alert says the refusal, in reach: {found:?}"
            );
            assert_eq!(
                crate::native_accessibility::written::announced_from("stash-restore-"),
                Vec::new(),
                "{press}: the note is no live region of its own"
            );
            if let Some(heard) = heard.next(cx) {
                assert_eq!(
                    heard.iter().filter(|said| *said == PAUSED_REFUSAL).count(),
                    1,
                    "{press}: AT-SPI announces the refusal once as it lands: {heard:?}"
                );
            }
            // Scrolled to the note, it is in reach, read where it lies, and
            // nothing says it again.
            scroll.set_offset(point(kept.x, -scroll.max_offset().y));
            let found = reached(cx);
            assert!(
                found.iter().any(|node| node.role == Role::Status
                    && node.clip.is_some()
                    && node.live == gpui::accesskit::Live::Off
                    && node.name.as_deref() == Some(PAUSED_REFUSAL)),
                "{press}: the note scrolled into view is a status in reach: {found:?}"
            );
            if let Some(heard) = heard.next(cx) {
                assert!(
                    !heard.iter().any(|said| said == PAUSED_REFUSAL),
                    "{press}: AT-SPI says the refusal once, not again on scroll: {heard:?}"
                );
            }
            scroll.set_offset(kept);
            crate::views::tests::draw(cx);
        }
        git(&["merge", "--abort"]);
        drop(stashes);
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        drop(fixture);
    }

    /// The reveal of a press's answer expires with the frame that draws it.
    /// At 18 pt in a 1480 × 980 window, Stashes beside the list, the saved
    /// files focused, a click on Review restore… while a merge pauses is
    /// refused in the footer, always in view. The window narrowed to 461 ×
    /// 490 afterwards, the detail stacked beneath the list is not scrolled
    /// to the refusal; Tab and Shift+Tab back onto the saved files reveal
    /// them whole, and once a read finds the merge ended the note says so
    /// with the saved files still focused and whole in view.
    ///
    /// A refactor guard, not a regression test: the code before Stashes
    /// took the reveal as it renders passed it too, since a frame beside
    /// the list cleared the reveal and every stacked frame's footer spent
    /// it, so no frame it could reach left one due. It holds that take
    /// (`NoteReveal::due_in`) to its promise for every frame that draws
    /// Stashes: a press's reveal kept past the frame beside the list would
    /// scroll the first stacked frame to Review restore… and the note,
    /// whatever focus had done since, and win over any end of the pause
    /// before that frame.
    #[gpui::test]
    async fn a_press_answer_beside_the_list_expires_with_its_frame(cx: &mut gpui::TestAppContext) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = two_stashes(&repository);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(1480.), px(980.)));
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - slack
                && inner.right() <= outer.right() + slack
                && inner.top() >= outer.top() - slack
                && inner.bottom() <= outer.bottom() + slack
        };
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        open_selected_stash(&app, cx).await;
        let stashes = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");
        let files = stashes.read_with(cx, |stashes, _| stashes.file_focus.clone());
        let on_files =
            |cx: &mut gpui::VisualTestContext| cx.update(|window, _| files.is_focused(window));
        // Focus on the saved files, as a click on one moves it.
        cx.update(|window, cx| window.focus(&files, cx));
        crate::views::tests::draw(cx);

        // Beside the list, a click on Review restore… in the footer, which
        // leaves focus on the saved files, is refused there.
        let restore = cx
            .debug_bounds("restore-saved-stash")
            .expect("Review restore…");
        cx.simulate_click(restore.center(), Modifiers::none());
        recovery_lands(&app, cx);
        quiet_reads_land(&app, cx);
        crate::views::tests::draw(cx);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
            Some(PAUSED_REFUSAL.to_owned()),
            "the click is refused"
        );
        assert!(
            cx.debug_bounds("stash-restore-refusal").is_some() && on_files(cx),
            "the refusal is in the footer, the saved files focused"
        );

        // Narrowed, the detail stacks beneath the list and is not scrolled
        // to the refusal answered beside it.
        cx.simulate_resize(size(px(461.), px(490.)));
        for _ in 0..3 {
            crate::views::tests::draw(cx);
        }
        let scroll = stashes.read_with(cx, |stashes, _| stashes.detail.scroll().clone());
        let [note, view] = ["stash-restore-refusal", "stash-detail"]
            .map(|name| cx.debug_bounds(name).expect(name));
        assert!(
            on_files(cx) && !inside(note, view),
            "stacked, the saved files keep focus and the refusal {note:?} is not revealed \
             in {view:?} at {:?}",
            scroll.offset()
        );
        // The keyboard moves off the saved files and back, which the detail
        // reveals whole.
        for key in ["tab", "shift-tab"] {
            cx.simulate_keystrokes(key);
            crate::views::tests::draw(cx);
        }
        let files_in_view = |cx: &mut gpui::VisualTestContext| {
            let [files, view] =
                ["stash-files", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
            (inside(files, view), files, view)
        };
        let (shown, files_at, view) = files_in_view(cx);
        assert!(
            on_files(cx) && shown,
            "the saved files {files_at:?} are focused, whole in {view:?}"
        );
        let kept = scroll.offset();

        // The merge ends in a terminal; the next read says so in the note.
        git(&["merge", "--abort"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        crate::views::tests::draw(cx);
        assert!(
            stashes.read_with(cx, |stashes, _| stashes.pause_ended)
                && cx.debug_bounds("stash-restore-ended").is_some(),
            "the note says the merge ended"
        );
        let (shown, files_at, view) = files_in_view(cx);
        assert!(
            on_files(cx) && shown && scroll.offset() == kept,
            "the saved files {files_at:?} keep focus, whole in {view:?}, the detail kept at \
             {kept:?}, not {:?}",
            scroll.offset()
        );
        drop(stashes);
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        drop(fixture);
    }

    /// While Git prepares Review restore…'s restoration, Review restore…
    /// says so in place, at 13 pt in a 1480 × 980 window and at 18 pt in a
    /// 461 × 490 one, where it lies in the stacked detail: pressed from the
    /// keyboard, its name says it is preparing at once, and once the
    /// preparation looks busy it reads "Preparing…", keeping focus, whole
    /// with its ring, and its width, so Drop… and the footer stay where they
    /// were. Once Git answers, the review opens in Stashes' place. At rest
    /// its label is drawn as Drop…'s beside it, as large and as heavy, in
    /// the primary label colour. Nothing in Stashes said Git was preparing:
    /// the window's progress label that said so lay beneath the dialog's
    /// backdrop; and the label took the kit's small size, 11.375 px beside
    /// Drop…'s 12 px at 13 pt.
    #[gpui::test]
    async fn review_restore_says_it_is_preparing_in_place(cx: &mut gpui::TestAppContext) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        // The app's palette, whose accent draws the keyboard ring.
        crate::focus_ring::sweep::prepare(&app, cx);
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let _git = two_stashes(&repository);
        // Review restore…'s accessibility node as the window would write it
        // now, built inside a draw as `tags::tests` builds Review tag's.
        let name = |stashes: &Entity<StashBrowser>, cx: &mut gpui::VisualTestContext| {
            let stashes = stashes.clone();
            crate::views::tests::view_node(cx, move |window, cx| {
                use gpui::Element as _;
                let detail = stashes.read(cx).detail.clone();
                let focus = detail.focus("restore-saved-stash", cx);
                let button = stashes.read(cx).review_restore(&focus, cx);
                let button = button.base(window, cx);
                let element = RenderOnce::render(button, window, cx).into_element();
                let mut node = gpui::accesskit::Node::new(
                    element.a11y_role().expect("Review restore…'s role"),
                );
                element.write_a11y_info(&mut node);
                node
            })
            .label()
            .map(str::to_owned)
        };
        let footer = |cx: &mut gpui::VisualTestContext| {
            ["restore-saved-stash", "drop-saved-stash", "stash-actions"]
                .map(|name| cx.debug_bounds(name).unwrap_or_else(|| panic!("no {name}")))
        };
        let label = |cx: &mut gpui::VisualTestContext| match (
            cx.debug_bounds("restore-saved-stash/rest").is_some(),
            cx.debug_bounds("restore-saved-stash/preparing").is_some(),
        ) {
            (true, false) => "Review restore…",
            (false, true) => RESTORE_PREPARING,
            drawn => panic!("Review restore… draws one label, not {drawn:?}"),
        };
        for (width, height, interface) in [(1480., 980., 13), (461., 490., 18)] {
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            open_selected_stash(&app, cx).await;
            let stashes = app
                .read_with(cx, |app, _| app.recovery.browser.clone())
                .and_then(|browser| browser.upgrade())
                .expect("Stashes is open");
            tab_to(&stashes, "restore-saved-stash", cx);
            let at_rest = footer(cx);
            assert_eq!(label(cx), "Review restore…", "{at}");
            assert_eq!(
                name(&stashes, cx).as_deref(),
                Some("Review restore…"),
                "{at}"
            );
            // At rest its label is drawn as Drop…'s beside it, the compact
            // helper's: as large and as heavy, in the kit's primary label
            // colour.
            let text = |key: &str| {
                (
                    appearance::drawn_size(key),
                    appearance::drawn_weight(key),
                    appearance::drawn_color(key),
                )
            };
            let (size, weight, color) = text("restore-saved-stash/rest/text");
            let (drop_size, drop_weight, _) = text("drop-saved-stash/text");
            assert!(
                size.is_some() && size == drop_size,
                "{at}: Review restore…'s label, {size:?}, is as large as Drop…'s, {drop_size:?}"
            );
            assert!(
                weight.is_some() && weight == drop_weight,
                "{at}: Review restore…'s label, {weight:?}, is as heavy as Drop…'s, {drop_weight:?}"
            );
            let primary =
                cx.read(|cx| gpui_kit::component::Theme::global(cx).button_primary_foreground);
            assert_eq!(
                color,
                Some(primary),
                "{at}: Review restore…'s label is in the primary label colour"
            );

            let held = app.update(cx, |app, _| app.operations.hold());
            space(cx);
            crate::views::tests::draw(cx);
            assert!(
                stashes.read_with(cx, |stashes, cx| stashes.preparing(cx)),
                "{at}: Git prepares the restoration"
            );
            // Heard at once; the look waits as the busy look does.
            assert_eq!(
                name(&stashes, cx).as_deref(),
                Some(RESTORE_PREPARING_NAME),
                "{at}: its name says it is preparing at once"
            );
            assert_eq!(label(cx), "Review restore…", "{at}: the look waits");
            cx.executor()
                .advance_clock(crate::workspace::BUSY_LOOK_DELAY);
            crate::views::tests::draw(cx);
            assert_eq!(
                label(cx),
                RESTORE_PREPARING,
                "{at}: it reads Preparing… once the preparation looks busy"
            );
            let preparing = footer(cx);
            for ((before, now), name) in
                at_rest
                    .iter()
                    .zip(&preparing)
                    .zip(["Review restore…", "Drop…", "the footer"])
            {
                assert_eq!(before, now, "{at}: {name} keeps its place and width");
            }
            assert_eq!(
                cx.update(|window, cx| stashes.read(cx).detail.focused(window)),
                Some("restore-saved-stash".into()),
                "{at}: Review restore… keeps focus"
            );
            let ring = ring_around(cx, preparing[0])
                .unwrap_or_else(|| panic!("{at}: Review restore… draws no ring"));
            assert!(ring.whole, "{at}: Review restore…'s ring {ring:?} is whole");

            drop(held);
            recovery_lands(&app, cx);
            assert!(
                cx.debug_bounds("operation-consequences").is_some()
                    && cx.debug_bounds("stash-browser").is_none(),
                "{at}: the review opens in place of Stashes"
            );
            drop(stashes);
            cx.update(|window, cx| window.close_all_dialogs(cx));
            crate::views::tests::draw(cx);
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        drop(fixture);
    }

    /// Stacked at 18 pt in a 461 × 490 window, where the detail is about
    /// 60 px tall, a stashed document's open control is one of the detail's
    /// controls. Review restore… refused while a merge pauses, Shift+Tab
    /// onto After's Open in default app scrolls it into the detail's view
    /// with its ring, and once a read finds the merge ended the note says
    /// so and the detail stays where it is, the control focused and whole
    /// in view. Unknown to the detail, the control was not revealed, and the
    /// note's reveal, which scrolls only while nothing else in the detail
    /// holds focus, found none there and scrolled it away.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn a_focused_open_control_stays_in_view_as_a_pause_ends(cx: &mut gpui::TestAppContext) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        // The app's palette, whose accent draws the keyboard ring.
        crate::focus_ring::sweep::prepare(&app, cx);
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let git = two_stashes(&repository);
        // The newest stash saves a document's second version.
        let pdf = |text: &str| {
            format!("%PDF-1.4\n% {text}\n1 0 obj\n<<>>\nendobj\ntrailer\n<<>>\n%%EOF\n")
        };
        let document = repository.join("document.pdf");
        std::fs::write(&document, pdf("first")).unwrap();
        git(&["add", "document.pdf"]);
        git(&["commit", "-q", "-m", "Add a document"]);
        std::fs::write(&document, pdf("second")).unwrap();
        git(&["stash", "push", "-q", "-m", "Document"]);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        let slack = px(0.5);
        let inside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - slack
                && inner.right() <= outer.right() + slack
                && inner.top() >= outer.top() - slack
                && inner.bottom() <= outer.bottom() + slack
        };
        let landed = |cx: &mut gpui::VisualTestContext, selector: &'static str| {
            for _ in 0..400 {
                cx.executor()
                    .advance_clock(std::time::Duration::from_millis(50));
                crate::views::tests::draw(cx);
                if cx.debug_bounds(selector).is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("{selector} never drawn");
        };

        // Refused while the merge pauses, unread, which the refusal has the
        // window read, with the document chosen.
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        open_selected_stash(&app, cx).await;
        let stashes = app
            .read_with(cx, |app, _| app.recovery.browser.clone())
            .and_then(|browser| browser.upgrade())
            .expect("Stashes is open");
        cx.update(|window, cx| {
            stashes.update(cx, |stashes, cx| stashes.select_file(0, window, cx))
        });
        landed(cx, "system-preview-1");
        press_review_restore(&stashes, cx);
        recovery_lands(&app, cx);
        quiet_reads_land(&app, cx);
        assert_eq!(
            stashes.read_with(cx, |stashes, _| stashes.restore_line.clone()),
            Some(PAUSED_REFUSAL.to_owned())
        );
        assert!(app.read_with(cx, |app, _| app.integration_state.is_some()));

        // Shift+Tab back onto After's open control, which scrolls into the
        // detail's view with its ring.
        let detail = stashes.read_with(cx, |stashes, _| stashes.detail.clone());
        let open = cx.update(|_, cx| detail.focus(("system-preview", 1_usize), cx));
        let mut reached = false;
        for _ in 0..12 {
            cx.simulate_keystrokes("shift-tab");
            crate::views::tests::draw(cx);
            if cx.update(|window, _| open.is_focused(window)) {
                reached = true;
                break;
            }
        }
        assert!(reached, "Shift+Tab reaches After's open control");
        crate::views::tests::draw(cx);
        let room = cx.read(appearance::button_ring_room);
        let in_view = |cx: &mut gpui::VisualTestContext| {
            let [control, view] =
                ["system-preview-1", "stash-detail"].map(|name| cx.debug_bounds(name).expect(name));
            (inside(control.dilate(room), view), control, view)
        };
        let (shown, control, view) = in_view(cx);
        assert!(
            shown,
            "the open control {control:?} and its ring lie in the detail's view {view:?}"
        );
        let ring = ring_around(cx, control)
            .unwrap_or_else(|| panic!("the open control {control:?} draws no ring"));
        assert!(ring.whole, "its ring {ring:?} is whole");
        let kept = detail.scroll().offset();

        // The merge ends in a terminal; the next read says so in the note.
        git(&["merge", "--abort"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        quiet_reads_land(&app, cx);
        crate::views::tests::draw(cx);
        assert!(
            stashes.read_with(cx, |stashes, _| stashes.restore_line.is_none()
                && stashes.pause_ended)
                && cx.debug_bounds("stash-restore-ended").is_some(),
            "the note says the merge ended"
        );
        assert_eq!(
            detail.scroll().offset(),
            kept,
            "the detail stays where the keyboard left it"
        );
        let (shown, control, view) = in_view(cx);
        assert!(
            cx.update(|window, _| open.is_focused(window)) && shown,
            "the open control {control:?} keeps focus in the detail's view {view:?}"
        );
        drop(stashes);
        cx.update(|window, cx| window.close_all_dialogs(cx));
        crate::views::tests::draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        drop(fixture);
    }

    /// A recovery action refuses while an integration is paused from the
    /// pause the window read last, which a change outside the app can end
    /// unseen, as it does here: the test's local watch reports nothing,
    /// like one whose read has not landed yet, or a retained tab's restored
    /// pause. Review stash in an open Save changes to stash… form is
    /// refused in the core's words and asks for a quiet read of the working
    /// state; once it lands and finds the merge ended, the form's line says
    /// so instead of refusing. Revert, as a keyboard's commit menu offers
    /// it, is refused in the window's error and asks the same, and that
    /// error ends with the merge. Nothing read the pause again: each try was
    /// refused from the same stale pause until some other read landed, and
    /// the form went on refusing after it.
    #[gpui::test]
    async fn a_paused_refusal_reads_the_pause_again_and_gives_way_once_it_ends(
        cx: &mut gpui::TestAppContext,
    ) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let identity = [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ];
        let git = |arguments: &[&str]| {
            crate::views::tests::fixture_git(&repository, &[&identity[..], arguments].concat())
        };
        git(&["reset", "-q", "--hard"]);
        git(&["checkout", "-q", "-b", "side"]);
        std::fs::write(repository.join("side.txt"), "side\n").unwrap();
        git(&["add", "side.txt"]);
        git(&["commit", "-q", "-m", "side"]);
        git(&["checkout", "-q", "main"]);
        let paused = |cx: &mut gpui::VisualTestContext| {
            app.read_with(cx, |app, _| app.integration_state.is_some())
        };
        let asked = |cx: &mut gpui::VisualTestContext| {
            app.read_with(cx, |app, _| app.automatic.quiet_read_requested())
        };

        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        read_pause(&app, cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_stash_form(window, cx)));
        crate::views::tests::draw(cx);
        let form = app.read_with(cx, |app, _| app.recovery.stash.clone().expect("the form"));
        cx.update(|window, cx| {
            form.update(cx, |form, cx| {
                form.name
                    .update(cx, |name, cx| name.set_value("paused work", window, cx))
            })
        });
        // The merge ends in a terminal, unseen.
        git(&["merge", "--abort"]);
        // Review stash, as its button asks, with nothing run after it.
        cx.update(|window, cx| {
            window.dispatch_action(
                Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
                cx,
            )
        });
        frame(cx);
        assert_eq!(
            form.read_with(cx, |form, _| form.error.clone()),
            Some(PAUSED_REFUSAL.into()),
            "Review stash is refused from the pause read last"
        );
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "the form reopens"
        );
        assert!(
            cx.debug_bounds("recovery-form-error").is_some(),
            "the form draws the refusal"
        );
        assert!(asked(cx), "the refusal asks for a quiet read");
        assert!(paused(cx), "the read has not landed");

        quiet_reads_land(&app, cx);
        frame(cx);
        assert!(!paused(cx), "the read finds the merge ended");
        assert_eq!(
            form.read_with(cx, |form, _| (form.error.clone(), form.pause_ended)),
            (None, true),
            "the refusal gives way"
        );
        assert!(
            cx.debug_bounds("recovery-form-error").is_none(),
            "the form draws no refusal"
        );
        assert!(
            cx.debug_bounds("recovery-form-note").is_some(),
            "the form says the merge ended"
        );
        assert_eq!(
            crate::native_accessibility::written::announced("recovery-form-note")
                .map(|announced| announced.live),
            Some(gpui::accesskit::Live::Polite),
            "a status, heard once speech pauses"
        );
        cx.update(|window, cx| window.close_dialog(cx));
        crate::views::tests::draw(cx);

        // Revert, as Enter on a keyboard's commit menu chooses it.
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        read_pause(&app, cx).await;
        git(&["merge", "--abort"]);
        let commit = app.read_with(cx, |app, _| commit_target(&app.commits[0]));
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.choose_commit_recovery(RecoveryKind::Revert, commit, window, cx)
            })
        });
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error.clone()),
            Some(PAUSED_REFUSAL.into()),
            "Revert is refused in the window's error"
        );
        assert!(asked(cx), "the refusal asks for a quiet read");
        quiet_reads_land(&app, cx);
        assert!(!paused(cx), "the read finds the merge ended again");
        assert_eq!(
            app.read_with(cx, |app, _| app.operation_error.clone()),
            None,
            "the window's refusal ends with the merge"
        );
        drop(fixture);
    }

    /// The recovery forms a paused integration refuses.
    #[derive(Clone, Copy, Debug)]
    enum PausedForm {
        /// Amend last commit…, its title edited.
        Amend,
        /// Save changes to stash…, named.
        Stash,
    }

    /// What `form` holds: why its review was refused, if it was, and the
    /// field it was given.
    fn paused_form(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
        form: PausedForm,
    ) -> (Option<String>, String) {
        app.read_with(cx, |app, cx| match form {
            PausedForm::Amend => {
                let form = app
                    .recovery
                    .amend
                    .as_ref()
                    .expect("the amend form")
                    .read(cx);
                (form.error.clone(), form.title.read(cx).value().to_string())
            }
            PausedForm::Stash => {
                let form = app
                    .recovery
                    .stash
                    .as_ref()
                    .expect("the stash form")
                    .read(cx);
                (form.error.clone(), form.name.read(cx).value().to_string())
            }
        })
    }

    /// Opens `form` at rest and fills its field with `value`, then pauses a
    /// merge outside the app and reads it, as the local watcher would, and
    /// asks for the form's review, as its button does: the review is
    /// refused in the core's words without reading a plan, and the form
    /// opens again with that refusal beneath its fields and the value kept,
    /// asking for a quiet read of the pause. The refusal is announced at
    /// once, and asked for again while still paused, the form opens again
    /// with it already there and announces it again by a fresh node.
    async fn reopened_refused_while_paused(cx: &mut gpui::TestAppContext, form: PausedForm) {
        let (fixture, app, cx) =
            crate::views::tests::history_window_from(cx, crate::views::tests::history_script(3, 3))
                .await;
        cx.update(|_, cx| cx.set_reduce_motion(true));
        let repository = app.read_with(cx, |app, _| app.path.clone().expect("a repository"));
        let identity = [
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ];
        let git = |arguments: &[&str]| {
            crate::views::tests::fixture_git(&repository, &[&identity[..], arguments].concat())
        };
        git(&["reset", "-q", "--hard"]);
        git(&["checkout", "-q", "-b", "side"]);
        std::fs::write(repository.join("side.txt"), "side\n").unwrap();
        git(&["add", "side.txt"]);
        git(&["commit", "-q", "-m", "side"]);
        git(&["checkout", "-q", "main"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        crate::views::tests::settle(&app, cx).await;
        let dialog_open =
            |cx: &mut gpui::VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));

        // Opened at rest, its field filled.
        let value = match form {
            PausedForm::Amend => {
                let head = app.read_with(cx, |app, _| {
                    app.work_status
                        .as_ref()
                        .and_then(|status| status.head.clone())
                        .expect("HEAD")
                });
                cx.update(|window, cx| {
                    app.update(cx, |app, cx| {
                        app.prepare_commit_recovery(RecoveryKind::Amend, head, None, window, cx)
                    })
                });
                let read = app.update(cx, |app, _| app.recovery.task.take());
                read.expect("the amend plan is read").await;
                crate::views::tests::draw(cx);
                let form =
                    app.read_with(cx, |app, _| app.recovery.amend.clone().expect("the form"));
                cx.update(|window, cx| {
                    form.update(cx, |form, cx| {
                        form.title.update(cx, |title, cx| {
                            title.set_value("Reworded while paused", window, cx)
                        })
                    })
                });
                "Reworded while paused"
            }
            PausedForm::Stash => {
                cx.update(|window, cx| app.update(cx, |app, cx| app.open_stash_form(window, cx)));
                crate::views::tests::draw(cx);
                let form =
                    app.read_with(cx, |app, _| app.recovery.stash.clone().expect("the form"));
                cx.update(|window, cx| {
                    form.update(cx, |form, cx| {
                        form.name
                            .update(cx, |name, cx| name.set_value("paused work", window, cx))
                    })
                });
                "paused work"
            }
        };
        crate::views::tests::draw(cx);
        assert!(dialog_open(cx), "{form:?}: the form opened");
        assert_eq!(
            paused_form(&app, cx, form),
            (None, value.into()),
            "{form:?}: at rest"
        );

        // A merge pauses, and the window reads it.
        git(&["merge", "-q", "--no-commit", "--no-ff", "side"]);
        read_pause(&app, cx).await;
        assert!(
            dialog_open(cx),
            "{form:?}: the form stays open as the pause is read"
        );
        // Review, as its button asks.
        let nothing = app.update(cx, |app, _| app.operations.hold());
        cx.update(|window, cx| {
            window.dispatch_action(
                Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
                cx,
            )
        });
        frame(cx);
        assert_eq!(
            paused_form(&app, cx, form),
            (Some(PAUSED_REFUSAL.into()), value.into()),
            "{form:?}: refused in the core's words, its field kept"
        );
        assert!(dialog_open(cx), "{form:?}: the form opens again");
        assert!(
            cx.debug_bounds("recovery-form-error").is_some(),
            "{form:?}: the form draws its refusal"
        );
        assert_eq!(
            app.read_with(cx, |app, _| (
                app.operation_busy,
                app.recovery.task.is_some()
            )),
            (None, false),
            "{form:?}: no plan is read"
        );
        assert!(
            app.read_with(cx, |app, _| app.automatic.quiet_read_requested()),
            "{form:?}: the refusal asks for a quiet read of the pause"
        );
        // Announced at once, as when it first appears in an open form.
        let first = refusal_announced(form);
        quiet_reads_land(&app, cx);
        assert!(
            app.read_with(cx, |app, _| app.integration_state.is_some()),
            "{form:?}: the read finds the merge still paused"
        );

        // Review again, still paused: the form opens again with the same
        // refusal already there, announced again by a fresh node.
        cx.update(|window, cx| {
            window.dispatch_action(
                Box::new(gpui_kit::component::dialog::Confirm { secondary: false }),
                cx,
            )
        });
        frame(cx);
        assert_eq!(
            paused_form(&app, cx, form),
            (Some(PAUSED_REFUSAL.into()), value.into()),
            "{form:?}: refused again"
        );
        assert!(dialog_open(cx), "{form:?}: the form opens again once more");
        let second = refusal_announced(form);
        assert_ne!(
            first, second,
            "{form:?}: the refusal it opens with is a fresh node, announced again"
        );
        drop(nothing);
        quiet_reads_land(&app, cx);
        drop(fixture);
    }

    /// The node that announces the open recovery form's refusal: one, at
    /// once.
    fn refusal_announced(form: PausedForm) -> String {
        let mut announced =
            crate::native_accessibility::written::announced_from("recovery-form-error-");
        assert_eq!(
            announced.len(),
            1,
            "{form:?}: one refusal announced: {announced:?}"
        );
        let (node, how) = announced.pop().expect("the refusal");
        assert_eq!(
            how,
            crate::native_accessibility::written::Announced::ASSERTIVE,
            "{form:?}: the refusal is announced at once"
        );
        // Its name, which AT-SPI announces, and its value, which macOS does.
        let written = crate::native_accessibility::written::get(&node).expect("the refusal's node");
        assert_eq!(
            (written.node.label(), written.node.value()),
            (Some(PAUSED_REFUSAL), Some(PAUSED_REFUSAL)),
            "{form:?}: the refusal is its node's name and value"
        );
        node
    }

    /// Amend last commit…, opened at rest, is refused while a merge is
    /// paused and opens again with the refusal and the reworded title.
    #[gpui::test]
    async fn the_amend_form_reopens_refused_while_paused(cx: &mut gpui::TestAppContext) {
        reopened_refused_while_paused(cx, PausedForm::Amend).await;
    }

    /// Save changes to stash…, opened at rest, is refused while a merge is
    /// paused and opens again with the refusal and the stash's name.
    #[gpui::test]
    async fn the_stash_form_reopens_refused_while_paused(cx: &mut gpui::TestAppContext) {
        reopened_refused_while_paused(cx, PausedForm::Stash).await;
    }
}
